//! Slot machines in 3D: the reels turn on the placed machine in
//! the world, not on a flat screen nor on a second model in a view. Every machine in use
//! (`nvf_play::slots::machines::SlotMachines`) is drawn so, so a passer-by sees the same
//! spin and result from any angle.
//!
//! - **The machine's own reels.** The placed models `clutter\casino\Slot_Machine01*.nif`
//!   have three reel nodes `Reel01`..`Reel03`, all at (-0.33, 2.12, 33.69) in the body
//!   node, each holding one drum shape (`Reel01:5`): 8 sides, radius about 8.19, 3.79
//!   wide, mapped onto a small strip of the cabinet's texture (`Slot_Machine01_D.dds`, 512
//!   square) [observed: fnvtool nif decode, slots-170-findings.md §5]. The window in front
//!   of them faces 15° above the machine's front (-Y) [observed: the window frame's
//!   vertices and normals, (0, -0.97, 0.25)], so the payline lies 15° above -Y round the
//!   reels' axis. While the machine is in use its drum shapes are hidden and a drum of
//!   NVForever's own turns in each one's place, of the same size (from the shape's own
//!   vertices, so a mod's variant fits too): the casino's 14 stops, each the symbol's own
//!   picture (CSNO ICON, 256 × 128: sharp up close), with the separator picture
//!   (`NV_SlotMachine-SymbolS.dds`) between them, as the original's reel models lay them
//!   out (`nv_slotmachine-reel01.nif`: symbol, separator, symbol … [observed]). Each face
//!   wears a copy of the drum shape's own material with the picture in place of the
//!   cabinet's, so it is lit as the rest of the machine [design].
//! - **The original's reel models.** The drums are laid out as `nv_slotmachine-reel0n.nif`
//!   lays its own: 14 shapes `Cylinder06:0`..`:13`, each two opposite faces of one picture,
//!   28 faces of equal height round the drum, symbol and separator in turn, each face the
//!   whole picture [observed: fnvtool nif decode; radius about 7.25, 3.79 wide]. So the
//!   casino's 14 stops are the 14 symbol faces, a separator between each two. Each face
//!   wears a copy of the reel model's own material (its lighting shader, its normal map
//!   `shadefade01LOD_n.dds`), the symbol face with the stop's picture from the casino
//!   record; the separator faces keep the model's own. The drums keep the placed
//!   machine's size (its window), not the reel model's [design]. Without the reel model
//!   (a load order that lacks it) the faces are NVForever's own proportions.
//! - **Without reel nodes.** `Slot_Machine02.nif` (four of the Atomic Wrangler's) is one
//!   static shape of the same cabinet [observed: its window frame lies where 01's does]:
//!   the drums are put where 01's reels stand, a hair larger than the drawn ones. A model
//!   of another size gets no drums; the menu draws its flat reels then (the fallback).
//! - **The payline**: a thin red line across each reel and a red arrow at each end, on
//!   the line the symbols stop on [design, "a line or arrows"].
//! - **The arm** (`Handle`) is pulled on a spin: the minigame model's `Forward` turns it
//!   0.774 rad by 0.33 s and 0.861 rad by 0.5 s [observed: NV_SlotMachine-Minigame.NIF],
//!   held to 0.83 s, then let back over 0.5 s [design]. The placed models have no
//!   `Forward`, coins `Backward` nor `Payout_*` sequences (only `Idle` and a still
//!   `Backward`/`Off` pose) [observed], so those minigame clips have nothing to play on.
//! - **The camera** ([design], revised 2026-11-11: "third person ... only on wins",
//!   the moves less fast): a slow pan from the player's view in close on the reels, where
//!   it stays for the pull, the turn and the stop; only a win pulls it out to third person
//!   of the player for the celebration (`fnv_game::slots::celebration`), then back in to
//!   the reels. A loss leaves it on the reels. Every move eases in and out (smootherstep)
//!   over 1.2 to 1.6 s. The game's own camera is moved
//!   (`roulette_view::TableCamera`), and the player's body is drawn meanwhile
//!   (`PlayerControl::scene_cam`).
//!
//! Log: `slots view: drums on <ref> (<how>)`, `slots view: <ref> has no reels to drive`.

use crate::roulette_view::TableCamera;
use bevy::asset::RenderAssetUsages;
use bevy::math::Affine3A;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use fnv_game::slots::REELS;
use fnv_sim::FormId;
use nvf_assets::model::ModelCtx;
use nvf_assets::nif_material::NifMaterial;
use nvf_play::pause::AidTime;
use nvf_play::slots::machines::{MachineSpin, SlotMachines};
use nvf_play::slots::{SlotsReels3d, SlotsScreen, SlotsSpin, SpinPhase};
use std::collections::HashMap;

