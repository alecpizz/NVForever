//! JIP LN NVSE batch 12 (#1006; docs/spec-notes/jip-world-12.md) on a synthetic plugin:
//! the climate, wind, moons and lightning, image space modifiers, cells, the light at a
//! point, fast travel, game settings, purged references, internal markers, the barter
//! menu, the item menus, the recipe menu and its conditions, the quantity and text input
//! menus, GetWheelDisabled.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{menus, Game, GameConfig, RunCtx};
use fnv_game::nvse_jip12::{BarterView, ItemMenuView, LightField, MenuList};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;
use std::collections::BTreeSet;

const WRLD: u32 = 0x930;
const PCELL: u32 = 0x931;
const EXT: u32 = 0x932;
const EXT2: u32 = 0x933;
const ROOM: u32 = 0x934;
const CLMT: u32 = 0x935;
const CLMT2: u32 = 0x936;
const STAT: u32 = 0x937;
const NORTH: u32 = 0x938;
const MARKER: u32 = 0x939;
const REF_OUT: u32 = 0x93A;
const REF_NORTH: u32 = 0x93B;
const IMAD: u32 = 0x93C;
const IMAD2: u32 = 0x943;
const IMAD_HOLD_A: u32 = 0x944;
const IMAD_HOLD_B: u32 = 0x945;
const RCPE: u32 = 0x93D;
const MISC: u32 = 0x93E;
const NPC: u32 = 0x93F;
const NPC_REF: u32 = 0x940;
const GMST_F: u32 = 0x941;
const GMST_S: u32 = 0x942;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn edid(sig: &[u8; 4], fid: u32, name: &str, rest: &[fnv_esm::raw::Subrecord]) -> Vec<u8> {
    record(sig, 0, fid, &[vec![sub(b"EDID", z(name))], rest.to_vec()].concat())
}

