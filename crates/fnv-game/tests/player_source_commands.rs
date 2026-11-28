//! Authored portable player-command consumers; no game data or native parity claim.
use crate::common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{actor_values as av, cond, functions, game::{GameSnapshot, RunCtx}, trace::EventKind, Game, GameConfig, GameData};
use fnv_script::{vm::{ArgValue, Value, XVal}, FunctionTable};
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};

const REP: FormId = FormId(0x850);
const OTHER_REP: FormId = FormId(0x851);
const EXPELLED_FAC: FormId = FormId(0x870);
const OTHER_FAC: FormId = FormId(0x871);
const NPC: FormId = FormId(0x300);
const ESSENTIAL_NPC: FormId = FormId(0x301);
const REMOTE_NPC: FormId = FormId(0x302);
const ROOM: FormId = FormId(0x200);
const REMOTE_ROOM: FormId = FormId(0x201);
struct Fixture { data: GameData, forms: FormStore, refs: RefTable }

fn fixture() -> Fixture {
    let npc = |id, name, strength| record(b"NPC_", 0, id, &[
        sub(b"EDID", z(name)),
        sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![strength, 5, 5, 5, 5, 5, 5]].concat()),
    ]);
    let rep = |id, name| record(b"REPU", 0, id, &[sub(b"EDID", z(name)), sub(b"DATA", 20f32.to_le_bytes().to_vec())]);
    let fac = |id, name| record(b"FACT", 0, id, &[sub(b"EDID", z(name))]);
    let placed = record(b"ACHR", 0, NPC.0, &[sub(b"EDID", z("ProgressionNeighborRef")), sub(b"NAME", 0x800u32.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let placed_essential = record(b"ACHR", 0, ESSENTIAL_NPC.0, &[sub(b"EDID", z("ProgressionEssentialRef")), sub(b"NAME", 0x800u32.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let placed_remote = record(b"ACHR", 0, REMOTE_NPC.0, &[sub(b"EDID", z("ProgressionRemoteRef")), sub(b"NAME", 0x800u32.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let cell = record(b"CELL", 0, ROOM.0, &[sub(b"EDID", z("ProgressionRoom"))]);
    let children = group(ROOM.0.to_le_bytes(), 6, &group(ROOM.0.to_le_bytes(), 9, &[placed, placed_essential].concat()));
    let remote_cell = record(b"CELL", 0, REMOTE_ROOM.0, &[sub(b"EDID", z("ProgressionRemoteRoom"))]);
    let remote_children = group(REMOTE_ROOM.0.to_le_bytes(), 6, &group(REMOTE_ROOM.0.to_le_bytes(), 9, &placed_remote));
    // World::formula_capacity requires both carry settings, as in the aid fixture.
    let settings = [
        record(b"GMST", 0, 0x860, &[sub(b"EDID", z("fAVDCarryWeightsBase")), sub(b"DATA", 150f32.to_le_bytes().to_vec())]),
        record(b"GMST", 0, 0x861, &[sub(b"EDID", z("fAVDCarryWeightMult")), sub(b"DATA", 10f32.to_le_bytes().to_vec())]),
    ].concat();
    let bytes = [tes4(),
        top(b"GMST", &settings),
        top(b"FACT", &[fac(EXPELLED_FAC.0, "ExpelledFaction"), fac(OTHER_FAC.0, "OtherFaction")].concat()),
        top(b"NPC_", &[npc(7, "Player", 5), npc(0x800, "ProgressionNeighbor", 8)].concat()),
        top(b"REPU", &[rep(REP.0, "ProgressionRep"), rep(OTHER_REP.0, "ProgressionOtherRep")].concat()),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children, remote_cell, remote_children].concat()))),
    ].concat();
    let plugins = vec![Plugin::parse(&bytes, "PlayerSourceControls.esm".into()).unwrap()];
    let order = LoadOrder::new(&plugins).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let path = root.join("script/functions.json");
    let catalogue = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let forms = FormStore::build(&order, &schema);
    let refs = RefTable::build(&order, &schema);
    let data = GameData::build(&order, &schema, FunctionTable::load(&path).unwrap(), &catalogue, &refs);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() });
    g.player_entered(Some(ROOM));
    g
}

// run_query compiles source to SCDA and decodes it before running the VM. A named
// function's neutral result alone is insufficient: reject skips and unread arguments.
fn query(g: &mut Game<'_>, body: &str) -> Result<XVal, String> {
    g.run_query(&format!("Begin Function {{}}\n{body}\nEnd\n"), RunCtx::default())
}
fn success(g: &mut Game<'_>, body: &str) -> XVal {
    let before = g.events.len();
    let result = query(g, body).unwrap_or_else(|e| panic!("{body}: {e}"));
    assert!(!g.events[before..].iter().any(|e| matches!(e.kind,
        EventKind::Stub { .. } | EventKind::Unhandled { .. } | EventKind::Skipped { .. }
        | EventKind::CallFailed { .. } | EventKind::ScriptError { .. } | EventKind::ArgUnread { .. }
    )), "{body}: {:?}", &g.events[before..]);
    result
}
fn compile_control(g: &Game<'_>, body: &str) -> fnv_script::compile::Compiled {
    fnv_script::compile::compile_runtime(&format!("Begin Function {{}}\n{body}\nEnd\n"), &g.data.table, &fnv_game::game::GameEnv { g })
}
fn runtime_failure(g: &mut Game<'_>, body: &str, reason: &str) {
    let compiled = compile_control(g, body);
    assert!(compiled.errors().next().is_none(), "runtime guard was never admitted: {body}: {:?}", compiled.diags);
    let before = g.events.len();
    assert_eq!(query(g, body), Err(reason.to_string()), "wrong failure boundary: {body}");
    assert!(!g.events[before..].iter().any(|e| matches!(e.kind,
        EventKind::Stub { .. } | EventKind::Unhandled { .. } | EventKind::Skipped { .. } | EventKind::ArgUnread { .. }
    )), "runtime guard was replaced by unsupported/unread input: {body}: {:?}", &g.events[before..]);
    for event in &g.events[before..] {
        if let EventKind::CallFailed { error, .. } | EventKind::ScriptError { error, .. } = &event.kind {
            assert_eq!(error, reason, "unexpected runtime error trace: {body}");
        }
    }
}

#[test]
fn source_advance_and_alias_reach_the_existing_menu_without_awarding_xp() {
    let f = fixture(); let mut g = game(&f);
    g.player.xp = 123.5;
    let level = g.player_level();
    let menus = g.events.iter().filter(|e| matches!(e.kind, EventKind::MenuOpened { menu: fnv_game::game::menus::LEVEL_UP })).count();
    success(&mut g, "AdvancePCLevel");
    assert_eq!(g.player_level(), level + 1.0);
    assert_eq!(g.menu, Some(fnv_game::game::menus::LEVEL_UP));
    assert_eq!(g.player.xp, 123.5);
    success(&mut g, "AdvLevel");
    assert_eq!(g.player_level(), level + 2.0);
    assert_eq!(g.player.xp, 123.5);
    assert_eq!(g.events.iter().filter(|e| matches!(e.kind, EventKind::MenuOpened { menu: fnv_game::game::menus::LEVEL_UP })).count(), menus + 1, "advancing while already open retains the existing menu event policy");
    // Main's source compiler drops surplus call-line arguments with a warning.
    // This input is an admitted advancement, not a runtime-invalid argument case.
    let compiled = compile_control(&g, "AdvancePCLevel 1");
    assert!(compiled.errors().next().is_none(), "{:?}", compiled.diags);
    assert!(compiled.diags.iter().any(|d| !d.error && d.kind == "surplus_arguments" && d.detail == "1 dropped: 1"));
    success(&mut g, "AdvancePCLevel 1");
    assert_eq!(g.player_level(), level + 3.0);
    assert_eq!(g.player.xp, 123.5);
}

#[test]
fn attributes_keep_modifier_damage_and_carry_consumers_and_tell_handlers() {
    let f = fixture(); let mut g = game(&f);
    success(&mut g, "player.ModAV Strength 2\nplayer.DamageAV Strength 1");
    success(&mut g, "SetOnActorValueChangeEventHandler 1 (Begin Function {int code, float before, float after}\nplayer.AuxVarSetFlt \"*progressionCode\" code 0\nplayer.AuxVarSetFlt \"*progressionChange\" (after - before) 0\nEnd) 0");
    let before = functions::actor_value_parts(&mut g, FormId::PLAYER_REF, av::STRENGTH).unwrap();
    let carry = g.world.encumbrance(FormId::PLAYER_REF).unwrap().capacity;
    let neighbor = functions::actor_value(&mut g, NPC, av::STRENGTH).unwrap();
    success(&mut g, "ProgressionNeighborRef.ModPCAttribute Strength 1");
    assert_eq!(functions::actor_value_parts(&mut g, FormId::PLAYER_REF, av::STRENGTH).unwrap(), [before[0] + 1.0, before[1], before[2]]);
    assert_eq!(functions::actor_value(&mut g, NPC, av::STRENGTH).unwrap(), neighbor);
    assert!(g.world.encumbrance(FormId::PLAYER_REF).unwrap().capacity > carry);
    assert!(g.world.changed().any(|(id, _)| id == FormId::PLAYER_REF));
    assert_eq!(success(&mut g, "SetFunctionValue (player.AuxVarGetFlt \"*progressionCode\" 0)"), XVal::Num(f64::from(av::STRENGTH)));
    assert_eq!(success(&mut g, "SetFunctionValue (player.AuxVarGetFlt \"*progressionChange\" 0)"), XVal::Num(1.0));
    success(&mut g, "ModPCAttribute Strength -1");
    assert_eq!(functions::actor_value_parts(&mut g, FormId::PLAYER_REF, av::STRENGTH).unwrap(), before);
}

#[test]
fn exact_removal_keeps_fractions_other_axis_other_reputation_and_events() {
    let f = fixture(); let mut g = game(&f);
    success(&mut g, "SetReputation ProgressionRep 0 3.75\nSetReputation ProgressionRep 1 11.5\nSetReputation ProgressionOtherRep 1 9.25");
    let start = g.events.len();
    success(&mut g, "RemoveReputationExact ProgressionRep 0 0.5");
    assert_eq!(g.player.reputation[&REP], [3.25, 11.5]);
    assert_eq!(g.player.reputation[&OTHER_REP], [0.0, 9.25]);
    assert!(g.events[start..].iter().any(|e| matches!(e.kind, EventKind::ReputationChanged { reputation, axis: 0, value } if reputation == REP && value == 3.25)));
    success(&mut g, "RemoveReputationExact ProgressionRep 1 100.25");
    assert_eq!(g.player.reputation[&REP], [3.25, 0.0]);
    success(&mut g, "SetReputation ProgressionRep 1 30\nRemoveReputationExact ProgressionRep 1 0.25");
    assert_eq!(g.player.reputation[&REP][1], 29.75, "loss keeps a value above the scale");
    assert_eq!(success(&mut g, "SetFunctionValue (GetReputation ProgressionRep 1)"), XVal::Num(29.75));
}

#[test]
fn explicit_null_receiver_continues_and_mutates_player_while_non_player_commands_still_abort() {
    let f = fixture();
    let mut g = game(&f);
    let before_str = functions::actor_value_parts(&mut g, FormId::PLAYER_REF, av::STRENGTH).unwrap();
    let neighbor_str = functions::actor_value(&mut g, NPC, av::STRENGTH).unwrap();

    // Uninitialized `ref rNull` and explicit `let rNull := 0` both continue execution
    // and mutate the player, matching w4g_06_player_cmds_explicit_receiver.
    assert_eq!(
        success(&mut g, "ref rNull\nrNull.ModPCAttribute Strength 1\nSetFunctionValue 1"),
        XVal::Num(1.0)
    );
    assert_eq!(
        functions::actor_value_parts(&mut g, FormId::PLAYER_REF, av::STRENGTH).unwrap(),
        [before_str[0] + 1.0, before_str[1], before_str[2]]
    );
    assert_eq!(
        success(&mut g, "ref rNull\nlet rNull := 0\nrNull.ModPCAttribute Strength 1\nSetFunctionValue 1"),
        XVal::Num(1.0)
    );
    assert_eq!(
        functions::actor_value_parts(&mut g, FormId::PLAYER_REF, av::STRENGTH).unwrap(),
        [before_str[0] + 2.0, before_str[1], before_str[2]]
    );
    assert_eq!(functions::actor_value(&mut g, NPC, av::STRENGTH).unwrap(), neighbor_str);

    // Sibling player-source commands share the same null-receiver policy.
    let level = g.player_level();
    assert_eq!(success(&mut g, "ref rNull\nrNull.AdvancePCLevel\nSetFunctionValue 1"), XVal::Num(1.0));
    assert_eq!(g.player_level(), level + 1.0);
    success(&mut g, "SetReputation ProgressionRep 0 3.5");
    assert_eq!(
        success(&mut g, "ref rNull\nrNull.RemoveReputationExact ProgressionRep 0 0.5\nSetFunctionValue 1"),
        XVal::Num(1.0)
    );
    assert_eq!(g.player.reputation[&REP], [3.0, 0.0]);

    // Non-player extender commands MUST still reject explicit null receivers before dispatch.
    runtime_failure(&mut g, "ref rNull\nrNull.ClampToGround\nSetFunctionValue 1", "clamptoground called on no reference");
    runtime_failure(&mut g, "ref rNull\nrNull.GetDroppedRefs\nSetFunctionValue 1", "getdroppedrefs called on no reference");
}

#[test]
fn bad_arguments_and_unknown_nonzero_receivers_do_not_mutate_player_state() {
    let f = fixture(); let mut g = game(&f);
    // A missing implementation could fail receiver admission in the VM too.
    // Require actual supported dispatch first, on a separate stateful control game.
    let mut admitted = game(&f);
    success(&mut admitted, "ModPCAttribute Strength 1\nRemoveReputationExact ProgressionRep 0 0.5\nAdvancePCLevel");
    assert_eq!(functions::actor_value_parts(&mut admitted, FormId::PLAYER_REF, av::STRENGTH).unwrap(), [6.0, 0.0, 0.0]);
    assert_eq!(admitted.player.reputation[&REP], [0.0, 0.0]);
    assert_eq!(admitted.player_level(), g.player_level() + 1.0);
    let level = g.player_level();
    let strength = functions::actor_value_parts(&mut g, FormId::PLAYER_REF, av::STRENGTH).unwrap();
    for (body, reason) in [
        ("ModPCAttribute Repair 1", "ModPCAttribute requires a SPECIAL attribute"),
        ("ModPCAttribute NoSuchAttribute 1", "no actor value named \"NoSuchAttribute\""),
        ("ModPCAttribute Strength \"bad\"", "parameter 2 is missing"),
        // Player resolves to reference 0x14; this fixture names its base, not the reference.
        ("RemoveReputationExact Player 0 1", "0x00000014 is not a reputation"),
        ("ref receiver\nlet receiver := 0x123456\nreceiver.ModPCAttribute Strength 1", "invalid calling reference"),
    ] {
        runtime_failure(&mut g, body, reason);
        assert_eq!(g.player_level(), level, "{body}");
        assert_eq!(functions::actor_value_parts(&mut g, FormId::PLAYER_REF, av::STRENGTH).unwrap(), strength, "{body}");
        assert!(g.player.reputation.is_empty(), "{body}");
    }
    // A compiler rejection has its own oracle and cannot substitute for any runtime
    // failure above. This malformed operand never dispatches a player command.
    let malformed = "ModPCAttribute (";
    let compiled = compile_control(&g, malformed);
    assert!(compiled.errors().any(|d| d.kind == "syntax" && d.detail == "expression ends early"), "{:?}", compiled.diags);
    let before = g.events.len();
    assert_eq!(query(&mut g, malformed), Err("script did not compile".to_string()));
    assert!(g.events[before..].iter().any(|e| matches!(&e.kind, EventKind::Skipped { unit, what } if unit == "query" && what == "query: syntax: expression ends early")));
    assert_eq!(g.player_level(), level);
    assert_eq!(functions::actor_value_parts(&mut g, FormId::PLAYER_REF, av::STRENGTH).unwrap(), strength);
    assert!(g.player.reputation.is_empty());
}

#[test]
fn exact_removal_invalid_axis_retains_the_existing_script_return_policy() {
    let f = fixture(); let mut g = game(&f);
    success(&mut g, "SetReputation ProgressionRep 0 2.5");
    // Valid exact-removal dispatch must work before testing write_axis. Unsupported
    // command errors cannot satisfy the exact axis-specific failure below.
    success(&mut g, "RemoveReputationExact ProgressionRep 0 0.25");
    assert_eq!(g.player.reputation[&REP], [2.25, 0.0]);
    success(&mut g, "SetReputation ProgressionRep 0 2.5");
    runtime_failure(&mut g, "RemoveReputationExact ProgressionRep 2 0.5\nModPCAttribute Strength 1", "reputation axis 2 is not 0 (infamy) or 1 (fame)");
    assert_eq!(g.player.reputation[&REP], [2.5, 0.0]);
    assert_eq!(functions::actor_value_parts(&mut g, FormId::PLAYER_REF, av::STRENGTH).unwrap(), [5.0, 0.0, 0.0]);
}

#[test]
fn player_changes_roundtrip_through_the_real_snapshot_and_resume_source_calls() {
    let f = fixture(); let mut g = game(&f);
    success(&mut g, "ModPCAttribute Strength 2\nSetReputation ProgressionRep 1 7.75\nRemoveReputationExact ProgressionRep 1 0.5\nAdvancePCLevel");
    let level = g.player_level();
    let snapshot: GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.checked_snapshot().unwrap()).unwrap()).unwrap();
    let world = World::restore(&f.forms, &f.refs, &snapshot.sim).unwrap();
    let mut restored = Game::restore(&f.data, world, g.cfg, &snapshot).unwrap();
    assert_eq!(restored.player_level(), level);
    assert_eq!(functions::actor_value_parts(&mut restored, FormId::PLAYER_REF, av::STRENGTH).unwrap(), [7.0, 0.0, 0.0]);
    assert_eq!(restored.player.reputation[&REP], [0.0, 7.25]);
    success(&mut restored, "ModPCAttribute Strength -1\nRemoveReputationExact ProgressionRep 1 0.25");
    assert_eq!(restored.player.reputation[&REP], [0.0, 7.0]);
    assert_eq!(functions::actor_value_parts(&mut restored, FormId::PLAYER_REF, av::STRENGTH).unwrap(), [6.0, 0.0, 0.0]);
}

