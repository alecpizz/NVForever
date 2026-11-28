//! References a script enables that render streaming never drew.
//!
//! Render streaming (`engine/nvforever/src/world.rs`) skips references that are not
//! enabled at the start of a game (initially disabled with no enable parent, or following
//! a parent in the other state), so `RefVisual` sync can hide what scripts disable but has nothing to show for
//! what they enable (VCG01's Rorschach cards in Doc Mitchell's house, enabled by Doc's
//! lines). While scripts run, this module draws those references itself when the
//! simulation says they are present, the way `dropped.rs` draws created references: the
//! base form's model at the reference's pose (the simulation's, if a script moved it),
//! as children of the cell's `RenderCell` entity, with `RefVisual` on each mesh entity
//! (crosshair bounds and visual sync see them like any other). Collision follows from
//! `RefChanged` (`phys.rs` restores a reference's collision when it becomes present).
//! One that physics then moves (the tutorial's bottles, shot off their fence) is drawn
//! where the simulation holds it ([`follow`]; `crate::moved` only moves render
//! streaming's entities, the children of a cell).
//!
//! A posable reference with a fitting `XRGD` (a clutter skeleton) is drawn in its stored
//! pose by the same rule render streaming uses (`crate::clutter_pose::drawn`, issue #2339:
//! the Wild Wasteland refrigerator skeleton, under an initially disabled enable parent,
//! stood upright). Presence comes from the simulation, so a save with the parent already
//! enabled is drawn here, posed, too.
//!
//! Not drawn here: actors (ACHR/ACRE; `actors.rs` builds them), SpeedTrees, markers, and
//! lights' light sources (only a light's model). The enabled-at-start rule is a copy of
//! world.rs's (`WorldData::enabled`) [coordination: one shared function would keep the
//! two from drifting; docs/design/layer5-play.md].

use crate::clutter_pose::{self, BodyNodes, Posed};
use crate::phys::PhysState;
use crate::script::ScriptState;
use crate::sim::SimState;
use crate::{PlaySet, PlayWorld, RefVisual, RenderCell};
use bevy::math::Affine3A;
use bevy::prelude::*;
use fnv_sim::FormId;
use fnv_world::placed::{base_object, placement, BaseObject, Placement};
use fnv_world::transform::ref_rotation;
use nvf_assets::model::{ModelCtx, PreparedModel};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Base types render streaming draws (world.rs `DRAWN`), without TREE (SpeedTree).
const DRAWN: [&[u8; 4]; 24] = [
    b"STAT", b"SCOL", b"MSTT", b"FURN", b"DOOR", b"CONT", b"ACTI", b"TACT", b"TERM", b"LIGH", b"MISC", b"WEAP", b"AMMO", b"ARMO",
    b"ALCH", b"BOOK", b"NOTE", b"KEYM", b"PWAT", b"IMOD", b"CHIP", b"CCRD", b"CMNY", b"PROJ",
];

/// A reference render streaming leaves out at the start but this module can draw.
#[derive(Debug, Clone)]
struct Dormant {
    placement: Placement,
    path: String,
    /// The base form's record type (whether its `XRGD` can pose it, `clutter_pose::posable`).
    sig: [u8; 4],
}

#[derive(Component, Clone, Copy, Debug)]
pub struct ScriptDrawnRoot(pub u32);

/// A shape drawn here whose colour an external emittance source drives (its shader sets
/// "External_Emittance" and its reference names an XEMI source): the renderer swaps in
/// the driven material (nvforever `emittance.rs`), as render streaming does for
/// references enabled from the start. Without it the Strip's street lamp beams, enabled
/// by script at night, drew white instead of the stripStreetLight colour.
#[derive(Component, Clone, Copy, Debug)]
pub struct WantsEmittance(pub u32);

/// The model a [`ScriptDrawnRoot`] draws (archive path): the engine puts the model's
/// particle emitters under the root (`nvforever::particles`, particles.md §7.4).
#[derive(Component, Clone, Debug)]
pub struct ScriptDrawnModel(pub String);

/// On the rig root of a door that opens in place drawn here: what nvforever's door
/// drawing (`nvforever::doors::DoorRig`) needs to pose it from `crate::doors::Doors`,
/// as for one render streaming drew (#2420).
#[derive(Component, Clone, Debug)]
pub struct ScriptDrawnDoor {
    pub reference: u32,
    pub path: String,
    /// The rig's node entities by name.
    pub nodes: HashMap<String, Entity>,
    /// ONAM "Open by Default" of the placement.
    pub open_by_default: bool,
}

