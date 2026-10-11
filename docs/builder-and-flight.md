# Building and launching

Start `./Launch.ps1`, or `./Launch.ps1 -Hpr` for a small single-stage design. The app opens in Build. The interface uses Bevy 0.18.1 with bevy_egui 0.39.1.

The header offers **Dark** (neutral charcoal) and **Light** (warm white) themes, shared by the inspector, model cards, pop-up tools, charts and inspection background. Your choice is stored in `output/ui-theme.json`. Scientific heatmap and trajectory colors retain their data meaning.

**Reset rocket** restores the mission loaded at startup, pauses at launch, centers the camera and returns to Build. It clears selections and placement previews; Undo recovers your previous draft. **Rewind flight** and Home simply pause at time zero while keeping the current design.

1. Drag a card from **Parts** into 3D. Click an actual body, nose, fin or placed component to open its properties on the right. A copper selection box surrounds the selected visible component and follows edits, camera movement and zoom. Enable **Internals** to click the motor, payload, avionics or packed parachute; a visible preview canopy selects that stageÃ¢â‚¬â„¢s recovery settings.
2. Edit airframe length/diameter, nose shape/length, or fin count, root/tip chord, span, sweep, thickness and station. Stations are measured aft of each stage's nose. The top stage includes the nose; its body length is shown separately.
3. Enter measured dry mass and CG. Dry mass includes empty motor casing, fittings, recovery hardware and electronics. Payload and remaining propellant are added separately. Material-based dry mass is available as an explicit alternative; added palette components always contribute their own mass.
4. Select a motor curve from the supplied RASP library. The filter checks outer diameter and stage length; motor mount clearance still requires checking. Replacing a recognized motor adjusts its empty casing mass and first moment. For an unrecognized previous motor, the UI asks you to include the new casing in the measured mass.
5. Configure staging triggers, ignition delay, recovery envelopes and the idealized flight computer. Dropping a transition creates a booster immediately below the chosen stage, selects its transition and preserves existing stagesÃ¢â‚¬â„¢ recovery/avionics assignments. Its fore diameter matches the upper stage; its editable aft diameter initially uses 1.2 times that diameter. The booster copies the target stageÃ¢â‚¬â„¢s motor, dry mass and materials as a starting template; set its actual motor, mass and recovery. Remove the selected stage from the inspector. Stages are saved bottom-first.
6. Click Launch simulation. Pending geometry automatically triggers a Blender rebuild. Blender runs in a separate background process. The UI stays responsive, then loads the new surface mesh. Files and logs are in `output/design/model-N/`; `.blend`, GLB, mesh JSON and mission JSON remain available there. `BLENDER_PATH` can select a different executable.
7. In Simulate, set rail/site conditions, mean wind speed and the bearing it comes **from**, gust amplitude/period, and shear. Apply & launch integrates a new trajectory using the edited conditions.

The sidebar contains Parts and their categories. To start from a different saved design, run `rocket-lab.exe view --mission path/to/mission.json`. The current applied mission is written to `output/design/model-N/mission.json` during the automatic model build. Mission JSON exposes all force coefficients, sensor parameters and solver controls. The 3D draft updates immediately; applied CG/CP and flight reflect the most recently applied configuration. Launch simulation applies edits and rebuilds the Blender exterior as needed. Model export replaces exterior mass distribution and inertia with evaluated mesh integrals while honoring measured mass/CG overrides. Imported aerodynamic tables remain in use unless changed builder geometry triggers the preliminary geometry estimate. See the model-flight calculation modes for material volumes and lumped internal hardware.

## Camera and trajectory

A short left click opens component properties. Move at least six logical pixels while holding left click to start camera translation. Hold right click and drag to translate the camera in its view plane, keeping its orientation fixed. Shift + right click provides mouse-look at the current camera position. While held, WASD flies forward/left/back/right and Q/E flies down/up. Release restores the pointer. Wheel zoom changes camera distance. Vehicle restores close inspection; Fit flight frames the retained vehicle's complete integrated path. Flown and Projected independently toggle cyan history and dashed amber future. The white marker locates the vehicle; a second marker marks apogee. The future arc is the remaining deterministic trajectory for the applied inputs, including staging and recovery, not a probability envelope or live atmospheric forecast. A duration timeout shows only the calculated portion of the flight. Vehicles are enlarged for readability.

The Focus selector provides whole-vehicle, forward-body, stage-fin and nose-tip inspection. Model, Pressure and Airflow modes share the same camera controls. Pressure and airflow assumptions remain documented in [surface-visualization.md](surface-visualization.md).

## Launch weather

The default retains the standard-atmosphere profile exactly. Enabling measured weather initializes from standard conditions at the chosen site. Enter temperature in Ã‚Â°C and **station pressure** in hPa, measured at the site, rather than sea-level-reduced weather-report pressure. A dry-air lapse profile is anchored at the site; it supplies density, sound speed, Mach, dynamic pressure, heating and surface pressure consistently. There is no humidity or weather-file interpolation model.

Mean wind uses a compass bearing from north, clockwise, describing where wind comes from. The existing deterministic gust is sinusoidal in east and 0.37 times that signal in north; shear multiplies the mean by `1 + shear_per_km Ãƒâ€” AGL_km`. Clouds drift visually with the integrated ground-level wind. Cloud cover, base AGL and thickness affect rendering, without adding cloud-induced forces or turbulence. Sky color fades with vehicle altitude. Close inspection hides the cloud/ground layer for an unobstructed exterior view; zoom out or use Fit flight to see the environment.

