//! Explicit provisional mapping of the supplied Plumbbob CDX1, not a general native importer.
use rocket_lab::{config::*, imports};
fn main() -> Result<()> {
    let xml = std::fs::read_to_string("sources/Plumbbob_MKIII_R75.CDX1")?;
    let doc = roxmltree::Document::parse(&xml)?;
    let get = |tag: &str| -> Result<f64> {
        Ok(doc
            .descendants()
            .find(|n| n.has_tag_name(tag))
            .and_then(|n| n.text())
            .ok_or_else(|| format!("missing CDX1 {tag}"))?
            .parse()?)
    };
    let mut m = Mission::demo(true);
    m.name = "Plumbbob MKIII R75 / provisional source mapping".into();
    m.description="Actual supplied RASP curves and older CDX1 cumulative weights/CGs. SYNTHETIC aero and damping; assumed motor CGs and cylindrical inertia. No claim to reproduce the newer 168 km CSV. Nested-stage gaps/overlaps, surface finish and atmospheric overrides are not resolved.".into();
    let design = doc
        .descendants()
        .find(|n| n.has_tag_name("RocketDesign"))
        .ok_or("missing RocketDesign")?;
    let parts = design
        .children()
        .filter(|n| matches!(n.tag_name().name(), "NoseCone" | "BodyTube" | "Booster"))
        .collect::<Vec<_>>();
    let dimension = |part: roxmltree::Node<'_, '_>, tag: &str| -> Result<f64> {
        Ok(part
            .children()
            .find(|n| n.has_tag_name(tag))
            .and_then(|n| n.text())
            .ok_or("missing geometry dimension")?
            .parse::<f64>()?
            * 0.0254)
    };
    if parts.len() != 4 {
        return Err("expected the supplied nose/body/two-booster design".into());
    }
    let lengths = [
        dimension(parts[3], "Length")?,
        dimension(parts[2], "Length")?,
        dimension(parts[0], "Length")? + dimension(parts[1], "Length")?,
    ];
    let diameters = [
        dimension(parts[3], "Diameter")?,
        dimension(parts[2], "Diameter")?,
        dimension(parts[1], "Diameter")?,
    ];
    let lb = 0.45359237;
    let cumulative = [
        get("Booster2LaunchWt")? * lb,
        get("Booster1LaunchWt")? * lb,
        get("SustainerLaunchWt")? * lb,
    ];
    let cg = [
        get("Booster2CG")? * 0.0254,
        get("Booster1CG")? * 0.0254,
        get("SustainerCG")? * 0.0254,
    ];
    let names = ["O6000W", "O3400-IM-P", "N5800-CS"];
    let labels = [
        "Bottom booster / O6000W",
        "Upper booster / O3400-IM-P",
        "Sustainer / N5800-CS",
    ];
    for i in 0..3 {
        let upper_mass = if i < 2 { cumulative[i + 1] } else { 0.0 };
        let upper_cg = if i < 2 { cg[i + 1] } else { 0.0 };
        let upper_length = lengths[i + 1..].iter().sum::<f64>();
        let wet_mass = cumulative[i] - upper_mass;
        let wet_cg = (cumulative[i] * cg[i] - upper_mass * upper_cg) / wet_mass - upper_length;
        let s = &mut m.stages[i];
        s.name = labels[i].into();
        s.length_m = lengths[i];
        s.diameter_m = diameters[i];
        s.motor = imports::import_motor_named("sources/rasp.eng", Some(names[i]))?;
        s.motor.cg_m = s.length_m * 0.75;
        s.dry_mass_kg = wet_mass - s.motor.propellant_mass_kg;
        s.dry_cg_m =
            (wet_mass * wet_cg - s.motor.propellant_mass_kg * s.motor.cg_m) / s.dry_mass_kg;
        s.payload_mass_kg = 0.0;
        s.payload_cg_m = s.length_m * 0.15;
        s.aero_source="SYNTHETIC complete-stack placeholder. Replace with verified RASAero exports. Partial CFD is not applied.".into();
        for a in &mut s.aero {
            a.cp_m = lengths[i..].iter().sum::<f64>() * 0.76;
        }
        s.ignition_delay_s = match i {
            0 => 0.0,
            1 => get("Booster1IgnitionDelay")?,
            _ => get("SustainerIgnitionDelay")?,
        };
        s.separation = if i == 2 {
            Trigger::Never
        } else {
            Trigger::Burnout {
                delay_s: if i == 0 {
                    get("Booster2Delay")?
                } else {
                    get("Booster1SeparationDelay")?
                },
            }
        };
        s.separation_speed_mps = 0.5;
        s.nose_radius_m = get("BluntRadius")? * 0.0254;
    }
    m.launch.altitude_m = get("Altitude")? * 0.3048;
    m.launch.elevation_deg = 90.0 - get("RodAngle")?;
    m.launch.rail_length_m = get("RodLength")? * 0.3048;
    m.wind.east_mps = get("WindSpeed")? * 0.44704;
    m.wind.north_mps = 0.0;
    m.wind.gust_mps = 0.0;
    m.recovery.drogue.area_m2 = std::f64::consts::PI * (get("Size1")? * 0.0254).powi(2) / 4.0;
    m.recovery.drogue.cd = get("CD1")?;
    m.recovery.main.area_m2 = std::f64::consts::PI * (get("Size2")? * 0.0254).powi(2) / 4.0;
    m.recovery.main.cd = get("CD2")?;
    m.recovery.main_altitude_agl_m = get("Altitude2")? * 0.3048;
    m.validate()?;
    m.save("examples/plumbbob-provisional.json")?;
    println!(
        "Wrote examples/plumbbob-provisional.json. Actual motors/mass inputs; provisional aero, inertia, separation, thermal and avionics models."
    );
    Ok(())
}
