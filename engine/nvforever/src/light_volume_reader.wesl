// The light volume's shared reader (#3032 stage 2i M2; nvforever volumetric_clouds_lightvol.rs
// `LightVolumeReader`, docs/design/graphics-enhanced.md §3.23 "Ground shadows from the light
// volume"): the optical depth toward the light at a world position, from the completed front sets
// of the volumetric clouds' light volume. One module for every reader outside the trace (the
// ground cloud shadows and the light in the air, medium_air_grid.wesl; reflections later); the trace keeps its own
// reader (volumetric_clouds_march.wesl `vc_light_handoff`), whose addressing this one repeats.
//
// The contract (what a reader may assume):
// - `LightVolumeWindow` is filled on the CPU from `LightVolumeReader` for this frame; a level with
//   presence 0 is never sampled, so its texture may be any stand-in of the right type.
// - A level's set is a COMPLETED bake: every layer was written by command buffers of an earlier
//   frame, and none of its texels is written during this frame, whatever order the views draw in.
//   Its snapshot (the sun, the weather's inputs) is older than the frame: at the default spreads
//   (8 and 32 frames a sweep) about 2 x (spread + 1) frames at most for the near level and about
//   2 x (far spread + near spread + 1) for the far one (a finished far set waits for a near sweep
//   to start before it is promoted), one frame more for the reader than for the trace.
// - The one exception to "measured this frame": on a frame the light volume made no plan while its
//   state is held (its view's image not ready, say), the reader keeps its last levels for up to 4
//   frames (`LVR_KEEP_FRAMES`) without rerunning the drop checks, so for those frames the window's
//   camera, drift and presence are the last planned frame's, not this one's. Safe for the
//   textures (nothing is swept on such a frame, so no texel of the sets is written); a reader
//   that needs this frame's coordinates or a hard bound on a front's age must allow for it.
// - Positions are world positions (Bevy axes, game units). The levels are addressed in the
//   cloud field's coordinates under this frame's drift (the window vectors are measured from
//   `cam` this frame), so a stored depth moves with the wind every frame: no reprojection is
//   needed for the drift. What ages is only the sun's direction, a weather fade's progress and the
//   bend's tilt between the snapshot and now.
// - The depth is toward the light the bake was made for (within 3 degrees of today's, the
//   scheduler's drop rule); the projection to the floor uses the caller's `L`.
// - "No data" is `covered == 0`: outside the far window (or the near one with no far level), no
//   front ready, the light under the horizon for a point below the floor, or every presence 0.
//   Between 0 and 1 the levels cover part of the point (a guard band, a level fading in) and the
//   caller mixes its own fallback over the rest.
//
// Sources [documented: cited publications]: Kim and Neumann 2001, "Opacity Shadow Maps" (opacity
// toward the light accumulated on slices and looked up at a receiver); Bauer 2019, "Creating the
// Atmospheric World of Red Dead Redemption 2", slides 33 and 35 (the clouds' shadow from a
// precomputed product toward the sun). The form is our own; an engineering choice for the enhanced
// look, not the original game's (which casts no cloud shadows).
//
// Mirror: volumetric_clouds_lightvol.rs `lvr_optical_depth`, `lvr_level`, `lvr_rise`.

struct LightVolumeWindow {
    // Per level (near, far): xy the camera `cam`'s position in nodes from the front set's lowest
    // node (field x, z), z the spacing across, w the spacing up (units); the trace's `lv_near`,
    // `lv_far` for that front.
    near: vec4<f32>,
    far: vec4<f32>,
    // x, y: the near and the far level's nodes across; z, w: their layers.
    size: vec4<f32>,
    // xyz: the camera the windows are measured from (world); w: the bend's radius (the clouds'
    // `reach.y`).
    cam: vec4<f32>,
    // x: the slab floor's height (the clouds' `slab.x`, Bevy y; the bent base, layer zero), y: the
    // guard band (cells of each level), z: the near level's presence, w: the far level's (0..1).
    floor: vec4<f32>,
}

