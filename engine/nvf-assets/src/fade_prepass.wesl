// Depth-only prepass fragment for meshes in a crossfade (nif_material.rs `fade_prepass`):
// the dither of the main pass, then the alpha-mask discard Bevy's depth-only prepass
// fragment does.
//
// COVERAGE_MASK (nif_material.rs `soft_edges`, a multisampled view's prepass): instead of
// the alpha test at the cutoff, the soft edge the main pass used to draw is written here
// as a sample mask (coverage.wesl). GRASS_SHARE (nvforever grass.rs): the share is
// alpha / cutoff, as grass.wesl's main pass took it, instead of the one-pixel edge.

import bevy_pbr::{
    prepass::io::VertexOutput,
    render::pbr_prepass_functions::prepass_sample_color_and_alpha_discard,
};
@if(VISIBILITY_RANGE_DITHER) import bevy_pbr::render::pbr_functions::visibility_range_dither;
@if(COVERAGE_MASK) import bevy_pbr::render::pbr_prepass_functions::nvf_prepass_sample_color;
@if(COVERAGE_MASK) import nvf_assets::coverage::{edge_share, sample_mask};
@if(MOTION_VECTOR_PREPASS) import bevy_pbr::render::pbr_prepass_functions::calculate_motion_vector;

@if(!COVERAGE_MASK) @fragment
fn fragment(in: VertexOutput) {
    @if(VISIBILITY_RANGE_DITHER) visibility_range_dither(in.position, in.visibility_range_dither);
    prepass_sample_color_and_alpha_discard(in);
}

@if(COVERAGE_MASK && !MOTION_VECTOR_PREPASS) @fragment
fn fragment(in: VertexOutput) -> @builtin(sample_mask) u32 {
    return coverage(in);
}

// The same with the motion vector, for a view with a motion-vector prepass (nif_material.rs
// `prepass_coverage`: Modern Feel's object motion blur keeps multisampling).
@if(COVERAGE_MASK && MOTION_VECTOR_PREPASS)
struct MotionOutput {
    @location(1) motion_vector: vec2<f32>,
    @builtin(sample_mask) mask: u32,
}

@if(COVERAGE_MASK && MOTION_VECTOR_PREPASS) @fragment
fn fragment(in: VertexOutput) -> MotionOutput {
    var out: MotionOutput;
    out.mask = coverage(in);
    out.motion_vector = calculate_motion_vector(in.world_position, in.previous_world_position);
    return out;
}

@if(COVERAGE_MASK)
fn coverage(in: VertexOutput) -> u32 {
    @if(VISIBILITY_RANGE_DITHER) visibility_range_dither(in.position, in.visibility_range_dither);
    let sampled = nvf_prepass_sample_color(in);
    @if(GRASS_SHARE) let share = saturate(sampled.color.a / max(sampled.alpha_cutoff, 0.01));
    @if(GRASS_SHARE) if share < 0.03 {
        discard;
    }
    @if(!GRASS_SHARE) let share = edge_share(sampled.color.a, sampled.alpha_cutoff);
    let mask = sample_mask(share, in.position.xy);
    if mask == 0u {
        discard;
    }
    return mask;
}
