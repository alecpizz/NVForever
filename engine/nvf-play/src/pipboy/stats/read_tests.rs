//! The STATS page's figures and worn-apparel effects over a synthetic plugin (no game
//! data): worn enchantments are the game's active effects, counted once (#2495).

use super::*;
use fnv_esm::raw::{write_subrecords, Subrecord};

const P: FormId = FormId::PLAYER_REF;
const MOD_PER: u32 = 0x850;
const MOD_SPEECH: u32 = 0x851;
const REDUCE_AGI: u32 = 0x852;
const ENCH_HAT: u32 = 0x880;
const ENCH_GLASSES: u32 = 0x881;
const ENCH_METAL: u32 = 0x882;
const HAT: u32 = 0x890;
const GLASSES: u32 = 0x891;
const METAL: u32 = 0x892;
const PERCEPTION: u16 = 6;
const AGILITY: u16 = 10;
const LUCK: u16 = 11;
const SPEECH: u16 = 38;

fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
    Subrecord { sig: *sig, data, extended: false }
}

fn z(s: &str) -> Vec<u8> {
    [s.as_bytes(), &[0]].concat()
}

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn record(sig: &[u8; 4], fid: u32, subs: &[Subrecord]) -> Vec<u8> {
    let payload = write_subrecords(subs);
    [&sig[..], &(payload.len() as u32).to_le_bytes(), &0u32.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
}

fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &sig[..], &0i32.to_le_bytes(), &[0; 8], body].concat()
}

fn tes4() -> Vec<u8> {
    let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat();
    let payload = write_subrecords(&[sub(b"HEDR", hedr)]);
    [&b"TES4"[..], &(payload.len() as u32).to_le_bytes(), &1u32.to_le_bytes(), &0u32.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
}

/// MGEF DATA (72 bytes): flags (2 Recover, 4 Detrimental), archetype 0 (Value Modifier)
/// and actor value; everything else empty (as fnv-game's `tests/abilities.rs`).
fn mgef(fid: u32, edid: &str, flags: u32, av: i32) -> Vec<u8> {
    let data = [u32s(&[flags]), 0f32.to_le_bytes().to_vec(), u32s(&[0]), vec![0xFF; 8], vec![0; 4], vec![0; 4], 1f32.to_le_bytes().to_vec(), vec![0; 24], vec![0; 8], u32s(&[0]), av.to_le_bytes().to_vec()].concat();
    record(b"MGEF", fid, &[sub(b"EDID", z(edid)), sub(b"DATA", data)])
}

/// EFID + EFIT (magnitude, area, duration, type Self, actor value), with conditions.
fn effect(base: u32, magnitude: u32, av: i32, conds: Vec<Subrecord>) -> Vec<Subrecord> {
    let mut v = vec![sub(b"EFID", base.to_le_bytes().to_vec()), sub(b"EFIT", [u32s(&[magnitude, 0, 0, 0]), av.to_le_bytes().to_vec()].concat())];
    v.extend(conds);
    v
}

/// A condition `GetActorValue Luck >= value` on the subject (comparison byte 0x60).
fn luck_at_least(value: f32) -> Subrecord {
    let mut d = vec![0x60, 0, 0, 0];
    d.extend(value.to_le_bytes());
    d.extend(14u16.to_le_bytes());
    d.extend([0, 0]);
    d.extend(u32::from(LUCK).to_le_bytes());
    d.extend([0; 12]);
    sub(b"CTDA", d)
}

/// ENCH of type 3 (Apparel) with its effects.
fn ench(fid: u32, edid: &str, effects: Vec<Vec<Subrecord>>) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"ENIT", [u32s(&[3, 0, 0]), vec![0; 4]].concat())];
    subs.extend(effects.into_iter().flatten());
    record(b"ENCH", fid, &subs)
}

/// ARMO in one biped slot with its object effect.
fn armo(fid: u32, edid: &str, slot: u32, ench: u32) -> Vec<u8> {
    let bmdt = [u32s(&[slot]), vec![0; 4]].concat();
    let data = [u32s(&[1, 100]), 1f32.to_le_bytes().to_vec()].concat();
    record(b"ARMO", fid, &[sub(b"EDID", z(edid)), sub(b"EITM", ench.to_le_bytes().to_vec()), sub(b"BMDT", bmdt), sub(b"DATA", data)])
}

