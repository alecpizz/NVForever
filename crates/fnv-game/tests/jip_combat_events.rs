//! JIP LN combat & weapon event handlers (`SetOnFireWeaponEventHandler`,
//! `SetOnReloadWeaponEventHandler`, `SetOnCriticalHitEventHandler`;
//! docs/spec-notes/jip-events.md §2, §5; Part of #2272) on a synthetic plugin.

use crate::common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::anim_state::{AnimAction, AnimState};
use fnv_game::combat::HitInfo;
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const XMARKER: u32 = 0x3B;
const CELL_A: u32 = 0x200;
const GUARD_REF: FormId = FormId(0x300);
const RAIDER_REF: FormId = FormId(0x301);
const TRAP_REF: FormId = FormId(0x302);
const GUARD: u32 = 0x800;
const RAIDER: u32 = 0x801;
const PISTOL: u32 = 0x810;
const SHOTGUN: u32 = 0x811;
const KNIFE: u32 = 0x812;
const BULLET: u32 = 0x818;
const BLAST: u32 = 0x819;
const TRAP: u32 = 0x820;
const NPC_LIST: u32 = 0x830;
const RANGED_LIST: u32 = 0x831;
const PLAYER_FIRE_HITS: u32 = 0xA00;
const RELOAD_HITS: u32 = 0xA01;
const CRIT_WILDCARD_HITS: u32 = 0xA02;
const PLAYER_FIRE_UDF: u32 = 0xB00;
const RELOAD_UDF: u32 = 0xB01;
const CRIT_WILDCARD_UDF: u32 = 0xB02;

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

fn weap(id: u32, edid: &str, anim: u32) -> Vec<u8> {
    let data = [10i32.to_le_bytes().to_vec(), 100i32.to_le_bytes().to_vec(), 1.0f32.to_le_bytes().to_vec(), vec![0; 3]].concat();
    let mut dnam = anim.to_le_bytes().to_vec();
    dnam.resize(204, 0);
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
    let globs = [
        glob(PLAYER_FIRE_HITS, "PlayerFireHits"),
        glob(RELOAD_HITS, "ReloadHits"),
        glob(CRIT_WILDCARD_HITS, "CritWildcardHits"),
    ]
    .concat();
    let npcs = [npc(0x7, "Player"), npc(GUARD, "TestGuard"), npc(RAIDER, "TestRaider")].concat();
    let weaps = [weap(PISTOL, "TestPistol", 3), weap(SHOTGUN, "TestShotgun", 5), weap(KNIFE, "TestKnife", 1)].concat();
    let proj = record(b"PROJ", 0, BULLET, &[sub(b"EDID", z("TestBullet"))]);
    let expl = record(b"EXPL", 0, BLAST, &[sub(b"EDID", z("TestBlast"))]);
    let xmarker = record(b"STAT", 0, XMARKER, &[sub(b"EDID", z("XMarker"))]);
    let trap = record(b"ACTI", 0, TRAP, &[sub(b"EDID", z("TestTrap"))]);
    let scpts = [
        udf(
            PLAYER_FIRE_UDF,
            "PlayerFireUdf",
            "scn PlayerFireUdf\nref rWeapon\nstring_var sMarker\nBegin Function {rWeapon}\n  set PlayerFireHits to PlayerFireHits + 1\nEnd\n",
        ),
        udf(
            RELOAD_UDF,
            "ReloadUdf",
            "scn ReloadUdf\nref rWeapon\nstring_var sMarker\nBegin Function {rWeapon}\n  set ReloadHits to ReloadHits + 1\nEnd\n",
        ),
        udf(
            CRIT_WILDCARD_UDF,
            "CritWildcardUdf",
            "scn CritWildcardUdf\nref rSource\nref rWeapon\nstring_var sMarker\nBegin Function {rSource, rWeapon}\n  set CritWildcardHits to CritWildcardHits + 1\nEnd\n",
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
        top(b"STAT", &xmarker),
        top(b"ACTI", &trap),
        top(b"WEAP", &weaps),
        top(b"PROJ", &proj),
        top(b"EXPL", &expl),
        top(b"NPC_", &npcs),
        top(b"FLST", &lists),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell))),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "JipCombatEvents.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    assert!(data.source_units.iter().all(|u| u.error.is_none()), "UDF source compile errors: {:?}", data.source_units);
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

fn aux_size(g: &mut Game, key: &str) -> f64 {
    match q(g, &format!("SetFunctionValue (PlayerREF.AuxVarSize \"{key}\")")) {
        XVal::Num(n) => n,
        v => panic!("AuxVarSize {key}: {v:?}"),
    }
}

