//! JohnnyGuitar NVSE batch 6 (#1007; docs/spec-notes/ext-jg6.md) on a synthetic plugin:
//! actor values by code, actor states, karma, cells, lighting templates, radios, prompts,
//! primitives, effect shaders and the per-reference calls.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const MISC: u32 = 0x900;
const ROOM: u32 = 0x910;
const ROOM2: u32 = 0x911;
const REF_PROMPT: u32 = 0x912;
const REF_BOX: u32 = 0x913;
const REF_PLAIN: u32 = 0x914;
const LGTM: u32 = 0x920;
const TACT_RADIO: u32 = 0x930;
const TACT_SPEAKER: u32 = 0x931;
const REF_RADIO: u32 = 0x932;
const REF_SPEAKER: u32 = 0x933;
const EFSH: u32 = 0x940;
const GLOB: u32 = 0x950;
const NPC: u32 = 0x960;
const ACTOR_ON: u32 = 0x961;
const ACTOR_OFF: u32 = 0x962;

fn edid(sig: &[u8; 4], flags: u32, fid: u32, name: &str, rest: &[fnv_esm::raw::Subrecord]) -> Vec<u8> {
    record(sig, flags, fid, &[vec![sub(b"EDID", z(name))], rest.to_vec()].concat())
}

fn le(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

/// A persistent reference (record flag 0x400), so walks find it outside the loaded area.
fn placed(fid: u32, name: &str, base: u32, rest: &[fnv_esm::raw::Subrecord]) -> Vec<u8> {
    let subs = [vec![sub(b"EDID", z(name)), sub(b"NAME", le(base))], rest.to_vec(), vec![sub(b"DATA", vec![0; 24])]].concat();
    record(b"REFR", 0x400, fid, &subs)
}

fn plugin_bytes() -> Vec<u8> {
    // ACBS (flags, fatigue, barter gold, level, calc min/max, speed, karma, disposition,
    // template flags) and DATA (health, the S.P.E.C.I.A.L.), as tests/actor_values.rs.
    let mut acbs = 0u32.to_le_bytes().to_vec();
    for v in [50u16, 0, 1, 0, 0, 100] {
        acbs.extend(v.to_le_bytes());
    }
    acbs.extend(0f32.to_le_bytes());
    acbs.extend([0u8; 4]);
    let data = [100i32.to_le_bytes().to_vec(), vec![5, 5, 5, 5, 5, 5, 5]].concat();
    let npc_player = record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player")), sub(b"ACBS", acbs.clone()), sub(b"DATA", data.clone())]);
    let npc = record(b"NPC_", 0, NPC, &[sub(b"EDID", z("TestNpc")), sub(b"ACBS", acbs), sub(b"DATA", data)]);
    let misc = edid(b"MISC", 0, MISC, "TestThing", &[]);
    // XPRM [documented: schema world.json]: bounds x y z, colour r g b, unknown, type 1 box.
    let mut xprm = Vec::new();
    for v in [10.0f32, 20.0, 30.0, 1.0, 0.0, 0.0, 0.0] {
        xprm.extend(v.to_le_bytes());
    }
    xprm.extend(le(1));
    let room = edid(b"CELL", 0, ROOM, "TestRoom", &[sub(b"FULL", z("Test Room")), sub(b"DATA", vec![1])]);
    let room_refs = [
        placed(REF_PROMPT, "TestPromptRef", MISC, &[sub(b"XATO", z("Open Me"))]),
        placed(REF_BOX, "TestBoxRef", MISC, &[sub(b"XPRM", xprm)]),
        placed(REF_PLAIN, "TestPlainRef", MISC, &[]),
        placed(REF_RADIO, "TestRadioRef", TACT_RADIO, &[]),
        placed(REF_SPEAKER, "TestSpeakerRef", TACT_SPEAKER, &[]),
    ]
    .concat();
    let room_children = group(ROOM.to_le_bytes(), 6, &group(ROOM.to_le_bytes(), 8, &room_refs));
    let room2 = edid(b"CELL", 0, ROOM2, "TestRoom2", &[sub(b"DATA", vec![1])]);
    // Two actors in the second room, the second initially disabled (record flag 0x800).
    let actor = |fid: u32, name: &str, flags: u32| record(b"ACHR", 0x400 | flags, fid, &[sub(b"EDID", z(name)), sub(b"NAME", le(NPC)), sub(b"DATA", vec![0; 24])]);
    let room2_refs = [actor(ACTOR_ON, "TestActorOn", 0), actor(ACTOR_OFF, "TestActorOff", 0x800)].concat();
    let room2_children = group(ROOM2.to_le_bytes(), 6, &group(ROOM2.to_le_bytes(), 8, &room2_refs));
    let lgtm = edid(b"LGTM", 0, LGTM, "TestTemplate", &[]);
    // TACT header flag 0x00020000: the Radio Station box (radio.md §2).
    let tact = [edid(b"TACT", 0x0002_0000, TACT_RADIO, "TestStation", &[]), edid(b"TACT", 0, TACT_SPEAKER, "TestSpeaker", &[])].concat();
    let efsh = edid(b"EFSH", 0, EFSH, "TestShader", &[]);
    let glob = edid(b"GLOB", 0, GLOB, "TestCount", &[sub(b"FNAM", vec![b's']), sub(b"FLTV", 0f32.to_le_bytes().to_vec())]);
    [
        tes4(),
        top(b"GLOB", &glob),
        top(b"NPC_", &[npc_player, npc].concat()),
        top(b"MISC", &misc),
        top(b"TACT", &tact),
        top(b"EFSH", &efsh),
        top(b"LGTM", &lgtm),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[room, room_children, room2, room2_children].concat()))),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Jg6.esm".into()).unwrap()];
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
    Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() })
}

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(g: &mut Game, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(x) => x,
        XVal::Ref(r) => f64::from(r),
        v => panic!("{expr}: {v:?}"),
    }
}

