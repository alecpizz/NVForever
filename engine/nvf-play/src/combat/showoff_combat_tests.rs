//! End-to-end engine boundary tests for ShowOff NVSE `ShowOff:OnFireWeapon` and
//! `ForceWeaponJamAnim` / `ForceJamAnim` playback in `nvf-play`'s live combat systems
//! (`weapon_state`, `fire`, `actor_attacks`, `sync_player_anim_events`, `fly`).

use super::*;
use fnv_combat::data::{AmmoStats, ProjectileStats};
use fnv_game::anim_state::AnimAction;
use fnv_game::game::RunCtx;
use fnv_script::vm::XVal;
use fnv_sim::{SimConfig, World};

const WEAP_PISTOL: FormId = FormId(0x0000_0D10);
const WEAP_REVOLVER_357: FormId = FormId(0x0000_0D11);
const AMMO_10MM: FormId = FormId(0x0000_0D15);
const PROJ_BULLET: FormId = FormId(0x0000_0D20);
const CELL_ID: FormId = FormId(0x0000_0D40);

fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + data.len());
    out.extend_from_slice(sig);
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
    out
}

fn rec(sig: &[u8; 4], id: u32, subs: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = subs.concat();
    let mut out = Vec::with_capacity(24 + body.len());
    out.extend_from_slice(sig);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out.extend_from_slice(&body);
    out
}

fn group(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(24 + body.len());
    out.extend_from_slice(b"GRUP");
    out.extend_from_slice(&((24 + body.len()) as u32).to_le_bytes());
    out.extend_from_slice(&label);
    out.extend_from_slice(&gtype.to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out.extend_from_slice(body);
    out
}

fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
    group(*sig, 0, body)
}

fn z(s: &str) -> Vec<u8> {
    let mut out = s.as_bytes().to_vec();
    out.push(0);
    out
}

fn weap_rec(id: FormId, edid: &str, anim_type: u32, reload_anim: u8, jam_time: f32) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&100i32.to_le_bytes());
    data.extend_from_slice(&100i32.to_le_bytes());
    data.extend_from_slice(&2.0f32.to_le_bytes());
    data.extend_from_slice(&10i16.to_le_bytes());
    data.push(8u8);
    let mut dnam = vec![0u8; 204];
    dnam[0..4].copy_from_slice(&anim_type.to_le_bytes());
    dnam[14] = 1; // ammo_use
    dnam[15] = reload_anim;
    dnam[24..28].copy_from_slice(&PROJ_BULLET.0.to_le_bytes());
    dnam[92..96].copy_from_slice(&1.5f32.to_le_bytes()); // reload_time
    dnam[96..100].copy_from_slice(&jam_time.to_le_bytes()); // jam_time
    rec(
        b"WEAP",
        id.0,
        &[
            sub(b"EDID", &z(edid)),
            sub(b"DATA", &data),
            sub(b"DNAM", &dnam),
            sub(b"NAM0", &AMMO_10MM.0.to_le_bytes()),
        ],
    )
}

fn build_plugin() -> fnv_esm::Plugin {
    let tes4 = rec(b"TES4", 0, &[sub(b"HEDR", &[0, 0, 0x80, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0])]);
    let mut acbs = vec![0u8; 20];
    acbs.extend_from_slice(&0i16.to_le_bytes());
    acbs.extend_from_slice(&0u16.to_le_bytes());
    let npc_data = [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat();
    let player_npc = rec(b"NPC_", 0x7, &[sub(b"EDID", &z("Player")), sub(b"ACBS", &acbs), sub(b"DATA", &npc_data)]);
    let ammo = rec(b"AMMO", AMMO_10MM.0, &[sub(b"EDID", &z("TestAmmo10mm"))]);
    let proj = rec(b"PROJ", PROJ_BULLET.0, &[sub(b"EDID", &z("TestBullet"))]);
    let weaps = [
        weap_rec(WEAP_PISTOL, "ShowOffPistol", 3, 0, 1.4),
        weap_rec(WEAP_REVOLVER_357, "ShowOffRevolver357", 3, 20, 0.0),
    ]
    .concat();
    let achr_data: Vec<u8> = [0.0f32; 6].iter().flat_map(|v| v.to_le_bytes()).collect();
    let player_ref = rec(
        b"ACHR",
        FormId::PLAYER_REF.0,
        &[sub(b"EDID", &z("PlayerREF")), sub(b"NAME", &0x7u32.to_le_bytes()), sub(b"DATA", &achr_data)],
    );
    let cell_body = [
        rec(b"CELL", CELL_ID.0, &[sub(b"EDID", &z("ShowOffCombatCell"))]),
        group(CELL_ID.0.to_le_bytes(), 6, &group(CELL_ID.0.to_le_bytes(), 9, &player_ref)),
    ]
    .concat();
    let bytes = [
        tes4,
        top(b"AMMO", &ammo),
        top(b"PROJ", &proj),
        top(b"WEAP", &weaps),
        top(b"NPC_", &player_npc),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell_body))),
    ]
    .concat();
    fnv_esm::Plugin::parse(&bytes, "ShowOffCombatBridge.esm".into()).unwrap()
}

