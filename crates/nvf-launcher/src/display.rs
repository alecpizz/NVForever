//! The Display rows the launcher shows as lists rather than text: Resolution, Monitor,
//! Display sync and Frame rate limit. Each list's words and the text it writes are the game's
//! (engine/nvforever/src/window_mode.rs, frame_pace.rs, settings.rs); the tests read those
//! files so a change there shows here.

/// One entry of a list: what the page shows and what the INI keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    pub stored: String,
}

fn choice(label: impl Into<String>, stored: impl Into<String>) -> Choice {
    Choice { label: label.into(), stored: stored.into() }
}

/// A display attached to the desktop: its device name (what `monitor=` keeps, `\\.\DISPLAY1`
/// on Windows), its desktop size and position, the sizes of its video modes and whether it
/// is the system's primary display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Monitor {
    pub name: String,
    pub size: (u32, u32),
    pub position: (i32, i32),
    pub modes: Vec<(u32, u32)>,
    pub primary: bool,
}

/// Display sync, as `DisplaySync::name` / `stored` have it (frame_pace.rs).
pub fn sync_choices() -> Vec<Choice> {
    vec![choice("Off", "off"), choice("Vertical sync", "vsync"), choice("Variable refresh", "variable")]
}

/// The frame rate limits (frame_pace.rs `CAPS`; 0 is Off).
pub const CAPS: [u32; 6] = [0, 30, 60, 90, 120, 144];

/// Frame rate limit: a slider of [`CAPS`] steps in the game, kept as the step's part of 0..1
/// (`choice_part`: 0.00, 0.20, … 1.00).
pub fn cap_choices() -> Vec<Choice> {
    let n = CAPS.len() - 1;
    CAPS.iter().enumerate().map(|(i, c)| choice(if *c == 0 { "Off".to_string() } else { format!("{c} a second") }, format!("{:.2}", i as f32 / n as f32))).collect()
}

/// The cap a stored part stands for (`choice_index`).
pub fn cap_index(stored: &str) -> Option<usize> {
    let v: f32 = stored.trim().parse().ok()?;
    let n = CAPS.len() - 1;
    Some(((v.clamp(0.0, 1.0) * n as f32).round() as usize).min(n))
}

/// Physics rate (nvf-play phys_rate.rs `RateChoice::LISTED`: `60 Hz` … `Match FPS`, kept
/// as `60` … `fps`).
pub fn physics_choices() -> Vec<Choice> {
    [60, 120, 144, 160].iter().map(|hz| choice(format!("{hz} Hz"), hz.to_string())).chain([choice("Match FPS", "fps")]).collect()
}

/// Counter: last figure (perf_overlay/readings.rs `Low`).
pub fn counter_low_choices() -> Vec<Choice> {
    vec![choice("1% low", "low"), choice("Slowest frame", "slowest")]
}

/// Crosshair style (nvf-play reticle.rs `CrosshairStyle`, kept by its name).
pub fn crosshair_choices() -> Vec<Choice> {
    vec![choice("Cross", "Cross"), choice("Arrows", "Arrows")]
}

/// The HUD's and the Pip-Boy's named colours (nvf-play ui_colour.rs `PRESETS`, Amber being
/// interface.rs `AMBER_RGB`), then Custom; kept by name.
pub const PRESETS: [(&str, [u8; 3]); 4] = [("Amber", [255, 182, 66]), ("Green", [26, 255, 128]), ("Blue", [46, 207, 255]), ("White", [230, 240, 255])];
pub const CUSTOM: &str = "Custom";

pub fn colour_choices() -> Vec<Choice> {
    PRESETS.iter().map(|(n, _)| choice(*n, *n)).chain([choice(CUSTOM, CUSTOM)]).collect()
}