fn s(g: &mut Game, expr: &str) -> String {
    match q(g, &format!("string_var sOut\nlet sOut := {expr}\nSetFunctionValue sOut")) {
        XVal::Str(s) => s,
        v => panic!("{expr}: {v:?}"),
    }
}

/// An array's entries as FormIDs.
fn forms(g: &mut Game, expr: &str) -> Vec<u32> {
    let at = |i: &str| format!("array_var a\nlet a := {expr}\nSetFunctionValue {i}");
    let len = match q(g, &at("(ar_Size a)")) {
        XVal::Num(x) => x as usize,
        v => panic!("{expr}: {v:?}"),
    };
    (0..len)
        .map(|i| match q(g, &at(&format!("(a[{i}])"))) {
            XVal::Ref(r) => r,
            XVal::Num(x) => x as u32,
            v => panic!("{expr}[{i}]: {v:?}"),
        })
        .collect()
}

#[test]
fn actor_values_by_code() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: ModActorValueAlt, "Player.ModActorValueAlt 5 3 … increase Player's
    // Strength by 3"].
    let str0 = n(&mut g, "player.GetActorValue Strength");
    q(&mut g, "player.ModActorValueAlt 5 3");
    assert_eq!(n(&mut g, "player.GetActorValue Strength"), str0 + 3.0);
    // Out-of-range AV code is safely rejected instead of crashing [design: Wave 4
    // jg_17_mod_av_alt_out_of_range crashed JohnnyGuitarNVSE 528 with 0xC0000005].
    q(&mut g, "player.ModActorValueAlt 999 5");
    assert_eq!(n(&mut g, "player.GetActorValue Strength"), str0 + 3.0);
    // GetActorValueModifierAlt reads the effects' modifiers only: ModAV's change is in
    // none of its filters [observed 2026-11-08: jg6.av.mod].
    for filter in ["1", "2", "3", "4", ""] {
        assert_eq!(n(&mut g, &format!("player.GetActorValueModifierAlt Strength {filter}")), 0.0, "filter {filter}");
    }
    // [documented: DamageActorValueAlt and RestoreActorValueAlt on Health, code 16].
    let hp = n(&mut g, "player.GetActorValue Health");
    q(&mut g, "player.DamageActorValueAlt 16 5");
    assert_eq!(n(&mut g, "player.GetActorValue Health"), hp - 5.0);
    q(&mut g, "player.RestoreActorValueAlt 16 5");
    assert_eq!(n(&mut g, "player.GetActorValue Health"), hp);
    // [documented: ForceActorValueAlt, the current value becomes the number].
    q(&mut g, "player.ForceActorValueAlt 16 50");
    assert_eq!(n(&mut g, "player.GetActorValue Health"), 50.0);
    // [documented: SetActorValueAlt sets the base; UnsetAV puts back the base before it].
    let luck = n(&mut g, "player.GetBaseActorValue Luck");
    q(&mut g, "player.SetActorValueAlt 11 9");
    assert_eq!(n(&mut g, "player.GetBaseActorValue Luck"), 9.0);
    q(&mut g, "player.SetActorValue Luck 7");
    q(&mut g, "player.UnsetAV Luck");
    assert_eq!(n(&mut g, "player.GetBaseActorValue Luck"), luck);
    // [documented: UnforceAV "does nothing"].
    q(&mut g, "player.UnforceAV Health");
    assert_eq!(n(&mut g, "player.GetActorValue Health"), 50.0);
}

