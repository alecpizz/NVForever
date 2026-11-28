//! Actors in the player's way (docs/spec-notes/physics.md §6.8): a living, drawn actor
//! blocks the player where its body is now: on its feet, walking, on a stool or in bed.
//!
//! An actor has no collider for this [design]. Its hit volumes are for shots
//! (`crate::hitvol`; the character controller passes through them), and the bodies of its
//! skeleton model are the actor's own, not the cell's (`fnv_phys::placed`: added at the
//! placement they stayed where the actor was placed). Each frame `crate::hitvol` works
//! out a [`Block`] per actor from what is drawn, and the player's wanted velocity is
//! clipped against the blocks before the character controller moves it ([`clip`]): the
//! player stops at an actor and slides round it, and one caught inside (an actor walked
//! or sat down into the player) is let out and moved out. Nothing here can wedge the
//! character, and rays never meet a block.
//!
//! - **Where** [design]: with posed body parts (`crate::hitvol::ActorParts`), the line on
//!   the ground under the torso's bones, pelvis to neck: a point under a standing or
//!   sitting person, the length of the body under one lying down or a four-legged
//!   creature. Without them, the middle of the actor's render bounds.
//! - **How wide** [design]: [`RADIUS_SHARE`] of the bounds' height round that line (22
//!   units for a person 128 tall, the player's own radius), within [`MIN_RADIUS`] and
//!   [`MAX_RADIUS`]. The bounds' width is not used: it changes as the actor turns.
//!   A walking creature's circle instead uses at least its scaled skeleton BBX
//!   half-width (`walking_radius`), independently of its world capsule [hypothesis:
//!   capture run15b_b2 spacing, 2026-11-15; design, #2535].
//!   When it stops and loses its walking character, `hitvol::sync_volumes` keeps
//!   that wider circle at its root feet instead of reverting to the posed torso
//!   block [design, #2535].
//! - **Who is left out**: the dead. An actor the character controller walks
//!   (`PhysState::npcs`) has its block where its character is, made anew every fixed
//!   step (`crate::player`), and so has the player for those actors: characters pass
//!   through each other's bodies (`Character::passes_characters`) and this rule alone
//!   keeps them apart. (Left to the controller, a character was carried along by any
//!   other that walked past it within 3.5 units, as on a moving platform.)
//!
//! **Pushing** [design; how far the game's actors move each other is the capture
//! `pc_actor_push`, physics.md §6.8]: an actor that stands still gives way to one that
//! walks into it. Whoever moves the actors says who stands where
//! ([`ActorBlocks::standing`]); such a block is `pushable`, and a walker coming at it
//! keeps [`PUSH_SHARE`] of the speed it comes at it with, the standing actor moving
//! ahead of it at that speed ([`Push`], taken by the actors' owner, which moves the
//! actor as far as it has room). The walkers are the player and the actors the
//! character controller walks; the player's block gives way to those actors when the
//! player stands still on the ground (`crate::player`). An actor on furniture, asleep
//! or on its way is not pushed.
//!
//! Not here: an actor that walks on the navmesh surface (no character) is not stopped
//! by a block and pushes no one; actors are kept out of each other afterwards
//! (`fnv_ai::apart`, run by `nvforever`'s residents; `--actors-pass-through` off).

use crate::hitvol::PartSegment;
use bevy::prelude::*;
use fnv_combat::data::Limb;
use std::collections::HashMap;

