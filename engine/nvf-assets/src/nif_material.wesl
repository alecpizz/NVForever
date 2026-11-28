// NIF surface material (nif_material.rs): Bevy's standard PBR fragment with six changes,
// all [hypothesis, docs/spec-notes/nif.md §6 "Emissive light", "Specular", "View-angle
// falloff", "Multiply blend", "Additive blend", "Alpha"]:
// - the alpha is scaled by the view-angle falloff (blended effect shapes only) and, in the
//   main pass, by the soft intersection with the surface behind (`soft_fade`);
// - a blended shape that is also alpha-tested drops the fragments below its cut;
// - the emissive light is modulated by the surface colour (diffuse texture × vertex
//   colour), as NIF emissive is;
// - ambient light is diffuse only: the specular part Bevy adds for its ambient light is
//   taken out again, since the game's ambient term is a flat colour and highlights come
//   from lights (and environment maps, added after the lighting: NifSurface::env; and, on
//   authored materials with reflections on, from the view's reflection cube: NVF_PBR_REFLECT
//   below);
// - a multiply-blended shape's colour is a factor on what lies behind it: summed in
//   display values, held to 0..1, and neither fogged nor tonemapped;
// - an additive shape adds what it would add to the display values of the opaque frame
//   behind it, read from Bevy's view transmission texture (`add_display`);
// - an unlit alpha-blended shape (smoke, mist, dust, particles) puts about what it would
//   put on display values: its colour term is colour × alpha^1.5 (`blend_colour_scale`).

// Meshes in a crossfade (VisibilityRange; streamed cells fading in and out) are dithered
// first, as Bevy's own PBR fragment does.
//
// SHADE_LIGHT (shapes with NifSurface::shade_light, the player's first-person arms): the
// sun's shadows on the shape are those in the shade light's shadow map, which holds only
// casters near the camera, and those in the sun's own map cast from beyond the player's
// unseen body (`with_shade_light`, `shadow_past_body`) [engineering choice].
//
// REFRACT (shapes with the Refraction shader flag, main pass): what lies behind the shape
// (Bevy's view transmission texture), shifted on the screen by the shape's normal (normal
// map applied) in view space times `surface.refraction` (the strength already in screen
// UV) [hypothesis, nif_material.rs "Refraction"]; nothing of the shape's own colour or
// lighting. With a fire period the vertex normal's tilt and the normal map's, scrolled
// over time, are used instead (nif_material.rs "Fire refraction").
//
// Holes (nif_material.rs "Holes"): where `surface.holes.x` is 0 or more, the holes
// texture's alpha (0-255) at or above it leaves a hole and the band of `surface.holes.y`
// below it is drawn in `surface.holes_edge`; at 255 or more there are none. With
// `surface.holes.z` 1 the test is turned round (a cut-out): alpha below it leaves a
// hole, and there is no band.
//
// FACEGEN_TINT (an NPC's face and skin shapes, main pass; nif_material.rs "FaceGen tint"):
// the `.egt` colour offset is added to the diffuse texture's stored sRGB value (0-255)
// and held to 0-255, as fnv_actor::facegen::add_offset does to the texture's bytes.
//
// SOFT_EDGES (alpha-tested shapes in the main pass of a multisampled view): instead of
// the alpha test, the fragment's alpha is the share of the pixel's samples it covers
// (nif_material.rs `soft_edges`).
//
// FIRST_PERSON_DEPTH (the arms and weapon in view, main pass of a perspective view;
// nif_material.rs "First-person depth"): the fragment's depth is written squeezed into the
// nearest share of the depth range (first_person_depth.wesl), so the world never hides it.
//
// TERRAIN_ANTI_TILING with TERRAIN_SEPARATE (the landscape's own materials with Terrain
// anti-tiling on, main pass; nif_material.rs "Landscape anti-tiling", [design, #2551]): the
// surface is set up by nvf_assets terrain_pbr.wesl (`terrain_pbr_standard`), its diffuse
// texture and normal map read anti-tiled at the footprint taken first, instead of by Bevy's
// pbr_input_from_standard_material; the rest of the fragment is as for any opaque or
// blended lit shape.
//
// TERRAIN_LAYER (the landscape's own materials, main pass; nif_material.rs "Landscape
// layers"): the alpha is the layer's painted opacity (vertex alpha x material alpha); the
// diffuse texture's alpha is left out, as the original leaves it (#3789).
//
// PIPBOY_SCREEN (the Pip-Boy's screen with the menus on it, main pass; NifSurface::
// pipboy_screen): the texel is the menus' picture as `pipboy_picture` makes it (rolled,
// softened, washed, with noise; NifSurface::pipboy_a, pipboy_b), red stops at a ceiling and
// the pixels are marked for the image-space grade.

import bevy_pbr::{
    render::pbr_fragment::pbr_input_from_standard_material,
    render::pbr_functions::alpha_discard,
};
@if(VISIBILITY_RANGE_DITHER) import bevy_pbr::render::pbr_functions::visibility_range_dither;
@if(TERRAIN_ANTI_TILING) import nvf_assets::terrain_pbr::{terrain_footprint, terrain_pbr_standard};

@if(PREPASS_PIPELINE) import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
};
@if(!PREPASS_PIPELINE) import bevy_pbr::{
    render::forward_io::{VertexOutput, FragmentOutput},
    render::pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing, calculate_F0},
    render::pbr_types::{STANDARD_MATERIAL_FLAGS_UNLIT_BIT, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_ADD, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_BLEND, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_PREMULTIPLIED, STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT, STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT},
    render::pbr_lighting::{EnvBRDFApprox, F_AB},
    render::mesh_view_bindings::{lights, view, globals, view_transmission_texture, view_transmission_sampler, fog},
    render::mesh_view_types::FOG_MODE_ATMOSPHERIC,
    render::view_transformations::{position_ndc_to_world, depth_ndc_to_view_z},
};
@if(!PREPASS_PIPELINE && BLEND_MULTIPLY && DISTANCE_FOG && NVF_PHYSICAL_PBR) import bevy_pbr::render::pbr_functions::apply_fog;
@if(!PREPASS_PIPELINE && DEPTH_PREPASS) import bevy_pbr::prepass::utils as prepass_utils;
@if(!PREPASS_PIPELINE) import bevy_pbr::render::mesh_functions::get_world_from_local;
@if(!PREPASS_PIPELINE) import nvf_assets::first_person_depth;
@if(!PREPASS_PIPELINE) import nvf_assets::tone_input::tony_inverse_each;
@if(!PREPASS_PIPELINE && SHADE_LIGHT) import bevy_pbr::{
    render::pbr_types::PbrInput,
    render::mesh_types::{MESH_FLAGS_SHADOW_RECEIVER_BIT, NVF_MESH_FLAGS_NO_DIRECTIONAL_SHADOWS_BIT},
    render::mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    render::shadows::{fetch_directional_shadow, get_cascade_index},
    render::shadow_sampling::sample_shadow_map,
};
// The sun visibility (nvforever sun_visibility.rs) for the shade light's algebra, only
// while the view has the plumbing (the shader def then exists: bevy_pbr mesh.rs).
@if(!PREPASS_PIPELINE && SHADE_LIGHT && NVF_SUN_VISIBILITY) import bevy_pbr::render::mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT;
@if(!PREPASS_PIPELINE && SHADE_LIGHT && NVF_SUN_VISIBILITY) import nvforever::sun_visibility::nvf_sun_visibility;
// The probe volume's bounce light (nvforever rtgi_volume.rs), only in the pipelines that read
// it (nif_material.rs `reads_rtgi_volume`).
@if(!PREPASS_PIPELINE && NVF_RTGI_VOLUME) import nvforever::rtgi_volume::nvf_rtgi_face;

import nvf_assets::nif_surface_bindings::{NifSurface, nif_surface, standard_material, holes_alpha, facegen_size, facegen_texel, base_color_texel, base_color_texel_level, base_color_size, normal_map_texel};
@if(NVF_PBR_VIEW_OCCLUSION && !PREPASS_PIPELINE) import nvf_assets::nif_surface_bindings::occlusion_texel;
@if(NVF_PBR_VIEW_OCCLUSION && !PREPASS_PIPELINE) import bevy_pbr::render::pbr_types::STANDARD_MATERIAL_FLAGS_OCCLUSION_TEXTURE_BIT;
@if(NVF_PBR_VIEW_HEIGHT && !PREPASS_PIPELINE) import nvf_assets::nif_surface_bindings::height_texel;
@if(NVF_PBR_VIEW_HEIGHT && !PREPASS_PIPELINE) import bevy_pbr::render::pbr_types::STANDARD_MATERIAL_FLAGS_DEPTH_MAP_BIT;
@if(NVF_PBR_PARALLAX_MASK && !PREPASS_PIPELINE) import bevy_pbr::render::pbr_types as parallax_mask_types;
@if(MESH_TERRAIN_BLEND) import nvf_assets::nif_surface_bindings::{mesh_terrain_lookup, mesh_terrain_view, mesh_terrain_shading, mesh_terrain_unlit_view, mesh_terrain_diagnostic, mesh_terrain_blend_normal, mesh_terrain_target_colour, mesh_terrain_target_normal, mesh_terrain_face_up, MeshTerrainInputs, LOOKUP_SUCCESS, LAND_ROUGHNESS, LAND_REFLECTANCE};
@if(!NIF_SURFACE_BLEND) import nvf_assets::nif_surface_bindings::{env_cube_texel, env_mask_texel, env_mask_alpha};
// NVF_PBR_VIEW=reflection (nvf-assets authored.rs `PbrView::Reflection`): the view's cube,
// read here only by this opted-in fragment (NVF_PBR_REFLECT) in a view that has one.
@if(NVF_PBR_REFLECT && ENVIRONMENT_MAP && !PREPASS_PIPELINE) import bevy_pbr::render::mesh_view_bindings::light_probes;
@if(NVF_PBR_VIEW_REFLECTION && ENVIRONMENT_MAP && NVF_PBR_REFLECT && !PREPASS_PIPELINE) import bevy_pbr::render::mesh_view_bindings::environment_map_sampler;
@if(NVF_PBR_VIEW_REFLECTION && ENVIRONMENT_MAP && NVF_PBR_REFLECT && !PREPASS_PIPELINE && !MULTIPLE_LIGHT_PROBES_IN_ARRAY) import bevy_pbr::render::mesh_view_bindings::specular_environment_map;
@if(NVF_PBR_VIEW_REFLECTION && ENVIRONMENT_MAP && NVF_PBR_REFLECT && !PREPASS_PIPELINE && MULTIPLE_LIGHT_PROBES_IN_ARRAY) import bevy_pbr::render::mesh_view_bindings::specular_environment_maps;