fn placed(sig: &[u8; 4], fid: u32, name: &str, base: u32, pos: [f32; 3], rot: [f32; 3]) -> Vec<u8> {
    let data = [f32b(pos[0]), f32b(pos[1]), f32b(pos[2]), f32b(rot[0]), f32b(rot[1]), f32b(rot[2])].concat();
    record(sig, 0, fid, &[sub(b"EDID", z(name)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn plugin_bytes() -> Vec<u8> {
    let npcs = [record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]), edid(b"NPC_", NPC, "TestNpc", &[])].concat();
    let gmst = [edid(b"GMST", GMST_F, "fTestSetting", &[sub(b"DATA", f32b(2.5))]), edid(b"GMST", GMST_S, "sTestText", &[sub(b"DATA", z("Hello"))])].concat();
    let clmt = [edid(b"CLMT", CLMT, "TestClimate", &[]), edid(b"CLMT", CLMT2, "TestClimateTwo", &[])].concat();
    let stat = [
        edid(b"STAT", STAT, "TestStatic", &[sub(b"MODL", z("clutter\\box.nif"))]),
        edid(b"STAT", NORTH, "NorthMarker", &[sub(b"MODL", z("marker_north.nif"))]),
        edid(b"STAT", MARKER, "TestMarker", &[sub(b"MODL", z("markers\\testmarker.nif"))]),
    ]
    .concat();
    let misc = edid(b"MISC", MISC, "TestThing", &[]);
    let imad_dnam = [u32s(&[1]), f32b(10.0), vec![0; 236]].concat();
    let imad_hold_dnam = [u32s(&[0]), f32b(1.0), vec![0; 236]].concat();
    let imad = [
        edid(b"IMAD", IMAD, "TestModifier", &[sub(b"DNAM", imad_dnam.clone())]),
        edid(b"IMAD", IMAD2, "TestModifierTwo", &[sub(b"DNAM", imad_dnam)]),
        edid(b"IMAD", IMAD_HOLD_A, "TestHoldModifierA", &[sub(b"DNAM", imad_hold_dnam.clone())]),
        edid(b"IMAD", IMAD_HOLD_B, "TestHoldModifierB", &[sub(b"DNAM", imad_hold_dnam)]),
    ]
    .concat();
    let rcpe = edid(b"RCPE", RCPE, "TestRecipe", &[sub(b"FULL", z("Test Recipe"))]);
    let wrld = edid(b"WRLD", WRLD, "TestWorld", &[sub(b"CNAM", u32s(&[CLMT]))]);
    let pcell = edid(b"CELL", PCELL, "", &[sub(b"DATA", vec![0])]);
    let ext = edid(b"CELL", EXT, "TestExterior", &[sub(b"DATA", vec![0]), sub(b"XCLC", u32s(&[0, 0]))]);
    let ext_children = group(EXT.to_le_bytes(), 6, &group(EXT.to_le_bytes(), 9, &[placed(b"REFR", REF_OUT, "TestRefOut", STAT, [10.0, 10.0, 0.0], [0.0; 3]), placed(b"ACHR", NPC_REF, "NpcRef", NPC, [20.0, 10.0, 0.0], [0.0; 3])].concat()));
    let ext2 = edid(b"CELL", EXT2, "TestExteriorTwo", &[sub(b"DATA", vec![0]), sub(b"XCLC", [(-3i32).to_le_bytes(), 4i32.to_le_bytes()].concat())]);
    let world_children = group(WRLD.to_le_bytes(), 1, &[pcell, group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &[ext, ext_children, ext2].concat()))].concat());
    let room = edid(b"CELL", ROOM, "TestRoom", &[sub(b"DATA", vec![1])]);
    let room_children = group(ROOM.to_le_bytes(), 6, &group(ROOM.to_le_bytes(), 9, &placed(b"REFR", REF_NORTH, "TestNorthRef", NORTH, [0.0; 3], [0.0, 0.0, std::f32::consts::FRAC_PI_2])));
    [
        tes4(),
        top(b"GMST", &gmst),
        top(b"NPC_", &npcs),
        top(b"STAT", &stat),
        top(b"MISC", &misc),
        top(b"IMAD", &imad),
        top(b"CLMT", &clmt),
        top(b"RCPE", &rcpe),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[room, room_children].concat()))),
        top(b"WRLD", &[wrld, world_children].concat()),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Jip12.esm".into()).unwrap()];
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
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(EXT));
    g.player_entered(Some(FormId(EXT)));
    g
}

const VARS: &str = "float fX\nref rItem\nstring_var sOut\narray_var aList\narray_var aEl\n";

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

/// An array's elements as text, "none" for no array (`ar_Size` -1).
fn elements(g: &mut Game, expr: &str, spec: &str) -> String {
    s(g, &format!("let aList := {expr}\nlet sOut := \"none\"\nif (ar_Size aList) >= 0\nlet sOut := \"\"\nforeach aEl <- aList\nlet sOut := sOut + (sv_Construct \"{spec} \" aEl[\"value\"])\nloop\nendif\nSetFunctionValue sOut")).trim().to_string()
}

fn ids(g: &mut Game, expr: &str) -> String {
    elements(g, expr, "%i")
}

