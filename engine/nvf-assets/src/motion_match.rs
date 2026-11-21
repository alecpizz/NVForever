//! Motion matching: the locomotion pose picked each moment from every frame of the
//! character's locomotion clips by how well it carries on from the pose shown and how
//! well it follows where the controls are taking the character (docs/design/
//! motion-matching.md) [documented: the method is Simon Clavet's, "Motion Matching and
//! The Road to Next-Gen Animation", GDC 2016; the features, their normalisation and the
//! spring that predicts the trajectory follow Daniel Holden, "Learned Motion Matching",
//! SIGGRAPH 2020 §3, and his articles at theorangeduck.com]. Not a behaviour of the
//! original game: an NVForever feature behind a setting.
//!
//! A [`Motion`] is what the animator can play as one: a looping clip, or two or more
//! loops played on one shared phase at fixed weights (a diagonal: the forward and the
//! sideways clips). The [`Database`] holds one row per 1/[`ROW_HZ`] s of every motion,
//! each with [`FEATURES`] numbers taken from the pose the animator itself gives there
//! (a scratch [`Animator`], so the rows are what will be seen); [`Matcher`] holds the
//! motion and phase playing and decides when to jump.
//!
//! Frame: the character's ground frame, game units: +x right, +y forward, +z up (the
//! actor's axes, actors.md §8.2); yaw grows anticlockwise seen from above (a turn to the
//! left).

use crate::actor::PreparedSkeleton;
use crate::anim_blend::{Animator, Mode};
use crate::anim_play::Clip;
use bevy::prelude::*;
use std::ops::Range;
use std::sync::Arc;

/// Numbers per row: both feet's places (6) and velocities (6), the hips' velocity (3),
/// the trajectory's places at the [`HORIZONS`] (6) and its facings there (6).
pub const FEATURES: usize = 27;
/// The trajectory is looked at this far ahead, seconds [documented: Clavet 2016 and
/// Holden 2020 use 1/3, 2/3 and 1 s at 60 Hz].
pub const HORIZONS: [f32; 3] = [1.0 / 3.0, 2.0 / 3.0, 1.0];
/// The groups of features, each normalised as one (its spread over the database) and
/// weighted as one: feet places, feet velocities, hip velocity, trajectory places,
/// trajectory facings.
const GROUPS: [Range<usize>; 5] = [0..6, 6..12, 12..15, 15..21, 21..27];
/// Where the pose's features end and the trajectory's begin.
pub const POSE_FEATURES: usize = 15;
/// Rows per second of motion.
pub const ROW_HZ: f32 = 30.0;
/// The pose is sampled twice as finely as the rows, for the velocities' differences.
const SAMPLE_HZ: f32 = 2.0 * ROW_HZ;
/// The smallest spread a group is divided by (its units): a group that hardly varies
/// over the database (the facings, when only the turn clips turn) is not blown up into
/// the one that decides everything.
///
/// The facings' is a fixed half a unit (about 30° at the horizon) whatever their spread:
/// with few turning motions in a database their spread is tiny, and a turn of the view
/// while running weighed more than where the character goes [observed: the first smoke
/// run's costs, 2026-11-20].
const MIN_SPREAD: [f32; 5] = [1.0, 5.0, 5.0, 5.0, 0.5];
const FIXED_SPREAD: [bool; 5] = [false, false, false, false, true];

/// The bones the features are read from.
pub const LEFT_FOOT: &str = "Bip01 L Foot";
pub const RIGHT_FOOT: &str = "Bip01 R Foot";
pub const HIPS: &str = "Bip01 Pelvis";

/// How much each group of features counts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub feet_position: f32,
    pub feet_velocity: f32,
    pub hip_velocity: f32,
    pub trajectory_position: f32,
    pub trajectory_facing: f32,
}

impl Default for Weights {
    /// [design: a start, tuned by eye; the pose's groups keep the motion continuous, the
    /// trajectory's make it answer the controls]
    fn default() -> Self {
        // The trajectory at twice the pose's: with loops only (no clip that stops or starts)
        // the pose of carrying on always costs nothing, and at the trajectory's weight of 1
        // a walk went on in place after the keys were let go [observed: the first smoke
        // run's costs, 2026-11-20].
        Weights { feet_position: 0.75, feet_velocity: 1.0, hip_velocity: 1.0, trajectory_position: 2.0, trajectory_facing: 1.5 }
    }
}

impl Weights {
    fn of_group(&self, g: usize) -> f32 {
        [self.feet_position, self.feet_velocity, self.hip_velocity, self.trajectory_position, self.trajectory_facing][g]
    }

    /// From `feet_position,feet_velocity,hip_velocity,trajectory_position,trajectory_facing`
    /// (a test aid's text; `None` unless all five are numbers).
    pub fn parse(text: &str) -> Option<Weights> {
        let v: Vec<f32> = text.split(',').map(|s| s.trim().parse().ok()).collect::<Option<_>>()?;
        let [a, b, c, d, e] = v.as_slice() else { return None };
        Some(Weights { feet_position: *a, feet_velocity: *b, hip_velocity: *c, trajectory_position: *d, trajectory_facing: *e })
    }
}

/// What a motion is, for the player's body to play it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Standing: the idle alone (it plays under everything, as without matching).
    Idle,
    /// Walking or running: one clip or a blend of a direction pair.
    Move,
    /// Turning on the spot.
    Turn,
    /// A clip from the motion matching folder (a start, a stop, a pivot), played as a
    /// move is.
    Extra,
}

/// One clip of a motion.
#[derive(Clone)]
pub struct MotionClip {
    pub clip: Arc<Clip>,
    pub weight: f32,
    /// Added to the shared phase for this clip (its footfalls lined up with the first
    /// clip's; `anim_blend::Layer::phase_offset`).
    pub offset: f32,
}

/// Clips played as one on a shared phase.
#[derive(Clone)]
pub struct Motion {
    pub kind: Kind,
    /// For the log: `run forward 0.75 + right 0.25`.
    pub label: String,
    pub clips: Vec<MotionClip>,
    /// Wraps round at its end (every original locomotion clip does); one that does not
    /// is only matched where a second of it is left, and is left before its end.
    pub looping: bool,
    /// A turn the motion stands for that its root does not make, radians/s
    /// (anticlockwise): the turning clips step on the spot while the controls turn the
    /// character [observed: `mtturnleft.kf` and `mtturnright.kf` turn `Bip01` by 0°].
    pub implied_turn: f32,
    /// The root's travel scaled to what the planted feet say ([`Database::build`] sets
    /// it; 1 as made): the original clips' root travel is not their feet's [observed:
    /// the foot skate trace, 2026-11-20: played at the root's speed, the planted feet of
    /// the original's run slid 80-100 units/s and of its strafes about 240].
    pub travel_scale: f32,
}

