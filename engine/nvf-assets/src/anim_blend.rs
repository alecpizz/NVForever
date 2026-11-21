//! Several KF clips at once on one actor skeleton, combined bone by bone by track
//! priority (docs/spec-notes/animation.md §9.4). Used for an actor under direct control
//! (the player): an idle under everything, movement clips over it, a weapon stance over
//! the arms, draw and holster clips over all of them.
//!
//! Rule [hypothesis: nif.xml names the blend interpolator's fields "High Priority",
//! "Next High Priority", "High Weights Sum" and "Next High Weights Sum", which reads as
//! priority-first blending; the details are not documented]: for each bone, the tracks of
//! the playing clips are taken from the highest priority down. Tracks of equal priority
//! share by their clips' weights, whatever those add up to: a weight says how a clip
//! shares a bone with the others at its priority, not how much of the bone the priority
//! takes. How much it takes is the clips' fade (their ease, 0 to 1): a priority group
//! takes its clips' eases added up, at most what is still free (starting at 1); lower
//! groups get what is left and the bind pose fills any rest. So a clip that is alone at
//! its priority on a bone has all of that bone once faded in, even at a small weight
//! [observed: the rifles' `aimup` and `aimdown` clips have no arm tracks, so the arms
//! must stay with `aim` while its weight drops; animation.md §9.6]. Clips fade in and out
//! by their ease, so a change of clip at the same priority cross-fades.
//!
//! The actor's root node is its placement and is never posed; the clips' accumulation
//! root is held at the identity: its motion is the actor's movement, which the
//! character controller provides (actors.md §7.2 for idles).
//!
//! No allocation per frame once the clips are bound: bindings are made when a clip
//! starts, and the per-frame work reuses the animator's buffers.

use crate::actor::{clip_target, PreparedSkeleton};
use crate::anim_play::{cycle_time, Clip};
use bevy::math::Affine3A;
use bevy::prelude::*;
use std::cmp::Reverse;
use std::sync::Arc;

/// Fade-out time and fade-in time for clips without a `Blend` key, seconds
/// [engineering choice].
pub const DEFAULT_FADE: f32 = 0.2;

type EmitterSelection = ((i32, i32), (bool, Option<f32>));

/// How a layer's time runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// By the clip's own cycle type, at `rate`.
    Loop,
    /// Once from the start, holding the last pose.
    Once,
    /// On the animator's shared phase (movement clips of different lengths stay in step).
    Synced,
}

/// One clip playing on the skeleton.
#[derive(Debug, Clone)]
pub struct Layer {
    pub clip: Arc<Clip>,
    /// Script sequence priority, above the KF's per-bone priority. Normal body
    /// layers use zero; a higher sequence tier wins before comparing bone tracks.
    pub sequence_priority: i32,
    /// Explicit deactivation easing; ordinary body clips keep DEFAULT_FADE.
    pub fade_out: f32,
    /// The caller's name for the role the clip plays (two roles may play one clip).
    pub tag: u32,
    pub mode: Mode,
    /// (node, track index, priority) for every track that drives a node: a skeleton node,
    /// or an extra node counted on from the skeleton's last ([`Animator::set_extra`]).
    bound: Vec<(u16, u16, i32)>,
    /// Clip time since the clip's start (Loop and Once): seconds × rate × frequency.
    pub time: f32,
    /// The clip's share among the tracks of its priority on a bone; follows `target`
    /// while the layer is asked for and stays as it was while the layer fades out.
    pub weight: f32,
    pub target: f32,
    /// The fade, 0 to 1: how much of a bone the layer's priority takes from those below.
    pub ease: f32,
    /// Seconds to fade in (ease 0 to 1); fading out takes [`Self::fade_out`].
    pub fade_in: f32,
    /// The fade is taken through [`settle`] instead of straight ([`Self::shown_ease`]): the
    /// first-person arms' clips with movement inertia (nvforever `player_body.rs`).
    pub smooth: bool,
    /// Playback speed multiplier (Loop and Once).
    pub rate: f32,
    /// Added to the shared phase before a [`Mode::Synced`] layer samples its clip (a
    /// share of the cycle): lines the clips of a blend up by their footfalls
    /// ([`crate::motion_match`]); 0 for every other layer.
    pub phase_offset: f32,
    seen: bool,
    /// Not asked for at the last [`Animator::end`]: fading out.
    leaving: bool,
    /// Each bound track's rotation as the last [`Animator::pose`] blended it (on its side of
    /// the sphere), so the next frame's stays on the same side: kept continuous, a clip's
    /// rotation never flips sign against the others it is blended with.
    aligned: Vec<Option<Vec4>>,
}

/// How sharp [`settle`]'s start is: the curve is a critically damped spring's step
/// response over `SETTLE_SHARPNESS` of its time constants.
const SETTLE_SHARPNESS: f32 = 6.0;

/// The settling curve of 0–1 [design: the owner's seventh playtest, 2026-11-14: "we
/// should not have any linear curves - everything should be a smooth spring with a slow
/// finish"]: the step response of a critically damped spring, scaled to arrive at 1. It
/// leaves 0 without a jolt, is quick early (over half the way in the first third of the
/// time) and very slow at the end (the last quarter of the time covers a twentieth). It
/// arrives at rest: the spring's last bit of speed (about a tenth of the average) is taken
/// out over the end by `x²(x − 1)`, which leaves the start and the middle as they were and
/// keeps the curve rising; a fade or a move that stopped at the spring's own end lost that
/// speed in one frame, a visible stop (the trace `movement-inertia-trace-weapons`: the
/// 10mm pistol's hold coming in at the draw's end, 9 units/s to 1 in a frame) [design: the
/// owner, 2026-11-15, "pistol drawing animation is not smooth at all", "make the aiming
/// down sights animation smoother at the end - same time length"].
pub fn settle(x: f32) -> f32 {
    let k = SETTLE_SHARPNESS;
    let x = x.clamp(0.0, 1.0);
    let norm = 1.0 - (1.0 + k) * (-k).exp();
    let spring = (1.0 - (1.0 + k * x) * (-k * x).exp()) / norm;
    let end_speed = k * k * (-k).exp() / norm;
    spring - end_speed * x * x * (x - 1.0)
}

/// [`settle`] at `x` with its first and second derivatives (its speed and its pull).
pub fn settle_slopes(x: f32) -> (f32, f32, f32) {
    let k = SETTLE_SHARPNESS;
    let x = x.clamp(0.0, 1.0);
    let norm = 1.0 - (1.0 + k) * (-k).exp();
    let end_speed = k * k * (-k).exp() / norm;
    let e = (-k * x).exp();
    let speed = k * k * x * e / norm - end_speed * (3.0 * x * x - 2.0 * x);
    let pull = k * k * e * (1.0 - k * x) / norm - end_speed * (6.0 * x - 2.0);
    (settle(x), speed, pull)
}

