//! nvforever — the NVForever game binary. Started with no argument that chooses a world or
//! a mode (`MODE_ARGS`; e.g. double-clicked, or with `--data` and window or sound switches
//! only) it is the game on its main menu, as `--play --menu`. `--explore`, or `--cell`,
//! `--pos` and the other world arguments without `--play`, open the free-camera explorer:
//! terrain and objects of a worldspace from the user's own game data, streamed around the
//! camera (docs/design/layer3.md). Read-only: plugins and archives are only read.
//! Without `--esm-schema`, `--nif-schema` and NVF_PLAY_SCRIPT_FUNCTIONS the schemas are
//! looked for in `schemas/` beside the program (the release archive's layout,
//! tools/release/README-release.md), else in the build's checkout.
//!
//! Keys: W/A/S/D move, Q/E down/up, Shift faster, right mouse drag look, wheel speed,
//! F12 screenshot, Esc quit. With `--play` the camera is the player instead (Layer 4,
//! `nvf-play`: walking with collision, activation, menus; docs/design/layer4-play.md),
//! Esc opens the pause menu (Continue / Save / Load / Settings / Quit; the settings page
//! switches what `settings.rs` and `nvf_play::settings` list while the game runs) and
//! closes the open menu. `--play --menu` opens on the main menu (`nvf_play::main_menu`).

/// The console lines written before the log is up, with the player's home folder hidden
/// like every logged line (crash.rs, Privacy). Shadows `std::eprintln!` in this crate.
macro_rules! eprintln {
    ($($t:tt)*) => {
        $crate::crash::eprint_line(format_args!($($t)*))
    };
}

mod activators;
mod reports;
mod native_crash;
mod nif_mirror;
mod anim_paths;
mod actor_feet;
mod tumble;
mod actor_jumps;
mod actor_materials;
mod anim_objects;
mod anim_state;
mod actor_groups;
mod actor_scale;
mod actors;
mod attack_anims;
mod authored_pose;
mod cell_buffer;
mod clutter_pose;
mod cell_resets;
mod alert;
mod anim_sounds;
mod cpu_placement;
mod crash;
mod events_log;
mod creature_pace;
mod creature_stance;
mod creator_view;
mod death_cam;
mod deaths;
mod decals;
mod dismember;
mod knockouts;
mod player_knockdown;
mod detection;
mod dialogue_look;
mod door_fade;
mod draw_distance;
mod doors;
mod emittance;
mod fade;
mod face_morphs;
mod facegen;
mod flashlight;
mod eye_adapt;
mod fog;
mod fly;
mod foot_ik;
mod anti_lag;
mod frame_pace;
mod frame_telemetry;
mod blit_warm;
mod handoff;
mod furniture;
mod graphics;
mod grass;
mod head_look;
mod idles;
mod lights;
mod limbs;
mod lips;
mod memory_budget;
mod gpu_poll;
mod pipeline_gate;
mod lockpick_view;
mod roulette_view;
mod blackjack_view;
mod body_weapons;
mod terminal_view;
mod scope_lens;
mod scope_view;
mod poker_view;
mod lod;
mod locomotion;
mod combat_face;
mod combat_stance;
mod combat_tactics;
mod console_toggles;
mod npc_combat;
mod destroyed;
mod npc_grenades;
mod npc_stimpaks;
mod use_weapon;
mod ui_attach;
mod provoked;
mod weapons;
mod lodgen;
mod nav;
mod object_anims;
mod outfits;
mod own_sounds;
mod pipboy_arm;
mod pipboy_controls;
mod pipboy_ready;
mod player_body;
mod stealth_field;
mod stray_commands;
mod post;
mod quality;
mod particles;
mod shot_fx;
mod ragdoll;
mod swing_traps;
mod wrecks;
mod talk_clips;
mod readahead;
mod reflection;
mod refraction;
mod residents;
mod resurrect;
mod appearance;
mod script_look;
mod ripples;
mod settings;
mod iron_sights;
mod head_bob;
mod view_sway;
mod weapon_lower;
mod setup;
mod shadow_probe;
mod shadow_thin;
mod shafts;
mod imagespace;
mod imposters;
mod sky;
mod start_times;
mod weather_fx;
mod limping;
mod sneaking;
mod swimming;
mod ssao;
mod taa;
mod fg_presenter;
mod frame_gen;
mod fsr;
mod speech_look;
mod stress_sample;
mod profile;
mod perf_overlay;
mod occlusion;
mod rooms;
mod terrain;
mod texture_swap;
mod effect_particles;
mod effect_shaders;
mod vats_highlight;
mod tree;
mod treelod;
mod underwater;
mod vigor_view;
mod water;
mod window_icon;
mod window_mode;
mod world;
mod worldspace;
mod zones;