impl Motion {
    /// Seconds a cycle takes at rate 1, as the animator moves a synced phase (the clips'
    /// lengths by their weights; `Animator::advance`), or the idle's own.
    pub fn length(&self) -> f32 {
        let (mut len, mut w) = (0.0, 0.0);
        for c in &self.clips {
            len += c.weight * c.clip.length() / c.clip.frequency.max(1e-6);
            w += c.weight;
        }
        if w > 1e-6 {
            len / w
        } else {
            0.0
        }
    }

    /// The accumulation root's ground place and yaw at `u` cycles into the motion (any
    /// real `u`: a looping motion carries on cycle after cycle; one that does not stops
    /// at its ends). The clips' by their weights.
    pub fn root_at(&self, u: f32) -> (Vec2, f32) {
        let (mut p, mut yaw, mut w) = (Vec2::ZERO, 0.0, 0.0);
        for c in &self.clips {
            let (q, y) = clip_root(&c.clip, u + c.offset, self.looping);
            p += c.weight * q;
            yaw += c.weight * y;
            w += c.weight;
        }
        let implied = self.implied_turn * u * self.length();
        if w > 1e-6 {
            (p / w * self.travel_scale, yaw / w + implied)
        } else {
            (Vec2::ZERO, implied)
        }
    }
}

/// `v` turned anticlockwise by `a` radians.
fn turn(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// Rotation about z of a unit quaternion (w, x, y, z), anticlockwise.
fn yaw_of([w, x, y, z]: [f32; 4]) -> f32 {
    (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z))
}

/// `a` wrapped to (−π, π].
fn wrap(a: f32) -> f32 {
    let r = a.rem_euclid(std::f32::consts::TAU);
    if r > std::f32::consts::PI {
        r - std::f32::consts::TAU
    } else {
        r
    }
}

/// The clip's accumulation root on the ground and its yaw at `u` cycles in, relative to
/// where it starts, a whole cycle's travel and turn carried on per cycle (looping) or the
/// ends held. No root track: no motion.
fn clip_root(clip: &Clip, u: f32, looping: bool) -> (Vec2, f32) {
    let Some(track) = clip.accum_track() else { return (Vec2::ZERO, 0.0) };
    let len = clip.length();
    let at = |t: f32| {
        let p = track.translation.sample(clip.start + t).map_or(Vec2::ZERO, |v| Vec2::new(v[0], v[1]));
        let y = track.rotation_at(clip.start + t).map_or(0.0, yaw_of);
        (p, y)
    };
    let (p0, y0) = at(0.0);
    let local = |t: f32| {
        let (p, y) = at(t);
        // Places relative to the start, in the start's frame.
        (turn(p - p0, -y0), wrap(y - y0))
    };
    if !looping || len <= 0.0 {
        return local(u.clamp(0.0, 1.0) * len);
    }
    let k = u.floor();
    let (dp, dy) = local(len);
    let (mut p, mut y) = (Vec2::ZERO, 0.0);
    let k = k as i32;
    if k > 0 {
        for _ in 0..k.min(64) {
            p += turn(dp, y);
            y += dy;
        }
    } else {
        for _ in 0..(-k).min(64) {
            y -= dy;
            p -= turn(dp, y);
        }
    }
    let (q, z) = local((u - k as f32) * len);
    (p + turn(q, y), y + z)
}

/// The bone that mirrors `name` across the body's middle: `Bip01 L Foot` and `Bip01 R
/// Foot`, `Bip01 LPauldron` and `Bip01 RPauldron` [observed: the bone names of
/// `_male\skeleton.nif`]; `None` for a bone on the middle (or without a partner).
pub fn mirror_name(name: &str) -> Option<String> {
    let rest = name.strip_prefix("Bip01 ")?;
    let swapped = match rest.strip_prefix('L') {
        Some(r) => format!("Bip01 R{r}"),
        None => format!("Bip01 L{}", rest.strip_prefix('R')?),
    };
    // `Bip01 L Foot` (a space after the side) or `Bip01 LPauldron` (a capital after it).
    let after = rest[1..].chars().next();
    after.is_some_and(|c| c == ' ' || c.is_ascii_uppercase()).then_some(swapped)
}

/// Rows of a clip baked by [`mirror_clip`], a second.
const MIRROR_HZ: f32 = 30.0;

