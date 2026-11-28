//! Integration tests for the NVSE zero-stub batch:
//! `SetPersistent`, `GetFallTimeRemaining`, `GetCalculatedActorSpread`, and
//! `IsItemUnique` / `ToggleItemUnique`
//! (`docs/spec-notes/nvse-stubs-persistent-fall-spread-unique.md`).

use crate::common;

use common::*;
use fnv_esm::raw::Subrecord;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::functions::console::{persistent_ref_count, ConsoleCtx};
use fnv_game::functions::senses::Motion;
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::perks;
use fnv_game::ray::{with_caster, CastKind, Hit};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CELL_LOADED: u32 = 0x200;
const CELL_UNLOADED: u32 = 0x201;

const STAT: u32 = 0x800;
const STAT_GROUP: u32 = 0x801;
const REF_TEMP: u32 = 0x301;
const REF_PERSIST: u32 = 0x302;
const REF_PERSIST_GROUP_ONLY: u32 = 0x303;
const NPC_BASE: u32 = 0x820;
const NPC_REF: u32 = 0x310;

const WEAP_PISTOL: u32 = 0x840;
const WEAP_PISTOL_DONOR: u32 = 0x841;
const WEAP_RIFLE: u32 = 0x842;
const WEAP_LASER_PISTOL: u32 = 0x843;
const WEAP_PLASMA_RIFLE: u32 = 0x844;
const WEAP_FLAMER: u32 = 0x845;
const WEAP_GRENADE: u32 = 0x846;
const ARMO_VEST: u32 = 0x850;
const ALCH_STIM: u32 = 0x860;
const FLST_REPAIR: u32 = 0x870;
const FLST_UNIQUE: u32 = 0x871;

const PERK_SPREAD: u32 = 0x880;
const PERK_STR_REQ: u32 = 0x881;
const PERK_AIM_MOVE: u32 = 0x882;
const PERK_JURY: u32 = 0x883;

const SKILL_GUNS: u32 = 41;

fn weap_dnam(anim_type: u32, min_spread: f32, skill: u32, strength_req: u32, skill_req: u32) -> Subrecord {
    let mut d = vec![0u8; 204];
    d[0..4].copy_from_slice(&anim_type.to_le_bytes());
    d[16..20].copy_from_slice(&min_spread.to_le_bytes());
    d[104..108].copy_from_slice(&skill.to_le_bytes());
    d[168..172].copy_from_slice(&strength_req.to_le_bytes());
    d[200..204].copy_from_slice(&skill_req.to_le_bytes());
    sub(b"DNAM", d)
}

fn weap(fid: u32, edid: &str, anim_type: u32, min_spread: f32, strength_req: u32, skill_req: u32, repl: Option<u32>) -> Vec<u8> {
    let mut data = vec![0u8; 15];
    data[0..4].copy_from_slice(&100i32.to_le_bytes()); // value
    data[4..8].copy_from_slice(&100i32.to_le_bytes()); // health
    data[8..12].copy_from_slice(&2.0f32.to_le_bytes()); // weight
    let mut subs = vec![
        sub(b"EDID", z(edid)),
        sub(b"FULL", z(edid)),
        sub(b"DATA", data),
        weap_dnam(anim_type, min_spread, SKILL_GUNS, strength_req, skill_req),
    ];
    if let Some(l) = repl {
        subs.push(sub(b"REPL", l.to_le_bytes().to_vec()));
    }
    record(b"WEAP", 0, fid, &subs)
}

fn armo(fid: u32, edid: &str) -> Vec<u8> {
    let mut bmdt = vec![0u8; 8];
    bmdt[0..4].copy_from_slice(&(1u32 << 2).to_le_bytes());
    let mut data = Vec::with_capacity(12);
    data.extend(100i32.to_le_bytes());
    data.extend(100i32.to_le_bytes());
    data.extend(5.0f32.to_le_bytes());
    let mut dnam = vec![0u8; 12];
    dnam[4..8].copy_from_slice(&5.0f32.to_le_bytes());
    record(
        b"ARMO",
        0,
        fid,
        &[
            sub(b"EDID", z(edid)),
            sub(b"FULL", z(edid)),
            sub(b"BMDT", bmdt),
            sub(b"DATA", data),
            sub(b"DNAM", dnam),
        ],
    )
}

fn perk_ep(fid: u32, edid: &str, point: u8, function: u8, value: f32) -> Vec<u8> {
    let subs = vec![
        sub(b"EDID", z(edid)),
        sub(b"FULL", z(edid)),
        sub(b"DATA", vec![0, 1, 1, 1, 0]),
        sub(b"PRKE", vec![2, 0, 0]),
        sub(b"DATA", vec![point, function, 1]),
        sub(b"EPFT", vec![1]),
        sub(b"EPFD", value.to_le_bytes().to_vec()),
        sub(b"PRKF", vec![]),
    ];
    record(b"PERK", 0, fid, &subs)
}