/// A hat (Perception +1 and Speech +2, as a worn bonus), glasses (Perception +2 only
/// while Luck is 7 or more, as the Four Eyes glasses' condition) and metal armour
/// (Agility -1, Detrimental, as a worn penalty).
fn plugin() -> fnv_esm::Plugin {
    let mgefs = [mgef(MOD_PER, "IncPer", 2, 6), mgef(MOD_SPEECH, "IncSpeech", 2, 38), mgef(REDUCE_AGI, "RedAgi", 2 | 4, 10)].concat();
    let enchs = [
        ench(ENCH_HAT, "EnchHat", vec![effect(MOD_PER, 1, 6, vec![]), effect(MOD_SPEECH, 2, 38, vec![])]),
        ench(ENCH_GLASSES, "EnchGlasses", vec![effect(MOD_PER, 2, 6, vec![luck_at_least(7.0)])]),
        ench(ENCH_METAL, "EnchMetal", vec![effect(REDUCE_AGI, 1, 10, vec![])]),
    ]
    .concat();
    let armos = [armo(HAT, "Hat", 1 << 0, ENCH_HAT), armo(GLASSES, "Glasses", 1 << 15, ENCH_GLASSES), armo(METAL, "Metal", 1 << 2, ENCH_METAL)].concat();
    let player = record(b"NPC_", 0x7, &[sub(b"EDID", z("Player")), sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat())]);
    let bytes = [tes4(), top(b"MGEF", &mgefs), top(b"NPC_", &player), top(b"ENCH", &enchs), top(b"ARMO", &armos)].concat();
    fnv_esm::Plugin::parse(&bytes, "Worn.esm".into()).unwrap()
}

/// The game over the fixture: the player with Perception, Agility and Luck 5 and Speech
/// 15, wearing `items`, the worn enchantments brought up to date.
fn wearing(items: &[u32]) -> ScriptState {
    let mut state = ScriptState::over_plugins(&[plugin()]);
    let g = &mut state.rt.as_mut().unwrap().game;
    for (code, v) in [(PERCEPTION, 5.0), (AGILITY, 5.0), (LUCK, 5.0), (SPEECH, 15.0)] {
        gf::set_actor_value_current(g, P, code, v).unwrap();
    }
    for &item in items {
        g.world.add_item(P, FormId(item), 1).unwrap();
        g.world.equip(P, FormId(item)).unwrap();
    }
    g.sync_actor_effects(P);
    state
}

/// The page's figure and permanent value of `code`, after checking that the figure is the
/// game's own actor value.
fn figure(g: &mut fnv_game::Game<'static>, code: u16) -> (f64, f64) {
    let shown = shown_and_permanent(g, code).unwrap();
    assert_eq!(shown.0, gf::actor_value(g, P, code).unwrap(), "actor value {code}: the page's figure is the game's");
    shown
}

/// The worn effects as (item, [(actor value, signed amount)]), by item.
fn worn_changes(g: &fnv_game::Game<'_>) -> Vec<(u32, Vec<(u16, f64)>)> {
    let mut out: Vec<(u32, Vec<(u16, f64)>)> =
        worn_effects(g).into_iter().map(|((item, _), effects)| (item.0, effects.iter().filter_map(|e| e.change).map(|c| (c.code, c.amount)).collect())).collect();
    out.sort_by_key(|x| x.0);
    out
}

/// A worn bonus and a worn penalty are counted once: the figure is the game's actor
/// value (base, modifiers, the worn enchantment in the temporary pool), marked against the
/// permanent value, and each worn item has its line's changes.
#[test]
fn worn_bonus_and_penalty_are_counted_once() {
    let mut state = wearing(&[HAT, METAL]);
    let g = &mut state.rt.as_mut().unwrap().game;
    assert_eq!(figure(g, PERCEPTION), (6.0, 5.0));
    assert_eq!(figure(g, SPEECH), (17.0, 15.0));
    assert_eq!(figure(g, AGILITY), (4.0, 5.0));
    assert_eq!(model::modified_mark(6.0, 5.0), Some("(+)"));
    assert_eq!(model::modified_mark(4.0, 5.0), Some("(-)"));
    assert_eq!(worn_changes(g), vec![(HAT, vec![(PERCEPTION, 1.0), (SPEECH, 2.0)]), (METAL, vec![(AGILITY, -1.0)])]);
}

