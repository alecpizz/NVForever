//! Original source-shaped keyword/filter/trade consumers on own synthetic forms.
use crate::common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{
    game::{ActMode, Game, GameConfig},
    GameData,
};
use fnv_script::{vm::XVal, FunctionTable};
use fnv_sim::{FormId, FormStore, RefTable, World};

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}
fn source_script(id: u32, name: &str, body: &str) -> Vec<u8> {
    record(
        b"SCPT",
        0,
        id,
        &[
            sub(b"EDID", z(name)),
            unit(0, &[], &[], &[])[0].clone(),
            sub(b"SCDA", vec![]),
            sub(
                b"SCTX",
                format!("scn {name}\nstring_var ownSourceMarker\n{body}").into_bytes(),
            ),
        ],
    )
}
fn fixture(scripted: bool) -> Fixture {
    let mut bytes = tes4();
    let mut trader = vec![
        sub(b"EDID", z("OwnedTradeActor")),
        sub(b"FULL", z("Synthetic trader")),
    ];
    if scripted {
        trader.push(sub(b"SCRI", 0x850u32.to_le_bytes().to_vec()));
    }
    bytes.extend(top(
        b"NPC_",
        &[
            record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))]),
            record(b"NPC_", 0, 0x810, &trader),
            record(b"NPC_", 0, 0x811, &[sub(b"EDID", z("OwnedCompanionActor"))]),
            record(b"NPC_", 0, 0x812, &[sub(b"EDID", z("OwnedOtherActor"))]),
        ]
        .concat(),
    ));
    bytes.extend(top(b"SCPT", &[
        source_script(0x850,"OwnedActivationBlock","Begin OnActivate\nSetModLocalData \"owned.order\" 3\nSetModLocalData \"owned.block\" ((GetModLocalData \"owned.block\") + 1)\nEnd\n"),
        source_script(0x851,"OwnedVoid","Begin Function {}\nCall OwnedZero\nEnd\n"),
        source_script(0x852,"OwnedZero","Begin Function {}\nSetFunctionValue 0\nEnd\n"),
        source_script(0x853,"OwnedRemoveLater","Begin Function {ref actor, int allowed}\nRemoveEventHandler \"ShowOff:OnPreActivate\" OwnedLater\nEnd\n"),
        source_script(0x854,"OwnedLater","Begin Function {ref actor, int allowed}\nSetModLocalData \"owned.later\" ((GetModLocalData \"owned.later\") + 1)\nEnd\n"),
    ].concat()));
    let plugins = vec![Plugin::parse(&bytes, "OwnedPreactivation.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let path = root.join("script/functions.json");
    let json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(
        &lo,
        &schema,
        FunctionTable::load(&path).unwrap(),
        &json,
        &refs,
    );
    for id in 0x850..=0x854 {
        let unit = data.script(FormId(id)).unwrap_or_else(|| {
            let (units, issues) = fnv_script::unit::read_load_order(&lo, &schema);
            let headers: Vec<_> = units.iter().map(|u| (&u.owner, &u.header, u.has_scda, u.source.is_some())).collect();
            panic!("missing enabled fixture SCPT 0x{:08X}; decoded headers={headers:?}; read issues={issues:?}; source selections={:?}", id, data.source_units);
        });
        assert!(unit.unit.header.flags & 1 != 0, "fixture SCPT 0x{:08X} disabled: {:?}", id, unit.unit.header);
        assert!(unit.unit.source.is_some());
        assert!(
            data.source_units
                .iter()
                .any(|s| s.key == unit.key() && s.error.is_none()),
            "source unit {id:X} must actually compile"
        );
    }
    Fixture { data, forms, refs }
}
fn query(g: &mut Game, body: &str) -> XVal {
    g.run_query(
        &format!("ref ownRef\narray_var recipients\nBegin Function {{}}\n{body}\nEnd"),
        Default::default(),
    )
    .unwrap()
}
fn num(g: &mut Game, key: &str) -> f64 {
    let XVal::Num(n) = query(g, &format!("SetFunctionValue (GetModLocalData \"{key}\")")) else {
        panic!("number")
    };
    n
}
fn target(g: &mut Game, key: &str) -> FormId {
    let XVal::Ref(id) = query(g, &format!("SetFunctionValue (GetModLocalData \"{key}\")")) else {
        panic!("reference")
    };
    assert_ne!(id, 0);
    FormId(id)
}
fn game(f: &Fixture) -> Game<'_> {
    let mut g = Game::new(
        &f.data,
        World::new(&f.forms, &f.refs, Default::default()),
        GameConfig {
            strict: true,
            ..Default::default()
        },
    );
    query(&mut g,"SetModLocalData \"owned.trader\" (player.PlaceAtMe OwnedTradeActor)\nSetModLocalData \"owned.companion\" (player.PlaceAtMe OwnedCompanionActor)\nSetModLocalData \"owned.other\" (player.PlaceAtMe OwnedOtherActor)");
    g
}
fn no_errors(g: &Game) {
    assert!(
        !g.events
            .iter()
            .any(|e| matches!(e.kind, fnv_game::trace::EventKind::ScriptError { .. })),
        "{:?}",
        g.events
    );
}

