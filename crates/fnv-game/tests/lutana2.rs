//! Lutana NVSE batch 2 (#1009; docs/spec-notes/ext-lutana.md §9) on a synthetic plugin
//! written for the test (no game data, made-up names): map markers and radio stations
//! from their REFR data, hotkeys, the INI writers, ingestible flags, form counts by type
//! and the plugins that hold a form.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const STAT: u32 = 0x800;
const TACT: u32 = 0x801;
const FOOD: u32 = 0x802;
const ARMO: u32 = 0x803;
const FLST: u32 = 0x804;
const ROOM: u32 = 0x900;
const MARKER: u32 = 0x910;
const RADIO: u32 = 0x911;
const TRAVEL_ONLY_MARKER: u32 = 0x912;
const PLAIN_MARKER: u32 = 0x913;
const VISIBLE_ONLY_MARKER: u32 = 0x914;
const PROBE_MARKER: u32 = 0x915;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let stat = record(b"STAT", 0, STAT, &[sub(b"EDID", z("TestStatic"))]);
    let tact = record(b"TACT", 0, TACT, &[sub(b"EDID", z("TestRadioStation"))]);
    // ENIT: value, flags (2 Food), withdrawal, addiction chance, consume sound
    // [documented: schemas/fnv ALCH ENIT].
    let enit = [5i32.to_le_bytes().to_vec(), vec![2, 0, 0, 0], 0u32.to_le_bytes().to_vec(), 0f32.to_le_bytes().to_vec(), 0u32.to_le_bytes().to_vec()].concat();
    let alch = record(b"ALCH", 0, FOOD, &[sub(b"EDID", z("TestFood")), sub(b"ENIT", enit)]);
    let armo = record(b"ARMO", 0, ARMO, &[sub(b"EDID", z("TestArmor"))]);
    let flst = record(b"FLST", 0, FLST, &[sub(b"EDID", z("TestFormList"))]);
    let room = record(b"CELL", 0, ROOM, &[sub(b"EDID", z("TestRoom")), sub(b"DATA", vec![1])]);
    let marker = record(
        b"REFR",
        0,
        MARKER,
        &[sub(b"EDID", z("TestMarkerRef")), sub(b"NAME", STAT.to_le_bytes().to_vec()), sub(b"XMRK", vec![]), sub(b"FNAM", vec![1 | 2]), sub(b"FULL", z("Test Place")), sub(b"TNAM", vec![5, 0]), sub(b"DATA", vec![0; 24])],
    );
    let travel_only = record(
        b"REFR",
        0,
        TRAVEL_ONLY_MARKER,
        &[
            sub(b"EDID", z("TravelOnlyMarkerRef")),
            sub(b"NAME", STAT.to_le_bytes().to_vec()),
            sub(b"XMRK", vec![]),
            sub(b"FNAM", vec![2]),
            sub(b"FULL", z("Travel Only Place")),
            sub(b"TNAM", vec![2, 0]),
            sub(b"DATA", vec![0; 24]),
        ],
    );
    let plain = record(
        b"REFR",
        0,
        PLAIN_MARKER,
        &[
            sub(b"EDID", z("PlainMarkerRef")),
            sub(b"NAME", STAT.to_le_bytes().to_vec()),
            sub(b"XMRK", vec![]),
            sub(b"FNAM", vec![0]),
            sub(b"FULL", z("Plain Place")),
            sub(b"TNAM", vec![3, 0]),
            sub(b"DATA", vec![0; 24]),
        ],
    );
    let visible_only = record(
        b"REFR",
        0,
        VISIBLE_ONLY_MARKER,
        &[
            sub(b"EDID", z("VisibleOnlyMarkerRef")),
            sub(b"NAME", STAT.to_le_bytes().to_vec()),
            sub(b"XMRK", vec![]),
            sub(b"FNAM", vec![1]),
            sub(b"FULL", z("Visible Only Place")),
            sub(b"TNAM", vec![4, 0]),
            sub(b"DATA", vec![0; 24]),
        ],
    );
    let probe = record(
        b"REFR",
        0,
        PROBE_MARKER,
        &[
            sub(b"EDID", z("ProbeMarkerRef")),
            sub(b"NAME", STAT.to_le_bytes().to_vec()),
            sub(b"XMRK", vec![]),
            sub(b"FNAM", vec![0]),
            sub(b"FULL", z("Probe Place")),
            sub(b"TNAM", vec![6, 0]),
            sub(b"DATA", vec![0; 24]),
        ],
    );
    let xrdo = [6000f32.to_le_bytes(), 2u32.to_le_bytes(), 15f32.to_le_bytes(), 0u32.to_le_bytes()].concat();
    let radio = record(b"REFR", 0, RADIO, &[sub(b"EDID", z("TestRadioRef")), sub(b"NAME", TACT.to_le_bytes().to_vec()), sub(b"XRDO", xrdo), sub(b"DATA", vec![0; 24])]);
    let children = group(ROOM.to_le_bytes(), 6, &group(ROOM.to_le_bytes(), 9, &[marker, travel_only, plain, visible_only, probe, radio].concat()));
    let bytes = [
        tes4(),
        top(b"STAT", &stat),
        top(b"TACT", &tact),
        top(b"ALCH", &alch),
        top(b"ARMO", &armo),
        top(b"FLST", &flst),
        top(b"NPC_", &record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))])),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[room, children].concat()))),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "Lutana2Test.esm".into()).unwrap()];
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
    let body = if expr.contains('\n') { expr.to_string() } else { format!("SetFunctionValue ({expr})") };
    match q(g, &body) {
        XVal::Num(x) => x,
        XVal::Ref(r) => f64::from(r),
        v => panic!("{expr}: {v:?}"),
    }
}

