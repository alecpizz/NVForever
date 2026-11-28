//! Integration tests for ShowOff objective & weapon-unholster events
//! (`ShowOff:OnDisplayObjective`, `ShowOff:OnCompleteObjective`,
//! `ShowOff:OnWeaponUnholster`) and built-in xNVSE image-space modifier events
//! (`OnApplyIMOD`, `OnRemoveIMOD`).

use crate::common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{
    anim_state::{AnimAction, AnimState},
    game::{Game, GameConfig},
    nvse_hud::WEAPON_FRAMES,
    trace::EventKind,
    GameData,
};
use fnv_script::{vm::XVal, FunctionTable};
use fnv_sim::{FormId, FormStore, RefTable, World};

const FISTS: u32 = 0x1F4;
const NPC_GUARD: u32 = 0x810;
const CREA_GHOUL: u32 = 0x811;
const WEAP_PISTOL: u32 = 0x820;
const WEAP_RIFLE: u32 = 0x821;
const QUEST_A: u32 = 0x830;
const QUEST_B: u32 = 0x831;
const IMAD_FADE: u32 = 0x840;
const IMAD_HOLD: u32 = 0x841;
const FLST_IMADS: u32 = 0x850;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn quest_record(id: u32, edid: &str, objectives: &[i32]) -> Vec<u8> {
    let mut subs = vec![
        sub(b"EDID", z(edid)),
        sub(b"DATA", [vec![0u8, 0, 0, 0], 5.0f32.to_le_bytes().to_vec()].concat()),
    ];
    for &idx in objectives {
        subs.push(sub(b"QOBJ", idx.to_le_bytes().to_vec()));
        subs.push(sub(b"NNAM", z(&format!("Objective {idx}"))));
    }
    record(b"QUST", 0, id, &subs)
}

fn imad_record(id: u32, edid: &str, animatable: u32, duration: f32) -> Vec<u8> {
    let dnam = [u32s(&[animatable]), duration.to_le_bytes().to_vec(), vec![0; 236]].concat();
    record(b"IMAD", 0, id, &[sub(b"EDID", z(edid)), sub(b"DNAM", dnam)])
}

fn fixture(with_fists: bool) -> Fixture {
    let mut bytes = tes4();
    bytes.extend(top(
        b"NPC_",
        &[
            record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))]),
            record(b"NPC_", 0, NPC_GUARD, &[sub(b"EDID", z("OwnedGuardNpc"))]),
        ]
        .concat(),
    ));
    let crea_data = [vec![4u8, 0, 0, 0], vec![0; 6], vec![0; 7]].concat();
    bytes.extend(top(
        b"CREA",
        &record(b"CREA", 0, CREA_GHOUL, &[sub(b"EDID", z("OwnedGhoulCrea")), sub(b"DATA", crea_data)]),
    ));
    let mut weaps = vec![
        record(b"WEAP", 0, WEAP_PISTOL, &[sub(b"EDID", z("OwnedPistol")), sub(b"DATA", vec![0; 15])]),
        record(b"WEAP", 0, WEAP_RIFLE, &[sub(b"EDID", z("OwnedRifle")), sub(b"DATA", vec![0; 15])]),
    ];
    if with_fists {
        weaps.push(record(b"WEAP", 0, FISTS, &[sub(b"EDID", z("Fists")), sub(b"DATA", vec![0; 15])]));
    }
    bytes.extend(top(b"WEAP", &weaps.concat()));
    bytes.extend(top(
        b"QUST",
        &[quest_record(QUEST_A, "OwnedQuestA", &[50, 30, 5, 10]), quest_record(QUEST_B, "OwnedQuestB", &[10, 20])].concat(),
    ));
    bytes.extend(top(
        b"IMAD",
        &[imad_record(IMAD_FADE, "OwnedFadeImad", 1, 0.5), imad_record(IMAD_HOLD, "OwnedHoldImad", 0, 1.0)].concat(),
    ));
    bytes.extend(top(
        b"FLST",
        &record(
            b"FLST",
            0,
            FLST_IMADS,
            &[sub(b"EDID", z("OwnedImadList")), sub(b"LNAM", IMAD_FADE.to_le_bytes().to_vec())],
        ),
    ));
    let plugins = vec![Plugin::parse(&bytes, "OwnedShowOffEvents.esm".into()).unwrap()];
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
    Game::new(
        &f.data,
        World::new(&f.forms, &f.refs, Default::default()),
        GameConfig { strict: true, ..Default::default() },
    )
}

fn query(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("ref ownRef\narray_var aTmp\nBegin Function {{}}\n{body}\nEnd"), Default::default()).unwrap()
}