/// `clip` mirrored left for right across the body's middle (x → −x in the actor's frame),
/// baked into linear keys at [`MIRROR_HZ`] on skeleton `skel`: a sidestep to the left
/// from one to the right, a walk leading with the other foot. Each bone takes its
/// partner's pose reflected in model space, put back into its own axes by the bind pose
/// (the left and right bones' axes need not mirror each other), so the bind pose mirrors
/// to itself. Text keys `Left` and `Right` swap. Every bone the clip or its mirror drives
/// gets a track; tracks on nodes outside the skeleton are left out.
pub fn mirror_clip(skel: &PreparedSkeleton, clip: &Clip) -> Clip {
    use bevy::math::{Affine3A, Mat3};
    use fnv_nif::anim::{Channel, Key, KeyKind, Keys, TransformTrack};
    let n = skel.locals.len();
    let partner: Vec<usize> = (0..n)
        .map(|i| skel.names[i].as_deref().and_then(mirror_name).and_then(|m| skel.by_name.get(&m).copied()).unwrap_or(i))
        .collect();
    let track_of: Vec<Option<usize>> = (0..n).map(|i| skel.names[i].as_deref().and_then(|nm| clip.tracks.iter().position(|(t, _)| t == nm))).collect();
    let driven: Vec<bool> = (0..n).map(|i| i > 0 && (track_of[i].is_some() || track_of[partner[i]].is_some())).collect();
    // The accumulation root: held at the identity in the poses, as the animator holds it
    // (`Animator::new`), its track (the clip's travel) mirrored by itself. Its bind turns a
    // quarter about z (`_male\skeleton.nif`'s `Bip01`), which the clips carry on the node
    // below it instead [observed 2026-11-20: mirrored with it, a walk forward went back].
    let accum = clip.accum_root.as_deref().and_then(|r| skel.by_name.get(r).copied()).filter(|&a| a > 0);
    let affine = |t: &Transform| Affine3A::from_scale_rotation_translation(t.scale, t.rotation, t.translation);
    let model = |locals: &[Transform]| {
        let mut m = vec![Affine3A::IDENTITY; n];
        for i in 1..n {
            let parent = skel.parents[i].map_or(Affine3A::IDENTITY, |p| m[p]);
            m[i] = parent * affine(&locals[i]);
        }
        m
    };
    let s = Affine3A::from_mat3(Mat3::from_diagonal(Vec3::new(-1.0, 1.0, 1.0)));
    let bind = model(&skel.locals);
    // Per bone: what takes its partner's reflected bind back to its own bind.
    let fix: Vec<Affine3A> = (0..n).map(|i| (s * bind[partner[i]] * s).inverse() * bind[i]).collect();
    let len = clip.length();
    let steps = ((len * MIRROR_HZ).round() as usize).max(1);
    let mut keys_t: Vec<Vec<Key<3>>> = vec![Vec::new(); n];
    let mut keys_r: Vec<Vec<Key<4>>> = vec![Vec::new(); n];
    let mut keys_s: Vec<Vec<Key<1>>> = vec![Vec::new(); n];
    fn key<const N: usize>(time: f32, value: [f32; N]) -> Key<N> {
        Key { time, value, forward: [0.0; N], backward: [0.0; N], tbc: [0.0; 3] }
    }
    for k in 0..=steps {
        let time = clip.start + len * k as f32 / steps as f32;
        let locals: Vec<Transform> = (0..n)
            .map(|i| {
                let bind = skel.locals[i];
                let Some(t) = track_of[i].map(|j| &clip.tracks[j].1) else { return bind };
                Transform {
                    translation: t.translation.sample(time).map_or(bind.translation, Vec3::from_array),
                    rotation: t.rotation_at(time).map_or(bind.rotation, |[w, x, y, z]| Quat::from_xyzw(x, y, z, w).normalize()),
                    scale: Vec3::splat(t.scale.sample(time).map_or(bind.scale.x, |[v]| v)),
                }
            })
            .collect();
        let mut held = locals.clone();
        if let Some(a) = accum {
            held[a] = Transform::IDENTITY;
        }
        let m = model(&held);
        let mirrored: Vec<Affine3A> = (0..n).map(|i| if Some(i) == accum { Affine3A::IDENTITY } else { s * m[partner[i]] * s * fix[i] }).collect();
        for i in (1..n).filter(|&i| driven[i]) {
            let parent = skel.parents[i].map_or(Affine3A::IDENTITY, |p| mirrored[p]);
            let (scale, mut rot, tr) = if Some(i) == accum {
                // Its travel to the other side, its turn the other way.
                let l = locals[i];
                (l.scale, Quat::from_xyzw(l.rotation.x, -l.rotation.y, -l.rotation.z, l.rotation.w), Vec3::new(-l.translation.x, l.translation.y, l.translation.z))
            } else {
                (parent.inverse() * mirrored[i]).to_scale_rotation_translation()
            };
            if let Some(prev) = keys_r[i].last() {
                let [w, x, y, z] = prev.value;
                if Quat::from_xyzw(x, y, z, w).dot(rot) < 0.0 {
                    rot = -rot;
                }
            }
            keys_t[i].push(key(time, tr.to_array()));
            keys_r[i].push(key(time, [rot.w, rot.x, rot.y, rot.z]));
            keys_s[i].push(key(time, [scale.x]));
        }
    }
    let mut tracks = Vec::new();
    let mut priorities = Vec::new();
    for i in (1..n).filter(|&i| driven[i]) {
        let Some(name) = skel.names[i].clone() else { continue };
        let source = track_of[partner[i]].or(track_of[i]).unwrap_or(0);
        let track = TransformTrack {
            translation: Channel::Keys(Keys { kind: KeyKind::Linear, keys: std::mem::take(&mut keys_t[i]) }),
            rotation: Channel::Keys(Keys { kind: KeyKind::Linear, keys: std::mem::take(&mut keys_r[i]) }),
            euler: None,
            scale: Channel::Keys(Keys { kind: KeyKind::Linear, keys: std::mem::take(&mut keys_s[i]) }),
            per_cycle: None,
        };
        tracks.push((name, Arc::new(track)));
        priorities.push(clip.priorities.get(source).copied().unwrap_or(0));
    }
    let swap = |t: &str| {
        let lower = t.to_ascii_lowercase();
        if lower.contains("left") {
            t.replace("Left", "Right").replace("left", "right")
        } else if lower.contains("right") {
            t.replace("Right", "Left").replace("right", "left")
        } else {
            t.to_string()
        }
    };
    Clip {
        name: format!("{} (mirrored)", clip.name),
        start: clip.start,
        stop: clip.stop,
        frequency: clip.frequency,
        cycle_type: clip.cycle_type,
        tracks,
        priorities,
        skipped: 0,
        accum_root: clip.accum_root.clone(),
        text_keys: clip.text_keys.iter().map(|(t, k)| (*t, swap(k))).collect(),
        visibility: Vec::new(),
        emitters: Vec::new(),
        materials: Vec::new(),
        free: Vec::new(),
        free_visibility: Vec::new(),
        lights: Vec::new(),
    }
}

/// A foot this near its lowest over a motion is on the ground (units) [design].
const CONTACT_LIFT: f32 = 2.0;

/// The median speed (units/s, over the ground plane of the model) of the feet while they
/// are on the ground, from the feature bones' places sampled `dt` apart; `None` with too
/// few such samples.
fn contact_speed(poses: &[[Vec3; 3]], dt: f32, looping: bool) -> Option<f32> {
    let n = poses.len();
    if n < 3 || dt <= 0.0 {
        return None;
    }
    let mut speeds = Vec::new();
    for foot in 0..2 {
        let low = poses.iter().map(|p| p[foot].z).fold(f32::INFINITY, f32::min);
        for k in 0..n {
            if poses[k][foot].z > low + CONTACT_LIFT {
                continue;
            }
            let (a, b, span) = if looping {
                ((k + n - 1) % n, (k + 1) % n, 2.0 * dt)
            } else if k == 0 || k == n - 1 {
                continue;
            } else {
                (k - 1, k + 1, 2.0 * dt)
            };
            let d = poses[b][foot] - poses[a][foot];
            speeds.push(Vec2::new(d.x, d.y).length() / span);
        }
    }
    if speeds.len() < 3 {
        return None;
    }
    speeds.sort_by(f32::total_cmp);
    Some(speeds[speeds.len() / 2])
}

/// One row: a motion and the phase (0–1) within it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    pub motion: u16,
    pub phase: f32,
}

/// A motion as the database holds it.
#[derive(Clone)]
pub struct MotionInfo {
    pub motion: Motion,
    /// Seconds per cycle at rate 1 ([`Motion::length`]).
    pub length: f32,
    /// Ground speed over a cycle, units/s, and the turn, radians/s (anticlockwise).
    pub speed: f32,
    pub turn: f32,
    /// Its rows in [`Database::rows`], evenly spaced in phase from 0.
    pub rows: Range<usize>,
}

/// Every row of a set of motions, with their features.
pub struct Database {
    pub motions: Vec<MotionInfo>,
    pub rows: Vec<Row>,
    raw: Vec<[f32; FEATURES]>,
    /// Each row's root velocity (in its velocities: the feet's and the hips' are over the
    /// ground), so a query can put the character's own in its place ([`Self::query`]).
    root_v: Vec<Vec2>,
    norm: Vec<[f32; FEATURES]>,
    mean: [f32; FEATURES],
    /// Per feature: the group's weight over its spread.
    scale: [f32; FEATURES],
    pub weights: Weights,
}

