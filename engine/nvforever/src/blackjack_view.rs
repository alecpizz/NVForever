//! The blackjack table (`nvf_play::blackjack`, docs/spec-notes/blackjack.md §6): the
//! game's models of the casino's table, the dealer's cards and the player's two hands,
//! drawn by a camera of their own into a picture the menu shows behind its text
//! ([`nvf_play::blackjack::BlackjackView`]).
//!
//! - The table is the casino's MOD3 model (`NV_Blackjack-Table<x>.NIF`); the cards are
//!   `NV_Blackjack-Dealer.NIF`, `-Hand1.NIF` and `-Hand2.NIF`, seven cards each, all at the
//!   shoe until a sequence moves them [observed: the files' nodes and sequences].
//! - A card dealt is posed by its own deal sequence (`Hand1_03` for the player's third)
//!   from when it left the shoe and held at its end. The model's hole card is its first,
//!   `Dealer_01`: its deal lays it face down, `Dealer_02`'s lays the second face up
//!   [observed: buckallobill, in game, FNV 1.4.0.525: frames of a round]. The rules deal the hole card
//!   second, so the dealer's first two faces change places ([`dealer_face_slot`]) and the
//!   first card turns by `Dealer_Reveal` once revealed; the last round's cards by `<model>_Discard` as the next
//!   deal sweeps them away. Cards not on the table are hidden [hypothesis: each sequence
//!   holds every card of its model; NVForever takes from it only its own card's track].
//! - A card's face is the shape named `<card>:n`: its picture is set to the card's face
//!   from the casino's deck (`CasinoDef::face_texture`) [hypothesis: the face shapes
//!   carry a stand-in picture]. The back and shadow are the file's.
//! - The bet's chips: the game's chip models (`NV_Blackjack-Chip_100/025/010/005/001.NIF`).
//!   Each is a column 2 units high and 1.7 across whose top lies just under the felt, each
//!   value at its own place [observed: the files' nodes and bounds], so the original places
//!   them itself. NVForever draws each chip as its model squeezed to [`CHIP_THICK`], stacked
//!   by value just in front of the seat's cards ([`BET_FORWARD`]) [design].
//! - The table's two green arrows (`Hand1_Arrow:0`, `Hand1_Arrow:0@#2`) are shown only
//!   after a split, the one at the hand being played [hypothesis: they mark the active
//!   hand; the owner's screenshot of an unsplit hand shows none; which arrow is which hand
//!   by their places].
//! - The camera looks from the player's side over the player's cards towards the dealer's
//!   [design: fitted to the screenshot], with lights of its own on render layer
//!   [`TABLE_LAYER`].

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::AmbientLight;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::PrimaryWindow;
use fnv_game::blackjack::{Card, Seat};
use nvf_assets::anim_play::Clip;
use nvf_assets::model::{ModelCtx, Rig};
use nvf_assets::nif_material::NifMaterial;
use nvf_play::blackjack::{BlackjackScreen, BlackjackView, Shown, Session};
use nvf_play::pause::AidTime;
use std::collections::HashMap;

