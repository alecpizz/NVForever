//! Ragdolls (docs/spec-notes/ragdoll.md §4): the jointed bodies of a skeleton
//! (`fnv_collide::ragdoll`) as dynamic rapier bodies held together by impulse joints, so a
//! dead actor falls and comes to rest under gravity and contacts.
//!
//! - **Bodies.** One dynamic body per ragdoll body, started at its bone's pose, with the
//!   stored mass, inertia tensor (an actor's) and damping, and continuous collision
//!   detection (a thin limb must not pass through a floor). Their colliders carry
//!   [`Owner::Actor`] on the DEAD_BIP layer, so rays find the corpse as they found its
//!   hit volume.
//! - **Joints.** The stored constraints with their limits ([`joint`]): a ragdoll
//!   constraint is a ball joint limited about its twist axis, with two more joints for its
//!   cone and its plane cones ([`cone_joints`]); a hinge is a revolute joint [design].
//! - **Contacts** [design]: with the world (terrain, references, other ragdolls); not
//!   between two small ragdolls ([`SMALL_RAGDOLL_SPAN`]: a heap of small corpses would
//!   cost the solver for nothing the eye sees; hypothesis that the original allows it); not
//!   with characters (they walk through corpses as they do through hit volumes); between
//!   two bodies of one ragdoll only when they are not joined and do not touch in the bind
//!   pose (so an arm rests on the chest instead of inside it, and overlapping neighbours
//!   do not fight their joints).
//!
//! - **Rest** [design]: a ragdoll that has come to rest is held with fixed bodies until a
//!   hit or a blast ([`PhysWorld::set_ragdoll_at_rest`]). Before that the game settles it
//!   ([`PhysWorld::settle_ragdoll`]: damping raised over a fraction of a second or two), so
//!   that a ragdoll still moving is not stopped dead.
//!
//! Poses in and out are those of the bodies' **bones** (nodes), game units.

use crate::convert::{self, arr3, vec3, ConvertError};
use crate::world::{body_state, BodyState, Owner, PhysWorld};
use crate::layers;
use fnv_collide::ragdoll::{cross, dot, unit, JointKind, Ragdoll, RagdollJoint};
use fnv_collide::Iso;
use rapier3d::math::{Matrix, Pose, Rotation};
use rapier3d::parry::query;
use rapier3d::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Largest friction given to a ragdoll collider [design: some stored values (10 on the
/// human pelvis) would glue a body to a slope].
pub const MAX_FRICTION: f32 = 1.0;

/// How fast a joint's relative turning dies out, per second [design: stands in for the
/// constraints' stored `max_friction`, whose unit is unknown; without it limbs swing
/// like chains].
pub const JOINT_DAMPING: f32 = 6.0;

/// The damping (per second, on each body's linear and angular velocity, on top of its
/// own) a settling ragdoll has at the end of its settle ([`PhysWorld::settle_ragdoll`])
/// [design, 2026-11-11: raised along a settle of `T` seconds from 0 to this, free motion
/// left at its end is about `exp(-15·T/2)` of what it was (2 % after half a second, under
/// a thousandth after one), so holding the ragdoll at rest then shows no stop. rapier
/// damps each body once a step, after the constraint solve, so a turn the joints and
/// contacts drive anew every step (a light limb shivering against a limit) keeps most of
/// its speed [observed: smoke `mantis-egg-spawn`, antennae and legs at 0.3-3 rad/s when
/// held; raising the joints' damping motors as well did not lower it]].
pub const SETTLE_DAMPING: f32 = 15.0;

/// The most a push turns the body it strikes, radians per second, and the shortest arm
/// (game units) its turning is worked out with [design].
pub const MAX_PUSH_SPIN: f32 = 5.0;
pub const PUSH_ARM: f32 = 6.0;

/// A twist or hinge range is kept inside ±this many radians: rapier measures the angle
/// within (−π, π], so a limit at π could never be reached. No stored twist or hinge limit
/// of the human skeleton comes near it (the widest, an elbow's, is −2.27) [observed].
const MAX_LIMIT: f32 = 3.0;

/// Bodies of one ragdoll closer than this in the bind pose never collide with each
/// other, game units [design].
pub const SELF_COLLISION_GAP: f32 = 1.0;

/// A ragdoll whose bodies' shapes fit inside a box this long on every side in the bind
/// pose (game units, actor scale included) is small: two small ragdolls of different
/// actors pass through each other, while each still meets the world, loose objects and
/// every bigger ragdoll [design: 64 units holds rats, geckos, radroaches, bloatflies and
/// mantis nymphs and leaves people, dogs and anything bigger colliding; hypothesis: the
/// original's rule for corpse against corpse is not known].
pub const SMALL_RAGDOLL_SPAN: f32 = 64.0;

/// How far past its limits (radians) a placed object's joint may start and still keep
/// them ([`PhysWorld::add_object_ragdoll`]) [design].
pub const START_SLACK: f32 = 0.05;

/// How to start a ragdoll.
#[derive(Debug, Clone, Copy)]
pub struct RagdollStart<'a> {
    /// The actor reference it belongs to.
    pub form_id: u32,
    pub ragdoll: &'a Arc<Ragdoll>,
    /// The actor's uniform scale (shapes and pivots are multiplied by it).
    pub scale: f32,
    /// The pose of each body's bone, in `ragdoll.bodies` order.
    pub nodes: &'a [Iso],
    /// Starting velocity of every body, game units per second.
    pub velocity: [f32; 3],
    /// The body part each body's colliders report (`Owner::Actor::part`); missing
    /// entries are [`crate::actors::WHOLE`].
    pub parts: &'a [u8],
    /// Start at rest (a corpse put back where it lay): see
    /// [`PhysWorld::set_ragdoll_at_rest`].
    pub asleep: bool,
}

/// What [`PhysWorld::add_ragdoll`] built.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RagdollReport {
    pub bodies: usize,
    pub joints: usize,
    /// Pairs of bodies of the ragdoll that collide with each other.
    pub self_pairs: usize,
    pub errors: Vec<ConvertError>,
    /// A placed object's joints built without their limits, by stored index: they
    /// started outside them.
    pub freed: Vec<usize>,
}

/// What a ragdoll touches ([`PhysWorld::ragdoll_contacts`]): contact pairs by what the
/// other collider belongs to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RagdollContacts {
    /// The terrain's heightfields.
    pub terrain: ContactTally,
    /// References whose body is not dynamic (buildings, rocks, furniture).
    pub fixed: ContactTally,
    /// References whose body is dynamic (loose objects).
    pub loose: ContactTally,
    /// Other actors' ragdolls that move, and those held at rest.
    pub moving_ragdolls: ContactTally,
    pub resting_ragdolls: ContactTally,
    /// Two of its own bodies.
    pub own: ContactTally,
    /// Anything else (gore pieces).
    pub other: ContactTally,
    /// The other actors' ragdolls it touches, by FormID, ascending.
    pub ragdolls: Vec<u32>,
    /// The references (fixed and loose) it touches, by FormID, ascending.
    pub refs: Vec<u32>,
}

