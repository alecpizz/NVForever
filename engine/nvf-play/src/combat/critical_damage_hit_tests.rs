//! Compiled source edits through real melee/shot producers, physical actor collision,
//! and the Game health/hit consumers. Portable records, no native or scene evidence.
use super::*;
use fnv_game::game::RunCtx;
use fnv_script::vm::XVal;
use fnv_sim::{SimConfig, World};

#[path = "../../../test-fixtures/critical_hit_records.rs"]
mod records;
use records::*;

pub(super) fn query(a: &mut App, body: &str) -> XVal {
    a.world_mut().resource_scope(|world, mut script: Mut<ScriptState>| {
        let mut sim = world.resource_mut::<SimState>();
        with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
            rt.game
                .run_query(&format!("Begin Function {{}}\n{body}\nEnd"), RunCtx::default())
                .unwrap_or_else(|e| panic!("source {body}: {e}"))
        })
    })
}
pub(super) fn app() -> App {
    let plugins = Box::leak(vec![plugin()].into_boxed_slice());
    let script = ScriptState::over_plugins(plugins);
    let game = &script.rt.as_ref().unwrap().game;
    let (forms, refs) = (game.world.forms, game.world.placed);
    let lo = Box::leak(Box::new(fnv_esm::LoadOrder::new(plugins).unwrap()));
    let schema = Box::leak(Box::new(
        fnv_esm::Schema::load_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv")).unwrap(),
    ));
    let mut a = App::new();
    a.insert_resource(script)
        .insert_resource(SimState {
            forms,
            refs,
            world: World::new(forms, refs, SimConfig::default()),
        })
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
        .add_systems(Update, (equip_sync, fire, melee, actor_attacks, fly).chain());
    a.update();
    a.world_mut().resource_scope(|world, mut script: Mut<ScriptState>| {
        let mut sim = world.resource_mut::<SimState>();
        with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
            rt.game.world.get(FormId::PLAYER_REF).unwrap().cell = Some(CELL);
            rt.game.player_entered(Some(CELL));
        });
    });
    query(
        &mut a,
        "Player.SetPos X 0\nPlayer.SetPos Y 0\nPlayer.SetPos Z 0\nPlayer.SetAV CriticalChance 100\nref source\nlet source := StringToRef \"00000A45\"\nsource.SetAV CriticalChance 100\nref target\nlet target := StringToRef \"00000A46\"\ntarget.SetAV Health 1000",
    );
    {
        let mut phys = a.world_mut().resource_mut::<PhysState>();
        phys.world
            .set_actor_volume(TARGET.0, crate::hitvol::volume_for([-15.0, 65.0, 0.0], [15.0, 95.0, 100.0], false));
        phys.world.update_queries();
    }
    a
}
pub(super) fn tick(a: &mut App) -> Vec<ActorHit> {
    a.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f64(1.0 / 60.0));
    a.update();
    a.world_mut().resource_mut::<Messages<ActorHit>>().drain().collect()
}
// Public compiled getters read the actual JipHit recorded by Game::hit; a positive
// health-damage assertion also prevents the getters' absent-record defaults passing.
fn assert_jip_hit(a: &mut App, attacker: FormId, weapon: FormId, damage: f64, critical: bool) -> f64 {
    let mut recorded = |getter: &str| {
        query(
            a,
            &format!(
                "ref struck\nlet struck := StringToRef \"{:08X}\"\nSetFunctionValue (struck.{getter})",
                TARGET.0
            ),
        )
    };
    let XVal::Num(health_damage) = recorded("GetHitHealthDamage") else {
        panic!("JIP hit health damage must be numeric")
    };
    assert!(health_damage > 0.0);
    assert!(
        (health_damage - damage).abs() < 0.001,
        "actual JIP recorder damage equals the health loss"
    );
    assert_eq!(recorded("GetHitExtendedFlag 1"), XVal::Num(f64::from(u8::from(critical))));
    assert_eq!(recorded("GetHitAttacker"), XVal::Ref(attacker.0));
    assert_eq!(recorded("GetHitWeapon"), XVal::Ref(weapon.0));
    let XVal::Num(base_weapon_damage) = recorded("GetHitBaseWeaponDamage") else {
        panic!("JIP hit base weapon damage must be numeric")
    };
    base_weapon_damage
}
fn health(a: &mut App) -> f64 {
    let XVal::Num(h) = query(
        a,
        "ref target\nlet target := StringToRef \"00000A46\"\nSetFunctionValue (target.GetAV Health)",
    ) else {
        panic!("health query")
    };
    h
}
#[derive(Clone, Copy)]
enum Attack {
    PlayerMelee,
    PlayerProjectile,
    NpcProjectile,
}
fn hit(attack: Attack, value: Option<u16>, critical: bool, neighbor: bool) -> (f64, f64) {
    let mut a = app();
    let (weapon, name, shooter) = match (attack, neighbor) {
        (Attack::PlayerMelee, _) => (CLUB, "CriticalHitClub", FormId::PLAYER_REF),
        (_, true) => (NEIGHBOR, "CriticalHitNeighbor", FormId::PLAYER_REF),
        (Attack::PlayerProjectile, _) => (GUN, "CriticalHitGun", FormId::PLAYER_REF),
        (Attack::NpcProjectile, _) => (GUN, "CriticalHitGun", SHOOTER),
    };
    if let Some(value) = value {
        let edited = if neighbor { "CriticalHitGun" } else { name };
        query(&mut a, &format!("SetWeaponCritDamage {value} {edited}"));
    }
    query(
        &mut a,
        &format!(
            "ref source\nlet source := StringToRef \"{:08X}\"\nsource.SetAV CriticalChance {}\nsource.AddItem {name} 10 1",
            shooter.0,
            if critical { 100 } else { 0 }
        ),
    );
    let before = health(&mut a);
    assert!((before - 1000.0).abs() < 0.001);
    let mut events = Vec::new();
    match attack {
        Attack::PlayerMelee => {
            query(&mut a, &format!("Player.EquipItem {name} 1"));
            events.extend(tick(&mut a));
            assert_eq!(a.world().resource::<Combat>().weapon, Some(CLUB));
            assert!(a.world().resource::<Combat>().melee);
            a.world_mut().resource_mut::<WeaponAnim>().ready = true;
            a.world_mut().resource_mut::<PlayerControl>().trigger = true;
            events.extend(tick(&mut a));
            a.world_mut().resource_mut::<PlayerControl>().trigger = false;
        }
        _ => {
            query(
                &mut a,
                &format!(
                    "ref source\nlet source := StringToRef \"{:08X}\"\nsource.FireWeapon {name}",
                    shooter.0
                ),
            );
            events.extend(tick(&mut a));
            assert_eq!(
                a.world().resource::<Combat>().script_shots.len(),
                1,
                "actual compiled FireWeapon request queued"
            );
            events.extend(tick(&mut a));
            let combat = a.world().resource::<Combat>();
            assert_eq!(combat.flying.len(), 1, "actual emitted physical projectile");
            let flight = &combat.flying[0];
            assert_eq!((flight.proj, flight.by.actor, flight.by.weapon), (PROJECTILE, shooter, weapon));
        }
    }
    for _ in 0..180 {
        events.extend(tick(&mut a));
        if !events.is_empty() {
            break;
        }
    }
    let after = health(&mut a);
    assert!(after < before, "real collision must reach Game health");
    assert_eq!(events.len(), 1, "one actual hit, not injected hit messages");
    let event = events[0];
    assert_eq!(
        (event.form_id, event.by, event.weapon, event.location),
        (TARGET.0, Some(shooter), Some(weapon), 0)
    );
    assert!(!event.died && !event.knocked_out);
    {
        let g = &a.world().resource::<ScriptState>().rt.as_ref().unwrap().game;
        let last = g.extra[&TARGET].last_hit.as_ref().unwrap();
        assert_eq!(
            (last.attacker, last.weapon, last.location, last.critical),
            (Some(shooter), Some(weapon), 0, critical)
        );
    }
    let base_weapon_damage = assert_jip_hit(&mut a, shooter, weapon, before - after, critical);
    assert_eq!(
        a.world().resource::<Combat>().forms.weapons[&weapon].crit_damage,
        11,
        "typed record stays unchanged"
    );
    let stats = a.world().resource::<CombatStats>();
    match attack {
        Attack::PlayerMelee => assert_eq!((stats.swings, stats.melee_hits), (1, 1)),
        // FireWeapon uses actor_attacks, not the player's trigger/cycle producer.
        // Its physical flight was checked above; projectiles counts only that cycle.
        _ => assert_eq!((stats.actor_shots, stats.projectiles, stats.actor_hits), (1, 0, 1)),
    }
    (before - after, base_weapon_damage)
}
fn comparisons(attack: Attack) {
    let (baseline, baseline_base) = hit(attack, None, true, false);
    let (eleven, eleven_base) = hit(attack, Some(11), true, false);
    let (thirty_seven, thirty_seven_base) = hit(attack, Some(37), true, false);
    assert!(
        (baseline - eleven).abs() < 0.001,
        "explicit baseline setter retains ordinary record behavior"
    );
    assert!(
        (baseline_base - eleven_base).abs() < 0.001,
        "GetHitBaseWeaponDamage matches baseline crit damage"
    );
    assert!(
        (thirty_seven - eleven - 26.0).abs() < 0.001,
        "same real hit: critical bonus changes by exactly26"
    );
    assert!(
        (thirty_seven_base - eleven_base - 26.0).abs() < 0.001,
        "GetHitBaseWeaponDamage includes SetWeaponCritDamage bonus on crits (+26)"
    );
    let (ordinary_eleven, ordinary_eleven_base) = hit(attack, Some(11), false, false);
    let (ordinary_thirty_seven, ordinary_thirty_seven_base) = hit(attack, Some(37), false, false);
    assert!(
        (ordinary_eleven - ordinary_thirty_seven).abs() < 0.001,
        "noncritical health damage stays unchanged"
    );
    assert!(
        (ordinary_eleven_base - ordinary_thirty_seven_base).abs() < 0.001,
        "noncritical GetHitBaseWeaponDamage excludes crit damage"
    );
    assert!(
        (eleven - ordinary_eleven - 11.0).abs() < 0.001,
        "critical/ordinary baseline control"
    );
    assert!(
        (eleven_base - ordinary_eleven_base - 11.0).abs() < 0.001,
        "GetHitBaseWeaponDamage crit minus non-crit equals crit damage (11)"
    );
}
#[test]
fn compiled_critical_setter_reaches_player_melee_health_and_actor_hit() {
    comparisons(Attack::PlayerMelee);
}
#[test]
fn compiled_critical_setter_reaches_player_projectile_health_and_actor_hit() {
    comparisons(Attack::PlayerProjectile);
    let (baseline, baseline_base) = hit(Attack::PlayerProjectile, None, true, true);
    let (edited_neighbor, edited_neighbor_base) = hit(Attack::PlayerProjectile, Some(37), true, true);
    assert!(
        (baseline - edited_neighbor).abs() < 0.001,
        "another weapon's source edit must not change this hit"
    );
    assert!(
        (baseline_base - edited_neighbor_base).abs() < 0.001,
        "another weapon's source edit must not change GetHitBaseWeaponDamage"
    );
}
#[test]
fn compiled_critical_setter_reaches_npc_projectile_health_and_actor_hit() {
    comparisons(Attack::NpcProjectile);
}

