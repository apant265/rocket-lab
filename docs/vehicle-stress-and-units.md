# Vehicle stress and display units

Choose **Imperial** or **Metric** in the header. Imperial is the default: parts
use inches, altitude uses feet, wind uses mph, flight speed uses ft/s, mass uses
pounds, pressure uses psi, stress uses ksi, and temperature uses °F. Thermal,
density, energy, area, volume, inertia and force properties also convert. The
preference is saved to `output/ui-units.json`. Property values support dragging,
typing and +/− steps. Explanations appear in hover tooltips.

Unit changes affect display only. Rust flight and analysis use SI; mission JSON
and raw solver CSVs keep their explicitly named SI units. Analysis PNGs and their
metadata use the selected display units. Switching units does not rewrite a part
or change the flight. Absolute temperature and temperature differences use
separate conversions.

## Stress at a flight timestamp

Choose **Simulate → Stress → Compute**. The thermal prefix supplies local material
temperatures. The result covers every retained body/nose section and planar fin
region, with a default vehicle-shaped 2D image, a 3D overlay, section material
details and `vehicle-stress.json`. Detached stages are excluded from the retained
vehicle result. Unknown mounts and hardware are gray and listed as unresolved;
their mass still contributes to section loads.

For each material, open **Build → Material → Structural properties** and enter
Young's modulus, Poisson ratio, optional allowable stress and optional measured
temperature factors. Factors interpolate linearly in temperature, clamp outside
the supplied range, and scale stiffness and strength separately. No table means
constant properties. Preset names and densities do not imply known strength.
Missing modulus leaves the scalar stress/E indicator unavailable; missing strength
leaves the strength/stress ratio unavailable. The legacy primary-fin material law
remains a compatibility fallback when the material has no entered mechanical data.

**Built-in** uses CPU quasi-static annular beam/shell section loads: axial force,
bending moment, transverse shear and estimated external-pressure hoop stress,
combined as a peak section von Mises value. Planar fins use tapered cantilever
integration. Inertia loads come from recorded acceleration, section/hardware,
payload and remaining propellant masses; aerodynamic resultants come from the
recorded q, incidence and coefficients. For a Blender mass model, the configured
section distribution is calibrated to its measured mesh mass and centroid. Wall
thickness and local radius determine the section properties. Material stiffness
affects the stress/E indicator; allowable strength affects the strength ratio.
A statically determined beam need not change stress when only modulus changes.

**External (CPU)** runs actual CalculiX S8 quadratic shells for the revolved
retained body/nose envelope, with a distinct material and wall thickness for each
axial section. Primary fins use separate C3D20R quadratic solid models. Additional
planar fin sets retain their explicitly labeled beam screening. Every shell and
primary fin needs entered modulus; allowable strength remains optional. The shell
uses an aft clamp, lumped inertia/aero resultants and estimated body pressure.
Neighboring section rings are bonded; diameter discontinuities are idealized at
their common ring. The full deck, integration-point tensors, nodal displacements
and solver logs are preserved. A failed external solve does not substitute a
built-in result. Refinement increases axial sections, circumferential shell
elements (16/24/32/48) and fin elements.

Each external primary fin solid uses its material evaluated at a uniform root
temperature; its structural result records that temperature. The built-in fin
screening evaluates the computed span temperatures. External fin solids do not
yet resolve a temperature-dependent modulus gradient across the fin mesh.

The material grid includes explicit shell-part boundaries, preserving short
sections that could otherwise fall between uniform cell centers. External thermal
strips remain independent across changes in material, region or cell width.

The displayed vehicle colors are the **peak section envelope**; they do not show
resolved circumferential variations. The stress/E indicator is not a tensor strain
field. The strength ratio is allowable/stress, not a certified safety factor.
Body displacement is available from external FEA; the built-in body does not
solve displacement. GPU acceleration remains available for built-in flow and
material thermal diffusion. Flight dynamics and structural solves use the CPU.

These structural models do not resolve joint/contact mechanics, hardware mount
stresses, anisotropic laminate failure, buckling, thermal expansion, fracture,
flutter or transient flexible-body motion. They post-process the recorded flight
and do not feed deformation or failure back into the trajectory.

## Verification

`python scripts/check_vehicle_stress.py` runs real CalculiX against an axial shell
reference using explicitly synthetic verification material data. The checked
100 mm-radius, 1 mm-wall cylinder under 1,000 N matched stress within 0.16% and end
displacement within 0.20%. Halving modulus over half its length gave displacement
ratio **1.50124** against **1.5**. The script preserves cases and writes
`output/vehicle-stress-verification/check.json`. Rust tests cover section equations,
material serialization/validation, temperature factors, missing data, field-to-mesh
mapping, display-unit round trips and property stepping. These are numerical
verification cases, not measured-flight or vehicle-failure validation.

Solver element, material and output definitions follow the
[CalculiX author's manual](https://www.dhondt.de/ccx_2.21.pdf).
