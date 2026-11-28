//! SUP NVSE batch 5 (#1008; docs/spec-notes/ext-sup5.md) on a synthetic plugin, called
//! from scripts: Excel workbooks, JSON files, file sizes and times, the load-order dump,
//! screenshots, QR codes and the nothing values of the network, the desktop and the
//! clipboard, on a made-up game folder with NVForever's layer beside it. Its own test
//! binary: the file roots are global to the process.

mod common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;
use std::path::PathBuf;

const CELL: u32 = 0x200;
const NPC: u32 = 0x900;
const MARK: u32 = 0x901;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn plugin_bytes() -> Vec<u8> {
    let npcs = [record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]), record(b"NPC_", 0, NPC, &[sub(b"EDID", z("TestNpc")), sub(b"FULL", z("Tester"))])].concat();
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
    let data = [f32b(100.0), f32b(200.0), f32b(0.0), vec![0; 12]].concat();
    let refs = record(b"ACHR", 0, MARK, &[sub(b"EDID", z("MarkRef")), sub(b"NAME", NPC.to_le_bytes().to_vec()), sub(b"DATA", data)]);
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs));
    [tes4(), top(b"NPC_", &npcs), top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat())))].concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Sup5.esm".into()).unwrap()];
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

/// The made-up game folder and NVForever's layer, set once for the process.
fn folders() -> (PathBuf, PathBuf) {
    static DIRS: std::sync::OnceLock<(PathBuf, PathBuf)> = std::sync::OnceLock::new();
    DIRS.get_or_init(|| {
        use fnv_game::mod_files::{set_roots, Roots};
        let base = std::env::temp_dir().join(format!("nvf-sup5-{}", std::process::id()));
        let (game, layer) = (base.join("game"), base.join("layer"));
        // Windows reuses process ids: an earlier run's folder of the same id would hand
        // this run its workbooks (a renamed sheet, an erased one) and fail the Excel test.
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(game.join("Data")).unwrap();
        std::fs::create_dir_all(&layer).unwrap();
        std::fs::write(game.join("nvac.log"), b"0123456789").unwrap();
        std::fs::write(game.join("good.json"), br#"{"name":"Joe","level":7,"inv":[1,"two"],"pos":{"x":1.5},"flag_t":true,"flag_f":false,"empty":null}"#).unwrap();
        std::fs::write(game.join("bad.json"), b"{ nope").unwrap();
        std::fs::write(game.join("notjson.txt"), b"{}").unwrap();
        set_roots(Roots { read: vec![game.join("Data")], write: Some(layer.clone()), ..Roots::default() });
        (game, layer)
    })
    .clone()
}

fn game(f: &Fixture) -> Game<'_> {
    folders();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() });
    g.player_entered(Some(FormId(CELL)));
    g
}

const VARS: &str = "float fA\nstring_var sOut\narray_var aList\narray_var aIn\n";

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("{VARS}Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(g: &mut Game, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(x) => x,
        v => panic!("{expr}: {v:?}"),
    }
}

fn s(g: &mut Game, expr: &str) -> String {
    match q(g, &format!("let sOut := {expr}\nSetFunctionValue sOut")) {
        XVal::Str(s) => s,
        v => panic!("{expr}: {v:?}"),
    }
}

