//! Lutana NVSE batch 1 (#1009; docs/spec-notes/ext-lutana.md) on a synthetic plugin:
//! challenges, casinos, recipes, actor AI data, perks, zones, leveled lists, activators,
//! terminals, load screens, form flags and playability, and the misc getters.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CHAL: u32 = 0x900;
const CSNO: u32 = 0x901;
const RCPE: u32 = 0x902;
const RCPE2: u32 = 0x903;
const RCCT: u32 = 0x904;
const ITEM_A: u32 = 0x905;
const ITEM_B: u32 = 0x906;
const ITEM_C: u32 = 0x907;
const NPC: u32 = 0x910;
const CSTY: u32 = 0x911;
const PERK_TRAIT: u32 = 0x912;
const PERK: u32 = 0x913;
const ECZN: u32 = 0x914;
const GLOB: u32 = 0x915;
const LVLI: u32 = 0x916;
const ACTI: u32 = 0x917;
const TACT: u32 = 0x918;
const SOUN: u32 = 0x919;
const TERM: u32 = 0x91A;
const NOTE: u32 = 0x91B;
const LSCR: u32 = 0x91C;
const LSCT: u32 = 0x91D;
const LSCT2: u32 = 0x91E;
const HAIR: u32 = 0x91F;
const LIGH: u32 = 0x920;
const NOTE2: u32 = 0x921;
const ROOM: u32 = 0x930;
const TERM_REF: u32 = 0x931;
const LIGHT_REF: u32 = 0x932;