/// Contact pairs of one kind, and the deepest overlap among them (game units, 0 when
/// none overlaps).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ContactTally {
    pub pairs: usize,
    pub deepest: f32,
}

impl RagdollContacts {
    /// For a log line: `terrain 3 (0.4 deep), fixed 0, loose 0, moving ragdolls 2 (1.2 deep), resting ragdolls 0, own 1 (0.0 deep), other 0, touching [00012345], refs [0001ABCD]`.
    pub fn summary(&self) -> String {
        let t = |name: &str, c: &ContactTally| if c.pairs == 0 { format!("{name} 0") } else { format!("{name} {} ({:.1} deep)", c.pairs, c.deepest) };
        let ids: Vec<String> = self.ragdolls.iter().map(|f| format!("{f:08X}")).collect();
        format!(
            "{}, {}, {}, {}, {}, {}, {}, touching [{}], refs [{}]",
            t("terrain", &self.terrain),
            t("fixed", &self.fixed),
            t("loose", &self.loose),
            t("moving ragdolls", &self.moving_ragdolls),
            t("resting ragdolls", &self.resting_ragdolls),
            t("own", &self.own),
            t("other", &self.other),
            ids.join(" "),
            self.refs.iter().map(|f| format!("{f:08X}")).collect::<Vec<_>>().join(" ")
        )
    }
}

/// A ragdoll in the world.
#[derive(Debug, Clone)]
pub(crate) struct RagdollBodies {
    /// Per ragdoll body; `None` when its shape could not be converted.
    pub(crate) bodies: Vec<Option<RigidBodyHandle>>,
    /// Per body, its frame in its bone's (the body transform, scaled).
    local: Vec<Iso>,
    /// Per body, the other bodies it collides with (bit per body index).
    collide: Vec<u64>,
    /// Held at rest: its bodies are fixed until a push or a blast
    /// ([`PhysWorld::set_ragdoll_at_rest`]).
    at_rest: bool,
    /// What it was built from, to build it again at rest or roused.
    pub(crate) data: Arc<Ragdoll>,
    scale: f32,
    parts: Vec<u8>,
    /// Per stored joint, its joints in the world (the joint and a ragdoll constraint's
    /// [`cone_joints`]; none at rest, or when cut).
    joints: Vec<Vec<ImpulseJointHandle>>,
    /// Joints cut by dismemberment ([`PhysWorld::cut_ragdoll`]), by stored index: never
    /// built again.
    cut: Vec<usize>,
    /// Smaller than [`SMALL_RAGDOLL_SPAN`]: no contacts with other small ragdolls.
    small: bool,
    /// Settling ([`PhysWorld::settle_ragdoll`]): the share of [`SETTLE_DAMPING`] its
    /// bodies have on top of their own; `None` when it is not settling.
    settle: Option<f32>,
    /// A placed object's jointed bodies (a swinging trap, [`PhysWorld::add_object_ragdoll`]),
    /// not an actor's: its colliders report the reference ([`Owner::Ref`]) and its
    /// keyframed and static bodies are fixed.
    object: bool,
}

/// Contact filtering for ragdoll colliders (see the module notes).
pub(crate) struct RagdollHooks<'a> {
    pub ragdolls: &'a BTreeMap<u32, RagdollBodies>,
    pub of_body: &'a HashMap<RigidBodyHandle, (u32, u16)>,
    /// References that pass through one actor's ragdoll ([`PhysWorld::set_ref_passes_ragdoll`]).
    pub passes: &'a HashMap<u32, u32>,
}

impl PhysicsHooks for RagdollHooks<'_> {
    fn filter_contact_pair(&self, c: &PairFilterContext) -> Option<SolverFlags> {
        let rag = |b: Option<RigidBodyHandle>| b.and_then(|h| self.of_body.get(&h)).copied();
        let owner = |h: ColliderHandle| c.colliders.get(h).and_then(|co| Owner::decode(co.user_data));
        // Gore pieces (`crate::gore`) meet the world and each other only.
        let (o1, o2) = (owner(c.collider1), owner(c.collider2));
        if PhysWorld::is_gore(o1) || PhysWorld::is_gore(o2) {
            let other = if PhysWorld::is_gore(o1) { o2 } else { o1 };
            let alive_or_corpse = matches!(other, Some(Owner::Character { .. } | Owner::Actor { .. })) && !PhysWorld::is_gore(other);
            return (!alive_or_corpse).then_some(SolverFlags::COMPUTE_RIGID_IMPULSES);
        }
        match (rag(c.rigid_body1), rag(c.rigid_body2)) {
            (Some((fa, ia)), Some((fb, ib))) if fa == fb => {
                let row = self.ragdolls.get(&fa).and_then(|r| r.collide.get(ia as usize)).copied().unwrap_or(0);
                (ib < 64 && row & (1 << ib) != 0).then_some(SolverFlags::COMPUTE_RIGID_IMPULSES)
            }
            (Some((fa, _)), Some((fb, _))) => {
                let small = |f: u32| self.ragdolls.get(&f).is_some_and(|r| r.small);
                (!(small(fa) && small(fb))).then_some(SolverFlags::COMPUTE_RIGID_IMPULSES)
            }
            (Some((fa, _)), None) => self.meets(fa, owner(c.collider2)).then_some(SolverFlags::COMPUTE_RIGID_IMPULSES),
            (None, Some((fb, _))) => self.meets(fb, owner(c.collider1)).then_some(SolverFlags::COMPUTE_RIGID_IMPULSES),
            (None, None) => Some(SolverFlags::COMPUTE_RIGID_IMPULSES),
        }
    }
}

impl RagdollHooks<'_> {
    /// Whether actor `rag`'s ragdoll has contacts with a collider of `other`: not with
    /// characters or actor hit volumes, nor with a reference set to pass through it.
    fn meets(&self, rag: u32, other: Option<Owner>) -> bool {
        match other {
            Some(Owner::Character { .. } | Owner::Actor { .. }) => false,
            Some(Owner::Ref { form_id, .. }) => self.passes.get(&form_id) != Some(&rag),
            _ => true,
        }
    }
}

/// A bone or body pose whose rotation may have drifted from orthonormal (it comes from
/// an animated skeleton): the nearest rotation is used.
fn loose_pose(iso: &Iso, k: f32) -> Option<Pose> {
    let m = Matrix::from_cols_array(&iso.rotation).transpose();
    let q = Rotation::from_mat3(&m).normalize();
    let t = vec3(iso.translation) * k;
    (q.is_finite() && t.is_finite() && m.determinant() > 0.0).then(|| Pose::from_parts(t, q))
}

fn iso_of(p: &Pose, k: f32) -> Iso {
    Iso { translation: arr3(p.translation / k), rotation: Matrix::from_quat(p.rotation).transpose().to_cols_array() }
}

/// The rotation whose x axis is `x` and whose y axis is `y` made square to it.
fn frame(x: [f32; 3], y: [f32; 3]) -> Option<Rotation> {
    let x = unit(x)?;
    let d = dot(x, y);
    let y = unit([y[0] - d * x[0], y[1] - d * x[1], y[2] - d * x[2]])?;
    let z = cross(x, y);
    Some(Rotation::from_mat3(&Matrix::from_cols(vec3(x), vec3(y), vec3(z))).normalize())
}

