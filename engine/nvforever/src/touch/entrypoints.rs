//! Settings owns the editor's entry and return path; no persistent HUD shortcut.
use super::{layout::Layout, TouchInputSystems};
use bevy::{input::{keyboard::KeyboardInput, ButtonState}, prelude::*, window::PrimaryWindow};
use nvf_play::{pause::{FormOver, Page, Paused}, settings::{self, ChoiceList, Setting}, PlayState};

pub(super) const KEY: &str = "touch-layout-editor-v1";
const OPEN: &str = "Customize layout";
const READY: &str = "Ready";

#[derive(Resource, Default)]
pub(super) struct Session {
    active: bool,
    opening: bool,
    draining: u8,
    blocking: bool,
    owns_form: bool,
    viewport: Option<(Entity, Vec2)>,
    ids: [Option<(u64, bool)>; 32],
    overflow: bool,
}
impl Session {
    pub fn blocking(&self) -> bool { self.blocking || self.active || self.draining > 0 }
    pub fn window(&self) -> Option<Entity> { self.viewport.map(|(window, _)| window) }
    pub fn record(&mut self, event: &super::TouchInput) {
        if self.window() != Some(event.window) { return; }
        let existing = self.ids.iter().position(|slot| slot.is_some_and(|(id, _)| id == event.id));
        if !self.blocking() && existing.is_none() { return; }
        let slot = existing.or_else(|| self.ids.iter().position(Option::is_none));
        if let Some(slot) = slot {
            self.ids[slot] = Some((event.id, matches!(event.phase, super::TouchPhase::Ended | super::TouchPhase::Canceled)));
        } else { self.overflow = true; }
    }
    pub fn publish(&self, ownership: &mut nvf_play::ui_pointer::TouchUiCapture) {
        for (id, _) in self.ids.iter().flatten() { ownership.block(*id); }
        if self.overflow { ownership.quarantine_overflow(); }
    }
    pub fn observe_neutral(&mut self, neutral: bool) {
        if neutral && !self.blocking() { self.ids.fill(None); self.overflow = false; }
    }
    /// The sole raw reader drops entry and closing batches before editor routing.
    pub fn discard_batch(&mut self) -> bool {
        let discard = self.opening || !self.active;
        self.opening = false;
        discard && self.blocking()
    }
    pub fn closed(&mut self) {
        if self.active { self.active = false; self.draining = 2; self.blocking = true; }
    }
    pub fn interrupt(&mut self, layout: &mut Layout) {
        if self.active { layout.cancel_editor(); self.closed(); }
    }
}
fn unavailable(world: &World) -> Option<&'static str> {
    if world.resource::<Session>().viewport.is_some_and(|(_, size)| size.x >= 400. && size.y >= 300.) {
        None
    } else { Some("Rotate the device or enlarge the window. The editor needs at least 400 x 300 screen units.") }
}
fn open(world: &mut World, name: &str) -> bool {
    if name != OPEN || unavailable(world).is_some()
        || world.resource::<Session>().blocking()
        || world.resource::<Layout>().editor.is_some()
        || !world.get_resource::<Paused>().is_some_and(|paused| paused.0)
        || !world.get_resource::<State<PlayState>>().is_some_and(|state| *state.get() == PlayState::Menu)
        || !world.get_resource::<Page>().is_some_and(|page| matches!(page, Page::Settings { .. }))
        || world.resource::<FormOver>().0 { return false; }
    world.resource_mut::<Layout>().open(true);
    world.resource_mut::<Layout>().message =
        "Drag a control to move it. Save keeps changes; Cancel returns to Settings.".into();
    let mut session = world.resource_mut::<Session>();
    session.active = true; session.opening = true; session.blocking = true; session.owns_form = true;
    world.resource_mut::<FormOver>().0 = true;
    true
}
#[derive(bevy::ecs::system::SystemParam)]
struct EditorContext<'w> {
    paused: Option<Res<'w, Paused>>,
    page: Option<Res<'w, Page>>,
    play: Option<Res<'w, State<PlayState>>>,
    next: Option<Res<'w, NextState<PlayState>>>,
}
fn prepare(
    mut session: ResMut<Session>, mut layout: ResMut<Layout>, mut over: ResMut<FormOver>,
    mut keys: MessageReader<KeyboardInput>, window: Query<(Entity, &Window), With<PrimaryWindow>>,
    EditorContext { paused, page, play, next }: EditorContext,
) {
    for slot in &mut session.ids { if slot.is_some_and(|(_, ended)| ended) { *slot = None; } }
    let escape = keys.read().any(|key| Some(key.window) == session.window()
        && key.key_code == KeyCode::Escape && key.state == ButtonState::Pressed && !key.repeat);
    let viewport = window.single().ok().map(|(entity, window)| (entity, window.resolution.size()));
    let effective = match next.as_deref() {
        Some(NextState::Pending(state) | NextState::PendingIfDifferent(state)) => Some(*state),
        _ => play.as_deref().map(|state| *state.get()),
    };
    if session.active {
        let valid = viewport == session.viewport && window.single().is_ok_and(|(_, window)| window.focused)
            && effective == Some(PlayState::Menu) && paused.as_ref().is_some_and(|paused| paused.0)
            && page.as_deref().is_some_and(|page| matches!(page, Page::Settings { .. }));
        if escape || !valid { layout.cancel_editor(); session.closed(); }
        else if layout.editor.is_none() { session.closed(); }
    }
    session.blocking = session.active || session.draining > 0;
    session.draining = session.draining.saturating_sub(1);
    if !session.blocking && session.owns_form { over.0 = false; session.owns_form = false; }
    if !session.blocking && session.ids.iter().all(Option::is_none) { session.viewport = viewport; }
}
pub(super) fn install(app: &mut App) {
    app.init_resource::<Session>().init_resource::<FormOver>()
        .add_systems(PreUpdate, prepare.before(TouchInputSystems));
    settings::add_choice_list(app, Setting {
        key: KEY, group: "Controls", label: OPEN,
        note: "Move or resize touch controls, change opacity, aiming and profiles. Save keeps changes; Cancel returns here without resuming the game.",
        switches: "", get: |_| false, set: |_, _| {}, slider: None,
    }, ChoiceList { names: |_| vec![READY.into(), OPEN.into()], chosen: |_| READY.into(),
        pick: open, save: |_| READY.into(), load: |_, name| name == READY, ask_keep: false });
    settings::set_choice_action(app, KEY, |name| name == OPEN);
    settings::set_disabled_status(app, KEY, unavailable);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_mask_is_bounded_window_owned_and_keeps_the_release_frame() {
        let mut world = World::new();
        let window = world.spawn_empty().id();
        let foreign = world.spawn_empty().id();
        let mut session = Session { active: true, viewport: Some((window, Vec2::new(800., 400.))), ..default() };
        let mut event = super::super::TouchInput { window, id: 0, phase: super::super::TouchPhase::Started,
            position: Vec2::ONE, force: None };
        event.window = foreign; session.record(&event);
        assert!(session.ids.iter().all(Option::is_none));
        event.window = window;
        for id in 0..33 { event.id = id; session.record(&event); }
        assert_eq!(session.ids.iter().flatten().count(), 32);
        assert!(session.overflow);
        session.closed();
        let mut mask = nvf_play::ui_pointer::TouchUiCapture::default();
        session.publish(&mut mask);
        assert!(!mask.allows(99), "overflow stays quarantined through the closing drain");
        session.observe_neutral(true);
        assert!(session.overflow, "the form's active drain still owns its mask");
        session.blocking = false; session.draining = 0;
        session.observe_neutral(true);
        mask.begin_frame(); session.publish(&mut mask);
        assert!(mask.allows(99));
        session.active = true; event.id = 70; event.phase = super::super::TouchPhase::Ended;
        session.record(&event); session.publish(&mut mask);
        assert!(!mask.allows(70), "an End in the closing batch is masked through UI focus");
    }
}
