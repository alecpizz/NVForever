//! SUP NVSE, batch 6 (#1008; docs/spec-notes/ext-sup6.md): the mouse, the cursor's look,
//! the debug texts, the screen, fonts, plugin headers and the machine, the HUD's
//! visibility flags, the lockpick menu and the grenade held. The engine fills what only
//! it knows into [`SupState`] each frame (the mouse, the window, the fonts, the lockpick
//! menu, the throw held) and takes the scripts' requests (fake mouse moves, a bobby pin's
//! new health); it draws the debug texts and the cursor's traits (`nvf_play::sup_screen`).

use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::BTreeMap;

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

fn text(args: &[XVal], k: usize) -> Option<String> {
    match args.get(k)? {
        XVal::Str(s) => Some(s.clone()),
        XVal::Num(x) => Some(format!("{x}")),
        _ => None,
    }
}

/// The return value and the outputs, for the VM to write (`out_call`).
fn outputs(g: &mut Game, ret: f64, vals: &[f64]) -> XVal {
    let entries = std::iter::once(ret).chain(vals.iter().copied()).enumerate().map(|(i, v)| (XKey::Num(i as f64), XVal::Num(v))).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// A debug text a script put on the screen [documented: DebugTextCreate].
#[derive(Debug, Clone, PartialEq)]
pub struct DebugText {
    /// Its place in the window's pixels from the top left [hypothesis: the page's 100 100
    /// and 600 700 are pixels].
    pub x: f32,
    pub y: f32,
    pub text: String,
    /// The text's size: the height of its letters in pixels, 16 until set
    /// [hypothesis: the page's example sets 20].
    pub zoom: f32,
}

/// The font files by number 1-9 [documented: Fallout_default.ini `[Fonts]`; the engine's
/// `nvf_play::interface::FONT_FILES`].
pub const FONT_FILES: [&str; 9] = [
    "textures\\fonts\\glow_monofonto_large.fnt",
    "textures\\fonts\\monofonto_large.fnt",
    "textures\\fonts\\glow_monofonto_medium.fnt",
    "textures\\fonts\\monofonto_verylarge02_dialogs2.fnt",
    "textures\\fonts\\fixedsys_comp_uniform_width.fnt",
    "textures\\fonts\\glow_monofonto_vl_dialogs.fnt",
    "textures\\fonts\\baked-in_monofonto_large.fnt",
    "textures\\fonts\\glow_futura_caps_large.fnt",
    "textures\\fonts\\nvfont_test.fnt",
];

/// GetFontTrait answers fonts 1-8; font 9 reads 0 [observed: sup6 fonts].
pub const LOADED_FONTS: usize = 8;

/// GetModTraitSTR's text for a header without the field [observed: sup6 modtraits].
const NO_TEXT: &str = "(null)";

/// A font file's line height, its first number (a little-endian float) [documented:
/// fonts.md §2]; 0 for a file too short.
pub fn font_line_height(bytes: &[u8]) -> f64 {
    bytes.get(..4).map_or(0.0, |b| f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])))
}

/// The default size of a debug text's letters [hypothesis].
pub const DEBUG_TEXT_ZOOM: f32 = 16.0;

/// One of the cursor's traits and its gradual change [documented: SetCursorTraitGradual,
/// "works similar to SetUIFloatGradual"].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorTrait {
    pub value: f64,
    /// start, end, duration, mode and the game time it started.
    pub gradual: Option<(f64, f64, f64, u8, f64)>,
}

/// The cursor's traits by number: 0 alpha, 1 width, 2 height, 3-5 red, green, blue
/// [documented: GetCursorTrait]. At rest: fully seen, NVForever's arrow's size (`size`)
/// and white, the arrow's own colours [hypothesis].
pub const CURSOR_TRAITS: usize = 6;

