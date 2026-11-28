//! Live `SetImageSpaceTrait`, `SetCellImageSpace`, and `SetWeatherTraitNumeric` (15..=20)
//! engine integration tests through the real `world::cell_image_space`,
//! `imagespace::update_grade`, `sky::sync_weather_fog`, and `sky::apply_cell_fog` systems.
use super::*;
use crate::authored_test_records::{group, record, sub, tes4, top, u32s, z};
use crate::imagespace::{update_grade, ImageSpaceGrade, ScreenModifiers};
use crate::world::{cell_image_space, cell_lighting, Streamer, WorldData};
use fnv_game::game::RunCtx;
use fnv_script::vm::XVal;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use fnv_world::imagespace::{
    HDR_BLOOM_ALPHA_EXTERIOR, HDR_BLOOM_ALPHA_INTERIOR, HDR_BLOOM_BLUR_RADIUS,
    HDR_EMISSIVE_MULT, HDR_EYE_ADAPT_SPEED, HDR_SKIN_DIMMER, HDR_SUNLIGHT_DIMMER,
};
use fnv_world::index::parent_use;
use nvf_play::script::{with_world, ScriptState};
use nvf_play::sim::SimState;
use nvf_play::{PlayData, PlayState, PlayWorld};

const CELL_INT: u32 = 0xE00;
const IMGS_A: u32 = 0xE01;
const IMGS_B: u32 = 0xE02;
const WRLD_EXT: u32 = 0xE03;
const WTHR_A: u32 = 0xE04;
const WTHR_B: u32 = 0xE05;
const WRLD_EXT_B: u32 = 0xE06;
const IMGS_C: u32 = 0xE07;
const IMAD_A: u32 = 0xE08;
const WRLD_CHILD: u32 = 0xE09;
const PCELL_EXT: u32 = 0xE0A;
const EXT_CELL_A: u32 = 0xE0B;
const EXT_CELL_B: u32 = 0xE0C;
const LGTM_A: u32 = 0xE0D;
const LGTM_B: u32 = 0xE0E;
const CELL_INT_LIGHT: u32 = 0xE0F;
const CELL_INT_NO_XCLL: u32 = 0xE10;

