//! The phone's soft keyboard for in-game text fields (Android). A field has the
//! keyboard when the settings search or a form holds it ([`KeyboardCapture`]), the
//! character's name is being entered, or the console is open. The window's IME
//! switch is the existing bridge: winit asks the activity to show or hide the soft
//! input, and typed keys keep arriving as ordinary `KeyboardInput` text, Backspace
//! and Enter for each field's existing consumer. Desktop and a physical keyboard are
//! untouched: off the phone this never changes the window.
use crate::bindings::KeyboardCapture;
use bevy::{input::touch::{TouchInput, TouchPhase}, prelude::*, window::PrimaryWindow};

#[derive(Resource)]
pub struct SoftKeyboard {
    pub enabled: bool,
    shown: bool,
    reshow: bool,
}
impl Default for SoftKeyboard {
    fn default() -> Self { Self::for_platform(cfg!(target_os = "android")) }
}
impl SoftKeyboard {
    pub fn for_platform(enabled: bool) -> Self { Self { enabled, shown: false, reshow: false } }
}

pub struct SoftKeyboardPlugin;
impl Plugin for SoftKeyboardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SoftKeyboard>().add_systems(PostUpdate, follow);
    }
}

type Fields<'w> = (
    Option<Res<'w, KeyboardCapture>>,
    Option<Res<'w, crate::pause::FormOver>>,
    Option<Res<'w, crate::pause::FormTextInput>>,
    Option<Res<'w, crate::chargen::ChargenMenus>>,
    Option<Res<'w, crate::console::Console>>,
);

fn follow(
    mut keyboard: ResMut<SoftKeyboard>,
    (capture, form, form_text, chargen, console): Fields<'_>,
    mut touches: MessageReader<TouchInput>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let tapped = touches.read().any(|t| t.phase == TouchPhase::Started);
    if !keyboard.enabled { return; }
    let Ok(mut window) = windows.single_mut() else { return; };
    // Non-text overlays over the pause menu (Customize Touch Layout, Controls Help, DLSS dialog)
    // hold `FormOver` and `KeyboardCapture` to block menu keys, but must never open the soft keyboard.
    let non_text_form = form.is_some_and(|f| f.0) && !form_text.is_some_and(|t| t.0);
    let focused = (capture.is_some_and(|c| c.0) && !non_text_form)
        || chargen.is_some_and(|c| c.name_open())
        || console.is_some_and(|c| c.open);
    // Focus loss hides it; returning to a still-focused field shows it again.
    let wanted = focused && window.focused;
    if keyboard.reshow {
        keyboard.reshow = false;
        if wanted {
            window.ime_enabled = true;
            keyboard.shown = true;
            return;
        }
    }
    if wanted && keyboard.shown && tapped {
        // A tap on the field asks again, so a keyboard the player put away comes back.
        // The IME switch only acts on a change: off this frame, on the next.
        window.ime_enabled = false;
        keyboard.reshow = true;
    } else if wanted != keyboard.shown || window.ime_enabled != wanted {
        window.ime_enabled = wanted;
        keyboard.shown = wanted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::{keyboard::{Key, KeyboardInput}, ButtonState};

    fn scene(enabled: bool) -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_message::<TouchInput>().add_message::<KeyboardInput>()
            .insert_resource(SoftKeyboard::for_platform(enabled)).init_resource::<KeyboardCapture>()
            .init_resource::<crate::chargen::ChargenMenus>().add_systems(PostUpdate, follow);
        let window = app.world_mut().spawn((Window::default(), PrimaryWindow)).id();
        app.update();
        (app, window)
    }
    fn ime(app: &App, window: Entity) -> bool { app.world().get::<Window>(window).unwrap().ime_enabled }
    fn tap(app: &mut App, window: Entity) {
        app.world_mut().write_message(TouchInput { window, id: 1, phase: TouchPhase::Started, position: Vec2::new(200.0, 100.0), force: None });
    }

    #[test]
    fn a_focused_field_shows_the_keyboard_a_tap_reasks_and_close_or_focus_loss_hides_it() {
        let (mut app, window) = scene(true);
        assert!(!ime(&app, window));
        app.world_mut().resource_mut::<KeyboardCapture>().0 = true;
        app.update();
        assert!(ime(&app, window), "the field's focus shows the soft keyboard");
        // The player put it away; tapping the field asks again (off, then on).
        tap(&mut app, window);
        app.update();
        assert!(!ime(&app, window));
        app.update();
        assert!(ime(&app, window));
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        assert!(!ime(&app, window), "focus loss hides it");
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert!(ime(&app, window), "returning to the open field shows it again");
        // Done, confirm or cancel close the field and the keyboard with it.
        app.world_mut().resource_mut::<KeyboardCapture>().0 = false;
        app.update();
        assert!(!ime(&app, window));
        let mut console = crate::console::Console::default();
        console.open = true;
        app.insert_resource(console);
        app.update();
        assert!(ime(&app, window), "the console's line is a text field too");
        app.world_mut().resource_mut::<crate::console::Console>().open = false;
        app.update();
        assert!(!ime(&app, window));
    }

    #[test]
    fn the_character_name_field_owns_the_keyboard_until_done() {
        let (mut app, window) = scene(true);
        app.world_mut().resource_mut::<crate::chargen::ChargenMenus>().open_name_for_test();
        app.update();
        assert!(ime(&app, window));
        app.world_mut().resource_mut::<crate::chargen::ChargenMenus>().answer_for_test();
        app.update();
        assert!(!ime(&app, window), "Done hides it while the menu closes");
    }

    #[test]
    fn non_text_form_overlay_suppresses_soft_keyboard_even_when_tapped() {
        let (mut app, window) = scene(true);
        app.init_resource::<crate::pause::FormOver>().init_resource::<crate::pause::FormTextInput>();
        // Search opened IME first; opening a non-text overlay (Customize Touch Layout) hides it.
        app.world_mut().resource_mut::<KeyboardCapture>().0 = true;
        app.update();
        assert!(ime(&app, window));
        app.world_mut().resource_mut::<crate::pause::FormOver>().0 = true;
        tap(&mut app, window);
        app.update();
        app.update();
        assert!(!ime(&app, window), "non-text overlay like Customize Touch Layout must suppress IME");
        // An explicit text form (Report a Bug) keeps IME active.
        app.world_mut().resource_mut::<crate::pause::FormTextInput>().0 = true;
        app.update();
        assert!(ime(&app, window), "text form overlay keeps IME enabled");
        app.world_mut().resource_mut::<crate::pause::FormTextInput>().0 = false;
        app.world_mut().resource_mut::<crate::pause::FormOver>().0 = false;
        app.world_mut().resource_mut::<KeyboardCapture>().0 = false;
        app.update();
        assert!(!ime(&app, window));
    }

    #[test]
    fn desktop_and_physical_keyboards_keep_the_window_ime_untouched() {
        let (mut app, window) = scene(false);
        app.world_mut().resource_mut::<KeyboardCapture>().0 = true;
        app.world_mut().write_message(KeyboardInput { key_code: KeyCode::KeyA, logical_key: Key::Character("a".into()),
            state: ButtonState::Pressed, text: Some("a".into()), repeat: false, window });
        tap(&mut app, window);
        app.update();
        app.update();
        assert!(!ime(&app, window));
        assert!(!SoftKeyboard::default().enabled || cfg!(target_os = "android"));
    }
}
