# Verification record

## Current extension: October 10, 2026

The suite contains **69 core/integration tests plus 18 additional viewer tests (87 distinct)**. All pass, and `cargo clippy --all-targets --features viewer -- -D warnings` passes. New checks cover finite-volume uniform flow/slip walls and exact/refined Sod shock tubes; imported wind consistency between flight and surface fields; geographic DEM interpolation/contact; multidimensional aero grids; reefing; unilateral tether loads and complete moving-canopy flights; timed sensor/pyro faults and command interlocks; thermal anisotropy; RSE/component import semantics; signed reference descent speed; grouped undo/redo; and preserving edits when undoing a rocket reset. Application-owned render checks cover both neutral themes and reset state in `output/reset-rocket-check.json`.

Timestamp analysis adds four numerical checks: causal heat history, exact interpolated timestamps, stage-discontinuity handling, conservative thermal refinement, and rectangular-cantilever stress/deflection convergence (the first test combines causal history with interpolation). The native smoke checks backward seek, pause, stress invalidation/restoration and exclusion of future heating. It executes Preview and Standard CFD at 4 s, Standard thermal through 43.5 s and Fine thermal/stress through 4.123 s on the hardware GPU. See `output/timestamp-check.json`, `output/timestamp-summary.json` and [detail presets](timestamp-analysis.md). The 29 viewer library tests include 11 tests also present in the core run.

The actual RTX 3060 Ti/Vulkan executes the Euler compute shader. At 96 × 64 cells, the 300-step CPU/GPU comparison gives maximum pressure error **0.09093 Pa** and Mach error **1.93e-5**, with identical cell coordinates/solid masks. The 400-cell exact Sod comparison gives density/pressure mean absolute errors **0.01114 / 0.00835** and improves over 200 cells. The Mach 0.3/1/3/5.5 body-section cases reach iteration residuals below **1.9e-6** after 12,000 steps; this is not a rocket grid-refinement study or validated force database.

The current advanced thermal export has **20 columns × 2 cells × 2,623 frames = 104,920 temperatures**. Its complete CPU/GPU comparison has maximum difference **0.12605 K**. Current `output/thermal` artifacts refer to this advanced case; the older 60-column figures below are historical checks of the Kate/Anvil case, not the current files.

Both Kate/Anvil and advanced cases pass the recorded three-timestep criteria. Their two finest landing positions differ by **0.083 m / 0.098 m** respectively. The independent newer RASAero reference check **fails 5% peak tolerances**: apogee error **−7.5055%**, peak Mach **−5.6550%**, altitude RMSE **8,067.21 m**, speed-magnitude RMSE **111.05 m/s**, Mach RMSE **0.39868**, over 2,453 of 2,456 samples. The source's signed descent velocity is converted to a magnitude for the speed comparison. Configuration identity remains unconfirmed; no fitting was used. Convergence is not physical validation.

Twenty advanced seeded dispersion flights all land, with apogee p05/p50/p95 **3.254/3.459/3.771 km** and an exported illustrative Gaussian landing ellipse. A synthetic geographic terrain run also completes; touchdown is approximately **4.245 m/s at ENU 1,240.23 / 654.66 m**. These results use documented synthetic wind/material/uncertainty inputs.

Application-owned Bevy smoke captures verify the rendered modern builder, catalogue, thermal outer/inner views, computed timestamp CFD and fin stress and recovery views. Blender headless generated the new transition card, 29,832-triangle advanced mesh and 424,464-triangle imported-component model. The native desktop helper failed to capture the current window with `foreground window did not report a process id` on two attempts. Current verification therefore does not claim a new manual end-to-end native interaction test; the existing actual-egui held-drag and camera tests provide separate input evidence.

Commands, input conventions, solver bounds and remaining requirements are in [advanced-laboratory.md](advanced-laboratory.md). Earlier sections below record previous implementation milestones and should not be read as current test totals.

Verified locally on October 10, 2026 with Rust 1.91.1, Windows/MSVC, Bevy 0.18.1 and bevy_egui 0.39.1.

## Checks completed

- **25 automated tests passed:** 3 dynamics/conservation unit tests and 22 integration/input tests.
- `cargo clippy --all-targets --features viewer -- -D warnings` passed.
- Native Bevy executable built and ran successfully. The automatic 150-frame smoke run captured `output/viewer-smoke.png`; the frame was visually inspected. The check exercises the actual renderer, schematic staged vehicle, world grid, trajectory gizmos, flight instruments, charts and mission controls. It is not an automated click-by-click test of every editing action.
- Headless HPR, synthetic sounding and provisional Plumbbob flights completed; telemetry, events, mission snapshots, summaries and detached-stage histories were exported.
- RASAero reference comparison converted all 2,456 samples to SI.
- Twelve seeded HPR dispersion runs completed with seed 42, with all twelve retained vehicles landing.

