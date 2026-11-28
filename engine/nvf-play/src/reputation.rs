//! Reputation in play (docs/spec-notes/reputation.md; owner's playtest list 5, item 13):
//! the rules are the game's (`fnv_game::functions::reputation`); this module shows what
//! came of them.
//!
//! - **The box**: when a reputation's title changes (its fame or infamy reaches another
//!   level, from a script's AddReputation or a crime, `crate::crime`) the game opens a
//!   message box for it: the reputation's name as the title, the new title and its
//!   description, a Vault Boy face at the left and the `sOk` button [observed: the
//!   owner's two screenshots in C:\fnv-private\Faction Reputation, Goodsprings at fame
//!   level 2 and the Powder Gangers at infamy level 3; the words shown are NVForever's
//!   own, [`TITLES`]]. It waits in the game's queue of boxes (`Game::show_box` with
//!   the REPU's FormID), so it stops the game as a message box does; `crate::script_hud`
//!   draws it with [`panel`].
//! - **Sound**: `UIRepGood` when fame reached a new level, else `UIRepBad` [observed: the
//!   two SOUN records in FalloutNV.esm; hypothesis: when each plays].
//! - **Log**: `reputation:` lines for each change of title.
//! - **Not in a fight**: while the player is in combat the box, sound and log line wait,
//!   and come once the fight has been over for a short while (a title that changed back
//!   meanwhile shows nothing) [owner requests 2026-10-19 and 2026-10-30, #1050]: the
//!   shared gate `crate::after_combat` (in combat is the wait menu's test, `crate::wait`).
//! - A save keeps the titles last shown, so a load shows no box for what it restores.
//! - **Disguises**: the titles are those of the player's real reputation
//!   (`fnv_game::functions::reputation::real`), so faction armour, whose scripts set
//!   every reputation to 0 while it is worn and back after, shows no box when put on or
//!   taken off; fame or infamy earned while it is worn shows by the real title.

use crate::message_menu::PanelView;
use crate::save::SaveHook;
use crate::script::ScriptState;
use crate::sfx::PlaySound;
use crate::sim::SimState;
use crate::PlaySet;
use bevy::prelude::*;
use fnv_game::functions as gf;
use fnv_sim::FormId;
use std::collections::BTreeMap;

/// The titles of the fame and infamy levels (0–3 each), by fame then infamy: NVForever's
/// own words [design, #2982]. The load order holds no title setting [observed:
/// FalloutNV.esm and the DLC masters, `fnvtool esm inventory`], and the original's
/// built-in titles are not in the data, so they are not copied (LEGAL.md).
pub const TITLES: [[&str; 4]; 4] = [
    ["Neutral", "Distrusted", "Despised", "Reviled"],
    ["Welcomed", "Uncertain", "Rough Helper", "Kind Brute"],
    ["Trusted", "Lovable Rogue", "Puzzle", "Feared Benefactor"],
    ["Celebrated", "Cheerful Scoundrel", "Grim Champion", "Enigma"],
];

/// The titles' descriptions, as [`TITLES`]: NVForever's own words [design, #2982], used
/// where the load order has no setting `sRepTitlePos…Neg…Desc` ([`description`]).
const DESCRIPTIONS: [[&str; 4]; 4] = [
    [
        "Too little is known of you here for anyone to have made up their mind.",
        "Word of your misdeeds has spread, and people here keep you at arm's length.",
        "Your deeds have turned most people here against you.",
        "Your cruelty is so well known here that people want you gone.",
    ],
    [
        "Your help has been noticed, and people here are glad to see you.",
        "You have done some good here and some harm, and people cannot decide about you yet.",
        "You have lent a hand here now and then, but your harm is what people remember.",
        "People here fear your violence, yet some recall a kindness of yours.",
    ],
    [
        "Your good deeds are known here, and people count you as a friend.",
        "People here think well of you, though you are known to stir up trouble.",
        "Your deeds here have been both generous and harsh, and nobody knows what to expect from you.",
        "People here dread you, though they cannot deny the good you have done.",
    ],
    [
        "Your generosity is a byword here, and people hold you in the highest regard.",
        "People here adore you, and forgive the mischief you get up to.",
        "People here see you as a champion, if a frightening one.",
        "Your deeds here have been so great and so terrible that people hardly know whether to cheer or run.",
    ],
];