/// The payline's angle above the machine's front, round the reels' axis [observed: the
/// window frame of Slot_Machine01*.nif faces (0, -0.97, 0.25)].
const PAYLINE_DEG: f32 = 15.0;
/// The vanilla cabinet (model space, Slot_Machine01*'s and 02's): the reels' axis, each
/// drum's centre along it, radius and width [observed: the `Reel0n:5` shapes].
const AXIS_YZ: Vec2 = Vec2::new(2.122, 33.688);
const DRUM_X: [f32; REELS] = [-8.054, -2.451, 3.152];
const DRUM_R: f32 = 8.19;
const DRUM_W: f32 = 3.79;
/// The vanilla cabinet's size, for a model without reel nodes to be taken for it.
const CABINET: Vec3 = Vec3::new(40.6, 33.8, 60.0);
/// How much of a symbol picture's width shows on its face, and of the separator's height
/// [design: the pictures' plain margins; keeps their proportions on the drum].
const SYMBOL_U: (f32, f32) = (0.1, 0.9);
const SEPARATOR_V: (f32, f32) = (0.25, 0.75);
/// Segments of a symbol face and of a separator round the drum [design].
const SYMBOL_SEGMENTS: usize = 4;
const SEPARATOR_SEGMENTS: usize = 2;
const SEPARATOR: &str = "textures\\terminals\\nv_slotmachine\\NV_SlotMachine-SymbolS.dds";
/// The arm's pull [observed: the minigame model's `Forward` keys] and its return [design].
const PULL: [(f32, f32); 4] = [(0.0, 0.0), (0.33, 0.774), (0.5, 0.861), (0.83, 0.861)];
const PULL_BACK: f32 = 0.5;

/// The camera (model space of the machine): the reel view's distance in front of the
/// payline and field of view; the third-person shot's eye and aim [design: fitted by eye].
const REEL_VIEW_BACK: f32 = 30.0;
const REEL_FOV: f32 = 40.0;
const THIRD_EYE: Vec3 = Vec3::new(55.0, -205.0, 95.0);
const THIRD_AIM: Vec3 = Vec3::new(2.0, -60.0, 42.0);
const THIRD_FOV: f32 = 55.0;
/// The timings of the camera's moves, seconds [design: the first ones
/// (0.25 to 0.8 s) were "a little fast/jarring"]: the pan in from the player's
/// view, the zoom out for a win and the way back in.
const OPEN_PAN: f32 = 1.6;
/// The win's two moves are the game's (`nvf_play::slots::win_hold` holds the next spin
/// until they are over).
const ZOOM_OUT: f32 = nvf_play::slots::WIN_CAMERA_OUT;
const BACK_IN: f32 = nvf_play::slots::WIN_CAMERA_BACK;

pub struct SlotsViewPlugin;

impl Plugin for SlotsViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (drums, camera).chain().after(crate::advance_animations).after(nvf_play::PlaySet::Ui).before(nvf_play::PlaySet::Sync))
            .add_systems(Last, report);
    }
}

/// A machine's drums as drawn.
struct Drawn {
    /// Despawned when the machine is no longer in use.
    holder: Entity,
    drums: [Entity; REELS],
    /// The world's own reel shapes, hidden meanwhile (entity, visibility before).
    hidden: Vec<(Entity, Visibility)>,
    /// The arm: its entity and stored pose.
    arm: Option<(Entity, Transform)>,
    /// The spin last seen (its number), for the arm.
    spin: u32,
    pulled_at: Option<f32>,
}

/// What the original's reel model (`nv_slotmachine-reel0n.nif`) gives the drums: how many
/// faces it has round the drum (symbols and separators), and the material of a symbol face
/// and of a separator face [observed: its shapes, symbol first, then in turn].
struct ReelModel {
    faces: usize,
    radius: f32,
    width: f32,
    symbol: Handle<NifMaterial>,
    separator: Handle<NifMaterial>,
}

const REEL_MODEL: &str = "meshes\\terminals\\nv_slotmachine\\nv_slotmachine-reel01.nif";

/// Read the original's reel model: its faces are the quads of its shapes (four vertices
/// each), its axis the direction it is narrowest along, its radius the farthest vertex
/// from that axis.
fn reel_model(models: &mut ModelCtx) -> Option<ReelModel> {
    let model = models.model_with_cpu(REEL_MODEL)?;
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut points = Vec::new();
    let mut faces = 0;
    for shape in &model.shapes {
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = shape.cpu.as_ref().and_then(|m| m.attribute(Mesh::ATTRIBUTE_POSITION)) else { continue };
        faces += p.len() / 4;
        for v in p {
            let v = shape.transform.transform_point3(Vec3::from(*v));
            lo = lo.min(v);
            hi = hi.max(v);
            points.push(v);
        }
    }
    let size = hi - lo;
    let axis = if size.x <= size.y && size.x <= size.z { 0 } else if size.y <= size.z { 1 } else { 2 };
    let mid = (lo + hi) / 2.0;
    let radius = points
        .iter()
        .map(|v| {
            let mut d = *v - mid;
            d[axis] = 0.0;
            d.length()
        })
        .fold(0.0f32, f32::max);
    let (a, b) = (model.shapes.first()?, model.shapes.get(1)?);
    (faces >= 4 && radius > 0.5).then(|| ReelModel { faces, radius, width: size[axis], symbol: a.material.clone(), separator: b.material.clone() })
}

