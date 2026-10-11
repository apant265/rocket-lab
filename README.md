# Rocket Lab

A Rust six-degree-of-freedom flight engine with a native Bevy 3D laboratory for collegiate high-power and sounding-rocket studies. The physics library runs without a graphics stack. The viewer plays exactly the same computed flight, with scrubbing, plots, staging, telemetry, reference overlays, a mission editor and CSV/JSON export.

This is a working engineering prototype, **not a validated replacement for RASAero II or a complete hypersonic vehicle-analysis suite**. The HPR and sounding demos use explicitly synthetic motor and aerodynamic curves. The additional Plumbbob example uses supplied motor curves and older CDX1 masses, with prominently labeled provisional aerodynamics and mass-distribution assumptions. None of these results validates the newer Plumbbob trajectory export.

**Simulate** includes timestamp-based CFD, thermal and whole-vehicle stress analysis with actual solver refinement. **Built-in** analysis defaults to hardware GPU flow and material heat diffusion, with the actual adapter shown; GPU errors stop the job. **External (CPU)** runs OpenFOAM v2406 for compressible flow and wall heat diffusion, and CalculiX for section-specific quadratic vehicle shells and primary fin solids. Flight dynamics and stress integration use the CPU. [Setup, scope and analytical checks](docs/external-solvers.md). Step backward/forward, scrub, or enter an exact time; choose Preview, Standard, Fine or Ultra and **Compute**. Calculations run in the background with elapsed time and approximate remaining time, and export their captured timestamp and numerical settings. Drag either sidebar's inner edge or the bottom panel's top edge to resize; charts and the parts grid adapt. [Timestamp analysis and computational detail](docs/timestamp-analysis.md). Advanced recovery, weather, imported data and numerical checks remain available through mission configuration and CLI commands. [Capabilities and model limits](docs/advanced-laboratory.md). Build a runnable Windows bundle with `./scripts/package.ps1`; its `Launch.cmd` starts the 5 lb Kate and Anvil study without Rust installed.

## Run

From this directory:

```powershell
# Native flight laboratory
cargo run --features viewer -- view --mission examples/hpr.json

# Three-stage demonstration crossing Mach 5
cargo run --features viewer -- view --mission examples/sounding.json

# Supplied Plumbbob motor/mass inputs, with provisional aero and inertia
cargo run --features viewer -- view --mission examples/plumbbob-provisional.json

# Headless flight and output files
cargo run -- export-model --mission examples/kate-anvil-desert.json --out output/model-flight --simulate
cargo run -- simulate --mission examples/hpr.json --out output/hpr
cargo run -- simulate --mission examples/sounding.json --out output/sounding

# Repeatable dispersion and comparison with your RASAero flight export
cargo run -- dispersion --mission examples/hpr.json --runs 20 --seed 42
cargo run -- compare --mission examples/sounding.json --reference "sources/Plumbbob_MKIII_R75 PDR Flight Data.CSV"

cargo test
```

Alternatively run `./Launch.ps1` (Kate and Anvil, 5 lb desert study), `./Launch.ps1 -Hpr`, or `./Launch.ps1 -Sounding`. First compilation of the optional renderer can take several minutes. Rust 1.91.1 on Windows is supported; Bevy 0.18.1 and bevy_egui 0.39.1 are pinned to compatible major/minor releases for this installed toolchain. Commit `Cargo.lock` for reproducibility. A working Windows graphics driver and Rust's MSVC linker are required for the viewer.

The app opens in **Build**, with Blender-rendered parts cards, drag/drop placement previews and an immediately updated 3D draft. Drag cards from **Parts** into 3D, then click the placed body, nose, fin or component to edit its properties. Body tubes snap above or below the pointed tube and extend the stage; a nose attaches at the top. Each explicit section has length, diameter and axial offset properties. Resizing moves downstream sections without scaling them. Original measured dry mass stays in the hardware allocation when converting the shell into separate parts. A transition creates a new booster immediately below the stage you drop it onto. Enable **Internals** to select motors, payloads and packed recovery; visible preview canopies also open recovery settings. A theme-colored selection box marks the selected component and follows its edited dimensions. **Launch simulation** validates the design and runs Blender headless when geometry needs rebuilding, then switches to **Simulate**, which contains launch conditions, telemetry, plots and playback. The native interface uses `bevy_egui`. **Export 3D model** writes Blender/GLB geometry and captures mesh-derived mass, 3D centroid, full inertia and crossflow projected shape. Launch regenerates legacy models and uses these properties. [Model-based flight and remaining approximations](docs/model-flight.md) explain measured mass overrides, internal hardware and aerodynamic inputs.

