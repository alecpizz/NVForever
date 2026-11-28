//! The game's controls and their default keys and mouse buttons (docs/design/
//! player-body.md §1.2). Every control of the game's `[Controls]` INI section is here,
//! bound as the game binds it by default; systems ask for a control
//! ([`ControlInput::pressed`]) instead of a key.
//!
//! The defaults are the owner's `FalloutPrefs.ini` `[Controls]` values [observed
//! 2026-10-09; hypothesis: they are the game's defaults, nothing in them is rebound].
//! Each value is four bytes `00 KK MM JJ` [observed: the layout fits every line]:
//! a DirectInput keyboard scan code `KK` [documented: Microsoft DirectInput `DIK_*`
//! keyboard constants], a mouse button `MM` (0 left, 1 right, 2 middle; `FF` none —
//! `Use` = 0 attacks, `Block` = 1 aims, by their names) and a controller button `JJ`
//! (`crate::pad::game_pad`). NVForever never reads the game's INIs at run time
//! (docs/design/install-layout.md); the values are built in.
//!
//! **The player's own keys** are kept in `[Controls]` of the home's `NVForever.ini`
//! (beside `[Settings]`, `crate::settings::SettingsFile`), one line per control by its
//! game INI name, the key and mouse button by name: `Forward=W`, `Block=Left Alt, Right
//! Mouse`, `Use=Left Mouse`, `Auto Move=None` ([`KEY_NAMES`], [`MOUSE_NAMES`]), and
//! `Always Run By Default`, `Invert Y` (1/0), `Mouse Sensitivity`, `Mouse Vertical
//! Sensitivity` (#2201) and `Aim Sensitivity` (the look while aiming, owner 2026-11-15;
//! both read as 1 when missing). Each start reads
//! them ([`apply_stored`], `PostStartup`); a line missing from an INI that exists is
//! written with the control's key then, so the file always lists every control to edit.
//! A value that does not parse is skipped with a warning (the default stays). The pause
//! menu's Controls page (`crate::pause`) rebinds a control ([`rebind`]): the new key or
//! button is taken from any other control that had it, which gets the old one instead
//! (a swap, so no control is left without its input by accident), and the section is
//! written again. Esc and the console's key (`` ` ``) cannot be bound [design: they
//! open the pause menu and the console].
//!
//! **Sprint** (NVForever's own, NVF Features › Advanced Movement; `crate::stance`) has no key
//! until the player binds one, and acts only while its switch is on: off, its Controls row is
//! greyed and cannot be rebound. Run keeps Left Shift whatever Sprint's state; a Sprint key
//! the player sets is handled as any other binding (a swap with the control that had it)
//! [design, #3000: the maintainer's decision on #3002].
//!
//! **The controller** (`crate::pad`, docs/design/controller.md): `Input Device` in
//! `[Controls]` (`Keyboard and Mouse` or `Controller`, [`InputDevice`]) says which works
//! the controls; the controller's buttons are kept in `[Controller]`, one line per
//! control by the same names (`Activate=A`, `Forward=Left Stick Up`, `Hotkey5=None`),
//! with `Look Speed Horizontal`, `Look Speed Vertical`, the stick's own `Stick Sensitivity`,
//! `Stick Vertical Sensitivity` and `Aim Sensitivity`, and `Rumble` (1/0, the pad
//! shakes, `crate::rumble`; the game's `bGamePadRumble=1` [observed 2026-10-16, the
//! owner's `FalloutPrefs.ini`]). Their defaults are the game's
//! (the `JJ` bytes and its `fXenonHorizLookSpeed`, `fXenonVertLookSpeed`); the D-pad,
//! which the game's values leave free, takes Hotkeys 1 to 4 [design: the console
//! version's quick keys are on the D-pad, documented in its published control lists].
//! A control rebound on the Controls tab with Controller chosen takes a controller
//! button, swapping as keys do; Start cannot be bound (the pause menu).

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use crate::pad::PadInput;
use fnv_install::{guard_write, ini_get, ini_set};
use std::path::Path;

/// The game's controls, in the INI's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Control {
    Forward,
    Back,
    SlideLeft,
    SlideRight,
    /// Attack.
    Use,
    Activate,
    /// Aim down the sights (guns) or block (melee and fists).
    Block,
    /// Draw the weapon, reload it, or (held) holster it.
    ReadyItem,
    CrouchSneak,
    /// Walk while Always Run is on (run while it is off).
    Run,
    AlwaysRun,
    AutoMove,
    Jump,
    /// First / third person; held with the mouse moving, circle the camera.
    TogglePov,
    /// The Pip-Boy.
    MenuMode,
    /// Wait.
    Rest,
    Vats,
    Hotkey1,
    /// The INI names the second hotkey `Ammo Swap`.
    AmmoSwap,
    Hotkey3,
    Hotkey4,
    Hotkey5,
    Hotkey6,
    Hotkey7,
    Hotkey8,
    QuickSave,
    QuickLoad,
    Grab,
    /// Lower or raise the drawn weapon by hand (NVForever's own; not in the game's INI).
    LowerWeapon,
    /// Run faster for AP while held (NVForever's own; acts only with NVF Features ›
    /// Advanced Movement › Sprint on, `crate::stance`).
    Sprint,
    /// Open or close photo mode (NVForever's own; not in the game's INI, `crate::photo_mode`).
    PhotoMode,
    // The Tweaks tab's keys (`crate::tweaks`, docs/design/tweaks.md): none bound unless
    // the player binds them.
    /// Holster the weapon at once (Stewie's Tweaks iHolsterWeaponKey).
    HolsterWeapon,
    /// The Pip-Boy light on or off (iTogglePipboyLightKey).
    ToggleLight,
    /// The crosshair shown or hidden (iToggleCrosshairKey).
    ToggleCrosshair,
    /// The sneak label shown or hidden (iToggleSneakIndicatorKey).
    ToggleSneakLabel,
    /// Every menu and the HUD hidden or shown, as ToggleMenus (iToggleMenusKey).
    ToggleMenus,
    /// Back to the main menu at once (iExitToMainMenuKey).
    ExitToMainMenu,
    /// Quit at once (iExitGameKey).
    ExitGame,
    /// No collision while held (iDisableCollisionKey).
    NoCollision,
    /// The Pip-Boy opened on a page (iPipboyStatsKey, iPipboyItemsKey, iPipboyDataKey).
    PipBoyStats,
    PipBoyItems,
    PipBoyData,
    /// The Pip-Boy opened on a DATA tab (iOpenLocalMapKey, iOpenMapKey, iOpenQuestsKey,
    /// iOpenRadioKey).
    OpenLocalMap,
    OpenWorldMap,
    OpenQuests,
    OpenRadio,
    /// True iron sights on or off (iToggleTrueIronSightsKey).
    ToggleIronSights,
    /// The weapon in hand dropped (iDropEquippedWeaponKey).
    DropWeapon,
    /// The Radio volume a tenth up or down (iRadioVolumeUpKey, iRadioVolumeDownKey).
    RadioVolumeUp,
    RadioVolumeDown,
    /// The weapon equipped before the one now (iEquipLastWeaponKey).
    LastWeapon,
    /// Smoothed looking on or off (iToggleSmoothCameraKey).
    SmoothCamera,
    /// Activate again and again while held (iRepeatActivateKey).
    RepeatActivate,
    /// The iron sights on the weapon's second sighting node or back (iToggleSightingNodeHotkey).
    ToggleSightingNode,
    /// The custom map marker where the player stands (iPlaceMapMarkerAtPlayerPosKey).
    PlaceMarkerHere,
    /// Hide the chosen parts of the HUD or show them again (iHideHUDKey).
    HideHud,
    /// End the Pip-Boy radio's song or talk now (iSkipRadioSongKey).
    SkipRadioSong,
    /// Open and close the console (the original's ` key; Stewie's Tweaks iConsoleKey), a
    /// binding like any other [design: owner 2026-11-16: "make the console hotkey and
    /// screenshot hotkey a normal rebindable key"].
    Console,
    /// Save a picture of the screen (NVForever's F12; Stewie's Tweaks iScreenshotKey).
    Screenshot,
}

/// `[Controls]` of the owner's `FalloutPrefs.ini`: (control, INI name, value)
/// [observed 2026-10-09], then NVForever's own controls (after [`GAME_INI_CONTROLS`],
/// coded the same way): Lower Weapon on H [design: the owner 2026-10-27, moved off Ready
/// Item tapped twice so a reload is not held back waiting for a second tap]; Sprint unbound,
/// with no controller button either, until the player binds it [design, #3000: the
/// maintainer's decision on #3002]; Photo Mode unbound, no key and no controller button,
/// until the player binds it under Controls, Actions (the pause menu's Photo Mode entry
/// opens it meanwhile) [design, #3634: the maintainer's decision].
pub const GAME_CONTROLS: [(Control, &str, u32); 59] = [
    (Control::Forward, "Forward", 0x0011FF13),
    (Control::Back, "Back", 0x001FFF14),
    (Control::SlideLeft, "Slide Left", 0x001EFF17),
    (Control::SlideRight, "Slide Right", 0x0020FF16),
    (Control::Use, "Use", 0x00FF0011),
    (Control::Activate, "Activate", 0x0012FF0A),
    (Control::Block, "Block", 0x00380110),
    (Control::ReadyItem, "Ready Item", 0x0013FF0C),
    (Control::CrouchSneak, "Crouch/Sneak", 0x001DFF08),
    (Control::Run, "Run", 0x002AFFFF),
    (Control::AlwaysRun, "Always Run", 0x003AFFFF),
    (Control::AutoMove, "Auto Move", 0x0010FFFF),
    (Control::Jump, "Jump", 0x0039FF0D),
    (Control::TogglePov, "Toggle POV", 0x0021020F),
    (Control::MenuMode, "Menu Mode", 0x000FFF0B),
    (Control::Rest, "Rest", 0x0014FF07),
    (Control::Vats, "Vats", 0x002FFF0E),
    (Control::Hotkey1, "Hotkey1", 0x0002FFFF),
    (Control::AmmoSwap, "Ammo Swap", 0x0003FF01),
    (Control::Hotkey3, "Hotkey3", 0x0004FFFF),
    (Control::Hotkey4, "Hotkey4", 0x0005FFFF),
    (Control::Hotkey5, "Hotkey5", 0x0006FFFF),
    (Control::Hotkey6, "Hotkey6", 0x0007FFFF),
    (Control::Hotkey7, "Hotkey7", 0x0008FFFF),
    (Control::Hotkey8, "Hotkey8", 0x0009FFFF),
    (Control::QuickSave, "QuickSave", 0x003FFFFF),
    (Control::QuickLoad, "QuickLoad", 0x0043FFFF),
    (Control::Grab, "Grab", 0x002CFF09),
    (Control::LowerWeapon, "Lower Weapon", 0x0023FFFF),
    (Control::Sprint, "Sprint", 0xFFFFFFFF),
    (Control::PhotoMode, "Photo Mode", 0xFFFFFFFF),
    (Control::HolsterWeapon, "Holster Weapon", 0xFFFFFFFF),
    (Control::ToggleLight, "Toggle Light", 0xFFFFFFFF),
    (Control::ToggleCrosshair, "Toggle Crosshair", 0xFFFFFFFF),
    (Control::ToggleSneakLabel, "Toggle Sneak Label", 0xFFFFFFFF),
    (Control::ToggleMenus, "Toggle Menus", 0xFFFFFFFF),
    (Control::ExitToMainMenu, "Exit to Main Menu", 0xFFFFFFFF),
    (Control::ExitGame, "Exit Game", 0xFFFFFFFF),
    (Control::NoCollision, "No Collision", 0xFFFFFFFF),
    (Control::PipBoyStats, "Pip-Boy Stats", 0xFFFFFFFF),
    (Control::PipBoyItems, "Pip-Boy Items", 0xFFFFFFFF),
    (Control::PipBoyData, "Pip-Boy Data", 0xFFFFFFFF),
    (Control::OpenLocalMap, "Local Map", 0xFFFFFFFF),
    (Control::OpenWorldMap, "World Map", 0xFFFFFFFF),
    (Control::OpenQuests, "Quests", 0xFFFFFFFF),
    (Control::OpenRadio, "Radio", 0xFFFFFFFF),
    (Control::ToggleIronSights, "Toggle Iron Sights", 0xFFFFFFFF),
    (Control::DropWeapon, "Drop Weapon", 0xFFFFFFFF),
    (Control::RadioVolumeUp, "Radio Volume Up", 0xFFFFFFFF),
    (Control::RadioVolumeDown, "Radio Volume Down", 0xFFFFFFFF),
    (Control::LastWeapon, "Last Weapon", 0xFFFFFFFF),
    (Control::SmoothCamera, "Smooth Camera", 0xFFFFFFFF),
    (Control::RepeatActivate, "Repeat Activate", 0xFFFFFFFF),
    (Control::ToggleSightingNode, "Toggle Sighting Node", 0xFFFFFFFF),
    (Control::PlaceMarkerHere, "Place Marker Here", 0xFFFFFFFF),
    (Control::HideHud, "Hide HUD", 0xFFFFFFFF),
    (Control::SkipRadioSong, "Skip Radio Song", 0xFFFFFFFF),
    // Bound by default, as before they were bindings (the keys as the INI codes them).
    (Control::Console, "Console", 0x0029FFFF),
    (Control::Screenshot, "Screenshot", 0x0058FFFF),
];

