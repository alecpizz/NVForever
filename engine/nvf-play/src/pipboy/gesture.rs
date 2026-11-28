//! Owned Android arm pose. Native inventory admission/close remains the menu's.
use super::{Frame, Phase, PipBoy, HOLD_SECONDS};
use crate::{
    control::{PlayerControl, Pov},
    menu::{ActiveMenu, MenuInput, MenuSystems, OpenInventory},
    PlaySet, PlayState,
};
use bevy::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Drag,
    Settle,
    Open,
    Close,
    WaitOpen,
    WaitClose,
}
#[derive(Clone, Copy)]
struct Session {
    original_open: bool,
    progress: f32,
    goal: bool,
    stage: Stage,
    model: Entity,
    abort: bool,
    orbit: Option<crate::control::Orbit>,
}
/// No resource on old callers is required; an idle instance never changes a pose.
#[derive(Resource, Default)]
pub struct ArmGesture {
    session: Option<Session>,
}
impl ArmGesture {
    pub fn captures(&self) -> bool {
        self.session.is_some()
    }
    pub fn model(&self) -> Option<Entity> {
        self.session.map(|s| s.model)
    }
    pub fn progress(&self) -> Option<f32> {
        self.session.map(|s| s.progress)
    }
    pub fn begin(&mut self, open: bool, model: Entity) -> bool {
        if self.captures() {
            return false;
        }
        self.session = Some(Session {
            original_open: open,
            progress: if open { 1.0 } else { 0.0 },
            goal: open,
            stage: Stage::Drag,
            model,
            abort: false,
            orbit: None,
        });
        true
    }
    pub fn drag(&mut self, progress: f32) {
        if !progress.is_finite() {
            self.cancel();
            return;
        }
        if let Some(s) = self
            .session
            .as_mut()
            .filter(|s| s.stage == Stage::Drag && !s.abort)
        {
            s.progress = progress.clamp(0.0, 1.0);
        }
    }
    pub fn release(&mut self) {
        if let Some(s) = self
            .session
            .as_mut()
            .filter(|s| s.stage == Stage::Drag && !s.abort)
        {
            s.goal = s.progress >= 0.5;
            s.stage = Stage::Settle;
        }
    }
    /// Return to the original endpoint. Do not turn a cancellation into an open.
    pub fn cancel(&mut self) {
        if let Some(s) = self.session.as_mut() {
            if matches!(s.stage, Stage::WaitOpen | Stage::WaitClose) {
                s.abort = true;
            } else {
                s.goal = s.original_open;
                s.stage = Stage::Settle;
            }
        }
    }
    /// Lifecycle/model replacement: restore the admitted native state immediately.
    pub fn abort(&mut self) {
        if let Some(s) = self.session.as_mut() {
            s.abort = true;
        }
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<ArmGesture>()
        .add_systems(
            PreUpdate,
            cancel_hardware
                .after(bevy::input::InputSystems)
                .after(crate::pad::InputSwitchSystems::Switch),
        )
        .add_systems(Update, requests.before(MenuSystems).in_set(PlaySet::Ui));
}
fn cancel_hardware(
    mut gesture: ResMut<ArmGesture>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    (gate, bindings): (
        Option<Res<crate::pad::PlatformInputGate>>,
        Option<Res<crate::bindings::Bindings>>,
    ),
) {
    if !gesture.captures() {
        return;
    }
    if gate
        .as_deref()
        .is_some_and(crate::pad::PlatformInputGate::native_blocked)
        || bindings
            .as_ref()
            .is_some_and(|b| b.device == crate::bindings::InputDevice::Controller)
    {
        gesture.abort();
    } else if keys.just_pressed(KeyCode::Escape)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::East))
    {
        gesture.cancel();
    }
}
fn inventory(active: &ActiveMenu) -> bool {
    active
        .0
        .as_ref()
        .is_some_and(|m| m.mode == crate::menu::model::Mode::Inventory)
}
fn finish(
    pip: &mut PipBoy,
    control: &mut PlayerControl,
    open: bool,
    orbit: Option<crate::control::Orbit>,
) {
    pip.manual_pose = false;
    pip.phase = if open { Phase::Up } else { Phase::Down };
    pip.clip_time = if open { HOLD_SECONDS } else { 0.0 };
    if !open {
        if let Some(pov) = pip.saved_pov.take() {
            control.pov = pov;
        }
        if let Some(mut orbit) = orbit {
            // Restore camera orientation, never an old held-input flag.
            orbit.held = false;
            orbit.moved = false;
            control.orbit = orbit;
        }
    }
}
/// Requests are emitted before the actual menu systems and resolved after them.
/// A canceled session emits nothing; request messages never wait behind a finger.
fn requests(
    mut gesture: ResMut<ArmGesture>,
    active: Res<ActiveMenu>,
    state: Res<State<PlayState>>,
    gate: Option<Res<crate::pad::PlatformInputGate>>,
    (mut open, mut close): (MessageWriter<OpenInventory>, MessageWriter<MenuInput>),
) {
    let Some(s) = gesture.session.as_mut() else {
        return;
    };
    if s.abort
        || gate
            .as_deref()
            .is_some_and(crate::pad::PlatformInputGate::native_blocked)
        || matches!(state.get(), PlayState::Loading | PlayState::Dialogue)
    {
        s.abort = true;
        return;
    }
    match s.stage {
        Stage::Open if *state.get() == PlayState::Playing && active.0.is_none() => {
            open.write(OpenInventory);
            s.stage = Stage::WaitOpen;
        }
        Stage::Close if *state.get() == PlayState::Menu && inventory(&active) => {
            close.write(MenuInput(crate::menu::model::Command::Close));
            s.stage = Stage::WaitClose;
        }
        Stage::Open | Stage::Close => {
            s.abort = true;
        }
        _ => {}
    }
}

