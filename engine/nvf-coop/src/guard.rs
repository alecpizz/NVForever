//! The host's checks on what a guest sends: every message from a guest passes
//! [`Guard::check`] after it parses and before the host's session acts on it or passes it
//! on to the other guests ([`crate::net`]); each item of a guest's reliable stream passes
//! [`Guard::admit_item`] before the game has it. No IO here: the session hands over the
//! message, the sender and the time, and drops what fails.
//!
//! The sender is the guest its address belongs to (the session looks it up); an id a
//! message names is never taken as the sender.
//!
//! The built-in rules, in order:
//! - **Rate.** A token bucket per sender address and message [`Kind`] ([`Rate`], a table
//!   with defaults, [`default_rate`], and a setter, [`Guard::set_rate`]); a message over
//!   it is dropped. The reliable stream's items have one per guest and item kind, for
//!   the kinds that have a rate ([`Guard::admit_item`]; kept until the guest says Hello
//!   again or leaves).
//! - **Host-only messages** (Welcome, Full, Who, Names, and the world's: Npcs, Lines,
//!   Presence, Wear, Clock, Scene, Actors) from a guest are dropped.
//! - **Hello**: the name keeps its printable characters only, cut to
//!   [`crate::net::NAME_MAX`] bytes. (The lobby's proof is the session's
//!   [`crate::seam::Admission`]; a wrong one is counted here as [`Reason::Secret`].)
//! - **Look**: at most [`crate::net::LOOK_MAX`] bytes.
//! - **Revive**: its perks keep the bits this build knows ([`crate::net::revive_perk::KNOWN`]).
//! - **Rel**: each item within [`crate::rel::ITEM_MAX`], and the frame within
//!   [`crate::rel::PACKET_BUDGET`] as a sender packs one.
//! - **State**: every float finite, the feet within the world ([`WORLD_XY`], [`WORLD_Z`]),
//!   the yaw wrapped into [0, 2π) and the pitch clamped to ±π/2, at most
//!   [`crate::net::MAX_WORN`] worn items. Then the forms: the weapon, each worn item and
//!   the furniture must be 0 or pass the host's [`FormCheck`]; one that does not is
//!   cleared to 0 (and counted), the rest of the state kept, so a player whose one item
//!   the host's game does not know still moves. Then movement: in the same [`Place`] the
//!   feet may move [`MAX_SPEED`] × the time since the last accepted state, plus
//!   [`SLACK`]. A farther jump is dropped, unless the guest keeps reporting from the new
//!   spot for [`SETTLE`] (a fast travel within one worldspace, a script moving the
//!   player): then it is taken. A change of place (a door, a fast travel to another
//!   worldspace) is taken at once. The accepted state's weapon is the guest's, and the one
//!   it held before, for a while ([`LastState::weapon_before`]).
//!
//! Then the **rules** a layer above registers per kind ([`Guard::add_rule`], [`Rule`]),
//! for messages and for reliable items alike (an item's kind is [`Kind::Item`] of its
//! first byte, the `crate::repl` kind). A rule sees the message or item and the
//! guest's last accepted state ([`Guard::last_state`]). The world's are
//! `crate::world`'s ([`crate::world::add_world_rules`]).
//!
//! Every drop is counted by kind and reason ([`Guard::stats`]), and each drop other than
//! the rate's counts as a bad message against the guest that sent it ([`Guard::bad`]),
//! for a kick policy.

use std::collections::{BTreeMap, HashMap};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::net::{attack, emote, more_flag, revive_perk, Msg, Place, PlayerState, COPIES, LOOK_MAX, MAX_WORN, NAME_MAX, REL_PACKETS, SEND_HZ, TICK};
use crate::rel::{Frame, ITEM_MAX, PACKET_BUDGET};
use crate::repl::kind as repl_kind;

/// The kinds of message, and of reliable item, the guard limits, counts and holds rules
/// for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Hello,
    Welcome,
    Full,
    Who,
    Kicked,
    State,
    Ping,
    Bye,
    Names,
    Look,
    Rel,
    /// The world's ([`crate::net`]): guest to host (and the host's to its guests).
    Hit,
    Unlock,
    Door,
    Revive,
    /// The world's, from the host only.
    Npcs,
    Lines,
    Presence,
    Wear,
    Clock,
    Scene,
    Actors,
    /// An item of the reliable stream, by its first byte (its `crate::repl` kind).
    Item(u8),
}

/// A token bucket's size and refill: `burst` messages at once after a quiet spell,
/// `per_sec` in the long run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rate {
    pub burst: f32,
    pub per_sec: f32,
}

/// Network ticks a second.
const TICKS: f32 = 1000.0 / TICK.as_millis() as f32;

/// The rate each guest may send each kind at, until [`Guard::set_rate`] changes it; None:
/// not limited on its own (a reliable item: its frames are). [design: each well above
/// what a game sends: a state [`SEND_HZ`] times a second, the reliable stream's whole
/// budget ([`REL_PACKETS`] every network tick), a Hello every half second while
/// unanswered, a look every two seconds; half as much again or more on top. The world's
/// guest messages each go [`COPIES`] times, so their rates are in copies: hits as a fast
/// automatic weapon or a shotgun's pellets land, a burst of 40 and 30 a second; unlocks
/// 2 a second, doors 4, revives 1. A guest's reliable items: a carried object's poses
/// (sent 20 times a second) 30 a second, drops and takes 10, a digest's items (one
/// every few seconds, a crowded area's in several) 4, and the syncing screen's ask, the
/// only effect a guest sends, one per 5 seconds: each ask makes the host send the
/// guest's whole area again.]
pub fn default_rate(kind: Kind) -> Option<Rate> {
    let r = |burst: f32, per_sec: f32| Some(Rate { burst, per_sec });
    let copies = |burst: f32, per_sec: f32| r(burst * COPIES as f32, per_sec * COPIES as f32);
    match kind {
        Kind::Hit => copies(40.0, 30.0),
        Kind::Unlock => copies(4.0, 2.0),
        Kind::Door => copies(8.0, 4.0),
        Kind::Revive => copies(3.0, 1.0),
        Kind::Npcs | Kind::Lines | Kind::Presence | Kind::Wear | Kind::Clock | Kind::Scene | Kind::Actors => r(10.0, 10.0),
        Kind::State | Kind::Ping => r(2.0 * SEND_HZ as f32, 3.0 * SEND_HZ as f32),
        Kind::Rel => r(300.0, 1.5 * REL_PACKETS as f32 * TICKS),
        Kind::Hello => r(10.0, 5.0),
        Kind::Look => r(10.0, 5.0),
        Kind::Bye | Kind::Welcome | Kind::Full | Kind::Who | Kind::Kicked | Kind::Names => r(10.0, 10.0),
        Kind::Item(repl_kind::CARRY) => r(60.0, 30.0),
        Kind::Item(repl_kind::LOOSE) => r(20.0, 10.0),
        Kind::Item(repl_kind::DIGEST) => r(16.0, 4.0),
        Kind::Item(repl_kind::EFFECTS) => r(1.0, 0.2),
        Kind::Item(_) => None,
    }
}

