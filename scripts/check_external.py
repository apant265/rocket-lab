"""Reproducible external-solver checks against analytical fields (stdlib only)."""
import argparse
import json
import math
from pathlib import Path
import subprocess
import sys

def run(root,mode,problem):
    case=root/mode;case.mkdir(parents=True,exist_ok=True)
    request=case/'openfoam-request.json';request.write_text(json.dumps({'mode':mode,'problem':problem}))
    subprocess.run([sys.executable,str(Path(__file__).with_name('openfoam_analysis.py')),'--request',str(request)],check=True)
    return json.loads((case/'openfoam-result.json').read_text())

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--out',default='output/external-validation');args=parser.parse_args();root=Path(args.out)
    rho=1.0;pressure=86115.0;u=2*math.sqrt(1.4*pressure/rho)
    far=[rho,rho*u,0,pressure/.4+.5*rho*u*u]
    f=run(root,'flow',{'nx':24,'ny':16,'dx':.05,'dy':.05,'x0':-.4,'y0':-.4,'solid':[False]*384,'far':far,'steps':50,'dt':1e-5})
    pressure_error=max(abs(.4*(c[3]-.5*(c[1]**2+c[2]**2)/c[0])/pressure-1) for c in f['cells'])
    velocity_error=max(abs(c[1]/c[0]/u-1) for c in f['cells'])
    conductivity=.35;density=1800;cp=900;thickness=.004;h=200;hot=400;initial=288.15;end=5
    c={'stage':0,'kind':'body','material':{'density_kg_m3':density,'thermal':{'conductivity_w_m_k':conductivity,'specific_heat_j_kg_k':cp}},'thickness_m':thickness}
    t=run(root,'thermal',{'columns':[c],'boundaries':[[{'h_w_m2_k':h,'recovery_k':hot}]]*2,'times':[0,end],'initial_k':initial,'step':.005,'write_step':1,'layers':16,'dx':[.02]})
    # Convective slab, insulated x=0, h at x=L: mu*tan(mu)=Bi.
    bi=h*thickness/conductivity;fo=conductivity/(density*cp)*end/thickness**2
    theta=[0.0,0.0]
    for n in range(100):
        a=n*math.pi+1e-10;b=n*math.pi+math.pi/2-1e-10
        for _ in range(60):
            m=(a+b)/2
            if m*math.tan(m)>bi:b=m
            else:a=m
        m=(a+b)/2;coefficient=4*math.sin(m)/(2*m+math.sin(2*m))*math.exp(-m*m*fo)
        theta[0]+=coefficient*(math.sin(m)-math.sin(m/2))/(m/2)
        theta[1]+=coefficient*math.sin(m/2)/(m/2)
    expected=[hot+(initial-hot)*v for v in theta];actual=t['temperatures'][-1][0];thermal_error=max(abs(a-b) for a,b in zip(actual,expected))
    geometry={'fin_root_chord_m':.2,'fin_tip_chord_m':.2,'fin_sweep_m':0,'fin_span_m':.08,'fin_thickness_m':.004}
    s=run(root,'stress',{'poisson':0.0,'fins':[{'stage':0,'geometry':geometry,'divisions':32,'pressure':1000,'temperature_k':288.15,'youngs':10e9,'allowable':100e6}]})
    # Compare stress at its actual outermost Gauss point, rather than a surface
    # stress that the integration-point output never claims to sample.
    gauss=(1-1/math.sqrt(3))/2;x=gauss*.08/32;z=.004-gauss*.004/4
    stress_expected=3*1000*(.08-x)**2/.004**2*(2*z/.004-1)
    stress_error=abs(max(v['stress_pa'] for v in s['rows'])/stress_expected-1)
    tip_expected=1.5*1000*.08**4/10e9/.004**3;tip_actual=s['rows'][-1]['deflection_m'];tip_error=abs(tip_actual/tip_expected-1)
    report={'flow':{'pressure_relative_error':pressure_error,'velocity_relative_error':velocity_error,'backend':f['backend']},'thermal':{'outer_inner_expected_k':expected,'outer_inner_actual_k':actual,'max_error_k':thermal_error,'backend':t['backend']},'stress':{'integration_point_expected_pa':stress_expected,'tip_expected_m':tip_expected,'tip_actual_m':tip_actual,'stress_relative_error':stress_error,'tip_relative_error':tip_error,'backend':s['backend']},'passed':pressure_error<1e-5 and velocity_error<1e-5 and thermal_error<.5 and stress_error<.02 and tip_error<.02}
    (root/'external-reference-check.json').write_text(json.dumps(report,indent=2));print(json.dumps(report,indent=2))
    if not report['passed']:raise SystemExit('External analytical check failed')

if __name__=='__main__':main()
