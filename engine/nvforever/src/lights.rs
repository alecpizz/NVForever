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

/// The embedded asset path under which bevy_pbr registers `bevy_pbr::pbr_functions`.
const PBR_FUNCTIONS: &str = "embedded://bevy_pbr/render/pbr_functions.wesl";

mod display_shader;
#[cfg(test)]
pub(crate) use self::display_shader::{
    lighting_patched, spot_display_scale, spot_specular_scale, BEVY_ATTENUATION,
    FIRST_PERSON_FLASHLIGHT_DIFFUSE, FIRST_PERSON_FLASHLIGHT_SPECULAR, FRESNEL_EDIT,
    GAME_ATTENUATION, PHYSICAL_ATTENUATION, SPECULAR_EDITS,
};
pub(crate) use self::display_shader::{display_patched, physical_patched};
pub use self::display_shader::unix_lines;
pub(crate) mod reflection_gate;

/// A light colour of a world view as display values (bytes / 255, or a model's stored
/// colour): handed to Bevy unconverted, for the display-value sum (`display_shader`).
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

/// With HDR output (hdr.rs) the values above 1 stay: the image-space grade keeps them
/// apart from its clipped picture and the HDR pass maps them between paper white and the
/// screen's peak. Everything at or below 1 is the same as with [`GAME_CLIP`].
const GAME_HDR: &str = "} @elif(TONEMAP_METHOD_TONY_MC_MAPFACE) {\n    color = max(color, vec3(0.0));";

/// Set once the clip is in place (`NVF_TONEMAP=tony` keeps Bevy's curve, a test aid);
/// sky.rs then leaves out its inverse of Tony McMapface.
static CLIP_INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the main view shows display values clipped (see [`GAME_CLIP`]).
pub fn display_clip() -> bool {
    CLIP_INSTALLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// The main view's tonemapper curve, which everything that writes display values into the
/// HDR picture (the sky, the volumetric clouds) maps them for (nvf_assets tone_input.wesl
/// `to_tonemap_input`). One place to ask, so a choice of curves (#4690) changes it here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToneCurve {
    /// Display values clipped ([`GAME_CLIP`], [`GAME_HDR`]): the value is the input.
    Clipped,
    /// Bevy's Tony McMapface: Physical lighting ([`physical_tony`]) and `NVF_TONEMAP=tony`.
    TonyMcMapface,
}

impl ToneCurve {
    /// The code the shaders take (tone_input.wesl `TONE_CLIPPED`).
    pub fn code(self) -> f32 {
        match self {
            ToneCurve::Clipped => 1.0,
            ToneCurve::TonyMcMapface => 0.0,
        }
    }
}

/// The main view's curve this frame.
pub fn tone_curve() -> ToneCurve {
    if display_clip() { ToneCurve::Clipped } else { ToneCurve::TonyMcMapface }
}

/// Whether the main world view keeps Bevy's Tony McMapface curve under Physical lighting:
/// its light is linear and runs above 1, which the display-value clip would cut off. The
/// volumetric clouds and the air tonemap with the same curve (graphics-enhanced.md §3.25);
/// a choice of tonemapper for every view is #4690's.
pub fn physical_tony() -> bool {
    nvf_assets::authored::physical_pbr()
}

pub struct LightsPlugin;

impl Plugin for LightsPlugin {
    fn build(&self, app: &mut App) {
        let handle: Handle<Shader> = app.world().resource::<AssetServer>().load(PBR_LIGHTING);
        let functions: Handle<Shader> = app.world().resource::<AssetServer>().load(PBR_FUNCTIONS);
        let tonemapping: Handle<Shader> = app.world().resource::<AssetServer>().load(TONEMAPPING_SHARED);
        app.insert_resource(LightingShader { handle, patched: false })
            .insert_resource(FunctionsShader { handle: functions, patched: false })
            .insert_resource(TonemapShader { handle: tonemapping, patched: false, hdr: false })
            .add_systems(PreUpdate, patch_clip)
            .add_systems(Update, (patch_attenuation, patch_display_sum, sync_script_lights, animate_lights, bind_model_lights, animate_model_lights).chain().after(nvf_play::PlaySet::Scripts));
    }
}

#[derive(Resource)]
struct TonemapShader {
    handle: Handle<Shader>,
    patched: bool,
    /// The output the clip was installed for (HDR or SDR): a live switch swaps it.
    hdr: bool,
}

