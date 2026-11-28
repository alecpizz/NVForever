//! JohnnyGuitar NVSE batch 5 (#1007; docs/spec-notes/ext-jg5.md) on a synthetic plugin:
//! alternate textures, FaceGen model flags and icons, worldspace map textures and zones,
//! region map names and the location name, camera shots, body part strings, sounds,
//! equip types, recipe and weapon mod lookups, actor effect types and the time played.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const TXST1: u32 = 0x900;
const TXST2: u32 = 0x901;
const WEAP: u32 = 0x910;
const WEAP2: u32 = 0x911;
const WEAP_PLAIN: u32 = 0x912;
const IMOD: u32 = 0x913;
const ARMO: u32 = 0x920;
const WRLD: u32 = 0x930;
const ZONE1: u32 = 0x931;
const ZONE2: u32 = 0x932;
const EXT: u32 = 0x933;
const EXT_BARE: u32 = 0x934;
const ROOM: u32 = 0x935;
const REGN_HIGH: u32 = 0x936;
const REGN_LOW: u32 = 0x937;
const REGN_BARE: u32 = 0x938;
const MISC: u32 = 0x939;
const REF_OUT: u32 = 0x93A;
const REF_BARE: u32 = 0x93B;
const REF_IN: u32 = 0x93C;
const SOUN: u32 = 0x940;
const EXPL: u32 = 0x941;
const PROJ: u32 = 0x942;
const IMAD1: u32 = 0x950;
const IMAD2: u32 = 0x951;
const CAMS: u32 = 0x952;
const BPTD: u32 = 0x960;
const SPEL: u32 = 0x970;
const RCCT: u32 = 0x980;
const RCCT_SUB: u32 = 0x981;
const ITEM_A: u32 = 0x982;
const ITEM_B: u32 = 0x983;
const RCPE1: u32 = 0x984;
const RCPE2: u32 = 0x985;
const ALCH: u32 = 0x990;

fn edid(sig: &[u8; 4], fid: u32, name: &str, rest: &[fnv_esm::raw::Subrecord]) -> Vec<u8> {
    record(sig, 0, fid, &[vec![sub(b"EDID", z(name))], rest.to_vec()].concat())
}

