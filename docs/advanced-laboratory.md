# Advanced laboratory and completion status

This extension adds working analysis tools to the Rust/Bevy simulator. Run `rocket-lab.exe view --mission examples/advanced-laboratory.json` to exercise the new recovery, environment and material options, or use the default **Kate + Anvil / desert** for the supplied three-stage study and its 5 lb payload. Mass controls use pounds; saved missions and solver APIs use SI.

Use the timestamp analysis controls in **Simulate** for computed flow, thermal diffusion and whole-vehicle section stress. [Detail presets and seeking](timestamp-analysis.md), [material properties, units and structural scope](vehicle-stress-and-units.md). Flight checks and data imports are available through the CLI and saved mission configuration. Long calculations run on a worker while the viewport remains responsive. Inspect the calculation report and exported files before interpreting a picture as a converged solution.

## Delivered capabilities

| Feature | Current behavior |
|---|---|
| Builder | Blender-rendered model cards, Parts cards with held drag/drop onto the stage axis, triangle-based selection in 3D, transition-created boosters, immediate geometry, editable dimensions/density/mass/CG, independent tube/nose/transition/fin components, undo/redo, save/load and headless Blender rebuild |
| Camera and flight | Held left/right drag translates the camera; Shift-right drag looks around; zoom and Fit flight show flown and future arcs; rotation follows integrated rigid-body dynamics |
| Flight physics | f64 RK4 translation/quaternion/Euler rotation, fuel-dependent mass/CG/inertia, four Mach regimes, detached bodies and configurable staging, imported coefficient grids and optional rarefied-flow screening bridge |
| Recovery | Inflated vented mesh and suspension lines for each stage; reefing, opening factors, deployment interlocks, optional moving canopy mass and unilateral spring/damper tether with rigid-body torque |
| Environment | Imported MSL wind and dry-air pressure/temperature profiles, gusts, visual clouds, geographic elevation grid, local-terrain landing and main deployment, satellite texture with geographic landing overlays |
| GPU flow | Actual headless wgpu conservative finite-volume Euler computation; CPU f64 reference; pressure coefficient image, velocity arrows, residual history and CSV export |
| Thermal | GPU or CPU two-cell through-wall diffusion, axial/fin/contact conduction, separate effective axial/through conductivities and prescribed motor heating |
| Structures | Timestamp annular beam/shell screening over retained body/nose sections and planar fins, section material properties and temperature factors; external CalculiX S8 vehicle shells and separate primary fin solids. Post-flight fin frequency and energy-bound ablation remain screening estimates. |
| Avionics | Sampled sensors, latency/filtering/descent confirmation, power/pyro interlocks, deterministic timed sensor/power/pyro faults and configurable one-shot recovery/separation rules |
| Imports and analysis | Mapped OpenRocket component import with explicit omissions, named RASP/RSE motors, timestep convergence, independent reference residuals, seeded dispersion and GeoJSON landing footprint |

## Reproduce the checks

These commands use the built executable. With a source checkout, replace `./rocket-lab.exe` with `cargo run --features viewer --` (CPU-only commands also work without the viewer feature).

```powershell
./rocket-lab.exe view --mission examples/advanced-laboratory.json
./rocket-lab.exe simulate --mission examples/advanced-laboratory.json --out output/advanced
./rocket-lab.exe validate --mission examples/advanced-laboratory.json --out output/advanced-validation
./rocket-lab.exe cfd-check --cells 400
./rocket-lab.exe cfd --mission examples/hpr.json --mach 3 --altitude 15000 --nx 96 --ny 64 --steps 12000 --gpu --out output/cfd/supersonic
./rocket-lab.exe thermal --mission examples/advanced-laboratory.json --out output/thermal-cpu
./rocket-lab.exe dispersion --mission examples/advanced-laboratory.json --runs 20 --seed 42 --out output/advanced-dispersion
./rocket-lab.exe environment --mission examples/advanced-laboratory.json --dem examples/synthetic-terrain.asc --out output/advanced-terrain.json
./rocket-lab.exe simulate --mission output/advanced-terrain.json --out output/advanced-terrain
./rocket-lab.exe import-ork --base examples/kate-anvil-desert.json --ork "sources/Plumbbob_MKIII_R75 (1).ork" --stage-map 2,2,1,0 --out output/import/plumbbob-components.json
```

`--gpu` requires the viewer-enabled executable and a hardware adapter; a rejected calculation returns an error rather than labeling a CPU result GPU. The renderer's thermal worker can use its documented CPU fallback. On this machine the hardware adapter is **NVIDIA GeForce RTX 3060 Ti / Vulkan**.

The four supplied GPU body-section cases at Mach **0.3, 1, 3 and 5.5**, each 96 × 64 cells and 12,000 iterations, reached final relative-density iteration residuals below **1.9e-6**. This residual measures successive iterates, not discretization error. The independent 400-cell Sod shock-tube check gives mean absolute density error **0.01114** and pressure error **0.00835**, with both improving from 200 cells. For the 6,144-cell/300-step GPU/CPU comparison, maximum pressure difference is **0.09093 Pa** and maximum Mach difference **1.93e-5**; `scripts/check_cfd.py` checks cell identity and tolerances.

