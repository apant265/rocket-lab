"""Blender scene/mesh exchange, evaluated with object transforms and modifiers."""
import json
import sys
import traceback
from pathlib import Path
import bpy

request = json.loads(Path(sys.argv[sys.argv.index('--')+1]).read_text(encoding='utf-8'))
result_path = Path(request['result'])
def clean(path):
    return Path(path).as_posix().removeprefix('//?/')

def convert():
    bpy.ops.object.select_all(action='SELECT')
    bpy.ops.object.delete(use_global=False)
    if request['operation'] == 'import':
        path = clean(request['input'])
        ext = Path(path).suffix.lower()
        if ext == '.blend':
            bpy.ops.wm.open_mainfile(filepath=path, load_ui=False, use_scripts=False)
        elif ext in ('.glb', '.gltf'):
            bpy.ops.import_scene.gltf(filepath=path)
        elif ext == '.ply':
            bpy.ops.wm.ply_import(filepath=path)
        elif ext == '.fbx':
            bpy.ops.import_scene.fbx(filepath=path)
        elif ext == '.dae':
            if not hasattr(bpy.ops.wm, 'collada_import'):
                raise ValueError('This Blender installation does not include Collada import; use GLB or OBJ')
            bpy.ops.wm.collada_import(filepath=path)
        positions, indices = [], []
        deps = bpy.context.evaluated_depsgraph_get()
        for obj in bpy.context.scene.objects:
            if obj.type != 'MESH':
                continue
            evaluated = obj.evaluated_get(deps)
            mesh = evaluated.to_mesh()
            mesh.calc_loop_triangles()
            base = len(positions)
            positions.extend([list(evaluated.matrix_world @ v.co) for v in mesh.vertices])
            indices.extend(base + i for tri in mesh.loop_triangles for i in tri.vertices)
            evaluated.to_mesh_clear()
            if len(indices) > 3_000_000:
                raise ValueError('Scene exceeds 1,000,000 triangles')
        if not indices:
            raise ValueError('Scene contains no evaluated mesh geometry')
        geometry = result_path.with_name('geometry.json')
        geometry.write_text(json.dumps({'positions': positions, 'indices': indices}, separators=(',', ':')), encoding='utf-8')
        return {'ok': True, 'geometry': str(geometry), 'backend': bpy.app.version_string}
    for part in request['model']['parts']:
        # Blender +Z nose, meters; glTF exporter handles its standard +Y scene basis.
        if request['format'] == 'ply':
            axis = request['axis']
            vertices = [(p[1],-p[0],p[2]) if axis == 'x' else tuple(p) if axis == 'y' else (p[0],-p[2],p[1]) for p in part['positions']]
        else:
            vertices = [(p[0], -p[2], p[1]) for p in part['positions']]
        faces = [part['indices'][i:i+3] for i in range(0, len(part['indices']), 3)]
        mesh = bpy.data.meshes.new(part['name'])
        mesh.from_pydata(vertices, [], faces)
        mesh.update()
        obj = bpy.data.objects.new(part['name'], mesh)
        bpy.context.collection.objects.link(obj)
        mat = bpy.data.materials.new(part['name']+' material')
        mat.diffuse_color = (*part['base_color'], 1)
        obj.data.materials.append(mat)
        obj['rocket_stage'] = part['stage']
        obj['component_index'] = part.get('component_index', -1)
    output = clean(request['output'])
    ext = request['format']
    if ext in ('glb', 'gltf'):
        bpy.ops.export_scene.gltf(filepath=output, export_format='GLB' if ext == 'glb' else 'GLTF_SEPARATE', export_extras=True)
    elif ext == 'blend':
        bpy.ops.wm.save_as_mainfile(filepath=output)
    elif ext == 'ply':
        bpy.ops.wm.ply_export(filepath=output, global_scale=1/request['units'])
    elif ext == 'fbx':
        bpy.ops.export_scene.fbx(filepath=output, object_types={'MESH'}, add_leaf_bones=False)
    elif ext == 'dae':
        bpy.ops.wm.collada_export(filepath=output)
    if not Path(output).is_file() or Path(output).stat().st_size == 0:
        raise ValueError('Scene exporter did not write a file')
    return {'ok': True, 'output': output, 'backend': bpy.app.version_string}
try:
    result = convert()
except Exception as error:
    traceback.print_exc()
    result = {'ok': False, 'error': str(error)}
result_path.write_text(json.dumps(result), encoding='utf-8')
print('ROCKET_SCENE_RESULT ' + json.dumps(result))
if not result['ok']:
    sys.exit(1)
