//! Generic overlay capture for dedicated terminal, companion and dialogue touch screens.
use bevy::{ecs::system::SystemParam, prelude::*};
use nvf_play::touch_ui_options::{Feature, Retirements};

fn bit(feature: Feature) -> u8 {
    match feature { Feature::Terminal => 1, Feature::Companion => 2, Feature::Lockpick => 4, Feature::Dialogue => 8 }
}
#[derive(Clone, Copy)]
struct Contact { window: Entity, id: u64, features: u8, retired: bool, ended: bool }

#[derive(Resource, Default)]
pub(super) struct Capture {
    active: bool,
    draining: bool,
    features: u8,
    previous_features: u8,
    present: u8,
    previous_present: u8,
    primary: Option<Entity>,
    contacts: [Option<Contact>; 32],
    overflow: Option<Entity>,
}
impl Capture {
    pub(super) fn blocking(&self) -> bool {
        self.active || self.draining
    }

    pub(super) fn hides_overlay(&self) -> bool {
        self.blocking() && !self.dialogue_only()
    }

    fn dialogue_only(&self) -> bool {
        self.active && self.features == bit(Feature::Dialogue) && self.present == self.features
    }
    fn record(&mut self, event: &super::TouchInput, claimed: bool) -> bool {
        if self.primary != Some(event.window) { return false; }
        let index = self.contacts.iter().position(|slot| slot.is_some_and(|c| c.window == event.window && c.id == event.id));
        let retired = index.is_some_and(|i| self.contacts[i].unwrap().retired) || self.overflow == Some(event.window);
        let features = (self.features & self.present) | if self.draining { self.previous_features & self.previous_present } else { 0 };
        if (!claimed || features == 0) && index.is_none() { return retired; }
        if let Some(i) = index.or_else(|| self.contacts.iter().position(Option::is_none)) {
            let old = self.contacts[i];
            self.contacts[i] = Some(Contact { window: event.window, id: event.id,
                features: old.map_or(features, |c| c.features),
                retired: retired || self.draining,
                ended: matches!(event.phase, super::TouchPhase::Ended | super::TouchPhase::Canceled) });
        } else { self.overflow = Some(event.window); }
        retired || self.draining || self.overflow == Some(event.window)
    }
    pub(super) fn publish(&mut self, ownership: &mut nvf_play::ui_pointer::TouchUiCapture, neutral: bool) {
        if neutral { self.contacts.fill(None); self.overflow = None; }
        for c in self.contacts.iter().flatten().filter(|c| c.retired && Some(c.window) == self.primary) {
            ownership.block(c.id);
        }
        if self.overflow == self.primary && self.primary.is_some() { ownership.quarantine_overflow(); }
    }
}

