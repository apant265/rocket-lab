"""Pinned OpenCFD v2406 case generation/execution and ASCII field import.
No shell interpolation of paths, and no replacement of failed external results.
"""
import argparse
import json
import math
from pathlib import Path
import re
import subprocess
import uuid

IMAGE = "opencfd/openfoam-default:2406"
HEADER = 'FoamFile { version 2.0; format ascii; class %s; object %s; }\n'

def write(case, name, body, cls="dictionary"):
    p = case / name
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(HEADER % (cls, p.name) + body, encoding="utf-8")

def mesh(case, nx, ny, nz, dx, dy, dz, mask=None, origin=(0, 0, 0), thermal=False):
    """Conformal hexahedra; remove masked cells; outward face orientation."""
    mask = mask or [False] * (nx * ny * nz)
    points = [(origin[0]+i*dx, origin[1]+j*dy, origin[2]+k*dz)
              for k in range(nz+1) for j in range(ny+1) for i in range(nx+1)]
    node = lambda i, j, k: (k*(ny+1)+j)*(nx+1)+i
    cells = [(i,j,k) for k in range(nz) for j in range(ny) for i in range(nx)
             if not mask[(k*ny+j)*nx+i]]
    faces = {}
    for owner, (i,j,k) in enumerate(cells):
        v = [node(i,j,k),node(i+1,j,k),node(i+1,j+1,k),node(i,j+1,k),
             node(i,j,k+1),node(i+1,j,k+1),node(i+1,j+1,k+1),node(i,j+1,k+1)]
        for side, corners in enumerate([(0,4,7,3),(1,2,6,5),(0,1,5,4),(3,7,6,2),(0,3,2,1),(4,5,6,7)]):
            vertices = tuple(v[n] for n in corners)
            key = tuple(sorted(vertices))
            if key in faces:
                faces[key][2] = owner
                continue
            if thermal:
                patch = f"hot{i}" if side==3 and j==ny-1 else "insulated"
                if side>=4: patch="frontBack"
            elif nz>1:
                patch = "root" if side==0 and i==0 else "loaded" if side==5 and k==nz-1 else "free"
            else:
                outside = (side==0 and i==0) or (side==1 and i==nx-1) or (side==2 and j==0) or (side==3 and j==ny-1)
                patch="frontBack" if side>=4 else "farfield" if outside else "wall"
            faces[key] = [vertices, owner, None, patch]
    internal=[f for f in faces.values() if f[2] is not None]
    patches={}
    for f in faces.values():
        if f[2] is None: patches.setdefault(f[3],[]).append(f)
    ordered=internal+[f for group in patches.values() for f in group]
    # Removed solid cells leave interior grid nodes unused. Compact the points
    # before exporting so checkMesh accepts the actual fluid-only mesh.
    used=sorted({v for face in ordered for v in face[0]})
    remap={old:new for new,old in enumerate(used)}
    points=[points[i] for i in used]
    for face in ordered: face[0]=tuple(remap[v] for v in face[0])
    base="constant/polyMesh/"
    text=lambda rows: f"{len(rows)}\n(\n"+"\n".join(rows)+"\n)\n"
    write(case,base+"points",text(["(%g %g %g)" % p for p in points]),"vectorField")
    write(case,base+"faces",text(["4("+" ".join(map(str,f[0]))+")" for f in ordered]),"faceList")
    write(case,base+"owner",text([str(f[1]) for f in ordered]),"labelList")
    write(case,base+"neighbour",text([str(f[2]) for f in internal]),"labelList")
    start=len(internal); boundaries=[]
    for name, group in patches.items():
        kind="empty" if name=="frontBack" else "wall" if name=="wall" or name.startswith("hot") else "patch"
        boundaries.append(f"{name} {{ type {kind}; nFaces {len(group)}; startFace {start}; }}")
        start+=len(group)
    write(case,base+"boundary",text(boundaries),"polyBoundaryMesh")
    return patches,cells

