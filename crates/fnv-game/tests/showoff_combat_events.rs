//! ShowOff NVSE `ShowOff:OnFireWeapon` and `ForceWeaponJamAnim` / `ForceJamAnim`
//! rules-layer tests (docs/spec-notes/ext-showoff.md §4, §11) on a synthetic plugin.

use crate::common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::nvse_showoff::MAX_FORCE_JAM_REQUESTS;
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CELL_A: u32 = 0x200;
const GUARD_REF: FormId = FormId(0x300);
const RAIDER_REF: FormId = FormId(0x301);
const TRAP_REF: FormId = FormId(0x302);
const GUARD: u32 = 0x800;
const RAIDER: u32 = 0x801;
const PISTOL: u32 = 0x810;
const SHOTGUN: u32 = 0x811;
const REVOLVER_357: u32 = 0x812;
const KNIFE: u32 = 0x813;
const GRENADE: u32 = 0x814;
const BULLET: u32 = 0x818;
const TRAP: u32 = 0x820;
const NPC_LIST: u32 = 0x830;
const RANGED_LIST: u32 = 0x831;
const HIT_COUNT: u32 = 0xA00;
const ALT_PLAYER_UDF: u32 = 0xB00;
const CLASSIC_SHOTGUN_UDF: u32 = 0xB01;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn glob(id: u32, edid: &str) -> Vec<u8> {
    record(b"GLOB", 0, id, &[sub(b"EDID", z(edid)), sub(b"FNAM", vec![b'f']), sub(b"FLTV", 0f32.to_le_bytes().to_vec())])
}

fn npc(id: u32, edid: &str) -> Vec<u8> {
    let mut acbs = vec![0u8; 20];
    acbs.extend(0i16.to_le_bytes());
    acbs.extend(0u16.to_le_bytes());
    let data = [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat();
    record(b"NPC_", 0, id, &[sub(b"EDID", z(edid)), sub(b"ACBS", acbs), sub(b"DATA", data)])
}

fn weap(id: u32, edid: &str, anim: u32, reload_anim: u8, jam_time: f32) -> Vec<u8> {
    let data = [10i32.to_le_bytes().to_vec(), 100i32.to_le_bytes().to_vec(), 1.0f32.to_le_bytes().to_vec(), vec![0; 3]].concat();
    let mut dnam = vec![0u8; 204];
    dnam[0..4].copy_from_slice(&anim.to_le_bytes());
    dnam[15] = reload_anim;
    // WEAP DNAM `jam_time` sits at byte offset 96 (schemas/fnv/objects.json).
    dnam[96..100].copy_from_slice(&jam_time.to_le_bytes());
    record(b"WEAP", 0, id, &[sub(b"EDID", z(edid)), sub(b"DATA", data), sub(b"DNAM", dnam)])
}

fn flst(id: u32, edid: &str, members: &[u32]) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid))];
    for &m in members {
        subs.push(sub(b"LNAM", m.to_le_bytes().to_vec()));
    }
    record(b"FLST", 0, id, &subs)
}

fn udf(id: u32, edid: &str, source: &str) -> Vec<u8> {
    let mut fields = vec![sub(b"EDID", z(edid))];
    fields.extend(unit(0, &script(&[(0, None, Vec::new())]), &[], &[]));
    fields.push(sub(b"SCTX", source.as_bytes().to_vec()));
    record(b"SCPT", 0, id, &fields)
}