/// A worn enchantment whose condition fails changes nothing and has no line; once the
/// condition passes it changes the figure and is listed.
#[test]
fn a_failing_condition_changes_nothing_and_is_not_listed() {
    let mut state = wearing(&[GLASSES]);
    let g = &mut state.rt.as_mut().unwrap().game;
    assert!(g.effects.iter().any(|e| e.worn == Some(FormId(GLASSES)) && e.inactive), "the game holds the glasses' effect, inactive");
    let (shown, permanent) = figure(g, PERCEPTION);
    assert_eq!((shown, permanent), (5.0, 5.0));
    assert_eq!(model::modified_mark(shown, permanent), None);
    assert!(worn_changes(g).is_empty());
    // The condition passes (checked every second).
    gf::set_actor_value_current(g, P, LUCK, 8.0).unwrap();
    for _ in 0..90 {
        g.tick();
    }
    assert_eq!(figure(g, PERCEPTION), (7.0, 5.0));
    assert_eq!(worn_changes(g), vec![(GLASSES, vec![(PERCEPTION, 2.0)])]);
}

/// #3119: an item's effect text is its values, then the names of effects shown by name;
/// either alone stands by itself, and an empty name is left out.
#[test]
fn effect_text_puts_the_names_after_the_values() {
    assert_eq!(with_names("STR +2".into(), ["Damage +25%".to_string()]), "STR +2, Damage +25%");
    assert_eq!(with_names(String::new(), ["Damage +25%".to_string()]), "Damage +25%");
    assert_eq!(with_names("HP +40".into(), Vec::<String>::new()), "HP +40");
    assert_eq!(with_names(String::new(), ["".to_string(), "Low-Light Vision".to_string()]), "Low-Light Vision");
    assert_eq!(with_names(String::new(), Vec::<String>::new()), "");
}

/// #3118: the Perks list shows a perk the player has unless it is Hidden; Playable (the
/// level-up menu's) does not matter. Lord Death's DATA is 00 00 03 00 00 (3 ranks, not
/// Playable, not Hidden) [observed: `fnvtool esm decode`]; Psycho's effect perk is Hidden;
/// a 4-byte DATA (Ant Sight) has no Hidden byte.
#[test]
fn a_perk_is_listed_unless_hidden_playable_or_not() {
    let lord_death = PerkData::of(&[0, 0, 3, 0, 0]);
    assert!(lord_death.listed(), "not Playable, not Hidden: listed");
    assert_eq!(lord_death.ranks, 3);
    assert!(PerkData::of(&[0, 2, 10, 1, 0]).listed(), "Playable, not Hidden: listed as before");
    assert!(!PerkData::of(&[0, 0, 1, 0, 1]).listed(), "Hidden: left out");
    assert!(!PerkData::of(&[0, 0, 1, 1, 1]).listed(), "Playable and Hidden: left out");
    assert!(PerkData::of(&[0, 0, 1, 0]).listed(), "4-byte DATA, not Playable: listed");
    assert_eq!(PerkData::of(&[]).ranks, 1, "no DATA: one rank");
}

/// #3400: an actor value's short name (AVIF ANAM) is read in the load order's code page,
/// as FULL is: on a Russian install Windows-1251's "Крас." (Charisma's abbreviation, say),
/// not the Latin-1 letters "Êðàñ." the font has no glyphs for.
#[test]
fn a_short_name_is_read_in_the_code_page() {
    use fnv_util::text::CodePage;
    let anam = [0xCA, 0xF0, 0xE0, 0xF1, b'.', 0];
    assert_eq!(zstring(&anam, CodePage::Windows1251), "Крас.");
    assert_eq!(zstring(b"CHR\0junk", CodePage::Windows1252), "CHR", "stops at the first NUL");
    assert_ne!(zstring(&anam, CodePage::Windows1252), "Крас.");
}