/// A block's radius as a share of the actor's height [design].
pub const RADIUS_SHARE: f32 = 0.17;
/// Radius limits, game units [design].
pub const MIN_RADIUS: f32 = 10.0;
pub const MAX_RADIUS: f32 = 60.0;
/// The speed at which a player inside a block is moved out of it, game units per second
/// [design: about a walk].
pub const PUSH_OUT: f32 = 80.0;
/// Of the speed at which a walker comes at a standing actor, the share at which the two
/// then move on together [design: a placeholder until the capture `pc_actor_push` says
/// how fast the game's player pushes an actor].
pub const PUSH_SHARE: f32 = 0.5;
/// A pushed player that got less than this share of the way it was pushed is held by
/// something (a wall): it is not pushed again for [`HELD_SECS`] seconds of fixed steps
/// [design: half a second, which was 30 steps at 60 a second; in seconds so it lasts as
/// long at any physics rate].
pub const HELD_SHARE: f32 = 0.5;
pub const HELD_SECS: f32 = 0.5;
/// The player as the actor of a [`Block`] or a [`Push`] (its reference's FormID).
pub const PLAYER: u32 = fnv_sim::FormId::PLAYER_REF.0;

/// Widen only actor blocking, leaving the world capsule and smaller defaults intact
/// [hypothesis: BBX half-width fits capture run15b_b2 spacing, 2026-11-15; design,
/// #2535: `hitvol` supplies scaled widths only for creatures].
pub fn walking_radius(current: f32, bbx: Option<f32>) -> f32 {
    bbx.filter(|r| r.is_finite()).map_or(current, |r| current.max(r))
}

/// Where an actor is in the player's way: `radius` round the line `a`–`b` on the ground,
/// from `bottom` to `top`. Game units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    pub actor: u32,
    pub a: [f32; 2],
    pub b: [f32; 2],
    pub radius: f32,
    pub bottom: f32,
    pub top: f32,
    /// Standing where a walker pushes it aside ([`ActorBlocks::standing`]).
    pub pushable: bool,
}

/// A standing actor pushed aside by a walker: how far (x, y), game units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Push {
    pub actor: u32,
    pub by: [f32; 2],
}

/// The actors in the player's way this frame (`crate::hitvol` fills it).
#[derive(Resource, Default)]
pub struct ActorBlocks {
    pub list: Vec<Block>,
    /// Fixed steps in which a block changed the player's velocity (for the exit report).
    pub steps_blocked: u64,
    /// Fixed steps in which a block changed the velocity of an actor the character
    /// controller walks.
    pub steps_actor_blocked: u64,
    /// The actors that may be pushed aside and where each stands (x, y of its feet), as
    /// whoever moves the actors has them (`nvforever`'s residents); empty: nobody is
    /// pushed.
    pub standing: HashMap<u32, [f32; 2]>,
    /// Pushes the actors' owner has not taken yet ([`ActorBlocks::take_pushes`]).
    pub pushes: Vec<Push>,
    /// Fixed steps in which the player pushed an actor, in which a walking actor pushed
    /// another, and in which one pushed the player (for the exit report).
    pub steps_pushing: [u64; 3],
    /// Seconds of fixed steps for which the player is not pushed: something held it when
    /// an actor last pushed it (`crate::player`; each step takes its `dt` off, and it is
    /// over once under half a step is left, so the steps' rounding never adds one).
    pub player_held: f32,
    /// Actors playing an attack clip (nvforever `attack_anims`): their blocks stay where
    /// they were when the attack began, so a lunge moves nobody [design; owner
    /// 2026-10-11: a dog gets into range and bites, it does not push the player around].
    pub attacking: std::collections::HashSet<u32>,
    /// Actors their attack clip carries (nvforever `attack_anims`, `residents`): their
    /// characters are stopped by the blocks as any walk is, but push nobody aside
    /// ([`clip_pushing_nobody`]) [design: a lunge stops at the player, as a bite does
    /// not push the player around, owner 2026-10-11].
    pub carried: std::collections::HashSet<u32>,
    /// Actors that are in nobody's way: no block (perched birds, nvforever `residents`;
    /// docs/design/actor-behaviour.md §5.8).
    pub in_nobodys_way: std::collections::HashSet<u32>,
}

impl ActorBlocks {
    /// Where a standing actor is with the pushes its owner has not taken yet.
    pub fn standing_now(&self, actor: u32) -> Option<[f32; 2]> {
        let at = *self.standing.get(&actor)?;
        Some(self.pushes.iter().filter(|p| p.actor == actor).fold(at, |at, p| [at[0] + p.by[0], at[1] + p.by[1]]))
    }