/// The `x` at which [`settle`] is `y` (by halving; `settle` only rises).
pub fn settle_inv(y: f32) -> f32 {
    let y = y.clamp(0.0, 1.0);
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if settle(mid) < y {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

impl Layer {
    fn advance_fade(&mut self, dt: f32) {
        if self.leaving {
            let down = if self.fade_out <= 0.0 { 1.0 } else { dt / self.fade_out };
            self.ease = (self.ease - down).max(0.0);
        } else {
            let up = if self.fade_in <= 0.0 { f32::INFINITY } else { dt / self.fade_in };
            self.ease = (self.ease + up).min(1.0);
            self.weight = if self.target > self.weight { (self.weight + up).min(self.target) } else { (self.weight - dt / DEFAULT_FADE).max(self.target) };
        }
    }

    /// The fade as the blend takes it: straight, or ([`Self::smooth`]) through [`settle`]:
    /// coming in it follows the curve; going out it follows the curve's mirror, so two
    /// layers of a priority crossing over the same time share the bones by the curve
    /// itself, quick at first and slow at the end.
    /// Fading out (no longer wanted).
    pub fn is_leaving(&self) -> bool {
        self.leaving
    }

    pub fn shown_ease(&self) -> f32 {
        match (self.smooth, self.leaving) {
            (false, _) => self.ease,
            (true, false) => settle(self.ease),
            (true, true) => 1.0 - settle(1.0 - self.ease),
        }
    }

    /// Take the fade straight again (no longer [`Self::smooth`]), keeping the shown fade
    /// where it is: a layer that stays (an idle, a weapon's hold) after the settling curve
    /// is switched off fades the clips' own way from then on, with no step.
    pub fn straighten(&mut self) {
        if self.smooth {
            self.ease = self.shown_ease();
            self.smooth = false;
        }
    }

    /// Start to leave or to come back (`leaving`), keeping a smooth layer's shown fade
    /// where it is: the coming-in and going-out curves differ, so the fade's place on
    /// the other curve is found.
    fn set_leaving(&mut self, leaving: bool) {
        if self.smooth && leaving != self.leaving {
            let shown = self.shown_ease();
            self.ease = if leaving { 1.0 - settle_inv(1.0 - shown) } else { settle_inv(shown) };
        }
        self.leaving = leaving;
    }

    /// The time within the clip the layer samples, given the animator's phase.
    pub fn sample_time(&self, phase: f32) -> f32 {
        let c = &self.clip;
        match self.mode {
            Mode::Synced => c.start + (phase + self.phase_offset).rem_euclid(1.0) * c.length(),
            Mode::Once => cycle_time(c.start + self.time, c.start, c.stop, 2),
            Mode::Loop => cycle_time(c.start + self.time, c.start, c.stop, c.cycle_type),
        }
    }

    /// Whole cycles a looping Loop layer has played, for the tracks whose rotation adds
    /// up from cycle to cycle (`TransformTrack::rotation_after`, docs/spec-notes/animation.md
    /// §9.12); 0 for every other layer.
    pub fn cycles(&self) -> i64 {
        let c = &self.clip;
        let len = c.length();
        match self.mode {
            // From the same wrap `sample_time` makes, so the count and the time sampled
            // agree at the loop (a quotient rounded on its own can count a cycle early).
            Mode::Loop if c.cycle_type == 0 && len > 0.0 && self.time.is_finite() => {
                let within = cycle_time(c.start + self.time, c.start, c.stop, 0) - c.start;
                ((self.time - within) / len).round() as i64
            }
            _ => 0,
        }
    }

    /// Whether a Once layer has reached the clip's end.
    pub fn finished(&self) -> bool {
        self.time >= self.clip.length()
    }
}

#[derive(Debug, Clone, Copy)]
struct Contribution {
    bone: u16,
    priority: (i32, i32),
    /// The layer's weight × ease: its share within its priority group.
    weight: f32,
    ease: f32,
    layer: u16,
    track: u16,
    /// Its place in the layer's `bound` (and `aligned`).
    slot: u16,
}

/// A node outside the skeleton that clips drive by name: a part of the weapon model
/// the actor holds (`##Slide`, `##Clip`; docs/spec-notes/animation.md §9.8).
#[derive(Debug, Clone)]
pub struct ExtraNode {
    pub name: String,
    pub entity: Entity,
    /// Its transform in the model: what it takes where no clip drives it.
    pub bind: Transform,
}

/// Layered clips on a spawned actor's skeleton ([`crate::actor::spawn_actor`]).
#[derive(Component)]
pub struct Animator {
    pub skel: Arc<PreparedSkeleton>,
    /// The skeleton's node entities (`nodes[0]` is the actor's root).
    pub nodes: Vec<Entity>,
    pub layers: Vec<Layer>,
    /// Shared phase (0–1) of the [`Mode::Synced`] layers.
    pub phase: f32,
    /// The pose of the last [`Animator::pose`]: local transforms in NIF space, the
    /// skeleton's nodes first, then the extra nodes.
    pub locals: Vec<Transform>,
    /// Nodes outside the skeleton ([`Self::set_extra`]).
    extra: Vec<ExtraNode>,
    /// Nodes held at the identity (the accumulation root).
    held: Vec<usize>,
    /// Nodes with a rest pose of their own in place of the bind pose ([`Self::set_rest`]).
    rest: Vec<(usize, Transform)>,
    /// The clip whose end gives the extra nodes their rest poses ([`Self::set_extra_rest`]).
    extra_rest: Option<Arc<Clip>>,
    scratch: Vec<Contribution>,
    /// Loop seams smoothed ([`Self::smooth_seams`]); `None`: the pose as the clips give it.
    seams: Option<Seams>,
    /// A looped clip, or the synced phase, wrapped round in the last [`Self::advance`].
    wrapped: bool,
    /// The pose jumps at the next [`Self::advance`] for another reason ([`Self::smooth_jump`]).
    jumped: bool,
    /// The last [`Self::advance`]'s seam was such a jump (`wrapped` is set too).
    jump_seam: bool,
    /// The last [`Self::advance`]'s seconds.
    last_dt: f32,
    /// The last [`Self::pose`] eased across a loop's seam or a jump (for a motion trace).
    pub seamed: bool,
    /// Each node's rotation in the last [`Self::pose`], the side of the sphere this frame's
    /// rotations are blended on (`pose`).
    last_rotations: Vec<Quat>,
}

/// Inertialization of loop seams [design: the owner's sixth playtest, 2026-11-14: "the
/// idle gun holding animation does not look smooth, small hitching in it"; the trace showed
/// the first-person hand's motion turning back in one frame each time the idle clip looped
/// (its last keys do not run on into its first): Daniel Holden's inertialization,
/// theorangeduck.com]. When a loop wraps, the pose carries on from where it was going for
/// that frame; the difference to the clips' pose (an offset, and its rate) then dies away on
/// a critically damped spring, so the seam's jump and its turn of direction become one
/// smooth curve. Translation and rotation; scale is the clips'.
#[derive(Debug, Clone, Default)]
struct Seams {
    omega: f32,
    /// The pose shown last frame and its rates (translation, rotation as scaled axis per s).
    shown: Vec<Transform>,
    vel_t: Vec<Vec3>,
    vel_r: Vec<Vec3>,
    /// The clips' pose last frame.
    raw: Vec<Transform>,
    /// The offsets dying away (translation, rotation as scaled axis) and their rates.
    off_t: Vec<(Vec3, Vec3)>,
    off_r: Vec<(Vec3, Vec3)>,
    /// An offset was set at a wrap last frame; its rate is set this frame, once the clips'
    /// rate after the seam is known.
    pending: bool,
    active: bool,
    /// The pending seam is a jump ([`Animator::smooth_jump`]), not a loop's wrap.
    jump: bool,
}

/// `dt` seconds of a critically damped spring at rest `0` (value, rate), exact.
fn decay(v: Vec3, r: Vec3, omega: f32, dt: f32) -> (Vec3, Vec3) {
    let b = r + omega * v;
    let k = (-omega * dt).exp();
    ((v + b * dt) * k, (b - omega * (v + b * dt)) * k)
}

/// An offset's rate at a jump, kept only as far as it takes the offset `x` back towards
/// zero and never past it (a critically damped spring at `omega` crosses zero when the
/// rate towards it is over `omega` × the offset): what moves it away, or across, is
/// dropped. Without this a clip that starts with a sharp move (a lever-action rifle's
/// shot, `2hrattack5is.kf`) handed its whole speed to an offset of almost nothing, and the
/// spring flung the arm out and back: the left hand left the Trail Carbine for a quarter
/// of a second at each shot (the trace `movement-inertia-trace-lever-sights`, 9.5 units
/// against the clip's own 4.6) [design: inertialization's overshoot clamp, David Bollo,
/// GDC 2018 "Inertialization: High-Performance Animation Transitions in Gears of War"].
fn toward_rest(x: Vec3, rate: Vec3, omega: f32) -> Vec3 {
    let len = x.length();
    if len < 1e-6 {
        return Vec3::ZERO;
    }
    let d = x / len;
    d * rate.dot(d).clamp(-omega * len, 0.0)
}

/// The rotation taking `b` to `a`, as a scaled axis (the shorter way).
fn rot_diff(a: Quat, b: Quat) -> Vec3 {
    let mut d = a * b.inverse();
    if d.w < 0.0 {
        d = -d;
    }
    d.to_scaled_axis()
}

impl Seams {
    /// Take the clips' pose `raw` after `dt` seconds (`wrapped`: a loop wrapped round in
    /// them) and give the pose to show in its place.
    // The loops walk the node index through several arrays of the same length at once.
    #[allow(clippy::needless_range_loop)]
    fn smooth(&mut self, raw: &mut [Transform], wrapped: bool, jump: bool, dt: f32) {
        let n = raw.len();
        if self.shown.len() != n {
            // The first pose, or the skeleton changed: start from the clips'.
            *self = Seams { omega: self.omega, shown: raw.to_vec(), vel_t: vec![Vec3::ZERO; n], vel_r: vec![Vec3::ZERO; n], raw: raw.to_vec(), off_t: vec![(Vec3::ZERO, Vec3::ZERO); n], off_r: vec![(Vec3::ZERO, Vec3::ZERO); n], ..Seams::default() };
            return;
        }
        if dt <= 0.0 {
            raw.copy_from_slice(&self.shown);
            return;
        }
        if wrapped {
            // Carry on from where the pose was going; the clips' pose jumped: offset it.
            for i in 0..n {
                let s = self.shown[i];
                let t = s.translation + self.vel_t[i] * dt;
                let q = (Quat::from_scaled_axis(self.vel_r[i] * dt) * s.rotation).normalize();
                self.off_t[i] = (t - raw[i].translation, Vec3::ZERO);
                self.off_r[i] = (rot_diff(q, raw[i].rotation), Vec3::ZERO);
            }
            (self.pending, self.active, self.jump) = (true, true, jump);
        } else if self.active {
            for i in 0..n {
                if self.pending {
                    // The offsets' rates: the pose's own rate less the clips' after the seam.
                    let rt = (raw[i].translation - self.raw[i].translation) / dt;
                    let rr = rot_diff(raw[i].rotation, self.raw[i].rotation) / dt;
                    self.off_t[i].1 = self.vel_t[i] - rt;
                    self.off_r[i].1 = self.vel_r[i] - rr;
                    // A jump (a clip started again): the clip's own motion from its start
                    // shows whole; the offset only dies away (`toward_rest`).
                    if self.jump {
                        self.off_t[i].1 = toward_rest(self.off_t[i].0, self.off_t[i].1, self.omega);
                        self.off_r[i].1 = toward_rest(self.off_r[i].0, self.off_r[i].1, self.omega);
                    }
                }
                self.off_t[i] = decay(self.off_t[i].0, self.off_t[i].1, self.omega, dt);
                self.off_r[i] = decay(self.off_r[i].0, self.off_r[i].1, self.omega, dt);
            }
            self.pending = false;
        }
        let clips = raw.to_vec();
        if self.active {
            let mut most = 0.0f32;
            for i in 0..n {
                raw[i].translation += self.off_t[i].0;
                raw[i].rotation = (Quat::from_scaled_axis(self.off_r[i].0) * raw[i].rotation).normalize();
                most = most.max(self.off_t[i].0.length()).max(self.off_r[i].0.length() * 10.0);
            }
            if !self.pending && most < 1e-5 {
                self.active = false;
            }
        }
        for i in 0..n {
            self.vel_t[i] = (raw[i].translation - self.shown[i].translation) / dt;
            self.vel_r[i] = rot_diff(raw[i].rotation, self.shown[i].rotation) / dt;
        }
        self.shown.copy_from_slice(raw);
        self.raw = clips;
    }
}

/// Where `clip` leaves the node named `name` at its end (`bind` fills what the track
/// leaves out); `None` when the clip has no transform track for it.
pub fn track_end(clip: &Clip, name: &str, bind: Transform) -> Option<Transform> {
    let (_, t) = clip.tracks.iter().find(|(n, _)| n == name)?;
    let time = clip.stop;
    Some(Transform {
        translation: t.translation.sample(time).map_or(bind.translation, Vec3::from_array),
        rotation: t.rotation_at(time).map_or(bind.rotation, |[w, x, y, z]| Quat::from_xyzw(x, y, z, w).normalize()),
        scale: Vec3::splat(t.scale.sample(time).map_or(bind.scale.x, |[s]| s)),
    })
}

impl Animator {
    /// An animator on a spawned skeleton; `accum_root` (normally `Bip01`) is held at the
    /// identity.
    pub fn new(skel: Arc<PreparedSkeleton>, nodes: Vec<Entity>, accum_root: Option<&str>) -> Animator {
        let held = accum_root.and_then(|r| clip_target(&skel, r)).into_iter().collect();
        let locals = skel.locals.clone();
        Animator { skel, nodes, layers: Vec::new(), phase: 0.0, locals, extra: Vec::new(), held, rest: Vec::new(), extra_rest: None, scratch: Vec::new(), seams: None, wrapped: false, jumped: false, jump_seam: false, last_dt: 0.0, seamed: false, last_rotations: Vec::new() }
    }

    /// Smooth the seams of looping clips: at a wrap the pose carries on and eases onto the
    /// clips' over about `seconds` (95%); `None` leaves the pose as the clips give it.
    pub fn smooth_seams(&mut self, seconds: Option<f32>) {
        match (seconds, self.seams.as_mut()) {
            (Some(s), Some(seams)) => seams.omega = 4.743_864_6 / s.max(1e-3),
            (Some(s), None) => self.seams = Some(Seams { omega: 4.743_864_6 / s.max(1e-3), ..Seams::default() }),
            (None, _) => self.seams = None,
        }
    }

    /// The pose jumps at the next [`Self::advance`] (a clip started again from its start, a
    /// fire clip at each shot): with [`Self::smooth_seams`] on it eases across the jump as
    /// across a loop's seam, the new clip's own motion on top; otherwise nothing.
    pub fn smooth_jump(&mut self) {
        self.jumped = self.seams.is_some();
    }

    /// Replace the nodes outside the skeleton that clips drive by name (none: an empty
    /// list). A name the skeleton has stays the skeleton's; a track drives every extra
    /// node of its name (the 9mm pistol's model has two nodes `##nmClip`, the first
    /// empty and the second holding the magazine; animation.md §9.8). The clips playing
    /// are bound again, so a weapon that arrives half way through a clip moves with it.
    pub fn set_extra(&mut self, extra: Vec<ExtraNode>) {
        self.extra = extra;
        // The last pose's extra nodes, and their rest poses, were the old list's.
        let n = self.skel.locals.len();
        self.locals.truncate(n);
        self.rest.retain(|(i, _)| *i < n);
        self.extra_rest = None;
        for i in 0..self.layers.len() {
            let bound = self.bind(&self.layers[i].clip);
            self.layers[i].bound = bound;
        }
    }

    /// Go on from where `old` is, for a model rebuilt in its place (another outfit, a new
    /// look): its clips with their times, weights and fades, its phase, and its last pose
    /// on the nodes of the same name. Without this the new model's clips fade in from
    /// nothing and the bind pose (a T-pose) shows for their first fifth of a second
    /// [owner 2026-10-17]. The clips are bound to this animator's nodes by name, so a
    /// different skeleton takes what it shares.
    pub fn continue_from(&mut self, old: &Animator) {
        self.phase = old.phase;
        self.layers = old.layers.clone();
        for i in 0..self.layers.len() {
            let bound = self.bind(&self.layers[i].clip);
            self.layers[i].bound = bound;
        }
        for (name, &i) in &self.skel.by_name {
            if let (Some(&j), Some(l)) = (old.skel.by_name.get(name), self.locals.get_mut(i)) {
                if let Some(&t) = old.locals.get(j) {
                    *l = t;
                }
            }
        }
    }

    /// The nodes a track named `name` drives: its skeleton node (not the root), else
    /// every extra node of that name.
    fn targets(&self, name: &str) -> Vec<usize> {
        if self.skel.by_name.contains_key(name) {
            return clip_target(&self.skel, name).into_iter().collect();
        }
        let n = self.skel.locals.len();
        self.extra.iter().enumerate().filter(|(_, x)| x.name == name).map(|(k, _)| n + k).collect()
    }

    /// `clip`'s tracks bound to nodes: (node, track index, priority).
    fn bind(&self, clip: &Clip) -> Vec<(u16, u16, i32)> {
        clip.tracks
            .iter()
            .zip(&clip.priorities)
            .enumerate()
            .flat_map(|(ti, ((name, _), &p))| self.targets(name).into_iter().filter(|b| !self.held.contains(b)).map(move |b| (b as u16, ti as u16, p)))
            .collect()
    }

    /// The names of the extra nodes `clip` has a transform track for, a name once for
    /// each node of it (for reports).
    pub fn extra_driven<'a>(&'a self, clip: &Clip) -> Vec<&'a str> {
        let n = self.skel.locals.len();
        let mut nodes: Vec<usize> = clip.tracks.iter().flat_map(|(name, _)| self.targets(name)).filter_map(|i| i.checked_sub(n)).collect();
        nodes.sort_unstable();
        nodes.dedup();
        let mut out: Vec<&str> = nodes.into_iter().map(|k| self.extra[k].name.as_str()).collect();
        out.sort_unstable();
        out
    }

    /// Give `node` a rest pose in place of its bind pose (`None`: the bind pose again):
    /// what it takes where no clip drives it, and what fills the part of it that the
    /// clips leave free.
    pub fn set_rest(&mut self, node: usize, pose: Option<Transform>) {
        self.rest.retain(|(n, _)| *n != node);
        if let Some(p) = pose {
            self.rest.push((node, p));
        }
    }

    /// Give the extra nodes (the weapon's parts) that `clip` drives the pose it leaves
    /// them in at its end as their rest pose, the others their place in their model
    /// (`None`: every extra node its place). For the draw clip while the weapon is in the
    /// hand: the 9mm pistol's draw clip cocks the hammer, which its aim clip does not
    /// key, and its attack and reload clips leave it cocked [observed: animation.md
    /// §9.8; hypothesis: the game leaves a part where the last clip put it, as §9.7
    /// reads the `Weapon` node]. Cheap to call every frame with the same clip.
    pub fn set_extra_rest(&mut self, clip: Option<&Arc<Clip>>) {
        if match (&self.extra_rest, clip) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        } {
            return;
        }
        let n = self.skel.locals.len();
        self.rest.retain(|(i, _)| *i < n);
        if let Some(c) = clip {
            for (k, x) in self.extra.iter().enumerate() {
                if let Some(p) = track_end(c, &x.name, x.bind) {
                    self.rest.push((n + k, p));
                }
            }
        }
        self.extra_rest = clip.cloned();
    }

    /// The rest pose of the extra node at `k` ([`Self::set_extra_rest`]): where it is
    /// when no clip drives it.
    pub fn extra_rest(&self, k: usize) -> Option<Transform> {
        let i = self.skel.locals.len() + k;
        self.rest.iter().find(|(n, _)| *n == i).map(|(_, p)| *p).or_else(|| self.extra.get(k).map(|x| x.bind))
    }

    /// Start of a frame's requests: every layer not asked for again by [`Self::end`] fades
    /// out.
    pub fn begin(&mut self) {
        for l in &mut self.layers {
            l.seen = false;
        }
    }

    /// Keep `clip` playing in role `tag` at `weight` (starting it, faded in, if it is not
    /// playing in that role). A weight of 0 keeps the clip on the bones it has alone at
    /// its priority. Returns the layer's index.
    pub fn want(&mut self, clip: &Arc<Clip>, tag: u32, weight: f32, mode: Mode, fade_in: f32) -> usize {
        if let Some(i) = self.find(clip, tag) {
            let l = &mut self.layers[i];
            (l.target, l.seen, l.mode, l.fade_in) = (weight, true, mode, fade_in);
            return i;
        }
        self.push(clip, tag, weight, mode, fade_in)
    }

    /// Play `clip` in role `tag` from its start, replacing any layer of that role.
    pub fn restart(&mut self, clip: &Arc<Clip>, tag: u32, weight: f32, mode: Mode, fade_in: f32) -> usize {
        self.layers.retain(|l| l.tag != tag);
        self.push(clip, tag, weight, mode, fade_in)
    }

    /// Drop every layer of role `tag` at once (no fade).
    pub fn cut(&mut self, tag: u32) {
        self.layers.retain(|l| l.tag != tag);
    }

    fn find(&self, clip: &Arc<Clip>, tag: u32) -> Option<usize> {
        self.layers.iter().position(|l| l.tag == tag && Arc::ptr_eq(&l.clip, clip))
    }

    fn push(&mut self, clip: &Arc<Clip>, tag: u32, weight: f32, mode: Mode, fade_in: f32) -> usize {
        let bound = self.bind(clip);
        let ease = if fade_in <= 0.0 { 1.0 } else { 0.0 };
        self.layers.push(Layer {
            clip: clip.clone(),
            sequence_priority: 0,
            fade_out: DEFAULT_FADE,
            tag,
            mode,
            bound,
            time: 0.0,
            weight,
            target: weight,
            ease,
            fade_in,
            smooth: false,
            rate: 1.0,
            phase_offset: 0.0,
            seen: true,
            leaving: false,
            aligned: Vec::new(),
        });
        self.layers.len() - 1
    }

    /// End of a frame's requests (see [`Self::begin`]): the layers not asked for fade out
    /// from here on, keeping their weights.
    pub fn end(&mut self) {
        for l in &mut self.layers {
            l.set_leaving(!l.seen);
        }
    }

    /// The layer playing `clip` in role `tag`.
    pub fn layer(&self, clip: &Arc<Clip>, tag: u32) -> Option<&Layer> {
        self.find(clip, tag).map(|i| &self.layers[i])
    }

    /// Whether the playing clips show or hide the node named `name` (a part of a weapon
    /// model, animation.md §9.7): the visibility track of the highest priority among
    /// them, the latest started of equals; `None` when no playing clip has one for it.
    pub fn visible(&self, name: &str) -> Option<bool> {
        let mut best: Option<((i32, i32), bool)> = None;
        for l in self.layers.iter().filter(|l| l.ease > 0.0) {
            for (n, priority, track) in &l.clip.visibility {
                if n != name {
                    continue;
                }
                if let Some(v) = track.at(l.sample_time(self.phase)) {
                    let priority = (l.sequence_priority, *priority);
                    if best.is_none_or(|(p, _)| priority >= p) {
                        best = Some((priority, v));
                    }
                }
            }
        }
        best.map(|(_, v)| v)
    }

    /// What the playing clips say of the particle system named `system` (a weapon
    /// model's, the dynamite's fuse sparks): whether its emitter is on and its birth rate
    /// where keyed, from the emitter track of the highest priority among them, the latest
    /// started of equals; `None` when no playing clip has one for it.
    pub fn emitter(&self, system: &str) -> Option<(bool, Option<f32>)> {
        let mut best: Option<EmitterSelection> = None;
        for l in self.layers.iter().filter(|l| l.ease > 0.0) {
            for e in l.clip.emitters.iter().filter(|e| e.system == system && (e.rate.is_some() || e.active.is_some())) {
                let priority = (l.sequence_priority, e.priority);
                if best.is_none_or(|(p, _)| priority >= p) {
                    best = Some((priority, e.at(l.sample_time(self.phase))));
                }
            }
        }
        best.map(|(_, v)| v)
    }

    /// Where the last [`Self::pose`] put the extra node at `k` in the list given to
    /// [`Self::set_extra`] (its local transform); `None` before the first pose with it.
    pub fn extra_local(&self, k: usize) -> Option<Transform> {
        self.locals.get(self.skel.locals.len() + k).copied()
    }

    /// The extra nodes the last [`Self::pose`] left away from their place in their
    /// model, with how far: units moved and degrees turned (for reports).
    pub fn extra_moved(&self) -> impl Iterator<Item = (&str, f32, f32)> {
        let n = self.skel.locals.len();
        self.extra.iter().zip(self.locals.iter().skip(n)).filter(|(x, l)| x.bind != **l).map(|(x, l)| {
            (x.name.as_str(), x.bind.translation.distance(l.translation), x.bind.rotation.angle_between(l.rotation).to_degrees())
        })
    }

    /// Advance fades, weights and times by `dt` seconds; synced layers move their shared
    /// phase at `sync_rate` times their own speed (weighted by their shares). Layers
    /// faded out are dropped.
    pub fn advance(&mut self, dt: f32, sync_rate: f32) {
        let (mut len_sum, mut w_sum) = (0.0, 0.0);
        self.last_dt = dt;
        self.wrapped = std::mem::take(&mut self.jumped);
        self.jump_seam = self.wrapped;
        for l in &mut self.layers {
            // A shown looping clip about to wrap round (for `smooth_seams`).
            let len = l.clip.length();
            if matches!(l.mode, Mode::Loop) && l.ease > 0.0 && len > 0.0 && l.clip.cycle_type == 0 {
                let next = l.time + dt * l.rate * l.clip.frequency;
                self.wrapped |= (next / len).floor() != (l.time / len).floor();
            }
            l.advance_fade(dt);
            match l.mode {
                Mode::Synced => {
                    let w = l.weight * l.ease;
                    len_sum += w * l.clip.length() / l.clip.frequency;
                    w_sum += w;
                }
                _ => l.time += dt * l.rate * l.clip.frequency,
            }
        }
        self.layers.retain(|l| l.ease > 0.0 || !l.leaving);
        if w_sum > 1e-6 && len_sum > 1e-6 {
            let next = self.phase + dt * sync_rate * w_sum / len_sum;
            self.wrapped |= next >= 1.0;
            self.phase = next.rem_euclid(1.0);
        }
    }

    /// Advance the selected layers' blends and the final pose's smoothing clock,
    /// without moving clip time or the locomotion phase. A menu can animate a
    /// first-person script pose while the world stays held (pipboy.md §2).
    pub fn advance_pose_blends(&mut self, dt: f32, select: impl Fn(&Layer) -> bool) {
        if !dt.is_finite() || dt <= 0.0 { return; }
        self.last_dt += dt;
        for layer in &mut self.layers {
            if select(layer) { layer.advance_fade(dt); }
        }
        self.layers.retain(|layer| layer.ease > 0.0 || !layer.leaving);
    }

    /// Blend the layers into [`Self::locals`].
    pub fn pose(&mut self) {
        let skel = &self.skel;
        self.scratch.clear();
        for (li, l) in self.layers.iter_mut().enumerate() {
            if l.ease <= 0.0 {
                l.aligned.clear();
                continue;
            }
            l.aligned.resize(l.bound.len(), None);
            let ease = l.shown_ease();
            // A fade that shows as nothing adds nothing. A leaving smooth layer's last frame
            // (its fade a hair over 0, the settling curve rounding it to none) alone in its
            // priority group shared 0 ÷ 0 below, and the bones it drives took their bind pose
            // for that frame: the left arm flung out for one frame of the grenade's put-away
            // [observed: the trace `movement-inertia-trace-weapons`, 2026-11-16].
            if ease <= 0.0 || ease.is_nan() {
                continue;
            }
            for (slot, &(bone, track, priority)) in l.bound.iter().enumerate() {
                self.scratch.push(Contribution { bone, priority: (l.sequence_priority, priority), weight: l.weight * ease, ease, layer: li as u16, track, slot: slot as u16 });
            }
        }
        self.scratch.sort_unstable_by_key(|c| (c.bone, Reverse(c.priority)));
        // Each clip's rotation of a bone is blended on the side of the sphere it was on last
        // frame (`Layer::aligned`); one just come in, on the side of the bone's last pose.
        // Taken by the first clip's side each frame, two clips whose rotations of a bone
        // passed half a turn apart while they crossed over flipped the blend to the other way
        // round in one frame (the straight razor's put-away over the idle: the upper arm, a
        // jump of the hand of 34 units) [observed: the trace `movement-inertia-trace-weapons`,
        // 2026-11-15].
        self.last_rotations.clear();
        self.last_rotations.extend(self.locals.iter().map(|l| l.rotation));
        // Bones without tracks: the bind pose, or their own rest pose; held nodes: the
        // identity; extra nodes: their place in their model.
        self.locals.clone_from(&skel.locals);
        self.locals.extend(self.extra.iter().map(|x| x.bind));
        for &h in &self.held {
            self.locals[h] = Transform::IDENTITY;
        }
        for &(n, pose) in &self.rest {
            self.locals[n] = pose;
        }
        let phase = self.phase;
        let mut i = 0;
        while i < self.scratch.len() {
            let bone = self.scratch[i].bone as usize;
            // The bone's rest pose (set above).
            let bind = self.locals[bone];
            let (mut t, mut q, mut s) = (Vec3::ZERO, Vec4::ZERO, 0.0f32);
            let first: Option<Vec4> = self.last_rotations.get(bone).map(|r| Vec4::from(*r));
            let mut free = 1.0f32;
            while i < self.scratch.len() && self.scratch[i].bone as usize == bone {
                // One priority group.
                let p = self.scratch[i].priority;
                let g0 = i;
                let (mut sum, mut ease) = (0.0, 0.0);
                while i < self.scratch.len() && self.scratch[i].bone as usize == bone && self.scratch[i].priority == p {
                    sum += self.scratch[i].weight;
                    ease += self.scratch[i].ease;
                    i += 1;
                }
                if free <= 1e-6 {
                    continue;
                }
                let take = ease.min(free);
                free -= take;
                for c in &self.scratch[g0..i] {
                    // Clips all at weight 0 share by their fades.
                    let f = if sum > 1e-6 { c.weight / sum * take } else { c.ease / ease * take };
                    let l = &self.layers[c.layer as usize];
                    let track = &l.clip.tracks[c.track as usize].1;
                    let time = l.sample_time(phase);
                    t += f * track.translation.sample(time).map_or(bind.translation, Vec3::from_array);
                    let r = track.rotation_after(time, l.cycles()).map_or(bind.rotation, |[w, x, y, z]| Quat::from_xyzw(x, y, z, w));
                    let mut v = Vec4::from(r);
                    let reference = l.aligned.get(c.slot as usize).copied().flatten().or(first).unwrap_or(v);
                    if v.dot(reference) < 0.0 {
                        v = -v;
                    }
                    q += f * v;
                    s += f * track.scale.sample(time).map_or(bind.scale.x, |[x]| x);
                    if let Some(a) = self.layers[c.layer as usize].aligned.get_mut(c.slot as usize) {
                        *a = Some(v);
                    }
                }
            }
            if free > 1e-6 {
                t += free * bind.translation;
                let mut v = Vec4::from(bind.rotation);
                if first.is_some_and(|r| v.dot(r) < 0.0) {
                    v = -v;
                }
                q += free * v;
                s += free * bind.scale.x;
            }
            let rotation = if q.length_squared() > 1e-12 { Quat::from_vec4(q).normalize() } else { bind.rotation };
            let posed = Transform { translation: t, rotation, scale: Vec3::splat(s) };
            // Finite samples far enough apart can still overflow the sum: the rest pose then.
            if posed.translation.is_finite() && posed.rotation.is_finite() && posed.scale.is_finite() {
                self.locals[bone] = posed;
            }
        }
        self.seamed = self.wrapped && self.seams.is_some();
        if let Some(s) = self.seams.as_mut() {
            s.smooth(&mut self.locals, self.wrapped, self.jump_seam, self.last_dt);
            // A pose asked for again without time passing is not a second seam.
            (self.wrapped, self.jump_seam) = (false, false);
        }
    }

    /// Node `i`'s local transform from the layers `pick` accepts alone, at their times,
    /// shared by weight × fade whatever their priorities (what those clips would give the
    /// node by themselves); `None` when none of them has a track for it.
    pub fn local_from(&self, i: usize, pick: impl Fn(&Layer) -> bool) -> Option<Transform> {
        let bind = self.skel.locals.get(i).copied()?;
        let (mut t, mut q, mut s, mut sum) = (Vec3::ZERO, Vec4::ZERO, 0.0f32, 0.0f32);
        let mut first: Option<Vec4> = None;
        for l in self.layers.iter().filter(|l| l.ease > 0.0 && pick(l)) {
            let w = (l.weight * l.shown_ease()).max(1e-4);
            for &(_, track, _) in l.bound.iter().filter(|b| b.0 as usize == i) {
                let track = &l.clip.tracks[track as usize].1;
                let time = l.sample_time(self.phase);
                t += w * track.translation.sample(time).map_or(bind.translation, Vec3::from_array);
                let mut v = Vec4::from(track.rotation_after(time, l.cycles()).map_or(bind.rotation, |[w, x, y, z]| Quat::from_xyzw(x, y, z, w)));
                if v.dot(*first.get_or_insert(v)) < 0.0 {
                    v = -v;
                }
                q += w * v;
                s += w * track.scale.sample(time).map_or(bind.scale.x, |[x]| x);
                sum += w;
            }
        }
        (sum > 0.0 && q.length_squared() > 1e-12).then(|| Transform { translation: t / sum, rotation: Quat::from_vec4(q).normalize(), scale: Vec3::splat(s / sum) })
    }

    /// Write the pose to the skeleton's node entities (the root excluded) and to the
    /// extra nodes.
    pub fn apply(&self, transforms: &mut Query<&mut Transform>) {
        let extra = self.extra.iter().map(|x| &x.entity).zip(self.locals.iter().skip(self.skel.locals.len()));
        for (e, local) in self.nodes.iter().zip(&self.locals).skip(1).chain(extra) {
            if let Ok(mut tr) = transforms.get_mut(*e) {
                if *tr != *local {
                    *tr = *local;
                }
            }
        }
    }

    /// Node `i`'s transform relative to the actor's root in the current pose (along the
    /// skeleton's own hierarchy).
    pub fn model_space(&self, i: usize) -> Affine3A {
        let mut m = Affine3A::IDENTITY;
        let mut cur = Some(i);
        let mut depth = 0;
        while let Some(n) = cur.filter(|&n| n > 0) {
            let l = self.locals[n];
            m = Affine3A::from_scale_rotation_translation(l.scale, l.rotation, l.translation) * m;
            cur = self.skel.parents[n];
            depth += 1;
            if depth > 256 {
                break;
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_nif::anim::{Channel, TransformTrack};
    use std::collections::HashMap;

    /// Root, Bip01 (accumulation root), Bone A, Bone B.
    fn skeleton() -> Arc<PreparedSkeleton> {
        let names = ["Scene Root", "Bip01", "A", "B"];
        let locals = vec![Transform::IDENTITY, Transform::from_xyz(0.0, 0.0, 60.0), Transform::from_xyz(1.0, 0.0, 0.0), Transform::from_xyz(0.0, 2.0, 0.0)];
        Arc::new(PreparedSkeleton {
            names: names.iter().map(|n| Some(n.to_string())).collect(),
            parents: vec![None, Some(0), Some(1), Some(2)],
            locals,
            bind_world: vec![Affine3A::IDENTITY; 4],
            by_name: names.iter().enumerate().map(|(i, n)| (n.to_string(), i)).collect::<HashMap<_, _>>(),
            ..Default::default()
        })
    }

    /// A clip moving each named node to a fixed translation, at a priority.
    fn clip(tracks: &[(&str, [f32; 3], i32)], len: f32) -> Arc<Clip> {
        Arc::new(Clip {
            name: "c".into(),
            start: 0.0,
            stop: len,
            frequency: 1.0,
            cycle_type: 0,
            tracks: tracks
                .iter()
                .map(|(n, t, _)| {
                    let tt = TransformTrack { translation: Channel::Static(Some(*t)), rotation: Channel::Static(None), euler: None, scale: Channel::Static(None), per_cycle: None };
                    (n.to_string(), Arc::new(tt))
                })
                .collect(),
            priorities: tracks.iter().map(|t| t.2).collect(),
            skipped: 0,
            accum_root: Some("Bip01".into()),
            text_keys: Vec::new(),
            visibility: Vec::new(),
            emitters: Vec::new(),
            materials: Vec::new(),
            free: Vec::new(),
            free_visibility: Vec::new(),
            lights: Vec::new(),
        })
    }

    fn animator() -> Animator {
        let nodes = (1..=4).map(|i| Entity::from_raw_u32(i).unwrap()).collect();
        Animator::new(skeleton(), nodes, Some("Bip01"))
    }

    #[test]
    fn higher_priority_wins_and_lower_fills_the_rest() {
        let mut a = animator();
        let base = clip(&[("A", [10.0, 0.0, 0.0], 10), ("B", [0.0, 10.0, 0.0], 10), ("Bip01", [5.0, 5.0, 5.0], 10)], 1.0);
        let top = clip(&[("A", [20.0, 0.0, 0.0], 30)], 1.0);
        a.want(&base, 0, 1.0, Mode::Loop, 0.0);
        a.want(&top, 1, 1.0, Mode::Loop, 0.0);
        a.pose();
        assert_eq!(a.locals[2].translation, Vec3::new(20.0, 0.0, 0.0), "the higher priority takes the bone");
        assert_eq!(a.locals[3].translation, Vec3::new(0.0, 10.0, 0.0), "bones it does not drive keep the base");
        assert_eq!(a.locals[1], Transform::IDENTITY, "the accumulation root is held");
        // Half faded in on top: the base fills the other half.
        a.layers[1].ease = 0.5;
        a.pose();
        assert!((a.locals[2].translation.x - 15.0).abs() < 1e-5);
        // Nothing on a bone at all: its bind pose.
        a.layers.clear();
        a.pose();
        assert_eq!(a.locals[2].translation, Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn a_leaving_layer_shown_as_nothing_leaves_the_pose_to_the_others() {
        let mut a = animator();
        let base = clip(&[("A", [10.0, 0.0, 0.0], 10)], 1.0);
        let top = clip(&[("A", [20.0, 0.0, 0.0], 30)], 1.0);
        a.want(&base, 0, 1.0, Mode::Loop, 0.0);
        a.want(&top, 1, 1.0, Mode::Loop, 0.0);
        // Its fade a hair over nothing, on the way out along the settling curve: shown as 0.
        let l = &mut a.layers[1];
        l.smooth = true;
        l.set_leaving(true);
        l.ease = 1e-9;
        assert_eq!(l.shown_ease(), 0.0, "the fade shows as nothing");
        a.pose();
        assert_eq!(a.locals[2].translation, Vec3::new(10.0, 0.0, 0.0), "the base takes the bone, not its bind pose");
    }

    /// A clip turning node `name` about z by `angle` (radians), at a priority.
    fn turning(name: &str, angle: f32, priority: i32) -> Arc<Clip> {
        let c = clip(&[(name, [0.0; 3], priority)], 1.0);
        let mut c = Arc::into_inner(c).expect("a new clip");
        let q = Quat::from_rotation_z(angle);
        c.tracks[0].1 = Arc::new(TransformTrack { translation: Channel::Static(None), rotation: Channel::Static(Some([q.w, q.x, q.y, q.z])), euler: None, scale: Channel::Static(None), per_cycle: None });
        Arc::new(c)
    }

    #[test]
    fn two_clips_crossing_half_a_turn_apart_blend_without_a_flip() {
        // A holds the bone still; B turns it on from 120° to 240°, both at one priority: the
        // blend follows B's turn by its share and never flips round. B a new layer each frame (no history: the side of the bone's last pose), then one
        // layer whose clip turns on (its own last side), down to a quarter share.
        for (kept, share) in [(false, 1.0), (true, 1.0), (true, 0.27)] {
            let mut a = animator();
            a.want(&turning("A", 0.0, 10), 0, 1.0, Mode::Loop, 0.0);
            let mut last: Option<Quat> = None;
            for k in 0..=60 {
                let b = turning("A", (120.0 + 2.0 * k as f32).to_radians(), 10);
                if kept && k > 0 {
                    a.layers[1].clip = b;
                } else {
                    a.restart(&b, 1, 1.0, Mode::Loop, 0.0);
                }
                a.layers[1].weight = share;
                a.layers[0].weight = 1.0 - share * 0.5;
                a.pose();
                let now = a.locals[2].rotation;
                if let Some(l) = last {
                    assert!(l.angle_between(now).to_degrees() < 3.0, "kept {kept}, share {share}: the blend jumped {}° at {}°", l.angle_between(now).to_degrees(), 120 + 2 * k);
                }
                last = Some(now);
            }
        }
    }

    #[test]
    fn script_sequence_priority_is_independent_of_bone_priority_and_fades_out() {
        let mut a = animator();
        let base = clip(&[("A", [10.0,0.0,0.0], 200)],1.0);
        let overlay = clip(&[("A", [30.0,0.0,0.0], 1)],1.0);
        a.begin();
        a.want(&base,0,1.0,Mode::Loop,0.0);
        let slot = a.want(&overlay,1,1.0,Mode::Once,0.0);
        a.layers[slot].sequence_priority = 1;
        a.layers[slot].fade_out = 0.4;
        a.end();
        a.pose();
        assert_eq!(a.locals[2].translation.x,30.0,"sequence priority wins over the KF bone priority");
        a.begin();
        a.want(&base,0,1.0,Mode::Loop,0.0);
        a.end();
        a.advance(0.2,1.0);
        a.pose();
        assert!((a.locals[2].translation.x-20.0).abs()<1e-4,"the explicit 0.4-second fade is halfway out");
        a.advance(0.21,1.0);
        a.pose();
        assert_eq!(a.locals[2].translation.x,10.0);
    }

    #[test]
    fn admitted_pose_time_moves_a_smoothed_overlay_without_ticking_ordinary_layers() {
        use fnv_nif::anim::{Key, KeyKind, Keys};
        let mut a = animator();
        a.smooth_seams(Some(0.1));
        let base = clip(&[("A", [10.0, 0.0, 0.0], 200), ("B", [0.0, 10.0, 0.0], 200)], 1.0);
        let mut overlay = (*clip(&[("A", [30.0, 0.0, 0.0], 1)], 1.0)).clone();
        overlay.tracks[0].1 = Arc::new(TransformTrack {
            translation: Channel::Keys(Keys { kind: KeyKind::Linear, keys: [(0.0, 30.0), (1.0, 50.0)].into_iter().map(|(time, x)| Key { time, value: [x, 0.0, 0.0], forward: [0.0; 3], backward: [0.0; 3], tbc: [0.0; 3] }).collect() }),
            rotation: Channel::Static(None), euler: None, scale: Channel::Static(None), per_cycle: None,
        });
        let overlay = Arc::new(overlay);
        let base_slot = a.want(&base, 0, 1.0, Mode::Loop, 0.5);
        a.layers[base_slot].ease = 0.5;
        a.layers[base_slot].time = 0.125;
        a.pose();
        let ordinary_pose = a.locals[3];
        a.begin();
        a.want(&base, 0, 1.0, Mode::Loop, 0.5);
        let slot = a.want(&overlay, 1, 1.0, Mode::Once, 0.25);
        a.layers[slot].sequence_priority = 1;
        a.end();
        let phase = a.phase;
        a.advance(0.0, 1.0);
        a.advance_pose_blends(0.25, |layer| layer.tag == 1);
        a.pose();
        assert_eq!(a.locals[2].translation.x, 30.0, "the smoothed pose must accept the admitted menu sample");
        assert_eq!(a.locals[3], ordinary_pose, "the uncovered ordinary pose stays held");
        assert_eq!((a.layers[base_slot].ease, a.layers[base_slot].time), (0.5, 0.125));
        assert_eq!(a.layers[slot].time, 0.0, "selected blends do not advance sample time");
        assert_eq!(a.phase, phase, "locomotion phase stays held");
        a.layers[slot].time = 0.5;
        a.advance(0.0, 1.0);
        a.advance_pose_blends(0.0, |_| true);
        a.pose();
        assert_eq!(a.locals[2].translation.x, 30.0, "a frame without admitted time keeps its shown pose");
    }

    #[test]
    fn a_rebuilt_model_goes_on_in_the_old_ones_pose() {
        let idle = clip(&[("A", [10.0, 0.0, 0.0], 10), ("B", [0.0, 10.0, 0.0], 10)], 2.0);
        let mut old = animator();
        old.begin();
        old.want(&idle, 0, 1.0, Mode::Loop, DEFAULT_FADE);
        old.end();
        for _ in 0..10 {
            old.advance(0.05, 1.0);
        }
        old.pose();
        // A fresh animator asking for the same clip starts faded out: the bind pose shows.
        let mut fresh = animator();
        fresh.begin();
        fresh.want(&idle, 0, 1.0, Mode::Loop, DEFAULT_FADE);
        fresh.end();
        fresh.pose();
        assert_eq!(fresh.locals[2].translation, Vec3::new(1.0, 0.0, 0.0), "the bind pose (the T-pose)");
        // Going on from the old one: the same pose on its first frame, the clip at its time.
        let mut new = animator();
        new.continue_from(&old);
        assert_eq!(new.locals[2], old.locals[2]);
        new.begin();
        new.want(&idle, 0, 1.0, Mode::Loop, DEFAULT_FADE);
        new.end();
        new.pose();
        assert_eq!(new.locals[2].translation, Vec3::new(10.0, 0.0, 0.0));
        assert_eq!(new.layers[0].time, old.layers[0].time);
    }

    #[test]
    fn a_clip_started_again_eases_across_its_jump_when_asked() {
        // A fire clip that kicks from 0 to 1 over its 0.2 seconds, started again from its
        // start every 0.15 seconds, as by rapid fire.
        let fire = |smooth: bool| {
            let mut a = animator();
            a.smooth_seams(smooth.then_some(0.1));
            use fnv_nif::anim::{Key, KeyKind, Keys};
            let key = |time: f32, x: f32| Key { time, value: [x, 0.0, 0.0], forward: [0.0; 3], backward: [0.0; 3], tbc: [0.0; 3] };
            let base = clip(&[("A", [0.0, 0.0, 0.0], 30)], 0.2);
            let kick = TransformTrack { translation: Channel::Keys(Keys { kind: KeyKind::Linear, keys: vec![key(0.0, 0.0), key(0.2, 1.0)] }), rotation: Channel::Static(None), euler: None, scale: Channel::Static(None), per_cycle: None };
            let x = Arc::new(Clip { tracks: vec![("A".to_string(), Arc::new(kick))], ..(*base).clone() });
            let mut xs = Vec::new();
            for frame in 0..60 {
                if frame % 9 == 0 {
                    a.restart(&x, 70, 1.0, Mode::Once, 0.0);
                    a.smooth_jump();
                }
                a.advance(1.0 / 60.0, 1.0);
                a.pose();
                xs.push(a.locals[2].translation.x);
            }
            xs.windows(2).skip(1).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max)
        };
        assert!(fire(false) > 0.6, "each start snaps back: {}", fire(false));
        assert!(fire(true) < 0.15, "eased across: {}", fire(true));
    }

    #[test]
    fn a_clip_started_with_a_sharp_move_is_never_flung_past_it() {
        // A lever-action shot: from the held pose at 0 the clip kicks to 1 in 0.03 s and
        // comes back over the rest of its 0.3 s, started again from a still pose. Easing
        // across the start must not push the pose outside the clip's own way (the left
        // hand left the Trail Carbine at each shot, `toward_rest`).
        use fnv_nif::anim::{Key, KeyKind, Keys};
        let key = |time: f32, x: f32| Key { time, value: [x, 0.0, 0.0], forward: [0.0; 3], backward: [0.0; 3], tbc: [0.0; 3] };
        let base = clip(&[("A", [0.0, 0.0, 0.0], 30)], 0.3);
        let kick = TransformTrack { translation: Channel::Keys(Keys { kind: KeyKind::Linear, keys: vec![key(0.0, 0.0), key(0.03, 1.0), key(0.3, 0.0)] }), rotation: Channel::Static(None), euler: None, scale: Channel::Static(None), per_cycle: None };
        let shot = Arc::new(Clip { tracks: vec![("A".to_string(), Arc::new(kick))], ..(*base).clone() });
        let mut a = animator();
        a.smooth_seams(Some(0.3));
        for _ in 0..30 {
            a.begin();
            a.want(&base, 0, 1.0, Mode::Loop, 0.0);
            a.end();
            a.advance(1.0 / 60.0, 1.0);
            a.pose();
        }
        let mut lowest = f32::MAX;
        for frame in 0..40 {
            a.begin();
            a.want(&base, 0, 1.0, Mode::Loop, 0.0);
            if frame == 0 {
                a.restart(&shot, 70, 1.0, Mode::Once, 0.0);
                a.smooth_jump();
            } else {
                a.want(&shot, 70, 1.0, Mode::Once, 0.0);
            }
            a.end();
            a.advance(1.0 / 60.0, 1.0);
            a.pose();
            lowest = lowest.min(a.locals[2].translation.x);
        }
        assert!(lowest > -0.02, "flung below the held pose to {lowest}");
    }

    #[test]
    fn a_smoothed_loop_seam_carries_on_and_eases_onto_the_clip() {
        // A loop that runs from 0 up to 1 over a second and starts again at 0: a jump and a
        // turn of direction at every wrap.
        let saw = |smooth: Option<f32>| {
            let mut a = animator();
            a.smooth_seams(smooth);
            use fnv_nif::anim::{Key, KeyKind, Keys};
            let key = |time: f32, x: f32| Key { time, value: [x, 0.0, 0.0], forward: [0.0; 3], backward: [0.0; 3], tbc: [0.0; 3] };
            let base = clip(&[("A", [0.0, 0.0, 0.0], 30)], 1.0);
            let ramp = TransformTrack { translation: Channel::Keys(Keys { kind: KeyKind::Linear, keys: vec![key(0.0, 0.0), key(1.0, 1.0)] }), rotation: Channel::Static(None), euler: None, scale: Channel::Static(None), per_cycle: None };
            let x = Arc::new(Clip { tracks: vec![("A".to_string(), Arc::new(ramp))], ..(*base).clone() });
            let mut xs = Vec::new();
            for _ in 0..180 {
                a.begin();
                a.want(&x, 0, 1.0, Mode::Loop, 0.0);
                a.end();
                a.advance(1.0 / 60.0, 1.0);
                a.pose();
                xs.push(a.locals[2].translation.x);
            }
            xs
        };
        let raw = saw(None);
        let biggest_raw = raw.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(biggest_raw > 0.9, "the clip itself jumps back at the seam: {biggest_raw}");
        let smooth = saw(Some(0.25));
        let biggest = smooth.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(biggest < 0.1, "no jump when smoothed: {biggest}");
        // Half a second after each seam the pose is the clip's again.
        for (k, (s, r)) in smooth.iter().zip(&raw).enumerate() {
            if k % 60 >= 45 && k > 60 {
                assert!((s - r).abs() < 0.02, "frame {k}: {s} against {r}");
            }
        }
    }

    #[test]
    fn a_smooth_crossover_shares_the_bones_by_one_eased_curve() {
        let mut a = animator();
        let x = clip(&[("A", [0.0, 0.0, 0.0], 30)], 1.0);
        let y = clip(&[("A", [1.0, 0.0, 0.0], 30)], 1.0);
        a.begin();
        a.want(&x, 0, 1.0, Mode::Loop, 0.0);
        a.end();
        a.advance(0.1, 1.0);
        // Cross over to y over 0.4 s, both smooth, the leaving one over the same time.
        a.begin();
        a.want(&y, 0, 1.0, Mode::Loop, 0.4);
        a.end();
        for l in &mut a.layers {
            (l.smooth, l.fade_out) = (true, 0.4);
        }
        let mut shares = Vec::new();
        for _ in 0..8 {
            a.advance(0.05, 1.0);
            a.pose();
            shares.push(a.locals[2].translation.x);
            a.begin();
            a.want(&y, 0, 1.0, Mode::Loop, 0.4);
            a.end();
        }
        // y's share follows the settling curve of the time: quick first, slow at the end.
        for (k, s) in shares.iter().enumerate() {
            let t = (k + 1) as f32 * 0.05 / 0.4;
            assert!((s - settle(t)).abs() < 1e-3, "{k}: {s} against {}", settle(t));
        }
        assert!(shares[2] > 0.6 && shares[5] > 0.95 && shares[5] < 1.0, "{shares:?}");
    }

    #[test]
    fn a_layer_straightened_mid_fade_keeps_its_place_and_fades_straight() {
        let mut a = animator();
        let x = clip(&[("A", [0.0, 0.0, 0.0], 30)], 1.0);
        // y over x: its fade is how much of the bone it takes.
        let y = clip(&[("A", [1.0, 0.0, 0.0], 40)], 1.0);
        a.begin();
        a.want(&x, 0, 1.0, Mode::Loop, 0.0);
        a.end();
        a.begin();
        a.want(&x, 0, 1.0, Mode::Loop, 0.0);
        a.want(&y, 1, 1.0, Mode::Loop, 0.4);
        a.end();
        a.layers[1].smooth = true;
        a.advance(0.1, 1.0);
        a.pose();
        let before = a.locals[2].translation.x;
        assert!((before - settle(0.25)).abs() < 1e-4, "{before}");
        // The settling curve switched off part-way (Animation Smoothing off): no step.
        a.layers[1].straighten();
        assert!(!a.layers[1].smooth);
        a.pose();
        assert!((a.locals[2].translation.x - before).abs() < 1e-5, "{} against {before}", a.locals[2].translation.x);
        // From there straight at its own rate: 0.05 s of a 0.4 s fade is an eighth more.
        a.advance(0.05, 1.0);
        a.pose();
        assert!((a.locals[2].translation.x - (before + 0.125)).abs() < 1e-4, "{}", a.locals[2].translation.x);
        // A straight layer stays as it is.
        let ease = a.layers[0].ease;
        a.layers[0].straighten();
        assert_eq!(a.layers[0].ease, ease);
    }

    #[test]
    fn the_settling_curve_is_quick_first_and_slow_last_and_inverts() {
        assert_eq!(settle(0.0), 0.0);
        assert!((settle(1.0) - 1.0).abs() < 1e-6);
        assert!(settle(0.02) < 0.01, "no jolt at the start");
        assert!(settle(0.33) > 0.5 && 1.0 - settle(0.75) < 0.06);
        for y in [0.0, 0.1, 0.5, 0.9, 0.99, 1.0] {
            assert!((settle(settle_inv(y)) - y).abs() < 1e-4, "{y}");
        }
    }

    #[test]
    fn a_smooth_layer_turning_back_mid_fade_keeps_its_place() {
        let mut a = animator();
        let x = clip(&[("A", [0.0, 0.0, 0.0], 30)], 1.0);
        let y = clip(&[("A", [1.0, 0.0, 0.0], 30)], 1.0);
        a.begin();
        a.want(&x, 0, 1.0, Mode::Loop, 0.0);
        a.end();
        a.advance(0.1, 1.0);
        let ask = |a: &mut Animator, c: &Arc<Clip>| {
            a.begin();
            a.want(c, 0, 1.0, Mode::Loop, 0.4);
            a.end();
            for l in &mut a.layers {
                (l.smooth, l.fade_out) = (true, 0.4);
            }
        };
        // Towards y for a while, then back to x: the pose carries on from where it is.
        ask(&mut a, &y);
        for _ in 0..4 {
            a.advance(0.05, 1.0);
            ask(&mut a, &y);
        }
        a.pose();
        let before = a.locals[2].translation.x;
        ask(&mut a, &x);
        a.advance(1e-4, 1.0);
        a.pose();
        let after = a.locals[2].translation.x;
        assert!(before > 0.5 && (after - before).abs() < 0.01, "{before} then {after}");
    }

    #[test]
    fn equal_priorities_share_by_weight_and_fade() {
        let mut a = animator();
        let x = clip(&[("A", [10.0, 0.0, 0.0], 30)], 1.0);
        let y = clip(&[("A", [0.0, 10.0, 0.0], 30)], 1.0);
        a.begin();
        a.want(&x, 0, 1.0, Mode::Loop, 0.0);
        a.end();
        a.advance(0.1, 1.0);
        // Switch to y with a 0.2 s fade in; x fades out over DEFAULT_FADE.
        a.begin();
        a.want(&y, 0, 1.0, Mode::Loop, 0.2);
        a.end();
        a.advance(0.1, 1.0);
        a.pose();
        let t = a.locals[2].translation;
        assert!((t.x - 5.0).abs() < 1e-4 && (t.y - 5.0).abs() < 1e-4, "{t}");
        a.begin();
        a.want(&y, 0, 1.0, Mode::Loop, 0.2);
        a.end();
        a.advance(0.2, 1.0);
        assert_eq!(a.layers.len(), 1, "a faded-out layer is dropped");
        a.pose();
        assert_eq!(a.locals[2].translation, Vec3::new(0.0, 10.0, 0.0));
    }

    /// A rifle's aim: `level` has the spine (A) and the arms (B), `up` the spine only,
    /// both over an idle that has both bones.
    #[test]
    fn a_bone_one_clip_has_alone_stays_with_it_at_any_weight() {
        let mut a = animator();
        let idle = clip(&[("A", [1.0, 1.0, 1.0], 10), ("B", [2.0, 2.0, 2.0], 10)], 1.0);
        let level = clip(&[("A", [10.0, 0.0, 0.0], 30), ("B", [0.0, 10.0, 0.0], 35)], 1.0);
        let up = clip(&[("A", [30.0, 0.0, 0.0], 30)], 1.0);
        for (wl, wu) in [(1.0, 0.0), (0.75, 0.25), (0.25, 0.75), (0.0, 1.0)] {
            a.begin();
            a.want(&idle, 0, 1.0, Mode::Loop, 0.0);
            a.want(&level, 1, wl, Mode::Loop, 0.0);
            a.want(&up, 2, wu, Mode::Loop, 0.0);
            a.end();
            // Long enough for the weights to reach their targets.
            a.advance(1.0, 1.0);
            a.pose();
            assert!((a.locals[2].translation.x - (10.0 * wl + 30.0 * wu)).abs() < 1e-4, "the spine shares by weight at {wl}/{wu}: {}", a.locals[2].translation);
            assert_eq!(a.locals[3].translation, Vec3::new(0.0, 10.0, 0.0), "the arms stay with the level clip at {wl}/{wu}");
        }
        // Let go of both: they fade out together and the idle comes through on both bones.
        a.begin();
        a.want(&idle, 0, 1.0, Mode::Loop, 0.0);
        a.end();
        a.advance(DEFAULT_FADE * 0.75, 1.0);
        a.pose();
        assert!((a.locals[3].translation - Vec3::new(1.5, 4.0, 1.5)).length() < 1e-4, "{}", a.locals[3].translation);
        a.advance(DEFAULT_FADE, 1.0);
        a.pose();
        assert_eq!(a.layers.len(), 1);
        assert_eq!((a.locals[2].translation, a.locals[3].translation), (Vec3::splat(1.0), Vec3::splat(2.0)));
    }

    #[test]
    fn a_weight_change_moves_the_share_without_a_jump() {
        let mut a = animator();
        let x = clip(&[("A", [10.0, 0.0, 0.0], 30)], 1.0);
        let y = clip(&[("A", [0.0, 10.0, 0.0], 30)], 1.0);
        let step = |a: &mut Animator, wx: f32, wy: Option<f32>, dt: f32| {
            a.begin();
            a.want(&x, 0, wx, Mode::Loop, DEFAULT_FADE);
            if let Some(wy) = wy {
                a.want(&y, 1, wy, Mode::Loop, DEFAULT_FADE);
            }
            a.end();
            a.advance(dt, 1.0);
            a.pose();
            a.locals[2].translation
        };
        assert_eq!(step(&mut a, 0.5, Some(0.5), 1.0), Vec3::new(5.0, 5.0, 0.0));
        // y is let go and x asked for in full: the next frame is still close to half and half.
        let t = step(&mut a, 1.0, None, 0.01);
        assert!((t.x - 5.0).abs() < 0.5 && (t.y - 5.0).abs() < 0.5, "{t}");
        let t = step(&mut a, 1.0, None, 1.0);
        assert_eq!(t, Vec3::new(10.0, 0.0, 0.0));
    }

    #[test]
    fn the_highest_priority_visibility_track_decides() {
        use fnv_nif::anim::BoolTrack;
        let with = |priority: i32, track: BoolTrack| {
            let mut c = (*clip(&[("A", [0.0; 3], 30)], 1.0)).clone();
            c.visibility = vec![("##Gun".to_string(), priority, track)];
            Arc::new(c)
        };
        let mut a = animator();
        assert_eq!(a.visible("##Gun"), None);
        // The holster clip hides the part; the draw clip, above it, shows it half way.
        let holster = with(0, BoolTrack { value: Some(false), keys: Vec::new() });
        let draw = with(55, BoolTrack { value: None, keys: vec![(0.0, false), (0.5, true)] });
        a.want(&holster, 0, 1.0, Mode::Once, 0.0);
        assert_eq!(a.visible("##Gun"), Some(false));
        a.want(&draw, 1, 1.0, Mode::Once, 0.0);
        a.advance(0.25, 1.0);
        assert_eq!(a.visible("##Gun"), Some(false));
        a.advance(0.5, 1.0);
        assert_eq!(a.visible("##Gun"), Some(true));
        assert_eq!(a.visible("##Other"), None);
    }

    #[test]
    fn a_clip_lights_and_puts_out_a_weapon_emitter() {
        use crate::anim_play::EmitterTrack;
        use fnv_nif::anim::BoolTrack;
        // The dynamite's throw: the fuse on at 1.0 s, off at the release (1.833 s), at
        // 21.6 a second; no rate keys, so the rate is the interpolator's lone value.
        let mut c = (*clip(&[("A", [0.0; 3], 30)], 2.6)).clone();
        let active = BoolTrack { value: None, keys: vec![(0.0, false), (1.0, true), (1.833, false)] };
        c.emitters = vec![EmitterTrack { system: "##PCloud01".into(), priority: 55, rate: None, active: Some(active), modifiers: Vec::new() }];
        let throw = Arc::new(c);
        let mut a = animator();
        assert_eq!(a.emitter("##PCloud01"), None);
        a.want(&throw, 0, 1.0, Mode::Once, 0.0);
        a.advance(0.5, 1.0);
        assert_eq!(a.emitter("##PCloud01"), Some((false, None)));
        a.advance(0.75, 1.0);
        assert_eq!(a.emitter("##PCloud01"), Some((true, None)));
        a.advance(0.75, 1.0);
        assert_eq!(a.emitter("##PCloud01"), Some((false, None)));
        assert_eq!(a.emitter("##Other"), None);
    }

    #[test]
    fn a_rest_pose_stands_in_for_the_bind_pose() {
        let mut a = animator();
        let rest = Transform::from_xyz(0.0, 0.0, 7.0);
        a.set_rest(3, Some(rest));
        a.pose();
        assert_eq!(a.locals[3], rest, "no clip drives the node");
        // A clip on it, half faded in: the rest pose fills the other half.
        let c = clip(&[("B", [0.0, 0.0, 1.0], 30)], 1.0);
        a.want(&c, 0, 1.0, Mode::Loop, 0.0);
        a.layers[0].ease = 0.5;
        a.pose();
        assert!((a.locals[3].translation.z - 4.0).abs() < 1e-5, "{}", a.locals[3].translation);
        a.layers.clear();
        a.set_rest(3, None);
        a.pose();
        assert_eq!(a.locals[3].translation, Vec3::new(0.0, 2.0, 0.0), "the bind pose again");
    }

    /// A pistol's slide: a node of the weapon model, not of the skeleton.
    #[test]
    fn clips_drive_extra_nodes_by_name() {
        let mut a = animator();
        let bind = Transform::from_xyz(5.0, 0.0, 0.0);
        let part = |name: &str| ExtraNode { name: name.to_string(), entity: Entity::from_raw_u32(9).unwrap(), bind };
        let fire = clip(&[("A", [1.0, 0.0, 0.0], 30), ("##Slide", [2.0, 0.0, 0.0], 55), ("##Other", [7.0, 0.0, 0.0], 55)], 1.0);
        // Started before the weapon is there: nothing of it to drive yet.
        a.want(&fire, 0, 1.0, Mode::Once, 0.0);
        a.pose();
        assert_eq!(a.locals.len(), 4);
        // The weapon arrives: the playing clip takes its slide; a name the skeleton has
        // stays the skeleton's.
        a.set_extra(vec![part("##Slide"), part("A"), part("##Idle")]);
        assert_eq!(a.extra_driven(&fire), ["##Slide"]);
        a.pose();
        assert_eq!(a.locals.len(), 7);
        assert_eq!(a.locals[4].translation, Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(a.locals[5], bind, "the skeleton's A is driven, not the weapon's");
        assert_eq!(a.locals[2].translation, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(a.locals[6], bind, "a part no clip drives keeps its place in the model");
        assert_eq!(a.extra_moved().collect::<Vec<_>>(), [("##Slide", 3.0, 0.0)]);
        // Half faded in: the rest is the part's place in the model.
        a.layers[0].ease = 0.5;
        a.pose();
        assert!((a.locals[4].translation.x - 3.5).abs() < 1e-5);
        // The weapon goes: the clip plays on without it.
        a.set_extra(Vec::new());
        a.pose();
        assert_eq!(a.locals.len(), 4);
        assert_eq!(a.locals[2].translation, Vec3::new(1.0, 0.0, 0.0));
    }

    /// The 9mm pistol: its draw clip cocks the hammer, the aim clip does not key it, and
    /// two nodes share the magazine's name.
    #[test]
    fn parts_rest_where_the_draw_clip_leaves_them() {
        let mut a = animator();
        let bind = Transform::from_xyz(5.0, 0.0, 0.0);
        let part = |name: &str| ExtraNode { name: name.to_string(), entity: Entity::from_raw_u32(9).unwrap(), bind };
        a.set_extra(vec![part("##Hammer"), part("##Clip"), part("##Clip"), part("##Bar")]);
        let draw = clip(&[("##Hammer", [8.0, 0.0, 0.0], 0)], 1.0);
        let reload = clip(&[("##Clip", [3.0, 0.0, 0.0], 0)], 1.0);
        // In the hand: the hammer rests where the draw clip leaves it, the others at their
        // place in the model.
        a.set_extra_rest(Some(&draw));
        a.pose();
        assert_eq!(a.locals[4].translation.x, 8.0);
        assert_eq!(a.extra_rest(0).map(|t| t.translation.x), Some(8.0));
        assert_eq!(a.locals[7], bind);
        // A clip driving it fades out back to that rest pose, not to the model's.
        let fire = clip(&[("##Hammer", [0.0, 0.0, 0.0], 0)], 1.0);
        a.want(&fire, 0, 1.0, Mode::Once, 0.0);
        a.layers[0].ease = 0.5;
        a.pose();
        assert!((a.locals[4].translation.x - 4.0).abs() < 1e-5);
        a.cut(0);
        // Every node of the magazine's name moves with its track.
        a.want(&reload, 1, 1.0, Mode::Once, 0.0);
        a.pose();
        assert_eq!((a.locals[5].translation.x, a.locals[6].translation.x), (3.0, 3.0));
        assert_eq!(a.extra_driven(&reload), ["##Clip", "##Clip"]);
        // Holstered: back to the model's place.
        a.cut(1);
        a.set_extra_rest(None);
        a.pose();
        assert_eq!(a.locals[4], bind);
        // A new weapon drops the old one's rest poses.
        a.set_extra_rest(Some(&draw));
        a.set_extra(vec![part("##Hammer")]);
        a.pose();
        assert_eq!(a.locals[4], bind);
    }

    #[test]
    fn synced_layers_share_a_phase() {
        let mut a = animator();
        let long = clip(&[("A", [1.0, 0.0, 0.0], 30)], 2.0);
        let short = clip(&[("B", [1.0, 0.0, 0.0], 30)], 1.0);
        a.want(&long, 0, 0.5, Mode::Synced, 0.0);
        a.want(&short, 1, 0.5, Mode::Synced, 0.0);
        // Weighted length 1.5 s: a quarter of it moves the phase by a quarter.
        a.advance(0.375, 1.0);
        assert!((a.phase - 0.25).abs() < 1e-5, "{}", a.phase);
        assert!((a.layers[0].sample_time(a.phase) - 0.5).abs() < 1e-5);
        assert!((a.layers[1].sample_time(a.phase) - 0.25).abs() < 1e-5);
        // Twice the rate, twice the phase step.
        a.advance(0.375, 2.0);
        assert!((a.phase - 0.75).abs() < 1e-5, "{}", a.phase);
    }

    #[test]
    fn a_loop_counts_the_cycles_its_sampled_time_has_wrapped() {
        let mut a = animator();
        let c = clip(&[("A", [1.0, 0.0, 0.0], 55)], 2.67);
        let i = a.want(&c, 0, 1.0, Mode::Loop, 0.0);
        // Near a loop, where a quotient rounded on its own can count a cycle early: the
        // count and the time sampled always add back up to the layer's time.
        for t in [0.0, 1.0, 2.669, 2.67, 45.39, 45.4, 1000.0, -0.5, -2.67] {
            a.layers[i].time = t;
            let (n, within) = (a.layers[i].cycles(), a.layers[i].sample_time(0.0));
            assert!((n as f32 * 2.67 + within - t).abs() < 1e-3, "time {t}: {n} cycles and {within}");
        }
        // Once and Synced layers count none.
        let j = a.restart(&c, 1, 1.0, Mode::Once, 0.0);
        a.layers[j].time = 10.0;
        assert_eq!(a.layers[j].cycles(), 0);
    }

    #[test]
    fn once_holds_its_last_pose_and_restart_rewinds() {
        let mut a = animator();
        let c = clip(&[("A", [1.0, 0.0, 0.0], 55)], 0.4);
        let i = a.restart(&c, 7, 1.0, Mode::Once, 0.0);
        a.advance(0.5, 1.0);
        assert!(a.layers[i].finished());
        assert_eq!(a.layers[i].sample_time(0.0), 0.4);
        let i = a.restart(&c, 7, 1.0, Mode::Once, 0.0);
        assert_eq!((a.layers.len(), a.layers[i].time), (1, 0.0));
        a.cut(7);
        assert!(a.layers.is_empty());
    }

    #[test]
    fn model_space_follows_the_pose() {
        let mut a = animator();
        a.pose();
        // Bip01 held at the identity: B is at A (1, 0, 0) + (0, 2, 0).
        let m = a.model_space(3);
        assert!((m.translation - Vec3A::new(1.0, 2.0, 0.0)).length() < 1e-5);
    }
}