fn aux_flt(g: &mut Game, key: &str, idx: u32) -> f64 {
    match q(g, &format!("SetFunctionValue (PlayerREF.AuxVarGetFlt \"{key}\" {idx})")) {
        XVal::Num(n) => n,
        v => panic!("AuxVarGetFlt {key}[{idx}]: {v:?}"),
    }
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
fn fire_weapon_event_filters_god_mode_and_same_frame_dedup() {
    let f = fixture();
    let mut g = game(&f);

    // Register named PlayerREF-filtered UDF + inline unfiltered, FormList, and RaiderRef handlers.
    q(
        &mut g,
        "SetOnFireWeaponEventHandler PlayerFireUdf 1 PlayerREF\n\
         SetOnFireWeaponEventHandler (Begin Function {ref rWeapon}\n\
           PlayerREF.AuxVarSetRef \"*fa_a\" (GetSelf) -1\n\
           PlayerREF.AuxVarSetRef \"*fa_w\" rWeapon -1\n\
         End) 1\n\
         SetOnFireWeaponEventHandler (Begin Function {ref rWeapon}\n\
           PlayerREF.AuxVarSetRef \"*fl_a\" (GetSelf) -1\n\
           PlayerREF.AuxVarSetRef \"*fl_w\" rWeapon -1\n\
         End) 1 NpcActors\n\
         SetOnFireWeaponEventHandler (Begin Function {ref rWeapon}\n\
           PlayerREF.AuxVarSetRef \"*fr_w\" rWeapon -1\n\
         End) 1 RaiderRef",
    );

    // Player fires pistol: weapon_fired followed by projectile_fired in the same frame fires once.
    g.weapon_fired(FormId::PLAYER_REF, FormId(PISTOL));
    g.projectile_fired(FormId(BULLET), FormId::PLAYER_REF, FormId(PISTOL), 10.0, [0.0; 3]);
    assert_eq!(aux_size(&mut g, "*fa_a"), 1.0, "player shot fires once across weapon_fired + projectile_fired");
    assert_eq!(aux_ref(&mut g, "*fa_a", 0), FormId::PLAYER_REF);
    assert_eq!(aux_ref(&mut g, "*fa_w", 0), FormId(PISTOL));
    assert_eq!(global(&g, PLAYER_FIRE_HITS), 1.0);
    assert_eq!(aux_size(&mut g, "*fl_a"), 0.0, "player does not match NpcActors list");
    assert_eq!(aux_size(&mut g, "*fr_w"), 0.0, "player does not match RaiderRef");

    // Player shotgun burst: weapon_fired + 3 pellets in the same frame fires once.
    g.weapon_fired(FormId::PLAYER_REF, FormId(SHOTGUN));
    for _ in 0..3 {
        g.projectile_fired(FormId(BULLET), FormId::PLAYER_REF, FormId(SHOTGUN), 5.0, [0.0; 3]);
    }
    assert_eq!(aux_size(&mut g, "*fa_a"), 2.0);
    assert_eq!(aux_ref(&mut g, "*fa_w", 1), FormId(SHOTGUN));
    assert_eq!(global(&g, PLAYER_FIRE_HITS), 2.0);

    // NPC shotgun burst via projectile_fired (4 pellets in one frame) fires once and matches NpcActors only.
    for _ in 0..4 {
        g.projectile_fired(FormId(BULLET), GUARD_REF, FormId(SHOTGUN), 5.0, [0.0; 3]);
    }
    assert_eq!(aux_size(&mut g, "*fa_a"), 3.0);
    assert_eq!(aux_ref(&mut g, "*fa_a", 2), GUARD_REF);
    assert_eq!(aux_ref(&mut g, "*fa_w", 2), FormId(SHOTGUN));
    assert_eq!(aux_size(&mut g, "*fl_a"), 1.0);
    assert_eq!(aux_ref(&mut g, "*fl_a", 0), GUARD_REF);
    assert_eq!(global(&g, PLAYER_FIRE_HITS), 2.0, "NPC does not match PlayerREF filter");
    assert_eq!(aux_size(&mut g, "*fr_w"), 0.0, "GuardRef does not match RaiderRef filter");

    // Advancing a frame lets the same NPC fire again.
    g.tick();
    g.projectile_fired(FormId(BULLET), GUARD_REF, FormId(SHOTGUN), 5.0, [0.0; 3]);
    assert_eq!(aux_size(&mut g, "*fa_a"), 4.0);
    assert_eq!(aux_size(&mut g, "*fl_a"), 2.0);

    // God mode blocks player fireweapon [documented: SetOnFireWeaponEventHandler]; demigod (TIM) still fires.
    g.tick();
    q(&mut g, "ToggleGodMode");
    g.weapon_fired(FormId::PLAYER_REF, FormId(PISTOL));
    g.projectile_fired(FormId(BULLET), FormId::PLAYER_REF, FormId(PISTOL), 10.0, [0.0; 3]);
    assert_eq!(aux_size(&mut g, "*fa_a"), 4.0, "god mode suppresses player fireweapon");
    q(&mut g, "ToggleGodMode\nToggleImmortalMode 1");
    g.weapon_fired(FormId::PLAYER_REF, FormId(PISTOL));
    assert_eq!(aux_size(&mut g, "*fa_a"), 5.0, "demigod / TIM still fires fireweapon");
    assert_eq!(global(&g, PLAYER_FIRE_HITS), 3.0);

    // Melee weapons and non-actor traps do not fire fireweapon.
    g.tick();
    g.weapon_fired(FormId::PLAYER_REF, FormId(KNIFE));
    g.projectile_fired(FormId(BULLET), TRAP_REF, FormId(PISTOL), 10.0, [400.0, 0.0, 0.0]);
    assert_eq!(aux_size(&mut g, "*fa_a"), 5.0);

    // Unregistering the PlayerREF-filtered handler stops it while unfiltered keeps firing.
    q(&mut g, "SetOnFireWeaponEventHandler PlayerFireUdf 0 PlayerREF");
    g.tick();
    g.weapon_fired(FormId::PLAYER_REF, FormId(PISTOL));
    assert_eq!(aux_size(&mut g, "*fa_a"), 6.0);
    assert_eq!(global(&g, PLAYER_FIRE_HITS), 3.0, "removed PlayerREF handler no longer runs");
}

#[test]
fn reload_weapon_event_fires_once_per_reload_for_player_and_npcs() {
    let f = fixture();
    let mut g = game(&f);

    g.world.add_item(FormId::PLAYER_REF, FormId(PISTOL), 1).unwrap();
    g.world.equip(FormId::PLAYER_REF, FormId(PISTOL)).unwrap();
    g.world.add_item(GUARD_REF, FormId(SHOTGUN), 1).unwrap();
    g.world.equip(GUARD_REF, FormId(SHOTGUN)).unwrap();
    g.set_held_weapon(RAIDER_REF, Some(FormId(PISTOL)));

    q(
        &mut g,
        "SetOnReloadWeaponEventHandler ReloadUdf 1\n\
         SetOnReloadWeaponEventHandler (Begin Function {ref rWeapon}\n\
           PlayerREF.AuxVarSetRef \"*rl_a\" (GetSelf) -1\n\
           PlayerREF.AuxVarSetRef \"*rl_w\" rWeapon -1\n\
           PlayerREF.AuxVarSetFlt \"*rl_act\" (GetAnimAction) -1\n\
         End) 1",
    );

    let anim = |action: Option<AnimAction>| AnimState { action, ..AnimState::default() };

    // Idle -> Reload fires once; GetAnimAction inside the handler sees 9 (Reload).
    g.set_anim_state(FormId::PLAYER_REF, anim(None));
    g.set_anim_state(FormId::PLAYER_REF, anim(Some(AnimAction::Reload)));
    assert_eq!(aux_size(&mut g, "*rl_a"), 1.0);
    assert_eq!(aux_ref(&mut g, "*rl_a", 0), FormId::PLAYER_REF);
    assert_eq!(aux_ref(&mut g, "*rl_w", 0), FormId(PISTOL));
    assert_eq!(aux_flt(&mut g, "*rl_act", 0), 9.0);
    assert_eq!(global(&g, RELOAD_HITS), 1.0);

    // Staying in Reload across frames does not re-fire.
    g.set_anim_state(FormId::PLAYER_REF, anim(Some(AnimAction::Reload)));
    assert_eq!(aux_size(&mut g, "*rl_a"), 1.0);
    assert_eq!(global(&g, RELOAD_HITS), 1.0);

    // Non-jam ForceScriptAnim does not fire reloadweapon.
    g.set_anim_state(FormId::PLAYER_REF, anim(None));
    g.set_anim_state(FormId::PLAYER_REF, anim(Some(AnimAction::ForceScriptAnim)));
    assert_eq!(aux_size(&mut g, "*rl_a"), 1.0);
    assert_eq!(global(&g, RELOAD_HITS), 1.0);

    // Entering AnimAction::Jam DOES fire reloadweapon once while GetSequenceAnimGroup 4 reads 200 (JamA)
    // [observed 2026-11-24: w6t2_14_jip_on_reload_and_seq_anim_group].
    g.set_anim_state(FormId::PLAYER_REF, anim(Some(AnimAction::Jam)));
    g.set_anim_state(FormId::PLAYER_REF, anim(Some(AnimAction::Jam)));
    assert_eq!(aux_size(&mut g, "*rl_a"), 2.0);
    assert_eq!(aux_flt(&mut g, "*rl_act", 1), 9.0);
    assert_eq!(global(&g, RELOAD_HITS), 2.0);

    // Round-by-round reload (ReloadOpen -> Reload -> None) fires once on ReloadOpen (17).
    g.set_anim_state(FormId::PLAYER_REF, anim(None));
    g.set_anim_state(FormId::PLAYER_REF, anim(Some(AnimAction::ReloadOpen)));
    g.set_anim_state(FormId::PLAYER_REF, anim(Some(AnimAction::Reload)));
    g.set_anim_state(FormId::PLAYER_REF, anim(None));
    assert_eq!(aux_size(&mut g, "*rl_a"), 3.0);
    assert_eq!(aux_flt(&mut g, "*rl_act", 2), 17.0);
    assert_eq!(global(&g, RELOAD_HITS), 3.0);

    // NPC with equipped weapon and NPC with held_weapon fallback both fire reloadweapon.
    g.set_anim_state(GUARD_REF, anim(Some(AnimAction::Reload)));
    assert_eq!(aux_size(&mut g, "*rl_a"), 4.0);
    assert_eq!(aux_ref(&mut g, "*rl_a", 3), GUARD_REF);
    assert_eq!(aux_ref(&mut g, "*rl_w", 3), FormId(SHOTGUN));
    assert_eq!(global(&g, RELOAD_HITS), 4.0);

    g.set_anim_state(RAIDER_REF, anim(Some(AnimAction::Reload)));
    assert_eq!(aux_size(&mut g, "*rl_a"), 5.0);
    assert_eq!(aux_ref(&mut g, "*rl_a", 4), RAIDER_REF);
    assert_eq!(aux_ref(&mut g, "*rl_w", 4), FormId(PISTOL));
    assert_eq!(global(&g, RELOAD_HITS), 5.0);

    // Actor with no equipped or held weapon entering Reload does not fire with form 0.
    g.set_anim_state(RAIDER_REF, anim(None));
    g.set_held_weapon(RAIDER_REF, None);
    g.set_anim_state(RAIDER_REF, anim(Some(AnimAction::Reload)));
    assert_eq!(aux_size(&mut g, "*rl_a"), 5.0);
    assert_eq!(global(&g, RELOAD_HITS), 5.0);

    // Removing the named handler stops ReloadUdf while the inline handler still fires.
    q(&mut g, "SetOnReloadWeaponEventHandler ReloadUdf 0");
    g.set_anim_state(FormId::PLAYER_REF, anim(Some(AnimAction::Reload)));
    assert_eq!(aux_size(&mut g, "*rl_a"), 6.0);
    assert_eq!(global(&g, RELOAD_HITS), 5.0, "removed ReloadUdf no longer runs");
}

#[test]
fn critical_hit_event_filters_normal_vs_crit_and_hit_location() {
    let f = fixture();
    let mut g = game(&f);

    q(
        &mut g,
        "SetOnCriticalHitEventHandler CritWildcardUdf 1 XMarker PlayerREF TestPistol\n\
         SetOnCriticalHitEventHandler (Begin Function {ref rSource, ref rWeapon}\n\
           PlayerREF.AuxVarSetRef \"*ca_t\" (GetSelf) -1\n\
           PlayerREF.AuxVarSetRef \"*ca_s\" rSource -1\n\
           PlayerREF.AuxVarSetRef \"*ca_w\" rWeapon -1\n\
           PlayerREF.AuxVarSetFlt \"*ca_loc\" (GetHitLocation) -1\n\
           PlayerREF.AuxVarSetFlt \"*ca_flg\" (GetHitExtendedFlag 1) -1\n\
         End) 1\n\
         SetOnCriticalHitEventHandler (Begin Function {ref rSource, ref rWeapon}\n\
           PlayerREF.AuxVarSetRef \"*ce_t\" (GetSelf) -1\n\
         End) 1 GuardRef PlayerREF TestPistol\n\
         SetOnCriticalHitEventHandler (Begin Function {ref rSource, ref rWeapon}\n\
           PlayerREF.AuxVarSetRef \"*cl_t\" (GetSelf) -1\n\
         End) 1 NpcActors PlayerREF RangedList\n\
         SetOnCriticalHitEventHandler (Begin Function {ref rSource, ref rWeapon}\n\
           PlayerREF.AuxVarSetRef \"*cnw_t\" (GetSelf) -1\n\
         End) 1 GuardRef PlayerREF TestShotgun\n\
         SetOnCriticalHitEventHandler (Begin Function {ref rSource, ref rWeapon}\n\
           PlayerREF.AuxVarSetRef \"*cns_t\" (GetSelf) -1\n\
         End) 1 GuardRef RaiderRef TestPistol",
    );

    // Normal (non-critical) hit must not fire criticalhit.
    g.hit(
        GUARD_REF,
        10.0,
        HitInfo { attacker: Some(FormId::PLAYER_REF), weapon: Some(FormId(PISTOL)), location: 1, ..HitInfo::default() },
    );
    assert_eq!(aux_size(&mut g, "*ca_t"), 0.0, "non-critical hit does not fire criticalhit");
    assert_eq!(global(&g, CRIT_WILDCARD_HITS), 0.0);
    assert_eq!(aux_size(&mut g, "*ce_t"), 0.0);
    assert_eq!(aux_size(&mut g, "*cl_t"), 0.0);

    // Critical hit on GuardRef by PlayerREF with TestPistol (head = 1).
    g.hit(
        GUARD_REF,
        15.0,
        HitInfo { attacker: Some(FormId::PLAYER_REF), weapon: Some(FormId(PISTOL)), location: 1, critical: true, ..HitInfo::default() },
    );
    assert_eq!(aux_size(&mut g, "*ca_t"), 1.0);
    assert_eq!(aux_ref(&mut g, "*ca_t", 0), GUARD_REF);
    assert_eq!(aux_ref(&mut g, "*ca_s", 0), FormId::PLAYER_REF);
    assert_eq!(aux_ref(&mut g, "*ca_w", 0), FormId(PISTOL));
    assert_eq!(aux_flt(&mut g, "*ca_loc", 0), 1.0, "GetHitLocation reads current hit in criticalhit handler");
    assert_eq!(aux_flt(&mut g, "*ca_flg", 0), 1.0, "GetHitExtendedFlag 1 reads critical flag");
    assert_eq!(global(&g, CRIT_WILDCARD_HITS), 1.0, "XMarker target wildcard matches GuardRef");
    assert_eq!(aux_size(&mut g, "*ce_t"), 1.0, "exact [GuardRef, PlayerREF, TestPistol] matches");
    assert_eq!(aux_size(&mut g, "*cl_t"), 1.0, "FormList [NpcActors, PlayerREF, RangedList] matches");
    assert_eq!(aux_size(&mut g, "*cnw_t"), 0.0, "non-matching weapon filter does not fire");
    assert_eq!(aux_size(&mut g, "*cns_t"), 0.0, "non-matching source filter does not fire");

    // Critical hit on RaiderRef by PlayerREF with TestPistol: wildcard + list match, GuardRef exact filter does not.
    g.hit(
        RAIDER_REF,
        15.0,
        HitInfo { attacker: Some(FormId::PLAYER_REF), weapon: Some(FormId(PISTOL)), location: 0, critical: true, ..HitInfo::default() },
    );
    assert_eq!(aux_size(&mut g, "*ca_t"), 2.0);
    assert_eq!(aux_ref(&mut g, "*ca_t", 1), RAIDER_REF);
    assert_eq!(global(&g, CRIT_WILDCARD_HITS), 2.0, "XMarker target wildcard also matches RaiderRef");
    assert_eq!(aux_size(&mut g, "*cl_t"), 2.0, "NpcActors list also matches RaiderRef");
    assert_eq!(aux_size(&mut g, "*ce_t"), 1.0, "GuardRef-only filter skips RaiderRef");

    // Critical hit on an already-dead target does not fire.
    assert!(g.kill(RAIDER_REF, None));
    g.hit(
        RAIDER_REF,
        15.0,
        HitInfo { attacker: Some(FormId::PLAYER_REF), weapon: Some(FormId(PISTOL)), location: 0, critical: true, ..HitInfo::default() },
    );
    assert_eq!(aux_size(&mut g, "*ca_t"), 2.0);
    assert_eq!(global(&g, CRIT_WILDCARD_HITS), 2.0);

    // Removing the XMarker-filtered handler stops it while unfiltered still runs.
    q(&mut g, "SetOnCriticalHitEventHandler CritWildcardUdf 0 XMarker PlayerREF TestPistol");
    g.hit(
        GUARD_REF,
        5.0,
        HitInfo { attacker: Some(FormId::PLAYER_REF), weapon: Some(FormId(PISTOL)), location: 0, critical: true, ..HitInfo::default() },
    );
    assert_eq!(aux_size(&mut g, "*ca_t"), 3.0);
    assert_eq!(global(&g, CRIT_WILDCARD_HITS), 2.0, "removed XMarker handler no longer runs");
}

#[test]
fn combat_jip_events_respect_newest_first_and_under_arity_skip() {
    let f = fixture();
    let mut g = game(&f);

    q(
        &mut g,
        "SetOnCriticalHitEventHandler (Begin Function {ref rSource, ref rWeapon}\n\
           PlayerREF.AuxVarSetFlt \"*ord\" 1 -1\n\
         End) 1\n\
         SetOnCriticalHitEventHandler (Begin Function {ref rSource, ref rWeapon}\n\
           PlayerREF.AuxVarSetFlt \"*ord\" 2 -1\n\
         End) 1\n\
         SetOnCriticalHitEventHandler (Begin Function {ref rOnlyOne}\n\
           PlayerREF.AuxVarSetFlt \"*under\" 1 -1\n\
         End) 1",
    );

    g.hit(
        GUARD_REF,
        10.0,
        HitInfo { attacker: Some(FormId::PLAYER_REF), weapon: Some(FormId(PISTOL)), location: 0, critical: true, ..HitInfo::default() },
    );
    assert_eq!(aux_size(&mut g, "*ord"), 2.0);
    assert_eq!((aux_flt(&mut g, "*ord", 0), aux_flt(&mut g, "*ord", 1)), (2.0, 1.0), "newest-first JIP handler order");
    assert_eq!(aux_size(&mut g, "*under"), 0.0, "handler declaring fewer than 2 parameters is skipped");
}

#[test]
fn jip_hit_info_queries_cover_projectile_melee_explosion_fatigue_base_and_3d_data() {
    let f = fixture();
    let mut g = game(&f);

    // Unhit actor returns 0 / null ref / invalid 3D array (Ar_Size == -1).
    assert_eq!(q(&mut g, "SetFunctionValue (GuardRef.GetHitProjectile)"), XVal::Ref(0));
    assert_eq!(q(&mut g, "SetFunctionValue (GuardRef.GetHitBaseWeaponDamage)"), XVal::Num(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GuardRef.GetHitFatigueDamage)"), XVal::Num(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (Ar_Size (GuardRef.GetHit3DData 0))"), XVal::Num(-1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (Ar_Size (GuardRef.GetHit3DData 1))"), XVal::Num(-1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (Ar_Size (GuardRef.GetHit3DData 2))"), XVal::Num(-1.0));

    // Register an OnHit handler that captures all JIP hit-info getters before health damage applies.
    q(
        &mut g,
        "SetOnHitEventHandler (Begin Function { }\n\
           PlayerREF.AuxVarSetRef \"*hi_proj\" (GetHitProjectile) -1\n\
           PlayerREF.AuxVarSetFlt \"*hi_base\" (GetHitBaseWeaponDamage) -1\n\
           PlayerREF.AuxVarSetFlt \"*hi_fat\" (GetHitFatigueDamage) -1\n\
           PlayerREF.AuxVarSetFlt \"*hi_blk\" (GetHitExtendedFlag 0) -1\n\
           PlayerREF.AuxVarSetFlt \"*hi_snk\" (GetHitExtendedFlag 7) -1\n\
           PlayerREF.AuxVarSetFlt \"*hi_exp\" (GetHitExtendedFlag 8) -1\n\
           PlayerREF.AuxVarSetFlt \"*hi_msk\" (GetHitExtendedFlag 256) -1\n\
           array_var aPos\n\
           let aPos := GetHit3DData 0\n\
           PlayerREF.AuxVarSetFlt \"*hi_p0sz\" (Ar_Size aPos) -1\n\
           if (Ar_Size aPos) == 3\n\
             PlayerREF.AuxVarSetFlt \"*hi_px\" (aPos[0]) -1\n\
             PlayerREF.AuxVarSetFlt \"*hi_py\" (aPos[1]) -1\n\
             PlayerREF.AuxVarSetFlt \"*hi_pz\" (aPos[2]) -1\n\
           endif\n\
           array_var aDir\n\
           let aDir := GetHit3DData 1\n\
           PlayerREF.AuxVarSetFlt \"*hi_a1sz\" (Ar_Size aDir) -1\n\
           if (Ar_Size aDir) == 3\n\
             PlayerREF.AuxVarSetFlt \"*hi_dx\" (aDir[0]) -1\n\
             PlayerREF.AuxVarSetFlt \"*hi_dy\" (aDir[1]) -1\n\
             PlayerREF.AuxVarSetFlt \"*hi_dz\" (aDir[2]) -1\n\
           endif\n\
           array_var aBoth\n\
           let aBoth := GetHit3DData 2\n\
           PlayerREF.AuxVarSetFlt \"*hi_b2sz\" (Ar_Size aBoth) -1\n\
           if (Ar_Size aBoth) == 6\n\
             PlayerREF.AuxVarSetFlt \"*hi_b3\" (aBoth[3]) -1\n\
             PlayerREF.AuxVarSetFlt \"*hi_b4\" (aBoth[4]) -1\n\
             PlayerREF.AuxVarSetFlt \"*hi_b5\" (aBoth[5]) -1\n\
           endif\n\
           PlayerREF.AuxVarSetFlt \"*hi_bad\" (Ar_Size (GetHit3DData 9)) -1\n\
         End) 1 GuardRef",
    );

    // 1. Projectile crit hit with fatigue damage, base+crit weapon damage (22 + 200 = 222),
    // sneak attack, and non-unit incoming direction normalized to a unit 3D vector.
    let proj_ref = g
        .projectile_fired(FormId(BULLET), FormId::PLAYER_REF, FormId(PISTOL), 22.0, [0.0, 0.0, 50.0])
        .expect("live projectile ref");
    let raw_dir = [-0.608, 1.774, -0.694];
    let want_dir = fnv_game::combat::unit_hit_dir(raw_dir).expect("unit incoming direction");
    g.hit(
        GUARD_REF,
        18.0,
        HitInfo {
            attacker: Some(FormId::PLAYER_REF),
            weapon: Some(FormId(PISTOL)),
            projectile: Some(proj_ref),
            location: 1,
            critical: true,
            sneak_attack: true,
            base_weapon_damage: 222.0,
            fatigue_damage: 125.0,
            hit_pos: Some([12.5, 34.0, 78.25]),
            hit_dir: Some(raw_dir),
            ..HitInfo::default()
        },
    );
    assert_eq!(aux_ref(&mut g, "*hi_proj", 0), proj_ref);
    assert_eq!(aux_flt(&mut g, "*hi_base", 0), 222.0);
    assert_eq!(aux_flt(&mut g, "*hi_fat", 0), 125.0);
    assert_eq!(aux_flt(&mut g, "*hi_blk", 0), 0.0);
    assert_eq!(aux_flt(&mut g, "*hi_snk", 0), 1.0);
    assert_eq!(aux_flt(&mut g, "*hi_exp", 0), 0.0);
    assert_eq!(aux_flt(&mut g, "*hi_msk", 0), 0.0, "GetHitExtendedFlag takes bit index, not bitmask 256");
    assert_eq!(aux_flt(&mut g, "*hi_p0sz", 0), 3.0);
    assert_eq!((aux_flt(&mut g, "*hi_px", 0), aux_flt(&mut g, "*hi_py", 0), aux_flt(&mut g, "*hi_pz", 0)), (12.5, 34.0, 78.25));
    assert_eq!(aux_flt(&mut g, "*hi_a1sz", 0), 3.0);
    let (dx, dy, dz) = (aux_flt(&mut g, "*hi_dx", 0), aux_flt(&mut g, "*hi_dy", 0), aux_flt(&mut g, "*hi_dz", 0));
    let norm = (dx * dx + dy * dy + dz * dz).sqrt();
    assert!((norm - 1.0).abs() < 1e-5, "GetHit3DData 1 must be unit length, got ({dx}, {dy}, {dz}) norm={norm}");
    assert!((dx - f64::from(want_dir[0])).abs() < 1e-5);
    assert!((dy - f64::from(want_dir[1])).abs() < 1e-5);
    assert!((dz - f64::from(want_dir[2])).abs() < 1e-5);
    assert_eq!(aux_flt(&mut g, "*hi_b2sz", 0), 6.0);
    assert!((aux_flt(&mut g, "*hi_b3", 0) - dx).abs() < 1e-5);
    assert!((aux_flt(&mut g, "*hi_b4", 0) - dy).abs() < 1e-5);
    assert!((aux_flt(&mut g, "*hi_b5", 0) - dz).abs() < 1e-5);
    assert_eq!(aux_flt(&mut g, "*hi_bad", 0), -1.0, "invalid dataType=9 returns ar_Null (Ar_Size == -1)");

    // 2. Melee blocked hit: no projectile (Ref(0)), flag 0 = 1, flag 8 = 0, unit east direction [1, 0, 0].
    g.hit(
        GUARD_REF,
        6.0,
        HitInfo {
            attacker: Some(FormId::PLAYER_REF),
            weapon: Some(FormId(KNIFE)),
            location: 0,
            blocking: true,
            base_weapon_damage: 14.0,
            hit_pos: Some([1.0, 2.0, 3.0]),
            hit_dir: Some([3.0, 0.0, 0.0]),
            ..HitInfo::default()
        },
    );
    assert_eq!(aux_size(&mut g, "*hi_proj"), 1.0, "null GetHitProjectile ref does not append to AuxVarSetRef");
    assert_eq!(q(&mut g, "SetFunctionValue (GuardRef.GetHitProjectile)"), XVal::Ref(0));
    assert_eq!(aux_flt(&mut g, "*hi_base", 1), 14.0);
    assert_eq!(aux_flt(&mut g, "*hi_fat", 1), 0.0);
    assert_eq!(aux_flt(&mut g, "*hi_blk", 1), 1.0);
    assert_eq!(aux_flt(&mut g, "*hi_snk", 1), 0.0);
    assert_eq!(aux_flt(&mut g, "*hi_exp", 1), 0.0);
    assert_eq!((aux_flt(&mut g, "*hi_dx", 1), aux_flt(&mut g, "*hi_dy", 1), aux_flt(&mut g, "*hi_dz", 1)), (1.0, 0.0, 0.0));

    // 3. Explosion hit: live EXPL ref (GetType == 81, IsReference == 1), flag 8 = 1 (256 = 0),
    // GetHitBaseWeaponDamage returns EXPL base damage (125.0), and GetHit3DData returns all zeros (3 / 3 / 6).
    let expl_ref = g
        .explosion_placed(FormId(BLAST), Some(FormId::PLAYER_REF), [5.0, 6.0, 7.0], 200.0)
        .expect("live explosion ref");
    g.hit(
        GUARD_REF,
        10.0,
        HitInfo {
            projectile: Some(expl_ref),
            location: -1,
            explosion: true,
            base_weapon_damage: 125.0,
            ..HitInfo::default()
        },
    );
    assert_eq!(aux_size(&mut g, "*hi_proj"), 2.0);
    assert_eq!(aux_ref(&mut g, "*hi_proj", 1), expl_ref);
    assert_eq!(q(&mut g, "SetFunctionValue (IsReference (GuardRef.GetHitProjectile))"), XVal::Num(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GetType (GuardRef.GetHitProjectile))"), XVal::Num(81.0));
    assert_eq!(aux_flt(&mut g, "*hi_base", 2), 125.0, "explosion hit reports EXPL base damage");
    assert_eq!(aux_flt(&mut g, "*hi_exp", 2), 1.0);
    assert_eq!(aux_flt(&mut g, "*hi_msk", 2), 0.0, "bitmask 256 returns 0 even when flag 8 is set");
    assert_eq!(aux_flt(&mut g, "*hi_p0sz", 2), 3.0);
    assert_eq!(aux_flt(&mut g, "*hi_a1sz", 2), 3.0);
    assert_eq!(aux_flt(&mut g, "*hi_b2sz", 2), 6.0);
    assert_eq!((aux_flt(&mut g, "*hi_px", 2), aux_flt(&mut g, "*hi_py", 2), aux_flt(&mut g, "*hi_pz", 2)), (0.0, 0.0, 0.0));
    assert_eq!((aux_flt(&mut g, "*hi_dx", 2), aux_flt(&mut g, "*hi_dy", 2), aux_flt(&mut g, "*hi_dz", 2)), (0.0, 0.0, 0.0));
    assert_eq!((aux_flt(&mut g, "*hi_b3", 2), aux_flt(&mut g, "*hi_b4", 2), aux_flt(&mut g, "*hi_b5", 2)), (0.0, 0.0, 0.0));

    // 4. Non-finite coordinates and zero-length non-explosion direction are guarded and yield ar_Null (Ar_Size == -1).
    g.hit(
        GUARD_REF,
        1.0,
        HitInfo {
            location: 0,
            hit_pos: Some([f32::NAN, 1.0, 2.0]),
            hit_dir: Some([0.0, f32::INFINITY, 0.0]),
            ..HitInfo::default()
        },
    );
    assert_eq!(aux_flt(&mut g, "*hi_p0sz", 3), -1.0);
    assert_eq!(aux_flt(&mut g, "*hi_a1sz", 3), -1.0);
    assert_eq!(aux_flt(&mut g, "*hi_b2sz", 3), -1.0);

    g.hit(
        GUARD_REF,
        1.0,
        HitInfo {
            location: 0,
            hit_pos: Some([1.0, 2.0, 3.0]),
            hit_dir: Some([0.0, 0.0, 0.0]),
            ..HitInfo::default()
        },
    );
    assert_eq!(aux_flt(&mut g, "*hi_p0sz", 4), 3.0);
    assert_eq!(aux_flt(&mut g, "*hi_a1sz", 4), -1.0, "zero-length non-explosion direction yields ar_Null");
    assert_eq!(aux_flt(&mut g, "*hi_b2sz", 4), -1.0);

    // 5. ToggleImmortalMode 2 on the player zeroes health damage only (jip-ui-9.md §3);
    // GetHitFatigueDamage still records the hit's fatigue damage.
    q(&mut g, "ToggleImmortalMode 2");
    g.hit(
        FormId::PLAYER_REF,
        15.0,
        HitInfo { attacker: Some(GUARD_REF), fatigue_damage: 90.0, location: 0, ..HitInfo::default() },
    );
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.GetHitHealthDamage)"), XVal::Num(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.GetHitFatigueDamage)"), XVal::Num(90.0));
}