#[test]
fn map_markers_and_radio_stations_from_their_data() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "TestMarkerRef.IsMapMarker"), 1.0);
    assert_eq!(n(&mut g, "TestRadioRef.IsMapMarker"), 0.0);
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerTravel"), 1.0);
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerHidden"), 0.0);
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerType"), 5.0, "Cave");
    // Observed original-game FNAM matrix (#3468 check 252 + 2026-11-22 live map-marker probe, ext-lutana.md §9):
    // - FNAM=0: Vis=0, Trv=0, Hid=0
    // - FNAM=1 (Visible): Vis=1, Trv=0, Hid=0
    // - FNAM=2 (Can Travel To alone): Vis=0, Trv=0, Hid=0
    // - FNAM=3 (Visible | Can Travel To): Vis=2, Trv=1, Hid=0
    assert_eq!(n(&mut g, "PlainMarkerRef.GetMapMarkerVisible"), 0.0, "FNAM=0 starts 0");
    assert_eq!(n(&mut g, "PlainMarkerRef.GetMapMarkerTravel"), 0.0, "FNAM=0 travel starts 0");
    assert_eq!(n(&mut g, "VisibleOnlyMarkerRef.GetMapMarkerVisible"), 1.0, "FNAM=1 starts 1");
    assert_eq!(n(&mut g, "VisibleOnlyMarkerRef.GetMapMarkerTravel"), 0.0, "FNAM=1 travel starts 0");
    assert_eq!(n(&mut g, "TravelOnlyMarkerRef.GetMapMarkerVisible"), 0.0, "FNAM=2 alone reads 0");
    assert_eq!(n(&mut g, "TravelOnlyMarkerRef.GetMapMarkerTravel"), 0.0, "FNAM=2 alone travel reads 0");
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerVisible"), 2.0, "FNAM=3 reads 2 before discovery");
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerTravel"), 1.0, "FNAM=3 travel reads 1");
    // ShowMap 0 on a marker already at 2 leaves it at 2 (#3468).
    q(&mut g, "ShowMap TestMarkerRef 0");
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerVisible"), 2.0, "ShowMap 0 leaves FNAM=3 at 2");
    // ShowMap 0 -> Vis=1, Trv=0; ShowMap 1 -> Vis=2, Trv=1; subsequent ShowMap 0 keeps 2.
    q(&mut g, "ShowMap TravelOnlyMarkerRef 0");
    assert_eq!(n(&mut g, "TravelOnlyMarkerRef.GetMapMarkerVisible"), 1.0, "ShowMap 0 gives 1");
    assert_eq!(n(&mut g, "TravelOnlyMarkerRef.GetMapMarkerTravel"), 0.0, "ShowMap 0 keeps travel 0");
    q(&mut g, "ShowMap TravelOnlyMarkerRef 1");
    assert_eq!(n(&mut g, "TravelOnlyMarkerRef.GetMapMarkerVisible"), 2.0, "ShowMap 1 makes it a travel target");
    assert_eq!(n(&mut g, "TravelOnlyMarkerRef.GetMapMarkerTravel"), 1.0, "ShowMap 1 sets GetMapMarkerTravel=1");
    q(&mut g, "ShowMap TravelOnlyMarkerRef 0");
    assert_eq!(n(&mut g, "TravelOnlyMarkerRef.GetMapMarkerVisible"), 2.0, "ShowMap 0 on already-2 leaves it at 2");
    assert_eq!(n(&mut g, "TravelOnlyMarkerRef.GetMapMarkerTravel"), 1.0);
    // Player discovery sets both Vis=2 and GetMapMarkerTravel=1; ShowMap 0 leaves it at 2.
    q(&mut g, "ShowMap PlainMarkerRef 0");
    assert_eq!(n(&mut g, "PlainMarkerRef.GetMapMarkerVisible"), 1.0);
    assert_eq!(n(&mut g, "PlainMarkerRef.GetMapMarkerTravel"), 0.0);
    g.set_found_markers([FormId(PLAIN_MARKER)]);
    assert_eq!(n(&mut g, "PlainMarkerRef.GetMapMarkerVisible"), 2.0, "discovered marker reads 2");
    assert_eq!(n(&mut g, "PlainMarkerRef.GetMapMarkerTravel"), 1.0, "discovered marker sets GetMapMarkerTravel=1");
    q(&mut g, "ShowMap PlainMarkerRef 0");
    assert_eq!(n(&mut g, "PlainMarkerRef.GetMapMarkerVisible"), 2.0, "ShowMap 0 on discovered marker leaves it at 2");
    g.set_found_markers([]);
    // Probe transitions on FNAM=0: SetMapMarkerVisible 1/2 only sets Visible; SetMapMarkerTravel 1
    // alone reads Vis=0, Trv=1; Visible+Travel -> Vis=2; Hidden 1 keeps Vis=2; Visible 0 -> Vis=0, Trv=1.
    q(&mut g, "ProbeMarkerRef.SetMapMarkerVisible 1");
    assert_eq!(n(&mut g, "ProbeMarkerRef.GetMapMarkerVisible"), 1.0);
    assert_eq!(n(&mut g, "ProbeMarkerRef.GetMapMarkerTravel"), 0.0);
    q(&mut g, "ProbeMarkerRef.SetMapMarkerVisible 2");
    assert_eq!(n(&mut g, "ProbeMarkerRef.GetMapMarkerVisible"), 1.0, "SetMapMarkerVisible 2 sets only bit 0");
    assert_eq!(n(&mut g, "ProbeMarkerRef.GetMapMarkerTravel"), 0.0);
    q(&mut g, "ProbeMarkerRef.SetMapMarkerVisible 0\nProbeMarkerRef.SetMapMarkerTravel 1");
    assert_eq!(n(&mut g, "ProbeMarkerRef.GetMapMarkerVisible"), 0.0, "SetMapMarkerTravel 1 alone keeps Vis=0");
    assert_eq!(n(&mut g, "ProbeMarkerRef.GetMapMarkerTravel"), 1.0, "SetMapMarkerTravel 1 sets Trv=1");
    q(&mut g, "ProbeMarkerRef.SetMapMarkerVisible 1");
    assert_eq!(n(&mut g, "ProbeMarkerRef.GetMapMarkerVisible"), 2.0, "Visible 1 + Travel 1 reads 2");
    assert_eq!(n(&mut g, "ProbeMarkerRef.GetMapMarkerTravel"), 1.0);
    q(&mut g, "ProbeMarkerRef.SetMapMarkerHidden 1");
    assert_eq!(n(&mut g, "ProbeMarkerRef.GetMapMarkerVisible"), 2.0, "Hidden 1 does not zero GetMapMarkerVisible");
    assert_eq!(n(&mut g, "ProbeMarkerRef.GetMapMarkerHidden"), 1.0);
    q(&mut g, "ProbeMarkerRef.SetMapMarkerVisible 0");
    assert_eq!(n(&mut g, "ProbeMarkerRef.GetMapMarkerVisible"), 0.0, "SetMapMarkerVisible 0 drops Vis to 0");
    assert_eq!(n(&mut g, "ProbeMarkerRef.GetMapMarkerTravel"), 1.0, "SetMapMarkerVisible 0 leaves Trv at 1");
    assert_eq!(
        match q(&mut g, "SetFunctionValue (TestMarkerRef.GetCustomMapMarkerIcon)") {
            XVal::Str(s) => s,
            v => panic!("{v:?}"),
        },
        "Interface\\Icons\\World Map\\icon_map_cave.dds"
    );
    // On FNAM=3, SetMapMarkerTravel 0 drops Vis 2 -> 1 and Trv 1 -> 0; SetMapMarkerHidden 1 preserves Vis.
    q(&mut g, "TestMarkerRef.SetMapMarkerTravel 0");
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerVisible"), 1.0, "SetMapMarkerTravel 0 on FNAM=3 drops 2 -> 1");
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerTravel"), 0.0);
    q(&mut g, "TestMarkerRef.SetMapMarkerHidden 1");
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerVisible"), 1.0, "SetMapMarkerHidden alone keeps marker at 1");
    q(&mut g, "TestMarkerRef.SetMapMarkerTravel 1");
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerVisible"), 2.0, "script SetMapMarkerTravel 1 on visible marker reads 2");
    q(&mut g, "TestMarkerRef.SetMapMarkerTravel 0");
    q(&mut g, "TestMarkerRef.SetMapMarkerType 14");
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerTravel"), 0.0);
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerHidden"), 1.0);
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerType"), 14.0);
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerVisible"), 1.0, "Visible without Can Travel To");
    assert_eq!(
        match q(&mut g, "SetFunctionValue (TestMarkerRef.GetCustomMapMarkerIcon)") {
            XVal::Str(s) => s,
            v => panic!("{v:?}"),
        },
        "Interface\\Icons\\World Map\\icon_map_vault.dds",
        "GetCustomMapMarkerIcon default path follows SetMapMarkerType"
    );
    assert_eq!(g.marker_flags(FormId(MARKER)), Some(1 | 4));
    // SetMapMarkerVisible 0 hides the marker, and ShowAllMapMarkers skips it while Hidden is set.
    q(&mut g, "TestMarkerRef.SetMapMarkerVisible 0");
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerVisible"), 0.0);
    // Script SetMapMarkerTravel 1 on an unshown/hidden marker must still read 0 until shown/visible.
    q(&mut g, "TestMarkerRef.SetMapMarkerTravel 1");
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerVisible"), 0.0, "SetMapMarkerTravel 1 alone does not reveal an unshown marker");
    q(&mut g, "TestMarkerRef.SetMapMarkerTravel 0");
    q(&mut g, "ShowAllMapMarkers 1 0 0");
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerVisible"), 1.0, "skipHidden=0 reveals hidden marker");
    q(&mut g, "ShowAllMapMarkers 1 1 1");
    assert_eq!(n(&mut g, "TestMarkerRef.GetMapMarkerVisible"), 0.0, "skipHidden respects SetMapMarkerHidden");
    q(&mut g, "ShowAllMapMarkers 0");
    // Name and reputation accessors reflect SetMapMarkerName / SetMapMarkerRep.
    q(&mut g, "TestMarkerRef.SetMapMarkerName \"Owned Outpost\"");
    assert_eq!(g.map_marker_name_override(FormId(MARKER)), Some("Owned Outpost"));
    q(&mut g, "TestMarkerRef.SetMapMarkerRep 0");
    assert_eq!(g.map_marker_reputation_override(FormId(MARKER)), Some(None));
    // XRDO [documented: GetRadioRadius's example reads 6000, GetRadioStatic 15].
    assert_eq!(n(&mut g, "TestRadioRef.IsRadioRef"), 1.0);
    assert_eq!(n(&mut g, "TestRadioRef.GetRadioRadius"), 6000.0);
    assert_eq!(n(&mut g, "TestRadioRef.GetRadioStatic"), 15.0);
    // A range type the schema names (2, Worldspace and Linked Interiors) reads as its
    // number, as RNVTARef's Everywhere read 1 in the original [observed 2026-11-08].
    assert_eq!(n(&mut g, "TestRadioRef.GetRadioBroadcastType"), 2.0);
    q(&mut g, "TestRadioRef.SetRadioRadius 2000");
    q(&mut g, "TestRadioRef.SetRadioBroadcastType 1");
    q(&mut g, "TestRadioRef.SetRadioPosRef TestMarkerRef");
    assert_eq!(n(&mut g, "TestRadioRef.GetRadioRadius"), 2000.0);
    assert_eq!(n(&mut g, "TestRadioRef.GetRadioBroadcastType"), 1.0);
    assert_eq!(n(&mut g, "TestRadioRef.GetRadioPosRef"), f64::from(MARKER));
    // OOB broadcast types (outside 0..=4) and static percentages (outside 0..=100) are
    // rejected without mutating prior values [observed: w6d2_33].
    q(&mut g, "TestRadioRef.SetRadioBroadcastType 9\nTestRadioRef.SetRadioBroadcastType -1\nTestRadioRef.SetRadioStatic 150\nTestRadioRef.SetRadioStatic -5");
    assert_eq!(n(&mut g, "TestRadioRef.GetRadioBroadcastType"), 1.0);
    assert_eq!(n(&mut g, "TestRadioRef.GetRadioStatic"), 15.0);
}

