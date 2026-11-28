//! What the STATS page shows, as plain data ([`Snapshot`]), and the rules that turn the
//! game's numbers into the page's words (docs/spec-notes/pipboy.md §5.1). Nothing here
//! reads the game; `read.rs` fills a snapshot.

use fnv_sim::FormId;

/// The actor values of the six body parts the Vault Boy shows, in the order head, torso,
/// left arm, right arm, left leg, right leg: Perception, Endurance, Left Attack, Right
/// Attack, Left Mobility and Right Mobility Condition [documented: xEdit `ActorValue`
/// 25–30; the AVIF records name them Head, Chest, Left Arm, Right Arm, Left Leg,
/// RightLeg (observed)].
pub const LIMBS: [u16; 6] = [25, 26, 27, 28, 29, 30];


/// The experience a level needs: (n − 1) × ((n − 2) / 2 × `iXPBumpBase` + `iXPBase`)
/// [documented: GECK wiki Settings, "Experience required to level up"; defaults 200 and
/// 150].
pub fn xp_for_level(level: f64, base: f64, bump: f64) -> f64 {
    (level - 1.0) * ((level - 2.0) / 2.0 * bump + base)
}

/// "(+)" or "(-)" after a value raised or lowered from its permanent value (base and
/// permanent modifier) [observed: "34 (+)" for the owner's Melee Weapons with the Vault
/// Jumpsuit's +2 worn; hypothesis: a lowered value shows "(-)"].
pub fn modified_mark(shown: f64, permanent: f64) -> Option<&'static str> {
    if shown.round() > permanent.round() {
        Some("(+)")
    } else if shown.round() < permanent.round() {
        Some("(-)")
    } else {
        None
    }
}

/// The body parts of [`LIMBS`], for the log and the test aid.
pub const LIMB_NAMES: [&str; 6] = ["head", "torso", "left arm", "right arm", "left leg", "right leg"];

/// The CND page's hints that use an item: the Stimpak (0) and the Doctor's Bag (1).
pub const STIMPAK: usize = 0;
pub const DOCTORS_BAG: usize = 1;
/// The RAD page's RadAway hint [design: stats::use_hints].
pub const RADAWAY: usize = 2;

/// Why the CND page does not use a hint's item, or `None` when it does [design: owner's
/// request 2026-10-23, #3431]: none carried; a Stimpak on the body at full health, or on a limb
/// that is whole; a Stimpak on a limb in Hardcore mode, where it mends no limb
/// [observed: FalloutNV.esm's help text, "In Hardcore Mode, a Doctor's Bag or an actual
/// doctor is required"]; a Doctor's Bag on a limb that is whole, or on the body when no limb
/// is hurt. `hp` is current and whole, `limbs` each part's condition from 0 to 1 (as
/// [`Snapshot::limbs`]).
pub fn refusal(hint: usize, limb: Option<usize>, count: i32, hp: (f64, f64), limbs: &[f64; 6], hardcore: bool) -> Option<String> {
    refusal_with(hint, limb, count, hp, limbs, hardcore, false)
}

/// [`refusal`], with the player's Aid at Full Health tweak: nothing hurt is no reason
/// [design: docs/design/tweaks.md].
pub fn refusal_with(hint: usize, limb: Option<usize>, count: i32, hp: (f64, f64), limbs: &[f64; 6], hardcore: bool, at_full: bool) -> Option<String> {
    const WHOLE: f64 = 1.0 - 1e-6;
    if count <= 0 {
        return Some("none carried".into());
    }
    match (hint, limb) {
        (STIMPAK, Some(_)) if hardcore => Some("a Stimpak mends no limb in Hardcore mode".into()),
        (STIMPAK, Some(i)) if !at_full && limbs.get(i).is_none_or(|&c| c >= WHOLE) => Some(format!("the {} is not hurt", LIMB_NAMES.get(i).unwrap_or(&"limb"))),
        (STIMPAK, None) if !at_full && hp.0 >= hp.1 - 1e-6 => Some("health is full".into()),
        (DOCTORS_BAG, Some(i)) if !at_full && limbs.get(i).is_none_or(|&c| c >= WHOLE) => Some(format!("the {} is not hurt", LIMB_NAMES.get(i).unwrap_or(&"limb"))),
        (DOCTORS_BAG, None) if !at_full && limbs.iter().all(|&c| c >= WHOLE) => Some("no limb is hurt".into()),
        (STIMPAK | DOCTORS_BAG, _) => None,
        _ => Some("not used from the STATS page".into()),
    }
}