/// Token buckets kept, at most, one per sender address and kind [design: the guests' and
/// whoever else is knocking; the bucket heard from least lately goes first].
pub const BUCKETS_MAX: usize = 256;
/// The world's bounds for a player's feet, game units [design: far past any worldspace's
/// cells, which span tens of thousands of units].
pub const WORLD_XY: f32 = 1_000_000.0;
pub const WORLD_Z: f32 = 100_000.0;
/// The highest reliable item kind (an item's first byte, the `crate::repl` kind): any other
/// is refused [design: piece 1's 0x00-0x0F, the world's 0x10-0x2F and shared progress's
/// 0x30-0x3F; a later piece raises it with its own range].
pub const ITEM_KIND_MAX: u8 = 0x3F;
/// The longest an emote's clip time can be, seconds [design].
pub const EMOTE_T_MAX: f32 = 3600.0;
/// The fastest a player's feet move, game units a second [design: several times a
/// sprinting player's speed, with room for a long fall].
pub const MAX_SPEED: f32 = 2_000.0;
/// What a state may move beyond [`MAX_SPEED`] × time [design: states bunched by the
/// network arrive together, a twentieth of a second of movement each].
pub const SLACK: f32 = 500.0;
/// How long a guest keeps reporting from a spot it jumped to before the host takes it
/// [design].
pub const SETTLE: Duration = Duration::from_secs(1);

/// What the game side knows of forms, by the host's load order (the host's game
/// implements it). Without one only the numeric checks are made.
pub trait FormCheck: Send + Sync {
    /// A form a player can hold as a weapon.
    fn carriable(&self, form: u32) -> bool;
    /// A form a player can wear.
    fn wearable(&self, form: u32) -> bool;
    /// A placed reference a player can sit or lie on.
    fn furniture(&self, reference: u32) -> bool;
    /// A place a player can be in: a known interior cell or worldspace.
    fn place(&self, place: Place) -> bool {
        let _ = place;
        true
    }
    /// A playable inventory item a player can drop (ALCH, AMMO, ARMO, BOOK, KEYM, MISC,
    /// WEAP, NOTE, CHIP, CMNY, IMOD; not one the data marks unplayable). By default every
    /// form passes.
    fn item(&self, form: u32) -> bool {
        let _ = form;
        true
    }
}

/// Why a message was dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reason {
    /// Over the sender's rate for its kind.
    Rate,
    /// A message only the host sends.
    HostOnly,
    /// A field out of its bounds (a size, a float that is not finite, feet outside the
    /// world).
    Bounds,
    /// Feet farther from the last accepted state than the guest could have moved.
    Moved,
    /// A Hello without the lobby's proof.
    Secret,
    /// A registered [`Rule`], by its own name.
    Rule(&'static str),
}

/// What a check says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Accept,
    Drop(Reason),
}

/// A guest's last accepted state: where it was, what it held, and when the host took it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LastState {
    pub place: Place,
    pub feet: [f32; 3],
    /// The weapon in hand (0: none), as the state's form checks left it.
    pub weapon: u32,
    /// The weapon it held before this one, and when it changed (None: no change seen):
    /// a hit of a shot fired before the switch may come after it.
    pub weapon_before: Option<(u32, Instant)>,
    pub at: Instant,
}

/// What a [`Rule`] judges by.
pub struct Ctx<'a> {
    /// The guest that sent it (by its address; None: an address the host does not know).
    pub from_id: Option<u8>,
    pub kind: Kind,
    pub now: Instant,
    /// The guest's last accepted state ([`Guard::last_state`]).
    pub last: Option<LastState>,
    pub forms: Option<&'a dyn FormCheck>,
}

/// What a [`Rule`] judges: a message, or an item of the reliable stream.
pub enum Subject<'a> {
    Msg(&'a Msg),
    Item(&'a [u8]),
}

/// A check a layer above adds for one kind ([`Guard::add_rule`]): it runs after the
/// built-in ones, on what they let through.
pub trait Rule: Send + Sync {
    fn check(&self, ctx: &Ctx, subject: &Subject) -> Verdict;
}

/// What the guard dropped so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Messages and items dropped, by kind and reason.
    pub dropped: Vec<(Kind, Reason, u64)>,
    /// Form fields cleared from states that were kept.
    pub forms_cleared: u64,
    /// Bad messages per guest ([`Guard::bad`]).
    pub bad: Vec<(u8, u64)>,
}

struct Bucket {
    tokens: f32,
    at: Instant,
}

/// A guest's last accepted state, and a spot it jumped to.
struct Track {
    last: LastState,
    jump: Option<Jump>,
}

struct Jump {
    feet: [f32; 3],
    since: Instant,
    at: Instant,
}

