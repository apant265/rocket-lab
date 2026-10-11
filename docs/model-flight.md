# Exported 3D model → flight

Use **Export 3D model** in Build to export the current design. Blender runs headless and writes `rocket.blend`, `rocket.glb`, `rocket.mesh.json`, the input `mission.json`, `simulation-mission.json` and a build receipt under `output/design/model-<revision>/`. The GLB is the portable model; the mesh JSON contains the same evaluated geometry and component identities consumed by Rust.

**Launch simulation** exports an updated model when needed, computes its flight properties and launches it. Changes to dimensions, components or physical properties invalidate the captured properties. Errors in model export or mass integration stop launch and appear in the status bar. Legacy display models are regenerated before launch.

The flight uses:

- Dry centroid and all six independent entries of the inertia tensor from evaluated triangles, using volume integrals for material parts. The rigid-body solver uses the full tensor and accounts for off-axis CG in aerodynamic, thrust and recovery moments. Staging applies the parallel-axis theorem and moves each released body's position and velocity to its own 3D centroid. Quenched propellant stays with a stage separated before burnout.
- Density multiplied by actual closed mesh volume for external components and material-mode shells/fins. Implicit material-mode airframes and noses are exported with an inner wall instead of a solid interior. The nose wall is an inward radial offset, not a constant normal thickness. Overrides keep the configured component mass and axial CG while using the mesh for its distribution/inertia.
- Measured stage dry mass distributed uniformly over the evaluated native exterior surface, shifted axially to honor measured dry CG. A measured total does not identify individual component densities or the real internal distribution. Explicit shell conversions retain their existing measured hardware allocation; that allocation and unmeshed internal components keep their lumped cylinder models.
- Two orthogonal exterior projected areas and area centroids for the large-incidence crossflow approximation. These are triangle projection sums, with inner bore faces excluded, interpolated by transverse flow direction. Overlapping parts are not occluded, and aerodynamic interference is not resolved.

Configured motor curves, remaining propellant, payload (including the 5 lb Kate/Anvil payload), recovery and avionics remain inputs. Motor/payload inertia still uses the existing lumped models. Mesh volume integration rejects open or inconsistently wound solids. Captured properties include a deterministic FNV-1a geometry checksum and a configuration fingerprint; saved missions retain these properties and reject stale configuration changes.

This connects actual 3D geometry to flight mass, inertia and crossflow shape. Small-angle CN/CP and Mach-dependent drag still come from the mission's aerodynamic tables or the preliminary builder estimate. It does not turn the flight integrator into a coupled 3D CFD solver. The separate OpenFOAM flow analysis remains a planar section, thermal diffusion remains the material wall-strip model, and CalculiX remains the primary fin analysis. External changes to a GLB are not imported automatically: use the corresponding evaluated mesh JSON with physics metadata and a matching mission.

## Headless commands

Run from the application/project folder. Set `BLENDER_PATH` if Blender is installed outside the detected Windows locations.

```powershell
# Export the model and run the flight in one command
rocket-lab.exe export-model --mission examples/kate-anvil-desert.json --out output/model-flight --simulate

# Run again using an already exported model
rocket-lab.exe simulate --mission output/model-flight/mission.json --model output/model-flight/rocket.mesh.json --out output/model-flight/replay

# Inspect the same model and flight in Bevy
rocket-lab.exe view --mission output/model-flight/mission.json --model output/model-flight/rocket.mesh.json --start-time 4
```

`flight/` contains telemetry, events, landing/trajectory GeoJSON, detached-stage telemetry and the exact mission with captured model properties. Replaying `simulation-mission.json` without `--model` retains the captured mass/inertia; supplying `--model` recomputes them from the mesh.
