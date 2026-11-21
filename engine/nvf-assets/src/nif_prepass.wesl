// Depth-only prepass fragment of NIF shapes (nif_material.rs `nif_prepass`; the shadow
// passes use it too): the dither of a crossfade, then the alpha test on the alpha the
// main pass tests. Bevy's own prepass fragment tests material alpha × texture alpha and
// leaves the vertex alpha out, which the main pass multiplies in: a shape whose vertex
// alpha falls below its cutoff then held depth where the main pass drew nothing, and
// the clear colour showed through whatever lay behind it (docs/spec-notes/nif.md §6
// "Alpha", "Vertex alpha and the test").

import bevy_pbr::{
    prepass::io::VertexOutput,
    render::pbr_prepass_functions::prepass_sample_color_and_alpha_discard,
};
@if(VISIBILITY_RANGE_DITHER) import bevy_pbr::render::pbr_functions::visibility_range_dither;
@if(FIRST_PERSON_DEPTH) import nvf_assets::first_person_depth::squeeze;
@if(MOTION_VECTOR_PREPASS) import bevy_pbr::render::pbr_prepass_functions::calculate_motion_vector;
@if(MAY_DISCARD) import bevy_pbr::{
    render::pbr_types,
    render::mesh_view_bindings::view,
};
@if(MAY_DISCARD) import nvf_assets::nif_surface_bindings::{nif_surface, standard_material, holes_alpha, base_color_texel};
@if(COVERAGE_MASK) import nvf_assets::coverage::{edge_share, sample_mask};

// Bevy's cutoff for blended shapes in a depth-only pass (its PREMULTIPLIED_ALPHA_CUTOFF).
@if(MAY_DISCARD) const BLENDED_CUTOFF: f32 = 0.05;

// A first-person shape (nif_material.rs "First-person depth") writes its depth squeezed
// in front of the world, as the main pass does.
@if(FIRST_PERSON_DEPTH && !MOTION_VECTOR_PREPASS) @fragment
fn fragment(in: VertexOutput) -> @builtin(frag_depth) f32 {
    test_fragment(in);
    return squeeze(in.position.z);
}

@if(!FIRST_PERSON_DEPTH && !MOTION_VECTOR_PREPASS && !COVERAGE_MASK) @fragment
fn fragment(in: VertexOutput) {
    test_fragment(in);
}

// COVERAGE_MASK (nif_material.rs `soft_edges`, a multisampled view's depth-only prepass):
// an alpha-tested shape's soft edge is written here, as the samples it covers.
@if(!FIRST_PERSON_DEPTH && !MOTION_VECTOR_PREPASS && COVERAGE_MASK) @fragment
fn fragment(in: VertexOutput) -> @builtin(sample_mask) u32 {
    return test_fragment(in);
}

// Motion uses Bevy's previous model/skin/morph and unjittered camera matrices. Depth
// squeezing changes only z; the hands retain their real screen-space motion.
@if(MOTION_VECTOR_PREPASS)
struct MotionOutput {
    @location(1) motion_vector: vec2<f32>,
    // A multisampled view's soft edge (nif_material.rs `prepass_coverage`).
    @if(COVERAGE_MASK)
    @builtin(sample_mask) mask: u32,
    @if(FIRST_PERSON_DEPTH || UNCLIPPED_DEPTH_ORTHO_EMULATION)
    @builtin(frag_depth) depth: f32,
}

@if(MOTION_VECTOR_PREPASS) @fragment
fn fragment(in: VertexOutput) -> MotionOutput {
    var out: MotionOutput;
    @if(COVERAGE_MASK) out.mask = test_fragment(in);
    @if(!COVERAGE_MASK) test_fragment(in);
    out.motion_vector = calculate_motion_vector(in.world_position, in.previous_world_position);
    @if(FIRST_PERSON_DEPTH) out.depth = squeeze(in.position.z);
    @if(!FIRST_PERSON_DEPTH && UNCLIPPED_DEPTH_ORTHO_EMULATION) out.depth = in.unclipped_depth;
    return out;
}

// The dither and the alpha test, which discard the fragment. Returns the samples the
// fragment covers: all of them, but for an alpha-tested shape's soft edge (COVERAGE_MASK).
fn test_fragment(in: VertexOutput) -> u32 {
    var mask = 0xffffffffu;
    @if(VISIBILITY_RANGE_DITHER) visibility_range_dither(in.position, in.visibility_range_dither);
    @if(MAY_DISCARD) let standard = standard_material(in.instance_index);
    @if(MAY_DISCARD) let surface = nif_surface(in.instance_index);
    @if(MAY_DISCARD) let flags = standard.flags;
    @if(MAY_DISCARD) var alpha = standard.base_color.a;
    @if(MAY_DISCARD && VERTEX_COLORS) alpha *= in.color.a;
    @if(MAY_DISCARD && VERTEX_UVS_A) if (flags & pbr_types::STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u {
        let uv = (standard.uv_transform * vec3(in.uv, 1.0)).xy;
        alpha *= base_color_texel(in.instance_index, uv, view.mip_bias).a;
    }
    // The cut of a blended shape that is also alpha-tested (0 on every other shape); the
    // view-angle falloff of the main pass is left out, since a shadow has no view angle.
    @if(MAY_DISCARD) if alpha < surface.alpha_cut {
        discard;
    }
    // The holes of the main pass (nif_material.rs "Holes").
    @if(MAY_DISCARD && VERTEX_UVS_A) if surface.holes.x >= 0.0 && surface.holes.x < 255.0 {
        let h = holes_alpha(in.instance_index, in.uv) * 255.0;
        if select((h >= surface.holes.x), (h < surface.holes.x), (surface.holes.z > 0.5)) {
            discard;
        }
    }
    @if(MAY_DISCARD) let alpha_mode = flags & pbr_types::STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS;
    @if(MAY_DISCARD) if alpha_mode == pbr_types::STANDARD_MATERIAL_FLAGS_ALPHA_MODE_MASK {
        @if(!COVERAGE_MASK) if alpha < standard.alpha_cutoff {
            discard;
        }
        @if(COVERAGE_MASK) mask = sample_mask(edge_share(alpha, standard.alpha_cutoff), in.position.xy);
        @if(COVERAGE_MASK) if mask == 0u {
            discard;
        }
    } else if alpha_mode == pbr_types::STANDARD_MATERIAL_FLAGS_ALPHA_MODE_BLEND
        || alpha_mode == pbr_types::STANDARD_MATERIAL_FLAGS_ALPHA_MODE_ADD
        || alpha_mode == pbr_types::STANDARD_MATERIAL_FLAGS_ALPHA_MODE_ALPHA_TO_COVERAGE {
        if alpha < BLENDED_CUTOFF {
            discard;
        }
    } else {
        // Premultiplied and multiply shapes: as Bevy tests them.
        prepass_sample_color_and_alpha_discard(in);
    }
    return mask;
}
