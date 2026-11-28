//! End-to-end combat boundary tests: real `ActorFire` → `actor_attacks` → `fly` →
//! `bullet_snap::tick` → `ScaledSound` for hitscan `Ghost` flights, non-hitscan
//! ballistic `Flying` bullets, player-owned exclusion, and absent-INI no-op.

use super::*;
use crate::bullet_snap::BulletSnap;
use fnv_combat::bullet_snap::SoundFolder;
use fnv_combat::data::{ProjectileStats, WeaponStats};
use fnv_combat::fx::FxForms;
use fnv_sim::{FormStore, RefTable, SimConfig, World};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const NPC_SHOOTER: FormId = FormId(0x0000_0E01);
const WEAP_SUPERSONIC: FormId = FormId(0x0000_0E10);
const WEAP_SUBSONIC: FormId = FormId(0x0000_0E11);
const PROJ_HITSCAN: FormId = FormId(0x0000_0E20);
const PROJ_SUBSONIC: FormId = FormId(0x0000_0E21);

static SEQ: AtomicU64 = AtomicU64::new(0);

fn build_active_snap(tag: &str, ini: &str) -> (BulletSnap, PathBuf) {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("nvf-combat-snap-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let ini_path = dir.join("NVSE").join("Plugins").join("BulletSnapConfig").join("BulletSnap.ini");
    std::fs::create_dir_all(ini_path.parent().unwrap()).unwrap();
    std::fs::write(&ini_path, ini).unwrap();
    for folder in SoundFolder::ALL {
        let p = dir.join(folder.vfs_folder().replace('\\', "/")).join("snap.wav");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"RIFF").unwrap();
    }
    let vfs = fnv_vfs::Vfs::open(&dir, &[]).unwrap();
    let snap = BulletSnap::from_vfs(&vfs);
    assert!(snap.active());
    (snap, dir)
}

fn combat_snap_app(snap: BulletSnap) -> App {
    let mut forms = CombatForms::default();
    forms.projectiles.insert(
        PROJ_HITSCAN,
        ProjectileStats {
            edid: Some("SnapHitscanProj".into()),
            kind: Some("Missile".into()),
            flags: vec!["Hitscan".into(), "Supersonic".into()],
            speed: 25_000.0,
            range: 20_000.0,
            ..Default::default()
        },
    );
    forms.projectiles.insert(
        PROJ_SUBSONIC,
        ProjectileStats {
            edid: Some("SnapSubsonicProj".into()),
            kind: Some("Missile".into()),
            flags: Vec::new(),
            speed: 12_000.0,
            range: 20_000.0,
            ..Default::default()
        },
    );
    forms.weapons.insert(
        WEAP_SUPERSONIC,
        WeaponStats {
            edid: Some("SnapRifle".into()),
            base_damage: 38,
            projectile: Some(PROJ_HITSCAN),
            projectile_count: 1,
            ..Default::default()
        },
    );
    forms.weapons.insert(
        WEAP_SUBSONIC,
        WeaponStats {
            edid: Some("SnapSubsonicGun".into()),
            base_damage: 50,
            projectile: Some(PROJ_SUBSONIC),
            projectile_count: 1,
            ..Default::default()
        },
    );

    let sim_forms = Box::leak(Box::new(FormStore::default()));
    let sim_refs = Box::leak(Box::new(RefTable::default()));
    let combat = Combat {
        forms,
        rules: Rules::defaults(),
        tuning: CombatTuning::default(),
        weapon: None,
        equipped: None,
        mods: 0,
        silenced_logged: None,
        fists: None,
        ammo: None,
        self_ammo: false,
        melee: false,
        swings: Melee::default(),
        blows: 0,
        boom: Vec::new(),
        meltdowns: HashSet::new(),
        debris: Vec::new(),
        cycle: None,
        kick: Kick::default(),
        flying: Vec::new(),
        dropped_live: Vec::new(),
        settled: Vec::new(),
        landed: Vec::new(),
        fire_landed: Vec::new(),
        splashes: Vec::new(),
        ghosts: Vec::new(),
        looping: None,
        ejects_seen: 0,
        vats_throw: None,
        last_shot: 0.0,
        next_visual: 0,
        pending: Vec::new(),
        script_shots: Vec::new(),
        guarding: HashMap::new(),
        blocked_blows: Vec::new(),
        fresh_reload: false,
        guard_down_until: 0.0,
        marks: Vec::new(),
        rng: SplitMix(1),
        swing_rng: SplitMix(2),
        time: 0.0,
        no_spread: false,
        tracer_hold: None,
        load_magazine: None,
        load_ammo: None,
        chosen_ammo: std::collections::BTreeMap::new(),
        crosshair_cone: None,
        player_spread: None,
        grenade_hold: 0.0,
        grenade_held: false,
        vats_cone: None,
        vats_blast: VatsBlast::default(),
        player_blocked_at: None,
        special_swing: None,
        throw_logged: None,
        spread_perks_logged: None,
        flinch_until: 0.0,
        flinch_end: None,
        flinch_rng: SplitMix(3),
        kick_rng: SplitMix(5),
        kick_just_settled: false,
        kick_left: [0.0; 2],
        kick_back_goal: [0.0; 2],
        kick_back: [crate::ease::Spring::default(); 2],
        spent_rng: SplitMix(4),
        blood_rng: SplitMix(6),
        blood_chance_logged: false,
        localized_dt_dr: false,
    };
    let mut app = App::new();
    app.insert_resource(SimState { forms: sim_forms, refs: sim_refs, world: World::new(sim_forms, sim_refs, SimConfig::default()) })
        .insert_resource(combat)
        .insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 64.0, feet: [0.0, 0.0, 0.0], body_eye: None, body_view: None, placed: None })
        .insert_resource(Sfx::new(FxForms::default()))
        .insert_resource(snap)
        .init_resource::<Time>()
        .init_resource::<PhysState>()
        .init_resource::<CombatStats>()
        .init_resource::<HitFeedback>()
        .init_resource::<ActorParts>()
        .init_resource::<WeaponAnim>()
        .init_resource::<ShotVisuals>()
        .init_resource::<crate::dismember::Dismemberment>()
        .init_resource::<held_grenade_probe::Probe>()
        .add_message::<ActorFire>()
        .add_message::<ActorStrike>()
        .add_message::<ShotFired>()
        .add_message::<PlaySound>()
        .add_message::<ScaledSound>()
        .add_message::<ActorHit>()
        .add_message::<ActorPush>()
        .add_message::<ImpactFx>()
        .add_message::<WeaponBroken>()
        .add_message::<Knockback>()
        .add_message::<ExplosionFx>()
        .add_message::<crate::world_sounds::ImpactPicked>()
        .add_message::<crate::RefChanged>()
        .add_message::<crate::Travel>()
        .add_message::<CasingFx>()
        .add_message::<LoopSound>()
        .add_message::<ForcedHitReaction>()
        .add_systems(Update, (actor_attacks, fly, crate::bullet_snap::tick).chain());
    app
}