/// A colour row's three part rows (red, green, blue).
pub fn colour_parts(key: &str) -> Option<[&'static str; 3]> {
    match key {
        "hud-colour" => Some(["hud-red", "hud-green", "hud-blue"]),
        "pipboy-colour" => Some(["pipboy-red", "pipboy-green", "pipboy-blue"]),
        _ => None,
    }
}

/// The colour row a part row belongs to.
pub fn colour_of_part(key: &str) -> Option<&'static str> {
    ["hud-colour", "pipboy-colour"].into_iter().find(|c| colour_parts(c).is_some_and(|p| p.contains(&key)))
}

pub fn preset_rgb(name: &str) -> Option<[u8; 3]> {
    PRESETS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())).map(|(_, c)| *c)
}

/// Language: English (`en`), then each language file found (`lang/<id>.toml` beside the
/// program or in the home), by its own name; kept by id (nvf-play language.rs).
pub fn language_choices(dirs: &[std::path::PathBuf]) -> Vec<Choice> {
    let (tables, _) = fnv_util::words::read_dirs(dirs);
    std::iter::once(choice("English", "en")).chain(tables.into_iter().map(|t| choice(t.name, t.id))).collect()
}

/// The words of a stepped slider's steps, as the game's page shows them: HUD scale 50% to
/// 150% (interface.rs, 21 steps), Menu scale 50% to 100% (menu_scale.rs, 11 steps), the
/// fields of view in degrees, a colour part 0 to 255.
pub fn steps(key: &str) -> Option<Vec<String>> {
    match key {
        "ui-scale" => Some((0..21).map(|i| format!("{}%", 50 + 5 * i)).collect()),
        "menu-scale" => Some((0..11).map(|i| format!("{}%", 50 + 5 * i)).collect()),
        // graphics.rs AA_CHOICES, SHADOW_FILTERS and SHADOW_RESOLUTIONS, shadow_rate.rs
        // SUN_SHADOW_CHOICES, light_shadows.rs CHOICES.
        "anti-aliasing" => Some(["Off", "2x", "4x", "8x", "FXAA", "SMAA", "TAA"].map(String::from).to_vec()),
        "shadow-filter" => Some(["Fast", "Smooth", "Temporal"].map(String::from).to_vec()),
        "shadow-resolution" => Some(["2048", "4096"].map(String::from).to_vec()),
        "light-shadows" => Some(["Off", "Performance", "Quality"].map(String::from).to_vec()),
        "sun-shadows" => Some(["Off", "Performance", "Quality"].map(String::from).to_vec()),
        // full_detail.rs FullDetail::CHOICES (#2701).
        "full-detail" => Some(["Off", "7 x 7", "9 x 9"].map(String::from).to_vec()),
        // distant_objects.rs DistantObjects::CHOICES (#3014).
        "distant-objects" => Some(["Off", "8 cells", "12 cells"].map(String::from).to_vec()),
        // Degrees in steps of 5 (engine settings.rs FOV_MIN..FOV_MAX, VIEWMODEL_FOV_*).
        "field-of-view" => Some((0..13).map(|i| format!("{}°", 50 + 5 * i)).collect()),
        "viewmodel-field-of-view" => Some((0..13).map(|i| format!("{}°", 30 + 5 * i)).collect()),
        // The light in the air's strength, 0 % to 200 % in steps of 10 (engine
        // settings.rs `air_strength_slider`: the stored part is strength / 2).
        "volumetric-light-strength" => Some((0..21).map(|i| format!("{}%", 10 * i)).collect()),
        // The Custom difficulty's three: 50-300% in steps of 5 (nvf-play difficulty.rs).
        "difficulty-player-damage" | "difficulty-npc-damage" | "difficulty-kill-xp" => Some((0..51).map(|i| format!("{}%", 50 + 5 * i)).collect()),
        k if colour_of_part(k).is_some() => Some((0..=255).map(|i: u32| i.to_string()).collect()),
        _ => None,
    }
}

