// [design] Mesh terrain blend's single-texture capture (terrain_surface.rs
// `SurfaceCaptureMaterial`): the unlit colour of a landscape draw that is not a folded
// base (a fallback or non-bindless base, or a blended layer drawn over a base), with one
// diffuse texture, one sampler and one small uniform, so its material group stays small
// on a GPU without bindless textures. Colour: material x vertex x diffuse (a missing
// texture counts as white). A base copy writes alpha 1; a layer copy writes material x
// vertex alpha (the diffuse's has no part, as in the near layers' draws, #3789), which its
// pipeline's alpha blend composites over the copies before it, in the near layers' order.
// No lighting, scorch, fog or exposure.
//
// TERRAIN_ANTI_TILING (#2551; terrain_surface.rs `SurfaceCaptureMaterial::from_source`):
// the source draws its texture anti-tiled (nvf_assets terrain_pbr.wesl
// `terrain_pbr_standard`), so the copy reads it the same way: at the same world lattice
// place (the bake draws its copies at the cells' world places, terrain_surface.rs
// `bake_root`), through the same transform, with the same Hex3 taps and blend
// (terrain_sampling.wesl), gradients from this view.
import bevy_pbr::render::forward_io::{VertexOutput, FragmentOutput};
@if(TERRAIN_ANTI_TILING) import nvf_assets::terrain_sampling::{TerrainFootprint, terrain_hex3, terrain_sample_diffuse, terrain_world_uv};

struct SurfaceCapture {
    // Linear material colour, alpha the material's opacity.
    colour: vec4<f32>,
    // The texture coordinate transform's x axis (xy) and y axis (zw).
    uv_axes: vec4<f32>,
    // xy the transform's translation; z 1 with a diffuse texture; w 1 for a layer copy.
    uv_offset_flags: vec4<f32>,
    // x the anti-tiled source's world units per texture repeat (0: read plainly).
    anti_tiling: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> surface_capture: SurfaceCapture;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var diffuse_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var diffuse_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    var colour = surface_capture.colour;
    @if(VERTEX_COLORS) colour *= in.color;
    @if(VERTEX_UVS_A && !TERRAIN_ANTI_TILING) {
        let uv = mat2x2<f32>(surface_capture.uv_axes.xy, surface_capture.uv_axes.zw) * in.uv + surface_capture.uv_offset_flags.xy;
        // Sampled unconditionally (uniform control flow), then ignored without a texture.
        let texel = textureSample(diffuse_texture, diffuse_sampler, uv);
        colour *= vec4(select(vec3(1.0), texel.rgb, surface_capture.uv_offset_flags.z > 0.5), 1.0);
    }
    // The anti-tiled read: the place in the world over the repeat, then the transform
    // (terrain_pbr.wesl `terrain_footprint`); derivatives here, in uniform control flow.
    @if(TERRAIN_ANTI_TILING) {
        let at_uv = mat2x2<f32>(surface_capture.uv_axes.xy, surface_capture.uv_axes.zw) * terrain_world_uv(in.world_position.xyz, surface_capture.anti_tiling.x) + surface_capture.uv_offset_flags.xy;
        let at_texel = terrain_sample_diffuse(diffuse_texture, diffuse_sampler, terrain_hex3(at_uv), TerrainFootprint(at_uv, dpdx(at_uv), dpdy(at_uv)));
        colour *= vec4(select(vec3(1.0), at_texel.rgb, surface_capture.uv_offset_flags.z > 0.5), 1.0);
    }
    var out: FragmentOutput;
    out.color = vec4(colour.rgb, select(1.0, colour.a, surface_capture.uv_offset_flags.w > 0.5));
    return out;
}