/// Not drawable, warned once.
#[derive(Default)]
struct Views {
    drawn: HashMap<FormId, Drawn>,
    failed: HashMap<FormId, ()>,
    /// The original's reel model, read on first use (`None` inside: not there).
    reel: Option<Option<ReelModel>>,
}

/// Machines given drums this run, for the summary.
static BUILT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// How a machine's drums are placed: under which entity, and each drum's centre (that
/// entity's space), radius and width.
struct Fit {
    parent: Entity,
    centres: [Vec3; REELS],
    radius: f32,
    width: f32,
    hide: Vec<Entity>,
    arm: Option<(Entity, Transform)>,
    how: &'static str,
}

fn payline_dir() -> Vec3 {
    let b = PAYLINE_DEG.to_radians();
    Vec3::new(0.0, -b.cos(), b.sin())
}

/// A direction `a` radians round the axis from the payline (positive: down the window).
fn round(a: f32) -> Vec3 {
    Quat::from_rotation_x(a) * payline_dir()
}

/// One face of a drum, centred on the payline: `angle` radians tall, `u`/`v` the part of
/// its picture shown; its outside faces out.
fn face_mesh(radius: f32, width: f32, angle: f32, segments: usize, u: (f32, f32), v: (f32, f32)) -> Mesh {
    let mut pos = Vec::new();
    let mut nor = Vec::new();
    let mut uv = Vec::new();
    for j in 0..=segments {
        let f = j as f32 / segments as f32;
        let d = round(-angle / 2.0 + f * angle);
        for (x, uu) in [(-width / 2.0, u.0), (width / 2.0, u.1)] {
            pos.push([x, d.y * radius, d.z * radius]);
            nor.push([0.0, d.y, d.z]);
            uv.push([uu, v.0 + f * (v.1 - v.0)]);
        }
    }
    let mut idx: Vec<u32> = Vec::new();
    for j in 0..segments as u32 {
        let (l0, r0, l1, r1) = (2 * j, 2 * j + 1, 2 * j + 2, 2 * j + 3);
        idx.extend([l0, l1, r1, l0, r1, r0]);
    }
    let n = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nor)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32, 1.0, 1.0, 1.0]; n])
        .with_inserted_indices(Indices::U32(idx))
}

/// A flat shape from points in the plane facing the payline (x across, y up the window),
/// `lift` in front of the drum.
fn flat_mesh(points: &[[f32; 2]], tris: &[u32], radius: f32, lift: f32) -> Mesh {
    let n = payline_dir();
    let up = Vec3::new(0.0, n.z, -n.y);
    let pos: Vec<[f32; 3]> = points.iter().map(|p| (Vec3::X * p[0] + up * p[1] + n * (radius + lift)).to_array()).collect();
    let k = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![n.to_array(); k])
        .with_inserted_indices(Indices::U32(tris.to_vec()))
}

/// A drum mesh's centre and half sizes from its positions: half x along its axis, and its
/// radius (the farthest vertex from the axis through the centre) for y and z. The radius
/// is read about the axis, not from the box, so a polygonal drum turned about its axis
/// measures the same (an 8-sided drum's box is cos 15° narrower when it sits 15° turned).
/// `None` when the mesh keeps no positions in the main world (a GPU mesh already
/// extracted to the render world: reading it there panics).
fn mesh_bounds(m: &Mesh) -> Option<(Vec3, Vec3)> {
    let Ok(Some(bevy::mesh::VertexAttributeValues::Float32x3(p))) = m.try_attribute_option(Mesh::ATTRIBUTE_POSITION) else { return None };
    if p.is_empty() {
        return None;
    }
    let lo = p.iter().fold(Vec3::splat(f32::MAX), |a, v| a.min(Vec3::from(*v)));
    let hi = p.iter().fold(Vec3::splat(f32::MIN), |a, v| a.max(Vec3::from(*v)));
    let c = (lo + hi) / 2.0;
    let r = p.iter().map(|v| Vec2::new(v[1] - c.y, v[2] - c.z).length()).fold(0.0f32, f32::max);
    Some((c, Vec3::new((hi.x - lo.x) / 2.0, r, r)))
}

/// Smootherstep: eases in and out with no jolt at either end (its speed and acceleration
/// both start and end at 0), for the camera's moves.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (6.0 * t - 15.0) + 10.0)
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The arm's turn (radians) `t` seconds after the pull.
fn arm_angle(t: f32) -> f32 {
    let held = PULL[PULL.len() - 1];
    if t >= held.0 {
        return held.1 * (1.0 - smooth((t - held.0) / PULL_BACK));
    }
    for w in PULL.windows(2) {
        if t <= w[1].0 {
            let f = (t - w[0].0) / (w[1].0 - w[0].0);
            return w[0].1 + (w[1].1 - w[0].1) * smooth(f);
        }
    }
    0.0
}

/// The drum's turn for a reel standing at `pos` stops of `n`: stop `k`'s face is at the
/// payline when `pos` is `k`; the next stops lie above it and come down as it turns.
fn drum_turn(pos: f32, n: usize) -> Quat {
    Quat::from_rotation_x(pos * std::f32::consts::TAU / n.max(1) as f32)
}

