// Height-based blending of the folded landscape layers (terrain.wesl with
// TERRAIN_HEIGHT_BLEND; terrain.rs "Height blending"; docs/design/graphics-enhanced.md
// §3.12) [design, #2550]. Plain WGSL on purpose: no imports and no conditions, so the
// engine's tests parse and validate this very file with naga (terrain.rs tests), and
// terrain.rs `height_weights` mirrors it on the CPU.
//
// Seven entries: the base at index 0, folded layer k at k + 1. Every constant is
// NVForever's own starting choice, to be settled from pictures; none is a fact about the
// original game.

// The luminance weights (Rec. 709) applied to linear colour.
const TERRAIN_LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
// Height = gain x (raw decoded texel luminance - reference luminance), clamped
// to +- limit. The caller supplies the start-up gain and global/local reference.
const TERRAIN_HEIGHT_LIMIT: f32 = 0.25;

fn terrain_height(texel: vec3<f32>, reference: vec3<f32>, gain: f32) -> f32 {
    return clamp(gain * (dot(texel, TERRAIN_LUMA) - dot(reference, TERRAIN_LUMA)), -TERRAIN_HEIGHT_LIMIT, TERRAIN_HEIGHT_LIMIT);
}

// The share of the base and of each of the `n` layers in the ordered blend: `a[k]` is
// layer k's painted opacity, each layer laid over
// what came before it, as the `mix` chain of terrain.wesl's ordinary path does.
// w[k + 1] = a[k] x the product of (1 - a[j]) over the later layers j; w[0] the base's
// remainder. They sum to 1.
fn terrain_ordered_shares(a: array<f32, 6>, n: u32) -> array<f32, 7> {
    var paint = a;
    var w = array<f32, 7>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    var remaining = 1.0;
    for (var i = i32(min(n, 6u)) - 1; i >= 0; i -= 1) {
        let s = clamp(paint[i], 0.0, 1.0);
        w[i + 1] = s * remaining;
        remaining *= 1.0 - s;
    }
    w[0] = remaining;
    return w;
}

// Depth competition [design, #2550]: score = share + clamp(height, -share, share).
// Retain scores within depth of the highest, multiply by their shares, then normalise.
// Equal heights still sharpen unequal shares; there is no equal-height gate. Zero
// shares stay zero and a sole contributor stays one. The share cap makes the score
// continuous as a painted layer vanishes. Depth is positive (floored for roundoff).
fn terrain_height_weights(w: array<f32, 7>, h: array<f32, 7>, depth: f32) -> array<f32, 7> {
    var p = w;
    var height = h;
    var total = 0.0;
    for (var i = 0; i < 7; i += 1) {
        p[i] = max(p[i], 0.0);
        total += p[i];
    }
    if total <= 0.0 {
        return array<f32, 7>(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    }
    var score = array<f32, 7>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    var highest = 0.0;
    for (var i = 0; i < 7; i += 1) {
        p[i] = p[i] / total;
        score[i] = p[i] + clamp(height[i], -p[i], p[i]);
        highest = max(highest, score[i]);
    }
    let d = max(depth, 0.001);
    var q = array<f32, 7>(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    var q_total = 0.0;
    for (var i = 0; i < 7; i += 1) {
        q[i] = p[i] * max(0.0, d - (highest - score[i]));
        q_total += q[i];
    }
    for (var i = 0; i < 7; i += 1) {
        q[i] = q[i] / q_total;
    }
    return q;
}
