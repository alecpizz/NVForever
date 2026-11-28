//! The Pip-Boy on the first-person arms (docs/spec-notes/pipboy.md §1): the model's
//! screen shows the menus' image while the Pip-Boy is up (`nvf_play::pipboy`), the
//! button of the page shown is lit, and the window's pointer is followed onto the screen
//! and the three buttons.
//!
//! The raise itself is the arms rig's clip (`player_body.rs`, `ClipKey::PipBoy`).
//!
//! - The parts, by shape name in `pipboy3000\pipboyarm.nif` [observed]: the screen
//!   `pipboyscreen:0`; the buttons `PipBoyButton01`, `02` and `03` (each two shapes), at
//!   the glows `StatsGlow:0`, `ItemsGlow:0` and `DataGlow:0` (the same places, a little
//!   above them): STATS, ITEMS, DATA from the elbow out.
//! - The screen's texture coordinates run from 0 to about 0.75 both ways (u −0.010 to
//!   0.753, v 0.002 to 0.762) over a 4:3 shape [observed]. The canvas fills 0.8 of the
//!   texture each way, as a 1024 × 768 canvas in the top left of a 1280 × 960 image
//!   [observed: fitted to the owner's ITEMS screenshot, where the menus are 1.07 times
//!   as wide and 1.10 times as high as a fill of 0.75 gave]; so the screen shows the
//!   canvas up to about 964 across and 731 down, and the image the menus are drawn into
//!   (`nvf_play::pipboy::TARGET`) is laid out the same way.
//! - The screen with the menus is drawn unlit, as a lit display [hypothesis], in the
//!   original's amber (`SCREEN_TINT`, fitted). The game's `fBrightenPipboy=1.3`
//!   [documented: `Fallout_default.ini` `[InterfaceFX]`] is not applied as such: what it
//!   brightens is not known, and the fit already matches the screenshot.
//! - The model's glass while the menus are on the screen: `ScreenLit:8`, a lit and glossy
//!   copy of the screen in the same place, stays hidden; `glare:0`, a sheet added 0.47
//!   units in front of it with `ScreenGlare.dds` [observed: their properties and
//!   vertices], is drawn on a copy of its material that adds in linear light
//!   ([`glare_material`]). Through the engine's display-value add it lay over the menus as
//!   a pale grey-blue haze [observed]; in the original it is a warm glow across the top
//!   and a patch on the right [observed: private job pipboy_special_device_base]. The
//!   fit's numbers are in `nvf_play::pipboy::device_screen`. The materials are copies: the
//!   shapes' own come back when the Pip-Boy is down.
//! - While the Pip-Boy is up only the page's button glows [observed: the owner's
//!   screenshots: the button of the page shown is bright, the other two dimmer]; the
//!   pointer lights none (a player's comparison with the original, #1516). Down, all
//!   three glow, as before (owner, 2026-10-10).
//! - Under a Stealth Boy (`stealth_field.rs`) the screen's and the glare's own materials
//!   are the field's see-through copies; the menus and the glare are built from the real
//!   ones instead, and the field leaves both alone while the menus are on the screen, so
//!   the menus stay readable (owner, 2026-10-23). They used to inherit the refraction and
//!   the screen was blank.
//! - The pointer is followed onto the shapes as the arm's bones pose them, from CPU
//!   copies of their meshes ([`PartMesh`], [`Posed`]): the drawn meshes are kept on the
//!   GPU only and are skinned, so Bevy's mesh ray casts found nothing and clicks on the
//!   screen and the buttons did nothing (the owner's playtest, 2026-10-11).

use crate::player_body::PlayerBody;
use crate::stealth_field::StealthField;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::window::PrimaryWindow;
use nvf_assets::nif_material::{Falloff, NifMaterial};
use nvf_play::pipboy::device_screen::{GLARE_LINEAR_SCALE, SCREEN_TINT};
use nvf_play::pipboy::transition::ScreenLook;
use nvf_play::pipboy::{AidTarget, Frame, Page, Phase, PipBoy, PipBoyAid, PipBoyInput, PipBoyScreen, Warming, CANVAS, TARGET};
use nvf_play::PlayerCamera;
use std::sync::Arc;

/// The part of the screen's texture the canvas fills [observed, module docs].
pub const UV_EXTENT: f32 = 0.8;

/// A part of the Pip-Boy model the menus use.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipBoyPart {
    Screen,
    /// The lit copy of the screen (`ScreenLit:8`), hidden while the menus are on it.
    ScreenLit,
    /// The glare sheet (`glare:0`), drawn over the menus ([`glare_material`]).
    Glare,
    Glow(Page),
    Button(Page),
    /// The rad meter's needle (`RadNeedle:0`, `:1`), the tab knob and the scroll wheel
    /// (`pipboy_controls.rs`).
    Needle,
    TabKnob,
    ScrollKnob,
}

/// The part a shape of the Pip-Boy model is, by its name.
pub fn part(name: &str) -> Option<PipBoyPart> {
    let n = name.trim().to_ascii_lowercase();
    let page = |s: &str| match s {
        "01" => Some(Page::Stats),
        "02" => Some(Page::Items),
        "03" => Some(Page::Data),
        _ => None,
    };
    match n.as_str() {
        "pipboyscreen:0" => Some(PipBoyPart::Screen),
        "screenlit:8" => Some(PipBoyPart::ScreenLit),
        "glare:0" => Some(PipBoyPart::Glare),
        "statsglow:0" => Some(PipBoyPart::Glow(Page::Stats)),
        "itemsglow:0" => Some(PipBoyPart::Glow(Page::Items)),
        "dataglow:0" => Some(PipBoyPart::Glow(Page::Data)),
        _ if n.starts_with("radneedle:") => Some(PipBoyPart::Needle),
        _ if n.starts_with("tabknob:") => Some(PipBoyPart::TabKnob),
        _ if n.starts_with("scrollknob:") => Some(PipBoyPart::ScrollKnob),
        _ => n.strip_prefix("pipboybutton").and_then(|r| page(r.get(..2)?)).map(PipBoyPart::Button),
    }
}

/// Where a spot of the screen's texture lies on the canvas.
pub fn canvas_point(uv: Vec2) -> Option<Vec2> {
    let p = uv / UV_EXTENT * CANVAS;
    (p.x >= 0.0 && p.y >= 0.0 && p.x < CANVAS.x && p.y < CANVAS.y).then_some(p)
}

pub struct PipBoyArmPlugin;

impl Plugin for PipBoyArmPlugin {
    fn build(&self, app: &mut App) {
        crate::pipboy_ready::setup(app);
        app.init_resource::<crate::pipboy_controls::ControlAngles>()
            .init_resource::<MenusWorn>()
            // The look after the frame's page change and its transition are decided, so the
            // new page never shows a frame under the last look (`upload_look`).
            .add_systems(Update, upload_look.after(nvf_play::pipboy::transition::run).after(show_screen))
            .add_systems(Update, (follow_pointer.in_set(nvf_play::pipboy::PointerSet).before(nvf_play::pipboy::flat_pointer), (show_screen, crate::pipboy_controls::turn_controls.run_if(|pip: Res<PipBoy>| pip.warming.is_none())).chain().after(follow_pointer).after(nvf_play::player::BodySet).before(nvf_play::PlaySet::Ui)))
            .add_systems(Update, measure_raise.in_set(nvf_play::pipboy::WarmSet))
            .add_systems(Update, requeue_raise.after(show_screen).before(nvf_play::PlaySet::Ui));
        if std::env::var_os("NVF_PLAY_TEST_NO_PIPBOY_WARM").is_none() {
            app.add_systems(Update, warm_up.in_set(nvf_play::pipboy::WarmSet).before(measure_raise));
        } else {
            info!("test aid NVF_PLAY_TEST_NO_PIPBOY_WARM: hidden warm-up disabled");
        }
    }
}

/// [design: this file] A render-frame floor covers extraction and the second skin variant.
const WARM_FRAMES: u32 = 4;
const START_WARM_CAP_SECS: f64 = 0.25;
/// [design: this file] Allow cold preparation, but bound a failed shader/upload or lost rig.
const FIRST_RAISE_CAP_SECS: f64 = 3.0;

#[derive(Default)]
struct WarmState {
    root: Option<Entity>,
    start_done: bool,
    teardown: bool,
    first_open_done: bool,
    raise_released: bool,
}

#[derive(SystemParam)]
struct WarmUp<'w, 's> {
    pip: ResMut<'w, PipBoy>,
    loading: ResMut<'w, nvf_play::loading::Loading>,
    gate: Res<'w, crate::pipeline_gate::PipelineGate>,
    render: Res<'w, crate::pipboy_ready::PipBoyReady>,
    body: Res<'w, PlayerBody>,
    screen: Option<Res<'w, PipBoyScreen>>,
    cams: Query<'w, 's, &'static Camera>,
    use_pipboy: Res<'w, nvf_play::pipboy::UsePipBoy>,
    sim: Option<Res<'w, nvf_play::sim::SimState>>,
    state: Local<'s, WarmState>,
}

