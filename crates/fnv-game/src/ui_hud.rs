//! HUD places, visibility and texts beside the current drawing [design: ui-tile-tree.md §6.2].

use crate::game::Game;
use fnv_ui::{Ui, Value};

mod copies;
mod timelines;
pub use copies::{build, CompassMark, Enemy, MarkKind, Frame, Prompt};
pub use timelines::{Animation, AnimationFrame, XpFrame};

/// What the engine's HUD currently shows; no drawing or engine types here [design].
/// The pieces' `visible` does not follow these; the children it is observed on do
/// (`write`), the rest wait for their captures [observed: hud.md §14.1].
#[derive(Clone, Copy, Debug, Default)]
pub struct Shown {
    pub root: bool,
    /// Block held with a gun, the view zooming to its Sight FOV.
    pub aiming: bool,
    pub corners: bool,
    pub sneak: bool,
    pub reticle: bool,
    pub enemy: bool,
    pub quests: bool,
    pub messages: bool,
    pub subtitles: bool,
    pub info: bool,
    pub location: bool,
    pub xp: bool,
    pub radiation: bool,
    pub breath: bool,
    pub limbs: bool,
    pub needs: bool,
    pub ammo_label: bool,
    pub condition: bool,
}

/// Texts from the current drawing, without engine types [design: ui-tile-tree.md §6.2].
/// Optional texts keep their last value when absent [observed: hud.md §14.2].
#[derive(Clone, Debug)]
pub struct Texts {
    pub radiation_word: String,
    pub radiation_value: String,
    pub xp_label: String,
    pub xp_level_up: String,
    /// Dehydration, hunger, sleep, radiation and limbs, in file order [documented:
    /// hud_main_menu.xml, HardcoreMode; hud.md §13].
    pub needs: [String; 5],
    pub sneak: Option<String>,
    pub prompt_key: Option<String>,
    pub prompt_verb: Option<String>,
    /// Code-made tiles' changing inputs; defaults need no engine types [design].
    pub frame: Frame,
}

/// The need words' game settings and the words used without them, in [`Texts::needs`]
/// order [documented: hud_main_menu.xml, HardcoreMode; observed: hud.md §14.2, H20 ends in
/// a zero] (the engine's `hud_needs` draws the same).
pub const NEED_WORDS: [(&str, &str); 5] = [("sStatsH20Abbrev", "H20"), ("sStatsFODAbbrev", "FOD"), ("sStatsSLPAbbrev", "SLP"), ("sStatsRADAbbrev", "RAD"), ("sStatsLMBAbbrev", "LMB")];

/// `HitPoints`' place in interface units, from the view's top left [observed: hud.md §14,
/// the pieces' place table].
pub const HIT_POINTS_AT: (f64, f64) = (40.0, 803.0);
/// `HardcoreMode`'s place, the need words' piece [observed: hud.md §14, the pieces' place
/// table; private job hud_hardcore2, hc0-hc11, the same in every mode].
pub const HARDCORE_MODE_AT: (f64, f64) = (190.0, 803.0);