fn imgs_dnam(saturation: f32, brightness: f32, tint: [f32; 3], tint_amount: f32) -> Vec<u8> {
    let mut vals = [0.0f32; 38];
    vals[0] = 0.5; // eye_adapt_speed
    vals[2] = 1.0; // blur_passes
    vals[3] = 1.0; // emissive_mult
    vals[4] = 1.4; // target_lum
    vals[6] = 1.0; // bright_scale
    vals[11] = 1.1; // sunlight_dimmer
    vals[14] = 1.0; // skin_dimmer
    vals[15] = 0.25; // bloom_blur_radius
    vals[16] = 0.1; // bloom_alpha_interior
    vals[17] = 0.2; // bloom_alpha_exterior
    vals[25] = saturation;
    vals[26] = 0.5; // contrast_avg_lum
    vals[27] = 1.0; // contrast
    vals[28] = brightness;
    vals[29] = tint[0];
    vals[30] = tint[1];
    vals[31] = tint[2];
    vals[32] = tint_amount;
    vals.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn wthr_fnam(fog: [f32; 6]) -> Vec<u8> {
    fog.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn wthr_nam0(rgb: [u8; 3]) -> Vec<u8> {
    // 10 colour types x 6 times x 4 bytes (RGBA) = 240 bytes.
    let mut out = Vec::with_capacity(10 * 6 * 4);
    for _ in 0..(10 * 6) {
        out.extend([rgb[0], rgb[1], rgb[2], 0]);
    }
    out
}

fn wthr_pnam(rgb: [u8; 3]) -> Vec<u8> {
    // 4 cloud layers x 6 times x 4 bytes (RGBA) = 96 bytes.
    let mut out = Vec::with_capacity(4 * 6 * 4);
    for _ in 0..(4 * 6) {
        out.extend([rgb[0], rgb[1], rgb[2], 0]);
    }
    out
}

fn xcll_bytes(ambient: [u8; 3], fog: [u8; 3], fog_near: f32, fog_far: f32, clip: f32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(40);
    for c in [ambient, [20, 30, 40], fog] {
        bytes.extend([c[0], c[1], c[2], 0]);
    }
    bytes.extend(fog_near.to_le_bytes());
    bytes.extend(fog_far.to_le_bytes());
    bytes.extend(0i32.to_le_bytes());
    bytes.extend(270i32.to_le_bytes());
    bytes.extend(0.5f32.to_le_bytes());
    bytes.extend(clip.to_le_bytes());
    bytes.extend(1.0f32.to_le_bytes());
    bytes
}

fn fixture(world_id: u32, interior: Option<u32>) -> (WorldData, PlayData, &'static FormStore, &'static RefTable) {
    let imgs = [
        record(
            b"IMGS",
            0,
            IMGS_A,
            &[sub(b"EDID", z("OwnedImgA")), sub(b"DNAM", imgs_dnam(1.0, 1.0, [0.2, 0.4, 0.6], 0.1))],
        ),
        record(
            b"IMGS",
            0,
            IMGS_B,
            &[sub(b"EDID", z("OwnedImgB")), sub(b"DNAM", imgs_dnam(0.6, 1.25, [0.8, 0.2, 0.1], 0.3))],
        ),
        record(
            b"IMGS",
            0,
            IMGS_C,
            &[sub(b"EDID", z("OwnedImgC")), sub(b"DNAM", imgs_dnam(0.3, 1.4, [0.1, 0.7, 0.9], 0.5))],
        ),
    ]
    .concat();
    let imad_a = {
        let mut header = vec![0u8; 244];
        // Key counts for 0x0B (HDR_SUNLIGHT_DIMMER mult), 0x13 (contrast mult), TNAM tint.
        header[8 + 0x0B * 8..8 + 0x0B * 8 + 4].copy_from_slice(&1u32.to_le_bytes());
        header[8 + 0x13 * 8..8 + 0x13 * 8 + 4].copy_from_slice(&1u32.to_le_bytes());
        header[176..180].copy_from_slice(&1u32.to_le_bytes());
        let key = |v: f32| [0.0f32, v].into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>();
        let tnam: Vec<u8> = [0.0f32, 0.9, 0.3, 0.1, 0.6].into_iter().flat_map(f32::to_le_bytes).collect();
        record(
            b"IMAD",
            0,
            IMAD_A,
            &[
                sub(b"EDID", z("OwnedAdptA")),
                sub(b"DNAM", header),
                sub(b"\x0bIAD", key(2.0)),
                sub(b"\x13IAD", key(1.5)),
                sub(b"TNAM", tnam),
            ],
        )
    };
    let wthrs = [
        record(
            b"WTHR",
            0,
            WTHR_A,
            &[
                sub(b"EDID", z("OwnedWeatherA")),
                sub(b"NAM0", wthr_nam0([30, 40, 50])),
                sub(b"PNAM", wthr_pnam([60, 70, 80])),
                sub(b"FNAM", wthr_fnam([1000.0, 100_000.0, 500.0, 50_000.0, 1.0, 1.0])),
                sub(b"DATA", vec![0, 0, 0, 255, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            ],
        ),
        record(
            b"WTHR",
            0,
            WTHR_B,
            &[
                sub(b"EDID", z("OwnedWeatherB")),
                sub(b"NAM0", wthr_nam0([80, 90, 100])),
                sub(b"PNAM", wthr_pnam([10, 20, 30])),
                sub(b"FNAM", wthr_fnam([2000.0, 60_000.0, 800.0, 30_000.0, 1.0, 1.0])),
                sub(b"DATA", vec![0, 0, 0, 255, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            ],
        ),
    ]
    .concat();
    let lgtms = [
        record(
            b"LGTM",
            0,
            LGTM_A,
            &[sub(b"EDID", z("OwnedCartoLgtmA")), sub(b"DATA", xcll_bytes([10, 15, 20], [5, 10, 15], 100.0, 4000.0, 8000.0))],
        ),
        record(
            b"LGTM",
            0,
            LGTM_B,
            &[sub(b"EDID", z("OwnedCartoLgtmB")), sub(b"DATA", xcll_bytes([90, 100, 110], [40, 50, 60], 300.0, 9000.0, 22000.0))],
        ),
    ]
    .concat();
    let cells = [
        record(
            b"CELL",
            0,
            CELL_INT,
            &[
                sub(b"EDID", z("OwnedIntCell")),
                sub(b"DATA", vec![1]),
                sub(b"XCIM", u32s(&[IMGS_A])),
            ],
        ),
        record(
            b"CELL",
            0,
            CELL_INT_LIGHT,
            &[
                sub(b"EDID", z("OwnedIntCellLight")),
                sub(b"DATA", vec![1]),
                sub(b"XCLL", xcll_bytes([1, 2, 3], [4, 5, 6], 50.0, 2000.0, 5000.0)),
                sub(b"LTMP", u32s(&[LGTM_A])),
                sub(b"LNAM", u32s(&[0x1FF])),
                sub(b"XCIM", u32s(&[IMGS_A])),
            ],
        ),
        record(
            b"CELL",
            0,
            CELL_INT_NO_XCLL,
            &[
                sub(b"EDID", z("OwnedIntCellNoXcll")),
                sub(b"DATA", vec![1]),
                sub(b"XCIM", u32s(&[IMGS_A])),
            ],
        ),
    ]
    .concat();
    let pcell_ext = record(b"CELL", 0, PCELL_EXT, &[sub(b"DATA", vec![0])]);
    let ext_cells = [
        record(b"CELL", 0, EXT_CELL_A, &[sub(b"EDID", z("OwnedExtCellA")), sub(b"DATA", vec![0]), sub(b"XCLC", u32s(&[0, 0]))]),
        record(b"CELL", 0, EXT_CELL_B, &[sub(b"EDID", z("OwnedExtCellB")), sub(b"DATA", vec![0]), sub(b"XCLC", u32s(&[1, 0]))]),
    ]
    .concat();
    let wrld_ext_children = group(
        WRLD_EXT.to_le_bytes(),
        1,
        &[pcell_ext, group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &ext_cells))].concat(),
    );
    let wrlds = [
        record(
            b"WRLD",
            0,
            WRLD_EXT,
            &[sub(b"EDID", z("OwnedExtWorld")), sub(b"INAM", u32s(&[IMGS_A]))],
        ),
        wrld_ext_children,
        record(
            b"WRLD",
            0,
            WRLD_EXT_B,
            &[sub(b"EDID", z("OwnedExtWorldB")), sub(b"INAM", u32s(&[IMGS_B]))],
        ),
        record(
            b"WRLD",
            0,
            WRLD_CHILD,
            &[
                sub(b"EDID", z("OwnedChildWorld")),
                sub(b"WNAM", u32s(&[WRLD_EXT])),
                sub(b"PNAM", parent_use::IMAGE_SPACE.to_le_bytes().to_vec()),
                sub(b"INAM", u32s(&[IMGS_C])),
            ],
        ),
    ]
    .concat();
    let bytes = [
        tes4(),
        top(b"IMGS", &imgs),
        top(b"IMAD", &imad_a),
        top(b"LGTM", &lgtms),
        top(b"WTHR", &wthrs),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cells))),
        top(b"WRLD", &wrlds),
    ]
    .concat();

    let plugins = Box::leak(
        vec![fnv_esm::Plugin::parse(&bytes, "AuthoredCartographerLive.esm".into()).unwrap()]
            .into_boxed_slice(),
    );
    let lo = Box::leak(Box::new(fnv_esm::LoadOrder::new(plugins).unwrap()));
    let schema = Box::leak(Box::new(
        fnv_esm::Schema::load_dir(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv"),
        )
        .unwrap(),
    ));
    let index = Box::leak(Box::new(fnv_world::WorldIndex::build(lo, schema)));
    let forms = Box::leak(Box::new(FormStore::build(lo, schema)));
    let refs = Box::leak(Box::new(RefTable::build(lo, schema)));
    let world = WorldData {
        lo,
        schema,
        index,
        world: world_id,
        persistent_by_grid: Default::default(),
        imposters: Box::leak(Box::new(fnv_world::imposter::ImposterRegions::build(lo, schema))),
        weather_regions: &[],
        dropped_plugins: Vec::new(),
    };
    let play = PlayData {
        lo,
        schema,
        index,
        world: world_id,
        interior,
        eye: [0.0; 3],
        heading_deg: 0.0,
        pitch_deg: 0.0,
        auto_walk: 0.0,
        start_at_cell_spot: false,
        packages: false,
    };
    (world, play, forms, refs)
}

fn runtime(
    play: PlayData,
    forms: &'static FormStore,
    refs: &'static RefTable,
) -> (ScriptState, SimState) {
    let mut boot = App::new();
    boot.add_plugins(MinimalPlugins)
        .insert_resource(PlayWorld(play))
        .insert_resource(SimState {
            forms,
            refs,
            world: World::new(forms, refs, SimConfig::default()),
        })
        .insert_resource(nvf_play::Location {
            world: play.world,
            interior: play.interior,
        })
        .insert_resource(State::new(PlayState::Playing))
        .init_resource::<NextState<PlayState>>()
        .insert_resource(nvf_play::player::Player {
            heading: 0.0,
            pitch: 0.0,
            eye_height: 64.0,
            feet: [0.0; 3],
            body_eye: None,
            body_view: None,
            placed: None,
        })
        .init_resource::<nvf_play::phys::PhysState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<bevy::input::mouse::AccumulatedMouseScroll>()
        .init_resource::<nvf_play::bindings::Bindings>()
        .init_resource::<nvf_play::pause::PlayClock>()
        .init_resource::<nvf_play::pause::Paused>()
        .init_resource::<nvf_play::pause::Page>()
        .init_resource::<nvf_play::loading::Loading>()
        .init_resource::<nvf_play::tiles::mods::ModsPage>()
        .init_resource::<nvf_play::tutorial_menu::TutorialView>()
        .init_resource::<nvf_play::control::ControlLocks>()
        .init_resource::<nvf_play::control::LockStats>()
        .init_resource::<nvf_play::menu::ActiveMenu>()
        .init_resource::<nvf_play::wait::WaitMenu>()
        .init_resource::<nvf_play::terminal::TerminalScreen>()
        .init_resource::<nvf_play::lockpick::LockpickScreen>()
        .init_resource::<nvf_play::console::Console>()
        .init_resource::<nvf_play::poker::PokerScreen>()
        .init_resource::<nvf_play::credits::Credits>()
        .init_resource::<Assets<Image>>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_message::<nvf_play::tutorial_menu::TutorialInput>()
        .add_message::<nvf_play::RefChanged>()
        .add_message::<nvf_play::fade::RefFade>()
        .add_message::<nvf_play::RefRepose>()
        .add_message::<nvf_play::loading::TravelRequest>()
        .add_message::<nvf_play::OpenContainer>()
        .add_message::<nvf_play::ItemUsed>()
        .add_message::<nvf_play::furniture::UseFurniture>()
        .add_message::<nvf_play::terminal::OpenTerminal>()
        .add_message::<nvf_play::save::LoadRefused>()
        .add_message::<nvf_play::pause::MainMenuRequest>()
        .configure_sets(Update, nvf_play::PlaySet::Ui.run_if(|| false))
        .configure_sets(Update, nvf_play::PlaySet::Sync.run_if(|| false))
        .add_plugins(nvf_play::script::ScriptPlugin);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        boot.update();
        let script = boot.world().resource::<ScriptState>();
        assert!(
            script.off.is_none(),
            "public script bootstrap refused: {:?}",
            script.off
        );
        if script.rt.is_some() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "public script bootstrap timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    (
        boot.world_mut().remove_resource::<ScriptState>().unwrap(),
        boot.world_mut().remove_resource::<SimState>().unwrap(),
    )
}

