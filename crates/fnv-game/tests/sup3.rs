//! SUP NVSE batch 3 (#1008; docs/spec-notes/ext-sup3.md) on a synthetic plugin, called
//! from scripts: the fly (TFC) camera's place, the angles from it to a point, the
//! positions at an angle and distance, an axis argument beside a reference named `Z`,
//! and the INI readers on a made-up game folder. Its own test binary: the file roots are
//! global to the process.

mod common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CELL: u32 = 0x200;
const NPC: u32 = 0x900;
const MARK: u32 = 0x901;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn placed(sig: &[u8; 4], fid: u32, edid: &str, base: u32, pos: [f32; 3]) -> Vec<u8> {
    let data = [f32b(pos[0]), f32b(pos[1]), f32b(pos[2]), vec![0; 12]].concat();
    record(sig, 0, fid, &[sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn plugin_bytes() -> Vec<u8> {
    let npcs = [record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]), record(b"NPC_", 0, NPC, &[sub(b"EDID", z("TestNpc"))])].concat();
    let stat = record(b"STAT", 0, 0x910, &[sub(b"EDID", z("TestStatic"))]);
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
    // An actor at (100, 200, 0), and a static placed with the editor ID `Z`, as
    // OldWorldBlues.esm and LonesomeRoad.esm each have one.
    let refs = [placed(b"ACHR", MARK, "MarkRef", NPC, [100.0, 200.0, 0.0]), placed(b"REFR", 0x902, "Z", 0x910, [0.0, 0.0, 0.0])].concat();
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs));
    [
        tes4(),
        top(b"NPC_", &npcs),
        top(b"STAT", &stat),
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
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Sup3.esm".into()).unwrap()];
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

const VARS: &str = "float fA\nfloat fB\nfloat fX\nfloat fY\nfloat fZ\nstring_var sOut\narray_var aList\n";

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("{VARS}Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(g: &mut Game, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(x) => x,
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
fn the_fly_camera_place() {
    let f = fixture();
    let mut g = game(&f);
    // Never used: all 0 [observed: sup3 cam.off.*].
    assert_eq!((n(&mut g, "GetTFCPos X"), n(&mut g, "GetTFCAngle Z")), (0.0, 0.0));
    // Turned on: the player's eye [observed: sup3 cam.on.pos].
    q(&mut g, "ToggleFlyCam");
    assert_eq!(n(&mut g, "GetTFC"), 1.0);
    let feet = n(&mut g, "player.GetPos Z");
    assert_eq!(n(&mut g, "GetTFCPos Z") - feet, 118.0);
    // Setters return 0 and the value holds; angles are not wrapped [observed: sup3
    // cam.setpos, cam.setangle.wide].
    assert_eq!(n(&mut g, "SetTFCPos Z 25"), 0.0);
    assert_eq!(n(&mut g, "GetTFCPos Z"), 25.0);
    assert!(g.ui.camera.tfc_set, "the engine is told");
    q(&mut g, "SetTFCAngle Z 400\nSetTFCAngle X -100");
    assert_eq!((n(&mut g, "GetTFCAngle Z"), n(&mut g, "GetTFCAngle X")), (400.0, -100.0));
    // The raw rotation: X and Z in radians, Y always 0 [observed: sup3 cam.setrot.*,
    // 2026-11-24 w6_t1_12_sup_tfc_and_mouse & w6_t1_12b_sup_tfc_rot_set].
    q(&mut g, "SetTFCRot X 0.5\nSetTFCRot Y 1");
    assert!((n(&mut g, "GetTFCAngle X") - 28.6479).abs() < 1e-4);
    assert_eq!((n(&mut g, "GetTFCRot X"), n(&mut g, "GetTFCRot Y"), n(&mut g, "GetTFCAngle Z")), (0.5, 0.0, 400.0));
    // SetTFCAngleEx 15 90 -> GetTFCRotEx returns (0.2618, 1.5708) [observed: w6_t1_12_sup_tfc_and_mouse].
    q(&mut g, "SetTFCAngleEx 15 90");
    let rot_from_ang = s(&mut g, "GetTFCRotEx fA fB\nlet sOut := sv_Construct \"%.4f %.4f\" fA fB\nSetFunctionValue sOut");
    assert_eq!(rot_from_ang, "0.2618 1.5708");
    // SetTFCRotEx 0.5 1.0 -> GetTFCRotEx (0.5000, 1.0000), GetTFCAngleEx (28.65, 57.30),
    // GetTFCRot X/Y/Z (0.5000, 0.0000, 1.0000) [observed: w6_t1_12b_sup_tfc_rot_set].
    q(&mut g, "SetTFCRotEx 0.5 1.0");
    let rot_ex = s(
        &mut g,
        "GetTFCRotEx fA fB\nGetTFCAngleEx fX fY\nlet sOut := sv_Construct \"%.4f %.4f %.2f %.2f %.4f %.4f %.4f\" fA fB fX fY (GetTFCRot X) (GetTFCRot Y) (GetTFCRot Z)\nSetFunctionValue sOut",
    );
    assert_eq!(rot_ex, "0.5000 1.0000 28.65 57.30 0.5000 0.0000 1.0000");
    // SetTFCRot Z sets Z in radians [hypothesis: symmetric with GetTFCRot Z & SetTFCRotEx].
    q(&mut g, "SetTFCRot Z 1.5");
    assert!((n(&mut g, "GetTFCRot Z") - 1.5).abs() < 1e-6);
    assert!((n(&mut g, "GetTFCAngle Z") - 1.5_f64.to_degrees()).abs() < 1e-6);
    // The Ex forms write the script's variables [observed: w6_t1_12_sup_tfc_and_mouse].
    q(&mut g, "SetTFCPosEx 1 2 3\nSetTFCAngleEx 10 20");
    let out = s(&mut g, "GetTFCPosEx fX fY fZ\nGetTFCAngleEx fA fB\nlet sOut := sv_Construct \"%g %g %g %g %g\" fX fY fZ fA fB\nSetFunctionValue sOut");
    assert_eq!(out, "1 2 3 10 20");
    // Looking at points from (1, 2, 3) [observed: sup3 cam.calc.*].
    assert_eq!(n(&mut g, "GetCalculatedAngleZForTFCCamera 1 -98"), -180.0);
    assert_eq!(n(&mut g, "GetCalculatedAngleZForTFCCamera 101 2"), 90.0);
    assert_eq!(n(&mut g, "GetCalculatedAngleXForTFCCamera 101 2 103"), -45.0);
    // Turned off, it keeps its place [observed: sup3 cam.off2.*].
    q(&mut g, "ToggleFlyCam");
    assert_eq!(n(&mut g, "GetTFCPos Z"), 3.0);
}

#[test]
fn an_axis_beside_a_reference_named_z() {
    let f = fixture();
    let mut g = game(&f);
    // `Z` is the axis for a parameter the catalogue types as one, here `X/Z`, though the
    // load order has a reference with that editor ID [observed: sup3 with the DLCs].
    q(&mut g, "SetTFCAngle Z 45\nSetTFCPos Z 7");
    assert_eq!((n(&mut g, "GetTFCAngle Z"), n(&mut g, "GetTFCPos Z")), (45.0, 7.0));
}

#[test]
fn positions_at_an_angle_and_distance() {
    let f = fixture();
    let mut g = game(&f);
    // From MarkRef at (100, 200): 0 north, 90 east [documented: GetCalculatedPos].
    let at = |g: &mut Game, a: i32| s(g, &format!("MarkRef.GetCalculatedPos {a} 25 fX fY\nlet sOut := sv_Construct \"%.1f %.1f\" fX fY\nSetFunctionValue sOut"));
    assert_eq!((at(&mut g, 0), at(&mut g, 90)), ("100.0 225.0".to_string(), "125.0 200.0".to_string()));
    let alt = s(&mut g, "GetCalculatedPosAlt 180 25 fX fY 10 20\nlet sOut := sv_Construct \"%.1f %.1f\" fX fY\nSetFunctionValue sOut");
    assert_eq!(alt, "10.0 -5.0");
}

/// A made-up game folder: `Data\NVSE\Plugins\Test.ini` with a byte order mark, a
/// section line with a space before it and keys out of order.
fn game_folder() {
    use fnv_game::mod_files::{set_roots, Roots};
    let dir = std::env::temp_dir().join(format!("nvf-sup3-{}", std::process::id()));
    let plugins = dir.join("Data/NVSE/Plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    let text = "\u{feff}[Settings]\r\nbB = 1\r\nfSpeed = 2.5 ; a comment\r\naFirst = x\r\n ; a note\r\n [Other]\r\nsName = Some text\r\n";
    std::fs::write(plugins.join("Test.ini"), text.as_bytes()).unwrap();
    set_roots(Roots { read: vec![dir.join("Data")], ..Roots::default() });
}

#[test]
fn the_ini_readers() {
    game_folder();
    let f = fixture();
    let mut g = game(&f);
    let file = "\"Data/NVSE/Plugins/Test.ini\"";
    // [documented: ReadINIFloatFromFile; observed: sup3 ini.float.*: the byte order mark,
    // the section in any case].
    assert_eq!(n(&mut g, &format!("ReadINIFloatFromFile {file} \"settings\" \"bB\"")), 1.0);
    assert_eq!(n(&mut g, &format!("ReadINIFloatFromFile {file} \"Settings\" \"fSpeed\"")), 2.5);
    assert_eq!(n(&mut g, &format!("ReadINIFloatFromFile {file} \"Settings\" \"bNone\"")), 0.0);
    assert_eq!(n(&mut g, &format!("IsNumberNAN (ReadINIFloatFromFileAlt {file} \"Settings\" \"bNone\")")), 1.0);
    assert_eq!(n(&mut g, "IsNumberNAN (ReadINIFloatFromFileAlt \"Data\\NVSE\\Plugins\\None.ini\" \"Settings\" \"bB\")"), 1.0);
    assert_eq!(s(&mut g, &format!("let sOut := ReadINIStringFromFile {file} \"Other\" \"sName\"\nSetFunctionValue sOut")), "Some text");
    // Keys and sections sorted [observed: sup3 ini.keys].
    let list = |g: &mut Game, expr: &str| s(g, &format!("let aList := {expr}\nlet sOut := sv_Construct \"%g %z %z\" (ar_Size aList) (aList[0]) (aList[(ar_Size aList) - 1])\nSetFunctionValue sOut"));
    assert_eq!(list(&mut g, &format!("ReadINISectionKeysFromFile {file} \"SETTINGS\"")), "3 aFirst fSpeed");
    assert_eq!(list(&mut g, &format!("ReadINISectionsFromFile {file}")), "2 Other Settings");
    // No file: no array [observed: sup3 ini.sections.nofile].
    assert_eq!(n(&mut g, "ar_Size (ReadINISectionsFromFile \"Data/NVSE/Plugins/None.ini\")"), -1.0);
    // Not outside the game folder [decision].
    assert_eq!(n(&mut g, "ReadINIFloatFromFile \"../Data/NVSE/Plugins/Test.ini\" \"Settings\" \"bB\""), 0.0);
}