/// A hint at the status page's top right: "(9) Stimpak", or just the name when the player
/// has none [observed: "(9) Stimpak", "(3) Doctor's Bag", and "RadAway", "Rad-X" without
/// a count on the owner's screenshots; hypothesis: the count is left out at 0].
pub fn hint_text(name: &str, count: i32) -> String {
    if count > 0 {
        format!("({count}) {name}")
    } else {
        name.to_string()
    }
}

pub use fnv_game::item_text::{avif_form, changes_text, whole, Change};

// The karma's alignment, the titles and their columns live with the rules
// (`fnv_game::karma_titles`), which GetPlayerKarmaTitle reads too.
pub use fnv_game::karma_titles::{karma_alignment, karma_column, karma_title, karma_title_setting, karma_title_with_ttw, TtwKarmaTitles};

/// [`karma_title_setting`] with optional `Data\NVSE\Plugins\TTW.ini` `[Karma Titles]`
/// overrides checked first (the INI file belongs to the player's installed data, not a
/// repo fallback table). When `ttw` is empty, behaves identically to
/// [`karma_title_setting`].
pub fn karma_title_setting_with_ttw(level: i64, alignment: &str, ttw: &TtwKarmaTitles, setting: impl Fn(&str) -> Option<String>) -> Option<String> {
    if let Some(title) = ttw.resolve(level, alignment) {
        return Some(title.to_string());
    }
    karma_title_setting(level, alignment, setting)
}

/// The title of a reputation from the levels (0–3) of fame and infamy
/// (`crate::reputation::TITLES`).
pub fn reputation_title(fame: usize, infamy: usize) -> &'static str {
    crate::reputation::title(fame, infamy)
}

/// The New Vegas statistics the player's General list names, in the order of their codes
/// [documented: GECK wiki GetPCMiscStat names them; observed: the original's list starts
/// Quests Completed, Locations Discovered, People Killed, Creatures Killed, Locks Picked,
/// Computers Hacked, Stimpaks Taken, Rad-X Taken, RadAway Taken, Chems Taken, owner's
/// screenshot in the private references, "pipboy stat tracking", 2026-10-14] (`fnv_game::misc_stats`).
pub const MISC_STATS: [&str; 43] = fnv_game::misc_stats::NAMES;

/// What a row of a list stands for, so a view that keeps tiles for its rows knows them by
/// this and not by their words (`crate::tiles::stats_menu`) [design].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RowKey {
    /// A row made up for a test or a view's own use.
    #[default]
    None,
    /// An actor value, by its code (S.P.E.C.I.A.L., Skills).
    Av(u16),
    /// A perk's record (Perks).
    Perk(FormId),
    /// A statistic, by its place in [`MISC_STATS`] (General).
    Stat(u8),
    /// A reputation's record (General's other list).
    Rep(FormId),
    /// A line of the EFF list, by its place.
    Effect(u16),
}

/// One row of a list (S.P.E.C.I.A.L., Skills, Perks, General): what it stands for, its name,
/// the number at the right, the mark after it, and what the display beside the list shows of it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Row {
    pub key: RowKey,
    pub name: String,
    pub value: Option<String>,
    pub mark: Option<String>,
    pub description: String,
    /// The picture, a path under `textures\`.
    pub icon: Option<String>,
    /// A weapon skill's mark, beside the picture in a view drawn from the file.
    pub badge: Option<String>,
    /// Drawn bold (a tag skill under the player's Embolden Tag Skills tweak).
    pub bold: bool,
}