fn le(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

/// MODS and its kin [documented: schema common.json AlternateTexture]: a count, then
/// per entry a u32-length shape name, the texture set and the 3D index.
fn alts(sig: &[u8; 4], rows: &[(&str, u32, i32)]) -> fnv_esm::raw::Subrecord {
    let mut b = le(rows.len() as u32);
    for (name, set, at) in rows {
        b.extend(le(name.len() as u32));
        b.extend(name.as_bytes());
        b.extend(le(*set));
        b.extend(at.to_le_bytes());
    }
    sub(sig, b)
}

/// RDAT [documented: schemas/fnv/sky.json RegionDataHeader]: type, flags, priority.
fn rdat(kind: u32, priority: u8) -> fnv_esm::raw::Subrecord {
    sub(b"RDAT", [le(kind), vec![0, priority, 0, 0]].concat())
}

/// BPND [documented: schema combat.json]: 84 bytes, the part type the sixth.
fn bpnd(part: u8) -> fnv_esm::raw::Subrecord {
    let mut b = vec![0u8; 84];
    b[5] = part;
    sub(b"BPND", b)
}

fn placed(fid: u32, name: &str, base: u32) -> Vec<u8> {
    record(b"REFR", 0, fid, &[sub(b"EDID", z(name)), sub(b"NAME", le(base)), sub(b"DATA", vec![0; 24])])
}

fn plugin_bytes() -> Vec<u8> {
    let npc_player = record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]);
    let txst = [edid(b"TXST", TXST1, "TestTextures1", &[]), edid(b"TXST", TXST2, "TestTextures2", &[])].concat();
    let imod = edid(b"IMOD", IMOD, "TestWeaponMod", &[]);
    let weap = [
        edid(b"WEAP", WEAP, "TestWeapon", &[sub(b"MODL", z("weapons\\test.nif")), alts(b"MODS", &[("Barrel", TXST1, 1), ("Grip", TXST2, 3)]), sub(b"WMI1", le(IMOD))]),
        edid(b"WEAP", WEAP2, "TestWeapon2", &[sub(b"WMI2", le(IMOD))]),
        edid(b"WEAP", WEAP_PLAIN, "TestWeaponPlain", &[]),
    ]
    .concat();
    // Male biped (MODL, MODS, MODD torso), male world (MOD2, MO2S), the icons, female
    // biped (MOD3, MO3S, MOSD head), female world (MOD4), the equip type (7 body wear).
    let armo = edid(
        b"ARMO",
        ARMO,
        "TestArmor",
        &[
            sub(b"MODL", z("armor\\m.nif")),
            alts(b"MODS", &[("Body", TXST1, 0)]),
            sub(b"MODD", vec![2]),
            sub(b"MOD2", z("armor\\go.nif")),
            alts(b"MO2S", &[("GO", TXST2, 2)]),
            sub(b"ICON", z("icons\\m.dds")),
            sub(b"MOD3", z("armor\\f.nif")),
            alts(b"MO3S", &[("Body", TXST2, 0)]),
            sub(b"MOSD", vec![1]),
            sub(b"MOD4", z("armor\\fgo.nif")),
            sub(b"ICO2", z("icons\\f.dds")),
            sub(b"ETYP", 7i32.to_le_bytes().to_vec()),
        ],
    );
    let alch = edid(b"ALCH", ALCH, "TestDrink", &[sub(b"ETYP", 13i32.to_le_bytes().to_vec())]);
    // ECZN DATA: owner, rank, minimum level, flags, maximum level.
    let eczn = [edid(b"ECZN", ZONE1, "TestZone1", &[sub(b"DATA", [le(0), vec![0, 1, 0, 0]].concat())]), edid(b"ECZN", ZONE2, "TestZone2", &[sub(b"DATA", [le(0), vec![0, 1, 0, 0]].concat())])].concat();
    let regn = [
        edid(b"REGN", REGN_HIGH, "TestRegionHigh", &[rdat(4, 60), sub(b"RDMP", z("High Place"))]),
        edid(b"REGN", REGN_LOW, "TestRegionLow", &[rdat(4, 10), sub(b"RDMP", z("Low Place"))]),
        edid(b"REGN", REGN_BARE, "TestRegionBare", &[]),
    ]
    .concat();
    let misc = edid(b"MISC", MISC, "TestThing", &[]);
    let wrld = edid(b"WRLD", WRLD, "TestWorld", &[sub(b"FULL", z("Test World")), sub(b"ICON", z("interface\\worldmap\\test.dds")), sub(b"XEZN", le(ZONE1))]);
    let ext = edid(b"CELL", EXT, "TestExterior", &[sub(b"DATA", vec![0]), sub(b"XCLC", u32s(&[0, 0])), sub(b"XCLR", u32s(&[REGN_LOW, REGN_HIGH, REGN_BARE]))]);
    let ext_children = group(EXT.to_le_bytes(), 6, &group(EXT.to_le_bytes(), 9, &placed(REF_OUT, "TestRefOut", MISC)));
    let ext_bare = edid(b"CELL", EXT_BARE, "TestExteriorBare", &[sub(b"DATA", vec![0]), sub(b"XCLC", u32s(&[1, 0]))]);
    let bare_children = group(EXT_BARE.to_le_bytes(), 6, &group(EXT_BARE.to_le_bytes(), 9, &placed(REF_BARE, "TestRefBare", MISC)));
    let world_children = group(WRLD.to_le_bytes(), 1, &group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &[ext, ext_children, ext_bare, bare_children].concat())));
    let room = edid(b"CELL", ROOM, "TestRoom", &[sub(b"FULL", z("Test Room")), sub(b"DATA", vec![1])]);
    let room_children = group(ROOM.to_le_bytes(), 6, &group(ROOM.to_le_bytes(), 9, &placed(REF_IN, "TestRefIn", MISC)));
    let soun = edid(b"SOUN", SOUN, "TestSound", &[]);
    let expl = edid(b"EXPL", EXPL, "TestExplosion", &[]);
    let proj = edid(b"PROJ", PROJ, "TestProjectile", &[]);
    let imad = [edid(b"IMAD", IMAD1, "TestModifier1", &[]), edid(b"IMAD", IMAD2, "TestModifier2", &[])].concat();
    // CAMS DATA: action Hit, location Target, target Attacker, flags 9, then the
    // player's, target's and global time multipliers, max, min, target %.
    let mut cams_data = u32s(&[2, 2, 0, 9]);
    for v in [0.5f32, 0.25, 1.0, 4.0, 1.5, 0.5] {
        cams_data.extend(v.to_le_bytes());
    }
    let cams = edid(b"CAMS", CAMS, "TestShot", &[sub(b"DATA", cams_data), sub(b"MNAM", le(IMAD1))]);
    // The torso with a name, the head without (its part starts at BPNN).
    let bptd = edid(
        b"BPTD",
        BPTD,
        "TestBodyParts",
        &[
            sub(b"BPTN", z("Torso")),
            sub(b"BPNN", z("Bip01 Spine")),
            sub(b"BPNT", z("Bip01 Spine2")),
            sub(b"BPNI", z("")),
            bpnd(0),
            sub(b"NAM4", z("Bip01 Spine1")),
            sub(b"BPNN", z("Bip01 Head")),
            sub(b"BPNT", z("Bip01 HeadTarget")),
            bpnd(1),
            sub(b"NAM4", z("HeadBone")),
        ],
    );
    let spel = edid(b"SPEL", SPEL, "TestPoison", &[sub(b"SPIT", [u32s(&[5, 0, 0]), vec![0; 4]].concat())]);
    let rcct = [edid(b"RCCT", RCCT, "TestCategory", &[sub(b"DATA", vec![0])]), edid(b"RCCT", RCCT_SUB, "TestSubCategory", &[sub(b"DATA", vec![1])])].concat();
    let items = [edid(b"MISC", ITEM_A, "TestInput", &[]), edid(b"MISC", ITEM_B, "TestOutput", &[])].concat();
    let rcpe = [
        edid(b"RCPE", RCPE2, "TestRecipe2", &[sub(b"DATA", u32s(&[44, 0, RCCT, RCCT_SUB])), sub(b"RCIL", le(ITEM_A)), sub(b"RCQY", le(1)), sub(b"RCOD", le(ITEM_B)), sub(b"RCQY", le(2))]),
        edid(b"RCPE", RCPE1, "TestRecipe1", &[sub(b"DATA", u32s(&[44, 0, RCCT, 0])), sub(b"RCIL", le(ITEM_B)), sub(b"RCQY", le(1)), sub(b"RCOD", le(ITEM_A)), sub(b"RCQY", le(1)), sub(b"RCOD", le(ITEM_B)), sub(b"RCQY", le(1))]),
    ]
    .concat();
    [
        tes4(),
        top(b"NPC_", &npc_player),
        top(b"TXST", &txst),
        top(b"IMOD", &imod),
        top(b"WEAP", &weap),
        top(b"ARMO", &armo),
        top(b"ALCH", &alch),
        top(b"MISC", &[misc, items].concat()),
        top(b"ECZN", &eczn),
        top(b"REGN", &regn),
        top(b"SOUN", &soun),
        top(b"EXPL", &expl),
        top(b"PROJ", &proj),
        top(b"IMAD", &imad),
        top(b"CAMS", &cams),
        top(b"BPTD", &bptd),
        top(b"SPEL", &spel),
        top(b"RCCT", &rcct),
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
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Jg5.esm".into()).unwrap()];
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

/// A map array's value at a number key, as a FormID; 0 when the key is missing.
fn map_at(g: &mut Game, expr: &str, key: i32) -> u32 {
    match q(g, &format!("array_var a\nlet a := {expr}\nif eval (ar_HasKey a {key})\nSetFunctionValue (a[{key}])\nelse\nSetFunctionValue 0\nendif")) {
        XVal::Ref(r) => r,
        XVal::Num(x) => x as u32,
        v => panic!("{expr}[{key}]: {v:?}"),
    }
}

#[test]
fn weapon_and_armor_alt_textures() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: GetWeaponAltTextures, the texture sets in record order].
    assert_eq!(forms(&mut g, "GetWeaponAltTextures TestWeapon"), vec![TXST1, TXST2]);
    assert_eq!(forms(&mut g, "GetWeaponAltTextures TestWeaponPlain"), Vec::<u32>::new());
    assert_eq!(forms(&mut g, "GetWeaponAltTextures TestArmor"), Vec::<u32>::new());
    // [documented: GetArmorAltTextures: 1 male biped, 2 female biped, 3 male world].
    assert_eq!(forms(&mut g, "GetArmorAltTextures TestArmor 1"), vec![TXST1]);
    assert_eq!(forms(&mut g, "GetArmorAltTextures TestArmor 2"), vec![TXST2]);
    assert_eq!(forms(&mut g, "GetArmorAltTextures TestArmor 3"), vec![TXST2]);
    assert_eq!(forms(&mut g, "GetArmorAltTextures TestArmor 4"), Vec::<u32>::new());
    assert_eq!(forms(&mut g, "GetArmorAltTextures TestArmor 5"), Vec::<u32>::new());
    // [documented: GetAltTexturesEx: 3D index -> texture set].
    assert_eq!(map_at(&mut g, "GetAltTexturesEx TestWeapon", 3), TXST2);
    assert_eq!(map_at(&mut g, "GetAltTexturesEx TestWeapon", 2), 0);
    assert_eq!(map_at(&mut g, "GetAltTexturesEx TestArmor 3", 2), TXST2);
    // Without a model ID, model 3 [observed: jg5, MS18RangerBattlearmor].
    assert_eq!(map_at(&mut g, "GetAltTexturesEx TestArmor", 2), TXST2);
    assert_eq!(map_at(&mut g, "GetAltTexturesEx TestArmor", 0), 0);
    // A model outside 1-4 gives no array [observed: jg5, ar_Size -1].
    for expr in ["GetArmorAltTextures TestArmor 5", "GetAltTexturesEx TestArmor 0"] {
        let size = q(&mut g, &format!("array_var a\nlet a := {expr}\nSetFunctionValue (ar_Size a)"));
        assert_eq!(size, XVal::Num(-1.0), "{expr}");
    }
    // Replace one, add one [documented: SetWeaponAltTexture and its note].
    assert_eq!(n(&mut g, "SetWeaponAltTexture TestWeapon 1 TestTextures2"), 1.0);
    assert_eq!(n(&mut g, "SetWeaponAltTexture TestWeapon 7 TestTextures1"), 1.0);
    assert_eq!(forms(&mut g, "GetWeaponAltTextures TestWeapon"), vec![TXST2, TXST2, TXST1]);
    assert_eq!(map_at(&mut g, "GetAltTexturesEx TestWeapon", 7), TXST1);
    // Not a texture set: nothing (see docs/spec-notes/ext-jg5.md).
    assert_eq!(n(&mut g, "SetWeaponAltTexture TestWeapon 1 TestThing"), 0.0);
    assert_eq!(forms(&mut g, "GetWeaponAltTextures TestWeapon"), vec![TXST2, TXST2, TXST1]);
    // [documented: ClearWeaponAltTexture, -1 for all; observed: w4c_b6_06_clear_weap_alt_tex].
    assert_eq!(n(&mut g, "ClearWeaponAltTexture TestWeapon 3"), 1.0);
    assert_eq!(n(&mut g, "ClearWeaponAltTexture TestWeapon 3"), 0.0);
    assert_eq!(forms(&mut g, "GetWeaponAltTextures TestWeapon"), vec![TXST2, TXST1]);
    assert_eq!(n(&mut g, "ClearWeaponAltTexture TestWeapon -1"), 1.0);
    assert_eq!(forms(&mut g, "GetWeaponAltTextures TestWeapon"), Vec::<u32>::new());
    // -1 still returns 1 even when no alt textures are present, while a specific empty index returns 0.
    assert_eq!(n(&mut g, "ClearWeaponAltTexture TestWeaponPlain 5"), 0.0);
    assert_eq!(n(&mut g, "ClearWeaponAltTexture TestWeaponPlain -1"), 1.0);
    // [documented: SetArmorAltTexture]: the female world model gains one.
    assert_eq!(n(&mut g, "SetArmorAltTexture TestArmor 4 5 TestTextures1"), 1.0);
    assert_eq!(forms(&mut g, "GetArmorAltTextures TestArmor 4"), vec![TXST1]);
    assert_eq!(forms(&mut g, "GetArmorAltTextures TestArmor 1"), vec![TXST1], "the other models unchanged");
    assert_eq!(n(&mut g, "SetArmorAltTexture TestArmor 9 5 TestTextures1"), 0.0);
}