Parts now uses **Body & fins**, **Inner components**, **Mass & recovery**, and **Assemblies**, with the separate Booster stage card removed. Includes elliptical/freeform/tube fins, launch lug, engine block, streamer and repeated pod housings. Freeform fins have an outline point editor; tube fins have automatic touching-tube sizing. Added components expose material/density, mass/CG overrides, offsets, rotation, color, finish and comments. The [builder guide](docs/builder-and-flight.md#openrocket-style-parts) describes controls and the remaining differences from OpenRocket.

Choose **Imperial** or **Metric** in the header for display units; properties support dragging, typing and +/− steps. Explanations appear on hover. [Vehicle stress, material data and units](docs/vehicle-stress-and-units.md). Choose **Dark** or **Light** in the header for neutral charcoal or warm-white styling; the preference is saved in `output/ui-theme.json`. **Reset rocket** restores the loaded design, rewinds the flight and centers the camera. Undo restores your draft edits. **Rewind flight** (or Home) pauses playback at launch without changing the design.

Hold **left or right click** and drag in the 3D viewport to move the camera position. **Shift + right click** provides mouse-look; **WASD** moves the camera and **Q/E** moves down/up while held; the **mouse wheel** changes camera distance. **Vehicle** restores close-up inspection; **Fit flight** frames the whole trajectory. Cyan shows the flown path and dashed amber shows the remaining computed trajectory. Space toggles playback; Home resets. Vehicle models are enlarged for visibility; trajectories use kilometers. [Builder, weather and camera guide](docs/builder-and-flight.md).

**2D analysis images:** open Simulate Ã¢â€ â€™ Analysis to view full-size computed pressure, speed and Mach fields, wall temperature sections/heating histories, or the whole-vehicle stress envelope and separate fin envelope. Wheel zooms, drag pans and hover reports values. Save PNG + metadata exports units and captured/viewed timestamps. Switch to **3D vehicle** to restore the rocket. [2D viewer and export](docs/timestamp-analysis.md#large-2d-result-images).

**Surface laboratory:** switch between Model, Pressure, Airflow, Thermal and Stress. Choose a timestamp and Compute to generate a material or whole-vehicle stress map. External thermal runs OpenFOAM material-region diffusion on the CPU with CSV export. The separate fast GPU/CPU preview includes estimated fin-root and component contacts. Both expose their actual backend and scope. The descent model now permits physical tail-first instability and includes an effective parachute suspension torque; **Apogee** jumps to the computed event. [Thermal and descent model](docs/thermal-and-descent.md). Pressure colors use a labeled windward-incidence approximation, with a converted absolute-pressure legend. The airflow heatmap shows tangential freestream incidence; animated paths show geometric deflection and direction. Inspect the whole active stack, forward body, fins or rounded nose tip. Enable the Mach-angle guide in supersonic flight. Bundled meshes were generated in headless Blender and include native `.blend` and GLB assets. [Surface modeling assumptions, rebuild commands and CSV export](docs/surface-visualization.md).

```powershell
./scripts/Build-Models.ps1
cargo run --features viewer -- view --mission examples/plumbbob-provisional.json --start-time 5
cargo run -- surface --mission examples/plumbbob-provisional.json --time 4 --out output/surface
```

## What is implemented

| Area | Model |
|---|---|
| Rigid body | f64 translation, normalized quaternion, body angular velocity, Euler rotational dynamics with full inertia tensor |
| Integration | RK4, motor/time-event step splitting, damping and canopy-drag step bounds; threshold events evaluated at integration boundaries |
| Environment | Spherical Earth altitude and inverse-square gravity, optional Coriolis/centrifugal terms, standard-atmosphere or measured-site dry-air lapse profile, wind bearing/speed, shear and deterministic gusts; visual cloud layer |
| Builder | Component tree, conical/ogive/elliptical nose, trapezoidal fin sets, measured mass and CG, motor library, staging/recovery/avionics controls, headless Blender rebuild, project JSON save/load |
| Vehicle mass | Dry structure, payload/avionics allocation and remaining propellant, mesh-derived dry centroid/inertia for model flights; lumped motor/payload and legacy cylinder models with parallel-axis terms |
| Propulsion | Piecewise-linear thrust curves, impulse-proportional propellant depletion, motor misalignment and thrust-line offsets |
| Aero | Per-stack Mach tables, power-on/off drag, normal-force derivative, CP movement, bounded high-angle extension, pitch/yaw/roll damping and fin-cant roll forcing |
| Staging | Burnout plus delay, absolute time, ascending Mach or altitude triggers, upper-stage ignition delay and altitude/tilt/power interlocks |
| Detached stages | Independent rigid-body ballistic propagation, mass retained if separated before burnout, momentum-balanced separation impulse and CG repositioning; exported history and 3D bodies |
| Recovery | Sensor-commanded drogue and altitude-commanded main, linear canopy inflation, Mach/q deployment interlocks, line-load failure, main replacing drogue |
| Flight computer | Sampled filtered altitude, barometer/GNSS ceilings, deterministic sensor disturbance, latency queue, descent confirmation, IMU range flags, power budget and pyro interlocks |
| Loads and heating | Dynamic pressure, specific force, Reynolds/Knudsen numbers, legacy lumped flight temperature plus independent spatial GPU/CPU wall diffusion and structural screening |
| Analysis | Flight/event logs, source inspection, imperial-to-SI reference conversion, overlap RMSE comparison, seeded parameter dispersion |
| Surface visualization | Evaluated Blender geometry, pressure-incidence colors, labeled geometric airflow paths, Mach-angle guide and per-vertex CSV export; independent of flight forces |

Subsonic, transonic, supersonic and hypersonic regimes use one continuous set of equations and coefficient interpolation; crossing a regime does not reset the state. There is no software restriction that prohibits staging in a particular regime. The aerodynamic table must describe the **entire remaining stack at that stage**, referenced to its diameter and measured from its nose. This permits externally generated RASAero/CFD/wind-tunnel coefficients to supply transonic drag peaks and high-speed CP changes without inventing a universal curve.

## Import real inputs

All mission values are SI. Stages are ordered **bottom booster first, top sustainer last**, zero-indexed in CLI operations. Stage lengths add aft of the top nose. Dry mass includes empty motor hardware, parachutes, electronics and fixtures; use `payload_mass_kg` only for mass not already counted. CG stations are measured aft of that stage's own nose. Aerodynamic CP stations are measured aft of the active stack's nose.

```powershell
# One RASP motor per file: imported curve is in seconds/Newtons, propellant in kg.
# Supply motor CG explicitly and include casing mass in the stage dry mass.
cargo run -- motor --mission examples/hpr.json --eng examples/demo.eng --stage 0 --cg 1.8 --out output/with-motor.json

# Select a named entry from the supplied multi-motor RASP database
cargo run -- motor --mission examples/plumbbob-provisional.json --eng sources/rasp.eng --name O6000W --stage 0 --cg 1.18 --out output/with-real-motor.json

# Explicit normalized table, not an arbitrary RASAero trajectory CSV
cargo run -- aero --mission examples/hpr.json --csv examples/aero-si.csv --stage 0 --source "Complete vehicle / named run / documented reference area" --out output/with-aero.json

cargo run -- inspect "sources/Plumbbob_MKIII_R75.CDX1"
cargo run -- inspect "sources/Plumbbob_MKIII_R75 (1).ork"
cargo run -- reference "sources/Plumbbob_MKIII_R75 PDR Flight Data.CSV"
```

Aero CSV columns: `mach,cd_power_off,cd_power_on,cn_alpha_per_rad,cp_m`. Mach must strictly increase from zero. Include every regime needed for the flight. Outside the table, endpoint coefficients are held and a warning is recorded. Damping coefficients are dimensionless positive magnitudes referenced to active stack length, with moment scale `q A LÃ‚Â² / (2 V)`; they are engineering placeholders unless measured. Full import semantics are in [docs/model.md](docs/model.md).

The RASAero and OpenRocket loaders inspect geometry, component data, motor designations and saved outputs. `import-ork` additionally maps supported native components onto an explicit base mission, with an explicit stage map and an omissions report; it preserves base motors/recovery and does not claim complete native configuration parity. RASP and RSE thrust curves are supported. `cargo run --example plumbbob` regenerates the provisional example from CDX1 and named RASP motors; its assumptions are recorded in the mission description and source review. The supplied flight CSV and partial CFD table are rejected by the aero importer. Read [the source review](docs/source-review.md) before using these data for validation.

## Output and verification

Each flight writes `telemetry.csv`, `mission.json`, `summary.json`, `events.json` and, when applicable, `detached-stages.csv`. Temperature and force warnings are screening results; a warning does not automatically destroy the rigid body. `completed` means the retained vehicle landed; detached recovery status is recorded separately in its CSV and events. A duration timeout remains a partial flight. Discarded motors are quenched; configured boosters have independent recovery systems. The solver continues detached bodies after retained touchdown until all bodies land or duration expires.

Tests cover atmosphere checkpoints, inverse-square gravity, motor impulse, changing mass/CG/inertia, quaternion rotation, vacuum energy and angular-momentum conservation, separation conservation, restoring moments, all four regimes, staged ignition, complete parachute recovery, power/sensor/deployment failures, repeatability, timestep convergence, input validation and supplied-source unit conversions. See [docs/validation.md](docs/validation.md) for observed runs and limitations.

The important unfinished models are real-gas chemistry, rarefied-flow aerodynamics, a validated thermosphere, ablation, 3D conjugate heat transfer, coupled material failure and fracture, aeroelastic flutter, flexible bodies, tether/canopy dynamics, collision/contact of stages, thrust-vector control, firmware/HIL execution and receiver-specific avionics dynamics. Whole-vehicle section stress and material diffusion are available as post-processing, with their scope in [vehicle stress and units](docs/vehicle-stress-and-units.md). A real vehicle requires validated configuration-specific input data and comparisons with independent flight measurements.




## Parts, parachutes and satellite maps

The default launcher now opens the 5 lb Kate and Anvil desert study. Build supports a draggable parts palette, materials/density, wall thickness and mass/CG overrides; mass displays use pounds. Each booster can deploy its own vented gore canopy with suspension lines. Satellite maps support NASA imagery without a key, Google Maps Static API with a configured key, and georeferenced local PNG/JPEG files. Tracks and landing points export to `trajectory.geojson`. See [the workflow and exact limits](docs/assembly-recovery-maps.md).
