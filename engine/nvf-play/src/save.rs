//! Saving and loading (docs/design/save-load.md): NVForever's own save files
//! (`fnv_save`), never the original's `.fos`.
//!
//! - **Saving** takes the game snapshot (`fnv_game::GameSnapshot`, the simulation's inside
//!   it) and the engine's own state ([`EngineSnapshot`]) at the end of a frame, after the
//!   script frame's effects have reached the world, so nothing a script asked for is
//!   still queued (design §5 q1). It is allowed in the world, from the pause menu and from
//!   the console; not in a conversation, another menu, a script's message box or under a
//!   loading screen. Requests: [`SaveRequest`] (the console's `save <name>`), the
//!   QuickSave key (F5, slot `quicksave`) and autosaves (slot `autosave`): after each
//!   travel behind a loading screen (a fast travel has its own switch), after a wait and
//!   after a sleep that passed at least an hour (`crate::wait::RestEnded`), and when a
//!   script calls `Autosave` [documented: GECK wiki Autosave], each behind a switch on the
//!   settings page. A script's `SystemSave`
//!   writes the one system slot (`systemsave`), overwritten by each later call; it waits
//!   for a moment saving is allowed, as autosaves do [documented: GECK wiki SystemSave;
//!   the waiting is a hypothesis]. A script's `ForceSave` is a save by hand under a new
//!   name (`Save <n>`, as the pause menu names one) and fails, as one by hand does, when
//!   saving is not allowed [documented: GECK wiki ForceSave]. Both are behind the
//!   scripts' autosave switch. A rolling slot keeps its previous file as `.bak`.
//! - **Loading** starts the world from the plugins at the saved place and applies the save
//!   before the scripts' first frame: nvforever `--load <name|path>` reads the header,
//!   starts there, and hands the body over as [`PendingLoad`]; `crate::script` restores the
//!   game and the simulation when its runtime is made and sends `RefChanged` (marked
//!   `restored`) for every reference the save changed, so the drawn world follows it;
//!   [`apply_engine`] then puts back the engine's part. A save that cannot be restored is
//!   refused as it is read ([`read_for_load`]: nvforever opens the main menu and says why),
//!   before anything starts from it; were a restore still to fail, [`LoadRefused`] ends the
//!   same way. Loading while playing ([`LoadRequest`]: the console's
//!   `load <name>`, the QuickLoad key F9) is nvforever's: it starts itself again with
//!   `--load`, so the save always lands in a world built fresh from the plugins.
//! - Not saved (design §3): physics in motion (objects load at rest), menus, music and
//!   radio playback position, AI agents' patrol and sandbox progress (packages start
//!   again from where each actor stands).
//! - **Thumbnail** (design §5 q8): a 512×288 PNG of the main camera's picture, for the load
//!   menu (`crate::pause`). The snapshot is still taken at the end of the frame the save
//!   was asked in; the render world captures the main 3D view's post-processed texture in
//!   `Core3d` after `Core3dSystems::PostProcess` and before `ui_pass`, so the thumbnail
//!   has no HUD or menus and no UI node is hidden or blinks on screen (#3808). When the
//!   picture arrives (a frame or two later), a thread crops it to 16:9, scales it down by
//!   area averaging, encodes the PNG and writes the file, so the frame does not wait for
//!   the encoder or the disk. No picture within [`SHOT_WAIT_FRAMES`] frames: the save is
//!   written without one. A load or quit waits for the saves in flight ([`SaveWrites`]).
//!
//! - **A co-op guest's own game**: a guest joining another player's world
//!   keeps its own game as it was then ([`KeepSolo`] asks, [`keep_solo`] takes it into
//!   [`SoloBase`] before the guest is moved). Each save made while it is kept writes that
//!   game with the character as it is now (`GameSnapshot::with_character_of`): the place,
//!   the world, quests and time are the guest's own, the inventory, values, level, XP and
//!   perks are what it has now. So a guest loads back into its own game where it left it,
//!   with what it gained in the other world. The engine's part is its own game's too,
//!   apart from the view, sneak, run, flashlight and the magazine in hand. Outside co-op
//!   nothing asks for it, and saves are written as before [design].
//!
//! The random numbers are not restored as saved: a load re-seeds them, so a reload does
//! not replay the same rolls (design §5 q3); test aid `NVF_PLAY_TEST_SAVE_SAME_ROLLS=1`
//! keeps the saved positions.
//!
//! Log: `save: wrote <path> (<kind>, <bytes> bytes, <n> changed references), thumbnail
//! 512x288 (<bytes> bytes PNG) in <ms> ms (snapshot <ms> ms)` (`no thumbnail` without one),
//! `save: not now: <why>`, `load: restored <path>: …`.

use fnv_util::words::{tr, trf};
use crate::script::ScriptState;
use crate::sim::SimState;
use crate::{Location, PlaySet, PlayState, PlayWorld};
use bevy::camera::{CameraOutputMode, NormalizedRenderTarget};
use bevy::core_pipeline::{
    blit::{BlitPipeline, BlitPipelineKey},
    Core3d, Core3dSystems,
};
use bevy::prelude::*;
use bevy::render::{
    camera::ExtractedCamera,
    render_resource::{
        Buffer, BufferDescriptor, BufferUsages, Extent3d, MapMode, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, SpecializedRenderPipelines, TexelCopyBufferInfo,
        TexelCopyBufferLayout, Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    },
    renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, ViewQuery},
    sync_world::MainEntity,
    view::{ExtractedWindow, ResolvedCompositingSpace, ViewTarget},
    Render, RenderApp, RenderSystems,
};
use bevy::window::PrimaryWindow;
use fnv_save::{PluginEntry, SaveHeader, SaveKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, AtomicU8, Ordering},
    mpsc, Arc, Mutex,
};

mod death_recovery;

pub const BODY_FORMAT: &str = "nvf.save.body/1";
pub const ENGINE_FORMAT: &str = "nvf.engine.state/1";

/// The thumbnail's size (design §5 q8).
pub const THUMB_W: u32 = 512;
pub const THUMB_H: u32 = 288;
/// Frames to wait for the window's picture before a save is written without a thumbnail.
pub const SHOT_WAIT_FRAMES: u32 = 30;

/// Save now (at the end of this frame, when saving is allowed).
#[derive(Message, Debug, Clone)]
pub struct SaveRequest {
    /// The save's name; ignored for the rolling slots (quick, auto).
    pub name: String,
    pub kind: SaveKind,
}

/// Load this save: nvforever starts again with `--load` (module notes).
#[derive(Message, Debug, Clone)]
pub struct LoadRequest {
    pub path: PathBuf,
}

/// Where saves go and the autosave switches.
#[derive(Resource, Debug, Clone)]
pub struct SaveConfig {
    /// `fnv_install::EngineHome::saves` (`Documents\My Games\NVForever\saves`).
    pub dir: Option<PathBuf>,
    pub autosave_travel: bool,
    pub autosave_fast_travel: bool,
    pub autosave_wait: bool,
    pub autosave_sleep: bool,
    pub autosave_scripts: bool,
}

impl Default for SaveConfig {
    fn default() -> Self {
        SaveConfig::with_dir(fnv_install::EngineHome::default_root().map(|r| fnv_install::EngineHome::new(r).saves()))
    }
}

impl SaveConfig {
    /// Saves in `dir`, every autosave switch on.
    pub fn with_dir(dir: Option<PathBuf>) -> SaveConfig {
        SaveConfig { dir, autosave_travel: true, autosave_fast_travel: true, autosave_wait: true, autosave_sleep: true, autosave_scripts: true }
    }

    /// The file of a save by name, or a path given as is (it has a folder or the
    /// extension).
    pub fn path_of(&self, name: &str) -> Result<PathBuf, String> {
        let p = PathBuf::from(name);
        if p.components().count() > 1 || name.ends_with(&format!(".{}", fnv_save::EXTENSION)) {
            return Ok(p);
        }
        let dir = self.dir.as_ref().ok_or("no NVForever home folder (set NVFOREVER_HOME)")?;
        fnv_save::path_of(dir, name).map_err(|e| e.to_string())
    }
}

/// A part of the engine's state another crate keeps (nvforever's sky): saved under `key`
/// in [`EngineSnapshot::extra`] and put back on load.
#[derive(Clone, Copy)]
pub struct SaveHook {
    pub key: &'static str,
    pub save: fn(&World) -> Option<serde_json::Value>,
    pub load: fn(&mut World, &serde_json::Value),
    /// For a save made with another load order (design §4.1): move the FormIDs the part
    /// holds to the current load order ([`fnv_save::PluginRemap::map`]; `None` for a
    /// plugin no longer loaded). `None` for a part that holds none.
    pub remap: Option<fn(&mut serde_json::Value, &fnv_save::PluginRemap)>,
}

#[derive(Resource, Default, Clone)]
pub struct SaveHooks(pub Vec<SaveHook>);

/// Register a [`SaveHook`].
pub fn add_save_hook(app: &mut App, h: SaveHook) {
    app.init_resource::<SaveHooks>();
    app.world_mut().resource_mut::<SaveHooks>().0.push(h);
}

/// The engine's own part of a save (design §3.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EngineSnapshot {
    pub format: String,
    #[serde(default)]
    pub third_person: bool,
    /// The third-person camera's distance.
    #[serde(default)]
    pub distance: f32,
    #[serde(default)]
    pub sneak: bool,
    #[serde(default)]
    pub always_run: bool,
    #[serde(default)]
    pub flashlight: bool,
    /// The weapon in hand and the rounds in its magazine.
    #[serde(default)]
    pub magazine: Option<(u32, u32)>,
    /// The type of round in that magazine (an Ammo Swap's pick; #1574). Absent in older
    /// saves: the weapon's default pick, as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magazine_ammo: Option<u32>,
    /// Player's remembered ammo type per weapon base form for the Keep Chosen Ammo tweak
    /// (#3305). Absent in older saves: empty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub chosen_ammo: BTreeMap<u32, u32>,
    /// The Pip-Boy's active quest.
    #[serde(default)]
    pub active_quest: Option<u32>,
    /// Parts kept by other crates ([`SaveHook`]).
    #[serde(default)]
    pub extra: BTreeMap<String, serde_json::Value>,
    /// The same part of players 2–4 of a co-op game (docs/design/coop-save.md); absent
    /// for one player.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coop: Vec<CoopEngine>,
}

/// The engine's part of one of players 2–4 (docs/design/coop-save.md): the fields of
/// [`EngineSnapshot`] that belong to one player. The Pip-Boy's active quest and the other
/// crates' parts stay shared.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CoopEngine {
    /// The player's number, 2–4 (`fnv_game::coop::CoopPlayer::slot`).
    pub slot: u8,
    #[serde(default)]
    pub third_person: bool,
    #[serde(default)]
    pub distance: f32,
    #[serde(default)]
    pub sneak: bool,
    #[serde(default)]
    pub always_run: bool,
    #[serde(default)]
    pub flashlight: bool,
    #[serde(default)]
    pub magazine: Option<(u32, u32)>,
}

/// Players 2–4's engine part: kept from the save loaded and written into the next one
/// (nothing plays them yet; the game layer keeps the rest, `fnv_game::Game::coop`).
#[derive(Resource, Debug, Clone, Default)]
pub struct CoopEngineState(pub Vec<CoopEngine>);

/// A save's body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveBody {
    pub format: String,
    pub game: fnv_game::game::GameSnapshot,
    pub engine: EngineSnapshot,
}

impl SaveBody {
    /// Whether the runtime can restore this body: every format in it, and all that the
    /// snapshots' own restores check.
    fn check(&self) -> Result<(), String> {
        if self.format != BODY_FORMAT {
            return Err(format!("body format {:?}, this NVForever reads {BODY_FORMAT}", self.format));
        }
        if self.engine.format != ENGINE_FORMAT {
            return Err(format!("engine format {:?}, this NVForever reads {ENGINE_FORMAT}", self.engine.format));
        }
        self.game.check()
    }
}