#[test]
fn armor_facegen_flags_and_icons() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: GetFacegenModelFlag: 0 head, 1 torso; male MODD torso, female MOSD head].
    assert_eq!(n(&mut g, "GetFacegenModelFlag TestArmor 1 0"), 1.0);
    assert_eq!(n(&mut g, "GetFacegenModelFlag TestArmor 0 0"), 0.0);
    assert_eq!(n(&mut g, "GetFacegenModelFlag TestArmor 0 1"), 1.0);
    assert_eq!(n(&mut g, "GetFacegenModelFlag TestArmor 1 1"), 0.0);
    assert_eq!(n(&mut g, "GetFacegenModelFlag TestArmor 4 0"), 0.0);
    q(&mut g, "SetFacegenModelFlag TestArmor 1 0 0");
    q(&mut g, "SetFacegenModelFlag TestArmor 3 0 1");
    assert_eq!(n(&mut g, "GetFacegenModelFlag TestArmor 1 0"), 0.0);
    assert_eq!(n(&mut g, "GetFacegenModelFlag TestArmor 3 0"), 1.0);
    assert_eq!(n(&mut g, "GetFacegenModelFlag TestArmor 0 1"), 1.0, "the female flags unchanged");
    // [documented: SetBipedIconPathAlt `path female form`, read by GetBipedIconPath].
    assert_eq!(s(&mut g, "GetBipedIconPath 1 TestArmor"), "icons\\f.dds");
    q(&mut g, "SetBipedIconPathAlt \"icons\\new_f.dds\" 1 TestArmor");
    q(&mut g, "SetBipedIconPathAlt \"icons\\new_m.dds\" 0 TestArmor");
    assert_eq!(s(&mut g, "GetBipedIconPath 1 TestArmor"), "icons\\new_f.dds");
    assert_eq!(s(&mut g, "GetBipedIconPath 0 TestArmor"), "icons\\new_m.dds");
}

