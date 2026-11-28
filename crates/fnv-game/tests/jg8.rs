//! JohnnyGuitar NVSE batch 8 (#1007; docs/spec-notes/ext-jg8.md) on a synthetic plugin:
//! the event handlers fired by the game, file hashes, texture and bitmap headers, arrays,
//! the system colours and the sleep/wait menu's state.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::nvse_jg8::{bmp_pixel, dds_header, md5, sha1};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const XMARKER: u32 = 0x3B;
const GLOB: u32 = 0x900;
const QUST: u32 = 0x901;
const REPU: u32 = 0x902;
const MISC_A: u32 = 0x903;
const MISC_B: u32 = 0x904;
const ACTI_TRAP: u32 = 0x905;
const QUST_OTHER: u32 = 0x906;
const PERK_A: u32 = 0x907;
const PERK_B: u32 = 0x908;
const FLST_QP: u32 = 0x909;
const CELL_A: u32 = 0x200;
const TRAP_REF: FormId = FormId(0x305);
const MISC_REF: FormId = FormId(0x306);
const NPC_BASE: u32 = 0x910;
const NPC_OTHER: u32 = 0x911;
const FLST_BASE: u32 = 0x912;
const FLST_REF: u32 = 0x913;
const CELL_ID: u32 = 0x920;
const NPC_REF: u32 = 0x921;

fn edid(sig: &[u8; 4], fid: u32, name: &str) -> Vec<u8> {
    record(sig, 0, fid, &[sub(b"EDID", z(name))])
}

