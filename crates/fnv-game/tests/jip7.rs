//! JIP LN NVSE batch 7 (#1006; docs/spec-notes/jip-records-7.md) on a synthetic plugin:
//! ammunition, ammo effects, projectiles, explosions, impact data, weathers and climates,
//! armour.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const AMMO: u32 = 0x900;
const CASING: u32 = 0x901;
const AMEF: u32 = 0x902;
const AMEF2: u32 = 0x903;
const PROJ: u32 = 0x904;
const PROJ2: u32 = 0x905;
const LIGH: u32 = 0x906;
const EXPL: u32 = 0x907;
const IPCT: u32 = 0x908;
const TXST: u32 = 0x909;
const SOUN: u32 = 0x90A;
const IPDS: u32 = 0x90B;
const WEAP: u32 = 0x90C;
const CREA: u32 = 0x90D;
const WTHR: u32 = 0x90E;
const WTHR2: u32 = 0x90F;
const CLMT: u32 = 0x910;
const GLOB: u32 = 0x911;
const ARMO: u32 = 0x912;
const IPCT_BARE: u32 = 0x913;
const IPCT_NONE: u32 = 0x914;
const TXST_DECAL: u32 = 0x915;
const TXST_LAND: u32 = 0x916;
const TXST_FLAG: u32 = 0x917;
const LTEX: u32 = 0x918;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn edid(sig: &[u8; 4], fid: u32, name: &str, rest: &[fnv_esm::raw::Subrecord]) -> Vec<u8> {
    record(sig, 0, fid, &[vec![sub(b"EDID", z(name))], rest.to_vec()].concat())
}

/// PROJ DATA [documented: schemas/fnv ProjectileData]: flags u16, type u16 (1 missile),
/// gravity, speed 1000, range 5000, light, muzzle flash light, tracer chance, proximity,
/// timer, explosion, sound, muzzle flash duration, fade, impact force, three sounds and
/// the default weapon, rotation, bouncy mult.
fn proj_data() -> Vec<u8> {
    [vec![0, 0], 1u16.to_le_bytes().to_vec(), f32b(0.0), f32b(1000.0), f32b(5000.0), u32s(&[0, 0]), f32b(0.0), f32b(0.0), f32b(0.0), u32s(&[0, 0]), f32b(0.0), f32b(0.0), f32b(0.0), u32s(&[0, 0, 0]), f32b(0.0), f32b(0.0), f32b(0.0), f32b(0.5)].concat()
}

/// EXPL DATA [documented: ExplosionData]: force, damage 100, radius 300, light, sound 1,
/// flags, IS radius, impact dataset, sound 2, radiation level, dissipation, radius, sound
/// level.
fn expl_data() -> Vec<u8> {
    [f32b(0.0), f32b(100.0), f32b(300.0), u32s(&[0, 0, 0]), f32b(0.0), u32s(&[0, 0]), f32b(0.0), f32b(0.0), f32b(0.0), u32s(&[1])].concat()
}

