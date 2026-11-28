//! JIP LN NVSE batch 9 (#1006; docs/spec-notes/jip-ui-9.md) on a synthetic plugin: the
//! HUD cursor and the mouse, camera and menu switches, the hotkeys' inventory references,
//! the Pip-Boy radio, the volumes, fonts and colours, the health bar's actor, immortal
//! mode, interaction by form type, GetPosEx and the NIF block values (materials, the
//! translation and rotation into script variables).

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::combat::HitInfo;
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::nif_blocks::{SceneEdit, SceneNode};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CELL: u32 = 0x200;
const NPC: u32 = 0x900;
const MARK: u32 = 0x901;
const MISC: u32 = 0x910;
const DOOR: u32 = 0x911;
const DOOR_REF: u32 = 0x912;
const ACTI: u32 = 0x913;
const ACTI_REF: u32 = 0x914;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn placed(sig: &[u8; 4], fid: u32, edid: &str, base: u32, pos: [f32; 3]) -> Vec<u8> {
    let data = [f32b(pos[0]), f32b(pos[1]), f32b(pos[2]), vec![0; 12]].concat();
    record(sig, 0, fid, &[sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn plugin_bytes() -> Vec<u8> {
    let npcs = [record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]), record(b"NPC_", 0, NPC, &[sub(b"EDID", z("TestNpc"))])].concat();
    let misc = record(b"MISC", 0, MISC, &[sub(b"EDID", z("TestMisc")), sub(b"DATA", [u32s(&[5]), f32b(1.0)].concat())]);
    let door = record(b"DOOR", 0, DOOR, &[sub(b"EDID", z("TestDoor"))]);
    let acti = record(b"ACTI", 0, ACTI, &[sub(b"EDID", z("TestActi"))]);
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
    let refs = [
        placed(b"ACHR", MARK, "MarkRef", NPC, [100.0, 200.0, 0.0]),
        placed(b"REFR", DOOR_REF, "DoorRef", DOOR, [10.0, 20.0, 30.0]),
        placed(b"REFR", ACTI_REF, "ActiRef", ACTI, [0.0, 0.0, 0.0]),
    ]
    .concat();
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs));
    [
        tes4(),
        top(b"NPC_", &npcs),
        top(b"MISC", &misc),
        top(b"DOOR", &door),
        top(b"ACTI", &acti),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat()))),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Jip9.esm".into()).unwrap()];
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
    g.player_entered(Some(FormId(CELL)));
    g
}

const VARS: &str = "float fX\nfloat fY\nfloat fZ\nfloat fR\nref rItem\nstring_var sOut\n";

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("{VARS}Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(g: &mut Game, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(x) => x,
        XVal::Ref(r) => f64::from(r),
        v => panic!("{expr}: {v:?}"),
    }
}

fn s(g: &mut Game, body: &str) -> String {
    match q(g, body) {
        XVal::Str(s) => s,
        v => panic!("{body}: {v:?}"),
    }
}

#[test]
fn the_hud_cursor_and_the_mouse_switches() {
    let f = fixture();
    let mut g = game(&f);
    // The state before the call, so asked without an argument the state [observed: jip9
    // hudcursor 0 0 1 0].
    assert_eq!(n(&mut g, "ToggleHUDCursor"), 0.0);
    assert_eq!(n(&mut g, "ToggleHUDCursor 1"), 0.0);
    assert_eq!(n(&mut g, "ToggleHUDCursor"), 1.0);
    // Any menu switches it off [documented].
    g.open_menu(1002);
    assert_eq!(n(&mut g, "ToggleHUDCursor"), 0.0);
    q(&mut g, "ToggleHUDCursor 1");
    assert_eq!(n(&mut g, "ToggleHUDCursor"), 0.0, "not while a menu is open");
    // CloseActiveMenu closes the menu on top [documented].
    q(&mut g, "CloseActiveMenu");
    assert_eq!(g.menu, None);
    g.open_menu(1002);
    q(&mut g, "CloseActiveMenu 1");
    assert_eq!(g.menu, None);
    // The mouse's axes: 0-3 [documented], the state before; 9 keeps its two low bits
    // [observed: jip9 mouse.1 and mouse.9, 3 3 1 3].
    assert_eq!(n(&mut g, "ToggleMouseMovement"), 3.0);
    assert_eq!(n(&mut g, "ToggleMouseMovement 2"), 3.0);
    assert_eq!(n(&mut g, "ToggleMouseMovement"), 2.0);
    assert_eq!(n(&mut g, "ToggleMouseMovement 9"), 2.0);
    assert_eq!(n(&mut g, "ToggleMouseMovement"), 1.0);
    // The vanity wheel and the camera's collision are on until turned off.
    assert_eq!((n(&mut g, "ToggleVanityWheel"), n(&mut g, "ToggleCameraCollision")), (1.0, 1.0));
    assert_eq!((n(&mut g, "ToggleVanityWheel 0"), n(&mut g, "ToggleCameraCollision 0")), (1.0, 1.0));
    assert_eq!((n(&mut g, "ToggleVanityWheel"), n(&mut g, "ToggleCameraCollision")), (0.0, 0.0));
    let j = &g.other.jip9;
    assert!(!j.vanity_wheel && !j.camera_collision && j.mouse_axes == 1);
}