fn placed_obj(id: FormId, edid: &str, base: u32) -> Vec<u8> {
    let data: Vec<u8> = [0.0f32; 6].iter().flat_map(|v| v.to_le_bytes()).collect();
    record(b"REFR", 0, id.0, &[sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn plugin_bytes() -> Vec<u8> {
    let glob = record(b"GLOB", 0, GLOB, &[sub(b"EDID", z("TestGlobal")), sub(b"FNAM", vec![b'f']), sub(b"FLTV", 0f32.to_le_bytes().to_vec())]);
    let flst_qp = record(
        b"FLST",
        0,
        FLST_QP,
        &[
            sub(b"EDID", z("QuestPerkList")),
            sub(b"LNAM", QUST.to_le_bytes().to_vec()),
            sub(b"LNAM", PERK_A.to_le_bytes().to_vec()),
        ],
    );
    let flst_base = record(b"FLST", 0, FLST_BASE, &[sub(b"EDID", z("ListNpcBase")), sub(b"LNAM", NPC_BASE.to_le_bytes().to_vec())]);
    let flst_ref = record(b"FLST", 0, FLST_REF, &[sub(b"EDID", z("ListNpcRef")), sub(b"LNAM", NPC_REF.to_le_bytes().to_vec())]);
    let refs = [placed_obj(TRAP_REF, "TrapRef", ACTI_TRAP), placed_obj(MISC_REF, "MiscRef", MISC_A)].concat();
    let cell_a = [
        record(b"CELL", 0, CELL_A, &[sub(b"EDID", z("TestCellA"))]),
        group(CELL_A.to_le_bytes(), 6, &group(CELL_A.to_le_bytes(), 9, &refs)),
    ]
    .concat();
    let npc_ref = record(b"ACHR", 0, NPC_REF, &[sub(b"EDID", z("TestNpcRef")), sub(b"NAME", NPC_BASE.to_le_bytes().to_vec())]);
    let cell_npc = [
        record(b"CELL", 0, CELL_ID, &[sub(b"EDID", z("TestCell"))]),
        group(CELL_ID.to_le_bytes(), 6, &group(CELL_ID.to_le_bytes(), 8, &npc_ref)),
    ]
    .concat();
    [
        tes4(),
        top(b"NPC_", &[edid(b"NPC_", 0x7, "Player"), edid(b"NPC_", NPC_BASE, "TestNpcBase"), edid(b"NPC_", NPC_OTHER, "OtherNpcBase")].concat()),
        top(b"STAT", &edid(b"STAT", XMARKER, "XMarker")),
        top(b"ACTI", &edid(b"ACTI", ACTI_TRAP, "TestTrap")),
        top(b"GLOB", &glob),
        top(b"QUST", &[edid(b"QUST", QUST, "TestQuest"), edid(b"QUST", QUST_OTHER, "OtherQuest")].concat()),
        top(b"PERK", &[edid(b"PERK", PERK_A, "TestPerk"), edid(b"PERK", PERK_B, "OtherPerk")].concat()),
        top(b"FLST", &flst_qp),
        top(b"REPU", &edid(b"REPU", REPU, "TestReputation")),
        top(b"MISC", &[edid(b"MISC", MISC_A, "ZuluThing"), edid(b"MISC", MISC_B, "AlphaThing")].concat()),
        top(b"FLST", &[flst_base, flst_ref].concat()),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell_a, cell_npc].concat()))),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Jg8.esm".into()).unwrap()];
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

fn s(g: &mut Game, body: &str) -> String {
    match q(g, body) {
        XVal::Str(s) => s,
        v => panic!("{body}: {v:?}"),
    }
}

/// TestGlobal after the event: each handler adds what it was given.
fn global(g: &mut Game) -> f64 {
    f64::from(g.world.global(FormId(GLOB)).unwrap())
}

#[test]
fn handlers_hear_the_game() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: SetJohnnyOnFailQuestEventHandler, the quest as the argument; a filter
    // that does not match keeps the handler quiet].
    q(&mut g, "SetJohnnyOnFailQuestEventHandler 1 (begin function {ref rQ}\nif rQ == TestQuest\nlet TestGlobal := TestGlobal + 1\nendif\nend) 0 TestQuest");
    q(&mut g, "SetJohnnyOnFailQuestEventHandler 1 (begin function {ref rQ}\nlet TestGlobal := TestGlobal + 100\nend) 0 TestReputation");
    q(&mut g, "FailQuest TestQuest");
    assert_eq!(global(&mut g), 1.0);
    // [documented: SetJohnnyOnSleepWaitEventHandler, 0 both, 1 wait, 2 rest].
    q(&mut g, "let TestGlobal := 0\nSetJohnnyOnSleepWaitEventHandler 1 (begin function {int iMode}\nlet TestGlobal := TestGlobal + iMode\nend) 0 0");
    q(&mut g, "SetJohnnyOnSleepWaitEventHandler 1 (begin function {int iMode}\nlet TestGlobal := TestGlobal + 10\nend) 0 2");
    g.begin_rest(false, 2);
    assert_eq!(global(&mut g), 1.0);
    g.begin_rest(true, 8);
    assert_eq!(global(&mut g), 1.0 + 2.0 + 10.0);
    // [documented: SetJohnnyOnLimbGoneEventHandler, (actor, limb), the limb filter].
    q(&mut g, "let TestGlobal := 0\nSetJohnnyOnLimbGoneEventHandler 1 (begin function {ref rA, int iLimb}\nlet TestGlobal := TestGlobal + iLimb\nend) 0 0 1");
    g.dismember(FormId::PLAYER_REF, &[(1, false), (3, false)], None);
    assert_eq!(global(&mut g), 1.0);
    // [observed 2026-11-24: w6t2p3_12_rep_fame_infamy: (reputation, fame, infamy)].
    q(
        &mut g,
        "let TestGlobal := 0\n\
         SetOnReputationChangeEventHandler 1 (begin function {ref rR, float fFame, float fInf}\n\
         PlayerREF.AuxVarSetFlt \"*rep_ok\" (rR == TestReputation)\n\
         PlayerREF.AuxVarSetFlt \"*rep_fame\" fFame\n\
         PlayerREF.AuxVarSetFlt \"*rep_inf\" fInf\n\
         let TestGlobal := fFame * 10 + fInf\n\
         end) 0 TestReputation",
    );
    fnv_game::functions::reputation_store(&mut g, FormId(REPU), 1, 5.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*rep_ok""#), 1.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*rep_fame""#), 5.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*rep_inf""#), 0.0);
    assert_eq!(global(&mut g), 50.0);
    fnv_game::functions::reputation_store(&mut g, FormId(REPU), 0, 3.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*rep_fame""#), 5.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*rep_inf""#), 3.0);
    assert_eq!(global(&mut g), 53.0);
    // Removing it: `0 function flag filters` as set.
    q(&mut g, "let TestGlobal := 0");
    let removed = g.other.jg8.handlers.len();
    q(&mut g, "SetJohnnyOnFailQuestEventHandler 0 (begin function {ref rQ}\nend) 0 TestQuest");
    assert_eq!(g.other.jg8.handlers.len(), removed, "another function's removal leaves the handler");
    // Aliases SetOnSettingsUpdateEventHandler and SetOnKBCTrlUIDeltaHandler:
    // MenuID filter 0 or omitted matches any menu; positive MenuID matches only that menu.
    q(&mut g, "let TestGlobal := 0\nSetOnSettingsUpdateEventHandler 1 (begin function {}\nlet TestGlobal := TestGlobal + 7\nend) 0");
    g.jg8_settings_closed();
    assert_eq!(global(&mut g), 7.0);
    q(&mut g, "let TestGlobal := 0\nSetOnKBCTrlUIDeltaHandler 1 (begin function {int m}\nlet TestGlobal := TestGlobal + m\nend) 0 1002");
    q(&mut g, "SetOnKBCTrlUIDeltaHandler 1 (begin function {int m}\nlet TestGlobal := TestGlobal + 1\nend) 0 0");
    q(&mut g, "SetJohnnyOnKeyboardControllerSelectionChangeEventHandler 1 (begin function {int m}\nlet TestGlobal := TestGlobal + 10\nend) 0");
    g.jg8_keyboard_selection(1008.0);
    assert_eq!(global(&mut g), 11.0, "only wildcard 0 and unfiltered handlers fire for 1008");
    g.jg8_keyboard_selection(1002.0);
    assert_eq!(global(&mut g), 11.0 + 1002.0 + 1.0 + 10.0, "exact 1002 plus both wildcards fire");
    // Unregistering with setOrRemove=0 and matching func+filter removes that handler.
    let before = g.other.jg8.handlers.len();
    q(&mut g, "ref h\nlet h := (begin function {int m}\nlet TestGlobal := TestGlobal + 99\nend)\nSetOnKBCTrlUIDeltaHandler 1 h 0 1002\nSetOnKBCTrlUIDeltaHandler 0 h 0 1002");
    assert_eq!(g.other.jg8.handlers.len(), before);
}

#[test]
fn files_and_headers() {
    // [documented: RFC 1321 and RFC 3174 test vectors].
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    assert_eq!(hex(&md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(hex(&sha1(&[b'a'; 1000])), "291e9a6c66994949b57ba5e650361e98fc36b1ba");
    // [documented: dds.md §1, height at 12, width 16, mip count 28, FourCC 84].
    let mut dds = vec![0u8; 128];
    dds[..4].copy_from_slice(b"DDS ");
    dds[12..16].copy_from_slice(&256u32.to_le_bytes());
    dds[16..20].copy_from_slice(&512u32.to_le_bytes());
    dds[28..32].copy_from_slice(&10u32.to_le_bytes());
    dds[84..88].copy_from_slice(b"DXT5");
    assert_eq!(dds_header(&dds), Some((512, 256, 10, *b"DXT5")));
    // A 2x2 24-bit bitmap, bottom-up rows padded to 8 bytes: top-left red, bottom-right blue.
    let mut bmp = vec![0u8; 54];
    bmp[..2].copy_from_slice(b"BM");
    bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&2u32.to_le_bytes());
    bmp[22..26].copy_from_slice(&2u32.to_le_bytes());
    bmp[28] = 24;
    bmp.extend_from_slice(&[0, 0, 0, 255, 0, 0, 0, 0]); // bottom row: black, blue
    bmp.extend_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]); // top row: red, green
    assert_eq!(bmp_pixel(&bmp, 0, 0), Some([255, 0, 0]));
    assert_eq!(bmp_pixel(&bmp, 1, 0), Some([0, 255, 0]));
    assert_eq!(bmp_pixel(&bmp, 1, 1), Some([0, 0, 255]));
    assert_eq!(bmp_pixel(&bmp, 2, 0), None);
}

#[test]
fn arrays_colours_and_menus() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: Ar_SortEditor by editor ID, reverse with 1].
    assert_eq!(s(&mut g, "array_var a\nlet a := Ar_SortEditor (Ar_List ZuluThing, AlphaThing)\nSetFunctionValue (GetEditorID a[0])"), "AlphaThing");
    assert_eq!(s(&mut g, "array_var a\nlet a := Ar_SortEditor (Ar_List AlphaThing, ZuluThing) 1\nSetFunctionValue (GetEditorID a[0])"), "ZuluThing");
    // [documented: Ar_Shuffle keeps the elements].
    assert_eq!(n(&mut g, "Ar_Size (Ar_Shuffle (Ar_List 1, 2, 3, 4, 5))"), 5.0);
    assert_eq!(q(&mut g, "array_var a\nlet a := Ar_Shuffle (Ar_List 1, 2, 3, 4, 5)\nSetFunctionValue (a[0] + a[1] + a[2] + a[3] + a[4])"), XVal::Num(15.0));
    // [documented: GetSystemColor / GetSystemColorAlt, SetSystemColor's colour, amber
    // before].
    // [observed: probe set jg8, 1/4/5 FFB642, 2 FF432A, 3 21E779].
    assert_eq!(n(&mut g, "GetSystemColor 1"), f64::from(0xFFB642u32));
    assert_eq!(n(&mut g, "GetSystemColor 2"), f64::from(0xFF432Au32));
    assert_eq!(n(&mut g, "GetSystemColor 3"), f64::from(0x21E779u32));
    q(&mut g, "SetSystemColor 4 10 20 30");
    assert_eq!(s(&mut g, "int r\nint gg\nint b\nGetSystemColorAlt 4 r gg b\nSetFunctionValue (sv_Construct \"%g %g %g\" r gg b)"), "10 20 30");
    // Invalid ids return 0 and preserve output variables unchanged [observed: w6_t1_08_jg_system_color_alt].
    assert_eq!(s(&mut g, "int r = 111\nint gg = 222\nint b = 333\nGetSystemColorAlt 0 r gg b\nSetFunctionValue (sv_Construct \"%g %g %g\" r gg b)"), "111 222 333");
    assert_eq!(s(&mut g, "int r = 111\nint gg = 222\nint b = 333\nGetSystemColorAlt 6 r gg b\nSetFunctionValue (sv_Construct \"%g %g %g\" r gg b)"), "111 222 333");
    assert_eq!(n(&mut g, "GetSystemColor 6"), 0.0);
    // [documented: GetSleepWaitMenuState 0 closed, 1 wait, 2 sleep].
    assert_eq!(n(&mut g, "GetSleepWaitMenuState"), 0.0);
    g.other.jg8.sleep_menu = true;
    g.open_menu(1012);
    assert_eq!(n(&mut g, "GetSleepWaitMenuState"), 2.0);
    // ClearCustomMapMarker returns 1 even when no custom marker is set [observed: Wave 4
    // jg_29_custom_map_marker_set_clear_ret, JohnnyGuitarNVSE 528: 1,1,1,1].
    assert_eq!(n(&mut g, "ClearCustomMapMarker"), 1.0);
    g.other.jg8.custom_marker = Some((FormId(0x1), [10.0, 20.0, 30.0]));
    assert_eq!(n(&mut g, "ClearCustomMapMarker"), 1.0);
    assert_eq!(g.other.jg8.custom_marker, None);
    assert_eq!(n(&mut g, "ClearCustomMapMarker"), 1.0);
    assert_eq!(n(&mut g, "IsBSALoaded \"Fallout - Meshes.bsa\""), 0.0);
}