#[test]
fn the_sky() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetCurrentClimate"), f64::from(CLMT));
    assert_eq!(g.script_climate(), None);
    q(&mut g, "ForceClimate TestClimateTwo");
    assert_eq!((g.script_climate(), n(&mut g, "GetCurrentClimate")), (Some(FormId(CLMT2)), f64::from(CLMT2)));
    q(&mut g, "ForceClimate");
    assert_eq!(g.script_climate(), None);
    assert_eq!(n(&mut g, "SetCurrentClimate TestClimateTwo"), 0.0);
    assert_eq!(g.script_climate(), Some(FormId(CLMT2)));
    // Not a climate: nothing.
    q(&mut g, "ForceClimate TestStatic");
    assert_eq!(g.script_climate(), Some(FormId(CLMT2)));
    // Indoors the set climate does not follow.
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(ROOM));
    g.player_entered(Some(FormId(ROOM)));
    assert_eq!((g.script_climate(), n(&mut g, "GetCurrentClimate")), (None, 0.0));

    // The wind: -1 radian in degrees [observed: jip12 wind], then a script's read back
    // as given [observed: jip12 setwind], the engine's angle from +x.
    assert!((n(&mut g, "GetWindDirection") + 57.2958).abs() < 1e-3);
    assert_eq!(g.script_wind(), None);
    q(&mut g, "SetWindDirection 90");
    assert_eq!((n(&mut g, "GetWindDirection"), g.script_wind()), (90.0, Some(0.0)));
    q(&mut g, "SetWindDirection 270");
    assert_eq!((n(&mut g, "GetWindDirection"), g.script_wind()), (270.0, Some(180.0)));

    // The moons.
    assert_eq!(s(&mut g, "SetFunctionValue (GetMoonTexture 3)"), "\\Textures\\Sky\\Masser_one_wan.dds");
    assert_eq!(s(&mut g, "SetFunctionValue (GetMoonTexture 4)"), "", "the new moon [observed]");
    q(&mut g, "SetMoonTexture 3 \"textures/sky/one_wan_alt.dds\"");
    assert_eq!(s(&mut g, "SetFunctionValue (GetMoonTexture 3)"), "textures/sky/one_wan_alt.dds");
    assert_eq!(g.moon_textures()[3].as_deref(), Some("sky\\one_wan_alt.dds"));
    assert_eq!(g.moon_textures()[0], None);
    q(&mut g, "SetMoonTexture 0 \"\\Textures\\Sky\\Full_alt.dds\"");
    assert_eq!(g.moon_textures()[0].as_deref(), Some("Sky\\Full_alt.dds"), "a leading backslash");

    // Lightning: outdoors in the rain only.
    q(&mut g, "TriggerLightningFX");
    assert!(!g.take_lightning(), "indoors");
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(EXT));
    g.player_entered(Some(FormId(EXT)));
    q(&mut g, "TriggerLightningFX");
    assert!(!g.take_lightning(), "dry");
    g.set_shown_weather(FormId(0x99), true);
    q(&mut g, "TriggerLightningFX");
    assert!(g.take_lightning());
    assert!(!g.take_lightning(), "taken once");
}