pub const DEALER_MODEL: &str = "meshes\\terminals\\nv_blackjack\\nv_blackjack-dealer.nif";
pub const HAND_MODELS: [&str; 2] = ["meshes\\terminals\\nv_blackjack\\nv_blackjack-hand1.nif", "meshes\\terminals\\nv_blackjack\\nv_blackjack-hand2.nif"];
/// The chips by value, highest first [observed: the CSNO records' model list].
pub const CHIPS: [(u32, &str); 5] = [
    (100, "meshes\\terminals\\nv_blackjack\\nv_blackjack-chip_100.nif"),
    (25, "meshes\\terminals\\nv_blackjack\\nv_blackjack-chip_025.nif"),
    (10, "meshes\\terminals\\nv_blackjack\\nv_blackjack-chip_010.nif"),
    (5, "meshes\\terminals\\nv_blackjack\\nv_blackjack-chip_005.nif"),
    (1, "meshes\\terminals\\nv_blackjack\\nv_blackjack-chip_001.nif"),
];
/// The render layer of the table, its lights and its camera (lockpicking has 7).
pub const TABLE_LAYER: usize = 9;
/// Where the table is put, far from the world (Bevy space) [design].
const PLACE: Vec3 = Vec3::new(200_000.0, -200_000.0, 0.0);
/// The camera (game units, the table's space): aimed between the player's first card and
/// the dealer's, from this far back towards the player and this high, with this field of
/// view [design: fitted by eye to the owner's screenshot].
const CAMERA_BACK: f32 = 26.0;
const CAMERA_UP: f32 = 26.0;
const AIM_TOWARDS_DEALER: f32 = 0.4;
const FOV_DEG: f32 = 50.0;
/// Each chip this thick, the stacks this far apart [hypothesis: by the screenshot's look].
const CHIP_THICK: f32 = 0.3;
const CHIP_STACK_GAP: f32 = 2.0;
/// The bet's spot: this far in front of the seat's first card (towards the player) and
/// this far to its left [design: the chip models' own spot, `Chips_1` at (-9.9, 0.25) in
/// NV_Blackjack-Chip_001.NIF, is by the box on the right]; the felt this far under the
/// cards [hypothesis].
const BET_FORWARD: f32 = 4.5;
const BET_LEFT: f32 = 1.0;
const FELT_BELOW_CARDS: f32 = 0.12;
/// The least distance from one card of a hand to the next, so each card's corner shows
/// [design: fitted by eye; the felt's boxes are about 12 units apart].
const CARD_SPREAD: f32 = 1.5;
/// Most chips in one stack [design].
const STACK_MAX: u32 = 20;
pub struct BlackjackViewPlugin;

impl Plugin for BlackjackViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, show.after(nvf_play::PlaySet::Ui).before(nvf_play::PlaySet::Sync));
    }
}

/// A card model as drawn: its rig, its nodes' entities by name, and its seven cards' node
/// indices, face shapes and the picture each face shows now.
struct CardModel {
    rig: Rig,
    /// The model's prefix: `Dealer`, `Hand1`, `Hand2`.
    prefix: &'static str,
    cards: Vec<CardSlot>,
}

struct CardSlot {
    node: usize,
    /// Moved this far (the card's parent space) once dealt, so the hand fans out.
    spread: Vec3,
    entity: Entity,
    faces: Vec<Entity>,
    showing: Option<Card>,
}

impl CardModel {
    /// `across`: the way a hand fans out when its sequences leave the cards in one place
    /// (the table's space, flat).
    fn new(rig: Rig, prefix: &'static str, by_name: &HashMap<String, Entity>, shapes: &[Entity], across: Vec3) -> Option<CardModel> {
        let mut cards = Vec::new();
        let spreads = spreads(&rig, prefix, across);
        for n in 1..=7 {
            let name = format!("{prefix}_{n:02}");
            let node = node_index(&rig, &name)?;
            let entity = *by_name.iter().find(|(k, _)| k.eq_ignore_ascii_case(&name))?.1;
            // The face: a shape named `<card>:n` (not the back's or the shadow's).
            let face_prefix = format!("{name}:").to_ascii_lowercase();
            let faces = rig
                .shapes
                .iter()
                .zip(shapes)
                .filter(|(s, _)| rig.nodes[s.node].name.as_deref().is_some_and(|m| m.to_ascii_lowercase().starts_with(&face_prefix)) || (s.node == node))
                .map(|(_, e)| *e)
                .collect();
            let spread = spreads.get(n - 1).copied().unwrap_or(Vec3::ZERO);
            cards.push(CardSlot { node, entity, spread, faces, showing: None });
        }
        Some(CardModel { rig, prefix, cards })
    }

    fn clip(&self, name: &str) -> Option<&Clip> {
        self.rig.group(name)
    }
}

/// Each card's move (its parent's space) that puts it at least [`CARD_SPREAD`] past the
/// one before, along the way the deal sequences lay the hand out (the first card towards
/// the second, flat on the felt), else `across` [design: the sequences leave the cards
/// almost on top of each other, so only the top one reads [observed: buckallobill, in game, FNV 1.4.0.525]].
fn spreads(rig: &Rig, prefix: &str, across: Vec3) -> Vec<Vec3> {
    let ends: Vec<Option<(usize, Vec3)>> = (1..=7)
        .map(|n| {
            let name = format!("{prefix}_{n:02}");
            node_index(rig, &name).map(|i| (i, end_of(rig, rig.group(&name), i)))
        })
        .collect();
    let (Some((_, first)), Some((_, second))) = (ends[0], ends[1]) else { return Vec::new() };
    let flat = |v: Vec3| Vec3::new(v.x, v.y, 0.0);
    let dir = flat(second - first).try_normalize().filter(|_| flat(second - first).length() > 0.05).unwrap_or(across);
    ends.iter()
        .enumerate()
        .map(|(k, e)| {
            let Some((i, at)) = *e else { return Vec3::ZERO };
            let along = (at - first).dot(dir);
            let more = (k as f32 * CARD_SPREAD - along).max(0.0);
            parent_chain(rig, i).inverse().transform_vector3(dir * more)
        })
        .collect()
}