use bevy::image::{ImageAddressMode, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use clap::Parser;
use fly::FlyCam;
use fnv_world::land::{load_land, LandParts, SIDE};
use nvf_assets::{ArchivePlugin, GameData, ModelCache, TextureCache};
use std::path::PathBuf;
use terrain::{TerrainCache, TerrainSettings};
use world::{game_to_bevy, Streamer, WorldData};

const MASTERS: [&str; 10] = [
    "FalloutNV.esm",
    "DeadMoney.esm",
    "HonestHearts.esm",
    "OldWorldBlues.esm",
    "LonesomeRoad.esm",
    "GunRunnersArsenal.esm",
    "ClassicPack.esm",
    "MercenaryPack.esm",
    "TribalPack.esm",
    "CaravanPack.esm",
];

/// NVForever: Fallout: New Vegas on your own game data.
///
/// With no argument that chooses a world or a mode it opens on the main menu (as
/// `--play --menu`). The free-camera explorer needs `--explore`, or `--cell`, `--pos` or
/// another world argument without `--play`. Without the schema options the schemas are
/// read from `schemas/` beside the program when it is there (the release archive).
#[derive(Parser, Clone, Resource)]
#[command(name = "nvforever", version)]
struct Args {
    /// The game's folder or its Data folder (read-only). Defaults to $NVFOREVER_DATA, else
    /// the folder stored in NVForever.ini, else it is asked for (setup.rs). After start-up
    /// it is always the Data folder.
    #[arg(long, env = "NVFOREVER_DATA")]
    data: Option<PathBuf>,
    /// Plugins in load order (default: those turned on in NVForever's own plugins.txt in
    /// its home; with --data, the base game and the DLC masters present).
    #[arg(long, num_args = 1.., default_values_t = MASTERS.map(String::from))]
    plugins: Vec<String>,
    /// Record schema directory.
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv"))]
    esm_schema: PathBuf,
    /// NIF schema directory.
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/nif"))]
    nif_schema: PathBuf,
    /// Worldspace Editor ID or FormID.
    #[arg(long, default_value = "WastelandNV")]
    world: String,
    /// Start in this exterior cell: Editor ID (e.g. Goodsprings) or "x,y".
    #[arg(long, default_value = "Goodsprings", allow_hyphen_values = true)]
    cell: String,
    /// Start at this game position "x,y,z" instead of above the cell's centre.
    #[arg(long, allow_hyphen_values = true)]
    pos: Option<String>,
    /// Compass heading in degrees (0 north, 90 east).
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
    heading: f32,
    /// Pitch in degrees (negative looks down).
    #[arg(long, default_value_t = -10.0, allow_hyphen_values = true)]
    pitch: f32,
    /// Cells loaded around the camera's cell in each direction (2: the 5 x 5 cells of
    /// the game's uGridsToLoad=5, its default and the owner's INI value).
    #[arg(long, default_value_t = 2)]
    radius: i32,
    /// Time of day in hours (0-24).
    #[arg(long, default_value_t = 12.0)]
    hour: f32,
    /// Weather Editor ID or load-order FormID, kept for the whole run (default: the
    /// weather system's rolls, docs/spec-notes/world.md §6; the parity lab's frames pin it).
    #[arg(long)]
    weather: Option<String>,
    /// Seed of the weather rolls (default: a fixed one, so every run rolls the same).
    #[arg(long)]
    weather_seed: Option<u64>,
    /// A loaded game's weather: shown at the start and kept while the weather system
    /// lists it (not a pin, unlike --weather).
    #[arg(skip)]
    start_weather: Option<String>,
    /// A loaded game's GameDaysPassed, for the sky alone ([`start_days`]).
    #[arg(skip)]
    sky_day: Option<f32>,
    /// Override the weather's fog distances: "near,far" in game units.
    #[arg(long)]
    fog: Option<String>,
    /// Distant LOD (terrain chunks, object blocks) out to this distance in game units
    /// (the game's fBlockLoadDistance; default the owner's INI value) [hypothesis: lod.rs].
    #[arg(long, default_value_t = 125_000.0)]
    lod_distance: f32,
    /// Finest terrain LOD (level 4) within this distance (fBlockLoadDistanceLow).
    #[arg(long, default_value_t = 50_000.0)]
    lod_near_distance: f32,
    /// Coarser terrain LOD splits while the camera is nearer than this x its side
    /// (fSplitDistanceMult).
    #[arg(long, default_value_t = 1.5)]
    lod_split: f32,
    /// Distant trees drawn as billboards out to this distance (fTreeLoadDistance); 0: none.
    #[arg(long, default_value_t = 40_000.0)]
    tree_distance: f32,
    /// Do not draw distant LOD.
    #[arg(long)]
    no_lod: bool,
    /// Draw every room of an interior: no room and portal culling (rooms.rs).
    #[arg(long)]
    no_room_culling: bool,
    /// Occlusion planes hide nothing (rooms.rs, occlusion.rs).
    #[arg(long)]
    no_occlusion_planes: bool,
    /// Use the occlusion planes of exterior cells too (off by default: some poke out of
    /// their walls and would hide what is seen over them; occlusion.rs).
    #[arg(long)]
    occlusion_planes_outdoors: bool,
    /// Seconds a streamed cell takes to fade in or out against the distant LOD; 0 swaps
    /// at once (fade.rs).
    #[arg(long, default_value_t = 1.0)]
    cell_fade: f32,
    /// Do not generate distant terrain from the landscape records; draw only the game's
    /// LOD chunks (docs/design/generated-lod.md).
    #[arg(long)]
    no_lodgen: bool,
    /// Generated terrain replaces the game's level-4 LOD within this many 4-cell chunks.
    #[arg(long, default_value_t = 4)]
    lodgen_radius: i32,
    /// Generated terrain: diffuse texels per chunk side (power of two; normals get half).
    #[arg(long, default_value_t = 1024)]
    lodgen_res: usize,
    /// Generated terrain: mesh vertex step in landscape grid vertices (1, 2, 4, 8, 16 or 32).
    #[arg(long, default_value_t = 2)]
    lodgen_step: usize,
    /// Game days passed at the start of a new game, whole days (default: the GameDaysPassed
    /// global). A loaded save keeps its own game day; only the sky of a save made before
    /// saves held the day starts on this, until it follows the game.
    #[arg(long)]
    day: Option<f32>,
    /// Do not apply the image-space grade (the worldspace's image space and the weather's
    /// adapters, an interior cell's own image space), for comparisons.
    #[arg(long)]
    no_grade: bool,
    /// Put an image space modifier on the screen, by Editor ID or 0xFormID, for looking
    /// at one outside the scripts that play it: an animated one plays again and again, a
    /// second apart; with `@<seconds>` it is held at that time after its start
    /// (docs/spec-notes/image-modifiers.md §6). Repeatable.
    #[arg(long, value_name = "MODIFIER[@SECONDS]")]
    imod: Vec<String>,
    /// Play this image space modifier (Editor ID or 0xFormID) when the player dies, on
    /// top of the death camera's own blur (`death_cam.rs`).
    #[arg(long, value_name = "MODIFIER")]
    death_imod: Option<String>,
    /// The sun casts shadows; with --play it does unless --no-sun-shadows is given (owner
    /// 2026-10-26, once the first-person flicker was fixed; off from 2026-10-13 for their
    /// frame cost and that flicker). Runs without --play (screenshots, the parity lab)
    /// keep them off: the original game casts no sun shadows from the world
    /// (docs/spec-notes/world.md §6, "Sun path and shadows").
    #[arg(long)]
    sun_shadows: bool,
    /// With --play: the sun casts no shadows (the settings page turns them on and off too).
    #[arg(long, conflicts_with = "sun_shadows")]
    no_sun_shadows: bool,
    /// Sun shadows reach this far from the camera (game units), with --sun-shadows or when
    /// turned on from the settings page; 0 keeps them off.
    #[arg(long, default_value_t = sky::DEFAULT_SHADOW_DISTANCE)]
    shadow_distance: f32,
    /// Test aid: beyond 1,000 units of the camera a placed object casts no shadow when its
    /// bounding radius is under this part of its distance (its angular size in radians;
    /// shadow_thin.rs); 0, the default, lets everything cast as far as the shadows reach.
    #[arg(long, default_value_t = shadow_thin::DEFAULT_RATIO)]
    shadow_thin: f32,
    /// Samples per pixel for anti-aliasing: 1 (off), 2, 4 or 8. The cut-out edges of
    /// alpha-tested shapes (plants, fences, hair, grass) are smoothed with it too, like
    /// the game's transparency multisampling; with 1 they are tested per pixel.
    #[arg(long, default_value_t = 4)]
    msaa: u32,
    /// Do not grow grass on the landscape (docs/spec-notes/grass.md).
    #[arg(long)]
    no_grass: bool,
    /// Grass is drawn in full up to this distance in game units (the game's
    /// fGrassStartFadeDistance; default the owner's INI value).
    #[arg(long, default_value_t = 7000.0)]
    grass_distance: f32,
    /// Grass goes over this distance beyond --grass-distance (the game's fGrassFadeRange).
    #[arg(long, default_value_t = 1000.0)]
    grass_fade_range: f32,
    /// Multiplier on every grass type's density (1: as the records say).
    #[arg(long, default_value_t = 1.0)]
    grass_density: f32,
    /// Factors on the ambient light and on the sun's light for grass, "ambient,sun"
    /// (docs/spec-notes/grass.md §5).
    #[arg(long, default_value = "1.3,0.25")]
    grass_light: String,
    /// Edge of the squares that each get one candidate plant per grass type, in game units
    /// (the game's iMinGrassSize; larger: sparser grass).
    #[arg(long, default_value_t = 80.0)]
    grass_min_size: f32,
    /// World units per terrain texture repeat (the game's is 256 [observed;
    /// docs/spec-notes/world.md §3 "Texture repeat"]).
    #[arg(long, default_value_t = 256.0)]
    terrain_tile: f32,
    /// Do not draw models whose path contains any of these texts (diagnostics).
    #[arg(long, value_delimiter = ',')]
    hide: Vec<String>,
    /// Render, save a PNG here once the cells around the start are loaded, and exit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Window size "WxH" (also the screenshot size).
    #[arg(long, default_value = "1280x800")]
    size: String,
    /// The window: `windowed` (of --size) or `borderless` (borderless full screen on the
    /// display chosen in Display settings, the first display until one is chosen). With
    /// --play the settings page's Display group switches it too.
    #[arg(long, value_enum, default_value_t = window_mode::WindowChoice::Windowed)]
    window: window_mode::WindowChoice,
    /// The game's field-of-view setting in degrees (fDefaultFOV, default 75): the horizontal
    /// angle at 4:3; other aspects keep its vertical angle, so 16:9 is wider (75 → 91.3°
    /// horizontal) [hypothesis; docs/spec-notes/camera.md].
    #[arg(long, default_value_t = 75.0)]
    fov: f32,
    /// Added to the z of --pos (e.g. the eye height above a captured player position).
    #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
    eye_height: f32,
    /// With --screenshot: seconds to wait after the start's cells are loaded (shaders
    /// compile meanwhile; longer to catch something moving, e.g. an --npc-travel NPC).
    #[arg(long, default_value_t = 4.0, requires = "screenshot")]
    screenshot_delay: f32,
    /// With --screenshot: also write the lab's frame document (fnv.frame/1) here.
    #[arg(long, requires = "screenshot")]
    frame_json: Option<PathBuf>,
    /// Multiplier on every light's brightness (LIGH colour × fade; 1 = as stored).
    #[arg(long, default_value_t = 1.0)]
    light_scale: f32,
    /// Test aid: fly forward at this speed (game units per second).
    #[arg(long, default_value_t = 0.0)]
    auto_fly: f32,
    /// Test aid: game time passes at this many game hours per real second (0: it stands
    /// still). Logs a "time-lapse" sample of the sun and light colours at every whole hour.
    #[arg(long, default_value_t = 0.0)]
    time_scale: f32,
    /// Test aid with --time-scale: also save a screenshot in this folder at every whole hour.
    #[arg(long, requires = "time_scale")]
    timelapse: Option<PathBuf>,
    /// Test aid with --timelapse: game minutes between saved frames (60: one per hour; a
    /// few minutes give frames for a video).
    #[arg(long, default_value_t = 60.0, requires = "timelapse")]
    timelapse_step: f32,
    /// Test aid: exit with success after this many seconds (smoke tests).
    #[arg(long)]
    run_seconds: Option<f32>,
    /// Do not draw the text overlay (implied by --frame-json, and by --play unless
    /// --overlay is given).
    #[arg(long)]
    no_overlay: bool,
    /// Where the game's threads run on a processor whose L3 cache is split into domains
    /// of different sizes (dual-CCD X3D Ryzens): `auto` puts them on the largest L3,
    /// `off` leaves them on every processor (cpu_placement.rs). Wins over NVForever.ini's
    /// `[Settings] cpu-cache-placement` for this run; without either, `auto`.
    #[arg(long, value_enum)]
    cpu_cache_placement: Option<cpu_placement::Mode>,
    /// Show the frame rate counter, the performance overlay (perf_overlay/; a switch on the
    /// settings page too, its rows as the settings give them). Once any setting is saved, NVForever.ini's Frame rate counter
    /// switch wins over this flag, as for every switch.
    #[arg(long)]
    fps: bool,
    /// Latency reduction: AMD Anti-Lag through Vulkan (AMD GPUs only), the driver keeping the
    /// CPU from preparing frames ahead of the GPU (anti_lag.rs). Once any setting is saved,
    /// NVForever.ini's Latency reduction switch wins over this flag, as for every switch.
    #[arg(long)]
    latency_reduction: bool,
    /// FSR 3.1 frame generation: a frame made between each two real frames, from optical
    /// flow and the motion FSR gets (frame_gen.rs). Needs --fsr (any mode) and vsync; turns
    /// latency reduction on with it. Once any setting is saved, NVForever.ini's Frame
    /// generation switch wins over this flag, as for every switch.
    #[arg(long)]
    frame_generation: bool,
    /// With --play: draw the text overlay (the developer's position, loading counts and
    /// key help), which the game leaves off (owner, list 5 item 68); the settings page's
    /// Developer group turns it on and off too.
    #[arg(long, conflicts_with = "no_overlay")]
    overlay: bool,
    /// The look: `vanilla` (the picture the parity and picture checks compare) or
    /// `enhanced` (ambient occlusion, finer and longer sun shadows, bloom, sharpening,
    /// 16x anisotropic filtering, sun shafts, screen-space water reflections; each also on
    /// the settings page, which also offers FXAA, SMAA or temporal anti-aliasing instead of --msaa;
    /// docs/design/graphics-enhanced.md). Default: vanilla (`graphics::PLAY_DEFAULT`); test
    /// runs (--screenshot, --timelapse, --frame-json, --run-seconds) always default to it.
    #[arg(long, value_enum)]
    graphics: Option<graphics::Preset>,
    /// FSR 3.1 upscaling (frame generation: --frame-generation). Replaces other anti-aliasing while on.
    /// Saved Graphics settings take precedence in normal play, as for other switches.
    #[arg(long, value_enum, default_value = "off")]
    fsr: graphics::FsrMode,
    /// Instead of --graphics: exactly these improvements on, by the settings page's keys
    /// (ambient-occlusion, fine-shadows, bloom, sharpen, anisotropic, light-shafts,
    /// water-reflections) and choices (fxaa, smaa or taa instead of --msaa;
    /// shadow-filter-hardware or shadow-filter-temporal), for comparing them one at a time.
    #[arg(long, value_delimiter = ',', conflicts_with = "graphics")]
    graphics_only: Option<Vec<String>>,
    /// Test aid: log the mean and the slowest frame time every this many seconds (real
    /// time), for measuring what a graphics switch costs; the picture is then not held to
    /// the display's refresh (no vsync).
    #[arg(long)]
    log_frame_times: Option<f32>,
    /// Test aid with --log-frame-times: after each logged period switch to the next of
    /// these sets of improvements, in a loop, starting with the first: `;`-separated,
    /// each `vanilla`, `enhanced` or keys as for --graphics-only; `no-sun-shadows` among
    /// a set's keys switches the sun's shadows off for it (e.g. `vanilla;no-sun-shadows`);
    /// `shadow-distance=<units>`, `cascades=<n>` and `shadow-thin=<ratio>` change them.
    #[arg(long, requires = "log_frame_times", allow_hyphen_values = true)]
    graphics_cycle: Option<String>,
    /// Play: the camera is the player, walking with collision, activating with E
    /// (Layer 4, nvf-play) instead of the free-fly camera.
    #[arg(long)]
    play: bool,
    /// The free-camera explorer (a developer's view, no player) even when no other
    /// argument chooses a world; with no such argument the game opens on its main menu.
    #[arg(long, conflicts_with = "play")]
    explore: bool,
    /// Do not draw NPCs (workstream 5, docs/design/static-actors.md).
    #[arg(long)]
    no_actors: bool,
    /// Test aid: open every door that opens in place as soon as it is drawn (doors.rs).
    #[arg(long)]
    open_doors: bool,
    /// Draw NPCs with the race's base face (no FaceGen morphs or face textures).
    #[arg(long)]
    no_facegen: bool,
    /// Tint NPCs' skin and shape their faces from their own FaceGen values alone, without
    /// adding their race's, for comparisons (docs/spec-notes/actors.md §4.2, §4.3).
    #[arg(long)]
    no_race_facegen: bool,
    /// Test aid: draw the actors that are not enabled at the start of a game (they are
    /// spawned hidden until a script enables them; docs/spec-notes/actors.md §11), e.g.
    /// to look at Joe Cobb in the saloon.
    #[arg(long)]
    show_disabled_actors: bool,
    /// How NPCs are posed: the skeleton's idle clip in a loop (the default), or the
    /// frozen bind pose (docs/spec-notes/actors.md §7).
    #[arg(long, value_enum, default_value_t = actors::ActorPose::Idle)]
    actor_pose: actors::ActorPose,
    /// Test aid with --actor-pose idle: hold every NPC at this clip time (seconds); with
    /// --actor-walk-test, at this time of the walk script (run from its start, repeatable).
    #[arg(long)]
    actor_pose_time: Option<f32>,
    /// Test aid: NPCs walk a fixed script along their forward axis at this speed (units
    /// per second): out, stop, turn, back, and a backward leg (docs/spec-notes/actors.md
    /// §8.4). Needs --actor-pose idle.
    #[arg(long)]
    actor_walk_test: Option<f32>,
    /// How NPCs' weapons are shown: holstered (the default), drawn, or not at all
    /// (docs/spec-notes/actors.md §9). Needs --actor-pose idle.
    #[arg(long, value_enum, default_value_t = weapons::WeaponMode::Holstered)]
    actor_weapons: weapons::WeaponMode,
    /// Test aid: armed NPCs draw and holster their weapon, each state held this many
    /// seconds (docs/spec-notes/actors.md §9.4).
    #[arg(long)]
    actor_weapon_test: Option<f32>,
    /// Test aid: standing NPCs within this distance of the camera (game units) turn on
    /// the spot to face it, playing their turn clips (docs/spec-notes/actors.md §8.9).
    /// Needs --actor-pose idle; not for --npc-travel travellers.
    #[arg(long, conflicts_with = "actor_walk_test")]
    actor_turn_test: Option<f32>,
    /// With --actor-walk-test: only NPCs within this distance of the start position
    /// (game units; 0 = all).
    #[arg(long, default_value_t = 0.0, requires = "actor_walk_test")]
    actor_walk_test_radius: f32,
    /// Test aid with --play: walk forward at this speed (game units per second).
    #[arg(long, default_value_t = 0.0, requires = "play")]
    auto_walk: f32,
    /// An NPC walks to a target over the navmesh: "ACTOR=TARGET", each a placed
    /// reference (Editor ID or FormID) or the target as "x,y,z" (repeatable;
    /// docs/design/navmesh.md §5); "ACTOR@x,y,z=TARGET" sets the actor off from x,y,z
    /// in its own space.
    #[arg(long)]
    npc_travel: Vec<String>,
    /// The --npc-travel paths may go over the navmesh's ledge links: the NPC drops off
    /// or jumps up the ledge (docs/spec-notes/actor-jumps.md).
    #[arg(long, requires = "npc_travel")]
    npc_ledges: bool,
    /// Walking speed of --npc-travel NPCs, game units per second.
    #[arg(long, default_value_t = nav::WALK_SPEED)]
    npc_speed: f32,
    /// An NPC walks to a piece of furniture and uses it: "ACTOR=FURNITURE[:SECONDS]",
    /// the actor a placed reference (Editor ID or FormID), the furniture a reference
    /// FormID; with SECONDS it gets up again after that long on it (repeatable;
    /// docs/spec-notes/furniture.md).
    #[arg(long)]
    npc_sit: Vec<String>,
    /// The free camera follows the first --npc-travel or --npc-sit NPC, else the first
    /// --npc-packages NPC (through doors too).
    #[arg(long, conflicts_with = "play")]
    follow_npc: bool,
    /// With --play: the sky keeps the hour of --hour instead of following the game's
    /// clock (GameHour, which runs at the TimeScale global, 30).
    #[arg(long, requires = "play")]
    still_sky: bool,
    /// NPCs that live by their AI packages: placed actors (Editor ID or FormID, comma
    /// separated or repeated), or `all` for every actor and creature of the cells
    /// loaded around the camera, choose their package by the hour and walk to where it
    /// sends them (docs/design/actor-behaviour.md §5.4). Their clock starts at --hour
    /// and runs at --time-scale, or at --npc-timescale when the sky stands still. With
    /// --play the choice uses the game's own state (quests, scripts), and `all` is the
    /// default (see --no-npc-packages).
    #[arg(long, value_delimiter = ',')]
    npc_packages: Vec<String>,
    /// With --play: NPCs do not live by their AI packages; they stand where they are
    /// placed, as without --play.
    #[arg(long, requires = "play", conflicts_with = "npc_packages")]
    no_npc_packages: bool,
    /// Game seconds per second for --npc-packages when --time-scale is 0 (the game's
    /// TimeScale is 30).
    #[arg(long, default_value_t = 30.0, requires = "npc_packages")]
    npc_timescale: f32,
    /// NPCs and creatures may stand and walk inside each other (by default two that
    /// overlap are moved apart; fnv_ai::apart).
    #[arg(long)]
    actors_pass_through: bool,
    /// Draw the navmesh near the camera and the travellers' paths.
    #[arg(long)]
    show_navmesh: bool,
    /// With --play: draw the developer markers of shots: a cross at each impact and a
    /// line from the muzzle along each hitscan shot (settings page: Developer).
    #[arg(long)]
    hit_markers: bool,
    /// With --play: god mode, the player takes no damage from hits and cannot be killed
    /// by them (settings page: Developer).
    #[arg(long)]
    god: bool,
    /// With --play: start a new game: in Doc Mitchell's house at the opening's start
    /// marker, with the opening quest VCG00 started (docs/design/layer5-play.md); --cell,
    /// --pos and --heading are ignored.
    #[arg(long, requires = "play")]
    new_game: bool,
    /// With --play: load a save (a name in NVForever's saves folder, or a path to a
    /// .nvfsave): start at its place and put its state back (nvf_play::save,
    /// docs/design/save-load.md); --cell, --pos, --heading, --pitch, --hour, --day, --weather
    /// and --world are taken from the save.
    #[arg(long, requires = "play", conflicts_with = "new_game")]
    load: Option<String>,
    /// With --play: open on the main menu (nvf_play::main_menu): Continue, New Game, Load
    /// Game, Settings, Quit over the plugins' loading screens. The game behind it is a new
    /// game, held until New Game.
    #[arg(long, requires = "play", conflicts_with_all = ["new_game", "load"])]
    menu: bool,
    /// With --menu: why the run before could not restore the save it was given, for the
    /// message box (a run that fails so starts this one, handoff.rs).
    #[arg(long, requires = "menu")]
    load_refused: Option<String>,
    /// Open the window with its outer top-left corner here on the desktop, "X,Y" in
    /// pixels (a new run taking over a load keeps the old window's place, handoff.rs).
    #[arg(long, value_parser = |s: &str| handoff::parse_at(s).ok_or("expected X,Y"))]
    window_at: Option<IVec2>,
    /// Maximise the window once it is shown.
    #[arg(long)]
    window_maximized: bool,
    /// With --play: a run taking over from one that loads a save or goes back to the main
    /// menu (handoff.rs): the window stays hidden until its loading screen is up, then
    /// this file is written for the old run to end.
    #[arg(long, requires = "play", hide = true)]
    handoff: Option<PathBuf>,
    /// With --play: close message boxes that ask nothing (an OK button only, such as the
    /// add-on packs' start-up boxes) as soon as a script shows them; boxes with a choice
    /// stay open. For testing; the box's text is still logged.
    #[arg(long, requires = "play")]
    no_messages: bool,
    /// With --play: do not play dialogue voice (lines still last as long as their sound;
    /// the log still names each file).
    #[arg(long)]
    no_voice: bool,
    /// With --play: do not play sound effects (weapons, impacts, explosions, placed
    /// sounds and the cells' background loops).
    #[arg(long)]
    no_sound: bool,
    /// With --play: play world sounds dry, without the acoustic space's reverb (the
    /// settings page's Sound tab switch `reverb`, kept in the INI, wins).
    #[arg(long)]
    no_reverb: bool,
    /// With --play: dialogue voice volume, 1 = as recorded.
    #[arg(long, default_value_t = 1.0)]
    voice_volume: f32,
    /// With --play: do not play music (the log still names what would play).
    #[arg(long)]
    no_music: bool,
    /// With --play: music volume, 1 = as recorded less the records' decibels.
    #[arg(long, default_value_t = 1.0)]
    music_volume: f32,
    /// With --play: do not move a speaker's face from the line's `.lip` file
    /// (docs/spec-notes/lipsync.md).
    #[arg(long)]
    no_lip: bool,
    /// With --play: no blinking or eye movement on faces between lines
    /// (docs/spec-notes/lipsync.md §5.3).
    #[arg(long)]
    no_face_idle: bool,
    /// With --play: what an open menu holds (the dialogue screen, inventory, container,
    /// message boxes and the menus scripts open). "living": game logic only (scripts'
    /// game time, the game clock, physics, travelling NPCs, the player's movement), while
    /// animations, wind, water and sky go on. "original": the picture stands still too.
    /// The Esc pause menu always holds everything. --dialogue-pause is the old name.
    /// docs/design/dialogue.md, Pausing.
    #[arg(long, alias = "dialogue-pause", default_value = "living", value_parser = ["living", "original"], requires = "play")]
    menu_pause: String,
    /// In first person draw the player's own body too (seen looking down), not the arms
    /// alone as the original does; a switch of the settings page (player_body.rs).
    #[arg(long, requires = "play")]
    first_person_body: bool,
    /// Keep the first-person arms and weapon still while turning and moving: no sway or
    /// bob (view_sway.rs); a switch of the settings page.
    #[arg(long, requires = "play")]
    no_weapon_sway: bool,
    /// No head bob in first person: the camera stays still while walking and running
    /// (head_bob.rs); a switch of the settings page.
    #[arg(long, requires = "play")]
    no_head_bob: bool,
    /// Keep the first-person weapon up against walls and friendly actors close ahead
    /// (weapon_lower.rs); a switch of the settings page.
    #[arg(long, requires = "play")]
    no_weapon_lower: bool,
    /// Aim down the sights as the original does: the gun is not lined up with the eye
    /// (iron_sights.rs); a switch of the settings page.
    #[arg(long, requires = "play")]
    no_true_iron_sights: bool,
    /// 3D scopes: a scoped gun shows the zoomed world in its scope's lens instead of the
    /// full-screen picture (scope_view.rs, a test version); a switch of the settings page.
    #[arg(long, requires = "play")]
    scope_3d: bool,
    /// Do not turn actors' heads to the player and to NPCs near them (head_look.rs,
    /// docs/spec-notes/actors.md §12).
    #[arg(long)]
    no_head_tracking: bool,
    /// Do not put actors' feet on the ground under them (slopes, steps): the legs play
    /// the animation as it is (foot_ik.rs, docs/spec-notes/actors.md §13).
    #[arg(long)]
    no_foot_ik: bool,
    /// Test aid without --play: the free camera stands in for the player, so actors near
    /// it look at it.
    #[arg(long, conflicts_with_all = ["play", "no_head_tracking"])]
    head_track_camera: bool,
    /// Test aid: every actor near the camera is told to look at this game position
    /// "x,y,z", the way a conversation or a script forces a head's target.
    #[arg(long, allow_hyphen_values = true, conflicts_with = "no_head_tracking")]
    head_track_point: Option<String>,
    /// With --play: in a conversation, do not turn the actor to the player nor the
    /// player's view to the actor (docs/design/dialogue-play.md §8).
    #[arg(long)]
    no_dialogue_look: bool,
    /// With --play: in a conversation, bring the view back onto the actor each time the
    /// choices appear. Off by default (owner decision 2026-10-10): only the opening turn
    /// happens and the view is the player's own for the rest of the conversation.
    #[arg(long)]
    dialogue_recentre: bool,
    /// Accepted for older command lines; not coming back is the default now.
    #[arg(long, hide = true)]
    no_dialogue_recentre: bool,
    /// With --play: in a conversation, do not narrow the view onto the speaker nor blur
    /// what is behind them (docs/design/dialogue-play.md §8.8).
    #[arg(long)]
    no_dialogue_zoom: bool,
}

impl Args {
    /// The game's Data folder, settled at the start of `main` (setup.rs).
    fn data(&self) -> &std::path::Path {
        self.data.as_deref().expect("the game folder is settled before anything reads it")
    }
}

#[derive(Component)]
struct Overlay;

fn parse_vec<const N: usize>(s: &str) -> Option<[f32; N]> {
    let v: Vec<f32> = s.split(',').map(|x| x.trim().parse().ok()).collect::<Option<_>>()?;
    v.try_into().ok()
}

/// The distant LOD distances from the command line (lod.rs).
fn lod_settings(args: &Args) -> lod::LodSettings {
    lod::LodSettings {
        far: args.lod_distance.max(0.0),
        near: args.lod_near_distance.max(0.0),
        split: args.lod_split.max(0.0),
        trees: args.tree_distance.max(0.0),
    }
}

/// The generated-terrain job: parameters, cache folder (NVForever's home, never the game's
/// folder) and shared data (docs/design/generated-lod.md).
fn lodgen_job(args: &Args, world: &WorldData, game: &GameData) -> Result<lodgen::GenJob, String> {
    let (res, step) = (args.lodgen_res, args.lodgen_step);
    if !(16..=4096).contains(&res) || !res.is_power_of_two() {
        return Err(format!("--lodgen-res {res}: expected a power of two from 16 to 4096"));
    }
    if step == 0 || 32 % step != 0 {
        return Err(format!("--lodgen-step {step}: expected 1, 2, 4, 8, 16 or 32"));
    }
    let params = lodgen::GenParams { res, step, tile: args.terrain_tile };
    let world_name = world.index.worldspaces[&world.world].edid.clone().unwrap_or_default();
    let cache = fnv_install::EngineHome::user().map(|home| {
        let (key, manifest) = lodgen::cache_key(&params, &world_name, args.data(), &args.plugins);
        let dir = home.cache().join("lod").join(world_name.to_ascii_lowercase()).join(key);
        info!("generated LOD cache: {}", dir.display());
        lodgen::GenCache { dir, manifest, game: fnv_install::GameInstall::open(args.data()).ok() }
    });
    if cache.is_none() {
        warn!("no NVForever home folder: generated LOD is not cached");
    }
    Ok(lodgen::GenJob {
        lo: world.lo,
        schema: world.schema,
        index: world.index,
        world: world.world,
        data: game.clone(),
        params,
        cache,
        mips: Default::default(),
    })
}

/// The archives that stop the start ([`fnv_vfs::Vfs::broken_archives`]): the run log's
/// one-line reason and the message box's text. None when every archive reads.
fn damaged_archives(broken: &[(String, String)]) -> Option<(String, String)> {
    if broken.is_empty() {
        return None;
    }
    const ADVICE: &str = "Reinstall or remove the mod the archive came with (for the game's own archives, verify the game files in Steam), then start NVForever again.";
    let list: Vec<String> = broken.iter().map(|(name, why)| format!("{name}: {why}")).collect();
    let why = format!("damaged archive, the game will not start: {}. {ADVICE}", list.join("; "));
    let (what, are) = if broken.len() == 1 { ("an archive", "is") } else { ("archives", "are") };
    let items: Vec<String> = list.iter().map(|l| format!("\u{2022} {l}")).collect();
    let text = format!("NVForever cannot start: {what} it would load {are} damaged.\n\n{}\n\n{ADVICE}", items.join("\n"));
    Some((why, text))
}

/// Which archives the game reads, lowest precedence first (mods' archives follow their
/// plugins; loose files override them all).
fn report_archives(data: Res<GameData>, world: Res<WorldData>) {
    info!("archives: {}", data.vfs.archives().collect::<Vec<_>>().join(", "));
    // Read before the log was up: said now. A broken mod's files are skipped, not fatal.
    for (name, why) in data.vfs.skipped() {
        warn!("skipped {name}: cannot be read: {why}");
    }
    for d in &world.dropped_plugins {
        warn!("{d}");
    }
}

/// Arguments that choose a world or a mode. A command line with none of them is the game
/// on its main menu (`with_default_mode`); with any of them it means what it did before.
const MODE_ARGS: [&str; 23] = [
    "--play", "--explore", "--menu", "--new-game", "--load", "--handoff", "--cell", "--pos", "--world", "--screenshot", "--frame-json",
    "--timelapse", "--time-scale", "--auto-fly", "--npc-travel", "--npc-sit", "--npc-packages", "--follow-npc", "--head-track-camera",
    "--actor-walk-test", "--actor-turn-test", "--actor-weapon-test", "--show-navmesh",
];

/// The command line (program name first) with `--play --menu` added when none of its
/// arguments chooses a world or a mode (`MODE_ARGS`, as `--flag value` or `--flag=value`).
fn with_default_mode(mut args: Vec<String>) -> Vec<String> {
    let chosen = args.iter().skip(1).any(|a| MODE_ARGS.contains(&a.split('=').next().unwrap_or(a)));
    if !chosen {
        args.extend(["--play".to_string(), "--menu".to_string()]);
    }
    args
}

/// `--esm-schema` and `--nif-schema` from beside the program (the release archive:
/// `schemas/fnv`, `schemas/nif` next to the executable) when the command line does not
/// name them and they are there; the build's checkout otherwise (the options' defaults).
fn with_schemas_beside(mut args: Vec<String>, dir: &std::path::Path) -> Vec<String> {
    for (flag, rel) in [("--esm-schema", "schemas/fnv"), ("--nif-schema", "schemas/nif")] {
        let given = args.iter().skip(1).any(|a| a.split('=').next() == Some(flag));
        let path = dir.join(rel);
        if !given && path.is_dir() {
            args.extend([flag.to_string(), path.display().to_string()]);
        }
    }
    args
}

/// The command line this run acts on (`with_default_mode`, `with_schemas_beside`); a run
/// taking over a load starts from it (handoff.rs).
static COMMAND_LINE: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

/// This run's arguments after the program name, as acted on (`COMMAND_LINE`).
fn command_line() -> Vec<String> {
    COMMAND_LINE.get().map_or_else(|| std::env::args().skip(1).collect(), |a| a[1..].to_vec())
}

/// Bevy's task pools. Test aid `NVF_PLAY_TEST_ASYNC_THREADS=<n>` sets the async compute
/// pool's size, which is 25 % of the cores and 1 to 4 threads otherwise [documented:
/// bevy_app 0.20.1 `task_pool_plugin.rs`], to compare with a smaller machine's. Pipelines
/// compile there; the game's file reads have a pool of their own, with an aid of its own
/// (`nvf_assets::read_pool`).
/// Bevy's task pools: its defaults, with a thread budget only when the game was put on
/// the largest L3 domain (`placed`, cpu_placement.rs); `None` leaves the defaults as they are.
fn task_pools(placed: Option<usize>) -> TaskPoolPlugin {
    let mut task_pool_options = bevy::app::TaskPoolOptions::default();
    if let Some(n) = placed {
        task_pool_options.max_total_threads = n;
    }
    if let Some(n) = std::env::var("NVF_PLAY_TEST_ASYNC_THREADS").ok().and_then(|v| v.parse::<usize>().ok()) {
        task_pool_options.async_compute.min_threads = n;
        task_pool_options.async_compute.max_threads = n;
        info!("test aid NVF_PLAY_TEST_ASYNC_THREADS: {n}");
    }
    TaskPoolPlugin { task_pool_options }
}

fn main() -> AppExit {
    let mut line = with_default_mode(std::env::args().collect());
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(std::path::Path::to_path_buf)) {
        line = with_schemas_beside(line, &dir);
        let functions = dir.join("schemas").join("script").join("functions.json");
        if std::env::var_os("NVF_PLAY_SCRIPT_FUNCTIONS").is_none() && functions.is_file() {
            // Before any thread starts; read by the script engines (nvf_play::script, residents.rs).
            std::env::set_var("NVF_PLAY_SCRIPT_FUNCTIONS", &functions);
        }
    }
    let mut args = Args::parse_from(&line);
    let _ = COMMAND_LINE.set(line);
    // Taking over a load: tell the old run at once that this run takes part (#1903).
    let handoff_marked = args.handoff.as_deref().map(handoff::mark_started);
    // On a processor with L3 domains of different sizes, the game's threads on the largest
    // (cpu_placement.rs): before any thread pool or other thread starts, so all of them
    // start there. Logged once the log is up.
    let cpu_placed = cpu_placement::place(args.cpu_cache_placement);
    if args.explore {
        eprintln!("explore: the free-camera explorer (--explore)");
    }
    let developer_data = args.data.is_some();
    let code_page_named = match setup::text_code_page() {
        Ok((c, named)) => {
            if c != fnv_util::text::CodePage::default() {
                eprintln!("setup: game text read as {c}");
            }
            fnv_util::text::set_code_page(c);
            named
        }
        Err(e) => {
            eprintln!("error: {e}");
            return AppExit::error();
        }
    };
    // The languages of NVForever's own words (#1443): beside the program, in the home.
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(std::path::Path::to_path_buf));
    let home = fnv_install::EngineHome::default_root();
    for problem in fnv_util::words::load(&fnv_util::words::dirs(exe_dir.as_deref(), home.as_deref())) {
        eprintln!("warning: language file skipped: {problem}");
    }
    // The game's folder: `--data`, NVForever.ini or asked for; its files checked (setup.rs).
    let install = match setup::game_install(args.data.as_deref()) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("error: {e}");
            return AppExit::error();
        }
    };
    args.data = Some(install.data.clone());
    setup::install_fonts(&install);
    // Every `fnv_install::guard_write` from here on refuses writes into this install,
    // also where the writer has none at hand (saves, test aids; #994).
    fnv_install::remember_game(&install);
    let default_plugins = args.plugins.iter().map(String::as_str).eq(MASTERS);
    // A player's start (no --data, no --plugins): the home's own plugins.txt (setup.rs).
    // Developer and smoke runs keep the fixed masters below, whatever the list says.
    if default_plugins && !developer_data {
        match setup::plugin_list(&install) {
            Ok(p) => args.plugins = p,
            Err(e) => {
                eprintln!("error: {e}");
                return AppExit::error();
            }
        }
    } else if default_plugins {
        // Without --plugins: the DLC masters that are there (all of them are optional).
        let addons = install.addon_masters();
        args.plugins.retain(|p| p == MASTERS[0] || addons.contains(&p.as_str()));
        if addons.len() < MASTERS.len() - 1 {
            eprintln!("setup: playing without the add-ons not installed; masters: {}", args.plugins.join(", "));
        }
    }
    // The run log and crash reports in NVForever's home (crash.rs).
    crash::install(args.data(), args.handoff.is_some());
    // Player reports: the player's choice, asked once on a player's start (reports/).
    reports::decide(developer_data, args.handoff.is_some());
    let fail = |e: String| {
        eprintln!("error: {e}");
        crash::early_exit(&e);
        AppExit::error()
    };
    // The plugins, opened: a broken mod's plugin (and any that need it) is left out with
    // a warning, and the rest of the start uses the list without it (world.rs).
    let opened = world::open_plugins(args.data(), &args.plugins);
    args.plugins = opened.plugins.iter().map(|p| p.name.clone()).collect();
    // No setting names the code page: the plugins' own text says it (#1443).
    if !code_page_named {
        setup::guess_code_page(&opened.plugins);
    }
    // NVForever's own words in the language of the game's text, when the code page names
    // one and a file of it was read (#1948); a `language=` in NVForever.ini still wins.
    setup::follow_text_language();
    // `--load`: the save's place decides where the world starts (its worldspace before
    // the world is opened); its body waits for the scripts (nvf_play::save). It is read
    // in full now, remapped to the plugins opened and checked as the restore will
    // (nvf_play::save::read_for_load): a save that cannot be loaded opens the main menu
    // instead, and the player is told why once the window is up [owner 2026-11-01, #1357]
    // (`handoff::LoadFailed`). So does a run a failed restore started (`--load-refused`).
    let mut load_failed = args.load_refused.clone();
    if let Some(why) = &load_failed {
        eprintln!("load: {why}; opening the main menu");
    }
    let pending = match args.load.clone() {
        Some(name) => match start_from_save(&name, &opened, &mut args) {
            Ok(p) => Some(p),
            Err(why) => {
                eprintln!("load: {why}; opening the main menu");
                load_failed = Some(why);
                args.load = None;
                args.menu = true;
                None
            }
        },
        None => None,
    };
    // NVForever.ini's `[Archive] SArchiveList`, in place of the shipped base list (#1142).
    if let Some((list, from)) = nvf_play::game_start::archive_list_for_run() {
        if fnv_vfs::set_archive_list(list.clone()).is_ok() {
            if std::env::var(fnv_vfs::ARCHIVE_ORDER_ENV).is_ok_and(|v| !v.trim().is_empty()) {
                eprintln!("vfs: SArchiveList from {from} not used: {} sets the whole order", fnv_vfs::ARCHIVE_ORDER_ENV);
            } else {
                eprintln!("vfs: SArchiveList from {from}: {}", list.join(", "));
                // A name not in Data (a typo) is left out of the order: say so.
                let present: Vec<String> = std::fs::read_dir(args.data())
                    .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_ascii_lowercase()).collect())
                    .unwrap_or_default();
                for a in list.iter().filter(|a| !present.contains(&a.to_ascii_lowercase())) {
                    eprintln!("vfs: warning: SArchiveList names {a}, which is not in {}: left out", args.data().display());
                }
            }
        }
    }
    let game = match GameData::open_for_plugins(args.data(), &args.plugins, &args.nif_schema) {
        Ok(g) => g,
        Err(e) => return fail(e),
    };
    // A damaged archive stops the start (owner 2026-10-25): the player is told which one
    // and why, and no save or world loads with its assets missing. A file inside a
    // readable archive that fails later is only that file's error.
    if let Some((why, text)) = damaged_archives(game.vfs.broken_archives()) {
        crash::tell_player_at_start(&text, args.handoff.is_some());
        return fail(why);
    }
    let t0 = std::time::Instant::now();
    let world = match WorldData::open(&args.esm_schema, opened, &args.world) {
        Ok(w) => w,
        Err(e) => return fail(e),
    };
    info!("world index built in {:.2}s", t0.elapsed().as_secs_f64());

    // The main menu stands over a new game, held until New Game is chosen.
    if args.menu {
        args.new_game = true;
    }
    // A new game starts at the opening's marker (nvf-play).
    if args.new_game {
        match nvf_play::script::new_game_start(world.lo, world.schema, world.index) {
            Ok(s) => {
                info!("new game: interior {:08X}, feet {:?}, heading {:.0}", s.interior, s.feet, s.heading_deg);
                args.cell = format!("0x{:08X}", s.interior);
                args.pos = Some(format!("{},{},{}", s.feet[0], s.feet[1], s.feet[2] + nvf_play::player::EYE_HEIGHT));
                args.heading = s.heading_deg;
                args.eye_height = 0.0;
            }
            Err(e) => return fail(format!("--new-game: {e}")),
        }
    }
    // Interior mode when --cell names an interior cell.
    let interior = world.index.cell_by_name(&args.cell).filter(|c| c.world.is_none()).map(|c| c.form_id);
    // Start position (game space).
    let start = match args.pos.as_deref() {
        Some(p) => match parse_vec::<3>(p) {
            Some(v) => Vec3::from_array(v) + Vec3::Z * args.eye_height,
            None => return fail(format!("--pos {p}: expected x,y,z")),
        },
        None if interior.is_some() => {
            // The middle of the cell's references, a little above.
            let ps: Vec<Vec3> = world.index.cells[&interior.unwrap()]
                .refs
                .iter()
                .filter_map(|&r| fnv_world::placed::placement(world.lo, world.schema, r))
                .map(|p| Vec3::from_array(p.position))
                .collect();
            let n = ps.len().max(1) as f32;
            ps.iter().copied().sum::<Vec3>() / n + Vec3::Z * 150.0
        }
        None => {
            let grid = match parse_vec::<2>(&args.cell) {
                Some([x, y]) => (x as i32, y as i32),
                None => match world.index.cell_by_name(&args.cell).and_then(|c| c.grid) {
                    Some(g) => g,
                    None => return fail(format!("no exterior cell {}", args.cell)),
                },
            };
            let ground = world
                .grid_cell(grid)
                .and_then(|c| c.land)
                .and_then(|l| load_land(world.lo, world.schema, l, LandParts::HEIGHTS))
                .and_then(|l| l.heights)
                .map_or(0.0, |h| h[(SIDE / 2) * SIDE + SIDE / 2]);
            let s = fnv_world::CELL_SIZE;
            Vec3::new((grid.0 as f32 + 0.5) * s, (grid.1 as f32 + 0.5) * s, ground + 400.0)
        }
    };
    let seed = args.weather_seed.unwrap_or(sky::DEFAULT_WEATHER_SEED);
    // A start worldspace without a usable climate (a broken mod) takes another's sky
    // (sky.rs `fallback`); travel keeps the previous sky instead (worldspace.rs).
    let mut sky_state = match sky::SkyState::new(&world, args.weather.as_deref(), args.start_weather.as_deref(), args.hour, seed) {
        Ok(s) => s,
        Err(e) => match sky::SkyState::fallback(&world, args.weather.as_deref(), args.start_weather.as_deref(), args.hour, seed) {
            Ok(s) => {
                // The log is not up yet: straight to the output.
                eprintln!("warning: sky of the start worldspace: {e}; another worldspace's climate instead");
                s
            }
            Err(_) => return fail(e),
        },
    };
    if let Some(saved) = pending.as_ref().and_then(|p| p.body.engine.extra.get(sky::SAVE_HOOK.key)) {
        sky_state.restore_saved(saved);
    }
    sky_state.shadows = (args.sun_shadows || (args.play && !args.no_sun_shadows)) && args.shadow_distance > 0.0;
    if args.shadow_distance > 0.0 {
        sky_state.shadow_distance = args.shadow_distance;
    }
    sky_state.grade = !args.no_grade;
    let mut screen_modifiers = imagespace::ScreenModifiers::default();
    for arg in &args.imod {
        match imagespace::parse_forced(&world, arg) {
            Ok(f) => screen_modifiers.forced.push(f),
            Err(e) => return fail(e),
        }
    }
    let death_imod = match args.death_imod.as_deref().map(|m| fnv_world::image_modifier::find_image_modifier(world.lo, world.schema, m).ok_or(m)) {
        Some(Err(m)) => return fail(format!("--death-imod {m}: no image space modifier {m}")),
        Some(Ok(f)) => Some(f),
        None => None,
    };
    sky_state.time_scale = args.time_scale.max(0.0);
    let (start_day, sky_day) = start_days(args.day, args.sky_day, sky::find_global(&world, "GameDaysPassed"));
    sky_state.days_passed = sky_day;
    if let Some(dir) = &args.timelapse {
        if let Err(e) = fnv_install::guard_write(dir, None) {
            return fail(format!("--timelapse {}: {e}", dir.display()));
        }
    }
    for (flag, path) in [("--screenshot", &args.screenshot), ("--frame-json", &args.frame_json)] {
        if let Some(Err(e)) = path.as_deref().map(|p| fnv_install::guard_write(p, None)) {
            return fail(format!("{flag}: {e}"));
        }
    }
    if let Some(dir) = &args.timelapse {
        if let Err(e) = std::fs::create_dir_all(dir) {
            return fail(format!("--timelapse {}: {e}", dir.display()));
        }
    }
    if let Some(id) = interior {
        sky_state.interior = fnv_world::lighting::cell_lighting(world.lo, world.schema, id);
        sky_state.interior_grade = sky::SkyState::cell_grade(world.lo, world.schema, id);
    }
    if let Some(f) = args.fog.as_deref() {
        match parse_vec::<2>(f) {
            Some([n, fa]) => sky_state.fog_override = Some((n, fa)),
            None => return fail(format!("--fog {f}: expected near,far")),
        }
    }
    let msaa = match args.msaa {
        1 => Msaa::Off,
        2 => Msaa::Sample2,
        4 => Msaa::Sample4,
        8 => Msaa::Sample8,
        n => return fail(format!("--msaa {n}: expected 1, 2, 4 or 8")),
    };
    let Some(grass_light) = parse_vec::<2>(&args.grass_light) else {
        return fail(format!("--grass-light {}: expected ambient,sun", args.grass_light));
    };
    let Some([w, h]) = args.size.split_once('x').and_then(|(w, h)| Some([w.parse::<u32>().ok()?, h.parse::<u32>().ok()?])) else {
        return fail(format!("--size {}: expected WxH", args.size));
    };

    if let Some(dir) = args.screenshot.as_deref().and_then(|p| p.parent()).filter(|d| !d.as_os_str().is_empty()) {
        if let Err(e) = std::fs::create_dir_all(dir) {
            return fail(format!("{}: {e}", dir.display()));
        }
    }
    let mut play = args.play.then_some(nvf_play::PlayData {
        lo: world.lo,
        schema: world.schema,
        index: world.index,
        world: world.world,
        interior,
        eye: start.to_array(),
        heading_deg: args.heading,
        pitch_deg: args.pitch,
        auto_walk: args.auto_walk,
        start_at_cell_spot: args.pos.is_none(),
        packages: false,
    });
    // Also for an interior start: LOD stays off inside and is switched on at the door out
    // (world::travel), with generated terrain for the start worldspace. Also with
    // --no-lod: the settings page can switch distant LOD on.
    let lodgen = if args.no_lodgen {
        None
    } else {
        match lodgen_job(&args, &world, &game) {
            Ok(job) => Some(lod::GenState::new(job, args.lodgen_radius.max(0))),
            Err(e) => return fail(e),
        }
    };
    let lod_origin = lod::grid_origin(&game, &world);
    let actor_world = (!args.no_actors).then(|| {
        let t0 = std::time::Instant::now();
        let mut a = actors::ActorWorld::build(world.lo, world.schema);
        a.facegen_enabled = !args.no_facegen;
        a.race_facegen = !args.no_race_facegen;
        a.show_disabled = args.show_disabled_actors;
        a.pose = args.actor_pose;
        a.pose_time = args.actor_pose_time;
        a.walk_test = args.actor_walk_test.map(|speed| actors::WalkTestArgs { speed, radius: args.actor_walk_test_radius, center: start });
        a.weapon_mode = args.actor_weapons;
        a.weapon_test = args.actor_weapon_test;
        if a.walk_test.is_some() && a.pose == actors::ActorPose::Bind {
            warn!("--actor-walk-test needs --actor-pose idle; actors stay in the bind pose");
        }
        info!("actor records loaded in {:.2}s", t0.elapsed().as_secs_f64());
        a
    });
    // The character creator's choices and sliders (nvf_play::creator), in play; it says
    // what it has once the log is up.
    let mut creator_data = actor_world
        .as_ref()
        .filter(|_| play.is_some())
        .map(|a| nvf_play::creator::CreatorData(std::sync::Arc::new(fnv_actor::creator::Creator::build(&a.actors, |p| game.read(p)))));
    let open_doors = args.open_doors;
    // What the overlay system and the settings page go by.
    args.no_overlay |= args.frame_json.is_some() || (args.play && !args.overlay);
    if args.follow_npc && args.npc_travel.is_empty() && args.npc_sit.is_empty() && args.npc_packages.is_empty() {
        return fail("--follow-npc needs --npc-travel, --npc-sit or --npc-packages".to_string());
    }
    // In play mode every actor of the loaded cells lives by its packages unless told
    // otherwise; the actor test aids pose the cell streamer's actors, so they keep them.
    let mut npc_packages = args.npc_packages.clone();
    let actor_test_aids = args.actor_pose != actors::ActorPose::Idle
        || args.actor_pose_time.is_some()
        || args.actor_walk_test.is_some()
        || args.actor_turn_test.is_some()
        || args.actor_weapon_test.is_some();
    if args.play
        && npc_packages.is_empty()
        && !args.no_npc_packages
        && !args.no_actors
        && !actor_test_aids
        && args.npc_travel.is_empty()
        && args.npc_sit.is_empty()
    {
        npc_packages.push("all".to_string());
    }
    if let Some(p) = play.as_mut() {
        p.packages = !npc_packages.is_empty();
    }
    let mut residents = if npc_packages.is_empty() {
        None
    } else {
        // Game hours per second: the sky's rate when it runs, else the NPCs' own.
        let rate = if sky_state.time_scale > 0.0 { sky_state.time_scale as f64 } else { args.npc_timescale.max(0.0) as f64 / 3600.0 };
        let clock = residents::Clock { hour: sky_state.hour, days_passed: start_day, rate };
        match residents::Residents::build(&world, &npc_packages, clock, args.npc_speed, args.play) {
            Ok(mut r) => {
                r.set_keep_apart(!args.actors_pass_through);
                Some(r)
            }
            Err(e) => return fail(e),
        }
    };
    // Furniture: used by the player in play mode and by --npc-sit travellers.
    let mut trips = args.npc_travel.clone();
    let furnishings = if args.play || !args.npc_sit.is_empty() || residents.is_some() {
        match furniture::Furnishings::build(&world, &game, &args.npc_sit, trips.len()) {
            Ok((mut f, sit_trips)) => {
                trips.extend(sit_trips);
                if let Some(r) = residents.as_mut() {
                    r.set_furniture_facts(std::sync::Arc::new(f.asset_facts(&game)));
                }
                Some(f)
            }
            Err(e) => return fail(e),
        }
    } else {
        None
    };
    let nav_state = if trips.is_empty() && !args.show_navmesh && residents.is_none() {
        None
    } else {
        match nav::NavState::build(&world, &trips, args.npc_speed, args.npc_ledges, args.show_navmesh, args.follow_npc && (!trips.is_empty() || residents.is_some())) {
            Ok(n) => Some(n),
            Err(e) => return fail(e),
        }
    };
    let mut graphics = match &args.graphics_only {
        Some(keys) => match graphics::Graphics::only(keys) {
            Ok(g) => g,
            Err(e) => return fail(e),
        },
        None => graphics::Graphics::preset(graphics_preset(&args)),
    };
    graphics.fsr = args.fsr.quality();
    let frame_times = match (args.log_frame_times, args.graphics_cycle.as_deref()) {
        (None, _) => None,
        (Some(every), cycle) => {
            let cycle = match cycle.map(graphics::FrameTimes::parse_cycle).transpose() {
                Ok(c) => c.unwrap_or_default(),
                Err(e) => return fail(e),
            };
            if let Some(first) = cycle.first() {
                graphics = first.graphics;
            }
            Some(graphics::FrameTimes { every, cycle })
        }
    };
    let mut app = App::new();
    // A command for an entity that is gone is logged, not fatal (stray_commands.rs).
    app.set_error_handler(stray_commands::handler);
    // NVF_FIXED_FRAME_TIME_MS=<ms>: every frame moves the clocks on by that much, whatever the
    // real frame took (Bevy's `TimeUpdateStrategy::ManualDuration`, set before Bevy's time
    // plugin, which keeps it), so the test aids reach the same view on the same frame in every
    // run: renderer changes compared frame by frame (docs/design/fsr-upscaling.md, "Mip
    // bias"). A diagnostic, not a setting.
    if let Some(ms) = std::env::var("NVF_FIXED_FRAME_TIME_MS").ok().and_then(|v| v.trim().parse::<f64>().ok()).filter(|ms| ms.is_finite() && *ms > 0.0) {
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(ms / 1000.0)));
        app.add_systems(Startup, move || info!("fixed frame time: every frame {ms} ms (NVF_FIXED_FRAME_TIME_MS)"));
    }
    // Asks for VK_AMD_anti_lag when Bevy creates the device: before DefaultPlugins (anti_lag.rs).
    app.add_plugins(anti_lag::AntiLagInitPlugin);
    app.add_plugins(
        DefaultPlugins
            .set(task_pools(cpu_placed.pool_threads))
            .set(ImagePlugin {
                default_sampler: ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    anisotropy_clamp: if graphics.anisotropic { 16 } else { 1 },
                    ..ImageSamplerDescriptor::linear()
                },
            })
            // Sound goes through nvf_play::audio_out, which follows the output device.
            .disable::<bevy::audio::AudioPlugin>()
            // Parts of Bevy the game never uses: sprites and 2D text (their layout,
            // bounds and picking), world serialization, Bevy's own animation (actors are
            // posed by nvf_play's clips), the clipboard and the UI widgets (the menus are
            // bevy_ui nodes with their own controls). Each adds systems that run every
            // frame (#1381). Kept: the sprite renderer, whose resources bevy_ui_render
            // reads, and glTF and scenes, as PbrPlugin registers a glTF extension.
            .disable::<bevy::sprite::SpritePlugin>()
            .disable::<bevy::world_serialization::WorldSerializationPlugin>()
            .disable::<bevy::animation::AnimationPlugin>()
            .disable::<bevy::clipboard::ClipboardPlugin>()
            .disable::<bevy::ui_widgets::ButtonPlugin>()
            .disable::<bevy::ui_widgets::CheckboxPlugin>()
            .disable::<bevy::ui_widgets::TextInputPlugin>()
            .disable::<bevy::ui_widgets::ListBoxPlugin>()
            .disable::<bevy::ui_widgets::MenuPlugin>()
            .disable::<bevy::ui_widgets::DialogPlugin>()
            .disable::<bevy::ui_widgets::ModalDialogPlugin>()
            .disable::<bevy::ui_widgets::popover::PopoverPlugin>()
            .disable::<bevy::ui_widgets::RadioGroupPlugin>()
            .disable::<bevy::ui_widgets::ScrollAreaPlugin>()
            .disable::<bevy::ui_widgets::ScrollbarPlugin>()
            .disable::<bevy::ui_widgets::SliderPlugin>()
            .disable::<bevy::ui_widgets::TabPlugin>()
            .disable::<bevy::input_focus::pointer_focus::PointerFocusPlugin>()
            // The console's lines go to the run log too (crash.rs).
            .set(bevy::log::LogPlugin { custom_layer: crash::file_layer, fmt_layer: crash::console_layer, filter: reports::log_filter(), ..default() })
            .set(WindowPlugin {
                // Scale factor 1 so screenshots are exactly --size pixels on any display.
                primary_window: Some(Window {
                    title: "NVForever".into(),
                    mode: args.window.mode(),
                    resolution: bevy::window::WindowResolution::new(w, h).with_scale_factor_override(1.0),
                    position: args.window_at.map_or(WindowPosition::Automatic, WindowPosition::At),
                    // Taking over a load: shown once its loading screen is up (handoff.rs).
                    visible: args.handoff.is_none(),
                    // Measuring frame times: not held to the display's refresh.
                    present_mode: if args.log_frame_times.is_some() { bevy::window::PresentMode::AutoNoVsync } else { default() },
                    // `NVF_SWAPCHAIN_LATENCY=<n>`: frames the swapchain may queue (images: n + 1;
                    // Bevy's 2 otherwise), for measuring frame generation's pacing.
                    desired_maximum_frame_latency: std::env::var("NVF_SWAPCHAIN_LATENCY").ok().and_then(|v| v.trim().parse::<u32>().ok()).and_then(std::num::NonZeroU32::new),
                    ..default()
                }),
                ..default()
            }),
    )
    .add_plugins((window_icon::WindowIconPlugin, window_mode::WindowModePlugin))
    .add_plugins(nvf_play::audio_out::AudioOutPlugin)
    .add_plugins((ArchivePlugin(game), lights::LightsPlugin, fog::FogPlugin, eye_adapt::EyeAdaptPlugin, emittance::EmittancePlugin, water::WaterPlugin, tree::TreePlugin, reflection::ReflectionPlugin, ripples::RipplesPlugin))
    .add_plugins((imagespace::ImageSpacePlugin, sky::SkyPlugin, graphics::GraphicsPlugin(graphics, msaa, frame_times)))
    // The switches the settings page weighs against the INI (`--play` only), for the
    // frame split's start-time decision (perf_overlay/split.rs).
    .add_plugins(perf_overlay::PerfOverlayPlugin(args.fps, play.is_some().then(|| settings::given_switches(&args, &command_line()))))
    // Vsync as Bevy's default, off while frame times are measured; no frame-rate cap.
    .add_plugins(frame_pace::FramePacePlugin(frame_pace::FramePace { sync: if args.log_frame_times.is_none() { frame_pace::DisplaySync::Vsync } else { frame_pace::DisplaySync::Off }, cap: 0 }))
    .add_plugins(anti_lag::AntiLagPlugin(args.latency_reduction))
    .add_plugins(frame_gen::FrameGenPlugin(args.frame_generation))
    .add_plugins(blit_warm::BlitWarmPlugin)
    // Lights clustered on the CPU, Bevy's own fallback: its GPU clustering made about six
    // new GPU buffers a frame (10,235 in 30 s at night against 4,060 on the CPU), GPU
    // memory made and freed every frame behind the night stalls (#897; wgpu trace,
    // 2026-10-29).
    .add_systems(Startup, |mut clusters: ResMut<bevy::light::cluster::GlobalClusterSettings>| {
        clusters.gpu_clustering = None;
        info!("lights: clustered on the CPU (GPU clustering made new buffers every frame)");
    })
    .add_plugins(grass::GrassPlugin(grass::GrassConfig {
        enabled: !args.no_grass,
        fade_start: args.grass_distance.max(0.0),
        fade_range: args.grass_fade_range.max(0.0),
        light: (grass_light[0].max(0.0), grass_light[1].max(0.0)),
        generate: fnv_world::grass::GrassSettings { min_size: args.grass_min_size, density_scale: args.grass_density.max(0.0), ..Default::default() },
    }))
    .add_plugins(draw_distance::DrawDistancePlugin(draw_distance::DrawDistances::new(&args)))
    // Small objects far away cast no sun shadows (shadow_thin.rs).
    .add_plugins(shadow_thin::ShadowThinPlugin(args.shadow_thin))
    // Interiors draw the rooms seen through portals only (rooms.rs).
    .add_plugins(rooms::RoomsPlugin { off: args.no_room_culling, planes_off: args.no_occlusion_planes, planes_outdoors: args.occlusion_planes_outdoors })
    .insert_resource(ClearColor(Color::srgb(0.62, 0.72, 0.85)))
    .insert_resource(GlobalAmbientLight { brightness: 600.0, ..default() })
    .insert_resource(sky_state)
    .insert_resource(screen_modifiers)
    .insert_resource(fly::AutoFly(args.auto_fly))
    .insert_resource(lod::LodState::new(&world, lod_settings(&args), !args.no_lod && interior.is_none(), lodgen).with_origin(lod_origin))
    .insert_resource(TerrainSettings { tile: args.terrain_tile })
    .init_resource::<TerrainCache>()
    .init_resource::<imposters::Imposters>()
    .insert_resource(Streamer {
        radius: args.radius,
        loaded: Default::default(),
        fades: Default::default(),
        fade_secs: args.cell_fade.max(0.0),
        settled: false,
        arrival: None,
        per_frame: 1,
        spawning: Default::default(),
        dying: Default::default(),
        ahead: Default::default(),
        hide: args.hide.iter().map(|h| h.to_ascii_lowercase()).collect(),
        interior,
        light_scale: args.light_scale,
        managed: nav_state
            .iter()
            .flat_map(|n| n.travellers.iter().map(|t| t.reference))
            .chain(residents.iter().flat_map(|r| r.references()))
            .collect(),
        manage_actors: residents.as_ref().is_some_and(|r| r.all),
        stats: Default::default(),
        buffer: Default::default(),
        centre: None,
        waiting: Vec::new(),
        model_overrides: Default::default(),
    })
    .insert_resource(world)
    .init_resource::<nvf_assets::actor::ActorParts>()
    // Cached models, actor parts and textures no live entity uses go after a grace time
    // (nvf_assets::release): otherwise every visited cell's GPU assets stay.
    .add_plugins(nvf_assets::AssetReleasePlugin)
    // Their keep limits follow the machine's RAM and GPU memory budget (memory_budget.rs).
    .add_plugins(memory_budget::MemoryBudgetPlugin)
    // wgpu's clean-up of finished GPU work on a thread of its own, not the render thread's
    // queue submit (gpu_poll.rs, #1427).
    .add_plugins(gpu_poll::GpuPollPlugin)
    .insert_resource(StartPose {
        position: start,
        heading: args.heading,
        pitch: args.pitch,
        fov: args.fov,
        overlay: !args.no_overlay,
        play: play.is_some(),
        msaa,
    })
    .insert_resource(args.clone())
    .add_systems(Startup, (world::setup_root, setup, sky::setup_sky))
    .add_systems(Update, sky::tell_sky_colors.after(sky::update_sky))
    .add_systems(Update, (fly::fly.run_if(move || play.is_none()), sky::sky_keys.run_if(move || play.is_none()), sky::weather_region, sky::update_sky, sky::interior_clip, sky::update_clouds, sky::update_night_sky, world::stream.run_if(world::world_not_held), imposters::update, fade::fade_cells, world::face_billboards, lod::update_lod, treelod::update_distant_trees, overlay, keys).chain())
    .add_systems(Update, world::note_other_cameras.after(world::stream))
    .add_systems(Update, (cell_buffer::remember, cell_buffer::tell_game).chain().after(world::stream))
    .add_systems(Update, world::script_markers.before(world::stream))
    .add_systems(Update, world::draw_waiting_actors.after(world::stream))
    // Actors of the next ring of cells read and prepared before they come into range.
    .insert_resource(readahead::ReadAhead::from_env())
    .add_systems(Update, readahead::read_ahead.run_if(world::world_not_held).after(world::stream).after(world::draw_waiting_actors))
    .add_systems(Last, readahead::report)
    .init_resource::<AnimCost>()
    .add_systems(Update, advance_animations)
    // Vertex morphs (waving flags, sagging wires): docs/spec-notes/vertex-morphs.md.
    .add_systems(Update, nvf_assets::morph::play_vertex_morphs.after(world::stream))
    .init_resource::<activators::ActivatorStats>()
    .add_systems(
        Update,
        (nvf_assets::material_anim::bind_rig_materials, activators::drive, nvf_assets::material_anim::animate_rig_materials)
            .chain()
            .after(advance_animations)
            .after(nvf_play::PlaySet::Scripts),
    )
    .add_systems(Last, activators::report)
    .add_plugins(nvf_play::doors::DoorsPlugin)
    .add_plugins(destroyed::DestroyedPlugin)
    .add_systems(Update, (doors::register, doors::open_all.run_if(move || open_doors), doors::pose).chain().after(nvf_play::doors::DoorSystems).after(advance_animations))
    .init_resource::<locomotion::ActorAnimCost>()
    .add_systems(PreUpdate, actors::take_background_work)
    .init_resource::<world::LateMeshes>()
    .add_systems(PreUpdate, (world::take_cell_work, world::respecialize_late_meshes).chain())
    .add_systems(Last, world::report)
    .add_systems(Update, (locomotion::walk_test, locomotion::script_turns, locomotion::turn_to, locomotion::drive, weapons::reparent).chain())
    // Clips that drive actors' part materials (the Securitron screens), once posed.
    .add_systems(Update, actor_materials::pose.after(locomotion::drive))
    .init_resource::<actor_materials::PlayerRigMaterials>()
    .add_systems(Update, actor_materials::pose_player.after(nvf_play::player::BodySet))
    .add_systems(Update, timelapse.after(sky::update_sky))
    // The sun's cascades reach behind the player's body (sky::SUN_REACH_BACK).
    // Only the sun's casters that can shade the view's slice of each cascade (#1378).
    .add_systems(PostUpdate, sky::sun_casters_over_slice.after(bevy::light::SimulationLightSystems::CheckLightVisibility))
    .add_systems(PostUpdate, sky::reach_toward_sun.after(bevy::light::SimulationLightSystems::UpdateDirectionalLightCascades).before(bevy::light::SimulationLightSystems::UpdateLightFrusta))
    .add_systems(Startup, (crash::report_start, report_archives))
    .add_systems(Startup, move || cpu_placed.report())
    .add_plugins(start_times::StartTimesPlugin)
    .add_plugins(reports::ReportsPlugin { plugins: args.plugins.clone(), data: args.data().to_path_buf(), mo2: fnv_install::mo2::usvfs_loaded(), handoff: args.handoff.is_some(), start_kind: if args.load.is_some() { "save_load" } else { "new_game" } })
    .add_systems(Last, (crash::note_place, events_log::note_context, crash::report_exit, stray_commands::report));
    if let Some(probe) = actor_feet::from_env() {
        app.insert_resource(probe).add_systems(Last, actor_feet::probe);
    }
    if let Some(radius) = args.actor_turn_test {
        app.insert_resource(locomotion::TurnTest { radius, started: 0, finished: 0, degrees: 0.0 })
            .add_systems(Update, locomotion::turn_test.before(locomotion::turn_to))
            .add_systems(Last, locomotion::turn_test_report);
    }
    if args.new_game {
        app.insert_resource(nvf_play::script::NewGame);
    }
    if args.no_messages {
        app.insert_resource(nvf_play::script::NoMessages);
    }
    app.insert_resource(nvf_play::voice::VoiceSettings { play: !args.no_voice, volume: args.voice_volume, ..Default::default() });
    app.insert_resource(nvf_play::sfx::SfxSettings { play: !args.no_sound, reverb: !args.no_reverb, ..Default::default() });
    app.insert_resource(nvf_play::combat::HitMarks { show: args.hit_markers });
    if args.god {
        nvf_play::combat::set_god_mode(true);
    }
    app.insert_resource(nvf_play::music::MusicSettings { play: !args.no_music, volume: args.music_volume });
    app.insert_resource(if args.menu_pause == "original" { nvf_play::pause::MenuPause::Original } else { nvf_play::pause::MenuPause::Living });
    app.insert_resource(nvf_play::dialogue_look::LookSettings { enabled: !args.no_dialogue_look, recentre_on_choices: args.dialogue_recentre && !args.no_dialogue_recentre });
    app.insert_resource(nvf_play::dialogue_zoom::ZoomSettings { enabled: !args.no_dialogue_zoom });
    if let Some(p) = pending {
        app.insert_resource(p);
    }
    if args.menu {
        app.insert_resource(nvf_play::main_menu::MainMenuStart);
    } else if args.play {
        // A run that starts in the world: held behind a loading screen until its cells,
        // actors and scripts are ready (nvf_play::loading::StartCover).
        app.insert_resource(nvf_play::loading::StartCover);
    }
    if let Some(data) = play {
        // Travel reloads cells before streaming runs again; the play systems run in
        // nvf-play's own sets.
        // Streaming follows the camera, which the player systems move: stream after them.
        // The loading screen sends the travel in the Input set and reads in `Last` whether
        // the renderer is done: both this frame.
        app.add_plugins(nvf_play::PlayPlugin(data))
            .add_systems(Update, (worldspace::switch, world::travel, world::cell_image_space).chain().after(nvf_play::PlaySet::Input).before(world::stream))
            .add_plugins(pipeline_gate::PipelineGatePlugin)
            .add_systems(Update, world::loading_gate.after(world::stream))
            .configure_sets(Update, nvf_play::PlaySet::Physics.before(world::stream));
        // Animation groups scripts play on objects (object_anims.rs).
        app.add_plugins(object_anims::ObjectAnimsPlugin);
        // The flashlight's light on the front of the player's body (flashlight.rs).
        app.add_plugins(flashlight::FlashlightPlugin);
        // Textures scripts swap on references' shapes (texture_swap.rs).
        app.add_plugins(texture_swap::TextureSwapPlugin);
        // Effect shaders and actor alpha scripts and magic effects ask for (effect_shaders.rs).
        app.add_plugins(effect_shaders::EffectShadersPlugin);
        app.add_plugins(vats_highlight::VatsHighlightPlugin);
        // How many refracting meshes are in view, for the log (refraction.rs).
        app.add_plugins(refraction::RefractionPlugin);
        // The lock and the bobby pin of the lockpicking menu (lockpick_view.rs).
        app.add_plugins(lockpick_view::LockpickViewPlugin);
        // The table, wheel and chips of the roulette menu (roulette_view.rs).
        app.add_plugins(roulette_view::RouletteViewPlugin);
        // The blackjack table (blackjack_view.rs).
        app.add_plugins(blackjack_view::BlackjackViewPlugin);
        // The terminal's monitor (terminal_view.rs).
        app.add_plugins(terminal_view::TerminalViewPlugin);
        // A weapon's scope: its picture, night vision and sway (scope_view.rs).
        app.add_plugins(scope_view::ScopeViewPlugin);
        // The poker table (poker_view.rs).
        app.add_plugins(poker_view::PokerViewPlugin);
    }
    if let Some(a) = actor_world {
        if let Some(c) = creator_data.take() {
            app.insert_resource(c);
        }
        app.insert_resource(a);
        let force_point = match args.head_track_point.as_deref().map(|p| parse_vec::<3>(p).ok_or(p)) {
            Some(Err(p)) => return fail(format!("--head-track-point {p}: expected x,y,z")),
            Some(Ok(p)) => Some(p),
            None => None,
        };
        // Also with --no-head-tracking: the settings page can switch it on.
        app.add_plugins(head_look::HeadLookPlugin { enabled: !args.no_head_tracking, camera_is_player: args.head_track_camera, force_point });
        // Also with --no-foot-ik: the settings page can switch it on.
        app.add_plugins(foot_ik::FootIkPlugin { enabled: !args.no_foot_ik });
        // Actors jumping and falling: their jump and landing clips (actor_jumps.rs); the
        // --npc-travel walkers' too, without --play.
        app.add_plugins(actor_jumps::ActorJumpsPlugin { physics: play.is_some() });
        if play.is_some() {
            // The player's own body (player_body.rs): needs the actor records.
            app.add_plugins(creator_view::CreatorViewPlugin { light_scale: args.light_scale });
            // The Vigor Tester's machine: its camera and lights, the room behind it.
            app.add_plugins(vigor_view::VigorViewPlugin);
            app.add_plugins((player_body::PlayerBodyPlugin { first_person_body: args.first_person_body, weapon_sway: !args.no_weapon_sway, head_bob: !args.no_head_bob, weapon_lower: !args.no_weapon_lower, true_iron_sights: !args.no_true_iron_sights }, stealth_field::StealthFieldPlugin, pipboy_arm::PipBoyArmPlugin, deaths::DeathsPlugin, limbs::LimbsPlugin, ragdoll::RagdollPlugin, swing_traps::SwingTrapsPlugin, wrecks::WrecksPlugin, shot_fx::ShotFxPlugin, particles::ParticlesPlugin, dismember::DismemberPlugin));
            app.insert_resource(nvf_play::aim_zoom::Scope3d { on: args.scope_3d, lens: false });
            app.add_plugins(decals::DecalsPlugin);
            app.add_plugins(knockouts::KnockoutsPlugin);
            // The stress tests' samples (NVF_PLAY_TEST_SAMPLE; nothing without it).
            app.add_plugins(stress_sample::StressSamplePlugin);
            // The span profile (NVF_PLAY_TEST_PROFILE; nothing without it; profile.rs).
            app.add_plugins(profile::ProfilePlugin);
            app.add_plugins(frame_telemetry::FrameTelemetryPlugin(args.data().to_path_buf()));
            app.insert_resource(death_cam::DeathCam::with_imod(death_imod)).add_plugins(death_cam::DeathCamPlugin);
            // The player knocked down (player_knockdown.rs).
            app.add_plugins(player_knockdown::PlayerKnockdownPlugin);
            app.add_plugins((cell_resets::CellResetsPlugin, resurrect::ResurrectPlugin, appearance::AppearancePlugin, zones::ZonesPlugin));
            // What drawn NPCs wear and hold follows their inventories (outfits.rs).
            app.add_plugins(outfits::OutfitsPlugin);
            if shadow_probe::enabled() {
                app.add_plugins(shadow_probe::ShadowProbePlugin);
            }
            app.add_plugins(lips::LipsPlugin { enabled: !args.no_lip, idle: !args.no_face_idle });
            // The sounds clips and doors name (anim_sounds.rs).
            app.add_plugins(anim_sounds::AnimSoundsPlugin);
            // Add-on nodes', projectiles' and actors' weapons' sounds (own_sounds.rs).
            app.add_plugins(own_sounds::OwnSoundsPlugin);
            // The conversation's actor turns to the player (dialogue_look.rs).
            app.add_plugins(dialogue_look::DialogueLookPlugin);
            // AttachUIComponent's rects over their model nodes (ui_attach.rs, #1835).
            app.add_plugins(ui_attach::UiAttachPlugin);
            // Who says a line to someone turns to them (speech_look.rs).
            app.add_plugins(speech_look::SpeechLookPlugin);
            // Who perceives whom (detection.rs): the drawn actors and the player.
            app.add_plugins(detection::DetectionPlugin);
            // The weather's wind on the bodies that respond to it, the tumbleweeds (tumble.rs).
            app.add_plugins(tumble::TumblePlugin);
            // Alerted actors draw their weapons and look at what alerted them (alert.rs).
            app.add_plugins(alert::AlertPlugin);
            // Fighters look angry (combat_face.rs; lips.rs draws the face).
            app.add_plugins(combat_face::CombatFacePlugin);
            // Alerted creatures raise their claws and hands (creature_stance.rs).
            app.add_plugins(creature_stance::CreatureStancePlugin);
            // Robots and Mr. House light up as they talk (talk_clips.rs).
            app.add_plugins(talk_clips::TalkClipsPlugin);
            // Actors sneaking by script or package (sneaking.rs).
            app.add_plugins(sneaking::SneakingPlugin);
            // Actors swimming in water (swimming.rs).
            app.add_plugins(swimming::SwimmingPlugin);
            // Actors limping on a crippled leg (limping.rs).
            app.add_plugins(limping::LimpingPlugin);
            // Actors drawn at a script's scale (actor_scale.rs).
            app.add_plugins(actor_scale::ActorScalePlugin);
            // Creatures' own speeds and alert stance (creature_pace.rs).
            app.add_plugins(creature_pace::CreaturePacePlugin);
            // Heads held by a script's Look (script_look.rs).
            app.add_plugins(script_look::ScriptLookPlugin);
            // Actors in Combat attack their target (npc_combat.rs).
            app.add_plugins(npc_combat::NpcCombatPlugin);
            // NPCs heal with the stimpaks they carry (npc_stimpaks.rs).
            app.add_plugins(npc_stimpaks::NpcStimpaksPlugin);
            // Creatures fight with their body weapons and switch between them (body_weapons.rs).
            app.add_plugins(body_weapons::BodyWeaponsPlugin);
            // Scripted weapon use: UseWeapon, Use Weapon packages (use_weapon.rs).
            app.add_plugins(use_weapon::UseWeaponPlugin);
            app.add_plugins(combat_tactics::CombatTacticsPlugin);
            // Fights started by hits; the player's yield (provoked.rs).
            app.add_plugins(provoked::ProvokedPlugin);
            // How fighters aim, shoot and bite (attack_anims.rs).
            app.add_plugins(attack_anims::AttackAnimsPlugin);
            // What the actors' bodies do, told to the rules (anim_state.rs).
            app.add_plugins(anim_state::AnimStatePlugin);
            // JIP's NIF block functions on the drawn nodes (nif_mirror.rs).
            app.add_plugins(nif_mirror::NifMirrorPlugin);
            // kNVSE's replacement animations on the drawn actors (anim_paths.rs).
            app.add_plugins(anim_paths::AnimPathsPlugin);
        }
    }
    if play.is_some() {
        app.add_systems(Update, (script_weather, worldspace::script_sky, sky_follows_game).chain().before(sky::update_sky));
        // The weather's sounds, precipitation and lightning (weather_fx.rs).
        app.add_plugins(weather_fx::WeatherFxPlugin);
    }
    if play.is_some() || residents.is_some() {
        // Idle animations (idles.rs): after the game asked for them (scripts, packages,
        // dialogue lines), before the actors are posed. The player's body, posed earlier
        // in the frame, takes them up in the next one.
        app.init_resource::<idles::Idles>()
            .init_resource::<idles::IdleHolds>()
            .add_systems(Update, idles::drive.after(nvf_play::PlaySet::Scripts).after(residents::drive).before(locomotion::walk_test))
            .add_systems(Last, idles::report);
        // The props idles hold (anim_objects.rs): after the idles started and ended.
        app.init_resource::<anim_objects::AnimObjects>()
            .add_systems(Startup, anim_objects::read)
            .add_systems(Update, anim_objects::hold.after(idles::drive))
            .add_systems(Last, anim_objects::report);
    }
    app.init_resource::<idles::PlayerIdles>();
    if let Some(f) = furnishings {
        // After the travellers moved, before the actors are posed.
        app.insert_resource(f)
            .add_systems(Startup, furniture::log_startup)
            .add_systems(Update, (furniture::seat_orders, furniture::drive_actors).chain().after(nav::drive).before(locomotion::walk_test))
            .add_systems(Last, furniture::report);
    }
    if let Some(n) = nav_state {
        let follow = n.follow;
        app.insert_resource(n).add_systems(Startup, nav::log_startup).add_systems(
            Update,
            // After the read-ahead, which sets its loading-screen rule (`ReadAhead::unseen`) for this frame.
            (nav::drive.after(world::stream).after(readahead::read_ahead), nav::follow_camera.after(fly::fly).before(world::stream), nav::draw.after(nav::drive)),
        ).add_systems(
            Last,
            nav::report,
        );
        if let Some(r) = residents {
            app.insert_resource(r)
                .add_systems(Startup, residents::log_startup)
                .add_systems(Update, residents::drive.after(world::stream).after(readahead::read_ahead).after(nvf_play::PlaySet::Scripts).before(nav::drive))
                .add_systems(Update, residents::melee_knockback::arrange.after(residents::drive).run_if(resource_exists::<nvf_play::MeleeKnockbackAid>))
                .add_systems(Last, residents::report);
            // Patrols and sandboxes go on after a load where the save had them.
            if play.is_some() {
                nvf_play::save::add_save_hook(&mut app, residents::SAVE_HOOK);
            }
            // Load doors: bodies fade out and in, the doors sound (door_fade.rs).
            app.init_resource::<door_fade::DoorFades>()
                .add_message::<nvf_play::world_sounds::DoorPassed>()
                .add_systems(Update, door_fade::fade.after(residents::drive).before(locomotion::walk_test))
                .add_systems(Last, door_fade::report);
        }
        if play.is_none() {
            // Travellers' door trips (and the camera following one) use the play mode's
            // travel message; without --play, the renderer applies it alone.
            app.add_message::<nvf_play::Travel>();
            if follow {
                app.add_systems(Update, (worldspace::switch, world::travel).chain().after(nav::drive));
            }
        }
    }
    if args.screenshot.is_some() {
        app.add_systems(Update, screenshot_and_exit);
    }
    if let Some(secs) = args.run_seconds {
        #[allow(clippy::too_many_arguments)]
        let summary = move |time: Res<Time>,
                            clocks: RunClocks,
                            streamer: Res<Streamer>,
                            lod: Res<lod::LodState>,
                            grass: Res<grass::GrassCache>,
                            actors: Option<Res<actors::ActorWorld>>,
                            anim: Res<AnimCost>,
                            actor_anim: Res<locomotion::ActorAnimCost>,
                            mut commands: Commands,
                            mut t: Local<(f32, u32)>| {
            // While a menu holds the picture (the virtual clock is paused: --menu-pause
            // original, the Esc pause menu) the limit counts real time, so a run always
            // ends.
            // The start's loading screen does not count: the run's seconds are counted
            // from when it comes down (nvf_play::loading::StartCover).
            if !clocks.3.as_deref().is_some_and(|l| l.starting()) {
                // (A clock running at no speed, the classic VATS style's full stop, counts
                // real time as well.)
                let still = clocks.0.is_paused() || nvf_play::pause::stands_still(false, clocks.0.relative_speed());
                t.0 += if still { clocks.1.delta_secs().min(0.25) } else { time.delta_secs() };
            }
            t.1 += 1;
            // The seconds a late start held the world test aids back (`--play` with
            // NVF_PLAY_AIDS_FROM_SCRIPTS, `nvf_play::pause::WorldAidClock`) are added, so
            // their last steps still run.
            let held = clocks.2.as_deref().map_or(0.0, |w| w.held_secs());
            if t.0 >= secs + held {
                if held > 0.0 {
                    info!("--run-seconds {secs}: run {held:.1} s longer, as long as the world test aids were held back");
                }
                let (baked, cached) = lod.gen.as_ref().map_or((0, 0), |g| (g.baked, g.cached));
                info!(
                    "run finished after {:.1}s: {} cells loaded in total, {} loaded now, {} frames ({:.1} per second), generated LOD chunks {} baked, {} from the cache, actors {:?}",
                    t.0, streamer.stats.cells_loaded, streamer.loaded.len(), t.1, t.1 as f32 / t.0, baked, cached, streamer.stats.actors
                );
                info!("distant LOD at the end: {}", lod.describe());
                info!("distant LOD blocks over the run: {}", lod.block_stats.describe());
                info!("grass at the end: {}, {} generated in total in {:.0} ms", grass.describe(), grass.plants_generated, grass.build_ms);
                info!(
                    "animation: up to {} clips playing at once, {:.3} ms per frame on average, {:.2} us per clip and frame, {} switches of nodes shown and hidden by visibility tracks",
                    anim.peak,
                    anim.total.as_secs_f64() * 1e3 / f64::from(anim.frames.max(1)),
                    anim.total.as_secs_f64() * 1e6 / anim.clip_frames.max(1) as f64,
                    anim.switched
                );
                let c = &*actor_anim;
                if c.actor_frames == 0 {
                    info!("actor animation: no actors posed");
                } else {
                    // The us per actor figure is approximate: `total` less `held_time` still holds
                    // the loop's cost on held actors, divided by sampled actor frames only, so
                    // many held corpses can raise it.
                    info!(
                    "actor animation: up to {} actors posed at once, {:.3} ms per frame on average, {:.2} us per actor and frame, {:.2} clips per actor and frame ({:.1}% of actor frames with more than one clip), {:.1} tracks per actor and frame",
                    c.peak,
                    c.total.as_secs_f64() * 1e3 / f64::from(c.frames.max(1)),
                    c.total.saturating_sub(c.held_time).as_secs_f64() * 1e6 / c.actor_frames.max(1) as f64,
                    c.layer_frames as f64 / c.actor_frames.max(1) as f64,
                    100.0 * c.blend_frames as f64 / c.actor_frames.max(1) as f64,
                    c.tracks as f64 / c.actor_frames as f64
                    );
                    info!("actor turns: {} actor frames playing a turn-on-the-spot clip", c.turn_frames);
                    info!("actor idle under movement: {} actor frames with the idle laid under another clip", c.under_frames);
                    info!("actor moves without a clip: {} actor frames moving with no movement clip showing; {} actor frames of a creature's blow on its head and neck over its run", c.clipless_frames, c.head_blow_frames);
                }
                if c.held_frames + c.dead_tracks > 0 {
                    info!(
                        "death poses: {} actor frames held without sampling ({} at the clip's end), {} more at the clip's end sampled ({:.2} finished poses per frame), {} tracks sampled on dead actors ({:.1} per frame), {:.2} us per held pose (in the actor animation line's ms, not its per actor figures)",
                        c.held_frames,
                        c.finished_held_frames,
                        c.finished_sampled_frames,
                        (c.finished_held_frames + c.finished_sampled_frames) as f64 / f64::from(c.frames.max(1)),
                        c.dead_tracks,
                        c.dead_tracks as f64 / f64::from(c.frames.max(1)),
                        c.held_time.as_secs_f64() * 1e6 / c.held_frames.max(1) as f64
                    );
                }
                if let Some(a) = actors {
                    let fg = a.facegen.lock().expect("facegen cache");
                    info!(
                        "FaceGen: {} shapes morphed, {} faces textured, {} skins textured, {} shapes tinted in the shader ({:.1} ms making offsets, {:.1} ms reading .egt files), {} EGMs not matching their shape",
                        fg.morphed_shapes,
                        fg.faces_tinted,
                        fg.skins_tinted,
                        fg.shapes_tinted,
                        fg.offset_time.as_secs_f64() * 1e3,
                        fg.egt_time.as_secs_f64() * 1e3,
                        fg.mismatched
                    );
                }
                commands.write_message(AppExit::Success);
            }
        };
        app.add_systems(Update, summary);
    }
    if play.is_some() {
        settings::register(&mut app);
        console_toggles::register(&mut app);
        app.init_resource::<handoff::Handoff>().add_systems(Update, (handoff::leave, handoff::reveal.after(world::loading_gate), handoff::hold_restore.after(handoff::reveal)));
        handoff::add_end_now(&mut app);
        if let Some(why) = load_failed {
            app.insert_resource(handoff::LoadFailed(why)).add_systems(Update, handoff::tell_load_failed);
        }
        if let Some(file) = args.handoff.clone() {
            match &handoff_marked {
                Some(Err(e)) => warn!("handoff: started as the new run, but cannot tell the old run so ({}: {e}); it ends after {} s", handoff::started_marker(&file).display(), handoff::JOIN_WAIT_SECS),
                _ => info!("handoff: started as the new run (ready file {})", file.display()),
            }
            app.insert_resource(handoff::Hidden::new(file, args.window_maximized));
        }
        nvf_play::save::add_save_hook(&mut app, sky::SAVE_HOOK);
    }
    if args.window_maximized && args.handoff.is_none() {
        app.add_systems(Startup, |mut w: Query<&mut Window, With<bevy::window::PrimaryWindow>>| {
            if let Ok(mut w) = w.single_mut() {
                w.set_maximized(true);
            }
        });
    }
    // `Update` runs its systems one after another on the main thread (Bevy's
    // single-threaded executor). The game's Update is some hundreds of small systems,
    // most of them chained or sharing data, so the multithreaded executor found little to
    // run side by side and its own work (a task per system, woken on another thread) cost
    // more than the systems did: at Goodsprings the schedule took about 4.2 ms of the
    // main thread around 1.5 ms of systems [observed 2026-11-10, Ryzen 7 7700X, release
    // build, frames held by the CPU: Goodsprings 5.67 -> 5.36 ms, the Strip 5.93 -> 5.68 ms,
    // a 25-a-side battle 10.72 -> 10.62 ms, the Prospector Saloon unchanged;
    // docs/design/frame-rate-audit.md section 9]. Systems that split their own work over
    // the task pools (transform propagation, par_iter) still do. The other schedules
    // measured no different either way and keep Bevy's default.
    // `NVF_PLAY_TEST_UPDATE_EXECUTOR=multi` (test aid) puts Bevy's default back, for
    // measuring one against the other. Systems with no order between them that touch the
    // same data (ambiguities) run in one fixed order here, frame after frame, where the
    // multi-threaded executor picked one each frame; a hidden dependency shows up as one
    // frame late every time instead of now and then. `NVF_PLAY_TEST_UPDATE_AMBIGUITIES=1`
    // (test aid) has Bevy list them as the schedule is built.
    if std::env::var("NVF_PLAY_TEST_UPDATE_EXECUTOR").as_deref() != Ok("multi") {
        app.edit_schedule(Update, |s| {
            s.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
    }
    if std::env::var("NVF_PLAY_TEST_UPDATE_AMBIGUITIES").as_deref() == Ok("1") {
        app.edit_schedule(Update, |s| {
            s.set_build_settings(bevy::ecs::schedule::ScheduleBuildSettings {
                ambiguity_detection: bevy::ecs::schedule::LogLevel::Warn,
                ..default()
            });
        });
    }
    app.run()
}

/// `--load <name>`: read the save in full for the plugins opened and start at its place
/// (the world, cell, position, view, hour and weather of `args`); why not, when it cannot
/// be loaded.
fn start_from_save(name: &str, opened: &world::OpenedPlugins, args: &mut Args) -> Result<nvf_play::save::PendingLoad, String> {
    let path = nvf_play::save::SaveConfig::default().path_of(name)?;
    let plugins: Vec<fnv_save::PluginEntry> = opened.plugins.iter().map(|p| fnv_save::PluginEntry { name: p.name.clone(), size: p.file_size }).collect();
    let (mut header, body, order) = nvf_play::save::read_for_load(&path, &plugins).map_err(|e| format!("--load {name}: {e}"))?;
    // A save made with another load order: the place's FormIDs move to the current one; a
    // place in a plugin no longer loaded cannot be started in.
    if let Some(o) = &order {
        let pl = &mut header.place;
        for id in [&mut pl.world, &mut pl.interior].into_iter().flatten() {
            match o.remap.map(*id) {
                Some(n) => *id = n,
                None => return Err(format!("--load {name}: the save was made in 0x{id:08X} of {}, which is not loaded now", o.remap.plugin_of(*id).unwrap_or("a plugin"))),
            }
        }
    }
    let pl = &header.place;
    if let Some(w) = pl.world {
        args.world = format!("0x{w:08X}");
    }
    args.cell = match pl.interior {
        Some(c) => format!("0x{c:08X}"),
        None => format!("{},{}", (pl.feet[0] / fnv_world::CELL_SIZE).floor(), (pl.feet[1] / fnv_world::CELL_SIZE).floor()),
    };
    args.pos = Some(format!("{},{},{}", pl.feet[0], pl.feet[1], pl.feet[2] + nvf_play::player::EYE_HEIGHT));
    args.eye_height = 0.0;
    args.heading = pl.heading_deg;
    args.pitch = pl.pitch_deg;
    args.hour = header.game_hour;
    args.sky_day = header.game_days_passed;
    if header.weather.is_some() && args.weather.is_none() {
        args.start_weather = header.weather.clone();
    }
    // Before the log is set up: straight to the output.
    eprintln!("load: {} ({}, level {}, {}): starting at {:?}", path.display(), header.player_name, header.level, header.location, pl);
    Ok(nvf_play::save::PendingLoad { path, body, order })
}

/// The whole days a run starts on: the game's start clock (`--day`, else the plugins'), and
/// the sky's, which a save's day overrides. A save's day stays out of the start clock, which
/// only a new game takes (`Core::prepare` in residents.rs); a save that cannot be restored
/// plays no game at all, it opens the main menu [owner 2026-11-01, #1357].
fn start_days(day: Option<f32>, sky_day: Option<f32>, plugins: Option<f32>) -> (f32, f32) {
    let game = day.or(plugins).unwrap_or(0.0).floor();
    (game, sky_day.map_or(game, f32::floor))
}

/// The arguments of this run without those a save or the window decide (with their
/// values, in both the `--flag value` and `--flag=value` forms), for the run that takes
/// over a load (handoff.rs).
fn restart_args(args: &[String]) -> Vec<String> {
    const VALUED: [&str; 13] = ["--load", "--load-refused", "--cell", "--pos", "--heading", "--pitch", "--hour", "--weather", "--world", "--eye-height", "--size", "--window-at", "--handoff"];
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let name = a.split('=').next().unwrap_or(a);
        if a == "--new-game" || a == "--menu" || a == "--window-maximized" {
            i += 1;
            continue;
        }
        if VALUED.contains(&name) {
            i += if a.contains('=') { 1 } else { 2 };
            continue;
        }
        out.push(a.clone());
        i += 1;
    }
    out
}

