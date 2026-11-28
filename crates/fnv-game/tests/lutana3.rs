//! Lutana NVSE batch 3 (#1009; docs/spec-notes/ext-lutana.md §10) on a synthetic plugin
//! written for the test (no game data, made-up names): the record setters read back,
//! CopyFaceGenFrom and its undo, perk ranks and perk entries, SetBaseForm, the cell
//! events, GetCellRefs, SetVariableFloat and BaseAddItemHealth.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const ACTI: u32 = 0x800;
const ANIO: u32 = 0x801;
const IDLE_A: u32 = 0x802;
const IDLE_B: u32 = 0x803;
const LSCT: u32 = 0x804;
const LSCR: u32 = 0x805;
const RACE_A: u32 = 0x806;
const RACE_B: u32 = 0x807;
const HAIR: u32 = 0x808;
const NPC_A: u32 = 0x809;
const NPC_B: u32 = 0x80A;
const PERK: u32 = 0x80B;
const SPELL_A: u32 = 0x80C;
const SPELL_B: u32 = 0x80D;
const CONT: u32 = 0x80E;
const MISC: u32 = 0x80F;
const STAT: u32 = 0x810;
const ROOM: u32 = 0x900;
const HALL: u32 = 0x901;
const ACTI_REF: u32 = 0x910;
const NPC_REF: u32 = 0x911;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn npc(id: u32, edid: &str, race: u32, female: bool, hair: Option<u32>) -> Vec<u8> {
    let flags: u32 = u32::from(female);
    let acbs = [&flags.to_le_bytes()[..], &[0; 4], &1u16.to_le_bytes(), &[0; 6], &0f32.to_le_bytes(), &[0; 4]].concat();
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"ACBS", acbs), sub(b"RNAM", race.to_le_bytes().to_vec())];
    if let Some(h) = hair {
        subs.push(sub(b"HNAM", h.to_le_bytes().to_vec()));
    }
    record(b"NPC_", 0, id, &subs)
}