#[test]
fn pipboy_stats_snapshot_resolves_ttw_ini_karma_titles_for_levels_1_through_50() {
    fn glob(fid: u32, edid: &str, v: f32) -> Vec<u8> {
        record(b"GLOB", fid, &[sub(b"EDID", z(edid)), sub(b"FNAM", vec![b'f']), sub(b"FLTV", v.to_le_bytes().to_vec())])
    }
    let globs = [
        glob(0x900, "KarmaVeryEvil", -750.0),
        glob(0x901, "KarmaEvil", -250.0),
        glob(0x902, "KarmaGood", 250.0),
        glob(0x903, "KarmaVeryGood", 750.0),
    ]
    .concat();
    let player = record(b"NPC_", 0x7, &[sub(b"EDID", z("Player")), sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat())]);
    let bytes = [tes4(), top(b"GLOB", &globs), top(b"NPC_", &player)].concat();
    let plugins: &'static [fnv_esm::Plugin] = Box::leak(Box::new([fnv_esm::Plugin::parse(&bytes, "Karma.esm".into()).unwrap()]));
    let lo: &'static LoadOrder<'static> = Box::leak(Box::new(LoadOrder::new(plugins).unwrap()));
    let schema: &'static Schema = Box::leak(Box::new(Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap()));
    let mut forms = Forms::new(lo, schema);
    let mut sim = SimState::new(lo, schema, &fnv_world::WorldIndex::default());
    let mut script = ScriptState::over_plugins(plugins);

    // No TTW.ini and no sKarmicTitle* setting in the synthetic plugin: the title is the
    // alignment word, and the tile's setting title is None.
    {
        let g = &mut script.rt.as_mut().unwrap().game;
        g.player.level = Some(45);
        gf::set_actor_value_current(g, P, av::KARMA, -500.0).unwrap();
    }
    let vanilla = snapshot(&mut forms, &mut sim, Some(&mut script));
    assert_eq!(vanilla.karma_title.as_deref(), Some("Evil"));
    assert_eq!(vanilla.karma_setting_title, None);

    // Synthetic TTW.ini with level 1..=50 titles across Good, Neutral, and Evil.
    let mut ini = String::from("[Karma Titles]\n");
    for lvl in 1..=50 {
        ini.push_str(&format!("sKarmicTitleGood{lvl}=SynthGood{lvl}\n"));
        ini.push_str(&format!("sKarmicTitleNeutral{lvl}=SynthNeutral{lvl}\n"));
        ini.push_str(&format!("sKarmicTitleEvil{lvl}=SynthEvil{lvl}\n"));
    }
    script.rt.as_mut().unwrap().game.mod_files.write_bytes("NVSE/Plugins/TTW.ini", ini.as_bytes());

    let evil_45 = snapshot(&mut forms, &mut sim, Some(&mut script));
    assert_eq!(evil_45.karma_title.as_deref(), Some("SynthEvil45"));
    assert_eq!(evil_45.karma_setting_title.as_deref(), Some("SynthEvil45"));

    {
        let g = &mut script.rt.as_mut().unwrap().game;
        g.player.level = Some(50);
        gf::set_actor_value_current(g, P, av::KARMA, 800.0).unwrap();
    }
    let good_50 = snapshot(&mut forms, &mut sim, Some(&mut script));
    assert_eq!(good_50.karma_title.as_deref(), Some("SynthGood50"));
    assert_eq!(good_50.karma_setting_title.as_deref(), Some("SynthGood50"));

    {
        let g = &mut script.rt.as_mut().unwrap().game;
        g.player.level = Some(31);
        gf::set_actor_value_current(g, P, av::KARMA, 0.0).unwrap();
    }
    let neutral_31 = snapshot(&mut forms, &mut sim, Some(&mut script));
    assert_eq!(neutral_31.karma_title.as_deref(), Some("SynthNeutral31"));
    assert_eq!(neutral_31.karma_setting_title.as_deref(), Some("SynthNeutral31"));
}