#[test]
fn the_cursor_where_the_engine_tells_it() {
    let f = fixture();
    let mut g = game(&f);
    // No window told and no INI: 1280 by 720; outside MenuMode and the HUD cursor the
    // pointer is its centre [observed: jip9 cursor.*, 640 360 and 853.333 480].
    assert_eq!((n(&mut g, "GetCursorPos X"), n(&mut g, "GetCursorPos Y")), (640.0, 360.0));
    let x1 = n(&mut g, "GetCursorPos X 1");
    assert!((x1 - 853.333).abs() < 1e-3, "{x1}");
    assert_eq!(n(&mut g, "GetCursorPos Y 1"), 480.0);
    // A window twice the interface's height: two pixels a unit.
    g.other.jip9.window = Some((2560.0, 1920.0));
    assert_eq!(n(&mut g, "GetCursorPos X"), 1280.0, "the centre, with no cursor known");
    // The engine tells the system cursor: outside the menus that is the pointer
    // [observed: a jip9 rerun gave 1278.5 / 431.94 where the mouse was].
    g.other.jip9.cursor_px = Some((640.0, 480.0));
    assert_eq!(n(&mut g, "GetCursorPos X"), 640.0, "the system cursor");
    q(&mut g, "ToggleHUDCursor 1");
    assert_eq!((n(&mut g, "GetCursorPos X"), n(&mut g, "GetCursorPos Y")), (640.0, 480.0));
    // In XML coordinates: the tree's pointer, else the pixels over two.
    assert_eq!(n(&mut g, "GetCursorPos Y 1"), 240.0);
    g.ui_pointer.at = Some((10.0, 20.0));
    assert_eq!(n(&mut g, "GetCursorPos X 1"), 10.0);
    // SetCursorPos asks the engine; the place reads back at once.
    q(&mut g, "SetCursorPos 100 50 1");
    assert_eq!(g.other.jip9.cursor_warp, Some((100.0, 50.0, true)));
    assert_eq!(n(&mut g, "GetCursorPos X"), 200.0);
    q(&mut g, "ToggleHUDCursor 0\nSetCursorPos 5 5");
    assert_eq!(g.other.jip9.cursor_warp, Some((100.0, 50.0, true)), "not outside the cursor's modes");
}

#[test]
fn a_headless_window_is_the_ini_display_size() {
    let mut f = fixture();
    f.data.ini_texts = vec!["[Display]\niSize W=1920\niSize H=1080\n".into()];
    let mut g = game(&f);
    assert_eq!((n(&mut g, "GetCursorPos X"), n(&mut g, "GetCursorPos Y")), (960.0, 540.0));
    assert_eq!((n(&mut g, "GetCursorPos X 1"), n(&mut g, "GetCursorPos Y 1")), (853.3333333333334, 480.0));
}

#[test]
fn hotkeys_give_and_take_inventory_references() {
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "player.AddItem TestMisc 2");
    q(&mut g, "SetHotkeyItem 4 TestMisc");
    let r = n(&mut g, "GetHotkeyItemRef 4") as u32;
    assert_eq!(g.temp_item(FormId(r)), Some((FormId::PLAYER_REF, FormId(MISC))));
    assert_eq!(n(&mut g, "GetHotkeyItemRef 5"), 0.0, "an empty key");
    // The reference on another key: 1 and 3-8, not 2 (Ammo Swap) [documented].
    let set = |g: &mut Game, k: u32| match q(g, &format!("let rItem := GetHotkeyItemRef 4\nlet fR := rItem.SetHotkeyItemRef {k}\nSetFunctionValue fR")) {
        XVal::Num(x) => x,
        v => panic!("{v:?}"),
    };
    assert_eq!(set(&mut g, 7), 1.0);
    assert_eq!(n(&mut g, "GetHotkeyItem 7"), f64::from(MISC));
    assert_eq!(set(&mut g, 2), 0.0);
    // Not carried any more: none.
    q(&mut g, "player.RemoveItem TestMisc 2");
    assert_eq!(n(&mut g, "GetHotkeyItemRef 4"), 0.0);
}

