//! The image-space grade as a full-screen pass after tonemapping: the worldspace's base
//! image space with the weather's adapters for the time of day (fnv_world::imagespace,
//! docs/spec-notes/world.md §6), and on top of it the image space modifiers scripts play
//! on the screen (fnv_world::image_modifier, docs/spec-notes/image-modifiers.md): a fade
//! to a colour, blur, double vision, radial blur, and their changes to the grade.
//!
//! Formula [hypothesis: parameter names from the records; order and the use of display
//! (sRGB) values follow the classic image-space pass, to compare with screenshots]:
//! saturation around the luminance, then a tint towards luminance × tint colour by the
//! tint amount, then brightness as a factor, then contrast around the average luminance.
//!
//! Modifiers (image-modifiers.md §5): the picture is blurred first (two passes of
//! [`blur_pass`], only while a modifier blurs), read doubled and radially blurred by the
//! grade pass, graded with the modifiers' multiply/add on the grade, and last covered by
//! the fade colour. The modifiers on the screen are the script runtime's
//! (`fnv_game::Game::image_modifiers`) and those of `--imod`.
//!
//! Depth of field (image-modifiers.md §8): the modifiers' and the engine's own (VATS
//! focused on the mark, aiming down the sights by the game settings), the strongest, mixed
//! into the blur passes by each point's depth; logged as "depth of field: …" when it
//! changes. Photo mode's, while its Blur is up, is a lens blur instead (`crate::bokeh`:
//! discs sized by each point's circle of confusion, three passes of their own).
//!
//! Motion blur (image-modifiers.md §9): the modifiers' strongest, or the iron sights'
//! setting, smears each point along its own motion on the screen over a short exposure,
//! the motion found by reprojecting the point's depth with the last frame's camera
//! ([`update_motion`], a pass before the blur); logged as "motion blur: …" when it changes.
//!
//! VATS mode's look (docs/design/vats-mode.md §5, `nvf_play::vats_mode::VatsMode::look`)
//! comes after the grade and before the fade colour: the amber grade left of the swipe's
//! edge, the creamy band on the edge and the wash over the whole screen [design].

use crate::sky::SkyState;
use crate::world::WorldData;
use bevy::core_pipeline::fullscreen_material::{fullscreen_material_system, FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::BoxedSystem;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::extract_component::{ComponentUniforms, DynamicUniformIndex, ExtractComponent};
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId, ColorTargetState, ColorWrites, FilterMode, FragmentState,
    Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor,
    ShaderStages, ShaderType, TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::ShaderRef;
use fnv_world::image_modifier::{combine, graded, image_modifier, Dof, ImageModifier, ScreenEffect, DOF_BACK, DOF_FRONT, DOF_NO_SKY};
use fnv_world::imagespace::Hdr;
use nvf_play::tiles::menus::TileMenuBlur;

/// Grade parameters on the main camera.
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType)]
#[extract_app(bevy::render::RenderApp)]
pub struct ImageSpaceGrade {
    /// Tint colour (rgb) and amount (a).
    pub tint: Vec4,
    /// Fade colour (rgb) and amount (a): the picture goes towards the colour.
    pub fade: Vec4,
    /// Radial blur strength, ramp up, start, ramp down.
    pub radial: Vec4,
    /// Radial blur down start, its centre (x, y in screen parts); double vision strength.
    pub radial2: Vec4,
    pub saturation: f32,
    pub contrast: f32,
    pub contrast_avg_lum: f32,
    pub brightness: f32,
    /// Blur radius in the records' units (0 none, 7 the most).
    pub blur: f32,
    /// Seconds, for the swing of double vision.
    pub time: f32,
    /// The blur leaves alone what is nearer than this depth (the depth prepass's value,
    /// larger nearer) and does not take it into the blur of what is behind: 0 for none.
    /// The Pip-Boy keeps itself sharp over the blurred world so.
    pub sharp: f32,
    /// VATS mode: the swipe's edge (a share of the screen width, graded to its left), the
    /// band on the edge, the wash over the screen, the grade's strength (all 0: none).
    pub vats: Vec4,
    /// VATS mode: the grade's colour ([`vats_tint`]), what a picture's lightness is
    /// multiplied by (rgb); and, above 0, that the picture is darkened to this part of
    /// its brightness instead of graded (the classic style).
    pub vats_tint: Vec4,
    /// Depth of field: strength, distance, range (game units), the target's distance
    /// along the view (below 0: the point under the screen's centre).
    pub dof: Vec4,
    /// Depth of field: use target (0/1), the flags, the camera's near plane.
    pub dof_mode: Vec4,
    /// Motion blur: the strength (set by `update_grade`; `update_motion` turns it into
    /// the exposure in frames, 0 for none), the depth nearer than which the player's own
    /// body is left sharp (the depth prepass's value), the longest smear as a part of the
    /// screen height.
    pub motion: Vec4,
    /// Clip space of this frame to clip space of the last one (`update_motion`).
    pub reproject: Mat4,
    /// Photo mode's look (`nvf_play::photo_mode::PhotoLook`): warmth (-1 cool to 1 warm),
    /// vignette (0 to 1), film grain (0 to 1), letterbox aspect (width over height, 0 none).
    pub photo: Vec4,
    /// Photo mode's lens blur (`crate::bokeh`): the aperture (0 off, to 1), the focus
    /// (game units along the view), the largest disc (a part of the screen height).
    pub bokeh: Vec4,
}

/// VATS mode's grade for the interface's colour `rgb` (the settings page's HUD colour):
/// the amber's own figures for amber (docs/design/vats-mode.md §5), and for another
/// colour that colour with its strongest channel lifted as far as amber's red is, so the
/// picture is as bright [design: the fork owner 2026-11-10, the VATS colour "should follow
/// the UI color selected in the menu like the rest of the HUD"].
pub fn vats_tint(rgb: [u8; 3]) -> Vec4 {
    const AMBER: [f32; 3] = [1.45, 0.92, 0.42];
    if rgb == nvf_play::interface::AMBER_RGB {
        return Vec4::new(AMBER[0], AMBER[1], AMBER[2], 0.0);
    }
    let c = rgb.map(|v| f32::from(v) / 255.0);
    let most = c[0].max(c[1]).max(c[2]).max(1e-3);
    let k = AMBER[0] / most;
    Vec4::new(c[0] * k, c[1] * k, c[2] * k, 0.0)
}

impl Default for ImageSpaceGrade {
    fn default() -> Self {
        ImageSpaceGrade {
            tint: Vec4::new(1.0, 1.0, 1.0, 0.0),
            fade: Vec4::ZERO,
            radial: Vec4::ZERO,
            radial2: Vec4::new(0.0, 0.5, 0.5, 0.0),
            saturation: 1.0,
            contrast: 1.0,
            contrast_avg_lum: 0.5,
            brightness: 1.0,
            blur: 0.0,
            time: 0.0,
            sharp: 0.0,
            vats: Vec4::ZERO,
            vats_tint: vats_tint(nvf_play::interface::AMBER_RGB),
            dof: Vec4::new(0.0, 0.0, 0.0, -1.0),
            dof_mode: Vec4::ZERO,
            motion: Vec4::ZERO,
            reproject: Mat4::IDENTITY,
            photo: Vec4::ZERO,
            bokeh: Vec4::ZERO,
        }
    }
}

impl FullscreenMaterial for ImageSpaceGrade {
    fn fragment_shader() -> ShaderRef {
        "embedded://nvforever/imagespace.wesl".into()
    }

    /// After tonemapping: the game grades the final display colours [hypothesis].
    fn schedule_configs(system: ScheduleConfigs<BoxedSystem>) -> ScheduleConfigs<BoxedSystem> {
        system.in_set(Core3dSystems::PostProcess).after(tonemapping)
    }
}

/// A camera whose grade holds only the modifiers held in a 3D scope's lens
/// ([`ScreenModifiers::hold_lens`]), not the image space's or the screen's: its picture is
/// drawn on the lens and graded with the rest of the screen by the main camera after.
#[derive(Component, Clone, Copy, Default)]
pub struct LensGrade;

/// A modifier put on the screen from the command line (`--imod`).
#[derive(Debug, Clone, PartialEq)]
pub struct Forced {
    pub form: u32,
    /// Hold the modifier at this many seconds after its start; without it an animated
    /// one plays again and again, a second apart.
    pub at: Option<f32>,
}

