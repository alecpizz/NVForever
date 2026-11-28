//! The Caravan pot as real objects [design: stacked like real physics
//! objects, with the cash, coins, and caps falling onto the table; cash bends a bit
//! while it falls and caps and coins bounce a bit when they settle].
//!
//! Each piece of [`crate::caravan_view::money`] is a rapier rigid body: a cap or coin a
//! short cylinder, a note a thin card. They are dropped one after another, notes first, from
//! a hand's height over their heap's place with a little sideways throw and spin, fall under
//! gravity, bounce, slide and come to rest on the planks and on each other, as the solver
//! finds; the pile is wherever they settle [design]. A note falls as a dropped bill does, like
//! a leaf: slowly, face to the air, rocking from side to side and gliding with each rock,
//! turning a little, until it touches down ([`PotSim::flutter`]); the drawing bends it by
//! how fast it falls ([`PotSim::shown`]) [design: the air pushing on an offset point made
//! the notes tumble end over end and spin].
//!
//! Sizes and weights are a real cap's, coin's and note's ([`crate::caravan_view`] for the
//! sizes) [hypothesis: a crown cap weighs about 2 g, a coin about 5 g, a note about 1 g;
//! the game gives none]. The cards' and the screen's words' places ([`money_zones`],
//! [`text_zones`]) are walls on the table no piece can slide into, so nothing comes to rest
//! where a card can lie or under a number [design].
//!
//! Units: rapier runs in metres, the table in its own units ([`UNITS_PER_M`]); the table's
//! up is +z (rapier's gravity is set along -z).

use crate::caravan_view::{Money, Piece, MM};
use nvf_play::caravan::screen::FRAME_W;
use bevy::prelude::{Quat, Vec2, Vec3};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::*;

/// The frame's height (units; [`FRAME_W`] wide).
const FRAME_H: f32 = 960.0;
/// Table units to the metre.
pub const UNITS_PER_M: f32 = MM * 1000.0;
/// The solver's step (s), and the most steps a frame may take (a slow frame lets the
/// pot fall slower rather than stall).
const STEP: f32 = 1.0 / 240.0;
const MAX_STEPS: u32 = 16;
/// The solver's passes a step, and its continuous-collision sub-steps: a cap is a few grams
/// and a note a thin card, which jitter in heaps and slip a tip through the planks with
/// the defaults [design: the pot tests, 2026-11-12].
const SOLVER_PASSES: usize = 8;
const CCD_SUBSTEPS: usize = 4;
/// The fastest a note turns (rad/s): paper slaps down, it does not spin [design].
const NOTE_TURN_MOST: f32 = 4.0;
/// The fall runs at this much of real time, so a 15 cm drop reads [design].
const PACE: f32 = 0.7;
/// Each kind falls as one handful: all the notes, then all the coins, then all the caps
/// [design: all bills, then coins, then caps]. A handful is let go
/// this long after the pot is laid or the one before it is down; every stack in it starts
/// at that moment, and its pieces follow one after another, this far apart (s) [design: each stack starts falling at the same time ... individual items still fall].
const FIRST_DROP: f32 = 0.15;
const STACK_GAP: f32 = 0.06;
/// Notes up a stack follow further apart (s): each lands on the one under it after that
/// one has come down, rather than knocking it into the planks [design: the pot test,
/// 2026-11-12, with fans overlapping, had one note pressed in and lifting again].
const NOTE_STACK_GAP: f32 = 0.25;
/// Pieces of one handful that would start in each other are let go one over the other,
/// this far apart (m) [design].
const DROP_SPACE: f32 = 0.002;
/// How long a handful of coins may take to lie still before the caps follow anyway (s)
/// [design].
const COIN_WAIT_MOST: f32 = 1.5;
/// How high a piece starts over the planks, and how far off its place it is thrown from
/// (m) [design].
const DROP_HIGH: (f32, f32) = (0.10, 0.18);
/// Caps and coins are let go lower, close over their heap, so they land on it and stack
/// rather than skid away (the first shots, 2026-11-12, scattered them) [design].
const ROUND_HIGH: (f32, f32) = (0.04, 0.08);
const THROW_OFF: (f32, f32) = (0.002, 0.008);
/// How far a cap or coin is tipped when let go (rad), and how fast it wobbles over and
/// spins flat (rad/s): little enough that a cap lands crown up, as a dropped crown cap
/// mostly does [hypothesis: by eye; its model has no inside, so one upside down shows only
/// its rim].
const ROUND_TIP: f32 = 0.15;
const ROUND_WOBBLE: f32 = 1.5;
const ROUND_SPIN: f32 = 8.0;
/// Real weights (kg): a cap, a coin, a note [hypothesis, see the module notes].
const CAP_KG: f32 = 0.002;
const COIN_KG: f32 = 0.005;
const NOTE_KG: f32 = 0.001;
/// A note's thickness for the solver (m): thicker than paper so it never slips through.
const NOTE_THICK: f32 = 0.0008;
/// Bounce and grip: metal on wood bounces a little, paper not at all [design].
const ROUND_BOUNCE: f32 = 0.25;
/// A cap's or coin's body reaches this much of its drawn half-width (its drawn edge is
/// crimped or milled, not solid all round) [design: the earlier 0.92 let drawn pieces
/// overlap where the bodies touched].
const ROUND_REACH: f32 = 0.98;
/// Caps' and coins' bodies have rounded edges this round (m): flat cylinder on flat
/// cylinder rocks without end in the solver, and a real cap's edge is rolled anyway
/// [design: the pot test, 2026-11-12, had caps rocking at 2 rad/s after 30 s].
const ROUND_EDGE: f32 = 0.0006;
const ROUND_GRIP: f32 = 0.7;
const NOTE_BOUNCE: f32 = 0.0;
/// The caps and coins wait until every note lies still (slower than this, m/s) or this
/// long after the last note was let go (s); then the first follows after [`FIRST_DROP`]
/// [design: the notes fall first].
const NOTE_STILL: f32 = 0.02;
const NOTE_WAIT_MOST: f32 = 2.5;
const NOTE_GRIP: f32 = 0.8;
/// A falling note: how fast it sinks (m/s), how far it rocks either side (rad) and how
/// often (rocks a second), how fast it glides with each rock (m/s), and the most it turns
/// flat while falling (rad/s); how hard it is held to that (per s) [design: by eye, a bill
/// dropped from a hand's height flutters down in about half a second].
const NOTE_FALL: f32 = 0.45;
const NOTE_ROCK: f32 = 0.28;
const NOTE_ROCK_HZ: f32 = 2.2;
const NOTE_GLIDE: f32 = 0.12;
const NOTE_TURN: f32 = 1.2;
const NOTE_HOLD: f32 = 18.0;
/// The height (m) over which a falling note's rocking dies away before it lands.
const NOTE_FLATTEN: f32 = 0.06;
/// How slowly a note sinks just over the table (m/s): the air under it cushions it.
const NOTE_LAND: f32 = 0.12;
/// A landed note lying flat and still, or this long (s) after it landed however it lies,
/// is pinned where it is: paper under caps does not move, and a thin light card squeezed
/// between the planks and the caps on it shook them without end in the solver (the pot
/// tests, 2026-11-12) [design].
const NOTE_PIN_AFTER: f32 = 1.5;
/// A note tipped less than this (the cosine: about 2 degrees, its ends 3 mm) is laid flat
/// when pinned [design].
const NOTE_LAY_FLAT: f32 = 0.9994;
/// The most a cap or coin is lifted out of the pinned piece under it when it is pinned
/// (m): its own thickness at most [design].
const ROUND_UNSINK_MOST: f32 = 0.004;
/// A cap or coin tipped more than this (rad, about 34 degrees) when it would be pinned is
/// tipped down to [`RETIP_TO`] (about 20) first [design].
const RETIP_FROM: f32 = 0.6;
const RETIP_TO: f32 = 0.35;
/// A flat note slower than this (m/s) is pinned [design].
const NOTE_PIN_V: f32 = 0.05;
/// A landed cap or coin lying still this long (s), or this long after it landed however it
/// lies, is pinned where it is too: a real one is dead still a moment after it settles,
/// while in the solver a few grams resting on a heap creep and tremble without end (the pot
/// tests, 2026-11-12: caps still moving 8 s on) [design]. Still: slower than
/// [`ROUND_STILL_V`] (m/s) and [`ROUND_STILL_W`] (rad/s).
const ROUND_PIN_STILL: f32 = 0.25;
const ROUND_PIN_AFTER: f32 = 3.0;
const ROUND_STILL_V: f32 = 0.05;
const ROUND_STILL_W: f32 = 3.0;
/// A note has landed only when its lowest point is this near the table (m), so one that
/// brushes a landed note above the table keeps drifting down (the pot test: one slid off
/// another and fell free onto an edge).
const NOTE_LANDED_LOW: f32 = 0.004;
/// How much a note bends by the air on it (table units at most) [design].
const NOTE_BEND: f32 = 9.0;
/// A cap or coin never comes to rest on its rim [design: forced onto the
/// top or bottom side, not allowing them to sit on the rim]: once it slows below
/// [`FLAT_SLOW`] (m/s) tipped more than [`FLAT_TIP`] (rad) from lying flat, crown up or
/// down, it is turned onto the nearer face at [`FLAT_RATE`] times the angle left (rad/s),
/// for at most [`FLAT_MOST`] seconds, so one wedged in a heap cannot keep the pot from
/// settling [design]. Below the tip it may lean on the pieces under it.
const FLAT_SLOW: f32 = 0.12;
const FLAT_TIP: f32 = 0.45;
const FLAT_RATE: f32 = 14.0;
const FLAT_MOST: f32 = 2.0;
/// A note whose lowest point is under this (m) is down on the table or the notes on it,
/// for [`PotSim::lay_flat`].
const NOTE_DOWN: f32 = 0.006;
/// The walls' height round the card and word places (m): higher than anything is let go
/// from, so nothing comes to rest on top of one in the air (the pot test, 2026-11-12: a
/// note lay on an 8 cm wall).
const WALL_HIGH: f32 = 0.5;
/// The table's edges are walls this far in from the screen's (frame units), so nothing
/// slides out of sight, and every piece rests whole at least this far inside the screen
/// [design: caps of the big heaps were cut by the screen's edges].
pub(crate) const EDGE_IN: f32 = 20.0;
/// Seen from straight above, a piece in the air is drawn this much bigger per metre up, so
/// the fall shows (the table's camera has no perspective) [design]. Only in the air: from
/// the first touch it eases back to its own size at [`LANDED_EASE`] per second, since a
/// piece drawn bigger than its body reaches into the table or the piece over it (a note into the table, a coin through a cap [observed: buckallobill, in game, FNV 1.4.0.525]).
const DEPTH_SCALE: f32 = 2.2;
const LANDED_EASE: f32 = 12.0;
/// Once a piece has landed its turning and sliding are damped this much (per s) [design:
/// the pot test, 2026-11-12, had caps still rocking at 2-4 rad/s after 8 s].
const LANDED_TURN_DAMP: f32 = 12.0;
const LANDED_SLIDE_DAMP: f32 = 1.5;
/// A landed piece turning slower than this (rad/s) for this long (s) goes to sleep.
const LANDED_SLEEP_TURN: f32 = 0.8;
const LANDED_SLEEP_AFTER: f32 = 0.4;
/// Everything at rest this long: the pot has settled and the solver stops.
const SETTLE: f32 = 0.6;
/// However it lies, the pot is called settled this long after its last piece was let go,
/// so a piece that keeps rocking cannot keep the solver running [design].
const SETTLE_MOST: f32 = 8.0;