/// The host's checks (module notes).
#[derive(Default)]
pub struct Guard {
    buckets: HashMap<(SocketAddr, Kind), Bucket>,
    /// Reliable items' buckets, per guest and item kind.
    item_buckets: HashMap<(u8, Kind), Bucket>,
    rates: HashMap<Kind, Option<Rate>>,
    tracks: HashMap<u8, Track>,
    rules: HashMap<Kind, Vec<Box<dyn Rule>>>,
    forms: Option<Arc<dyn FormCheck>>,
    dropped: BTreeMap<(Kind, Reason), u64>,
    forms_cleared: u64,
    bad: HashMap<u8, u64>,
}

impl Guard {
    pub fn new(forms: Option<Arc<dyn FormCheck>>) -> Self {
        Self { forms, ..Self::default() }
    }

    pub fn set_forms(&mut self, forms: Option<Arc<dyn FormCheck>>) {
        self.forms = forms;
    }

    /// Each guest may send messages of `kind` at `rate` from now on.
    pub fn set_rate(&mut self, kind: Kind, rate: Rate) {
        self.rates.insert(kind, Some(rate));
    }

    /// The rate each guest may send `kind` at (None: not limited on its own).
    pub fn rate(&self, kind: Kind) -> Option<Rate> {
        self.rates.get(&kind).copied().unwrap_or_else(|| default_rate(kind))
    }

    /// A check for `kind`, run after the built-in ones and the rules added before it.
    pub fn add_rule(&mut self, kind: Kind, rule: Box<dyn Rule>) {
        self.rules.entry(kind).or_default().push(rule);
    }

    pub fn stats(&self) -> Stats {
        let mut bad: Vec<(u8, u64)> = self.bad.iter().map(|(id, n)| (*id, *n)).collect();
        bad.sort_unstable();
        Stats { dropped: self.dropped.iter().map(|(&(k, r), &n)| (k, r, n)).collect(), forms_cleared: self.forms_cleared, bad }
    }

    /// Guest `id`'s bad messages so far: dropped by a check other than the rate, or not
    /// parsing.
    pub fn bad(&self, id: u8) -> u64 {
        self.bad.get(&id).copied().unwrap_or(0)
    }

    /// Guest `id`'s last accepted state.
    pub fn last_state(&self, id: u8) -> Option<LastState> {
        self.tracks.get(&id).map(|t| t.last)
    }

    /// Guest `id` starts again (a Hello): its next state is taken wherever it is, and
    /// its reliable items' buckets are full again (its stream starts again too).
    pub fn forget(&mut self, id: u8) {
        self.tracks.remove(&id);
        self.item_buckets.retain(|(g, _), _| *g != id);
    }

    /// Guest `id` left (or a new guest takes its slot): everything kept of it goes.
    pub fn drop_guest(&mut self, id: u8) {
        self.forget(id);
        self.bad.remove(&id);
    }

    /// Count a drop of `kind` for `reason`, against guest `from_id` unless it is the rate.
    pub fn note(&mut self, kind: Kind, reason: Reason, from_id: Option<u8>) {
        *self.dropped.entry((kind, reason)).or_default() += 1;
        if let (Some(id), false) = (from_id, reason == Reason::Rate) {
            self.note_bad(id);
        }
    }

    /// Count a bad message against guest `id` (one that did not parse, say).
    pub fn note_bad(&mut self, id: u8) {
        *self.bad.entry(id).or_default() += 1;
    }

    /// Check a guest's message from `from`, sent by guest `peer` (the guest that address
    /// belongs to; None: an address the host does not know yet), at `now`. Accept: act
    /// on it, as it may have been cleaned (a name, a state's forms).
    pub(crate) fn check(&mut self, from: SocketAddr, peer: Option<u8>, msg: &mut Msg, now: Instant) -> Verdict {
        let kind = msg.kind();
        let verdict = match self.judge(from, peer, msg, now) {
            Err(reason) => Verdict::Drop(reason),
            Ok(track) => {
                let v = self.run_rules(peer, kind, &Subject::Msg(msg), now);
                // A state is the guest's last only once every check took it.
                if let (Verdict::Accept, Some(id), Some(last)) = (v, peer, track) {
                    self.tracks.insert(id, Track { last, jump: None });
                }
                v
            }
        };
        if let Verdict::Drop(reason) = verdict {
            self.note(kind, reason, peer);
        }
        verdict
    }

    /// The hook a layer above's checks run in: the rules registered for `kind`, on a
    /// message or item from guest `from_id`. Drops are counted.
    pub fn admit(&mut self, from_id: Option<u8>, kind: Kind, subject: &Subject, now: Instant) -> Verdict {
        let v = self.run_rules(from_id, kind, subject, now);
        if let Verdict::Drop(reason) = v {
            self.note(kind, reason, from_id);
        }
        v
    }

    /// [`Guard::admit`] for an item of guest `id`'s reliable stream, by its kind, after
    /// the guest's rate for that kind (if it has one).
    pub fn admit_item(&mut self, id: u8, item: &[u8], now: Instant) -> Verdict {
        // An empty item has no kind, and a kind past the last is none the host knows: both
        // are refused before any rule sees them.
        let first = item.first().copied();
        let kind = Kind::Item(first.unwrap_or(0));
        if first.is_none_or(|k| k > ITEM_KIND_MAX) {
            self.note(kind, Reason::Bounds, Some(id));
            return Verdict::Drop(Reason::Bounds);
        }
        if let Some(rate) = self.rate(kind) {
            let b = self.item_buckets.entry((id, kind)).or_insert(Bucket { tokens: rate.burst, at: now });
            if !take_token(b, rate, now) {
                self.note(kind, Reason::Rate, Some(id));
                return Verdict::Drop(Reason::Rate);
            }
        }
        self.admit(Some(id), kind, &Subject::Item(item), now)
    }

    fn run_rules(&self, from_id: Option<u8>, kind: Kind, subject: &Subject, now: Instant) -> Verdict {
        let Some(rules) = self.rules.get(&kind) else { return Verdict::Accept };
        let ctx = Ctx { from_id, kind, now, last: from_id.and_then(|id| self.last_state(id)), forms: self.forms.as_deref() };
        rules.iter().map(|r| r.check(&ctx, subject)).find(|v| *v != Verdict::Accept).unwrap_or(Verdict::Accept)
    }

