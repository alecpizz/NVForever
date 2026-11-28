//! Actual regular-activation entry and existing trade-message consumers.
use super::*;
use crate::script::{ByHand, ScriptState};
use crate::script_draw::tests::{play_over, record, sub, top};
use bevy::ecs::message::Messages;
use fnv_script::vm::XVal;

// The same enabled Object SCHR fields as the core fixture builder, using the
// accepted ScriptHeader schema rather than an all-zero (disabled) header.
fn enabled_script_header() -> Vec<u8> {
    [0u32.to_le_bytes().to_vec(), 0u32.to_le_bytes().to_vec(),
     0u32.to_le_bytes().to_vec(), 0u32.to_le_bytes().to_vec(),
     0u16.to_le_bytes().to_vec(), 1u16.to_le_bytes().to_vec()].concat()
}

fn fixture(scripted: bool, teammate: bool) -> App {
    let text = |sig: &[u8; 4], s: &str| sub(sig, [s.as_bytes(), &[0]].concat());
    let mut trader = vec![text(b"EDID", "OwnedMenuTrader")];
    if scripted {
        trader.push(sub(b"SCRI", 0x820u32.to_le_bytes().to_vec()));
    }
    let source="scn OwnedMenuActivation\nstring_var ownedSource\nBegin OnActivate\nSetModLocalData \"owned.block\" ((GetModLocalData \"owned.block\") + 1)\nEnd\n";
    let play = play_over(&[
        top(
            b"NPC_",
            &[
                record(b"NPC_", 0, 7, &[text(b"EDID", "Player")]),
                record(b"NPC_", 0, 0x810, &trader),
                record(b"NPC_", 0, 0x811, &[text(b"EDID", "OwnedMenuCompanion")]),
                record(b"NPC_", 0, 0x812, &[text(b"EDID", "OwnedMenuOther")]),
            ]
            .concat(),
        ),
        top(
            b"SCPT",
            &record(
                b"SCPT",
                0,
                0x820,
                &[
                    text(b"EDID", "OwnedMenuActivation"),
                    sub(b"SCHR", enabled_script_header()),
                    sub(b"SCDA", vec![]),
                    sub(b"SCTX", source.as_bytes().to_vec()),
                ],
            ),
        ),
    ]);
    let mut scripts = ScriptState::over_plugins(play.lo.plugins);
    let game = &mut scripts.rt.as_mut().unwrap().game;
    game.cfg.strict = true;
    let unit = game.data.script(FormId(0x820u32)).unwrap_or_else(|| {
        let (units, issues) = fnv_script::unit::read_load_order(play.lo, play.schema);
        let headers: Vec<_> = units.iter().map(|u| (&u.owner, &u.header, u.has_scda, u.source.is_some())).collect();
        panic!("missing enabled fixture SCPT 0x{:08X}; decoded headers={headers:?}; read issues={issues:?}; source selections={:?}", 0x820u32, game.data.source_units);
    });
    assert!(unit.unit.header.flags & 1 != 0, "fixture SCPT 0x{:08X} disabled: {:?}", 0x820u32, unit.unit.header);
    assert!(unit.unit.source.is_some());
    assert!(game
        .data
        .source_units
        .iter()
        .any(|s| s.key == unit.key() && s.error.is_none()));
    let sim = SimState::new(play.lo, play.schema, play.index);
    let mut app = App::new();
    app.insert_resource(play)
        .insert_resource(sim)
        .insert_resource(scripts)
        .insert_resource(State::new(PlayState::Playing));
    app.add_message::<OpenBarter>()
        .add_message::<OpenContainer>()
        .add_systems(
            Update,
            (scripts_open_barter, scripts_open_teammate_containers),
        );
    query(&mut app,"SetModLocalData \"owned.trader\" (player.PlaceAtMe OwnedMenuTrader)\nSetModLocalData \"owned.companion\" (player.PlaceAtMe OwnedMenuCompanion)\nSetModLocalData \"owned.other\" (player.PlaceAtMe OwnedMenuOther)\nAssignKeyword (EditorIDToFormID \"OwnedMenuTrader\") \"OwnedTrader\"\nAssignKeyword (EditorIDToFormID \"OwnedMenuCompanion\") \"OwnedCompanion\"");
    if teammate {
        query(
            &mut app,
            "let ownRef := GetModLocalData \"owned.companion\"\nownRef.SetPlayerTeammate 1",
        );
    }
    query(
        &mut app,
        r#"let recipients := GetKeywordForms "OwnedTrader"
ar_Cat recipients (GetKeywordForms "OwnedCompanion")
SetEventHandlerAlt "ShowOff:OnPreActivate" (Begin Function {ref activator, int allowed}
ref recipient = ThisAlt
if (!(IsKeyPressed 42 2) || (HasKeyword recipient "OwnedCompanion" && !recipient.GetPlayerTeammate))
return
endif
SetModLocalData "owned.pre_receiver" recipient
SetModLocalData "owned.pre_actor" activator
SetFunctionValue 0
DispatchEventAlt "OwnedMenuTrade" recipient
End) 0::recipients 1::player
SetEventHandlerAlt "OwnedMenuTrade" (Begin Function {ref recipient}
if HasKeyword recipient "OwnedCompanion"
recipient.OpenTeammateContainer
else
recipient.ShowBarterMenu 35
endif
End) 1::recipients"#,
    );
    app
}
fn query(app: &mut App, body: &str) -> XVal {
    app.world_mut()
        .resource_scope(|world, mut sim: Mut<SimState>| {
            let mut scripts = world.resource_mut::<ScriptState>();
            crate::script::with_world(scripts.rt.as_mut().unwrap(), &mut sim, |rt| {
                rt.game
                    .run_query(
                        &format!(
                            "ref ownRef\narray_var recipients\nBegin Function {{}}\n{body}\nEnd"
                        ),
                        default(),
                    )
                    .unwrap()
            })
        })
}
fn target(app: &mut App, key: &str) -> FormId {
    let XVal::Ref(id) = query(
        app,
        &format!("SetFunctionValue (GetModLocalData \"{key}\")"),
    ) else {
        panic!("reference")
    };
    assert_ne!(id, 0);
    FormId(id)
}
fn activate(app: &mut App, reference: FormId) -> ByHand {
    app.world_mut()
        .resource_scope(|world, mut sim: Mut<SimState>| {
            let mut scripts = world.resource_mut::<ScriptState>();
            crate::script::activate_by_hand(Some(&mut scripts), &mut sim, reference.0).unwrap()
        })
}
fn held(app: &mut App) {
    app.world_mut()
        .resource_mut::<ScriptState>()
        .rt
        .as_mut()
        .unwrap()
        .game
        .input
        .held
        .insert(42);
}
fn messages(app: &mut App) -> (Vec<OpenBarter>, Vec<OpenContainer>) {
    let barter = app
        .world_mut()
        .resource_mut::<Messages<OpenBarter>>()
        .drain()
        .collect();
    let containers = app
        .world_mut()
        .resource_mut::<Messages<OpenContainer>>()
        .drain()
        .collect();
    (barter, containers)
}
fn no_errors(app: &App) {
    let game = &app
        .world()
        .resource::<ScriptState>()
        .rt
        .as_ref()
        .unwrap()
        .game;
    assert!(
        !game
            .events
            .iter()
            .any(|e| matches!(e.kind, fnv_game::trace::EventKind::ScriptError { .. })),
        "{:?}",
        game.events
    );
}

