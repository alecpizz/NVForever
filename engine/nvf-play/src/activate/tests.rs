//! Targeting, activation and visual sync in a small Bevy app over a hand-built
//! simulation (forms and references in memory) and physics world (no game data).

use super::*;
use bevy::camera::primitives::Aabb;
use crate::phys::PhysState;
use crate::PlayData;
use bevy::ecs::message::Messages;
use fnv_collide::{CollisionObject, Iso, Motion, Shape};
use fnv_phys::{CharacterParams, PhysParams, PhysWorld, RefPose};
use fnv_sim::forms::{ActorBase, BaseForm, Container, Door, Item, ItemKind};
use fnv_sim::refs::{Lock, PlacedRef, Teleport};
use fnv_sim::{FormStore, RefTable, SimConfig, World};
use fnv_world::{Cell, WorldIndex};

mod creature_onactivate_tests;
mod crosshair_bars_tests;
mod crosshair_bed_item_tests;
mod crosshair_prompt_tests;
mod crosshair_sleeper_tests;
mod crosshair_water_tests;
mod perk_choice_tests;
mod reticle_limb_tests;

const STIMPAK: u32 = 0x100;
const KEY: u32 = 0x101;
const CHEST: u32 = 0x120;
const DOOR: u32 = 0x130;
const HIDDEN_DOOR: u32 = 0x131; // no FULL, as OffDoorHidden01 (#1598)
const TRIGGER: u32 = 0x140;
const CHAIR: u32 = 0x150;
const LEVER: u32 = 0x160; // a named activator with a model
const BED: u32 = 0x152;
const SETTLER: u32 = 0x170; // an NPC
const NPC: u32 = 0x171; // a living NPC behind bars (#3233)
const ROBOT: u32 = 0x172; // a creature with nothing to say (#3479)
const CELL_EXT: u32 = 0x200;
const CELL_INT: u32 = 0x201;
const WORLD: u32 = 0x3C;
const STIM_REF: u32 = 0x301; // XCNT 3, no collision
const CHEST_REF: u32 = 0x302;
const SAFE_REF: u32 = 0x303; // a chest locked at level 50 with KEY
const DOOR_REF: u32 = 0x304; // -> DOOR_DEST in CELL_INT
const DOOR_DEST: u32 = 0x305;
const TRIGGER_REF: u32 = 0x306; // unnamed activator
const CHAIR_REF: u32 = 0x307;
const LEVER_REF: u32 = 0x308;
const LEVER_TRIGGER_REF: u32 = 0x309; // LEVER placed as a box primitive (a trigger)
const BED_REF: u32 = 0x30A;
const SETTLER_REF: u32 = 0x30B; // ACHR of SETTLER
const NPC_REF: u32 = 0x30C; // a living NPC (ACHR of NPC)
const ROBOT_REF: u32 = 0x30D; // ACRE of ROBOT

fn base(id: u32, name: Option<&str>, data: BaseData) -> BaseForm {
    BaseForm { id: FormId(id), sig: *b"MISC", edid: Some(format!("Edid{id:X}")), name: name.map(Into::into), script: None, prompt: None, voice_filter: false, data }
}

fn item(kind: ItemKind) -> BaseData {
    BaseData::Item(Item { kind, value: Some(1), money: None, weight: Some(0.5), slots: None, projectile: None, health: None, non_playable: false, cant_drop: false, armor_class: Default::default(), power_armor: false, armor_dr: None, armor_dt: None, weapon_damage: None, weight_mods: [0.0; 3], drawable: false, not_for_npcs: false, thrown: false, quest_item: false, icon: None, placed_count: None, teaches: None, cant_be_taken: false, modulates_voice: false, weapon_traits: None })
}

pub(crate) fn placed(id: u32, base: u32, cell: u32) -> PlacedRef {
    PlacedRef {
        id: FormId(id),
        sig: *b"REFR",
        edid: None,
        base: FormId(base),
        cell: Some(FormId(cell)),
        stored_in: Some(FormId(cell)),
        plugin: 0,
        flags: 0,
        in_persistent_group: false,
        pos: [0.0; 3],
        rot: [0.0; 3],
        scale: None,
        count: None,
        ammo: None,
        health: None,
        owner: None,
        lock: None,
        teleport: None,
        enable_parent: None,
        linked: None,
        patrol_idle: None,
        patrol_topic: None,
        merchant_container: None,
        marker_reputation: None,
        open_by_default: false,
        primitive: None,
        activate_parents: Vec::new(),
        parent_activate_only: false,
        prompt: None,
        radiation: None,
        radius: None,
        encounter_zone: None,
        level_modifier: None,
        map_marker: None,
        radio: None,
    }
}