/// The lists kept by their names: Button style (nvf-play pad.rs `ButtonStyle`), Upscaler
/// (engine settings.rs `UPSCALERS`; its "Download DLSS…" is the game's own), Upscaling mode
/// (upscaler.rs `UpscaleQuality`), DLSS model (`DlssModel`), FSR library (settings.rs) and
/// Generated frames (`FRAMES_CHOICES`).
pub fn named_choices(key: &str) -> Option<Vec<Choice>> {
    let names: &[&str] = match key {
        "button-style" => &["Auto", "Xbox", "PlayStation"],
        "upscaler" => &["Off", "FSR 3.1", "DLSS"],
        "upscaler-quality" => &["Native AA", "Quality", "Balanced", "Performance", "Ultra Performance"],
        "dlss-model" => &["Default", "K", "L", "M", "J"],
        "fsr-library" => &["Built-in", "Game folder"],
        "frame-generation-frames" => &["2x", "3x", "4x"],
        _ => return None,
    };
    Some(names.iter().map(|n| choice(*n, *n)).collect())
}

/// Frame generation (frame_gen.rs `FrameGenKind`: shown `FSR 3.1`, kept `fsr`).
pub fn frame_gen_choices() -> Vec<Choice> {
    vec![choice("Off", "off"), choice("FSR 3.1", "fsr"), choice("DLSS", "dlss")]
}

/// Shared list aliases for the values that do not depend on a display or language directory.
pub(crate) fn common_choices(key: &str) -> Option<Vec<Choice>> {
    match key {
        "display-sync" => Some(sync_choices()),
        "frame-cap" => Some(cap_choices()),
        "physics-rate" => Some(physics_choices()),
        "fps-counter-low" => Some(counter_low_choices()),
        "crosshair-style" => Some(crosshair_choices()),
        "hud-colour" | "pipboy-colour" => Some(colour_choices()),
        "frame-generation" => Some(frame_gen_choices()),
        k => named_choices(k),
    }
}

/// Rows whose value the game works out from the rows under them (quality.rs): the Quality
/// preset (the Enhanced effects row went with the settings menu overhaul, #2803). Choosing one
/// in the game rewrites those rows (a preset from the game folder's preset files); the INI's
/// word alone does nothing, so the launcher shows it only.
pub fn read_only(key: &str) -> bool {
    key == "quality"
}

/// The game's name for display `i` of the left-to-right order (`monitor_label`).
pub fn monitor_label(i: usize, m: &Monitor) -> String {
    format!("Display {} ({}x{})", i + 1, m.size.0, m.size.1)
}

/// Monitor: the displays in the game's order (left to right, then top to bottom).
pub fn monitor_choices(monitors: &[Monitor]) -> Vec<Choice> {
    monitors.iter().enumerate().map(|(i, m)| choice(monitor_label(i, m), m.name.clone())).collect()
}

/// The display `monitor=` names, else the primary display, else the first (the game's rule,
/// window_mode.rs `chosen_index`, #3334).
pub fn chosen_monitor<'a>(monitors: &'a [Monitor], stored: Option<&str>) -> Option<&'a Monitor> {
    stored.and_then(|s| monitors.iter().find(|m| m.name.eq_ignore_ascii_case(s.trim()))).or_else(|| monitors.iter().find(|m| m.primary)).or_else(|| monitors.first())
}

/// Sizes offered when the displays cannot be asked (not Windows): the common ones.
const COMMON: [(u32, u32); 8] = [(1280, 720), (1366, 768), (1600, 900), (1920, 1080), (1920, 1200), (2560, 1440), (3440, 1440), (3840, 2160)];

/// The sizes of `m`'s modes and its desktop, at least 640x480, smallest first, once each
/// (`MonitorModes::sizes`); the common sizes without a display.
pub fn sizes(m: Option<&Monitor>) -> Vec<(u32, u32)> {
    let mut s: Vec<(u32, u32)> = match m {
        Some(m) => m.modes.iter().copied().filter(|s| s.0 >= 640 && s.1 >= 480).chain([m.size]).collect(),
        None => COMMON.to_vec(),
    };
    s.sort_by_key(|s| (u64::from(s.0) * u64::from(s.1), s.0));
    s.dedup();
    s
}

