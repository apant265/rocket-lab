//! Preliminary geometry-dependent aero for builder designs. This is a low-Mach
//! small-angle estimate with a synthetic Mach drag envelope, not RASAero parity.
use crate::config::{AeroPoint, Mission, NoseShape, StageGeometry};
pub fn nose_radius_fraction(shape: NoseShape, fraction: f64, slenderness: f64) -> f64 {
    let x = fraction.clamp(0.0, 1.0);
    match shape {
        NoseShape::Conical => x,
        NoseShape::Elliptical => (1.0 - (1.0 - x).powi(2)).sqrt(),
        NoseShape::TangentOgive => {
            let rho = (1.0 + slenderness * slenderness) * 0.5;
            ((rho * rho - (slenderness * (1.0 - x)).powi(2))
                .max(0.0)
                .sqrt()
                + 1.0
                - rho)
                .clamp(0.0, 1.0)
        }
    }
}
pub fn estimate_aero(m: &mut Mission) {
    let top = m.stages.len() - 1;
    let geometries: Vec<_> = m
        .stages
        .iter()
        .enumerate()
        .map(|(i, s)| {
            s.geometry
                .clone()
                .unwrap_or_else(|| StageGeometry::generic(s, i == top))
        })
        .collect();
    for active in 0..m.stages.len() {
        let diameter = m.stages[active].diameter_m;
        let area = std::f64::consts::PI * diameter * diameter * 0.25;
        let nose = &geometries[top];
        let top_radius = m.stages[top].diameter_m * 0.5;
        let volume_fraction = (0..400)
            .map(|k| {
                let f = (k as f64 + 0.5) / 400.0;
                nose_radius_fraction(nose.nose_shape, f, nose.nose_length_m / top_radius).powi(2)
                    / 400.0
            })
            .sum::<f64>();
        let cn_nose = 2.0 * (m.stages[top].diameter_m / diameter).powi(2);
        let mut cn = cn_nose;
        let mut moment = cn_nose * nose.nose_length_m * (1.0 - volume_fraction);
        let mut offset = 0.0;
        let mut wetted = 0.0;
        for i in (active..=top).rev() {
            let s = &m.stages[i];
            let g = &geometries[i];
            let component_shell = s.materials.as_ref().is_some_and(|m| m.component_shell);
            let mut sets = Vec::new();
            if !component_shell {
                sets.push(g.clone());
            }
            for p in s.components.iter().filter(|p| p.kind.planar_fin()) {
                if let Some(shape) = &p.shape {
                    let mut shape = shape.clone();
                    shape.fin_position_m = p.position_m;
                    if p.kind != crate::assembly::PartKind::FinSet {
                        // Equivalent trapezoid for the existing preliminary
                        // Barrowman estimate; the rendered/mass outline is exact.
                        shape.fin_tip_chord_m = (2.0 * p.fin_area_cg().0 / shape.fin_span_m
                            - shape.fin_root_chord_m)
                            .max(0.001);
                    }
                    sets.push(shape);
                }
            }
            for g in &sets {
                let span = g.fin_span_m;
                let chord = g.fin_root_chord_m + g.fin_tip_chord_m;
                let mid =
                    span.hypot(g.fin_sweep_m + (g.fin_tip_chord_m - g.fin_root_chord_m) * 0.5);
                let interference = 1.0 + (s.diameter_m * 0.5) / (span + s.diameter_m * 0.5);
                let fin_cn = interference * 4.0 * g.fin_count as f64 * (span / diameter).powi(2)
                    / (1.0 + (1.0 + (2.0 * mid / chord).powi(2)).sqrt());
                let fin_cp = offset
                    + g.fin_position_m
                    + g.fin_sweep_m * (g.fin_root_chord_m + 2.0 * g.fin_tip_chord_m)
                        / (3.0 * chord)
                    + (chord - g.fin_root_chord_m * g.fin_tip_chord_m / chord) / 6.0;
                cn += fin_cn;
                moment += fin_cn * fin_cp;
            }
            wetted += std::f64::consts::PI * s.diameter_m * s.length_m
                + sets
                    .iter()
                    .map(|g| {
                        g.fin_count as f64 * g.fin_span_m * (g.fin_root_chord_m + g.fin_tip_chord_m)
                    })
                    .sum::<f64>();
            if i < top {
                let step_cn = 2.0 * (s.diameter_m.powi(2) - m.stages[i + 1].diameter_m.powi(2))
                    / diameter.powi(2);
                cn += step_cn;
                moment += step_cn * offset;
            }
            offset += s.length_m;
        }
        let cp = (moment / cn.max(0.01)).clamp(0.0, offset);
        let finish = m.stages[active..]
            .iter()
            .flat_map(|s| s.components.iter())
            .filter(|p| p.external())
            .map(|p| match p.finish.as_str() {
                "Rough" => 1.5,
                "Smooth" => 0.8,
                "Polished" => 0.65,
                _ => 1.0,
            })
            .collect::<Vec<_>>();
        let factor = if finish.is_empty() {
            1.0
        } else {
            finish.iter().sum::<f64>() / finish.len() as f64
        };
        let explicit_cd = m.stages[active..]
            .iter()
            .flat_map(|s| s.components.iter())
            .filter(|p| p.external())
            .filter_map(|p| {
                p.cd_override
                    .map(|cd| cd * (p.diameter_m / diameter).powi(2) * p.instance_count as f64)
            })
            .sum::<f64>();
        let friction = 0.004 * wetted / area * factor;
        let angle = (top_radius / nose.nose_length_m.max(0.01)).atan();
        let sin = angle.sin();
        m.stages[active].aero = [0.0, 0.5, 0.8, 1.0, 1.15, 1.5, 2.0, 3.0, 5.0, 8.0, 12.0]
            .into_iter()
            .map(|mach| {
                let wave = if mach < 0.8 {
                    0.03 * sin
                } else if mach <= 1.15 {
                    0.08 + 0.35 * (mach - 0.8) / 0.35 + sin
                } else {
                    2.1 * sin * sin + 0.5 * sin / (mach * mach - 1.0_f64).sqrt()
                };
                let cd = friction + 0.12 + wave + explicit_cd;
                AeroPoint {
                    mach,
                    cd_power_off: cd,
                    cd_power_on: (cd - 0.07).max(0.04),
                    cn_alpha_per_rad: cn.max(0.1),
                    cp_m: cp,
                }
            })
            .collect();
        m.stages[active].aero_source="PRELIMINARY builder geometry estimate: low-Mach small-angle Barrowman CN/CP; synthetic drag envelope. Higher-speed CN/CP held. Import validated coefficients for quantitative predictions.".into();
    }
}