fn fixture() -> Fixture {
    let acti = record(b"ACTI", 0, ACTI, &[sub(b"EDID", z("TestLever")), sub(b"XATO", z("Pull"))]);
    let idles = [record(b"IDLE", 0, IDLE_A, &[sub(b"EDID", z("TestIdleA"))]), record(b"IDLE", 0, IDLE_B, &[sub(b"EDID", z("TestIdleB"))])].concat();
    let anio = record(b"ANIO", 0, ANIO, &[sub(b"EDID", z("TestAnimObject")), sub(b"DATA", IDLE_A.to_le_bytes().to_vec())]);
    // LSCT DATA: type, x, y, width, height, orientation, font 1 and its r g b, alignment,
    // 20 unknown, font 2 and its r g b, 4 unknown, stats [documented: schemas/fnv LSCT].
    let f = |v: f32| v.to_le_bytes().to_vec();
    let u = |v: u32| v.to_le_bytes().to_vec();
    let lsct_data = [u(3), u(0), u(0), u(100), u(50), f(0.0), u(6), f(255.0), f(180.0), f(53.0), u(2), vec![0; 20], u(6), f(255.0), f(255.0), f(255.0), vec![0; 4], u(0)].concat();
    let lsct = record(b"LSCT", 0, LSCT, &[sub(b"EDID", z("TestLoadType")), sub(b"DATA", lsct_data)]);
    let lscr = record(b"LSCR", 0, LSCR, &[sub(b"EDID", z("TestLoadScreen")), sub(b"ICON", z("interface\\loading\\a.dds"))]);
    let races = [record(b"RACE", 0, RACE_A, &[sub(b"EDID", z("TestRaceA"))]), record(b"RACE", 0, RACE_B, &[sub(b"EDID", z("TestRaceB"))])].concat();
    let hair = record(b"HAIR", 0, HAIR, &[sub(b"EDID", z("TestHair"))]);
    let npcs = [npc(0x7, "Player", RACE_A, false, None), npc(NPC_A, "TestNpcA", RACE_A, false, None), npc(NPC_B, "TestNpcB", RACE_B, true, Some(HAIR))].concat();
    let spells = [record(b"SPEL", 0, SPELL_A, &[sub(b"EDID", z("TestSpellA"))]), record(b"SPEL", 0, SPELL_B, &[sub(b"EDID", z("TestSpellB"))])].concat();
    // PERK DATA: trait, min level, ranks 3, playable, hidden; an Ability entry, then an
    // entry point entry (point 27, function 9, 0 tabs) with a button label.
    let perk = record(
        b"PERK",
        0,
        PERK,
        &[
            sub(b"EDID", z("TestPerk")),
            sub(b"DATA", vec![0, 1, 3, 1, 0]),
            sub(b"PRKE", vec![1, 0, 0]),
            sub(b"DATA", SPELL_A.to_le_bytes().to_vec()),
            sub(b"PRKF", Vec::new()),
            sub(b"PRKE", vec![2, 0, 0]),
            sub(b"DATA", vec![27, 9, 0]),
            sub(b"EPFT", vec![4]),
            sub(b"EPF2", z("Poke")),
            sub(b"EPF3", vec![0, 0]),
            sub(b"PRKF", Vec::new()),
        ],
    );
    let cont = record(b"CONT", 0, CONT, &[sub(b"EDID", z("TestChest"))]);
    let misc = record(b"MISC", 0, MISC, &[sub(b"EDID", z("TestJunk"))]);
    let stat = record(b"STAT", 0, STAT, &[sub(b"EDID", z("TestRock"))]);
    let room = record(b"CELL", 0, ROOM, &[sub(b"EDID", z("TestRoom")), sub(b"DATA", vec![1])]);
    let hall = record(b"CELL", 0, HALL, &[sub(b"EDID", z("TestHall")), sub(b"DATA", vec![1])]);
    let acti_ref = record(b"REFR", 0, ACTI_REF, &[sub(b"EDID", z("TestLeverRef")), sub(b"NAME", ACTI.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let npc_ref = record(b"ACHR", 0, NPC_REF, &[sub(b"EDID", z("TestNpcRef")), sub(b"NAME", NPC_A.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let children = group(ROOM.to_le_bytes(), 6, &group(ROOM.to_le_bytes(), 9, &[acti_ref, npc_ref].concat()));
    let bytes = [
        tes4(),
        top(b"ACTI", &acti),
        top(b"IDLE", &idles),
        top(b"ANIO", &anio),
        top(b"LSCT", &lsct),
        top(b"LSCR", &lscr),
        top(b"RACE", &races),
        top(b"HAIR", &hair),
        top(b"SPEL", &spells),
        top(b"PERK", &perk),
        top(b"CONT", &cont),
        top(b"MISC", &misc),
        top(b"STAT", &stat),
        top(b"NPC_", &npcs),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[room, children, hall].concat()))),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "Lutana3Test.esm".into()).unwrap()];
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
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() });
    g.player_entered(Some(FormId(ROOM)));
    g
}

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("array_var aR\nstring_var sOut\nBegin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(g: &mut Game, expr: &str) -> f64 {
    let body = if expr.contains('\n') { expr.to_string() } else { format!("SetFunctionValue ({expr})") };
    match q(g, &body) {
        XVal::Num(x) => x,
        XVal::Ref(r) => f64::from(r),
        v => panic!("{expr}: {v:?}"),
    }
}

fn s(g: &mut Game, expr: &str) -> String {
    match q(g, &format!("let sOut := {expr}\nSetFunctionValue sOut")) {
        XVal::Str(s) => s,
        v => panic!("{expr}: {v:?}"),
    }
}

