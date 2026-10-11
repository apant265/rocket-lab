// Same two-cell finite-volume equations as thermal::advance (CPU f64).
// One cooperative workgroup; lanes process multiple columns with a synchronized
// double buffer. 1024 columns use exactly 16 KiB of workgroup storage.
struct Settings {count:u32,start:u32,end:u32,total:u32,initial:f32,step:f32,padding:vec2<f32>}
@group(0) @binding(0) var<storage,read> columns:array<vec4<f32>>;
@group(0) @binding(1) var<storage,read> links:array<vec2<f32>>;
@group(0) @binding(2) var<storage,read> boundaries:array<vec4<f32>>;
@group(0) @binding(3) var<storage,read_write> state:array<vec2<f32>>;
@group(0) @binding(4) var<storage,read_write> history:array<vec2<f32>>;
@group(0) @binding(5) var<uniform> settings:Settings;
var<workgroup> temperatures:array<vec2<f32>,1024>;
var<workgroup> next_temperatures:array<vec2<f32>,1024>;
@compute @workgroup_size(256)
fn main(@builtin(local_invocation_index) lane:u32) {
    for(var i=lane;i<settings.count;i+=256u) {temperatures[i]=state[i];}
    workgroupBarrier();
    for(var frame=settings.start;frame<settings.end;frame+=1u) {
        let interval=boundaries[frame*settings.count*2u].x;
        let steps=u32(max(1.0,ceil(interval/settings.step)));
        let dt=max(1e-10,interval/f32(steps));
        for(var sub=0u;sub<steps;sub+=1u) {
            for(var i=lane;i<settings.count;i+=256u) {
                var result=vec2<f32>(0.0);
                let c=columns[i*2u];let m=columns[i*2u+1u];let b=boundaries[(frame*settings.count+i)*2u];let attached_stage=boundaries[(frame*settings.count+i)*2u+1u].x;
                let t=temperatures[i];var exchange=vec2<f32>(0.0);
                for(var j=0u;j<u32(m.z);j+=1u) {let link=links[u32(m.y)+j];let other=columns[u32(link.x)*2u+1u].w;if m.w==other || (m.w>=attached_stage && other>=attached_stage) {exchange+=link.y*(temperatures[u32(link.x)]-t);}}
                let cap=c.x/dt;let g=c.y;let conv=b.w*c.z;
                let rad=m.x*c.w*5.670374419e-8*c.z*(t.x+b.y)*(t.x*t.x+b.y*b.y);
                let boundary=conv+rad;
                let a=cap+g+boundary;let d=cap+g;
                let x=cap*t.x+exchange.x+conv*b.z+rad*b.y;let y=cap*t.y+exchange.y+boundaries[(frame*settings.count+i)*2u+1u].y;
                let determinant=cap*(cap+2.0*g+boundary)+g*boundary;
                result=vec2<f32>((x*d+g*y)/determinant,(a*y+g*x)/determinant);
                if interval==0.0 {result=t;}
                next_temperatures[i]=result;
            }
            workgroupBarrier();
            for(var i=lane;i<settings.count;i+=256u) {temperatures[i]=next_temperatures[i];}
            workgroupBarrier();
        }
        for(var i=lane;i<settings.count;i+=256u) {history[frame*settings.count+i]=temperatures[i];}
    }
    for(var i=lane;i<settings.count;i+=256u) {state[i]=temperatures[i];}
}