/// A number from 0 to 1, the same each time for `i` and `salt`.
fn roll(i: usize, salt: u32) -> f32 {
    let mut h = (i as u32).wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h % 10_000) as f32 / 9_999.0
}

fn between(r: (f32, f32), t: f32) -> f32 {
    r.0 + (r.1 - r.0) * t
}

fn v(p: Vec3) -> Vector {
    Vector::new(p.x, p.y, p.z)
}

fn bv(p: Vector) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

fn q(r: &Rotation) -> Quat {
    Quat::from_xyzw(r.x, r.y, r.z, r.w)
}

fn rq(r: Quat) -> Rotation {
    Rotation::from_xyzw(r.x, r.y, r.z, r.w).normalize()
}

/// A piece's half-sizes for the solver (m): along its length, across, and its height.
fn half_extent(kind: Money) -> Vec3 {
    let half = kind.size() / 2.0 / UNITS_PER_M;
    if kind.note() { Vec3::new(half.x, half.y, NOTE_THICK / 2.0) } else { Vec3::new(half.x * ROUND_REACH, half.x * ROUND_REACH, kind.thick() / UNITS_PER_M / 2.0) }
}

/// The handfuls the pot falls in, in order: notes, coins, caps.
const WAVES: usize = 3;

/// Which handful a piece falls with.
fn handful(kind: Money) -> usize {
    if kind.note() {
        0
    } else if matches!(kind, Money::Cap | Money::NukaCap) {
        2
    } else {
        1
    }
}

/// How far a piece reaches round its middle across the table, and up and down as it is let
/// go (m), tipped as [`PotSim::drop`] tips it.
fn drop_reach(kind: Money) -> (f32, f32) {
    let e = half_extent(kind);
    if kind.note() { (e.truncate().length(), e.z) } else { (e.x, e.z + e.x * ROUND_TIP.sin()) }
}