#[test]
fn worldspace_map_texture_and_zones() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: Get/SetWorldSpaceMapTexture].
    assert_eq!(s(&mut g, "GetWorldSpaceMapTexture TestWorld"), "interface\\worldmap\\test.dds");
    q(&mut g, "SetWorldSpaceMapTexture TestWorld \"interface\\worldmap\\other.dds\"");
    assert_eq!(s(&mut g, "GetWorldSpaceMapTexture TestWorld"), "interface\\worldmap\\other.dds");
    // [documented: GetWorldspaceEncounterZone, alias GetWorldZone].
    assert_eq!(n(&mut g, "GetWorldspaceEncounterZone TestWorld"), f64::from(ZONE1));
    assert_eq!(n(&mut g, "GetCellEncounterZone TestExterior"), f64::from(ZONE1), "the cell takes its worldspace's zone");
    // [documented: SetWorldspaceEncounterZone: 1 when done, none removes it].
    assert_eq!(n(&mut g, "SetWorldZone TestWorld TestZone2"), 1.0);
    assert_eq!(n(&mut g, "GetWorldZone TestWorld"), f64::from(ZONE2));
    assert_eq!(n(&mut g, "GetCellEncounterZone TestExterior"), f64::from(ZONE2));
    assert_eq!(n(&mut g, "SetWorldspaceEncounterZone TestWorld TestThing"), 0.0, "not a zone");
    assert_eq!(n(&mut g, "GetWorldZone TestWorld"), f64::from(ZONE2));
    // A cell's own zone still wins over the worldspace's.
    q(&mut g, "SetCellEncounterZone TestExterior TestZone1");
    assert_eq!(n(&mut g, "GetCellEncounterZone TestExterior"), f64::from(ZONE1));
    assert_eq!(n(&mut g, "SetWorldspaceEncounterZone TestWorld"), 1.0);
    assert_eq!(n(&mut g, "GetWorldZone TestWorld"), 0.0);
    assert_eq!(n(&mut g, "GetCellEncounterZone TestExteriorBare"), 0.0);
}