/// Limit an angular axis to `[lo, hi]`, or lock it when the range is empty.
fn limit(j: &mut GenericJoint, axis: JointAxis, mask: JointAxesMask, lo: f32, hi: f32) {
    let (lo, hi) = (lo.clamp(-MAX_LIMIT, MAX_LIMIT), hi.clamp(-MAX_LIMIT, MAX_LIMIT));
    if !(lo.is_finite() && hi.is_finite()) {
        return;
    }
    if hi - lo < 1e-3 {
        j.lock_axes(mask);
    } else {
        j.set_limits(axis, [lo, hi]);
    }
}

/// A damping coefficient kept to `[0, 10]`; a non-finite one (a broken file) is 0.
fn damping(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 10.0)
    } else {
        0.0
    }
}

/// Whether a stored constraint's pivots (and a spring's length) are finite once scaled
/// by `s`: a NaN pivot in a joint would poison every body of the ragdoll.
pub fn joint_is_finite(j: &RagdollJoint, s: f32) -> bool {
    let spring = match j.kind {
        JointKind::Spring { length } => (length * s).is_finite(),
        _ => true,
    };
    spring && (vec3(j.pivot_a) * s).is_finite() && (vec3(j.pivot_b) * s).is_finite()
}

/// The rapier joint for a stored constraint: body 1 is the constraint's `b` (the body
/// attached to), body 2 its `a`. `s` scales the pivots (actor scale × metres per unit).
pub fn joint(j: &RagdollJoint, s: f32) -> GenericJoint {
    let (pa, pb) = (vec3(j.pivot_a) * s, vec3(j.pivot_b) * s);
    let ball = || GenericJointBuilder::new(JointAxesMask::LOCKED_SPHERICAL_AXES).local_anchor1(pb).local_anchor2(pa).build();
    let mut out = match j.kind {
        JointKind::Ragdoll { twist_a, plane_a, twist_b, plane_b, twist_min, twist_max, .. } => {
            match (frame(twist_a, plane_a), frame(twist_b, plane_b)) {
                (Some(fa), Some(fb)) => {
                    let mut g = GenericJointBuilder::new(JointAxesMask::LOCKED_SPHERICAL_AXES)
                        .local_frame1(Pose::from_parts(pb, fb))
                        .local_frame2(Pose::from_parts(pa, fa))
                        .build();
                    // x: the twist. The cone and the plane cones are joints of their own
                    // ([`cone_joints`]).
                    limit(&mut g, JointAxis::AngX, JointAxesMask::ANG_X, twist_min, twist_max);
                    g
                }
                _ => ball(),
            }
        }
        JointKind::Hinge { axis_a, perp_a, axis_b, perp_b, limits } => match (frame(axis_a, perp_a), frame(axis_b, perp_b)) {
            (Some(fa), Some(fb)) => {
                let mut g = GenericJointBuilder::new(JointAxesMask::LOCKED_REVOLUTE_AXES)
                    .local_frame1(Pose::from_parts(pb, fb))
                    .local_frame2(Pose::from_parts(pa, fa))
                    .build();
                if let Some([lo, hi]) = limits {
                    limit(&mut g, JointAxis::AngX, JointAxesMask::ANG_X, lo, hi);
                }
                g
            }
            _ => ball(),
        },
        JointKind::Spring { length } => RopeJointBuilder::new((length * s).max(1e-4)).local_anchor1(pb).local_anchor2(pa).build().into(),
        JointKind::Ball | JointKind::Other { .. } => ball(),
    };
    // Joined bodies overlap by construction.
    out.set_contacts_enabled(false);
    for (axis, mask) in [(JointAxis::AngX, JointAxesMask::ANG_X), (JointAxis::AngY, JointAxesMask::ANG_Y), (JointAxis::AngZ, JointAxesMask::ANG_Z)] {
        if !out.locked_axes.contains(mask) && !matches!(j.kind, JointKind::Spring { .. }) {
            out.set_motor_velocity(axis, 0.0, JOINT_DAMPING);
        }
    }
    out
}

/// The cone limits of a ragdoll constraint (ragdoll.md §3), each held by a joint of its own
/// between the same two bodies as [`joint`]'s: the **cone** (the angle between the two
/// twist axes at most `cone_max`) and the **plane cones** (the angle between a's twist axis
/// and b's plane axis, π/2 − plane, inside π/2 − `plane_max` … π/2 − `plane_min`). Each is a
/// rapier joint with no locked axes whose two coupled angular axes limit the angle between
/// its frames' x axes: one row along the axis that turns one onto the other, the exact
/// gradient of that angle in every direction [design, 2026-11-13: limited about the twist
/// frame's y and z axes apart, as before, the cone was a box that let a limb through on
/// the diagonal (observed: `a_ragdoll_joint_holds_its_cone_in_every_direction` fails that
/// way); with the stored inertia these joints brought the human skeleton's worst joint at
/// rest from 5.4° to 2.3° outside its limits at 60 steps a second and from 4.6° to 0.1° at
/// 160 (ragdoll.md §4)].
pub fn cone_joints(j: &RagdollJoint, s: f32) -> Vec<GenericJoint> {
    let JointKind::Ragdoll { twist_a, plane_a, twist_b, plane_b, cone_max, plane_min, plane_max, .. } = j.kind else { return Vec::new() };
    let (pa, pb) = (vec3(j.pivot_a) * s, vec3(j.pivot_b) * s);
    let coupled = JointAxesMask::ANG_Y | JointAxesMask::ANG_Z;
    let cone_joint = |fa: Rotation, fb: Rotation, lo: f32, hi: f32| {
        let mut g = GenericJointBuilder::new(JointAxesMask::empty()).local_frame1(Pose::from_parts(pb, fb)).local_frame2(Pose::from_parts(pa, fa)).coupled_axes(coupled).build();
        g.set_limits(JointAxis::AngY, [lo, hi]);
        g.set_contacts_enabled(false);
        g
    };
    let mut out = Vec::new();
    let (pi, half_pi) = (std::f32::consts::PI, std::f32::consts::FRAC_PI_2);
    if let (Some(fa), Some(fb)) = (frame(twist_a, plane_a), frame(twist_b, plane_b)) {
        let cone = cone_max.abs();
        if cone.is_finite() && cone < pi {
            out.push(cone_joint(fa, fb, -1.0, cone));
        }
    }
    // The plane cones: the angle between a's twist axis and b's plane axis is π/2 − plane.
    if let (Some(fa), Some(fb)) = (frame(twist_a, plane_a), frame(plane_b, twist_b)) {
        let lo = if plane_max.is_finite() && plane_max < half_pi { half_pi - plane_max } else { -1.0 };
        let hi = if plane_min.is_finite() && plane_min > -half_pi { half_pi - plane_min } else { 4.0 };
        if lo > 0.0 || hi < pi {
            out.push(cone_joint(fa, fb, lo, hi));
        }
    }
    out
}