#[test]
fn cells_light_and_settings() {
    let f = fixture();
    let mut g = game(&f);
    // Only cells in memory [observed: jip12 extcell].
    assert_eq!(n(&mut g, "GetExteriorCell TestWorld -3 4"), 0.0);
    g.set_buffered_cells(BTreeSet::from([FormId(EXT2)]));
    assert_eq!(n(&mut g, "GetExteriorCell TestWorld -3 4"), f64::from(EXT2));
    g.set_buffered_cells(BTreeSet::new());
    // An empty array, not none [observed: jip12 buffered.interiors].
    assert_eq!(n(&mut g, "ar_Size (GetBufferedCells 1)"), 0.0);
    assert_eq!(n(&mut g, "GetExteriorCell TestWorld 9 9"), 0.0);
    assert_eq!(n(&mut g, "GetWorldspacePersistentCell TestWorld"), f64::from(PCELL));
    assert!((n(&mut g, "GetCellNorthRotation TestRoom") - 90.0).abs() < 1e-4);
    assert_eq!(n(&mut g, "GetCellNorthRotation TestExterior"), 0.0);

    // Buffered: the loaded cells, and what the engine keeps.
    assert_eq!(n(&mut g, "GetCellBuffered TestExterior"), 1.0);
    assert_eq!(n(&mut g, "GetCellBuffered TestRoom"), 0.0);
    assert_eq!(n(&mut g, "TestRefOut.GetCellBuffered"), 1.0);
    g.set_buffered_cells(BTreeSet::from([FormId(ROOM)]));
    assert_eq!(n(&mut g, "GetCellBuffered TestRoom"), 1.0);
    assert_eq!(ids(&mut g, "GetBufferedCells 1"), format!("{ROOM:08X}"));
    assert!(ids(&mut g, "GetBufferedCells 0").contains(&format!("{EXT:08X}")));

    // The light at a point.
    assert_eq!(n(&mut g, "GetLightAmountAtPoint 0 0 0"), 0.0, "nothing measured yet");
    g.set_light_field(LightField { outdoors: None, interior: 0.2, lights: vec![([0.0, 0.0, 0.0], 100.0, 0.5)] });
    assert!((n(&mut g, "GetLightAmountAtPoint 0 0 0") - 70.0).abs() < 1e-3);
    assert!((n(&mut g, "GetLightAmountAtPoint 50 0 0") - 57.5).abs() < 1e-3, "0.2 + 0.5 × 0.75");
    assert!((n(&mut g, "GetLightAmountAtPoint 500 0 0") - 20.0).abs() < 1e-3);

    // Fast travel, per calling script.
    assert_eq!(n(&mut g, "GetPCFastTravelled"), 0.0);
    g.note_fast_travel();
    assert_eq!(n(&mut g, "GetPCFastTravelled"), 1.0);
    assert_eq!(n(&mut g, "GetPCFastTravelled"), 0.0, "since the last ask");
    assert_eq!(n(&mut g, "GetLoadOrderChanged"), 0.0);
    g.set_load_order_changed(true);
    assert_eq!(n(&mut g, "GetLoadOrderChanged"), 1.0);

    // The settings, by their names, filtered by kind.
    assert_eq!(s(&mut g, "let aList := GetGameSettings\nSetFunctionValue (sv_Construct \"%g %z\" aList[\"fTestSetting\"] aList[\"sTestText\"])"), "2.5 Hello");
    assert_eq!(n(&mut g, "ar_Size (GetGameSettings 2)"), 1.0);
    // A setting named with or without quotes: unquoted, the word is the setting's own
    // record, whose Editor ID is the name (`nvse_misc::setting_name`) [observed: mods
    // write `GetNumericGameSetting fVATSShotLongBurstTime`].
    assert_eq!(n(&mut g, "GetNumericGameSetting \"fTestSetting\""), 2.5);
    assert_eq!(n(&mut g, "GetNumericGameSetting fTestSetting"), 2.5);
    assert_eq!(s(&mut g, "SetFunctionValue (GetStringGameSetting sTestText)"), "Hello");
    assert_eq!(n(&mut g, "10 / GetNumericGameSetting fTestSetting"), 4.0);
    q(&mut g, "SetNumericGameSetting fTestSetting 5");
    assert_eq!(n(&mut g, "GetNumericGameSetting fTestSetting"), 5.0);
    q(&mut g, "ModNumericGameSetting fTestSetting -2.5");
    assert_eq!(n(&mut g, "GetNumericGameSetting \"fTestSetting\""), 2.5);
    // Another record's name is no setting.
    assert_eq!(n(&mut g, "GetNumericGameSetting TestMarker"), 0.0);
    assert_eq!(n(&mut g, "GetStringHash"), 0.0);

    // Internal markers: the marker model's own, a script's.
    assert_eq!(n(&mut g, "SetInternalMarker TestMarker"), 1.0);
    assert_eq!(n(&mut g, "SetInternalMarker TestStatic"), 0.0);
    assert_eq!(n(&mut g, "SetInternalMarker TestStatic 1"), 0.0, "2-arg setter returns 0 [observed: w6d2p4_53]");
    assert_eq!(n(&mut g, "SetInternalMarker TestStatic"), 1.0);
    assert!(g.internal_marker(FormId(STAT)));
    assert_eq!(n(&mut g, "SetInternalMarker TestMarker 0"), 0.0);
    assert!(!g.internal_marker(FormId(MARKER)));

    // Image space modifiers on: ApplyImageSpaceModifier A, B, A -> GetActiveIMODs
    // returns [A, B, A] end-to-end in application order without deduplication
    // [observed: w4d_24_active_imods_order: `n=3,0=00054E13,1=001736A8,2=00054E13`].
    assert_eq!(ids(&mut g, "GetActiveIMODs"), "none");
    q(&mut g, "ApplyImageSpaceModifier TestModifier");
    assert_eq!(ids(&mut g, "GetActiveIMODs"), format!("{IMAD:08X}"));
    q(&mut g, "ApplyImageSpaceModifier TestModifierTwo\nApplyImageSpaceModifier TestModifier");
    assert_eq!(ids(&mut g, "GetActiveIMODs"), format!("{IMAD:08X} {IMAD2:08X} {IMAD:08X}"));
}

