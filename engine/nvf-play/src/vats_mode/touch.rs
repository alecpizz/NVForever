//! Optional VATS finger owner for both targeting styles; see docs/design/android-vats-touch.md.
//! The wrapper supplies the sole raw stream. No mouse, keyboard or Touches writes.
use super::*;
use bevy::{camera::{NormalizedRenderTarget, RenderTarget}, ecs::system::SystemParam,
    input::touch::{TouchInput, TouchPhase}, window::PrimaryWindow};

pub const SETTING_KEY: &str = "touch-vats-targeting-v1";
const HOLD: f64 = 0.45;
const MAX_RETIRED: usize = 16;
const MAX_POINTS: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode { #[default] Off, Tap, Hold }
impl Mode {
    fn name(self) -> &'static str { match self { Self::Off => "Off", Self::Tap => "Tap", Self::Hold => "Hold" } }
    fn parse(name: &str) -> Option<Self> { match name { "Off" => Some(Self::Off), "Tap" => Some(Self::Tap), "Hold" => Some(Self::Hold), _ => None } }
}

/// Logical window pixels from the root's actual primary window/world camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub window: Entity,
    pub camera: Entity,
    pub logical: Vec2,
    pub physical: UVec2,
    pub density: f32,
    pub admitted: bool,
    pub fresh: bool,
    /// True only with no held, release or cancel contacts in the real batch.
    pub neutral: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry { frame: Frame, viewport: URect, projection: Mat4 }
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stay { entered: Option<f64>, weapon: Option<FormId>, style: VatsStyle }
impl Stay { fn of(v: &VatsMode) -> Self { Self { entered: v.entered_at, weapon: v.weapon.as_ref().map(|w| w.id), style: v.style } } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Body { actor: u32, part: Option<u8> }
#[derive(Clone, Copy)]
struct Contact {
    id: u64, at: Vec2, body: Option<Body>, started: f64, committed: bool, fire: bool,
    // Bounded raw path for the real consumer; out-and-back cannot become a tap.
    points: [Vec2; MAX_POINTS], len: usize,
}
#[derive(Clone, Copy)]
struct Request { contact: Contact, geometry: Geometry, stay: Stay }

#[derive(Resource)]
pub struct State {
    mode: Mode,
    geometry: Option<Geometry>,
    stay: Option<Stay>,
    contact: Option<Contact>,
    retired: Vec<u64>,
    overflow: bool,
    hardware_hidden: bool,
    pending: Option<Request>,
    highlight: Option<Body>,
    progress: f32,
    queued: Option<bool>,
}
impl State {
    pub fn for_platform(android: bool) -> Self {
        Self {
            mode: if android { Mode::Tap } else { Mode::Off },
            geometry: None, stay: None, contact: None, retired: Vec::new(),
            overflow: false, hardware_hidden: false, pending: None,
            highlight: None, progress: 0.0, queued: None,
        }
    }
}
impl Default for State {
    fn default() -> Self { Self::for_platform(cfg!(target_os = "android")) }
}
impl State {
    pub fn mode(&self) -> Mode { self.mode }
    pub fn progress(&self) -> f32 { self.progress }
    pub fn highlighted(&self) -> Option<(u32, Option<u8>)> { self.highlight.map(|b| (b.actor, b.part)) }
    fn retire(&mut self, id: u64) {
        if !self.retired.contains(&id) {
            if self.retired.len() < MAX_RETIRED { self.retired.push(id); } else { self.overflow = true; }
        }
    }
    fn cancel(&mut self) {
        if let Some(c) = self.contact.take() { self.retire(c.id); }
        if let Some(r) = self.pending.take() { self.retire(r.contact.id); }
        self.highlight = None;
        self.progress = 0.0;
        self.queued = None;
    }
    fn active(&self) -> bool { self.mode != Mode::Off && self.geometry.is_some() && self.stay.is_some() && !self.hardware_hidden && !self.overflow }
    fn request(&mut self, c: Contact) {
        if let (Some(geometry), Some(stay)) = (self.geometry, self.stay) {
            self.pending = Some(Request { contact: c, geometry, stay });
        }
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<State>();
    #[cfg(any(target_os = "android", test))]
    register_settings(app);
    app.add_systems(PostUpdate, feedback.after(VatsUpdate).before(bevy::ui::UiSystems::Layout));
}

pub fn register_settings(app: &mut App) {
    app.init_resource::<State>();
    if app.world().get_resource::<crate::settings::Settings>().is_some_and(|s| s.index(SETTING_KEY).is_some()) {
        return;
    }
    crate::settings::add_choice_list(app, crate::settings::Setting {
        key: SETTING_KEY, group: "Touch", label: "V.A.T.S. body selection",
        note: "Classic and Mark and Fire: tap or hold a body part to queue once; hold takes 0.45 seconds. Off uses existing controls. Touch Fire confirms queued attacks.",
        switches: "", get: |_| false, set: |_, _| {}, slider: None,
    }, crate::settings::ChoiceList {
        names: |_| [Mode::Off, Mode::Tap, Mode::Hold].iter().map(|m| m.name().to_owned()).collect(),
        chosen: |w| w.resource::<State>().mode.name().to_owned(), pick,
        save: |w| w.resource::<State>().mode.name().to_owned(), load: pick, ask_keep: false,
    });
}
fn pick(w: &mut World, name: &str) -> bool {
    let Some(mode) = Mode::parse(name) else { return false };
    let mut s = w.resource_mut::<State>();
    if s.mode != mode { s.cancel(); s.mode = mode; s.geometry = None; s.stay = None; }
    true
}

#[derive(SystemParam)]
struct Admission<'w> {
    bindings: Option<Res<'w, crate::bindings::Bindings>>,
    platform: Option<Res<'w, crate::pad::PlatformInputGate>>,
    switching: Option<Res<'w, crate::pad::TouchDeviceSwitch>>,
    keyboard: Option<Res<'w, crate::bindings::KeyboardCapture>>,
    free_camera: Option<Res<'w, crate::free_cam::FreeCam>>,
    paused: Option<Res<'w, crate::pause::Paused>>,
    creator: Option<Res<'w, crate::creator::CreatorMenu>>,
    fresh: Option<Res<'w, crate::menu::FreshMenu>>,
}
impl Admission<'_> {
    fn allows(&self, id: u64) -> bool {
        self.allows_routed(id) && self.switching.as_ref().is_none_or(|s| s.allows_actions())
    }
    // The parent's routed raw stream precedes InputSystems/automatic selector.
    // TouchDeviceSwitch there still describes the previous frame. Actual return
    // contacts are quarantined by that parent; Consumer rechecks CURRENT switch.
    fn allows_routed(&self, id: u64) -> bool {
        self.bindings.as_ref().is_none_or(|b| b.device != crate::bindings::InputDevice::Controller)
            && self.platform.as_ref().is_none_or(|p| p.allows_actions() && p.allows_touch(id))
            && self.keyboard.as_ref().is_none_or(|k| !k.0)
            && self.free_camera.as_ref().is_none_or(|c| !c.active)
            && self.paused.as_ref().is_none_or(|p| !p.0)
            && self.creator.as_ref().is_none_or(|c| !c.is_open())
            && self.fresh.as_ref().is_none_or(|f| !f.0)
    }
}

#[derive(SystemParam)]
struct Projection<'w, 's> {
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    cameras: Query<'w, 's, (&'static Camera, &'static GlobalTransform, &'static RenderTarget), With<PlayerCamera>>,
}
impl Projection<'_, '_> {
    fn geometry(&self, mut f: Frame) -> Option<Geometry> {
        let w = self.windows.get(f.window).ok()?;
        let (c, _, target) = self.cameras.get(f.camera).ok()?;
        let viewport = c.physical_viewport_rect()?;
        if !f.admitted || f.fresh || !w.focused || !f.logical.is_finite() || f.logical.min_element() <= 0.0
            || f.physical.min_element() == 0 || !f.density.is_finite() || f.density <= 0.0
            || w.resolution.size() != f.logical || w.resolution.physical_size() != f.physical
            || w.resolution.scale_factor() != f.density || !c.is_active || !c.clip_from_view().is_finite()
            || c.physical_target_size() != Some(f.physical)
            || viewport != URect::from_corners(UVec2::ZERO, f.physical)
            || c.logical_viewport_rect()?.size() != f.logical
            || !matches!(target.normalize(Some(f.window)), Some(NormalizedRenderTarget::Window(w)) if w.entity() == f.window) { return None; }
        // Neutral is a batch witness, not persistent geometry identity.
        f.neutral = false;
        Some(Geometry { frame: f, viewport, projection: c.clip_from_view() })
    }
    fn hit(&self, g: Geometry, at: Vec2, phys: &mut PhysState, v: &VatsMode) -> Option<Body> {
        if !at.is_finite() || !Rect::from_corners(Vec2::ZERO, g.frame.logical).contains(at) { return None; }
        let (c, t, _) = self.cameras.get(g.frame.camera).ok()?;
        let ray = c.viewport_to_world(t, at).ok()?;
        let from = crate::space::to_game(ray.origin);
        let dir = crate::space::to_game(*ray.direction);
        let hit = phys.ray(from, dir, v.weapon.as_ref()?.reach, shot_filter(phys))?;
        match hit.owner {
            Owner::Actor { form_id, part, .. } if phys.world.actor_volume(form_id).is_some_and(|b| !b.dead)
                && (v.style == VatsStyle::DeadEye || v.classic.targets.iter().any(|t| t.actor == form_id)) =>
                Some(Body { actor: form_id, part: (part != fnv_phys::actors::WHOLE).then_some(part) }),
            _ => None,
        }
    }
}

/// Root-owned input adapter: prepare before events; no independent raw reader.
#[derive(SystemParam)]
pub struct Bridge<'w, 's> {
    state: ResMut<'w, State>,
    vats: Res<'w, VatsMode>,
    real: Res<'w, Time<Real>>,
    projection: Projection<'w, 's>,
    gate: Admission<'w>,
}
impl Bridge<'_, '_> {
    pub fn cancel(&mut self) { self.state.cancel(); self.state.geometry = None; self.state.stay = None; }
    pub fn hardware_intent(&mut self) { self.cancel(); self.state.hardware_hidden = true; }
    /// Root calls only after its normal consumed controller-return/physical policy.
    pub fn restore_touch(&mut self) { self.state.hardware_hidden = false; }
    /// Trusted sole-reader witness; an unread raw batch is never neutral. This
    /// also releases retired IDs while projection is missing or another UI owns.
    pub fn observe_neutral(&mut self, neutral: bool) {
        if neutral { self.state.retired.clear(); self.state.overflow = false; }
    }
    fn actual_contact(&self, t: &TouchInput, primary: Entity) -> bool {
        self.state.mode != Mode::Off
            && self.vats.phase == Phase::Marking && t.window == primary
            && t.position.is_finite()
            && self.projection.windows.get(primary).is_ok_and(|window| window.focused)
    }
    /// Retire an actual controller-return/tail contact without inventing input or
    /// restoring a hardware-hidden presentation. Does not queue/accept attacks.
    pub fn quarantine_touch(&mut self, t: &TouchInput, primary: Entity) -> bool {
        if !self.actual_contact(t, primary) { return false; }
        self.cancel(); self.state.retire(t.id); true
    }
    /// The first real touch after physical input restores presentation only.
    /// Root has already applied higher UI/platform admission. KeyboardMouse alone
    /// cannot identify this edge because physical mouse and touch share it.
    pub fn consume_touch_return(&mut self, t: &TouchInput, primary: Entity) -> bool {
        if !self.state.hardware_hidden || t.phase != TouchPhase::Started
            || !self.actual_contact(t, primary) || !self.gate.allows_routed(t.id) { return false; }
        self.cancel(); self.state.hardware_hidden = false; self.state.retire(t.id); true
    }
    pub fn owns(&self, id: u64) -> bool {
        self.state.overflow || self.state.retired.contains(&id) || self.state.contact.is_some_and(|c| c.id == id)
    }
    /// Current live finger only; retired IDs remain owned but cannot establish a
    /// competing-contact relationship after geometry/hardware cancellation.
    pub fn active_contact(&self) -> Option<u64> { self.state.contact.map(|c| c.id) }
    /// Current non-selector gates for a routed ID. This is not late effect
    /// authority: Consumer still validates the actual current selector too.
    pub fn admits_routed(&self, id: u64) -> bool { self.gate.allows_routed(id) }
    pub fn available(&self) -> bool { self.state.active() }
    pub fn prepare(&mut self, frame: Option<Frame>) {
        if frame.is_some_and(|f| f.neutral) { self.state.retired.clear(); self.state.overflow = false; }
        let geometry = frame.and_then(|f| self.projection.geometry(f));
        let stay = (self.vats.phase == Phase::Marking && !self.vats.pad).then(|| Stay::of(&self.vats));
        if geometry != self.state.geometry || stay != self.state.stay || geometry.is_none() || stay.is_none() {
            self.state.cancel(); self.state.geometry = geometry; self.state.stay = stay;
        }
        if !self.state.active() { return; }
        if let Some(mut c) = self.state.contact {
            if !self.gate.allows_routed(c.id) {
                self.state.cancel(); return;
            }
            if !c.fire && !c.committed && self.state.mode == Mode::Hold {
                self.state.progress = ((self.real.elapsed_secs_f64() - c.started) / HOLD).clamp(0.0, 1.0) as f32;
                if self.state.progress >= 1.0 { self.state.request(c); c.committed = true; self.state.contact = Some(c); }
            }
        }
    }
    pub fn event(&mut self, t: &TouchInput) -> bool { self.route(t, false) }
    /// Only for the actual touch Fire target; never call for physical Use.
    pub fn fire(&mut self, t: &TouchInput) -> bool { self.route(t, true) }
    fn route(&mut self, t: &TouchInput, fire: bool) -> bool {
        if self.owns(t.id) && self.state.retired.contains(&t.id) { return true; }
        if !self.state.active() { return self.owns(t.id); }
        let g = self.state.geometry.unwrap();
        if t.window != g.frame.window || !self.gate.allows_routed(t.id) || !t.position.is_finite() { self.state.cancel(); return false; }
        if t.phase == TouchPhase::Started {
            if self.state.contact.is_some() || self.state.pending.is_some() {
                self.state.cancel(); self.state.retire(t.id); return true;
            }
            // A miss is owned until neutral too: it must not become a fallback click.
            let c = Contact { id: t.id, at: t.position, body: None, started: self.real.elapsed_secs_f64(), committed: fire, fire,
                points: [t.position; MAX_POINTS], len: 1 };
            self.state.contact = Some(c);
            self.state.queued = None;
            if fire { self.state.request(c); }
            return true;
        }
        let Some(mut c) = self.state.contact.filter(|c| c.id == t.id) else { return false };
        if c.fire != fire { self.state.cancel(); return true; }
        c.at = t.position;
        if t.phase == TouchPhase::Canceled || c.len == MAX_POINTS {
            self.state.cancel(); return true;
        }
        c.points[c.len] = t.position; c.len += 1;
        if let Some(r) = self.state.pending.as_mut().filter(|r| r.contact.id == c.id) { r.contact = c; }
        if t.phase == TouchPhase::Ended {
            if !fire && !c.committed && self.state.mode == Mode::Tap { self.state.request(c); }
            self.state.contact = None; self.state.retire(t.id); self.state.highlight = None; self.state.progress = 0.0;
        } else { self.state.contact = Some(c); }
        true
    }
}

/// Borrowed inside the actual VATS update; no adjacent message/queue guard contract.
#[derive(SystemParam)]
pub(super) struct Consumer<'w, 's> {
    state: Option<ResMut<'w, State>>,
    projection: Projection<'w, 's>,
    gate: Admission<'w>,
}
#[derive(Clone, Copy)]
pub(super) struct Selection { pub actor: u32, pub part: Option<u8>, pub queue: bool }
pub(super) struct Applied { pub selection: Option<Selection>, pub fire: bool, pub hold_camera: bool }
impl Consumer<'_, '_> {
    pub(super) fn refuse(&mut self) { if let Some(s) = self.state.as_mut() { s.cancel(); } }
    pub(super) fn queued(&mut self, success: bool) {
        if let Some(s) = self.state.as_mut() { s.queued = Some(success); }
    }
    pub(super) fn take(&mut self, v: &VatsMode, phys: &mut PhysState, keys: &Keys) -> Applied {
        let empty = || Applied { selection: None, fire: false, hold_camera: false };
        let Some(s) = self.state.as_mut() else { return empty() };
        if !s.active() || v.phase != Phase::Marking || v.pad
            || s.stay != Some(Stay::of(v)) || keys.mark || keys.accept || keys.toggle || keys.unmark || keys.special || keys.target_step != 0 || keys.part_step != 0 {
            s.cancel(); return empty();
        }
        let Some(g) = s.geometry.filter(|g| self.projection.geometry(g.frame) == Some(*g)) else { s.cancel(); return empty() };
        let request = s.pending.take();
        let id = request.map(|r| r.contact.id).or_else(|| s.contact.map(|c| c.id));
        if id.is_some_and(|id| !self.gate.allows(id)) { s.cancel(); return empty(); }
        if let Some(r) = request {
            if r.geometry != g || r.stay != Stay::of(v) { s.cancel(); return empty(); }
            let c = r.contact;
            if c.fire { return Applied { selection: None, fire: !v.queue.is_empty(), hold_camera: false }; }
            let body = c.body.or_else(|| self.projection.hit(g, c.points[0], phys, v));
            if let Some(body) = body.filter(|b| c.points[..c.len].iter().all(|at| self.projection.hit(g, *at, phys, v) == Some(*b))) {
                if let Some(contact) = s.contact.as_mut().filter(|old| old.id == c.id) {
                    contact.body = Some(body); contact.points[0] = contact.at; contact.len = 1;
                    s.highlight = Some(body);
                }
                return Applied { selection: Some(Selection { actor: body.actor, part: body.part, queue: true }), fire: false, hold_camera: s.contact.is_some() };
            }
            s.cancel(); return empty();
        }
        if let Some(mut c) = s.contact.filter(|c| !c.fire) {
            let body = c.body.or_else(|| self.projection.hit(g, c.points[0], phys, v));
            if let Some(body) = body.filter(|b| c.points[..c.len].iter().all(|at| self.projection.hit(g, *at, phys, v) == Some(*b))) {
                c.body = Some(body); c.points[0] = c.at; c.len = 1;
                s.contact = Some(c); s.highlight = Some(body);
                return Applied { selection: Some(Selection { actor: body.actor, part: body.part, queue: false }), fire: false, hold_camera: true };
            }
            s.cancel();
        }
        empty()
    }
}

#[derive(Component)]
struct Feedback;
type FeedbackNode<'a> = (&'a mut Node, &'a mut Text, &'a mut TextFont, &'a mut Visibility, &'a mut UiTargetCamera, &'a mut TextColor);
fn feedback(mut commands: Commands, state: Res<State>, vats: Res<VatsMode>, scale: Option<Res<UiScale>>, mut old: Query<FeedbackNode, With<Feedback>>) {
    let contact = state.contact.filter(|c| !c.fire && c.body.is_some());
    let geometry = state.geometry.filter(|_| state.active() && vats.phase == Phase::Marking);
    let scale = scale.as_ref().map_or(1.0, |s| s.0);
    let Some((c, g)) = contact.zip(geometry).filter(|(_, g)| scale.is_finite() && scale > 0.0
        && (g.frame.logical / scale).is_finite() && (18.0 / scale).is_finite()) else {
        for (_, _, _, mut visible, _, _) in &mut old { *visible = Visibility::Hidden; }
        return;
    };
    let word = if c.committed { match state.queued {
        Some(true) => "Queued · Fire confirms", Some(false) => "Cannot queue · check AP/ammo", None => "Selecting",
    }.to_owned() } else if state.mode == Mode::Hold { format!("Hold to queue · {:.0}%", state.progress * 100.0) } else { "Release to queue".to_owned() };
    let left = px((c.at.x - 100.0).clamp(0.0, (g.frame.logical.x - 200.0).max(0.0)) / scale);
    let top = px((c.at.y + 32.0).min((g.frame.logical.y - 28.0).max(0.0)) / scale);
    if let Ok((mut node, mut text, mut font, mut visible, mut camera, mut colour)) = old.single_mut() {
        node.left = left; node.top = top;
        if text.0 != word { text.0 = word; }
        font.font_size = FontSize::Px(18.0 / scale);
        *visible = Visibility::Visible;
        *camera = UiTargetCamera(g.frame.camera);
        colour.0 = crate::interface::amber(255.0);
    } else {
        commands.spawn((Feedback, UiTargetCamera(g.frame.camera), Pickable::IGNORE,
            Node { position_type: PositionType::Absolute, left, top, ..default() },
            GlobalZIndex(950), BackgroundColor(Color::srgba(0.02, 0.02, 0.01, 0.9)), Text::new(word),
            TextFont { font_size: FontSize::Px(18.0 / scale), ..default() }, TextColor(crate::interface::amber(255.0))));
    }
}

#[cfg(test)]
mod tests;
