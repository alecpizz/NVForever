//! Authored portable records and real Game queries through actor_attacks -> fly.
//! CPU source/consumer controls; no installed script, native code or graphics assets.
use super::*;
use fnv_esm::raw::{Subrecord, write_subrecords};
use fnv_game::game::RunCtx;
use fnv_script::vm::XVal;
use fnv_sim::{World, SimConfig};

const GUN: FormId = FormId(0x900);
const NEIGHBOR_GUN: FormId = FormId(0x901);
const AMMO: FormId = FormId(0x902);
const PROJ: FormId = FormId(0x904);
const NEIGHBOR: FormId = FormId(0x905);
const CELL: FormId = FormId(0x906);

fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord { Subrecord { sig: *sig, data, extended: false } }
fn z(text: &str) -> Vec<u8> { [text.as_bytes(), &[0]].concat() }
fn record(sig: &[u8; 4], flags: u32, id: u32, subs: &[Subrecord]) -> Vec<u8> {
    let body = write_subrecords(subs);
    [&sig[..], &(body.len() as u32).to_le_bytes(), &flags.to_le_bytes(), &id.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &body].concat()
}
fn group(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &sig[..], &0i32.to_le_bytes(), &[0; 8], body].concat()
}
fn weapon(id: FormId, name: &str, ammo: Option<FormId>, projectile: FormId) -> Vec<u8> {
    // Schema WeaponDNAM: animation type at0, projectile at36, count at42.
    let mut dnam = vec![0; 204];
    dnam[..4].copy_from_slice(&10u32.to_le_bytes());
    dnam[36..40].copy_from_slice(&projectile.0.to_le_bytes());
    dnam[42] = 1;
    let mut subs = vec![sub(b"EDID", z(name)), sub(b"DNAM", dnam), sub(b"DATA", [0i32.to_le_bytes().to_vec(), 100i32.to_le_bytes().to_vec(), 1f32.to_le_bytes().to_vec(), 10i16.to_le_bytes().to_vec(), vec![1]].concat())];
    if let Some(a) = ammo { subs.push(sub(b"NAM0", a.0.to_le_bytes().to_vec())); }
    record(b"WEAP", 0, id.0, &subs)
}
fn projectile(id: FormId, name: &str, speed: f32, force: f32) -> Vec<u8> {
    // Schema ProjectileData: flags/type at0/2, speed at8, range at12, force at52.
    let mut data = vec![0; 84];
    data[2..4].copy_from_slice(&1u16.to_le_bytes());
    data[8..12].copy_from_slice(&speed.to_le_bytes());
    data[12..16].copy_from_slice(&5000f32.to_le_bytes());
    data[52..56].copy_from_slice(&force.to_le_bytes());
    record(b"PROJ", 0, id.0, &[sub(b"EDID", z(name)), sub(b"DATA", data)])
}
fn plugin() -> fnv_esm::Plugin {
    let header = record(b"TES4", 1, 0, &[sub(b"HEDR", [1.34f32.to_le_bytes(), 0u32.to_le_bytes(), 0x1000u32.to_le_bytes()].concat())]);
    let player = record(b"NPC_", 0, FormId::PLAYER_BASE.0, &[sub(b"EDID", z("Player")), sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat())]);
    let weapons = [weapon(GUN, "OwnedGun", Some(AMMO), NEIGHBOR), weapon(NEIGHBOR_GUN, "NeighborGun", None, NEIGHBOR)].concat();
    let ammo = record(b"AMMO", 0, AMMO.0, &[sub(b"EDID", z("OwnedAmmo")), sub(b"DAT2", [1u32.to_le_bytes().to_vec(), PROJ.0.to_le_bytes().to_vec(), 0f32.to_le_bytes().to_vec(), 0u32.to_le_bytes().to_vec(), 1f32.to_le_bytes().to_vec()].concat())]);
    let cell = record(b"CELL", 0, CELL.0, &[sub(b"EDID", z("OwnedCell")), sub(b"DATA", vec![1])]);
    let projectiles = [projectile(PROJ, "OwnedProjectile", 1000.0, 10.0), projectile(NEIGHBOR, "NeighborProjectile", 1500.0, 20.0)].concat();
    fnv_esm::Plugin::parse(&[header, group(b"NPC_", &player), group(b"WEAP", &weapons), group(b"AMMO", &ammo), group(b"PROJ", &projectiles), group(b"CELL", &cell)].concat(), "OwnedLaunch.esm".into()).unwrap()
}
fn app() -> App {
    let plugins = Box::leak(vec![plugin()].into_boxed_slice());
    let state = ScriptState::over_plugins(plugins);
    let game = &state.rt.as_ref().unwrap().game;
    let (forms, refs) = (game.world.forms, game.world.placed);
    let lo = Box::leak(Box::new(fnv_esm::LoadOrder::new(plugins).unwrap()));
    let schema = Box::leak(Box::new(fnv_esm::Schema::load_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv")).unwrap()));
    let mut app = App::new();
    app.insert_resource(state)
        .insert_resource(SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) })
        .insert_resource(PlayWorld(crate::PlayData { lo, schema, index: Box::leak(Box::default()), world: 0, interior: None, eye: [0.0; 3], heading_deg: 0.0, pitch_deg: 0.0, auto_walk: 0.0, start_at_cell_spot: false, packages: false }))
        .init_resource::<Time>().init_resource::<PhysState>().init_resource::<CombatStats>()
        .init_resource::<HitFeedback>().init_resource::<ActorParts>().init_resource::<WeaponAnim>()
        .init_resource::<ShotVisuals>().init_resource::<crate::dismember::Dismemberment>()
        .init_resource::<held_grenade_probe::Probe>()
        .add_message::<ActorFire>().add_message::<ActorStrike>().add_message::<ShotFired>()
        .add_message::<PlaySound>().add_message::<crate::sfx::ScaledSound>().add_message::<ActorHit>().add_message::<ActorPush>()
        .add_message::<ImpactFx>().add_message::<WeaponBroken>().add_message::<Knockback>()
        .add_message::<ExplosionFx>().add_message::<crate::world_sounds::ImpactPicked>()
        .add_message::<crate::RefChanged>()
        .add_systems(Startup, init_combat).add_systems(Update, (actor_attacks, fly).chain());
    app.update();
    app.world_mut().resource_scope(|world, mut script: Mut<ScriptState>| {
        let mut sim = world.resource_mut::<SimState>();
        with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
            rt.game.world.get(FormId::PLAYER_REF).unwrap().cell = Some(CELL);
            rt.game.player_entered(Some(CELL));
        });
    });
    query(&mut app, "Player.SetPos X 10\nPlayer.SetPos Y 20\nPlayer.SetPos Z 30\nPlayer.AddItem OwnedGun 20 1");
    app
}
fn query(app: &mut App, body: &str) -> XVal {
    app.world_mut().resource_scope(|world, mut script: Mut<ScriptState>| {
        let mut sim = world.resource_mut::<SimState>();
        with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| rt.game.run_query(&format!("Begin Function {{}}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("authored source query failed: {body}: {e}")))
    })
}
fn tick(app: &mut App) {
    app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f64(1.0 / 60.0));
    app.update();
}
fn fire(app: &mut App, weapon: &str, remove: bool) {
    let before = app.world().resource::<Combat>().flying.len();
    query(app, &format!("Player.FireWeapon {weapon}{}", if remove { "\nPlayer.RemoveItem OwnedGun 1 1" } else { "" }));
    tick(app);
    assert_eq!(app.world().resource::<Combat>().flying.len(), before, "FireWeapon retains the current next-frame scheduling policy");
    assert_eq!(app.world().resource::<Combat>().script_shots.len(), 1);
    tick(app);
    assert_eq!(app.world().resource::<Combat>().flying.len(), before + 1, "one physical projectile, not a getter-only pass");
    assert!(app.world().resource::<Combat>().script_shots.is_empty());
}
fn last(app: &App, id: FormId, speed: f32, force: f32) {
    let f = app.world().resource::<Combat>().flying.last().unwrap();
    assert_eq!(f.proj, id, "requested projectile identity");
    assert!((f.p.speed - speed).abs() < 0.001, "actual initial speed {:?}", f.p);
    assert!((vec::len(f.p.vel) - speed.abs()).abs() < 0.001, "actual physical velocity");
    assert!((f.force - force).abs() < 0.001, "force carried to actual hit path");
    assert_eq!(f.origin, [10.0, 20.0, 94.0], "actual helper origin policy retained");
}
fn typed_unchanged(app: &App) {
    let c = app.world().resource::<Combat>();
    assert_eq!((c.forms.projectiles[&PROJ].speed, c.forms.projectiles[&PROJ].impact_force), (1000.0, 10.0));
    assert_eq!((c.forms.projectiles[&NEIGHBOR].speed, c.forms.projectiles[&NEIGHBOR].impact_force), (1500.0, 20.0));
}

#[test]
fn fireweapon_uses_session_speed_force_and_keeps_typed_neighbors() {
    let mut a = app();
    query(&mut a, "SetProjectileTraitNumeric OwnedProjectile 2 500\nSetProjectileTraitNumeric OwnedProjectile 9 10.1\nSetProjectileTraitNumeric OwnedAmmo 2 17\nSetProjectileTraitNumeric OwnedProjectile 99 17");
    let g = &a.world().resource::<ScriptState>().rt.as_ref().unwrap().game;
    assert_eq!(g.num_field(PROJ, "DATA", "speed"), 500.0);
    fire(&mut a, "OwnedGun", true);
    last(&a, PROJ, 500.0, 10.1);
    let f = a.world().resource::<Combat>().flying.last().unwrap();
    let reference = f.rref.expect("actual temporary shot reference");
    assert!(a.world().resource::<ScriptState>().rt.as_ref().unwrap().game.gaps.projs.shots.contains_key(&reference));
    assert_eq!(a.world().resource::<SimState>().world.peek(FormId::PLAYER_REF).unwrap().inventory.as_ref().unwrap().count(GUN), 19);
    fire(&mut a, "NeighborGun", false);
    last(&a, NEIGHBOR, 1500.0, 20.0);
    typed_unchanged(&a);
}

#[test]
fn restoring_traits_changes_next_launch_without_rewriting_existing_flight() {
    let mut a = app();
    query(&mut a, "SetProjectileTraitNumeric OwnedProjectile 2 500\nSetProjectileTraitNumeric OwnedProjectile 9 7");
    fire(&mut a, "OwnedGun", false);
    let first = a.world().resource::<Combat>().flying.last().unwrap().id;
    query(&mut a, "SetProjectileTraitNumeric OwnedProjectile 2 1000\nSetProjectileTraitNumeric OwnedProjectile 9 10");
    fire(&mut a, "OwnedGun", false);
    last(&a, PROJ, 1000.0, 10.0);
    let old = a.world().resource::<Combat>().flying.iter().find(|f| f.id == first).unwrap();
    assert_eq!((old.p.speed, old.force), (500.0, 7.0));
    typed_unchanged(&a);
}

#[test]
fn temporary_projectile_aliases_snapshot_at_creation_and_isolate_post_clone_edits() {
    let mut a = app();
    // Clones created before OwnedProjectile is edited snapshot the authored (1000.0, 10.0)
    // traits and remain isolated from later edits on OwnedProjectile.
    let XVal::Ref(own) = query(&mut a, "ref r\nlet r := TempCloneForm OwnedProjectile 1\nSetFunctionValue r") else { panic!("actual temporary clone") };
    let XVal::Ref(sibling) = query(&mut a, "ref r\nlet r := TempCloneForm OwnedProjectile 1\nSetFunctionValue r") else { panic!("actual sibling clone") };
    assert_ne!(own, sibling);
    query(&mut a, &format!("SetProjectileTraitNumeric OwnedProjectile 2 800\nSetProjectileTraitNumeric OwnedProjectile 9 11\nSetProjectileTraitNumeric {own:#010X} 2 350\nSetProjectileTraitNumeric {own:#010X} 9 7\nSetAmmoProjectile OwnedAmmo {own:#010X}"));
    fire(&mut a, "OwnedGun", false);
    last(&a, FormId(own), 350.0, 7.0);
    // sibling was cloned before OwnedProjectile was mutated to (800, 11), so it keeps (1000, 10).
    query(&mut a, &format!("SetAmmoProjectile OwnedAmmo {sibling:#010X}"));
    fire(&mut a, "OwnedGun", false);
    last(&a, FormId(sibling), 1000.0, 10.0);
    // A clone created AFTER OwnedProjectile was mutated inherits (800, 11) and stays isolated
    // when OwnedProjectile is mutated again to (600, 9).
    let XVal::Ref(after_edit) = query(&mut a, "ref r\nlet r := TempCloneForm OwnedProjectile 1\nSetFunctionValue r") else { panic!("actual post-edit clone") };
    query(&mut a, &format!("SetProjectileTraitNumeric OwnedProjectile 2 600\nSetProjectileTraitNumeric OwnedProjectile 9 9\nSetAmmoProjectile OwnedAmmo {after_edit:#010X}"));
    fire(&mut a, "OwnedGun", false);
    last(&a, FormId(after_edit), 800.0, 11.0);
    query(&mut a, "SetAmmoProjectile OwnedAmmo OwnedProjectile");
    fire(&mut a, "OwnedGun", false);
    last(&a, PROJ, 600.0, 9.0);
    // This initial-launch control does not qualify cloned-shot reference lifetime,
    // its visual assets or save eligibility (separate producer admission boundary).
    typed_unchanged(&a);
}

#[test]
fn clearing_ammo_projectile_falls_back_to_weapon_without_leaking_neighbor_edits() {
    let mut a = app();
    query(&mut a, "SetProjectileTraitNumeric OwnedProjectile 2 500\nSetProjectileTraitNumeric OwnedProjectile 9 7\nSetAmmoProjectile OwnedAmmo");
    fire(&mut a, "OwnedGun", false);
    last(&a, NEIGHBOR, 1500.0, 20.0);
    query(&mut a, "SetAmmoProjectile OwnedAmmo OwnedProjectile");
    fire(&mut a, "OwnedGun", false);
    last(&a, PROJ, 500.0, 7.0);
    typed_unchanged(&a);
}

#[test]
fn ordinary_actor_shot_without_script_runtime_preserves_typed_values() {
    let mut a = app();
    a.world_mut().remove_resource::<ScriptState>();
    a.world_mut().write_message(ActorFire { shooter: FormId::PLAYER_REF, weapon: GUN, origin: [10.0, 20.0, 94.0], aim: [0.0, 1.0, 0.0], cone: 0.0, muzzle: None, harmless: false, sure: None });
    tick(&mut a);
    last(&a, PROJ, 1000.0, 10.0);
    typed_unchanged(&a);
}

// Independently authored consumer controls. Notes were recorded before this code in
// NOTES-BEFORE.md; the five R2 controls above are preserved byte for byte.
const MISSING_GUN: FormId = FormId(0x908);
const EMPTY_GUN: FormId = FormId(0x909);
const ABSENT_PROJECTILE: FormId = FormId(0x9f0);

fn consumer_weapon(id: FormId, name: &str, ammo: Option<FormId>, projectile: FormId) -> Vec<u8> {
    // Exact main schema WeaponDNAM: 3 is Pistol - Ballistic (1 Hand). The original
    // R2 throwing-weapon helper remains unchanged for its original controls.
    let mut dnam = vec![0; 204];
    dnam[..4].copy_from_slice(&3u32.to_le_bytes());
    // Ammo Use 1 (schema WeaponDNAM byte 14): an ordinary gun spends a round a shot; 0
    // would spend none (#3023).
    dnam[14] = 1;
    dnam[36..40].copy_from_slice(&projectile.0.to_le_bytes());
    dnam[42] = 1;
    let mut subs = vec![sub(b"EDID", z(name)), sub(b"DNAM", dnam), sub(b"DATA", [0i32.to_le_bytes().to_vec(), 100i32.to_le_bytes().to_vec(), 1f32.to_le_bytes().to_vec(), 10i16.to_le_bytes().to_vec(), vec![1]].concat())];
    if let Some(a) = ammo { subs.push(sub(b"NAM0", a.0.to_le_bytes().to_vec())); }
    record(b"WEAP", 0, id.0, &subs)
}

fn consumer_plugin() -> fnv_esm::Plugin {
    let header = record(b"TES4", 1, 0, &[sub(b"HEDR", [1.34f32.to_le_bytes(), 0u32.to_le_bytes(), 0x1000u32.to_le_bytes()].concat())]);
    let player = record(b"NPC_", 0, FormId::PLAYER_BASE.0, &[sub(b"EDID", z("Player")), sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat())]);
    let weapons = [
        consumer_weapon(GUN, "OwnedGun", Some(AMMO), NEIGHBOR),
        consumer_weapon(NEIGHBOR_GUN, "NeighborGun", None, NEIGHBOR),
        consumer_weapon(MISSING_GUN, "MissingProjectileGun", None, ABSENT_PROJECTILE),
        consumer_weapon(EMPTY_GUN, "NoProjectileGun", None, FormId(0)),
    ].concat();
    let ammo = record(b"AMMO", 0, AMMO.0, &[sub(b"EDID", z("OwnedAmmo")), sub(b"DAT2", [1u32.to_le_bytes().to_vec(), PROJ.0.to_le_bytes().to_vec(), 0f32.to_le_bytes().to_vec(), 0u32.to_le_bytes().to_vec(), 1f32.to_le_bytes().to_vec()].concat())]);
    let cell = record(b"CELL", 0, CELL.0, &[sub(b"EDID", z("OwnedCell")), sub(b"DATA", vec![1])]);
    let projectiles = [projectile(PROJ, "OwnedProjectile", 1000.0, 10.0), projectile(NEIGHBOR, "NeighborProjectile", 1500.0, 20.0)].concat();
    // No record for ABSENT_PROJECTILE: selection must survive the absent typed entry.
    fnv_esm::Plugin::parse(&[header, group(b"NPC_", &player), group(b"WEAP", &weapons), group(b"AMMO", &ammo), group(b"PROJ", &projectiles), group(b"CELL", &cell)].concat(), "OwnedConsumerControls.esm".into()).unwrap()
}