// [design] Whether the drawing view has a cube of its own. ENVIRONMENT_MAP alone does not say
// so: bevy_pbr also sets it for a view that sees a light-probe volume, and then leaves
// `view_cubemap_index` at -1 (vendor/bevy_pbr/src/light_probe/mod.rs, the view's light probe
// uniform) [documented]. Every use of the cube on authored materials (the effective metallic,
// the reflection weight, the metal base lift, the mirror debug view) asks this too.
@if(NVF_PBR_REFLECT && ENVIRONMENT_MAP && !PREPASS_PIPELINE) fn nvf_view_has_cube() -> bool {
    return light_probes.view_cubemap_index >= 0;
}

// [design] The base colour an authored metal is lit with in a view with a reflection cube
// (#2981, nvf-assets authored.rs `lifted_base` is its CPU mirror): mix(base, lifted,
// metallic), lifted = base x min(gain, 1 / brightest channel), never below base: one gain on
// all three channels, limited so the brightest reaches 1, so the hue is kept. The game's
// diffuse is a painted colour far darker than a metal's reflectance; gain 1 leaves the base
// as it is.
@if(NVF_PBR_REFLECT && ENVIRONMENT_MAP && !PREPASS_PIPELINE) fn nvf_metal_base(base: vec3<f32>, gain: f32, metallic: f32) -> vec3<f32> {
    let brightest = max(max(base.r, base.g), max(base.b, 1e-4));
    let lifted = base * max(min(gain, 1.0 / brightest), 1.0);
    return mix(base, lifted, metallic);
}

// NVF_PBR_VIEW=reflection: what a white perfect mirror (metallic 1, roughness 0) facing N
// writes for the viewer at V: the view's cube at level 0 along the mirror direction (z
// negated, as bevy_pbr environment_map.wesl samples it; the component's rotation is
// identity, nvf_assets reflection_cube.rs), times the light's intensity and the view's
// exposure, as Bevy's environment term writes it (F ~ 1 there).
@if(NVF_PBR_VIEW_REFLECTION && ENVIRONMENT_MAP && NVF_PBR_REFLECT && !PREPASS_PIPELINE) fn nvf_reflection_view(N: vec3<f32>, V: vec3<f32>) -> vec3<f32> {
    var dir = reflect(-V, N);
    dir.z = -dir.z;
    var texel = vec3(0.0);
    @if(!MULTIPLE_LIGHT_PROBES_IN_ARRAY) if light_probes.view_cubemap_index >= 0 { texel = textureSampleLevel(specular_environment_map, environment_map_sampler, dir, 0.0).rgb; }
    @if(MULTIPLE_LIGHT_PROBES_IN_ARRAY) if light_probes.view_cubemap_index >= 0 { texel = textureSampleLevel(specular_environment_maps[light_probes.view_cubemap_index], environment_map_sampler, dir, 0.0).rgb; }
    return texel * light_probes.intensity_for_view * view.exposure;
}

// The scene light that scales an environment map by default (env_map.rs `scene_light`,
// its CPU mirror; NifSurface::env_how y = 2): the view's ambient colour plus every
// directional light's (the sun or moon, or an interior's directional light), without
// any N.L, as display values held to 0..1 per channel. In a world view (negative ambient
// alpha, nvforever lights.rs DISPLAY_HELPERS) the uniforms hold display values x the
// light unit (x pi for a directional light), which view.exposure and 1/pi undo, as the
// display-value sum does. Point lights are left out. Other views: 1 [hypothesis, from the
// captures of the original; nif.md section 6 "Environment maps on other shapes"].
@if(!PREPASS_PIPELINE && !NIF_SURFACE_BLEND) fn env_scene_light() -> vec3<f32> {
    if lights.ambient_color.a >= 0.0 {
        return vec3(1.0);
    }
    var sum = lights.ambient_color.rgb * view.exposure;
    for (var i = 0u; i < lights.n_directional_lights; i += 1u) {
        sum += lights.directional_lights[i].color.rgb * (view.exposure / 3.14159265);
    }
    return clamp(sum, vec3(0.0), vec3(1.0));
}

// The direction the cube is read in (env_map.rs `lookup`, its CPU mirror): the view reflected
// about the interpolated normal (`mark` 2) or the shading normal (1), turned as `axes` says
// (env_map.rs Axes). Which side the normal faces does not matter: reflect(I, N) is
// reflect(I, -N). An interpolated normal of no length (a vertex stores none, or opposing ones
// meet) falls back to the view direction, so nothing that is not finite is sampled.
@if(!PREPASS_PIPELINE && !NIF_SURFACE_BLEND) fn env_lookup(v: vec3<f32>, shading: vec3<f32>, interpolated: vec3<f32>, axes: f32, mark: f32) -> vec3<f32> {
    var n = shading;
    if mark > 1.5 {
        let squared = dot(interpolated, interpolated);
        n = select(v, interpolated * inverseSqrt(max(squared, 1e-12)), squared > 1e-12);
    }
    let r = reflect(-v, n);
    if axes > 3.5 {
        return vec3(r.x, -r.z, r.y);
    } else if axes > 2.5 {
        return vec3(r.x, r.y, r.z);
    } else if axes > 1.5 {
        return vec3(r.x, -r.y, -r.z);
    } else if axes > 0.5 {
        return vec3(-r.x, r.y, -r.z);
    }
    return vec3(r.x, r.y, -r.z);
}

// Main-pass geometry uses the render viewport, which can be smaller than the
// display attachment when FSR is enabled. Transmission remains a full-size texture.
@if(!PREPASS_PIPELINE) fn frag_coord_to_uv(fc: vec2<f32>) -> vec2<f32> {
    return (fc - view.main_pass_viewport.xy) / view.main_pass_viewport.zw;
}
@if(!PREPASS_PIPELINE) fn frag_coord_to_ndc(fc: vec4<f32>) -> vec3<f32> {
    let uv = frag_coord_to_uv(fc.xy);
    return vec3(uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), fc.z);
}
@if(!PREPASS_PIPELINE) fn transmission_uv(uv: vec2<f32>) -> vec2<f32> {
    let pixel = view.main_pass_viewport.xy + uv * view.main_pass_viewport.zw;
    return clamp(pixel, view.main_pass_viewport.xy + 0.5,
        view.main_pass_viewport.xy + view.main_pass_viewport.zw - 0.5)
        / vec2<f32>(textureDimensions(view_transmission_texture));
}

// The FaceGen offset at texture coordinate `uv` (CPU: fnv_actor::facegen::add_offset):
// the offset image stretched over the texture, flipped vertically (its first row is the
// texture's bottom row) and read bilinearly between texel centres, held at its edges. A
// coordinate outside 0..1 reads the offset where the repeated texture is read.
@if(FACEGEN_TINT) fn facegen_offset_at(ii: u32, uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(facegen_size(ii));
    let t = fract(uv);
    let p = clamp(vec2(t.x, 1.0 - t.y) * size - 0.5, vec2(0.0), size - 1.0);
    let p0 = vec2<i32>(floor(p));
    let p1 = min(p0 + 1, vec2<i32>(size) - 1);
    let f = p - floor(p);
    let top = mix(facegen_texel(ii, p0), facegen_texel(ii, vec2(p1.x, p0.y)), f.x);
    let bottom = mix(facegen_texel(ii, vec2(p0.x, p1.y)), facegen_texel(ii, p1), f.x);
    return mix(top, bottom, f.y);
}

// sRGB transfer functions (IEC 61966-2-1), as the GPU applies them to an sRGB texture.
@if(FACEGEN_TINT) fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3(0.0031308));
}

@if(FACEGEN_TINT) fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

// PIPBOY_SCREEN: the sRGB transfer functions again, for the Pip-Boy's screen
// (`pipboy_picture`), held at 0 below.
@if(PIPBOY_SCREEN) fn screen_display(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3(0.0));
    return select(1.055 * pow(x, vec3(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3(0.0031308));
}

@if(PIPBOY_SCREEN) fn screen_light(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3(0.0));
    return select(pow((x + 0.055) / 1.055, vec3(2.4)), x / 12.92, x <= vec3(0.04045));
}