/// `--graphics`, or its default: vanilla for the test runs, whose pictures, timings and
/// expectations were made with it, the play default otherwise (graphics.rs).
fn graphics_preset(args: &Args) -> graphics::Preset {
    let test_run = args.screenshot.is_some() || args.timelapse.is_some() || args.frame_json.is_some() || args.run_seconds.is_some();
    args.graphics.unwrap_or(if test_run { graphics::Preset::Vanilla } else { graphics::PLAY_DEFAULT })
}

#[derive(Resource)]
struct StartPose {
    position: Vec3,
    heading: f32,
    pitch: f32,
    /// The game's field-of-view setting in degrees (see `vertical_fov`).
    fov: f32,
    overlay: bool,
    /// The camera is the player (`--play`).
    play: bool,
    /// Anti-aliasing samples of the main view (`--msaa`).
    msaa: Msaa,
}

/// Vertical field of view (radians) for the game's FOV setting (degrees): the setting is the
/// horizontal angle at a 4:3 aspect and the vertical angle stays fixed at every other aspect
/// ("Hor+"), so Bevy's vertical `fov` needs no update on resize [hypothesis; matched at 16:9
/// on four reference frames, docs/spec-notes/camera.md].
fn vertical_fov(setting_deg: f32) -> f32 {
    nvf_play::free_cam::vertical_fov(setting_deg)
}

