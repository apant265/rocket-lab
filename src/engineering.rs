//! Post-flight structural and ablation screening from temperature and loads.
//! Does not fracture the rigid body or change its mass/aerodynamics in flight.
use crate::{flight::FlightResult, thermal::History};
pub fn screen(f: &FlightResult, h: &History) -> serde_json::Value {
    let mut rows = Vec::new();
    for (stage, s) in f.mission.stages.iter().enumerate() {
        let q = f
            .samples
            .iter()
            .filter(|v| v.stage <= stage)
            .map(|v| v.q_pa)
            .fold(0.0, f64::max);
        let peaks = |kind: &str| {
            h.columns
                .iter()
                .enumerate()
                .filter(|(_, c)| c.stage == stage && c.kind == kind)
                .flat_map(|(i, _)| h.temperatures.iter().map(move |t| t[i][0]))
                .fold(288.15, f64::max)
        };
        let mut parts = Vec::new();
        for law in f
            .mission
            .experiments
            .thermal
            .iter()
            .filter(|l| l.stage == stage)
        {
            let temp = peaks(&law.region);
            let mut max_stress: f64 = 0.0;
            let mut max_deflection: f64 = 0.0;
            let mut min_margin = f64::INFINITY;
            let mut first_frequency = None;
            if let Some(g) = &s.geometry
                && law.region == "fin"
            {
                let chord = (g.fin_root_chord_m + g.fin_tip_chord_m) * 0.5;
                let inertia = chord * g.fin_thickness_m.powi(3) / 12.0;
                let density = s
                    .materials
                    .as_ref()
                    .map_or(1850.0, |m| m.fins.density_kg_m3);
                first_frequency = Some(
                    1.875104_f64.powi(2) / std::f64::consts::TAU / (g.fin_span_m * g.fin_span_m)
                        * (law.youngs_modulus_pa * inertia / (density * chord * g.fin_thickness_m))
                            .sqrt(),
                );
                for v in f.samples.iter().filter(|v| v.stage <= stage) {
                    let t = h.temperature(v.time_s, stage, "fin", g.fin_position_m, false);
                    let strength = ((law.ablation_temperature_k - t)
                        / (law.ablation_temperature_k - 288.15).max(1.0))
                    .clamp(0.01, 1.0);
                    let load = v.q_pa * 1.2 * v.alpha_deg.to_radians().sin().abs() * chord; // N per span meter
                    let moment = load * g.fin_span_m.powi(2) * 0.5;
                    let stress = moment * g.fin_thickness_m * 0.5 / inertia;
                    let deflection = load * g.fin_span_m.powi(4)
                        / (8.0 * law.youngs_modulus_pa * strength * inertia);
                    max_stress = max_stress.max(stress);
                    max_deflection = max_deflection.max(deflection);
                    min_margin =
                        min_margin.min(law.allowable_stress_pa * strength / stress.max(1e-9));
                }
            }
            let mut excess = 0.0;
            for (frame, pair) in h.times.windows(2).enumerate() {
                let dt = pair[1] - pair[0];
                for (i, c) in h
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.stage == stage && c.kind == law.region)
                {
                    if h.temperatures[frame + 1][i][0] >= law.ablation_temperature_k {
                        let sample = f.samples.iter().rev().find(|s| s.time_s <= pair[1]);
                        if let Some(s) = sample {
                            let radiation = c.material.thermal.emissivity
                                * crate::thermal::SIGMA
                                * (law.ablation_temperature_k.powi(4)
                                    - crate::atmosphere::for_mission(s.altitude_m, &f.mission)
                                        .temperature
                                        .powi(4));
                            excess += (s.heat_flux_w_m2 * c.incidence.powi(2) - radiation).max(0.0)
                                * c.area_m2
                                * dt;
                        }
                    }
                }
            }
            parts.push(serde_json::json!({"region":law.region,"peak_outer_k":temp,"maximum_fin_root_stress_pa":max_stress,"maximum_fin_tip_deflection_m":max_deflection,"minimum_stress_margin":if min_margin.is_finite(){Some(min_margin)}else{None},"cold_fin_first_bending_frequency_hz":first_frequency,"potential_ablation_mass_kg_energy_bound":excess/law.heat_of_ablation_j_kg,"ablation_threshold_crossed":temp>=law.ablation_temperature_k}));
        }
        rows.push(serde_json::json!({"stage":stage+1,"maximum_attached_q_pa":q,"regions":parts,"thermal_limit_exceeded":h.columns.iter().enumerate().filter(|(_,c)|c.stage==stage).any(|(i,c)|h.temperatures.iter().any(|t|t[i][0]>c.material.thermal.limit_temperature_k))}));
    }
    serde_json::json!({"stages":rows,"requires_supplied_material_laws":true,"model":"Cantilever strip with uniform normal load; linear user-modulus softening to user ablation threshold. Ablation output is an energy bound, not removed mass. Natural frequency does not predict flutter. No laminate failure criterion, shell buckling, fin interference, fracture or feedback to rigid-body flight."})
}