fn sim() -> SimState {
    let mut forms = FormStore::default();
    let chest = || BaseData::Container(Container { respawns: false, entries: Vec::new() });
    for mut b in [
        base(STIMPAK, Some("Stimpak"), item(ItemKind::Aid)),
        base(KEY, Some("Safe Key"), item(ItemKind::Key)),
        base(CHEST, Some("Chest"), chest()),
        base(DOOR, Some("Door"), BaseData::Door(Door { automatic: false, minimal_use: false })),
        base(HIDDEN_DOOR, None, BaseData::Door(Door { automatic: false, minimal_use: false })),
        base(TRIGGER, None, BaseData::Activator),
        base(CHAIR, Some("Chair"), BaseData::Furniture),
        base(LEVER, Some("Lever"), BaseData::Activator),
        base(BED, Some("Bed"), BaseData::Furniture),
        BaseForm { sig: *b"NPC_", ..base(SETTLER, Some("Settler"), BaseData::Actor(ActorBase { entries: Vec::new(), attributes: None })) },
        BaseForm { sig: *b"NPC_", ..base(NPC, Some("Rick Lancer"), BaseData::Actor(ActorBase { entries: Vec::new(), attributes: None })) },
        BaseForm { sig: *b"CREA", ..base(ROBOT, Some("Robot"), BaseData::Actor(ActorBase { entries: Vec::new(), attributes: None })) },
        base(FormId::PLAYER_BASE.0, Some("Courier"), BaseData::Actor(ActorBase { entries: Vec::new(), attributes: None })),
    ] {
        // [design, #2538] World::lock_source identifies doors through FormStore::sig.
        if matches!(b.data, BaseData::Door(_)) {
            b.sig = *b"DOOR";
            forms.sigs.insert(b.id, b.sig);
        }
        forms.bases.insert(b.id, b);
    }
    let mut refs = RefTable::default();
    let stim = PlacedRef { count: Some(3), ..placed(STIM_REF, STIMPAK, CELL_EXT) };
    let safe = PlacedRef { lock: Some(Lock { level: 50, key: Some(FormId(KEY)), leveled: false }), ..placed(SAFE_REF, CHEST, CELL_EXT) };
    let tp = Teleport { door: FormId(DOOR_DEST), pos: [10.0, 20.0, 30.0], rot: [0.0, 0.0, std::f32::consts::FRAC_PI_2], no_alarm: false };
    let door = PlacedRef { teleport: Some(tp), ..placed(DOOR_REF, DOOR, CELL_EXT) };
    // An inaccessible door (header bit 8) and a chest with an Activation Prompt (XATO).
    let sealed = PlacedRef { flags: fnv_sim::refs::INACCESSIBLE, ..placed(0x7777, DOOR, CELL_EXT) };
    let prompted = PlacedRef { prompt: Some("Search Footlocker".into()), ..placed(0x7778, CHEST, CELL_EXT) };
    // Nameless doors (#1598): one leading nowhere, one that teleports, one with a prompt.
    let hidden = placed(0x7779, HIDDEN_DOOR, CELL_EXT);
    let hidden_travel = PlacedRef { teleport: Some(tp), ..placed(0x777A, HIDDEN_DOOR, CELL_EXT) };
    let hidden_prompted = PlacedRef { prompt: Some("Enter Hatch".into()), ..placed(0x777B, HIDDEN_DOOR, CELL_EXT) };
    // A nameless activator with a prompt (#2160: Hoover Dam's control panel).
    let panel = PlacedRef { prompt: Some("Activate".into()), ..placed(0x777C, TRIGGER, CELL_EXT) };
    refs.refs.insert(panel.id, panel);
    let settler = PlacedRef { sig: *b"ACHR", ..placed(SETTLER_REF, SETTLER, CELL_EXT) };
    refs.refs.insert(settler.id, settler);
    let box_trigger = PlacedRef { primitive: Some(fnv_sim::refs::Primitive { sphere: false, bounds: [30.0, 25.0, 31.0] }), ..placed(LEVER_TRIGGER_REF, LEVER, CELL_EXT) };
    refs.refs.insert(FormId(ROBOT_REF), PlacedRef { sig: *b"ACRE", ..placed(ROBOT_REF, ROBOT, CELL_EXT) });
    refs.refs.insert(FormId(NPC_REF), PlacedRef { sig: *b"ACHR", ..placed(NPC_REF, NPC, CELL_EXT) });
    for r in [stim, safe, door, sealed, prompted, hidden, hidden_travel, hidden_prompted, box_trigger, placed(LEVER_REF, LEVER, CELL_EXT), placed(CHEST_REF, CHEST, CELL_EXT), placed(DOOR_DEST, DOOR, CELL_INT), placed(TRIGGER_REF, TRIGGER, CELL_EXT), placed(CHAIR_REF, CHAIR, CELL_EXT), placed(BED_REF, BED, CELL_EXT)] {
        refs.refs.insert(r.id, r);
    }
    let forms: &'static FormStore = Box::leak(Box::new(forms));
    let refs: &'static RefTable = Box::leak(Box::new(refs));
    SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) }
}