#[test]
fn get_reticle_target_limb_tracks_actor_limb_max_range_and_clear() {
    use fnv_game::functions::senses::Reticle;
    let f = fixture();
    let mut g = game(&f);

    // Headless / no reticle: -1.
    assert_eq!(q(&mut g, "SetFunctionValue (GetReticleTargetLimb)"), XVal::Num(-1.0));

    // Hit on a non-actor (limb: None): -1.
    g.senses.reticle = Some(Reticle { start: [0.0; 3], dir: [0.0, 1.0, 0.0], hit: Some(120.0), limb: None });
    assert_eq!(q(&mut g, "SetFunctionValue (GetReticleTargetLimb)"), XVal::Num(-1.0));

    // Actor limb hits (Head 1 = 1, Torso = 0, Brain = 13, Weapon = 14) + optional maxRange.
    g.senses.reticle = Some(Reticle { start: [0.0; 3], dir: [0.0, 1.0, 0.0], hit: Some(250.0), limb: Some(1) });
    assert_eq!(q(&mut g, "SetFunctionValue (GetReticleTargetLimb)"), XVal::Num(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GetReticleTargetLimb 0)"), XVal::Num(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GetReticleTargetLimb 300)"), XVal::Num(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GetReticleTargetLimb 200)"), XVal::Num(-1.0));

    g.senses.reticle = Some(Reticle { start: [0.0; 3], dir: [0.0, 1.0, 0.0], hit: Some(80.0), limb: Some(13) });
    assert_eq!(q(&mut g, "SetFunctionValue (GetReticleTargetLimb)"), XVal::Num(13.0));

    // Looking away clears back to -1.
    g.senses.reticle = Some(Reticle { start: [0.0; 3], dir: [0.0, 1.0, 0.0], hit: None, limb: None });
    assert_eq!(q(&mut g, "SetFunctionValue (GetReticleTargetLimb)"), XVal::Num(-1.0));
    g.senses.reticle = None;
    assert_eq!(q(&mut g, "SetFunctionValue (GetReticleTargetLimb)"), XVal::Num(-1.0));
}