/// The faces at the box's left: more fame than infamy, more infamy, as much of each
/// [observed: the owner's screenshots show `glow_message_vaultboy_very_happy` for fame
/// level 2 without infamy and `glow_message_vaultboy_sad` for infamy level 3 without
/// fame; hypothesis: the rule, and the
/// neutral face for mixed titles].
const GOOD_FACE: &str = "textures\\interface\\icons\\message icons\\glow_message_vaultboy_very_happy.dds";
const BAD_FACE: &str = "textures\\interface\\icons\\message icons\\glow_message_vaultboy_sad.dds";
const MIXED_FACE: &str = "textures\\interface\\icons\\message icons\\glow_message_vaultboy_neutral.dds";

/// The setting naming the box's button, read through `fnv_game::ui_loading::game_string`
/// (the load order's GMST, else NVForever's own label; LEGAL.md) [design, #1183].
const OK_SETTING: &str = "sOk";

const NUMBERS: [&str; 4] = ["Zero", "One", "Two", "Three"];

pub fn title(fame: usize, infamy: usize) -> &'static str {
    TITLES[fame.min(3)][infamy.min(3)]
}

/// The description of a title: the game setting `sRepTitlePos<fame>Neg<infamy>Desc`
/// (numbers in words) when the load order has it [hypothesis: the pattern of the one
/// FalloutNV.esm holds, `sRepTitlePosTwoNegThreeDesc`], else [`DESCRIPTIONS`].
pub fn description(fame: usize, infamy: usize, setting: impl Fn(&str) -> Option<String>) -> String {
    let (f, i) = (fame.min(3), infamy.min(3));
    setting(&format!("sRepTitlePos{}Neg{}Desc", NUMBERS[f], NUMBERS[i])).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| DESCRIPTIONS[f][i].to_string())
}

pub fn face(fame: usize, infamy: usize) -> &'static str {
    match fame.cmp(&infamy) {
        std::cmp::Ordering::Greater => GOOD_FACE,
        std::cmp::Ordering::Less => BAD_FACE,
        std::cmp::Ordering::Equal => MIXED_FACE,
    }
}

/// The fame and infamy levels of the player's real reputation now.
pub fn levels(g: &fnv_game::Game, rep: FormId) -> (usize, usize) {
    let scale = g.data.reputations.get(&rep).copied().unwrap_or(0.0) as f64;
    let [infamy, fame] = gf::reputation_real(g, rep);
    (gf::reputation_level(fame, scale), gf::reputation_level(infamy, scale))
}

/// A reputation's name (REPU FULL), else its Editor ID.
pub fn name(g: &fnv_game::Game, rep: FormId) -> String {
    g.data.reputation_names.get(&rep).cloned().unwrap_or_else(|| g.data.name(rep))
}

/// The box for reputation `rep` (the front of the game's box queue).
pub fn panel(g: &fnv_game::Game, sim: &SimState, rep: FormId) -> PanelView {
    let (fame, infamy) = levels(g, rep);
    let setting = |n: &str| match sim.forms.settings.get(&n.to_ascii_lowercase()) {
        Some(fnv_sim::settings::Setting::Str(s)) => Some(s.clone()),
        _ => None,
    };
    let name = name(g, rep);
    PanelView {
        title: Some(name),
        text: format!("{}\n{}", title(fame, infamy), description(fame, infamy, setting)),
        buttons: vec![fnv_game::ui_loading::game_string(g, OK_SETTING)],
        icon: Some(face(fame, infamy).into()),
    }
}

/// Whether the box at the front is a reputation's.
pub fn is_reputation(g: &fnv_game::Game, front: FormId) -> bool {
    g.data.reputations.contains_key(&front)
}

/// The levels last shown for each reputation; `None` until the game is first seen (or
/// again after a load without them).
#[derive(Resource, Default)]
pub struct Shown(Option<BTreeMap<u32, (usize, usize)>>);

pub struct ReputationPlugin;