#[test]
fn karma_without_message() {
    let f = fixture();
    let mut g = game(&f);
    let k = n(&mut g, "player.GetActorValue Karma");
    // [documented: RewardKarmaAlt: RewardKarma without the corner message].
    q(&mut g, "RewardKarmaAlt 100");
    assert_eq!(n(&mut g, "player.GetActorValue Karma"), k + 100.0);
    assert!(g.crimes.take_notes().is_empty());
}

#[test]
fn actor_states() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: GetLifeState 0 alive, 3 unconscious, 5 restrained].
    assert_eq!(n(&mut g, "player.GetLifeState"), 0.0);
    q(&mut g, "player.SetRestrained 1");
    assert_eq!(n(&mut g, "player.GetLifeState"), 5.0);
    assert_eq!(n(&mut g, "player.IsInDialogueWithPlayer"), 0.0);
    assert_eq!(n(&mut g, "player.GetCurrentFurnitureRef"), 0.0);
    assert_eq!(n(&mut g, "player.HasHealthDamageEffect"), 0.0);
    assert_eq!(forms(&mut g, "GetTempIngestibleEffects"), Vec::<u32>::new());
    assert_eq!(n(&mut g, "player.ModNthTempEffectTimeLeft 0 5"), 0.0);
    // [documented: GetHotkeySlot, the hotkey of the item, 0 for none].
    assert_eq!(n(&mut g, "TestPlainRef.GetHotkeySlot"), 0.0);
    q(&mut g, "player.AddItem TestThing 1");
    q(&mut g, "SetHotkeyItem 3 TestThing");
    assert_eq!(n(&mut g, "TestPlainRef.GetHotkeySlot"), 3.0);
    // Nothing owned: no alarm, no note.
    q(&mut g, "TestPlainRef.SendTrespassAlarmAlt");
    assert!(g.crimes.take_notes().is_empty());
}

#[test]
fn cells_and_templates() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: IsCellVisited; IsCellExpired -1 not visited, 0 not expired, 1 after
    // ResetInterior].
    assert_eq!(n(&mut g, "IsCellVisited TestRoom"), 0.0);
    assert_eq!(n(&mut g, "IsCellExpired TestRoom"), -1.0);
    g.set_loaded([FormId(ROOM)].into_iter().collect());
    g.set_loaded(Default::default());
    assert_eq!(n(&mut g, "IsCellVisited TestRoom"), 1.0);
    assert_eq!(n(&mut g, "IsCellVisited TestRoom2"), 0.0);
    assert_eq!(n(&mut g, "IsCellExpired TestRoom"), 0.0);
    q(&mut g, "ResetInterior TestRoom");
    assert_eq!(n(&mut g, "IsCellExpired TestRoom"), 1.0);
    // [documented: Get/SetLightingTemplateCell].
    assert_eq!(n(&mut g, "GetLightingTemplateCell TestTemplate"), 0.0);
    q(&mut g, "SetLightingTemplateCell TestTemplate TestRoom2");
    assert_eq!(n(&mut g, "GetLightingTemplateCell TestTemplate"), f64::from(ROOM2));
    q(&mut g, "SetLightingTemplateCell TestTemplate TestThing");
    assert_eq!(n(&mut g, "GetLightingTemplateCell TestTemplate"), f64::from(ROOM2));
    // No climate: phase 0 (see docs/spec-notes/ext-jg6.md); the frame step.
    assert_eq!(n(&mut g, "GetMoonPhase"), 0.0);
    assert!((n(&mut g, "GameGetSecondsPassed") - 1.0 / 60.0).abs() < 1e-9);
}