pub(crate) fn test_streamer(interior: Option<u32>) -> Streamer {
    Streamer {
        radius: 2,
        loaded: default(),
        fades: default(),
        fade_secs: 0.0,
        settled: true,
        arrival: None,
        per_frame: 1,
        spawning: default(),
        dying: default(),
        ahead: default(),
        hide: default(),
        interior,
        light_scale: 1.0,
        managed: default(),
        manage_actors: false,
        stats: default(),
        buffer: default(),
        centre: None,
        waiting: default(),
        model_overrides: default(),
        coop_areas: Vec::new(),
        travels: 0,
    }
}

fn source(a: &mut App, body: &str) -> XVal {
    a.world_mut()
        .resource_scope(|world, mut script: Mut<ScriptState>| {
            let mut sim = world.resource_mut::<SimState>();
            with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
                rt.game
                    .run_query(
                        &format!("Begin Function {{}}\n{body}\nEnd"),
                        RunCtx::default(),
                    )
                    .unwrap_or_else(|e| panic!("source {body}: {e}"))
            })
        })
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn fog_range(f: &FogFalloff) -> (f32, f32) {
    match f {
        FogFalloff::Linear { start, end } => (*start, *end),
        FogFalloff::Atmospheric { extinction, .. } => (extinction.x, extinction.y),
        other => panic!("unexpected fog falloff variant: {other:?}"),
    }
}