/// Once Bevy's tonemapping functions have loaded, swap Tony McMapface's curve for the
/// clip; the sky system is told to set its colours again.
fn patch_clip(mut state: ResMut<TonemapShader>, mut shaders: ResMut<Assets<Shader>>, sky: Option<ResMut<crate::sky::SkyState>>) {
    if state.patched {
        // HDR output switched live (hdr.rs): the clip for the other output in its place.
        let hdr = bevy::render::view::window::NVF_HDR_SURFACE_ACTIVE.load(std::sync::atomic::Ordering::Relaxed);
        if hdr == state.hdr || !CLIP_INSTALLED.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let Some(shader) = shaders.get(&state.handle) else { return };
        let bevy::shader::Source::Wesl(source) = &shader.source else { return };
        let (from, to) = if hdr { (GAME_CLIP, GAME_HDR) } else { (GAME_HDR, GAME_CLIP) };
        let source = unix_lines(source);
        if source.contains(from) {
            let patched = crate::fog::with_source(shader, source.replace(from, to));
            let _ = shaders.insert(&state.handle, patched);
            info!("lights: the tonemapping clip switched to {}", if hdr { "HDR" } else { "SDR" });
        }
        state.hdr = hdr;
        return;
    }
    let Some(shader) = shaders.get(&state.handle) else { return };
    // With HDR output asked for, wait for the window's surface: only an fp16 surface shows
    // light above white; on an SDR surface the clip stays, so the picture is the SDR one.
    if crate::hdr::on() && !bevy::render::view::window::NVF_HDR_SURFACE_DECIDED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    state.patched = true;
    if std::env::var("NVF_TONEMAP").is_ok_and(|v| v.eq_ignore_ascii_case("tony")) {
        info!("lights: NVF_TONEMAP=tony, the main view keeps Tony McMapface");
        return;
    }
    if physical_tony() {
        info!("lights: physical PBR, the main view keeps Tony McMapface");
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
    let hdr = bevy::render::view::window::NVF_HDR_SURFACE_ACTIVE.load(std::sync::atomic::Ordering::Relaxed);
    state.hdr = hdr;
    let patched = crate::fog::with_source(shader, source.replace(BEVY_TONY, if hdr { GAME_HDR } else { GAME_CLIP }));
    let _ = shaders.insert(&state.handle, patched);
    CLIP_INSTALLED.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(mut sky) = sky {
        sky.refresh();
    }
    info!("lights: display values shown {} (in Tony McMapface's place)", if hdr { "unclipped for HDR output" } else { "clipped" });
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
    let physical = nvf_assets::authored::physical_pbr();
    // The reflection gate goes in with the display-value sum, and under Physical lighting when
    // this run has authored reflections: then the reflection cube keeps the view's environment
    // map, authored materials alone read it, and other surfaces take the light loop's flat
    // ambient. Otherwise the sky's image-based light reaches every surface (physical_ibl.rs;
    // graphics-enhanced.md §3.25).
    let gating = !physical || nvf_assets::reflection_cube::enabled();
    let bevy::shader::Source::Wesl(source) = &shader.source else {
        error!("{PBR_FUNCTIONS}: not WESL; surfaces keep Bevy's linear lighting sum");
        crate::ssao_lighting::set_hook_state(crate::ssao_lighting::HookState::Failed);
        crate::rtgi_volume::set_hook_state(crate::rtgi_volume::HookState::Failed);
        if gating {
            reflection_gate_failed();
        }
        return;
    };
    let source = unix_lines(source);
    let Some(patched) = (if physical { physical_patched(&source) } else { display_patched(&source) }) else {
        error!("{PBR_FUNCTIONS}: apply_pbr_lighting not as expected (Bevy changed?); surfaces keep Bevy's linear lighting sum");
        crate::ssao_lighting::set_hook_state(crate::ssao_lighting::HookState::Failed);
        crate::rtgi_volume::set_hook_state(crate::rtgi_volume::HookState::Failed);
        if gating {
            reflection_gate_failed();
        }
        return;
    };
    // The sun visibility's hook goes into the same function, after these edits; it acts
    // only under its shader def (sun_visibility.rs).
    let patched = match crate::sun_visibility::hook_patched(&patched) {
        Some(hooked) => hooked,
        None => {
            error!("{PBR_FUNCTIONS}: the sun visibility hook's anchors not found (Bevy changed?); clouds and terrain do not shade the sun");
            patched
        }
    };
    // The ambient occlusion's hook goes in after the sun's; it too acts only under its
    // shader def (ssao_lighting.rs).
    // Under physical PBR the light loop is another text: the occlusion's hook for it comes
    // with #3828's split part 2 (#4287); until then the occlusion stays on the picture.
    let (patched, ao_hooked) = match (!physical).then(|| crate::ssao_lighting::hook_patched(&patched)).flatten() {
        Some(hooked) => (hooked, true),
        None if physical => {
            info!("ambient occlusion: on the picture under physical PBR (its lighting hook is still to come)");
            crate::ssao_lighting::set_hook_state(crate::ssao_lighting::HookState::Failed);
            (patched, false)
        }
        None => {
            error!("{PBR_FUNCTIONS}: the ambient occlusion hook's anchors not found (Bevy changed?); the occlusion stays on the picture");
            crate::ssao_lighting::set_hook_state(crate::ssao_lighting::HookState::Failed);
            (patched, false)
        }
    };
    // The bounce light's hook goes in after the ambient occlusion's, whose names it uses; it
    // too acts only under its shader def (rtgi_volume.rs).
    // Under physical PBR it waits with the occlusion's hook (#4287), whose names it needs.
    let (patched, rtgi_hooked) = match (!physical).then(|| crate::rtgi_volume::hook_patched(&patched)).flatten() {
        Some(hooked) => (hooked, true),
        None if physical => {
            info!("RTGI volume: not under physical PBR yet (it needs the occlusion's lighting hook)");
            crate::rtgi_volume::set_hook_state(crate::rtgi_volume::HookState::Failed);
            (patched, false)
        }
        None => {
            error!("{PBR_FUNCTIONS}: the RTGI volume hook's anchors not found (Bevy or an earlier patch changed?); no view gets a bounce light volume");
            crate::rtgi_volume::set_hook_state(crate::rtgi_volume::HookState::Failed);
            (patched, false)
        }
    };
    // The reflection gate last (lights/reflection_gate.rs): the view's environment map light
    // reaches only fragments that opt in. Made whether or not reflections are on in this run;
    // without the opt-in def its text compiles to what Bevy's did without a cube. Under
    // physical PBR (the Physical lighting row), PhysicalIblPlugin provides full-scene IBL directly.
    let (patched, gated) = if !gating {
        (patched, false)
    } else {
        match reflection_gate::gate_patched(&patched) {
            Some(gated) => (gated, true),
            None => (patched, false),
        }
    };
    let patched = crate::fog::with_source(shader, patched);
    // Nothing is published as installed unless the patched shader is in place: a gate
    // reported installed over Bevy's ungated text would hand the cube to every lit shader.
    if let Err(e) = shaders.insert(&state.handle, patched) {
        error!("{PBR_FUNCTIONS}: the patched shader could not be stored ({e:?}); surfaces keep Bevy's lighting sum");
        crate::ssao_lighting::set_hook_state(crate::ssao_lighting::HookState::Failed);
        crate::rtgi_volume::set_hook_state(crate::rtgi_volume::HookState::Failed);
        if gating {
            reflection_gate_failed();
        }
        return;
    }
    if physical {
        info!("lights: physical PBR linear lighting installed");
    } else {
        info!("lights: display-value lighting sum installed");
    }
    if ao_hooked {
        crate::ssao_lighting::set_hook_state(crate::ssao_lighting::HookState::Installed);
        info!("lights: ambient occlusion hook installed");
    }
    if rtgi_hooked {
        crate::rtgi_volume::set_hook_state(crate::rtgi_volume::HookState::Installed);
        info!("lights: RTGI volume hook installed");
    }
    if gating {
        if gated {
            nvf_assets::reflection_cube::set_gate_state(nvf_assets::reflection_cube::GateState::Installed);
            info!("lights: reflection gate installed (the environment map light reaches opted-in fragments only)");
        } else {
            reflection_gate_failed();
        }
    }
}

/// The reflection gate could not be made: no reflection cube is ever attached in this run
/// (nvf_assets reflection_cube.rs), since an ungated `pbr_functions` would give it to every
/// lit shader.
fn reflection_gate_failed() {
    nvf_assets::reflection_cube::set_gate_state(nvf_assets::reflection_cube::GateState::Failed);
    error!("{PBR_FUNCTIONS}: the reflection gate's anchors not found (Bevy changed?); reflections on authored materials stay off (no cube is attached)");
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
pub type NotLightSource = (Without<PointLight>, Without<SpotLight>, Without<SessionLight>);

/// On a placed light that flickers or pulses: its motion, full intensity and phase.
#[derive(Component, Debug, Clone, Copy)]
pub struct MovingLight {
    pub motion: LightMotion,
    pub full: f32,
    pub seed: f32,
}

/// One cell-owned light instance, including a currently non-emitting one. The
/// renderer derives values from canonical session edits only when their revision
/// changes (docs/spec-notes/light-session-consumers.md).
#[derive(Component)]
pub struct SessionLight {
    base: u32,
    original: fnv_world::lighting::LightData,
    radius_offset: Option<f32>,
    scale: f32,
    model_color: Option<[f32; 3]>,
    model_driven: bool,
    seed: f32,
    seen: Option<u64>,
}

#[cfg(test)]
impl SessionLight {
    pub(crate) fn test_stub() -> Self {
        Self {
            base: 0,
            original: fnv_world::lighting::LightData {
                radius: 128.0,
                color: [255, 255, 255],
                flags: 0,
                falloff_exponent: 1.0,
                fov: 90.0,
                fade: 1.0,
            },
            radius_offset: None,
            scale: 1.0,
            model_color: None,
            model_driven: false,
            seed: 0.0,
            seen: None,
        }
    }
}

struct LightValues {
    shown: [f32; 3],
    intensity: f32,
    radius: f32,
    moving: Option<MovingLight>,
    spot: Option<f32>,
    emitting: bool,
}

fn light_values(l: fnv_world::lighting::LightData, instance: &SessionLight) -> LightValues {
    let radius = placed_light_radius(l.radius, instance.radius_offset);
    let shown = if l.color == [0; 3] {
        instance.model_color.unwrap_or([0.0; 3]).map(|v| v.clamp(0.0, 1.0))
    } else {
        l.color.map(|b| b as f32 / 255.0)
    };
    let intensity = lumens(l.fade.max(0.0) * instance.scale, crate::sky::light_unit());
    let motion = LightMotion::of(l.flags);
    LightValues {
        shown,
        intensity,
        radius: if matches!(motion, Some(LightMotion::Flicker | LightMotion::FlickerSlow)) { radius * FLAME_REACH } else { radius },
        moving: motion.map(|motion| MovingLight { motion, full: intensity, seed: instance.seed }),
        spot: (l.flags & light_flags::SPOT != 0).then(|| (l.fov.clamp(1.0, 179.0) / 2.0).to_radians()),
        emitting: l.flags & (light_flags::NEGATIVE | light_flags::OFF_BY_DEFAULT) == 0 && radius > 0.0
            && (l.color != [0; 3] || instance.model_color.is_some() || instance.model_driven),
    }
}

pub(crate) type SessionLights<'w, 's> = Query<'w, 's, (Entity, &'static mut SessionLight, Option<&'static mut PointLight>, Option<&'static mut SpotLight>, Option<&'static mut crate::emittance::LightEmittance>)>;

pub(crate) fn sync_script_lights(mut commands: Commands, script: Option<Res<nvf_play::script::ScriptState>>, mut lights: SessionLights) {
    let Some(game) = script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game) else { return };
    sync_lights_from_game(game, &mut commands, &mut lights);
}

fn sync_lights_from_game(game: &fnv_game::Game<'_>, commands: &mut Commands, lights: &mut SessionLights) {
    let revision = game.gaps.jip4.light_revision;
    for (entity, mut instance, point, spot, emittance) in lights.iter_mut() {
        if instance.seen == Some(revision) { continue; }
        instance.seen = Some(revision);
        let l = game.session_light_data(fnv_sim::FormId(instance.base), instance.original);
        let v = light_values(l, &instance);
        if let Some(mut e) = emittance {
            e.color = LinearRgba::rgb(v.shown[0], v.shown[1], v.shown[2]);
        }
        if !v.emitting {
            commands.entity(entity).remove::<(PointLight, SpotLight, MovingLight)>();
            continue;
        }
        let color = display_color(v.shown);
        if let Some(angle) = v.spot {
            if let Some(mut s) = spot {
                s.color = color;
                s.intensity = v.intensity;
                s.range = v.radius;
                s.outer_angle = angle;
                s.inner_angle = angle * 0.8;
            } else {
                commands.entity(entity).insert(SpotLight { color, intensity: v.intensity, range: v.radius, outer_angle: angle, inner_angle: angle * 0.8, shadow_maps_enabled: false, ..default() });
            }
            commands.entity(entity).remove::<PointLight>();
        } else {
            if let Some(mut p) = point {
                p.color = color;
                p.intensity = v.intensity;
                p.range = v.radius;
            } else {
                commands.entity(entity).insert(PointLight { color, intensity: v.intensity, range: v.radius, shadow_maps_enabled: false, ..default() });
            }
            commands.entity(entity).remove::<SpotLight>();
        }
        if let Some(moving) = v.moving {
            commands.entity(entity).insert(moving);
        } else {
            commands.entity(entity).remove::<MovingLight>();
        }
    }
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
fn bind_model_lights(
    mut commands: Commands,
    rigs: Query<(Entity, &LightRig)>,
    new_rigs: Query<(Entity, &LightRig), Added<LightRig>>,
    new_lights: Query<Entity, (Added<PointLight>, With<ModelDrivenLight>)>,
    mut lights: Query<(Entity, &mut ModelDrivenLight), With<PointLight>>,
) {
    if new_rigs.is_empty() && new_lights.is_empty() {
        return;
    }
    for (root, rig) in &new_rigs {
        let Some((e, mut l)) = lights.iter_mut().find(|(_, l)| l.reference == rig.reference && l.root.is_none()) else { continue };
        bind_model_light(&mut commands, e, &mut l, root, rig);
    }
    // A dormant modeled light can resume after its rig was loaded. Unrelated
    // muzzle/fire lights must not cause a scan of all rigs and modeled lights.
    for e in &new_lights {
        let Ok((_, mut l)) = lights.get_mut(e) else { continue };
        if l.root.is_some() { continue; }
        let Some((root, rig)) = rigs.iter().find(|(_, rig)| rig.reference == l.reference) else { continue };
        bind_model_light(&mut commands, e, &mut l, root, rig);
    }
}

fn bind_model_light(commands: &mut Commands, entity: Entity, light: &mut ModelDrivenLight, root: Entity, rig: &LightRig) {
    light.root = Some(root);
    commands.entity(entity).insert((ChildOf(rig.anchor), light.light.local));
    debug!("model lights: 0x{:08X} light {} hangs from {}", rig.reference, light.light.name, light.light.parent.as_deref().unwrap_or("the root"));
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
    let physical = nvf_assets::authored::physical_pbr();
    let Some(patched) = display_shader::lighting_patched(&source, physical) else {
        error!("{PBR_LIGHTING}: expected anchors not found (Bevy changed?); lights keep Bevy's defaults");
        return;
    };
    let patched = crate::fog::with_source(shader, patched);
    let _ = shaders.insert(&state.handle, patched);
    if physical {
        info!("lights: physical PBR installed (EON diffuse, Cook-Torrance GGX, 0.045 roughness floor, inverse-square attenuation)");
    } else {
        info!("lights: game attenuation and display-value Fresnel installed");
    }
}

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
/// (docs/spec-notes/model-lights.md §2). A valid light registers a cell-owned instance
/// even when it currently emits nothing; the result keeps that separate from the
/// existing emitting-light statistics.
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
) -> Option<SpawnedLight> {
    let l = light_data(world.lo, world.schema, ligh)?;
    let from_model = model.and_then(|m| models.model_light_colour(&format!(r"meshes\{m}")));
    let prepared = model.and_then(|m| models.model(&format!(r"meshes\{m}")));
    let model_driven = prepared.as_ref().and_then(|pm| pm.light.clone());
    // The light sits on the model's `AttachLight` node when it has one (a fire barrel's
    // flames, 102 units over its foot), else at the reference [hypothesis,
    // model-lights.md §4].
    let at: Vec3 = prepared.as_ref().and_then(|pm| nvf_assets::model::attach_light(&pm.nodes)).map_or(rt.translation, |a| (rt * a).translation).into();
    let instance = SessionLight {
        base: ligh, original: l, radius_offset: ref_xrds(world.lo, world.schema, placed.form_id), scale,
        model_color: from_model, model_driven: model_driven.is_some(),
        seed: (placed.form_id.wrapping_mul(2_654_435_761) >> 8) as f32 / 16_777_216.0,
        seen: None,
    };
    let values = light_values(l, &instance);
    let shown = values.shown;
    let model_driven = model_driven.map(|light| ModelDrivenLight { reference: placed.form_id, light, root: None });
    let color = display_color(shown);
    let driven = placed.emittance.and_then(|x| emittance.light(world, x, LinearRgba::rgb(shown[0], shown[1], shown[2])));
    let intensity = values.intensity;
    let radius = values.radius;
    let t = Transform::from_translation(at);
    // Lights out of step with each other: a phase from the reference.
    let moving = values.moving;
    if !values.emitting {
        let mut e = commands.spawn((instance, t, Visibility::Inherited, RefVisual(placed.form_id), ChildOf(cell_e)));
        if let Some(d) = driven { e.insert(d); }
        if let Some(m) = model_driven { e.insert(m); }
        return Some(SpawnedLight { emitting: false });
    }
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
            instance,
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
        let mut e = commands.spawn((PointLight { color, intensity, range: radius, shadow_maps_enabled: false, ..default() }, t, RefVisual(placed.form_id), ChildOf(cell_e), instance));
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
    Some(SpawnedLight { emitting: true })
}

pub struct SpawnedLight {
    pub emitting: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_point_light_does_not_rebind_models_and_cold_modeled_light_still_binds() {
        let mut app = App::new();
        app.add_systems(Update, bind_model_lights);
        let anchor = app.world_mut().spawn(Transform::IDENTITY).id();
        let root = app.world_mut().spawn(LightRig { reference: 42, anchor }).id();
        let model = || ModelDrivenLight {
            reference: 42, root: None,
            light: nvf_assets::model::ModelLight {
                name: "AuthoredLight".into(), parent: None,
                local: Transform::from_xyz(1.0, 2.0, 3.0), diffuse: [1.0; 3], dimmer: 1.0,
            },
        };
        let established = app.world_mut().spawn((PointLight::default(), model())).id();
        app.update();
        assert_eq!(app.world().get::<ModelDrivenLight>(established).unwrap().root, Some(root));
        let parent = app.world().get::<ChildOf>(established).unwrap().parent();
        // Leave a deferred unbound model in an existing archetype. A foreign point
        // light is not a model/rig publication event and must not trigger its bind.
        app.world_mut().get_mut::<ModelDrivenLight>(established).unwrap().root = None;
        let foreign = app.world_mut().spawn(PointLight::default()).id();
        app.update();
        assert_eq!(app.world().get::<ModelDrivenLight>(established).unwrap().root, None);
        assert_eq!(app.world().get::<ChildOf>(established).unwrap().parent(), parent);
        assert!(app.world().get::<ModelDrivenLight>(foreign).is_none());
        // The actual off->on publication is eligible even with an older rig.
        app.world_mut().entity_mut(established).remove::<PointLight>();
        app.update();
        app.world_mut().entity_mut(established).insert(PointLight::default());
        app.update();
        assert_eq!(app.world().get::<ModelDrivenLight>(established).unwrap().root, Some(root));
        assert_eq!(app.world().get::<ChildOf>(established).unwrap().parent(), anchor);
        let later = app.world_mut().spawn((PointLight::default(), model())).id();
        app.update();
        assert_eq!(app.world().get::<ModelDrivenLight>(later).unwrap().root, Some(root));
        assert_eq!(app.world().get::<ChildOf>(later).unwrap().parent(), anchor);
    }

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
    fn the_fresnel_line_is_in_bevys_shader_once() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../vendor/bevy_pbr/src/render/pbr_lighting.wesl");
        let source = unix_lines(&std::fs::read_to_string(path).expect("vendored pbr_lighting.wesl"));
        assert_eq!(source.matches(FRESNEL_EDIT.0).count(), 1, "the edit's anchor");
        let mut patched = source.replace(BEVY_ATTENUATION, GAME_ATTENUATION);
        for (from, to) in SPECULAR_EDITS {
            patched = patched.replace(from, to);
        }
        assert_eq!(patched.matches(FRESNEL_EDIT.0).count(), 1, "the anchor survives the other edits");
        let patched = patched.replace(FRESNEL_EDIT.0, FRESNEL_EDIT.1);
        assert!(patched.contains("let F = select(fresnel(F0, LdotH), F0,"));
        // The anisotropic and transmission callers keep Bevy's Fresnel.
        assert!(patched.contains("let Fa = fresnel(F0, LdotH);"));
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
        // The product is formed in display values and made linear by the sRGB curve, the
        // frame's own (not a 2.2 power, which shows dark products at half and less).
        assert!(out.contains("direct_light += nvf_display_to_linear(min(nvf_linear_to_display(diffuse_color) * nvf_sum, vec3(1.0))) / view_bindings::view.exposure;"));
        assert!(at("fn nvf_linear_to_display(") < at("fn nvf_display_to_linear(") && at("fn nvf_display_to_linear(") < at("fn apply_pbr_lighting("));
        assert!(!out.contains("pow(nvf_sum"));
        assert_eq!(out.matches("fn apply_pbr_lighting(").count(), 1);
        assert!(out.starts_with("head\n") && out.ends_with("tail\n"));
        // A missing anchor (Bevy changed) leaves the source alone.
        assert!(display_patched(&source.replace("    // Total light\n", "")).is_none());
        // Windows line ends (a checkout with core.autocrlf, issue #946) still patch.
        assert!(display_patched(&unix_lines(&source.replace('\n', "\r\n"))).is_some());
    }

    #[test]
    fn physical_pbr_patches_vendored_shaders_and_preserves_sun_hooks() {
        let lighting_src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../vendor/bevy_pbr/src/render/pbr_lighting.wesl")).expect("vendored pbr_lighting.wesl");
        let physical_lighting = lighting_patched(&lighting_src, true).expect("physical lighting_patched succeeds");
        assert!(physical_lighting.contains("fn Fd_OrenNayar("));
        assert!(physical_lighting.contains("fn nvf_dielectric_E("));
        assert!(physical_lighting.contains(PHYSICAL_ATTENUATION), "physical inverse-square attenuation installed");
        assert!(physical_lighting.contains("clamp(perceptualRoughness, 0.045, 1.0)"), "desktop 0.045 roughness floor installed");
        assert_eq!(physical_lighting.matches("Fd_OrenNayar(input, &derived_input, diffuse_color)").count(), 2, "point_light and directional_light call EON Fd_OrenNayar");
        assert!(!physical_lighting.contains("Fd_Burley(input, &derived_input)"), "no Fd_Burley calls remain");
        assert!(physical_lighting.contains("    let F = fresnel(F0, LdotH);"), "Cook-Torrance keeps full Schlick Fresnel");
        assert!(!physical_lighting.contains("select(fresnel(F0, LdotH), F0"), "display Fresnel clamp is absent");
        assert!(physical_lighting.contains("fn nvf_light_colour("), "display-space light uniforms remain linearized");
        // One sRGB decode (the owner's review of #4199): main's nvf_display_to_linear, once, and
        // the HDR decode written with it; the physical pbr_functions carries no copy of its own.
        assert_eq!(physical_lighting.matches("fn nvf_display_to_linear(").count(), 1);
        assert!(physical_lighting.contains("return nvf_display_to_linear(x / m) * m;"));
        assert!(!physical_lighting.contains("@if(!PREPASS_FRAGMENT)\nfn nvf_display_to_linear("), "pbr_lighting is read by prepass shaders too");
        assert!(!physical_lighting.contains("NVF_DISPLAY_TO_LINEAR"), "the mark is replaced");

        let display_lighting = lighting_patched(&lighting_src, false).expect("display lighting_patched succeeds");
        assert!(!display_lighting.contains("fn Fd_OrenNayar("));
        assert!(display_lighting.contains(GAME_ATTENUATION));
        assert!(display_lighting.contains("clamp(perceptualRoughness, 0.089, 1.0)"));
        assert!(display_lighting.contains("let F = select(fresnel(F0, LdotH), F0,"));
        assert_eq!(display_lighting.matches("Fd_Burley(input, &derived_input)").count(), 2);

        let functions_src = unix_lines(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../vendor/bevy_pbr/src/render/pbr_functions.wesl")).expect("vendored pbr_functions.wesl"));
        let physical_functions = physical_patched(&functions_src).expect("physical_patched succeeds");
        assert!(physical_functions.contains("fn nvf_apply_pbr_lighting("));
        assert!(physical_functions.contains("fn nvf_specular_ao("));
        assert!(physical_functions.contains("// nvf_physical_ibl_diff_ao"));
        assert!(physical_functions.contains("let nvf_ambient_lin = "));
        assert!(!physical_functions.contains("nvf_point_display"));
        assert!(!physical_functions.contains("fn nvf_srgb_to_linear_hdr(") && !physical_functions.contains("fn nvf_display_to_linear("), "no copy of the decode");
        assert!(physical_functions.contains("lighting::nvf_srgb_to_linear_hdr("));
        let sun_patched = crate::sun_visibility::hook_patched(&physical_functions).expect("sun_visibility hook applies after physical_patched");
        // Authored reflections under physical lighting: the reflection gate goes in after them.
        assert!(reflection_gate::gate_patched(&sun_patched).is_some(), "the reflection gate applies after physical_patched");
    }

    /// CPU white-furnace check for EON (`Fd_OrenNayar` without dielectric coupling):
    /// integrating $\rho \, f_{\text{EON}}(\omega_i, \omega_o) \cos\theta_i$ with $\rho = 1$
    /// over the upper hemisphere must equal 1.0 within `0.002` across all roughnesses.
    #[test]
    fn eon_diffuse_passes_white_furnace_conservation() {
        let c1 = 0.5f32 - 2.0 / (3.0 * PI);
        let c2 = 2.0f32 / 3.0 - 28.0 / (15.0 * PI);
        let g = |mu: f32| -> f32 {
            let x = 1.0 - mu.clamp(0.0, 1.0);
            x * (0.057_108_53 + x * (0.491_881_87 + x * (-0.332_181_44 + x * 0.071_442_99)))
        };
        let (n_mu, n_phi) = (120usize, 120usize);
        let (dmu, dphi) = (1.0 / n_mu as f32, PI / n_phi as f32);
        for pr in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let af = 1.0 / (1.0 + c1 * pr);
            let e_avg = af * (1.0 + pr * c2);
            let rho_ms = e_avg / (1.0 - (1.0 - e_avg)).max(1e-6);
            for mu_o in [0.1f32, 0.5, 0.9] {
                let e_o = af * (1.0 + pr * g(mu_o));
                let sin_o = (1.0 - mu_o * mu_o).max(0.0).sqrt();
                let mut total = 0.0f32;
                for i in 0..n_mu {
                    let mu_i = (i as f32 + 0.5) * dmu;
                    let sin_i = (1.0 - mu_i * mu_i).max(0.0).sqrt();
                    let e_i = af * (1.0 + pr * g(mu_i));
                    let f_ms = if pr > 1e-5 { (rho_ms / PI) * (1.0 - e_i) * (1.0 - e_o) / (1.0 - e_avg).max(1e-5) } else { 0.0 };
                    for j in 0..n_phi {
                        let phi = (j as f32 + 0.5) * dphi;
                        let s = sin_i * sin_o * phi.cos();
                        let t = if s > 0.0 { 1.0 / mu_i.max(mu_o) } else { 1.0 };
                        let f_ss = (af * (1.0 + pr * s * t)) / PI;
                        total += (f_ss + f_ms) * mu_i * dmu * dphi * 2.0;
                    }
                }
                assert!((total - 1.0).abs() < 0.002, "EON white furnace failed at pr={pr}, mu_o={mu_o}: got {total}");
            }
        }
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
        const UNCHANGED: u32 = 0x102;
        const OFF: u32 = 0x103;
        const UNCHANGED_REF: u32 = 0x307;
        const OFF_REF: u32 = 0x308;

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
            let lights = group(*b"LIGH", 0, &cat(&[&ligh(POINT, 0), &ligh(SPOT, light_flags::SPOT), &ligh(UNCHANGED, 0), &ligh(OFF, light_flags::OFF_BY_DEFAULT)]));
            let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", b"TestCell\0".to_vec())]);
            let refs = cat(&[
                &refr(POINT_REF, 0, POINT, None),
                &refr(SPOT_REF, 0, SPOT, None),
                &refr(PARENT_REF, 0, POINT, None),
                &refr(CHILD_REF, 0, POINT, Some((PARENT_REF, false))),
                &refr(OPPOSITE_REF, 0, POINT, Some((PARENT_REF, true))),
                &refr(DORMANT_REF, ref_flags::INITIALLY_DISABLED, POINT, None),
                &refr(UNCHANGED_REF, 0, UNCHANGED, None),
                &refr(OFF_REF, 0, OFF, None),
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
            let (source, emitting) = spawn_result(app, cell, r);
            assert!(emitting);
            source
        }

        fn spawn_result(app: &mut App, cell: Entity, r: u32) -> (Entity, bool) {
            let spawned = app.world_mut()
                .run_system_once(move |mut commands: Commands, world: Res<WorldData>, mut models: ModelCtx, mut emittance: ResMut<EmittanceState>| {
                    let p = placement(world.lo, world.schema, r).unwrap();
                    spawn_light(&mut commands, cell, &world, p.base.unwrap(), None, &mut models, Affine3A::IDENTITY, 1.0, &p, &mut emittance)
                })
                .unwrap().expect("valid light registers an owner");
            (*app.world().get::<Children>(cell).unwrap().last().unwrap(), spawned.emitting)
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

        #[derive(Resource)]
        struct LightGame(fnv_game::Game<'static>);

        fn session_bridge(mut commands: Commands, game: Res<LightGame>, mut lights: SessionLights) {
            sync_lights_from_game(&game.0, &mut commands, &mut lights);
        }

        fn session_emittance(game: Res<LightGame>, world: Res<WorldData>, mut state: ResMut<EmittanceState>) {
            state.refresh_session_lights(&world, &game.0);
        }

        fn session_app() -> (App, TestFiles) {
            let (mut app, files) = light_app();
            let world = app.world().resource::<WorldData>();
            let sim = app.world().resource::<SimState>();
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
            let json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let data = Box::leak(Box::new(fnv_game::GameData::build(world.lo, world.schema, fnv_script::FunctionTable::load(&path).unwrap(), &json, sim.refs)));
            let game = fnv_game::Game::new(data, World::new(sim.forms, sim.refs, SimConfig::default()), Default::default());
            app.insert_resource(LightGame(game)).init_resource::<Time>()
                .insert_resource(crate::sky::authored_session_sky())
                .add_systems(Update, (session_bridge, session_emittance, crate::emittance::update_emittance).chain());
            (app, files)
        }

        fn source(app: &mut App, body: &str) {
            let text = format!("Begin Function {{}}\n{body}\nEnd");
            app.world_mut().resource_mut::<LightGame>().0.run_query(&text, Default::default()).unwrap();
        }

        fn instances(app: &mut App) -> usize {
            let mut query = app.world_mut().query_filtered::<Entity, With<SessionLight>>();
            query.iter(app.world()).count()
        }

        #[test]
        fn session_setters_reach_existing_point_spot_and_flag_consumers() {
            let (mut app, _files) = session_app();
            let cell = cell(&mut app);
            let point = spawn(&mut app, cell, POINT_REF);
            let spot = spawn(&mut app, cell, SPOT_REF);
            let unchanged = spawn(&mut app, cell, UNCHANGED_REF);
            app.update();
            let original = app.world().get::<PointLight>(unchanged).unwrap().color;
            source(&mut app, "SetLightTraitNumeric (BuildRef 0 256) 0 450\nSetLightTraitNumeric (BuildRef 0 256) 1 32\nSetLightTraitNumeric (BuildRef 0 256) 6 2\nSetLightTraitNumeric (BuildRef 0 257) 5 60");
            app.update();
            let p = app.world().get::<PointLight>(point).unwrap();
            assert_eq!(p.range, 450.0);
            assert_eq!(p.color, display_color([32.0 / 255.0, 200.0 / 255.0, 100.0 / 255.0]));
            assert_eq!(p.intensity, lumens(2.0, crate::sky::light_unit()));
            let s = app.world().get::<SpotLight>(spot).unwrap();
            assert!((s.outer_angle - 30f32.to_radians()).abs() < 1e-6);
            assert_eq!(app.world().get::<PointLight>(unchanged).unwrap().color, original);
            source(&mut app, "SetLightFlag (BuildRef 0 256) 3 1\nSetLightFlag (BuildRef 0 256) 9 1");
            app.update();
            assert!(app.world().get::<PointLight>(point).is_none());
            assert_eq!(app.world().get::<SpotLight>(point).unwrap().range, 450.0 * FLAME_REACH);
            assert_eq!(app.world().get::<MovingLight>(point).unwrap().full, lumens(2.0, crate::sky::light_unit()));
            source(&mut app, "SetLightFlag (BuildRef 0 256) 9 0\nSetLightFlag (BuildRef 0 256) 3 0");
            app.update();
            assert!(app.world().get::<SpotLight>(point).is_none());
            assert_eq!(app.world().get::<PointLight>(point).unwrap().range, 450.0);
            assert!(app.world().get::<MovingLight>(point).is_none());
            assert_eq!(instances(&mut app), 3, "type/flag updates reuse the same owners");
        }

        #[test]
        fn cold_dormant_repeated_and_cell_lifecycle_keep_one_light_instance() {
            let (mut app, _files) = session_app();
            source(&mut app, "SetLightTraitNumeric (BuildRef 0 256) 0 410\nSetLightFlag (BuildRef 0 259) 5 0");
            let first = cell(&mut app);
            let point = spawn(&mut app, first, POINT_REF);
            let (off, emitting) = spawn_result(&mut app, first, OFF_REF);
            assert!(!emitting, "streaming retains a currently non-emitting owner");
            assert!(app.world().get::<PointLight>(off).is_none());
            app.update();
            assert_eq!(app.world().get::<PointLight>(point).unwrap().range, 410.0);
            assert!(app.world().get::<PointLight>(off).is_some(), "cold edit reaches dormant carrier");
            let revision = app.world().resource::<LightGame>().0.gaps.jip4.light_revision;
            for _ in 0..12 {
                source(&mut app, "SetLightTraitNumeric (BuildRef 0 256) 0 410");
                app.update();
                assert_eq!(instances(&mut app), 2);
            }
            assert_eq!(app.world().resource::<LightGame>().0.gaps.jip4.light_revision, revision, "same values do not invalidate every held frame");
            let held = app.world().resource::<LightGame>().0.time;
            assert_eq!(held, 0.0);
            app.world_mut().resource_mut::<LightGame>().0.tick();
            app.update();
            assert_eq!(instances(&mut app), 2);
            assert!(app.world().resource::<LightGame>().0.time > held);
            source(&mut app, "SetLightFlag (BuildRef 0 256) 5 1");
            app.update();
            assert!(app.world().get::<PointLight>(point).is_none());
            source(&mut app, "SetLightFlag (BuildRef 0 256) 5 0");
            app.update();
            assert!(app.world().get::<PointLight>(point).is_some());
            app.world_mut().entity_mut(first).despawn();
            assert!(app.world().get_entity(point).is_err() && app.world().get_entity(off).is_err());
            assert_eq!(instances(&mut app), 0);
            let next = cell(&mut app);
            let again = spawn(&mut app, next, POINT_REF);
            app.update();
            assert_ne!(again, point);
            assert_eq!(app.world().get::<PointLight>(again).unwrap().range, 410.0);
            assert_eq!(instances(&mut app), 1);
        }

        #[test]
        fn light_backed_emittance_updates_at_fixed_hour_without_compounding() {
            let (mut app, _files) = session_app();
            let cell = cell(&mut app);
            let point = spawn(&mut app, cell, POINT_REF);
            let spot = spawn(&mut app, cell, SPOT_REF);
            let light = app.world_mut().run_system_once(|world: Res<WorldData>, mut state: ResMut<EmittanceState>| {
                state.light(&world, SPOT, LinearRgba::rgb(1.0, 200.0 / 255.0, 100.0 / 255.0)).unwrap()
            }).unwrap();
            app.world_mut().entity_mut(point).insert(light);
            let light = app.world_mut().run_system_once(|world: Res<WorldData>, mut state: ResMut<EmittanceState>| {
                state.light(&world, SPOT, LinearRgba::rgb(1.0, 200.0 / 255.0, 100.0 / 255.0)).unwrap()
            }).unwrap();
            app.world_mut().entity_mut(spot).insert(light);
            let material = app.world_mut().run_system_once(|world: Res<WorldData>, mut state: ResMut<EmittanceState>, mut materials: ResMut<Assets<nvf_assets::NifMaterial>>| {
                let shared = materials.add(nvf_assets::nif_material::nif_material(StandardMaterial { emissive: LinearRgba::WHITE, ..default() }));
                state.material(&world, &mut materials, &shared, SPOT).unwrap()
            }).unwrap();
            app.update();
            let hour = app.world().resource::<crate::sky::SkyState>().hour;
            source(&mut app, "SetLightTraitNumeric (BuildRef 0 257) 1 64\nSetLightTraitNumeric (BuildRef 0 257) 2 128\nSetLightTraitNumeric (BuildRef 0 257) 3 255\nSetLightTraitNumeric (BuildRef 0 257) 6 0.5");
            app.update();
            let expected = display_color([64.0 / 255.0 * 0.5, (200.0 / 255.0) * (128.0 / 255.0 * 0.5), 100.0 / 255.0 * 0.5]);
            assert_eq!(app.world().get::<PointLight>(point).unwrap().color, expected);
            let spot_expected = display_color([(64.0 / 255.0) * (64.0 / 255.0 * 0.5), (128.0 / 255.0) * (128.0 / 255.0 * 0.5), 0.5]);
            assert_eq!(app.world().get::<SpotLight>(spot).unwrap().color, spot_expected);
            let emitted = app.world().resource::<Assets<nvf_assets::NifMaterial>>().get(&material).unwrap().base.emissive;
            assert!((emitted.blue - 0.5).abs() < 1e-6);
            assert!(emitted.red > 0.02 && emitted.red < 0.03 && emitted.green > 0.1 && emitted.green < 0.12);
            for _ in 0..8 {
                source(&mut app, "SetLightTraitNumeric (BuildRef 0 257) 6 0.5");
                app.update();
                assert_eq!(app.world().get::<PointLight>(point).unwrap().color, expected);
                assert_eq!(app.world().get::<SpotLight>(spot).unwrap().color, spot_expected);
                assert_eq!(app.world().resource::<Assets<nvf_assets::NifMaterial>>().get(&material).unwrap().base.emissive, emitted);
            }
            assert_eq!(app.world().resource::<crate::sky::SkyState>().hour, hour);
            assert_eq!(instances(&mut app), 2);
        }
    }

    /// The vendored `pbr_functions` with every edit lights.rs makes before the gate.
    fn chain_before_gate() -> String {
        let path = format!("{}/../vendor/bevy_pbr/src/render/pbr_functions.wesl", env!("CARGO_MANIFEST_DIR"));
        let source = unix_lines(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")));
        let display = display_patched(&source).expect("display patch applies");
        let sun = crate::sun_visibility::hook_patched(&display).expect("sun hook applies");
        let ao = crate::ssao_lighting::hook::hook_patched(&sun).expect("ambient occlusion hook applies");
        crate::rtgi_volume::hook_patched(&ao).expect("RTGI volume hook applies")
    }

    /// The installer itself: the whole chain in its order stored and the bounce light's hook
    /// (#3009) reported installed; with its anchor doubled (which fails no other edit), or
    /// without the display sum, reported failed and not in the text. Run in a process of its
    /// own, as it sets the process-wide states of three hooks.
    #[test]
    fn the_installer_reports_the_rtgi_hook() {
        use crate::rtgi_volume::{hook::IMPORT_LINE, hook_state, HookState};
        const CHILD: &str = "NVF_TEST_RTGI_INSTALLER_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["lights::tests::the_installer_reports_the_rtgi_hook", "--exact", "--nocapture", "--test-threads=1"])
                .env(CHILD, "1")
                .output()
                .expect("start the test alone");
            let text = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success() && text.contains("1 passed"), "{text}\n{}", String::from_utf8_lossy(&output.stderr));
            return;
        }
        let good = std::fs::read_to_string(format!("{}/../vendor/bevy_pbr/src/render/pbr_functions.wesl", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let doubled = unix_lines(&good).replacen(IMPORT_LINE, &IMPORT_LINE.repeat(2), 1);
        assert_eq!(hook_state(), HookState::Pending);
        for (source, installed) in [(&good, true), (&doubled, false), (&good, true), (&String::new(), false), (&good, true)] {
            let mut world = World::new();
            let mut shaders = Assets::<Shader>::default();
            let handle = shaders.add(Shader::from_wesl(source.clone(), PBR_FUNCTIONS));
            world.insert_resource(shaders);
            world.insert_resource(FunctionsShader { handle: handle.clone(), patched: false });
            world.run_system_cached(patch_display_sum).unwrap();
            let bevy::shader::Source::Wesl(text) = &world.resource::<Assets<Shader>>().get(&handle).unwrap().source else { panic!("WESL") };
            if installed {
                assert_eq!(*text, reflection_gate::gate_patched(&chain_before_gate()).unwrap());
            } else {
                assert!(!text.contains("NVF_RTGI_VOLUME"));
            }
            assert_eq!(hook_state(), if installed { HookState::Installed } else { HookState::Failed });
        }
    }

    /// The gate applies after the other edits, leaves no plain `@if(ENVIRONMENT_MAP)` block
    /// (only the module's import), puts both blocks under the opt-in, keeps the braces
    /// balanced, and removing exactly its added text gives the input back.
    #[test]
    fn reflection_gate_applies_after_the_other_edits_and_gates_both_blocks() {
        use reflection_gate::{gate_patched, ungated_blocks, GATE_EDITS};
        let input = chain_before_gate();
        assert_eq!(ungated_blocks(&input).len(), 2, "Bevy's two environment blocks, before the gate");
        let gated = gate_patched(&input).expect("the gate's anchors match");
        assert!(ungated_blocks(&gated).is_empty());
        assert_eq!(gated.matches(&format!("@if(ENVIRONMENT_MAP && {}) {{", nvf_assets::reflection_cube::REFLECT_DEF)).count(), 2, "the def the NIF pipelines set");
        assert_eq!(gated.matches("    if specular_occlusion > 0.0 {\n").count(), 2, "both cube lookups skipped at weight 0, the reflection and the transmitted light");
        assert_eq!(gated.matches("    var transmitted_environment_light: environment_map::EnvironmentMapLight;\n").count(), 1);
        assert!(crate::sun_visibility::tests::balanced(&gated));
        // Undo: each replacement back to its anchor, in order, gives the input.
        let mut undone = gated.clone();
        for (anchor, text) in GATE_EDITS {
            let at = undone.find(text).expect("each replacement present once");
            undone.replace_range(at..at + text.len(), anchor);
        }
        assert_eq!(undone, input);
        // Applied twice it refuses (its anchors are gone): never doubled.
        assert!(gate_patched(&gated).is_none());
    }

    /// A source with an extra plain environment block (a later Bevy's) is refused.
    #[test]
    fn reflection_gate_refuses_a_source_with_another_ungated_block() {
        let input = chain_before_gate().replace("// Total light\n", "// Total light\n@if(ENVIRONMENT_MAP) {\n}\n");
        assert!(reflection_gate::gate_patched(&input).is_none());
    }

    /// Issue #3121: on `FIRST_PERSON_DEPTH` shapes only, shadow-casting spot lights
    /// (the player's flashlight) take the diffuse and specular scales in
    /// `lights/display_shader.rs`, matching the Rust constants, while `!FIRST_PERSON_DEPTH`
    /// keeps full strength and the full shader-patch chain stays balanced.
    #[test]
    fn first_person_flashlight_patch_scales_shadow_spots_only() {
        let chained = reflection_gate::gate_patched(&chain_before_gate())
            .expect("full pbr_functions patch chain applies");
        assert!(crate::sun_visibility::tests::balanced(&chained));
        let diffuse_lit = format!(
            "let nvf_fp_scale = select(1.0, {:.2}, ((*light).flags & mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u);",
            FIRST_PERSON_FLASHLIGHT_DIFFUSE,
        );
        let spec_lit = format!(
            "direct_light += light_contrib * (max(shadow, 0.0) * select(1.0, {:.2}, (view_bindings::clustered_lights.data[light_id].flags & mesh_view_types::POINT_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u));",
            FIRST_PERSON_FLASHLIGHT_SPECULAR,
        );
        assert_eq!(chained.matches(&diffuse_lit).count(), 1, "diffuse scale matches Rust constant once");
        assert_eq!(chained.matches(&spec_lit).count(), 1, "specular scale matches Rust constant once");
        assert_eq!(
            chained.matches("@if(!FIRST_PERSON_DEPTH)\n    let nvf_fp_scale = 1.0;").count(),
            1,
            "world and third-person spot diffuse stay at 1.0",
        );
        assert_eq!(
            // (`max(shadow, 0.0)`: a held lamp's shadow may be under 0, which only the display
            // sum takes; lights/display_shader.rs `DISPLAY_EDITS`.)
            chained.matches("@if(!FIRST_PERSON_DEPTH)\n        direct_light += light_contrib * max(shadow, 0.0);").count(),
            1,
            "world and third-person spot specular stay unscaled",
        );
    }
}
