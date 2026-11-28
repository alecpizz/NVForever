//! Optional Android maintenance presentation. Root input routing supplies one admitted
//! raw stream and an explicit full-window UI camera. This is not a native XML pointer.
use bevy::ecs::system::SystemParam;
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use std::{collections::VecDeque, sync::Arc};
use crate::{pipboy::{item_mod, repair as player_repair}, touch_safe_area::SafeInsets};

mod sheet;
#[cfg(test)]
pub(crate) mod tests;

const QUEUED: usize = 32;
const DRAG: f32 = 12.0;
const ROW: f32 = 48.0;
pub const SETTING_KEY: &str = "touch-maintenance-v1";

/// Exact presentation supplied by the common router, in logical window pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Context {
    pub window: Entity,
    pub camera: Entity,
    pub logical: Vec2,
    pub physical: UVec2,
    pub density: f32,
    pub focused: bool,
    pub safe: SafeInsets,
    pub ui_scale: f32,
}
impl Context {
    pub fn from_window(window: Entity, camera: Entity, value: &Window, safe: SafeInsets, ui_scale: f32) -> Self {
        Self { window, camera, logical: value.resolution.size(), physical: value.resolution.physical_size(), density: value.resolution.scale_factor(), focused: value.focused, safe, ui_scale }
    }
    fn valid(self) -> bool {
        self.focused && self.logical.is_finite() && self.logical.min_element() > 0.0
            && self.physical.min_element() > 0 && self.density.is_finite() && self.density > 0.0
            && self.ui_scale.is_finite() && self.ui_scale > 0.0
            && (self.logical / self.ui_scale).is_finite() && (ROW / self.ui_scale).is_finite()
            && [self.safe.left, self.safe.top, self.safe.right, self.safe.bottom].iter().all(|v| v.is_finite() && *v >= 0.0)
            && self.logical.x - self.safe.left - self.safe.right >= 280.0
            && self.logical.y - self.safe.top - self.safe.bottom >= 240.0
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Source {
    Repair(player_repair::Session),
    Mod(item_mod::Session),
    Vendor(fnv_sim::FormId, crate::repair::View),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind { Repair, Mod, Vendor }
/// Evaluated inside each existing consumer, using its actual ScriptState borrow.
/// An absent runtime/menu preserves the existing no-script hand-built path.
pub(crate) fn menu_allows(kind: Kind, game: Option<&fnv_game::Game<'_>>) -> bool {
    use fnv_game::game::menus;
    game.is_none_or(|g| {
        !g.loading && !matches!(g.side_menu(), Some(menus::LOADING | menus::VATS))
            && g.open_menu_code().is_none_or(|code| match kind {
                Kind::Repair => matches!(code, menus::INVENTORY | menus::REPAIR),
                Kind::Mod => matches!(code, menus::INVENTORY | menus::ITEM_MOD),
                Kind::Vendor => code == fnv_game::repair::REPAIR_SERVICES_MENU,
            })
    })
}
impl Source {
    fn same_scroll_source(&self, next: &Self) -> bool {
        match (self, next) {
            (Self::Repair(a), Self::Repair(b)) => a.offer == b.offer && a.skill == b.skill && a.settings == b.settings && a.selected == b.selected && a.preview == b.preview,
            (Self::Mod(a), Self::Mod(b)) => a.offer == b.offer && a.selected == b.selected,
            (Self::Vendor(id, a), Self::Vendor(other, b)) => {
                let mut compare = b.clone();
                compare.first = a.first;
                id == other && a == &compare
            }
            _ => false,
        }
    }
    fn kind(&self) -> Kind {
        match self { Self::Repair(_) => Kind::Repair, Self::Mod(_) => Kind::Mod, Self::Vendor(..) => Kind::Vendor }
    }
    fn first(&self) -> usize {
        match self { Self::Repair(s) => s.first, Self::Mod(s) => s.first, Self::Vendor(_, v) => v.first }
    }
    fn rows(&self) -> usize {
        match self { Self::Repair(s) => s.offer.parts.len() + 1, Self::Mod(s) => s.offer.choices.len() + 1, Self::Vendor(_, v) => v.rows.len() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action { Select(usize), Confirm, Cancel, Scroll(i32) }
#[derive(Clone)]
pub(crate) struct Request {
    source: Arc<Source>,
    generation: u64,
    context: Context,
    id: u64,
    pub action: Action,
}
impl Request {
    pub(crate) fn repair(&self, session: Option<&player_repair::Session>) -> bool {
        matches!(self.source.as_ref(), Source::Repair(s) if Some(s) == session)
    }
    pub(crate) fn modding(&self, session: Option<&item_mod::Session>) -> bool {
        matches!(self.source.as_ref(), Source::Mod(s) if Some(s) == session)
    }
    pub(crate) fn vendor(&self, vendor: fnv_sim::FormId, view: &crate::repair::View) -> bool {
        matches!(self.source.as_ref(), Source::Vendor(id, v) if *id == vendor && v == view)
    }
}

#[derive(Clone)]
struct Contact {
    id: u64,
    origin: Vec2,
    at: Vec2,
    source: Arc<Source>,
    target: Option<Action>,
    dragging: bool,
    quarantined: bool,
    remainder: f32,
}

/// Absent on legacy desktop/headless paths; no synthetic keyboard or mouse state.
#[derive(Resource)]
pub struct State {
    enabled: bool,
    source: Option<Arc<Source>>,
    context: Option<Context>,
    allowed: bool,
    hardware_hidden: bool,
    contact: Option<Contact>,
    retired: Vec<u64>,
    queue: VecDeque<Request>,
    generation: u64,
    painted: Option<u64>,
}
impl Default for State {
    fn default() -> Self {
        Self::for_platform(cfg!(target_os = "android"))
    }
}
impl State {
    /// Saved presentation preference, independent of transient input admission.
    pub fn enabled(&self) -> bool { self.enabled }
    fn for_platform(android: bool) -> Self {
        Self { enabled: android, source: None, context: None, allowed: false, hardware_hidden: false, contact: None, retired: Vec::new(), queue: VecDeque::new(), generation: 0, painted: None }
    }
    pub(crate) fn visible(&self) -> bool { self.enabled && self.allowed && !self.hardware_hidden && self.source.is_some() && self.context.is_some() }
    fn retire(&mut self) {
        if let Some(c) = self.contact.take() {
            if !self.retired.contains(&c.id) { self.retired.push(c.id); }
        }
        self.queue.clear();
        self.generation = self.generation.wrapping_add(1);
    }
    fn set_source(&mut self, next: Option<Source>) {
        if self.source.as_deref() == next.as_ref() { return; }
        let same_kind = self.source.as_ref().zip(next.as_ref()).is_some_and(|(old, new)| old.kind() == new.kind());
        // A scroll is allowed to keep its finger while the actual first row advances.
        let scrolling = self.source.as_ref().zip(next.as_ref()).is_some_and(|(a, b)| a.same_scroll_source(b)) && self.contact.as_ref().is_some_and(|c| c.dragging);
        let old_contact = scrolling.then(|| self.contact.take()).flatten();
        self.retire();
        self.source = next.map(Arc::new);
        if !same_kind { self.hardware_hidden = false; }
        if let Some(mut contact) = old_contact {
            if let Some(source) = &self.source { contact.source = source.clone(); self.contact = Some(contact); }
        }
    }
    fn enqueue(&mut self, source: Arc<Source>, id: u64, action: Action) {
        let Some(context) = self.context else { return };
        if let Action::Scroll(by) = action {
            if let Some(last) = self.queue.back_mut().filter(|r| r.id == id && r.generation == self.generation && r.source == source) {
                if let Action::Scroll(previous) = &mut last.action { *previous = previous.saturating_add(by); return; }
            }
        }
        if self.queue.len() < QUEUED {
            self.queue.push_back(Request { source, generation: self.generation, context, id, action });
        } else {
            // Bounded refusal; never retain a partial gesture for a later menu.
            self.retire();
        }
    }
}

/// Leaf install; shared wrapper adds this plugin once and supplies the router hooks.
pub struct MaintenanceTouchPlugin;
impl Plugin for MaintenanceTouchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<State>().init_resource::<sheet::Drawn>().add_systems(PostUpdate, (publish, sheet::draw).chain().before(bevy::ui::UiSystems::Layout));
        crate::settings::add_setting(app, crate::settings::Setting {
            key: SETTING_KEY, group: crate::pause::GROUP_MENUS, label: "Touch repair, mods and workbenches",
            note: "Finger taps and list dragging in ordinary vendor repair/workbenches; Pip-Boy donor/mod lists keep their touch UI. Turn off for ordinary controls; on by default on Android.",
            switches: "", get: |w| w.resource::<State>().enabled,
            set: |w, enabled| {
                let changed={
                    let mut state=w.resource_mut::<State>();
                    let changed=state.enabled!=enabled;
                    if changed {state.retire();state.enabled=enabled;state.hardware_hidden=false;}
                    changed
                };
                if changed {
                    if let Some(mut normal)=w.get_resource_mut::<crate::menu::normal_services::NormalServices>() {normal.cancel();}
                }
            }, slider: None,
        });
    }
}

fn publish(
    mut state: ResMut<State>,
    repair: Option<Res<player_repair::PipRepair>>,
    modding: Option<Res<item_mod::PipItemMod>>,
    vendor: Option<Res<crate::repair::RepairServices>>,
) {
    // Compare borrowed live models first: no per-frame list clone in steady state.
    let next = if let Some((id, v)) = vendor.as_ref().and_then(|v| v.0.as_ref()).and_then(|v| v.touch_view()) {
        if matches!(state.source.as_deref(), Some(Source::Vendor(old_id, old)) if *old_id == id && old == v) { return; }
        Some(Source::Vendor(id, v.clone()))
    } else if let Some(s) = repair.as_ref().and_then(|r| r.0.as_ref()) {
        if matches!(state.source.as_deref(), Some(Source::Repair(old)) if old == s) { return; }
        Some(Source::Repair(s.clone()))
    } else if let Some(s) = modding.as_ref().and_then(|m| m.0.as_ref()) {
        if matches!(state.source.as_deref(), Some(Source::Mod(old)) if old == s) { return; }
        Some(Source::Mod(s.clone()))
    } else { None };
    state.set_source(next);
}

/// One common raw reader calls this bridge; it has no independent device readers.
#[derive(SystemParam)]
pub struct Bridge<'w> { state: ResMut<'w, State> }
impl Bridge<'_> {
    pub fn enabled(&self) -> bool { self.state.enabled() }
    pub fn prepare(&mut self, context: Option<Context>, allowed: bool) {
        let context = context.filter(|c| c.valid());
        let s = &mut self.state;
        if s.context != context || s.allowed != allowed { s.retire(); }
        s.context = context;
        s.allowed = allowed;
    }
    pub fn captures_touch(&self) -> bool { self.state.visible() }
    /// Includes hidden presentation, so the common router can offer action-free return.
    pub fn available(&self) -> bool { self.state.enabled && self.state.allowed && self.state.context.is_some() && self.state.source.is_some() }
    /// Retired ownership survives source/modal/hardware cancellation until real neutral.
    pub fn owns(&self, id: u64) -> bool { self.state.retired.contains(&id) || self.state.contact.as_ref().is_some_and(|c| c.id == id) }
    /// Parent's actual Touches witness: no held, pressed, release or cancel-edge
    /// contact remains. A refused release frame must stay captured until this.
    pub fn observe_neutral(&mut self, neutral: bool) {
        if neutral { self.state.retired.clear(); }
    }
    pub fn hardware_intent(&mut self) { self.state.retire(); self.state.hardware_hidden = true; }
    pub fn restore_touch(&mut self) { self.state.retire(); self.state.hardware_hidden = false; }
    pub fn cancel(&mut self) { self.state.retire(); self.state.allowed = false; }
    /// `admitted` must include the shared modal/platform/per-ID gate. Blank sheet
    /// space is intentionally owned; a hidden-sheet return never selects a row.
    pub fn event(&mut self, event: &TouchInput, admitted: bool) -> bool {
        let s = &mut self.state;
        if s.retired.contains(&event.id) {
            if matches!(event.phase, TouchPhase::Ended | TouchPhase::Canceled) { s.retired.retain(|id| *id != event.id); }
            return true;
        }
        if !admitted || !s.enabled || !s.allowed || s.context.is_none() || s.source.is_none() {
            let owned = s.contact.as_ref().is_some_and(|c| c.id == event.id);
            s.retire();
            if matches!(event.phase, TouchPhase::Ended | TouchPhase::Canceled) { s.retired.retain(|id| *id != event.id); }
            return owned;
        }
        let context = s.context.unwrap();
        if event.window != context.window { return false; }
        if !event.position.is_finite() { s.retire(); return true; }
        if event.phase == TouchPhase::Started {
            if s.contact.is_some() || s.retired.len() >= 16 { return true; }
            let quarantined = s.hardware_hidden || s.painted != Some(s.generation);
            s.hardware_hidden = false;
            let source = s.source.as_ref().unwrap().clone();
            let target = sheet::Geometry::new(context).target(event.position, &source);
            s.contact = Some(Contact { id: event.id, origin: event.position, at: event.position, source, target, dragging: false, quarantined, remainder: 0.0 });
            return true;
        }
        let Some(mut c) = s.contact.take() else { return s.visible() };
        if c.id != event.id { s.contact = Some(c); return true; }
        if c.quarantined {
            if !matches!(event.phase, TouchPhase::Ended | TouchPhase::Canceled) { s.contact = Some(c); }
            return true;
        }
        match (event.phase, c.target) {
            (TouchPhase::Moved, _) => {
                let geometry = sheet::Geometry::new(context);
                if (event.position - c.origin).length() > DRAG { c.dragging = true; }
                if c.dragging && matches!(c.target, Some(Action::Select(_))) {
                    c.remainder += c.at.y - event.position.y;
                    let rows = (c.remainder / ROW).trunc() as i32;
                    if rows != 0 {
                        c.remainder -= rows as f32 * ROW;
                        s.enqueue(c.source.clone(), c.id, Action::Scroll(rows));
                    }
                }
                if !geometry.list.contains(c.origin) { c.target = None; }
                c.at = event.position;
                s.contact = Some(c);
            }
            (TouchPhase::Ended, Some(target)) if !c.dragging && sheet::Geometry::new(context).target(event.position, &c.source) == Some(target) => s.enqueue(c.source, c.id, target),
            _ => {}
        }
        true
    }
}

/// Optional in the existing consumers: standalone legacy Apps need no mobile resources.
#[derive(SystemParam)]
pub struct Requests<'w, 's> {
    state: Option<ResMut<'w, State>>,
    gate: MaintenanceAdmission<'w>,
    windows: Query<'w, 's, (&'static Window, Has<bevy::window::PrimaryWindow>)>,
    cameras: Query<'w, 's, (&'static Camera, &'static bevy::camera::RenderTarget)>,
    scale: Option<Res<'w, UiScale>>,
    safe: Option<Res<'w, SafeInsets>>,
}
impl Requests<'_, '_> {
    pub(crate) fn enabled(&self)->bool {self.state.as_ref().is_some_and(|s|s.enabled)}
    pub(crate) fn take(&mut self, kind: Kind) -> Vec<Request> {
        let Some(s) = self.state.as_mut() else { return Vec::new() };
        let mut mine = Vec::new();
        let mut other = VecDeque::new();
        while let Some(r) = s.queue.pop_front() {
            if r.source.kind() != kind { other.push_back(r); continue; }
            let context_valid = self.windows.get(r.context.window).is_ok_and(|(w, primary)| {
                primary && w.focused && Context::from_window(r.context.window, r.context.camera, w, self.safe.as_ref().map_or(SafeInsets::default(), |v| **v), self.scale.as_ref().map_or(1.0, |v| v.0)) == r.context
            }) && self.cameras.get(r.context.camera).is_ok_and(|(c, target)| {
                c.is_active && c.viewport.as_ref().is_none_or(|v| v.physical_position == UVec2::ZERO && v.physical_size == r.context.physical)
                    && c.physical_target_size() == Some(r.context.physical)
                    && c.physical_viewport_rect() == Some(URect::from_corners(UVec2::ZERO, r.context.physical))
                    && c.logical_viewport_rect().is_some_and(|rect| rect.size() == r.context.logical)
                    && matches!(target.normalize(Some(r.context.window)), Some(bevy::camera::NormalizedRenderTarget::Window(w)) if w.entity() == r.context.window)
            });
            if s.visible() && r.generation == s.generation && s.context == Some(r.context)
                && context_valid && self.gate.allows(r.id) { mine.push(r); }
        }
        s.queue = other;
        mine
    }
}

/// Same platform/device/modal gates as generic menu touch, except its OverlayCapture:
/// that flag blocks the underlay while this sheet is the actual admitted owner.
#[derive(SystemParam)]
struct MaintenanceAdmission<'w> {
    bindings: Option<Res<'w, crate::bindings::Bindings>>,
    platform: Option<Res<'w, crate::pad::PlatformInputGate>>,
    switching: Option<Res<'w, crate::pad::TouchDeviceSwitch>>,
    keyboard: Option<Res<'w, crate::bindings::KeyboardCapture>>,
    free_camera: Option<Res<'w, crate::free_cam::FreeCam>>,
    fresh: Option<Res<'w, crate::menu::FreshMenu>>,
    paused: Option<Res<'w, crate::pause::Paused>>,
    creator: Option<Res<'w, crate::creator::CreatorMenu>>,
}
impl MaintenanceAdmission<'_> {
    fn allows(&self, id: u64) -> bool {
        self.bindings.as_ref().is_none_or(|b| b.device != crate::bindings::InputDevice::Controller)
            && self.platform.as_ref().is_none_or(|p| p.allows_actions() && p.allows_touch(id))
            && self.switching.as_ref().is_none_or(|s| s.allows_actions())
            && self.keyboard.as_ref().is_none_or(|k| !k.0)
            && self.free_camera.as_ref().is_none_or(|c| !c.active)
            && self.fresh.as_ref().is_none_or(|f| !f.0)
            && self.paused.as_ref().is_none_or(|p| !p.0)
            && self.creator.as_ref().is_none_or(|c| !c.is_open())
    }
}

pub(crate) fn scroll(first: usize, by: i32, rows: usize) -> usize {
    first.saturating_add_signed(by as isize).min(rows.saturating_sub(1))
}