#[test]
fn region_map_names_and_location_name() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: GetRegionMapName; the field: RDMP after an RDAT of type 4].
    assert_eq!(s(&mut g, "GetRegionMapName TestRegionHigh"), "High Place");
    // No map data fails the call [observed: jg5, "script call failed"].
    let fails = |g: &mut Game, body: &str| g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).is_err();
    assert!(fails(&mut g, "string_var sOut\nlet sOut := GetRegionMapName TestRegionBare"));
    assert!(fails(&mut g, "string_var sOut\nlet sOut := GetLocationName"), "no calling reference");
    // [documented: GetLocationName; observed: jg5]: the first map name in XCLR order.
    let loc = |g: &mut Game, r: &str| match q(g, &format!("ref rX\nset rX to {r}\nstring_var sOut\nlet sOut := rX.GetLocationName\nSetFunctionValue sOut")) {
        XVal::Str(s) => s,
        v => panic!("{r}: {v:?}"),
    };
    assert_eq!(loc(&mut g, "TestRefOut"), "Low Place", "listed first, though of lower priority");
    assert_eq!(loc(&mut g, "TestRefBare"), "Test World", "no region: the worldspace's name");
    assert_eq!(loc(&mut g, "TestRefIn"), "Test Room", "indoors: the cell's name");
    // [documented: SetRegionMapName: a region without map data gains it; observed: jg5, 1].
    assert_eq!(n(&mut g, "SetRegionMapName TestRegionHigh \"Higher Place\""), 1.0);
    assert_eq!(n(&mut g, "SetRegionMapName TestRegionBare \"Bare Place\""), 1.0);
    assert_eq!(s(&mut g, "GetRegionMapName TestRegionHigh"), "Higher Place");
    assert_eq!(s(&mut g, "GetRegionMapName TestRegionBare"), "Bare Place");
    q(&mut g, "SetRegionMapName TestRegionLow \"\"");
    assert_eq!(loc(&mut g, "TestRefOut"), "Higher Place", "an empty name is passed over");
}