/// Where the Tweaks tab's keys start in [`GAME_CONTROLS`]: none is bound unless the
/// player binds it (`crate::tweaks`), but the console's and the screenshot's at the end
/// ([`BOUND_EXTRA_CONTROLS`]), bound by default.
pub const TWEAK_CONTROLS: usize = 31;

/// The controls after the Tweaks tab's that are bound by default: the console and the
/// screenshot ([`Control::Console`], [`Control::Screenshot`]).
pub const BOUND_EXTRA_CONTROLS: usize = 2;

/// How many of [`GAME_CONTROLS`] are the game's own (the extender's control codes 0–27).
pub const GAME_INI_CONTROLS: usize = 28;

/// The rest of the owner's `[Controls]`: `bAlwaysRunByDefault=1`, `bInvertYValues=0`,
/// `fMouseSensitivity=0.0020` [observed 2026-10-09].
pub const ALWAYS_RUN_BY_DEFAULT: bool = true;
pub const INVERT_Y: bool = false;
/// Radians of view turn per mouse count [hypothesis: the unit of `fMouseSensitivity`].
pub const MOUSE_SENSITIVITY: f32 = 0.0020;
/// The owner's `fXenonHorizLookSpeed=1500`, `fXenonVertLookSpeed=1200` [observed
/// 2026-10-16, `[Controls]` of `FalloutPrefs.ini`]. Read as milliradians a second with
/// the right stick pushed fully [hypothesis: the unit is not known; 1.5 rad/s turns
/// about 86° a second].
pub const LOOK_SPEED_HORIZONTAL: f32 = 1500.0;
pub const LOOK_SPEED_VERTICAL: f32 = 1200.0;
/// The owner's `bGamePadRumble=1` [observed 2026-10-16, `[Controls]` of
/// `FalloutPrefs.ini`].
pub const RUMBLE: bool = true;

/// Which device works the controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputDevice {
    #[default]
    KeyboardMouse,
    Controller,
}

impl InputDevice {
    pub fn name(self) -> &'static str {
        match self {
            InputDevice::KeyboardMouse => "Keyboard and Mouse",
            InputDevice::Controller => "Controller",
        }
    }

    pub fn parse(s: &str) -> Option<InputDevice> {
        [InputDevice::KeyboardMouse, InputDevice::Controller].into_iter().find(|d| d.name().eq_ignore_ascii_case(s.trim()))
    }

    pub fn other(self) -> InputDevice {
        match self {
            InputDevice::KeyboardMouse => InputDevice::Controller,
            InputDevice::Controller => InputDevice::KeyboardMouse,
        }
    }
}

/// The D-pad's controls where the game's values give none (module notes) [design].
pub const DPAD_HOTKEYS: [(Control, GamepadButton); 4] = [
    (Control::Hotkey1, GamepadButton::DPadUp),
    (Control::AmmoSwap, GamepadButton::DPadRight),
    (Control::Hotkey3, GamepadButton::DPadDown),
    (Control::Hotkey4, GamepadButton::DPadLeft),
];

/// The key of a DirectInput keyboard scan code [documented: Microsoft `DIK_*` values].
pub fn dik_key(code: u8) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match code {
        0x01 => Escape,
        0x02 => Digit1,
        0x03 => Digit2,
        0x04 => Digit3,
        0x05 => Digit4,
        0x06 => Digit5,
        0x07 => Digit6,
        0x08 => Digit7,
        0x09 => Digit8,
        0x0A => Digit9,
        0x0B => Digit0,
        0x0C => Minus,
        0x0D => Equal,
        0x0E => Backspace,
        0x0F => Tab,
        0x10 => KeyQ,
        0x11 => KeyW,
        0x12 => KeyE,
        0x13 => KeyR,
        0x14 => KeyT,
        0x15 => KeyY,
        0x16 => KeyU,
        0x17 => KeyI,
        0x18 => KeyO,
        0x19 => KeyP,
        0x1A => BracketLeft,
        0x1B => BracketRight,
        0x1C => Enter,
        0x1D => ControlLeft,
        0x1E => KeyA,
        0x1F => KeyS,
        0x20 => KeyD,
        0x21 => KeyF,
        0x22 => KeyG,
        0x23 => KeyH,
        0x24 => KeyJ,
        0x25 => KeyK,
        0x26 => KeyL,
        0x27 => Semicolon,
        0x28 => Quote,
        0x29 => Backquote,
        0x2A => ShiftLeft,
        0x2B => Backslash,
        0x2C => KeyZ,
        0x2D => KeyX,
        0x2E => KeyC,
        0x2F => KeyV,
        0x30 => KeyB,
        0x31 => KeyN,
        0x32 => KeyM,
        0x33 => Comma,
        0x34 => Period,
        0x35 => Slash,
        0x36 => ShiftRight,
        0x37 => NumpadMultiply,
        0x38 => AltLeft,
        0x39 => Space,
        0x3A => CapsLock,
        0x3B => F1,
        0x3C => F2,
        0x3D => F3,
        0x3E => F4,
        0x3F => F5,
        0x40 => F6,
        0x41 => F7,
        0x42 => F8,
        0x43 => F9,
        0x44 => F10,
        0x57 => F11,
        0x58 => F12,
        0x9D => ControlRight,
        0xB8 => AltRight,
        0xC8 => ArrowUp,
        0xCB => ArrowLeft,
        0xCD => ArrowRight,
        0xD0 => ArrowDown,
        _ => return None,
    })
}

/// The mouse button of an INI mouse byte (0 left, 1 right, 2 middle, 3–7 extra; `FF` none).
pub fn ini_mouse(code: u8) -> Option<MouseButton> {
    Some(match code {
        0 => MouseButton::Left,
        1 => MouseButton::Right,
        2 => MouseButton::Middle,
        3 => MouseButton::Back,
        4 => MouseButton::Forward,
        5..=7 => MouseButton::Other(u16::from(code)),
        _ => return None,
    })
}

/// A control's key and mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Binding {
    pub key: Option<KeyCode>,
    pub mouse: Option<MouseButton>,
}

impl Binding {
    /// From an INI value `00 KK MM JJ`.
    pub fn from_ini(v: u32) -> Binding {
        Binding { key: dik_key((v >> 16) as u8), mouse: ini_mouse((v >> 8) as u8) }
    }
}

/// The bindings in use.
#[derive(Resource, Debug, Clone)]
pub struct Bindings {
    map: Vec<(Control, Binding)>,
    pad: Vec<(Control, Option<PadInput>)>,
    pub always_run_by_default: bool,
    pub invert_y: bool,
    pub mouse_sensitivity: f32,
    pub device: InputDevice,
    /// Switch the saved device on use [design, #2200; `crate::pad`].
    pub automatic_input_switching: bool,
    /// Right-stick look speeds (see [`LOOK_SPEED_HORIZONTAL`]).
    pub look_speed: [f32; 2],
    /// The controller rumbles (`crate::rumble`).
    pub rumble: bool,
    /// The Controls tab's Stick Sensitivity: the right stick's look speeds times this.
    pub stick_sensitivity: f32,
    /// A key may serve two controls (the Duplicate Binds tweak, set by `crate::tweaks`):
    /// rebinding does not swap.
    pub allow_duplicates: bool,
    /// The Controls tab's Vertical Sensitivity of keyboard and mouse and of the controller
    /// ([`device_slot`]): the vertical look's speed as a multiple of the horizontal's, 1 as
    /// the game has it (the mouse's one value for both axes, the stick's own vertical look
    /// speed) [design: #2201; the original has no such setting].
    pub vertical: [f32; 2],
    /// The Controls tab's Aim Sensitivity of keyboard and mouse and of the controller
    /// ([`device_slot`]): the look's speed while the player aims down the sights or through
    /// a scope, as a multiple of the unzoomed look's; 1 leaves the look as it is unzoomed
    /// (`crate::player::aim_scale`) [design: owner 2026-11-15].
    pub aim: [f32; 2],
}

/// The place of `device` in [`Bindings::vertical`] and [`Bindings::aim`].
pub fn device_slot(device: InputDevice) -> usize {
    match device {
        InputDevice::KeyboardMouse => 0,
        InputDevice::Controller => 1,
    }
}

impl Default for Bindings {
    fn default() -> Self {
        Bindings::game_defaults()
    }
}

impl Bindings {
    /// The look sensitivity of the device in use as a multiple of the game's (the Controls
    /// tab's Mouse Sensitivity or Stick Sensitivity; 1 is the game's).
    pub fn sensitivity(&self) -> f32 {
        match self.device {
            InputDevice::KeyboardMouse => self.mouse_sensitivity / MOUSE_SENSITIVITY,
            InputDevice::Controller => self.stick_sensitivity,
        }
    }

    /// The Vertical Sensitivity of the device in use ([`Bindings::vertical`]).
    pub fn vertical_sensitivity(&self) -> f32 {
        self.vertical[device_slot(self.device)]
    }

    /// The Aim Sensitivity of the device in use ([`Bindings::aim`]).
    pub fn aim_sensitivity(&self) -> f32 {
        self.aim[device_slot(self.device)]
    }

    pub fn game_defaults() -> Bindings {
        Self::game_defaults_for_platform(cfg!(target_os = "android"))
    }

    pub fn game_defaults_for_platform(android: bool) -> Bindings {
        let look_sens = if android { 1.25 } else { 1.0 };
        Bindings {
            map: GAME_CONTROLS.iter().map(|&(c, _, v)| (c, Binding::from_ini(v))).collect(),
            pad: game_pad_defaults(),
            always_run_by_default: ALWAYS_RUN_BY_DEFAULT,
            invert_y: INVERT_Y,
            mouse_sensitivity: MOUSE_SENSITIVITY * look_sens,
            device: InputDevice::KeyboardMouse,
            automatic_input_switching: AUTOMATIC_INPUT_SWITCHING,
            look_speed: [LOOK_SPEED_HORIZONTAL, LOOK_SPEED_VERTICAL],
            rumble: RUMBLE,
            stick_sensitivity: 1.0,
            allow_duplicates: false,
            vertical: [look_sens, look_sens],
            aim: [1.0, 1.0],
        }
    }

    /// The control's controller input.
    pub fn pad(&self, c: Control) -> Option<PadInput> {
        self.pad.iter().find(|(k, _)| *k == c).and_then(|(_, p)| *p)
    }

    pub fn set_pad(&mut self, c: Control, p: Option<PadInput>) {
        match self.pad.iter_mut().find(|(k, _)| *k == c) {
            Some((_, x)) => *x = p,
            None => self.pad.push((c, p)),
        }
    }

    /// Bind controller input `p` to `c`; a control that had it gets `c`'s old one (a
    /// swap). The controls that gave it up.
    pub fn rebind_pad(&mut self, c: Control, p: PadInput) -> Vec<Control> {
        let old = self.pad(c);
        let mut swapped = Vec::new();
        let swaps = !self.allow_duplicates;
        for (k, x) in self.pad.iter_mut().filter(|(k, x)| swaps && *k != c && *x == Some(p)) {
            *x = old;
            swapped.push(*k);
        }
        self.set_pad(c, Some(p));
        swapped
    }

    /// What a prompt names for the control on the device in use: `[E]`, `[A]`, the shape
    /// of the built-in `sPCMenuHint*` labels [design, #4495: in place of the key and ")"
    /// read from the original's screen; with a controller NVForever writes the button's
    /// letter where the original shows its picture].
    pub fn prompt(&self, c: Control) -> String {
        let b = self.get(c);
        let name = match self.device {
            InputDevice::Controller => self.pad(c).map(|p| crate::pad::pad_shown(p).to_string()),
            InputDevice::KeyboardMouse => b.key.and_then(key_name).map(String::from).or_else(|| b.mouse.map(mouse_name)),
        };
        format!("[{}]", name.unwrap_or_else(|| "?".into()))
    }

    /// What the Controls tab shows for the control on the device in use.
    pub fn text_on(&self, c: Control) -> String {
        match self.device {
            InputDevice::KeyboardMouse => self.get(c).text(),
            // The button style's names (#1452); the INI keeps the Xbox ones ([`pad_text`]).
            InputDevice::Controller => self.pad(c).map_or_else(|| "None".into(), |p| crate::pad::pad_shown(p).to_string()),
        }
    }

    pub fn get(&self, c: Control) -> Binding {
        self.map.iter().find(|(k, _)| *k == c).map_or_else(Binding::default, |(_, b)| *b)
    }

