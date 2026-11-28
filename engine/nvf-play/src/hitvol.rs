//! Actor hit volumes in play (docs/design/combat.md §5.2, step C3): every placed actor
//! (ACHR, ACRE) that is drawn gets a capsule in the physics world (`fnv_phys::actors`),
//! so shots, melee rays and the crosshair's collision ray can hit it.
//!
//! The capsule is sized and placed from the actor's render bounds each frame [design]:
//! the union of the world-space bounding boxes of the actor's drawn parts; feet at the
//! bottom centre, height the box's height, radius the smaller horizontal half-extent
//! (clamped to [`MIN_RADIUS`]..[`MAX_RADIUS`]). An actor that is hidden, removed by the
//! simulation or no longer drawn loses its volume. Dead actors (the script runtime's dead
//! state, combat C5) are on the DEAD_BIP layer; once a dead actor has a ragdoll in the
//! physics world, the ragdoll's bodies are what rays hit and it has no volume here.
//!
//! **Body parts** (step C8): when the renderer publishes an actor's posed body parts
//! ([`ActorParts`]: for each part of its BPTD, the segments along the part's bones), the
//! actor gets one capsule per segment instead (`PhysWorld::set_actor_parts`), so a hit
//! says which part it struck. A capsule's radius is a share of the actor's size (the
//! largest side of its bounds) by kind of limb ([`limb_radius`], [design]), or the widest
//! of the skeleton's own ragdoll shapes on the part's bones where that is wider
//! ([`PartSegment::radius`]: a robot's body, [observed: Mister Gutsy 21.2 against 14.6],
//! #1536; a human's shapes are all thinner than the shares). Each part's
//! chain of segments is pulled in by that radius at its two ends, so the capsule's caps
//! end at the part's end joints instead of reaching into the next part (the torso's top
//! cap would otherwise cover the lower head) [design]. The torso's lower end (the pelvis)
//! stays: pulled in, it left the hips between the thighs uncovered [observed: the
//! Goodsprings Settler, 2026-10-09].
//!
//! The same bounds and posed parts say where each living actor stands in the player's
//! way (`crate::blocking`); hit volumes themselves do not stop anyone walking.
//!
//! **Which meshes are looked at** (#1388): the bounds walk only the entities with an
//! [`ActorBoundsCandidate`], so not every static and tree of the loaded world. The mark is
//! kept in one place, by the hooks of the immutable [`RefVisual`]: an entity has it when
//! its reference may be an actor ([`SimState::may_be_actor`]), and every entity an actor's
//! bounds can come from has a `RefVisual`, whichever code spawned it. It says nothing of
//! visibility, bounds or the actor's presence, which are read each frame as before, as is
//! [`SimState::is_actor`]: a created reference's base can change under a mark that stays
//! [design]. It is not inherited: a child without a `RefVisual` of its own (the player
//! body, shader overlays, cut-off gore) was never counted, and the children of an actor
//! fading out at a door keep theirs after its root loses its own, as they did.
//!
//! Test aids that only measure (`crate::test_aid`): `NVF_PLAY_TEST_HITVOL_SCAN=1` logs
//! `hitvol scan:` lines, the walk's counts ([`ScanCounts`]) every [`SCAN_LOG_FRAMES`]
//! frames and a summary at exit; `NVF_PLAY_TEST_HITVOL_COMPARE=1` walks every `RefVisual`
//! as well each frame, the walk the mark replaced ([`compare_walks`], in the schedule only
//! then), and logs what the marked walk misses and each actor's meshes and volume as they
//! change (`hitvol compare:`, a summary at exit; what it does not show: [`ScanCompare`]).

use bevy::camera::primitives::Aabb;
use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::prelude::*;
use fnv_combat::data::Limb;
use fnv_phys::{HitVolume, PartCapsule};
use fnv_sim::FormId;
use std::collections::{BTreeMap, HashMap};

use crate::blocking::{self, ActorBlocks};
use crate::phys::{PhysState, PhysSystems};
use crate::sim::SimState;
use crate::{space, RefVisual};

/// Radius limits, game units [design: a thin limb pose or a T-pose bind box should not
/// make a needle or a wall].
pub const MIN_RADIUS: f32 = 10.0;
pub const MAX_RADIUS: f32 = 150.0;

/// Volumes in the world now, and the most at once (for the exit report).
#[derive(Resource, Default)]
pub struct HitVolumes {
    pub count: usize,
    pub most: usize,
    /// The living actors drawn now and where each one's feet are (game space), the
    /// player not among them.
    pub living: Vec<(u32, [f32; 3])>,
    /// What this frame's walk over the meshes found.
    pub scan: ScanCounts,
}

/// What one frame's walk for the actors' bounds found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanCounts {
    /// Meshes walked: with a mark, bounds and a transform.
    pub matched: usize,
    /// Those shown.
    pub visible: usize,
    /// Those shown whose reference is an actor ([`SimState::is_actor`]), before actors not
    /// present or with a ragdoll are left out.
    pub accepted: usize,
    /// Actors that got a volume from them.
    pub actors: usize,
}

/// On every entity of a reference that may be an actor ([`SimState::may_be_actor`]; see
/// the module notes). Kept by [`RefVisual`]'s hooks, nowhere else. A sparse set: most
/// entities never have it, and an actor's meshes keep their row in their table. A query
/// filtered on it walks archetypes, not tables, so the walk's order is not what it was;
/// its result is the same: bounds are mins and maxes, in a `BTreeMap` [design].
#[derive(Component, Clone, Copy, Debug, Default)]
#[component(storage = "SparseSet")]
pub struct ActorBoundsCandidate;

/// The references the other co-op players' bodies are drawn with on a guest (and on the
/// host before a guest's actor exists): `0xFE00_C000` + the player's id. They are neither
/// placed nor created references, so [`SimState::is_actor`] never accepts them, yet a shot
/// must meet them like any body for friendly fire [observed: buckallobill, in game: a
/// guest's shots went through the host]. They get a hit volume and part capsules, but no
/// block (the players are not in each other's way) and are not among the living actors
/// (no "enemies nearby" refusal, no combat music from a fellow player) [design]. Outside
/// co-op nothing is drawn with them.
pub const PLAYER_PUPPETS: std::ops::Range<u32> = 0xFE00_C000..0xFE00_C100;

/// Whether `form_id` is one of the [`PLAYER_PUPPETS`].
pub fn is_player_puppet(form_id: u32) -> bool {
    PLAYER_PUPPETS.contains(&form_id)
}

/// A creature skeleton's BBX half-width before base/reference scale [documented:
/// schemas/nif/scene.json; design: attached only to creature roots in actors.rs, #2535].
#[derive(Component)]
pub struct CreatureBbx {
    pub reference: u32,
    pub half_width: f32,
}

impl CreatureBbx {
    /// A circle fitted to the spacing [hypothesis: capture run15b_b2, 2026-11-15;
    /// the original's shape was not seen]. Root scale includes base and reference
    /// scale (`nvforever::actors::spawn_creature`) [design, #2535].
    fn radius(&self, root: &GlobalTransform) -> Option<f32> {
        let radius = self.half_width * root.affine().matrix3.x_axis.length();
        (radius.is_finite() && radius > 0.0).then_some(radius)
    }
}

/// Whether `entity` should have the mark now. Only with the simulation: [`crate::PlayPlugin`]
/// brings it together with [`HitVolumePlugin`], so a run that is not `--play` keeps no
/// volumes and spends nothing on marks. One that gets it late is marked by
/// [`reclassify_candidates`] at the next `First`: if it arrives after `First`, that frame's
/// walk does not find the meshes that were there before it.
/// A draw-only copy (`crate::DrawOnly`, nvforever's full-detail ring) is never one.
fn wants_candidate(world: &World, entity: Entity) -> bool {
    !world.entity(entity).contains::<crate::DrawOnly>() && world.get_resource::<SimState>().is_some_and(|sim| world.get::<RefVisual>(entity).is_some_and(|r| sim.may_be_actor(r.0) || is_player_puppet(r.0)))
}

/// Set the mark from the entity's reference as it is when this runs (not as it was when
/// queued). The hooks queue this; a world applying commands runs what they queue before
/// the next command, within the same sync point, so no system sees a `RefVisual` whose
/// mark is yet to come [Bevy 0.20 `world/command_queue.rs`].
fn settle_candidate(mut entity: EntityWorldMut) {
    let wanted = wants_candidate(entity.world(), entity.id());
    if wanted != entity.contains::<ActorBoundsCandidate>() {
        if wanted {
            entity.insert(ActorBoundsCandidate);
        } else {
            entity.remove::<ActorBoundsCandidate>();
        }
    }
}

/// [`RefVisual`] inserted, new or replacing another: most meshes of a world need no change
/// and queue nothing.
pub(crate) fn ref_visual_inserted(mut world: DeferredWorld, ctx: HookContext) {
    if wants_candidate(&world, ctx.entity) != world.entity(ctx.entity).contains::<ActorBoundsCandidate>() {
        world.commands().entity(ctx.entity).queue_silenced(settle_candidate);
    }
}

/// [`RefVisual`] removed, or its entity despawned (then the queued command finds nothing).
pub(crate) fn ref_visual_removed(mut world: DeferredWorld, ctx: HookContext) {
    if world.entity(ctx.entity).contains::<ActorBoundsCandidate>() {
        world.commands().entity(ctx.entity).queue_silenced(settle_candidate);
    }
}