/// [design: this file, player_body.rs] Only the renderer borrows the rig: no menu or POV change.
fn warm_up(params: WarmUp) {
    let WarmUp { mut pip, mut loading, gate, render, body, screen, cams, use_pipboy, sim, mut state } = params;
    if pip.phase != Phase::Raising {
        state.raise_released = false;
    }
    let root = body.first_person_root();
    if root != state.root {
        state.root = root;
        state.first_open_done = false;
        pip.invalidate_warm();
    }
    // [design: this file] One full body/UI pass restores bones, culling and materials;
    // camera_on has already switched off before the cover may fall.
    if state.teardown {
        let off = screen.as_ref().is_none_or(|s| cams.get(s.camera).map_or(true, |c| !c.is_active));
        if off {
            state.teardown = false;
            info!("pipboy: warm-up teardown complete (screen camera off)");
        } else if pip.phase != Phase::Down && !loading.starting() {
            // [design: this file] A new real opening owns the camera after cancellation.
            state.teardown = false;
        }
    }
    loading.pipboy_warming = state.teardown;
    if let Some(w) = pip.warming.as_mut().filter(|w| w.device && !w.drawn) {
        w.frames = gate.frames();
    }
    if let Some(w) = &pip.warming {
        let elapsed = w.since.elapsed().as_secs_f64();
        let posed = screen.as_ref().is_some_and(|s| cams.get(s.camera).is_ok_and(|c| c.is_active) && (!w.device || w.drawn));
        let own = render.status();
        // [design: #1803] The start already waits for compilation. Only get the
        // real draws submitted here; a cold in-play raise also waits for their PSOs.
        let submitted = !w.device || if w.covered { own.submitted() } else { own.complete() };
        let ready = submitted && posed && gate.frames().wrapping_sub(w.frames) >= WARM_FRAMES;
        // [design: this file] Closing a held raise cancels without playing the lowering clip.
        let cancelled = !w.covered && pip.phase != Phase::Raising;
        let capped = elapsed >= if w.covered { START_WARM_CAP_SECS } else { FIRST_RAISE_CAP_SECS };
        if ready || capped || cancelled {
            let w = pip.warming.take().expect("warming");
            let asked = gate.asked().saturating_sub(w.asked_at);
            let ms = elapsed * 1000.0;
            info!("pipboy: warm-up asked {asked} pipelines, ready after {ms:.0} ms (covered={}, capped={}, cancelled={cancelled})", w.covered, !ready && !cancelled);
            info!("pipboy: own rig pipelines {:?}; submitted={}, prepared={}, bound={}, shapes={}", own.pipelines, own.queued, own.prepared, own.bound, own.shapes);
            if !w.covered {
                state.raise_released = !cancelled;
                if capped && !ready && !cancelled {
                    warn!("pipboy: first raise cap reached after {ms:.0} ms; raising with incomplete readiness");
                }
                info!("pipboy: first raise held {ms:.0} ms for {asked} pipelines");
            }
            if !cancelled {
                pip.warmed |= w.device && own.submitted();
                state.first_open_done |= !w.covered && w.device && own.complete();
            } else {
                pip.phase = Phase::Down;
                pip.clip_time = 0.0;
            }
            if w.covered {
                pip.frame = w.saved_frame;
            }
            if let Some((page, stats_tab)) = w.saved_page {
                pip.page = page;
                pip.tabs[Page::Stats.index()] = stats_tab;
            }
            if w.covered || cancelled {
                state.teardown = true;
            }
        }
        loading.pipboy_warming = pip.warming.is_some() || state.teardown;
        return;
    }
    let covered = loading.starting() && !loading.world_held() && !state.start_done && root.is_some();
    // [design: #1803] Teardown removes the hidden rig from retained phases. Verify
    // the first real submission too, even when its pipelines were warmed at start.
    let cold = !state.raise_released && root.is_some() && pip.phase == Phase::Raising && pip.frame == Frame::Device && (!pip.warmed || !state.first_open_done);
    if state.teardown || screen.is_none() || (!covered && !cold) {
        return;
    }
    let device = use_pipboy.on && sim.as_deref().is_some_and(nvf_play::pipboy::wears_pipboy);
    let saved_frame = pip.frame;
    // [design: pipboy/items.rs::draw] ITEMS needs an open inventory. STATS can draw
    // a complete page without opening a menu; restore the player's choice afterwards.
    let saved_page = covered.then_some((pip.page, pip.tabs[Page::Stats.index()]));
    if covered {
        pip.frame = if device { Frame::Device } else { Frame::Screen };
        pip.page = Page::Stats;
        pip.tabs[Page::Stats.index()] = 0;
        state.start_done = true;
    } else {
        pip.clip_time = 0.0;
    }
    pip.warming = Some(Warming { since: std::time::Instant::now(), frames: gate.frames(), asked_at: gate.asked(), covered, device, saved_frame, saved_page, drawn: false });
    loading.pipboy_warming = covered;
    info!("pipboy: warm-up start (covered={covered}, device={device}, asked={}, frame={})", gate.asked(), gate.frames());
}

/// [design: #1803, #2862] A ready PSO is not a retained phase entry. Showing the rig
/// again must refresh the main bins with its current mesh slabs and material bindings
/// throughout the raise and once on the transition into `Phase::Up` (covering both
/// shapes that only enter the frustum near the top of a holstered/empty-hands raise
/// and warm `Frame::Screen` -> `Frame::Device` switches).
fn requeue_raise(
    pip: Res<PipBoy>,
    mut meshes: Query<&mut Mesh3d, With<crate::pipboy_ready::PipBoyShape>>,
    mut up_refreshed: Local<bool>,
) {
    let active = pip.warming.is_none() && pip.on_arm();
    let refresh = if active && pip.phase == Phase::Raising {
        *up_refreshed = false;
        true
    } else if active && pip.phase == Phase::Up {
        let first_up = !*up_refreshed;
        *up_refreshed = true;
        first_up
    } else {
        *up_refreshed = false;
        false
    };
    if refresh {
        for mut mesh in &mut meshes {
            mesh.set_changed();
        }
    }
}

/// [design: this file] Count the visible raise after the hidden hold, also with the aid off.
fn measure_raise(pip: Res<PipBoy>, gate: Res<crate::pipeline_gate::PipelineGate>, mut began: Local<Option<u32>>) {
    if pip.warming.is_some() {
        *began = None;
    } else if pip.phase == Phase::Raising && began.is_none() {
        *began = Some(gate.asked());
    } else if pip.phase == Phase::Up {
        if let Some(n) = began.take() {
            info!("pipboy: up: {} pipelines asked since the raise began", gate.asked().saturating_sub(n));
        }
    } else if pip.phase == Phase::Down {
        *began = None;
    }
}

/// The shapes' own materials while the menus are on the screen, and the copies worn then.
#[derive(Default)]
pub(crate) struct Swapped {
    screen: Option<(Entity, Handle<NifMaterial>)>,
    glare: Option<(Entity, Handle<NifMaterial>)>,
    menus: Option<Copied>,
    glass: Option<Copied>,
    /// The menus' missing place was logged for this raise.
    warned: bool,
    /// The buttons last logged as lit.
    lit: Option<String>,
    reported: bool,
}

/// A material copied from a shape's own, and what it was made from: a rig rebuilt on other
/// materials (another texture, other settings) gets a copy of its own, not the first rig's.
struct Copied {
    source: Handle<NifMaterial>,
    image: Option<Handle<Image>>,
    worn: Handle<NifMaterial>,
}

/// The copy of `source` (and `image`) in `cache`, made when it is of anything else. A copy
/// no shape wears any more goes with its last handle.
fn copy_of(cache: &mut Option<Copied>, source: &Handle<NifMaterial>, image: Option<&Handle<Image>>, materials: &mut Assets<NifMaterial>, make: impl FnOnce(&NifMaterial) -> NifMaterial) -> Handle<NifMaterial> {
    if let Some(c) = cache.as_ref().filter(|c| c.source == *source && c.image.as_ref() == image) {
        return c.worn.clone();
    }
    let copy = make(&materials.get(source).cloned().unwrap_or_default());
    let worn = materials.add(copy);
    *cache = Some(Copied { source: source.clone(), image: image.cloned(), worn: worn.clone() });
    worn
}

/// Whether the glow of `page`'s button shows: the page's alone while the Pip-Boy shows on
/// the arm, all three otherwise (module docs).
pub(crate) fn glow_lit(pip: &PipBoy, page: Page) -> bool {
    !pip.on_arm() || page == pip.page
}

/// The glare's material while the menus are on the screen, from the shape's own: its
/// texture × [`GLARE_LINEAR_SCALE`] × its vertex colour, unlit, added in linear light as the
/// terminal's glare is (`terminal_view.rs`) [design]. Not on Bevy's own additive material as
/// there: the arms' depth is squeezed in front of the world (`NifSurface::first_person`),
/// and a plain material's fragments would all fall behind the screen. Faded by nothing: the
/// sheet lies 0.47 units off the screen, and its soft intersection would thin it.
pub(crate) fn glare_material(own: &NifMaterial) -> NifMaterial {
    let mut m = own.clone();
    let [r, g, b] = GLARE_LINEAR_SCALE;
    m.base.base_color = Color::linear_rgb(r, g, b);
    m.base.emissive = LinearRgba::BLACK;
    m.base.emissive_texture = None;
    m.base.normal_map_texture = None;
    m.base.unlit = true;
    m.base.alpha_mode = AlphaMode::Add;
    m.base.fog_enabled = false;
    m.extension.add_linear = true;
    m.extension.add_whole = 0.0;
    m.extension.alpha_cut = 0.0;
    m.extension.falloff = Falloff::NONE.params;
    m.extension.soft = Vec4::ZERO;
    nvf_assets::authored::clear_for_unlit_copy(&mut m);
    m
}

