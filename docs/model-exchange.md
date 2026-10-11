# Import and export

For editable rockets, use **File → New rocket / Open rocket / Save / Save as**. Rocket Lab JSON files include parts, dimensions, materials, motors, launch conditions and embedded imported meshes. Unsaved edits prompt before switching designs. New provides a single-stage starting design with body, nose and fins. The shortcuts are Ctrl+N, Ctrl+O, Ctrl+S and Ctrl+Shift+S.

Use **Import** in Build, or drop a model file into the app. Choose units, the positive nose axis, scale and destination stage. **Add part** creates a selectable component; **Replace exterior** replaces the selected stage's exterior while preserving its motor, payload, recovery and measured dry mass. The imported triangles are embedded in the saved mission. Click the model to edit its length, diameter, position, material and mass.

| Format | Import | Export | Dependency |
| --- | --- | --- | --- |
| Binary / ASCII STL, OBJ | Yes | Yes | Native Rust |
| STEP / STP, IGES / IGS, BREP / BRP, FreeCAD FCStd | Yes | Yes | FreeCADCmd |
| GLB, glTF, PLY, Blender BLEND, FBX | Yes | Yes | Blender |
| Collada DAE | If the installed Blender provides the operator | If available through CLI | Blender |
| Rocket Lab mesh JSON | Evaluated model input through CLI | Yes | Native Rust |

Proprietary CAD projects such as SolidWorks, Fusion and Inventor should first be exported from their authoring application to **STEP**, or to STL / OBJ. The app does not claim to parse every vendor's native project format. CAD exports contain **faceted BREP shells / solids** from the evaluated triangles; they do not recover parametric features or the original analytic surfaces.

FreeCAD is discovered in common installation directories or through `FREECAD_PATH`. Blender is discovered similarly or through `BLENDER_PATH`. These variables point to the executables. No CAD tool is needed for native STL / OBJ import. Export first evaluates a changed design in Blender, unless a matching evaluated mesh is supplied to the CLI.

STL, OBJ and PLY need an explicit unit scale and nose axis. Millimeters and +Z are the STL defaults; OBJ defaults to meters and +Y. CAD converters use millimeters. GLB / glTF use meters and their +Y scene basis; Blender / FBX exchange uses the Blender +Z basis. Mesh JSON remains meters / +Y. Imported scene geometry includes evaluated modifiers and world transforms; all mesh objects become one component. External textures and CAD feature history are not preserved in that component.

The importer checks triangle data and topology. Closed, consistently oriented meshes can use material density and their enclosed volume. Open surfaces need a mass override; model flight distributes that entered mass across the actual surface. Length and diameter edits scale the triangles. The exterior radius profile is calculated by slicing triangles, including hollow CAD surfaces, rather than treating every vertex ring as an exterior contour. Existing aerodynamic tables remain in use for imported models; importing geometry does not produce validated CFD-derived flight coefficients.

**Export** offers the whole vehicle, selected stage or selected part. Inner tubes, couplers, centering rings, engine blocks and bulkheads have evaluated geometry in the export. Packed recovery, prescribed payloads and generic motor hardware remain in mission inputs. Fittings controls illustrative seams, retainers and nozzle contours. The **Blender + flight bundle** option retains the full evaluated-model workflow. Export/import conversion runs in the background and reports failures with a converter log. Geometry input is limited to one million triangles and 256 MiB; increasing CAD tessellation tolerance can reduce overly large models.

```powershell
cargo run -- import-geometry --mission examples/advanced-laboratory.json --input rocket.stl --units millimeters --axis z --replace --out output/imported.json
cargo run -- export-geometry --mission output/imported.json --out output/rocket.obj --units meters --axis y
cargo run -- export-geometry --mission output/imported.json --out output/stage.step --stage 0
```

To reuse an already evaluated model, supply `--model output/design/rocket.mesh.json` with its matching mission. `--component N` requires `--stage N`. `--no-fittings` excludes illustrative fittings.

# Builder attachments

Drag a card from **Parts** into the 3D editor. Click the placed part for its properties and selection box. Body tubes and transitions attach at tube ends, extend the current stage and move downstream parts. A transition is a regular tapered section with independent fore and aft diameters; it does **not** create another booster. The next body tube matches the adjoining transition's aft diameter. A nose attaches at the top. Use **Move in 3D** to place an existing component at another attachment point.

Inner tubes attach inside a body tube. Centering rings attach around the nearest inner tube; **Fit to tubes** sets the outer diameter to the body bore and the inner diameter to the mount tube's outside diameter. The parent selector changes the containing tube. Parent movement carries its children; ring fitting responds to diameter / wall edits. Disabling automatic fitting allows a manually entered ring bore. Invalid containment blocks launch and is shown in Properties.

Select an inner tube and enable **Motor mount**, or drag the Motor card onto a stage containing an inner tube. Select a motor curve from that mount's properties. Choices are filtered by its bore and available length, including overhang. The motor CG and preview follow the mount's aft end. One active motor mount per stage is currently supported. Entered motor dimensions allow custom curves to be paired with their actual casing geometry. Empty casing mass still belongs in measured dry mass or hardware allocation; the thrust curve supplies propellant mass separately.