fn run_nvse(app: &mut App, body: &str) -> XVal {
    app.world_mut().resource_scope(|world, mut script: Mut<ScriptState>| {
        let mut sim = world.resource_mut::<SimState>();
        with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
            rt.game
                .run_query(
                    &format!("Begin Function {{}}\n{body}\nEnd"),
                    RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() },
                )
                .unwrap_or_else(|e| panic!("NVSE `{body}` failed: {e}"))
        })
    })
}

fn nvse_num(app: &mut App, expr: &str) -> f64 {
    match run_nvse(app, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(n) => n,
        v => panic!("{expr}: {v:?}"),
    }
}

fn step_app(app: &mut App, dt: f32) {
    app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(dt));
    app.update();
}

fn bridge_app() -> App {
    let plugins = Box::leak(vec![build_plugin()].into_boxed_slice());
    let script = ScriptState::over_plugins(plugins);
    let game = &script.rt.as_ref().unwrap().game;
    let (forms, refs) = (game.world.forms, game.world.placed);
    let lo = Box::leak(Box::new(fnv_esm::LoadOrder::new(plugins).unwrap()));
    let schema = Box::leak(Box::new(
        fnv_esm::Schema::load_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv")).unwrap(),
    ));

    let mut app = App::new();
    app.insert_resource(script)
        .insert_resource(SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) })
        .insert_resource(PlayWorld(crate::PlayData {
            lo,
            schema,
            index: Box::leak(Box::default()),
            world: 0,
            interior: None,
            eye: [0.0; 3],
            heading_deg: 0.0,
            pitch_deg: 0.0,
            auto_walk: 0.0,
            start_at_cell_spot: false,
            packages: false,
        }))
        .insert_resource(Player {
            heading: 0.0,
            pitch: 0.0,
            eye_height: 64.0,
            feet: [0.0; 3],
            body_eye: None,
            body_view: None,
            placed: None,
        })
        .init_resource::<PlayerControl>()
        .init_resource::<MoveIntent>()
        .init_resource::<PlayerLimbs>()
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
        .add_message::<CasingFx>()
        .add_message::<LoopSound>()
        .add_message::<ForcedHitReaction>()
        .add_systems(Startup, init_combat)
        .add_systems(Update, (weapon_state, fire, actor_attacks, sync_player_anim_events, fly).chain());
    app.update();

    {
        let mut sim = app.world_mut().resource_mut::<SimState>();
        sim.world.add_item(FormId::PLAYER_REF, WEAP_PISTOL, 1).unwrap();
        sim.world.add_item(FormId::PLAYER_REF, WEAP_REVOLVER_357, 1).unwrap();
        sim.world.add_item(FormId::PLAYER_REF, AMMO_10MM, 40).unwrap();
        sim.world.equip(FormId::PLAYER_REF, WEAP_PISTOL).unwrap();
    }
    {
        let mut c = app.world_mut().resource_mut::<Combat>();
        c.forms.ammo.insert(AMMO_10MM, AmmoStats { edid: Some("TestAmmo10mm".into()), ..AmmoStats::default() });
        c.forms.projectiles.insert(PROJ_BULLET, ProjectileStats { speed: 2000.0, range: 5000.0, ..ProjectileStats::default() });
        c.forms.weapons.insert(
            WEAP_PISTOL,
            WeaponStats {
                edid: Some("ShowOffPistol".into()),
                projectile: Some(PROJ_BULLET),
                ammo: Some(AMMO_10MM),
                clip_size: 8,
                ammo_use: 1,
                fire_rate: 10.0,
                attack_shots_per_sec: 10.0,
                reload_time: 1.5,
                jam_time: 1.4,
                reload_anim: Some("ReloadA".into()),
                base_damage: 10,
                health: 100,
                ..WeaponStats::default()
            },
        );
        c.forms.weapons.insert(
            WEAP_REVOLVER_357,
            WeaponStats {
                edid: Some("ShowOffRevolver357".into()),
                projectile: Some(PROJ_BULLET),
                ammo: Some(AMMO_10MM),
                clip_size: 6,
                ammo_use: 1,
                fire_rate: 10.0,
                attack_shots_per_sec: 10.0,
                reload_time: 1.5,
                jam_time: 0.0,
                reload_anim: Some("ReloadX".into()),
                base_damage: 10,
                health: 100,
                ..WeaponStats::default()
            },
        );
    }
    equip_player(&mut app, WEAP_PISTOL, 4);
    app
}

