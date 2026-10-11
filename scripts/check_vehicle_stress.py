"""Real CalculiX verification: axial shell solution and local material response.

Synthetic verification properties only; these are not rocket material allowables.
Run from the project root: python scripts/check_vehicle_stress.py
"""
import importlib.util
import json
import math
from pathlib import Path

spec = importlib.util.spec_from_file_location("solver", Path(__file__).with_name("openfoam_analysis.py"))
solver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(solver)
root = Path("output/vehicle-stress-verification").resolve()
root.mkdir(parents=True, exist_ok=True)

def run(name, moduli):
    case = root / name
    case.mkdir(exist_ok=True)
    case=case/(__import__('uuid').uuid4().hex[:8]);case.mkdir()
    # Eight 125 mm axial sections of a 100 mm radius, 1 mm wall cylinder.
    shells = [dict(column=i, stage=0, start=i/8, end=(i+1)/8,
                   r0=.1, r1=.1, thickness=.001, youngs=moduli[i],
                   poisson=.3, allowable=100e6, temperature_k=288.15,
                   material="Synthetic verification material", pressure=0.)
              for i in range(8)]
    request = dict(shells=shells, loads=[dict(station_m=0., force_n=[0.,0.,-1000.])], circumference=24,radius_reference="mid_surface")
    result=solver.vehicle_stress(case,request)
    (case/"result.json").write_text(json.dumps(result,indent=2))
    return result

if __name__ == "__main__":
    uniform=run("uniform",[100e9]*8)
    changed=run("two-materials",[50e9]*4+[100e9]*4)
    analytical=1000/(2*math.pi*.1*.001)
    # Aft clamp introduces a local boundary layer; check axial stress away from it.
    axial=uniform["rows"][1]["axial_pa"]
    assert abs(abs(axial)/analytical-1)<.05, (axial,analytical)
    expected=1000/(2*math.pi*.1*.001*100e9)
    tip=uniform["rows"][0]["deflection_m"]
    softer=changed["rows"][0]["deflection_m"]
    assert abs(tip/expected-1)<.05, (tip,expected)
    assert abs(softer/tip-1.5)<.05, (softer,tip)
    assert changed["checks"]["material_sections"]==8
    report=dict(completed=True,backend=uniform["backend"],axial_pa=axial,
                reference_axial_pa=analytical,uniform_tip_m=tip,
                reference_tip_m=expected,two_material_tip_m=softer,
                expected_displacement_ratio=1.5,actual_displacement_ratio=softer/tip)
    (root/"check.json").write_text(json.dumps(report,indent=2))
    print(json.dumps(report,indent=2))