fn flst(fid: u32, edid: &str, items: &[u32]) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid))];
    for &i in items {
        subs.push(sub(b"LNAM", i.to_le_bytes().to_vec()));
    }
    record(b"FLST", 0, fid, &subs)
}

fn placed_ref(sig: &[u8; 4], flags: u32, fid: u32, edid: &str, base: u32, pos: [f32; 3]) -> Vec<u8> {
    let data: Vec<u8> = [pos[0], pos[1], pos[2], 0.0, 0.0, 0.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    record(
        sig,
        flags,
        fid,
        &[sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)],
    )
}

fn plugin_bytes() -> Vec<u8> {
    let stat = [
        record(b"STAT", 0, STAT, &[sub(b"EDID", z("TestCrate"))]),
        record(b"STAT", 0, STAT_GROUP, &[sub(b"EDID", z("TestGroupCrate"))]),
    ]
    .concat();
    let weaps = [
        weap(WEAP_PISTOL, "TestPistol", 3, 0.5, 6, 50, Some(FLST_REPAIR)),
        weap(WEAP_PISTOL_DONOR, "TestPistolDonor", 3, 0.5, 4, 25, Some(FLST_REPAIR)),
        weap(WEAP_RIFLE, "TestRifle", 5, 0.25, 6, 50, None),
        weap(WEAP_LASER_PISTOL, "TestLaserPistol", 4, 0.5, 6, 50, None),
        weap(WEAP_PLASMA_RIFLE, "TestPlasmaRifle", 6, 0.25, 6, 50, None),
        weap(WEAP_FLAMER, "TestFlamer", 8, 0.25, 6, 50, None),
        weap(WEAP_GRENADE, "TestGrenade", 10, 0.5, 6, 50, None),
    ]
    .concat();
    let armors = armo(ARMO_VEST, "TestArmor");
    let alch = record(
        b"ALCH",
        0,
        ALCH_STIM,
        &[sub(b"EDID", z("TestStimpak")), sub(b"FULL", z("TestStimpak")), sub(b"DATA", 0.5f32.to_le_bytes().to_vec())],
    );
    let flsts = [
        flst(FLST_REPAIR, "TestRepairList", &[WEAP_PISTOL, WEAP_PISTOL_DONOR]),
        flst(FLST_UNIQUE, "TestUniqueList", &[WEAP_PISTOL, ARMO_VEST, ALCH_STIM]),
    ]
    .concat();
    let perks_rec = [
        perk_ep(PERK_SPREAD, "TestSpreadPerk", perks::CALCULATE_GUN_SPREAD, perks::MULTIPLY_VALUE, 0.5),
        perk_ep(PERK_STR_REQ, "TestStrReqPerk", perks::MODIFY_WEAPON_STRENGTH_REQ, perks::ADD_VALUE, 2.0),
        perk_ep(PERK_AIM_MOVE, "TestAimMovePerk", perks::MODIFY_AIMING_MOVE_SPEED, perks::MULTIPLY_VALUE, 2.0),
        perk_ep(PERK_JURY, "TestJuryRigging", perks::HAS_JURY_RIGGING, perks::SET_VALUE, 1.0),
    ]
    .concat();
    let mut acbs = vec![0u8; 24];
    acbs[4..6].copy_from_slice(&100u16.to_le_bytes());
    let npcs = [
        record(
            b"NPC_",
            0,
            0x7,
            &[
                sub(b"EDID", z("Player")),
                sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![6, 5, 5, 5, 5, 5, 5]].concat()),
            ],
        ),
        record(
            b"NPC_",
            0,
            NPC_BASE,
            &[
                sub(b"EDID", z("TestActor")),
                sub(b"FULL", z("TestActor")),
                sub(b"ACBS", acbs),
                sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![6; 7]].concat()),
            ],
        ),
    ]
    .concat();

    let cell_loaded = record(b"CELL", 0, CELL_LOADED, &[sub(b"EDID", z("LoadedCell")), sub(b"DATA", vec![1])]);
    let loaded_refs = [
        placed_ref(b"ACHR", 0x400, NPC_REF, "TestActorRef", NPC_BASE, [0.0, 0.0, 600.8625]),
        // Placed in the persistent group (type 8) WITHOUT static record flag 0x400.
        placed_ref(b"REFR", 0, REF_PERSIST_GROUP_ONLY, "PersistGroupOnlyRef", STAT_GROUP, [5.0, 0.0, 0.0]),
    ]
    .concat();
    let loaded_children = group(CELL_LOADED.to_le_bytes(), 6, &group(CELL_LOADED.to_le_bytes(), 8, &loaded_refs));

    let cell_unloaded = record(b"CELL", 0, CELL_UNLOADED, &[sub(b"EDID", z("UnloadedCell")), sub(b"DATA", vec![1])]);
    let temp_ref = placed_ref(b"REFR", 0, REF_TEMP, "TempCrateRef", STAT, [10.0, 0.0, 0.0]);
    let persist_ref = placed_ref(b"REFR", 0x400, REF_PERSIST, "PersistCrateRef", STAT, [20.0, 0.0, 0.0]);
    let unloaded_children = group(
        CELL_UNLOADED.to_le_bytes(),
        6,
        &[
            group(CELL_UNLOADED.to_le_bytes(), 8, &persist_ref),
            group(CELL_UNLOADED.to_le_bytes(), 9, &temp_ref),
        ]
        .concat(),
    );

    let cells = [cell_loaded, loaded_children, cell_unloaded, unloaded_children].concat();
    [
        tes4(),
        top(b"STAT", &stat),
        top(b"WEAP", &weaps),
        top(b"ARMO", &armors),
        top(b"ALCH", &alch),
        top(b"FLST", &flsts),
        top(b"PERK", &perks_rec),
        top(b"NPC_", &npcs),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cells))),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "NvseStubsBatch2.esm".into()).unwrap()];
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
    g.player_entered(Some(FormId(CELL_LOADED)));
    g
}

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default())
        .unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn num(g: &mut Game, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(x) => x,
        other => panic!("{expr}: expected Num, got {other:?}"),
    }
}