#[test]
fn active_imods_preserves_non_animatable_stack_order_and_removal() {
    let f = fixture();
    let mut g = game(&f);
    assert!(!f.data.image_modifiers[&FormId(IMAD_HOLD_A)].animatable);
    assert!(!f.data.image_modifiers[&FormId(IMAD_HOLD_B)].animatable);
    assert_eq!(ids(&mut g, "GetActiveIMODs"), "none");

    // Non-animatable ApplyImageSpaceModifier A, B, A preserves exact application order
    // and duplicates on the same tick [observed: w4d_24_active_imods_order:
    // `n=3,0=00054E13,1=001736A8,2=00054E13`].
    q(&mut g, "ApplyImageSpaceModifier TestHoldModifierA\nApplyImageSpaceModifier TestHoldModifierB\nApplyImageSpaceModifier TestHoldModifierA");
    let expected = format!("{IMAD_HOLD_A:08X} {IMAD_HOLD_B:08X} {IMAD_HOLD_A:08X}");
    assert_eq!(ids(&mut g, "GetActiveIMODs"), expected, "same-tick non-animatable [A, B, A]");

    // Retained across ticks without expiring or collapsing (see docs/spec-notes/jip-world-12.md §2).
    g.frames(10);
    assert_eq!(ids(&mut g, "GetActiveIMODs"), expected, "next-tick non-animatable [A, B, A]");

    // RemoveImageSpaceModifier A removes all matching A instances while leaving B active
    // [observed: w1_3656_objectives_unholster_imod].
    q(&mut g, "RemoveImageSpaceModifier TestHoldModifierA");
    assert_eq!(n(&mut g, "IsImageSpaceActive TestHoldModifierA"), 0.0);
    assert_eq!(n(&mut g, "IsImageSpaceActive TestHoldModifierB"), 1.0);
    assert_eq!(ids(&mut g, "GetActiveIMODs"), format!("{IMAD_HOLD_B:08X}"));

    // Removing the remaining B clears the list cleanly back to ar_Null.
    q(&mut g, "RemoveImageSpaceModifier TestHoldModifierB");
    assert_eq!(ids(&mut g, "GetActiveIMODs"), "none");

    // At MAX_IMOD_STACK per form, re-applying evicts the oldest matching entry and
    // appends the new one in exact application order without disturbing other forms.
    for _ in 0..Game::MAX_IMOD_STACK {
        q(&mut g, "ApplyImageSpaceModifier TestHoldModifierA");
    }
    q(&mut g, "ApplyImageSpaceModifier TestHoldModifierB\nApplyImageSpaceModifier TestHoldModifierA");
    assert_eq!(g.image_modifiers.iter().filter(|m| m.modifier == FormId(IMAD_HOLD_A)).count(), Game::MAX_IMOD_STACK);
    assert_eq!(g.image_modifiers.iter().filter(|m| m.modifier == FormId(IMAD_HOLD_B)).count(), 1);
    let active_after_cap = ids(&mut g, "GetActiveIMODs");
    let tokens: Vec<&str> = active_after_cap.split_whitespace().collect();
    assert_eq!(tokens.len(), Game::MAX_IMOD_STACK + 1);
    assert_eq!(tokens[Game::MAX_IMOD_STACK - 1], format!("{IMAD_HOLD_B:08X}"));
    assert_eq!(tokens[Game::MAX_IMOD_STACK], format!("{IMAD_HOLD_A:08X}"));
}