#[test]
fn live_imagespace_traits_refresh_while_set_cell_image_space_waits_for_next_load() {
    let (world, play, forms, refs) = fixture(0, Some(CELL_INT));
    let (script, sim) = runtime(play, forms, refs);
    let mut sky = authored_session_sky();
    sky.set_interior(Some(fnv_world::lighting::CellLighting::default()));

    let mut a = App::new();
    a.insert_resource(Time::<()>::default())
        .insert_resource(world)
        .insert_resource(test_streamer(Some(CELL_INT)))
        .insert_resource(sky)
        .insert_resource(script)
        .insert_resource(sim)
        .init_resource::<ScreenModifiers>()
        .add_message::<nvf_play::Travel>()
        .add_systems(Update, (cell_image_space, update_grade).chain());
    let cam = a.world_mut().spawn(ImageSpaceGrade::default()).id();

    // Cold frame reads authored CELL XCIM (OwnedImgA).
    a.update();
    let g = *a.world().get::<ImageSpaceGrade>(cam).unwrap();
    assert!(approx(g.saturation, 1.0), "authored saturation: {}", g.saturation);
    assert!(approx(g.brightness, 1.0), "authored brightness: {}", g.brightness);
    assert!(approx(g.tint.w, 0.1), "authored tint_amount: {}", g.tint.w);
    let hdr = a.world().resource::<ScreenModifiers>().graded_hdr().unwrap();
    assert!(approx(hdr.v[HDR_SUNLIGHT_DIMMER], 1.1));
    assert!(approx(hdr.v[HDR_BLOOM_BLUR_RADIUS], 0.25));

    // Live SetImageSpaceTrait across HDR, Bloom, GetHit/NightEye, and Cinematic (tint RGB 0..=255).
    source(
        &mut a,
        "SetImageSpaceTrait OwnedImgA 0 0.8\n\
         SetImageSpaceTrait OwnedImgA 2 3.0\n\
         SetImageSpaceTrait OwnedImgA 3 1.75\n\
         SetImageSpaceTrait OwnedImgA 11 2.4\n\
         SetImageSpaceTrait OwnedImgA 14 0.85\n\
         SetImageSpaceTrait OwnedImgA 15 1.5\n\
         SetImageSpaceTrait OwnedImgA 16 0.4\n\
         SetImageSpaceTrait OwnedImgA 17 0.6\n\
         SetImageSpaceTrait OwnedImgA 18 0.9\n\
         SetImageSpaceTrait OwnedImgA 21 0.5\n\
         SetImageSpaceTrait OwnedImgA 25 0.72\n\
         SetImageSpaceTrait OwnedImgA 26 0.38\n\
         SetImageSpaceTrait OwnedImgA 27 1.35\n\
         SetImageSpaceTrait OwnedImgA 28 0.92\n\
         SetImageSpaceTrait OwnedImgA 29 51\n\
         SetImageSpaceTrait OwnedImgA 30 153\n\
         SetImageSpaceTrait OwnedImgA 31 204\n\
         SetImageSpaceTrait OwnedImgA 32 0.45",
    );
    assert_eq!(source(&mut a, "SetFunctionValue (GetImageSpaceTrait OwnedImgA 18)"), XVal::Num(0.9));
    assert_eq!(source(&mut a, "SetFunctionValue (GetImageSpaceTrait OwnedImgA 21)"), XVal::Num(0.5));

    // Trait edits on the already-loaded IMGS refresh live without leaving the cell [hypothesis].
    a.update();
    let g = *a.world().get::<ImageSpaceGrade>(cam).unwrap();
    assert!(approx(g.saturation, 0.72), "live saturation: {}", g.saturation);
    assert!(approx(g.contrast_avg_lum, 0.38), "live contrast_avg_lum: {}", g.contrast_avg_lum);
    assert!(approx(g.contrast, 1.35), "live contrast: {}", g.contrast);
    assert!(approx(g.brightness, 0.92), "live brightness: {}", g.brightness);
    assert!(approx(g.tint.x, 51.0 / 255.0), "live tint R: {}", g.tint.x);
    assert!(approx(g.tint.y, 153.0 / 255.0), "live tint G: {}", g.tint.y);
    assert!(approx(g.tint.z, 204.0 / 255.0), "live tint B: {}", g.tint.z);
    assert!(approx(g.tint.w, 0.45), "live tint_amount: {}", g.tint.w);
    let hdr = a.world().resource::<ScreenModifiers>().graded_hdr().unwrap();
    assert!(approx(hdr.v[HDR_EYE_ADAPT_SPEED], 0.8));
    assert!(approx(hdr.blur_passes, 3.0));
    assert!(approx(hdr.v[HDR_EMISSIVE_MULT], 1.75));
    assert!(approx(hdr.v[HDR_SUNLIGHT_DIMMER], 2.4));
    assert!(approx(hdr.v[HDR_SKIN_DIMMER], 0.85));
    assert!(approx(hdr.v[HDR_BLOOM_BLUR_RADIUS], 1.5));
    assert!(approx(hdr.v[HDR_BLOOM_ALPHA_INTERIOR], 0.4));
    assert!(approx(hdr.v[HDR_BLOOM_ALPHA_EXTERIOR], 0.6));

    // Unchanged-frame zero-work control: a sentinel value in sky.interior_grade stays untouched.
    a.world_mut().resource_mut::<SkyState>().interior_grade.as_mut().unwrap().saturation = 7.75;
    a.update();
    assert_eq!(
        a.world().resource::<SkyState>().interior_grade.unwrap().saturation,
        7.75,
        "cell_image_space must do zero work on unchanged frames"
    );

    // Documented next-load rule (GECK wiki SetCellImageSpace): calling SetCellImageSpace on
    // the cell the player is already in must NOT switch the active image space before reload.
    source(&mut a, "SetCellImageSpace OwnedIntCell OwnedImgB");
    a.update();
    assert_eq!(
        a.world().resource::<SkyState>().interior_grade.unwrap().saturation,
        7.75,
        "SetCellImageSpace on current cell must wait until next cell load"
    );
    let g = *a.world().get::<ImageSpaceGrade>(cam).unwrap();
    assert!(approx(g.brightness, 0.92), "must keep OwnedImgA brightness before reload, got {}", g.brightness);

    // Even when SetImageSpaceTrait triggers a live refresh while SetCellImageSpace is pending,
    // only the already-bound OwnedImgA traits refresh — OwnedImgB still waits for next load.
    source(
        &mut a,
        "SetImageSpaceTrait OwnedImgA 25 0.61\n\
         SetImageSpaceTrait OwnedImgB 25 0.35",
    );
    a.update();
    let g = *a.world().get::<ImageSpaceGrade>(cam).unwrap();
    assert!(approx(g.saturation, 0.61), "bound OwnedImgA trait refreshes live: {}", g.saturation);
    assert!(approx(g.brightness, 0.92), "pending SetCellImageSpace must not apply early: {}", g.brightness);

    // Next cell load (Travel / re-entry) binds OwnedImgB and applies its live trait override.
    a.world_mut().write_message(nvf_play::Travel {
        world: None,
        interior: Some(CELL_INT),
        feet: [0.0; 3],
        heading_deg: 0.0,
        pitch_deg: None,
    });
    a.update();
    let g = *a.world().get::<ImageSpaceGrade>(cam).unwrap();
    assert!(approx(g.saturation, 0.35), "reloaded cell binds OwnedImgB + trait override saturation: {}", g.saturation);
    assert!(approx(g.brightness, 1.25), "reloaded cell binds OwnedImgB brightness: {}", g.brightness);

    // Game-instance switch (new/loaded game) immediately restores authored IMGS_A.
    let (fresh_script, fresh_sim) = runtime(play, forms, refs);
    a.insert_resource(fresh_script);
    a.insert_resource(fresh_sim);
    a.update();
    let g = *a.world().get::<ImageSpaceGrade>(cam).unwrap();
    assert!(approx(g.saturation, 1.0), "fresh Game restores authored OwnedImgA saturation: {}", g.saturation);
    assert!(approx(g.brightness, 1.0), "fresh Game restores authored OwnedImgA brightness: {}", g.brightness);
    let hdr = a.world().resource::<ScreenModifiers>().graded_hdr().unwrap();
    assert!(approx(hdr.v[HDR_SUNLIGHT_DIMMER], 1.1), "fresh Game restores authored HDR");
}