#[test]
fn improved_console_guard_passes_and_missing_plugin_guard_still_stops() {
    let f = fixture(false);
    let mut g = game(&f);
    assert_eq!(
        query(
            &mut g,
            "SetFunctionValue (IsPluginInstalled (\"Improved Console\"))"
        ),
        XVal::Num(1.0)
    );
    assert_eq!(
        query(
            &mut g,
            "SetFunctionValue (GetPluginVersion (\"Improved Console\"))"
        ),
        XVal::Num(3.0)
    );
    assert_eq!(
        query(
            &mut g,
            "SetFunctionValue (GetPluginVersion (\"Improved Console\") >= 1)"
        ),
        XVal::Num(1.0)
    );
    // Negative control: a genuinely missing plugin still stops before registration.
    query(&mut g,"if eval !(IsPluginInstalled (\"NoSuchConsolePlugin\"))\nSetModLocalData \"owned.missing_guard\" 1\nreturn\nendif\nSetModLocalData \"owned.missing_after\" 1");
    assert_eq!(num(&mut g, "owned.missing_guard"), 1.0);
    assert_eq!(num(&mut g, "owned.missing_after"), 0.0);
    // Positive control: the Improved Console guard now passes and registers the handler.
    query(&mut g,"if eval !(IsPluginInstalled (\"Improved Console\"))\nSetModLocalData \"owned.guard\" 1\nreturn\nendif\nSetEventHandlerAlt \"ShowOff:OnPreActivate\" OwnedZero\nSetModLocalData \"owned.after_guard\" 1");
    assert_eq!(num(&mut g, "owned.guard"), 0.0);
    assert_eq!(num(&mut g, "owned.after_guard"), 1.0);
    let trader = target(&mut g, "owned.trader");
    assert!(
        g.default_activation_by_hand(trader).unwrap().is_none(),
        "registered preactivation handler cancels hand activation"
    );
    no_errors(&g);
}

#[test]
fn observer_prehandler_and_block_order_distinguishes_explicit_zero_from_void() {
    for (callback, cancels) in [("OwnedVoid", false), ("OwnedZero", true)] {
        let f = fixture(true);
        let mut g = game(&f);
        // Both remain ordinary Call value 0. Only the explicit setter may veto.
        assert_eq!(
            query(&mut g, &format!("SetFunctionValue (Call {callback})")),
            XVal::Num(0.0)
        );
        query(&mut g,&format!("SetEventHandler \"OnActivate\" (Begin Function {{ref object, ref actor}}\nSetModLocalData \"owned.order\" 1\nEnd)\nSetEventHandlerAlt \"ShowOff:OnPreActivate\" (Begin Function {{ref actor, int allowed}}\nSetModLocalData \"owned.saw_observer\" (GetModLocalData \"owned.order\")\nSetModLocalData \"owned.order\" 2\nSetModLocalData \"owned.allowed\" allowed\n{}\nEnd)",if cancels{"SetFunctionValue 0"}else{"Call OwnedZero"}));
        let trader = target(&mut g, "owned.trader");
        let before = g.world.snapshot();
        let result = g
            .activate(FormId::PLAYER_REF, trader, ActMode::Hand)
            .unwrap();
        assert_eq!(
            num(&mut g, "owned.saw_observer"),
            1.0,
            "preactivation first producer must hear observer first"
        );
        assert_eq!(num(&mut g, "owned.allowed"), 1.0);
        assert_eq!(num(&mut g, "owned.order"), if cancels { 2.0 } else { 3.0 });
        assert_eq!(num(&mut g, "owned.block"), if cancels { 0.0 } else { 1.0 });
        if cancels {
            assert_eq!(result["result"], "prevented");
            assert_eq!(g.world.snapshot(), before);
        }
        no_errors(&g);
    }
}