fn index() -> &'static WorldIndex {
    let mut ix = WorldIndex::default();
    ix.cells.insert(CELL_EXT, Cell { form_id: CELL_EXT, world: Some(WORLD), grid: Some((0, 0)), ..Default::default() });
    let int = Cell { form_id: CELL_INT, name: Some("Doc Mitchell's House".into()), ..Default::default() };
    ix.cells.insert(CELL_INT, int);
    Box::leak(Box::new(ix))
}

fn play_data(index: &'static WorldIndex) -> PlayData {
    let lo: &'static fnv_esm::LoadOrder<'static> = Box::leak(Box::new(fnv_esm::LoadOrder::new(&[]).expect("empty load order")));
    let schema: &'static fnv_esm::Schema = Box::leak(Box::default());
    PlayData { lo, schema, index, world: WORLD, interior: None, eye: [0.0; 3], heading_deg: 0.0, pitch_deg: 0.0, auto_walk: 0.0, start_at_cell_spot: false, packages: false }
}

fn boxed(half: f32) -> CollisionObject {
    CollisionObject {
        node: None,
        node_block: 0,
        node_transform: Iso::IDENTITY,
        body_transform: Iso::IDENTITY,
        motion: Motion::Static,
        layer: layers::CLUTTER,
        mass: 10.0,
        friction: 0.5,
        restitution: 0.0,
        shape: Shape::Box { half_extents: [half; 3] }.into(),
        shell: 0.0,
        material: None,
        wind: false,
    }
}

/// The player at the origin looking east (+x) at eye height 100; references with
/// collision boxes at the given x (half size 10, at eye height).
pub(crate) fn scene(colliding: &[(u32, f32)]) -> App {
    let mut world = PhysWorld::new(PhysParams::default());
    for &(id, x) in colliding {
        world.add_ref(&RefPose { form_id: id, position: [x, 0.0, 100.0], rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], scale: 1.0 }, &[boxed(10.0)]);
    }
    let me = world.add_character(1, CharacterParams::default(), [0.0, 0.0, 0.0]);
    let mut phys = PhysState::default();
    (phys.world, phys.player) = (world, Some(me));
    let mut app = App::new();
    app.insert_resource(sim())
        .insert_resource(PlayWorld(play_data(index())))
        .insert_resource(phys)
        .insert_resource(Player { heading: std::f32::consts::FRAC_PI_2, pitch: 0.0, eye_height: 100.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None })
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<crate::bindings::Bindings>()
        .init_resource::<Target>()
        .init_resource::<HudMessage>()
        .init_resource::<crate::furniture::PlayerSeat>()
        .add_message::<TravelRequest>()
        .add_message::<OpenContainer>()
        .add_message::<RefChanged>()
        .add_message::<crate::furniture::UseFurniture>()
        .add_message::<crate::world_sounds::Activated>()
        .add_message::<crate::terminal::OpenTerminal>()
        .add_message::<crate::lockpick::OpenLockpick>()
        .add_message::<crate::pickpocket::OpenPickpocket>()
        .add_message::<crate::pickpocket::Pickpocketed>()
        .add_message::<crate::ui_sounds::UiSound>()
        .add_systems(Update, (find_target, activate, sync_visuals).chain());
    app
}