#[test]
fn focused_vanilla_census_keeps_source_support_separate_from_native_encoding() {
    let f = fixture(); let mut g = game(&f);
    for (name, source) in [
        ("AdvancePCLevel", "AdvancePCLevel"),
        ("ModPCAttribute", "ModPCAttribute Strength 0"),
        ("RemoveReputationExact", "RemoveReputationExact ProgressionRep 0 0.25"),
        ("SetPCExpelled", "SetPCExpelled ExpelledFaction 0"),
        ("ShowNameMenu", "ShowNameMenu"),
        ("KillAllActors", "KillAllActors"),
    ] {
        assert!(g.data.table.unnumbered.contains_key(&name.to_ascii_lowercase()));
        assert_eq!(g.data.table.by_name(name), None, "native encoding is unmeasured");
        success(&mut g, source);
    }
    assert_eq!(g.data.table.by_name("GetPCExpelled"), Some(0x10C1));
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let catalogue = serde_json::from_str(&std::fs::read_to_string(root).unwrap()).unwrap();
    let (console, other) = functions::console::unhandled_unused(&catalogue);
    assert_eq!((console.len(), other.len()), (33, 76)); // SameSexAsPC registered (#3787)
    for name in ["AdvancePCLevel", "ModPCAttribute", "RemoveReputationExact", "SetPCExpelled", "GetPCExpelled", "ShowNameMenu", "KillAllActors"] {
        assert!(!console.iter().chain(&other).any(|n| n == name));
    }
}

