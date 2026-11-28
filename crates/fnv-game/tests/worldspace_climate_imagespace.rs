//! Live `SetWorldspaceClimate` (`WRLD CNAM`) and `SetWorldspaceImagespace` (`WRLD INAM`)
//! overrides across climate, weather, moon phase, night, imagespace, and `WNAM`/`PNAM`
//! child-worldspace inheritance consumers (`docs/spec-notes/jip-world-12.md`).

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use fnv_world::index::parent_use;
use serde_json::Value as Json;

const WRLD_MAIN: u32 = 0xA10;
const PCELL_MAIN: u32 = 0xA11;
const EXT_MAIN: u32 = 0xA12;

const WRLD_CHILD_INHERIT: u32 = 0xA20;
const PCELL_INHERIT: u32 = 0xA21;
const EXT_INHERIT: u32 = 0xA22;

const WRLD_CHILD_OWN: u32 = 0xA30;
const PCELL_OWN: u32 = 0xA31;
const EXT_OWN: u32 = 0xA32;

const ROOM: u32 = 0xA40;

const CLMT_DEFAULT: u32 = 0x15F;
const CLMT_A: u32 = 0xA50;
const CLMT_B: u32 = 0xA51;
const CLMT_C: u32 = 0xA52;

const WTHR_CLEAR: u32 = 0xA60;
const WTHR_STORM: u32 = 0xA61;
const WTHR_GATED: u32 = 0xA62;

const IMGS_A: u32 = 0xA70;
const IMGS_B: u32 = 0xA71;
const IMGS_C: u32 = 0xA72;
const IMGS_CELL: u32 = 0xA73;

const GLOB_YEAR: u32 = 0x35;
const GLOB_MONTH: u32 = 0x36;
const GLOB_DAY: u32 = 0x37;
const GLOB_HOUR: u32 = 0x38;
const GLOB_DAYS: u32 = 0x39;
const GLOB_GATE: u32 = 0xA80;
const STAT_DUMMY: u32 = 0xA90;

fn edid(sig: &[u8; 4], fid: u32, name: &str, rest: &[fnv_esm::raw::Subrecord]) -> Vec<u8> {
    record(sig, 0, fid, &[vec![sub(b"EDID", z(name))], rest.to_vec()].concat())
}

fn wlst(entries: &[(u32, i32, u32)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(entries.len() * 12);
    for &(weather, chance, global) in entries {
        out.extend_from_slice(&weather.to_le_bytes());
        out.extend_from_slice(&chance.to_le_bytes());
        out.extend_from_slice(&global.to_le_bytes());
    }
    out
}

fn world_block(wrld: Vec<u8>, wrld_id: u32, pcell: Vec<u8>, ext: Vec<u8>) -> Vec<u8> {
    let children = group(wrld_id.to_le_bytes(), 1, &[pcell, group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &ext))].concat());
    [wrld, children].concat()
}

