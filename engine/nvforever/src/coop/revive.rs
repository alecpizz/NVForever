//! A downed player's body on the others' screens [observed: buckallobill, in game, FNV 1.4.0.525].
//!
//! - **Where it lies.** While a player is down, its puppet's pelvis here is where it is
//!   revived from: [`Peer::body`], told to the crosshair's targeting through
//!   `nvf_play::coop_mates::Mate::body`, so the "Revive" rollover is on the body where it
//!   slid or rolled to, not where the player stood when it went down.
//! - **Getting up.** Revived, its own game gets the player up from where its body lies
//!   with the knockdown's get-up clip (`player_knockdown.rs`). Here its puppet does the
//!   same from its own ragdoll's pose, as a knocked-out actor gets up (`knockouts.rs`):
//!   the ragdoll leaves the world, the get-up clip of the side it lies on plays once as a
//!   [`Dying`] clip from its first frame, laid so that the clip's pelvis and pelvis → head
//!   line are where the ragdoll's were, and when the clip is over the puppet follows its
//!   player again. Where its ragdoll came to rest here and where its player's own body
//!   got up differ (each game runs its own ragdoll): the difference is taken off over the
//!   second half of the clip, while the body rises, so the puppet ends on its player
//!   without a snap [design: adjust discrepancies while they get up, try not
//!   to snap/teleport] ([`BLEND_FROM`]). A corpse that stays down is left where it lies. It starts when the revived player's own body starts (its state stops
//!   saying `flag::LYING`), so both screens see it get up together. Without a body, a ragdoll pose or a clip, the puppet is drawn anew
//!   standing, as before. Log: `co-op: <name> gets up face up (<clip>, 2.3 s)`.

use super::{label, Coop, Puppet};
use crate::knockouts::{bone, face_up, pelvis_head};
use crate::limbs::ActorRig;
use crate::locomotion::{Dying, Locomotion};
use crate::ragdoll::{Corpse, Ragdolls};
use bevy::prelude::*;
use nvf_play::RefVisual;
use std::sync::Arc;

/// A revived player's puppet getting up ([`super::Peer::rising`]).
#[derive(Clone, Copy, Debug)]
pub(super) enum Rising {
    /// Revived at `since` (seconds of `Time`): its body still lies.
    Start { since: f32 },
    /// Its get-up clip starts this frame: the ragdoll's pelvis and head (world space).
    Align { pelvis: Vec3, head: Vec3, length: f32 },
    /// The clip plays from `start` for `length` seconds (of `Time`); `turn` and `shift`
    /// are what laying it over the body added to the puppet's standing transform, taken
    /// off from [`BLEND_FROM`] of the clip on.
    Up { start: f32, length: f32, turn: Quat, shift: Vec3 },
}

/// The share of the get-up clip after which the puppet is moved from where its body lay
/// toward its player, finished at the clip's end [design: the lying part of the clip is
/// not slid along the ground; the rising part hides the move].
const BLEND_FROM: f32 = 0.4;

/// How much of the lay-over is left at time `t` of a get-up of `length` seconds: 1 until
/// [`BLEND_FROM`] of it, then down to 0 at its end, eased.
fn left(t: f32, length: f32) -> f32 {
    if length <= 0.0 {
        return 0.0;
    }
    let u = ((t / length - BLEND_FROM) / (1.0 - BLEND_FROM)).clamp(0.0, 1.0);
    1.0 - u * u * (3.0 - 2.0 * u)
}

/// A get-up that has not started this long after the revive is given up: the puppet is
/// drawn anew, standing (seconds) [design: the revived game's body lies about 2.5 s
/// (observed: buckallobill, in game, FNV 1.4.0.525)].
const START_WAIT: f32 = 6.0;

/// The least a body lies here before it gets up (seconds) [design: the fallen player's
/// own ragdoll lies longer, about 2.5 s].
const MIN_LIE: f32 = 1.0;