impl PhysWorld {
    /// Add (or replace) the ragdoll of actor `s.form_id`: a dynamic body per ragdoll body
    /// at its bone's pose, joined by the ragdoll's constraints.
    pub fn add_ragdoll(&mut self, s: &RagdollStart) -> RagdollReport {
        let nodes: Vec<Option<Iso>> = (0..s.ragdoll.bodies.len()).map(|i| s.nodes.get(i).copied()).collect();
        self.build_ragdoll(s.form_id, s.ragdoll.clone(), s.scale, s.parts.to_vec(), Vec::new(), &nodes, s.velocity, s.asleep, false)
    }

    /// Add the jointed bodies of placed object `s.form_id` (a swinging trap,
    /// docs/spec-notes/traps.md §4): as [`PhysWorld::add_ragdoll`], `nodes` the poses of
    /// the bodies' nodes, but its keyframed and static bodies (the anchors) are fixed
    /// where they are, its dynamic ones hang on them by the model's constraints, and its
    /// colliders are the reference's ([`Owner::Ref`], each body's own layer). `parts` is
    /// not used.
    pub fn add_object_ragdoll(&mut self, s: &RagdollStart) -> RagdollReport {
        let nodes: Vec<Option<Iso>> = (0..s.ragdoll.bodies.len()).map(|i| s.nodes.get(i).copied()).collect();
        self.build_ragdoll(s.form_id, s.ragdoll.clone(), s.scale, Vec::new(), Vec::new(), &nodes, s.velocity, s.asleep, true)
    }

    /// Build a ragdoll: dynamic bodies and joints, or fixed bodies alone when `at_rest`.
    /// The joints in `cut` are left out.
    #[allow(clippy::too_many_arguments)]
    fn build_ragdoll(&mut self, form_id: u32, data: Arc<Ragdoll>, scale: f32, parts: Vec<u8>, cut: Vec<usize>, nodes: &[Option<Iso>], velocity: [f32; 3], at_rest: bool, object: bool) -> RagdollReport {
        self.remove_ragdoll(form_id);
        let k = self.k;
        let f = scale.max(1e-3);
        let mut report = RagdollReport::default();
        let start_velocity = Some(vec3(velocity) * k).filter(|v| v.is_finite()).unwrap_or_default();
        let n = data.bodies.len();
        let joints = vec![Vec::new(); data.joints.len()];
        let mut rag = RagdollBodies { bodies: vec![None; n], local: Vec::with_capacity(n), collide: vec![0; n], at_rest, data: data.clone(), scale, parts, joints, cut, small: false, object, settle: None };
        // The bodies' shapes in the bind pose, for the self-collision pairs.
        let mut rest: Vec<Vec<(Pose, SharedShape)>> = Vec::with_capacity(n);
        for (i, b) in data.bodies.iter().enumerate() {
            let o = &b.object;
            let mut local = o.body_transform;
            local.translation = local.translation.map(|v| v * f);
            rag.local.push(local);
            let mut leaves = Vec::new();
            convert::leaves(&o.shape, &Iso::IDENTITY, f * k, &mut leaves, &mut report.errors);
            let pose = nodes.get(i).copied().flatten().and_then(|node| loose_pose(&node.compose(&local), k));
            let rest_pose = loose_pose(&b.rest(), f * k).unwrap_or_default();
            rest.push(leaves.iter().map(|l| (rest_pose * l.pose, l.shape.clone())).collect());
            let (Some(pose), false) = (pose, leaves.is_empty()) else { continue };
            let part = rag.parts.get(i).copied().unwrap_or(crate::actors::WHOLE);
            let owner = if object { Owner::Ref { form_id, object: i as u16, layer: o.layer } } else { Owner::Actor { form_id, layer: layers::DEAD_BIP, part } }.encode();
            // An object's anchors (keyframed or static bodies) do not move.
            let held = object && o.motion != fnv_collide::Motion::Dynamic;
            let builder = if at_rest || held {
                RigidBodyBuilder::fixed().pose(pose).user_data(owner)
            } else {
                RigidBodyBuilder::dynamic()
                    .pose(pose)
                    .linvel(start_velocity)
                    .linear_damping(damping(b.linear_damping))
                    .angular_damping(damping(b.angular_damping))
                    .ccd_enabled(true)
                    .user_data(owner)
            };
            let body = self.insert_body(builder);
            // Known at once, so `remove_ragdoll` finds it even when the build stops part way.
            self.ragdoll_of.insert(body, (form_id, i as u16));
            let friction = if o.friction.is_finite() { o.friction.clamp(0.0, MAX_FRICTION) } else { 0.5 };
            for leaf in leaves {
                let c = ColliderBuilder::new(leaf.shape)
                    .position(leaf.pose)
                    .friction(friction)
                    .restitution(0.0)
                    .active_hooks(ActiveHooks::FILTER_CONTACT_PAIRS)
                    .user_data(owner);
                self.w.insert_collider(c, Some(body));
            }
            if !at_rest && !held {
                self.set_mass(body, o.mass);
                // An actor's bodies turn with their stored inertia (ragdoll.md §2, §4).
                if let (Some(t), false) = (b.inertia, object) {
                    self.set_inertia(body, t.map(|v| v * (f * k) * (f * k)));
                }
            }
            rag.bodies[i] = Some(body);
            report.bodies += 1;
        }
        let mut joined = vec![0u64; n];
        for (ji, j) in data.joints.iter().enumerate() {
            if j.a < 64 && j.b < 64 {
                joined[j.a] |= 1 << j.b;
                joined[j.b] |= 1 << j.a;
            }
            if at_rest || rag.cut.contains(&ji) {
                continue;
            }
            let (Some(Some(a)), Some(Some(b))) = (rag.bodies.get(j.a), rag.bodies.get(j.b)) else { continue };
            if !joint_is_finite(j, f * k) {
                // Left out and reported: the two bodies fall apart instead.
                report.errors.push(ConvertError::BadDimension);
                continue;
            }
            // A placed object's joint that starts outside its own limits (the swinging log's
            // rear chain meets the ram turned 90 degrees past a twist limit of 5) is built
            // without them, a ball joint: held to its limits it wrenches the whole object
            // from the first step [observed: traps.md §4; design: the original's handling
            // is not known].
            let starts_outside = object && matches!((nodes.get(j.a), nodes.get(j.b)), (Some(Some(na)), Some(Some(nb))) if !j.within(&j.angles(na, nb), START_SLACK));
            let built = if starts_outside {
                report.freed.push(ji);
                joint(&RagdollJoint { kind: JointKind::Ball, ..j.clone() }, f * k)
            } else {
                joint(j, f * k)
            };
            rag.joints[ji].push(self.w.insert_impulse_joint(*b, *a, built));
            if !starts_outside {
                for cone in cone_joints(j, f * k) {
                    rag.joints[ji].push(self.w.insert_impulse_joint(*b, *a, cone));
                }
            }
            report.joints += 1;
        }
        // Its size from the bind-pose shapes (scaled as built).
        let mut extent: Option<(Vector, Vector)> = None;
        for (pose, shape) in rest.iter().flatten() {
            let a = shape.compute_aabb(pose);
            let e = extent.get_or_insert((a.mins, a.maxs));
            *e = (e.0.min(a.mins), e.1.max(a.maxs));
        }
        rag.small = !object && extent.is_some_and(|(lo, hi)| (hi - lo).max_element() < SMALL_RAGDOLL_SPAN * k);
        let gap = SELF_COLLISION_GAP * f * k;
        // A placed object's parts do not meet each other: its bodies share one Havok
        // collision group (a swinging trap's chains and weight, UPB `ColGroupInfo`), and
        // Havok does not collide bodies of one group [observed: traps.md §4; hypothesis:
        // the group's rule]. Left to collide, one chain's links knocked the other's about.
        let pairs = if object { 0 } else { n.min(64) };
        for i in 0..pairs {
            for j in i + 1..n.min(64) {
                if joined[i] & (1 << j) != 0 {
                    continue;
                }
                let apart = rest[i].iter().all(|(pa, sa)| {
                    rest[j].iter().all(|(pb, sb)| query::distance(pa, sa.as_ref(), pb, sb.as_ref()).is_ok_and(|d| d.distance > gap))
                });
                if apart {
                    rag.collide[i] |= 1 << j;
                    rag.collide[j] |= 1 << i;
                    report.self_pairs += 1;
                }
            }
        }
        self.ragdolls.insert(form_id, rag);
        self.mark_dirty(crate::world::DirtyBy::Bodies);
        report
    }