fn plugin_bytes() -> Vec<u8> {
    let npcs = record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]);
    let globs = [
        edid(b"GLOB", GLOB_YEAR, "GameYear", &[sub(b"FNAM", vec![b'f']), sub(b"FLTV", 2281.0f32.to_le_bytes().to_vec())]),
        edid(b"GLOB", GLOB_MONTH, "GameMonth", &[sub(b"FNAM", vec![b'f']), sub(b"FLTV", 10.0f32.to_le_bytes().to_vec())]),
        edid(b"GLOB", GLOB_DAY, "GameDay", &[sub(b"FNAM", vec![b'f']), sub(b"FLTV", 19.0f32.to_le_bytes().to_vec())]),
        edid(b"GLOB", GLOB_HOUR, "GameHour", &[sub(b"FNAM", vec![b'f']), sub(b"FLTV", 10.0f32.to_le_bytes().to_vec())]),
        edid(b"GLOB", GLOB_DAYS, "GameDaysPassed", &[sub(b"FNAM", vec![b'f']), sub(b"FLTV", 4.0f32.to_le_bytes().to_vec())]),
        edid(b"GLOB", GLOB_GATE, "WeatherGateGlob", &[sub(b"FNAM", vec![b'f']), sub(b"FLTV", 0.0f32.to_le_bytes().to_vec())]),
    ]
    .concat();
    let wthrs = [
        edid(b"WTHR", WTHR_CLEAR, "TestWeatherClear", &[]),
        edid(b"WTHR", WTHR_STORM, "TestWeatherStorm", &[]),
        edid(b"WTHR", WTHR_GATED, "TestWeatherGated", &[]),
    ]
    .concat();
    // CLMT_DEFAULT: 0x0000015F DefaultClimate, moon phase length 3 days (0x83).
    // CLMT_A: sunrise 06:00 (36), sunset 18:00 (108), moon phase length 4 days (0x84).
    // CLMT_B: sunrise 12:00 (72), sunset 16:00 (96), moon phase length 2 days (0x82);
    //         at GameHour=10, CLMT_A is daytime (0) while CLMT_B is nighttime (1).
    // CLMT_C: sunrise 06:00 (36), sunset 18:00 (108), moon phase length 8 days (0x88).
    let clmts = [
        edid(b"CLMT", CLMT_DEFAULT, "DefaultClimate", &[sub(b"TNAM", vec![30, 54, 96, 120, 0, 0x83])]),
        edid(b"CLMT", CLMT_A, "ClimateAlpha", &[sub(b"TNAM", vec![36, 42, 102, 108, 10, 0x84]), sub(b"WLST", wlst(&[(WTHR_CLEAR, 80, 0)]))]),
        edid(
            b"CLMT",
            CLMT_B,
            "ClimateBeta",
            &[sub(b"TNAM", vec![72, 78, 90, 96, 10, 0x82]), sub(b"WLST", wlst(&[(WTHR_STORM, 40, 0), (WTHR_GATED, 90, GLOB_GATE)]))],
        ),
        edid(b"CLMT", CLMT_C, "ClimateGamma", &[sub(b"TNAM", vec![36, 42, 102, 108, 10, 0x88]), sub(b"WLST", wlst(&[(WTHR_CLEAR, 50, 0)]))]),
    ]
    .concat();
    let imgs = [
        edid(b"IMGS", IMGS_A, "ImageSpaceAlpha", &[]),
        edid(b"IMGS", IMGS_B, "ImageSpaceBeta", &[]),
        edid(b"IMGS", IMGS_C, "ImageSpaceGamma", &[]),
        edid(b"IMGS", IMGS_CELL, "ImageSpaceCell", &[]),
    ]
    .concat();
    let stat = edid(b"STAT", STAT_DUMMY, "DummyStatic", &[]);

    let wrld_main = edid(b"WRLD", WRLD_MAIN, "MainWorld", &[sub(b"CNAM", u32s(&[CLMT_A])), sub(b"INAM", u32s(&[IMGS_A]))]);
    let pcell_main = edid(b"CELL", PCELL_MAIN, "", &[sub(b"DATA", vec![0])]);
    let ext_main = edid(b"CELL", EXT_MAIN, "MainExtCell", &[sub(b"DATA", vec![0]), sub(b"XCLC", u32s(&[0, 0]))]);

    let inherit_flags = (parent_use::CLIMATE | parent_use::IMAGE_SPACE).to_le_bytes().to_vec();
    let wrld_inherit = edid(
        b"WRLD",
        WRLD_CHILD_INHERIT,
        "ChildInheritWorld",
        &[sub(b"WNAM", u32s(&[WRLD_MAIN])), sub(b"PNAM", inherit_flags), sub(b"CNAM", u32s(&[CLMT_C])), sub(b"INAM", u32s(&[IMGS_C]))],
    );
    let pcell_inherit = edid(b"CELL", PCELL_INHERIT, "", &[sub(b"DATA", vec![0])]);
    let ext_inherit = edid(b"CELL", EXT_INHERIT, "InheritExtCell", &[sub(b"DATA", vec![0]), sub(b"XCLC", u32s(&[0, 0]))]);

    let wrld_own = edid(
        b"WRLD",
        WRLD_CHILD_OWN,
        "ChildOwnWorld",
        &[sub(b"WNAM", u32s(&[WRLD_MAIN])), sub(b"PNAM", 0u16.to_le_bytes().to_vec()), sub(b"CNAM", u32s(&[CLMT_C])), sub(b"INAM", u32s(&[IMGS_C]))],
    );
    let pcell_own = edid(b"CELL", PCELL_OWN, "", &[sub(b"DATA", vec![0])]);
    let ext_own = edid(b"CELL", EXT_OWN, "OwnExtCell", &[sub(b"DATA", vec![0]), sub(b"XCLC", u32s(&[0, 0]))]);

    let room = edid(b"CELL", ROOM, "InteriorRoom", &[sub(b"DATA", vec![1]), sub(b"XCIM", u32s(&[IMGS_CELL]))]);

    let worlds = [
        world_block(wrld_main, WRLD_MAIN, pcell_main, ext_main),
        world_block(wrld_inherit, WRLD_CHILD_INHERIT, pcell_inherit, ext_inherit),
        world_block(wrld_own, WRLD_CHILD_OWN, pcell_own, ext_own),
    ]
    .concat();

    [
        tes4(),
        top(b"GLOB", &globs),
        top(b"NPC_", &npcs),
        top(b"STAT", &stat),
        top(b"WTHR", &wthrs),
        top(b"CLMT", &clmts),
        top(b"IMGS", &imgs),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &room))),
        top(b"WRLD", &worlds),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "WorldClimateImg.esm".into()).unwrap()];
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
    enter(&mut g, EXT_MAIN);
    g
}

