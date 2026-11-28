//! Direct screen selection; only the native picker can publish a usable target.
use super::Targeted;
use bevy::prelude::*;
use bevy::{
    camera::RenderTarget,
    window::{PrimaryWindow, WindowRef},
};

pub const HOLD_SECONDS: f32 = 0.6;
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum Confirmation {
    #[default]
    Off,
    Tap,
    DoubleTap,
    Hold,
}
#[derive(Clone, Copy, Debug)]
pub struct ScreenPoint {
    pub window: Entity,
    pub id: u64,
    pub position: Vec2,
}
#[derive(Resource, Default)]
pub struct TouchActivation {
    mode: Confirmation,
    point: Option<ScreenPoint>,
    target: Option<Targeted>,
    expected: Option<u32>,
    generation: u64,
    down: bool,
    disarmed: bool,
    confirm: bool,
    ready: Option<Targeted>,
    held: f32,
    sampled: bool,
}
impl TouchActivation {
    pub fn configure(&mut self, mode: Confirmation) {
        if self.mode != mode {
            self.cancel();
            self.mode = mode;
        }
    }
    pub fn begin(&mut self, point: ScreenPoint, expected: Option<u32>) -> u64 {
        self.cancel();
        self.generation = self.generation.wrapping_add(1);
        if self.mode != Confirmation::Off && point.position.is_finite() {
            self.point = Some(point);
            self.expected = expected;
            self.down = true;
        }
        self.generation
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn point(&self) -> Option<ScreenPoint> {
        self.point
    }
    pub fn target(&self) -> Option<&Targeted> {
        self.target.as_ref()
    }
    pub fn might_activate(&self) -> bool {
        self.point.is_some() && !self.disarmed
    }
    pub fn progress(&self) -> f32 {
        if self.mode == Confirmation::Hold && self.down && !self.disarmed {
            (self.held / HOLD_SECONDS).clamp(0., 1.)
        } else {
            0.
        }
    }
    pub fn end(&mut self, confirm: bool) {
        self.down = false;
        self.confirm = confirm;
    }
    pub fn cancel(&mut self) {
        self.point = None;
        self.target = None;
        self.expected = None;
        self.down = false;
        self.disarmed = false;
        self.confirm = false;
        self.ready = None;
        self.held = 0.;
        self.sampled = false;
    }
    pub(super) fn publish(&mut self, target: Option<Targeted>, dt: f32) {
        self.ready = None;
        if self.disarmed || self.point.is_none() {
            return;
        }
        let Some(target) = target.filter(Targeted::prompted) else {
            self.disarmed = true;
            self.target = None;
            self.held = 0.;
            self.confirm = false;
            return;
        };
        if self.expected.is_some_and(|id| id != target.form_id) {
            self.disarmed = true;
            self.target = None;
            self.held = 0.;
            self.confirm = false;
            return;
        }
        self.expected = Some(target.form_id);
        self.target = Some(target.clone());
        if self.mode == Confirmation::Hold && self.down {
            // Start the hold only after the first real legal target sample.
            if self.sampled {
                self.held += dt.clamp(0., 0.1);
            }
            self.confirm = self.held >= HOLD_SECONDS;
        }
        self.sampled = true;
        if self.confirm {
            self.ready = Some(target);
            self.confirm = false;
            self.disarmed = true;
        }
    }
    pub(super) fn take_ready(&mut self, input: &crate::bindings::ControlInput) -> Option<Targeted> {
        let ready = self.ready.take();
        if !input.virtual_action_allowed(crate::bindings::Control::Activate) {
            return None;
        }
        ready
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Admission<'w> {
    bindings: Option<Res<'w, crate::bindings::Bindings>>,
    gate: Option<Res<'w, crate::pad::PlatformInputGate>>,
    switching: Option<Res<'w, crate::pad::TouchDeviceSwitch>>,
    capture: Option<Res<'w, crate::bindings::KeyboardCapture>>,
    state: Option<Res<'w, State<crate::PlayState>>>,
    next: Option<Res<'w, NextState<crate::PlayState>>>,
    locks: Option<Res<'w, crate::control::ControlLocks>>,
    overlay: Option<Res<'w, crate::pipboy::touch_items::OverlayCapture>>,
    keys: Option<Res<'w, ButtonInput<KeyCode>>>,
    mouse: Option<Res<'w, ButtonInput<MouseButton>>>,
    vats: Option<Res<'w, crate::vats_mode::VatsMode>>,
    wheel: Option<Res<'w, crate::weapon_wheel::WeaponWheel>>,
}
impl Admission<'_> {
    pub(super) fn allows(&self, id: u64) -> bool {
        let firing = self.bindings.as_ref().is_some_and(|b| {
            let binding = b.get(crate::bindings::Control::Use);
            binding
                .key
                .is_some_and(|k| self.keys.as_ref().is_some_and(|keys| keys.pressed(k)))
                || binding
                    .mouse
                    .is_some_and(|m| self.mouse.as_ref().is_some_and(|mouse| mouse.pressed(m)))
        });
        self.vats.as_ref().is_none_or(|v| !v.holds_input())
            && self.wheel.as_ref().is_none_or(|w| !w.holds_input())
            && self
                .bindings
                .as_ref()
                .is_none_or(|b| b.device == crate::bindings::InputDevice::KeyboardMouse)
            && !firing
            && self.overlay.as_ref().is_none_or(|o| !o.0)
            && self.gate.as_ref().is_none_or(|g| g.allows_touch(id))
            && self.switching.as_ref().is_none_or(|s| s.allows_actions())
            && self.capture.as_ref().is_none_or(|c| !c.0)
            && self.locks.as_ref().is_none_or(|l| !l.movement)
            && self
                .state
                .as_ref()
                .is_none_or(|s| *s.get() == crate::PlayState::Playing)
            && self.next.as_ref().is_none_or(|s| match &**s {
                NextState::Pending(s) | NextState::PendingIfDifferent(s) => {
                    *s == crate::PlayState::Playing
                }
                NextState::Unchanged => true,
            })
    }
}
pub(super) type Context<'w, 's> = (
    Option<ResMut<'w, TouchActivation>>,
    CameraView<'w, 's>,
    WindowView<'w, 's>,
    Admission<'w>,
    crate::bindings::ControlInput<'w, 's>,
    Option<Res<'w, Time<Real>>>,
);
type CameraView<'w, 's> = Query<
    'w,
    's,
    (
        &'static Camera,
        &'static GlobalTransform,
        Option<&'static RenderTarget>,
    ),
    With<crate::PlayerCamera>,
>;
type WindowView<'w, 's> = Query<'w, 's, (Entity, &'static Window), With<PrimaryWindow>>;
pub(super) fn camera_ray(
    point: ScreenPoint,
    cameras: &CameraView,
    windows: &WindowView,
) -> Option<([f32; 3], [f32; 3])> {
    let (window_id, window) = windows.single().ok()?;
    let size = Vec2::new(window.width(), window.height());
    if point.window != window_id
        || !point.position.is_finite()
        || !size.is_finite()
        || size.min_element() <= 0.
        || !Rect::from_corners(Vec2::ZERO, size).contains(point.position)
    {
        return None;
    }
    let (camera, transform, target) = cameras.single().ok()?;
    if !camera.is_active || !transform.translation().is_finite() {
        return None;
    }
    let viewport = camera.logical_viewport_rect()?;
    let position = match target {
        Some(RenderTarget::Image(_)) => viewport.min + point.position / size * viewport.size(),
        Some(RenderTarget::Window(WindowRef::Entity(entity))) if *entity != window_id => {
            return None;
        }
        Some(RenderTarget::Window(_)) | None => {
            if !viewport.contains(point.position) {
                return None;
            }
            point.position
        }
        _ => return None,
    };
    let ray = camera.viewport_to_world(transform, position).ok()?;
    Some((
        crate::space::to_game(transform.translation()),
        crate::space::to_game(ray.direction.as_vec3()),
    ))
}

#[cfg(test)]
mod tests;
