//! Historical alternate barter presentation and the shared late admission policy.
//! Production touch routing uses menu::normal_touch over the ordinary presentation;
//! this module's original alternate-presentation tests remain historical coverage.
use super::{model::{self, Category, Command, MenuModel, Mode, RowGuard, RowKey, Side, Units}, ActiveMenu, MenuInput};
use bevy::{camera::RenderTarget, ecs::{entity::ContainsEntity, system::SystemParam}, input::touch::{TouchInput, TouchPhase}, prelude::*, window::PrimaryWindow};
use fnv_sim::FormId;
use std::collections::BTreeMap;

mod view;
mod commit;
pub(crate) use commit::Commits;
#[cfg(test)]
pub(crate) mod tests;

pub const KEY: &str = "touch-barter-v1";
const TRAVEL: f32 = 12.0;
const MAX_IDS: usize = 32;
const PLAYER: FormId = FormId::PLAYER_REF;

#[derive(Resource)]
#[cfg_attr(not(target_os = "android"), derive(Default))]
pub struct Options { pub enabled: bool }
#[cfg(target_os = "android")]
impl Default for Options { fn default() -> Self { Self { enabled: true } } }

/// Supplied by the sole raw-input owner. Camera is an actual full-window UI target;
/// neutral means no held *or release/cancel-edge* touch remains in that window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub window: Entity,
    pub camera: Entity,
    pub size: Vec2,
    pub safe: crate::touch_safe_area::SafeInsets,
    pub ui_scale: f32,
    pub density: f32,
    pub admitted: bool,
    pub fresh: bool,
    pub neutral: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Session { merchant: FormId, stock: FormId, caps: FormId }