/// Called after menu lifetime sync, before scripts [design: ui-tile-tree.md §4.1].
pub fn write(ui: &mut Ui, shown: Shown, texts: &Texts) {
    let Some(root) = ui.find_tile("HUDMainMenu") else { return };
    // A script's SetUIFloat lasts until the next frame's write [hypothesis:
    // menu-xml.md §6, ui-tile-tree.md §4.2; original overwrite frequency unobserved].
    // Root shown 1 [observed: hud.md §14], hidden 0 [hypothesis].
    ui.set_trait(root, "visible", Value::Num(f64::from(shown.root)));
    // Leave root code values alone pending hud.md §14 "Open" captures [design].
    let screen = ui.tile(ui.screen());
    let size = screen.get("width").zip(screen.get("height")).map(|(w, h)| (w.num(), h.num()));
    if let Some((width, height)) = size.filter(|(w, h)| w.is_finite() && h.is_finite() && *w > 0.0 && *h > 0.0) {
        // The view is 960 units high [documented: hud.md §2/§14]; at a UI scale other
        // than 1 it is 960 / scale units high, the pieces as far from their edges [design:
        // #1631]. The scale is the run's (`Frame::ui_scale`), at least a tenth.
        let scale = if texts.frame.ui_scale.is_finite() { texts.frame.ui_scale.max(0.1) } else { 1.0 };
        let view = 960.0 / scale;
        let w = view * width / height;
        // How much further down a piece kept to the view's middle (0.5) or bottom (1)
        // stands: nothing at 1 [design].
        let down = |anchor: f64| anchor * (view - 960.0);
        // Truncate W and W/2 before offsets [hypothesis: hud.md §14: at 16:9,
        // W - 386 = 1320.67 reads 1320; truncating the middle before the half-unit
        // offset also reproduces the child's 820.5. Nearest would give 1321].
        let (right, middle) = (w.floor(), (w / 2.0).floor());
        // Every place below [observed: hud.md §14, every piece's place table]. The edge
        // each keeps to as the scale changes is the hand-built HUD's (0 top, 0.5 middle,
        // 1 bottom) [design: #1631].
        for (name, x, y, anchor) in [
            ("HitPoints", HIT_POINTS_AT.0, HIT_POINTS_AT.1, 1.0),
            ("ActionPoints", right - 386.0, 803.0, 1.0),
            ("HardcoreMode", HARDCORE_MODE_AT.0, HARDCORE_MODE_AT.1, 1.0),
            ("SneakMeter", middle, 30.0, 0.0),
            ("ReticleCenter", middle - 34.5, 439.5, 0.5),
            ("EnemyHealth", middle - 175.0, 843.0, 1.0),
            ("QuestReminder", 45.0, 200.0, 0.0),
            ("Messages", 25.0, 55.0, 0.0),
            ("Info", middle - 160.0, 820.0, 0.5),
            ("Hokeys", right - 371.0, 483.0, 0.5),
            ("Region_Location", 45.0, 728.0, 1.0),
            ("XPMeter", right - 420.0, 650.0, 1.0),
            ("RadiationMeter", right - 417.0, 30.0, 0.0),
            ("BreathMeter", middle, 110.0, 0.0),
            ("Explosive_positioning_rect", middle, 480.0, 0.5),
            ("crippled_limb_indicator", right - 230.0, 130.0, 0.0),
            ("DDTIcon", 414.0, 852.0, 1.0),
            ("DDTIconEnemy", middle - 207.0, 846.0, 1.0),
            ("DDTIconEnemyAP", middle - 207.0, 846.0, 1.0),
            ("AmmoTypeLabel", right - 236.0, 883.0, 1.0),
            ("CNDArrows", right - 253.0, 886.0, 1.0),
        ] {
            if let Some(tile) = ui.find_child_path(root, name) {
                ui.set_trait(tile, "x", Value::Num(x));
                ui.set_trait(tile, "y", Value::Num(y + down(anchor)));
            }
        }
        if let Some(tile) = ui.find_child_path(root, "Subtitles") {
            ui.set_trait(tile, "x", Value::Num(middle - 400.0));
        }
    }
    if let Some(tile) = ui.find_child_path(root, "CNDArrows") {
        ui.set_trait(tile, "height", Value::Num(20.0)); // [observed: hud.md §14].
    }
    // A piece's visible keeps its value whether or not the piece shows: SneakMeter,
    // Info and RadiationMeter kept theirs out of sneaking, with no prompt and out of
    // radiation, as BreathMeter, XPMeter and the limb indicator when not shown. Only
    // aiming changed one: ReticleCenter 0, EnemyHealth 8 [observed: hud.md §14.1].
    // The corners, AmmoTypeLabel and CNDArrows were seen shown only [hypothesis: the
    // same]. With the root hidden they read 0 [observed: hud.md §14.3]. Pieces we do
    // not draw keep the file's visible [design]. The corner pieces (the two corners,
    // the radiation meter, the ammo label and the condition arrows) also read 0 while
    // `shown.corners` is off (movement lock, portrait, held camera, weapon wheel), as
    // the hand-built HUD hides them then (#980) [design; not captured in the original].
    let reticle = if shown.reticle && !shown.aiming { 64.0 } else { 0.0 };
    let enemy = if shown.aiming { 8.0 } else { 1.0 };
    for (name, value, corner) in [
        ("ActionPoints", 1.0, true),
        ("HitPoints", 2.0, true),
        ("RadiationMeter", 4.0, true),
        ("QuestReminder", 16.0, false),
        ("Region_Location", 32.0, false),
        ("ReticleCenter", reticle, false),
        ("SneakMeter", 128.0, false),
        ("Messages", 256.0, false),
        ("Info", 512.0, false),
        ("Subtitles", 1024.0, false),
        ("XPMeter", 4096.0, false),
        ("BreathMeter", 8192.0, false),
        ("crippled_limb_indicator", 32768.0, false),
        ("HardcoreMode", 65536.0, false),
        ("EnemyHealth", enemy, false),
        ("AmmoTypeLabel", 1.0, true),
        ("CNDArrows", 1.0, true),
    ] {
        if let Some(tile) = ui.find_child_path(root, name) {
            let on = shown.root && (!corner || shown.corners);
            ui.set_trait(tile, "visible", Value::Num(if on { value } else { 0.0 }));
        }
    }
    // What shows is the children's alpha: the sneak word while sneaking, the radiation
    // meter's four pieces in radiation, 255 shown and 0 not [observed: hud.md §14.1].
    for (path, on) in [
        ("SneakMeter/sneak_nif", shown.sneak),
        ("RadiationMeter/radiation_text", shown.radiation),
        ("RadiationMeter/radiation_text_value", shown.radiation),
        ("RadiationMeter/radiation_pointer", shown.radiation),
        ("RadiationMeter/radiation_bracket", shown.radiation),
    ] {
        if let Some(tile) = ui.find_child_path(root, path) {
            ui.set_trait(tile, "alpha", Value::Num(if on { 255.0 } else { 0.0 }));
        }
    }
    // Every path and string below [observed: hud.md §14.2]. Radiation and XP words
    // and the needs are written even when hidden; absent sneak/prompt texts keep
    // their last value, as observed after standing or losing the target.
    for (path, value) in [
        ("RadiationMeter/radiation_text", Some(texts.radiation_word.as_str())),
        ("RadiationMeter/radiation_text_value", Some(texts.radiation_value.as_str())),
        ("XPMeter/XPLabel", Some(texts.xp_label.as_str())),
        ("XPMeter/XPLevelUp", Some(texts.xp_level_up.as_str())),
        ("SneakMeter/sneak_nif", texts.sneak.as_deref()),
        ("Info/justify_center_hotrect/PCShortcutLabel", texts.prompt_key.as_deref()),
        ("Info/justify_center_hotrect", texts.prompt_verb.as_deref()),
        ("Info/justify_center_hotrect/button_text", texts.prompt_verb.as_deref()),
    ] {
        if let Some(value) = value {
            if let Some(tile) = ui.find_child_path(root, path) {
                ui.set_trait_str(tile, "string", value);
            }
        }
    }
    // The need words are written when their text changes, not every frame: a script's
    // SetUIString on one lasts until the HUD's own word changes [observed: the jip-probes
    // set gaps1, "NVF one" then "NVF 2" read back in later steps, "H20" only after the
    // probe wrote it; hypothesis: written on change, the stage changes not yet probed].
    if let Some(hc) = ui.find_child_path(root, "HardcoreMode") {
        for (name, word) in ["Dehydration", "Hunger", "Sleep", "Rads", "LMBs"].iter().zip(&texts.needs) {
            if let Some(tile) = ui.find_child_path(hc, name) {
                ui.write_changed(tile, "string", Value::Str(word.clone()));
            }
        }
    }
    copies::write_under(ui, root, shown, texts);
}

