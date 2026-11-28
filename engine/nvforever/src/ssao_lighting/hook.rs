//! The ambient occlusion's text hook in bevy_pbr's `pbr_functions` (ssao_lighting.rs module
//! notes). Plain text and the standard library only, so the shader compile checks
//! (engine/nvf-shader-tests) apply the very same edits before composing the shaders that
//! read the occlusion.

/// Where an edit's text goes against its anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Before,
    After,
    Replace,
}

/// The display-value ambient line of lights/display_shader.rs `DISPLAY_EDITS`, as it
/// stands in the patched `apply_pbr_lighting`.
pub const AMBIENT_LINE: &str = "        let nvf_ambient = view_bindings::lights.ambient_color.rgb * view_bindings::view.exposure * diffuse_occlusion;\n";

/// The same line kept without the def, and its twin with the occlusion under it, per colour
/// channel by the multi-bounce fit with the surface's diffuse colour
/// (ssao_lighting.wesl `nvf_ao_multibounce`; `diffuse_color` is `apply_pbr_lighting`'s own,
/// declared before the lights). `-view_z`
/// is the fragment's view distance from its own world position (so the first-person arms,
/// drawn at a squeezed depth, still compare their real distance with the texels', which the
/// occlusion pass unsqueezed).
pub const AMBIENT_LINE_HOOKED: &str = "        // NVForever ambient occlusion (nvforever ssao_lighting.rs): on the display-value
        // ambient alone, never on the direct lights nor on Bevy's ambient highlight above.
        @if(!NVF_AMBIENT_OCCLUSION)
        let nvf_ambient = view_bindings::lights.ambient_color.rgb * view_bindings::view.exposure * diffuse_occlusion;
        @if(NVF_AMBIENT_OCCLUSION)
        let nvf_ambient = view_bindings::lights.ambient_color.rgb * view_bindings::view.exposure * diffuse_occlusion * nvf_ao_multibounce(nvf_ao_ambient, diffuse_color);
";

/// The start of `apply_pbr_lighting`'s point light loop, after `view_z` is declared.
pub const LAMP_ANCHOR: &str = "    // Point lights (direct)\n";

/// Once per fragment, before the light loops: the ambient's factor (the one read of the
/// occlusion texture; the ambient line below uses it) and from it the factor on the light of
/// lamps that cast no shadow here (`NVF_PLAY_DEBUG_AO_LAMPS`), `mix(1, g, w)`.
pub const LAMP_TEXT: &str = "    // NVForever ambient occlusion (nvforever ssao_lighting.rs): the ambient's factor, read
    // once, and from it the factor on the light of lamps that cast no shadow here (1 while
    // NVF_PLAY_DEBUG_AO_LAMPS is 0). First-person shapes (the arms, the weapon, the Pip-Boy;
    // nvf_assets nif_material.rs FIRST_PERSON_DEPTH) take none: only the world is occluded.
    @if(NVF_AMBIENT_OCCLUSION && !FIRST_PERSON_DEPTH)
    let nvf_ao_ambient = nvf_ambient_visibility(in.frag_coord.xy, -view_z);
    @if(NVF_AMBIENT_OCCLUSION && FIRST_PERSON_DEPTH)
    let nvf_ao_ambient = 1.0;
    @if(NVF_AMBIENT_OCCLUSION)
    let nvf_ao_lamps = nvf_ao_multibounce(nvf_lamp_visibility(nvf_ao_ambient), diffuse_color);
    // And the factor on lamps whose shadow is looked up here: a share of it.
    @if(NVF_AMBIENT_OCCLUSION)
    let nvf_ao_shadowed = nvf_shadowed_lamp_visibility(nvf_ao_lamps);
    // The factor on the lamps' highlights, and its share for lamps whose shadow is looked up.
    @if(NVF_AMBIENT_OCCLUSION)
    let nvf_ao_spec = nvf_specular_visibility(nvf_ao_ambient, NdotV, roughness);
    @if(NVF_AMBIENT_OCCLUSION)
    let nvf_ao_spec_shadowed = nvf_shadowed_lamp_visibility(vec3(nvf_ao_spec)).x;

";

/// A point light's part of the display-value sum (lights/display_shader.rs `DISPLAY_EDITS`).
pub const POINT_LINE: &str = "        if nvf_display { nvf_direct += nvf_point_display(light_id, in.world_position.xyz, in.N) * shadow; }\n";