/// A render entity of `form_id`: a box of half size 2 centred at game point `at`.
fn visual(app: &mut App, form_id: u32, at: [f32; 3]) -> Entity {
    let aabb = Aabb { center: Vec3::ZERO.into(), half_extents: Vec3::splat(2.0).into() };
    let gt = GlobalTransform::from_translation(space::to_bevy(at));
    app.world_mut().spawn((RefVisual(form_id), aabb, gt, InheritedVisibility::VISIBLE, Visibility::Inherited)).id()
}

fn press_e(app: &mut App) {
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyE);
    app.update();
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release(KeyCode::KeyE);
    keys.clear();
}

fn target(app: &App) -> Option<(u32, Source)> {
    app.world().resource::<Target>().0.as_ref().map(|t| (t.form_id, t.source))
}

fn drain<M: Message>(app: &mut App) -> Vec<M> {
    app.world_mut().resource_mut::<Messages<M>>().drain().collect()
}

#[test]
fn a_restored_change_still_reaches_the_drawn_world() {
    let mut app = scene(&[]);
    let stim = visual(&mut app, STIM_REF, [60.0, 0.0, 100.0]);
    app.update();
    assert_eq!(app.world().get::<Visibility>(stim), Some(&Visibility::Inherited));
    app.world_mut().resource_mut::<SimState>().world.activate(FormId::PLAYER_REF, FormId(STIM_REF)).unwrap();
    app.world_mut().resource_mut::<Messages<RefChanged>>().write(RefChanged::restored(STIM_REF));
    app.update();
    assert_eq!(app.world().get::<Visibility>(stim), Some(&Visibility::Hidden));
}

#[test]
fn describe_by_type_lock_and_destination() {
    let sim = sim();
    let ix = index();
    assert_eq!(describe(&sim, ix, STIM_REF), Some(("Stimpak".into(), Action::Take { count: 3 })));
    assert_eq!(describe(&sim, ix, CHEST_REF), Some(("Chest".into(), Action::Open)));
    assert_eq!(describe(&sim, ix, SAFE_REF), Some(("Chest".into(), Action::Locked { level: 50 })));
    assert_eq!(describe(&sim, ix, DOOR_REF), Some(("Door".into(), Action::Travel { to: Some("Doc Mitchell's House".into()) })));
    assert_eq!(describe(&sim, ix, TRIGGER_REF), None, "unnamed activators are not offered");
    // With an Activation Prompt it is offered, by the prompt alone (#2160).
    assert_eq!(describe(&sim, ix, 0x777C), Some((String::new(), Action::Activate)));
    let t = Targeted { form_id: 0x777C, name: String::new(), action: Action::Activate, distance: 50.0, source: pick::Source::Bounds, crime: false, empty: false, through: None };
    let p = crate::reticle::Prompt::of(&t, &sim);
    assert_eq!((p.action.as_str(), p.name.as_str()), ("Activate", ""));
    assert_eq!(describe(&sim, ix, 0x7779), None, "nor an unnamed door leading nowhere (#1598)");
    assert!(
        matches!(describe(&sim, ix, 0x777A), Some((_, Action::Travel { .. }))),
        "an unnamed teleport door is still offered: a quest may send the player through it"
    );
    assert_eq!(describe(&sim, ix, 0x777B).map(|(_, a)| a), Some(Action::Open), "an unnamed door with a prompt is still offered");
    // A primitive is a trigger volume even when its activator has a name (level-design.md
    // §4.1): the same activator placed as an object is offered.
    assert_eq!(describe(&sim, ix, LEVER_REF), Some(("Lever".into(), Action::Activate)));
    assert_eq!(describe(&sim, ix, LEVER_TRIGGER_REF), None, "primitives are not offered");
    assert_eq!(describe(&sim, ix, 0x999), None);
}