fn enter(g: &mut Game<'_>, cell: u32) {
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(cell));
    g.player_entered(Some(FormId(cell)));
}

fn q(g: &mut Game<'_>, body: &str) -> XVal {
    g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn num(g: &mut Game<'_>, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(x) => x,
        XVal::Ref(r) => f64::from(r),
        v => panic!("{expr}: {v:?}"),
    }
}

/// `SetClimateTraitNumeric` validates `CLMT`, caps unsigned sun times at `1430` (10-min
/// slots `0..=143`), clamps moons to `0..=3`, and clamps phase length to `0..=63`
/// [observed: `w6d2_16_climate_traits_and_weather_add_remove`].
#[test]
fn set_climate_trait_numeric_bounds_and_clmt_validation() {
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "SetClimateTraitNumeric ClimateAlpha 0 25");
    assert_eq!(num(&mut g, "GetClimateTraitNumeric ClimateAlpha 0"), 20.0);
    q(&mut g, "SetClimateTraitNumeric ClimateAlpha 0 1500");
    assert_eq!(num(&mut g, "GetClimateTraitNumeric ClimateAlpha 0"), 1430.0);
    q(&mut g, "SetClimateTraitNumeric ClimateAlpha 0 -10");
    assert_eq!(num(&mut g, "GetClimateTraitNumeric ClimateAlpha 0"), 1430.0);
    q(&mut g, "SetClimateTraitNumeric ClimateAlpha 5 4");
    assert_eq!(num(&mut g, "GetClimateTraitNumeric ClimateAlpha 5"), 3.0);
    q(&mut g, "SetClimateTraitNumeric ClimateAlpha 6 64");
    assert_eq!(num(&mut g, "GetClimateTraitNumeric ClimateAlpha 6"), 63.0);
    // Non-CLMT form is ignored.
    q(&mut g, "SetClimateTraitNumeric TestStatic 0 300");
    assert_eq!(num(&mut g, "GetClimateTraitNumeric TestStatic 0"), 0.0);
}