pub fn size_text(s: (u32, u32)) -> String {
    format!("{}x{}", s.0, s.1)
}

/// A size from "WxH", as the game reads it (`parse_size`: 320x240 to 16384 a side).
pub fn parse_size(t: &str) -> Option<(u32, u32)> {
    let (w, h) = t.trim().split_once(['x', 'X'])?;
    let s = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    ((320..=16384).contains(&s.0) && (240..=16384).contains(&s.1)).then_some(s)
}

/// `resolution=` is `<window size>;<full screen size or desktop>` (`resolution_save`); a
/// window size alone keeps full screen at the desktop's.
pub fn split_resolution(v: &str) -> (String, String) {
    match v.split_once(';') {
        Some((w, f)) if !f.trim().is_empty() => (w.trim().to_string(), f.trim().to_string()),
        Some((w, _)) => (w.trim().to_string(), "desktop".to_string()),
        None => (v.trim().to_string(), "desktop".to_string()),
    }
}

pub fn join_resolution(window: &str, full: &str) -> String {
    format!("{window};{full}")
}

/// The window sizes: none larger than the display's desktop (the game's rule in a window),
/// with the one in use kept.
pub fn window_choices(m: Option<&Monitor>, current: &str) -> Vec<Choice> {
    let mut s: Vec<(u32, u32)> = sizes(m).into_iter().filter(|s| m.is_none_or(|m| s.0 <= m.size.0 && s.1 <= m.size.1)).collect();
    keep_current(&mut s, current);
    s.into_iter().map(|s| choice(size_text(s), size_text(s))).collect()
}

/// The full screen sizes: the display's own (`desktop`, borderless at its size) first, then
/// every size of its modes, with the one in use kept.
pub fn full_choices(m: Option<&Monitor>, current: &str) -> Vec<Choice> {
    let own = match m {
        Some(m) => format!("The display's own ({})", size_text(m.size)),
        None => "The display's own".to_string(),
    };
    let mut s = sizes(m);
    keep_current(&mut s, current);
    std::iter::once(choice(own, "desktop")).chain(s.into_iter().map(|s| choice(size_text(s), size_text(s)))).collect()
}

fn keep_current(s: &mut Vec<(u32, u32)>, current: &str) {
    if let Some(c) = parse_size(current) {
        if !s.contains(&c) {
            s.push(c);
            s.sort_by_key(|s| (u64::from(s.0) * u64::from(s.1), s.0));
        }
    }
}

/// The entry of `choices` that `value` stands for: the same text (any case, and a size in
/// any `x`/`X` spelling), or for the frame rate limit the same step.
pub fn chosen(key: &str, choices: &[Choice], value: &str) -> Option<usize> {
    if key == "frame-cap" {
        return cap_index(value);
    }
    let v = value.trim();
    choices
        .iter()
        .position(|c| c.stored.eq_ignore_ascii_case(v) || (parse_size(v).is_some() && parse_size(&c.stored) == parse_size(v)))
        .or_else(|| choices.iter().position(|c| c.label.eq_ignore_ascii_case(v)))
}