// PIPBOY_SCREEN: a whole-number hash (PCG), for the screen's noise.
@if(PIPBOY_SCREEN) fn screen_hash(v: u32) -> u32 {
    let state = v * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// PIPBOY_SCREEN: a value in 0..1 for the view pixel `cell` on frame `frame`, the `k`th of
// two.
@if(PIPBOY_SCREEN) fn screen_noise(cell: vec2<u32>, frame: u32, k: u32) -> f32 {
    let h = screen_hash(cell.x ^ screen_hash(cell.y ^ screen_hash(frame * 2u + k)));
    return f32(h >> 8u) / 16777216.0;
}

// PIPBOY_SCREEN (NifSurface::pipboy_a, pipboy_b; nvf-play pipboy/transition.rs and
// pipboy/device_screen.rs): the menus' picture at `uv` (texture coordinates; `dx`, `dy` their
// change across one view pixel), in linear light. Rolled up by a.x of the canvas, wrapping
// inside it (a.w and b.z its share of the texture's height and width; a shudder is the same
// roll, so the rows it uncovers come in from the canvas's other edge [hypothesis: what the
// original shows there is not known, docs/spec-notes/pipboy.md §3]); softened by a 3 x 3
// tent of taps b.x view pixels apart; its display values scaled by 1 - a.y with a.z added; and
// triangular noise of reach b.y added to them, new on every frame, at the view pixel `pixel`.
// Every tap is read inside the canvas: wrapped up and down while it rolls, then held half a
// texel inside its edges, so the filter never reads the image beyond it (the undercoat, or the
// far side of the texture, which the sampler repeats).
@if(PIPBOY_SCREEN) fn pipboy_picture(instance_index: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>, a: vec4<f32>, b: vec4<f32>, pixel: vec2<f32>) -> vec3<f32> {
    let canvas = vec2(select(1.0, b.z, b.z > 0.0), select(1.0, a.w, a.w > 0.0));
    let half = 0.5 / vec2<f32>(max(base_color_size(instance_index), vec2(1u)));
    let rolling = a.x != 0.0;
    var at = uv;
    if rolling {
        at.y = fract(uv.y / canvas.y + a.x) * canvas.y;
    }
    var sum = vec3(0.0);
    for (var j = -1; j <= 1; j += 1) {
        for (var i = -1; i <= 1; i += 1) {
            let w = f32((2 - abs(i)) * (2 - abs(j)));
            var p = at + (f32(i) * dx + f32(j) * dy) * b.x;
            if rolling {
                p.y = fract(p.y / canvas.y) * canvas.y;
            }
            p = clamp(p, half, max(canvas - half, half));
            sum += w * base_color_texel_level(instance_index, p, 0.0).rgb;
        }
    }
    var d = screen_display(sum / 16.0) * (1.0 - a.y) + vec3(a.z);
    let cell = vec2<u32>(max(pixel, vec2(0.0)));
    let n = screen_noise(cell, globals.frame_count, 0u) + screen_noise(cell, globals.frame_count, 1u) - 1.0;
    d += vec3(n * b.y);
    return screen_light(clamp(d, vec3(0.0), vec3(1.0)));
}

// A texel `texel` (linear, as sampled) with the offset `offset` (colour units) added to
// its stored value and held to 0..255 (CPU mirror: nvforever facegen.rs tests, `tint_texel`).
@if(FACEGEN_TINT) fn facegen_tint(texel: vec3<f32>, offset: vec3<f32>) -> vec3<f32> {
    let stored = linear_to_srgb(saturate(texel)) * 255.0;
    return srgb_to_linear(clamp(stored + offset, vec3(0.0), vec3(255.0)) / 255.0);
}

// View-angle falloff of BSShaderNoLightingProperty (docs/spec-notes/nif.md §6): linear in
// the cosine from the start opacity at the start angle to the stop opacity at the stop
// angle, held beyond either end [hypothesis: linear and clamped].
fn falloff_opacity(cos_angle: f32, p: vec4<f32>) -> f32 {
    let t = clamp((cos_angle - p.y) / (p.x - p.y), 0.0, 1.0);
    return mix(p.w, p.z, t);
}

// The factor a multiply-blended shape puts on what lies behind it (blend factors ZERO and
// SRC_COLOR: result = destination × source colour), for the lit colour and the emissive
// light of the shape, both linear (CPU mirror: nif_material.rs `multiply_factor`). The
// game's shader output is taken as texture × (light + emissive) in display values, held
// to 0..1 when it is written [hypothesis], so the two parts are summed as display values
// (gamma 2.2) and the sum is limited before it goes back to linear.
fn multiply_factor(lit: vec3<f32>, emissive: vec3<f32>) -> vec3<f32> {
    let display = pow(max(lit, vec3(0.0)), vec3(1.0 / 2.2)) + pow(max(emissive, vec3(0.0)), vec3(1.0 / 2.2));
    return pow(saturate(display), vec3(2.2));
}

// Hot light sources (nif.md §6 "Emitted light on screen"). The original draws emitted
// light and clips each channel on screen, so a lamp's glass burns white-yellow with an
// orange edge; the renderer's tonemapper (Bevy's Tony McMapface, in the HDR view) rolls
// very bright orange off to cream. Where a shape emits light, the lit colour plus the
// emitted light is held to HOT_MAX per channel and passed through the inverse of the
// tonemapper's grey response (the same table as nvforever sky.wesl), so after the
// tonemapper it lands where clipping would put it. Lit surfaces without emitted light
// keep the tonemapper [hypothesis from the original's frames of Dead Money's lantern].
const HOT_MAX: f32 = 0.95;

// `lit` plus the emitted light `e` as a hot source: held per channel, mapped through
// the inverse response. Only light that goes past white is a hot source: blended in as
// the emitted light's brightest channel rises from 0.8 to 1.5, so faint effect shapes
// (window light rays, white × 1 under a warm emittance colour) and the dark parts of a
// glow map keep the tonemapper; the lantern glass (× 10) and neon (× 25) burn.
fn hot_light(lit: vec3<f32>, e: vec3<f32>) -> vec3<f32> {
    let hot_share = smoothstep(0.8, 1.5, max(e.r, max(e.g, e.b)));
    // No hot share (the brightest emitted channel at 0.8 or less, nearly every lit pixel):
    // the mix below gives `lit + e`, so the three walks of the inverse table are skipped
    // (#2868). No derivatives here, so the branch may differ between fragments.
    var out = lit + e;
    if hot_share > 0.0 {
        let d = min(lit + e, vec3(HOT_MAX));
        let hot = tony_inverse_each(d);
        out = mix(lit + e, hot, hot_share);
    }
    // HDR output (nvforever hdr.rs): the SDR picture up to white, and above white the light
    // the shape really emits (its emissive multiplier: lantern glass x 10, neon x 25), so
    // the HDR pass shows a neon tube brighter than a lantern. Clipped at 1 per channel, as
    // the SDR picture is, both give the same.
    @if(NVF_HDR) out = min(out, vec3(1.0)) + max(lit + e - vec3(1.0), vec3(0.0));
    return out;
}

// The linear light an additive shape (blend factors SRC_ALPHA or ONE, and ONE) of linear
// colour `colour` and alpha `a` adds over the opaque frame `behind` (linear; CPU mirror
// and the reasons: nif_material.rs `add_display`). The game adds colour × alpha to the
// frame's display values [hypothesis]: the sum is formed in display values (gamma 2.2),
// held to ADD_DISPLAY_MAX (2.0) or the frame's own value if brighter, and the difference
// to the frame returned as linear light.
// The sRGB transfer, display value to linear (as bevy::color's Srgba to LinearRgba;
// CPU mirror: nif_spawn.rs `unlit_vertex_colors`' conversion).
fn display_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3(0.0)) + 0.055) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

// The light an additive shape adds once fogged (CPU mirror: nif_material.rs
// `fogged_addition`): its colour through the fog less black through the same fog.
fn fogged_addition(fogged: vec3<f32>, fogged_black: vec3<f32>) -> vec3<f32> {
    return max(fogged - fogged_black, vec3(0.0));
}

fn add_display(colour: vec3<f32>, a: f32, behind: vec3<f32>) -> vec3<f32> {
    let b = max(behind, vec3(0.0));
    let shown = pow(b, vec3(1.0 / 2.2));
    let added = pow(max(colour, vec3(0.0)), vec3(1.0 / 2.2)) * saturate(a);
    let sum = min(shown + added, max(shown, vec3(2.0)));
    return max(pow(sum, vec3(2.2)) - b, vec3(0.0));
}

// The factor on the colour of an unlit alpha-blended shape of alpha `a` (CPU mirror and
// the reasons: nif_material.rs `blend_colour_scale`). Bevy's blend is colour × alpha +
// behind × (1 − alpha) in linear light; the game's is the same sum in display values
// [hypothesis], where light smoke over a dark room adds far less. Scaled by √alpha, the
// colour term becomes colour × alpha^1.5, as for additive shapes; the share of what lies
// behind is kept (nif_material.rs BLEND_ALPHA_POWER).
fn blend_colour_scale(a: f32) -> f32 {
    return sqrt(saturate(a));
}



// The image space's Emissive Mult over the worldspace base image space's (nvforever
// fog.rs and sky.rs `emissive_scale`), which the view's game fog carries in `bi.y`; 1
// with any other fog, and in a view without fog (the scope's picture, which has no `fog`
// binding).
@if(!PREPASS_PIPELINE) fn view_emissive_scale() -> f32 {
    @if(!PREPASS_PIPELINE && DISTANCE_FOG) return select(1.0, fog.bi.y, fog.mode == FOG_MODE_ATMOSPHERIC && fog.bi.y > 0.0);
    @if(!PREPASS_PIPELINE && !DISTANCE_FOG) return 1.0;
}

// Soft intersections (nif_material.rs module notes): the share of the shape kept at this
// fragment, falling from 1 at `distance` in front of the opaque surface behind it (the
// depth prepass) to 0 on it. Not faded where the shape lies along that surface (the two
// facings, from screen derivatives, within about 25 degrees), nor in front of the sky,
// nor without a prepass. Called in uniform control flow (screen derivatives). Compiled only
// into the main pass of shapes with a soft distance (NVF_SOFT_FADE); 1 for every other shape,
// as the select below gives for a distance of 0, without the prepass read (#2868).
@if(!PREPASS_PIPELINE) fn soft_fade(frag_coord: vec4<f32>, world_pos: vec3<f32>, distance: f32) -> f32 {
    // A first-person shape's depth as it was before it was squeezed, so the fade is
    // measured against where it is (nif_material.rs "First-person depth").
    @if(!PREPASS_PIPELINE && NVF_SOFT_FADE && DEPTH_PREPASS) let depth = first_person_depth::unsqueeze(prepass_utils::prepass_depth(frag_coord, 0u));
    @if(!PREPASS_PIPELINE && NVF_SOFT_FADE && DEPTH_PREPASS) let scene = position_ndc_to_world(frag_coord_to_ndc(vec4(frag_coord.xy, max(depth, 1e-7), 1.0)));
    @if(!PREPASS_PIPELINE && NVF_SOFT_FADE && DEPTH_PREPASS) let scene_n = cross(dpdx(scene), dpdy(scene));
    @if(!PREPASS_PIPELINE && NVF_SOFT_FADE && DEPTH_PREPASS) let own_n = cross(dpdx(world_pos), dpdy(world_pos));
    @if(!PREPASS_PIPELINE && NVF_SOFT_FADE && DEPTH_PREPASS) let lengths = length(scene_n) * length(own_n);
    @if(!PREPASS_PIPELINE && NVF_SOFT_FADE && DEPTH_PREPASS) let along = select(0.0, abs(dot(scene_n, own_n)) / lengths, lengths > 1e-12);
    @if(!PREPASS_PIPELINE && NVF_SOFT_FADE && DEPTH_PREPASS) let gap = depth_ndc_to_view_z(frag_coord.z) - depth_ndc_to_view_z(max(depth, 1e-7));
    @if(!PREPASS_PIPELINE && NVF_SOFT_FADE && DEPTH_PREPASS) let fade = mix(saturate(gap / max(distance, 1e-3)), 1.0, smoothstep(0.85, 0.95, along));
    @if(!PREPASS_PIPELINE && NVF_SOFT_FADE && DEPTH_PREPASS) return select(fade, 1.0, distance <= 0.0 || depth <= 0.0);
    @if(!PREPASS_PIPELINE && NVF_SOFT_FADE && !DEPTH_PREPASS) return 1.0;
    // A shape without a soft distance (NifSurfaceKey::soft, #2868): the fade above is 1.
    @if(!PREPASS_PIPELINE && !NVF_SOFT_FADE) return 1.0;
}