#[test]
fn record_setters_read_back() {
    let f = fixture();
    let mut g = game(&f);
    // The activator prompt, by form and by reference.
    assert_eq!(s(&mut g, "GetActivatorPrompt TestLever"), "Pull");
    q(&mut g, "TestLeverRef.SetActivatorPrompt \"Yank\"");
    assert_eq!(s(&mut g, "TestLeverRef.GetActivatorPrompt"), "Yank");
    // The Anim Object's idle; a form that is no idle is refused.
    assert_eq!(n(&mut g, "GetAnimObjectIdle TestAnimObject"), f64::from(IDLE_A));
    q(&mut g, "SetAnimObjectIdle TestAnimObject TestRock\nSetAnimObjectIdle TestAnimObject TestIdleB");
    assert_eq!(n(&mut g, "GetAnimObjectIdle TestAnimObject"), f64::from(IDLE_B));
    // The loading screen type's text colour [documented: the page's 255 180 53];
    // raw unclamped f32 floats (including NaN) stored verbatim
    // [observed: jip_09_lsct_text_rgb_clamp, w4d_25_lsct_rgb_nan_and_password_key].
    assert_eq!(n(&mut g, "let aR := GetLoadScreenTypeTextRGB TestLoadType\nSetFunctionValue aR[\"r\"] * 1000000 + aR[\"g\"] * 1000 + aR[\"b\"]"), 255_180_053.0);
    q(&mut g, "SetLoadScreenTypeTextRGB TestLoadType 59.3 181 300");
    assert_eq!(n(&mut g, "let aR := GetLoadScreenTypeTextRGB TestLoadType\nSetFunctionValue aR[\"b\"]"), 300.0);
    assert!((n(&mut g, "let aR := GetLoadScreenTypeTextRGB TestLoadType\nSetFunctionValue aR[\"r\"]") - 59.3).abs() < 1e-4);
    q(&mut g, "SetLoadScreenTypeTextRGB TestLoadType -10 100 50");
    assert_eq!(n(&mut g, "let aR := GetLoadScreenTypeTextRGB TestLoadType\nSetFunctionValue aR[\"r\"]"), -10.0);
    g.run_query("float fNan\nBegin Function { }\nlet fNan := Log -1.0\nSetLoadScreenTypeTextRGB TestLoadType fNan 123.5 45\nEnd\n", RunCtx::default()).unwrap();
    assert!(n(&mut g, "let aR := GetLoadScreenTypeTextRGB TestLoadType\nSetFunctionValue aR[\"r\"]").is_nan(), "NaN must be stored verbatim");
    assert_eq!(n(&mut g, "let aR := GetLoadScreenTypeTextRGB TestLoadType\nSetFunctionValue aR[\"g\"]"), 123.5);
    assert_eq!(n(&mut g, "let aR := GetLoadScreenTypeTextRGB TestLoadType\nSetFunctionValue aR[\"b\"]"), 45.0);
    q(&mut g, "SetLoadScreenTexture TestLoadScreen \"interface\\loading\\b.dds\"");
    // Race, template and template flag on the base.
    q(&mut g, "SetRace TestNpcA TestRaceB");
    assert_eq!(n(&mut g, "GetRace TestNpcA"), f64::from(RACE_B));
    q(&mut g, "TestNpcRef.LNSetActorTemplate TestNpcB");
    assert_eq!(n(&mut g, "GetActorTemplate TestNpcA"), f64::from(NPC_B));
}

#[test]
fn copy_face_and_undo() {
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "TestNpcRef.CopyFaceGenFrom TestNpcB");
    // The source's race, sex and face, on the actor; drawn again.
    assert_eq!(n(&mut g, "TestNpcRef.GetIsRace TestRaceB"), 1.0);
    assert_eq!(n(&mut g, "TestNpcRef.GetIsSex Female"), 1.0);
    assert_eq!(g.extra.get(&FormId(NPC_REF)).map_or(0, |e| e.face_matches.len()), 1);
    assert!(!g.take_appearance_changes().is_empty());
    // Undone: as before.
    q(&mut g, "TestNpcRef.CopyFaceGenFrom");
    assert_eq!(n(&mut g, "TestNpcRef.GetIsRace TestRaceA"), 1.0);
    assert_eq!(n(&mut g, "TestNpcRef.GetIsSex Female"), 0.0);
    assert_eq!(g.extra.get(&FormId(NPC_REF)).map_or(0, |e| e.face_matches.len()), 0);
}