#[test]
fn radios_prompts_primitives_shaders() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: GetAllGameRadios: the radio stations' references].
    assert_eq!(forms(&mut g, "GetAllGameRadios"), vec![REF_RADIO]);
    assert_eq!(n(&mut g, "GetTalkingActivatorActor TestRadioRef"), 0.0);
    // [documented: Get/SetRefActivationPromptOverride, XATO].
    assert_eq!(s(&mut g, "TestPromptRef.GetRefActivationPromptOverride"), "Open Me");
    assert_eq!(s(&mut g, "TestPlainRef.GetRefActivationPromptOverride"), "");
    q(&mut g, "TestPlainRef.SetRefActivationPromptOverride \"Go\"");
    assert_eq!(s(&mut g, "TestPlainRef.GetRefActivationPromptOverride"), "Go");
    assert_eq!(fnv_game::nvse_jg6::activation_prompt(&g, FormId(REF_PLAIN)).as_deref(), Some("Go"));
    // [documented: GetPrimitiveType 1 box; RemovePrimitive 1 on success].
    assert_eq!(n(&mut g, "TestBoxRef.GetPrimitiveType"), 1.0);
    assert_eq!(n(&mut g, "TestPlainRef.GetPrimitiveType"), 0.0);
    assert_eq!(n(&mut g, "TestBoxRef.RemovePrimitive"), 1.0);
    assert_eq!(n(&mut g, "TestBoxRef.RemovePrimitive"), 0.0);
    assert_eq!(n(&mut g, "TestBoxRef.GetPrimitiveType"), 0.0);
    // [documented: GetPlayingEffectShaders].
    assert_eq!(forms(&mut g, "TestPlainRef.GetPlayingEffectShaders"), Vec::<u32>::new());
    q(&mut g, "TestPlainRef.PlayMagicShaderVisuals TestShader");
    assert_eq!(forms(&mut g, "TestPlainRef.GetPlayingEffectShaders"), vec![EFSH]);
    assert_eq!(n(&mut g, "TestPlainRef.GetMineArmedEx"), 0.0);
}

#[test]
fn calls_per_reference() {
    let f = fixture();
    let mut g = game(&f);
    // A cell that is not loaded gives no calls [observed: jg6 rerun, Doc's house].
    g.run_source("CallPerRef (begin function {ref r}\nlet TestCount := TestCount + 1\nend) 0 0 0 TestRoom", RunCtx::default()).unwrap();
    assert_eq!(n(&mut g, "TestCount"), 0.0);
    g.set_loaded([FormId(ROOM)].into_iter().collect());
    // [documented: CallPerRef: once per reference of the cell; the type filters].
    g.run_source("CallPerRef (begin function {ref r}\nlet TestCount := TestCount + 1\nend) 0 0 0 TestRoom", RunCtx::default()).unwrap();
    assert_eq!(n(&mut g, "TestCount"), 5.0);
    g.run_source("set TestCount to 0\nCallPerRefEx (begin function {ref r}\nlet TestCount := TestCount + 1\nend) (ar_List 31) 0 0 TestRoom", RunCtx::default()).unwrap();
    assert_eq!(n(&mut g, "TestCount"), 3.0);
    // Disabled references are left out.
    q(&mut g, "TestPlainRef.Disable");
    g.run_source("set TestCount to 0\nCallPerRef (begin function {ref r}\nlet TestCount := TestCount + 1\nend) 31 0 0 TestRoom", RunCtx::default()).unwrap();
    assert_eq!(n(&mut g, "TestCount"), 2.0);
    // No actors are loaded: level 0 calls nothing.
    g.run_source("set TestCount to 0\nCallPerMobileObject (begin function {ref r}\nlet TestCount := TestCount + 1\nend) 0", RunCtx::default()).unwrap();
    assert_eq!(n(&mut g, "TestCount"), 0.0);
}

