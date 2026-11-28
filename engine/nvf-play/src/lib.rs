//! nvf-play — Layer 4 in the game: the player (physics-driven first-person controller),
//! activation through the crosshair, and container and inventory menus, on top of the
//! headless `fnv-sim` (references, inventories, activation, clock) and `fnv-phys`
//! (collision, character controller). Design and part ownership:
//! docs/design/layer4-play.md.
//!
//! `nvforever --play` adds [`PlayPlugin`]; without it the binary stays the M3 explorer.
//!
//! Layer 5 (workstream 5D, docs/design/layer5-play.md): [`script`] runs the `fnv-game`
//! runtime (quest and object scripts, events, messages) next to the simulation.
//! [`dialogue`] is the screen for its conversations (docs/design/dialogue-play.md);
//! [`dialogue_look`] turns the player's view to the actor they talk to.

pub mod after_combat;
pub mod game_files;
pub mod activate;
pub mod aid;
pub mod aim_zoom;
pub mod touch_action_presentation;
pub mod touch_movement;
pub mod ambient;
pub mod audio_out;
pub mod bindings;
pub mod blocking;
pub mod bullet_snap;
pub mod chargen;
pub mod clutter_pose;
pub mod collision_view;
pub mod creator;
pub mod combat;
pub mod compass;
pub mod compass_strip;
pub mod console;
pub mod soft_keyboard;
pub mod editor;
pub mod container_anim;
pub mod control;
pub mod controller_aim;
pub mod coop_downed;
pub mod coop_mates;
pub mod coop_subtitles;
pub mod coop_progress;
pub mod corner_messages;
pub mod crafting;
pub mod cursor;
pub mod crime;
pub mod dialogue;
pub mod dialogue_look;
pub mod dialogue_zoom;
pub mod dialogue_menu;
pub mod dialogue_device;
pub mod dialogue_touch;
pub mod dialogue_split;
pub mod touch_safe_area;
pub mod touch_ui_options;
pub mod difficulty;
pub mod hardcore;
pub mod dismember;
pub mod doors;
pub mod created_spots;
pub mod dropped;
pub mod ease;
pub mod effect_sounds;
pub mod fade;
pub mod drowning;
pub mod falls;
pub mod file_bytes;
pub mod key_repeat;
pub mod query_job;
pub mod swing_traps;
pub mod flashlight;
pub mod free_cam;
pub mod script_camera;
pub mod photo_lights;
pub mod photo_mode;
pub mod shake;
pub mod sprint;
pub mod game_start;
pub mod furniture;
pub mod grab;
pub mod hitvol;
pub mod inertia;
pub mod holotape;
pub mod interface;
pub mod intro;
pub mod levelup;
pub mod hotkeys;
pub mod hud;
pub mod hud_readability;
pub mod hud_breath;
pub mod hud_limbs;
pub mod heartbeat;
pub mod marker_added;
pub mod vanity;
pub mod grenade_indicator;
pub mod hud_needs;
pub mod hud_target;
pub mod aim_assist;
pub mod knockouts;
pub mod loading;
pub mod loading_look;
pub mod lobby;
pub mod main_menu;
pub mod menu_scale;
pub mod menu_readability;
pub mod location_markers;
pub mod lockpick;
pub mod roulette;
pub mod slots;
pub mod smooth_terrain;
pub mod softlock;
pub mod blackjack;
pub mod borders;
pub mod companion_wheel;
pub mod caravan;
pub mod challenges;
pub mod poker;
pub mod flourish;
pub mod casino_hud;
pub mod menu;
pub mod menu_shapes;
pub mod mod_log;
pub mod gas_traps;
pub mod grass_fire;
pub mod mines;
pub mod thrown;
pub mod perk_marks;
pub mod message_menu;
pub mod moved;
pub mod music;
pub mod pad;
pub mod full_screen_key;
pub mod pause;
pub mod vector_font;
pub mod graphics_resume;
pub mod phys;
pub mod phys_load;
pub mod phys_rate;
pub mod pickpocket;
pub mod pipboy;
pub mod player;
pub mod quest_messages;
pub mod radiation;
pub mod radio;
pub mod reverb;
pub mod repair;
pub mod maintenance_touch;
pub mod repeat;
pub mod reputation;
pub mod quick_loot;
pub mod reticle;
pub mod rumble;
pub mod save;
pub mod screen_splatter;
pub mod script;
pub mod script_input;
pub mod script_switches;
mod script_texture_filter;
#[cfg(target_os = "windows")]
mod script_cursor;
pub mod script_draw;
pub mod script_events;
pub mod script_hud;
pub mod jip_physics;
pub mod script_quantity;
pub mod sup_screen;
pub mod sup_shots;
pub mod script_trigger;
pub mod settings;
pub mod settings_layout;
pub mod settings_menu;
pub mod settings_view;
pub mod start_menu;
pub mod sim;
pub mod space;
pub mod stance;
pub mod terminal;
pub mod tiles;
pub mod tutorial_menu;
pub mod tweaks;
pub mod ui_pointer;
pub mod credits;
mod test_aid;
pub use test_aid::melee_knockback::MeleeKnockbackAid;
pub use test_aid::{drives_run, MEASURING_AIDS};
pub mod sfx;
pub mod start_read;
pub mod sneak_gauges;
pub mod sound_cover;
pub mod sound_grid;
pub mod ui_colour;
pub mod language;
pub mod ui_sounds;
pub mod stress;
pub mod vats_classic;
pub mod vats_mode;
pub mod kill_cam;
pub mod weapon_wheel;
pub mod voice;
pub mod voice_filter;
pub mod wait;
pub mod world_sounds;
pub mod xp_meter;

