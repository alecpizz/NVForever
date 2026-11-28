//! Co-op replication, the test build's engine side (docs/design/coop-replication.md):
//! the host's world is the one world, and each guest's game draws it.
//!
//! - **Change log (§6.1, phase 2).** The host's `fnv_sim::World` keeps the references
//!   written to (`World::set_tracking`, `take_touched`). Every [`TICK`] the host reads
//!   the components of the touched ones (and every [`SWEEP`] of all it holds near each
//!   guest, so a reference coming into a guest's area goes too), and sends each guest
//!   what differs from what it last sent that guest ([`nvf_coop::repl::Shadow`]), on the
//!   reliable stream. A value still as the plugins place it is not sent: both games
//!   have it.
//! - **Desync detector.** Every [`DIGEST_EVERY`] a guest hashes what it holds of the
//!   references it has made in its area; the host compares with its own, logs each one
//!   that differs (`replication desync:`) and sends it whole again. That also takes
//!   back what the guest's own save had changed there.
//! - **Looks (phase 1).** For every actor the host draws: its base, the level and rolls of
//!   its leveled picks, and the items it wears and holds (`World::wear_as` on the guest).
//!   Worn items come from the actor's inventory, rolled with the world's sequential
//!   random numbers, so two games disagree on them; they are sent.
//! - **Actors (phase 3).** The moving actors go on the compact stream, 18 bytes each,
//!   at 20, 10 or 3 Hz by distance and view, only when their pose changed (and once a
//!   second while they stand), in place of the old `Npcs` message.
//! - **Loot stays per-player [design].** Container and corpse contents are not
//!   sent, and a loose item one player takes stays for the others (its `deleted` flag is
//!   not replicated nor compared).
//! - **Players' drops are shared [design].** An item a player
//!   drops (`RefState::player_drop`) has one copy, in the host's world: every player sees
//!   it ([`Value::Item`]) and its `deleted` flag goes to all, so whoever takes it takes it
//!   for everyone. A guest's drop goes to the host once it comes to rest
//!   ([`Loose::Dropped`]); the host makes it and tells the guest its number
//!   ([`Loose::Made`]), and the guest's own copy takes that number. A guest's pickup is
//!   its own game's at once and a request to the host ([`Loose::Took`]); the host's world
//!   decides who was first, and a guest that was not gives the item back
//!   ([`Loose::Refused`]), so nothing is doubled. The host's save keeps them.
//! - **The host checks the guests [design].** A guest's carry, drop and pickup and its
//!   digest's resends are checked against the host's world first (`super::checks`): a
//!   carry of a loaded loose object in the guest's space, a step at a time; a drop in
//!   the guest's space, at most [`super::checks::DROPS_KEPT`] of a guest's lying at once
//!   (the oldest goes); a pickup of a player's drop within reach (a refused one is given
//!   back, as a late one is); at most [`super::checks::FORCED_PER_SECOND`] resends a
//!   second. A refusal is counted and logged (`co-op check:`), the guest is marked on the
//!   link (`Link::note_bad`), and the message dropped. A refused drop gets no answer: the
//!   guest keeps its local copy, its own and not the world's [design: `Loose::Refused`
//!   gives a pickup back by taking it out of the guest's inventory, which a drop is not in,
//!   so it would take other items].
//! - **Players' acts relayed after the checks [design].** A guest's door, unlock and
//!   revive of another guest reach the host alone; the host passes each on
//!   (`Link::relay_door`, `relay_unlock`, `relay_revive`) once its check passes
//!   (`super::take_events`).
//! - **Perf.** Every [`PERF_EVERY`] a `replication perf:` line on each side.
//!
//! Solo: every system returns at once without a lobby (R1).

use super::*;
use nvf_coop::repl::{self, comp, ActorPose, Change, Digest, Effect, Loose, PoseBone, Shadow, Value};
use std::collections::BTreeSet;
use std::time::Instant;
use bevy::text::FontSize;

/// A script's move of the host to where it was just moved (within [`SUMMON_NEAR`] units,
/// within this many seconds) takes no guest again.
const SUMMON_SAME: f32 = 2.0;
const SUMMON_NEAR: f32 = 64.0;

/// Whether two summons go to the same place.
fn same_summon(a: &Effect, b: &Effect) -> bool {
    match (a, b) {
        (Effect::Summon { interior: i1, world: w1, pos: p1, .. }, Effect::Summon { interior: i2, world: w2, pos: p2, .. }) => {
            i1 == i2 && w1 == w2 && ((p1[0] - p2[0]).powi(2) + (p1[1] - p2[1]).powi(2) + (p1[2] - p2[2]).powi(2)).sqrt() <= SUMMON_NEAR
        }
        _ => false,
    }
}

/// Whether a script's move `since` seconds after `last` is the same trip again.
fn again(since: f32, last: &Effect, now: &Effect) -> bool {
    since < SUMMON_SAME && same_summon(last, now)
}

/// How often the host sends changes and actor poses.
const TICK: f32 = 0.05;
/// How often the host goes over every reference it holds near each guest.
const SWEEP: f32 = 1.0;
/// How often a guest sends its digest.
const DIGEST_EVERY: f32 = 3.0;
/// A reference changed this recently is in flight, not out of step (`Repl::sent_at`)
/// [design: two digests].
const SETTLE: f32 = 2.0 * DIGEST_EVERY;
/// How often each side writes its perf line.
const PERF_EVERY: f32 = 5.0;
/// A guest's area: its interior cell, or this far around it outdoors (game units). The
/// leash keeps it within 6000 of the host.
const REACH: f32 = 9000.0;
/// A standing actor's pose goes again this often, so the guest keeps it.
const KEEP_ALIVE: f32 = 1.0;
/// How often a guest sends the pose of what it carries (seconds).
const CARRY_EVERY: f32 = 0.05;
/// A dropped or thrown object is followed at most this long before its last pose goes.
const CARRY_FALL_MAX: f32 = 10.0;
/// The most a reliable item takes (under `rel::ITEM_MAX`).
const ITEM: usize = 1000;
/// Env var: the host's numbers in the window's corner.
pub const HUD_ENV: &str = "NVF_REPLICATION_HUD";
/// Env var (a test aid): the perf line also without a lobby, the solo baseline the
/// host's numbers are compared with. Without it solo runs none of this module.
pub const PERF_ENV: &str = "NVF_REPLICATION_PERF";

fn solo_perf() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var(PERF_ENV).is_ok_and(|v| v == "1"))
}

impl Repl {
    /// A guest's digest goes on its next frame (the syncing screen's ask follows it on
    /// the same stream, so what its own save changed in its area is sent back first).
    pub(super) fn digest_now(&mut self) {
        self.last_digest = f32::MIN;
    }
}

#[derive(Default)]
pub(super) struct Repl {
    /// A guest's: where the host's script moved the host's player (interior cell or 0,
    /// worldspace, feet, yaw): this guest goes there too (`coop::leash`).
    pub summon: Option<(u32, u32, [f32; 3], f32)>,
    /// The host's: the friendly-fire rule as last sent to the guests (`None`: send it).
    rules_sent: Option<bool>,
    /// The host's: guests whose stream started again, to be told the animation group
    /// each reference was last asked to play (a rocket bay a script opened before they
    /// joined stays open for them).
    anims_for: Vec<u8>,
    /// The host's: where its player stood at the end of the last frame, so a script's move
    /// takes along only the guests that were within sight of it ([`fnv_game::coop::STORY_NEAR`]).
    host_was: Option<(Place, [f32; 3])>,
    /// The host's: the last script's move that took no guest along, and when: the same
    /// move again within [`SUMMON_SAME`] is the same trip (a script moved the host 13
    /// times in one second, each logged "player 2 too far" otherwise) [observed: buckallobill, in game, FNV 1.4.0.525].
    left_behind: Option<(f32, Effect)>,
    /// The host's: body parts gone here, for the guests ([`dismember_out`]); a guest's:
    /// those the host sent, to take off here ([`dismember_in`]).
    dismembered: Vec<Effect>,
    /// The host's: guests that asked for the syncing screen's answer (`Effect::Sync`),
    /// with their serial: their whole area goes again at the next sweep, then the answer.
    sync_asked: Vec<(u8, u32)>,
    /// The host's: the last summon each guest was sent, and when ([`same_summon`]).
    summoned: HashMap<u8, (f32, Effect)>,
    /// A guest's: the serial of the host's last answer to its `Effect::Sync`.
    pub sync_done: Option<u32>,
    /// A guest's: when the stream last set each reference's enabled flag. The host's
    /// presence list, made up to two seconds before and paged, does not undo it
    /// (`coop::STREAM_WINS`: references enabled by the stream at a join were disabled
    /// again by an older list for 6 s otherwise [observed: buckallobill, in game, FNV 1.4.0.525]).
    pub enabled_heard: HashMap<u32, f32>,
    /// The host's: the bodies at rest here, as [`Value::Pose`] by reference
    /// ([`corpses_out`]); a guest's: those the host sent, to lay down here ([`corpses_in`]).
    host_rest: HashMap<u32, Value>,
    poses_in: Vec<(u32, Vec<Option<PoseBone>>)>,
    /// A guest's: references the host enabled or disabled that this game changed too, for
    /// their visuals and collision to follow ([`nvf_play::RefChanged`]).
    refreshed: Vec<u32>,
    // The host's.
    shadows: HashMap<u8, Shadow>,
    /// References to send whole to a guest (a digest differed, or the stream started again).
    forced: HashMap<u8, BTreeSet<u32>>,
    /// The host's: when each reference's last change went to each guest. A digest is
    /// late by up to [`DIGEST_EVERY`] and the trip, so a reference that moves all the
    /// time (a tumbleweed in the wind) always differs from the host's copy by then; one
    /// changed within [`SETTLE`] is not a desync (two tumbleweeds were sent whole again
    /// every digest otherwise, a guest's corrections matching its changes [observed: buckallobill, in game, FNV 1.4.0.525]).
    sent_at: HashMap<u8, HashMap<u32, f32>>,
    dirty: BTreeSet<FormId>,
    last_tick: f32,
    last_sweep: f32,
    /// The NPCs `share_npcs` found this frame, for the actor stream.
    pub host_npcs: Vec<NpcState>,
    /// Each guest's poses as last sent: when, and the pose.
    poses: HashMap<u8, HashMap<u32, (f32, ActorPose)>>,
    /// What each guest's area held at the last sweep.
    interest: HashMap<u8, usize>,
    changes_sent: HashMap<u8, u64>,
    desyncs: u64,
    /// The host's: guests whose stream started again and whose first digest has not come.
    /// That digest names what the guest's own save holds unlike the host's world (its
    /// syncing screen sends it before its ask, `syncing`): those go back to it as
    /// corrections, not desyncs.
    starting: BTreeSet<u8>,
    /// The host's: until when (seconds) each guest's first digest round lasts. A digest
    /// goes in items of up to 124 references, so the round, not the first item, is what
    /// is "starting" [design].
    start_round: std::collections::BTreeMap<u8, f32>,
    /// The host's: references a first digest found out of step past the resend cap of its
    /// second, kept to be sent in the next seconds and not counted against the guest.
    start_backlog: std::collections::BTreeMap<u8, std::collections::VecDeque<u32>>,
    /// The host's: what those first digests sent back.
    start_fixes: u64,
    effects_sent: u64,
    // The guest's.
    pub inbox: Vec<Event>,
    /// The guest's: its inbox waits for its own game to be kept (`replicate`), told once.
    held_for_solo: bool,
    /// Per actor: what its Look, Anim and Life said.
    slow: HashMap<u32, Slow>,
    last_digest: f32,
    applied: u64,
    corrections: u64,
    /// The guest's: corrections since the last perf line, by reference and component,
    /// so a run of them names what the host keeps changing.
    corrected: HashMap<(u32, u8), u64>,
    poses_heard: u64,
    /// The lowest (here - host) clock difference seen, and the newest round's.
    lag_floor: Option<i64>,
    lag: Vec<f32>,
    effects_played: u64,
    /// A guest's own dropped items not yet at rest: since when, where it
    /// was last seen and since when it has lain there; and those sent to the host.
    drops_waiting: HashMap<u32, (f32, [f32; 3], f32)>,
    drops_sent: BTreeSet<u32>,
    /// A guest's copies of the players' dropped items in the host's world (base, count),
    /// and those the host's world took away (its own pickup or another player's).
    shared: HashMap<u32, (u32, i32)>,
    host_took: BTreeSet<u32>,
    /// Players' drops made, taken and refused (both sides).
    loose: u64,
    /// The object this guest's physics moves (held, or falling after it let go), the
    /// time it was let go, and when its pose last went to the host.
    carrying: Option<(u32, Option<f32>)>,
    last_carry: f32,
    carries_sent: u64,
    // The host's: guests' carried objects put here.
    carries_heard: u64,
    // Both.
    perf: Perf,
}

