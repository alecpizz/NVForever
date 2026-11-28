//! Furniture in use (docs/spec-notes/furniture.md): actors and the player get onto
//! chairs, stools, benches and beds, stay there, and get off again.
//!
//! `fnv_actor::furniture` decides everything that does not need the engine: the entry
//! point (a marker of the furniture's model that the base's marker flags enable), the
//! clips (the ones the game's IDLE tree names for the marker, the actor's sex and the
//! phase), and where the actor's root stands in each phase (`UseFrames`). Here the clips
//! are played and the roots moved:
//!
//! - An actor with a [`SitOn`] walks the last steps to the entry point, plays the
//!   getting-on clip, then the loop on the furniture; [`GetUp`] makes it play the
//!   getting-off clip, after which it is back at the entry point with its back to the
//!   furniture and both components are gone. `--npc-sit ACTOR=FURNITURE[:SECONDS]`
//!   sends a travelling NPC (`nav.rs`) to a piece of furniture and sits it down.
//! - The player uses furniture by activating it (`nvf_play::furniture`) and gets up by
//!   pressing a movement control or activating it again. Its third-person body plays the
//!   clips ([`BodyOverride`], read by `player_body.rs`); in first person the view is that
//!   body's eye and the body is not drawn [design]. Beds are used the same way: the
//!   player lies down as a sleeping actor does [owner request 2026-10-10]. No game time
//!   passes and there is no sleep menu (not built).
//!
//! While a clip plays, `GetSitting` / `GetSleeping` and the other furniture functions of
//! the script runtime answer for the user (`nvf_play::furniture::set_use`).
//!
//! Getting off plays in a frame placed so that the getting-off clip's first `Bip01`
//! position is where the getting-on clip left the body: the plain exits start where their
//! enter clips stop (furniture.md §4), so for them nothing changes; Doc Mitchell's
//! `SpecialIdle_NVDocChairFrontExit` (sequence `SpecialIdle_DocSitLeftExit`) starts its
//! `Bip01` at (−52.2, 0.1) instead of the seat's (0, 54.3) [observed: the KF], and the
//! body jumped 75 units to the side before walking off (owner's playtest list 5 item 40)
//! [hypothesis: the game plays every exit from where the body sits].
//!
//! A refused actor is told why: [`SitRefused`], which the resident's seat acts on once
//! (`residents.rs`; the rules, `fnv_actor::furniture_retry`). Identical refusals in a row
//! are logged once.
//!
//! Fidgets while on furniture (state 3 of the IDLE tree) are `idles.rs`'s: they play over
//! the loop and fade out when getting off starts (furniture.md §8).
//!
//! Not done: work spots without getting-on clips (marker 22), checking the entry point
//! against the navmesh, and sandboxing actors choosing furniture themselves (residents
//! use the furniture their package names, a bed for Sleep and a seat for Eat packages:
//! `residents.rs`).

use crate::actors::ActorWorld;
use crate::limbs::ActorRig;
use crate::locomotion::Locomotion;
use crate::nav::NavState;
use crate::world::WorldData;
use bevy::prelude::*;
use fnv_actor::furniture::{self as furn, state, BodyAt, ClipEnds, EntryPoint, Frame, Furniture, Marker, Phase, Use, UseEvent, UseFrames, UseIdles, User};
use fnv_actor::furniture_facts::{missing_body_nodes, AssetFacts};
use fnv_actor::furniture_retry::{worth_logging, Refusal};
use fnv_actor::idle::Idles;
use fnv_actor::locomotion::yaw_of;
use fnv_sim::FormId;
use nvf_assets::actor::{bind_clip, ActorCtx, ActorParts};
use nvf_assets::anim_play::{BoundClip, Clip};
use nvf_assets::data::GameData;
use nvf_assets::model::ModelCtx;
use nvf_play::furniture::{PlayerSeat, UseFurniture};
use nvf_play::script::ScriptState;
use nvf_play::RefVisual;
use std::collections::HashMap;
use std::sync::Arc;

/// Speed of the last steps to the entry point, game units per second (the travellers'
/// walking speed, `nav::WALK_SPEED`).
const APPROACH_SPEED: f32 = crate::nav::WALK_SPEED;
/// The longest way an actor is moved to an entry point without a path, game units
/// [design: an actor sent to furniture arrives next to it].
const APPROACH_MAX: f32 = 400.0;
/// Shorter last steps than this are a shuffle in place: the actor keeps its heading
/// instead of turning to face the way and back [design].
const STEP_MIN: f32 = 16.0;

/// Whether an actor `d` units from its entry point is put on the furniture at once
/// rather than stepping there and playing the getting-on clip: on a cross (marker ID 10,
/// `fnv_ai::furniture::CRUCIFIX_MARKER`) standing within a shuffle ([`STEP_MIN`]) of it.
/// A crucified actor's package holds it at its cross from the start (Anders 0x001413C3
/// is placed 8 units from his), and a sidestep there played the walk clips (with a
/// crippled leg, the limping ones) and then the climb onto the cross in sight of the
/// player [observed: issue #2471's log, smoke anders-cross-head-limit]. Found on it
/// instead [design; whether the original shows the climb is observer check 174].
/// One walked to a cross from further away still climbs on.
pub fn found_on_cross(marker_id: u8, d: f32) -> bool {
    marker_id == fnv_ai::furniture::CRUCIFIX_MARKER && d < STEP_MIN
}

/// An actor is to use this furniture reference (put on the actor's root entity).
#[derive(Component, Debug, Clone, Copy)]
pub struct SitOn {
    pub furniture: u32,
    /// Get up by itself after this many seconds on it.
    pub stay: Option<f32>,
    /// Already on it: no walk to the entry point and no getting-on clip. For an actor
    /// that was on this furniture when its cell was left and is drawn again
    /// (`residents.rs`) [design: coming back finds it as it was].
    pub at_once: bool,
}

/// The actor is to get off the furniture it is on.
#[derive(Component, Debug, Clone, Copy)]
pub struct GetUp;

/// The last [`SitOn`] was refused. On the actor's root until a use starts or its owner
/// wants something else, so that the same refusal again is not logged again.
#[derive(Component, Debug, Clone, Copy)]
pub struct SitRefused {
    pub furniture: u32,
    pub refusal: Refusal,
    /// Not acted on yet: its owner clears this, so each refusal is acted on once.
    pub unread: bool,
}

/// Why a use cannot begin: the kind, which decides what happens next, and the words for
/// the log and the player's message.
#[derive(Debug, Clone)]
struct PlanError {
    refusal: Refusal,
    detail: String,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail)
    }
}

fn refused(refusal: Refusal, detail: impl Into<String>) -> PlanError {
    PlanError { refusal, detail: detail.into() }
}

/// Who uses the furniture, as the IDLE conditions need it.
#[derive(Debug, Clone, Copy)]
struct Who {
    reference: u32,
    base: u32,
    female: bool,
}