/// The lockpick menu as the engine shows it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LockpickView {
    /// The pin's angle in degrees, 0 left, 90 up, 180 right (`crate::lockpick::Picking::pin`).
    pub angle: f32,
    /// The lock's level, 0 to 100 (the record's), and the player's Lockpick skill.
    pub level: f32,
    pub skill: f32,
    /// The pin's health, 1 new [documented: SetBobbyPinHealth, "will start at 1.0"].
    pub health: f32,
}

/// The mouse as the engine saw it this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Mouse {
    /// The pointer's place on the system's screen, in pixels [documented:
    /// GetMousePosition, "system mouse position"]; the engine gives the window's
    /// pixels from the top left, the last place while the pointer is outside it.
    pub pos: (f64, f64),
    /// This frame's movement and the last frame's that moved [documented:
    /// GetMouseMovement, 0 current, 1 last; hypothesis: "last" is the last that moved].
    pub moved: (f64, f64),
    pub last_moved: (f64, f64),
    /// The wheel's notches this frame and the last that turned, as above.
    pub wheel: f64,
    pub last_wheel: f64,
}

/// What batch 6 keeps; the engine reads and fills it.
#[derive(Debug, Clone)]
pub struct SupState {
    pub debug_texts: BTreeMap<String, DebugText>,
    pub cursor: [CursorTrait; CURSOR_TRAITS],
    /// The arrow's size in pixels (the engine writes it once it has the arrow), for the
    /// width and height traits at rest.
    pub cursor_size: (f64, f64),
    pub mouse: Mouse,
    /// FakeMouseMovement's moves this frame, for the engine.
    pub fake_moves: Vec<(f32, f32)>,
    /// The window's size in pixels.
    pub window: (f64, f64),
    /// By font number (index 0 is font 1): the file's line height, which GetFontTrait
    /// gives as both the base and the line height [observed: sup6 fonts].
    pub fonts: Vec<f64>,
    pub lockpick: Option<LockpickView>,
    /// SetBobbyPinHealth's new health, for the engine's menu.
    pub pin_health: Option<f32>,
    /// Seconds the player held the throw of the throwable weapon in hand: from the
    /// attack's press to its release, kept after the throw until the next one
    /// [documented: GetGrenadeTimeHeld's note].
    pub grenade_held: f64,
}

impl Default for SupState {
    fn default() -> Self {
        let rest = |v: f64| CursorTrait { value: v, gradual: None };
        SupState {
            debug_texts: BTreeMap::new(),
            cursor: [rest(255.0), rest(f64::NAN), rest(f64::NAN), rest(255.0), rest(255.0), rest(255.0)],
            cursor_size: (32.0, 32.0),
            mouse: Mouse::default(),
            fake_moves: Vec::new(),
            window: (0.0, 0.0),
            fonts: Vec::new(),
            lockpick: None,
            pin_health: None,
            grenade_held: 0.0,
        }
    }
}

/// A gradual change's value at `t` seconds since it started [documented:
/// SetCursorTraitGradual's modes]: 0 start to end in the duration, then end; 1 start to
/// end in a sixth, end for two thirds, back to start in a sixth, then start; 2 start to
/// end and back in the duration, again and again; 3 start to end in the duration, again
/// and again.
pub fn gradual_value(start: f64, end: f64, duration: f64, mode: u8, t: f64) -> f64 {
    let lerp = |f: f64| start + (end - start) * f.clamp(0.0, 1.0);
    if duration <= 0.0 {
        return if mode == 1 { start } else { end };
    }
    match mode {
        1 => {
            let s = duration / 6.0;
            if t < s {
                lerp(t / s)
            } else if t < duration - s {
                end
            } else if t < duration {
                lerp((duration - t) / s)
            } else {
                start
            }
        }
        2 => {
            let p = (t / duration).fract();
            if p < 0.5 {
                lerp(p * 2.0)
            } else {
                lerp((1.0 - p) * 2.0)
            }
        }
        3 => lerp((t / duration).fract()),
        _ => lerp(t / duration),
    }
}