fn plugin_bytes() -> Vec<u8> {
    let npc = record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]);
    // DATA: speed 2000, flags, unused, value, clip rounds 8; DAT2: 1 projectile per shot,
    // the projectile, weight, the casing, consumed 0.5.
    let ammo = edid(
        b"AMMO",
        AMMO,
        "TestAmmo",
        &[
            sub(b"DATA", [f32b(2000.0), vec![0, 0, 0, 0], 5i32.to_le_bytes().to_vec(), vec![8]].concat()),
            sub(b"DAT2", [u32s(&[1, PROJ]), f32b(0.1), u32s(&[CASING]), f32b(0.5)].concat()),
            sub(b"RCIL", u32s(&[AMEF])),
        ],
    );
    let misc = edid(b"MISC", CASING, "TestCasing", &[]);
    // DT Mod (2), subtract (2), 3.
    let amef = [edid(b"AMEF", AMEF, "TestAmmoEffect", &[sub(b"DATA", [u32s(&[2, 2]), f32b(3.0)].concat())]), edid(b"AMEF", AMEF2, "TestAmmoEffect2", &[sub(b"DATA", [u32s(&[0, 1]), f32b(1.5)].concat())])].concat();
    let proj = [edid(b"PROJ", PROJ, "TestProjectile", &[sub(b"DATA", proj_data()), sub(b"NAM1", z("flash.nif")), sub(b"VNAM", u32s(&[1]))]), edid(b"PROJ", PROJ2, "TestProjectile2", &[sub(b"DATA", proj_data())])].concat();
    let ligh = edid(b"LIGH", LIGH, "TestLight", &[]);
    let expl = edid(b"EXPL", EXPL, "TestExplosion", &[sub(b"DATA", expl_data())]);
    // IPCT DATA: duration 2, orientation 1, angle 15, radius 4, sound level 1, flags 0;
    // DODT: widths 8 and 16, heights 8 and 16, depth 4, shininess 0.5, parallax scale,
    // passes 4, flags 2 (alpha blending), colour 255 128 0.
    let dodt = [f32b(8.0), f32b(16.0), f32b(8.0), f32b(16.0), f32b(4.0), f32b(0.5), f32b(0.04), vec![4, 2, 0, 0, 255, 128, 0, 0]].concat();
    let ipct = edid(
        b"IPCT",
        IPCT,
        "TestImpact",
        &[sub(b"DATA", [f32b(2.0), u32s(&[1]), f32b(15.0), f32b(4.0), u32s(&[1, 0])].concat()), sub(b"DODT", dodt), sub(b"DNAM", u32s(&[TXST])), sub(b"SNAM", u32s(&[SOUN]))],
    );
    let ipct = [
        ipct,
        // Without decal data: one with a texture set whose DODT colour bytes are 64 00 34 00,
        // one with no texture set (jip-records-7.md, the colour's packing).
        edid(b"IPCT", IPCT_BARE, "TestImpactBare", &[sub(b"DATA", [f32b(2.0), u32s(&[1]), f32b(15.0), f32b(4.0), u32s(&[1, 0])].concat()), sub(b"DNAM", u32s(&[TXST_DECAL]))]),
        edid(b"IPCT", IPCT_NONE, "TestImpactNone", &[sub(b"DATA", [f32b(2.0), u32s(&[1]), f32b(15.0), f32b(4.0), u32s(&[1, 0])].concat())]),
    ]
    .concat();
    let decal = [f32b(8.0), f32b(16.0), f32b(8.0), f32b(16.0), f32b(4.0), f32b(0.5), f32b(0.04), vec![4, 0, 0, 0, 0x64, 0, 0x34, 0]].concat();
    let txst = [
        edid(b"TXST", TXST, "TestDecal", &[]),
        edid(b"TXST", TXST_DECAL, "TestDecalColoured", &[sub(b"DODT", decal)]),
        // A landscape texture's set with DNAM 0, and another set with DNAM 1 (jip-records-5.md).
        edid(b"TXST", TXST_LAND, "TestLandSet", &[sub(b"DNAM", vec![0, 0])]),
        edid(b"TXST", TXST_FLAG, "TestFlaggedSet", &[sub(b"DNAM", vec![1, 0])]),
    ]
    .concat();
    let ltex = edid(b"LTEX", LTEX, "TestLand", &[sub(b"TNAM", u32s(&[TXST_LAND]))]);
    let soun = edid(b"SOUN", SOUN, "TestSound", &[]);
    let ipds = edid(b"IPDS", IPDS, "TestImpactSet", &[]);
    let weap = edid(b"WEAP", WEAP, "TestGun", &[]);
    let crea = edid(b"CREA", CREA, "TestCreature", &[sub(b"NAM4", u32s(&[6]))]);
    let wthr = [
        edid(b"WTHR", WTHR, "TestWeather", &[sub(b"DNAM", z("sky\\layer0.dds")), sub(b"ANAM", z("sky\\layer2.dds")), sub(b"MODL", z("weather\\rain.nif"))]),
        edid(b"WTHR", WTHR2, "TestWeather2", &[]),
    ]
    .concat();
    // One weather, chance 60, no global.
    let clmt = edid(b"CLMT", CLMT, "TestClimate", &[sub(b"WLST", u32s(&[WTHR, 60, 0]))]);
    let glob = edid(b"GLOB", GLOB, "TestGlobal", &[sub(b"FNAM", vec![b's']), sub(b"FLTV", f32b(1.0))]);
    // BMDT: biped flags, general flags 0x08 (Medium), unused.
    let armo = edid(b"ARMO", ARMO, "TestArmor", &[sub(b"BMDT", [u32s(&[4]), vec![0x08, 0, 0, 0]].concat()), sub(b"BMCT", z("armor.rdt"))]);
    [
        tes4(),
        top(b"NPC_", &npc),
        top(b"AMMO", &ammo),
        top(b"MISC", &misc),
        top(b"AMEF", &amef),
        top(b"PROJ", &proj),
        top(b"LIGH", &ligh),
        top(b"EXPL", &expl),
        top(b"IPCT", &ipct),
        top(b"TXST", &txst),
        top(b"LTEX", &ltex),
        top(b"SOUN", &soun),
        top(b"IPDS", &ipds),
        top(b"WEAP", &weap),
        top(b"CREA", &crea),
        top(b"WTHR", &wthr),
        top(b"CLMT", &clmt),
        top(b"GLOB", &glob),
        top(b"ARMO", &armo),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Jip7.esm".into()).unwrap()];
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

/// The value a query's last line sets.
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

#[test]
fn ammunition_and_its_effects() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: SetAmmoTraitNumeric's table: 0 speed, 1 projectiles per shot, 2 ammo
    // consumed percent, 3 clip rounds], read through the earlier getters.
    q(&mut g, "SetAmmoTraitNumeric TestAmmo 0 2500");
    q(&mut g, "SetAmmoTraitNumeric TestAmmo 2 0.25");
    q(&mut g, "SetAmmoTraitNumeric TestAmmo 3 12");
    assert_eq!(n(&mut g, "GetAmmoSpeed TestAmmo"), 2500.0);
    assert_eq!(n(&mut g, "GetAmmoConsumedPercent TestAmmo"), 0.25);
    assert_eq!(g.num_field(FormId(AMMO), "DATA", "clip_rounds"), 12.0);
    q(&mut g, "SetAmmoTraitNumeric TestAmmo 1 3");
    assert_eq!(g.num_field(FormId(AMMO), "DAT2", "projectiles_per_shot"), 3.0);
    // Projectile and casing; left out, none [documented].
    q(&mut g, "SetAmmoProjectile TestAmmo TestProjectile2");
    assert_eq!(g.num_field(FormId(AMMO), "DAT2", "projectile"), f64::from(PROJ2));
    assert_eq!(n(&mut g, "GetAmmoCasing TestAmmo"), f64::from(CASING));
    q(&mut g, "SetAmmoCasing TestAmmo");
    assert_eq!(n(&mut g, "GetAmmoCasing TestAmmo"), 0.0);
    // The effects list: 1 when it changed.
    assert_eq!(n(&mut g, "GetNumAmmoEffects TestAmmo"), 1.0);
    assert_eq!(n(&mut g, "AddAmmoEffect TestAmmo TestAmmoEffect2"), 1.0);
    assert_eq!(n(&mut g, "AddAmmoEffect TestAmmo TestAmmoEffect2"), 0.0);
    assert_eq!((n(&mut g, "GetNumAmmoEffects TestAmmo"), n(&mut g, "GetNthAmmoEffect TestAmmo 1")), (2.0, f64::from(AMEF2)));
    assert_eq!(n(&mut g, "RemoveAmmoEffect TestAmmo TestAmmoEffect"), 1.0);
    assert_eq!(n(&mut g, "RemoveAmmoEffect TestAmmo TestAmmoEffect"), 0.0);
    assert_eq!((n(&mut g, "GetNumAmmoEffects TestAmmo"), n(&mut g, "GetNthAmmoEffect TestAmmo 0")), (1.0, f64::from(AMEF2)));
    // An effect's traits [documented: SetAmmoEffectTraitNumeric's table].
    assert_eq!(n(&mut g, "GetAmmoEffectTraitNumeric TestAmmoEffect 2"), 3.0);
    q(&mut g, "SetAmmoEffectTraitNumeric TestAmmoEffect 0 3");
    q(&mut g, "SetAmmoEffectTraitNumeric TestAmmoEffect 2 0.8");
    assert_eq!((n(&mut g, "GetAmmoEffectTraitNumeric TestAmmoEffect 0"), n(&mut g, "GetAmmoEffectTraitNumeric TestAmmoEffect 2")), (3.0, 0.8));
}

