//! The interface's two colours as settings: the one the HUD and the menus are drawn in,
//! and the one of the Pip-Boy's screens (`crate::interface::hud_rgb`, `pipboy_rgb`). Each
//! is a row of named colours on the settings page (Amber, the default, a few others and
//! Custom) over a slider for each of red, green and blue (0 to 255), kept in `[Settings]`
//! of `NVForever.ini` like every row (`crate::settings`): `hud-colour=Amber`,
//! `hud-red=1.00`, … The page's Defaults puts both back to amber.
//!
//! **Custom** [design: #1953]: the list always offers it. Taking it keeps the colour as it
//! is and shows the three sliders under it ([`channel_shown`]); taking a named colour sets
//! that colour and hides them again. A colour is Custom while it was chosen so (or a slider
//! was moved, [`CustomColours`]) or while it is none of the named ones (an INI or a test aid
//! setting a slider directly). `hud-colour=Custom` in the INI keeps the choice; there is no
//! key of its own for it. A slider's row keeps its key while hidden: the INI and the test
//! aids set it all the same.
//!
//! Not the original's settings: its own HUD and Pip-Boy colour choices are not copied
//! [design: owner 2026-10-26, "rgb colour pickers" for the HUD and the Pip-Boy, amber
//! the default]. The colours of the presets other than amber are NVForever's [design].
//!
//! What is drawn already is recoloured when a colour changes ([`recolour`]): every UI
//! colour that is the old colour at some brightness becomes the new one at that
//! brightness, the Pip-Boy's screens (under `crate::pipboy::PipBoyRoot`) by its own
//! colour, everything else by the HUD's. What is drawn later takes the new colour from
//! `amber` and `pipboy_amber`. The mouse pointer keeps the amber it was made in
//! (`crate::cursor`). The Pip-Boy device's background follows its colour by its own owner,
//! whose nodes are left alone here (`crate::pipboy::device_screen`).

use crate::interface::{hud_rgb, pipboy_rgb, set_hud_rgb, set_pipboy_rgb, AMBER_RGB};
use crate::settings::{add_choice_list, add_setting, ChoiceList, Setting, Slider};
use bevy::prelude::*;

/// The group these rows register with (shown under Display › Interface,
/// `crate::settings_layout`).
pub const GROUP: &str = "Interface";

/// The named colours, the default first [design, module notes].
pub const PRESETS: [(&str, [u8; 3]); 4] = [("Amber", AMBER_RGB), ("Green", [26, 255, 128]), ("Blue", [46, 207, 255]), ("White", [230, 240, 255])];

/// What the list shows for a colour that is none of [`PRESETS`].
pub const CUSTOM: &str = "Custom";

/// Which of the two colours: 0 the HUD's, 1 the Pip-Boy's.
fn rgb<const W: usize>() -> [u8; 3] {
    if W == 0 {
        hud_rgb()
    } else {
        pipboy_rgb()
    }
}

fn set_rgb<const W: usize>(rgb: [u8; 3]) {
    if W == 0 {
        set_hud_rgb(rgb)
    } else {
        set_pipboy_rgb(rgb)
    }
}

/// The preset's name for `rgb`, else [`CUSTOM`].
pub fn preset_name(rgb: [u8; 3]) -> &'static str {
    PRESETS.iter().find(|(_, c)| *c == rgb).map_or(CUSTOM, |(n, _)| n)
}

/// Whether each colour, the HUD's (0) and the Pip-Boy's (1), is Custom by choice (module
/// notes) [design: #1953]. Not kept on its own: `hud-colour=Custom` is.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomColours(pub [bool; 2]);

/// The name a colour `rgb` goes by in its list: [`CUSTOM`] when chosen so (`custom`) or
/// none of the presets, else the preset's.
pub fn chosen_name(rgb: [u8; 3], custom: bool) -> &'static str {
    if custom {
        CUSTOM
    } else {
        preset_name(rgb)
    }
}