#[test]
fn set_and_get_pc_expelled_track_membership_ctda_persistence_and_guards() {
    let f = fixture();
    let mut g = game(&f);

    // Real catalogue-backed CTDA parse for GetPCExpelled (condition index 193, opcode 0x10C1,
    // ptFaction) using a nonidentity local-to-load-order FormID map.
    const LOCAL_FAC: u32 = 0x123;
    let ctda = |raw: u32| {
        cond::parse(
            &serde_json::json!({
                "type": 0, "function": 193, "comparison_value_raw": 1.0f32.to_bits(),
                "parameter1_raw": raw, "parameter2_raw": 0, "run_on": 0,
            }),
            &g.data.param_types,
            &|id| (id == LOCAL_FAC).then_some(EXPELLED_FAC.0),
        )
        .unwrap()
    };
    let c_expelled = ctda(LOCAL_FAC);
    assert_eq!(c_expelled.opcode(), 0x10C1);
    assert!(matches!(c_expelled.params.as_slice(), [ArgValue::Value(Value::Ref(id))] if *id == EXPELLED_FAC.0));

    // Initial state: not expelled, snapshot omits empty expelled_from.
    assert_eq!(success(&mut g, "SetFunctionValue (GetPCExpelled ExpelledFaction)"), XVal::Num(0.0));
    assert_eq!(g.eval_conditions(std::slice::from_ref(&c_expelled), Some(FormId::PLAYER_REF), None, "initial")[0].value, 0.0);
    assert!(!serde_json::to_string(&g.checked_snapshot().unwrap()).unwrap().contains("expelled_from"));

    // Join ExpelledFaction on both player and neighbor; also keep OtherFaction + enemy_of separate.
    success(&mut g, "player.AddToFaction ExpelledFaction 2\nProgressionNeighborRef.AddToFaction ExpelledFaction 1\nSetPCEnemyOfFaction OtherFaction 1");
    assert_eq!(success(&mut g, "SetFunctionValue (player.GetInFaction ExpelledFaction)"), XVal::Num(1.0));
    assert_eq!(success(&mut g, "SetFunctionValue (player.GetFactionRank ExpelledFaction)"), XVal::Num(2.0));

    // Valid explicit receiver still acts on the player, expels membership, and leaves neighbor untouched.
    success(&mut g, "ProgressionNeighborRef.SetPCExpelled ExpelledFaction 1");
    assert_eq!(success(&mut g, "SetFunctionValue (GetPCExpelled ExpelledFaction)"), XVal::Num(1.0));
    assert_eq!(success(&mut g, "SetFunctionValue (GetPCExpelled OtherFaction)"), XVal::Num(0.0));
    assert_eq!(g.eval_conditions(std::slice::from_ref(&c_expelled), Some(FormId::PLAYER_REF), None, "expelled")[0].value, 1.0);
    assert!(g.conditions_pass(std::slice::from_ref(&c_expelled), Some(FormId::PLAYER_REF), None, "expelled pass"));
    assert_eq!(success(&mut g, "SetFunctionValue (player.GetInFaction ExpelledFaction)"), XVal::Num(0.0));
    assert_eq!(success(&mut g, "SetFunctionValue (player.GetFactionRank ExpelledFaction)"), XVal::Num(-1.0));
    assert_eq!(success(&mut g, "SetFunctionValue (ProgressionNeighborRef.GetFactionRank ExpelledFaction)"), XVal::Num(1.0));
    assert_eq!(success(&mut g, "SetFunctionValue (GetPCEnemyOfFaction ExpelledFaction)"), XVal::Num(0.0));
    assert_eq!(success(&mut g, "SetFunctionValue (GetPCEnemyOfFaction OtherFaction)"), XVal::Num(1.0));

    // Save/restore preserves expelled_from and keeps CTDA & source readback agreeing.
    let snap: GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.checked_snapshot().unwrap()).unwrap()).unwrap();
    let world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
    let mut restored = Game::restore(&f.data, world, g.cfg, &snap).unwrap();
    assert_eq!(success(&mut restored, "SetFunctionValue (GetPCExpelled ExpelledFaction)"), XVal::Num(1.0));
    assert_eq!(restored.eval_conditions(std::slice::from_ref(&c_expelled), Some(FormId::PLAYER_REF), None, "restored")[0].value, 1.0);

    // Clearing the flag via ref expression clears GetPCExpelled without re-adding faction membership.
    success(&mut restored, "ref rFac\nlet rFac := ExpelledFaction\nSetPCExpelled (rFac) 0");
    assert_eq!(success(&mut restored, "SetFunctionValue (GetPCExpelled ExpelledFaction)"), XVal::Num(0.0));
    assert_eq!(restored.eval_conditions(std::slice::from_ref(&c_expelled), Some(FormId::PLAYER_REF), None, "cleared")[0].value, 0.0);
    assert_eq!(success(&mut restored, "SetFunctionValue (player.GetInFaction ExpelledFaction)"), XVal::Num(0.0));

    // Explicit null receiver still targets the player and continues execution.
    assert_eq!(
        success(&mut restored, "ref empty\nempty.SetPCExpelled ExpelledFaction 1\nSetFunctionValue (GetPCExpelled ExpelledFaction)"),
        XVal::Num(1.0)
    );
    success(&mut restored, "ref empty\nempty.SetPCExpelled ExpelledFaction 0");
    assert_eq!(success(&mut restored, "SetFunctionValue (GetPCExpelled ExpelledFaction)"), XVal::Num(0.0));

    // Unknown non-zero receiver and invalid-input boundaries fail cleanly without mutating state.
    runtime_failure(&mut restored, "ref receiver\nlet receiver := 0x123456\nreceiver.SetPCExpelled ExpelledFaction 1", "invalid calling reference");
    runtime_failure(&mut restored, "SetPCExpelled ProgressionRep 1", "parameter 1 (0x00000850) is not a faction");
    runtime_failure(&mut restored, "SetPCExpelled NoSuchFaction 1", "unknown faction \"NoSuchFaction\"");
    // Vanilla opcode GetPCExpelled reports CallFailed and returns 0 on a non-faction form.
    let before_bad_get = restored.events.len();
    assert_eq!(query(&mut restored, "SetFunctionValue (GetPCExpelled ProgressionRep)"), Ok(XVal::Num(0.0)));
    assert!(restored.events[before_bad_get..].iter().any(|e| matches!(
        &e.kind,
        EventKind::CallFailed { name, error, .. } if name == "GetPCExpelled" && error == "parameter 1 (0x00000850) is not a faction"
    )));
    assert!(restored.player.expelled_from.is_empty());

    // Null-form CTDA shortcut returns 0 without CallFailed.
    let c_null = ctda(0);
    let before = restored.events.len();
    assert_eq!(restored.eval_conditions(std::slice::from_ref(&c_null), Some(FormId::PLAYER_REF), None, "null")[0].value, 0.0);
    assert!(!restored.events[before..].iter().any(|e| matches!(e.kind, EventKind::CallFailed { .. })));
}