#[test]
fn camera_shot_traits() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: GetCameraShotFlags, GetCameraShotImageSpaceModifier].
    assert_eq!(n(&mut g, "GetCameraShotFlags TestShot"), 9.0);
    assert_eq!(n(&mut g, "GetCameraShotImageSpaceModifier TestShot"), f64::from(IMAD1));
    // [documented: GetCameraShotTraitNumeric's notes, traits 0-8].
    let want = [2.0, 2.0, 0.0, 0.5, 0.25, 1.0, 4.0, 1.5, 0.5];
    for (t, w) in want.iter().enumerate() {
        assert_eq!(n(&mut g, &format!("GetCameraShotTraitNumeric TestShot {t}")), *w, "trait {t}");
    }
    assert_eq!(n(&mut g, "GetCameraShotTraitNumeric TestShot 9"), 0.0);
    q(&mut g, "SetCameraShotFlags TestShot 33");
    q(&mut g, "SetCameraShotImageSpaceModifier TestShot TestModifier2");
    // 1 when set; a code outside the table is kept [observed: jg5, location 7 reads 7].
    assert_eq!(n(&mut g, "SetCameraShotTraitNumeric TestShot 0 1"), 1.0);
    assert_eq!(n(&mut g, "SetCameraShotTraitNumeric TestShot 1 7"), 1.0);
    assert_eq!(n(&mut g, "SetCameraShotTraitNumeric TestShot 6 2.5"), 1.0);
    assert_eq!(n(&mut g, "SetCameraShotTraitNumeric TestShot 9 1"), 0.0);
    assert_eq!(n(&mut g, "GetCameraShotFlags TestShot"), 33.0);
    assert_eq!(n(&mut g, "GetCameraShotImageSpaceModifier TestShot"), f64::from(IMAD2));
    assert_eq!(n(&mut g, "GetCameraShotTraitNumeric TestShot 0"), 1.0);
    assert_eq!(n(&mut g, "GetCameraShotTraitNumeric TestShot 1"), 7.0);
    assert_eq!(n(&mut g, "GetCameraShotTraitNumeric TestShot 6"), 2.5);
    // The kill camera reads the changed shot, keeping the last valid location.
    let shot = fnv_game::nvse_jg5::camera_shot(&g, FormId(CAMS)).unwrap();
    assert_eq!((shot.flags, shot.imod, shot.max_time), (33, Some(FormId(IMAD2)), 2.5));
    assert_eq!(shot.location, fnv_game::camera_paths::ShotActor::Target);
    q(&mut g, "SetCameraShotImageSpaceModifier TestShot TestThing");
    assert_eq!(n(&mut g, "GetCameraShotImageSpaceModifier TestShot"), f64::from(IMAD2), "not a modifier");
    q(&mut g, "SetCameraShotImageSpaceModifier TestShot");
    assert_eq!(n(&mut g, "GetCameraShotImageSpaceModifier TestShot"), 0.0);
}