/// The list's choices: the presets, then [`CUSTOM`], always (module notes).
pub fn choice_names() -> Vec<String> {
    PRESETS.iter().map(|(n, _)| n.to_string()).chain([CUSTOM.to_string()]).collect()
}

/// Choice `name` taken: the colour and whether it is Custom by choice after it; `None`: not
/// a choice. Custom keeps the colour as it is; a preset sets its colour [design: #1953].
pub fn picked(name: &str, rgb: [u8; 3]) -> Option<([u8; 3], bool)> {
    if name.eq_ignore_ascii_case(CUSTOM) {
        return Some((rgb, true));
    }
    PRESETS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, c)| (*c, false))
}

/// Colour `which`'s Custom choice (none before the page first sets one).
fn custom_flag(world: &World, which: usize) -> bool {
    world.get_resource::<CustomColours>().is_some_and(|c| c.0[which])
}

fn set_custom_flag(world: &mut World, which: usize, on: bool) {
    world.get_resource_or_insert_with(CustomColours::default).0[which] = on;
}

/// Whether colour `which` (0 the HUD's, 1 the Pip-Boy's) shows its sliders: it is Custom
/// ([`chosen_name`]).
pub fn custom_shown(flags: Option<&CustomColours>, which: usize) -> bool {
    let rgb = if which == 0 { hud_rgb() } else { pipboy_rgb() };
    chosen_name(rgb, flags.is_some_and(|f| f.0[which])) == CUSTOM
}

/// The colour a slider row `key` is a part of: which (0 the HUD's, 1 the Pip-Boy's) and
/// its list row's key; `None` for every other row.
pub fn channel_of(key: &str) -> Option<(usize, &'static str)> {
    match key {
        "hud-red" | "hud-green" | "hud-blue" => Some((0, "hud-colour")),
        "pipboy-red" | "pipboy-green" | "pipboy-blue" => Some((1, "pipboy-colour")),
        _ => None,
    }
}

/// Whether row `key` is shown, as far as the colours go: a colour's slider only while the
/// colour is Custom, every other row always [design: #1953].
pub fn channel_shown(key: &str, flags: Option<&CustomColours>) -> bool {
    channel_of(key).is_none_or(|(which, _)| custom_shown(flags, which))
}

fn names<const W: usize>(_: &World) -> Vec<String> {
    choice_names()
}

fn chosen<const W: usize>(w: &World) -> String {
    chosen_name(rgb::<W>(), custom_flag(w, W)).to_string()
}

/// Take a preset by name, or [`CUSTOM`] (module notes).
fn pick<const W: usize>(w: &mut World, name: &str) -> bool {
    let Some((rgb, custom)) = picked(name, rgb::<W>()) else { return false };
    set_rgb::<W>(rgb);
    set_custom_flag(w, W, custom);
    true
}

fn save<const W: usize>(w: &World) -> String {
    chosen::<W>(w)
}

/// Channel `C` (0 red, 1 green, 2 blue) as a slider's part.
fn channel<const W: usize, const C: usize>(_: &World) -> f32 {
    // Rounded as `Slider::fit` keeps a part, so a stored value compares equal.
    (f32::from(rgb::<W>()[C]) / 255.0 * 1000.0).round() / 1000.0
}

/// A slider moved: the colour is Custom by choice from then on, so its sliders stay up
/// when they pass a named colour's values on the way [design].
fn set_channel<const W: usize, const C: usize>(w: &mut World, v: f32) {
    let mut c = rgb::<W>();
    c[C] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    set_rgb::<W>(c);
    set_custom_flag(w, W, true);
}

/// A slider's part as the 0 to 255 the page shows.
fn shown(v: f32) -> String {
    format!("{:.0}", v * 255.0)
}

