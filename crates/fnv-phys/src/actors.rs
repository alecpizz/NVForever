//! Actor hit volumes (docs/design/combat.md §5.2, steps C3 and C8): a kinematic body per
//! placed actor with either one vertical capsule ([`PhysWorld::set_actor_volume`]) or one
//! capsule per body part ([`PhysWorld::set_actor_parts`], posed from the skeleton by the
//! caller each frame), so shots and melee rays hit NPCs and creatures, and say which part.
//!
//! - The colliders are on the Havok BIPED layer while the actor lives and DEAD_BIP once
//!   it is dead (`crate::layers`; the layer names are documented, using them for actors
//!   is our choice). Queries can skip either with `Filter::exclude_layers`.
//! - They take part in no contacts: dynamic clutter is not pushed and the character
//!   controller ignores them [design: combat volumes are for shots, not for walking].
//!
//! Sizes and bone positions come from the caller; this module keeps the shapes where it
//! is told.
//!
//! **Cost of a write** [design: rapier3d 0.36.0 `geometry/broad_phase_bvh/update.rs:79-94,272`,
//! `geometry/collider.rs:439-444`]: placing a capsule attached to the actor's body
//! (`Collider::set_position_wrt_parent`) flags it `PARENT`, which rapier's broad phase treats
//! as a re-parent: at the next refresh the leaf is taken out of the tree and inserted again
//! as a brand-new one, and the whole tree is refitted. So
//! - a call whose volume and parts are bit-identical to the last one writes nothing and does
//!   not dirty the world (counted in `RefreshStats::hitvol_skipped`);
//! - by default ([`PhysWorld::set_standalone_hit_volumes`], on unless switched off), new
//!   actors' capsules are colliders of no body, placed at the pose rapier would give them on
//!   the body (`body pose * local pose`, worked out at the next refresh or step, where rapier
//!   works it out for an attached collider), which flags only `POSITION`: no forced
//!   re-parent. A leaf inside its margin is left alone; one that leaves it is handled as any
//!   moved collider (updated in place, re-inserted by SAH in a small batch, or covered by a
//!   full refit; rapier `update.rs:139,288`). The body is kept (same bodies, same digest).
//!   Switched off, the capsules are attached to the body as before.
//!
//! Exact ties (docs/spec-notes/physics.md §5.1): a first-hit `ray`/`sweep` whose two
//! candidates have the same time of impact (coincident or mirrored capsules) used to return
//! the first in tree-traversal order, so the two representations could pick different
//! winners [observed: `cargo test -p fnv-phys --test synthetic hitvol`, 2026-11-03]. Since
//! #1459 ties are broken by owner (form id, layer, part), then segment (the capsule's index
//! in the parts list, stored with `Owner::encode_segment`), then the hit's geometry, so
//! both representations, and any tree history, give the same answer [design, #1459].

use crate::world::{Owner, PhysWorld};
use crate::{layers, BodyState};
use rapier3d::math::{Pose, Rotation};
use rapier3d::prelude::*;

/// A capsule standing on `feet`: `height` from the bottom of the lower cap to the top of
/// the upper cap, game units. With parts, the actor's overall extent.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct HitVolume {
    pub feet: [f32; 3],
    pub radius: f32,
    pub height: f32,
    pub dead: bool,
}

impl HitVolume {
    fn layer(&self) -> u8 {
        if self.dead {
            layers::DEAD_BIP
        } else {
            layers::BIPED
        }
    }

    /// Whether every size and position is finite (a broken skeleton or record can give
    /// NaN, which must not reach rapier).
    pub fn is_finite(&self) -> bool {
        self.feet.iter().all(|v| v.is_finite()) && self.radius.is_finite() && self.height.is_finite()
    }

    /// Centre of the capsule, game units.
    pub fn center(&self) -> [f32; 3] {
        let h = self.height.max(2.0 * self.radius);
        [self.feet[0], self.feet[1], self.feet[2] + h / 2.0]
    }
}

/// One body part's capsule: the segment `a`–`b` (game space) swept by `radius`. `part`
/// is the index of the part in its body part data (BPTD).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct PartCapsule {
    pub part: u8,
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub radius: f32,
}

impl PartCapsule {
    /// Whether its segment and radius are finite.
    pub fn is_finite(&self) -> bool {
        self.a.iter().chain(&self.b).all(|v| v.is_finite()) && self.radius.is_finite()
    }
}

/// The part index of a whole-actor capsule.
pub const WHOLE: u8 = u8::MAX;

