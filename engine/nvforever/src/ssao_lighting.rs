//! Ambient occlusion inside the lighting (#2897 stage two, docs/design/graphics-enhanced.md
//! §3.1.2; the switch's default placement): instead of multiplying the finished picture
//! (ssao.rs's post passes, now the fallback), the occlusion factor scales the display-value
//! ambient term of the patched `apply_pbr_lighting` (lights/display_shader.rs `nvf_ambient`)
//! and the light of lamps that cast no shadow ([`LAMPS`]), so the sun, the moon and every
//! highlight stay as they are. An engineering choice, not the original game's.
//!
//! The parts, built the way the sun visibility's are (sun_visibility.rs):
//! - bevy_pbr's view component `NvfAmbientOcclusionBindings` (vendored, engine/vendor/README.md):
//!   a uniform ([`AmbientOcclusionLightingUniform`]) and the half-resolution occlusion at
//!   mesh view bindings 43-44, which gives the view's depth-writing pipelines the shader def
//!   `NVF_AMBIENT_OCCLUSION`; ssao.rs inserts and removes it during extraction, fills the
//!   texture in its `ao_lighting_pass` between the prepass and the main pass, and writes the
//!   uniform;
//! - ssao_lighting.wesl, imported as `nvforever::ssao_lighting` under that def:
//!   `nvf_ambient_visibility`, a depth-aware 2 x 2 upsample of the half-resolution texels;
//! - the text hook ([`hook`]), patched into `pbr_functions` by lights.rs right after the sun
//!   visibility's hook: every statement it adds is under `@if(NVF_AMBIENT_OCCLUSION)`, and
//!   the one line it replaces is kept word for word under `@if(!NVF_AMBIENT_OCCLUSION)`, so
//!   without the def the shader text a pipeline compiles is what it was before.
//!
//! Who reads it (the def's default is the vendored rule: pipelines that write depth): water
//! and refracting NIF shapes opt out (in no prepass: they would take the bed's or the
//! wall's occlusion behind them), the landscape's blended layers opt in (they lie on the
//! prepass's surface), grass and leaf cards read it unless their constants say otherwise
//! (grass.rs `GRASS_READS_AMBIENT_OCCLUSION`, tree.rs `LEAVES_READ_AMBIENT_OCCLUSION`).

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::prelude::*;
use bevy::render::render_resource::ShaderType;
use bevy::shader::Shader;
use std::sync::atomic::{AtomicU8, Ordering};

pub mod hook;
pub(crate) use hook::hook_patched;

/// ssao_lighting.wesl, imported by the patched `apply_pbr_lighting` as
/// `nvforever::ssao_lighting`.
const SHADER: Handle<Shader> = uuid_handle!("8d1e6a2c-3f47-4b9e-a5c1-6e02d7f4b318");

/// The exponent the factor is raised to before it scales the display-value sum (the ambient
/// term, and the lamps' by [`LAMPS`]). The sum is a display value: the light is the texture
/// times its 2.2nd power (lights/display_shader.rs) and the shown value comes back as about
/// the sum itself, so with exponent 1 the shown value of a surface lit by what the factor
/// reaches falls by the factor itself, as the picture placement (stage one) multiplies the
/// shown picture by it. The first value, 1/2.2, scaled the linear light by the factor instead,
/// so the shown value fell by only about f^0.45, less than half the picture placement's
/// darkening at the same strength; its comment's claim that it kept stage one's meaning was
/// wrong, stage one multiplying the tonemapped, multisample-resolved picture. At exponent 1
/// the Goodsprings street changes as many pixels as stage one (20.3 % against 20.9 % by more
/// than 2 levels of 255 at strength 0.8, mean darkening 1.85 against 2.34 levels) [observed
/// 2026-11-18, the main session's pictures] [engineering choice: 1; the aid
/// `NVF_PLAY_DEBUG_AO_EXPONENT` (ssao.rs `AoDebug`) replaces it for comparisons].
pub const EXPONENT: f32 = 1.0;

/// How much of the factor reaches the light of lamps (point and spot lights) that cast no
/// shadow at the surface: all of it. #2897 asked for the ambient term only, but interiors are
/// lit almost wholly by such lamps (in the sum's direct part), and on the ambient alone the
/// occlusion all but vanished there: Doc Mitchell's house changed 0.7 % of its pixels against
/// stage one's 9.7 %, with the lamps at 1 as well 5.1 % (the Prospector Saloon 5.9 % against
/// 7.4 %) [observed 2026-11-18, the main session's pictures at exponent 1, strength 0.8]. A
/// lamp casting no shadow lights a crease as fully as open floor, so nothing else keeps its
/// light out; the sun, the moon and an interior's directional light are never touched
/// [engineering choice: 1, the player's, judged in play on 2026-11-18;
/// `NVF_PLAY_DEBUG_AO_LAMPS` replaces it, 0 for the ambient term alone].
pub const LAMPS: f32 = 1.0;

/// Whether the ambient's factor takes the multi-bounce fit by the surface's colour
/// (ssao_lighting.wesl `nvf_ao_multibounce`; Jimenez et al. 2016, the "later multibounce"
/// #2897 left open): a light surface's creases keep the light it bounces into them, a dark
/// one's stay dark, and coloured surfaces keep their colour in the shade [engineering
/// choice: on; `NVF_PLAY_DEBUG_AO_MULTIBOUNCE=0` gives the plain factor, for comparing]. The
/// lamps' factor stays plain.
pub const MULTIBOUNCE: bool = true;