fn slider<const W: usize, const C: usize>() -> Slider {
    Slider { get: channel::<W, C>, set: set_channel::<W, C>, min: 0.0, step: 1.0 / 255.0, shown: Some(shown) }
}

fn rows<const W: usize>(app: &mut App, key: &'static str, label: &'static str, note: &'static str, channels: [(&'static str, &'static str); 3]) {
    add_choice_list(
        app,
        Setting { key, group: GROUP, label, note, switches: "", get: |_| false, set: |_, _| {}, slider: None },
        ChoiceList { names: names::<W>, chosen: chosen::<W>, pick: pick::<W>, save: save::<W>, load: pick::<W>, ask_keep: false },
    );
    let sliders = [slider::<W, 0>(), slider::<W, 1>(), slider::<W, 2>()];
    for ((key, label), s) in channels.into_iter().zip(sliders) {
        add_setting(app, Setting::slider(key, GROUP, label, "One part of the colour above, 0 to 255; drag the meter or use Left and Right.", s));
    }
}

/// `c` is `old` at some brightness (not black): the same brightness of `new`, with `c`'s
/// alpha; else `None`.
pub fn retint(c: Color, old: [u8; 3], new: [u8; 3]) -> Option<Color> {
    let s = c.to_srgba();
    let ch = [s.red, s.green, s.blue];
    let o = old.map(|x| f32::from(x) / 255.0);
    let m = (0..3).max_by(|&a, &b| o[a].total_cmp(&o[b]))?;
    if o[m] <= 0.0 || ch[m] <= 0.0 {
        return None;
    }
    let b = ch[m] / o[m];
    if b > 1.0 + 2e-3 || (0..3).any(|i| (ch[i] - o[i] * b).abs() > 1.5 / 255.0) {
        return None;
    }
    let n = new.map(|x| f32::from(x) / 255.0 * b);
    Some(Color::srgba(n[0], n[1], n[2], s.alpha))
}

/// The colours the drawn interface has now (module notes).
#[derive(Resource, Clone, Copy)]
struct Drawn {
    hud: [u8; 3],
    pipboy: [u8; 3],
}

/// Recolour what is drawn when a colour changed (module notes).
#[allow(clippy::type_complexity)]
fn recolour(
    mut drawn: ResMut<Drawn>,
    roots: Query<Entity, With<crate::pipboy::PipBoyRoot>>,
    owned: Query<Entity, With<crate::dialogue_touch::PaletteRoot>>,
    children: Query<&Children>,
    mut nodes: Query<(Entity, Option<&mut TextColor>, Option<&mut ImageNode>, Option<&mut BackgroundColor>, Option<&mut BorderColor>), (Or<(With<TextColor>, With<ImageNode>, With<BackgroundColor>, With<BorderColor>)>, Without<crate::pipboy::device_screen::ScreenFxNode>)>,
) {
    let now = Drawn { hud: hud_rgb(), pipboy: pipboy_rgb() };
    if now.hud == drawn.hud && now.pipboy == drawn.pipboy {
        return;
    }
    let pipboy: Vec<Entity> = roots.iter().flat_map(|r| std::iter::once(r).chain(children.iter_descendants(r))).collect();
    let owned: bevy::ecs::entity::EntityHashSet = owned.iter()
        .flat_map(|root| std::iter::once(root).chain(children.iter_descendants(root)))
        .collect();
    for (e, text, image, back, border) in &mut nodes {
        if owned.contains(&e) {
            continue;
        }
        let (old, new) = if pipboy.contains(&e) { (drawn.pipboy, now.pipboy) } else { (drawn.hud, now.hud) };
        if old == new {
            continue;
        }
        let fix = |c: &mut Color| {
            if let Some(n) = retint(*c, old, new) {
                *c = n;
            }
        };
        if let Some(mut t) = text {
            fix(&mut t.0);
        }
        if let Some(mut i) = image {
            fix(&mut i.color);
        }
        if let Some(mut b) = back {
            fix(&mut b.0);
        }
        if let Some(mut b) = border {
            let b = &mut *b;
            for c in [&mut b.top, &mut b.right, &mut b.bottom, &mut b.left] {
                fix(c);
            }
        }
    }
    info!("interface colours: HUD {:?}, Pip-Boy {:?}", now.hud, now.pipboy);
    *drawn = now;
}

