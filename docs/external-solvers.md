# External flow, thermal and structural solvers

Simulate → Analysis at timestamp defaults to hardware GPU **Built-in** analysis. Choose CFD,
Thermal or Stress, a timestamp and a detail preset, then Compute. These jobs run
actual solver programs in Docker and import their fields into Rust. The app stays
responsive. **Built-in** retains the separate internal CPU/GPU models. An
external failure displays the solver error; it never substitutes preview values.

The installed runtime is OpenCFD v2406 (`opencfd/openfoam-default:2406`). Stress
uses CalculiX 2.21, installed in the `rocket-lab-analysis:2406` Docker image. Python
3 on the host generates cases and imports ASCII fields; it uses only the standard
library. Docker Desktop must be running. These external jobs use the CPU. No GPU
acceleration or general-purpose CFD GPU capability is implied by detecting a GPU.

To prepare another machine:

```powershell
docker pull opencfd/openfoam-default:2406
docker build --tag rocket-lab-analysis:2406 --file scripts/Dockerfile.analysis scripts
python scripts/check_external.py
```

`ROCKET_PYTHON` can select the host Python executable. Cases run with Docker
networking disabled; the initial image pull/build requires network access. Solver
arguments and case names are generated locally, with paths passed as argv mounts.
Each execution gets a fresh case folder, avoiding stale fields from earlier jobs.

| Analysis | Actual solver and imported result | Current physical scope |
|---|---|---|
| CFD | OpenFOAM `rhoCentralFoam`; pressure, velocity, density and temperature | Compressible planar body section, laminar viscosity with Sutherland transport, perfect gas, no-slip adiabatic stair-step wall, freestream farfield. Excludes 3D fins, turbulence, chemistry and rarefaction. A finite flow-time budget does not establish a steady solution. |
| Thermal | OpenFOAM `laplacianFoam`; transient material temperatures | Independent planar wall strips per stage/region/material, axial and through-wall isotropic diffusion, prescribed time-varying recovery-temperature convection. Effective conductivity comes from the entered through-wall law. Excludes radiation, internal heat sources, anisotropy, interregion contacts, fin-root contact and CFD wall-flux coupling. |
| Stress | CalculiX S8 quadratic vehicle shells and separate C3D20R primary fin solids; displacements and integration-point von Mises stress | Every retained body/nose section uses its assigned material/thickness. Aft-clamped bonded shell with prescribed inertia/aero resultants; primary fins have clamped roots and prescribed pressure. Enter modulus, Poisson ratio and optional strength/temperature factors per material. Extra fins retain beam screening. No joint/contact, thermal expansion, buckling, laminate failure or fracture. See [full scope and verification](vehicle-stress-and-units.md). |

Thermal integrates from launch to the selected timestamp, preserving flight/event
knots. Preview/Standard/Fine/Ultra use 2/4/8/16 cells through each wall and show
averages over its outer/inner halves. The spatial/temporal presets also refine
axial and fin columns and the diffusion step. Structural presets use 32/64/128/256
span divisions, one quarter as many chord divisions, four elements through the
thickness, and eight integration points per quadratic element. Large stress
meshes can require substantial CPU time and memory.

The 3D pressure/airflow overlays retain their explicitly labeled geometric
estimates; actual OpenFOAM pressure and velocity appear in the inline planar map.
Thermal and whole-vehicle section stress results color the vehicle. Stress bins display
the maximum sampled integration-point stress; these points lie inside elements,
so this is not a recovered boundary stress or a mesh-independent failure margin.
None of these post-processing jobs changes the recorded 6DoF trajectory.

Every job saves its request, result, solver case paths, numerical fields and logs
under `output/snapshots/`. CFD saves `compute.json`, `flow.csv` and a pressure SVG.
Thermal saves temperature CSV/model metadata. CalculiX cases preserve `.inp`,
`.dat` and solver logs, plus imported `stress.json` and `vehicle-stress.json`. Mesh checks must report
`Mesh OK.`; a CalculiX job must finish and return complete node/element fields.
Inspect convergence and refine before interpreting a vehicle result.

Headless reproduction:

```powershell
cargo run -- external --mission examples/advanced-laboratory.json --time 4 --analysis flow --detail preview --out output/external-flow
cargo run -- external --mission examples/advanced-laboratory.json --time 4 --analysis thermal --detail standard --out output/external-thermal
python scripts/check_vehicle_stress.py
python scripts/check_external.py --out output/external-validation
```

Analytical acceptance checks passed on this machine: uniform compressible flow
preserved pressure to displayed precision and velocity within 6e-13 relative;
a convectively heated, insulated slab matched outer/inner half-wall averages
within **0.086 K**; a rectangular cantilever matched its integration-point stress
within **0.55%** and tip deflection within **0.20%**. These verify those numerical
cases, not the rocket's aerodynamic inputs or general hypersonic fidelity.

Primary solver references: [OpenFOAM rhoCentralFoam](https://doc.openfoam.com/2306/tools/processing/solvers/rtm/compressible/rhoCentralFoam/),
[OpenFOAM solver guide](https://www.openfoam.com/documentation/user-guide/a-reference/a.1-standard-solvers),
[CalculiX documentation](https://www.dhondt.de/new_calc.htm).