#[derive(Default, Clone)]
struct Slow {
    base: u32,
    level: u16,
    all_levels: bool,
    rolls: u8,
    furniture: u32,
    weapon: u32,
    dead: bool,
    hostile: bool,
    /// Its interior cell or worldspace in the host's world (`net::NpcState::space`).
    space: u32,
}

#[derive(Default)]
struct Perf {
    since: Option<f32>,
    frames: Vec<f32>,
    /// Milliseconds spent in replication, per frame.
    spent: Vec<f32>,
    traffic: HashMap<u8, net::Traffic>,
}

impl Perf {
    fn add_spent(&mut self, ms: f32) {
        if let Some(last) = self.spent.last_mut() {
            *last += ms;
        }
    }
}

/// Every frame: the frame time, and a new slot for the replication time spent in it.
pub(super) fn frame_clock(mut coop: ResMut<Coop>, real: Res<Time<Real>>) {
    if coop.link.is_none() && !solo_perf() {
        return;
    }
    let p = &mut coop.repl.perf;
    p.frames.push(real.delta_secs() * 1000.0);
    p.spent.push(0.0);
}

fn percentile(v: &[f32], q: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(f32::total_cmp);
    s[((s.len() - 1) as f32 * q).round() as usize]
}

fn average(v: &[f32]) -> f32 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f32>() / v.len() as f32
    }
}

/// The values of reference `r` that every game must agree on (no actor's Look, Life or
/// Anim: those come from the drawn actors): from its state, or its placement when it has
/// none. `None` for a reference neither has.
fn world_values(world: &fnv_sim::World, r: FormId) -> Option<Vec<Value>> {
    let placed = world.placed.get(r);
    let (base, st) = match world.peek(r) {
        Some(st) => (st.base, Some(st)),
        None => (placed?.base, None),
    };
    let data = world.forms.get(base).map(|b| &b.data);
    let actor = matches!(data, Some(fnv_sim::forms::BaseData::Actor(_)));
    let item = matches!(data, Some(fnv_sim::forms::BaseData::Item(_)));
    let mut out = Vec::with_capacity(5);
    let (enabled, deleted) = match st {
        Some(s) => (s.enabled(), s.deleted()),
        None => (!placed.is_some_and(|p| p.initially_disabled()), false),
    };
    // A player's dropped item is made first on a guest that has not got it (module notes).
    let shared = item && r.is_dynamic() && st.is_some_and(|s| s.player_drop);
    if let (true, Some(s)) = (shared, st) {
        out.push(Value::Item { base: base.0, count: s.count, cell: s.cell.map_or(0, |c| c.0), pos: s.pos, rot: s.rot });
    }
    // A loose item one player took is still there for the others (per-player loot), but
    // not one a player dropped.
    out.push(Value::Enabled { enabled, deleted: deleted && (!item || shared) });
    if !actor {
        let (cell, pos, rot, scale) = match st {
            Some(s) => (s.cell, s.pos, s.rot, s.scale),
            None => {
                let p = placed?;
                (p.cell, p.pos, p.rot, None)
            }
        };
        out.push(Value::Place { cell: cell.map_or(0, |c| c.0), pos, rot, scale: scale.unwrap_or(0.0) });
        if r.is_dynamic() && !item && !matches!(data, Some(fnv_sim::forms::BaseData::Projectile { .. })) {
            out.push(Value::Exists { base: base.0, cell: cell.map_or(0, |c| c.0), pos, rot });
        }
    }
    if placed.is_some_and(|p| p.teleport.is_none()) && matches!(data, Some(fnv_sim::forms::BaseData::Door(_))) {
        out.push(Value::Open(st.map_or(placed.is_some_and(|p| p.open_by_default), |s| s.open)));
    }
    if let Some(l) = world.lock_of(r) {
        out.push(Value::Lock { has: true, locked: l.locked, level: l.level });
    } else if placed.is_some_and(|p| p.lock.is_some()) {
        out.push(Value::Lock { has: false, locked: false, level: 0 });
    }
    Some(out)
}

/// Whether `v` is what the plugins give `r` (both games have it without being told).
fn is_authored(world: &fnv_sim::World, r: FormId, v: &Value) -> bool {
    let Some(p) = world.placed.get(r) else { return false };
    match v {
        Value::Enabled { enabled, deleted } => *enabled == !p.initially_disabled() && !*deleted,
        Value::Place { cell, pos, rot, scale } => *cell == p.cell.map_or(0, |c| c.0) && *pos == p.pos && *rot == p.rot && *scale == 0.0,
        Value::Open(o) => *o == p.open_by_default,
        Value::Lock { has, locked, .. } => *has == p.lock.is_some() && (*locked || !*has),
        _ => false,
    }
}

/// Whether reference `r` is in the area of a player at `place`, `feet`.
fn in_area(world: &fnv_sim::World, r: FormId, place: Place, feet: [f32; 3]) -> bool {
    let (cell, pos) = match world.peek(r) {
        Some(s) => (s.cell, s.pos),
        None => match world.placed.get(r) {
            Some(p) => (p.cell, p.pos),
            None => return false,
        },
    };
    match place {
        Place::Interior(c) => cell.is_some_and(|x| x.0 == c),
        Place::Exterior(w) => cell.and_then(|c| world.placed.world_of(c)).is_some_and(|x| x.0 == w) && (pos[0] - feet[0]).hypot(pos[1] - feet[1]) <= REACH,
    }
}

/// One guest's changes this tick: each reference of `area` (on a sweep, the host's
/// changed references in the guest's area; else those touched since the last tick) whose
/// values the guest was not sent, and, whole, every reference a digest found out of step
/// (`forced`), among them or not. A value the plugins give the reference goes only when
/// forced or once sent otherwise (both games have it).
///
/// A forced reference the host itself never changed (a book the guest's own save, or its
/// own physics, put elsewhere) is not among a sweep's. It went nowhere when the resend
/// fell on a sweep, as it always does after a syncing screen's ask, and the next digests
/// found it out of step again [observed: buckallobill, in game, FNV 1.4.0.525: a third
/// player's three books flagged twice, 3 s apart].
fn guest_changes(shadow: &mut Shadow, area: &[FormId], forced: &BTreeSet<u32>, values: impl Fn(FormId) -> Option<Vec<Value>>, authored: impl Fn(FormId, &Value) -> bool) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut consider = |rf: FormId, force: bool| {
        let Some(vals) = values(rf) else { return };
        for v in vals {
            let new = shadow.of(rf.0).all(|s| s.comp() != v.comp());
            if !force && new && authored(rf, &v) {
                continue;
            }
            if shadow.take(rf.0, &v) || force {
                changes.push(Change { reference: rf.0, value: v });
            }
        }
    };
    for &rf in area {
        consider(rf, forced.contains(&rf.0));
    }
    let named: BTreeSet<u32> = area.iter().map(|r| r.0).collect();
    for &f in forced.iter().filter(|f| !named.contains(f)) {
        consider(FormId(f), true);
    }
    changes
}

/// The references of a guest's digest (`refs`: reference, digest) it holds unlike the
/// host, with the host's values: not one the host sent it within [`SETTLE`] (in flight),
/// nor one the host has no values for.
fn out_of_step(refs: &[(u32, u32)], sent_at: &HashMap<u32, f32>, now: f32, values: impl Fn(FormId) -> Option<Vec<Value>>) -> Vec<(u32, Vec<Value>)> {
    refs.iter()
        .filter(|(reference, _)| !sent_at.get(reference).is_some_and(|&t| now - t < SETTLE))
        .filter_map(|&(reference, theirs)| values(FormId(reference)).filter(|mine| repl::digest_of(mine) != theirs).map(|mine| (reference, mine)))
        .collect()
}

/// How long a guest's first digest round lasts after its first item (seconds): every
/// item of one digest goes in one burst, so this is well under [`DIGEST_EVERY`].
const START_ROUND: f32 = 1.0;
/// References kept at most per guest from a first digest, to send past the resend cap.
const START_BACKLOG_MAX: usize = 4096;

/// One of a guest's digest items, as the host judges it: what the guest holds unlike the
/// host's world goes to it again (corrections in its first digest round, desyncs after),
/// at most [`checks::FORCED_PER_SECOND`] a second. The excess of a first round waits in
/// `start_backlog` (an honest guest's save may differ in many places); the excess of any
/// other digest is a bad message.
fn host_digest(r: &mut Repl, host_checks: &mut checks::HostChecks, id: u8, d: &Digest, now: f32, values: impl Fn(FormId) -> Option<Vec<Value>>, name: impl Fn(u32) -> String) {
    if r.starting.remove(&id) {
        r.start_round.insert(id, now + START_ROUND);
    }
    let starting = r.start_round.get(&id).is_some_and(|&end| now < end);
    let sent_at = r.sent_at.entry(id).or_default();
    let found = out_of_step(&d.refs, sent_at, now, &values);
    let mut found = found.into_iter();
    while let Some((reference, mine)) = found.next() {
        // A digest naming every reference is no flood.
        if !host_checks.force(id, now) {
            if starting {
                let q = r.start_backlog.entry(id).or_default();
                for rf in std::iter::once(reference).chain(found.by_ref().map(|(rf, _)| rf)) {
                    if q.len() < START_BACKLOG_MAX {
                        q.push_back(rf);
                    }
                }
            } else {
                super::refused_by(host_checks, id, checks::why::DIGEST_RESENDS, now);
            }
            break;
        }
        force_ref(r, id, reference, &mine, starting, &name);
    }
}

/// Send `reference` again to guest `id` (`mine`: the host's values for it).
fn force_ref(r: &mut Repl, id: u8, reference: u32, mine: &[Value], starting: bool, name: &impl Fn(u32) -> String) {
    let rf = FormId(reference);
    let name = name(reference);
    let shadow = r.shadows.entry(id).or_default();
    if starting {
        r.start_fixes += 1;
        info!("replication: player {}'s own game holds {name} {rf} unlike the host: sent", id + 1);
    } else {
        r.desyncs += 1;
        let what: Vec<String> = mine.iter().filter(|v| !shadow.of(reference).any(|s| s == *v)).map(|v| format!("{v:?}")).collect();
        info!("replication desync: player {} holds {name} {rf} unlike the host ({}): sent again", id + 1, if what.is_empty() { "its own change".to_string() } else { what.join(", ") });
    }
    shadow.forget(reference);
    r.forced.entry(id).or_default().insert(reference);
}

