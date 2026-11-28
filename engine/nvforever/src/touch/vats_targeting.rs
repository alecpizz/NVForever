//! Adapter for the parent's sole raw stream; no reader, device writer or UI stack.
use super::*;
use bevy::{ecs::system::SystemParam, input::touch::Touches};
use nvf_play::vats_mode::touch::{Bridge as CoreBridge, Frame};

#[derive(Resource, Default)]
pub(super) struct Routing {
    primary: Option<Entity>,
    fire: Option<u64>,
    allowed: bool,
    hardware_busy: bool,
    return_batch: bool,
}

#[derive(SystemParam)]
pub(super) struct Bridge<'w, 's> {
    core: Option<CoreBridge<'w, 's>>,
    routing: ResMut<'w, Routing>,
    touches: Res<'w, Touches>,
    cameras: Query<'w, 's, Entity, With<nvf_play::PlayerCamera>>,
    fresh: Option<Res<'w, nvf_play::menu::FreshMenu>>,
}
impl Bridge<'_, '_> {
    pub(super) fn begin(&mut self, hardware_intent: bool, unread_empty: bool) {
        self.routing.allowed = false;
        self.routing.primary = None;
        self.routing.return_batch = false;
        if let Some(core) = self.core.as_mut() {
            let neutral = unread_empty && self.touches.iter().next().is_none()
                && !self.touches.any_just_released() && !self.touches.any_just_canceled();
            core.observe_neutral(neutral);
            if hardware_intent { core.hardware_intent(); }
        }
    }
    /// One call before the raw loop. Empty means unread reader.is_empty(), not a
    /// fabricated neutral sample; Touches still describes the preceding frame.
    pub(super) fn prepare(&mut self, primary: Entity, window: &Window,
        allowed: bool, unread_empty: bool, hardware_intent: bool, hardware_busy: bool) {
        self.routing.primary = Some(primary);
        self.routing.allowed = allowed;
        self.routing.hardware_busy = hardware_busy || hardware_intent;
        self.routing.return_batch = false;
        let Some(core) = self.core.as_mut() else { return; };
        if hardware_intent { core.hardware_intent(); }
        let neutral = unread_empty && self.touches.iter().next().is_none()
            && !self.touches.any_just_released() && !self.touches.any_just_canceled();
        core.observe_neutral(neutral);
        let frame = self.cameras.single().ok().map(|camera| Frame {
            window: primary, camera, logical: window.resolution.size(),
            physical: window.resolution.physical_size(), density: window.scale_factor(),
            admitted: allowed && !hardware_intent && !hardware_busy,
            fresh: self.fresh.as_ref().is_some_and(|f| f.0), neutral,
        });
        core.prepare(frame);
        if self.routing.fire != core.active_contact() { self.routing.fire = None; }
    }
    pub(super) fn cancel(&mut self) {
        self.routing.allowed = false;
        self.routing.fire = None;
        if let Some(core) = self.core.as_mut() { core.cancel(); }
    }
    pub(super) fn owns(&self, id: u64) -> bool {
        self.core.as_ref().is_some_and(|core| core.owns(id))
    }
    /// Called for actual controller-return events before the old early return.
    /// Every ID in that batch is retired; no invented Started or neutral input.
    pub(super) fn consume_return(&mut self, event: &TouchInput, primary: Entity) {
        if let Some(core) = self.core.as_mut() { core.quarantine_touch(event, primary); }
    }
    /// Runs before new generic routes. A hidden presentation consumes the first
    /// real contact even if the aggregate device already says KeyboardMouse.
    pub(super) fn returning(&mut self, event: &TouchInput) -> bool {
        let Some(primary) = self.routing.primary else { return false; };
        let Some(core) = self.core.as_mut() else { return false; };
        if !core.admits_routed(event.id) && !core.owns(event.id) { return false; }
        if self.routing.return_batch || self.routing.hardware_busy {
            return core.quarantine_touch(event, primary);
        }
        if self.routing.allowed && core.consume_touch_return(event, primary) {
            self.routing.return_batch = true;
            return true;
        }
        false
    }
    /// Owned IDs continue before new painted routes. New body starts require
    /// unclaimed=true; actual Fire is identified by the parent's current layout.
    pub(super) fn event(&mut self, event: &TouchInput, fire_hit: bool, unclaimed: bool) -> bool {
        let Some(primary) = self.routing.primary.filter(|p| *p == event.window) else { return false; };
        let Some(core) = self.core.as_mut() else { return false; };
        let owned = core.owns(event.id);
        if !core.admits_routed(event.id) { core.cancel(); return owned; }
        // Unknown terminal/move edges cannot cancel or steal the real owner.
        if !owned && event.phase != TouchPhase::Started { return false; }
        if event.phase == TouchPhase::Started
            && core.active_contact().is_some_and(|id| id != event.id) {
            self.routing.fire = None;
            return core.quarantine_touch(event, primary);
        }
        if !owned && !(self.routing.allowed && (fire_hit || unclaimed)) { return false; }
        if !owned && !core.available() {
            // Enabled Classic marking with missing/stale projection must not
            // fall through to generic Fire/look. Off and other modes return false.
            return core.quarantine_touch(event, primary);
        }
        let fire = self.routing.fire == Some(event.id)
            || (!owned && fire_hit && event.phase == TouchPhase::Started);
        let claimed = if fire { self.routing.fire = Some(event.id); core.fire(event) }
            else { core.event(event) };
        claimed || core.quarantine_touch(event, primary)
    }
    /// Called only by menu_sheets replay after the current selector. That sole
    /// publisher uses owns() in its existing actual-Touches loop after begin_frame.
    pub(super) fn after_switch(&mut self, hardware_intent: bool, consumed_return: bool) {
        let Some(core) = self.core.as_mut() else { return; };
        if hardware_intent { core.hardware_intent(); }
        else if consumed_return { core.restore_touch(); }
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Routing>();
}