impl Plugin for ReputationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shown>().add_systems(Update, watch.after(PlaySet::Scripts).before(PlaySet::Sync));
        crate::save::add_save_hook(app, SAVE_HOOK);
    }
}

const SAVE_HOOK: SaveHook = SaveHook {
    key: "reputation_shown",
    save: |w| {
        let s = w.get_resource::<Shown>()?.0.as_ref()?;
        serde_json::to_value(s.iter().map(|(k, v)| (format!("{k:08X}"), [v.0, v.1])).collect::<BTreeMap<_, _>>()).ok()
    },
    load: |w, v| {
        let m: BTreeMap<String, [usize; 2]> = serde_json::from_value(v.clone()).unwrap_or_default();
        let m = m.into_iter().filter_map(|(k, [f, i])| Some((u32::from_str_radix(&k, 16).ok()?, (f, i)))).collect();
        w.insert_resource(Shown(Some(m)));
    },
    // Keyed by faction FormID in hex: moved to the current load order, those of plugins
    // no longer loaded dropped.
    remap: Some(|v, r| {
        if let Some(o) = v.as_object_mut() {
            *o = std::mem::take(o).into_iter().filter_map(|(k, x)| Some((format!("{:08X}", r.map(u32::from_str_radix(&k, 16).ok()?)?), x))).collect();
        }
    }),
};

/// A title that changed since it was last shown: the box, the sound, the log line, held
/// back while the player is in combat and for a while after (`crate::after_combat`).
pub(crate) fn watch(
    mut script: Option<ResMut<ScriptState>>,
    sim: Res<SimState>,
    after: Option<Res<crate::after_combat::AfterCombat>>,
    mut shown: ResMut<Shown>,
    mut sounds: MessageWriter<PlaySound>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let g = &mut rt.game;
    if shown.0.is_some() && after.is_some_and(|a| !a.settled()) {
        return;
    }
    let now: BTreeMap<u32, (usize, usize)> = g.player.reputation.keys().chain(g.player.reputation_hidden.keys()).map(|&r| (r.0, levels(g, r))).collect();
    let Some(before) = shown.0.as_ref() else {
        shown.0 = Some(now);
        return;
    };
    if *before == now {
        return;
    }
    for (&r, &(fame, infamy)) in &now {
        let (f0, i0) = before.get(&r).copied().unwrap_or((0, 0));
        if (f0, i0) == (fame, infamy) {
            continue;
        }
        let rep = FormId(r);
        let [inf, fam] = gf::reputation_real(g, rep);
        let name = name(g, rep);
        info!("reputation: {name} {} -> {} (fame {fam}, infamy {inf})", title(f0, i0), title(fame, infamy));
        // The player's tweak keeps reputation changes quiet [design: docs/design/tweaks.md].
        if tweaks.as_ref().is_some_and(|t| t.no_reputation_messages) {
            continue;
        }
        g.show_box(rep);
        let sound = if fame > f0 { "UIRepGood" } else { "UIRepBad" };
        if let Some(s) = sim.forms.by_edid(sound) {
            sounds.write(PlaySound { sound: s, at: None });
        }
    }
    shown.0 = Some(now);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_and_faces_follow_fame_and_infamy() {
        assert_eq!(title(0, 0), "Neutral");
        assert_eq!(title(2, 0), "Trusted");
        assert_eq!(title(0, 3), "Reviled");
        assert_eq!(title(2, 3), "Feared Benefactor");
        assert_eq!(title(3, 2), "Grim Champion");
        assert_eq!(title(9, 9), "Enigma");
        assert_eq!(face(2, 0), GOOD_FACE);
        assert_eq!(face(0, 3), BAD_FACE);
        assert_eq!(face(1, 1), MIXED_FACE);
    }

    #[test]
    fn a_plugins_description_wins() {
        let none = |_: &str| None;
        assert!(description(2, 0, none).starts_with("Your good deeds"));
        let one = |n: &str| (n == "sRepTitlePosTwoNegThreeDesc").then(|| "Theirs.".to_string());
        assert_eq!(description(2, 3, one), "Theirs.");
        assert!(description(3, 2, one).starts_with("People here see you"));
    }
}
