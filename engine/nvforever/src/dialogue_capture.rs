//! PC capture using the same archive, NIF, material and dialogue plugins as the game.
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    ui::IsDefaultUiCamera,
    window::WindowResolution,
};
use clap::Parser;
use fnv_sim::FormId;
use nvf_play::dialogue_touch::model_capture::{self, Scene};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const CASING: &str = "Textures/Interface/NVForever/Dialogue/classic.png";

#[derive(Parser)]
struct Args {
    #[arg(long)]
    game: PathBuf,
    #[arg(long)]
    nif_schema: PathBuf,
    #[arg(long)]
    esm_schema: PathBuf,
    /// Classic industrial frame override; uses the bundled frame if absent.
    #[arg(long)]
    classic: Option<PathBuf>,
    /// New evidence directory, outside the game installation.
    #[arg(long)]
    output: PathBuf,
    #[arg(long,value_parser=["Normal","Classic","Field Notes","Retro Console","Wrist Terminal"])]
    theme: String,
    #[arg(long,value_parser=["choices","speech","last"])]
    scene: String,
    /// Logical width. The logical height is 360.
    #[arg(long, default_value_t = 640)]
    width: u32,
    #[arg(long, default_value_t = 1)]
    density: u32,
    /// Placed ACHR from the original master, for Classic. Must admit a greeting.
    #[arg(long)]
    speaker: Option<String>,
    /// Original master ARMO FormID, hexadecimal. Required for Wrist Terminal.
    #[arg(long)]
    pipboy_armo: Option<String>,
}

#[derive(Resource)]
pub(crate) struct CaptureWorn(pub(crate) Vec<FormId>);

fn original_model(data: &nvf_assets::GameData, path: &str) {
    assert!(
        data.vfs.source_of(path).is_some_and(|name| Path::new(name)
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("Fallout - Meshes.bsa"))),
        "original master mesh archive must provide {path}"
    );
}

fn actor_provider(app: &mut App, game: &fnv_install::GameInstall, args: &Args) {
    let opened = crate::world::open_plugins(&game.data, &[], &["FalloutNV.esm".into()]);
    assert!(
        opened.dropped.is_empty() && opened.plugins.len() == 1,
        "original master required"
    );
    let lo = fnv_esm::LoadOrder::new(&opened.plugins).expect("master load order");
    let schema = fnv_esm::Schema::load_dir(&args.esm_schema).expect("ESM schema");
    let actors = crate::actors::ActorWorld::build(&lo, &schema);
    if args.theme == "Classic" {
        let hex = args
            .speaker
            .as_ref()
            .expect("--speaker requires a placed ACHR for Classic");
        let id = FormId(u32::from_str_radix(hex.trim_start_matches("0x"), 16).expect("hex ACHR"));
        let index = fnv_world::WorldIndex::build(&lo, &schema);
        let mut sim = nvf_play::sim::SimState::new(&lo, &schema, &index);
        let base = sim
            .refs
            .get(id)
            .filter(|r| &r.sig == b"ACHR")
            .expect("selected record must be a placed NPC")
            .base;
        let mut script = nvf_play::script::fixtures::over_plugins(&opened.plugins);
        nvf_play::script::with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
            rt.game.start_dialogue(id, None)
        })
        .expect("original NPC greeting");
        app.insert_resource(script)
            .insert_resource(sim)
            .insert_resource(Speaker {
                base,
                reference: id,
            })
            .init_resource::<nvf_assets::actor::ActorParts>()
            .init_resource::<nvf_play::dialogue_look::SpeakerFace>()
            .add_systems(Startup, spawn_speaker)
            .add_systems(
                PostUpdate,
                read_face.after(bevy::transform::TransformSystems::Propagate),
            );
    } else {
        let hex = args
            .pipboy_armo
            .as_ref()
            .expect("--pipboy-armo required for Wrist Terminal");
        let id = FormId(u32::from_str_radix(hex.trim_start_matches("0x"), 16).expect("hex ARMO"));
        assert!(
            actors
                .actors
                .armors
                .get(&id)
                .is_some_and(|a| a.slots & fnv_actor::resolve::slot::PIPBOY != 0),
            "selected ARMO must occupy the Pip-Boy slot"
        );
        let model =
            crate::dialogue_pipboy::model_path(&actors, &[id]).expect("actual worn Pip-Boy model");
        original_model(app.world().resource::<nvf_assets::GameData>(), &model);
        let appearance = actors
            .resolve_as(
                FormId::PLAYER_BASE.0,
                FormId::PLAYER_REF.0,
                Some(&[id]),
                None,
            )
            .expect("player appearance from master");
        assert!(
            appearance
                .parts
                .iter()
                .any(|p| p.role == "armor" && p.form == id && !p.model.is_empty()),
            "player has no resolved selected armor part"
        );
        app.insert_resource(CaptureWorn(vec![id]));
    }
    app.insert_resource(actors);
}