/// The image space modifiers on the screen.
#[derive(Resource, Default)]
pub struct ScreenModifiers {
    records: HashMap<u32, Option<ImageModifier>>,
    pub forced: Vec<Forced>,
    /// Modifiers the engine holds on the screen at their first keys while a state lasts:
    /// a night-vision scope's (`scope_view.rs`, docs/spec-notes/weapon-scopes.md §4).
    pub held: Vec<u32>,
    /// Modifiers held the same way in a 3D scope's lens only ([`LensGrade`]): a
    /// night-vision scope's while the scope is shown in the lens.
    pub lens_held: Vec<u32>,
    /// Modifiers the engine plays once (an explosion's, `shot_fx`): the record and when it
    /// started, in `Time::elapsed_secs`.
    played: Vec<(u32, f32)>,
    /// Records played once that are not animatable and were left off the screen.
    refused: Vec<u32>,
    /// The modifiers of the last frame, to log what comes and goes.
    shown: Vec<u32>,
    /// The game settings of the engine's own depth of field, read once.
    dof_settings: Option<DofSettings>,
    /// The depth of field logged last.
    dof_shown: String,
    /// The motion blur logged last.
    motion_shown: String,
    /// The HDR values on the screen now: the image space's (with the weather's adapters)
    /// and the same with the modifiers on it ([`ScreenModifiers::hdr_ratio`]).
    hdr: Option<(Hdr, Hdr)>,
    /// The modifiers whose intro sound was played and that are still on the screen
    /// ([`modifier_sounds`]).
    sounded: Vec<u32>,
}

impl ScreenModifiers {
    /// Play modifier `form` once from `now`; one already playing starts again, as a
    /// script's does (image-modifiers.md §3). Only an animatable record is shown: one that
    /// is not would stay until something removes it [design].
    pub fn play(&mut self, form: u32, now: f32) {
        self.played.retain(|(f, _)| *f != form);
        self.played.push((form, now));
    }

    /// Hold modifier `form` on the screen at its first keys (`on`), or take it off.
    pub fn hold(&mut self, form: u32, on: bool) {
        self.held.retain(|f| *f != form);
        if on {
            self.held.push(form);
        }
    }

    /// Hold modifier `form` in a 3D scope's lens ([`LensGrade`]) at its first keys (`on`),
    /// or take it off.
    pub fn hold_lens(&mut self, form: u32, on: bool) {
        self.lens_held.retain(|f| *f != form);
        if on {
            self.lens_held.push(form);
        }
    }

    /// How much the modifiers on the screen change HDR value `i` (`fnv_world::imagespace::
    /// HDR_*`): the value with them over the value without; 1 when they leave it alone or
    /// the image space has none (image-modifiers.md §11).
    pub fn hdr_ratio(&self, i: usize) -> f32 {
        match &self.hdr {
            Some((base, graded)) if base.v[i] > 0.0 && graded.v[i].is_finite() => (graded.v[i] / base.v[i]).max(0.0),
            _ => 1.0,
        }
    }

    /// The HDR values on the screen now, modifiers included (the image space's own before
    /// the first frame is graded).
    pub fn graded_hdr(&self) -> Option<Hdr> {
        self.hdr.map(|(_, g)| g)
    }

    #[cfg(test)]
    pub(crate) fn set_hdr_for_test(&mut self, base: Hdr, graded: Hdr) {
        self.hdr = Some((base, graded));
    }

    fn record(&mut self, world: &WorldData, form: u32) -> Option<&ImageModifier> {
        self.records.entry(form).or_insert_with(|| image_modifier(world.lo, world.schema, form)).as_ref()
    }
}

/// Current assigned trait values for nonanimated records (image-modifiers.md).
/// Keep the decoded record immutable: another session/lens must not inherit edits.
fn live_modifier_effect(m: &ImageModifier, seconds: f32, game: Option<&fnv_game::Game<'_>>) -> ScreenEffect {
    let mut effect = m.at(seconds);
    let Some(game) = game.filter(|g| !m.animatable && !g.traits.values.is_empty()) else { return effect };
    for trait_id in 1..=63 {
        let Some(value) = game.traits.image_modifier_override(fnv_sim::FormId(m.form_id), trait_id) else { continue };
        let value = value as f32;
        if !value.is_finite() { continue; }
        match trait_id {
            1 => effect.radial.center[0] = value,
            2 => effect.radial.center[1] = value,
            3..=36 => {
                let slot = ((trait_id - 3) / 2) as usize;
                if (trait_id - 3) % 2 == 0 {
                    effect.cinematic.hdr[slot].mult = value;
                } else {
                    effect.cinematic.hdr[slot].add = value;
                }
            }
            37 => effect.cinematic.saturation.mult = value,
            38 => effect.cinematic.saturation.add = value,
            // Match the IMAD getter's IAD signatures and the record decoder's field order.
            39 => effect.cinematic.contrast_avg_lum.mult = value,
            40 => effect.cinematic.contrast_avg_lum.add = value,
            41 => effect.cinematic.contrast.mult = value,
            42 => effect.cinematic.contrast.add = value,
            43 => effect.cinematic.brightness.mult = value,
            44 => effect.cinematic.brightness.add = value,
            45 => effect.blur_radius = value,
            46 => effect.double_vision = value,
            // Tint RGBA are all 0–255, alpha included (image-modifiers.md).
            47..=50 => effect.tint[(trait_id - 47) as usize] = value / 255.0,
            // Fade RGB are 0–255; alpha keeps the getter's normalized units (image-modifiers.md).
            51..=53 => effect.fade[(trait_id - 51) as usize] = value / 255.0,
            54 => effect.fade[3] = value,
            55 => effect.radial.strength = value,
            56 => effect.radial.ramp_up = value,
            57 => effect.radial.start = value,
            58 => effect.radial.ramp_down = value,
            59 => effect.radial.down_start = value,
            60 => effect.dof.strength = value,
            61 => effect.dof.distance = value,
            62 => effect.dof.range = value,
            63 => effect.motion_blur = value,
            _ => unreachable!(),
        }
    }
    effect
}

/// `--imod <Editor ID or 0xFormID>[@seconds]`.
pub fn parse_forced(world: &WorldData, arg: &str) -> Result<Forced, String> {
    let (name, at) = match arg.split_once('@') {
        Some((n, t)) => (n, Some(t.parse::<f32>().map_err(|_| format!("--imod {arg}: {t} is not a number of seconds"))?)),
        None => (arg, None),
    };
    let form = fnv_world::image_modifier::find_image_modifier(world.lo, world.schema, name).ok_or_else(|| format!("--imod {arg}: no image space modifier {name}"))?;
    Ok(Forced { form, at })
}

pub struct ImageSpacePlugin;

impl Plugin for ImageSpacePlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "imagespace.wesl");
        bevy::asset::embedded_asset!(app, "imagespace_blur.wesl");
        app.init_resource::<ScreenModifiers>()
            .add_plugins(FullscreenMaterialPlugin::<ImageSpaceGrade>::default())
            // A modifier's sounds: before sfx plays them.
            .add_systems(Update, update_grade.after(crate::sky::update_sky).before(nvf_play::sfx::SfxSet))
            .add_systems(PostUpdate, update_motion.after(bevy::transform::TransformSystems::Propagate).after(bevy::camera::CameraUpdateSystems))
            .add_systems(PostUpdate, tile_menu_blur);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .add_systems(RenderStartup, init_blur)
            .add_systems(Render, prepare_blur.in_set(RenderSystems::Prepare))
            .add_systems(Core3d, blur_pass.in_set(Core3dSystems::PostProcess).after(tonemapping).after(bevy::anti_alias::fxaa::fxaa).after(bevy::anti_alias::smaa::smaa).before(fullscreen_material_system::<ImageSpaceGrade>));
    }
}