#[test]
fn perk_ranks_and_entries() {
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "PlayerREF.SetPerkRank TestPerk 2");
    assert_eq!(n(&mut g, "PlayerREF.GetPerkRank TestPerk"), 2.0);
    // Cut to the perk's 3 ranks; 0 and negative leave the rank unchanged
    // [observed: lu3 perkrank, b3_lut_13_set_perk_rank_clamp].
    q(&mut g, "PlayerREF.SetPerkRank TestPerk 9");
    assert_eq!(n(&mut g, "PlayerREF.GetPerkRank TestPerk"), 3.0);
    q(&mut g, "PlayerREF.SetPerkRank TestPerk 1");
    assert_eq!(n(&mut g, "PlayerREF.GetPerkRank TestPerk"), 1.0);
    q(&mut g, "PlayerREF.SetPerkRank TestPerk 0 0");
    assert_eq!(n(&mut g, "PlayerREF.GetPerkRank TestPerk"), 1.0);
    q(&mut g, "PlayerREF.SetPerkRank TestPerk -1 0");
    assert_eq!(n(&mut g, "PlayerREF.GetPerkRank TestPerk"), 1.0);
    // An NPC holds it at that rank; 0 leaves it unchanged.
    q(&mut g, "TestNpcRef.SetPerkRank TestPerk 2");
    assert_eq!(n(&mut g, "TestNpcRef.HasPerk TestPerk"), 1.0);
    q(&mut g, "TestNpcRef.SetPerkRank TestPerk 0");
    assert_eq!(n(&mut g, "TestNpcRef.HasPerk TestPerk"), 1.0);
    assert_eq!(g.extra.get(&FormId(NPC_REF)).and_then(|e| e.perks.get(&FormId(PERK))), Some(&2));
    // The entries, changed for the session. Both are rank 0, priority 0: the ability
    // before the entry point, as Built to Last's [observed: L5.ext rerun, #2040].
    assert_eq!(n(&mut g, "GetNthPerkEntryForm TestPerk 0"), f64::from(SPELL_A));
    assert_eq!(n(&mut g, "GetNthPerkEntryForm TestPerk 1"), 0.0);
    q(&mut g, "SetNthPerkEntryForm TestPerk 0 TestSpellB");
    assert_eq!(n(&mut g, "GetNthPerkEntryForm TestPerk 0"), f64::from(SPELL_B));
    assert_eq!(n(&mut g, "GetNthPerkEntryFunction TestPerk 1"), 27.0);
    q(&mut g, "SetNthPerkEntryFunction TestPerk 1 7");
    assert_eq!(n(&mut g, "GetNthPerkEntryFunction TestPerk 1"), 7.0);
    assert_eq!(s(&mut g, "GetNthPerkEntryString TestPerk 1"), "Poke");
    q(&mut g, "SetNthPerkEntryString TestPerk 1 \"Prod\"");
    assert_eq!(s(&mut g, "GetNthPerkEntryString TestPerk 1"), "Prod");
}