#[test]
fn cloned_projectile_numeric_traits_keep_the_clone_and_original_independent() {
    let f = fixture();
    let mut g = game(&f);
    let XVal::Ref(clone) = q(&mut g, "ref ownClone\nlet ownClone := TempCloneForm TestProjectile 1\nSetFunctionValue ownClone") else { panic!("clone reference") };
    assert_eq!(g.num_field(FormId(clone), "DATA", "speed"), 1000.0);
    assert_eq!(n(&mut g, &format!("GetProjectileTraitNumeric {clone:#010X} 2")), 1000.0);
    q(&mut g, &format!("SetProjectileTraitNumeric {clone:#010X} 2 0"));
    assert_eq!(g.num_field(FormId(clone), "DATA", "speed"), 0.0);
    assert_eq!(n(&mut g, &format!("GetProjectileTraitNumeric {clone:#010X} 2")), 0.0);
    assert_eq!(g.num_field(FormId(PROJ), "DATA", "speed"), 1000.0);
    // Wrong signature and out-of-range trait IDs still change nothing.
    q(&mut g, "SetProjectileTraitNumeric TestAmmo 2 17");
    q(&mut g, &format!("SetProjectileTraitNumeric {clone:#010X} 99 17"));
    assert_eq!(g.num_field(FormId(clone), "DATA", "speed"), 0.0);
    let next = game(&f);
    assert_eq!(next.num_field(FormId(PROJ), "DATA", "speed"), 1000.0);
}

