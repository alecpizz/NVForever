//! The third-person body's walking, running and turning by motion matching
//! (`nvf_assets::motion_match`, docs/design/motion-matching.md), under the Motion Matching
//! switch (NVF Features). In place of the clip choice of [`super::pose_rig`] (the idle,
//! the movement clips weighted by direction, the turning clip): the matcher picks a motion
//! and a phase from every frame of the body's locomotion clips, and a jump to another is
//! eased over by the animator's inertialization (`Animator::smooth_jump`, `Seams`).
//!
//! The motions of a clip set (posture, body, weapon drawn, limping): the idle; the walk
//! and the run in 16 directions, each a blend of the forward or backward clip with the
//! sideways one, as the original blends a diagonal (their footfalls lined up by the clips'
//! `Enum: Left` keys, actors.md §8.3); the two turning clips; each of those mirrored (left
//! for right, `motion_match::mirror_clip`); and any clip put in the skeleton's
//! `locomotion\motionmatching\` folder (none ships with the game: a mod's or the player's
//! own starts, stops and pivots). Each set's database is built once, on a thread of its
//! own, and kept for the session. While the matcher plays, the legs set the ground speed
//! (`nvf_play::player::AnimationPace`) and `crate::foot_ik` locks the planted feet.

use super::{ClipKey, Frame, Rig, Stance, TAG_IDLE, TAG_MOVE, TAG_TURN};
use bevy::prelude::*;
use fnv_actor::player_clips::{direction_weights, Dir};
use nvf_assets::actor::ActorCtx;
use nvf_assets::anim_play::Clip;
use nvf_assets::motion_match::{predict, Database, Kind, Matcher, Motion, MotionClip, Weights};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

/// Seconds a jump of the pose takes to ease out (95%) [design: Holden's inertialization
/// half-lives of about 0.1 s settle in about 0.3 s].
pub(super) const BLEND_SECONDS: f32 = 0.3;
/// How fast the predicted turn eases off, seconds (half-life): the mouse turns the body
/// at once, so a turn is taken to go on a little and then stop [design].
const TURN_HALFLIFE: f32 = 0.5;
/// The turn a turning clip stands for, radians/s: it steps on the spot without turning its
/// root (`Motion::implied_turn`); played faster or slower by the actual turn [design].
const TURN_CLIP_RATE: f32 = std::f32::consts::FRAC_PI_2;
/// Directions of the movement motions (one every 22.5°) [design].
const DIRECTIONS: usize = 16;
/// Shares of the run in the movement motions: the walk and the run, none between: a
/// walk and a run blended on one phase plant their feet at different shares of the cycle,
/// so the blend's foot is never still on the ground (planted feet slid 110-200 units/s in
/// the half-way blend) [observed: the foot skate trace, 2026-11-20].
const GAITS: [f32; 2] = [0.0, 1.0];
/// A movement motion's playback rate stays within these (the clip at another speed than
/// it was made for; the matcher picks the nearest gait first) [design].
const RATE: (f32, f32) = (0.5, 2.0);
/// How fast the ground speed follows the legs' pace, seconds (half-life) [design: quick
/// enough that a gait's change is not a slide, slow enough that it is not a jolt].
const PACE_HALFLIFE: f32 = 0.08;
/// Standing (the idle or a turn) while the keys ask to move for this long, the keys'
/// speed is let through: a guard, so a search that keeps the idle never holds the player
/// [design].
const STUCK_SECONDS: f32 = 0.3;
/// How far ahead on the controls' spring the pose's velocity is taken, seconds [design].
const LOOK_AHEAD: f32 = 0.1;
/// Seconds between the log's summaries of the matcher's work.
const SUMMARY_SECONDS: f32 = 10.0;

/// What the controls ask of the body this frame, for the matcher.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Input {
    /// The character's ground velocity in its frame (x right, y forward), units/s.
    pub velocity: Vec2,
    /// The velocity the controls ask for, in the same frame.
    pub goal: Vec2,
    /// Turning, radians/s, anticlockwise seen from above (to the left).
    pub turn: f32,
    /// How fast the velocity reaches the goal, seconds (half-life): about as the
    /// controller's (Movement Inertia's springs, else at once).
    pub halflife: f32,
}