/// A seat's chips: their root and the count drawn.
struct Chips {
    root: Entity,
    count: Option<u32>,
}

#[derive(Default)]
struct Drawn {
    entities: Vec<Entity>,
    models: Vec<CardModel>,
    /// The table's root.
    holder: Option<Entity>,
    /// The table's space (its root) and where the player's first card and the dealer's
    /// land, in it.
    chips: Option<Chips>,
    /// The table's arrows at the first and the split hand.
    arrows: [Option<Entity>; 2],
    player_spot: Vec3,
    dealer_spot: Vec3,
    /// Across the table, left to right as the player sees it (the table's space).
    across: Vec3,
    materials: HashMap<(Entity, Card), Handle<NifMaterial>>,
    warned: bool,
    /// The table stands where the world's does (`nvf_play::blackjack::Session::table_ref`):
    /// the game's own camera looks from here (Bevy space) [design].
    camera_at: Option<Transform>,
    /// The world's own drawing of that table, hidden meanwhile (entity, visibility before).
    hidden: Vec<(Entity, Visibility)>,
}

impl Drawn {
    fn in_world(&self) -> bool {
        self.camera_at.is_some()
    }

    fn layer(&self) -> RenderLayers {
        RenderLayers::layer(if self.in_world() { 0 } else { TABLE_LAYER })
    }

    /// The table's game space into its root's: Bevy's when drawn alone, none under the
    /// world root.
    fn to_root(&self) -> Mat4 {
        if self.in_world() {
            Mat4::IDENTITY
        } else {
            Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2)
        }
    }
}

/// What the table in the world needs: the simulation and the plugins, the world root, the
/// references' render entities, the camera's place.
type InWorld<'w, 's> = (
    Option<Res<'w, nvf_play::sim::SimState>>,
    Option<Res<'w, nvf_play::PlayWorld>>,
    Query<'w, 's, Entity, With<crate::world::WorldRoot>>,
    Query<'w, 's, (Entity, &'static nvf_play::RefVisual)>,
    ResMut<'w, crate::roulette_view::TableCamera>,
);

/// The world's table the menu's stands on: the world root, the reference's place (game
/// space), its model's bounds, the player in its model space, and what to hide.
struct WorldTable {
    root: Entity,
    place: Affine3A,
    bounds: (Vec3, Vec3),
    player: Vec3,
    twins: Vec<fnv_sim::FormId>,
}

fn node_index(rig: &Rig, name: &str) -> Option<usize> {
    rig.nodes.iter().position(|n| n.name.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(name)))
}

/// A node's local transform at `t` of `clip`, else its stored one.
fn sampled(rig: &Rig, clip: Option<&Clip>, i: usize, t: f32) -> Transform {
    let mut tr = rig.nodes[i].local;
    let Some(name) = rig.nodes[i].name.as_deref() else { return tr };
    let Some((_, track)) = clip.and_then(|c| c.tracks.iter().find(|(n, _)| n.eq_ignore_ascii_case(name))) else { return tr };
    if let Some(v) = track.translation.sample(t) {
        tr.translation = Vec3::from_array(v);
    }
    if let Some([w, x, y, z]) = track.rotation_at(t) {
        tr.rotation = Quat::from_xyzw(x, y, z, w).normalize();
    }
    if let Some([s]) = track.scale.sample(t) {
        tr.scale = Vec3::splat(s);
    }
    tr
}

/// Time `t` seconds into a clip, played once and held at its end.
fn clip_time(clip: Option<&Clip>, t: f32) -> f32 {
    clip.map_or(0.0, |c| (c.start + t.max(0.0) * c.frequency).min(c.stop))
}