#[test]
fn body_part_trait_strings() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: GetBodyPartTraitString: 1 part node, 2 VATS target, 3 start node,
    // 4 part name, 5 target bone].
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 0 1"), "Bip01 Spine");
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 0 2"), "Bip01 Spine2");
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 0 3"), "");
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 0 4"), "Torso");
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 0 5"), "Bip01 Spine1");
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 1 1"), "Bip01 Head");
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 1 2"), "Bip01 HeadTarget");
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 1 4"), "", "the head has no name");
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 1 5"), "HeadBone");
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 2 1"), "", "no such part");
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 0 6"), "", "no such trait");
    // SetBodyPartName's change (JIP batch 8) reads back as trait 4; the other traits stay.
    q(&mut g, "SetBodyPartName TestBodyParts 0 \"Chest\"");
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 0 4"), "Chest");
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 0 1"), "Bip01 Spine");
    q(&mut g, "SetBodyPartName TestBodyParts 2 \"Tail\"");
    assert_eq!(s(&mut g, "GetBodyPartTraitString TestBodyParts 2 4"), "", "no such part");
}

#[test]
fn sounds_equip_types_and_lookups() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: SetExplosionSound], read by GetExplosionTraitForm (1 sound 1, 2 sound 2).
    q(&mut g, "SetExplosionSound TestExplosion 2 TestSound");
    assert_eq!(n(&mut g, "GetExplosionTraitForm TestExplosion 2"), f64::from(SOUN));
    q(&mut g, "SetExplosionSound TestExplosion 1 TestThing");
    assert_eq!(n(&mut g, "GetExplosionTraitForm TestExplosion 1"), 0.0, "not a sound");
    // [documented: SetProjectileSound]; nothing reads it back yet.
    q(&mut g, "SetProjectileSound TestProjectile 2 TestSound");
    // [documented: SetEquipType, read by GetEquipType].
    assert_eq!(n(&mut g, "GetEquipType TestDrink"), 13.0);
    // 0-13 taken, giving 1; -1 and 40 refused [observed: jg5, the Stimpak].
    assert_eq!(n(&mut g, "SetEquipType TestDrink 12"), 1.0);
    assert_eq!(n(&mut g, "GetEquipType TestDrink"), 12.0);
    assert_eq!(n(&mut g, "SetEquipType TestDrink 40"), 0.0);
    assert_eq!(n(&mut g, "SetEquipType TestDrink -1"), 0.0);
    assert_eq!(n(&mut g, "GetEquipType TestDrink"), 12.0, "outside the table");
    // [documented: GetRecipeCategoryFlags, 1 a subcategory].
    assert_eq!(n(&mut g, "GetRecipeCategoryFlags TestSubCategory"), 1.0);
    assert_eq!(n(&mut g, "GetRecipeCategoryFlags TestCategory"), 0.0);
    // [documented: GetFormRecipesAlt, the recipes that output the item].
    assert_eq!(forms(&mut g, "GetFormRecipesAlt TestOutput"), vec![RCPE1, RCPE2]);
    assert_eq!(forms(&mut g, "GetFormRecipesAlt TestInput"), vec![RCPE1]);
    // [documented: GetWeaponsForMod; observed: w4c_b3_02_region_loc_weapmod, non-IMOD returns ar_Null].
    assert_eq!(forms(&mut g, "GetWeaponsForMod TestWeaponMod"), vec![WEAP, WEAP2]);
    assert_eq!(n(&mut g, "ar_Size (GetWeaponsForMod TestThing)"), -1.0, "non-IMOD returns ar_Null");
    // [documented: GetActorEffectType: 5 poison, -1 for anything else].
    assert_eq!(n(&mut g, "GetActorEffectType TestPoison"), 5.0);
    assert_eq!(n(&mut g, "GetActorEffectType TestThing"), -1.0);
}