#[test]
fn cloned_projectile_light_edits_keep_the_original_independent() {
    let f = fixture();
    let mut g = game(&f);
    let XVal::Ref(clone) = q(&mut g, "ref ownClone\nlet ownClone := TempCloneForm TestProjectile 1\nSetFunctionValue ownClone") else { panic!("clone reference") };
    q(&mut g, &format!("SetProjectileLight {clone:#010X} TestLight"));
    assert_eq!(g.num_field(FormId(clone), "DATA", "light"), f64::from(LIGH));
    assert_eq!(n(&mut g, &format!("GetProjectileLight {clone:#010X}")), f64::from(LIGH));
    assert_eq!(g.num_field(FormId(PROJ), "DATA", "light"), 0.0);
    q(&mut g, &format!("SetProjectileLight {clone:#010X}"));
    assert_eq!(g.num_field(FormId(clone), "DATA", "light"), 0.0);
}

#[test]
fn cloned_projectile_muzzle_flash_edits_keep_the_original_independent() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(s(&mut g, "GetProjectileMuzzleFlash TestProjectile"), "flash.nif");
    let XVal::Ref(clone) = q(&mut g, "ref ownClone\nlet ownClone := TempCloneForm TestProjectile 1\nSetFunctionValue ownClone") else { panic!("clone reference") };
    q(&mut g, &format!(r#"SetProjectileMuzzleFlash {clone:#010X} "owned\clone-flash.nif""#));
    assert_eq!(s(&mut g, &format!("GetProjectileMuzzleFlash {clone:#010X}")), "owned\\clone-flash.nif");
    assert_eq!(s(&mut g, "GetProjectileMuzzleFlash TestProjectile"), "flash.nif");
}

#[test]
fn projectiles_explosions_and_impact_data() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: SetProjectileTraitNumeric's table].
    q(&mut g, "SetProjectileTraitNumeric TestProjectile 2 3000");
    q(&mut g, "SetProjectileTraitNumeric TestProjectile 12 90");
    q(&mut g, "SetProjectileTraitNumeric TestProjectile 14 2");
    let p = FormId(PROJ);
    assert_eq!((g.num_field(p, "DATA", "speed"), g.num_field(p, "DATA", "rotation.z"), g.num_field(p, "VNAM", "")), (3000.0, 90.0, 2.0));
    assert_eq!(g.num_field(p, "DATA", "range"), 5000.0, "the others stay");
    // [documented: SetExplosionTraitNumeric's table].
    q(&mut g, "SetExplosionTraitNumeric TestExplosion 2 1500");
    assert_eq!((g.num_field(FormId(EXPL), "DATA", "radius"), g.num_field(FormId(EXPL), "DATA", "damage")), (1500.0, 100.0));
    // Impact data [documented: GetImpactDataTrait's tables].
    for (t, want) in [(0, 2.0), (1, 1.0), (2, 15.0), (3, 4.0), (4, 1.0), (5, f64::from(TXST)), (6, f64::from(SOUN)), (7, 0.0), (8, 8.0), (9, 16.0), (12, 4.0), (13, 0.5), (15, 4.0), (16, 4.0), (17, 255_128_000.0), (18, 0.0)] {
        assert_eq!(n(&mut g, &format!("GetImpactDataTrait TestImpact {t}")), want, "trait {t}");
    }
    // The setter numbers the decal data from 5: 10 is the shininess, 13 the flags, 14 the
    // colour [documented: SetImpactDataTraitNumeric's table].
    q(&mut g, "SetImpactDataTraitNumeric TestImpact 10 3");
    q(&mut g, "SetImpactDataTraitNumeric TestImpact 13 9");
    q(&mut g, "SetImpactDataTraitNumeric TestImpact 14 1002003");
    assert_eq!(n(&mut g, "GetImpactDataTrait TestImpact 13"), 3.0);
    assert_eq!(n(&mut g, "GetImpactDataTrait TestImpact 16"), 9.0);
    assert_eq!(n(&mut g, "GetImpactDataTrait TestImpact 17"), 1_002_003.0);
    q(&mut g, "SetImpactDataTraitForm TestImpact 2 TestSound");
    assert_eq!(n(&mut g, "GetImpactDataTrait TestImpact 7"), f64::from(SOUN));
    q(&mut g, "SetImpactDataSet TestGun TestImpactSet");
    assert_eq!(g.num_field(FormId(WEAP), "INAM", ""), f64::from(IPDS));
    // The impact material [documented: Get/SetImpactMaterialType, 6 Organic, 10 Organic Bug].
    assert_eq!(n(&mut g, "GetImpactMaterialType TestCreature"), 6.0);
    q(&mut g, "SetImpactMaterialType 10 TestCreature");
    assert_eq!(n(&mut g, "GetImpactMaterialType TestCreature"), 10.0);
    q(&mut g, "SetImpactMaterialType 12 TestCreature");
    assert_eq!(n(&mut g, "GetImpactMaterialType TestCreature"), 10.0, "12 is past the table");
}

