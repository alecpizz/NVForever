//! SetClass source-call admission and the class-dependent vendor consumer.

use crate::common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, GameSnapshot, RunCtx};
use fnv_game::{services, GameData};
use fnv_script::{vm::XVal, FunctionTable};
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CLASS_A: u32 = 0x900;
const CLASS_B: u32 = 0x901;
const NPC: u32 = 0x902;
const HIGH_NPC: u32 = 0x903;
const CREATURE: u32 = 0x904;
const MISC: u32 = 0x905;
const NPC_LIST: u32 = 0x906;
const CREATURE_LIST: u32 = 0x907;
const TEMPLATE_NPC: u32 = 0x908;
const CELL: u32 = 0x200;
const NPC_REF: FormId = FormId(0x300);
const SIBLING_REF: FormId = FormId(0x301);
const LEVELED_NPC_REF: FormId = FormId(0x302);
const CREATURE_REF: FormId = FormId(0x303);
const LEVELED_CREATURE_REF: FormId = FormId(0x304);
const MISC_REF: FormId = FormId(0x305);
const TEMPLATE_REF: FormId = FormId(0x306);

fn actor(sig: &[u8; 4], id: u32, name: &str, template: bool) -> Vec<u8> {
    // Existing record/schema contract: Autocalc Service; Traits inheritance for the
    // template control. Auto-calc stats is deliberately off: recalculation is open.
    let mut acbs = vec![0; 24];
    acbs[..4].copy_from_slice(&(1u32 << 23).to_le_bytes());
    acbs[22..24].copy_from_slice(&u16::from(template).to_le_bytes());
    let mut fields = vec![sub(b"EDID", z(name)), sub(b"ACBS", acbs), sub(b"CNAM", CLASS_A.to_le_bytes().to_vec())];
    if template {
        fields.push(sub(b"TPLT", NPC.to_le_bytes().to_vec()));
    }
    record(sig, 0, id, &fields)
}

fn list(sig: &[u8; 4], id: u32, name: &str, entries: &[(i16, u32)]) -> Vec<u8> {
    let mut fields = vec![sub(b"EDID", z(name)), sub(b"LVLD", vec![0]), sub(b"LVLF", vec![0])];
    for (level, base) in entries {
        fields.push(sub(b"LVLO", [level.to_le_bytes().to_vec(), vec![0; 2], base.to_le_bytes().to_vec(), 1i16.to_le_bytes().to_vec(), vec![0; 2]].concat()));
    }
    record(sig, 0, id, &fields)
}

fn placed(sig: &[u8; 4], id: FormId, name: &str, base: u32) -> Vec<u8> {
    record(sig, 0, id.0, &[sub(b"EDID", z(name)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])])
}

