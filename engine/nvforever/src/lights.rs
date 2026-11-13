//! Light sources of LIGH references (docs/spec-notes/world.md §8). The game's lights
//! brighten surfaces by colour × fade × attenuation × N·L in the display units of the sky
//! lighting (sky.rs `light_unit`), with an attenuation that reaches 0 at the light's
//! radius [hypothesis]. Bevy's physically based point lights fall off with the inverse
//! square of the distance instead: a hot spot at the bulb and little light a few feet
//! away. [`LightsPlugin`] replaces Bevy's distance attenuation (`getDistanceAttenuation`
//! in `bevy_pbr::lighting`) with the game's, and [`spawn_light`] sets intensities in
//! display units.

use bevy::prelude::*;
use bevy::shader::Shader;
use fnv_world::lighting::{light_data, light_flags, placed_light_radius, ref_xrds};
use fnv_world::placed::Placement;
use nvf_play::RefVisual;
use std::f32::consts::PI;

use crate::world::WorldData;

/// The embedded asset path under which bevy_pbr registers `bevy_pbr::lighting`.
const PBR_LIGHTING: &str = "embedded://bevy_pbr/render/pbr_lighting.wesl";

/// Bevy's attenuation: windowed inverse square [observed: bevy_pbr 0.20.1].
const BEVY_ATTENUATION: &str = "return getRangeFalloff(distanceSquare, inverseRangeSquared) * 1.0 / max(distanceSquare, 0.0001);";

/// The game's attenuation: 1 − (d / radius)², clamped [hypothesis: shape to calibrate
/// against in-game captures; the LIGH falloff exponent, 1 for most lights, is not used].
const GAME_ATTENUATION: &str = "return saturate(1.0 - distanceSquare * inverseRangeSquared);";

/// The embedded asset path under which bevy_pbr registers `bevy_pbr::pbr_functions`.
const PBR_FUNCTIONS: &str = "embedded://bevy_pbr/render/pbr_functions.wesl";

/// The display-value lighting sum (docs/spec-notes/world.md §8 "Light on surfaces in
/// display values") [hypothesis, fitted to the original's frames]: the game adds its
/// lights as display values (colour bytes / 255) — ambient + Σ sun and lights × fade ×
/// attenuation × N·L — and multiplies the texture by the sum, where Bevy adds linear
/// light. In a world view (marked by a negative ambient alpha, [`display_ambient`]) the
/// patched `apply_pbr_lighting` leaves Bevy's diffuse terms out (diffuse colour 0 for the
/// lights and the ambient light, so only their specular parts stay), forms the sum from
/// the same lights' colours, which hold display values there ([`display_color`]), and
/// adds diffuse colour × sum^2.2 (the display gamma) as light. Views without the mark
/// (the minigame views) keep Bevy's sum. `nvf_apply_pbr_lighting` takes weights for the
/// ambient and the direct part of the sum (grass.wesl); `apply_pbr_lighting` is it with
/// weights 1. Each helper carries the condition Bevy puts on `apply_pbr_lighting`
/// (`@if(!PREPASS_FRAGMENT)`), whose line the first edit takes in.
const DISPLAY_HELPERS: &str = "// NVForever (nvforever lights.rs DISPLAY_HELPERS): the game's lighting sum in display values.
@if(!PREPASS_FRAGMENT)
fn nvf_display_lighting() -> bool {
    return view_bindings::lights.ambient_color.a < 0.0;
}

// A clustered point or spot light's diffuse light on a surface at P facing N, as display
// values: colour × fade × attenuation × N.L (the uniform holds colour × fade × pi × light
// unit, nvforever lights.rs `lumens`).
@if(!PREPASS_FRAGMENT)
fn nvf_point_display(light_id: u32, P: vec3<f32>, N: vec3<f32>) -> vec3<f32> {
    let light = &view_bindings::clustered_lights.data[light_id];
    let to_light = (*light).position_radius.xyz - P;
    let d2 = dot(to_light, to_light);
    let attenuation = lighting::getDistanceAttenuation(d2, (*light).color_inverse_square_range.w);
    let n_dot_l = saturate(dot(N, to_light * inverseSqrt(max(d2, 0.0001))));
    return (*light).color_inverse_square_range.rgb * (attenuation * n_dot_l * view_bindings::view.exposure / 3.14159265);
}

// The same for a spot light, with Bevy's cone falloff (bevy_pbr::lighting::spot_light).
@if(!PREPASS_FRAGMENT)
fn nvf_spot_display(light_id: u32, P: vec3<f32>, N: vec3<f32>) -> vec3<f32> {
    let light = &view_bindings::clustered_lights.data[light_id];
    var spot_dir = vec3<f32>((*light).light_custom_data.x, 0.0, (*light).light_custom_data.y);
    spot_dir.y = sqrt(max(0.0, 1.0 - spot_dir.x * spot_dir.x - spot_dir.z * spot_dir.z));
    if ((*light).flags & mesh_view_types::POINT_LIGHT_FLAGS_SPOT_LIGHT_Y_NEGATIVE) != 0u {
        spot_dir.y = -spot_dir.y;
    }
    let cd = dot(-spot_dir, normalize((*light).position_radius.xyz - P));
    let cone = saturate(cd * (*light).light_custom_data.z + (*light).light_custom_data.w);
    return nvf_point_display(light_id, P, N) * (cone * cone);
}

// What is left of Bevy's ambient light in a world view, its highlight, takes the ambient
// colour as linear light: the factor from the display value to it (a^2.2 / a).
@if(!PREPASS_FRAGMENT)
fn nvf_ambient_ratio() -> vec3<f32> {
    return pow(max(view_bindings::lights.ambient_color.rgb * view_bindings::view.exposure, vec3(1e-6)), vec3(1.2));
}

// How far the remaster leans the outdoor light towards the sun (0..1): none at a low sun
// (dawn and dusk keep their soft shade), all from 35 degrees up [engineering choice].
@if(!PREPASS_FRAGMENT)
fn nvf_lean_at(i: u32) -> f32 {
    return smoothstep(0.2, 0.57, view_bindings::lights.directional_lights[i].direction_to_light.y);
}

@if(!PREPASS_FRAGMENT)
fn nvf_sun_cloud(i: u32, p: vec3<f32>) -> f32 {
    @if(SCREEN_INDIRECT) {
        let light = &view_bindings::lights.directional_lights[i];
        if ((*light).flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
            // No cloud shadow on surfaces: shapes drawn without the remaster's shader def
            // (some LOD) stayed sunlit inside a cloud, as straight-edged patches; the fog
            // keeps its clouds (atmos.wesl).
            return 1.0;
        }
    }
    // 2: no cloud and no lean (the caller reads 1.5 or more as not the sun).
    return 2.0;
}