/// A piece's lowest point (m) with its middle at `c` turned by `rot`: a note's lowest
/// corner, a cap's or coin's lowest point of its rim.
fn lowest_point(kind: Money, c: Vec3, rot: Quat) -> Vec3 {
    let e = half_extent(kind);
    let local = if kind.note() {
        let mut best = Vec3::ZERO;
        let mut z = f32::MAX;
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                for sz in [-1.0, 1.0] {
                    let l = Vec3::new(sx * e.x, sy * e.y, sz * e.z);
                    if (rot * l).z < z {
                        (best, z) = (l, (rot * l).z);
                    }
                }
            }
        }
        best
    } else {
        let down = rot.inverse() * -Vec3::Z;
        let flat = Vec2::new(down.x, down.y).normalize_or_zero() * e.x;
        Vec3::new(flat.x, flat.y, e.z * down.z.signum())
    };
    c + rot * local
}

/// A frame point (y down, table units) on the table in metres (y up).
fn to_m(p: Vec2) -> Vec2 {
    Vec2::new(p.x, -p.y) / UNITS_PER_M
}

/// One piece of the pot: where it is aimed, when it falls, its body once it has.
pub struct PotPiece {
    pub piece: Piece,
    /// Its handful ([`handful`]); how long after the handful starts it is let go (its
    /// place up its stack, [`STACK_GAP`] apart); and how high over the planks (m).
    wave: usize,
    delay: f32,
    high: f32,
    body: Option<RigidBodyHandle>,
    seed: usize,
    /// A note's bend now (table units), eased.
    bend: f32,
    /// How long a cap or coin has been turned flat ([`FLAT_MOST`]).
    flattened: f32,
    /// Its shape, once it falls.
    collider: Option<ColliderHandle>,
    /// When it was let go, and for a note how it drifts toward its place while it falls (m/s).
    release_clock: f32,
    drift: Vec3,
    /// Whether it has touched anything yet, and how much of the fall's growth it is still
    /// drawn with (1 in the air, easing to 0 once it touched).
    touched: bool,
    air: f32,
    /// When it landed, and for a note whether it is pinned to the table since
    /// ([`NOTE_PIN_AFTER`]).
    touched_at: f32,
    pinned: bool,
    /// How long a landed cap or coin has lain still ([`ROUND_PIN_STILL`]).
    still_for: f32,
    /// Whether a cap or coin has been tipped down once before pinning ([`RETIP_FROM`]).
    retipped: bool,
}

/// How a piece is drawn now: its middle on the table (table units, z up from the planks to
/// its middle), its turn, a note's bend, and whether it is still moving.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    pub at: Vec3,
    /// How much bigger it is drawn than its body ([`DEPTH_SCALE`]).
    pub grow: f32,
    pub rot: Quat,
    pub bend: f32,
    pub moving: bool,
}

/// The pot's bodies and their clock.
pub struct PotSim {
    w: PhysicsWorld,
    pub pieces: Vec<PotPiece>,
    clock: f32,
    carry: f32,
    still: f32,
    /// When each handful (notes, coins, caps) is let go, once the one before is down.
    wave_from: [Option<f32>; WAVES],
    /// When the last piece was let go.
    last_out: f32,
    /// The planks: touching them, or a piece that has landed, is landing (a wall only
    /// turns a piece aside; a piece still falling only knocks it).
    planks: ColliderHandle,
}

impl PotSim {
    /// The table: the planks, and walls round `zones` (frame units, y down).
    pub fn new(zones: &[(Vec2, Vec2)]) -> PotSim {
        let mut w = PhysicsWorld::new();
        w.gravity = Vector::new(0.0, 0.0, -9.81);
        w.integration_parameters.dt = STEP;
        // Small objects: contact tolerances on a centimetre's scale, not a metre's.
        w.integration_parameters.length_unit = 0.01;
        w.integration_parameters.num_solver_iterations = SOLVER_PASSES;
        w.integration_parameters.max_ccd_substeps = CCD_SUBSTEPS;
        let planks = ColliderBuilder::cuboid(5.0, 5.0, 0.05).translation(Vector::new(0.0, 0.0, -0.05)).friction(0.6).restitution(0.3);
        let planks = w.insert_collider(planks, None);
        // A little further in than [`EDGE_IN`]: a cap is drawn a hair wider than its body
        // ([`ROUND_REACH`]), and one resting against the wall must still lie whole inside.
        let (fw, fh, e, t) = (FRAME_W, FRAME_H, EDGE_IN + 2.0, 200.0);
        let edges = [
            (Vec2::new(-t, -t), Vec2::new(e, fh + t)),
            (Vec2::new(fw - e, -t), Vec2::new(fw + t, fh + t)),
            (Vec2::new(-t, -t), Vec2::new(fw + t, e)),
            (Vec2::new(-t, fh - e), Vec2::new(fw + t, fh + t)),
        ];
        for (a, b) in zones.iter().chain(&edges) {
            let (lo, hi) = (to_m(Vec2::new(a.x, b.y)), to_m(Vec2::new(b.x, a.y)));
            let (mid, half) = ((lo + hi) / 2.0, (hi - lo) / 2.0);
            if half.x <= 0.0 || half.y <= 0.0 {
                continue;
            }
            let wall = ColliderBuilder::cuboid(half.x, half.y, WALL_HIGH / 2.0).translation(Vector::new(mid.x, mid.y, WALL_HIGH / 2.0)).friction(0.0).restitution(0.2);
            w.insert_collider(wall, None);
        }
        PotSim { w, pieces: Vec::new(), clock: 0.0, carry: 0.0, still: 0.0, wave_from: [None; WAVES], last_out: f32::MAX, planks }
    }