fn consumer_app() -> App {
    // The normal importer creates CombatForms; no test mutates that cache to arrange
    // a missing projectile, projectile selection, or an edited launch value.
    let plugins = Box::leak(vec![consumer_plugin()].into_boxed_slice());
    let state = ScriptState::over_plugins(plugins);
    let game = &state.rt.as_ref().unwrap().game;
    let (forms, refs) = (game.world.forms, game.world.placed);
    let lo = Box::leak(Box::new(fnv_esm::LoadOrder::new(plugins).unwrap()));
    let schema = Box::leak(Box::new(fnv_esm::Schema::load_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/fnv")).unwrap()));
    let mut app = App::new();
    app.insert_resource(state)
        .insert_resource(SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) })
        .insert_resource(PlayWorld(crate::PlayData { lo, schema, index: Box::leak(Box::default()), world: 0, interior: None, eye: [0.0; 3], heading_deg: 0.0, pitch_deg: 0.0, auto_walk: 0.0, start_at_cell_spot: false, packages: false }))
        .init_resource::<Time>().init_resource::<PhysState>().init_resource::<CombatStats>()
        .init_resource::<HitFeedback>().init_resource::<ActorParts>().init_resource::<WeaponAnim>()
        .init_resource::<ShotVisuals>().init_resource::<crate::dismember::Dismemberment>()
        .init_resource::<held_grenade_probe::Probe>()
        .add_message::<ActorFire>().add_message::<ActorStrike>().add_message::<ShotFired>()
        .add_message::<PlaySound>().add_message::<crate::sfx::ScaledSound>().add_message::<ActorHit>().add_message::<ActorPush>()
        .add_message::<ImpactFx>().add_message::<WeaponBroken>().add_message::<Knockback>()
        .add_message::<ExplosionFx>().add_message::<crate::world_sounds::ImpactPicked>()
        .add_message::<crate::RefChanged>()
        .add_systems(Startup, init_combat).add_systems(Update, (actor_attacks, fly).chain());
    app.update();
    app.world_mut().resource_scope(|world, mut script: Mut<ScriptState>| {
        let mut sim = world.resource_mut::<SimState>();
        with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| {
            rt.game.world.get(FormId::PLAYER_REF).unwrap().cell = Some(CELL);
            rt.game.player_entered(Some(CELL));
        });
    });
    query(&mut app, "Player.SetPos X 10\nPlayer.SetPos Y 20\nPlayer.SetPos Z 30\nPlayer.AddItem OwnedGun 20 1");
    app
}