/// The modifiers on the screen now as one effect: the script runtime's, each at its own
/// age, and the command line's; with the forms on the screen and the Editor IDs of the
/// ones whose depth of field and motion blur are shown.
fn screen_effect(mods: &mut ScreenModifiers, world: &WorldData, script: Option<&nvf_play::script::ScriptState>, now: f32) -> (ScreenEffect, Vec<u32>, String, String) {
    let mut playing: Vec<(u32, f32)> = Vec::new();
    if let Some(rt) = script.and_then(|s| s.rt.as_ref()) {
        // Draw at most one active instance per IMAD form (keeping the most recently
        // applied entry, preserving pre-stacking visuals for unmodded scripts such as
        // VCG00's Addiction01ISFX while Game counts stacked instances for OnRemoveIMOD)
        // [design].
        for m in &rt.game.image_modifiers {
            let form = m.modifier.0;
            let elapsed = m.elapsed as f32;
            if let Some(slot) = playing.iter_mut().find(|(f, _)| *f == form) {
                slot.1 = elapsed;
            } else {
                playing.push((form, elapsed));
            }
        }
    }
    for f in mods.forced.clone() {
        let seconds = f.at.unwrap_or_else(|| {
            // Again and again, a second apart.
            let d = mods.record(world, f.form).filter(|m| m.animatable).map_or(0.0, |m| m.duration);
            if d > 0.0 {
                now % (d + 1.0)
            } else {
                now
            }
        });
        playing.push((f.form, seconds));
    }
    playing.extend(mods.held.iter().map(|&f| (f, 0.0)));
    // Those played once: gone when they have run.
    let mut played = std::mem::take(&mut mods.played);
    played.retain(|&(form, start)| match mods.record(world, form) {
        Some(m) if m.animatable => !m.finished(now - start),
        _ => {
            if !mods.refused.contains(&form) {
                mods.refused.push(form);
                warn!("image space modifier 0x{form:08X} played once is not animatable (or not found): left off the screen");
            }
            false
        }
    });
    playing.extend(played.iter().map(|&(form, start)| (form, now - start)));
    mods.played = played;
    let mut effects = Vec::new();
    let mut forms = Vec::new();
    for (form, seconds) in playing {
        let Some(m) = mods.record(world, form) else { continue };
        // A looping `--imod` is off the screen in its second apart.
        if m.finished(seconds) {
            continue;
        }
        effects.push(live_modifier_effect(m, seconds, script.and_then(|s| s.rt.as_ref()).map(|rt| &rt.game)));
        forms.push(form);
    }
    // The strongest depth of field and motion blur are the ones shown (`combine`).
    let (mut dof_source, mut motion_source) = (String::new(), String::new());
    let (mut top, mut top_motion) = (0.0, 0.0);
    for (e, &form) in effects.iter().zip(&forms) {
        if e.dof.strength > top {
            top = e.dof.strength;
            dof_source = mods.record(world, form).map_or_else(|| format!("{form:#010x}"), |m| m.edid.clone());
        }
        if e.motion_blur > top_motion {
            top_motion = e.motion_blur;
            motion_source = mods.record(world, form).map_or_else(|| format!("{form:#010x}"), |m| m.edid.clone());
        }
    }
    (combine(&effects), forms, dof_source, motion_source)
}

/// The modifiers' sounds (image-modifiers.md §10): a modifier that comes on the screen
/// plays its intro sound (RDSD) and one that leaves it its outro sound (RDSI), at the
/// listener [hypothesis: the xEdit names "Sound - Intro" and "Sound - Outro"; the data
/// fits, NVDLC03BattleBrewDrinkISFX starts a heartbeat and ends with
/// NVDLC03RedMistWearOff, the stealth suit's ON and OFF modifiers have UIRadioSignalFound
/// and UIRadioSignalLost as intro]. Logged as "image space modifier sound: …".
fn modifier_sounds(mods: &mut ScreenModifiers, world: &WorldData, forms: &[u32], mut sounds: Option<ResMut<Messages<nvf_play::sfx::PlaySound>>>) {
    let came: Vec<u32> = forms.iter().copied().filter(|f| !mods.sounded.contains(f)).collect();
    let went: Vec<u32> = mods.sounded.iter().copied().filter(|f| !forms.contains(f)).collect();
    if came.is_empty() && went.is_empty() {
        return;
    }
    let mut play = |mods: &mut ScreenModifiers, form: u32, intro: bool| {
        let Some(m) = mods.record(world, form) else { return };
        let Some(sound) = (if intro { m.intro_sound } else { m.outro_sound }) else { return };
        info!("image space modifier sound: {} {} 0x{sound:08X}", m.edid, if intro { "intro" } else { "outro" });
        if let Some(s) = sounds.as_deref_mut() {
            s.write(nvf_play::sfx::PlaySound { sound: fnv_sim::FormId(sound), at: None });
        }
    };
    for f in came {
        play(mods, f, true);
        mods.sounded.push(f);
    }
    for f in went {
        play(mods, f, false);
        mods.sounded.retain(|&x| x != f);
    }
}

/// The depth of field in a conversation (image-modifiers.md §8.4), behind the speaker only:
/// the mix towards the blur at full zoom (past 1 it turns into the contrast effect of §8.5),
/// what stays clean behind the face (the speaker's own body), and where the blur is full,
/// game units [design: to tune against the original's shots: a wall close behind the
/// speaker a little blurred, a far background strongly].
const TALK_DOF_STRENGTH: f32 = 1.0;
const TALK_DOF_DISTANCE: f32 = 32.0;
const TALK_DOF_RANGE: f32 = 512.0;

/// The game settings of the engine's own depth of field (image-modifiers.md §8.2, §8.4)
/// [observed values: FalloutNV.esm GMST; the fallbacks are those values].
#[derive(Debug, Clone, Copy)]
struct DofSettings {
    vats_range: f32,
    vats_strength: f32,
    sights_distance: f32,
    sights_range: f32,
    sights_strength: f32,
    /// Motion blur down the sights at full zoom (§9.3).
    sights_motion_blur: f32,
}

impl DofSettings {
    fn read(sim: Option<&nvf_play::sim::SimState>) -> DofSettings {
        let get = |name: &str, or: f32| sim.and_then(|s| s.forms.setting_f32(name)).unwrap_or(or);
        DofSettings {
            vats_range: get("fVATSDOFRange", 500.0),
            vats_strength: get("fVATSDOFStrengthCap", 0.6),
            sights_distance: get("fIronSightsDOFDistance", 275.0),
            sights_range: get("fIronSightsDOFRange", 650.0),
            sights_strength: get("fIronSightsDOFStrengthCap", 0.0),
            sights_motion_blur: get("fIronSightsMotionBlur", 0.0),
        }
    }
}

/// The engine's own depth of field (image-modifiers.md §8.4) [hypothesis]: VATS focused
/// on the mark (`vats_focus`, its distance along the view), beyond it only, faded with
/// the mode's look (`vats_grade`, 0 … 1); aiming down the sights by the zoom's
/// `aim_blend`; a conversation on the speaker's face (`talk`: the zoom's blend and the face's
/// distance from the camera), beyond it only. The strongest, with the target's distance
/// (below 0: none) and its name.
fn engine_dof(s: &DofSettings, vats_grade: f32, vats_focus: Option<f32>, aim_blend: f32, talk: Option<(f32, f32)>) -> Option<(Dof, f32, &'static str)> {
    let vats = vats_focus
        .filter(|_| vats_grade > 0.0 && s.vats_strength > 0.0)
        .map(|f| (Dof { strength: s.vats_strength * vats_grade, distance: 0.0, range: s.vats_range, use_target: true, flags: DOF_BACK }, f.max(0.0), "VATS"));
    let sights = (aim_blend > 0.0 && s.sights_strength > 0.0).then(|| {
        (Dof { strength: s.sights_strength * aim_blend, distance: s.sights_distance, range: s.sights_range, use_target: false, flags: 0 }, -1.0, "iron sights")
    });
    let talk = talk.filter(|(blend, _)| *blend > 0.0).map(|(blend, f)| {
        (Dof { strength: TALK_DOF_STRENGTH * blend, distance: TALK_DOF_DISTANCE, range: TALK_DOF_RANGE, use_target: true, flags: DOF_BACK }, f.max(0.0), "dialogue")
    });
    [vats, sights, talk].into_iter().flatten().reduce(|a, b| if b.0.strength > a.0.strength { b } else { a })
}