/// What a use needs, decided once.
struct Plan {
    furniture: u32,
    base: u32,
    entry: EntryPoint,
    get_on: Arc<Clip>,
    /// Its VFS path.
    get_on_path: String,
    /// The loop on the furniture, when the IDLE tree names one, and its VFS path.
    stay: Option<Arc<Clip>>,
    stay_path: Option<String>,
    frames: UseFrames,
    /// How far the getting-on clip moves the root (`Bip01`), in the actor's space.
    travel: Vec3,
    /// The body's turn in a standing actor (the getting-on clip's first).
    standing_yaw: f32,
}

/// `--npc-sit`: a traveller to sit down when it arrives.
struct Order {
    /// Index of its trip in `NavState::travellers`.
    traveller: usize,
    furniture: u32,
    stay: Option<f32>,
    asked: bool,
}

/// Counts for the run summary.
#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    pub sat: u32,
    pub stood: u32,
    /// Refusals logged; `repeated` were the same as the one before and were not.
    pub failed: u32,
    pub repeated: u32,
    pub player_sat: u32,
    pub player_stood: u32,
}

/// The furniture records and who is on what.
#[derive(Resource)]
pub struct Furnishings {
    bases: HashMap<u32, Furniture>,
    tree: UseIdles,
    /// Markers by model path.
    markers: HashMap<String, Arc<Vec<Marker>>>,
    orders: Vec<Order>,
    /// Per furniture reference: the marker index and the reference of each user.
    users: HashMap<u32, Vec<(usize, u32)>>,
    pub stats: Stats,
    startup: Vec<String>,
}

/// Why a clip does not fit the actor's skeleton (`missing_body_nodes`), if it does not.
fn misfit(rig: &ActorRig, path: &str, clip: &Clip) -> Option<String> {
    let tracks = clip.tracks.iter().map(|(n, _)| n.as_str());
    let missing = missing_body_nodes(&rig.skeleton, path, tracks, |n| rig.skel.by_name.contains_key(n));
    missing.first().map(|first| format!("clip {path} does not fit skeleton {}: {} body tracks have no node, first {first}", rig.skeleton, missing.len()))
}

fn clip_body(clip: &Clip, t: f32) -> BodyAt {
    let track = |n: &str| clip.tracks.iter().find(|(name, _)| name == n).map(|(_, t)| t);
    let yaw = |n: &str| track(n).and_then(|tt| tt.rotation_at(t)).map_or(0.0, yaw_of);
    BodyAt { bip01: track("Bip01").and_then(|tt| tt.translation.sample(t)).unwrap_or([0.0; 3]), yaw: yaw("Bip01") + yaw("Bip01 NonAccum") }
}

/// The frame a getting-off clip `off` plays in so that its first `Bip01` position is
/// where the getting-on clip `on`, played in `clip`, left it.
fn exit_frame(clip: Frame, on: &ClipEnds, off: &ClipEnds) -> Frame {
    let (e, s) = (on.stop.bip01, off.start.bip01);
    let seat = Frame { pos: clip.point([e[0], e[1], 0.0]), heading: clip.heading };
    Frame { pos: seat.point([-s[0], -s[1], 0.0]), heading: clip.heading }
}

fn clip_ends(clip: &Clip) -> ClipEnds {
    ClipEnds { start: clip_body(clip, clip.start), stop: clip_body(clip, clip.stop) }
}

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim().trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
}

impl Furnishings {
    /// Read the furniture records and the furniture part of the IDLE tree, and turn the
    /// `--npc-sit ACTOR=FURNITURE[:SECONDS]` requests into trips for `NavState` (returned;
    /// they follow the `first_trip` trips already asked for). Each trip ends at the entry
    /// point nearest the actor's placed position, not at the furniture's origin: walked to
    /// the origin, the actor stood in the chair and was then slid backwards out of it to
    /// the entry point (owner 2026-10-16: Easy Pete "moonwalks" before sitting).
    pub fn build(world: &WorldData, data: &GameData, sit: &[String], first_trip: usize) -> Result<(Furnishings, Vec<String>), String> {
        let t0 = std::time::Instant::now();
        let mut issues = fnv_sim::extract::Issues::default();
        let bases: HashMap<u32, Furniture> = furn::furniture(world.lo, world.schema, &mut issues).into_iter().map(|f| (f.id.0, f)).collect();
        let tree = UseIdles::build(&Idles::build(world.lo, world.schema), world.lo);
        let startup =
            vec![format!("furniture: {} bases, {} idles for using them, read in {:.2}s", bases.len(), tree.len(), t0.elapsed().as_secs_f64())];
        let mut f = Furnishings { bases, tree, markers: HashMap::new(), orders: Vec::new(), users: HashMap::new(), stats: Stats::default(), startup };
        let mut trips = Vec::new();
        let refs = (!sit.is_empty()).then(|| fnv_sim::RefTable::from_index(world.lo, world.schema, world.index));
        for spec in sit {
            let bad = || format!("--npc-sit {spec}: expected ACTOR=FURNITURE[:SECONDS], the furniture as a reference FormID");
            let (who, rest) = spec.split_once('=').ok_or_else(bad)?;
            let (target, stay) = match rest.split_once(':') {
                Some((t, s)) => (t, Some(s.trim().parse::<f32>().map_err(|_| bad())?)),
                None => (rest, None),
            };
            let furniture = hex(target).ok_or_else(bad)?;
            let placed = world.index.placement(world.lo, world.schema, furniture).ok_or_else(|| format!("--npc-sit {spec}: no placed reference {furniture:08X}"))?;
            let base = placed.base.filter(|b| f.bases.contains_key(b)).ok_or_else(|| format!("--npc-sit {spec}: {furniture:08X} is not furniture"))?;
            let record = f.bases[&base].clone();
            // Where the actor starts, to pick the entry point it will come up to.
            let from = refs.as_ref().and_then(|refs| {
                let who = who.trim();
                let id = refs.by_edid(who).or_else(|| who.trim_start_matches("0x").parse::<FormId>().ok())?;
                refs.get(id).map(|r| r.pos)
            });
            let markers = record.model.as_deref().map(|m| f.markers(data, m)).unwrap_or_default();
            let points = furn::entry_points(&markers, placed.position, placed.rotation[2], placed.scale);
            let entry = from.and_then(|from| furn::nearest_entry(&points, from, |p| record.marker_enabled(p.index)));
            let to = match entry {
                Some(e) => format!("{},{},{}@{furniture:08X}", e.pos[0], e.pos[1], e.pos[2]),
                None => format!("{furniture:08X}"),
            };
            f.startup.push(format!(
                "furniture: {} is to use {furniture:08X} ({}){}",
                who.trim(),
                record.edid.as_deref().unwrap_or("?"),
                entry.map_or(String::new(), |e| format!(", walking to marker {} (ID {})", e.index, e.marker_id))
            ));
            f.orders.push(Order { traveller: first_trip + trips.len(), furniture, stay, asked: false });
            trips.push(format!("{}={to}", who.trim()));
        }
        Ok((f, trips))
    }