/// When the simulation arrives, or another takes its place with other placements (the
/// hooks only see inserts): set every mark again. In `First`, so before this frame's walk
/// without a sync point of its own in `Update`, and a reference table's address tells it
/// has changed: `SimState` itself changes every frame. In a game the simulation is there
/// before any mesh and this finds nothing to do: [`crate::PlayPlugin`] inserts it once,
/// while building the app, nothing else inserts or removes it, and `SimState::refs` is a
/// `&'static` no code assigns, so the table cannot change later in a frame [design]. Only
/// tests replace it, between frames. Were it done after `First`, that frame's walk would
/// go by the old marks and the next `First` would set them.
fn reclassify_candidates(sim: Option<Res<SimState>>, mut table: Local<usize>, refs: Query<(Entity, &RefVisual, Has<ActorBoundsCandidate>)>, mut commands: Commands) {
    let Some(sim) = sim else {
        *table = 0;
        return;
    };
    let now = std::ptr::from_ref(sim.refs) as usize;
    if *table == now {
        return;
    }
    *table = now;
    for (e, r, has) in &refs {
        if (sim.may_be_actor(r.0) || is_player_puppet(r.0)) != has {
            commands.entity(e).queue_silenced(settle_candidate);
        }
    }
}

/// One segment of a body part, game space (step C8).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartSegment {
    /// The part's index in the actor's body part data.
    pub part: u8,
    pub limb: Limb,
    pub a: [f32; 3],
    pub b: [f32; 3],
    /// The widest of the skeleton's own collision shapes on the part's bones (its
    /// ragdoll bodies), game units: the capsule is at least this thick ([`part_capsules`]).
    pub radius: Option<f32>,
}

/// The part number of the weapon an actor holds drawn: one segment along the weapon,
/// after the body parts (no BPTD part has this index). A hit on it wears the weapon, not
/// the actor (`crate::combat`, docs/spec-notes/combat-rules.md §17).
pub const WEAPON_PART: u8 = 254;

/// An actor's body parts as posed last frame.
#[derive(Debug, Clone)]
pub struct PosedParts {
    /// The body part data (BPTD) the parts index.
    pub data: FormId,
    pub segments: Vec<PartSegment>,
    /// The weapon in the actor's hand, when it has a [`WEAPON_PART`] segment.
    pub weapon: Option<FormId>,
}

/// The posed body parts of drawn actors by reference FormID, published by the renderer
/// from the animated skeletons (step C8).
#[derive(Resource, Default)]
pub struct ActorParts {
    pub by_ref: HashMap<u32, PosedParts>,
}

impl ActorParts {
    /// The body part data an actor's posed parts index.
    pub fn data_of(&self, form_id: u32) -> Option<FormId> {
        self.by_ref.get(&form_id).map(|p| p.data)
    }
}

/// A body part capsule's radius as a share of the actor's size [design: about a human's
/// proportions at 128 units, a little generous for play].
pub fn limb_radius(limb: Limb, size: f32) -> f32 {
    let share = match limb {
        Limb::Head => 0.065,
        Limb::Torso => 0.10,
        Limb::Arm => 0.042,
        Limb::Leg => 0.058,
        Limb::Other => 0.05,
    };
    (share * size).max(2.0)
}

/// The capsules of an actor's posed parts, `size` being its largest side (see the module
/// notes): consecutive segments of one part form its chain, whose two ends are pulled in
/// by the radius (never past the next joint, or a lone segment's middle; not the torso's
/// lower end).
pub fn part_capsules(segments: &[PartSegment], size: f32) -> Vec<PartCapsule> {
    let mut out: Vec<PartCapsule> = Vec::with_capacity(segments.len());
    let mut i = 0;
    while i < segments.len() {
        // A chain: the segments of one part that follow on from each other (a part can
        // have side branches, `nvforever` limbs).
        let part = segments[i].part;
        let j = (i + 1..segments.len()).find(|&n| segments[n].part != part || segments[n].a != segments[n - 1].b).unwrap_or(segments.len());
        // At least as thick as the skeleton's own shapes on the part's bones: a robot's
        // body is far wider than a human's share of its size (Mister Gutsy's body capsule
        // 21 units against 14.6, a protectron's 18 against 13; #1536), while a human's
        // shapes are all thinner than the shares, which stay.
        let radius = limb_radius(segments[i].limb, size).max(segments[i].radius.unwrap_or(0.0));
        for (k, s) in segments[i..j].iter().enumerate() {
            let (mut a, mut b) = (Vec3::from(s.a), Vec3::from(s.b));
            let d = b - a;
            let len = d.length();
            if len > 1e-3 {
                // A lone segment shrinks from both ends, at most to its middle; an end
                // segment of a longer chain only from its outer end, at most to its joint.
                let most = if j - i == 1 { len / 2.0 } else { len };
                let pull = |n: f32| d / len * n.min(most);
                if k == 0 && segments[i].limb != Limb::Torso {
                    a += pull(radius);
                }
                if i + k + 1 == j {
                    b -= pull(radius);
                }
            }
            out.push(PartCapsule { part, a: a.to_array(), b: b.to_array(), radius });
        }
        i = j;
    }
    out
}

pub struct HitVolumePlugin;

impl Plugin for HitVolumePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HitVolumes>()
            .init_resource::<ActorParts>()
            .init_resource::<ActorBlocks>()
            .add_systems(First, reclassify_candidates)
            .add_systems(Last, report);
        // Only when asked for: the aids' systems are not in the schedule otherwise. Without
        // the comparison, `sync_volumes` looks up that its resource is missing and clones
        // nothing. Piped, the comparison is one system with the walk, so no sync point can
        // fall between them and change the meshes it compares [Bevy 0.20 `PipeSystem`].
        let compare = aid_on("NVF_PLAY_TEST_HITVOL_COMPARE");
        if compare {
            app.init_resource::<ScanCompare>();
        }
        app.add_systems(Update, volume_systems(compare));
        if aid_on("NVF_PLAY_TEST_HITVOL_SCAN") {
            app.init_resource::<ScanLog>().add_systems(Update, log_scan.after(sync_volumes).in_set(crate::PlaySet::Physics));
        }
    }
}

fn volume_systems(compare: bool) -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    let systems = if compare { sync_volumes.pipe(compare_walks).into_configs() } else { sync_volumes.into_configs() };
    systems.after(PhysSystems).in_set(crate::PlaySet::Physics)
}

fn aid_on(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !matches!(v.trim(), "" | "0"))
}