#[test]
fn priority_and_allowed_filters_update_after_a_sticky_veto() {
    let f = fixture(false);
    let mut g = game(&f);
    query(&mut g,"SetEventHandlerAlt \"ShowOff:OnPreActivate\" (Begin Function {ref actor, int allowed}\nSetModLocalData \"owned.first\" allowed\nSetFunctionValue 0\nEnd) \"priority\"::3\nSetEventHandlerAlt \"ShowOff:OnPreActivate\" (Begin Function {ref actor, int allowed}\nSetModLocalData \"owned.must_skip\" 1\nEnd) \"priority\"::2 2::1\nSetEventHandlerAlt \"ShowOff:OnPreActivate\" (Begin Function {ref actor, int allowed}\nSetModLocalData \"owned.last\" 1\nSetModLocalData \"owned.last_allowed\" allowed\nSetFunctionValue 1\nEnd) \"priority\"::1 2::0");
    let trader = target(&mut g, "owned.trader");
    assert!(
        g.default_activation_by_hand(trader).unwrap().is_none(),
        "one explicit veto remains sticky"
    );
    assert_eq!(num(&mut g, "owned.first"), 1.0);
    assert_eq!(num(&mut g, "owned.must_skip"), 0.0);
    assert_eq!(num(&mut g, "owned.last"), 1.0);
    assert_eq!(num(&mut g, "owned.last_allowed"), 0.0);
    no_errors(&g);
}

#[test]
fn keyword_recipients_and_custom_event_open_existing_trade_queues() {
    for companion in [false, true] {
        let f = fixture(false);
        let mut g = game(&f);
        query(&mut g,"AssignKeyword (EditorIDToFormID \"OwnedTradeActor\") \"OwnedTrader\"\nAssignKeyword (EditorIDToFormID \"OwnedCompanionActor\") \"OwnedCompanion\"\nlet ownRef := GetModLocalData \"owned.companion\"\nownRef.SetPlayerTeammate 1\nlet recipients := GetKeywordForms \"OwnedTrader\"\nar_Cat recipients (GetKeywordForms \"OwnedCompanion\")\nSetEventHandlerAlt \"ShowOff:OnPreActivate\" (Begin Function {ref actor, int allowed}\nif !(IsKeyPressed 42 2)\nreturn\nendif\nSetModLocalData \"owned.receiver\" ThisAlt\nSetFunctionValue 0\nDispatchEventAlt \"OwnedTradeOpen\" ThisAlt\nEnd) 0::recipients 1::player\nSetEventHandlerAlt \"OwnedTradeOpen\" (Begin Function {ref recipient}\nif HasKeyword recipient \"OwnedCompanion\"\nrecipient.OpenTeammateContainer\nelse\nrecipient.ShowBarterMenu 35\nendif\nEnd) 1::recipients");
        let selected = target(
            &mut g,
            if companion {
                "owned.companion"
            } else {
                "owned.trader"
            },
        );
        assert!(
            g.default_activation_by_hand(selected).unwrap().is_some(),
            "ordinary unmodified input keeps normal activation"
        );
        assert!(g.barter_requests.is_empty() && g.teammate_containers.is_empty());
        g.input.held.insert(42);
        assert!(
            g.default_activation_by_hand(selected).unwrap().is_none(),
            "modified regular activation must take the real preactivate path"
        );
        assert_eq!(
            query(
                &mut g,
                "SetFunctionValue (GetModLocalData \"owned.receiver\")"
            ),
            XVal::Ref(selected.0)
        );
        if companion {
            assert_eq!(g.teammate_containers, vec![selected]);
            assert!(g.barter_requests.is_empty());
        } else {
            assert_eq!(g.barter_requests.len(), 1);
            assert_eq!(g.barter_requests[0].merchant, selected);
            assert_eq!(g.barter_requests[0].discount, 35);
            assert!(g.teammate_containers.is_empty());
        }
        let other = target(&mut g, "owned.other");
        assert!(
            g.default_activation_by_hand(other).unwrap().is_some(),
            "unlisted actor bypasses recipient filter"
        );
        no_errors(&g);
    }
}

#[test]
fn callback_filter_snapshot_survives_unregister_until_dispatch_finishes() {
    let f = fixture(false);
    let mut g = game(&f);
    query(&mut g,"AssignKeyword OwnedTradeActor \"OwnedRecipient\"\nSetEventHandlerAlt \"ShowOff:OnPreActivate\" OwnedRemoveLater \"priority\"::2\nSetEventHandlerAlt \"ShowOff:OnPreActivate\" OwnedLater \"priority\"::1 0::(GetKeywordForms \"OwnedRecipient\")");
    let trader = target(&mut g, "owned.trader");
    let arrays = g.ext.arrays.len();
    assert!(arrays > 0);
    assert!(g.default_activation_by_hand(trader).unwrap().is_some());
    assert_eq!(
        num(&mut g, "owned.later"),
        0.0,
        "unregistered later callback is skipped in the same dispatch"
    );
    assert_eq!(
        g.ext.arrays.len(),
        arrays - 1,
        "dispatch releases its final array-filter hold"
    );
    assert!(g.default_activation_by_hand(trader).unwrap().is_some());
    assert_eq!(num(&mut g, "owned.later"), 0.0);
    no_errors(&g);
}