#[test]
fn projectile_and_melee_hits_populate_jip_hit_projectile_base_damage_and_3d_data() {
    // 1. Real projectile hit populates GetHitProjectile (live ref), GetHitBaseWeaponDamage (> 0),
    // GetHit3DData 0/1/2 (3 / 3 / 6 finite values), and clears explosion flag 8.
    let mut a = app();
    query(&mut a, "Player.SetAV CriticalChance 0\nPlayer.AddItem CriticalHitGun 10 1\nPlayer.FireWeapon CriticalHitGun");
    tick(&mut a);
    let mut hits = Vec::new();
    for _ in 0..8 {
        hits.extend(tick(&mut a));
        if !hits.is_empty() {
            break;
        }
    }
    assert_eq!(hits.len(), 1, "projectile must land on target");
    let target_get = |a: &mut App, expr: &str| {
        query(a, &format!("ref struck\nlet struck := StringToRef \"{:08X}\"\nSetFunctionValue ({expr})", TARGET.0))
    };
    let XVal::Ref(proj_ref) = target_get(&mut a, "struck.GetHitProjectile") else {
        panic!("GetHitProjectile must return a ref");
    };
    assert_ne!(proj_ref, 0, "projectile hit must record a live projectile reference");
    assert_eq!(target_get(&mut a, "IsReference (struck.GetHitProjectile)"), XVal::Num(1.0));
    let XVal::Num(base_dmg) = target_get(&mut a, "struck.GetHitBaseWeaponDamage") else {
        panic!("GetHitBaseWeaponDamage must return a number");
    };
    assert!(base_dmg > 0.0, "expected positive base weapon damage, got {base_dmg}");
    assert_eq!(target_get(&mut a, "struck.GetHitFatigueDamage"), XVal::Num(0.0));
    assert_eq!(target_get(&mut a, "struck.GetHitExtendedFlag 8"), XVal::Num(0.0));
    assert_eq!(target_get(&mut a, "Ar_Size (struck.GetHit3DData 0)"), XVal::Num(3.0));
    assert_eq!(target_get(&mut a, "Ar_Size (struck.GetHit3DData 1)"), XVal::Num(3.0));
    assert_eq!(target_get(&mut a, "Ar_Size (struck.GetHit3DData 2)"), XVal::Num(6.0));
    let XVal::Num(hit_y) = target_get(&mut a, "(struck.GetHit3DData 0)[1]") else {
        panic!("3D pos Y must be numeric");
    };
    assert!(hit_y.is_finite() && hit_y > 50.0 && hit_y < 110.0, "hit Y near target volume: {hit_y}");
    // Incoming travel direction from Player (0,0,0) -> Target (0,80,0) is unit +Y (0, 1, 0).
    let (XVal::Num(dx), XVal::Num(dy), XVal::Num(dz)) = (
        target_get(&mut a, "(struck.GetHit3DData 1)[0]"),
        target_get(&mut a, "(struck.GetHit3DData 1)[1]"),
        target_get(&mut a, "(struck.GetHit3DData 1)[2]"),
    ) else {
        panic!("3D dir components must be numeric");
    };
    let dir_norm = (dx * dx + dy * dy + dz * dz).sqrt();
    assert!((dir_norm - 1.0).abs() < 1e-4, "GetHit3DData 1 must be a unit direction vector, got ({dx}, {dy}, {dz}) norm={dir_norm}");
    assert!(dy > 0.99 && dx.abs() < 0.05 && dz.abs() < 0.05, "South->North shot must point along +Y, got ({dx}, {dy}, {dz})");
    let (XVal::Num(b3), XVal::Num(b4), XVal::Num(b5)) = (
        target_get(&mut a, "(struck.GetHit3DData 2)[3]"),
        target_get(&mut a, "(struck.GetHit3DData 2)[4]"),
        target_get(&mut a, "(struck.GetHit3DData 2)[5]"),
    ) else {
        panic!("GetHit3DData 2[3..6] must be numeric");
    };
    assert!((b3 - dx).abs() < 1e-5 && (b4 - dy).abs() < 1e-5 && (b5 - dz).abs() < 1e-5, "GetHit3DData 2[3..6] must equal GetHit3DData 1");

    // 2. Real melee hit clears GetHitProjectile back to 0 while keeping base weapon damage & 3D data.
    query(&mut a, "Player.AddItem CriticalHitClub 1 1\nPlayer.EquipItem CriticalHitClub 1");
    tick(&mut a);
    a.world_mut().resource_mut::<WeaponAnim>().ready = true;
    a.world_mut().resource_mut::<PlayerControl>().trigger = true;
    let mut melee_hits = tick(&mut a);
    a.world_mut().resource_mut::<PlayerControl>().trigger = false;
    for _ in 0..180 {
        if !melee_hits.is_empty() {
            break;
        }
        melee_hits.extend(tick(&mut a));
    }
    assert_eq!(melee_hits.len(), 1, "melee swing must land on target");
    assert_eq!(target_get(&mut a, "struck.GetHitProjectile"), XVal::Ref(0));
    let XVal::Num(melee_base) = target_get(&mut a, "struck.GetHitBaseWeaponDamage") else {
        panic!("melee base weapon damage must be numeric");
    };
    assert!(melee_base > 0.0, "expected positive melee base weapon damage, got {melee_base}");
    assert_eq!(target_get(&mut a, "Ar_Size (struck.GetHit3DData 0)"), XVal::Num(3.0));
    assert_eq!(target_get(&mut a, "Ar_Size (struck.GetHit3DData 2)"), XVal::Num(6.0));
}