#[test]
fn keyword_filtered_regular_activation_reaches_real_barter_and_container_messages() {
    for companion in [false, true] {
        let mut app = fixture(false, true);
        held(&mut app);
        let selected = target(
            &mut app,
            if companion {
                "owned.companion"
            } else {
                "owned.trader"
            },
        );
        let result = activate(&mut app, selected);
        assert!(
            matches!(result, ByHand::Refused),
            "source preactivation must prevent the normal talk/default path"
        );
        assert_eq!(
            query(
                &mut app,
                "SetFunctionValue (GetModLocalData \"owned.pre_receiver\")"
            ),
            XVal::Ref(selected.0)
        );
        assert_eq!(
            query(
                &mut app,
                "SetFunctionValue (GetModLocalData \"owned.pre_actor\")"
            ),
            XVal::Ref(FormId::PLAYER_REF.0)
        );
        app.update();
        let (barter, containers) = messages(&mut app);
        if companion {
            assert!(barter.is_empty());
            assert_eq!(containers.len(), 1);
            assert_eq!(containers[0].container, selected.0);
        } else {
            assert!(containers.is_empty());
            assert_eq!(barter.len(), 1);
            assert_eq!(barter[0].merchant, selected.0);
            assert_eq!(barter[0].discount, 35);
        }
        app.update();
        let (b, c) = messages(&mut app);
        assert!(
            b.is_empty() && c.is_empty(),
            "message consumer must not repeat the trade"
        );
        no_errors(&app);
    }
}