/// Send what a first digest round left over, as the cap allows each second.
fn drain_start_backlog(r: &mut Repl, host_checks: &mut checks::HostChecks, now: f32, values: impl Fn(FormId) -> Option<Vec<Value>>, name: impl Fn(u32) -> String) {
    let ids: Vec<u8> = r.start_backlog.keys().copied().collect();
    for id in ids {
        while let Some(&reference) = r.start_backlog.get(&id).and_then(|q| q.front()) {
            if !host_checks.force(id, now) {
                break;
            }
            r.start_backlog.get_mut(&id).and_then(|q| q.pop_front());
            // The host's values now, not those of the round: what has not changed since
            // the guest's last word is already in step.
            if let Some(mine) = values(FormId(reference)) {
                force_ref(r, id, reference, &mine, true, &name);
            }
        }
        if r.start_backlog.get(&id).is_some_and(|q| q.is_empty()) {
            r.start_backlog.remove(&id);
        }
    }
}

/// The host's side and the guest's, each frame (module notes).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn replicate(
    mut commands: Commands,
    time: Res<Time>,
    mut coop: ResMut<Coop>,
    (mut sim, mut script, actor_world, mut residents): (Option<ResMut<nvf_play::sim::SimState>>, Option<ResMut<nvf_play::script::ScriptState>>, Option<Res<ActorWorld>>, Option<ResMut<Residents>>),
    loc: Option<Res<Location>>,
    player: Option<Res<Player>>,
    (grab, mut phys, mut repose, mut changed): (Option<Res<nvf_play::grab::GrabState>>, Option<ResMut<nvf_play::phys::PhysState>>, MessageWriter<nvf_play::RefRepose>, MessageWriter<nvf_play::RefChanged>),
    solo: Option<Res<nvf_play::save::SoloBase>>,
) {
    let started = Instant::now();
    let now = time.elapsed_secs();
    let coop = &mut *coop;
    let Some(link) = coop.link.as_ref() else {
        if let Some(s) = sim.as_deref_mut() {
            s.world.set_tracking(false);
        }
        if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
            rt.game.coop_effects = None;
        }
        if !coop.repl.shadows.is_empty() || !coop.repl.slow.is_empty() {
            coop.repl = Repl::default();
        }
        if let Some(p) = phys.as_deref_mut().filter(|p| !p.follow_only.is_empty()) {
            p.follow_only.clear();
        }
        if solo_perf() {
            perf_line(coop, now);
        }
        return;
    };
    let host = link.is_host();
    let Some(sim) = sim.as_deref_mut() else { return };
    // What this game's physics carries goes to the other players: a guest's to the host,
    // the host's to every guest.
    send_carry(coop, grab.as_deref(), phys.as_deref(), now);
    let moved = if host {
        host_side(coop, sim, script.as_deref_mut(), actor_world.as_deref(), phys.as_deref(), now)
    } else if solo.is_none() {
        // Not before this game is kept as the guest's own (`nvf_play::save::SoloBase`,
        // coop.rs module notes): what the host's stream changes here (an NPC dressed as the
        // host has it, `World::wear_as`) would be kept in it, and every save this guest
        // makes is that game. An NPC the host had dead and searched was undressed here
        // (`wear_as` with no items) before the game was kept, and stood in his underwear
        // when that save was loaded solo [observed: buckallobill, in game, FNV 1.4.0.525]. The stream waits in the inbox meanwhile.
        if !coop.repl.held_for_solo && !coop.repl.inbox.is_empty() {
            coop.repl.held_for_solo = true;
            info!("replication: the host's stream waits until this game is kept as the guest's own");
        }
        Vec::new()
    } else {
        if std::mem::take(&mut coop.repl.held_for_solo) {
            info!("replication: this game is kept as the guest's own; the host's stream ({} messages waiting) is applied", coop.repl.inbox.len());
        }
        guest_side(&mut commands, coop, sim, script.as_deref_mut(), residents.as_deref_mut(), (loc.as_deref(), player.as_deref(), phys.as_deref()), now)
    };
    // The guests the checks refused here are marked on the link.
    super::note_bad(coop);
    // A guest's copies of the objects the host's world moves follow the host: this
    // game's physics does not write them back (`PhysState::follow_only`), or its own
    // simulation of a rolling object would differ from the host's and the desync
    // detector would send it again every digest [observed: buckallobill, in game, FNV 1.4.0.525]. What this guest
    // carries is its own until it is put down.
    if let Some(p) = phys.as_deref_mut() {
        if host {
            if !p.follow_only.is_empty() {
                p.follow_only.clear();
            }
        } else {
            p.follow_only.extend(moved.iter().copied());
            if let Some((id, _)) = coop.repl.carrying {
                p.follow_only.remove(&id);
            }
        }
    }
    for id in moved {
        repose.write(nvf_play::RefRepose { form_id: id });
    }
    // The host's enables and disables of what is not an actor (the actors' drawing
    // follows their flags itself): a wreck disabled on the host stayed drawn on a
    // guest whose world had it disabled otherwise [observed: buckallobill, in game, FNV 1.4.0.525].
    // Their enable children with them (bottles that follow a marker: the stream enabled
    // the marker, the bottles were drawn with no collision body, and a guest's shots went
    // through them [observed: buckallobill, in game, FNV 1.4.0.525]).
    let refreshed = std::mem::take(&mut coop.repl.refreshed);
    if !refreshed.is_empty() {
        for r in super::with_enable_children(sim.refs, refreshed.into_iter().map(FormId).collect()) {
            changed.write(nvf_play::RefChanged::new(r.0));
        }
    }
    let ms = started.elapsed().as_secs_f32() * 1000.0;
    coop.repl.perf.add_spent(ms);
    perf_line(coop, now);
}

/// A guest: the pose of the object its physics moves goes to the host while held, and
/// after it lets go until the body sleeps (a throw, a drop), so the host's game moves its
/// own copy (the host is the world's authority; nothing else tells it).
fn send_carry(coop: &mut Coop, grab: Option<&nvf_play::grab::GrabState>, phys: Option<&nvf_play::phys::PhysState>, now: f32) {
    let (Some(link), Some(phys)) = (coop.link.as_ref(), phys) else { return };
    let r = &mut coop.repl;
    let held = grab.and_then(|g| g.held.as_ref()).and_then(|h| match h.on {
        fnv_phys::GripOn::Ref(id) => Some(id),
        fnv_phys::GripOn::Ragdoll(_) => None,
    });
    match (held, r.carrying) {
        (Some(id), Some((was, _))) if id == was => r.carrying = Some((id, None)),
        (Some(id), _) => {
            info!("replication: carrying {id:#010X}: its pose goes to the other players");
            r.carrying = Some((id, None));
        }
        (None, Some((id, None))) => r.carrying = Some((id, Some(now))),
        _ => {}
    }
    let Some((id, let_go)) = r.carrying else { return };
    if now - r.last_carry < CARRY_EVERY {
        return;
    }
    r.last_carry = now;
    let Some((pos, rot, sleeping)) = phys.ref_pose_now(id) else {
        r.carrying = None;
        return;
    };
    let done = let_go.is_some_and(|t| sleeping || now - t >= CARRY_FALL_MAX);
    // A full stream refuses it; the stream starts again and resends the whole state.
    let _ = link.send_reliable(None, repl::pack_carry(&repl::Carry { reference: id, pos, rot, done }));
    r.carries_sent += 1;
    if done {
        info!("replication: {id:#010X} at rest at {pos:?}: its last pose went to the other players");
        r.carrying = None;
    }
}