/// On the rig root of a model drawn here whose rig has named sequences (`Forward`,
/// `Left`, …): its node entities by name, so the engine can play the groups scripts
/// ask for on it as on one render streaming drew (`nvforever::object_anims`). VMS15's
/// rising bomber is enabled by the detonator and then plays `Forward`.
#[derive(Component, Clone, Debug)]
pub struct ScriptDrawnRig {
    pub reference: u32,
    pub path: String,
    pub nodes: HashMap<String, Entity>,
}

#[derive(Resource, Default)]
pub struct ScriptDrawn {
    /// Dormant references per render cell, read when the cell first loads.
    cells: HashMap<RenderCell, Vec<Dormant>>,
    /// Drawn: reference → (root, the cell entity it hangs under).
    pub drawn: BTreeMap<u32, (Entity, Entity)>,
    /// References whose stored pose was logged (once per reference, however often its
    /// cell comes back).
    posed_told: HashSet<u32>,
}

pub struct ScriptDrawPlugin;

impl Plugin for ScriptDrawPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptDrawn>().add_systems(Update, (sync, follow).chain().in_set(PlaySet::Sync));
    }
}

/// Whether render streaming draws a reference at the start of a game (world.rs's rule):
/// not deleted, and its enable parent in the required state, or with no parent not
/// initially disabled.
fn enabled_at_start(play: &PlayWorld, p: &Placement, depth: usize) -> bool {
    if p.deleted() {
        return false;
    }
    match p.enable_parent {
        None => !p.initially_disabled(),
        Some(_) if depth > 16 => false,
        Some((parent, opposite)) => match placement(play.lo, play.schema, parent) {
            Some(pp) => enabled_at_start(play, &pp, depth + 1) != opposite,
            None => !opposite && !p.initially_disabled(),
        },
    }
}

pub(crate) fn is_marker(path: &str) -> bool {
    let file = path.rsplit('\\').next().unwrap_or(path);
    file.starts_with("marker") || path.contains("\\markers\\")
}

/// Whether a base form is of a type drawn from its model ([`DRAWN`]; not a SpeedTree).
pub(crate) fn drawn_type(b: &BaseObject) -> bool {
    DRAWN.contains(&&b.sig) && !b.is_speedtree()
}

/// The dormant references of a render cell.
fn dormant_in(play: &PlayWorld, sim: &SimState, cell: RenderCell) -> Vec<Dormant> {
    let ids: Vec<FormId> = match cell {
        RenderCell::Interior(id) => sim.refs.by_cell.get(&FormId(id)).cloned().unwrap_or_default(),
        RenderCell::Exterior { world, x, y } => sim
            .refs
            .world_grid
            .get(&FormId(world))
            .and_then(|g| g.get(&(x, y)))
            .and_then(|c| sim.refs.by_cell.get(c))
            .cloned()
            .unwrap_or_default(),
    };
    ids.into_iter()
        .filter_map(|r| {
            let p = play.index.placement(play.lo, play.schema, r.0)?;
            // A primitive enabled by a script still shows nothing (level-design.md §4.1).
            if &p.sig != b"REFR" || p.deleted() || !p.draws_base_model() || enabled_at_start(play, &p, 0) {
                return None;
            }
            let b = base_object(play.lo, play.schema, p.base?)?;
            if !drawn_type(&b) {
                return None;
            }
            let path = b.world_model_path().filter(|m| !is_marker(m) && !m.ends_with(".spt"))?;
            Some(Dormant { placement: p, path, sig: b.sig })
        })
        .collect()
}

/// Reference pose in game space (the simulation's position and rotation once it holds
/// state for the reference).
fn pose(sim: &SimState, p: &Placement) -> Affine3A {
    let (pos, rot) = sim.world.peek(FormId(p.form_id)).map_or((p.position, p.rotation), |s| (s.pos, s.rot));
    let m = Mat3::from_cols_array(&ref_rotation(rot)).transpose(); // row-major
    let scale = sim.script_scale(p.form_id).unwrap_or(p.scale);
    Affine3A::from_mat3_translation(m * scale, Vec3::from_array(pos))
}