#[test]
fn live_exterior_worldspace_imagespace_and_weather_fog_respect_console_override_and_isolation() {
    let (world, play, forms, refs) = fixture(WRLD_EXT, None);
    let (script, sim) = runtime(play, forms, refs);
    let wthr_a = fnv_world::sky::weather(world.lo, world.schema, WTHR_A).unwrap();
    let wthr_b = fnv_world::sky::weather(world.lo, world.schema, WTHR_B).unwrap();

    let mut sky = authored_session_sky();
    sky.climate.sunrise = (6.0, 7.0);
    sky.climate.sunset = (18.0, 19.0);
    sky.weather = wthr_a;
    sky.hour = 12.0;
    sky.changed = false;

    let mut a = App::new();
    a.insert_resource(world)
        .insert_resource(test_streamer(None))
        .insert_resource(sky)
        .insert_resource(script)
        .insert_resource(sim)
        .add_message::<nvf_play::Travel>()
        .add_systems(Update, (cell_image_space, sync_weather_fog).chain());

    // Initial exterior sync picks up WRLD INAM (IMGS_A) and authored WTHR_A fog.
    a.update();
    {
        let sky = a.world().resource::<SkyState>();
        assert_eq!(sky.world_image_space_id, Some(IMGS_A));
        assert!(approx(sky.image_space.unwrap().saturation, 1.0));
        assert_eq!(sky.fog(), Some((1000.0, 100_000.0, 1.0)));
        assert!(approx(sky.fog_power(), 1.0));
    }

    // Unchanged frame does zero churn (sky.changed remains false).
    a.world_mut().resource_mut::<SkyState>().changed = false;
    a.update();
    assert!(!a.world().resource::<SkyState>().changed, "unchanged exterior frame must not dirty SkyState");

    // Live SetImageSpaceTrait on exterior worldspace WRLD INAM refreshes sky.image_space and sun_scale.
    let base_sun_scale = a.world().resource::<SkyState>().sun_scale();
    source(
        &mut a,
        "SetImageSpaceTrait OwnedImgA 11 2.2\n\
         SetImageSpaceTrait OwnedImgA 25 0.65",
    );
    a.update();
    {
        let sky = a.world().resource::<SkyState>();
        assert!(sky.changed, "live exterior IMGS edit must refresh SkyState");
        assert!(approx(sky.image_space.unwrap().saturation, 0.65));
        assert!(approx(sky.sun_scale(), base_sun_scale * 2.0), "sunlight dimmer 1.1 -> 2.2 doubles sun_scale");
    }

    // Live SetWeatherTraitNumeric (15..=20) updates daytime/nighttime fog and DistanceFog.
    a.world_mut().resource_mut::<SkyState>().changed = false;
    source(
        &mut a,
        "SetWeatherTraitNumeric OwnedWeatherA 15 2500\n\
         SetWeatherTraitNumeric OwnedWeatherA 16 40000\n\
         SetWeatherTraitNumeric OwnedWeatherA 17 1200\n\
         SetWeatherTraitNumeric OwnedWeatherA 18 18000\n\
         SetWeatherTraitNumeric OwnedWeatherA 19 1.6\n\
         SetWeatherTraitNumeric OwnedWeatherA 20 2.4",
    );
    a.update();
    {
        let sky = a.world().resource::<SkyState>();
        assert!(sky.changed, "live exterior WTHR fog edit must refresh SkyState");
        assert_eq!(sky.fog(), Some((2500.0, 40_000.0, 1.0)));
        assert!(approx(sky.fog_power(), 1.6));
        let mut dist_fog = initial_fog();
        apply_cell_fog(sky, &mut dist_fog, 1.0, false);
        assert_eq!(fog_range(&dist_fog.falloff), (2500.0, 40_000.0));
    }

    // Nighttime uses traits 17, 18, 20.
    a.world_mut().resource_mut::<SkyState>().hour = 0.0;
    {
        let sky = a.world().resource::<SkyState>();
        assert_eq!(sky.fog(), Some((1200.0, 18_000.0, 1.0)));
        assert!(approx(sky.fog_power(), 2.4));
    }

    // Weather-fade blending combines live overrides on both previous and current weather.
    {
        let mut sky = a.world_mut().resource_mut::<SkyState>();
        sky.hour = 12.0;
        sky.prev = Some(wthr_b);
        sky.fade = 0.5;
    }
    source(
        &mut a,
        "SetWeatherTraitNumeric OwnedWeatherB 15 500\n\
         SetWeatherTraitNumeric OwnedWeatherB 16 20000\n\
         SetWeatherTraitNumeric OwnedWeatherB 19 0.8",
    );
    a.update();
    {
        let sky = a.world().resource::<SkyState>();
        assert_eq!(sky.fog(), Some((1500.0, 30_000.0, 1.0)));
        assert!(approx(sky.fog_power(), 1.2));
    }

    // Explicit console/CLI fog_override keeps precedence over live SetWeatherTraitNumeric.
    a.world_mut().resource_mut::<SkyState>().fog_override = Some((300.0, 9000.0));
    {
        let sky = a.world().resource::<SkyState>();
        assert!(approx(sky.fog_power(), 1.0));
        let mut dist_fog = initial_fog();
        apply_cell_fog(sky, &mut dist_fog, 1.0, false);
        assert_eq!(fog_range(&dist_fog.falloff), (300.0, 9000.0));
    }

    // Game-instance switch restores authored exterior fog and worldspace image space.
    {
        let mut sky = a.world_mut().resource_mut::<SkyState>();
        sky.fog_override = None;
        sky.prev = None;
        sky.fade = 1.0;
    }
    let (fresh_script, fresh_sim) = runtime(play, forms, refs);
    a.insert_resource(fresh_script);
    a.insert_resource(fresh_sim);
    a.update();
    {
        let sky = a.world().resource::<SkyState>();
        assert!(sky.weather_fog_overrides.is_empty());
        assert_eq!(sky.fog(), Some((1000.0, 100_000.0, 1.0)));
        assert!(approx(sky.fog_power(), 1.0));
        assert!(approx(sky.image_space.unwrap().saturation, 1.0));
    }
}