## Observed example runs

Surface extension: **30 automated tests pass** (25 existing flight/import tests plus 5 surface/mesh tests). Viewer and core pass Clippy with warnings denied. Headless Blender 5.2.0 LTS generated all three bundled models; Plumbbob has 28 parts, 41,760 vertices and 58,784 triangles. Native viewer captures verify pressure, rounded-tip detail, supersonic airflow, shaded geometry, hypersonic staging, geometry edits falling back to primitives and restoring the matching Blender mesh. The pressure CSV exported 41,760 vertices at T+3.922 s, spanning 87.460–159.578 kPa; tangential fractions remain within 0–1. The incidence fields remain approximate and have no CFD validation; see [surface-visualization.md](surface-visualization.md).

| Configuration | Apogee AGL | Peak Mach | Landing speed | Input status |
|---|---:|---:|---:|---|
| HPR demonstrator | 3.507 km | 1.131 | 4.15 m/s | Synthetic motor and aero |
| Three-stage sounding demo | 166.144 km | 5.792 | 4.03 m/s | Synthetic motor and aero |
| Plumbbob provisional mapping | 147.482 km | 5.757 | 9.32 m/s | Supplied RASP motors and older CDX1 masses; provisional aero/inertia/avionics |
| Supplied newer RASAero CSV | 168.141 km | 6.208 | Not established here | Reference simulation export, not measured flight data |

The provisional mapping differs from the newer reference by **−12.29% in apogee**, with overlapping-altitude RMSE approximately **13.20 km**. This is an intentionally exposed difference, not a calibrated accuracy claim. Configurations, CGs and masses differ between the supplied files, and the provisional aero tables are synthetic. The synthetic sounding demo's closer apogee is coincidental and should not be used as evidence of accuracy.

The synthetic sounding demo also exceeds its placeholder q and skin-temperature limits. The model continues its rigid-body flight while reporting those exceedances; that is not a finding that the real vehicle survives them.

For the demonstrated uncertainty assumptions, the twelve HPR runs give apogee p05/p50/p95 approximately **3.300 / 3.463 / 3.676 km**. Those distributions are illustrative, not measured tolerances or a validated landing-risk assessment.

## What the automated tests establish

Atmosphere tests compare standard sea-level, 11 km and 20 km checkpoints and check continuity/positivity over the modeled range. Gravity tests verify the spherical altitude and inverse-square ratio. A drag-free rigid body conserves central-field mechanical energy and orbital angular momentum, and its quaternion reproduces an analytic spin. Separation tests conserve total mass, linear momentum and the weighted CG position, including nonzero angular velocity.

Motor tests verify trapezoidal impulse, fuel bookkeeping and the exact integrated impulses of the three named supplied database entries. Mass tests verify CG/inertia evolution and the older CDX1's 182 lb/137 inch loaded stack values. Aerodynamic tests check interpolation, regime boundaries, restoring moments and fin-cant/damping torque signs.

Full flights verify recovery and landing, all four regimes, staged ignition, Mach-triggered separation in each regime, quaternion normalization, finite detached-state histories, strictly increasing telemetry time, timestep-refined apogee, identical repeated runs and failure behavior for insufficient thrust, battery exhaustion, altitude-sensor loss and unsafe chute deployment. Import tests cover actual local design files, imperial units, named-motor selection, valid SI tables and rejection of trajectory/partial CFD files as complete aero inputs.

These checks establish implementation consistency within the stated equations. They do **not** validate the approximations against real flight measurements. Broader uncertainty/error-controlled integration, vehicle-specific experimental validation, atmosphere/heating improvements, full native configuration import, experimental recovery validation and aeroelastic models remain necessary for a higher-fidelity tool. See `model.md` and `source-review.md` for the exact assumptions.

## Builder and launch-weather checks

Six additional tests verify measured-site pressure/temperature anchoring at an elevated launch site, atmospheric layer continuity, identical custom-weather dynamic pressure in the flight/surface models, legacy standard-profile defaults, weather validation, fin-span/position influence on CN/CP, analytic conical/elliptical nose CP limits, geometry JSON round trips and nose-referenced CP for every staged configuration. Three viewer tests verify right-drag translates within the view plane without rotating, free mouse-look rotates without orbiting the camera position, and pitch is clamped while the quaternion stays normalized. There are 36 headless core/integration tests and three additional viewer camera tests.