// A directional light's diffuse light on a surface facing N, as display values.
@if(!PREPASS_FRAGMENT)
fn nvf_directional_display(i: u32, N: vec3<f32>) -> vec3<f32> {
    let light = &view_bindings::lights.directional_lights[i];
    return (*light).color.rgb * (saturate(dot(N, (*light).direction_to_light.xyz)) * view_bindings::view.exposure / 3.14159265);
}

@if(!PREPASS_FRAGMENT)
fn apply_pbr_lighting(
    in: pbr_types::PbrInput,
) -> vec4<f32> {
    return nvf_apply_pbr_lighting(in, vec2(1.0));
}

@if(!PREPASS_FRAGMENT)
fn nvf_apply_pbr_lighting(
    in: pbr_types::PbrInput,
    nvf_weights: vec2<f32>,
) -> vec4<f32> {";

/// Replacements in `apply_pbr_lighting` [observed: bevy_pbr 0.20.1 `pbr_functions.wesl`],
/// in order; the light loops' line occurs three times (point, spot, directional).
const DISPLAY_EDITS: [(&str, &str); 7] = [
    ("@if(!PREPASS_FRAGMENT)\nfn apply_pbr_lighting(\n    in: pbr_types::PbrInput,\n) -> vec4<f32> {", DISPLAY_HELPERS),
    (
        "    lighting_input.diffuse_color = diffuse_color;",
        "    let nvf_display = nvf_display_lighting();
    var nvf_direct = vec3(0.0);
    // The screen indirect light (gi.rs): how open the pixel is to the ambient light, and
    // the light bounced onto it from the surfaces around it (linear, after exposure).
    // Point and spot lights cast no shadows, so they take part of the occlusion too.
    var nvf_open = vec3(1.0);
    // A sun that casts shadows lights the view (outdoors): the remaster lights it up to
    // 1.15x as it climbs, a harder desert light (nvf_lean_at); the ambient stays.
    var nvf_lean = 0.0;
    var nvf_local_open = 1.0;
    var nvf_bounce = vec3(0.0);
    @if(SCREEN_INDIRECT) {
        let nvf_si = textureLoad(view_bindings::screen_indirect_texture, vec2<i32>(in.frag_coord.xy - view_bindings::view.viewport.xy), 0);
        // A debug view of the texture itself (gi.rs, NVF_GI's debug view).
        if nvf_si.a < -0.5 {
            return vec4(nvf_si.rgb, 1.0);
        }
        // The ambient light from above and below (sky and ground), by the surface's
        // normal-mapped normal, so shapes read in the shade too [engineering choice].
        let nvf_up = saturate(in.N.y * 0.5 + 0.5);
        // The openness acts on linear light: the display sum is raised to 2.2 after it.
        nvf_open = pow(max(1.0 - nvf_si.a, 0.0), 1.0 / 2.2) * mix(vec3(0.80, 0.74, 0.68), vec3(1.10, 1.12, 1.17), nvf_up);
        nvf_local_open = pow(max(1.0 - 0.6 * nvf_si.a, 0.0), 1.0 / 2.2);
        nvf_bounce = nvf_si.rgb;
    }
    lighting_input.diffuse_color = select(diffuse_color, vec3(0.0), nvf_display);",
    ),
    (
        "        direct_light += light_contrib * shadow;",
        "        direct_light += light_contrib * shadow;
        if nvf_display { nvf_direct += nvf_point_display(light_id, in.world_position.xyz, in.N) * shadow * nvf_local_open; }",
    ),
    (
        "        direct_light += light_contrib * shadow;",
        "        direct_light += light_contrib * shadow;
        if nvf_display { nvf_direct += nvf_spot_display(light_id, in.world_position.xyz, in.N) * shadow * nvf_local_open; }",
    ),
    (
        "        direct_light += light_contrib * shadow;",
        "        direct_light += light_contrib * shadow;
        if nvf_display {
            let nvf_cloud = nvf_sun_cloud(i, in.world_position.xyz);
            let nvf_sun_lean = select(nvf_lean_at(i), 0.0, nvf_cloud >= 1.5);
            nvf_lean = max(nvf_lean, nvf_sun_lean);
            nvf_direct += nvf_directional_display(i, in.N) * shadow * select(nvf_cloud * (1.0 + 0.15 * nvf_sun_lean), 1.0, nvf_cloud >= 1.5);
        }",
    ),
    (
        "        indirect_light += ambient::ambient_light(in.world_position, in.N, in.V, NdotV, diffuse_color, F0, perceptual_roughness, diffuse_occlusion);",
        "        indirect_light += ambient::ambient_light(in.world_position, in.N, in.V, NdotV, select(diffuse_color, vec3(0.0), nvf_display), F0, perceptual_roughness, diffuse_occlusion) * select(vec3(1.0), nvf_ambient_ratio(), nvf_display);",
    ),
    (
        "    // Total light\n",
        "    @if(SCREEN_INDIRECT) {
        // The remaster's materials (graphics-remaster.md §4): the lights' highlights a
        // little softer, and the sky (bright above, the ground's colour below) reflected
        // by the specular mask's F0 with Schlick's Fresnel and the roughness, hidden where
        // the surroundings close the view (gi.rs) [engineering choice, by eye].
        if nvf_display {
            direct_light *= 0.7;
            let nvf_amb = pow(max(view_bindings::lights.ambient_color.rgb * view_bindings::view.exposure, vec3(0.0)), vec3(2.2));
            // Grey (the ambient light's brightness, not its tint) and only on smooth
            // surfaces: rock, rust and wood are too rough to mirror the sky.
            let nvf_amb_l = dot(nvf_amb, vec3(0.2126, 0.7152, 0.0722));
            let nvf_sky = vec3(nvf_amb_l) * mix(0.25, 1.2, saturate(R.y * 0.6 + 0.5));
            let nvf_smooth = 1.0 - smoothstep(0.3, 0.6, perceptual_roughness);
            let nvf_env = lighting::EnvBRDFApprox(F0, F_ab) * nvf_sky * dot(nvf_open, vec3(0.3333)) * specular_occlusion * nvf_smooth;
            direct_light += nvf_env / view_bindings::view.exposure;
        }
    }
    direct_light += diffuse_color * nvf_bounce / view_bindings::view.exposure;
    if !nvf_display {
        indirect_light *= nvf_open;
    }
    if nvf_display {
        var nvf_ambient = view_bindings::lights.ambient_color.rgb * view_bindings::view.exposure * diffuse_occlusion * nvf_open;
        // Open shade under a high sun is lit by the blue sky: the ambient light leans cooler
        // as the sun climbs (remaster) [engineering choice].
        nvf_ambient *= mix(vec3(1.0), vec3(0.965, 1.0, 1.07), nvf_lean);
        let nvf_sum = max(nvf_weights.x * nvf_ambient + nvf_weights.y * nvf_direct, vec3(0.0));
        // Held to 1 per channel, as the original's lit colour is (lights.rs GAME_CLIP).
        direct_light += min(diffuse_color * pow(nvf_sum, vec3(2.2)), vec3(1.0)) / view_bindings::view.exposure;
    }

    // Total light\n",
    ),
];