/// The screen's material with the menus' image on it, from the shape's own.
pub(crate) fn menus_material(own: &NifMaterial, image: Handle<Image>) -> NifMaterial {
    let mut m = own.clone();
    let scale = (CANVAS / TARGET.as_vec2()) / UV_EXTENT;
    let [r, g, b] = SCREEN_TINT;
    m.base.base_color = Color::LinearRgba(LinearRgba::rgb(r, g, b));
    m.base.base_color_texture = Some(image);
    m.base.emissive_texture = None;
    m.base.normal_map_texture = None;
    m.base.unlit = true;
    m.base.alpha_mode = AlphaMode::Opaque;
    m.base.uv_transform = bevy::math::Affine2::from_scale(scale);
    m.extension.pipboy_screen = true;
    (m.extension.pipboy_a, m.extension.pipboy_b) = ScreenLook::default().surface();
    nvf_assets::authored::clear_for_unlit_copy(&mut m);
    m
}

/// The menus' copy of the screen's material while the model's screen wears it (set by
/// [`show_screen`]), for [`upload_look`].
#[derive(Resource, Default)]
pub(crate) struct MenusWorn(Option<Handle<NifMaterial>>);

/// Put the screen's look on the menus' copy, each frame after nvf-play's page change and its
/// transition ran (`pipboy::transition::run`, in `PlaySet::Ui`): put on in `show_screen`, which
/// runs before that set, a new page was drawn for a frame under the last look (unwashed) and
/// the end of a change came a frame late.
fn upload_look(worn: Res<MenusWorn>, look: Option<Res<ScreenLook>>, mut materials: ResMut<Assets<NifMaterial>>) {
    if let (Some(menus), Some(look)) = (worn.0.as_ref(), look) {
        put_look(menus, &look, &mut materials);
    }
}

/// The screen's picture as `look` says it is now (nvf-play `pipboy/transition.rs`: the page
/// change's roll and wash, the softening and the noise), on the menus' copy of the material,
/// written only when it changed: a material changed is sent to the GPU again with its slab.
fn put_look(menus: &Handle<NifMaterial>, look: &ScreenLook, materials: &mut Assets<NifMaterial>) -> bool {
    let (a, b) = look.surface();
    if materials.get(menus).is_none_or(|m| (m.extension.pipboy_a, m.extension.pipboy_b) == (a, b)) {
        return false;
    }
    if let Some(mut m) = materials.get_mut(menus) {
        (m.extension.pipboy_a, m.extension.pipboy_b) = (a, b);
    }
    true
}

/// Put the copies on the screen and the glare while the menus are on the model (`on`), and
/// give the shapes their own materials back otherwise, or when the rig was rebuilt. A copy
/// is made once for each source, and no shared source material is touched.
/// Returns whether an existing copy changed source [design: this file, #1803].
fn wear_materials(on: bool, announce: bool, image: &Handle<Image>, parts: &mut Query<(Entity, &PipBoyPart, &mut MeshMaterial3d<NifMaterial>)>, field: &mut StealthField, materials: &mut Assets<NifMaterial>, swapped: &mut Swapped) -> bool {
    let find = |parts: &Query<(Entity, &PipBoyPart, &mut MeshMaterial3d<NifMaterial>)>, part: PipBoyPart| parts.iter().find(|(_, p, _)| **p == part).map(|(e, _, m)| (e, m.0.clone()));
    let model_screen = find(parts, PipBoyPart::Screen);
    let model_glare = find(parts, PipBoyPart::Glare);
    for (slot, now) in [(&mut swapped.screen, model_screen.as_ref()), (&mut swapped.glare, model_glare.as_ref())] {
        if let Some((e, own)) = slot.clone() {
            if !on || now.is_none_or(|(s, _)| *s != e) {
                // Set at once, not by command: the Stealth Boy's field runs next and must
                // see the shape's own material to make it see-through again.
                if let Ok((_, _, mut m)) = parts.get_mut(e) {
                    m.0 = own;
                }
                *slot = None;
            }
        }
    }
    if !on {
        swapped.reported = false;
        return false;
    }
    let before = [&swapped.menus, &swapped.glass].map(|c| c.as_ref().map(|c| c.source.id()));
    if let (None, Some((e, mut own))) = (&swapped.screen, model_screen) {
        // Under a Stealth Boy the screen wears the field's see-through copy: take the real
        // material back, so neither the menus nor the screen afterwards refract.
        if let Some(original) = field.take_original(e, &own, materials) {
            own = original;
            if announce { info!("pipboy: the menus are on the screen drawn plain under the Stealth Boy"); }
        }
        let menus = copy_of(&mut swapped.menus, &own, Some(image), materials, |source| menus_material(source, image.clone()));
        if let Ok((_, _, mut m)) = parts.get_mut(e) {
            m.0 = menus;
        }
        swapped.screen = Some((e, own));
    }
    // The image replaced while the menus are up takes effect at once.
    if let Some((e, own)) = swapped.screen.clone() {
        let menus = copy_of(&mut swapped.menus, &own, Some(image), materials, |source| menus_material(source, image.clone()));
        if let Ok((_, _, mut m)) = parts.get_mut(e) {
            if m.0 != menus {
                m.0 = menus;
            }
        }
    }
    if let (None, Some((e, mut own))) = (&swapped.glare, model_glare) {
        if let Some(original) = field.take_original(e, &own, materials) {
            own = original;
            if announce { info!("pipboy: the glare is drawn plain under the Stealth Boy"); }
        }
        let glass = copy_of(&mut swapped.glass, &own, None, materials, glare_material);
        if let Ok((_, _, mut m)) = parts.get_mut(e) {
            m.0 = glass;
        }
        swapped.glare = Some((e, own));
    }
    if announce && !swapped.reported {
        if swapped.screen.is_some() { info!("pipboy: the menus are on the model's screen"); }
        if swapped.glare.is_some() { info!("pipboy: the glare is over the menus"); }
        swapped.reported = true;
    }
    let after = [&swapped.menus, &swapped.glass].map(|c| c.as_ref().map(|c| c.source.id()));
    before.into_iter().zip(after).any(|(before, after)| before.is_some() && before != after)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn show_screen(
    mut pip: ResMut<PipBoy>,
    screen: Option<ResMut<PipBoyScreen>>,
    body: Res<PlayerBody>,
    mut field: ResMut<StealthField>,
    mut parts: Query<(Entity, &PipBoyPart, &mut MeshMaterial3d<NifMaterial>)>,
    mut visibility: Query<&mut Visibility>,
    mut materials: ResMut<Assets<NifMaterial>>,
    mut swapped: Local<Swapped>,
    worn: Option<ResMut<MenusWorn>>,
) {
    let Some(mut screen) = screen else { return };
    let shown = pip.on_arm() && (body.first_person_drawn() || (pip.warming.is_some() && body.first_person_root().is_some()));
    let model_has_screen = parts.iter().any(|(_, p, _)| *p == PipBoyPart::Screen);
    screen.on_model = shown && model_has_screen;
    // [design: this file] Compare Copied.source after resolving any Stealth Boy copy.
    if wear_materials(screen.on_model, pip.warming.is_none(), &screen.image, &mut parts, &mut field, &mut materials, &mut swapped) {
        pip.invalidate_warm();
    }
    if let Some(mut worn) = worn {
        let now = swapped.menus.as_ref().filter(|_| swapped.screen.is_some()).map(|c| c.worn.clone());
        if worn.0 != now {
            worn.0 = now;
        }
    }
    // Said once each time the Pip-Boy comes up when the menus have nowhere to show, so a
    // player's log says why the arm came up blank or not at all (#1330).
    if !pip.on_arm() {
        swapped.warned = false;
    } else if !swapped.warned && pip.clip_time >= nvf_play::pipboy::HOLD_SECONDS {
        let why = if !body.first_person_drawn() {
            Some("the first-person arms are not drawn")
        } else if !model_has_screen {
            Some("the arm's model has no screen shape (pipboyscreen:0)")
        } else {
            None
        };
        if let Some(why) = why {
            warn!("pipboy: up, but the menus are not on the arm: {why}");
            swapped.warned = true;
        }
    }
    // The glows and the lit copy of the screen (hidden while the menus are on it).
    let mut lit: [bool; 3] = default();
    for (e, p, _) in &parts {
        let shown = match *p {
            PipBoyPart::Glow(page) => glow_lit(&pip, page),
            PipBoyPart::ScreenLit => !screen.on_model,
            _ => continue,
        };
        let want = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if let Ok(mut v) = visibility.get_mut(e) {
            if *v != want {
                *v = want;
            }
            if let (PipBoyPart::Glow(page), true) = (*p, *v != Visibility::Hidden) {
                lit[page.index()] = true;
            }
        }
    }
    // Said on each change, from the glows' visibility as set here (what is drawn from it is
    // for a shot to show). Nothing while no glow is on.
    if pip.warming.is_none() && lit.contains(&true) {
        let names = Page::ALL.into_iter().filter(|p| lit[p.index()]).map(|p| p.title()).collect::<Vec<_>>().join(", ");
        if swapped.lit.as_deref() != Some(names.as_str()) {
            info!("pipboy: buttons lit: {names}");
            swapped.lit = Some(names);
        }
    }
}

/// A Pip-Boy part's mesh as loaded (positions, texture coordinates, joints), kept on the
/// CPU: the drawn mesh lives on the GPU only, and Bevy's mesh ray casts do not skin.
#[derive(Component, Clone)]
pub struct PartMesh(pub Arc<Mesh>);

/// A shape posed in the world: its triangles' corners and texture coordinates.
pub struct Posed {
    pub points: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub triangles: Vec<[usize; 3]>,
}

impl Posed {
    /// Pose `mesh`: skinned by `joints` (each joint's world matrix times its inverse bind
    /// pose, as Bevy's skinning does), else placed by `world`.
    pub fn new(mesh: &Mesh, joints: Option<&[Mat4]>, world: Mat4) -> Option<Posed> {
        let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
        let uvs: Vec<Vec2> = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(uv)) => uv.iter().map(|u| Vec2::from_array(*u)).collect(),
            _ => return None,
        };
        let skin = joints.and_then(|j| match (mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX), mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT)) {
            (Some(VertexAttributeValues::Uint16x4(ji)), Some(VertexAttributeValues::Float32x4(jw))) => Some((j, ji, jw)),
            _ => None,
        });
        let points = pos
            .iter()
            .enumerate()
            .map(|(v, p)| {
                let p = Vec3::from_array(*p);
                match skin {
                    Some((j, ji, jw)) => {
                        let mut m = Mat4::ZERO;
                        for k in 0..4 {
                            if let Some(joint) = j.get(ji[v][k] as usize) {
                                m += *joint * jw[v][k];
                            }
                        }
                        m.transform_point3(p)
                    }
                    None => world.transform_point3(p),
                }
            })
            .collect();
        let triangles = match mesh.indices() {
            Some(i) => {
                let i: Vec<usize> = i.iter().collect();
                i.as_chunks::<3>().0.to_vec()
            }
            None => (0..pos.len() / 3).map(|t| [3 * t, 3 * t + 1, 3 * t + 2]).collect(),
        };
        Some(Posed { points, uvs, triangles })
    }

    /// The nearest triangle the ray meets, both faces: the distance along the ray and the
    /// texture coordinates there.
    pub fn hit(&self, origin: Vec3, dir: Vec3) -> Option<(f32, Vec2)> {
        let mut best: Option<(f32, Vec2)> = None;
        for &[a, b, c] in &self.triangles {
            let (pa, pb, pc) = (self.points[a], self.points[b], self.points[c]);
            let (e1, e2) = (pb - pa, pc - pa);
            let h = dir.cross(e2);
            let det = e1.dot(h);
            if det.abs() < 1e-9 {
                continue;
            }
            let s = origin - pa;
            let u = s.dot(h) / det;
            let q = s.cross(e1);
            let v = dir.dot(q) / det;
            let t = e2.dot(q) / det;
            if u < 0.0 || v < 0.0 || u + v > 1.0 || t <= 0.0 || best.is_some_and(|(d, _)| d <= t) {
                continue;
            }
            best = Some((t, self.uvs[a] * (1.0 - u - v) + self.uvs[b] * u + self.uvs[c] * v));
        }
        best
    }

    /// The point of the shape at texture coordinates `uv`, if some triangle covers them.
    pub fn at_uv(&self, uv: Vec2) -> Option<Vec3> {
        self.triangles.iter().find_map(|&[a, b, c]| {
            let (ta, tb, tc) = (self.uvs[a], self.uvs[b], self.uvs[c]);
            let (e1, e2, p) = (tb - ta, tc - ta, uv - ta);
            let det = e1.perp_dot(e2);
            if det.abs() < 1e-12 {
                return None;
            }
            let u = p.perp_dot(e2) / det;
            let v = e1.perp_dot(p) / det;
            (u >= 0.0 && v >= 0.0 && u + v <= 1.0).then(|| self.points[a] * (1.0 - u - v) + self.points[b] * u + self.points[c] * v)
        })
    }

    /// The middle of the shape's corners.
    pub fn middle(&self) -> Option<Vec3> {
        (!self.points.is_empty()).then(|| self.points.iter().sum::<Vec3>() / self.points.len() as f32)
    }
}