/// Owners with authoritative live palette painting run after generic recolouring.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Recolour;

pub struct UiColourPlugin;

impl Plugin for UiColourPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Drawn { hud: hud_rgb(), pipboy: pipboy_rgb() }).init_resource::<CustomColours>().add_systems(Update, recolour.in_set(Recolour).after(crate::PlaySet::Ui).before(crate::PlaySet::Sync));
        rows::<0>(
            app,
            "hud-colour",
            "HUD colour",
            "The colour of the HUD and the menus: Amber (the default) or another; the sliders below set it to any colour.",
            [("hud-red", "HUD red"), ("hud-green", "HUD green"), ("hud-blue", "HUD blue")],
        );
        rows::<1>(
            app,
            "pipboy-colour",
            "Pip-Boy colour",
            "The colour of the Pip-Boy's screens (items, stats, data): Amber (the default) or another; the sliders below set it to any colour.",
            [("pipboy-red", "Pip-Boy red"), ("pipboy-green", "Pip-Boy green"), ("pipboy-blue", "Pip-Boy blue")],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_is_named_by_its_preset_else_custom() {
        assert_eq!(preset_name(AMBER_RGB), "Amber");
        assert_eq!(preset_name([26, 255, 128]), "Green");
        assert_eq!(preset_name([1, 2, 3]), CUSTOM);
        assert_eq!(PRESETS[0], ("Amber", [255, 182, 66]), "amber is the default");
    }

    #[test]
    fn custom_is_always_offered_and_keeps_the_colour() {
        let names = choice_names();
        assert_eq!(names.last().map(String::as_str), Some(CUSTOM), "at a preset too");
        assert_eq!(names.len(), PRESETS.len() + 1);
        // Custom at amber's values stays Custom, with the values as they were.
        assert_eq!(picked("Custom", AMBER_RGB), Some((AMBER_RGB, true)));
        assert_eq!(chosen_name(AMBER_RGB, true), CUSTOM);
        // A preset leaves Custom and sets its colour.
        assert_eq!(picked("green", [1, 2, 3]), Some(([26, 255, 128], false)));
        assert_eq!(chosen_name([26, 255, 128], false), "Green");
        // Values that are no preset are Custom whatever was chosen (an INI's or an aid's).
        assert_eq!(chosen_name([1, 2, 3], false), CUSTOM);
        assert_eq!(picked("Purple", AMBER_RGB), None);
    }

    #[test]
    fn the_sliders_belong_to_their_colour() {
        assert_eq!(channel_of("hud-green"), Some((0, "hud-colour")));
        assert_eq!(channel_of("pipboy-blue"), Some((1, "pipboy-colour")));
        assert_eq!(channel_of("hud-colour"), None);
        assert!(channel_shown("language", None), "not a slider of a colour: always shown");
    }

    #[test]
    fn the_custom_choice_survives_the_ini_and_defaults_put_back_the_preset() {
        // The list's INI text is its chosen name (`save`), read back by `load` (= `pick`):
        // Custom at a preset's values reads back as Custom with those values, a preset as
        // the preset.
        for (rgb, custom) in [(AMBER_RGB, true), (AMBER_RGB, false), ([1, 2, 3], false), ([26, 255, 128], true)] {
            let saved = chosen_name(rgb, custom);
            let (back, flag) = picked(saved, rgb).unwrap();
            assert_eq!(chosen_name(back, flag), saved, "{rgb:?} {custom}");
            assert_eq!(back, rgb, "the values come back as they were");
        }
        // Defaults loads the start's text ("Amber"): the preset, out of Custom.
        assert_eq!(picked(chosen_name(AMBER_RGB, false), [1, 2, 3]), Some((AMBER_RGB, false)));
    }

    #[test]
    fn a_slider_part_is_a_channel_and_survives_its_steps() {
        let s = slider::<0, 0>();
        for x in [0u8, 1, 66, 128, 182, 254, 255] {
            let v = s.fit(f32::from(x) / 255.0);
            assert_eq!((v * 255.0).round() as u8, x, "{x} on a step");
            assert_eq!(shown(v), x.to_string());
        }
    }

    #[test]
    fn the_device_screens_nodes_are_left_to_their_owner() {
        use crate::pipboy::device_screen::ScreenFxNode;
        // A colour no other test sets, so a change is seen whatever the global colours are.
        let old = [10, 20, 30];
        let full = crate::interface::tint(old, 255.0);
        let mut app = App::new();
        app.insert_resource(Drawn { hud: old, pipboy: old }).add_systems(Update, recolour);
        let hud = app.world_mut().spawn(BackgroundColor(full)).id();
        let fx = app.world_mut().spawn((ScreenFxNode, BackgroundColor(full), ImageNode { color: full, ..default() })).id();
        app.update();
        assert_ne!(app.world().get::<BackgroundColor>(hud).unwrap().0, full, "the HUD's takes the new colour");
        assert_eq!(app.world().get::<BackgroundColor>(fx).unwrap().0, full);
        assert_eq!(app.world().get::<ImageNode>(fx).unwrap().color, full);
    }

    #[test]
    fn dialogue_owners_keep_their_current_palette_when_hud_retints() {
        let _palette = crate::interface::colours_at([80, 100, 120], [30, 60, 90]);
        let mut app = App::new();
        app.edit_schedule(Update, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
        app.insert_resource(Drawn { hud: [80, 100, 120], pipboy: [30, 60, 90] })
            .add_systems(Update, recolour);
        let old = crate::interface::tint([80, 100, 120], 127.5).with_alpha(0.7);
        let hud = app.world_mut().spawn(TextColor(old)).id();
        let owner = app.world_mut().spawn(crate::dialogue_touch::PaletteRoot).id();
        let casing = app.world_mut().spawn((ImageNode { color: old, ..default() }, ChildOf(owner))).id();
        let screen = app.world_mut().spawn((TextColor(old), ChildOf(casing))).id();
        set_hud_rgb([40, 50, 60]);
        set_pipboy_rgb([15, 35, 55]);
        app.update();
        assert_eq!(app.world().get::<TextColor>(hud).unwrap().0, retint(old, [80, 100, 120], [40, 50, 60]).unwrap());
        assert_eq!(app.world().get::<ImageNode>(casing).unwrap().color, old);
        assert_eq!(app.world().get::<TextColor>(screen).unwrap().0, old);
    }

    #[test]
    fn only_the_old_colour_at_a_brightness_is_recoloured() {
        let green = [26, 255, 128];
        let half = crate::interface::tint(AMBER_RGB, 128.0).with_alpha(0.5);
        let got = retint(half, AMBER_RGB, green).unwrap().to_srgba();
        let want = crate::interface::tint(green, 128.0).to_srgba();
        assert!((got.red - want.red).abs() < 1e-4 && (got.green - want.green).abs() < 1e-4 && (got.blue - want.blue).abs() < 1e-4, "{got:?} {want:?}");
        assert_eq!(got.alpha, 0.5, "alpha kept");
        assert!(retint(Color::BLACK, AMBER_RGB, green).is_none(), "black stays");
        assert!(retint(Color::srgb(0.8, 0.1, 0.1), AMBER_RGB, green).is_none(), "another colour stays");
        assert!(retint(Color::WHITE, AMBER_RGB, green).is_none(), "brighter than the colour: not it");
    }
}