    /// The control's name in the game's INI.
    pub fn name(c: Control) -> &'static str {
        GAME_CONTROLS.iter().find(|(k, _, _)| *k == c).map_or("?", |(_, n, _)| n)
    }

    /// The control of an INI name (without case).
    pub fn by_name(name: &str) -> Option<Control> {
        GAME_CONTROLS.iter().find(|(_, n, _)| n.eq_ignore_ascii_case(name.trim())).map(|(c, _, _)| *c)
    }

    pub fn set(&mut self, c: Control, b: Binding) {
        match self.map.iter_mut().find(|(k, _)| *k == c) {
            Some((_, x)) => *x = b,
            None => self.map.push((c, b)),
        }
    }

    /// Bind `input` to `c`; any other control that had it gets `c`'s old input of that
    /// kind instead (a swap). The controls that gave it up.
    pub fn rebind(&mut self, c: Control, input: Input) -> Vec<Control> {
        let old = self.get(c);
        let mut swapped = Vec::new();
        // The player's tweak lets two controls share an input [design: docs/design/tweaks.md].
        let swaps = !self.allow_duplicates;
        for (k, b) in self.map.iter_mut().filter(|(k, _)| swaps && *k != c) {
            match input {
                Input::Key(key) if b.key == Some(key) => b.key = old.key,
                Input::Mouse(m) if b.mouse == Some(m) => b.mouse = old.mouse,
                _ => continue,
            }
            swapped.push(*k);
        }
        let mut new = old;
        match input {
            Input::Key(key) => new.key = Some(key),
            Input::Mouse(m) => new.mouse = Some(m),
        }
        self.set(c, new);
        swapped
    }

    /// Every control's binding, in the INI's order.
    pub fn all(&self) -> impl Iterator<Item = (Control, Binding)> + '_ {
        GAME_CONTROLS.iter().map(|&(c, _, _)| (c, self.get(c)))
    }
}

/// The game's controller buttons (`JJ`), then the D-pad's hotkeys where it gave none.
fn game_pad_defaults() -> Vec<(Control, Option<PadInput>)> {
    let mut v: Vec<(Control, Option<PadInput>)> = GAME_CONTROLS.iter().map(|&(c, _, x)| (c, crate::pad::game_pad(x as u8))).collect();
    for (c, b) in DPAD_HOTKEYS {
        if let Some((_, x @ None)) = v.iter_mut().find(|(k, _)| *k == c) {
            *x = Some(PadInput::Button(b));
        }
    }
    v
}

/// A controller input as the INI writes it; `None` for none.
pub fn pad_text(p: Option<PadInput>) -> String {
    p.map_or_else(|| "None".into(), |p| crate::pad::pad_name(p).to_string())
}

/// The control's name on the Controls page [design: what it does in NVForever, not the
/// original menu's wording].
pub fn label(c: Control) -> &'static str {
    use Control::*;
    match c {
        Forward => "Forward",
        Back => "Back",
        SlideLeft => "Strafe Left",
        SlideRight => "Strafe Right",
        Use => "Attack",
        Activate => "Activate",
        Block => "Aim / Block",
        ReadyItem => "Ready / Reload",
        CrouchSneak => "Sneak",
        Run => "Run / Walk",
        AlwaysRun => "Always Run",
        AutoMove => "Auto Move",
        Jump => "Jump",
        TogglePov => "Toggle View",
        MenuMode => "Pip-Boy",
        Rest => "Wait",
        Vats => "V.A.T.S.",
        Hotkey1 => "Hotkey 1",
        AmmoSwap => "Ammo Swap",
        Hotkey3 => "Hotkey 3",
        Hotkey4 => "Hotkey 4",
        Hotkey5 => "Hotkey 5",
        Hotkey6 => "Hotkey 6",
        Hotkey7 => "Hotkey 7",
        Hotkey8 => "Hotkey 8",
        QuickSave => "Quick Save",
        QuickLoad => "Quick Load",
        Grab => "Grab",
        LowerWeapon => "Lower Weapon",
        Sprint => "Sprint",
        PhotoMode => "Photo Mode",
        HolsterWeapon => "Holster Weapon",
        ToggleLight => "Pip-Boy Light",
        ToggleCrosshair => "Show/Hide Crosshair",
        ToggleSneakLabel => "Show/Hide Sneak Label",
        ToggleMenus => "Show/Hide HUD",
        ExitToMainMenu => "Main Menu",
        ExitGame => "Quit Game",
        NoCollision => "No Collision (held)",
        PipBoyStats => "Pip-Boy: Stats",
        PipBoyItems => "Pip-Boy: Items",
        PipBoyData => "Pip-Boy: Data",
        OpenLocalMap => "Pip-Boy: Local Map",
        OpenWorldMap => "Pip-Boy: World Map",
        OpenQuests => "Pip-Boy: Quests",
        OpenRadio => "Pip-Boy: Radio",
        ToggleIronSights => "True Iron Sights",
        DropWeapon => "Drop Weapon",
        RadioVolumeUp => "Radio Volume Up",
        RadioVolumeDown => "Radio Volume Down",
        LastWeapon => "Last Weapon",
        SmoothCamera => "Smooth Camera",
        RepeatActivate => "Repeat Activate",
        ToggleSightingNode => "Second Iron Sights",
        PlaceMarkerHere => "Place Marker Here",
        HideHud => "Hide HUD",
        SkipRadioSong => "Skip Radio Song",
        Console => "Console",
        Screenshot => "Screenshot",
    }
}

/// A key or a mouse button, as the Controls page takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Key(KeyCode),
    Mouse(MouseButton),
}

/// The keys a control can be bound to and their names in the INI and on the Controls
/// page [design]. Punctuation is named in words, so a value's comma only ever parts the
/// key from the mouse button.
pub const KEY_NAMES: &[(KeyCode, &str)] = {
    use KeyCode::*;
    &[
        (KeyA, "A"), (KeyB, "B"), (KeyC, "C"), (KeyD, "D"), (KeyE, "E"), (KeyF, "F"), (KeyG, "G"),
        (KeyH, "H"), (KeyI, "I"), (KeyJ, "J"), (KeyK, "K"), (KeyL, "L"), (KeyM, "M"), (KeyN, "N"),
        (KeyO, "O"), (KeyP, "P"), (KeyQ, "Q"), (KeyR, "R"), (KeyS, "S"), (KeyT, "T"), (KeyU, "U"),
        (KeyV, "V"), (KeyW, "W"), (KeyX, "X"), (KeyY, "Y"), (KeyZ, "Z"),
        (Digit1, "1"), (Digit2, "2"), (Digit3, "3"), (Digit4, "4"), (Digit5, "5"),
        (Digit6, "6"), (Digit7, "7"), (Digit8, "8"), (Digit9, "9"), (Digit0, "0"),
        (F1, "F1"), (F2, "F2"), (F3, "F3"), (F4, "F4"), (F5, "F5"), (F6, "F6"),
        (F7, "F7"), (F8, "F8"), (F9, "F9"), (F10, "F10"), (F11, "F11"), (F12, "F12"),
        // The console's key, a binding now (`Control::Console`).
        (Backquote, "`"),
        (Space, "Space"), (Tab, "Tab"), (Enter, "Enter"), (Backspace, "Backspace"), (CapsLock, "Caps Lock"),
        (ShiftLeft, "Left Shift"), (ShiftRight, "Right Shift"), (ControlLeft, "Left Ctrl"), (ControlRight, "Right Ctrl"),
        (AltLeft, "Left Alt"), (AltRight, "Right Alt"),
        (ArrowUp, "Up"), (ArrowDown, "Down"), (ArrowLeft, "Left"), (ArrowRight, "Right"),
        (Insert, "Insert"), (Delete, "Delete"), (Home, "Home"), (End, "End"), (PageUp, "Page Up"), (PageDown, "Page Down"),
        (Minus, "Minus"), (Equal, "Equals"), (BracketLeft, "Left Bracket"), (BracketRight, "Right Bracket"),
        (Semicolon, "Semicolon"), (Quote, "Apostrophe"), (Backslash, "Backslash"), (Comma, "Comma"), (Period, "Period"), (Slash, "Slash"),
        (Numpad0, "Num 0"), (Numpad1, "Num 1"), (Numpad2, "Num 2"), (Numpad3, "Num 3"), (Numpad4, "Num 4"),
        (Numpad5, "Num 5"), (Numpad6, "Num 6"), (Numpad7, "Num 7"), (Numpad8, "Num 8"), (Numpad9, "Num 9"),
        (NumpadMultiply, "Num *"), (NumpadAdd, "Num +"), (NumpadSubtract, "Num -"), (NumpadDivide, "Num /"),
        (NumpadDecimal, "Num ."), (NumpadEnter, "Num Enter"),
    ]
};

/// The mouse buttons' names (the INI's 0, 1, 2, 3, 4; more as `Mouse 6`…).
pub const MOUSE_NAMES: [(MouseButton, &str); 5] = [
    (MouseButton::Left, "Left Mouse"),
    (MouseButton::Right, "Right Mouse"),
    (MouseButton::Middle, "Middle Mouse"),
    (MouseButton::Back, "Mouse 4"),
    (MouseButton::Forward, "Mouse 5"),
];

/// Keys no control may take: Esc (the pause menu). The console's key is a binding of its own
/// now ([`Control::Console`]).
pub const RESERVED_KEYS: [KeyCode; 1] = [KeyCode::Escape];

/// Whether `c`'s key or mouse button was pressed this frame, by `bindings` (none: never);
/// for the controls read outside the game's own control handling (the console, the
/// screenshot).
pub fn binding_just_pressed(bindings: Option<&Bindings>, c: Control, keys: impl Fn(KeyCode) -> bool, mouse: impl Fn(MouseButton) -> bool) -> bool {
    bindings.is_some_and(|b| {
        let x = b.get(c);
        x.key.is_some_and(&keys) || x.mouse.is_some_and(&mouse)
    })
}

pub fn key_name(k: KeyCode) -> Option<&'static str> {
    KEY_NAMES.iter().find(|(c, _)| *c == k).map(|(_, n)| *n)
}

pub fn mouse_name(m: MouseButton) -> String {
    match MOUSE_NAMES.iter().find(|(b, _)| *b == m) {
        Some((_, n)) => n.to_string(),
        None => match m {
            MouseButton::Other(n) => format!("Mouse {}", n + 1),
            _ => "Mouse ?".into(),
        },
    }
}

/// The input of a name (without case).
pub fn parse_input(name: &str) -> Option<Input> {
    let name = name.trim();
    if let Some((k, _)) = KEY_NAMES.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)) {
        return Some(Input::Key(*k));
    }
    if let Some((m, _)) = MOUSE_NAMES.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)) {
        return Some(Input::Mouse(*m));
    }
    let n: u16 = name.get(..6).filter(|p| p.eq_ignore_ascii_case("mouse ")).and_then(|_| name[6..].trim().parse().ok())?;
    (6..=16).contains(&n).then_some(Input::Mouse(MouseButton::Other(n - 1)))
}

impl Binding {
    /// As the INI and the Controls page write it: `W`, `Left Alt, Right Mouse`, `None`.
    pub fn text(&self) -> String {
        let parts: Vec<String> = self.key.and_then(key_name).map(String::from).into_iter().chain(self.mouse.map(mouse_name)).collect();
        if parts.is_empty() {
            "None".into()
        } else {
            parts.join(", ")
        }
    }

    /// From [`Binding::text`]: at most one key and one mouse button; `None` or nothing
    /// for neither.
    pub fn parse(text: &str) -> Result<Binding, String> {
        let mut b = Binding::default();
        for part in text.split(',').map(str::trim).filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("none")) {
            match parse_input(part) {
                Some(Input::Key(k)) if RESERVED_KEYS.contains(&k) => return Err(format!("{part:?} is kept for the menus")),
                Some(Input::Key(_)) if b.key.is_some() => return Err(format!("{part:?}: one key only")),
                Some(Input::Mouse(_)) if b.mouse.is_some() => return Err(format!("{part:?}: one mouse button only")),
                Some(Input::Key(k)) => b.key = Some(k),
                Some(Input::Mouse(m)) => b.mouse = Some(m),
                None => return Err(format!("{part:?} is not a key or mouse button name")),
            }
        }
        Ok(b)
    }
}

/// The INI section the controls are kept in.
pub const SECTION: &str = "Controls";
const ALWAYS_RUN_KEY: &str = "Always Run By Default";
const INVERT_Y_KEY: &str = "Invert Y";
const SENSITIVITY_KEY: &str = "Mouse Sensitivity";
const DEVICE_KEY: &str = "Input Device";
/// Default policy, with a Controls opt-out [design, #2200].
pub const AUTOMATIC_INPUT_SWITCHING: bool = true;
pub const AUTOMATIC_INPUT_KEY: &str = "Automatic Input Switching";
/// The INI section the controller's buttons are kept in.
pub const PAD_SECTION: &str = "Controller";
const LOOK_KEYS: [&str; 2] = ["Look Speed Horizontal", "Look Speed Vertical"];
pub const RUMBLE_KEY: &str = "Rumble";
const STICK_SENSITIVITY_KEY: &str = "Stick Sensitivity";
/// Where each device's Vertical Sensitivity is kept ([`Bindings::vertical`]): a multiple,
/// two decimals.
const VERTICAL_KEYS: [(&str, &str); 2] = [(SECTION, "Mouse Vertical Sensitivity"), (PAD_SECTION, "Stick Vertical Sensitivity")];
/// Where each device's Aim Sensitivity is kept ([`Bindings::aim`]): a multiple, two
/// decimals, under the same name in each device's section.
const AIM_KEYS: [(&str, &str); 2] = [(SECTION, "Mouse Aim Sensitivity"), (PAD_SECTION, "Stick Aim Sensitivity")];