#[test]
fn ordinary_unlisted_and_non_teammate_activation_keep_neighbor_paths() {
    for case in 0..3 {
        let mut app = fixture(false, false);
        if case != 0 {
            held(&mut app);
        }
        let selected = target(
            &mut app,
            match case {
                0 => "owned.trader",
                1 => "owned.other",
                _ => "owned.companion",
            },
        );
        assert!(
            !matches!(activate(&mut app, selected), ByHand::Refused),
            "void/missing recipient callback must not become numeric0 veto"
        );
        app.update();
        let (b, c) = messages(&mut app);
        assert!(b.is_empty() && c.is_empty());
        no_errors(&app);
    }
}

#[test]
fn scripted_target_is_vetoed_by_hand_but_script_activate_keeps_its_bypass() {
    let mut app = fixture(true, true);
    held(&mut app);
    let selected = target(&mut app, "owned.trader");
    assert!(matches!(activate(&mut app, selected), ByHand::Refused));
    assert_eq!(
        query(
            &mut app,
            "SetFunctionValue (GetModLocalData \"owned.block\")"
        ),
        XVal::Num(0.0)
    );
    query(
        &mut app,
        "let ownRef := GetModLocalData \"owned.trader\"\nownRef.Activate player 1",
    );
    assert_eq!(
        query(
            &mut app,
            "SetFunctionValue (GetModLocalData \"owned.block\")"
        ),
        XVal::Num(1.0)
    );
    no_errors(&app);
}

#[test]
fn inventory_menu_apply_input_dispatches_and_cancels_preactivate_inventory_item() {
    let text = |sig: &[u8; 4], s: &str| sub(sig, [s.as_bytes(), &[0]].concat());
    let aid_id = 0x910u32;
    let play = play_over(&[
        top(b"NPC_", &record(b"NPC_", 0, 7, &[text(b"EDID", "Player")])),
        top(
            b"ALCH",
            &record(
                b"ALCH",
                0,
                aid_id,
                &[text(b"EDID", "OwnedMenuAid"), text(b"FULL", "Menu Tonic")],
            ),
        ),
    ]);
    let mut scripts = ScriptState::over_plugins(play.lo.plugins);
    scripts.rt.as_mut().unwrap().game.cfg.strict = true;
    let mut sim = SimState::new(play.lo, play.schema, play.index);
    sim.world.add_item(FormId::PLAYER_REF, FormId(aid_id), 2).unwrap();
    let model = MenuModel::open_inventory(&mut sim.world).unwrap();

    let mut app = App::new();
    app.insert_resource(play)
        .insert_resource(sim)
        .insert_resource(scripts)
        .insert_resource(ActiveMenu(Some(model)))
        .insert_resource(PromptKeys::default())
        .insert_resource(State::new(PlayState::Menu))
        .insert_resource(NextState::<PlayState>::Unchanged)
        .add_message::<MenuInput>()
        .add_message::<RefChanged>()
        .add_message::<ItemUsed>()
        .add_message::<crate::ui_sounds::UiSound>()
        .add_message::<crate::crime::ItemsTaken>()
        .add_message::<crate::pickpocket::Pickpocketed>()
        .add_systems(Update, apply_input);

    // Register cancelling handler for ShowOff:OnPreActivateInventoryItemAlt.
    query(
        &mut app,
        r#"SetEventHandlerAlt "ShowOff:OnPreActivateInventoryItemAlt" (Begin Function {ref itemBase, ref itemRef, int allowed, int key, int eq}
SetModLocalData "menu.inv_calls" ((GetModLocalData "menu.inv_calls") + 1)
SetModLocalData "menu.inv_item" itemBase
if GetModLocalData "menu.allow" == 0
SetFunctionValue 0
endif
End) 0::player 1::OwnedMenuAid"#,
    );

    // Command::One (Enter/Space) vetoed -> item NOT consumed, no ItemUsed emitted.
    app.world_mut().write_message(MenuInput(Command::One));
    app.update();
    assert_eq!(query(&mut app, "SetFunctionValue (GetModLocalData \"menu.inv_calls\")"), XVal::Num(1.0));
    assert_eq!(query(&mut app, "SetFunctionValue (GetModLocalData \"menu.inv_item\")"), XVal::Ref(aid_id));
    assert_eq!(app.world_mut().resource_mut::<SimState>().world.item_count(FormId::PLAYER_REF, FormId(aid_id)).unwrap(), 2);
    assert!(app.world_mut().resource_mut::<Messages<ItemUsed>>().drain().next().is_none());

    // Clicking the already-selected row (Command::Select) also fires and respects veto.
    app.world_mut().write_message(MenuInput(Command::Select(Side::Player, 0)));
    app.update();
    assert_eq!(query(&mut app, "SetFunctionValue (GetModLocalData \"menu.inv_calls\")"), XVal::Num(2.0));
    assert_eq!(app.world_mut().resource_mut::<SimState>().world.item_count(FormId::PLAYER_REF, FormId(aid_id)).unwrap(), 2);
    assert!(app.world_mut().resource_mut::<Messages<ItemUsed>>().drain().next().is_none());

    // Allowing proceeds to vanilla consumption and emits ItemUsed.
    query(&mut app, "SetModLocalData \"menu.allow\" 1");
    app.world_mut().write_message(MenuInput(Command::One));
    app.update();
    assert_eq!(query(&mut app, "SetFunctionValue (GetModLocalData \"menu.inv_calls\")"), XVal::Num(3.0));
    assert_eq!(app.world_mut().resource_mut::<SimState>().world.item_count(FormId::PLAYER_REF, FormId(aid_id)).unwrap(), 1);
    let used: Vec<_> = app.world_mut().resource_mut::<Messages<ItemUsed>>().drain().collect();
    assert_eq!(used.len(), 1);
    assert_eq!(used[0].item, aid_id);
    no_errors(&app);
}