/// Measures a text: font number (0 when absent) and string to its width in interface
/// units, as [`write_text_widths`] takes it. The engine's fonts behind it, owned, so the
/// game holds no engine types [design].
pub type Measure = std::sync::Arc<dyn Fn(f64, &str) -> f64 + Send + Sync>;

/// A frame's HUD writes kept back by [`defer`]: what [`write`] and [`write_text_widths`]
/// would have made.
pub struct Pending {
    pub shown: Shown,
    pub texts: Texts,
    /// The text widths' measure, `None` when the frame would have written no widths (no
    /// game fonts) [design].
    pub widths: Option<Measure>,
}

/// Keeps this frame's writes for [`flush`] instead of making them, for a frame where
/// nothing reads the HUD's traits (no tile-drawn HUD, no mod tiles, no script has called
/// a UI function on the HUD): such a frame does no tree work [design: maintainer's review
/// of #902]. The widths are kept as their measure: they are measured from the strings
/// [`write`] puts in the tree, which are not there yet.
pub fn defer(game: &mut Game, shown: Shown, texts: Texts, widths: Option<Measure>) {
    game.ui_hud_pending = Some(Box::new(Pending { shown, texts, widths }));
}

/// Makes the writes [`defer`] kept, before a UI function reads or changes the HUD, in a
/// frame's order (the strings, then the widths measured from them), so a script reads
/// this frame's values as it would had they been written [design].
pub fn flush(game: &mut Game) {
    if let Some(pending) = game.ui_hud_pending.take() {
        let Pending { shown, texts, widths } = *pending;
        write(&mut game.ui_tree, shown, &texts);
        if let Some(measure) = widths {
            write_text_widths(&mut game.ui_tree, &*measure);
        }
    }
}