def field(case,name,dimension,value,boundaries,vector=False):
    write(case,"0/"+name,f"dimensions {dimension};\ninternalField uniform {value};\nboundaryField {{\n"+boundaries+"\n}","volVectorField" if vector else "volScalarField")

def control(case,solver,end,dt,stride=1):
    write(case,"system/controlDict",f"application {solver}; startFrom startTime; startTime 0; stopAt endTime; endTime {end:.15g}; deltaT {dt:.15g}; writeControl timeStep; writeInterval {stride}; writeFormat ascii; writePrecision 12; timePrecision 12; purgeWrite 0; runTimeModifiable false; libs (\"libfluidThermophysicalModels.so\" \"libthermoTools.so\");")

def execute(root,cases):
    # Fixed commands and generated alphanumeric names only; root is an argv mount.
    script="source /usr/lib/openfoam/openfoam2406/etc/bashrc || exit; "
    for name,solver in cases:
        if not re.fullmatch(r"[a-z0-9_-]+",name): raise ValueError("Invalid case name")
        if solver == "ccx": script+=f"cd /work/{name} && ccx analysis > log.solver 2>&1 || exit; "
        else: script+=f"cd /work/{name} && checkMesh > log.checkMesh 2>&1 && {solver} > log.solver 2>&1 || exit; "
    solver_image = "rocket-lab-analysis:2406" if any(s == "ccx" for _,s in cases) else IMAGE
    result=subprocess.run(["docker","run","--rm","--network","none","--entrypoint","bash","--mount",f"type=bind,source={root.resolve()},target=/work",solver_image,"-lc",script],capture_output=True,text=True,timeout=3600)
    if result.returncode:
        errors=[]
        for name,_ in cases:
            for log in ["log.checkMesh","log.solver"]:
                p=root/name/log
                if p.exists(): errors.append(f"{name}/{log}:\n"+p.read_text(errors="replace")[-3500:])
        raise RuntimeError("OpenFOAM failed; no preview substitution.\n"+"\n".join(errors)+result.stderr)
    for name,solver in cases:
        if solver != "ccx" and "Mesh OK." not in (root/name/"log.checkMesh").read_text():
            raise RuntimeError(f"OpenFOAM mesh check failed: {root/name}/log.checkMesh")

def values(path,count,vector=False):
    s=re.sub(r"/\*.*?\*/|//[^\n]*","",path.read_text(),flags=re.S)
    uniform=re.search(r"internalField\s+uniform\s+([^;]+);",s)
    if uniform:
        v=[float(x) for x in uniform[1].strip("() \n").split()] if vector else float(uniform[1])
        return [v]*count
    m=re.search(r"internalField\s+nonuniform\s+List<\w+>\s+(\d+)\s*\((.*?)\)\s*;",s,re.S)
    if not m or int(m[1])!=count: raise ValueError(f"Invalid field count: {path}")
    rows=re.findall(r"\(([^()]*)\)",m[2]) if vector else m[2].split()
    out=[[float(x) for x in row.split()] for row in rows] if vector else [float(x) for x in rows]
    if len(out)!=count or any(not math.isfinite(x) for row in out for x in (row if vector else [row])): raise ValueError(f"Invalid field: {path}")
    return out

def times(case):
    return sorted((p for p in case.iterdir() if p.is_dir() and re.fullmatch(r"\d+(?:\.\d+)?(?:e[-+]?\d+)?",p.name)),key=lambda p:float(p.name))

