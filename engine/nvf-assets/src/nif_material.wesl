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
//   from lights (and environment maps, not drawn yet);
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
// PIPBOY_SCREEN (the Pip-Boy's screen with the menus on it, main pass; NifSurface::
// pipboy_screen): the texel is the menus' picture as `pipboy_picture` makes it (rolled,
// softened, washed, with noise; NifSurface::pipboy_a, pipboy_b), red stops at a ceiling and
// the pixels are marked for the image-space grade.

import bevy_pbr::{
    render::pbr_fragment::pbr_input_from_standard_material,
    render::pbr_functions::alpha_discard,
};
@if(VISIBILITY_RANGE_DITHER) import bevy_pbr::render::pbr_functions::visibility_range_dither;

@if(PREPASS_PIPELINE) import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@if(!PREPASS_PIPELINE) import bevy_pbr::{
    render::forward_io::{VertexOutput, FragmentOutput},
    render::pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing, calculate_F0},
    render::pbr_types::{STANDARD_MATERIAL_FLAGS_UNLIT_BIT, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_ADD, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_BLEND, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_PREMULTIPLIED, STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT},
    render::pbr_lighting::{EnvBRDFApprox, F_AB},
    render::mesh_view_bindings::{lights, view, globals, view_transmission_texture, view_transmission_sampler, fog},
    render::mesh_view_types::FOG_MODE_ATMOSPHERIC,
    render::view_transformations::{position_ndc_to_world, depth_ndc_to_view_z},
};
// The deferred G-buffer pass (Solari, nvforever solari.rs) reads the same flags.
@if(PREPASS_PIPELINE) import bevy_pbr::render::pbr_types::{STANDARD_MATERIAL_FLAGS_UNLIT_BIT, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_ADD, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_BLEND, STANDARD_MATERIAL_FLAGS_ALPHA_MODE_PREMULTIPLIED, STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT};
@if(!PREPASS_PIPELINE && DEPTH_PREPASS) import bevy_pbr::prepass::utils as prepass_utils;
@if(!PREPASS_PIPELINE) import nvf_assets::first_person_depth;
@if(!PREPASS_PIPELINE && SHADE_LIGHT) import bevy_pbr::{
    render::pbr_types::PbrInput,
    render::mesh_types::MESH_FLAGS_SHADOW_RECEIVER_BIT,
    render::mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    render::shadows::{fetch_directional_shadow, get_cascade_index},
    render::shadow_sampling::sample_shadow_map,
};

import nvf_assets::nif_surface_bindings::{NifSurface, nif_surface, standard_material, holes_alpha, facegen_size, facegen_texel, base_color_texel, base_color_texel_level, base_color_size, normal_map_texel, env_mask_value};
@if(SCREEN_INDIRECT) import bevy_pbr::render::mesh_view_bindings::{screen_indirect_texture, screen_probe_faces, screen_probe_sampler};

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
const TONY_N: u32 = 24u;
var<private> TONY_IN: array<f32, 24> = array<f32, 24>(0.0, 0.005, 0.01, 0.02, 0.04, 0.07, 0.1, 0.15, 0.2, 0.3, 0.4, 0.5, 0.6, 0.8, 1.0, 1.3, 1.7, 2.2, 3.0, 5.0, 8.0, 16.0, 40.0, 100.0);
var<private> TONY_OUT: array<f32, 24> = array<f32, 24>(0.0, 0.0056, 0.0113, 0.0222, 0.0437, 0.0723, 0.0999, 0.1470, 0.1812, 0.2542, 0.3140, 0.3663, 0.4125, 0.4910, 0.5457, 0.6172, 0.6867, 0.7454, 0.8070, 0.8796, 0.9216, 0.9473, 0.9771, 0.9875);

fn tony_inverse(y: f32) -> f32 {
    for (var i = 1u; i < TONY_N; i++) {
        if y <= TONY_OUT[i] {
            let t = (y - TONY_OUT[i - 1u]) / max(TONY_OUT[i] - TONY_OUT[i - 1u], 1e-6);
            return mix(TONY_IN[i - 1u], TONY_IN[i], t);
        }
    }
    return TONY_IN[TONY_N - 1u];
}