The native app was exercised through its real Build and Simulate menus: select the HPR starter, change the conical nose to a tangent ogive, launch, rebuild in Blender 5.2 headless, load the 33,864-triangle mesh and enter flight playback. Playback controls, mouse-wheel zoom, cloud rendering and Fit flight with flown/projected arcs were inspected in the running app. `cargo clippy --features viewer --all-targets -- -D warnings` passes. These workflow and formula checks are implementation checks, not validation against measured rocket flights.


## Assembly, recovery and georeferencing extension

Six additional integration tests verify density-derived part masses, separate mass/CG overrides feeding inertia, structural material mass replacing measured mass, invalid shell rejection, spherical coordinate round trips at altitude and downrange, all detached stages inflating their own mains and landing, stage-specific deployment inhibition/line failure, and the 5 lb Kate and Anvil allocation preserving the supplied assembled mass and first moment. The core suite now contains 42 tests; viewer coverage adds the three camera checks, a canopy vent/winding check and two map/provider checks.

The new desert study completes at 595.834 s with 155.521 km apogee, peak Mach 5.857 and 9.66 m/s retained touchdown. Detached S1/S2 touch down at 4.77/4.99 m/s, with independent drogue and main events. These are provisional-model outputs, not experimental validation. The original source example is preserved.

The complete extension passes 42 core tests plus eight additional viewer/UI/map tests (50 total). The egui interaction test feeds press, held movement and release through the actual palette/drop widgets. Native checks verified part placement and the density/override inspector, all three deployed canopy views, NASA imagery retrieval, map overlays and geographic landing markers. Native smoke captures include parachute-preview.png, booster-parachute.png, second-stage-parachute.png and retained-parachute.png. NASA retrieval produced a 1024 x 1024 real desert image; Google was not live-tested because GOOGLE_MAPS_API_KEY was absent. All-target viewer Clippy passes with warnings denied.

## Visual catalogue, GPU heat diffusion and descent extension

The current suite passes **48 core tests and 11 additional viewer tests (59 distinct tests)**. Added checks cover ray/axis placement after camera movement, stage selection and station clamping, annular geometry, through-wall energy conservation and equilibration, material-dependent heat transfer, conservative axial/contact diffusion, separation cutting a thermal joint, legacy material-property loading, tail-first descent instability and the restoring moment of a nose suspension attachment. The existing actual-egui held-drag test now uses the image-card widget's interaction path. All-target viewer Clippy passes with warnings denied.

Fifteen transparent catalogue images were generated by Blender 5.2 headless using `scripts/build_part_previews.py`. Render checks include the Structure and Internals palettes plus outer/inner thermal maps. Native checks verified cards, ray-based click-to-place, an immediate new bulkhead at 0.81 m, its 0.3021 lb density-derived mass, shell transparency and a cyan motor-tube placement preview snapped to S2 at 0.87 m. The desktop helper's fast native drag collapsed press/movement into one input batch, so that automation check is inconclusive; native click placement and a multi-frame held-drag egui test provide separate evidence. No global input-event reordering was introduced.

The actual NVIDIA RTX 3060 Ti executed the WGSL thermal shader over 60 columns and 5,936 frames in approximately 1.9 s including preparation and the short reference check. A complete independent CPU flight/thermal run then compared **712,320 outer/inner temperature values** against the GPU export: the largest difference was **0.19144 K**, in S2 fin inner temperature near 595.843 s. `output/thermal/compute.json` and `full-reference-check.json` record the backend and comparison. `scripts/check_thermal.py` reproduces the full comparison after running the CPU thermal CLI on the exported mission. CPU fallback was also exercised when shader validation rejected a development version; the corrected shader uses the GPU.

These results verify implementation consistency, energy transfer and GPU/reference agreement. They do not establish accuracy of the prescribed convection, contact, composite-property or large-angle aerodynamic approximations against real vehicles or CFD data. See [thermal-and-descent.md](thermal-and-descent.md).

The deliberately recovery-disabled ballistic HPR demo also completes without a scripted attitude change. Its computed axis tilt rises from 41.5° at apogee (25.09 s) to 98.1° ten seconds later, with nonzero integrated pitch/yaw rates. This illustrates a descent overturn rather than guaranteeing continuous tumbling in every vehicle or in vacuum. Normal recovery can oppose this motion.