#[test]
fn inaccessible_doors_and_activation_prompts() {
    // docs/spec-notes/cells-refs.md §2, §3.
    let sim = sim();
    let ix = index();
    assert_eq!(describe(&sim, ix, 0x7777), Some(("Door".into(), Action::Inaccessible)));
    assert_eq!(Action::Inaccessible.label(), "Inaccessible");
    assert_eq!(crate::reticle::activation_prompt(&sim, 0x7778).as_deref(), Some("Search Footlocker"));
    assert_eq!(crate::reticle::activation_prompt(&sim, CHEST_REF), None);
}

#[test]
fn the_key_unlocks() {
    let mut sim = sim();
    sim.world.add_item(FormId::PLAYER_REF, FormId(KEY), 1).unwrap();
    assert_eq!(describe(&sim, index(), SAFE_REF).unwrap().1, Action::Unlock);
}

#[test]
fn an_empty_chest_is_marked_empty_until_it_holds_something() {
    let mut app = scene(&[(CHEST_REF, 100.0)]);
    app.update();
    let empty = |app: &App| app.world().resource::<Target>().0.as_ref().map(|t| t.empty);
    assert_eq!(empty(&app), Some(true));
    app.world_mut().resource_mut::<SimState>().world.add_item(FormId(CHEST_REF), FormId(STIMPAK), 1).unwrap();
    app.update();
    assert_eq!(empty(&app), Some(false));
    // A locked chest is not looked into.
    let mut sim = sim();
    assert!(!holds_nothing(None, &mut sim, SAFE_REF, &Action::Locked { level: 50 }));
    assert!(sim.world.peek(FormId(SAFE_REF)).is_none(), "not materialised");
}

#[test]
fn a_body_whose_weapon_lies_beside_it_is_not_empty() {
    // The chest stands in for a body that dropped its only item as it died (#1032): its
    // loot menu lists the dropped reference, so the prompt is not "[Empty]".
    let mut sim = sim();
    sim.world.add_item(FormId(CHEST_REF), FormId(STIMPAK), 1).unwrap();
    let r = sim.world.drop_item(FormId(CHEST_REF), FormId(STIMPAK), 1).unwrap()[0];
    assert!(holds_nothing(None, &mut sim, CHEST_REF, &Action::Open), "nothing linked yet");
    sim.world.get(r).unwrap().dropped_by = Some(FormId(CHEST_REF));
    assert!(!holds_nothing(None, &mut sim, CHEST_REF, &Action::Open));
    // Picked up from the ground: empty again.
    sim.world.activate(FormId::PLAYER_REF, r).unwrap();
    assert!(holds_nothing(None, &mut sim, CHEST_REF, &Action::Open));
}

#[test]
fn the_key_plays_the_unlock_sound() {
    let mut app = scene(&[(SAFE_REF, 50.0)]);
    app.world_mut().resource_mut::<SimState>().world.add_item(FormId::PLAYER_REF, FormId(KEY), 1).unwrap();
    app.update();
    press_e(&mut app);
    let sounds = drain::<crate::ui_sounds::UiSound>(&mut app);
    assert_eq!(sounds.iter().map(|s| (s.menu, s.event)).collect::<Vec<_>>(), vec![(crate::ui_sounds::UiMenu::Lockpick, crate::ui_sounds::UiEvent::Unlocked)]);
    assert_eq!(drain::<OpenContainer>(&mut app).iter().map(|m| m.container).collect::<Vec<_>>(), vec![SAFE_REF]);
    // Opened again, it is no longer locked: no sound.
    press_e(&mut app);
    assert!(drain::<crate::ui_sounds::UiSound>(&mut app).is_empty());
}