// `lit` plus the emitted light `e` as a hot source: held per channel, mapped through
// the inverse response. Only light that goes past white is a hot source: blended in as
// the emitted light's brightest channel rises from 0.8 to 1.5, so faint effect shapes
// (window light rays, white × 1 under a warm emittance colour) and the dark parts of a
// glow map keep the tonemapper; the lantern glass (× 10) and neon (× 25) burn.
fn hot_light(lit: vec3<f32>, e: vec3<f32>) -> vec3<f32> {
    let d = min(lit + e, vec3(HOT_MAX));
    let hot = vec3(tony_inverse(d.r), tony_inverse(d.g), tony_inverse(d.b));
    return mix(lit + e, hot, smoothstep(0.8, 1.5, max(e.r, max(e.g, e.b))));
}

// The linear light an additive shape (blend factors SRC_ALPHA or ONE, and ONE) of linear
// colour `colour` and alpha `a` adds over the opaque frame `behind` (linear; CPU mirror
// and the reasons: nif_material.rs `add_display`). The game adds colour × alpha to the
// frame's display values [hypothesis]: the sum is formed in display values (gamma 2.2),
// held to ADD_DISPLAY_MAX (2.0) or the frame's own value if brighter, and the difference
// to the frame returned as linear light.
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
// nor without a prepass. Called in uniform control flow (screen derivatives).
@if(!PREPASS_PIPELINE) fn soft_fade(frag_coord: vec4<f32>, world_pos: vec3<f32>, distance: f32) -> f32 {
    // A first-person shape's depth as it was before it was squeezed, so the fade is
    // measured against where it is (nif_material.rs "First-person depth").
    @if(!PREPASS_PIPELINE && DEPTH_PREPASS) let depth = first_person_depth::unsqueeze(prepass_utils::prepass_depth(frag_coord, 0u));
    @if(!PREPASS_PIPELINE && DEPTH_PREPASS) let scene = position_ndc_to_world(frag_coord_to_ndc(vec4(frag_coord.xy, max(depth, 1e-7), 1.0)));
    @if(!PREPASS_PIPELINE && DEPTH_PREPASS) let scene_n = cross(dpdx(scene), dpdy(scene));
    @if(!PREPASS_PIPELINE && DEPTH_PREPASS) let own_n = cross(dpdx(world_pos), dpdy(world_pos));
    @if(!PREPASS_PIPELINE && DEPTH_PREPASS) let lengths = length(scene_n) * length(own_n);
    @if(!PREPASS_PIPELINE && DEPTH_PREPASS) let along = select(0.0, abs(dot(scene_n, own_n)) / lengths, lengths > 1e-12);
    @if(!PREPASS_PIPELINE && DEPTH_PREPASS) let gap = depth_ndc_to_view_z(frag_coord.z) - depth_ndc_to_view_z(max(depth, 1e-7));
    @if(!PREPASS_PIPELINE && DEPTH_PREPASS) let fade = mix(saturate(gap / max(distance, 1e-3)), 1.0, smoothstep(0.85, 0.95, along));
    @if(!PREPASS_PIPELINE && DEPTH_PREPASS) return select(fade, 1.0, distance <= 0.0 || depth <= 0.0);
    @if(!PREPASS_PIPELINE && !DEPTH_PREPASS) return 1.0;
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
    let missing = (1.0 - own) + share * (1.0 - shade);
    if missing < 0.000001 {
        // Neither map has a shadow here.
        return shadowed;
    }
    let wanted = min(shade, shadow_past_body(sun, in, view_z));
    var unshadowed = in;
    unshadowed.flags &= ~MESH_FLAGS_SHADOW_RECEIVER_BIT;
    let lit = apply_pbr_lighting(unshadowed);
    let k = (1.0 + share - wanted) / missing;
    if lights.ambient_color.a < 0.0 {
        // The display-value sum (nvforever lights.rs DISPLAY_HELPERS): the diffuse light is
        // texture × sum^2.2, so its display value (^(1/2.2)) is linear in the shadow
        // values; the same algebra there (the specular part is taken along).
        let lit_d = pow(max(lit.rgb, vec3(0.0)), vec3(1.0 / 2.2));
        let shadowed_d = pow(max(shadowed.rgb, vec3(0.0)), vec3(1.0 / 2.2));
        return vec4(pow(max(lit_d - (lit_d - shadowed_d) * k, vec3(0.0)), vec3(2.2)), shadowed.a);
    }
    return lit - (lit - shadowed) * k;
}

@if(FIRST_PERSON_DEPTH) struct FirstPersonOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