/// A save [`read_for_load`] let through that the runtime could not restore, and why:
/// nvforever starts again on the main menu and says why, as for a save refused when it was
/// read [owner 2026-11-01, #1357].
#[derive(Message, Debug, Clone)]
pub struct LoadRefused(pub String);

/// The save to apply as the game starts (nvforever `--load`).
#[derive(Resource)]
pub struct PendingLoad {
    pub path: PathBuf,
    pub body: SaveBody,
    /// The save was made with another load order ([`read_for_load`]).
    pub order: Option<OrderChange>,
}

/// How a save's load order differs from the current one, once its body was remapped.
#[derive(Debug, Clone)]
pub struct OrderChange {
    pub remap: fnv_save::PluginRemap,
    /// The save's plugins no longer loaded.
    pub missing: Vec<String>,
}

/// Co-op: keep this game as it is now, as the guest's own ([`keep_solo`]),
/// once it runs; coop.rs asks for it as a guest joins another's world.
#[derive(Resource, Default)]
pub struct KeepSolo;

/// A co-op guest's own game as it was when it joined another's world (module notes);
/// `None` when it could not be taken (the scripts do not run). While it is here, saves
/// write it with the character as it is now.
#[derive(Resource)]
pub struct SoloBase(pub Option<Box<Solo>>);

pub struct Solo {
    pub game: fnv_game::game::GameSnapshot,
    pub engine: EngineSnapshot,
    /// The header's place, location and game date then.
    pub header: SaveHeader,
}

/// The engine part waiting for the runtime to be restored.
#[derive(Resource)]
pub struct PendingEngine(pub EngineSnapshot, pub PathBuf, pub Option<OrderChange>);

/// Whether the save given at the start (`--load`) is still being put back (scripts that
/// cannot run never put it back: not waited for then).
#[derive(bevy::ecs::system::SystemParam)]
pub struct Restoring<'w> {
    engine: Option<Res<'w, PendingEngine>>,
    script: Option<Res<'w, ScriptState>>,
}

impl Restoring<'_> {
    pub fn busy(&self) -> bool {
        self.engine.is_some() && self.script.as_ref().is_none_or(|s| s.off.is_none())
    }
}

/// The loading screen up now is a fast travel's (`crate::pipboy` sets it as the trip
/// leaves): the autosave after it follows the fast-travel switch.
#[derive(Resource, Default)]
pub struct FastTravelLanding(pub bool);

/// Saves asked for and not yet written (one waits while saving is not allowed).
#[derive(Resource, Default)]
struct SaveQueue(Vec<SaveRequest>);

/// The outcome of each save and load, for the console.
#[derive(Resource, Default)]
pub struct SaveLog(pub Vec<String>);

/// A save written (or failed), for the pause menu's status line.
#[derive(Message, Debug, Clone)]
pub struct SaveDone {
    pub path: PathBuf,
    pub result: Result<(), String>,
}

/// A save whose snapshot is taken, waiting for its picture.
struct Unwritten {
    path: PathBuf,
    header: SaveHeader,
    body: Vec<u8>,
    kind: SaveKind,
    refs: usize,
    t0: std::time::Instant,
    snapshot_ms: f64,
}

/// A file written on the writer thread: body and thumbnail bytes, or why not.
struct Written {
    path: PathBuf,
    kind: SaveKind,
    result: Result<(usize, usize), String>,
    refs: usize,
    t0: std::time::Instant,
    snapshot_ms: f64,
}

/// Render-world `Core3d` system set that captures the pre-UI save thumbnail after
/// `Core3dSystems::PostProcess` and before `ui_pass`. Downstream post-UI/display passes
/// such as `nvforever::hdr::hdr_expand` order `.after(SaveShotSystems)` so SDR thumbnails
/// capture the pre-expansion `[0, 1]` tonemapped frame deterministically [design, #3808].
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SaveShotSystems;

/// The pre-UI world picture as captured for a save thumbnail.
struct Shot {
    id: u64,
    w: u32,
    h: u32,
    bgra: bool,
    px: Vec<u8>,
}

/// Shared main-world / render-world bridge for capturing a pre-UI save thumbnail without
/// hiding any UI node (#3808). `requested` holds `0` when idle or the monotonic non-zero
/// request ID of the active capture.
#[derive(Resource, Clone)]
struct ShotBridge {
    requested: Arc<AtomicU64>,
    tx: mpsc::Sender<Shot>,
}

/// Saves between their snapshot and their file: waiting for the pre-UI picture, or being
/// written on another thread.
#[derive(Resource)]
pub struct SaveWrites {
    waiting: Vec<Unwritten>,
    /// The frame and request ID the picture was asked for, until it arrives or times out.
    asked: Option<(u32, u64)>,
    next_shot_id: u64,
    shot: Option<Shot>,
    /// Signal to the render world carrying the active non-zero capture request ID (`0` idle) (#3808).
    shot_req: Arc<AtomicU64>,
    /// Channel delivering completed pre-UI thumbnail captures from the render world.
    shot_rx: Mutex<mpsc::Receiver<Shot>>,
    threads: Vec<std::thread::JoinHandle<()>>,
    in_flight: usize,
    tx: mpsc::Sender<Written>,
    rx: Mutex<mpsc::Receiver<Written>>,
}

impl Default for SaveWrites {
    fn default() -> Self {
        Self::new_with_bridge().0
    }
}

impl SaveWrites {
    fn new_with_bridge() -> (Self, ShotBridge) {
        let (tx, rx) = mpsc::channel();
        let (shot_tx, shot_rx) = mpsc::channel();
        let shot_req = Arc::new(AtomicU64::new(0));
        let bridge = ShotBridge { requested: shot_req.clone(), tx: shot_tx };
        let writes = SaveWrites {
            waiting: Vec::new(),
            asked: None,
            next_shot_id: 1,
            shot: None,
            shot_req,
            shot_rx: Mutex::new(shot_rx),
            threads: Vec::new(),
            in_flight: 0,
            tx,
            rx: Mutex::new(rx),
        };
        (writes, bridge)
    }

    /// Whether a save is not on disk yet (a load waits for it).
    pub fn busy(&self) -> bool {
        !self.waiting.is_empty() || self.in_flight > 0
    }
}

pub struct SavePlugin;

impl Plugin for SavePlugin {
    fn build(&self, app: &mut App) {
        let (writes, bridge) = SaveWrites::new_with_bridge();
        app.init_resource::<SaveConfig>()
            .init_resource::<SaveHooks>()
            .init_resource::<SaveQueue>()
            .init_resource::<SaveLog>()
            .insert_resource(writes)
            .init_resource::<CoopEngineState>()
            .init_resource::<FastTravelLanding>()
            .init_resource::<death_recovery::RecoveryPolicy>()
            .init_resource::<death_recovery::DeathRecovery>()
            .add_message::<crate::wait::RestEnded>()
            .add_message::<SaveRequest>()
            .add_message::<SaveDone>()
            .add_message::<LoadRequest>()
            .add_message::<LoadRefused>()
            .add_systems(Update, (requests, autosaves).in_set(PlaySet::Ui))
            .add_systems(Update, (death_recovery::update, write_saves, take_shot, finish_writes, apply_engine, keep_solo).chain().after(PlaySet::Sync))
            .add_systems(Last, flush_on_exit);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(bridge)
                .init_resource::<RenderSaveShot>()
                .add_systems(Render, prepare_save_shot.in_set(RenderSystems::Prepare).ambiguous_with_all())
                .add_systems(Core3d, capture_save_shot.in_set(SaveShotSystems).after(Core3dSystems::PostProcess).before(bevy::ui_render::ui_pass))
                .add_systems(RenderGraph, finish_save_shot.in_set(RenderGraphSystems::Finish));
        }
        if let Some(p) = app.world_mut().remove_resource::<PendingLoad>() {
            let PendingLoad { path, body, order } = p;
            app.insert_resource(PendingEngine(body.engine, path.clone(), order));
            app.insert_resource(crate::script::PendingGame(Box::new(body.game), path));
        }
        let setting = |key, label, note, get: fn(&World) -> bool, set: fn(&mut World, bool)| crate::settings::Setting { key, group: GROUP, label, note, switches: "", get, set, slider: None };
        crate::settings::add_setting(
            app,
            setting("autosave-travel", "Autosave after travel", "On: the game saves to the autosave slot after each loading screen (doors, coc, a script moving you); fast travel has its own row.", |w| w.resource::<SaveConfig>().autosave_travel, |w, on| w.resource_mut::<SaveConfig>().autosave_travel = on),
        );
        crate::settings::add_setting(
            app,
            setting("autosave-fast-travel", "Save on fast travel", "On: the game saves to the autosave slot when a fast travel arrives.", |w| w.resource::<SaveConfig>().autosave_fast_travel, |w, on| w.resource_mut::<SaveConfig>().autosave_fast_travel = on),
        );
        crate::settings::add_setting(
            app,
            setting("autosave-wait", "Save on wait", "On: the game saves to the autosave slot when a wait ends with at least one hour passed.", |w| w.resource::<SaveConfig>().autosave_wait, |w, on| w.resource_mut::<SaveConfig>().autosave_wait = on),
        );
        crate::settings::add_setting(
            app,
            setting("autosave-sleep", "Save on sleep", "On: the game saves to the autosave slot when the player wakes from a sleep of at least one hour.", |w| w.resource::<SaveConfig>().autosave_sleep, |w, on| w.resource_mut::<SaveConfig>().autosave_sleep = on),
        );
        crate::settings::add_setting(
            app,
            setting("autosave-scripts", "Save when a script asks", "On: a script's Autosave saves to the autosave slot (quest milestones), SystemSave to the system slot, ForceSave as a new save.", |w| w.resource::<SaveConfig>().autosave_scripts, |w, on| w.resource_mut::<SaveConfig>().autosave_scripts = on),
        );
    }
}

const GROUP: &str = "Saving";

/// The QuickSave and QuickLoad keys and the [`SaveRequest`]s.
#[allow(clippy::too_many_arguments)]
fn requests(input: crate::bindings::ControlInput, state: Res<State<PlayState>>, mut asked: MessageReader<SaveRequest>, mut queue: ResMut<SaveQueue>, cfg: Res<SaveConfig>, mut load: MessageWriter<LoadRequest>, mut log: ResMut<SaveLog>, tweaks: Option<Res<crate::tweaks::TweakSettings>>) {
    queue.0.extend(asked.read().cloned());
    // The player's tweak: QuickLoad works in the main and pause menus too [design: docs/design/tweaks.md].
    let menu_load = tweaks.is_some_and(|t| t.quick_load_in_menus) && *state.get() == PlayState::Menu;
    if *state.get() != PlayState::Playing && !menu_load {
        return;
    }
    if !menu_load && input.just_pressed(crate::bindings::Control::QuickSave) {
        info!("save: QuickSave key");
        queue.0.push(SaveRequest { name: fnv_save::QUICK_NAME.into(), kind: SaveKind::Quick });
    }
    if input.just_pressed(crate::bindings::Control::QuickLoad) {
        match cfg.path_of(fnv_save::QUICK_NAME) {
            Ok(p) if p.exists() => {
                info!("load: QuickLoad key: {}", p.display());
                load.write(LoadRequest { path: p });
            }
            Ok(_) => log.0.push(tr("No quicksave yet").into()),
            Err(e) => log.0.push(e),
        }
    }
}