/// The game-space box of a part: the eight corners of its local bounds through its
/// transform, then into game space.
fn game_box(aabb: &Aabb, gt: &GlobalTransform) -> ([f32; 3], [f32; 3]) {
    let t = gt.affine();
    let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for i in 0..8 {
        let s = Vec3::new(if i & 1 == 0 { -1.0 } else { 1.0 }, if i & 2 == 0 { -1.0 } else { 1.0 }, if i & 4 == 0 { -1.0 } else { 1.0 });
        let p = space::to_game(t.transform_point3(c + h * s));
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

/// The capsule for an actor's game-space bounds.
pub fn volume_for(lo: [f32; 3], hi: [f32; 3], dead: bool) -> HitVolume {
    let half_x = (hi[0] - lo[0]) / 2.0;
    let half_y = (hi[1] - lo[1]) / 2.0;
    HitVolume {
        feet: [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, lo[2]],
        radius: half_x.min(half_y).clamp(MIN_RADIUS, MAX_RADIUS),
        height: (hi[2] - lo[2]).max(2.0 * MIN_RADIUS),
        dead,
    }
}


/// What the walk for the bounds reads of a mesh.
type BoundsPart = (&'static RefVisual, &'static Aabb, &'static GlobalTransform, &'static InheritedVisibility);

/// Each actor's game-space bounds: the union of the boxes of its shown meshes.
type Bounds = BTreeMap<u32, ([f32; 3], [f32; 3])>;

/// Walk meshes for the actors' bounds, whichever way the meshes were found.
fn gather<'a>(parts: impl Iterator<Item = (&'a RefVisual, &'a Aabb, &'a GlobalTransform, &'a InheritedVisibility)>, sim: &SimState) -> (Bounds, ScanCounts) {
    let mut bounds = Bounds::new();
    let mut scan = ScanCounts::default();
    for (r, aabb, gt, vis) in parts {
        scan.matched += 1;
        if !vis.get() {
            continue;
        }
        scan.visible += 1;
        // Placed actors and created ones (PlaceAtMe), `SimState::is_actor`.
        if !sim.is_actor(r.0) && !is_player_puppet(r.0) {
            continue;
        }
        scan.accepted += 1;
        let (lo, hi) = game_box(aabb, gt);
        let e = bounds.entry(r.0).or_insert((lo, hi));
        for k in 0..3 {
            e.0[k] = e.0[k].min(lo[k]);
            e.1[k] = e.1[k].max(hi[k]);
        }
    }
    (bounds, scan)
}

/// The marked meshes whose bounds make actors' hit volumes and blocking: never a
/// draw-only copy (`crate::DrawOnly`), even one marked before its `DrawOnly` came (the
/// hooks settle a mark before the next command, which may be the one adding it).
type CandidateParts<'w, 's> = Query<'w, 's, BoundsPart, (With<ActorBoundsCandidate>, Without<crate::DrawOnly>)>;

#[allow(clippy::too_many_arguments)]
fn sync_volumes(
    sim: Res<SimState>,
    script: Option<Res<crate::script::ScriptState>>,
    posed: Res<ActorParts>,
    parts: CandidateParts,
    creatures: Query<(&CreatureBbx, &GlobalTransform, &InheritedVisibility)>,
    mut compare: Option<ResMut<ScanCompare>>,
    mut phys: ResMut<PhysState>,
    mut vols: ResMut<HitVolumes>,
    mut blocks: ResMut<ActorBlocks>,
) {
    // No way out of here when nothing is found: no bounds is what clears the volumes,
    // blocks, living actors and heights below.
    let (mut bounds, mut scan) = gather(parts.iter(), &sim);
    if let Some(c) = compare.as_deref_mut() {
        c.used.clone_from(&bounds);
    }
    // A corpse with a ragdoll is hit on its ragdoll bodies (`fnv_phys::ragdoll`).
    bounds.retain(|&id, _| sim.present(id) && !phys.world.has_ragdoll(id));
    let stale: Vec<u32> = phys.world.actor_volumes().filter(|id| !bounds.contains_key(id)).collect();
    for id in stale {
        phys.world.remove_actor_volume(id);
    }
    // The blocks of actors in an attack keep their place (`ActorBlocks::attacking`).
    let held: HashMap<u32, blocking::Block> = blocks.list.iter().filter(|b| blocks.attacking.contains(&b.actor)).map(|b| (b.actor, *b)).collect();
    blocks.list.clear();
    vols.living.clear();
    phys.actor_heights.clear();
    phys.actor_block_radii.clear();
    let mut creature_feet = HashMap::new();
    for (bbx, root, vis) in &creatures {
        if vis.get() && bounds.contains_key(&bbx.reference) && !crate::combat::is_dead(script.as_deref(), bbx.reference) {
            if let Some(radius) = bbx.radius(root) {
                phys.actor_block_radii.insert(bbx.reference, radius);
                creature_feet.insert(bbx.reference, space::to_game(root.translation()));
            }
        }
    }
    for (&id, &(lo, hi)) in &bounds {
        let dead = crate::combat::is_dead(script.as_deref(), id);
        let v = volume_for(lo, hi, dead);
        let puppet = is_player_puppet(id);
        if !dead && id != FormId::PLAYER_REF.0 && !puppet {
            vols.living.push((id, v.feet));
            // How tall its character is made (`PhysState::add_npc`).
            phys.actor_heights.insert(id, hi[2] - lo[2]);
        }
        let posed = posed.by_ref.get(&id).filter(|p| !p.segments.is_empty());
        // In the player's way where its body is now, unless the character controller
        // walks it (its block is where its character is, `crate::player`).
        if !dead && id != FormId::PLAYER_REF.0 && !puppet && !phys.npcs.contains_key(&id) && !blocks.in_nobodys_way.contains(&id) {
            let mut block = blocking::block_for(id, lo, hi, posed.and_then(|p| blocking::torso(&p.segments)));
            // Stopping removes the character (nvforever::residents::think), but must
            // keep the widened creature circle at its feet [design, #2535].
            if let Some(&radius) = phys.actor_block_radii.get(&id).filter(|&&r| r > block.radius) {
                block = blocking::Block::character(id, creature_feet[&id], radius, hi[2] - lo[2], false);
            }
            // One that stands where it may be pushed is where its owner has it, with
            // the pushes still to come.
            // ACBS "Not Pushable": never pushed aside (actor-fields.md §1).
            let not_pushable = sim.base_of(id).zip(script.as_deref().and_then(|s| s.rt.as_ref())).is_some_and(|(b, rt)| { let (seed, at) = sim.pick(&rt.game, FormId(id)); rt.game.runtime_minor_flags(b, seed, at) }.not_pushable);
            let block = if not_pushable { block } else { blocks.standing_now(id).map_or(block, |at| block.standing_at(at)) };
            blocks.list.push(held.get(&id).copied().unwrap_or(block));
        }
        match posed {
            Some(p) => {
                let size = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f32, f32::max);
                let caps = part_capsules(&p.segments, size);
                if phys.world.actor_part_count(id) != caps.len() {
                    debug!("actor {id:08X}: {} part capsules (size {size:.0}, bounds {lo:?}..{hi:?}): {caps:?}", caps.len());
                }
                phys.world.set_actor_parts(id, v, &caps);
            }
            None => phys.world.set_actor_volume(id, v),
        }
    }
    if bounds.len() != vols.count {
        debug!("actor hit volumes: {}", bounds.len());
    }
    vols.count = bounds.len();
    vols.most = vols.most.max(vols.count);
    scan.actors = bounds.len();
    vols.scan = scan;
    if let Some(c) = compare.as_deref_mut() {
        c.kept = bounds;
    }
}

/// `NVF_PLAY_TEST_HITVOL_COMPARE`: piped onto [`sync_volumes`], which changes no mesh, so
/// it sees the meshes that walk saw.
fn compare_walks(mut compare: ResMut<ScanCompare>, every_mesh: EveryMeshes, sim: Res<SimState>, phys: Res<PhysState>) {
    compare.check_bounds(&every_mesh, &sim);
    compare.check_volumes(phys.world.actor_volumes());
}

/// Frames between `hitvol scan:` lines ([`log_scan`]).
pub const SCAN_LOG_FRAMES: u64 = 300;

/// The walk's counts over the run (`NVF_PLAY_TEST_HITVOL_SCAN`).
#[derive(Resource, Default, Debug)]
struct ScanLog {
    frames: u64,
    /// Totals over the run.
    total: [u64; 4],
    most: ScanCounts,
}

fn log_scan(vols: Res<HitVolumes>, mut log: ResMut<ScanLog>) {
    let s = vols.scan;
    log.frames += 1;
    for (t, n) in log.total.iter_mut().zip([s.matched, s.visible, s.accepted, s.actors]) {
        *t += n as u64;
    }
    log.most = ScanCounts { matched: log.most.matched.max(s.matched), visible: log.most.visible.max(s.visible), accepted: log.most.accepted.max(s.accepted), actors: log.most.actors.max(s.actors) };
    if log.frames % SCAN_LOG_FRAMES == 1 {
        info!("hitvol scan: matched={} visible={} accepted={} actors={}", s.matched, s.visible, s.accepted, s.actors);
    }
}

/// Lines naming what the marked walk missed, at most (the summary counts them all).
const COMPARE_TOLD: u64 = 20;

/// What the comparison reads of each mesh: the walk the mark replaced.
type EveryMesh = (Entity, BoundsPart, Has<ActorBoundsCandidate>);
/// Every mesh of a reference the comparison walks: not the draw-only copies
/// (`crate::DrawOnly`), which never make hit volumes ([`CandidateParts`]).
type EveryMeshes<'w, 's> = Query<'w, 's, EveryMesh, Without<crate::DrawOnly>>;

/// An actor's meshes in one frame's walk over every `RefVisual`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MeshCounts {
    shown: usize,
    all: usize,
    /// Those that were not its own in the last frame's walk: a mesh is walked once it has
    /// bounds, so a part put on counts a frame or more after its spawn.
    new: usize,
    /// Those that were its own in the last frame's walk and are not now.
    gone: usize,
}

/// The marked walk against the walk over every `RefVisual`, over the run
/// (`NVF_PLAY_TEST_HITVOL_COMPARE`), by [`compare_walks`]: the bounds [`sync_volumes`] goes
/// on to use against those of every mesh in the same frame, and the ids that have a volume
/// in physics against the actors it kept.
///
/// It shows the mark misses no mesh in the frames of this run. It does not show the frame
/// is as a build without the mark would have it: both walks share [`gather`], so a fault in
/// the union is in both, and blocks, heights and ray hits are not compared. The volumes are
/// compared by id alone ([`fnv_phys`]'s `actor_volumes`, the keys of its actors): a stale
/// place, a wrong size or layer, or wrong part capsules pass.
#[derive(Resource, Default, Debug)]
struct ScanCompare {
    frames: u64,
    /// Frames in which the full walk accepted a mesh.
    frames_with_actors: u64,
    /// The most meshes in a frame: the full walk's, the marked one's, accepted.
    full_matched: usize,
    marked_matched: usize,
    accepted: usize,
    /// The most meshes in a frame that the mark spared the walk.
    left_out: usize,
    /// Meshes the full walk accepted that have no mark, over all frames.
    missed: u64,
    /// References whose bounds differ between the walks, over all frames.
    mismatched: u64,
    /// Frames in which the ids with a volume in physics are not those of the actors kept.
    /// Ids only: see above.
    volume_ids_wrong: u64,
    /// This frame's bounds from [`sync_volumes`], before leaving out actors not present or
    /// with a ragdoll, as the full walk's are.
    used: Bounds,
    /// The actors [`sync_volumes`] gave a volume this frame.
    kept: Bounds,
    /// This frame's meshes of each actor, from the full walk. Hidden ones count here because
    /// they are in no bounds: the rifle of an actor under Weapons Unequipped (`nvforever`'s
    /// alert.rs hides its `Weapon` node) comes and goes without changing the shown number.
    meshes: BTreeMap<u32, MeshCounts>,
    /// The actor meshes of the last frame's full walk and their actors.
    walked: HashMap<Entity, u32>,
    /// Each actor's meshes, shown and all, and whether physics has a volume for it, as last
    /// logged.
    shown: BTreeMap<u32, (usize, usize, bool)>,
}

impl ScanCompare {
    fn told(&self) -> u64 {
        self.missed + self.mismatched + self.volume_ids_wrong
    }