#[test]
fn expression_lambda_veto_and_mid_dispatch_unregister_on_pre_scripted_activate() {
    let f = fixture(true);
    let mut g = game(&f);
    query(
        &mut g,
        "SetEventHandlerAlt \"ShowOff:OnPreScriptedActivate\" ({ref actor, int blockFlag, int allowed} => 0) \"priority\"::2",
    );
    let trader = target(&mut g, "owned.trader");
    let res = g.activate(FormId::PLAYER_REF, trader, ActMode::Block).unwrap();
    assert_eq!(res["result"], "prevented", "expression-only lambda => 0 must veto");
    assert_eq!(num(&mut g, "owned.block"), 0.0);

    let f2 = fixture(true);
    let mut g2 = game(&f2);
    query(
        &mut g2,
        "SetEventHandlerAlt \"ShowOff:OnPreScriptedActivate\" ({ref actor, int blockFlag, int allowed} => 1) \"priority\"::2",
    );
    let trader2 = target(&mut g2, "owned.trader");
    let res2 = g2.activate(FormId::PLAYER_REF, trader2, ActMode::Block).unwrap();
    assert_eq!(res2["result"], "script", "expression-only lambda => 1 must allow");
    assert_eq!(num(&mut g2, "owned.block"), 1.0);
    no_errors(&g);
    no_errors(&g2);
}

#[test]
fn script_activate_does_not_emit_regular_preactivation() {
    let f = fixture(true);
    let mut g = game(&f);
    query(&mut g,"SetEventHandlerAlt \"ShowOff:OnPreActivate\" (Begin Function {ref actor, int allowed}\nSetModLocalData \"owned.pre_calls\" 1\nSetFunctionValue 0\nEnd)\nlet ownRef := GetModLocalData \"owned.trader\"\nownRef.Activate player 1");
    assert_eq!(num(&mut g, "owned.pre_calls"), 0.0);
    assert_eq!(num(&mut g, "owned.block"), 1.0);
    no_errors(&g);
}

#[test]
fn invalid_callback_preserves_diagnostics_without_turning_error_into_a_veto() {
    let f = fixture(false);
    let mut g = game(&f);
    query(&mut g, "SetEventHandlerAlt \"ShowOff:OnPreActivate\" 0");
    let trader = target(&mut g, "owned.trader");
    assert!(g.default_activation_by_hand(trader).unwrap().is_some());
    assert!(g.events.iter().any(|e| matches!(&e.kind, fnv_game::trace::EventKind::ScriptError { error, .. } if error.contains("function script"))), "{:?}", g.events);
    assert_eq!(
        query(&mut g, "SetFunctionValue (GetCurrentEventName)"),
        XVal::Str(String::new())
    );
}