fn num(g: &mut Game, key: &str) -> f64 {
    let XVal::Num(n) = query(g, &format!("SetFunctionValue (GetModLocalData \"{key}\")")) else {
        panic!("expected number for {key}")
    };
    n
}

fn form(g: &mut Game, key: &str) -> FormId {
    let XVal::Ref(id) = query(g, &format!("SetFunctionValue (GetModLocalData \"{key}\")")) else {
        panic!("expected ref for {key}")
    };
    FormId(id)
}

fn text(g: &mut Game, key: &str) -> String {
    let XVal::Str(s) = query(g, &format!("SetFunctionValue (GetModLocalData \"{key}\")")) else {
        panic!("expected string for {key}")
    };
    s
}

fn no_errors(g: &Game) {
    assert!(
        !g.events.iter().any(|e| matches!(e.kind, EventKind::ScriptError { .. })),
        "{:?}",
        g.events
    );
}

#[test]
fn objective_events_dispatch_pre_update_skip_hidden_and_follow_qobj_order() {
    let f = fixture(true);
    // Vanilla-unchanged control: no handlers registered.
    {
        let mut g = game(&f);
        query(&mut g, "SetObjectiveDisplayed OwnedQuestA 10 1\nSetObjectiveCompleted OwnedQuestA 10 1\nCompleteAllObjectives OwnedQuestA");
        assert_eq!(query(&mut g, "SetFunctionValue (ar_Size (GetEventHandlers \"\"))"), XVal::Num(0.0));
        assert_eq!(query(&mut g, "SetFunctionValue (GetObjectiveDisplayed OwnedQuestA 10)"), XVal::Num(1.0));
        assert_eq!(query(&mut g, "SetFunctionValue (GetObjectiveCompleted OwnedQuestA 30)"), XVal::Num(1.0));
        no_errors(&g);
    }

    let mut g = game(&f);
    query(
        &mut g,
        r#"SetEventHandler "ShowOff:OnDisplayObjective" (Begin Function {ref rQuest, int iObj}
SetModLocalData "disp.calls" ((GetModLocalData "disp.calls") + 1)
SetModLocalData "disp.quest" rQuest
SetModLocalData "disp.obj" iObj
SetModLocalData "disp.getter" (GetObjectiveDisplayed rQuest iObj)
SetModLocalData "disp.ev" (GetCurrentEventName)
End)
SetEventHandler "ShowOff:OnDisplayObjective" (Begin Function {ref rQuest, int iObj}
SetModLocalData "disp.a10" ((GetModLocalData "disp.a10") + 1)
End) "first"::OwnedQuestA "second"::10
SetEventHandlerAlt "ShowOff:OnDisplayObjective" (Begin Function {ref rQuest, int iObj}
SetModLocalData "disp.a50" ((GetModLocalData "disp.a50") + 1)
End) 1::OwnedQuestA 2::50
SetEventHandler "ShowOff:OnCompleteObjective" (Begin Function {ref rQuest, int iObj}
int iN
let iN := (GetModLocalData "comp.calls") + 1
SetModLocalData "comp.calls" iN
SetModLocalData "comp.quest" rQuest
SetModLocalData "comp.obj" iObj
SetModLocalData "comp.seq" ((GetModLocalData "comp.seq") * 100 + iObj)
SetModLocalData "comp.getter" (GetObjectiveCompleted rQuest iObj)
SetModLocalData "comp.ev" (GetCurrentEventName)
End)
SetEventHandler "ShowOff:OnCompleteObjective" (Begin Function {ref rQuest, int iObj}
SetModLocalData "comp.a10" ((GetModLocalData "comp.a10") + 1)
End) "first"::OwnedQuestA "second"::10
SetEventHandlerAlt "ShowOff:OnCompleteObjective" (Begin Function {ref rQuest, int iObj}
SetModLocalData "comp.a50" ((GetModLocalData "comp.a50") + 1)
End) 1::OwnedQuestA 2::50"#,
    );

    // Display objective 10: fires OnDisplayObjective PRE-update (GetObjectiveDisplayed == 0 inside callback),
    // then updates getter to 1, matching first/second filters.
    query(&mut g, "SetObjectiveDisplayed OwnedQuestA 10 1");
    assert_eq!(num(&mut g, "disp.calls"), 1.0);
    assert_eq!(form(&mut g, "disp.quest"), FormId(QUEST_A));
    assert_eq!(num(&mut g, "disp.obj"), 10.0);
    assert_eq!(num(&mut g, "disp.getter"), 0.0, "OnDisplayObjective must fire PRE-update");
    assert_eq!(query(&mut g, "SetFunctionValue (GetObjectiveDisplayed OwnedQuestA 10)"), XVal::Num(1.0));
    assert_eq!(text(&mut g, "disp.ev"), "ShowOff:OnDisplayObjective");
    assert_eq!(num(&mut g, "disp.a10"), 1.0);
    assert_eq!(num(&mut g, "disp.a50"), 0.0);
    assert_eq!(num(&mut g, "comp.calls"), 0.0);

    // Repeating display on an already-displayed objective must not fire again.
    query(&mut g, "SetObjectiveDisplayed OwnedQuestA 10 1");
    assert_eq!(num(&mut g, "disp.calls"), 1.0);

    // Completing displayed objective 10 fires OnCompleteObjective PRE-update (GetObjectiveCompleted == 0 inside callback).
    query(&mut g, "SetObjectiveCompleted OwnedQuestA 10 1");
    assert_eq!(num(&mut g, "comp.calls"), 1.0);
    assert_eq!(form(&mut g, "comp.quest"), FormId(QUEST_A));
    assert_eq!(num(&mut g, "comp.obj"), 10.0);
    assert_eq!(num(&mut g, "comp.getter"), 0.0, "OnCompleteObjective must fire PRE-update");
    assert_eq!(query(&mut g, "SetFunctionValue (GetObjectiveCompleted OwnedQuestA 10)"), XVal::Num(1.0));
    assert_eq!(text(&mut g, "comp.ev"), "ShowOff:OnCompleteObjective");
    assert_eq!(num(&mut g, "comp.a10"), 1.0);
    assert_eq!(num(&mut g, "comp.a50"), 0.0);

    // Repeating completion or un-completing must not fire OnCompleteObjective.
    query(&mut g, "SetObjectiveCompleted OwnedQuestA 10 1\nSetObjectiveCompleted OwnedQuestA 10 0");
    assert_eq!(num(&mut g, "comp.calls"), 1.0);

    // Hiding an objective must not fire OnDisplayObjective.
    query(&mut g, "SetObjectiveDisplayed OwnedQuestA 10 0");
    assert_eq!(num(&mut g, "disp.calls"), 1.0);

    // Completing a HIDDEN (undisplayed) objective (30 or hidden 10) must NOT fire OnCompleteObjective,
    // while still updating GetObjectiveCompleted to 1.
    query(&mut g, "SetObjectiveCompleted OwnedQuestA 10 1\nSetObjectiveCompleted OwnedQuestA 30 1");
    assert_eq!(num(&mut g, "comp.calls"), 1.0, "hidden objective completion must not fire OnCompleteObjective");
    assert_eq!(query(&mut g, "SetFunctionValue (GetObjectiveCompleted OwnedQuestA 10)"), XVal::Num(1.0));
    assert_eq!(query(&mut g, "SetFunctionValue (GetObjectiveCompleted OwnedQuestA 30)"), XVal::Num(1.0));

    // Re-displaying hidden, completed objective 10 clears completed and fires OnDisplayObjective PRE-update only.
    query(&mut g, "SetObjectiveDisplayed OwnedQuestA 10 1");
    assert_eq!(num(&mut g, "disp.calls"), 2.0);
    assert_eq!(num(&mut g, "disp.getter"), 0.0);
    assert_eq!(num(&mut g, "comp.calls"), 1.0);
    assert_eq!(query(&mut g, "SetFunctionValue (GetObjectiveCompleted OwnedQuestA 10)"), XVal::Num(0.0));

    // Undefined objective index must not fire either event.
    query(&mut g, "SetObjectiveDisplayed OwnedQuestA 999 1\nSetObjectiveCompleted OwnedQuestA 999 1");
    assert_eq!(num(&mut g, "disp.calls"), 2.0);
    assert_eq!(num(&mut g, "comp.calls"), 1.0);

    // Completed quest refuses SetObjectiveDisplayed/SetObjectiveCompleted (#2958) and CompleteAllObjectives does not fire.
    query(&mut g, "SetObjectiveDisplayed OwnedQuestB 10 1\nCompleteQuest OwnedQuestB\nSetObjectiveDisplayed OwnedQuestB 20 1\nSetObjectiveCompleted OwnedQuestB 10 1\nCompleteAllObjectives OwnedQuestB");
    assert_eq!(num(&mut g, "disp.calls"), 3.0, "only initial OwnedQuestB 10 display before CompleteQuest fired");
    assert_eq!(num(&mut g, "comp.calls"), 1.0, "completed quest must not fire OnCompleteObjective");

    // CompleteAllObjectives on OwnedQuestA (QOBJ record order: [50, 30, 5, 10]):
    // Mark 10 completed, hide 30 (and reset completed=0), display 50 and 5.
    // CompleteAllObjectives must fire ONLY for displayed incomplete objectives (50 and 5),
    // skipping hidden 30 and already-completed 10, in QOBJ record order: 50 then 5 (5005, NOT 550!).
    query(
        &mut g,
        "SetObjectiveCompleted OwnedQuestA 10 1\n\
         SetObjectiveCompleted OwnedQuestA 30 0\n\
         SetObjectiveDisplayed OwnedQuestA 50 1\n\
         SetObjectiveDisplayed OwnedQuestA 5 1\n\
         SetModLocalData \"comp.seq\" 0\n\
         CompleteAllObjectives OwnedQuestA",
    );
    assert_eq!(num(&mut g, "comp.calls"), 4.0, "10 + 50 + 5 completed while displayed");
    assert_eq!(num(&mut g, "comp.seq"), 5005.0, "CompleteAllObjectives fires in QOBJ record order: 50 then 5");
    assert_eq!(num(&mut g, "comp.obj"), 5.0);
    assert_eq!(num(&mut g, "comp.getter"), 0.0, "PRE-update inside CompleteAllObjectives callbacks");
    assert_eq!(num(&mut g, "comp.a50"), 1.0);
    assert_eq!(query(&mut g, "SetFunctionValue (GetObjectiveCompleted OwnedQuestA 50)"), XVal::Num(1.0));
    assert_eq!(query(&mut g, "SetFunctionValue (GetObjectiveCompleted OwnedQuestA 30)"), XVal::Num(1.0));
    assert_eq!(query(&mut g, "SetFunctionValue (GetObjectiveCompleted OwnedQuestA 5)"), XVal::Num(1.0));
    no_errors(&g);
}