/// The log line's text for photo mode's lens blur (rounded as [`dof_text`]).
fn bokeh_text(aperture: f32, focus: f32) -> String {
    format!("lens blur (bokeh), aperture {aperture:.2}, focus {:.0}, discs up to {:.1}% of the screen height (photo mode)", (focus / 10.0).round() * 10.0, aperture * crate::bokeh::MAX_COC * 100.0)
}

/// The log line's text for a depth of field (rounded, so a fade logs a few lines).
fn dof_text(d: &Dof, focus: f32, source: &str) -> String {
    if d.strength == 0.0 {
        return "off".to_string();
    }
    let mut t = format!("strength {:.1}, distance {:.0}, range {:.0}", d.strength, d.distance, d.range);
    if d.use_target {
        if focus >= 0.0 {
            t += &format!(", focus {:.0}", (focus / 100.0).round() * 100.0);
        } else {
            t += ", focus on the screen's centre";
        }
        if d.flags & DOF_FRONT != 0 {
            t += ", front";
        }
        if d.flags & DOF_BACK != 0 {
            t += ", back";
        }
    }
    if d.flags & DOF_NO_SKY != 0 {
        t += ", no sky";
    }
    t + &format!(" ({source})")
}

/// The motion blur shown: the modifiers' `strength` from `source`, or the iron sights'
/// setting by the zoom's `aim_blend` when stronger (image-modifiers.md §9.3); with the
/// log line's text ("off" for none; strength in steps of 0.05, so a fade logs a few
/// lines).
fn motion_blur(s: Option<&DofSettings>, strength: f32, source: &str, aim_blend: f32) -> (f32, String) {
    let sights = s.map_or(0.0, |s| s.sights_motion_blur * aim_blend);
    let (v, name) = if sights > strength { (sights, "iron sights") } else { (strength, source) };
    if v <= 0.0 {
        return (0.0, "off".to_string());
    }
    let step = (v * 20.0).round() / 20.0;
    let shown = if step > 0.0 { format!("{step:.2}") } else { "under 0.05".to_string() };
    (v, format!("strength {shown} ({name})"))
}

/// The exposure of the motion blur per unit of strength, in seconds: each point is
/// smeared along where it went on the screen over the last `strength ×` this [hypothesis:
/// the wiki gives 0 as none and 10 as the most, "how much to blur the screen while
/// moving"; 10 then smears a third of a second, and the records' 0.15 already shows a
/// plain smear on a brisk turn: smoke imagespace-motion-blur's motion_turning.png].
const MOTION_EXPOSURE: f32 = 1.0 / 30.0;

/// The longest smear, as a part of the screen height [hypothesis]; strengths above about
/// 1 reach it when the view turns briskly.
const MOTION_MAX_REACH: f32 = 0.08;

/// What is nearer to the eye than this (game units) moves with the view, the player's own
/// arms and weapon, and is not smeared [hypothesis: the game's own setting for the gun,
/// `fIronSightsGunMotionBlur`, is 0; the arms lie within it, as for the Pip-Boy].
const MOTION_SHARP: f32 = 40.0;

/// A camera that moved further than this (game units) or turned more than
/// [`MOTION_CUT_TURN`] (radians) in one frame was cut to a new view, not moved: no smear
/// that frame [design].
const MOTION_CUT_DISTANCE: f32 = 300.0;
const MOTION_CUT_TURN: f32 = std::f32::consts::FRAC_PI_4;

/// The motion blur's exposure in frames for a frame of `dt` seconds in which the camera
/// moved `moved` units and turned `turned` radians: what the last frame's motion on the
/// screen is scaled by (0: none).
fn motion_frames(strength: f32, dt: f32, moved: f32, turned: f32) -> f32 {
    if strength <= 0.0 || dt <= 0.0 || moved > MOTION_CUT_DISTANCE || turned > MOTION_CUT_TURN {
        return 0.0;
    }
    strength * MOTION_EXPOSURE / dt
}

/// The camera of the last frame, per graded camera: its clip-from-world, place and turn.
#[derive(Default)]
struct LastViews(HashMap<Entity, (Mat4, Vec3, Quat)>);

/// After the camera has moved for this frame: the reprojection from this frame's clip
/// space to the last frame's and the exposure in frames (`motion_frames`) on each graded
/// camera.
fn update_motion(time: Res<Time>, mut last: Local<LastViews>, mut cams: Query<(Entity, &Camera, &GlobalTransform, &mut ImageSpaceGrade)>) {
    let dt = time.delta_secs();
    for (entity, camera, transform, mut grade) in &mut cams {
        let (_, rot, pos) = transform.to_scale_rotation_translation();
        let clip_from_world = camera.clip_from_view() * transform.to_matrix().inverse();
        let Some((prev, prev_pos, prev_rot)) = last.0.insert(entity, (clip_from_world, pos, rot)) else {
            grade.motion.x = 0.0;
            continue;
        };
        grade.motion.x = motion_frames(grade.motion.x, dt, pos.distance(prev_pos), prev_rot.angle_between(rot));
        grade.reproject = prev * clip_from_world.inverse();
    }
}

/// The blur behind the open wait menu, in the modifiers' blur radius units [engineering
/// choice: by eye against the owner's screenshot of the original menu].
const WAIT_MENU_BLUR: f32 = 3.0;

/// The blur behind a menu drawn from its tiles, in the same units, on and off at once as the
/// wait menu's [design: the original's shots show the world blurred behind the tutorial and
/// container menus (docs/spec-notes/tutorial-menu.md §5), not how strongly or how fast].
const TILE_MENU_BLUR: f32 = 3.0;
/// The blur behind the pause menu: a third of a tile menu's [design: owner 2026-11-15, "the
/// bluring of the box behind the whole menu is way too strong" at 3].
const PAUSE_MENU_BLUR: f32 = 1.0;

/// The blur of the world behind the Pip-Boy, in the same units [documented:
/// `Fallout_default.ini` `[Pipboy] fBlurRadiusPipboy=3.5`; hypothesis: the same units as
/// a modifier's blur radius; its `fBlurIntensityPipboy=0.25` is not used, what it scales
/// is not known]. Eased in and out with the Pip-Boy's clip.
const PIPBOY_BLUR: f32 = 3.5;

/// While the Pip-Boy shows, what is nearer to the eye than this (game units) is the
/// player's own arms and is not blurred [engineering choice: the Pip-Boy is about 11 in
/// front of the eye; the nearest world the player can stand against is further].
const PIPBOY_SHARP: f32 = 40.0;

/// A fade colour and amount (`ScreenEffect::fade`) with black laid over it to `f` (0 …
/// 1): the share of the screen covered grows to `f` of what was left, and the colour
/// darkens with it.
pub(crate) fn black_over(fade: [f32; 4], f: f32) -> [f32; 4] {
    let f = f.clamp(0.0, 1.0);
    let a = fade[3].clamp(0.0, 1.0);
    let covered = a + (1.0 - a) * f;
    let keep = if covered > 0.0 { a * (1.0 - f) / covered } else { 0.0 };
    [fade[0] * keep, fade[1] * keep, fade[2] * keep, covered]
}

/// Base cinematic grade used before applying active `IMAD` modifiers: under Physical
/// lighting outdoors, set aside the weather's tint, contrast and brightness (made for the
/// display values, not linear light) while keeping its HDR values and letting gameplay
/// modifiers (`effect`) compose cleanly.
fn effective_grade_base(base: fnv_world::imagespace::Cinematic, physical: bool, indoors: bool) -> fnv_world::imagespace::Cinematic {
    if physical && !indoors {
        fnv_world::imagespace::Cinematic { hdr: base.hdr, ..fnv_world::imagespace::Cinematic::default() }
    } else {
        base
    }
}