mod cjk_font;

use bevy::prelude::*;
use fnv_esm::{LoadOrder, Schema};
use fnv_world::WorldIndex;

/// What the game hands the plugin: the plugins as loaded (shared with render streaming,
/// hence `'static`), the start pose and test aids.
#[derive(Clone, Copy)]
pub struct PlayData {
    pub lo: &'static LoadOrder<'static>,
    pub schema: &'static Schema,
    pub index: &'static WorldIndex,
    /// The worldspace the game starts in (FormID). Where the player is later is
    /// [`Location`]: door travel can change the worldspace.
    pub world: u32,
    /// Start inside this interior cell instead of the worldspace.
    pub interior: Option<u32>,
    /// Start eye position, game space.
    pub eye: [f32; 3],
    /// Compass heading in degrees (0 north, 90 east).
    pub heading_deg: f32,
    /// Pitch in degrees (negative looks down).
    pub pitch_deg: f32,
    /// Test aid: walk forward at this speed (game units per second); 0 = off.
    pub auto_walk: f32,
    /// No start position was asked for: an interior start goes where the console's `coc`
    /// puts the player (`player::safe::cell_spot`), facing that way.
    pub start_at_cell_spot: bool,
    /// Actors live by their AI packages (nvforever's residents): the game runs the
    /// packages' begin, end and change scripts and idles from the start, the player's
    /// script packages included (`fnv_game::ai`).
    pub packages: bool,
}

/// The controls in `--play`, for nvforever's help line.
pub const HELP: &str =
    "WASD move  Shift walk  Caps Lock always run  Ctrl sneak  Q auto move  Space jump  mouse look  E use  Tab inventory (hold: flashlight)  T wait  Esc pause  [ ] hour  F12 screenshot";

/// [`PlayData`] as a resource, for systems that read the plugins.
#[derive(Resource, Clone, Copy, Deref)]
pub struct PlayWorld(pub PlayData);

/// Whether the player is in the world, in a menu (no movement or looking in menus), in
/// a conversation or behind a loading screen.
#[derive(States, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PlayState {
    #[default]
    Playing,
    Menu,
    /// Talking to an actor ([`dialogue`]): a pausing menu like `Menu` (the player, the
    /// world and the game clock stand still, the cursor is free) with the dialogue
    /// screen's own keys; the actor's lines go on in real time.
    Dialogue,
    /// Behind a loading screen ([`loading`]): nothing happens in the world until the
    /// destination is loaded.
    Loading,
}

/// Order of the play systems within `Update`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaySet {
    /// Read input (keys, mouse).
    Input,
    /// Stream physics cells, step the world, move the player and the camera.
    Physics,
    /// Crosshair ray, prompt, activation.
    Activation,
    /// Layer 5 scripts: events, the frame's tick, their effects (`script`).
    Scripts,
    /// Menus.
    Ui,
    /// Bring render entities in line with the simulation.
    Sync,
}

/// Marks the `Camera3d` that is the player's eyes.
#[derive(Component)]
pub struct PlayerCamera;