// The distance `s >= 0` along a ray whose bent height above the floor is `bh < 0` at its start
// and `bh + b s + a s^2` along it (a >= 0) at which it reaches the floor: the positive root,
// cancellation-resistant; -1 when it never does (the light at or under the horizon).
fn lvr_rise(a: f32, b: f32, bh: f32) -> f32 {
    if a <= 1e-12 {
        if b <= 1e-6 {
            return -1.0;
        }
        return -bh / b;
    }
    let root = sqrt(b * b - 4.0 * a * bh);
    if b >= 0.0 {
        return -2.0 * bh / (b + root);
    }
    return (root - b) / (2.0 * a);
}

// One level at a point already addressed (volumetric_clouds_march.wesl `vc_lv_fetch`): `c` its
// position in nodes from the set's corner, `bh` its bent height above the floor (>= 0), `up` the
// level's spacing up, `size` its nodes across (x) and layers (y). x: how far inside the window the
// point is (cells, -1 outside), y: the depth there (bilinear across, the height interpolated here;
// the second layer is read only when the point is between two layers, never at the floor).
fn lvr_level(t: texture_2d_array<f32>, s: sampler, c: vec2<f32>, bh: f32, up: f32, size: vec2<f32>) -> vec2<f32> {
    let nodes = max(size.x, 2.0);
    let layers = max(size.y, 2.0);
    let edge = min(min(c.x, nodes - 1.0 - c.x), min(c.y, nodes - 1.0 - c.y));
    if bh < 0.0 || edge < 0.0 {
        return vec2<f32>(-1.0, 0.0);
    }
    let y = min(bh / max(up, 1.0), layers - 1.0);
    let y0 = min(floor(y), layers - 2.0);
    let f = y - y0;
    let uv = (c + 0.5) / nodes;
    var d = textureSampleLevel(t, s, uv, i32(y0), 0.0).r;
    if f > 0.0 {
        d = mix(d, textureSampleLevel(t, s, uv, i32(y0) + 1, 0.0).r, f);
    }
    return vec2<f32>(edge, d);
}

// The optical depth toward the light at world position `P`, the light from `L` (unit, toward the
// light): x the depth over the share the levels cover, y that share (0 = no data, module notes). A
// point under the bent floor is moved along `L` to where it meets the floor (no cloud is under the
// floor, so the depth from there is the point's own); a point on or above it is read at its own
// bent height. The near level is mixed over the far one through its guard band, the far one over
// nothing through its own, each times its presence (the trace's nesting, `vc_light_handoff`).
fn lvr_optical_depth(tn: texture_2d_array<f32>, tf: texture_2d_array<f32>, s: sampler, w: LightVolumeWindow, P: vec3<f32>, L: vec3<f32>) -> vec2<f32> {
    let presence = w.floor.zw;
    if max(presence.x, presence.y) <= 0.0 {
        return vec2<f32>(0.0, 0.0);
    }
    let cam = w.cam.xyz;
    let twice_r = 2.0 * max(w.cam.w, 1.0);
    var rel = P - cam;
    var bh = cam.y + rel.y + dot(rel.xz, rel.xz) / twice_r - w.floor.x;
    if bh < 0.0 {
        let a = dot(L.xz, L.xz) / twice_r;
        let b = L.y + 2.0 * dot(rel.xz, L.xz) / twice_r;
        let t = lvr_rise(a, b, bh);
        if t < 0.0 {
            return vec2<f32>(0.0, 0.0);
        }
        rel = rel + L * t;
        bh = 0.0;
    }
    let guard = max(w.floor.y, 1e-3);
    var optical = 0.0;
    var keep = 1.0;
    if presence.x > 0.0 {
        let r = lvr_level(tn, s, w.near.xy + rel.xz / max(w.near.z, 1.0), bh, w.near.w, w.size.xz);
        let wn = clamp(r.x / guard, 0.0, 1.0) * presence.x;
        optical += wn * r.y;
        keep = 1.0 - wn;
    }
    if presence.y > 0.0 && keep > 0.0 {
        let r = lvr_level(tf, s, w.far.xy + rel.xz / max(w.far.z, 1.0), bh, w.far.w, w.size.yw);
        let wf = clamp(r.x / guard, 0.0, 1.0) * presence.y;
        optical += keep * wf * r.y;
        keep *= 1.0 - wf;
    }
    let covered = 1.0 - keep;
    if covered <= 0.0 {
        return vec2<f32>(0.0, 0.0);
    }
    return vec2<f32>(optical / covered, covered);
}