/// Copy the sky's grade, with the modifiers on the screen, onto every graded camera.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn update_grade(
    time: Res<Time>,
    sky: Res<SkyState>,
    world: Option<Res<WorldData>>,
    script: Option<Res<nvf_play::script::ScriptState>>,
    new_game: Option<Res<nvf_play::script::NewGame>>,
    wait: Option<Res<nvf_play::wait::WaitMenu>>,
    sleep: Option<Res<nvf_play::wait::SleepFade>>,
    (pip, projections, death, vats): (
        Option<Res<nvf_play::pipboy::PipBoy>>,
        Query<(&Projection, &GlobalTransform), With<nvf_play::PlayerCamera>>,
        Option<Res<crate::death_cam::DeathCam>>,
        Option<Res<nvf_play::vats_mode::VatsMode>>,
    ),
    (sim, aim, wheel, talk, creator, photo): (Option<Res<nvf_play::sim::SimState>>, Option<Res<nvf_play::aim_zoom::AimZoom>>, Option<Res<nvf_play::weapon_wheel::WeaponWheel>>, Option<Res<nvf_play::dialogue_zoom::DialogueZoom>>, Option<Res<nvf_play::creator::CreatorMenu>>, Option<Res<nvf_play::photo_mode::PhotoMode>>),
    mut mods: ResMut<ScreenModifiers>,
    mut grades: Query<(&mut ImageSpaceGrade, Has<LensGrade>)>,
    sounds: Option<ResMut<Messages<nvf_play::sfx::PlaySound>>>,
) {
    let now = time.elapsed_secs();
    let (mut effect, forms, dof_source, motion_source) = match &world {
        Some(w) => screen_effect(&mut mods, w, script.as_deref(), now),
        None => (ScreenEffect::default(), Vec::new(), String::new(), String::new()),
    };
    // The gene projector's studio (`creator_view.rs`) is a picture of its own, not the
    // scene: the scene's modifiers are left off it (they go on playing and come back when
    // it closes) [design: owner 2026-11-08, the studio was pure black in Tale of Two
    // Wastelands' birth scene, where CG00BirthBaseISFX (Bright Scale x2, Bright Clamp
    // x0.2) and the eye adaptation it drives left the player a silhouette on black]. The
    // cell's own grade stays.
    if creator.as_deref().is_some_and(nvf_play::creator::CreatorMenu::in_studio) {
        effect = ScreenEffect::default();
    }
    // A new game shows nothing of the world before its opening runs: until the scripts
    // have started the screen is white, as the opening's first modifier then holds it
    // [engineering choice: the script data loads while the room is already drawn].
    if new_game.is_some() && script.as_deref().is_some_and(|s| s.rt.is_none() && s.off.is_none()) {
        effect.fade = [1.0; 4];
    }
    // Sleeping fades the world to black under the menu (`nvf_play::wait`), and the dead
    // player's screen goes black (`death_cam.rs`): black over whatever fade is on the
    // screen already.
    if let Some(f) = sleep.map(|s| s.0).filter(|f| *f > 0.0) {
        effect.fade = black_over(effect.fade, f);
    }
    if let Some(f) = death.as_deref().map(|d| d.black()).filter(|f| *f > 0.0) {
        effect.fade = black_over(effect.fade, f);
    }
    if forms != mods.shown {
        if let Some(w) = &world {
            let names: Vec<String> = forms.iter().map(|&f| mods.record(w, f).map_or_else(|| format!("{f:#010x}"), |m| m.edid.clone())).collect();
            info!("image space modifiers on the screen: {}", if names.is_empty() { "none".to_string() } else { names.join(", ") });
        }
        mods.shown = forms.clone();
    }
    // The world behind the Pip-Boy is blurred, the Pip-Boy is not.
    let camera = projections.single().ok();
    let near = camera.and_then(|(p, _)| match p {
        Projection::Perspective(p) => Some(p.near),
        _ => None,
    });
    // Depth of field: the modifiers' or the engine's own, the stronger.
    if mods.dof_settings.is_none() && sim.is_some() {
        mods.dof_settings = Some(DofSettings::read(sim.as_deref()));
    }
    let vats_look = vats.as_deref().map_or([0.0; 4], |v| v.look());
    let vats_focus = vats.as_deref().and_then(|v| v.focus()).zip(camera).map(|(p, (_, t))| (nvf_play::space::to_bevy(p) - t.translation()).dot(*t.forward()));
    let aim_blend = aim.as_deref().map_or(0.0, |a| if a.sight.is_some() { a.blend } else { 0.0 });
    // Straight to the face, not along the view: looking around does not move the focus.
    let talk_focus = talk.as_deref().and_then(|z| z.face().map(|p| (z.blend(), p))).zip(camera).map(|((blend, p), (_, t))| (blend, (nvf_play::space::to_bevy(p) - t.translation()).length()));
    let (mut dof, mut focus, mut source) = (effect.dof, -1.0f32, dof_source);
    if let Some((d, f, name)) = mods.dof_settings.as_ref().and_then(|s| engine_dof(s, vats_look[3], vats_focus, aim_blend, talk_focus)) {
        if d.strength > dof.strength {
            (dof, focus, source) = (d, f, name.to_string());
        }
    }
    // Photo mode's own depth of field, while its Blur is up, wins over every other: its
    // lens blur (`crate::bokeh`), in place of the Gaussian one.
    let photo_dof = photo.as_deref().and_then(|p| p.depth_of_field());
    if photo_dof.is_some() {
        (dof, focus) = (Dof::default(), -1.0);
    }
    let bokeh = photo_dof.map_or(Vec4::ZERO, |p| Vec4::new(p.strength, p.focus, crate::bokeh::MAX_COC, 0.0));
    let text = match photo_dof {
        Some(p) => bokeh_text(p.strength, p.focus),
        None => dof_text(&dof, focus, &source),
    };
    if text != mods.dof_shown {
        info!("depth of field: {text}");
        mods.dof_shown = text;
    }
    let (motion, text) = motion_blur(mods.dof_settings.as_ref(), effect.motion_blur, &motion_source, aim_blend);
    if text != mods.motion_shown {
        info!("motion blur: {text}");
        mods.motion_shown = text;
    }
    let pipboy = pip.filter(|p| p.on_arm()).map_or(0.0, |p| crate::player_body::pipboy_ease(p.clip_time));
    let (pip_blur, sharp) = match near {
        Some(n) if pipboy > 0.0 => (PIPBOY_BLUR * pipboy, n / PIPBOY_SHARP),
        _ => (0.0, 0.0),
    };
    let base = sky.cinematic();
    let c_hdr = graded(&base, &effect);
    mods.hdr = Some((base.hdr, c_hdr.hdr));
    let grade_base = effective_grade_base(base, nvf_assets::authored::physical_pbr(), sky.interior.is_some());
    let c = graded(&grade_base, &effect);
    if let Some(w) = &world {
        modifier_sounds(&mut mods, w, &forms, sounds);
    }
    let r = &effect.radial;
    // Photo mode's sliders on top of the grade (`nvf_play::photo_mode`), and no motion blur
    // while its camera moves for the shot [design].
    let (photo_b, photo_s, photo_c) = photo.as_deref().and_then(|p| p.look()).unwrap_or((1.0, 1.0, 1.0));
    let motion = if photo.as_deref().is_some_and(|p| p.on) { 0.0 } else { motion };
    let g = ImageSpaceGrade {
        tint: Vec4::new(c.tint[0], c.tint[1], c.tint[2], c.tint_amount),
        fade: Vec4::from_array(effect.fade),
        radial: Vec4::new(r.strength, r.ramp_up, r.start, r.ramp_down),
        radial2: Vec4::new(r.down_start, r.center[0], r.center[1], effect.double_vision),
        saturation: c.saturation * photo_s,
        contrast: c.contrast * photo_c,
        contrast_avg_lum: c.contrast_avg_lum,
        brightness: c.brightness * photo_b,
        // The world behind the wait menu is blurred (docs/spec-notes/wait-menu.md §2).
        // The dying player's view blurs (`death_cam.rs`), and the world behind the weapon
        // wheel (docs/design/weapon-wheel.md §5).
        blur: effect.blur_radius.max(if wait.is_some_and(|w| w.is_open()) { WAIT_MENU_BLUR } else { 0.0 }).max(pip_blur).max(death.map_or(0.0, |d| d.blur())).max(wheel.map_or(0.0, |w| w.blur())),
        time: now,
        sharp,
        vats: Vec4::from_array(vats_look),
        // The mode's own amber, whatever the HUD's colour [design: the fork owner
        // 2026-11-10, after trying it with a white HUD: "revert ... back to the previous
        // orange"]: Dead Eye's grade. The classic style has no colour over the picture:
        // the fourth figure is how dark it goes instead (`vats_classic::DARKEN`; 0 for
        // the grade).
        vats_tint: vats_tint(nvf_play::interface::AMBER_RGB).with_w(if vats.as_deref().is_some_and(|v| v.style() == nvf_play::vats_classic::VatsStyle::Classic) { nvf_play::vats_classic::DARKEN } else { 0.0 }),
        dof: Vec4::new(dof.strength, dof.distance, dof.range, focus),
        dof_mode: Vec4::new(if dof.use_target { 1.0 } else { 0.0 }, f32::from(dof.flags), near.unwrap_or(1.0), 0.0),
        motion: Vec4::new(motion, near.map_or(0.0, |n| n / MOTION_SHARP), MOTION_MAX_REACH, 0.0),
        reproject: Mat4::IDENTITY,
        photo: Vec4::from_array(photo.as_deref().map_or([0.0; 4], |p| p.finish())),
        bokeh,
    };
    // A 3D scope's lens: a neutral grade with only the lens's modifiers on it.
    let lens = match &world {
        Some(w) if !mods.lens_held.is_empty() => {
            let game = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
            let effects: Vec<ScreenEffect> = mods.lens_held.clone().into_iter().filter_map(|f| mods.record(w, f).map(|m| live_modifier_effect(m, 0.0, game))).collect();
            let e = combine(&effects);
            let c = graded(&fnv_world::imagespace::Cinematic::default(), &e);
            let r = &e.radial;
            ImageSpaceGrade {
                tint: Vec4::new(c.tint[0], c.tint[1], c.tint[2], c.tint_amount),
                radial: Vec4::new(r.strength, r.ramp_up, r.start, r.ramp_down),
                radial2: Vec4::new(r.down_start, r.center[0], r.center[1], e.double_vision),
                saturation: c.saturation,
                contrast: c.contrast,
                contrast_avg_lum: c.contrast_avg_lum,
                brightness: c.brightness,
                blur: e.blur_radius,
                time: now,
                ..ImageSpaceGrade::default()
            }
        }
        _ => ImageSpaceGrade { time: now, ..ImageSpaceGrade::default() },
    };
    for (mut grade, in_lens) in &mut grades {
        *grade = if in_lens { lens } else { g };
    }
}