fn equip_player(app: &mut App, weapon: FormId, magazine: u32) {
    {
        let mut sim = app.world_mut().resource_mut::<SimState>();
        sim.world.equip(FormId::PLAYER_REF, weapon).unwrap();
    }
    let w = app.world().resource::<Combat>().forms.weapons[&weapon].clone();
    {
        let mut c = app.world_mut().resource_mut::<Combat>();
        c.weapon = Some(weapon);
        c.equipped = Some(Some(weapon));
        c.ammo = Some(AMMO_10MM);
        let mut cy = Cycle::new(&w, Timing::of(&w, &ClipTiming::default(), &c.tuning), false, 40);
        cy.magazine = magazine;
        c.cycle = Some(cy);
    }
    {
        let mut anim = app.world_mut().resource_mut::<WeaponAnim>();
        anim.ready = true;
        anim.reload = w.reload_anim.clone();
        anim.jam = w.reload_anim.as_deref().and_then(jam_anim);
    }
    app.world_mut().resource_mut::<PlayerControl>().weapon_out = true;
    step_app(app, 0.016);
}

#[test]
fn force_weapon_jam_anim_interrupts_reload_blocks_firing_and_clears() {
    let mut app = bridge_app();

    // Vanilla control: normal firing works cleanly with zero jams.
    app.world_mut().resource_mut::<PlayerControl>().trigger = true;
    step_app(&mut app, 0.05);
    assert_eq!(app.world().resource::<CombatStats>().shots, 1);
    assert_eq!(app.world().resource::<WeaponAnim>().jams, 0);
    assert_eq!(app.world().resource::<CombatStats>().jams, 0);
    app.world_mut().resource_mut::<PlayerControl>().trigger = false;
    step_app(&mut app, 0.2);

    // Register JIP SetOnReloadWeaponEventHandler to confirm it fires once on reload
    // and once when ForceJamAnim enters AnimAction::Jam (2 total, no double-fire on sync).
    run_nvse(
        &mut app,
        "SetOnReloadWeaponEventHandler (Begin Function {ref rW}\n\
           PlayerREF.AuxVarSetFlt \"*jip_reload\" 1 -1\n\
         End) 1 PlayerREF",
    );

    // Start a reload (magazine is 3/8) and verify reload anim state.
    app.world_mut().resource_mut::<PlayerControl>().reload = true;
    step_app(&mut app, 0.05);
    assert!(app.world().resource::<Combat>().cycle.as_ref().unwrap().reloading());
    assert_eq!(nvse_num(&mut app, "PlayerREF.GetAnimAction"), f64::from(AnimAction::Reload.code()));
    assert_eq!(nvse_num(&mut app, "PlayerREF.IsAnimPlayingEx 4"), 1.0);
    assert_eq!(nvse_num(&mut app, "PlayerREF.AuxVarSize \"*jip_reload\""), 1.0);
    assert_eq!(app.world().resource::<Combat>().cycle.as_ref().unwrap().magazine, 3);

    // ForceJamAnim interrupts the reload immediately and enters Phase::Jammed
    // (GetAnimAction == 9 via AnimAction::Jam, IsAnimPlayingEx 4 == 0, GetSequenceAnimGroup 4 == 200 JamA,
    // and SetOnReloadWeaponEventHandler fires once on jam entry [observed 2026-11-24: w6t2_14]).
    assert_eq!(nvse_num(&mut app, "PlayerREF.ForceJamAnim"), 1.0);
    step_app(&mut app, 0.05);
    {
        let c = app.world().resource::<Combat>();
        let cy = c.cycle.as_ref().unwrap();
        assert!(cy.jammed(), "ForceJamAnim must move cycle into Phase::Jammed");
        assert!(!cy.reloading(), "ForceJamAnim must cancel the active reload");
        assert_eq!(cy.magazine, 3, "canceled reload must not refill magazine");
        assert!(c.player_sights_held_down(), "jam holds sights down");
    }
    assert_eq!(app.world().resource::<WeaponAnim>().jams, 1, "WeaponAnim::jams incremented for player_body playback");
    assert_eq!(app.world().resource::<CombatStats>().jams, 1);
    assert_eq!(nvse_num(&mut app, "PlayerREF.GetAnimAction"), f64::from(AnimAction::Jam.code()));
    assert_eq!(nvse_num(&mut app, "PlayerREF.GetAnimAction"), 9.0);
    assert_eq!(nvse_num(&mut app, "PlayerREF.GetSequenceAnimGroup 4"), 200.0);
    assert_eq!(nvse_num(&mut app, "PlayerREF.IsAnimPlayingEx 4"), 0.0, "jam is not counted as reload by IsAnimPlayingEx 4");
    assert_eq!(nvse_num(&mut app, "PlayerREF.AuxVarSize \"*jip_reload\""), 2.0, "jam entry fires SetOnReloadWeaponEventHandler once without double-firing on sync");

    // While jammed, pulling trigger or pressing reload does nothing, and re-calling
    // ForceWeaponJamAnim / ForceJamAnim returns 1 without restarting anim.jams.
    app.world_mut().resource_mut::<PlayerControl>().trigger = true;
    app.world_mut().resource_mut::<PlayerControl>().reload = true;
    assert_eq!(nvse_num(&mut app, "PlayerREF.ForceWeaponJamAnim"), 1.0);
    assert_eq!(nvse_num(&mut app, "PlayerREF.ForceJamAnim"), 1.0);
    step_app(&mut app, 0.2);
    assert_eq!(app.world().resource::<CombatStats>().shots, 1, "cannot fire while jammed");
    assert_eq!(app.world().resource::<WeaponAnim>().jams, 1, "repeated ForceJamAnim while jammed does not restart jam clip");
    assert!(app.world().resource::<Combat>().cycle.as_ref().unwrap().jammed());

    // Advance past the 1.4 s jam_time: jam clears and firing resumes normally.
    app.world_mut().resource_mut::<PlayerControl>().trigger = false;
    step_app(&mut app, 1.3);
    assert!(!app.world().resource::<Combat>().cycle.as_ref().unwrap().jammed(), "jam clears after jam_time");
    assert_eq!(nvse_num(&mut app, "PlayerREF.GetAnimAction"), -1.0);

    app.world_mut().resource_mut::<PlayerControl>().trigger = true;
    step_app(&mut app, 0.05);
    assert_eq!(app.world().resource::<CombatStats>().shots, 2, "weapon fires again after jam clears");
}