#[test]
fn excel_cells_sheets_and_arrays() {
    let f = fixture();
    let mut g = game(&f);
    let (game_dir, layer) = folders();
    // A missing file: -1 [documented: every Excel page's note].
    assert_eq!(n(&mut g, "ExcelReadFloat \"xlNone\" 0 0 0"), -1.0);
    assert_eq!(n(&mut g, "ExcelGetWorkSheetCount \"xlNone\""), -1.0);
    // ReadString and SetSheetName too, as numbers [observed: sup5 xl.missing.string].
    assert_eq!(n(&mut g, "ExcelReadString \"xlNone\" 0 0 0"), -1.0);
    assert_eq!(n(&mut g, "ExcelSetSheetName \"xlNone\" 0"), -1.0);
    // Writing makes the file and the sheet; 1 when written [documented: ExcelWriteFloat].
    assert_eq!(n(&mut g, "ExcelWriteFloat \"xlBook\" 0 0 0 10"), 1.0);
    assert_eq!(n(&mut g, "XLSWriteString \"xlBook\" 0 4 1 \"MyText\""), 1.0);
    assert_eq!(n(&mut g, "ExcelReadFloat \"xlBook\" 0 0 0"), 10.0);
    assert_eq!(s(&mut g, "ExcelReadString \"xlBook\" 0 4 1"), "MyText");
    // Types [documented: ExcelGetCellValueType]: a float 2, text 3, empty 0.
    assert_eq!(n(&mut g, "ExcelGetCellValueType \"xlBook\" 0 0 0"), 2.0);
    assert_eq!(n(&mut g, "ExcelGetCellValueType \"xlBook\" 0 4 1"), 3.0);
    assert_eq!(n(&mut g, "ExcelGetCellValueType \"xlBook\" 0 1 1"), 0.0);
    assert_eq!(n(&mut g, "ExcelGetRowCount \"xlBook\" 0"), 5.0);
    assert_eq!(n(&mut g, "ExcelGetColCount \"xlBook\" 0"), 2.0);
    // A third sheet makes the second too [hypothesis].
    assert_eq!(n(&mut g, "ExcelWriteFloat \"xlBook\" 2 0 0 1.5"), 1.0);
    assert_eq!(n(&mut g, "ExcelGetWorkSheetCount \"xlBook\""), 3.0);
    assert_eq!(s(&mut g, "ExcelSetSheetName \"xlBook\" 1"), "Sheet2");
    assert_eq!(n(&mut g, "ExcelSetSheetName \"xlBook\" 7"), -1.0);
    // Names cut to 31 [documented: ExcelSetSheetName]; a rename returns 1
    // (see docs/spec-notes/ext-sup5.md §1).
    assert_eq!(n(&mut g, "ExcelSetSheetName \"xlBook\" 1 \"A name far longer than thirty-one letters\""), 1.0);
    assert_eq!(s(&mut g, "ExcelSetSheetName \"xlBook\" 1"), "A name far longer than thirty-o");
    // Erasing [documented: ExcelEraseCell, ExcelEraseSheet]; rows past 65,535 stay
    // rejected with -1 (see docs/spec-notes/ext-sup5.md §1).
    assert_eq!(n(&mut g, "ExcelEraseCell \"xlBook\" 0 4 1"), 1.0);
    assert_eq!(n(&mut g, "ExcelGetCellValueType \"xlBook\" 0 4 1"), 0.0);
    assert_eq!(n(&mut g, "ExcelEraseSheet \"xlBook\" 2"), 1.0);
    assert_eq!(n(&mut g, "ExcelGetCellValueType \"xlBook\" 2 0 0"), 0.0);
    assert_eq!(n(&mut g, "ExcelGetRowCount \"xlBook\" 2"), 0.0);
    assert_eq!(n(&mut g, "ExcelWriteFloat \"xlBook\" 0 70000 0 1"), -1.0);
    assert_eq!(n(&mut g, "ExcelGetWorkSheetCount \"xlBook\""), 3.0);
    // The file is a workbook in NVForever's layer, not in the game folder.
    let bytes = std::fs::read(layer.join("GameRoot").join("xlBook.xls")).unwrap();
    assert_eq!(&bytes[..4], &[0xD0, 0xCF, 0x11, 0xE0]);
    assert!(!game_dir.join("xlBook.xls").exists());
    // An array down a column, a form as its name [documented: ExcelArrayToFile].
    q(&mut g, "let aIn := ar_Construct \"array\"\nar_Append aIn 2\nar_Append aIn \"MyString\"\nar_Append aIn 3.5\nar_Append aIn MarkRef\nExcelArrayToFile \"xlArr\" 0 0 0 aIn");
    assert_eq!(s(&mut g, "ExcelReadString \"xlArr\" 0 3 0"), "Tester");
    assert_eq!(n(&mut g, "ExcelReadFloat \"xlArr\" 0 2 0"), 3.5);
    // Along a row with 1.
    q(&mut g, "let aIn := ar_Construct \"array\"\nar_Append aIn 7\nar_Append aIn 8\nExcelArrayToFile \"xlArr\" 1 0 0 aIn 1");
    assert_eq!(n(&mut g, "ExcelReadFloat \"xlArr\" 1 0 1"), 8.0);
    // Read back, `*Plugin:hex` as the form [documented: ExcelArrayFromFile].
    q(&mut g, "ExcelWriteString \"xlArr\" 0 4 0 \"*Sup5.esm:000900\"");
    let out = s(&mut g, "\"\"\nlet aList := ExcelArrayFromFile \"xlArr\" 0 0 0\nlet sOut := sv_Construct \"%g %g %z %g %z\" (ar_Size aList) (aList[0]) (aList[1]) (aList[2]) (aList[3])");
    assert_eq!(out, "5 2 MyString 3.5 Tester");
    assert_eq!(n(&mut g, "IsFormValid (ExcelArrayFromFile \"xlArr\" 0 0 0)[4]"), 1.0);
    let row = s(&mut g, "\"\"\nlet aList := ExcelArrayFromFile \"xlArr\" 1 0 1\nlet sOut := sv_Construct \"%g %g %g\" (ar_Size aList) (aList[0]) (aList[1])");
    assert_eq!(row, "2 7 8");
    // A file that is no workbook is not overwritten: -1 [documented].
    std::fs::write(game_dir.join("xlText.xls"), b"plain text").unwrap();
    assert_eq!(n(&mut g, "ExcelWriteFloat \"xlText\" 0 0 0 1"), -1.0);
    assert_eq!(std::fs::read(game_dir.join("xlText.xls")).unwrap(), b"plain text");
}

