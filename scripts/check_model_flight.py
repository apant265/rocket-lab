"""Verify exported geometry/flight receipts without launching another solver."""
import argparse
import json
import math
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('--out', default='output/model-flight-check.json')
args = parser.parse_args()
checks = []
def check(name, passed, detail=None):
    checks.append({'name': name, 'passed': bool(passed), 'detail': detail})

def volume(part):
    points = part['positions']; total = 0.0
    for n in range(0,len(part['indices']),3):
        a,b,c = [points[part['indices'][n+i]] for i in range(3)]
        cross = [b[1]*c[2]-b[2]*c[1], b[2]*c[0]-b[0]*c[2], b[0]*c[1]-b[1]*c[0]]
        total += sum(a[i]*cross[i] for i in range(3))/6
    return abs(total)

for case in ['model-flight','model-material','model-parts']:
    root = Path('output')/case
    mesh = json.loads((root/'rocket.mesh.json').read_text())
    mission = json.loads((root/'flight/mission.json').read_text())
    summary = json.loads((root/'flight/summary.json').read_text())
    check(case+' evaluated geometry', mesh['physics_version']==1 and all((root/file).is_file() for file in ['rocket.blend','rocket.glb','rocket.mesh.json']))
    check(case+' completed flight', summary['completed'], summary)
    for i,s in enumerate(mission['stages']):
        mass = s['model_mass']; matrix = mass['inertia_kg_m2']
        determinant = (matrix[0][0]*(matrix[1][1]*matrix[2][2]-matrix[2][1]*matrix[1][2])
            - matrix[1][0]*(matrix[0][1]*matrix[2][2]-matrix[2][1]*matrix[0][2])
            + matrix[2][0]*(matrix[0][1]*matrix[1][2]-matrix[1][1]*matrix[0][2]))
        check(case+f' stage {i+1} positive mass/tensor', mass['mass_kg']>0 and determinant>0 and all(math.isfinite(v) for row in matrix for v in row), mass)
        check(case+f' stage {i+1} geometry checksum',len(mass['mesh_checksum_fnv1a64'])==16)
    if case=='model-flight':
        check('Kate/Anvil payload is 5 lb', abs(mission['stages'][-1]['payload_mass_kg']/0.45359237-5)<1e-12)
        for name in ['telemetry.csv','events.json','summary.json']:
            replay= root/'replay'/name
            check('saved model deterministic replay '+name,replay.read_bytes()==(root/'flight'/name).read_bytes())
    if case=='model-material':
        stage=mission['stages'][0]; structure=stage['materials']; r=stage['diameter_m']/2
        nose=stage['geometry']['nose_length_m']; wall=structure['airframe_wall_m']
        body=next(p for p in mesh['parts'] if p['kind']=='body' and p.get('component_index') is None)
        expected=math.pi*(r*r-(r-wall)**2)*(stage['length_m']-nose)*math.sin(math.tau/96)/(math.tau/96)
        error=abs(volume(body)/expected-1)
        check('hollow cylinder volume against analytic 96-sided annulus',error<2e-5,{'relative_error':error,'mesh_volume_m3':volume(body),'analytic_volume_m3':expected})
        expected_mass=structure['hardware_mass_kg']+sum(volume(p)*structure[{'body':'airframe','nose':'nose','fin':'fins'}[p['kind']]]['density_kg_m3'] for p in mesh['parts'] if p['kind'] in ['body','nose','fin'])
        check('material mass uses actual shell and fin volumes',abs(expected_mass-stage['model_mass']['mass_kg'])<2e-5,{'mesh_dry_kg':stage['model_mass']['mass_kg'],'independent_kg':expected_mass})
parts=json.loads(Path('output/model-parts-geometry-check.json').read_text())
check('custom part evaluated geometry checks',parts.get('passed',False),parts)
report={'passed':all(c['passed'] for c in checks),'checks':checks}
Path(args.out).write_text(json.dumps(report,indent=2))
print(f"{sum(c['passed'] for c in checks)}/{len(checks)} model-flight checks passed")
if not report['passed']:
    for c in checks:
        if not c['passed']: print('FAILED:',c['name'],c['detail'])
    raise SystemExit(1)