/// Resolve the same geometry and default hardware/recovery used by the builder.
pub fn initialize_geometry(m: &mut Mission) {
    let native = std::fs::read_to_string("sources/Plumbbob_MKIII_R75.CDX1").ok();
    let doc = native
        .as_deref()
        .and_then(|s| roxmltree::Document::parse(s).ok());
    let plumbbob = m.stages.len() == 3
        && (m.stages[0].diameter_m - 0.156718).abs() < 1e-5
        && (m.stages[2].length_m - 2.2987).abs() < 1e-5;
    let top = m.stages.len() - 1;
    for (i, s) in m.stages.iter_mut().enumerate() {
        if s.materials.is_none() {
            s.materials = Some(crate::assembly::Structure::for_stage(s));
        }
        if i != top && s.recovery.is_none() {
            let mut r = m.recovery.clone();
            r.main.area_m2 = (s.dry_mass_kg + s.motor.propellant_mass_kg) * 9.80665
                / (0.5 * 1.225 * r.main.cd * 6.0_f64.powi(2));
            s.recovery = Some(r);
        }
        if s.geometry.is_some() {
            continue;
        }
        let mut g = StageGeometry::generic(s, i == top);
        if plumbbob && let Some(doc) = &doc {
            let body = if i == top {
                doc.descendants().find(|n| n.has_tag_name("BodyTube"))
            } else {
                doc.descendants()
                    .filter(|n| n.has_tag_name("Booster"))
                    .nth(1 - i)
            };
            if let Some(fin) = body.and_then(|n| n.children().find(|f| f.has_tag_name("Fin"))) {
                let number = |name: &str| {
                    fin.children()
                        .find(|n| n.has_tag_name(name))
                        .and_then(|n| n.text())
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.0)
                };
                g.fin_count = number("Count") as u32;
                g.fin_root_chord_m = number("Chord") * 0.0254;
                g.fin_tip_chord_m = number("TipChord") * 0.0254;
                g.fin_span_m = number("Span") * 0.0254;
                g.fin_sweep_m = number("SweepDistance") * 0.0254;
                g.fin_thickness_m = number("Thickness") * 0.0254;
                g.fin_position_m = s.length_m - number("Location") * 0.0254;
                if i == top {
                    g.nose_length_m = 0.508;
                }
            }
        }
        s.geometry = Some(g);
    }
}