/// The share of the fragment's distance (plus one unit) within which a texel's distance
/// counts as the same surface: stage one's fallback tolerance (ssao.wesl
/// `FALLBACK_TOLERANCE`) [engineering choice].
pub const DISTANCE_TOLERANCE: f32 = 0.05;

/// The share of the lamps' factor ([`LAMPS`]) that also reaches the light of a lamp whose
/// shadow is looked up at the fragment (the ALC shadows row's lamps, the flashlight): 0..1
/// [engineering choice, 2026-11-24: half. At 0, as it was, a room lit by casting lamps kept
/// its occlusion on the ambient light alone (0.7% of pixels changed by the occlusion in Doc
/// Mitchell's house against 2.5% with the lamps casting none); at 1 a crease the lamp's
/// shadow already darkens is darkened twice]. `NVF_PLAY_DEBUG_AO_SHADOWED=<0..1>` gives
/// another for a run.
pub const SHADOWED_LAMPS: f32 = 0.5;

/// [`SHADOWED_LAMPS`], or the test aid's.
pub fn shadowed_lamps() -> f32 {
    static SHARE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *SHARE.get_or_init(|| match std::env::var("NVF_PLAY_DEBUG_AO_SHADOWED").ok().and_then(|v| v.trim().parse::<f32>().ok()) {
        Some(v) => {
            bevy::log::info!("ambient occlusion: NVF_PLAY_DEBUG_AO_SHADOWED={v}");
            v.clamp(0.0, 1.0)
        }
        None => SHADOWED_LAMPS,
    })
}

/// The weight of the occlusion on the lamps' highlights (the specular part of point and spot
/// lights; ssao_lighting.wesl `nvf_specular_visibility`): 0..1 [engineering choice,
/// 2026-11-24: in full, the published specular occlusion as it is]. Not the sun's or the
/// moon's, as their diffuse light takes none; not the ambient highlight, which the NIF and
/// terrain shaders take back with the un-occluded factor (hook.rs `HOOK_EDITS`).
/// `NVF_PLAY_DEBUG_AO_HIGHLIGHTS=<0..1>` gives another for a run.
pub const HIGHLIGHTS: f32 = 1.0;

/// [`HIGHLIGHTS`], or the test aid's.
pub fn highlights() -> f32 {
    static WEIGHT: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *WEIGHT.get_or_init(|| match std::env::var("NVF_PLAY_DEBUG_AO_HIGHLIGHTS").ok().and_then(|v| v.trim().parse::<f32>().ok()) {
        Some(v) => {
            bevy::log::info!("ambient occlusion: NVF_PLAY_DEBUG_AO_HIGHLIGHTS={v}");
            v.clamp(0.0, 1.0)
        }
        None => HIGHLIGHTS,
    })
}

/// ssao_lighting.wesl `nvf_specular_visibility` on the CPU, for the tests: a highlight's
/// factor from the ambient's factor `g`, the cosine between normal and view, the roughness
/// and the weight.
#[cfg_attr(not(test), allow(dead_code))]
pub fn specular_visibility(g: f32, n_dot_v: f32, roughness: f32, weight: f32) -> f32 {
    let w = weight.clamp(0.0, 1.0);
    if w <= 0.0 {
        return 1.0;
    }
    let exponent = (-16.0 * roughness.clamp(0.0, 1.0) - 1.0).exp2();
    let open = ((n_dot_v.max(0.0) + g).powf(exponent) - 1.0 + g).clamp(0.0, 1.0);
    1.0 + (open - 1.0) * w
}

/// `NvfAmbientOcclusion` of ssao_lighting.wesl (field for field).
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct AmbientOcclusionLightingUniform {
    /// Exponent, debug view (1 on), fade end (game units), distance tolerance.
    pub params: Vec4,
    /// Strength, the lamps' weight (0..1), the multi-bounce fit (1 on); w unused.
    pub extra: Vec4,
    /// The share of the lamps' factor on lamps whose shadow is looked up ([`SHADOWED_LAMPS`]),
    /// the weight of the factor on the lamps' highlights ([`HIGHLIGHTS`]); z, w unused.
    pub more: Vec4,
}

impl AmbientOcclusionLightingUniform {
    /// The uniform for an occlusion raised to `exponent`, faded out at `fade_end` (game
    /// units), made with `strength`, on lamps without a shadow by `lamps` (held to 0..1), the
    /// ambient's factor with or without the multi-bounce fit ([`MULTIBOUNCE`]), with or
    /// without the debug view.
    pub fn new(exponent: f32, fade_end: f32, strength: f32, lamps: f32, multibounce: bool, debug: bool) -> Self {
        AmbientOcclusionLightingUniform {
            params: Vec4::new(exponent, if debug { 1.0 } else { 0.0 }, fade_end, DISTANCE_TOLERANCE),
            extra: Vec4::new(strength, lamps.clamp(0.0, 1.0), if multibounce { 1.0 } else { 0.0 }, 0.0),
            more: Vec4::new(shadowed_lamps(), highlights(), 0.0, 0.0),
        }
    }
}

/// Whether lights.rs has put the hook into `pbr_functions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HookState {
    /// Bevy's `pbr_functions` has not loaded yet.
    Pending,
    Installed,
    /// The source was not as expected: the occlusion stays on the picture.
    Failed,
}

static HOOK_STATE: AtomicU8 = AtomicU8::new(0);