/// What [`tile_menu_blur`] tells of a state: this contribution, not the screen's whole blur.
fn tile_menu_blur_text(now: TileMenuBlur) -> String {
    let names: Vec<&str> = [(now.tutorial, nvf_play::tiles::tutorial_menu::TUTORIAL_MENU), (now.container, "ContainerMenu"), (now.barter, nvf_play::tiles::barter_menu::BARTER_MENU), (now.recipe, nvf_play::tiles::recipe_menu::RECIPE_MENU), (now.traits, nvf_play::tiles::trait_menu::TRAIT_MENU), (now.tags, nvf_play::tiles::tag_menu::TAG_MENU)].into_iter().filter(|m| m.0).map(|m| m.1).collect();
    let names: Vec<&str> = names.into_iter().chain(now.repair_services.then_some(nvf_play::tiles::repair_services_menu::REPAIR_SERVICES_MENU)).collect();
    if names.is_empty() {
        "off".to_string()
    } else {
        format!("{} radius {TILE_MENU_BLUR}", names.join(", "))
    }
}

/// The world blurred behind a menu drawn from its tiles: the strongest of this and what
/// `update_grade` worked out this frame (the modifiers, the wait menu, the Pip-Boy, death, the
/// weapon wheel), so closing the menu leaves theirs as they were [design]. After `Update`, so
/// the drawing's decision of this frame (`nvf_play::tiles::draw`) reaches this frame's picture
/// whatever the order of `update_grade` and the drawing; `update_grade` writes every grade
/// afresh each frame, so nothing stays when the menu goes. A 3D scope's lens has its own
/// grade, which holds only the lens's modifiers, and `sharp`, what the Pip-Boy keeps sharp, is
/// left alone. With nothing drawn from tiles no grade is touched.
fn tile_menu_blur(
    drawing: Option<Res<TileMenuBlur>>,
    mut last: Local<TileMenuBlur>,
    mut grades: Query<&mut ImageSpaceGrade, Without<LensGrade>>,
    (paused, main_menu, mut was_paused): (Option<Res<nvf_play::pause::Paused>>, Option<Res<nvf_play::main_menu::MainMenu>>, Local<bool>),
) {
    // The pause menu over a game blurs the world behind it as a tile menu does, so its text
    // reads over a busy picture [design: owner 2026-11-15, the menu overhaul].
    let pause = paused.is_some_and(|p| p.0) && !main_menu.is_some_and(|m| m.open);
    if pause != *was_paused {
        info!("pause menu blur: {}", if pause { format!("radius {PAUSE_MENU_BLUR}") } else { "off".to_string() });
        *was_paused = pause;
    }
    if pause {
        for mut grade in &mut grades {
            grade.blur = grade.blur.max(PAUSE_MENU_BLUR);
        }
    }
    let now = drawing.as_deref().copied().unwrap_or_default();
    if now != *last {
        info!("tile menu blur: {}", tile_menu_blur_text(now));
        // What the menu had covered: the grades hold this frame's blur without the menu's.
        let own = grades.iter().map(|g| g.blur).fold(0.0, f32::max);
        if !now.any() && own > 0.0 {
            info!("tile menu blur: the screen's own blur {own:.1} stays");
        }
        *last = now;
    }
    if !now.any() {
        return;
    }
    for mut grade in &mut grades {
        grade.blur = grade.blur.max(TILE_MENU_BLUR);
    }
}

/// The blur passes' pipelines: the motion blur, across and down, per target format and whether the
/// view's depth is multisampled (the depth is read to keep what is near sharp).
#[derive(Resource)]
pub(crate) struct BlurPipelines {
    /// The layouts with a single-sampled and a multisampled depth.
    layouts: [BindGroupLayoutDescriptor; 2],
    sampler: Sampler,
    base: RenderPipelineDescriptor,
    by_format: HashMap<(TextureFormat, bool), [CachedRenderPipelineId; BLUR_ENTRIES.len()]>,
}

/// The blur pipelines of a view, whether its depth is multisampled, and the picture's
/// format they were built for.
#[derive(Component)]
pub(crate) struct BlurPipelineIds([CachedRenderPipelineId; BLUR_ENTRIES.len()], bool, TextureFormat);

/// The blur shader's passes, in the order they run: the motion blur, photo mode's lens
/// blur (`crate::bokeh`), then the modifiers' blur across and down.
const BLUR_ENTRIES: [&str; 6] = ["motion_blur", "bokeh_prefilter", "bokeh", "bokeh_fill", "blur_across", "blur_down"];

fn init_blur(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>) {
    let screen = || (texture_2d(TextureSampleType::Float { filterable: true }), sampler(SamplerBindingType::Filtering), uniform_buffer::<ImageSpaceGrade>(true));
    let (a, b, c) = screen();
    let single = BindGroupLayoutDescriptor::new("imagespace_blur_layout", &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (a, b, c, texture_depth_2d())));
    let (a, b, c) = screen();
    let multi = BindGroupLayoutDescriptor::new("imagespace_blur_layout_ms", &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (a, b, c, texture_depth_2d_multisampled())));
    // Filtered: each sample of the blur reads two pixels at once.
    let sampler = device.create_sampler(&SamplerDescriptor { mag_filter: FilterMode::Linear, min_filter: FilterMode::Linear, ..default() });
    let base = RenderPipelineDescriptor {
        label: Some("imagespace_blur_pipeline".into()),
        layout: vec![single.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState { shader: assets.load("embedded://nvforever/imagespace_blur.wesl"), ..default() }),
        ..default()
    };
    commands.insert_resource(BlurPipelines { layouts: [single, multi], sampler, base, by_format: HashMap::default() });
}