#[derive(Resource)]
struct Speaker {
    base: FormId,
    reference: FormId,
}

fn spawn_speaker(
    mut commands: Commands,
    speaker: Res<Speaker>,
    actors: Res<crate::actors::ActorWorld>,
    mut parts: ResMut<nvf_assets::actor::ActorParts>,
    mut models: nvf_assets::model::ModelCtx,
) {
    let parent = commands
        .spawn((crate::world::game_to_bevy(), Visibility::Inherited))
        .id();
    let mut ctx = nvf_assets::actor::ActorCtx {
        cache: &mut parts,
        models: &mut models,
    };
    let mut stats = crate::actors::ActorStats::default();
    assert!(
        crate::actors::spawn(
            &mut commands,
            parent,
            bevy::math::Affine3A::IDENTITY,
            Vec3::ZERO,
            speaker.reference.0,
            speaker.base.0,
            &actors,
            None,
            &mut ctx,
            &mut stats,
            false
        ),
        "selected NPC body required"
    );
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            ..default()
        },
        Transform::from_xyz(50.0, 120.0, -140.0).looking_at(Vec3::Y * 65.0, Vec3::Y),
    ));
}

fn read_face(
    speaker: Res<Speaker>,
    rigs: Query<&crate::limbs::ActorRig>,
    nodes: Query<&GlobalTransform>,
    mut face: ResMut<nvf_play::dialogue_look::SpeakerFace>,
) {
    let Some(rig) = rigs.iter().find(|r| r.reference == speaker.reference.0) else {
        return;
    };
    let Some(index) = rig.skel.by_name.get("Bip01 Head") else {
        return;
    };
    if let Ok(head) = nodes.get(rig.nodes[*index]) {
        face.0 = Some((
            speaker.reference.0,
            nvf_play::space::to_game(head.translation()),
        ));
    }
}

#[derive(Resource)]
struct Capture {
    args: Args,
    scene: Scene,
    ready_frames: u32,
    submitted: bool,
    observed: bool,
    deadline: Instant,
    finished: Arc<AtomicBool>,
}

fn capture(world: &mut World) {
    let state = world.resource::<Capture>();
    assert!(
        Instant::now() < state.deadline,
        "dialogue capture timed out for {} {}",
        state.args.theme,
        state.args.scene
    );
    let path = state.args.output.join("dialogue.png");
    if state.observed {
        let bytes = std::fs::read(path).expect("screenshot observer saved file");
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().expect("PNG header");
        let mut pixels = vec![0; reader.output_buffer_size().expect("PNG buffer")];
        let frame = reader
            .next_frame(&mut pixels)
            .expect("complete screenshot PNG");
        assert_eq!(
            (frame.width, frame.height),
            (
                state.args.width * state.args.density,
                360 * state.args.density
            )
        );
        let size = frame.color_type.samples();
        let mut samples = pixels[..frame.buffer_size()].chunks_exact(size);
        let first = samples.next().expect("screenshot pixels");
        assert!(samples.any(|p| p != first), "uniform screenshot");
        state.finished.store(true, Ordering::Release);
        world.write_message(AppExit::Success);
        return;
    }
    if state.submitted {
        return;
    }
    let (theme, scene, density) = (
        state.args.theme.clone(),
        state.scene,
        state.args.density as f32,
    );
    let ready = model_capture::ready(world, &theme, scene, density);
    let mut state = world.resource_mut::<Capture>();
    state.ready_frames = if ready { state.ready_frames + 1 } else { 0 };
    if state.ready_frames < 20 {
        return;
    }
    state.submitted = true;
    world
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path))
        .observe(|_: On<ScreenshotCaptured>, mut state: ResMut<Capture>| state.observed = true);
}

