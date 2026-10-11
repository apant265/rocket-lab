"""Rebuild scientific figures from exported solver data, not illustrative art."""
from pathlib import Path
import csv, json
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
root=Path(__file__).resolve().parents[1]
fig,axes=plt.subplots(2,2,figsize=(12,7),constrained_layout=True)
for ax,name in zip(axes.flat,['subsonic','transonic','supersonic','hypersonic']):
    directory=root/'output/cfd'/name
    report=json.loads((directory/'compute.json').read_text())
    rows=list(csv.DictReader((directory/'flow.csv').open()))
    nx,ny=report['nx'],report['ny']
    x=np.array([float(r['x_m']) for r in rows]).reshape(ny,nx)
    y=np.array([float(r['y_m']) for r in rows]).reshape(ny,nx)
    solid=np.array([r['solid']=='true' for r in rows]).reshape(ny,nx)
    cp=np.ma.array([float(r['cp']) for r in rows]).reshape(ny,nx)
    cp.mask=solid
    u=np.array([float(r['u_mps']) for r in rows]).reshape(ny,nx)
    v=np.array([float(r['v_mps']) for r in rows]).reshape(ny,nx)
    cmap=plt.colormaps['coolwarm'].copy();cmap.set_bad('#eeeeee')
    im=ax.pcolormesh(x,y,cp,cmap=cmap,vmin=-.3,vmax=1.5,shading='nearest')
    step=(slice(None,None,5),slice(None,None,5))
    magnitude=np.maximum(np.hypot(u,v),1e-10)
    ax.quiver(x[step],y[step],np.ma.array(u/magnitude,mask=solid)[step],np.ma.array(v/magnitude,mask=solid)[step],color='#444444',width=.0017,scale=38)
    ax.set_title(f'{name.title()} · Mach {report["mach"]:g} · density residual {report["residuals"][-1][1]:.1e}')
    ax.set_xlabel('Axial station (m)');ax.set_ylabel('Section coordinate (m)');ax.set_aspect('equal')
fig.colorbar(im,ax=axes.ravel().tolist(),label='Pressure coefficient Cp',shrink=.75)
fig.suptitle('Computed flow on RTX 3060 Ti · planar perfect-gas Euler body section\nFirst-order slip-wall grid; excludes viscosity, fins, heating, chemistry and rarefaction',fontsize=13)
fig.savefig(root/'output/cfd/regime-pressure.png',dpi=180);plt.close(fig)

directory=root/'output/validation'
if (directory/'telemetry.csv').exists():
    sim=list(csv.DictReader((directory/'telemetry.csv').open()))
    ref=list(csv.DictReader((root/'sources/Plumbbob_MKIII_R75 PDR Flight Data.CSV').open()))
    fig,axes=plt.subplots(3,1,figsize=(10,9),sharex=True,constrained_layout=True)
    configs=[('altitude_agl_m','Altitude (ft)',.3048,'Altitude above pad (m)'),('speed_mps','Velocity (ft/sec)',.3048,'Speed (m/s)'),('mach','Mach Number',1.,'Mach')]
    for ax,(sf,rf,scale,label) in zip(axes,configs):
        ax.plot([float(s['time_s']) for s in sim],[float(s[sf]) for s in sim],label='Rust provisional mission',lw=1.4)
        reference_values=[float(s[rf])*scale for s in ref]
        if sf == 'speed_mps':
            reference_values=np.abs(reference_values)
        ax.plot([float(s['Time (sec)']) for s in ref],reference_values,label='Supplied RASAero export',lw=1.1)
        ax.set_ylabel(label);ax.grid(alpha=.25)
    axes[0].legend();axes[-1].set_xlabel('Time (s)')
    axes[-1].set_xlim(0,min(float(sim[-1]['time_s']),float(ref[-1]['Time (sec)'])))
    fig.suptitle('Independent comparison · reference acceptance FAILED\nVehicle/run identity remains unresolved; no coefficient fitting',fontsize=13)
    fig.savefig(directory/'reference-comparison.png',dpi=180);plt.close(fig)
print('Exported solver figures')