// A first-person shape (nif_material.rs "First-person depth"): its colour, at its depth
// squeezed in front of the world.
// NVForever remaster (graphics-remaster.md §4 "Detail"): procedural micro-relief on the
// normal near the camera, where the 2010 textures run out of resolution: the gradient of
// two octaves of world-space value noise (cells of 9 and 3.5 game units), tilting the
// normal by up to about 0.3, faded out between 150 and 700 units [engineering choice].
// The same code is in terrain.wesl and nif_material.wesl. Integer hashing keeps it exact
// at the Mojave's coordinates (tens of thousands of units).
fn nvf_hash3(c: vec3<i32>) -> f32 {
    var h = (u32(c.x) * 0x8da6b343u) ^ (u32(c.y) * 0xd8163841u) ^ (u32(c.z) * 0xcb1ab31fu);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h & 0xffffu) / 65535.0;
}
fn nvf_noise3(p: vec3<f32>) -> f32 {
    let i = vec3<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = mix(mix(nvf_hash3(i), nvf_hash3(i + vec3(1, 0, 0)), u.x), mix(nvf_hash3(i + vec3(0, 1, 0)), nvf_hash3(i + vec3(1, 1, 0)), u.x), u.y);
    let b = mix(mix(nvf_hash3(i + vec3(0, 0, 1)), nvf_hash3(i + vec3(1, 0, 1)), u.x), mix(nvf_hash3(i + vec3(0, 1, 1)), nvf_hash3(i + vec3(1, 1, 1)), u.x), u.y);
    return mix(a, b, u.z);
}
fn nvf_relief(p: vec3<f32>) -> f32 {
    return nvf_noise3(p / 9.0) * 0.65 + nvf_noise3(p / 3.5) * 0.35;
}
// The same relief as a detail map on the colour: darker in its hollows, lighter on its
// crests, by up to `amount`, near the camera only.
fn nvf_detail_colour(c: vec3<f32>, p: vec3<f32>, distance: f32, amount: f32) -> vec3<f32> {
    let fade = 1.0 - smoothstep(150.0, 700.0, distance);
    if fade <= 0.0 {
        return c;
    }
    return c * (1.0 + amount * fade * (nvf_relief(p) - 0.5) * 2.0);
}
fn nvf_detail_normal(n: vec3<f32>, p: vec3<f32>, distance: f32, strength: f32) -> vec3<f32> {
    let fade = 1.0 - smoothstep(150.0, 700.0, distance);
    if fade <= 0.0 {
        return n;
    }
    let e = 0.6;
    let h = nvf_relief(p);
    let g = vec3(nvf_relief(p + vec3(e, 0.0, 0.0)) - h, nvf_relief(p + vec3(0.0, e, 0.0)) - h, nvf_relief(p + vec3(0.0, 0.0, e)) - h) / e;
    let tangential = g - dot(g, n) * n;
    return normalize(n - tangential * (strength * fade));
}

// NVForever remaster (graphics-remaster.md §4 "Environment maps"): what a shape's
// environment map stands for, the sky above and the ground below seen along the reflected
// ray, at the map's scale × its mask × a Fresnel rise towards grazing angles, hidden where
// the surroundings close the view (gi.rs). Linear light after exposure, added after the
// lighting [engineering choice: the game's cube maps are not drawn].
// The local light probe's faces (nvforever probe.rs FACES: +X, -X, +Y up -Z, -Y up +Z, +Z,
// -Z; up Y otherwise) sampled along world direction `d`: the face of its largest
// component, then that camera's 90-degree projection.
@if(SCREEN_INDIRECT && !PREPASS_PIPELINE)
fn probe_sample(d: vec3<f32>) -> vec3<f32> {
    let a = abs(d);
    var face = 0;
    var fwd = vec3(1.0, 0.0, 0.0);
    var up = vec3(0.0, 1.0, 0.0);
    if a.x >= a.y && a.x >= a.z {
        face = select(1, 0, d.x > 0.0);
        fwd = vec3(sign(d.x), 0.0, 0.0);
    } else if a.y >= a.z {
        face = select(3, 2, d.y > 0.0);
        fwd = vec3(0.0, sign(d.y), 0.0);
        up = select(vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -1.0), d.y > 0.0);
    } else {
        face = select(5, 4, d.z > 0.0);
        fwd = vec3(0.0, 0.0, sign(d.z));
    }
    let right = cross(fwd, up);
    let z = dot(d, fwd);
    let ndc = vec2(dot(d, right), dot(d, up)) / max(z, 1e-4);
    let uv = vec2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    return textureSampleLevel(screen_probe_faces, screen_probe_sampler, uv, face, 0.0).rgb;
}