#[test]
fn set_worldspace_climate_reaches_current_climate_weathers_night_moon_and_respects_precedence() {
    let f = fixture();
    let mut g = game(&f);

    // Authored baseline on MainWorld (CLMT_A): daytime at GameHour=10, moon phase = (4/4)%8 = 1.
    assert_eq!(g.worldspace_climate_override(FormId(WRLD_MAIN)), None);
    assert_eq!(g.worldspace_climate(FormId(WRLD_MAIN)), Some(FormId(CLMT_A)));
    assert_eq!(g.script_climate(), None);
    assert_eq!(g.current_climate(), Some(FormId(CLMT_A)));
    assert_eq!(num(&mut g, "GetWorldspaceClimate MainWorld"), f64::from(CLMT_A));
    assert_eq!(num(&mut g, "GetCurrentClimate"), f64::from(CLMT_A));
    assert_eq!(g.current_climate_default_weather(), Some(FormId(WTHR_CLEAR)));
    assert_eq!(num(&mut g, "IsNight"), 0.0);
    assert_eq!(num(&mut g, "GetMoonPhase"), 1.0);

    // SetWorldspaceClimate switches active climate, weather table, IsNight, and GetMoonPhase.
    q(&mut g, "SetWorldspaceClimate MainWorld ClimateBeta");
    assert_eq!(g.worldspace_climate_override(FormId(WRLD_MAIN)), Some(FormId(CLMT_B)));
    assert_eq!(g.worldspace_climate(FormId(WRLD_MAIN)), Some(FormId(CLMT_B)));
    assert_eq!(g.script_climate(), Some(FormId(CLMT_B)));
    assert_eq!(g.current_climate(), Some(FormId(CLMT_B)));
    assert_eq!(num(&mut g, "GetWorldspaceClimate MainWorld"), f64::from(CLMT_B));
    assert_eq!(num(&mut g, "GetCurrentClimate"), f64::from(CLMT_B));
    assert_eq!(g.current_climate_weathers().len(), 2);
    // WeatherGateGlob == 0, so WTHR_STORM (chance 40) wins over gated WTHR_GATED (chance 90).
    assert_eq!(g.current_climate_default_weather(), Some(FormId(WTHR_STORM)));
    g.world.set_global(FormId(GLOB_GATE), 1.0).unwrap();
    assert_eq!(g.current_climate_default_weather(), Some(FormId(WTHR_GATED)));
    // At GameHour=10, ClimateBeta sunrise is 12:00 so IsNight reads 1; moon phase = (4/2)%8 = 2.
    assert_eq!(num(&mut g, "IsNight"), 1.0);
    assert_eq!(num(&mut g, "GetMoonPhase"), 2.0);

    // Precedence: SetCurrentClimate overrides SetWorldspaceClimate in the same worldspace.
    assert_eq!(num(&mut g, "SetCurrentClimate ClimateGamma"), 0.0);
    assert_eq!(g.script_climate(), Some(FormId(CLMT_C)));
    assert_eq!(num(&mut g, "GetCurrentClimate"), f64::from(CLMT_C));
    assert_eq!(num(&mut g, "GetWorldspaceClimate MainWorld"), f64::from(CLMT_B));

    // Precedence: ForceClimate overrides both; clearing ForceClimate falls back to SetWorldspaceClimate.
    q(&mut g, "ForceClimate ClimateAlpha");
    assert_eq!(g.script_climate(), Some(FormId(CLMT_A)));
    assert_eq!(num(&mut g, "GetCurrentClimate"), f64::from(CLMT_A));
    q(&mut g, "ForceClimate");
    assert_eq!(g.script_climate(), Some(FormId(CLMT_B)));
    assert_eq!(num(&mut g, "GetCurrentClimate"), f64::from(CLMT_B));

    // Indoors: no current exterior climate and empty weather list, while GetMoonPhase
    // retains the active moon phase from MainWorld's ClimateBeta (4.0 / 2 % 8 = 2)
    // [observed: w4c_b3_03_jg6_actor_world_misc, GetMoonPhase returns the active moon
    // phase indoors rather than 0].
    enter(&mut g, ROOM);
    assert_eq!(g.script_climate(), None);
    assert_eq!(g.current_climate(), None);
    assert_eq!(num(&mut g, "GetCurrentClimate"), 0.0);
    assert!(g.current_climate_weathers().is_empty());
    assert_eq!(g.current_climate_default_weather(), None);
    assert_eq!(num(&mut g, "GetMoonPhase"), 2.0);
}