/// Where a machine's drums go, from its drawing in the world.
// The drawing's parts come from the system's own queries and resources, passed through.
#[allow(clippy::too_many_arguments)]
fn fit_of(
    machine: FormId,
    rigs: &Query<(Entity, &crate::object_anims::GroupRig)>,
    refs: &Query<(Entity, &nvf_play::RefVisual)>,
    sim: &nvf_play::sim::SimState,
    play: &nvf_play::PlayWorld,
    models: &mut ModelCtx,
    root: Entity,
    commands: &mut Commands,
) -> Option<Fit> {
    // A rig with reel nodes: the drums take the reel shapes' places.
    if let Some((_, g)) = rigs.iter().find(|(_, g)| g.form_id == machine.0) {
        let model = g.model();
        if let Some(rig) = model.rig.as_ref() {
            let idx = |name: &str| rig.nodes.iter().position(|n| n.name.as_deref().is_some_and(|m| m.eq_ignore_ascii_case(name)));
            let mut centres = [Vec3::ZERO; REELS];
            let (mut radius, mut width) = (0.0f32, 0.0f32);
            let mut hide = Vec::new();
            let mut parent = None;
            let mut ok = true;
            for (i, c) in centres.iter_mut().enumerate() {
                let Some(r) = idx(&format!("Reel{:02}", i + 1)) else {
                    ok = false;
                    break;
                };
                // The reel's shape: on the reel node or a node under it.
                let shape = rig.shapes.iter().find(|s| s.node == r || rig.nodes[s.node].parent == Some(r));
                let Some(shape) = shape else {
                    ok = false;
                    break;
                };
                let local = if shape.node == r { Transform::IDENTITY } else { rig.nodes[shape.node].local };
                // The drum's size from its vertices; the vanilla drum's when the mesh keeps
                // none on the CPU.
                // Placed models' GPU meshes are render-world only once drawn, so the
                // bounds come from a CPU copy of the same file, by shape name.
                let bounds = models.meshes.get(&shape.mesh).and_then(mesh_bounds).or_else(|| {
                    let cpu = models.model_with_cpu(g.source())?;
                    let named = cpu.shapes.iter().find(|s| s.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(&shape.look.0)))?;
                    named.cpu.as_deref().and_then(mesh_bounds)
                });
                let (centre, half) = match bounds {
                    Some((c, h)) => (local.transform_point(c), h * local.scale),
                    None => (Vec3::new(DRUM_X[i] - DRUM_X[1], 0.0, 0.0), Vec3::new(DRUM_W / 2.0, DRUM_R, DRUM_R)),
                };
                radius = radius.max(half.y.max(half.z));
                width = width.max(2.0 * half.x);
                let reel = rig.nodes[r].local;
                *c = reel.translation + reel.rotation * Vec3::new(centre.x, 0.0, 0.0);
                if shape.node != r {
                    if let Some(e) = rig.nodes[shape.node].name.as_deref().and_then(|n| g.node(n)) {
                        hide.push(e);
                    }
                }
                if let Some(e) = rig.nodes[r].name.as_deref().and_then(|n| g.node(n)) {
                    if shape.node == r {
                        hide.push(e);
                    }
                }
                let p = rig.nodes[r].parent.and_then(|p| rig.nodes[p].name.as_deref()).and_then(|n| g.node(n));
                parent = parent.or(p);
            }
            if ok && radius > 0.5 && width > 0.5 {
                let arm = idx("Handle").and_then(|h| Some((g.node(rig.nodes[h].name.as_deref()?)?, rig.nodes[h].local)));
                return Some(Fit { parent: parent?, centres, radius, width, hide, arm, how: "its own reel nodes" });
            }
        }
    }
    // Otherwise a static machine of the vanilla cabinet: at its reels' places.
    if !refs.iter().any(|(_, v)| v.0 == machine.0) {
        return None;
    }
    let place = crate::roulette_view::ref_place(sim, machine)?;
    let base = sim.world.peek(machine).map(|r| r.base).or_else(|| sim.base_of(machine.0))?;
    let path = nvf_play::dropped::model_path(&play.0, base, 0)?;
    let (lo, hi) = models.model(&format!("meshes\\{}", path.trim_start_matches("meshes\\").trim_start_matches("Meshes\\")))?.bounds?;
    let size = hi - lo;
    if ((size / CABINET) - Vec3::ONE).abs().max_element() > 0.15 {
        return None;
    }
    let holder = commands.spawn((Name::new("Slot machine reels (static machine)"), Transform::from_matrix(Mat4::from(place)), Visibility::Inherited, ChildOf(root))).id();
    let centres = DRUM_X.map(|x| Vec3::new(x, AXIS_YZ.x, AXIS_YZ.y));
    Some(Fit { parent: holder, centres, radius: DRUM_R * 1.01, width: DRUM_W, hide: Vec::new(), arm: None, how: "the cabinet's reel places (static model)" })
}