@if(SCREEN_INDIRECT && !PREPASS_PIPELINE)
fn env_reflection(instance_index: u32, uv: vec2<f32>, n: vec3<f32>, v: vec3<f32>, frag: vec4<f32>) -> vec3<f32> {
    let surface = nif_surface(instance_index);
    if surface.env.x <= 0.0 {
        return vec3(0.0);
    }
    var mask = 1.0;
    if surface.env.y > 0.5 {
        mask = env_mask_value(instance_index, uv);
    }
    let r = reflect(-v, n);
    // What the local light probe (nvforever probe.rs) saw along the reflected ray, with a
    // third of an open sky's light above the horizon: glass and metal keep a sheen even
    // where the camera's own position sees only ground (no parallax in a single probe).
    let amb = pow(max(lights.ambient_color.rgb * view.exposure, vec3(0.0)), vec3(2.2));
    let open_sky = amb * mix(vec3(0.3, 0.27, 0.24), vec3(2.0, 2.15, 2.4), saturate(r.y * 0.6 + 0.5));
    let sky = mix(probe_sample(r), open_sky, 0.35);
    let n_dot_v = saturate(dot(n, v));
    let fresnel = 0.5 + 0.5 * pow(1.0 - n_dot_v, 5.0);
    let si = textureLoad(screen_indirect_texture, vec2<i32>(frag.xy - view.viewport.xy), 0);
    let open = select(1.0 - max(si.a, 0.0), 1.0, si.a < -0.5);
    return sky * (min(surface.env.x, 2.0) * mask * fresnel * open);
}

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
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    @if(FACEGEN_TINT && VERTEX_UVS_A) if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u {
        // base_color is material colour × vertex colour × texel: the texel is read again
        // as pbr_input_from_standard_material reads it, and its tinted value takes its
        // place in that product.
        let standard = standard_material(in.instance_index);
        let uv = (standard.uv_transform * vec3(in.uv, 1.0)).xy;
        let texel = base_color_texel(in.instance_index, uv, view.mip_bias).rgb;
        var factor = standard.base_color.rgb;
        @if(FACEGEN_TINT && VERTEX_UVS_A && VERTEX_COLORS) factor *= in.color.rgb;
        let tinted = facegen_tint(texel, facegen_offset_at(in.instance_index, uv));
        pbr_input.material.base_color = vec4(pbr_input.material.base_color.rgb + factor * (tinted - texel), pbr_input.material.base_color.a);
    }
    @if(PIPBOY_SCREEN && VERTEX_UVS_A) if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u {
        // NifSurface::pipboy_screen: base_color is material colour x vertex colour x texel;
        // the texel's place in that product is taken by the screen's picture as it is now
        // (pipboy_picture), as FACEGEN_TINT's above.
        let standard = standard_material(in.instance_index);
        var factor = standard.base_color.rgb;
        @if(PIPBOY_SCREEN && VERTEX_UVS_A && VERTEX_COLORS) factor *= in.color.rgb;
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
    // The deferred G-buffer (Solari, solari.rs) has no view fog to scale by.
    @if(!PREPASS_PIPELINE) let view_scale = view_emissive_scale();
    @if(PREPASS_PIPELINE) let view_scale = 1.0;
    let emissive_scale = select(view_scale, 1.0, surface.emissive_rule.x > 0.5 || (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS) == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_ADD);
    let e = vec4(pbr_input.material.emissive.rgb * emissive_scale, pbr_input.material.emissive.a);
    pbr_input.material.emissive = vec4(e.rgb * pbr_input.material.base_color.rgb, e.a);

    // Kept out of the lighting sum: multiply_factor adds it in display values.
    @if(BLEND_MULTIPLY) let multiply_emissive = pbr_input.material.emissive.rgb;
    @if(BLEND_MULTIPLY) pbr_input.material.emissive = vec4(vec3(0.0), e.a);

    // Detail relief (nvf_detail_normal), not on FaceGen skin.
    @if(SCREEN_INDIRECT && !FACEGEN_TINT) {
        let nvf_d = length(view.world_position - pbr_input.world_position.xyz);
        pbr_input.N = nvf_detail_normal(pbr_input.N, pbr_input.world_position.xyz, nvf_d, 4.0);
        pbr_input.material.base_color = vec4(nvf_detail_colour(pbr_input.material.base_color.rgb, pbr_input.world_position.xyz, nvf_d, 0.12), pbr_input.material.base_color.a);
    }
    // The G-buffer for Solari's physical lighting (nvforever solari.rs): the game's specular
    // masks, fitted to its display-value highlights, turn rock and rust to foil when lit
    // physically; held to a plain dielectric's sheen.
    @if(PREPASS_PIPELINE) pbr_input.material.reflectance = min(pbr_input.material.reflectance, vec3(0.3));
    @if(PREPASS_PIPELINE) pbr_input.material.perceptual_roughness = max(pbr_input.material.perceptual_roughness, 0.55);
    @if(PREPASS_PIPELINE) let out = deferred_output(in, pbr_input);
    @if(!PREPASS_PIPELINE) var out: FragmentOutput;
    @if(!PREPASS_PIPELINE) if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        // Hot light source (hot_light): the emitted light is added after the lighting.
        @if(!PREPASS_PIPELINE && !TONEMAP_IN_SHADER && !BLEND_MULTIPLY) let hot_e = pbr_input.material.emissive.rgb;
        @if(!PREPASS_PIPELINE && !TONEMAP_IN_SHADER && !BLEND_MULTIPLY) pbr_input.material.emissive = vec4(vec3(0.0), pbr_input.material.emissive.a);
        out.color = apply_pbr_lighting(pbr_input);
        @if(!PREPASS_PIPELINE && SHADE_LIGHT) out.color = with_shade_light(pbr_input, out.color);
        @if(SCREEN_INDIRECT && !PREPASS_PIPELINE && VERTEX_UVS_A) {
            out.color = vec4(out.color.rgb + env_reflection(in.instance_index, in.uv, pbr_input.N, pbr_input.V, in.position), out.color.a);
        }
        // Bevy's ambient specular, with the same inputs as in apply_pbr_lighting and
        // bevy_pbr::ambient::ambient_light (no lightmaps are used here).
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
        out.color = vec4(max(out.color.rgb - view.exposure * ambient_spec, vec3(0.0)), out.color.a);
        @if(!PREPASS_PIPELINE && !TONEMAP_IN_SHADER && !BLEND_MULTIPLY) out.color = vec4(hot_light(out.color.rgb, hot_e), out.color.a);
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
    // The colour is a factor, not light: no fog and no tonemapping. Bevy's multiply blend
    // is destination × colour + destination × (1 − alpha), so the colour is premultiplied
    // as bevy_pbr::pbr_functions::premultiply_alpha does.
    @if(!PREPASS_PIPELINE && BLEND_MULTIPLY) let factor = multiply_factor(out.color.rgb, multiply_emissive);
    @if(!PREPASS_PIPELINE && BLEND_MULTIPLY) out.color = vec4(factor * out.color.a, out.color.a);
    @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY && BLEND_PREMULTIPLIED_ALPHA) if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS) == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_ADD {
        // Additive: fog (and, in views without HDR, the tonemapping) on the colour at
        // alpha 1, which Bevy's premultiplying leaves as it is with alpha 0; then the
        // display-value sum over the opaque frame behind (the anchor in nif_material.rs
        // keeps Bevy's copy of it in every view). A ONE / ONE shape adds its colour whole,
        // faded only by the view-angle falloff.
        let add_a = select(out.color.a, falloff, surface.add_whole > 0.5);
        let fogged = main_pass_post_lighting_processing(pbr_input, vec4(out.color.rgb, 1.0));
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
    @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY) let blend_mode = pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_ALPHA_MODE_RESERVED_BITS;
    @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY) if blend_mode == STANDARD_MATERIAL_FLAGS_ALPHA_MODE_BLEND && (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) != 0u {
        out.color = vec4(out.color.rgb * blend_colour_scale(out.color.a), out.color.a);
    }
    // NifSurface::pipboy_screen: the mark that keeps these pixels out of the image-space grade
    // (imagespace.wesl SCREEN_MARK).
    @if(PIPBOY_SCREEN) out.color.a = 0.25;
    @if(!PREPASS_PIPELINE && !BLEND_MULTIPLY && SOFT_EDGES) out.color.a = edge;
    return out;
}