/// The host's side; the references a guest's carry moved (their collision follows).
fn host_side(coop: &mut Coop, sim: &mut nvf_play::sim::SimState, mut script: Option<&mut nvf_play::script::ScriptState>, aw: Option<&ActorWorld>, phys: Option<&nvf_play::phys::PhysState>, now: f32) -> Vec<u32> {
    let mut moved = Vec::new();
    let Some(link) = coop.link.as_ref() else { return moved };
    let r = &mut coop.repl;
    let host_checks = &mut coop.checks;
    let guests: Vec<(u8, Place, [f32; 3], f32)> = coop.peers.iter().filter(|(_, p)| !p.lost).filter_map(|(&id, p)| p.samples.back().map(|(_, s)| (id, s.place, s.feet, s.yaw))).collect();
    let game = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| &mut rt.game);
    if guests.is_empty() {
        sim.world.set_tracking(false);
        r.dirty.clear();
        if let Some(g) = game {
            g.coop_effects = None;
        }
        return moved;
    }
    sim.world.set_tracking(true);
    // The script effects the host's scripts asked for go to every guest (§7, phase 4).
    let mut summons: Vec<(Effect, Vec<u8>)> = Vec::new();
    if let Some(g) = game {
        let asked = std::mem::take(g.coop_effects.get_or_insert_with(Vec::new));
        let mut effects: Vec<Effect> = asked
            .into_iter()
            .filter_map(|e| match e {
                fnv_game::sound::CoopEffect::Sound { sound, at } => Some(Effect::Sound { sound: sound.0, at_ref: at.0, pos: [0.0; 3] }),
                fnv_game::sound::CoopEffect::Music(m) => Some(Effect::Music { music: m.0 }),
                fnv_game::sound::CoopEffect::Anim { on, group, flag } => Some(Effect::Anim { on: on.0, group, flag: flag.clamp(0, 2) as u8, settled: false }),
                fnv_game::sound::CoopEffect::AnimReset(on) => Some(Effect::AnimReset { on: on.0 }),
                fnv_game::sound::CoopEffect::PlayerMoved => {
                    // Where the script put the host's player.
                    let s = sim.world.peek(FormId::PLAYER_REF)?;
                    let cell = s.cell?;
                    let summon = match sim.world.placed.world_of(cell) {
                        Some(w) => Effect::Summon { interior: 0, world: w.0, pos: s.pos, yaw: s.rot[2] },
                        None => Effect::Summon { interior: cell.0, world: 0, pos: s.pos, yaw: s.rot[2] },
                    };
                    // A script that moves the host again and again to the same spot (14
                    // moves in a second) is one trip for the guests: one summon, one
                    // loading screen.
                    if !r.summoned.is_empty() && r.summoned.values().all(|(at, last)| now - at < SUMMON_SAME && same_summon(last, &summon)) {
                        return None;
                    }
                    if r.left_behind.as_ref().is_some_and(|(at, last)| again(now - at, last, &summon)) {
                        return None;
                    }
                    // Only the guests within sight of where the host stood go along
                    // [design: a cutscene pulls in the players within sight].
                    let (near, far) = near_guests(r.host_was, &guests);
                    let names = |ids: &[u8]| ids.iter().map(|i| format!("player {}", i + 1)).collect::<Vec<_>>().join(", ");
                    info!(
                        "replication: a script moved the host to {cell} {:?}: {} go with it{}",
                        s.pos,
                        if near.is_empty() { "no guest within sight".to_string() } else { names(&near) },
                        if far.is_empty() { String::new() } else { format!("; {} too far, left where they are", names(&far)) }
                    );
                    r.left_behind = near.is_empty().then(|| (now, summon.clone()));
                    if !near.is_empty() {
                        summons.push((summon, near));
                    }
                    None
                }
            })
            .collect();
        // The lobby's rules: when they change, and again for a stream started anew.
        if r.rules_sent != Some(coop.friendly_fire) {
            r.rules_sent = Some(coop.friendly_fire);
            info!("replication: the rules go to the guests: friendly fire {}", if coop.friendly_fire { "on" } else { "off" });
            effects.push(Effect::Rules { friendly_fire: coop.friendly_fire });
        }
        // The body parts gone on the host.
        effects.append(&mut r.dismembered);
        if !effects.is_empty() {
            r.effects_sent += effects.len() as u64;
            for item in repl::pack_effects(&effects, ITEM) {
                // A full stream refuses it; the stream starts again and resends the whole state.
                let _ = link.send_reliable(None, item);
            }
        }
        // A guest whose stream started again: what the host's scripts last asked each
        // reference to play, shown on its copy as a draw shows it.
        let asked: Vec<Effect> = if r.anims_for.is_empty() {
            Vec::new()
        } else {
            g.anims.asked_all().iter().map(|(on, (group, _))| Effect::Anim { on: on.0, group: *group, flag: 0, settled: true }).collect()
        };
        for id in std::mem::take(&mut r.anims_for) {
            if asked.is_empty() {
                info!("replication: player {} is told the 0 animation groups the host's scripts last asked (none asked yet)", id + 1);
            } else {
                info!("replication: player {} is told the {} animation groups the host's scripts last asked", id + 1, asked.len());
                r.effects_sent += asked.len() as u64;
                for item in repl::pack_effects(&asked, ITEM) {
                    // A full stream refuses it; the stream starts again and resends the whole state.
                    let _ = link.send_reliable(Some(id), item);
                }
            }
        }
        for (summon, to) in summons {
            r.effects_sent += 1;
            for &id in &to {
                r.summoned.insert(id, (now, summon.clone()));
            }
            for id in to {
                for item in repl::pack_effects(std::slice::from_ref(&summon), ITEM) {
                    // A full stream refuses it; the stream starts again and resends the whole state.
                    let _ = link.send_reliable(Some(id), item);
                }
            }
        }
    }
    r.host_was = sim.world.peek(FormId::PLAYER_REF).and_then(|s| {
        let c = s.cell?;
        Some((match sim.world.placed.world_of(c) {
            Some(w) => Place::Exterior(w.0),
            None => Place::Interior(c.0),
        }, s.pos))
    });
    drain_start_backlog(r, host_checks, now, |rf| world_values(&sim.world, rf), |rf| sim.ref_name(None, rf).unwrap_or("?").to_string());
    for e in std::mem::take(&mut r.inbox) {
        match e {
            Event::RelReset { id } => {
                r.rules_sent = None;
                if !r.anims_for.contains(&id) {
                    r.anims_for.push(id);
                }
                r.shadows.remove(&id);
                r.sent_at.remove(&id);
                r.poses.remove(&id);
                r.starting.insert(id);
                r.start_round.remove(&id);
                r.start_backlog.remove(&id);
                info!("replication: player {} starts again: the whole area goes to it", id + 1);
            }
            Event::Reliable { id, item } => {
                let guest = guests.iter().find(|g| g.0 == id).map(|&(_, place, feet, _)| checks::Guest { space: super::space_of(place), feet });
                if let Some(c) = repl::unpack_carry(&item) {
                    // Checked first (`checks::carry`): a loaded loose object in the
                    // guest's space, moved a step at a time.
                    let verdict = match &guest {
                        Some(g) => checks::carry(&checks::SimWorld { sim, combat: None, phys }, g, &c, host_checks.last_carry(id, c.reference, now)),
                        None => Err(checks::why::CARRY_TARGET),
                    };
                    if let Err(why) = verdict {
                        super::refused_by(host_checks, id, why, now);
                        continue;
                    }
                    host_checks.note_carry(id, c.reference, c.pos, c.done, now);
                    // A guest carries this object: the host's copy goes where the guest
                    // has it; the other guests get it by the change log (the write
                    // touches it); the carrier is not sent its own move back.
                    let rf = FormId(c.reference);
                    let Ok(s) = sim.world.get(rf) else { continue };
                    s.pos = c.pos;
                    s.rot = c.rot;
                    if !moved.contains(&c.reference) {
                        moved.push(c.reference);
                    }
                    r.carries_heard += 1;
                    if c.done {
                        info!("replication: player {} put {} {rf} down at {:?}", id + 1, sim.ref_name(None, rf.0).unwrap_or("?"), c.pos);
                    }
                    if let Some(values) = world_values(&sim.world, rf) {
                        let shadow = r.shadows.entry(id).or_default();
                        for v in values.iter().filter(|v| v.comp() == comp::PLACE) {
                            shadow.take(rf.0, v);
                        }
                    }
                } else if let Some(l) = repl::unpack_loose(&item) {
                    host_loose(link, r, (host_checks, guest.as_ref(), phys), sim, id, l, now);
                } else if let Some(effects) = repl::unpack_effects(&item) {
                    // A guest behind its syncing screen: its whole area again,
                    // at a sweep now, and then the answer.
                    for e in effects {
                        if let Effect::Sync { serial, done: false } = e {
                            info!("replication: player {} asks to be synced ({serial}): its whole area goes again", id + 1);
                            r.shadows.remove(&id);
                            r.sent_at.remove(&id);
                            r.poses.remove(&id);
                            r.sync_asked.retain(|(g, _)| *g != id);
                            r.sync_asked.push((id, serial));
                            r.last_sweep = f32::MIN;
                        }
                    }
                } else if let Some(d) = repl::unpack_digest(&item) {
                    host_digest(r, host_checks, id, &d, now, |rf| world_values(&sim.world, rf), |rf| sim.ref_name(None, rf).unwrap_or("?").to_string());
                }
            }
            _ => {}
        }
    }
    if now - r.last_tick < TICK {
        return moved;
    }
    r.last_tick = now;
    r.dirty.extend(sim.world.take_touched());
    let sweep = now - r.last_sweep >= SWEEP;
    if sweep {
        r.last_sweep = now;
    }
    let dead = |x: FormId| script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.is_dead(x));
    let rolls = aw.and_then(|w| w.rolls.lock().ok().map(|m| m.clone())).unwrap_or_default();
    for &(id, place, feet, yaw) in &guests {
        let shadow = r.shadows.entry(id).or_default();
        let forced = r.forced.remove(&id).unwrap_or_default();
        let area: Vec<FormId> = if sweep {
            let refs: Vec<FormId> = sim.world.changed().map(|(k, _)| k).filter(|&k| k != FormId::PLAYER_REF && in_area(&sim.world, k, place, feet)).collect();
            r.interest.insert(id, refs.len());
            refs
        } else {
            r.dirty.iter().copied().filter(|&k| k != FormId::PLAYER_REF && in_area(&sim.world, k, place, feet)).collect()
        };
        let mut changes = guest_changes(shadow, &area, &forced, |rf| world_values(&sim.world, rf), |rf, v| is_authored(&sim.world, rf, v));
        // The actors the host draws: Life and Anim each tick, Look on a sweep.
        for n in &r.host_npcs {
            let rf = FormId(n.reference);
            let mut vals = vec![Value::Life { dead: n.has(npc_flag::DEAD) || dead(rf) }, Value::Anim { furniture: n.furniture, weapon: n.weapon, hostile: n.hostile, space: n.space }];
            if sweep || shadow.of(n.reference).all(|s| s.comp() != comp::LOOK) {
                let worn = sim.world.equipment(rf).unwrap_or_default().into_iter().map(|w| w.0).collect();
                let level = aw.map_or(fnv_sim::zones::LevelAt::plain(0), |w| w.level(n.reference));
                vals.push(Value::Look { base: n.base, level: level.level, all_levels: level.all_levels, rolls: rolls.get(&n.reference).copied().unwrap_or(0).min(127) as u8, worn });
            }
            // Its body as it lies at rest here ([`corpses_out`]).
            if let Some(p) = r.host_rest.get(&n.reference) {
                vals.push(p.clone());
            }
            for v in vals {
                if shadow.take(n.reference, &v) {
                    changes.push(Change { reference: n.reference, value: v });
                }
            }
        }
        if !changes.is_empty() {
            let sent_at = r.sent_at.entry(id).or_default();
            sent_at.retain(|_, t| now - *t < SETTLE);
            for c in &changes {
                sent_at.insert(c.reference, now);
            }
            *r.changes_sent.entry(id).or_default() += changes.len() as u64;
            for item in repl::pack_changes(&changes, ITEM) {
                // A full stream refuses it; the stream starts again and resends the whole state.
                let _ = link.send_reliable(Some(id), item);
            }
        }
        // The syncing screen's answer, after the sweep's changes on the same stream.
        if sweep {
            if let Some(i) = r.sync_asked.iter().position(|(g, _)| *g == id) {
                let (_, serial) = r.sync_asked.remove(i);
                info!("replication: player {} is synced ({serial}): {} changes of its area sent before the answer", id + 1, changes.len());
                for item in repl::pack_effects(&[Effect::Sync { serial, done: true }], ITEM) {
                    // A full stream refuses it; the stream starts again and resends the whole state.
                    let _ = link.send_reliable(Some(id), item);
                }
            }
        }
        // The compact actor stream.
        let sent = r.poses.entry(id).or_default();
        let mut poses = Vec::new();
        let eye = [feet[0], feet[1], feet[2] + 120.0];
        for n in &r.host_npcs {
            let pose = ActorPose { reference: n.reference, feet: n.feet, heading: n.heading, pitch: n.pitch, flags: n.flags, attacks: n.attacks }.quantised();
            let d = (n.feet[0] - feet[0]).hypot(n.feet[1] - feet[1]);
            let hz = repl::pose_hz(d, repl::in_view(eye, yaw, n.feet));
            let go = match sent.get(&n.reference) {
                None => true,
                Some((at, last)) => {
                    let age = now - at;
                    (age + 1e-3 >= 1.0 / hz && *last != pose) || age >= KEEP_ALIVE
                }
            };
            if go {
                sent.insert(n.reference, (now, pose));
                poses.push(pose);
            }
        }
        sent.retain(|_, (at, _)| now - *at < 10.0);
        if !poses.is_empty() {
            link.send_actors(id, &poses);
        }
    }
    r.dirty.clear();
    moved
}

/// A round's snapshot lag (ms) for the perf line: the (here - host) clock difference over
/// the lowest seen (`floor`), which it lowers. A host started again resets `floor`
/// (`Event::RelReset`).
fn snapshot_lag(floor: &mut Option<i64>, here_ms: u32, host_ms: u32) -> f32 {
    let off = i64::from(here_ms) - i64::from(host_ms);
    let low = floor.map_or(off, |f| f.min(off));
    *floor = Some(low);
    (off - low) as f32
}

