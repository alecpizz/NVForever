//! Caption/glyph changes on the existing action node; never input, layout or saved keys.
use super::{
    icons::{self, TouchIcon},
    TouchVisual, BUTTONS,
};
use bevy::prelude::*;
use nvf_play::{
    bindings::Control,
    touch_action_presentation::{AdaptiveTouchActions, AimPresentation, PublishPresentation},
};

pub(super) fn install(app: &mut App) {
    app.add_plugins(nvf_play::touch_action_presentation::TouchActionPresentationPlugin)
        .add_systems(
            Update,
            show.after(PublishPresentation).after(super::show_labels),
        );
}
fn show(
    presentation: Res<AimPresentation>,
    settings: Res<AdaptiveTouchActions>,
    parents: Query<&TouchVisual>,
    mut captions: Query<(&ChildOf, &TouchVisual, &mut Text)>,
    mut glyphs: Query<(&ChildOf, &mut TouchIcon)>,
) {
    let Some(action) = BUTTONS
        .iter()
        .position(|b| b.control == Some(Control::Block))
    else {
        return;
    };
    let mine = |parent: &ChildOf| matches!(parents.get(parent.parent()),Ok(TouchVisual::Button(i)) if *i==action);
    let label = presentation.0.label(settings.0);
    for (parent, visual, mut text) in &mut captions {
        if mine(parent) && matches!(visual, TouchVisual::Caption) && text.0 != label {
            text.0 = label.into();
        }
    }
    if let Some(index) = icons::semantic_index(presentation.0.glyph(settings.0)) {
        for (parent, mut icon) in &mut glyphs {
            if mine(parent) && icon.0 != index {
                icon.0 = index;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::{
        mouse::MouseButtonInput,
        touch::{TouchInput, TouchPhase},
        ButtonState, InputPlugin,
    };
    use bevy::window::{PrimaryWindow, WindowFocused};
    use nvf_play::{
        bindings::{Bindings, ControlInput, InputDevice},
        touch_action_presentation::AimRole,
    };
    #[derive(Resource)]
    struct Wanted(AimRole);
    #[derive(Resource, Default)]
    struct Seen {
        down: bool,
        pressed: bool,
        released: bool,
    }
    fn model(wanted: Option<Res<Wanted>>, mut model: ResMut<AimPresentation>) {
        if let Some(wanted) = wanted {
            model.0 = wanted.0;
        }
    }
    fn observe(input: ControlInput, mut seen: ResMut<Seen>) {
        seen.down = input.pressed(Control::Block);
        seen.pressed = input.just_pressed(Control::Block);
        seen.released = input.just_released(Control::Block);
    }
    fn scene() -> (App, Entity, Vec2) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin, bevy::state::app::StatesPlugin))
            .insert_state(nvf_play::PlayState::Playing)
            .add_message::<WindowFocused>()
            .add_message::<bevy::window::AppLifecycle>()
            .init_resource::<Bindings>()
            .init_resource::<Seen>()
            .insert_resource(Wanted(AimRole::Aim))
            .add_plugins(super::super::TouchControlsPlugin)
            .add_systems(Update, model.after(PublishPresentation).before(show))
            .add_systems(Update, observe.after(show));
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: bevy::window::WindowResolution::new(1000, 1000),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        // Test geometry must not inherit a developer's persisted hide/position choices.
        app.world_mut()
            .resource_mut::<super::super::Layout>()
            .config = Default::default();
        // Publish physical image boxes without requiring a renderer/GPU in this model test.
        let mut query = app
            .world_mut()
            .query_filtered::<(&mut ComputedNode, &mut InheritedVisibility), With<TouchIcon>>();
        for (mut node, mut inherited) in query.iter_mut(app.world_mut()) {
            *inherited = InheritedVisibility::VISIBLE;
            node.size = Vec2::splat(48.0);
        }
        app.update();
        let index = BUTTONS
            .iter()
            .position(|b| b.control == Some(Control::Block))
            .unwrap();
        let point = app
            .world()
            .resource::<super::super::Layout>()
            .config
            .center(super::super::Target::Button(index), Vec2::splat(1000.0));
        (app, window, point)
    }
    fn finger(app: &mut App, window: Entity, point: Vec2, phase: TouchPhase) {
        app.world_mut().write_message(TouchInput {
            window,
            position: point,
            id: 9,
            phase,
            force: None,
        });
    }
    fn physical(app: &mut App, window: Entity, down: bool) {
        app.world_mut().write_message(MouseButtonInput {
            window,
            button: MouseButton::Right,
            state: if down {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            },
        });
    }
    fn shown(app: &mut App) -> (Entity, Entity, Entity, String, usize) {
        let world = app.world_mut();
        let action = BUTTONS
            .iter()
            .position(|b| b.control == Some(Control::Block))
            .unwrap();
        let mut q = world.query::<(Entity, &TouchVisual)>();
        let button = q
            .iter(world)
            .find_map(|(e, v)| matches!(v,TouchVisual::Button(i) if *i==action).then_some(e))
            .unwrap();
        let mut q = world.query::<(Entity, &ChildOf, &Text)>();
        let (caption, label) = q
            .iter(world)
            .find_map(|(e, p, t)| (p.parent() == button).then_some((e, t.0.clone())))
            .unwrap();
        let mut q = world.query::<(Entity, &ChildOf, &TouchIcon)>();
        let (glyph, index) = q
            .iter(world)
            .find_map(|(e, p, i)| (p.parent() == button).then_some((e, i.0)))
            .unwrap();
        (button, caption, glyph, label, index)
    }
    #[test]
    fn a_held_touch_survives_caption_and_icon_changes_and_releases_its_original_action() {
        let (mut app, w, p) = scene();
        let initial = shown(&mut app);
        let layout = app
            .world()
            .resource::<super::super::Layout>()
            .config
            .encode();
        let node = app.world().get::<Node>(initial.0).unwrap();
        let geometry = (node.left, node.top, node.width, node.height);
        finger(&mut app, w, p, TouchPhase::Started);
        app.update();
        assert!(app.world().resource::<Seen>().down && app.world().resource::<Seen>().pressed);
        for (role, label, semantic) in [
            (AimRole::Block, "Block", "block"),
            (AimRole::Scope, "Scope", "aim"),
            (AimRole::Aim, "Aim", "aim"),
        ] {
            app.world_mut().resource_mut::<Wanted>().0 = role;
            app.update();
            let now = shown(&mut app);
            assert_eq!((now.0, now.1, now.2), (initial.0, initial.1, initial.2));
            assert_eq!(now.3, label);
            assert_eq!(now.4, icons::semantic_index(semantic).unwrap());
            assert!(app.world().resource::<Seen>().down);
            assert!(
                !app.world().resource::<Seen>().pressed && !app.world().resource::<Seen>().released,
                "presentation cannot synthesize edges"
            );
            let node = app.world().get::<Node>(now.0).unwrap();
            assert_eq!((node.left, node.top, node.width, node.height), geometry);
        }
        finger(&mut app, w, p, TouchPhase::Ended);
        app.update();
        assert!(!app.world().resource::<Seen>().down && app.world().resource::<Seen>().released);
        assert_eq!(
            app.world()
                .resource::<super::super::Layout>()
                .config
                .encode(),
            layout
        );
        assert!(layout["buttons"].get("Aim").is_some());
        assert!(layout["buttons"].get("Block").is_none());
    }
    #[test]
    fn static_preference_reuses_the_existing_nodes_and_same_texture_handle() {
        let (mut app, _, _) = scene();
        app.world_mut().resource_mut::<Wanted>().0 = AimRole::Block;
        app.update();
        let initial = shown(&mut app);
        assert_eq!(initial.3, "Block");
        let handle = app
            .world()
            .get::<ImageNode>(initial.2)
            .unwrap()
            .image
            .clone();
        assert_ne!(
            handle,
            ImageNode::default().image,
            "the model fixture must allocate a real raster"
        );
        app.world_mut().resource_mut::<AdaptiveTouchActions>().0 = false;
        app.update();
        let now = shown(&mut app);
        assert_eq!((now.0, now.1, now.2), (initial.0, initial.1, initial.2));
        assert_eq!(
            (now.3.as_str(), now.4),
            ("Aim", icons::semantic_index("aim").unwrap())
        );
        assert_eq!(
            app.world().get::<ImageNode>(initial.2).unwrap().image,
            handle
        );
        assert!(!app.world().resource::<Seen>().down);
    }
    #[test]
    fn changing_presentation_and_lifting_touch_preserves_an_independent_mouse_owner() {
        let (mut app, w, p) = scene();
        physical(&mut app, w, true);
        app.update();
        finger(&mut app, w, p, TouchPhase::Started);
        app.update();
        app.world_mut().resource_mut::<Wanted>().0 = AimRole::Block;
        finger(&mut app, w, p, TouchPhase::Ended);
        app.update();
        assert!(app.world().resource::<Seen>().down);
        assert!(!app.world().resource::<Seen>().released);
        physical(&mut app, w, false);
        app.update();
        assert!(!app.world().resource::<Seen>().down && app.world().resource::<Seen>().released);
    }
    #[test]
    fn focus_cancellation_and_controller_hand_off_keep_the_original_release_route() {
        let (mut app, w, p) = scene();
        finger(&mut app, w, p, TouchPhase::Started);
        app.update();
        app.world_mut().resource_mut::<Wanted>().0 = AimRole::Block;
        app.world_mut().write_message(WindowFocused {
            window: w,
            focused: false,
        });
        app.update();
        assert!(!app.world().resource::<Seen>().down);
        app.world_mut().write_message(WindowFocused {
            window: w,
            focused: true,
        });
        finger(&mut app, w, p, TouchPhase::Ended);
        app.update();
        finger(&mut app, w, p, TouchPhase::Started);
        app.update();
        assert!(app.world().resource::<Seen>().down);
        app.world_mut().resource_mut::<Bindings>().device = InputDevice::Controller;
        app.update();
        assert!(!app.world().resource::<Seen>().down);
        finger(&mut app, w, p, TouchPhase::Ended);
        app.update();
        app.world_mut().resource_mut::<Bindings>().device = InputDevice::KeyboardMouse;
        app.update();
        assert!(!app.world().resource::<Seen>().down);
    }
    #[test]
    fn the_presentation_adapter_does_not_release_a_latched_aggregate_owner() {
        let (mut app, _, _) = scene();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        for role in [AimRole::Block, AimRole::Scope, AimRole::Aim] {
            app.world_mut().resource_mut::<AimPresentation>().0 = role;
            app.world_mut().run_system_cached(show).unwrap();
            assert!(app
                .world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Right));
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Right);
        app.world_mut().run_system_cached(show).unwrap();
        assert!(!app
            .world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Right));
    }
    #[test]
    fn adaptive_latched_aim_survives_art_change_but_not_controller_takeover_or_return_tap() {
        use bevy::input::gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent};
        let (mut app, w, p) = scene();
        app.add_plugins(nvf_play::pad::InputSwitchPlugin);
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        app.world_mut()
            .resource_mut::<super::super::Layout>()
            .config
            .aiming
            .toggle_aim = true;
        app.world_mut().resource_mut::<Wanted>().0 = AimRole::Block;
        app.update();
        let initial = shown(&mut app);
        finger(&mut app, w, p, TouchPhase::Started);
        finger(&mut app, w, p, TouchPhase::Ended);
        app.update();
        assert!(app.world().resource::<Seen>().down);
        app.world_mut().resource_mut::<Wanted>().0 = AimRole::Scope;
        app.update();
        let scope = shown(&mut app);
        assert_eq!(
            (scope.0, scope.1, scope.2),
            (initial.0, initial.1, initial.2)
        );
        assert_eq!(scope.3, "Scope");
        assert!(app.world().resource::<Seen>().down);
        let pad_edge = |app: &mut App, down| {
            app.world_mut().write_message(RawGamepadEvent::Button(
                RawGamepadButtonChangedEvent::new(
                    pad,
                    GamepadButton::South,
                    if down { 1.0 } else { 0.0 },
                ),
            ));
        };
        pad_edge(&mut app, true);
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::Controller
        );
        assert!(!app.world().resource::<Seen>().down);
        app.update();
        assert!(
            !app.world()
                .resource::<super::super::TouchState>()
                .aim_latched
        );
        assert_eq!(
            *app.world().get::<Visibility>(initial.0).unwrap(),
            Visibility::Hidden
        );
        pad_edge(&mut app, false);
        app.update();
        finger(&mut app, w, p, TouchPhase::Started);
        finger(&mut app, w, p, TouchPhase::Ended);
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::KeyboardMouse
        );
        assert!(!app.world().resource::<Seen>().down);
        assert!(
            !app.world()
                .resource::<super::super::TouchState>()
                .aim_latched
        );
        assert_eq!(
            *app.world().get::<Visibility>(initial.0).unwrap(),
            Visibility::Inherited
        );
        assert_eq!(shown(&mut app).3, "Scope");
        finger(&mut app, w, p, TouchPhase::Started);
        finger(&mut app, w, p, TouchPhase::Ended);
        app.update();
        assert!(
            app.world().resource::<Seen>().down,
            "a subsequent deliberate tap can aim"
        );
    }

    #[test]
    fn adaptive_scope_presentation_cannot_rearm_a_latch_after_same_batch_drawer_commit() {
        let (mut app, w, p) = scene();
        app.world_mut()
            .resource_mut::<super::super::Layout>()
            .config
            .aiming
            .toggle_aim = true;
        app.world_mut()
            .resource_mut::<super::super::drawer::Options>()
            .enabled = true;
        app.world_mut().resource_mut::<Wanted>().0 = AimRole::Scope;
        app.update();
        let initial = shown(&mut app);
        finger(&mut app, w, p, TouchPhase::Started);
        finger(&mut app, w, p, TouchPhase::Ended);
        app.update();
        assert!(app.world().resource::<Seen>().down);
        let start = super::super::drawer::handle(Vec2::splat(1000.0), 24.0).center();
        for (phase, y) in [
            (TouchPhase::Started, start.y),
            (TouchPhase::Moved, start.y - 140.0),
        ] {
            app.world_mut().write_message(TouchInput {
                window: w,
                position: Vec2::new(start.x, y),
                id: 21,
                phase,
                force: None,
            });
            app.update();
        }
        app.world_mut().write_message(TouchInput {
            window: w,
            position: start - Vec2::new(0.0, 140.0),
            id: 21,
            phase: TouchPhase::Ended,
            force: None,
        });
        finger(&mut app, w, p, TouchPhase::Started);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Messages<nvf_play::menu::OpenInventory>>()
                .len(),
            1
        );
        assert!(!app.world().resource::<Seen>().down);
        assert!(
            !app.world()
                .resource::<super::super::TouchState>()
                .aim_latched
        );
        assert!(app
            .world()
            .resource::<super::super::TouchState>()
            .contacts
            .is_empty());
        let now = shown(&mut app);
        assert_eq!((now.0, now.1, now.2), (initial.0, initial.1, initial.2));
        assert_eq!(now.3, "Scope");
        app.update();
        assert!(
            !app.world().resource::<Seen>().down,
            "discarded touch must not replay next frame"
        );
    }
    fn authored_weapon_world(app: &mut App) -> (fnv_sim::FormId, fnv_sim::FormId) {
        use fnv_sim::{
            forms::{ActorBase, BaseData, BaseForm, Item, ItemKind},
            FormId, FormStore, RefTable, SimConfig,
        };
        let pistol = FormId(0xF001);
        let melee = FormId(0xF002);
        let mut forms = FormStore::default();
        let actor = BaseForm {
            id: FormId::PLAYER_BASE,
            sig: *b"NPC_",
            edid: Some("AuthoredPlayer".into()),
            name: None,
            script: None,
            prompt: None,
            voice_filter: false,
            data: BaseData::Actor(ActorBase {
                entries: vec![],
                attributes: None,
            }),
        };
        forms.sigs.insert(actor.id, actor.sig);
        forms.bases.insert(actor.id, actor);
        for id in [pistol, melee] {
            let item = Item {
                kind: ItemKind::Weapon,
                value: Some(1),
                money: None,
                weight: Some(1.0),
                slots: None,
                projectile: None,
                health: Some(100),
                non_playable: false,
                cant_drop: false,
                armor_class: Default::default(),
                power_armor: false,
                armor_dr: None,
                armor_dt: None,
                weapon_damage: Some(1.0),
                weight_mods: [0.0; 3],
                drawable: true,
                not_for_npcs: false,
                thrown: false,
                quest_item: false,
                icon: None,
                placed_count: None,
                teaches: None,
                cant_be_taken: false,
                modulates_voice: false,
                weapon_traits: None,
            };
            let base = BaseForm {
                id,
                sig: *b"WEAP",
                edid: Some(format!("AuthoredWeapon{}", id.0)),
                name: None,
                script: None,
                prompt: None,
                voice_filter: false,
                data: BaseData::Item(item),
            };
            forms.sigs.insert(id, base.sig);
            forms.bases.insert(id, base);
        }
        let forms = Box::leak(Box::new(forms));
        let refs = Box::leak(Box::new(RefTable::default()));
        let mut sim = nvf_play::sim::SimState {
            forms,
            refs,
            world: fnv_sim::World::new(forms, refs, SimConfig::default()),
        };
        for id in [pistol, melee] {
            sim.world.add_item(FormId::PLAYER_REF, id, 1).unwrap();
        }
        app.insert_resource(sim)
            .insert_resource(nvf_play::PlayWorld(nvf_play::PlayData {
                lo: Box::leak(Box::new(fnv_esm::LoadOrder::new(&[]).unwrap())),
                schema: Box::leak(Box::default()),
                index: Box::leak(Box::default()),
                world: 1,
                interior: None,
                eye: [0.0; 3],
                heading_deg: 0.0,
                pitch_deg: 0.0,
                auto_walk: 0.0,
                start_at_cell_spot: false,
                packages: false,
            }))
            .insert_resource(nvf_play::control::PlayerControl {
                weapon_out: true,
                ..default()
            })
            .insert_resource(nvf_play::combat::WeaponAnim {
                ready: true,
                ..default()
            });
        app.world_mut()
            .run_system_cached(nvf_play::combat::init_combat)
            .unwrap();
        {
            let mut combat = app.world_mut().resource_mut::<nvf_play::combat::Combat>();
            combat.forms.weapons.insert(
                pistol,
                fnv_combat::data::WeaponStats {
                    edid: Some("AuthoredPistol".into()),
                    anim_type: Some("One Hand Pistol".into()),
                    sight_fov: 35.0,
                    health: 100,
                    clip_size: 12,
                    ..default()
                },
            );
            combat.forms.weapons.insert(
                melee,
                fnv_combat::data::WeaponStats {
                    edid: Some("AuthoredMelee".into()),
                    anim_type: Some("Melee (1 Hand)".into()),
                    health: 100,
                    ..default()
                },
            );
        }
        app.world_mut().remove_resource::<Wanted>();
        app.add_systems(
            Update,
            nvf_play::control::read_controls.before(PublishPresentation),
        );
        (pistol, melee)
    }
    fn equip_authored(app: &mut App, weapon: fnv_sim::FormId) {
        app.world_mut()
            .resource_mut::<nvf_play::sim::SimState>()
            .world
            .equip(fnv_sim::FormId::PLAYER_REF, weapon)
            .unwrap();
        app.world_mut()
            .run_system_cached(nvf_play::combat::equip_sync)
            .unwrap();
        app.update();
        assert_eq!(
            app.world()
                .resource::<nvf_play::combat::Combat>()
                .player_weapon(),
            Some(weapon)
        );
    }
    #[test]
    fn equipping_an_actual_pistol_keeps_the_aim_target_visible_rasterized_and_actionable() {
        let (mut app, w, p) = scene();
        let (pistol, melee) = authored_weapon_world(&mut app);
        equip_authored(&mut app, melee);
        assert_eq!(app.world().resource::<AimPresentation>().0, AimRole::Block);
        let block = shown(&mut app);
        let old_handle = app.world().get::<ImageNode>(block.2).unwrap().image.clone();
        let old_pixels = app
            .world()
            .resource::<Assets<Image>>()
            .get(&old_handle)
            .unwrap()
            .data
            .clone();
        let layout = app
            .world()
            .resource::<super::super::Layout>()
            .config
            .encode();
        equip_authored(&mut app, pistol);
        let aim = shown(&mut app);
        assert_eq!(app.world().resource::<AimPresentation>().0, AimRole::Aim);
        assert_eq!(
            app.world()
                .resource::<nvf_play::combat::Combat>()
                .sight_fov(),
            Some((pistol, 35.0))
        );
        assert_eq!(aim.0, block.0);
        assert_eq!(aim.2, block.2);
        assert_eq!(
            *app.world().get::<Visibility>(aim.0).unwrap(),
            Visibility::Inherited
        );
        let node = app.world().get::<Node>(aim.0).unwrap();
        assert!(matches!(node.width,Val::Px(width) if width>=48.0));
        assert_eq!(aim.4, icons::semantic_index("aim").unwrap());
        let handle = app.world().get::<ImageNode>(aim.2).unwrap().image.clone();
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&handle)
            .unwrap();
        assert!(image
            .data
            .as_ref()
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0));
        assert_ne!(
            image.data, old_pixels,
            "the raster really changed from shield to aim"
        );
        assert_eq!(
            app.world()
                .resource::<super::super::Layout>()
                .config
                .encode(),
            layout
        );
        finger(&mut app, w, p, TouchPhase::Started);
        app.update();
        assert!(app.world().resource::<Seen>().down && app.world().resource::<Seen>().pressed);
        assert!(
            app.world()
                .resource::<nvf_play::control::PlayerControl>()
                .aiming
        );
        assert!(
            !app.world()
                .resource::<nvf_play::control::PlayerControl>()
                .trigger
        );
        finger(&mut app, w, p, TouchPhase::Ended);
        app.update();
        assert!(!app.world().resource::<Seen>().down && app.world().resource::<Seen>().released);
    }

    #[test]
    fn a_persisted_hidden_aim_is_an_explicit_layout_choice_not_a_pistol_capability_rule() {
        let (mut app, _, _) = scene();
        let (pistol, melee) = authored_weapon_world(&mut app);
        let mut saved = app
            .world()
            .resource::<super::super::Layout>()
            .config
            .encode();
        saved["buttons"]["Aim"]["visible"] = serde_json::json!(false);
        app.world_mut()
            .resource_mut::<super::super::Layout>()
            .config = super::super::layout::Config::decode(&saved).unwrap();
        for weapon in [melee, pistol] {
            equip_authored(&mut app, weapon);
            let now = shown(&mut app);
            assert_eq!(
                *app.world().get::<Visibility>(now.0).unwrap(),
                Visibility::Hidden
            );
            assert_eq!(
                app.world()
                    .resource::<super::super::Layout>()
                    .config
                    .encode()["buttons"]["Aim"]["visible"],
                false
            );
        }
        app.world_mut()
            .resource_mut::<super::super::Layout>()
            .config
            .buttons[6]
            .visible = true;
        app.update();
        let now = shown(&mut app);
        assert_eq!(
            *app.world().get::<Visibility>(now.0).unwrap(),
            Visibility::Inherited
        );
    }
    #[test]
    fn pistol_aim_participates_in_global_controller_autohide_and_the_consumed_return_touch() {
        let (mut app, w, p) = scene();
        let (pistol, _) = authored_weapon_world(&mut app);
        equip_authored(&mut app, pistol);
        app.add_plugins(nvf_play::pad::InputSwitchPlugin);
        app.world_mut().spawn(Gamepad::default());
        app.world_mut().resource_mut::<Bindings>().device = InputDevice::Controller;
        app.update();
        let aim = shown(&mut app);
        assert_eq!(
            *app.world().get::<Visibility>(aim.0).unwrap(),
            Visibility::Hidden
        );
        let world = app.world_mut();
        let mut q = world.query::<(&TouchVisual, &Visibility)>();
        assert!(q
            .iter(world)
            .any(|(v, visibility)| matches!(v, TouchVisual::Button(7))
                && *visibility == Visibility::Hidden));
        finger(&mut app, w, p, TouchPhase::Started);
        finger(&mut app, w, p, TouchPhase::Ended);
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::KeyboardMouse
        );
        assert!(!app.world().resource::<Seen>().down);
        assert_eq!(
            *app.world().get::<Visibility>(aim.0).unwrap(),
            Visibility::Inherited
        );
        finger(&mut app, w, p, TouchPhase::Started);
        app.update();
        assert!(app.world().resource::<Seen>().down);
    }
}