#[test]
fn jip_xmarker_wildcard_applies_only_to_critical_hit() {
    let f = fixture();
    let mut g = game(&f);
    q(
        &mut g,
        r#"SetOnHealthDamageEventHandler (Begin Function {float dmg, ref src}
             PlayerREF.AuxVarSetFlt "*xm_hd" ((PlayerREF.AuxVarGetFlt "*xm_hd") + 1)
           End) 1 XMarker
           SetOnHealthDamageEventHandler (Begin Function {float dmg, ref src}
             PlayerREF.AuxVarSetFlt "*ex_hd" ((PlayerREF.AuxVarGetFlt "*ex_hd") + 1)
           End) 1 PlayerREF
           SetOnAnimActionEventHandler (Begin Function {int act}
             PlayerREF.AuxVarSetFlt "*xm_aa" ((PlayerREF.AuxVarGetFlt "*xm_aa") + 1)
           End) 1 XMarker 0
           SetOnCrippledLimbEventHandler (Begin Function {int limb}
             PlayerREF.AuxVarSetFlt "*xm_cl" ((PlayerREF.AuxVarGetFlt "*xm_cl") + 1)
           End) 1 XMarker
           SetOnUseAidItemEventHandler (Begin Function {ref it}
             PlayerREF.AuxVarSetFlt "*xm_ai" ((PlayerREF.AuxVarGetFlt "*xm_ai") + 1)
           End) 1 XMarker"#,
    );
    g.jip_event("healthdamage", Some(FormId::PLAYER_REF), vec![XVal::Num(-5.0), XVal::Ref(0)]);
    g.jip_event("animaction", Some(FormId::PLAYER_REF), vec![XVal::Num(0.0)]);
    g.jip_event("crippledlimb", Some(FormId::PLAYER_REF), vec![XVal::Num(2.0)]);
    g.jip_event("useaiditem", Some(FormId::PLAYER_REF), vec![XVal::Ref(PISTOL)]);
    assert_eq!(aux_flt(&mut g, "*ex_hd", 0), 1.0, "exact PlayerREF filter fires");
    assert_eq!(aux_size(&mut g, "*xm_hd"), 0.0, "XMarker must not wildcard healthdamage");
    assert_eq!(aux_size(&mut g, "*xm_aa"), 0.0, "XMarker must not wildcard animaction");
    assert_eq!(aux_size(&mut g, "*xm_cl"), 0.0, "XMarker must not wildcard crippledlimb");
    assert_eq!(aux_size(&mut g, "*xm_ai"), 0.0, "XMarker must not wildcard useaiditem");
}