#[test]
fn time_played_counts_up() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: GetTimePlayed: 0 milliseconds, 1 seconds, 2 minutes].
    let ms = n(&mut g, "GetTimePlayed 0");
    let secs = n(&mut g, "GetTimePlayed 1");
    let mins = n(&mut g, "GetTimePlayed 2");
    // Each read is a little later than the one before.
    assert!(ms >= 0.0 && secs >= ms / 1000.0 && mins * 60.0 >= secs && mins * 60.0 < secs + 1.0, "{ms} {secs} {mins}");
    assert_eq!(n(&mut g, "GetTimePlayed 3"), 0.0, "another unit [observed: jg5]");
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert!(n(&mut g, "GetTimePlayed 0") >= ms + 15.0);
}

#[test]
fn rgb_color_packs_channels() {
    let f = fixture();
    let mut g = game(&f);
    // [observed: jg5, R × 65536 + G × 256 + B].
    assert_eq!(n(&mut g, "GetRGBColor 255 128 1"), 16744449.0);
    assert_eq!(n(&mut g, "GetRGBColor 1 0 0"), 65536.0);
    assert_eq!(n(&mut g, "GetRGBColor 0 0 1"), 1.0);
    assert_eq!(n(&mut g, "GetRGBColor 255 255 255"), 16777215.0);
    // Any channel outside 0..=255 returns 0 [observed: Wave 4 jg_16_rgb_color_out_of_range,
    // JohnnyGuitarNVSE 528].
    assert_eq!(n(&mut g, "GetRGBColor 256 0 0"), 0.0);
    assert_eq!(n(&mut g, "GetRGBColor 0 256 0"), 0.0);
    assert_eq!(n(&mut g, "GetRGBColor -1 0 0"), 0.0);
}
