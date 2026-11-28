//! Live settings, startup/restore scripts and real damage-rule controls.
use super::*;
use crate::{settings::SettingsFile, tweaks::TweakSettings};
use fnv_script::vm::XVal;
use std::path::PathBuf;

struct OwnedInputs {
    root: PathBuf,
    data: PathBuf,
    nvf: PathBuf,
}
impl OwnedInputs {
    fn new() -> Self {
        let base = std::env::var_os("NVF_STEWIE_PROFILE_HOME")
            .expect("set NVF_STEWIE_PROFILE_HOME to a dedicated test directory");
        let base = PathBuf::from(base);
        assert!(
            base.is_absolute(),
            "process-owned fixture root must be absolute"
        );
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = base.join(format!("owned-{}-{stamp}", std::process::id()));
        let data = root.join("Data");
        std::fs::create_dir_all(data.join("NVSE/Plugins")).unwrap();
        std::fs::create_dir(data.join("NVSE/Plugins/scripts")).unwrap();
        std::fs::write(data.join("NVSE/Plugins/scripts/gr_owned_profile.txt"), "SetModLocalData \"profile.restart.rate\" (StewVal \"bCritChanceIgnoresFireRate:Tweaks\")\n").unwrap();
        std::fs::write(data.join("NVSE/Plugins/scripts/ln_owned_profile.txt"), "SetModLocalData \"profile.load.rate\" (StewVal \"bCritChanceIgnoresFireRate:Tweaks\")\n").unwrap();
        let nvf = root.join("NVForever.ini");
        fnv_game::mod_files::set_roots(fnv_game::mod_files::Roots {
            read: vec![data.clone()],
            write: None,
            ..default()
        });
        Self { root, data, nvf }
    }
    fn profile(&self, ignore_rate: u8, scale: f64) {
        std::fs::write(self.data.join("NVSE/Plugins/nvse_stewie_tweaks.ini"), format!("[Tweaks]\nbCritChanceIgnoresFireRate={ignore_rate}\nbScaleCriticalDamage=1\n[Critical Hits]\nfCriticalDamageMult={scale}\n")).unwrap();
    }
}
impl Drop for OwnedInputs {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.data.join("NVSE/Plugins/nvse_stewie_tweaks.ini"));
        let _ = std::fs::remove_file(&self.nvf);
        for name in ["gr_owned_profile.txt", "ln_owned_profile.txt"] {
            let _ = std::fs::remove_file(self.data.join("NVSE/Plugins/scripts").join(name));
        }
        let _ = std::fs::remove_dir(self.data.join("NVSE/Plugins/scripts"));
        for path in [
            self.data.join("NVSE/Plugins"),
            self.data.join("NVSE"),
            self.data.clone(),
            self.root.clone(),
        ] {
            let _ = std::fs::remove_dir(path);
        }
    }
}
fn fresh(settings: Option<&TweakSettings>) -> (Runtime, SimState) {
    let original = ScriptState::over_no_plugins().rt.unwrap();
    let data = original.game.data;
    let forms = original.game.world.forms;
    let refs = original.game.world.placed;
    let mut sim = SimState {
        forms,
        refs,
        world: World::new(forms, refs, fnv_sim::SimConfig::default()),
    };
    let rt = Runtime::new(
        data,
        &mut sim,
        true,
        false,
        settings.map(|settings| &settings.game),
    );
    (rt, sim)
}
fn source(rt: &mut Runtime, body: &str) -> XVal {
    rt.game
        .run_query(
            &format!("Begin Function {{}}\n{body}\nEnd\n"),
            Default::default(),
        )
        .unwrap()
}
fn damage(tweaks: &fnv_game::tweaks::Tweaks) -> fnv_combat::damage::Damage {
    use fnv_combat::{
        damage::{self, Attacker, HitInput, HitPart, NoPerks, Target},
        data::WeaponStats,
        tuning::{CombatTuning, Rules},
    };
    let mut rules = Rules::defaults();
    crate::combat::apply_critical_tweaks(&mut rules, tweaks);
    let weapon = WeaponStats {
        automatic: true,
        fire_rate: 10.0,
        crit_mult: 1.0,
        crit_damage: 20,
        base_damage: 10,
        ..default()
    };
    damage::hit(
        &rules,
        &CombatTuning::default(),
        &HitInput {
            weapon: &weapon,
            projectiles: 1,
            condition: 1.0,
            ammo_effects: &[],
            part: HitPart::default(),
            power: 1.0,
        },
        &Attacker::default(),
        &Target::default(),
        &NoPerks,
        &NoPerks,
        0.0,
    )
}

