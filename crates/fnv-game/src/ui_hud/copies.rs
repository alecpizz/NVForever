//! Template copies beside the existing drawing [design: ui-tile-tree.md §4.1].

use super::{Shown, Texts};
use fnv_ui::{MemSource, TileId, Ui, Value};

/// Kinds supplied by the drawing, including its unused player marker [design].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    Actor,
    Hostile,
    Place,
    Found,
    Quest,
    Player,
}

/// A mark in compass-window units [design]. `heading` is the tile's left edge,
/// equal to `_Heading` [observed: hud.md §14.4–§14.5].
#[derive(Clone, Debug)]
pub struct CompassMark {
    pub kind: MarkKind,
    pub heading: f64,
    pub distance: f64,
}

/// Prompt details shared with the drawing [design]. Height 42/80 gives the two
/// captured subtitle positions; other heights follow the same rule [hypothesis:
/// hud.md §14.3]. Text widths use the game's fonts [design: hud.md §14.8].
#[derive(Clone, Debug)]
pub struct Prompt {
    pub name: String,
    pub weight: String,
    pub value: String,
    pub empty: bool,
    pub locked: bool,
    pub lock_text: String,
    pub item: bool,
    pub height: f64,
}

/// [design] Current target selected by the existing HUD adapter; selection beyond
/// health below base is unsettled [observed: private job hud_enemy3, n0-g2].
#[derive(Clone, Debug)]
pub struct Enemy {
    pub name: String,
    pub health: f64,
    pub hostile: bool,
}

/// Frame values independent of the engine [design]. Shares are bounded to 0..1;
/// defaults are full health/AP, no weapon, prompt or marks [design].
#[derive(Clone, Debug)]
pub struct Frame {
    pub health: f64,
    pub action_points: f64,
    pub condition: Option<f64>,
    pub ammo: String,
    pub radiation: f64,
    pub prompt: Option<Prompt>,
    pub marks: Vec<CompassMark>,
    pub aim_hostile: bool,
    pub enemy: Option<Enemy>,
    pub hardcore: bool,
    /// [design] Ordered need stage: absent 0, Minor 1, Advanced 2, Critical 3, Deadly 4.
    pub need_stages: [usize; 4],
    pub crippled: [bool; 6],
    pub animation: super::AnimationFrame,
    /// The HUD's size as a part of the original's (NVForever's UI scale setting, 1 the
    /// original's): the places are written for a view `960 / ui_scale` units high, each
    /// piece kept as far from its edge ([`super::write`]) [design: #1631].
    pub ui_scale: f64,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            health: 1.0,
            action_points: 1.0,
            condition: None,
            ammo: String::new(),
            radiation: 0.0,
            prompt: None,
            marks: Vec::new(),
            aim_hostile: false,
            enemy: None,
            hardcore: false,
            need_stages: [0; 4],
            crippled: [false; 6],
            animation: Default::default(),
            ui_scale: 1.0,
        }
    }
}

/// All HUD code-copy creation goes here [design]. The copies are `Origin::Code`, so
/// the tile renderer does not draw them as mod tiles.
/// Includes are already expanded in loaded templates [documented:
/// crates/fnv-ui/src/tree.rs, Ui::add_from_template / expand_includes].
fn code_copy(ui: &mut Ui, at: &str, template: &str, name: &str) -> Option<TileId> {
    let tile = ui.add_code_tile_from_template(&MemSource::default(), at, template, Some(name))?;
    // Keep separate pools even when templates are missing or share names [design].
    ui.set_trait(tile, "_NVFHudTemplate", Value::Str(template.into()));
    Some(tile)
}

fn num(ui: &mut Ui, tile: TileId, name: &str, value: f64) {
    ui.set_trait(tile, name, Value::Num(value));
}

fn string(ui: &mut Ui, tile: TileId, value: &str) {
    ui.set_trait_str(tile, "string", value);
}

fn find(ui: &Ui, path: &str) -> Option<TileId> {
    let root = ui.find_tile("HUDMainMenu")?;
    find_under(ui, root, path)
}