#[test]
fn set_worldspace_imagespace_reaches_exterior_and_cell_precedence() {
    let f = fixture();
    let mut g = game(&f);

    // Authored baseline on MainWorld (IMGS_A).
    assert_eq!(g.worldspace_image_space_override(FormId(WRLD_MAIN)), None);
    assert_eq!(g.worldspace_image_space(FormId(WRLD_MAIN)), Some(FormId(IMGS_A)));
    assert_eq!(g.current_worldspace_image_space_override(), None);
    assert_eq!(g.current_worldspace_image_space(), Some(FormId(IMGS_A)));
    assert_eq!(g.current_image_space(), Some(FormId(IMGS_A)));
    assert_eq!(num(&mut g, "GetWorldspaceImagespace MainWorld"), f64::from(IMGS_A));

    // SetWorldspaceImagespace updates getter and exterior image-space consumers.
    q(&mut g, "SetWorldspaceImagespace MainWorld ImageSpaceBeta");
    assert_eq!(g.worldspace_image_space_override(FormId(WRLD_MAIN)), Some(FormId(IMGS_B)));
    assert_eq!(g.worldspace_image_space(FormId(WRLD_MAIN)), Some(FormId(IMGS_B)));
    assert_eq!(g.current_worldspace_image_space_override(), Some(FormId(IMGS_B)));
    assert_eq!(g.current_worldspace_image_space(), Some(FormId(IMGS_B)));
    assert_eq!(g.current_image_space(), Some(FormId(IMGS_B)));
    assert_eq!(num(&mut g, "GetWorldspaceImagespace MainWorld"), f64::from(IMGS_B));

    // Explicit SetCellImageSpace on an exterior cell takes precedence over worldspace image space.
    q(&mut g, "SetCellImageSpace MainExtCell ImageSpaceCell");
    assert_eq!(num(&mut g, "GetCellImageSpace MainExtCell"), f64::from(IMGS_CELL));
    assert_eq!(g.current_image_space(), Some(FormId(IMGS_CELL)));
    assert_eq!(g.current_worldspace_image_space(), Some(FormId(IMGS_B)));

    // Indoors: worldspace image space queries return None, cell XCIM / SetCellImageSpace drives current_image_space.
    enter(&mut g, ROOM);
    assert_eq!(g.current_worldspace_image_space_override(), None);
    assert_eq!(g.current_worldspace_image_space(), None);
    assert_eq!(num(&mut g, "GetCellImageSpace InteriorRoom"), f64::from(IMGS_CELL));
    assert_eq!(g.current_image_space(), Some(FormId(IMGS_CELL)));
    q(&mut g, "SetCellImageSpace InteriorRoom ImageSpaceGamma");
    assert_eq!(num(&mut g, "GetCellImageSpace InteriorRoom"), f64::from(IMGS_C));
    assert_eq!(g.current_image_space(), Some(FormId(IMGS_C)));
}