#[test]
fn force_weapon_jam_anim_survives_post_script_anim_sync_without_double_reload_event() {
    let mut app = bridge_app();
    run_nvse(
        &mut app,
        "SetOnReloadWeaponEventHandler (Begin Function {ref rW}\n\
           PlayerREF.AuxVarSetFlt \"*jip_reload\" 1 -1\n\
         End) 1 PlayerREF",
    );

    // 1. From idle drawn weapon: ForceWeaponJamAnim queues pending jam and fires once.
    assert_eq!(nvse_num(&mut app, "PlayerREF.ForceWeaponJamAnim"), 1.0);
    assert_eq!(nvse_num(&mut app, "PlayerREF.AuxVarSize \"*jip_reload\""), 1.0);
    assert_eq!(nvse_num(&mut app, "PlayerREF.GetAnimAction"), f64::from(AnimAction::Jam.code()));

    // Simulate post-script animation sync (AnimStatePlugin::tell / bridge_player_anim_state)
    // running before combat::fire consumes force_jam_requests.
    {
        let mut had = true;
        app.world_mut().resource_scope(|world, mut script: Mut<ScriptState>| {
            world.resource_scope(|world, mut sim: Mut<SimState>| {
                let combat = world.resource::<Combat>();
                let anim = world.resource::<WeaponAnim>();
                with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
                    assert!(rt.game.has_pending_force_jam(FormId::PLAYER_REF));
                    bridge_player_anim_state(&mut rt.game, combat, anim, false, false, None, 0.016, &mut had);
                });
            });
        });
    }
    assert_eq!(nvse_num(&mut app, "PlayerREF.GetAnimAction"), f64::from(AnimAction::Jam.code()));
    assert_eq!(nvse_num(&mut app, "PlayerREF.GetSequenceAnimGroup 4"), 200.0);
    assert_eq!(nvse_num(&mut app, "PlayerREF.AuxVarSize \"*jip_reload\""), 1.0);

    // Next frame: combat::fire consumes force_jam_requests -> enters Phase::Jammed without re-firing.
    step_app(&mut app, 0.05);
    assert!(app.world().resource::<Combat>().cycle.as_ref().unwrap().jammed());
    assert_eq!(nvse_num(&mut app, "PlayerREF.GetAnimAction"), f64::from(AnimAction::Jam.code()));
    assert_eq!(
        nvse_num(&mut app, "PlayerREF.AuxVarSize \"*jip_reload\""),
        1.0,
        "SetOnReloadWeaponEventHandler must fire strictly once across ForceWeaponJamAnim -> anim sync -> combat::fire"
    );

    // 2. Let jam finish, start a reload (+1), then ForceWeaponJamAnim (+1), run pre-fire sync, then step_app.
    step_app(&mut app, 1.5);
    assert!(!app.world().resource::<Combat>().cycle.as_ref().unwrap().jammed());
    app.world_mut().resource_mut::<PlayerControl>().reload = true;
    step_app(&mut app, 0.05);
    assert_eq!(nvse_num(&mut app, "PlayerREF.AuxVarSize \"*jip_reload\""), 2.0);
    assert_eq!(nvse_num(&mut app, "PlayerREF.ForceWeaponJamAnim"), 1.0);
    assert_eq!(nvse_num(&mut app, "PlayerREF.AuxVarSize \"*jip_reload\""), 3.0);
    {
        let mut had = true;
        app.world_mut().resource_scope(|world, mut script: Mut<ScriptState>| {
            world.resource_scope(|world, mut sim: Mut<SimState>| {
                let combat = world.resource::<Combat>();
                let anim = world.resource::<WeaponAnim>();
                with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
                    assert!(rt.game.has_pending_force_jam(FormId::PLAYER_REF));
                    bridge_player_anim_state(&mut rt.game, combat, anim, false, false, None, 0.016, &mut had);
                });
            });
        });
    }
    step_app(&mut app, 0.05);
    assert!(app.world().resource::<Combat>().cycle.as_ref().unwrap().jammed());
    assert_eq!(
        nvse_num(&mut app, "PlayerREF.AuxVarSize \"*jip_reload\""),
        3.0,
        "reload + pending-jam sync + combat::fire must not double/triple-fire reload handler"
    );
}