/// The impact colour's packing and the texture-set flag 0 (jip-records-7.md and
/// jip-records-5.md, #2039).
#[test]
fn impact_colour_packing_and_texture_set_flag() {
    let f = fixture();
    let mut g = game(&f);
    // Set as R + G × 256 + B × 65536, carries included; read B as the upper 16 bits.
    for (set, read) in [(256_000.0, 1.0), (999_000.0, 231_003.0), (254_999_999.0, 254_232_002.0), (16_777_215.0, 16_009_218.0), (300.0, 300.0), (1256.0, 1256.0), (65535.0, 65535.0), (65536.0, 65536.0), (255_255_255.0, 255_255_255.0)] {
        q(&mut g, &format!("SetImpactDataTraitNumeric TestImpact 14 {set}"));
        assert_eq!(n(&mut g, "GetImpactDataTrait TestImpact 17"), read, "set {set}");
    }
    // Past 255255255 or below 0 nothing is set.
    for bad in ["255255256", "-1"] {
        q(&mut g, &format!("SetImpactDataTraitNumeric TestImpact 14 {bad}"));
        assert_eq!(n(&mut g, "GetImpactDataTrait TestImpact 17"), 255_255_255.0, "set {bad}");
    }
    // No DODT: the texture set's colour (64 00 34 00), or all FF without a texture set.
    assert_eq!(n(&mut g, "GetImpactDataTrait TestImpactBare 17"), 100_000_052.0);
    assert_eq!(n(&mut g, "GetImpactDataTrait TestImpactNone 17"), 255_320_535.0);
    q(&mut g, "SetImpactDataTraitNumeric TestImpactNone 14 300300300");
    assert_eq!(n(&mut g, "GetImpactDataTrait TestImpactNone 17"), 255_320_535.0, "past 255255255");
    q(&mut g, "SetImpactDataTraitNumeric TestImpactNone 14 10020030");
    assert_eq!(n(&mut g, "GetImpactDataTrait TestImpactNone 17"), 10_020_030.0);
    // Flag 0: 1 for a texture set a landscape texture names, whatever its DNAM; DNAM bit 0
    // for the others, 0 without DNAM.
    assert_eq!(n(&mut g, "GetTextureSetFlag TestLandSet 0"), 1.0);
    assert_eq!(n(&mut g, "GetTextureSetFlag TestFlaggedSet 0"), 1.0);
    assert_eq!(n(&mut g, "GetTextureSetFlag TestDecalColoured 0"), 0.0);
    assert_eq!(n(&mut g, "GetTextureSetFlag TestDecal 0"), 0.0);
}