fn bit(on: bool) -> &'static str {
    if on {
        "1"
    } else {
        "0"
    }
}

/// The INI's `Mouse Sensitivity`: four decimals as before (`0.0020`), more where the low
/// end of the slider needs them (6% is `0.00012`), trailing zeros past the fourth dropped.
fn mouse_sensitivity_text(s: f32) -> String {
    let mut t = format!("{s:.6}");
    while t.ends_with('0') && t.split_once('.').is_some_and(|(_, f)| f.len() > 4) {
        t.pop();
    }
    t
}

/// `text` (an INI, or empty) with every control in `[Controls]`.
pub fn with_controls(text: &str, b: &Bindings) -> String {
    let mut t = b.all().fold(text.to_string(), |t, (c, x)| ini_set(&t, SECTION, Bindings::name(c), &x.text()));
    t = ini_set(&t, SECTION, ALWAYS_RUN_KEY, bit(b.always_run_by_default));
    t = ini_set(&t, SECTION, INVERT_Y_KEY, bit(b.invert_y));
    t = ini_set(&t, SECTION, SENSITIVITY_KEY, &mouse_sensitivity_text(b.mouse_sensitivity));
    t = ini_set(&t, SECTION, DEVICE_KEY, b.device.name());
    t = ini_set(&t, SECTION, AUTOMATIC_INPUT_KEY, bit(b.automatic_input_switching));
    t = b.all().fold(t, |t, (c, _)| ini_set(&t, PAD_SECTION, Bindings::name(c), &pad_text(b.pad(c))));
    for (k, v) in LOOK_KEYS.iter().zip(b.look_speed) {
        t = ini_set(&t, PAD_SECTION, k, &format!("{v:.0}"));
    }
    t = ini_set(&t, PAD_SECTION, STICK_SENSITIVITY_KEY, &format!("{:.2}", b.stick_sensitivity));
    for ((section, k), v) in VERTICAL_KEYS.iter().zip(b.vertical).chain(AIM_KEYS.iter().zip(b.aim)) {
        t = ini_set(&t, section, k, &format!("{v:.2}"));
    }
    ini_set(&t, PAD_SECTION, RUMBLE_KEY, bit(b.rumble))
}

/// What `[Controls]` of `text` changes in `b`: the lines found, and what changed
/// (`Jump Space -> J`); values that do not parse go to `bad`.
pub fn read_controls(text: &str, b: &mut Bindings, bad: &mut Vec<String>) -> (usize, Vec<String>) {
    let (mut found, mut changed) = (0, Vec::new());
    for (c, name, _) in GAME_CONTROLS {
        let Some(v) = ini_get(text, SECTION, name) else { continue };
        found += 1;
        match Binding::parse(&v) {
            Ok(x) if x != b.get(c) => {
                changed.push(format!("{name} {} -> {}", b.get(c).text(), x.text()));
                b.set(c, x);
            }
            Ok(_) => {}
            Err(e) => bad.push(format!("{name}={v} ({e})")),
        }
    }
    let switch = |section: &str, key: &str, at: &mut bool, found: &mut usize, changed: &mut Vec<String>, bad: &mut Vec<String>| {
        let Some(v) = ini_get(text, section, key) else { return };
        *found += 1;
        let shown = if section == SECTION { key.to_string() } else { format!("[{section}] {key}") };
        match v.trim() {
            "1" | "0" => {
                let on = v.trim() == "1";
                if on != *at {
                    changed.push(format!("{shown} {}", bit(on)));
                    *at = on;
                }
            }
            _ => bad.push(format!("{shown}={v} (not 1 or 0)")),
        }
    };
    switch(SECTION, ALWAYS_RUN_KEY, &mut b.always_run_by_default, &mut found, &mut changed, bad);
    switch(SECTION, INVERT_Y_KEY, &mut b.invert_y, &mut found, &mut changed, bad);
    switch(SECTION, AUTOMATIC_INPUT_KEY, &mut b.automatic_input_switching, &mut found, &mut changed, bad);
    if let Some(v) = ini_get(text, SECTION, SENSITIVITY_KEY) {
        found += 1;
        match v.trim().parse::<f32>() {
            Ok(s) if s.is_finite() && s > 0.0 => {
                // Below the slider's least (a hand edit) it is brought up to it, not refused.
                let s = clamp_to_least(s, SENSITIVITY_LEAST * MOUSE_SENSITIVITY);
                if (s - b.mouse_sensitivity).abs() > 1e-7 {
                    changed.push(format!("{SENSITIVITY_KEY} {s}"));
                    b.mouse_sensitivity = s;
                }
            }
            _ => bad.push(format!("{SENSITIVITY_KEY}={v} (not a number above 0)")),
        }
    }
    if let Some(v) = ini_get(text, SECTION, DEVICE_KEY) {
        found += 1;
        match InputDevice::parse(&v) {
            Some(d) if d != b.device => {
                changed.push(format!("{DEVICE_KEY} {}", d.name()));
                b.device = d;
            }
            Some(_) => {}
            None => bad.push(format!("{DEVICE_KEY}={v} (not \"Keyboard and Mouse\" or \"Controller\")")),
        }
    }
    for (c, name, _) in GAME_CONTROLS {
        let Some(v) = ini_get(text, PAD_SECTION, name) else { continue };
        found += 1;
        let p = match v.trim() {
            n if n.is_empty() || n.eq_ignore_ascii_case("none") => Ok(None),
            n => match crate::pad::parse_pad(n) {
                Some(p) if crate::pad::RESERVED_PAD.contains(&p) => Err(format!("{n:?} is kept for the pause menu")),
                Some(p) => Ok(Some(p)),
                None => Err(format!("{n:?} is not a controller button name")),
            },
        };
        match p {
            Ok(p) if p != b.pad(c) => {
                changed.push(format!("[{PAD_SECTION}] {name} {} -> {}", pad_text(b.pad(c)), pad_text(p)));
                b.set_pad(c, p);
            }
            Ok(_) => {}
            Err(e) => bad.push(format!("[{PAD_SECTION}] {name}={v} ({e})")),
        }
    }
    for (i, k) in LOOK_KEYS.iter().enumerate() {
        let Some(v) = ini_get(text, PAD_SECTION, k) else { continue };
        found += 1;
        match v.trim().parse::<f32>() {
            Ok(s) if s.is_finite() && s > 0.0 => {
                if (s - b.look_speed[i]).abs() > 1e-3 {
                    changed.push(format!("{k} {s}"));
                    b.look_speed[i] = s;
                }
            }
            _ => bad.push(format!("[{PAD_SECTION}] {k}={v} (not a number above 0)")),
        }
    }
    if let Some(v) = ini_get(text, PAD_SECTION, STICK_SENSITIVITY_KEY) {
        found += 1;
        match v.trim().parse::<f32>() {
            Ok(m) if m.is_finite() && m > 0.0 => {
                let m = clamp_to_least(m, SENSITIVITY_LEAST);
                if (m - b.stick_sensitivity).abs() > 1e-4 {
                    changed.push(format!("[{PAD_SECTION}] {STICK_SENSITIVITY_KEY} {m}"));
                    b.stick_sensitivity = m;
                }
            }
            _ => bad.push(format!("[{PAD_SECTION}] {STICK_SENSITIVITY_KEY}={v} (not a number above 0)")),
        }
    }
    // Each device's Vertical Sensitivity and Aim Sensitivity: multiples on the sliders' floor.
    for (keys, values) in [(&VERTICAL_KEYS, &mut b.vertical), (&AIM_KEYS, &mut b.aim)] {
        for (i, (section, k)) in keys.iter().enumerate() {
            let Some(v) = ini_get(text, section, k) else { continue };
            found += 1;
            let name = if *section == PAD_SECTION { format!("[{PAD_SECTION}] {k}") } else { k.to_string() };
            match v.trim().parse::<f32>() {
                Ok(m) if m.is_finite() && m > 0.0 => {
                    let m = clamp_to_least(m, SENSITIVITY_LEAST);
                    if (m - values[i]).abs() > 1e-4 {
                        changed.push(format!("{name} {m}"));
                        values[i] = m;
                    }
                }
                _ => bad.push(format!("{name}={v} (not a number above 0)")),
            }
        }
    }
    switch(PAD_SECTION, RUMBLE_KEY, &mut b.rumble, &mut found, &mut changed, bad);
    (found, changed)
}

/// The number of `[Controls]` lines a full section has.
const LINES: usize = 2 * GAME_CONTROLS.len() + 5 + LOOK_KEYS.len() + 2 + VERTICAL_KEYS.len() + AIM_KEYS.len();

/// Read the player's keys from the INI (module notes) and fill in the lines it lacks.
pub fn apply_stored(world: &mut World) {
    let Some(path) = world.get_resource::<crate::settings::SettingsFile>().and_then(|f| f.0.clone()) else { return };
    if !world.contains_resource::<Bindings>() {
        return;
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            warn!("controls: {} not read: {e}", path.display());
            return;
        }
    };
    let mut bad = Vec::new();
    let (found, changed) = read_controls(&text, &mut world.resource_mut::<Bindings>(), &mut bad);
    let always_run = world.resource::<Bindings>().always_run_by_default;
    if let Some(mut c) = world.get_resource_mut::<crate::control::PlayerControl>() {
        c.always_run = always_run;
    }
    for b in &bad {
        warn!("controls: {b} in {}; the default kept", path.display());
    }
    let what = if changed.is_empty() { String::new() } else { format!(": {}", changed.join(", ")) };
    info!("controls: {found} of {LINES} line(s) read from {}, {} changed{what}; input device {}", path.display(), changed.len(), world.resource::<Bindings>().device.name());
    if found < LINES {
        store(world);
        info!("controls: the {} missing line(s) written to [{SECTION}] and [{PAD_SECTION}]", LINES - found);
    }
}

/// Whether `c`'s row is greyed now (`crate::stance::binding_greyed`): it is not rebound or
/// cleared, and the log says why.
fn greyed(world: &World, c: Control) -> bool {
    match crate::stance::binding_greyed(world, c) {
        Some(why) => {
            info!("controls: {} not changed: greyed ({why})", Bindings::name(c));
            true
        }
        None => false,
    }
}

/// Bind `input` to `c` (the Controls page), log it and keep it in the INI.
pub fn rebind(world: &mut World, c: Control, input: Input) {
    if greyed(world, c) {
        return;
    }
    let Some(mut b) = world.get_resource_mut::<Bindings>() else { return };
    let before = b.get(c).text();
    let swapped = b.rebind(c, input);
    let after = b.get(c).text();
    let gave: Vec<String> = swapped.iter().map(|&o| format!("{} now {}", Bindings::name(o), b.get(o).text())).collect();
    let gave = if gave.is_empty() { String::new() } else { format!(" ({})", gave.join(", ")) };
    info!("controls: {} bound to {after} (was {before}){gave}", Bindings::name(c));
    cancel_input_holds(world);
    store(world);
}

/// Bind controller input `p` to `c` (the Controls page with Controller chosen).
pub fn rebind_pad(world: &mut World, c: Control, p: PadInput) {
    if greyed(world, c) {
        return;
    }
    let Some(mut b) = world.get_resource_mut::<Bindings>() else { return };
    let before = pad_text(b.pad(c));
    let swapped = b.rebind_pad(c, p);
    let gave: Vec<String> = swapped.iter().map(|&o| format!("{} now {}", Bindings::name(o), pad_text(b.pad(o)))).collect();
    let gave = if gave.is_empty() { String::new() } else { format!(" ({})", gave.join(", ")) };
    info!("controls: {} bound to controller {} (was {before}){gave}", Bindings::name(c), crate::pad::pad_name(p));
    cancel_input_holds(world);
    store(world);
}

/// Switch the device that works the controls (the Controls tab's first row).
pub fn set_device(world: &mut World, d: InputDevice) {
    let Some(mut b) = world.get_resource_mut::<Bindings>() else { return };
    let changed = b.device != d;
    b.device = d;
    // Cancel pending release actions before readers can interpret the device change
    // as a completed tap/selection. Hardware releases remain visible [design, #2200].
    if changed { cancel_input_holds(world); }
    info!("controls: input device {}", d.name());
    store(world);
}

/// Cancel release-triggered actions after a device is lost or input is invalidated.
/// Input backends use this without changing the player's saved device choice.
pub fn cancel_input_holds(world: &mut World) {
    crate::controller_aim::cancel(world);
    if let Some(mut control) = world.get_resource_mut::<crate::control::PlayerControl>() {
        control.cancel_input_holds();
    }
    if let Some(mut grab) = world.get_resource_mut::<crate::grab::GrabState>() {
        grab.tap = None;
    }
    if let Some(mut wheel) = world.get_resource_mut::<crate::weapon_wheel::WeaponWheel>() {
        wheel.cancel_input_hold();
    }
    let mut pointers = world.query::<&mut crate::ui_pointer::PointerState>();
    for mut pointer in pointers.iter_mut(world) {
        if *pointer == crate::ui_pointer::PointerState::Pressed {
            *pointer = crate::ui_pointer::PointerState::None;
        }
    }
}