/// One line of the EFF list: the source's name and its changes.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectLine {
    pub name: String,
    pub text: String,
}

/// A reputation the player has a standing with.
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    pub form: FormId,
    pub name: String,
    pub title: &'static str,
    pub fame: f64,
    pub infamy: f64,
    pub icon: Option<String>,
}

/// The status page's hints at the top right.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Hints {
    /// Stimpak, Doctor's Bag, RadAway, Rad-X: the text without the key.
    pub texts: [String; 4],
    /// How many the player carries.
    pub counts: [i32; 4],
    /// Drawn bright: the item would be used if chosen now ([`refusal`] on the body)
    /// [design: owner's request 2026-10-27, the CND hints lit while usable]; RadAway
    /// whenever one is carried (#1899). Every page reads it here, the hand-built one and
    /// the tile page alike.
    pub lit: [bool; 4],
}

impl Hints {
    /// The hints with their texts and counts: RadAway lit when carried (A uses it,
    /// #1899); the CND hints are lit later, once the body is known ([`refusal`]).
    pub fn new(texts: [String; 4], counts: [i32; 4]) -> Hints {
        let mut lit = [false; 4];
        lit[RADAWAY] = counts[RADAWAY] > 0;
        Hints { texts, counts, lit }
    }
}

/// Everything the page draws, compared frame to frame to know when to draw it again.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Snapshot {
    /// Whether the scripts' game is running (actor values, experience and perks come from
    /// it; without it the figures are "--").
    pub game: bool,
    pub level: Option<i64>,
    /// Current and maximum.
    pub hp: Option<(f64, f64)>,
    pub ap: Option<(f64, f64)>,
    /// Experience and what the next level needs.
    pub xp: Option<(f64, f64)>,
    /// The way through this level, in percent, under the player's Stats Menu Percentage
    /// XP tweak (shown instead of `xp`) [design: docs/design/tweaks.md].
    pub xp_percent: Option<f64>,
    /// No reputation to list and the player's Hide Empty Reputation tweak on: General
    /// shows the statistics only, with no switch [design: docs/design/tweaks.md].
    pub hide_reputation: bool,
    pub name: String,
    /// Each body part's condition, 0 to 1 ([`LIMBS`]).
    pub limbs: [f64; 6],
    pub hints: Hints,
    /// The CND limb-switch hint, from sSwitchLimbs [design: read::snapshot].
    pub switch_limbs: String,
    pub rad_resist: Option<f64>,
    pub rads: f64,
    /// Whether Hardcore mode is on, and its three needs: dehydration, hunger, sleep deprivation.
    pub hardcore: bool,
    pub needs: [f64; 3],
    pub effects: Vec<EffectLine>,
    pub special: Vec<Row>,
    pub skills: Vec<Row>,
    pub perks: Vec<Row>,
    pub general: Vec<Row>,
    pub standings: Vec<Standing>,
    pub karma: Option<(f64, &'static str, &'static str)>,
    /// The karma title for the player's level and alignment ([`karma_title`]).
    pub karma_title: Option<String>,
    /// That title when a setting holds it ([`karma_title_setting`]): a view drawn from the menu
    /// file writes only this, the table's words being from no data file [design: LEGAL.md].
    pub karma_setting_title: Option<String>,
}

impl Snapshot {
    /// The rows of a list tab (1 S.P.E.C.I.A.L., 2 Skills, 3 Perks, 4 General in the
    /// mode given).
    pub fn rows(&self, tab: usize, reputation: bool) -> Vec<Row> {
        match tab {
            1 => self.special.clone(),
            2 => self.skills.clone(),
            3 => self.perks.clone(),
            4 if reputation => self.standings.iter().map(|s| Row { key: RowKey::Rep(s.form), name: s.name.clone(), icon: s.icon.clone(), ..Row::default() }).collect(),
            4 => self.general.clone(),
            _ => Vec::new(),
        }
    }
}