#[test]
fn take_an_item_by_its_bounds_then_open_the_chest_behind_it() {
    let mut app = scene(&[(CHEST_REF, 100.0)]);
    let stim = visual(&mut app, STIM_REF, [60.0, 0.0, 100.0]);
    app.update();
    // The stimpak has no collision; its render box is nearer than the chest's collision.
    assert_eq!(target(&app), Some((STIM_REF, Source::Bounds)));
    let d = app.world().resource::<Target>().0.as_ref().unwrap().distance;
    assert!((d - 58.0).abs() < 1e-3, "{d}");
    press_e(&mut app);
    assert_eq!(drain::<RefChanged>(&mut app).iter().map(|m| m.form_id).collect::<Vec<_>>(), vec![STIM_REF]);
    assert_eq!(app.world().get::<Visibility>(stim), Some(&Visibility::Hidden));
    let sim = app.world().resource::<SimState>();
    assert!(!sim.present(STIM_REF));
    assert_eq!(sim.world.peek(FormId::PLAYER_REF).unwrap().inventory.as_ref().unwrap().count(FormId(STIMPAK)), 3);
    // Taken: the chest (by its collision, the player's capsule ignored) is next.
    app.update();
    assert_eq!(target(&app), Some((CHEST_REF, Source::Collision)));
    press_e(&mut app);
    assert_eq!(drain::<OpenContainer>(&mut app).iter().map(|m| m.container).collect::<Vec<_>>(), vec![CHEST_REF]);
    // The stimpak's cell streams in again: its new render entity starts hidden.
    let again = visual(&mut app, STIM_REF, [60.0, 0.0, 100.0]);
    let chest = visual(&mut app, CHEST_REF, [100.0, 0.0, 100.0]);
    app.update();
    assert_eq!(app.world().get::<Visibility>(again), Some(&Visibility::Hidden));
    assert_eq!(app.world().get::<Visibility>(chest), Some(&Visibility::Inherited));
}

#[test]
fn walls_and_reach_limit_the_bounds() {
    // A wall (a non-activatable reference) in front of the item hides it.
    let mut app = scene(&[(0x999, 40.0)]);
    visual(&mut app, STIM_REF, [60.0, 0.0, 100.0]);
    app.update();
    assert_eq!(target(&app), None);
    // Out of reach.
    let mut app = scene(&[]);
    visual(&mut app, STIM_REF, [pick::REACH + 10.0, 0.0, 100.0]);
    app.update();
    assert_eq!(target(&app), None);
}

#[test]
fn a_door_sends_travel_and_a_locked_chest_to_the_lockpicking_menu() {
    let mut app = scene(&[(DOOR_REF, 50.0)]);
    app.update();
    assert_eq!(target(&app), Some((DOOR_REF, Source::Collision)));
    press_e(&mut app);
    let t: Vec<Travel> = drain::<TravelRequest>(&mut app).into_iter().map(|r| r.0).collect();
    assert_eq!(t.len(), 1);
    assert_eq!((t[0].interior, t[0].feet, t[0].heading_deg), (Some(CELL_INT), [10.0, 20.0, 30.0], 90.0));
    let p = app.world().resource::<SimState>().world.peek(FormId::PLAYER_REF).unwrap().clone();
    assert_eq!((p.cell, p.pos), (Some(FormId(CELL_INT)), [10.0, 20.0, 30.0]));

    let mut app = scene(&[(SAFE_REF, 50.0)]);
    press_e(&mut app);
    assert!(drain::<OpenContainer>(&mut app).is_empty());
    // Without its key: the lockpicking menu is asked for (it opens or says why not).
    let asked: Vec<u32> = drain::<crate::lockpick::OpenLockpick>(&mut app).into_iter().map(|o| o.reference).collect();
    assert_eq!(asked, vec![SAFE_REF]);
}

#[test]
fn no_target_no_activation() {
    let mut app = scene(&[]);
    press_e(&mut app);
    assert_eq!(target(&app), None);
    assert!(drain::<RefChanged>(&mut app).is_empty());
    assert!(app.world().resource::<SimState>().world.peek(FormId::PLAYER_REF).is_none(), "nothing materialised");
}