// How far past the plane through the eye, square to the sun, a past-body shadow lookup
// starts (game units): the unseen body's half-width and the top of its head
// [engineering choice; nvforever player_body.rs "The shadow"].
const BODY_CLEARANCE: f32 = 24.0;

// The sun's shadow on a fragment from what lies beyond the player's unseen body: the
// sun's own shadow map, read where the fragment is but with the depth of a point moved
// toward the sun past the plane through the eye and BODY_CLEARANCE more. Casters between
// the fragment and that point, the body standing round the eye among them, do not count.
// A point past the cascade's near plane takes a depth just short of it, so casters
// squashed onto that plane still shade (nvforever sky.rs `reach_toward_sun` puts the
// plane well behind the body). One cascade, the one the fragment's depth picks, without
// Bevy's blend into the next (the arms lie in the first) and without the PCSS path
// (NVForever sets no soft shadows).
@if(!PREPASS_PIPELINE && SHADE_LIGHT) fn shadow_past_body(sun: u32, in: PbrInput, view_z: f32) -> f32 {
    let light = &lights.directional_lights[sun];
    let cascade_index = get_cascade_index(sun, view_z);
    if cascade_index >= (*light).num_cascades {
        return 1.0;
    }
    let cascade = &(*light).cascades[cascade_index];
    let to_sun = (*light).direction_to_light.xyz;
    // As in Bevy's sample_directional_cascade.
    let normal_offset = (*light).shadow_normal_bias * (*cascade).texel_size * in.world_normal;
    let at = in.world_position.xyz + normal_offset + (*light).shadow_depth_bias * to_sun;
    // Orthographic: clip space is NDC.
    let ndc = ((*cascade).clip_from_world * vec4(at, 1.0)).xyz;
    if any(ndc.xy < vec2(-1.0)) || any(ndc.xy > vec2(1.0)) || ndc.z < 0.0 {
        return 1.0;
    }
    let past = max(dot(view.world_position - at, to_sun), 0.0) + BODY_CLEARANCE;
    let depth = min(((*cascade).clip_from_world * vec4(at + to_sun * past, 1.0)).z, 0.99999);
    let uv = ndc.xy * vec2(0.5, -0.5) + vec2(0.5);
    let array_index = i32((*light).depth_texture_base_index + cascade_index);
    return sample_shadow_map(uv, depth, array_index, in.frag_coord.xy, (*cascade).texel_size);
}

// The lit colour of a fragment whose sun shadows are those of the shade light and those
// from beyond the unseen body, the darker of the two (nif_material.rs, "Shade light").
// Of the directional lights with shadows, the brightest
// is the sun and the next the shade light, which has the sun's colour at `share` of its
// strength. `shadowed` is Bevy's lighting of `in`, each light with its own shadows.
// Without two such lights it is the result.
//
// Bevy's lighting has no opening for another shadow value, so the result is put together
// from two of its results. With S the sun's unshadowed light on the fragment and R the
// rest of its light:
//   shadowed = R + S * (own + share * shade)    (the shadow values of the two maps)
//   lit      = R + S * (1 + share)              (the same without shadows)
// so S = (lit - shadowed) / ((1 - own) + share * (1 - shade)), and the colour wanted,
// R + S * wanted with wanted = min(shade, past), is lit - S * (1 + share - wanted).
// Whatever shades the point past the body lies on the fragment's own line to the sun, so
// past >= own: where neither map has a shadow (own = shade = 1) the result is `shadowed`.
//
// With the sun visibility (nvforever sun_visibility.rs, shader def NVF_SUN_VISIBILITY) the
// outdoor sun's shadow value C becomes V(C) inside apply_pbr_lighting, for receivers and
// non-receivers alike (non-receivers with C = 1); the shade light is never flagged, so
// its values stay as they were. Then
//   shadowed = R + S * (V(own) + share * shade)
//   lit      = R + S * (V(1) + share)
// so S = (lit - shadowed) / missing with missing = (V(1) - V(own)) + share * (1 - shade),
// and the colour wanted, R + S * V(wanted) (the sun's light with the visibility of the
// wanted shadow value, the shade light's taken out as before), is
// lit - S * (V(1) + share - V(wanted)): k = (V(1) + share - V(wanted)) / missing.
// Off (no def, or the sun not flagged: indoors) V(C) = C, so V(1) = 1, V(own) = own and
// V(wanted) = wanted, and both lines are the ones above term for term:
//   missing = (1 - own) + share * (1 - shade),   k = (1 + share - wanted) / missing.
// `missing` near 0 (V(1) = V(own), e.g. own = 1, or a fully opaque cloud, with shade = 1)
// returns `shadowed`, which keeps the shade light's share there as it did before.
@if(!PREPASS_PIPELINE && SHADE_LIGHT) fn sun_shadow_value(sun: u32, in: PbrInput, cascade: f32, view_z: f32) -> f32 {
    @if(NVF_SUN_VISIBILITY)
    if (lights.directional_lights[sun].flags & DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT) != 0u {
        // As the hook in apply_pbr_lighting computes it (same position, light, view_z).
        return nvf_sun_visibility(in.world_position.xyz, lights.directional_lights[sun].direction_to_light, cascade, view_z);
    }
    return cascade;
}

// The sRGB transfer both ways, held at 0 below, as the display-value sum uses it
// (nvforever lights/display_shader.rs `nvf_linear_to_display`, `nvf_display_to_linear`).
@if(!PREPASS_PIPELINE && SHADE_LIGHT) fn shade_display(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3(0.0));
    return select(1.055 * pow(x, vec3(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3(0.0031308));
}

@if(!PREPASS_PIPELINE && SHADE_LIGHT) fn shade_linear(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3(0.0));
    return select(pow((x + 0.055) / 1.055, vec3(2.4)), x / 12.92, x <= vec3(0.04045));
}

@if(!PREPASS_PIPELINE && SHADE_LIGHT) fn with_shade_light(in: PbrInput, shadowed: vec4<f32>) -> vec4<f32> {
    if (in.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) == 0u {
        return shadowed;
    }
    let none = lights.n_directional_lights;
    var sun = none;
    var sun_amount = 0.0;
    var shade_light = none;
    var shade_amount = 0.0;
    for (var i = 0u; i < none; i += 1u) {
        if (lights.directional_lights[i].flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) == 0u {
            continue;
        }
        let colour = lights.directional_lights[i].color.rgb;
        let amount = max(colour.r, max(colour.g, colour.b));
        if sun == none || amount > sun_amount {
            shade_light = sun;
            shade_amount = sun_amount;
            sun = i;
            sun_amount = amount;
        } else if shade_light == none || amount > shade_amount {
            shade_light = i;
            shade_amount = amount;
        }
    }
    if shade_light == none || shade_amount <= 0.0 {
        return shadowed;
    }
    let share = shade_amount / sun_amount;
    // As in apply_pbr_lighting.
    let view_z = dot(vec4(
        view.view_from_world[0].z,
        view.view_from_world[1].z,
        view.view_from_world[2].z,
        view.view_from_world[3].z
    ), in.world_position);
    let own = fetch_directional_shadow(sun, in.world_position, in.world_normal, view_z, in.frag_coord.xy);
    let shade = fetch_directional_shadow(shade_light, in.world_position, in.world_normal, view_z, in.frag_coord.xy);
    // V(1) and V(own): 1 and own without the sun visibility (see above).
    let v_lit = sun_shadow_value(sun, in, 1.0, view_z);
    let v_own = sun_shadow_value(sun, in, own, view_z);
    let missing = (v_lit - v_own) + share * (1.0 - shade);
    if missing < 0.000001 {
        // Neither map has a shadow here (or the clouds leave no sun to tell apart).
        return shadowed;
    }
    let wanted = min(shade, shadow_past_body(sun, in, view_z));
    // Only the directional lights' shadows are taken off (vendored bevy_pbr mesh_types.wesl):
    // with the receiver bit cleared instead, the lamps' shadows (lamp shadows, the Pip-Boy
    // light) and the ambient occlusion's factor on lamps (nvforever ssao_lighting) differed
    // between `lit` and `shadowed` too, and k (up to 1/share) multiplied that difference
    // into the colour: first-person shapes glowed in the lamps' colour [observed 2026-11-19,
    // Prospector Saloon at 22:00, lamp shadows Quality with ambient occlusion].
    var unshadowed = in;
    unshadowed.flags |= NVF_MESH_FLAGS_NO_DIRECTIONAL_SHADOWS_BIT;
    let lit = apply_pbr_lighting(unshadowed);
    let k = (v_lit + share - sun_shadow_value(sun, in, wanted, view_z)) / missing;
    @if(!NVF_PHYSICAL_PBR)
    if lights.ambient_color.a < 0.0 {
        // The display-value sum (nvforever lights.rs DISPLAY_HELPERS): the diffuse light is
        // the linear colour of the texture's display value × the sum (sRGB curve), so its
        // display value is linear in the shadow values; the same algebra there (the
        // specular part is taken along).
        let lit_d = shade_display(lit.rgb);
        let shadowed_d = shade_display(shadowed.rgb);
        return vec4(shade_linear(lit_d - (lit_d - shadowed_d) * k), shadowed.a);
    }
    // Physical lighting: linear light, never below black, with the shadowed alpha.
    @if(NVF_PHYSICAL_PBR) return vec4(max(lit.rgb - (lit.rgb - shadowed.rgb) * k, vec3(0.0)), shadowed.a);
    @if(!NVF_PHYSICAL_PBR) return lit - (lit - shadowed) * k;
}