/// Follow the window's pointer onto the model: where it lies on the screen goes to the
/// menus (`PipBoyScreen::pointer`); a click on a button shows its page. The pointer on a
/// button does nothing else: the button's light follows the page shown. The shapes are
/// posed as the arm's bones hold them this frame. The test aid's target (`PipBoyAid`)
/// stands in for the window's pointer. A window pointer asleep under the controller
/// (`nvf_play::pad::pointer`, #2310) is on nothing.
pub(crate) fn pointer_pose(mesh: &Mesh, joints: Option<&[Mat4]>, world: Mat4) -> Option<Posed> {
    if mesh.primitive_topology()!=bevy::render::render_resource::PrimitiveTopology::TriangleList || !world.is_finite() || !world.row(3).abs_diff_eq(Vec4::W,1e-6) {return None;}
    let Some(VertexAttributeValues::Float32x3(positions))=mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {return None};
    let Some(VertexAttributeValues::Float32x2(uvs))=mesh.attribute(Mesh::ATTRIBUTE_UV_0) else {return None};
    if positions.is_empty() || positions.len()!=uvs.len() || !positions.iter().flatten().all(|v|v.is_finite()) || !uvs.iter().flatten().all(|v|v.is_finite()) {return None;}
    if mesh.indices().is_some_and(|indices| !indices.len().is_multiple_of(3) || indices.iter().any(|i|i>=positions.len())) {return None;}
    if let Some(joints)=joints {
        let Some(VertexAttributeValues::Uint16x4(indices))=mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX) else {return None};
        let Some(VertexAttributeValues::Float32x4(weights))=mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT) else {return None};
        if indices.len()!=positions.len() || weights.len()!=positions.len() || !joints.iter().all(|m|m.is_finite()) {return None;}
        if indices.iter().zip(weights).any(|(index,weight)|index.iter().zip(weight).any(|(i,w)|!w.is_finite() || (*w!=0.0 && usize::from(*i)>=joints.len()))) {return None;}
        for (index,weight) in indices.iter().zip(weights) {
            let mut matrix=Mat4::ZERO;
            for k in 0..4 {if let Some(joint)=joints.get(usize::from(index[k])) {matrix+=*joint*weight[k];}}
            if !matrix.is_finite() || !matrix.row(3).abs_diff_eq(Vec4::W,1e-6) {return None;}
        }
    }
    let shape=Posed::new(mesh,joints,world)?;
    shape.points.iter().all(|p|p.is_finite()).then_some(shape)
}

fn touch_canvas(cam:&Camera,at:&GlobalTransform,posed:&[(PipBoyPart,Posed,bool)],position:Vec2)->Option<Vec2> {
    let ray=cam.viewport_to_world(at,position).ok()?;
    let hit=posed.iter().filter(|(_,_,visible)|*visible).filter_map(|(part,mesh,_)|
        mesh.hit(ray.origin,Vec3::from(ray.direction)).map(|(distance,uv)|(distance,*part,uv)))
        .min_by(|a,b|a.0.total_cmp(&b.0));
    match hit {Some((_,PipBoyPart::Screen,uv))=>canvas_point(uv),_=>None}
}
#[cfg(any(target_os="android",test))]
type MapPublication<'w,'s> = (
    Option<ResMut<'w,nvf_play::pipboy::data::touch_map::TouchMap>>,
    Option<Res<'w,nvf_play::touch_safe_area::SafeInsets>>,Option<Res<'w,UiScale>>,
    Option<Res<'w,nvf_play::pipboy::touch_presentation::PipBoySize>>,
    Query<'w,'s,Entity,With<PrimaryWindow>>,
    Query<'w,'s,(Entity,&'static PipBoyPart,Option<&'static Visibility>,Option<&'static InheritedVisibility>)>,
);
type PointerParts<'w, 's> = Query<'w, 's, (Entity, &'static PipBoyPart, &'static PartMesh, &'static GlobalTransform, Option<&'static SkinnedMesh>, Option<&'static Visibility>, Option<&'static InheritedVisibility>)>;
type TouchPointerInput<'w> = (Option<Res<'w,nvf_play::pad::pointer::PadPointer>>, Option<ResMut<'w,nvf_play::pipboy::touch_items::TouchItems>>, Option<Res<'w,nvf_play::ui_pointer::TouchUiCapture>>);