fn setup(mut commands: Commands, start: Res<StartPose>) {
    let pos = game_to_bevy().transform_point(start.position);
    let (yaw, pitch) = (-start.heading.to_radians(), start.pitch.to_radians());
    let cam = commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: vertical_fov(start.fov),
            far: sky::OUTDOOR_FAR,
            near: 5.0,
            ..default()
        }),
        Transform::from_translation(pos).with_rotation(Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0)),
        FlyCam { yaw, pitch, speed: 800.0 },
        start.msaa,
        imagespace::ImageSpaceGrade::default(),
        sky::initial_fog(),
        // Water reads the depth of what lies behind it (water.rs).
        bevy::core_pipeline::prepass::DepthPrepass,
        // The world, the water (which the reflection camera leaves out), the player's
        // first-person arms and seen body, and the impact decals (which need this
        // camera's prepass).
        bevy::camera::visibility::RenderLayers::from_layers(&[0, reflection::WATER_LAYER, sky::ARMS_LAYER, sky::BODY_LAYER, decals::DECAL_LAYER]),
    )).id();
    if start.play {
        commands.entity(cam).insert(nvf_play::PlayerCamera);
    }
    // Hidden with --no-overlay; the settings page can show it.
    commands.spawn((
        Text::new(""),
        TextFont { font_size: FontSize::Px(14.0), ..default() },
        Node { position_type: PositionType::Absolute, top: Val::Px(8.0), left: Val::Px(8.0), ..default() },
        if start.overlay { Visibility::Inherited } else { Visibility::Hidden },
        Overlay,
    ));
}