/// Set the Controls opt-out, saved with the device [design, #2200].
pub fn set_automatic_input_switching(world: &mut World, on: bool) {
    let Some(mut b) = world.get_resource_mut::<Bindings>() else { return };
    b.automatic_input_switching = on;
    info!("controls: automatic input switching {}", if on { "on" } else { "off" });
    store(world);
}

/// Turn the controller's rumble on or off (the Controls tab's Rumble row).
pub fn set_rumble(world: &mut World, on: bool) {
    let Some(mut b) = world.get_resource_mut::<Bindings>() else { return };
    b.rumble = on;
    info!("controls: controller rumble {}", if on { "on" } else { "off" });
    store(world);
}

/// Turn the vertical look's inversion on or off (the Controls tab's Invert Y row).
pub fn set_invert_y(world: &mut World, on: bool) {
    let Some(mut b) = world.get_resource_mut::<Bindings>() else { return };
    b.invert_y = on;
    info!("controls: invert Y {}", if on { "on" } else { "off" });
    store(world);
}

/// The Controls tab's Mouse Sensitivity and Stick Sensitivity sliders, as multiples of the
/// game's (`fMouseSensitivity`, the right stick's look speeds): from a twentieth to three
/// times [design: owner 2026-11-03, #1451: NVForever's own range, not the original
/// slider's, to be tuned by playing; the mouse and the stick set apart. #1632: the least
/// lowered from 25% to 5%, as 25% still turned a full circle in about 20 cm at 1600 dpi;
/// 5% is about 100 cm, 100% about 5 cm, 300% about 1.7 cm, reading `fMouseSensitivity` as
/// radians a count (hypothesis, see [`MOUSE_SENSITIVITY`])].
pub const SENSITIVITY_LEAST: f32 = 0.05;
pub const SENSITIVITY_MOST: f32 = 3.0;

/// The sliders' steps in percent [design, #1632]: about a tenth apart (a step changes the
/// turn by about the same share anywhere on the slider), whole percents at the bottom, and
/// the round values the slider had before (50%, 100%, 150%, ...) kept.
pub const SENSITIVITY_STEPS: [u16; 44] = [
    5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 20, 22, 25, 28, 32, 36, 40, 45, 50, 55, 60, 65, 70, 75, 80, 85, 90, 95, 100, 110, 120, 130,
    140, 150, 165, 180, 200, 225, 250, 275, 300,
];

/// `v` raised to `least` when below it (a value read from the INI under the slider's least
/// is clamped, not refused); a hair under is left as it is (an INI's rounding).
fn clamp_to_least(v: f32, least: f32) -> f32 {
    if v < least * (1.0 - 1e-4) {
        least
    } else {
        v
    }
}

/// A sensitivity multiple as the Controls tab shows it: a percentage of the game's.
pub fn sensitivity_text(m: f32) -> String {
    format!("{:.0}%", m * 100.0)
}

/// The index of the step of [`SENSITIVITY_STEPS`] nearest multiple `m` (on the
/// logarithmic scale the meter uses).
fn nearest_step(m: f32) -> usize {
    let at = m.max(SENSITIVITY_LEAST / 2.0).ln();
    let off = |p: u16| (f32::from(p) / 100.0).ln() - at;
    (0..SENSITIVITY_STEPS.len()).min_by(|&a, &b| off(SENSITIVITY_STEPS[a]).abs().total_cmp(&off(SENSITIVITY_STEPS[b]).abs())).unwrap_or(0)
}

/// The sensitivity `steps` of [`SENSITIVITY_STEPS`] from multiple `m` (from the step
/// nearest it), on the steps and in the sliders' range.
pub fn step_sensitivity(m: f32, steps: i32) -> f32 {
    let last = SENSITIVITY_STEPS.len() as i32 - 1;
    let at = (nearest_step(m) as i32 + steps).clamp(0, last);
    f32::from(SENSITIVITY_STEPS[at as usize]) / 100.0
}

/// Multiple `m` as a part of the slider's meter (0 at the least, 1 at the most), on a
/// logarithmic scale so the slow end, where a few percent matter, has room [design,
/// #1632].
pub fn sensitivity_part(m: f32) -> f32 {
    ((m.max(f32::MIN_POSITIVE) / SENSITIVITY_LEAST).ln() / (SENSITIVITY_MOST / SENSITIVITY_LEAST).ln()).clamp(0.0, 1.0)
}

/// The multiple at part `p` of the slider's meter, on the steps.
pub fn sensitivity_at_part(p: f32) -> f32 {
    step_sensitivity(SENSITIVITY_LEAST * (SENSITIVITY_MOST / SENSITIVITY_LEAST).powf(p.clamp(0.0, 1.0)), 0)
}

/// Set the look sensitivity of the device in use to multiple `m` of the game's (the
/// Controls tab's Mouse Sensitivity or Stick Sensitivity); acts at once.
pub fn set_sensitivity(world: &mut World, m: f32) {
    let Some(mut b) = world.get_resource_mut::<Bindings>() else { return };
    if (b.sensitivity() - m).abs() <= 1e-4 {
        return;
    }
    if b.device == InputDevice::Controller {
        b.stick_sensitivity = m;
        info!("controls: stick sensitivity {}", sensitivity_text(m));
    } else {
        b.mouse_sensitivity = m * MOUSE_SENSITIVITY;
        info!("controls: mouse sensitivity {} ({:.4})", sensitivity_text(m), b.mouse_sensitivity);
    }
    store(world);
}

/// Set the Vertical Sensitivity of the device in use to multiple `m` of its horizontal
/// look (the Controls tab's Vertical Sensitivity, on the steps of the sensitivity sliders);
/// acts at once.
pub fn set_vertical(world: &mut World, m: f32) {
    let Some(mut b) = world.get_resource_mut::<Bindings>() else { return };
    let i = device_slot(b.device);
    if (b.vertical[i] - m).abs() <= 1e-4 {
        return;
    }
    b.vertical[i] = m;
    info!("controls: {} vertical sensitivity {}", if i == 1 { "stick" } else { "mouse" }, sensitivity_text(m));
    store(world);
}

/// Set the Aim Sensitivity of the device in use to multiple `m` of its unzoomed look (the
/// Controls tab's Aim Sensitivity, on the steps of the sensitivity sliders); acts at once.
pub fn set_aim_sensitivity(world: &mut World, m: f32) {
    let Some(mut b) = world.get_resource_mut::<Bindings>() else { return };
    let i = device_slot(b.device);
    if (b.aim[i] - m).abs() <= 1e-4 {
        return;
    }
    b.aim[i] = m;
    info!("controls: {} aim sensitivity {}", if i == 1 { "stick" } else { "mouse" }, sensitivity_text(m));
    store(world);
}

/// Take the key off `c` (the Controls page's Delete), keeping its mouse button.
pub fn clear_key(world: &mut World, c: Control) {
    if greyed(world, c) {
        return;
    }
    let Some(mut b) = world.get_resource_mut::<Bindings>() else { return };
    if b.device == InputDevice::Controller {
        b.set_pad(c, None);
        info!("controls: {} controller button cleared", Bindings::name(c));
        store(world);
        return;
    }
    let mut x = b.get(c);
    x.key = None;
    b.set(c, x);
    info!("controls: {} key cleared, now {}", Bindings::name(c), x.text());
    store(world);
}

/// Every control back to the game's default keys, or with Controller chosen its
/// controller buttons (the Controls page's Defaults); the look settings are kept.
pub fn reset(world: &mut World) {
    let Some(mut b) = world.get_resource_mut::<Bindings>() else { return };
    let d = Bindings::game_defaults();
    if b.device == InputDevice::Controller {
        b.pad = d.pad;
        info!("controls: every control back to the game's controller buttons");
    } else {
        b.map = d.map;
        info!("controls: every control back to the game's keys");
    }
    store(world);
}

/// The Controls tab's rows away from the game's defaults ([`Bindings::game_defaults`]),
/// of both devices, in the order of `crate::settings_view::ControlRow::all`: the Changed
/// list's Controls rows [design: #1953 point 6]. A control's key and mouse button and its
/// controller button are each their own row; the sensitivities are compared as the sliders
/// show them (to a ten-thousandth of the game's).
pub fn changed_controls(b: &Bindings) -> Vec<crate::settings_view::ControlRow> {
    use crate::settings_view::ControlRow as R;
    let d = Bindings::game_defaults();
    let same = |row: R| match row {
        R::Device => b.device == d.device,
        R::AutomaticInput => b.automatic_input_switching == d.automatic_input_switching,
        R::Binding(c, InputDevice::KeyboardMouse) => b.get(c) == d.get(c),
        R::Binding(c, InputDevice::Controller) => b.pad(c) == d.pad(c),
        R::Invert => b.invert_y == d.invert_y,
        R::Sensitivity(InputDevice::KeyboardMouse) => ((b.mouse_sensitivity - d.mouse_sensitivity) / MOUSE_SENSITIVITY).abs() <= 1e-4,
        R::Sensitivity(InputDevice::Controller) => (b.stick_sensitivity - d.stick_sensitivity).abs() <= 1e-4,
        R::Vertical(dev) => (b.vertical[device_slot(dev)] - d.vertical[device_slot(dev)]).abs() <= 1e-4,
        R::AimSensitivity(dev) => (b.aim[device_slot(dev)] - d.aim[device_slot(dev)]).abs() <= 1e-4,
        R::Rumble => b.rumble == d.rumble,
    };
    R::all().into_iter().filter(|r| !same(*r)).collect()
}

/// Put exactly `rows` of the Controls tab back as the game has them by default (the
/// Changed list's Defaults), each set directly, with no swap: a control's key goes back
/// without moving another control's, every other row is left as it is [design: #1953].
/// Logged and kept in the INI once.
pub fn restore_rows(world: &mut World, rows: &[crate::settings_view::ControlRow]) {
    use crate::settings_view::ControlRow as R;
    if rows.is_empty() {
        return;
    }
    let Some(mut b) = world.get_resource_mut::<Bindings>() else { return };
    let d = Bindings::game_defaults();
    for row in rows {
        match *row {
            R::Device => b.device = d.device,
            R::AutomaticInput => b.automatic_input_switching = d.automatic_input_switching,
            R::Binding(c, InputDevice::KeyboardMouse) => b.set(c, d.get(c)),
            R::Binding(c, InputDevice::Controller) => b.set_pad(c, d.pad(c)),
            R::Invert => b.invert_y = d.invert_y,
            R::Sensitivity(InputDevice::KeyboardMouse) => b.mouse_sensitivity = d.mouse_sensitivity,
            R::Sensitivity(InputDevice::Controller) => b.stick_sensitivity = d.stick_sensitivity,
            R::Vertical(dev) => b.vertical[device_slot(dev)] = d.vertical[device_slot(dev)],
            R::AimSensitivity(dev) => b.aim[device_slot(dev)] = d.aim[device_slot(dev)],
            R::Rumble => b.rumble = d.rumble,
        }
    }
    let names: Vec<String> = rows.iter().map(|r| r.name()).collect();
    info!("controls: {} back to the game's", names.join(", "));
    store(world);
}

/// Write `[Controls]` to the INI, keeping its other lines.
fn store(world: &World) {
    let Some(path) = world.get_resource::<crate::settings::SettingsFile>().and_then(|f| f.0.clone()) else { return };
    let Some(b) = world.get_resource::<Bindings>() else { return };
    if let Err(e) = write(&path, b) {
        warn!("controls: not saved: {e}");
    }
}

fn write(path: &Path, b: &Bindings) -> Result<(), String> {
    guard_write(path, None).map_err(|e| e.to_string())?;
    let old = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, with_controls(&old, b)).map_err(|e| format!("{}: {e}", path.display()))
}

/// A text field has the keyboard this frame (the settings page's search while it is open,
/// `crate::pause`): no control reads as pressed or released, so a letter typed there
/// moves, fires or saves nothing [design: #1953]. Set each frame in `PreUpdate`, before
/// the game's input, and still on the frame the field closes.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardCapture(pub bool);