/// Record what became of the hook (lights.rs `patch_display_sum`).
pub(crate) fn set_hook_state(state: HookState) {
    HOOK_STATE.store(
        match state {
            HookState::Pending => 0,
            HookState::Installed => 1,
            HookState::Failed => 2,
        },
        Ordering::Relaxed,
    );
}

/// What became of the hook; the occlusion goes into the lighting only once it is in place.
pub(crate) fn hook_state() -> HookState {
    match HOOK_STATE.load(Ordering::Relaxed) {
        0 => HookState::Pending,
        1 => HookState::Installed,
        _ => HookState::Failed,
    }
}

pub struct SsaoLightingPlugin;

impl Plugin for SsaoLightingPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "ssao_lighting.wesl", |s, _| Shader::from_wesl(s, "embedded://nvforever/ssao_lighting.wesl"));
    }
}

#[cfg(test)]
mod tests {
    use super::hook::*;
    use super::*;
    use crate::lights::unix_lines;
    use crate::sun_visibility::tests::{balanced, block_after, code, code_lines, resolve_known};

    /// ssao_lighting.wesl's `NVF_AO_MIN_WEIGHT`: below it the factor is 1.
    const MIN_WEIGHT: f32 = 0.001;

    fn vendored(file: &str) -> String {
        let path = format!("{}/../vendor/bevy_pbr/src/render/{file}", env!("CARGO_MANIFEST_DIR"));
        unix_lines(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")))
    }

    /// The vendored source with the display-value patch and the sun visibility's hook (as
    /// lights.rs makes them), then this hook.
    fn hooked() -> (String, String) {
        let display = crate::lights::display_patched(&vendored("pbr_functions.wesl")).expect("display patch applies");
        let sun = crate::sun_visibility::hook_patched(&display).expect("sun hook applies");
        let hooked = hook_patched(&sun).expect("ambient occlusion hook applies");
        (sun, hooked)
    }

    fn wesl() -> String {
        unix_lines(include_str!("ssao_lighting.wesl"))
    }

    /// Without the def nothing changes: each added text is comments and statements under
    /// `@if(NVF_AMBIENT_OCCLUSION)`, the replaced line is kept under its negation, and taking
    /// the additions out gives the input back word for word.
    #[test]
    fn hook_identity_without_the_def() {
        let (input, hooked) = hooked();
        assert_ne!(input, hooked);
        for (_, text, place) in HOOK_EDITS {
            if place == Place::Replace {
                continue;
            }
            let code: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")).collect();
            // The def alone, or the def and more (first-person shapes: `&& !FIRST_PERSON_DEPTH`).
            assert!(code[0].starts_with("@if(NVF_AMBIENT_OCCLUSION)") || code[0].starts_with("@if(NVF_AMBIENT_OCCLUSION && "), "{text}");
        }
        for (line, hooked_line) in [(AMBIENT_LINE, AMBIENT_LINE_HOOKED), (POINT_LINE, POINT_LINE_HOOKED), (SPOT_LINE, SPOT_LINE_HOOKED)] {
            assert!(hooked_line.contains(&format!("        @if(!NVF_AMBIENT_OCCLUSION)\n{line}")), "the line kept word for word under the negation: {line}");
            assert!(hooked_line.contains("        @if(NVF_AMBIENT_OCCLUSION)\n"), "its twin under the def: {line}");
        }
        let mut back = hooked.clone();
        for (anchor, text, place) in HOOK_EDITS {
            match place {
                Place::Before => back = back.replacen(&format!("{text}{anchor}"), anchor, 1),
                Place::After => back = back.replacen(&format!("{anchor}{text}"), anchor, 1),
                Place::Replace => back = back.replacen(text, anchor, 1),
            }
        }
        assert_eq!(back, input);
        // Resolved without the def, the code lines are the input's, with the sun's def on
        // or off.
        for sun in [false, true] {
            let known = [("NVF_AMBIENT_OCCLUSION", false), ("NVF_SUN_VISIBILITY", sun)];
            let want = resolve_known(&input, &known[1..]);
            let got = resolve_known(&hooked, &known);
            assert_eq!(code_lines(&got), code_lines(&want), "sun {sun}");
            for name in ["nvf_ambient_visibility", "nvf_ao_debug", "nvf_lamp_visibility", "nvf_ao_lamps"] {
                assert!(!code(&got).contains(name), "sun {sun}: {name}");
            }
        }
    }

    /// Each anchor occurs once where the hook looks for it (in the display- and
    /// sun-patched source, from the previous edit on), and a missing one leaves the source
    /// alone.
    #[test]
    fn each_anchor_occurs_once() {
        let (input, _) = hooked();
        assert_eq!(input.matches(HOOK_EDITS[0].0).count(), 1, "the import line");
        assert_eq!(input.matches(AMBIENT_LINE).count(), 1, "the display-value ambient line");
        assert_eq!(input.matches(LAMP_ANCHOR).count(), 1, "the point light loop");
        assert_eq!(input.matches(POINT_LINE).count(), 1, "the point lights' display line");
        assert_eq!(input.matches(SPOT_LINE).count(), 1, "the spot lights' display line");
        // In the file's order, so each is found after the one before.
        let order: Vec<usize> = HOOK_EDITS.iter().map(|(a, _, _)| input.find(a).unwrap()).collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "the anchors in the file's order");
        // The debug view's anchor ends main_pass_post_lighting_processing, once in the file.
        assert_eq!(input.matches(DEBUG_ANCHOR).count(), 1, "the debug view's anchor");
        let at = input.find(DEBUG_ANCHOR).unwrap();
        let last_fn = input[..at].rfind("\nfn ").expect("a function before it");
        assert!(input[last_fn..].starts_with(DEBUG_FUNCTION), "the anchor is main_pass_post_lighting_processing's");
        for (anchor, _, _) in HOOK_EDITS {
            assert!(hook_patched(&input.replacen(anchor, "", 1)).is_none(), "missing {anchor:?}");
        }
        // The debug anchor in another function (Bevy moved it) is refused too.
        let moved = input.replacen(DEBUG_ANCHOR, &format!("}}\nfn nvf_elsewhere(pbr_input: pbr_types::PbrInput, output_color: vec4<f32>) {{\n{DEBUG_ANCHOR}"), 1);
        assert!(hook_patched(&moved).is_none());
    }

