"""Render the builder catalogue in a fresh, headless Blender process.
blender -b --factory-startup --python scripts/build_part_previews.py
"""
import bpy
import math
import sys
from pathlib import Path
from mathutils import Vector

out = Path(__file__).resolve().parents[1] / 'assets' / 'parts'
out.mkdir(parents=True, exist_ok=True)
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
scene = bpy.context.scene
scene.render.engine = 'CYCLES'
scene.cycles.samples = 24
scene.render.resolution_x = 256
scene.render.resolution_y = 192
scene.render.resolution_percentage = 100
scene.render.film_transparent = True
scene.render.image_settings.file_format = 'PNG'
scene.world.color = (.3, .3, .3)

def mat(name, color, metal=0):
    m = bpy.data.materials.new(name)
    m.diffuse_color = (*color, 1)
    m.use_nodes = True
    bs = m.node_tree.nodes.get('Principled BSDF')
    bs.inputs['Base Color'].default_value = (*color, 1)
    bs.inputs['Metallic'].default_value = metal
    bs.inputs['Roughness'].default_value = .3
    return m

silver = mat('Aluminum', (.53,.65,.73), .7)
white = mat('Fiberglass', (.82,.87,.9))
orange = mat('Recovery orange', (1,.25,.04))
dark = mat('Carbon', (.06,.10,.13), .25)
green = mat('Circuit board', (.02,.36,.18))
objects = []

def mesh(v,f,m):
    data = bpy.data.meshes.new('Part')
    data.from_pydata(v,[],f)
    data.update()
    ob = bpy.data.objects.new('Part',data)
    scene.collection.objects.link(ob)
    ob.data.materials.append(m)
    objects.append(ob)
    return ob

def tube(r,h,wall,m=white,z=0):
    n=64
    v=[(rr*math.cos(i*math.tau/n),rr*math.sin(i*math.tau/n),zz+z)
       for rr,zz in [(r,-h/2),(r,h/2),(max(.001,r-wall),h/2),(max(.001,r-wall),-h/2)] for i in range(n)]
    f=[]
    for k in range(4):
        for i in range(n):
            j=(i+1)%n
            f.append((k*n+i,k*n+j,((k+1)%4)*n+j,((k+1)%4)*n+i))
    return mesh(v,f,m)

def cube(size,pos,m):
    bpy.ops.mesh.primitive_cube_add(size=1,location=pos)
    ob=bpy.context.object
    ob.scale=size
    ob.data.materials.append(m)
    bevel=ob.modifiers.new('Edge detail','BEVEL')
    bevel.width=.04
    bevel.segments=2
    objects.append(ob)

def fins():
    for a in [0,math.pi/2,math.pi,math.pi*1.5]:
        v=[]
        for y in [-.035,.035]:
            for x,z in [(0,.5),(.7,.12),(.7,-.38),(0,-.5)]:
                v.append((x*math.cos(a)-y*math.sin(a),x*math.sin(a)+y*math.cos(a),z))
        mesh(v,[(0,3,2,1),(4,5,6,7),(0,1,5,4),(1,2,6,5),(2,3,7,6),(3,0,4,7)],silver)

def canopy():
    n=96
    v=[]
    for j in range(17):
        theta=.09+(math.pi/2-.09)*j/16
        for i in range(n):
            a=i*math.tau/n
            r=math.sin(theta)*(1+.025*math.cos(16*a))
            v.append((r*math.cos(a),r*math.sin(a),.72*math.cos(theta)+.4))
    ob=mesh(v,[(j*n+i,j*n+(i+1)%n,(j+1)*n+(i+1)%n,(j+1)*n+i) for j in range(16) for i in range(n)],orange)
    ob.data.materials.append(white)
    for p in ob.data.polygons:
        p.material_index=(p.index%n)//6%2
        p.use_smooth=True
    for i in range(16):
        a=i*math.tau/16
        curve=bpy.data.curves.new('Suspension line','CURVE')
        curve.dimensions='3D'
        curve.bevel_depth=.007
        sp=curve.splines.new('POLY'); sp.points.add(1)
        sp.points[0].co=(math.cos(a),math.sin(a),.4,1)
        sp.points[1].co=(0,0,-.8,1)
        ob=bpy.data.objects.new('Line',curve); scene.collection.objects.link(ob)
        ob.data.materials.append(white); objects.append(ob)