    /// The pieces to drop, in handfuls ([`handful`]): all the notes together from now, then
    /// once they lie still all the coins together, then all the caps. Pieces of one
    /// handful that would start in each other start one over the other instead.
    pub fn add(&mut self, pieces: &[Piece], first_seed: usize) {
        let mut order: Vec<(usize, Piece)> = pieces.iter().copied().enumerate().map(|(k, p)| (first_seed + k, p)).collect();
        order.sort_by_key(|(_, p)| handful(p.kind));
        let first = self.pieces.len();
        for (seed, piece) in order {
            let kind = piece.kind;
            let wave = handful(kind);
            let (reach, up) = drop_reach(kind);
            let aim = to_m(piece.at);
            // Its place up its stack: the pieces of its handful laid before it that it lies
            // on (their footprints overlap).
            let under = self.pieces[first..].iter().filter(|o| o.wave == wave && (to_m(o.piece.at) - aim).length() < reach + drop_reach(o.piece.kind).0).count();
            let delay = if kind.note() { NOTE_STACK_GAP } else { STACK_GAP } * under as f32;
            let mut high = between(if kind.note() { DROP_HIGH } else { ROUND_HIGH }, roll(seed, 1)) + piece.lift / UNITS_PER_M;
            // Raised over every piece of its handful it would start in, until clear: each
            // raise only goes up, so this ends.
            loop {
                let clash = self.pieces[first..].iter().filter(|o| o.wave == wave).find_map(|o| {
                    let (r, u) = drop_reach(o.piece.kind);
                    let near = (to_m(o.piece.at) - aim).length() < reach + r + 2.0 * THROW_OFF.1;
                    let over = o.high + u + up + DROP_SPACE;
                    // Already raised over it (to rounding): not a clash.
                    (near && (o.high - high).abs() < up + u + DROP_SPACE && over > high + 1e-6).then_some(over)
                });
                match clash {
                    Some(h) => high = h,
                    None => break,
                }
            }
            self.pieces.push(PotPiece { piece, wave, delay, high, body: None, seed, bend: 0.0, flattened: 0.0, collider: None, touched: false, air: 1.0, release_clock: 0.0, drift: Vec3::ZERO, touched_at: 0.0, pinned: false, still_for: 0.0, retipped: false });
        }
        self.wave_from = [None; WAVES];
        self.wave_from[0] = Some(self.clock + FIRST_DROP);
        self.last_out = f32::MAX;
        self.still = 0.0;
    }

    /// Whether every piece of handful `wave` has been let go and lies still (or has had
    /// long enough); a handful with no pieces is down at once.
    fn wave_down(&self, wave: usize) -> bool {
        let Some(from) = self.wave_from[wave] else { return false };
        let mut of = self.pieces.iter().filter(|p| p.wave == wave);
        if !of.clone().all(|p| p.body.is_some()) {
            return false;
        }
        let most = if wave == 0 { NOTE_WAIT_MOST } else { COIN_WAIT_MOST };
        self.clock >= from + most
            || of.all(|p| p.pinned || p.body.and_then(|h| self.w.bodies.get(h)).is_some_and(|b| b.is_sleeping() || (p.touched && b.linvel().length() < NOTE_STILL)))
    }

    /// When piece `k` is let go, if that is known yet.
    fn release_at(&self, k: usize) -> Option<f32> {
        let p = &self.pieces[k];
        self.wave_from[p.wave].map(|t| t + p.delay)
    }

    /// Whether every piece has fallen and lies still: nothing more to step.
    pub fn settled(&self) -> bool {
        self.pieces.iter().all(|p| p.body.is_some()) && (self.still >= SETTLE || self.clock >= self.last_out + SETTLE_MOST)
    }

    /// Whether the pot came to rest of itself (not only by [`SETTLE_MOST`]).
    #[cfg(test)]
    fn at_rest(&self) -> bool {
        self.pieces.iter().all(|p| p.body.is_some()) && self.still >= SETTLE
    }

    /// Run the pot on by `dt` seconds of the game.
    pub fn advance(&mut self, dt: f32) {
        if self.settled() {
            return;
        }
        self.carry += dt * PACE;
        let mut n = 0;
        while self.carry >= STEP && n < MAX_STEPS {
            self.carry -= STEP;
            self.step();
            n += 1;
        }
        if n == MAX_STEPS {
            self.carry = 0.0;
        }
    }

    fn step(&mut self) {
        self.clock += STEP;
        for wave in 1..WAVES {
            if self.wave_from[wave].is_none() && self.wave_down(wave - 1) {
                // A handful with no pieces takes no time.
                let empty = !self.pieces.iter().any(|p| p.wave == wave);
                self.wave_from[wave] = Some(if empty { self.clock } else { self.clock + FIRST_DROP });
            }
        }
        for k in 0..self.pieces.len() {
            if self.pieces[k].body.is_none() && self.release_at(k).is_some_and(|t| self.clock >= t) {
                let h = self.drop(k);
                self.pieces[k].body = Some(h);
                if self.pieces.iter().all(|p| p.body.is_some()) {
                    self.last_out = self.clock;
                }
            }
        }
        self.flutter();
        self.lay_flat();
        self.calm_notes();
        self.w.step();
        self.calm_notes();
        // Landed: touching the planks, or a piece that has landed (a wall only turns it
        // aside and a piece still falling only knocks it, so a note keeps fluttering past
        // either). From then on it is damped as a light thing on wood is: a cap stops
        // rocking, a note stops turning.
        let landed: Vec<ColliderHandle> = std::iter::once(self.planks).chain(self.pieces.iter().filter(|p| p.touched).filter_map(|p| p.collider)).collect();
        for p in &mut self.pieces {
            let Some(c) = p.collider.filter(|_| !p.touched) else { continue };
            let near = !p.piece.kind.note()
                || p.body.and_then(|h| self.w.bodies.get(h)).is_some_and(|b| lowest_point(p.piece.kind, bv(b.translation()), q(b.rotation())).z < NOTE_LANDED_LOW);
            p.touched = near
                && self.w.narrow_phase.contact_pairs_with(c).any(|pair| {
                    let other = if pair.collider1 == c { pair.collider2 } else { pair.collider1 };
                    pair.has_any_active_contact() && landed.contains(&other)
                });
            if p.touched {
                p.touched_at = self.clock;
                if let Some(b) = p.body.and_then(|h| self.w.bodies.get_mut(h)) {
                    b.set_angular_damping(LANDED_TURN_DAMP);
                    b.set_linear_damping(LANDED_SLIDE_DAMP);
                    // A landed piece barely moving soon sleeps, so a heap stops trembling.
                    let a = b.activation_mut();
                    a.angular_threshold = LANDED_SLEEP_TURN;
                    a.time_until_sleep = LANDED_SLEEP_AFTER;
                }
            }
        }
        self.pin_notes();
        let moving = self.pieces.iter().filter_map(|p| p.body).filter_map(|h| self.w.bodies.get(h)).any(|b| !b.is_sleeping() && (b.linvel().length() > 0.01 || b.angvel().length() > 1.0));
        self.still = if moving || self.pieces.iter().any(|p| p.body.is_none()) { 0.0 } else { self.still + STEP };
    }

