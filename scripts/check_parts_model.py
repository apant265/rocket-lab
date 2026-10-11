"""Check actual headless Blender exports against custom fin/tube component inputs."""
import argparse
import json
import math
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('--mission', required=True)
parser.add_argument('--model', required=True)
parser.add_argument('--out', required=True)
args = parser.parse_args()
mission = json.loads(Path(args.mission).read_text(encoding='utf-8'))
model = json.loads(Path(args.model).read_text(encoding='utf-8'))
checks = []

def dot(a, b):
    return sum(x*y for x, y in zip(a, b))

def cross(a, b):
    return [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]

def volume(part):
    v = part['positions']; t = part['indices']
    return sum(dot(v[t[i]], cross(v[t[i+1]], v[t[i+2]]))/6 for i in range(0, len(t), 3))

for part in model['parts']:
    assert all(math.isfinite(x) for v in part['positions'] for x in v)
    assert all(math.isfinite(x) for v in part['normals'] for x in v)
    assert len(part['indices']) % 3 == 0
    assert all(0 <= i < len(part['positions']) for i in part['indices'])

for stage_index, stage in enumerate(mission['stages']):
    offset = sum(s['length_m'] for s in mission['stages'][stage_index+1:])
    for p in stage.get('components', []):
        kind = p['kind']
        if kind in ('fin_set', 'elliptical_fin', 'freeform_fin'):
            g = p['shape']
            if kind == 'freeform_fin': points = p['fin_points']
            elif kind == 'elliptical_fin':
                points = [[g['fin_root_chord_m']*(1-math.cos(math.pi*i/48))*.5,
                           g['fin_span_m']*math.sin(math.pi*i/48)] for i in range(49)]
            else: points = [[0, 0], [g['fin_sweep_m'], g['fin_span_m']],
                            [g['fin_sweep_m']+g['fin_tip_chord_m'], g['fin_span_m']], [g['fin_root_chord_m'], 0]]
            area = abs(sum(a[0]*b[1]-a[1]*b[0] for a,b in zip(points, points[1:]+points[:1])))*.5
            expected = area*g['fin_thickness_m']
            for i in range(g['fin_count']):
                name = p['name']+f' {i+1}'
                actual = next(part for part in model['parts'] if part['stage'] == stage_index and part['name'] == name)
                error = abs(volume(actual)/expected-1)
                assert error < 1e-4, (name, error)
                checks.append({'part':name, 'check':'positive enclosed fin volume', 'relative_error':error})
        elif kind in ('tube_fin', 'pod_set', 'launch_lug'):
            for i in range(p['instance_count']):
                name = p['name']+f' {i+1}'
                actual = next(part for part in model['parts'] if part['stage'] == stage_index and part['name'] == name)
                v = actual['positions']
                center = [(min(x[k] for x in v)+max(x[k] for x in v))*.5 for k in range(3)]
                angle = math.radians(p['radial_angle_deg'])+i*math.tau/p['instance_count']
                expected = [p['radial_offset_m']*math.cos(angle), -(offset+p['position_m']+p['length_m']*.5),
                            -p['radial_offset_m']*math.sin(angle)]
                error = max(abs(a-b) for a,b in zip(center, expected))
                assert error < 1e-6, (name, error)
                assert abs(max(x[1] for x in v)-min(x[1] for x in v)-p['length_m']) < 1e-6
                checks.append({'part':name, 'check':'instance position and length', 'max_position_error_m':error})
assert checks, 'no new parts tested'
Path(args.out).write_text(json.dumps({'passed':True, 'checks':checks, 'mesh_parts':len(model['parts'])},indent=2),encoding='utf-8')
print(f'Passed {len(checks)} custom-part checks against {len(model["parts"])} evaluated Blender meshes.')
