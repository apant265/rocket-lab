"""Run in a fresh headless Blender: blender --background --factory-startup --python ... -- ...
Writes actual evaluated Blender geometry as .blend, .glb and a Bevy mesh JSON.
The scene belongs only to this background process; the user's interactive Blender is untouched.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import sys
import xml.etree.ElementTree as ET
import bpy
from mathutils import Vector

parser = argparse.ArgumentParser()
parser.add_argument('--mission', required=True)
parser.add_argument('--design')
parser.add_argument('--out', required=True)
args = parser.parse_args(sys.argv[sys.argv.index('--') + 1:])
mission_path = Path(args.mission).resolve()
mission = json.loads(mission_path.read_text(encoding='utf-8'))
out = Path(args.out).resolve()
out.mkdir(parents=True, exist_ok=True)
bpy.context.preferences.filepaths.save_version = 0
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
stages = mission['stages']
design = ET.parse(args.design).getroot().find('RocketDesign') if args.design else None
native = ([design.find('NoseCone'), design.find('BodyTube')] + list(design.findall('Booster'))) if design is not None else []

def field(element, tag, default=0):
    return float(element.findtext(tag, str(default))) * 0.0254

def material(name, color, metallic=0, roughness=.35):
    mat = bpy.data.materials.new(name)
    mat.diffuse_color = (*color, 1)
    mat.use_nodes = True
    bsdf = next(n for n in mat.node_tree.nodes if n.type == 'BSDF_PRINCIPLED')
    bsdf.inputs['Base Color'].default_value = (*color, 1)
    bsdf.inputs['Metallic'].default_value = metallic
    bsdf.inputs['Roughness'].default_value = roughness
    return mat

ivory = material('Ceramic white', (.65, .73, .79), .2)
carbon = material('Carbon graphite', (.055, .085, .105), .35)
alloy = material('Brushed titanium', (.38, .45, .49), .85, .24)
orange = material('Recovery orange', (.9, .24, .035), .1)
black = material('Nozzle graphite', (.015, .02, .026), .15)
objects = []

def mesh_object(name, vertices, faces, stage, mat, kind, smooth=False):
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    obj.data.materials.append(mat)
    for poly in mesh.polygons:
        poly.use_smooth = smooth
    obj['stage'] = stage
    obj['surface_kind'] = kind
    objects.append(obj)
    return obj

def lathe(name, rings, stage, mat, kind, segments=96, caps=True):
    # (station aft of whole-stack nose, radial radius), Blender Z points to nose.
    vertices = [(r * math.cos(j*math.tau/segments), r * math.sin(j*math.tau/segments), -s)
                for s, r in rings for j in range(segments)]
    faces = []
    for i in range(len(rings)-1):
        for j in range(segments):
            a = i*segments+j
            b = i*segments+(j+1)%segments
            faces.append((a, a+segments, b+segments, b))
    if caps:
        faces.append(tuple(range(segments-1, -1, -1)))
        last = (len(rings)-1)*segments
        faces.append(tuple(last+j for j in range(segments)))
    obj = mesh_object(name, vertices, faces, stage, mat, kind, True)
    if caps:
        obj.data.polygons[-1].use_smooth = False
        obj.data.polygons[-2].use_smooth = False
    return obj

def build_component(p, offset, stage_index):
    kind = p['kind']; pos = offset+p['position_m']; length = p['length_m']; radius = p['diameter_m']*.5
    planar = kind in ('fin_set', 'elliptical_fin', 'freeform_fin')
    tubes = kind in ('tube_fin', 'launch_lug', 'pod_set')
    internal = kind in ('inner_tube', 'coupler', 'centering_ring', 'engine_block', 'bulkhead')
    if kind == 'custom_mesh':
        data = p['mesh']; radial_scale = p['diameter_m'] / data['diameter_m']; axial_scale = length / data['length_m']
        vertices = [(v[0]*radial_scale, -v[2]*radial_scale, v[1]*axial_scale-pos-length*.5) for v in data['positions']]
        faces = [tuple(data['indices'][i:i+3]) for i in range(0, len(data['indices']), 3)]
        color = p.get('appearance_rgba', [.96, .61, .18, 1])[:3]
        mat = material(p['name']+' finish', color)
        for index in range(p.get('instance_count', 1)):
            angle = math.radians(p.get('radial_angle_deg', 0))+index*math.tau/p.get('instance_count', 1)
            obj = mesh_object(p['name']+f' {index+1}', vertices, faces, stage_index, mat, 'custom_mesh')
            obj.location = (p.get('radial_offset_m', 0)*math.cos(angle), p.get('radial_offset_m', 0)*math.sin(angle), 0)
        return
    if kind not in ('body_tube', 'nose_cone', 'transition') and not planar and not tubes and not internal:
        return
    color = p.get('appearance_rgba', [.96, .61, .18, 1])[:3]
    roughness = {'Rough':.9, 'Normal':.5, 'Smooth':.3, 'Polished':.15}.get(p.get('finish'), .5)
    mat = material(p['name']+' finish', color, roughness=roughness)
    angle0 = math.radians(p.get('radial_angle_deg', 0))
    radial_offset = p.get('radial_offset_m', 0)
    if planar:
        g = p['shape']; chord = g['fin_root_chord_m']; span = g['fin_span_m']; thick = g['fin_thickness_m']
        if kind == 'freeform_fin':
            points = p['fin_points']
        elif kind == 'elliptical_fin':
            points = [[chord*(1-math.cos(math.pi*k/48))*.5, span*math.sin(math.pi*k/48)] for k in range(49)]
        else:
            points = [[0,0], [g['fin_sweep_m'],span], [g['fin_sweep_m']+g['fin_tip_chord_m'],span], [chord,0]]
        n = len(points)
        for index in range(g['fin_count']):
            angle = angle0+index*math.tau/g['fin_count']; vertices = []
            for side in [-1, 1]:
                for station, outboard in points:
                    radial = radius+radial_offset+outboard; tangent = side*thick*.5
                    vertices.append((radial*math.cos(angle)-tangent*math.sin(angle), radial*math.sin(angle)+tangent*math.cos(angle), -(pos+station)))
            faces = [tuple(reversed(range(n))), tuple(range(n, 2*n))]
            faces += [(j,(j+1)%n,(j+1)%n+n,j+n) for j in range(n)]
            mesh_object(p['name']+f' {index+1}', vertices, faces, stage_index, mat, 'fin')
        return
    if kind == 'bulkhead':
        lathe(p['name'], [(pos, radius), (pos+length, radius)], stage_index, mat, 'internal')
        return
    if tubes or internal:
        wall = p['thickness_m']; bore = max(1e-7, radius-wall)
        rings = [(pos,radius), (pos+length,radius), (pos+length,bore), (pos,bore), (pos,radius)]
        for index in range(p.get('instance_count', 1)):
            angle = angle0+index*math.tau/p.get('instance_count', 1)
            obj = lathe(p['name']+f' {index+1}', rings, stage_index, mat, 'internal' if internal else kind, caps=False)
            obj.location = (radial_offset*math.cos(angle), radial_offset*math.sin(angle), 0)
        return
    fore = p.get('fore_diameter_m', 0)*.5 if kind == 'transition' else radius
    rings = []
    for k in range(65):
        u = k/64
        if kind == 'nose_cone':
            shape = p.get('shape', {}).get('nose_shape', 'conical'); r = radius*u
            if shape == 'elliptical': r = radius*math.sqrt(max(0, 1-(1-u)**2))
            if shape == 'tangent_ogive':
                rho = (radius*radius+length*length)/(2*radius); r = math.sqrt(max(0, rho*rho-(length-length*u)**2))+radius-rho
        else: r = fore+(radius-fore)*u
        rings.append((pos+length*u, max(1e-7, r)))
    wall = p['thickness_m']; profile = rings+[(x,max(1e-7,r-wall)) for x,r in reversed(rings)]+[rings[0]]
    lathe(p['name'], profile, stage_index, mat, 'nose' if kind == 'nose_cone' else 'body', caps=False)

def indexed_component(p, offset, stage, index):
    start = len(objects)
    build_component(p, offset, stage)
    for obj in objects[start:]:
        obj['component_index'] = index


def physical_lathe(name, rings, stage, mat, kind):
    structure = stages[stage].get('materials') or {}
    if structure.get('use_material_mass'):
        wall = structure['nose_wall_m' if kind == 'nose' else 'airframe_wall_m']
        profile = rings+[(station,max(1e-7,radius-wall)) for station,radius in reversed(rings)]+[rings[0]]
        return lathe(name, profile, stage, mat, kind, caps=False)
    return lathe(name, rings, stage, mat, kind)


offset = 0
for i in reversed(range(len(stages))):
    stage = stages[i]
    length, radius = stage['length_m'], stage['diameter_m']*.5
    top = i == len(stages)-1
    geometry = stage.get('geometry')
    if stage.get('materials',{}).get('component_shell',False):
        for index,p in enumerate(stage.get('components',[])):indexed_component(p,offset,i,index)
        offset+=length
        continue
    nose_shape = geometry.get('nose_shape', 'conical') if geometry else 'conical'
    nose_length = (geometry['nose_length_m'] if geometry else (field(native[0], 'Length') if native else length*.22)) if top else 0
    body_start = min(length-.001, max([nose_length if top else 0] + [p["length_m"] for p in stage.get("components",[]) if p["kind"] == "transition" and abs(p["position_m"]) < 1e-6]))
    if nose_length:
        tip = min(stage['nose_radius_m'], radius*.08, nose_length*.15)
        slope = (radius-tip)/max(.001, nose_length-tip)
        join_angle = math.atan(1/slope)
        nose_rings = []
        for k in range(20):
            angle = join_angle*k/19
            nose_rings.append((offset + tip*(1-math.cos(angle)), max(1e-7, tip*math.sin(angle))))
        join_s, join_r = nose_rings[-1]
        for k in range(1,61):
            f = k/60
            nose_rings.append((join_s+(offset+nose_length-join_s)*f, join_r+(radius-join_r)*f))
        if nose_shape == 'tangent_ogive':
            rho=(radius*radius+nose_length*nose_length)/(2*radius)
            nose_rings=[(offset+nose_length*k/100,max(1e-7,math.sqrt(max(0,rho*rho-(nose_length-nose_length*k/100)**2))+radius-rho)) for k in range(101)]
        elif nose_shape == 'elliptical':
            nose_rings=[(offset+nose_length*k/100,max(1e-7,radius*math.sqrt(max(0,1-(1-k/100)**2)))) for k in range(101)]
        physical_lathe(f'{nose_shape.replace("_", " ").title()} nose', nose_rings, i, alloy, 'nose')
    rings = [(offset+body_start+(length-body_start)*k/40, radius) for k in range(41)]
    physical_lathe(f'Stage {i+1} airframe', rings, i, ivory if top else carbon, 'body')
    # Coupler seam bands and aft motor retention; resolved in the exported mesh.
    for station in [body_start+.04, length-.055]:
        lathe(f'Stage {i+1} seam', [(offset+station, radius*1.01), (offset+station+.006, radius*1.015),
              (offset+station+.012, radius*1.01)], i, alloy, 'seam')
    fin_element = (native[1].find('Fin') if top else native[-(i+1)].find('Fin')) if native else None
    chord = field(fin_element, 'Chord') if fin_element is not None else length*.14
    tip_chord = field(fin_element, 'TipChord') if fin_element is not None else chord*.4
    sweep = field(fin_element, 'SweepDistance') if fin_element is not None else chord*.65
    span = field(fin_element, 'Span') if fin_element is not None else radius*2
    thickness = field(fin_element, 'Thickness') if fin_element is not None else .005
    count = int(fin_element.findtext('Count', '4')) if fin_element is not None else 4
    placement = field(fin_element, 'Location') if fin_element is not None else max(chord, sweep+tip_chord)
    root_start = offset+length-placement
    if geometry:
        chord=geometry['fin_root_chord_m'];tip_chord=geometry['fin_tip_chord_m'];sweep=geometry['fin_sweep_m']
        span=geometry['fin_span_m'];thickness=geometry['fin_thickness_m'];count=geometry['fin_count']
        root_start=offset+geometry['fin_position_m']
    for fin in range(count):
        angle = fin*math.tau/count
        vertices, faces = [], []
        radial_steps, chord_steps = 12, 18
        for side in [-1,1]:
            for r in range(radial_steps+1):
                eta = r/radial_steps
                local_chord = chord+(tip_chord-chord)*eta
                lead = root_start+sweep*eta
                for c in range(chord_steps+1):
                    xi = c/chord_steps
                    # Hexagonal wedge section: thin leading/trailing edge, full central thickness.
                    half = thickness*.5*min(1.0, xi/.22, (1-xi)/.22)
                    radial = radius*.985+span*eta
                    tangent = side*max(half, .00015)
                    vertices.append((radial*math.cos(angle)-tangent*math.sin(angle),
                                     radial*math.sin(angle)+tangent*math.cos(angle), -(lead+xi*local_chord)))
        width = chord_steps+1
        plane = (radial_steps+1)*width
        for side in range(2):
            for r in range(radial_steps):
                for c in range(chord_steps):
                    a = side*plane+r*width+c
                    quad = (a, a+1, a+width+1, a+width)
                    faces.append(quad if side == 0 else tuple(reversed(quad)))
        for r in range(radial_steps):
            for c in [0,chord_steps]:
                a = r*width+c
                quad = (a,a+width,a+width+plane,a+plane)
                faces.append(quad if c == 0 else tuple(reversed(quad)))
        for c in range(chord_steps):
            for r in [0,radial_steps]:
                a = r*width+c
                quad = (a,a+plane,a+plane+1,a+1)
                faces.append(quad if r == 0 else tuple(reversed(quad)))
        obj = mesh_object(f'Stage {i+1} fin {fin+1}',vertices,faces,i,alloy,'fin')
        # Recalculate outward normals on each closed, tessellated fin.
        bpy.context.view_layer.objects.active = obj
        obj.select_set(True)
        bpy.ops.object.mode_set(mode='EDIT')
        bpy.ops.mesh.select_all(action='SELECT')
        bpy.ops.mesh.normals_make_consistent(inside=False)
        bpy.ops.object.mode_set(mode='OBJECT')
        obj.select_set(False)
    lathe(f'Stage {i+1} motor retainer',[(offset+length-.012,radius*.92),(offset+length+.014,radius*.94),
          (offset+length+.016,radius*.70),(offset+length-.012,radius*.68)],i,alloy,'retainer')
    lathe(f'Stage {i+1} nozzle bell',[(offset+length-.045,radius*.25),(offset+length-.025,radius*.22),
          (offset+length+.012,radius*.40),(offset+length+.03,radius*.45)],i,black,'nozzle')
    for index,p in enumerate(stage.get('components',[])):indexed_component(p,offset,i,index)
    offset += length

# Recalculate closed shells. Caps are corrected alongside the side-wall winding.
for obj in objects:
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)
    bpy.ops.object.mode_set(mode='EDIT')
    bpy.ops.mesh.select_all(action='SELECT')
    bpy.ops.mesh.normals_make_consistent(inside=False)
    bpy.ops.object.mode_set(mode='OBJECT')
    obj.select_set(False)

document = {'schema_version':1,'physics_version':1,'generator':f'Blender {bpy.app.version_string} headless',
            'mission_sha256':hashlib.sha256(mission_path.read_bytes()).hexdigest(),
            'coordinate_system':'Bevy model: +Y nose, X/Z radial; whole-stack nose at origin; meters',
            'stages':[{'length_m':s['length_m'],'diameter_m':s['diameter_m'],'geometry':s.get('geometry'),'components':s.get('components',[]),'component_shell':s.get('materials',{}).get('component_shell',False)} for s in stages],
            'notes':['Evaluated exterior and structural internal geometry; material volumes or measured mass distribution consumed by Rust model physics. Packed recovery, payload and generic motor hardware remain mission inputs. Not CFD certification.',
                     'RASAero fin Location interpreted as distance forward from stage aft end; verify native placement and nesting.',
                     'Coupler seams, retention rings and nozzle contours are illustrative.'], 'parts':[]}
depsgraph = bpy.context.evaluated_depsgraph_get()
for obj in objects:
    evaluated = obj.evaluated_get(depsgraph)
    mesh = evaluated.to_mesh()
    mesh.calc_loop_triangles()
    positions, normals, indices, lookup = [],[],[],{}
    for tri in mesh.loop_triangles:
        for loop_index in tri.loops:
            vertex = mesh.vertices[mesh.loops[loop_index].vertex_index]
            p = evaluated.matrix_world @ vertex.co
            normal = mesh.corner_normals[loop_index].vector
            normal = (evaluated.matrix_world.to_3x3().inverted().transposed() @ normal).normalized()
            # Blender X,Y,Z -> Bevy X,Z,-Y: a right-handed basis.
            p = [round(p.x,7),round(p.z,7),round(-p.y,7)]
            n = [round(normal.x,7),round(normal.z,7),round(-normal.y,7)]
            key = tuple(p+n)
            if key not in lookup:
                lookup[key]=len(positions)
                positions.append(p)
                normals.append(n)
            indices.append(lookup[key])
    document['parts'].append({'name':obj.name,'stage':obj['stage'],'kind':obj['surface_kind'],
                              'component_index':obj.get('component_index'),
                              'base_color':list(obj.data.materials[0].diffuse_color[:3]),
                              'positions':positions,'normals':normals,'indices':indices})
    evaluated.to_mesh_clear()
(out/'rocket.mesh.json').write_text(json.dumps(document,separators=(',',':')),encoding='utf-8')
bpy.ops.wm.save_as_mainfile(filepath=str(out/'rocket.blend'))
# Export the same Blender scene for reuse in tools that accept native glTF.
bpy.ops.export_scene.gltf(filepath=(out/'rocket.glb').as_posix().removeprefix('//?/'),export_format='GLB',export_animations=False,
                          export_cameras=False,export_lights=False,export_extras=True)
receipt = {'blender':bpy.app.version_string,'parts':len(document['parts']),
           'vertices':sum(len(p['positions']) for p in document['parts']),
           'triangles':sum(len(p['indices'])//3 for p in document['parts']),
           'mission':str(mission_path),'outputs':['rocket.blend','rocket.glb','rocket.mesh.json']}
(out/'build-receipt.json').write_text(json.dumps(receipt,indent=2),encoding='utf-8')
print('ROCKET_MODEL_BUILD '+json.dumps(receipt))