#[allow(clippy::too_many_arguments)]
fn follow_pointer(
    pip: Res<PipBoy>,
    screen: Option<ResMut<PipBoyScreen>>,
    cams: Query<(Entity, &Camera, &GlobalTransform, &bevy::camera::RenderTarget), With<PlayerCamera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    parts: PointerParts<'_, '_>,
    globals: Query<&GlobalTransform>,
    binds: Res<Assets<SkinnedMeshInverseBindposes>>,
    mouse: nvf_play::pad::ActionButtons<MouseButton>,
    mut aid: ResMut<PipBoyAid>,
    mut aid_over: Local<Option<Page>>,
    mut out: MessageWriter<PipBoyInput>,
    (pad_pointer, mut touch, overlay): TouchPointerInput<'_>,
    #[cfg(any(target_os = "android", test))] mut geometry: Option<ResMut<crate::touch::pipboy_model::Geometry>>,
    #[cfg(any(target_os="android",test))] (mut map,map_safe,map_ui,map_size,map_windows,map_parts):MapPublication<'_,'_>,
) {
    #[cfg(any(target_os = "android", test))]
    if let Some(geometry) = geometry.as_deref_mut() { *geometry = default(); }
    #[cfg(any(target_os="android",test))]
    let map_active=pip.phase==nvf_play::pipboy::Phase::Up && pip.page==Page::Data && pip.tab()<=1
        && screen.as_ref().is_some_and(|s|s.on_model && pip.frame==Frame::Device);
    #[cfg(any(target_os="android",test))]
    if screen.is_none() || (screen.as_ref().is_some_and(|s|nvf_play::pipboy::pointer_source(pip.frame,s.on_model)==nvf_play::pipboy::PointerSource::Device) && !map_active) {
        if let Some(map)=map.as_deref_mut(){map.publish(None);}
    }

    let mouse = mouse.snapshot();
    let Some(mut screen) = screen else { return };
    let was_over = aid_over.take();
    // The flat image has the pointer otherwise: never both (`pipboy::pointer_source`).
    if nvf_play::pipboy::pointer_source(pip.frame, screen.on_model) != nvf_play::pipboy::PointerSource::Device {
        return;
    }
    screen.pointer = None;
    if !pip.open() {
        return;
    }
    let (Ok((camera_entity, cam, at, _map_target)), Ok(window)) = (cams.single(), windows.single()) else {
        #[cfg(any(target_os="android",test))]
        if let Some(map)=map.as_deref_mut(){map.publish(None);}
        return;
    };
    #[cfg(any(target_os = "android", test))]
    let mut admitted = Vec::new();
    #[cfg(any(target_os = "android", test))]
    let model_presented = map_windows.single().ok().is_some_and(|primary|
        crate::touch::pipboy_model::Geometry::camera_presented(cam, _map_target, primary));
    let posed: Vec<(PipBoyPart, Posed, bool)> = parts
        .iter()
        .filter(|(_, p, ..)| matches!(p, PipBoyPart::Screen | PipBoyPart::Button(_)) || (cfg!(any(target_os = "android", test)) && matches!(p, PipBoyPart::ScrollKnob)))
        .filter_map(|(entity, p, mesh, gt, skin, visible, inherited)| {
            #[cfg(any(target_os = "android", test))]
            let shape = {
                // Touch projection requires a complete pose for every skinned part.
                let joints = if let Some(s) = skin {
                    let inv = binds.get(&s.inverse_bindposes)?;
                    if inv.len() != s.joints.len() || inv.is_empty() { return None; }
                    let pose: Vec<Mat4> = s.joints.iter().zip(inv.iter()).map(|(j, ib)| globals.get(*j).ok().map(|g| g.to_matrix() * *ib)).collect::<Option<_>>()?;
                    if !pose.iter().all(|m| m.is_finite()) { return None; }
                    Some(pose)
                } else { None };
                pointer_pose(&mesh.0, joints.as_deref(), gt.to_matrix())?
            };
            #[cfg(not(any(target_os = "android", test)))]
            let shape = {
                let joints: Option<Vec<Mat4>> = skin.and_then(|s| {
                    let inv = binds.get(&s.inverse_bindposes)?;
                    s.joints.iter().zip(inv.iter()).map(|(j, ib)| globals.get(*j).ok().map(|g| g.to_matrix() * *ib)).collect()
                });
                Posed::new(&mesh.0, joints.as_deref(), gt.to_matrix())?
            };
            let touch_visible = cam.is_active && visible.is_some_and(|v| *v != Visibility::Hidden) && inherited.is_some_and(|v| v.get());
            #[cfg(any(target_os = "android", test))]
            if touch_visible && model_presented {
                if let Some(bounds) = crate::touch::pipboy_model::Geometry::projected_bounds(cam, at, &shape.points, window.resolution.size()) {
                    let triangles = if *p == PipBoyPart::Screen {
                        shape.triangles.iter().filter_map(|[a,b,c]| Some([
                            cam.world_to_viewport(at, *shape.points.get(*a)?).ok()?,
                            cam.world_to_viewport(at, *shape.points.get(*b)?).ok()?,
                            cam.world_to_viewport(at, *shape.points.get(*c)?).ok()?,
                        ])).collect()
                    } else { Vec::new() };
                    admitted.push((entity, *p, bounds, triangles));
                }
            }
            #[cfg(not(any(target_os = "android", test)))]
            let _ = (entity, visible, inherited, camera_entity);
            Some((*p, shape, touch_visible))
        })
        .collect();
    #[cfg(any(target_os = "android", test))]
    if let Some(geometry) = geometry.as_deref_mut() {
        use crate::touch::pipboy_model::ButtonTarget;
        geometry.size = window.resolution.size();
        geometry.image = Some(screen.image.id());
        geometry.page = Some(pip.page);
        geometry.tab = pip.tab();
        geometry.camera = Some(camera_entity);
        for (entity, part, bounds, triangles) in admitted {
            geometry.entities.push(entity);
            match part {
                PipBoyPart::Button(page) => {
                    if let Some(old) = geometry.buttons.iter_mut().find(|b| b.page == page) {
                        old.bounds = old.bounds.union(bounds);
                    } else { geometry.buttons.push(ButtonTarget { page, bounds }); }
                }
                PipBoyPart::ScrollKnob => geometry.knob = Some(geometry.knob.map_or(bounds, |old| old.union(bounds))),
                PipBoyPart::Screen => { geometry.screen = Some(geometry.screen.map_or(bounds, |old| old.union(bounds))); geometry.screen_triangles.extend(triangles); }
                _ => {}
            }
        }
        geometry.entities.sort_unstable();
        geometry.entities.dedup();
    }
    // Where the pointer is: the test aid's target, else the window's pointer.
    let target = aid.target.and_then(|t| {
        let world = match t {
            AidTarget::Button(page) => posed.iter().find(|(p, ..)| *p == PipBoyPart::Button(page)).and_then(|(_, s, _)| s.middle()),
            AidTarget::Canvas(c) => posed.iter().find(|(p, ..)| *p == PipBoyPart::Screen).and_then(|(_, s, _)| s.at_uv(c / CANVAS * UV_EXTENT)),
        }?;
        cam.world_to_viewport(at, world).ok()
    });
    if aid.target.is_some() && target.is_none() {
        warn!("test aid: pipboy target {:?} is not in view", aid.target);
    }
    if let Some(touch) = touch.as_deref_mut() {
        touch.project(|position|touch_canvas(cam,at,&posed,position));
    }
    let sleeping = nvf_play::pad::pointer::asleep(pad_pointer.as_deref());
    let cursor = if overlay.as_ref().is_some_and(|o| o.pointer_suppressed()) { None }
        else if aid.target.is_some() { target } else { window.cursor_position().filter(|_| !sleeping) };
    if let Some(ray) = cursor.and_then(|c| cam.viewport_to_world(at, c).ok()) {
    let (origin, dir) = (ray.origin, Vec3::from(ray.direction));
    let hit = posed.iter().filter_map(|(p, s, _)| s.hit(origin, dir).map(|(t, uv)| (t, *p, uv))).min_by(|a, b| a.0.total_cmp(&b.0));
    let clicked = mouse.just_pressed(MouseButton::Left) || (aid.click == 3 && matches!(aid.target, Some(AidTarget::Button(_))));
    match hit {
        Some((_, PipBoyPart::Screen, uv)) => screen.pointer = canvas_point(uv),
        Some((_, PipBoyPart::Button(page), _)) => {
            if aid.target.is_some() {
                // What the hover-only smoke step needs said: the pointer did reach it.
                if was_over != Some(page) {
                    info!("test aid: the pointer on the {} button", page.title());
                }
                *aid_over = Some(page);
            }
            if clicked {
                info!("pipboy: the {} button pressed", page.title());
                out.write(PipBoyInput::Page(page));
                if aid.click == 3 {
                    aid.click = 0;
                }
            }
        }
        _ => {}
    }
    }
    #[cfg(any(target_os="android",test))]
    if let Some(map)=map.as_deref_mut() {
        use nvf_play::pipboy::data::touch_map::{Presentation,Projection,ProjectionStamp};
        let projection=(|| {
            if pip.phase!=nvf_play::pipboy::Phase::Up || pip.page!=Page::Data || pip.tab()>1
                || !posed.iter().any(|(part,_,visible)|*part==PipBoyPart::Screen && *visible) {return None;}
            let live=geometry.as_deref()?;
            if live.screen.is_none() || map_parts.iter().any(|(entity,part,visible,inherited)|
                matches!(part,PipBoyPart::Screen|PipBoyPart::Button(_)|PipBoyPart::ScrollKnob)
                && visible.is_some_and(|v|*v!=Visibility::Hidden) && inherited.is_some_and(|v|v.get())
                && !live.entities.contains(&entity)) {return None;}
            let stamp=ProjectionStamp::new(map_windows.single().ok()?,window,&screen,true,Presentation {
                safe:map_safe.as_deref().copied().unwrap_or_default(),menu:nvf_play::interface::unit_scale(),
                ui:map_ui.as_deref().map_or(1.0,|s|s.0),size:map_size.as_deref().map_or(1.0,nvf_play::pipboy::touch_presentation::PipBoySize::ratio),
            }).with_camera(camera_entity,cam,_map_target)?;
            let camera=cam.clone();let at=*at;
            // Existing CPU pose ownership moves once; no vertex/triangle clone
            // and no retained projection history. Physical pointer ran above.
            Projection::new(stamp,move|point|touch_canvas(&camera,&at,&posed,point))
        })();
        map.publish(projection);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    /// Real CPU projection/show-screen ordering, excluding render warm-up proof.
    pub(crate) fn install_coupled_clip_projection(app: &mut App) {
        app.add_systems(Update, follow_pointer.in_set(nvf_play::pipboy::PointerSet)
            .before(nvf_play::pipboy::flat_pointer))
            .add_systems(Update, show_screen.after(follow_pointer)
                .after(nvf_play::player::BodySet).before(nvf_play::PlaySet::Ui));
    }
    use super::*;

    fn capped_warm_app(covered: bool, device: bool) -> App {
        let mut app = App::new();
        app.init_resource::<PlayerBody>()
            .init_resource::<nvf_play::loading::Loading>()
            .init_resource::<crate::pipeline_gate::PipelineGate>()
            .init_resource::<crate::pipboy_ready::PipBoyReady>()
            .init_resource::<nvf_play::pipboy::UsePipBoy>()
            .add_systems(Update, warm_up);
        let camera = app.world_mut().spawn(Camera::default()).id();
        app.insert_resource(PipBoyScreen { image: Handle::default(), camera, on_model: false, pointer: None, scale: 1.0 });
        let mut pip = PipBoy::default();
        pip.phase = if covered { Phase::Down } else { Phase::Raising };
        pip.frame = if device { Frame::Device } else { Frame::Screen };
        pip.warming = Some(Warming {
            since: std::time::Instant::now() - std::time::Duration::from_secs(4),
            frames: 0, asked_at: 0, covered, device, saved_frame: Frame::Device, saved_page: None, drawn: false,
        });
        app.insert_resource(pip);
        app
    }

    #[test]
    fn a_cold_raise_releases_at_the_cap_even_without_a_rig_and_does_not_hold_again() {
        let mut app = capped_warm_app(false, true);
        app.update();
        let pip = app.world().resource::<PipBoy>();
        assert!(pip.warming.is_none() && !pip.warmed);
        assert_eq!((pip.phase, pip.clip_time), (Phase::Raising, 0.0));
        app.update();
        assert!(app.world().resource::<PipBoy>().warming.is_none());
    }

    #[test]
    fn a_cold_raise_can_cancel_before_the_cap() {
        let mut app = capped_warm_app(false, true);
        app.world_mut().resource_mut::<PipBoy>().warming.as_mut().unwrap().since = std::time::Instant::now();
        app.update();
        assert!(app.world().resource::<PipBoy>().warming.is_some());
        app.world_mut().resource_mut::<PipBoy>().lower();
        app.update();
        let pip = app.world().resource::<PipBoy>();
        assert!(pip.warming.is_none());
        assert_eq!(pip.phase, Phase::Down);
    }

    #[test]
    fn screen_only_warm_keeps_a_later_device_cold_and_waits_for_camera_teardown() {
        let mut app = capped_warm_app(true, false);
        {
            let mut pip = app.world_mut().resource_mut::<PipBoy>();
            pip.page = Page::Stats;
            pip.warming.as_mut().unwrap().saved_page = Some((Page::Items, 3));
        }
        app.update();
        let pip = app.world().resource::<PipBoy>();
        assert!(pip.warming.is_none() && !pip.warmed);
        assert_eq!(pip.frame, Frame::Device);
        assert_eq!((pip.page, pip.tabs[Page::Stats.index()]), (Page::Items, 3));
        assert!(app.world().resource::<nvf_play::loading::Loading>().pipboy_warming);
        app.update();
        assert!(app.world().resource::<nvf_play::loading::Loading>().pipboy_warming);
        let camera = app.world().resource::<PipBoyScreen>().camera;
        app.world_mut().get_mut::<Camera>(camera).unwrap().is_active = false;
        app.update();
        assert!(!app.world().resource::<nvf_play::loading::Loading>().pipboy_warming);
    }

    #[test]
    fn the_parts_are_known_by_their_shapes() {
        assert_eq!(part("pipboyscreen:0"), Some(PipBoyPart::Screen));
        assert_eq!(part("ItemsGlow:0"), Some(PipBoyPart::Glow(Page::Items)));
        assert_eq!(part("PipBoyButton01:1"), Some(PipBoyPart::Button(Page::Stats)));
        assert_eq!(part("PipBoyButton03:0"), Some(PipBoyPart::Button(Page::Data)));
        assert_eq!(part("ScreenLit:8"), Some(PipBoyPart::ScreenLit));
        assert_eq!(part("glare:0"), Some(PipBoyPart::Glare));
        assert_eq!(part("RadNeedle:0"), Some(PipBoyPart::Needle));
        assert_eq!(part("RadNeedle:1"), Some(PipBoyPart::Needle));
        assert_eq!(part("TabKnob:0"), Some(PipBoyPart::TabKnob));
        assert_eq!(part("ScrollKnob:0"), Some(PipBoyPart::ScrollKnob));
        assert_eq!(part("RadNeedle"), None);
        assert_eq!(part("PipBoyArm:0"), None);
    }

    #[test]
    fn a_posed_shape_is_hit_and_found_by_its_texture_coordinates() {
        // A square 2 x 2 at z = 5 facing the origin, uv 0..1, moved up by `world`.
        let mut m = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]]);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        m.insert_indices(bevy::mesh::Indices::U16(vec![0, 1, 2, 0, 2, 3]));
        let s = Posed::new(&m, None, Mat4::from_translation(Vec3::new(0.0, 0.0, 5.0))).unwrap();
        let (t, uv) = s.hit(Vec3::ZERO, Vec3::new(0.5, 0.0, 5.0).normalize()).unwrap();
        assert!((uv - Vec2::new(0.75, 0.5)).length() < 1e-4, "{uv}");
        assert!((t - Vec3::new(0.5, 0.0, 5.0).length()).abs() < 1e-4);
        assert!(s.hit(Vec3::ZERO, Vec3::new(3.0, 0.0, 5.0).normalize()).is_none());
        let p = s.at_uv(Vec2::new(0.75, 0.5)).unwrap();
        assert!((p - Vec3::new(0.5, 0.0, 5.0)).length() < 1e-4);
        // Skinned: one joint moving everything 2 to the side.
        m.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(vec![[0, 0, 0, 0]; 4]));
        m.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, vec![[1.0f32, 0.0, 0.0, 0.0]; 4]);
        let joint = [Mat4::from_translation(Vec3::new(2.0, 0.0, 5.0))];
        let s = Posed::new(&m, Some(&joint), Mat4::IDENTITY).unwrap();
        assert!((s.middle().unwrap() - Vec3::new(2.0, 0.0, 5.0)).length() < 1e-4);
    }

    #[test]
    fn the_screen_maps_onto_the_canvas() {
        assert_eq!(canvas_point(Vec2::ZERO), Some(Vec2::ZERO));
        let p = canvas_point(Vec2::new(0.4, 0.4)).unwrap();
        assert!((p - CANVAS / 2.0).length() < 1e-3);
        assert_eq!(canvas_point(Vec2::new(0.81, 0.1)), None);
    }

    fn surface(colour: Color) -> NifMaterial {
        let mut m = nvf_assets::nif_material::nif_material(StandardMaterial { base_color: colour, alpha_mode: AlphaMode::Add, ..default() });
        m.extension.first_person = true;
        m.extension.soft = Vec4::new(1.5, 0.0, 0.0, 0.0);
        m
    }

    #[test]
    fn the_glare_adds_in_linear_light_on_the_arms_depth_keeping_its_texture() {
        let mut own = surface(Color::WHITE);
        own.base.base_color_texture = Some(Handle::default());
        let m = glare_material(&own);
        assert!(m.extension.add_linear && m.base.unlit && m.base.alpha_mode == AlphaMode::Add);
        assert!(m.extension.first_person, "on the arms' squeezed depth");
        assert_eq!(m.extension.soft, Vec4::ZERO, "not thinned by the screen just behind it");
        assert!(m.base.base_color_texture.is_some());
        let [r, g, b] = GLARE_LINEAR_SCALE;
        assert_eq!(m.base.base_color.to_linear(), LinearRgba::rgb(r, g, b));
        // The source is a copy away, and keeps the engine's add.
        assert!(!own.extension.add_linear && own.base.base_color == Color::WHITE);
    }

    /// #2981: the glare and menus copies of a material that wore an authored set carry none
    /// of it (no ORM, occlusion or depth map, no mark, no legacy look to apply one again).
    #[test]
    fn the_unlit_copies_of_an_authored_material_carry_no_set() {
        let mut images = Assets::<Image>::default();
        let mut own = surface(Color::WHITE);
        own.base.alpha_mode = AlphaMode::Opaque;
        let orm = images.add(Image::default());
        own.base.metallic_roughness_texture = Some(orm.clone());
        own.base.occlusion_texture = Some(orm);
        own.base.depth_map = Some(images.add(Image::default()));
        own.extension.emissive_rule.w = nvf_assets::authored::AUTHORED_MARK;
        own.extension.legacy = Some(nvf_assets::authored::LegacyLook { normal: None, specular: None, roughness: 1.0, reflectance: 0.3, metallic: 0.0, tangents: false, specular_tint: Color::WHITE, parallax: Default::default(), scripted_tint: None });
        for m in [glare_material(&own), menus_material(&own, Handle::default())] {
            assert!(m.base.metallic_roughness_texture.is_none() && m.base.occlusion_texture.is_none() && m.base.depth_map.is_none());
            assert!(!nvf_assets::authored::is_authored(&m) && m.extension.legacy.is_none());
        }
        assert!(own.base.metallic_roughness_texture.is_some(), "the source left alone");
    }

    #[test]
    fn the_menus_copy_wears_the_screen_look_and_takes_its_changes_once() {
        let own = surface(Color::WHITE);
        let m = menus_material(&own, Handle::default());
        assert_eq!((m.extension.pipboy_a, m.extension.pipboy_b), ScreenLook::default().surface(), "the static look from the start");
        assert_eq!((own.extension.pipboy_a, own.extension.pipboy_b), (Vec4::ZERO, Vec4::ZERO), "the source left alone");
        let mut materials = Assets::<NifMaterial>::default();
        let menus = materials.add(m);
        assert!(!put_look(&menus, &ScreenLook::default(), &mut materials), "the same look writes nothing");
        let washed = ScreenLook::during(nvf_play::pipboy::transition::Kind::VerticalHold, 100.0);
        assert!(put_look(&menus, &washed, &mut materials));
        assert_eq!((materials.get(&menus).unwrap().extension.pipboy_a, materials.get(&menus).unwrap().extension.pipboy_b), washed.surface());
        assert!(!put_look(&menus, &washed, &mut materials));
    }

    #[test]
    fn the_look_goes_on_after_the_page_change_and_only_while_the_copy_is_worn() {
        use nvf_play::pipboy::transition::Kind;
        let mut app = App::new();
        app.init_resource::<Assets<NifMaterial>>().init_resource::<MenusWorn>().insert_resource(ScreenLook::during(Kind::Plain, 0.0));
        // The order in the plugin: after the transition's system.
        app.add_systems(Update, (|mut look: ResMut<ScreenLook>| *look = ScreenLook::during(Kind::Shudder, 95.0), upload_look).chain());
        let menus = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(menus_material(&surface(Color::WHITE), Handle::default()));
        // Not worn: nothing is written.
        app.update();
        let m = app.world().resource::<Assets<NifMaterial>>().get(&menus).unwrap();
        assert_eq!((m.extension.pipboy_a, m.extension.pipboy_b), ScreenLook::default().surface());
        // Worn: the look of this frame, not the last one's.
        app.world_mut().resource_mut::<MenusWorn>().0 = Some(menus.clone());
        app.update();
        let m = app.world().resource::<Assets<NifMaterial>>().get(&menus).unwrap();
        assert_eq!((m.extension.pipboy_a, m.extension.pipboy_b), ScreenLook::during(Kind::Shudder, 95.0).surface());
    }

    /// `On` says the menus are on the model, as `show_screen` would, and the image they are
    /// drawn into.
    #[derive(Resource)]
    struct On(bool, Handle<Image>);

    #[derive(Resource)]
    struct Announce(bool);

    #[derive(Resource, Default)]
    struct Reported(bool);

    fn material_app() -> App {
        let mut app = App::new();
        app.init_resource::<StealthField>().init_resource::<Assets<NifMaterial>>().init_resource::<Reported>().insert_resource(Announce(true)).insert_resource(On(false, Handle::default())).add_systems(
            Update,
            |on: Res<On>, announce: Res<Announce>, mut reported: ResMut<Reported>, mut parts: Query<(Entity, &PipBoyPart, &mut MeshMaterial3d<NifMaterial>)>, mut field: ResMut<StealthField>, mut materials: ResMut<Assets<NifMaterial>>, mut swapped: Local<Swapped>| {
                wear_materials(on.0, announce.0, &on.1, &mut parts, &mut field, &mut materials, &mut swapped);
                reported.0 = swapped.reported;
            },
        );
        app
    }

    /// The screen and the glare of a rig, on materials of their own.
    fn rig(app: &mut App) -> ((Entity, Handle<NifMaterial>), (Entity, Handle<NifMaterial>)) {
        rig_on(app, surface(Color::BLACK), surface(Color::WHITE))
    }

    fn rig_on(app: &mut App, screen: NifMaterial, glare: NifMaterial) -> ((Entity, Handle<NifMaterial>), (Entity, Handle<NifMaterial>)) {
        let world = app.world_mut();
        let mut materials = world.resource_mut::<Assets<NifMaterial>>();
        let (s, g) = (materials.add(screen), materials.add(glare));
        let screen = world.spawn((PipBoyPart::Screen, MeshMaterial3d(s.clone()))).id();
        let glare = world.spawn((PipBoyPart::Glare, MeshMaterial3d(g.clone()))).id();
        ((screen, s), (glare, g))
    }

    fn worn(app: &App, e: Entity) -> Handle<NifMaterial> {
        app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0.clone()
    }

    fn glare_worn(app: &App, e: Entity) -> bool {
        app.world().resource::<Assets<NifMaterial>>().get(&worn(app, e)).is_some_and(|m| m.extension.add_linear)
    }

    #[test]
    fn hidden_swaps_are_silent_and_teardown_keeps_the_same_copies_for_the_real_open() {
        let mut app = material_app();
        let ((screen, own_screen), (glare, own_glare)) = rig(&mut app);
        app.insert_resource(On(true, Handle::default())).insert_resource(Announce(false));
        app.update();
        let copies = (worn(&app, screen), worn(&app, glare));
        assert!(!app.world().resource::<Reported>().0);
        // A cold hold can finish without taking the copies off.
        app.insert_resource(Announce(true));
        app.update();
        assert!(app.world().resource::<Reported>().0);
        assert_eq!((worn(&app, screen), worn(&app, glare)), copies);
        app.insert_resource(On(false, Handle::default()));
        app.update();
        assert_eq!((worn(&app, screen), worn(&app, glare)), (own_screen, own_glare));
        app.insert_resource(On(true, Handle::default()));
        app.update();
        assert_eq!((worn(&app, screen), worn(&app, glare)), copies);
    }

    fn worn_material(app: &App, e: Entity) -> NifMaterial {
        app.world().resource::<Assets<NifMaterial>>().get(&worn(app, e)).unwrap().clone()
    }

    #[test]
    fn a_rig_on_other_materials_gets_copies_of_its_own() {
        let mut app = material_app();
        app.init_resource::<Assets<Image>>();
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let [t1, t2, menus_1, menus_2] = [(); 4].map(|_| images.add(Image::default()));
        // Two rigs whose materials differ in the texture and in settings a copy keeps.
        let on = |texture: &Handle<Image>, double_sided: bool, decal: bool| {
            let mut m = surface(Color::WHITE);
            m.base.base_color_texture = Some(texture.clone());
            m.base.double_sided = double_sided;
            m.extension.decal = decal;
            m
        };
        let ((screen_1, _), (glare_1, _)) = rig_on(&mut app, on(&t1, false, false), on(&t1, false, false));
        app.world_mut().insert_resource(On(true, menus_1.clone()));
        app.update();
        let (first_screen, first_glare) = (worn(&app, screen_1), worn(&app, glare_1));
        assert_eq!(worn_material(&app, glare_1).base.base_color_texture.as_ref(), Some(&t1));
        // Down and up on the same sources and image: the same copies.
        app.world_mut().insert_resource(On(false, menus_1.clone()));
        app.update();
        app.world_mut().insert_resource(On(true, menus_1.clone()));
        app.update();
        assert!(worn(&app, screen_1) == first_screen && worn(&app, glare_1) == first_glare, "one copy for each source");
        // The rig rebuilt on other materials, while up: nothing of the first rig's.
        let ((screen_2, own_screen_2), (glare_2, own_glare_2)) = rig_on(&mut app, on(&t2, true, false), on(&t2, true, true));
        app.world_mut().despawn(screen_1);
        app.world_mut().despawn(glare_1);
        app.update();
        let glare = worn_material(&app, glare_2);
        assert!(worn(&app, glare_2) != first_glare);
        assert_eq!(glare.base.base_color_texture.as_ref(), Some(&t2));
        assert!(glare.base.double_sided && glare.extension.decal && glare.extension.add_linear);
        let screen = worn_material(&app, screen_2);
        assert!(worn(&app, screen_2) != first_screen);
        assert!(screen.base.double_sided, "from the new rig's own");
        assert_eq!(screen.base.base_color_texture.as_ref(), Some(&menus_1));
        // Another image on the same source: another screen copy, the glare's as it was.
        let glare_copy = worn(&app, glare_2);
        app.world_mut().insert_resource(On(false, menus_2.clone()));
        app.update();
        let own_screen_2_again = own_screen_2.clone();
        assert_eq!((worn(&app, screen_2), worn(&app, glare_2)), (own_screen_2, own_glare_2));
        app.world_mut().insert_resource(On(true, menus_2.clone()));
        app.update();
        assert_eq!(worn_material(&app, screen_2).base.base_color_texture.as_ref(), Some(&menus_2));
        assert!(worn(&app, glare_2) == glare_copy);
        // Replaced while up, no reopening: the shape wears the new image the same frame.
        app.world_mut().insert_resource(On(true, menus_1.clone()));
        app.update();
        assert_eq!(worn_material(&app, screen_2).base.base_color_texture.as_ref(), Some(&menus_1));
        // Down, it still gets its own back.
        app.world_mut().insert_resource(On(false, menus_1.clone()));
        app.update();
        assert_eq!(worn(&app, screen_2), own_screen_2_again);
    }

    #[test]
    fn the_shapes_get_their_own_materials_back_down_and_the_new_rig_gets_the_copies() {
        let mut app = material_app();
        let ((screen, own_screen), (glare, own_glare)) = rig(&mut app);
        app.update();
        assert_eq!((worn(&app, screen), worn(&app, glare)), (own_screen.clone(), own_glare.clone()), "down: their own");
        // Up: copies, the sources as they were.
        app.world_mut().insert_resource(On(true, Handle::default()));
        app.update();
        assert!(worn(&app, screen) != own_screen);
        assert!(glare_worn(&app, glare));
        let materials = app.world().resource::<Assets<NifMaterial>>();
        assert!(!materials.get(&own_glare).unwrap().extension.add_linear);
        assert_eq!(materials.get(&own_glare).unwrap().base.base_color, Color::WHITE);
        // The rig rebuilt while up: the new shapes wear them, and down they get their own.
        let ((screen_2, own_screen_2), (glare_2, own_glare_2)) = rig(&mut app);
        app.world_mut().despawn(screen);
        app.world_mut().despawn(glare);
        app.update();
        assert!(glare_worn(&app, glare_2));
        assert!(worn(&app, screen_2) != own_screen_2);
        app.world_mut().insert_resource(On(false, Handle::default()));
        app.update();
        assert_eq!((worn(&app, screen_2), worn(&app, glare_2)), (own_screen_2, own_glare_2));
    }

    #[test]
    fn a_rig_without_a_glare_or_a_screen_is_left_alone() {
        let mut app = material_app();
        let own = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(surface(Color::WHITE));
        let glare = app.world_mut().spawn((PipBoyPart::Glare, MeshMaterial3d(own.clone()))).id();
        // No screen: `show_screen` says the menus are not on the model.
        app.update();
        assert_eq!(worn(&app, glare), own);
        // A screen and no glare: the menus alone.
        app.world_mut().despawn(glare);
        let own_screen = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(surface(Color::BLACK));
        let screen = app.world_mut().spawn((PipBoyPart::Screen, MeshMaterial3d(own_screen.clone()))).id();
        app.world_mut().insert_resource(On(true, Handle::default()));
        app.update();
        assert!(worn(&app, screen) != own_screen);
    }

    #[test]
    fn under_the_stealth_field_the_glare_is_built_from_the_real_material() {
        let mut app = material_app();
        let ((_, _), (glare, own_glare)) = rig(&mut app);
        // The field's see-through copy on the glare, with the shape's own kept by the field.
        let mut copy = surface(Color::WHITE);
        copy.extension.refract = true;
        let copy = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(copy);
        app.world_mut().get_mut::<MeshMaterial3d<NifMaterial>>(glare).unwrap().0 = copy.clone();
        app.world_mut().resource_mut::<StealthField>().keep_original(glare, own_glare.clone());
        app.world_mut().insert_resource(On(true, Handle::default()));
        app.update();
        let materials = app.world().resource::<Assets<NifMaterial>>();
        let on = materials.get(&worn(&app, glare)).unwrap();
        assert!(on.extension.add_linear && !on.extension.refract, "plain, not refracting");
        assert!(materials.get(&copy).is_none(), "the see-through copy is dropped");
        // Down: the real one for the field to make see-through again.
        app.world_mut().insert_resource(On(false, Handle::default()));
        app.update();
        assert_eq!(worn(&app, glare), own_glare);
    }

    /// A square 2 x 2 facing +Z about its origin, uv 0..1.
    fn square() -> Arc<Mesh> {
        let mut m = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]]);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        m.insert_indices(bevy::mesh::Indices::U16(vec![0, 1, 2, 0, 2, 3]));
        Arc::new(m)
    }

    /// The pointer systems as the game chains them, then the menus' taking of the input
    /// (`PlaySet::Ui`, after them), over a camera, three buttons and their glows.
    fn pointer_app() -> (App, [Entity; 3]) {
        let mut app = App::new();
        app.init_resource::<PipBoy>()
            .init_resource::<PipBoyAid>()
            .init_resource::<PlayerBody>()
            .init_resource::<StealthField>()
            .init_resource::<Assets<NifMaterial>>()
            .init_resource::<Assets<SkinnedMeshInverseBindposes>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<PipBoyInput>()
            .insert_resource(PipBoyScreen { image: Handle::default(), camera: Entity::PLACEHOLDER, on_model: true, pointer: None, scale: 1.0 })
            .add_systems(
                Update,
                (follow_pointer, show_screen, |mut input: MessageReader<PipBoyInput>, mut pip: ResMut<PipBoy>| {
                    for &i in input.read() {
                        pip.apply(i);
                    }
                })
                    .chain(),
            );
        let world = app.world_mut();
        world.spawn((Window::default(), PrimaryWindow));
        let mut camera = Camera::default();
        camera.computed.clip_from_view = bevy::camera::CameraProjection::get_clip_from_view(&PerspectiveProjection::default());
        camera.computed.target_info = Some(bevy::camera::RenderTargetInfo { physical_size: UVec2::new(1280, 960), scale_factor: 1.0 });
        world.spawn((camera, GlobalTransform::IDENTITY, PlayerCamera));
        let mesh = square();
        let glows = Page::ALL.map(|page| {
            let x = page.index() as f32 * 3.0 - 3.0;
            world.spawn((PipBoyPart::Button(page), PartMesh(mesh.clone()), GlobalTransform::from_translation(Vec3::new(x, 0.0, -10.0))));
            world.spawn((PipBoyPart::Glow(page), MeshMaterial3d::<NifMaterial>::default(), Visibility::Inherited)).id()
        });
        world.resource_mut::<PipBoy>().page = Page::Items;
        world.resource_mut::<PipBoy>().phase = nvf_play::pipboy::Phase::Up;
        (app, glows)
    }

    /// A frame; the arms are not built here, so the screen is said to be on the model.
    fn frame(app: &mut App) {
        app.world_mut().resource_mut::<PipBoyScreen>().on_model = true;
        app.update();
    }

    /// Projection/command fixture supplies an explicit on-model screen. It does
    /// not run show_screen or claim first-person body/wear ownership coverage.
    pub(crate) fn model_control_fixture() -> App {
        let mut app=App::new();
        let mut pip=PipBoy::default(); pip.phase=nvf_play::pipboy::Phase::Up; pip.page=Page::Items;
        app.init_resource::<PipBoyAid>().init_resource::<Assets<SkinnedMeshInverseBindposes>>()
            .init_resource::<ButtonInput<MouseButton>>().add_message::<PipBoyInput>()
            .insert_resource(pip)
            .insert_resource(PipBoyScreen {image:Handle::default(),camera:Entity::PLACEHOLDER,on_model:true,pointer:None,scale:1.0})
            .add_systems(Update,follow_pointer.in_set(nvf_play::pipboy::PointerSet))
            .add_systems(Update,(|mut input:MessageReader<PipBoyInput>,mut pip:ResMut<PipBoy>| {
                for &command in input.read() {pip.apply(command);}
            }).in_set(nvf_play::PlaySet::Ui));
        app.world_mut().spawn((Window::default(),PrimaryWindow));
        let mut camera=Camera::default();
        camera.computed.clip_from_view=bevy::camera::CameraProjection::get_clip_from_view(&PerspectiveProjection::default());
        camera.computed.target_info=Some(bevy::camera::RenderTargetInfo {physical_size:UVec2::new(1280,960),scale_factor:1.0});
        app.world_mut().spawn((camera,GlobalTransform::IDENTITY,PlayerCamera));
        for page in Page::ALL {
            app.world_mut().spawn((PipBoyPart::Button(page),PartMesh(square()),GlobalTransform::from_translation(Vec3::new(page.index() as f32*3.0-3.0,-2.5,-10.0)),Visibility::Visible,InheritedVisibility::VISIBLE));
        }
        app.world_mut().spawn((PipBoyPart::Screen,PartMesh(square()),GlobalTransform::from(Transform::from_xyz(0.0,2.0,-10.0).with_scale(Vec3::new(3.0,2.0,1.0))),Visibility::Visible,InheritedVisibility::VISIBLE));
        app
    }

    fn lit(app: &App, glows: [Entity; 3]) -> [bool; 3] {
        glows.map(|g| *app.world().get::<Visibility>(g).unwrap() != Visibility::Hidden)
    }

    #[test]
    fn the_pointer_lights_no_button_a_click_shows_its_page_and_down_all_glow() {
        let (mut app, glows) = pointer_app();
        frame(&mut app);
        assert_eq!(lit(&app, glows), [false, true, false], "only the page shown");
        // The pointer on another page's button: neither the page nor the lights change.
        app.world_mut().resource_mut::<PipBoyAid>().target = Some(AidTarget::Button(Page::Stats));
        for _ in 0..3 {
            frame(&mut app);
        }
        assert_eq!(app.world().resource::<PipBoy>().page, Page::Items);
        assert_eq!(lit(&app, glows), [false, true, false], "hovering lights nothing");
        // A click on it shows its page, whose button lights the frame after.
        app.world_mut().resource_mut::<PipBoyAid>().click = 3;
        frame(&mut app);
        assert_eq!(app.world().resource::<PipBoy>().page, Page::Stats);
        frame(&mut app);
        assert_eq!(lit(&app, glows), [true, false, false], "the page clicked");
        // Down: all three.
        app.world_mut().resource_mut::<PipBoy>().phase = nvf_play::pipboy::Phase::Down;
        frame(&mut app);
        assert_eq!(lit(&app, glows), [true, true, true], "down, all glow");
    }
    include!("pipboy_arm/pointer_pose_safety_tests.rs");
    include!("pipboy_arm/map_touch_projection_tests.rs");

    #[test]
    fn requeue_raise_refreshes_while_raising_and_once_when_entering_up() {
        #[derive(Resource, Default)]
        struct Refreshed(bool);
        let mut app = App::new();
        app.init_resource::<PipBoy>()
            .init_resource::<Refreshed>()
            .add_systems(
                Update,
                (
                    requeue_raise,
                    |q: Query<Ref<Mesh3d>, With<crate::pipboy_ready::PipBoyShape>>, mut r: ResMut<Refreshed>| {
                        r.0 = q.single().is_ok_and(|m| m.is_changed());
                    },
                )
                    .chain(),
            );
        app.world_mut().spawn((Mesh3d::default(), crate::pipboy_ready::PipBoyShape));
        let refreshed = |app: &App| app.world().resource::<Refreshed>().0;
        // First frame marks spawned component as added/changed; settle baseline.
        app.update();

        // Down: no refresh.
        app.update();
        assert!(!refreshed(&app));

        // Raising: refreshes every frame.
        app.world_mut().resource_mut::<PipBoy>().phase = Phase::Raising;
        app.update();
        assert!(refreshed(&app));

        // First Up frame: refreshes once so meshes newly inside the frustum or switched
        // from Frame::Screen -> Frame::Device enter the main bins.
        app.world_mut().resource_mut::<PipBoy>().phase = Phase::Up;
        app.update();
        assert!(refreshed(&app));

        // Subsequent Up frames: no per-frame churn.
        app.update();
        assert!(!refreshed(&app));

        // Switching to Screen and back to warm Device (direct Phase::Up) re-arms the single refresh.
        app.world_mut().resource_mut::<PipBoy>().frame = Frame::Screen;
        app.update();
        assert!(!refreshed(&app));
        app.world_mut().resource_mut::<PipBoy>().frame = Frame::Device;
        app.update();
        assert!(refreshed(&app));
    }
}