#[test]
fn radio_volumes_fonts_colours_and_the_health_bar() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetPipboyRadio"), 0.0);
    g.radio.pipboy.station = Some(FormId(ACTI_REF));
    assert_eq!(n(&mut g, "GetPipboyRadio"), 0.0, "tuned but off");
    g.radio.pipboy.on = true;
    assert_eq!(n(&mut g, "GetPipboyRadio"), f64::from(ACTI_REF));
    // The defaults [observed: 60 50 60 100 50 for music, footsteps, voice, effects, radio].
    let all: Vec<f64> = (0..6).map(|t| n(&mut g, &format!("GetGameVolume {t}"))).collect();
    assert_eq!(all, [100.0, 50.0, 60.0, 100.0, 60.0, 50.0]);
    assert_eq!(n(&mut g, "GetGameVolume 6"), 0.0);
    q(&mut g, "SetGameVolume 4 30\nSetGameVolume 1 150\nSetGameVolume 4 101");
    // Past 100 nothing changes [observed: jip9 volume.set, w4c_a04_set_game_volume_gt100].
    assert_eq!((n(&mut g, "GetGameVolume 4"), n(&mut g, "GetGameVolume 1")), (30.0, 50.0));
    // Negative level resets channel to its default [observed: b3_jip9_02_game_volume_neg_and_default,
    // w4c_a04_set_game_volume_gt100]; omitted level also resets to default [observed: jip9 volume.set].
    q(&mut g, "SetGameVolume 0 30\nSetGameVolume 0 -10");
    assert_eq!(n(&mut g, "GetGameVolume 0"), 100.0, "negative level resets master to default");
    q(&mut g, "SetGameVolume 4");
    assert_eq!(n(&mut g, "GetGameVolume 4"), 60.0, "back to the default");
    assert_eq!(g.other.jip9.volume_sets, [(4, 30.0), (0, 30.0), (0, 100.0), (4, 60.0)]);
    // Fonts: only types 1-8 and 10-89, and a file that is there.
    assert_eq!(n(&mut g, "SetFontFile 9 \"textures\\fonts\\x.fnt\""), 0.0);
    assert_eq!(n(&mut g, "SetFontFile 2 \"textures\\fonts\\no such file.fnt\""), 0.0);
    // Integer channels wrap modulo 256 (u8 conversion) [observed: w4c_b1_15b_syscolor_cursor]
    // and fractional channel inputs truncate toward zero [observed: w4h_10_set_system_color_fractional,
    // PR #3803 / #3802].
    q(&mut g, "SetSystemColor 1 300 -5 12.4\nSetSystemColor 6 1 1 1");
    assert_eq!(g.other.jip9.colours.get(&1), Some(&[44, 251, 12]));
    assert_eq!(g.other.jip9.colour_sets.len(), 1);
    q(&mut g, "SetSystemColor 1 300 -10 128");
    assert_eq!(g.other.jip9.colours.get(&1), Some(&[44, 246, 128]));
    assert_eq!(n(&mut g, "GetSystemColor 1"), 2_946_688.0);
    q(&mut g, "SetSystemColor 1 12.7 12.3 12.5");
    assert_eq!(g.other.jip9.colours.get(&1), Some(&[12, 12, 12]));
    assert_eq!(n(&mut g, "GetSystemColor 1"), f64::from(0x0C0C0C_u32));
    q(&mut g, "SetSystemColor 1 13.5 0.4 254.6");
    assert_eq!(g.other.jip9.colours.get(&1), Some(&[13, 0, 254]));
    assert_eq!(n(&mut g, "GetSystemColor 1"), f64::from(0x0D00FE_u32));
    // The health bar's actor, as the engine tells it.
    assert_eq!(n(&mut g, "GetEnemyHealthTarget"), 0.0);
    g.other.jip9.enemy_target = Some(FormId(MARK));
    assert_eq!(n(&mut g, "GetEnemyHealthTarget"), f64::from(MARK));
}