/// Marks a render entity spawned for a reference (its FormID): by nvforever for placed
/// references, by [`dropped`] for references the simulation created (dropped items), so
/// the game can hide what the simulation removed and the crosshair can find items by
/// their render bounds.
///
/// Immutable: giving an entity another reference means inserting a new `RefVisual`, so
/// its hooks see every change and keep [`hitvol::ActorBoundsCandidate`] right.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[component(immutable, on_insert = hitvol::ref_visual_inserted, on_remove = hitvol::ref_visual_removed)]
pub struct RefVisual(pub u32);

/// Beside [`RefVisual`] on every entity nvforever's full-detail scenery ring draws (its
/// `full_detail.rs`, #2701): the full model of a reference in a cell beyond the loaded
/// ones, drawn only. The simulation's state still shows and hides it ([`activate`]'s
/// `sync_visuals`), but nothing that acts on what is loaded reads it: the console's and
/// the editor's picks, the crosshair, the loaded-reference counts and the scripts' NIF
/// functions leave it out (`Without<DrawOnly>`) [design].
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawOnly;

/// On the entity of a cell nvforever's full-detail ring draws (its `full_detail.rs`): no
/// [`RenderCell`] (nothing of the game draws into it), but the parent of draw-only copies
/// that [`moved`] moves with their references.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawOnlyCell;

/// The drawn parts of references that a ray or a box can pick ([`console::pick_under`],
/// the crosshair, the outlines): every [`RefVisual`] but the draw-only ones ([`DrawOnly`]).
pub type PickVisuals<'w, 's> = Query<'w, 's, (&'static RefVisual, &'static bevy::camera::primitives::Aabb, &'static GlobalTransform, &'static InheritedVisibility), Without<DrawOnly>>;

#[cfg(test)]
mod draw_only_tests {
    use super::*;
    use bevy::camera::primitives::Aabb;
    use bevy::ecs::system::RunSystemOnce;

    /// #2701: the picks (console, editor, crosshair, outlines) see a loaded reference's
    /// drawn parts and never the full-detail ring's draw-only ones.
    #[test]
    fn picks_leave_draw_only_parts_out() {
        let mut world = World::new();
        let part = (Aabb::default(), GlobalTransform::IDENTITY, InheritedVisibility::VISIBLE);
        world.spawn((RefVisual(7), part));
        world.spawn((RefVisual(8), part, DrawOnly));
        let seen = world.run_system_once(|v: PickVisuals| v.iter().map(|(r, ..)| r.0).collect::<Vec<u32>>()).unwrap();
        assert_eq!(seen, vec![7]);
    }
}

/// On the root of a reference's own node hierarchy (an animated model spawned with
/// `nvf_assets::model::Rig::spawn_nodes`: nvforever's doors, containers, activators and
/// scripted models, [`script_draw`]'s enabled references): the reference and its node
/// entities by their names as the NIF spells them (the first of a repeated name), for what
/// follows a named node (AttachUIComponent, nvforever `ui_attach.rs`). Those node entities
/// carry no `Name` [design: #1835].
///
/// Immutable, so whoever indexes it by reference sees every change in its hooks.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct RefNodes {
    pub reference: u32,
    pub nodes: std::collections::HashMap<String, Entity>,
}

/// Put by nvforever on the entity of every render cell it streams in (its transform is
/// the cell's origin in game space, under the world root). [`dropped`] spawns the
/// references the simulation created in that cell as its children, so they come and go
/// with render streaming without nvforever knowing them.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RenderCell {
    /// An exterior grid square of a worldspace (FormID).
    Exterior { world: u32, x: i32, y: i32 },
    /// An interior cell (FormID).
    Interior(u32),
}

/// Where the player is now: the worldspace, and the interior cell when inside one. Set
/// from [`PlayData`] at the start; physics changes it when it applies a [`Travel`]
/// (PlaySet::Physics), so later sets of the same frame see the destination.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location {
    /// The worldspace (FormID): the one being played, or, inside an interior, the one
    /// the player last walked in.
    pub world: u32,
    pub interior: Option<u32>,
}