#[test]
fn disabled_actors_have_no_processing_level() {
    let f = fixture();
    let mut g = game(&f);
    g.set_loaded([FormId(ROOM2)].into_iter().collect());
    assert_eq!(n(&mut g, "TestActorOff.GetDisabled"), 1.0);
    // The disabled actor is left out [observed: L5.ext rerun, #2040, 16 where 20 are
    // loaded with 4 disabled]; CallPerMobileObject calls the same one [documented].
    assert_eq!(n(&mut g, "ar_Size (GetActorsByProcessingLevel 0)"), 1.0);
    assert_eq!(n(&mut g, "(GetActorsByProcessingLevel 0)[0]"), f64::from(ACTOR_ON));
    g.run_source("set TestCount to 0
CallPerMobileObject (begin function {ref r}
let TestCount := TestCount + 1
end) 0", RunCtx::default()).unwrap();
    assert_eq!(n(&mut g, "TestCount"), 1.0);
    // Enabled, it is listed.
    q(&mut g, "TestActorOff.Enable");
    assert_eq!(n(&mut g, "ar_Size (GetActorsByProcessingLevel 0)"), 2.0);
}

/// Getters with nothing to report [observed: L5.ext rerun, #2040, gaps7 terrain.far and
/// tempeffects.player].
#[test]
fn getters_with_nothing_found() {
    let f = fixture();
    let mut g = game(&f);
    // No terrain there: -2048.
    assert_eq!(n(&mut g, "GetTerrainHeight 900000 900000"), -2048.0);
    // No temporary effects: no array, which ar_Size reads -1.
    assert_eq!(n(&mut g, "ar_Size (PlayerREF.GetTempEffects)"), -1.0);
}

#[test]
fn get_refs_in_cell_leaves_out_the_references_beyond_max_distance() {
    // [documented: GetRefsInCell, maxDistance; from the calling reference (hypothesis, as
    // CallPerRef's distance).] A sixth argument compiles and changes nothing
    // (script-compiler.md §8.1).
    let f = fixture();
    let mut g = game(&f);
    g.set_loaded([FormId(ROOM)].into_iter().collect());
    assert_eq!(n(&mut g, "ar_Size (TestPromptRef.GetRefsInCell TestRoom 0 0 0)"), 5.0);
    q(&mut g, "TestPlainRef.SetPos X 5000");
    assert_eq!(n(&mut g, "ar_Size (TestPromptRef.GetRefsInCell TestRoom 0 0 0 100)"), 4.0);
    assert_eq!(n(&mut g, "ar_Size (TestPromptRef.GetRefsInCell TestRoom 0 0 0 100 45)"), 4.0);
    assert_eq!(n(&mut g, "ar_Size (TestPromptRef.GetRefsInCell TestRoom 0 0 0 0)"), 5.0);
}

#[test]
fn get_threshold_av_resolves_bare_perception_and_caps_special_values() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "PlayerREF.GetThresholdAV Perception"), 5.0);
    q(&mut g, "PlayerREF.SetActorValue Perception 8");
    assert_eq!(n(&mut g, "PlayerREF.GetThresholdAV Perception"), 8.0);
    assert_eq!(n(&mut g, "PlayerREF.GetThresholdedActorValue Perception"), 8.0);
    assert_eq!(n(&mut g, "PlayerREF.GetThresholdAV \"Perception\""), 8.0);
    assert_eq!(n(&mut g, "PlayerREF.GetThresholdAV 6"), 8.0);
    // SPECIAL values above 10 threshold to 10 [documented: GetThresholdedActorValue].
    q(&mut g, "PlayerREF.ModActorValue Perception 5");
    assert_eq!(n(&mut g, "PlayerREF.GetThresholdAV Perception"), 10.0);
}