#[test]
fn scripted_activate_dispatches_showoff_on_pre_scripted_activate_with_flag_and_veto() {
    for (run_block, cancels) in [(true, false), (true, true), (false, false), (false, true)] {
        let f = fixture(true);
        let mut g = game(&f);
        query(
            &mut g,
            &format!(
                "SetEventHandlerAlt \"ShowOff:OnPreActivate\" (Begin Function {{ref actor, int allowed}}\n\
                 SetModLocalData \"owned.regular_calls\" ((GetModLocalData \"owned.regular_calls\") + 1)\n\
                 SetFunctionValue 0\n\
                 End)\n\
                 SetEventHandlerAlt \"ShowOff:OnPreScriptedActivate\" (Begin Function {{ref actor, int blockFlag, int allowed}}\n\
                 SetModLocalData \"owned.scripted_calls\" ((GetModLocalData \"owned.scripted_calls\") + 1)\n\
                 SetModLocalData \"owned.scripted_target\" ThisAlt\n\
                 SetModLocalData \"owned.scripted_actor\" actor\n\
                 SetModLocalData \"owned.scripted_flag\" blockFlag\n\
                 SetModLocalData \"owned.scripted_allowed\" allowed\n\
                 {}\n\
                 End) \"priority\"::2 1::player\n\
                 SetEventHandlerAlt \"ShowOff:OnPreScriptedActivate\" (Begin Function {{ref actor, int blockFlag, int allowed}}\n\
                 SetModLocalData \"owned.block_only_calls\" ((GetModLocalData \"owned.block_only_calls\") + 1)\n\
                 End) \"priority\"::1 2::1\n\
                 SetEventHandlerAlt \"ShowOff:OnPreScriptedActivate\" (Begin Function {{ref actor, int blockFlag, int allowed}}\n\
                 SetModLocalData \"owned.after_veto_saw_allowed\" allowed\n\
                 SetFunctionValue 1\n\
                 End) \"priority\"::1 3::0",
                if cancels { "SetFunctionValue 0" } else { "" }
            ),
        );
        let flag = if run_block { 1 } else { 0 };
        let trader = target(&mut g, "owned.trader");
        query(
            &mut g,
            &format!("let ownRef := GetModLocalData \"owned.trader\"\nownRef.Activate player {flag}"),
        );
        assert_eq!(num(&mut g, "owned.regular_calls"), 0.0, "scripted Activate must not fire OnPreActivate");
        assert_eq!(num(&mut g, "owned.scripted_calls"), 1.0);
        assert_eq!(target(&mut g, "owned.scripted_target"), trader);
        assert_eq!(target(&mut g, "owned.scripted_actor"), FormId::PLAYER_REF);
        assert_eq!(num(&mut g, "owned.scripted_flag"), f64::from(flag));
        assert_eq!(num(&mut g, "owned.scripted_allowed"), 1.0);
        assert_eq!(num(&mut g, "owned.block_only_calls"), f64::from(flag));
        if cancels {
            assert_eq!(num(&mut g, "owned.block"), 0.0, "explicit 0 veto must cancel scripted Activate");
            assert_eq!(num(&mut g, "owned.after_veto_saw_allowed"), 0.0);
        } else {
            assert_eq!(
                num(&mut g, "owned.block"),
                if run_block { 1.0 } else { 0.0 },
                "void callback must not veto scripted Activate"
            );
        }
        no_errors(&g);
    }

    // Hand activation must not fire ShowOff:OnPreScriptedActivate.
    let f = fixture(true);
    let mut g = game(&f);
    query(
        &mut g,
        "SetEventHandlerAlt \"ShowOff:OnPreScriptedActivate\" (Begin Function {ref actor, int blockFlag, int allowed}\n\
         SetModLocalData \"owned.scripted_on_hand\" 1\n\
         SetFunctionValue 0\n\
         End)",
    );
    let trader = target(&mut g, "owned.trader");
    g.activate(FormId::PLAYER_REF, trader, ActMode::Hand).unwrap();
    assert_eq!(num(&mut g, "owned.scripted_on_hand"), 0.0);
    assert_eq!(num(&mut g, "owned.block"), 1.0);
    no_errors(&g);
}

