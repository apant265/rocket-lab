"""Full-grid WGSL f32 vs CPU f64 verification for the same initial/grid state."""
import csv,json,sys
from pathlib import Path
a=Path(sys.argv[1] if len(sys.argv)>1 else 'output/cfd-cpu-check/flow.csv')
b=Path(sys.argv[2] if len(sys.argv)>2 else 'output/cfd-gpu-check/flow.csv')
cpu=list(csv.DictReader(a.open()));gpu=list(csv.DictReader(b.open()))
assert len(cpu)==len(gpu)
report={}
for key in ['density_kg_m3','pressure_pa','temperature_k','u_mps','v_mps','mach','cp']:
    errors=[]
    for c,g in zip(cpu,gpu):
        assert (c['x_m'],c['y_m'],c['solid'])==(g['x_m'],g['y_m'],g['solid'])
        if c['solid']=='false':errors.append(abs(float(c[key])-float(g[key])))
    report[key]={'max_absolute_error':max(errors),'rmse':(sum(e*e for e in errors)/len(errors))**.5}
report['passed']=report['pressure_pa']['max_absolute_error']<.2 and report['mach']['max_absolute_error']<1e-4
(b.parent/'cpu-reference-check.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf8')
print(json.dumps(report,indent=2));assert report['passed']