The advanced thermal case compares **104,920 outer/inner temperatures**, with maximum GPU/CPU discrepancy **0.12605 K**. `scripts/check_thermal.py` checks the full exported history. The advanced flight's two finest timestep runs land **0.098 m** apart. Twenty illustrative seeded dispersion runs all land; apogee p05/p50/p95 is approximately **3.254 / 3.459 / 3.771 km**. These uncertainty assumptions are demonstrations, not measured manufacturing/weather statistics.

## Input semantics

The optional mission `environment` and `experiments` objects default to disabled/empty additions, preserving older JSON files. `examples/advanced-laboratory.json` is an executable schema example. Its material constants and weather are illustrative, not measured properties of Kate, Anvil or Plumbbob.

* Wind and air-profile heights are **MSL meters**. Wind interpolates linearly and holds its endpoints; pressure interpolates logarithmically, temperature linearly. Outside the air-profile interval the standard/custom atmosphere is used, so supply matched endpoints to avoid discontinuities. Humidity and turbulent atmospheric realizations are absent.
* Geographic ESRI ASCII terrain uses latitude/longitude degrees, meter MSL heights and north-first rows. Projected rasters must be reprojected externally. Missing cells are rejected. Outside the grid the ground falls back to launch-site elevation. `altitude_agl_m` telemetry remains height above the pad; terrain clearance controls ground contact and main deployment. A satellite image alone supplies no elevation.
* Aero grids are stage-specific arrays indexed **Reynolds → alpha → Mach**, with logarithmic Reynolds interpolation. Alpha is 0–180 degrees. `ca_off/ca_on` are positive axial-drag magnitudes, `cn` opposes transverse velocity and `cm` is the moment about the nose in `body Z × transverse-flow` direction, referenced to `q A D`. A grid replaces the baseline coefficients for that remaining configuration. Include the complete stack and declare its provenance. Imported coefficients, rather than the experimental flow image, drive 6DoF forces.
* A positive `canopy_mass_kg` enables the moving-canopy/tether model. That mass is allocated within stage dry mass, not added a second time; it must be below 20% of stage dry mass. The body retains approximate stowed CG/inertia. The canopy initializes near line extension with body velocity, so bag extraction is not modeled. A new main replaces the drogue state. Fabric dynamics, discarded-canopy debris and collision are absent.
* Sensor faults have global-flight start/end times. One-shot rules use sampled retained-vehicle conditions and gate on power/pyro state and configured envelopes. Detached stages retain their independent default recovery controller. These rules are a configuration language, not an executable flight-software or inertial-navigation simulator.
* Thermal laws select a stage and region. Axial and through conductivities are effective values; prescribed motor watts apply until a **global flight time**, not a stage-relative ignition time. The structural report uses entered modulus/strength/thresholds. Ablation is an energy bound, not removed flight mass. Cold first bending frequency does not predict flutter.
* OpenRocket `--stage-map` maps **native top-first stages to zero-based bottom-first mission stages**. The supplied four native stages use `2,2,1,0`. Geometry/material/component mass overrides and supported positions are mapped; unsupported subtree overrides/repetition are rejected, and report-only items remain explicit in `openrocket-mapping.json`. Existing base motors and recovery commands are preserved. The native experimental motor mass and named payload allocations are omitted when already represented in the base. Dry mass is preserved via the remaining hardware allocation; total CG is not guaranteed to match the native design. RSE thrust curves are supported, but embedded mass/CG curves are not yet consumed.

## What still prevents calling this complete or validated

The delivered CFD is a **2D planar, perfect-gas, inviscid Euler body section**, using first-order Rusanov fluxes and stair-step slip walls. It excludes fins, viscosity, boundary layers, turbulence, 3D stage interference, wall heating, real-gas chemistry, rarefaction and moving separation. It is separate from the viewport's approximate 3D pressure/incidence colors and thermal boundary model. The optional Knudsen bridge is a drag screening interpolation, not DSMC. Hypersonic trajectory support does not establish hypersonic aerothermal fidelity. Governing-equation context: [SU2 theory](https://su2code.github.io/docs_v7/Theory/) and [NASA rarefied-flow review](https://ntrs.nasa.gov/api/citations/19710016459/downloads/19710016459.pdf).

Thermal diffusion and structural screening are post-processing. Fracture, fin flutter, shell buckling, laminate failure, pyrolysis, ablated-mass evolution and feedback into aerodynamic/avionics states are unfinished. The builder supports axis-based components rather than arbitrary KSP attachment graphs; full OpenRocket override/configuration parity is unfinished. Google imagery requires the user's API key; no key is bundled. NASA imagery and local georeferenced textures are supported. Authoritative elevation/weather data must be supplied/imported.

The supplied older RASAero design, OpenRocket design and newer trajectory CSV describe different configurations. The present independent comparison **fails the selected 5% peak tolerances**: apogee differs by **−7.51%**, peak Mach by **−5.65%**. Numerical convergence and GPU/reference agreement do not fix that mismatch. Completing physical validation needs one authoritative configuration/run, matching motor/aero/mass/CG data, measured flight telemetry, recovery characterization, and material layup/strength/thermal data. Full 3D viscous/real-gas CFD and coupled failure models require additional implementation and independent benchmarks; they are not represented as completed work here.