/// Move the player to another place: through a door into an interior (`interior:
/// Some(cell)`) or out into a worldspace (`interior: None`; `world` names it, and may
/// differ from the one being played). Render streaming and physics both follow it.
/// Activation and scripts ask for it with a [`loading::TravelRequest`]; it goes out once
/// the loading screen is up.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct Travel {
    /// The destination worldspace (FormID) for travel outdoors; `None` into an interior,
    /// or outdoors meaning the current worldspace.
    pub world: Option<u32>,
    pub interior: Option<u32>,
    /// Destination feet position, game space.
    pub feet: [f32; 3],
    /// Compass heading in degrees on arrival.
    pub heading_deg: f32,
    /// The view's pitch in degrees on arrival (negative looks down, as `Player::pitch`);
    /// `None` keeps the pitch the player had. A script's MoveTo of the player sets it to
    /// the marker's (`fnv_game` `move_ref`): the add-ons' intro slides and the ending's
    /// slides are framed for a level view, and the player cannot look there (#2710).
    pub pitch_deg: Option<f32>,
}

impl Travel {
    /// Where the player is after this travel, leaving `from`. Into an interior the
    /// worldspace stays the one left.
    pub fn destination(&self, from: Location) -> Location {
        match self.interior {
            Some(_) => Location { world: from.world, interior: self.interior },
            None => Location { world: self.world.unwrap_or(from.world), interior: None },
        }
    }
}

/// Open the container menu on this container reference.
#[derive(Message, Clone, Copy, Debug)]
pub struct OpenContainer {
    pub container: u32,
}

/// The simulation changed whether a reference is present (taken, dropped, enabled,
/// disabled): visuals and collision must follow. Sent for placed references and for
/// references the simulation creates (a drop from the inventory sends one per new
/// reference; taking a dropped item back sends one for it).
///
/// `restored`: sent by a load for a reference as the save has it, not for anything that
/// happened now. Visuals and collision follow it all the same; what reads a change as a
/// deed of this moment (`crate::crime`, `crate::world_sounds`) must not.
///
/// `taker`: an actor other than the player that picked the reference up (a script's
/// `ref.Activate actor`, #3918), when known; such a pick-up is not the player's theft.
#[derive(Message, Clone, Copy, Debug)]
pub struct RefChanged {
    pub form_id: u32,
    pub restored: bool,
    pub taker: Option<u32>,
}

impl RefChanged {
    /// A change happening now.
    pub fn new(form_id: u32) -> RefChanged {
        RefChanged { form_id, restored: false, taker: None }
    }

    /// A load's change: the reference as the save has it.
    pub fn restored(form_id: u32) -> RefChanged {
        RefChanged { form_id, restored: true, taker: None }
    }
}

/// A script or the console moved or turned a placed or created reference (SetPos, SetAngle,
/// Rotate, SetScale: `fnv_game::Game::turned`, and what it held before the tick cleared it):
/// its collision follows when the settle write-back would otherwise undo the move
/// (`crate::phys::PhysState::repose_ref`), and its drawn model follows (`crate::moved`,
/// `crate::script_draw`).
#[derive(Message, Clone, Copy, Debug)]
pub struct RefRepose {
    pub form_id: u32,
}

/// The player used up an item from the inventory (an aid item): the script runtime
/// uses it (`crate::aid`, `fnv_game::Game::ingest`: its effects, effect scripts
/// included, with potency, the usage monitor and the addiction roll).
/// The menu has already removed the item from the simulation.
#[derive(Message, Clone, Copy, Debug)]
pub struct ItemUsed {
    pub item: u32,
    /// Used on one limb (its condition code, 25-30): the Pip-Boy's CND page puts a
    /// Stimpak on the limb chosen (`fnv_game::Game::ingest_on`).
    pub limb: Option<u16>,
}

pub struct PlayPlugin(pub PlayData);