def flow(root,p):
    case=root/"flow"
    patches,cells=mesh(case,p["nx"],p["ny"],1,p["dx"],p["dy"],p["dy"],p["solid"],(p["x0"],p["y0"],-p["dy"]/2))
    rho,ru,rv,energy=p["far"];u,v=ru/rho,rv/rho;pressure=0.4*(energy-0.5*rho*(u*u+v*v));temperature=pressure/(rho*287.05)
    for name,dim,value,vector in [("U","[0 1 -1 0 0 0 0]",f"({u} {v} 0)",True),("T","[0 0 0 1 0 0 0]",temperature,False),("p","[1 -1 -2 0 0 0 0]",pressure,False)]:
        wall="type fixedValue; value uniform (0 0 0);" if name=="U" else "type zeroGradient;"
        bc=f"farfield {{ type freestream; freestreamValue uniform {value}; }}\nwall {{ {wall} }}\nfrontBack {{ type empty; }}"
        field(case,name,dim,value,bc,vector)
    write(case,"constant/thermophysicalProperties","thermoType {type hePsiThermo; mixture pureMixture; transport sutherland; thermo hConst; equationOfState perfectGas; specie specie; energy sensibleInternalEnergy;} mixture {specie {molWeight 28.966;} thermodynamics {Cp 1004.675; Hf 0;} transport {As 1.458e-6; Ts 110.4;}}")
    write(case,"constant/turbulenceProperties","simulationType laminar;")
    write(case,"system/fvSchemes","fluxScheme Kurganov; ddtSchemes {default Euler;} gradSchemes {default Gauss linear;} divSchemes {default none; div(tauMC) Gauss linear;} laplacianSchemes {default Gauss linear corrected;} interpolationSchemes {default linear; reconstruct(rho) vanLeer; reconstruct(U) vanLeerV; reconstruct(T) vanLeer;} snGradSchemes {default corrected;}")
    write(case,"system/fvSolution",'solvers { "(rho|rhoU|rhoE)" {solver diagonal;} U {solver smoothSolver; smoother GaussSeidel; tolerance 1e-9; relTol 0.01;} "(h|e)" {$U; tolerance 1e-10; relTol 0;} }')
    control(case,"rhoCentralFoam",p["steps"]*p["dt"],p["dt"],p["steps"])
    execute(root,[("flow","rhoCentralFoam")])
    last=times(case)[-1];n=len(cells)
    if float(last.name) < p["steps"]*p["dt"]*(1-1e-8): raise RuntimeError("OpenFOAM did not write the requested end time")
    pressures=values(last/"p",n);velocities=values(last/"U",n,True);temperatures=values(last/"T",n)
    densities=values(last/"rho",n) if (last/"rho").exists() else [a/(287.05*t) for a,t in zip(pressures,temperatures)]
    out=[p["far"][:] for _ in p["solid"]]
    for (i,j,_),r,(a,b,_),pr in zip(cells,densities,velocities,pressures):
        if min(r,pr)<=0: raise RuntimeError("Nonphysical OpenFOAM flow field")
        out[j*p["nx"]+i]=[r,r*a,r*b,pr/0.4+0.5*r*(a*a+b*b)]
    return {"cells":out,"backend":"OpenFOAM v2406 rhoCentralFoam / CPU / laminar viscous planar section","flow_time_s":float(last.name),"residuals":[]}