#[test]
fn child_worldspace_pnam_inheritance_and_invalid_input_controls() {
    let f = fixture();
    let mut g = game(&f);

    // ChildInheritWorld has PNAM CLIMATE | IMAGE_SPACE -> inherits MainWorld's authored and overridden values.
    assert_eq!(g.worldspace_climate(FormId(WRLD_CHILD_INHERIT)), Some(FormId(CLMT_A)));
    assert_eq!(g.worldspace_image_space(FormId(WRLD_CHILD_INHERIT)), Some(FormId(IMGS_A)));
    // ChildOwnWorld has PNAM = 0 -> keeps its own authored CLMT_C and IMGS_C.
    assert_eq!(g.worldspace_climate(FormId(WRLD_CHILD_OWN)), Some(FormId(CLMT_C)));
    assert_eq!(g.worldspace_image_space(FormId(WRLD_CHILD_OWN)), Some(FormId(IMGS_C)));

    // Parent overrides propagate to ChildInheritWorld but not ChildOwnWorld.
    q(&mut g, "SetWorldspaceClimate MainWorld ClimateBeta");
    q(&mut g, "SetWorldspaceImagespace MainWorld ImageSpaceBeta");
    assert_eq!(g.worldspace_climate_override(FormId(WRLD_CHILD_INHERIT)), Some(FormId(CLMT_B)));
    assert_eq!(g.worldspace_climate(FormId(WRLD_CHILD_INHERIT)), Some(FormId(CLMT_B)));
    assert_eq!(g.worldspace_image_space_override(FormId(WRLD_CHILD_INHERIT)), Some(FormId(IMGS_B)));
    assert_eq!(g.worldspace_image_space(FormId(WRLD_CHILD_INHERIT)), Some(FormId(IMGS_B)));

    enter(&mut g, EXT_INHERIT);
    assert_eq!(g.script_climate(), Some(FormId(CLMT_B)));
    assert_eq!(g.current_worldspace_image_space(), Some(FormId(IMGS_B)));

    enter(&mut g, EXT_OWN);
    assert_eq!(g.script_climate(), None);
    assert_eq!(g.current_climate(), Some(FormId(CLMT_C)));
    assert_eq!(g.current_worldspace_image_space(), Some(FormId(IMGS_C)));

    // Child's own override takes precedence over parent override without mutating parent.
    q(&mut g, "SetWorldspaceClimate ChildInheritWorld ClimateGamma");
    q(&mut g, "SetWorldspaceImagespace ChildInheritWorld ImageSpaceGamma");
    assert_eq!(g.worldspace_climate(FormId(WRLD_CHILD_INHERIT)), Some(FormId(CLMT_C)));
    assert_eq!(g.worldspace_image_space(FormId(WRLD_CHILD_INHERIT)), Some(FormId(IMGS_C)));
    assert_eq!(g.worldspace_climate(FormId(WRLD_MAIN)), Some(FormId(CLMT_B)));
    assert_eq!(g.worldspace_image_space(FormId(WRLD_MAIN)), Some(FormId(IMGS_B)));

    // Invalid and null inputs do not mutate existing overrides.
    q(&mut g, "SetWorldspaceClimate MainWorld 0");
    q(&mut g, "SetWorldspaceClimate MainWorld");
    q(&mut g, "SetWorldspaceClimate MainWorld ImageSpaceAlpha");
    q(&mut g, "SetWorldspaceClimate DummyStatic ClimateAlpha");
    assert_eq!(g.worldspace_climate(FormId(WRLD_MAIN)), Some(FormId(CLMT_B)));
    assert_eq!(g.worldspace_climate(FormId(STAT_DUMMY)), None);

    q(&mut g, "SetWorldspaceImagespace MainWorld 0");
    q(&mut g, "SetWorldspaceImagespace MainWorld");
    q(&mut g, "SetWorldspaceImagespace MainWorld ClimateAlpha");
    q(&mut g, "SetWorldspaceImagespace DummyStatic ImageSpaceAlpha");
    assert_eq!(g.worldspace_image_space(FormId(WRLD_MAIN)), Some(FormId(IMGS_B)));
    assert_eq!(g.worldspace_image_space(FormId(STAT_DUMMY)), None);
}

#[test]
fn get_moon_phase_indoors_uses_active_or_default_climate() {
    let f = fixture();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() });
    // Start directly indoors without ever visiting an exterior worldspace:
    // `w4c_b3_03_jg6_actor_world_misc` observed `moon=1` indoors, while the
    // `GameDaysPassed = 5.0` / `DefaultClimate` (0x0000015F, phase length 3 ->
    // floor(5.0 / 3.0) % 8 = 1) derivation (see docs/spec-notes/ext-jg6.md).
    g.world.set_global(FormId(GLOB_DAYS), 5.0).unwrap();
    enter(&mut g, ROOM);
    assert_eq!(g.current_climate(), None);
    assert_eq!(num(&mut g, "GetMoonPhase"), 1.0);

    // Advancing GameDaysPassed indoors advances the moon phase.
    g.world.set_global(FormId(GLOB_DAYS), 6.0).unwrap();
    assert_eq!(num(&mut g, "GetMoonPhase"), 2.0);
    g.world.set_global(FormId(GLOB_DAYS), 12.0).unwrap();
    assert_eq!(num(&mut g, "GetMoonPhase"), 4.0);

    // ForceClimate indoors overrides the phase length (ClimateBeta = 2 -> floor(12/2)%8 = 6);
    // clearing ForceClimate falls back to DefaultClimate (phase length 3 -> 4).
    q(&mut g, "ForceClimate ClimateBeta");
    assert_eq!(num(&mut g, "GetMoonPhase"), 6.0);
    q(&mut g, "ForceClimate");
    assert_eq!(num(&mut g, "GetMoonPhase"), 4.0);

    // Visiting MainWorld (ClimateAlpha, phase length 4 -> floor(12/4)%8 = 3) and returning
    // indoors retains MainWorld's climate for GetMoonPhase.
    enter(&mut g, EXT_MAIN);
    assert_eq!(num(&mut g, "GetMoonPhase"), 3.0);
    enter(&mut g, ROOM);
    assert_eq!(g.current_climate(), None);
    assert_eq!(num(&mut g, "GetMoonPhase"), 3.0);
}