#[test]
fn world_and_deprecated() {
    let f = fixture();
    let mut g = game(&f);
    // GetCellRefs as GetRefsInCell (type 0 every one) in the player's cell.
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(ROOM));
    let in_room = n(&mut g, "let aR := GetCellRefs 0 0 0 TestRoom\nSetFunctionValue (Ar_Size aR)");
    assert!(in_room >= 2.0, "{in_room}");
    // A base swap.
    q(&mut g, "TestLeverRef.SetBaseForm TestRock");
    assert_eq!(n(&mut g, "TestLeverRef.GetBaseObject"), f64::from(STAT));
    // No regions here.
    assert_eq!(n(&mut g, "let aR := GetPlayerRegions\nSetFunctionValue (Ar_Size aR)"), 0.0);
    // The base container's unit at half health.
    q(&mut g, "BaseAddItemHealth TestJunk 2 0.5 TestChest");
    let entries = g.world.base_entries(FormId(CONT)).unwrap();
    assert_eq!(entries.iter().map(|e| (e.item, e.count, e.extra.condition)).collect::<Vec<_>>(), vec![(FormId(MISC), 2, Some(0.5))]);
    // The event names called as functions give 2 and 3 [observed: lu3 events].
    assert_eq!(n(&mut g, "OnCellEnter") * 10.0 + n(&mut g, "OnCellExit"), 23.0);
}

#[test]
fn cell_events_follow_the_player() {
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "SetEventHandler \"OnCellEnter\" (Begin Function {ref rCell}\nPlayerREF.AuxVarSetRef \"*enter\" rCell\nEnd)\nSetEventHandler \"OnCellExit\" (Begin Function {ref rCell}\nPlayerREF.AuxVarSetRef \"*exit\" rCell\nEnd)");
    // The first frame in a cell sends nothing.
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(ROOM));
    g.tick();
    assert_eq!(n(&mut g, "PlayerREF.AuxVarGetRef \"*enter\""), 0.0);
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(HALL));
    g.tick();
    assert_eq!(n(&mut g, "PlayerREF.AuxVarGetRef \"*enter\""), f64::from(HALL));
    assert_eq!(n(&mut g, "PlayerREF.AuxVarGetRef \"*exit\""), f64::from(ROOM));
}

/// The co-op perks are offered at a level-up only in a co-op game: a single player game
/// lists what the original does (`fnv_game::coop_perks`).
#[test]
fn the_co_op_perks_are_offered_only_in_a_co_op_game() {
    let mut f = fixture();
    fnv_game::coop_perks::add(&mut f.data);
    let mut g = game(&f);
    let coop = [fnv_game::coop_perks::HEALING_HANDS, fnv_game::coop_perks::QUACK_SURGEON];
    let sheet = fnv_game::levelup::PerkSheet::of_player(&mut g, 10);
    assert!(sheet.perks.contains(&FormId(PERK)), "the plugin's perk is offered");
    assert!(coop.iter().all(|p| !sheet.perks.contains(p)), "no co-op perk out of a session");
    g.coop_session = true;
    let sheet = fnv_game::levelup::PerkSheet::of_player(&mut g, 10);
    assert!(coop.iter().all(|p| sheet.perks.contains(p)), "both in a session");
}