fn prc(g: &mut Game) -> usize {
    let lines = persistent_ref_count(g, &ConsoleCtx::default(), &[]).unwrap();
    lines[0]
        .strip_prefix("Persistent references: ")
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn set_persistent_toggles_flag_and_reaches_all_consumers() {
    let f = fixture();
    let mut g = game(&f);

    // Baseline: PersistCrateRef is persistent, TempCrateRef is not.
    assert_eq!(num(&mut g, "TempCrateRef.IsPersistent"), 0.0);
    assert_eq!(num(&mut g, "TempCrateRef.GetReferenceFlag 1024"), 0.0);
    assert_eq!(num(&mut g, "GetPermanent TempCrateRef"), 0.0);
    assert_eq!(num(&mut g, "PersistCrateRef.IsPersistent"), 1.0);
    assert_eq!(num(&mut g, "GetNumRefsInCell UnloadedCell 0"), 1.0, "unloaded cell walks only persistent refs");
    assert_eq!(num(&mut g, "Ar_Size (GetFormRefs TestCrate)"), 1.0);
    let prc_before = prc(&mut g);

    // Set another flag bit first to ensure SetPersistent preserves unrelated record flags.
    q(&mut g, "TempCrateRef.SetReferenceFlag 64 1");
    assert_eq!(num(&mut g, "TempCrateRef.SetPersistent 1"), 0.0, "SetPersistent returns 0 [observed: gaps6]");
    assert_eq!(num(&mut g, "TempCrateRef.IsPersistent"), 1.0);
    assert_eq!(num(&mut g, "TempCrateRef.GetReferenceFlag 1024"), 1.0);
    assert_eq!(num(&mut g, "TempCrateRef.GetReferenceFlag 64"), 1.0, "unrelated flag bit preserved");
    assert_eq!(num(&mut g, "GetPermanent TempCrateRef"), 1.0);
    assert_eq!(num(&mut g, "GetNumRefsInCell UnloadedCell 0"), 2.0);
    assert_eq!(num(&mut g, "Ar_Size (GetFormRefs TestCrate)"), 2.0);
    assert_eq!(prc(&mut g), prc_before + 1);

    // Clear persistence on both TempCrateRef and the originally persistent PersistCrateRef.
    q(&mut g, "TempCrateRef.SetPersistent 0\nPersistCrateRef.SetPersistent 0");
    assert_eq!(num(&mut g, "TempCrateRef.IsPersistent"), 0.0);
    assert_eq!(num(&mut g, "TempCrateRef.GetReferenceFlag 1024"), 0.0);
    assert_eq!(num(&mut g, "TempCrateRef.GetReferenceFlag 64"), 1.0, "unrelated flag bit still preserved");
    assert_eq!(num(&mut g, "PersistCrateRef.IsPersistent"), 0.0);
    assert_eq!(num(&mut g, "GetNumRefsInCell UnloadedCell 0"), 0.0);
    assert_eq!(num(&mut g, "Ar_Size (GetFormRefs TestCrate)"), 0.0);
    assert_eq!(prc(&mut g), prc_before - 1);

    // Re-enabling via SetPersistent 1 restores both IsPersistent and flag bit 0x400.
    q(&mut g, "PersistCrateRef.SetPersistent 1");
    assert_eq!(num(&mut g, "PersistCrateRef.IsPersistent"), 1.0);
    assert_eq!(num(&mut g, "PersistCrateRef.GetReferenceFlag 1024"), 1.0);

    // Runtime-placed reference toggled persistent reaches IsPersistent and GetFormRefs.
    let spawned = g.world.place(FormId(STAT), Some(FormId(CELL_UNLOADED)), [30.0, 0.0, 0.0], [0.0; 3], 1).unwrap();
    assert_eq!(
        g.run_query(
            "Begin Function { }\nSetFunctionValue (IsPersistent)\nEnd\n",
            RunCtx { self_ref: Some(spawned), ..RunCtx::default() }
        )
        .unwrap(),
        XVal::Num(0.0)
    );
    g.run_query(
        "Begin Function { }\nSetPersistent 1\nEnd\n",
        RunCtx { self_ref: Some(spawned), ..RunCtx::default() },
    )
    .unwrap();
    assert_eq!(
        g.run_query(
            "Begin Function { }\nSetFunctionValue (IsPersistent)\nEnd\n",
            RunCtx { self_ref: Some(spawned), ..RunCtx::default() }
        )
        .unwrap(),
        XVal::Num(1.0)
    );
    assert_eq!(num(&mut g, "Ar_Size (GetFormRefs TestCrate)"), 2.0, "PersistCrateRef + persistent runtime ref");

    // Wave 4D w4d_06_player_setpersistent_0 [observed]: Player.IsPersistent starts at 1,
    // Player.SetPersistent 0 clears it to 0, and Player.SetPersistent 1 restores 1.
    assert_eq!(num(&mut g, "player.IsPersistent"), 1.0);
    q(&mut g, "player.SetPersistent 0");
    assert_eq!(num(&mut g, "player.IsPersistent"), 0.0);
    assert_eq!(num(&mut g, "player.GetReferenceFlag 1024"), 0.0);
    q(&mut g, "player.SetPersistent 1");
    assert_eq!(num(&mut g, "player.IsPersistent"), 1.0);
    assert_eq!(num(&mut g, "player.GetReferenceFlag 1024"), 1.0);
}

#[test]
fn set_persistent_ignores_unrelated_flag_writes_on_persistent_group_ref() {
    let f = fixture();
    let mut g = game(&f);

    // PersistGroupOnlyRef sits in a persistent cell group (type 8) with static record flags == 0.
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.IsPersistent"), 1.0);
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.GetReferenceFlag 1024"), 0.0);

    // Writing an unrelated flag via SetReferenceFlag or SetFlagsLow writes `other.flags`
    // without bit 0x400, which must NOT clear persistence.
    q(&mut g, "PersistGroupOnlyRef.SetReferenceFlag 64 1");
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.GetReferenceFlag 64"), 1.0);
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.IsPersistent"), 1.0, "SetReferenceFlag must not clear persistent-group ref");

    q(&mut g, "PersistGroupOnlyRef.SetFlagsLow 64");
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.GetReferenceFlag 64"), 1.0);
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.GetReferenceFlag 1024"), 0.0);
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.IsPersistent"), 1.0, "SetFlagsLow must not clear persistent-group ref");

    // Explicit SetPersistent 0 clears persistence and keeps flag bit 0x400 clear while preserving bit 64.
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.SetPersistent 0"), 0.0);
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.IsPersistent"), 0.0);
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.GetReferenceFlag 1024"), 0.0);
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.GetReferenceFlag 64"), 1.0);

    // Explicit SetPersistent 1 restores persistence and synchronizes flag bit 0x400 while preserving bit 64.
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.SetPersistent 1"), 0.0);
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.IsPersistent"), 1.0);
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.GetReferenceFlag 1024"), 1.0);
    assert_eq!(num(&mut g, "PersistGroupOnlyRef.GetReferenceFlag 64"), 1.0);
}

#[test]
fn player_set_persistent_zero_preserves_all_consumers() {
    use fnv_game::functions::console::{persistent_ref_count, ConsoleCtx};

    let f = fixture();
    let mut g = game(&f);

    // Baseline: player is persistent by default.
    assert_eq!(num(&mut g, "player.IsPersistent"), 1.0);
    assert_eq!(num(&mut g, "player.GetPermanent"), 1.0);
    assert_eq!(num(&mut g, "GetPermanent player"), 1.0);
    let prc_before = persistent_ref_count(&mut g, &ConsoleCtx::default(), &[]).unwrap();
    let walk_before = num(&mut g, "GetNumRefsInCell TestCell 200");
    let form_refs_before = num(&mut g, "Ar_Size (GetFormRefs TestCrate)");

    // Clear player persistence via SetPersistent 0 [observed: w4d_06_player_setpersistent_0]:
    // verify all Game::persistent consumers stay healthy and consistent.
    assert_eq!(num(&mut g, "player.SetPersistent 0"), 0.0);
    assert_eq!(num(&mut g, "player.IsPersistent"), 0.0);
    assert_eq!(num(&mut g, "player.GetReferenceFlag 1024"), 0.0);
    assert_eq!(num(&mut g, "player.GetPermanent"), 0.0);
    assert_eq!(num(&mut g, "GetPermanent player"), 0.0);
    assert_eq!(num(&mut g, "GetNumRefsInCell TestCell 200"), walk_before, "ref walk already excludes player");
    assert_eq!(num(&mut g, "Ar_Size (GetFormRefs TestCrate)"), form_refs_before, "GetFormRefs unaffected");
    let prc_after = persistent_ref_count(&mut g, &ConsoleCtx::default(), &[]).unwrap();
    assert_eq!(prc_after, prc_before, "PersistentRefCount stays valid");

    // Snapshot + restore resets session-only SetPersistent override back to default 1 (see docs/spec-notes/nvse-stubs-persistent-fall-spread-unique.md).
    let snap = g.snapshot();
    let mut restored = Game::restore(
        &f.data,
        World::restore(&f.forms, &f.refs, &snap.sim).unwrap(),
        GameConfig::default(),
        &snap,
    )
    .unwrap();
    assert_eq!(num(&mut restored, "player.IsPersistent"), 1.0, "session-only override resets on load");

    // Restore player persistence in the live session.
    assert_eq!(num(&mut g, "player.SetPersistent 1"), 0.0);
    assert_eq!(num(&mut g, "player.IsPersistent"), 1.0);
    assert_eq!(num(&mut g, "player.GetReferenceFlag 1024"), 1.0);
    assert_eq!(num(&mut g, "player.GetPermanent"), 1.0);
}

#[test]
fn get_fall_time_remaining_handles_grounded_direct_and_kinematic_airborne() {
    let f = fixture();
    let mut g = game(&f);

    // Grounded actor: 0.
    assert_eq!(num(&mut g, "TestActorRef.GetFallTimeRemaining"), 0.0);

    // Explicit engine-supplied value takes precedence and clears cleanly.
    g.set_fall_time_remaining(FormId(NPC_REF), Some(1.25));
    assert!((num(&mut g, "TestActorRef.GetFallTimeRemaining") - 1.25).abs() < 1e-6);
    g.set_fall_time_remaining(FormId(NPC_REF), None);
    assert_eq!(num(&mut g, "TestActorRef.GetFallTimeRemaining"), 0.0);

    // Airborne without a lent ray caster returns 0 safely.
    g.gaps.air_times.insert(FormId(NPC_REF), 0.5);
    assert_eq!(num(&mut g, "TestActorRef.GetFallTimeRemaining"), 0.0);

    // With a floor at z = 0 and actor at z = 600.8625 (0.5s into a 686.7-unit drop):
    // untracked velocity estimates vz = -686.7 * 0.5 = -343.35 -> remaining time = sqrt(2) - 0.5 = 0.91421s.
    let floor = |from: [f32; 3], to: [f32; 3], kind: CastKind| {
        if kind == CastKind::World && from[2] > 0.0 && to[2] <= 0.0 {
            Some(Hit { distance: from[2], point: [from[0], from[1], 0.0], form: None, material: None })
        } else {
            None
        }
    };
    let expected = 2.0f64.sqrt() - 0.5;
    let t_est = with_caster(&floor, || num(&mut g, "TestActorRef.GetFallTimeRemaining"));
    assert!((t_est - expected).abs() < 1e-4, "expected ~{expected}, got {t_est}");

    // Tracked downward velocity yields the same kinematic solution.
    g.jip2.velocity.insert(FormId(NPC_REF), ([0.0, 0.0, 600.8625], [0.0, 0.0, -343.35]));
    let t_vel = with_caster(&floor, || num(&mut g, "TestActorRef.GetFallTimeRemaining"));
    assert!((t_vel - expected).abs() < 1e-4, "expected ~{expected}, got {t_vel}");

    // Rising jump (vz = +300 at z = 100) takes longer than falling.
    g.world.get(FormId(NPC_REF)).unwrap().pos = [0.0, 0.0, 100.0];
    g.jip2.velocity.insert(FormId(NPC_REF), ([0.0, 0.0, 100.0], [0.0, 0.0, 300.0]));
    let expected_jump = (300.0 + (300.0f64 * 300.0 + 2.0 * 686.7 * 100.0).sqrt()) / 686.7;
    let t_jump = with_caster(&floor, || num(&mut g, "TestActorRef.GetFallTimeRemaining"));
    assert!((t_jump - expected_jump).abs() < 1e-4, "expected ~{expected_jump}, got {t_jump}");

    // Lower actor gravity multiplier slows the fall (longer remaining time).
    q(&mut g, "TestActorRef.SetActorGravityMult 0.25");
    let t_low_g = with_caster(&floor, || num(&mut g, "TestActorRef.GetFallTimeRemaining"));
    assert!(t_low_g > t_jump, "lower gravity must increase fall time remaining ({t_low_g} > {t_jump})");

    // Higher local gravity vector speeds up the fall (shorter remaining time).
    q(&mut g, "TestActorRef.SetActorGravityMult 1");
    g.other.jip13.gravity = Some(([0.0, 0.0, -1373.4], None));
    let t_high_g = with_caster(&floor, || num(&mut g, "TestActorRef.GetFallTimeRemaining"));
    assert!(t_high_g < t_jump, "higher gravity must decrease fall time remaining ({t_high_g} < {t_jump})");
}

#[test]
fn get_calculated_actor_spread_covers_weapon_stance_limbs_perks_and_wobble() {
    let f = fixture();
    let mut g = game(&f);

    // No weapon equipped and bare call without subject return 0.
    assert_eq!(num(&mut g, "player.GetCalculatedActorSpread"), 0.0);
    assert_eq!(num(&mut g, "GetCalculatedActorSpread"), 0.0);

    // Equip 1H pistol (strength_req 6, skill_req 50, Guns skill).
    // Player has Agility 5, Luck 5 => derived Guns = 2 + 2*5 + ceil(5*0.5) = 15;
    // SetActorValue Guns 35 puts effective Guns at 50.
    q(
        &mut g,
        "player.SetActorValue Strength 6\nplayer.SetActorValue Guns 35\nplayer.AddItem TestPistol 1\nplayer.EquipItem TestPistol",
    );
    assert_eq!(num(&mut g, "player.GetActorValue Strength"), 6.0);
    assert_eq!(num(&mut g, "player.GetActorValue Guns"), 50.0);
    // Standing + unaimed = 0.1 + 0.2 = 0.3; skill_mult = 1 - 0.5 * 50/100 = 0.75 => 0.225.
    let standing = num(&mut g, "player.GetCalculatedActorSpread");
    assert!((standing - 0.225).abs() < 1e-6, "standing unaimed = {standing}");

    // Aiming removes unaimed penalty (0.2): 0.75 * 0.1 = 0.075.
    g.jip2.told.aiming = true;
    let aimed = num(&mut g, "player.GetCalculatedActorSpread");
    assert!((aimed - 0.075).abs() < 1e-6, "standing aimed = {aimed}");

    // Aiming + sneaking removes standing penalty too, clamping to fMinGunSpreadValue (0.01).
    g.set_motion(FormId::PLAYER_REF, Motion { sneaking: true, ..Motion::default() });
    let crouched_aimed = num(&mut g, "player.GetCalculatedActorSpread");
    assert!((crouched_aimed - 0.01).abs() < 1e-6, "clamped to min spread = {crouched_aimed}");

    // Moving while aiming & sneaking: walking adds 0.1 (0.075), running adds 0.2 (0.15).
    g.set_motion(FormId::PLAYER_REF, Motion { sneaking: true, moving: true, running: false, ..Motion::default() });
    let walking = num(&mut g, "player.GetCalculatedActorSpread");
    assert!((walking - 0.075).abs() < 1e-6, "walking = {walking}");
    g.set_motion(FormId::PLAYER_REF, Motion { sneaking: true, moving: true, running: true, ..Motion::default() });
    let running = num(&mut g, "player.GetCalculatedActorSpread");
    assert!((running - 0.15).abs() < 1e-6, "running = {running}");

    // MODIFY_AIMING_MOVE_SPEED (×2) halves the moving penalty: 0.2 / 2 * 0.75 = 0.075.
    q(&mut g, "player.AddPerk TestAimMovePerk");
    let running_perk = num(&mut g, "player.GetCalculatedActorSpread");
    assert!((running_perk - 0.075).abs() < 1e-6, "running with aim-move perk = {running_perk}");

    // Reset to standing unaimed and test Strength + skill shortfalls (Guns 15 + derived 15 = 30).
    g.jip2.told.aiming = false;
    g.set_motion(FormId::PLAYER_REF, Motion::default());
    q(&mut g, "player.SetActorValue Strength 4\nplayer.SetActorValue Guns 15");
    assert_eq!(num(&mut g, "player.GetActorValue Strength"), 4.0);
    assert_eq!(num(&mut g, "player.GetActorValue Guns"), 30.0);
    // skill_mult = 1 - 0.5 * 0.30 = 0.85; terms = 0.2 + 0.1 + (6-4)*0.025 + (50-30)*0.01 = 0.55 => 0.4675.
    let shortfall = num(&mut g, "player.GetCalculatedActorSpread");
    assert!((shortfall - 0.4675).abs() < 1e-6, "shortfall = {shortfall}");

    // MODIFY_WEAPON_STRENGTH_REQ (+2) eliminates the Strength shortfall: 0.85 * 0.50 = 0.425.
    q(&mut g, "player.AddPerk TestStrReqPerk");
    let after_str_perk = num(&mut g, "player.GetCalculatedActorSpread");
    assert!((after_str_perk - 0.425).abs() < 1e-6, "after str-req perk = {after_str_perk}");

    // Crippled arms on 1H pistol: left arm alone adds 0, right arm adds 0.2 * skill_mult,
    // both arms add 0.4 * skill_mult (2x right-arm delta) [observed: w4c_a06b_spread_settled].
    q(&mut g, "player.SetActorValue Strength 6\nplayer.SetActorValue Guns 35\nplayer.CrippleLimb 2");
    assert!((num(&mut g, "player.GetCalculatedActorSpread") - 0.225).abs() < 1e-6, "1H ignores left arm alone");
    q(&mut g, "player.RestoreActorValue LeftAttackCondition 100\nplayer.CrippleLimb 3");
    assert!((num(&mut g, "player.GetCalculatedActorSpread") - 0.375).abs() < 1e-6, "1H right arm crippled adds 0.2 * 0.75");
    q(&mut g, "player.CrippleLimb 2");
    // Both arms crippled on 1H: 0.75 * (0.2 + 0.1 + 0.4) = 0.525.
    assert!((num(&mut g, "player.GetCalculatedActorSpread") - 0.525).abs() < 1e-6, "1H both arms crippled");
    // SetNoGunWobble 1 does NOT zero GetCalculatedActorSpread on 1H [observed: w4c_a06b_spread_settled].
    q(&mut g, "player.SetNoGunWobble 1");
    assert!((num(&mut g, "player.GetCalculatedActorSpread") - 0.525).abs() < 1e-6, "SetNoGunWobble 1 preserves 1H spread");
    q(&mut g, "player.SetNoGunWobble 0");

    // 2H rifle with both arms crippled uses fCrippledArms2HSpreadPenalty (0.6):
    // 0.75 * (0.2 + 0.1 + 0.6) = 0.675 [observed: w4d_09_spread_all_anim_types].
    q(&mut g, "player.AddItem TestRifle 1\nplayer.EquipItem TestRifle");
    assert!((num(&mut g, "player.GetCalculatedActorSpread") - 0.675).abs() < 1e-6, "2H both arms crippled");

    // CALCULATE_GUN_SPREAD perk (×0.5) halves final spread: 0.3375.
    q(&mut g, "player.AddPerk TestSpreadPerk");
    assert!((num(&mut g, "player.GetCalculatedActorSpread") - 0.3375).abs() < 1e-6, "spread perk halves result");

    // SetNoGunWobble 1 does NOT zero GetCalculatedActorSpread on 2H either [observed: w4c_a06b_spread_settled].
    q(&mut g, "player.SetNoGunWobble 1");
    assert!((num(&mut g, "player.GetCalculatedActorSpread") - 0.3375).abs() < 1e-6, "SetNoGunWobble 1 preserves 2H spread");
}

#[test]
fn get_calculated_actor_spread_crippled_arms_across_anim_types() {
    // [observed: w4d_09_spread_all_anim_types (`wave4d_extenders.json`, #3802 / PR #3803)]:
    // - 1H guns (anim 3, 4): right arm +0.2, left arm alone 0, both arms +0.4.
    // - 2H ranged (anim 5, 6, 8): either single arm +0.4, both arms +0.6.
    // - Thrown (anim 10): 0 for right, left, and both arms.
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "player.SetActorValue Strength 6\nplayer.SetActorValue Guns 35");
    // skill_mult = 0.75, base unaimed + standing = 0.3 -> baseline 0.225.
    let cases = [
        ("TestPistol", 0.375, 0.225, 0.525),      // anim 3 (1H): r=+0.2, l=0, b=+0.4
        ("TestLaserPistol", 0.375, 0.225, 0.525), // anim 4 (1H): r=+0.2, l=0, b=+0.4
        ("TestRifle", 0.525, 0.525, 0.675),       // anim 5 (2H): r=+0.4, l=+0.4, b=+0.6
        ("TestPlasmaRifle", 0.525, 0.525, 0.675), // anim 6 (2H): r=+0.4, l=+0.4, b=+0.6
        ("TestFlamer", 0.525, 0.525, 0.675),      // anim 8 (2H): r=+0.4, l=+0.4, b=+0.6
        ("TestGrenade", 0.225, 0.225, 0.225),     // anim 10 (thrown): r=0, l=0, b=0
    ];
    for (weap_name, want_r, want_l, want_b) in cases {
        q(
            &mut g,
            &format!(
                "player.RestoreActorValue LeftAttackCondition 100\n\
                 player.RestoreActorValue RightAttackCondition 100\n\
                 player.AddItem {weap_name} 1\n\
                 player.EquipItem {weap_name}"
            ),
        );
        let base = num(&mut g, "player.GetCalculatedActorSpread");
        assert!((base - 0.225).abs() < 1e-6, "{weap_name} baseline = {base}");

        q(&mut g, "player.CrippleLimb 3"); // right arm
        let r = num(&mut g, "player.GetCalculatedActorSpread");
        assert!((r - want_r).abs() < 1e-6, "{weap_name} right arm = {r}, want {want_r}");

        q(&mut g, "player.RestoreActorValue RightAttackCondition 100\nplayer.CrippleLimb 2"); // left arm only
        let l = num(&mut g, "player.GetCalculatedActorSpread");
        assert!((l - want_l).abs() < 1e-6, "{weap_name} left arm = {l}, want {want_l}");

        q(&mut g, "player.CrippleLimb 3"); // both arms
        let b = num(&mut g, "player.GetCalculatedActorSpread");
        assert!((b - want_b).abs() < 1e-6, "{weap_name} both arms = {b}, want {want_b}");
    }
}