#[test]
fn immortal_mode_keeps_the_player_alive() {
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "player.SetActorValue Health 100");
    let health = |g: &mut Game| n(g, "player.GetActorValue Health");
    // The mode before the call; stored as a u8 byte (-1 wraps to 255, 3/5/99 read back
    // verbatim) and values outside 1..=2 hold nothing [observed: jip9 immortal.5;
    // w4d_18_immortal_mode_oob: m0=0|neg(prev=0,cur=255)|m3(prev=255,cur=3,hp=200->190)|m99(prev=3,cur=99)].
    assert_eq!(n(&mut g, "ToggleImmortalMode"), 0.0);
    assert_eq!(n(&mut g, "ToggleImmortalMode -1"), 0.0);
    assert_eq!((n(&mut g, "ToggleImmortalMode"), f64::from(g.immortal_mode())), (255.0, 0.0));
    assert_eq!(n(&mut g, "ToggleImmortalMode 3"), 255.0);
    assert_eq!((n(&mut g, "ToggleImmortalMode"), f64::from(g.immortal_mode())), (3.0, 0.0));
    q(&mut g, "player.DamageAV Health 10");
    assert_eq!(health(&mut g), 90.0, "mode 3 holds nothing");
    q(&mut g, "player.RestoreAV Health 10");
    assert_eq!(n(&mut g, "ToggleImmortalMode 99"), 3.0);
    assert_eq!(n(&mut g, "ToggleImmortalMode 5"), 99.0);
    assert_eq!((n(&mut g, "ToggleImmortalMode"), f64::from(g.immortal_mode())), (5.0, 0.0));
    assert_eq!(n(&mut g, "ToggleImmortalMode 2"), 5.0);
    let r = g.hit(FormId::PLAYER_REF, 30.0, HitInfo { location: -1, ..HitInfo::default() });
    assert_eq!((r.dealt, health(&mut g)), (0.0, 100.0), "no damage in mode 2");
    // Wave 4 jip_45_immortal_mode_damageav_bypass [observed]: ToggleImmortalMode 2 also
    // blocks scripted DamageAV Health on the player.
    q(&mut g, "player.DamageAV Health 10");
    assert_eq!(health(&mut g), 100.0, "DamageAV Health blocked in mode 2");
    q(&mut g, "ToggleImmortalMode 1");
    let r = g.hit(FormId::PLAYER_REF, 500.0, HitInfo { location: -1, ..HitInfo::default() });
    assert_eq!((r.dealt, health(&mut g)), (99.0, 1.0), "never below 1 HP in mode 1");
    assert!(!g.is_dead(FormId::PLAYER_REF));
    q(&mut g, "ToggleImmortalMode 0");
    q(&mut g, "player.RestoreAV Health 99");
    q(&mut g, "player.DamageAV Health 10");
    assert_eq!(health(&mut g), 90.0, "DamageAV Health resumes in mode 0");
    let r = g.hit(FormId::PLAYER_REF, 5.0, HitInfo { location: -1, ..HitInfo::default() });
    assert!(r.dealt > 0.0);
}

#[test]
fn interaction_disabled_by_form_type() {
    let f = fixture();
    let mut g = game(&f);
    assert!(!g.player_interaction_disabled(FormId(DOOR_REF)));
    q(&mut g, "SetInteractionDisabledType 1 28");
    assert_eq!((n(&mut g, "GetInteractionDisabledType 28"), n(&mut g, "GetInteractionDisabledType 21")), (1.0, 0.0));
    assert!(g.player_interaction_disabled(FormId(DOOR_REF)));
    assert!(!g.player_interaction_disabled(FormId(ACTI_REF)));
    assert_eq!(g.default_activation_by_hand(FormId(DOOR_REF)).unwrap(), None, "the player's own activation does nothing");
    // A reference closed by SetInteractionDisabled counts too.
    q(&mut g, "ActiRef.SetInteractionDisabled 1");
    assert!(g.player_interaction_disabled(FormId(ACTI_REF)));
    q(&mut g, "SetInteractionDisabledType 0 28");
    assert!(!g.player_interaction_disabled(FormId(DOOR_REF)));
}

