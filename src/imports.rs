//! Explicit import boundaries: trajectory CSV is never treated as an aero database.
use crate::config::*;
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};
pub fn import_motor(path: impl AsRef<Path>) -> Result<Motor> {
    import_motor_named(path, None)
}
pub fn import_motor_named(path: impl AsRef<Path>, name: Option<&str>) -> Result<Motor> {
    if path
        .as_ref()
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rse"))
    {
        return import_rse(path, name);
    }
    let database = fs::read_to_string(path)?;
    let text = if let Some(name) = name {
        let mut selected = String::new();
        let mut active = false;
        let mut matches = 0;
        for line in database
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim().starts_with(';'))
        {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() >= 7 {
                active = fields[0] == name;
                if active {
                    matches += 1;
                }
            }
            if active {
                selected.push_str(line);
                selected.push('\n');
            }
        }
        if matches != 1 {
            return Err(format!(
                "motor '{name}' matched {matches} entries; select a unique motor designation"
            )
            .into());
        }
        selected
    } else {
        database
    };
    let mut lines = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim().starts_with(';'));
    let header = lines
        .next()
        .ok_or("empty RASP motor file")?
        .split_whitespace()
        .collect::<Vec<_>>();
    if header.len() < 7 {
        return Err(
            "RASP header needs name diameter length delays propellant_mass total_mass manufacturer"
                .into(),
        );
    }
    let mut points: Vec<[f64; 2]> = Vec::new();
    for line in lines {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 2 {
            return Err(
                "Provide one motor per .eng file or select a database entry with --name".into(),
            );
        }
        points.push([parts[0].parse()?, parts[1].parse()?]);
    }
    if points.first().is_some_and(|p| p[0] > 0.0) {
        points.insert(0, [0.0, 0.0]);
    }
    if points.len() < 2
        || points[0] != [0.0, 0.0]
        || points.last().unwrap()[1] != 0.0
        || points.windows(2).any(|w| w[1][0] <= w[0][0])
        || points
            .iter()
            .any(|p| !p[0].is_finite() || !p[1].is_finite() || p[0] < 0.0 || p[1] < 0.0)
    {
        return Err("Invalid motor curve: ordered nonnegative points, start [0,0], end at zero thrust required".into());
    }
    let propellant_mass_kg: f64 = header[4].parse()?;
    let total: f64 = header[5].parse()?;
    if !propellant_mass_kg.is_finite()
        || propellant_mass_kg <= 0.0
        || !total.is_finite()
        || total < propellant_mass_kg
    {
        return Err("invalid RASP masses".into());
    }
    Ok(Motor {
        name: format!("{} ({})", header[0], header[6]),
        propellant_mass_kg,
        cg_m: 0.0,
        points,
        thrust_offset_m: [0.0, 0.0],
        misalignment_deg: [0.0, 0.0],
    })
}
/// RockSim engine XML. Thrust is N, times s, header mass g, dimensions mm.
/// Native per-sample mass/CG are reported by RSE but the current motor model
/// uses impulse-weighted propellant depletion and a supplied fixed motor CG.
pub fn import_rse(path: impl AsRef<Path>, name: Option<&str>) -> Result<Motor> {
    let xml = fs::read_to_string(path)?;
    let doc = roxmltree::Document::parse(&xml)?;
    let engines: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name("engine") && name.is_none_or(|s| n.attribute("code") == Some(s)))
        .collect();
    if engines.len() != 1 {
        return Err(format!(
            "RSE selection matched {} engines; choose a unique code",
            engines.len()
        )
        .into());
    }
    let e = engines[0];
    let prop: f64 = e
        .attribute("propWt")
        .ok_or("RSE missing propWt")?
        .parse::<f64>()?
        / 1000.0;
    let mut points: Vec<[f64; 2]> = e
        .descendants()
        .filter(|n| n.has_tag_name("eng-data"))
        .map(|n| {
            Ok([
                n.attribute("t").ok_or("RSE sample missing t")?.parse()?,
                n.attribute("f").ok_or("RSE sample missing f")?.parse()?,
            ])
        })
        .collect::<Result<_>>()?;
    if points.first().is_some_and(|p| p[0] > 0.0) {
        points.insert(0, [0.0, 0.0]);
    }
    if prop < 0.0
        || !prop.is_finite()
        || points.len() < 2
        || points[0] != [0.0, 0.0]
        || points.last().unwrap()[1] != 0.0
        || points.windows(2).any(|w| w[1][0] <= w[0][0])
        || points.iter().flatten().any(|v| !v.is_finite() || *v < 0.0)
    {
        return Err("Invalid RSE mass or thrust curve".into());
    }
    Ok(Motor {
        name: format!(
            "{} ({})",
            e.attribute("code").unwrap_or("RSE engine"),
            e.attribute("mfg").unwrap_or("unknown")
        ),
        propellant_mass_kg: prop,
        cg_m: 0.0,
        points,
        thrust_offset_m: [0.0; 2],
        misalignment_deg: [0.0; 2],
    })
}
#[derive(Deserialize)]
struct AeroRow {
    mach: f64,
    cd_power_off: f64,
    cd_power_on: f64,
    cn_alpha_per_rad: f64,
    cp_m: f64,
}
pub fn import_aero(path: impl AsRef<Path>) -> Result<Vec<AeroPoint>> {
    let mut reader = csv::Reader::from_path(path)?;
    let required = [
        "mach",
        "cd_power_off",
        "cd_power_on",
        "cn_alpha_per_rad",
        "cp_m",
    ];
    for name in required {
        if !reader.headers()?.iter().any(|h| h == name) {
            return Err(format!("Aero CSV missing '{name}'. RASAero flight exports and partial CFD tables are not complete aero tables.").into());
        }
    }
    let mut points = Vec::new();
    for row in reader.deserialize::<AeroRow>() {
        let r = row?;
        points.push(AeroPoint {
            mach: r.mach,
            cd_power_off: r.cd_power_off,
            cd_power_on: r.cd_power_on,
            cn_alpha_per_rad: r.cn_alpha_per_rad,
            cp_m: r.cp_m,
        });
    }
    if points.len() < 2
        || points[0].mach != 0.0
        || points.iter().any(|a| {
            [
                a.mach,
                a.cd_power_off,
                a.cd_power_on,
                a.cn_alpha_per_rad,
                a.cp_m,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
        })
        || points.windows(2).any(|w| w[1].mach <= w[0].mach)
    {
        return Err("invalid aero CSV: ordered Mach stations starting at 0 required".into());
    }
    Ok(points)
}
#[derive(Serialize)]
pub struct DesignInspection {
    pub source: String,
    pub format: String,
    pub geometry: Vec<serde_json::Value>,
    pub motors: Vec<String>,
    pub saved_outputs: Vec<serde_json::Value>,
    pub notes: Vec<String>,
}
pub fn inspect_design(path: impl AsRef<Path>) -> Result<DesignInspection> {
    let path = path.as_ref();
    let bytes = fs::read(path)?;
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    let xml = if ext == "ork" {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        let mut file = archive.by_name("rocket.ork")?;
        if file.size() > 20_000_000 {
            return Err("ORK XML exceeds 20 MB".into());
        }
        let mut xml = String::new();
        file.read_to_string(&mut xml)?;
        xml
    } else {
        String::from_utf8(bytes)?
    };
    let doc = roxmltree::Document::parse(&xml)?;
    let mut result = DesignInspection {
        source: path.display().to_string(),
        format: ext.clone(),
        geometry: Vec::new(),
        motors: Vec::new(),
        saved_outputs: Vec::new(),
        notes: Vec::new(),
    };
    if ext == "cdx1" {
        for n in doc
            .descendants()
            .filter(|n| matches!(n.tag_name().name(), "NoseCone" | "BodyTube" | "Booster"))
        {
            let get = |key: &str| {
                n.children()
                    .find(|c| c.has_tag_name(key))
                    .and_then(|c| c.text())
                    .and_then(|s| s.parse::<f64>().ok())
            };
            result.geometry.push(serde_json::json!({"part":n.tag_name().name(),"length_m":get("Length").map(|x|x*0.0254),"diameter_m":get("Diameter").map(|x|x*0.0254),"location_m":get("Location").map(|x|x*0.0254)}));
        }
        for n in doc.descendants().filter(|n| n.is_element()) {
            let key = n.tag_name().name();
            if key.ends_with("Engine") {
                result.motors.push(n.text().unwrap_or("").into());
            }
            if matches!(
                key,
                "MaxAltitude"
                    | "MaxVelocity"
                    | "TimetoApogee"
                    | "SustainerLaunchWt"
                    | "Booster1LaunchWt"
                    | "Booster2LaunchWt"
                    | "UseBooster1"
                    | "UseBooster2"
                    | "IncludeBooster1"
                    | "IncludeBooster2"
            ) {
                result.saved_outputs.push(serde_json::json!({"field":key,"raw_value":n.text(),"units":"native RASAero: ft, ft/s, lb, s, or boolean"}));
            }
        }
        result.notes.push("Inspection only: motor names are not thrust curves; stored output values may be stale. Geometry uses inches converted to SI. Resolve booster enable flags against simulation include flags.".into());
    } else if ext == "ork" {
        for n in doc.descendants().filter(|n| {
            matches!(
                n.tag_name().name(),
                "nosecone"
                    | "bodytube"
                    | "stage"
                    | "trapezoidfinset"
                    | "masscomponent"
                    | "parachute"
            )
        }) {
            let fields = n
                .children()
                .filter(|c| c.is_element() && c.tag_name().name() != "subcomponents")
                .filter_map(|c| {
                    c.text().map(|v| {
                        (
                            c.tag_name().name().to_owned(),
                            serde_json::Value::String(v.into()),
                        )
                    })
                })
                .collect::<serde_json::Map<_, _>>();
            result
                .geometry
                .push(serde_json::json!({"part":n.tag_name().name(),"fields":fields}));
        }
        for n in doc.descendants().filter(|n| n.has_tag_name("designation")) {
            result.motors.push(n.text().unwrap_or("").into());
        }
        result.notes.push("Inspection only: native OpenRocket SI geometry and saved configuration; component overrides, nested positions and motor assignments require explicit mapping to a mission.".into());
    } else {
        return Err("supported design formats: .CDX1 and zipped .ork".into());
    }
    Ok(result)
}
#[derive(Clone, Debug, Serialize)]
pub struct ReferencePoint {
    pub time_s: f64,
    pub stage: String,
    pub altitude_agl_m: f64,
    pub speed_mps: f64,
    pub mach: f64,
}
pub fn reference_flight(path: impl AsRef<Path>) -> Result<Vec<ReferencePoint>> {
    let mut reader = csv::Reader::from_path(path)?;
    let h = reader.headers()?.clone();
    let index = |name: &str| {
        h.iter()
            .position(|v| v == name)
            .ok_or_else(|| format!("missing RASAero trajectory column {name}"))
    };
    let ti = index("Time (sec)")?;
    let si = index("Stage")?;
    let ai = index("Altitude (ft)")?;
    let vi = index("Velocity (ft/sec)")?;
    let mi = index("Mach Number")?;
    let mut output = Vec::new();
    for row in reader.records() {
        let row = row?;
        let p = ReferencePoint {
            time_s: row[ti].parse()?,
            stage: row[si].into(),
            altitude_agl_m: row[ai].parse::<f64>()? * 0.3048,
            // RASAero's signed descent velocity is compared with our speed magnitude.
            // This field is not a reconstructed vertical-velocity measurement.
            speed_mps: row[vi].parse::<f64>()?.abs() * 0.3048,
            mach: row[mi].parse()?,
        };
        if [p.time_s, p.altitude_agl_m, p.speed_mps, p.mach]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err("non-finite reference telemetry".into());
        }
        output.push(p);
    }
    if output.is_empty() || output.windows(2).any(|w| w[1].time_s <= w[0].time_s) {
        return Err("reference times must increase".into());
    }
    Ok(output)
}