impl Plugin for PlayPlugin {
    fn build(&self, app: &mut App) {
        let d = self.0;
        // Populate `fnv_game::mod_files` read roots before `SimState::new` and
        // `PostStartup` settings load so Stewie modular override folders
        // (`NVSE\Plugins\Tweaks\INIs\*.ini`, `GameSettings\*.ini`) are visible at start.
        if !app.world().contains_resource::<script_input::ModFileRootsReady>() {
            if let Some(game_data) = app.world().get_resource::<nvf_assets::GameData>() {
                script_input::init_mod_file_roots(game_data);
                app.insert_resource(script_input::ModFileRootsReady);
            }
        }
        // Whose record names a file that is not there (game_files.rs, #2757).
        game_files::note_load_order(d.lo);
        app.insert_resource(PlayWorld(d))
            .insert_resource(Location { world: d.world, interior: d.interior })
            .insert_resource(sim::SimState::new(d.lo, d.schema, d.index))
            .init_state::<PlayState>()
            .add_message::<Travel>()
            .add_message::<OpenContainer>()
            .add_message::<RefChanged>()
            .add_message::<ItemUsed>()
            .add_message::<furniture::UseFurniture>()
            .add_message::<world_sounds::Activated>()
            .add_message::<world_sounds::DoorPassed>()
            .add_message::<world_sounds::ImpactPicked>()
            .add_message::<ui_sounds::UiSound>()
            .add_systems(First, cjk_font::update)
            .init_resource::<furniture::PlayerSeat>()
            .configure_sets(Update, (PlaySet::Input, PlaySet::Physics, PlaySet::Activation, PlaySet::Scripts, PlaySet::Ui, PlaySet::Sync).chain())
            .add_plugins((sim::SimPlugin, phys::PhysPlugin, player::PlayerPlugin, activate::ActivatePlugin, menu::MenuPlugin, dropped::DroppedPlugin, moved::MovedPlugin, pause::PausePlugin, wait::WaitPlugin, flashlight::FlashlightPlugin))
            .add_plugins((script::ScriptPlugin, hitvol::HitVolumePlugin, combat::CombatPlugin, bullet_snap::BulletSnapPlugin, sfx::SfxPlugin, music::MusicPlugin, ambient::AmbientPlugin, radio::RadioPlugin, voice::VoicePlugin, holotape::HolotapePlugin, loading::LoadingPlugin, dialogue_look::DialogueLookPlugin, hud::HudPlugin, compass::CompassPlugin))
            .add_plugins((world_sounds::WorldSoundsPlugin, effect_sounds::EffectSoundsPlugin, reticle::ReticlePlugin, quick_loot::QuickLootPlugin, xp_meter::XpMeterPlugin, radiation::RadiationPlugin, borders::BordersPlugin))
            .add_plugins((ui_sounds::UiSoundsPlugin, pad::PadPlugin, key_repeat::KeyRepeatPlugin, rumble::RumblePlugin, shake::ShakePlugin, script_input::ScriptInputPlugin, aim_assist::AimAssistPlugin))
            .add_plugins(script_switches::ScriptSwitchesPlugin)
            .add_plugins(script_texture_filter::ScriptTextureFilterPlugin)
            .add_plugins((chargen::ChargenPlugin, creator::CreatorPlugin, fade::FadePlugin))
            .add_plugins((levelup::LevelUpPlugin, sneak_gauges::SneakGaugesPlugin, sound_cover::SoundCoverPlugin))
            .add_plugins(perk_marks::PerkMarksPlugin)
            .add_plugins(softlock::SoftlockPlugin)
            .add_plugins(sup_screen::SupScreenPlugin)
            .add_plugins(sup_shots::SupShotsPlugin)
            .add_plugins(tiles::TilesPlugin)
            .add_plugins(pipboy::PipBoyPlugin)
            .add_plugins(terminal::TerminalPlugin)
            .add_plugins((coop_downed::CoopDownedPlugin, coop_mates::CoopMatesPlugin, coop_subtitles::CoopSubtitlesPlugin))
            .add_plugins(tutorial_menu::TutorialMenuPlugin)
            .add_plugins(credits::CreditsPlugin)
            .add_plugins(lockpick::LockpickPlugin)
            .add_plugins(roulette::RoulettePlugin)
            .add_plugins(slots::SlotsPlugin)
            .add_plugins(blackjack::BlackjackPlugin)
            .add_plugins(companion_wheel::CompanionWheelPlugin)
            .add_plugins(caravan::CaravanPlugin)
            .add_plugins(poker::PokerPlugin)
            .add_plugins((crime::CrimePlugin, reputation::ReputationPlugin, knockouts::KnockoutsPlugin, after_combat::AfterCombatPlugin))
            .add_plugins((console::ConsolePlugin, soft_keyboard::SoftKeyboardPlugin))
            .add_plugins(editor::EditorPlugin)
            .add_plugins(location_markers::LocationMarkersPlugin)
            .add_plugins(collision_view::CollisionViewPlugin)
            .add_plugins(free_cam::FreeCamPlugin)
            .add_plugins(script_camera::ScriptCameraPlugin)
            .add_plugins(photo_mode::PhotoModePlugin)
            .add_plugins(aim_zoom::AimZoomPlugin)
            .add_plugins(save::SavePlugin)
            .add_plugins(main_menu::MainMenuPlugin)
            .add_plugins(lobby::LobbyPlugin)
            .add_plugins(ui_pointer::UiPointerPlugin)
            .add_plugins(intro::IntroPlugin)
            .add_plugins(screen_splatter::ScreenSplatterPlugin)
            .add_plugins(cursor::CursorPlugin)
            .add_plugins(crafting::CraftingPlugin)
            .add_plugins(jip_physics::JipPhysicsPlugin)
            .add_plugins(script_quantity::ScriptQuantityPlugin)
            .add_plugins(mines::MinesPlugin)
            .add_plugins(grass_fire::GrassFirePlugin)
            .add_plugins(thrown::ThrownPlugin)
            .add_plugins(falls::FallsPlugin)
            .add_plugins(swing_traps::SwingTrapsPlugin)
            .add_plugins(drowning::DrowningPlugin)
            .add_plugins(pickpocket::PickpocketPlugin)
            .add_plugins(repair::RepairPlugin)
            .add_plugins(container_anim::ContainerAnimPlugin)
            .add_plugins(vats_mode::VatsModePlugin)
            .add_plugins(kill_cam::KillCamPlugin)
            .add_plugins(weapon_wheel::WeaponWheelPlugin)
            .add_plugins(grab::GrabPlugin)
            .add_plugins(difficulty::DifficultyPlugin)
            .add_plugins(tweaks::TweaksPlugin)
            .add_plugins(hardcore::HardcorePlugin)
            .add_plugins((ui_colour::UiColourPlugin, language::LanguagePlugin))
            .add_plugins((casino_hud::CasinoHudPlugin, flourish::FlourishPlugin));
        test_aid::setup(app);
        stress::setup(app);
        settings::setup(app);
        full_screen_key::setup(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOJAVE: u32 = 0xDA726;
    const FREESIDE: u32 = 0x12D94D;
    const HOUSE: u32 = 0x100;

    fn travel(world: Option<u32>, interior: Option<u32>) -> Travel {
        Travel { world, interior, feet: [0.0; 3], heading_deg: 0.0, pitch_deg: None }
    }

    /// Every plugin of this crate is added somewhere (here or by nvforever): the crosshair's
    /// plugin was once written and never added, so the crosshair was not drawn (the
    /// playtest of 2026-10-11).
    #[test]
    fn every_plugin_is_added() {
        fn sources(dir: &std::path::Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    sources(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(std::fs::read_to_string(&p).unwrap());
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut texts = Vec::new();
        sources(&root.join("src"), &mut texts);
        sources(&root.join("../nvforever/src"), &mut texts);
        let plugins: Vec<String> = texts
            .iter()
            .flat_map(|t| t.lines())
            .filter_map(|l| l.trim().strip_prefix("impl Plugin for ").and_then(|r| r.split_whitespace().next()).map(str::to_string))
            .collect();
        assert!(plugins.len() > 30, "{plugins:?}");
        let added = |p: &str| {
            let word = |l: &str| l.split(|c: char| !c.is_alphanumeric() && c != '_').any(|w| w == p);
            texts.iter().flat_map(|t| t.lines()).filter(|l| word(l)).any(|l| {
                let l = l.trim();
                !l.starts_with("impl Plugin for") && !l.starts_with("pub struct") && !l.starts_with("struct") && !l.starts_with("//")
            })
        };
        let missing: Vec<&String> = plugins.iter().filter(|p| !added(p)).collect();
        assert!(missing.is_empty(), "plugins never added: {missing:?}");
    }

    #[test]
    fn travel_changes_the_location() {
        let outside = Location { world: MOJAVE, interior: None };
        // Into an interior: the worldspace is kept for the way back out.
        let inside = travel(None, Some(HOUSE)).destination(outside);
        assert_eq!(inside, Location { world: MOJAVE, interior: Some(HOUSE) });
        // Out into the same worldspace, named or not.
        assert_eq!(travel(Some(MOJAVE), None).destination(inside), outside);
        assert_eq!(travel(None, None).destination(inside), outside);
        // Through a gate into another worldspace, and back.
        let freeside = travel(Some(FREESIDE), None).destination(outside);
        assert_eq!(freeside, Location { world: FREESIDE, interior: None });
        assert_eq!(travel(Some(MOJAVE), None).destination(freeside), outside);
        // From an interior out into another worldspace than the one it was entered from.
        assert_eq!(travel(Some(FREESIDE), None).destination(inside), freeside);
    }
}