#[allow(clippy::too_many_arguments)]
fn guest_side(commands: &mut Commands, coop: &mut Coop, sim: &mut nvf_play::sim::SimState, mut script: Option<&mut nvf_play::script::ScriptState>, mut residents: Option<&mut Residents>, (loc, player, phys): (Option<&Location>, Option<&Player>, Option<&nvf_play::phys::PhysState>), now: f32) -> Vec<u32> {
    // The references whose place changed here (their collision follows).
    let mut moved = Vec::new();
    let Some(link) = coop.link.as_ref() else { return moved };
    let r = &mut coop.repl;
    for e in std::mem::take(&mut r.inbox) {
        match e {
            Event::RelReset { .. } => {
                info!("replication: the host's stream starts again");
                // A host started again counts its clock from zero: the lag is measured over
                // the new run's lowest (124204 ms read after a load's handoff, 2-6 ms
                // before, otherwise [observed: buckallobill, in game, FNV 1.4.0.525]). Only the perf line reads it.
                r.lag_floor = None;
                r.lag.clear();
            }
            Event::Reliable { item, .. } => {
                if let Some(changes) = repl::unpack_changes(&item) {
                    for c in changes {
                        // What this game's physics carries is its own until it is put
                        // down (the host follows it, `send_carry`).
                        if r.carrying.is_some_and(|(id, _)| id == c.reference) && c.value.comp() == comp::PLACE {
                            continue;
                        }
                        r.applied += 1;
                        // The host's disable of an actor it was drawing: its NPC samples
                        // from before must not show it here again (`place_like_host`), and
                        // this game stops following it (disabled NPCs came back "enabled
                        // as in the host's world" for 5-9 s, one for good, otherwise
                        // [observed: buckallobill, in game, FNV 1.4.0.525]).
                        if let Value::Enabled { enabled, deleted } = c.value {
                            r.enabled_heard.insert(c.reference, now);
                            if enabled && !deleted {
                                coop.host_disabled.remove(&c.reference);
                            } else if coop.host_disabled.insert(c.reference) && coop.npcs.remove(&c.reference).is_some() {
                                info!("replication: the host disabled {:#010X}: not followed here any more", c.reference);
                            }
                        }
                        if let Value::Pose { bones } = &c.value {
                            r.poses_in.push((c.reference, bones.clone()));
                        }
                        if apply(sim, &mut r.slow, &c, residents.as_deref_mut(), commands) {
                            if matches!(c.value, Value::Enabled { .. }) && !sim.is_actor(c.reference) {
                                r.refreshed.push(c.reference);
                            }
                            r.corrections += 1;
                            *r.corrected.entry((c.reference, c.value.comp())).or_default() += 1;
                            if matches!(c.value.comp(), comp::PLACE | comp::ITEM) && !moved.contains(&c.reference) {
                                moved.push(c.reference);
                            }
                        }
                        // The host's verdict on whether it is hostile to the players,
                        // for this game's compass.
                        if let (Value::Anim { hostile, .. }, Some(g)) = (&c.value, script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| &mut rt.game)) {
                            let rf = FormId(c.reference);
                            if *hostile {
                                g.coop_hostile.insert(rf);
                            } else {
                                g.coop_hostile.remove(&rf);
                            }
                        }
                        match c.value {
                            // Not one this player has already taken (a digest sends it again).
                            Value::Item { base, count, .. } if !sim.world.peek(FormId(c.reference)).is_some_and(|s| s.deleted()) => {
                                r.shared.insert(c.reference, (base, count));
                            }
                            Value::Enabled { deleted: true, .. } if r.shared.contains_key(&c.reference) => {
                                r.host_took.insert(c.reference);
                            }
                            _ => {}
                        }
                    }
                } else if let Some(c) = repl::unpack_carry(&item) {
                    // The host carries this object: this game's copy goes where the
                    // host has it (not one this game holds itself).
                    if r.carrying.is_some_and(|(id, _)| id == c.reference) {
                        continue;
                    }
                    let rf = FormId(c.reference);
                    let Ok(s) = sim.world.get(rf) else { continue };
                    s.pos = c.pos;
                    s.rot = c.rot;
                    r.carries_heard += 1;
                    if !moved.contains(&c.reference) {
                        moved.push(c.reference);
                    }
                    if c.done {
                        info!("replication: the host put {} {rf} down at {:?}", sim.ref_name(None, rf.0).unwrap_or("?"), c.pos);
                    }
                } else if let Some(l) = repl::unpack_loose(&item) {
                    guest_loose(link, r, sim, l, &mut moved);
                } else if let Some(effects) = repl::unpack_effects(&item) {
                    // Played as this game's own scripts' would be (the sound at the
                    // reference, which this game has too).
                    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
                        for e in effects {
                            let g = &mut rt.game;
                            r.effects_played += 1;
                            match e {
                                Effect::Sound { sound, at_ref, .. } => g.sound_requests.push(fnv_game::sound::SoundRequest { sound: FormId(sound), at: Some(FormId(at_ref)), system: false, id: 0, point: None }),
                                Effect::Music { music } => g.music_requests.push(FormId(music)),
                                Effect::Summon { interior, world, pos, yaw } => {
                                    info!("replication: the host's script moved it: this player goes too");
                                    r.summon = Some((interior, world, pos, yaw));
                                }
                                Effect::Anim { on, group, flag, settled } => {
                                    let name = g.anim_group_name(group);
                                    if settled {
                                        if let Some(name) = name {
                                            g.anims.show_asked(FormId(on), group, name);
                                        }
                                    } else {
                                        g.ask_anim(fnv_game::anims::AnimRequest { on: FormId(on), group, name, start: fnv_game::anims::Start::from_flag(i32::from(flag)) });
                                    }
                                }
                                Effect::AnimReset { on } => g.anims.reset(FormId(on)),
                                // A checkpoint's hand-over is the progress side's (piece 3):
                                // the host sends none.
                                Effect::Checkpoint { .. } => {}
                                Effect::Dismember { .. } => r.dismembered.push(e),
                                Effect::Sync { serial, done } => {
                                    if done {
                                        info!("replication: the host's answer {serial}: this game holds the host's world here");
                                        r.sync_done = Some(serial);
                                    }
                                }
                                Effect::Rules { friendly_fire } => {
                                    if coop.friendly_fire != friendly_fire {
                                        info!("replication: the host's rules: friendly fire {}", if friendly_fire { "on" } else { "off" });
                                    }
                                    coop.friendly_fire = friendly_fire;
                                }
                            }
                        }
                    }
                }
            }
            Event::Actors { ms, here_ms, poses } => {
                r.lag.push(snapshot_lag(&mut r.lag_floor, here_ms, ms));
                r.poses_heard += poses.len() as u64;
                for p in poses {
                    let s = r.slow.get(&p.reference).cloned().unwrap_or_default();
                    let flags = p.flags | if s.dead { npc_flag::DEAD } else { 0 };
                    let n = NpcState {
                        reference: p.reference,
                        feet: p.feet,
                        heading: p.heading,
                        pitch: p.pitch,
                        flags,
                        weapon: s.weapon,
                        attacks: p.attacks,
                        furniture: s.furniture,
                        base: s.base,
                        level: s.level,
                        all_levels: s.all_levels,
                        rolls: s.rolls,
                        hostile: s.hostile,
                        space: s.space,
                    };
                    if super::traced(p.reference) {
                        info!("co-op trace: {:08X} pose heard at {now:.3}: ({:.0}, {:.0}, {:.0})", p.reference, n.feet[0], n.feet[1], n.feet[2]);
                    }
                    let t = coop.npcs.entry(p.reference).or_default();
                    t.samples.push_back((now, n));
                    while t.samples.len() > 8 {
                        t.samples.pop_front();
                    }
                }
            }
            _ => {}
        }
    }
    guest_drops(link, r, sim, phys, now);
    // The digest of what this game holds near its player.
    if now - r.last_digest >= DIGEST_EVERY {
        r.last_digest = now;
        let (Some(loc), Some(player)) = (loc, player) else { return moved };
        let place = place_of(loc);
        let feet = player.feet;
        // Not this game's own created references, nor its actors hidden as not in the
        // host's world here (`not_in_hosts_world`).
        let hidden = &coop.hidden;
        let refs: Vec<FormId> = sim.world.changed().map(|(k, _)| k).filter(|&k| k != FormId::PLAYER_REF && k.0 < GUEST_CREATED && !hidden.contains(&k)).collect();
        let mut d = Digest { refs: Vec::new() };
        for rf in refs {
            if in_area(&sim.world, rf, place, feet) {
                if let Some(v) = world_values(&sim.world, rf) {
                    d.refs.push((rf.0, repl::digest_of(&v)));
                }
            }
        }
        for item in repl::pack_digest(&d, ITEM) {
            // A full stream refuses it; the stream starts again and resends the whole state.
            let _ = link.send_reliable(None, item);
        }
    }
    moved
}

/// Apply one of the host's changes here; whether it changed anything.
fn apply(sim: &mut nvf_play::sim::SimState, slow: &mut HashMap<u32, Slow>, c: &Change, residents: Option<&mut Residents>, commands: &mut Commands) -> bool {
    let rf = FormId(c.reference);
    let w = &mut sim.world;
    match &c.value {
        Value::Enabled { enabled, deleted } => {
            let item = w.peek(rf).map(|s| s.base).or_else(|| w.placed.get(rf).map(|p| p.base)).and_then(|b| w.forms.item(b)).is_some();
            let shared = w.peek(rf).is_some_and(|s| s.player_drop);
            let Ok(st) = w.get(rf) else { return false };
            let was = (st.enabled(), st.deleted());
            let _ = w.set_enabled(rf, *enabled);
            // A loose item this player took stays taken here (per-player loot); a player's
            // dropped item is the host's world's.
            if !item || shared {
                let _ = w.set_deleted(rf, *deleted);
            }
            w.peek(rf).is_some_and(|s| (s.enabled(), s.deleted()) != was)
        }
        Value::Place { cell, pos, rot, scale } => {
            let Ok(st) = w.get(rf) else { return false };
            let new_cell = (*cell != 0).then_some(FormId(*cell));
            let new_scale = (*scale != 0.0).then_some(*scale);
            let changed = st.cell != new_cell || st.pos != *pos || st.rot != *rot || st.scale != new_scale;
            st.cell = new_cell;
            st.pos = *pos;
            st.rot = *rot;
            st.scale = new_scale;
            changed
        }
        Value::Open(o) => {
            let Ok(st) = w.get(rf) else { return false };
            let changed = st.open != *o;
            st.open = *o;
            changed
        }
        Value::Lock { has, locked, level } => {
            let was = w.lock_of(rf).map(|l| (l.locked, l.level));
            let _ = match (has, locked) {
                (true, true) => w.lock(rf, Some(*level)),
                _ => w.unlock(rf),
            };
            w.lock_of(rf).map(|l| (l.locked, l.level)) != was
        }
        Value::Exists { base, cell, pos, rot } => {
            if w.peek(rf).is_some() {
                return false;
            }
            match w.place_as(rf, FormId(*base), (*cell != 0).then_some(FormId(*cell)), *pos, *rot) {
                Ok(_) => {
                    info!("replication: the host's {} {rf} made here", w.forms.get(FormId(*base)).and_then(|b| b.edid.clone()).unwrap_or_default());
                    true
                }
                Err(e) => {
                    warn!("replication: the host's {rf} cannot be made here: {e:?}");
                    false
                }
            }
        }
        Value::Item { base, count, cell, pos, rot } => {
            if let Some(had) = w.peek(rf).map(|s| s.count) {
                let Ok(st) = w.get(rf) else { return false };
                st.count = *count;
                st.player_drop = true;
                return had != *count;
            }
            match w.place_as(rf, FormId(*base), (*cell != 0).then_some(FormId(*cell)), *pos, *rot) {
                Ok(_) => {
                    if let Ok(st) = w.get(rf) {
                        st.count = *count;
                        st.player_drop = true;
                    }
                    info!("replication: a player's dropped {} x{count} {rf} lies here too", w.forms.get(FormId(*base)).and_then(|b| b.edid.clone()).unwrap_or_default());
                    true
                }
                Err(e) => {
                    warn!("replication: a player's dropped {rf} cannot be made here: {e:?}");
                    false
                }
            }
        }
        Value::Life { dead } => {
            let s = slow.entry(c.reference).or_default();
            let changed = s.dead != *dead;
            s.dead = *dead;
            changed
        }
        Value::Anim { furniture, weapon, hostile, space } => {
            let s = slow.entry(c.reference).or_default();
            let changed = (s.furniture, s.weapon, s.space) != (*furniture, *weapon, *space);
            (s.furniture, s.weapon, s.space) = (*furniture, *weapon, *space);
            // The compass's hostile mark (`Game::coop_hostile`, set by `guest_side`).
            s.hostile = *hostile;
            changed
        }
        // Laid down by [`corpses_in`].
        Value::Pose { .. } => false,
        Value::Look { base, level, all_levels, rolls, worn } => {
            let s = slow.entry(c.reference).or_default();
            (s.base, s.level, s.all_levels, s.rolls) = (*base, *level, *all_levels, *rolls);
            let items: Vec<FormId> = worn.iter().map(|&i| FormId(i)).collect();
            let dressed = w.wear_as(rf, &items).unwrap_or(false);
            if dressed {
                info!("replication: {rf} dressed as in the host's game ({} items)", items.len());
                if let Some(res) = residents {
                    res.redraw(commands, c.reference);
                }
            }
            dressed
        }
    }
}

