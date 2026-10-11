"""Check a saved whole-vehicle stress field for coverage and material response.

python scripts/check_vehicle_field.py --mission M.json --field vehicle-stress.json
Uses entered mechanical data only; the legacy primary-fin fallback is flagged.
"""
import argparse
import json
import math
from pathlib import Path

def adjusted(material, temperature):
    m=material.get('mechanical')
    if m is None:return None,None
    points=m.get('temperature_factors',[])
    stiffness=strength=1.
    if points:
        a=points[0];b=a
        for point in points:
            if point[0]<=temperature:a=point
            if point[0]>=temperature:b=point;break
            b=point
        u=max(0.,min(1.,(temperature-a[0])/max(1e-9,b[0]-a[0])))
        stiffness=a[1]+u*(b[1]-a[1]);strength=a[2]+u*(b[2]-a[2])
    return m['youngs_modulus_pa']*stiffness,(m['allowable_stress_pa']*strength if m.get('allowable_stress_pa') is not None else None)

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--mission',required=True);parser.add_argument('--field',required=True);args=parser.parse_args()
    mission=json.loads(Path(args.mission).read_text(encoding='utf-8'));field=json.loads(Path(args.field).read_text(encoding='utf-8'))
    materials={}
    for stage in mission['stages']:
        for kind in ('airframe','nose','fins'):
            m=stage['materials'][kind];materials[m['name']]=m
        for part in stage.get('components',[]):materials[part['material']['name']]=part['material']
    assert field['points'] and len({p['column'] for p in field['points']})==len(field['points'])
    checked=0
    for p in field['points']:
        assert math.isfinite(p['stress_pa']) and p['stress_pa']>=0
        e,strength=adjusted(materials[p['material']],p['temperature_k'])
        if e is not None:
            assert p['strain'] is not None and math.isclose(p['strain'],p['stress_pa']/e,rel_tol=1e-10,abs_tol=1e-12)
            if strength is not None and p['stress_pa']>1e-9:assert math.isclose(p['margin'],strength/p['stress_pa'],rel_tol=1e-10)
            elif strength is None:assert p['margin'] is None
            checked+=1
    kinds={p['kind'] for p in field['points']}
    assert {'body','nose','fin'}<=kinds
    report=dict(passed=True,backend=field['backend'],points=len(field['points']),kinds=sorted(kinds),stages=sorted({p['stage']+1 for p in field['points']}),material_response_checked=checked,peak_pa=max(p['stress_pa'] for p in field['points']))
    Path(args.field).with_name('vehicle-field-check.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps(report,indent=2))

if __name__=='__main__':main()