fn le(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn edid(sig: &[u8; 4], fid: u32, name: &str, rest: &[fnv_esm::raw::Subrecord]) -> Vec<u8> {
    record(sig, 0, fid, &[vec![sub(b"EDID", z(name))], rest.to_vec()].concat())
}

fn plugin_bytes() -> Vec<u8> {
    // CHAL DATA [documented: schemas/fnv/gameplay.json CHAL]: type 2 (KillInCategory),
    // threshold 50, flags 2 (Recurring), interval 10, values; SNAM, XNAM.
    let chal = edid(b"CHAL", CHAL, "TestChal", &[sub(b"DATA", [u32s(&[2, 50, 2, 10]), vec![0; 8]].concat()), sub(b"SNAM", le(ITEM_A)), sub(b"XNAM", le(ITEM_B))]);
    // CSNO DATA: shuffle share, payout, 7 reel stops, decks, max winnings, chip, quest,
    // soft 17 flags.
    let csno = edid(b"CSNO", CSNO, "TestCasino", &[sub(b"DATA", [f32b(0.5), f32b(1.5), u32s(&[2, 2, 2, 2, 2, 2, 2]), u32s(&[3, 9000, 0, 0, 0])].concat())]);
    // RCPE DATA: skill (Survival 44), level 30, category, subcategory; RCIL/RCOD pairs.
    let rcpe = edid(b"RCPE", RCPE, "TestRecipe", &[sub(b"DATA", u32s(&[44, 30, RCCT, 0])), sub(b"RCIL", le(ITEM_A)), sub(b"RCQY", le(2)), sub(b"RCOD", le(ITEM_C)), sub(b"RCQY", le(1))]);
    let rcpe2 = edid(b"RCPE", RCPE2, "TestRecipe2", &[sub(b"DATA", [(-1i32).to_le_bytes().to_vec(), u32s(&[0, 0, 0])].concat()), sub(b"RCIL", le(ITEM_A)), sub(b"RCQY", le(1)), sub(b"RCOD", le(ITEM_B)), sub(b"RCQY", le(3))]);
    let rcct = edid(b"RCCT", RCCT, "TestCategory", &[sub(b"DATA", vec![0])]);
    let misc = [edid(b"MISC", ITEM_A, "ItemA", &[]), edid(b"MISC", ITEM_B, "ItemB", &[]), edid(b"MISC", ITEM_C, "ItemC", &[])].concat();
    // AIDT [documented: schemas/fnv ActorAIData]: aggression … mood, 3 unused, services
    // (Weapons 1 | Food 16), teaches, max training, assistance, aggro behaviour on, radius
    // 650.
    let aidt = [vec![0, 0, 50, 50, 0, 0, 0, 0], le(1 | 16), vec![0, 0, 0, 1], le(650)].concat();
    let npc = edid(b"NPC_", NPC, "TestNPC", &[sub(b"AIDT", aidt), sub(b"ZNAM", le(CSTY))]);
    let npc_player = record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]);
    let csty = edid(b"CSTY", CSTY, "TestStyle", &[]);
    // PERK DATA: trait, min level, ranks, playable, hidden.
    let perks = [edid(b"PERK", PERK_TRAIT, "TestTrait", &[sub(b"DATA", vec![1, 1, 1, 1, 0])]), edid(b"PERK", PERK, "TestPerk", &[sub(b"DATA", vec![0, 2, 1, 0, 0])])].concat();
    // ECZN DATA: owner, rank, minimum level, flags (Never Resets), unused.
    let eczn = edid(b"ECZN", ECZN, "TestZone", &[sub(b"DATA", [le(0), vec![0, 5, 1, 0]].concat())]);
    let glob = edid(b"GLOB", GLOB, "TestGlobal", &[sub(b"FNAM", vec![b'f']), sub(b"FLTV", f32b(0.0))]);
    let lvli = edid(b"LVLI", LVLI, "TestList", &[sub(b"LVLD", vec![10]), sub(b"LVLF", vec![0]), sub(b"LVLG", le(GLOB))]);
    let acti = edid(b"ACTI", ACTI, "TestActivator", &[sub(b"RNAM", le(TACT))]);
    let tact = edid(b"TACT", TACT, "TestStation", &[]);
    let soun = edid(b"SOUN", SOUN, "TestSound", &[]);
    // TERM DNAM: difficulty 3 (Hard), flags, server, unused; PNAM the password note.
    let term = edid(b"TERM", TERM, "TestTerminal", &[sub(b"DNAM", vec![3, 0, 0, 0]), sub(b"PNAM", le(NOTE))]);
    let notes = [
        edid(b"NOTE", NOTE, "TestNote", &[sub(b"DATA", vec![0])]),
        edid(b"NOTE", NOTE2, "TestNote2", &[sub(b"DATA", vec![0])]),
    ]
    .concat();
    let lscr = edid(b"LSCR", LSCR, "TestScreen", &[sub(b"WMI1", le(LSCT))]);
    let lsct = [edid(b"LSCT", LSCT, "TestType", &[]), edid(b"LSCT", LSCT2, "TestType2", &[])].concat();
    // HAIR DATA flags: Playable (1).
    let hair = edid(b"HAIR", HAIR, "TestHair", &[sub(b"DATA", vec![1])]);
    // LIGH DATA: time, radius 200, colour, flags, falloff, FOV, value, weight.
    let ligh = edid(b"LIGH", LIGH, "TestLight", &[sub(b"DATA", [le(0), le(200), vec![255, 255, 255, 0], le(0), f32b(1.0), f32b(90.0), le(0), f32b(0.0)].concat())]);
    let term_ref = edid(b"REFR", TERM_REF, "TestTerminalRef", &[sub(b"NAME", le(TERM)), sub(b"DATA", vec![0; 24])]);
    let light_ref = edid(b"REFR", LIGHT_REF, "TestLightRef", &[sub(b"NAME", le(LIGH)), sub(b"DATA", vec![0; 24])]);
    let cell = [edid(b"CELL", ROOM, "TestRoom", &[sub(b"DATA", vec![1])]), group(ROOM.to_le_bytes(), 6, &group(ROOM.to_le_bytes(), 9, &[term_ref, light_ref].concat()))].concat();
    let cells = top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell)));
    [
        tes4(),
        top(b"GLOB", &glob),
        top(b"SOUN", &soun),
        top(b"NOTE", &notes),
        top(b"MISC", &misc),
        top(b"LIGH", &ligh),
        top(b"HAIR", &hair),
        top(b"CSTY", &csty),
        top(b"NPC_", &[npc_player, npc].concat()),
        top(b"ACTI", &acti),
        top(b"TACT", &tact),
        top(b"TERM", &term),
        top(b"LVLI", &lvli),
        top(b"PERK", &perks),
        top(b"CSNO", &csno),
        top(b"CHAL", &chal),
        top(b"RCCT", &rcct),
        top(b"RCPE", &[rcpe, rcpe2].concat()),
        top(b"ECZN", &eczn),
        top(b"LSCT", &lsct),
        top(b"LSCR", &lscr),
        cells,
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Lutana.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() })
}

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(g: &mut Game, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(x) => x,
        XVal::Ref(r) => f64::from(r),
        v => panic!("{expr}: {v:?}"),
    }
}