#[test]
fn show_name_menu_opens_the_name_menu_and_guards_receivers() {
    let f = fixture();
    let mut g = game(&f);

    // Unknown non-zero receivers must fail without opening the menu.
    runtime_failure(&mut g, "ref r\nlet r := 0x123456\nr.ShowNameMenu", "invalid calling reference");
    assert_eq!(g.menu, None);

    // Explicit null receiver opens menus::NAME (1051) and continues execution.
    let mut g_null = game(&f);
    assert_eq!(success(&mut g_null, "ref empty\nempty.ShowNameMenu\nSetFunctionValue (MenuMode 1051)"), XVal::Num(1.0));
    assert_eq!(g_null.menu, Some(fnv_game::game::menus::NAME));

    // Omitted receiver opens menus::NAME (1051) and emits MenuOpened once.
    success(&mut g, "ShowNameMenu");
    assert_eq!(g.menu, Some(fnv_game::game::menus::NAME));
    assert_eq!(success(&mut g, "SetFunctionValue (MenuMode 1051)"), XVal::Num(1.0));
    let opened = g.events.iter().filter(|e| matches!(e.kind, EventKind::MenuOpened { menu: fnv_game::game::menus::NAME })).count();
    assert_eq!(opened, 1);

    // Calling again while already open preserves existing Game::open_menu deduplication.
    success(&mut g, "ProgressionNeighborRef.ShowNameMenu");
    assert_eq!(g.menu, Some(fnv_game::game::menus::NAME));
    assert_eq!(g.events.iter().filter(|e| matches!(e.kind, EventKind::MenuOpened { menu: fnv_game::game::menus::NAME })).count(), 1);
}