#[test]
fn carry_climate_sky_preserves_live_exterior_imagespace_override() {
    let mut from = authored_session_sky();
    from.world_image_space_id = Some(IMGS_A);
    let live_grade = fnv_world::imagespace::Cinematic {
        saturation: 0.42,
        brightness: 1.35,
        ..Default::default()
    };
    from.image_space = Some(live_grade);

    // Same worldspace INAM (e.g. ForceClimate / SetCurrentClimate rebuild): preserve live override.
    let mut same_world = authored_session_sky();
    same_world.world_image_space_id = Some(IMGS_A);
    same_world.image_space = Some(fnv_world::imagespace::Cinematic::default());
    crate::worldspace::carry_climate_sky(&from, &mut same_world, |_| None);
    assert_eq!(same_world.image_space, Some(live_grade));

    // Different worldspace INAM: do not carry over the old world's image space.
    let mut other_world = authored_session_sky();
    other_world.world_image_space_id = Some(IMGS_B);
    let other_base = fnv_world::imagespace::Cinematic {
        saturation: 0.6,
        ..Default::default()
    };
    other_world.image_space = Some(other_base);
    crate::worldspace::carry_climate_sky(&from, &mut other_world, |_| None);
    assert_eq!(other_world.image_space, Some(other_base));
}

#[test]
fn cell_image_space_resolves_new_worldspace_inam_when_sky_is_reused() {
    let (world, play, forms, refs) = fixture(WRLD_EXT, None);
    let (script, sim) = runtime(play, forms, refs);
    let sky = authored_session_sky();

    let mut a = App::new();
    a.insert_resource(world)
        .insert_resource(test_streamer(None))
        .insert_resource(sky)
        .insert_resource(script)
        .insert_resource(sim)
        .add_message::<nvf_play::Travel>()
        .add_systems(Update, cell_image_space);

    // Initial exterior frame resolves WRLD_EXT -> IMGS_A, then applies a live edit.
    a.update();
    source(&mut a, "SetImageSpaceTrait OwnedImgA 25 0.77");
    a.update();
    {
        let sky = a.world().resource::<SkyState>();
        assert_eq!(sky.world_image_space_id, Some(IMGS_A));
        assert!(approx(sky.image_space.unwrap().saturation, 0.77));
    }

    // Switch world.world to WRLD_EXT_B without rebuilding SkyState (simulating the
    // worldspace::switch Err fallback where sky.world_image_space_id still holds IMGS_A).
    a.world_mut().resource_mut::<WorldData>().set_world(WRLD_EXT_B);
    a.update();
    {
        let sky = a.world().resource::<SkyState>();
        assert_eq!(sky.world_image_space_id, Some(IMGS_B));
        let grade = sky.image_space.expect("WRLD_EXT_B INAM must resolve IMGS_B");
        assert!(approx(grade.saturation, 0.6), "expected IMGS_B saturation 0.6, got {}", grade.saturation);
        assert!(approx(grade.brightness, 1.25), "expected IMGS_B brightness 1.25, got {}", grade.brightness);
    }
}

#[test]
fn live_set_worldspace_imagespace_and_exterior_cell_imagespace_precedence() {
    let (world, play, forms, refs) = fixture(WRLD_EXT, None);
    let (script, sim) = runtime(play, forms, refs);
    let mut streamer = test_streamer(None);
    streamer.centre = Some((0, 0));

    let mut a = App::new();
    a.insert_resource(Time::<()>::default())
        .insert_resource(world)
        .insert_resource(streamer)
        .insert_resource(authored_session_sky())
        .insert_resource(script)
        .insert_resource(sim)
        .init_resource::<ScreenModifiers>()
        .add_message::<nvf_play::Travel>()
        .add_systems(Update, (cell_image_space, update_grade).chain());
    let cam = a.world_mut().spawn(ImageSpaceGrade::default()).id();

    // Cold exterior starts with authored WRLD_EXT INAM (OwnedImgA).
    a.update();
    assert_eq!(a.world().resource::<SkyState>().world_image_space_id, Some(IMGS_A));
    assert!(approx(a.world().get::<ImageSpaceGrade>(cam).unwrap().saturation, 1.0));
    assert!(approx(a.world().get::<ImageSpaceGrade>(cam).unwrap().brightness, 1.0));

    // Unchanged frame does zero work.
    a.world_mut().resource_mut::<SkyState>().changed = false;
    a.update();
    assert!(!a.world().resource::<SkyState>().changed);

    // Live SetWorldspaceImagespace switches active exterior IMGS immediately without travel.
    source(&mut a, "SetWorldspaceImagespace OwnedExtWorld OwnedImgB");
    a.update();
    assert_eq!(a.world().resource::<SkyState>().world_image_space_id, Some(IMGS_B));
    assert!(a.world().resource::<SkyState>().changed);
    let g = *a.world().get::<ImageSpaceGrade>(cam).unwrap();
    assert!(approx(g.saturation, 0.6), "SetWorldspaceImagespace saturation: {}", g.saturation);
    assert!(approx(g.brightness, 1.25), "SetWorldspaceImagespace brightness: {}", g.brightness);

    // Live SetImageSpaceTrait on top of the SetWorldspaceImagespace override refreshes immediately.
    source(&mut a, "SetImageSpaceTrait OwnedImgB 25 0.44");
    a.update();
    assert!(approx(a.world().get::<ImageSpaceGrade>(cam).unwrap().saturation, 0.44));

    // Child worldspace with PNAM Use Image Space Data inherits parent's SetWorldspaceImagespace override.
    a.world_mut().resource_mut::<WorldData>().set_world(WRLD_CHILD);
    a.world_mut().resource_mut::<Streamer>().centre = None;
    a.update();
    assert_eq!(a.world().resource::<SkyState>().world_image_space_id, Some(IMGS_B));
    assert!(approx(a.world().get::<ImageSpaceGrade>(cam).unwrap().saturation, 0.44));

    // Return to WRLD_EXT cell (0, 0) (OwnedExtCellA).
    a.world_mut().resource_mut::<WorldData>().set_world(WRLD_EXT);
    a.world_mut().resource_mut::<Streamer>().centre = Some((0, 0));
    a.update();
    assert_eq!(a.world().resource::<SkyState>().world_image_space_id, Some(IMGS_B));

    // SetCellImageSpace on the currently occupied exterior cell waits until next cell load.
    source(&mut a, "SetCellImageSpace OwnedExtCellA OwnedImgC");
    a.world_mut().resource_mut::<SkyState>().changed = false;
    a.update();
    assert_eq!(a.world().resource::<SkyState>().world_image_space_id, Some(IMGS_B));
    assert!(!a.world().resource::<SkyState>().changed);

    // Moving to OwnedExtCellB (1, 0) keeps worldspace override (IMGS_B), then re-entering
    // OwnedExtCellA (0, 0) binds OwnedImgC ahead of SetWorldspaceImagespace.
    a.world_mut().resource_mut::<Streamer>().centre = Some((1, 0));
    a.update();
    assert_eq!(a.world().resource::<SkyState>().world_image_space_id, Some(IMGS_B));
    a.world_mut().resource_mut::<Streamer>().centre = Some((0, 0));
    a.update();
    assert_eq!(a.world().resource::<SkyState>().world_image_space_id, Some(IMGS_C));
    let g = *a.world().get::<ImageSpaceGrade>(cam).unwrap();
    assert!(approx(g.saturation, 0.3), "exterior SetCellImageSpace saturation: {}", g.saturation);
    assert!(approx(g.brightness, 1.4), "exterior SetCellImageSpace brightness: {}", g.brightness);

    // Fresh Game instance clears session SetWorldspaceImagespace and exterior SetCellImageSpace.
    let (fresh_script, fresh_sim) = runtime(play, forms, refs);
    a.insert_resource(fresh_script);
    a.insert_resource(fresh_sim);
    a.update();
    assert_eq!(a.world().resource::<SkyState>().world_image_space_id, Some(IMGS_A));
    assert!(approx(a.world().get::<ImageSpaceGrade>(cam).unwrap().saturation, 1.0));
}