## Aerodynamics and scope

Unchanged starter/imported missions retain their coefficient tables. Editing geometry generates a prominently labeled **preliminary** small-angle low-Mach Barrowman-style normal-force slope and nose/fin/diameter-step CP for every remaining stack. The geometric nose pressure-center integral and trapezoidal fin formulas follow the mathematics described in the [OpenRocket technical documentation](https://openrocket.sourceforge.net/techdoc.pdf). Drag is a synthetic Mach envelope using geometric wetted/reference area and nose slenderness. CN/CP are held at higher speeds. This is an independent engineering estimate, without OpenRocket's full aerodynamic model, RASAero II parity, fin interactions, a validated transonic/hypersonic solution or CFD.

The flight engine can cross all four regimes and stage there, but quantitative prediction still requires validated whole-stack Mach-dependent coefficients. Inertia remains based on axisymmetric cylinder mass distributions. Detached boosters have independent optional recovery controllers and separately rendered fabric canopies. Avionics models sample/latency/power/interlocks; they do not execute firmware. See [model.md](model.md) and [validation.md](validation.md) for the broader limits.




See [parts, recovery and satellite maps](assembly-recovery-maps.md) for the drag-and-drop palette, pounds, density, per-stage recovery, georeferenced ground imagery and GeoJSON export.

## End attachment and offsets

Drag a body tube onto the upper/lower half of a section to attach above/below it. The stack extends rather than clamping the new section inside the old one. A nose cone attaches at the top, and a repeated nose drop selects the existing nose. Click each section to edit its length, diameter and signed axial offset; positive is aft, negative is forward. Length edits shift downstream stations rather than scaling all parts. Move in 3D snaps exterior sections to attachment boundaries. Internal components still fit inside their stage.

Converting a measured stage into explicit sections preserves its whole-stage dry mass and CG in the hardware allocation, with zero mass overrides on the original shell/fins. New sections add their material mass. To use density for the original sections, disable their overrides and recheck the hardware allocation to avoid counting the measurement twice. Manual offsets can intentionally create gaps or overlaps; the automatic attachment path creates end-to-end joints.


## OpenRocket-style Parts

The separate **Booster stage** card has been removed. Parts now uses the component families in the [OpenRocket basic design guide](https://openrocket.readthedocs.io/en/latest/user_guide/basic_rocket_design.html):

- **Body & fins:** nose cone, body tube, transition, trapezoidal fins, elliptical fins, freeform fins, tube fins, launch lug and rail button.
- **Inner components:** inner tube, tube coupler, centering ring, bulkhead and engine block.
- **Mass & recovery:** parachute, streamer, shock cord, mass component, motor, payload and avionics.
- **Assemblies:** pods.

Each card has a headless Blender preview. Drag it into the vehicle, then click the placed component for its properties and selection box. Materials, density, dimensions, thickness, axial offset, rotation, measured mass/CG overrides, color, finish and comments are stored with the component. Elliptical/freeform fin mass and axial CG use their actual polygon outline. Freeform outlines support 3Ã¢â‚¬â€œ64 points and concave shapes; crossing/degenerate outlines prevent launch. Tube fins have automatic touching-tube sizing, editable count and radial offset. Repeated tube fins/pods contribute mass and radial inertia for every instance. Blender exports their exterior geometry for the launch view as well as the immediate builder mesh.

A parachute or streamer component edits one existing drogue/main recovery channel for that stage; select apogee for the drogue channel. The retained stage uses the mission recovery controller. A streamer uses prescribed area/Cd in the flight model and renders as an open ribbon when that channel deploys; its wavy shape is illustrative. Preview recovery displays the main channel. Packed recovery dimensions describe its stowed envelope, while deployed area is separate. Only one active device per channel is modeled.

This reproduces the core component catalogue and adds this application's motor/payload/avionics cards. It does **not** establish full OpenRocket parity. Pods are repeated radial tube housings with mass and geometry, without a nested child-component tree, pod-specific thrust or parallel-booster separation. Tube-fin/pod/launch-lug aerodynamic interactions are not resolved; provide measured whole-vehicle coefficient tables for predictive flight. Elliptical/freeform fins use an area-equivalent trapezoid in the preliminary normal-force estimate. Finish changes an approximate skin-friction factor and optional component Cd adds a frontal-area-weighted contribution; neither is OpenRocket's full roughness/override model. Nose profiles currently support conical, tangent-ogive and elliptical shapes. The external stress solver still analyzes the configured primary trapezoidal fin. Transition drops continue creating an axial booster below the target stage.

## Exported model flight

**Export 3D model** saves Blender and GLB files plus evaluated mesh data. **Launch simulation** uses mesh-derived dry mass, centroid, full inertia and projected crossflow shape. See [model flight](model-flight.md) for the calculation modes, saved outputs and aerodynamic limits.

Front transitions replace the leading cylinder section in the builder, Blender export, fallback flight mesh and CFD body profile. The default Kate and Anvil example now includes editable 6 in and 2.5 in interstage tapers, using lengths from the supplied OpenRocket design with fore diameters reconciled to the current adjacent stages. G10 wall properties remain editable assumptions. Nominal taper mass and CG are allocated from the existing assembled dry measurements. Thermal axial cells resolve these tapered walls and their section materials; the CFD numerical wall remains Cartesian.