/// A node's parents' stored transforms (model space, the root left out).
fn parent_chain(rig: &Rig, i: usize) -> Mat4 {
    let mut chain = Vec::new();
    let mut n = i;
    while let Some(p) = rig.nodes[n].parent {
        if p == 0 {
            break;
        }
        chain.push(p);
        n = p;
    }
    chain.iter().rev().fold(Mat4::IDENTITY, |m, &p| m * Mat4::from(rig.nodes[p].local.compute_affine()))
}

/// Where a node ends up (model space) at the end of a clip.
fn end_of(rig: &Rig, clip: Option<&Clip>, i: usize) -> Vec3 {
    let t = clip.map_or(0.0, |c| c.stop);
    let mut chain = vec![i];
    while let Some(p) = rig.nodes[*chain.last().unwrap()].parent {
        if p == 0 {
            break;
        }
        chain.push(p);
    }
    let mut m = Affine3A::IDENTITY;
    for &n in chain.iter().rev() {
        m *= sampled(rig, clip, n, t).compute_affine();
    }
    m.translation.into()
}

/// A card on the table: seconds since it was dealt, and since it started to be swept off.
type Place = (f32, Option<f32>);

fn model_of(seat: Seat) -> usize {
    match seat {
        Seat::Dealer => 0,
        Seat::Hand(h) => 1 + h.min(1),
    }
}

#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    screen: Res<BlackjackScreen>,
    view: Option<Res<BlackjackView>>,
    mut models: ModelCtx,
    window: Query<&Window, With<PrimaryWindow>>,
    time: AidTime,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    mut mats: Query<&mut MeshMaterial3d<NifMaterial>>,
    mut drawn: Local<Drawn>,
    (sim, play, world_root, refs, mut table_camera): InWorld,
) {
    let Some(s) = screen.0.as_ref() else {
        if !drawn.entities.is_empty() || view.is_some() {
            for e in drawn.entities.drain(..) {
                commands.entity(e).despawn();
            }
            crate::roulette_view::unhide(&mut drawn.hidden, &mut visibility);
            if drawn.in_world() {
                table_camera.set_if_neq(crate::roulette_view::TableCamera(None));
            }
            *drawn = Drawn { warned: drawn.warned, ..default() };
            commands.remove_resource::<BlackjackView>();
        }
        return;
    };
    if drawn.entities.is_empty() {
        // At the table in the world: fitted onto it, as roulette's (`roulette_view::fit`).
        let world = s.table_ref.zip(sim.as_deref()).zip(play.as_deref()).zip(world_root.single().ok()).and_then(|(((t, sim), play), root)| {
            let place = crate::roulette_view::ref_place(sim, t)?;
            let base = sim.world.peek(t).map(|r| r.base).or_else(|| sim.base_of(t.0))?;
            let path = nvf_play::dropped::model_path(&play.0, base, 0)?;
            let bounds = models.model(&format!("meshes\\{}", path.trim_start_matches("meshes\\").trim_start_matches("Meshes\\")))?.bounds?;
            let player = sim.world.peek(fnv_sim::FormId::PLAYER_REF).map(|p| Vec3::from(p.pos))?;
            let staff = crate::roulette_view::in_the_way(sim, play, &mut models, t);
            info!("blackjack view: at {t} ({path}); {} things at the table hidden: {staff:?}", staff.len());
            let twins = nvf_play::roulette::table_twins(sim, Some(play), t, "blackjack").into_iter().chain(staff).collect();
            Some(WorldTable { root, place, bounds, player: place.inverse().transform_point3(player), twins })
        });
        if !build(&mut commands, s, &mut models, &window, &mut drawn, world.as_ref()) {
            return;
        }
        if let Some(w) = &world {
            for r in &w.twins {
                let more = crate::roulette_view::hide_ref(r.0, &refs, &mut visibility);
                drawn.hidden.extend(more);
            }
        }
    }
    if let Some(at) = drawn.camera_at {
        table_camera.set_if_neq(crate::roulette_view::TableCamera(Some((at, FOV_DEG))));
    }
    let now = time.elapsed_secs();
    pose(&drawn.models, &s.shown, s.discard.as_ref(), s.reveal, now, model_of, &mut transforms, &mut visibility);
    // The arrow at the hand being played, after a split.
    let split = s.table.hands.len() == 2 && s.table.phase == fnv_game::blackjack::Phase::Playing;
    for (i, a) in drawn.arrows.iter().enumerate() {
        if let Some(mut v) = a.and_then(|e| visibility.get_mut(e).ok()) {
            v.set_if_neq(if split && s.table.active == i { Visibility::Inherited } else { Visibility::Hidden });
        }
    }
    let (layer, to_root) = (drawn.layer(), drawn.to_root());
    let Drawn { models: main, materials, chips: own_chips, player_spot, dealer_spot, .. } = &mut *drawn;
    faces(s, main, &s.shown, s.discard.as_ref(), model_of, materials, &mut models, &mut mats);
    if let Some(c) = own_chips {
        chips(&mut commands, c, s.chips_shown(now), *player_spot, *dealer_spot, &mut models, (&layer, to_root));
    }
}