    /// Pin each landed note that lies flat and still, or has had [`NOTE_PIN_AFTER`]; and
    /// each landed cap or coin that has lain still [`ROUND_PIN_STILL`], or has had
    /// [`ROUND_PIN_AFTER`].
    fn pin_notes(&mut self) {
        let clock = self.clock;
        for p in &mut self.pieces {
            if !p.touched || p.pinned {
                continue;
            }
            // How deep a cap or coin is pressed into a pinned piece under it (m): pinned so,
            // it would stay inside it [observed: buckallobill, in game: coins clipping].
            let sunk = match (p.piece.kind.note(), p.collider, p.body.and_then(|h| self.w.bodies.get(h))) {
                (false, Some(c), Some(me)) => {
                    let my_z = me.translation().z;
                    self.w
                        .narrow_phase
                        .contact_pairs_with(c)
                        .filter(|pair| {
                            let other = if pair.collider1 == c { pair.collider2 } else { pair.collider1 };
                            let under = self.w.colliders.get(other).and_then(|o| o.parent()).and_then(|h| self.w.bodies.get(h));
                            under.is_some_and(|o| o.is_fixed() && o.translation().z < my_z)
                        })
                        .flat_map(|pair| pair.manifolds().iter().flat_map(|m| m.points.iter().map(|pt| -pt.dist)))
                        .fold(0.0f32, f32::max)
                }
                _ => 0.0,
            };
            let Some(b) = p.body.and_then(|h| self.w.bodies.get_mut(h)) else { continue };
            let up = q(b.rotation()) * Vec3::Z;
            let pin = if p.piece.kind.note() {
                // Pinned as soon as it lies flat and all but still, before the note after
                // it lands: one still drifting was pressed into the planks by it (the pot
                // test, 2026-11-12, with fans overlapping).
                let flat = up.z.abs() > 0.998;
                let still = b.linvel().length() < NOTE_PIN_V && b.angvel().length() < 0.5;
                (flat && still) || clock - p.touched_at >= NOTE_PIN_AFTER
            } else {
                let lies = up.z.abs().clamp(0.0, 1.0).acos() <= FLAT_TIP || p.flattened >= FLAT_MOST;
                let still = b.linvel().length() < ROUND_STILL_V && b.angvel().length() < ROUND_STILL_W;
                p.still_for = if lies && still { p.still_for + STEP } else { 0.0 };
                p.still_for >= ROUND_PIN_STILL || clock - p.touched_at >= ROUND_PIN_AFTER
            };
            // A cap or coin about to be pinned wedged steeply in a heap is first tipped down
            // to [`RETIP_TO`] about its lowest point and left loose a little longer, once, so
            // the solver settles it there (the pot tests: caps pinned 43 and 50
            // degrees off flat high in the big heaps; [design]: no cap on its rim).
            let tip = up.z.abs().clamp(0.0, 1.0).acos();
            if pin && !p.piece.kind.note() && !p.retipped && tip > RETIP_FROM {
                let rot = q(b.rotation());
                let c = bv(b.translation());
                let low = lowest_point(p.piece.kind, c, rot);
                let face = if up.z >= 0.0 { Vec3::Z } else { -Vec3::Z };
                let turn = Quat::from_axis_angle(up.cross(face).normalize_or(Vec3::X), tip - RETIP_TO);
                let new_rot = turn * rot;
                let mut new_c = low + turn * (c - low);
                // Its new lowest point no deeper than the old one.
                new_c.z += (low.z - lowest_point(p.piece.kind, new_c, new_rot).z).max(0.0);
                b.set_rotation(rq(new_rot), true);
                b.set_translation(v(new_c), true);
                b.set_linvel(Vector::ZERO, true);
                b.set_angvel(Vector::ZERO, true);
                p.retipped = true;
                p.touched_at = clock;
                p.still_for = 0.0;
                continue;
            }
            if pin {
                // A note all but flat is laid flat (its turn kept): lifting a note pressed in
                // by its deepest corner raised its middle 2 mm (the pot test, 2026-11-12),
                // and laid flat it needs only its own thickness.
                if p.piece.kind.note() && up.z.abs() > NOTE_LAY_FLAT {
                    let along = q(b.rotation()) * Vec3::X;
                    let mut flat = Quat::from_rotation_z(along.y.atan2(along.x));
                    if up.z < 0.0 {
                        flat *= Quat::from_rotation_x(std::f32::consts::PI);
                    }
                    b.set_rotation(rq(flat), false);
                }
                // Pinned clear of the planks: a hard landing may still have it pressed in.
                let low = lowest_point(p.piece.kind, bv(b.translation()), q(b.rotation())).z;
                // And out of the pinned piece it rests on.
                let lift = (-low).max(sunk.min(ROUND_UNSINK_MOST));
                if lift > 0.0 {
                    let mut at = bv(b.translation());
                    at.z += lift;
                    b.set_translation(v(at), false);
                }
                b.set_linvel(Vector::ZERO, false);
                b.set_angvel(Vector::ZERO, false);
                b.set_body_type(RigidBodyType::Fixed, false);
                p.pinned = true;
            }
        }
    }

    /// A note never spins up, whatever knocks it or however it lands ([`NOTE_TURN_MOST`]).
    fn calm_notes(&mut self) {
        for p in &self.pieces {
            if !p.piece.kind.note() || p.pinned {
                continue;
            }
            if let Some(b) = p.body.and_then(|h| self.w.bodies.get_mut(h)) {
                let w = bv(b.angvel());
                if w.length() > NOTE_TURN_MOST {
                    b.set_angvel(v(w.clamp_length_max(NOTE_TURN_MOST)), true);
                }
            }
        }
    }

    /// The falling notes, each steered as a falling leaf until it first touches anything:
    /// sinking at [`NOTE_FALL`], rocking about its length and gliding across with each
    /// rock, turning slowly, carried toward its place; from the touch the solver has it.
    fn flutter(&mut self) {
        let clock = self.clock;
        for p in &self.pieces {
            let Some(h) = p.body else { continue };
            if !p.piece.kind.note() || p.touched {
                continue;
            }
            let Some(b) = self.w.bodies.get_mut(h) else { continue };
            let t = clock - p.release_clock;
            let s = p.seed;
            let phase = t * NOTE_ROCK_HZ * std::f32::consts::TAU + roll(s, 10) * std::f32::consts::TAU;
            let yaw = p.piece.turn + (roll(s, 11) - 0.5) * 2.0 * NOTE_TURN * t;
            // The rocking dies away over the last [`NOTE_FLATTEN`] above the table, so it
            // settles flat rather than landing on an edge.
            let low = lowest_point(p.piece.kind, bv(b.translation()), q(b.rotation())).z;
            let calm = (low / NOTE_FLATTEN).clamp(0.0, 1.0);
            let rock = NOTE_ROCK * phase.sin() * calm;
            let goal = Quat::from_rotation_z(yaw) * Quat::from_rotation_x(rock);
            let mut err = goal * q(b.rotation()).inverse();
            if err.w < 0.0 {
                err = -err;
            }
            let (axis, angle) = err.to_axis_angle();
            b.set_angvel(v((axis * angle * NOTE_HOLD).clamp_length_max(NOTE_TURN_MOST)), true);
            // Across its face it glides the way it tips; it sinks evenly; it drifts to its place.
            let across = Quat::from_rotation_z(yaw) * Vec3::Y;
            let sink = NOTE_LAND + (NOTE_FALL - NOTE_LAND) * calm;
            let want = p.drift + across * (NOTE_GLIDE * phase.cos() * calm) - Vec3::Z * sink;
            let now = bv(b.linvel());
            let ease = (NOTE_HOLD * STEP).min(1.0);
            b.set_linvel(v(now + (want - now) * ease), true);
        }
    }