#[test]
fn quest_and_perk_handlers_support_formlist_and_xmarker_filters() {
    let f = fixture();
    let mut g = game(&f);
    // [observed 2026-11-24: w6t2p3_09_11_flst_real, FNV 1.4.0.525 + JG 528:
    // qStart(u=2,d=1,fl=1,xm=2)|qStopFl=1|pAdd(u=1,d=1,fl=1,xm=1)|pRemFl=1]
    q(
        &mut g,
        r#"
SetJohnnyOnStartQuestEventHandler 1 (begin function {ref rQ}
PlayerREF.AuxVarSetFlt "*qs_fl" ((PlayerREF.AuxVarGetFlt "*qs_fl") + 1)
end) 0 QuestPerkList
SetJohnnyOnStartQuestEventHandler 1 (begin function {ref rQ}
PlayerREF.AuxVarSetFlt "*qs_xm" ((PlayerREF.AuxVarGetFlt "*qs_xm") + 1)
end) 0 XMarker
SetJohnnyOnStartQuestEventHandler 1 (begin function {ref rQ}
PlayerREF.AuxVarSetFlt "*qs_other" ((PlayerREF.AuxVarGetFlt "*qs_other") + 1)
end) 0 OtherQuest
SetJohnnyOnStopQuestEventHandler 1 (begin function {ref rQ}
PlayerREF.AuxVarSetFlt "*qstop_fl" ((PlayerREF.AuxVarGetFlt "*qstop_fl") + 1)
end) 0 QuestPerkList
SetJohnnyOnStopQuestEventHandler 1 (begin function {ref rQ}
PlayerREF.AuxVarSetFlt "*qstop_xm" ((PlayerREF.AuxVarGetFlt "*qstop_xm") + 1)
end) 0 XMarker
SetJohnnyOnAddPerkEventHandler 1 (begin function {ref rP, int iO, int iN}
PlayerREF.AuxVarSetFlt "*pa_fl" ((PlayerREF.AuxVarGetFlt "*pa_fl") + 1)
end) 0 QuestPerkList
SetJohnnyOnAddPerkEventHandler 1 (begin function {ref rP, int iO, int iN}
PlayerREF.AuxVarSetFlt "*pa_xm" ((PlayerREF.AuxVarGetFlt "*pa_xm") + 1)
end) 0 XMarker
SetJohnnyOnAddPerkEventHandler 1 (begin function {ref rP, int iO, int iN}
PlayerREF.AuxVarSetFlt "*pa_other" ((PlayerREF.AuxVarGetFlt "*pa_other") + 1)
end) 0 OtherPerk
SetJohnnyOnRemovePerkEventHandler 1 (begin function {ref rP}
PlayerREF.AuxVarSetFlt "*pr_fl" ((PlayerREF.AuxVarGetFlt "*pr_fl") + 1)
end) 0 QuestPerkList
SetJohnnyOnRemovePerkEventHandler 1 (begin function {ref rP}
PlayerREF.AuxVarSetFlt "*pr_xm" ((PlayerREF.AuxVarGetFlt "*pr_xm") + 1)
end) 0 XMarker

StartQuest TestQuest
StopQuest TestQuest
PlayerREF.AddPerk TestPerk
PlayerREF.RemovePerk TestPerk
"#,
    );
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*qs_fl""#), 1.0, "StartQuest FormList matches");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*qs_xm""#), 1.0, "StartQuest XMarker wildcard matches");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*qs_other""#), 0.0, "StartQuest unrelated filter does not match");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*qstop_fl""#), 1.0, "StopQuest FormList matches");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*qstop_xm""#), 1.0, "StopQuest XMarker wildcard matches");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*pa_fl""#), 1.0, "AddPerk FormList matches");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*pa_xm""#), 1.0, "AddPerk XMarker wildcard matches");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*pa_other""#), 0.0, "AddPerk unrelated filter does not match");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*pr_fl""#), 1.0, "RemovePerk FormList matches");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*pr_xm""#), 1.0, "RemovePerk XMarker wildcard matches");

    // Non-member quest/perk triggers XMarker wildcard but not QuestPerkList.
    q(&mut g, "StartQuest OtherQuest\nPlayerREF.AddPerk OtherPerk");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*qs_fl""#), 1.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*qs_xm""#), 2.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*qs_other""#), 1.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*pa_fl""#), 1.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*pa_xm""#), 2.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*pa_other""#), 1.0);
}