/// The idle under every motion, played by its own time ([`Kind::Idle`]'s phase is the
/// idle's).
pub const IDLE_TAG: u32 = 0;
/// Tags of a motion's clips in the scratch animator (and in the player's body: the
/// movement tags, `player_body.rs`).
pub const MOTION_TAG: u32 = 10;

/// Play `m` at `u` cycles on the scratch animator `a` (its layers set up for `m` by
/// [`set_up`]) and give the feature bones' places in the model.
fn pose_at(a: &mut Animator, m: &Motion, idle: bool, u: f32, bones: [usize; 3]) -> [Vec3; 3] {
    let idle_layer = idle.then(|| a.layers.iter().position(|l| l.tag == IDLE_TAG)).flatten();
    if m.kind == Kind::Idle {
        if let Some(i) = idle_layer {
            let len = a.layers[i].clip.length();
            a.layers[i].time = u.rem_euclid(1.0) * len;
        }
    } else {
        a.phase = u.rem_euclid(1.0);
        if !m.looping {
            // A clip that does not loop: its end, not its start again.
            a.phase = u.clamp(0.0, 0.999_99);
        }
        if let Some(i) = idle_layer {
            a.layers[i].time = 0.0;
        }
    }
    a.pose();
    bones.map(|b| Vec3::from(a.model_space(b).translation))
}

/// The scratch animator's layers for `m`: the idle under it (when there is one), and its
/// clips on the shared phase, all at once (no fades).
fn set_up(a: &mut Animator, m: &Motion, idle: Option<&Arc<Clip>>) {
    a.layers.clear();
    if let Some(c) = idle {
        a.want(c, IDLE_TAG, 1.0, Mode::Loop, 0.0);
    }
    if m.kind != Kind::Idle {
        for (j, c) in m.clips.iter().enumerate() {
            let i = a.want(&c.clip, MOTION_TAG + j as u32, c.weight, Mode::Synced, 0.0);
            a.layers[i].phase_offset = c.offset;
        }
    }
}

/// The trajectory features of motion `m` at `u` cycles: the root's places at the
/// [`HORIZONS`] and its facings there, in the frame of its place and yaw at `u`.
fn trajectory_of(m: &Motion, length: f32, u: f32) -> [f32; 12] {
    let mut out = [0.0; 12];
    let (p0, y0) = m.root_at(u);
    for (h, secs) in HORIZONS.iter().enumerate() {
        let (p, y) = m.root_at(u + secs / length.max(1e-3));
        let d = turn(p - p0, -y0);
        let f = turn(Vec2::Y, y - y0);
        out[2 * h..2 * h + 2].copy_from_slice(&[d.x, d.y]);
        out[6 + 2 * h..6 + 2 * h + 2].copy_from_slice(&[f.x, f.y]);
    }
    out
}

/// The root's velocity at `u` cycles in its own frame there, units/s.
fn root_velocity(m: &Motion, length: f32, u: f32) -> Vec2 {
    let du = 0.5 / SAMPLE_HZ / length.max(1e-3);
    let (a, _) = m.root_at(u - du);
    let (b, _) = m.root_at(u + du);
    let (_, y) = m.root_at(u);
    let span = if m.looping { 2.0 * du } else { (u + du).min(1.0) - (u - du).max(0.0) };
    if span <= 0.0 {
        return Vec2::ZERO;
    }
    turn(b - a, -y) / (span * length.max(1e-3))
}

impl Database {
    /// Rows for `motions` on skeleton `skel` (its accumulation root `accum_root` held, as
    /// the player's animator holds it), `idle` under every motion. Motions without
    /// length are left out; `None` when the skeleton lacks a feature bone.
    pub fn build(skel: Arc<PreparedSkeleton>, accum_root: Option<&str>, idle: Option<Arc<Clip>>, motions: Vec<Motion>, weights: Weights) -> Option<Database> {
        let bone = |n: &str| skel.by_name.get(n).copied();
        let bones = [bone(LEFT_FOOT)?, bone(RIGHT_FOOT)?, bone(HIPS)?];
        let mut a = Animator::new(skel.clone(), Vec::new(), accum_root);
        let mut infos = Vec::new();
        let mut rows = Vec::new();
        let mut raw = Vec::new();
        let mut root_vs = Vec::new();
        for mut m in motions {
            let length = m.length();
            if length <= 1e-3 || m.clips.is_empty() {
                continue;
            }
            let looping = m.looping;
            set_up(&mut a, &m, idle.as_ref());
            let n = ((length * SAMPLE_HZ).round() as usize).max(4);
            // Samples at u = k / n (a looping motion's last is its first again).
            let count = if looping { n } else { n + 1 };
            let poses: Vec<[Vec3; 3]> = (0..count).map(|k| pose_at(&mut a, &m, idle.is_some(), k as f32 / n as f32, bones)).collect();
            let dt = length / n as f32;
            // The planted feet's speed: each foot within `CONTACT_LIFT` of its lowest moves
            // back under the body at the ground speed the clip was made for; the root's
            // travel is scaled to it.
            if m.kind != Kind::Idle && m.implied_turn == 0.0 {
                let (root, _) = m.root_at(1.0);
                let root_speed = (root - m.root_at(0.0).0).length() / length;
                if let Some(feet) = contact_speed(&poses, dt, looping).filter(|_| root_speed > 1.0) {
                    m.travel_scale = (feet / root_speed).clamp(0.3, 3.0);
                }
            }
            let first = rows.len();
            let step = 2;
            let mut k = 0;
            while k < count {
                let u = k as f32 / n as f32;
                // A clip that does not loop is matched only where it has a second left.
                if !looping && (1.0 - u) * length < HORIZONS[2] - 1e-3 && k > 0 {
                    break;
                }
                let (prev, next, span) = if looping {
                    ((k + count - 1) % count, (k + 1) % count, 2.0 * dt)
                } else {
                    let (p, q) = (k.saturating_sub(1), (k + 1).min(count - 1));
                    (p, q, (q - p) as f32 * dt)
                };
                let root_v2 = root_velocity(&m, length, u);
                let root_v = root_v2.extend(0.0);
                let vel = |b: usize| if span > 0.0 { (poses[next][b] - poses[prev][b]) / span + root_v } else { root_v };
                let mut f = [0.0; FEATURES];
                let (l, r, lv, rv, hv) = (poses[k][0], poses[k][1], vel(0), vel(1), vel(2));
                f[0..3].copy_from_slice(&l.to_array());
                f[3..6].copy_from_slice(&r.to_array());
                f[6..9].copy_from_slice(&lv.to_array());
                f[9..12].copy_from_slice(&rv.to_array());
                f[12..15].copy_from_slice(&hv.to_array());
                f[POSE_FEATURES..].copy_from_slice(&trajectory_of(&m, length, u));
                rows.push(Row { motion: infos.len() as u16, phase: u });
                raw.push(f);
                root_vs.push(root_v2);
                k += step;
            }
            let (p0, y0) = m.root_at(0.0);
            let (p1, y1) = m.root_at(1.0);
            let (speed, turn) = ((p1 - p0).length() / length, (y1 - y0) / length);
            infos.push(MotionInfo { motion: m, length, speed, turn, rows: first..rows.len() });
        }
        if rows.is_empty() {
            return None;
        }
        let mut db = Database { motions: infos, rows, raw, root_v: root_vs, norm: Vec::new(), mean: [0.0; FEATURES], scale: [1.0; FEATURES], weights };
        db.normalise();
        Some(db)
    }

