//! Portable triangle geometry and optional headless CAD/scene conversion.
use crate::{config::Result, surface::MeshDocument};
use glam::DVec3;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const MAX_TRIANGLES: usize = 1_000_000;
const MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    Millimeters,
    Centimeters,
    Meters,
    Inches,
    Feet,
}
impl Units {
    pub const ALL: [Self; 5] = [
        Self::Millimeters,
        Self::Centimeters,
        Self::Meters,
        Self::Inches,
        Self::Feet,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Millimeters => "mm",
            Self::Centimeters => "cm",
            Self::Meters => "m",
            Self::Inches => "in",
            Self::Feet => "ft",
        }
    }
    pub fn meters(self) -> f64 {
        match self {
            Self::Millimeters => 0.001,
            Self::Centimeters => 0.01,
            Self::Meters => 1.,
            Self::Inches => 0.0254,
            Self::Feet => 0.3048,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Z,
}
impl Axis {
    pub fn label(self) -> &'static str {
        match self {
            Self::X => "+X",
            Self::Y => "+Y",
            Self::Z => "+Z",
        }
    }
    fn to_model(self, p: DVec3) -> DVec3 {
        match self {
            Self::X => DVec3::new(-p.y, p.x, p.z),
            Self::Y => p,
            Self::Z => DVec3::new(p.x, p.z, -p.y),
        }
    }
    fn to_file(self, p: DVec3) -> DVec3 {
        match self {
            Self::X => DVec3::new(p.y, -p.x, p.z),
            Self::Y => p,
            Self::Z => DVec3::new(p.x, -p.z, p.y),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    pub source: String,
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub length_m: f64,
    pub diameter_m: f64,
    pub closed: bool,
    pub volume_m3: f64,
    pub centroid_m: [f64; 3],
    pub profile: Vec<[f64; 2]>,
}
impl Geometry {
    pub fn new(source: String, positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Result<Self> {
        let mut g = Self {
            source,
            positions,
            indices,
            length_m: 0.,
            diameter_m: 0.,
            closed: false,
            volume_m3: 0.,
            centroid_m: [0.; 3],
            profile: Vec::new(),
        };
        g.check_triangles()?;
        let mut remap = HashMap::new();
        let mut used = Vec::new();
        for i in &mut g.indices {
            *i = *remap.entry(*i).or_insert_with(|| {
                let index = used.len() as u32;
                used.push(g.positions[*i as usize]);
                index
            });
        }
        g.positions = used;
        let (min, max) = g.bounds();
        let center = (min + max) * 0.5;
        for p in &mut g.positions {
            *p = (DVec3::from_array(p.map(f64::from)) - center)
                .as_vec3()
                .to_array();
        }
        let (min, max) = g.bounds();
        g.length_m = max.y - min.y;
        g.diameter_m = g
            .positions
            .iter()
            .map(|p| 2. * (p[0] as f64).hypot(p[2] as f64))
            .fold(0., f64::max);
        if g.length_m <= 1e-9 || g.diameter_m <= 1e-9 {
            return Err(
                "Model needs nonzero axial length and radial extent; choose another nose axis"
                    .into(),
            );
        }
        let (closed, volume, centroid) = g.integrals();
        if closed && volume < 0. {
            for face in g.indices.chunks_exact_mut(3) {
                face.swap(1, 2);
            }
        }
        g.closed = closed;
        g.volume_m3 = if closed { volume.abs() } else { 0. };
        g.centroid_m = centroid;
        g.profile = g.make_profile();
        Ok(g)
    }
    fn check_triangles(&self) -> Result<()> {
        if self.positions.len() < 3
            || self.positions.len() > MAX_TRIANGLES * 3
            || self.indices.is_empty()
            || !self.indices.len().is_multiple_of(3)
            || self.indices.len() / 3 > MAX_TRIANGLES
            || self.positions.iter().flatten().any(|v| !v.is_finite())
            || self
                .indices
                .iter()
                .any(|&v| v as usize >= self.positions.len())
        {
            return Err("Invalid or oversized triangle mesh (maximum 1,000,000 triangles)".into());
        }
        for f in self.indices.chunks_exact(3) {
            let [a, b, c] = [f[0], f[1], f[2]]
                .map(|i| DVec3::from_array(self.positions[i as usize].map(f64::from)));
            if (b - a).cross(c - a).length_squared() <= 1e-30 {
                return Err(
                    "Mesh contains a degenerate triangle; repair it before importing".into(),
                );
            }
        }
        Ok(())
    }
    fn bounds(&self) -> (DVec3, DVec3) {
        self.positions.iter().fold(
            (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
            |(min, max), p| {
                let p = DVec3::from_array(p.map(f64::from));
                (min.min(p), max.max(p))
            },
        )
    }
    fn integrals(&self) -> (bool, f64, [f64; 3]) {
        let mut edges = HashMap::<([u32; 3], [u32; 3]), (usize, i32)>::new();
        let mut volume = 0.;
        let mut first = DVec3::ZERO;
        let mut area = 0.;
        let mut surface_first = DVec3::ZERO;
        for f in self.indices.chunks_exact(3) {
            let raw = [f[0], f[1], f[2]].map(|i| self.positions[i as usize]);
            let keys = raw.map(|p| p.map(|v| if v == 0. { 0 } else { v.to_bits() }));
            for i in 0..3 {
                let a = keys[i];
                let b = keys[(i + 1) % 3];
                let (key, sign) = if a < b { ((a, b), 1) } else { ((b, a), -1) };
                let e = edges.entry(key).or_default();
                e.0 += 1;
                e.1 += sign;
            }
            let [a, b, c] = raw.map(|p| DVec3::from_array(p.map(f64::from)));
            let v = a.dot(b.cross(c)) / 6.;
            volume += v;
            first += (a + b + c) * (v * 0.25);
            let face_area = (b - a).cross(c - a).length() * 0.5;
            area += face_area;
            surface_first += (a + b + c) * (face_area / 3.);
        }
        let closed = edges.values().all(|&(n, s)| n == 2 && s == 0) && volume.abs() > 1e-18;
        (
            closed,
            volume,
            if closed {
                (first / volume).to_array()
            } else {
                (surface_first / area).to_array()
            },
        )
    }
    fn make_profile(&self) -> Vec<[f64; 2]> {
        // Slice triangles, not just vertex rings: inner-wall vertices in a hollow
        // CAD solid must never pull the exterior envelope inward.
        const N: usize = 512;
        let mut radii = vec![0_f64; N + 1];
        let step = self.length_m / N as f64;
        let tolerance = self.length_m * 1e-7;
        for f in self.indices.chunks_exact(3) {
            let vertices = [f[0], f[1], f[2]].map(|i| self.positions[i as usize].map(f64::from));
            let stations = vertices.map(|p| self.length_m * 0.5 - p[1]);
            let first = ((stations.into_iter().fold(f64::INFINITY, f64::min) - tolerance) / step)
                .ceil()
                .max(0.) as usize;
            let last = ((stations.into_iter().fold(f64::NEG_INFINITY, f64::max) + tolerance) / step)
                .floor()
                .clamp(0., N as f64) as usize;
            for (k, radius) in radii.iter_mut().enumerate().take(last + 1).skip(first) {
                let station = k as f64 * step;
                for i in 0..3 {
                    let j = (i + 1) % 3;
                    let a = stations[i];
                    let b = stations[j];
                    if station < a.min(b) - tolerance || station > a.max(b) + tolerance {
                        continue;
                    }
                    if (a - b).abs() < 1e-12 {
                        *radius = radius
                            .max(vertices[i][0].hypot(vertices[i][2]))
                            .max(vertices[j][0].hypot(vertices[j][2]));
                    } else {
                        let u = ((station - a) / (b - a)).clamp(0., 1.);
                        let x = vertices[i][0] + u * (vertices[j][0] - vertices[i][0]);
                        let z = vertices[i][2] + u * (vertices[j][2] - vertices[i][2]);
                        *radius = radius.max(x.hypot(z));
                    }
                }
            }
        }
        radii
            .into_iter()
            .enumerate()
            .map(|(i, r)| [i as f64 * step, r])
            .collect()
    }
    pub fn validate(&self) -> Result<()> {
        self.check_triangles()?;
        let (closed, volume, cg) = self.integrals();
        let (min, max) = self.bounds();
        let diameter = self
            .positions
            .iter()
            .map(|p| 2. * (p[0] as f64).hypot(p[2] as f64))
            .fold(0., f64::max);
        let near = |a: f64, b: f64| (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1e-8);
        if self.length_m <= 1e-9
            || self.diameter_m <= 1e-9
            || self.centroid_m.iter().any(|v| !v.is_finite())
            || !near(self.length_m, max.y - min.y)
            || !near(self.diameter_m, diameter)
            || self.closed != closed
            || !near(self.volume_m3, if closed { volume.abs() } else { 0. })
            || self.centroid_m.iter().zip(cg).any(|(&a, b)| !near(a, b))
            || self.profile != self.make_profile()
            || (min + max).length() > 1e-5
        {
            return Err("Stored mesh measurements do not match its triangles".into());
        }
        Ok(())
    }
    pub fn scaled_positions(&self, length: f64, diameter: f64) -> Vec<[f32; 3]> {
        self.positions
            .iter()
            .map(|p| {
                [
                    (p[0] as f64 * diameter / self.diameter_m) as f32,
                    (p[1] as f64 * length / self.length_m) as f32,
                    (p[2] as f64 * diameter / self.diameter_m) as f32,
                ]
            })
            .collect()
    }
    pub fn radius(&self, station: f64, length: f64, diameter: f64) -> f64 {
        if station < 0. || station > length {
            return 0.;
        }
        let s = station * self.length_m / length;
        let i = self.profile.partition_point(|p| p[0] < s);
        let r = if i == 0 {
            self.profile[0][1]
        } else if i == self.profile.len() {
            self.profile[i - 1][1]
        } else {
            let a = self.profile[i - 1];
            let b = self.profile[i];
            a[1] + (b[1] - a[1]) * ((s - a[0]) / (b[0] - a[0]).max(1e-12)).clamp(0., 1.)
        };
        r * diameter / self.diameter_m
    }
}
fn extension(path: &Path) -> String {
    path.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase()
}
/// Insert transactionally: invalid geometry never replaces the current design.
pub fn insert(
    mission: &mut crate::config::Mission,
    stage: usize,
    geometry: Geometry,
    replace: bool,
) -> Result<usize> {
    geometry.validate()?;
    let mut updated = mission.clone();
    crate::design::initialize_geometry(&mut updated);
    let top = updated.stages.len() - 1;
    let s = updated
        .stages
        .get_mut(stage)
        .ok_or("Select an existing stage")?;
    let mut p = crate::assembly::Part::new(crate::assembly::PartKind::CustomMesh, s);
    p.name = geometry.source.clone();
    p.length_m = geometry.length_m;
    p.diameter_m = geometry.diameter_m;
    p.position_m = 0.;
    p.axial_offset_m = 0.;
    p.cg_m = p.length_m * 0.5;
    p.fore_diameter_m = 0.;
    p.thickness_m = 0.002_f64.min(p.diameter_m * 0.1);
    p.material = s.materials.as_ref().map_or_else(
        || crate::assembly::Material::presets().remove(0),
        |m| m.airframe.clone(),
    );
    p.mass_override = replace || !geometry.closed;
    p.mass_kg = if replace { s.dry_mass_kg } else { 0.1 };
    if replace {
        if p.length_m <= 0.02 {
            return Err(
                "A replacement stage must be longer than 20 mm; import this as a part instead"
                    .into(),
            );
        }
        let ratio = p.length_m / s.length_m;
        s.length_m = p.length_m;
        s.diameter_m = p.diameter_m;
        s.dry_cg_m *= ratio;
        s.motor.cg_m *= ratio;
        s.payload_cg_m *= ratio;
        s.components.clear();
        s.geometry = Some(crate::config::StageGeometry::generic(s, stage == top));
        let mut structure = crate::assembly::Structure::for_stage(s);
        structure.component_shell = true;
        structure.hardware_mass_kg = 0.;
        structure.airframe_wall_m = p.thickness_m;
        structure.nose_wall_m = p.thickness_m;
        s.materials = Some(structure);
    } else if p.length_m > s.length_m {
        return Err("Imported part is longer than the stage; choose Replace exterior or change import scale".into());
    }
    p.mesh = Some(geometry);
    let index = s.components.len();
    s.components.push(p);
    s.model_mass = None;
    updated.validate()?;
    *mission = updated;
    Ok(index)
}
pub fn cad_format(path: &Path) -> bool {
    matches!(
        extension(path).as_str(),
        "step" | "stp" | "iges" | "igs" | "brep" | "brp" | "fcstd"
    )
}
pub fn import(path: &Path, units: Units, axis: Axis, tolerance_mm: f64) -> Result<Geometry> {
    if fs::metadata(path)?.len() > MAX_BYTES {
        return Err("Import file exceeds 256 MiB".into());
    }
    let ext = extension(path);
    let (positions,indices,scale)=match ext.as_str() {
        "stl"=>{let (p,i)=read_stl(&fs::read(path)?)?;(p,i,units.meters())},
        "obj"=>{let (p,i)=read_obj(&fs::read_to_string(path)?)?;(p,i,units.meters())},
        "step"|"stp"|"iges"|"igs"|"brep"|"brp"|"fcstd"=>{let raw=convert_import(path,tolerance_mm,true)?;(raw.positions,raw.indices,0.001)},
        "glb"|"gltf"|"ply"|"blend"|"fbx"|"dae"=>{let raw=convert_import(path,tolerance_mm,false)?;(raw.positions,raw.indices,units.meters())},
        _=>return Err("Unsupported format. Import STL/OBJ or convert proprietary CAD to STEP, IGES, BREP or FCStd.".into()),
    };
    let positions = positions
        .iter()
        .map(|p| {
            axis.to_model(DVec3::from_array(p.map(f64::from)) * scale)
                .as_vec3()
                .to_array()
        })
        .collect();
    Geometry::new(
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        positions,
        indices,
    )
}
fn read_stl(bytes: &[u8]) -> Result<(Vec<[f32; 3]>, Vec<u32>)> {
    let mut p = Vec::new();
    if bytes.len() >= 84 {
        let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        if n <= MAX_TRIANGLES && 84 + 50 * n == bytes.len() {
            for face in bytes[84..].chunks_exact(50) {
                for v in face[12..48].chunks_exact(12) {
                    p.push(std::array::from_fn(|i| {
                        f32::from_le_bytes(v[i * 4..i * 4 + 4].try_into().unwrap())
                    }));
                }
            }
            let indices = (0..p.len() as u32).collect();
            return Ok((p, indices));
        }
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "Invalid or truncated binary STL")?;
    let mut facet = false;
    let mut vertices = 0;
    for line in text.lines() {
        let f: Vec<_> = line.split_whitespace().collect();
        match f.first().copied() {
            Some("facet") => {
                if facet {
                    return Err("Malformed STL facet".into());
                }
                facet = true;
                vertices = 0;
            }
            Some("vertex") => {
                if !facet || f.len() != 4 || vertices >= 3 {
                    return Err("Malformed STL vertex".into());
                }
                p.push([f[1].parse()?, f[2].parse()?, f[3].parse()?]);
                vertices += 1;
            }
            Some("endfacet") => {
                if !facet || vertices != 3 {
                    return Err("STL facet needs three vertices".into());
                }
                facet = false;
            }
            _ => {}
        }
        if p.len() / 3 > MAX_TRIANGLES {
            return Err("STL exceeds triangle limit".into());
        }
    }
    if facet || p.is_empty() || !p.len().is_multiple_of(3) {
        return Err("Malformed or empty STL".into());
    }
    let indices = (0..p.len() as u32).collect();
    Ok((p, indices))
}
fn read_obj(text: &str) -> Result<(Vec<[f32; 3]>, Vec<u32>)> {
    let mut p = Vec::<[f32; 3]>::new();
    let mut faces = Vec::new();
    for line in text.lines() {
        let f: Vec<_> = line
            .split('#')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        match f.first().copied() {
            Some("v") => {
                if f.len() < 4 {
                    return Err("OBJ vertex needs three coordinates".into());
                }
                let mut v = [f[1].parse::<f32>()?, f[2].parse()?, f[3].parse()?];
                if f.len() == 5 {
                    let w = f[4].parse::<f32>()?;
                    if !w.is_finite() || w == 0. {
                        return Err("Invalid OBJ homogeneous coordinate".into());
                    }
                    v = v.map(|x| x / w);
                }
                p.push(v);
            }
            Some("f") => {
                if f.len() < 4 || f.len() > 65 {
                    return Err("OBJ face needs 3–64 vertices".into());
                }
                let face = f[1..]
                    .iter()
                    .map(|t| {
                        let v = t.split('/').next().unwrap().parse::<i64>()?;
                        let i = if v > 0 {
                            v - 1
                        } else if v < 0 {
                            p.len() as i64 + v
                        } else {
                            -1
                        };
                        if i < 0 {
                            return Err("OBJ vertex index is out of range".into());
                        }
                        Ok(i as usize)
                    })
                    .collect::<Result<Vec<_>>>()?;
                faces.push(face);
            }
            _ => {}
        }
        if p.len() > MAX_TRIANGLES * 3 || faces.len() > MAX_TRIANGLES {
            return Err("OBJ exceeds geometry limit".into());
        }
    }
    let mut indices = Vec::new();
    for f in faces {
        if f.iter().any(|&i| i >= p.len()) {
            return Err("OBJ face references a missing vertex".into());
        }
        if f.len() == 3 {
            indices.extend(f.iter().map(|&i| i as u32));
            continue;
        }
        let normal = (0..f.len())
            .map(|i| {
                DVec3::from_array(p[f[i]].map(f64::from))
                    .cross(DVec3::from_array(p[f[(i + 1) % f.len()]].map(f64::from)))
            })
            .sum::<DVec3>()
            .abs();
        let projected: Vec<_> = f
            .iter()
            .map(|&i| {
                let v = p[i].map(f64::from);
                if normal.x >= normal.y && normal.x >= normal.z {
                    [v[1], v[2]]
                } else if normal.y >= normal.z {
                    [v[0], v[2]]
                } else {
                    [v[0], v[1]]
                }
            })
            .collect();
        for t in crate::assembly::triangulate_fin(&projected)? {
            indices.extend(t.map(|i| f[i] as u32));
        }
        if indices.len() / 3 > MAX_TRIANGLES {
            return Err("OBJ exceeds triangle limit".into());
        }
    }
    Ok((p, indices))
}
#[derive(Deserialize)]
struct RawGeometry {
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
}
fn executable(env: &str, candidates: &[&str]) -> Result<PathBuf> {
    std::env::var_os(env)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| candidates.iter().map(PathBuf::from).find(|p| p.is_file()))
        .ok_or_else(|| format!("Converter not found; set {env} to its executable").into())
}
pub fn freecad() -> Result<PathBuf> {
    executable(
        "FREECAD_PATH",
        &[
            "C:/Program Files/FreeCAD 1.1/bin/freecadcmd.exe",
            "C:/Program Files/FreeCAD 1.0/bin/FreeCADCmd.exe",
            "/usr/bin/FreeCADCmd",
            "/usr/bin/freecadcmd",
        ],
    )
}
fn blender() -> Result<PathBuf> {
    executable(
        "BLENDER_PATH",
        &[
            "C:/Program Files/Blender Foundation/Blender 5.2/blender.exe",
            "C:/Program Files/Blender Foundation/Blender 4.5/blender.exe",
            "/usr/bin/blender",
        ],
    )
}
fn converter(request: serde_json::Value, cad: bool) -> Result<serde_json::Value> {
    let ticket = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    let work = std::env::current_dir()?
        .join("output/exchange")
        .join(ticket);
    fs::create_dir_all(&work)?;
    let mut request = request;
    request["result"] = serde_json::to_value(work.join("result.json"))?;
    let req = work.join("request.json");
    fs::write(&req, serde_json::to_vec(&request)?)?;
    let mut cmd = Command::new(if cad { freecad()? } else { blender()? });
    if cad {
        cmd.arg("scripts/cad_exchange.py")
            .env("ROCKET_LAB_CAD_REQUEST", &req);
    } else {
        cmd.args([
            "--background",
            "--factory-startup",
            "--python-exit-code",
            "1",
            "--python",
            "scripts/scene_exchange.py",
            "--",
        ])
        .arg(&req);
    }
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let output = cmd.output()?;
    fs::write(
        work.join("converter.log"),
        [output.stdout, output.stderr].concat(),
    )?;
    let result: serde_json::Value =
        serde_json::from_slice(&fs::read(work.join("result.json")).map_err(|_| {
            format!(
                "Converter did not produce a result; see {}",
                work.join("converter.log").display()
            )
        })?)?;
    if !output.status.success() || result["ok"] != true {
        return Err(format!(
            "Conversion failed: {}. Log: {}",
            result["error"]
                .as_str()
                .unwrap_or("converter process failed"),
            work.join("converter.log").display()
        )
        .into());
    }
    Ok(result)
}
fn convert_import(path: &Path, tolerance_mm: f64, cad: bool) -> Result<RawGeometry> {
    if !tolerance_mm.is_finite() || !(0.001..=10.).contains(&tolerance_mm) {
        return Err("CAD tessellation tolerance must be 0.001–10 mm".into());
    }
    let result = converter(
        serde_json::json!({"operation":"import","input":fs::canonicalize(path)?,"tolerance_mm":tolerance_mm}),
        cad,
    )?;
    let geometry = PathBuf::from(
        result["geometry"]
            .as_str()
            .ok_or("Converter omitted mesh output")?,
    );
    if fs::metadata(&geometry)?.len() > MAX_BYTES {
        return Err("Converted mesh exceeds 256 MiB; increase tessellation tolerance".into());
    }
    Ok(serde_json::from_slice(&fs::read(geometry)?)?)
}
/// Export evaluated triangles. CAD exports contain faceted shells/solids, not source features.
pub fn export(
    model: &MeshDocument,
    path: &Path,
    units: Units,
    axis: Axis,
    stage: Option<usize>,
    component: Option<usize>,
    fittings: bool,
) -> Result<()> {
    if component.is_some() && stage.is_none() {
        return Err("A component export requires its stage index".into());
    }
    let parts: Vec<_> = model
        .parts
        .iter()
        .filter(|p| {
            stage.is_none_or(|s| p.stage == s)
                && component.is_none_or(|i| p.component_index == Some(i))
                && (fittings || !matches!(p.kind.as_str(), "seam" | "retainer" | "nozzle"))
        })
        .collect();
    if parts.is_empty() {
        return Err("No matching evaluated geometry to export".into());
    }
    let mut source = model.clone();
    source.parts = parts.into_iter().cloned().collect();
    let mut prepared = source.clone();
    for p in &mut prepared.parts {
        for v in &mut p.positions {
            *v = (axis.to_file(DVec3::from_array(v.map(f64::from))) / units.meters())
                .as_vec3()
                .to_array();
        }
        for n in &mut p.normals {
            *n = axis
                .to_file(DVec3::from_array(n.map(f64::from)))
                .as_vec3()
                .to_array();
        }
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let ext = extension(path);
    match ext.as_str() {
        "obj" => {
            let mut file = std::io::BufWriter::new(fs::File::create(path)?);
            writeln!(file, "# Rocket Lab; units {}; nose axis {}", units.label(), axis.label())?;
            let mut offset = 1u32;
            for p in &prepared.parts {
                writeln!(file, "o {}", p.name.replace(['\n','\r',' '], "_"))?;
                for v in &p.positions { writeln!(file, "v {} {} {}", v[0], v[1], v[2])?; }
                for n in &p.normals { writeln!(file, "vn {} {} {}", n[0], n[1], n[2])?; }
                for f in p.indices.chunks_exact(3) {
                    writeln!(file, "f {0}//{0} {1}//{1} {2}//{2}", f[0]+offset, f[1]+offset, f[2]+offset)?;
                }
                offset += p.positions.len() as u32;
            }
            file.flush()?;
        }
        "stl" => {
            let mut file = std::io::BufWriter::new(fs::File::create(path)?);
            let mut header = [0u8;80];
            let label = format!("Rocket Lab; units {}; nose {}", units.label(), axis.label());
            header[..label.len()].copy_from_slice(label.as_bytes());
            file.write_all(&header)?;
            let count = prepared.parts.iter().map(|p| p.indices.len()/3).sum::<usize>();
            let count = u32::try_from(count).map_err(|_| "Too many triangles for STL")?;
            file.write_all(&count.to_le_bytes())?;
            for p in &prepared.parts {
                for f in p.indices.chunks_exact(3) {
                    let raw = [f[0],f[1],f[2]].map(|i| p.positions[i as usize]);
                    let [a,b,c] = raw.map(|v| DVec3::from_array(v.map(f64::from)));
                    let normal = (b-a).cross(c-a).normalize_or_zero().as_vec3().to_array();
                    for value in normal.into_iter().chain(raw.into_iter().flatten()) {
                        file.write_all(&value.to_le_bytes())?;
                    }
                    file.write_all(&[0,0])?;
                }
            }
            file.flush()?;
        }
        "json" => fs::write(path, serde_json::to_vec(&source)?)?,
        "step" | "stp" | "iges" | "igs" | "brep" | "brp" | "fcstd" | "glb" | "gltf" | "ply" | "blend" | "fbx" | "dae" => {
            converter(serde_json::json!({"operation":"export","output":std::path::absolute(path)?,"model":source,"units":units.meters(),"axis":axis,"format":ext}), cad_format(path))?;
        }
        _ => return Err("Export format must be STL, OBJ, GLB, glTF, PLY, BLEND, FBX, DAE, STEP, IGES, BREP, FCStd or JSON".into()),
    }
    Ok(())
}