#[test]
fn weathers_and_climates() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(s(&mut g, "GetWeatherPrecipitationModel TestWeather"), "weather\\rain.nif");
    assert_eq!(s(&mut g, "GetWeatherTexture TestWeather 0"), "sky\\layer0.dds");
    assert_eq!(s(&mut g, "GetWeatherTexture TestWeather 1"), "");
    assert_eq!(s(&mut g, "GetWeatherTexture TestWeather 2"), "sky\\layer2.dds");
    q(&mut g, "SetWeatherTexture TestWeather 1 \"sky\\new.dds\"");
    q(&mut g, "SetWeatherPrecipitationModel TestWeather \"weather\\snow.nif\"");
    assert_eq!(s(&mut g, "GetWeatherTexture TestWeather 1"), "sky\\new.dds");
    assert_eq!(s(&mut g, "GetWeatherPrecipitationModel TestWeather"), "weather\\snow.nif");
    // The transition override: 0 or none removes it [documented].
    q(&mut g, "SetWeatherTransitionTimeOverride 90");
    assert_eq!(g.weather_transition_override(), Some(90.0));
    q(&mut g, "SetWeatherTransitionTimeOverride");
    assert_eq!(g.weather_transition_override(), None);
    // The climate's weathers [documented: ClimateAddWeatherType: one already listed takes
    // the chance and global].
    assert_eq!(n(&mut g, "GetClimateNumWeatherTypes TestClimate"), 1.0);
    assert_eq!(n(&mut g, "ClimateAddWeatherType TestClimate TestWeather2 30 TestGlobal"), 1.0);
    assert_eq!(n(&mut g, "ClimateAddWeatherType TestClimate TestWeather 45"), 1.0);
    assert_eq!(n(&mut g, "GetClimateNumWeatherTypes TestClimate"), 2.0);
    assert_eq!((n(&mut g, "GetClimateNthWeatherChance TestClimate 0"), n(&mut g, "GetClimateNthWeatherGlobal TestClimate 1")), (45.0, f64::from(GLOB)));
    assert_eq!(n(&mut g, "ClimateRemoveWeatherType TestClimate TestWeather"), 1.0);
    assert_eq!(n(&mut g, "ClimateRemoveWeatherType TestClimate TestWeather"), 0.0);
    assert_eq!((n(&mut g, "GetClimateNumWeatherTypes TestClimate"), n(&mut g, "GetClimateNthWeatherType TestClimate 0")), (1.0, f64::from(WTHR2)));
}

#[test]
fn armour() {
    let f = fixture();
    let mut g = game(&f);
    // [documented: SetArmorClass's table, 1 light, 2 medium, 3 heavy].
    assert_eq!(g.armor_class(FormId(ARMO)), 2);
    q(&mut g, "SetArmorClass TestArmor 3");
    assert_eq!(g.armor_class(FormId(ARMO)), 3);
    q(&mut g, "SetArmorClass TestArmor 1");
    assert_eq!(g.armor_class(FormId(ARMO)), 1);
    q(&mut g, "SetArmorClass TestArmor 4");
    assert_eq!(g.armor_class(FormId(ARMO)), 1, "4 is past the table");
    q(&mut g, "SetArmorRCT TestArmor \"other.rdt\"");
    // Each returns the value before the call; left out, it is unchanged [documented;
    // observed: jip7 armor.condpenalty, armor.maxpercent].
    assert_eq!(n(&mut g, "SetArmorConditionPenalty"), 1.0);
    assert_eq!(n(&mut g, "SetArmorConditionPenalty 1.5"), 1.0);
    assert_eq!(n(&mut g, "SetArmorConditionPenalty"), 1.5);
    assert_eq!(n(&mut g, "SetDamageToArmorMaxPercent 0.025"), 1.0);
    assert_eq!(n(&mut g, "SetDamageToArmorMaxPercent"), 0.025);
}