    /// Give a dynamic body the inertia tensor `inertia` (kg·m², rows of a 3×3 matrix in the
    /// body's frame) about its mass centre, in place of the one rapier works out from its
    /// shapes; its mass and mass centre stay. Its colliders then carry no mass of their own
    /// (density 0) and the body's mass properties are all its additional ones. Returns
    /// false, changing nothing, when the tensor's principal moments are not all positive.
    ///
    /// For actors' ragdolls [observed, `_male\skeleton.nif`, ragdoll.md §2]: rapier's
    /// moments from the shapes equal the stored ones for the head, hands and feet and about
    /// the thighs' and calves' short sides, but are a seventeenth to a sixty-seventh of them
    /// for the neck, a thirty-sixth for the pelvis, a sixth for the lowest spine body and a
    /// fifteenth for the upper arms about their length. Turning that easily, those bodies
    /// spun through their joints' limits under a fall or a shove and corpses lay with heads
    /// turned round, necks bent back and hips and shoulders twisted (ragdoll.md §4).
    fn set_inertia(&mut self, body: RigidBodyHandle, inertia: [f32; 9]) -> bool {
        let Some(b) = self.w.bodies.get(body) else { return false };
        let mp = b.mass_properties().local_mprops;
        let props = MassProperties::with_inertia_matrix(mp.local_com, mp.mass(), Matrix::from_cols_array(&inertia).transpose());
        let moments = props.principal_inertia();
        if !(mp.mass() > 0.0 && moments.is_finite() && moments.min_element() > 0.0) {
            return false;
        }
        let colliders: Vec<ColliderHandle> = b.colliders().to_vec();
        for c in colliders {
            self.w.colliders[c].set_density(0.0);
        }
        let b = &mut self.w.bodies[body];
        b.set_additional_mass_properties(props, true);
        b.recompute_mass_properties_from_colliders(&self.w.colliders);
        true
    }

    /// Build a ragdoll again where it is now, at rest or moving.
    fn rebuild_ragdoll(&mut self, form_id: u32, at_rest: bool) -> bool {
        let Some(nodes) = self.ragdoll_nodes(form_id) else { return false };
        let nodes: Vec<Option<Iso>> = nodes.iter().map(|b| b.as_ref().map(|b| Iso { translation: b.translation, rotation: b.rotation })).collect();
        let Some(r) = self.ragdolls.get(&form_id) else { return false };
        let (data, scale, parts, cut, object) = (r.data.clone(), r.scale, r.parts.clone(), r.cut.clone(), r.object);
        self.build_ragdoll(form_id, data, scale, parts, cut, &nodes, [0.0; 3], at_rest, object);
        true
    }

    /// Cut a ragdoll apart (dismemberment, docs/spec-notes/dismemberment.md §5): every
    /// joint between a body in `bodies` and one outside them goes, now and in later
    /// rebuilds. Returns the joints newly cut; none without a ragdoll.
    pub fn cut_ragdoll(&mut self, form_id: u32, bodies: &[usize]) -> usize {
        let Some(r) = self.ragdolls.get_mut(&form_id) else { return 0 };
        let mut handles = Vec::new();
        let mut woken = Vec::new();
        let mut newly = 0;
        for (ji, j) in r.data.joints.iter().enumerate() {
            if bodies.contains(&j.a) != bodies.contains(&j.b) && !r.cut.contains(&ji) {
                r.cut.push(ji);
                newly += 1;
                handles.extend(r.joints.get_mut(ji).map(std::mem::take).unwrap_or_default());
                woken.extend([j.a, j.b].iter().filter_map(|&i| r.bodies.get(i).copied().flatten()));
            }
        }
        for h in handles {
            self.w.impulse_joints.remove(h, true);
        }
        // Like a struck body, the parts must not sleep through the change.
        self.struck.extend(woken);
        if newly > 0 {
            self.mark_dirty(crate::world::DirtyBy::Other);
        }
        newly
    }

    /// The stored joints of the actor's ragdoll that dismemberment cut.
    pub fn ragdoll_cut(&self, form_id: u32) -> Vec<usize> {
        self.ragdolls.get(&form_id).map_or_else(Vec::new, |r| r.cut.clone())
    }

    /// Hold a ragdoll where it lies: fixed bodies take the place of its jointed ones, so
    /// it neither creeps nor costs anything, and still stops rays and loose objects. A
    /// push ([`PhysWorld::push_ragdoll`]) or a blast ([`PhysWorld::push_dynamic`]) makes
    /// it a ragdoll again [design: the engine's own sleep is not used for this; a body
    /// just added wakes up, and joints resting on their limits can keep one from
    /// sleeping]. A ragdoll the player holds ([`PhysWorld::ragdoll_held`]) is not: its
    /// held body would stop following the grip. Returns whether it is at rest.
    pub fn set_ragdoll_at_rest(&mut self, form_id: u32) -> bool {
        if self.ragdoll_held(form_id) {
            return false;
        }
        match self.ragdolls.get(&form_id).map(|r| r.at_rest) {
            Some(true) => true,
            Some(false) => self.rebuild_ragdoll(form_id, true),
            None => false,
        }
    }

    /// Make a ragdoll held at rest move again.
    pub(crate) fn rouse_ragdoll(&mut self, form_id: u32) {
        if self.ragdolls.get(&form_id).is_some_and(|r| r.at_rest) {
            self.rebuild_ragdoll(form_id, false);
        }
    }

    /// Settle a moving ragdoll before it is held at rest: `share` (0 to 1) of
    /// [`SETTLE_DAMPING`] is added to each body's own damping, so that its motion dies out
    /// as friction would stop it, instead of stopping dead when its bodies are fixed
    /// ([`PhysWorld::set_ragdoll_at_rest`]). The caller raises the share over the settle.
    /// A push or a blast that strikes it ends the settle ([`PhysWorld::push_ragdoll`],
    /// [`PhysWorld::push_dynamic`]), as does [`PhysWorld::unsettle_ragdoll`]. Returns false
    /// without a moving actor's ragdoll.
    pub fn settle_ragdoll(&mut self, form_id: u32, share: f32) -> bool {
        let share = if share.is_finite() { share.clamp(0.0, 1.0) } else { 0.0 };
        self.set_settle(form_id, Some(share))
    }