bpy.ops.object.camera_add(location=(3,-5,2.5))
camera=bpy.context.object
camera.rotation_euler=(Vector((0,0,0))-camera.location).to_track_quat('-Z','Y').to_euler()
camera.data.type='ORTHO';camera.data.ortho_scale=2.75
scene.camera=camera
for location,power,size in [((2,-3,5),600,4),((-3,-1,2),400,3),((0,3,3),700,3)]:
    bpy.ops.object.light_add(type='AREA',location=location)
    light=bpy.context.object;light.data.energy=power;light.data.shape='DISK';light.data.size=size
    light.rotation_euler=(-light.location).to_track_quat('-Z','Y').to_euler()

start = int(sys.argv[sys.argv.index('--')+1]) if '--' in sys.argv else 0
for index in range(start,23):
    for ob in objects: bpy.data.objects.remove(ob,do_unlink=True)
    objects=[]
    if index==0: tube(.32,1.5,.045)
    elif index==1:
        n=64;v=[]
        for j in range(25):
            t=j/24;r=.4*math.sqrt(max(0,1-(1-t)**2))
            v += [(r*math.cos(i*math.tau/n),r*math.sin(i*math.tau/n),.8-1.6*t) for i in range(n)]
        mesh(v,[(j*n+i,j*n+(i+1)%n,(j+1)*n+(i+1)%n,(j+1)*n+i) for j in range(24) for i in range(n)],silver)
    elif index==2: tube(.15,1,.02,dark);fins()
    elif index==3: tube(.28,1.5,.08,silver);tube(.23,.22,.09,dark,z=-.82);tube(.3,.06,.06,orange,z=.73)
    elif index in [4,14]:
        cube((.58,.18,1.1),(0,0,0),green)
        for z in [-.35,0,.3]: cube((.36,.12,.18),(0,-.16,z),silver)
        tube(.35,1.45,.025,dark)
    elif index==5: canopy()
    elif index==6: tube(.3,1.3,.035,dark);fins();tube(.22,.25,.09,silver,z=-.7)
    elif index==7: tube(.32,1.45,.05,orange)
    elif index==8: tube(.4,.8,.04,dark)
    elif index==9: tube(.62,.11,.61,white)
    elif index==10: tube(.62,.11,.22,white)
    elif index==11: tube(.3,.25,.17,dark);tube(.39,.06,.20,silver,z=.12);tube(.39,.06,.20,silver,z=-.12)
    elif index==12:
        for j in range(7):
            bpy.ops.mesh.primitive_torus_add(major_radius=.5,minor_radius=.035,location=(0,0,j*.08-.24))
            ob=bpy.context.object;ob.data.materials.append(orange);objects.append(ob)
    elif index==13: tube(.4,.65,.39,silver)
    elif index==15:
        n=64;v=[]
        for j in range(25):
            t=j/24;r=.23+.32*t
            v += [(r*math.cos(i*math.tau/n),r*math.sin(i*math.tau/n),.65-1.3*t) for i in range(n)]
        mesh(v,[(j*n+i,j*n+(i+1)%n,(j+1)*n+(i+1)%n,(j+1)*n+i) for j in range(24) for i in range(n)],silver)
    elif index in [16,17]:
        tube(.15,1,.02,dark)
        outline=[(.5*math.sin(j*math.pi/32),.5*math.cos(j*math.pi/32)) for j in range(33)] if index==16 else [(0,.5),(.6,.3),(.4,.05),(.75,-.4),(0,-.5)]
        for angle in [0,math.pi/2,math.pi,3*math.pi/2]:
            v=[((x+.15)*math.cos(angle)-side*math.sin(angle),(x+.15)*math.sin(angle)+side*math.cos(angle),z) for side in [-.035,.035] for x,z in outline]
            n=len(outline);f=[tuple(reversed(range(n))),tuple(range(n,2*n))]+[(i,(i+1)%n,(i+1)%n+n,i+n) for i in range(n)]
            mesh(v,f,silver)
    elif index==18:
        tube(.23,1.2,.03,dark)
        for i in range(6):
            ob=tube(.23,.8,.035,white);a=i*math.tau/6;ob.location=(.46*math.cos(a),.46*math.sin(a),-.15)
    elif index==19: tube(.13,1.25,.035,silver)
    elif index==20: tube(.53,.22,.20,white)
    elif index==21:
        v=[]
        for j in range(49):
            t=j/48;v.extend([(-.24,.10*math.sin(4*math.pi*t),.9-1.8*t),(.24,.10*math.sin(4*math.pi*t),.9-1.8*t)])
        mesh(v,[(2*j,2*j+1,2*j+3,2*j+2) for j in range(48)],orange)
    elif index==22:
        tube(.25,1.5,.03,dark)
        for x in [-.55,.55]:
            ob=tube(.19,1.1,.03,white);ob.location.x=x
    scene.render.filepath=str(out/f'{index:02}.png')
    bpy.ops.render.render(write_still=True)
print('Rendered model thumbnails:',out)