/// How long a guest's dropped item lies still (within [`DROP_STILL`] units) before it goes
/// to the host as at rest: its body need not sleep (a dropped pistol lay still for 10 s
/// without sleeping [observed: buckallobill, in game, FNV 1.4.0.525]).
const DROP_REST: f32 = 0.5;
const DROP_STILL: f32 = 1.0;

/// The host: a guest's dropped item or pickup (module notes), checked first
/// (`checks::dropped`, `checks::took`): `guest` is where the guest's newest state put it.
#[allow(clippy::type_complexity)]
fn host_loose(link: &Link, r: &mut Repl, (host_checks, guest, phys): (&mut checks::HostChecks, Option<&checks::Guest>, Option<&nvf_play::phys::PhysState>), sim: &mut nvf_play::sim::SimState, id: u8, l: Loose, now: f32) {
    match l {
        Loose::Dropped { local, base, count, cell, pos, rot } => {
            // In the guest's space: the cell indoors, its worldspace outdoors. A refused
            // drop gets no answer: the guest's copy stays its own (`Loose::Refused` would
            // take items from it).
            let space = sim.world.placed.world_of(FormId(cell)).map_or(cell, |w| w.0);
            if let Err(why) = checks::dropped(guest.map(|g| g.space), space) {
                super::refused_by(host_checks, id, why, now);
                return;
            }
            match sim.world.place(FormId(base), (cell != 0).then_some(FormId(cell)), pos, rot, count) {
                Ok(made) => {
                    if let Ok(s) = sim.world.get(made) {
                        s.player_drop = true;
                    }
                    r.loose += 1;
                    info!("replication: player {} dropped {} x{count}: made here as {made}, every player sees it", id + 1, sim.name(FormId(base)).unwrap_or("?"));
                    // A full stream refuses it; the stream starts again and resends the whole state.
                    let _ = link.send_reliable(Some(id), repl::pack_loose(&Loose::Made { local, host: made.0 }));
                    // A guest's drops lying here are bounded: its oldest goes.
                    if let Some(old) = host_checks.dropped(id, made.0) {
                        let _ = sim.world.set_deleted(FormId(old), true);
                        info!("replication: player {}'s oldest drop {old:#010X} taken away: at most {} of its drops lie in the world", id + 1, checks::DROPS_KEPT);
                    }
                }
                Err(e) => warn!("replication: player {}'s dropped {base:#010X} cannot be made here: {e:?}", id + 1),
            }
        }
        Loose::Took { reference } => {
            let rf = FormId(reference);
            let lying = sim.world.peek(rf).filter(|s| s.player_drop).map(|s| (s.base.0, s.count, !s.deleted() && s.enabled()));
            r.loose += 1;
            // Lying, but not where this guest can take it: given back as a late one is.
            let verdict = match guest {
                Some(g) => checks::took(&checks::SimWorld { sim, combat: None, phys }, g, reference),
                None => Err(checks::why::TAKE_TARGET),
            };
            match lying {
                Some((base, count, true)) if verdict.is_err() => {
                    super::refused_by(host_checks, id, verdict.err().unwrap_or(checks::why::TAKE_TARGET), now);
                    // A full stream refuses it; the stream starts again and resends the whole state.
                    let _ = link.send_reliable(Some(id), repl::pack_loose(&Loose::Refused { reference, base, count }));
                }
                Some((_, count, true)) => {
                    let _ = sim.world.set_deleted(rf, true);
                    host_checks.taken(reference);
                    info!("replication: player {} took {} x{count} {rf}: gone for every player", id + 1, sim.ref_name(None, reference).unwrap_or("?"));
                }
                Some((base, count, false)) => {
                    info!("replication: player {} took {} {rf}, but it was already taken: it gives it back", id + 1, sim.ref_name(None, reference).unwrap_or("?"));
                    // A full stream refuses it; the stream starts again and resends the whole state.
                    let _ = link.send_reliable(Some(id), repl::pack_loose(&Loose::Refused { reference, base, count }));
                }
                None => warn!("replication: player {} took {rf}, which is no player's dropped item here", id + 1),
            }
        }
        Loose::Made { .. } | Loose::Refused { .. } => {}
    }
}

/// A guest: the host's answer about its drop or pickup.
fn guest_loose(link: &Link, r: &mut Repl, sim: &mut nvf_play::sim::SimState, l: Loose, moved: &mut Vec<u32>) {
    match l {
        Loose::Made { local, host } => {
            let (lf, hf) = (FormId(local), FormId(host));
            r.loose += 1;
            match sim.world.peek(lf).filter(|s| !s.deleted() && s.enabled()).cloned() {
                Some(s) => {
                    // This game's copy takes the host's number: the same item for everyone.
                    let _ = sim.world.set_deleted(lf, true);
                    if sim.world.peek(hf).is_none() && sim.world.place_as(hf, s.base, s.cell, s.pos, s.rot).is_ok() {
                        if let Ok(n) = sim.world.get(hf) {
                            n.count = s.count;
                            n.extra = s.extra;
                            n.player_drop = true;
                        }
                    }
                    r.shared.insert(host, (s.base.0, s.count));
                    if !moved.contains(&host) {
                        moved.push(host);
                    }
                    info!("replication: this player's dropped {} {lf} is {hf} in the host's world", sim.name(s.base).unwrap_or("?"));
                }
                None => {
                    // Taken back here before the host had it: taken there too.
                    info!("replication: this player's dropped {lf} was taken back before the host made it ({hf}): taken there too");
                    // A full stream refuses it; the stream starts again and resends the whole state.
                    let _ = link.send_reliable(None, repl::pack_loose(&Loose::Took { reference: host }));
                }
            }
        }
        Loose::Refused { reference, base, count } => {
            r.loose += 1;
            let back = sim.world.remove_item(FormId::PLAYER_REF, FormId(base), count).unwrap_or(0);
            info!("replication: another player took {} {reference:#010X} first: {back} of {count} given back", sim.name(FormId(base)).unwrap_or("?"));
        }
        Loose::Dropped { .. } | Loose::Took { .. } => {}
    }
}

/// A guest: its own drops that came to rest go to the host, and its pickups of the
/// players' dropped items are asked of it (module notes).
fn guest_drops(link: &Link, r: &mut Repl, sim: &nvf_play::sim::SimState, phys: Option<&nvf_play::phys::PhysState>, now: f32) {
    type Mine = (FormId, u32, i32, u32, [f32; 3], [f32; 3]);
    let mine: Vec<Mine> = sim
        .world
        .created()
        .filter(|(k, s)| k.0 >= GUEST_CREATED && s.player_drop && !s.deleted() && s.enabled() && !r.drops_sent.contains(&k.0))
        .map(|(k, s)| (k, s.base.0, s.count, s.cell.map_or(0, |c| c.0), s.pos, s.rot))
        .collect();
    r.drops_waiting.retain(|k, _| mine.iter().any(|m| m.0 .0 == *k));
    for (k, base, count, cell, pos, rot) in mine {
        let (pos, rot, sleeping) = phys.and_then(|p| p.ref_pose_now(k.0)).unwrap_or((pos, rot, false));
        let w = r.drops_waiting.entry(k.0).or_insert((now, pos, now));
        if (w.1[0] - pos[0]).hypot(w.1[1] - pos[1]).hypot(w.1[2] - pos[2]) > DROP_STILL {
            (w.1, w.2) = (pos, now);
        }
        let (since, still) = (w.0, w.2);
        if !sleeping && now - still < DROP_REST && now - since < CARRY_FALL_MAX {
            continue;
        }
        // A full stream refuses it; the stream starts again and resends the whole state.
        let _ = link.send_reliable(None, repl::pack_loose(&Loose::Dropped { local: k.0, base, count, cell, pos, rot }));
        r.drops_sent.insert(k.0);
        r.drops_waiting.remove(&k.0);
        r.loose += 1;
        info!("replication: this player dropped {} x{count} {k}, at rest at {pos:?}: it goes to the host", sim.name(FormId(base)).unwrap_or("?"));
    }
    let gone: Vec<u32> = r.shared.keys().copied().filter(|&k| sim.world.peek(FormId(k)).is_none_or(|s| s.deleted() || !s.enabled())).collect();
    for k in gone {
        r.shared.remove(&k);
        if r.host_took.remove(&k) {
            continue;
        }
        r.loose += 1;
        info!("replication: this player took the dropped {} {k:#010X}: asked of the host", sim.ref_name(None, k).unwrap_or("?"));
        // A full stream refuses it; the stream starts again and resends the whole state.
        let _ = link.send_reliable(None, repl::pack_loose(&Loose::Took { reference: k }));
    }
}