fn placed_actor(id: FormId, edid: &str, base: u32, x: f32) -> Vec<u8> {
    let data = [x, 0.0, 0.0, 0.0, 0.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    record(b"ACHR", 0, id.0, &[sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn placed_obj(id: FormId, edid: &str, base: u32, x: f32) -> Vec<u8> {
    let data = [x, 0.0, 0.0, 0.0, 0.0, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    record(b"REFR", 0, id.0, &[sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn fixture() -> Fixture {
    let globs = glob(HIT_COUNT, "ShowOffFireHits");
    let npcs = [npc(0x7, "Player"), npc(GUARD, "TestGuard"), npc(RAIDER, "TestRaider")].concat();
    let weaps = [
        weap(PISTOL, "TestPistol", 3, 0, 1.4),
        weap(SHOTGUN, "TestShotgun", 5, 11, 1.6),
        weap(REVOLVER_357, "TestRevolver357", 3, 20, 0.0),
        weap(KNIFE, "TestKnife", 1, 0, 0.0),
        weap(GRENADE, "TestGrenade", 10, 0, 1.4),
    ]
    .concat();
    let proj = record(b"PROJ", 0, BULLET, &[sub(b"EDID", z("TestBullet"))]);
    let trap = record(b"ACTI", 0, TRAP, &[sub(b"EDID", z("TestTrap"))]);
    let scpts = [
        udf(
            ALT_PLAYER_UDF,
            "AltPlayerUdf",
            "scn AltPlayerUdf\nref rWeapon\nstring_var sMarker\nBegin Function {rWeapon}\n  set ShowOffFireHits to ShowOffFireHits + 1\nEnd\n",
        ),
        udf(
            CLASSIC_SHOTGUN_UDF,
            "ClassicShotgunUdf",
            "scn ClassicShotgunUdf\nref rWeapon\nstring_var sMarker\nBegin Function {rWeapon}\n  PlayerREF.AuxVarSetRef \"*_cls_a\" (GetSelfAlt) -1\n  PlayerREF.AuxVarSetRef \"*_cls_w\" rWeapon -1\nEnd\n",
        ),
    ]
    .concat();
    let lists = [
        flst(NPC_LIST, "NpcActors", &[GUARD_REF.0, RAIDER_REF.0]),
        flst(RANGED_LIST, "RangedList", &[PISTOL, SHOTGUN]),
    ]
    .concat();
    let refs = [
        placed_actor(FormId::PLAYER_REF, "PlayerREF", 0x7, 0.0),
        placed_actor(GUARD_REF, "GuardRef", GUARD, 0.0),
        placed_actor(RAIDER_REF, "RaiderRef", RAIDER, 200.0),
        placed_obj(TRAP_REF, "TrapRef", TRAP, 400.0),
    ]
    .concat();
    let cell = [
        record(b"CELL", 0, CELL_A, &[sub(b"EDID", z("TestCellA"))]),
        group(CELL_A.to_le_bytes(), 6, &group(CELL_A.to_le_bytes(), 9, &refs)),
    ]
    .concat();
    let bytes = [
        tes4(),
        top(b"GLOB", &globs),
        top(b"SCPT", &scpts),
        top(b"ACTI", &trap),
        top(b"WEAP", &weaps),
        top(b"PROJ", &proj),
        top(b"NPC_", &npcs),
        top(b"FLST", &lists),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell))),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "ShowOffCombatEvents.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    assert!(data.source_units.iter().all(|u| u.error.is_none()), "UDF source compile errors: {:?}", data.source_units);
    for id in [ALT_PLAYER_UDF, CLASSIC_SHOTGUN_UDF] {
        let unit = data.script(FormId(id)).unwrap_or_else(|| panic!("missing fixture SCPT 0x{id:08X}"));
        assert!(unit.unit.source.is_some(), "UDF 0x{id:08X} must compile from SCTX");
    }
    assert_eq!(data.source_units.len(), 2, "both fixture UDFs must compile from SCTX: {:?}", data.source_units);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() })
}

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() })
        .unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn num(g: &mut Game, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(n) => n,
        v => panic!("{expr}: {v:?}"),
    }
}

fn aux_size(g: &mut Game, key: &str) -> f64 {
    num(g, &format!("PlayerREF.AuxVarSize \"{key}\""))
}

fn aux_flt(g: &mut Game, key: &str, idx: u32) -> f64 {
    num(g, &format!("PlayerREF.AuxVarGetFlt \"{key}\" {idx}"))
}

fn aux_ref(g: &mut Game, key: &str, idx: u32) -> FormId {
    match q(g, &format!("SetFunctionValue (PlayerREF.AuxVarGetRef \"{key}\" {idx})")) {
        XVal::Ref(r) => FormId(r),
        v => panic!("AuxVarGetRef {key}[{idx}]: {v:?}"),
    }
}

fn global(g: &Game, id: u32) -> f32 {
    g.world.global(FormId(id)).unwrap()
}

#[test]
fn showoff_on_fire_weapon_filters_dedup_god_mode_and_session_lifecycle() {
    let f = fixture();
    let mut g = game(&f);

    // Vanilla-unchanged control: firing with no ShowOff:OnFireWeapon handler registered does no work.
    assert!(!g.weapon_fired(FormId::PLAYER_REF, FormId(PISTOL)));
    assert!(g.projectile_fired(FormId(BULLET), FormId::PLAYER_REF, FormId(PISTOL), 10.0, [0.0; 3]).is_some());
    assert!(g.showoff.fired_this_frame.1.is_empty(), "unregistered ShowOff:OnFireWeapon stays zero-cost");
    g.tick();

    // Register:
    // - Alt 0::Player (base NPC_ 0x7 matching PlayerREF)
    // - Alt 0::PlayerREF 1::TestPistol (exact actor + weapon)
    // - Alt 0::NpcActors 1::RangedList (FormList actor + weapon)
    // - Unfiltered with priority 5 vs priority 1 + zero-arg handler
    // - Classic SetEventHandler with "first"::TestShotgun (survives session load)
    // - JIP SetOnFireWeaponEventHandler control to confirm JIP god-mode exclusion is preserved
    q(
        &mut g,
        "SetEventHandlerAlt \"ShowOff:OnFireWeapon\" AltPlayerUdf 0::Player\n\
         SetEventHandlerAlt \"ShowOff:OnFireWeapon\" (Begin Function {ref rW}\n\
           PlayerREF.AuxVarSetFlt \"*prio\" 5 -1\n\
         End) \"priority\"::5\n\
         SetEventHandlerAlt \"ShowOff:OnFireWeapon\" (Begin Function {ref rW}\n\
           PlayerREF.AuxVarSetFlt \"*prio\" 1 -1\n\
           PlayerREF.AuxVarSetRef \"*all_a\" (GetSelfAlt) -1\n\
           PlayerREF.AuxVarSetRef \"*all_w\" rW -1\n\
         End) \"priority\"::1\n\
         SetEventHandlerAlt \"ShowOff:OnFireWeapon\" (Begin Function {}\n\
           PlayerREF.AuxVarSetFlt \"*zero\" 1 -1\n\
         End)\n\
         SetEventHandlerAlt \"ShowOff:OnFireWeapon\" (Begin Function {ref rW}\n\
           PlayerREF.AuxVarSetRef \"*pp_w\" rW -1\n\
         End) 0::PlayerREF 1::TestPistol\n\
         SetEventHandlerAlt \"ShowOff:OnFireWeapon\" (Begin Function {ref rW}\n\
           PlayerREF.AuxVarSetRef \"*npc_a\" (GetSelf) -1\n\
           PlayerREF.AuxVarSetRef \"*npc_w\" rW -1\n\
         End) 0::NpcActors 1::RangedList\n\
         SetEventHandler \"ShowOff:OnFireWeapon\" ClassicShotgunUdf \"first\"::TestShotgun\n\
         SetOnFireWeaponEventHandler (Begin Function {ref rW}\n\
           PlayerREF.AuxVarSetFlt \"*jip\" 1 -1\n\
         End) 1 PlayerREF",
    );

    // Player fires pistol: weapon_fired + projectile_fired in same frame deduplicate to 1 dispatch.
    g.weapon_fired(FormId::PLAYER_REF, FormId(PISTOL));
    g.projectile_fired(FormId(BULLET), FormId::PLAYER_REF, FormId(PISTOL), 10.0, [0.0; 3]);
    assert_eq!(aux_size(&mut g, "*all_a"), 1.0, "player shot fires once across weapon_fired + projectile_fired");
    assert_eq!(aux_ref(&mut g, "*all_a", 0), FormId::PLAYER_REF);
    assert_eq!(aux_ref(&mut g, "*all_w", 0), FormId(PISTOL));
    assert_eq!((aux_flt(&mut g, "*prio", 0), aux_flt(&mut g, "*prio", 1)), (5.0, 1.0), "higher priority runs first");
    assert_eq!(aux_size(&mut g, "*zero"), 1.0, "zero-arg handler runs");
    assert_eq!(global(&g, HIT_COUNT), 1.0, "0::Player base filter matches PlayerREF");
    assert_eq!(aux_size(&mut g, "*pp_w"), 1.0, "0::PlayerREF 1::TestPistol matches");
    assert_eq!(aux_size(&mut g, "*npc_a"), 0.0, "player does not match NpcActors");
    assert_eq!(aux_size(&mut g, "*_cls_a"), 0.0, "pistol does not match first::TestShotgun");
    assert_eq!(aux_size(&mut g, "*jip"), 1.0);

    // NPC multi-pellet shotgun burst in one frame fires once and matches NpcActors + RangedList + classic first::TestShotgun.
    for _ in 0..4 {
        g.projectile_fired(FormId(BULLET), GUARD_REF, FormId(SHOTGUN), 5.0, [0.0; 3]);
    }
    assert_eq!(aux_size(&mut g, "*all_a"), 2.0);
    assert_eq!(aux_ref(&mut g, "*all_a", 1), GUARD_REF);
    assert_eq!(aux_ref(&mut g, "*all_w", 1), FormId(SHOTGUN));
    assert_eq!(aux_size(&mut g, "*npc_a"), 1.0);
    assert_eq!(aux_ref(&mut g, "*npc_a", 0), GUARD_REF);
    assert_eq!(aux_ref(&mut g, "*npc_w", 0), FormId(SHOTGUN));
    assert_eq!(aux_size(&mut g, "*_cls_a"), 1.0);
    assert_eq!(aux_ref(&mut g, "*_cls_a", 0), GUARD_REF);
    assert_eq!(aux_ref(&mut g, "*_cls_w", 0), FormId(SHOTGUN));
    assert_eq!(global(&g, HIT_COUNT), 1.0, "NPC does not match 0::Player");

    // Next frame: same NPC can fire again.
    g.tick();
    g.projectile_fired(FormId(BULLET), GUARD_REF, FormId(SHOTGUN), 5.0, [0.0; 3]);
    assert_eq!(aux_size(&mut g, "*npc_a"), 2.0);

    // God mode: ShowOff:OnFireWeapon still fires, while JIP SetOnFireWeaponEventHandler stays suppressed.
    g.tick();
    q(&mut g, "ToggleGodMode");
    g.weapon_fired(FormId::PLAYER_REF, FormId(PISTOL));
    g.projectile_fired(FormId(BULLET), FormId::PLAYER_REF, FormId(PISTOL), 10.0, [0.0; 3]);
    assert_eq!(global(&g, HIT_COUNT), 2.0, "ShowOff:OnFireWeapon fires in god mode");
    assert_eq!(aux_size(&mut g, "*jip"), 1.0, "JIP SetOnFireWeaponEventHandler stays suppressed in god mode");
    q(&mut g, "ToggleGodMode");

    // Melee weapons and non-actor traps do not fire ShowOff:OnFireWeapon.
    g.tick();
    g.weapon_fired(FormId::PLAYER_REF, FormId(KNIFE));
    g.projectile_fired(FormId(BULLET), TRAP_REF, FormId(PISTOL), 10.0, [400.0, 0.0, 0.0]);
    assert_eq!(global(&g, HIT_COUNT), 2.0);
    assert_eq!(aux_size(&mut g, "*all_a"), 4.0);

    // RemoveEventHandler removes AltPlayerUdf while other handlers keep running.
    q(&mut g, "RemoveEventHandler \"ShowOff:OnFireWeapon\" AltPlayerUdf");
    g.tick();
    g.weapon_fired(FormId::PLAYER_REF, FormId(PISTOL));
    assert_eq!(global(&g, HIT_COUNT), 2.0, "removed AltPlayerUdf no longer runs");
    assert_eq!(aux_size(&mut g, "*all_a"), 5.0);

    // Session load flushes SetEventHandlerAlt registrations and keeps classic SetEventHandler.
    let session = g.take_session();
    let mut g2 = game(&f);
    g2.put_session(session);
    g2.weapon_fired(FormId::PLAYER_REF, FormId(SHOTGUN));
    assert_eq!(aux_size(&mut g2, "*all_a"), 0.0, "SetEventHandlerAlt flushed on load");
    assert_eq!(aux_size(&mut g2, "*_cls_a"), 1.0, "classic SetEventHandler survives load");
    assert_eq!(aux_ref(&mut g2, "*_cls_a", 0), FormId::PLAYER_REF);
    assert_eq!(aux_ref(&mut g2, "*_cls_w", 0), FormId(SHOTGUN));
}

#[test]
fn force_weapon_jam_anim_checks_weapon_jam_capability_overrides_and_cap() {
    let f = fixture();
    let mut g = game(&f);

    // Equipped jam-capable pistol, holstered -> 0 and no queued request.
    g.world.add_item(FormId::PLAYER_REF, FormId(PISTOL), 1).unwrap();
    g.world.equip(FormId::PLAYER_REF, FormId(PISTOL)).unwrap();
    g.set_weapon_out(FormId::PLAYER_REF, false);
    assert_eq!(num(&mut g, "PlayerREF.ForceWeaponJamAnim"), 0.0);
    assert_eq!(num(&mut g, "PlayerREF.ForceJamAnim"), 0.0);
    assert!(g.take_force_jam_requests().is_empty());

    // Drawn jam-capable pistol -> 1; repeated call deduplicates actor in the queue,
    // immediately enters AnimAction::Jam, reports GetSequenceAnimGroup 4 == 200 (JamA),
    // and fires SetOnReloadWeaponEventHandler once with slot 4 == 200 inside callback
    // [observed 2026-11-24: w6t2_14_jip_on_reload_and_seq_anim_group].
    q(
        &mut g,
        "SetOnReloadWeaponEventHandler (Begin Function {ref rW}\n\
           PlayerREF.AuxVarSetFlt \"*jam_rl\" ((PlayerREF.AuxVarGetFlt \"*jam_rl\") + 1)\n\
           PlayerREF.AuxVarSetFlt \"*jam_s4cb\" (PlayerREF.GetSequenceAnimGroup 4)\n\
         End) 1",
    );
    assert_eq!(num(&mut g, "PlayerREF.GetSequenceAnimGroup 4"), -1.0, "holstered slot 4 is -1");
    g.set_weapon_out(FormId::PLAYER_REF, true);
    assert_eq!(num(&mut g, "PlayerREF.GetSequenceAnimGroup 4"), 24.0, "drawn weapon slot 4 is 24 (Equip)");
    assert_eq!(num(&mut g, "PlayerREF.ForceWeaponJamAnim"), 1.0);
    assert_eq!(num(&mut g, "PlayerREF.GetSequenceAnimGroup 4"), 200.0, "jammed pistol slot 4 is 200 (JamA)");
    assert_eq!(num(&mut g, "PlayerREF.AuxVarGetFlt \"*jam_rl\""), 1.0, "ForceWeaponJamAnim fires SetOnReloadWeaponEventHandler once");
    assert_eq!(num(&mut g, "PlayerREF.AuxVarGetFlt \"*jam_s4cb\""), 200.0, "callback sees GetSequenceAnimGroup 4 == 200");
    assert_eq!(num(&mut g, "PlayerREF.ForceJamAnim"), 1.0);
    assert_eq!(num(&mut g, "PlayerREF.AuxVarGetFlt \"*jam_rl\""), 1.0, "repeat ForceJamAnim while jamming does not double-fire");
    assert_eq!(g.take_force_jam_requests(), vec![FormId::PLAYER_REF]);
    assert!(g.take_force_jam_requests().is_empty());

    // Re-calling ForceWeaponJamAnim / ForceJamAnim while already jamming (AnimAction::Jam,
    // GetAnimAction == 9, IsAnimPlayingEx 4 == 0) still returns 1.
    g.set_anim_state(
        FormId::PLAYER_REF,
        fnv_game::anim_state::AnimState { action: Some(fnv_game::anim_state::AnimAction::Jam), ..Default::default() },
    );
    assert_eq!(num(&mut g, "PlayerREF.GetAnimAction"), 9.0);
    assert_eq!(num(&mut g, "PlayerREF.IsAnimPlayingEx 4"), 0.0);
    assert_eq!(num(&mut g, "PlayerREF.ForceWeaponJamAnim"), 1.0);
    assert_eq!(num(&mut g, "PlayerREF.ForceJamAnim"), 1.0);
    assert_eq!(g.take_force_jam_requests(), vec![FormId::PLAYER_REF]);

    // Drawn .357-like revolver (jam_time == 0.0) -> 0 and no queued request.
    g.world.add_item(FormId::PLAYER_REF, FormId(REVOLVER_357), 1).unwrap();
    g.world.equip(FormId::PLAYER_REF, FormId(REVOLVER_357)).unwrap();
    assert_eq!(num(&mut g, "PlayerREF.ForceWeaponJamAnim"), 0.0);
    assert!(g.take_force_jam_requests().is_empty());

    // Runtime SetWeaponAnimJamTime override enables jam, then resetting to 0 disables it again.
    q(&mut g, "SetWeaponAnimJamTime 1.25 TestRevolver357");
    assert_eq!(num(&mut g, "PlayerREF.ForceJamAnim"), 1.0);
    assert_eq!(g.take_force_jam_requests(), vec![FormId::PLAYER_REF]);
    q(&mut g, "SetWeaponAnimJamTime 0 TestRevolver357");
    assert_eq!(num(&mut g, "PlayerREF.ForceJamAnim"), 0.0);
    assert!(g.take_force_jam_requests().is_empty());

    // Unarmed / fists (no equipped weapon, weapon drawn) -> 0.
    g.world.unequip(FormId::PLAYER_REF, FormId(REVOLVER_357)).unwrap();
    assert_eq!(num(&mut g, "PlayerREF.ForceWeaponJamAnim"), 0.0);
    assert!(g.take_force_jam_requests().is_empty());

    // Drawn melee (animation_type 1) and thrown grenade (animation_type 10, even with jam_time > 0) -> 0.
    g.world.add_item(FormId::PLAYER_REF, FormId(KNIFE), 1).unwrap();
    g.world.equip(FormId::PLAYER_REF, FormId(KNIFE)).unwrap();
    assert_eq!(num(&mut g, "PlayerREF.ForceWeaponJamAnim"), 0.0);
    g.world.add_item(FormId::PLAYER_REF, FormId(GRENADE), 1).unwrap();
    g.world.equip(FormId::PLAYER_REF, FormId(GRENADE)).unwrap();
    assert_eq!(num(&mut g, "PlayerREF.ForceWeaponJamAnim"), 0.0);
    assert!(g.take_force_jam_requests().is_empty());

    // TempCloneForm of jam-capable pistol inherits DNAM jam_time & animation_type -> 1.
    let cloned = match q(&mut g, "ref rClone\nlet rClone := TempCloneForm TestPistol\nPlayerREF.AddItem rClone 1 1\nPlayerREF.EquipItem rClone 0 1\nSetFunctionValue rClone") {
        XVal::Ref(r) => FormId(r),
        v => panic!("TempCloneForm: {v:?}"),
    };
    assert_ne!(cloned, FormId(PISTOL));
    assert_eq!(num(&mut g, "PlayerREF.ForceWeaponJamAnim"), 1.0);
    assert_eq!(g.take_force_jam_requests(), vec![FormId::PLAYER_REF]);

    // NPC with drawn jam-capable pistol -> 1 and queues GuardRef; dead NPC -> 0.
    g.world.add_item(GUARD_REF, FormId(PISTOL), 1).unwrap();
    g.world.equip(GUARD_REF, FormId(PISTOL)).unwrap();
    g.set_weapon_out(GUARD_REF, true);
    assert_eq!(num(&mut g, "GuardRef.ForceJamAnim"), 1.0);
    assert_eq!(g.take_force_jam_requests(), vec![GUARD_REF]);
    assert!(g.kill(GUARD_REF, None));
    assert_eq!(num(&mut g, "GuardRef.ForceJamAnim"), 0.0);
    assert!(g.take_force_jam_requests().is_empty());

    // Safety-cap boundary: unconsumed headless queue never exceeds MAX_FORCE_JAM_REQUESTS.
    for i in 0..(MAX_FORCE_JAM_REQUESTS + 8) {
        let actor = g.world.place(FormId(RAIDER), Some(FormId(CELL_A)), [i as f32, 0.0, 0.0], [0.0; 3], 1).unwrap();
        g.world.add_item(actor, FormId(PISTOL), 1).unwrap();
        g.world.equip(actor, FormId(PISTOL)).unwrap();
        g.set_weapon_out(actor, true);
        let ret = g
            .run_query(
                "Begin Function { }\nSetFunctionValue (ForceJamAnim)\nEnd\n",
                RunCtx { self_ref: Some(actor), ..Default::default() },
            )
            .unwrap();
        assert_eq!(ret, XVal::Num(1.0));
    }
    let queued = g.take_force_jam_requests();
    assert_eq!(queued.len(), MAX_FORCE_JAM_REQUESTS, "headless queue caps at MAX_FORCE_JAM_REQUESTS");
}