#[test]
fn json_files() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: JSONIsFileValid's table].
    assert_eq!(n(&mut g, "JSONIsFileValid \"good.json\""), 1.0);
    assert_eq!(n(&mut g, "JSONIsFileValid \"bad.json\""), -3.0);
    assert_eq!(n(&mut g, "JSONIsFileValid \"none.json\""), -4.0);
    assert_eq!(n(&mut g, "JSONIsFileValid \"../good.json\""), -5.0);
    assert_eq!(n(&mut g, "JSONIsFileValid \"notjson.txt\""), -6.0);
    // Wave 4D w4d_37_sup_json_and_excel_edges (#3802) [observed]: JSONGetValue always
    // returns a tagged List `[type_code]` or `[type_code, scalar_value]` (-2 missing,
    // 0 null, 1 object, 2 array, 3 string, 4 bool, 5 number).
    let one = s(&mut g, "\"\"\nlet aList := JSONGetValue \"good.json\" \"name\" 0\nlet sOut := sv_Construct \"%g %g %z\" (ar_Size aList) (aList[0]) (aList[1])");
    assert_eq!(one, "2 3 Joe");
    let num_v = s(&mut g, "\"\"\nlet aList := JSONGetValue \"good.json\" \"level\" 0\nlet sOut := sv_Construct \"%g %g %g\" (ar_Size aList) (aList[0]) (aList[1])");
    assert_eq!(num_v, "2 5 7");
    let bool_t = s(&mut g, "\"\"\nlet aList := JSONGetValue \"good.json\" \"flag_t\" 0\nlet sOut := sv_Construct \"%g %g %g\" (ar_Size aList) (aList[0]) (aList[1])");
    assert_eq!(bool_t, "2 4 1");
    let bool_f = s(&mut g, "\"\"\nlet aList := JSONGetValue \"good.json\" \"flag_f\" 0\nlet sOut := sv_Construct \"%g %g %g\" (ar_Size aList) (aList[0]) (aList[1])");
    assert_eq!(bool_f, "2 4 0");
    let null_v = s(&mut g, "\"\"\nlet aList := JSONGetValue \"good.json\" \"empty\" 0\nlet sOut := sv_Construct \"%g %g\" (ar_Size aList) (aList[0])");
    assert_eq!(null_v, "1 0");
    let inv = s(&mut g, "\"\"\nlet aList := JSONGetValue \"good.json\" \"inv\" 1\nlet sOut := sv_Construct \"%g %g\" (ar_Size aList) (aList[0])");
    assert_eq!(inv, "1 2");
    let obj = s(&mut g, "\"\"\nlet aList := JSONGetValue \"good.json\" \"pos\" 0\nlet sOut := sv_Construct \"%g %g\" (ar_Size aList) (aList[0])");
    assert_eq!(obj, "1 1");
    let nested = s(&mut g, "\"\"\nlet aList := JSONGetValue \"good.json\" \"pos.x\" 0\nlet sOut := sv_Construct \"%g %g %g\" (ar_Size aList) (aList[0]) (aList[1])");
    assert_eq!(nested, "2 5 1.5");
    let miss = s(&mut g, "\"\"\nlet aList := JSONGetValue \"good.json\" \"nope\" 0\nlet sOut := sv_Construct \"%g %g\" (ar_Size aList) (aList[0])");
    assert_eq!(miss, "1 -2");
    // A missing file: 1-element list with negative file error code (-4) [observed: sup5
    // json.name & sup_32_json_get_value_missing_file saw size 1 non-string element 0].
    let none = s(&mut g, "\"\"\nlet aList := JSONGetValue \"none.json\" \"name\" 0\nlet sOut := sv_Construct \"%g|%g\" (ar_Size aList) (aList[0])");
    assert_eq!(none, "1|-4");
}