/// `source` with [`DISPLAY_EDITS`] made, each to the first occurrence after the last
/// edit; `None` when one is missing (Bevy changed).
fn display_patched(source: &str) -> Option<String> {
    let mut out = String::with_capacity(source.len() + 4096);
    let mut rest = source;
    for (from, to) in DISPLAY_EDITS {
        let at = rest.find(from)?;
        out.push_str(&rest[..at]);
        out.push_str(to);
        rest = &rest[at + from.len()..];
    }
    out.push_str(rest);
    Some(out)
}

/// A light colour of a world view as display values (bytes / 255, or a model's stored
/// colour): handed to Bevy unconverted, for the display-value sum ([`DISPLAY_HELPERS`]).
pub fn display_color(c: [f32; 3]) -> Color {
    Color::LinearRgba(LinearRgba::rgb(c[0], c[1], c[2]))
}

/// The display value (0..1, sRGB) of a linear colour channel: the inverse of
/// `fnv_world::sky::srgb_to_linear`, so a record's byte b comes back as b / 255.
pub fn linear_to_display(v: f32) -> f32 {
    let v = v.max(0.0);
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

/// A world view's ambient light colour (display values) for `GlobalAmbientLight`, with
/// the negative alpha that marks the view for the display-value sum.
pub fn display_ambient(c: [f32; 3]) -> Color {
    Color::LinearRgba(LinearRgba::new(c[0], c[1], c[2], -1.0))
}

/// The embedded asset path under which bevy_core_pipeline registers its tonemapping
/// functions (`bevy_core_pipeline::tonemapping`).
const TONEMAPPING_SHARED: &str = "embedded://bevy_core_pipeline/tonemapping.wesl";

/// Bevy's Tony McMapface curve [observed: bevy_core_pipeline 0.20.1 `tonemapping.wesl`],
/// the main view's tonemapper (every other view here has none).
const BEVY_TONY: &str = "} @elif(TONEMAP_METHOD_TONY_MC_MAPFACE) {\n    color = sample_tony_mc_mapface_lut(color);";

/// The original's display: its lit colour, a display value, is shown as it is and held to
/// 1 per channel (world.md §8 "Light on surfaces in display values") [hypothesis, from the
/// original's frames: the Dead Money lantern's wall clips to (255, 255, 255) and, under
/// Dead Money's weather, to (255, 255, 101), saturated, where Tony McMapface turns bright
/// light pale and never reaches white]. The tonemapping pass keeps running, and with it
/// the eye adaptation's exposure (eye_adapt.rs), which Bevy applies in that pass.
const GAME_CLIP: &str = "} @elif(TONEMAP_METHOD_TONY_MC_MAPFACE) {\n    color = min(color, vec3(1.0));";

/// Set once the clip is in place (`NVF_TONEMAP=tony` keeps Bevy's curve, a test aid);
/// sky.rs then leaves out its inverse of Tony McMapface.
static CLIP_INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the main view shows display values clipped (see [`GAME_CLIP`]).
pub fn display_clip() -> bool {
    CLIP_INSTALLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Bevy's shader text with Windows line ends made `\n`: the patches' anchors are written
/// with `\n`. A checkout with git's `core.autocrlf` gave the bevy_pbr vendored until #939
/// `\r\n` (issue #946: no patch matched, so grass lost `nvf_apply_pbr_lighting`); Bevy
/// now comes from crates.io as published, and this stays as a guard for any patched copy.
pub fn unix_lines(source: &str) -> String {
    source.replace("\r\n", "\n")
}

pub struct LightsPlugin;

impl Plugin for LightsPlugin {
    fn build(&self, app: &mut App) {
        let handle: Handle<Shader> = app.world().resource::<AssetServer>().load(PBR_LIGHTING);
        let functions: Handle<Shader> = app.world().resource::<AssetServer>().load(PBR_FUNCTIONS);
        let tonemapping: Handle<Shader> = app.world().resource::<AssetServer>().load(TONEMAPPING_SHARED);
        app.insert_resource(LightingShader { handle, patched: false })
            .insert_resource(FunctionsShader { handle: functions, patched: false })
            .insert_resource(TonemapShader { handle: tonemapping, patched: false })
            .add_systems(PreUpdate, patch_clip)
            .add_systems(Update, (patch_attenuation, patch_display_sum, animate_lights, bind_model_lights, animate_model_lights).chain());
    }
}

#[derive(Resource)]
struct TonemapShader {
    handle: Handle<Shader>,
    patched: bool,
}

/// Once Bevy's tonemapping functions have loaded, swap Tony McMapface's curve for the
/// clip; the sky system is told to set its colours again.
fn patch_clip(mut state: ResMut<TonemapShader>, mut shaders: ResMut<Assets<Shader>>, sky: Option<ResMut<crate::sky::SkyState>>) {
    if state.patched {
        return;
    }
    let Some(shader) = shaders.get(&state.handle) else { return };
    state.patched = true;
    if std::env::var("NVF_TONEMAP").is_ok_and(|v| v.eq_ignore_ascii_case("tony")) {
        info!("lights: NVF_TONEMAP=tony, the main view keeps Tony McMapface");
        return;
    }
    let bevy::shader::Source::Wesl(source) = &shader.source else {
        error!("{TONEMAPPING_SHARED}: not WESL; the main view keeps Tony McMapface");
        return;
    };
    let source = unix_lines(source);
    if !source.contains(BEVY_TONY) {
        error!("{TONEMAPPING_SHARED}: Tony McMapface not found (Bevy changed?); the main view keeps it");
        return;
    }
    let patched = crate::fog::with_source(shader, source.replace(BEVY_TONY, GAME_CLIP));
    let _ = shaders.insert(&state.handle, patched);
    CLIP_INSTALLED.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(mut sky) = sky {
        sky.refresh();
    }
    info!("lights: display values shown clipped (in Tony McMapface's place)");
}

#[derive(Resource)]
struct FunctionsShader {
    handle: Handle<Shader>,
    patched: bool,
}

/// Once bevy_pbr's `pbr_functions` has loaded, put the display-value sum into
/// `apply_pbr_lighting`. Pipelines that already use it are rebuilt.
fn patch_display_sum(mut state: ResMut<FunctionsShader>, mut shaders: ResMut<Assets<Shader>>) {
    if state.patched {
        return;
    }
    let Some(shader) = shaders.get(&state.handle) else { return };
    state.patched = true;
    let bevy::shader::Source::Wesl(source) = &shader.source else {
        error!("{PBR_FUNCTIONS}: not WESL; surfaces keep Bevy's linear lighting sum");
        return;
    };
    let source = unix_lines(source);
    let Some(patched) = display_patched(&source) else {
        error!("{PBR_FUNCTIONS}: apply_pbr_lighting not as expected (Bevy changed?); surfaces keep Bevy's linear lighting sum");
        return;
    };
    let patched = crate::fog::with_source(shader, patched);
    let _ = shaders.insert(&state.handle, patched);
    info!("lights: display-value lighting sum installed");
}

/// How a light's brightness moves by itself: LIGH DATA flags 3 "Flicker", 6 "Flicker
/// Slow", 7 "Pulse" and 8 "Pulse Slow" [documented: xEdit LIGH DATA flags; GECK wiki
/// Light: "Pulse and Pulse Slow makes the light pulsate, growing brighter and dimming.
/// Flicker and Flicker Slow gives the light a flickering effect, such as a candle would
/// emit"]. 655 placed lights of FalloutNV.esm flicker (fire barrels, broken tubes) and
/// 209 pulse [observed: `crates/fnv-world/examples/light_flags_survey.rs`,
/// docs/spec-notes/effects-animation-audit.md §4].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightMotion {
    Flicker,
    FlickerSlow,
    Pulse,
    PulseSlow,
}

impl LightMotion {
    /// The motion a LIGH's DATA flags give it (the first of the four flags set).
    pub fn of(flags: u32) -> Option<LightMotion> {
        [(light_flags::FLICKER, LightMotion::Flicker), (1 << 6, LightMotion::FlickerSlow), (1 << 7, LightMotion::Pulse), (1 << 8, LightMotion::PulseSlow)]
            .into_iter()
            .find(|(f, _)| flags & f != 0)
            .map(|(_, m)| m)
    }

    /// The share of its brightness the light gives at `t` seconds, `seed` keeping
    /// lights out of step [hypothesis: the curves are not documented; a flicker is
    /// smoothed random steps between [`FLICKER_LOW`] and 1, 12 a second (slow: 3), a pulse a sine
    /// between 0.5 and 1 over 1.5 s (slow: 4 s); the record's fade is the brightest it
    /// gets. Owner review: docs/owner-review.md, "Light flicker and pulse"].
    pub fn factor(self, t: f32, seed: f32) -> f32 {
        match self {
            LightMotion::Flicker => FLICKER_LOW + (1.0 - FLICKER_LOW) * noise(t * 12.0 + seed * 97.0),
            LightMotion::FlickerSlow => FLICKER_LOW + (1.0 - FLICKER_LOW) * noise(t * 3.0 + seed * 97.0),
            LightMotion::Pulse => 0.75 + 0.25 * (std::f32::consts::TAU * (t / 1.5 + seed)).sin(),
            LightMotion::PulseSlow => 0.75 + 0.25 * (std::f32::consts::TAU * (t / 4.0 + seed)).sin(),
        }
    }
}

/// The dimmest a flickering light gets, as a share of its brightness [hypothesis, fitted
/// to the owner's frames of the Abandoned Shack's fire barrel (2026-10-27), where the
/// glow on the wall behind it stays steady across five frames; 0.55 before].
const FLICKER_LOW: f32 = 0.8;

/// How much further than its record radius a flickering light (a fire) reaches
/// [hypothesis, fitted to the same frames: the barrel's 128-unit light warms the wall
/// behind it about 190 units out from the flames; owner 2026-10-27: light sources like
/// the fire should brighten the place]. Steady lights keep the record radius.
pub const FLAME_REACH: f32 = 1.5;

/// Smoothed random steps in 0..1: a value per whole `x`, eased between neighbours.
fn noise(x: f32) -> f32 {
    let hash = |i: f32| ((i * 12.9898).sin() * 43_758.547).rem_euclid(1.0);
    let i = x.floor();
    let f = x - i;
    let f = f * f * (3.0 - 2.0 * f);
    hash(i) + (hash(i + 1.0) - hash(i)) * f
}

/// Leaves out a reference's light sources: they carry its [`RefVisual`] to follow its
/// enabled state ([`spawn_light`]), but are no part of its model, neither its root nor a
/// shape to swap for another model's.
pub type NotLightSource = (Without<PointLight>, Without<SpotLight>);

/// On a placed light that flickers or pulses: its motion, full intensity and phase.
#[derive(Component, Debug, Clone, Copy)]
pub struct MovingLight {
    pub motion: LightMotion,
    pub full: f32,
    pub seed: f32,
}

/// Flicker and pulse the lights that do so.
fn animate_lights(time: Res<Time>, mut lights: Query<(&MovingLight, Option<&mut PointLight>, Option<&mut SpotLight>)>, mut seen: Local<usize>) {
    let t = time.elapsed_secs_wrapped();
    let n = lights.iter().len();
    if n != *seen {
        *seen = n;
        info!("lights: {n} flickering or pulsing");
    }
    for (m, point, spot) in &mut lights {
        let i = m.full * m.motion.factor(t, m.seed);
        if let Some(mut p) = point {
            p.intensity = i;
        }
        if let Some(mut s) = spot {
            s.intensity = i;
        }
    }
}

/// On a LIGH reference's light whose model has a sequence driving that light's colour or
/// dimmer (docs/spec-notes/model-lights.md): the reference, the light in the model, and,
/// once the model's rig is drawn ([`LightRig`]), the rig root whose [`Playing`] clip it
/// follows.
///
/// [`Playing`]: nvf_assets::anim_play::Playing
#[derive(Component, Debug, Clone)]
pub struct ModelDrivenLight {
    pub reference: u32,
    pub light: nvf_assets::model::ModelLight,
    pub root: Option<Entity>,
}

/// On the root of a LIGH reference's drawn rig whose clips drive its light (world.rs):
/// the reference and the rig node the light hangs from.
#[derive(Component, Debug, Clone, Copy)]
pub struct LightRig {
    pub reference: u32,
    pub anchor: Entity,
}

/// Hang each model-driven point light from its rig's node, where the model holds the
/// light: it then moves as the model's sequences move that node (the Vault klaxon's
/// light turns 224 units out on its spinning `Dummy`, model-lights.md §2). Spot lights
/// keep their place and aim [design: no base-game model drives a spot light].
fn bind_model_lights(mut commands: Commands, rigs: Query<(Entity, &LightRig), Added<LightRig>>, mut lights: Query<(Entity, &mut ModelDrivenLight), With<PointLight>>) {
    if rigs.is_empty() {
        return;
    }
    for (root, rig) in &rigs {
        let Some((e, mut l)) = lights.iter_mut().find(|(_, l)| l.reference == rig.reference && l.root.is_none()) else { continue };
        l.root = Some(root);
        commands.entity(e).insert((ChildOf(rig.anchor), l.light.local));
        debug!("model lights: 0x{:08X} light {} hangs from {}", rig.reference, l.light.name, l.light.parent.as_deref().unwrap_or("the root"));
    }
}

/// Colour the model-driven lights by their rig's playing clip: the diffuse key times
/// the dimmer (its key, else the stored dimmer), held to 0..1, as display values like the
/// LIGH colour, for the display-value lighting sum ([`display_color`]) [hypothesis: as the
/// model colour of a black LIGH colour, world.md §8]. A light no
/// playing clip drives keeps the colour it was spawned with (the klaxon until its alarm
/// script plays `Left`) [hypothesis, model-lights.md §3].
fn animate_model_lights(
    roots: Query<&nvf_assets::anim_play::Playing>,
    mut lights: Query<(&ModelDrivenLight, &mut PointLight), Without<crate::emittance::LightEmittance>>,
    mut seen: Local<(usize, usize)>,
) {
    use nvf_assets::anim_play::LightTarget;
    let mut driven = 0;
    for (l, mut p) in &mut lights {
        let Some(playing) = l.root.and_then(|r| roots.get(r).ok()) else { continue };
        let Some(c) = playing.light_value(&l.light.name, LightTarget::Diffuse) else { continue };
        driven += 1;
        let dimmer = playing.light_value(&l.light.name, LightTarget::Dimmer).map_or(l.light.dimmer, |v| v[0]);
        let color = display_color(c.map(|v| (v * dimmer.max(0.0)).clamp(0.0, 1.0)));
        if p.color != color {
            p.color = color;
        }
    }
    let n = (lights.iter().len(), driven);
    if n != *seen {
        *seen = n;
        info!("model lights: {} following their models, {} coloured by a playing clip", n.0, n.1);
    }
}

#[derive(Resource)]
struct LightingShader {
    handle: Handle<Shader>,
    patched: bool,
}

/// Once bevy_pbr's lighting shader has loaded, swap its distance attenuation for the
/// game's. Pipelines that already use it are rebuilt.
fn patch_attenuation(mut state: ResMut<LightingShader>, mut shaders: ResMut<Assets<Shader>>) {
    if state.patched {
        return;
    }
    let Some(shader) = shaders.get(&state.handle) else { return };
    state.patched = true;
    let bevy::shader::Source::Wesl(source) = &shader.source else {
        warn!("{PBR_LIGHTING}: not WESL; lights keep Bevy's inverse-square falloff");
        return;
    };
    let source = unix_lines(source);
    if !source.contains(BEVY_ATTENUATION) {
        warn!("{PBR_LIGHTING}: distance attenuation not found (Bevy changed?); lights keep Bevy's inverse-square falloff");
        return;
    }
    let mut patched = source.replace(BEVY_ATTENUATION, GAME_ATTENUATION);
    if SPECULAR_EDITS.iter().all(|(from, _)| patched.matches(from).count() == 1) {
        for (from, to) in SPECULAR_EDITS {
            patched = patched.replace(from, to);
        }
    } else {
        error!("{PBR_LIGHTING}: light colour lines not found (Bevy changed?); highlights take display colours as light");
    }
    let patched = crate::fog::with_source(shader, patched);
    let _ = shaders.insert(&state.handle, patched);
    info!("lights: game attenuation installed");
}

/// Bevy's point and directional lights apply their colour to what is left of their light
/// in a world view, the specular highlight ([`DISPLAY_HELPERS`]: the diffuse part is
/// summed apart). There the colour holds display values: the highlight takes its linear
/// light, the display value raised to the display gamma, as before the display-value sum
/// [observed: bevy_pbr 0.20.1 `pbr_lighting.wesl`; the helper goes in front of
/// `getDistanceAttenuation`]. Without it, lamps put white sparkles on the floors of
/// Jacobstown's lodge that the original does not show.
const SPECULAR_EDITS: [(&str, &str); 3] = [
    (
        "fn getDistanceAttenuation(",
        "// NVForever (nvforever lights.rs SPECULAR_EDITS): a light's colour uniform as linear light;
// in a world view it holds display values (scaled by pi and the light unit).
fn nvf_light_colour(c: vec3<f32>) -> vec3<f32> {
    if view_bindings::lights.ambient_color.a >= 0.0 {
        return c;
    }
    let k = view_bindings::view.exposure / PI;
    return pow(max(c * k, vec3(0.0)), vec3(2.2)) / k;
}

fn getDistanceAttenuation(",
    ),
    (
        "    return color_times_NdotL * (*light).color_inverse_square_range.rgb *
        rangeAttenuation * texture_sample;",
        "    return color_times_NdotL * nvf_light_colour((*light).color_inverse_square_range.rgb) *
        rangeAttenuation * texture_sample;",
    ),
    ("color *= (*light).color.rgb * texture_sample;", "color *= nvf_light_colour((*light).color.rgb) * texture_sample;"),
];

/// Bevy lumens for a light that adds `brightness` × colour × attenuation × N·L on screen:
/// Bevy divides lumens by 4π (candela) and Lambert diffuse by π; the camera exposure is
/// undone by the light unit.
pub fn lumens(brightness: f32, unit: f32) -> f32 {
    4.0 * PI * PI * brightness * unit
}

/// A LIGH reference's light: a point light (spot light for spot types) with the light's
/// colour as display values [documented: LIGH DATA], brightness fade × `scale`, and as range the base
/// radius plus the reference's XRDS (world.md §8 "Placed light radius"), at the
/// reference (`rt`, its transform in the cell) or on its model's `AttachLight` node.
/// Negative and off-by-default lights are not drawn. A reference with an emittance
/// source (XEMI) has its colour scaled by it (emittance.rs). A black record colour takes
/// the colour of the light inside the LIGH's model (`model`, as stored under `meshes\`)
/// when it has one (world.md §8 "Black LIGH colours"). A light whose model drives its
/// colour by a sequence gets [`ModelDrivenLight`] (docs/spec-notes/model-lights.md) and is
/// drawn even when both colours are black: its keys light it. The source carries
/// [`RefVisual`], so `RefVisual` sync puts it out and lights it again with the reference
/// (docs/spec-notes/model-lights.md §2).
#[allow(clippy::too_many_arguments)]
pub fn spawn_light(
    commands: &mut Commands,
    cell_e: Entity,
    world: &WorldData,
    ligh: u32,
    model: Option<&str>,
    models: &mut nvf_assets::model::ModelCtx,
    rt: bevy::math::Affine3A,
    scale: f32,
    placed: &Placement,
    emittance: &mut crate::emittance::EmittanceState,
) -> bool {
    let Some(l) = light_data(world.lo, world.schema, ligh) else { return false };
    let radius = placed_light_radius(l.radius, ref_xrds(world.lo, world.schema, placed.form_id));
    if l.flags & (light_flags::NEGATIVE | light_flags::OFF_BY_DEFAULT) != 0 || radius <= 0.0 {
        return false;
    }
    let from_model = (l.color == [0; 3]).then(|| model.and_then(|m| models.model_light_colour(&format!(r"meshes\{m}")))).flatten();
    let prepared = model.and_then(|m| models.model(&format!(r"meshes\{m}")));
    let model_driven = prepared.as_ref().and_then(|pm| pm.light.clone());
    // The light sits on the model's `AttachLight` node when it has one (a fire barrel's
    // flames, 102 units over its foot), else at the reference [hypothesis,
    // model-lights.md §4].
    let at: Vec3 = prepared.as_ref().and_then(|pm| nvf_assets::model::attach_light(&pm.nodes)).map_or(rt.translation, |a| (rt * a).translation).into();
    let shown = match from_model {
        // The NIF holds the colour as stored (display values) [hypothesis, as LIGH DATA].
        Some(c) => c.map(|v| v.clamp(0.0, 1.0)),
        None if l.color == [0; 3] && model_driven.is_none() => return false,
        None => l.color.map(|b| b as f32 / 255.0),
    };
    let model_driven = model_driven.map(|light| ModelDrivenLight { reference: placed.form_id, light, root: None });
    let color = display_color(shown);
    let driven = placed.emittance.and_then(|x| emittance.light(world, x, LinearRgba::rgb(shown[0], shown[1], shown[2])));
    let intensity = lumens(l.fade.max(0.0) * scale, crate::sky::light_unit());
    let radius = if matches!(LightMotion::of(l.flags), Some(LightMotion::Flicker | LightMotion::FlickerSlow)) { radius * FLAME_REACH } else { radius };
    let t = Transform::from_translation(at);
    // Lights out of step with each other: a phase from the reference.
    let moving = LightMotion::of(l.flags).map(|motion| MovingLight { motion, full: intensity, seed: (placed.form_id.wrapping_mul(2_654_435_761) >> 8) as f32 / 16_777_216.0 });
    // Neither kind casts shadows. If one ever does, give it the sun's render layers
    // (`sky::SHADOW_LAYER`, `sky::BODY_LAYER`), or the player's body casts none from it.
    if l.flags & light_flags::SPOT != 0 {
        let a = (l.fov.clamp(1.0, 179.0) / 2.0).to_radians();
        // Spot lights point down their reference's -z [hypothesis].
        let mut e = commands.spawn((
            SpotLight { color, intensity, range: radius, outer_angle: a, inner_angle: a * 0.8, shadow_maps_enabled: false, ..default() },
            t.looking_to(-Vec3::Z, Vec3::Y),
            RefVisual(placed.form_id),
            ChildOf(cell_e),
        ));
        if let Some(d) = driven {
            e.insert(d);
        }
        if let Some(m) = moving {
            e.insert(m);
        }
        if let Some(m) = model_driven {
            e.insert(m);
        }
    } else {
        let mut e = commands.spawn((PointLight { color, intensity, range: radius, shadow_maps_enabled: false, ..default() }, t, RefVisual(placed.form_id), ChildOf(cell_e)));
        if let Some(d) = driven {
            e.insert(d);
        }
        if let Some(m) = moving {
            e.insert(m);
        }
        if let Some(m) = model_driven {
            e.insert(m);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flicker_and_pulse_stay_within_the_light() {
        assert_eq!(LightMotion::of(light_flags::FLICKER), Some(LightMotion::Flicker));
        assert_eq!(LightMotion::of(1 << 8), Some(LightMotion::PulseSlow));
        assert_eq!(LightMotion::of(light_flags::SPOT), None);
        for m in [LightMotion::Flicker, LightMotion::FlickerSlow, LightMotion::Pulse, LightMotion::PulseSlow] {
            let v: Vec<f32> = (0..400).map(|k| m.factor(k as f32 * 0.01, 0.3)).collect();
            assert!(v.iter().all(|f| (0.5..=1.0).contains(f)), "{m:?}");
            let (lo, hi) = v.iter().fold((1.0f32, 0.0f32), |(a, b), f| (a.min(*f), b.max(*f)));
            assert!(hi - lo > 0.1, "{m:?} moves");
        }
        // Two lights out of step.
        assert_ne!(LightMotion::Pulse.factor(1.0, 0.1), LightMotion::Pulse.factor(1.0, 0.6));
    }

    #[test]
    fn display_sum_patch_goes_in_order() {
        // The anchors as bevy_pbr 0.20.1 has them, in its order, with stand-in text between.
        let source = "head\n@if(!PREPASS_FRAGMENT)\nfn apply_pbr_lighting(\n    in: pbr_types::PbrInput,\n) -> vec4<f32> {\n    lighting_input.diffuse_color = diffuse_color;\n        direct_light += light_contrib * shadow;\n        direct_light += light_contrib * shadow;\n        direct_light += light_contrib * shadow;\n        indirect_light += ambient::ambient_light(in.world_position, in.N, in.V, NdotV, diffuse_color, F0, perceptual_roughness, diffuse_occlusion);\n    // Total light\n    tail\n";
        let out = display_patched(source).expect("all anchors found");
        let at = |s: &str| out.find(s).unwrap_or_else(|| panic!("{s} missing"));
        assert!(at("nvf_point_display(light_id") < at("nvf_spot_display(light_id"));
        assert!(at("nvf_spot_display(light_id") < at("nvf_directional_display(i,"));
        assert!(at("nvf_directional_display(i,") < at("let nvf_sum"));
        assert!(at("let nvf_sum") < at("// Total light"));
        assert_eq!(out.matches("fn apply_pbr_lighting(").count(), 1);
        assert!(out.starts_with("head\n") && out.ends_with("tail\n"));
        // A missing anchor (Bevy changed) leaves the source alone.
        assert!(display_patched(&source.replace("    // Total light\n", "")).is_none());
        // Windows line ends (a checkout with core.autocrlf, issue #946) still patch.
        assert!(display_patched(&unix_lines(&source.replace('\n', "\r\n"))).is_some());
    }

    #[test]
    fn display_values_round_trip() {
        for b in [0u8, 1, 10, 50, 128, 200, 255] {
            assert!((linear_to_display(fnv_world::sky::srgb_to_linear(b)) - b as f32 / 255.0).abs() < 1e-4, "{b}");
        }
    }

    #[test]
    fn display_units() {
        // Lumens → candela (÷4π) → Lambert (÷π) → exposure (÷unit) gives back the brightness.
        let unit = 1000.0;
        let b = lumens(2.2, unit) / (4.0 * PI) / PI / unit;
        assert!((b - 2.2).abs() < 1e-5);
    }

    // A placed light's source and the reference's enabled state (#1619), on a synthetic
    // plugin decoded through the repository schemas, as `crates/fnv-sim/tests/synthetic.rs`.
    mod follows_its_reference {
        use super::*;
        use crate::emittance::EmittanceState;
        use crate::outfits::tests::{asset_fixture, TestFiles};
        use crate::world::{spawn_dormant, StreamStats, WorldData};
        use bevy::ecs::system::RunSystemOnce;
        use bevy::math::Affine3A;
        use fnv_esm::raw::{write_subrecords, Subrecord};
        use fnv_esm::{LoadOrder, Plugin, Schema};
        use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
        use fnv_world::placed::{placement, ref_flags};
        use nvf_assets::model::ModelCtx;
        use nvf_play::sim::SimState;
        use nvf_play::RefChanged;

        const POINT: u32 = 0x100;
        const SPOT: u32 = 0x101;
        const CELL: u32 = 0x200;
        const POINT_REF: u32 = 0x301;
        const SPOT_REF: u32 = 0x302;
        const PARENT_REF: u32 = 0x303;
        const CHILD_REF: u32 = 0x304; // XESP -> PARENT_REF
        const OPPOSITE_REF: u32 = 0x305; // XESP -> PARENT_REF, opposite
        const DORMANT_REF: u32 = 0x306; // Initially Disabled

        fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
            Subrecord { sig: *sig, data, extended: false }
        }

        fn cat(parts: &[&[u8]]) -> Vec<u8> {
            parts.concat()
        }

        fn record(sig: &[u8; 4], flags: u32, fid: u32, subs: &[Subrecord]) -> Vec<u8> {
            let payload = write_subrecords(subs);
            cat(&[sig, &(payload.len() as u32).to_le_bytes(), &flags.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload])
        }

        fn group(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
            cat(&[b"GRUP", &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body])
        }

        /// LIGH DATA: time, radius, colour, flags, falloff exponent, fov, value, weight.
        fn ligh(fid: u32, flags: u32) -> Vec<u8> {
            let data = cat(&[&0i32.to_le_bytes(), &300u32.to_le_bytes(), &[255, 200, 100, 0], &flags.to_le_bytes(), &1f32.to_le_bytes(), &90f32.to_le_bytes(), &0u32.to_le_bytes(), &0f32.to_le_bytes()]);
            record(b"LIGH", 0, fid, &[sub(b"DATA", data), sub(b"FNAM", 1f32.to_le_bytes().to_vec())])
        }

        fn refr(fid: u32, flags: u32, base: u32, parent: Option<(u32, bool)>) -> Vec<u8> {
            let data: Vec<u8> = [0f32; 6].iter().flat_map(|f| f.to_le_bytes()).collect();
            let mut subs = vec![sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)];
            subs.extend(parent.map(|(p, opposite)| sub(b"XESP", cat(&[&p.to_le_bytes(), &[opposite as u8, 0, 0, 0]]))));
            record(b"REFR", flags, fid, &subs)
        }

        fn plugin_bytes() -> Vec<u8> {
            let tes4 = record(b"TES4", 1, 0, &[sub(b"HEDR", cat(&[&1.34f32.to_le_bytes(), &0i32.to_le_bytes(), &0x800u32.to_le_bytes()]))]);
            let lights = group(*b"LIGH", 0, &cat(&[&ligh(POINT, 0), &ligh(SPOT, light_flags::SPOT)]));
            let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", b"TestCell\0".to_vec())]);
            let refs = cat(&[
                &refr(POINT_REF, 0, POINT, None),
                &refr(SPOT_REF, 0, SPOT, None),
                &refr(PARENT_REF, 0, POINT, None),
                &refr(CHILD_REF, 0, POINT, Some((PARENT_REF, false))),
                &refr(OPPOSITE_REF, 0, POINT, Some((PARENT_REF, true))),
                &refr(DORMANT_REF, ref_flags::INITIALLY_DISABLED, POINT, None),
            ]);
            let label = CELL.to_le_bytes();
            let children = group(label, 6, &group(label, 9, &refs));
            let cells = group(*b"CELL", 0, &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cat(&[&cell, &children]))));
            cat(&[&tes4, &lights, &cells])
        }

        fn light_app() -> (App, TestFiles) {
            let (mut app, files) = asset_fixture();
            let plugins: &'static [Plugin] = Box::leak(vec![Plugin::parse(&plugin_bytes(), "Test.esm".into()).unwrap()].into_boxed_slice());
            let lo: &'static LoadOrder<'static> = Box::leak(Box::new(LoadOrder::new(plugins).unwrap()));
            let schema: &'static Schema = Box::leak(Box::new(Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap()));
            let forms: &'static FormStore = Box::leak(Box::new(FormStore::build(lo, schema)));
            let refs: &'static RefTable = Box::leak(Box::new(RefTable::build(lo, schema)));
            app.insert_resource(WorldData { lo, schema, ..crate::outfits::tests::empty_world_data() })
                .insert_resource(SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) })
                .init_resource::<EmittanceState>()
                .add_message::<RefChanged>()
                .add_systems(Update, nvf_play::activate::sync_visuals);
            (app, files)
        }

        fn cell(app: &mut App) -> Entity {
            app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited)).id()
        }

        /// The light source of enabled-at-start reference `r`, spawned under `cell` as
        /// render streaming does (`world.rs` `spawn_ref`).
        fn spawn(app: &mut App, cell: Entity, r: u32) -> Entity {
            app.world_mut()
                .run_system_once(move |mut commands: Commands, world: Res<WorldData>, mut models: ModelCtx, mut emittance: ResMut<EmittanceState>| {
                    let p = placement(world.lo, world.schema, r).unwrap();
                    assert!(spawn_light(&mut commands, cell, &world, p.base.unwrap(), None, &mut models, Affine3A::IDENTITY, 1.0, &p, &mut emittance));
                })
                .unwrap();
            *app.world().get::<Children>(cell).unwrap().last().unwrap()
        }

        /// The holder and light source of reference `r`, not enabled at the start of a game
        /// (`world.rs` `spawn_dormant`).
        fn spawn_not_enabled(app: &mut App, cell: Entity, r: u32) -> (Entity, Entity) {
            app.world_mut()
                .run_system_once(move |mut commands: Commands, world: Res<WorldData>, mut models: ModelCtx, mut emittance: ResMut<EmittanceState>| {
                    let p = placement(world.lo, world.schema, r).unwrap();
                    let mut stats = StreamStats::default();
                    spawn_dormant(&mut commands, cell, &world, &p, r, Affine3A::IDENTITY, &mut models, &mut None, 1.0, &mut emittance, &mut stats, None);
                    assert_eq!(stats.lights_hidden, 1);
                })
                .unwrap();
            let holder = *app.world().get::<Children>(cell).unwrap().last().unwrap();
            (holder, app.world().get::<Children>(holder).unwrap()[0])
        }

        /// What Bevy's visibility propagation makes of an entity and its ancestors: the
        /// `InheritedVisibility` that its light clustering and `detection.rs` read. The
        /// propagation itself takes the renderer's plugins; `lights-disable` runs it.
        fn shines(app: &App, e: Entity) -> bool {
            let mut at = e;
            loop {
                match app.world().get::<Visibility>(at) {
                    Some(Visibility::Hidden) => return false,
                    Some(Visibility::Visible) => return true,
                    _ => {}
                }
                match app.world().get::<ChildOf>(at) {
                    Some(parent) => at = parent.parent(),
                    None => return true,
                }
            }
        }

        /// `r` set enabled or not, and the frame's `RefChanged` for each of `told`.
        fn set_enabled(app: &mut App, r: u32, enabled: bool, told: &[u32]) {
            app.world_mut().resource_mut::<SimState>().world.set_enabled(FormId(r), enabled).unwrap();
            for &form_id in told {
                app.world_mut().write_message(RefChanged::new(form_id));
            }
            app.update();
        }

        #[test]
        fn point_and_spot_sources_go_out_and_come_back() {
            let (mut app, _files) = light_app();
            for r in [POINT_REF, SPOT_REF] {
                let cell = cell(&mut app);
                let source = spawn(&mut app, cell, r);
                assert_eq!(app.world().get::<SpotLight>(source).is_some(), r == SPOT_REF);
                app.update();
                assert!(shines(&app, source), "{r:X} is on at the start");
                set_enabled(&mut app, r, false, &[r]);
                assert!(!shines(&app, source), "{r:X} disabled");
                set_enabled(&mut app, r, true, &[r]);
                assert!(shines(&app, source), "{r:X} enabled again");
            }
        }

        #[test]
        fn a_source_follows_its_enable_parent() {
            let (mut app, _files) = light_app();
            let cell = cell(&mut app);
            let child = spawn(&mut app, cell, CHILD_REF);
            let (holder, opposite) = spawn_not_enabled(&mut app, cell, OPPOSITE_REF);
            app.update();
            assert!(shines(&app, child));
            assert!(!shines(&app, opposite) && !shines(&app, holder));
            // A parent's change names its children too (`script_events.rs`).
            let family = [PARENT_REF, CHILD_REF, OPPOSITE_REF];
            set_enabled(&mut app, PARENT_REF, false, &family);
            assert!(!shines(&app, child));
            assert!(shines(&app, opposite));
            set_enabled(&mut app, PARENT_REF, true, &family);
            assert!(shines(&app, child));
            assert!(!shines(&app, opposite));
        }

        #[test]
        fn a_source_stays_out_when_its_cell_loads_again() {
            let (mut app, _files) = light_app();
            let first = cell(&mut app);
            let source = spawn(&mut app, first, POINT_REF);
            app.update();
            set_enabled(&mut app, POINT_REF, false, &[POINT_REF]);
            app.world_mut().entity_mut(first).despawn();
            assert!(app.world().get_entity(source).is_err());
            // Render streaming goes by the plugins' flags: the light is spawned again.
            let again = cell(&mut app);
            let source = spawn(&mut app, again, POINT_REF);
            app.update();
            assert!(!shines(&app, source), "no script tells it again");
            set_enabled(&mut app, POINT_REF, true, &[POINT_REF]);
            assert!(shines(&app, source));
        }

        #[test]
        fn a_source_spawned_disabled_still_waits_for_enable() {
            let (mut app, _files) = light_app();
            let cell = cell(&mut app);
            let (holder, source) = spawn_not_enabled(&mut app, cell, DORMANT_REF);
            app.update();
            assert!(!shines(&app, source));
            set_enabled(&mut app, DORMANT_REF, true, &[DORMANT_REF]);
            assert!(shines(&app, holder) && shines(&app, source));
            set_enabled(&mut app, DORMANT_REF, false, &[DORMANT_REF]);
            assert!(!shines(&app, source));
        }
    }
}
