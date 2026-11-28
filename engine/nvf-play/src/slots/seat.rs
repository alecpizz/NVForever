//! Playing a slot machine in the world (part A): the player sits on the
//! machine's stool, or stands in front of it where it has none, for as long as the slot
//! machine menu is open; the spin, its result and a win's celebration length go out as
//! `crate::slots::SlotsSpin` (part B's hook) for whoever draws the machine and the body
//! (nvforever `slots_seat.rs`; the reels and the camera, part B's `slots_view.rs`).
//!
//! - **Seated or standing** ([`stance_for`]) [observed: every playable slot machine of a
//!   51-plugin load order, 207 in 9 cells]: the machines carry no furniture marker of
//!   their own; where a machine is played seated, a stool (any FURN) stands straight out
//!   from its front, its centre 50 to 80 units from the machine's along the machine's local
//!   -y, at most about 30 to the side, on the same floor (105 machines: The Tops, the Atomic
//!   Wrangler, the Black Chip, half of Gomorrah, V&V, the Silver Palace and the Monte
//!   Carlo; one at the Sierra Madre on its Nuka stool). The others have nothing there (102:
//!   the Silver Palace's rows on wooden bases, most
//!   of Gomorrah's, the back rows of V&V) and are played standing [design: the player stands square at
//!   the machine at the right distance]. A stool is looked for in a box in front of the
//!   machine ([`FRONT`], [`SIDE`], [`RISE`]), the nearest one taken.
//! - **The machine's front** [observed: `Slot_Machine01.nif`'s glass plate at local
//!   y -18.7..-15.6, the stools on that side]: local -y. A standing player stands
//!   [`STAND_OFF`] units out on it, facing the machine (the machine's own heading).
//! - **Sitting down** uses the stool as any seat (`crate::furniture::UseFurniture`): the
//!   menu opens once the player is on it ([`crate::furniture::PlayerSeat::seated`]), or
//!   after [`SIT_WAIT`] seconds whatever happened; when it closes the player gets up the
//!   same way (the getting-off clip).
//! - **The celebration**'s length is `fnv_game::slots::celebration` (part B's): 0.8 s at
//!   2 x to 3 s from about 120 x.
//!
//! Log lines start `slots seat:`.

use crate::phys::PhysState;
use crate::sim::SimState;
use bevy::prelude::*;

/// A machine's frame: its reference's place and Z angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub pos: [f32; 3],
    pub theta: f32,
}

impl Frame {
    /// Local (game units) to game space, as the reference's frame turns.
    pub fn to_world(&self, l: [f32; 2]) -> [f32; 2] {
        let (s, c) = self.theta.sin_cos();
        [self.pos[0] + l[0] * c + l[1] * s, self.pos[1] - l[0] * s + l[1] * c]
    }
}

/// A reference's frame (its place now, else as placed).
pub fn frame_of(sim: &SimState, r: u32) -> Option<Frame> {
    let id = fnv_sim::FormId(r);
    let (pos, rot) = sim.world.peek(id).map(|x| (x.pos, x.rot)).or_else(|| sim.refs.get(id).map(|x| (x.pos, x.rot)))?;
    Some(Frame { pos, theta: rot[2] })
}

/// Put the player standing at `xy` on `floor`, facing `heading`.
pub fn stand_player(phys: &mut PhysState, player: &mut crate::player::Player, xy: [f32; 2], floor: f32, heading: f32) {
    phys.place_player([xy[0], xy[1], floor], 60.0, 200.0);
    player.feet = phys.prev_feet;
    player.heading = heading;
    player.pitch = 0.0;
}

/// A stool's centre must be this far out in front of the machine's, along its local -y
/// (game units) [observed: 49 to 80 in the base game, 110 for two Monte Carlo diner
/// stools; design: the margins].
pub const FRONT: (f32, f32) = (20.0, 140.0);
/// ... and at most this far to the side [observed: up to 30].
pub const SIDE: f32 = 40.0;
/// ... and at most this far above or below [design: the same floor].
pub const RISE: f32 = 60.0;
/// Where a standing player's feet are, out from the machine's centre on its front side
/// [design: the machine's front at 19, a body's half depth and a forearm's reach].
pub const STAND_OFF: f32 = 44.0;
/// The longest the menu waits for the player to sit down, seconds [design: the
/// getting-on clips are 2 to 3 s].
pub const SIT_WAIT: f32 = 6.0;
/// Where the reels are on the machine, in its frame (game units): the eye looks there
/// [observed: `Slot_Machine01.nif`'s reel nodes at y about 2, the window about 38 up;
/// hypothesis: the mods' machines are the same size].
pub const REEL_POINT: [f32; 3] = [0.0, -10.0, 40.0];

/// How the player plays a machine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stance {
    /// On this stool (a FURN reference).
    Seated { stool: u32 },
    /// Standing at `spot` (game space, the floor under it), facing `heading`.
    Standing { spot: [f32; 3], heading: f32 },
}

impl Stance {
    pub fn seated(&self) -> bool {
        matches!(self, Stance::Seated { .. })
    }
}

/// Where the play at a machine is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SeatPhase {
    /// Not at a machine.
    #[default]
    Free,
    /// Sitting down on the stool, the menu waiting.
    Sitting,
    /// The menu is open.
    Playing,
    /// The menu closed, still on the stool: the player gets up a moment later (nvforever
    /// `slots_seat.rs`) [design].
    Ending,
    /// Getting up from the stool after the menu closed.
    Leaving,
}