/// Build a machine's drums and payline marks under `fit.parent`.
fn build(commands: &mut Commands, fit: &Fit, spin: &MachineSpin, models: &mut ModelCtx, base_material: Option<Handle<NifMaterial>>, reel: Option<&ReelModel>) -> (Entity, [Entity; REELS]) {
    let holder = commands.spawn((Name::new("Slot machine drums"), Transform::IDENTITY, Visibility::Inherited, ChildOf(fit.parent))).id();
    let n = spin.reel.len().max(1);
    let step = std::f32::consts::TAU / n as f32;
    // The reel model's layout when its faces are a symbol and a separator per stop: faces
    // of equal height, each the whole picture [observed]. Otherwise NVForever's own: the
    // symbol's face as tall as its picture is for the drum's width, the separator the
    // rest of the stop [design].
    let reel = reel.filter(|r| r.faces == 2 * n);
    let (symbol_mesh, sep_mesh) = if reel.is_some() {
        let face = |segments| face_mesh(fit.radius, fit.width, step / 2.0, segments, (0.0, 1.0), (0.0, 1.0));
        (face(SYMBOL_SEGMENTS), face(SEPARATOR_SEGMENTS))
    } else {
        let symbol_h = fit.width * 128.0 / ((SYMBOL_U.1 - SYMBOL_U.0) * 256.0);
        let symbol_a = (symbol_h / fit.radius).min(step * 0.8);
        let sep_a = step - symbol_a;
        let sep_u = {
            let h = sep_a * fit.radius;
            let shown = (fit.width / h) * ((SEPARATOR_V.1 - SEPARATOR_V.0) * 128.0) / 256.0;
            let m = ((1.0 - shown) / 2.0).clamp(0.0, 0.45);
            (m, 1.0 - m)
        };
        (face_mesh(fit.radius, fit.width, symbol_a, SYMBOL_SEGMENTS, SYMBOL_U, (0.0, 1.0)), face_mesh(fit.radius, fit.width, sep_a, SEPARATOR_SEGMENTS, sep_u, SEPARATOR_V))
    };
    // The reel model's material keeps its normal map, which wants tangents.
    let tangents = reel.is_some();
    let add = |models: &mut ModelCtx, m: Mesh| -> (Handle<Mesh>, bool) {
        if tangents {
            if let Ok(t) = m.clone().with_generated_tangents() {
                return (models.meshes.add(t), true);
            }
        }
        (models.meshes.add(m), false)
    };
    let (symbol_mesh, symbol_tangents) = add(models, symbol_mesh);
    let (sep_mesh, sep_tangents) = add(models, sep_mesh);
    let template = reel.map(|r| r.symbol.clone()).or(base_material).and_then(|h| models.materials.get(&h).cloned());
    let keep_maps = reel.is_some() && symbol_tangents;
    let mut material_of = |path: &str| -> Handle<NifMaterial> {
        let tex = models.textures.get(path, true, &models.data, &mut models.images, models.formats.as_deref());
        let mut mat = template.clone().unwrap_or_else(|| nvf_assets::nif_material::nif_material(StandardMaterial::default()));
        mat.base.base_color_texture = tex;
        if !keep_maps {
            mat.base.normal_map_texture = None;
            mat.base.specular_texture = None;
        }
        mat.base.emissive_texture = None;
        models.materials.add(mat)
    };
    let pictures: Vec<Handle<NifMaterial>> = (0..spin.symbols.len()).map(|i| material_of(&format!("textures\\{}", spin.symbols[i]))).collect();
    let separator = match reel {
        Some(r) if sep_tangents => r.separator.clone(),
        _ => material_of(SEPARATOR),
    };
    let drums = std::array::from_fn(|i| {
        let drum = commands.spawn((Name::new(format!("Slot reel {}", i + 1)), Transform::from_translation(fit.centres[i]), Visibility::Inherited, ChildOf(holder))).id();
        for (k, &sym) in spin.reel.iter().enumerate() {
            let a = -(k as f32) * step;
            if let Some(m) = pictures.get(sym as usize) {
                commands.spawn((Mesh3d(symbol_mesh.clone()), MeshMaterial3d(m.clone()), Transform::from_rotation(Quat::from_rotation_x(a)), Visibility::Inherited, ChildOf(drum)));
            }
            commands.spawn((Mesh3d(sep_mesh.clone()), MeshMaterial3d(separator.clone()), Transform::from_rotation(Quat::from_rotation_x(a - step / 2.0)), Visibility::Inherited, ChildOf(drum)));
        }
        drum
    });
    // The payline: a thin line across each reel and an arrow at each end [design].
    let red = Color::srgb(0.9, 0.05, 0.03);
    let mark = models.materials.add(nvf_assets::nif_material::nif_material(StandardMaterial { base_color: red, emissive: LinearRgba::rgb(0.6, 0.02, 0.01), unlit: true, double_sided: true, cull_mode: None, ..default() }));
    let (w, t) = (fit.width / 2.0, 0.035);
    let line = models.meshes.add(flat_mesh(&[[-w, -t], [w, -t], [w, t], [-w, t]], &[0, 1, 2, 0, 2, 3], fit.radius, 0.05));
    let a = 0.45;
    let left = models.meshes.add(flat_mesh(&[[-w, -a], [-w + a * 1.1, 0.0], [-w, a]], &[0, 1, 2], fit.radius, 0.06));
    let right = models.meshes.add(flat_mesh(&[[w, -a], [w, a], [w - a * 1.1, 0.0]], &[0, 1, 2], fit.radius, 0.06));
    for (i, c) in fit.centres.iter().enumerate() {
        commands.spawn((Mesh3d(line.clone()), MeshMaterial3d(mark.clone()), Transform::from_translation(*c), Visibility::Inherited, ChildOf(holder)));
        let arrow = match i {
            0 => Some(&left),
            2 => Some(&right),
            _ => None,
        };
        if let Some(m) = arrow {
            commands.spawn((Mesh3d(m.clone()), MeshMaterial3d(mark.clone()), Transform::from_translation(*c), Visibility::Inherited, ChildOf(holder)));
        }
    }
    (holder, drums)
}