    /// End a ragdoll's settle ([`PhysWorld::settle_ragdoll`]): its bodies have their own
    /// damping again. Returns whether it was settling.
    pub fn unsettle_ragdoll(&mut self, form_id: u32) -> bool {
        self.ragdoll_settling(form_id) && self.set_settle(form_id, None)
    }

    /// Whether the ragdoll is settling ([`PhysWorld::settle_ragdoll`]).
    pub fn ragdoll_settling(&self, form_id: u32) -> bool {
        self.ragdolls.get(&form_id).is_some_and(|r| r.settle.is_some())
    }

    fn set_settle(&mut self, form_id: u32, settle: Option<f32>) -> bool {
        let Some(r) = self.ragdolls.get_mut(&form_id).filter(|r| !r.at_rest && !r.object) else { return false };
        r.settle = settle;
        let extra = settle.unwrap_or(0.0) * SETTLE_DAMPING;
        for (b, d) in r.bodies.iter().zip(&r.data.bodies) {
            let Some(body) = b.and_then(|h| self.w.bodies.get_mut(h)) else { continue };
            body.set_linear_damping(damping(d.linear_damping) + extra);
            body.set_angular_damping(damping(d.angular_damping) + extra);
        }
        true
    }

    /// The ragdolls held at rest that have a body within `radius` of `center` (game units).
    pub(crate) fn ragdolls_at_rest_near(&self, center: [f32; 3], radius: f32) -> Vec<u32> {
        let (c, r2) = (vec3(center) * self.k, (radius * self.k).powi(2));
        let near = |r: &RagdollBodies| r.bodies.iter().flatten().filter_map(|&b| self.w.bodies.get(b)).any(|b| (b.position().translation - c).length_squared() <= r2);
        self.ragdolls.iter().filter(|(_, r)| r.at_rest && near(r)).map(|(&id, _)| id).collect()
    }

    /// Remove an actor's ragdoll.
    pub fn remove_ragdoll(&mut self, form_id: u32) -> bool {
        let Some(r) = self.ragdolls.remove(&form_id) else {
            // Bodies of a build that stopped part way (a panic its caller caught) are
            // known only by `ragdoll_of`.
            let orphans: Vec<RigidBodyHandle> = self.ragdoll_of.iter().filter(|(_, o)| o.0 == form_id).map(|(&b, _)| b).collect();
            for b in orphans {
                self.ragdoll_of.remove(&b);
                self.remove_body(b);
            }
            return false;
        };
        for b in r.bodies.into_iter().flatten() {
            self.ragdoll_of.remove(&b);
            self.remove_body(b);
        }
        true
    }

    /// Whether the actor's ragdoll counts as small ([`SMALL_RAGDOLL_SPAN`]).
    pub fn ragdoll_is_small(&self, form_id: u32) -> bool {
        self.ragdolls.get(&form_id).is_some_and(|r| r.small)
    }

    /// Contact pairs touching now (as of the last step) between bodies of two different
    /// ragdolls. Diagnostics: what a heap of corpses costs the solver.
    pub fn ragdoll_contacts_between(&self) -> usize {
        let of = |c: ColliderHandle| self.w.colliders.get(c).and_then(|c| c.parent()).and_then(|b| self.ragdoll_of.get(&b)).map(|o| o.0);
        self.w
            .narrow_phase
            .contact_pairs()
            .filter(|p| p.has_any_active_contact())
            .filter(|p| matches!((of(p.collider1), of(p.collider2)), (Some(a), Some(b)) if a != b))
            .count()
    }

    /// What the bodies of an actor's ragdoll touch now (as of the last step): every pair
    /// with an active contact, by what the other collider is, or only those of body
    /// `body` (index in the ragdoll's bodies). Diagnostics for a ragdoll that does not come
    /// to rest (#2083); reads the world, changes nothing. Empty without a ragdoll.
    pub fn ragdoll_contacts(&self, form_id: u32, body: Option<usize>) -> RagdollContacts {
        let mut out = RagdollContacts::default();
        let Some(r) = self.ragdolls.get(&form_id) else { return out };
        let mut seen = std::collections::HashSet::new();
        let mut others = std::collections::BTreeSet::new();
        let mut refs = std::collections::BTreeSet::new();
        for (i, h) in r.bodies.iter().enumerate() {
            if body.is_some_and(|b| b != i) {
                continue;
            }
            let Some(b) = h.and_then(|h| self.w.bodies.get(h)) else { continue };
            for &c in b.colliders() {
                for pair in self.w.contact_pairs_with(c).filter(|p| p.has_any_active_contact()) {
                    // A pair of two of its own bodies is met from both sides: counted once.
                    let (c1, c2) = (pair.collider1.into_raw_parts(), pair.collider2.into_raw_parts());
                    let key = if c1 < c2 { (c1, c2) } else { (c2, c1) };
                    if !seen.insert(key) {
                        continue;
                    }
                    let other = if pair.collider1 == c { pair.collider2 } else { pair.collider1 };
                    let depth = pair.find_deepest_contact().map_or(0.0, |(_, k)| (-k.dist / self.k).max(0.0));
                    let co = self.w.colliders.get(other);
                    let parent = co.and_then(|co| co.parent());
                    let owner = co.and_then(|co| Owner::decode(co.user_data));
                    let touch = match (parent.and_then(|p| self.ragdoll_of.get(&p)), owner) {
                        (Some(&(f, _)), _) if f == form_id => &mut out.own,
                        (Some(&(f, _)), _) => {
                            others.insert(f);
                            if self.ragdolls.get(&f).is_some_and(|o| o.at_rest) {
                                &mut out.resting_ragdolls
                            } else {
                                &mut out.moving_ragdolls
                            }
                        }
                        (None, Some(Owner::Terrain { .. })) => &mut out.terrain,
                        (None, Some(Owner::Ref { form_id: f, .. })) => {
                            refs.insert(f);
                            match parent.and_then(|p| self.w.bodies.get(p)) {
                                Some(p) if p.is_dynamic() => &mut out.loose,
                                _ => &mut out.fixed,
                            }
                        }
                        _ => &mut out.other,
                    };
                    touch.pairs += 1;
                    touch.deepest = touch.deepest.max(depth);
                }
            }
        }
        out.ragdolls = others.into_iter().collect();
        out.refs = refs.into_iter().collect();
        out
    }

    /// Whether the ragdoll is held at rest ([`PhysWorld::set_ragdoll_at_rest`]), not merely
    /// asleep in the engine ([`PhysWorld::ragdoll_asleep`] is either).
    pub fn is_ragdoll_at_rest(&self, form_id: u32) -> bool {
        self.ragdolls.get(&form_id).is_some_and(|r| r.at_rest)
    }