fn request_actor_shot(app: &mut App, weapon: FormId) {
    app.world_mut().write_message(ActorFire { shooter: FormId::PLAYER_REF, weapon, origin: [10.0, 20.0, 94.0], aim: [0.0, 1.0, 0.0], cone: 0.0, muzzle: None, harmless: false, sure: None });
}

fn take_fired(app: &mut App) -> Vec<ShotFired> {
    app.world_mut().resource_mut::<bevy::ecs::message::Messages<ShotFired>>().drain().collect()
}

#[test]
fn selected_missing_typed_projectile_keeps_actor_counter_and_effect_without_flight() {
    let mut a = consumer_app();
    {
        let c = a.world().resource::<Combat>();
        assert_eq!(c.forms.chain(MISSING_GUN, None).unwrap().projectile, Some(ABSENT_PROJECTILE), "actual schema/importer preserves the selected ID");
        assert!(!c.forms.projectiles.contains_key(&ABSENT_PROJECTILE));
        assert!(c.flying.is_empty());
    }
    assert!(take_fired(&mut a).is_empty());
    let before = a.world().resource::<CombatStats>().clone();
    request_actor_shot(&mut a, MISSING_GUN);
    tick(&mut a);
    let stats = a.world().resource::<CombatStats>();
    assert_eq!(stats.actor_shots, before.actor_shots + 1, "the selection precedes the absent typed physical record");
    assert_eq!(stats.shots, before.shots);
    assert_eq!(stats.projectiles, before.projectiles);
    assert!(a.world().resource::<Combat>().flying.is_empty());
    let events = take_fired(&mut a);
    assert_eq!(events.len(), 1, "the existing renderer/detection effect message still occurs");
    assert_eq!((events[0].projectile, events[0].weapon, events[0].shooter), (ABSENT_PROJECTILE, MISSING_GUN, FormId::PLAYER_REF));
    assert_eq!(events[0].at, Some([10.0, 20.0, 94.0]));
    assert_eq!(events[0].dir, Some([0.0, 1.0, 0.0]));
    typed_unchanged(&a);
}