/// Presses follow [`InputDevice`]; releases query both devices [design, #2200].
/// While the console's free camera flies (`crate::free_cam`) its
/// keys are the camera's: no control reads as pressed; while a text field has the keys
/// ([`KeyboardCapture`]) none reads as pressed or released.
#[derive(SystemParam)]
pub struct ControlInput<'w, 's> {
    pub keys: crate::pad::ActionButtons<'w, KeyCode>,
    pub mouse: crate::pad::ActionButtons<'w, MouseButton>,
    pub bindings: Res<'w, Bindings>,
    pub free_cam: Option<Res<'w, crate::free_cam::FreeCam>>,
    pub pads: crate::pad::ActionPads<'w, 's>,
    pub sticks: Option<Res<'w, crate::pad::StickButtons>>,
    /// Keys and buttons scripts disabled (DisableKey, DisableControl; task D6).
    pub blocked: Option<Res<'w, crate::script_input::ScriptBlocked>>,
    /// A playing kill camera holds the gameplay controls (`crate::kill_cam`).
    pub kill_cam: Option<Res<'w, crate::kill_cam::KillCam>>,
    /// A text field has the keyboard ([`KeyboardCapture`]).
    pub capture: Option<Res<'w, KeyboardCapture>>,
    pub platform_capture: Option<Res<'w, crate::pad::PlatformInputGate>>,
    pub touch_movement: Option<Res<'w, crate::touch_movement::TouchMovementInput>>,
    pub touch_switch: Option<Res<'w, crate::pad::TouchDeviceSwitch>>,
    /// Photo mode has the keys (`crate::photo_mode`).
    pub photo: Option<Res<'w, crate::photo_mode::PhotoMode>>,
}