/// The matcher's choice for the animator.
pub(super) struct Play {
    /// The clips: (clip, tag, weight, phase offset), on the shared phase.
    pub layers: Vec<(Arc<Clip>, u32, f32, f32)>,
    pub jumped: bool,
    pub phase: f32,
    /// Standing: the idle's time to jump to.
    pub idle_time: Option<f32>,
    /// The shared phase's playback rate.
    pub rate: f32,
}

/// A clip set: posture, female, the weapon drawn, limping.
type Set = (Stance, bool, Option<&'static str>, bool);

/// A rig's matcher.
#[derive(Default)]
pub(super) struct State {
    set: Option<Set>,
    /// The set's database key (its clips' paths).
    key: Option<String>,
    db: Option<Arc<Database>>,
    matcher: Matcher,
    since_summary: f32,
    told: (u64, u64),
    /// The jolt watch (`watch`): the feet's and the hips' places and velocities in the
    /// model last frame, the jolts since the last report, seconds since it.
    watched: Option<([Vec3; 3], Option<[Vec3; 3]>, f32)>,
    jolts: Vec<f32>,
    since_jolts: f32,
    /// The legs' pace, units/s, eased (`nvf_play::player::AnimationPace`); `None` while the
    /// matcher does not play; seconds standing while the keys ask to move.
    pace: Option<f32>,
    standing_asked: f32,
}

impl State {
    /// The ground speed the legs set this frame (`nvf_play::player::AnimationPace`).
    pub(super) fn pace(&self) -> Option<f32> {
        self.pace
    }
}

/// The databases built this session, by key, and those being built.
static BUILT: LazyLock<Mutex<HashMap<String, Arc<Database>>>> = LazyLock::new(Mutex::default);
type Building = (String, JoinHandle<Option<Database>>, Instant, usize);
static BUILDING: LazyLock<Mutex<Vec<Building>>> = LazyLock::new(Mutex::default);

/// The weights of the features: the default, or the test aid `NVF_PLAY_TEST_MM_WEIGHTS`
/// (`feet position, feet velocity, hip velocity, trajectory position, facing`).
fn weights() -> Weights {
    static W: LazyLock<Weights> = LazyLock::new(|| {
        let w = std::env::var("NVF_PLAY_TEST_MM_WEIGHTS").ok().and_then(|t| Weights::parse(&t));
        if let Some(w) = w {
            info!("motion matching: weights {w:?} (NVF_PLAY_TEST_MM_WEIGHTS)");
        }
        w.unwrap_or_default()
    });
    *W
}

/// The share of the cycle where the clip's left foot comes down (its `Enum: Left` key).
fn foot_phase(c: &Clip) -> Option<f32> {
    fnv_actor::locomotion::foot_phase_of(c.start, c.stop, &c.text_keys)
}

/// The motions of a clip set, with the key of its database (the clips' paths); `None`
/// without a forward walk.
fn motions(rig: &mut Rig, ctx: &mut ActorCtx, (stance, female, drawn, limping): Set) -> Option<(String, Option<Arc<Clip>>, Vec<Motion>)> {
    let mut key = rig.skeleton.clone();
    let mut keyed = |path: &str| {
        key.push('|');
        key.push_str(path);
    };
    let idle = rig.clip(ctx, ClipKey::Idle(stance, limping));
    if let Some(c) = &idle {
        keyed(&c.path);
    }
    let dirs = [Dir::Forward, Dir::Backward, Dir::Left, Dir::Right];
    let mut by: HashMap<(Dir, bool), Arc<Clip>> = HashMap::new();
    for fast in [false, true] {
        for d in dirs {
            if let Some(c) = rig.clip(ctx, ClipKey::Move(stance, female, fast, d, drawn, limping)) {
                keyed(&c.path);
                by.insert((d, fast), c.clip.clone());
            }
        }
    }
    by.get(&(Dir::Forward, false))?;
    let mut out = Vec::new();
    if let Some(c) = &idle {
        out.push(Motion { kind: Kind::Idle, label: "idle".into(), clips: vec![MotionClip { clip: c.clip.clone(), weight: 1.0, offset: 0.0 }], looping: true, implied_turn: 0.0, travel_scale: 1.0 });
    }
    for &run in &GAITS {
        for k in 0..DIRECTIONS {
            let angle = k as f32 * std::f32::consts::TAU / DIRECTIONS as f32;
            let (right, forward) = angle.sin_cos();
            let Some(w) = direction_weights(forward, right) else { continue };
            let mut clips: Vec<MotionClip> = Vec::new();
            let mut whole = true;
            for (d, dw) in w.into_iter().filter(|(_, dw)| *dw > 1e-3) {
                for (fast, gw) in [(false, 1.0 - run), (true, run)] {
                    if gw <= 1e-3 {
                        continue;
                    }
                    match by.get(&(d, fast)) {
                        Some(c) => clips.push(MotionClip { clip: c.clone(), weight: dw * gw, offset: 0.0 }),
                        None => whole = false,
                    }
                }
            }
            if !whole || clips.is_empty() {
                continue;
            }
            // The clips' left footfalls at the first clip's.
            let first = foot_phase(&clips[0].clip);
            for c in clips.iter_mut().skip(1) {
                if let (Some(a), Some(b)) = (first, foot_phase(&c.clip)) {
                    c.offset = b - a;
                }
            }
            let gait = match run {
                r if r <= 0.0 => "walk",
                r if r >= 1.0 => "run",
                _ => "walk-run",
            };
            out.push(Motion { kind: Kind::Move, label: format!("{gait} {:.0}°", angle.to_degrees()), clips, looping: true, implied_turn: 0.0, travel_scale: 1.0 });
        }
    }
    for left in [true, false] {
        if let Some(c) = rig.clip(ctx, ClipKey::Turn(stance, drawn, left, limping)) {
            keyed(&c.path);
            let implied_turn = if left { TURN_CLIP_RATE } else { -TURN_CLIP_RATE };
            out.push(Motion { kind: Kind::Turn, label: format!("turn {}", if left { "left" } else { "right" }), clips: vec![MotionClip { clip: c.clip.clone(), weight: 1.0, offset: 0.0 }], looping: true, implied_turn, travel_scale: 1.0 });
        }
    }
    // The extra clips: a mod's or the player's own, under the skeleton's folder.
    let folder = format!("{}locomotion\\motionmatching\\", rig.folder);
    let mut extra = ctx.models.data.vfs.paths(|p| p.starts_with(&folder) && p.ends_with(".kf"));
    extra.sort();
    for path in extra {
        if let Some(clip) = ctx.clip(&path) {
            keyed(&path);
            let looping = clip.cycle_type == 0;
            let label = path.rsplit('\\').next().unwrap_or(&path).to_string();
            out.push(Motion { kind: Kind::Extra, label, clips: vec![MotionClip { clip, weight: 1.0, offset: 0.0 }], looping, implied_turn: 0.0, travel_scale: 1.0 });
        }
    }
    Some((key, idle.map(|c| c.clip.clone()), out))
}

/// `list` with every motion's mirror beside it (the extra clips' too, but for their own
/// root motion they are kept as made): left for right, the other foot leading
/// (`motion_match::mirror_clip`) [design: Clavet 2016 mirrors the database; here it also
/// stands in for a badly made clip: the original's run to the right plants its feet at a
/// third of its root's speed, the mirrored run to the left at nine tenths, observed: the
/// motion matching log, 2026-11-20].
fn with_mirrors(skel: &nvf_assets::actor::PreparedSkeleton, list: Vec<Motion>) -> Vec<Motion> {
    let mut mirrored: HashMap<*const Clip, Arc<Clip>> = HashMap::new();
    let mut out = Vec::with_capacity(list.len() * 2);
    for m in list {
        if m.kind == Kind::Extra {
            out.push(m);
            continue;
        }
        let mut twin = m.clone();
        for c in &mut twin.clips {
            c.clip = mirrored.entry(Arc::as_ptr(&c.clip)).or_insert_with(|| Arc::new(nvf_assets::motion_match::mirror_clip(skel, &c.clip))).clone();
        }
        let first = twin.clips.first().and_then(|c| foot_phase(&c.clip));
        for c in twin.clips.iter_mut().skip(1) {
            c.offset = match (first, foot_phase(&c.clip)) {
                (Some(a), Some(b)) => b - a,
                _ => 0.0,
            };
        }
        twin.implied_turn = -m.implied_turn;
        twin.label = match m.kind {
            Kind::Move => {
                let (gait, angle) = m.label.rsplit_once(' ').unwrap_or((&m.label, "0°"));
                let a: f32 = angle.trim_end_matches('°').parse().unwrap_or(0.0);
                format!("{gait} {:.0}° mirrored", (360.0 - a).rem_euclid(360.0))
            }
            Kind::Turn if m.implied_turn > 0.0 => "turn right mirrored".into(),
            Kind::Turn => "turn left mirrored".into(),
            _ => format!("{} mirrored", m.label),
        };
        out.push(m);
        out.push(twin);
    }
    out
}

/// Take in the databases whose builds have finished.
fn collect_built() {
    let mut building = BUILDING.lock().unwrap_or_else(|e| e.into_inner());
    let mut i = 0;
    while i < building.len() {
        if !building[i].1.is_finished() {
            i += 1;
            continue;
        }
        let (key, handle, started, motions) = building.swap_remove(i);
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        match handle.join() {
            Ok(Some(db)) => {
                info!("motion matching: database of {} rows from {} motions ({motions} asked for) built in {ms:.0} ms", db.rows.len(), db.motions.len());
                for m in db.motions.iter().filter(|m| m.motion.kind != Kind::Move || m.motion.label.contains("0°")) {
                    let (go, _) = m.motion.root_at(1.0);
                    debug!("motion matching: {} lasts {:.2} s, {:.1} units/s (the feet's; the root's × {:.2}), turns {:.1}°/s, goes ({:.0}, {:.0}) a cycle", m.motion.label, m.length, m.speed, m.motion.travel_scale, m.turn.to_degrees(), go.x, go.y);
                }
                BUILT.lock().unwrap_or_else(|e| e.into_inner()).insert(key, Arc::new(db));
            }
            Ok(None) => warn!("motion matching: no database for {key}: the skeleton lacks a foot or the pelvis, or no clip has length; the original's clips play"),
            Err(_) => warn!("motion matching: building the database for {key} failed; the original's clips play"),
        }
    }
}

/// The frame's motion for the third-person `rig`, `None` while its database is not ready
/// (the original's choice plays meanwhile).
pub(super) fn choose(rig: &mut Rig, ctx: &mut ActorCtx, f: &Frame, input: Input) -> Option<Play> {
    collect_built();
    let set: Set = (f.stance, f.female, f.drawn, f.limping);
    if rig.mm.set != Some(set) {
        rig.mm.set = Some(set);
        rig.mm.db = None;
        rig.mm.matcher.reset();
        rig.mm.key = None;
        if let Some((key, idle, list)) = motions(rig, ctx, set) {
            let built = BUILT.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned();
            if built.is_none() {
                let mut building = BUILDING.lock().unwrap_or_else(|e| e.into_inner());
                if !building.iter().any(|b| b.0 == key) {
                    let skel = rig.animator.skel.clone();
                    let count = list.len();
                    for i in 0..skel.names.len().min(8) {
                        let l = skel.locals[i];
                        debug!("motion matching: skeleton node {i} {:?} under {:?}: at {:?}, turned {:?} (euler zyx {:?})", skel.names[i], skel.parents[i], l.translation, l.rotation, l.rotation.to_euler(EulerRot::ZYX));
                    }
                    info!("motion matching: building the database of {count} motions for {set:?}");
                    let handle = std::thread::spawn(move || {
                        let list = with_mirrors(&skel, list);
                        Database::build(skel, Some("Bip01"), idle, list, weights())
                    });
                    building.push((key.clone(), handle, Instant::now(), count));
                }
            }
            rig.mm.key = Some(key);
        } else {
            debug!("motion matching: no forward walk for {set:?}; the original's clips play");
        }
    }
    if rig.mm.db.is_none() {
        let key = rig.mm.key.as_ref()?;
        rig.mm.db = BUILT.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned();
    }
    let db = rig.mm.db.clone()?;
    // Where the motion playing has got to: the idle's own time, else the shared phase.
    let a = &rig.animator;
    let playing = rig.mm.matcher.current.and_then(|(m, _)| db.motions.get(m as usize));
    let phase = match playing.map(|m| m.motion.kind) {
        Some(Kind::Idle) => a.layers.iter().find(|l| l.tag == TAG_IDLE && !l.is_leaving()).map_or(0.0, |l| (l.time / l.clip.length().max(1e-3)).rem_euclid(1.0)),
        _ => a.phase,
    };
    // A flick of the mouse turns the view a quarter in a frame: its rate (tens of
    // radians/s) would wrap the predicted facing round the circle, and a turn either way
    // looked alike. Taken at most as twice the turning clips' [design].
    let turn = input.turn.clamp(-2.0 * TURN_CLIP_RATE, 2.0 * TURN_CLIP_RATE);
    let trajectory = predict(input.velocity, input.goal, input.halflife, turn, TURN_HALFLIFE);
    let prev = rig.mm.matcher.current.map(|(m, _)| (m, phase));
    // The pose's velocity is taken a moment ahead on the controls' spring: the legs set the
    // speed (`AnimationPace`), so the speed now is the motion's own, and a walk asked to run
    // kept the walk for seconds [observed: the foot skate trace, 2026-11-20].
    let ahead = nvf_assets::motion_match::spring_velocity(input.velocity, input.goal, input.halflife, LOOK_AHEAD);
    let choice = rig.mm.matcher.step(&db, phase, ahead, &trajectory, input.goal, f.dt)?;
    let info = &db.motions[choice.motion as usize];
    if choice.jumped {
        let (carry_on, best) = choice.searched.unwrap_or((0.0, 0.0));
        // The costs group by group (feet places, feet velocities, hips, trajectory,
        // facings) of the row taken and of carrying on, for tuning the weights.
        let groups = |m: u16, ph: f32| {
            let f = db.features_at(m, ph)?;
            let q = db.query(prev.map_or(m, |p| p.0), prev.map_or(ph, |p| p.1), input.velocity, &trajectory)?;
            Some(db.group_costs(&f, &q).map(|c| (c * 100.0).round() / 100.0))
        };
        let there = groups(choice.motion, choice.phase);
        let before = prev.and_then(|(m, ph)| groups(m, ph));
        debug!(
            "motion matching: to {} at {:.2} (cost {carry_on:.2} carrying on {before:?}, {best:.2} there {there:?}; velocity {:.0} {:.0}, asked {:.0} {:.0})",
            info.motion.label, choice.phase, input.velocity.x, input.velocity.y, input.goal.x, input.goal.y
        );
    }
    rig.mm.since_summary += f.dt;
    if rig.mm.since_summary >= SUMMARY_SECONDS {
        rig.mm.since_summary = 0.0;
        let m = &rig.mm.matcher;
        info!("motion matching: {} searches, {} jumps in the last {SUMMARY_SECONDS:.0} s; playing {}", m.searches - rig.mm.told.0, m.jumps - rig.mm.told.1, info.motion.label);
        rig.mm.told = (m.searches, m.jumps);
    }
    let tag_base = if info.motion.kind == Kind::Turn { TAG_TURN } else { TAG_MOVE };
    let layers = match info.motion.kind {
        Kind::Idle => Vec::new(),
        _ => info.motion.clips.iter().take(4).enumerate().map(|(j, c)| (c.clip.clone(), tag_base + j as u32, c.weight, c.offset)).collect(),
    };
    let rate = match info.motion.kind {
        Kind::Move | Kind::Extra if info.speed > 1.0 => (f.speed / info.speed).clamp(RATE.0, RATE.1),
        Kind::Turn if info.turn.abs() > 0.1 => (turn.abs() / info.turn.abs()).clamp(RATE.0, RATE.1),
        _ => 1.0,
    };
    let idle_time = (info.motion.kind == Kind::Idle).then(|| choice.phase * info.motion.clips[0].clip.length());
    // The legs' pace: a loop's speed over its cycle; a clip that does not loop (a start, a
    // stop) its root's speed where it is now.
    let target = match info.motion.kind {
        Kind::Idle | Kind::Turn => 0.0,
        _ if info.motion.looping => info.speed,
        _ => db.root_speed_at(choice.motion, choice.phase).unwrap_or(info.speed),
    };
    let standing = matches!(info.motion.kind, Kind::Idle | Kind::Turn) && input.goal.length() > 1.0;
    rig.mm.standing_asked = if standing { rig.mm.standing_asked + f.dt } else { 0.0 };
    let k = 1.0 - (-f.dt * std::f32::consts::LN_2 / PACE_HALFLIFE).exp();
    let eased = rig.mm.pace.unwrap_or(input.velocity.length());
    let eased = eased + (target - eased) * k;
    rig.mm.pace = (rig.mm.standing_asked < STUCK_SECONDS).then_some(eased);
    Some(Play { layers, jumped: choice.jumped, phase: choice.phase, idle_time, rate })
}

/// The body left matching this frame (in the air, swimming, an idle over it, the switch
/// off): the next search starts afresh.
pub(super) fn pause(rig: &mut Rig) {
    rig.mm.matcher.reset();
    rig.mm.pace = None;
}

/// The test aid `NVF_PLAY_TEST_FEET_JOLT`: every 10 s the log reports how sharply the
/// third-person body's feet and hips changed speed frame to frame (units/s²; a pop of the
/// pose is a spike), with Motion Matching on or off alike, for comparing the two.
pub(super) fn watch(rig: &mut Rig, dt: f32) {
    static ON: LazyLock<bool> = LazyLock::new(|| std::env::var_os("NVF_PLAY_TEST_FEET_JOLT").is_some());
    if !*ON || dt <= 0.0 {
        return;
    }
    let a = &rig.animator;
    let bone = |n: &str| a.skel.by_name.get(n).copied();
    let (Some(l), Some(r), Some(h)) = (bone(nvf_assets::motion_match::LEFT_FOOT), bone(nvf_assets::motion_match::RIGHT_FOOT), bone(nvf_assets::motion_match::HIPS)) else { return };
    let now = [l, r, h].map(|b| Vec3::from(a.model_space(b).translation));
    let st = &mut rig.mm;
    if let Some((last, last_v, last_dt)) = st.watched {
        let v = [0, 1, 2].map(|i| (now[i] - last[i]) / dt);
        // The velocities are of the last two frames' middles: the change over the time
        // between them (frames of uneven length are measured fairly).
        let between = 0.5 * (dt + last_dt);
        if let Some(lv) = last_v {
            for i in 0..3 {
                let j = (v[i] - lv[i]).length() / between;
                if j > 60_000.0 {
                    let playing = st.matcher.current.zip(st.db.as_ref()).and_then(|((m, _), db)| db.motions.get(m as usize)).map_or("the original's clips", |m| m.motion.label.as_str());
                    info!("third-person feet jolt: {j:.0} units/s² at the {} ({playing}; frames of {:.1} and {:.1} ms)", ["left foot", "right foot", "hips"][i], last_dt * 1000.0, dt * 1000.0);
                }
                st.jolts.push(j);
            }
        }
        st.watched = Some((now, Some(v), dt));
    } else {
        st.watched = Some((now, None, dt));
    }
    st.since_jolts += dt;
    if st.since_jolts >= SUMMARY_SECONDS && !st.jolts.is_empty() {
        st.since_jolts = 0.0;
        let mut j = std::mem::take(&mut st.jolts);
        j.sort_by(f32::total_cmp);
        let at = |q: f32| j[((j.len() - 1) as f32 * q) as usize];
        info!("third-person feet jolt: median {:.0}, 99th percentile {:.0}, worst {:.0} units/s² over {} samples", at(0.5), at(0.99), at(1.0), j.len());
    }
}