    /// Turn each slowing cap or coin that stands on its rim or leans steeply onto its
    /// nearer face ([`FLAT_TIP`]).
    fn lay_flat(&mut self) {
        for p in &mut self.pieces {
            let Some(h) = p.body else { continue };
            if p.flattened >= FLAT_MOST || p.pinned {
                continue;
            }
            let Some(b) = self.w.bodies.get_mut(h) else { continue };
            if b.linvel().length() > FLAT_SLOW {
                continue;
            }
            let (c, rot) = (bv(b.translation()), q(b.rotation()));
            let low = lowest_point(p.piece.kind, c, rot);
            // A note drifts down slowly: it is only laid flat once its lowest edge is down
            // on the table or the notes there, so one never stands on its edge (the second
            // and third shots, 2026-11-12: one stood against a wall, one stood free).
            if p.piece.kind.note() && low.z > NOTE_DOWN {
                continue;
            }
            let up = rot * Vec3::Z;
            let face = if up.z >= 0.0 { Vec3::Z } else { -Vec3::Z };
            let tip = up.angle_between(face);
            // Starts past the tip; once started, goes on till it lies flat.
            let started = p.flattened > 0.0;
            if tip <= 0.03 || (!started && tip <= FLAT_TIP) {
                continue;
            }
            let axis = up.cross(face).normalize_or(Vec3::X);
            let spin = bv(b.angvel()).dot(Vec3::Z) * Vec3::Z;
            let turn = axis * tip * FLAT_RATE + spin * 0.5;
            // Turned about its lowest point, which stays where it rests, so the piece
            // lies down rather than its low edge going into what it rests on.
            b.set_angvel(v(turn), true);
            b.set_linvel(v(-turn.cross(low - c)), true);
            p.flattened += STEP;
        }
    }

    /// Drop piece `k`: a body over its place, thrown in a little, spinning.
    fn drop(&mut self, k: usize) -> RigidBodyHandle {
        let (piece, s) = (self.pieces[k].piece, self.pieces[k].seed);
        let kind = piece.kind;
        let aim = to_m(piece.at);
        let high = self.pieces[k].high;
        let off = Vec2::from_angle(roll(s, 2) * std::f32::consts::TAU) * between(THROW_OFF, roll(s, 3));
        // A note sinks at its own pace ([`flutter`](Self::flutter)), a cap or coin falls.
        let fall = if kind.note() { high / NOTE_FALL } else { (2.0 * high / 9.81).sqrt() };
        let start = Vec3::new(aim.x + off.x, aim.y + off.y, high);
        let throw = Vec3::new(-off.x / fall, -off.y / fall, 0.0);
        self.pieces[k].release_clock = self.clock;
        self.pieces[k].drift = throw;
        let turn = Quat::from_rotation_z(piece.turn);
        let tip_axis = Vec3::new(roll(s, 4) - 0.5, roll(s, 5) - 0.5, 0.0).normalize_or(Vec3::X);
        let (tip, wobble, spin) = if kind.note() { (0.0, 0.0, 0.0) } else { (ROUND_TIP, ROUND_WOBBLE, ROUND_SPIN) };
        let rot = Quat::from_axis_angle(tip_axis, (roll(s, 6) - 0.5) * 2.0 * tip) * turn;
        let angvel = Vec3::new((roll(s, 7) - 0.5) * wobble, (roll(s, 8) - 0.5) * wobble, (roll(s, 9) - 0.5) * spin) * 2.0;
        let body = RigidBodyBuilder::dynamic().pose(Pose::from_parts(v(start), rq(rot))).linvel(v(throw)).angvel(v(angvel)).ccd_enabled(true)
            // Paper on the table stops turning at once.
            .angular_damping(if kind.note() { 3.0 } else { 0.4 })
            .linear_damping(0.05);
        let h = self.w.insert_body(body);
        let half = kind.size() / 2.0 / UNITS_PER_M;
        let c = if kind.note() {
            let r = NOTE_THICK * 0.4;
            ColliderBuilder::round_cuboid(half.x - r, half.y - r, NOTE_THICK / 2.0 - r, r).mass(NOTE_KG).friction(NOTE_GRIP)
                .restitution(NOTE_BOUNCE)
                // Paper never bounces, whatever it lands on (the default averages with the
                // planks' bounce).
                .restitution_combine_rule(CoefficientCombineRule::Min)
        } else {
            let (thick, kg) = if matches!(kind, Money::Cap | Money::NukaCap) { (kind.thick(), CAP_KG) } else { (kind.thick(), COIN_KG) };
            // Rapier's cylinder stands along its y: laid along the body's z.
            let (hh, rr) = (thick / UNITS_PER_M / 2.0, half.x * ROUND_REACH);
            ColliderBuilder::round_cylinder(hh - ROUND_EDGE, rr - ROUND_EDGE, ROUND_EDGE)
                .rotation(Vector::new(std::f32::consts::FRAC_PI_2, 0.0, 0.0))
                .mass(kg)
                .friction(ROUND_GRIP)
                .restitution(ROUND_BOUNCE)
        };
        self.pieces[k].collider = Some(self.w.insert_collider(c, Some(h)));
        h
    }

    /// How piece `k` is drawn now; `None` before it falls. Eases a note's bend.
    pub fn shown(&mut self, k: usize, dt: f32) -> Option<Shown> {
        let p = &self.pieces[k];
        let b = self.w.bodies.get(p.body?)?;
        let rot = q(b.rotation());
        // Drawn never inside the planks: a hard landing may press a corner in for a step
        // before the solver pushes it out, and the player never sees that.
        let low = lowest_point(p.piece.kind, bv(b.translation()), rot).z;
        let at = (bv(b.translation()) + Vec3::Z * (-low).max(0.0)) * UNITS_PER_M;
        let vel = bv(b.linvel());
        let moving = !b.is_sleeping() && vel.length() > 0.01;
        let mut bend = 0.0;
        if p.piece.kind.note() {
            // The air bends the ends back from the way the face moves.
            let vn = vel.dot(rot * Vec3::Z);
            let goal = (-vn * 6.0).clamp(-1.0, 1.0) * NOTE_BEND * if at.z > NOTE_THICK * UNITS_PER_M * 2.0 { 1.0 } else { 0.0 };
            let ease = 1.0 - (-12.0 * dt).exp();
            bend = p.bend + (goal - p.bend) * ease;
        }
        self.pieces[k].bend = bend;
        if self.pieces[k].touched {
            self.pieces[k].air *= (-LANDED_EASE * dt).exp();
        }
        let grow = 1.0 + DEPTH_SCALE * (at.z / UNITS_PER_M) * self.pieces[k].air;
        Some(Shown { at, grow, rot, bend, moving })
    }