/// The player's Keyboard and Mouse With a Controller tweak: with the controller as the
/// input device, the play controls' keys and mouse buttons (and the mouse's look) work
/// too; menus keep the device's [design: docs/design/tweaks.md]. Set by `crate::tweaks`.
static KEYS_WITH_PAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_keys_with_pad(on: bool) {
    KEYS_WITH_PAD.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn keys_with_pad() -> bool {
    KEYS_WITH_PAD.load(std::sync::atomic::Ordering::Relaxed)
}

/// How a controller input is asked about.
#[derive(Clone, Copy)]
enum Ask {
    Pressed,
    JustPressed,
    JustReleased,
}

/// Existing controller held-state rule, also used by script control events. No
/// gameplay/script blocking is applied here (control-input-events.md).
pub(crate) fn pad_binding_held(binding: Option<PadInput>, pads: &crate::pad::ActionPads, sticks: Option<&crate::pad::StickButtons>) -> bool {
    match binding {
        Some(PadInput::Button(b)) => pads.iter().any(|g| g.pressed(b)),
        Some(PadInput::Stick(d)) => sticks.is_some_and(|s| s.pressed(d) && pads.iter().any(|g| d.amount(g.left_stick()) >= crate::pad::STICK_PRESS)),
        None => false,
    }
}

impl ControlInput<'_, '_> {
    /// The free camera or a text field has the keys.
    pub fn suppressed(&self) -> bool {
        self.free_cam.as_ref().is_some_and(|f| f.active) || self.photo.as_ref().is_some_and(|p| p.on) || self.captured()
    }

    /// A text field has the keyboard ([`KeyboardCapture`]).
    pub fn captured(&self) -> bool {
        self.capture.as_ref().is_some_and(|c| c.0) || self.platform_capture.as_ref().is_some_and(|p| !p.allows_actions())
    }

    /// A playing kill camera holds `c`: every control but those that open a menu or
    /// save, so the game can still be paused or left (`crate::kill_cam`, module notes).
    pub fn held(&self, c: Control) -> bool {
        self.kill_cam.as_ref().is_some_and(|k| k.active())
            && !matches!(c, Control::MenuMode | Control::Rest | Control::QuickSave | Control::QuickLoad)
            // The player's tweak leaves walking and running (docs/design/tweaks.md).
            && !(crate::kill_cam::move_during() && matches!(c, Control::Forward | Control::Back | Control::SlideLeft | Control::SlideRight | Control::Run | Control::AlwaysRun | Control::AutoMove))
    }

    /// A controller works the controls.
    pub fn controller(&self) -> bool {
        self.bindings.device == InputDevice::Controller
    }

    /// A platform-owned action may act through its current keyboard/mouse binding.
    /// It still obeys device selection, capture and script-disabled inputs.
    pub fn virtual_action_allowed(&self, control: Control) -> bool {
        if self.controller() || self.suppressed() || self.held(control) {
            return false;
        }
        let binding = self.bindings.get(control);
        // Touch uses the primary key when present, otherwise its mouse binding.
        // A disabled primary must not be bypassed via an unused secondary binding.
        if let Some(key) = binding.key { self.open_key(key) }
        else { binding.mouse.is_some_and(|button| self.open_button(button)) }
    }

    /// Derived physical stick edges with the switching crossing filtered [design, #2200].
    pub fn stick_buttons(&self) -> Option<crate::pad::StickButtons> {
        self.sticks.as_ref().map(|s| self.pads.filter_sticks(s))
    }

    fn pad_is(&self, c: Control, ask: Ask) -> bool {
        if matches!(ask, Ask::Pressed) {
            return pad_binding_held(self.bindings.pad(c), &self.pads, self.sticks.as_deref());
        }
        match self.bindings.pad(c) {
            Some(PadInput::Button(b)) => self.pads.iter().any(|g| match ask {
                Ask::Pressed => g.pressed(b),
                Ask::JustPressed => g.just_pressed(b),
                Ask::JustReleased => g.just_released(b),
            }),
            Some(PadInput::Stick(d)) => self.sticks.as_ref().is_some_and(|s| match ask {
                Ask::Pressed => s.pressed(d) && self.pads.iter().any(|g| d.amount(g.left_stick()) >= crate::pad::STICK_PRESS),
                Ask::JustPressed => s.just_pressed(d) && self.pads.iter().any(|g| d.amount(g.left_stick()) >= crate::pad::STICK_PRESS),
                Ask::JustReleased => s.just_released(d),
            }),
            None => false,
        }
    }

    /// Controller gesture ownership excludes physical keyboard/mouse contributions
    /// admitted by Keys With Pad. It retains the same device/action/capture gates.
    pub(crate) fn pad_pressed(&self, c:Control)->bool {
        self.controller() && !self.suppressed() && !self.held(c) && self.pad_is(c,Ask::Pressed)
    }
    pub(crate) fn pad_just_pressed(&self, c:Control)->bool {
        self.controller() && !self.suppressed() && !self.held(c) && self.pad_is(c,Ask::JustPressed)
    }

    /// The left stick (x right, y forward) when the controller works the controls and
    /// the movement controls are on the stick.
    pub fn move_stick(&self) -> Option<Vec2> {
        let on_stick = [Control::Forward, Control::Back, Control::SlideLeft, Control::SlideRight].iter().all(|&c| matches!(self.bindings.pad(c), Some(PadInput::Stick(_))));
        (self.controller() && on_stick && !self.suppressed() && !self.held(Control::Forward)).then(|| crate::pad::stick(&self.pads, true))
    }

    /// Digital movement for player arbitration: analog must not quantize itself.
    pub fn digital_movement_pressed(&self, c: Control) -> bool {
        matches!(c, Control::Forward | Control::Back | Control::SlideLeft | Control::SlideRight) && self.button_pressed(c)
    }

    pub fn touch_movement_pressed(&self, c: Control) -> bool { self.touch_direction(c, Ask::Pressed) }

    fn touch_direction(&self, c: Control, ask: Ask) -> bool {
        if !matches!(c, Control::Forward | Control::Back | Control::SlideLeft | Control::SlideRight) { return false; }
        let amount = |v: Vec2| match c {
            Control::Forward => v.y, Control::Back => -v.y,
            Control::SlideRight => v.x, Control::SlideLeft => -v.x, _ => 0.0,
        };
        let Some(sample) = self.touch_movement.as_deref() else { return false };
        let current = amount(sample.admitted(self, self.touch_switch.as_deref())) > 0.0001;
        let previous = crate::touch_movement::TouchMovementInput { axes: sample.previous_axes, ..default() };
        let previous = amount(previous.admitted(self, self.touch_switch.as_deref())) > 0.0001;
        match ask { Ask::Pressed => current, Ask::JustPressed => current && !previous, Ask::JustReleased => previous && !current }
    }

    pub fn pressed(&self, c: Control) -> bool { self.button_pressed(c) || self.touch_direction(c, Ask::Pressed) }

    fn button_pressed(&self, c: Control) -> bool {
        if self.suppressed() || self.held(c) {
            return false;
        }
        if self.controller() {
            return self.pad_is(c, Ask::Pressed) || (keys_with_pad() && self.keys_pressed(c));
        }
        self.keys_pressed(c)
    }

    fn keys_pressed(&self, c: Control) -> bool {
        if self.controller() { return self.physical_pressed(c); }
        let b = self.bindings.get(c);
        b.key.is_some_and(|k| self.keys.pressed(k) && self.open_key(k)) || b.mouse.is_some_and(|m| self.mouse.pressed(m) && self.open_button(m))
    }

    /// Only existing raw keyboard/mouse provenance, never a pad or virtual touch.
    /// Callers choose device eligibility; script, platform and capture gates remain.
    pub fn physical_pressed(&self, c:Control)->bool { self.physical_is(c,Ask::Pressed) }
    pub fn physical_just_pressed(&self, c:Control)->bool { self.physical_is(c,Ask::JustPressed) }
    pub fn physical_just_released(&self, c:Control)->bool {
        self.physical_is(c,Ask::JustReleased) && !self.physical_pressed(c)
    }
    pub fn physical_mouse_pressed(&self, mouse:MouseButton)->bool {
        !self.suppressed() && self.open_button(mouse) && self.mouse.physical_pressed(mouse)
    }
    fn physical_is(&self,c:Control,ask:Ask)->bool {
        if self.suppressed() || self.held(c) {return false;}
        let b=self.bindings.get(c);
        b.key.is_some_and(|k|self.open_key(k) && match ask {
            Ask::Pressed=>self.keys.physical_pressed(k),
            Ask::JustPressed=>self.keys.physical_just_pressed(k),
            Ask::JustReleased=>self.keys.physical_just_released(k),
        }) || b.mouse.is_some_and(|m|self.open_button(m) && match ask {
            Ask::Pressed=>self.mouse.physical_pressed(m),
            Ask::JustPressed=>self.mouse.physical_just_pressed(m),
            Ask::JustReleased=>self.mouse.physical_just_released(m),
        })
    }

    fn open_key(&self, k: KeyCode) -> bool {
        !self.blocked.as_ref().is_some_and(|b| b.keys.contains(&k))
    }

    fn open_button(&self, m: MouseButton) -> bool {
        !self.blocked.as_ref().is_some_and(|b| b.mouse.contains(&m))
    }

    pub fn just_pressed(&self, c: Control) -> bool {
        if self.suppressed() || self.held(c) {
            return false;
        }
        let b = self.bindings.get(c);
        let keys = || b.key.is_some_and(|k| self.keys.just_pressed(k) && self.open_key(k)) || b.mouse.is_some_and(|m| self.mouse.just_pressed(m) && self.open_button(m));
        if self.controller() {
            return self.pad_is(c, Ask::JustPressed) || (keys_with_pad() && self.physical_just_pressed(c));
        }
        keys() || self.touch_direction(c, Ask::JustPressed)
    }

    /// Released this frame, and neither of its inputs is still down.
    pub fn just_released(&self, c: Control) -> bool {
        if self.held(c) || self.captured() {
            return false;
        }
        let b = self.bindings.get(c);
        let released = self.touch_direction(c, Ask::JustReleased) || self.pad_is(c, Ask::JustReleased) || b.key.is_some_and(|k| self.keys.just_released(k)) || b.mouse.is_some_and(|m| self.mouse.just_released(m));
        released && !self.pressed(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_input_switching_round_trips_and_legacy_devices_keep_their_meaning() {
        for device in [InputDevice::KeyboardMouse, InputDevice::Controller] {
            let mut b = Bindings::default();
            let mut bad = Vec::new();
            read_controls(&format!("[Controls]\nInput Device={}\n", device.name()), &mut b, &mut bad);
            assert_eq!(b.device, device);
            assert!(b.automatic_input_switching);
            b.automatic_input_switching = false;
            let text = with_controls("", &b);
            let mut back = Bindings::default();
            assert_eq!(read_controls(&text, &mut back, &mut bad).0, LINES);
            assert_eq!(back.device, device);
            assert!(!back.automatic_input_switching);
            assert!(bad.is_empty());
            read_controls("[Controls]\nAutomatic Input Switching=bad\n", &mut back, &mut bad);
            assert!(!back.automatic_input_switching);
            assert_eq!(bad.len(), 1);
        }
        use crate::settings_view::ControlRow;
        let mut app = App::new();
        app.init_resource::<Bindings>();
        set_automatic_input_switching(app.world_mut(), false);
        assert_eq!(changed_controls(app.world().resource::<Bindings>()), [ControlRow::AutomaticInput]);
        restore_rows(app.world_mut(), &[ControlRow::AutomaticInput]);
        assert!(changed_controls(app.world().resource::<Bindings>()).is_empty());
    }

    #[test]
    fn the_sensitivity_steps_within_its_range() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
        assert_eq!(sensitivity_text(1.0), "100%");
        assert!(near(step_sensitivity(1.0, 1), 1.1));
        assert!(near(step_sensitivity(1.0, -2), 0.9));
        assert!(near(step_sensitivity(1.0, -100), 0.05), "not under a twentieth");
        assert!(near(step_sensitivity(1.0, 100), 3.0), "not past three times");
        assert!(near(step_sensitivity(0.05, 1), 0.06), "whole percents at the bottom");
        // A value from the INI off the steps is brought onto them.
        assert!(near(step_sensitivity(1.03, 0), 1.0) && near(step_sensitivity(1.063, 0), 1.1), "the nearest on the log scale");
        assert!(near(step_sensitivity(0.0001, 0), 0.05) && near(step_sensitivity(9.0, -1), 2.75));
        assert!(near(sensitivity_part(0.05), 0.0) && near(sensitivity_part(3.0), 1.0));
        assert!(near(sensitivity_part(0.01), 0.0) && near(sensitivity_part(0.0), 0.0));
        assert!(near(sensitivity_at_part(sensitivity_part(1.5)), 1.5));
        assert!(near(sensitivity_at_part(2.0), 3.0), "a drag past the meter's end stops at the most");
        assert!(near(sensitivity_at_part(-1.0), 0.05));
        // The values the slider had before are still steps (the smoke scenarios use them).
        for p in [25, 50, 100, 150, 200, 300] {
            assert!(SENSITIVITY_STEPS.contains(&p), "{p}");
        }
        // The mouse's and the stick's are kept apart.
        let mut b = Bindings::game_defaults();
        b.mouse_sensitivity = 0.003;
        assert!(near(b.sensitivity(), 1.5));
        b.device = InputDevice::Controller;
        assert!(near(b.sensitivity(), 1.0));
        let mut bad = vec![];
        let mut back = Bindings::game_defaults();
        let (_, changed) = read_controls(&ini_set(&with_controls("", &b), PAD_SECTION, STICK_SENSITIVITY_KEY, "2.00"), &mut back, &mut bad);
        assert!(bad.is_empty(), "{bad:?}");
        assert!(near(back.stick_sensitivity, 2.0) && near(back.mouse_sensitivity, 0.003), "{changed:?}");
    }

    /// #2201: each device's Vertical Sensitivity is kept in the INI and read back apart; an
    /// INI written before it (no such lines) reads as 1, the look as before.
    #[test]
    fn the_vertical_sensitivities_are_kept_apart_and_missing_lines_read_as_one() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
        let mut b = Bindings::game_defaults();
        assert_eq!(b.vertical, [1.0, 1.0]);
        b.vertical = [0.8, 1.2];
        assert!(near(b.vertical_sensitivity(), 0.8));
        b.device = InputDevice::Controller;
        assert!(near(b.vertical_sensitivity(), 1.2));
        let text = with_controls("", &b);
        assert!(text.contains("Mouse Vertical Sensitivity=0.80") && text.contains("Stick Vertical Sensitivity=1.20"), "{text}");
        let (mut back, mut bad) = (Bindings::game_defaults(), vec![]);
        let (found, changed) = read_controls(&text, &mut back, &mut bad);
        assert!(bad.is_empty(), "{bad:?}");
        assert_eq!(found, LINES);
        assert!(near(back.vertical[0], 0.8) && near(back.vertical[1], 1.2), "{changed:?}");
        assert!(changed.contains(&"Mouse Vertical Sensitivity 0.8".to_string()) && changed.contains(&"[Controller] Stick Vertical Sensitivity 1.2".to_string()), "{changed:?}");
        // Without the lines: 1 each, and two lines fewer found (written at the start).
        let old: String = text.lines().filter(|l| !l.contains("Vertical Sensitivity")).map(|l| format!("{l}\n")).collect();
        let mut back = Bindings::game_defaults();
        let (found, _) = read_controls(&old, &mut back, &mut bad);
        assert_eq!((found, back.vertical), (LINES - 2, [1.0, 1.0]));
        // Below the slider's least: clamped; not a number: refused.
        let low = ini_set(&ini_set(&text, SECTION, "Mouse Vertical Sensitivity", "0.01"), PAD_SECTION, "Stick Vertical Sensitivity", "x");
        let mut back = Bindings::game_defaults();
        read_controls(&low, &mut back, &mut bad);
        assert!(near(back.vertical[0], SENSITIVITY_LEAST) && near(back.vertical[1], 1.0));
        assert_eq!(bad, ["[Controller] Stick Vertical Sensitivity=x (not a number above 0)"]);
        // Away from the default: in the Changed list, and its Defaults puts it back.
        use crate::settings_view::ControlRow as R;
        let mut b = Bindings::game_defaults();
        b.vertical[1] = 0.5;
        assert_eq!(changed_controls(&b), [R::Vertical(InputDevice::Controller)]);
        let mut app = App::new();
        app.insert_resource(b);
        restore_rows(app.world_mut(), &[R::Vertical(InputDevice::Controller)]);
        assert_eq!(app.world().resource::<Bindings>().vertical, [1.0, 1.0]);
    }

    /// Each device's Aim Sensitivity (owner 2026-11-15) is kept under "Mouse Aim Sensitivity" or "Stick Aim
    /// Sensitivity" in its own section and read back apart; an INI written before it reads as 1, the look
    /// while aiming as before; the setter, the Changed list and its Defaults act on the
    /// device's own.
    #[test]
    fn the_aim_sensitivities_are_kept_apart_and_missing_lines_read_as_one() {
        use crate::settings_view::ControlRow as R;
        let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
        let mut b = Bindings::game_defaults();
        assert_eq!(b.aim, [1.0, 1.0]);
        b.aim = [0.6, 1.5];
        assert!(near(b.aim_sensitivity(), 0.6));
        b.device = InputDevice::Controller;
        assert!(near(b.aim_sensitivity(), 1.5));
        let text = with_controls("", &b);
        assert_eq!(ini_get(&text, SECTION, "Mouse Aim Sensitivity").as_deref(), Some("0.60"), "{text}");
        assert_eq!(ini_get(&text, PAD_SECTION, "Stick Aim Sensitivity").as_deref(), Some("1.50"), "{text}");
        let (mut back, mut bad) = (Bindings::game_defaults(), vec![]);
        let (found, changed) = read_controls(&text, &mut back, &mut bad);
        assert!(bad.is_empty(), "{bad:?}");
        assert_eq!(found, LINES);
        assert!(near(back.aim[0], 0.6) && near(back.aim[1], 1.5), "{changed:?}");
        assert!(changed.contains(&"Mouse Aim Sensitivity 0.6".to_string()) && changed.contains(&"[Controller] Stick Aim Sensitivity 1.5".to_string()), "{changed:?}");
        assert_eq!(back.vertical, [1.0, 1.0], "the vertical sensitivities untouched");
        // Without the lines: 1 each, and two lines fewer found (written at the start).
        let old: String = text.lines().filter(|l| !l.contains("Aim Sensitivity")).map(|l| format!("{l}\n")).collect();
        let mut back = Bindings::game_defaults();
        let (found, _) = read_controls(&old, &mut back, &mut bad);
        assert_eq!((found, back.aim), (LINES - 2, [1.0, 1.0]));
        // Below the slider's least: clamped; not a number: refused.
        let low = ini_set(&ini_set(&text, SECTION, "Mouse Aim Sensitivity", "0.01"), PAD_SECTION, "Stick Aim Sensitivity", "x");
        let mut back = Bindings::game_defaults();
        read_controls(&low, &mut back, &mut bad);
        assert!(near(back.aim[0], SENSITIVITY_LEAST) && near(back.aim[1], 1.0));
        assert_eq!(bad, ["[Controller] Stick Aim Sensitivity=x (not a number above 0)"]);
        // The setter sets the device in use's alone.
        let mut app = App::new();
        app.insert_resource(Bindings::game_defaults());
        set_aim_sensitivity(app.world_mut(), 0.5);
        app.world_mut().resource_mut::<Bindings>().device = InputDevice::Controller;
        set_aim_sensitivity(app.world_mut(), 2.0);
        assert_eq!(app.world().resource::<Bindings>().aim, [0.5, 2.0]);
        // Away from the default: in the Changed list, and its Defaults puts it back.
        assert_eq!(changed_controls(app.world().resource::<Bindings>()), [R::Device, R::AimSensitivity(InputDevice::KeyboardMouse), R::AimSensitivity(InputDevice::Controller)]);
        restore_rows(app.world_mut(), &[R::AimSensitivity(InputDevice::Controller)]);
        assert_eq!(app.world().resource::<Bindings>().aim, [0.5, 1.0]);
    }

    /// #1632: centimetres of mouse travel for a full turn at 1600 dpi, reading the
    /// sensitivity as radians a count (see `MOUSE_SENSITIVITY` above).
    fn cm_per_turn_at_1600_dpi(m: f32) -> f32 {
        std::f32::consts::TAU / (m * MOUSE_SENSITIVITY) / 1600.0 * 2.54
    }

    #[test]
    fn the_slowest_mouse_sensitivity_suits_a_1600_dpi_mouse() {
        assert!(cm_per_turn_at_1600_dpi(SENSITIVITY_LEAST) >= 60.0, "{}", cm_per_turn_at_1600_dpi(SENSITIVITY_LEAST));
        let default = cm_per_turn_at_1600_dpi(1.0);
        assert!((4.9..5.1).contains(&default), "the default is unchanged: {default}");
        assert!(cm_per_turn_at_1600_dpi(SENSITIVITY_MOST) < 2.0);
        // The steps rise, stay in the range, and no step changes the turn by more than a
        // fifth (finer than the old 25% -> 30% at the bottom).
        assert_eq!(f32::from(SENSITIVITY_STEPS[0]) / 100.0, SENSITIVITY_LEAST);
        assert_eq!(f32::from(SENSITIVITY_STEPS[SENSITIVITY_STEPS.len() - 1]) / 100.0, SENSITIVITY_MOST);
        for w in SENSITIVITY_STEPS.windows(2) {
            assert!(w[0] < w[1] && f32::from(w[1]) / f32::from(w[0]) <= 1.2 + 1e-6, "{w:?}");
        }
        // Each step walks to the next and back.
        for (i, &p) in SENSITIVITY_STEPS.iter().enumerate() {
            let m = f32::from(p) / 100.0;
            assert_eq!(nearest_step(m), i, "{p}");
            assert!((sensitivity_at_part(sensitivity_part(m)) - m).abs() < 1e-5, "{p}");
        }
        // The meter is logarithmic: the slow end has room, 100% sits about three quarters in.
        let at_default = sensitivity_part(1.0);
        assert!((0.7..0.76).contains(&at_default), "{at_default}");
    }

    #[test]
    fn a_saved_sensitivity_below_the_least_is_clamped_not_refused() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-7;
        let text = with_controls("", &Bindings::game_defaults());
        // Old INIs: 25% (the old least) keeps loading as it was.
        let mut b = Bindings::game_defaults();
        let mut bad = vec![];
        read_controls(&ini_set(&text, SECTION, SENSITIVITY_KEY, "0.0005"), &mut b, &mut bad);
        assert!(bad.is_empty() && near(b.mouse_sensitivity, 0.0005), "{bad:?}");
        // Under the new least: brought up to it.
        let low = ini_set(&ini_set(&text, SECTION, SENSITIVITY_KEY, "0.00001"), PAD_SECTION, STICK_SENSITIVITY_KEY, "0.01");
        let mut b = Bindings::game_defaults();
        read_controls(&low, &mut b, &mut bad);
        assert!(bad.is_empty(), "{bad:?}");
        assert!(near(b.mouse_sensitivity, SENSITIVITY_LEAST * MOUSE_SENSITIVITY), "{}", b.mouse_sensitivity);
        assert!((b.stick_sensitivity - SENSITIVITY_LEAST).abs() < 1e-6);
        // Zero is still refused (not a number above 0).
        let mut b = Bindings::game_defaults();
        read_controls(&ini_set(&text, SECTION, SENSITIVITY_KEY, "0"), &mut b, &mut bad);
        assert_eq!(bad.len(), 1);
        // The low end is written with the decimals it needs and read back.
        assert_eq!(mouse_sensitivity_text(0.002), "0.0020");
        assert_eq!(mouse_sensitivity_text(0.06 * MOUSE_SENSITIVITY), "0.00012");
        let mut slow = Bindings::game_defaults();
        slow.mouse_sensitivity = 0.07 * MOUSE_SENSITIVITY;
        let mut back = Bindings::game_defaults();
        read_controls(&with_controls("", &slow), &mut back, &mut bad);
        assert!((back.sensitivity() - 0.07).abs() < 1e-4, "{}", back.sensitivity());
    }

    #[test]
    fn ini_values_decode() {
        let b = Bindings::game_defaults();
        assert_eq!(b.get(Control::Forward), Binding { key: Some(KeyCode::KeyW), mouse: None });
        assert_eq!(b.get(Control::Use), Binding { key: None, mouse: Some(MouseButton::Left) });
        assert_eq!(b.get(Control::Block), Binding { key: Some(KeyCode::AltLeft), mouse: Some(MouseButton::Right) });
        assert_eq!(b.get(Control::TogglePov), Binding { key: Some(KeyCode::KeyF), mouse: Some(MouseButton::Middle) });
        assert_eq!(b.get(Control::CrouchSneak).key, Some(KeyCode::ControlLeft));
        assert_eq!(b.get(Control::AlwaysRun).key, Some(KeyCode::CapsLock));
        assert_eq!(b.get(Control::AutoMove).key, Some(KeyCode::KeyQ));
        assert_eq!(b.get(Control::MenuMode).key, Some(KeyCode::Tab));
        assert_eq!(b.get(Control::AmmoSwap).key, Some(KeyCode::Digit2));
        assert_eq!(b.get(Control::QuickLoad).key, Some(KeyCode::F9));
        assert_eq!(b.get(Control::LowerWeapon).key, Some(KeyCode::KeyH));
        assert_eq!(Bindings::name(Control::Rest), "Rest");
        // Every control has a key or a button, but the Tweaks tab's, which start unbound;
        // the console and the screenshot at the end on their old keys.
        // (Sprint and Photo Mode, NVForever's own, have none until the player binds them:
        // #3000, #3634.)
        let unbound = [Control::Sprint, Control::PhotoMode];
        for (c, _, _) in GAME_CONTROLS[..TWEAK_CONTROLS].iter().filter(|(c, _, _)| !unbound.contains(c)) {
            let x = b.get(*c);
            assert!(x.key.is_some() || x.mouse.is_some(), "{c:?}");
        }
        assert_eq!(b.get(Control::Sprint), Binding::default());
        let extra = GAME_CONTROLS.len() - BOUND_EXTRA_CONTROLS;
        for (c, _, _) in &GAME_CONTROLS[TWEAK_CONTROLS..extra] {
            assert_eq!(b.get(*c), Binding { key: None, mouse: None }, "{c:?}");
        }
        assert_eq!(b.get(Control::Console), Binding { key: Some(KeyCode::Backquote), mouse: None });
        assert_eq!(b.get(Control::Screenshot), Binding { key: Some(KeyCode::F12), mouse: None });
    }

    /// Photo Mode's one binding, its opener (#3634, the maintainer's decision): no key and no
    /// controller button until the player binds them; kept in the INI as the others are.
    #[test]
    fn photo_mode_is_unbound_by_default() {
        let b = Bindings::game_defaults();
        assert_eq!(b.get(Control::PhotoMode), Binding::default());
        assert_eq!(b.pad(Control::PhotoMode), None);
        assert_eq!(Bindings::by_name("Photo Mode"), Some(Control::PhotoMode));
        assert!(with_controls("", &b).contains("Photo Mode="));
    }

    /// Sprint (#3000, the maintainer's decision on #3002): no key and no button by default, and
    /// Run keeps Left Shift whatever Sprint's state; a Sprint key the player sets is any
    /// binding (a swap with the control that had it).
    #[test]
    fn sprint_is_unbound_by_default_and_run_keeps_shift() {
        let mut b = Bindings::game_defaults();
        assert_eq!(b.get(Control::Sprint), Binding::default(), "no key by default");
        assert_eq!(b.pad(Control::Sprint), None, "no button by default");
        assert_eq!(b.get(Control::Run).key, Some(KeyCode::ShiftLeft));
        assert_eq!(b.text_on(Control::Sprint), "None");
        assert!(with_controls("", &b).contains("Sprint="), "kept in the INI as the others are");
        // Bound to a free key: Run is untouched.
        assert!(b.rebind(Control::Sprint, Input::Key(KeyCode::KeyG)).is_empty());
        assert_eq!((b.get(Control::Sprint).key, b.get(Control::Run).key), (Some(KeyCode::KeyG), Some(KeyCode::ShiftLeft)));
        // Bound to Run's key: the Controls tab's usual swap, no special case.
        let mut b = Bindings::game_defaults();
        assert_eq!(b.rebind(Control::Sprint, Input::Key(KeyCode::ShiftLeft)), [Control::Run]);
        assert_eq!((b.get(Control::Sprint).key, b.get(Control::Run).key), (Some(KeyCode::ShiftLeft), None));
    }

    #[test]
    fn shared_keys_leave_the_other_control_alone() {
        let mut b = Bindings::game_defaults();
        b.allow_duplicates = true;
        let jump = b.get(Control::Jump).key;
        assert!(b.rebind(Control::Activate, Input::Key(jump.unwrap())).is_empty());
        assert_eq!(b.get(Control::Jump).key, jump, "Jump keeps its key");
        assert_eq!(b.get(Control::Activate).key, jump);
    }

    #[test]
    fn bindings_are_written_by_name_and_read_back() {
        let b = Bindings::game_defaults();
        assert_eq!(b.get(Control::Block).text(), "Left Alt, Right Mouse");
        assert_eq!(b.get(Control::Use).text(), "Left Mouse");
        assert_eq!(Binding::default().text(), "None");
        for (c, x) in b.all() {
            assert_eq!(Binding::parse(&x.text()), Ok(x), "{c:?}");
        }
        assert_eq!(Binding::parse(" left alt , RIGHT MOUSE "), Ok(b.get(Control::Block)));
        assert_eq!(Binding::parse("Mouse 7"), Ok(Binding { key: None, mouse: Some(MouseButton::Other(6)) }));
        assert_eq!(Binding::parse(""), Ok(Binding::default()));
        assert!(Binding::parse("W, E").is_err());
        assert!(Binding::parse("Banana").is_err());
        assert!(Binding::parse("Escape").is_err());
        assert_eq!(Bindings::by_name("crouch/sneak"), Some(Control::CrouchSneak));

        let ini = "[Game]\nPath=F:\\FNV\n";
        let text = with_controls(ini, &b);
        assert!(text.starts_with("[Game]\nPath=F:\\FNV\n\n[Controls]\nForward=W\nBack=S\n"), "{text}");
        assert!(text.contains("\nMouse Sensitivity=0.0020\n"));
        let mut back = Bindings::game_defaults();
        let (found, changed) = read_controls(&text, &mut back, &mut Vec::new());
        assert_eq!((found, changed.len()), (LINES, 0));

        let edited = ini_set(&ini_set(&text, SECTION, "Jump", "J"), SECTION, "Invert Y", "1");
        let edited = ini_set(&edited, SECTION, "Vats", "Purple");
        let mut bad = Vec::new();
        let (_, changed) = read_controls(&edited, &mut back, &mut bad);
        assert_eq!(changed, ["Jump Space -> J", "Invert Y 1"]);
        assert_eq!(back.get(Control::Jump).key, Some(KeyCode::KeyJ));
        assert!(back.invert_y);
        assert_eq!(back.get(Control::Vats).key, Some(KeyCode::KeyV), "a bad line keeps the default");
        assert_eq!(bad.len(), 1);

        // The controller's rumble, on by default (the game's bGamePadRumble=1).
        assert!(text.contains("
Rumble=1"), "{text}");
        let (_, changed) = read_controls(&ini_set(&edited, PAD_SECTION, RUMBLE_KEY, "0"), &mut back, &mut bad);
        assert_eq!(changed, ["[Controller] Rumble 0"]);
        assert!(!back.rumble);
    }

    #[test]
    fn a_rebind_swaps_with_the_control_that_had_the_input() {
        let mut b = Bindings::game_defaults();
        // Jump to W: Forward had W and gets Space.
        assert_eq!(b.rebind(Control::Jump, Input::Key(KeyCode::KeyW)), [Control::Forward]);
        assert_eq!(b.get(Control::Jump).key, Some(KeyCode::KeyW));
        assert_eq!(b.get(Control::Forward).key, Some(KeyCode::Space));
        // A free key takes nothing from anyone; the mouse button is kept.
        assert!(b.rebind(Control::Block, Input::Key(KeyCode::KeyK)).is_empty());
        assert_eq!(b.get(Control::Block), Binding { key: Some(KeyCode::KeyK), mouse: Some(MouseButton::Right) });
        // Attack to the right button: Block had it and gets Attack's left button.
        assert_eq!(b.rebind(Control::Use, Input::Mouse(MouseButton::Right)), [Control::Block]);
        assert_eq!(b.get(Control::Block).mouse, Some(MouseButton::Left));
        assert_eq!(b.get(Control::Use).mouse, Some(MouseButton::Right));
    }

    #[test]
    fn the_ini_is_filled_in_and_a_rebind_is_kept() {
        let dir = std::env::temp_dir().join(format!("nvf-controls-{}", std::process::id()));
        let path = dir.join("NVForever.ini");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "[Game]\nPath=F:\\FNV\n\n[Controls]\nJump=J\n").unwrap();
        let start = || {
            let mut app = App::new();
            app.init_resource::<Bindings>();
            app.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
            app.add_systems(PostStartup, apply_stored);
            app.update();
            app
        };
        let mut app = start();
        assert_eq!(app.world().resource::<Bindings>().get(Control::Jump).key, Some(KeyCode::KeyJ));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[Controls]\nJump=J\nForward=W\n") && text.contains("Path=F:\\FNV"), "{text}");
        rebind(app.world_mut(), Control::Activate, Input::Key(KeyCode::KeyF));
        let app = start();
        let b = app.world().resource::<Bindings>();
        assert_eq!(b.get(Control::Activate).key, Some(KeyCode::KeyF));
        assert_eq!(b.get(Control::TogglePov).key, Some(KeyCode::KeyE), "the swap is kept too");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn input_follows_bindings() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>().init_resource::<ButtonInput<MouseButton>>().init_resource::<Bindings>();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Middle);
        let mut sys = bevy::ecs::system::SystemState::<ControlInput>::new(app.world_mut());
        let c = sys.get(app.world()).expect("resources");
        assert!(c.pressed(Control::TogglePov) && c.just_pressed(Control::TogglePov));
        assert!(!c.pressed(Control::Use));
    }

    #[test]
    fn a_text_field_holding_the_keys_hides_them_from_the_controls() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>().init_resource::<ButtonInput<MouseButton>>().init_resource::<Bindings>().init_resource::<KeyboardCapture>();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        let mut sys = bevy::ecs::system::SystemState::<ControlInput>::new(app.world_mut());
        assert!(sys.get(app.world()).unwrap().just_pressed(Control::Forward));
        app.world_mut().resource_mut::<KeyboardCapture>().0 = true;
        let c = sys.get(app.world()).unwrap();
        assert!(c.suppressed() && !c.pressed(Control::Forward) && !c.just_pressed(Control::Forward));
        // Let go while the field holds the keys (the frame it closes too): no release either.
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyW);
        assert!(!sys.get(app.world()).unwrap().just_released(Control::Forward));
        app.world_mut().resource_mut::<KeyboardCapture>().0 = false;
        assert!(sys.get(app.world()).unwrap().just_released(Control::Forward));
    }

    #[test]
    fn the_changed_controls_go_back_one_by_one_without_a_swap() {
        use crate::settings_view::ControlRow as R;
        let mut app = App::new();
        app.init_resource::<Bindings>();
        let world = app.world_mut();
        assert!(changed_controls(world.resource::<Bindings>()).is_empty());
        // Jump to W (Forward gets Space), the controller's Jump to RB, the stick slower, no rumble.
        let mut b = world.resource_mut::<Bindings>();
        b.rebind(Control::Jump, Input::Key(KeyCode::KeyW));
        b.rebind_pad(Control::Jump, PadInput::Button(GamepadButton::RightTrigger));
        b.stick_sensitivity = 0.5;
        b.rumble = false;
        let changed = changed_controls(world.resource::<Bindings>());
        assert_eq!(
            changed,
            [
                R::Binding(Control::Forward, InputDevice::KeyboardMouse),
                R::Binding(Control::Jump, InputDevice::KeyboardMouse),
                R::Binding(Control::Jump, InputDevice::Controller),
                R::Binding(Control::Vats, InputDevice::Controller),
                R::Sensitivity(InputDevice::Controller),
                R::Rumble,
            ]
        );
        // Jump's key alone back: Forward keeps Space (no swap), everything else as it was.
        restore_rows(world, &[R::Binding(Control::Jump, InputDevice::KeyboardMouse), R::Rumble]);
        let b = world.resource::<Bindings>();
        assert_eq!(b.get(Control::Jump).key, Some(KeyCode::Space));
        assert_eq!(b.get(Control::Forward).key, Some(KeyCode::Space), "Forward is not given W back");
        assert!(b.rumble);
        assert_eq!(changed_controls(b), [R::Binding(Control::Forward, InputDevice::KeyboardMouse), R::Binding(Control::Jump, InputDevice::Controller), R::Binding(Control::Vats, InputDevice::Controller), R::Sensitivity(InputDevice::Controller)]);
        restore_rows(world, &changed_controls(world.resource::<Bindings>()));
        assert!(changed_controls(world.resource::<Bindings>()).is_empty());
    }

    #[test]
    fn android_defaults_set_look_and_vertical_sensitivity_to_1_25_while_desktop_keeps_1_0() {
        let android = Bindings::game_defaults_for_platform(true);
        assert!((android.sensitivity() - 1.25).abs() < 1e-6);
        assert!((android.vertical_sensitivity() - 1.25).abs() < 1e-6);
        assert!((android.vertical[1] - 1.25).abs() < 1e-6);
        let desktop = Bindings::game_defaults_for_platform(false);
        assert!((desktop.sensitivity() - 1.0).abs() < 1e-6);
        assert!((desktop.vertical_sensitivity() - 1.0).abs() < 1e-6);
        assert!((desktop.vertical[1] - 1.0).abs() < 1e-6);
    }
}