#[test]
fn json_cache_utf8_bom_data_paths_and_showoff_cache_gating() {
    let f = fixture();
    let mut g = game(&f);
    let (game_dir, layer) = folders();
    std::fs::create_dir_all(game_dir.join("Data").join("Config")).unwrap();
    let bom_json = "\u{feff}{\"title\":\"caf\u{e9}\",\"nested\":{\"items\":[10,20]}}";
    std::fs::write(game_dir.join("Data").join("Config").join("sup_utf8.json"), bom_json.as_bytes()).unwrap();

    // Case-insensitive Data\ path + UTF-8 BOM + non-ASCII across SUP and ShowOff JSON.
    assert_eq!(n(&mut g, r#"JSONIsFileValid "Data\Config\sup_utf8.json""#), 1.0);
    assert_eq!(n(&mut g, r#"JSONIsFileValid "data/config/SUP_UTF8.JSON""#), 1.0);
    assert_eq!(n(&mut g, r#"JSONCacheFile "Data\Config\sup_utf8.json" 1"#), 0.0);
    assert_eq!(n(&mut g, r#"JSONIsFileCached "data/config/sup_utf8.json""#), 1.0);
    assert_eq!(n(&mut g, r#"(JSONGetValue "Data\Config\sup_utf8.json" "title" 1)[0]"#), 3.0);
    assert_eq!(s(&mut g, r#"(JSONGetValue "Data\Config\sup_utf8.json" "title" 1)[1]"#), "caf\u{e9}");
    assert_eq!(n(&mut g, r#"(JSONGetValue "Data\Config\sup_utf8.json" "nested.items.1" 1)[0]"#), 5.0);
    assert_eq!(n(&mut g, r#"(JSONGetValue "Data\Config\sup_utf8.json" "nested.items.1" 1)[1]"#), 20.0);

    // Unified SUP JSON cache: cached writes via nvse_json are visible to JSONGetValue(iCached=1),
    // while uncached JSONGetValue(iCached=0) still reads disk.
    assert_eq!(n(&mut g, r#"JSONSetValueFloat "Data\Config\sup_utf8.json" "hp" 99 1"#), 1.0);
    assert_eq!(n(&mut g, r#"(JSONGetValue "Data\Config\sup_utf8.json" "hp" 1)[0]"#), 5.0);
    assert_eq!(n(&mut g, r#"(JSONGetValue "Data\Config\sup_utf8.json" "hp" 1)[1]"#), 99.0);
    assert_eq!(n(&mut g, r#"(JSONGetValue "Data\Config\sup_utf8.json" "hp" 0)[0]"#), -2.0);
    // JSONCacheFileFromString feeds JSONGetValue(iCached=1) through the same cache.
    assert_eq!(n(&mut g, r#"JSONCacheFileFromString (JSONToString "Data\Config\sup_utf8.json") "mem.json""#), 0.0);
    assert_eq!(n(&mut g, r#"(JSONGetValue "mem.json" "hp" 1)[1]"#), 99.0);
    assert_eq!(n(&mut g, r#"JSONCloseFile "mem.json" 1"#), 0.0);
    assert_eq!(n(&mut g, r#"JSONCloseFile "Data\Config\sup_utf8.json" 1"#), 0.0);
    assert_eq!(n(&mut g, r#"JSONIsFileCached "Data\Config\sup_utf8.json""#), 0.0);

    // ShowOff ReadFromJSON / WriteToJSON: UTF-8 BOM + non-ASCII + useCache gating + layer persistence.
    assert_eq!(s(&mut g, r#"ReadFromJSON "Data\Config\sup_utf8.json" "/title" 0 1"#), "caf\u{e9}");
    assert_eq!(n(&mut g, r#"ReadFromJSON "Data\Config\sup_utf8.json" "/nested/items/1" 0 1"#), 20.0);
    // Cached write without saving stays in cache; uncached read still sees original disk file.
    assert_eq!(n(&mut g, r#"WriteToJSON 55 "Data\Config\sup_utf8.json" "/nested/items/0" 0 1 0"#), 1.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "Data\Config\sup_utf8.json" "/nested/items/0" 0 1"#), 55.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "Data\Config\sup_utf8.json" "/nested/items/0" 0 0"#), 10.0);
    // Uncached saved write persists to NVForever's GameRoot layer (never the game folder) and
    // drops any stale ShowOff cache entry.
    assert_eq!(n(&mut g, r#"WriteToJSON 77 "Data\Config\sup_utf8.json" "/nested/items/0" 0 0 1"#), 1.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "Data\Config\sup_utf8.json" "/nested/items/0" 0 0"#), 77.0);
    assert_eq!(n(&mut g, r#"ReadFromJSON "Data\Config\sup_utf8.json" "/nested/items/0" 0 1"#), 77.0);
    assert!(layer.join("GameRoot").join("Data").join("Config").join("sup_utf8.json").exists());
    assert_eq!(std::fs::read(game_dir.join("Data").join("Config").join("sup_utf8.json")).unwrap(), bom_json.as_bytes());
    // Escaping path is refused even when saveFile is 0.
    assert_eq!(n(&mut g, r#"WriteToJSON 1 "..\outside.json" "/a" 0 1 0"#), 0.0);
}

#[test]
fn file_sizes_times_and_the_load_order() {
    let f = fixture();
    let mut g = game(&f);
    let (game_dir, layer) = folders();
    // [documented: GetFileSize, in bytes from the game folder]; missing -1 (see docs/spec-notes/ext-sup5.md §3).
    assert_eq!(n(&mut g, "GetFileSize \"nvac.log\""), 10.0);
    assert_eq!(n(&mut g, "GetFileSize \"none.log\""), -1.0);
    // "DD/MM/YYYY  HH:MM" / "MM/DD/YYYY  HH:MM" [observed: 2026-11-23,
    // GSDocMitchellHouse (0x00103DF9), `sup_33_file_time_str_formats` and
    // `sup_19_getfiletime_keys`, SUP NVSE 855].
    let t = s(&mut g, "GetFileTimeSTR \"nvac.log\" 2 0");
    assert_eq!(t.len(), 17, "{t}");
    assert_eq!((&t[2..3], &t[5..6], &t[10..12], &t[14..15]), ("/", "/", "  ", ":"));
    assert_eq!(s(&mut g, "GetFileTimeSTR \"none.log\" 2 0"), "");
    // 5-element StringMap with zero-padded String values on existing file, empty
    // StringMap (size 0) on missing file [observed: 2026-11-23, GSDocMitchellHouse
    // (0x00103DF9), `sup_19_getfiletime_keys`, SUP NVSE 855].
    assert_eq!(n(&mut g, "ar_Size (GetFileTime \"nvac.log\" 2)"), 5.0);
    assert_eq!(n(&mut g, "ar_Size (GetFileTime \"none.log\" 2)"), 0.0);
    assert_eq!(s(&mut g, "typeof (GetFileTime \"nvac.log\" 2)"), "StringMap");
    assert_eq!(s(&mut g, "typeof ((GetFileTime \"nvac.log\" 2)[\"Year\"])"), "String");
    assert!(s(&mut g, "(GetFileTime \"nvac.log\" 2)[\"Year\"]").parse::<i32>().unwrap() >= 2026);
    // The values, in the computer's time zone: the file is given a written time of
    // 2026-07-15 12:00:30 UTC. In mid-month the month reads "07" in every time zone, so
    // the two orders tell apart whatever the zone.
    let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_784_116_830);
    std::fs::File::options().write(true).open(game_dir.join("nvac.log")).unwrap().set_modified(at).unwrap();
    let local = jiff::Timestamp::try_from(at).unwrap().to_zoned_datetime(jiff::tz::TimeZone::system());
    let keys = ["Day", "Hour", "Minute", "Month", "Year"];
    let fields: Vec<String> = keys.iter().map(|k| s(&mut g, &format!("(GetFileTime \"nvac.log\" 2)[\"{k}\"]"))).collect();
    let want = vec![
        format!("{:02}", local.day()),
        format!("{:02}", local.hour()),
        format!("{:02}", local.minute()),
        format!("{:02}", local.month()),
        format!("{:04}", local.year()),
    ];
    assert_eq!(fields, want);
    assert_eq!(s(&mut g, "(GetFileTime \"nvac.log\" 2)[\"Month\"]"), "07");
    let (dmy, mdy) = (s(&mut g, "GetFileTimeSTR \"nvac.log\" 2 0"), s(&mut g, "GetFileTimeSTR \"nvac.log\" 2 1"));
    assert_eq!(dmy, local.strftime("%d/%m/%Y  %H:%M").to_string());
    assert_eq!(mdy, local.strftime("%m/%d/%Y  %H:%M").to_string());
    assert_eq!((&dmy[3..5], &mdy[0..2], &dmy[6..10]), ("07", "07", "2026"), "{dmy} {mdy}");
    // A request outside 0-2 gives nothing [design].
    assert_eq!(n(&mut g, "ar_Size (GetFileTime \"nvac.log\" 3)"), 0.0);
    assert_eq!(s(&mut g, "GetFileTimeSTR \"nvac.log\" 3 0"), "");
    // The load order into NVForever's SUPNVSE.log [documented: DumpLoadOrder].
    q(&mut g, "DumpLoadOrder");
    let log = std::fs::read_to_string(layer.join("GameRoot").join("SUPNVSE.log")).unwrap();
    assert!(log.contains("00 Sup5.esm"), "{log}");
    assert!(!game_dir.join("SUPNVSE.log").exists());
}

#[test]
fn the_network_the_desktop_and_the_clipboard_give_nothing() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: the pages' failure text; decision: no network].
    assert_eq!(s(&mut g, "GetNexusModVersion 73160"), "ERROR");
    assert_eq!(s(&mut g, "GetNexusModVersionAPI 73160"), "ERROR");
    assert_eq!(s(&mut g, "GetGitHubLatestReleaseTag \"x/y\""), "ERROR");
    let multi = s(&mut g, "\"\"\nlet aIn := ar_Construct \"array\"\nar_Append aIn 73160\nar_Append aIn 58277\nlet aList := GetNexusModVersionMulti aIn\nlet sOut := sv_Construct \"%g %z\" (ar_Size aList) (aList[1])");
    assert_eq!(multi, "2 ERROR");
    assert_eq!(n(&mut g, "GetNVSEOnlineVersion"), 0.0);
    assert_eq!(n(&mut g, "IsNexusAPIKeyValid"), 0.0);
    assert_eq!(s(&mut g, "GetPastebinRawTextString \"j1SY2TFH\""), "");
    assert_eq!(n(&mut g, "ar_Size (GetPastebinRawTextArray \"j1SY2TFH\")"), 0.0);
    // [decision: no programs, folders or pages opened].
    assert_eq!(n(&mut g, "IsProgramRunning \"steam.exe\""), 0.0);
    q(&mut g, "OpenNexusModPage 73160\nOpenNexusUserPage 1\nOpenFolder \"C:\\\\\"");
    // [decision: no clipboard, fonts or language].
    q(&mut g, "StringToClipboard \"x\"");
    assert_eq!(s(&mut g, "ClipboardToString"), "");
    assert_eq!(s(&mut g, "GetUserWindowsLanguage"), "");
    assert_eq!(n(&mut g, "ar_Size (GetWindowsFonts)"), 0.0);
}

#[test]
fn screenshots_and_qr_codes() {
    let f = fixture();
    let mut g = game(&f);
    let (_, layer) = folders();
    // Requests for the engine [documented: CaptureScreenshot's default name and formats].
    q(&mut g, "Screenshot\nCaptureScreenshot \"MyScreen\" 1\nCaptureScreenshotAlt \"MyMod\" \"MyImage\" 0 1000 0 1200 6 0 50");
    let shots: Vec<(String, bool, u8)> = g.ui.screenshots.iter().map(|r| (r.path.clone(), r.root, r.format)).collect();
    assert_eq!(shots, vec![("a_Screenshot.jpg".to_string(), true, 0), ("MyScreen.bmp".to_string(), true, 1), ("textures\\SUPScreenshots\\MyMod\\MyImage.dds".to_string(), false, 6)]);
    assert_eq!(g.ui.screenshots[2].crop, Some([0.0, 1000.0, 0.0, 1200.0]));
    // Deleting [documented: DeleteScreenshot, -1 missing, -2 outside its folder].
    assert_eq!(n(&mut g, "DeleteScreenshot \"MyMod\" \"MyImage.dds\""), -1.0);
    assert_eq!(n(&mut g, "DeleteScreenshot \"..\\\\x\" \"MyImage.dds\""), -2.0);
    assert!(g.mod_files.write_bytes("textures\\SUPScreenshots\\MyMod\\MyImage.dds", b"DDS "));
    assert!(layer.join("textures/SUPScreenshots/MyMod/MyImage.dds").exists());
    assert_eq!(n(&mut g, "DeleteScreenshot \"MyMod\" \"MyImage.dds\""), 1.0);
    assert!(!layer.join("textures/SUPScreenshots/MyMod/MyImage.dds").exists());
    // A QR code as a PNG, the folder made [documented: TextToQRCode].
    q(&mut g, "TextToQRCode \"QrCodes\\MyQrCode\" \"I love bacon\" 0");
    let png = std::fs::read(layer.join("GameRoot/QrCodes/MyQrCode.png")).unwrap();
    assert_eq!(&png[1..4], b"PNG");
    // The original's size for this text [observed: sup5 qr.where].
    assert_eq!(png.len(), 268_590);
    // Nothing drawn [decision].
    q(&mut g, "\"\"\nlet aIn := ar_Construct \"array\"\nDrawGraphicsToDDSTextureAndSave \"MyMod\" \"T.dds\" aIn 500 500");
    // No dialog: "" [decision].
    assert_eq!(s(&mut g, "OpenFolderDialogBox MarkRef"), "");
}

struct TimeFixture(PathBuf);

impl Drop for TimeFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn failed_zone_discovery_uses_utc_and_warns_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tracing_subscriber::{layer::Context, prelude::*, Layer};

    const CHILD: &str = "NVF_SUP5_FAILED_ZONE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let missing = std::env::temp_dir().join(format!("nvf-missing-zone-{}.tzif", std::process::id()));
        assert!(!missing.exists());
        let mut tz = std::ffi::OsString::from(":");
        tz.push(missing);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "failed_zone_discovery_uses_utc_and_warns_once"])
            .env(CHILD, "1")
            .env("TZ", tz)
            .env_remove("TZDIR")
            .status()
            .unwrap();
        assert!(status.success(), "invalid-zone child: {status}");
        return;
    }

    struct Warnings;
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    impl<S: tracing::Subscriber> Layer<S> for Warnings {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            let meta = event.metadata();
            if meta.target() == "fnv_game::local_time" && *meta.level() == tracing::Level::WARN {
                COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    use jiff::{civil::Time, tz::TimeZone, Timestamp};
    assert!(TimeZone::try_system().is_err(), "the real zone lookup must fail");
    let _subscriber = tracing::subscriber::set_default(tracing_subscriber::registry().with(Warnings));
    let file = TimeFixture(folders().0.join("failed-zone-time.log"));
    let instant: Timestamp = "2024-03-10T07:00:30Z".parse().unwrap();
    let modified = std::time::SystemTime::try_from(instant).unwrap();
    std::fs::File::create(&file.0).unwrap().set_modified(modified).unwrap();
    assert_eq!(std::fs::metadata(&file.0).unwrap().modified().unwrap(), modified);

    // Clock fields wrap at midnight; accept any matching value between samples.
    let in_window = |before: i64, value: i64, after: i64, cycle: i64| {
        (value - before.rem_euclid(cycle)).rem_euclid(cycle) <= after - before
    };
    let f = fixture();
    for _ in 0..2 {
        let mut g = game(&f);
        let before = Timestamp::now().as_second().div_euclid(3600);
        let hour = n(&mut g, "(GetUserTime)[\"Hour\"]");
        let after = Timestamp::now().as_second().div_euclid(3600);
        assert!((0.0..24.0).contains(&hour) && hour.fract() == 0.0);
        assert!(in_window(before, hour as i64, after, 24), "{hour}: UTC hour outside sampled interval {before}..={after}");
        let before = Timestamp::now().as_second();
        let stamp = s(&mut g, "GetTimeStamp");
        let after = Timestamp::now().as_second();
        let t = Time::strptime("%H-%M-%S", &stamp).unwrap();
        let seconds = i64::from(t.hour()) * 3600 + i64::from(t.minute()) * 60 + i64::from(t.second());
        assert!(in_window(before, seconds, after, 86_400), "{stamp}: UTC stamp outside sampled interval {before}..={after}");
        let keys = ["Day", "Hour", "Minute", "Month", "Year"];
        let fields: Vec<_> = keys.iter().map(|k| s(&mut g, &format!("(GetFileTime \"failed-zone-time.log\" 2)[\"{k}\"]"))).collect();
        assert_eq!(fields, ["10", "07", "00", "03", "2024"]);
        assert_eq!(s(&mut g, "GetFileTimeSTR \"failed-zone-time.log\" 2"), "10/03/2024  07:00");
    }
    assert_eq!(COUNT.load(Ordering::Relaxed), 1, "one warning per process, not per call or Game");
}

#[cfg(windows)]
#[test]
fn unrepresentable_file_mtime_returns_empty_script_results() {
    use jiff::Timestamp;
    use std::time::{Duration, SystemTime};

    let f = fixture();
    let mut g = game(&f);
    let path = TimeFixture(folders().0.join("unrepresentable-time.log"));
    let file = std::fs::File::create(&path.0).unwrap();
    let size = |g: &mut Game| n(g, "ar_Size (GetFileTime \"unrepresentable-time.log\" 2)");
    let text = |g: &mut Game| s(g, "GetFileTimeSTR \"unrepresentable-time.log\" 2");
    let outside = SystemTime::try_from(Timestamp::MAX).unwrap() + Duration::from_secs(1);
    file.set_modified(outside).unwrap();
    let actual = std::fs::metadata(&path.0).unwrap().modified().unwrap();
    assert_eq!(actual, outside, "the filesystem must retain the out-of-range value");
    assert!(Timestamp::try_from(actual).is_err());
    assert_eq!(size(&mut g), 0.0);
    assert_eq!(text(&mut g), "");

    let ordinary: Timestamp = "2024-03-10T07:00:30Z".parse().unwrap();
    file.set_modified(SystemTime::try_from(ordinary).unwrap()).unwrap();
    assert_eq!(size(&mut g), 5.0, "the same admitted file has representable metadata");
    assert!(!text(&mut g).is_empty());
}