    /// Whether the player's grab holds a body of this actor's ragdoll.
    pub fn ragdoll_held(&self, form_id: u32) -> bool {
        self.grip.as_ref().is_some_and(|g| g.on == crate::GripOn::Ragdoll(form_id))
    }

    pub fn has_ragdoll(&self, form_id: u32) -> bool {
        self.ragdolls.contains_key(&form_id)
    }

    /// The actors that have a ragdoll, by FormID.
    pub fn ragdolls(&self) -> impl Iterator<Item = u32> + '_ {
        self.ragdolls.keys().copied()
    }

    /// The pose of each ragdoll body's bone now, in the ragdoll's body order (`None` for
    /// a body that was not built), and its velocity and sleep state.
    pub fn ragdoll_nodes(&self, form_id: u32) -> Option<Vec<Option<BodyState>>> {
        self.nodes_drawn(form_id, None)
    }

    /// [`PhysWorld::ragdoll_nodes`] as drawn this frame, `alpha` of a step after the last
    /// one: each body's pose blended between before and after the last step
    /// (`crate::drawn`). For drawing only.
    pub fn drawn_ragdoll_nodes(&self, form_id: u32, alpha: f32) -> Option<Vec<Option<BodyState>>> {
        self.nodes_drawn(form_id, Some(alpha))
    }

    /// The bones' poses from the bodies' poses now (`alpha` `None`) or as drawn.
    fn nodes_drawn(&self, form_id: u32, alpha: Option<f32>) -> Option<Vec<Option<BodyState>>> {
        let r = self.ragdolls.get(&form_id)?;
        let k = self.k;
        Some(
            r.bodies
                .iter()
                .zip(&r.local)
                .map(|(b, local)| {
                    let h = (*b)?;
                    let body = self.w.bodies.get(h)?;
                    let pose = alpha.map_or(*body.position(), |a| self.drawn_pose(h, body, a));
                    let node = iso_of(&pose, k).compose(&local.inverse());
                    Some(BodyState { translation: node.translation, rotation: node.rotation, ..body_state(body, k) })
                })
                .collect(),
        )
    }

    /// Whether `form_id`'s ragdoll is a placed object's ([`PhysWorld::add_object_ragdoll`]),
    /// not an actor's.
    pub fn is_object_ragdoll(&self, form_id: u32) -> bool {
        self.ragdolls.get(&form_id).is_some_and(|r| r.object)
    }

    /// Whether the ragdoll is at rest: held ([`PhysWorld::set_ragdoll_at_rest`]) or every
    /// body asleep.
    pub fn ragdoll_asleep(&self, form_id: u32) -> bool {
        self.ragdolls.get(&form_id).is_some_and(|r| r.at_rest || r.bodies.iter().flatten().all(|&b| self.w.bodies.get(b).is_none_or(|b| b.is_sleeping())))
    }

    /// The fastest linear (game units per second) and angular (radians per second) speed
    /// among the ragdoll's bodies.
    pub fn ragdoll_speed(&self, form_id: u32) -> Option<(f32, f32)> {
        let r = self.ragdolls.get(&form_id)?;
        let bodies = r.bodies.iter().flatten().filter_map(|&b| self.w.bodies.get(b));
        Some(bodies.fold((0.0f32, 0.0f32), |m, b| (m.0.max(b.linvel().length() / self.k), m.1.max(b.angvel().length()))))
    }

    /// The mass centre of the ragdoll's heaviest body (the trunk of a person), game units.
    pub fn ragdoll_center(&self, form_id: u32) -> Option<[f32; 3]> {
        self.center_drawn(form_id, None)
    }

    /// [`PhysWorld::ragdoll_center`] as drawn this frame, `alpha` of a step after the last
    /// one (`crate::drawn`): for what follows a corpse on screen (the kill camera).
    pub fn drawn_ragdoll_center(&self, form_id: u32, alpha: f32) -> Option<[f32; 3]> {
        self.center_drawn(form_id, Some(alpha))
    }

    /// The speed of [`PhysWorld::ragdoll_center`]'s body (its heaviest), game units a
    /// second: for telling a corpse that should move from one that rests.
    pub fn ragdoll_center_speed(&self, form_id: u32) -> Option<f32> {
        let b = self.w.bodies.get(self.heaviest(form_id)?)?;
        Some(b.linvel().length() / self.k)
    }

    /// A ragdoll's heaviest body: by the stored masses, the first of equals, the same body
    /// however it is built.
    fn heaviest(&self, form_id: u32) -> Option<RigidBodyHandle> {
        let r = self.ragdolls.get(&form_id)?;
        let mut best: Option<(f32, RigidBodyHandle)> = None;
        for (b, d) in r.bodies.iter().zip(&r.data.bodies) {
            if let Some(h) = b.filter(|_| best.is_none_or(|(m, _)| d.object.mass > m)) {
                best = Some((d.object.mass, h));
            }
        }
        best.map(|b| b.1)
    }

    /// The heaviest body's mass centre now (`alpha` `None`) or as drawn.
    fn center_drawn(&self, form_id: u32, alpha: Option<f32>) -> Option<[f32; 3]> {
        let h = self.heaviest(form_id)?;
        let b = self.w.bodies.get(h)?;
        let Some(a) = alpha else { return Some(arr3(b.center_of_mass() / self.k)) };
        let pose = self.drawn_pose(h, b, a);
        Some(arr3((pose.rotation * b.local_center_of_mass() + pose.translation) / self.k))
    }

    /// A sphere holding an actor's ragdoll, game units: its centre its heaviest body's (the
    /// trunk of a person), its radius out to the far side of every one of its colliders'
    /// bounds. What a blast reaches a corpse by, as it reaches a living actor by its hit
    /// volume (#2346) [design]. None for a placed object's ragdoll, or none at all.
    pub fn ragdoll_bounds(&self, form_id: u32) -> Option<([f32; 3], f32)> {
        let r = self.ragdolls.get(&form_id).filter(|r| !r.object)?;
        let center = *self.ragdoll_body_centers(form_id).first()?;
        let c = vec3(center) * self.k;
        let mut reach = 0.0f32;
        for (lo, hi) in r.bodies.iter().flatten().filter_map(|&b| self.body_extent(b)) {
            reach = reach.max(((lo + hi) * 0.5 - c).length() + ((hi - lo) * 0.5).length());
        }
        Some((center, reach / self.k))
    }

    /// The centres of an actor's ragdoll bodies (each the middle of its colliders' bounds),
    /// heaviest first by their stored masses (the trunk of a person first), game units: the
    /// points of a corpse a blast sees and measures to (#2346) [design]. The colliders'
    /// bounds, not the bodies' mass centres, which the engine works out only at its next step.
    /// Empty for a placed object's ragdoll, or none at all.
    pub fn ragdoll_body_centers(&self, form_id: u32) -> Vec<[f32; 3]> {
        let Some(r) = self.ragdolls.get(&form_id).filter(|r| !r.object) else { return Vec::new() };
        let mut out: Vec<(f32, [f32; 3])> = r
            .bodies
            .iter()
            .zip(&r.data.bodies)
            .filter_map(|(b, d)| self.body_extent((*b)?).map(|(lo, hi)| (d.object.mass, arr3((lo + hi) * 0.5 / self.k))))
            .collect();
        // Stable: equal masses keep the stored order.
        out.sort_by(|x, y| y.0.total_cmp(&x.0));
        out.into_iter().map(|(_, p)| p).collect()
    }

    /// The bounds of a body's colliders (engine units), if it has any.
    fn body_extent(&self, b: RigidBodyHandle) -> Option<(Vector, Vector)> {
        let body = self.w.bodies.get(b)?;
        body.colliders().iter().filter_map(|&h| self.w.colliders.get(h)).map(|co| co.compute_aabb()).map(|a| (a.mins, a.maxs)).reduce(|x, y| (x.0.min(y.0), x.1.max(y.1)))
    }

    /// Change the velocity of the ragdoll body nearest to `at` by `dv` (game units per
    /// second), as a hit there does: the body's own velocity changes by `dv` whatever its
    /// mass, and it is turned as a blow at `at` turns it, by at most [`MAX_PUSH_SPIN`] (a
    /// small body struck off centre would otherwise spin fast enough to pass through the
    /// ground). A settling ragdoll stops settling. Returns false without a ragdoll.
    pub fn push_ragdoll(&mut self, form_id: u32, at: [f32; 3], dv: [f32; 3]) -> bool {
        self.rouse_ragdoll(form_id);
        self.unsettle_ragdoll(form_id);
        let Some(r) = self.ragdolls.get(&form_id) else { return false };
        let k = self.k;
        let p = vec3(at) * k;
        let nearest = r
            .bodies
            .iter()
            .flatten()
            .filter_map(|&b| Some((b, (self.w.bodies.get(b)?.center_of_mass() - p).length_squared())))
            .min_by(|x, y| x.1.total_cmp(&y.1));
        let Some((b, _)) = nearest else { return false };
        let body = &mut self.w.bodies[b];
        let dv = vec3(dv) * k;
        // The arm of the blow, kept away from zero so that the spin stays bounded.
        let r = p - body.center_of_mass();
        let spin = r.cross(dv) / r.length_squared().max((PUSH_ARM * k).powi(2));
        let spin = spin.clamp_length_max(MAX_PUSH_SPIN);
        body.set_linvel(body.linvel() + dv, true);
        body.set_angvel(body.angvel() + spin, true);
        true
    }

    /// Step with the ragdolls' contact filter.
    pub(crate) fn step_world(&mut self) {
        if self.ragdolls.is_empty() && self.gore.is_empty() {
            self.w.step();
        } else {
            let hooks = RagdollHooks { ragdolls: &self.ragdolls, of_body: &self.ragdoll_of, passes: &self.passes_ragdoll };
            self.w.step_with_events(&hooks, &());
        }
    }

    /// Update user changes, islands, broad phase and narrow phase without advancing time,
    /// using the ragdolls' contact filter.
    ///
    /// A zero-length `PhysicsPipeline::step` registers newly inserted bodies and user
    /// changes with `islands` before narrow-phase contact transitions run — unlike
    /// `CollisionPipeline::step`, which omits island registration and both trips rapier
    /// 0.36.1's `active_island_id != u32::MAX` assertion (`island_manager/manager.rs:141`)
    /// when new bodies overlap after prior stepping and leaves touching pairs unlinked in
    /// the persistent islands.
    ///
    /// Because rapier 0.36.1's `dt <= 0.0` early return skips joining a deferred BVH
    /// optimization pass (`BroadPhaseBvh::take_deferred_optimize` temporarily replaces the
    /// tree with an empty `Bvh`), run one no-op zero-dt pass whenever that happened so
    /// queries see the completed tree immediately.
    pub(crate) fn detect(&mut self) {
        let saved_dt = self.w.integration_parameters.dt;
        self.w.integration_parameters.dt = 0.0;
        self.step_world();
        if self.w.query_pipeline_with_filter(QueryFilter::default()).bvh.is_empty() && !self.w.colliders.is_empty() {
            // `step_world()` clears `self.w.quarantine()` at the start of `step_inner`,
            // so flush any quarantine produced by the first zero-dt pass before running
            // the second pass that joins the deferred BVH optimization tree.
            if !self.w.quarantine().is_empty() {
                self.handle_quarantine();
            }
            self.step_world();
        }
        self.w.integration_parameters.dt = saved_dt;
    }
}