fn check_rate_cache(rt: &mut Runtime, sim: &mut SimState, moment: Moment, key: &str, expected: f64) {
    let read = format!("SetFunctionValue (GetModLocalData \"{key}\")");
    source(rt, &format!("SetModLocalData \"{key}\" 9"));
    assert_eq!(source(rt, &read), XVal::Num(9.0));
    script_runner(rt, sim, moment);
    assert_eq!(source(rt, &read), XVal::Num(expected), "cache {key}");
}

#[test]
#[ignore = "process-local ModFiles roots: run this exact selector alone with dedicated NVF_STEWIE_PROFILE_HOME and --test-threads=1"]
fn application_defaults_ignore_original_ini_at_startup_and_live_choices_reach_restore_without_version_uplift() {
    let files = OwnedInputs::new();
    let file = SettingsFile(Some(files.nvf.clone()));
    let mut settings = TweakSettings::default();
    let (no_profile, _) = fresh(Some(&settings));
    assert_eq!(no_profile.game.tweaks, fnv_game::tweaks::Tweaks::default());
    let ordinary = damage(&no_profile.game.tweaks);
    assert!(ordinary.critical && ordinary.crit_bonus > 0.0);
    files.profile(1, 1.5);
    let (mut unbound, _) = fresh(None);
    assert_eq!(
        source(
            &mut unbound,
            "SetFunctionValue (StewVal \"bCritChanceIgnoresFireRate:Tweaks\")"
        ),
        XVal::Num(1.0),
        "the original INI admits a contrary value through the unbound query"
    );
    assert_eq!(
        source(
            &mut unbound,
            "SetFunctionValue (StewVal \"fCriticalDamageMult:Critical Hits\")"
        ),
        XVal::Num(1.5)
    );
    let (mut rt, mut sim) = fresh(Some(&settings));
    assert_eq!(rt.game.tweaks, no_profile.game.tweaks);
    assert_eq!(
        source(
            &mut rt,
            "SetFunctionValue (StewVal \"bCritChanceIgnoresFireRate:Tweaks\")"
        ),
        XVal::Num(0.0)
    );
    assert_eq!(
        source(
            &mut rt,
            "SetFunctionValue (StewVal \"fCriticalDamageMult:Critical Hits\")"
        ),
        XVal::Num(1.0)
    );
    check_rate_cache(&mut rt, &mut sim, Moment::Restart, "profile.restart.rate", 0.0);
    check_rate_cache(&mut rt, &mut sim, Moment::NewGame, "profile.load.rate", 0.0);
    let with_profile = damage(&rt.game.tweaks);
    assert_eq!(with_profile.crit_chance, ordinary.crit_chance);
    assert_eq!(with_profile.crit_bonus, ordinary.crit_bonus);
    assert_eq!(with_profile.dealt, ordinary.dealt);
    // Preserve the dependency predicate and real message branch: `< 960` passes now that
    // #3183 is promoted, while unpromoted `< 970` (#3184) still queues the real guard warning.
    assert_eq!(source(&mut rt, "if eval (GetPluginVersion \"lStewieAl's Tweaks\") < 960\nMessageBoxEx \"Unsupported compatibility level\"\nSetFunctionValue 0\nreturn\nendif\nSetFunctionValue 1"), XVal::Num(1.0));
    assert!(rt.game.front_box().is_none(), "promoted 960 gate must not trigger warning");
    assert_eq!(source(&mut rt, "if eval (GetPluginVersion \"lStewieAl's Tweaks\") < 970\nMessageBoxEx \"Unsupported compatibility level\"\nSetFunctionValue 0\nreturn\nendif\nSetFunctionValue 1"), XVal::Num(0.0));
    assert!(
        rt.game.front_box().is_some(),
        "the real guard warning stays queued; it is never suppressed or dismissed"
    );
    assert_eq!(
        source(
            &mut rt,
            "SetFunctionValue (GetPluginVersion \"lStewieAl's Tweaks\")"
        ),
        XVal::Num(960.0)
    );
    let saved = with_world(&mut rt, &mut sim, |rt| rt.game.snapshot());
    let bytes = serde_json::to_vec(&saved).unwrap();
    assert!(!serde_json::from_slice::<serde_json::Value>(&bytes)
        .unwrap()
        .as_object()
        .unwrap()
        .contains_key("tweaks"));
    files.profile(0, 0.5);
    settings.game.crit_ignores_fire_rate = true;
    settings.game.crit_damage_scale = 2.0;
    rt.restore_save(&mut sim, serde_json::from_slice(&bytes).unwrap())
        .unwrap();
    rt.game.bind_stewie_stat_settings(&settings.game);
    check_rate_cache(&mut rt, &mut sim, Moment::Restart, "profile.restart.rate", 1.0);
    check_rate_cache(&mut rt, &mut sim, Moment::Load, "profile.load.rate", 1.0);
    let chosen_hit = damage(&rt.game.tweaks);
    assert_eq!(chosen_hit.crit_chance, ordinary.crit_chance * 10.0);
    assert_eq!(
        chosen_hit.crit_bonus,
        ordinary.crit_bonus * 2.0,
        "saved game defaults do not replace current application settings before Restart"
    );
    assert!(chosen_hit.dealt > ordinary.dealt);
    assert_eq!(rt.game.tweaks.crit_damage_scale, 2.0);
    assert_eq!(
        source(
            &mut rt,
            "SetFunctionValue (StewVal \"bCritChanceIgnoresFireRate:Tweaks\")"
        ),
        XVal::Num(1.0),
        "recognized loaded setting follows actual rules after a runtime original-file edit"
    );
    assert_eq!(
        source(
            &mut rt,
            "SetFunctionValue (StewieGetValue \"fCriticalDamageMult:Critical Hits\")"
        ),
        XVal::Num(2.0)
    );
    assert_eq!(
        source(
            &mut rt,
            "SetFunctionValue (StewVal \"bScaleCriticalDamage:Tweaks\")"
        ),
        XVal::Num(1.0),
        "the unrepresented enable switch retains its original-file value"
    );
    // A real stored NVForever choice reaches both combat and the live query.
    std::fs::write(
        &files.nvf,
        "[Settings]\ntweak-crit-ignores-fire-rate=0\ntweak-crit-damage-scale=0.75\n",
    )
    .unwrap();
    let mut app = App::new();
    app.add_plugins(crate::tweaks::TweaksPlugin);
    app.insert_resource(file);
    crate::settings::apply_stored(app.world_mut());
    let chosen = app.world().resource::<TweakSettings>();
    assert_eq!(
        chosen.game.crit_damage_scale, 1.5,
        "the existing settings reader really applies slider part0.75 as x1.5"
    );
    let (mut chosen_rt, _) = fresh(Some(chosen));
    let chosen_hit = damage(&chosen_rt.game.tweaks);
    assert_eq!(chosen_hit.crit_chance, ordinary.crit_chance);
    assert_eq!(chosen_hit.crit_bonus, ordinary.crit_bonus * 1.5);
    assert!(chosen_hit.dealt > ordinary.dealt);
    assert_eq!(
        source(
            &mut chosen_rt,
            "SetFunctionValue (StewVal \"bCritChanceIgnoresFireRate:Tweaks\")"
        ),
        XVal::Num(0.0)
    );
    assert_eq!(
        source(
            &mut chosen_rt,
            "SetFunctionValue (StewieGetValue \"fCriticalDamageMult:Critical Hits\")"
        ),
        XVal::Num(1.5)
    );
}