#[test]
fn a_dropped_item_is_targeted_by_its_collision_and_taken_back() {
    let mut app = scene(&[]);
    // The player (facing east) carries 3 stimpaks and drops 2: one created reference.
    let dropped = {
        let mut sim = app.world_mut().resource_mut::<SimState>();
        let p = FormId::PLAYER_REF;
        sim.world.add_item(p, FormId(STIMPAK), 3).unwrap();
        sim.world.get(p).unwrap().rot[2] = std::f32::consts::FRAC_PI_2;
        let refs = sim.world.drop_item(p, FormId(STIMPAK), 2).unwrap();
        assert_eq!(refs.len(), 1);
        // Where it came to rest: in front of the eye.
        sim.world.get(refs[0]).unwrap().pos = [60.0, 0.0, 100.0];
        refs[0].0
    };
    {
        let mut phys = app.world_mut().resource_mut::<PhysState>();
        let pose = RefPose { form_id: dropped, position: [60.0, 0.0, 100.0], rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], scale: 1.0 };
        phys.world.add_ref(&pose, &[boxed(5.0)]);
    }
    app.update();
    assert_eq!(target(&app), Some((dropped, Source::Collision)));
    assert_eq!(app.world().resource::<Target>().0.as_ref().unwrap().action, Action::Take { count: 2 });
    press_e(&mut app);
    assert_eq!(drain::<RefChanged>(&mut app).iter().map(|m| m.form_id).collect::<Vec<_>>(), vec![dropped]);
    let d = play_data(index());
    let mut phys = app.world_mut().remove_resource::<PhysState>().unwrap();
    let sim = app.world().resource::<SimState>();
    // The player's own activation takes the whole stack back [hypothesis, World::activate].
    assert_eq!(sim.world.peek(FormId::PLAYER_REF).unwrap().inventory.as_ref().unwrap().count(FormId(STIMPAK)), 3);
    assert!(!sim.present(dropped));
    // RefChanged removes its collision (Part P's handler).
    phys.sync_ref(&crate::phys::LoadCtx { d: &d, sim, data: None }, dropped);
    assert!(!phys.has_collision(dropped));
}

#[test]
fn activating_furniture_asks_to_use_it_and_says_nothing() {
    let mut app = scene(&[(CHAIR_REF, 80.0), (CHEST_REF, 150.0)]);
    app.update();
    assert_eq!(target(&app), Some((CHAIR_REF, Source::Collision)));
    assert_eq!(describe(app.world().resource::<SimState>(), index(), CHAIR_REF), Some(("Chair".into(), Action::Activate)));
    press_e(&mut app);
    let asked = drain::<crate::furniture::UseFurniture>(&mut app);
    assert_eq!(asked.iter().map(|m| m.reference).collect::<Vec<_>>(), vec![CHAIR_REF]);
    // No "nothing happens" line, and nothing else was activated.
    assert_eq!(app.world().resource::<HudMessage>().text, "");
    assert!(drain::<OpenContainer>(&mut app).is_empty());
}

/// The playtest's "cannot open containers" (2026-10-11): E opens a container, and E also
/// closes the container menu; the press that opens it must not close it in the same frame.
#[test]
fn e_on_a_chest_opens_the_container_menu_and_it_stays_open() {
    let mut app = scene(&[(CHEST_REF, 100.0)]);
    // `scene` runs targeting and activation; here the menus follow them, as in play.
    app.world_mut().resource_mut::<Schedules>().remove(Update);
    app.add_plugins(bevy::state::app::StatesPlugin).init_state::<PlayState>();
    crate::menu::add_open_and_keys(&mut app, (find_target, activate).chain());
    app.update();
    assert_eq!(target(&app), Some((CHEST_REF, Source::Collision)));
    press_e(&mut app);
    let open = |app: &App| app.world().resource::<crate::menu::ActiveMenu>().0.as_ref().map(|m| m.mode);
    assert_eq!(open(&app), Some(crate::menu::model::Mode::Container(FormId(CHEST_REF))));
    app.update();
    assert_eq!(*app.world().resource::<State<PlayState>>().get(), PlayState::Menu);
    assert!(open(&app).is_some(), "the menu stays open after the press that opened it");
    // The next press of E closes it.
    press_e(&mut app);
    app.update();
    assert_eq!(*app.world().resource::<State<PlayState>>().get(), PlayState::Playing);
}

#[test]
fn a_water_prompt_carries_its_h2o() {
    assert_eq!(water_label("Faucet", Some(-15.0)), "Faucet (H2O -15)");
    assert_eq!(water_label("Faucet", Some(0.2)), "Faucet");
    assert_eq!(water_label("Faucet", None), "Faucet");
}