#[test]
fn explosion_falloff_reaches_get_hit_base_weapon_damage() {
    // Wave 4D w4d_13_explosion_falloff_outdoor [observed]: GetHitBaseWeaponDamage on
    // explosion hits includes distance attenuation (before difficulty scaling).
    let mut a = app();
    let expl_id = FormId(0x0000_0A99);
    a.world_mut().resource_mut::<Combat>().forms.explosions.insert(
        expl_id,
        fnv_combat::data::ExplosionStats {
            edid: Some("TestFalloffExpl".into()),
            damage: 100.0,
            radius: 400.0,
            ..Default::default()
        },
    );
    let target_get = |a: &mut App, expr: &str| {
        query(a, &format!("ref struck\nlet struck := StringToRef \"{:08X}\"\nSetFunctionValue ({expr})", TARGET.0))
    };

    // 1. Point-blank blast at target center (0, 80, 50) -> full 100.0 base damage.
    a.world_mut().resource_mut::<Combat>().boom.push(QueuedBlast {
        id: expl_id,
        at: [0.0, 80.0, 50.0],
        cause: None,
        normal: Some([0.0, 0.0, 1.0]),
        source: None,
    });
    let close_hits = tick(&mut a);
    assert_eq!(close_hits.len(), 1, "point-blank blast must hit target");
    assert_eq!(target_get(&mut a, "struck.GetHitExtendedFlag 8"), XVal::Num(1.0));
    let XVal::Num(bd_close) = target_get(&mut a, "struck.GetHitBaseWeaponDamage") else {
        panic!("close base damage must be numeric");
    };
    let XVal::Num(hd_close) = target_get(&mut a, "struck.GetHitHealthDamage") else {
        panic!("close health damage must be numeric");
    };
    assert!((bd_close - 100.0).abs() < 1e-3, "point-blank blast GetHitBaseWeaponDamage must be 100.0, got {bd_close}");
    assert!((hd_close - bd_close).abs() < 1e-3, "unarmored normal-difficulty health damage must match base ({hd_close} vs {bd_close})");

    // 2. Distant blast at (0, 280, 50) (d ~ 200u within radius 400u) -> attenuated base damage (< 100.0).
    a.world_mut().resource_mut::<Combat>().boom.push(QueuedBlast {
        id: expl_id,
        at: [0.0, 280.0, 50.0],
        cause: None,
        normal: Some([0.0, 0.0, 1.0]),
        source: None,
    });
    let far_hits = tick(&mut a);
    assert_eq!(far_hits.len(), 1, "distant blast within radius must hit target");
    assert_eq!(target_get(&mut a, "struck.GetHitExtendedFlag 8"), XVal::Num(1.0));
    let XVal::Num(bd_far) = target_get(&mut a, "struck.GetHitBaseWeaponDamage") else {
        panic!("distant base damage must be numeric");
    };
    let XVal::Num(hd_far) = target_get(&mut a, "struck.GetHitHealthDamage") else {
        panic!("distant health damage must be numeric");
    };
    assert!(bd_far > 0.0 && bd_far < 99.0, "distant blast GetHitBaseWeaponDamage must include distance falloff (< 100), got {bd_far}");
    assert!((hd_far - bd_far).abs() < 1e-3, "distant blast health damage must scale from attenuated base ({hd_far} vs {bd_far})");
}
