struct Settings {nx:u32,ny:u32,pad0:u32,pad1:u32,dx:f32,dy:f32,dt:f32,pad2:f32,far:vec4<f32>}
@group(0) @binding(0) var<storage,read> old:array<vec4<f32>>;
@group(0) @binding(1) var<storage,read_write> next:array<vec4<f32>>;
@group(0) @binding(2) var<storage,read> solid:array<u32>;
@group(0) @binding(3) var<uniform> settings:Settings;
fn pressure(c:vec4<f32>)->f32{return 0.4*(c.w-0.5*dot(c.yz,c.yz)/c.x);}
fn flux(c:vec4<f32>,axis:u32)->vec4<f32>{let p=pressure(c);let v=c[axis+1u]/c.x;var f=vec4<f32>(c.x*v,c.y*v,c.z*v,(c.w+p)*v);f[axis+1u]+=p;return f;}
fn wave(c:vec4<f32>,axis:u32)->f32{return abs(c[axis+1u]/c.x)+sqrt(1.4*pressure(c)/c.x);}
fn rusanov(a:vec4<f32>,b:vec4<f32>,axis:u32)->vec4<f32>{return 0.5*(flux(a,axis)+flux(b,axis))-0.5*max(wave(a,axis),wave(b,axis))*(b-a);}
fn neighbor(x:i32,y:i32,c:vec4<f32>,axis:u32)->vec4<f32>{
    if x<0||y<0||x>=i32(settings.nx)||y>=i32(settings.ny){return settings.far;}
    let k=u32(y)*settings.nx+u32(x);if solid[k]!=0u{var ghost=c;ghost[axis+1u]=-ghost[axis+1u];return ghost;}return old[k];
}
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id:vec3<u32>){
    let k=id.x;if k>=settings.nx*settings.ny{return;}let c=old[k];
    if solid[k]!=0u{next[k]=c;return;}
    let x=i32(k%settings.nx);let y=i32(k/settings.nx);
    let right=rusanov(c,neighbor(x+1,y,c,0u),0u);let left=rusanov(neighbor(x-1,y,c,0u),c,0u);
    let top=rusanov(c,neighbor(x,y+1,c,1u),1u);let bottom=rusanov(neighbor(x,y-1,c,1u),c,1u);
    next[k]=c-settings.dt/settings.dx*(right-left)-settings.dt/settings.dy*(top-bottom);
}