#[test]
#[ignore = "process-local ModFiles roots: run this exact selector alone with dedicated NVF_STEWIE_PROFILE_HOME and --test-threads=1"]
fn scheduled_restore_reads_current_settings_at_restart_and_deferred_load() {
    let files = OwnedInputs::new();
    std::fs::write(
        files.data.join("NVSE/Plugins/nvse_stewie_tweaks.ini"),
        "[Tweaks]\nbIndividialItemStats=0\n[Critical Hits]\nfCriticalDamageMult=1.75\n",
    )
    .unwrap();
    std::fs::write(
        &files.nvf,
        "[Settings]\ntweak-single-item-figures=1\ntweak-crit-damage-scale=0.75\n",
    )
    .unwrap();
    for (name, phase) in [("gr_owned_profile.txt", "restart"), ("ln_owned_profile.txt", "load")] {
        std::fs::write(
            files.data.join("NVSE/Plugins/scripts").join(name),
            format!(
                "SetModLocalData \"restore.{phase}.scale\" (StewVal \"fCriticalDamageMult:Critical Hits\")\n\
                 SetModLocalData \"restore.{phase}.single\" (StewieGetValue \"bIndividialItemStats:Tweaks\")\n"
            ),
        )
        .unwrap();
    }

    let mut state = ScriptState::over_no_plugins();
    let mut before = state.rt.take().unwrap();
    assert_eq!(
        source(&mut before, "SetFunctionValue (StewVal \"fCriticalDamageMult:Critical Hits\")"),
        XVal::Num(1.75),
        "the conflicting original INI is visible to a game with no application handoff"
    );
    assert_eq!(
        source(&mut before, "SetFunctionValue (StewieGetValue \"bIndividialItemStats:Tweaks\")"),
        XVal::Num(0.0)
    );
    let (forms, refs) = (before.game.world.forms, before.game.world.placed);
    let saved = before.game.snapshot();
    let (send, recv) = std::sync::mpsc::channel();
    send.send(Ok((before.game.data, 0.0))).unwrap();
    state.loader = Some(Mutex::new(recv));

    use std::sync::LazyLock;
    static LO: LazyLock<fnv_esm::LoadOrder<'static>> =
        LazyLock::new(|| fnv_esm::LoadOrder::new(&[]).unwrap());
    static SCHEMA: LazyLock<fnv_esm::Schema> = LazyLock::new(|| {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv");
        fnv_esm::Schema::load_dir(dir).unwrap()
    });
    static INDEX: LazyLock<fnv_world::WorldIndex> = LazyLock::new(fnv_world::WorldIndex::default);
    let mut app = App::new();
    app.add_plugins(crate::tweaks::TweaksPlugin)
        .insert_resource(SettingsFile(Some(files.nvf.clone())))
        .insert_resource(state)
        .insert_resource(PendingGame(Box::new(saved), files.root.join("owned-restore.nvfsave")))
        .insert_resource(SimState { forms, refs, world: World::new(forms, refs, default()) })
        .insert_resource(PlayWorld(PlayData {
            lo: &LO,
            schema: &SCHEMA,
            index: &INDEX,
            world: 0,
            interior: None,
            eye: [0.0; 3],
            heading_deg: 0.0,
            pitch_deg: 0.0,
            auto_walk: 0.0,
            start_at_cell_spot: false,
            packages: false,
        }))
        .insert_resource(Location { world: 0, interior: None })
        .init_resource::<Time>()
        .init_resource::<crate::pause::PlayClock>()
        .init_resource::<crate::pause::Paused>()
        .init_resource::<crate::pause::Page>()
        .init_resource::<crate::loading::Loading>()
        .init_resource::<crate::tiles::mods::ModsPage>()
        .init_resource::<PhysState>()
        .init_resource::<ScriptHud>()
        .init_resource::<crate::corner_messages::CornerMessages>()
        .init_resource::<crate::quest_messages::QuestMessages>()
        .add_message::<RefChanged>()
        .add_message::<crate::fade::RefFade>()
        .add_message::<crate::RefRepose>()
        .add_message::<TravelRequest>()
        .add_message::<OpenContainer>()
        .add_message::<ItemUsed>()
        .add_message::<crate::furniture::UseFurniture>()
        .add_message::<crate::terminal::OpenTerminal>()
        .add_message::<crate::save::LoadRefused>();

    let world = app.world_mut();
    crate::settings::apply_stored(world);
    let mut schedule = Schedule::default();
    schedule.add_systems((take_pending_game, run_scripts).chain());
    schedule.run(world);

    let read = |rt: &mut Runtime, key: &str| {
        source(rt, &format!("SetFunctionValue (GetModLocalData \"restore.{key}\")"))
    };
    {
        let mut script = world.resource_mut::<ScriptState>();
        let rt = script.rt.as_mut().unwrap();
        assert_eq!(rt.game.save_files.loaded.as_ref().unwrap().name, "owned-restore");
        assert_eq!(read(rt, "restart.scale"), XVal::Num(1.5), "restored Restart scale");
        assert_eq!(read(rt, "restart.single"), XVal::Num(1.0), "restored Restart inventory");
        assert_eq!(read(rt, "load.scale"), XVal::Num(0.0), "Load still waits for the player");
        assert_eq!(read(rt, "load.single"), XVal::Num(0.0), "Load still waits for the player");
    }

    // Advance the real system past its player gate, without a render or physics schedule.
    let _ = world.resource_mut::<PhysState>().place_player([0.0; 3], 60.0, 1000.0);
    world.resource_mut::<Time>().advance_by(std::time::Duration::from_millis(16));
    schedule.run(world);
    let mut script = world.resource_mut::<ScriptState>();
    let rt = script.rt.as_mut().unwrap();
    assert_eq!(read(rt, "load.scale"), XVal::Num(1.5), "deferred Load scale");
    assert_eq!(read(rt, "load.single"), XVal::Num(1.0), "deferred Load inventory");
}

mod current_profile;
mod inventory_settings;