#[derive(SystemParam)]
pub(super) struct Bridge<'w> {
    state: ResMut<'w, Capture>,
    terminal: Option<Res<'w, nvf_play::terminal::TerminalScreen>>,
    terminal_settings: Option<Res<'w, nvf_play::terminal::touch::Settings>>,
    wheel: Option<Res<'w, nvf_play::companion_wheel::WheelScreen>>,
    wheel_settings: Option<Res<'w, nvf_play::companion_wheel::touch::Settings>>,
    lockpick: Option<Res<'w, nvf_play::lockpick::LockpickScreen>>,
    lockpick_options: Option<Res<'w, nvf_play::lockpick::touch::Options>>,
    dialogue: Option<Res<'w, nvf_play::dialogue_touch::TouchDialogue>>,
    play: Option<Res<'w, State<nvf_play::PlayState>>>,
    next: Option<Res<'w, NextState<nvf_play::PlayState>>>,
    retirements: Option<ResMut<'w, Retirements>>,
    paused: Option<Res<'w, nvf_play::pause::Paused>>,
}
impl Bridge<'_> {
    pub(super) fn refresh(&mut self) -> bool {
        for slot in &mut self.state.contacts { if slot.is_some_and(|c| c.ended) { *slot = None; } }
        let terminal = self
            .terminal
            .as_deref()
            .zip(self.terminal_settings.as_deref())
            .is_some_and(|(screen, settings)| {
                nvf_play::terminal::touch::captures(settings, screen)
            });
        let wheel = self
            .wheel
            .as_deref()
            .zip(self.wheel_settings.as_deref())
            .is_some_and(|(screen, settings)| {
                nvf_play::companion_wheel::touch::captures(settings, screen)
            });
        let play = match self.next.as_deref() {
            Some(NextState::Pending(next) | NextState::PendingIfDifferent(next)) => Some(*next),
            _ => self.play.as_deref().map(|state| *state.get()),
        };
        let dialogue =
            play.is_some_and(|state| self.dialogue.as_deref().is_some_and(|d| d.captures(state)));
        let lockpick = self.lockpick.as_deref().zip(self.lockpick_options.as_deref())
            .is_some_and(|(screen, options)| nvf_play::lockpick::touch::active(screen, options));
        let active = terminal || wheel || dialogue || lockpick;
        self.state.previous_features = self.state.features;
        self.state.features = u8::from(terminal) | (u8::from(wheel) << 1) | (u8::from(lockpick) << 2) | (u8::from(dialogue) << 3);
        let closed = self.state.previous_features & !self.state.features;
        for c in self.state.contacts.iter_mut().flatten() { if c.features & closed != 0 { c.retired = true; } }
        self.state.previous_present = self.state.present;
        self.state.present = u8::from(self.terminal.as_deref().is_some_and(|s| s.0.is_some()))
            | (u8::from(self.wheel.as_deref().is_some_and(|s| s.0.is_some())) << 1)
            | (u8::from(self.lockpick.as_deref().is_some_and(|s| s.0.is_some())) << 2)
            | (u8::from(play == Some(nvf_play::PlayState::Dialogue)) << 3);
        if play != Some(nvf_play::PlayState::Menu) { self.state.present &= 8; }
        if self.paused.as_ref().is_some_and(|p| p.0) { self.state.present = 0; }
        // The first frame after closure still owns the old screen's input batch.
        self.state.draining = self.state.active && !active;
        self.state.active = active;
        self.state.blocking()
    }
    pub(super) fn prepare_retirements(&mut self, primary: Entity, generic: impl Iterator<Item = u64>) {
        self.state.primary = Some(primary);
        let Some(pending) = self.retirements.as_mut().map(|r| r.take()) else { return; };
        let changed = [Feature::Terminal, Feature::Companion, Feature::Lockpick, Feature::Dialogue]
            .into_iter().filter(|f| pending.changed(*f)).fold(0, |bits, f| bits | bit(f));
        for contact in self.state.contacts.iter_mut().flatten() {
            if contact.features & changed != 0 { contact.retired = true; }
        }
        // A leaf may have admitted the contact after the parent's previous pass.
        // Accept its actual window only with a current/recent real feature session.
        let relevant = changed & (self.state.present | self.state.previous_present);
        if relevant == 0 { return; }
        for (window, id) in pending.contacts().filter(|(window, _)| *window == primary).chain(generic.map(|id| (primary, id))) {
            if let Some(c) = self.state.contacts.iter_mut().flatten().find(|c| c.window == window && c.id == id) { c.retired = true; }
            else if let Some(slot) = self.state.contacts.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(Contact { window, id, features: relevant, retired: true, ended: false });
            } else { self.state.overflow = Some(primary); }
        }
        if pending.overflowed() { self.state.overflow = Some(primary); }
    }
    pub(super) fn dialogue_only(&self) -> bool { self.state.dialogue_only() }

    pub(super) fn capture_event(&mut self, event: &super::TouchInput) { self.state.record(event, true); }

    pub(super) fn capture_batch(&mut self, touches: &mut MessageReader<super::TouchInput>) {
        for event in touches.read() { self.state.record(event, true); }
    }
    pub(super) fn tail(&mut self, event: &super::TouchInput) -> bool { self.state.record(event, false) }
}