    /// [`Self::used`] against the walk over every mesh.
    fn check_bounds(&mut self, every_mesh: &EveryMeshes, sim: &SimState) {
        let (full, full_scan) = gather(every_mesh.iter().map(|(_, part, _)| part), sim);
        let marked = every_mesh.iter().filter(|(_, _, has)| *has).count();
        self.meshes.clear();
        let mut walked = HashMap::with_capacity(self.walked.len());
        for (e, (r, _, _, vis), has) in every_mesh {
            if sim.is_actor(r.0) {
                walked.insert(e, r.0);
                let count = self.meshes.entry(r.0).or_default();
                count.all += 1;
                count.new += usize::from(self.walked.get(&e) != Some(&r.0));
                if vis.get() {
                    count.shown += 1;
                    if !has {
                        if self.told() < COMPARE_TOLD {
                            warn!("hitvol compare: missed entity {e} of reference {:08X}", r.0);
                        }
                        self.missed += 1;
                    }
                }
            }
        }
        for (_, r) in self.walked.iter().filter(|(e, r)| walked.get(*e) != Some(*r)) {
            if let Some(count) = self.meshes.get_mut(r) {
                count.gone += 1;
            }
        }
        self.walked = walked;
        let ids: std::collections::BTreeSet<u32> = full.keys().chain(self.used.keys()).copied().collect();
        for id in ids {
            let (a, b) = (full.get(&id), self.used.get(&id));
            if a != b {
                if self.told() < COMPARE_TOLD {
                    warn!("hitvol compare: bounds of reference {id:08X} differ: every mesh {a:?}, used {b:?}");
                }
                self.mismatched += 1;
            }
        }
        self.frames += 1;
        self.frames_with_actors += u64::from(full_scan.accepted > 0);
        self.full_matched = self.full_matched.max(full_scan.matched);
        self.marked_matched = self.marked_matched.max(marked);
        self.accepted = self.accepted.max(full_scan.accepted);
        self.left_out = self.left_out.max(full_scan.matched.saturating_sub(marked));
    }

    /// `volumes`: the ids with a volume in physics, against [`Self::kept`]. One line per
    /// change of an actor's meshes or volume, "has a volume" as physics has it, with the
    /// meshes that came and went in that frame: what a smoke scenario can hold one actor to.
    fn check_volumes(&mut self, volumes: impl Iterator<Item = u32>) {
        let volumes: std::collections::BTreeSet<u32> = volumes.collect();
        if !volumes.iter().eq(self.kept.keys()) {
            if self.told() < COMPARE_TOLD {
                warn!("hitvol compare: volume ids differ from the actors kept: {volumes:08X?}");
            }
            self.volume_ids_wrong += 1;
        }
        let now: BTreeMap<u32, (usize, usize, bool)> = self.meshes.iter().map(|(&id, c)| (id, (c.shown, c.all, volumes.contains(&id)))).collect();
        for (id, &(shown, all, has)) in &now {
            let MeshCounts { new, gone, .. } = self.meshes[id];
            if self.shown.get(id) != Some(&(shown, all, has)) || new + gone > 0 {
                info!("hitvol compare: actor {id:08X} has {shown} of {all} meshes shown and {}; {new} new, {gone} gone", if has { "a volume" } else { "no volume" });
            }
        }
        for id in self.shown.keys().filter(|id| !now.contains_key(id)) {
            info!("hitvol compare: actor {id:08X} has 0 of 0 meshes shown and no volume");
        }
        self.shown = now;
    }
}

/// At exit: the counts, and a probe of each volume: a level ray from 150 units away
/// (from the side the player is on) at its aim point (the torso with body parts, else the
/// capsule's middle) should hit that actor first,
/// unless something stands in between.
fn report(mut exit: MessageReader<AppExit>, vols: Res<HitVolumes>, blocks: Res<ActorBlocks>, mut phys: ResMut<PhysState>, scan: Option<Res<ScanLog>>, compare: Option<Res<ScanCompare>>) {
    if exit.read().last().is_none() {
        return;
    }
    if let Some(l) = scan.filter(|l| l.frames > 0) {
        let mean = |i: usize| l.total[i] as f64 / l.frames as f64;
        info!(
            "hitvol scan summary: {} frames, per frame on average matched={:.1} visible={:.1} accepted={:.1} actors={:.1}, at most matched={} visible={} accepted={} actors={}",
            l.frames, mean(0), mean(1), mean(2), mean(3), l.most.matched, l.most.visible, l.most.accepted, l.most.actors
        );
    }
    if let Some(c) = compare {
        info!(
            "hitvol compare summary: frames={} with_actors={} every_mesh_matched_most={} marked_matched_most={} left_out_most={} accepted_most={} missed={} mismatched={} volume_ids_wrong={}",
            c.frames, c.frames_with_actors, c.full_matched, c.marked_matched, c.left_out, c.accepted, c.missed, c.mismatched, c.volume_ids_wrong
        );
    }
    let ids: Vec<u32> = phys.world.actor_volumes().collect();
    let player = phys.player.as_ref().map(|c| c.feet);
    let (mut hit, mut other, mut none) = (0, 0, 0);
    for id in &ids {
        let Some(c) = phys.world.actor_aim_point(*id) else { continue };
        let (dx, dy) = player.map_or((1.0, 0.0), |p| {
            let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
            let l = (dx * dx + dy * dy).sqrt();
            if l > 1e-3 { (dx / l, dy / l) } else { (1.0, 0.0) }
        });
        let from = [c[0] + dx * 150.0, c[1] + dy * 150.0, c[2]];
        match phys.ray(from, [-dx, -dy, 0.0], 300.0, fnv_phys::Filter::default()) {
            Some(h) if matches!(h.owner, fnv_phys::Owner::Actor { form_id, layer, .. } if form_id == *id && layer == fnv_phys::layers::BIPED) => hit += 1,
            Some(_) => other += 1,
            None => none += 1,
        }
    }
    let with_parts = ids.iter().filter(|&&id| phys.world.actor_part_count(id) > 0).count();
    info!(
        "actor hit volumes: {} at exit ({with_parts} with body parts), at most {} at once; probe rays: {hit} hit their actor, {other} hit something else first, {none} hit nothing",
        vols.count, vols.most
    );
    // The block nearest to the player and how far the two are from touching.
    let nearest = phys.player.as_ref().and_then(|c| {
        let room = |b: &blocking::Block| b.room([c.feet[0], c.feet[1]], c.params.radius);
        blocks.list.iter().min_by(|a, b| room(a).total_cmp(&room(b))).map(|b| {
            format!("; the nearest, {:08X} round ({:.0}, {:.0}) to ({:.0}, {:.0}), is {:.1} units from touching the player at ({:.0}, {:.0}, {:.0})", b.actor, b.a[0], b.a[1], b.b[0], b.b[1], room(b), c.feet[0], c.feet[1], c.feet[2])
        })
    });
    info!(
        "actors in the player's way: {} at exit; they changed the player's step {} times, a walking actor's {} times{}",
        blocks.list.len(),
        blocks.steps_blocked,
        blocks.steps_actor_blocked,
        nearest.unwrap_or_default()
    );
    let [player, actors, pushed] = blocks.steps_pushing;
    // Those that stand in the way and may be pushed, nearest to the player first.
    let from = phys.player.as_ref().map_or([0.0; 2], |c| [c.feet[0], c.feet[1]]);
    let mut standing: Vec<(f32, u32, [f32; 2])> =
        blocks.list.iter().filter(|b| b.pushable).map(|b| ((b.a[0] - from[0]).hypot(b.a[1] - from[1]), b.actor, b.a)).collect();
    standing.sort_by(|a, b| a.0.total_cmp(&b.0));
    let nearest: Vec<String> = standing.iter().take(6).map(|(d, id, at)| format!("{id:08X} at ({:.0}, {:.0}), {d:.0} units away", at[0], at[1])).collect();
    info!(
        "actors pushed aside: {} may be pushed at exit{}{}; the player pushed one in {player} steps, a walking actor another in {actors}, one the player in {pushed}",
        standing.len(),
        if nearest.is_empty() { "" } else { ": " },
        nearest.join("; ")
    );
}