/// The stored pose a dormant reference is drawn in (`clutter_pose::drawn`, the rule render
/// streaming uses), or `None` for the file's own. `bodies` gives the model's body nodes
/// (`clutter_pose::body_nodes`; tests hand them in).
fn stored_pose(play: &PlayWorld, d: &Dormant, model: &PreparedModel, bodies: impl FnOnce() -> Option<BodyNodes>) -> Option<Posed> {
    clutter_pose::drawn(play.lo, &d.sig, d.placement.form_id, &d.path, &model.nodes, bodies)
}

type Cells<'w, 's> = Query<'w, 's, (Entity, &'static RenderCell, &'static Transform), Without<ScriptDrawnRoot>>;

/// Move the roots of the references physics moved this frame
/// (`PhysState::settle_placed`), a script's Rotate turned or SetScale resized
/// (`Game::turned`), or the console moved (`crate::RefRepose`, which still names what the
/// tick cleared from `turned`), to the simulation's pose and scale; a body moving under
/// physics is drawn between the last two steps instead (`crate::moved::drawn_affine`).
fn follow(sim: Res<SimState>, phys: Res<PhysState>, script: Res<ScriptState>, mut repose: MessageReader<crate::RefRepose>, state: Res<ScriptDrawn>, cells: Cells, mut roots: Query<&mut Transform, With<ScriptDrawnRoot>>) {
    let turned = script.rt.iter().flat_map(|rt| rt.game.turned.iter().map(|r| r.0));
    // Clutter drawn between physics steps too (`PhysState::between`), every frame while it
    // moves and once after.
    let moving = phys.moved_now.iter().chain(&phys.between.redraw).copied();
    for id in moving.chain(turned).chain(repose.read().map(|m| m.form_id)) {
        let Some(&(root, cell_e)) = state.drawn.get(&id) else { continue };
        let (Ok((_, _, cell)), Ok(mut t), Some(p)) = (cells.get(cell_e), roots.get_mut(root), sim.refs.get(FormId(id))) else { continue };
        *t = Transform::from_matrix(Mat4::from(Affine3A::from_translation(-cell.translation) * crate::moved::drawn_affine(&sim, &phys, id, p)));
    }
}