    /// The two lines left alone (module notes, HOOK_EDITS) are the same before and after.
    #[test]
    fn bevy_ambient_and_diffuse_occlusion_untouched() {
        let (input, hooked) = hooked();
        for wanted in ["ambient::ambient_light(in.world_position, in.N, in.V, NdotV,", "    let diffuse_occlusion = in.diffuse_occlusion;\n"] {
            let lines = |s: &str| s.lines().filter(|l| l.contains(wanted.trim_end_matches('\n'))).map(str::to_string).collect::<Vec<_>>();
            let before = lines(&input);
            assert_eq!(before.len(), 1, "{wanted} once in the input");
            assert_eq!(lines(&hooked), before, "{wanted}");
        }
    }

    /// With the def the occlusion reaches the ambient line and the debug view alone: once
    /// in the ambient (after `view_z` is declared), once in the debug view at the end of
    /// `main_pass_post_lighting_processing`, the import before the function; brackets close in
    /// every variant; the names imported are the WESL's.
    #[test]
    fn hook_resolves_with_and_without_the_def() {
        let (_, hooked) = hooked();
        assert!(balanced(&hooked));
        for (_, text, _) in HOOK_EDITS {
            assert!(balanced(text), "{text}");
        }
        for on in [false, true] {
            for sun in [false, true] {
                let known = [("NVF_AMBIENT_OCCLUSION", on), ("NVF_SUN_VISIBILITY", sun)];
                let r = resolve_known(&hooked, &known);
                let case = format!("ambient occlusion {on}, sun {sun}");
                assert!(!code(&r).contains("NVF_AMBIENT_OCCLUSION"), "{case}: every condition resolved");
                assert!(balanced(&r), "{case}");
                let lighting = block_after(&r, "fn nvf_apply_pbr_lighting(");
                assert!(!lighting.contains("\nfn "), "{case}: the lighting function ends where it should");
                assert_eq!(lighting.matches("let nvf_ambient = ").count(), 1, "{case}: one ambient line");
                assert_eq!(lighting.matches("nvf_ambient_visibility(in.frag_coord.xy, -view_z)").count(), usize::from(on), "{case}: the ambient alone");
                assert!(!lighting.contains("nvf_ao_debug"), "{case}: no debug view in the lighting itself");
                // The lamps: the factor formed once, then taken by the point and spot lights
                // without a shadow, never by the directional lights.
                assert_eq!(lighting.matches("let nvf_ao_lamps = nvf_ao_multibounce(nvf_lamp_visibility(nvf_ao_ambient), diffuse_color);").count(), usize::from(on), "{case}");
                assert_eq!(lighting.matches("let nvf_ao_ambient = nvf_ambient_visibility(in.frag_coord.xy, -view_z);").count(), usize::from(on), "{case}: the texels are read once a fragment");
                assert_eq!(lighting.matches("* mix(nvf_ao_lamps, nvf_ao_shadowed, select(0.0, shadows::nvf_shadow_fade(light_id),").count(), if on { 2 } else { 0 }, "{case}");
                assert_eq!(lighting.matches("if nvf_display { nvf_direct += nvf_point_display(").count(), 1, "{case}: one point line");
                assert_eq!(lighting.matches("if nvf_display { nvf_direct += nvf_spot_display(").count(), 1, "{case}: one spot line");
                let directional = block_after(lighting, "for (var i: u32 = 0u; i < n_directional_lights; i = i + 1u) {");
                assert!(!directional.contains("nvf_ao_lamps"), "{case}: the directional lights keep their light");
                let post = block_after(&r, "fn main_pass_post_lighting_processing(");
                assert!(!post.contains("\nfn "), "{case}");
                assert_eq!(post.matches("if nvf_ao_debug() {").count(), usize::from(on), "{case}: the debug view at the end");
                if on {
                    let at = |s: &str| lighting.find(s).unwrap_or_else(|| panic!("{case}: {s}"));
                    assert!(at("let view_z") < at("= nvf_ambient_visibility("), "{case}: view_z before the factor is read");
                    assert!(at("= nvf_ambient_visibility(") < at("* nvf_ao_multibounce(nvf_ao_ambient, diffuse_color);"), "{case}: the factor is read before the ambient line takes it");
                    assert!(at("let diffuse_color") < at("* nvf_ao_multibounce("), "{case}: the diffuse colour exists where the ambient line takes it");
                    assert!(at("* nvf_ao_multibounce(nvf_ao_ambient, diffuse_color);") < at("    // Total light\n"), "{case}");
                    assert!(at("let view_z") < at("let nvf_ao_lamps") && at("let nvf_ao_lamps") < at("    // Point lights (direct)\n"), "{case}: formed after view_z, before the loops");
                    let point = at("nvf_point_display(light_id, in.world_position.xyz, in.N) * shadow * mix(nvf_ao_lamps, nvf_ao_shadowed, select(");
                    let spot = at("nvf_spot_display(light_id, in.world_position.xyz, in.N) * shadow * mix(nvf_ao_lamps, nvf_ao_shadowed, select(");
                    assert!(point < at("    // Spot lights (direct)\n"), "{case}: in the point loop");
                    assert!(at("    // Spot lights (direct)\n") < spot && spot < at("    // directional lights (direct)\n"), "{case}: in the spot loop");
                    // Only where bevy_pbr leaves the light's shadow unread.
                    assert_eq!(lighting.matches("(in.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u && (view_bindings::clustered_lights.data[light_id].flags & mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u)").count(), 4, "{case}: each lamp line's diffuse and highlight");
                    // The highlights: what Bevy's line added is taken back by the factor, for
                    // the point and the spot lights, from the one factor read above.
                    assert_eq!(lighting.matches("if nvf_display { direct_light -= light_contrib * (max(shadow, 0.0) * (1.0 - mix(nvf_ao_spec, nvf_ao_spec_shadowed, select(0.0, shadows::nvf_shadow_fade(light_id),").count(), 2, "{case}");
                    assert_eq!(lighting.matches("let nvf_ao_spec = nvf_specular_visibility(nvf_ao_ambient, NdotV, roughness);").count(), 1, "{case}");
                    let p = |s: &str| post.find(s).unwrap_or_else(|| panic!("{case}: {s}"));
                    // After the fog (and the dither), before the alpha is premultiplied; the
                    // same position and distance as the ambient line's.
                    assert!(p("apply_fog(") < p("if nvf_ao_debug()") && p("if nvf_ao_debug()") < p("premultiply_alpha(pbr_input.material.flags"), "{case}");
                    assert!(post.contains("nvf_ambient_visibility(pbr_input.frag_coord.xy, -nvf_view_z)"));
                    assert!(r.find("import nvforever::ssao_lighting::{nvf_ambient_visibility, nvf_ao_debug, nvf_ao_multibounce, nvf_lamp_visibility, nvf_shadowed_lamp_visibility, nvf_specular_visibility};").expect("import") < r.find("fn nvf_apply_pbr_lighting(").unwrap());
                }
            }
        }
        // The debug view's distance is the lighting's `view_z`, written out the same way.
        let lighting_view_z = &hooked[hooked.find("    let view_z = dot(vec4<f32>(").unwrap()..];
        let lighting_view_z = &lighting_view_z[..lighting_view_z.find("in.world_position);").unwrap()];
        let debug_view_z = &DEBUG_TEXT[DEBUG_TEXT.find("        let nvf_view_z = dot(vec4<f32>(").unwrap()..];
        let debug_view_z = &debug_view_z[..debug_view_z.find("pbr_input.world_position);").unwrap()];
        fn rows(t: &str) -> Vec<&str> {
            t.lines().skip(1).map(str::trim).collect()
        }
        assert_eq!(rows(lighting_view_z), rows(debug_view_z));
        let wesl = wesl();
        for f in ["fn nvf_ambient_visibility(frag: vec2<f32>, dist: f32) -> f32", "fn nvf_ao_debug() -> bool", "fn nvf_lamp_visibility(g: f32) -> f32"] {
            assert!(wesl.contains(f), "ssao_lighting.wesl has {f}");
        }
        // The shadow condition the lamps' lines test is bevy_pbr's own for looking the
        // shadow up (both light loops).
        let functions = vendored("pbr_functions.wesl");
        assert_eq!(functions.matches("if ((in.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u\n                && (view_bindings::clustered_lights.data[light_id].flags & mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {").count(), 1, "the point loop's");
        assert_eq!(functions.matches("if ((in.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u\n                && (view_bindings::clustered_lights.data[light_id].flags &\n                    mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {").count(), 1, "the spot loop's");
    }