#[test]
fn live_weather_rgb_colors_adapters_and_transition_override_reach_sky_and_grade() {
    let (world, play, forms, refs) = fixture(WRLD_EXT, None);
    let (script, sim) = runtime(play, forms, refs);
    let wthr_a = fnv_world::sky::weather(world.lo, world.schema, WTHR_A).unwrap();
    let wthr_b = fnv_world::sky::weather(world.lo, world.schema, WTHR_B).unwrap();

    let mut sky = authored_session_sky();
    sky.climate.sunrise = (6.0, 7.0);
    sky.climate.sunset = (18.0, 19.0);
    sky.weather = wthr_a;
    sky.hour = 7.0; // sunrise.1 = exact 100% Day key on 6-time NAM0/PNAM weather
    sky.changed = true;

    let mut a = App::new();
    a.insert_resource(Time::<()>::default())
        .insert_resource(world)
        .insert_resource(test_streamer(None))
        .insert_resource(sky)
        .insert_resource(script)
        .insert_resource(sim)
        .init_resource::<ScreenModifiers>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<SkyMaterial>>()
        .init_resource::<ClearColor>()
        .init_resource::<GlobalAmbientLight>()
        .add_message::<nvf_play::Travel>()
        .add_systems(
            Update,
            (cell_image_space, sync_weather_fog, sync_weather_visuals, update_sky, update_grade, tell_sky_colors).chain(),
        );
    let cam = a
        .world_mut()
        .spawn((
            Transform::default(),
            DistanceFog::default(),
            ImageSpaceGrade::default(),
            FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 },
        ))
        .id();
    let sun = a.world_mut().spawn((DirectionalLight::default(), Transform::default(), Sun)).id();

    // Initial authored exterior frame.
    a.update();
    let initial_fog_color = a.world().get::<DistanceFog>(cam).unwrap().color;
    let initial_ambient = a.world().resource::<GlobalAmbientLight>().color;
    let initial_sun = a.world().get::<DirectionalLight>(sun).unwrap().color;
    let initial_sun_scale = a.world().resource::<SkyState>().sun_scale();
    assert!(approx(a.world().get::<ImageSpaceGrade>(cam).unwrap().contrast, 1.0));

    // Unchanged frame does zero churn.
    a.update();
    assert!(!a.world().resource::<SkyState>().changed);

    // Live SetWeatherRGBColor across Fog (1), Ambient (3), Sunlight (4), and PNAM cloud layer 0 (2).
    source(
        &mut a,
        "SetWeatherRGBColor OwnedWeatherA 1 1 200100050\n\
         SetWeatherRGBColor OwnedWeatherA 3 1 040080120\n\
         SetWeatherRGBColor OwnedWeatherA 4 1 255180090\n\
         SetWeatherRGBColor OwnedWeatherA 2 1 120140160 0",
    );
    a.update();
    {
        let sky = a.world().resource::<SkyState>();
        assert_eq!(sky.color_bytes(ColorType::Fog), [200.0, 100.0, 50.0]);
        assert_eq!(sky.color_bytes(ColorType::Ambient), [40.0, 80.0, 120.0]);
        assert_eq!(sky.color_bytes(ColorType::Sunlight), [255.0, 180.0, 90.0]);
        let cloud = sky.cloud_color_for(&sky.weather, 0);
        assert!(approx(cloud[0], fnv_world::sky::srgb_to_linear(120)));
        assert!(approx(cloud[1], fnv_world::sky::srgb_to_linear(140)));
        assert!(approx(cloud[2], fnv_world::sky::srgb_to_linear(160)));
    }
    assert_ne!(a.world().get::<DistanceFog>(cam).unwrap().color, initial_fog_color);
    assert_ne!(a.world().resource::<GlobalAmbientLight>().color, initial_ambient);
    assert_ne!(a.world().get::<DirectionalLight>(sun).unwrap().color, initial_sun);

    // Live SetWeatherImageSpaceMod attaches OwnedAdptA to Day (slot 1): contrast *1.5, sun dimmer *2.0, tint alpha 0.6.
    source(&mut a, "SetWeatherImageSpaceMod OwnedWeatherA 1 OwnedAdptA");
    a.update();
    let g = *a.world().get::<ImageSpaceGrade>(cam).unwrap();
    assert!(approx(g.contrast, 1.5), "weather adapter contrast: {}", g.contrast);
    assert!(approx(g.tint.w, 0.6), "weather adapter tint alpha: {}", g.tint.w);
    assert!(
        approx(a.world().resource::<SkyState>().sun_scale(), initial_sun_scale * 2.0),
        "weather adapter sunlight dimmer must double sun_scale"
    );

    // Explicit clear (SetWeatherImageSpaceMod OwnedWeatherA 1 0) removes the adapter immediately.
    source(&mut a, "SetWeatherImageSpaceMod OwnedWeatherA 1 0");
    a.update();
    assert!(approx(a.world().get::<ImageSpaceGrade>(cam).unwrap().contrast, 1.0));
    assert!(approx(a.world().resource::<SkyState>().sun_scale(), initial_sun_scale));

    // SetWeatherTransitionTimeOverride (10s) accelerates weather fade and blends overridden colors across prev/weather.
    {
        let mut sky = a.world_mut().resource_mut::<SkyState>();
        sky.prev = Some(wthr_b);
        sky.fade = 0.0;
    }
    source(
        &mut a,
        "SetWeatherTransitionTimeOverride 10\n\
         SetWeatherRGBColor OwnedWeatherB 1 1 000200150",
    );
    a.update();
    {
        let mut sky = a.world_mut().resource_mut::<SkyState>();
        // Advance 5 real seconds -> 5 / 10 = 0.5 fade (without override, 255 trans_delta = 30s -> 0.1667).
        sky.advance_fade(5.0 * (30.0 / 3600.0));
        assert!(approx(sky.fade, 0.5), "expected 0.5 fade under 10s override, got {}", sky.fade);
        assert_eq!(sky.color_bytes(ColorType::Fog), [100.0, 150.0, 100.0]);
    }

    // Fresh Game instance clears all weather visual overrides and restores authored colors/transition.
    {
        let mut sky = a.world_mut().resource_mut::<SkyState>();
        sky.prev = None;
        sky.fade = 1.0;
    }
    let (fresh_script, fresh_sim) = runtime(play, forms, refs);
    a.insert_resource(fresh_script);
    a.insert_resource(fresh_sim);
    a.update();
    let sky = a.world().resource::<SkyState>();
    assert_eq!(sky.weather_visual_overrides, WeatherVisualOverrides::default());
    assert_eq!(sky.color_bytes(ColorType::Fog), [30.0, 40.0, 50.0]);
}