#[allow(clippy::too_many_arguments)]
fn overlay(
    streamer: Res<Streamer>,
    models: Res<ModelCache>,
    terrain: Res<TerrainCache>,
    textures: Res<TextureCache>,
    sky: Res<sky::SkyState>,
    lod: Res<lod::LodState>,
    grass: Res<grass::GrassCache>,
    cam: Query<&GlobalTransform, With<FlyCam>>,
    mut text: Query<&mut Text, With<Overlay>>,
    play_state: Option<Res<State<nvf_play::PlayState>>>,
    args: Res<Args>,
) {
    if args.no_overlay {
        return;
    }
    let (Ok(cam), Ok(mut t)) = (cam.single(), text.single_mut()) else { return };
    let g = world::bevy_to_game(cam.translation());
    let c = fnv_world::cell_of(g.x, g.y);
    let s = &streamer.stats;
    t.0 = format!(
        "pos {:.0}, {:.0}, {:.0}  cell {},{}
cells {} ({} loads, {:.0} ms total)  refs drawn {} (animated {})  disabled {}  not drawn {}  speedtree {}  no model {}  shapes {}
models {} (failed {})  textures failed {}  terrain layers {}  quadrants without base {}  water {}  {}  lights {}  {}
{}
{}",
        g.x, g.y, g.z, c.0, c.1,
        streamer.loaded.len(), s.cells_loaded, s.load_ms,
        s.refs_drawn, s.refs_animated, s.refs_skipped_disabled, s.refs_skipped_type, s.refs_speedtree, s.refs_no_model, s.shapes,
        models.len(), models.failures.len(), textures.failures.len(), terrain.layers, terrain.quadrants_without_base,
        s.water_planes, lod.describe(), s.lights, grass.describe(), sky.describe(),
        if play_state.is_some() { nvf_play::HELP } else { "WASD/QE move  Shift fast  right-drag look  wheel speed  [ ] hour  F12 screenshot  Esc quit" },
    );
}