/// An actor's body and its colliders.
#[derive(Debug, Clone)]
pub(crate) struct ActorBody {
    body: RigidBodyHandle,
    volume: HitVolume,
    /// (collider, part, half segment length, radius); one with [`WHOLE`] for the single
    /// capsule.
    colliders: Vec<(ColliderHandle, u8, f32, f32)>,
    /// The part capsules of the last [`PhysWorld::set_actor_parts`] that wrote them (empty
    /// after [`PhysWorld::set_actor_volume`]): a call with bit-identical ones writes nothing.
    parts: Vec<PartCapsule>,
    /// Its colliders belong to no body ([`PhysWorld::set_standalone_hit_volumes`] when the
    /// actor's body was made).
    standalone: bool,
}

/// Whether two volumes are the same to the bit (`PartialEq` would take -0.0 for 0.0).
fn same_volume(a: &HitVolume, b: &HitVolume) -> bool {
    a.feet.map(f32::to_bits) == b.feet.map(f32::to_bits) && a.radius.to_bits() == b.radius.to_bits() && a.height.to_bits() == b.height.to_bits() && a.dead == b.dead
}

/// Whether two lists of part capsules are the same to the bit.
fn same_parts(a: &[PartCapsule], b: &[PartCapsule]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| x.part == y.part && x.a.map(f32::to_bits) == y.a.map(f32::to_bits) && x.b.map(f32::to_bits) == y.b.map(f32::to_bits) && x.radius.to_bits() == y.radius.to_bits())
}

/// In no group and colliding with none: queries (whose filter has no groups) still see it.
pub(crate) const NO_CONTACTS: InteractionGroups = InteractionGroups::new(Group::NONE, Group::NONE, InteractionTestMode::And);

/// Shapes are rebuilt only when their size changes by more than this, game units.
const RESIZE: f32 = 0.5;

impl PhysWorld {
    fn actor_body(&mut self, form_id: u32, v: HitVolume) -> RigidBodyHandle {
        let k = self.k;
        let f = Vector::new(v.feet[0], v.feet[1], v.feet[2]) * k;
        if let Some(a) = self.actors.get(&form_id) {
            let b = a.body;
            if let Some(body) = self.w.bodies.get_mut(b) {
                body.set_translation(f, true);
            }
            return b;
        }
        let owner = Owner::Actor { form_id, layer: v.layer(), part: WHOLE };
        let body = self.insert_body(RigidBodyBuilder::kinematic_position_based().translation(f).user_data(owner.encode()));
        let standalone = self.standalone_hit_volumes;
        self.actors.insert(form_id, ActorBody { body, volume: v, colliders: Vec::new(), parts: Vec::new(), standalone });
        body
    }

    /// Replace an actor's colliders when their number or parts differ from `want`.
    fn ensure_colliders(&mut self, form_id: u32, want: &[(u8, f32, f32)]) {
        let Some(a) = self.actors.get(&form_id) else { return };
        let same = a.colliders.len() == want.len() && a.colliders.iter().zip(want).all(|(c, w)| c.1 == w.0);
        if same {
            return;
        }
        let (body, standalone) = (a.body, a.standalone);
        let old: Vec<ColliderHandle> = a.colliders.iter().map(|c| c.0).collect();
        for c in old {
            self.w.remove_collider(c);
        }
        let k = self.k;
        // A standalone collider starts where an attached one would: rapier places a new
        // attached collider at `body pose * local pose`, the local pose being the identity
        // here [design: rapier3d 0.36.0 `dynamics/rigid_body_components.rs:1242`].
        let start = self.w.bodies.get(body).map(|b| *b.position() * Pose::IDENTITY);
        let mut cols = Vec::new();
        for &(part, half, radius) in want {
            let co = ColliderBuilder::capsule_z(half * k, radius.max(1.0) * k).collision_groups(NO_CONTACTS).solver_groups(NO_CONTACTS);
            let h = match (standalone, start) {
                (true, Some(pose)) => self.w.insert_collider(co.position(pose), None),
                _ => self.w.insert_collider(co, Some(body)),
            };
            cols.push((h, part, half, radius));
        }
        if let Some(a) = self.actors.get_mut(&form_id) {
            a.colliders = cols;
        }
    }