#[test]
fn no_selected_projectile_stays_silent_while_neighbor_still_launches() {
    let mut a = consumer_app();
    assert_eq!(a.world().resource::<Combat>().forms.chain(EMPTY_GUN, None).unwrap().projectile, None);
    assert!(take_fired(&mut a).is_empty());
    let before = a.world().resource::<CombatStats>().clone();
    request_actor_shot(&mut a, EMPTY_GUN);
    tick(&mut a);
    assert_eq!(a.world().resource::<CombatStats>().actor_shots, before.actor_shots);
    assert_eq!(a.world().resource::<CombatStats>().shots, before.shots);
    assert!(a.world().resource::<Combat>().flying.is_empty());
    assert!(take_fired(&mut a).is_empty(), "absence of a selection retains the earlier no-shot boundary");
    request_actor_shot(&mut a, NEIGHBOR_GUN);
    tick(&mut a);
    assert_eq!(a.world().resource::<CombatStats>().actor_shots, before.actor_shots + 1);
    assert_eq!(a.world().resource::<Combat>().flying.len(), 1);
    last(&a, NEIGHBOR, 1500.0, 20.0);
    let events = take_fired(&mut a);
    assert_eq!(events.len(), 1);
    assert_eq!((events[0].projectile, events[0].weapon), (NEIGHBOR, NEIGHBOR_GUN));
    typed_unchanged(&a);
}