fn keys(
    input: Res<ButtonInput<KeyCode>>,
    play_state: Option<Res<State<nvf_play::PlayState>>>,
    mut commands: Commands,
) {
    // With --play, Esc is nvf-play's: it opens the pause menu (Continue / Save / Load / Settings / Quit) or closes
    // the open menu, instead of quitting.
    if input.just_pressed(KeyCode::Escape) && play_state.is_none() {
        commands.write_message(AppExit::Success);
    }
    // F12: a shot in the home's screenshots folder at the first free number, through the
    // write guard. It went to the working folder unguarded, and the count restarted at 1
    // each run, over the last run's shots [#1987].
    if input.just_pressed(KeyCode::F12) {
        let dir = fnv_install::EngineHome::user().map_or_else(|| std::path::PathBuf::from("."), |h| h.screenshots());
        let path = fnv_install::first_unused(&dir, "nvforever-", "png");
        let ready = fnv_install::guard_write(&path, None)
            .map_err(|e| e.to_string())
            .and_then(|()| std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display())));
        match ready {
            Ok(()) => {
                info!("screenshot: {}", path.display());
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            }
            Err(e) => warn!("screenshot not taken: {e}"),
        }
    }
}

/// The lab's frame document: where the image is and how it was made.
/// `--play`: one clock. The game's GameHour starts at the sky's hour (`--hour`); from
/// then on the sky shows the game's time of day, which the simulation advances at the
/// TimeScale global. An hour set by hand (the `[` and `]` keys) becomes the game's. The
/// time-lapse test aid (`--time-scale`) keeps the sky's own clock, and so does
/// `--still-sky` (the settings page switches it while the game runs).
/// The weather scripts ask for (SetWeather, ForceWeather, ReleaseWeatherOverride;
/// `fnv_game::weather`), and back to the scripts the weather shown (IsRaining,
/// GetIsCurrentWeather).
fn script_weather(mut script: Option<ResMut<nvf_play::script::ScriptState>>, world: Option<Res<nvf_play::PlayWorld>>, mut sky: ResMut<sky::SkyState>) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let g = &mut rt.game;
    if let Some(r) = g.take_weather_request() {
        match world.as_deref().and_then(|w| fnv_world::sky::weather(w.lo, w.schema, r.weather.0).map(|wt| (w, wt))) {
            Some((w, wt)) => {
                let adapters = fnv_world::imagespace::weather_adapters(w.lo, w.schema, &wt).map(|id| id.and_then(|id| fnv_world::imagespace::adapter(w.lo, w.schema, id)));
                sky.script_weather(wt, adapters, r.instant, r.hold);
            }
            None => warn!("weather: a script asked for {}, which is not a weather", g.data.name(r.weather)),
        }
    }
    if g.take_weather_release() {
        sky.release_script_weather();
    }
    let (shown, raining) = (fnv_sim::FormId(sky.weather.form_id), sky.raining());
    if g.weather.shown != Some(shown) || g.weather.raining != raining {
        g.set_shown_weather(shown, raining);
    }
    // The scripts' wind and lightning (SetWindDirection, TriggerLightningFX;
    // `fnv_game::nvse_jip12`); their climate and moons: `worldspace::script_sky`.
    let wind = g.script_wind();
    if sky.script_wind != wind {
        sky.script_wind = wind;
    }
    if g.take_lightning() {
        sky.lightning_request = true;
    }
}