    /// Each feature less its mean over the rows, over its group's spread, times the
    /// group's weight [documented: Holden 2020 §3.1].
    fn normalise(&mut self) {
        let n = self.raw.len().max(1) as f32;
        let mut mean = [0.0; FEATURES];
        for f in &self.raw {
            for (m, x) in mean.iter_mut().zip(f) {
                *m += x / n;
            }
        }
        let mut scale = [1.0; FEATURES];
        for (g, range) in GROUPS.iter().enumerate() {
            let mut var = 0.0;
            for f in &self.raw {
                for d in range.clone() {
                    var += (f[d] - mean[d]).powi(2);
                }
            }
            let spread = if FIXED_SPREAD[g] { MIN_SPREAD[g] } else { (var / (n * range.len() as f32)).sqrt().max(MIN_SPREAD[g]) };
            for d in range.clone() {
                scale[d] = self.weights.of_group(g) / spread;
            }
        }
        self.mean = mean;
        self.scale = scale;
        self.norm = self.raw.iter().map(|f| self.normalised(f)).collect();
    }

    /// Weigh the groups anew.
    pub fn set_weights(&mut self, w: Weights) {
        if w != self.weights {
            self.weights = w;
            self.normalise();
        }
    }

    fn normalised(&self, f: &[f32; FEATURES]) -> [f32; FEATURES] {
        let mut out = [0.0; FEATURES];
        for d in 0..FEATURES {
            out[d] = (f[d] - self.mean[d]) * self.scale[d];
        }
        out
    }

    /// The query for the pose playing, `motion` at `phase`, with the character moving at
    /// `velocity` over the ground (its frame, units/s) and the controls' `trajectory`:
    /// the velocities of the feet and the hips are the clips' about the root plus the
    /// character's own, not the clips' root's. The character's speed is the controller's
    /// (actors.md §8.2), so a change of direction shows in the pose as the body really
    /// takes it, and the rows of the new direction come within reach as it does.
    pub fn query(&self, motion: u16, phase: f32, velocity: Vec2, trajectory: &[f32; 12]) -> Option<[f32; FEATURES]> {
        let mut q = self.features_at(motion, phase)?;
        let (i, j, t) = self.between(motion, phase)?;
        let rv = self.root_v[i].lerp(self.root_v[j], t);
        for d in [6, 9, 12] {
            q[d] += velocity.x - rv.x;
            q[d + 1] += velocity.y - rv.y;
        }
        q[POSE_FEATURES..].copy_from_slice(trajectory);
        Some(q)
    }

    /// The root's ground speed of `motion` at `phase`, units/s (between its rows).
    pub fn root_speed_at(&self, motion: u16, phase: f32) -> Option<f32> {
        let (i, j, t) = self.between(motion, phase)?;
        Some(self.root_v[i].lerp(self.root_v[j], t).length())
    }

    /// The two rows `motion` at `phase` falls between and how far it is from the first.
    fn between(&self, motion: u16, phase: f32) -> Option<(usize, usize, f32)> {
        let info = self.motions.get(motion as usize)?;
        let rows = info.rows.clone();
        let count = rows.len();
        if count == 0 {
            return None;
        }
        let step = if count > 1 { self.rows[rows.start + 1].phase - self.rows[rows.start].phase } else { 1.0 };
        let x = if info.motion.looping { phase.rem_euclid(1.0) } else { phase.clamp(0.0, 1.0) } / step.max(1e-6);
        let i = (x.floor() as usize).min(count - 1);
        let t = (x - i as f32).clamp(0.0, 1.0);
        let j = if info.motion.looping { (i + 1) % count } else { (i + 1).min(count - 1) };
        Some((rows.start + i, rows.start + j, t))
    }

    /// The raw features of `motion` at `phase`, between its two nearest rows (wrapping
    /// round a looping motion).
    pub fn features_at(&self, motion: u16, phase: f32) -> Option<[f32; FEATURES]> {
        let (i, j, t) = self.between(motion, phase)?;
        let (a, b) = (&self.raw[i], &self.raw[j]);
        let mut out = [0.0; FEATURES];
        for d in 0..FEATURES {
            out[d] = a[d] + (b[d] - a[d]) * t;
        }
        Some(out)
    }

    /// The cost of row `r` against the normalised query `q` (`pose`: the pose's features
    /// count too, else only the trajectory's).
    fn cost_of(&self, r: usize, q: &[f32; FEATURES], pose: bool) -> f32 {
        let from = if pose { 0 } else { POSE_FEATURES };
        self.norm[r][from..].iter().zip(&q[from..]).map(|(a, b)| (a - b) * (a - b)).sum()
    }

    /// The cost of raw features `f` against the raw query `q`, group by group (feet
    /// places, feet velocities, hip velocity, trajectory places, facings), for the log.
    pub fn group_costs(&self, f: &[f32; FEATURES], q: &[f32; FEATURES]) -> [f32; 5] {
        let (a, b) = (self.normalised(f), self.normalised(q));
        let mut out = [0.0; 5];
        for (g, range) in GROUPS.iter().enumerate() {
            out[g] = range.clone().map(|d| (a[d] - b[d]) * (a[d] - b[d])).sum();
        }
        out
    }

    /// The raw features of row `r`.
    pub fn row_features(&self, r: usize) -> Option<&[f32; FEATURES]> {
        self.raw.get(r)
    }

    /// The cost of raw features `f` against the raw query `q`.
    pub fn cost(&self, f: &[f32; FEATURES], q: &[f32; FEATURES]) -> f32 {
        let (a, b) = (self.normalised(f), self.normalised(q));
        a.iter().zip(&b).map(|(x, y)| (x - y) * (x - y)).sum()
    }