/// The machine the player plays, and how.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct SlotsSeat {
    pub machine: Option<u32>,
    pub stance: Option<Stance>,
    pub phase: SeatPhase,
    /// When the phase began (aid seconds).
    pub since: f32,
}

impl SlotsSeat {
    /// Whether the player is at a machine (sitting down, playing or getting up).
    pub fn busy(&self) -> bool {
        self.phase != SeatPhase::Free
    }
}

/// A point in the machine's frame: local (x right, y forward, z up) to game space.
pub fn local_to_game(f: Frame, l: [f32; 3]) -> [f32; 3] {
    let xy = f.to_world([l[0], l[1]]);
    [xy[0], xy[1], f.pos[2] + l[2]]
}

/// A game-space point in the machine's frame (x, y; the inverse of `Frame::to_world`).
pub fn game_to_local(f: Frame, p: [f32; 3]) -> [f32; 2] {
    let (s, c) = f.theta.sin_cos();
    let (dx, dy) = (p[0] - f.pos[0], p[1] - f.pos[1]);
    [dx * c - dy * s, dx * s + dy * c]
}

/// The nearest stool in front of the machine at `f`, of `furniture` (reference, place).
pub fn pick_stool(f: Frame, furniture: impl IntoIterator<Item = (u32, [f32; 3])>) -> Option<u32> {
    furniture
        .into_iter()
        .filter_map(|(r, p)| {
            let [x, y] = game_to_local(f, p);
            let ok = (-FRONT.1..=-FRONT.0).contains(&y) && x.abs() <= SIDE && (p[2] - f.pos[2]).abs() <= RISE;
            ok.then_some((x.hypot(y), r))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, r)| r)
}

/// Where a standing player stands at the machine at `f`, and the heading.
pub fn standing(f: Frame) -> ([f32; 3], f32) {
    (local_to_game(f, [0.0, -STAND_OFF, 0.0]), f.theta)
}

/// How the player plays `machine`: on the stool in front of it, else standing.
pub fn stance_for(sim: &SimState, machine: u32) -> Option<(Frame, Stance)> {
    let f = frame_of(sim, machine)?;
    let cell = sim.refs.get(fnv_sim::FormId(machine)).and_then(|r| r.cell);
    let furniture = sim.refs.refs.values().filter(|r| r.cell == cell && r.id.0 != machine).filter_map(|r| {
        let is_furn = sim.forms.get(r.base).is_some_and(|b| matches!(b.data, fnv_sim::forms::BaseData::Furniture));
        // `&&` keeps the enable check to the furniture.
        (is_furn && sim.world.is_enabled(r.id).unwrap_or(true)).then(|| (r.id.0, sim.world.peek(r.id).map_or(r.pos, |x| x.pos)))
    });
    let stance = match pick_stool(f, furniture) {
        Some(stool) => Stance::Seated { stool },
        None => {
            let (spot, heading) = standing(f);
            Stance::Standing { spot, heading }
        }
    };
    Some((f, stance))
}

/// The view's heading and pitch from `eye` to the machine's reels.
pub fn look_at_reels(f: Frame, eye: [f32; 3]) -> (f32, f32) {
    let p = local_to_game(f, REEL_POINT);
    let (dx, dy, dz) = (p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]);
    (dx.atan2(dy), dz.atan2(dx.hypot(dy)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn the_stool_in_front_is_taken_not_one_behind_or_aside() {
        // A machine facing south (heading PI: its front, local -y, is world +y).
        let f = Frame { pos: [1912.0, 1139.0, 13641.0], theta: PI };
        // The Atomic Wrangler's 0x00119EC7: its stool 53 units out, bearing 358 [observed].
        let front = (1, [1910.0, 1192.0, 13641.0]);
        let behind = (2, [1912.0, 1080.0, 13641.0]);
        let aside = (3, [1990.0, 1192.0, 13641.0]);
        let upstairs = (4, [1912.0, 1192.0, 13841.0]);
        assert_eq!(pick_stool(f, [behind, aside, upstairs, front]), Some(1));
        assert_eq!(pick_stool(f, [behind, aside, upstairs]), None);
        // Of two in front, the nearer.
        assert_eq!(pick_stool(f, [(5, [1912.0, 1250.0, 13641.0]), front]), Some(1));
    }

    #[test]
    fn standing_is_in_front_facing_the_machine() {
        let f = Frame { pos: [0.0, 0.0, 100.0], theta: PI / 2.0 };
        let (spot, heading) = standing(f);
        // Heading east: the front (local -y) is west.
        assert!((spot[0] + STAND_OFF).abs() < 1e-3 && spot[1].abs() < 1e-3 && spot[2] == 100.0, "{spot:?}");
        assert_eq!(heading, PI / 2.0);
        // Looking from there, the reels are ahead (east) and below a standing eye.
        let (h, p) = look_at_reels(f, [spot[0], spot[1], 100.0 + 115.0]);
        assert!((h - PI / 2.0).abs() < 1e-3 && p < -0.5, "{h} {p}");
        // And local and game frames are each other's inverse.
        let l = game_to_local(f, local_to_game(f, [3.0, -7.0, 0.0]));
        assert!((l[0] - 3.0).abs() < 1e-3 && (l[1] + 7.0).abs() < 1e-3);
    }
}
