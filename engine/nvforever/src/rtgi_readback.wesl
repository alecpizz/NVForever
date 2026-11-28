// The read of a bounce light volume on the GPU, once, for the developer aid that compares it with
// the CPU's (nvforever rtgi_volume/made.rs, #3009) [design]. Group 0 has the view's own uniform,
// texture and sampler at the bindings rtgi_volume.wesl declares (47-49).

import nvforever::rtgi_volume::nvf_rtgi_read;

struct Asked {
    p: vec4<f32>,
    g: vec4<f32>,
    n: vec4<f32>,
}

@group(1) @binding(0) var<storage, read> asked: array<Asked>;
// One for each: E and v.
@group(1) @binding(1) var<storage, read_write> answers: array<vec4<f32>>;

@compute @workgroup_size(32)
fn answer(@builtin(global_invocation_id) id: vec3<u32>) {
    let a = asked[id.x];
    let found = nvf_rtgi_read(a.p.xyz, a.g.xyz, a.n.xyz);
    answers[id.x] = vec4(found.e, found.v);
}