    /// Add the single hit capsule of actor reference `form_id`, or move and resize it
    /// (nothing changes when `v` is what it already is).
    pub fn set_actor_volume(&mut self, form_id: u32, v: HitVolume) {
        // A non-finite volume is ignored: the actor keeps the one it had (or none).
        if !v.is_finite() {
            return;
        }
        if self.actors.get(&form_id).is_some_and(|a| a.volume == v && a.colliders.len() == 1 && a.colliders[0].1 == WHOLE) {
            self.count_hitvol_writes(0, 1);
            return;
        }
        let k = self.k;
        let radius = v.radius.max(1.0);
        let half = (v.height.max(2.0 * radius) / 2.0 - radius).max(0.0);
        self.actor_body(form_id, v);
        self.ensure_colliders(form_id, &[(WHOLE, half, radius)]);
        let owner = Owner::Actor { form_id, layer: v.layer(), part: WHOLE }.encode();
        let c = v.center();
        let local = Pose::from_translation((Vector::new(c[0], c[1], c[2]) - Vector::new(v.feet[0], v.feet[1], v.feet[2])) * k);
        let a = self.actors.get_mut(&form_id).expect("added");
        a.volume = v;
        a.parts.clear();
        let (body, standalone) = (a.body, a.standalone);
        let (h, _, old_half, old_r) = a.colliders[0];
        a.colliders[0].2 = half;
        a.colliders[0].3 = radius;
        if let Some(co) = self.w.colliders.get_mut(h) {
            co.user_data = owner;
            if standalone {
                self.hitvol_pending.push((h, body, local));
            } else {
                co.set_position_wrt_parent(local);
            }
            if (old_half - half).abs() > RESIZE || (old_r - radius).abs() > RESIZE {
                co.set_shape(SharedShape::capsule_z(half * k, radius * k));
            }
        }
        self.count_hitvol_colliders(1);
        self.count_hitvol_writes(1, 0);
        self.mark_dirty(crate::world::DirtyBy::HitVolume);
    }

    /// One capsule per body part (step C8): `v` is the actor's overall extent (its body
    /// stands at `v.feet`); each part's capsule is placed on its segment.
    /// Parts whose segment or radius is not finite are left out.
    pub fn set_actor_parts(&mut self, form_id: u32, v: HitVolume, parts: &[PartCapsule]) {
        let finite: Vec<PartCapsule>;
        let parts = if parts.iter().all(PartCapsule::is_finite) {
            parts
        } else {
            finite = parts.iter().filter(|p| p.is_finite()).copied().collect();
            &finite[..]
        };
        if parts.is_empty() || !v.is_finite() {
            return self.set_actor_volume(form_id, v);
        }
        // Counted as before the skip below existed: every capsule handed in.
        self.count_hitvol_colliders(parts.len());
        // Bit-identical to the last write: nothing to write, nothing dirtied.
        if self.actors.get(&form_id).is_some_and(|a| same_volume(&a.volume, &v) && same_parts(&a.parts, parts) && a.colliders.len() == parts.len()) {
            self.count_hitvol_writes(0, parts.len() as u64);
            return;
        }
        let k = self.k;
        let feet = Vector::new(v.feet[0], v.feet[1], v.feet[2]);
        let want: Vec<(u8, f32, f32)> = parts
            .iter()
            .map(|p| {
                let len = (Vector::from(p.b) - Vector::from(p.a)).length();
                (p.part, (len / 2.0).max(0.0), p.radius)
            })
            .collect();
        self.actor_body(form_id, v);
        self.ensure_colliders(form_id, &want);
        let layer = v.layer();
        let a = self.actors.get_mut(&form_id).expect("added");
        a.volume = v;
        a.parts.clear();
        a.parts.extend_from_slice(parts);
        let (body, standalone) = (a.body, a.standalone);
        for (i, p) in parts.iter().enumerate() {
            let (h, _, old_half, old_r) = a.colliders[i];
            let (pa, pb) = (Vector::from(p.a), Vector::from(p.b));
            let mid = (pa + pb) * 0.5;
            let dir = (pb - pa).try_normalize().unwrap_or(Vector::Z);
            let local = Pose::from_parts((mid - feet) * k, Rotation::from_rotation_arc(Vector::Z, dir));
            let half = want[i].1;
            a.colliders[i].2 = half;
            a.colliders[i].3 = p.radius;
            if let Some(co) = self.w.colliders.get_mut(h) {
                co.user_data = Owner::Actor { form_id, layer, part: p.part }.encode_segment(i as u16);
                if standalone {
                    self.hitvol_pending.push((h, body, local));
                } else {
                    co.set_position_wrt_parent(local);
                }
                if (old_half - half).abs() > RESIZE || (old_r - p.radius).abs() > RESIZE {
                    co.set_shape(SharedShape::capsule_z(half * k, p.radius.max(1.0) * k));
                }
            }
        }
        self.count_hitvol_writes(parts.len() as u64, 0);
        self.mark_dirty(crate::world::DirtyBy::HitVolume);
    }