type Body = (Entity, &'static RefVisual, &'static ActorRig, Option<&'static Locomotion>, &'static mut Transform, &'static GlobalTransform, Has<Corpse>);

/// Where each downed puppet's body lies, and the revived ones' get-ups (module notes).
#[allow(clippy::too_many_arguments)]
pub(super) fn bodies(
    mut commands: Commands,
    time: Res<Time>,
    mut coop: ResMut<Coop>,
    mut phys: Option<ResMut<nvf_play::phys::PhysState>>,
    mut ragdolls: ResMut<Ragdolls>,
    mut puppets: Query<Body, With<Puppet>>,
    globals: Query<&GlobalTransform>,
    parents: Query<&ChildOf>,
) {
    let now = time.elapsed_secs();
    for (&id, p) in coop.peers.iter_mut() {
        let found = p.root.and_then(|e| puppets.get_mut(e).ok());
        if p.down {
            p.body = found.as_ref().and_then(|(_, _, rig, ..)| bone(rig, &globals, "Bip01 Pelvis")).map(nvf_play::space::to_game);
            continue;
        }
        let Some(rising) = p.rising else { continue };
        let name = label(id, &p.name);
        let Some((e, v, rig, lo, mut t, gt, corpse)) = found else {
            if matches!(rising, Rising::Start { since } if now - since > START_WAIT) {
                info!("co-op: {name}'s body is not here to get up: drawn standing");
                p.rising = None;
                p.redraw = true;
            }
            continue;
        };
        match rising {
            // The revived player's own body still lies (`flag::LYING`): so does this one.
            // (At least [`MIN_LIE`]: it may start here before its first sample saying it
            // lies has come.)
            Rising::Start { since } if now - since < START_WAIT && (now - since < MIN_LIE || p.samples.back().is_some_and(|(_, s)| s.has(nvf_coop::net::flag::LYING))) => {}
            Rising::Start { since } => {
                let lying = pelvis_head(rig, &globals);
                let side = usize::from(face_up(rig, &globals) == Some(false));
                let clip: Option<Arc<_>> = lo.and_then(|lo| lo.set.getup[side].clone().or_else(|| lo.set.getup[1 - side].clone()));
                let (Some(clip), Some((pelvis, head)), true) = (clip, lying, corpse) else {
                    if now - since > START_WAIT || !corpse {
                        info!("co-op: {name} stands up (no ragdoll or get-up clip): drawn standing");
                        p.rising = None;
                        p.redraw = true;
                    }
                    continue;
                };
                if let Some(ph) = phys.as_deref_mut() {
                    ph.world.remove_ragdoll(v.0);
                }
                ragdolls.rest.remove(&v.0);
                commands.entity(e).remove::<Corpse>();
                // Stood where its player is, then laid over the body ([`Rising::Align`]).
                if let Some(&(_, s)) = p.samples.back() {
                    t.translation = Vec3::from_array(s.feet);
                    t.rotation = Quat::from_rotation_z(-s.yaw);
                }
                let length = clip.clip.length();
                info!("co-op: {name} gets up face {} ({}, {length:.1} s)", if side == 0 { "up" } else { "down" }, clip.clip.name);
                commands.entity(e).insert(Dying { clip: Some(clip), t: 0.0 });
                p.rising = Some(Rising::Align { pelvis, head, length });
            }
            Rising::Align { pelvis, head, length } => {
                // The clip's first pose is on the bones: lay it over where the body lay
                // (`knockouts.rs`, the same turn and shift).
                let (mut turn, mut shift_local) = (Quat::IDENTITY, Vec3::ZERO);
                if let Some((pl, hd)) = pelvis_head(rig, &globals) {
                    let flat = |v: Vec3| Vec2::new(v.x, v.z);
                    let (want, got) = (flat(head - pelvis), flat(hd - pl));
                    if want.length() > 1.0 && got.length() > 1.0 {
                        let angle = got.angle_to(want);
                        let spin = Quat::from_rotation_y(-angle);
                        let origin = gt.translation();
                        let moved = origin + spin * (pl - origin);
                        let shift = Vec3::new(pelvis.x - moved.x, 0.0, pelvis.z - moved.z);
                        let to_local = parents.get(e).ok().and_then(|c| globals.get(c.parent()).ok()).map_or(bevy::math::Affine3A::IDENTITY, |g| g.affine().inverse());
                        let axis = to_local.transform_vector3(Vec3::Y).normalize_or(Vec3::Z);
                        turn = Quat::from_axis_angle(axis, -angle);
                        shift_local = to_local.transform_vector3(shift);
                        t.rotation = (turn * t.rotation).normalize();
                        t.translation += shift_local;
                        info!("co-op: {name} laid over its body: turned {:.0}°, moved {:.0} units", angle.to_degrees(), shift.length());
                    }
                }
                p.rising = Some(Rising::Up { start: now, length, turn, shift: shift_local });
            }
            Rising::Up { start, length, turn, shift } => {
                // Toward where its player stands now (as the puppet will be drawn when
                // the clip ends), the lay-over taken off as the body rises.
                if let Some((_, feet, yaw, _)) = p.pose(now - super::DELAY) {
                    let k = left(now - start, length);
                    t.rotation = (Quat::IDENTITY.slerp(turn, k) * Quat::from_rotation_z(-yaw)).normalize();
                    t.translation = Vec3::from_array(feet) + shift * k;
                }
                if now >= start + length {
                    commands.entity(e).remove::<Dying>();
                    p.rising = None;
                    info!("co-op: {name} is up: follows its player again");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lay-over stays while the body lies, then goes smoothly to nothing by the
    /// clip's end: no jump when the puppet follows its player again.
    #[test]
    fn the_lay_over_is_taken_off_while_the_body_rises() {
        assert_eq!(left(0.0, 2.0), 1.0);
        assert_eq!(left(0.8, 2.0), 1.0, "still lying: not slid along the ground");
        let mid = left(1.4, 2.0);
        assert!(mid > 0.0 && mid < 1.0, "half way through the rise: {mid}");
        assert!(left(1.9, 2.0) < 0.05, "nearly there at the end");
        assert_eq!(left(2.0, 2.0), 0.0, "on its player when the clip ends");
        assert_eq!(left(5.0, 2.0), 0.0);
        let mut last = 1.0;
        for i in 0..=40 {
            let k = left(i as f32 * 0.05, 2.0);
            assert!(k <= last, "never back");
            assert!(last - k < 0.1, "no step larger than a tenth in 0.05 s");
            last = k;
        }
    }
}