impl SupState {
    /// A cursor trait's value at game time `now`: NaN width and height stand for the
    /// arrow's own size.
    pub fn cursor_value(&self, k: usize, now: f64) -> f64 {
        let Some(c) = self.cursor.get(k) else { return 0.0 };
        let v = match c.gradual {
            Some((s, e, d, m, t0)) => gradual_value(s, e, d, m, now - t0),
            None => c.value,
        };
        match (k, v.is_nan()) {
            (1, true) => self.cursor_size.0,
            (2, true) => self.cursor_size.1,
            _ => v,
        }
    }

    /// A cursor trait as GetCursorTrait reads it: in a menu its value at `now`, out of
    /// menus the value as it was before a gradual change [observed: sup6 cursor.gradual].
    pub fn cursor_read(&self, k: usize, now: f64, menu: bool) -> f64 {
        if menu {
            return self.cursor_value(k, now);
        }
        let Some(c) = self.cursor.get(k) else { return 0.0 };
        match (k, c.value.is_nan()) {
            (1, true) => self.cursor_size.0,
            (2, true) => self.cursor_size.1,
            _ => c.value,
        }
    }

    /// Every cursor trait at `now`, for the engine.
    pub fn cursor_values(&self, now: f64) -> [f64; CURSOR_TRAITS] {
        std::array::from_fn(|k| self.cursor_value(k, now))
    }
}

pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let _ = (ctx, subject);
    let zero = XVal::Num(0.0);
    let menu = g.in_menu() || g.loading;
    let v = match name {
        // ---- the mouse ----
        // `xOut yOut`: the system's pointer [documented: GetMousePosition]. The original's
        // runtime compiler refused this and GetMouseMovement with their variables
        // [observed: sup6 mouse.pos, mouse.moves], as SUP's other getters into variables
        // (ext-sup3.md); scripts compiled ahead keep the pages' form.
        "getmouseposition" => {
            let (x, y) = g.sup.mouse.pos;
            outputs(g, 0.0, &[x, y])
        }
        // `xOut yOut request`: this frame's movement (0) or the last (1), only in a menu
        // [documented: GetMouseMovement]; 0 0 out of menus (see docs/spec-notes/ext-sup6.md §1). The VM leaves
        // the two output variables out of `args`: the request is the first left.
        "getmousemovement" => {
            let m = g.sup.mouse;
            let (x, y) = match (menu, num(args, 0, 0.0) as i32) {
                (false, _) => (0.0, 0.0),
                (true, 1) => m.last_moved,
                (true, _) => m.moved,
            };
            outputs(g, 0.0, &[x, y])
        }
        // `request`: the wheel's notches, the same way [documented: GetMouseWheelMovement].
        "getmousewheelmovement" => {
            let m = g.sup.mouse;
            XVal::Num(match (menu, num(args, 0, 0.0) as i32) {
                (false, _) => 0.0,
                (true, 1) => m.last_wheel,
                (true, _) => m.wheel,
            })
        }
        // `x y`: moves the camera or the cursor as the mouse would, by pixels
        // [documented: FakeMouseMovement]; the engine adds it to the next frame's
        // movement. Non-finite deltas are ignored [design].
        "fakemousemovement" => {
            let (x, y) = (num(args, 0, 0.0), num(args, 1, 0.0));
            if x.is_finite() && y.is_finite() {
                g.sup.fake_moves.push((x as f32, y as f32));
            }
            zero
        }
        // ---- the cursor's look ----
        // [documented: GetCursorTrait, SetCursorTrait, the traits 0-5]. The colours and
        // the alpha are 0-255 [hypothesis: as the UI's rgb traits].
        // Out of menus a gradual change does not move the value read: it reads what it
        // read before the change [observed: sup6 cursor.gradual, 255 at the call, and
        // cursor.gradual.next, a frame on still outside 50-200 and the same after the
        // stop; hypothesis: the change runs only while the cursor is shown].
        "getcursortrait" => {
            let k = num(args, 0, -1.0);
            XVal::Num(if (0.0..CURSOR_TRAITS as f64).contains(&k) { g.sup.cursor_read(k as usize, g.time, menu) } else { 0.0 })
        }
        "setcursortrait" => {
            let k = num(args, 0, -1.0);
            if (0.0..CURSOR_TRAITS as f64).contains(&k) {
                let v = num(args, 1, 0.0);
                if v.is_finite() {
                    g.sup.cursor[k as usize] = CursorTrait { value: v, gradual: None };
                }
            }
            zero
        }
        // `trait [start [end [duration [mode]]]]`: no more than the trait stops the
        // change where it is [documented: "SetCursorTraitGradual 0 Will stop"]; a start
        // alone sets it (see docs/spec-notes/ext-sup6.md §2). Modes over 3 read 0.
        "setcursortraitgradual" => {
            let k = num(args, 0, -1.0);
            if (0.0..CURSOR_TRAITS as f64).contains(&k) {
                let k = k as usize;
                let now = g.time;
                let here = g.sup.cursor_read(k, now, menu);
                let fin = |i: usize| num(args, i, 0.0).is_finite().then(|| num(args, i, 0.0));
                g.sup.cursor[k] = match args.len() {
                    0 | 1 => CursorTrait { value: here, gradual: None },
                    2 => CursorTrait { value: fin(1).unwrap_or(here), gradual: None },
                    _ => {
                        let mode = num(args, 4, 0.0) as i64;
                        let mode = if (0..=3).contains(&mode) { mode as u8 } else { 0 };
                        let (s, e, d) = (fin(1).unwrap_or(here), fin(2).unwrap_or(here), fin(3).unwrap_or(0.0).max(0.0));
                        // `value` keeps the value read out of menus (above).
                        CursorTrait { value: g.sup.cursor[k].value, gradual: Some((s, e, d, mode, now)) }
                    }
                };
            }
            zero
        }
        // ---- the debug texts ----
        // `name x y text`: creates a new debug text and returns 1; if `name` already
        // exists, updates its position and text in place (preserving zoom) [documented:
        // DebugTextCreate; hypothesis: an existing name is moved and changed, not doubled]
        // while returning -1 [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
        // SUP NVSE 855, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official
        // base masters, `w4c_b4_03_sup6_bobbypin_debugtext`: `dt(1,-1)`].
        "debugtextcreate" => {
            let Some(n) = text(args, 0) else { return Some(Ok(zero)) };
            let existed = g.sup.debug_texts.contains_key(&n);
            let zoom = g.sup.debug_texts.get(&n).map_or(DEBUG_TEXT_ZOOM, |d| d.zoom);
            g.sup.debug_texts.insert(n, DebugText { x: num(args, 1, 0.0) as f32, y: num(args, 2, 0.0) as f32, text: text(args, 3).unwrap_or_default(), zoom });
            XVal::Num(if existed { -1.0 } else { 1.0 })
        }
        "debugtextdestroy" => {
            if let Some(n) = text(args, 0) {
                g.sup.debug_texts.remove(&n);
            }
            zero
        }
        // [documented: DebugTextExists, 0/1].
        "debugtextexists" => XVal::Num(f64::from(u8::from(text(args, 0).is_some_and(|n| g.sup.debug_texts.contains_key(&n))))),
        "debugtextsetpos" | "debugtextsetstring" | "debugtextsetzoom" => {
            let Some(d) = text(args, 0).and_then(|n| g.sup.debug_texts.get_mut(&n)) else { return Some(Ok(zero)) };
            match name {
                "debugtextsetpos" => (d.x, d.y) = (num(args, 1, f64::from(d.x)) as f32, num(args, 2, f64::from(d.y)) as f32),
                "debugtextsetstring" => d.text = text(args, 1).unwrap_or_default(),
                _ => d.zoom = num(args, 1, f64::from(d.zoom)).max(0.0) as f32,
            }
            zero
        }
        // ---- the HUD's visibility flags ----
        // "replaced by an equivalent function in JohnnyGuitar NVSE" [documented:
        // GetHUDVisibilityFlags, SetHUDVisibilityFlags]: the same flags as
        // Get/SetHUDVisibilityOverride (`crate::nvse_hud`).
        "gethudvisibilityflags" => XVal::Num(f64::from(g.hud.visibility)),
        "sethudvisibilityflags" => {
            g.hud.visibility = (num(args, 0, 0.0) as i64).clamp(0, i64::from(u32::MAX)) as u32;
            zero
        }
        // ---- the screen ----
        // [documented: GetScreenTrait] 0, 1 the UI's width and height, 2, 3 the window's
        // pixels, 4 the resolution converter. The UI's size is in interface units with no
        // menu open too: 960 high and 960 x width/height wide, the converter 960/height
        // [observed: sup6 screen, `1706.67 960 1280 720 1.33333` in a 1280 x 720 window;
        // the hud_menus capture of 2026-10-31, menu-xml.md]. No window (a headless run):
        // 0 for all five [decision: the values are the window's].
        "getscreentrait" => {
            let (w, h) = g.sup.window;
            let has = w > 0.0 && h > 0.0;
            XVal::Num(match (has, num(args, 0, -1.0) as i32) {
                (false, _) => 0.0,
                (true, 0) => 960.0 * w / h,
                (true, 1) => 960.0,
                (true, 2) => w,
                (true, 3) => h,
                (true, 4) => 960.0 / h,
                _ => 0.0,
            })
        }
        // `font trait`: 0 the base height, 1 the line height [documented: GetFontTrait];
        // both are the font file's line height, its first number, in fonts 1-8, and font 9
        // reads 0 0 [observed: sup6 fonts, `31/31 31/31 26/26 36/36 25/25 34/34 36/36
        // 52/52 0/0`; fonts.md, monofonto_large 31 and glow_monofonto_vl_dialogs 34]. The
        // engine gives its fonts (the install's own files); with none given (a headless
        // run) the default files are read [`FONT_FILES`].
        "getfonttrait" => {
            let f = num(args, 0, 0.0) as i64;
            if g.sup.fonts.is_empty() {
                g.sup.fonts = FONT_FILES.iter().map(|p| font_line_height(&crate::mod_files::read_data(p).unwrap_or_default())).collect();
            }
            let v = usize::try_from(f - 1).ok().filter(|&i| i < LOADED_FONTS).and_then(|i| g.sup.fonts.get(i)).copied().unwrap_or(0.0);
            XVal::Num(v)
        }
        // `modIndex trait`: the plugin's author (0) and description (1), TES4's CNAM and
        // SNAM [documented: GetModTraitSTR]; "" for another index or trait.
        "getmodtraitstr" => {
            let i = num(args, 0, -1.0);
            let t = num(args, 1, -1.0) as i32;
            let v = (i >= 0.0).then(|| g.data.plugin_texts.get(i as usize)).flatten().map(|(a, d)| match t {
                // A text the header lacks reads "(null)" [observed: sup6 modtraits,
                // FalloutNV.esm's description].
                0 => a.clone().unwrap_or_else(|| NO_TEXT.into()),
                1 => d.clone().unwrap_or_else(|| NO_TEXT.into()),
                _ => String::new(),
            });
            XVal::Str(v.unwrap_or_default())
        }
        // The computer's Windows version, memory and time since it started: 0
        // [decision: no channel to the machine's state (the 2026-11-08 brief); follow-up].
        "getpctrait" => zero,
        // ---- the lockpick menu ----
        // `request`: 0 the pin's angle (NVForever's 0..180 degrees [design]; original SUP
        // NVSE 855 returned 0.89 in `w4h_11_sup_lockpick_menu_tiers` and 952.47 in
        // `w4e_12_sup_lockpick_menu_value_tiers`), 1 the lock tier enum (0 Very Easy/0,
        // 1 Easy/1..=25, 2 Average/26..=50, 3 Hard/51..=75, 4 Very Hard/76..=100), 2 the
        // Lockpick skill, other requests 0.0 while open; -1 with the menu shut
        // [documented: GetLockPickMenuValue; observed: Fallout: New Vegas 1.4.0.525 +
        // xNVSE 6.4.9 + SUP NVSE 855, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`),
        // 10 official base masters, isolated clean-room staging copy (read-only
        // `PrintToConsole` probe; zero writes to game install or Saves),
        // `sup_20_lockpick_and_bobbypin_closed`, `w4e_12_sup_lockpick_menu_value_tiers`
        // (`tools/capture/nvse/catalogs/wave4e_mods.json`, #3802), and
        // `w4h_11_sup_lockpick_menu_tiers` (`tools/capture/nvse/catalogs/wave4h_reconcile.json`,
        // #3802): `PROBE|w4h_11_sup_lockpick_menu_tiers|shut(-1.0,-1.0,-1.0,-1.0,-1.0)|open50(mm=1014,0.89,2.0,100.0,0.0,0.0)|tiers(1=1.0,25=1.0,50=2.0,75=3.0,100=4.0)`;
        // zero DLL, PDB, or third-party source consulted].
        "getlockpickmenuvalue" => XVal::Num(match (g.sup.lockpick, num(args, 0, -1.0) as i32) {
            (Some(l), 0) => f64::from(l.angle),
            (Some(l), 1) => {
                let raw = if l.level.is_finite() { l.level.clamp(0.0, 255.0) as u8 } else { 0 };
                crate::lockpick::Level::of(raw).index() as f64
            }
            (Some(l), 2) => f64::from(l.skill),
            (Some(_), _) => 0.0,
            (None, _) => -1.0,
        }),
        // `[health]`: sets the pin's health, above 1 too; without it returns the health
        // [documented: SetBobbyPinHealth]. With the menu shut: -1 and nothing set
        // [hypothesis: as GetLockPickMenuValue].
        "setbobbypinhealth" => match (g.sup.lockpick.as_mut(), args.first()) {
            (None, _) => XVal::Num(-1.0),
            (Some(l), Some(XVal::Num(h))) if h.is_finite() => {
                l.health = (*h as f32).max(0.0);
                g.sup.pin_health = Some(l.health);
                XVal::Num(f64::from(l.health))
            }
            (Some(l), _) => XVal::Num(f64::from(l.health)),
        },
        // ---- the throw held ----
        // [documented: GetGrenadeTimeHeld].
        "getgrenadetimeheld" => XVal::Num(g.sup.grenade_held),
        _ => return None,
    };
    Some(Ok(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradual_modes() {
        let r = |x: f64| (x * 1000.0).round() / 1000.0;
        assert_eq!(r(gradual_value(200.0, 50.0, 4.0, 0, 2.0)), 125.0);
        assert_eq!(r(gradual_value(200.0, 50.0, 4.0, 0, 9.0)), 50.0);
        // Mode 1: up in a sixth, held, down in a sixth.
        assert_eq!(r(gradual_value(0.0, 60.0, 6.0, 1, 0.5)), 30.0);
        assert_eq!(r(gradual_value(0.0, 60.0, 6.0, 1, 3.0)), 60.0);
        assert_eq!(r(gradual_value(0.0, 60.0, 6.0, 1, 5.5)), 30.0);
        assert_eq!(r(gradual_value(0.0, 60.0, 6.0, 1, 7.0)), 0.0);
        // Mode 2: there and back each period; mode 3: there each period.
        assert_eq!(r(gradual_value(0.0, 10.0, 2.0, 2, 1.0)), 10.0);
        assert_eq!(r(gradual_value(0.0, 10.0, 2.0, 2, 2.5)), 5.0);
        assert_eq!(r(gradual_value(0.0, 10.0, 2.0, 3, 3.0)), 5.0);
    }
}
