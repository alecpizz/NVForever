//! Player death recovery through the existing save loader, not a second restart path.
//! See docs/spec-notes/player-death-recovery.md for observation and policy boundaries.

use super::{LoadRequest, PendingEngine, SaveConfig, SaveLog};
use crate::pause::{Page, PageRequest, Paused};
use crate::script::ScriptState;
use crate::{PlayState, PlayWorld};
use bevy::prelude::*;
use fnv_sim::FormId;
use std::path::{Path, PathBuf};

/// GECK's documented default; a finite nonnegative live GMST takes precedence.
const DEFAULT_DELAY: f32 = 5.0;
const DELAY_SETTING: &str = "fPlayerDeathReloadTime";

/// Captured once by SavePlugin. Ordinary runs keep death recovery; assisted
/// runs must explicitly opt in with the dedicated, owned smoke profile.
#[derive(Resource)]
pub(super) struct RecoveryPolicy {
    enabled: bool,
}

impl FromWorld for RecoveryPolicy {
    fn from_world(world: &mut World) -> Self {
        let environment: Vec<_> = std::env::vars_os().collect();
        let cwd = std::env::current_dir().ok();
        let dir = world.get_resource::<SaveConfig>().and_then(|cfg| cfg.dir.as_deref());
        match Self::for_run(&environment, cwd.as_deref(), dir) {
            Ok(enabled) => {
                if !enabled {
                    info!("test policy: automatic death recovery disabled for an assisted run");
                }
                RecoveryPolicy { enabled }
            }
            Err(why) => {
                warn!("test policy: death recovery opt-in refused: {why}");
                RecoveryPolicy { enabled: false }
            }
        }
    }
}

impl RecoveryPolicy {
    fn for_run(environment: &[(std::ffi::OsString, std::ffi::OsString)], cwd: Option<&Path>, dir: Option<&Path>) -> Result<bool, &'static str> {
        let value = |name: &str| environment.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_os_str());
        let assisted = value("NVF_PLAY_AIDS_FROM_SCRIPTS").is_some()
            || environment.iter().any(|(key, _)| key.to_str().is_some_and(crate::drives_run));
        if !assisted {
            return Ok(true);
        }
        let Some(opt_in) = value("NVF_PLAY_TEST_DEATH_RECOVERY") else { return Ok(false) };
        if opt_in != "1" {
            return Err("expected NVF_PLAY_TEST_DEATH_RECOVERY=1");
        }
        let cwd = cwd.and_then(|p| p.canonicalize().ok()).ok_or("owned working directory is unavailable")?;
        let home = value("NVFOREVER_HOME").filter(|v| !v.is_empty()).ok_or("explicit owned NVFOREVER_HOME is required")?;
        let home = Path::new(home);
        let home = if home.is_absolute() { home.to_path_buf() } else { cwd.join(home) };
        let home = home.canonicalize().map_err(|_| "owned home is unavailable")?;
        if home.parent() != Some(cwd.as_path()) || home.file_name() != Some(std::ffi::OsStr::new("death-recovery-home")) {
            return Err("home must be death-recovery-home directly under this run's working directory");
        }
        let saves = home.join("saves").canonicalize().map_err(|_| "owned saves directory is unavailable")?;
        if saves.parent() != Some(home.as_path()) || saves.file_name() != Some(std::ffi::OsStr::new("saves")) {
            return Err("saves directory escapes the owned home");
        }
        let actual = dir.and_then(|p| p.canonicalize().ok()).ok_or("actual SaveConfig directory is unavailable")?;
        if actual != saves {
            return Err("actual SaveConfig does not select the owned home");
        }
        Ok(true)
    }
}

/// One attempt per uninterrupted death. A resurrection cancels and rearms it.
#[derive(Resource, Default)]
pub(super) struct DeathRecovery {
    dead: bool,
    elapsed: f64,
    handled: bool,
}