    /// The row nearest the raw query `q` and its cost, leaving out the rows `skip` says
    /// to and those heading away from where the query goes ([`heads_away`]) (by brute
    /// force: a set's few thousand rows take microseconds).
    pub fn search(&self, q: &[f32; FEATURES], pose: bool, skip: impl Fn(&Row) -> bool) -> Option<(usize, f32)> {
        let raw_q = *q;
        let q = self.normalised(q);
        let mut best: Option<(usize, f32)> = None;
        for (i, row) in self.rows.iter().enumerate() {
            if skip(row) || heads_away(&self.raw[i], &raw_q) {
                continue;
            }
            let c = self.cost_of(i, &q, pose);
            if best.is_none_or(|(_, b)| c < b) {
                best = Some((i, c));
            }
        }
        best
    }
}

/// A row heads away from a query when both go somewhere over the trajectory's second
/// (more than [`GATE_TRAVEL`] units) and their ways part by more than [`GATE_ANGLE`]: the
/// pose alone (from standing, every running pose is far) took a backward run for a start
/// forward [observed: the motion matching log, 2026-11-20; design: shipped matchers filter
/// candidates so, Clavet 2016].
pub fn heads_away(row: &[f32; FEATURES], q: &[f32; FEATURES]) -> bool {
    let end = POSE_FEATURES + 4;
    let (a, b) = (Vec2::new(row[end], row[end + 1]), Vec2::new(q[end], q[end + 1]));
    a.length() > GATE_TRAVEL && b.length() > GATE_TRAVEL && a.angle_to(b).abs() > GATE_ANGLE
}

/// Units of travel over the trajectory below which a row's or a query's way is not
/// judged, and the widest parting of ways a row may have from the query [design].
pub const GATE_TRAVEL: f32 = 40.0;
pub const GATE_ANGLE: f32 = 50.0 * std::f32::consts::PI / 180.0;

/// The trajectory the controls ask for, as features in the character's frame now: the
/// ground velocity eases from `velocity` to `goal` (both in the character's frame,
/// units/s) on a critically damped spring of half-life `halflife`, while the character
/// turns at `turn` radians/s (anticlockwise) easing off with half-life `turn_halflife`
/// [documented: Holden, "Spring-It-On: The Game Developer's Spring-Roll-Call", the
/// critically damped spring's closed form].
pub fn predict(velocity: Vec2, goal: Vec2, halflife: f32, turn_rate: f32, turn_halflife: f32) -> [f32; 12] {
    const STEPS: usize = 60;
    let y = 2.0 * std::f32::consts::LN_2 / halflife.max(1e-3);
    let k = std::f32::consts::LN_2 / turn_halflife.max(1e-3);
    let j0 = velocity - goal;
    let j1 = j0 * y;
    let vel = |t: f32| goal + (j0 + j1 * t) * (-y * t).exp();
    let yaw = |t: f32| turn_rate * (1.0 - (-k * t).exp()) / k;
    let mut out = [0.0; 12];
    let mut p = Vec2::ZERO;
    let mut t = 0.0f32;
    let mut h = 0;
    let end = HORIZONS[2];
    let dt = end / STEPS as f32;
    for _ in 0..STEPS {
        let mid = t + 0.5 * dt;
        p += turn(vel(mid), yaw(mid)) * dt;
        t += dt;
        while h < HORIZONS.len() && t >= HORIZONS[h] - 1e-4 {
            let f = turn(Vec2::Y, yaw(t));
            out[2 * h..2 * h + 2].copy_from_slice(&[p.x, p.y]);
            out[6 + 2 * h..6 + 2 * h + 2].copy_from_slice(&[f.x, f.y]);
            h += 1;
        }
    }
    out
}

/// The velocity `t` seconds on along [`predict`]'s spring.
pub fn spring_velocity(velocity: Vec2, goal: Vec2, halflife: f32, t: f32) -> Vec2 {
    let y = 2.0 * std::f32::consts::LN_2 / halflife.max(1e-3);
    let j0 = velocity - goal;
    goal + (j0 + j0 * y * t) * (-y * t).exp()
}

/// Seconds between searches while the controls hold still [documented: Clavet 2016
/// searches every few frames; design: 0.1 s].
pub const SEARCH_INTERVAL: f32 = 0.1;
/// A row this close in time to the one playing, in the same motion, is not a jump worth
/// making (it would only stutter) [design].
pub const SAME_WINDOW: f32 = 0.25;
/// A row must be cheaper than carrying on by this much to jump to it, and by this share
/// of carrying on's cost: near equals do not flicker back and forth [design].
pub const JUMP_MARGIN: f32 = 0.05;
pub const JUMP_SHARE: f32 = 0.1;
/// A motion that does not loop is left this long before its end [design].
const END_LEAVE: f32 = 0.15;

/// What the matcher chose for the frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Choice {
    pub motion: u16,
    pub phase: f32,
    /// A new motion or a new place in it: the animator jumps (inertialized).
    pub jumped: bool,
    /// A search ran this frame: the cost of carrying on and of the row taken.
    pub searched: Option<(f32, f32)>,
}

/// The motion playing and when to look for a better one.
#[derive(Debug, Clone, Default)]
pub struct Matcher {
    /// The motion and phase playing (`None`: nothing yet, or it stopped: the next search
    /// takes the best row whatever the pose).
    pub current: Option<(u16, f32)>,
    since: f32,
    last_goal: Vec2,
    pub searches: u64,
    pub jumps: u64,
}

impl Matcher {
    /// Forget the motion playing (the body left matching: a jump, a swim, the setting).
    pub fn reset(&mut self) {
        self.current = None;
    }