#[test]
fn on_av_change_wildcard_minus_one_and_truncation() {
    let f = fixture();
    let mut g = game(&f);
    // [observed 2026-11-24: w6t2p2_10_jg_on_av_change_timed and w6t2p3_10_av_m1_exact,
    // FNV 1.4.0.525 + JG 528: -1 is wildcard, flag=0 truncates 200.0->199.4->198.8 to
    // 200->199 and 199->198, flag=1 passes exact floats]
    q(
        &mut g,
        r#"
PlayerREF.SetAV Health 200
int iM1
let iM1 := -1
SetOnActorValueChangeEventHandler 1 (begin function {int iAv, float fOld, float fNew}
PlayerREF.AuxVarSetFlt "*av_i_n" ((PlayerREF.AuxVarGetFlt "*av_i_n") + 1)
PlayerREF.AuxVarSetFlt "*av_i_o" fOld
PlayerREF.AuxVarSetFlt "*av_i_new" fNew
end) 0 16
SetOnActorValueChangeEventHandler 1 (begin function {int iAv, float fOld, float fNew}
PlayerREF.AuxVarSetFlt "*av_f_n" ((PlayerREF.AuxVarGetFlt "*av_f_n") + 1)
PlayerREF.AuxVarSetFlt "*av_f_o" fOld
PlayerREF.AuxVarSetFlt "*av_f_new" fNew
end) 1 16
SetOnActorValueChangeEventHandler 1 (begin function {int iAv, float fOld, float fNew}
PlayerREF.AuxVarSetFlt "*av_m1_n" ((PlayerREF.AuxVarGetFlt "*av_m1_n") + 1)
end) 0 iM1
SetOnActorValueChangeEventHandler 1 (begin function {int iAv, float fOld, float fNew}
PlayerREF.AuxVarSetFlt "*av_ag_n" ((PlayerREF.AuxVarGetFlt "*av_ag_n") + 1)
end) 0 0

PlayerREF.DamageAV Health 0.6
"#,
    );
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_i_n""#), 1.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_i_o""#), 200.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_i_new""#), 199.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_f_n""#), 1.0);
    assert!((n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_f_new""#) - 199.4).abs() < 1e-4);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_m1_n""#), 1.0, "-1 acts as wildcard, not AV 0");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_ag_n""#), 0.0, "AV 0 filter still excludes Health");

    // Sub-integer drop inside the same whole number (199.4 -> 199.2): exact fires, trunc skips.
    q(&mut g, "PlayerREF.DamageAV Health 0.2");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_i_n""#), 1.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_f_n""#), 2.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_m1_n""#), 1.0);

    // Crossing the next integer boundary (199.2 -> 198.6): trunc fires 199.0 -> 198.0.
    q(&mut g, "PlayerREF.DamageAV Health 0.6");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_i_n""#), 2.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_i_o""#), 199.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_i_new""#), 198.0);
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*av_m1_n""#), 2.0);
}

#[test]
fn on_dying_handler_matches_base_form_ref_and_xmarker() {
    let f = fixture();
    let mut g = game(&f);
    // [observed 2026-11-24: w6t2p2_09_jg_on_dying_all_filters, FNV 1.4.0.525 + JG 528:
    // dead=1|u=1,xm=1,ref=1,base=1,flst=0]
    q(
        &mut g,
        r#"
SetJohnnyOnDyingEventHandler 1 (begin function {ref rDead}
PlayerREF.AuxVarSetFlt "*d_u" ((PlayerREF.AuxVarGetFlt "*d_u") + 1)
PlayerREF.AuxVarSetRef "*d_arg" rDead
end) 0
SetJohnnyOnDyingEventHandler 1 (begin function {ref rDead}
PlayerREF.AuxVarSetFlt "*d_xm" ((PlayerREF.AuxVarGetFlt "*d_xm") + 1)
end) 0 XMarker
SetJohnnyOnDyingEventHandler 1 (begin function {ref rDead}
PlayerREF.AuxVarSetFlt "*d_ref" ((PlayerREF.AuxVarGetFlt "*d_ref") + 1)
end) 0 TestNpcRef
SetJohnnyOnDyingEventHandler 1 (begin function {ref rDead}
PlayerREF.AuxVarSetFlt "*d_base" ((PlayerREF.AuxVarGetFlt "*d_base") + 1)
end) 0 TestNpcBase
SetJohnnyOnDyingEventHandler 1 (begin function {ref rDead}
PlayerREF.AuxVarSetFlt "*d_other" ((PlayerREF.AuxVarGetFlt "*d_other") + 1)
end) 0 OtherNpcBase
SetJohnnyOnDyingEventHandler 1 (begin function {ref rDead}
PlayerREF.AuxVarSetFlt "*d_fl_base" ((PlayerREF.AuxVarGetFlt "*d_fl_base") + 1)
end) 0 ListNpcBase
SetJohnnyOnDyingEventHandler 1 (begin function {ref rDead}
PlayerREF.AuxVarSetFlt "*d_fl_ref" ((PlayerREF.AuxVarGetFlt "*d_fl_ref") + 1)
end) 0 ListNpcRef
TestNpcRef.KillActor PlayerREF
"#,
    );
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*d_u""#), 1.0, "unfiltered handler fires");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetRef "*d_arg" == TestNpcRef"#), 1.0, "dying actor ref passed as arg");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*d_xm""#), 1.0, "XMarker (0x3B) acts as wildcard");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*d_ref""#), 1.0, "exact placed ref matches");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*d_base""#), 1.0, "NPC_ base form matches placed ref");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*d_other""#), 0.0, "unrelated NPC_ base form does not match");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*d_fl_base""#), 0.0, "FormList of NPC_ base forms does not match (flst=0)");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*d_fl_ref""#), 1.0, "FormList of placed refs still matches");

    // Unregistering an XMarker-registered handler with XMarker removes it; verify by
    // spawning and killing a second actor so Dying genuinely dispatches again.
    q(
        &mut g,
        r#"
ref h
ref rNpc2
let h := (begin function {ref rDead}
PlayerREF.AuxVarSetFlt "*d_rm" 1
end)
SetJohnnyOnDyingEventHandler 1 h 0 XMarker
SetJohnnyOnDyingEventHandler 0 h 0 XMarker
let rNpc2 := TestNpcRef.PlaceAtMe TestNpcBase 1
rNpc2.KillActor PlayerREF
"#,
    );
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*d_u""#), 2.0, "second actor kill must dispatch Dying");
    assert_eq!(n(&mut g, r#"PlayerREF.AuxVarGetFlt "*d_rm""#), 0.0, "unregistered XMarker handler must not fire on second kill");
}

#[test]
fn xmarker_filter_acts_as_wildcard_for_crosshair_and_jg8_events() {
    let f = fixture();
    let mut g = game(&f);

    // XMarker (0x3B) in filter slot 0 with form type 21 (ACTI) in slot 1 matches any ACTI ref
    // and rejects non-ACTI refs (matching active mods such as BetterTrapDisarm).
    q(
        &mut g,
        "let TestGlobal := 0\n\
         ref h\n\
         let h := (begin function {ref rTgt}\n\
           if rTgt == TrapRef\n\
             let TestGlobal := TestGlobal + 1\n\
           else\n\
             let TestGlobal := TestGlobal + 100\n\
           endif\n\
         end)\n\
         SetJohnnyOnCrosshairEventHandler 1 h 0 XMarker 21",
    );
    g.jg8_crosshair(MISC_REF);
    assert_eq!(global(&mut g), 0.0, "non-ACTI ref must not match form type 21");
    g.jg8_crosshair(TRAP_REF);
    assert_eq!(global(&mut g), 1.0, "XMarker wildcard + type 21 matches placed ACTI ref");

    // XMarker also acts as wildcard on SetOnReputationChangeEventHandler.
    q(
        &mut g,
        "let TestGlobal := 0\n\
         SetOnReputationChangeEventHandler 1 (begin function {ref rR}\n\
           let TestGlobal := TestGlobal + 1\n\
         end) 0 XMarker",
    );
    fnv_game::functions::reputation_store(&mut g, FormId(REPU), 1, 5.0);
    assert_eq!(global(&mut g), 1.0, "XMarker wildcard matches reputation change");
}