#[test]
fn toggle_item_unique_and_is_item_unique_filter_repair_sources_and_ignore_alch() {
    let f = fixture();
    let mut g = game(&f);

    // Baseline: nothing unique.
    assert_eq!(num(&mut g, "IsItemUnique TestPistol"), 0.0);
    assert_eq!(num(&mut g, "IsItemUnique TestArmor"), 0.0);
    assert_eq!(num(&mut g, "IsItemUnique TestStimpak"), 0.0);

    // Non-repairable ALCH ignores ToggleItemUnique (matches data3 observation).
    q(&mut g, "ToggleItemUnique TestStimpak 1");
    assert_eq!(num(&mut g, "IsItemUnique TestStimpak"), 0.0);

    // Single WEAP toggles cleanly and reaches Game::is_item_unique & player_repair_list.
    assert!(g.player_repair_list(FormId(WEAP_PISTOL), []).contains(&FormId(WEAP_PISTOL_DONOR)));
    q(&mut g, "ToggleItemUnique TestPistolDonor 1");
    assert_eq!(num(&mut g, "IsItemUnique TestPistolDonor"), 1.0);
    assert!(g.is_item_unique(FormId(WEAP_PISTOL_DONOR)));
    assert!(
        !g.player_repair_list(FormId(WEAP_PISTOL), []).contains(&FormId(WEAP_PISTOL_DONOR)),
        "unique donor excluded from repair list"
    );
    // Unique item can still be repaired as the target using non-unique donors.
    assert!(
        g.player_repair_list(FormId(WEAP_PISTOL_DONOR), []).contains(&FormId(WEAP_PISTOL)),
        "unique target still accepts non-unique donor"
    );

    // Under Jury Rigging, unique weapons are also filtered out as donor sources.
    q(&mut g, "player.AddPerk TestJuryRigging");
    assert!(
        !g.player_repair_list(FormId(WEAP_PISTOL), [FormId(WEAP_PISTOL_DONOR)]).contains(&FormId(WEAP_PISTOL_DONOR)),
        "unique donor excluded under Jury Rigging"
    );

    q(&mut g, "ToggleItemUnique TestPistolDonor 0");
    assert_eq!(num(&mut g, "IsItemUnique TestPistolDonor"), 0.0);
    assert!(g.player_repair_list(FormId(WEAP_PISTOL), []).contains(&FormId(WEAP_PISTOL_DONOR)));

    // Form list toggles WEAP + ARMO members while skipping ALCH.
    q(&mut g, "ToggleItemUnique TestUniqueList 1");
    assert_eq!(num(&mut g, "IsItemUnique TestPistol"), 1.0);
    assert_eq!(num(&mut g, "IsItemUnique TestArmor"), 1.0);
    assert_eq!(num(&mut g, "IsItemUnique TestStimpak"), 0.0);

    q(&mut g, "ToggleItemUnique TestUniqueList 0");
    assert_eq!(num(&mut g, "IsItemUnique TestPistol"), 0.0);
    assert_eq!(num(&mut g, "IsItemUnique TestArmor"), 0.0);
}
