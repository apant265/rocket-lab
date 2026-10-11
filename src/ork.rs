//! Deliberate OpenRocket component mapping onto an existing motor/flight setup.
//! Native stage map is explicit (top-first). Unsupported semantics are retained
//! in a report; imported geometry is not an assertion of OpenRocket parity.
use crate::{
    assembly::{Material, Part, PartKind, Structure},
    config::*,
};
use roxmltree::Node;
use std::io::Read;
fn text<'a>(n: Node<'a, 'a>, name: &str) -> Option<&'a str> {
    n.children()
        .find(|c| c.has_tag_name(name))
        .and_then(|c| c.text())
}
fn number(n: Node<'_, '_>, name: &str, default: f64) -> f64 {
    text(n, name)
        .and_then(|s| s.split_whitespace().last()?.parse().ok())
        .unwrap_or(default)
}
fn length(n: Node<'_, '_>) -> f64 {
    number(
        n,
        "length",
        number(n, "packedlength", number(n, "rootchord", 0.0)),
    )
}
fn material(n: Node<'_, '_>) -> Material {
    let native = n.children().find(|c| c.has_tag_name("material"));
    let name = native.and_then(|n| n.text()).unwrap_or("Fiberglass");
    let mut m = Material::presets()
        .into_iter()
        .find(|m| {
            m.name.to_lowercase().contains(&name.to_lowercase())
                || name.to_lowercase().contains(&m.name.to_lowercase())
        })
        .unwrap_or_else(|| Material::presets().remove(0));
    m.name = name.into();
    if let Some(d) = native
        .and_then(|n| n.attribute("density"))
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|d| *d > 0.0)
    {
        m.density_kg_m3 = d;
    }
    m
}
struct Collected {
    parts: Vec<Part>,
    notes: Vec<String>,
    raw: Vec<serde_json::Value>,
    fins: Vec<StageGeometry>,
    nose: f64,
    max_d: f64,
}
fn walk(
    n: Node<'_, '_>,
    start: f64,
    parent_length: f64,
    previous: f64,
    fallback_radius: f64,
    s: &Stage,
    c: &mut Collected,
) -> Result<f64> {
    let l = length(n);
    let axial = n
        .children()
        .find(|a| a.has_tag_name("axialoffset"))
        .or_else(|| n.children().find(|a| a.has_tag_name("position")));
    let offset = axial
        .and_then(|a| a.text())
        .and_then(|x| x.parse::<f64>().ok())
        .unwrap_or(0.0);
    let method = axial
        .and_then(|a| a.attribute("method").or_else(|| a.attribute("type")))
        .unwrap_or("after");
    let position = match method {
        "top" => start + offset,
        "bottom" => start + parent_length - l + offset,
        "middle" => start + (parent_length - l) * 0.5 + offset,
        "absolute" => offset,
        "after" => previous + offset,
        _ => return Err(format!("Unsupported OpenRocket axial method {method}").into()),
    };
    let name = text(n, "name").unwrap_or(n.tag_name().name());
    let radius = number(
        n,
        "outerradius",
        number(
            n,
            "radius",
            number(n, "aftradius", number(n, "packedradius", fallback_radius)),
        ),
    );
    let tag = n.tag_name().name();
    let kind = match tag {
        "bodytube" => Some(PartKind::BodyTube),
        "nosecone" => Some(PartKind::NoseCone),
        "transition" => Some(PartKind::Transition),
        "trapezoidfinset" => Some(PartKind::FinSet),
        "innertube" => Some(PartKind::InnerTube),
        "tubecoupler" => Some(PartKind::Coupler),
        "bulkhead" => Some(PartKind::Bulkhead),
        "centeringring" | "engineblock" => Some(PartKind::CenteringRing),
        "shockcord" => Some(PartKind::ShockCord),
        "masscomponent" | "parachute" => Some(PartKind::Mass),
        "railbutton" => Some(PartKind::RailButton),
        _ => None,
    };
    let fields=n.children().filter(|c|c.is_element()&&!c.has_tag_name("subcomponents")).map(|c|(c.tag_name().name().to_owned(),serde_json::json!({"text":c.text(),"attributes":c.attributes().map(|a|(a.name(),a.value())).collect::<std::collections::BTreeMap<_,_>>()}))).collect::<serde_json::Map<_,_>>();
    c.raw
        .push(serde_json::json!({"name":name,"kind":tag,"station_m":position,"fields":fields}));
    if text(n, "overridesubcomponentsmass") == Some("true")
        || text(n, "overridesubcomponentscg") == Some("true")
    {
        return Err(format!(
            "{name}: subtree override needs explicit mass reconciliation before import"
        )
        .into());
    }
    if let Some(kind) = kind {
        let payload = name.to_uppercase();
        if tag == "masscomponent" && payload.contains("MOTOR") {
            c.notes.push(format!("{name}: report-only; base motor/casing allocation preserved to avoid counting the motor twice"));
        } else if s.payload_mass_kg > 0.0
            && ["KATE", "ANVIL", "MUON"]
                .iter()
                .any(|v| payload.contains(v))
        {
            c.notes.push(format!("{name}: retained in report; existing named payload allocation already accounts for its mass"));
        } else {
            let mut p = Part::new(kind, s);
            p.name = name.into();
            p.position_m = position;
            p.length_m = l.max(0.001);
            p.diameter_m = (radius * 2.0).max(0.001);
            p.thickness_m = number(
                n,
                "thickness",
                number(n, "outerradius", radius) - number(n, "innerradius", radius - 0.002),
            )
            .clamp(0.0001, p.diameter_m * 0.5);
            p.material = material(n);
            p.fore_diameter_m = number(n, "foreradius", 0.0) * 2.0;
            p.mass_override = false;
            p.cg_m = p.length_m * 0.5;
            if kind == PartKind::FinSet {
                let mut g = StageGeometry::generic(s, false);
                g.fin_count = number(n, "fincount", number(n, "instancecount", 4.0)) as u32;
                g.fin_root_chord_m = number(n, "rootchord", 0.2);
                g.fin_tip_chord_m = number(n, "tipchord", 0.1);
                g.fin_span_m = number(n, "height", 0.1);
                g.fin_sweep_m = number(n, "sweeplength", 0.0);
                g.fin_thickness_m = p.thickness_m;
                g.fin_position_m = position;
                p.length_m = g.fin_root_chord_m.max(g.fin_sweep_m + g.fin_tip_chord_m);
                p.shape = Some(g.clone());
                c.fins.push(g);
            }
            if kind == PartKind::NoseCone {
                let g = p.shape.as_mut().unwrap();
                g.nose_shape = match text(n, "shape").unwrap_or("conical") {
                    "conical" => NoseShape::Conical,
                    "ellipsoid" => NoseShape::Elliptical,
                    "ogive" => NoseShape::TangentOgive,
                    other => {
                        c.notes
                            .push(format!("{name}: {other} nose mapped to conical profile"));
                        NoseShape::Conical
                    }
                };
                g.nose_length_m = p.length_m;
                c.nose = c.nose.max(position + l);
            }
            if let Some(mass) = text(n, "overridemass")
                .or_else(|| text(n, "mass"))
                .and_then(|x| x.parse::<f64>().ok())
            {
                p.mass_override = true;
                p.mass_kg = mass;
            }
            if tag == "parachute" && !p.mass_override {
                let density = p.material.density_kg_m3;
                let diameter = number(n, "diameter", 0.3);
                p.mass_override = true;
                p.mass_kg = std::f64::consts::PI * diameter * diameter * 0.25 * density;
            }
            if tag == "shockcord" && !p.mass_override {
                p.mass_override = true;
                p.mass_kg = number(n, "cordlength", 1.0) * p.material.density_kg_m3;
            }
            if let Some(cg) = text(n, "overridecg").and_then(|x| x.parse::<f64>().ok()) {
                p.cg_override = true;
                p.cg_m = cg;
            }
            if text(n, "instancecount")
                .and_then(|v| v.parse::<usize>().ok())
                .is_some_and(|v| v > 1)
                && kind != PartKind::FinSet
            {
                return Err(format!(
                    "{name}: repeated instances need explicit expansion before import"
                )
                .into());
            }
            if p.position_m < 0.0 {
                c.notes.push(format!("{name}: negative stage-relative offset cannot be represented; retained only in native report"));
            } else {
                if p.external() {
                    c.max_d = c.max_d.max(p.diameter_m);
                }
                c.parts.push(p);
            }
        }
    } else {
        c.notes.push(format!(
            "{name}: unsupported {tag}, retained only in native report"
        ));
    }
    if tag == "parachute" {
        c.notes.push(format!("{name}: native deployment '{}' retained in report; existing recovery controller preserved",text(n,"deployevent").unwrap_or("unspecified")));
    }
    if let Some(children) = n.children().find(|c| c.has_tag_name("subcomponents")) {
        let mut previous = position;
        for child in children.children().filter(Node::is_element) {
            previous = walk(child, position, l, previous, radius, s, c)?;
        }
    }
    Ok(position + l)
}
pub fn import(
    path: impl AsRef<std::path::Path>,
    base: &Mission,
    map: &str,
) -> Result<(Mission, serde_json::Value)> {
    let bytes = std::fs::read(&path)?;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut file = z.by_name("rocket.ork")?;
    if file.size() > 20_000_000 {
        return Err("ORK XML exceeds 20 MB".into());
    }
    let mut xml = String::new();
    file.read_to_string(&mut xml)?;
    let doc = roxmltree::Document::parse(&xml)?;
    let native: Vec<_> = doc
        .descendants()
        .filter(|n| {
            n.has_tag_name("stage")
                && n.parent().is_some_and(|p| p.has_tag_name("subcomponents"))
                && n.ancestors().filter(|n| n.has_tag_name("stage")).count() == 1
        })
        .collect();
    let mapping: Vec<usize> = map
        .split(',')
        .map(|v| v.trim().parse())
        .collect::<std::result::Result<_, _>>()?;
    if mapping.len() != native.len()
        || mapping.iter().any(|i| *i >= base.stages.len())
        || mapping.windows(2).any(|w| w[1] > w[0])
    {
        return Err("Map every native stage (top first) to an existing stage index, in descending order; e.g. 2,2,1,0".into());
    }
    let mut m = base.clone();
    let mut collected: Vec<_> = (0..m.stages.len())
        .map(|_| Collected {
            parts: vec![],
            notes: vec![],
            raw: vec![],
            fins: vec![],
            nose: 0.0,
            max_d: 0.0,
        })
        .collect();
    let mut offsets = vec![0.0; m.stages.len()];
    for (index, n) in native.iter().enumerate() {
        let target = mapping[index];
        let s = &m.stages[target];
        let mut previous = offsets[target];
        if let Some(children) = n.children().find(|c| c.has_tag_name("subcomponents")) {
            for child in children.children().filter(Node::is_element) {
                previous = walk(
                    child,
                    offsets[target],
                    s.length_m,
                    previous,
                    s.diameter_m * 0.5,
                    s,
                    &mut collected[target],
                )?;
            }
        }
        offsets[target] = previous;
    }
    let mut reports = Vec::new();
    for (i, c) in collected.into_iter().enumerate() {
        if c.parts.is_empty() {
            continue;
        }
        let s = &mut m.stages[i];
        let old_len = s.length_m;
        let old_dry = crate::assembly::dry_pieces(s)
            .iter()
            .map(|p| p.0)
            .sum::<f64>();
        let new_mass = c.parts.iter().map(Part::mass).sum::<f64>();
        if new_mass >= old_dry {
            return Err(format!("S{} native components {:.3} kg exceed base dry allocation {:.3} kg; reconcile masses first",i+1,new_mass,old_dry).into());
        }
        s.length_m = offsets[i].max(
            c.parts
                .iter()
                .map(|p| p.position_m + p.length_m)
                .fold(0.0, f64::max),
        );
        s.diameter_m = c.max_d.max(s.diameter_m);
        s.dry_cg_m = s.dry_cg_m / old_len * s.length_m;
        s.motor.cg_m = s.motor.cg_m / old_len * s.length_m;
        let mut material = Structure::for_stage(s);
        material.component_shell = true;
        material.hardware_mass_kg = old_dry - new_mass;
        material.hardware_cg_m = s.dry_cg_m;
        s.materials = Some(material);
        s.components = c.parts;
        let mut g = c
            .fins
            .first()
            .cloned()
            .unwrap_or_else(|| StageGeometry::generic(s, i == base.stages.len() - 1));
        g.nose_length_m = if i == base.stages.len() - 1 {
            c.nose.max(0.001)
        } else {
            0.0
        };
        g.fin_position_m = g.fin_position_m.max(g.nose_length_m);
        s.geometry = Some(g);
        reports.push(serde_json::json!({"stage":i,"imported_components":s.components.len(),"base_dry_mass_kg":old_dry,"native_component_mass_kg":new_mass,"remaining_hardware_kg":old_dry-new_mass,"notes":c.notes,"native_components":c.raw}));
    }
    m.experiments.aero_grids.clear();
    crate::design::estimate_aero(&mut m);
    m.validate()?;
    let report = serde_json::json!({"source":path.as_ref().display().to_string(),"mapping":mapping,"stages":reports,"limitations":"Existing motor curves, named payload and recovery logic preserved. Total base dry mass retained, remaining hardware CG uses scaled base station; this does not preserve measured overall CG. Primary flight geometry uses first fin set and stage envelopes. All native fields are retained in this report. Unsupported/negative-offset parts remain report-only. Review before accepting flight results."});
    std::fs::create_dir_all("output/import")?;
    std::fs::write(
        "output/import/openrocket-mapping.json",
        serde_json::to_string_pretty(&report)?,
    )?;
    Ok((m, report))
}