fn perf_line(coop: &mut Coop, now: f32) {
    let r = &mut coop.repl;
    let Some(link) = coop.link.as_ref() else {
        // The solo baseline ([`PERF_ENV`]).
        let since = *r.perf.since.get_or_insert(now);
        if now - since >= PERF_EVERY {
            let f = &r.perf.frames;
            info!("replication perf: solo frame avg {:.2} ms p95 {:.2} p99 {:.2} ({} frames)", average(f), percentile(f, 0.95), percentile(f, 0.99), f.len());
            r.perf.frames.clear();
            r.perf.spent.clear();
            r.perf.since = Some(now);
        }
        return;
    };
    let since = *r.perf.since.get_or_insert(now);
    let span = now - since;
    if span < PERF_EVERY {
        return;
    }
    let traffic = link.traffic();
    let rate = |id: u8, f: fn(&net::Traffic) -> u64, t: &net::Traffic| {
        let before = r.perf.traffic.get(&id).map_or(0, f);
        (f(t).saturating_sub(before)) as f32 / span / 1024.0
    };
    let frames = &r.perf.frames;
    let (avg, p95, p99) = (average(frames), percentile(frames, 0.95), percentile(frames, 0.99));
    let (spent_avg, spent_max) = (average(&r.perf.spent), r.perf.spent.iter().copied().fold(0.0, f32::max));
    let mut line = format!(
        "replication perf: {} frame avg {avg:.2} ms p95 {p95:.2} p99 {p99:.2} ({} frames); replication {spent_avg:.3} ms/frame (max {spent_max:.2})",
        if link.is_host() { "host" } else { "guest" },
        frames.len()
    );
    if link.is_host() {
        for t in &traffic {
            line += &format!(
                "; player {}: sent {:.2} KB/s heard {:.2} KB/s, area {} refs, actors {}, changes {}, pending {}, resent {}",
                t.id + 1,
                rate(t.id, |t| t.sent, t),
                rate(t.id, |t| t.heard, t),
                r.interest.get(&t.id).copied().unwrap_or(0),
                r.host_npcs.len(),
                r.changes_sent.get(&t.id).copied().unwrap_or(0),
                t.pending,
                t.resent
            );
        }
        line += &format!("; desyncs {} (first digests' corrections {}), effects {}, players' drops {}", r.desyncs, r.start_fixes, r.effects_sent, r.loose);
    } else if let Some(t) = traffic.first() {
        line += &format!(
            "; from host: heard {:.2} KB/s sent {:.2} KB/s, changes {} (corrections {}), poses {:.0}/s, snapshot lag avg {:.0} ms p95 {:.0} ms (over the lowest seen), effects {}, players' drops {}",
            rate(0, |t| t.heard, t),
            rate(0, |t| t.sent, t),
            r.applied,
            r.corrections,
            r.poses_heard as f32 / span,
            average(&r.lag),
            percentile(&r.lag, 0.95),
            r.effects_played,
            r.loose
        );
        if !r.corrected.is_empty() {
            let mut most: Vec<((u32, u8), u64)> = r.corrected.drain().collect();
            most.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let top: Vec<String> = most.iter().take(3).map(|((id, c), n)| format!("{id:08X} {} x{n}", comp_name(*c))).collect();
            line += &format!("; most corrected since the last line: {}", top.join(", "));
        }
        r.poses_heard = 0;
        r.lag.clear();
    }
    info!("{line}");
    if std::env::var(HUD_ENV).is_ok_and(|v| v == "1") {
        coop.repl_hud = Some(line);
    }
    let r = &mut coop.repl;
    r.perf.traffic = traffic.into_iter().map(|t| (t.id, t)).collect();
    r.perf.frames.clear();
    r.perf.spent.clear();
    r.perf.since = Some(now);
}

/// The optional corner readout ([`HUD_ENV`]): the newest perf line, top right.
#[derive(Component)]
pub(super) struct ReplHud;

/// The host's: every body part gone here goes to the guests (`Effect::Dismember`).
pub(super) fn dismember_out(mut coop: ResMut<Coop>, mut gone: MessageReader<nvf_play::dismember::Dismembered>) {
    let host = coop.link.as_ref().is_some_and(|l| l.is_host()) && coop.peers.values().any(|p| !p.lost);
    for d in gone.read() {
        if host {
            info!("replication: {:#010X} lost part {} ({}): the guests see it too", d.form_id, d.part, if d.exploded { "exploded" } else { "severed" });
            coop.repl.dismembered.push(Effect::Dismember { actor: d.form_id, data: d.data.0, part: d.part as u16, location: d.location, exploded: d.exploded, point: d.point, dir: d.dir });
        }
    }
}

/// A guest's: the host's gone body parts, taken off here (the game's record, read by
/// IsLimbGone) and drawn as the host's were (`crate::dismember`).
pub(super) fn dismember_in(mut coop: ResMut<Coop>, mut script: Option<ResMut<nvf_play::script::ScriptState>>, mut out: MessageWriter<nvf_play::dismember::Dismembered>) {
    if coop.repl.dismembered.is_empty() || coop.link.as_ref().is_none_or(|l| l.is_host()) {
        return;
    }
    let Some(g) = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| &mut rt.game) else { return };
    for e in std::mem::take(&mut coop.repl.dismembered) {
        let Effect::Dismember { actor, data, part, location, exploded, point, dir } = e else { continue };
        // Once: a digest may send it again.
        if g.dismember(FormId(actor), &[(location, exploded)], None).is_empty() {
            continue;
        }
        info!("replication: the host's {actor:#010X} lost part {part} ({}): taken off here too", if exploded { "exploded" } else { "severed" });
        out.write(nvf_play::dismember::Dismembered { form_id: actor, data: FormId(data), part: usize::from(part), location, exploded, point, dir });
    }
}

/// The host's: each body at rest here (`crate::ragdoll::Ragdolls::rest`, not one still
/// falling) as a [`Value::Pose`], for the guests ([`corpses_in`]). Sent with the actors the
/// host draws or simulates (`host_side`), so a guest that comes by later gets it too.
pub(super) fn corpses_out(mut coop: ResMut<Coop>, ragdolls: Res<crate::ragdoll::Ragdolls>, time: Res<Time>, mut last: Local<f32>) {
    let coop = &mut *coop;
    if !coop.link.as_ref().is_some_and(|l| l.is_host()) || coop.peers.is_empty() {
        coop.repl.host_rest.clear();
        return;
    }
    // As often as the change log goes out (`TICK`).
    let now = time.elapsed_secs();
    if now - *last < TICK {
        return;
    }
    *last = now;
    let r = &mut coop.repl;
    r.host_rest.retain(|id, _| ragdolls.rest.contains_key(id) && !ragdolls.falling.contains(id));
    for (&id, rest) in ragdolls.rest.iter().filter(|(id, _)| !ragdolls.falling.contains(id) && **id != FormId::PLAYER_REF.0) {
        let pose = Value::Pose { bones: rest.iter().map(|b| b.as_ref().map(pose_bone)).collect() };
        if r.host_rest.get(&id) != Some(&pose) {
            r.host_rest.insert(id, pose);
        }
    }
}

/// A guest's: the host's bodies at rest, laid down here in the host's pose and at rest
/// (`crate::ragdoll::start` puts a body back asleep in its stored pose). A body that died
/// in sight here keeps its own fall; one drawn dead (the guest came by afterwards, or
/// drew it again) is put in the host's pose at once [observed: buckallobill, in game, FNV 1.4.0.525].
pub(super) fn corpses_in(
    mut coop: ResMut<Coop>,
    mut ragdolls: ResMut<crate::ragdoll::Ragdolls>,
    phys: Option<ResMut<nvf_play::phys::PhysState>>,
    mut corpses: Query<(&crate::limbs::ActorRig, &mut crate::ragdoll::Corpse)>,
) {
    if coop.repl.poses_in.is_empty() {
        return;
    }
    let mut phys = phys;
    for (id, bones) in std::mem::take(&mut coop.repl.poses_in) {
        let pose: Vec<Option<fnv_collide::Iso>> = bones.iter().map(|b| b.as_ref().map(iso_of_bone)).collect();
        let drawn = corpses.iter_mut().find(|(rig, _)| rig.reference == id);
        if let Some((_, mut c)) = drawn {
            if c.fell_here {
                continue;
            }
            if c.body_bones().len() != pose.len() {
                warn!("replication: the host's body {id:#010X} has {} ragdoll bodies, {} here: its pose is not used", pose.len(), c.body_bones().len());
                continue;
            }
            if c.live {
                if let Some(p) = phys.as_deref_mut() {
                    p.world.remove_ragdoll(id);
                }
                c.live = false;
            }
        }
        if ragdolls.rest.get(&id) == Some(&pose) {
            continue;
        }
        let at = pose.iter().flatten().next().map(|i| i.translation).unwrap_or_default();
        info!("replication: the host's body {id:#010X} lies at rest at ({:.0}, {:.0}, {:.0}): laid down here in its pose", at[0], at[1], at[2]);
        ragdolls.rest.insert(id, pose);
        ragdolls.falling.remove(&id);
    }
}

fn pose_bone(i: &fnv_collide::Iso) -> PoseBone {
    let r = &i.rotation;
    let m = Mat3::from_cols(Vec3::new(r[0], r[3], r[6]), Vec3::new(r[1], r[4], r[7]), Vec3::new(r[2], r[5], r[8]));
    let q = Quat::from_mat3(&m).normalize();
    PoseBone::new(i.translation, [q.x, q.y, q.z, q.w])
}

fn iso_of_bone(b: &PoseBone) -> fnv_collide::Iso {
    let [x, y, z, w] = b.quat();
    let m = Mat3::from_quat(Quat::from_xyzw(x, y, z, w));
    let (c0, c1, c2) = (m.x_axis, m.y_axis, m.z_axis);
    fnv_collide::Iso { translation: b.at, rotation: [c0.x, c1.x, c2.x, c0.y, c1.y, c2.y, c0.z, c1.z, c2.z] }
}

pub(super) fn hud(mut commands: Commands, mut coop: ResMut<Coop>, mut shown: Query<&mut Text, With<ReplHud>>) {
    let Some(line) = coop.repl_hud.take() else { return };
    let text = line.replace("; ", "\n");
    match shown.single_mut() {
        Ok(mut t) => t.0 = text,
        Err(_) => {
            commands.spawn((
                ReplHud,
                Text::new(text),
                TextFont { font_size: FontSize::Px(11.0), ..default() },
                TextColor(Color::srgba(1.0, 0.85, 0.5, 0.9)),
                Node { position_type: PositionType::Absolute, top: Val::Px(8.0), right: Val::Px(8.0), max_width: Val::Px(520.0), ..default() },
            ));
        }
    }
}

/// A replicated component's name, for the perf line.
fn comp_name(c: u8) -> &'static str {
    match c {
        comp::ENABLED => "enabled",
        comp::PLACE => "place",
        comp::OPEN => "open",
        comp::LOCK => "lock",
        comp::LIFE => "life",
        comp::LOOK => "look",
        comp::EXISTS => "exists",
        comp::ANIM => "anim",
        comp::ITEM => "item",
        _ => "?",
    }
}

/// A guest, where it is: its id, place, feet and yaw.
type GuestAt = (u8, Place, [f32; 3], f32);