@if(FIRST_PERSON_DEPTH) struct FirstPersonOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

// A first-person shape (nif_material.rs "First-person depth"): its colour, at its depth
// squeezed in front of the world.
@if(FIRST_PERSON_DEPTH) @fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FirstPersonOutput {
    let out = nif_fragment(in, is_front);
    return FirstPersonOutput(out.color, first_person_depth::squeeze(in.position.z));
}
@if(!FIRST_PERSON_DEPTH) @fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    return nif_fragment(in, is_front);
}

fn nif_fragment(
    in: VertexOutput,
    is_front: bool,
) -> FragmentOutput {
    // [design] Mesh terrain blend: the derivatives of the fragment's world position, for
    // the face's geometric normal (the steepness fade, nif_surface_bindings.wesl
    // `mesh_terrain_face_up`), and of its game xy (Bevy (x, -z), the same differences
    // picked out) for the landscape layers' explicit-gradient reads, the colour level and
    // the local height reference (`mesh_terrain_detail`). Taken first, in uniform control
    // flow while every fragment of the quad still runs, before the visibility dither and
    // every discard below (terrain.wesl's order); the blend itself stays after the coverage
    // tests. Only the blend variant has these lines.
    @if(MESH_TERRAIN_BLEND && !PREPASS_PIPELINE) let terrain_wdx = dpdx(in.world_position.xyz);
    @if(MESH_TERRAIN_BLEND && !PREPASS_PIPELINE) let terrain_wdy = dpdy(in.world_position.xyz);
    @if(MESH_TERRAIN_BLEND && !PREPASS_PIPELINE) let terrain_dx = vec2(terrain_wdx.x, -terrain_wdx.z);
    @if(MESH_TERRAIN_BLEND && !PREPASS_PIPELINE) let terrain_dy = vec2(terrain_wdy.x, -terrain_wdy.z);
    // The probe volume's reads are moved off the surface along its geometric normal, taken
    // here for the same reason.
    @if(NVF_RTGI_VOLUME && !PREPASS_PIPELINE) nvf_rtgi_face(in.world_position.xyz);
    // Landscape anti-tiling's footprint, while every fragment of the quad still runs.
    @if(TERRAIN_ANTI_TILING) let terrain_fp = terrain_footprint(in.world_position, nif_surface(in.instance_index).terrain.x, standard_material(in.instance_index).uv_transform);
    @if(VISIBILITY_RANGE_DITHER) visibility_range_dither(in.position, in.visibility_range_dither);
    let surface = nif_surface(in.instance_index);
    // NifSurface::pipboy_screen: the picture's texture coordinates and their change across a
    // view pixel, taken here, before anything can discard (pipboy_picture).
    @if(PIPBOY_SCREEN && VERTEX_UVS_A) let screen_uv = (standard_material(in.instance_index).uv_transform * vec3(in.uv, 1.0)).xy;
    @if(PIPBOY_SCREEN && VERTEX_UVS_A) let screen_dx = dpdx(screen_uv);
    @if(PIPBOY_SCREEN && VERTEX_UVS_A) let screen_dy = dpdy(screen_uv);
    // Holes (nif_material.rs "Holes"): 1 in the edge band, 0 elsewhere.
    var holes_band = 0.0;
    @if(VERTEX_UVS_A) if surface.holes.x >= 0.0 && surface.holes.x < 255.0 {
        let h = holes_alpha(in.instance_index, in.uv) * 255.0;
        let cut_out = (surface.holes.z > 0.5);
        if select((h >= surface.holes.x), (h < surface.holes.x), cut_out) {
            discard;
        }
        holes_band = select(select(0.0, 1.0, h >= surface.holes.x - surface.holes.y), 0.0, cut_out);
    }
    // NifSurface::emissive_rule.z: an unlit shape's vertex colours are display values,
    // blended across its triangles as such and made linear here, per fragment
    // (nif_spawn.rs `unlit_vertex_colors`, nif.md §6 "Unlit vertex colours as display
    // values", #2435). Made linear at the vertices, the blend kept a colour falling to
    // black bright almost to the black edge: a glow card's soft wash became a board.
    var vin = in;
    @if(VERTEX_COLORS) if surface.emissive_rule.z > 0.5 {
        vin.color = vec4(display_to_linear(in.color.rgb), in.color.a);
    }
    @if(!TERRAIN_ANTI_TILING) var pbr_input = pbr_input_from_standard_material(vin, is_front);
    @if(TERRAIN_ANTI_TILING) var pbr_input = terrain_pbr_standard(vin, is_front, terrain_fp);
    // A landscape layer's alpha without its texture's (terrain_pbr_standard's is already so).
    @if(TERRAIN_LAYER && !TERRAIN_ANTI_TILING) pbr_input.material.base_color.a = standard_material(in.instance_index).base_color.a;
    @if(TERRAIN_LAYER && !TERRAIN_ANTI_TILING && VERTEX_COLORS) pbr_input.material.base_color.a *= vin.color.a;
    // An authored set's roughness is its scale (0 to 2, fnv-pbr `ROUGHNESS_SCALE_RANGE`) times
    // the ORM's green, so it can pass 1, which Bevy's sampling does not clamp: F_AB's
    // polynomial then leaves its range and the highlight terms blow up to white patches
    // (#2981). No surface is rougher than 1; Bevy raises the low end to 0.089 where the
    // highlight's shape needs it, and 0 here only keeps a signed map's negative out.
    pbr_input.material.perceptual_roughness = clamp(pbr_input.material.perceptual_roughness, 0.0, 1.0);
    // NVF_PBR_PARALLAX_MASK (nvf-assets authored.rs `parallax_mask`, a development switch, off
    // unless NVF_PBR_PARALLAX_MASK=1 at the start): an alpha-tested shape may carry a height
    // map. Its alpha (material x vertex x texel) is read again at the unshifted coordinates,
    // as nif_prepass.wesl, Bevy's prepass and the shadow passes read it, and replaces the
    // shifted texel's alpha for the tests below, so the main pass cuts what the depth prepass
    // cut (its equal-depth test then finds every kept fragment) and the soft edges come from
    // the same alpha. The colour, normal and ORM inside the cut stay the shifted texels'.
    @if(NVF_PBR_PARALLAX_MASK && !PREPASS_PIPELINE && VERTEX_UVS_A) if (pbr_input.material.flags & parallax_mask_types::STANDARD_MATERIAL_FLAGS_DEPTH_MAP_BIT) != 0u && (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS) == parallax_mask_types::STANDARD_MATERIAL_FLAGS_ALPHA_MODE_MASK && (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u {
        let unshifted_standard = standard_material(in.instance_index);
        let unshifted_uv = (unshifted_standard.uv_transform * vec3(in.uv, 1.0)).xy;
        var unshifted_alpha = unshifted_standard.base_color.a * base_color_texel(in.instance_index, unshifted_uv, view.mip_bias).a;
        @if(NVF_PBR_PARALLAX_MASK && !PREPASS_PIPELINE && VERTEX_UVS_A && VERTEX_COLORS) unshifted_alpha *= vin.color.a;
        pbr_input.material.base_color.a = unshifted_alpha;
    }
    // NVF_PBR_VIEW (nvf-assets authored.rs `PbrView`, a debug view, off unless NVF_PBR_VIEW is
    // set at the start): one material input, taken here, right after the standard material's
    // sampling and before the mesh terrain blend, and written over the colour at the end.
    @if(NVF_PBR_VIEW && !PREPASS_PIPELINE) var pbr_view = vec3(0.0);
    @if(NVF_PBR_VIEW_ROUGHNESS && !PREPASS_PIPELINE) pbr_view = vec3(pbr_input.material.perceptual_roughness);
    @if(NVF_PBR_VIEW_METALLIC && !PREPASS_PIPELINE) pbr_view = vec3(pbr_input.material.metallic);
    @if(NVF_PBR_VIEW_OCCLUSION && !PREPASS_PIPELINE) pbr_view = vec3(1.0);
    @if(NVF_PBR_VIEW_OCCLUSION && !PREPASS_PIPELINE && VERTEX_UVS_A) if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_OCCLUSION_TEXTURE_BIT) != 0u {
        // The ORM's red alone: pbr_input.diffuse_occlusion may hold the screen-space occlusion.
        let occlusion_uv = (standard_material(in.instance_index).uv_transform * vec3(in.uv, 1.0)).xy;
        pbr_view = vec3(occlusion_texel(in.instance_index, occlusion_uv, view.mip_bias));
    }
    @if(NVF_PBR_VIEW_NORMAL && !PREPASS_PIPELINE) pbr_view = 0.5 + 0.5 * pbr_input.N;
    // The height map as stored (the original height, not the inverted depth the march
    // reads), at the unshifted coordinates; dark blue where the material has none.
    @if(NVF_PBR_VIEW_HEIGHT && !PREPASS_PIPELINE) pbr_view = vec3(0.0, 0.0, 0.3);
    @if(NVF_PBR_VIEW_HEIGHT && !PREPASS_PIPELINE && VERTEX_UVS_A) if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_DEPTH_MAP_BIT) != 0u {
        let height_uv = (standard_material(in.instance_index).uv_transform * vec3(in.uv, 1.0)).xy;
        pbr_view = vec3(height_texel(in.instance_index, height_uv, view.mip_bias));
    }
    @if(NVF_PBR_VIEW_AUTHORED && !PREPASS_PIPELINE) {
        let view_brightness = 0.25 + 0.75 * dot(pbr_input.material.base_color.rgb, vec3(0.2126, 0.7152, 0.0722));
        pbr_view = select(vec3(0.5), vec3(1.0, 0.45, 0.05), surface.emissive_rule.w > 0.5) * view_brightness;
    }
    // reflection: as `authored` (grey, orange where no cube is read), and an authored surface
    // of a view with a cube shows what a white perfect mirror there would write.
    @if(NVF_PBR_VIEW_REFLECTION && !PREPASS_PIPELINE) {
        let reflection_brightness = 0.25 + 0.75 * dot(pbr_input.material.base_color.rgb, vec3(0.2126, 0.7152, 0.0722));
        pbr_view = select(vec3(0.5), vec3(1.0, 0.45, 0.05), surface.emissive_rule.w > 0.5) * reflection_brightness;
        @if(ENVIRONMENT_MAP && NVF_PBR_REFLECT) if surface.emissive_rule.w > 0.5 && nvf_view_has_cube() { pbr_view = nvf_reflection_view(pbr_input.N, pbr_input.V); }
    }
    @if(FACEGEN_TINT && VERTEX_UVS_A) if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u {
        // base_color is material colour × vertex colour × texel: the texel is read again
        // as pbr_input_from_standard_material reads it, and its tinted value takes its
        // place in that product.
        let standard = standard_material(in.instance_index);
        let uv = (standard.uv_transform * vec3(in.uv, 1.0)).xy;
        let texel = base_color_texel(in.instance_index, uv, view.mip_bias).rgb;
        var factor = standard.base_color.rgb;
        @if(FACEGEN_TINT && VERTEX_UVS_A && VERTEX_COLORS) factor *= vin.color.rgb;
        let tinted = facegen_tint(texel, facegen_offset_at(in.instance_index, uv));
        pbr_input.material.base_color = vec4(pbr_input.material.base_color.rgb + factor * (tinted - texel), pbr_input.material.base_color.a);
    }
    @if(PIPBOY_SCREEN && VERTEX_UVS_A) if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u {
        // NifSurface::pipboy_screen: base_color is material colour x vertex colour x texel;
        // the texel's place in that product is taken by the screen's picture as it is now
        // (pipboy_picture), as FACEGEN_TINT's above.
        let standard = standard_material(in.instance_index);
        var factor = standard.base_color.rgb;
        @if(PIPBOY_SCREEN && VERTEX_UVS_A && VERTEX_COLORS) factor *= vin.color.rgb;
        let picture = pipboy_picture(in.instance_index, screen_uv, screen_dx, screen_dy, surface.pipboy_a, surface.pipboy_b, in.position.xy);
        pbr_input.material.base_color = vec4(factor * picture, pbr_input.material.base_color.a);
    }
    @if(REFRACT) var tilt = (view.view_from_world * vec4(pbr_input.N, 0.0)).xy;
    @if(REFRACT && STANDARD_MATERIAL_NORMAL_MAP && VERTEX_UVS_A) if surface.fire_period != 0.0 {
        // Fire refraction (nif_material.rs "Fire refraction"): the normal map read
        // scrolled up its texture once every `fire_period` seconds (down when negative:
        // fract keeps the offset in 0..1 either way); its tangent-space tilt is added to
        // the tilt of the vertex normal, in place of the unscrolled map's.
        let scrolled = in.uv + vec2(0.0, fract(globals.time / surface.fire_period));
        tilt = (view.view_from_world * vec4(pbr_input.world_normal, 0.0)).xy;
        tilt += normal_map_texel(in.instance_index, scrolled).rg * 2.0 - 1.0;
    }
    // A cut-out (hair, a refracting actor's alpha-tested parts) keeps its outline.
    @if(REFRACT) if pbr_input.material.base_color.a < surface.alpha_cut {
        discard;
    }
    @if(REFRACT) let shifted = clamp(frag_coord_to_uv(in.position.xy) + vec2(tilt.x, -tilt.y) * surface.refraction, vec2(0.0), vec2(1.0));
    @if(REFRACT) var refracted: FragmentOutput;
    @if(REFRACT) refracted.color = vec4(textureSampleLevel(view_transmission_texture, view_transmission_sampler, transmission_uv(shifted), 0.0).rgb, 1.0);
    @if(REFRACT) return refracted;
    // |N·V|: the normal is already flipped towards the viewer on back faces of double-sided
    // shapes; abs() also covers single-sided ones seen edge-on.
    let cos_angle = abs(dot(normalize(pbr_input.world_normal), pbr_input.V));
    @if(PREPASS_PIPELINE) let falloff = falloff_opacity(cos_angle, surface.falloff);
    @if(!PREPASS_PIPELINE) let falloff = falloff_opacity(cos_angle, surface.falloff) * soft_fade(in.position, pbr_input.world_position.xyz, surface.soft.x);
    pbr_input.material.base_color.a *= falloff;
    if pbr_input.material.base_color.a < surface.alpha_cut {
        discard;
    }
    // Alpha-tested shape in a multisampled view (nif_material.rs `soft_edges`): the share
    // of the pixel's samples the shape covers, 1 at the cutoff and above, falling to 0
    // within a pixel outside it.
    @if(SOFT_EDGES) let edge_alpha = pbr_input.material.base_color.a;
    @if(SOFT_EDGES) let edge = saturate((edge_alpha - pbr_input.material.alpha_cutoff) / max(fwidth(edge_alpha), 0.0001) + 1.0);
    @if(SOFT_EDGES) if edge <= 0.0 {
        discard;
    }
    @if(SOFT_EDGES) pbr_input.material.base_color.a = 1.0;
    // PREPASS_COVERAGE (nif_material.rs `soft_edges`): the depth prepass has written the
    // shape, soft edge included, on the samples it covers, and the depth test is for
    // equality with it: nothing is left to test here.
    @if(PREPASS_COVERAGE) pbr_input.material.base_color.a = 1.0;
    @if(!SOFT_EDGES && !PREPASS_COVERAGE) pbr_input.material.base_color = alpha_discard(pbr_input.material.flags, pbr_input.material.alpha_cutoff, pbr_input.material.base_color);
    // [design] Mesh terrain blend (nif_surface_blend.rs; the mesh-terrain-blend settings row),
    // forward only: after the existing coverage tests and normal mapping, before lighting.
    // The weight comes from the unperturbed world_normal; alpha, positions, depth and that
    // geometric normal (shadow bias) stay as they are. Full shading moves the material
    // values the lighting reads towards the landscape's (terrain.rs `surface`), so the
    // mesh's normal-map creases, specular and emission fade out where the weight is 1.
    // The weight, normal and status views are drawn as unlit colour without fog (no early
    // return: the lighting code after it keeps uniform control flow), their emission
    // cleared below.
    @if(MESH_TERRAIN_BLEND && !PREPASS_PIPELINE) let terrain_unlit = mesh_terrain_unlit_view(mesh_terrain_view(in.instance_index));
    // [design] Round 10a: the lookup's inputs, with the game-xy derivatives captured at the
    // top of nif_fragment (before any discard), passed in for the lookup's branches; #2661
    // review: the face's steepness from the world-position derivatives captured there.
    @if(MESH_TERRAIN_BLEND && !PREPASS_PIPELINE) let terrain_inputs = MeshTerrainInputs(distance(view.world_position, in.world_position.xyz), terrain_dx, terrain_dy, view.mip_bias, mesh_terrain_shading(in.instance_index), mesh_terrain_face_up(terrain_wdx, terrain_wdy, pbr_input.world_normal));
    // [design] Reflections (NVF_PBR_REFLECT, below): the mesh's share of the reflection, 1
    // unless full mesh terrain shading moves the material towards the landscape's.
    @if(!PREPASS_PIPELINE && NVF_PBR_REFLECT) var reflect_share = 1.0;
    @if(MESH_TERRAIN_BLEND && !PREPASS_PIPELINE) {
        let terrain = mesh_terrain_lookup(in.instance_index, in.world_position.xyz, pbr_input.world_normal, terrain_inputs);
        let terrain_view = mesh_terrain_view(in.instance_index);
        if terrain_unlit {
            pbr_input.material.base_color = vec4(mesh_terrain_diagnostic(terrain_view, terrain, pbr_input.material.base_color.rgb), pbr_input.material.base_color.a);
            pbr_input.material.flags = (pbr_input.material.flags | STANDARD_MATERIAL_FLAGS_UNLIT_BIT) & ~STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT;
        } else if terrain_view == 3u {
            pbr_input.material.base_color = vec4(mesh_terrain_diagnostic(terrain_view, terrain, pbr_input.material.base_color.rgb), pbr_input.material.base_color.a);
        } else if terrain.status == LOOKUP_SUCCESS && terrain.weight > 0.0 {
            let w = terrain.weight;
            pbr_input.material.base_color = vec4(mix(pbr_input.material.base_color.rgb, mesh_terrain_target_colour(terrain), w), pbr_input.material.base_color.a);
            if mesh_terrain_shading(in.instance_index) == 1u {
                pbr_input.N = mesh_terrain_blend_normal(pbr_input.N, mesh_terrain_target_normal(terrain), w);
                pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, LAND_ROUGHNESS, w);
                pbr_input.material.reflectance = mix(pbr_input.material.reflectance, vec3(LAND_REFLECTANCE), w);
                pbr_input.material.metallic = mix(pbr_input.material.metallic, 0.0, w);
                pbr_input.material.emissive = vec4(pbr_input.material.emissive.rgb * (1.0 - w), pbr_input.material.emissive.a);
                pbr_input.material.clearcoat *= 1.0 - w;
                pbr_input.material.anisotropy_strength *= 1.0 - w;
                pbr_input.anisotropy_strength *= 1.0 - w;
                // The reflection fades with the mesh's share, so the dielectric sheen is gone
                // at w = 1 as well as the metal.
                @if(NVF_PBR_REFLECT) reflect_share = 1.0 - w;
            }
        }
    }
    // [design] Reflections on authored materials (#2981 reflections, slice 1; nvf_assets
    // reflection_cube.rs). NVF_PBR_REFLECT is set on every NIF main-pass pipeline while the
    // run's switch is on; ENVIRONMENT_MAP is the view's (bevy_pbr sets it once the view's
    // EnvironmentMapLight has both its images on the GPU, which reflection_cube.rs attaches
    // only after the cube's content is written). An authored material's metallic is the
    // ORM's blue (NVF_PBR_VIEW=metallic, taken above, shows it); a view without a cube holds
    // it at 0, so the water mirror, the scope, the minigame views and the first frames before
    // the cube shade it exactly as with the switch off. A view that has ENVIRONMENT_MAP only
    // for a light-probe volume has no cube of its own (nvf_view_has_cube) and holds it too.
    @if(!PREPASS_PIPELINE && NVF_PBR_REFLECT && !ENVIRONMENT_MAP) if surface.emissive_rule.w > 0.5 {
        pbr_input.material.metallic = 0.0;
    }
    @if(!PREPASS_PIPELINE && NVF_PBR_REFLECT && ENVIRONMENT_MAP) if surface.emissive_rule.w > 0.5 && !nvf_view_has_cube() {
        pbr_input.material.metallic = 0.0;
    }
    // A view with a cube: its reflection (bevy_pbr pbr_functions' environment blocks, compiled
    // for this fragment only by nvforever lights/reflection_gate.rs) is weighted by
    // specular_occlusion, set here for both lighting calls below (the second is the
    // first-person shadow correction, with_shade_light): the authored mark (every other
    // material gets 0, and the gate then skips the cube lookup), the ORM's occlusion (red;
    // pbr_input.diffuse_occlusion holds it alone, Bevy's screen-space occlusion being unused)
    // and the mesh's share under the mesh terrain blend.
    @if(!PREPASS_PIPELINE && NVF_PBR_REFLECT && ENVIRONMENT_MAP) {
        let reflect_weight = select(0.0, reflect_share * pbr_input.diffuse_occlusion.x, surface.emissive_rule.w > 0.5 && nvf_view_has_cube());
        pbr_input.specular_occlusion = pbr_input.specular_occlusion * reflect_weight;
    }
    // base_color already holds texture × vertex colour × material colour.
    // Bevy's pbr_input_from_standard_material fills the emissive light in only for lit
    // materials, so an unlit shape's stays black: it is read from the material here
    // (without the glow map, which unlit effect shapes do not use).
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) != 0u {
        pbr_input.material.emissive = standard_material(in.instance_index).emissive;
    }
    // The image space's Emissive Mult; not on additive shapes (light beams, glow cards):
    // the Strip's night adapter raises it to 4 (×2.67 over the Mojave's), and the
    // original's Strip lamp beams stay a faint haze under it where ours, scaled in
    // display values, blew out white (nif.md §6 "Additive blend", 2026-10-25).
    // Unlit shapes without External Emittance and authored cut-out signs also bypass the scale [hypothesis, fitted
    // to the 2026-11-06 frames; nif.md §6 "Unlit shapes that are pictures, and cut-out signs"].
    let emissive_scale = select(view_emissive_scale(), 1.0, surface.emissive_rule.x > 0.5 || (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS) == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_ADD);
    let e = vec4(pbr_input.material.emissive.rgb * emissive_scale, pbr_input.material.emissive.a);
    pbr_input.material.emissive = vec4(e.rgb * pbr_input.material.base_color.rgb, e.a);
    // [design] Metal base colour (#2981; docs/spec-notes/pbr-materials.md "Metal base colour"):
    // in a view with a cube an authored material's base colour for both lighting calls is
    // lifted by its set's metal gain (NifSurface::metal_gain in the uniform's terrain.y;
    // nvf_metal_base), by the effective metallic (after the per-view hold and the terrain
    // mix); the glow above keeps the unlifted colour, alpha is unchanged. One line, so the
    // stripped-source comparison takes it out whole.
    @if(NVF_PBR_REFLECT && ENVIRONMENT_MAP && !PREPASS_PIPELINE) if surface.emissive_rule.w > 0.5 && nvf_view_has_cube() { pbr_input.material.base_color = vec4(nvf_metal_base(pbr_input.material.base_color.rgb, surface.terrain.y, pbr_input.material.metallic), pbr_input.material.base_color.a); }
    @if(MESH_TERRAIN_BLEND && !PREPASS_PIPELINE) if terrain_unlit { pbr_input.material.emissive = vec4(vec3(0.0), e.a); }

    // Kept out of the lighting sum: multiply_factor adds it in display values.
    @if(BLEND_MULTIPLY) let multiply_emissive = pbr_input.material.emissive.rgb;
    @if(BLEND_MULTIPLY) pbr_input.material.emissive = vec4(vec3(0.0), e.a);

    @if(PREPASS_PIPELINE) let out = deferred_output(in, pbr_input);
    @if(!PREPASS_PIPELINE) var out: FragmentOutput;
    @if(!PREPASS_PIPELINE) if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        // Hot light source (hot_light): the emitted light is added after the lighting.
        @if(!PREPASS_PIPELINE && !TONEMAP_IN_SHADER && !BLEND_MULTIPLY) let hot_e = pbr_input.material.emissive.rgb;
        @if(!PREPASS_PIPELINE && !TONEMAP_IN_SHADER && !BLEND_MULTIPLY) pbr_input.material.emissive = vec4(vec3(0.0), pbr_input.material.emissive.a);
        out.color = apply_pbr_lighting(pbr_input);
        @if(!PREPASS_PIPELINE && SHADE_LIGHT) out.color = with_shade_light(pbr_input, out.color);
        // Bevy's ambient specular, with the same inputs as in apply_pbr_lighting and
        // bevy_pbr::ambient::ambient_light (no lightmaps are used here). Not under Physical
        // lighting, which keeps the physical specular image-based light.
        @if(!NVF_PHYSICAL_PBR) {
            let m = pbr_input.material;
            let F0 = calculate_F0(m.base_color.rgb, m.metallic, m.reflectance);
            let NdotV = max(dot(pbr_input.N, pbr_input.V), 0.0001);
            let spec = EnvBRDFApprox(F0, F_AB(m.perceptual_roughness, NdotV));
            let spec_occlusion = saturate(dot(F0, vec3(50.0 * 0.33)));
            var ambient_spec = spec * spec_occlusion * lights.ambient_color.rgb * pbr_input.diffuse_occlusion;
            if lights.ambient_color.a < 0.0 {
                // A world view (nvforever lights.rs DISPLAY_HELPERS): the highlight Bevy added
                // took the ambient colour as linear light (`nvf_ambient_ratio`).
                ambient_spec *= pow(max(lights.ambient_color.rgb * view.exposure, vec3(1e-6)), vec3(1.2));
            }
            // Removed in full, also from a metal (metallic 1 with reflections on): the reflection
            // cube's light, where the view has one, takes the highlight's place (#2981 reflections).
            @if(!NVF_PBR_METALLIC) out.color = vec4(max(out.color.rgb - view.exposure * ambient_spec, vec3(0.0)), out.color.a);
            // NVF_PBR_METALLIC (nvf-assets authored.rs `metallic_debug`, a debug switch, not set
            // while reflections are on, which retire it): on a
            // material shaded from an authored set (emissive_rule.w), only the dielectric share
            // (1 - m) of the highlight is removed, m the sampled metallic after the terrain mix,
            // so a metal keeps its ambient highlight; m = 0 removes it all, as above. Other
            // materials remove it all.
            @if(NVF_PBR_METALLIC) let removed = select(1.0, 1.0 - saturate(m.metallic), surface.emissive_rule.w > 0.5);
            @if(NVF_PBR_METALLIC) out.color = vec4(max(out.color.rgb - view.exposure * ambient_spec * removed, vec3(0.0)), out.color.a);
        }
        // The lit colour without the hot light, for the environment map's received-light
        // debug switch below.
        @if(!NIF_SURFACE_BLEND) let lit_rgb = out.color.rgb;
        @if(!PREPASS_PIPELINE && !TONEMAP_IN_SHADER && !BLEND_MULTIPLY && !NVF_PHYSICAL_PBR) out.color = vec4(hot_light(out.color.rgb, hot_e), out.color.a);
        @if(!PREPASS_PIPELINE && !TONEMAP_IN_SHADER && !BLEND_MULTIPLY && NVF_PHYSICAL_PBR) out.color = vec4(out.color.rgb + hot_e, out.color.a);
        // Environment map (nif_material.rs NifSurface::env, env_how; docs/spec-notes/nif.md
        // §6 "Window glass and environment maps", "Environment maps on other shapes"): the
        // cube read in the view direction reflected about the normal (`env_lookup`), × the scale × the
        // mask × the distance fade, added to the lit colour. The game is Z-up and the
        // cube's faces are laid out Y-up: the game-space direction (x, y, z) is looked up
        // as (x, z, y), which from Bevy's Y-up world space (x, z, -y of the game) is
        // (x, y, -z) [hypothesis, window shapes]. env_how.x 4, every other shape by default
        // (#4392): the game-space direction as it is, Bevy's (x, -z, y) [hypothesis, from the
        // original's picture of the Goodsprings camper]; 1 flip x, 2 flip y, 3 flip z are
        // debug switches (env_map.rs Axes). A premultiplied
        // shape (a blended window shape that reflects) has its colour multiplied by its
        // alpha first, so the reflection lies over the blend whatever the alpha; a blended
        // shape is weighted by its alpha, reflection included [hypothesis]. Not in the blend
        // type (nif_surface_blend.rs): it has no environment bindings, and a shape that
        // reflects is never given it (nvforever mesh_terrain_blend.rs `material_reasons`).
        // Distance fade, under the debug switch NVF_PLAY_DEBUG_ENV_DISTANCE_FADE=1 only (off
        // by default: env z = w = 0; env_map.rs ENV_MAP_LOD1, ENV_MAP_LOD2: fEnvMapLOD1/2), linear
        // between the two by the distance from the camera to the shape node's origin (the
        // mesh instance's translation, usually the reference's; a model whose parts sit
        // under nodes of their own fades part by part) [hypothesis], in game units (the
        // world's units); worked out first, so a faded-out shape reads neither its cube
        // nor its mask.
        @if(!NIF_SURFACE_BLEND) var env_fade = 1.0;
        @if(!NIF_SURFACE_BLEND) if surface.env.x > 0.0 && surface.env.w > surface.env.z {
            let d = distance(view.world_position, get_world_from_local(in.instance_index)[3].xyz);
            env_fade = 1.0 - saturate((d - surface.env.z) / (surface.env.w - surface.env.z));
        }
        // A premultiplied environment-mapped shape (a blended window shape, or another under
        // NVF_PLAY_DEBUG_ENV_BLEND=premultiplied; env_how.w marks every env shape) has its
        // colour multiplied by its alpha whether or not it still reflects: Bevy leaves a
        // Premultiplied material's colour as it is and blends it ONE / 1 - alpha, so a
        // faded-out or zero-strength pane would otherwise draw at its full texture colour.
        @if(!NIF_SURFACE_BLEND) if surface.env_how.w > 0.5 {
            let premultiplied = (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS) == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_PREMULTIPLIED;
            out.color = vec4(select(out.color.rgb, out.color.rgb * out.color.a, premultiplied), out.color.a);
        }
        @if(!NIF_SURFACE_BLEND) if surface.env.x > 0.0 && env_fade > 0.0 {
            let dir = env_lookup(pbr_input.V, pbr_input.N, pbr_input.world_normal, surface.env_how.x, surface.env_how.w);
            var mask = 1.0;
            @if(VERTEX_UVS_A) if surface.env.y > 0.5 && surface.env.y < 1.5 {
                mask = env_mask_texel(in.instance_index, (standard_material(in.instance_index).uv_transform * vec3(in.uv, 1.0)).xy);
            }
            @if(VERTEX_UVS_A) if surface.env.y > 1.5 {
                mask = env_mask_alpha(in.instance_index, (standard_material(in.instance_index).uv_transform * vec3(in.uv, 1.0)).xy);
            }
            var reflection = env_cube_texel(in.instance_index, dir) * surface.env.x * mask * env_fade;
            // × the scene light (env_how.y 2, the default for shapes without the window
            // flag; env_scene_light above).
            if surface.env_how.y > 1.5 {
                reflection *= env_scene_light();
            }
            // Debug switch NVF_PLAY_DEBUG_ENV_LIGHT=all|fade-flag (env_map.rs; env_how.y 1):
            // × the light the fragment receives, its lit colour's luminance over its base
            // colour's, at most 1. A rough stand-in for comparing frames, not a model of
            // the original.
            if surface.env_how.y > 0.5 && surface.env_how.y < 1.5 {
                let luma_weights = vec3(0.2126, 0.7152, 0.0722);
                reflection *= saturate(dot(lit_rgb, luma_weights) / max(dot(pbr_input.material.base_color.rgb, luma_weights), 0.02));
            }
            // Physical lighting: the received light and a Schlick-shaped Fresnel weight
            // (glass steeper than other shapes) [design, tuned by eye on #3828's branch].
            @if(NVF_PHYSICAL_PBR) if surface.env_how.z < 0.5 {
                let luma_w = vec3(0.2126, 0.7152, 0.0722);
                let recv = clamp(dot(lit_rgb, luma_w) / max(dot(pbr_input.material.base_color.rgb, luma_w), 0.04), 0.02, 1.0);
                let ndotv = max(dot(pbr_input.N, pbr_input.V), 0.0001);
                let fresnel_w = select(0.35 + 0.65 * pow(1.0 - ndotv, 3.0), 0.08 + 0.92 * pow(1.0 - ndotv, 4.0), surface.env.y < 0.5);
                reflection *= recv * fresnel_w;
            }
            out.color = vec4(out.color.rgb + reflection, out.color.a);
        }
    } else {
        out.color = pbr_input.material.base_color;
        // An unlit shape with a non-black emissive colour is drawn at its emissive light:
        // surface colour × emissive colour (a display colour, nif_spawn.rs
        // `shape_emissive`) × emissive multiplier, so a warm colour tints a white glow
        // texture (Dead Money's lantern glow) and neon authored at multiplier 25
        // (Gomorrah's tubes) burns far brighter than white and glows in an HDR view, as in
        // the original's frames. A black emissive colour leaves the surface colour
        // [hypothesis from the original's frames, nif.md §6 "Emissive colour of unlit
        // shapes"]. Without the light glow (tonemapped in the shader) it is never
        // brighter than the surface colour.
        @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY) let unlit_e = pbr_input.material.emissive.rgb;
        @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY) if any(unlit_e > vec3(0.0)) {
            @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY && TONEMAP_IN_SHADER) out.color = vec4(min(unlit_e, out.color.rgb), out.color.a);
            // Opaque solid emitters burn as hot sources; authored cut-out signs (emissive_rule.y)
            // keep their texture on the plain path [hypothesis, nif.md §6 "Unlit shapes that
            // are pictures, and cut-out signs"];
            // glows and beams that blend or add over the frame stay soft (hot_light), or a
            // lamp's glow card burns into a hard disc on the wall behind it.
            @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY && !TONEMAP_IN_SHADER) let unlit_blend = pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS;
            @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY && !TONEMAP_IN_SHADER) if surface.emissive_rule.y > 0.0 || unlit_blend == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_ADD || unlit_blend == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_BLEND || unlit_blend == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_PREMULTIPLIED {
                out.color = vec4(unlit_e, out.color.a);
            } else {
                out.color = vec4(hot_light(vec3(0.0), unlit_e), out.color.a);
            }
        }
    }
    @if(!PREPASS_PIPELINE) out.color = vec4(mix(out.color.rgb, surface.holes_edge.rgb, holes_band), out.color.a);
    // Physical lighting: a blended unlit colour is scaled before the fog and tonemapping
    // (the original-look order scales it after them, below).
    @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY && NVF_PHYSICAL_PBR) let blend_mode = pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS;
    @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY && NVF_PHYSICAL_PBR) if blend_mode == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_BLEND && (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) != 0u {
        out.color = vec4(out.color.rgb * blend_colour_scale(out.color.a), out.color.a);
    }
    // The colour is a factor, not light: no fog and no tonemapping. Bevy's multiply blend
    // is destination × colour + destination × (1 − alpha), so the colour is premultiplied
    // as bevy_pbr::pbr_functions::premultiply_alpha does. Under Physical lighting the
    // factor goes towards 1 with the fog's transmittance, so grime and occlusion decals
    // recede behind fog instead of showing through it.
    @if(!PREPASS_PIPELINE && BLEND_MULTIPLY) var factor = multiply_factor(out.color.rgb, multiply_emissive);
    @if(!PREPASS_PIPELINE && BLEND_MULTIPLY && DISTANCE_FOG && NVF_PHYSICAL_PBR) if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT) != 0u {
        let fog_black = apply_fog(fog, vec4(0.0, 0.0, 0.0, 1.0), pbr_input.world_position.xyz, view.world_position.xyz, pbr_input.frag_coord.xy).rgb;
        let fog_white = apply_fog(fog, vec4(1.0, 1.0, 1.0, 1.0), pbr_input.world_position.xyz, view.world_position.xyz, pbr_input.frag_coord.xy).rgb;
        let transmittance = saturate(fog_white - fog_black);
        factor = mix(vec3(1.0), factor, transmittance);
    }
    @if(!PREPASS_PIPELINE && BLEND_MULTIPLY) out.color = vec4(factor * out.color.a, out.color.a);
    @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY && BLEND_PREMULTIPLIED_ALPHA) if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS) == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_ADD {
        // Additive: fog (and, in views without HDR, the tonemapping) on the colour at
        // alpha 1, which Bevy's premultiplying leaves as it is with alpha 0; then the
        // display-value sum over the opaque frame behind (the anchor in nif_material.rs
        // keeps Bevy's copy of it in every view). A ONE / ONE shape adds its colour whole,
        // faded only by the view-angle falloff.
        let add_a = select(out.color.a, falloff, surface.add_whole > 0.5);
        // Fog takes the added light away and adds no fog colour of its own: the frame
        // behind is already fogged. Bevy's fog is colour × k + fog term, so the added
        // light is the fogged colour less the fogged black (nif_material.rs
        // `fogged_addition`; nif.md §6 "Additive blend", "Fog on added light", #2435).
        // Fogged as a surface, black vertex colour still added the room's fog colour, and
        // a ONE / ONE glow card with no texture (Gomorrah's wall sconces) was a flat,
        // hard-edged board of fog colour.
        let fogged_black = main_pass_post_lighting_processing(pbr_input, vec4(0.0, 0.0, 0.0, 1.0));
        let fogged = vec4(fogged_addition(main_pass_post_lighting_processing(pbr_input, vec4(out.color.rgb, 1.0)).rgb, fogged_black.rgb), 1.0);
        @if(!ADD_LINEAR) let behind = textureSampleLevel(view_transmission_texture, view_transmission_sampler, transmission_uv(frag_coord_to_uv(in.position.xy)), 0.0).rgb;
        @if(!ADD_LINEAR) out.color = vec4(add_display(fogged.rgb, add_a, behind), 0.0);
        // NifSurface::add_linear: the colour added as it is, in linear light.
        @if(ADD_LINEAR) out.color = vec4(fogged.rgb * saturate(add_a), 0.0);
    } else {
        out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    }
    // NifSurface::pipboy_screen: linear red's ceiling, fitted so a bar's red stays near the
    // original's 206 (nvf-play pipboy/device_screen.rs). Before the post-lighting step, which
    // tone maps here when the view has no HDR (the Glow setting off).
    @if(PIPBOY_SCREEN) out.color.r = min(out.color.r, 0.67);
    @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY && !BLEND_PREMULTIPLIED_ALPHA) out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY && !NVF_PHYSICAL_PBR) let blend_mode = pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS;
    @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY && !NVF_PHYSICAL_PBR) if blend_mode == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_BLEND && (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) != 0u {
        out.color = vec4(out.color.rgb * blend_colour_scale(out.color.a), out.color.a);
    }
    // NifSurface::pipboy_screen: the mark that keeps these pixels out of the image-space grade
    // (imagespace.wesl SCREEN_MARK).
    @if(PIPBOY_SCREEN) out.color.a = 0.25;
    @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY && SOFT_EDGES) out.color.a = edge;
    // NVF_PBR_VIEW: the input taken above in place of the colour; alpha, discards, coverage
    // and depth as drawn. A premultiplied (additive) draw adds it by its alpha, a multiplied
    // one by white: effects fade out of the view.
    @if(NVF_PBR_VIEW && !PREPASS_PIPELINE && !BLEND_MULTIPLY && !BLEND_PREMULTIPLIED_ALPHA) out.color = vec4(pbr_view, out.color.a);
    @if(NVF_PBR_VIEW && !PREPASS_PIPELINE && !BLEND_MULTIPLY && BLEND_PREMULTIPLIED_ALPHA) out.color = vec4(pbr_view * out.color.a, out.color.a);
    @if(NVF_PBR_VIEW && !PREPASS_PIPELINE && BLEND_MULTIPLY) out.color = vec4(vec3(out.color.a), out.color.a);
    return out;
}
