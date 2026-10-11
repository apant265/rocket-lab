use clap::{Parser, Subcommand};
use rocket_lab::{config::*, export, flight::Simulator, imports};
use std::{fs, path::PathBuf};
#[derive(Parser)]
#[command(version, about = "Rocket Lab | Rust 6DoF flight laboratory (SI units)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Import STL/OBJ or CAD/scene geometry as an editable stage component.
    ImportGeometry {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long, default_value_t = 0)]
        stage: usize,
        #[arg(long)]
        replace: bool,
        #[arg(long, value_enum, default_value = "millimeters")]
        units: rocket_lab::exchange::Units,
        #[arg(long, value_enum, default_value = "z")]
        axis: rocket_lab::exchange::Axis,
        #[arg(long, default_value_t = 0.1)]
        tolerance_mm: f64,
        #[arg(long)]
        out: PathBuf,
    },
    /// Export evaluated rocket geometry to a mesh or faceted CAD format.
    ExportGeometry {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long)]
        model: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        stage: Option<usize>,
        #[arg(long, requires = "stage")]
        component: Option<usize>,
        #[arg(long)]
        no_fittings: bool,
        #[arg(long, value_enum, default_value = "millimeters")]
        units: rocket_lab::exchange::Units,
        #[arg(long, value_enum, default_value = "z")]
        axis: rocket_lab::exchange::Axis,
    },
    /// Export an evaluated Blender/GLB model; optionally run its mesh-based flight.
    ExportModel {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long, default_value = "output/model-flight")]
        out: PathBuf,
        #[arg(long)]
        simulate: bool,
    },
    /// Run actual OpenCFD v2406 in Docker at a flight timestamp.
    External {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long, default_value_t = 4.0)]
        time: f64,
        #[arg(long, default_value = "flow", value_parser = ["flow", "thermal", "stress"])]
        analysis: String,
        #[arg(long, default_value = "preview", value_parser = ["preview", "standard", "fine", "ultra"])]
        detail: String,
        #[arg(long, default_value_t = 0.3)]
        poisson: f64,
        /// Override flow iteration budget for an explicit solver check.
        #[arg(long)]
        iterations: Option<usize>,
        #[arg(long, default_value = "output/openfoam")]
        out: PathBuf,
    },
    /// Import mapped OpenRocket components onto an explicit base mission.
    ImportOrk {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        ork: PathBuf,
        #[arg(long)]
        stage_map: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Experimental planar perfect-gas Euler CFD; no viscous/3D heating claim.
    Cfd {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long, default_value_t = 0)]
        stage: usize,
        #[arg(long, default_value_t = 3.0)]
        mach: f64,
        #[arg(long, default_value_t = 0.0)]
        alpha: f64,
        #[arg(long, default_value_t = 15000.0)]
        altitude: f64,
        #[arg(long, default_value_t = 192)]
        nx: usize,
        #[arg(long, default_value_t = 128)]
        ny: usize,
        #[arg(long, default_value_t = 3000)]
        steps: usize,
        #[arg(long)]
        gpu: bool,
        #[arg(long, default_value = "output/cfd")]
        out: PathBuf,
    },
    /// Independent exact Sod shock-tube check of the finite-volume flux.
    CfdCheck {
        #[arg(long, default_value_t = 400)]
        cells: usize,
    },
    /// Check dt, dt/2, dt/4 convergence and optionally an independent trajectory.
    Validate {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long)]
        reference: Option<PathBuf>,
        #[arg(long, default_value = "output/validation")]
        out: PathBuf,
        #[arg(long, default_value_t = 5.0)]
        tolerance_percent: f64,
    },
    /// Attach geographic ESRI ASCII DEM or imported environment JSON to a mission.
    Environment {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long)]
        profile: Option<PathBuf>,
        #[arg(long)]
        dem: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Solve material-dependent wall diffusion over a complete flight (CPU f64).
    Thermal {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long, default_value = "output/thermal-cpu")]
        out: PathBuf,
    },
    /// Download NASA true-color imagery and georeferencing into output/site (viewer feature).
    #[cfg(feature = "viewer")]
    Satellite {
        #[arg(long, allow_negative_numbers = true)]
        latitude: f64,
        #[arg(long, allow_negative_numbers = true)]
        longitude: f64,
        #[arg(long, default_value_t = 160.0)]
        span_km: f64,
        #[arg(long, default_value = "2025-06-15")]
        date: String,
    },
    /// Write a documented synthetic vehicle configuration.
    Demo {
        #[arg(long)]
        hypersonic: bool,
        #[arg(long, default_value = "mission.json")]
        out: PathBuf,
    },
    /// Integrate a full flight and export CSV telemetry plus JSON events/summary.
    Simulate {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long, default_value = "output/flight")]
        out: PathBuf,
        #[arg(long)]
        dt: Option<f64>,
        /// Evaluated rocket.mesh.json from Blender; supplies dry mass/inertia and crossflow shape.
        #[arg(long)]
        model: Option<PathBuf>,
    },
    /// Open the Bevy 3D flight viewer (requires --features viewer).
    View {
        #[arg(long)]
        mission: Option<PathBuf>,
        #[arg(long)]
        hypersonic: bool,
        #[arg(long)]
        smoke_test: bool,
        /// Open the 2D Analysis view and compute CFD at the initial timestamp.
        #[arg(long)]
        analysis_view: bool,
        /// Time to inspect initially; playback starts paused.
        #[arg(long, default_value_t = 4.0)]
        start_time: f64,
        /// Evaluated Blender mesh JSON; auto-select a matching bundled model otherwise.
        #[arg(long)]
        model: Option<PathBuf>,
    },
    /// Export an estimated surface pressure map on the evaluated Blender geometry.
    Surface {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long, default_value = "assets/models/rocket.mesh.json")]
        model: PathBuf,
        #[arg(long, default_value_t = 4.0)]
        time: f64,
        #[arg(long, default_value = "output/surface")]
        out: PathBuf,
    },
    /// Inspect RASAero .CDX1 or zipped OpenRocket .ork; no silent model conversion.
    Inspect {
        path: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Attach a complete SI aero table to one remaining-stack configuration.
    Aero {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long)]
        csv: PathBuf,
        #[arg(long)]
        stage: usize,
        #[arg(long)]
        source: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Attach one RASP .eng thrust curve. Stage dry mass must already include motor casing.
    Motor {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long)]
        eng: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        stage: usize,
        #[arg(long)]
        cg: f64,
        #[arg(long)]
        out: PathBuf,
    },
    /// Summarize the supplied RASAero flight export after converting imperial units.
    Reference { path: PathBuf },
    /// Compare an independent simulation with a RASAero trajectory export.
    Compare {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long)]
        reference: PathBuf,
        #[arg(long, default_value = "output/comparison")]
        out: PathBuf,
    },
    /// Run seeded parameter dispersion (demonstration uncertainty assumptions).
    Dispersion {
        #[arg(long)]
        mission: PathBuf,
        #[arg(long, default_value_t = 20)]
        runs: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[arg(long, default_value = "output/dispersion")]
        out: PathBuf,
    },
}
fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
fn print(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
fn run() -> Result<()> {
    match Cli::parse().command {
        Command::ImportGeometry {
            mission,
            input,
            stage,
            replace,
            units,
            axis,
            tolerance_mm,
            out,
        } => {
            let mut m = Mission::load(mission)?;
            let geometry = rocket_lab::exchange::import(&input, units, axis, tolerance_mm)?;
            let index = rocket_lab::exchange::insert(&mut m, stage, geometry, replace)?;
            m.save(&out)?;
            println!(
                "Imported stage {} component {} into {}",
                stage,
                index,
                out.display()
            );
        }
        Command::ExportGeometry {
            mission,
            model,
            out,
            stage,
            component,
            no_fittings,
            units,
            axis,
        } => {
            let mut m = Mission::load(mission)?;
            rocket_lab::design::initialize_geometry(&mut m);
            let model = if let Some(path) = model {
                rocket_lab::surface::MeshDocument::load(path, &m)?
            } else {
                let directory = std::env::current_dir()?.join(format!(
                    "output/exchange/cli-export-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_nanos()
                ));
                rocket_lab::model_physics::export_model(&m, &directory)?;
                rocket_lab::surface::MeshDocument::load(directory.join("rocket.mesh.json"), &m)?
            };
            rocket_lab::exchange::export(
                &model,
                &out,
                units,
                axis,
                stage,
                component,
                !no_fittings,
            )?;
            println!("Exported {}", out.display());
        }
        Command::External {
            mission,
            time,
            analysis: mode,
            detail,
            poisson,
            iterations,
            out,
        } => {
            use rocket_lab::{analysis, cfd, external};
            let mut m = Mission::load(mission)?;
            for s in &mut m.stages {
                if s.geometry.is_none() {
                    s.geometry = Some(StageGeometry::generic(s, true));
                }
            }
            let f = Simulator::new(m)?.run();
            let sample = analysis::sample_at(&f, time);
            let level = ["preview", "standard", "fine", "ultra"]
                .iter()
                .position(|s| *s == detail)
                .unwrap();
            let detail = [
                rocket_lab::analysis_detail::Detail::Preview,
                rocket_lab::analysis_detail::Detail::Standard,
                rocket_lab::analysis_detail::Detail::Fine,
                rocket_lab::analysis_detail::Detail::Ultra,
            ][level];
            let resolution = detail.thermal();
            let prefix = analysis::prefix(&f, sample.time_s, detail.boundary_step());
            std::fs::create_dir_all(&out)?;
            let backend = if mode == "flow" {
                let (grid, budget) = detail.flow();
                let p = cfd::prepare(
                    &f.mission,
                    cfd::Request {
                        stage: sample.stage,
                        altitude_m: sample.altitude_m,
                        mach: sample.mach,
                        alpha_deg: sample.alpha_deg,
                        grid,
                        steps: iterations.unwrap_or(budget),
                    },
                )?;
                let flow = external::flow(p, &out)?;
                flow.export(&out)?;
                flow.backend
            } else {
                let h = external::heat(
                    &prefix,
                    resolution,
                    detail.wall_layers(),
                    detail.boundary_step(),
                    &out.join("thermal"),
                )?;
                h.export(out.join("thermal"))?;
                if mode == "stress" {
                    let (rows, backend) = external::stress(
                        &prefix,
                        &h,
                        detail.stress(),
                        poisson,
                        &out.join("stress"),
                    )?;
                    std::fs::write(out.join("stress.json"), serde_json::to_vec_pretty(&rows)?)?;
                    let mut vehicle = external::vehicle_stress(
                        &prefix,
                        &h,
                        detail.stress(),
                        &out.join("vehicle-stress"),
                    )?;
                    rocket_lab::vehicle_stress::merge_primary_fins(
                        &prefix,
                        &h,
                        &mut vehicle,
                        &rows,
                    );
                    vehicle.backend.push_str(&format!("; {backend}"));
                    std::fs::write(
                        out.join("vehicle-stress.json"),
                        serde_json::to_vec_pretty(&vehicle)?,
                    )?;
                    vehicle.backend
                } else {
                    h.backend
                }
            };
            let report = serde_json::json!({"backend":backend,"timestamp_s":sample.time_s,"detail":detail.label().to_ascii_lowercase(),"output":out});
            std::fs::write(
                out.join("external-summary.json"),
                serde_json::to_vec_pretty(&report)?,
            )?;
            print(&report)?;
        }
        Command::ImportOrk {
            base,
            ork,
            stage_map,
            out,
        } => {
            let (m, r) = rocket_lab::ork::import(ork, &Mission::load(base)?, &stage_map)?;
            m.save(out)?;
            print(&r)?;
        }
        Command::CfdCheck { cells } => {
            if !(100..=4000).contains(&cells) {
                return Err("Sod check cells must be 100-4000".into());
            }
            let report = rocket_lab::cfd::sod_check(cells);
            print(&report)?;
            if !report.passed {
                return Err("Shock tube acceptance check failed".into());
            }
        }
        Command::Cfd {
            mission,
            stage,
            mach,
            alpha,
            altitude,
            nx,
            ny,
            steps,
            gpu,
            out,
        } => {
            let m = Mission::load(mission)?;
            let p = rocket_lab::cfd::prepare(
                &m,
                rocket_lab::cfd::Request {
                    stage,
                    altitude_m: altitude,
                    mach,
                    alpha_deg: alpha,
                    grid: [nx, ny],
                    steps,
                },
            )?;
            let solution = if gpu {
                #[cfg(feature = "viewer")]
                {
                    rocket_lab::cfd_gpu::solve(p)?
                }
                #[cfg(not(feature = "viewer"))]
                {
                    return Err("GPU CFD requires --features viewer".into());
                }
            } else {
                rocket_lab::cfd::solve(p)?
            };
            solution.export(&out)?;
            println!("{}; {}", solution.backend, out.display());
        }
        Command::Validate {
            mission,
            reference,
            out,
            tolerance_percent,
        } => {
            let m = Mission::load(mission)?;
            print(&rocket_lab::validation::convergence(&m, &out)?)?;
            if let Some(path) = reference {
                let r = imports::reference_flight(path)?;
                let f = Simulator::new(m)?.run();
                export::write_flight(&f, &out)?;
                print(&rocket_lab::validation::reference(
                    &f,
                    &r,
                    &out,
                    tolerance_percent,
                )?)?;
            }
        }
        Command::Environment {
            mission,
            profile,
            dem,
            out,
        } => {
            let mut m = Mission::load(mission)?;
            if let Some(p) = profile {
                m.environment = serde_json::from_str(&fs::read_to_string(p)?)?;
            }
            if let Some(p) = dem {
                m.environment.terrain = Some(rocket_lab::environment::import_ascii(p)?);
            }
            m.validate()?;
            m.save(out)?;
        }
        Command::Thermal { mission, out } => {
            let flight = Simulator::new(Mission::load(mission)?)?.run();
            let start = std::time::Instant::now();
            let problem = rocket_lab::thermal::prepare(&flight);
            let history = rocket_lab::thermal::solve(&problem);
            history.export(&out)?;
            fs::write(
                out.join("structures.json"),
                serde_json::to_string_pretty(&rocket_lab::engineering::screen(&flight, &history))?,
            )?;
            println!(
                "{} thermal columns, {} frames in {:.2} s; {}",
                history.columns.len(),
                history.times.len(),
                start.elapsed().as_secs_f64(),
                out.display()
            );
        }
        #[cfg(feature = "viewer")]
        Command::Satellite {
            latitude,
            longitude,
            span_km,
            date,
        } => {
            let data = rocket_lab::satellite::fetch(rocket_lab::satellite::Request {
                source: 0,
                latitude,
                longitude,
                span_km,
                date,
                image_path: String::new(),
                bounds: rocket_lab::satellite::Bounds {
                    west: 0.0,
                    south: 0.0,
                    east: 1.0,
                    north: 1.0,
                    mercator: false,
                },
            })?;
            println!(
                "Downloaded {} × {} satellite image into output/site. {}",
                data.width, data.height, data.attribution
            );
        }
        Command::Demo { hypersonic, out } => {
            let m = Mission::demo(hypersonic);
            m.validate()?;
            m.save(&out)?;
            println!("Wrote {}", out.display());
        }
        Command::ExportModel {
            mission,
            out,
            simulate,
        } => {
            let mut m = Mission::load(&mission)?;
            rocket_lab::design::initialize_geometry(&mut m);
            rocket_lab::model_physics::export_model(&m, &out)?;
            let model = rocket_lab::surface::MeshDocument::load(out.join("rocket.mesh.json"), &m)?;
            rocket_lab::model_physics::apply(&model, &mut m)?;
            m.save(out.join("simulation-mission.json"))?;
            if simulate {
                let result = Simulator::new(m)?.run();
                export::write_flight(&result, out.join("flight"))?;
                print(&result.summary)?;
            }
            println!(
                "3D model and flight properties exported to {}",
                out.display()
            );
        }
        Command::Simulate {
            mission,
            out,
            dt,
            model,
        } => {
            let mut m = Mission::load(mission)?;
            if let Some(dt) = dt {
                m.dt = dt;
            }
            if let Some(path) = model {
                let model = rocket_lab::surface::MeshDocument::load(path, &m)?;
                rocket_lab::model_physics::apply(&model, &mut m)?;
                fs::create_dir_all(&out)?;
                m.save(out.join("simulation-mission.json"))?;
            }
            let result = Simulator::new(m)?.run();
            export::write_flight(&result, out)?;
            print(&result.summary)?;
        }
        Command::View {
            mission,
            hypersonic,
            smoke_test,
            analysis_view,
            start_time,
            model,
        } => {
            let m = if let Some(path) = mission {
                Mission::load(path)?
            } else {
                Mission::demo(hypersonic)
            };
            m.validate()?;
            #[cfg(feature = "viewer")]
            rocket_lab::viewer::launch(m, smoke_test, start_time, model, analysis_view)?;
            #[cfg(not(feature = "viewer"))]
            {
                let _ = (m, smoke_test, start_time, model, analysis_view);
                return Err(
                    "Run cargo run --features viewer -- view [--mission examples/hpr.json]".into(),
                );
            }
        }
        Command::Surface {
            mission,
            model,
            time,
            out,
        } => {
            let m = Mission::load(mission)?;
            let mesh = rocket_lab::surface::MeshDocument::load(model, &m)?;
            let flight = Simulator::new(m.clone())?.run();
            if !time.is_finite() || time < 0.0 || time > flight.summary.end_time_s {
                return Err("Surface time must fall within the flight".into());
            }
            let sample = flight
                .samples
                .iter()
                .rev()
                .find(|s| s.time_s <= time)
                .expect("flight starts at zero");
            rocket_lab::surface::write_map(&mesh, &m, sample, &out)?;
            println!(
                "Saved estimated surface map at T+ {:.3} s to {}",
                sample.time_s,
                out.display()
            );
        }
        Command::Inspect { path, out } => {
            let data = imports::inspect_design(path)?;
            if let Some(out) = out {
                fs::write(out, serde_json::to_string_pretty(&data)?)?;
            }
            print(&data)?;
        }
        Command::Aero {
            mission,
            csv,
            stage,
            source,
            out,
        } => {
            let mut m = Mission::load(mission)?;
            let s = m
                .stages
                .get_mut(stage)
                .ok_or("stage index out of range (zero-based)")?;
            s.aero = imports::import_aero(csv)?;
            s.aero_source = source;
            m.validate()?;
            m.save(out)?;
        }
        Command::Motor {
            mission,
            eng,
            name,
            stage,
            cg,
            out,
        } => {
            let mut m = Mission::load(mission)?;
            let s = m
                .stages
                .get_mut(stage)
                .ok_or("stage index out of range (zero-based)")?;
            let mut motor = imports::import_motor_named(eng, name.as_deref())?;
            motor.cg_m = cg;
            s.motor = motor;
            m.validate()?;
            m.save(out)?;
        }
        Command::Reference { path } => {
            let rows = imports::reference_flight(path)?;
            print(
                &serde_json::json!({"samples":rows.len(),"max_altitude_agl_m":rows.iter().map(|p|p.altitude_agl_m).fold(0.0,f64::max),"max_mach":rows.iter().map(|p|p.mach).fold(0.0,f64::max),"max_speed_mps":rows.iter().map(|p|p.speed_mps).fold(0.0,f64::max)}),
            )?;
        }
        Command::Compare {
            mission,
            reference,
            out,
        } => {
            let rows = imports::reference_flight(reference)?;
            let result = Simulator::new(Mission::load(mission)?)?.run();
            export::write_flight(&result, &out)?;
            let report = export::compare(&result, &rows);
            fs::write(
                out.join("comparison.json"),
                serde_json::to_string_pretty(&report)?,
            )?;
            print(&report)?;
        }
        Command::Dispersion {
            mission,
            runs,
            seed,
            out,
        } => {
            print(&export::dispersion(
                &Mission::load(mission)?,
                runs,
                seed,
                out,
            )?)?;
        }
    }
    Ok(())
}