/// Autosaves: after a loading screen comes down, after a wait or a sleep, and on a
/// script's `Autosave`; a script's `SystemSave` and `ForceSave`.
#[allow(clippy::too_many_arguments)]
fn autosaves(
    loading: Res<crate::loading::Loading>,
    mut was: Local<bool>,
    mut fast: ResMut<FastTravelLanding>,
    mut rested: MessageReader<crate::wait::RestEnded>,
    script: Option<ResMut<ScriptState>>,
    cfg: Res<SaveConfig>,
    mut queue: ResMut<SaveQueue>,
    pending: Option<Res<PendingEngine>>,
) {
    let auto = || SaveRequest { name: fnv_save::AUTO_NAME.into(), kind: SaveKind::Auto };
    let active = loading.travelling();
    // A loading screen comes up only for travel (the start has none); not while a save
    // is still being put back.
    if *was && !active && pending.is_none() {
        let fast_travel = std::mem::take(&mut fast.0);
        if fast_travel && cfg.autosave_fast_travel {
            info!("save: autosave after fast travel");
            queue.0.push(auto());
        } else if !fast_travel && cfg.autosave_travel {
            info!("save: autosave after travel");
            queue.0.push(auto());
        }
    }
    *was = active;
    for r in rested.read() {
        if (r.sleep && cfg.autosave_sleep) || (!r.sleep && cfg.autosave_wait) {
            info!("save: autosave after {} ({} hour(s))", if r.sleep { "sleep" } else { "wait" }, r.hours);
            queue.0.push(auto());
        }
    }
    let Some(mut st) = script else { return };
    let Some(rt) = st.rt.as_mut() else { return };
    let n = std::mem::take(&mut rt.game.ui.autosaves);
    if n > 0 && cfg.autosave_scripts {
        info!("save: autosave asked by a script");
        queue.0.push(auto());
    }
    let system = std::mem::take(&mut rt.game.ui.system_saves);
    let force = std::mem::take(&mut rt.game.ui.force_saves);
    if !cfg.autosave_scripts {
        return;
    }
    if system > 0 {
        info!("save: SystemSave asked by a script");
        queue.0.push(SaveRequest { name: fnv_save::SYSTEM_NAME.into(), kind: SaveKind::System });
    }
    if force > 0 {
        let name = crate::pause::new_save_name(&cfg);
        info!("save: ForceSave asked by a script: {name:?}");
        queue.0.push(SaveRequest { name, kind: SaveKind::Manual });
    }
}

/// Why saving is refused from the player's death on (#1572).
const DEAD: &str = "the player is dead";

/// The player is dead (the death camera has the view until a load).
fn player_dead(world: &World) -> bool {
    world.get_resource::<ScriptState>().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.is_dead(fnv_sim::FormId::PLAYER_REF))
}

/// Why saving is not allowed now, if it is not.
pub fn not_now(world: &World) -> Option<String> {
    let st = world.get_resource::<ScriptState>()?;
    let Some(rt) = st.rt.as_ref() else { return Some("the scripts are not running yet".into()) };
    if world.resource::<crate::loading::Loading>().active() {
        return Some("a loading screen is up".into());
    }
    if player_dead(world) {
        return Some(DEAD.into());
    }
    // ToggleDisableSaves 1 (`fnv_game::nvse_jg7`): until every plugin that disabled saving
    // enables it again or a save is loaded [documented: ToggleDisableSaves].
    if rt.game.saves_disabled() {
        return Some("a script disabled saving".into());
    }
    if rt.game.dialogue.is_some() {
        return Some("in a conversation".into());
    }
    // A box answered and never read by its script is not on screen (#1649).
    if rt.game.menu.is_some() || rt.game.message_box.as_ref().is_some_and(|b| b.waiting()) {
        return Some("a script's menu or message box is open".into());
    }
    let state = *world.resource::<State<PlayState>>().get();
    let paused = world.resource::<crate::pause::Paused>().0;
    let console = world.get_resource::<crate::console::Console>().is_some_and(|c| c.open);
    let menu = world.resource::<crate::menu::ActiveMenu>().0.is_some();
    match state {
        PlayState::Playing => None,
        PlayState::Menu if (paused || console) && !menu => None,
        PlayState::Menu => Some("a menu is open".into()),
        PlayState::Dialogue => Some("in a conversation".into()),
        PlayState::Loading => Some("a loading screen is up".into()),
    }
}

fn write_saves(world: &mut World) {
    if world.resource::<SaveQueue>().0.is_empty() || world.contains_resource::<PendingEngine>() {
        return;
    }
    if let Some(why) = not_now(world) {
        // Autosaves and the system save wait for their moment (one of each); a save asked
        // for by hand says why not. A dead player's game is never saved: what waits is
        // dropped, so it is not written over the save loaded next.
        let mut q = world.resource_mut::<SaveQueue>();
        let (waits, asked): (Vec<_>, Vec<_>) = std::mem::take(&mut q.0).into_iter().partition(|r| matches!(r.kind, SaveKind::Auto | SaveKind::System));
        if why == DEAD && !waits.is_empty() {
            info!("save: {} waiting autosave(s) dropped: {why}", waits.len());
        }
        for r in waits.into_iter().filter(|_| why != DEAD) {
            if !q.0.iter().any(|k| k.kind == r.kind) {
                q.0.push(r);
            }
        }
        for r in asked {
            let line = format!("Cannot save {:?} now: {why}", r.name);
            info!("save: not now: {why}");
            world.resource_mut::<SaveLog>().0.push(line);
            say_in_the_corner(world, format!("Cannot save now: {why}"));
            // The pause menu's status line says it too, instead of "Saving..." for good.
            let path = world.resource::<SaveConfig>().path_of(&r.name).unwrap_or_default();
            world.write_message(SaveDone { path, result: Err(format!("not now: {why}")) });
        }
        return;
    }
    let reqs = std::mem::take(&mut world.resource_mut::<SaveQueue>().0);
    // One file per slot and name in a frame.
    let mut done: Vec<(SaveKind, String)> = Vec::new();
    for r in reqs {
        let key = (r.kind, r.name.clone());
        if done.contains(&key) {
            continue;
        }
        done.push(key);
        if let Err(e) = save_now(world, &r) {
            warn!("save: failed: {e}");
            say_in_the_corner(world, failed_note(r.kind, &e));
            world.resource_mut::<SaveLog>().0.push(format!("Save failed: {e}"));
            let path = world.resource::<SaveConfig>().path_of(&r.name).unwrap_or_default();
            world.write_message(SaveDone { path, result: Err(e) });
        }
    }
}

/// A save's game date and hour as the load list shows it (day.month.year hour:00).
pub fn game_date(h: &SaveHeader) -> String {
    format!("{:02}.{:02}.{} {:02.0}:00", h.game_day, h.game_month + 1, h.game_year, h.game_hour.floor())
}

/// The plugins of the running load order.
pub fn plugin_list(d: &crate::PlayData) -> Vec<PluginEntry> {
    d.lo.plugins.iter().map(|p| PluginEntry { name: p.name.clone(), size: p.file_size }).collect()
}

fn save_now(world: &mut World, r: &SaveRequest) -> Result<(), String> {
    let t0 = std::time::Instant::now();
    let cfg = world.resource::<SaveConfig>().clone();
    let name = match r.kind {
        SaveKind::Quick => fnv_save::QUICK_NAME,
        SaveKind::Auto => fnv_save::AUTO_NAME,
        SaveKind::System => fnv_save::SYSTEM_NAME,
        SaveKind::Manual => r.name.as_str(),
    };
    let path = cfg.path_of(name)?;
    let (mut game, story) = world.resource_scope(|world, mut sim: Mut<SimState>| {
        let mut st = world.resource_mut::<ScriptState>();
        let rt = st.rt.as_mut().ok_or("the scripts are not running")?;
        // The extender's SaveGame event, with the save's name [documented: NVSE Event
        // Handling], before the state is taken.
        crate::script::with_world(rt, &mut sim, |rt| {
            rt.game.dispatch_event("SaveGame", vec![fnv_script::vm::XVal::Str(name.to_string())], None);
            // The story to undo keeps the unique rewards of claimed quests
            // (coop-shared-progress.md §4.1).
            rt.game.checked_snapshot().map(|g| (g, rt.game.coop_story_to_undo()))
        })
    })?;
    // Test aid: this save reads as one from a later version (smoke `save-refused-load`).
    if std::env::var("NVF_PLAY_TEST_SAVE_UNKNOWN_FORMAT").is_ok_and(|n| n == name) {
        game.format = "fnv.game.state/2".into();
    }
    let engine = engine_snapshot(world);
    let mut header = header(world, r.kind, &game, &engine);
    // A co-op guest's save: its own game, with the character as it is now.
    let (game, engine) = match world.get_resource::<SoloBase>().and_then(|s| s.0.as_deref()) {
        Some(solo) => {
            let mut e = solo.engine.clone();
            (e.third_person, e.distance, e.sneak, e.always_run, e.flashlight, e.magazine) = (engine.third_person, engine.distance, engine.sneak, engine.always_run, engine.flashlight, engine.magazine);
            let h = &solo.header;
            (header.place, header.location, header.weather) = (h.place.clone(), h.location.clone(), h.weather.clone());
            (header.game_year, header.game_month, header.game_day, header.game_hour) = (h.game_year, h.game_month, h.game_day, h.game_hour);
            info!("save: a co-op guest's save: its own game at {} with the character as it is now", h.location);
            // What the host's story gave or took is not part of the character: taken off,
            // but for the unique rewards of the stages this character claimed
            // (docs/design/coop-save.md §5).
            let mut now = game;
            let undone = story.undo_on(&mut now);
            if !undone.is_empty() {
                info!("save: the host's story taken off the guest's save: {}", undone.join(", "));
            }
            (solo.game.clone().with_character_of(&now), e)
        }
        None => (game, engine),
    };
    let refs = game.sim.refs.len();
    let body = SaveBody { format: BODY_FORMAT.into(), game, engine };
    let bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    let snapshot_ms = t0.elapsed().as_secs_f64() * 1e3;
    world.resource_mut::<SaveWrites>().waiting.push(Unwritten { path, header, body: bytes, kind: r.kind, refs, t0, snapshot_ms });
    Ok(())
}

/// Ask the render world for this frame's pre-UI world picture for the saves just taken.
/// Never hides any UI root node, so the HUD and menus do not blink during a save (#3808).
fn take_shot(world: &mut World) {
    let frame = world.resource::<bevy::diagnostic::FrameCount>().0;
    let mut w = world.resource_mut::<SaveWrites>();
    if w.waiting.is_empty() || w.asked.is_some() {
        return;
    }
    // Discard any late picture from an earlier timed-out request.
    if let Ok(rx) = w.shot_rx.lock() {
        while rx.try_recv().is_ok() {}
    }
    let id = w.next_shot_id;
    w.next_shot_id = id.wrapping_add(1).max(1);
    w.asked = Some((frame, id));
    w.shot = None;
    w.shot_req.store(id, Ordering::Release);
}

/// Once the pre-UI picture arrives (or did not come in time), hand the saves to a writer
/// thread and report the files written.
fn finish_writes(world: &mut World) {
    let frame = world.resource::<bevy::diagnostic::FrameCount>().0;
    let mut w = world.resource_mut::<SaveWrites>();
    if let Some((_, req_id)) = w.asked {
        if w.shot.is_none() {
            let received = w.shot_rx.lock().ok().and_then(|rx| {
                while let Ok(shot) = rx.try_recv() {
                    if shot.id == req_id {
                        return Some(shot);
                    }
                }
                None
            });
            if let Some(shot) = received {
                w.shot = Some(shot);
            }
        }
    }
    let ready = w.asked.is_some_and(|(f, _)| w.shot.is_some() || frame.saturating_sub(f) > SHOT_WAIT_FRAMES);
    if ready {
        let shot = w.shot.take();
        if let Some((_, req_id)) = w.asked.take() {
            if shot.is_none() {
                let _ = w.shot_req.compare_exchange(req_id, 0, Ordering::AcqRel, Ordering::Acquire);
                warn!("save: no picture of the window within {SHOT_WAIT_FRAMES} frames; saving without a thumbnail");
            }
        }
        let batch = std::mem::take(&mut w.waiting);
        start_writer(&mut w, batch, shot);
    }
    report_written(world);
}