#[test]
fn scripted_activate_runs_pre_scripted_before_on_activate_and_on_pre_destroyed() {
    // 1. Unvetoed scripted Activate runs ShowOff:OnPreScriptedActivate BEFORE OnActivate,
    // and OnActivate receives GetSelfAlt == 0.
    let f = fixture(true);
    let mut g = game(&f);
    query(
        &mut g,
        r#"SetEventHandlerAlt "ShowOff:OnPreScriptedActivate" (Begin Function {ref actor, int blockFlag, int allowed}
SetModLocalData "ord.seq" ((GetModLocalData "ord.seq") * 10 + 1)
End)
SetEventHandler "OnActivate" (Begin Function {ref obj, ref actor}
SetModLocalData "ord.seq" ((GetModLocalData "ord.seq") * 10 + 2)
if eval (GetSelfAlt == 0)
SetModLocalData "ord.self_zero" 1
endif
End)"#,
    );
    let trader = target(&mut g, "owned.trader");
    g.activate(FormId::PLAYER_REF, trader, ActMode::Block).unwrap();
    assert_eq!(num(&mut g, "ord.seq"), 12.0, "PreScriptedActivate (1) must run before OnActivate (2)");
    assert_eq!(num(&mut g, "ord.self_zero"), 1.0, "OnActivate must have GetSelfAlt == 0");
    no_errors(&g);

    // 2. Vetoing ShowOff:OnPreScriptedActivate suppresses OnActivate completely.
    let f2 = fixture(true);
    let mut g2 = game(&f2);
    query(
        &mut g2,
        r#"SetEventHandlerAlt "ShowOff:OnPreScriptedActivate" (Begin Function {ref actor, int blockFlag, int allowed}
SetModLocalData "ord.pre" 1
SetFunctionValue 0
End)
SetEventHandler "OnActivate" (Begin Function {ref obj, ref actor}
SetModLocalData "ord.act" 1
End)"#,
    );
    let trader2 = target(&mut g2, "owned.trader");
    let res = g2.activate(FormId::PLAYER_REF, trader2, ActMode::Block).unwrap();
    assert_eq!(res["result"], "prevented");
    assert_eq!(num(&mut g2, "ord.pre"), 1.0);
    assert_eq!(num(&mut g2, "ord.act"), 0.0, "veto must skip OnActivate");
    no_errors(&g2);

    // 3. A pre-destroyed door (DOOR with SetDestroyed 1) still dispatches
    // ShowOff:OnPreScriptedActivate on scripted Activate (ActMode::Default and ActMode::Block),
    // returning "destroyed" when unvetoed and "prevented" when vetoed.
    let mut bytes = tes4();
    bytes.extend(top(
        b"NPC_",
        &record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))]),
    ));
    bytes.extend(top(
        b"DOOR",
        &record(
            b"DOOR",
            0,
            0x910,
            &[sub(b"EDID", z("OwnedDoor")), sub(b"FULL", z("Test Door"))],
        ),
    ));
    let plugins = vec![Plugin::parse(&bytes, "OwnedPreDestroyed.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let path = root.join("script/functions.json");
    let json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&path).unwrap(), &json, &refs);
    let mut g3 = Game::new(
        &data,
        World::new(&forms, &refs, Default::default()),
        GameConfig {
            strict: true,
            ..Default::default()
        },
    );
    query(
        &mut g3,
        r#"SetModLocalData "dest.door" (player.PlaceAtMe OwnedDoor)
let ownRef := GetModLocalData "dest.door"
ownRef.SetDestroyed 1
SetEventHandlerAlt "ShowOff:OnPreScriptedActivate" (Begin Function {ref actor, int blockFlag, int allowed}
SetModLocalData "dest.calls" ((GetModLocalData "dest.calls") + 1)
if eval (GetModLocalData "dest.veto")
SetFunctionValue 0
endif
End) 0::ownRef"#,
    );
    let door = target(&mut g3, "dest.door");
    let r_def = g3.activate(FormId::PLAYER_REF, door, ActMode::Default).unwrap();
    assert_eq!(r_def["result"], "destroyed");
    assert_eq!(num(&mut g3, "dest.calls"), 1.0);
    let r_blk = g3.activate(FormId::PLAYER_REF, door, ActMode::Block).unwrap();
    assert_eq!(r_blk["result"], "destroyed");
    assert_eq!(num(&mut g3, "dest.calls"), 2.0);
    query(&mut g3, "SetModLocalData \"dest.veto\" 1");
    let r_veto = g3.activate(FormId::PLAYER_REF, door, ActMode::Default).unwrap();
    assert_eq!(r_veto["result"], "prevented");
    assert_eq!(num(&mut g3, "dest.calls"), 3.0);
    no_errors(&g3);
}