/// The displays attached to the desktop, in the game's order. Asked of Windows
/// (EnumDisplayDevicesW / EnumDisplaySettingsW, user32): the device names are the ones winit
/// gives the game, so `monitor=` matches.
#[cfg(all(windows, feature = "desktop"))]
// The Win32 calls (#4592): the crate denies unsafe code everywhere else.
#[allow(unsafe_code)]
pub fn monitors() -> Vec<Monitor> {
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayDevicesW, EnumDisplaySettingsW, DEVMODEW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_PRIMARY_DEVICE, ENUM_CURRENT_SETTINGS};
    let wide = |w: &[u16]| String::from_utf16_lossy(&w[..w.iter().position(|c| *c == 0).unwrap_or(w.len())]);
    let blank_mode = || {
        // SAFETY: DEVMODEW is plain data (integers, arrays, unions of integers); all zeros is
        // a valid value, and the call reads dmSize.
        let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
        dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
        dm
    };
    let mut out = Vec::new();
    for i in 0.. {
        // SAFETY: as DEVMODEW; cb says the struct's size.
        let mut dd: DISPLAY_DEVICEW = unsafe { std::mem::zeroed() };
        dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        // SAFETY: a null device asks for adapter outputs; dd is a valid, sized struct.
        if unsafe { EnumDisplayDevicesW(std::ptr::null(), i, &mut dd, 0) } == 0 {
            break;
        }
        if dd.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP == 0 {
            continue;
        }
        let mut dm = blank_mode();
        // SAFETY: DeviceName is the NUL-terminated name Windows just wrote; dm is sized.
        if unsafe { EnumDisplaySettingsW(dd.DeviceName.as_ptr(), ENUM_CURRENT_SETTINGS, &mut dm) } == 0 {
            continue;
        }
        // SAFETY: for a display device the union holds the display fields (dmPosition).
        let p = unsafe { dm.Anonymous1.Anonymous2.dmPosition };
        let mut modes = Vec::new();
        for m in 0.. {
            let mut mode = blank_mode();
            // SAFETY: as above; m counts the device's modes until the call says there are no more.
            if unsafe { EnumDisplaySettingsW(dd.DeviceName.as_ptr(), m, &mut mode) } == 0 {
                break;
            }
            modes.push((mode.dmPelsWidth, mode.dmPelsHeight));
        }
        out.push(Monitor { name: wide(&dd.DeviceName), size: (dm.dmPelsWidth, dm.dmPelsHeight), position: (p.x, p.y), modes, primary: dd.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0 });
    }
    out.sort_by_key(|m| m.position);
    out
}