    /// How the pot lies, for the log: its pieces, how many caps and coins rest on another,
    /// how many are pinned, whether it came to rest by itself (not by [`SETTLE_MOST`]),
    /// and how many bodies lie more than a millimetre into the planks.
    pub fn report(&self) -> String {
        let (mut n, mut raised, mut pinned, mut sunk) = (0, 0, 0, 0);
        for p in &self.pieces {
            let Some(b) = p.body.and_then(|h| self.w.bodies.get(h)) else { continue };
            n += 1;
            pinned += usize::from(p.pinned);
            let low = lowest_point(p.piece.kind, bv(b.translation()), q(b.rotation())).z;
            // Raised: its middle higher than a flat one's on the notes, so it rests on, or
            // leans on, another cap or coin.
            let flat_middle = p.piece.kind.thick() / UNITS_PER_M / 2.0 + 2.0 * NOTE_THICK;
            raised += usize::from(!p.piece.kind.note() && b.translation().z > flat_middle + 0.001);
            sunk += usize::from(low < -0.001);
        }
        format!("{n} pieces, {raised} caps and coins resting on others, {pinned} pinned, at rest by itself: {}, in the planks: {sunk}", self.still >= SETTLE)
    }

    /// The deepest any two bodies (or a body and the planks or a wall) overlap now (m).
    #[cfg(test)]
    pub fn deepest(&self) -> f32 {
        let mut worst = 0.0f32;
        for pair in self.w.narrow_phase.contact_pairs() {
            for m in pair.manifolds() {
                for c in &m.points {
                    worst = worst.max(-c.dist);
                }
            }
        }
        worst
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caravan_view::money;
    use nvf_play::caravan::screen::{money_zones, text_zones};

    /// The most a piece at rest leans (rad, 40 degrees): one leaning on a heap may lie past
    /// [`FLAT_TIP`] (from where it is turned, while it can be), one on its rim stands near 90.
    const REST_TIP_MOST: f32 = 0.70;

    fn zones() -> Vec<(Vec2, Vec2)> {
        let mut z = money_zones(4.0);
        z.extend(text_zones(4.0));
        z
    }

    fn run(pot: u32) -> PotSim {
        let mut s = PotSim::new(&zones());
        s.add(&money(pot), 0);
        for _ in 0..(30.0 / STEP) as u32 {
            s.advance(STEP / PACE);
            if s.settled() {
                break;
            }
        }
        s
    }

    #[test]
    fn the_pot_falls_bounces_and_comes_to_rest_in_heaps() {
        let mut s = run(2000);
        let restless: Vec<String> = s
            .pieces
            .iter()
            .enumerate()
            .filter_map(|(k, p)| {
                let b = s.w.bodies.get(p.body?)?;
                let moving = !b.is_sleeping() && (b.linvel().length() > 0.01 || b.angvel().length() > 1.0);
                let up = q(b.rotation()) * Vec3::Z;
                moving.then(|| {
                    format!(
                        "{k} {:?} v {:?} w {:?} z {:.4} up {:?} touched {} flattened {:.2} damp {}",
                        p.piece.kind,
                        bv(b.linvel()),
                        bv(b.angvel()),
                        b.translation().z,
                        up,
                        p.touched,
                        p.flattened,
                        b.angular_damping()
                    )
                })
            })
            .collect();
        assert!(s.at_rest(), "the pot comes to rest by itself; still moving: {restless:?}");
        let n = s.pieces.len();
        let shown: Vec<(Money, Shown)> = (0..n).filter_map(|k| Some((s.pieces[k].piece.kind, s.shown(k, 1.0)?))).collect();
        assert_eq!(shown.len(), n);
        // On the table, none through the planks, none sunk into another.
        for (kind, sh) in &shown {
            assert!(sh.at.z > 0.0, "{kind:?} under the planks: {sh:?}");
            assert!(sh.at.x > -50.0 && sh.at.x < nvf_play::caravan::screen::FRAME_W + 50.0 && sh.at.y < 50.0 && sh.at.y > -1010.0, "{kind:?} off the table: {sh:?}");
        }
        assert!(s.deepest() < 0.0015, "pieces sink into each other: {} m", s.deepest());
        // No cap, coin or note rests on its rim or edge: each lies on a face, crown up or
        // down, at most leaning on the heap under it; and nothing leaves the screen.
        for (kind, sh) in &shown {
            let tip = (sh.rot * Vec3::Z).z.abs().clamp(0.0, 1.0).acos();
            assert!(tip <= REST_TIP_MOST, "{kind:?} at {} rests {:.0} degrees off flat", sh.at, tip.to_degrees());
        }
        for (kind, sh) in &shown {
            // Whole, at least [`EDGE_IN`] inside the screen (less a little: a drawn cap is a
            // hair wider than its body, [`ROUND_REACH`]).
            let h = kind.size() / 2.0;
            let reach = if kind.note() {
                [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)].iter().map(|&(x, y)| (sh.rot * Vec3::new(x * h.x, y * h.y, 0.0)).truncate().abs()).fold(Vec2::ZERO, Vec2::max)
            } else {
                Vec2::splat(h.x)
            };
            let (lo, hi) = (Vec2::new(sh.at.x, -sh.at.y) - reach, Vec2::new(sh.at.x, -sh.at.y) + reach);
            let e = EDGE_IN - 1.0;
            assert!(lo.x >= e && lo.y >= e && hi.x <= FRAME_W - e && hi.y <= FRAME_H - e, "{kind:?} not whole on the screen: {lo} .. {hi}");
        }
        // Stacked: some caps and coins lie on others, well off the planks.
        let cap_middle = crate::caravan_view::CAP_THICK / 2.0;
        let up = shown.iter().filter(|(k, sh)| !k.note() && sh.at.z > cap_middle + 3.0).count();
        assert!(up >= 5, "only {up} pieces rest on others");
        // None came to rest where a card can lie.
        for (kind, sh) in &shown {
            let f = Vec2::new(sh.at.x, -sh.at.y);
            for (a, b) in money_zones(0.0) {
                let inside = f.x > a.x + 2.0 && f.x < b.x - 2.0 && f.y > a.y + 2.0 && f.y < b.y - 2.0;
                assert!(!inside, "{kind:?} at {f} lies in a card's place");
            }
        }
    }