/// Spawn the table, the card models, the camera and the lights.
fn build(commands: &mut Commands, s: &Session, models: &mut ModelCtx, window: &Query<&Window, With<PrimaryWindow>>, drawn: &mut Drawn, world: Option<&WorldTable>) -> bool {
    let table_path = s.def.table_model.as_deref().map(|p| format!("meshes\\{p}")).unwrap_or_else(|| "meshes\\terminals\\nv_blackjack\\nv_blackjack-table.nif".into());
    let paths = [table_path.as_str(), DEALER_MODEL, HAND_MODELS[0], HAND_MODELS[1]];
    let loaded: Vec<_> = paths.iter().map(|p| models.model(p)).collect();
    let table_bounds = loaded[0].as_ref().and_then(|m| m.bounds);
    let rigs: Option<Vec<Rig>> = loaded.iter().map(|m| m.as_ref().and_then(|m| m.rig.clone())).collect();
    let Some(mut rigs) = rigs else {
        if !drawn.warned {
            warn!("blackjack view: a model of {paths:?} is missing or has no sequences; the menu is drawn without the table");
            drawn.warned = true;
        }
        return false;
    };
    // Where the first cards land, and the camera over them (the table's space).
    let first_of = |rig: &Rig, name: &str| node_index(rig, name).map(|i| end_of(rig, rig.group(name), i));
    let (Some(player_spot), Some(dealer_spot)) = (first_of(&rigs[2], "Hand1_01"), first_of(&rigs[1], "Dealer_01")) else {
        warn!("blackjack view: the card models lack Hand1_01 or Dealer_01");
        drawn.warned = true;
        return false;
    };
    // From the dealer towards the player, flat on the felt.
    let towards_player = Vec3::new(player_spot.x - dealer_spot.x, player_spot.y - dealer_spot.y, 0.0).normalize_or(Vec3::NEG_X);
    let across = towards_player.cross(Vec3::Z);
    let aim = player_spot.lerp(dealer_spot, AIM_TOWARDS_DEALER);
    let eye = aim + towards_player * CAMERA_BACK + Vec3::Z * CAMERA_UP;
    // At the table in the world: the menu's table fitted onto the world's, under the world
    // root, seen by the game's own camera; else alone, far off, by a camera of its own.
    let fitted = world.zip(table_bounds).map(|(w, tb)| {
        let f = crate::roulette_view::fit(tb, w.bounds, eye, w.player);
        info!("blackjack view: the menu's table fitted onto the world's: scale {:.3}, turned {:.0}°", f.matrix3.x_axis.length(), f.matrix3.x_axis.y.atan2(f.matrix3.x_axis.x).to_degrees());
        (w.root, w.place * f)
    });
    if let Some((_, a)) = fitted {
        let to = |p: Vec3| nvf_play::space::to_bevy(a.transform_point3(p).to_array());
        drawn.camera_at = Some(Transform::from_translation(to(eye)).looking_at(to(aim), Vec3::Y));
    }
    let layer = drawn.layer();
    let (holder, to_bevy) = match fitted {
        Some((root, a)) => (commands.spawn((Name::new("Blackjack table (at the world's)"), Transform::from_matrix(Mat4::from(a)), Visibility::Inherited, ChildOf(root))).id(), Transform::IDENTITY),
        // Game space (Z up) into Bevy's (Y up), as the world root does.
        None => (
            commands.spawn((Name::new("Blackjack table"), Transform::from_translation(PLACE), Visibility::Inherited)).id(),
            Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        ),
    };
    let hand2 = rigs.pop().unwrap();
    let hand1 = rigs.pop().unwrap();
    let dealer = rigs.pop().unwrap();
    let table = rigs.pop().unwrap();
    let (_, table_drawn, _) = table.spawn_nodes(commands, holder, to_bevy, &table_path, 0.0);
    let mut arrows = [None, None];
    for (sh, e) in table.shapes.iter().zip(&table_drawn) {
        match table.nodes[sh.node].name.as_deref().map(str::to_ascii_lowercase).as_deref() {
            Some("hand1_arrow:0") => arrows[0] = Some(*e),
            Some("hand1_arrow:0@#2") => arrows[1] = Some(*e),
            _ => continue,
        }
        commands.entity(*e).insert(Visibility::Hidden);
    }
    let mut shapes_total = table_drawn.len();
    for e in &table_drawn {
        commands.entity(*e).insert(layer.clone());
    }
    let mut card_models = Vec::new();
    for (rig, prefix, path) in [(dealer, "Dealer", DEALER_MODEL), (hand1, "Hand1", HAND_MODELS[0]), (hand2, "Hand2", HAND_MODELS[1])] {
        let (_, shapes, by_name) = rig.spawn_nodes(commands, holder, to_bevy, path, 0.0);
        shapes_total += shapes.len();
        for e in &shapes {
            commands.entity(*e).insert(layer.clone());
        }
        match CardModel::new(rig, prefix, &by_name, &shapes, across) {
            Some(m) => {
                for c in &m.cards {
                    commands.entity(c.entity).insert(Visibility::Hidden);
                }
                card_models.push(m);
            }
            None => {
                warn!("blackjack view: {path} lacks the nodes {prefix}_01 to _07");
                commands.entity(holder).despawn();
                drawn.warned = true;
                return false;
            }
        }
    }
    let chips_root = commands.spawn((Transform::default(), Visibility::Inherited)).id();
    commands.entity(holder).add_child(chips_root);
    drawn.models = card_models;
    drawn.holder = Some(holder);
    drawn.player_spot = player_spot;
    drawn.dealer_spot = dealer_spot;
    drawn.across = across;
    drawn.chips = Some(Chips { root: chips_root, count: None });
    drawn.arrows = arrows;
    if let Some((_, a)) = fitted {
        // The world's light, and a lamp over the felt (as roulette's).
        let centre = player_spot.lerp(dealer_spot, 0.5);
        let lamp = crate::roulette_view::table_lamp(commands, nvf_play::space::to_bevy(a.transform_point3(centre).to_array()));
        drawn.entities = vec![holder, lamp];
        commands.insert_resource(BlackjackView { image: None });
        info!("blackjack view: the table built at the world's for {} ({table_path}, {shapes_total} shapes)", s.def.name);
        return true;
    }
    let size = window.single().map(|w| Extent3d { width: w.physical_width().max(1), height: w.physical_height().max(1), depth_or_array_layers: 1 }).unwrap_or(Extent3d { width: 1280, height: 720, depth_or_array_layers: 1 });
    let mut image = Image::new_fill(size, TextureDimension::D2, &[0, 0, 0, 0], TextureFormat::Bgra8UnormSrgb, RenderAssetUsages::default());
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image = models.images.add(image);
    let to = |p: Vec3| PLACE + nvf_play::space::to_bevy(p.to_array());
    let camera = commands
        .spawn((
            Name::new("Blackjack camera"),
            Camera3d::default(),
            Camera { order: -3, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
            RenderTarget::Image(ImageRenderTarget::from(image.clone())),
            Projection::Perspective(PerspectiveProjection { fov: FOV_DEG.to_radians(), near: 1.0, far: 3000.0, ..default() }),
            Tonemapping::Linear,
            Msaa::Sample4,
            AmbientLight { color: Color::WHITE, brightness: 120.0, ..default() },
            Transform::from_translation(to(eye)).looking_at(to(aim), Vec3::Y),
            layer.clone(),
        ))
        .id();
    // A warm light from above the dealer's side and a dim fill from the player's
    // [design: the casino's lamps are not in the files; the screenshot is dim and warm].
    let centre = player_spot.lerp(dealer_spot, 0.5);
    let key = commands
        .spawn((
            DirectionalLight { illuminance: 2200.0, color: Color::srgb(1.0, 0.88, 0.72), ..default() },
            Transform::from_translation(to(centre + Vec3::new(20.0, 10.0, 80.0))).looking_at(to(centre), Vec3::Y),
            layer.clone(),
        ))
        .id();
    let fill = commands
        .spawn((
            DirectionalLight { illuminance: 500.0, ..default() },
            Transform::from_translation(to(eye + Vec3::Z * 30.0)).looking_at(to(centre), Vec3::Y),
            layer.clone(),
        ))
        .id();
    drawn.entities = vec![holder, camera, key, fill];
    commands.insert_resource(BlackjackView { image: Some(image) });
    info!("blackjack view: the table built for {} ({table_path}, {shapes_total} shapes); first cards land at {player_spot:?} (player) and {dealer_spot:?} (dealer)", s.def.name);
    true
}

/// Each card on the table posed by its sequence; the others hidden. `slot` says which of
/// `models` a seat's cards are (beyond them: not drawn here).
#[allow(clippy::too_many_arguments)]
fn pose(
    models: &[CardModel],
    shown: &[Shown],
    discard: Option<&(f32, Vec<Shown>)>,
    reveal: Option<f32>,
    now: f32,
    slot: fn(Seat) -> usize,
    transforms: &mut Query<&mut Transform>,
    visibility: &mut Query<&mut Visibility>,
) {
    let mut on_table: [[Option<Place>; 7]; 3] = [[None; 7]; 3];
    // The last round's cards: their discard sequence.
    if let Some((since, cards)) = discard {
        for c in cards {
            if let Some(m) = on_table.get_mut(slot(c.seat)).filter(|_| c.index < 7) {
                m[c.index] = Some((-1.0, Some(now - since)));
            }
        }
    }
    for c in shown {
        if let Some(m) = on_table.get_mut(slot(c.seat)).filter(|_| c.index < 7 && now >= c.since) {
            m[c.index] = Some((now - c.since, None));
        }
    }
    for (m, slots) in models.iter().zip(&on_table) {
        for (k, (card, at)) in m.cards.iter().zip(slots).enumerate() {
            let shown = at.is_some();
            if let Ok(mut v) = visibility.get_mut(card.entity) {
                v.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
            }
            let Some((dealt, discard)) = *at else { continue };
            // How much of the hand's fan the card has: growing as it is dealt, going as
            // it is swept off.
            let progress = |clip: Option<&Clip>, t: f32| clip.map_or(1.0, |c| ((clip_time(clip, t) - c.start) / (c.stop - c.start).max(1e-3)).clamp(0.0, 1.0));
            let tr = match discard {
                Some(t) => {
                    let clip = m.clip(&format!("{}_Discard", m.prefix));
                    let mut tr = sampled(&m.rig, clip, card.node, clip_time(clip, t));
                    tr.translation += card.spread * (1.0 - progress(clip, t));
                    tr
                }
                None if m.prefix == "Dealer" && k == 0 && reveal.is_some_and(|r| now >= r) => {
                    // The flip played in REVEAL_SECONDS whatever the sequence's own length
                    //, so the pause after it is the logic's.
                    let clip = m.clip("Dealer_Reveal");
                    let own = clip.map_or(1.0, |c| (c.stop - c.start) / c.frequency.max(1e-3));
                    let t = (now - reveal.unwrap_or(now)) * own / nvf_play::blackjack::REVEAL_SECONDS;
                    let mut tr = sampled(&m.rig, clip, card.node, clip_time(clip, t));
                    tr.translation += card.spread;
                    tr
                }
                None => {
                    let clip = m.clip(&format!("{}_{:02}", m.prefix, k + 1));
                    let mut tr = sampled(&m.rig, clip, card.node, clip_time(clip, dealt));
                    tr.translation += card.spread * progress(clip, dealt);
                    tr
                }
            };
            if let Ok(mut t) = transforms.get_mut(card.entity) {
                *t = tr;
            }
        }
    }
}

/// Which of its model's cards shows a card's face: the dealer's hole card (the rules'
/// second) on the model's face-down first card, and its first on the second.
pub fn dealer_face_slot(seat: Seat, index: usize) -> usize {
    match (seat, index) {
        (Seat::Dealer, 0) => 1,
        (Seat::Dealer, 1) => 0,
        _ => index,
    }
}

/// Each card's face picture: the card it shows now.
#[allow(clippy::too_many_arguments)]
fn faces(
    s: &Session,
    card_models: &mut [CardModel],
    shown: &[Shown],
    discard: Option<&(f32, Vec<Shown>)>,
    slot: fn(Seat) -> usize,
    materials: &mut HashMap<(Entity, Card), Handle<NifMaterial>>,
    models: &mut ModelCtx,
    mats: &mut Query<&mut MeshMaterial3d<NifMaterial>>,
) {
    let mut want: [[Option<Card>; 7]; 3] = [[None; 7]; 3];
    for c in discard.map(|d| d.1.as_slice()).unwrap_or_default().iter().chain(shown) {
        if let Some(m) = want.get_mut(slot(c.seat)).filter(|_| c.index < 7) {
            m[dealer_face_slot(c.seat, c.index)] = Some(c.card);
        }
    }
    for (m, want) in card_models.iter_mut().zip(want) {
        for (slot, card) in m.cards.iter_mut().zip(want) {
            let Some(card) = card else { continue };
            if slot.showing == Some(card) {
                continue;
            }
            slot.showing = Some(card);
            let Some(path) = s.def.face_texture(card).map(|p| format!("textures\\{p}")) else { continue };
            for &e in &slot.faces {
                let Ok(mut mm) = mats.get_mut(e) else { continue };
                let handle = match materials.get(&(e, card)) {
                    Some(h) => h.clone(),
                    None => {
                        let Some(tex) = models.textures.get(&path, true, &models.data, &mut models.images, models.formats.as_deref()) else {
                            warn!("blackjack view: no card picture {path}");
                            continue;
                        };
                        let Some(mut mat) = models.materials.get(&mm.0).cloned() else { continue };
                        mat.base.base_color_texture = Some(tex);
                        // The copy is of the material worn now, which may be another card's:
                        // the new card's set, or none, is taken from the legacy look (#2981).
                        nvf_assets::authored::retexture(&mut mat, &path, &mut models.textures, &models.data, &mut models.images, models.formats.as_deref());
                        let h = models.materials.add(mat);
                        materials.insert((e, card), h.clone());
                        h
                    }
                };
                mm.0 = handle;
            }
        }
    }
}

/// A seat's chips on the table, in stacks by value in front of its cards.
fn chips(commands: &mut Commands, c: &mut Chips, staked: u32, player_spot: Vec3, dealer_spot: Vec3, models: &mut ModelCtx, (layer, to_root): (&RenderLayers, Mat4)) {
    if c.count == Some(staked) {
        return;
    }
    c.count = Some(staked);
    let root = c.root;
    commands.entity(root).despawn_related::<Children>();
    let towards_player = Vec3::new(player_spot.x - dealer_spot.x, player_spot.y - dealer_spot.y, 0.0).normalize_or(Vec3::NEG_X);
    // Stacks side by side across the player's view, away from the cards.
    let side = Vec3::Z.cross(towards_player);
    let mut left = staked;
    let mut stack = 0.0;
    for (value, path) in CHIPS {
        let mut n = left / value;
        left -= n * value;
        let Some(model) = models.model(path) else { continue };
        while n > 0 {
            let here = n.min(STACK_MAX);
            n -= here;
            for i in 0..here {
                let Some((lo, hi)) = model.bounds else { continue };
                let felt = player_spot.z - FELT_BELOW_CARDS;
                let p = Vec3::new(player_spot.x, player_spot.y, felt) + towards_player * BET_FORWARD + side * (stack - BET_LEFT) + Vec3::Z * (i as f32 * CHIP_THICK);
                let centre = Vec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, lo.z);
                let squeeze = CHIP_THICK / (hi.z - lo.z).max(0.01);
                // In the game's space: the model's foot centre to the origin, squeezed, then
                // to its place; into Bevy's space as the table's root does.
                let game = Mat4::from_translation(p) * Mat4::from_scale(Vec3::new(1.0, 1.0, squeeze)) * Mat4::from_translation(-centre);
                let tr = Transform::from_matrix(to_root * game);
                let chip = commands.spawn((tr, Visibility::Inherited)).id();
                commands.entity(root).add_child(chip);
                for sh in &model.shapes {
                    let shape = commands.spawn((Transform::from_matrix(Mat4::from(sh.transform)), Mesh3d(sh.mesh.clone()), MeshMaterial3d(sh.material.clone()), layer.clone())).id();
                    commands.entity(chip).add_child(shape);
                }
            }
            stack += CHIP_STACK_GAP;
        }
    }
}