fn class(id: u32, name: &str, services: u32) -> Vec<u8> {
    record(b"CLAS", 0, id, &[sub(b"EDID", z(name)), sub(b"DATA", [vec![0; 20], services.to_le_bytes().to_vec(), vec![0; 4]].concat())])
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let npcs = [actor(b"NPC_", 7, "Player", false), actor(b"NPC_", NPC, "TestNpc", false), actor(b"NPC_", HIGH_NPC, "HighNpc", false), actor(b"NPC_", TEMPLATE_NPC, "TemplateNpc", true)].concat();
    let refs = [
        placed(b"ACHR", NPC_REF, "NpcRef", NPC),
        placed(b"ACHR", SIBLING_REF, "SiblingRef", NPC),
        placed(b"ACHR", LEVELED_NPC_REF, "LeveledNpcRef", NPC_LIST),
        placed(b"ACRE", CREATURE_REF, "CreatureRef", CREATURE),
        placed(b"ACRE", LEVELED_CREATURE_REF, "LeveledCreatureRef", CREATURE_LIST),
        placed(b"REFR", MISC_REF, "MiscRef", MISC),
        placed(b"ACHR", TEMPLATE_REF, "TemplateRef", TEMPLATE_NPC),
    ].concat();
    let cell = [record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]), group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs))].concat();
    let bytes = [
        tes4(),
        top(b"CLAS", &[class(CLASS_A, "ClassA", services::WEAPONS), class(CLASS_B, "ClassB", services::ARMOR)].concat()),
        top(b"NPC_", &npcs),
        top(b"CREA", &actor(b"CREA", CREATURE, "TestCreature", false)),
        top(b"MISC", &record(b"MISC", 0, MISC, &[sub(b"EDID", z("TestMisc"))])),
        top(b"LVLN", &list(b"LVLN", NPC_LIST, "NpcList", &[(1, NPC), (10, HIGH_NPC)])),
        top(b"LVLC", &list(b"LVLC", CREATURE_LIST, "CreatureList", &[(1, CREATURE)])),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell))),
    ].concat();
    let plugins = vec![Plugin::parse(&bytes, "ClassReceivers.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let json: Json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let table = FunctionTable::load(&path).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &json, &refs);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture, level: i32) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, ..Default::default() });
    g.player.level = Some(level);
    g.player_entered(Some(FormId(CELL)));
    g
}

fn try_query(g: &mut Game, body: &str, self_ref: Option<FormId>) -> Result<XVal, String> {
    let source = format!("ref r\nfloat n\nBegin Function {{ }}\n{body}\nEnd\n");
    g.run_query(&source, RunCtx { self_ref, ..Default::default() })
}

fn query(g: &mut Game, body: &str, self_ref: Option<FormId>) -> XVal {
    try_query(g, body, self_ref).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn class_of(g: &mut Game, name: &str) -> XVal {
    query(g, &format!("SetFunctionValue (GetClass {name})"), None)
}

#[test]
fn nonactor_and_unknown_receivers_do_not_receive_class_overrides() {
    let f = fixture();
    let mut g = game(&f, 1);
    // Integer-token literals saturate at i32::MAX (vm::literal). A decimal point
    // preserves this unknown positive FormID for the receiver control below.
    assert_eq!(query(&mut g, "let r := 3735928559.0\nSetFunctionValue r", Some(NPC_REF)), XVal::Ref(0xDEAD_BEEF));
    // Non-actor and unknown receivers with a valid CLAS argument continue safely without
    // mutating any base (w4g_05_isactordetected_and_setclass_invalid).
    for body in [
        "MiscRef.SetClass ClassB",
        "TestMisc.SetClass ClassB",
        "let r := 3735928559.0\nr.SetClass ClassB",
    ] {
        assert_eq!(query(&mut g, &format!("{body}\nSetFunctionValue 91"), Some(NPC_REF)), XVal::Num(91.0), "continues: {body}");
        assert!(g.forms.classes.is_empty(), "invalid SetClass must not mutate any base: {body}");
        assert_eq!(g.services(NPC_REF), services::WEAPONS, "implicit actor's real services stay unchanged: {body}");
        assert_eq!(class_of(&mut g, "TestMisc"), XVal::Ref(0), "nonactor class lookup stays empty: {body}");
    }
    // Omission is distinct: it still selects the calling actor.
    query(&mut g, "SetClass ClassB", Some(NPC_REF));
    assert_eq!(g.services(NPC_REF), services::ARMOR);
    assert_eq!(g.services(SIBLING_REF), services::ARMOR, "accepted base sharing is retained");
}

#[test]
fn explicit_null_receivers_continue_and_invalid_class_args_abort_in_strict_and_default_modes() {
    let f = fixture();
    for strict in [true, false] {
        let world = World::new(&f.forms, &f.refs, SimConfig::default());
        let mut g = Game::new(&f.data, world, GameConfig { strict, ..Default::default() });
        g.player.level = Some(1);
        g.player_entered(Some(FormId(CELL)));
        query(&mut g, "NpcRef.SetClass ClassB", None);
        let before = g.forms.classes.clone();
        assert_eq!(g.services(NPC_REF), services::ARMOR);
        assert_eq!(query(&mut g, "let n := 4294967296.0\nSetFunctionValue n", Some(NPC_REF)), XVal::Num(4_294_967_296.0));
        // Null/non-form receivers continue safely without falling back to implicit self (w4g_05).
        for body in [
            "r.SetClass ClassA",
            "let n := 0.5\nn.SetClass ClassA",
            "let n := -1\nn.SetClass ClassA",
            "let n := 4294967296.0\nn.SetClass ClassA",
        ] {
            assert_eq!(query(&mut g, &format!("{body}\nSetFunctionValue 91"), Some(NPC_REF)), XVal::Num(91.0), "null receiver continues (strict={strict}): {body}");
            assert_eq!(g.forms.classes, before, "null receiver must not mutate any base: {body}");
            assert_eq!(g.services(NPC_REF), services::ARMOR, "null receiver must not fall back to implicit self: {body}");
            assert_eq!(g.services(SIBLING_REF), services::ARMOR, "shared base state remains intact: {body}");
        }
        // Non-CLAS form argument and null class argument BOTH abort the script at runtime
        // in both strict and default modes, leaving the actor's class unchanged (w4g_05).
        for body in [
            "NpcRef.SetClass TestMisc",
            "NpcRef.SetClass r",
        ] {
            assert_eq!(
                try_query(&mut g, &format!("{body}\nSetFunctionValue 91"), Some(NPC_REF)),
                Err("setclass requires a CLAS form".into()),
                "invalid class arg aborts (strict={strict}): {body}",
            );
            assert_eq!(g.forms.classes, before, "aborted call must not mutate any base: {body}");
            assert_eq!(g.services(NPC_REF), services::ARMOR, "aborted call leaves services unchanged: {body}");
        }
    }
}

#[test]
fn omitted_receiver_without_self_changes_nothing() {
    let f = fixture();
    let mut g = game(&f, 1);
    query(&mut g, "SetClass ClassB\nSetFunctionValue 91", None);
    assert!(g.forms.classes.is_empty());
    assert_eq!(g.services(NPC_REF), services::WEAPONS);
}

#[test]
fn placed_leveled_actors_and_template_ownership_keep_the_existing_policy() {
    let f = fixture();
    for level in [1, 10] {
        let mut g = game(&f, level);
        let picked = g.template_source(FormId(NPC_LIST), LEVELED_NPC_REF, |_| false).unwrap().0;
        assert_eq!(picked, FormId(if level == 1 { NPC } else { HIGH_NPC }), "actual level-aware selection");
        assert_eq!(g.services(LEVELED_NPC_REF), services::WEAPONS);
        query(&mut g, "LeveledNpcRef.SetClass ClassB", None);
        assert_eq!(g.services(LEVELED_NPC_REF), services::ARMOR, "class-dependent consumer reads the admitted actor");
        assert_eq!(g.services(NPC_REF), services::WEAPONS, "carrier override does not rewrite its picked record");
        for name in ["CreatureRef", "LeveledCreatureRef"] {
            query(&mut g, &format!("{name}.SetClass ClassB"), None);
            assert_eq!(class_of(&mut g, name), XVal::Ref(CLASS_B), "existing creature admission: {name}");
        }
        assert_eq!(g.template_source(FormId(CREATURE_LIST), LEVELED_CREATURE_REF, |_| false).unwrap().0, FormId(CREATURE));
        query(&mut g, "TemplateRef.SetClass ClassB", None);
        assert_eq!(g.services(TEMPLATE_REF), services::ARMOR);
        assert_eq!(g.services(NPC_REF), services::WEAPONS, "Traits template is not modified");
    }
}

#[test]
fn source_class_change_still_drives_services_after_json_restore() {
    let f = fixture();
    let mut g = game(&f, 10);
    query(&mut g, "NpcRef.SetClass ClassB\nLeveledNpcRef.SetClass ClassB", None);
    let snapshot: GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.snapshot()).unwrap()).unwrap();
    let world = World::restore(&f.forms, &f.refs, &snapshot.sim).unwrap();
    let mut restored = Game::restore(&f.data, world, g.cfg, &snapshot).unwrap();
    assert_eq!(g.forms.classes, restored.forms.classes);
    for actor in [NPC_REF, SIBLING_REF, LEVELED_NPC_REF] {
        assert_eq!(restored.services(actor), services::ARMOR, "restored vendor consumer: {actor}");
    }
    assert_eq!(class_of(&mut restored, "LeveledNpcRef"), XVal::Ref(CLASS_B));
    let before = restored.forms.classes.clone();
    assert_eq!(query(&mut restored, "r.SetClass ClassA\nSetFunctionValue 91", Some(NPC_REF)), XVal::Num(91.0));
    assert_eq!(try_query(&mut restored, "NpcRef.SetClass TestMisc\nSetFunctionValue 91", Some(NPC_REF)), Err("setclass requires a CLAS form".into()));
    assert_eq!(try_query(&mut restored, "NpcRef.SetClass r\nSetFunctionValue 91", Some(NPC_REF)), Err("setclass requires a CLAS form".into()));
    assert_eq!(restored.forms.classes, before, "null receiver and invalid class aborts after load preserve all class overrides");
    assert_eq!(restored.services(NPC_REF), services::ARMOR, "explicit null does not fall back to self after load");
}
