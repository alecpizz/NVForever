// The volumetric medium's composition helpers (medium.rs; #3032, stage 1), imported by the
// sky proxy's material (volumetric_clouds.wesl) as `nvforever::medium_upsample`. Plain WGSL
// on purpose: no imports, no conditions, no bindings (the caller passes its own texture and
// sampler), so the engine's tests parse and validate this very file with naga. An
// engineering choice for the enhanced look, not the original game's.

// Where the world point `world_pos` lies in the far buffer, 0..1 from the top left: its
// position through the view's unjittered projection (`clip_from_world`, Bevy's
// `view.unjittered_clip_from_world`), Y flipped as Bevy's NDC to UV. The buffer is traced
// with the same unjittered projection (medium_trace.wesl), so a fragment rasterised with a
// jittered projection (TAA, FSR, DLSS) reads the buffer where its own ray lies, whatever the
// jitter's sign [design].
fn medium_screen_uv(clip_from_world: mat4x4<f32>, world_pos: vec3<f32>) -> vec2<f32> {
    let clip = clip_from_world * vec4<f32>(world_pos, 1.0);
    let ndc = clip.xy / max(clip.w, 1e-6);
    return vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
}

// The far buffer at uv, bilinear (the sampler's linear filter, clamped to the edge): the
// stored premultiplied colour and transmittance turned into premultiplied colour and alpha
// (1 - transmittance), ready to blend premultiplied over the sky. Filtering the stored
// values is exact for premultiplied blending: colour and transmittance are both linear in
// the share of each texel.
// A sample whose transmittance rounds to one hides nothing, so it adds no light either: a very
// thin edge's colour would otherwise be added at zero alpha.
fn medium_far(far_tex: texture_2d<f32>, far_samp: sampler, uv: vec2<f32>) -> vec4<f32> {
    let s = textureSampleLevel(far_tex, far_samp, uv, 0.0);
    let alpha = clamp(1.0 - s.a, 0.0, 1.0);
    if alpha <= 0.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(max(s.rgb, vec3<f32>(0.0)), alpha);
}