    /// The built-in checks; for a state that passes, what becomes the guest's last.
    fn judge(&mut self, from: SocketAddr, peer: Option<u8>, msg: &mut Msg, now: Instant) -> Result<Option<LastState>, Reason> {
        if !self.within_rate(from, msg.kind(), now) {
            return Err(Reason::Rate);
        }
        match msg {
            Msg::Welcome { .. } | Msg::Full { .. } | Msg::Who | Msg::Kicked { .. } | Msg::Names { .. } => Err(Reason::HostOnly),
            Msg::Npcs { .. } | Msg::Lines { .. } | Msg::Presence { .. } | Msg::Wear { .. } | Msg::Clock(_) | Msg::Scene(_) | Msg::Actors { .. } => Err(Reason::HostOnly),
            Msg::Revive { perks, .. } => {
                *perks &= revive_perk::KNOWN;
                Ok(None)
            }
            // Their checks are the world's rules (`crate::world`).
            Msg::Hit { .. } | Msg::Unlock { .. } | Msg::Door { .. } => Ok(None),
            Msg::Hello { name, .. } => {
                *name = clean_name(name);
                Ok(None)
            }
            Msg::Look { look, .. } => bounded(look.len() <= LOOK_MAX),
            Msg::Rel { frame, .. } => bounded(frame_fits(frame)),
            Msg::State { state, .. } => {
                state_bounds(state)?;
                if self.forms.as_ref().is_some_and(|f| !f.place(state.place)) {
                    return Err(Reason::Bounds);
                }
                self.state_forms(state);
                match peer {
                    Some(id) => self.movement(id, state, now).map(Some),
                    // Not a guest yet: the session asks who it is and drops it.
                    None => Ok(None),
                }
            }
            Msg::Ping | Msg::Bye { .. } => Ok(None),
        }
    }

    /// The token bucket of `from` for `kind`: whether this message is within its rate.
    fn within_rate(&mut self, from: SocketAddr, kind: Kind, now: Instant) -> bool {
        let Some(rate) = self.rate(kind) else { return true };
        let key = (from, kind);
        if !self.buckets.contains_key(&key) && self.buckets.len() >= BUCKETS_MAX {
            if let Some(oldest) = self.buckets.iter().min_by_key(|(_, b)| b.at).map(|(k, _)| *k) {
                self.buckets.remove(&oldest);
            }
        }
        let b = self.buckets.entry(key).or_insert(Bucket { tokens: rate.burst, at: now });
        take_token(b, rate, now)
    }

    /// Clear the forms the host's game says the player cannot have there.
    fn state_forms(&mut self, s: &mut PlayerState) {
        let Some(forms) = self.forms.clone() else { return };
        let mut cleared = 0;
        if s.weapon != 0 && !forms.carriable(s.weapon) {
            s.weapon = 0;
            cleared += 1;
        }
        if s.furniture != 0 && !forms.furniture(s.furniture) {
            s.furniture = 0;
            cleared += 1;
        }
        let worn: Vec<u32> = s.worn().to_vec();
        let kept: Vec<u32> = worn.iter().copied().filter(|&w| w != 0 && forms.wearable(w)).collect();
        if kept.len() != worn.len() {
            cleared += worn.len() - kept.len();
            s.worn = [0; MAX_WORN];
            s.worn[..kept.len()].copy_from_slice(&kept);
            s.worn_len = kept.len() as u8;
        }
        self.forms_cleared += cleared as u64;
    }

    /// Whether guest `id`'s feet could have got here from its last accepted state; the
    /// state to keep as its last if every check takes it.
    fn movement(&mut self, id: u8, s: &PlayerState, now: Instant) -> Result<LastState, Reason> {
        let take = LastState { place: s.place, feet: s.feet, weapon: s.weapon, weapon_before: None, at: now };
        let Some(t) = self.tracks.get_mut(&id) else { return Ok(take) };
        let before = if t.last.weapon != s.weapon { Some((t.last.weapon, now)) } else { t.last.weapon_before };
        let take = LastState { weapon_before: before, ..take };
        if t.last.place != s.place || reachable(t.last.feet, t.last.at, s.feet, now) {
            return Ok(take);
        }
        // A jump: taken once the guest has kept reporting from there for SETTLE.
        match &mut t.jump {
            Some(j) if reachable(j.feet, j.at, s.feet, now) => {
                j.feet = s.feet;
                j.at = now;
                if now.saturating_duration_since(j.since) >= SETTLE {
                    return Ok(take);
                }
            }
            jump => *jump = Some(Jump { feet: s.feet, since: now, at: now }),
        }
        Err(Reason::Moved)
    }
}

/// Refill `b` at `rate` up to `now`; whether it had a token for one more (taken).
fn take_token(b: &mut Bucket, rate: Rate, now: Instant) -> bool {
    let elapsed = now.saturating_duration_since(b.at).as_secs_f32();
    b.tokens = (b.tokens + elapsed * rate.per_sec).min(rate.burst);
    b.at = now;
    if b.tokens >= 1.0 {
        b.tokens -= 1.0;
        true
    } else {
        false
    }
}

fn bounded(ok: bool) -> Result<Option<LastState>, Reason> {
    if ok {
        Ok(None)
    } else {
        Err(Reason::Bounds)
    }
}