/// Capture format for the pre-UI save thumbnail blit: 8-bit sRGB RGBA so hardware
/// linear-to-sRGB conversion handles both SDR and HDR (`Rgba16Float`) main textures
/// uniformly [design, #3808].
const SHOT_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;

const MAP_PENDING: u8 = 0;
const MAP_DONE: u8 = 1;
const MAP_FAILED: u8 = 2;

/// WebGPU `copyTextureToBuffer` requires `bytes_per_row` to be a multiple of 256 bytes
/// [documented: WebGPU `copyTextureToBuffer`, `COPY_BYTES_PER_ROW_ALIGNMENT`].
const COPY_BYTES_PER_ROW_ALIGNMENT: u32 = 256;

/// Row byte stride aligned to [`COPY_BYTES_PER_ROW_ALIGNMENT`] (256 bytes) for
/// 4-byte-per-pixel RGBA8 readbacks.
fn aligned_rgba_bytes_per_row(width: u32) -> u32 {
    let unaligned = width.saturating_mul(4);
    unaligned.div_ceil(COPY_BYTES_PER_ROW_ALIGNMENT).saturating_mul(COPY_BYTES_PER_ROW_ALIGNMENT)
}

/// Strip row-alignment padding from a mapped RGBA8 buffer into tightly packed `width x height x 4` bytes.
fn unpad_rgba_rows(mapped: &[u8], width: u32, height: u32, bytes_per_row: u32) -> Option<Vec<u8>> {
    let row_bytes = (width as usize).checked_mul(4)?;
    let stride = bytes_per_row as usize;
    let rows = height as usize;
    if row_bytes == 0 || rows == 0 || stride < row_bytes || mapped.len() < stride.checked_mul(rows)? {
        return None;
    }
    if stride == row_bytes {
        return Some(mapped[..row_bytes * rows].to_vec());
    }
    let mut out = Vec::with_capacity(row_bytes * rows);
    for y in 0..rows {
        let start = y * stride;
        out.extend_from_slice(&mapped[start..start + row_bytes]);
    }
    Some(out)
}

struct PreparedSaveShot {
    id: u64,
    entity: Entity,
    pipeline_id: bevy::render::render_resource::CachedRenderPipelineId,
    texture: Texture,
    texture_view: TextureView,
    buffer: Buffer,
    width: u32,
    height: u32,
    bytes_per_row: u32,
}

struct SubmittedSaveShot {
    id: u64,
    buffer: Buffer,
    width: u32,
    height: u32,
    bytes_per_row: u32,
}

struct MappingSaveShot {
    id: u64,
    buffer: Buffer,
    width: u32,
    height: u32,
    bytes_per_row: u32,
    status: Arc<AtomicU8>,
}

#[derive(Resource, Default)]
struct RenderSaveShot {
    prepared: Option<PreparedSaveShot>,
    submitted: Vec<SubmittedSaveShot>,
    mapping: Vec<MappingSaveShot>,
}