/// Turn the reels of every machine in use; build or take away their drums.
#[allow(clippy::too_many_arguments)]
fn drums(
    mut commands: Commands,
    machines: Res<SlotMachines>,
    screen: Res<SlotsScreen>,
    mut models: ModelCtx,
    time: AidTime,
    mut views: Local<Views>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    rigs: Query<(Entity, &crate::object_anims::GroupRig)>,
    refs: Query<(Entity, &nvf_play::RefVisual)>,
    world_root: Query<Entity, With<crate::world::WorldRoot>>,
    sim: Option<Res<nvf_play::sim::SimState>>,
    play: Option<Res<nvf_play::PlayWorld>>,
    mut reels3d: ResMut<SlotsReels3d>,
) {
    let now = time.elapsed_secs();
    // Take away the drums of machines no longer in use.
    let gone: Vec<FormId> = views.drawn.keys().filter(|m| !machines.0.contains_key(m)).copied().collect();
    for m in gone {
        if let Some(mut d) = views.drawn.remove(&m) {
            commands.entity(d.holder).despawn();
            crate::roulette_view::unhide(&mut d.hidden, &mut visibility);
            if let Some((e, stored)) = d.arm {
                if let Ok(mut t) = transforms.get_mut(e) {
                    *t = stored;
                }
            }
            info!("slots view: drums on {:08X} taken away", m.0);
        }
    }
    views.failed.retain(|m, _| machines.0.contains_key(m));
    for (&m, spin) in &machines.0 {
        if !views.drawn.contains_key(&m) && !views.failed.contains_key(&m) {
            let (Some(sim), Some(play), Ok(root)) = (sim.as_deref(), play.as_deref(), world_root.single()) else { continue };
            match fit_of(m, &rigs, &refs, sim, play, &mut models, root, &mut commands) {
                Some(fit) => {
                    // The drum shape's material, to wear the pictures.
                    let material = rigs.iter().find(|(_, g)| g.form_id == m.0).and_then(|(_, g)| {
                        let rig = g.model().rig.as_ref()?;
                        let r = rig.nodes.iter().position(|n| n.name.as_deref().is_some_and(|x| x.eq_ignore_ascii_case("Reel01")))?;
                        rig.shapes.iter().find(|s| s.node == r || rig.nodes[s.node].parent == Some(r)).map(|s| s.material.clone())
                    });
                    if views.reel.is_none() {
                        views.reel = Some(reel_model(&mut models));
                        match views.reel.as_ref().and_then(Option::as_ref) {
                            Some(r) => info!("slots view: the reel model has {} faces (radius {:.2}, width {:.2})", r.faces, r.radius, r.width),
                            None => info!("slots view: no reel model ({REEL_MODEL}); the drums are NVForever's own"),
                        }
                    }
                    let reel = views.reel.as_ref().and_then(Option::as_ref);
                    let (holder, drums) = build(&mut commands, &fit, spin, &mut models, material, reel);
                    let mut hidden = Vec::new();
                    for e in &fit.hide {
                        if let Ok(mut v) = visibility.get_mut(*e) {
                            hidden.push((*e, *v));
                            *v = Visibility::Hidden;
                        }
                    }
                    info!(
                        "slots view: drums on {:08X} ({}; radius {:.2}, width {:.2}; {} stops; arm {}; faces {})",
                        m.0,
                        fit.how,
                        fit.radius,
                        fit.width,
                        spin.reel.len(),
                        if fit.arm.is_some() { "found" } else { "none" },
                        if reel.is_some_and(|r| r.faces == 2 * spin.reel.len()) { "as the reel model" } else { "NVForever's own" }
                    );
                    BUILT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let holder = if fit.how.starts_with("the cabinet") { fit.parent } else { holder };
                    views.drawn.insert(m, Drawn { holder, drums, hidden, arm: fit.arm, spin: spin.spin, pulled_at: None });
                }
                None => {
                    info!("slots view: {:08X} has no reels to drive (the menu's flat reels show)", m.0);
                    views.failed.insert(m, ());
                }
            }
        }
        let Some(d) = views.drawn.get_mut(&m) else { continue };
        let n = spin.reel.len();
        for (e, p) in d.drums.iter().zip(spin.positions(now)) {
            if let Ok(mut t) = transforms.get_mut(*e) {
                t.rotation = drum_turn(p, n);
            }
        }
        // A new spin pulls the arm.
        if spin.spin != d.spin {
            d.spin = spin.spin;
            if let Some((since, _)) = spin.spinning {
                d.pulled_at = Some(since);
            }
        }
        if let (Some((e, stored)), Some(t0)) = (d.arm, d.pulled_at) {
            let t = now - t0;
            if let Ok(mut tr) = transforms.get_mut(e) {
                *tr = Transform { rotation: stored.rotation * Quat::from_rotation_x(arm_angle(t)), ..stored };
            }
            if t > PULL[PULL.len() - 1].0 + PULL_BACK {
                d.pulled_at = None;
            }
        }
    }
    let mine = screen.0.as_ref().and_then(|s| s.machine).is_some_and(|m| views.drawn.contains_key(&m));
    reels3d.set_if_neq(SlotsReels3d(mine));
}