fn step_ms(app: &mut App, ms: u64) {
    app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(ms));
    app.update();
}

#[test]
fn combat_fly_emits_single_snap_for_hitscan_supersonic_and_ballistic_subsonic_shots() {
    let (snap, dir) = build_active_snap(
        "fly",
        "[General]\nImpactedPlayerSnap = 0\nDistancedBasedSnap = 1\nSubsonic = 1\nSubsonicThresholdSpeed = 21000\nSnapAngle = 75\n\
         [DistanceBased]\nVeryClose = 768\nClose = 1536\nDistance = 4092\nFar = 8192\n\
         [DamageBased]\nmiddledmg = 32\nhighdmg = 46\n",
    );
    let hc_close = snap.sound_id(SoundFolder::HighCaliberClose).unwrap();
    let ss_mid = snap.sound_id(SoundFolder::SubsonicMid).unwrap();

    let mut app = combat_snap_app(snap);
    let eye_z = app.world().resource::<Player>().eye()[2];

    // Fire 3 shots:
    // 1) NPC hitscan supersonic rifle (dmg 38 -> HighCaliber) passing 300 units from eye (VeryClose).
    // 2) NPC non-hitscan subsonic bullet (speed 12000 < 21000) passing 2000 units from eye (Distance).
    // 3) Player-owned shot passing close -> must NOT snap.
    app.world_mut().write_message_batch([
        ActorFire { shooter: NPC_SHOOTER, weapon: WEAP_SUPERSONIC, origin: [-1500.0, 300.0, eye_z], aim: [1.0, 0.0, 0.0], cone: 0.0, muzzle: None, harmless: false, sure: None },
        ActorFire { shooter: NPC_SHOOTER, weapon: WEAP_SUBSONIC, origin: [-1500.0, 2000.0, eye_z], aim: [1.0, 0.0, 0.0], cone: 0.0, muzzle: None, harmless: false, sure: None },
        ActorFire { shooter: FormId::PLAYER_REF, weapon: WEAP_SUPERSONIC, origin: [-1500.0, 200.0, eye_z], aim: [1.0, 0.0, 0.0], cone: 0.0, muzzle: None, harmless: false, sure: None },
    ]);

    let mut heard: Vec<ScaledSound> = Vec::new();
    for _ in 0..20 {
        step_ms(&mut app, 25); // 25 ms/frame -> hitscan moves 625 u/frame, subsonic 300 u/frame
        heard.extend(app.world_mut().resource_mut::<Messages<ScaledSound>>().drain());
    }

    assert_eq!(heard.len(), 2, "expected exactly 2 snaps (one supersonic hitscan, one subsonic ballistic; player shot excluded): {heard:?}");
    assert!(heard.iter().any(|s| s.sound == hc_close && (s.gain - 1.0).abs() < 1e-5), "missing VeryClose HighCaliber snap: {heard:?}");
    assert!(heard.iter().any(|s| s.sound == ss_mid && (s.gain - 0.65).abs() < 1e-5), "missing Distance Subsonic snap: {heard:?}");

    let stats = app.world().resource::<BulletSnap>().stats;
    assert_eq!(stats.triggered, 2);
    assert_eq!(stats.supersonic, 1);
    assert_eq!(stats.subsonic, 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn combat_fly_emits_zero_snaps_when_ini_absent() {
    let mut app = combat_snap_app(BulletSnap::inactive());
    let eye_z = app.world().resource::<Player>().eye()[2];
    app.world_mut().write_message(ActorFire {
        shooter: NPC_SHOOTER,
        weapon: WEAP_SUPERSONIC,
        origin: [-1500.0, 300.0, eye_z],
        aim: [1.0, 0.0, 0.0],
        cone: 0.0,
        muzzle: None,
        harmless: false,
        sure: None,
    });
    for _ in 0..10 {
        step_ms(&mut app, 25);
        assert!(app.world_mut().resource_mut::<Messages<ScaledSound>>().drain().next().is_none());
    }
    assert_eq!(app.world().resource::<BulletSnap>().stats.triggered, 0);
}