#[cfg(test)]
mod query_refresh_lifecycle_tests {
    use super::*;
    use fnv_collide::{CollisionObject, Motion, Shape};

    fn one_body() -> (PhysWorld, RigidBodyHandle) {
        let mut world=PhysWorld::new(crate::PhysParams {gravity:0.,..Default::default()});
        let pose=crate::RefPose {form_id:99,position:[0.,0.,100.],rotation:[1.,0.,0.,0.,1.,0.,0.,0.,1.],scale:1.};
        let shape=CollisionObject {node:None,node_block:0,node_transform:Iso::IDENTITY,body_transform:Iso::IDENTITY,motion:Motion::Dynamic,
            layer:1,mass:10.,friction:0.5,restitution:0.,shape:Shape::Box {half_extents:[10.;3]}.into(),shell:0.,material:None,wind:false};
        world.add_ref(&pose,&[shape]);world.step();
        let handle=world.refs[&99][0];(world,handle)
    }

    #[test]
    fn zero_dt_refresh_keeps_persistent_dt_pose_velocity_and_pending_user_force() {
        let (mut world,handle)=one_body();
        world.w.bodies[handle].add_force(rapier3d::math::Vector::new(3.,4.,5.),true);
        let force=world.w.bodies[handle].user_force();let torque=world.w.bodies[handle].user_torque();
        let body=world.ref_bodies(99)[0];let steps=world.steps();
        let dt=world.w.integration_parameters.dt.to_bits();let configured=world.params.dt.to_bits();
        for _ in 0..3 {
            world.mark_dirty_bodies();world.update_queries();
            assert_eq!(world.w.integration_parameters.dt.to_bits(),dt);assert_eq!(world.params.dt.to_bits(),configured);
            assert_eq!(world.steps(),steps);assert_eq!(world.ref_bodies(99)[0],body);
            assert_eq!(world.w.bodies[handle].user_force(),force);assert_eq!(world.w.bodies[handle].user_torque(),torque);
        }
        world.step();assert!(world.ref_bodies(99)[0].linvel[0]>body.linvel[0]);
    }

    #[test]
    fn actual_disabled_then_reenabled_body_is_registered_without_time_advancement() {
        let (mut world,handle)=one_body();let body=world.ref_bodies(99)[0];let steps=world.steps();
        world.w.bodies[handle].set_enabled(false);world.mark_dirty_bodies();world.update_queries();
        assert!(!world.w.bodies[handle].is_enabled());assert_eq!(world.steps(),steps);
        world.w.bodies[handle].set_enabled(true);world.mark_dirty_bodies();world.update_queries();
        assert!(world.w.bodies[handle].is_enabled());assert_eq!(world.steps(),steps);
        assert_eq!(world.ref_bodies(99)[0],body);assert_eq!(world.stats().bodies_awake,1);
    }
}