#[test]
fn switching_cancels_a_holdable_activate_tap_before_it_can_take_the_item() {
    use crate::{bindings::{Bindings, InputDevice}, grab::{Candidate, GrabState}};
    use bevy::input::{gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent}, keyboard::{Key, KeyboardInput}, ButtonState};
    let mut app = scene(&[]);
    app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, crate::pad::InputSwitchPlugin));
    app.init_resource::<GrabState>();
    visual(&mut app, STIM_REF, [60.0, 0.0, 100.0]);
    app.world_mut().resource_mut::<GrabState>().candidate = Some(Candidate {
        on: fnv_phys::GripOn::Ref(STIM_REF), at: [60.0, 0.0, 100.0], distance: 60.0,
    });
    let pad = app.world_mut().spawn(Gamepad::default()).id();
    let e = KeyboardInput { key_code: KeyCode::KeyE, logical_key: Key::Character("e".into()), state: ButtonState::Pressed, text: None, repeat: false, window: Entity::PLACEHOLDER };
    app.world_mut().write_message(e.clone());
    app.update();
    assert!(app.world().resource::<GrabState>().tap.is_some());
    app.world_mut().write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(pad, GamepadButton::North, 1.0)));
    app.update();
    assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
    crate::bindings::set_device(app.world_mut(), InputDevice::Controller);
    assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
    assert!(app.world().resource::<GrabState>().tap.is_none());
    assert!(drain::<RefChanged>(&mut app).is_empty());
    app.world_mut().write_message(KeyboardInput { state: ButtonState::Released, ..e });
    app.update();
    assert!(app.world().resource::<ButtonInput<KeyCode>>().just_released(KeyCode::KeyE));
    assert!(drain::<RefChanged>(&mut app).is_empty());
    assert_eq!(app.world().resource::<SimState>().world.peek(FormId::PLAYER_REF).and_then(|r| r.inventory.as_ref()).map_or(0, |i| i.count(FormId(STIMPAK))), 0);
}


/// [design, #2538] Both HUD states use the same partner lock as activation.
#[test]
fn load_door_lock_prompt_uses_partner_level_and_key() {
    let mut sim = sim();
    for id in [FormId(DOOR), FormId(HIDDEN_DOOR)] {
        assert_eq!(sim.forms.get(id).unwrap().sig, *b"DOOR");
        assert_eq!(sim.forms.sig(id), Some(*b"DOOR"));
    }
    sim.world.lock(FormId(DOOR_DEST), Some(255)).unwrap();
    assert_eq!(describe(&sim, index(), DOOR_REF).unwrap().1, Action::Locked { level: 255 });
    assert_eq!(describe(&sim, index(), DOOR_REF).unwrap().1.label(), "Locked (Requires Key)");
    sim.world.get(FormId(DOOR_DEST)).unwrap().lock.as_mut().unwrap().key = Some(FormId(KEY));
    sim.world.add_item(FormId::PLAYER_REF, FormId(KEY), 1).unwrap();
    assert_eq!(describe(&sim, index(), DOOR_REF).unwrap().1, Action::Unlock);
    sim.world.unlock(FormId(DOOR_REF)).unwrap();
    assert!(matches!(describe(&sim, index(), DOOR_REF).unwrap().1, Action::Travel { .. }));
}

#[test]
fn restored_unloaded_changes_are_summarised_while_live_and_drawn_changes_log_individually() {
    assert!(!log_ref_change_individually(true, 0), "restored unloaded refs go to the single summary line");
    assert!(log_ref_change_individually(true, 1), "restored drawn refs keep per-ref info");
    assert!(log_ref_change_individually(false, 0), "live refs keep per-ref info even when 0 entities updated");
    assert!(log_ref_change_individually(false, 3), "live drawn refs keep per-ref info");

    // End-to-end: a batch mixing restored undrawn refs, restored drawn refs, and live
    // changes still updates every drawn entity cleanly.
    let mut app = scene(&[]);
    let stim = visual(&mut app, STIM_REF, [60.0, 0.0, 100.0]);
    app.update();
    assert_eq!(app.world().get::<Visibility>(stim), Some(&Visibility::Inherited));
    app.world_mut().resource_mut::<SimState>().world.activate(FormId::PLAYER_REF, FormId(STIM_REF)).unwrap();
    let mut msgs = app.world_mut().resource_mut::<Messages<RefChanged>>();
    msgs.write(RefChanged::restored(STIM_REF));
    msgs.write(RefChanged::restored(0x0099_0001));
    msgs.write(RefChanged::restored(0x0099_0002));
    msgs.write(RefChanged::new(0x0099_0003));
    app.update();
    assert_eq!(app.world().get::<Visibility>(stim), Some(&Visibility::Hidden));
}