#[test]
fn inventory_item_preactivation_dispatches_base_and_alt_with_veto_and_script_equip_cancel() {
    let mut bytes = tes4();
    bytes.extend(top(
        b"NPC_",
        &record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))]),
    ));
    bytes.extend(top(
        b"ALCH",
        &record(
            b"ALCH",
            0,
            0x900,
            &[sub(b"EDID", z("OwnedAid")), sub(b"FULL", z("Test Tonic"))],
        ),
    ));
    bytes.extend(top(
        b"WEAP",
        &record(
            b"WEAP",
            0,
            0x901,
            &[
                sub(b"EDID", z("OwnedWeap")),
                sub(b"FULL", z("Test Pistol")),
                sub(b"DATA", vec![0; 15]),
            ],
        ),
    ));
    let plugins = vec![Plugin::parse(&bytes, "OwnedInvPre.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let path = root.join("script/functions.json");
    let json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(
        &lo,
        &schema,
        FunctionTable::load(&path).unwrap(),
        &json,
        &refs,
    );
    let mut g = Game::new(
        &data,
        World::new(&forms, &refs, Default::default()),
        GameConfig {
            strict: true,
            ..Default::default()
        },
    );
    let aid = FormId(0x900);
    let weap = FormId(0x901);
    g.world.add_item(FormId::PLAYER_REF, aid, 2).unwrap();
    g.world.add_item(FormId::PLAYER_REF, weap, 1).unwrap();

    // No handlers registered: fast path allows and allocates no temp_ref.
    assert!(g.pre_activate_inventory_item(FormId::PLAYER_REF, aid, 0, true));
    assert!(g.temp_item(FormId(fnv_game::game::TEMP_REF_BASE)).is_none());

    // Register both non-Alt (vetoes on aid) and Alt (observes sticky allowed=0 and filters 3::1 / 3::0).
    query(
        &mut g,
        r#"SetEventHandlerAlt "ShowOff:OnPreActivateInventoryItem" (Begin Function {ref itemBase, ref itemRef, int allowed, int key}
SetModLocalData "inv.base" itemBase
SetModLocalData "inv.refBase" (itemRef.GetBaseObject)
SetModLocalData "inv.allowed" allowed
SetModLocalData "inv.key" key
SetModLocalData "inv.self" ThisAlt
if itemBase == OwnedAid
SetFunctionValue 0
endif
End) 0::player
SetEventHandlerAlt "ShowOff:OnPreActivateInventoryItemAlt" (Begin Function {ref itemBase, ref itemRef, int allowed, int key, int eq}
SetModLocalData "inv.alt_only_allowed" ((GetModLocalData "inv.alt_only_allowed") + 1)
SetModLocalData "inv.alt_eq" eq
End) 0::player 3::1
SetEventHandlerAlt "ShowOff:OnPreActivateInventoryItemAlt" (Begin Function {ref itemBase, ref itemRef, int allowed, int key, int eq}
SetModLocalData "inv.alt_saw_veto" ((GetModLocalData "inv.alt_saw_veto") + 1)
SetModLocalData "inv.alt_key" key
SetModLocalData "inv.alt_eq_veto" eq
SetFunctionValue 1
End) 0::player 3::0"#,
    );

    // Aid: vetoed by non-Alt, Alt sees allowed=0 (3::1 skipped, 3::0 runs, sticky veto preserved).
    assert!(!g.pre_activate_inventory_item(FormId::PLAYER_REF, aid, 3, true));
    assert_eq!(target(&mut g, "inv.base"), aid);
    assert_eq!(target(&mut g, "inv.refBase"), aid);
    assert_eq!(target(&mut g, "inv.self"), FormId::PLAYER_REF);
    assert_eq!(num(&mut g, "inv.allowed"), 1.0);
    assert_eq!(num(&mut g, "inv.key"), 3.0);
    assert_eq!(num(&mut g, "inv.alt_only_allowed"), 0.0);
    assert_eq!(num(&mut g, "inv.alt_saw_veto"), 1.0);
    assert_eq!(num(&mut g, "inv.alt_key"), 3.0);
    assert_eq!(num(&mut g, "inv.alt_eq_veto"), 1.0);

    // Weapon unequip: allowed (equipping=false -> eq=0).
    assert!(g.pre_activate_inventory_item(FormId::PLAYER_REF, weap, 0, false));
    assert_eq!(num(&mut g, "inv.alt_only_allowed"), 1.0);
    assert_eq!(num(&mut g, "inv.alt_eq"), 0.0);

    // Scripted EquipItem and UseAidItem invoke ShowOff:OnPreActivateInventoryItemAlt and respect veto.
    query(
        &mut g,
        r#"SetEventHandlerAlt "ShowOff:OnPreActivateInventoryItemAlt" (Begin Function {ref itemBase, ref itemRef, int allowed, int key, int eq}
SetModLocalData "inv.script_veto" ((GetModLocalData "inv.script_veto") + 1)
SetFunctionValue 0
End) 0::player 1::OwnedAid"#,
    );
    query(&mut g, "player.EquipItem OwnedAid");
    assert_eq!(g.world.item_count(FormId::PLAYER_REF, aid).unwrap(), 2, "vetoed EquipItem must not consume aid");
    assert_eq!(num(&mut g, "inv.script_veto"), 1.0);
    query(&mut g, "player.UseAidItem OwnedAid");
    assert_eq!(g.world.item_count(FormId::PLAYER_REF, aid).unwrap(), 2, "vetoed UseAidItem must not consume aid");
    assert_eq!(num(&mut g, "inv.script_veto"), 2.0);
    // Unvetoed weapon EquipItem succeeds.
    query(&mut g, "player.EquipItem OwnedWeap");
    assert_eq!(query(&mut g, "SetFunctionValue (player.GetEquipped OwnedWeap)"), XVal::Num(1.0));

    // Scripted UnequipItem dispatches ShowOff:OnPreActivateInventoryItemAlt (bEquipping=1) and respects veto.
    query(
        &mut g,
        r#"SetModLocalData "inv.un_veto_flag" 1
SetEventHandlerAlt "ShowOff:OnPreActivateInventoryItemAlt" (Begin Function {ref itemBase, ref itemRef, int allowed, int key, int eq}
SetModLocalData "inv.un_calls" ((GetModLocalData "inv.un_calls") + 1)
SetModLocalData "inv.un_beq" eq
if eval (GetModLocalData "inv.un_veto_flag")
SetFunctionValue 0
endif
End) 0::player 1::OwnedWeap"#,
    );
    query(&mut g, "player.UnequipItem OwnedWeap");
    assert_eq!(query(&mut g, "SetFunctionValue (player.GetEquipped OwnedWeap)"), XVal::Num(1.0), "vetoed UnequipItem must keep weapon equipped");
    assert_eq!(num(&mut g, "inv.un_calls"), 1.0);
    assert_eq!(num(&mut g, "inv.un_beq"), 1.0);
    query(&mut g, "SetModLocalData \"inv.un_veto_flag\" 0\nplayer.UnequipItem OwnedWeap");
    assert_eq!(query(&mut g, "SetFunctionValue (player.GetEquipped OwnedWeap)"), XVal::Num(0.0), "unvetoed UnequipItem succeeds");
    assert_eq!(num(&mut g, "inv.un_calls"), 2.0);
    no_errors(&g);
}