fn prepare_blur(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<BlurPipelines>,
    views: Query<(Entity, &ExtractedView, &Msaa), With<ImageSpaceGrade>>,
) {
    for (entity, view, msaa) in &views {
        let format = view.target_format;
        let multi = msaa.samples() > 1;
        let ids = match pipelines.by_format.get(&(format, multi)) {
            Some(ids) => *ids,
            None => {
                let ids = BLUR_ENTRIES.map(|entry| {
                    let mut d = pipelines.base.clone();
                    d.layout = vec![pipelines.layouts[multi as usize].clone()];
                    if let Some(f) = d.fragment.as_mut() {
                        f.entry_point = Some(entry.into());
                        f.targets = vec![Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL })];
                        if multi {
                            f.shader_defs.push("MULTISAMPLED".into());
                        }
                    }
                    cache.queue_render_pipeline(d)
                });
                pipelines.by_format.insert((format, multi), ids);
                ids
            }
        };
        commands.entity(entity).insert(BlurPipelineIds(ids, multi, format));
    }
}

/// Smear the picture by its motion while a motion blur is on, draw photo mode's lens blur
/// while its Blur is up, then blur it across and down while a modifier blurs or a depth of
/// field is on (a view without a depth prepass is not blurred).
pub(crate) fn blur_pass(
    view: ViewQuery<(&ViewTarget, &DynamicUniformIndex<ImageSpaceGrade>, &ImageSpaceGrade, &BlurPipelineIds, &ViewPrepassTextures)>,
    pipelines: Res<BlurPipelines>,
    cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<ImageSpaceGrade>>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, index, grade, ids, prepass) = view.into_inner();
    let (smear, bokeh, blur) = (grade.motion.x > 0.0, grade.bokeh.x > 0.0, grade.blur > 0.0 || grade.dof.x != 0.0);
    if !(smear || bokeh || blur) || crate::ssao::depth_multisampled(prepass) != Some(ids.1) || !crate::ssao::format_matches(target, ids.2) {
        return;
    }
    let Some(depth) = prepass.depth_only_view() else { return };
    let wanted = [smear, bokeh, bokeh, bokeh, blur, blur];
    let mut passes = Vec::with_capacity(wanted.len());
    for (id, on) in ids.0.iter().zip(wanted) {
        if on {
            let Some(p) = cache.get_render_pipeline(*id) else { return };
            passes.push(p);
        }
    }
    let Some(binding) = uniforms.uniforms().binding() else { return };
    let layout = cache.get_bind_group_layout(&pipelines.layouts[ids.1 as usize]);
    for pipeline in passes {
        let post = target.post_process_write();
        let bind_group = device.create_bind_group("imagespace_blur_bind_group", &layout, &BindGroupEntries::sequential((post.source, &pipelines.sampler, binding.clone(), depth)));
        let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
            label: Some("imagespace_blur_pass"),
            color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, depth_slice: None, resolve_target: None, ops: Operations::default() })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[index.index()]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::{black_over, bokeh_text, dof_text, effective_grade_base, engine_dof, motion_blur, motion_frames, tile_menu_blur, tile_menu_blur_text, DofSettings, Hdr, ImageSpaceGrade, LensGrade, TileMenuBlur, MOTION_EXPOSURE, PIPBOY_BLUR, TALK_DOF_DISTANCE, TALK_DOF_RANGE, TALK_DOF_STRENGTH, WAIT_MENU_BLUR};
    use bevy::prelude::*;

    /// What `update_grade` writes this frame: the screen's own blur and what stays sharp.
    /// (`update_grade` itself needs a `SkyState`, which only `WorldData` builds; the smoke
    /// `tiles-tutorial` has the real one under this.)
    #[derive(Resource, Default)]
    struct Own(f32, f32);

    fn write_own(own: Res<Own>, mut grades: Query<&mut ImageSpaceGrade, Without<LensGrade>>) {
        for mut grade in &mut grades {
            *grade = ImageSpaceGrade { blur: own.0, sharp: own.1, ..ImageSpaceGrade::default() };
        }
    }

    /// The main camera's grade and a scope lens's, as `update_grade` and the blur run in a frame.
    fn grades_app() -> (App, Entity, Entity) {
        let mut app = App::new();
        app.init_resource::<Own>().init_resource::<TileMenuBlur>().add_systems(Update, write_own).add_systems(PostUpdate, tile_menu_blur);
        let main = app.world_mut().spawn(ImageSpaceGrade::default()).id();
        let lens = app.world_mut().spawn((ImageSpaceGrade::default(), LensGrade)).id();
        (app, main, lens)
    }

    fn frame(app: &mut App, entity: Entity, own: (f32, f32), tutorial: bool) -> (f32, f32) {
        app.insert_resource(Own(own.0, own.1));
        app.world_mut().resource_mut::<TileMenuBlur>().tutorial = tutorial;
        app.update();
        let g = app.world().get::<ImageSpaceGrade>(entity).unwrap();
        (g.blur, g.sharp)
    }

    #[test]
    fn a_tile_menus_blur_is_the_strongest_and_leaves_the_others_when_it_goes() {
        for (case, own, want) in [
            ("nothing else", 0.0, 3.0),
            ("the wait menu", WAIT_MENU_BLUR, 3.0),
            ("the Pip-Boy, coming up", PIPBOY_BLUR * crate::player_body::pipboy_ease(0.05), 3.0),
            ("the Pip-Boy, up", PIPBOY_BLUR, 3.5),
            ("death, at once", crate::death_cam::blur_at(0.0), 3.0),
            ("death, at its strongest", crate::death_cam::blur_at(crate::death_cam::BLUR_IN), 4.0),
        ] {
            let (mut app, main, _) = grades_app();
            assert_eq!(frame(&mut app, main, (own, 0.0), false).0, own, "{case}: before the menu");
            assert_eq!(frame(&mut app, main, (own, 0.0), true).0, want, "{case}: the first tiled frame");
            assert_eq!(frame(&mut app, main, (own, 0.0), true).0, want, "{case}: and again, not summed");
            assert_eq!(frame(&mut app, main, (own, 0.0), false).0, own, "{case}: closed, at once");
            assert_eq!(frame(&mut app, main, (own, 0.0), true).0, want, "{case}: reopened");
        }
    }

    /// The Pip-Boy's near depth stays sharp under the menu's blur, and a scope's lens keeps its
    /// own grade.
    #[test]
    fn a_tile_menus_blur_keeps_the_pipboys_sharp_depth_and_leaves_a_scope_lens() {
        let (mut app, main, lens) = grades_app();
        assert_eq!(frame(&mut app, main, (PIPBOY_BLUR, 0.25), true), (3.5, 0.25));
        assert_eq!(frame(&mut app, main, (0.0, 0.25), true), (3.0, 0.25));
        assert_eq!(app.world().get::<ImageSpaceGrade>(lens).unwrap().blur, 0.0);
    }

    /// With nothing drawn from tiles, or no tile drawing at all (the resource absent), the
    /// system writes no grade: `Changed` does not fire on them.
    #[test]
    fn no_tile_menu_writes_no_grade() {
        #[derive(Resource, Default)]
        struct Changes(usize);
        fn count(grades: Query<(), Changed<ImageSpaceGrade>>, mut changes: ResMut<Changes>) {
            changes.0 += grades.iter().count();
        }
        for present in [true, false] {
            let mut app = App::new();
            app.init_resource::<Changes>().add_systems(PostUpdate, tile_menu_blur).add_systems(Last, count);
            if present {
                app.init_resource::<TileMenuBlur>();
            }
            app.world_mut().spawn(ImageSpaceGrade { blur: 0.5, ..ImageSpaceGrade::default() });
            app.update();
            app.world_mut().resource_mut::<Changes>().0 = 0;
            app.update();
            app.update();
            assert_eq!(app.world().resource::<Changes>().0, 0, "resource present: {present}");
            if present {
                app.world_mut().resource_mut::<TileMenuBlur>().tutorial = true;
                app.update();
                assert_eq!(app.world().resource::<Changes>().0, 1, "drawn from tiles it does write");
            }
        }
    }

    #[test]
    fn the_tile_menu_blur_lines() {
        let tutorial = TileMenuBlur { tutorial: true, ..Default::default() };
        let container = TileMenuBlur { container: true, ..Default::default() };
        assert_eq!(tile_menu_blur_text(TileMenuBlur::default()), "off");
        assert_eq!(tile_menu_blur_text(tutorial), "TutorialMenu radius 3");
        assert_eq!(tile_menu_blur_text(container), "ContainerMenu radius 3");
        assert_eq!(tile_menu_blur_text(TileMenuBlur { barter: true, ..Default::default() }), "BarterMenu radius 3");
        assert_eq!(tile_menu_blur_text(TileMenuBlur { recipe: true, ..Default::default() }), "RecipeMenu radius 3");
        assert_eq!(tile_menu_blur_text(TileMenuBlur { repair_services: true, ..Default::default() }), "RepairServicesMenu radius 3");
        assert_eq!(tile_menu_blur_text(TileMenuBlur { tutorial: true, container: true, ..Default::default() }), "TutorialMenu, ContainerMenu radius 3");
        assert_eq!(tile_menu_blur_text(TileMenuBlur { traits: true, ..Default::default() }), "TraitMenu radius 3");
        assert_eq!(tile_menu_blur_text(TileMenuBlur { tags: true, ..Default::default() }), "CharGenMenu radius 3");
    }

    #[test]
    fn the_engine_focuses_vats_on_the_mark() {
        let s = DofSettings { vats_range: 500.0, vats_strength: 0.6, sights_distance: 275.0, sights_range: 650.0, sights_strength: 0.0, sights_motion_blur: 0.0 };
        // No mark, or the look gone: none.
        assert!(engine_dof(&s, 1.0, None, 0.0, None).is_none());
        assert!(engine_dof(&s, 0.0, Some(800.0), 0.0, None).is_none());
        let (d, f, name) = engine_dof(&s, 1.0, Some(812.0), 0.0, None).unwrap();
        assert_eq!((d.strength, d.range, f, name), (0.6, 500.0, 812.0, "VATS"));
        assert_eq!(dof_text(&d, f, name), "strength 0.6, distance 0, range 500, focus 800, back (VATS)");
        // The shipped iron sights cap is 0: no depth of field down the sights.
        assert!(engine_dof(&s, 0.0, None, 1.0, None).is_none());
        let raised = DofSettings { sights_strength: 0.5, ..s };
        let (d, _, name) = engine_dof(&raised, 0.0, None, 1.0, None).unwrap();
        assert_eq!((d.strength, d.distance, d.range, name), (0.5, 275.0, 650.0, "iron sights"));
        assert_eq!(dof_text(&fnv_world::image_modifier::Dof::default(), -1.0, ""), "off");
    }

    #[test]
    fn a_conversation_focuses_on_the_face_faded_with_the_zoom() {
        let s = DofSettings { vats_range: 500.0, vats_strength: 0.6, sights_distance: 275.0, sights_range: 650.0, sights_strength: 0.0, sights_motion_blur: 0.0 };
        // No zoom, none.
        assert!(engine_dof(&s, 0.0, None, 0.0, Some((0.0, 90.0))).is_none());
        let (d, f, name) = engine_dof(&s, 0.0, None, 0.0, Some((1.0, 90.0))).unwrap();
        assert_eq!((d.strength, d.distance, d.range, f, name), (TALK_DOF_STRENGTH, TALK_DOF_DISTANCE, TALK_DOF_RANGE, 90.0, "dialogue"));
        // Only what is behind the speaker.
        assert_eq!(d.flags, fnv_world::image_modifier::DOF_BACK);
        assert!(d.use_target);
        assert_eq!(dof_text(&d, f, name), "strength 1.0, distance 32, range 512, focus 100, back (dialogue)");
        let (half, _, _) = engine_dof(&s, 0.0, None, 0.0, Some((0.5, 90.0))).unwrap();
        assert_eq!(half.strength, TALK_DOF_STRENGTH * 0.5);
        // The stronger of it and VATS.
        assert_eq!(engine_dof(&s, 1.0, Some(800.0), 0.0, Some((0.5, 90.0))).unwrap().2, "VATS");
    }

    #[test]
    fn motion_blur_from_the_modifiers_or_the_sights() {
        let s = DofSettings { vats_range: 500.0, vats_strength: 0.6, sights_distance: 275.0, sights_range: 650.0, sights_strength: 0.0, sights_motion_blur: 0.0 };
        assert_eq!(motion_blur(Some(&s), 0.0, "", 1.0), (0.0, "off".to_string()));
        assert_eq!(motion_blur(Some(&s), 0.15, "VATScamNoDOF", 1.0), (0.15, "strength 0.15 (VATScamNoDOF)".to_string()));
        // The shipped fIronSightsMotionBlur is 0; a plugin raising it blurs down the sights.
        let raised = DofSettings { sights_motion_blur: 0.4, ..s };
        assert_eq!(motion_blur(Some(&raised), 0.15, "VATScamNoDOF", 0.5).1, "strength 0.20 (iron sights)");
        assert_eq!(motion_blur(None, 0.07, "MS10DrinkSapISFX", 1.0).1, "strength 0.05 (MS10DrinkSapISFX)");
        assert_eq!(motion_blur(None, 0.02, "MS10DrinkSapISFX", 1.0).1, "strength under 0.05 (MS10DrinkSapISFX)");
    }

    #[test]
    fn motion_blur_exposure_in_frames() {
        // Strength 1 at 60 frames a second: a thirtieth of a second, two frames.
        assert!((motion_frames(1.0, 1.0 / 60.0, 10.0, 0.05) - 2.0).abs() < 1e-5);
        // Strength 0.15 at 30 frames a second: 0.15 of a frame.
        assert!((motion_frames(0.15, 1.0 / 30.0, 10.0, 0.05) - 0.15).abs() < 1e-5);
        assert!((motion_frames(10.0, 0.1, 0.0, 0.0) - 10.0 * MOTION_EXPOSURE / 0.1).abs() < 1e-5);
        // No strength, no time, or a cut: none.
        assert_eq!(motion_frames(0.0, 0.016, 1.0, 0.0), 0.0);
        assert_eq!(motion_frames(1.0, 0.0, 1.0, 0.0), 0.0);
        assert_eq!(motion_frames(1.0, 0.016, 1000.0, 0.0), 0.0);
        assert_eq!(motion_frames(1.0, 0.016, 0.0, 1.0), 0.0);
    }

    #[test]
    fn photo_modes_lens_blur_line() {
        assert_eq!(bokeh_text(0.35, 1996.0), "lens blur (bokeh), aperture 0.35, focus 2000, discs up to 0.6% of the screen height (photo mode)");
    }

    #[test]
    fn black_lays_over_a_fade() {
        // Nothing on the screen: black to the amount asked.
        assert_eq!(black_over([0.0; 4], 0.0), [0.0; 4]);
        assert_eq!(black_over([0.0; 4], 0.25), [0.0, 0.0, 0.0, 0.25]);
        // Fully black whatever was there.
        assert_eq!(black_over([1.0, 1.0, 1.0, 0.5], 1.0), [0.0, 0.0, 0.0, 1.0]);
        // Half black over a half white fade: three quarters covered, a third of it white.
        let [r, _, _, a] = black_over([1.0, 1.0, 1.0, 0.5], 0.5);
        assert!((a - 0.75).abs() < 1e-6 && (r - 1.0 / 3.0).abs() < 1e-6, "{r} {a}");
    }

    #[test]
    fn effective_grade_base_neutralizes_exterior_wash_but_keeps_hdr_and_interiors() {
        let mut hdr = Hdr::default();
        hdr.v[fnv_world::imagespace::HDR_TARGET_LUM] = 2.4;
        let base = fnv_world::imagespace::Cinematic {
            saturation: 0.72,
            contrast: 0.85,
            contrast_avg_lum: 0.48,
            brightness: 1.12,
            tint: [0.85, 0.62, 0.31],
            tint_amount: 0.35,
            hdr,
        };
        let ext = effective_grade_base(base, true, false);
        assert_eq!(ext.saturation, 1.0);
        assert_eq!(ext.contrast, 1.0);
        assert_eq!(ext.brightness, 1.0);
        assert_eq!(ext.tint_amount, 0.0);
        assert_eq!(ext.hdr.v[fnv_world::imagespace::HDR_TARGET_LUM], 2.4);

        let intr = effective_grade_base(base, true, true);
        assert_eq!(intr.tint_amount, 0.35);
        assert_eq!(intr.saturation, 0.72);

        let vanilla = effective_grade_base(base, false, false);
        assert_eq!(vanilla.tint_amount, 0.35);
        assert_eq!(vanilla.brightness, 1.12);
    }
}

#[cfg(test)]
#[path = "imagespace_live_trait_tests.rs"]
mod live_trait_tests;