#[test]
fn control_tell_scripts_dispatches_showoff_on_pov_change_without_frame_spam() {
    use crate::control::{tell_scripts, PlayerControl, Pov};
    let text = |sig: &[u8; 4], s: &str| sub(sig, [s.as_bytes(), &[0]].concat());
    let play = play_over(&[top(b"NPC_", &record(b"NPC_", 0, 7, &[text(b"EDID", "Player")]))]);
    let mut scripts = ScriptState::over_plugins(play.lo.plugins);
    scripts.rt.as_mut().unwrap().game.cfg.strict = true;
    let sim = SimState::new(play.lo, play.schema, play.index);

    let mut app = App::new();
    app.insert_resource(play)
        .insert_resource(sim)
        .insert_resource(scripts)
        .insert_resource(PlayerControl::default())
        .add_systems(Update, tell_scripts);

    query(
        &mut app,
        r#"SetEventHandler "ShowOff:OnPOVChange" (Begin Function {int iPOV}
SetModLocalData "ctrl.pov_calls" ((GetModLocalData "ctrl.pov_calls") + 1)
SetModLocalData "ctrl.pov_val" iPOV
SetModLocalData "ctrl.pov_self_is_p" (GetSelfAlt == Player)
SetModLocalData "ctrl.pov_in_cb" (IsPC1stPerson)
End)"#,
    );

    // Steady first-person frames never fire.
    app.update();
    app.update();
    assert_eq!(query(&mut app, "SetFunctionValue (GetModLocalData \"ctrl.pov_calls\")"), XVal::Num(0.0));

    // Switch to third person -> fires once with 0; inside callback GetSelfAlt == Player and IsPC1stPerson == 1.
    app.world_mut().resource_mut::<PlayerControl>().pov = Pov::Third;
    app.update();
    app.update();
    assert_eq!(query(&mut app, "SetFunctionValue (GetModLocalData \"ctrl.pov_calls\")"), XVal::Num(1.0));
    assert_eq!(query(&mut app, "SetFunctionValue (GetModLocalData \"ctrl.pov_val\")"), XVal::Num(0.0));
    assert_eq!(query(&mut app, "SetFunctionValue (GetModLocalData \"ctrl.pov_self_is_p\")"), XVal::Num(1.0));
    assert_eq!(query(&mut app, "SetFunctionValue (GetModLocalData \"ctrl.pov_in_cb\")"), XVal::Num(1.0));
    assert_eq!(query(&mut app, "SetFunctionValue (IsPC1stPerson)"), XVal::Num(0.0));

    // Scripted ToggleFirstPerson 1 -> switches back to first person and fires once with 1;
    // inside callback GetSelfAlt == Player and IsPC1stPerson == 1.
    query(&mut app, "ToggleFirstPerson 1");
    app.update();
    app.update();
    assert_eq!(app.world().resource::<PlayerControl>().pov, Pov::First);
    assert_eq!(query(&mut app, "SetFunctionValue (GetModLocalData \"ctrl.pov_calls\")"), XVal::Num(2.0));
    assert_eq!(query(&mut app, "SetFunctionValue (GetModLocalData \"ctrl.pov_val\")"), XVal::Num(1.0));
    assert_eq!(query(&mut app, "SetFunctionValue (GetModLocalData \"ctrl.pov_self_is_p\")"), XVal::Num(1.0));
    assert_eq!(query(&mut app, "SetFunctionValue (GetModLocalData \"ctrl.pov_in_cb\")"), XVal::Num(1.0));
    assert_eq!(query(&mut app, "SetFunctionValue (IsPC1stPerson)"), XVal::Num(1.0));
    no_errors(&app);
}