    /// The frame's choice: `phase` is where the motion playing has got to (the
    /// animator's, after its last advance), `velocity` the character's over the ground
    /// (its frame), `trajectory` the controls' ([`predict`]), `goal` the velocity they
    /// ask for (a sharp change of it searches at once).
    pub fn step(&mut self, db: &Database, phase: f32, velocity: Vec2, trajectory: &[f32; 12], goal: Vec2, dt: f32) -> Option<Choice> {
        self.since += dt;
        let changed = (goal - self.last_goal).length() > (0.25 * self.last_goal.length()).max(30.0);
        let current = self.current.filter(|(m, _)| (*m as usize) < db.motions.len()).map(|(m, _)| (m, phase));
        let Some((m, ph)) = current else {
            // Nothing playing: the trajectory alone decides.
            let mut q = [0.0; FEATURES];
            q[POSE_FEATURES..].copy_from_slice(trajectory);
            let (r, c) = db.search(&q, false, |_| false)?;
            let row = db.rows[r];
            (self.current, self.since, self.last_goal) = (Some((row.motion, row.phase)), 0.0, goal);
            self.searches += 1;
            self.jumps += 1;
            return Some(Choice { motion: row.motion, phase: row.phase, jumped: true, searched: Some((f32::INFINITY, c)) });
        };
        let info = &db.motions[m as usize];
        let ending = !info.motion.looping && (1.0 - ph) * info.length < END_LEAVE;
        self.current = Some((m, ph));
        if self.since < SEARCH_INTERVAL && !changed && !ending {
            return Some(Choice { motion: m, phase: ph, jumped: false, searched: None });
        }
        self.since = 0.0;
        self.last_goal = goal;
        self.searches += 1;
        let now = db.features_at(m, ph)?;
        let q = db.query(m, ph, velocity, trajectory)?;
        let carry_on = if ending { f32::INFINITY } else { db.cost(&now, &q) };
        let length = info.length;
        let near = |r: &Row| {
            if r.motion != m {
                return false;
            }
            let d = (r.phase - ph).abs();
            let d = if info.motion.looping { d.min(1.0 - d) } else { d };
            d * length < SAME_WINDOW
        };
        let Some((r, best)) = db.search(&q, true, near) else {
            return Some(Choice { motion: m, phase: ph, jumped: false, searched: Some((carry_on, carry_on)) });
        };
        if best < carry_on - JUMP_MARGIN.max(JUMP_SHARE * carry_on) {
            let row = db.rows[r];
            self.current = Some((row.motion, row.phase));
            self.jumps += 1;
            return Some(Choice { motion: row.motion, phase: row.phase, jumped: true, searched: Some((carry_on, best)) });
        }
        Some(Choice { motion: m, phase: ph, jumped: false, searched: Some((carry_on, best)) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Affine3A;
    use fnv_nif::anim::{Channel, Key, KeyKind, Keys, TransformTrack};
    use std::collections::HashMap;

    /// Root, Bip01 (the accumulation root), the pelvis and two feet under it.
    fn skeleton() -> Arc<PreparedSkeleton> {
        let names = ["Scene Root", "Bip01", HIPS, LEFT_FOOT, RIGHT_FOOT];
        let locals = vec![Transform::IDENTITY, Transform::IDENTITY, Transform::from_xyz(0.0, 0.0, 50.0), Transform::from_xyz(-10.0, 0.0, -50.0), Transform::from_xyz(10.0, 0.0, -50.0)];
        Arc::new(PreparedSkeleton {
            names: names.iter().map(|n| Some(n.to_string())).collect(),
            parents: vec![None, Some(0), Some(1), Some(2), Some(2)],
            locals,
            bind_world: vec![Affine3A::IDENTITY; 5],
            by_name: names.iter().enumerate().map(|(i, n)| (n.to_string(), i)).collect::<HashMap<_, _>>(),
            ..Default::default()
        })
    }

    fn lin3(keys: &[(f32, [f32; 3])]) -> Channel<3> {
        Channel::Keys(Keys { kind: KeyKind::Linear, keys: keys.iter().map(|&(time, value)| Key { time, value, forward: [0.0; 3], backward: [0.0; 3], tbc: [0.0; 3] }).collect() })
    }

    fn track(t: Channel<3>) -> Arc<TransformTrack> {
        Arc::new(TransformTrack { translation: t, rotation: Channel::Static(None), euler: None, scale: Channel::Static(None), per_cycle: None })
    }

    /// A one-second loop whose root travels `dir` × `speed`, its feet planted in turn (each
    /// moving back under the body at the root's speed for half the cycle, then swung
    /// forward lifted 10 units).
    fn walk(name: &str, dir: Vec2, speed: f32) -> Arc<Clip> {
        let d = dir * speed;
        let h = speed * 0.25;
        let tracks = vec![
            ("Bip01".to_string(), track(lin3(&[(0.0, [0.0; 3]), (1.0, [d.x, d.y, 0.0])]))),
            (LEFT_FOOT.to_string(), track(lin3(&[(0.0, [-10.0, h, -50.0]), (0.5, [-10.0, -h, -50.0]), (0.75, [-10.0, 0.0, -40.0]), (1.0, [-10.0, h, -50.0])]))),
            (RIGHT_FOOT.to_string(), track(lin3(&[(0.0, [10.0, -h, -50.0]), (0.25, [10.0, 0.0, -40.0]), (0.5, [10.0, h, -50.0]), (1.0, [10.0, -h, -50.0])]))),
        ];
        Arc::new(Clip {
            name: name.into(),
            start: 0.0,
            stop: 1.0,
            frequency: 1.0,
            cycle_type: 0,
            priorities: vec![0; tracks.len()],
            tracks,
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

    fn one(kind: Kind, clip: Arc<Clip>) -> Motion {
        Motion { kind, label: clip.name.clone(), clips: vec![MotionClip { clip, weight: 1.0, offset: 0.0 }], looping: true, implied_turn: 0.0, travel_scale: 1.0 }
    }

    fn database() -> Database {
        let motions = vec![one(Kind::Move, walk("forward", Vec2::Y, 100.0)), one(Kind::Move, walk("right", Vec2::X, 100.0)), one(Kind::Move, walk("run", Vec2::Y, 300.0))];
        Database::build(skeleton(), Some("Bip01"), None, motions, Weights::default()).expect("database")
    }

    #[test]
    fn rows_cover_every_motion_at_thirty_a_second() {
        let db = database();
        assert_eq!(db.motions.len(), 3);
        for m in &db.motions {
            assert_eq!(m.rows.len(), 30, "{}", m.motion.label);
        }
        assert!((db.motions[0].speed - 100.0).abs() < 0.5);
        assert!((db.motions[2].speed - 300.0).abs() < 0.5);
    }

    #[test]
    fn a_rows_own_features_find_that_row() {
        let db = database();
        for r in [0, 17, 45, 89] {
            let (found, cost) = db.search(&db.raw[r], true, |_| false).unwrap();
            assert_eq!(found, r);
            assert!(cost < 1e-6);
        }
    }

    #[test]
    fn features_between_rows_are_interpolated() {
        let db = database();
        let a = db.features_at(0, 0.0).unwrap();
        assert_eq!(a, db.raw[0]);
        let half = db.features_at(0, 0.5 / 30.0).unwrap();
        for (d, h) in half.iter().enumerate() {
            assert!((h - 0.5 * (db.raw[0][d] + db.raw[1][d])).abs() < 1e-3);
        }
    }

    #[test]
    fn the_trajectory_picks_the_direction_and_speed() {
        let db = database();
        let mut q = [0.0; FEATURES];
        for (goal, want) in [(Vec2::new(0.0, 100.0), "forward"), (Vec2::new(100.0, 0.0), "right"), (Vec2::new(0.0, 300.0), "run")] {
            q[POSE_FEATURES..].copy_from_slice(&predict(goal, goal, 0.2, 0.0, 0.1));
            let (r, _) = db.search(&q, false, |_| false).unwrap();
            assert_eq!(db.motions[db.rows[r].motion as usize].motion.label, want);
        }
    }

    #[test]
    fn the_root_carries_on_over_cycles() {
        let m = one(Kind::Move, walk("forward", Vec2::Y, 100.0));
        let (p, _) = m.root_at(2.5);
        assert!((p - Vec2::new(0.0, 250.0)).length() < 1e-3);
        let (p, _) = m.root_at(-0.5);
        assert!((p - Vec2::new(0.0, -50.0)).length() < 1e-3);
    }

    #[test]
    fn foot_velocity_counts_the_root() {
        let db = database();
        // At phase 0.25 the left foot is planted: back under the body at the root's 100
        // units/s, so still over the ground. The right foot swings forward at 100 in the
        // model: 200 over the ground.
        let f = db.features_at(0, 0.25).unwrap();
        assert!(f[7].abs() < 1.0, "{}", f[7]);
        assert!((f[10] - 200.0).abs() < 1.0, "{}", f[10]);
    }

    #[test]
    fn the_root_travel_is_scaled_to_the_feet() {
        // A clip whose root claims twice the feet's speed is taken at the feet's.
        let mut c = (*walk("fast root", Vec2::Y, 100.0)).clone();
        c.tracks[0].1 = track(lin3(&[(0.0, [0.0; 3]), (1.0, [0.0, 200.0, 0.0])]));
        let db = Database::build(skeleton(), Some("Bip01"), None, vec![one(Kind::Move, Arc::new(c))], Weights::default()).unwrap();
        assert!((db.motions[0].speed - 100.0).abs() < 2.0, "{}", db.motions[0].speed);
        assert!((db.motions[0].motion.travel_scale - 0.5).abs() < 0.02);
    }

    #[test]
    fn prediction_eases_to_the_goal() {
        // From standing to 300 forward with a 0.2 s half-life: the place at 1 s is short of
        // 300 by about the spring's lag, 2 × half-life ÷ ln 2 × 300 ÷ 2 ... within reason.
        let t = predict(Vec2::ZERO, Vec2::new(0.0, 300.0), 0.2, 0.0, 0.1);
        assert!(t[5] > 150.0 && t[5] < 300.0, "{}", t[5]);
        assert!(t[4].abs() < 1e-3);
        // Steady: exactly the goal × time.
        let t = predict(Vec2::new(0.0, 300.0), Vec2::new(0.0, 300.0), 0.2, 0.0, 0.1);
        assert!((t[1] - 100.0).abs() < 0.1 && (t[5] - 300.0).abs() < 0.1);
        // Turning left: the facing turns anticlockwise (−x).
        let t = predict(Vec2::ZERO, Vec2::ZERO, 0.2, 2.0, 0.1);
        assert!(t[10] < 0.0);
    }

    #[test]
    fn the_matcher_jumps_when_the_controls_change_and_holds_otherwise() {
        let db = database();
        let mut m = Matcher::default();
        let fwd = Vec2::new(0.0, 100.0);
        let c = m.step(&db, 0.0, fwd, &predict(fwd, fwd, 0.2, 0.0, 0.1), fwd, 1.0 / 60.0).unwrap();
        assert!(c.jumped);
        assert_eq!(db.motions[c.motion as usize].motion.label, "forward");
        // Carrying on forward: no jump over a second of searches.
        let mut phase = c.phase;
        for _ in 0..60 {
            phase = (phase + 1.0 / 60.0).rem_euclid(1.0);
            let c = m.step(&db, phase, fwd, &predict(fwd, fwd, 0.2, 0.0, 0.1), fwd, 1.0 / 60.0).unwrap();
            assert!(!c.jumped, "{c:?}");
        }
        // Asked to go right: a jump to the right clip at once.
        let right = Vec2::new(100.0, 0.0);
        let c = m.step(&db, phase, fwd, &predict(fwd, right, 0.2, 0.0, 0.1), right, 1.0 / 60.0).unwrap();
        assert!(c.searched.is_some());
        let c = if c.jumped { c } else { m.step(&db, phase, right, &predict(right, right, 0.2, 0.0, 0.1), right, 0.2).unwrap() };
        assert!(c.jumped);
        assert_eq!(db.motions[c.motion as usize].motion.label, "right");
    }

    #[test]
    fn an_implied_turn_turns_the_facing() {
        let mut m = one(Kind::Turn, walk("turn", Vec2::ZERO, 0.0));
        m.implied_turn = std::f32::consts::FRAC_PI_2;
        let (_, yaw) = m.root_at(1.0);
        assert!((yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        let t = trajectory_of(&m, 1.0, 0.0);
        // A quarter turn to the left at 1 s: facing −x.
        assert!((t[10] + 1.0).abs() < 1e-3 && t[11].abs() < 1e-3, "{t:?}");
    }

    #[test]
    fn mirror_names_pair_left_and_right() {
        assert_eq!(mirror_name("Bip01 L Foot").as_deref(), Some("Bip01 R Foot"));
        assert_eq!(mirror_name("Bip01 R Finger12").as_deref(), Some("Bip01 L Finger12"));
        assert_eq!(mirror_name("Bip01 LPauldron").as_deref(), Some("Bip01 RPauldron"));
        assert_eq!(mirror_name("Bip01 Pelvis"), None);
        assert_eq!(mirror_name("Bip01 Spine1"), None);
        assert_eq!(mirror_name("Bip01"), None);
    }

    #[test]
    fn a_mirrored_sidestep_goes_the_other_way_on_the_other_foot() {
        let skel = skeleton();
        let right = walk("right", Vec2::X, 100.0);
        let left = mirror_clip(&skel, &right);
        let m = one(Kind::Move, Arc::new(left.clone()));
        let (p, _) = m.root_at(1.0);
        assert!((p - Vec2::new(-100.0, 0.0)).length() < 0.5, "{p}");
        // The mirrored left foot does what the right foot did, at −x.
        let mut a = Animator::new(skel.clone(), Vec::new(), Some("Bip01"));
        let mut b = Animator::new(skel.clone(), Vec::new(), Some("Bip01"));
        let (l, r) = (skel.by_name[LEFT_FOOT], skel.by_name[RIGHT_FOOT]);
        for phase in [0.0, 0.3, 0.6] {
            set_up(&mut a, &one(Kind::Move, right.clone()), None);
            set_up(&mut b, &m, None);
            let pa = pose_at(&mut a, &one(Kind::Move, right.clone()), false, phase, [l, r, l]);
            let pb = pose_at(&mut b, &m, false, phase, [l, r, l]);
            assert!((pb[0] - Vec3::new(-pa[1].x, pa[1].y, pa[1].z)).length() < 0.05, "{phase}: {} vs {}", pb[0], pa[1]);
            assert!((pb[1] - Vec3::new(-pa[0].x, pa[0].y, pa[0].z)).length() < 0.05, "{phase}");
        }
    }

    #[test]
    fn weights_parse() {
        assert_eq!(Weights::parse("1,2,3,4,5"), Some(Weights { feet_position: 1.0, feet_velocity: 2.0, hip_velocity: 3.0, trajectory_position: 4.0, trajectory_facing: 5.0 }));
        assert_eq!(Weights::parse("1,2"), None);
    }
}