#[test]
fn the_position_into_script_variables() {
    let f = fixture();
    let mut g = game(&f);
    let out = s(&mut g, "DoorRef.GetPosEx fX fY fZ\nlet sOut := sv_Construct \"%g %g %g\" fX fY fZ\nSetFunctionValue sOut");
    assert_eq!(out, "10 20 30");
    // GetEditorPosition on a placed ref writes pos + angles in degrees and returns the
    // origin cell; on Player (no editor placement) it returns 0 and leaves all six
    // output variables unchanged [observed: w6_t1_03_pos_ex_and_editor_pos].
    let ed_door = s(
        &mut g,
        "float rX\nfloat rY\nfloat rZ\nref c = DoorRef.GetEditorPosition fX fY fZ rX rY rZ\nlet sOut := sv_Construct \"%i %g %g %g %g %g %g\" c fX fY fZ rX rY rZ\nSetFunctionValue sOut",
    );
    assert_eq!(ed_door, format!("{CELL:08X} 10 20 30 0 0 0"));
    let ed_pc = s(
        &mut g,
        "float rX = 444\nfloat rY = 555\nfloat rZ = 666\nfX = 111\nfY = 222\nfZ = 333\nref c = PlayerREF.GetEditorPosition fX fY fZ rX rY rZ\nlet sOut := sv_Construct \"%i %g %g %g %g %g %g\" c fX fY fZ rX rY rZ\nSetFunctionValue sOut",
    );
    assert_eq!(ed_pc, "00000000 111 222 333 444 555 666");
}

fn node(name: &str, parent: Option<usize>, translation: [f32; 3], rotation: [f32; 3], material: Option<[f32; 9]>) -> SceneNode {
    SceneNode { name: name.into(), parent, translation, rotation, scale: 1.0, flags: 0, material, info: None }
}

#[test]
fn nif_block_materials_and_places() {
    let f = fixture();
    let mut g = game(&f);
    let r = FormId(DOOR_REF);
    // The reference's tree, as the engine tells it once a script asked.
    assert_eq!(n(&mut g, "DoorRef.GetMaterialPropertyValue \"Panel:1\" 7"), 0.0, "no tree yet");
    let mat = [0.1, 0.2, 0.3, 1.0, 0.5, 0.0, 10.0, 1.0, 2.0];
    g.scene.set_tree(r, vec![node("Scene Root", None, [0.0; 3], [0.0; 3], None), node("Arm", Some(0), [0.0, 0.0, 10.0], [0.0, 0.0, 90.0], None), node("Panel:1", Some(1), [5.0, 0.0, 0.0], [0.0; 3], Some(mat))]);
    let get = |g: &mut Game, id: u32| n(g, &format!("DoorRef.GetMaterialPropertyValue \"Panel:1\" {id}"));
    assert!((get(&mut g, 1) - 0.2).abs() < 1e-6);
    assert_eq!((get(&mut g, 8), get(&mut g, 9)), (2.0, 0.0));
    assert_eq!(n(&mut g, "DoorRef.GetMaterialPropertyValue \"Arm\" 7"), 0.0, "a node has no material");
    // A set reads back at once and goes to the engine [documented: the example sets the alpha].
    assert_eq!(n(&mut g, "DoorRef.SetMaterialPropertyValue \"Panel:1\" 7 0.6"), 0.0);
    assert!((get(&mut g, 7) - 0.6).abs() < 1e-6);
    let edits = g.scene.take_edits();
    assert_eq!(edits, [SceneEdit::Material { r, node: "Panel:1".into(), id: 7, v: 0.6 }]);
    q(&mut g, "DoorRef.SetMaterialPropertyValue \"Arm\" 7 0.6\nDoorRef.SetMaterialPropertyValue \"Panel:1\" 9 1");
    assert!(g.scene.take_edits().is_empty(), "no material, no value 9: nothing for the engine");
    // The translation into script variables: to the parent, then in the world (the arm
    // at Z 90, drawn with that rotation's transpose, so X goes to -Y
    // (`SceneNode::rotation`); the reference at 10 20 30, unturned).
    let place = |g: &mut Game, f: &str| s(g, &format!("let fR := DoorRef.{f}\nlet sOut := sv_Construct \"%g %.2f %.2f %.2f\" fR fX fY fZ\nSetFunctionValue sOut"));
    assert_eq!(place(&mut g, "GetNifBlockTranslationAlt \"Panel:1\" fX fY fZ"), "1 5.00 0.00 0.00");
    assert_eq!(place(&mut g, "GetNifBlockTranslationAlt \"Panel:1\" fX fY fZ 1"), "1 10.00 15.00 40.00");
    // A missing block: 0, the variables kept [documented].
    assert_eq!(place(&mut g, "GetNifBlockTranslationAlt \"Panel:1\" fX fY fZ 1\nlet fR := DoorRef.GetNifBlockTranslationAlt \"None\" fX fY fZ 1"), "0 10.00 15.00 40.00");
    // The rotation: to the parent (none), then the world's (the arm's), then intrinsic.
    // No turn gives Y as asin(-0), -0, and `%.2f` keeps its sign [observed: `%f` of -0,
    // #1919; the original's nif-blocks nb.obj.rot reads "90.00 -0.00 90.00"; hypothesis:
    // the same -0 for no turn at all].
    assert_eq!(place(&mut g, "GetNifBlockRotationAlt \"Panel:1\" fX fY fZ"), "1 0.00 -0.00 0.00");
    assert_eq!(place(&mut g, "GetNifBlockRotationAlt \"Panel:1\" fX fY fZ 1"), "1 0.00 -0.00 90.00");
    g.scene.set_tree(r, vec![node("Scene Root", None, [0.0; 3], [30.0, 0.0, 90.0], None)]);
    assert_eq!(place(&mut g, "GetNifBlockRotationAlt \"Scene Root\" fX fY fZ"), "1 30.00 -0.00 90.00");
    // Modes 2 & 3 decompose the transpose (drawn matrix) via ZYX Euler extraction
    // [observed: w6_t1_04_nif_trans_rot_alt]:
    // r0=(-1.9, 0.4, 23.5) -> r2=(1.9, 0.4, -23.5),
    // r1=(89.0, 83.1, 80.7) -> r3=(-89.2, -81.7, 82.3).
    assert_eq!(place(&mut g, "GetNifBlockRotationAlt \"Scene Root\" fX fY fZ 2"), "1 0.00 -30.00 -90.00");
    g.scene.set_tree(r, vec![node("Scene Root", None, [0.0; 3], [-1.9, 0.4, 23.5], None)]);
    assert_eq!(place(&mut g, "GetNifBlockRotationAlt \"Scene Root\" fX fY fZ 2"), "1 1.90 0.39 -23.50");
    g.scene.set_tree(r, vec![node("Scene Root", None, [0.0; 3], [89.0, 83.1, 80.7], None)]);
    assert_eq!(place(&mut g, "GetNifBlockRotationAlt \"Scene Root\" fX fY fZ 3"), "1 -89.17 -81.69 82.28");
    // Missing node returns 0 and leaves variables unchanged, and short aliases work.
    assert_eq!(place(&mut g, "GetNifRotAlt \"Scene Root\" fX fY fZ 0\nlet fR := DoorRef.GetNifBlockRotationAlt \"Missing\" fX fY fZ 0"), "0 89.00 83.10 80.70");
}