    /// Remove an actor's hit volume (it left the loaded area or was disabled).
    pub fn remove_actor_volume(&mut self, form_id: u32) -> bool {
        let Some(a) = self.actors.remove(&form_id) else { return false };
        // Standalone colliders go with it (attached ones go with the body).
        if a.standalone {
            for c in &a.colliders {
                self.w.remove_collider(c.0);
            }
        }
        self.remove_body(a.body);
        true
    }

    /// Whether actors whose hit volume is made from now on get colliders of no body (on, the
    /// default; see the module notes) or colliders attached to the actor's body (off, the
    /// representation before 2026-11-03). Actors that already have one keep theirs.
    pub fn set_standalone_hit_volumes(&mut self, on: bool) {
        self.standalone_hit_volumes = on;
    }

    /// Whether new actors' hit volumes get colliders of no body
    /// ([`PhysWorld::set_standalone_hit_volumes`]).
    pub fn standalone_hit_volumes(&self) -> bool {
        self.standalone_hit_volumes
    }

    /// Place the standalone capsules written since the last refresh or step where rapier
    /// would place them attached: `body pose * local pose`, with the body's pose now, the
    /// product rapier forms for an attached collider at that point
    /// [design: rapier3d 0.36.0 `pipeline/user_changes.rs:24`,
    /// `dynamics/rigid_body_components.rs:1274`].
    pub(crate) fn flush_hit_volumes(&mut self) {
        if self.hitvol_pending.is_empty() {
            return;
        }
        let _span = tracing::info_span!("phys::hitvol_flush").entered();
        let mut pending = std::mem::take(&mut self.hitvol_pending);
        for (c, b, local) in pending.drain(..) {
            let Some(body) = self.w.bodies.get(b) else { continue };
            let pose = *body.position() * local;
            if let Some(co) = self.w.colliders.get_mut(c) {
                co.set_position(pose);
            }
        }
        self.hitvol_pending = pending;
    }

    /// The actors that have a hit volume, by FormID.
    pub fn actor_volumes(&self) -> impl Iterator<Item = u32> + '_ {
        self.actors.keys().copied()
    }

    /// An actor's hit volume (its overall extent) as last set.
    pub fn actor_volume(&self, form_id: u32) -> Option<HitVolume> {
        self.actors.get(&form_id).map(|a| a.volume)
    }

    /// How many part capsules an actor has (0 with the single capsule).
    pub fn actor_part_count(&self, form_id: u32) -> usize {
        self.actors.get(&form_id).map_or(0, |a| a.colliders.iter().filter(|c| c.1 != WHOLE).count())
    }

    /// A point to aim at on an actor: with parts, the centre of its widest part capsule
    /// (the torso of a person), the first of equally wide ones (a part's own chain comes
    /// before its side branches, which share its radius: a creature's legs), else the
    /// middle of its capsule. Game units.
    pub fn actor_aim_point(&self, form_id: u32) -> Option<[f32; 3]> {
        let a = self.actors.get(&form_id)?;
        let widest = a.colliders.iter().filter(|c| c.1 != WHOLE).reduce(|best, c| if c.3 > best.3 { c } else { best });
        match widest.and_then(|c| self.w.colliders.get(c.0)) {
            Some(co) => {
                let t = co.position().translation / self.k;
                Some([t.x, t.y, t.z])
            }
            None => Some(a.volume.center()),
        }
    }

    /// The centres of an actor's body part capsules, widest first (none without parts),
    /// game units.
    pub fn actor_part_centers(&self, form_id: u32) -> Vec<[f32; 3]> {
        let Some(a) = self.actors.get(&form_id) else { return Vec::new() };
        let mut caps: Vec<(f32, [f32; 3])> = a
            .colliders
            .iter()
            .filter(|c| c.1 != WHOLE)
            .filter_map(|c| self.w.colliders.get(c.0).map(|co| (c.3, co.position().translation / self.k)))
            .map(|(r, t)| (r, [t.x, t.y, t.z]))
            .collect();
        caps.sort_by(|x, y| y.0.total_cmp(&x.0));
        caps.into_iter().map(|c| c.1).collect()
    }

    /// The centre of an actor's first capsule for body part `part`, game units.
    pub fn actor_part_center(&self, form_id: u32, part: u8) -> Option<[f32; 3]> {
        let c = self.actors.get(&form_id)?.colliders.iter().find(|c| c.1 == part && part != WHOLE)?;
        let t = self.w.colliders.get(c.0)?.position().translation / self.k;
        Some([t.x, t.y, t.z])
    }

    /// The body state of an actor's hit volume.
    pub fn actor_volume_state(&self, form_id: u32) -> Option<BodyState> {
        let a = self.actors.get(&form_id)?;
        Some(crate::world::body_state(&self.w.bodies[a.body], self.k))
    }
}