/// The same kept without the def, and its twin taking the lamps' factor where the light's
/// shadow is not looked up for this fragment (the light casts none, or the surface receives
/// none: exactly the condition under which bevy_pbr leaves `shadow` at 1 above). Lights with
/// a shadow (the Pip-Boy light, flashlight.rs, the ALC shadows row's lamps) take a share of the
/// factor: their shadow already keeps their light out of the way of what blocks it, but not
/// out of the creases a shadow map does not resolve (`nvf_ao_shadowed`, ssao_lighting.rs
/// `SHADOWED_LAMPS`; until 2026-11-24 they took none). A lamp whose shadow is fading in or
/// out (the ALCS row's fade, bevy_pbr shadows.wesl `nvf_shadow_fade`) goes from the one
/// factor to the other by what is left of the fade, its highlight too: until 2026-11-24 it
/// had neither its shadow nor the full factor for the fade's length, and the creases under
/// it popped.
pub const POINT_LINE_HOOKED: &str = "        @if(!NVF_AMBIENT_OCCLUSION)
        if nvf_display { nvf_direct += nvf_point_display(light_id, in.world_position.xyz, in.N) * shadow; }
        // NVForever ambient occlusion (nvforever ssao_lighting.rs): lamps without a shadow here.
        @if(NVF_AMBIENT_OCCLUSION)
        if nvf_display { nvf_direct += nvf_point_display(light_id, in.world_position.xyz, in.N) * shadow * mix(nvf_ao_lamps, nvf_ao_shadowed, select(0.0, shadows::nvf_shadow_fade(light_id), (in.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u && (view_bindings::clustered_lights.data[light_id].flags & mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u)); }
        // The lamp's highlight (Bevy's `direct_light` holds only that in a world view) by the
        // highlights' factor: what the line above added without it is taken back (the
        // highlight took the shadow held to 0, light_shadows.rs SHADOW_STRENGTH).
        @if(NVF_AMBIENT_OCCLUSION)
        if nvf_display { direct_light -= light_contrib * (max(shadow, 0.0) * (1.0 - mix(nvf_ao_spec, nvf_ao_spec_shadowed, select(0.0, shadows::nvf_shadow_fade(light_id), (in.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u && (view_bindings::clustered_lights.data[light_id].flags & mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u)))); }
";

/// A spot light's part of the display-value sum.
pub const SPOT_LINE: &str = "        if nvf_display { nvf_direct += nvf_spot_display(light_id, in.world_position.xyz, in.N) * shadow; }\n";

/// As [`POINT_LINE_HOOKED`], for spot lights.
pub const SPOT_LINE_HOOKED: &str = "        @if(!NVF_AMBIENT_OCCLUSION)
        if nvf_display { nvf_direct += nvf_spot_display(light_id, in.world_position.xyz, in.N) * shadow; }
        // NVForever ambient occlusion (nvforever ssao_lighting.rs): lamps without a shadow here.
        @if(NVF_AMBIENT_OCCLUSION)
        if nvf_display { nvf_direct += nvf_spot_display(light_id, in.world_position.xyz, in.N) * shadow * mix(nvf_ao_lamps, nvf_ao_shadowed, select(0.0, shadows::nvf_shadow_fade(light_id), (in.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u && (view_bindings::clustered_lights.data[light_id].flags & mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u)); }
        // The lamp's highlight (Bevy's `direct_light` holds only that in a world view) by the
        // highlights' factor: what the line above added without it is taken back (the
        // highlight took the shadow held to 0, light_shadows.rs SHADOW_STRENGTH).
        @if(NVF_AMBIENT_OCCLUSION)
        if nvf_display { direct_light -= light_contrib * (max(shadow, 0.0) * (1.0 - mix(nvf_ao_spec, nvf_ao_spec_shadowed, select(0.0, shadows::nvf_shadow_fade(light_id), (in.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u && (view_bindings::clustered_lights.data[light_id].flags & mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u)))); }
";

/// The end of `main_pass_post_lighting_processing`: after the fog, the tonemapping in the
/// shader and the dither, before the alpha is premultiplied.
pub const DEBUG_ANCHOR: &str = "    @if(PREMULTIPLY_ALPHA)\n    output_color = premultiply_alpha(pbr_input.material.flags, output_color);\n";

/// The function [`DEBUG_ANCHOR`] must lie in.
pub const DEBUG_FUNCTION: &str = "\nfn main_pass_post_lighting_processing(";

/// The debug view (`NVF_PLAY_DEBUG_AO=view-lighting`): the factor this fragment's ambient
/// took, drawn in place of the lit colour at the very end of the material's colour (after
/// whatever the material did to the lit colour, the fog and the dither), so what is seen is
/// exactly what `nvf_ambient_visibility` gave the fragment: the same position, the same
/// distance (`view_z` as `apply_pbr_lighting` forms it).
pub const DEBUG_TEXT: &str = "    // NVForever ambient occlusion (nvforever ssao_lighting.rs): the debug view draws the
    // factor this fragment's ambient light took, after everything else (first-person shapes
    // take none and keep their colour).
    @if(NVF_AMBIENT_OCCLUSION && !FIRST_PERSON_DEPTH)
    if nvf_ao_debug() {
        let nvf_view_z = dot(vec4<f32>(
            view_bindings::view.view_from_world[0].z,
            view_bindings::view.view_from_world[1].z,
            view_bindings::view.view_from_world[2].z,
            view_bindings::view.view_from_world[3].z
        ), pbr_input.world_position);
        output_color = vec4<f32>(vec3<f32>(nvf_ambient_visibility(pbr_input.frag_coord.xy, -nvf_view_z)), output_color.a);
    }
";

/// The hook in bevy_pbr's `pbr_functions` [documented:
/// engine/vendor/bevy_pbr/src/render/pbr_functions.wesl (bevy_pbr 0.20.1) with lights.rs
/// `DISPLAY_EDITS` (and, in the game, sun_visibility.rs `HOOK_EDITS`) made; the tests apply
/// them to that file], in order, each at the first occurrence of its anchor after the last
/// edit: (anchor, text, where the text goes). Deliberately left alone: Bevy's
/// `ambient::ambient_light` call and `let diffuse_occlusion = in.diffuse_occlusion;`, since
/// the NIF and terrain shaders subtract exactly that ambient highlight using the
/// un-occluded `diffuse_occlusion` (nvf_assets nif_material.wesl, terrain.wesl), and an
/// occluded highlight would be over-subtracted there; the directional lights (the sun, the
/// moon, an interior's directional light) and every light's specular part (Bevy's
/// `direct_light`).
pub const HOOK_EDITS: [(&str, &str, Place); 6] = [
    (
        "import bevy_render::maths::{E, powsafe};\n",
        "// NVForever ambient occlusion (nvforever ssao_lighting.rs).
@if(NVF_AMBIENT_OCCLUSION) import nvforever::ssao_lighting::{nvf_ambient_visibility, nvf_ao_debug, nvf_ao_multibounce, nvf_lamp_visibility, nvf_shadowed_lamp_visibility, nvf_specular_visibility};
",
        Place::After,
    ),
    (LAMP_ANCHOR, LAMP_TEXT, Place::Before),
    (POINT_LINE, POINT_LINE_HOOKED, Place::Replace),
    (SPOT_LINE, SPOT_LINE_HOOKED, Place::Replace),
    (AMBIENT_LINE, AMBIENT_LINE_HOOKED, Place::Replace),
    (DEBUG_ANCHOR, DEBUG_TEXT, Place::Before),
];

/// The index in [`HOOK_EDITS`] of the debug view's edit.
pub const DEBUG_EDIT: usize = 5;

/// `source` (lights.rs's display-value patch, and in the game the sun visibility's hook,
/// made) with [`HOOK_EDITS`]; `None` when an anchor is missing or the debug view's anchor
/// is not in `main_pass_post_lighting_processing` (Bevy or the display patch changed), and
/// the caller keeps `source`.
pub fn hook_patched(source: &str) -> Option<String> {
    let mut out = String::with_capacity(source.len() + 2048);
    let mut rest = source;
    for (i, (anchor, text, place)) in HOOK_EDITS.into_iter().enumerate() {
        let at = rest.find(anchor)?;
        if i == DEBUG_EDIT {
            let head = &rest[..at];
            if !head.rfind("\nfn ").is_some_and(|f| head[f..].starts_with(DEBUG_FUNCTION)) {
                return None;
            }
        }
        out.push_str(&rest[..at]);
        match place {
            Place::Before => {
                out.push_str(text);
                out.push_str(anchor);
            }
            Place::After => {
                out.push_str(anchor);
                out.push_str(text);
            }
            Place::Replace => out.push_str(text),
        }
        rest = &rest[at + anchor.len()..];
    }
    out.push_str(rest);
    Some(out)
}