    #[test]
    fn each_kind_falls_as_stacks_notes_then_coins_then_caps() {
        let mut s = PotSim::new(&zones());
        s.add(&money(10_434), 0);
        let mut out_at = vec![f32::NAN; s.pieces.len()];
        for _ in 0..(30.0 / STEP) as u32 {
            s.advance(STEP / PACE);
            for (k, p) in s.pieces.iter().enumerate() {
                if p.body.is_some() && out_at[k].is_nan() {
                    out_at[k] = s.clock;
                }
            }
            if s.settled() {
                break;
            }
        }
        let mut when = [Vec::new(), Vec::new(), Vec::new()];
        for (k, p) in s.pieces.iter().enumerate() {
            assert!(!out_at[k].is_nan(), "piece {k} never fell");
            when[p.wave].push(out_at[k]);
        }
        assert!(when.iter().all(|w| !w.is_empty()), "a big pot has notes, coins and caps");
        // Every stack of a handful starts at once; the pieces up a stack follow one by one.
        assert!(s.pieces.iter().any(|p| p.delay > 0.0), "some stack has more than one piece");
        for (k, p) in s.pieces.iter().enumerate() {
            let start = s.wave_from[p.wave].unwrap();
            assert!((out_at[k] - (start + p.delay)).abs() < STEP * 1.5, "piece {k} let go at {:.3} s, not {:.3} s", out_at[k], start + p.delay);
        }
        for (wave, w) in when.iter().enumerate() {
            let lo = w.iter().fold(f32::MAX, |a, &t| a.min(t));
            if wave > 0 {
                let before = when[wave - 1].iter().fold(f32::MIN, |a, &t| a.max(t));
                assert!(lo > before, "handful {wave} fell before handful {} was let go", wave - 1);
            }
        }
        // Pieces let go together never start inside each other.
        for a in 0..s.pieces.len() {
            for b in a + 1..s.pieces.len() {
                let (pa, pb) = (&s.pieces[a], &s.pieces[b]);
                if pa.wave != pb.wave {
                    continue;
                }
                let ((ra, ua), (rb, ub)) = (drop_reach(pa.piece.kind), drop_reach(pb.piece.kind));
                let near = (to_m(pa.piece.at) - to_m(pb.piece.at)).length() < ra + rb + 2.0 * THROW_OFF.1;
                assert!(!near || (pa.high - pb.high).abs() >= ua + ub, "pieces {a} {:?} and {b} {:?} start inside each other: highs {} {}, reaches {ra} {ua} {rb} {ub}, apart {}", pa.piece.kind, pb.piece.kind, pa.high, pb.high, (to_m(pa.piece.at) - to_m(pb.piece.at)).length());
            }
        }
    }

    #[test]
    fn the_notes_land_first_and_never_bounce() {
        let mut s = PotSim::new(&zones());
        s.add(&money(10_434), 0);
        let notes: Vec<usize> = (0..s.pieces.len()).filter(|&k| s.pieces[k].piece.kind.note()).collect();
        assert!(notes.len() >= 3, "a big pot has notes");
        let mut low = vec![f32::MAX; s.pieces.len()];
        for _ in 0..(30.0 / STEP) as u32 {
            s.advance(STEP / PACE);
            let round_out = s.pieces.iter().any(|p| !p.piece.kind.note() && p.body.is_some());
            for &k in &notes {
                let Some(b) = s.pieces[k].body.and_then(|h| s.w.bodies.get(h)) else {
                    assert!(!round_out, "a cap or coin fell before note {k}");
                    continue;
                };
                let z = b.translation().z;
                // No corner of a note ever goes into the planks.
                let half = s.pieces[k].piece.kind.size() / 2.0 / UNITS_PER_M;
                let r = q(b.rotation());
                for (sx, sy, sz) in [(-1.0, -1.0, -1.0), (1.0, -1.0, -1.0), (-1.0, 1.0, -1.0), (1.0, 1.0, -1.0), (-1.0, -1.0, 1.0), (1.0, -1.0, 1.0), (-1.0, 1.0, 1.0), (1.0, 1.0, 1.0)] {
                    let corner = z + (r * Vec3::new(sx * half.x, sy * half.y, sz * NOTE_THICK / 2.0)).z;
                    let p = &s.pieces[k];
                    // The body never passes through the planks (a corner pressed a few mm in
                    // by a hard landing is pushed out by the solver, and drawn lifted clear,
                    // below: what the player sees is that check).
                    assert!(corner > -0.01, "note {k} goes {:.2} mm into the table; touched {}, flattened {:.2} s, at {:.2} s, v {:.3} w {:.2}", -corner * 1000.0, p.touched, p.flattened, s.clock, b.linvel().length(), b.angvel().length());
                }
                // Once it has touched down, a note never rises again: no bounce (until the
                // caps land on it, which may lift an end).
                if low[k] < 0.003 && !round_out {
                    assert!(z <= low[k] + 0.0015, "note {k} bounced: {z} m after {} m", low[k]);
                }
                low[k] = low[k].min(z);
                if round_out {
                    assert!(b.linvel().length() < 0.05 || s.clock >= s.wave_from[0].unwrap_or(0.0) + NOTE_WAIT_MOST, "note {k} still falling when the caps came");
                }
            }
            // As drawn, no note is ever inside the table.
            for &k in &notes {
                let kind = s.pieces[k].piece.kind;
                if let Some(sh) = s.shown(k, STEP) {
                    let drawn = lowest_point(kind, sh.at / UNITS_PER_M, sh.rot).z;
                    assert!(drawn >= -1e-5, "note {k} drawn {:.2} mm into the table", -drawn * 1000.0);
                }
            }
            if s.settled() {
                break;
            }
        }
        assert!(s.settled());
    }

    #[test]
    fn the_same_pot_falls_the_same_way() {
        let (mut a, mut b) = (run(164), run(164));
        for k in 0..a.pieces.len() {
            assert_eq!(a.shown(k, 1.0).map(|s| s.at), b.shown(k, 1.0).map(|s| s.at));
        }
    }

    #[test]
    fn a_falling_note_bends_and_lies_flat_at_rest() {
        let note = Piece { kind: Money::Note20, at: Vec2::new(120.0, 120.0), turn: 0.3, lift: 0.0, lean: Vec2::ZERO };
        let mut s = PotSim::new(&[]);
        s.add(&[note], 0);
        let mut most = 0.0f32;
        for _ in 0..(6.0 / STEP) as u32 {
            s.advance(STEP / PACE);
            if let Some(sh) = s.shown(0, STEP) {
                most = most.max(sh.bend.abs());
            }
        }
        assert!(most > 2.0, "the note never bent: {most}");
        let rest = s.shown(0, 1.0).unwrap();
        assert!(rest.bend.abs() < 0.5 && !rest.moving, "{rest:?}");
    }
}