/// Finite, within the world, the angles wrapped, the worn list within its bound.
fn state_bounds(s: &mut PlayerState) -> Result<(), Reason> {
    if !s.feet.iter().chain([&s.yaw, &s.pitch, &s.emote.t]).all(|v| v.is_finite()) {
        return Err(Reason::Bounds);
    }
    let [x, y, z] = s.feet;
    if x.abs() > WORLD_XY || y.abs() > WORLD_XY || z.abs() > WORLD_Z || s.worn_len as usize > MAX_WORN {
        return Err(Reason::Bounds);
    }
    // The small enumerations and bit sets stay within what the protocol defines.
    let known_more = more_flag::SWIMMING | more_flag::COMBAT | more_flag::NO_TRAVEL;
    if s.pipboy > 2 || s.attack_kind > attack::POWER_RIGHT || s.emote.phase > emote::ONCE || !(0.0..=EMOTE_T_MAX).contains(&s.emote.t) || s.more & !known_more != 0 {
        return Err(Reason::Bounds);
    }
    s.yaw = s.yaw.rem_euclid(TAU);
    // rem_euclid can round up to TAU itself.
    if s.yaw >= TAU {
        s.yaw = 0.0;
    }
    s.pitch = s.pitch.clamp(-FRAC_PI_2, FRAC_PI_2);
    Ok(())
}

/// Whether feet at `to` at `now` could have come from `from` at `then`.
fn reachable(from: [f32; 3], then: Instant, to: [f32; 3], now: Instant) -> bool {
    let d = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2) + (to[2] - from[2]).powi(2)).sqrt();
    d <= MAX_SPEED * now.saturating_duration_since(then).as_secs_f32() + SLACK
}

/// A name's printable characters, cut to [`NAME_MAX`] bytes on a character boundary.
pub fn clean_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().filter(|c| !c.is_control()) {
        if out.len() + c.len_utf8() > NAME_MAX {
            break;
        }
        out.push(c);
    }
    out
}