    fn markers(&mut self, data: &GameData, model: &str) -> Arc<Vec<Marker>> {
        if let Some(m) = self.markers.get(model) {
            return m.clone();
        }
        let read = || {
            let bytes = data.read(model)?;
            let file = fnv_nif::file::NifFile::parse(&bytes, Some(&data.schema)).ok()?;
            let dec = fnv_nif::scene::Decoded::new(&file, &bytes, &data.schema);
            Some(furn::markers(&file, &dec))
        };
        // A panic reading a broken file counts as no markers (logged once: cached).
        let m = nvf_assets::guard::guarded(|| Ok(read().unwrap_or_default())).unwrap_or_else(|e| {
            bevy::log::warn!("{model}: furniture markers not read: {e}");
            Vec::new()
        });
        let m = Arc::new(m);
        self.markers.insert(model.to_string(), m.clone());
        m
    }

    /// What the assets say about who can use which furniture, for the AI's choices
    /// (`fnv_ai::furniture`). Reads every furniture model once.
    pub fn asset_facts(&mut self, data: &GameData) -> AssetFacts {
        let t0 = std::time::Instant::now();
        let paths = data.vfs.paths(|p| p.ends_with(".nif"));
        let facts = AssetFacts::build(paths.iter().map(String::as_str), &self.tree, self.bases.values(), |p| data.read(p), &data.schema);
        let (bases, read, folders) = facts.summary();
        self.startup.push(format!("furniture: eligibility facts: {bases} bases, markers read of {read}, tree clips in {folders} skeleton folders, in {:.2}s", t0.elapsed().as_secs_f64()));
        facts
    }

    fn user(plan_entry: &EntryPoint, furniture: u32, base: u32, who: Who, state: u8) -> User {
        User {
            state,
            marker_id: plan_entry.marker_id,
            furniture_ref: FormId(furniture),
            furniture_base: FormId(base),
            actor_base: FormId(who.base),
            // The third-person body plays the clips for the player too (module notes).
            first_person: false,
            female: who.female,
            child: false,
            alerted: false,
            attacked: false,
            random_percent: 99,
        }
    }

    /// Decide how `who`, standing at `from` (game space), uses `furniture`. `game`
    /// answers the idle conditions that are about the game's state ([`game_value`]).
    fn plan(&mut self, world: &WorldData, ctx: &mut ActorCtx, furniture: u32, from: [f32; 3], who: Who, game: furn::World) -> Result<Plan, PlanError> {
        let missing = |what: &str| refused(Refusal::MissingContent, what);
        let placed = world.index.placement(world.lo, world.schema, furniture).ok_or_else(|| missing("no such reference"))?;
        let base = placed.base.ok_or_else(|| missing("no base"))?;
        let record = self.bases.get(&base).ok_or_else(|| missing("not furniture"))?.clone();
        let model = record.model.clone().ok_or_else(|| missing("the furniture has no model"))?;
        let markers = self.markers(&ctx.models.data, &model);
        if markers.is_empty() {
            return Err(missing(&format!("{model} has no furniture markers")));
        }
        let points = furn::entry_points(&markers, placed.position, placed.rotation[2], placed.scale);
        // One user per reference, except where every marker is a seat of its own
        // (benches, stools around a table) [design, furniture.md §5].
        let seats = points.len() > 1 && points.iter().all(|p| matches!(p.marker_id, 14 | 15));
        // Its own earlier use does not count (an actor drawn again).
        let taken: Vec<(usize, u32)> = self.users.get(&furniture).into_iter().flatten().copied().filter(|(_, u)| *u != who.reference).collect();
        if !seats && !taken.is_empty() {
            return Err(refused(Refusal::Busy, "in use"));
        }
        let is_taken = |p: &EntryPoint| taken.iter().any(|(i, _)| *i == p.index);
        let entry = furn::nearest_entry(&points, from, |p| record.marker_enabled(p.index) && !is_taken(p)).ok_or_else(|| {
            // Someone may yet leave an enabled entry point; none enabled stays so.
            if points.iter().any(|p| record.marker_enabled(p.index) && is_taken(p)) {
                refused(Refusal::Busy, "no free entry point")
            } else {
                refused(Refusal::NoEnabledEntry, "no enabled entry point")
            }
        })?;
        let pick = |tree: &UseIdles, s: u8| tree.pick_in(&Self::user(&entry, furniture, base, who, s), game);
        let on = pick(&self.tree, state::GETTING_ON).ok_or_else(|| refused(Refusal::NoClip, format!("no clip for getting onto marker {}", entry.marker_id)))?;
        let get_on = ctx.clip(&on.clip).ok_or_else(|| missing(&format!("{} does not load", on.clip)))?;
        let (stay, stay_path) = pick(&self.tree, state::LOOP).and_then(|p| Some((ctx.clip(&p.clip)?, p.clip))).unzip();
        let ends = clip_ends(&get_on);
        let standing_yaw = ends.start.yaw;
        let loop_yaw = stay.as_ref().map_or(standing_yaw, |c| clip_body(c, c.start).yaw);
        let frames = UseFrames::new(&entry, &ends, loop_yaw);
        let travel = Vec3::from_array(ends.stop.bip01) - Vec3::from_array(ends.start.bip01);
        Ok(Plan { furniture, base, entry, get_on, get_on_path: on.clip, stay, stay_path, frames, travel: Vec3::new(travel.x, travel.y, 0.0), standing_yaw })
    }

    /// The clip that gets `who` off again, and its VFS path.
    fn get_off(&self, ctx: &mut ActorCtx, plan: &Plan, who: Who, game: furn::World) -> Option<(String, Arc<Clip>)> {
        let pick = self.tree.pick_in(&Self::user(&plan.entry, plan.furniture, plan.base, who, state::GETTING_OFF), game)?;
        let clip = ctx.clip(&pick.clip)?;
        Some((pick.clip, clip))
    }

    fn take(&mut self, plan: &Plan, user: u32) {
        self.release(plan.furniture, user);
        self.users.entry(plan.furniture).or_default().push((plan.entry.index, user));
    }

    /// Uses whose actor is gone: its visual was taken away while on the furniture (its
    /// cell was left) or the use ended without getting off. `live` are the actors on
    /// furniture now. Returns (furniture, user) of each use let go of.
    fn forget_gone(&mut self, live: &std::collections::HashSet<u32>) -> Vec<(u32, u32)> {
        let player = FormId::PLAYER_REF.0;
        let gone: Vec<(u32, u32)> =
            self.users.iter().flat_map(|(f, us)| us.iter().map(move |(_, u)| (*f, *u))).filter(|(_, u)| *u != player && !live.contains(u)).collect();
        for (f, u) in &gone {
            self.release(*f, *u);
        }
        gone
    }

    fn release(&mut self, furniture: u32, user: u32) {
        if let Some(u) = self.users.get_mut(&furniture) {
            u.retain(|(_, who)| *who != user);
            if u.is_empty() {
                self.users.remove(&furniture);
            }
        }
    }
}

pub fn log_startup(mut f: ResMut<Furnishings>) {
    for line in std::mem::take(&mut f.startup) {
        info!("{line}");
    }
}

fn rotation(heading: f32) -> Quat {
    Quat::from_rotation_z(-heading)
}

/// Bevy space (the world root's rotation, `nav.rs`) back to game space.
fn to_game(p: Vec3) -> Vec3 {
    Vec3::new(p.x, -p.z, p.y)
}