/// The first row shown so that `selected` is among `visible` rows from it, moved as
/// little as possible from `top`.
pub fn keep_in_view(top: usize, selected: usize, visible: usize, len: usize) -> usize {
    let visible = visible.max(1);
    let top = if selected < top {
        selected
    } else if selected >= top + visible {
        selected + 1 - visible
    } else {
        top
    };
    top.min(len.saturating_sub(visible))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_values_map_to_their_records() {
        assert_eq!(avif_form(5), Some(0x3E8));
        assert_eq!(avif_form(11), Some(0x3EE));
        assert_eq!(avif_form(16), Some(0x450));
        assert_eq!(avif_form(20), Some(0x454));
        assert_eq!(avif_form(25), Some(0x459));
        assert_eq!(avif_form(32), Some(0x4B0));
        assert_eq!(avif_form(41), Some(0x4B9));
        assert_eq!(avif_form(54), Some(0x5E4));
        assert_eq!(avif_form(76), Some(0x5FA));
        assert_eq!(avif_form(77), None);
    }

    #[test]
    fn levels_need_more_experience_each_time() {
        assert_eq!(xp_for_level(2.0, 200.0, 150.0), 200.0);
        assert_eq!(xp_for_level(3.0, 200.0, 150.0), 550.0);
        assert_eq!(xp_for_level(4.0, 200.0, 150.0), 1050.0);
    }

    #[test]
    fn effects_are_written_as_the_original_writes_them() {
        let c = [
            Change { value: "Speech".into(), amount: 2.0, detrimental: false },
            Change { value: "Melee Weap.".into(), amount: 2.0, detrimental: false },
        ];
        assert_eq!(changes_text(&c), "Speech +2, Melee Weap. +2");
        let d = [Change { value: "AGL".into(), amount: 1.0, detrimental: true }];
        assert_eq!(changes_text(&d), "AGL -1");
    }

    #[test]
    fn marks_and_hints() {
        assert_eq!(modified_mark(34.0, 32.0), Some("(+)"));
        assert_eq!(modified_mark(30.0, 32.0), Some("(-)"));
        assert_eq!(modified_mark(32.2, 32.0), None);
        assert_eq!(hint_text("Stimpak", 9), "(9) Stimpak");
    }

    #[test]
    fn the_cnd_hints_are_used_only_when_they_would_heal() {
        let whole = [1.0; 6];
        let mut arm = whole;
        arm[2] = 0.4;
        // A Stimpak on the body: only when health is not full.
        assert_eq!(refusal(STIMPAK, None, 3, (100.0, 100.0), &arm, false).as_deref(), Some("health is full"));
        assert_eq!(refusal(STIMPAK, None, 3, (60.0, 100.0), &whole, false), None);
        // On a limb: only when that limb is hurt, whatever the health.
        assert_eq!(refusal(STIMPAK, Some(2), 3, (100.0, 100.0), &arm, false), None);
        assert_eq!(refusal(STIMPAK, Some(3), 3, (60.0, 100.0), &arm, false).as_deref(), Some("the right arm is not hurt"));
        // A Doctor's Bag: when any limb is hurt.
        assert_eq!(refusal(DOCTORS_BAG, None, 1, (100.0, 100.0), &arm, false), None);
        assert_eq!(refusal(DOCTORS_BAG, None, 1, (60.0, 100.0), &whole, false).as_deref(), Some("no limb is hurt"));
        // None carried; this CND refusal helper does not handle the RAD actions.
        assert_eq!(refusal(STIMPAK, None, 0, (10.0, 100.0), &arm, false).as_deref(), Some("none carried"));
        assert!(refusal(2, None, 5, (10.0, 100.0), &arm, false).is_some());
        // Hardcore: a Stimpak on a limb is refused, on the body it is not; a Doctor's Bag on
        // a hurt limb is allowed and on a whole limb is refused (unless Aid at Full Health).
        assert_eq!(refusal(STIMPAK, Some(2), 3, (60.0, 100.0), &arm, true).as_deref(), Some("a Stimpak mends no limb in Hardcore mode"));
        assert_eq!(refusal(STIMPAK, None, 3, (60.0, 100.0), &arm, true), None);
        assert_eq!(refusal(DOCTORS_BAG, None, 1, (100.0, 100.0), &arm, true), None);
        assert_eq!(refusal(DOCTORS_BAG, Some(2), 1, (100.0, 100.0), &arm, true), None);
        assert_eq!(refusal(DOCTORS_BAG, Some(3), 1, (100.0, 100.0), &arm, true).as_deref(), Some("the right arm is not hurt"));
        assert_eq!(refusal_with(DOCTORS_BAG, Some(3), 1, (100.0, 100.0), &arm, true, true), None);
        assert_eq!(hint_text("RadAway", 0), "RadAway");
    }

    #[test]
    fn radaway_is_lit_while_carried_and_rad_x_never() {
        let texts: [String; 4] = Default::default();
        assert_eq!(Hints::new(texts.clone(), [3, 1, 2, 5]).lit, [false, false, true, false], "RadAway carried: lit; Rad-X is not used from here");
        assert_eq!(Hints::new(texts, [3, 1, 0, 5]).lit, [false; 4], "none carried: dim");
    }

    #[test]
    fn karma_and_reputation_words() {
        assert_eq!(karma_alignment(0.0, -750.0, -250.0, 250.0, 750.0).0, "Neutral");
        assert_eq!(karma_alignment(250.0, -750.0, -250.0, 250.0, 750.0).0, "Good");
        assert_eq!(karma_alignment(-800.0, -750.0, -250.0, 250.0, 750.0), ("Very Evil", "karma_evil"));
        // The plugins' setting wins; a level without one takes the nearest in its column;
        // none at all, the alignment word; 30 and over share.
        let esm = |n: &str| (n == "sKarmicTitleEvil29").then(|| "Demon Spawn".to_string());
        assert_eq!(karma_title(29, "Very Evil", esm), "Demon Spawn");
        assert_eq!(karma_title(45, "Evil", esm), "Demon Spawn");
        assert_eq!(karma_title(29, "Neutral", esm), "Neutral");
        assert_eq!(karma_title(1, "Good", |_| None), "Good");
        assert_eq!(karma_title_setting(29, "Very Evil", esm).as_deref(), Some("Demon Spawn"));
        assert_eq!(karma_title_setting(29, "Neutral", esm), None);
        let padded = |n: &str| (n == "sKarmicTitleNeutral01").then(|| "From the plugin".to_string());
        assert_eq!(karma_title_setting(1, "Neutral", padded).as_deref(), Some("From the plugin"));
        assert_eq!(karma_title(1, "Neutral", padded), "From the plugin");
        assert_eq!(reputation_title(0, 0), "Neutral");
        assert_eq!(reputation_title(3, 0), "Celebrated");
        assert_eq!(reputation_title(1, 1), "Uncertain");
    }

    #[test]
    fn every_list_knows_its_rows_by_what_they_stand_for() {
        let s = Snapshot { standings: vec![Standing { form: FormId(9), name: "A".into(), title: "T", fame: 1.0, infamy: 0.0, icon: None }], ..Snapshot::default() };
        assert_eq!(s.rows(4, true)[0].key, RowKey::Rep(FormId(9)));
        assert_eq!(Row::default().key, RowKey::None);
    }

    #[test]
    fn the_selection_stays_in_view() {
        assert_eq!(keep_in_view(0, 3, 9, 13), 0);
        assert_eq!(keep_in_view(0, 9, 9, 13), 1);
        assert_eq!(keep_in_view(4, 12, 9, 13), 4);
        assert_eq!(keep_in_view(4, 2, 9, 13), 2);
        assert_eq!(keep_in_view(6, 6, 9, 13), 4);
        assert_eq!(keep_in_view(0, 0, 9, 3), 0);
    }
}