#[inline]
fn find_under(ui: &Ui, parent: TileId, path: &str) -> Option<TileId> {
    ui.find_child_path(parent, path).filter(|id| ui.tile(*id).get("_NVFHudTemplate").is_some())
}

/// Called once after building each HUD menu, including save-load rebuilds [design].
/// Ordering follows the captured `*:N` paths [observed: hud.md §14–§14.4].
/// Uncaptured copy names retain template names [hypothesis: hud.md §14.6];
/// template identifiers/names [documented: menus/prefabs/hudtemplates.xml].
pub fn build(ui: &mut Ui) {
    // [observed: private jobs hud_jip file_0032, hud_prompt2 p0-p8,
    // hud_enemy2 e5] Last created is *:0; same-name indices also run backwards.
    // [hypothesis] The name copy retains the template name; it was not read by name.
    for (parent, template, name) in [
        ("HitPoints", "template_left_bracket", "left_bracket"),
        ("HitPoints", "template_meter", "meter"),
        ("HitPoints", "template_justify_left_text", "justify_left_text"),
        ("HitPoints", "template_compass_window", "compass_window"),
        ("ActionPoints", "template_right_bracket", "right_bracket"),
        ("ActionPoints", "template_meter", "meter"),
        ("ActionPoints", "template_justify_right_text", "justify_right_text"),
        ("ActionPoints", "template_justify_right_text", "justify_right_text"),
        ("ActionPoints", "template_justify_right_text", "justify_right_text"),
        ("ActionPoints", "template_meter", "meter"),
        ("ActionPoints", "template_meter_background", "MeterBackground"),
        ("ReticleCenter", "template_reticle_center", "reticle_center"),
        ("Info", "template_justify_center_text", "justify_center_text"),
        ("Info", "template_justify_center_text", "justify_center_text"),
        ("Info", "template_justify_center_text", "justify_center_text"),
        ("Info", "template_justify_right_text", "justify_right_text"),
        ("Info", "template_justify_left_text", "justify_left_text"),
        ("Info", "template_justify_right_text", "justify_right_text"),
        ("Info", "template_justify_left_text", "justify_left_text"),
        ("Info", "template_info_seperator", "info_seperator"),
        ("EnemyHealth", "template_enemy_health_bracket", "enemy_health_bracket"),
        ("EnemyHealth", "template_meter", "meter"),
        ("EnemyHealth", "template_justify_center_text", "justify_center_text"),
    ] {
        code_copy(ui, &format!("HUDMainMenu/{parent}"), template, name);
    }
    let window = "HUDMainMenu/HitPoints/compass_window";
    for _ in 0..2 {
        if let Some(group) = code_copy(ui, window, "template_compass_icon_group", "compass_icon_group") {
            num(ui, group, "y", 30.0); // [observed: hud.md §14.4].
        }
    }
    if let Some(group) = code_copy(ui, window, "template_compass_npc_icon_group", "compass_npc_icon_group") {
        num(ui, group, "y", 10.0); // [observed: hud.md §14.4].
        for _ in 0..10 {
            if let Some(tick) = code_copy(ui, &ui.path_of(group), "compass_npc_icon", "compass_npc_icon") {
                num(ui, tick, "visible", 0.0); // Unused slots hidden [design].
            }
        }
    }
    // Player/quest in group 1, places in group 2 [hypothesis: hud.md §14.6].
    if let Some(player) = code_copy(ui, &format!("{window}/compass_icon_group:0"), "template_compass_icon_player", "compass_icon") {
        num(ui, player, "visible", 0.0);
    }
    // Only captured code overrides; all other traits come from loaded templates
    // [design]. Coordinates, strings and widths [observed: hud.md §14–§14.4].
    for (path, x, y) in [
        ("HitPoints/meter", 20.0, 41.0),
        ("ActionPoints/meter:0", 90.0, 83.0),
        ("ActionPoints/MeterBackground", 90.0, 83.0),
        ("ActionPoints/meter:1", 30.0, 41.0),
    ] {
        if let Some(tile) = find(ui, path) {
            num(ui, tile, "x", x);
            num(ui, tile, "y", y);
        }
    }
    if let Some(tile) = find(ui, "ActionPoints/meter:0") {
        ui.set_trait(tile, "filename", Value::Str("Interface\\VATS\\vats_bar.dds".into()));
    }
    if let Some(tile) = find(ui, "ReticleCenter/reticle_center") {
        num(ui, tile, "depth", 1.0);
    }
    for (path, text) in [
        ("HitPoints/justify_left_text", "HP"),
        ("ActionPoints/justify_right_text:0", "CND"),
        ("ActionPoints/justify_right_text:2", "AP"),
        ("Info/justify_left_text:0", "VAL"),
        ("Info/justify_right_text:0", "--"),
        ("Info/justify_left_text:1", "WG"),
        ("Info/justify_right_text:1", "--"),
        ("Info/justify_center_text:0", "[EMPTY]"),
        ("Info/justify_center_text:1", "Locked"),
    ] {
        if let Some(tile) = find(ui, path) {
            string(ui, tile, text);
        }
    }
    if let Some(tile) = find(ui, "ActionPoints/justify_right_text:1") {
        // [design] Materialize the local alpha already used by the renderer when this
        // engine-created ammo copy omits it. Hover UI consumers read this trait before
        // forwarding it to their own text; an absent trait otherwise reads -999.
        // A template's explicit alpha, including an expression or zero, wins. Later
        // script writes remain untouched because this runs only when copies are built.
        if ui.tile(tile).trait_index("alpha").is_none() {
            num(ui, tile, "alpha", 255.0);
        }
    }
    if let Some(tile) = find(ui, "HitPoints/justify_left_text") {
        // The left-text template starts hidden [documented: hudtemplates.xml];
        // showing this label through alpha/visible is a hypothesis [hypothesis: hud.md §14.6].
        num(ui, tile, "visible", 1.0);
        num(ui, tile, "alpha", 255.0);
    }
    // [observed: private jobs hud_prompt2 p0-p8, hud_enemy2 e5; hud.md §14.8].
    for (path, x, y) in [
        ("Info/justify_center_text:2", 160.0, 2.0),
        ("Info/justify_center_text:1", 160.0, 65.0),
        ("Info/justify_center_text:0", 160.0, 65.0),
        ("Info/justify_right_text:1", 135.0, 60.0),
        ("Info/justify_left_text:1", 10.0, 60.0),
        ("Info/justify_right_text:0", 290.0, 60.0),
        ("Info/justify_left_text:0", 165.0, 60.0),
        ("Info/info_seperator", 0.0, 32.0),
        ("EnemyHealth/enemy_health_bracket", -7.0, 3.0),
        ("EnemyHealth/meter", 167.0, 3.0),
        ("EnemyHealth/justify_center_text", 170.0, 35.0),
    ] {
        if let Some(tile) = find(ui, path) {
            num(ui, tile, "x", x);
            num(ui, tile, "y", y);
            num(ui, tile, "alpha", 255.0);
            num(ui, tile, "visible", 0.0);
        }
    }
    for (path, w, h, brightness) in [
        ("Info/info_seperator", 500.0, 136.0, 255.0),
        ("EnemyHealth/enemy_health_bracket", 500.0, 64.0, 175.0),
        ("EnemyHealth/meter", 8.0, 20.0, 175.0),
    ] {
        if let Some(tile) = find(ui, path) {
            num(ui, tile, "width", w);
            num(ui, tile, "height", h);
            num(ui, tile, "brightness", brightness);
            if path.starts_with("EnemyHealth/") {
                num(ui, tile, "depth", 1.0);
            }
        }
    }
}

