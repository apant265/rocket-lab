# RASAero II, OpenRocket and the local Plumbbob evidence

Reviewed October 10, 2026. The spelling in the request is interpreted as **RASAero II**. Local files came from `C:/Users/apant/Downloads`; CFD results came from `C:/Users/apant/workspace/ISRP Karman PDR Review/CFD`. The related Codex chat was “Load latest Rocket conversation”. Original files are copied into `sources/` without modification.

## Software review

| Source | Useful role here | Implementation decision |
|---|---|---|
| [RASAero II](https://rasaero.com/home.htm) | Reports Mach 0.01–25 aerodynamics, power-on/off coefficients, CP variation and multistage flight | Prefer exported, configuration-specific coefficients and flight outputs for performance comparisons |
| [OpenRocket technical documentation](https://openrocket.sourceforge.net/techdoc.pdf) | Explains rigid-body forces/moments, quaternion RK4, mass properties and an event-driven simulator architecture | Separate dynamics, environment, configuration, imported data and visualization |
| Supplied OpenRocket `.ork` | Rich component geometry, named avionics, mass allocations, recovery packing and motor assignments | Inspect components and require explicit mapping of overrides, offsets and deployment settings |

These references inform architecture; this project does not reproduce either application's proprietary/complete aerodynamic model or inherit its validation evidence. Rendering a flight in Bevy adds no predictive fidelity.

## The actual local inputs

`Plumbbob_MKIII_R75.CDX1` contains a 20 inch conical nose, 70.5 inch body and 50.5/62 inch boosters. The saved simulation names CTI N5800-CS, CTI O3400-IM-P and AeroTech O6000W-P motors. Its native launch weights are 48.1, 96.4 and 182 lb; these may be configuration-level cumulative weights, so they must not be added as three independent stage masses. The saved maximum altitude is **497,861.3 ft = 151.748 km** and maximum speed is 5,746.049 ft/s. The file contains `UseBooster1/2=False` in the design and `IncludeBooster1/2=True` in the saved simulation. Resolve those settings against the actual generating run.

`Plumbbob_MKIII_R75 PDR Flight Data.CSV` has **2,456 samples**, peak altitude **551,643.6 ft = 168.141 km**, peak speed **6,008.23 ft/s = 1,831.309 m/s**, and peak Mach **6.2076**. Its initial weight is **182 lb = 82.554 kg** and pitch attitude is 87 degrees. This is a trajectory export, not flight instrumentation and not a Mach-sweep aero file. It does not by itself supply the complete powered/coast aerodynamic model for every configuration. Its apogee differs by about **16.39 km** from the older saved CDX1 output. Establish run identity before treating those as the same simulation.

`Plumbbob_MKIII_R75 (1).ork` includes named payload flight computers “MUON and ANVIL”, KATE 3 tracking, multiple mass overrides, payload recovery components and booster avionics. Several saved parachute events are `never`; the bottom-stage design includes an “Experimental Motor” mass component. The native configuration therefore needs deliberate reconciliation with the RASAero flight setup rather than automatic adoption of its saved event or motor assignments. See the generated `openrocket-design-inspection.json` and `rasaero-design-inspection.json` alongside this review.

`gpu_drag_table.csv` covers the **coasting sustainer/payload body at 15 km, Mach 1.5–5.3**, with reference area based on the **4.02 inch diameter**. The 0.15447 total Cd at Mach 5.3 explicitly excludes fin wave drag. Nose/body pressure drag was computed with axisymmetric inviscid CFD; base drag and body/fin skin friction include engineering additions. The accompanying grid checks report small changes in nose wave drag with refinement. These checks support the tested components; they do not establish complete-stack drag, powered base drag, fin interference, heating, flutter or whole-vehicle trajectory accuracy.

The table stops below the trajectory CSV's peak Mach 6.21 and contains no subsonic/transonic stations, no powered drag columns, no CN derivative and no CP curve. The importer rejects it as an incomplete vehicle aero table. It remains valuable supporting evidence and should be compared component by component with RASAero for the same configuration and reference area.

## Supplied motor database and provisional configuration

The related chat located `Downloads/rasp.eng` while this implementation was in progress. Its 897-entry database is now preserved in `sources/rasp.eng`. Named selection imports the motor variants explicitly rather than selecting a different similarly named curve.

| Motor | Propellant kg | Loaded kg | Integrated impulse N s | Last curve time s |
|---|---:|---:|---:|---:|
| AeroTech O6000W | 21.999 | 31.5246 | 39,687.214 | 6.828 |
| CTI O3400-IM-P | 11.272 | 16.842 | 21,041.011 | 6.322 |
| CTI N5800-CS | 9.425 | 14.826 | 20,367.689 | 3.594 |

Combined curve impulse is about 81,095.9 N s. This confirms available motor data; it does not establish which database revisions generated the newer export. The newer CSV enters the sustainer configuration at 20.20027 s with a weight of **49.4 lb**, whereas the older CDX1 gives **48.1 lb**. Its initial CG is **140 in**, versus the older CDX1's **137 in**. Those are concrete differences to reconcile.

`examples/plumbbob-provisional.json` is an explicit source-mapped experiment. Cumulative CDX1 masses are differenced into standalone stage wet masses; imported fuel mass is removed to form stage dry mass. The generator solves dry CGs to match the older cumulative loaded CGs, **assuming motor CG at 75% of each stage's length**. Cylinder inertia, synthetic full-stack aerodynamic curves, damping, thermal nodes, separation speed and avionics remain assumptions. It adds stage lengths rather than resolving nested-stage geometry and uses the standard atmosphere rather than the CDX1 temperature override. Payload/avionics mass is included in dry mass to prevent double counting. This provisional configuration is useful for replacing assumptions incrementally, not for accepting a particular altitude prediction.

## What is still needed to predict this vehicle

1. Identify the exact configuration/run that produced the newer CSV and resolve the older output and enable-flag discrepancy.
2. Reconcile the supplied motor revisions with the generating run, and confirm motor mass/CG histories and measured structural loads. Single-entry/named RASP and RSE thrust curves are supported; embedded RSE mass/CG histories are not yet consumed.
3. Map component and configuration masses, CGs and inertia consistently; account for nested boosters and overlaps rather than simply summing visual lengths.
4. Export complete powered/coast aero tables for all three remaining-stack configurations, including CN/CP and a documented reference area; extend/cross-check the high-speed and rarefied domains.
5. Set measured launch atmosphere/wind, actual avionics envelope, firing logic, deployment/inflation/line limits and material/thermal properties.
6. Compare independent flights and refine the models. The shipped synthetic sounding example is not calibrated to the supplied reference and must not be interpreted as corroborating its apogee.

## Source fingerprints (SHA-256)

| File | SHA-256 |
|---|---|
| CDX1 | `F04329897F0F0F4361930AFD089FE83BCFDDE2CEC990D31AFEF19D9860C67855` |
| Flight CSV | `D5FC4927DF24ED422D57AB82B30EE8E33FC67B93FF67FBF270A66F53906B6987` |
| ORK | `985F1B4540EDBDB1829BB4216CA5795EE0A54E355ACAC3CE768017BA98532C24` |
| CFD table | `3A859BF9B7AA80FE32E279EC377DF6AA6FB493DBAD6329F7E3E7F71A47EF05EB` |
| RASP database | `73636885B3B125D59D245D40B36EF2E23452A2125A788FCD7A9C031A4060059D` |