#[test]
fn normal_player_attack_uses_session_traits_and_preserves_ammo_counters_and_effect() {
    let mut a = consumer_app();
    assert!(!god_mode(), "this authored ammo-consumption control requires ordinary play policy");
    a.insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 64.0, feet: [10.0, 20.0, 30.0], body_eye: None, body_view: None, placed: None })
        .init_resource::<PlayerControl>().init_resource::<PlayerLimbs>()
        .add_message::<CasingFx>().add_message::<LoopSound>()
        .add_systems(Update, (equip_sync, super::fire).chain().before(actor_attacks));
    // Actual Game setters and inventory/equipment APIs; no FireWeapon request and no
    // manually injected Cycle::Fired, Flying, edited CombatForms, or counter state.
    query(&mut a, "Player.AddItem OwnedAmmo 3 1\nPlayer.EquipItem OwnedGun 1\nSetProjectileTraitNumeric OwnedProjectile 2 500\nSetProjectileTraitNumeric OwnedProjectile 9 7");
    assert_eq!(query(&mut a, "SetFunctionValue (GetProjectileTraitNumeric OwnedProjectile 2)"), XVal::Num(500.0));
    assert_eq!(query(&mut a, "SetFunctionValue (Player.GetItemCount OwnedAmmo)"), XVal::Num(3.0));
    a.world_mut().resource_mut::<WeaponAnim>().ready = true;
    tick(&mut a); // real equip_sync constructs the cycle with the trigger released
    {
        let c = a.world().resource::<Combat>();
        assert_eq!(c.weapon, Some(GUN));
        assert_eq!(c.ammo, Some(AMMO));
        assert!(!c.melee && !c.self_ammo);
        let cycle = c.cycle.as_ref().unwrap();
        assert!(!cycle.no_ammo);
        assert_eq!(cycle.magazine, 1);
        assert!(c.flying.is_empty() && c.script_shots.is_empty());
    }
    assert!(take_fired(&mut a).is_empty());
    let before = a.world().resource::<CombatStats>().clone();
    let attacks = a.world().resource::<WeaponAnim>().attacks;
    a.world_mut().resource_mut::<PlayerControl>().trigger = true;
    tick(&mut a); // the production player's Cycle consumes this input
    a.world_mut().resource_mut::<PlayerControl>().trigger = false;
    assert_eq!(query(&mut a, "SetFunctionValue (Player.GetItemCount OwnedAmmo)"), XVal::Num(2.0));
    assert_eq!(query(&mut a, "SetFunctionValue (Player.GetItemCount OwnedGun)"), XVal::Num(20.0));
    let stats = a.world().resource::<CombatStats>();
    assert_eq!(stats.shots, before.shots + 1);
    assert_eq!(stats.projectiles, before.projectiles + 1);
    assert_eq!(stats.actor_shots, before.actor_shots, "the actor/script-shot consumer did not make this shot");
    assert_eq!(a.world().resource::<WeaponAnim>().attacks, attacks + 1);
    let c = a.world().resource::<Combat>();
    assert_eq!(c.cycle.as_ref().unwrap().magazine, 0);
    assert_eq!(c.flying.len(), 1);
    assert!(c.script_shots.is_empty());
    last(&a, PROJ, 500.0, 7.0);
    let events = take_fired(&mut a);
    assert_eq!(events.len(), 1);
    assert_eq!((events[0].projectile, events[0].weapon, events[0].shooter), (PROJ, GUN, FormId::PLAYER_REF));
    assert_eq!((events[0].at, events[0].dir), (None, None), "normal held-weapon effect routing, distinct from ActorFire");
    typed_unchanged(&a);
}

