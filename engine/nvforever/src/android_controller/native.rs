//! The only Android-specific producer. The ECS consumer is shared with headless tests.
use super::*;
use std::sync::OnceLock;
use winit::platform::android::activity::{
    input::{Axis, InputEvent, KeyAction, Keycode, MotionAction},
    InputStatus,
};
static QUEUE: OnceLock<Arc<Mutex<Queue>>> = OnceLock::new();
pub(super) fn queue() -> Arc<Mutex<Queue>> {
    QUEUE
        .get_or_init(|| Arc::new(Mutex::new(Queue::default())))
        .clone()
}
pub(super) fn install() {
    if let Some(app) = bevy::android::ANDROID_APP.get() {
        for axis in AXES {
            app.enable_motion_axis(Axis::from(axis));
        }
    }
    winit::platform::android::set_input_event_handler(Some(input));
}
fn map_key(key: Keycode) -> Option<GamepadButton> {
    Some(match key {
        Keycode::ButtonA => GamepadButton::South,
        Keycode::ButtonB => GamepadButton::East,
        Keycode::ButtonX => GamepadButton::West,
        Keycode::ButtonY => GamepadButton::North,
        Keycode::ButtonL1 => GamepadButton::LeftTrigger,
        Keycode::ButtonR1 => GamepadButton::RightTrigger,
        Keycode::ButtonL2 => GamepadButton::LeftTrigger2,
        Keycode::ButtonR2 => GamepadButton::RightTrigger2,
        Keycode::ButtonSelect => GamepadButton::Select,
        Keycode::ButtonStart => GamepadButton::Start,
        Keycode::ButtonThumbl => GamepadButton::LeftThumb,
        Keycode::ButtonThumbr => GamepadButton::RightThumb,
        Keycode::DpadUp => GamepadButton::DPadUp,
        Keycode::DpadDown => GamepadButton::DPadDown,
        Keycode::DpadLeft => GamepadButton::DPadLeft,
        Keycode::DpadRight => GamepadButton::DPadRight,
        _ => return None,
    })
}
fn input(event: &InputEvent<'_>) -> Option<InputStatus> {
    let shared = queue();
    let mut queue = shared.lock().unwrap_or_else(|p| p.into_inner());
    let consumed = match event {
        InputEvent::KeyEvent(key) => {
            let mapped = map_key(key.key_code());
            let kind = if mapped.is_some() {
                NativeKind::MappedKey
            } else {
                NativeKind::UnmappedKey
            };
            let sources = key.source().into();
            match ingress(&queue, key.device_id(), sources, kind) {
                Ingress::Fallback => return None,
                // Claim late controller events even when no longer admissible. Letting
                // winit see them would synthesize ordinary keyboard input instead.
                Ingress::Drop => true,
                Ingress::Admit => {
                    let button = mapped?;
                    match key.action() {
                        KeyAction::Down => {
                            queue.key(
                                key.device_id(),
                                sources,
                                button,
                                true,
                                key.repeat_count() > 0,
                            );
                        }
                        KeyAction::Up => {
                            queue.key(key.device_id(), sources, button, false, false);
                        }
                        _ => queue.push(Sample::Release(key.device_id())),
                    }
                    true
                }
            }
        }
        InputEvent::MotionEvent(motion) => {
            let sources = motion.source().into();
            match ingress(&queue, motion.device_id(), sources, NativeKind::Motion) {
                Ingress::Fallback => return None,
                // A retired joystick must not fall into winit's touch translation.
                Ingress::Drop => true,
                Ingress::Admit => {
                    match motion.action() {
                        MotionAction::Move if motion.pointer_count() > 0 => {
                            let pointer = motion.pointer_at_index(0);
                            let axes = AXES
                                .into_iter()
                                .map(|a| (a, pointer.axis_value(Axis::from(a))))
                                .collect();
                            queue.motion(motion.device_id(), sources, axes);
                        }
                        MotionAction::Cancel => queue.push(Sample::Release(motion.device_id())),
                        // Unsupported controller-motion actions are owned and dropped.
                        _ => {}
                    }
                    true
                }
            }
        }
        _ => false,
    };
    consumed.then_some(InputStatus::Handled)
}
pub(super) fn poll(inbox: Res<Inbox>, mut warned: Local<bool>) {
    let result = crate::android::activity_string("controllerDevices", None).and_then(|s| {
        if s.is_empty() {
            Ok(())
        } else {
            inbox.lock().snapshot(&s)
        }
    });
    match result {
        Ok(()) => *warned = false,
        Err(error) => {
            inbox.lock().invalidate();
            if !*warned {
                warn!("Android controller metadata unavailable: {error}");
                *warned = true;
            }
        }
    }
}