/// The guests within [`fnv_game::coop::STORY_NEAR`] units of where the host stood (same
/// place), and the others. No place known for the host: every guest is near (as before).
fn near_guests(host: Option<(Place, [f32; 3])>, guests: &[(u8, Place, [f32; 3], f32)]) -> (Vec<u8>, Vec<u8>) {
    let near = |place: Place, feet: [f32; 3]| {
        host.is_none_or(|(hp, hf)| hp == place && (0..3).map(|i| (feet[i] - hf[i]).powi(2)).sum::<f32>() <= fnv_game::coop::STORY_NEAR * fnv_game::coop::STORY_NEAR)
    };
    let (n, f): (Vec<&GuestAt>, Vec<_>) = guests.iter().partition(|g| near(g.1, g.2));
    (n.iter().map(|g| g.0).collect(), f.iter().map(|g| g.0).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The snapshot lag over a host started again: its clock counts from zero, so without
    /// the reset every round reads the old run's lead (124204 ms).
    #[test]
    fn the_snapshot_lag_starts_again_with_the_host() {
        let mut floor = None;
        // The old host's run, 130 s in when the guest started: rounds 2-6 ms late.
        assert_eq!(snapshot_lag(&mut floor, 1_000, 130_000), 0.0);
        assert_eq!(snapshot_lag(&mut floor, 2_006, 131_000), 6.0);
        // The new run's clock starts again; the guest's goes on.
        assert_eq!(snapshot_lag(&mut floor, 3_000, 5), 131_995.0, "over the old run's floor");
        floor = None;
        assert_eq!(snapshot_lag(&mut floor, 3_000, 5), 0.0);
        assert_eq!(snapshot_lag(&mut floor, 4_003, 1_005), 3.0);
    }

    /// A script that moves the host to the same spot again and again is one summon for
    /// the guests; a move elsewhere is another.
    #[test]
    fn the_same_spot_again_is_one_summon() {
        let at = |interior, world, x: f32| Effect::Summon { interior, world, pos: [x, 0.0, 0.0], yaw: 0.0 };
        assert!(same_summon(&at(0x1555FD, 0, 0.0), &at(0x1555FD, 0, 30.0)));
        assert!(!same_summon(&at(0x1555FD, 0, 0.0), &at(0x1555FD, 0, 300.0)), "farther than a step");
        assert!(!same_summon(&at(0x1555FD, 0, 0.0), &at(0, 0xDA726, 0.0)), "another place");
        assert!(!same_summon(&at(0x1555FD, 0, 0.0), &Effect::Rules { friendly_fire: true }));
    }

    /// The same move again within a moment is the same trip, taken or not (13 moves to one
    /// spot in one second, each "player 2 too far"); later, or elsewhere, it is a new one.
    #[test]
    fn the_same_move_again_soon_is_one_trip() {
        let at = |x: f32| Effect::Summon { interior: 0x1555FD, world: 0, pos: [x, 0.0, 0.0], yaw: 0.0 };
        assert!(again(0.02, &at(0.0), &at(0.0)));
        assert!(again(1.9, &at(0.0), &at(10.0)));
        assert!(!again(2.5, &at(0.0), &at(0.0)), "a later move is another trip");
        assert!(!again(0.1, &at(0.0), &at(500.0)), "a move elsewhere is another trip");
    }

    #[test]
    fn a_resting_bone_reaches_a_guest_as_the_host_has_it() {
        // 90 degrees about z, then 30 about x (as `ragdoll`'s test), at a point.
        let (s, c) = 30f32.to_radians().sin_cos();
        let i = fnv_collide::Iso { translation: [120.0, -40.0, 66.0], rotation: [0.0, -c, s, 1.0, 0.0, 0.0, 0.0, s, c] };
        let back = iso_of_bone(&pose_bone(&i));
        assert_eq!(back.translation, i.translation);
        assert!(back.rotation.iter().zip(&i.rotation).all(|(a, b)| (a - b).abs() < 2e-4), "{:?}", back.rotation);
    }

    /// A script's move of the host takes the guests within sight of where it stood
    /// [design], not one in another place or farther; with no place known for
    /// the host, every guest, as before.
    #[test]
    fn a_scripted_move_takes_the_guests_within_sight() {
        let here = Place::Interior(0x0001_2345);
        let guests = [(0u8, here, [500.0, 0.0, 0.0], 0.0), (1, here, [3000.0, 0.0, 0.0], 0.0), (2, Place::Exterior(0xDA726), [0.0; 3], 0.0)];
        assert_eq!(near_guests(Some((here, [0.0; 3])), &guests), (vec![0], vec![1, 2]));
        assert_eq!(near_guests(None, &guests).0, vec![0, 1, 2]);
    }

    fn open_values(_: FormId) -> Option<Vec<Value>> {
        Some(vec![Value::Open(true)])
    }

    /// A digest of 300 references the guest holds unlike the host goes in three items;
    /// all three are the first round (not only the first item), and the excess over the
    /// resend cap waits and goes in the next second, none of it counted against the guest.
    #[test]
    fn a_first_digest_of_many_references_is_one_round_and_no_bad_message() {
        let mut r = Repl::default();
        let mut hc = checks::HostChecks::default();
        let d = Digest { refs: (1..=300u32).map(|x| (x, 0)).collect() };
        let items = repl::pack_digest(&d, ITEM);
        assert_eq!(items.len(), 3, "more than 124 references: more than one item");
        r.starting.insert(1);
        for (i, item) in items.iter().enumerate() {
            let d = repl::unpack_digest(item).unwrap();
            host_digest(&mut r, &mut hc, 1, &d, 10.0 + i as f32 * 0.05, open_values, |_| String::new());
        }
        assert_eq!(r.desyncs, 0, "the later items are in the first round");
        assert_eq!(r.start_fixes, u64::from(checks::FORCED_PER_SECOND));
        assert_eq!(hc.total(), 0, "nothing refused");
        assert!(hc.bad.is_empty(), "no bad message");
        drain_start_backlog(&mut r, &mut hc, 10.5, open_values, |_| String::new());
        assert_eq!(r.start_fixes, u64::from(checks::FORCED_PER_SECOND), "still the same second");
        drain_start_backlog(&mut r, &mut hc, 11.2, open_values, |_| String::new());
        assert_eq!(r.start_fixes, 300);
        assert!(r.start_backlog.is_empty());
        assert_eq!(r.forced.get(&1).map(|f| f.len()), Some(300));
        // The round is over: the same again is desyncs, and past the cap a refusal.
        r.forced.clear();
        let first = repl::unpack_digest(&items[0]).unwrap();
        host_digest(&mut r, &mut hc, 1, &first, 20.0, open_values, |_| String::new());
        assert_eq!(r.desyncs, 124);
        let more = repl::unpack_digest(&items[1]).unwrap();
        host_digest(&mut r, &mut hc, 1, &more, 20.1, open_values, |_| String::new());
        let last = repl::unpack_digest(&items[2]).unwrap();
        host_digest(&mut r, &mut hc, 1, &last, 20.2, open_values, |_| String::new());
        assert_eq!(r.desyncs, u64::from(checks::FORCED_PER_SECOND));
        assert_eq!(r.start_fixes, 300);
        assert_eq!(hc.total(), 1, "past the cap outside a first round: refused");
    }

    /// The host's side of one guest's stream, as `host_side` keeps it: what was sent, when,
    /// what a digest found out of step, and whether its first digest has come.
    #[derive(Default)]
    struct Stream {
        shadow: Shadow,
        sent_at: HashMap<u32, f32>,
        forced: BTreeSet<u32>,
        starting: bool,
        desyncs: u32,
        start_fixes: u32,
    }

    type Refs = std::collections::BTreeMap<u32, Vec<Value>>;

    fn digest(world: &Refs) -> Vec<(u32, u32)> {
        world.iter().map(|(&r, v)| (r, repl::digest_of(v))).collect()
    }

    fn apply(world: &mut Refs, changes: &[Change]) {
        for c in changes {
            let vals = world.entry(c.reference).or_default();
            vals.retain(|v| v.comp() != c.value.comp());
            vals.push(c.value.clone());
        }
    }

    impl Stream {
        /// The stream starts again (`Event::RelReset`), or a syncing screen asks.
        fn restart(&mut self, first_digest_to_come: bool) {
            self.shadow.clear();
            self.sent_at.clear();
            self.starting |= first_digest_to_come;
        }

        /// A digest heard, as `host_side` judges it.
        fn heard(&mut self, d: &[(u32, u32)], host: &Refs, now: f32) {
            let starting = std::mem::take(&mut self.starting);
            for (reference, _) in out_of_step(d, &self.sent_at, now, |rf| host.get(&rf.0).cloned()) {
                if starting {
                    self.start_fixes += 1;
                } else {
                    self.desyncs += 1;
                }
                self.shadow.forget(reference);
                self.forced.insert(reference);
            }
        }

        /// One tick's changes to the guest, as `host_side` sends them.
        fn tick(&mut self, area: &[u32], host: &Refs, authored: &Refs, now: f32) -> Vec<Change> {
            let area: Vec<FormId> = area.iter().map(|&r| FormId(r)).collect();
            let forced = std::mem::take(&mut self.forced);
            let changes = guest_changes(&mut self.shadow, &area, &forced, |rf| host.get(&rf.0).cloned(), |rf, v| authored.get(&rf.0).is_some_and(|a| a.contains(v)));
            for c in &changes {
                self.sent_at.insert(c.reference, now);
            }
            changes
        }
    }

    /// Three, four and six players: the guests join one after another, each through its
    /// syncing screen (its digest first, then its ask, answered after a sweep), the last
    /// with a save whose loose books lie elsewhere (its own physics settled them its own
    /// way). The host never moved those books, so no sweep names them: the first digest's
    /// resend has to reach the guest anyway. Every guest ends holding the host's world, and
    /// no digest after the first finds anything out of step (the third player's books
    /// were flagged twice, 3 s apart, before the fix).
    #[test]
    fn a_late_guest_with_its_own_save_ends_in_step() {
        let place = |x: f32| Value::Place { cell: 0x100, pos: [x, 0.0, 0.0], rot: [0.0; 3], scale: 0.0 };
        let on = Value::Enabled { enabled: true, deleted: false };
        // The plugins' world: three books, a door, a lamp.
        let authored: Refs = (1..=5u32).map(|r| (r, vec![on.clone(), place(r as f32 * 10.0)])).collect();
        // The host's: it opened the door and moved the lamp (its sweep names those two).
        let mut host = authored.clone();
        host.insert(4, vec![on.clone(), place(40.0), Value::Open(true)]);
        host.insert(5, vec![on.clone(), place(55.0)]);
        let host_changed = [4u32, 5];
        for players in [3usize, 4, 6] {
            let guests = players - 1;
            let mut streams: Vec<Stream> = (0..guests).map(|_| Stream::default()).collect();
            let mut worlds: Vec<Refs> = (0..guests)
                .map(|g| {
                    let mut w = authored.clone();
                    if g == guests - 1 {
                        for b in 1..=3u32 {
                            w.insert(b, vec![on.clone(), place(b as f32 * 10.0 + 3.5)]);
                        }
                    }
                    w
                })
                .collect();
            let mut now = 0.0f32;
            for g in 0..guests {
                // It joins: the stream starts, the area goes at a sweep.
                now += 5.0;
                streams[g].restart(true);
                let c = streams[g].tick(&host_changed, &host, &authored, now);
                apply(&mut worlds[g], &c);
                // Its syncing screen: its digest, then its ask; the sweep, then the answer.
                now += 0.5;
                let d = digest(&worlds[g]);
                streams[g].heard(&d, &host, now);
                streams[g].restart(false);
                now += 0.05;
                let c = streams[g].tick(&host_changed, &host, &authored, now);
                apply(&mut worlds[g], &c);
                // Every guest's digests go on, every 3 s, ticks between them.
                for _ in 0..5 {
                    now += DIGEST_EVERY;
                    for h in 0..=g {
                        let d = digest(&worlds[h]);
                        streams[h].heard(&d, &host, now);
                        now += TICK;
                        let c = streams[h].tick(&[], &host, &authored, now);
                        apply(&mut worlds[h], &c);
                    }
                }
            }
            for (g, (s, w)) in streams.iter().zip(&worlds).enumerate() {
                assert_eq!(s.desyncs, 0, "{players} players: player {} out of step after its first digest", g + 2);
                let expected = if g == guests - 1 { 3 } else { 0 };
                assert_eq!(s.start_fixes, expected, "{players} players: player {}'s own save's books", g + 2);
                for (r, vals) in &host {
                    assert_eq!(repl::digest_of(&w[r]), repl::digest_of(vals), "{players} players: player {} holds {r} as the host", g + 2);
                }
            }
        }
    }
}