pub fn run() {
    let args = Args::parse();
    assert!(
        matches!(args.width, 640 | 800) && matches!(args.density, 1 | 2),
        "supported logical widths 640/800 and densities 1/2"
    );
    let install = fnv_install::GameInstall::open(&args.game).expect("original game install");
    crate::setup::install_fonts(&install, None);
    assert!(
        args.output.is_absolute(),
        "absolute new output directory required"
    );
    let parent = args
        .output
        .parent()
        .expect("output parent")
        .canonicalize()
        .expect("existing output parent");
    assert!(
        !parent.starts_with(install.root.canonicalize().expect("canonical install")),
        "output must be outside game install"
    );
    std::fs::create_dir(&args.output).expect("fresh output directory");
    let overlay = args.output.join("overlay");
    let art = if let Some(path) = args.classic.as_ref().filter(|_| args.theme == "Classic") {
        let dest = overlay.join(CASING);
        let source = std::fs::read(path).expect("Classic casing override");
        std::fs::create_dir_all(dest.parent().unwrap()).expect("owned asset overlay");
        std::fs::write(&dest, &source).expect("stage unchanged casing");
        Some((dest, source))
    } else {
        None
    };
    let archives = nvf_assets::GameData::open_for_plugins_in(
        &install.data,
        art.as_ref().map(|_| overlay.as_path()),
        &["FalloutNV.esm"],
        &args.nif_schema,
    )
    .expect("original archives and NIF schema");
    assert!(
        archives.vfs.broken_archives().is_empty(),
        "damaged original archives"
    );
    if let Some((dest, source)) = art {
        assert_eq!(
            archives
                .vfs
                .loose_path(CASING)
                .and_then(|p| p.canonicalize().ok()),
            Some(dest.canonicalize().unwrap()),
            "overlay must supply casing"
        );
        assert!(
            archives.read(CASING).is_some_and(|bytes| bytes == source),
            "VFS casing mismatch"
        );
    }
    if args.theme == "Retro Console" {
        original_model(&archives, crate::terminal_view::MONITOR_MODEL);
    }
    let scene = match args.scene.as_str() {
        "speech" => Scene::Speech,
        "last" => Scene::Last,
        _ => Scene::Choices,
    };
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "NVForever dialogue model capture".into(),
                resizable: false,
                resolution: WindowResolution::new(args.width * args.density, 360 * args.density)
                    .with_scale_factor_override(args.density as f32),
                ..default()
            }),
            ..default()
        }),
    );
    app.add_plugins((
        nvf_assets::ArchivePlugin(archives),
        crate::terminal_view::DialogueTerminalPlugin,
    ));
    if matches!(args.theme.as_str(), "Wrist Terminal" | "Classic") {
        actor_provider(&mut app, &install, &args);
    }
    crate::dialogue_pipboy::install(&mut app);
    app.world_mut().spawn((
        Camera3d::default(),
        nvf_play::PlayerCamera,
        IsDefaultUiCamera,
        Transform::from_xyz(0.0, 100.0, -125.0).looking_at(Vec3::Y * 92.0, Vec3::Y),
    ));
    model_capture::install_scene(&mut app, &args.theme, scene);
    let finished = Arc::new(AtomicBool::new(false));
    app.insert_resource(Capture {
        args,
        scene,
        ready_frames: 0,
        submitted: false,
        observed: false,
        deadline: Instant::now() + Duration::from_secs(120),
        finished: finished.clone(),
    });
    app.add_systems(Last, capture);
    let result = app.run();
    assert!(
        matches!(result, AppExit::Success) && finished.load(Ordering::Acquire),
        "capture incomplete"
    );
}