/// Called between native follow_menu and follow_frame; the real body consumes
/// clip_time through its ordinary Pip-Boy Animator layer on the following pose pass.
pub(super) fn drive(
    mut gesture: ResMut<ArmGesture>,
    mut pip: ResMut<PipBoy>,
    mut control: ResMut<PlayerControl>,
    (active, state, next): (
        Res<ActiveMenu>,
        Res<State<PlayState>>,
        Res<NextState<PlayState>>,
    ),
    gate: Option<ResMut<crate::pad::PlatformInputGate>>,
    time: crate::pause::AidTime,
    locks: Option<Res<crate::control::ControlLocks>>,
) {
    let Some(mut s) = gesture.session else { return };
    let mut gate = gate;
    let native = gate
        .as_deref()
        .is_some_and(crate::pad::PlatformInputGate::native_blocked);
    let own_inventory = inventory(&active);
    let closing = matches!(
        &*next,
        NextState::Pending(PlayState::Playing) | NextState::PendingIfDifferent(PlayState::Playing)
    );
    let invalid = matches!(state.get(), PlayState::Loading | PlayState::Dialogue)
        || active.0.as_ref().is_some_and(|_| !own_inventory)
        || pip.frame != Frame::Device;
    if s.abort || native || invalid || locks.as_ref().is_some_and(|l| l.pipboy) {
        finish(
            &mut pip,
            &mut control,
            own_inventory && !closing,
            s.orbit.filter(|_| !native && !invalid),
        );
        gesture.session = None;
        if let Some(g) = gate.as_deref_mut() {
            g.set_arm_blocked(false);
        }
        return;
    }
    if s.stage == Stage::WaitOpen {
        if own_inventory {
            finish(&mut pip, &mut control, true, None);
            gesture.session = None;
            if let Some(g) = gate.as_deref_mut() {
                g.set_arm_blocked(false);
            }
            return;
        }
        // Native movement/Pip-Boy/script admission refused the open. Settle down.
        s.goal = false;
        s.stage = Stage::Settle;
    } else if s.stage == Stage::WaitClose {
        // Native close changes NextState; OnExit owns the model/script teardown.
        finish(
            &mut pip,
            &mut control,
            own_inventory && !closing,
            s.orbit.filter(|_| !native && !invalid),
        );
        gesture.session = None;
        if let Some(g) = gate.as_deref_mut() {
            g.set_arm_blocked(false);
        }
        return;
    }
    if !pip.manual_pose && pip.phase == Phase::Down {
        pip.saved_pov = Some(control.pov);
        s.orbit = Some(control.orbit);
    }
    pip.manual_pose = true;
    pip.phase = Phase::Raising;
    control.pov = Pov::First;
    control.orbit = default();
    // Preserve cold-rig warming, its borrowed pose and its actual render proof.
    if pip.warming.is_none() {
        if s.stage == Stage::Settle {
            let step = (time.delta_secs_f64() as f32).clamp(0.0, 0.05) / HOLD_SECONDS;
            s.progress = if s.goal {
                (s.progress + step).min(1.0)
            } else {
                (s.progress - step).max(0.0)
            };
        }
        pip.clip_time = s.progress * HOLD_SECONDS;
        if s.stage == Stage::Settle && (s.progress == 0.0 || s.progress == 1.0) {
            if s.goal && own_inventory {
                finish(&mut pip, &mut control, true, None);
                gesture.session = None;
            } else if !s.goal && !own_inventory {
                finish(&mut pip, &mut control, false, s.orbit);
                gesture.session = None;
            } else {
                s.stage = if s.goal { Stage::Open } else { Stage::Close };
                gesture.session = Some(s);
            }
        } else {
            gesture.session = Some(s);
        }
    } else {
        gesture.session = Some(s);
    }
    if let Some(g) = gate.as_deref_mut() {
        g.set_arm_blocked(gesture.captures());
    }
}

#[cfg(test)]
mod tests;