#[test]
fn on_cell_enter_script_updates_cell_lighting_fog_and_imagespace_live() {
    let (world, play, forms, refs) = fixture(0, Some(CELL_INT_LIGHT));
    let (script, sim) = runtime(play, forms, refs);
    let mut sky = authored_session_sky();
    sky.set_interior(crate::world::effective_cell_lighting(&world, None, CELL_INT_LIGHT));

    let mut a = App::new();
    a.insert_resource(Time::<()>::default())
        .insert_resource(world)
        .insert_resource(test_streamer(Some(CELL_INT_LIGHT)))
        .insert_resource(sky)
        .insert_resource(script)
        .insert_resource(sim)
        .init_resource::<ScreenModifiers>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<SkyMaterial>>()
        .init_resource::<ClearColor>()
        .init_resource::<GlobalAmbientLight>()
        .add_message::<nvf_play::Travel>()
        .add_systems(
            Update,
            (cell_lighting, cell_image_space, update_sky, interior_clip, update_grade).chain(),
        );
    let cam = a
        .world_mut()
        .spawn((
            Transform::default(),
            DistanceFog::default(),
            Projection::Perspective(PerspectiveProjection::default()),
            ImageSpaceGrade::default(),
            FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 },
        ))
        .id();
    let _sun = a.world_mut().spawn((DirectionalLight::default(), Transform::default(), Sun)).id();

    // Baseline frame in CELL_INT_LIGHT (template LGTM_A: ambient [10,15,20], clip 8000, fog 100..4000).
    a.update();
    let far = |a: &App| match a.world().get::<Projection>(cam).unwrap() {
        Projection::Perspective(p) => p.far,
        _ => panic!("expected perspective camera"),
    };
    assert_eq!(far(&a), 8000.0);
    assert_eq!(fog_range(&a.world().get::<DistanceFog>(cam).unwrap().falloff), (100.0, 4000.0));
    assert!(approx(a.world().get::<ImageSpaceGrade>(cam).unwrap().saturation, 1.0));

    // Register a real OnCellEnter UDF that applies template, fog, XCLL, and IMGS overrides on entry.
    source(
        &mut a,
        "SetEventHandler \"OnCellEnter\" (Begin Function {ref rCell}\n\
         if rCell == OwnedIntCellLight\n\
             SetCellLightingTemplate rCell OwnedCartoLgtmB\n\
             SetLightingTemplateTraitNumeric OwnedCartoLgtmB 10 450\n\
             SetLightingTemplateTraitNumeric OwnedCartoLgtmB 11 11500\n\
             SetLightingTemplateTraitNumeric OwnedCartoLgtmB 15 26000\n\
             SetImageSpaceTrait OwnedImgA 25 0.55\n\
         elseif rCell == OwnedIntCellNoXcll\n\
             SetInteriorLightingTraitNumeric rCell 0 65\n\
             SetInteriorLightingTraitNumeric rCell 12 180\n\
             SetInteriorLightingTraitNumeric rCell 13 3600\n\
             SetInteriorLightingTraitNumeric rCell 15 14000\n\
         endif\n\
         End)",
    );

    // Prime last_cell in another cell, then enter OwnedIntCellLight -> OnCellEnter fires!
    a.world_mut().resource_scope(|world, mut script: Mut<ScriptState>| {
        let mut sim = world.resource_mut::<SimState>();
        with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
            rt.game.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(CELL_INT));
            rt.game.tick();
            rt.game.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(CELL_INT_LIGHT));
            rt.game.tick();
        });
    });

    a.update();
    assert_eq!(a.world().resource::<SkyState>().interior.unwrap().ambient, [90, 100, 110]);
    assert_eq!(fog_range(&a.world().get::<DistanceFog>(cam).unwrap().falloff), (450.0, 11500.0));
    assert_eq!(far(&a), 26000.0);
    assert!(approx(a.world().get::<ImageSpaceGrade>(cam).unwrap().saturation, 0.55));

    // Now enter the XCLL-less interior cell OwnedIntCellNoXcll -> OnCellEnter writes XCLL traits live!
    a.world_mut().resource_mut::<Streamer>().interior = Some(CELL_INT_NO_XCLL);
    a.world_mut().resource_scope(|world, mut script: Mut<ScriptState>| {
        let mut sim = world.resource_mut::<SimState>();
        with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
            rt.game.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(CELL_INT_NO_XCLL));
            rt.game.tick();
        });
    });

    a.update();
    let synth = a.world().resource::<SkyState>().interior.expect("synthesized XCLL interior lighting");
    assert_eq!(synth.ambient, [65, 0, 0]);
    assert_eq!(fog_range(&a.world().get::<DistanceFog>(cam).unwrap().falloff), (180.0, 3600.0));
    assert_eq!(far(&a), 14000.0);
}