/// An array's entries as FormIDs.
fn forms(g: &mut Game, expr: &str) -> Vec<u32> {
    let at = |i: &str| format!("array_var a
let a := {expr}
SetFunctionValue {i}");
    let len = match q(g, &at("(ar_Size a)")) {
        XVal::Num(x) => x as usize,
        v => panic!("{expr}: {v:?}"),
    };
    (0..len)
        .map(|i| match q(g, &at(&format!("(a[{i}])"))) {
            XVal::Ref(r) => r,
            XVal::Num(x) => x as u32,
            v => panic!("{expr}[{i}]: {v:?}"),
        })
        .collect()
}

#[test]
fn challenges_read_and_set_their_record() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetChallengeType TestChal"), 2.0);
    assert_eq!(n(&mut g, "GetChallengeThreshold TestChal"), 50.0);
    assert_eq!(n(&mut g, "GetChallengeInterval TestChal"), 10.0);
    assert_eq!(n(&mut g, "GetChallengeFlags TestChal"), 2.0);
    assert_eq!(n(&mut g, "GetChallengeForm1 TestChal"), f64::from(ITEM_A));
    assert_eq!(n(&mut g, "GetChallengeForm2 TestChal"), f64::from(ITEM_B));
    q(&mut g, "SetChallengeThreshold TestChal 7
SetChallengeInterval TestChal 2
SetChallengeFlags TestChal 5
SetChallengeValue1 TestChal 3
SetChallengeValue3 TestChal 9
SetChallengeForm1 TestChal ItemC");
    assert_eq!(n(&mut g, "GetChallengeThreshold TestChal"), 7.0);
    assert_eq!(n(&mut g, "GetChallengeInterval TestChal"), 2.0);
    assert_eq!(n(&mut g, "GetChallengeFlags TestChal"), 5.0);
    assert_eq!(n(&mut g, "GetChallengeValue1 TestChal"), 3.0);
    assert_eq!(n(&mut g, "GetChallengeValue2 TestChal"), 0.0);
    assert_eq!(n(&mut g, "GetChallengeValue3 TestChal"), 9.0);
    assert_eq!(n(&mut g, "GetChallengeForm1 TestChal"), f64::from(ITEM_C));
    // A negative threshold is kept and read back signed (lutana1 chal.setthreshold.neg).
    q(&mut g, "SetChallengeThreshold TestChal -4");
    assert_eq!(n(&mut g, "GetChallengeThreshold TestChal"), -4.0);
    // Not a challenge: 0, and nothing is made.
    assert_eq!(n(&mut g, "GetChallengeThreshold ItemA"), 0.0);
    q(&mut g, "SetChallengeThreshold ItemA 3");
    assert_eq!(n(&mut g, "GetChallengeThreshold ItemA"), 0.0);
}

#[test]
fn casinos_read_and_set_their_record() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetCasinoNumDecks TestCasino"), 3.0);
    assert_eq!(n(&mut g, "GetCasinoMaxWinnings TestCasino"), 9000.0);
    assert_eq!(n(&mut g, "GetCasinoBJPayout TestCasino"), 1.5);
    assert_eq!(n(&mut g, "GetCasinoShufflePercent TestCasino"), 0.5);
    assert_eq!(n(&mut g, "GetCasinoBJ17Stand TestCasino"), 0.0);
    assert_eq!(n(&mut g, "GetCasinoReelStops TestCasino 7"), 2.0);
    // A symbol outside 1-7: -1 (lutana1 csno.reel.8).
    assert_eq!(n(&mut g, "GetCasinoReelStops TestCasino 8"), -1.0);
    q(&mut g, "SetCasinoNumDecks TestCasino 6
SetCasinoMaxWinnings TestCasino 12000
SetCasinoBJPayout TestCasino 2.5
SetCasinoShufflePercent TestCasino 0.25
SetCasinoBJ17Stand TestCasino 1
SetCasinoReelStops TestCasino 7 3");
    assert_eq!(n(&mut g, "GetCasinoNumDecks TestCasino"), 6.0);
    assert_eq!(n(&mut g, "GetCasinoMaxWinnings TestCasino"), 12000.0);
    assert_eq!(n(&mut g, "GetCasinoBJPayout TestCasino"), 2.5);
    assert_eq!(n(&mut g, "GetCasinoShufflePercent TestCasino"), 0.25);
    assert_eq!(n(&mut g, "GetCasinoBJ17Stand TestCasino"), 1.0);
    assert_eq!(n(&mut g, "GetCasinoReelStops TestCasino 7"), 3.0);
    // The tables play to the changed max winnings.
    assert_eq!(fnv_game::casino::max_winnings(&g, FormId(CSNO), 0), 12000);
    // As given, 0 decks and a share above 1 too (lutana1 csno.setnumdecks.0,
    // csno.setshufflepercent.over).
    q(&mut g, "SetCasinoNumDecks TestCasino 0
SetCasinoShufflePercent TestCasino 1.5");
    assert_eq!(n(&mut g, "GetCasinoNumDecks TestCasino"), 0.0);
    assert_eq!(n(&mut g, "GetCasinoShufflePercent TestCasino"), 1.5);
}

#[test]
fn recipes_read_and_set_their_lists() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetRecipeRequiredSkill TestRecipe"), 44.0);
    assert_eq!(n(&mut g, "GetRecipeRequiredSkillLevel TestRecipe"), 30.0);
    assert_eq!(n(&mut g, "GetRecipeRequiredSkill TestRecipe2"), -1.0);
    assert_eq!(n(&mut g, "GetRecipeInputCount TestRecipe ItemA"), 2.0);
    assert_eq!(n(&mut g, "GetRecipeOutputCount TestRecipe ItemC"), 1.0);
    assert_eq!(forms(&mut g, "GetFormRecipes ItemA"), vec![RCPE, RCPE2]);
    assert_eq!(forms(&mut g, "GetFormRecipes ItemA TestCategory"), vec![RCPE]);
    assert_eq!(forms(&mut g, "GetFormRecipeOutputs ItemA"), vec![ITEM_C, ITEM_B]);
    q(&mut g, "SetRecipeRequiredSkill TestRecipe 40
SetRecipeRequiredSkillLevel TestRecipe 45
SetRecipeInputCount TestRecipe ItemA 4
AddRecipeInputForm TestRecipe ItemB 1
AddRecipeOutputForm TestRecipe ItemB 2
ReplaceRecipeOutputForm TestRecipe ItemC ItemA");
    assert_eq!(n(&mut g, "GetRecipeRequiredSkill TestRecipe"), 40.0);
    assert_eq!(n(&mut g, "GetRecipeRequiredSkillLevel TestRecipe"), 45.0);
    assert_eq!(n(&mut g, "GetRecipeInputCount TestRecipe ItemA"), 4.0);
    assert_eq!(n(&mut g, "GetRecipeInputCount TestRecipe ItemB"), 1.0);
    assert_eq!(n(&mut g, "GetRecipeOutputCount TestRecipe ItemA"), 1.0);
    assert_eq!(n(&mut g, "GetRecipeOutputCount TestRecipe ItemC"), 0.0);
    // GetRecipeInputForms and GetRecipeOutputForms read the change too; an added entry
    // goes first (lutana1 rcpe.addinput.order).
    assert_eq!(forms(&mut g, "GetRecipeInputForms TestRecipe"), vec![ITEM_B, ITEM_A]);
    assert_eq!(forms(&mut g, "GetRecipeOutputForms TestRecipe"), vec![ITEM_B, ITEM_A]);
    q(&mut g, "RemoveRecipeInputForm TestRecipe ItemA
RemoveRecipeOutputForm TestRecipe ItemB
SetRecipeOutputCount TestRecipe ItemA 0");
    assert_eq!(forms(&mut g, "GetRecipeInputForms TestRecipe"), vec![ITEM_B]);
    assert_eq!(forms(&mut g, "GetRecipeOutputForms TestRecipe"), vec![ITEM_A]);
    // A count of 0 is kept (lutana1 rcpe.setoutcount.0).
    assert_eq!(n(&mut g, "GetRecipeOutputCount TestRecipe ItemA"), 0.0);
    // An item added twice is listed twice, the later first; the count read and the entry
    // removed are the first (lutana1 rcpe.addinput).
    q(&mut g, "AddRecipeInputForm TestRecipe ItemC 3
AddRecipeInputForm TestRecipe ItemC 2");
    assert_eq!(n(&mut g, "GetRecipeInputCount TestRecipe ItemC"), 2.0);
    q(&mut g, "RemoveRecipeInputForm TestRecipe ItemC");
    assert_eq!(n(&mut g, "GetRecipeInputCount TestRecipe ItemC"), 3.0);
    q(&mut g, "RemoveRecipeInputForm TestRecipe ItemC");
    // Without a skill the level stays (lutana1 rcpe.setskill.neg).
    q(&mut g, "SetRecipeRequiredSkill TestRecipe -1");
    assert_eq!(n(&mut g, "GetRecipeRequiredSkill TestRecipe"), -1.0);
    assert_eq!(n(&mut g, "GetRecipeRequiredSkillLevel TestRecipe"), 45.0);
    // Wave 4 jip_02_recipe_add_neg_count & jip_03_recipe_set_neg_count [observed]:
    // negative counts are accepted by AddRecipeInputForm and stored verbatim by
    // SetRecipeInputCount.
    let before = forms(&mut g, "GetRecipeInputForms TestRecipe").len();
    q(&mut g, "AddRecipeInputForm TestRecipe ItemC -1");
    assert_eq!(forms(&mut g, "GetRecipeInputForms TestRecipe").len(), before + 1);
    assert_eq!(n(&mut g, "GetRecipeInputCount TestRecipe ItemC"), -1.0);
    q(&mut g, "SetRecipeInputCount TestRecipe ItemB -5");
    assert_eq!(n(&mut g, "GetRecipeInputCount TestRecipe ItemB"), -5.0);
    q(&mut g, "RemoveRecipeInputForm TestRecipe ItemC");
    // Wave 4D w4d_15_recipe_unlisted_item [observed]: setting count on an unlisted item
    // does not add it and leaves counts unchanged.
    let before_unlisted = forms(&mut g, "GetRecipeInputForms TestRecipe").len();
    q(&mut g, "SetRecipeInputCount TestRecipe ItemC 5");
    assert_eq!(forms(&mut g, "GetRecipeInputForms TestRecipe").len(), before_unlisted);
    assert_eq!(n(&mut g, "GetRecipeInputCount TestRecipe ItemC"), 0.0);
    // The recipe no longer takes ItemA.
    assert_eq!(forms(&mut g, "GetFormRecipes ItemA"), vec![RCPE2]);
}

#[test]
fn actor_ai_data_services_and_aggro() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetServiceFlag TestNPC 16"), 1.0);
    assert_eq!(n(&mut g, "GetServiceFlag TestNPC 2"), 0.0);
    q(&mut g, "SetServiceFlag TestNPC 2 1
SetServiceFlag TestNPC 16 0");
    assert_eq!(n(&mut g, "GetServiceFlag TestNPC 2"), 1.0);
    assert_eq!(n(&mut g, "GetServiceFlag TestNPC 16"), 0.0);
    assert_eq!(n(&mut g, "LNGetAggroRadius TestNPC"), 650.0);
    q(&mut g, "LNSetAggroRadius 1000 TestNPC");
    assert_eq!(n(&mut g, "LNGetAggroRadius TestNPC"), 1000.0);
    assert_eq!(n(&mut g, "GetCombatStyle TestNPC"), f64::from(CSTY));
}

#[test]
fn get_combat_style_of_a_base_follows_its_traits_template() {
    // Issue #2138: the combat style is on the Traits tab [documented: GECK wiki Traits Tab
    // - NPC], so a base form's GetCombatStyle reads its "Use Traits" template's ZNAM.
    const TEMPLATE: u32 = 0x930;
    const OTHER_STYLE: u32 = 0x931;
    for (traits, ai, style) in [(true, false, OTHER_STYLE), (false, true, CSTY)] {
        let mut f = fixture();
        let mut template = f.data.actors[&FormId(NPC)].clone();
        template.combat_style = Some(FormId(OTHER_STYLE));
        f.data.actors.insert(FormId(TEMPLATE), template);
        let npc = f.data.actors.get_mut(&FormId(NPC)).unwrap();
        npc.template = Some(FormId(TEMPLATE));
        npc.template_traits = traits;
        npc.template_ai = ai;
        let mut g = game(&f);
        assert_eq!(n(&mut g, "GetCombatStyle TestNPC"), f64::from(style), "traits {traits}, AI data {ai}");
    }
}

#[test]
fn record_getters_and_setters() {
    let f = fixture();
    let mut g = game(&f);
    // Perks.
    assert_eq!(n(&mut g, "IsTrait TestTrait"), 1.0);
    assert_eq!(n(&mut g, "IsTrait TestPerk"), 0.0);
    assert_eq!(n(&mut g, "LNIsPlayable TestTrait"), 1.0);
    assert_eq!(n(&mut g, "LNIsPlayable TestPerk"), 0.0);
    q(&mut g, "LNSetIsPlayable 1 TestPerk");
    assert_eq!(n(&mut g, "LNIsPlayable TestPerk"), 1.0);
    // Hair.
    assert_eq!(n(&mut g, "LNIsPlayable TestHair"), 1.0);
    q(&mut g, "LNSetIsPlayable 0 TestHair");
    assert_eq!(n(&mut g, "LNIsPlayable TestHair"), 0.0);
    // Zones: the flag reads back, and GetZoneMatchLevel sees it.
    assert_eq!(n(&mut g, "GetZoneFlag TestZone 1"), 1.0);
    assert_eq!(n(&mut g, "GetZoneFlag TestZone 2"), 0.0);
    q(&mut g, "SetZoneFlag TestZone 2 1
SetZoneFlag TestZone 1 0");
    assert_eq!(n(&mut g, "GetZoneFlag TestZone 2"), 1.0);
    assert_eq!(n(&mut g, "GetZoneFlag TestZone 1"), 0.0);
    assert_eq!(n(&mut g, "GetZoneMatchLevel TestZone"), 1.0);
    // Leveled lists.
    assert_eq!(n(&mut g, "GetChanceNoneGlobal TestList"), f64::from(GLOB));
    // Activators: the radio station, and the sound setters read by their getters.
    assert_eq!(n(&mut g, "GetActivatorRadioStation TestActivator"), f64::from(TACT));
    q(&mut g, "SetActivatorSoundActivate TestSound TestActivator
SetActivatorRadioStation 0 TestActivator");
    assert_eq!(n(&mut g, "GetActivatorSoundActivate TestActivator"), f64::from(SOUN));
    assert_eq!(n(&mut g, "GetActivatorRadioStation TestActivator"), 0.0);
    // Wave 6D-2 w6d2p5_02_activator_sound_null_noop: non-SOUN and explicit 0 are no-ops.
    q(&mut g, "SetActivatorSoundActivate ItemA TestActivator
SetActivatorSoundActivate 0 TestActivator");
    assert_eq!(n(&mut g, "GetActivatorSoundActivate TestActivator"), f64::from(SOUN));
    // Terminals.
    assert_eq!(n(&mut g, "GetTerminalLock TestTerminal"), 3.0);
    assert_eq!(n(&mut g, "GetPasswordNote TestTerminal"), f64::from(NOTE));
    q(&mut g, "SetPasswordNote 0 TestTerminal");
    assert_eq!(n(&mut g, "GetPasswordNote TestTerminal"), 0.0);
    // Load screens.
    assert_eq!(n(&mut g, "GetLoadScreenType TestScreen"), f64::from(LSCT));
    q(&mut g, "SetLoadScreenType TestScreen TestType2");
    assert_eq!(n(&mut g, "GetLoadScreenType TestScreen"), f64::from(LSCT2));
    // Lights: base LIGH returns DATA radius; placed Light ref without XRDS returns 0.0
    // (does not fall back to base LIGH radius) [observed: Wave 4
    // jip_06_radius_ref_fallback_base: base 200.000, placed ref 0.000].
    assert_eq!(n(&mut g, "GetRadius TestLight"), 200.0);
    assert_eq!(n(&mut g, "TestLightRef.GetRadius"), 0.0);
    q(&mut g, "TestLightRef.SetRadius 125");
    assert_eq!(n(&mut g, "TestLightRef.GetRadius"), 125.0);
    q(&mut g, "PlayerREF.SetRadius 350 TestLight");
    assert_eq!(n(&mut g, "GetRadius TestLight"), 350.0);
    // Form flags, as GetFlagsLow reads them.
    assert_eq!(n(&mut g, "GetFormFlag ItemA 1024"), 0.0);
    q(&mut g, "SetFormFlag ItemA 1024 1");
    assert_eq!(n(&mut g, "GetFormFlag ItemA 1024"), 1.0);
    assert_eq!(n(&mut g, "GetFormFlag ItemA 1280"), 1.0, "any bit in mask (1024 | 256) matches [observed: w6d2_32]");
    assert_eq!(n(&mut g, "GetFlagsLow ItemA") as u32 & 1024, 1024);
    q(&mut g, "SetFormFlag ItemA 1024 0");
    assert_eq!(n(&mut g, "GetFormFlag ItemA 1024"), 0.0);
}

#[test]
fn misc_getters() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetDefaultMessageTime"), 2.0);
    q(&mut g, "SetDefaultMessageTime 3.5");
    assert_eq!(n(&mut g, "GetDefaultMessageTime"), 3.5);
    // Kept as given (lutana1 misc.setdmt).
    q(&mut g, "SetDMT -1");
    assert_eq!(n(&mut g, "GetDefaultMessageTime"), -1.0);
    // Not registered by the installed JIP LN: its own text (lutana1 misc.mdt).
    assert_eq!(q(&mut g, "SetFunctionValue (GetMDT)"), XVal::Str("GetMDT".into()));
    assert_eq!(n(&mut g, "GetPCCanUsePowerArmor"), 0.0);
    q(&mut g, "SetPCCanUsePowerArmor 1");
    assert_eq!(n(&mut g, "GetPCCanUsePowerArmor"), 1.0);
    // AddNoteNS files the note without a corner message.
    q(&mut g, "AddNoteNS TestNote");
    assert!(g.player.notes.contains(&FormId(NOTE)));
    assert!(g.take_note_notices().is_empty());
    match q(&mut g, "string_var s
let s := GetTimeStamp
SetFunctionValue s") {
        XVal::Str(s) => assert!(s.len() == 8 && s.as_bytes()[2] == b'-' && s.as_bytes()[5] == b'-', "{s}"),
        v => panic!("{v:?}"),
    }
    // Nothing under the crosshair.
    assert_eq!(n(&mut g, "CrosshairRefInList TestList"), 0.0);
}

/// One GetUserTime field.
fn user_time(g: &mut Game, key: &str) -> f64 {
    n(g, &format!("(GetUserTime)[\"{key}\"]"))
}

#[test]
fn local_time_functions_read_the_computers_clock() {
    let f = fixture();
    let mut g = game(&f);
    // GetUserTime [documented: the computer's local time, a string map] and GetTimeStamp
    // ("HH-MM-SS") read between two readings of the clock; a minute turning between those
    // readings retries.
    for _ in 0..3 {
        let before = jiff::ZonedDateTime::now();
        let stamp = match q(&mut g, "string_var s
let s := GetTimeStamp
SetFunctionValue s") {
            XVal::Str(s) => s,
            v => panic!("{v:?}"),
        };
        let got: Vec<f64> = ["Year", "Month", "Day", "DayOfWeek", "Hour", "Minute"].iter().map(|k| user_time(&mut g, k)).collect();
        let (second, milli) = (user_time(&mut g, "Second"), user_time(&mut g, "Millisecond"));
        let after = jiff::ZonedDateTime::now();
        if before.strftime("%Y%m%d%H%M").to_string() != after.strftime("%Y%m%d%H%M").to_string() {
            continue;
        }
        let want = [before.year(), i16::from(before.month()), i16::from(before.day()), i16::from(before.weekday().to_sunday_zero_offset()), i16::from(before.hour()), i16::from(before.minute())];
        assert_eq!(got, want.map(f64::from).to_vec());
        assert!((f64::from(before.second())..=f64::from(after.second())).contains(&second), "{second}");
        assert!((0.0..1000.0).contains(&milli), "{milli}");
        let (b, a) = (before.strftime("%H-%M-%S").to_string(), after.strftime("%H-%M-%S").to_string());
        assert!(b <= stamp && stamp <= a, "{b} {stamp} {a}");
        return;
    }
    panic!("the clock turned a minute on every try");
}

/// Session `SetPasswordNote` overrides must reach `GetPasswordNote`, `PlayerHasKey` on
/// placed `TERM` references, and `terminal::has_password` (`still_locked`, `opening`,
/// `opens_by_password`), ignore non-terminals, reset on fresh-run restore, and survive
/// same-run `put_session` (#2272), while base `TERM` forms return `0` from `PlayerHasKey`
/// [observed: b3_lut_06_player_has_key_terminal_note, w4d_25_lsct_rgb_nan_and_password_key].
#[test]
fn set_password_note_reaches_terminal_opening_prompt_player_has_key_and_session() {
    use fnv_game::hacking::{Difficulty, Refusal};
    use fnv_game::terminal::{self, Opening};

    let f = fixture();
    let mut g = game(&f);
    g.tweaks.terminal_password_prompt = true;

    // 1. Baseline authored password note + non-TERM guard.
    assert_eq!(n(&mut g, "GetPasswordNote TestTerminal"), f64::from(NOTE));
    assert_eq!(n(&mut g, "TestTerminalRef.GetPasswordNote"), f64::from(NOTE));
    q(&mut g, "SetPasswordNote TestNote ItemA");
    assert_eq!(n(&mut g, "GetPasswordNote ItemA"), 0.0, "non-TERM form must stay 0");
    assert_eq!(n(&mut g, "TestTerminalRef.PlayerHasKey"), 0.0);
    assert_eq!(n(&mut g, "TestTerminal.PlayerHasKey"), 0.0);
    assert!(!terminal::opens_by_password(&mut g, FormId(TERM_REF), FormId(TERM)));
    assert_eq!(terminal::still_locked(&mut g, FormId(TERM_REF), FormId(TERM)), Some(Difficulty::Hard));
    assert_eq!(terminal::opening(&mut g, FormId(TERM_REF), FormId(TERM), 0.0), Some(Opening::Refused(Refusal::Science { needed: 75.0 })));

    // 2. Filing the authored note unlocks the terminal and flips placed TERM PlayerHasKey 0 -> 1,
    // while calling PlayerHasKey on the base TERM form still returns 0.
    q(&mut g, "AddNoteNS TestNote");
    assert_eq!(n(&mut g, "TestTerminalRef.PlayerHasKey"), 1.0);
    assert_eq!(n(&mut g, "TestTerminal.PlayerHasKey"), 0.0);
    assert!(terminal::opens_by_password(&mut g, FormId(TERM_REF), FormId(TERM)));
    assert_eq!(terminal::still_locked(&mut g, FormId(TERM_REF), FormId(TERM)), None);

    // 3. Clearing the password note locks the terminal back out even while TestNote is held.
    q(&mut g, "SetPasswordNote 0 TestTerminal");
    assert_eq!(n(&mut g, "GetPasswordNote TestTerminal"), 0.0);
    assert_eq!(n(&mut g, "TestTerminalRef.GetPasswordNote"), 0.0);
    assert_eq!(n(&mut g, "TestTerminalRef.PlayerHasKey"), 0.0);
    assert!(!terminal::opens_by_password(&mut g, FormId(TERM_REF), FormId(TERM)));
    assert_eq!(terminal::still_locked(&mut g, FormId(TERM_REF), FormId(TERM)), Some(Difficulty::Hard));
    assert_eq!(terminal::opening(&mut g, FormId(TERM_REF), FormId(TERM), 0.0), Some(Opening::Refused(Refusal::Science { needed: 75.0 })));

    // 4. Reassigning via reference receiver to TestNote2: TestNote alone no longer opens it.
    q(&mut g, "TestTerminalRef.SetPasswordNote TestNote2");
    assert_eq!(n(&mut g, "GetPasswordNote TestTerminal"), f64::from(NOTE2));
    assert_eq!(n(&mut g, "TestTerminalRef.GetPasswordNote"), f64::from(NOTE2));
    assert_eq!(n(&mut g, "TestTerminalRef.PlayerHasKey"), 0.0);
    assert!(!terminal::opens_by_password(&mut g, FormId(TERM_REF), FormId(TERM)));
    assert_eq!(terminal::still_locked(&mut g, FormId(TERM_REF), FormId(TERM)), Some(Difficulty::Hard));

    // 5a. Fresh-run restore (without Session transfer) reverts to authored TestNote (which player holds).
    let snap: fnv_game::game::GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.checked_snapshot().unwrap()).unwrap()).unwrap();
    let fresh_world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    let mut fresh = Game::restore(&f.data, fresh_world, GameConfig::default(), &snap).unwrap();
    fresh.tweaks.terminal_password_prompt = true;
    assert_eq!(n(&mut fresh, "GetPasswordNote TestTerminal"), f64::from(NOTE));
    assert_eq!(n(&mut fresh, "TestTerminalRef.PlayerHasKey"), 1.0);
    assert!(terminal::opens_by_password(&mut fresh, FormId(TERM_REF), FormId(TERM)));
    assert_eq!(terminal::still_locked(&mut fresh, FormId(TERM_REF), FormId(TERM)), None);

    // 5b. Same-run restore with put_session preserves SetPasswordNote TestNote2 across load.
    let session = g.take_session();
    let world_session = g.world.take_session();
    let mut same_world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    same_world.put_session(world_session);
    let mut same_run = Game::restore(&f.data, same_world, GameConfig::default(), &snap).unwrap();
    same_run.put_session(session);
    same_run.tweaks.terminal_password_prompt = true;
    assert_eq!(n(&mut same_run, "GetPasswordNote TestTerminal"), f64::from(NOTE2));
    assert_eq!(n(&mut same_run, "TestTerminalRef.PlayerHasKey"), 0.0);
    assert!(!terminal::opens_by_password(&mut same_run, FormId(TERM_REF), FormId(TERM)));
    assert_eq!(terminal::still_locked(&mut same_run, FormId(TERM_REF), FormId(TERM)), Some(Difficulty::Hard));
    // Filing TestNote2 now opens the terminal straight to its menu without hacking and flips PlayerHasKey to 1.
    q(&mut same_run, "AddNoteNS TestNote2");
    assert_eq!(n(&mut same_run, "TestTerminalRef.PlayerHasKey"), 1.0);
    assert_eq!(n(&mut same_run, "TestTerminal.PlayerHasKey"), 0.0);
    assert!(terminal::opens_by_password(&mut same_run, FormId(TERM_REF), FormId(TERM)));
    assert_eq!(terminal::opening(&mut same_run, FormId(TERM_REF), FormId(TERM), 0.0), Some(Opening::Menu));
}

/// Wave 4D `w4d_21_cell_water_height_ext` [observed]: interior XCLW reads directly,
/// a loaded exterior cell without XCLW returns `f32::MAX` (NOT worldspace DNAM), and an
/// unloaded exterior cell (even with explicit XCLW) returns `0.0`.
#[test]
fn cell_water_height_loaded_vs_unloaded_exterior_and_interior() {
    const WRLD: u32 = 0x940;
    const PCELL: u32 = 0x941;
    const EXT_NO_XCLW: u32 = 0x942;
    const EXT_WATER: u32 = 0x943;
    const INT_ROOM: u32 = 0x944;

    let npc_player = record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]);
    // WRLD DNAM: default land height (-2048.0), default water height (-2300.0).
    let wrld_rec = edid(b"WRLD", WRLD, "TestWorld", &[sub(b"DNAM", [f32b(-2048.0), f32b(-2300.0)].concat())]);
    let pcell = edid(b"CELL", PCELL, "TestPersistent", &[sub(b"DATA", vec![0])]);
    let ext_no_xclw = edid(b"CELL", EXT_NO_XCLW, "TestExtNoXclw", &[sub(b"DATA", vec![0]), sub(b"XCLC", [0i32.to_le_bytes(), 0i32.to_le_bytes(), 0i32.to_le_bytes()].concat())]);
    let ext_water = edid(
        b"CELL",
        EXT_WATER,
        "TestExtWater",
        &[sub(b"DATA", vec![2]), sub(b"XCLC", [1i32.to_le_bytes(), 0i32.to_le_bytes(), 0i32.to_le_bytes()].concat()), sub(b"XCLW", f32b(3000.0))],
    );
    let block = group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &[ext_no_xclw, ext_water].concat()));
    let wrld_top = top(b"WRLD", &[wrld_rec, group(WRLD.to_le_bytes(), 1, &[pcell, block].concat())].concat());
    let int_room = edid(b"CELL", INT_ROOM, "TestIntRoom", &[sub(b"DATA", vec![1]), sub(b"XCLW", f32b(-2147483648.0))]);
    let cell_top = top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &int_room)));
    let bytes = [tes4(), top(b"NPC_", &npc_player), wrld_top, cell_top].concat();

    let plugins = vec![Plugin::parse(&bytes, "WaterHeight.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    let world = World::new(&forms, &refs, SimConfig::default());
    let mut g = Game::new(&data, world, GameConfig { strict: true, ..GameConfig::default() });

    // Interior cell reads XCLW sentinel directly.
    assert_eq!(n(&mut g, "GetCellWaterHeight TestIntRoom"), f64::from(-2147483648.0_f32));
    // Unloaded exterior cells (with or without XCLW) return 0.
    assert_eq!(n(&mut g, "GetCellWaterHeight TestExtNoXclw"), 0.0);
    assert_eq!(n(&mut g, "GetCellWaterHeight TestExtWater"), 0.0);
    // Loaded exterior cell without XCLW returns f32::MAX (not WRLD DNAM -2300.0).
    g.loaded_cells.insert(FormId(EXT_NO_XCLW));
    assert_eq!(n(&mut g, "GetCellWaterHeight TestExtNoXclw"), f64::from(f32::MAX));
    // Loaded exterior cell with XCLW returns its XCLW (3000.0).
    g.loaded_cells.insert(FormId(EXT_WATER));
    assert_eq!(n(&mut g, "GetCellWaterHeight TestExtWater"), 3000.0);
    // SetCellWaterHeight overrides the loaded exterior cell.
    q(&mut g, "SetCellWaterHeight 1234.5 TestExtNoXclw");
    assert_eq!(n(&mut g, "GetCellWaterHeight TestExtNoXclw"), 1234.5);
}