/// Whether a reliable frame is one a sender packs ([`crate::rel::Reliable::frames`]):
/// every item within [`ITEM_MAX`], and a frame of more than one item within
/// [`PACKET_BUDGET`] (each item with its 2-byte length).
fn frame_fits(frame: &Frame) -> bool {
    if frame.items.iter().any(|i| i.len() > ITEM_MAX) {
        return false;
    }
    frame.items.len() <= 1 || frame.items.iter().map(|i| i.len() + 2).sum::<usize>() <= PACKET_BUDGET
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::EmoteState;

    fn addr(n: u8) -> SocketAddr {
        SocketAddr::from(([10, 0, 0, n], 5000))
    }

    fn at(x: f32, y: f32) -> PlayerState {
        PlayerState::at(Place::Exterior(0xA001), [x, y, 100.0], 0.5)
    }

    fn state(s: PlayerState) -> Msg {
        Msg::State { id: 1, seq: 1, state: s }
    }

    /// Guest 1's message, from its address.
    fn check(g: &mut Guard, msg: &mut Msg, now: Instant) -> Verdict {
        g.check(addr(1), Some(1), msg, now)
    }

    fn refused(reason: Reason) -> Verdict {
        Verdict::Drop(reason)
    }

    #[test]
    fn a_flood_is_cut_to_the_rate_of_its_kind() {
        let mut g = Guard::default();
        let now = Instant::now();
        let ping = default_rate(Kind::Ping).unwrap();
        let passed = (0..10_000).filter(|_| check(&mut g, &mut Msg::Ping, now) == Verdict::Accept).count();
        assert_eq!(passed, ping.burst as usize);
        assert_eq!(g.stats().dropped, vec![(Kind::Ping, Reason::Rate, 10_000 - ping.burst as u64)]);
        // The rate is not a bad message.
        assert_eq!(g.bad(1), 0);
        // Another kind from the same guest, and another address, have their own buckets.
        assert_eq!(check(&mut g, &mut state(at(0.0, 0.0)), now), Verdict::Accept);
        assert_eq!(g.check(addr(2), Some(2), &mut Msg::Ping, now), Verdict::Accept);
        // Later the bucket is full again, and no fuller; between, it refills at its rate.
        let later = now + Duration::from_secs(10);
        let passed = (0..10_000).filter(|_| check(&mut g, &mut Msg::Ping, later) == Verdict::Accept).count();
        assert_eq!(passed, ping.burst as usize);
        let soon = later + Duration::from_millis(500);
        let passed = (0..10_000).filter(|_| check(&mut g, &mut Msg::Ping, soon) == Verdict::Accept).count();
        assert_eq!(passed, (ping.per_sec / 2.0) as usize);
    }

    #[test]
    fn rates_are_set_per_kind() {
        let mut g = Guard::default();
        let now = Instant::now();
        g.set_rate(Kind::Look, Rate { burst: 2.0, per_sec: 1.0 });
        assert_eq!(g.rate(Kind::Look), Some(Rate { burst: 2.0, per_sec: 1.0 }));
        let look = || Msg::Look { id: 1, look: vec![1] };
        assert_eq!(check(&mut g, &mut look(), now), Verdict::Accept);
        assert_eq!(check(&mut g, &mut look(), now), Verdict::Accept);
        assert_eq!(check(&mut g, &mut look(), now), refused(Reason::Rate));
        assert_eq!(check(&mut g, &mut look(), now + Duration::from_secs(1)), Verdict::Accept);
        assert_eq!(g.rate(Kind::Item(1)), None);
    }

    #[test]
    fn normal_traffic_passes() {
        let mut g = Guard::default();
        let mut now = Instant::now();
        // Ten seconds of states at SEND_HZ, the reliable stream's whole budget each tick, a
        // look every two seconds and a Hello every half second.
        let mut x = 0.0;
        for tick in 0..1000u32 {
            now += TICK;
            if tick % (100 / SEND_HZ) == 0 {
                x += 10.0;
                assert_eq!(check(&mut g, &mut state(at(x, 0.0)), now), Verdict::Accept, "state at tick {tick}");
            }
            if tick % 50 == 0 {
                assert_eq!(check(&mut g, &mut Msg::Hello { token: 1, lobby: 0, proof: 1, name: "A".into() }, now), Verdict::Accept);
            }
            if tick % 200 == 0 {
                assert_eq!(check(&mut g, &mut Msg::Look { id: 1, look: vec![1] }, now), Verdict::Accept);
            }
            for _ in 0..REL_PACKETS {
                let mut rel = Msg::Rel { epoch: 1, frame: Frame { ack: 0, first: 0, items: vec![vec![1; 100]] } };
                assert_eq!(check(&mut g, &mut rel, now), Verdict::Accept, "rel at tick {tick}");
            }
        }
        assert_eq!(g.stats(), Stats::default());
    }

    #[test]
    fn buckets_are_bounded() {
        let mut g = Guard::default();
        let now = Instant::now();
        for i in 0..1000u32 {
            let from = SocketAddr::from(([10, 1, (i >> 8) as u8, i as u8], 1));
            let _ = g.check(from, None, &mut Msg::Ping, now);
        }
        assert_eq!(g.buckets.len(), BUCKETS_MAX);
    }

    #[test]
    fn a_state_not_finite_is_dropped() {
        let now = Instant::now();
        for bad in [
            PlayerState { feet: [f32::NAN, 0.0, 0.0], ..at(0.0, 0.0) },
            PlayerState { feet: [0.0, f32::INFINITY, 0.0], ..at(0.0, 0.0) },
            PlayerState { yaw: f32::NAN, ..at(0.0, 0.0) },
            PlayerState { pitch: f32::NEG_INFINITY, ..at(0.0, 0.0) },
            PlayerState { emote: EmoteState { t: f32::NAN, ..EmoteState::default() }, ..at(0.0, 0.0) },
        ] {
            let mut g = Guard::default();
            assert_eq!(check(&mut g, &mut state(bad), now), refused(Reason::Bounds), "{bad:?}");
            assert_eq!(g.stats().dropped, vec![(Kind::State, Reason::Bounds, 1)]);
            assert_eq!(g.bad(1), 1);
        }
    }

    #[test]
    fn a_state_outside_the_world_is_dropped() {
        let now = Instant::now();
        let mut g = Guard::default();
        assert_eq!(check(&mut g, &mut state(at(WORLD_XY + 1.0, 0.0)), now), refused(Reason::Bounds));
        assert_eq!(check(&mut g, &mut state(at(0.0, -WORLD_XY - 1.0)), now), refused(Reason::Bounds));
        assert_eq!(check(&mut g, &mut state(PlayerState { feet: [0.0, 0.0, WORLD_Z * 2.0], ..at(0.0, 0.0) }), now), refused(Reason::Bounds));
        assert_eq!(check(&mut g, &mut state(PlayerState { worn_len: MAX_WORN as u8 + 1, ..at(0.0, 0.0) }), now), refused(Reason::Bounds));
        assert_eq!(check(&mut g, &mut state(at(WORLD_XY, -WORLD_XY)), now), Verdict::Accept);
    }

    #[test]
    fn a_state_with_values_the_protocol_does_not_have_is_dropped() {
        let now = Instant::now();
        let mut g = Guard::default();
        let bad = [
            PlayerState { pipboy: 3, ..at(0.0, 0.0) },
            PlayerState { attack_kind: attack::POWER_RIGHT + 1, ..at(0.0, 0.0) },
            PlayerState { emote: EmoteState { phase: emote::ONCE + 1, ..EmoteState::default() }, ..at(0.0, 0.0) },
            PlayerState { emote: EmoteState { t: -1.0, ..EmoteState::default() }, ..at(0.0, 0.0) },
            PlayerState { emote: EmoteState { t: EMOTE_T_MAX + 1.0, ..EmoteState::default() }, ..at(0.0, 0.0) },
            PlayerState { more: 8, ..at(0.0, 0.0) },
            PlayerState { more: 0x80, ..at(0.0, 0.0) },
        ];
        for s in bad {
            assert_eq!(check(&mut g, &mut state(s), now), refused(Reason::Bounds), "{s:?}");
        }
        let good = PlayerState { pipboy: 2, attack_kind: attack::POWER_RIGHT, emote: EmoteState { phase: emote::ONCE, t: EMOTE_T_MAX, ..EmoteState::default() }, more: 7, ..at(0.0, 0.0) };
        assert_eq!(check(&mut g, &mut state(good), now), Verdict::Accept);
    }

    #[test]
    fn a_reliable_item_of_no_known_kind_is_dropped() {
        let now = Instant::now();
        let mut g = Guard::default();
        assert_eq!(g.admit_item(1, &[], now), refused(Reason::Bounds));
        assert_eq!(g.admit_item(1, &[ITEM_KIND_MAX + 1, 0], now), refused(Reason::Bounds));
        assert_eq!(g.admit_item(1, &[0xFF], now), refused(Reason::Bounds));
        assert_eq!(g.admit_item(1, &[ITEM_KIND_MAX, 0], now), Verdict::Accept);
        assert_eq!(g.admit_item(1, &[0], now), Verdict::Accept);
        let refused: u64 = g.stats().dropped.iter().filter(|&&(k, r, _)| matches!(k, Kind::Item(_)) && r == Reason::Bounds).map(|&(_, _, n)| n).sum();
        assert_eq!(refused, 3);
    }

    #[test]
    fn angles_are_wrapped_and_clamped() {
        let mut g = Guard::default();
        let mut msg = state(PlayerState { yaw: -FRAC_PI_2, pitch: 3.0, ..at(0.0, 0.0) });
        assert_eq!(check(&mut g, &mut msg, Instant::now()), Verdict::Accept);
        let Msg::State { state: s, .. } = msg else { unreachable!() };
        assert!((s.yaw - 3.0 * FRAC_PI_2).abs() < 1e-5, "{}", s.yaw);
        assert_eq!(s.pitch, FRAC_PI_2);
    }

    #[test]
    fn walking_passes_and_a_teleport_in_the_same_place_does_not() {
        let mut g = Guard::default();
        let mut now = Instant::now();
        assert_eq!(check(&mut g, &mut state(at(0.0, 0.0)), now), Verdict::Accept);
        // Walking and running: a few hundred units a second.
        for i in 1..=40 {
            now += Duration::from_millis(50);
            assert_eq!(check(&mut g, &mut state(at(i as f32 * 25.0, 0.0)), now), Verdict::Accept, "step {i}");
        }
        assert_eq!(g.last_state(1).map(|l| l.feet), Some([1_000.0, 0.0, 100.0]));
        // Across the map in one twentieth of a second.
        now += Duration::from_millis(50);
        assert_eq!(check(&mut g, &mut state(at(50_000.0, 0.0)), now), refused(Reason::Moved));
        assert_eq!(g.stats().dropped, vec![(Kind::State, Reason::Moved, 1)]);
        assert_eq!(g.last_state(1).map(|l| l.feet), Some([1_000.0, 0.0, 100.0]));
        // The next walking state, near where it was, still passes.
        assert_eq!(check(&mut g, &mut state(at(1_010.0, 0.0)), now), Verdict::Accept);
    }

    #[test]
    fn the_last_state_keeps_the_weapon_and_the_one_before() {
        let mut g = Guard::default();
        let now = Instant::now();
        let held = |w: u32, x: f32| state(PlayerState { weapon: w, ..at(x, 0.0) });
        assert_eq!(check(&mut g, &mut held(0x10, 0.0), now), Verdict::Accept);
        assert_eq!(g.last_state(1).map(|l| (l.weapon, l.weapon_before)), Some((0x10, None)));
        let later = now + Duration::from_millis(50);
        assert_eq!(check(&mut g, &mut held(0x20, 1.0), later), Verdict::Accept);
        assert_eq!(g.last_state(1).map(|l| (l.weapon, l.weapon_before)), Some((0x20, Some((0x10, later)))));
        // The same weapon again keeps the switch as it was.
        assert_eq!(check(&mut g, &mut held(0x20, 2.0), later + Duration::from_millis(50)), Verdict::Accept);
        assert_eq!(g.last_state(1).map(|l| l.weapon_before), Some(Some((0x10, later))));
    }

    #[test]
    fn a_revives_perks_keep_the_known_bits() {
        let mut g = Guard::default();
        let mut msg = Msg::Revive { target: 0, by: 1, perks: 0xFF };
        assert_eq!(check(&mut g, &mut msg, Instant::now()), Verdict::Accept);
        assert_eq!(msg, Msg::Revive { target: 0, by: 1, perks: revive_perk::KNOWN });
    }

    #[test]
    fn a_change_of_place_is_taken_at_once() {
        let mut g = Guard::default();
        let now = Instant::now();
        assert_eq!(check(&mut g, &mut state(at(0.0, 0.0)), now), Verdict::Accept);
        let inside = PlayerState { place: Place::Interior(0x0010_4C1C), ..at(90_000.0, -40_000.0) };
        assert_eq!(check(&mut g, &mut state(inside), now + Duration::from_millis(50)), Verdict::Accept);
        assert_eq!(g.last_state(1).map(|l| l.place), Some(Place::Interior(0x0010_4C1C)));
        // And back out, far from where it went in.
        assert_eq!(check(&mut g, &mut state(at(-70_000.0, 3_000.0)), now + Duration::from_millis(100)), Verdict::Accept);
    }

    #[test]
    fn a_jump_the_guest_keeps_reporting_from_is_taken_after_it_settles() {
        let mut g = Guard::default();
        let mut now = Instant::now();
        assert_eq!(check(&mut g, &mut state(at(0.0, 0.0)), now), Verdict::Accept);
        let mut taken = None;
        for i in 1..=40 {
            now += Duration::from_millis(50);
            if check(&mut g, &mut state(at(60_000.0 + i as f32, 0.0)), now) == Verdict::Accept {
                taken = Some(i);
                break;
            }
        }
        // The first report starts the clock; SETTLE later it is taken.
        assert_eq!(taken, Some(1 + (SETTLE.as_millis() / 50) as i32));
        // A guest forgotten (a Hello) is taken anywhere.
        g.forget(1);
        assert_eq!(check(&mut g, &mut state(at(-90_000.0, 0.0)), now), Verdict::Accept);
    }

    struct Forms;

    impl FormCheck for Forms {
        fn carriable(&self, form: u32) -> bool {
            form == 0x0E37
        }
        fn wearable(&self, form: u32) -> bool {
            form < 0x100
        }
        fn furniture(&self, reference: u32) -> bool {
            reference == 0x0011_C601
        }
        fn place(&self, place: Place) -> bool {
            place != Place::Interior(0x0BAD)
        }
    }

    #[test]
    fn a_place_the_host_does_not_know_is_dropped() {
        let mut g = Guard::new(Some(Arc::new(Forms)));
        let now = Instant::now();
        let s = PlayerState { place: Place::Interior(0x0BAD), ..at(0.0, 0.0) };
        assert_eq!(check(&mut g, &mut state(s), now), refused(Reason::Bounds));
        assert_eq!(check(&mut g, &mut state(at(0.0, 0.0)), now), Verdict::Accept);
    }

    #[test]
    fn forms_the_host_does_not_allow_are_cleared() {
        let mut g = Guard::new(Some(Arc::new(Forms)));
        let now = Instant::now();
        let mut s = at(0.0, 0.0);
        s.weapon = 0x0BAD;
        s.furniture = 0x0BAD;
        s.worn[..3].copy_from_slice(&[0x10, 0x0BAD, 0x20]);
        s.worn_len = 3;
        let mut msg = state(s);
        assert_eq!(check(&mut g, &mut msg, now), Verdict::Accept);
        let Msg::State { state: s, .. } = msg else { unreachable!() };
        assert_eq!((s.weapon, s.furniture), (0, 0));
        assert_eq!(s.worn(), &[0x10, 0x20]);
        assert_eq!(g.stats().forms_cleared, 3);
        // What the host allows stays, and 0 is always allowed.
        let mut s = at(1.0, 0.0);
        s.weapon = 0x0E37;
        s.furniture = 0x0011_C601;
        let mut msg = state(s);
        assert_eq!(check(&mut g, &mut msg, now), Verdict::Accept);
        assert_eq!(msg, state(s));
        assert_eq!(check(&mut g, &mut state(at(2.0, 0.0)), now), Verdict::Accept);
        assert_eq!(g.stats().forms_cleared, 3);
        // Without a check, only the numbers are looked at.
        let mut g = Guard::default();
        let mut s = at(0.0, 0.0);
        s.weapon = 0x0BAD;
        let mut msg = state(s);
        assert_eq!(check(&mut g, &mut msg, now), Verdict::Accept);
        assert_eq!(msg, state(s));
    }

    #[test]
    fn a_hellos_name_keeps_its_printable_characters() {
        let mut g = Guard::default();
        let mut msg = Msg::Hello { token: 1, lobby: 0, proof: 1, name: "Co\u{7}u\nri\u{1b}[31mer".into() };
        assert_eq!(check(&mut g, &mut msg, Instant::now()), Verdict::Accept);
        assert_eq!(msg, Msg::Hello { token: 1, lobby: 0, proof: 1, name: "Couri[31mer".into() });
        assert_eq!(clean_name(&"é".repeat(40)).len(), NAME_MAX);
        assert!(clean_name(&"é".repeat(40)).chars().all(|c| c == 'é'));
    }

    #[test]
    fn looks_and_reliable_frames_keep_to_their_sizes() {
        let mut g = Guard::default();
        let now = Instant::now();
        assert_eq!(check(&mut g, &mut Msg::Look { id: 1, look: vec![0; LOOK_MAX] }, now), Verdict::Accept);
        assert_eq!(check(&mut g, &mut Msg::Look { id: 1, look: vec![0; LOOK_MAX + 1] }, now), refused(Reason::Bounds));
        let rel = |items: Vec<Vec<u8>>| Msg::Rel { epoch: 0, frame: Frame { ack: 0, first: 0, items } };
        assert_eq!(check(&mut g, &mut rel(vec![vec![0; ITEM_MAX]]), now), Verdict::Accept);
        assert_eq!(check(&mut g, &mut rel(vec![vec![0; 500], vec![0; 500]]), now), Verdict::Accept);
        assert_eq!(check(&mut g, &mut rel(Vec::new()), now), Verdict::Accept);
        assert_eq!(check(&mut g, &mut rel(vec![vec![0; ITEM_MAX + 1]]), now), refused(Reason::Bounds));
        assert_eq!(check(&mut g, &mut rel(vec![vec![0; 600], vec![0; 600]]), now), refused(Reason::Bounds));
        assert_eq!(check(&mut g, &mut rel(vec![Vec::new(); PACKET_BUDGET]), now), refused(Reason::Bounds));
    }

    #[test]
    fn messages_only_the_host_sends_are_dropped() {
        let mut g = Guard::default();
        let now = Instant::now();
        for mut msg in [Msg::Welcome { id: 1, proof: 0 }, Msg::Full { proof: 0 }, Msg::Who, Msg::Names { names: vec![(0, "Host".into())] }] {
            assert_eq!(check(&mut g, &mut msg, now), refused(Reason::HostOnly), "{msg:?}");
        }
        assert_eq!(g.bad(1), 4);
        assert_eq!(g.stats().dropped.iter().map(|d| d.2).sum::<u64>(), 4);
        for mut msg in [Msg::Ping, Msg::Bye { id: 1 }] {
            assert_eq!(check(&mut g, &mut msg, now), Verdict::Accept);
        }
        // A guest that leaves takes its count with it.
        g.drop_guest(1);
        assert_eq!(g.bad(1), 0);
    }

    /// A rule that drops a state lower than the guest's last.
    struct NoFalling;

    impl Rule for NoFalling {
        fn check(&self, ctx: &Ctx, subject: &Subject) -> Verdict {
            match (subject, ctx.last) {
                (Subject::Msg(Msg::State { state, .. }), Some(last)) if state.feet[2] < last.feet[2] => Verdict::Drop(Reason::Rule("falling")),
                _ => Verdict::Accept,
            }
        }
    }

    /// A rule that drops reliable items of one byte.
    struct NoShortItems;

    impl Rule for NoShortItems {
        fn check(&self, ctx: &Ctx, subject: &Subject) -> Verdict {
            assert_eq!(ctx.kind, Kind::Item(4));
            match subject {
                Subject::Item(item) if item.len() < 2 => Verdict::Drop(Reason::Rule("short")),
                _ => Verdict::Accept,
            }
        }
    }

    #[test]
    fn rules_run_after_the_built_in_checks_and_see_the_last_state() {
        let mut g = Guard::default();
        g.add_rule(Kind::State, Box::new(NoFalling));
        g.add_rule(Kind::Item(4), Box::new(NoShortItems));
        let now = Instant::now();
        assert_eq!(check(&mut g, &mut state(at(0.0, 0.0)), now), Verdict::Accept);
        let lower = PlayerState { feet: [0.0, 0.0, 50.0], ..at(0.0, 0.0) };
        assert_eq!(check(&mut g, &mut state(lower), now), refused(Reason::Rule("falling")));
        // A state a rule dropped is not the guest's last.
        assert_eq!(g.last_state(1).map(|l| l.feet[2]), Some(100.0));
        // Built-in checks come first: a state out of bounds never reaches the rule.
        assert_eq!(check(&mut g, &mut state(at(WORLD_XY * 2.0, 0.0)), now), refused(Reason::Bounds));
        // Items by their kind; other kinds have no rule.
        assert_eq!(g.admit_item(1, &[4], now), refused(Reason::Rule("short")));
        assert_eq!(g.admit_item(1, &[4, 0], now), Verdict::Accept);
        assert_eq!(g.admit_item(1, &[5], now), Verdict::Accept);
        let stats = g.stats();
        assert_eq!(stats.dropped, vec![(Kind::State, Reason::Bounds, 1), (Kind::State, Reason::Rule("falling"), 1), (Kind::Item(4), Reason::Rule("short"), 1)]);
        assert_eq!(stats.bad, vec![(1, 3)]);
    }
}