/// NaN and infinity set nothing (#1358 review); a number past the field is taken as the
/// original takes it [observed: jip7, jip7b].
#[test]
fn setters_keep_their_fields_in_range() {
    let f = fixture();
    let mut g = game(&f);
    const INF: &str = "(Pow 10 400)";
    const NAN: &str = "((Pow 10 400) - (Pow 10 400))";
    // Ammunition: clip rounds wrap as a byte, projectiles per shot keep their sign.
    q(&mut g, "SetAmmoTraitNumeric TestAmmo 3 300");
    assert_eq!(g.num_field(FormId(AMMO), "DATA", "clip_rounds"), 44.0);
    q(&mut g, "SetAmmoTraitNumeric TestAmmo 1 -4");
    assert_eq!(g.num_field(FormId(AMMO), "DAT2", "projectiles_per_shot"), -4.0);
    for bad in [INF, NAN] {
        q(&mut g, &format!("SetAmmoTraitNumeric TestAmmo 0 {bad}"));
        assert_eq!(n(&mut g, "GetAmmoSpeed TestAmmo"), 2000.0, "{bad}");
        q(&mut g, &format!("SetAmmoEffectTraitNumeric TestAmmoEffect 2 {bad}"));
        assert_eq!(n(&mut g, "GetAmmoEffectTraitNumeric TestAmmoEffect 2"), 3.0, "{bad}");
        // Projectile and explosion traits.
        q(&mut g, &format!("SetProjectileTraitNumeric TestProjectile 2 {bad}"));
        assert_eq!(g.num_field(FormId(PROJ), "DATA", "speed"), 1000.0, "{bad}");
        q(&mut g, &format!("SetExplosionTraitNumeric TestExplosion 2 {bad}"));
        assert_eq!(g.num_field(FormId(EXPL), "DATA", "radius"), 300.0, "{bad}");
        q(&mut g, &format!("SetImpactDataTraitNumeric TestImpact 0 {bad}"));
        assert_eq!(n(&mut g, "GetImpactDataTrait TestImpact 0"), 2.0, "{bad}");
    }
    // An effect type past 0-5 and a projectile sound level other than 1-2 set nothing.
    q(&mut g, "SetAmmoEffectTraitNumeric TestAmmoEffect 0 9");
    assert_eq!(n(&mut g, "GetAmmoEffectTraitNumeric TestAmmoEffect 0"), 2.0, "the fixture's type kept");
    q(&mut g, "SetProjectileTraitNumeric TestProjectile 14 7");
    assert_eq!(g.num_field(FormId(PROJ), "VNAM", ""), 1.0, "7 sets nothing");
    // The weather transition: infinity changes nothing.
    q(&mut g, "SetWeatherTransitionTimeOverride 90");
    q(&mut g, &format!("SetWeatherTransitionTimeOverride {INF}"));
    assert_eq!(g.weather_transition_override(), Some(90.0));
    // Armour: NaN and infinity set nothing; the penalty is 0-2, the share 0-1.
    for bad in [INF, NAN] {
        assert_eq!(n(&mut g, &format!("SetArmorConditionPenalty {bad}")), 1.0, "{bad}");
        assert_eq!(n(&mut g, &format!("SetDamageToArmorMaxPercent {bad}")), 1.0, "{bad}");
    }
    // The penalty takes any number, the share only 0-1.
    assert_eq!(n(&mut g, "SetArmorConditionPenalty -1"), 1.0);
    assert_eq!(n(&mut g, "SetArmorConditionPenalty 7"), -1.0);
    assert_eq!(n(&mut g, "SetArmorConditionPenalty"), 7.0);
    assert_eq!(n(&mut g, "SetDamageToArmorMaxPercent 3"), 1.0);
    assert_eq!(n(&mut g, "SetDamageToArmorMaxPercent"), 1.0);
    // A climate chance that is NaN adds nothing.
    assert_eq!(n(&mut g, &format!("ClimateAddWeatherType TestClimate TestWeather2 {NAN}")), 0.0);
    assert_eq!(n(&mut g, "GetClimateNumWeatherTypes TestClimate"), 1.0);
}