/// The camera's shots.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Shot {
    Reels,
    Third,
}

#[derive(Default)]
struct Cam {
    on: Option<FormId>,
    shot: Option<Shot>,
    from: Option<(Transform, f32)>,
    start: f32,
    length: f32,
    /// A win: when its result came and how long it is celebrated.
    won: Option<(f32, f32)>,
    last: Option<(Transform, f32)>,
}

/// A shot's camera (Bevy space) and field of view at the machine placed by `place`.
fn shot_at(place: Affine3A, shot: Shot) -> (Transform, f32) {
    let to = |p: Vec3| nvf_play::space::to_bevy(place.transform_point3(p).to_array());
    match shot {
        Shot::Reels => {
            let centre = Vec3::new(DRUM_X[1], AXIS_YZ.x, AXIS_YZ.y) + payline_dir() * DRUM_R;
            let eye = centre + payline_dir() * REEL_VIEW_BACK;
            (Transform::from_translation(to(eye)).looking_at(to(centre), Vec3::Y), REEL_FOV)
        }
        Shot::Third => (Transform::from_translation(to(THIRD_EYE)).looking_at(to(THIRD_AIM), Vec3::Y), THIRD_FOV),
    }
}

/// The spin camera [design]: third person at the pull, in close on the reels while
/// they turn, out to third person for a win's celebration, back for the next spin.
#[allow(clippy::too_many_arguments)]
fn camera(
    screen: Res<SlotsScreen>,
    reels3d: Res<SlotsReels3d>,
    mut spins: MessageReader<SlotsSpin>,
    time: AidTime,
    sim: Option<Res<nvf_play::sim::SimState>>,
    mut table_camera: ResMut<TableCamera>,
    mut control: ResMut<nvf_play::control::PlayerControl>,
    player_cam: Query<(&Transform, &Projection), With<nvf_play::PlayerCamera>>,
    mut body: Option<ResMut<crate::player_body::PlayerBody>>,
    mut cam: Local<Cam>,
) {
    let now = time.elapsed_secs();
    for s in spins.read() {
        match s.phase {
            SpinPhase::Started { .. } => {
                cam.won = None;
            }
            SpinPhase::Result { celebrate, .. } if celebrate > 0.0 => cam.won = Some((now, celebrate)),
            _ => {}
        }
    }
    let machine = screen.0.as_ref().and_then(|s| s.machine).filter(|_| reels3d.0);
    let place = machine.zip(sim.as_deref()).and_then(|(m, sim)| crate::roulette_view::ref_place(sim, m));
    let (Some(m), Some(place)) = (machine, place) else {
        if cam.on.take().is_some() {
            table_camera.set_if_neq(TableCamera(None));
            control.scene_cam = false;
            if let Some(b) = body.as_deref_mut().filter(|b| b.out_of_view) {
                b.out_of_view = false;
            }
            *cam = Cam::default();
        }
        return;
    };
    if cam.on != Some(m) {
        // Opened: from where the game's camera is now, in to the reels.
        let from = player_cam.single().ok().map(|(t, p)| (*t, if let Projection::Perspective(p) = p { p.fov.to_degrees() } else { REEL_FOV }));
        *cam = Cam { on: Some(m), shot: Some(Shot::Reels), from, start: now, length: OPEN_PAN, ..default() };
        control.scene_cam = true;
    }
    // The shot wanted now, and how slowly to get there: in on the reels, out to third
    // person only while a win is celebrated.
    let celebrating = cam.won.is_some_and(|(at, c)| now >= at && now < at + ZOOM_OUT + c);
    let (want, length) = if celebrating { (Shot::Third, ZOOM_OUT) } else if cam.shot == Some(Shot::Third) { (Shot::Reels, BACK_IN) } else { (Shot::Reels, OPEN_PAN) };
    if cam.shot != Some(want) {
        cam.from = cam.last;
        cam.shot = Some(want);
        cam.start = now;
        cam.length = length;
    }
    let (to, to_fov) = shot_at(place, want);
    let f = ease((now - cam.start) / cam.length.max(1e-3));
    let out = match cam.from {
        Some((from, from_fov)) if f < 1.0 => (
            Transform { translation: from.translation.lerp(to.translation, f), rotation: from.rotation.slerp(to.rotation, f), scale: Vec3::ONE },
            from_fov + (to_fov - from_fov) * f,
        ),
        _ => (to, to_fov),
    };
    cam.last = Some(out);
    table_camera.set_if_neq(TableCamera(Some(out)));
    // In close on the reels the seated body is out of the view: its head and arms
    // (leaning to the arm) come between the camera and the reels [observed 2026-11-11,
    // test run, 40-42.5 s]. Shown again once the camera is halfway out to third
    // person, whichever pan it is on.
    let (reels, third) = (shot_at(place, Shot::Reels).0.translation, shot_at(place, Shot::Third).0.translation);
    let close = out.0.translation.distance(reels) < 0.5 * reels.distance(third);
    if let Some(b) = body.as_deref_mut().filter(|b| b.out_of_view != close) {
        b.out_of_view = close;
        info!("slots view: the player's body {} the camera's view", if close { "out of" } else { "back in" });
    }
    if !control.scene_cam {
        control.scene_cam = true;
    }
}