/// The HUD's file text tiles whose `width` the original's code sets to the drawn text's
/// width [observed: hud.md §14.1: `sneak_nif` 106 with "[HIDDEN]", then 136; the prompt's
/// `button_text` 156 wide with the file's "Button Text" and 54 with "Talk" (its hotrect,
/// whose width the file works out from this child's, 176 and 74); `radiation_text_value`
/// 50 wide, then 49 ("+0", "+9", §14.2)].
pub const MEASURED_TEXTS: [&str; 3] = ["SneakMeter/sneak_nif", "Info/justify_center_hotrect/button_text", "RadiationMeter/radiation_text_value"];

/// [design] File texts with captured measurements plus the code-made prompt/enemy
/// texts and XP readouts (hud.md §14.8). Only engine-owned copies are added here.
pub fn measured_texts(ui: &Ui) -> Vec<fnv_ui::TileId> {
    let mut tiles: Vec<_> = MEASURED_TEXTS.iter().filter_map(|p| ui.find_tile(&format!("HUDMainMenu/{p}"))).collect();
    for piece in ["Info", "EnemyHealth", "XPMeter"] {
        if let Some(root) = ui.find_tile(&format!("HUDMainMenu/{piece}")) {
            tiles.extend(ui.tile(root).children.iter().copied().filter(|id| {
                let tile = ui.tile(*id);
                tile.kind == "text" && (piece == "XPMeter" || tile.get("_NVFHudTemplate").is_some())
            }));
        }
    }
    tiles
}

/// A text tile whose `width` the engine measured for a mod's HUD text
/// ([`unmeasured_texts`]); its width is measured again when its string changes.
pub const MEASURED_MARK: &str = "_NVFMeasured";

/// The HUD's text tiles that mods add and that read their own size: every `text` under
/// HUDMainMenu not in [`measured_texts`] whose file gives no `width` of its own (or whose
/// width the engine wrote before, [`MEASURED_MARK`]). The original measures every text
/// tile [hypothesis, menu-xml.md §3.1.2]; the vanilla pieces keep their own path (#4493).
pub fn unmeasured_texts(ui: &Ui) -> Vec<fnv_ui::TileId> {
    let Some(root) = ui.find_tile("HUDMainMenu") else { return Vec::new() };
    let known = measured_texts(ui);
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let tile = ui.tile(id);
        stack.extend(tile.children.iter().copied());
        if tile.kind != "text" || known.contains(&id) {
            continue;
        }
        if tile.get("width").is_none() || tile.get(MEASURED_MARK).is_some() {
            out.push(id);
        }
    }
    out
}

/// [design] Write measured text widths after [`write`], before scripts. `measure`
/// takes font number (0 when absent) and string, returning interface units. A missing
/// string keeps its old width [observed: hud.md §14.1, before the first sneak].
pub fn write_text_widths(ui: &mut Ui, measure: &dyn Fn(f64, &str) -> f64) {
    for tile in measured_texts(ui) {
        let t = ui.tile(tile);
        let Some(string) = t.get("string").map(Value::text) else { continue };
        let font = t.get("font").map_or(0.0, Value::num);
        ui.set_trait(tile, "width", Value::Num(measure(font, &string)));
    }
}