    /// The WESL's bindings are the vendored layout's (mesh_view_bindings.rs 43-44, nobody
    /// else's in group 0), and its uniform the Rust struct, field for field.
    #[test]
    fn bindings_and_uniform_match() {
        let wesl = wesl();
        let bindings = vendored("mesh_view_bindings.rs");
        assert!(wesl.contains("@group(0) @binding(43) var<uniform> nvf_ao: NvfAmbientOcclusion;"));
        assert!(wesl.contains("@group(0) @binding(44) var nvf_ao_texture: texture_2d<f32>;"));
        assert!(bindings.contains("(43, uniform_buffer_sized(false, None)),"), "binding 43 a uniform");
        assert!(bindings.contains("                44,\n                texture_2d(TextureSampleType::Float { filterable: false }),"), "binding 44 an unfilterable float 2D texture");
        assert!(bindings.contains("(43, ao.uniform.as_entire_binding()), (44, &ao.texture)"), "the bind group's entries");
        // 43 and 44 are used once each in the layout (no other feature has them).
        for n in ["(43,", "                44,\n"] {
            assert_eq!(bindings.matches(n).count(), if n.starts_with('(') { 2 } else { 1 }, "{n:?}");
        }
        // The uniform: three vec4s in the Rust order.
        let fields: Vec<usize> = ["params: vec4<f32>,", "extra: vec4<f32>,", "more: vec4<f32>,"].iter().map(|f| wesl.find(f).unwrap_or_else(|| panic!("{f}"))).collect();
        assert!(fields.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(AmbientOcclusionLightingUniform::min_size().get(), 48, "three vec4s, as ssao_lighting.wesl's struct");
        let u = AmbientOcclusionLightingUniform::new(EXPONENT, 7000.0, 0.4, 0.0, false, true);
        assert_eq!(u.params, Vec4::new(EXPONENT, 1.0, 7000.0, DISTANCE_TOLERANCE));
        assert_eq!(u.extra, Vec4::new(0.4, 0.0, 0.0, 0.0));
        assert_eq!(AmbientOcclusionLightingUniform::new(0.5, 7000.0, 0.4, 0.0, false, false).params, Vec4::new(0.5, 0.0, 7000.0, DISTANCE_TOLERANCE));
        assert_eq!(AmbientOcclusionLightingUniform::new(EXPONENT, 7000.0, 0.4, 0.5, false, false).extra, Vec4::new(0.4, 0.5, 0.0, 0.0));
        assert_eq!(AmbientOcclusionLightingUniform::new(EXPONENT, 7000.0, 0.4, 3.0, false, false).extra.y, 1.0, "held to 1");
        // The defaults built in (the player's choice, docs/design/graphics-enhanced.md §3.1.2).
        assert_eq!((EXPONENT, LAMPS), (1.0, 1.0));
        assert!(wesl.contains(&format!("const NVF_AO_MIN_WEIGHT: f32 = {MIN_WEIGHT};")), "the WESL's minimum weight is the Rust mirror's");
        // A zeroed uniform (fade end 0) reads as open everywhere.
        assert_eq!(upsample(&Texels::plane(4, 4, 0.5, 100.0), Vec2::new(2.5, 2.5), 100.0, &AmbientOcclusionLightingUniform::default()), 1.0);
    }

    /// ssao_lighting.wesl is valid WGSL by itself (it imports nothing, so no stand-ins are
    /// needed): naga parses and validates it, and both functions are there.
    #[test]
    fn the_shader_is_valid_wgsl() {
        use wgpu::naga;
        let source = wesl();
        assert!(!source.lines().any(|l| l.starts_with("import ") || l.trim_start().starts_with("@if")), "nothing for the test to resolve");
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{e:?}"));
        for f in ["nvf_ambient_visibility", "nvf_ao_debug", "nvf_lamp_visibility"] {
            assert!(module.functions.iter().any(|(_, func)| func.name.as_deref() == Some(f)), "{f}");
        }
    }

    /// A half-resolution texture for the mirror: (how open, distance) per texel.
    struct Texels {
        size: (i32, i32),
        data: Vec<(f32, f32)>,
    }

    impl Texels {
        fn plane(w: i32, h: i32, open: f32, dist: f32) -> Self {
            Texels { size: (w, h), data: vec![(open, dist); (w * h) as usize] }
        }

        fn load(&self, x: i32, y: i32) -> (f32, f32) {
            self.data[(y * self.size.0 + x) as usize]
        }

        fn set(&mut self, x: i32, y: i32, v: (f32, f32)) {
            self.data[(y * self.size.0 + x) as usize] = v;
        }
    }

    /// ssao_lighting.wesl `nvf_ambient_visibility`, step for step.
    fn upsample(tex: &Texels, frag: Vec2, dist: f32, u: &AmbientOcclusionLightingUniform) -> f32 {
        // The shader's `!(dist > 0.0)`: NaN too.
        if dist.is_nan() || dist <= 0.0 || dist >= u.params.z {
            return 1.0;
        }
        let t = (frag - 0.5) * 0.5;
        let base = t.floor();
        let f = t - base;
        let first = (base.x as i32, base.y as i32);
        let tolerance = u.params.w * dist + 1.0;
        let (mut total, mut weight) = (0.0, 0.0);
        for y in 0..2 {
            for x in 0..2 {
                let q = ((first.0 + x).clamp(0, tex.size.0 - 1), (first.1 + y).clamp(0, tex.size.1 - 1));
                let (open, d) = tex.load(q.0, q.1);
                let bilinear = (if x == 1 { f.x } else { 1.0 - f.x }) * (if y == 1 { f.y } else { 1.0 - f.y });
                let surface = if d > 0.0 { (1.0 - (d - dist).abs() / tolerance).max(0.0) } else { 0.0 };
                let w = bilinear * surface;
                total += open * w;
                weight += w;
            }
        }
        if weight.is_nan() || weight < MIN_WEIGHT {
            return 1.0;
        }
        let open = total / weight;
        let safe = if open >= 0.0 { open.clamp(0.0001, 1.0) } else { 1.0 };
        safe.powf(u.params.x)
    }

    fn uniform() -> AmbientOcclusionLightingUniform {
        AmbientOcclusionLightingUniform::new(EXPONENT, 7000.0, 0.4, 0.0, false, false)
    }

    /// On a plane every fragment gets the plane's factor exactly (at the exponent built in,
    /// 1, the factor itself), at even and odd pixels and at the borders.
    #[test]
    fn a_plane_gives_its_factor() {
        let tex = Texels::plane(8, 6, 0.7, 500.0);
        for frag in [Vec2::new(4.5, 4.5), Vec2::new(5.5, 4.5), Vec2::new(5.5, 5.5), Vec2::new(0.5, 0.5), Vec2::new(15.5, 11.5), Vec2::new(16.5, 12.5)] {
            let g = upsample(&tex, frag, 500.0, &uniform());
            assert!((g - 0.7).abs() < 1e-6, "{frag}: {g}");
        }
        // A slightly different distance on the same plane (a slope) still matches.
        let g = upsample(&tex, Vec2::new(5.5, 5.5), 510.0, &uniform());
        assert!((g - 0.7).abs() < 1e-6);
    }

    /// Across a depth edge each side keeps its own texels: the background beside a near
    /// box ignores the box's dark texels, and the box ignores the background's.
    #[test]
    fn across_an_edge_each_surface_keeps_its_own() {
        // Columns 0-1 a box at 200 units, closed (0.2); columns 2-3 a wall at 2000, open (1).
        let mut tex = Texels::plane(4, 4, 1.0, 2000.0);
        for y in 0..4 {
            for x in 0..2 {
                tex.set(x, y, (0.2, 200.0));
            }
        }
        // Full pixel 3.5 lies between texel 1 (box) and texel 2 (wall).
        let edge = Vec2::new(3.5, 3.5);
        let wall = upsample(&tex, edge, 2000.0, &uniform());
        assert_eq!(wall, 1.0, "the wall's fragment ignores the box");
        let boxed = upsample(&tex, edge, 200.0, &uniform());
        assert!((boxed - 0.2).abs() < 1e-6, "the box's fragment ignores the wall: {boxed}");
    }

    /// Where no texel holds the fragment's surface (something the depth prepass does not
    /// have: water over the bed, a pole a pixel wide) the factor is 1, not a neighbour's.
    #[test]
    fn no_match_gives_one() {
        let tex = Texels::plane(4, 4, 0.1, 3000.0);
        assert_eq!(upsample(&tex, Vec2::new(3.5, 3.5), 300.0, &uniform()), 1.0);
        // A texel on the surface but with no bilinear weight (pixel 2q exactly, its own
        // texel a different surface) does not count either.
        let mut tex = Texels::plane(4, 4, 0.1, 300.0);
        tex.set(1, 1, (0.1, 3000.0));
        assert_eq!(upsample(&tex, Vec2::new(2.5, 2.5), 300.0, &uniform()), 1.0);
    }

    /// From the fade's end on (where the occlusion pass writes 1 anyway) nothing is read.
    #[test]
    fn beyond_the_fade_gives_one() {
        let tex = Texels::plane(4, 4, 0.1, 7000.0);
        assert_eq!(upsample(&tex, Vec2::new(2.5, 2.5), 7000.0, &uniform()), 1.0);
        assert_eq!(upsample(&tex, Vec2::new(2.5, 2.5), 60000.0, &uniform()), 1.0);
        let tex = Texels::plane(4, 4, 0.1, 6999.0);
        assert!(upsample(&tex, Vec2::new(2.5, 2.5), 6999.0, &uniform()) < 1.0, "just inside the fade it is read");
        // No distance (or not a number) reads as open too.
        assert_eq!(upsample(&tex, Vec2::new(2.5, 2.5), 0.0, &uniform()), 1.0);
        assert_eq!(upsample(&tex, Vec2::new(2.5, 2.5), f32::NAN, &uniform()), 1.0);
    }

    /// A texture never written (all zero: distance 0) darkens nothing, even for a fragment
    /// right in front of the camera.
    #[test]
    fn zero_texels_give_one() {
        let tex = Texels::plane(4, 4, 0.0, 0.0);
        for dist in [0.5, 1.0, 5.0, 100.0, 6000.0] {
            assert_eq!(upsample(&tex, Vec2::new(3.5, 2.5), dist, &uniform()), 1.0, "{dist}");
        }
    }

    /// The exponent sets how the shown value falls. The display-value sum is shown as about
    /// itself (the texture's display value × the sum, lights/display_shader.rs; `shown` below
    /// stands in for it with a 2.2 power both ways), so with the exponent built in,
    /// 1, the shown value of a surface lit by what the factor reaches falls by the factor
    /// itself, as the picture placement multiplies the shown picture by it; with the first
    /// value, 1/2.2, it was the light that fell by the factor and the shown value only by
    /// f^(1/2.2).
    #[test]
    fn the_exponent_sets_how_the_shown_value_falls() {
        let tex = |f: f32| Texels::plane(4, 4, f, 300.0);
        let at = |f: f32, e: f32| upsample(&tex(f), Vec2::new(2.5, 2.5), 300.0, &AmbientOcclusionLightingUniform::new(e, 7000.0, 0.8, 0.0, false, false));
        let shown = |sum: f32| (sum.powf(2.2)).powf(1.0 / 2.2);
        for i in 1..=20 {
            let f = i as f32 / 20.0;
            // A pixel lit by the ambient alone (sum = A): shown A × g against A.
            assert!((shown(0.5 * at(f, EXPONENT)) / shown(0.5) - f).abs() < 1e-4, "exponent 1: the shown value falls by f = {f}");
            assert!((shown(0.5 * at(f, 1.0 / 2.2)) / shown(0.5) - f.powf(1.0 / 2.2)).abs() < 1e-4, "exponent 1/2.2: only by f^0.45, f = {f}");
            assert!(((0.5 * at(f, 1.0 / 2.2)).powf(2.2) / 0.5f32.powf(2.2) - f).abs() < 1e-4, "exponent 1/2.2: the light falls by f = {f}");
        }
    }

    /// ssao_lighting.wesl `nvf_lamp_visibility`, step for step.
    fn lamps(tex: &Texels, frag: Vec2, dist: f32, u: &AmbientOcclusionLightingUniform) -> f32 {
        let w = u.extra.y.clamp(0.0, 1.0);
        if w.is_nan() || w <= 0.0 {
            return 1.0;
        }
        1.0 + (upsample(tex, frag, dist, u) - 1.0) * w
    }

    /// The lamps' factor: exactly 1 with weight 0 (today's light, bit for bit), the
    /// ambient's factor with weight 1, in between linearly; on a lamp-lit crease with weight
    /// 1 the light scales by the occlusion itself, as the ambient's does.
    #[test]
    fn the_lamps_take_the_ambient_factor_by_their_weight() {
        let wesl = wesl();
        assert!(wesl.contains("let w = clamp(nvf_ao.extra.y, 0.0, 1.0);") && wesl.contains("return mix(1.0, g, w);"));
        let tex = Texels::plane(4, 4, 0.6, 300.0);
        let at = |w: f32| lamps(&tex, Vec2::new(2.5, 2.5), 300.0, &AmbientOcclusionLightingUniform::new(EXPONENT, 7000.0, 0.4, w, false, false));
        let g = upsample(&tex, Vec2::new(2.5, 2.5), 300.0, &uniform());
        assert_eq!(at(0.0), 1.0);
        assert!((at(1.0) - g).abs() < 1e-6);
        assert!((at(1.0) - 0.6).abs() < 1e-6, "a lamp-only crease: its shown value falls by the factor");
        assert!((at(0.5) - (1.0 + g) * 0.5).abs() < 1e-6);
        assert!(at(0.0) > at(0.5) && at(0.5) > at(1.0));
    }

    /// ssao_lighting.wesl `nvf_ao_multibounce` for one channel, step for step.
    fn multibounce(x: f32, albedo: f32, on: bool) -> f32 {
        if !on {
            return x;
        }
        let a = albedo.clamp(0.0, 1.0);
        let (k2, k1, k0) = (2.0404 * a - 0.3324, -4.7951 * a + 0.6417, 2.7552 * a + 0.6903);
        x.max(((x * k2 + k1) * x + k0) * x).clamp(0.0, 1.0)
    }

    /// The multi-bounce fit: the plain factor while off; never below it; an open surface
    /// stays open and a closed one closed; a lighter surface keeps more of its ambient in the
    /// same crease; the shader holds the same constants.
    #[test]
    fn the_multibounce_fit_lifts_light_surfaces_more_than_dark_ones() {
        let wesl = wesl();
        for c in ["2.0404 * a - 0.3324", "-4.7951 * a + 0.6417", "2.7552 * a + 0.6903", "max(vec3(x), ((x * k2 + k1) * x + k0) * x)"] {
            assert!(wesl.contains(c), "{c}");
        }
        const { assert!(MULTIBOUNCE) };
        for x in [0.0f32, 0.3, 0.6, 0.9, 1.0] {
            assert_eq!(multibounce(x, 0.7, false), x);
            for a in [0.0f32, 0.2, 0.5, 0.8, 1.0] {
                assert!(multibounce(x, a, true) >= x - 1e-6, "never below the factor ({x}, {a})");
            }
        }
        assert!((multibounce(1.0, 0.5, true) - 1.0).abs() < 1e-3, "open stays open");
        assert_eq!(multibounce(0.0, 0.9, true), 0.0, "closed stays closed");
        let (dark, light) = (multibounce(0.5, 0.1, true), multibounce(0.5, 0.9, true));
        assert!(light > dark + 0.1, "a light surface keeps more: {light} against {dark}");
        assert!((dark - 0.5).abs() < 0.03, "a dark one about the factor itself: {dark}");
    }

    #[test]
    fn a_highlight_in_a_crease_dims_and_an_open_one_does_not() {
        // Open: whole, whatever the view or the roughness.
        for (v, r) in [(1.0, 0.0), (0.3, 0.5), (0.05, 1.0)] {
            assert_eq!(specular_visibility(1.0, v, r, 1.0), 1.0);
        }
        // A rough surface's highlight falls as its diffuse light does.
        assert!((specular_visibility(0.5, 0.7, 1.0, 1.0) - 0.5).abs() < 1e-3);
        // A smooth one keeps more looked at straight on, and loses it at a grazing view.
        let (straight, grazing) = (specular_visibility(0.5, 1.0, 0.0, 1.0), specular_visibility(0.5, 0.1, 0.0, 1.0));
        assert!(straight > 0.7 && grazing < 0.3 && grazing > 0.0, "{straight} {grazing}");
        // Never brighter, never negative; the weight scales it and 0 leaves the highlight whole.
        for g in [0.0, 0.2, 0.6, 1.0] {
            for v in [0.0, 0.4, 1.0] {
                for r in [0.0, 0.3, 1.0] {
                    let f = specular_visibility(g, v, r, 1.0);
                    assert!((0.0..=1.0).contains(&f), "{g} {v} {r}: {f}");
                    assert_eq!(specular_visibility(g, v, r, 0.0), 1.0);
                    assert!((specular_visibility(g, v, r, 0.5) - (1.0 + f) * 0.5).abs() < 1e-6);
                }
            }
        }
        assert_eq!(HIGHLIGHTS, 1.0);
    }
}