#[test]
fn purged_references_go_with_their_cell() {
    let f = fixture();
    let mut g = game(&f);
    let made = match q(&mut g, "let rItem := TestRefOut.PlaceAtMe TestThing 1\nPlayerREF.AuxVarSetRef \"*r\" rItem\nSetFunctionValue rItem") {
        XVal::Ref(r) => FormId(r),
        v => panic!("{v:?}"),
    };
    assert!(made.is_dynamic());
    let toggle = |g: &mut Game, arg: &str| match q(g, &format!("let rItem := PlayerREF.AuxVarGetRef \"*r\"\nSetFunctionValue (rItem.TogglePurgeOnUnload{arg})")) {
        XVal::Num(x) => x,
        v => panic!("{v:?}"),
    };
    assert_eq!(toggle(&mut g, ""), 0.0);
    assert_eq!(toggle(&mut g, " 1"), 0.0, "returns previous state [observed: w6d2_23]");
    assert_eq!(toggle(&mut g, ""), 1.0);
    assert_eq!(toggle(&mut g, " 0"), 1.0, "returns previous state");
    assert_eq!(toggle(&mut g, " 1"), 0.0);
    // A placed reference is not purged [documented: dynamic references only].
    assert_eq!(n(&mut g, "TestRefOut.TogglePurgeOnUnload 1"), 0.0);
    assert!(!g.world.get(made).unwrap().deleted());
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(ROOM));
    g.player_entered(Some(FormId(ROOM)));
    assert!(g.world.get(made).unwrap().deleted(), "its cell left the loaded area");
    assert!(!g.world.get(FormId(REF_OUT)).unwrap().deleted());
}

#[test]
fn barter_and_item_menus() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetBarterGoldAlt"), 0.0);
    assert_eq!(n(&mut g, "ar_Size (GetBarterItems 0)"), -1.0);
    g.set_barter_view(Some(BarterView { merchant_caps: 350, stock: FormId(NPC_REF), bought: vec![FormId(MISC)], sold: Vec::new(), mult: [1.5, 0.5] }));
    assert_eq!(n(&mut g, "GetBarterGoldAlt"), 350.0);
    assert_eq!(n(&mut g, "ar_Size (GetBarterItems 0)"), 1.0);
    assert_eq!(n(&mut g, "ar_Size (GetBarterItems 1)"), -1.0);
    assert_eq!(q(&mut g, "let aList := GetBarterItems 0\nlet rItem := aList[0]\nSetFunctionValue (rItem.GetBaseObject)"), XVal::Ref(MISC));
    assert_eq!(n(&mut g, "GetBarterPriceMult 0"), 1.5);
    // Bought at least 1, sold at most 1 [documented].
    q(&mut g, "SetBarterPriceMult 0 0.5\nSetBarterPriceMult 1 1.25");
    assert_eq!(g.barter_price_mults(), [Some(1.0), Some(1.0)]);
    q(&mut g, "SetBarterPriceMult 1 0.8");
    assert_eq!(n(&mut g, "GetBarterPriceMult 1"), 0.800000011920929);
    g.set_barter_view(None);
    assert_eq!(g.barter_price_mults(), [None, None], "the trade's multipliers go with it");

    // The item menus: before menus are opened, inventory (1002) defaults to 1 while
    // container (1008) and barter (1053) return -1 [observed: jip12 filter.inventory;
    // Wave 4 jip_40_barter_and_filter_defaults: 1, -1, -1].
    assert_eq!(n(&mut g, "GetMenuItemFilter 1002"), 1.0);
    assert_eq!(n(&mut g, "GetMenuItemFilter 1008"), -1.0);
    assert_eq!(n(&mut g, "GetMenuItemFilter 1053"), -1.0);
    let view = ItemMenuView { filter: [3, 0], lists: [MenuList { owner: Some(FormId::PLAYER_REF), rows: vec![(0, FormId(MISC), false), (2, FormId(STAT), true)] }, MenuList::default()] };
    g.set_item_menu_view(1008, Some(view));
    assert_eq!(n(&mut g, "GetMenuItemFilter 1008"), 3.0);
    assert_eq!(n(&mut g, "GetMenuItemFilter 1008 1"), 0.0);
    assert_eq!(n(&mut g, "GetMenuItemFilter 1002"), 1.0);
    assert_eq!(n(&mut g, "ar_Size GetMenuItemListRefs"), 1.0);
    assert_eq!(q(&mut g, "let aList := GetMenuItemListRefs\nlet rItem := aList[2]\nSetFunctionValue (rItem.GetBaseObject)"), XVal::Ref(STAT));
    assert_eq!(n(&mut g, "ar_Size (GetMenuItemListRefs 1)"), 2.0);
    assert_eq!(n(&mut g, "ar_Size (GetMenuItemListRefs 0 1)"), -1.0);

    // The recipe menu and a condition.
    assert_eq!(n(&mut g, "GetRecipeMenuSelection"), 0.0);
    g.set_recipe_menu(Some((None, Some(FormId(RCPE)))));
    assert_eq!(n(&mut g, "GetRecipeMenuSelection"), f64::from(RCPE));
    assert!(g.recipe_conditions_pass(FormId(RCPE)));
    // GetLevel (index 80) >= 100: the player is level 1.
    q(&mut g, "AddRecipeCondition TestRecipe 80 0 3 100");
    assert!(!g.recipe_conditions_pass(FormId(RCPE)));

    // The wheel.
    assert_eq!(n(&mut g, "NpcRef.GetWheelDisabled"), 0.0);
    q(&mut g, "NpcRef.SetWheelDisabled 1");
    assert_eq!(n(&mut g, "NpcRef.GetWheelDisabled"), 1.0);
}