#[test]
fn imod_events_dispatch_pre_update_and_stack_on_reapply() {
    let f = fixture(true);
    // Vanilla-unchanged control + built-in KNOWN registration check.
    {
        let mut g = game(&f);
        assert_eq!(query(&mut g, "SetFunctionValue (ar_Size (GetEventHandlers \"OnApplyIMOD\"))"), XVal::Num(0.0));
        assert_eq!(query(&mut g, "SetFunctionValue (ar_Size (GetEventHandlers \"OnRemoveIMOD\"))"), XVal::Num(0.0));
        query(&mut g, "ApplyImageSpaceModifier OwnedHoldImad\nRemoveImageSpaceModifier OwnedHoldImad");
        assert!(g.image_modifiers.is_empty());
        no_errors(&g);
    }

    let mut g = game(&f);
    query(
        &mut g,
        r#"SetEventHandler "OnApplyIMOD" (Begin Function {ref rImad}
SetModLocalData "imod.apply.calls" ((GetModLocalData "imod.apply.calls") + 1)
SetModLocalData "imod.apply.last" rImad
SetModLocalData "imod.apply.active" (IsImageSpaceActive rImad)
SetModLocalData "imod.apply.ev" (GetCurrentEventName)
End)
SetEventHandler "OnApplyIMOD" (Begin Function {ref rImad}
SetModLocalData "imod.apply.fade_first" ((GetModLocalData "imod.apply.fade_first") + 1)
End) "first"::OwnedFadeImad
SetEventHandlerAlt "OnApplyIMOD" (Begin Function {ref rImad}
SetModLocalData "imod.apply.list" ((GetModLocalData "imod.apply.list") + 1)
End) 1::OwnedImadList
SetEventHandler "OnRemoveIMOD" (Begin Function {ref rImad}
SetModLocalData "imod.remove.calls" ((GetModLocalData "imod.remove.calls") + 1)
SetModLocalData "imod.remove.last" rImad
SetModLocalData "imod.remove.active_sum" ((GetModLocalData "imod.remove.active_sum") + (IsImageSpaceActive rImad))
SetModLocalData "imod.remove.active" (IsImageSpaceActive rImad)
SetModLocalData "imod.remove.ev" (GetCurrentEventName)
End)
SetEventHandlerAlt "OnRemoveIMOD" (Begin Function {ref rImad}
SetModLocalData "imod.remove.hold" ((GetModLocalData "imod.remove.hold") + 1)
End) 1::OwnedHoldImad"#,
    );

    // First apply of non-animatable HoldImad: fires OnApplyIMOD PRE-update with IsImageSpaceActive == 0.
    query(&mut g, "ApplyImageSpaceModifier OwnedHoldImad");
    assert_eq!(num(&mut g, "imod.apply.calls"), 1.0);
    assert_eq!(form(&mut g, "imod.apply.last"), FormId(IMAD_HOLD));
    assert_eq!(num(&mut g, "imod.apply.active"), 0.0, "first OnApplyIMOD must see IsImageSpaceActive == 0");
    assert_eq!(query(&mut g, "SetFunctionValue (IsImageSpaceActive OwnedHoldImad)"), XVal::Num(1.0));
    assert_eq!(text(&mut g, "imod.apply.ev"), "OnApplyIMOD");
    assert_eq!(num(&mut g, "imod.apply.fade_first"), 0.0);
    assert_eq!(num(&mut g, "imod.apply.list"), 0.0);

    // Re-applying an already active non-animatable IMOD fires OnApplyIMOD with IsImageSpaceActive == 1
    // and pushes another ActiveImageModifier entry in application order (capped at MAX_IMOD_STACK).
    query(&mut g, "ApplyImageSpaceModifier OwnedHoldImad");
    assert_eq!(num(&mut g, "imod.apply.calls"), 2.0);
    assert_eq!(num(&mut g, "imod.apply.active"), 1.0, "stacked re-apply sees IsImageSpaceActive == 1");
    assert_eq!(g.image_modifiers.len(), 2, "non-animatable re-apply preserves stacked entries in application order");

    // Snapshot restore with active IMOD must not spuriously fire OnApplyIMOD.
    let snap = g.snapshot();
    let restored = Game::restore(&f.data, World::new(&f.forms, &f.refs, Default::default()), GameConfig { strict: true, ..Default::default() }, &snap).unwrap();
    assert!(restored.image_modifier_active(FormId(IMAD_HOLD)));
    assert_eq!(restored.image_modifiers.len(), 2);
    assert!(restored.events.is_empty());

    // Single RemoveImageSpaceModifier removes BOTH stacked instances and fires OnRemoveIMOD TWICE,
    // with IsImageSpaceActive == 1 inside BOTH callbacks and 0 immediately afterwards.
    query(&mut g, "RemoveImageSpaceModifier OwnedHoldImad");
    assert_eq!(num(&mut g, "imod.remove.calls"), 2.0, "removing 2 stacked instances fires OnRemoveIMOD twice");
    assert_eq!(num(&mut g, "imod.remove.active_sum"), 2.0, "both OnRemoveIMOD callbacks see IsImageSpaceActive == 1");
    assert_eq!(form(&mut g, "imod.remove.last"), FormId(IMAD_HOLD));
    assert_eq!(num(&mut g, "imod.remove.active"), 1.0);
    assert_eq!(query(&mut g, "SetFunctionValue (IsImageSpaceActive OwnedHoldImad)"), XVal::Num(0.0));
    assert_eq!(text(&mut g, "imod.remove.ev"), "OnRemoveIMOD");
    assert_eq!(num(&mut g, "imod.remove.hold"), 2.0);

    // Repeated non-animatable re-applies stay bounded at MAX_IMOD_STACK entries per form.
    for _ in 0..80 {
        query(&mut g, "ApplyImageSpaceModifier OwnedHoldImad");
    }
    assert_eq!(
        g.image_modifiers.len(),
        Game::MAX_IMOD_STACK,
        "80 non-animatable re-applies stay capped at MAX_IMOD_STACK"
    );
    let before_cap_rm = num(&mut g, "imod.remove.calls");
    query(&mut g, "RemoveImageSpaceModifier OwnedHoldImad");
    assert_eq!(
        num(&mut g, "imod.remove.calls") - before_cap_rm,
        Game::MAX_IMOD_STACK as f64,
        "OnRemoveIMOD count is capped at MAX_IMOD_STACK"
    );
    assert_eq!(query(&mut g, "SetFunctionValue (IsImageSpaceActive OwnedHoldImad)"), XVal::Num(0.0));

    // Removing an inactive IMOD must not fire OnRemoveIMOD.
    let before_inactive_rm = num(&mut g, "imod.remove.calls");
    query(&mut g, "RemoveImageSpaceModifier OwnedHoldImad\nRemoveImageSpaceModifier OwnedFadeImad");
    assert_eq!(num(&mut g, "imod.remove.calls"), before_inactive_rm);

    // Apply animatable FadeImad (0.5s): PRE-update IsActive == 0, matches "first"::OwnedFadeImad and 1::OwnedImadList.
    let before_fade_apply = num(&mut g, "imod.apply.calls");
    query(&mut g, "ApplyImageSpaceModifier OwnedFadeImad");
    assert_eq!(num(&mut g, "imod.apply.calls"), before_fade_apply + 1.0);
    assert_eq!(num(&mut g, "imod.apply.active"), 0.0);
    assert_eq!(num(&mut g, "imod.apply.fade_first"), 1.0);
    assert_eq!(num(&mut g, "imod.apply.list"), 1.0);
    assert_eq!(num(&mut g, "imod.remove.calls"), before_inactive_rm);

    // Halfway through duration: still active, OnRemoveIMOD has not fired.
    g.frames(15);
    assert_eq!(num(&mut g, "imod.remove.calls"), before_inactive_rm);

    // Past duration (0.5s = 30 frames at 60Hz): natural expiry dispatches OnRemoveIMOD PRE-removal (IsActive == 1).
    g.frames(16);
    assert_eq!(num(&mut g, "imod.remove.calls"), before_inactive_rm + 1.0);
    assert_eq!(form(&mut g, "imod.remove.last"), FormId(IMAD_FADE));
    assert_eq!(num(&mut g, "imod.remove.active"), 1.0, "natural expiry OnRemoveIMOD sees IsImageSpaceActive == 1");
    assert_eq!(query(&mut g, "SetFunctionValue (IsImageSpaceActive OwnedFadeImad)"), XVal::Num(0.0));
    assert_eq!(num(&mut g, "imod.remove.hold"), 2.0 + Game::MAX_IMOD_STACK as f64, "1::OwnedHoldImad filter must ignore FadeImad");

    // Animatable re-apply stacks independent expiry timers (capped at MAX_IMOD_STACK).
    let before_anim_stack_rm = num(&mut g, "imod.remove.calls");
    query(&mut g, "ApplyImageSpaceModifier OwnedFadeImad");
    g.frames(15);
    query(&mut g, "ApplyImageSpaceModifier OwnedFadeImad");
    assert_eq!(g.image_modifiers.iter().filter(|m| m.modifier == FormId(IMAD_FADE)).count(), 2);
    g.frames(16);
    assert_eq!(num(&mut g, "imod.remove.calls"), before_anim_stack_rm + 1.0, "first animatable instance expires independently");
    assert_eq!(query(&mut g, "SetFunctionValue (IsImageSpaceActive OwnedFadeImad)"), XVal::Num(1.0));
    g.frames(16);
    assert_eq!(num(&mut g, "imod.remove.calls"), before_anim_stack_rm + 2.0, "second animatable instance expires afterwards");
    assert_eq!(query(&mut g, "SetFunctionValue (IsImageSpaceActive OwnedFadeImad)"), XVal::Num(0.0));
    no_errors(&g);

    // Invalid non-IMAD form fails ApplyImageSpaceModifier and must not fire OnApplyIMOD.
    let before = num(&mut g, "imod.apply.calls");
    let _ = g.run_query("Begin Function {}\nApplyImageSpaceModifier OwnedPistol\nEnd", Default::default());
    assert_eq!(num(&mut g, "imod.apply.calls"), before);
    // Keep `restored` live through the end.
    assert_eq!(restored.image_modifiers.len(), 2);
}