#[test]
fn get_angles_between_points_and_get_angle_ex_match_observed_jip() {
    let f = fixture();
    let mut g = game(&f);
    // East: pitch 0, heading -90; Diagonal (10,20,30)->(110,20,130): pitch 45, heading -90
    // [observed: w6_t1_01_angles_between_points].
    let east = s(&mut g, "GetAnglesBetweenPoints 0 0 0 100 0 0 fX fZ\nlet sOut := sv_Construct \"%.2f %.2f\" fX fZ\nSetFunctionValue sOut");
    assert_eq!(east, "0.00 -90.00");
    let diag = s(&mut g, "GetAnglesBetweenPoints 10 20 30 110 20 130 fX fZ\nlet sOut := sv_Construct \"%.2f %.2f\" fX fZ\nSetFunctionValue sOut");
    assert_eq!(diag, "45.00 -90.00");
    // GetAngleEx global (0) vs local (1) on Player rotated (25, 0, 315)
    // [observed: w6_t1_02_angle_ex_and_heading].
    q(&mut g, "PlayerREF.SetAngle X 25\nPlayerREF.SetAngle Z 315");
    g.tick();
    let glob = s(&mut g, "PlayerREF.GetAngleEx fX fY fZ 0\nlet sOut := sv_Construct \"%.2f %.2f %.2f\" fX fY fZ\nSetFunctionValue sOut");
    assert_eq!(glob, "25.00 0.00 315.00");
    let loc = s(&mut g, "PlayerREF.GetAngleEx fX fY fZ 1\nlet sOut := sv_Construct \"%.2f %.2f %.2f\" fX fY fZ\nSetFunctionValue sOut");
    assert_eq!(loc, "-18.25 17.39 42.19");
}
