//! Spent shell casings (Modern Feel, docs/design/modern-feel-vfx.md): small dynamic
//! cylinders thrown from a gun that tumble, bounce, roll and settle [design: NVForever's
//! own, not the original's; the original draws its casings without physics].
//!
//! - **Seen by nothing.** A casing's colliders carry [`CASING_TAG`] as their `user_data`,
//!   which [`Owner::decode`] reads as no owner: every query's [`crate::Filter`] passes
//!   over them (shots, the crosshair, activation, the camera, the ground under the feet),
//!   and characters walk through them (`character::solid_for`).
//! - **Contacts** [design]: with terrain, the still and loose collision of references
//!   and other casings; never with characters, actor hit volumes, ragdolls or gore
//!   (`crate::ragdoll::RagdollHooks`), and never with a reference collider on a layer
//!   the character passes through (triggers and the like).

use crate::convert::vec3;
use crate::world::{body_state, BodyState, Owner, PhysWorld};
use rapier3d::math::{Matrix, Pose, Rotation};
use rapier3d::prelude::*;

/// The `user_data` of a casing's body and colliders: a kind [`Owner::decode`] does not
/// know (it reads 1 to 4).
pub const CASING_TAG: u128 = 0xC5;

/// How to start a casing. Game units, row-major rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CasingStart {
    /// The engine's id for the casing.
    pub id: u32,
    pub translation: [f32; 3],
    pub rotation: [f32; 9],
    /// The cylinder: its half length along `axis` (in the casing's frame), its radius and
    /// its centre in the casing's frame.
    pub half_length: f32,
    pub radius: f32,
    pub axis: [f32; 3],
    pub center: [f32; 3],
    pub linvel: [f32; 3],
    /// Radians per second.
    pub angvel: [f32; 3],
    /// Kilograms.
    pub mass: f32,
}

/// Friction and bounce of brass on the world [design: tuned by eye; a casing hops two
/// or three times on stone and skitters a little].
pub const CASING_FRICTION: f32 = 0.55;
pub const CASING_RESTITUTION: f32 = 0.42;

/// Whether `user_data` is a casing's.
pub fn is_casing(user_data: u128) -> bool {
    user_data == CASING_TAG
}

impl PhysWorld {
    /// Add (or replace) a casing.
    pub fn add_casing(&mut self, s: &CasingStart) {
        self.remove_casing(s.id);
        let k = self.k;
        let m = Matrix::from_cols_array(&s.rotation).transpose();
        let q = Rotation::from_mat3(&m).normalize();
        let q = if q.is_finite() { q } else { Rotation::IDENTITY };
        let body = RigidBodyBuilder::dynamic()
            .pose(Pose::from_parts(vec3(s.translation) * k, q))
            .linvel(vec3(s.linvel) * k)
            .angvel(vec3(s.angvel))
            .linear_damping(0.05)
            // Rolling has no resistance of its own in rapier: the damping stands for it.
            .angular_damping(1.2)
            .ccd_enabled(true)
            .user_data(CASING_TAG);
        let h = self.insert_body(body);
        // Rapier's cylinder stands along its y: turn y onto the casing's axis.
        let axis = vec3(s.axis).try_normalize().unwrap_or(Vector::Y);
        let turn = Rotation::from_rotation_arc(Vector::Y, axis);
        let c = ColliderBuilder::cylinder((s.half_length * k).max(0.002), (s.radius * k).max(0.002))
            .position(Pose::from_parts(vec3(s.center) * k, turn))
            .friction(CASING_FRICTION)
            .restitution(CASING_RESTITUTION)
            .active_hooks(ActiveHooks::FILTER_CONTACT_PAIRS)
            .user_data(CASING_TAG);
        self.w.insert_collider(c, Some(h));
        if s.mass > 0.0 && s.mass.is_finite() {
            self.set_mass(h, s.mass);
        }
        self.casings.insert(s.id, h);
        self.mark_dirty(crate::world::DirtyBy::Bodies);
    }

    /// Remove a casing.
    pub fn remove_casing(&mut self, id: u32) -> bool {
        let Some(h) = self.casings.remove(&id) else { return false };
        self.remove_body(h);
        true
    }

    /// A casing's pose and velocity now.
    pub fn casing_state(&self, id: u32) -> Option<BodyState> {
        let b = self.w.bodies.get(*self.casings.get(&id)?)?;
        Some(body_state(b, self.k))
    }

    /// A casing as drawn: as [`PhysWorld::casing_state`], blended between the last two
    /// steps (`crate::drawn`).
    pub fn drawn_casing_state(&self, id: u32, alpha: f32) -> Option<BodyState> {
        self.drawn_state(*self.casings.get(&id)?, alpha)
    }

    /// How many casings are in the world.
    pub fn casing_count(&self) -> usize {
        self.casings.len()
    }

    /// Whether a contact between a casing and the collider whose owner is `other`
    /// (`other_data`: its `user_data`) is solved (see the module notes).
    pub(crate) fn casing_meets(other: Option<Owner>, other_data: u128) -> bool {
        match other {
            Some(Owner::Terrain { .. }) => true,
            Some(Owner::Ref { layer, .. }) => !crate::layers::character_ignores().contains(layer),
            Some(Owner::Character { .. } | Owner::Actor { .. }) => false,
            None => is_casing(other_data),
        }
    }
}