#[cfg(test)]
mod tests {
    /// #2701 round 3: an actor's draw-only copy (a full-detail ring copy drawn by a model
    /// swap) never feeds hit volumes or blocking, even when it carries the mark.
    #[test]
    fn draw_only_parts_make_no_hit_volume() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        // A dynamic reference (a placed actor's): a candidate by `SimState::may_be_actor`.
        world.insert_resource(crate::sim::SimState::over_placed(std::iter::empty()));
        let part = || (super::RefVisual(0xFF00_0009), super::Aabb::default(), GlobalTransform::IDENTITY, InheritedVisibility::VISIBLE);
        let actor = world.spawn(part()).id();
        // Marked before its DrawOnly came (another command): the consumer leaves it out.
        let late = world.spawn(part()).id();
        world.flush();
        world.entity_mut(late).insert(crate::DrawOnly);
        // Spawned with DrawOnly: never marked.
        let copy = world.spawn((part(), crate::DrawOnly)).id();
        world.flush();
        assert!(world.entity(actor).contains::<super::ActorBoundsCandidate>() && world.entity(late).contains::<super::ActorBoundsCandidate>());
        assert!(!world.entity(copy).contains::<super::ActorBoundsCandidate>());
        let n = world.run_system_once(|q: super::CandidateParts| q.iter().count()).unwrap();
        assert_eq!(n, 1, "only the actor's own mesh");
    }

    use super::*;

    #[test]
    fn volume_sync_and_optional_comparison_precede_activation() {
        for compare in [false, true] {
            let mut schedule = Schedule::default();
            schedule.configure_sets((crate::PlaySet::Physics, crate::PlaySet::Activation).chain());
            schedule.add_systems(crate::dismember::resolve.in_set(crate::PlaySet::Activation));
            schedule.add_systems(volume_systems(compare));
            schedule.initialize(&mut World::new()).unwrap();
            let systems: Vec<_> = schedule.systems().unwrap().map(|(key, s)| (key, s.name().to_string())).collect();
            let sync = systems.iter().position(|(_, name)| name.contains("sync_volumes")).unwrap();
            let resolve = systems.iter().position(|(_, name)| name.ends_with("dismember::resolve")).unwrap();
            assert_eq!(systems[sync].1.contains("compare_walks"), compare);
            assert!(sync < resolve, "comparison={compare}: {systems:?}");
        }
    }

    #[test]
    fn volume_from_bounds() {
        // A person about 128 tall, 60 wide (arms) and 30 deep.
        let v = volume_for([-30.0, -15.0, 0.0], [30.0, 15.0, 128.0], false);
        assert_eq!(v.feet, [0.0, 0.0, 0.0]);
        assert_eq!((v.radius, v.height), (15.0, 128.0));
        // A flat thing gets the minimum radius and height.
        let v = volume_for([0.0, 0.0, 5.0], [2.0, 2.0, 6.0], true);
        assert_eq!((v.radius, v.height, v.dead), (MIN_RADIUS, 2.0 * MIN_RADIUS, true));
    }

    #[test]
    fn limb_radii_scale_with_size() {
        // A 128-unit human: a torso about 26 across, arms about 11.
        assert!((limb_radius(Limb::Torso, 128.0) - 12.8).abs() < 0.01);
        assert!(limb_radius(Limb::Arm, 128.0) < limb_radius(Limb::Leg, 128.0));
        assert_eq!(limb_radius(Limb::Head, 10.0), 2.0);
    }

    #[test]
    fn part_chains_end_at_their_joints() {
        let seg = |part, limb, a: [f32; 3], b: [f32; 3]| PartSegment { part, limb, a, b, radius: None };
        // A torso of two segments from z 70 to 110 (radius 12.8 at size 128), a head of one
        // from 112 to 128 (radius 8.32).
        let segs = [seg(1, Limb::Torso, [0.0, 0.0, 70.0], [0.0, 0.0, 90.0]), seg(1, Limb::Torso, [0.0, 0.0, 90.0], [0.0, 0.0, 110.0]), seg(0, Limb::Head, [0.0, 0.0, 112.0], [0.0, 0.0, 128.0])];
        let caps = part_capsules(&segs, 128.0);
        assert_eq!(caps.len(), 3);
        // The torso's top end moves in by the radius; its lower end and the joint between
        // stay.
        assert!(caps[0].a[2] == 70.0 && caps[0].b[2] == 90.0);
        assert!(caps[1].a[2] == 90.0 && (caps[1].b[2] - 97.2).abs() < 1e-3);
        // The torso's top cap ends at 110; the head no longer starts inside it. A short
        // segment collapses to its middle.
        assert!((caps[1].b[2] + caps[1].radius - 110.0).abs() < 1e-3);
        assert_eq!((caps[2].a[2], caps[2].b[2]), (120.0, 120.0));
    }

    #[test]
    fn a_side_branch_is_a_chain_of_its_own() {
        let seg = |a: [f32; 3], b: [f32; 3]| PartSegment { part: 0, limb: Limb::Torso, a, b, radius: None };
        // A body along the abdomen and a leg branching from its start.
        let segs = [seg([0.0, 0.0, 100.0], [0.0, -20.0, 100.0]), seg([0.0, -20.0, 100.0], [0.0, -40.0, 100.0]), seg([0.0, 0.0, 100.0], [0.0, 0.0, 70.0])];
        let caps = part_capsules(&segs, 100.0);
        let r = limb_radius(Limb::Torso, 100.0);
        // The abdomen's far end is pulled in; the leg is its own chain: its far end too.
        assert!((caps[1].b[1] + 40.0 - r).abs() < 1e-3, "{caps:?}");
        assert!((caps[2].b[2] - 70.0 - r).abs() < 1e-3, "{caps:?}");
        assert_eq!(caps[2].a, [0.0, 0.0, 100.0]);
    }

    #[test]
    fn a_wider_ragdoll_shape_thickens_the_capsule_and_a_thinner_one_does_not() {
        // Mister Gutsy's body: its ragdoll capsule 21.2 against the share's 14.6 at size
        // 146; a human's arm shape (1.9) leaves the share.
        let seg = |limb, radius| PartSegment { part: 0, limb, a: [0.0, 0.0, 60.0], b: [0.0, 0.0, 100.0], radius };
        let wide = part_capsules(&[seg(Limb::Torso, Some(21.2))], 146.0);
        assert!((wide[0].radius - 21.2).abs() < 1e-4, "{wide:?}");
        let thin = part_capsules(&[seg(Limb::Arm, Some(1.9))], 128.0);
        assert!((thin[0].radius - limb_radius(Limb::Arm, 128.0)).abs() < 1e-4, "{thin:?}");
        assert_eq!(part_capsules(&[seg(Limb::Arm, None)], 128.0)[0].radius, thin[0].radius);
    }

    // The mark on meshes (#1388): which entities have it, and that the marked walk finds
    // what a walk over every `RefVisual` finds. `ScanCompare` is on in every frame of `app`.

    use fnv_sim::refs::PlacedRef;
    use fnv_sim::{FormStore, RefTable, SimConfig, World as SimWorld};

    const NPC: u32 = 0x100;
    const CREATURE: u32 = 0x101;
    const LEVELED_NPC: u32 = 0x102;
    const LEVELED_CREATURE: u32 = 0x103;
    const ITEM: u32 = 0x104;
    const ACHR_REF: u32 = 0x1000;
    const ACRE_REF: u32 = 0x1001;
    const STATIC_REF: u32 = 0x1002;

    fn placed(id: u32, base: u32, sig: [u8; 4]) -> PlacedRef {
        PlacedRef {
            id: FormId(id),
            sig,
            edid: None,
            base: FormId(base),
            cell: None,
            stored_in: None,
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

    /// The simulation over these placements (reference, its record type).
    fn sim_of(placements: &[(u32, [u8; 4])]) -> SimState {
        let mut forms = FormStore::default();
        for (base, sig) in [(NPC, *b"NPC_"), (CREATURE, *b"CREA"), (LEVELED_NPC, *b"LVLN"), (LEVELED_CREATURE, *b"LVLC"), (ITEM, *b"MISC")] {
            forms.sigs.insert(FormId(base), sig);
        }
        let mut refs = RefTable::default();
        for &(id, sig) in placements {
            let base = if &sig == b"REFR" { ITEM } else { NPC };
            refs.refs.insert(FormId(id), placed(id, base, sig));
        }
        let forms: &'static FormStore = Box::leak(Box::new(forms));
        let refs: &'static RefTable = Box::leak(Box::new(refs));
        SimState { forms, refs, world: SimWorld::new(forms, refs, SimConfig::default()) }
    }

    fn sim() -> SimState {
        sim_of(&[(ACHR_REF, *b"ACHR"), (ACRE_REF, *b"ACRE"), (STATIC_REF, *b"REFR")])
    }

    fn created(sim: &mut SimState, base: u32) -> u32 {
        sim.world.place(FormId(base), None, [0.0; 3], [0.0; 3], 1).unwrap().0
    }

    /// The resources the walk needs and its systems, in a frame's order.
    fn app(sim: SimState) -> App {
        let mut app = App::new();
        app.insert_resource(sim)
            .init_resource::<PhysState>()
            .init_resource::<HitVolumes>()
            .init_resource::<ActorParts>()
            .init_resource::<ActorBlocks>()
            .init_resource::<ScanLog>()
            .init_resource::<ScanCompare>()
            .add_systems(First, reclassify_candidates)
            .add_systems(Update, (sync_volumes.pipe(compare_walks), log_scan).chain());
        app
    }

    fn the_box() -> Aabb {
        Aabb { center: Vec3::ZERO.into(), half_extents: Vec3::splat(20.0).into() }
    }

    fn placement(at: [f32; 3]) -> GlobalTransform {
        GlobalTransform::from_translation(space::to_bevy(at))
    }

    /// A shown mesh of reference `r` with bounds, at game point `at`.
    fn mesh(app: &mut App, r: u32, at: [f32; 3]) -> Entity {
        app.world_mut().spawn((RefVisual(r), the_box(), placement(at), InheritedVisibility::VISIBLE)).id()
    }

    fn marked(app: &App, e: Entity) -> bool {
        app.world().get::<ActorBoundsCandidate>(e).is_some()
    }

    fn volumes(app: &App) -> Vec<u32> {
        let mut v: Vec<u32> = app.world().resource::<PhysState>().world.actor_volumes().collect();
        v.sort_unstable();
        v
    }

    fn scan(app: &App) -> ScanCounts {
        app.world().resource::<HitVolumes>().scan
    }

    /// The marked walk has found what the walk over every mesh finds, in all frames so far.
    fn assert_walks_agree(app: &App) {
        let c = app.world().resource::<ScanCompare>();
        assert!(c.frames > 0);
        assert_eq!((c.missed, c.mismatched, c.volume_ids_wrong), (0, 0, 0), "{c:?}");
    }

    /// Nothing is left of an actor that had a volume.
    fn assert_all_cleared(app: &App) {
        let world = app.world();
        assert!(world.resource::<PhysState>().world.actor_volumes().next().is_none());
        assert!(world.resource::<PhysState>().actor_heights.is_empty());
        assert!(world.resource::<PhysState>().actor_block_radii.is_empty());
        assert!(world.resource::<ActorBlocks>().list.is_empty());
        let vols = world.resource::<HitVolumes>();
        assert!(vols.living.is_empty());
        assert_eq!(vols.count, 0);
    }

    fn assert_stands(app: &App, id: u32) {
        let world = app.world();
        assert!(world.resource::<PhysState>().world.actor_volumes().any(|v| v == id));
        assert!(world.resource::<PhysState>().actor_heights.contains_key(&id));
        assert!(world.resource::<ActorBlocks>().list.iter().any(|b| b.actor == id));
        assert!(world.resource::<HitVolumes>().living.iter().any(|l| l.0 == id));
    }

    #[test]
    fn creature_bbx_tracks_scale_visibility_and_removal_beside_heights() {
        let mut app = app(sim());
        let e = mesh(&mut app, ACRE_REF, [0.0; 3]);
        let person = mesh(&mut app, ACHR_REF, [200.0, 0.0, 0.0]);
        // Synthetic width and two independently composed scales [design, #2535].
        app.world_mut().entity_mut(e).insert(CreatureBbx { reference: ACRE_REF, half_width: 80.0 });
        for (base, reference) in [(0.6, 1.0), (1.0, 1.25), (0.5, 0.5)] {
            let root = GlobalTransform::from(Transform::from_scale(Vec3::splat(base))) * Transform::from_scale(Vec3::splat(reference));
            app.world_mut().entity_mut(e).insert(root);
            app.update();
            let phys = app.world().resource::<PhysState>();
            assert!((phys.actor_block_radii[&ACRE_REF] - 80.0 * base * reference).abs() < 1e-4);
            assert!(!phys.actor_block_radii.contains_key(&ACHR_REF));
            assert!(phys.actor_heights.contains_key(&ACRE_REF));
        }
        app.world_mut().entity_mut(e).insert(InheritedVisibility::HIDDEN);
        app.update();
        assert!(app.world().resource::<PhysState>().actor_block_radii.is_empty());
        app.world_mut().entity_mut(e).insert(InheritedVisibility::VISIBLE);
        app.update();
        assert!(app.world().resource::<PhysState>().actor_block_radii.contains_key(&ACRE_REF));
        app.world_mut().entity_mut(e).despawn();
        app.world_mut().entity_mut(person).despawn();
        app.update();
        assert_all_cleared(&app);
    }

    #[test]
    fn differently_scaled_creatures_keep_both_widths_when_their_characters_stop() {
        use crate::blocking::Block;
        use crate::phys::NpcBody;
        // Synthetic pair, offset render bounds and independent reference scales
        // [design, #2535]. Skeleton width/base scales supplied by the main session
        // [observed: FalloutNV.esm and Fallout - Meshes.bsa, fnvtool decode, 2026-11-15].
        let ids = [ACRE_REF, ACHR_REF];
        let feet = [[0.0; 3], [400.0, 0.0, 0.0]];
        for reference_scales in [[1.0, 1.0], [1.25, 0.9]] {
            let mut app = app(sim_of(&[(ids[0], *b"ACRE"), (ids[1], *b"ACRE")]));
            let radii = [70.56 * 0.6 * reference_scales[0], 70.56 * reference_scales[1]];
            for (i, base) in [0.6, 1.0].into_iter().enumerate() {
                let e = mesh(&mut app, ids[i], feet[i]);
                let root = GlobalTransform::from(Transform::from_translation(space::to_bevy(feet[i])).with_scale(Vec3::splat(base)))
                    * Transform::from_scale(Vec3::splat(reference_scales[i]));
                app.world_mut().entity_mut(e).insert((
                    CreatureBbx { reference: ids[i], half_width: 70.56 },
                    Aabb { center: Vec3::new(15.0, 25.0, 64.0).into(), half_extents: Vec3::new(20.0, 20.0, 64.0).into() },
                    root,
                ));
            }
            // Residents drops each stopped character independently; a carried attack
            // can make it walking again [design: residents::think/carry, #2535].
            for walking in [[true, true], [false, true], [true, false], [false, false], [true, true]] {
                {
                    let mut phys = app.world_mut().resource_mut::<PhysState>();
                    for i in 0..2 {
                        phys.remove_npc(ids[i]);
                        if walking[i] {
                            let ch = phys.world.add_character(ids[i], fnv_phys::CharacterParams::default(), feet[i]);
                            let standing_height = ch.params.height;
                            phys.npcs.insert(ids[i], NpcBody { ch, prev_feet: feet[i], velocity: [0.0; 2], jump: false, sink: 0.0, lift: Default::default(), standing_height, sneak_crouch: false });
                        }
                    }
                }
                app.update();
                let phys = app.world().resource::<PhysState>();
                let drawn = app.world().resource::<ActorBlocks>();
                let mut pair = drawn.list.clone();
                for n in phys.npcs.values() {
                    assert_eq!(n.ch.params.radius, 22.0);
                    pair.push(Block::character(n.ch.id, n.ch.feet, blocking::walking_radius(n.ch.params.radius, phys.actor_block_radii.get(&n.ch.id).copied()), n.ch.params.height, false));
                }
                assert_eq!(pair.len(), 2);
                for i in 0..2 {
                    let block = pair.iter().find(|b| b.actor == ids[i]).unwrap();
                    assert!((block.radius - radii[i]).abs() < 1e-4, "walking={walking:?}: {pair:?}");
                    assert_eq!((block.a, block.b), ([feet[i][0], feet[i][1]], [feet[i][0], feet[i][1]]));
                    let direction = if i == 0 { 1.0 } else { -1.0 };
                    for carried in [false, true] {
                        let mut blocks = pair.clone();
                        let mut pushes = Vec::new();
                        let v = if carried {
                            // Even a pushable standing target stops a carried lunge
                            // [design: blocking::clip_pushing_nobody].
                            for b in &mut blocks { b.pushable = true; }
                            blocking::clip_pushing_nobody(ids[i], [400.0 * direction, 0.0], feet[i], block.radius, 128.0, 1.0, &mut blocks)
                        } else {
                            blocking::clip(ids[i], [400.0 * direction, 0.0], feet[i], block.radius, 128.0, 1.0, &mut blocks, &mut pushes)
                        };
                        assert!((400.0 - v[0].abs() - radii.iter().sum::<f32>()).abs() < 1e-4, "walking={walking:?}, carried={carried}: {v:?}");
                        assert!(pushes.is_empty());
                    }
                }
                // An attack holds the standing block across frames [design:
                // sync_volumes/ActorBlocks::attacking]; it must hold the wide circle.
                let before = drawn.list.clone();
                app.world_mut().resource_mut::<ActorBlocks>().attacking.extend(ids);
                app.update();
                assert_eq!(app.world().resource::<ActorBlocks>().list, before);
            }
        }
    }

    #[test]
    fn standing_blocks_keep_their_shape_without_a_wider_creature_bbx() {
        // Synthetic ordinary NPC, missing BBX and narrow creature BBX [design, #2535].
        for width in [None, Some(3.0)] {
            let mut app = app(sim());
            let e = mesh(&mut app, ACRE_REF, [100.0, 0.0, 0.0]);
            mesh(&mut app, ACHR_REF, [200.0, 0.0, 0.0]);
            if let Some(half_width) = width {
                app.world_mut().entity_mut(e).insert(CreatureBbx { reference: ACRE_REF, half_width });
            }
            app.update();
            for (id, x) in [(ACRE_REF, 100.0), (ACHR_REF, 200.0)] {
                let expected = blocking::block_for(id, [x - 20.0, -20.0, -20.0], [x + 20.0, 20.0, 20.0], None);
                let blocks = app.world().resource::<ActorBlocks>();
                assert_eq!(blocks.list.iter().find(|b| b.actor == id), Some(&expected));
            }
        }
    }

    #[test]
    fn may_be_actor_holds_for_all_that_is_actor_accepts() {
        let mut sim = sim();
        let creations = [NPC, CREATURE, LEVELED_NPC, LEVELED_CREATURE].map(|b| created(&mut sim, b));
        let item = created(&mut sim, ITEM);
        for id in [ACHR_REF, ACRE_REF].into_iter().chain(creations) {
            assert!(sim.is_actor(id) && sim.may_be_actor(id), "{id:08X}");
        }
        // A created item is no actor, but its id stays a candidate: only `is_actor` says.
        assert!(!sim.is_actor(item) && sim.may_be_actor(item));
        // A placed reference that is not an actor; an id nothing placed or created.
        assert!(!sim.is_actor(STATIC_REF) && !sim.may_be_actor(STATIC_REF));
        assert!(!sim.is_actor(0x2000) && !sim.may_be_actor(0x2000));
        // A dynamic id with no state yet is a candidate; `is_actor` waits for its base.
        assert!(!sim.is_actor(0xFF00_FFFF) && sim.may_be_actor(0xFF00_FFFF));
    }

    #[test]
    fn the_mark_follows_the_reference_both_ways() {
        let mut app = app(sim());
        let e = mesh(&mut app, ACHR_REF, [0.0; 3]);
        assert!(marked(&app, e));
        // Replaced: actor to static to actor.
        app.world_mut().entity_mut(e).insert(RefVisual(STATIC_REF));
        assert!(!marked(&app, e));
        app.world_mut().entity_mut(e).insert(RefVisual(ACRE_REF));
        assert!(marked(&app, e));
        // Removed and given back; despawned with its mark.
        app.world_mut().entity_mut(e).remove::<RefVisual>();
        assert!(!marked(&app, e));
        app.world_mut().entity_mut(e).insert(RefVisual(ACHR_REF));
        assert!(marked(&app, e));
        let s = mesh(&mut app, STATIC_REF, [0.0; 3]);
        assert!(!marked(&app, s));
        app.world_mut().entity_mut(e).despawn();
        app.world_mut().entity_mut(s).despawn();
        app.update();
        assert_all_cleared(&app);
    }

    #[test]
    fn the_mark_is_there_at_the_sync_point_that_brings_the_reference() {
        use bevy::ecs::schedule::ApplyDeferred;
        let mut app = app(sim());
        let replaced = mesh(&mut app, ACHR_REF, [0.0; 3]);
        let taken = mesh(&mut app, ACHR_REF, [0.0; 3]);
        let mut schedule = Schedule::default();
        schedule.add_systems(
            (
                move |mut commands: Commands| {
                    commands.spawn(RefVisual(ACHR_REF));
                    commands.spawn(RefVisual(STATIC_REF));
                    // Several changes in one batch: the last one counts.
                    commands.entity(replaced).insert(RefVisual(STATIC_REF)).insert(RefVisual(ACRE_REF)).insert(RefVisual(STATIC_REF));
                    commands.entity(taken).remove::<RefVisual>().insert(RefVisual(ACHR_REF)).remove::<RefVisual>();
                },
                ApplyDeferred,
                |sim: Res<SimState>, refs: Query<(&RefVisual, Has<ActorBoundsCandidate>)>, stale: Query<(), (With<ActorBoundsCandidate>, Without<RefVisual>)>| {
                    assert_eq!(refs.iter().count(), 3);
                    for (r, has) in &refs {
                        assert_eq!(has, sim.may_be_actor(r.0), "{r:?}");
                    }
                    assert!(stale.is_empty());
                },
            )
                .chain(),
        );
        schedule.run(app.world_mut());
    }

    #[test]
    fn marks_made_before_the_simulation_are_set_when_it_arrives() {
        let mut app = App::new();
        app.add_systems(First, reclassify_candidates);
        let actor = mesh(&mut app, ACHR_REF, [0.0; 3]);
        let stat = mesh(&mut app, STATIC_REF, [0.0; 3]);
        // Without the simulation no volumes are kept, and nothing is spent on marks.
        assert!(!marked(&app, actor) && !marked(&app, stat));
        app.update();
        assert!(!marked(&app, actor) && !marked(&app, stat));
        app.insert_resource(sim());
        app.update();
        assert!(marked(&app, actor) && !marked(&app, stat));
        // Nothing changes while the table stays.
        app.world_mut().entity_mut(actor).remove::<ActorBoundsCandidate>();
        app.update();
        assert!(!marked(&app, actor));
        // Another simulation with other placements: what is an actor now counts.
        app.insert_resource(sim_of(&[(ACHR_REF, *b"REFR"), (STATIC_REF, *b"ACHR")]));
        app.update();
        assert!(!marked(&app, actor) && marked(&app, stat));
    }

    #[test]
    fn the_marked_walk_finds_the_actors_and_only_candidates() {
        let mut sim = sim();
        let creations = [NPC, CREATURE, LEVELED_NPC, LEVELED_CREATURE].map(|b| created(&mut sim, b));
        let item = created(&mut sim, ITEM);
        let mut app = app(sim);
        // Actors far apart, each of two meshes; a static, a created item.
        let actors = [ACHR_REF, ACRE_REF].into_iter().chain(creations).collect::<Vec<_>>();
        for (i, &id) in actors.iter().enumerate() {
            let at = [i as f32 * 1000.0, 0.0, 0.0];
            mesh(&mut app, id, at);
            mesh(&mut app, id, [at[0], 0.0, 100.0]);
        }
        let stat = mesh(&mut app, STATIC_REF, [0.0, 500.0, 0.0]);
        let thing = mesh(&mut app, item, [0.0, 800.0, 0.0]);
        // One hidden actor mesh and one that has no bounds yet add nothing but a count.
        let hidden = mesh(&mut app, ACHR_REF, [0.0, 0.0, 5000.0]);
        app.world_mut().entity_mut(hidden).insert(InheritedVisibility::HIDDEN);
        let unbound = app.world_mut().spawn((RefVisual(ACHR_REF), placement([0.0, 0.0, -5000.0]), InheritedVisibility::VISIBLE)).id();
        app.update();
        assert!(!marked(&app, stat) && marked(&app, thing) && marked(&app, unbound));
        // Twelve actor meshes, the hidden one and the created item's walked; the static's
        // and the one without bounds not.
        assert_eq!(scan(&app), ScanCounts { matched: 14, visible: 13, accepted: 12, actors: 6 });
        assert_eq!(volumes(&app), { let mut a = actors.clone(); a.sort_unstable(); a });
        assert_walks_agree(&app);
        let c = app.world().resource::<ScanCompare>();
        assert_eq!((c.full_matched, c.marked_matched, c.left_out, c.accepted), (15, 14, 1, 12));
        // The bounds are the union of the two shown boxes, the hidden one not in them.
        let (lo, hi) = (game_box(&the_box(), &placement([0.0; 3])), game_box(&the_box(), &placement([0.0, 0.0, 100.0])));
        let height = app.world().resource::<PhysState>().actor_heights[&ACHR_REF];
        assert_eq!(height, hi.1[2] - lo.0[2]);
        // A ray at the actor hits it.
        let mut phys = app.world_mut().resource_mut::<PhysState>();
        let c = phys.world.actor_aim_point(ACRE_REF).unwrap();
        let hit = phys.ray([c[0] + 150.0, c[1], c[2]], [-1.0, 0.0, 0.0], 300.0, fnv_phys::Filter::default());
        assert!(matches!(hit.map(|h| h.owner), Some(fnv_phys::Owner::Actor { form_id, .. }) if form_id == ACRE_REF));
    }

    #[test]
    fn a_mesh_counts_on_any_frame_it_is_shown_with_bounds() {
        let mut app = app(sim());
        // Hidden at first, then shown, without touching its reference.
        let e = mesh(&mut app, ACHR_REF, [0.0; 3]);
        app.world_mut().entity_mut(e).insert(InheritedVisibility::HIDDEN);
        app.update();
        assert_eq!(scan(&app), ScanCounts { matched: 1, visible: 0, accepted: 0, actors: 0 });
        assert_all_cleared(&app);
        app.world_mut().entity_mut(e).insert(InheritedVisibility::VISIBLE);
        app.update();
        assert_stands(&app, ACHR_REF);
        // Its bounds arrive late (they are made after the frame that spawns the mesh).
        let late = app.world_mut().spawn((RefVisual(ACRE_REF), placement([900.0, 0.0, 0.0]), InheritedVisibility::VISIBLE)).id();
        app.update();
        assert_eq!(volumes(&app), [ACHR_REF]);
        app.world_mut().entity_mut(late).insert(the_box());
        app.update();
        assert_eq!(volumes(&app), [ACHR_REF, ACRE_REF]);
        // Frames in which nothing changes keep them: the walk looks at no change ticks.
        for _ in 0..3 {
            app.update();
            assert_eq!(volumes(&app), [ACHR_REF, ACRE_REF]);
            assert_eq!(scan(&app).accepted, 2);
        }
        // Hidden again, the actor goes.
        app.world_mut().entity_mut(e).insert(InheritedVisibility::HIDDEN);
        app.update();
        assert_eq!(volumes(&app), [ACRE_REF]);
        assert_walks_agree(&app);
    }

    #[test]
    fn a_created_reference_is_read_each_frame_under_a_mark_that_stays() {
        let mut sim = sim();
        let id = created(&mut sim, NPC);
        let mut app = app(sim);
        let e = mesh(&mut app, id, [0.0; 3]);
        app.update();
        assert_stands(&app, id);
        // Its base changes, its `RefVisual` does not: it is walked and no longer accepted.
        app.world_mut().resource_mut::<SimState>().world.get(FormId(id)).unwrap().base = FormId(ITEM);
        app.update();
        assert!(marked(&app, e));
        assert_eq!(scan(&app), ScanCounts { matched: 1, visible: 1, accepted: 0, actors: 0 });
        assert_all_cleared(&app);
        app.world_mut().resource_mut::<SimState>().world.get(FormId(id)).unwrap().base = FormId(CREATURE);
        app.update();
        assert_stands(&app, id);
        assert_walks_agree(&app);
    }

    #[test]
    fn with_no_meshes_left_the_volumes_blocks_living_and_heights_clear() {
        let mut app = app(sim());
        let e = mesh(&mut app, ACHR_REF, [0.0; 3]);
        app.update();
        assert_stands(&app, ACHR_REF);
        assert_eq!(app.world().resource::<HitVolumes>().most, 1);
        // The query is empty now; its emptiness is what must clear them.
        app.world_mut().entity_mut(e).despawn();
        app.update();
        assert_eq!(scan(&app), ScanCounts::default());
        assert_all_cleared(&app);
        // The exit report's most stays.
        assert_eq!(app.world().resource::<HitVolumes>().most, 1);
        // Back again, by a new entity, as an actor re-entering a cell.
        let e = mesh(&mut app, ACHR_REF, [0.0; 3]);
        app.update();
        assert_stands(&app, ACHR_REF);
        // Its only mesh loses its reference.
        app.world_mut().entity_mut(e).remove::<RefVisual>();
        app.update();
        assert_all_cleared(&app);
        assert_walks_agree(&app);
    }

    #[test]
    fn a_disabled_or_deleted_actor_loses_its_volume_and_comes_back() {
        let mut app = app(sim());
        let e = mesh(&mut app, ACHR_REF, [0.0; 3]);
        app.update();
        assert_stands(&app, ACHR_REF);
        app.world_mut().resource_mut::<SimState>().world.set_enabled(FormId(ACHR_REF), false).unwrap();
        app.update();
        // Accepted (shown, an actor), then left out: not in the world.
        assert_eq!(scan(&app), ScanCounts { matched: 1, visible: 1, accepted: 1, actors: 0 });
        assert_all_cleared(&app);
        app.world_mut().resource_mut::<SimState>().world.set_enabled(FormId(ACHR_REF), true).unwrap();
        app.update();
        assert_stands(&app, ACHR_REF);
        app.world_mut().resource_mut::<SimState>().world.set_deleted(FormId(ACHR_REF), true).unwrap();
        app.update();
        assert_all_cleared(&app);
        assert!(marked(&app, e));
        assert_walks_agree(&app);
    }

    #[test]
    fn a_ragdoll_takes_over_from_the_volume_and_gives_it_back() {
        use fnv_collide::ragdoll::{Ragdoll, RagdollBody};
        use fnv_collide::{CollisionObject, Iso, Motion, Shape};
        let object = CollisionObject {
            node: Some("Bip01".into()),
            node_block: 0,
            node_transform: Iso::IDENTITY,
            body_transform: Iso::IDENTITY,
            motion: Motion::Keyframed,
            layer: fnv_phys::layers::BIPED,
            mass: 1.0,
            friction: 0.3,
            restitution: 0.8,
            shape: Shape::Box { half_extents: [5.0; 3] }.into(),
            shell: 0.0,
            material: None,
            wind: false,
        };
        let body = RagdollBody { object, linear_damping: 0.1, angular_damping: 0.05, stored: Iso::IDENTITY, unit: 7.0, inertia: None };
        let ragdoll = std::sync::Arc::new(Ragdoll { bodies: vec![body], joints: vec![], problems: vec![] });
        let mut app = app(sim());
        mesh(&mut app, ACHR_REF, [0.0; 3]);
        app.update();
        assert_stands(&app, ACHR_REF);
        let nodes = [Iso::IDENTITY];
        let start = fnv_phys::RagdollStart { form_id: ACHR_REF, ragdoll: &ragdoll, scale: 1.0, nodes: &nodes, velocity: [0.0; 3], parts: &[], asleep: false };
        app.world_mut().resource_mut::<PhysState>().world.add_ragdoll(&start);
        app.update();
        assert_eq!(scan(&app), ScanCounts { matched: 1, visible: 1, accepted: 1, actors: 0 });
        assert_all_cleared(&app);
        assert!(app.world_mut().resource_mut::<PhysState>().world.remove_ragdoll(ACHR_REF));
        app.update();
        assert_stands(&app, ACHR_REF);
        assert_walks_agree(&app);
    }

    #[test]
    fn the_children_of_a_root_fading_at_a_door_still_count() {
        let mut app = app(sim());
        let root = app.world_mut().spawn(RefVisual(ACHR_REF)).id();
        let child = mesh(&mut app, ACHR_REF, [0.0; 3]);
        app.world_mut().entity_mut(child).insert(ChildOf(root));
        app.update();
        assert_stands(&app, ACHR_REF);
        // residents.rs takes the root's `RefVisual` as the fade starts, not its children's.
        app.world_mut().entity_mut(root).remove::<RefVisual>();
        app.update();
        assert!(!marked(&app, root) && marked(&app, child));
        assert_stands(&app, ACHR_REF);
        assert_walks_agree(&app);
        // A child without a reference of its own never counted: the mark is not inherited.
        let extra = app.world_mut().spawn((the_box(), placement([0.0, 0.0, 900.0]), InheritedVisibility::VISIBLE, ChildOf(child))).id();
        app.update();
        assert!(!marked(&app, extra));
        assert_eq!(scan(&app).accepted, 1);
    }

    #[test]
    fn the_comparison_reports_a_mesh_without_its_mark() {
        let mut app = app(sim());
        let e = mesh(&mut app, ACHR_REF, [0.0; 3]);
        mesh(&mut app, ACRE_REF, [900.0, 0.0, 0.0]);
        app.update();
        assert_walks_agree(&app);
        // No code but the hooks does this: here it shows the comparison can fail.
        app.world_mut().entity_mut(e).remove::<ActorBoundsCandidate>();
        app.update();
        let c = app.world().resource::<ScanCompare>();
        assert_eq!((c.missed, c.mismatched, c.frames, c.frames_with_actors), (1, 1, 2, 2));
        assert_eq!(volumes(&app), [ACRE_REF]);
    }

    #[test]
    fn the_comparison_goes_by_the_ids_physics_has() {
        let meshes = BTreeMap::from([(ACHR_REF, MeshCounts::default()), (ACRE_REF, MeshCounts::default())]);
        let mut c = ScanCompare { kept: Bounds::from([(ACHR_REF, ([0.0; 3], [1.0; 3]))]), meshes, ..Default::default() };
        // Physics has another actor than the one kept: counted, and "has a volume" is physics'.
        c.check_volumes([ACRE_REF].into_iter());
        assert_eq!(c.volume_ids_wrong, 1);
        assert_eq!(c.shown, BTreeMap::from([(ACHR_REF, (0, 0, false)), (ACRE_REF, (0, 0, true))]));
        c.check_volumes([ACHR_REF].into_iter());
        assert_eq!(c.volume_ids_wrong, 1);
        assert_eq!(c.shown, BTreeMap::from([(ACHR_REF, (0, 0, true)), (ACRE_REF, (0, 0, false))]));
    }

    #[test]
    fn the_walk_stands_alone_without_the_comparison() {
        let mut app = App::new();
        app.insert_resource(sim())
            .init_resource::<PhysState>()
            .init_resource::<HitVolumes>()
            .init_resource::<ActorParts>()
            .init_resource::<ActorBlocks>()
            .add_systems(Update, sync_volumes);
        mesh(&mut app, ACHR_REF, [0.0; 3]);
        app.update();
        assert_stands(&app, ACHR_REF);
    }

    #[test]
    fn the_comparison_follows_each_actors_meshes_and_volume() {
        let shown = |app: &App| app.world().resource::<ScanCompare>().shown.clone();
        let moved = |app: &App| {
            let c = app.world().resource::<ScanCompare>().meshes[&ACHR_REF];
            (c.new, c.gone)
        };
        let mut app = app(sim());
        let a = mesh(&mut app, ACHR_REF, [0.0; 3]);
        app.update();
        assert_eq!(shown(&app), BTreeMap::from([(ACHR_REF, (1, 1, true))]));
        assert_eq!(moved(&app), (1, 0));
        let b = mesh(&mut app, ACHR_REF, [0.0, 0.0, 100.0]);
        app.update();
        assert_eq!(shown(&app), BTreeMap::from([(ACHR_REF, (2, 2, true))]));
        assert_eq!(moved(&app), (1, 0));
        app.update();
        assert_eq!(moved(&app), (0, 0));
        // A hidden mesh counts among all of them, in no bounds. Taken away, only that
        // number changes, as a rifle that is hidden leaving the actor.
        let hidden = mesh(&mut app, ACHR_REF, [0.0, 0.0, 900.0]);
        app.world_mut().entity_mut(hidden).insert(InheritedVisibility::HIDDEN);
        app.update();
        assert_eq!(shown(&app), BTreeMap::from([(ACHR_REF, (2, 3, true))]));
        app.world_mut().entity_mut(hidden).despawn();
        app.update();
        assert_eq!(shown(&app), BTreeMap::from([(ACHR_REF, (2, 2, true))]));
        assert_eq!(moved(&app), (0, 1));
        // One replaced within a frame: both are counted though the numbers stay.
        app.world_mut().entity_mut(b).despawn();
        let b = mesh(&mut app, ACHR_REF, [0.0, 0.0, 100.0]);
        app.update();
        assert_eq!(shown(&app), BTreeMap::from([(ACHR_REF, (2, 2, true))]));
        assert_eq!(moved(&app), (1, 1));
        // Its meshes are shown and accepted, the actor is out of the world.
        app.world_mut().resource_mut::<SimState>().world.set_enabled(FormId(ACHR_REF), false).unwrap();
        app.update();
        assert_eq!(shown(&app), BTreeMap::from([(ACHR_REF, (2, 2, false))]));
        app.world_mut().entity_mut(a).despawn();
        app.world_mut().entity_mut(b).despawn();
        app.update();
        assert!(shown(&app).is_empty());
        assert_walks_agree(&app);
    }

    #[test]
    fn the_scan_log_adds_up_the_counts() {
        let mut app = app(sim());
        mesh(&mut app, ACHR_REF, [0.0; 3]);
        mesh(&mut app, STATIC_REF, [0.0; 3]);
        app.update();
        app.update();
        let l = app.world().resource::<ScanLog>();
        assert_eq!((l.frames, l.total), (2, [2, 2, 2, 2]));
        assert_eq!(l.most, ScanCounts { matched: 1, visible: 1, accepted: 1, actors: 1 });
    }
}