def thermal(root,r):
    cols=r["columns"];end=r["times"][-1];layers=r["layers"]
    if end==0: return {"times":[0.0],"temperatures":[[[r["initial_k"]]*2 for _ in cols]],"backend":"OpenFOAM thermal initial condition"}
    steps=max(1,math.ceil(end/r["step"]));stride=max(1,int(r["write_step"]/r["step"]));steps=math.ceil(steps/stride)*stride;dt=end/steps
    groups=[];previous=None
    for i,c in enumerate(cols):
        mat=c["material"];key=(c["stage"],c["kind"],mat["density_kg_m3"],json.dumps(mat["thermal"],sort_keys=True),c["thickness_m"],round(r["dx"][i],12))
        if key!=previous:groups.append([])
        groups[-1].append(i);previous=key
    cases=[]
    for g,indices in enumerate(groups):
        name=f"thermal{g}";case=root/name;c=cols[indices[0]];mat=c["material"];k=mat["thermal"]["conductivity_w_m_k"];cp=mat["thermal"]["specific_heat_j_kg_k"];rho=mat["density_kg_m3"]
        mesh(case,len(indices),layers,1,r["dx"][indices[0]],c["thickness_m"]/layers,r["dx"][indices[0]],thermal=True)
        table=lambda rows:"table ("+" ".join(f"({t:.15g} {v:.15g})" for t,v in rows)+")"
        bc="frontBack {type empty;} insulated {type zeroGradient;}\n"
        for j,i in enumerate(indices):
            h=[(t,b[i]["h_w_m2_k"]) for t,b in zip(r["times"],r["boundaries"])];ta=[(t,b[i]["recovery_k"]) for t,b in zip(r["times"],r["boundaries"])]
            bc+=f"hot{j} {{type externalWallHeatFluxTemperature; mode coefficient; kappaMethod lookup; kappa k; h {table(h)}; Ta {table(ta)}; value uniform {r['initial_k']};}}\n"
        field(case,"T","[0 0 0 1 0 0 0]",r["initial_k"],bc)
        field(case,"k","[1 1 -3 -1 0 0 0]",k,"frontBack {type empty;} insulated {type zeroGradient;} "+" ".join(f"hot{j} {{type zeroGradient;}}" for j in range(len(indices))))
        write(case,"constant/transportProperties",f"DT {k/(rho*cp)};")
        write(case,"system/fvSchemes","ddtSchemes {default Euler;} gradSchemes {default Gauss linear;} divSchemes {default none;} laplacianSchemes {default Gauss linear corrected;} interpolationSchemes {default linear;} snGradSchemes {default corrected;}")
        write(case,"system/fvSolution","solvers {T {solver PCG; preconditioner DIC; tolerance 1e-9; relTol 0;}} SIMPLE {nNonOrthogonalCorrectors 0;}")
        control(case,"laplacianFoam",end,dt,stride)
        with (case/"system/controlDict").open("a") as f: f.write("functions { conductivity {type readFields; libs (fieldFunctionObjects); fields (k); readOnStart true;} }\n")
        cases.append((name,"laplacianFoam"))
    execute(root,cases)
    folders=times(root/cases[0][0]);temperature_history=[]
    for folder in folders:
        rows=[[r["initial_k"]]*2 for _ in cols]
        for (name,_),indices in zip(cases,groups):
            values_t=values(root/name/folder.name/"T",len(indices)*layers)
            for j,i in enumerate(indices):
                inside=[values_t[y*len(indices)+j] for y in range(layers//2)]
                outside=[values_t[y*len(indices)+j] for y in range(layers//2,layers)]
                rows[i]=[sum(outside)/len(outside),sum(inside)/len(inside)]
        temperature_history.append(rows)
    if abs(float(folders[-1].name)-end)>1e-7: raise RuntimeError("OpenFOAM thermal history does not cover the requested timestamp")
    return {"times":[float(p.name) for p in folders],"temperatures":temperature_history,"backend":f"OpenFOAM v2406 laplacianFoam / CPU / {layers} wall cells; independent material regions"}

def stress(root,r):
    """Quadratic C3D20R fin elements, clamped root, static pressure on top face."""
    cases=[];models=[]
    for index,p in enumerate(r["fins"]):
        case=root/f"stress{index}";case.mkdir();g=p["geometry"];nx=p["divisions"];ny=max(4,nx//4);nz=4
        nodes=[];node_ids={};elements=[];element_x=[]
        def node(i,j,k):
            # Half-grid integer coordinates include quadratic edge midpoints.
            key=(i,j,k)
            if key not in node_ids:
                u=i/(2*nx);chord=g["fin_root_chord_m"]+(g["fin_tip_chord_m"]-g["fin_root_chord_m"])*u
                nodes.append((g["fin_span_m"]*u,g["fin_sweep_m"]*u+chord*j/(2*ny),g["fin_thickness_m"]*k/(2*nz)))
                node_ids[key]=len(nodes)
            return node_ids[key]
        edges=[(0,1),(1,2),(2,3),(3,0),(4,5),(5,6),(6,7),(7,4),(0,4),(1,5),(2,6),(3,7)]
        loaded=[]
        for k in range(nz):
            for j in range(ny):
                for i in range(nx):
                    verts=[(2*i,2*j,2*k),(2*i+2,2*j,2*k),(2*i+2,2*j+2,2*k),(2*i,2*j+2,2*k),
                           (2*i,2*j,2*k+2),(2*i+2,2*j,2*k+2),(2*i+2,2*j+2,2*k+2),(2*i,2*j+2,2*k+2)]
                    ids=[node(*v) for v in verts]+[node(*[(verts[a][d]+verts[b][d])//2 for d in range(3)]) for a,b in edges]
                    elements.append(ids);element_x.append(i)
                    if k==nz-1: loaded.append(len(elements))
        lines=["*HEADING","Rocket Lab quadratic fin / prescribed pressure","*NODE"]
        lines += [f"{i+1},"+",".join(f"{v:.12g}" for v in xyz) for i,xyz in enumerate(nodes)]
        lines += ["*ELEMENT,TYPE=C3D20R,ELSET=ALL"]
        for i,ids in enumerate(elements):
            lines += [f"{i+1},"+",".join(map(str,ids[:15]))+",",",".join(map(str,ids[15:]))]
        lines += ["*NSET,NSET=ALL,GENERATE",f"1,{len(nodes)},1","*NSET,NSET=ROOT"]
        root_ids=[v for (i,j,k),v in node_ids.items() if i==0]
        lines += [",".join(map(str,root_ids[i:i+16])) for i in range(0,len(root_ids),16)]
        lines += ["*MATERIAL,NAME=FIN","*ELASTIC",f"{p['youngs']},{p.get('poisson',r['poisson'])}","*SOLID SECTION,ELSET=ALL,MATERIAL=FIN","*BOUNDARY","ROOT,1,3","*STEP","*STATIC","*DLOAD"]
        lines += [f"{e},P2,{p['pressure']}" for e in loaded]
        lines += ["*NODE PRINT,NSET=ALL","U","*EL PRINT,ELSET=ALL","S","*END STEP"]
        (case/"analysis.inp").write_text("\n".join(lines)+"\n")
        cases.append((case.name,"ccx"));models.append((p,nodes,elements,element_x))
    if not cases: raise ValueError("Stress needs fin modulus and allowable strength.")
    execute(root,cases);rows=[];checks=[]
    for (name,_),(p,nodes,elements,element_x) in zip(cases,models):
        case=root/name;log=(case/"log.solver").read_text()
        if "Job finished" not in log or "*ERROR" in log: raise RuntimeError(f"CalculiX failed: {case}/log.solver")
        dat=(case/"analysis.dat").read_text();u={};sigma={};section=None
        for line in dat.splitlines():
            if "displacements (" in line: section="U";continue
            if "stresses (" in line: section="S";continue
            tokens=line.split()
            if not tokens or not tokens[0].isdigit(): continue
            if section=="U" and len(tokens)==4: u[int(tokens[0])]=[float(v) for v in tokens[1:]]
            elif section=="S" and len(tokens)==8:
                xx,yy,zz,xy,xz,yz=map(float,tokens[2:]);vm=math.sqrt(0.5*((xx-yy)**2+(yy-zz)**2+(zz-xx)**2)+3*(xy*xy+xz*xz+yz*yz))
                e=int(tokens[0]);sigma[e]=max(sigma.get(e,0),vm)
        if len(u)!=len(nodes) or len(sigma)!=len(elements): raise RuntimeError("Incomplete CalculiX field output")
        for x in range(p["divisions"]):
            ids=[i for i,station in enumerate(element_x) if station==x]
            vm=max(sigma[i+1] for i in ids);d=max(abs(u[n][2]) for i in ids for n in elements[i])
            rows.append({"stage":p["stage"],"span_fraction":(x+0.5)/p["divisions"],"stress_pa":vm,"temperature_k":p["temperature_k"],"deflection_m":d,"margin":p["allowable"]/vm if vm>1e-9 and p.get('allowable') is not None else None})
        checks.append({"stage":p["stage"],"nodes":len(nodes),"quadratic_elements":len(elements),"integration_points":8*len(elements),"solver":"CalculiX direct static","completed":True})
    return {"rows":rows,"checks":checks,"backend":"CalculiX / CPU / quadratic 3D elastic fins; prescribed pressure, temperature-adjusted modulus"}

def vehicle_stress(root,r):
    """Bonded, revolved S8 envelope; independent material per axial section.

    Prescribed inertia/aerodynamic resultants, aft clamp. This is static elastic
    shell FEA, not CAD contact, buckling, laminate failure or dynamic response.
    """
    shells=r["shells"];nc=r["circumference"]
    if not shells: raise ValueError("No retained structural shell sections")
    case=root/"vehicle-stress";case.mkdir();nodes=[];node_ids={};elements=[];regions=[]
    # Common boundary nodes bond successive sections. A diameter discontinuity
    # is idealized at its shared ring; exported decks retain the exact mesh.
    z=[shells[0]["start"]]+[p["end"] for p in shells]
    def midsurface(p,key):
        if r.get("radius_reference")=="mid_surface":return p[key]
        # Rocket requests provide outer radius. S8 nodes belong on the wall's
        # midsurface, not its exterior. Retain a small idealized blunt tip ring.
        return max(p[key]-.5*p["thickness"],.55*p["thickness"])
    radius=[midsurface(shells[0],"r0")]+[(midsurface(p,"r1")+midsurface(shells[i+1],"r0"))/2 if i+1<len(shells) else midsurface(p,"r1") for i,p in enumerate(shells)]
    for i,p in enumerate(shells):
        if i and abs(p["start"]-z[i])>1e-6: raise ValueError("Structural envelope has a gap; adjust part offsets")
        if p["youngs"]<=0 or p["thickness"]<=0: raise ValueError("Invalid structural material or thickness")
    def node(axial,angle):
        key=(axial,angle%(2*nc))
        if key not in node_ids:
            i=axial//2
            zi=z[i] if axial%2==0 else (z[i]+z[i+1])/2
            ri=radius[i] if axial%2==0 else (radius[i]+radius[i+1])/2
            phi=key[1]*math.pi/nc
            nodes.append((ri*math.cos(phi),ri*math.sin(phi),zi));node_ids[key]=len(nodes)
        return node_ids[key]
    for i in range(len(shells)):
        for j in range(nc):
            a=2*i;b=2*j
            elements.append([node(a,b),node(a,b+2),node(a+2,b+2),node(a+2,b),node(a,b+1),node(a+1,b+2),node(a+2,b+1),node(a+1,b)])
            regions.append((i,(j+.5)*2*math.pi/nc))
    lines=["*HEADING","Retained vehicle / bonded S8 material sections / SI","*NODE"]
    lines += [f"{i+1},"+",".join(f"{v:.12g}" for v in xyz) for i,xyz in enumerate(nodes)]
    lines += ["*ELEMENT,TYPE=S8,ELSET=ALL"]+[f"{i+1},"+",".join(map(str,e)) for i,e in enumerate(elements)]
    for i,p in enumerate(shells):
        lines += [f"*ELSET,ELSET=REGION{i},GENERATE",f"{i*nc+1},{(i+1)*nc},1",f"*MATERIAL,NAME=MAT{i}","*ELASTIC",f"{p['youngs']},{p['poisson']}",f"*SHELL SECTION,ELSET=REGION{i},MATERIAL=MAT{i}",str(p["thickness"])]
    lines += ["*NSET,NSET=ALL,GENERATE",f"1,{len(nodes)},1","*NSET,NSET=ROOT"]
    root_ids=[v for (i,j),v in node_ids.items() if i==2*len(shells)]
    lines += [",".join(map(str,root_ids[i:i+16])) for i in range(0,len(root_ids),16)]
    lines += ["*BOUNDARY","ROOT,1,6","*STEP","*STATIC","*CLOAD"]
    # Loads are divided equally around the nearest existing circumferential row.
    for load in r["loads"]:
        row=min(range(2*len(shells)+1),key=lambda a:abs((z[a//2] if a%2==0 else (z[a//2]+z[a//2+1])/2)-load["station_m"]))
        ids=[v for (a,j),v in node_ids.items() if a==row]
        for dof,force in enumerate(load["force_n"],1):
            if force:
                lines += [f"{n},{dof},{force/len(ids):.12g}" for n in ids]
    if any(p["pressure"] for p in shells):
        lines += ["*DLOAD"]+[f"REGION{i},P,{p['pressure']}" for i,p in enumerate(shells) if p["pressure"]]
    lines += ["*NODE PRINT,NSET=ALL","U","*EL PRINT,ELSET=ALL,GLOBAL=YES","S","*END STEP"]
    (case/"analysis.inp").write_text("\n".join(lines)+"\n")
    execute(root,[(case.name,"ccx")])
    log=(case/"log.solver").read_text()
    if "Job finished" not in log or "*ERROR" in log: raise RuntimeError(f"CalculiX failed: {case}/log.solver")
    u={};sigma={};section=None
    for line in (case/"analysis.dat").read_text().splitlines():
        if "displacements (" in line: section="U";continue
        if "stresses (" in line: section="S";continue
        t=line.split()
        if not t or not t[0].isdigit(): continue
        if section=="U" and len(t)==4: u[int(t[0])]=[float(v) for v in t[1:]]
        elif section=="S" and len(t)>=8:
            tensor=list(map(float,t[2:8]));xx,yy,zz,xy,xz,yz=tensor
            vm=math.sqrt(.5*((xx-yy)**2+(yy-zz)**2+(zz-xx)**2)+3*(xy*xy+xz*xz+yz*yz))
            e=int(t[0])
            if e not in sigma or vm>sigma[e][0]: sigma[e]=(vm,tensor)
    if len(sigma)!=len(elements) or any(n not in u for e in elements for n in e): raise RuntimeError("Incomplete CalculiX vehicle field")
    rows=[]
    for i,p in enumerate(shells):
        e=max(range(i*nc+1,(i+1)*nc+1),key=lambda e:sigma[e][0]);vm,tensor=sigma[e]
        xx,yy,zz,xy,xz,yz=tensor;phi=regions[e-1][1];tx=-math.sin(phi);ty=math.cos(phi)
        hoop=xx*tx*tx+yy*ty*ty+2*xy*tx*ty;shear=xz*tx+yz*ty
        d=max(math.sqrt(sum(v*v for v in u[n])) for ids in elements[i*nc:(i+1)*nc] for n in ids)
        rows.append({"column":p["column"],"stress_pa":vm,"deflection_m":d,"axial_pa":zz,"hoop_pa":hoop,"shear_pa":shear,"peak_tensor_pa":tensor})
    return {"rows":rows,"checks":{"nodes":len(nodes),"quadratic_shell_elements":len(elements),"material_sections":len(shells),"completed":True},"backend":"CalculiX / CPU / S8 vehicle shell with section-specific materials; aft clamp, prescribed inertia and aero loads"}

def main():
    parser=argparse.ArgumentParser();parser.add_argument("--request",required=True);args=parser.parse_args()
    request=Path(args.request).resolve();r=json.loads(request.read_text());root=request.parent/"openfoam"/("run-"+uuid.uuid4().hex[:12]);root.mkdir(parents=True)
    if r["mode"]=="flow":result=flow(root,r["problem"])
    elif r["mode"]=="thermal":result=thermal(root,r["problem"])
    elif r["mode"]=="vehicle-stress":result=vehicle_stress(root,r["problem"])
    elif r["mode"]=="stress":result=stress(root,r["problem"])
    else:raise ValueError("Unknown analysis mode")
    result["case_directory"]=str(root)
    (request.parent/"openfoam-result.json").write_text(json.dumps(result,allow_nan=False),encoding="utf-8")

if __name__=="__main__":main()