#[test]
fn projectile_flag_overrides_reach_launch_traits_and_clones() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetPluginVersion \"lStewieAl's Tweaks\""), 960.0);

    let proj = FormId(PROJ);
    assert_eq!(g.projectile_flags_override(proj), None);
    assert_eq!(g.projectile_flags(proj), Some(0));
    assert!(!g.projectile_can_be_disabled(proj));
    assert_eq!(g.projectile_launch_traits(proj).unwrap().flags, None);

    // Toggle Hitscan (0), Explosion (1), Can Be Disabled (5), Supersonic (7).
    q(&mut g, "SetProjectileFlag TestProjectile 0 1");
    q(&mut g, "SetProjectileFlag TestProjectile 1 1");
    q(&mut g, "SetProjectileFlag TestProjectile 5 1");
    q(&mut g, "SetProjectileFlag TestProjectile 7 1");
    let want = (1u16 << 0) | (1u16 << 1) | (1u16 << 5) | (1u16 << 7);
    assert_eq!(n(&mut g, "GetProjectileFlag TestProjectile 0"), 1.0);
    assert_eq!(n(&mut g, "GetProjectileFlag TestProjectile 5"), 1.0);
    assert_eq!(n(&mut g, "GetProjectileFlag TestProjectile 7"), 1.0);
    assert_eq!(g.projectile_flags_override(proj), Some(want));
    assert_eq!(g.projectile_flags(proj), Some(want));
    assert!(g.projectile_can_be_disabled(proj));
    assert_eq!(g.projectile_launch_traits(proj).unwrap().flags, Some(want));

    // Clear Hitscan (0) (e.g. Bullet Drop) and Can Be Disabled (5).
    q(&mut g, "SetProjectileFlag TestProjectile 0 0");
    q(&mut g, "SetProjectileFlag TestProjectile 5 0");
    let after = (1u16 << 1) | (1u16 << 7);
    assert_eq!(n(&mut g, "GetProjectileFlag TestProjectile 0"), 0.0);
    assert_eq!(g.projectile_flags_override(proj), Some(after));
    assert!(!g.projectile_can_be_disabled(proj));
    assert_eq!(g.projectile_launch_traits(proj).unwrap().flags, Some(after));

    // TempCloneForm inherits original overrides and isolates clone-specific edits.
    let clone_inherited = match q(&mut g, "SetFunctionValue (TempCloneForm TestProjectile)") {
        XVal::Ref(r) => FormId(r),
        v => panic!("TempCloneForm: {v:?}"),
    };
    assert_eq!(g.projectile_launch_traits(clone_inherited).unwrap().original, proj);
    assert_eq!(g.projectile_launch_traits(clone_inherited).unwrap().flags, Some(after));
    let clone_edited = match q(&mut g, "ref c\nlet c := TempCloneForm TestProjectile\nSetProjectileFlag c 10 1\nSetFunctionValue c") {
        XVal::Ref(r) => FormId(r),
        v => panic!("TempCloneForm edit: {v:?}"),
    };
    assert_eq!(g.projectile_flags_override(clone_edited), Some(after | (1u16 << 10)));
    assert_eq!(g.projectile_flags_override(proj), Some(after), "clone edit does not mutate original");
    assert_eq!(g.projectile_launch_traits(clone_edited).unwrap().flags, Some(after | (1u16 << 10)));

    // Non-projectile forms are refused without polluting field_overrides.
    q(&mut g, "SetProjectileFlag TestGun 0 1");
    assert_eq!(n(&mut g, "GetProjectileFlag TestGun 0"), 0.0);
    assert_eq!(g.projectile_flags_override(FormId(WEAP)), None);
    assert_eq!(g.projectile_flags(FormId(WEAP)), None);
    assert_eq!(g.num_field(FormId(WEAP), "DATA", "flags"), 0.0);
}