    /// The pushes since the last call, summed per actor.
    pub fn take_pushes(&mut self) -> HashMap<u32, [f32; 2]> {
        let mut sum: HashMap<u32, [f32; 2]> = HashMap::new();
        for p in self.pushes.drain(..) {
            let s = sum.entry(p.actor).or_default();
            s[0] += p.by[0];
            s[1] += p.by[1];
        }
        sum
    }
}

/// The ends of an actor's torso: the first and last joint of its first torso part.
pub fn torso(segments: &[PartSegment]) -> Option<([f32; 3], [f32; 3])> {
    let first = segments.iter().find(|s| s.limb == Limb::Torso)?;
    let last = segments.iter().rfind(|s| s.limb == Limb::Torso && s.part == first.part)?;
    Some((first.a, last.b))
}

/// The block of an actor with the game-space bounds `lo`–`hi` and, when it has posed
/// parts, the ends of its torso (see the module notes).
pub fn block_for(actor: u32, lo: [f32; 3], hi: [f32; 3], torso: Option<([f32; 3], [f32; 3])>) -> Block {
    let middle = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let (a, b) = torso.map_or((middle, middle), |(a, b)| ([a[0], a[1]], [b[0], b[1]]));
    Block { actor, a, b, radius: (RADIUS_SHARE * (hi[2] - lo[2])).clamp(MIN_RADIUS, MAX_RADIUS), bottom: lo[2], top: hi[2], pushable: false }
}

impl Block {
    /// The block of a character with its feet at `feet`.
    pub fn character(actor: u32, feet: [f32; 3], radius: f32, height: f32, pushable: bool) -> Block {
        let at = [feet[0], feet[1]];
        Block { actor, a: at, b: at, radius, bottom: feet[2], top: feet[2] + height, pushable }
    }

    /// The block as one that is pushed, with the middle of its line at `at` (where the
    /// actor's owner has it standing: the drawn pose follows a frame or two later).
    pub fn standing_at(mut self, at: [f32; 2]) -> Block {
        let half = [(self.b[0] - self.a[0]) / 2.0, (self.b[1] - self.a[1]) / 2.0];
        self.a = [at[0] - half[0], at[1] - half[1]];
        self.b = [at[0] + half[0], at[1] + half[1]];
        self.pushable = true;
        self
    }

    fn shift(&mut self, by: [f32; 2]) {
        self.a = [self.a[0] + by[0], self.a[1] + by[1]];
        self.b = [self.b[0] + by[0], self.b[1] + by[1]];
    }

    /// The point of the block's line nearest to `p`.
    fn nearest(&self, p: [f32; 2]) -> [f32; 2] {
        let d = [self.b[0] - self.a[0], self.b[1] - self.a[1]];
        let len2 = d[0] * d[0] + d[1] * d[1];
        let t = if len2 > 1e-6 { (((p[0] - self.a[0]) * d[0] + (p[1] - self.a[1]) * d[1]) / len2).clamp(0.0, 1.0) } else { 0.0 };
        [self.a[0] + d[0] * t, self.a[1] + d[1] * t]
    }

    /// How far a character of `radius` at `p` is from touching the block (negative
    /// inside it), heights aside.
    pub fn room(&self, p: [f32; 2], radius: f32) -> f32 {
        let c = self.nearest(p);
        (p[0] - c[0]).hypot(p[1] - c[1]) - (self.radius + radius)
    }
}

/// The horizontal velocity a character with its feet at `feet`, of `radius` and `height`,
/// may have for a step of `dt` seconds when it wants `velocity`: towards a block no
/// closer than touching it, sliding along it; inside one, out of it at [`PUSH_OUT`] at
/// least (or as fast as takes it out in this step). A `pushable` block gives way: of the
/// speed the character has too much towards it, [`PUSH_SHARE`] is kept and the block
/// moves ahead at that speed (it is moved here, and the move added to `pushes`). The
/// block of `me`, the character itself, is left out.
#[allow(clippy::too_many_arguments)]
pub fn clip(me: u32, velocity: [f32; 2], feet: [f32; 3], radius: f32, height: f32, dt: f32, blocks: &mut [Block], pushes: &mut Vec<Push>) -> [f32; 2] {
    clip_inner(me, velocity, feet, radius, height, dt, blocks, Some(pushes))
}