/// Allocate the temporary pre-UI capture texture and staging buffer only on a frame a save
/// thumbnail was requested (#3808). Selects only a `Camera3d` targeting the primary window
/// so `capture_save_shot` (in `Core3d`) always matches it. Zero persistent VRAM cost
/// between saves.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn prepare_save_shot(
    bridge: Res<ShotBridge>,
    mut state: ResMut<RenderSaveShot>,
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    blit_pipeline: Option<Res<BlitPipeline>>,
    pipelines: Option<ResMut<SpecializedRenderPipelines<BlitPipeline>>>,
    primary_windows: Query<MainEntity, (With<ExtractedWindow>, With<PrimaryWindow>)>,
    views: Query<(Entity, &ViewTarget, &ExtractedCamera, Option<&ResolvedCompositingSpace>), With<Camera3d>>,
) {
    state.prepared = None;
    let req_id = bridge.requested.load(Ordering::Acquire);
    if req_id == 0 {
        return;
    }
    let (Some(blit), Some(mut pipelines)) = (blit_pipeline, pipelines) else {
        return;
    };
    let Some(primary) = primary_windows.iter().next() else {
        return;
    };
    let Some((entity, target, _, resolved_space)) = views
        .iter()
        .filter(|(_, _, c, _)| matches!(c.target, Some(NormalizedRenderTarget::Window(w)) if w.entity() == primary) && !matches!(c.output_mode, CameraOutputMode::Skip))
        .min_by_key(|(_, _, c, _)| c.sorted_camera_index_for_target)
    else {
        return;
    };
    let size = target.main_texture().size();
    if size.width == 0 || size.height == 0 {
        return;
    }
    let key = BlitPipelineKey {
        target_format: SHOT_FORMAT,
        blend_state: None,
        samples: 1,
        source_space: ResolvedCompositingSpace::space(resolved_space),
        hdr: false,
        hdr_gamma22: false,
    };
    let pipeline_id = pipelines.specialize(&pipeline_cache, &blit, key);
    if pipeline_cache.get_render_pipeline(pipeline_id).is_none() {
        // Pipeline is still compiling on the first save; keep `requested` set and capture next frame.
        return;
    }
    let extent = Extent3d { width: size.width, height: size.height, depth_or_array_layers: 1 };
    let texture = render_device.create_texture(&TextureDescriptor {
        label: Some("nvf_save_thumbnail_capture"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: SHOT_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let texture_view = texture.create_view(&Default::default());
    let bytes_per_row = aligned_rgba_bytes_per_row(size.width);
    let buffer = render_device.create_buffer(&BufferDescriptor {
        label: Some("nvf_save_thumbnail_readback"),
        size: u64::from(bytes_per_row) * u64::from(size.height),
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    state.prepared = Some(PreparedSaveShot {
        id: req_id,
        entity,
        pipeline_id,
        texture,
        texture_view,
        buffer,
        width: size.width,
        height: size.height,
        bytes_per_row,
    });
}

/// Blit the main 3D view's post-processed texture into the capture texture and copy to the
/// readback buffer after `Core3dSystems::PostProcess` and before `ui_pass`, so the save
/// thumbnail has no HUD or menus without hiding any UI (#3808).
fn capture_save_shot(
    view: ViewQuery<(Entity, &ViewTarget)>,
    bridge: Res<ShotBridge>,
    mut state: ResMut<RenderSaveShot>,
    blit_pipeline: Option<Res<BlitPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (entity, target) = view.into_inner();
    if state.prepared.as_ref().is_none_or(|p| p.entity != entity) {
        return;
    }
    let Some(blit) = blit_pipeline else { return };
    let Some(prepared) = state.prepared.take() else { return };
    let Some(pipeline) = pipeline_cache.get_render_pipeline(prepared.pipeline_id) else {
        return;
    };
    let bind_group = blit.create_bind_group(ctx.render_device(), target.main_texture_view(), &pipeline_cache);
    {
        let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
            label: Some("nvf_save_thumbnail_blit"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &prepared.texture_view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    let extent = Extent3d { width: prepared.width, height: prepared.height, depth_or_array_layers: 1 };
    ctx.command_encoder().copy_texture_to_buffer(
        prepared.texture.as_image_copy(),
        TexelCopyBufferInfo {
            buffer: &prepared.buffer,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(prepared.bytes_per_row),
                rows_per_image: Some(prepared.height),
            },
        },
        extent,
    );
    state.submitted.push(SubmittedSaveShot {
        id: prepared.id,
        buffer: prepared.buffer,
        width: prepared.width,
        height: prepared.height,
        bytes_per_row: prepared.bytes_per_row,
    });
    let _ = bridge.requested.compare_exchange(prepared.id, 0, Ordering::AcqRel, Ordering::Acquire);
}

/// Start async readbacks after `RenderGraphSystems::Submit` and deliver completed captures
/// back to the main world without stalling the render thread (#3808).
fn finish_save_shot(bridge: Res<ShotBridge>, mut state: ResMut<RenderSaveShot>) {
    let submitted = std::mem::take(&mut state.submitted);
    for shot in submitted {
        let status = Arc::new(AtomicU8::new(MAP_PENDING));
        let done = status.clone();
        shot.buffer.slice(..).map_async(MapMode::Read, move |res| {
            done.store(if res.is_ok() { MAP_DONE } else { MAP_FAILED }, Ordering::Release);
        });
        state.mapping.push(MappingSaveShot {
            id: shot.id,
            buffer: shot.buffer,
            width: shot.width,
            height: shot.height,
            bytes_per_row: shot.bytes_per_row,
            status,
        });
    }
    let mut i = 0;
    while i < state.mapping.len() {
        match state.mapping[i].status.load(Ordering::Acquire) {
            MAP_PENDING => i += 1,
            MAP_DONE => {
                let m = state.mapping.swap_remove(i);
                let px = {
                    let slice = m.buffer.slice(..);
                    match slice.get_mapped_range() {
                        Ok(mapped) => unpad_rgba_rows(&mapped, m.width, m.height, m.bytes_per_row),
                        Err(e) => {
                            warn!("save: failed to read mapped thumbnail buffer: {e}");
                            None
                        }
                    }
                };
                m.buffer.unmap();
                if let Some(px) = px {
                    let _ = bridge.tx.send(Shot { id: m.id, w: m.width, h: m.height, bgra: false, px });
                }
            }
            _ => {
                let _ = state.mapping.swap_remove(i);
                warn!("save: GPU readback failed for save thumbnail; saving without one");
            }
        }
    }
}

fn start_writer(w: &mut SaveWrites, batch: Vec<Unwritten>, shot: Option<Shot>) {
    if batch.is_empty() {
        return;
    }
    w.in_flight += batch.len();
    let tx = w.tx.clone();
    w.threads.retain(|t| !t.is_finished());
    w.threads.push(std::thread::spawn(move || {
        let png = shot.and_then(|s| thumbnail_rgb(s.w, s.h, &s.px, s.bgra)).and_then(|rgb| encode_png(&rgb, THUMB_W, THUMB_H)).unwrap_or_default();
        for u in batch {
            let result = fnv_save::write(&u.path, &u.header, &png, &u.body, u.kind != SaveKind::Manual).map(|()| (u.body.len(), png.len())).map_err(|e| e.to_string());
            let _ = tx.send(Written { path: u.path, kind: u.kind, result, refs: u.refs, t0: u.t0, snapshot_ms: u.snapshot_ms });
        }
    }));
}

/// The corner message for a save that failed: which kind and a short reason, in
/// NVForever's own words. Without it a quicksave or an autosave that failed showed
/// nothing, and the player believed the game was saved (#3147). The reason is the
/// error's first line, cut at [`REASON_CHARS`] characters (the log has all of it).
fn failed_note(kind: SaveKind, error: &str) -> String {
    let what = match kind {
        SaveKind::Auto => tr("Autosave failed"),
        SaveKind::Quick => tr("Quicksave failed"),
        SaveKind::Manual | SaveKind::System => tr("Save failed"),
    };
    let line = error.lines().next().unwrap_or("").trim();
    let mut reason: String = line.chars().take(REASON_CHARS).collect();
    if reason.len() < line.len() {
        reason.push_str("...");
    }
    if reason.is_empty() {
        format!("{what}.")
    } else {
        format!("{what}: {reason}")
    }
}

/// The characters of an error a corner message shows.
const REASON_CHARS: usize = 120;

fn say_in_the_corner(world: &mut World, text: String) {
    if let Some(mut c) = world.get_resource_mut::<crate::corner_messages::CornerMessages>() {
        c.push(text.into());
    }
}

/// Log the files the writer thread finished, for the console, the corner and the pause menu.
fn report_written(world: &mut World) {
    let done: Vec<Written> = {
        let w = world.resource::<SaveWrites>();
        let rx = w.rx.lock().expect("the writer channel");
        rx.try_iter().collect()
    };
    for d in done {
        world.resource_mut::<SaveWrites>().in_flight -= 1;
        let line = match &d.result {
            Ok((bytes, shot)) => {
                let kind = format!("{:?}", d.kind).to_lowercase();
                let thumb = if *shot > 0 { format!("thumbnail {THUMB_W}x{THUMB_H} ({shot} bytes PNG)") } else { "no thumbnail".into() };
                info!("save: wrote {} ({kind}, {bytes} bytes, {} changed references), {thumb} in {:.0} ms (snapshot {:.0} ms)", d.path.display(), d.refs, d.t0.elapsed().as_secs_f64() * 1e3, d.snapshot_ms);
                // SUP's GetSaveName and GetSavedSaveSize read the last save written
                // (`fnv_game::nvse_sup4`).
                if let Some(rt) = world.get_resource_mut::<ScriptState>().and_then(|s| s.into_inner().rt.as_mut()) {
                    let name = d.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    rt.game.save_files.saved = Some(fnv_game::nvse_sup4::SaveFile { name, bytes: *bytes as u64 });
                }
                if let Some(mut c) = world.get_resource_mut::<crate::corner_messages::CornerMessages>() {
                    c.push(tr(if d.kind == SaveKind::Auto { "Autosaved" } else { "Game saved" }).to_string().into());
                }
                format!("Saved {}", d.path.display())
            }
            Err(e) => {
                warn!("save: failed: {e}");
                say_in_the_corner(world, failed_note(d.kind, e));
                format!("Save failed: {e}")
            }
        };
        world.resource_mut::<SaveLog>().0.push(line);
        world.write_message(SaveDone { path: d.path, result: d.result.map(|_| ()) });
    }
}

/// Quitting (or starting again for a load) while saves are not on disk: write those still
/// waiting for their picture without one and wait for the writer threads.
fn flush_on_exit(world: &mut World) {
    if world.resource::<Messages<AppExit>>().is_empty() || !world.resource::<SaveWrites>().busy() {
        return;
    }
    let mut w = world.resource_mut::<SaveWrites>();
    let batch = std::mem::take(&mut w.waiting);
    w.asked = None;
    w.shot_req.store(0, Ordering::Release);
    start_writer(&mut w, batch, None);
    let threads = std::mem::take(&mut w.threads);
    for t in threads {
        let _ = t.join();
    }
    report_written(world);
}

/// The thumbnail's RGB pixels from the window's picture (`px` 4 bytes a pixel, BGRA or
/// RGBA, alpha left out): the middle 16:9 of it, each pixel the average of the area it
/// covers.
fn thumbnail_rgb(w: u32, h: u32, px: &[u8], bgra: bool) -> Option<Vec<u8>> {
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 || px.len() < w * h * 4 {
        return None;
    }
    let (cw, ch) = if w * 9 > h * 16 { (h * 16 / 9, h) } else { (w, w * 9 / 16) };
    let (x0, y0) = ((w - cw) / 2, (h - ch) / 2);
    let (tw, th) = (THUMB_W as usize, THUMB_H as usize);
    // The source span of output pixel `i` of `n` over `len` source pixels, at least one wide.
    let span = |i: usize, n: usize, len: usize, at: usize, max: usize| {
        let a = at + i * len / n;
        (a, (at + (i + 1) * len / n).max(a + 1).min(max))
    };
    let mut out = Vec::with_capacity(tw * th * 3);
    for ty in 0..th {
        let (sy0, sy1) = span(ty, th, ch, y0, h);
        for tx in 0..tw {
            let (sx0, sx1) = span(tx, tw, cw, x0, w);
            let mut sum = [0u32; 3];
            for y in sy0..sy1 {
                for p in px[(y * w + sx0) * 4..(y * w + sx1) * 4].as_chunks::<4>().0 {
                    for (s, v) in sum.iter_mut().zip(p) {
                        *s += u32::from(*v);
                    }
                }
            }
            let n = ((sy1 - sy0) * (sx1 - sx0)) as u32;
            let avg = sum.map(|v| ((v + n / 2) / n) as u8);
            out.extend_from_slice(&if bgra { [avg[2], avg[1], avg[0]] } else { avg });
        }
    }
    Some(out)
}

fn encode_png(rgb: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    let mut wr = e.write_header().ok()?;
    wr.write_image_data(rgb).ok()?;
    wr.finish().ok()?;
    Some(out)
}

/// A save's thumbnail as RGBA pixels (width, height, pixels), for the load menu.
pub fn decode_thumbnail(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut d = png::Decoder::new(std::io::Cursor::new(bytes));
    d.set_transformations(png::Transformations::EXPAND);
    let mut r = d.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgba, png::BitDepth::Eight) => buf,
        (png::ColorType::Rgb, png::BitDepth::Eight) => buf.as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        _ => return None,
    };
    Some((info.width, info.height, rgba))
}

fn engine_snapshot(world: &World) -> EngineSnapshot {
    let mut e = EngineSnapshot { format: ENGINE_FORMAT.into(), ..default() };
    if let Some(c) = world.get_resource::<crate::control::PlayerControl>() {
        e.third_person = c.pov == crate::control::Pov::Third;
        e.distance = c.distance;
        e.sneak = c.sneak;
        e.always_run = c.always_run;
    }
    e.flashlight = world.get_resource::<crate::flashlight::Flashlight>().is_some_and(|f| f.on);
    e.magazine_ammo = world.get_resource::<crate::combat::Combat>().and_then(|c| c.magazine_ammo_for_save());
    e.chosen_ammo = world.get_resource::<crate::combat::Combat>().map(|c| c.chosen_ammo_for_save()).unwrap_or_default();
    e.magazine = world.get_resource::<crate::combat::Combat>().and_then(|c| c.magazine_for_save());
    let game = world.get_resource::<ScriptState>().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
    e.active_quest = match world.get_resource::<crate::pipboy::data::DataState>() {
        Some(d) => d.active_quest(game),
        None => game.and_then(|g| g.ui.active_quest).map(|q| q.0),
    };
    e.coop = world.get_resource::<CoopEngineState>().map(|c| c.0.clone()).unwrap_or_default();
    let hooks = world.resource::<SaveHooks>().0.clone();
    for h in hooks {
        if let Some(v) = (h.save)(world) {
            e.extra.insert(h.key.to_string(), v);
        }
    }
    e
}

fn header(world: &World, kind: SaveKind, game: &fnv_game::game::GameSnapshot, engine: &EngineSnapshot) -> SaveHeader {
    let mut h = SaveHeader::new(kind);
    let d = &world.resource::<PlayWorld>().0;
    let sim = world.resource::<SimState>();
    h.plugins = plugin_list(d);
    h.play_seconds = game.time;
    if let Ok(c) = sim.world.calendar() {
        (h.game_year, h.game_month, h.game_day, h.game_hour) = (c.year as u32, c.month as u32, c.day as u32, c.hour);
        h.game_days_passed = Some(c.days_passed);
    }
    h.player_name = game.player.name.clone().unwrap_or_else(|| "Courier".into());
    if let Some(rt) = world.resource::<ScriptState>().rt.as_ref() {
        h.level = rt.game.player_level().max(1.0) as u32;
    }
    let loc = world.get_resource::<Location>().copied();
    let player = world.get_resource::<crate::player::Player>();
    let feet = sim.world.peek(fnv_sim::FormId::PLAYER_REF).map(|s| s.pos).or(player.map(|p| p.feet)).unwrap_or_default();
    h.place = fnv_save::Place {
        world: loc.map(|l| l.world),
        interior: loc.and_then(|l| l.interior),
        feet: player.map_or(feet, |p| p.feet),
        heading_deg: player.map_or(0.0, |p| p.heading.to_degrees()),
        pitch_deg: player.map_or(0.0, |p| p.pitch.to_degrees()),
    };
    let cell = loc.and_then(|l| l.interior).or_else(|| sim.world.peek(fnv_sim::FormId::PLAYER_REF).and_then(|s| s.cell).map(|c| c.0));
    h.location = cell.map(|c| crate::console::place_name(d, c)).unwrap_or_default();
    h.coop = game
        .coop
        .iter()
        .map(|p| fnv_save::CoopEntry { slot: p.slot, name: p.character.name.clone().unwrap_or_else(|| format!("Player {}", p.slot)), level: p.character.level.unwrap_or(game.sim.config.player_level as i32).max(1) as u32 })
        .collect();
    h.weather = engine.extra.get("sky").and_then(|v| v.get("weather")).and_then(|w| w.as_str()).map(str::to_string);
    h
}

/// Take this game as the co-op guest's own ([`KeepSolo`], module notes) once it runs and a
/// save it loaded is back, before anything moves the player.
fn keep_solo(world: &mut World) {
    if !world.contains_resource::<KeepSolo>() || world.contains_resource::<SoloBase>() || world.contains_resource::<PendingEngine>() {
        return;
    }
    let Some(st) = world.get_resource::<ScriptState>() else { return };
    if st.off.is_some() {
        world.insert_resource(SoloBase(None));
        return;
    }
    if st.rt.is_none() || st.pending_game_waiting() || world.resource::<crate::loading::Loading>().active() {
        return;
    }
    let game = world.resource_scope(|world, mut sim: Mut<SimState>| {
        let mut st = world.resource_mut::<ScriptState>();
        let rt = st.rt.as_mut()?;
        Some(crate::script::with_world(rt, &mut sim, |rt| rt.game.snapshot()))
    });
    let Some(game) = game else { return };
    let engine = engine_snapshot(world);
    let header = header(world, SaveKind::Auto, &game, &engine);
    info!("save: this game kept as the co-op guest's own: {} at {:?}; its saves come back here", header.location, header.place.feet);
    world.insert_resource(SoloBase(Some(Box::new(Solo { game, engine, header }))));
}

/// Put back the engine's part once the runtime holds the restored game.
fn apply_engine(world: &mut World) {
    if !world.contains_resource::<PendingEngine>() {
        return;
    }
    if world.get_resource::<ScriptState>().is_none_or(|s| s.rt.is_none() || s.pending_game_waiting()) {
        return;
    }
    let Some(PendingEngine(e, path, order)) = world.remove_resource::<PendingEngine>() else { return };
    replay_claims(world);
    // The player's tweak: a load turns god mode off [design: docs/design/tweaks.md].
    if world.get_resource::<crate::tweaks::TweakSettings>().is_some_and(|t| t.game.god_mode_off_on_load) && crate::combat::god_mode() {
        crate::combat::set_god_mode(false);
        if let Some(rt) = world.get_resource_mut::<ScriptState>().and_then(|s| s.into_inner().rt.as_mut()) {
            rt.game.toggles.god_mode = false;
        }
        info!("save: god mode off after the load (tweak)");
    }
    // GetLoadOrderChanged (`fnv_game::nvse_jip12`): the save's plugins moved or are gone
    // [hypothesis: plugins only added after the save's own do not count].
    if let Some(rt) = world.get_resource_mut::<ScriptState>().and_then(|s| s.into_inner().rt.as_mut()) {
        rt.game.set_load_order_changed(order.is_some());
        rt.game.senses.first_person = !e.third_person;
    }
    world.insert_resource(CoopEngineState(e.coop.clone()));
    if let Some(mut c) = world.get_resource_mut::<crate::control::PlayerControl>() {
        c.pov = if e.third_person { crate::control::Pov::Third } else { crate::control::Pov::First };
        if e.distance > 0.0 {
            c.distance = e.distance;
        }
        c.sneak = e.sneak;
        c.always_run = e.always_run;
    }
    if let Some(mut f) = world.get_resource_mut::<crate::flashlight::Flashlight>() {
        f.on = e.flashlight;
    }
    if let Some(mut c) = world.get_resource_mut::<crate::combat::Combat>() {
        c.restore_chosen_ammo(&e.chosen_ammo);
        if let Some(m) = e.magazine {
            c.restore_magazine(m, e.magazine_ammo);
        }
    }
    if let Some(q) = e.active_quest {
        crate::pipboy::data::DataState::restore_active_quest(world, q);
    }
    let hooks = world.resource::<SaveHooks>().0.clone();
    for h in hooks {
        if let Some(v) = e.extra.get(h.key) {
            let mut v = v.clone();
            if let (Some(remap), Some(o)) = (h.remap, &order) {
                remap(&mut v, &o.remap);
            }
            (h.load)(world, &v);
        }
    }
    info!("load: engine state of {} put back ({} part(s) from other modules)", path.display(), e.extra.len());
    world.resource_mut::<SaveLog>().0.push(format!("Loaded {}", path.display()));
    // Plugins the save was made with that are not loaded: said in a corner message as the
    // game comes up (our wording [hypothesis]: what the original shows is not documented
    // and its executable's strings cannot be read).
    if let Some(o) = order.filter(|o| !o.missing.is_empty()) {
        let text = missing_content_text(&o.missing);
        world.resource_mut::<SaveLog>().0.push(text.clone());
        if let Some(mut c) = world.get_resource_mut::<crate::corner_messages::CornerMessages>() {
            c.push(crate::script_events::Note { text, icon: None, seconds: Some(6.0) });
        }
    }
}

/// What the player is told when a save's plugins are missing (design §4.1).
pub fn missing_content_text(missing: &[String]) -> String {
    trf("This save relies on content that is no longer present: {missing}. What came from it was removed.", &[("missing", &missing.join(", "))])
}

/// Read a save for `--load`: its header (to start the world at its place) and its body,
/// refused here when the runtime could not restore it ([`SaveBody::check`]), before anything
/// starts from it. A save made with another load order is remapped by plugin name (design §4.1): every
/// FormID's mod index moves to its plugin's place now; what came from plugins no longer
/// loaded is dropped and reported. A save carrying runtime lambda definitions is refused
/// when the remap would move its lambda index (callable-identity.md). Called before the log
/// is set up: its lines go straight to the output, as nvforever's first `load:` line.
pub fn read_for_load(path: &std::path::Path, current: &[PluginEntry]) -> Result<(SaveHeader, SaveBody, Option<OrderChange>), String> {
    let f = fnv_save::read(path).map_err(|e| e.to_string())?;
    let remap = fnv_save::PluginRemap::new(&f.header.plugins, current);
    let body_of = |v: serde_json::Value| -> Result<SaveBody, String> {
        let body: SaveBody = serde_json::from_value(v).map_err(|e| format!("{}: {e}", path.display()))?;
        body.check().map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(body)
    };
    let raw: serde_json::Value = serde_json::from_slice(&f.body).map_err(|e| format!("{}: {e}", path.display()))?;
    if remap.is_identity() {
        if let fnv_save::PluginCheck::Appended { added } = fnv_save::check_plugins(&f.header.plugins, current) {
            eprintln!("load: plugins added since the save, after its own: {}", added.join(", "));
        }
        return Ok((f.header, body_of(raw)?, None));
    }
    // Retired aliases are reserved identities, not plugin-indexed records. This
    // first persistence format supports identical/appended load orders only;
    // refuse before the generic walker can remap or discard their provenance.
    if raw.get("game").and_then(|g| g.get("retired_scene_bases")).is_some_and(|v| v.as_object().is_none_or(|m| !m.is_empty())) {
        return Err(format!("{} contains retired temporary scene identities and cannot be loaded with a reordered or missing plugin list", path.display()));
    }
    // Lambda handles and their saved `ref` tokens all have index FE. The walker keeps
    // those numbers unless the save had a plugin at FE that now sits elsewhere; then it
    // would move the tokens but not the handles, and cannot tell a lambda token from that
    // plugin's record. Refuse that case (explicit or legacy positional handles).
    let lambda = fnv_game::game::LAMBDA_BASE;
    if matches!(remap.form(lambda), fnv_save::Remapped::Kept(n) if n != lambda)
        && raw.get("game").and_then(|g| g.get("lambdas")).and_then(|v| v.as_array()).is_some_and(|l| !l.is_empty())
    {
        return Err(format!("{} contains runtime lambda definitions and cannot be loaded while its plugin at index FE moves", path.display()));
    }
    let missing = remap.missing();
    if !missing.is_empty() && current.len() > fnv_save::GONE_INDEX as usize {
        return Err(format!("{} was made with plugins that are not loaded now ({missing:?}) and the load order has 255 plugins: unload one to load it", path.display()));
    }
    let added: Vec<&str> = current.iter().filter(|c| !f.header.plugins.iter().any(|p| p.name.eq_ignore_ascii_case(&c.name))).map(|c| c.name.as_str()).collect();
    let moved = remap.moved().iter().map(|(n, a, b)| format!("{n} {a:02X}->{b:02X}")).collect::<Vec<_>>();
    let mut raw = raw;
    let report = fnv_save::remap_json_with(&mut raw, &remap, &fnv_game::game::SAVE_PAIR_FIELDS);
    let mut body = body_of(raw)?;
    let gone = |id: fnv_sim::FormId| fnv_save::is_gone(id.0);
    let dropped_game = body.game.drop_gone(gone);
    let e = &mut body.engine;
    e.magazine = e.magazine.and_then(|(w, n)| remap.map(w).map(|w| (w, n)));
    e.magazine_ammo = e.magazine_ammo.and_then(|a| remap.map(a));
    e.chosen_ammo = std::mem::take(&mut e.chosen_ammo).into_iter().filter_map(|(w, a)| Some((remap.map(w)?, remap.map(a)?))).collect();
    e.active_quest = e.active_quest.and_then(|q| remap.map(q));
    for c in &mut e.coop {
        c.magazine = c.magazine.and_then(|(w, n)| remap.map(w).map(|w| (w, n)));
    }
    eprintln!(
        "load: {} was made with another load order: moved {:?}, missing now {:?}, added {:?}; {} FormIDs remapped, {} entries of missing plugins dropped ({:?}), {} fields named a missing plugin's form, {} entries holding them dropped by the game layer",
        path.display(),
        moved,
        missing,
        added,
        report.moved,
        report.dropped_total(),
        report.dropped,
        report.gone_total(),
        dropped_game,
    );
    if !missing.is_empty() {
        eprintln!("load: {}", missing_content_text(&missing));
    }
    Ok((f.header, body, Some(OrderChange { remap, missing })))
}

/// Joint progression's replay (`fnv_game::coop_joint`, coop-shared-progress.md §3.5): when this character's
/// own world loads, the quest stages it claimed in another player's world are applied
/// before the first frame. Not for a load that joins a co-op session (that world is the
/// host's: the claims wait in the character for its own next load) [design].
fn replay_claims(world: &mut World) {
    use crate::coop_progress::{REJOIN_ENV, SYNC_ENV};
    if std::env::var_os(SYNC_ENV).is_some() || std::env::var_os(REJOIN_ENV).is_some() {
        return;
    }
    if world.get_resource::<crate::coop_progress::CoopProgress>().is_some_and(|p| p.role == crate::coop_progress::Role::Guest) {
        return;
    }
    let lines = match world.get_resource_mut::<ScriptState>().and_then(|s| s.into_inner().rt.as_mut()) {
        Some(rt) => rt.game.replay_claims(),
        None => return,
    };
    if let Some(mut corner) = world.get_resource_mut::<crate::corner_messages::CornerMessages>() {
        for l in lines {
            corner.push(l.into());
        }
    }
    // The map markers found in another player's world (coop-shared-progress.md §2):
    // found here too, as a save's own are (no notice, no XP).
    let markers = world.get_resource_mut::<ScriptState>().and_then(|s| s.into_inner().rt.as_mut()).map(|rt| std::mem::take(&mut rt.game.player.claims.markers)).unwrap_or_default();
    if !markers.is_empty() {
        info!("co-op: {} map marker(s) found in co-op are found here too", markers.len());
        let mut found = world.get_resource::<crate::pipboy::data::DataState>().map(|s| s.discovered().clone()).unwrap_or_default();
        found.extend(markers.iter().map(|m| m.0));
        crate::pipboy::data::DataState::restore_discovered(world, found);
    }
}

#[cfg(test)]
#[path = "save/retired_scene_tests.rs"]
mod retired_scene_tests;

#[cfg(test)]
#[path = "save/lambda_remap_tests.rs"]
mod lambda_remap_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// A failed save says so in the corner, by kind, with a short reason (#3147).
    #[test]
    fn a_failed_save_has_a_corner_message() {
        assert_eq!(failed_note(SaveKind::Quick, "disk full"), "Quicksave failed: disk full");
        assert_eq!(failed_note(SaveKind::Auto, "disk full\nmore"), "Autosave failed: disk full");
        assert_eq!(failed_note(SaveKind::Manual, ""), "Save failed.");
        assert_eq!(failed_note(SaveKind::System, " x "), "Save failed: x");
        let long = failed_note(SaveKind::Quick, &"é".repeat(500));
        assert_eq!(long.chars().count(), "Quicksave failed: ".len() + REASON_CHARS + 3);
        assert!(long.ends_with("..."));
        // It reaches the corner messages, once for the same text.
        let mut world = World::new();
        say_in_the_corner(&mut world, "nothing to hold it".into());
        world.init_resource::<crate::corner_messages::CornerMessages>();
        say_in_the_corner(&mut world, failed_note(SaveKind::Quick, "disk full"));
        say_in_the_corner(&mut world, failed_note(SaveKind::Quick, "disk full"));
        let mut c = world.resource_mut::<crate::corner_messages::CornerMessages>();
        assert_eq!(c.waiting(), 1);
        c.step(0.0);
        assert_eq!(c.showing().map(|n| n.text.as_str()), Some("Quicksave failed: disk full"));
    }

    fn csty_save_world(name: &str, callback_body: &str) -> (World, PathBuf, Vec<u8>) {
        use crate::script_draw::tests::{play_over, record, sub, top};
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let string = |sig: &[u8; 4], text: &str| sub(sig, [text.as_bytes(), &[0]].concat());
        let play = play_over(&[
            top(b"NPC_", &record(b"NPC_", 0, 7, &[string(b"EDID", "Player"), sub(b"ZNAM", 0xD20u32.to_le_bytes().to_vec())])),
            top(b"CSTY", &record(b"CSTY", 0, 0xD20, &[string(b"EDID", "OwnedSaveStyle")])),
        ]);
        // The assigned and unused cases may run concurrently in the same binary.
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("nvf-csty-save-{}-{serial}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{name}.nvfsave"));
        let previous = b"authored previous save bytes, not a player save".to_vec();
        std::fs::write(&path, &previous).unwrap();
        let mut scripts = ScriptState::over_plugins(play.lo.plugins);
        let game = &mut scripts.rt.as_mut().unwrap().game;
        game.run_source(&format!("SetEventHandler \"SaveGame\" (begin function {{string_var ownName}}\nSetModLocalData \"owned.callback\" 1\n{callback_body}\nend)"), Default::default()).unwrap();
        game.checked_snapshot().unwrap();
        let sim = SimState::new(play.lo, play.schema, play.index);
        let mut world = World::new();
        world.insert_resource(play);
        world.insert_resource(sim);
        world.insert_resource(scripts);
        world.insert_resource(SaveConfig::with_dir(Some(dir)));
        world.init_resource::<SaveHooks>();
        world.init_resource::<SaveWrites>();
        (world, path, previous)
    }

    fn csty_save_query(world: &mut World, expression: &str) -> fnv_script::vm::XVal {
        world.resource_mut::<ScriptState>().rt.as_mut().unwrap().game.run_query(
            &format!("Begin Function {{ }}\nSetFunctionValue ({expression})\nEnd\n"), Default::default(),
        ).unwrap()
    }

    fn assert_csty_callback_save_keeps_main_degradation(assign: bool) {
        use fnv_sim::FormId;
        for (kind, name) in [(SaveKind::Manual, "clone"), (SaveKind::Auto, fnv_save::AUTO_NAME)] {
            let mut callback = "SetModLocalData \"owned.clone\" (TempCloneForm OwnedSaveStyle 0)".to_string();
            if assign { callback.push_str("\nSetCombatStyleTraitNumeric (GetModLocalData \"owned.clone\") 42 7\nplayer.SetCombatStyle (GetModLocalData \"owned.clone\")"); }
            let (mut world, path, previous) = csty_save_world(name, &callback);
            save_now(&mut world, &SaveRequest { name: name.into(), kind }).unwrap();
            assert_eq!(csty_save_query(&mut world, "GetModLocalData \"owned.callback\""), fnv_script::vm::XVal::Num(1.0));
            let fnv_script::vm::XVal::Ref(alias) = csty_save_query(&mut world, "GetModLocalData \"owned.clone\"") else { panic!("SaveGame must create an actual alias") };
            assert_ne!(alias, 0);
            let game = &world.resource::<ScriptState>().rt.as_ref().unwrap().game;
            assert_eq!(game.combat_style(FormId::PLAYER_REF).unwrap().rushing_attack_distance_mult, if assign { 7.0 } else { 0.0 }, "the callback's assignment must be observable");
            let writes = world.resource::<SaveWrites>();
            assert_eq!(writes.waiting.len(), 1);
            assert!(writes.busy());
            assert_eq!(writes.waiting[0].path, path);
            let body: SaveBody = serde_json::from_slice(&writes.waiting[0].body).unwrap();
            let saved_style = body.game.extra.get(&FormId::PLAYER_REF).and_then(|extra| extra.combat_style);
            assert_eq!(saved_style, assign.then_some(FormId(alias)), "save preserves the carrier without silently dropping/remapping it");
            let sim = fnv_sim::World::restore(game.world.forms, game.world.placed, &body.game.sim).unwrap();
            let back = fnv_game::Game::restore(game.data, sim, game.cfg, &body.game).unwrap();
            assert_eq!(back.extra.get(&FormId::PLAYER_REF).and_then(|extra| extra.combat_style), saved_style);
            assert_eq!(back.combat_style_id(FormId::PLAYER_REF), Some(FormId(0xD20)));
            assert_eq!(back.combat_style(FormId::PLAYER_REF).unwrap().rushing_attack_distance_mult, 0.0, "loading uses the original authored style, not the session copy");
            assert_eq!(std::fs::read(&path).unwrap(), previous, "enqueueing alone must not overwrite the previous file");
            drop(back);
            drop(world);
            std::fs::remove_file(&path).unwrap();
            std::fs::remove_dir(path.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn an_unused_csty_clone_created_by_savegame_still_enqueues_manual_and_auto_saves() {
        assert_csty_callback_save_keeps_main_degradation(false);
    }

    #[test]
    fn an_assigned_csty_clone_created_by_savegame_loads_with_authored_fallback() {
        assert_csty_callback_save_keeps_main_degradation(true);
    }

    #[test]
    fn an_authored_style_edit_from_savegame_still_enqueues_without_overwriting_early() {
        let (mut world, path, previous) = csty_save_world("authored", "SetCombatStyleTraitNumeric OwnedSaveStyle 42 2");
        save_now(&mut world, &SaveRequest { name: "authored".into(), kind: SaveKind::Manual }).unwrap();
        assert_eq!(csty_save_query(&mut world, "GetModLocalData \"owned.callback\""), fnv_script::vm::XVal::Num(1.0));
        assert_eq!(csty_save_query(&mut world, "GetCombatStyleTraitNumeric OwnedSaveStyle 42"), fnv_script::vm::XVal::Num(2.0));
        assert_eq!(world.resource::<SaveWrites>().waiting.len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), previous, "enqueueing alone must not overwrite the existing owned file");
        drop(world);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    /// Exercise the real save entry point, including its SaveGame callback. A
    /// callback can introduce unsupported state after a preflight check.
    #[test]
    fn a_scene_clone_created_by_savegame_refuses_before_enqueue_or_overwrite() {
        use crate::script_draw::tests::{play_over, record, sub, top};
        let string = |sig: &[u8; 4], s: &str| sub(sig, [s.as_bytes(), &[0]].concat());
        let play = play_over(&[top(b"NPC_", &record(b"NPC_", 0, 7, &[string(b"EDID", "Player")])), top(b"STAT", &record(b"STAT", 0, 0xD10, &[string(b"EDID", "OwnedSceneBase"), string(b"MODL", "owned\\original.nif")]))]);
        let dir = std::env::temp_dir().join(format!("nvf-scene-save-callback-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for temporary in [false, true] {
            let name = if temporary { "clone" } else { "ordinary" };
            let path = dir.join(format!("{name}.nvfsave"));
            let previous = b"owned previous save, must survive refusal";
            std::fs::write(&path, previous).unwrap();
            let mut scripts = ScriptState::over_plugins(play.lo.plugins);
            let target = if temporary { "(TempCloneForm OwnedSceneBase 1)" } else { "OwnedSceneBase" };
            let handler = format!("SetEventHandler \"SaveGame\" (begin function {{string_var ownName}}\nplayer.PlaceAtMe {target}\nend)");
            scripts.rt.as_mut().unwrap().game.run_source(&handler, Default::default()).unwrap();
            let sim = SimState::new(play.lo, play.schema, play.index);
            let mut world = World::new();
            world.insert_resource(play);
            world.insert_resource(sim);
            world.insert_resource(scripts);
            world.insert_resource(SaveConfig::with_dir(Some(dir.clone())));
            world.init_resource::<SaveHooks>();
            world.init_resource::<SaveWrites>();
            let result = save_now(&mut world, &SaveRequest { name: name.into(), kind: SaveKind::Manual });
            let created = world.resource::<SimState>().world.created().count();
            assert_eq!(created, 1, "the SaveGame handler must actually place its reference");
            if temporary {
                assert!(result.unwrap_err().contains("temporary cloned base"));
                assert!(world.resource::<SaveWrites>().waiting.is_empty());
                assert!(!world.resource::<SaveWrites>().busy());
            } else {
                result.unwrap();
                assert_eq!(world.resource::<SaveWrites>().waiting.len(), 1);
            }
            assert_eq!(std::fs::read(&path).unwrap(), previous, "snapshotting alone must never overwrite the existing file");
            std::fs::remove_file(&path).unwrap();
        }
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn an_engine_snapshot_without_new_fields_still_reads() {
        let e: EngineSnapshot = serde_json::from_str(r#"{"format":"nvf.engine.state/1"}"#).unwrap();
        assert_eq!(e, EngineSnapshot { format: ENGINE_FORMAT.into(), ..default() });
        assert!(e.chosen_ammo.is_empty());
        let full = EngineSnapshot {
            format: ENGINE_FORMAT.into(),
            third_person: true,
            distance: 150.0,
            sneak: true,
            always_run: true,
            flashlight: true,
            magazine: Some((0x1234, 7)),
            magazine_ammo: Some(0x2345),
            chosen_ammo: [(0x1234, 0x2345), (0x1235, 0x2346)].into(),
            active_quest: Some(0x104C1C),
            extra: [("sky".to_string(), serde_json::json!({"weather": "NVWastelandClear"}))].into(),
            coop: vec![CoopEngine { slot: 3, third_person: true, distance: 90.0, sneak: true, always_run: false, flashlight: true, magazine: Some((0x1235, 2)) }],
        };
        let back: EngineSnapshot = serde_json::from_str(&serde_json::to_string(&full).unwrap()).unwrap();
        assert_eq!(back, full);
        // One player writes no `coop`, and empty `chosen_ammo` is omitted.
        let minimal = serde_json::to_string(&EngineSnapshot { format: ENGINE_FORMAT.into(), ..default() }).unwrap();
        assert!(!minimal.contains("coop"));
        assert!(!minimal.contains("chosen_ammo"));
    }

    #[test]
    fn chosen_ammo_remaps_and_drops_missing_plugins_on_load() {
        let st = ScriptState::over_no_plugins();
        let saved_plugins = vec![
            PluginEntry { name: "Master.esm".into(), size: 0 },
            PluginEntry { name: "ModA.esp".into(), size: 0 },
            PluginEntry { name: "ModB.esp".into(), size: 0 },
        ];
        // ModB moves from index 2 to 1; ModA is removed.
        let current_plugins = vec![
            PluginEntry { name: "Master.esm".into(), size: 0 },
            PluginEntry { name: "ModB.esp".into(), size: 0 },
        ];
        let mut engine = EngineSnapshot { format: ENGINE_FORMAT.into(), ..default() };
        engine.chosen_ammo.insert(0x0000_1001, 0x0200_2002); // Master weapon + ModB ammo -> ModB remaps to 01
        engine.chosen_ammo.insert(0x0100_3003, 0x0000_4004); // ModA weapon -> dropped
        engine.chosen_ammo.insert(0x0000_5005, 0x0100_6006); // ModA ammo -> dropped
        let body = SaveBody { format: BODY_FORMAT.into(), game: st.rt.as_ref().unwrap().game.snapshot(), engine };
        let dir = std::env::temp_dir().join(format!("nvf-chosen-ammo-remap-{}", std::process::id()));
        let path = dir.join("remap.nvfsave");
        let mut hdr = SaveHeader::new(SaveKind::Manual);
        hdr.plugins = saved_plugins;
        fnv_save::write(&path, &hdr, &[], &serde_json::to_vec(&body).unwrap(), false).unwrap();
        let (_, loaded, order) = read_for_load(&path, &current_plugins).unwrap();
        assert!(order.is_some());
        assert_eq!(loaded.engine.chosen_ammo, BTreeMap::from([(0x0000_1001, 0x0100_2002)]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_thumbnail_is_the_middle_16_9_scaled_down() {
        // A 4:3 picture, 1024x768, BGRA: the bands above and below 16:9 (green) are cut;
        // red on the left half, blue on the right.
        let (w, h) = (1024u32, 768u32);
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let c = if !(96..672).contains(&y) { [0, 255, 0] } else if x < w / 2 { [0, 0, 255] } else { [255, 0, 0] };
                px[i..i + 3].copy_from_slice(&c);
                px[i + 3] = 7;
            }
        }
        let rgb = thumbnail_rgb(w, h, &px, true).unwrap();
        assert_eq!(rgb.len(), (THUMB_W * THUMB_H * 3) as usize);
        let at = |x: u32, y: u32| rgb[((y * THUMB_W + x) * 3) as usize..][..3].to_vec();
        assert_eq!(at(0, 0), [255, 0, 0]);
        assert_eq!(at(THUMB_W - 1, THUMB_H - 1), [0, 0, 255]);
        let png = encode_png(&rgb, THUMB_W, THUMB_H).unwrap();
        let (tw, th, rgba) = decode_thumbnail(&png).unwrap();
        assert_eq!((tw, th), (THUMB_W, THUMB_H));
        assert_eq!(&rgba[..4], [255, 0, 0, 255]);
        assert!(thumbnail_rgb(10, 10, &px[..8], true).is_none());
        assert!(decode_thumbnail(b"not a png").is_none());
    }

    #[test]
    fn a_save_name_or_a_path_is_taken() {
        let cfg = SaveConfig::with_dir(Some(PathBuf::from("saves")));
        assert_eq!(cfg.path_of("Before Primm").unwrap(), PathBuf::from("saves").join("Before Primm.nvfsave"));
        assert_eq!(cfg.path_of("C:/x/y.nvfsave").unwrap(), PathBuf::from("C:/x/y.nvfsave"));
        assert!(cfg.path_of("bad:name").is_err());
    }

    /// A change that spoils a save's body.
    type BodyChange = fn(&mut SaveBody);

    /// The restore's own refusals come at the read, before the world starts at the save's
    /// place, and the player is told why (#1590).
    #[test]
    fn a_save_whose_snapshots_cannot_be_restored_is_refused_as_it_is_read() {
        let st = ScriptState::over_no_plugins();
        let good = SaveBody { format: BODY_FORMAT.into(), game: st.rt.as_ref().unwrap().game.snapshot(), engine: EngineSnapshot { format: ENGINE_FORMAT.into(), ..default() } };
        let dir = std::env::temp_dir().join(format!("nvf-refused-save-{}", std::process::id()));
        let read = |name: &str, body: &SaveBody| {
            let path = dir.join(format!("{name}.nvfsave"));
            fnv_save::write(&path, &SaveHeader::new(SaveKind::Manual), &[], &serde_json::to_vec(body).unwrap(), false).unwrap();
            read_for_load(&path, &[]).map(|_| ())
        };
        assert_eq!(read("good", &good), Ok(()));
        let changed: [(&str, BodyChange, &str); 6] = [
            ("body", |b| b.format = "nvf.save.body/2".into(), "body format"),
            ("engine", |b| b.engine.format = "nvf.engine.state/2".into(), "engine format"),
            ("game", |b| b.game.format = "fnv.game.state/2".into(), "unsupported snapshot format \"fnv.game.state/2\""),
            ("sim", |b| b.game.sim.format = "fnv.sim.state/2".into(), "unsupported snapshot format \"fnv.sim.state/2\""),
            ("game-rng", |b| b.game.rng_word_pos = "x".into(), "bad rng_word_pos"),
            ("sim-rng", |b| b.game.sim.rng_word_pos = "y".into(), "bad rng_word_pos"),
        ];
        for (name, change, why) in changed {
            let mut bad = good.clone();
            change(&mut bad);
            let e = read(name, &bad).unwrap_err();
            assert!(e.contains(why) && e.contains(&format!("{name}.nvfsave")), "{name}: {e}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A restore refused late leaves no runtime: the engine part stays unapplied, its
    /// success line unwritten, and no save is taken of what is there (#1590).
    #[test]
    fn with_no_runtime_a_pending_save_puts_nothing_back_and_saves_nothing() {
        let mut world = World::new();
        world.init_resource::<SaveHooks>();
        world.init_resource::<SaveLog>();
        world.init_resource::<SaveWrites>();
        let mut st = ScriptState::over_no_plugins();
        st.rt = None;
        world.insert_resource(st);
        world.insert_resource(SaveQueue(vec![SaveRequest { name: "after".into(), kind: SaveKind::Manual }]));
        world.insert_resource(PendingEngine(EngineSnapshot { format: ENGINE_FORMAT.into(), ..default() }, PathBuf::from("refused.nvfsave"), None));
        apply_engine(&mut world);
        write_saves(&mut world);
        assert!(world.contains_resource::<PendingEngine>());
        assert!(world.resource::<SaveLog>().0.is_empty());
        assert_eq!(world.resource::<SaveQueue>().0.len(), 1);
        assert!(!world.resource::<SaveWrites>().busy());
    }

    /// The player's stored autosave switches reach `SaveConfig`: `SavePlugin` starts every
    /// switch on and `settings::apply_stored` (`PostStartup`) sets the ones in
    /// `NVForever.ini`. Nothing may replace `SaveConfig` after registration (#3010 review).
    #[test]
    fn stored_autosave_switches_are_applied_to_the_save_config() {
        let dir = std::env::temp_dir().join(format!("nvf-autosave-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ini = dir.join("NVForever.ini");
        std::fs::write(&ini, "[Settings]\nautosave-travel=0\nautosave-fast-travel=0\nautosave-wait=0\nautosave-sleep=0\nautosave-scripts=0\n").unwrap();
        let mut app = App::new();
        app.add_plugins(SavePlugin);
        app.insert_resource(crate::settings::SettingsFile(Some(ini)));
        crate::settings::apply_stored(app.world_mut());
        let cfg = app.world().resource::<SaveConfig>();
        assert!(
            !cfg.autosave_travel && !cfg.autosave_fast_travel && !cfg.autosave_wait && !cfg.autosave_sleep && !cfg.autosave_scripts,
            "the INI's switches were not applied: {cfg:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #3808: taking a save thumbnail requests a pre-UI capture from the render world
    /// without ever hiding UI root nodes (eliminating the one-frame UI blink on quicksave)
    /// and encodes the delivered RGBA picture into the written save's 512x288 PNG thumbnail.
    #[test]
    fn saving_never_hides_ui_roots_and_writes_pre_ui_thumbnail() {
        let dir = std::env::temp_dir().join(format!("nvf-quicksave-no-blink-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("quicksave.nvfsave");

        let (mut writes, bridge) = SaveWrites::new_with_bridge();
        writes.waiting.push(Unwritten {
            path: path.clone(),
            header: SaveHeader::new(SaveKind::Quick),
            body: b"{}".to_vec(),
            kind: SaveKind::Quick,
            refs: 0,
            t0: std::time::Instant::now(),
            snapshot_ms: 1.0,
        });

        let mut world = World::new();
        world.insert_resource(bevy::diagnostic::FrameCount(10));
        world.insert_resource(writes);
        world.init_resource::<SaveLog>();
        world.init_resource::<Messages<SaveDone>>();
        let hud_root = world.spawn((Node::default(), Visibility::Inherited)).id();
        let menu_root = world.spawn((Node::default(), Visibility::Visible)).id();

        take_shot(&mut world);
        // Neither UI root was hidden, and the render-world pre-UI request is armed with ID 1.
        assert_eq!(*world.get::<Visibility>(hud_root).unwrap(), Visibility::Inherited);
        assert_eq!(*world.get::<Visibility>(menu_root).unwrap(), Visibility::Visible);
        assert_eq!(bridge.requested.load(Ordering::Acquire), 1);
        assert_eq!(world.resource::<SaveWrites>().asked, Some((10, 1)));

        // Simulate the render world completing the pre-UI RGBA capture on the next frame.
        let (w, h) = (32u32, 18u32);
        let px = [20u8, 180, 90, 255].repeat((w * h) as usize);
        bridge.requested.store(0, Ordering::Release);
        bridge.tx.send(Shot { id: 1, w, h, bgra: false, px }).unwrap();

        world.resource_mut::<bevy::diagnostic::FrameCount>().0 = 11;
        finish_writes(&mut world);
        assert_eq!(*world.get::<Visibility>(hud_root).unwrap(), Visibility::Inherited);
        assert_eq!(*world.get::<Visibility>(menu_root).unwrap(), Visibility::Visible);

        let threads = std::mem::take(&mut world.resource_mut::<SaveWrites>().threads);
        for t in threads {
            t.join().unwrap();
        }
        report_written(&mut world);

        let header = fnv_save::read_header(&path).unwrap();
        let (tw, th, rgba) = decode_thumbnail(&header.shot).expect("save has a valid PNG thumbnail");
        assert_eq!((tw, th), (THUMB_W, THUMB_H));
        assert_eq!(&rgba[..4], &[20, 180, 90, 255]);

        // Timeout path: if no render world answers within SHOT_WAIT_FRAMES, the request is
        // cleared and the save still writes without a thumbnail, still without hiding UI.
        let timeout_path = dir.join("timeout.nvfsave");
        world.resource_mut::<SaveWrites>().waiting.push(Unwritten {
            path: timeout_path.clone(),
            header: SaveHeader::new(SaveKind::Quick),
            body: b"{}".to_vec(),
            kind: SaveKind::Quick,
            refs: 0,
            t0: std::time::Instant::now(),
            snapshot_ms: 1.0,
        });
        world.resource_mut::<bevy::diagnostic::FrameCount>().0 = 20;
        take_shot(&mut world);
        assert_eq!(bridge.requested.load(Ordering::Acquire), 2);
        world.resource_mut::<bevy::diagnostic::FrameCount>().0 = 20 + SHOT_WAIT_FRAMES + 1;
        finish_writes(&mut world);
        assert_eq!(bridge.requested.load(Ordering::Acquire), 0);
        assert_eq!(*world.get::<Visibility>(hud_root).unwrap(), Visibility::Inherited);
        let threads = std::mem::take(&mut world.resource_mut::<SaveWrites>().threads);
        for t in threads {
            t.join().unwrap();
        }
        let header = fnv_save::read_header(&timeout_path).unwrap();
        assert!(header.shot.is_empty());

        // Late-readback filtering: a late capture for timed-out request 2 arriving while
        // request 3 is active is discarded and never misattributed to save 3.
        let later_path = dir.join("later.nvfsave");
        world.resource_mut::<SaveWrites>().waiting.push(Unwritten {
            path: later_path.clone(),
            header: SaveHeader::new(SaveKind::Quick),
            body: b"{}".to_vec(),
            kind: SaveKind::Quick,
            refs: 0,
            t0: std::time::Instant::now(),
            snapshot_ms: 1.0,
        });
        world.resource_mut::<bevy::diagnostic::FrameCount>().0 = 60;
        take_shot(&mut world);
        assert_eq!(bridge.requested.load(Ordering::Acquire), 3);
        let stale_px = [240u8, 10, 10, 255].repeat((w * h) as usize);
        bridge.tx.send(Shot { id: 2, w, h, bgra: false, px: stale_px }).unwrap();
        world.resource_mut::<bevy::diagnostic::FrameCount>().0 = 61;
        finish_writes(&mut world);
        // Save 3 is still waiting; the stale red shot (id 2) was rejected.
        assert_eq!(world.resource::<SaveWrites>().asked, Some((60, 3)));
        assert_eq!(world.resource::<SaveWrites>().waiting.len(), 1);
        let fresh_px = [15u8, 45, 220, 255].repeat((w * h) as usize);
        bridge.tx.send(Shot { id: 3, w, h, bgra: false, px: fresh_px }).unwrap();
        world.resource_mut::<bevy::diagnostic::FrameCount>().0 = 62;
        finish_writes(&mut world);
        let threads = std::mem::take(&mut world.resource_mut::<SaveWrites>().threads);
        for t in threads {
            t.join().unwrap();
        }
        let header = fnv_save::read_header(&later_path).unwrap();
        let (tw, th, rgba) = decode_thumbnail(&header.shot).expect("save 3 has a valid PNG thumbnail");
        assert_eq!((tw, th), (THUMB_W, THUMB_H));
        assert_eq!(&rgba[..4], &[15, 45, 220, 255]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn aligned_rgba_row_unpadding_handles_aligned_and_padded_widths() {
        assert_eq!(aligned_rgba_bytes_per_row(64), 256);
        assert_eq!(aligned_rgba_bytes_per_row(65), 512);
        // 2x2 image (8 bytes per row) padded to 256 bytes per row.
        let stride = aligned_rgba_bytes_per_row(2);
        assert_eq!(stride, 256);
        let mut mapped = vec![0xEEu8; (stride * 2) as usize];
        mapped[0..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        mapped[256..264].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);
        let unpacked = unpad_rgba_rows(&mapped, 2, 2, stride).unwrap();
        assert_eq!(unpacked, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        // Exact multiple (64x1): zero-copy fast path matches.
        let exact: Vec<u8> = (0u8..=255).collect();
        assert_eq!(unpad_rgba_rows(&exact, 64, 1, 256).unwrap(), exact);
        // Truncated or zero dimensions are rejected.
        assert!(unpad_rgba_rows(&mapped[..100], 2, 2, stride).is_none());
        assert!(unpad_rgba_rows(&mapped, 0, 2, stride).is_none());
        assert!(unpad_rgba_rows(&mapped, 2, 0, stride).is_none());
    }
}
