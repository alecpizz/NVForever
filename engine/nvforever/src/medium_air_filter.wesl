// The volumetric medium's light in the air, the filter pass (medium_air_grid.rs
// `air_filter_pass`; #3032 stage 3): a 3 x 3 filter in x and y within each
// slice of the grid volume, for r (N), g (M) and b (the slice's V), into a second volume the
// composite reads. The grid's visibility samples are dithered per column by interleaved
// gradient noise, which is made to be followed by a 3 x 3 filter [documented: Jimenez 2014,
// "Next Generation Post Processing in Call of Duty: Advanced Warfare", SIGGRAPH 2014
// Advances]; without one the dither shows as a fixed screen-door pattern, amplified by the
// ray contrast. An engineering choice of NVForever's own for the enhanced look, not the
// original game's behaviour. Entry `air_filter`, workgroup 8 x 8 x 1, one invocation per
// texel (the dispatch's z is the slice).
//
// Weights: the tent (1 2 1 / 2 4 2 / 1 2 1) / 16, the product of [1 2 1] / 4 per axis, rather
// than a box [design]: per axis its response (1 + cos w) / 2 is 0 at the grid's Nyquist
// frequency, where a column-to-column alternation lives (a 3 x 3 checkerboard of two values
// becomes their mean exactly), while a box's (1 + 2 cos w) / 3 leaves a third of it, inverted.
// At the grid's edges the weights of the neighbours that exist are renormalised (still the
// product of each axis's renormalised weights). The filter is linear with the same weights in
// every slice, so the filtered N and M are the cumulative sums of the filtered per-slice
// values, M stays monotone in z and N never exceeds M.

@group(0) @binding(0) var source: texture_3d<f32>;
@group(0) @binding(1) var filtered: texture_storage_3d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn air_filter(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(source);
    if id.x >= size.x || id.y >= size.y || id.z >= size.z {
        return;
    }
    let centre = vec2<i32>(id.xy);
    let last = vec2<i32>(size.xy) - vec2<i32>(1, 1);
    let slice = i32(id.z);
    var sum = vec3<f32>(0.0, 0.0, 0.0);
    var weight = 0.0;
    for (var dy = -1; dy <= 1; dy += 1) {
        for (var dx = -1; dx <= 1; dx += 1) {
            let p = centre + vec2<i32>(dx, dy);
            if p.x >= 0 && p.y >= 0 && p.x <= last.x && p.y <= last.y {
                let w = f32((2 - abs(dx)) * (2 - abs(dy)));
                sum += w * textureLoad(source, vec3<i32>(p, slice), 0).rgb;
                weight += w;
            }
        }
    }
    textureStore(filtered, id, vec4<f32>(sum / weight, 1.0));
}