fn share(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub(super) fn write_under(ui: &mut Ui, root: TileId, shown: Shown, texts: &Texts) {
    let input = &texts.frame;
    // Whole ticks, AP right anchored; condition nearest unit [observed: hud.md §14.3].
    for (path, value, right) in [("HitPoints/meter", input.health, false), ("ActionPoints/meter:1", input.action_points, true)] {
        if let Some(tile) = find_under(ui, root, path) {
            let width = (share(value) * 37.0).floor() * 8.0;
            num(ui, tile, "width", width);
            num(ui, tile, "x", if right { 30.0 + 296.0 - width } else { 20.0 });
        }
    }
    if let Some(tile) = find_under(ui, root, "ActionPoints/meter:0") {
        num(ui, tile, "width", (59.0 * share(input.condition.unwrap_or(0.0))).round());
    }
    if let Some(tile) = find_under(ui, root, "ActionPoints/justify_right_text:1") {
        string(ui, tile, &input.ammo);
    }
    if let Some(tile) = find_under(ui, root, "ReticleCenter/reticle_center") {
        // Keep user0/user1 as the template's strings: their numeric reads are 0
        // [observed: hud.md §14.4]. Select the loaded alternate [design].
        num(ui, tile, "systemcolor", if input.aim_hostile && !shown.aiming { 2.0 } else { 1.0 });
        let picture = ui.tile(tile).get(if shown.info && !input.aim_hostile { "user1" } else { "user0" }).cloned();
        if let Some(picture) = picture {
            ui.set_trait(tile, "filename", picture);
        }
    }
    write_prompt(ui, root, shown, input);
    if shown.radiation {
        if let Some(tile) = ui.find_child_path(root, "RadiationMeter/radiation_pointer") {
            num(ui, tile, "x", radiation_pointer_x(input.radiation));
        }
    }
    write_enemy(ui, root, shown, input);
    write_needs(ui, root, input);
    write_animations(ui, root, shown, input);
    write_marks(ui, root, &input.marks);
}

/// [observed: private job hud_radiation2 r_in_000-r_in_990; hypothesis: the rule
/// between 75 paired samples over 2.25..998.4 rads, x residuals within 0.7].
/// [hypothesis] Hold 315 above 1000 (Fatal, not captured). Only written while shown.
pub(super) fn radiation_pointer_x(rads: f64) -> f64 {
    let rads = if rads.is_finite() { rads.clamp(0.0, 1000.0) } else { 0.0 };
    15.0 + 0.3 * rads
}

fn write_prompt(ui: &mut Ui, root: TileId, shown: Shown, input: &Frame) {
    let prompt = input.prompt.as_ref().filter(|_| shown.info);
    let on = prompt.is_some();
    if let Some(info) = ui.find_child_path(root, "Info") {
        if let Some(prompt) = prompt {
            for (path, text) in [
                ("justify_right_text:0", prompt.value.as_str()),
                ("justify_right_text:1", prompt.weight.as_str()),
                ("justify_center_text:2", prompt.name.as_str()),
                ("justify_center_text:1", prompt.lock_text.as_str()),
            ] {
                if let Some(tile) = find_under(ui, info, path) {
                    string(ui, tile, text);
                }
            }
            if let Some(tile) = find_under(ui, info, "justify_center_text:2") {
                num(ui, tile, "height", prompt.height);
            }
        }
        // [observed: private job hud_prompt2 p2-p8] Hidden strings and alphas persist.
        for path in ["info_seperator", "justify_left_text:0", "justify_right_text:0", "justify_left_text:1", "justify_right_text:1", "justify_center_text:0", "justify_center_text:1", "justify_center_text:2"] {
            if let Some(tile) = find_under(ui, info, path) {
                let visible = match path {
                    "justify_center_text:2" | "justify_center_text:1" => on,
                    "justify_center_text:0" => prompt.is_some_and(|p| !p.locked),
                    _ => prompt.is_some_and(|p| p.item),
                };
                num(ui, tile, "visible", f64::from(visible));
                if let Some(p) = prompt {
                    let alpha = match path {
                        "justify_center_text:0" => p.empty && !p.locked,
                        "justify_center_text:1" => p.locked,
                        _ => true,
                    };
                    num(ui, tile, "alpha", if alpha { 255.0 } else { 0.0 });
                }
            }
        }
        if let Some(tile) = ui.find_child_path(info, "justify_center_hotrect") {
            num(ui, tile, "visible", if on { 2.0 } else { 0.0 });
            if on {
                if let Some(label) = ui.find_child_path(tile, "PCShortcutLabel") {
                    num(ui, label, "visible", 1.0);
                }
            }
        }
    }
    if let Some(tile) = ui.find_child_path(root, "Subtitles") {
        // [observed: private job hud_prompt2 p8_gone] Raised y persists for 19.9 s.
        // [hypothesis] A later prompt height replaces the previous offset.
        let y = prompt.map_or_else(|| ui.tile(tile).get("_NVFSubtitleY").map_or(660.0, Value::num), |p| 660.0 - (p.height.max(0.0) + 8.0));
        num(ui, tile, "_NVFSubtitleY", y);
        // Kept to the view's middle as the prompt it rises over (`Info`) at a UI scale
        // other than 1 (`super::write`), as the hand-built subtitles [design: #1631].
        let scale = if input.ui_scale.is_finite() { input.ui_scale.max(0.1) } else { 1.0 };
        num(ui, tile, "y", y + 0.5 * (960.0 / scale - 960.0));
    }
}

fn write_enemy(ui: &mut Ui, root: TileId, shown: Shown, input: &Frame) {
    let Some(enemy_root) = ui.find_child_path(root, "EnemyHealth") else { return };
    let enemy = input.enemy.as_ref();
    // [design, #2659] Shown owns visibility: classic VATS also shows full-health targets
    // (engine/nvf-play/src/tiles/mod.rs::HudTiles::shown).
    let visible = shown.enemy && enemy.is_some();
    if let Some(enemy) = enemy {
        num(ui, enemy_root, "systemcolor", if enemy.hostile { 2.0 } else { 1.0 });
        if let Some(tile) = find_under(ui, enemy_root, "meter") {
            // [observed: private jobs hud_enemy2 e5, hud_enemy3 n2, h1, g2;
            // hypothesis: rule between shares 0.156, 0.178, 0.5, 1.0].
            let width = 16.0 * (share(enemy.health) * 15.0).floor();
            num(ui, tile, "width", width);
            num(ui, tile, "x", 171.0 - width / 2.0);
        }
        if let Some(tile) = find_under(ui, enemy_root, "justify_center_text") {
            string(ui, tile, &enemy.name);
        }
    }
    for name in ["enemy_health_bracket", "meter", "justify_center_text"] {
        if let Some(tile) = find_under(ui, enemy_root, name) {
            num(ui, tile, "visible", f64::from(visible));
        }
    }
}

/// The limb word's alpha and system colour for `crippled` limbs: none hidden, one faded
/// (124, the HUD's colour) [observed: private jobs hud_hardcore2 hc9-hc10, hud_xp_limbs
/// l1], two full (255) [observed: hud_xp_limbs l2; hypothesis: the HUD's colour, not
/// read], three or more the alternative red at full alpha [observed: hud_compare
/// needs_limbs and needs_off at 1280 x 720, the original's LMB (255, 70, 44) in both shots
/// with an arm and both legs crippled; hypothesis: steady, not flashing, and four or more
/// as three]. Until T12e three or more kept the HUD's colour, drawn amber [hypothesis].
fn limb_word(crippled: usize) -> (f64, f64) {
    match crippled {
        0 => (0.0, 1.0),
        1 => (124.0, 1.0),
        2 => (255.0, 1.0),
        _ => (255.0, 2.0),
    }
}

fn write_needs(ui: &mut Ui, root: TileId, input: &Frame) {
    let Some(hc) = ui.find_child_path(root, "HardcoreMode") else { return };
    // [observed: private job hud_hardcore2 hc0-hc11] Fixed places in every mode.
    for (i, (name, x)) in [("Dehydration", 0.0), ("Hunger", 55.0), ("Sleep", 109.0), ("Rads", 162.0), ("LMBs", 218.0)].into_iter().enumerate() {
        let Some(tile) = ui.find_child_path(hc, name) else { continue };
        let (alpha, colour) = if i == 4 {
            limb_word(input.crippled.iter().filter(|c| **c).count())
        } else if i < 3 && !input.hardcore {
            (0.0, 1.0)
        } else {
            match input.need_stages[i] {
                0 => (0.0, 1.0),
                1 => (124.0, 1.0),
                2 => (255.0, 1.0),
                3 => (255.0, 2.0),
                // [design] 0.6 s cosine until a period is measured; Fatal as Deadly
                // [hypothesis]. Observed alpha bounds 128..255, systemcolor 2.
                _ => (128.0 + 127.0 * (0.5 + 0.5 * (input.animation.seconds / 0.6 * std::f64::consts::TAU).cos()), 2.0),
            }
        };
        num(ui, tile, "x", x);
        num(ui, tile, "y", 0.0);
        num(ui, tile, "zoom", 75.0);
        num(ui, tile, "visible", 1.0);
        num(ui, tile, "alpha", alpha);
        num(ui, tile, "systemcolor", colour);
    }
}

fn write_animations(ui: &mut Ui, root: TileId, shown: Shown, input: &Frame) {
    use super::timelines::{level_alpha, limb_alpha, xp_alpha};
    let xp = input.animation.xp.as_ref();
    if let Some(xp_root) = ui.find_child_path(root, "XPMeter") {
        for name in ["XPBracket", "XPPointer", "XPAmount", "XPLabel", "XPLastLevel", "XPNextLevel", "XPLevelUp"] {
            let Some(tile) = ui.find_child_path(xp_root, name) else { continue };
            let alpha = xp.map_or(if name == "XPLevelUp" { 255.0 } else { 0.0 }, |s| if name == "XPLevelUp" { level_alpha(s.age) } else { xp_alpha(s.age) });
            let visible = shown.corners && xp.is_some_and(|s| if name == "XPLevelUp" { s.level_up && s.age >= 7.2 } else { s.age < 6.0 });
            num(ui, tile, "alpha", alpha);
            num(ui, tile, "visible", f64::from(visible));
            if let Some(s) = xp {
                match name {
                    "XPAmount" => string(ui, tile, &format!("+{:.0}", s.amount)),
                    "XPLastLevel" => string(ui, tile, &s.level.to_string()),
                    "XPNextLevel" => string(ui, tile, &(s.level + 1).to_string()),
                    "XPPointer" => {
                        // [observed: private jobs hud_xp_limbs x1-x2b, hud_xp_limbs2 x9;
                        // hypothesis: interpolation, level 1 XP 10..200 only].
                        let t = ((s.age - 2.0) / 2.0).clamp(0.0, 1.0);
                        num(ui, tile, "x", 55.0 + 215.0 * share(s.from + (s.to - s.from) * t));
                    }
                    _ => {}
                }
            }
        }
    }
    let Some(limb_root) = ui.find_child_path(root, "crippled_limb_indicator") else { return };
    // [observed: private job hud_xp_limbs l1-l3] One run only; zoom and places.
    for (name, limb, x, y, w, h, tex, tex_broken) in [
        ("Head", Some(0), 0.0, 40.0, 63.75, 69.0, "Interface\\Stats\\head.dds", "Interface\\Stats\\head_broken.dds"),
        ("Face", None, 13.88, 62.5, 36.0, 48.0, "", ""),
        ("Torso", Some(1), -10.5, 104.5, 76.5, 96.0, "Interface\\Stats\\torso.dds", "Interface\\Stats\\torso_broken.dds"),
        ("Left_Arm", Some(2), 57.0, 104.5, 75.0, 39.0, "Interface\\Stats\\left_arm.dds", "Interface\\Stats\\left_arm_broken.dds"),
        ("Right_Arm", Some(3), -76.5, 100.75, 72.0, 40.5, "Interface\\Stats\\right_arm.dds", "Interface\\Stats\\right_arm_broken.dds"),
        ("Left_Leg", Some(4), 24.75, 169.0, 54.0, 84.0, "Interface\\Stats\\left_leg.dds", "Interface\\Stats\\left_leg_broken.dds"),
        ("Right_Leg", Some(5), -38.25, 166.75, 63.0, 84.0, "Interface\\Stats\\right_leg.dds", "Interface\\Stats\\right_leg_broken.dds"),
    ] {
        let Some(tile) = ui.find_child_path(limb_root, name) else { continue };
        let broken = limb.is_some_and(|i| input.animation.broken[i]);
        for (key, value) in [("x", x), ("y", y), ("width", w), ("height", h), ("zoom", 75.0), ("visible", 1.0)] {
            num(ui, tile, key, value);
        }
        let alpha = input.animation.limb_age.filter(|_| shown.corners).map_or(if broken { -16.0 } else { 0.0 }, |age| limb_alpha(age, broken));
        num(ui, tile, "alpha", alpha);
        if limb.is_some() {
            num(ui, tile, "systemcolor", if broken { 2.0 } else { 1.0 });
            ui.set_trait_str(tile, "filename", if broken { tex_broken } else { tex });
        }
    }
}

fn write_marks(ui: &mut Ui, root: TileId, marks: &[CompassMark]) {
    let Some(window) = ui.find_child_path(root, "HitPoints/compass_window") else { return };
    // Reuse slots, hiding the surplus [design]. Ten NPC slots were captured;
    // overflow slots and the other pools grow only as needed [hypothesis: hud.md §14.6].
    let mut slots = Vec::with_capacity(16);
    for (group, template, name, kinds) in [
        ("compass_npc_icon_group", "compass_npc_icon", "compass_npc_icon", &[MarkKind::Actor, MarkKind::Hostile][..]),
        ("compass_icon_group:0", "template_compass_icon_player", "compass_icon", &[MarkKind::Player][..]),
        ("compass_icon_group:0", "template_compass_icon_quest", "compass_icon", &[MarkKind::Quest][..]),
        (
            "compass_icon_group:1",
            "template_compass_icon_marker",
            "compass_icon",
            &[MarkKind::Place, MarkKind::Found][..],
        ),
    ] {
        let Some(parent) = find_under(ui, window, group) else {
            continue;
        };
        slots.clear();
        slots.extend(
            ui.tile(parent)
                .children
                .iter()
                .rev()
                .copied()
                .filter(|id| ui.tile(*id).get("_NVFHudTemplate").is_some_and(|v| v.text() == template)),
        );
        let incoming_count = marks.iter().filter(|m| kinds.contains(&m.kind)).count();
        if slots.len() < incoming_count {
            let path = format!("HUDMainMenu/HitPoints/compass_window/{group}");
            let mut grew = false;
            while slots.len() < incoming_count {
                let Some(tile) = code_copy(ui, &path, template, name) else {
                    break;
                };
                slots.push(tile);
                grew = true;
            }
            if grew {
                slots.clear();
                slots.extend(
                    ui.tile(parent)
                        .children
                        .iter()
                        .rev()
                        .copied()
                        .filter(|id| ui.tile(*id).get("_NVFHudTemplate").is_some_and(|v| v.text() == template)),
                );
            }
        }
        let mut incoming = marks.iter().filter(|m| kinds.contains(&m.kind));
        for &tile in &slots {
            let Some(mark) = incoming.next() else {
                num(ui, tile, "visible", 0.0);
                continue;
            };
            // x == _Heading, tick distance 1 [observed: hud.md §14.4].
            if matches!(mark.kind, MarkKind::Actor | MarkKind::Hostile) {
                // [observed: private jobs hud_enemy2 g0-g2, hud_enemy3 h1]
                num(ui, tile, "systemcolor", if mark.kind == MarkKind::Hostile { 2.0 } else { 1.0 });
            }
            num(ui, tile, "_Heading", mark.heading);
            num(ui, tile, "x", mark.heading);
            num(
                ui,
                tile,
                "_Distance",
                if matches!(mark.kind, MarkKind::Actor | MarkKind::Hostile) { 1.0 } else { mark.distance },
            );
            // 139 either side of x 139; quest/player band [hypothesis: hud.md §14.5].
            num(ui, tile, "visible", f64::from(mark.heading.is_finite() && (0.0..=278.0).contains(&mark.heading)));
        }
    }
}