fn session(m: &MenuModel) -> Option<Session> {
    let Mode::Barter(merchant) = m.mode else { return None };
    let trade = m.trade.as_ref()?;
    Some(Session { merchant, stock: trade.stock, caps: trade.caps })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RowToken {
    side: Side, index: usize, key: RowKey, count: i32, bartered: bool, price: Option<u32>,
}
impl RowToken {
    fn at(m: &MenuModel, side: Side, index: usize) -> Option<Self> {
        let row = m.list(side).rows.get(index)?;
        Some(Self { side, index, key: RowKey::at(&m.list(side).rows, index)?, count: row.count,
            bartered: row.bartered, price: row.price.map(f32::to_bits) })
    }
    fn same(self, m: &MenuModel) -> bool { Self::at(m, self.side, self.index) == Some(self) }
    fn guard(self, m: &MenuModel, selected: bool) -> RowGuard {
        RowGuard { mode: m.mode, side: self.side, index: self.index, key: self.key, selected, then: 1 }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Prompt { cmd: Command, side: Side, row: usize, max: i32, key: RowKey }
fn prompt(m: &MenuModel) -> Option<Prompt> {
    let q = m.quantity?;
    Some(Prompt { cmd: q.cmd, side: q.side, row: q.row, max: q.max,
        key: RowKey::at(&m.list(q.side).rows, q.row)? })
}
#[derive(Clone, Debug, PartialEq)]
struct Quote {
    session: Session, offered: [BTreeMap<Units, i32>; 2], purse: [i32; 2], balance: i64, mults: [u32; 2],
}
impl Quote {
    fn of(m: &MenuModel) -> Option<Self> {
        let t = m.trade.as_ref()?;
        Some(Self { session: session(m)?, offered: t.offered.clone(), purse: t.purse,
            balance: t.balance, mults: t.mults().map(f32::to_bits) })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target { Row(RowToken), Category(Side, i32), One, All, Accept, Close,
    Step(i32), Min, Max, Meter, ConfirmAmount, CancelAmount }
#[derive(Clone, Copy, Debug)]
struct Hit { rect: Rect, target: Target }
#[derive(Clone, Copy, Debug, PartialEq)]
struct Identity { session: Session, window: Entity, camera: Entity, size: Vec2,
    safe: crate::touch_safe_area::SafeInsets, ui_scale: f32, density: f32, category: [Category; 2], prompt: Option<Prompt> }
#[derive(Clone)]
struct Contact { id: u64, origin: Vec2, previous: Vec2, target: Option<Target>, identity: Identity,
    dragged: bool, scroll: bool, remainder: f32, quote: Option<Quote>, selection: Option<RowToken>, amount: Option<i32> }
#[derive(Clone)]
enum Action { Select(RowToken), Scroll(Side, i32), Category(Side, i32), Stage(RowToken, bool),
    Accept(Quote), Close, Amount(Prompt, Command, Option<i32>) }
struct Pending { id: u64, identity: Identity, action: Action }

#[derive(Resource)]
pub struct TouchBarter {
    frame: Option<Frame>, identity: Option<Identity>, touch_owner: bool,
    contact: Option<Contact>, pending: Option<Pending>, retired: Vec<u64>, overflow: bool,
    barrier: bool, close_drain: u8, first: [usize; 2], revision: u64,
}
impl Default for TouchBarter {
    fn default() -> Self { Self { frame: None, identity: None, touch_owner: true, contact: None,
        pending: None, retired: Vec::new(), overflow: false, barrier: false,
        close_drain: 0, first: [0; 2], revision: 0 } }
}
impl TouchBarter {
    fn retire(&mut self, id: u64) {
        if self.retired.contains(&id) { return; }
        if self.retired.len() == MAX_IDS {
            // Once IDs cannot be represented, abandon all owned proposals and
            // swallow the whole window until neutral; do not strand an owner.
            self.overflow = true; self.contact = None; self.pending = None;
            self.barrier = true; self.revision = self.revision.wrapping_add(1);
        }
        else { self.retired.push(id); }
    }
    fn cancel_contact(&mut self) {
        let changed = self.contact.is_some() || self.pending.is_some();
        if let Some(c) = self.contact.take() { self.retire(c.id); }
        if let Some(p) = self.pending.take() { self.retire(p.id); }
        self.barrier = true;
        if changed { self.revision = self.revision.wrapping_add(1); }
    }
    /// Full cancellation changes no hardware state and never releases a held touch
    /// into the old presentation. The parent supplies real neutral observations.
    pub fn cancel(&mut self) {
        self.cancel_contact();
        if self.frame.take().is_some() { self.revision = self.revision.wrapping_add(1); }
    }
    /// Called only for actual hardware provenance, never synthesized menu keys.
    pub fn hardware_intent(&mut self) { self.cancel(); self.touch_owner = false; }
    /// Also used by the already-consumed controller-to-touch selector edge.
    pub fn restore_touch(&mut self) {
        self.cancel_contact();
        if !self.touch_owner { self.revision = self.revision.wrapping_add(1); }
        self.touch_owner = true;
    }
    pub fn visible(&self) -> bool { self.frame.is_some_and(|f| f.admitted) && self.touch_owner }
    /// Scope the original UI's Touches filter to this sheet, its retired IDs and
    /// its one closing frame. It must not suppress physical mouse/keyboard/pad.
    pub fn captures_touch(&self, id: u64) -> bool {
        self.visible() || self.close_drain > 0 || self.overflow || self.retired.contains(&id)
    }
    pub fn committed_this_batch(&self) -> bool { self.barrier && self.pending.is_some() }
    pub fn prepare(&mut self, options: &Options, m: Option<&MenuModel>, frame: Frame) {
        let previous = (self.frame, self.first);
        self.barrier = false;
        self.close_drain = self.close_drain.saturating_sub(1);
        if frame.neutral { self.retired.clear(); self.overflow = false; }
        let pane = view::Panel::of(frame);
        let next = m.filter(|_| options.enabled).and_then(|m| Some(Identity {
            session: session(m)?, window: frame.window, camera: frame.camera, size: frame.size,
            safe: frame.safe, ui_scale: frame.ui_scale, density: frame.density, category: m.category, prompt: prompt(m),
        }));
        if self.identity != next || !frame.admitted || frame.fresh || pane.is_none() {
            self.cancel_contact();
            if self.identity.is_some() && next.is_none() { self.close_drain = 1; }
        }
        if self.identity.map(|i| i.session) != next.map(|i| i.session) { self.first = [0; 2]; }
        self.identity = next;
        self.frame = next.filter(|_| frame.admitted && !frame.fresh && pane.is_some()).map(|_| frame);
        if let (Some(m), Some(p)) = (m, pane) {
            for side in [Side::Player, Side::Container] {
                let i = side as usize;
                self.first[i] = model::keep_in_view(self.first[i], m.selected[i], m.list(side).rows.len(), p.rows);
            }
        }
        if previous != (self.frame, self.first) { self.revision = self.revision.wrapping_add(1); }
    }
    fn queue(&mut self, id: u64, identity: Identity, action: Action, final_edge: bool) {
        match (&mut self.pending, &action) {
            (Some(Pending { action: Action::Scroll(old_side, n), .. }), Action::Scroll(side, more)) if old_side == side => {
                *n = n.saturating_add(*more).clamp(-12, 12);
            }
            (Some(Pending { action: Action::Amount(old, command, _), .. }), Action::Amount(next, Command::Amount(n), _)) if old == next => {
                *command = Command::Amount(*n);
            }
            (None, _) => self.pending = Some(Pending { id, identity, action }),
            _ => {}
        }
        self.barrier |= final_edge;
        self.revision = self.revision.wrapping_add(1);
    }
    /// The sole raw reader calls this with admitted, window-matched edges. This
    /// resource owns no TouchInput MessageReader and synthesizes no mouse events.
    pub fn event(&mut self, event: &TouchInput, m: &MenuModel) -> bool {
        if self.retired.contains(&event.id) || self.overflow {
            // Retain release/cancel through picking later in this frame. Only an
            // actual neutral observation in prepare retires the quarantine.
            return true;
        }
        let (Some(frame), Some(identity)) = (self.frame, self.identity) else { return false };
        if event.window != frame.window { return false; }
        if !self.touch_owner && event.phase == TouchPhase::Started {
            self.restore_touch(); self.retire(event.id); return true;
        }
        if !self.visible() { return false; }
        if self.barrier {
            if event.phase == TouchPhase::Started { self.retire(event.id); }
            return true;
        }
        if !event.position.is_finite() { self.cancel_contact(); return true; }
        let Some(panel) = view::Panel::of(frame) else { self.cancel(); return true };
        if event.phase == TouchPhase::Started {
            if self.contact.is_some() {
                if self.contact.as_ref().is_some_and(|c| c.id == event.id) { self.cancel_contact(); }
                self.retire(event.id); return true;
            }
            let target = panel.hits(m, self.first).into_iter().find(|h| h.rect.contains(event.position)).map(|h| h.target);
            self.contact = Some(Contact { id: event.id, origin: event.position, previous: event.position,
                target, identity, dragged: false, scroll: false, remainder: 0.0,
                quote: (target == Some(Target::Accept)).then(|| Quote::of(m)).flatten(), selection: selected(m), amount: m.quantity.map(|q| q.amount) });
            self.revision = self.revision.wrapping_add(1);
            return true;
        }
        // Unknown fingers may not remove, commit or cancel the real owner.
        if !self.contact.as_ref().is_some_and(|c| c.id == event.id) { return true; }
        let mut c = self.contact.take().unwrap();
        if c.identity != identity || event.phase == TouchPhase::Canceled {
            self.pending = None; self.barrier = true; self.revision = self.revision.wrapping_add(1); return true;
        }
        c.dragged |= event.position.distance(c.origin) > TRAVEL;
        if matches!(c.target, Some(Target::Row(_))) && c.dragged {
            let delta = event.position - c.origin;
            c.scroll |= delta.y.abs() >= delta.x.abs();
        }
        if let Some(Target::Row(token)) = c.target.filter(|_| c.scroll) {
            c.remainder += c.previous.y - event.position.y;
            let lines = (c.remainder / 20.0).trunc().clamp(-12.0, 12.0) as i32;
            c.remainder -= lines as f32 * 20.0;
            if lines != 0 { self.queue(c.id, identity, Action::Scroll(token.side, lines), false); }
        } else if c.target == Some(Target::Meter) {
            if let (Some(q), Some(rect)) = (prompt(m), panel.meter) {
                let amount = model::amount_at((event.position.x - rect.min.x) / rect.width(), q.max);
                self.queue(c.id, identity, Action::Amount(q, Command::Amount(amount), None), false);
            }
        }
        c.previous = event.position;
        if event.phase == TouchPhase::Ended {
            if !c.dragged && c.target == panel.hits(m, self.first).into_iter().find(|h| h.rect.contains(event.position)).map(|h| h.target) {
                if let Some(action) = c.target.and_then(|target| action(target, m, c.quote, c.selection, c.amount)) {
                    self.queue(c.id, identity, action, true);
                }
            }
            self.barrier = true;
        } else { self.contact = Some(c); }
        self.revision = self.revision.wrapping_add(1);
        true
    }
}
fn selected(m: &MenuModel) -> Option<RowToken> { RowToken::at(m, m.focus, m.selected[m.focus as usize]) }
fn action(target: Target, m: &MenuModel, quote: Option<Quote>, selection: Option<RowToken>, amount: Option<i32>) -> Option<Action> {
    Some(match target {
        Target::Row(t) => Action::Select(t), Target::Category(s, n) => Action::Category(s, n),
        Target::One => Action::Stage(selection?, false), Target::All => Action::Stage(selection?, true),
        Target::Accept => Action::Accept(quote?), Target::Close => Action::Close,
        Target::Step(n) => Action::Amount(prompt(m)?, Command::AmountStep(n), None),
        Target::Min => Action::Amount(prompt(m)?, Command::Amount(1), None),
        Target::Max => Action::Amount(prompt(m)?, Command::Amount(i32::MAX), None),
        Target::ConfirmAmount => Action::Amount(prompt(m)?, Command::AmountOk, Some(amount?)),
        Target::CancelAmount => Action::Amount(prompt(m)?, Command::AmountCancel, None), Target::Meter => return None,
    })
}

#[derive(SystemParam)]
pub(super) struct LiveAdmission<'w, 's> {
    bindings: Option<Res<'w, crate::bindings::Bindings>>,
    switch: Option<Res<'w, crate::pad::TouchDeviceSwitch>>,
    platform: Option<Res<'w, crate::pad::PlatformInputGate>>,
    paused: Option<Res<'w, crate::pause::Paused>>,
    safe: Option<Res<'w, crate::touch_safe_area::SafeInsets>>,
    scale: Option<Res<'w, UiScale>>,
    fresh: Option<Res<'w, super::FreshMenu>>,
    capture: Option<Res<'w, crate::bindings::KeyboardCapture>>,
    free_cam: Option<Res<'w, crate::free_cam::FreeCam>>,
    creator: Option<Res<'w, crate::creator::CreatorMenu>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    cameras: Query<'w, 's, (&'static Camera, &'static RenderTarget)>,
}
impl LiveAdmission<'_, '_> {
    pub(super) fn creator_open(&self)->bool {self.creator.as_ref().is_some_and(|c|c.is_open())}
    pub(super) fn allows(&self, frame: Frame, id: u64) -> bool {
        self.allows_with_switch(frame,id,false)
    }
    pub(super) fn allows_consumed_return(&self,frame:Frame,id:u64)->bool {
        self.allows_with_switch(frame,id,true)
    }
    fn allows_with_switch(&self,frame:Frame,id:u64,consumed_return:bool)->bool {
        self.bindings.as_ref().is_none_or(|b| b.device == crate::bindings::InputDevice::KeyboardMouse)
            && (consumed_return || self.switch.as_ref().is_none_or(|s| s.allows_actions()))
            && self.platform.as_ref().is_none_or(|g| g.allows_actions() && g.allows_touch(id))
            && !self.paused.as_ref().is_some_and(|p| p.0)
            && !self.fresh.as_ref().is_some_and(|f| f.0)
            && !self.capture.as_ref().is_some_and(|c| c.0)
            && !self.free_cam.as_ref().is_some_and(|c| c.active)
            && self.safe.as_ref().map_or_else(Default::default, |s| **s) == frame.safe
            && self.scale.as_ref().map_or(1.0, |s| s.0) == frame.ui_scale
            && self.windows.get(frame.window).is_ok_and(|w| w.focused && w.resolution.size() == frame.size
                && w.scale_factor() == frame.density)
            && self.cameras.get(frame.camera).is_ok_and(|(camera, target)| camera.is_active
                && target.normalize(Some(frame.window)).is_some_and(|target| matches!(target,
                    bevy::camera::NormalizedRenderTarget::Window(w) if w.entity() == frame.window))
                && camera.logical_viewport_size().is_some_and(|size| size.abs_diff_eq(frame.size, 0.01))
                && self.windows.get(frame.window).is_ok_and(|w| camera.physical_viewport_rect()
                    == Some(URect::from_corners(UVec2::ZERO, UVec2::new(w.physical_width(), w.physical_height())))
                    && camera.physical_target_size() == Some(UVec2::new(w.physical_width(), w.physical_height()))))
    }
}
pub(super) fn menu_allows(script: Option<&crate::script::ScriptState>, creator: Option<&crate::creator::CreatorMenu>, m: &MenuModel) -> bool {
    game_allows(script.and_then(|s|s.rt.as_ref()).map(|rt|&rt.game),creator,m)
}
pub(super) fn game_allows(game:Option<&fnv_game::Game<'_>>,creator:Option<&crate::creator::CreatorMenu>,m:&MenuModel)->bool {
    if creator.is_some_and(crate::creator::CreatorMenu::is_open) { return false; }
    game.is_none_or(|game| {
        use fnv_game::game::menus;
        !game.loading && !matches!(game.side_menu(), Some(menus::LOADING | menus::VATS))
            && game.top_menu().is_none_or(|code| code == menus::BARTER
                || (m.quantity.is_some() && matches!(code, menus::QUANTITY | crate::script::MENU_CONTAINER | menus::INVENTORY)))
            // ui_prompt is the engine's own Quantity publication, not a top
            // menu. Its clearing can trail apply_input by one frame.
            && game.ui_prompt.is_none_or(|code| code == menus::QUANTITY)
    })
}
#[derive(SystemParam)]
struct Admission<'w,'s> {
    live: LiveAdmission<'w,'s>, script: Option<Res<'w,crate::script::ScriptState>>,
}
impl Admission<'_,'_> {
    fn allows(&self, frame:Frame, id:u64)->bool { self.live.allows(frame,id) }
    fn menu_allows(&self,m:&MenuModel)->bool {menu_allows(self.script.as_deref(),self.live.creator.as_deref(),m)}
}

/// Adapter for the parent's sole raw reader. No keyboard, mouse or touch reader
/// lives here: physical provenance and selector restoration have one shared owner.
#[derive(SystemParam)]
pub struct Bridge<'w, 's> {
    options: Res<'w, Options>, state: ResMut<'w, TouchBarter>,
    active: Option<Res<'w, ActiveMenu>>, admission: Admission<'w, 's>,
}
impl Bridge<'_, '_> {
    pub fn prepare(&mut self, frame: Frame) {
        self.state.prepare(&self.options, self.active.as_ref().and_then(|a| a.0.as_ref()), frame);
    }
    pub fn event(&mut self, event: &TouchInput) -> bool {
        let owned = self.state.captures_touch(event.id);
        if let Some(frame) = self.state.frame {
            if !self.admission.allows(frame, event.id) { self.state.cancel(); return owned; }
        }
        let Some(m) = self.active.as_ref().and_then(|a| a.0.as_ref()) else { return owned };
        if !self.admission.menu_allows(m) { self.state.cancel(); return owned; }
        self.state.event(event, m)
    }
    pub fn captures_touch(&self, id: u64) -> bool { self.state.captures_touch(id) }
    pub fn visible(&self) -> bool { self.state.visible() }
    pub fn committed_this_batch(&self) -> bool { self.state.committed_this_batch() }
    pub fn cancel(&mut self) { self.state.cancel(); }
    pub fn hardware_intent(&mut self) { self.state.hardware_intent(); }
    pub fn restore_touch(&mut self) { self.state.restore_touch(); }
}
fn available(world: &mut fnv_sim::World<'_>, m: &MenuModel, token: RowToken, amount: i32) -> bool {
    let Some(t) = m.trade.as_ref() else { return false };
    let Some(row) = m.list(token.side).rows.get(token.index).filter(|_| token.same(m)) else { return false };
    let source = if row.bartered { token.side.other() } else { token.side };
    let holder = if source == Side::Player { PLAYER } else { t.stock };
    let reserved = if row.bartered { 0 } else { t.offered[source as usize].get(&row.units()).copied().unwrap_or(0) };
    world.inventory_mut(holder).is_ok_and(|inventory| inventory.count_at(row.item, row.pick()) >= amount.saturating_add(reserved))
}
fn dispatch(mut state: ResMut<TouchBarter>, options: Res<Options>, mut active: ResMut<ActiveMenu>,
    mut sim: ResMut<crate::sim::SimState>, admission: Admission, mut guards: ResMut<commit::Guards>, mut out: MessageWriter<MenuInput>) {
    let Some(pending) = state.pending.take() else { return };
    let Some(frame) = state.frame.filter(|_| options.enabled && state.visible()) else { return };
    let Some(m) = active.0.as_mut().filter(|m| session(m) == Some(pending.identity.session)) else { return };
    if state.identity != Some(pending.identity) || m.category != pending.identity.category
        || prompt(m) != pending.identity.prompt || !admission.allows(frame, pending.id)
        || !admission.menu_allows(m) { state.cancel(); return; }
    let changed = |m: &mut MenuModel| { m.status = Some("Trade changed. Review the item and amount again.".into()); };
    let all = matches!(&pending.action, Action::Stage(_, true));
    let stage = matches!(&pending.action, Action::Stage(..));
    match pending.action {
        Action::Select(t) | Action::Stage(t, _) => {
            if m.quantity.is_some() || m.refresh(&mut sim.world).is_err() || !t.same(m)
                || (stage && !available(&mut sim.world, m, t, if all { t.count } else { 1 })) { changed(m); return; }
            out.write(MenuInput(Command::Expect(t.guard(m, stage))));
            let token = guards.arm(pending.id, frame, pending.identity, state.revision,
                commit::Expected::Row { row: t, stage: stage.then_some(all) });
            out.write(MenuInput(Command::CommitBarter(token)));
        }
        Action::Accept(quote) => {
            if m.quantity.is_some() || m.refresh(&mut sim.world).is_err() || Quote::of(m).as_ref() != Some(&quote) { changed(m); return; }
            let token = guards.arm(pending.id, frame, pending.identity, state.revision, commit::Expected::Accept(quote));
            out.write(MenuInput(Command::CommitBarter(token)));
        }
        Action::Scroll(side, by) if m.quantity.is_none() => {
            let token = guards.arm(pending.id, frame, pending.identity, state.revision, commit::Expected::Scroll(side, by));
            out.write(MenuInput(Command::CommitBarter(token)));
        }
        Action::Category(side, by) if m.quantity.is_none() => {
            let token = guards.arm(pending.id, frame, pending.identity, state.revision, commit::Expected::Command(Command::Category(side, by)));
            out.write(MenuInput(Command::CommitBarter(token)));
        }
        Action::Close => {
            let token = guards.arm(pending.id, frame, pending.identity, state.revision, commit::Expected::Command(Command::Close));
            out.write(MenuInput(Command::CommitBarter(token)));
        }
        Action::Amount(expected, cmd, amount) if prompt(m) == Some(expected) => {
            if amount.is_some_and(|amount| m.quantity.is_none_or(|q| q.amount != amount)) { changed(m); return; }
            if cmd == Command::AmountOk && !RowToken::at(m, expected.side, expected.row)
                .is_some_and(|row| available(&mut sim.world, m, row, m.quantity.unwrap().amount)) {
                m.quantity = None; let _ = m.refresh(&mut sim.world); changed(m); return;
            }
            let Some(row) = RowToken::at(m, expected.side, expected.row) else { return; };
            let token = guards.arm(pending.id, frame, pending.identity, state.revision,
                commit::Expected::Amount { prompt: expected, row, amount, command: cmd });
            out.write(MenuInput(Command::CommitBarter(token)));
        }
        _ => {}
    }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dispatch;
pub fn install(app: &mut App) {
    app.init_resource::<Options>().init_resource::<TouchBarter>().init_resource::<commit::Guards>()
        .add_message::<MenuInput>()
        .add_systems(Update, dispatch.in_set(Dispatch).before(super::MenuSystems).run_if(resource_exists::<crate::sim::SimState>))
        .add_systems(Update, view::draw.after(super::MenuSystems));
    #[cfg(any(target_os = "android", test))]
    register_settings(app);
}

/// Register the barter touch setting for Android and cross-crate touch tests.
pub fn register_settings(app: &mut App) {
    app.init_resource::<Options>().init_resource::<TouchBarter>();
    if app.world().get_resource::<crate::settings::Settings>().is_some_and(|s| s.index(KEY).is_some()) {
        return;
    }
    crate::settings::add_setting(app, crate::settings::Setting {
        key: KEY, group: crate::pause::GROUP_MENUS, label: "Touch in barter menus",
        note: "Use finger taps and list dragging in the ordinary buying and selling menus. Dragging browses without choosing an item. Mouse, keyboard and controller controls stay available.",
        switches: "", get: |w| w.resource::<Options>().enabled,
        set: |w, on| {
            let changed = {
                let mut options = w.resource_mut::<Options>();
                let changed = options.enabled != on;
                options.enabled = on;
                changed
            };
            if changed {
                w.resource_mut::<TouchBarter>().cancel();
                if let Some(mut normal) = w.get_resource_mut::<super::normal_touch::NormalBarter>() { normal.cancel(); }
            }
        }, slider: None,
    });
}