/// The compass heading a root transform faces.
fn heading_of(t: &Transform) -> f32 {
    let f = t.rotation * Vec3::Y;
    f.x.atan2(f.y)
}

/// The shorter way round from heading `a` to `b`, at share `k`.
fn lerp_heading(a: f32, b: f32, k: f32) -> f32 {
    let d = (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    a + d * k
}

/// What the game being played answers for an idle condition that is not about the
/// furniture's user: a quest's stage (`GetStage`). `None` without a game, or for any
/// other function: the idle is then not chosen.
fn game_value(script: &Option<ResMut<ScriptState>>, function: u16, param: Option<FormId>) -> Option<f32> {
    let game = &script.as_deref()?.rt.as_ref()?.game;
    match function {
        furn::func::GET_STAGE => Some(game.quests.get(&param?).map_or(0, |q| q.stage()) as f32),
        _ => None,
    }
}

fn tell_scripts(script: &mut Option<ResMut<ScriptState>>, user: u32, using: Option<(&Plan, u8)>) {
    if let Some(s) = script.as_deref_mut() {
        nvf_play::furniture::set_use(s, user, using.map(|(p, state)| (p.furniture, p.entry.marker_id, state)));
    }
}

// ---- actors -----------------------------------------------------------------------

/// The last steps from where an actor stopped to the entry point, as a person takes them:
/// it turns to face the way on the spot, walks there forwards, and turns on the spot to
/// the entry heading [design; the owner's playtest 2026-10-16: slid there in one
/// straight move while turning, Easy Pete walked backwards out of his chair with the
/// walk clip going forwards].
struct Approach {
    from: Frame,
    to: Frame,
    /// The heading it walks with.
    way: f32,
    /// Seconds turning to `way`, walking, turning to the entry heading.
    turn_in: f32,
    walk: f32,
    turn_out: f32,
    t: f32,
}

/// Where an approaching actor is, and its root's velocity (in its own space, X right,
/// Y forward; game units per second) and turn rate (radians per second, positive left).
struct Step {
    at: Frame,
    velocity: [f32; 2],
    turn: f32,
}

impl Approach {
    fn new(from: Frame, to: Frame) -> Approach {
        let d = [to.pos[0] - from.pos[0], to.pos[1] - from.pos[1]];
        let len = d[0].hypot(d[1]);
        let way = if len >= STEP_MIN { d[0].atan2(d[1]).rem_euclid(std::f32::consts::TAU) } else { from.heading };
        let turn = |a: f32, b: f32| fnv_nav::follow::heading_delta(a, b).abs() / fnv_nav::follow::SPOT_TURN_RATE;
        Approach { from, to, way, turn_in: turn(from.heading, way), walk: len / APPROACH_SPEED, turn_out: turn(way, to.heading), t: 0.0 }
    }

    fn len(&self) -> f32 {
        self.turn_in + self.walk + self.turn_out
    }

    /// The step `t` seconds in.
    fn at(&self, t: f32) -> Step {
        let turning = |a: f32, b: f32, k: f32| {
            let turn = -fnv_nav::follow::heading_delta(a, b).signum() * fnv_nav::follow::SPOT_TURN_RATE;
            (lerp_heading(a, b, k.clamp(0.0, 1.0)), if k < 1.0 { turn } else { 0.0 })
        };
        let pos = |k: f32| Vec3::from_array(self.from.pos).lerp(Vec3::from_array(self.to.pos), k.clamp(0.0, 1.0)).to_array();
        if t < self.turn_in {
            let (heading, turn) = turning(self.from.heading, self.way, t / self.turn_in);
            return Step { at: Frame { pos: self.from.pos, heading }, velocity: [0.0; 2], turn };
        }
        let t = t - self.turn_in;
        if t < self.walk {
            // The way in its own space: forwards, or (a shuffle) whichever way it steps.
            let d = Vec3::from_array(self.to.pos) - Vec3::from_array(self.from.pos);
            let v = Vec2::new(d.x, d.y) / self.walk;
            let (sn, cs) = self.way.sin_cos();
            let velocity = [v.x * cs - v.y * sn, v.x * sn + v.y * cs];
            return Step { at: Frame { pos: pos(t / self.walk), heading: self.way }, velocity, turn: 0.0 };
        }
        let k = if self.turn_out > 0.0 { (t - self.walk) / self.turn_out } else { 1.0 };
        let (heading, turn) = turning(self.way, self.to.heading, k);
        Step { at: Frame { pos: self.to.pos, heading }, velocity: [0.0; 2], turn }
    }
}

impl OnFurniture {
    /// The furniture reference used.
    pub fn furniture(&self) -> u32 {
        self.plan.furniture
    }

    /// On it (getting on, staying or getting off), not walking up to it.
    pub fn seated(&self) -> bool {
        matches!(self.stage, Stage::Using(_))
    }

    /// Getting off it, or asked to while still getting on.
    pub fn leaving(&self) -> bool {
        matches!(&self.stage, Stage::Using(u) if u.phase == Phase::GettingOff || u.leave_wanted)
    }
}

enum Stage {
    /// The last steps to the entry point: turn to face the way, walk there, turn to the
    /// entry heading (`Approach::at`).
    Approach(Approach),
    Using(Use),
}

/// An actor walking up to, on, or getting off furniture.
#[derive(Component)]
pub struct OnFurniture {
    plan: Plan,
    who: Who,
    stage: Stage,
    /// What the skeleton plays now and where in it (0–1); read by `locomotion::drive`.
    /// `None` while the actor still walks.
    pub pose: Option<(Arc<BoundClip>, f32)>,
    get_on: Arc<BoundClip>,
    stay: Option<Arc<BoundClip>>,
    get_off: Option<(Arc<BoundClip>, ClipEnds)>,
    /// The frame the getting-off clip plays in: placed so that its first `Bip01`
    /// position is where the getting-on clip left the body (module notes).
    exit_frame: Option<Frame>,
    /// The actor's parent's place in game space (its own transform is relative to it).
    origin: Vec3,
    stay_left: Option<f32>,
}

/// Drawn actors not yet told to sit.
type Unseated = (With<ActorRig>, Without<SitOn>);

/// `--npc-sit`: once a traveller has arrived and is drawn, it is told to sit.
pub fn seat_orders(mut commands: Commands, mut f: ResMut<Furnishings>, nav: Option<Res<NavState>>, actors: Query<(Entity, &RefVisual), Unseated>) {
    let Some(nav) = nav else { return };
    for o in f.orders.iter_mut().filter(|o| !o.asked) {
        let Some(tr) = nav.travellers.get(o.traveller) else { continue };
        if tr.failed.is_some() {
            o.asked = true;
            warn!("furniture: {} did not get to {:08X}", tr.label, o.furniture);
            continue;
        }
        if tr.arrived.is_none() {
            continue;
        }
        if let Some((e, _)) = actors.iter().find(|(_, v)| v.0 == tr.reference) {
            commands.entity(e).insert(SitOn { furniture: o.furniture, stay: o.stay, at_once: false });
            o.asked = true;
        }
    }
}

/// The actor is off its furniture at once, without the getting-off clip: game time
/// passed in the Wait menu and its package has it elsewhere (`residents.rs`), which is
/// where its owner puts it.
#[derive(Component)]
pub struct LeaveNow;

/// What [`drive_actors`] reads and writes of an actor about to begin a use.
type Starting = (Entity, &'static ActorRig, &'static mut Transform, &'static GlobalTransform, &'static SitOn, Option<&'static SitRefused>);

/// What [`drive_actors`] reads and writes of an actor on furniture.
type Seated = (Entity, &'static ActorRig, &'static mut Transform, &'static mut OnFurniture, Option<&'static mut Locomotion>, Has<GetUp>, Has<LeaveNow>);

/// Start, advance and end the actors' uses; place their roots.
#[allow(clippy::too_many_arguments)]
pub fn drive_actors(
    mut commands: Commands,
    time: Res<Time>,
    mut f: ResMut<Furnishings>,
    world: Res<WorldData>,
    actor_world: Option<Res<ActorWorld>>,
    mut parts: ResMut<ActorParts>,
    mut models: ModelCtx,
    mut starting: Query<Starting, Without<OnFurniture>>,
    mut using: Query<Seated>,
    mut script: Option<ResMut<ScriptState>>,
    mut nav: Option<ResMut<NavState>>,
) {
    let dt = time.delta_secs().min(0.1);
    // Actors no longer drawn leave their furniture free: drawn again, they are put back
    // on it (`SitOn::at_once`) or go elsewhere [fix of the owner's playtest 2026-10-11:
    // the stool stayed "in use" by its own sitter, who then stood in it].
    let live: std::collections::HashSet<u32> = using.iter().map(|(_, _, _, on, ..)| on.who.reference).collect();
    for (furniture, user) in f.forget_gone(&live) {
        info!("furniture: {user:08X} is off {furniture:08X} unseen (no longer drawn)");
        tell_scripts(&mut script, user, None);
    }
    let mut ctx = ActorCtx { cache: &mut parts, models: &mut models };
    for (e, rig, mut t, global, sit, before) in &mut starting {
        let at = to_game(global.translation());
        let female = actor_world.as_deref().and_then(|w| w.actors.npcs.get(&FormId(rig.base))).is_some_and(|n| n.female);
        let who = Who { reference: rig.reference, base: rig.base, female };
        let plan = f.plan(&world, &mut ctx, sit.furniture, at.to_array(), who, &|func, p| game_value(&script, func, p)).and_then(|p| {
            // A clip that does not fit is refused here rather than posed wrongly, before
            // the furniture is taken: anything that gets past the AI's choice
            // (`fnv_ai::furniture`), and `--npc-sit`.
            let stay = p.stay.as_deref().zip(p.stay_path.as_deref());
            if let Some(why) = misfit(rig, &p.get_on_path, &p.get_on).or_else(|| stay.and_then(|(c, path)| misfit(rig, path, c))) {
                return Err(refused(Refusal::IncompatibleClip, why));
            }
            let d = Vec3::from_array(p.entry.pos).distance(at);
            if d > APPROACH_MAX && !sit.at_once { Err(refused(Refusal::TooFar, format!("{d:.0} units from its entry point"))) } else { Ok((p, d)) }
        });
        match plan {
            Ok((plan, d)) => {
                // Already at a cross: held on it at once (#2471).
                let found_on = !sit.at_once && found_on_cross(plan.entry.marker_id, d);
                let at_once = sit.at_once || found_on;
                info!(
                    "furniture: {:08X} {} {:08X} by marker {} (ID {}), {d:.0} units away; clips {} / {}",
                    rig.reference,
                    if sit.at_once {
                        "is back on"
                    } else if found_on {
                        "is put on (a cross it stands at: no steps, no getting-on clip)"
                    } else {
                        "goes to"
                    },
                    plan.furniture,
                    plan.entry.index,
                    plan.entry.marker_id,
                    plan.get_on.name,
                    plan.stay.as_ref().map_or("none", |c| c.name.as_str())
                );
                f.take(&plan, rig.reference);
                let get_on = Arc::new(bind_clip(&rig.skel, plan.get_on.clone()));
                let stay = plan.stay.clone().map(|c| Arc::new(bind_clip(&rig.skel, c)));
                let from = Frame { pos: at.to_array(), heading: heading_of(&t) };
                let origin = at - t.translation;
                // Back on it at once: a use whose getting-on takes no time, so the next
                // update has the actor on, in the loop (scripts are told as for any use).
                let (stage, pose) = if at_once {
                    tell_scripts(&mut script, rig.reference, Some((&plan, state::GETTING_ON)));
                    let on = if plan.stay.is_some() { plan.frames.on } else { Frame { pos: plan.frames.clip.point(plan.travel.to_array()), heading: plan.frames.clip.heading } };
                    t.translation = Vec3::from_array(on.pos) - origin;
                    t.rotation = rotation(on.heading);
                    (Stage::Using(Use::begin(FormId(plan.furniture), FormId(plan.base), plan.entry, 0.0)), Some((get_on.clone(), 0.999)))
                } else {
                    (Stage::Approach(Approach::new(from, Frame { pos: plan.entry.pos, heading: plan.entry.heading })), None)
                };
                // Its body is the furniture's now: a conversation turns only its head.
                commands.entity(e).remove::<SitRefused>().insert(crate::dialogue_look::BodyHeld).insert(OnFurniture {
                    plan,
                    who,
                    stage,
                    pose,
                    get_on,
                    stay,
                    get_off: None,
                    exit_frame: None,
                    origin,
                    stay_left: sit.stay,
                });
            }
            Err(why) => {
                if worth_logging(before.map(|b| (b.furniture, b.refusal)), sit.furniture, why.refusal) {
                    warn!("furniture: {:08X} cannot use {:08X}: {why}", rig.reference, sit.furniture);
                    f.stats.failed += 1;
                } else {
                    f.stats.repeated += 1;
                }
                commands.entity(e).remove::<(SitOn, GetUp)>().insert(SitRefused { furniture: sit.furniture, refusal: why.refusal, unread: true });
            }
        }
    }

    for (e, rig, mut t, mut on, mut lo, get_up, leave_now) in &mut using {
        let on = &mut *on;
        if leave_now {
            if let Some(lo) = lo.as_deref_mut() {
                lo.velocity = [0.0; 2];
                lo.turn = 0.0;
            }
            f.stats.stood += 1;
            f.release(on.plan.furniture, on.who.reference);
            info!("furniture: {:08X} is off {:08X} at once (hours passed in the wait menu)", on.who.reference, on.plan.furniture);
            tell_scripts(&mut script, on.who.reference, None);
            commands.entity(e).remove::<(OnFurniture, SitOn, GetUp, LeaveNow, crate::dialogue_look::BodyHeld)>();
            continue;
        }
        let scale = t.scale.x.max(1e-3);
        let mut place = |frame: Frame| {
            t.translation = Vec3::from_array(frame.pos) - on.origin;
            t.rotation = rotation(frame.heading);
        };
        let clip_frame = on.plan.frames.clip;
        match &mut on.stage {
            Stage::Approach(a) => {
                a.t += dt;
                let step = a.at(a.t);
                place(step.at);
                if let Some(lo) = lo.as_deref_mut() {
                    // Its own scale: the clips are chosen for the unscaled speed.
                    lo.velocity = step.velocity.map(|v| v / scale);
                    lo.turn = step.turn;
                }
                if a.t >= a.len() {
                    let len = on.get_on.clip.length();
                    on.stage = Stage::Using(Use::begin(FormId(on.plan.furniture), FormId(on.plan.base), on.plan.entry, len));
                    on.pose = Some((on.get_on.clone(), 0.0));
                    place(clip_frame);
                    tell_scripts(&mut script, on.who.reference, Some((&on.plan, state::GETTING_ON)));
                }
            }
            Stage::Using(usage) => {
                if get_up {
                    usage.leave_wanted = true;
                }
                let ev = usage.advance(dt);
                if ev == Some(UseEvent::GotOn) {
                    f.stats.sat += 1;
                    info!("furniture: {:08X} is on {:08X}", on.who.reference, on.plan.furniture);
                    tell_scripts(&mut script, on.who.reference, Some((&on.plan, state::ON)));
                }
                if usage.phase == Phase::On {
                    if let Some(left) = on.stay_left.as_mut() {
                        *left -= dt;
                        if *left <= 0.0 {
                            usage.leave_wanted = true;
                            on.stay_left = None;
                        }
                    }
                    if usage.leave_wanted && usage.leave() {
                        // An exit clip that does not fit is not played: the actor is off on
                        // the next update, at the entry point, by the way of a seat with no
                        // exit clip.
                        let clip = f.get_off(&mut ctx, &on.plan, on.who, &|func, p| game_value(&script, func, p)).and_then(|(path, c)| match misfit(rig, &path, &c) {
                            Some(why) => {
                                warn!("furniture: {:08X} gets off {:08X} without a clip: {why}", on.who.reference, on.plan.furniture);
                                None
                            }
                            None => Some(c),
                        });
                        info!("furniture: {:08X} gets off {:08X} by clip {}", on.who.reference, on.plan.furniture, clip.as_ref().map_or("none", |c| c.name.as_str()));
                        let bound = clip.map(|c| (clip_ends(&c), Arc::new(bind_clip(&rig.skel, c))));
                        usage.begin_leaving(bound.as_ref().map_or(0.0, |(_, b)| b.clip.length()));
                        // Played from where the body sits: the plain exits start where
                        // their enter clips stop, a special one (Doc Mitchell's
                        // SpecialIdle_NVDocChairFrontExit, whose Bip01 starts 52 units to
                        // the side) would otherwise jump there first.
                        let on_stop = clip_ends(&on.plan.get_on).stop.bip01;
                        if let Some((ends, b)) = &bound {
                            let off = (ends.start.bip01[0] - on_stop[0]).hypot(ends.start.bip01[1] - on_stop[1]);
                            if off > 10.0 {
                                info!("furniture: {:08X}: {} starts {off:.0} units from where the body sits: played from the seat", on.who.reference, b.clip.name);
                            }
                        }
                        on.exit_frame = bound.as_ref().map(|(ends, _)| exit_frame(clip_frame, &clip_ends(&on.plan.get_on), ends));
                        on.get_off = bound.map(|(ends, b)| (b, ends));
                        tell_scripts(&mut script, on.who.reference, Some((&on.plan, state::GETTING_OFF)));
                    }
                }
                let phase = if usage.clip_len > 0.0 { (usage.t / usage.clip_len).min(0.999) } else { 0.999 };
                match usage.phase {
                    Phase::GettingOn => {
                        on.pose = Some((on.get_on.clone(), phase));
                        place(Frame { pos: clip_frame.point((on.plan.travel * phase).to_array()), heading: clip_frame.heading });
                    }
                    Phase::On => match &on.stay {
                        Some(stay) => {
                            let len = stay.clip.length().max(1e-3);
                            on.pose = Some((stay.clone(), (usage.t / len).fract()));
                            place(on.plan.frames.on);
                        }
                        // No loop for this furniture: the getting-on clip's last pose.
                        None => {
                            on.pose = Some((on.get_on.clone(), 0.999));
                            place(Frame { pos: clip_frame.point(on.plan.travel.to_array()), heading: clip_frame.heading });
                        }
                    },
                    Phase::GettingOff if ev != Some(UseEvent::GotOff) => {
                        if let Some((clip, ends)) = &on.get_off {
                            let travel = Vec3::from_array(ends.stop.bip01) - Vec3::from_array(ends.start.bip01);
                            on.pose = Some((clip.clone(), phase));
                            let frame = on.exit_frame.unwrap_or(clip_frame);
                            place(Frame { pos: frame.point([travel.x * phase, travel.y * phase, 0.0]), heading: frame.heading });
                        }
                    }
                    Phase::GettingOff => {
                        // Back on its feet: at the entry point, turned as the clip left it.
                        let end = match &on.get_off {
                            Some((_, ends)) => UseFrames { clip: on.exit_frame.unwrap_or(clip_frame), on: on.plan.frames.on }.after(ends, on.plan.standing_yaw),
                            None => Frame { pos: on.plan.entry.pos, heading: on.plan.entry.heading },
                        };
                        place(end);
                        if let Some(tr) = nav.as_deref_mut().and_then(|n| n.travellers.iter_mut().find(|tr| tr.reference == on.who.reference)) {
                            tr.walker.feet = end.pos;
                            tr.walker.heading = end.heading;
                        }
                        if let Some(lo) = lo.as_deref_mut() {
                            lo.velocity = [0.0; 2];
                            lo.turn = 0.0;
                        }
                        f.stats.stood += 1;
                        f.release(on.plan.furniture, on.who.reference);
                        info!("furniture: {:08X} got off {:08X}", on.who.reference, on.plan.furniture);
                        tell_scripts(&mut script, on.who.reference, None);
                        commands.entity(e).remove::<(OnFurniture, SitOn, GetUp, crate::dialogue_look::BodyHeld)>();
                    }
                }
            }
        }
    }
}

// ---- the player -------------------------------------------------------------------

/// A clip the player's third-person body plays instead of its own, with the body's
/// place (`player_body.rs`).
#[derive(Clone)]
pub struct BodyClip {
    pub clip: Arc<Clip>,
    pub looped: bool,
    /// Changes when the clip does: the body starts it from its beginning.
    pub serial: u32,
    pub pos: [f32; 3],
    pub heading: f32,
    /// The furniture is a bed: the body lies down, and the first-person view lies down
    /// with it (`player_body.rs`).
    pub lying: bool,
}

#[derive(Resource, Default)]
pub struct BodyOverride(pub Option<BodyClip>);

struct PlayerUsing {
    plan: Plan,
    usage: Use,
    get_off: Option<(Arc<Clip>, ClipEnds)>,
}

/// The player's use of furniture.
#[derive(Resource, Default)]
pub struct PlayerUse {
    using: Option<PlayerUsing>,
    serial: u32,
}

impl PlayerUse {
    /// Whether the player is on furniture (getting on, on it, or getting off).
    pub fn in_use(&self) -> bool {
        self.using.is_some()
    }
}

/// Where a getting-on or getting-off clip has the root at `t` seconds into it: the
/// player's body holds `Bip01` still (`nvf_assets::anim_blend`), so the root follows its
/// track.
fn root_at(frame: &Frame, clip: &Clip, t: f32) -> [f32; 3] {
    let p = clip_body(clip, clip.start + t.clamp(0.0, clip.length())).bip01;
    frame.point([p[0], p[1], 0.0])
}

/// The player's use of furniture: start it on activation, advance it, end it.
#[allow(clippy::too_many_arguments)]
pub fn drive_player(
    time: Res<Time>,
    f: Option<ResMut<Furnishings>>,
    world: Res<WorldData>,
    mut asked: MessageReader<UseFurniture>,
    mut seat: ResMut<PlayerSeat>,
    mut state_: ResMut<PlayerUse>,
    mut body: ResMut<BodyOverride>,
    player: Res<nvf_play::player::Player>,
    mut control: ResMut<nvf_play::control::PlayerControl>,
    mut phys: ResMut<nvf_play::phys::PhysState>,
    body_state: Res<crate::player_body::PlayerBody>,
    mut parts: ResMut<ActorParts>,
    mut models: ModelCtx,
    mut script: Option<ResMut<ScriptState>>,
    (mut hud, mut rest): (ResMut<nvf_play::activate::HudMessage>, MessageWriter<nvf_play::wait::OpenWait>),
) {
    let Some(mut f) = f else { return };
    let dt = time.delta_secs().min(0.1);
    let mut ctx = ActorCtx { cache: &mut parts, models: &mut models };
    let who = Who { reference: FormId::PLAYER_REF.0, base: FormId::PLAYER_BASE.0, female: body_state.female().unwrap_or(false) };
    let st = &mut *state_;
    for UseFurniture { reference } in asked.read().copied() {
        match &mut st.using {
            // Activating the furniture one is on: get up (not under the player's Activate
            // Doesn't Stand Up tweak, docs/design/tweaks.md).
            Some(u) if u.plan.furniture == reference => seat.leave |= !nvf_play::tweaks::SEAT_KEEPS_ON_ACTIVATE.load(std::sync::atomic::Ordering::Relaxed),
            Some(_) => {}
            None => match f.plan(&world, &mut ctx, reference, player.feet, who, &|func, p| game_value(&script, func, p)) {
                // Beds as seats: the player lies down as a sleeping actor does [owner
                // request 2026-10-10], and once lying the Sleep menu opens (below).
                Ok(plan) => {
                    info!(
                        "furniture: the player {} {reference:08X} by marker {} (ID {}); clips {} / {}",
                        if furn::is_bed(plan.entry.marker_id) { "lies down on" } else { "uses" },
                        plan.entry.index,
                        plan.entry.marker_id,
                        plan.get_on.name,
                        plan.stay.as_ref().map_or("none", |c| c.name.as_str())
                    );
                    f.take(&plan, who.reference);
                    // Sitting puts the weapon away [design].
                    control.weapon_out = false;
                    let usage = Use::begin(FormId(plan.furniture), FormId(plan.base), plan.entry, plan.get_on.length());
                    tell_scripts(&mut script, who.reference, Some((&plan, state::GETTING_ON)));
                    st.serial += 1;
                    st.using = Some(PlayerUsing { plan, usage, get_off: None });
                    seat.leave = false;
                }
                Err(why) => {
                    info!("furniture: the player cannot use {reference:08X}: {why}");
                    hud.show(format!("You cannot use this ({why})."));
                }
            },
        }
    }
    let Some(u) = st.using.as_mut() else {
        seat.leave = false;
        return;
    };
    if std::mem::take(&mut seat.leave) {
        u.usage.leave_wanted = true;
    }
    let ev = u.usage.advance(dt);
    if ev == Some(UseEvent::GotOn) {
        f.stats.player_sat += 1;
        st.serial += 1;
        info!("furniture: the player is on {:08X}", u.plan.furniture);
        tell_scripts(&mut script, who.reference, Some((&u.plan, state::ON)));
        // Lying on a bed: the Sleep menu comes up by itself (wait-menu.md §8).
        if u.usage.is_bed() {
            seat.bed = true;
            seat.on = Some(u.plan.furniture);
            rest.write(nvf_play::wait::OpenWait::default());
        }
    }
    if u.usage.phase == Phase::On && u.usage.leave_wanted && u.usage.leave() {
        let clip = f.get_off(&mut ctx, &u.plan, who, &|func, p| game_value(&script, func, p)).map(|(_, c)| c);
        u.usage.begin_leaving(clip.as_ref().map_or(0.0, |c| c.length()));
        u.get_off = clip.map(|c| (c.clone(), clip_ends(&c)));
        st.serial += 1;
        tell_scripts(&mut script, who.reference, Some((&u.plan, state::GETTING_OFF)));
    }
    let frames = u.plan.frames;
    let lying = u.usage.is_bed();
    let shown = match u.usage.phase {
        Phase::GettingOn => Some(BodyClip {
            clip: u.plan.get_on.clone(),
            looped: false,
            serial: st.serial,
            pos: root_at(&frames.clip, &u.plan.get_on, u.usage.t),
            heading: frames.clip.heading,
            lying,
        }),
        Phase::On => Some(match &u.plan.stay {
            Some(stay) => BodyClip { clip: stay.clone(), looped: true, serial: st.serial, pos: frames.on.pos, heading: frames.on.heading, lying },
            None => BodyClip {
                clip: u.plan.get_on.clone(),
                looped: false,
                serial: st.serial,
                pos: root_at(&frames.clip, &u.plan.get_on, f32::MAX),
                heading: frames.clip.heading,
                lying,
            },
        }),
        Phase::GettingOff if ev != Some(UseEvent::GotOff) => u.get_off.as_ref().map(|(c, _)| BodyClip {
            clip: c.clone(),
            looped: false,
            serial: st.serial,
            pos: root_at(&frames.clip, c, u.usage.t),
            heading: frames.clip.heading,
            lying,
        }),
        Phase::GettingOff => None,
    };
    match shown {
        Some(b) => {
            seat.hold = Some(b.pos);
            seat.bed = lying && u.usage.phase != Phase::GettingOff;
            seat.on = Some(u.plan.furniture);
            seat.seated = u.usage.phase == Phase::On;
            // Getting off is sent to the co-op players as off (`coop.rs`).
            seat.getting_off = u.usage.phase == Phase::GettingOff;
            body.0 = Some(b);
        }
        None => {
            // Off: the character stands at the entry point again.
            let end = match &u.get_off {
                Some((_, ends)) => frames.after(ends, u.plan.standing_yaw),
                None => Frame { pos: u.plan.entry.pos, heading: u.plan.entry.heading },
            };
            phys.place_player(end.pos, 60.0, 200.0);
            seat.hold = None;
            seat.bed = false;
            seat.on = None;
            seat.seated = false;
            body.0 = None;
            f.stats.player_stood += 1;
            f.release(u.plan.furniture, who.reference);
            info!("furniture: the player got off {:08X}", u.plan.furniture);
            tell_scripts(&mut script, who.reference, None);
            st.using = None;
        }
    }
}

/// The furniture line of the run summary.
pub fn report(mut exit: MessageReader<AppExit>, f: Res<Furnishings>) {
    if exit.read().next().is_none() {
        return;
    }
    let s = f.stats;
    info!(
        "furniture summary: {} of {} actors sat down, {} got up again, {} could not; the player sat down {} times and got up {}",
        s.sat,
        f.orders.len(),
        s.stood,
        s.failed,
        s.player_sat,
        s.player_stood
    );
    if s.failed + s.repeated > 0 {
        info!("furniture refusals: {} logged, {} identical repeats not logged", s.failed, s.repeated);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI, TAU};

    /// The numbers of `chair_forwardenter`, `chair_forwardexit` and Doc Mitchell's
    /// `SpecialIdle_NVDocChairFrontExit` (furniture.md §4; Bip01 from the KF's B-spline).
    #[test]
    fn exits_start_where_the_body_sits() {
        let at = |x: f32, y: f32| BodyAt { bip01: [x, y, 0.0], yaw: 0.0 };
        let on = ClipEnds { start: at(0.0, -1.1), stop: at(0.0, 54.3) };
        let plain = ClipEnds { start: at(0.0, 54.3), stop: at(0.0, -1.1) };
        let doc = ClipEnds { start: at(-52.2, 0.1), stop: at(-17.5, -14.1) };
        let clip = Frame { pos: [100.0, 200.0, 0.0], heading: FRAC_PI_2 };
        let same = exit_frame(clip, &on, &plain);
        assert!((same.pos[0] - 100.0).abs() < 1e-3 && (same.pos[1] - 200.0).abs() < 1e-3 && same.heading == clip.heading);
        // The special exit's first body position is the seat, not 52 units beside it.
        let seat = clip.point([0.0, 54.3, 0.0]);
        let f = exit_frame(clip, &on, &doc);
        let first = f.point([-52.2, 0.1, 0.0]);
        assert!((first[0] - seat[0]).abs() < 1e-3 && (first[1] - seat[1]).abs() < 1e-3, "{first:?} {seat:?}");
    }

    /// Easy Pete stopped in his chair, facing it (north), with the front entry point 54
    /// units south of him facing north too: he turns round, walks south forwards, turns
    /// round again; before, he slid south facing north with the walk clip going forwards.
    #[test]
    fn approach_walks_forwards_then_turns() {
        let a = Approach::new(Frame { pos: [0.0, 0.0, 0.0], heading: 0.0 }, Frame { pos: [0.0, -54.0, 0.0], heading: 0.0 });
        assert!((a.way - PI).abs() < 1e-3, "{}", a.way);
        assert!((a.turn_in - PI / fnv_nav::follow::SPOT_TURN_RATE).abs() < 1e-3 && (a.turn_out - a.turn_in).abs() < 1e-3);
        let walking = a.at(a.turn_in + a.walk / 2.0);
        assert!(walking.velocity[1] > 0.0 && walking.velocity[0].abs() < 1e-3, "{:?}", walking.velocity);
        assert!((walking.at.pos[1] + 27.0).abs() < 0.5 && walking.turn == 0.0);
        let turning = a.at(a.turn_in / 2.0);
        assert!(turning.at.pos == [0.0; 3] && turning.velocity == [0.0; 2] && turning.turn != 0.0);
        let end = a.at(a.len());
        assert!((end.at.pos[1] + 54.0).abs() < 1e-3 && fnv_nav::follow::heading_delta(end.at.heading, 0.0).abs() < 1e-3 && end.turn == 0.0);
        // A few units: a shuffle, no turning round.
        let short = Approach::new(Frame { pos: [0.0; 3], heading: 0.0 }, Frame { pos: [0.0, -5.0, 0.0], heading: 0.0 });
        assert!(short.turn_in == 0.0 && short.turn_out == 0.0 && short.at(short.walk / 2.0).velocity[1] < 0.0);
    }

    #[test]
    fn uses_of_actors_no_longer_drawn_are_let_go() {
        let mut f = Furnishings { bases: HashMap::new(), tree: UseIdles::default(), markers: HashMap::new(), orders: Vec::new(), users: HashMap::new(), stats: Stats::default(), startup: Vec::new() };
        let player = FormId::PLAYER_REF.0;
        f.users.insert(0x0010_6192, vec![(0, 0x0010_4F08)]);
        f.users.insert(0x0010_634A, vec![(2, 0x0010_4C80)]);
        f.users.insert(0x0017_6ACE, vec![(0, player)]);
        // Easy Pete is still on his chair; the settler's visual went away; the player's
        // use is the player's module's.
        let live = std::collections::HashSet::from([0x0010_4C80]);
        assert_eq!(f.forget_gone(&live), vec![(0x0010_6192, 0x0010_4F08)]);
        assert!(!f.users.contains_key(&0x0010_6192));
        assert_eq!(f.users[&0x0010_634A], vec![(2, 0x0010_4C80)]);
        assert_eq!(f.users[&0x0017_6ACE], vec![(0, player)]);
        assert!(f.forget_gone(&live).is_empty());
    }

    #[test]
    fn furniture_references_are_hex_form_ids() {
        assert_eq!(hex("0010634A"), Some(0x0010_634A));
        assert_eq!(hex(" 0x10634a "), Some(0x0010_634A));
        assert_eq!(hex("EasyPeteChairRef"), None);
    }

    #[test]
    fn headings_turn_the_short_way() {
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(close(lerp_heading(0.0, FRAC_PI_2, 0.5), FRAC_PI_2 / 2.0));
        // From just west of north to just east of it: through north, not round the south.
        assert!(close(lerp_heading(TAU - 0.2, 0.2, 0.5).rem_euclid(TAU), 0.0) || close(lerp_heading(TAU - 0.2, 0.2, 0.5), TAU));
        assert!(close(lerp_heading(0.3, 0.3 + PI - 0.01, 1.0), 0.3 + PI - 0.01));
    }

    #[test]
    fn a_crucified_actor_at_its_cross_is_found_on_it() {
        // Anders stands 8 units from his cross's marker: put on it, no steps.
        assert!(found_on_cross(fnv_ai::furniture::CRUCIFIX_MARKER, 8.0));
        // Walked there from further away, or any other furniture: the steps and the clip.
        assert!(!found_on_cross(fnv_ai::furniture::CRUCIFIX_MARKER, STEP_MIN + 1.0));
        assert!(!found_on_cross(14, 8.0));
    }

    #[test]
    fn a_root_transform_faces_its_heading() {
        for h in [0.0, 0.7, FRAC_PI_2, 2.5] {
            let t = Transform::from_rotation(rotation(h));
            assert!((heading_of(&t) - h).abs() < 1e-4, "{h}");
        }
        // Game space and the world root's space.
        assert_eq!(to_game(Vec3::new(1.0, 3.0, -2.0)), Vec3::new(1.0, 2.0, 3.0));
    }
}