#[test]
fn source_kill_all_actors_and_alias_share_the_loaded_area_kill_pass() {
    let f = fixture();
    let mut g = game(&f);
    g.loaded_cells.insert(ROOM);
    success(&mut g, "ProgressionEssentialRef.SetActorRefEssential 1");

    // Unknown non-zero receiver or invalid killer must fail without killing or knocking out any loaded actor.
    runtime_failure(&mut g, "ref r\nlet r := 0x123456\nr.KillAllActors", "invalid calling reference");
    runtime_failure(&mut g, "ref k\nlet k := 0x123456\nKillAllActors k", "invalid killer reference");
    runtime_failure(&mut g, "KillAllActors \"bad\"", "KillAllActors killer argument must be a reference");
    assert!(!g.is_dead(NPC) && !g.is_dead(ESSENTIAL_NPC) && !g.is_dead(REMOTE_NPC));

    // Explicit null receiver still executes KillAllActors cleanly.
    let mut g_null = game(&f);
    g_null.loaded_cells.insert(ROOM);
    assert_eq!(success(&mut g_null, "ref empty\nempty.KillAllActors\nSetFunctionValue 1"), XVal::Num(1.0));
    assert!(g_null.is_dead(NPC));
    assert!(!g_null.is_dead(FormId::PLAYER_REF));

    // KillAllActors with explicit killer kills loaded non-essential actors, knocks out
    // essential ones, attributes the killer, and spares the player and unloaded cells.
    success(&mut g, "KillAllActors Player");
    assert!(g.is_dead(NPC), "loaded non-essential actor must be killed");
    assert_eq!(success(&mut g, "SetFunctionValue (ProgressionNeighborRef.IsKiller Player)"), XVal::Num(1.0));
    assert!(!g.is_dead(ESSENTIAL_NPC), "essential actor must be knocked out, not killed");
    assert!(g.is_out_cold(ESSENTIAL_NPC) && g.knockout(ESSENTIAL_NPC).is_some(), "essential actor must enter unconscious state");
    assert!(!g.is_dead(FormId::PLAYER_REF), "player must never be killed by KillAllActors");
    assert!(!g.is_dead(REMOTE_NPC), "actor in an unloaded cell must be spared");

    // Alias KillAll (0 args) and empty-ref optional killer both execute the same pass cleanly.
    let mut g2 = game(&f);
    g2.loaded_cells.insert(ROOM);
    success(&mut g2, "ref k\nKillAll k");
    assert!(g2.is_dead(NPC));
    assert_eq!(success(&mut g2, "SetFunctionValue (ProgressionNeighborRef.IsKiller Player)"), XVal::Num(0.0));
    assert!(!g2.is_dead(REMOTE_NPC));

    // Console `kill_all_actors` keeps its formatted summary via the shared pass.
    let mut g3 = game(&f);
    g3.loaded_cells.insert(ROOM);
    success(&mut g3, "ProgressionEssentialRef.SetActorRefEssential 1");
    let lines = functions::console::kill_all_actors(&mut g3, &functions::console::ConsoleCtx::default(), &[]).unwrap();
    assert_eq!(lines, vec!["KillAllActors >> 1 killed, 1 essential knocked out"]);
}
