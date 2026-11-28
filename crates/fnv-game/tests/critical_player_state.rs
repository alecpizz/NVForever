//! Owned source/compiler controls for critical damage, player motion and Hardcore
//! eligibility (ext-last-core.md). No native encoding or original-game claim.
use crate::common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{Game, GameConfig, GameData, game::{GameSnapshot, RunCtx}, functions::senses::Motion};
use fnv_script::{FunctionTable, vm::XVal};
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};

const WEAPON: FormId = FormId(0x0100_0900);
const OTHER: FormId = FormId(0x0100_0901);
struct Fixture { data: GameData, forms: FormStore, refs: RefTable }
fn fixture() -> Fixture {
    let lead = [tes4(), top(b"NPC_", &record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))]))].concat();
    let weapon = |id, name: &str, damage: u16| record(b"WEAP", 0, id, &[
        sub(b"EDID", z(name)),
        sub(b"CRDT", [damage.to_le_bytes().to_vec(), vec![0; 2], 1.0f32.to_le_bytes().to_vec(), vec![0; 8]].concat()),
    ]);
    let own = [tes4(), top(b"WEAP", &[weapon(0x900, "OwnedCriticalWeapon", 11), weapon(0x901, "OwnedOtherWeapon", 7)].concat())].concat();
    let plugins = [Plugin::parse(&lead, "OwnedLead.esm".into()).unwrap(), Plugin::parse(&own, "OwnedCritical.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let path = root.join("script/functions.json");
    let json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&path).unwrap(), &json, &refs);
    Fixture { data, forms, refs }
}
fn game(f: &Fixture) -> Game<'_> {
    Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() })
}
fn query(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("ref empty\nBegin Function {{}}\n{body}\nEnd"), RunCtx::default()).unwrap()
}
fn source(g: &mut Game, expression: &str) -> XVal {
    query(g, &format!("SetFunctionValue ({expression})"))
}
fn restored<'a>(f: &'a Fixture, g: &Game<'_>) -> Game<'a> {
    let snap: GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.snapshot()).unwrap()).unwrap();
    let world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    Game::restore(&f.data, world, g.cfg, &snap).unwrap()
}