#[test]
fn showoff_on_fire_weapon_engine_dispatch_and_same_frame_force_jam() {
    let mut app = bridge_app();

    // Register ShowOff:OnFireWeapon filtered by 0::Player that records the fired weapon
    // and synchronously invokes ForceJamAnim on the shot.
    run_nvse(
        &mut app,
        "SetEventHandlerAlt \"ShowOff:OnFireWeapon\" (Begin Function {ref rWeap}\n\
           PlayerREF.AuxVarSetRef \"*so_actor\" (GetSelfAlt) -1\n\
           PlayerREF.AuxVarSetRef \"*so_weap\" rWeap -1\n\
           PlayerREF.ForceJamAnim\n\
         End) 0::Player",
    );

    // Pull trigger: ShowOff:OnFireWeapon fires once across fire + fly/projectile_fired,
    // and the synchronous ForceJamAnim jams the weapon on the very same frame.
    app.world_mut().resource_mut::<PlayerControl>().trigger = true;
    step_app(&mut app, 0.05);
    assert_eq!(app.world().resource::<CombatStats>().shots, 1);
    assert_eq!(nvse_num(&mut app, "PlayerREF.AuxVarSize \"*so_weap\""), 1.0, "ShowOff:OnFireWeapon fires once per shot");
    assert_eq!(
        run_nvse(&mut app, "SetFunctionValue (PlayerREF.AuxVarGetRef \"*so_actor\" 0)"),
        XVal::Ref(FormId::PLAYER_REF.0)
    );
    assert_eq!(
        run_nvse(&mut app, "SetFunctionValue (PlayerREF.AuxVarGetRef \"*so_weap\" 0)"),
        XVal::Ref(WEAP_PISTOL.0)
    );
    assert!(app.world().resource::<Combat>().cycle.as_ref().unwrap().jammed(), "same-frame ForceJamAnim from OnFireWeapon jams cycle");
    assert_eq!(app.world().resource::<WeaponAnim>().jams, 1);
    assert_eq!(nvse_num(&mut app, "PlayerREF.GetAnimAction"), f64::from(AnimAction::Jam.code()));
    assert_eq!(nvse_num(&mut app, "PlayerREF.IsAnimPlayingEx 4"), 0.0);

    // Switching to a .357-like weapon (jam_time == 0.0): ShowOff:OnFireWeapon still fires,
    // but ForceJamAnim returns 0 and never jams the revolver.
    app.world_mut().resource_mut::<PlayerControl>().trigger = false;
    equip_player(&mut app, WEAP_REVOLVER_357, 6);
    app.world_mut().resource_mut::<PlayerControl>().trigger = true;
    step_app(&mut app, 0.05);
    assert_eq!(nvse_num(&mut app, "PlayerREF.AuxVarSize \"*so_weap\""), 2.0);
    assert_eq!(
        run_nvse(&mut app, "SetFunctionValue (PlayerREF.AuxVarGetRef \"*so_weap\" 1)"),
        XVal::Ref(WEAP_REVOLVER_357.0)
    );
    assert!(!app.world().resource::<Combat>().cycle.as_ref().unwrap().jammed(), "weapon with jam_time == 0 must not jam");
    assert_eq!(app.world().resource::<WeaponAnim>().jams, 1, "WeaponAnim::jams unchanged for non-jamming weapon");

    // Holstered weapon: ForceJamAnim returns 0 and does not fire later when drawn.
    app.world_mut().resource_mut::<PlayerControl>().trigger = false;
    equip_player(&mut app, WEAP_PISTOL, 8);
    app.world_mut().resource_mut::<WeaponAnim>().ready = false;
    app.world_mut().resource_mut::<PlayerControl>().weapon_out = false;
    step_app(&mut app, 0.016);
    assert_eq!(nvse_num(&mut app, "PlayerREF.ForceJamAnim"), 0.0);
    app.world_mut().resource_mut::<WeaponAnim>().ready = true;
    app.world_mut().resource_mut::<PlayerControl>().weapon_out = true;
    step_app(&mut app, 0.016);
    assert!(!app.world().resource::<Combat>().cycle.as_ref().unwrap().jammed());
    assert_eq!(app.world().resource::<WeaponAnim>().jams, 1);
}