#[allow(clippy::too_many_arguments)]
pub(crate) fn sync(
    mut commands: Commands,
    play: Res<PlayWorld>,
    sim: Res<SimState>,
    script: Res<ScriptState>,
    cells: Cells,
    roots: Query<(), With<ScriptDrawnRoot>>,
    mut state: ResMut<ScriptDrawn>,
    mut models: ModelCtx,
    fades: Option<Res<crate::fade::Fades>>,
) {
    if script.rt.is_none() {
        return;
    }
    let st = &mut *state;
    let loaded: HashMap<RenderCell, (Entity, Vec3)> = cells.iter().map(|(e, c, t)| (*c, (e, t.translation))).collect();
    // Drawn under a cell entity that went (render streaming, travel): gone with it.
    st.drawn.retain(|_, (root, cell_e)| roots.contains(*root) && loaded.values().any(|(e, _)| e == cell_e));
    for (&cell, &(cell_e, origin)) in &loaded {
        let dormant = st.cells.entry(cell).or_insert_with(|| dormant_in(&play, &sim, cell));
        for d in dormant.iter() {
            let id = d.placement.form_id;
            // One fading out (Disable 1, `crate::fade`) stays until its fade is over
            // [from the branch claude/script-params-engine].
            let want = sim.present(id) || fades.as_ref().is_some_and(|f| f.fading_out(id));
            match st.drawn.get(&id) {
                Some(&(root, e)) if !want || e != cell_e => {
                    commands.entity(root).try_despawn();
                    st.drawn.remove(&id);
                    info!("reference 0x{id:08X} no longer drawn by scripts (disabled or moved cell)");
                }
                Some(_) => {}
                None if want => {
                    let Some(model) = models.model(&d.path) else {
                        warn_once!("reference 0x{id:08X}: model {} could not be loaded", d.path);
                        continue;
                    };
                    let t = Transform::from_matrix(Mat4::from(Affine3A::from_translation(-origin) * pose(&sim, &d.placement)));
                    let root = commands.spawn((Name::new(format!("Enabled {id:08X}")), ScriptDrawnRoot(id), ScriptDrawnModel(d.path.clone()), t, Visibility::default(), ChildOf(cell_e))).id();
                    // A rig that only plays the model as an effect (`Rig::effect`, a mine's light
                    // controller) is not drawn as one: as render streaming, a reference's own node
                    // hierarchy is for an idle, a door or script groups, and the flat shapes keep a
                    // mine's light parts (`crate::mines::MineLight`).
                    let shapes = match model.rig.as_ref().filter(|rig| rig.idle.is_some() || rig.door.is_some() || !rig.groups.is_empty()) {
                        Some(rig) => {
                            let (r, drawn, nodes) = rig.spawn_nodes(&mut commands, root, Transform::IDENTITY, &d.path, 0.0);
                            commands.entity(r).insert(RefVisual(id));
                            if !rig.groups.is_empty() {
                                commands.entity(r).insert(ScriptDrawnRig { reference: id, path: d.path.clone(), nodes: nodes.clone() });
                            }
                            // Its nodes by name, for what follows one (`crate::RefNodes`): the
                            // map spawn_nodes made, kept rather than dropped.
                            // A door that opens in place: nvforever's door drawing poses it
                            // as it does render streaming's (#2420: Ranger Station
                            // Charlie's door, enabled with its enable parent, stayed drawn
                            // shut while it opened and its collision swung away).
                            if rig.door.is_some() {
                                commands.entity(r).insert(ScriptDrawnDoor { reference: id, path: d.path.clone(), nodes: nodes.clone(), open_by_default: d.placement.open_by_default });
                            }
                            commands.entity(r).insert(crate::RefNodes { reference: id, nodes });
                            for e in &drawn {
                                commands.entity(*e).insert(RefVisual(id));
                            }
                            drawn.len()
                        }
                        None => {
                            // A jointed object left posed by its designer (`XRGD`), as render
                            // streaming draws one enabled from the start (issue #2339).
                            let posed = stored_pose(&play, d, &model, || clutter_pose::body_nodes(&models.data, &d.path));
                            if let Some(p) = posed.as_ref().filter(|_| st.posed_told.insert(id)) {
                                info!("clutter pose: 0x{id:08X} {} drawn in its stored pose, enabled at run time ({} bodies)", d.path, p.bodies);
                            }
                            // Shapes below a billboard node under an entity for it that
                            // turns to the camera, as render streaming draws them (#2549).
                            let mut boards = nvf_assets::model::BakedBillboards::default();
                            for s in &model.shapes {
                                let shape = clutter_pose::shape_transform(posed.as_ref(), s);
                                let (parent, shape) = posed.is_none().then(|| boards.place(&mut commands, root, &model, Affine3A::IDENTITY, s, ())).flatten().unwrap_or((root, shape));
                                let e = commands.spawn((Mesh3d(s.mesh.clone()), MeshMaterial3d(s.material.clone()), Transform::from_matrix(Mat4::from(shape)), RefVisual(id), ChildOf(parent))).id();
                                if let Some(x) = d.placement.emittance.filter(|_| s.external_emittance) {
                                    commands.entity(e).insert(WantsEmittance(x));
                                }
                                if let Some(l) = crate::mines::MineLight::of(s.name.as_deref(), &s.ancestors) {
                                    commands.entity(e).insert(l);
                                }
                            }
                            model.shapes.len()
                        }
                    };
                    info!("reference 0x{id:08X} drawn (enabled at run time): {}, {shapes} shapes, in {cell:?}", d.path);
                    st.drawn.insert(id, (root, cell_e));
                }
                None => {}
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use fnv_esm::raw::{write_subrecords, Subrecord};
    use fnv_esm::{LoadOrder, Plugin, Schema};
    use fnv_world::WorldIndex;

    pub(crate) fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
        Subrecord { sig: *sig, data, extended: false }
    }

    pub(crate) fn record(sig: &[u8; 4], flags: u32, fid: u32, subs: &[Subrecord]) -> Vec<u8> {
        let payload = write_subrecords(subs);
        [&sig[..], &(payload.len() as u32).to_le_bytes(), &flags.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
    }

    fn group(label: [u8; 4], kind: i32, body: &[u8]) -> Vec<u8> {
        [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &kind.to_le_bytes(), &[0; 8], body].concat()
    }

    pub(crate) fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
        group(*sig, 0, body)
    }

    /// The plugins' side of a game over one synthetic plugin, decoded through the
    /// repository schemas (no game data; the builders follow menu/model/tests.rs).
    pub(crate) fn play_over(groups: &[Vec<u8>]) -> PlayWorld {
        let hedr = [&1.34f32.to_le_bytes()[..], &0i32.to_le_bytes(), &0x800u32.to_le_bytes()].concat();
        let mut bytes = record(b"TES4", 1, 0, &[sub(b"HEDR", hedr)]);
        bytes.extend(groups.concat());
        let plugins: &'static [Plugin] = Box::leak(vec![Plugin::parse(&bytes, "Test.esm".into()).unwrap()].into_boxed_slice());
        let lo: &'static LoadOrder<'static> = Box::leak(Box::new(LoadOrder::new(plugins).unwrap()));
        let schema: &'static Schema = Box::leak(Box::new(Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap()));
        let index: &'static WorldIndex = Box::leak(Box::default());
        PlayWorld(crate::PlayData { lo, schema, index, world: 0x3C, interior: None, eye: [0.0; 3], heading_deg: 0.0, pitch_deg: 0.0, auto_walk: 0.0, start_at_cell_spot: false, packages: false })
    }

    /// Issue #1618: what the console moved reaches a root drawn here through `RefRepose`
    /// alone (the tick has cleared `Game::turned` by then, and physics moved nothing).
    #[test]
    fn a_console_move_moves_the_root_and_stays() {
        use crate::sim::placed_ref;
        use fnv_sim::refs::PlacedRef;
        const CARD: u32 = 0x301;
        let at = [4100.0, 10.0, 20.0];
        let origin = Vec3::new(4096.0, 0.0, 0.0);
        let sim = SimState::over_placed([PlacedRef { pos: at, ..placed_ref(CARD) }]);
        let mut app = App::new();
        app.add_message::<crate::RefRepose>().insert_resource(sim).insert_resource(PhysState::default()).insert_resource(ScriptState::over_no_plugins()).init_resource::<ScriptDrawn>().add_systems(Update, follow);
        let cell = app.world_mut().spawn((RenderCell::Interior(0x200), Transform::from_translation(origin))).id();
        let drawn = Transform::from_translation(Vec3::from_array(at) - origin);
        let root = app.world_mut().spawn((ScriptDrawnRoot(CARD), drawn, ChildOf(cell))).id();
        app.world_mut().resource_mut::<ScriptDrawn>().drawn.insert(CARD, (root, cell));
        // As `SetPos z 170` from the console: the simulation holds the new height.
        app.world_mut().resource_mut::<SimState>().world.get(FormId(CARD)).unwrap().pos[2] = 170.0;
        let z = |app: &App| app.world().get::<Transform>(root).unwrap().translation.z;
        app.update();
        assert!((z(&app) - 20.0).abs() < 1e-3, "nothing named it: no sweep of the scene");
        app.world_mut().write_message(crate::RefRepose { form_id: CARD });
        app.update();
        assert!((z(&app) - 170.0).abs() < 1e-3);
        // The same again on a later frame, and when it is named again.
        app.update();
        assert!((z(&app) - 170.0).abs() < 1e-3);
        app.world_mut().write_message(crate::RefRepose { form_id: CARD });
        app.update();
        assert!((z(&app) - 170.0).abs() < 1e-3);
    }

    /// Issue #1620, as world.rs's `enabled` is tested: under an enable parent a reference's
    /// own Initially Disabled flag does not count.
    #[test]
    fn under_an_enable_parent_a_references_own_flag_does_not_count() {
        const FLAGGED: u32 = 0x800;
        const DELETED: u32 = 0x20;
        const ON: u32 = 0x301;
        const OFF: u32 = 0x302;
        const CELL: u32 = 0x200;
        let refr = |fid: u32, flags: u32, parent: Option<(u32, bool)>| {
            let mut subs = vec![sub(b"NAME", 0x100u32.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])];
            if let Some((p, opposite)) = parent {
                subs.push(sub(b"XESP", [&p.to_le_bytes()[..], &[opposite as u8, 0, 0, 0]].concat()));
            }
            record(b"REFR", flags, fid, &subs)
        };
        // (reference, its flags, its parent, enabled at the start)
        let cases = [
            (ON, 0, None, true),
            (OFF, FLAGGED, None, false),
            (0x310, FLAGGED, Some((ON, false)), true),
            (0x311, FLAGGED, Some((ON, true)), false),
            (0x312, FLAGGED, Some((OFF, false)), false),
            (0x313, FLAGGED, Some((OFF, true)), true),
            (0x314, 0, Some((ON, false)), true),
            (0x315, 0, Some((OFF, false)), false),
            (0x316, 0, Some((OFF, true)), true),
            // Two deep, flagged all the way down.
            (0x317, FLAGGED, Some((0x310, false)), true),
            // Deleted stays out; a parent that does not exist leaves the flag as it was.
            (0x318, DELETED | FLAGGED, Some((ON, false)), false),
            (0x319, FLAGGED, Some((0x999, false)), false),
            (0x31A, 0, Some((0x999, false)), true),
            (0x31B, 0, Some((0x999, true)), false),
        ];
        let refs: Vec<u8> = cases.iter().flat_map(|&(id, flags, parent, _)| refr(id, flags, parent)).collect();
        let label = CELL.to_le_bytes();
        let children = group(label, 6, &group(label, 9, &refs));
        let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", b"TestCell\0".to_vec())]);
        let cells = top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat())));
        let play = play_over(&[cells]);
        for (id, _, _, want) in cases {
            let p = placement(play.lo, play.schema, id).unwrap();
            assert_eq!(enabled_at_start(&play, &p, 0), want, "0x{id:X}");
        }
    }

    /// Issue #2339: a posable MSTT with `XRGD` under an initially disabled enable parent
    /// (the Wild Wasteland refrigerator skeleton) is left to this module, and once its
    /// parent is enabled it is drawn in its stored pose, by the rule render streaming
    /// uses, not in the file's own.
    #[test]
    fn a_posed_reference_under_a_disabled_parent_is_drawn_posed_once_enabled() {
        use fnv_sim::refs::{EnableParent, PlacedRef};
        use fnv_sim::{FormStore, RefTable, SimConfig, World};
        use nvf_assets::model::{ModelNode, PreparedShape};
        use std::sync::Arc;
        const SKELETON: u32 = 0x100;
        const PLANT: u32 = 0x101;
        const CELL: u32 = 0x200;
        const PARENT: u32 = 0x300;
        const POSED: u32 = 0x301;
        const NO_XRGD: u32 = 0x302;
        const NOT_POSABLE: u32 = 0x303;
        const FLAGGED: u32 = 0x800;
        // Two entries: the torso at the reference turned 90 degrees about z (a clockwise
        // angle, as references store it), the head 45 out along it (clutter_pose's own test).
        let xrgd: Vec<u8> = [([0.0f32; 3], [0.0, 0.0, -std::f32::consts::FRAC_PI_2]), ([45.0, 0.0, 0.0], [0.0; 3])]
            .iter()
            .flat_map(|(pos, angles)| [&[0u8, 0, 0, 0][..], &pos.iter().chain(angles).flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()[..]].concat())
            .collect();
        let refr = |fid: u32, flags: u32, base: u32, parent: bool, with_xrgd: bool| {
            let mut subs = vec![sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])];
            if parent {
                subs.push(sub(b"XESP", [&PARENT.to_le_bytes()[..], &[0, 0, 0, 0]].concat()));
            }
            if with_xrgd {
                subs.push(sub(b"XRGD", xrgd.clone()));
            }
            record(b"REFR", flags, fid, &subs)
        };
        // The parent's base is not a drawn type (no record): only the children are dormant.
        let refs = [refr(PARENT, FLAGGED, 0x999, false, false), refr(POSED, 0, SKELETON, true, true), refr(NO_XRGD, 0, SKELETON, true, false), refr(NOT_POSABLE, 0, PLANT, true, true)].concat();
        let label = CELL.to_le_bytes();
        let children = group(label, 6, &group(label, 9, &refs));
        let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", b"TestCell\0".to_vec())]);
        let cells = top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat())));
        let base = |sig: &[u8; 4], fid: u32, edid: &[u8]| top(sig, &record(sig, 0, fid, &[sub(b"EDID", edid.to_vec()), sub(b"MODL", b"clutter\\test\\skeleton.nif\0".to_vec())]));
        let play = play_over(&[base(b"MSTT", SKELETON, b"TestSkeleton\0"), base(b"ACTI", PLANT, b"TestPlant\0"), cells]);

        // The simulation's side: the parent initially disabled, the children following it.
        let child = |id: u32, base: u32| PlacedRef {
            base: FormId(base),
            enable_parent: Some(EnableParent { reference: FormId(PARENT), opposite: false, pop_in: false }),
            ..crate::sim::placed_ref(id)
        };
        let placed = [PlacedRef { flags: FLAGGED, base: FormId(0x999), ..crate::sim::placed_ref(PARENT) }, child(POSED, SKELETON), child(NO_XRGD, SKELETON), child(NOT_POSABLE, PLANT)];
        let mut table = RefTable::default();
        for p in placed {
            table.by_cell.entry(FormId(CELL)).or_default().push(p.id);
            table.refs.insert(p.id, p);
        }
        let forms: &'static FormStore = Box::leak(Box::default());
        let table: &'static RefTable = Box::leak(Box::new(table));
        let mut sim = SimState { forms, refs: table, world: World::new(forms, table, SimConfig::default()) };

        let dormant = dormant_in(&play, &sim, RenderCell::Interior(CELL));
        let ids: Vec<u32> = dormant.iter().map(|d| d.placement.form_id).collect();
        assert_eq!(ids, [POSED, NO_XRGD, NOT_POSABLE], "render streaming leaves them out: their parent starts disabled");
        assert!(!sim.present(POSED));
        sim.world.set_enabled(FormId(PARENT), true).unwrap();
        assert!(sim.present(POSED), "the parent enabled (a script, or a save that holds it so)");

        // Its model: Torso (body 0) on the root, Head (body 1) under it; one shape on the
        // head, one on the root.
        let node = |name: &str, parent: Option<usize>, t: Vec3| ModelNode { name: Some(name.into()), parent, local: Affine3A::from_translation(t), billboard: None, addon: None, damage_stages: None };
        let shape = |node: Option<usize>, transform: Affine3A| PreparedShape {
            mesh: Handle::default(),
            material: Handle::default(),
            transform,
            name: None,
            ancestors: Vec::new(),
            node,
            own: Affine3A::IDENTITY,
            cpu: None,
            segments: Vec::new(),
            external_emittance: false,
            look: nvf_assets::material_anim::Look::stored([0.0; 3], 1.0, 1.0),
            missing_diffuse: None,
            block: 0,
        };
        let model = PreparedModel {
            nodes: vec![node("Torso", None, Vec3::new(0.0, 0.0, 69.0)), node("Head", Some(0), Vec3::new(45.0, 0.0, 0.0))],
            shapes: vec![shape(Some(1), Affine3A::from_translation(Vec3::new(45.0, 0.0, 69.0))), shape(None, Affine3A::IDENTITY)],
            ..Default::default()
        };
        let bodies = || Some(Arc::new(vec![Some("Torso".to_string()), Some("Head".to_string())]));
        let d = |id: u32| dormant.iter().find(|d| d.placement.form_id == id).unwrap();

        let posed = stored_pose(&play, d(POSED), &model, bodies).expect("posed from its XRGD");
        assert_eq!(posed.bodies, 2);
        let head = clutter_pose::shape_transform(Some(&posed), &model.shapes[0]);
        assert!((Vec3::from(head.translation) - Vec3::new(0.0, 45.0, 0.0)).length() < 1e-4, "the head lies where the entries put it: {head:?}");
        assert_eq!(clutter_pose::shape_transform(Some(&posed), &model.shapes[1]), Affine3A::IDENTITY, "a shape on the root keeps its place");

        // No XRGD, or a base type the clutter pose leaves alone: the file's own pose, and
        // the model's bodies are not even read.
        let unread = || -> Option<clutter_pose::BodyNodes> { panic!("the bodies are read only for a posable reference with XRGD") };
        assert!(stored_pose(&play, d(NO_XRGD), &model, unread).is_none());
        assert!(stored_pose(&play, d(NOT_POSABLE), &model, unread).is_none());
        assert_eq!(clutter_pose::shape_transform(None, &model.shapes[0]), model.shapes[0].transform);
        // Entries that do not fit the bodies: refused, the file's own pose.
        let three = || Some(Arc::new(vec![Some("Torso".to_string()), Some("Head".to_string()), Some("Head".to_string())]));
        assert!(stored_pose(&play, d(POSED), &model, three).is_none());
    }
}