/// Off Windows the displays are not asked (no display crate in the launcher): Resolution
/// offers the common sizes and Monitor stays a text box.
#[cfg(not(all(windows, feature = "desktop")))]
pub fn monitors() -> Vec<Monitor> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(file: &str) -> String {
        std::fs::read_to_string(format!("{}/../../engine/nvforever/src/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn lists_match_the_games_words_and_values() {
        let src = engine("frame_pace.rs");
        for c in sync_choices() {
            assert!(src.contains(&format!("=> \"{}\"", c.label)), "Display sync: {} is not a name in frame_pace.rs", c.label);
            assert!(src.contains(&format!("=> \"{}\"", c.stored)), "Display sync: {} is not stored by frame_pace.rs", c.stored);
        }
        let caps = format!("pub const CAPS: [u32; {}] = {:?};", CAPS.len(), CAPS);
        assert!(src.contains(&caps), "frame_pace.rs no longer has {caps}");
        let wm = engine("window_mode.rs");
        assert!(wm.contains("format!(\"Display {} ({}x{})\", i + 1, m.size.x, m.size.y)"), "monitor_label changed");
        assert!(wm.contains("format!(\"{};{full}\""), "resolution_save changed");
    }

    fn play(file: &str) -> String {
        std::fs::read_to_string(format!("{}/../../engine/nvf-play/src/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn the_other_lists_match_the_game() {
        let p = play("phys_rate.rs");
        assert!(p.contains("pub const LISTED: [RateChoice; 5] = [RateChoice::Fixed(60), RateChoice::Fixed(120), RateChoice::Fixed(144), RateChoice::Fixed(160), RateChoice::Fps];"));
        assert!(p.contains("RateChoice::Fixed(hz) => format!(\"{hz} Hz\")") && p.contains("RateChoice::Fps => \"Match FPS\"") && p.contains("RateChoice::Fps => \"fps\""));
        let r = engine("perf_overlay/readings.rs");
        for c in counter_low_choices() {
            assert!(r.contains(&format!("=> \"{}\"", c.label)) && r.contains(&format!("=> \"{}\"", c.stored)), "{c:?}");
        }
        let x = play("reticle.rs");
        for c in crosshair_choices() {
            assert!(x.contains(&format!("=> \"{}\"", c.label)), "{c:?}");
        }
        assert!(x.contains("save: |w| w.resource::<CrosshairStyle>().name().to_string()"), "the crosshair style is kept by name");
        let u = play("ui_colour.rs");
        assert!(u.contains("pub const PRESETS: [(&str, [u8; 3]); 4] = [(\"Amber\", AMBER_RGB), (\"Green\", [26, 255, 128]), (\"Blue\", [46, 207, 255]), (\"White\", [230, 240, 255])];"));
        assert!(u.contains("pub const CUSTOM: &str = \"Custom\";"));
        let i = play("interface.rs");
        assert!(i.contains("pub const AMBER_RGB: [u8; 3] = [255, 182, 66];"));
        assert!(i.contains("pub const UI_SCALE_CHOICES: usize = 21;") && i.contains("pub const UI_SCALE_RANGE: (f32, f32) = (0.5, 1.5);"));
        let m = play("menu_scale.rs");
        assert!(m.contains("const CHOICES: usize = 11;") && m.contains("scale.clamp(0.5, 1.0)"));
        let st = engine("settings.rs");
        for c in ["const FOV_MIN: f32 = 50.0;", "const FOV_MAX: f32 = 110.0;", "const FOV_CHOICES: usize = 13;", "const VIEWMODEL_FOV_MIN: f32 = 30.0;", "const VIEWMODEL_FOV_MAX: f32 = 90.0;", "const VIEWMODEL_FOV_CHOICES: usize = 13;"] {
            assert!(st.contains(c), "settings.rs no longer has {c}");
        }
        assert_eq!(steps("field-of-view").unwrap()[5], "75°");
        assert_eq!(steps("viewmodel-field-of-view").unwrap()[5], "55°");
        assert_eq!(steps("ui-scale").unwrap()[10], "100%");
        assert_eq!(steps("menu-scale").unwrap().last().unwrap(), "100%");
        // A stored 0.75 reads 150 %, as the game's page and its exported menu show it.
        let strength = steps("volumetric-light-strength").unwrap();
        assert_eq!((strength.len(), strength[0].as_str(), strength[15].as_str(), strength[20].as_str()), (21, "0%", "150%", "200%"));
        assert_eq!(strength[(0.75f64 * 20.0).round() as usize], "150%");
        assert_eq!(steps("pipboy-blue").unwrap().len(), 256);
        assert_eq!(colour_of_part("hud-green"), Some("hud-colour"));
        assert_eq!(preset_rgb("blue"), Some([46, 207, 255]));
        assert_eq!(chosen("physics-rate", &physics_choices(), "Match FPS"), Some(4));
        let langs = language_choices(&[std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../lang"))]);
        assert_eq!(langs[0], choice("English", "en"));
        assert!(langs.iter().any(|c| c.stored == "de" && c.label == "Deutsch"), "{langs:?}");
    }

    #[test]
    fn graphics_and_controls_lists_match_the_game() {
        let pad = play("pad.rs");
        for c in named_choices("button-style").unwrap() {
            assert!(pad.contains(&format!("ButtonStyle::{} => \"{}\"", c.label.replace(' ', ""), c.label)), "{c:?}");
        }
        assert!(pad.contains("save: |w| w.resource::<ButtonStyle>().name().to_string()"));
        let st = engine("settings.rs");
        assert!(st.contains("const UPSCALERS: [(&str, Upscaler); 3] = [(\"Off\", Upscaler::Off), (\"FSR 3.1\", Upscaler::Fsr), (\"DLSS\", Upscaler::Dlss)];"));
        assert!(st.contains("save: upscaler_chosen,") && st.contains("save: quality_chosen,") && st.contains("save: model_chosen,"));
        assert!(st.contains("const FSR_LIBRARY_BUILT_IN: &str = \"Built-in\";") && st.contains("const FSR_LIBRARY_FOLDER: &str = \"Game folder\";"));
        assert!(st.contains("const FRAMES_CHOICES: [&str; 3] = [\"2x\", \"3x\", \"4x\"];"));
        let up = engine("upscaler.rs");
        for key in ["upscaler-quality", "dlss-model"] {
            for c in named_choices(key).unwrap() {
                assert!(up.contains(&format!("=> \"{}\"", c.label)), "{key}: {c:?}");
            }
        }
        let fg = engine("frame_gen.rs");
        for c in frame_gen_choices() {
            assert!(fg.contains(&format!("=> \"{}\"", c.label)) && fg.contains(&format!("=> \"{}\"", c.stored)), "{c:?}");
        }
        let gr = engine("graphics.rs");
        for w in steps("anti-aliasing").unwrap().iter().chain(steps("shadow-filter").unwrap().iter()).chain(steps("shadow-resolution").unwrap().iter()) {
            assert!(gr.contains(&format!("(\"{w}\", ")), "graphics.rs: {w}");
        }
        assert!(engine("shadow_rate.rs").contains("pub const SUN_SHADOW_CHOICES: [&str; 3] = [\"Off\", \"Performance\", \"Quality\"];"));
        assert!(gr.contains("pub const AA_CHOICES: [(&str, ScreenAa, Msaa); 7]") && gr.contains("pub const SHADOW_FILTERS: [(&str, ShadowFilteringMethod); 3]"));
        assert!(gr.contains("pub const SHADOW_RESOLUTIONS: [(&str, usize); 2]"));
        assert!(engine("light_shadows.rs").contains("pub const CHOICES: [&str; 3] = [\"Off\", \"Performance\", \"Quality\"];"));
        let fd = engine("full_detail.rs");
        for w in steps("full-detail").unwrap() {
            assert!(fd.contains(&format!("(\"{w}\", FullDetail::")), "full_detail.rs: {w}");
        }
        assert!(fd.contains("pub const CHOICES: [(&'static str, FullDetail); 3]"));
        let dob = engine("distant_objects.rs");
        for w in steps("distant-objects").unwrap() {
            assert!(dob.contains(&format!("(\"{w}\", DistantObjects::")), "distant_objects.rs: {w}");
        }
        assert!(dob.contains("pub const CHOICES: [(&'static str, DistantObjects); 3]"));
        assert!(st.contains("pub(crate) const LEGACY_KEY: &str = \"fsr-upscaling\";") && st.contains("set_older_key(app, UPSCALER_KEY, LEGACY_KEY)"));
        assert!(st.contains("if fnv_install::ini_get(&text, section, \"display-sync\").is_some() {") && st.contains("let Some(old) = fnv_install::ini_get(&text, section, \"vsync\")"), "the old vsync key is read only without display-sync");
        let q = engine("quality.rs");
        assert!(q.contains("fn load_quality(_world: &mut World, text: &str) -> bool") && !q.contains("fn load_enhanced("), "the preset's INI word is read only to check it; no Enhanced row (#2803)");
    }

    #[test]
    fn frame_rate_limit_steps() {
        let c = cap_choices();
        assert_eq!(c.iter().map(|c| c.stored.as_str()).collect::<Vec<_>>(), ["0.00", "0.20", "0.40", "0.60", "0.80", "1.00"]);
        assert_eq!((c[0].label.as_str(), c[2].label.as_str()), ("Off", "60 a second"));
        assert_eq!(chosen("frame-cap", &c, "0.00"), Some(0));
        assert_eq!(chosen("frame-cap", &c, "0.4"), Some(2));
        assert_eq!(chosen("frame-cap", &c, "1"), Some(5));
        assert_eq!(chosen("frame-cap", &c, "x"), None);
    }

    fn monitor(name: &str, size: (u32, u32), modes: &[(u32, u32)]) -> Monitor {
        Monitor { name: name.into(), size, position: (0, 0), modes: modes.to_vec(), primary: false }
    }

    #[test]
    fn resolutions() {
        assert_eq!(split_resolution("1920x1080;desktop"), ("1920x1080".into(), "desktop".into()));
        assert_eq!(split_resolution("1280x720"), ("1280x720".into(), "desktop".into()));
        assert_eq!(split_resolution("1280x720;2560x1440"), ("1280x720".into(), "2560x1440".into()));
        assert_eq!(join_resolution("1600x900", "desktop"), "1600x900;desktop");
        let m = monitor(r"\\.\DISPLAY1", (1920, 1080), &[(640, 400), (1280, 720), (1920, 1080), (1920, 1080), (2560, 1440), (1600, 900)]);
        assert_eq!(sizes(Some(&m)), [(1280, 720), (1600, 900), (1920, 1080), (2560, 1440)]);
        // In a window nothing larger than the desktop; a size in use is kept.
        let w: Vec<String> = window_choices(Some(&m), "1024x768").into_iter().map(|c| c.stored).collect();
        assert_eq!(w, ["1024x768", "1280x720", "1600x900", "1920x1080"]);
        let f = full_choices(Some(&m), "desktop");
        assert_eq!((f[0].label.as_str(), f[0].stored.as_str()), ("The display's own (1920x1080)", "desktop"));
        assert_eq!(f.last().unwrap().stored, "2560x1440");
        assert_eq!(chosen("resolution", &f, "1920X1080"), f.iter().position(|c| c.stored == "1920x1080"));
        assert_eq!(chosen("resolution", &f, "Desktop"), Some(0));
        // No display known: the common sizes.
        assert!(window_choices(None, "").iter().any(|c| c.stored == "1920x1080"));
    }

    #[test]
    fn monitors_by_name() {
        let ms = [monitor(r"\\.\DISPLAY1", (2560, 1440), &[]), monitor(r"\\.\DISPLAY2", (1920, 1080), &[])];
        let c = monitor_choices(&ms);
        assert_eq!((c[1].label.as_str(), c[1].stored.as_str()), ("Display 2 (1920x1080)", r"\\.\DISPLAY2"));
        assert_eq!(chosen_monitor(&ms, Some(r"\\.\display2")).unwrap().size, (1920, 1080));
        assert_eq!(chosen_monitor(&ms, Some("gone")).unwrap().name, r"\\.\DISPLAY1", "the first display");
        assert_eq!(chosen_monitor(&ms, None).unwrap().name, r"\\.\DISPLAY1");
        assert_eq!(chosen("monitor", &c, r"\\.\DISPLAY2"), Some(1));
        // #3334: the primary display right of a portrait one is the default, as in the game.
        let ms = [monitor(r"\\.\DISPLAY2", (1080, 1920), &[]), Monitor { primary: true, ..monitor(r"\\.\DISPLAY1", (3840, 2160), &[]) }];
        assert_eq!(chosen_monitor(&ms, None).unwrap().name, r"\\.\DISPLAY1");
        assert_eq!(chosen_monitor(&ms, Some("gone")).unwrap().name, r"\\.\DISPLAY1");
        assert_eq!(chosen_monitor(&ms, Some(r"\\.\DISPLAY2")).unwrap().size, (1080, 1920));
    }

    /// Windows lists this machine's displays with their modes (none on a machine without
    /// one, such as a service session: then nothing to check).
    #[cfg(windows)]
    #[test]
    fn this_machines_displays() {
        for m in monitors() {
            assert!(m.name.starts_with(r"\\.\"), "{m:?}");
            assert!(m.size.0 > 0 && m.size.1 > 0 && !m.modes.is_empty(), "{m:?}");
        }
    }
}