#[test]
fn quantity_and_text_input_menus() {
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "ShowQuantityMenu (Begin Function {int i}\nPlayerREF.AuxVarSetFlt \"*q\" i\nEnd) 20 5");
    assert_eq!(g.menu, Some(menus::QUANTITY));
    assert_eq!(g.quantity_ask().map(|a| (a.max, a.initial)), Some((20, 5)));
    g.finish_quantity(Some(7));
    assert_eq!(g.menu, None);
    g.tick();
    assert_eq!(n(&mut g, "PlayerREF.AuxVarGetFlt \"*q\""), 7.0);
    q(&mut g, "ShowQuantityMenu (Begin Function {int i}\nPlayerREF.AuxVarSetFlt \"*q\" i\nEnd) 20");
    g.finish_quantity(None);
    g.tick();
    assert_eq!(n(&mut g, "PlayerREF.AuxVarGetFlt \"*q\""), -1.0, "Cancel");

    assert_eq!(n(&mut g, "ShowTextInputMenu (Begin Function {string_var t}\nPlayerREF.AuxVarSetStr \"*t\" t\nEnd) 700 500 \"Type %g:\" 3"), 1.0);
    assert_eq!(g.menu, Some(menus::NAME));
    assert_eq!(g.text_input().map(|t| (t.title.as_str(), t.min, t.max)), Some(("Type 3:", 1, 32767)));
    assert_eq!(n(&mut g, "ShowTextInputMenu (Begin Function {string_var t}\nEnd) 700 500 \"Again\""), 0.0, "one open");
    q(&mut g, "SetTextInputExtendedProps 0 0 2 10 3\nSetTextInputString \"Pre%g\" 9");
    let t = g.text_input().unwrap();
    assert_eq!((t.min, t.max, t.digits_only, t.enter_closes), (2, 10, true, true));
    assert_eq!(g.take_text_input_string().as_deref(), Some("Pre9"));
    g.finish_text_input("Typed".into());
    assert_eq!(g.menu, None);
    g.tick();
    assert_eq!(s(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetStr \"*t\")"), "Typed");
}