/// [`clip`] for a walker that pushes nobody aside ([`ActorBlocks::carried`]): every
/// block stops it.
#[allow(clippy::too_many_arguments)]
pub fn clip_pushing_nobody(me: u32, velocity: [f32; 2], feet: [f32; 3], radius: f32, height: f32, dt: f32, blocks: &mut [Block]) -> [f32; 2] {
    clip_inner(me, velocity, feet, radius, height, dt, blocks, None)
}

#[allow(clippy::too_many_arguments)]
fn clip_inner(me: u32, velocity: [f32; 2], feet: [f32; 3], radius: f32, height: f32, dt: f32, blocks: &mut [Block], mut pushes: Option<&mut Vec<Push>>) -> [f32; 2] {
    let mut v = velocity;
    let mut speed = v[0].hypot(v[1]);
    // Twice: sliding along one block can lead into the next. If the first pass made no
    // contact at all, the velocity and blocks are unchanged and the second pass is a no-op.
    for _ in 0..2 {
        let mut touched = false;
        let mut step_reach = speed * dt;
        for b in blocks.iter_mut() {
            if b.actor == me || feet[2] >= b.top || feet[2] + height <= b.bottom {
                continue;
            }
            // Cheap axis-aligned rejection before projecting onto the block's torso segment:
            // if the point lies beyond `reach` on either axis, the Euclidean distance to
            // any point on segment `a..=b` is strictly greater than `reach`.
            let reach = b.radius + radius + step_reach;
            let (min_x, max_x) = if b.a[0] <= b.b[0] { (b.a[0], b.b[0]) } else { (b.b[0], b.a[0]) };
            if feet[0] < min_x - reach || feet[0] > max_x + reach {
                continue;
            }
            let (min_y, max_y) = if b.a[1] <= b.b[1] { (b.a[1], b.b[1]) } else { (b.b[1], b.a[1]) };
            if feet[1] < min_y - reach || feet[1] > max_y + reach {
                continue;
            }
            let c = b.nearest([feet[0], feet[1]]);
            let d = [feet[0] - c[0], feet[1] - c[1]];
            let dist = d[0].hypot(d[1]);
            let room = dist - (b.radius + radius);
            if room > step_reach {
                continue;
            }
            // Away from the block; standing on its line, the way the character goes.
            let n = if dist > 1e-3 {
                [d[0] / dist, d[1] / dist]
            } else if speed > 1e-3 {
                [v[0] / speed, v[1] / speed]
            } else {
                [1.0, 0.0]
            };
            let towards = -(v[0] * n[0] + v[1] * n[1]);
            let most = if room < 0.0 { -(-room / dt).min(PUSH_OUT) } else { room / dt };
            if towards > most {
                touched = true;
                let mut stop = towards - most;
                if b.pushable {
                    if let Some(pushes) = pushes.as_deref_mut() {
                        let give = PUSH_SHARE * stop;
                        let by = [-n[0] * give * dt, -n[1] * give * dt];
                        b.shift(by);
                        pushes.push(Push { actor: b.actor, by });
                        stop -= give;
                    }
                }
                v[0] += n[0] * stop;
                v[1] += n[1] * stop;
                speed = v[0].hypot(v[1]);
                step_reach = speed * dt;
            }
        }
        if !touched {
            break;
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn person(at: [f32; 2]) -> Block {
        block_for(7, [at[0] - 60.0, at[1] - 15.0, 0.0], [at[0] + 60.0, at[1] + 15.0, 128.0], Some(([at[0], at[1], 70.0], [at[0], at[1], 110.0])))
    }

    /// [`super::clip`] against blocks that stay as they are.
    fn clip(velocity: [f32; 2], feet: [f32; 3], radius: f32, height: f32, dt: f32, blocks: &[Block]) -> [f32; 2] {
        let mut pushes = Vec::new();
        let v = super::clip(PLAYER, velocity, feet, radius, height, dt, &mut blocks.to_vec(), &mut pushes);
        assert!(pushes.is_empty());
        v
    }

    /// Walk a character of radius 22 from `from` with the wanted velocity for `steps`.
    fn walk(mut feet: [f32; 3], want: [f32; 2], steps: usize, blocks: &[Block]) -> [f32; 3] {
        for _ in 0..steps {
            let v = clip(want, feet, 22.0, 128.0, DT, blocks);
            feet[0] += v[0] * DT;
            feet[1] += v[1] * DT;
        }
        feet
    }

    #[test]
    fn a_person_is_as_wide_as_the_player_however_it_is_turned() {
        let b = person([0.0, 0.0]);
        assert!((b.radius - 21.76).abs() < 0.01);
        // Turned a quarter: the bounds are wide the other way, the block is the same.
        let turned = block_for(7, [-15.0, -60.0, 0.0], [15.0, 60.0, 128.0], Some(([0.0, 0.0, 70.0], [0.0, 0.0, 110.0])));
        assert_eq!((b.radius, b.a, b.b), (turned.radius, turned.a, turned.b));
        // Small and huge creatures keep to the limits.
        assert_eq!(block_for(1, [0.0; 3], [10.0, 10.0, 20.0], None).radius, MIN_RADIUS);
        assert_eq!(block_for(1, [0.0; 3], [10.0, 10.0, 900.0], None).radius, MAX_RADIUS);
    }

    #[test]
    fn the_block_is_under_the_torso_not_the_bounds() {
        // Sat on a stool 40 units from where the bounds' middle is.
        let b = block_for(7, [-60.0, -15.0, 0.0], [60.0, 15.0, 128.0], Some(([40.0, 2.0, 50.0], [42.0, 2.0, 90.0])));
        assert_eq!((b.a, b.b), ([40.0, 2.0], [42.0, 2.0]));
        // No posed parts: the bounds' middle.
        let b = block_for(7, [-60.0, -15.0, 0.0], [60.0, 15.0, 128.0], None);
        assert_eq!((b.a, b.b), ([0.0, 0.0], [0.0, 0.0]));
    }

    #[test]
    fn the_torso_is_the_first_torso_part() {
        let seg = |part, limb, a: [f32; 3], b: [f32; 3]| PartSegment { part, limb, a, b, radius: None };
        let segs = [
            seg(0, Limb::Head, [0.0, 0.0, 112.0], [0.0, 0.0, 128.0]),
            seg(1, Limb::Torso, [1.0, 0.0, 70.0], [2.0, 0.0, 90.0]),
            seg(1, Limb::Torso, [2.0, 0.0, 90.0], [3.0, 0.0, 110.0]),
            seg(4, Limb::Torso, [9.0, 9.0, 9.0], [9.0, 9.0, 9.0]),
        ];
        assert_eq!(torso(&segs), Some(([1.0, 0.0, 70.0], [3.0, 0.0, 110.0])));
        assert_eq!(torso(&segs[..1]), None);
    }

    #[test]
    fn creature_walking_blocks_widen_both_sides_but_not_world_capsules() {
        // Synthetic bound/height; the capsule still follows npc_params [design, #2535].
        let params = crate::phys::npc_params(Some(128.0));
        let small = crate::phys::npc_params(Some(30.0));
        assert_eq!(params.radius, 22.0);
        assert_eq!(walking_radius(params.radius, None), params.radius);
        assert_eq!(walking_radius(small.radius, Some(3.0)), small.radius);
        assert_eq!(walking_radius(params.radius, Some(f32::NAN)), params.radius);
        let wide = walking_radius(params.radius, Some(70.0));
        assert_eq!(wide, 70.0);
        // Each can be the moving actor: creature/creature and player/creature.
        for (me, other) in [(42.0, wide), (wide, 42.0), (22.0, wide), (wide, 22.0)] {
            let b = Block::character(7, [300.0, 0.0, 0.0], other, 128.0, false);
            let v = clip([300.0, 0.0], [0.0; 3], me, 128.0, 1.0, &[b]);
            assert!((300.0 - v[0] - (me + other)).abs() < 1e-4);
            assert_eq!(params.radius, 22.0);
        }
    }

    #[test]
    fn walking_at_a_person_stops_where_they_touch() {
        let blocks = [person([100.0, 0.0])];
        let reach = blocks[0].radius + 22.0;
        let end = walk([0.0, 0.0, 0.0], [300.0, 0.0], 120, &blocks);
        assert!((end[0] - (100.0 - reach)).abs() < 0.01 && end[1] == 0.0, "{end:?}");
        // Where the person is not, nothing stops the walk.
        let free = walk([0.0, 200.0, 0.0], [300.0, 0.0], 60, &blocks);
        assert!((free[0] - 300.0).abs() < 0.01);
    }

    #[test]
    fn walking_past_slides_round() {
        let blocks = [person([100.0, 0.0])];
        let reach = blocks[0].radius + 22.0;
        // A little to the side of the middle: round the person and on.
        let end = walk([0.0, 10.0, 0.0], [300.0, 0.0], 240, &blocks);
        assert!(end[0] > 150.0 && end[1] >= reach - 0.01, "{end:?}");
        // Never closer than touching on the way.
        let mut feet = [0.0, 10.0, 0.0];
        for _ in 0..240 {
            feet = walk(feet, [300.0, 0.0], 1, &blocks);
            assert!((feet[0] - 100.0).hypot(feet[1]) >= reach - 0.01, "{feet:?}");
        }
    }

    #[test]
    fn above_and_below_is_free() {
        let blocks = [person([100.0, 0.0])];
        assert_eq!(clip([300.0, 0.0], [60.0, 0.0, 128.0], 22.0, 128.0, DT, &blocks), [300.0, 0.0]);
        assert_eq!(clip([300.0, 0.0], [60.0, 0.0, -128.0], 22.0, 128.0, DT, &blocks), [300.0, 0.0]);
    }

    #[test]
    fn caught_inside_gets_out() {
        let blocks = [person([100.0, 0.0])];
        let reach = blocks[0].radius + 22.0;
        // Standing still 10 units from the person: moved out, away from it, and no further.
        let v = clip([0.0, 0.0], [90.0, 0.0, 0.0], 22.0, 128.0, DT, &blocks);
        assert_eq!(v, [-PUSH_OUT, 0.0]);
        let end = walk([90.0, 0.0, 0.0], [0.0, 0.0], 120, &blocks);
        assert!((end[0] - (100.0 - reach)).abs() < 0.01, "{end:?}");
        // Walking out is as fast as wanted; walking further in is not possible.
        assert_eq!(clip([-300.0, 0.0], [90.0, 0.0, 0.0], 22.0, 128.0, DT, &blocks), [-300.0, 0.0]);
        assert!(clip([300.0, 0.0], [90.0, 0.0, 0.0], 22.0, 128.0, DT, &blocks)[0] <= -PUSH_OUT + 0.01);
        // On the very spot: out the way the character goes.
        let v = clip([0.0, 50.0], [100.0, 0.0, 0.0], 22.0, 128.0, DT, &blocks);
        assert!(v[0].abs() < 1e-4 && v[1] >= PUSH_OUT - 0.01, "{v:?}");
    }

    #[test]
    fn a_lying_body_blocks_along_its_length() {
        // A torso lying along y from 0 to 60.
        let b = block_for(7, [-20.0, -10.0, 0.0], [20.0, 70.0, 128.0], Some(([0.0, 0.0, 30.0], [0.0, 60.0, 30.0])));
        let reach = b.radius + 22.0;
        for y in [0.0, 30.0, 60.0] {
            let end = walk([-200.0, y, 0.0], [300.0, 0.0], 120, &[b]);
            assert!((end[0] + reach).abs() < 0.01, "{y}: {end:?}");
        }
        // Past its end the way is free.
        let end = walk([-200.0, 60.0 + reach + 1.0, 0.0], [300.0, 0.0], 120, &[b]);
        assert!(end[0] > 300.0, "{end:?}");
    }

    /// Walk at blocks that are pushed, moving them as their owner would: by every push.
    fn push_walk(mut feet: [f32; 3], want: [f32; 2], steps: usize, blocks: &mut [Block]) -> ([f32; 3], Vec<Push>) {
        let mut all = Vec::new();
        for _ in 0..steps {
            let v = super::clip(PLAYER, want, feet, 22.0, 128.0, DT, blocks, &mut all);
            feet[0] += v[0] * DT;
            feet[1] += v[1] * DT;
        }
        (feet, all)
    }

    #[test]
    fn a_standing_actor_is_pushed_ahead_at_a_share_of_the_speed() {
        let mut blocks = [person([100.0, 0.0]).standing_at([100.0, 0.0])];
        let reach = blocks[0].radius + 22.0;
        // 300 units a second for 2 s: 0.19 s up to the actor, then the two go on together.
        let (end, pushes) = push_walk([0.0, 0.0, 0.0], [300.0, 0.0], 120, &mut blocks);
        let touching = (100.0 - reach) / 300.0;
        let expect = 100.0 - reach + (2.0 - touching) * 300.0 * PUSH_SHARE;
        assert!((end[0] - expect).abs() < 1.0 && end[1] == 0.0, "{end:?} {expect}");
        // The actor is as far ahead as when they touched, and the pushes add up to its way.
        assert!((blocks[0].a[0] - end[0] - reach).abs() < 0.01, "{:?} {end:?}", blocks[0]);
        let sum: f32 = pushes.iter().map(|p| p.by[0]).sum();
        assert!((sum - (blocks[0].a[0] - 100.0)).abs() < 0.01 && pushes.iter().all(|p| p.actor == 7 && p.by[1] == 0.0));
        // Once the walker stops, so does the actor.
        let before = blocks[0];
        let (_, pushes) = push_walk(end, [0.0, 0.0], 30, &mut blocks);
        assert!(pushes.is_empty() && blocks[0] == before);
    }

    #[test]
    fn an_actor_pushed_off_centre_goes_aside_and_the_walker_past() {
        let mut blocks = [person([100.0, 0.0]).standing_at([100.0, 0.0])];
        let (end, _) = push_walk([0.0, 12.0, 0.0], [300.0, 0.0], 180, &mut blocks);
        // The actor went ahead and to the far side; the walker is past where it stood.
        assert!(blocks[0].a[0] > 100.0 && blocks[0].a[1] < 0.0, "{:?}", blocks[0]);
        assert!(end[0] > 200.0 && end[1] >= 12.0, "{end:?}");
        // Never closer than touching.
        assert!(blocks[0].room([end[0], end[1]], 22.0) >= -0.01);
    }

    #[test]
    fn pushes_wait_for_their_owner() {
        let mut blocks = ActorBlocks::default();
        assert_eq!(blocks.standing_now(7), None);
        blocks.standing.insert(7, [100.0, 0.0]);
        blocks.pushes.extend([Push { actor: 7, by: [2.0, 0.5] }, Push { actor: 9, by: [1.0, 0.0] }, Push { actor: 7, by: [1.0, 0.0] }]);
        // Until the owner has moved the actor, it counts as standing where the pushes put it.
        assert_eq!(blocks.standing_now(7), Some([103.0, 0.5]));
        let taken = blocks.take_pushes();
        assert_eq!((taken[&7], taken[&9]), ([3.0, 0.5], [1.0, 0.0]));
        assert!(blocks.pushes.is_empty() && blocks.standing_now(7) == Some([100.0, 0.0]));
    }

    #[test]
    fn a_pushed_block_keeps_its_shape_round_where_it_stands() {
        // A creature's strip along x, as drawn round (0, 0); it stands at (50, 10) now.
        let b = block_for(3, [-40.0, -10.0, 0.0], [40.0, 10.0, 60.0], Some(([-30.0, 0.0, 30.0], [30.0, 0.0, 30.0]))).standing_at([50.0, 10.0]);
        assert_eq!((b.a, b.b, b.pushable), ([20.0, 10.0], [80.0, 10.0], true));
        let c = Block::character(PLAYER, [1.0, 2.0, 3.0], 22.0, 128.9, true);
        assert_eq!((c.a, c.b, c.bottom, c.top, c.pushable), ([1.0, 2.0], [1.0, 2.0], 3.0, 131.9, true));
    }

    #[test]
    fn a_character_does_not_meet_its_own_block() {
        // Two characters 30 units apart (inside each other): each is moved out of the
        // other's block, not out of its own.
        let mut blocks = [Block::character(1, [0.0, 0.0, 0.0], 22.0, 128.0, false), Block::character(2, [30.0, 0.0, 0.0], 22.0, 128.0, false)];
        let mut pushes = Vec::new();
        assert_eq!(super::clip(1, [0.0, 0.0], [0.0, 0.0, 0.0], 22.0, 128.0, DT, &mut blocks, &mut pushes), [-PUSH_OUT, 0.0]);
        assert_eq!(super::clip(2, [0.0, 0.0], [30.0, 0.0, 0.0], 22.0, 128.0, DT, &mut blocks, &mut pushes), [PUSH_OUT, 0.0]);
        // Alone with its own block, nothing happens.
        assert_eq!(super::clip(1, [50.0, 0.0], [0.0, 0.0, 0.0], 22.0, 128.0, DT, &mut blocks[..1], &mut pushes), [50.0, 0.0]);
        assert!(pushes.is_empty());
    }

    #[test]
    fn clip_aabb_reject_and_no_push_match_exact_two_pass_semantics() {
        // Mix distant blocks, overlapping block, angled corner pair (requiring second pass),
        // and pushable block; verify both `clip` and `clip_pushing_nobody`.
        let initial = vec![
            Block::character(10, [5000.0, 5000.0, 0.0], 22.0, 128.0, true),
            Block::character(11, [-4000.0, 10.0, 0.0], 22.0, 128.0, false),
            Block { actor: 12, a: [44.0, -20.0], b: [44.0, 20.0], radius: 22.0, bottom: 0.0, top: 128.0, pushable: true },
            Block::character(13, [0.0, 44.0, 0.0], 22.0, 128.0, false),
            Block::character(14, [12.0, 0.0, 0.0], 22.0, 128.0, true),
        ];
        // `clip_pushing_nobody` must not mutate `pushable` flags or shift pushable blocks.
        let mut no_push_blocks = initial.clone();
        let v_no_push = super::clip_pushing_nobody(1, [180.0, 180.0], [0.0, 0.0, 0.0], 22.0, 128.0, DT, &mut no_push_blocks);
        assert_eq!(no_push_blocks, initial);
        assert!(v_no_push[0] < 0.0, "overlapping block at +X pushes walker toward -X: {v_no_push:?}");

        // `clip` shifts pushable blocks and records pushes, leaving distant blocks untouched.
        let mut push_blocks = initial.clone();
        let mut pushes = Vec::new();
        let v_push = super::clip(1, [180.0, 180.0], [0.0, 0.0, 0.0], 22.0, 128.0, DT, &mut push_blocks, &mut pushes);
        assert!(!pushes.is_empty());
        assert_eq!(push_blocks[0], initial[0], "distant block untouched");
        assert_eq!(push_blocks[1], initial[1], "distant block untouched");
        assert_ne!(v_push, v_no_push, "pushing yields less stop than clip_pushing_nobody");
    }
}