fn sky_follows_game(mut sim: ResMut<nvf_play::sim::SimState>, mut sky: ResMut<sky::SkyState>, args: Res<Args>, mut shown: Local<Option<f32>>) {
    // The weather lists' gating globals as the scripts set them (VStoryEventHooverDamBattleStarted).
    for id in sky.gate_globals() {
        if let Ok(v) = sim.world.global(fnv_sim::FormId(id)) {
            sky.set_live_global(id, v);
        }
    }
    if sky.time_scale > 0.0 || args.still_sky {
        return;
    }
    let set_by_hand = shown.is_none_or(|h| (h - sky.hour).abs() > 1e-4);
    if set_by_hand {
        if let Some((id, _)) = sim.world.forms.global_by_edid(fnv_sim::clock::GAME_HOUR) {
            if let Err(e) = sim.world.set_global(id, sky.hour) {
                warn!("game clock: {e}");
            }
        }
    } else if let Ok(c) = sim.world.calendar() {
        sky.follow_game_clock(c.hour, c.days_passed);
    }
    *shown = Some(sky.hour);
}

fn write_frame_json(out: &std::path::Path, image: &std::path::Path, args: &Args, sky: &sky::SkyState) -> std::io::Result<()> {
    let abs = std::path::absolute(image)?;
    let doc = serde_json::json!({
        "schema": "fnv.frame/1",
        "image": abs.to_string_lossy(),
        "size": args.size,
        "camera": {"pos": args.pos, "eye_height": args.eye_height, "heading": args.heading, "pitch": args.pitch, "fov": args.fov},
        "world": args.world,
        "hour": sky.hour,
        "weather": sky.weather.edid,
    });
    fnv_install::guard_write(out, None).map_err(std::io::Error::other)?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, serde_json::to_vec_pretty(&doc)?)
}