#[test]
fn weapon_unholster_event_dispatches_at_draw_start_pre_update_and_zero_unarmed() {
    let f = fixture(true);
    // Vanilla-unchanged control: no handlers registered.
    {
        let mut g = game(&f);
        g.world.add_item(FormId::PLAYER_REF, FormId(WEAP_PISTOL), 1).unwrap();
        query(&mut g, "player.EquipItem OwnedPistol");
        g.set_weapon_out(FormId::PLAYER_REF, true);
        g.set_weapon_out(FormId::PLAYER_REF, false);
        assert_eq!(query(&mut g, "SetFunctionValue (ar_Size (GetEventHandlers \"\"))"), XVal::Num(0.0));
        no_errors(&g);
    }

    let mut g = game(&f);
    g.world.add_item(FormId::PLAYER_REF, FormId(WEAP_PISTOL), 1).unwrap();
    query(
        &mut g,
        r#"player.EquipItem OwnedPistol
SetModLocalData "npc.ref" (player.PlaceAtMe OwnedGuardNpc)
SetModLocalData "crea.ref" (player.PlaceAtMe OwnedGhoulCrea)
SetEventHandler "ShowOff:OnWeaponUnholster" (Begin Function {ref rWeapon}
SetModLocalData "unh.calls" ((GetModLocalData "unh.calls") + 1)
SetModLocalData "unh.actor" GetSelfAlt
SetModLocalData "unh.weapon" rWeapon
SetModLocalData "unh.is_out" (GetSelfAlt.IsWeaponOut)
SetModLocalData "unh.ev" (GetCurrentEventName)
End)
SetEventHandler "ShowOff:OnWeaponUnholster" (Begin Function {ref rWeapon}
SetModLocalData "unh.pistol" ((GetModLocalData "unh.pistol") + 1)
End) "first"::OwnedPistol
SetEventHandlerAlt "ShowOff:OnWeaponUnholster" (Begin Function {ref rWeapon}
SetModLocalData "unh.player_only" ((GetModLocalData "unh.player_only") + 1)
End) 0::player
SetEventHandlerAlt "ShowOff:OnWeaponUnholster" (Begin Function {ref rWeapon}
SetModLocalData "unh.guard_rifle" ((GetModLocalData "unh.guard_rifle") + 1)
End) 0::OwnedGuardNpc 1::OwnedRifle"#,
    );

    let npc = form(&mut g, "npc.ref");
    let crea = form(&mut g, "crea.ref");

    // Repeated holstered reports do not fire.
    g.set_weapon_out(FormId::PLAYER_REF, false);
    assert_eq!(num(&mut g, "unh.calls"), 0.0);

    // 1. Engine animated draw start (AnimAction::Equip while holstered) fires immediately with IsWeaponOut == 0;
    //    interleaved per-frame set_weapon_out(false) calls (as `weapon_state` and `idles.rs` emit every frame while
    //    drawing), repeated Equip frames, and draw completion (`set_weapon_out(true)`) must NOT double-fire.
    g.set_weapon_out(FormId::PLAYER_REF, false);
    g.set_anim_state(FormId::PLAYER_REF, AnimState { action: Some(AnimAction::Equip), ..Default::default() });
    assert_eq!(num(&mut g, "unh.calls"), 1.0, "fires at draw start on AnimAction::Equip");
    assert_eq!(form(&mut g, "unh.actor"), FormId::PLAYER_REF);
    assert_eq!(form(&mut g, "unh.weapon"), FormId(WEAP_PISTOL));
    assert_eq!(num(&mut g, "unh.is_out"), 0.0, "IsWeaponOut must read 0 inside callback (PRE-update)");
    assert_eq!(text(&mut g, "unh.ev"), "ShowOff:OnWeaponUnholster");
    assert_eq!(num(&mut g, "unh.pistol"), 1.0);
    assert_eq!(num(&mut g, "unh.player_only"), 1.0);
    assert_eq!(num(&mut g, "unh.guard_rifle"), 0.0);

    for _ in 0..5 {
        g.tick();
        g.set_weapon_out(FormId::PLAYER_REF, false);
        g.set_anim_state(FormId::PLAYER_REF, AnimState { action: Some(AnimAction::Equip), ..Default::default() });
    }
    assert_eq!(num(&mut g, "unh.calls"), 1.0, "interleaved set_weapon_out(false) during Equip must not clear pending latch");
    // Also cover the NPC ordering where anim_state clears Equip -> None right before set_weapon_out(true) on the same frame.
    g.tick();
    g.set_anim_state(FormId::PLAYER_REF, AnimState::default());
    g.set_weapon_out(FormId::PLAYER_REF, true);
    assert_eq!(num(&mut g, "unh.calls"), 1.0, "draw completion must not double-fire after Equip draw-start");
    assert_eq!(query(&mut g, "SetFunctionValue (player.IsWeaponOut)"), XVal::Num(1.0));

    // Repeated drawn frames must not spam the event.
    for _ in 0..5 {
        g.set_weapon_out(FormId::PLAYER_REF, true);
    }
    assert_eq!(num(&mut g, "unh.calls"), 1.0);

    // Holstering must not fire OnWeaponUnholster.
    g.set_anim_state(FormId::PLAYER_REF, AnimState::default());
    g.set_weapon_out(FormId::PLAYER_REF, false);
    assert_eq!(num(&mut g, "unh.calls"), 1.0);

    // Genuine draw cancellation (Equip -> None without set_weapon_out(true) across frames) clears the latch
    // so a subsequent draw fires cleanly once.
    g.tick();
    g.set_anim_state(FormId::PLAYER_REF, AnimState { action: Some(AnimAction::Equip), ..Default::default() });
    assert_eq!(num(&mut g, "unh.calls"), 2.0, "cancelled draw still fired at draw start");
    g.set_anim_state(FormId::PLAYER_REF, AnimState::default());
    g.tick();
    g.set_weapon_out(FormId::PLAYER_REF, false);
    g.set_anim_state(FormId::PLAYER_REF, AnimState { action: Some(AnimAction::Equip), ..Default::default() });
    assert_eq!(num(&mut g, "unh.calls"), 3.0, "next draw after cancelled draw fires once");
    g.set_weapon_out(FormId::PLAYER_REF, true);
    assert_eq!(num(&mut g, "unh.calls"), 3.0, "completion after retry draw must not double-fire");
    g.set_anim_state(FormId::PLAYER_REF, AnimState::default());
    g.set_weapon_out(FormId::PLAYER_REF, false);

    // 2. Direct set_weapon_out(true) fallback (NPC without prior Equip action) also dispatches PRE-update (IsWeaponOut == 0)
    //    with held_weapon, matching 0::OwnedGuardNpc + 1::OwnedRifle.
    g.set_held_weapon(npc, Some(FormId(WEAP_RIFLE)));
    g.set_weapon_out(npc, true);
    assert_eq!(num(&mut g, "unh.calls"), 4.0);
    assert_eq!(form(&mut g, "unh.actor"), npc);
    assert_eq!(form(&mut g, "unh.weapon"), FormId(WEAP_RIFLE));
    assert_eq!(num(&mut g, "unh.is_out"), 0.0, "direct set_weapon_out(true) must also dispatch PRE-update");
    assert_eq!(num(&mut g, "unh.pistol"), 3.0);
    assert_eq!(num(&mut g, "unh.player_only"), 3.0);
    assert_eq!(num(&mut g, "unh.guard_rifle"), 1.0);

    // 3. Unarmed actor passes null form 0 (NOT Fists 0x1F4) even when Fists WEAP exists in game data!
    g.set_weapon_out(crea, true);
    assert_eq!(num(&mut g, "unh.calls"), 5.0);
    assert_eq!(form(&mut g, "unh.actor"), crea);
    assert_eq!(form(&mut g, "unh.weapon"), FormId(0), "unarmed must pass 0, not Fists 0x1F4");
    assert_eq!(num(&mut g, "unh.is_out"), 0.0);

    // 4. Scripted SetWeaponOut 1 fires immediately at draw START (frame 0, IsWeaponOut == 0),
    //    and interleaved per-frame set_weapon_out(false) during WEAPON_FRAMES, WEAPON_FRAMES expiry in hud_tick,
    //    and subsequent set_weapon_out(true) must NOT double-fire.
    query(&mut g, "player.SetWeaponOut 1");
    assert_eq!(num(&mut g, "unh.calls"), 6.0, "SetWeaponOut 1 fires at draw start");
    assert_eq!(form(&mut g, "unh.actor"), FormId::PLAYER_REF);
    assert_eq!(form(&mut g, "unh.weapon"), FormId(WEAP_PISTOL));
    assert_eq!(num(&mut g, "unh.is_out"), 0.0, "SetWeaponOut 1 callback sees IsWeaponOut == 0");
    assert_eq!(query(&mut g, "SetFunctionValue (player.IsWeaponOut)"), XVal::Num(0.0));
    for _ in 0..WEAPON_FRAMES {
        g.set_weapon_out(FormId::PLAYER_REF, false);
        g.tick();
    }
    assert_eq!(query(&mut g, "SetFunctionValue (player.IsWeaponOut)"), XVal::Num(1.0));
    assert_eq!(num(&mut g, "unh.calls"), 6.0, "interleaved set_weapon_out(false) and hud_tick completion must not double-fire");
    g.set_weapon_out(FormId::PLAYER_REF, true);
    assert_eq!(num(&mut g, "unh.calls"), 6.0, "already drawn: no duplicate fire");

    // Snapshot restore with weapon_out == true does not spuriously fire OnWeaponUnholster.
    let snap = g.snapshot();
    let mut restored = Game::restore(&f.data, World::new(&f.forms, &f.refs, Default::default()), GameConfig { strict: true, ..Default::default() }, &snap).unwrap();
    assert!(restored.extra.get(&FormId::PLAYER_REF).is_some_and(|e| e.weapon_out));
    query(
        &mut restored,
        r#"SetEventHandler "ShowOff:OnWeaponUnholster" (Begin Function {ref rWeapon}
SetModLocalData "restored.unh" ((GetModLocalData "restored.unh") + 1)
End)"#,
    );
    // First engine frame after load transitioning true -> false must not fire unholster.
    restored.set_weapon_out(FormId::PLAYER_REF, false);
    assert!(!restored.extra.get(&FormId::PLAYER_REF).is_some_and(|e| e.weapon_out));
    assert_eq!(num(&mut restored, "restored.unh"), 0.0);
    no_errors(&restored);
    no_errors(&g);
}

#[test]
fn weapon_unholster_unarmed_without_fists_record_passes_null_form() {
    let f = fixture(false);
    let mut g = game(&f);
    query(
        &mut g,
        r#"SetEventHandler "ShowOff:OnWeaponUnholster" (Begin Function {ref rWeapon}
SetModLocalData "unh.calls" ((GetModLocalData "unh.calls") + 1)
SetModLocalData "unh.weapon" rWeapon
SetModLocalData "unh.is_out" (GetSelfAlt.IsWeaponOut)
End)"#,
    );
    g.set_weapon_out(FormId::PLAYER_REF, true);
    assert_eq!(num(&mut g, "unh.calls"), 1.0);
    assert_eq!(form(&mut g, "unh.weapon"), FormId(0));
    assert_eq!(num(&mut g, "unh.is_out"), 0.0);
    no_errors(&g);
}