#[test]
fn canonical_critical_damage_preserves_readback_and_reaches_the_combat_view() {
    let f = fixture();
    let mut g = game(&f);
    let baseline_snap = g.snapshot();
    assert_eq!(source(&mut g, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(11.0));
    assert_eq!(g.weapon_critical_damage_override(WEAPON), None);
    // Unmodified TempCloneForm snapshots authored critical damage (11) and stays isolated
    // from later source mutations.
    let XVal::Ref(unmod_clone_id) = query(&mut g, "SetFunctionValue (TempCloneForm OwnedCriticalWeapon 0)") else {
        panic!("expected unmodified clone ref");
    };
    let unmod_clone = FormId(unmod_clone_id);
    for (value, expected) in [(0.0, 0.0), (37.0, 37.0), (65535.0, 65535.0), (12.75, 12.0), (-5.0, 65531.0), (-1.0, 65535.0), (65536.0, 0.0)] {
        query(&mut g, &format!("SetWeaponCritDamage {value} OwnedCriticalWeapon"));
        assert_eq!(source(&mut g, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(expected), "SetWeaponCritDamage {value} wraps into u16");
        assert_eq!(g.weapon_critical_damage_override(WEAPON), Some(expected as u32));
        assert_eq!(source(&mut g, "GetWeaponCritDamage OwnedOtherWeapon"), XVal::Num(7.0));
        assert_eq!(g.weapon_critical_damage_override(OTHER), None);
    }
    assert_eq!(g.weapon_critical_damage_override(unmod_clone), Some(11));
    assert_eq!(
        g.run_query("Begin Function {}\nSetFunctionValue (GetWeaponCritDamage)\nEnd", RunCtx { self_ref: Some(unmod_clone), ..Default::default() }).unwrap(),
        XVal::Num(11.0),
    );
    // TempCloneForm snapshots live modified critical damage at clone time and stays
    // bi-directionally isolated from later source/clone edits (including clone chains)
    // (see docs/spec-notes/ext-last-core.md §7).
    query(&mut g, "SetWeaponCritDamage 77 OwnedCriticalWeapon");
    let XVal::Ref(clone_id) = query(&mut g, "SetFunctionValue (TempCloneForm OwnedCriticalWeapon 0)") else {
        panic!("expected clone ref");
    };
    let clone = FormId(clone_id);
    assert_eq!(
        g.run_query("Begin Function {}\nSetFunctionValue (GetWeaponCritDamage)\nEnd", RunCtx { self_ref: Some(clone), ..Default::default() }).unwrap(),
        XVal::Num(77.0),
    );
    assert_eq!(g.weapon_critical_damage_override(clone), Some(77));
    query(&mut g, "SetWeaponCritDamage 88 OwnedCriticalWeapon");
    assert_eq!(source(&mut g, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(88.0));
    assert_eq!(g.weapon_critical_damage_override(WEAPON), Some(88));
    assert_eq!(
        g.run_query("Begin Function {}\nSetFunctionValue (GetWeaponCritDamage)\nEnd", RunCtx { self_ref: Some(clone), ..Default::default() }).unwrap(),
        XVal::Num(77.0),
    );
    assert_eq!(g.weapon_critical_damage_override(clone), Some(77));
    g.run_query("Begin Function {}\nSetWeaponCritDamage 99\nEnd", RunCtx { self_ref: Some(clone), ..Default::default() }).unwrap();
    assert_eq!(source(&mut g, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(88.0));
    assert_eq!(
        g.run_query("Begin Function {}\nSetFunctionValue (GetWeaponCritDamage)\nEnd", RunCtx { self_ref: Some(clone), ..Default::default() }).unwrap(),
        XVal::Num(99.0),
    );
    assert_eq!(g.weapon_critical_damage_override(clone), Some(99));
    // Clone-of-clone chain snapshots its immediate source clone's value (99) and stays
    // isolated from later edits to that source clone, exercised inside a single UDF.
    assert_eq!(
        query(
            &mut g,
            "ref rCl1\nref rCl2\nSetWeaponCritDamage 77 OwnedCriticalWeapon\nlet rCl1 := TempCloneForm OwnedCriticalWeapon 0\nSetWeaponCritDamage 88 OwnedCriticalWeapon\nSetWeaponCritDamage 99 rCl1\nlet rCl2 := TempCloneForm rCl1 0\nSetWeaponCritDamage 44 rCl1\nSetFunctionValue ((GetWeaponCritDamage OwnedCriticalWeapon) * 10000 + (GetWeaponCritDamage rCl1) * 100 + (GetWeaponCritDamage rCl2))",
        ),
        XVal::Num(884499.0),
    );
    // Characterize the accepted generic setter's explicit empty argument fallback.
    // This remains unqualified for native parity and is deliberately not tightened.
    let own = "ref empty\nBegin Function {}\nSetWeaponCritDamage 19 empty\nEnd";
    g.run_query(own, RunCtx { self_ref: Some(WEAPON), ..Default::default() }).unwrap();
    assert_eq!(source(&mut g, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(19.0));
    let invalid_receiver = "ref empty\nBegin Function {}\nempty.SetWeaponCritDamage 23 OwnedCriticalWeapon\nEnd";
    assert!(g.run_query(invalid_receiver, RunCtx { self_ref: Some(WEAPON), ..Default::default() }).is_err());
    assert_eq!(source(&mut g, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(19.0));
    query(&mut g, "SetWeaponCritDamage 37 OwnedCriticalWeapon");
    // Fresh-process restore (no put_session): reverts to ESM default (not saved).
    let mut fresh = restored(&f, &g);
    assert_eq!(source(&mut fresh, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(11.0));
    assert_eq!(fresh.weapon_critical_damage_override(WEAPON), None);
    assert_eq!(game(&f).weapon_critical_damage_override(WEAPON), None);
    // Same-run baseline and mutated reloads via take_session/put_session carry loaded
    // base-form overrides and exclude transient TempCloneForm clones.
    let session = g.take_session();
    let mut same_run_base = Game::restore(&f.data, World::restore(&f.forms, &f.refs, &baseline_snap.sim).unwrap(), g.cfg, &baseline_snap).unwrap();
    same_run_base.put_session(session);
    assert_eq!(source(&mut same_run_base, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(37.0));
    assert_eq!(same_run_base.weapon_critical_damage_override(WEAPON), Some(37));
    assert_eq!(same_run_base.weapon_critical_damage_override(clone), None);
    let session2 = same_run_base.take_session();
    let mut same_run_mut = restored(&f, &g);
    same_run_mut.put_session(session2);
    assert_eq!(source(&mut same_run_mut, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(37.0));
    assert_eq!(same_run_mut.weapon_critical_damage_override(WEAPON), Some(37));
}

#[test]
fn player_swimming_source_tracks_player_motion_across_call_contexts() {
    let f = fixture();
    let mut g = game(&f);
    for (player, other) in [(false, true), (true, false), (false, false), (true, true)] {
        g.set_motion(FormId::PLAYER_REF, Motion { swimming: player, ..Default::default() });
        g.set_motion(OTHER, Motion { swimming: other, ..Default::default() });
        assert_eq!(source(&mut g, "IsPlayerSwimming"), XVal::Num(f64::from(u8::from(player))));
        assert_eq!(source(&mut g, "OwnedOtherWeapon.IsPlayerSwimming"), XVal::Num(f64::from(u8::from(player))));
        assert_eq!(source(&mut g, "PlayerREF.IsSwimming"), XVal::Num(0.0));
    }
    g.set_motion(FormId::PLAYER_REF, Motion { swimming: false, ..Default::default() });
    assert_eq!(query(&mut g, "if IsPlayerSwimming\nSetFunctionValue 9\nelse\nSetFunctionValue 4\nendif"), XVal::Num(4.0));
    g.set_motion(FormId::PLAYER_REF, Motion { swimming: true, ..Default::default() });
    assert_eq!(query(&mut g, "if IsPlayerSwimming\nSetFunctionValue 9\nelse\nSetFunctionValue 4\nendif"), XVal::Num(9.0));
    assert_eq!(source(&mut game(&f), "IsPlayerSwimming"), XVal::Num(0.0));
}

#[test]
fn disabling_hardcore_eligibility_persists_without_changing_mode_or_needs_toggle() {
    let f = fixture();
    let mut g = game(&f);
    g.set_hardcore(true);
    assert_eq!(source(&mut g, "IsAlwaysHardcore"), XVal::Num(1.0));
    assert_eq!(source(&mut g, "GetHardcoreTracking"), XVal::Num(1.0));
    for command in ["DisableHardcoreTracking", "babby", "DisableHardcoreTracking"] {
        query(&mut g, command);
        assert_eq!(source(&mut g, "IsAlwaysHardcore"), XVal::Num(0.0));
        assert_eq!(source(&mut g, "IsHardcore"), XVal::Num(1.0));
        assert_eq!(source(&mut g, "GetHardcoreTracking"), XVal::Num(1.0));
    }
    let mut control = game(&f);
    control.set_hardcore(true);
    let before = control.pass_hours(1.0, false);
    let after = g.pass_hours(1.0, false);
    assert_eq!(after.needs, before.needs, "the actual wait consumer keeps accumulating needs");
    assert!(after.needs.iter().any(|(_, from, to)| to > from));
    let mut loaded = restored(&f, &g);
    loaded.set_hardcore(true);
    assert_eq!(source(&mut loaded, "IsAlwaysHardcore"), XVal::Num(0.0));
    let mut fresh = game(&f);
    fresh.set_hardcore(true);
    assert_eq!(source(&mut fresh, "IsAlwaysHardcore"), XVal::Num(1.0));
}

#[test]
fn setcritdam_alias_matches_setweaponcritdamage_bytes_and_execution() {
    let f = fixture();
    let canon = f.data.table.unnumbered.get("setweaponcritdamage").expect("canonical SetWeaponCritDamage");
    let alias = f.data.table.unnumbered.get("setcritdam").expect("SetCritDam alias");
    assert_eq!(f.data.table.by_name("setcritdam"), None, "SetCritDam stays unnumbered");
    assert_eq!(alias.name, canon.name);
    assert_eq!(alias.params.len(), canon.params.len());

    // Source compilation through the canonical and alias spellings yields identical bytes and refs.
    let mut g = game(&f);
    let (c_canon, c_alias, c_lower) = {
        let env = fnv_game::game::GameEnv { g: &g };
        (
            fnv_script::compile::compile_mod_runtime(
                "Begin Function {}\nSetWeaponCritDamage 55 OwnedCriticalWeapon\nEnd",
                &f.data.table,
                &env,
            ),
            fnv_script::compile::compile_mod_runtime(
                "Begin Function {}\nSetCritDam 55 OwnedCriticalWeapon\nEnd",
                &f.data.table,
                &env,
            ),
            fnv_script::compile::compile_mod_runtime(
                "Begin Function {}\nsetcritdam 55 OwnedCriticalWeapon\nEnd",
                &f.data.table,
                &env,
            ),
        )
    };
    assert_eq!(c_canon.errors().count(), 0, "{:?}", c_canon.errors().collect::<Vec<_>>());
    assert_eq!(c_alias.errors().count(), 0, "{:?}", c_alias.errors().collect::<Vec<_>>());
    assert_eq!(c_lower.errors().count(), 0, "{:?}", c_lower.errors().collect::<Vec<_>>());
    assert_eq!(c_alias.bytes, c_canon.bytes);
    assert_eq!(c_alias.refs, c_canon.refs);
    assert_eq!(c_lower.bytes, c_canon.bytes);

    // Explicit weapon argument updates both script getter and combat critical-damage view.
    assert_eq!(source(&mut g, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(11.0));
    query(&mut g, "SetCritDam 55 OwnedCriticalWeapon");
    assert_eq!(source(&mut g, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(55.0));
    assert_eq!(g.weapon_critical_damage_override(WEAPON), Some(55));
    assert_eq!(source(&mut g, "GetWeaponCritDamage OwnedOtherWeapon"), XVal::Num(7.0));
    assert_eq!(g.weapon_critical_damage_override(OTHER), None);

    // Calling-reference form (omitted weapon arg) updates the calling weapon's base form.
    query(&mut g, "OwnedCriticalWeapon.SetCritDam 77");
    assert_eq!(source(&mut g, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(77.0));
    assert_eq!(g.weapon_critical_damage_override(WEAPON), Some(77));

    // Implicit self_ref calling-reference form works identically to SetWeaponCritDamage.
    g.run_query("Begin Function {}\nSetCritDam 88\nEnd", RunCtx { self_ref: Some(WEAPON), ..Default::default() }).unwrap();
    assert_eq!(source(&mut g, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(88.0));
    assert_eq!(g.weapon_critical_damage_override(WEAPON), Some(88));

    // Explicit empty receiver fails cleanly without mutating OwnedCriticalWeapon.
    let invalid_receiver = "ref empty\nBegin Function {}\nempty.SetCritDam 23 OwnedCriticalWeapon\nEnd";
    assert!(g.run_query(invalid_receiver, RunCtx { self_ref: Some(WEAPON), ..Default::default() }).is_err());
    assert_eq!(source(&mut g, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(88.0));

    // Session lifetime matches SetWeaponCritDamage: save/load restore resets override.
    let mut loaded = restored(&f, &g);
    assert_eq!(source(&mut loaded, "GetWeaponCritDamage OwnedCriticalWeapon"), XVal::Num(11.0));
    assert_eq!(loaded.weapon_critical_damage_override(WEAPON), None);
}
