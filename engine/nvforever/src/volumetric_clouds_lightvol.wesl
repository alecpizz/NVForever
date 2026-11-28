// The light volume's producer (#3032 stage 2g, checkpoint B1; volumetric_clouds_lightvol.rs,
// docs/design/graphics-enhanced.md §3.23): one layer of one level a draw, top down. A texel is a
// node of a world-aligned lattice in field coordinates (its window snapped to whole nodes, the
// bend inverted at the node), and holds the optical depth from the node to the bent domain's top
// along the light: the density integrated from the node to where the straight ray crosses the
// next layer's bent height (sub-steps of at most LV_STEP units), plus the layer above read there
// (bilinear). The near level takes the far level's depth where that point leaves its window,
// mixed through the guard band; beyond the far window the depth above is zero. The top layer is
// cleared to zero by the pass that writes it. B2: `volume` is the level's frozen snapshot (the
// uniform as it stood when this set's sweep began: the camera `lv_cam`, the windows, the drift,
// the light, the weather), so every layer of one sweep is made from the same inputs, whatever
// frame it is drawn on; the near level's far block is the far set it reads, as that camera sees it.
//
// Sources [documented: cited publications]: Kim and Neumann 2001, "Opacity Shadow Maps" (opacity
// toward the light accumulated on slices and interpolated); Bauer 2019, "Creating the Atmospheric
// World of Red Dead Redemption 2", slides 33 and 35 (a cloud sample's light from one lookup
// toward the sun plus a precomputed shadow product). The form is our own (the
// top-down sweep); the density is the march module's own `vc_shape`, uneroded, as the light taps
// read it. An engineering choice for the enhanced look, not the original game's.
//
// Mirror: volumetric_clouds_lightvol.rs `lv_sweep_node`, the tests' `lv_sweep_fill`.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;
import nvforever::volumetric_clouds_march::{CloudVolume, vc_shape, vc_roots, vc_lv_read};

// volumetric_clouds_lightvol.rs `LvSliceUniform`: the same field.
struct LvSlice {
    // x: the level (0 near, 1 far), y: the layer this draw writes, z: the level's footprint
    // (width and placement width, units), w: 1 to take the far level where the step leaves the
    // window (the near level nested in the far one).
    slot: vec4<f32>,
}

@group(0) @binding(0) var<uniform> lv_slice: LvSlice;
@group(0) @binding(1) var<uniform> volume: CloudVolume;
@group(0) @binding(2) var noise_base: texture_3d<f32>;
@group(0) @binding(3) var noise_sampler: sampler;
@group(0) @binding(4) var noise_erosion: texture_3d<f32>;
// The layer above (a one-layer array view of this level) and the outer level (the far level's
// whole array; for a level with no outer one, the layer above again, never read).
@group(0) @binding(5) var lv_above: texture_2d_array<f32>;
@group(0) @binding(6) var lv_outer: texture_2d_array<f32>;

// The longest sub-step along the light between two layers (units; volumetric_clouds_lightvol.rs
// LV_SWEEP_STEP) and the most sub-steps a texel takes (a sun under about 3.6 degrees takes longer
// ones).
const LV_STEP: f32 = 250.0;
const LV_STEPS_MOST: u32 = 64u;

@fragment
fn lv_produce(in: FullscreenVertexOutput) -> @location(0) f32 {
    let v = volume;
    let is_far = lv_slice.slot.x > 0.5;
    var lvl = v.lv_near;
    var nodes = v.lv_size.x;
    if is_far {
        lvl = v.lv_far;
        nodes = v.lv_size.y;
    }
    let cam = v.lv_cam.xyz;
    let ij = floor(in.position.xy);
    let across = max(lvl.z, 1.0);
    let x = (ij.x - lvl.x) * across;
    let z = (ij.y - lvl.y) * across;
    let twice_r = 2.0 * max(v.reach.y, 1.0);
    // The node's height from the camera: the bend inverted at its bent height above the base.
    let rel = vec3<f32>(x, v.slab.x + lv_slice.slot.y * lvl.w - cam.y - (x * x + z * z) / twice_r, z);
    let ld = v.light_dir.xyz;
    // Where the straight ray from the node crosses the next layer's bent height.
    let a = dot(ld.xz, ld.xz) / twice_r;
    let b = ld.y + 2.0 * dot(rel.xz, ld.xz) / twice_r;
    let r = vc_roots(a, b, -lvl.w);
    if r.x < 0.5 || r.z <= 0.0 {
        return 0.0;
    }
    let s = r.z;
    let n = clamp(u32(ceil(s / LV_STEP)), 1u, LV_STEPS_MOST);
    let ds = s / f32(n);
    let fp = lv_slice.slot.z;
    var tau = 0.0;
    for (var i = 0u; i < n; i++) {
        let d = vc_shape(rel + ld * ((f32(i) + 0.5) * ds), cam, fp, fp, noise_base, noise_sampler, noise_erosion, false, v);
        tau += d.x * d.y * ds;
    }
    // The layer above where the step ends (the same lattice: bilinear), zero beyond the window.
    let p_end = rel + ld * s;
    let c = lvl.xy + p_end.xz / across;
    let edge = min(min(c.x, nodes - 1.0 - c.x), min(c.y, nodes - 1.0 - c.y));
    var up = 0.0;
    if edge >= 0.0 {
        up = textureSampleLevel(lv_above, noise_sampler, (c + 0.5) / max(nodes, 2.0), 0, 0.0).r;
    }
    // Nested: the far level through the guard band and beyond the window, when the sweep's
    // snapshot has a far set to read (B2: `lv.z`; none in the first frames).
    if lv_slice.slot.w > 0.5 && v.lv.z > 0.5 {
        let w = clamp(edge / max(v.lv.w, 1e-3), 0.0, 1.0);
        if w < 1.0 {
            let o = vc_lv_read(lv_outer, noise_sampler, p_end, cam, v.lv_far, v.lv_size.yw, v);
            up = w * up + (1.0 - w) * o.y;
        }
    }
    return max(tau + up, 0.0);
}