/// Session `SetPerkLevel`, `SetPerkFlag` (0 trait, 1 playable, 2 hidden), and
/// `LNSetIsPlayable` on PERK forms must reach `PerkSheet::of_player`, `TraitSheet::of_player`,
/// `chargen::requirement_line`, ITR `GetPerkEligibility`, `GetAllPerks`, and
/// `revision::keeps_perk`, reset on fresh-run restore, and survive same-run `put_session`
/// (#2272).
#[test]
fn perk_level_and_flag_overrides_reach_consumers_and_session() {
    let f = fixture();
    let mut g = game(&f);

    // 1. Baseline defaults + non-PERK / out-of-range flag guards.
    assert_eq!(n(&mut g, "GetPerkLevel TestPerk"), 1.0);
    assert_eq!(n(&mut g, "GetPerkFlag TestPerk 0"), 0.0);
    assert_eq!(n(&mut g, "GetPerkFlag TestPerk 1"), 1.0);
    assert_eq!(n(&mut g, "GetPerkFlag TestPerk 2"), 0.0);
    assert_eq!(n(&mut g, "IsTrait TestPerk"), 0.0);
    assert_eq!(n(&mut g, "LNIsPlayable TestPerk"), 1.0);
    assert!(fnv_game::levelup::PerkSheet::of_player(&mut g, 1).can_take(FormId(PERK)));
    assert!(!fnv_game::chargen::TraitSheet::of_player(&g).traits.contains(&FormId(PERK)));
    assert_eq!(fnv_game::chargen::requirement_line(&g, FormId(PERK)), "--");

    q(&mut g, "SetPerkLevel TestJunk 8\nSetPerkFlag TestJunk 1 1\nSetPerkFlag TestPerk 3 1");
    assert_eq!(n(&mut g, "GetPerkLevel TestJunk"), 0.0, "non-PERK form must stay 0");
    assert_eq!(n(&mut g, "GetPerkFlag TestJunk 1"), 0.0, "non-PERK flag must stay 0");
    assert_eq!(n(&mut g, "GetPerkFlag TestPerk 3"), 0.0, "out-of-range flag ID must stay 0");

    // 2. SetPerkLevel reaches PerkSheet, requirement_line, and ITR GetPerkEligibility.
    q(&mut g, "SetPerkLevel TestPerk 6");
    assert_eq!(n(&mut g, "GetPerkLevel TestPerk"), 6.0);
    assert_eq!(fnv_game::chargen::requirement_line(&g, FormId(PERK)), "Level 6--");
    let sheet4 = fnv_game::levelup::PerkSheet::of_player(&mut g, 4);
    assert!(sheet4.perks.contains(&FormId(PERK)));
    assert!(!sheet4.can_take(FormId(PERK)), "level 4 player must see level-6 perk as faint");
    let sheet6 = fnv_game::levelup::PerkSheet::of_player(&mut g, 6);
    assert!(sheet6.can_take(FormId(PERK)), "level 6 player must be able to take level-6 perk");
    g.player.level = Some(4);
    assert_eq!(n(&mut g, "let aR := GetPerkEligibility TestPerk\nSetFunctionValue (aR[\"eligible\"])"), 0.0);
    g.player.level = Some(6);
    assert_eq!(n(&mut g, "let aR := GetPerkEligibility TestPerk\nSetFunctionValue (aR[\"eligible\"])"), 1.0);
    // Min level 0 hides a perk from level-up offering (challenge-reward gate).
    q(&mut g, "SetPerkLevel TestPerk 0");
    assert!(!fnv_game::levelup::PerkSheet::of_player(&mut g, 10).perks.contains(&FormId(PERK)));
    q(&mut g, "SetPerkLevel TestPerk 6");

    // 3. Playable flag via LNSetIsPlayable & SetPerkFlag 1 reaches PerkSheet.
    q(&mut g, "LNSetIsPlayable 0 TestPerk");
    assert_eq!(n(&mut g, "LNIsPlayable TestPerk"), 0.0);
    assert_eq!(n(&mut g, "GetPerkFlag TestPerk 1"), 0.0);
    assert!(!fnv_game::levelup::PerkSheet::of_player(&mut g, 6).perks.contains(&FormId(PERK)));
    q(&mut g, "SetPerkFlag TestPerk 1 1");
    assert_eq!(n(&mut g, "LNIsPlayable TestPerk"), 1.0);
    assert!(fnv_game::levelup::PerkSheet::of_player(&mut g, 6).can_take(FormId(PERK)));

    // 4. Hidden flag (SetPerkFlag 2) reaches PerkSheet and GetAllPerks hidden filter.
    q(&mut g, "SetPerkFlag TestPerk 2 1");
    assert!(!fnv_game::levelup::PerkSheet::of_player(&mut g, 6).perks.contains(&FormId(PERK)));
    q(&mut g, "PlayerREF.SetPerkRank TestPerk 1");
    // Empty GetAllPerks returns Ar_Null (Ar_Size == -1).
    assert_eq!(n(&mut g, "let aR := PlayerREF.GetAllPerks 0 0\nSetFunctionValue (Ar_Size aR)"), -1.0);
    assert_eq!(n(&mut g, "let aR := PlayerREF.GetAllPerks 0 1\nSetFunctionValue (Ar_Size aR)"), 1.0);
    q(&mut g, "SetPerkFlag TestPerk 2 0");
    assert_eq!(n(&mut g, "let aR := PlayerREF.GetAllPerks 0 0\nSetFunctionValue (Ar_Size aR)"), 1.0);

    // 5. Trait flag (SetPerkFlag 0) reaches IsTrait, TraitSheet, PerkSheet, and revision::keeps_perk.
    let rev_ctx = fnv_game::functions::Ctx {
        unit: fnv_game::revision::REVISION_UNIT.into(),
        opcode: 0x11C5,
        on: Some(FormId::PLAYER_REF),
        self_ref: None,
        action_ref: None,
        quest: None,
        inst: None,
        in_condition: false,
    };
    assert!(fnv_game::revision::keeps_perk(&g, &rev_ctx, FormId::PLAYER_REF, FormId(PERK)), "non-trait perk is kept by revision");
    q(&mut g, "SetPerkFlag TestPerk 0 1");
    assert_eq!(n(&mut g, "IsTrait TestPerk"), 1.0);
    assert!(!fnv_game::revision::keeps_perk(&g, &rev_ctx, FormId::PLAYER_REF, FormId(PERK)), "trait perk is not kept by revision");
    assert!(fnv_game::chargen::TraitSheet::of_player(&g).traits.contains(&FormId(PERK)));
    g.player.perks.remove(&FormId(PERK));
    assert!(!fnv_game::levelup::PerkSheet::of_player(&mut g, 6).perks.contains(&FormId(PERK)));

    // 6a. Fresh-run restore (without Session transfer) resets session-only overrides to authored PERK DATA.
    q(&mut g, "SetPerkLevel TestPerk 8");
    let snap: fnv_game::game::GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.checked_snapshot().unwrap()).unwrap()).unwrap();
    let fresh_world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    let mut fresh = Game::restore(&f.data, fresh_world, GameConfig::default(), &snap).unwrap();
    assert_eq!(n(&mut fresh, "GetPerkLevel TestPerk"), 1.0);
    assert_eq!(n(&mut fresh, "GetPerkFlag TestPerk 0"), 0.0);
    assert_eq!(n(&mut fresh, "IsTrait TestPerk"), 0.0);
    assert!(!fnv_game::chargen::TraitSheet::of_player(&fresh).traits.contains(&FormId(PERK)));
    assert!(fnv_game::levelup::PerkSheet::of_player(&mut fresh, 1).can_take(FormId(PERK)));

    // 6b. Same-run restore with put_session preserves SetPerkLevel / SetPerkFlag overrides across load.
    let session = g.take_session();
    let world_session = g.world.take_session();
    let mut same_world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    same_world.put_session(world_session);
    let mut same_run = Game::restore(&f.data, same_world, GameConfig::default(), &snap).unwrap();
    same_run.put_session(session);
    assert_eq!(n(&mut same_run, "GetPerkLevel TestPerk"), 8.0);
    assert_eq!(n(&mut same_run, "GetPerkFlag TestPerk 0"), 1.0);
    assert_eq!(n(&mut same_run, "IsTrait TestPerk"), 1.0);
    assert_eq!(fnv_game::chargen::requirement_line(&same_run, FormId(PERK)), "Level 8--");
    assert!(fnv_game::chargen::TraitSheet::of_player(&same_run).traits.contains(&FormId(PERK)));
    assert!(!fnv_game::levelup::PerkSheet::of_player(&mut same_run, 10).perks.contains(&FormId(PERK)));
}