// Candidate-only API control: the baseline comparison compiles the preceding three
// independent consumer controls without this new-API case; no fake baseline shim.
#[test]
fn fixed_random_multi_projectile_launch_preserves_order_and_full_spawn_state() {
    let a = app();
    let c = a.world().resource::<Combat>();
    let mut chain = c.forms.chain(GUN, Some(AMMO)).unwrap();
    chain.projectiles_per_shot = 3; // local authored input, not a typed cache edit
    chain.explosion = Some(FormId(0xa51));
    let draws = [0.0, 0.875, 0.25, 0.0, 0.25, 0.25];
    let origin = [3.0, 4.0, 5.0];
    let aim = [0.0, 1.0, 0.0];
    let mut ordinary_count = 0;
    let ordinary = shot::fire(&c.forms, &chain, origin, aim, 60.0, Some(PLAYER), 980.0, &c.tuning, &mut || {
        let draw = draws[ordinary_count]; ordinary_count += 1; draw
    });
    let mut resolved_count = 0;
    let resolved = shot::fire_projectile(&c.forms.projectiles[&PROJ], &chain, origin, aim, 60.0, Some(PLAYER), 980.0, &c.tuning, &mut || {
        let draw = draws[resolved_count]; resolved_count += 1; draw
    });
    assert_eq!((ordinary_count, resolved_count), (6, 6), "exactly two ordered draws for each projectile");
    assert_eq!((ordinary.len(), resolved.len()), (3, 3));
    assert_eq!(serde_json::to_value(&ordinary).unwrap(), serde_json::to_value(&resolved).unwrap(), "every serialized physical spawn field agrees");
    // Independent geometric sentinels: forward, then thirty degrees toward +X,
    // then thirty degrees toward +Z. They catch reordered or reused random pairs
    // even though the two public APIs share the extracted implementation.
    let expected: [[f32; 3]; 3] = [[0.0, 1000.0, 0.0], [500.0, 866.0254, 0.0], [0.0, 866.0254, 500.0]];
    for (shot, velocity) in resolved.iter().zip(expected) {
        assert_eq!(shot.pos, origin);
        assert_eq!(shot.shooter, Some(PLAYER));
        assert_eq!(shot.explosion, chain.explosion);
        for (actual, expected) in shot.vel.into_iter().zip(velocity) {
            assert!((actual - expected).abs() < 0.001, "ordered analytic velocity: {actual} versus {expected}");
        }
    }
    typed_unchanged(&a);
}

#[path = "projectile_lifecycle_tests.rs"]
mod lifecycle_tests;

#[path = "projectile_zone_tests.rs"]
mod projectile_zone_tests;
