# Blender geometry and surface views

**Simulate** timestamp analysis offers a separate computed GPU/CPU **planar Euler body section** with Cp colors, velocity arrows and residuals. The 3D surface maps described below retain their incidence approximation; they are not mapped from that 2D computation. Spatial thermal diffusion likewise uses its prescribed boundary model. [Exact solver scope and checks](advanced-laboratory.md).

The viewer consumes actual triangulated, evaluated **headless Blender** geometry. `scripts/build_model.py` creates a rounded conical nose, tessellated bodies, four closed profiled fins per stage, seams, retainers and nozzle contours. The Plumbbob model uses the supplied CDX1 body dimensions, nose length/blunt radius and fin root/tip/span/sweep/thickness. Fin `Location` is provisionally interpreted as a distance forward from the stage aft end: verify native placement and stage nesting. Fittings/nozzle details are illustrative. Generic HPR and sounding demo models use synthetic fin/nose proportions.

Each generation writes `rocket.blend`, `rocket.glb`, `rocket.mesh.json` and a build receipt. The native `.blend` and reusable GLB preserve the scene; the JSON exports the same evaluated corner positions, split normals and triangle indices directly into Rust/Bevy. Geometry is in meters with the whole-stack nose at the origin and model +Y toward the nose. Each part retains its stage identity during separation. Stage count/dimensions, triangle indices and finite unit normals are validated before loading; changing mission geometry rebuilds the preview or falls back to primitives until a matching model is generated. This exterior mesh does not change the flight engine's inertia or coefficient tables.

## Pressure estimate

Surface pressure is `p = p∞ + q Cp`, with ambient pressure and dynamic pressure from the current flight state and wind-relative velocity. Body attitude transforms the incoming direction into model coordinates. The windward incidence estimate is `Cp = Cp,max max(n · upstream, 0)²`. `Cp,max` uses isentropic stagnation for Mach ≤1, or normal-shock downstream pressure followed by isentropic stagnation for Mach >1, with gamma fixed at 1.4. This is a modified Newtonian approximation for hypersonic flow; using it at lower Mach is **illustrative**, especially transonic flight. The stagnation bound increases from Cp=1 at low Mach to approximately 1.83937 at high Mach. Normal shock relations follow [NASA Glenn](https://www.grc.nasa.gov/WWW/BGH/normal.html); modified Newtonian methodology is described in [NASA's hypersonic pressure analysis](https://ntrs.nasa.gov/api/citations/19650000987/downloads/19650000987.pdf).

The blue-to-red legend uses `ln(1+80 Cp)/ln(1+80 Cp,max)` to make shallow surface incidence visible. Legend endpoints are **absolute pressure in kPa**, from ambient to the stagnation bound; red does not mean temperature or failure. At negligible dynamic pressure the map is ambient blue. A slender conical nose has nearly constant low incidence, while the small rounded tip reaches the stagnation pressure: use **Rounded nose tip** to resolve the red/yellow gradient. Lighting is disabled in pressure mode and vertex colors are converted from sRGB to linear so the legend and surface palette agree. Discarded stages are gray; their surface fields are not calculated. Rarefied-flow conditions display a warning.

The map does not resolve surface shielding, suction/expansion, base/wake pressure, shock interactions, boundary layers, fin/body interference or real-gas effects. Internal/occluded mesh surfaces also receive an unshielded incidence estimate in CSV output. No pressure integration feeds back into the flight forces. The separate flight thermal node and stagnation heat-flux estimate are not a resolved temperature field.

## Airflow view

The **surface heatmap** colors the tangential projection of the unit incoming freestream onto each surface: `sqrt(1 - (n · upstream)²)`. The legend runs from blue (0, incoming direction normal to the surface) to red (1, incoming direction tangent to the surface). It describes incidence geometry; it is **not wall velocity**, boundary-layer speed or a solved local fluid velocity. At negligible air speed it is blue. This fraction is also exported in the pressure CSV. Surface colors and path colors have separate meanings.

Animated paths illustrate **geometric displacement around an axisymmetric body envelope**. Radius is `sqrt(seed_gap² + body_radius(station)²)`, with an illustrative aft closure. This is not a compressible flow solver. Paths avoid the body but do not model fins, wakes or turbulence. Blue-to-green path color indicates radial deflection, not pressure, speed or vorticity; arrows move forward-to-aft at an illustrative animation rate independent of playback. They are hidden for negligible relative speed, angle of attack ≥20°, or Knudsen number ≥0.01. Small incidence is approximated axially; pressure colors still use the actual relative-flow direction. Recovery does not get a canopy flow field.

The orange wire guide uses the Mach angle `μ = asin(1/M)` for M>1.05, oriented to the incoming velocity. It is a Mach-angle reference, **not a calculated bow-shock shape**. It uses the same incidence/continuum limits as paths. Pressure coloring and paths can be enabled together. Camera focus choices cover the active stack, forward body, active-stage fins and rounded tip; displayed geometry is enlarged independently of the kilometer trajectory.

## Rebuild and export

```powershell
# Installed Blender runs without opening its GUI.
./scripts/Build-Models.ps1
./scripts/Build-Models.ps1 -Mission examples/hpr.json -Out assets/models/hpr
./scripts/Build-Models.ps1 -Mission examples/sounding.json -Out assets/models/sounding

# Custom model (generic proportions unless an explicit CDX1 design is supplied).
./scripts/Build-Models.ps1 -Mission your-mission.json -Out assets/models/custom
cargo run --features viewer -- view --mission your-mission.json --model assets/models/custom/rocket.mesh.json --start-time 5

# No graphics required for CSV export. Uses the last recorded sample at/before time.
cargo run -- surface --mission examples/plumbbob-provisional.json --time 4 --out output/surface
```

The viewer also exports the selected recorded sample with **Export current pressure map CSV**. `surface-pressure.csv` contains part/stage/vertex, model coordinates, estimated Cp, pressure in Pa and tangential freestream fraction; `surface-pressure.json` includes time, Mach, ambient/q, generator and assumptions. Split-normal vertices are intentionally duplicated. Exported fields remain an incidence approximation, not validated CFD input.

Verification: all three bundled Blender models load, normals and face winding agree, changed dimensions/invalid indices/zero normals are rejected, pressure limits and continuity across Mach 1 pass, wind-relative incidence matches flight telemetry, and displacement paths clear the body. The automated viewer capture includes transonic pressure, rounded-tip pressure, supersonic airflow, shaded geometry and hypersonic pressure after staging.