/// `fnv_save::list` already orders manual, quick, auto and system saves by saved time.
/// Use the same complete format/plugin/body preflight as an ordinary load. A header
/// alone cannot establish that a file is loadable, or that its player is alive.
fn latest_living(dir: Option<&Path>, plugins: &[fnv_save::PluginEntry]) -> Option<PathBuf> {
    let dir = dir?;
    for (path, _) in fnv_save::list(dir) {
        match super::read_for_load(&path, plugins) {
            Ok((_, body, _)) if !body.game.extra.get(&FormId::PLAYER_REF).is_some_and(|e| e.dead) => return Some(path),
            Ok(_) => info!("death recovery: skipping {}: its player is already dead", path.display()),
            Err(why) => warn!("death recovery: skipping {}: {why}", path.display()),
        }
    }
    None
}

/// Runs after the script frame's effects reach the world and before save requests are
/// flushed. It neither writes a save nor changes the existing dead-save refusal.
pub(super) fn update(world: &mut World) {
    if world.get_resource::<RecoveryPolicy>().is_some_and(|policy| !policy.enabled) {
        return;
    }
    if world.contains_resource::<PendingEngine>() {
        return;
    }
    // In co-op a player at 0 Health is down, waiting to be revived: co-op's own rules
    // decide what follows (`crate::coop_progress`: everyone down loads the last shared
    // save, a guest down alone its own last save). Recovering here would send a downed
    // player to the Load menu mid-revive and race the host's shared-save load [design].
    if world.get_resource::<crate::coop_progress::CoopProgress>().is_some_and(|p| p.role != crate::coop_progress::Role::Solo) {
        *world.resource_mut::<DeathRecovery>() = DeathRecovery::default();
        return;
    }
    let Some(rt) = world.get_resource::<ScriptState>().and_then(|s| s.rt.as_ref()) else { return };
    // In co-op a player at 0 Health is down, waiting to be revived or to respawn: co-op's
    // own rules decide what follows (`nvforever`'s `coop.rs` and `coop/revive.rs`, the
    // downed screen `crate::coop_downed`). Recovering here would send a downed player to
    // the Load menu mid-revive [design].
    if rt.game.coop_game {
        *world.resource_mut::<DeathRecovery>() = DeathRecovery::default();
        return;
    }
    let dead = rt.game.is_dead(FormId::PLAYER_REF);
    let delay = rt.game.world.setting_f32(DELAY_SETTING).filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(DEFAULT_DELAY);
    if !dead {
        *world.resource_mut::<DeathRecovery>() = DeathRecovery::default();
        return;
    }
    // Real seconds, unaffected by the death camera's slow clock; no time is charged
    // while menus or loading hold the world. Read death first so resurrection cancels
    // even while a menu is open.
    let running = world.get_resource::<State<PlayState>>().is_some_and(|s| *s.get() == PlayState::Playing)
        && !world.resource::<Paused>().0
        && !world.resource::<Time<Virtual>>().is_paused();
    let dt = world.resource::<Time<Real>>().delta_secs_f64();
    let due = {
        let mut recovery = world.resource_mut::<DeathRecovery>();
        if !recovery.dead {
            recovery.dead = true;
        } else if running && dt.is_finite() && dt > 0.0 {
            recovery.elapsed += dt;
        }
        if recovery.handled || !running || recovery.elapsed < f64::from(delay) {
            false
        } else {
            // Latch before disk work or dispatch, including the no-save case.
            recovery.handled = true;
            true
        }
    };
    if !due {
        return;
    }

    // Keep a real menu underneath the handoff, so a missing/unreadable save or a child
    // that cannot start does not strand the player behind the death camera's black.
    world.resource_mut::<Paused>().0 = true;
    world.resource_mut::<PageRequest>().0 = Some(Page::Load { selected: 0 });
    world.resource_mut::<NextState<PlayState>>().set(PlayState::Menu);
    if !world.resource::<Messages<LoadRequest>>().is_empty() {
        info!("death recovery: a load is already requested; no second request");
        return;
    }
    let cfg = world.resource::<SaveConfig>();
    let plugins = world.get_resource::<PlayWorld>().map(|p| super::plugin_list(&p.0));
    let path = plugins.and_then(|plugins| latest_living(cfg.dir.as_deref(), &plugins));
    match path {
        Some(path) => {
            info!("death recovery: loading {} after {delay} real unpaused seconds", path.display());
            world.write_message(LoadRequest { path });
        }
        None => {
            let why = "No living save could be loaded; the Load menu is open.";
            warn!("death recovery: {why}");
            world.resource_mut::<SaveLog>().0.push(why.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::{SaveBody, SaveQueue, SaveRequest, BODY_FORMAT, ENGINE_FORMAT};
    use fnv_save::{SaveHeader, SaveKind};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("nvf-death-recovery-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    fn app(dir: Option<PathBuf>, delay: Option<f32>) -> App {
        use crate::script_draw::tests::{play_over, record, sub, top};
        let groups = delay.map(|value| top(b"GMST", &record(b"GMST", 0, 0x800, &[
            sub(b"EDID", b"fPlayerDeathReloadTime\0".to_vec()), sub(b"DATA", value.to_le_bytes().to_vec()),
        ]))).into_iter().collect::<Vec<_>>();
        let play = play_over(&groups);
        let mut app = App::new();
        app.insert_resource(ScriptState::over_plugins(play.lo.plugins))
            .insert_resource(play)
            .insert_resource(SaveConfig::with_dir(dir))
            .insert_resource(State::new(PlayState::Playing))
            .init_resource::<NextState<PlayState>>()
            .init_resource::<Time<Real>>()
            .init_resource::<Time<Virtual>>()
            .init_resource::<Paused>()
            .init_resource::<PageRequest>()
            .init_resource::<SaveLog>()
            .init_resource::<DeathRecovery>()
            .add_message::<LoadRequest>()
            .add_systems(Update, update);
        app
    }

    fn tick(app: &mut App, seconds: f64) {
        app.world_mut().resource_mut::<Time<Real>>().advance_by(Duration::from_secs_f64(seconds));
        app.update();
    }

    fn kill(app: &mut App) {
        assert!(app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.kill(FormId::PLAYER_REF, None));
        tick(app, 0.0);
    }

    fn body(app: &App) -> SaveBody {
        SaveBody { format: BODY_FORMAT.into(), game: app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.snapshot(), engine: crate::save::EngineSnapshot { format: ENGINE_FORMAT.into(), ..default() } }
    }

    fn save(app: &App, dir: &Path, name: &str, time: u64, kind: SaveKind, body: &SaveBody) -> PathBuf {
        let path = dir.join(format!("{name}.nvfsave"));
        let mut header = SaveHeader::new(kind);
        header.saved_unix = time;
        header.plugins = crate::save::plugin_list(&app.world().resource::<PlayWorld>().0);
        fnv_save::write(&path, &header, &[], &serde_json::to_vec(body).unwrap(), false).unwrap();
        path
    }

    fn requests(app: &App) -> Vec<PathBuf> {
        let messages = app.world().resource::<Messages<LoadRequest>>();
        messages.get_cursor().read(messages).map(|m| m.path.clone()).collect()
    }

    #[test]
    fn loaded_eight_second_setting_and_runtime_change_reach_recovery() {
        let dir = Scratch::new();
        let mut app = app(Some(dir.0.clone()), Some(8.0));
        let path = save(&app, &dir.0, "living", 1, SaveKind::Manual, &body(&app));
        kill(&mut app);
        tick(&mut app, 7.0);
        assert!(!app.world().resource::<DeathRecovery>().handled);
        assert!(requests(&app).is_empty());
        app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.world.change_setting(DELAY_SETTING, fnv_sim::settings::Setting::Float(7.5)).unwrap();
        tick(&mut app, 0.5);
        assert_eq!(requests(&app), [path]);
        assert!(app.world().resource::<DeathRecovery>().handled);
        assert!(app.world().resource::<Paused>().0);
        assert_eq!(app.world().resource::<PageRequest>().0, Some(Page::Load { selected: 0 }));
    }

    #[test]
    fn a_player_down_in_coop_is_left_to_coop() {
        use crate::coop_progress::{CoopProgress, Role};
        for role in [Role::Host, Role::Guest] {
            let dir = Scratch::new();
            let mut app = app(Some(dir.0.clone()), Some(1.0));
            save(&app, &dir.0, "living", 1, SaveKind::Manual, &body(&app));
            app.world_mut().insert_resource(CoopProgress { role, ..Default::default() });
            kill(&mut app);
            tick(&mut app, 30.0);
            assert!(requests(&app).is_empty(), "{role:?}");
            assert!(!app.world().resource::<Paused>().0);
            assert!(app.world().resource::<PageRequest>().0.is_none());
            // Back to solo while still dead (the link closed): recovery runs as usual.
            app.world_mut().resource_mut::<CoopProgress>().role = Role::Solo;
            tick(&mut app, 0.0);
            tick(&mut app, 1.5);
            assert_eq!(requests(&app).len(), 1, "{role:?}");
        }
    }

    #[test]
    fn missing_and_invalid_settings_keep_the_documented_default() {
        for setting in [None, Some(f32::NAN), Some(f32::INFINITY), Some(-1.0)] {
            let dir = Scratch::new();
            let mut app = app(Some(dir.0.clone()), setting);
            let path = save(&app, &dir.0, "living", 1, SaveKind::Manual, &body(&app));
            kill(&mut app);
            tick(&mut app, 4.0);
            assert!(requests(&app).is_empty());
            tick(&mut app, 1.0);
            assert_eq!(requests(&app), [path]);
        }
    }

    #[test]
    fn menus_and_paused_time_do_not_charge_and_resurrection_cancels() {
        let dir = Scratch::new();
        let mut app = app(Some(dir.0.clone()), Some(8.0));
        save(&app, &dir.0, "living", 1, SaveKind::Manual, &body(&app));
        kill(&mut app);
        tick(&mut app, 3.0);
        app.world_mut().insert_resource(State::new(PlayState::Menu));
        tick(&mut app, 100.0);
        app.world_mut().insert_resource(State::new(PlayState::Playing));
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        tick(&mut app, 100.0);
        assert_eq!(app.world().resource::<DeathRecovery>().elapsed, 3.0);
        app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.resurrect(FormId::PLAYER_REF, false);
        tick(&mut app, 0.0);
        assert!(!app.world().resource::<DeathRecovery>().dead);
        assert!(requests(&app).is_empty());
        assert!(!app.world().resource::<Paused>().0);
        assert!(app.world().resource::<PageRequest>().0.is_none());
    }

    #[test]
    fn newest_manual_quick_and_auto_slots_each_dispatch_only_once() {
        for (name, kind) in [("manual", SaveKind::Manual), ("quicksave", SaveKind::Quick), ("autosave", SaveKind::Auto)] {
            let dir = Scratch::new();
            let mut app = app(Some(dir.0.clone()), Some(1.0));
            let good = body(&app);
            save(&app, &dir.0, "older", 1, SaveKind::Manual, &good);
            let newest = save(&app, &dir.0, name, 2, kind, &good);
            kill(&mut app);
            tick(&mut app, 1.0);
            assert_eq!(requests(&app), [newest]);
            app.world_mut().resource_mut::<Messages<LoadRequest>>().clear();
            // Even after the requester has consumed its message and the menu closes,
            // that same death cannot send a second automatic request.
            app.world_mut().resource_mut::<Paused>().0 = false;
            tick(&mut app, 100.0);
            assert!(requests(&app).is_empty());
            assert!(app.world().resource::<DeathRecovery>().handled);
        }
    }

    #[test]
    fn corrupt_wrong_format_and_dead_newest_saves_fall_back_to_a_living_one() {
        let dir = Scratch::new();
        let mut app = app(Some(dir.0.clone()), Some(1.0));
        let good = body(&app);
        let living = save(&app, &dir.0, "living", 1, SaveKind::Manual, &good);
        let mut dead = good.clone();
        dead.game.extra.entry(FormId::PLAYER_REF).or_default().dead = true;
        save(&app, &dir.0, "autosave", 2, SaveKind::Auto, &dead);
        let mut wrong = good.clone();
        wrong.engine.format = "nvf.engine.state/future".into();
        save(&app, &dir.0, "future", 3, SaveKind::Manual, &wrong);
        let mut header = SaveHeader::new(SaveKind::Quick);
        header.saved_unix = 4;
        fnv_save::write(&dir.0.join("quicksave.nvfsave"), &header, &[], b"not JSON", false).unwrap();
        std::fs::write(dir.0.join("bad-header.nvfsave"), b"not a save").unwrap();
        kill(&mut app);
        tick(&mut app, 1.0);
        assert_eq!(requests(&app), [living]);
    }

    #[test]
    fn only_dead_or_unreadable_saves_leave_a_usable_load_menu() {
        let dir = Scratch::new();
        let mut app = app(Some(dir.0.clone()), Some(1.0));
        kill(&mut app);
        let dead = body(&app);
        save(&app, &dir.0, "autosave", 1, SaveKind::Auto, &dead);
        std::fs::write(dir.0.join("quicksave.nvfsave"), b"broken").unwrap();
        tick(&mut app, 1.0);
        assert!(requests(&app).is_empty());
        assert!(app.world().resource::<Paused>().0);
        assert_eq!(app.world().resource::<PageRequest>().0, Some(Page::Load { selected: 0 }));
        assert!(matches!(app.world().resource::<NextState<PlayState>>(), NextState::Pending(PlayState::Menu)));
        assert!(app.world().resource::<SaveLog>().0.last().unwrap().contains("Load menu is open"));
    }

    #[test]
    fn no_profile_or_unreadable_directory_still_opens_recovery_menu() {
        let dir = Scratch::new();
        let not_a_dir = dir.0.join("file");
        std::fs::write(&not_a_dir, b"not a directory").unwrap();
        for path in [None, Some(not_a_dir)] {
            let mut app = app(path, Some(0.0));
            kill(&mut app);
            assert!(requests(&app).is_empty());
            assert!(app.world().resource::<Paused>().0);
            assert_eq!(app.world().resource::<PageRequest>().0, Some(Page::Load { selected: 0 }));
        }
    }

    #[test]
    fn an_existing_load_request_is_not_duplicated() {
        let dir = Scratch::new();
        let mut app = app(Some(dir.0.clone()), Some(1.0));
        let path = save(&app, &dir.0, "living", 1, SaveKind::Manual, &body(&app));
        kill(&mut app);
        app.world_mut().write_message(LoadRequest { path: path.clone() });
        tick(&mut app, 1.0);
        assert_eq!(requests(&app), [path]);
        assert!(app.world().resource::<DeathRecovery>().handled);
    }

    #[test]
    fn resurrection_after_a_failed_attempt_rearms_a_later_death() {
        let dir = Scratch::new();
        let mut app = app(Some(dir.0.clone()), Some(1.0));
        let good = body(&app);
        kill(&mut app);
        tick(&mut app, 1.0);
        assert!(requests(&app).is_empty());
        app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.resurrect(FormId::PLAYER_REF, false);
        tick(&mut app, 0.0);
        app.world_mut().resource_mut::<Paused>().0 = false;
        let path = save(&app, &dir.0, "later", 2, SaveKind::Manual, &good);
        kill(&mut app);
        tick(&mut app, 1.0);
        assert_eq!(requests(&app), [path]);
    }

    #[test]
    fn death_recovery_preserves_the_existing_autosave_instead_of_overwriting_it() {
        let dir = Scratch::new();
        let mut app = app(Some(dir.0.clone()), Some(1.0));
        let path = save(&app, &dir.0, "autosave", 1, SaveKind::Auto, &body(&app));
        let original = std::fs::read(&path).unwrap();
        app.init_resource::<crate::loading::Loading>()
            .init_resource::<crate::save::SaveWrites>()
            .insert_resource(SaveQueue(vec![SaveRequest { name: "autosave".into(), kind: SaveKind::Auto }]));
        kill(&mut app);
        super::super::write_saves(app.world_mut());
        assert!(app.world().resource::<SaveQueue>().0.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        tick(&mut app, 1.0);
        assert_eq!(requests(&app).as_slice(), std::slice::from_ref(&path));
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
    include!("death_recovery_assisted_tests.rs");
}
