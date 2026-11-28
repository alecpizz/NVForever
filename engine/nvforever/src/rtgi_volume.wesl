// The bounce light of a probe volume (nvforever rtgi_volume.rs, docs/design/rtgi-volume.md,
// #3009): what a fragment's ambient light becomes where the view has a volume [design], an
// engineering choice, not the original game's.
//
// Imported by bevy_pbr's `apply_pbr_lighting` (patched in by rtgi_volume/hook.rs `HOOK_EDITS`)
// and by nvf_assets nif_material.wesl, both only with the shader def NVF_RTGI_VOLUME, which
// only NIF surfaces' pipelines take (nif_material.rs `reads_rtgi_volume`) in a view whose mesh
// view bind group holds bindings 47-49 (bevy_pbr mesh_view_bindings.rs
// `NvfRtgiVolumeBindings`).
//
// The texture holds four slabs along z, each of the grid's dimensions with a texel of zeros
// all around: (C0, m), (Cx, m v), (Cy, 0), (Cz, 0), everything times m, which is 1 at a point
// that counts and 0 at one that does not (rtgi-volume.md "The texture"). Four filtered reads,
// all of explicit level, so they need no uniform control flow.

struct NvfRtgiVolume {
    // xyz: the first point that is not of the shell (Bevy's world, game units); w: 1 / spacing.
    origin: vec4<f32>,
    // xyz: the grid's points, without the shell; w: how far a read is moved off the surface.
    dimensions: vec4<f32>,
    // xyz: 1 / the texture's sides; w: a slab's texels along z.
    atlas: vec4<f32>,
    // pay, least, thin (the support taken as all there is), pull.
    fit: vec4<f32>,
    // b, g, share (0 the stock light, 1 all of the new one), 0.
    light: vec4<f32>,
}

@group(0) @binding(47) var<uniform> nvf_rtgi: NvfRtgiVolume;
@group(0) @binding(48) var nvf_rtgi_texture: texture_3d<f32>;
@group(0) @binding(49) var nvf_rtgi_sampler: sampler;

// The triangle's normal, of either side and any length (`nvf_rtgi_face`).
var<private> nvf_rtgi_face_normal: vec3<f32>;

// Keeps the triangle's normal from the world position's derivatives: called by the fragment
// shader before anything is discarded.
fn nvf_rtgi_face(p: vec3<f32>) {
    nvf_rtgi_face_normal = cross(dpdx(p), dpdy(p));
}

struct NvfRtgiRead {
    // The irradiance, clamped after the filter.
    e: vec3<f32>,
    // The light around.
    v: f32,
}

// The read at `p` with the geometric normal `g`, unit and on the side the camera sees, and
// the shading normal `n`. Outside the shell all is zero.
fn nvf_rtgi_read(p: vec3<f32>, g: vec3<f32>, n: vec3<f32>) -> NvfRtgiRead {
    let q = (p + nvf_rtgi.dimensions.w * g - nvf_rtgi.origin.xyz) * nvf_rtgi.origin.w;
    if !(all(q > vec3(-1.0)) && all(q < nvf_rtgi.dimensions.xyz)) {
        return NvfRtgiRead(vec3(0.0), 0.0);
    }
    let t = q + 1.5;
    let slab = vec3(0.0, 0.0, nvf_rtgi.atlas.w);
    let c0 = textureSampleLevel(nvf_rtgi_texture, nvf_rtgi_sampler, t * nvf_rtgi.atlas.xyz, 0.0);
    let cx = textureSampleLevel(nvf_rtgi_texture, nvf_rtgi_sampler, (t + slab) * nvf_rtgi.atlas.xyz, 0.0);
    let cy = textureSampleLevel(nvf_rtgi_texture, nvf_rtgi_sampler, (t + 2.0 * slab) * nvf_rtgi.atlas.xyz, 0.0);
    let cz = textureSampleLevel(nvf_rtgi_texture, nvf_rtgi_sampler, (t + 3.0 * slab) * nvf_rtgi.atlas.xyz, 0.0);
    let w = 1.0 / max(c0.a, nvf_rtgi.fit.z);
    let e = max(w * (c0.rgb + cx.rgb * n.x + cy.rgb * n.y + cz.rgb * n.z), vec3(0.0));
    return NvfRtgiRead(e, w * cx.a);
}

fn nvf_rtgi_linear(x: vec3<f32>) -> vec3<f32> {
    return pow(max(x, vec3(0.0)), vec3(2.2));
}

fn nvf_rtgi_luminance(x: vec3<f32>) -> f32 {
    return dot(x, vec3(0.2126, 0.7152, 0.0722));
}

// The light the diffuse colour is multiplied by, in the place of S^2.2: `ambient` the ambient
// part A of the display-value sum `sum` S (0 or more), `occlusion` the occlusion that is in A
// (the bounce takes it once, in linear light), the fragment at `p` with the shading normal
// `n`, seen from `to_view`.
fn nvf_rtgi_light(ambient: vec3<f32>, sum: vec3<f32>, occlusion: vec3<f32>, p: vec3<f32>, n: vec3<f32>, to_view: vec3<f32>) -> vec3<f32> {
    // The shading normal where the triangle is too small to have one of its own.
    let face2 = dot(nvf_rtgi_face_normal, nvf_rtgi_face_normal);
    let face = select(n, nvf_rtgi_face_normal * inverseSqrt(max(face2, 1e-30)), face2 > 1e-30);
    let g = select(face, -face, dot(face, to_view) < 0.0);
    let read = nvf_rtgi_read(p, g, n);
    let k = nvf_rtgi.fit.y + (1.0 - nvf_rtgi.fit.y) * min(nvf_rtgi.fit.x * read.v, 1.0);
    let bounce = nvf_rtgi.light.y * read.e * occlusion;
    var light = nvf_rtgi_linear(sum - (1.0 - nvf_rtgi.light.x) * k * ambient) + bounce;
    let was = nvf_rtgi_linear(sum);
    let before = nvf_rtgi_luminance(was);
    if before > 0.0 {
        light += nvf_rtgi.fit.w * (nvf_rtgi_luminance(light) * was / before - light);
    }
    // The whole change of it, the ambient's and the colour's too. At 0 it is the light that
    // was, at 1 all of the new one, both exactly.
    if nvf_rtgi.light.z < 1.0 {
        light = was + nvf_rtgi.light.z * (light - was);
    }
    return light;
}