/// With --time-scale: the clock starts once the cells around the start are loaded; then
/// one log line per whole game hour, and with --timelapse a screenshot every
/// --timelapse-step game minutes.
#[allow(clippy::too_many_arguments)]
fn timelapse(
    mut last: Local<Option<i32>>,
    mut seen: Local<u32>,
    mut last_slot: Local<Option<i64>>,
    mut frames: Local<u32>,
    mut sky: ResMut<sky::SkyState>,
    streamer: Res<Streamer>,
    args: Res<Args>,
    mut commands: Commands,
) {
    if sky.time_scale <= 0.0 || sky.interior.is_some() {
        return;
    }
    if !sky.clock_running {
        let side = (2 * streamer.radius + 1) as usize;
        if streamer.loaded.len() >= side * side {
            sky.clock_running = true;
            info!("time-lapse starts: {}", sky.sample());
        }
        return;
    }
    if let Some(dir) = &args.timelapse {
        // Slots of --timelapse-step minutes from midnight; the hour wraps at 24, so a change
        // of slot (not an increase) marks a new frame. Waits for the first slot boundary.
        let step = args.timelapse_step.max(0.5) as f64;
        let slot = (sky.hour as f64 * 60.0 / step).floor() as i64;
        if *last_slot != Some(slot) {
            if last_slot.is_some() {
                *frames += 1;
                let minutes = (slot as f64 * step).round() as i64;
                let name = format!("{:04}_{:02}{:02}.png", *frames, minutes / 60 % 24, minutes % 60);
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(dir.join(name)));
            }
            *last_slot = Some(slot);
        }
    }
    let hour = sky.hour.floor() as i32;
    if *last == Some(hour) {
        return;
    }
    let first = last.is_none();
    *last = Some(hour);
    if first {
        return; // Wait for the first whole hour.
    }
    *seen += 1;
    info!("time-lapse {}  ({} hours passed)", sky.sample(), *seen);
}

/// The `--run-seconds` limit's clocks: the virtual and the real one, and how long the
/// world test aids were held back (`nvf_play::pause::WorldAidClock`).
type RunClocks<'w> = (Res<'w, Time<Virtual>>, Res<'w, Time<Real>>, Option<Res<'w, nvf_play::pause::WorldAidClock>>, Option<Res<'w, nvf_play::loading::Loading>>);

/// Time spent advancing single clips (animated models; actors are timed by
/// `locomotion::ActorAnimCost`), for the `--run-seconds` summary.
#[derive(Resource, Default)]
struct AnimCost {
    total: std::time::Duration,
    frames: u32,
    /// Clips advanced, summed over frames, and the most at once.
    clip_frames: u64,
    peak: usize,
    /// Nodes switched between shown and hidden by the clips' visibility tracks (`Playing::show`).
    switched: u64,
}

fn advance_animations(
    time: Res<Time>,
    playing: Query<&mut nvf_assets::anim_play::Playing>,
    transforms: Query<&mut Transform>,
    visibility: Query<&mut Visibility>,
    mut cost: ResMut<AnimCost>,
) {
    let t0 = std::time::Instant::now();
    let n = playing.iter().len();
    cost.switched += nvf_assets::anim_play::advance_instances(time, playing, transforms, visibility) as u64;
    cost.total += t0.elapsed();
    cost.frames += 1;
    cost.clip_frames += n as u64;
    cost.peak = cost.peak.max(n);
}

/// --screenshot: wait for the cells around the start and for pipelines and textures, save,
/// exit.
#[allow(clippy::too_many_arguments)]
fn screenshot_and_exit(
    mut frames: Local<u32>,
    mut elapsed: Local<f32>,
    mut ready_at: Local<Option<f32>>,
    time: Res<Time>,
    args: Res<Args>,
    streamer: Res<Streamer>,
    models: Res<ModelCache>,
    textures: Res<TextureCache>,
    sky: Res<sky::SkyState>,
    lod: Res<lod::LodState>,
    mut commands: Commands,
) {
    *frames += 1;
    *elapsed += time.delta_secs();
    let side = (2 * streamer.radius + 1) as usize;
    let want = if streamer.interior.is_some() { 1 } else { side * side };
    if ready_at.is_none() && streamer.loaded.len() >= want && lod.gen_pending() == 0 {
        *ready_at = Some(*elapsed);
        let s = &streamer.stats;
        info!(
            "cells loaded in {:.2}s ({:.0} ms loading): refs drawn {} (animated {}, doors {}, with named sequences {}, posed {}), disabled {}, not drawn {} {:?}, no model {} (error markers {}), shapes {}",
            *elapsed, s.load_ms, s.refs_drawn, s.refs_animated, s.refs_doors, s.refs_group_rigs, s.refs_posed, s.refs_skipped_disabled, s.refs_skipped_type, s.skipped_types, s.refs_no_model, s.refs_error_marker, s.shapes
        );
    }
    let Some(r) = *ready_at else { return };
    // Pipelines compile during the first seconds; give them time after the last load.
    if *frames < 10_000 && *elapsed >= r + args.screenshot_delay.max(0.0) {
        if let Some(path) = &args.screenshot {
            for f in models.failures.iter().take(20) {
                warn!("model: {f}");
            }
            for f in textures.failures.iter().take(20) {
                warn!("texture: {f}");
            }
            info!("models failed {}, textures failed {}", models.failures.len(), textures.failures.len());
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
            if let Some(fj) = &args.frame_json {
                if let Err(e) = write_frame_json(fj, path, &args, &sky) {
                    error!("frame json {}: {e}", fj.display());
                }
            }
        }
        *frames = 10_000;
        *ready_at = Some(*elapsed);
    } else if *frames >= 10_000 && *elapsed >= r + 3.0 {
        commands.write_message(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::{restart_args, start_days, vertical_fov, with_default_mode, with_schemas_beside, Args};
    use clap::Parser;

    fn parse(a: &[&str]) -> Args {
        let line: Vec<String> = std::iter::once("nvforever").chain(a.iter().copied()).map(String::from).collect();
        Args::try_parse_from(with_default_mode(line)).unwrap()
    }

    #[test]
    fn no_arguments_open_the_main_menu() {
        for a in [&[][..], &["--data", "D"], &["--data=D", "--run-seconds", "20", "--size", "1600x900", "--no-music"]] {
            let args = parse(a);
            assert!(args.play && args.menu && !args.explore, "{a:?}");
        }
    }

    #[test]
    fn world_and_mode_arguments_keep_their_meaning() {
        let args = parse(&["--cell", "Goodsprings"]);
        assert!(!args.play && !args.menu && !args.explore);
        assert_eq!(args.cell, "Goodsprings");
        let args = parse(&["--pos=-1,2,3"]);
        assert!(!args.play && !args.menu);
        let args = parse(&["--explore"]);
        assert!(args.explore && !args.play && !args.menu);
        let args = parse(&["--cell", "Goodsprings", "--play"]);
        assert!(args.play && !args.menu);
        let args = parse(&["--play", "--new-game"]);
        assert!(args.play && args.new_game && !args.menu);
        let args = parse(&["--play", "--load", "quicksave"]);
        assert!(args.play && !args.menu && args.load.is_some());
        let args = parse(&["--play", "--menu"]);
        assert!(args.play && args.menu);
        assert!(Args::try_parse_from(["nvforever", "--explore", "--play"]).is_err());
    }

    #[test]
    fn a_refused_load_is_told_on_the_main_menu_only() {
        assert_eq!(parse(&["--play", "--menu", "--load-refused=old: why"]).load_refused.as_deref(), Some("old: why"));
        assert!(Args::try_parse_from(["nvforever", "--play", "--load-refused=old: why"]).is_err());
    }

    #[test]
    fn a_saves_day_starts_the_sky_and_not_the_game() {
        // A save's day, over --day for the sky, never in the game's start clock (#1607).
        assert_eq!(start_days(None, Some(25.375), Some(5.0)), (5.0, 25.0));
        assert_eq!(start_days(Some(30.0), Some(25.375), Some(5.0)), (30.0, 25.0));
        // A save without the day, and a new game: both on --day, else the plugins' day.
        assert_eq!(start_days(Some(30.5), None, Some(5.0)), (30.0, 30.0));
        assert_eq!(start_days(None, None, Some(5.5)), (5.0, 5.0));
        assert_eq!(start_days(None, None, None), (0.0, 0.0));
    }

    #[test]
    fn schemas_beside_the_program() {
        let dir = std::env::temp_dir().join(format!("nvf-schemas-beside-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("schemas/fnv")).unwrap();
        let line = with_schemas_beside(vec!["nvforever".into()], &dir);
        assert_eq!(line, ["nvforever".to_string(), "--esm-schema".into(), dir.join("schemas/fnv").display().to_string()]);
        let given = vec!["nvforever".to_string(), "--esm-schema=X".into()];
        assert_eq!(with_schemas_beside(given.clone(), &dir), given);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_restart_for_a_load_drops_what_the_save_decides() {
        let a: Vec<String> = ["--data", "D", "--cell", "Goodsprings", "--play", "--menu", "--pos=-1,2,3", "--new-game", "--hour", "9", "--no-music", "--load", "old", "--load-refused=old: why", "--size", "1600x900", "--window-at=-8,0", "--window-maximized", "--handoff", "f"].map(String::from).to_vec();
        assert_eq!(restart_args(&a), ["--data", "D", "--play", "--no-music"]);
    }

    #[test]
    fn fov_setting_is_horizontal_at_4_3() {
        let v = vertical_fov(75.0);
        let horizontal = |aspect: f32| 2.0 * ((v / 2.0).tan() * aspect).atan().to_degrees();
        assert!((horizontal(4.0 / 3.0) - 75.0).abs() < 1e-3);
        assert!((v.to_degrees() - 59.84).abs() < 0.01);
        // The 16:9 angle the reference frames matched (docs/spec-notes/camera.md).
        assert!((horizontal(16.0 / 9.0) - 91.31).abs() < 0.01);
    }
}