#[test]
fn hotkeys_ini_writers_and_forms() {
    let f = fixture();
    let mut g = game(&f);
    g.world.inventory_mut(FormId::PLAYER_REF).unwrap().add(FormId(FOOD), 1);
    q(&mut g, "SetHotkey 3 TestFood 0.5 0");
    assert_eq!(n(&mut g, "GetHotkeyItem 3"), f64::from(FOOD));
    assert_eq!(n(&mut g, "array_var a\nlet a := SaveHotkeys\nSetFunctionValue a[2][1]"), 0.5);
    q(&mut g, "ClearAllHotkeys");
    assert_eq!(n(&mut g, "GetHotkeyItem 3"), 0.0);
    // The INI writers, through NVForever's in-memory layer.
    q(&mut g, r#"SetINISection "Main" (ar_Map "fA"::1.5 "fB"::2) "NVFTestLU2""#);
    assert_eq!(n(&mut g, r#"GetINIFloat "Main:fB" "NVFTestLU2""#), 2.0);
    q(&mut g, r#"RemoveINIKey "Main:fB" "NVFTestLU2""#);
    assert_eq!(n(&mut g, r#"GetINIFloat "Main:fB" "NVFTestLU2""#), 0.0);
    assert_eq!(n(&mut g, r#"GetINIFloat "Main:fA" "NVFTestLU2""#), 1.5);
    q(&mut g, r#"RemoveINISection "Main" "NVFTestLU2""#);
    assert_eq!(n(&mut g, r#"GetINIFloat "Main:fA" "NVFTestLU2""#), 0.0);
    // The food flag off, as the page's example [documented], and Medicine (4) + bitmask tests
    // [observed: w6d2p2_25_ingestible_flag_and_skill_magazine].
    assert_eq!(n(&mut g, "GetIngestibleFlag TestFood 2"), 1.0, "Food");
    assert_eq!(n(&mut g, "GetIngestibleFlag TestFood 4"), 0.0, "Food is not Medicine");
    assert_eq!(n(&mut g, "GetIngestibleFlag TestFood 6"), 1.0, "any-bit mask 2|4 matches Food");
    q(&mut g, "SetIngestibleFlag TestFood 4 1");
    assert_eq!(n(&mut g, "GetIngestibleFlag TestFood 4"), 1.0, "Medicine flag set");
    assert_eq!(n(&mut g, "GetEffectFlag TestFood 2"), 1.0, "Medicine is ENIT bit 2");
    assert_eq!(n(&mut g, "GetIngestibleFlag TestFood 5"), 1.0, "mask 1|4 matches Medicine");
    q(&mut g, "SetIngestibleFlag TestFood 4 0");
    assert_eq!(n(&mut g, "GetIngestibleFlag TestFood 4"), 0.0, "Medicine flag cleared");
    q(&mut g, "SetIngestibleFlag TestFood 2 0");
    assert_eq!(n(&mut g, "GetIngestibleFlag TestFood 2"), 0.0);
    assert_eq!(n(&mut g, "GetIngestibleFlag TestFood 0"), 0.0);
    assert_eq!(n(&mut g, "IsSkillMagazine TestFood"), 0.0);
    assert_eq!(n(&mut g, "GetShowQuestItems"), 1.0);
    q(&mut g, "SetShowQuestItems 0");
    assert_eq!(n(&mut g, "GetShowQuestItems"), 0.0);
    assert_eq!(n(&mut g, "GetFormCountType (ar_List TestFood TestStatic TestFood) 47"), 2.0, "ALCH is type 47");
    // The array's ID from an array variable, as the page's example and the probe.
    assert_eq!(n(&mut g, "array_var aF\nfloat fN\nlet aF := ar_List TestFood, TestStatic\nlet fN := GetFormCountType aF, 47\nSetFunctionValue fN"), 1.0);
    assert_eq!(
        q(&mut g, "array_var a\nlet a := GetFormMods TestFood\nSetFunctionValue a[0]"),
        XVal::Str("Lutana2Test.esm".into())
    );
}

/// `SetBipedModelList` validates `ARMO` + `FLST`, treats non-`FLST` and explicit `0` as
/// no-ops, and clears only on omitted 2nd arg [observed: `w6d2p5_04_biped_model_list_omit`].
#[test]
fn set_biped_model_list_validates_flst_null_and_omit() {
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "SetBipedModelList TestArmor TestFormList");
    assert_eq!(n(&mut g, "GetBipedModelList TestArmor"), f64::from(FLST));
    q(&mut g, "SetBipedModelList TestArmor TestFood");
    assert_eq!(n(&mut g, "GetBipedModelList TestArmor"), f64::from(FLST));
    q(&mut g, "SetBipedModelList TestArmor 0");
    assert_eq!(n(&mut g, "GetBipedModelList TestArmor"), f64::from(FLST));
    q(&mut g, "SetBipedModelList TestArmor");
    assert_eq!(n(&mut g, "GetBipedModelList TestArmor"), 0.0);
    // Non-ARMO target is ignored.
    q(&mut g, "SetBipedModelList TestFood TestFormList");
    assert_eq!(n(&mut g, "GetBipedModelList TestFood"), 0.0);
}