#[test]
fn pov_change_dispatches_showoff_on_pov_change_only_on_transitions() {
    let f = fixture(false);
    let mut g = game(&f);
    // No handlers registered: silent.
    g.set_first_person(false);
    g.set_first_person(true);

    query(
        &mut g,
        r#"SetEventHandler "ShowOff:OnPOVChange" (Begin Function {int iPOV}
SetModLocalData "pov.calls" ((GetModLocalData "pov.calls") + 1)
SetModLocalData "pov.last" iPOV
SetModLocalData "pov.getter" IsPC1stPerson
SetModLocalData "pov.self_is_player" (GetSelfAlt == PlayerREF)
End)
SetEventHandlerAlt "ShowOff:OnPOVChange" (Begin Function {int iPOV}
SetModLocalData "pov.player_only" ((GetModLocalData "pov.player_only") + 1)
End) 0::Player
SetEventHandlerAlt "ShowOff:OnPOVChange" (Begin Function {int iPOV}
SetModLocalData "pov.third_only" ((GetModLocalData "pov.third_only") + 1)
End) 1::0
SetEventHandlerAlt "ShowOff:OnPOVChange" (Begin Function {int iPOV}
SetModLocalData "pov.first_only" ((GetModLocalData "pov.first_only") + 1)
End) 1::1"#,
    );

    // Repeating current POV (first person) must not fire.
    g.set_first_person(true);
    assert_eq!(num(&mut g, "pov.calls"), 0.0);

    // First -> Third fires once with arg=0, calling ref Player (0::Player),
    // IsPC1stPerson == 1 inside callback and 0 after [observed 2026-11-24: w6t2p2_03].
    g.set_first_person(false);
    assert_eq!(num(&mut g, "pov.calls"), 1.0);
    assert_eq!(num(&mut g, "pov.last"), 0.0);
    assert_eq!(num(&mut g, "pov.getter"), 1.0);
    assert_eq!(num(&mut g, "pov.self_is_player"), 1.0);
    assert_eq!(num(&mut g, "pov.player_only"), 1.0);
    assert_eq!(query(&mut g, "SetFunctionValue IsPC1stPerson"), XVal::Num(0.0));
    assert_eq!(num(&mut g, "pov.third_only"), 1.0);
    assert_eq!(num(&mut g, "pov.first_only"), 0.0);

    // Repeating Third must not spam.
    for _ in 0..3 {
        g.set_first_person(false);
    }
    assert_eq!(num(&mut g, "pov.calls"), 1.0);

    // Third -> First fires once with arg=1, calling ref Player, and IsPC1stPerson == 1 inside and after.
    g.set_first_person(true);
    assert_eq!(num(&mut g, "pov.calls"), 2.0);
    assert_eq!(num(&mut g, "pov.last"), 1.0);
    assert_eq!(num(&mut g, "pov.getter"), 1.0);
    assert_eq!(num(&mut g, "pov.self_is_player"), 1.0);
    assert_eq!(num(&mut g, "pov.player_only"), 2.0);
    assert_eq!(query(&mut g, "SetFunctionValue IsPC1stPerson"), XVal::Num(1.0));
    assert_eq!(num(&mut g, "pov.third_only"), 1.0);
    assert_eq!(num(&mut g, "pov.first_only"), 1.0);
    no_errors(&g);
}