fn report(mut exit: MessageReader<AppExit>) {
    let built = BUILT.load(std::sync::atomic::Ordering::Relaxed);
    if exit.read().next().is_some() && built > 0 {
        info!("slots view summary: drums built on {built} machines");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_payline_faces_the_window() {
        let p = payline_dir();
        assert!(p.y < -0.95 && p.z > 0.25 && p.z < 0.27);
        // Down the window from the payline is a positive turn.
        assert!(round(0.3).z < p.z);
    }

    #[test]
    fn a_stop_comes_to_the_payline() {
        let n = 14;
        let step = std::f32::consts::TAU / n as f32;
        for k in 0..n {
            // Face k sits at -k steps on the drum; turned by k stops it is on the line.
            let face = Quat::from_rotation_x(-(k as f32) * step) * payline_dir();
            let on = drum_turn(k as f32, n) * face;
            assert!((on - payline_dir()).length() < 1e-4, "stop {k}");
            // The next stop stands above the line, the one before below it.
            let next = drum_turn(k as f32, n) * (Quat::from_rotation_x(-((k + 1) as f32) * step) * payline_dir());
            assert!(next.z > on.z);
        }
    }

    #[test]
    fn the_arm_is_pulled_and_let_back() {
        assert_eq!(arm_angle(0.0), 0.0);
        assert!((arm_angle(0.33) - 0.774).abs() < 1e-4);
        assert!((arm_angle(0.7) - 0.861).abs() < 1e-4);
        assert!(arm_angle(0.83 + PULL_BACK + 0.01).abs() < 1e-4);
    }

    #[test]
    fn a_turned_drum_measures_the_same_radius() {
        // An 8-sided drum, a vertex on +Z, then the same drum turned 15° about its axis.
        let drum = |turn: f32| {
            let mut pos = Vec::new();
            for k in 0..8 {
                let a = turn + k as f32 * std::f32::consts::TAU / 8.0;
                for x in [0.0f32, 3.79] {
                    pos.push([x, 8.19 * a.sin(), 8.19 * a.cos()]);
                }
            }
            Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        };
        for turn in [0.0f32, 15f32.to_radians()] {
            let (c, h) = mesh_bounds(&drum(turn)).unwrap();
            assert!(c.yz().length() < 1e-4 && (c.x - 1.895).abs() < 1e-4);
            assert!((h.y - 8.19).abs() < 1e-4 && (h.z - 8.19).abs() < 1e-4 && (h.x - 1.895).abs() < 1e-4);
        }
    }

    #[test]
    fn an_extracted_mesh_has_no_bounds_and_does_not_panic() {
        // A placed machine's reel mesh is render-world only: once drawn, its vertex data
        // is gone from the main world (the 103dbf51 crash at The Tops).
        let mut m = face_mesh(8.0, 4.0, 0.3, 4, SYMBOL_U, (0.0, 1.0));
        assert!(mesh_bounds(&m).is_some());
        m.take_gpu_data().unwrap();
        assert!(mesh_bounds(&m).is_none());
    }

    #[test]
    fn faces_face_out() {
        let m = face_mesh(8.0, 4.0, 0.3, 4, SYMBOL_U, (0.0, 1.0));
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
        let Some(Indices::U32(i)) = m.indices() else { panic!() };
        // Each triangle's normal points away from the axis.
        for t in i.chunks(3) {
            let (a, b, c) = (Vec3::from(p[t[0] as usize]), Vec3::from(p[t[1] as usize]), Vec3::from(p[t[2] as usize]));
            let n = (b - a).cross(c - a);
            let mid = (a + b + c) / 3.0;
            assert!(n.dot(Vec3::new(0.0, mid.y, mid.z)) > 0.0);
        }
    }
}
