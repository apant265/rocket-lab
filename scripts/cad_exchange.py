"""Run with FreeCADCmd; JSON request/result avoids command-string interpolation."""
import json
import os
import traceback
from pathlib import Path
import FreeCAD as App
import Part
import Mesh

request = json.loads(Path(os.environ['ROCKET_LAB_CAD_REQUEST']).read_text(encoding='utf-8'))
result_path = Path(request['result'])

def clean(path):
    return str(path).removeprefix('\\\\?\\')

def convert():
    if request['operation'] == 'import':
        path = Path(request['input'])
        if path.suffix.lower() == '.fcstd':
            doc = App.openDocument(clean(path))
            # Export leaf geometry once, honoring object/global placements.
            shapes = []
            for obj in doc.Objects:
                if not hasattr(obj, 'Shape') or obj.Shape.isNull():
                    continue
                if any(hasattr(child, 'Shape') and not child.Shape.isNull() for child in getattr(obj, 'InList', [])):
                    continue
                shape = obj.Shape.copy()
                if hasattr(obj, 'getGlobalPlacement'):
                    shape.Placement = obj.getGlobalPlacement()
                shapes.append(shape)
        else:
            shape = Part.Shape()
            shape.read(clean(path))
            shapes = [shape]
        positions, indices = [], []
        for shape in shapes:
            points, triangles = shape.tessellate(float(request['tolerance_mm']))
            base = len(positions)
            positions.extend([[p.x, p.y, p.z] for p in points])
            indices.extend(base + i for tri in triangles for i in tri)
            if len(indices) > 3_000_000:
                raise ValueError('CAD tessellation exceeds 1,000,000 triangles; increase tolerance')
        if not indices:
            raise ValueError('CAD file contains no tessellatable 3D shape')
        geometry = result_path.with_name('geometry.json')
        geometry.write_text(json.dumps({'positions': positions, 'indices': indices}, separators=(',', ':')), encoding='utf-8')
        return {'ok': True, 'geometry': str(geometry), 'backend': App.Version()[0:3], 'units': 'mm'}
    doc = App.newDocument('RocketLabExport')
    objects = []
    axis = request['axis']
    def point(p):
        x, y, z = p
        return tuple(v * 1000 for v in ((y, -x, z) if axis == 'x' else (x, y, z) if axis == 'y' else (x, -z, y)))
    for index, part in enumerate(request['model']['parts']):
        positions = [point(p) for p in part['positions']]
        faces = part['indices']
        facets = [[positions[faces[i+j]] for j in range(3)] for i in range(0, len(faces), 3)]
        mesh = Mesh.Mesh(facets)
        shape = Part.Shape()
        shape.makeShapeFromMesh(mesh.Topology, 0.001)
        if mesh.isSolid() and shape.Shells:
            solids = [Part.makeSolid(shell) for shell in shape.Shells]
            shape = solids[0] if len(solids) == 1 else Part.makeCompound(solids)
        obj = doc.addObject('Part::Feature', f'Part{index+1}')
        obj.Label = part['name']
        obj.Shape = shape
        objects.append(obj)
    doc.recompute()
    output = clean(request['output'])
    if request['format'] == 'fcstd':
        doc.saveAs(output)
    else:
        Part.export(objects, output)
    if not Path(output).is_file() or Path(output).stat().st_size == 0:
        raise ValueError('CAD exporter did not write a file')
    return {'ok': True, 'output': output, 'parts': len(objects), 'representation': 'faceted BREP; no parametric feature history'}

try:
    result = convert()
except Exception as error:
    traceback.print_exc()
    result = {'ok': False, 'error': str(error)}
result_path.write_text(json.dumps(result), encoding='utf-8')
print('ROCKET_CAD_RESULT ' + json.dumps(result))
