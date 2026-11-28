//! The co-op session: a host and up to [`MAX_GUESTS`] guests, over a
//! [`Transport`] ([`Session::host_on`], [`Session::join_on`]; no real transport is in this
//! part, see [`crate::seam`]). Peers are known by their [`SocketAddr`].
//!
//! The host is the hub: each game sends its own player's [`PlayerState`] to the host
//! [`SEND_HZ`] times a second, and the host passes every guest's state on to the other
//! guests along with its own (peer 0); so it does each player's look, and every player's
//! name ([`Msg::Names`]). A reliable, ordered stream runs between the host and each guest
//! ([`crate::rel`], [`Session::send_reliable`]) for the layer above. Nothing here knows
//! about Bevy.
//!
//! **The host runs the world.** The host's NPCs are the only ones that think: it sends
//! each one it draws ([`NpcState`]) [`NPC_HZ`] times a second, and the guests draw
//! theirs there; the moving ones also go to each guest as compact poses
//! ([`Session::send_actors`], [`ActorPose`]). A guest's hits on NPCs go to the host
//! ([`Session::send_hit`], [`HitView`]), which applies them in its world; what they do
//! comes back in the states. The NPCs' hits on a guest go the other way
//! ([`Session::send_hit_to`]). The host's game clock goes to the guests ([`ClockView`],
//! [`CLOCK_EVERY`]), so do the lines its NPCs are saying ([`LineView`], every
//! [`LINES_EVERY`] and at once when they change), what its scripts do to its player
//! ([`SceneView`]), its world's presence and its doors' open flags, page by page
//! ([`Msg::Presence`]), and what its NPCs wear ([`Msg::Wear`]). A door, container or
//! terminal a player unlocks goes to everyone through the host ([`Session::send_unlock`]),
//! and so does a door one opens or closes ([`Session::send_door`]); a player another
//! revives is told so through the host ([`Session::send_revive`]). The host's session
//! passes none of a guest's on by itself: its game checks them against its world and
//! then passes on what passes ([`Session::relay_unlock`], [`Session::relay_door`],
//! [`Session::relay_revive`]; [`Session::note_bad`] counts what does not) [design].
//!
//! **Who may join.** A guest's Hello carries the lobby's proof ([`Admission`]; the
//! lobby's secret, [`crate::seam::SharedSecret`]); the host drops a Hello without it
//! before anything else, a returning guest's token included. The host's answer proves the
//! secret back, and a guest takes as its host only an address whose answer does.
//!
//! **The host checks every guest's message** ([`crate::guard`]) before acting on it or
//! passing it on: a rate per sender and kind, bounds, movement, forms, and the rules a
//! layer above adds ([`Session::add_rule`]). The sender is the guest its address belongs
//! to, never an id the message names.
//!
//! **Dropouts are not disconnects.** A peer that goes quiet for [`LOST_AFTER`] is only
//! *lost*: its slot, id and player stay, and it is dropped only after [`GRACE`]. A guest
//! that stops hearing the host keeps sending Hello with its session token, and the host
//! gives a known token its old id back, from whatever address it now comes from (a
//! Wi-Fi blip, a router that changed the port). Only a deliberate Bye drops a peer at once.
//!
//! **Loading screens are not dropouts either.** [`Link`] runs the session on its own
//! thread, so heartbeats keep going while the game's frame is stalled loading a cell; the
//! game only hands it its newest state and takes the events.
//!
//! **Everything a peer can make grow is bounded**: the peers ([`MAX_GUESTS`]), the
//! per-address byte counts ([`TRAFFIC_ADDRS`]), the events waiting for the game
//! ([`EVENTS_MAX`]; a state or a look replaces the one before it), each reliable stream's
//! queue ([`crate::rel::MAX_UNACKED`]) and every list on the wire.
//!
//! Packets are little-endian: the magic `NVCO`, the protocol version, a kind byte, then
//! the kind's fields. A packet that does not parse is dropped.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher, Hasher};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::guard::{FormCheck, Guard, Kind, Rate, Reason, Rule, Stats, Verdict};
use crate::rel::{Frame, Reliable, ITEM_MAX, PACKET_BUDGET};
use crate::seam::{Admission, Admit, Transport};

pub use crate::repl::ActorPose;
use crate::repl::{ACTORS_PER_PACKET, ACTOR_BYTES};

pub const MAGIC: [u8; 4] = *b"NVCO";
/// The protocol's version: a packet of another is dropped. 138 is the core protocol on
/// its own (players and the reliable stream; the Hello and the host's answers carry the
/// lobby's proof), so no game of an earlier protocol joins it, nor it them. 139 adds the
/// world's messages (kinds 9, 10, 12-14, 17, 19, 20, 23, 24, 31), with a hit carried as
/// its fields ([`HitView`]) rather than the game's own bytes. 140 is the internet transport
/// under it ([`crate::stack`]: every packet sealed with Noise, its own kinds 0xE0-0xE8 under
/// the same magic and version) and the host's Kick ([`Msg::Kicked`], kind 40).
pub const VERSION: u8 = 140;
pub const MAX_GUESTS: usize = 5;
pub const SEND_HZ: u32 = 20;
/// A peer that sends nothing for this long is lost (shown frozen, still in its slot).
pub const LOST_AFTER: Duration = Duration::from_secs(4);
/// A lost peer that has not come back after this long is dropped, and a guest stops trying.
pub const GRACE: Duration = Duration::from_secs(120);
/// How often a guest sends Hello while it has no answer.
const HELLO_EVERY: Duration = Duration::from_millis(500);
/// The network thread's tick.
pub const TICK: Duration = Duration::from_millis(10);
/// The longest name, in bytes.
pub const NAME_MAX: usize = 32;
/// IP and UDP headers, counted with each packet's bytes in [`Session::traffic`].
const UDP_OVERHEAD: u64 = 28;
/// The most reliable packets one stream sends per network tick ([`TICK`]): about 110 KB/s
/// while a join's snapshot goes, far less after.
pub const REL_PACKETS: usize = 4;
/// Items one reliable frame carries at most on the wire (each takes at least its 2-byte
/// length within [`PACKET_BUDGET`]).
pub const REL_ITEMS_MAX: usize = PACKET_BUDGET / 2;
/// How often the host sends every player's name ([`Msg::Names`]), and at once after a
/// guest joins.
pub const NAMES_EVERY: Duration = Duration::from_secs(2);
/// A message the other side must not miss and takes once (a hit, an unlock, a door, a
/// revive) is sent this many times [design: one burst of loss does not take them all].
pub const COPIES: usize = 3;
/// A goodbye is sent this many times ([`Session::bye`]) [design: as [`COPIES`]].
pub const BYE_COPIES: usize = COPIES;
/// Between the copies of a goodbye [design: apart enough that one burst of loss does not
/// take them all, short enough that quitting does not wait].
const BYE_GAP: Duration = Duration::from_millis(15);
/// Addresses whose bytes are counted ([`Session::traffic`]), at most: past it, those not
/// of a current peer are forgotten [design: the peers and a few the guest still tries].
pub const TRAFFIC_ADDRS: usize = MAX_GUESTS + 4;
/// Events waiting for the game, at most [design: a game takes them every frame; a stalled
/// one still has room for a long load]. A state or a look replaces the same player's one
/// still waiting; past the bound other events are dropped (and counted), and reliable
/// frames are not taken (nor acknowledged) until the game takes its events, so the
/// sender keeps them.
pub const EVENTS_MAX: usize = 4096;

/// Where a player is: an interior cell, or a worldspace (exterior), by FormID in the
/// game's load order. Both games must run the same load order for the IDs to agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Interior(u32),
    Exterior(u32),
}

/// The most a look takes (the game's own encoding, [`Session::set_look`]), and how often it
/// is sent again (it changes rarely; a lost one is made up for by the next).
pub const LOOK_MAX: usize = 1200;
pub const LOOK_EVERY: Duration = Duration::from_secs(2);
/// States this far behind the last are from a game started again (10 s of them).
const RESTARTED: u32 = 200;
/// A reliable frame of a stream this few behind the guest's is one late on the way (each
/// Hello the host hears starts one more); any other number is a new stream [design].
const EPOCHS_LATE: u16 = 8;
/// Worn items sent with a state, at most.
pub const MAX_WORN: usize = 10;

/// An NPC of the host's world as the host draws it (module notes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NpcState {
    /// The placed reference: the same in every game, as they run the same load order.
    pub reference: u32,
    /// Feet position, game units, Z up.
    pub feet: [f32; 3],
    /// Facing, radians (compass: 0 north, π/2 east).
    pub heading: f32,
    /// Its aim up or down while it fights, radians.
    pub pitch: f32,
    /// [`npc_flag`] bits.
    pub flags: u8,
    /// The weapon it aims (WEAP FormID), 0 for none.
    pub weapon: u32,
    /// Attacks begun (wrapping), counted as [`PlayerState::attacks`] are.
    pub attacks: u8,
    /// The furniture reference it is using or walking up to (a chair, a bed), 0 for
    /// none; [`npc_flag::SEATED`] once it is on it.
    pub furniture: u32,
    /// Its base form as the host's world has it (an NPC, a creature or a leveled list of
    /// them): what a guest makes it of when the host's game created it (a dynamic
    /// reference the guest's world does not have).
    pub base: u32,
    /// The level its leveled lists are read at in the host's game, and whether every
    /// entry at or under it is eligible; level 0: none told. Under 32768.
    pub level: u16,
    pub all_levels: bool,
    /// How many times its leveled picks were rolled again (a resurrection), up to 127.
    /// With [`Self::level`], so a guest draws the leveled pick the host draws.
    pub rolls: u8,
    /// Hostile to the players in the host's game: fighting one or ready to attack one,
    /// for a guest's compass. Sent in the top bit of [`Self::level`].
    pub hostile: bool,
    /// Where it is in the host's world: its interior cell, or the worldspace of the
    /// exterior it stands in (0: not told). A guest draws it only there [observed:
    /// buckallobill, in game, FNV 1.4.0.525: drawn wherever the guest was, an interior's
    /// NPCs hung in the air outside at the interior's coordinates].
    pub space: u32,
}

impl NpcState {
    pub fn has(&self, f: u8) -> bool {
        self.flags & f != 0
    }
}

/// [`NpcState::flags`] bits.
pub mod npc_flag {
    pub const MOVING: u8 = 1;
    /// The weapon is out.
    pub const DRAWN: u8 = 2;
    pub const SNEAKING: u8 = 4;
    pub const DEAD: u8 = 8;
    /// Fighting: aiming at someone.
    pub const AIMING: u8 = 16;
    pub const SIGHTS: u8 = 32;
    /// On its furniture ([`super::NpcState::furniture`]), not walking up to it.
    pub const SEATED: u8 = 64;
    /// Knocked down or out and lying: the others' copies lie until it clears, then get up.
    pub const KNOCKED: u8 = 128;
}

/// NPC states in one packet (45 bytes each: under 1200 bytes in all).
pub const NPCS_PER_PACKET: usize = 26;
/// How often the host sends its NPCs.
pub const NPC_HZ: u32 = 10;
/// How often the host sends the lines its NPCs are saying (and at once when they change).
pub const LINES_EVERY: Duration = Duration::from_millis(250);
/// Lines sent at most.
pub const LINES_MAX: usize = 24;
/// How often the host sends a page of its world's presence ([`Msg::Presence`]).
pub const PRESENCE_EVERY: Duration = Duration::from_millis(200);
/// References one presence page carries at most (5 bytes each: 1000 bytes).
pub const PRESENCE_PAGE: usize = 200;
/// The host's lists ([`Msg::Presence`]): each placed reference whose own enabled flag
/// differs from its plugin's ...
pub const LIST_ENABLED: u8 = 0;
/// ... and each whose open flag (a door) differs from its plugin's.
pub const LIST_OPEN: u8 = 1;
/// How many lists there are.
pub const LISTS: usize = 2;
/// How often the host sends a page of what its NPCs wear ([`Msg::Wear`]).
pub const WEAR_EVERY: Duration = Duration::from_millis(250);
/// NPCs one wear page carries at most [design: an entry is at most 69 bytes with
/// [`WEAR_ITEMS`], so a full page stays within one packet].
pub const WEAR_PAGE: usize = 16;
/// Items one NPC's entry carries at most (its worn armour and its weapon).
pub const WEAR_ITEMS: usize = 16;
/// How often the host sends its game clock.
pub const CLOCK_EVERY: Duration = Duration::from_secs(2);
/// How often the host sends its [`SceneView`] when it has not changed.
pub const SCENE_EVERY: Duration = Duration::from_millis(500);
/// Image space modifiers (and worn items) a [`SceneView`] carries at most.
pub const SCENE_MODS_MAX: usize = 16;
/// Texture swaps a [`SceneView`] carries at most, and the longest shape name or texture
/// path one may have (bytes); a longer one is not sent.
pub const SCENE_SWAPS_MAX: usize = 8;
pub const SWAP_TEXT_MAX: usize = 200;
/// The most a [`SceneView`] takes on the wire: past it, its last swaps are not sent
/// [design: within one packet; a slideshow's swaps are far shorter].
pub const SCENE_BYTES_MAX: usize = 1200;
/// Hit ids remembered per sender, to take each hit once ([`COPIES`]).
pub const HITS_KEPT: usize = 64;
/// The copies of one unlock, door or revive ([`COPIES`]) go in one burst: another the
/// same within this long is one of them [design].
pub const COPY_WINDOW: Duration = Duration::from_secs(1);

/// What one sender's unlocks, doors and revives said lately ([`COPY_WINDOW`]), to take
/// each once: they carry no id as a hit does, and the host checked and passed on each of
/// the three copies otherwise, each guest hearing it nine times [design].
#[derive(Default)]
struct Copies {
    seen: VecDeque<(Instant, (u8, u32, u8))>,
}

impl Copies {
    /// Whether `key` (the message's kind, reference and flag) is new here (and note it).
    fn first(&mut self, key: (u8, u32, u8), now: Instant) -> bool {
        while self.seen.front().is_some_and(|(at, _)| now.saturating_duration_since(*at) >= COPY_WINDOW) {
            self.seen.pop_front();
        }
        // Only the newest entry for this kind and reference counts: a door that goes open,
        // close, open inside the window is three events, not two and a copy [design].
        if self.seen.iter().rev().find(|(_, k)| k.0 == key.0 && k.1 == key.1).is_some_and(|(_, k)| *k == key) {
            return false;
        }
        self.seen.retain(|(_, k)| !(k.0 == key.0 && k.1 == key.1));
        if self.seen.len() >= HITS_KEPT {
            self.seen.pop_front();
        }
        self.seen.push_back((now, key));
        true
    }
}

/// [`Copies`]' keys.
fn unlock_key(reference: u32, terminal: bool) -> (u8, u32, u8) {
    (0, reference, u8::from(terminal))
}

fn door_key(reference: u32, open: bool) -> (u8, u32, u8) {
    (1, reference, u8::from(open))
}

fn revive_key(target: u8, by: u8, perks: u8) -> (u8, u32, u8) {
    (2, u32::from(target) | u32::from(by) << 8, perks)
}

/// [`Msg::Revive`]'s `perks` bits: the reviver's co-op perks that act on the revived.
pub mod revive_perk {
    /// Healing Hands: the revived is cured of all radiation.
    pub const CURE_RADS: u8 = 1;
    /// Every bit this build knows; the host clears the others (`crate::guard`).
    pub const KNOWN: u8 = CURE_RADS;
}

/// A hit as it goes between games: a guest's on the host's world (to the host), or the
/// host's NPCs' on a guest's player (to that guest). `target` is the reference hit (in
/// the load order every game runs), `dealt` the damage the hitter's game rolled,
/// `weapon` the WEAP FormID (0: none, unarmed), `location` the body part hit (-1: none;
/// [`HitView::OBJECT`]: a placed object that is not an actor), and `limb` what the hit
/// did to the part's condition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitView {
    pub target: u32,
    pub dealt: f32,
    pub weapon: u32,
    pub location: i32,
    pub critical: bool,
    pub limb: Option<LimbView>,
}

impl HitView {
    /// [`HitView::location`] of a shot on a placed object that is not an actor: its hit
    /// blocks run on the host.
    pub const OBJECT: i32 = -1000;
    /// [`HitView::location`] of a hit on no body part.
    pub const NO_PART: i32 = -1;
}

/// A hit's effect on one limb: the condition's actor value, its health left (percent),
/// the damage to it, and whether it counts toward the limb's points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LimbView {
    pub condition: u16,
    pub health_percent: f64,
    pub damage: f64,
    pub points: bool,
}

/// The host's game clock, as its time globals hold it (GameYear, GameMonth, GameDay,
/// GameHour, GameDaysPassed): the guests set theirs to it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClockView {
    pub year: f32,
    pub month: f32,
    pub day: f32,
    pub hour: f32,
    pub days_passed: f32,
}

/// What the host's scripts do to the host's player now, for the guests to share: the
/// control groups held (`locks`, bit `i` for group `i`: movement, Pip-Boy, fighting,
/// POV, looking, rollover, sneaking) and the image space modifiers on the screen (IMAD
/// FormID, seconds since applied), at most [`SCENE_MODS_MAX`]; what the host's scripts
/// put on the host's player and it still wears (`worn`: item FormID, locked on), at most
/// [`SCENE_MODS_MAX`]; and, while a group is held, the textures its scripts swapped on
/// references' shapes (`swaps`: reference FormID, shape name, texture path; a
/// slideshow's slides), at most [`SCENE_SWAPS_MAX`], each string at most
/// [`SWAP_TEXT_MAX`] bytes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneView {
    pub locks: u8,
    pub mods: Vec<(u32, f32)>,
    pub worn: Vec<(u32, bool)>,
    pub swaps: Vec<(u32, String, String)>,
}

/// A line one of the host's NPCs is saying, as the host's game has it: the guests say it
/// too. References are the load order's, which every game runs; the host's player is
/// [`LineView::HOST`], a guest's actor in the host's world is [`LineView::guest`] of its id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineView {
    pub speaker: u32,
    /// The listener, 0 for none.
    pub target: u32,
    pub topic: u32,
    pub info: u32,
    /// Seconds (game time) left until it ends, as the host sent it.
    pub remaining: f32,
    /// [`LineView::SAY_TO`] and the others.
    pub flags: u8,
    /// A line of a conversation between two actors: its number in it, 0 for none.
    pub conversation: u16,
}

impl LineView {
    pub const SAY_TO: u8 = 1;
    pub const GREETING: u8 = 2;
    pub const HELLO: u8 = 4;
    pub const FORCE_SUBTITLES: u8 = 8;
    pub const TWO_D: u8 = 16;
    /// The host's player as a speaker or listener.
    pub const HOST: u32 = 0xFE00_C000;

    /// Player `id` as a speaker or listener (the host is id 0: [`LineView::HOST`]).
    pub fn guest(id: u8) -> u32 {
        Self::HOST + id as u32
    }

    pub fn has(&self, f: u8) -> bool {
        self.flags & f != 0
    }
}

/// [`PlayerState::flags`] bits.
pub mod flag {
    /// The weapon is out (drawn, not holstered).
    pub const DRAWN: u8 = 1;
    /// Aiming (the Block/aim button held).
    pub const AIMING: u8 = 2;
    pub const SNEAKING: u8 = 4;
    /// Off the ground (jumping, falling).
    pub const AIR: u8 = 8;
    /// Looking down the sights or a scope.
    pub const SIGHTS: u8 = 16;
    /// Dead: the world's actors leave this player alone.
    pub const DOWN: u8 = 32;
    /// The flashlight is on.
    pub const LIGHT: u8 = 64;
    /// Revived, the body still lies before getting up (the others' copies of it start
    /// their get-up when this clears).
    pub const LYING: u8 = 128;
}

/// [`PlayerState::attack_kind`]: what the newest attack was.
pub mod attack {
    pub const SHOT: u8 = 0;
    pub const SWING: u8 = 1;
    /// Power attacks: standing, then moving forward, back, left, right.
    pub const POWER: u8 = 2;
    pub const POWER_FORWARD: u8 = 3;
    pub const POWER_BACK: u8 = 4;
    pub const POWER_LEFT: u8 = 5;
    pub const POWER_RIGHT: u8 = 6;
}

/// One player's state, as its own game sees it.
///
/// Attacks and reloads are counters, not events: a lost packet loses nothing, the next
/// state's counter is still higher, and the other side plays one more of them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerState {
    pub place: Place,
    /// Feet position, game units, Z up.
    pub feet: [f32; 3],
    /// Facing, radians (compass: 0 north, π/2 east).
    pub yaw: f32,
    /// Looking up or down, radians (negative down).
    pub pitch: f32,
    /// [`flag`] bits.
    pub flags: u8,
    /// The weapon in hand (WEAP FormID), 0 for none (fists).
    pub weapon: u32,
    /// Attacks begun so far (shots, swings), and what the newest was ([`attack`]).
    pub attacks: u16,
    pub attack_kind: u8,
    /// Reloads begun so far.
    pub reloads: u16,
    /// Worn armour and clothing (ARMO FormIDs), the first `worn_len`.
    pub worn: [u32; MAX_WORN],
    pub worn_len: u8,
    /// The Pip-Boy: 0 down, 1 the glance, 2 the full Pip-Boy.
    pub pipboy: u8,
    /// The furniture reference holding the player (a chair, a couch, a bed), 0 for none:
    /// the others draw the player getting on, sitting or lying there, and getting off.
    pub furniture: u32,
    /// [`more_flag`] bits: the states [`PlayerState::flags`] has no room for.
    pub more: u8,
    /// The emote the player plays, for the others to draw on its puppet.
    pub emote: EmoteState,
}

/// An emote as it goes to the others: which (`id` 0 for none, else the emote's number
/// plus one), which of its clips (`variant`), its phase ([`emote`]), which of its loop's
/// clips (`step`) and how far into the clip it is (`t`, seconds), so one who joins or
/// comes near late sees it where it is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EmoteState {
    pub id: u8,
    pub variant: u8,
    pub phase: u8,
    pub step: u8,
    pub t: f32,
}

/// [`EmoteState::phase`] values.
pub mod emote {
    pub const ENTER: u8 = 0;
    pub const STAY: u8 = 1;
    pub const EXIT: u8 = 2;
    /// A one-shot emote's only clip.
    pub const ONCE: u8 = 3;
}

/// [`PlayerState::more`] bits.
pub mod more_flag {
    /// In deep water: the others draw the player swimming.
    pub const SWIMMING: u8 = 1;
    /// In combat: the others cannot fast travel to this player.
    pub const COMBAT: u8 = 2;
    /// Somewhere no one may fast travel to this player: in a conversation or a scene that
    /// holds the controls, or with fast travel turned off by a script.
    pub const NO_TRAVEL: u8 = 4;
}

impl PlayerState {
    /// A state with nothing in hand and nothing worn.
    pub fn at(place: Place, feet: [f32; 3], yaw: f32) -> Self {
        Self { place, feet, yaw, pitch: 0.0, flags: 0, weapon: 0, attacks: 0, attack_kind: 0, reloads: 0, worn: [0; MAX_WORN], worn_len: 0, pipboy: 0, furniture: 0, more: 0, emote: EmoteState::default() }
    }

    pub fn has(&self, f: u8) -> bool {
        self.flags & f != 0
    }

    pub fn worn(&self) -> &[u32] {
        &self.worn[..(self.worn_len as usize).min(MAX_WORN)]
    }
}

/// A message on the wire. Public so the host's rules ([`crate::guard::Rule`]) can read
/// what they judge; only the session encodes and sends them.
#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    /// `lobby` names the lobby the guest asks for (0 for none): a host that was given a
    /// lobby number ([`Session::set_lobby`]) answers only Hellos naming it, or none. `proof`
    /// is the guest's [`Admission::hello`].
    Hello { token: u64, lobby: u32, proof: u64, name: String },
    /// The host lets the guest in as `id`; `proof` is the host's [`Admission::answer`].
    Welcome { id: u8, proof: u64 },
    /// The host has no free slot; `proof` as a Welcome's.
    Full { proof: u64 },
    /// The host does not know the sender: say Hello again.
    Who,
    /// The host removed this guest (Kick); with `banned` it will not let its key in again
    /// while the lobby lasts (Ban), and answers its Hellos so.
    Kicked { banned: bool },
    State { id: u8, seq: u32, state: PlayerState },
    /// "Still here", from a game with no state to send (a guest choosing its save).
    Ping,
    Bye { id: u8 },
    /// Every player's name, from the host (the host is id 0): a guest learns the host's
    /// and the other guests' names from it.
    Names { names: Vec<(u8, String)> },
    /// How player `id` looks (race, face, hair...; the game's own encoding).
    Look { id: u8, look: Vec<u8> },
    /// The reliable stream between the host and one guest ([`crate::rel`]), both ways;
    /// `epoch` is the host's number for the stream, new each time the guest says Hello.
    Rel { epoch: u16, frame: Frame },
    /// Some of the host's NPCs (all of one round share its `seq`).
    Npcs { seq: u32, npcs: Vec<NpcState> },
    /// A hit ([`HitView`]): a guest's on the host's world, or the host's NPCs' on a guest
    /// (`by`: the reference that dealt it, 0 for none; a guest's goes with 0, and the host
    /// does not read it). `id` numbers the sender's hits: each is sent [`COPIES`] times
    /// and taken once.
    Hit { id: u16, by: u32, hit: HitView },
    /// A door, container or terminal (`terminal`) was unlocked.
    Unlock { reference: u32, terminal: bool },
    /// The host's game clock.
    Clock(ClockView),
    /// What the host's scripts do to the host's player now ([`SceneView`]).
    Scene(SceneView),
    /// The lines the host's NPCs are saying now (the newest `seq` wins).
    Lines { seq: u32, lines: Vec<LineView> },
    /// Page `page` of `pages` of one of the host's lists (`list`: [`LIST_ENABLED`], each
    /// placed reference whose own enabled flag differs from the one its plugin gives it,
    /// with the flag; [`LIST_OPEN`], the same for the open flag). `serial` changes when
    /// the list does; the pages go round, one at a time.
    Presence { list: u8, serial: u32, page: u16, pages: u16, refs: Vec<(u32, bool)> },
    /// What some of the host's NPCs wear and hold, as its game draws them: each NPC's
    /// reference and its worn armour and weapon (the whole set, so each entry stands
    /// alone). The host sends a page every [`WEAR_EVERY`], round and round.
    Wear { npcs: Vec<(u32, Vec<u32>)> },
    /// A door was opened (`open`) or closed by a player (to the host, and from it to
    /// everyone) or in the host's world.
    Door { reference: u32, open: bool },
    /// Player `by` revived player `target` (who is down); `perks`: the reviver's co-op
    /// perks that act on the revived ([`revive_perk`]). The host takes the sender as `by`.
    Revive { target: u8, by: u8, perks: u8 },
    /// The host's moving actors, for one guest (the compact stream, [`ActorPose`]); all
    /// packets of one round share `seq`. `ms`: the host's clock when the round went (its
    /// session's milliseconds), so a guest can tell how late each round is.
    Actors { seq: u32, ms: u32, poses: Vec<ActorPose> },
}

impl Msg {
    /// Its kind, as the guard counts and limits it.
    pub fn kind(&self) -> Kind {
        match self {
            Msg::Hello { .. } => Kind::Hello,
            Msg::Welcome { .. } => Kind::Welcome,
            Msg::Full { .. } => Kind::Full,
            Msg::Who => Kind::Who,
            Msg::Kicked { .. } => Kind::Kicked,
            Msg::State { .. } => Kind::State,
            Msg::Ping => Kind::Ping,
            Msg::Bye { .. } => Kind::Bye,
            Msg::Names { .. } => Kind::Names,
            Msg::Look { .. } => Kind::Look,
            Msg::Rel { .. } => Kind::Rel,
            Msg::Npcs { .. } => Kind::Npcs,
            Msg::Hit { .. } => Kind::Hit,
            Msg::Unlock { .. } => Kind::Unlock,
            Msg::Clock(_) => Kind::Clock,
            Msg::Scene(_) => Kind::Scene,
            Msg::Lines { .. } => Kind::Lines,
            Msg::Presence { .. } => Kind::Presence,
            Msg::Wear { .. } => Kind::Wear,
            Msg::Door { .. } => Kind::Door,
            Msg::Revive { .. } => Kind::Revive,
            Msg::Actors { .. } => Kind::Actors,
        }
    }
}

/// What [`Link::events`] reports.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// This guest was let in as peer `id` (guests only; again after the host restarted).
    Welcomed(u8),
    /// The host had no free slot (guests only).
    Refused,
    /// A peer joined (`name` is empty when only its state has been seen).
    Joined { id: u8, name: String },
    State { id: u8, state: PlayerState },
    /// A peer went quiet; it keeps its slot for [`GRACE`].
    Lost(u8),
    /// A lost peer is back.
    Back(u8),
    /// A peer left, or was lost for longer than [`GRACE`].
    Left(u8),
    /// The host went quiet (guests only); trying to reconnect.
    HostLost,
    /// The host answered again (guests only).
    HostBack,
    /// The host left, or did not answer for [`GRACE`] (guests only).
    HostGone,
    /// The host removed this guest (guests only); `banned`: it will not be let in again.
    /// The link stops as at [`Event::HostGone`].
    Kicked { banned: bool },
    /// A peer's look, when first heard or changed.
    Look { id: u8, look: Vec<u8> },
    /// A player's name, when first heard or changed (guests only; [`Msg::Names`]).
    Named { id: u8, name: String },
    /// An item of the reliable stream from player `id` (the host: 0), once and in order.
    Reliable { id: u8, item: Vec<u8> },
    /// The reliable stream with player `id` started again (a guest's Hello on the host;
    /// a new stream from the host on a guest, id 0): what was sent on it before may be
    /// lost, so the layer above sends its whole state again.
    RelReset { id: u8 },
    /// Some of the host's NPCs, as it draws them (guests only).
    Npcs(Vec<NpcState>),
    /// Player `id` hit something: on the host, guest `id`'s hit on its world (`by` 0);
    /// on a guest, the host's (id 0) NPC `by`'s hit on this player. Each hit once.
    Hit { id: u8, by: u32, hit: HitView },
    /// Player `id` (0 on a guest: whoever it was) unlocked a door, container or terminal
    /// (`terminal`). Each comes once ([`COPY_WINDOW`]). On the host, a guest's reaches the other
    /// guests only through [`Session::relay_unlock`], once the game's checks pass.
    Unlocked { id: u8, reference: u32, terminal: bool },
    /// The host's game clock, each time it comes (guests only).
    Clock(ClockView),
    /// The host's [`SceneView`], each time it comes (guests only).
    Scene(SceneView),
    /// The lines the host's NPCs are saying now, each time they come (guests only).
    Lines(Vec<LineView>),
    /// A page of one of the host's lists, each time it comes (guests only;
    /// [`Msg::Presence`]).
    Presence { list: u8, serial: u32, page: u16, pages: u16, refs: Vec<(u32, bool)> },
    /// What some of the host's NPCs wear and hold ([`Msg::Wear`]; guests only).
    Wear(Vec<(u32, Vec<u32>)>),
    /// Player `id` (0 on a guest: the host's world, or whoever it was) opened (`open`) or
    /// closed a door. Each comes once ([`COPY_WINDOW`]). On the host, a guest's reaches the other
    /// guests only through [`Session::relay_door`], once the game's checks pass.
    Door { id: u8, reference: u32, open: bool },
    /// Player `by` revived this player (the host's, or a guest's), with its co-op perks
    /// ([`revive_perk`]). Each comes once ([`COPY_WINDOW`]).
    Revived { by: u8, perks: u8 },
    /// Guest `by` asks to revive guest `target`, with its co-op perks (host only). Nothing
    /// reaches the target until the game's own checks pass and it calls
    /// [`Session::relay_revive`]. Each comes once ([`COPY_WINDOW`]).
    ReviveAsked { by: u8, target: u8, perks: u8 },
    /// Some of the host's moving actors (guests only), with the host's clock when they
    /// went (milliseconds) and this game's when they came.
    Actors { ms: u32, here_ms: u32, poses: Vec<ActorPose> },
}

impl Event {
    /// The player an event is about, if one (the world's are about none: a player's
    /// waiting state is replaced past them, [`Session::push_event`]).
    fn about(&self) -> Option<u8> {
        match self {
            Event::Welcomed(id) | Event::Lost(id) | Event::Back(id) | Event::Left(id) => Some(*id),
            Event::Joined { id, .. } | Event::State { id, .. } | Event::Look { id, .. } | Event::Named { id, .. } | Event::Reliable { id, .. } | Event::RelReset { id } => Some(*id),
            Event::Refused | Event::HostLost | Event::HostBack | Event::HostGone | Event::Kicked { .. } => Some(0),
            Event::Npcs(_) | Event::Hit { .. } | Event::Unlocked { .. } | Event::Clock(_) | Event::Scene(_) | Event::Lines(_) | Event::Presence { .. } | Event::Wear(_) | Event::Door { .. } | Event::Revived { .. } | Event::ReviveAsked { .. } | Event::Actors { .. } => None,
        }
    }

    /// One the game needs only the newest of: a newer replaces the one still waiting.
    fn newest_only(&self) -> bool {
        matches!(self, Event::Clock(_) | Event::Scene(_) | Event::Lines(_))
    }
}

/// What [`Session::traffic`] reports for one player.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Traffic {
    pub id: u8,
    pub sent: u64,
    pub heard: u64,
    pub pending: usize,
    pub resent: u64,
}

/// A peer as one side sees it.
struct Peer {
    id: u8,
    /// The host's record of a guest: its address and token. Unused for a guest's peers.
    addr: Option<SocketAddr>,
    token: u64,
    last_heard: Instant,
    last_seq: Option<u32>,
    lost: bool,
    look: Option<Vec<u8>>,
    /// Its name: the host's record of a guest's (its Hello), a guest's of another player's
    /// (the host's [`Msg::Names`]).
    name: String,
    /// The host's reliable stream with this guest, and its number (host only).
    rel: Reliable,
    epoch: u16,
    /// The ids of a guest's newest hits, at most [`HITS_KEPT`] (host only).
    hits: VecDeque<u16>,
    /// A guest's unlocks, doors and revives lately (host only).
    copies: Copies,
}

impl Peer {
    fn new(id: u8, now: Instant) -> Self {
        Self { id, addr: None, token: 0, last_heard: now, last_seq: None, lost: false, look: None, name: String::new(), rel: Reliable::new(), epoch: 0, hits: VecDeque::new(), copies: Copies::default() }
    }

    /// Keep `look`; whether it is new.
    fn take_look(&mut self, look: Vec<u8>) -> bool {
        if self.look.as_ref() == Some(&look) {
            return false;
        }
        self.look = Some(look);
        true
    }
}

enum Role {
    Host,
    Guest {
        /// Where the host may be (the addresses the transport's join was given); Hello goes to each until the host answers, from whatever address
        /// it has, and that one is `host`.
        candidates: Vec<SocketAddr>,
        host: Option<SocketAddr>,
        token: u64,
        lobby: u32,
        name: String,
        id: Option<u8>,
        last_heard: Instant,
        last_hello: Option<Instant>,
        host_lost: bool,
        /// The host asked who this is (it knows this guest by another address): say
        /// Hello again.
        asked: bool,
        gone: bool,
    },
}

/// One of the host's paged lists ([`Msg::Presence`]): the list, its serial and the next
/// page to send.
#[derive(Default)]
struct Paged {
    refs: Vec<(u32, bool)>,
    serial: u32,
    page: usize,
}

/// What the host sends of its world, and what a guest heard of it (module notes).
#[derive(Default)]
struct WorldWire {
    /// The host's NPCs to send, the last time they went and the round's number; a
    /// guest's newest round heard.
    npcs: Vec<NpcState>,
    last_npcs: Option<Instant>,
    npc_seq: u32,
    heard_npcs: Option<u32>,
    /// This game's last hit's id (from a random start: a game started again does not reuse
    /// the ids its last one sent), and the ids of the host's newest hits heard (guest only).
    hit_seq: u16,
    heard_hits: VecDeque<u16>,
    /// The host's unlocks, doors and revives lately (guest only).
    heard_copies: Copies,
    /// The host's game clock now, and when it was last sent.
    clock: Option<ClockView>,
    last_clock: Option<Instant>,
    /// The host's scene now, and when and what it last sent.
    scene: SceneView,
    last_scene: Option<(Instant, SceneView)>,
    /// The host's NPCs' lines, and the last sent (when, which); a guest's newest heard.
    lines: Vec<LineView>,
    last_lines: Option<(Instant, Vec<(u32, u32)>)>,
    lines_seq: u32,
    heard_lines: Option<u32>,
    /// The host's lists ([`Msg::Presence`]), and when the last pages went.
    lists: [Paged; LISTS],
    last_presence: Option<Instant>,
    /// What the host's NPCs wear ([`Msg::Wear`]), the next page to send and when the
    /// last went.
    wear: Vec<(u32, Vec<u32>)>,
    wear_page: usize,
    last_wear: Option<Instant>,
    /// The host's actor rounds sent, a guest's newest heard.
    actor_seq: u32,
    heard_actors: Option<u32>,
}

/// One side of the link, ticked by [`Link`]'s thread (or by hand in tests).
pub struct Session {
    transport: Box<dyn Transport>,
    admission: Box<dyn Admission>,
    role: Role,
    /// The host's guests, or the other peers a guest has heard of (the host among them).
    peers: Vec<Peer>,
    state: Option<PlayerState>,
    seq: u32,
    last_send: Option<Instant>,
    look: Option<Vec<u8>>,
    last_look: Option<Instant>,
    /// A host's lobby number, once it has one: Hellos for another lobby are not answered.
    lobby: Option<u32>,
    /// The host's own name, and when every player's names last went.
    own_name: String,
    last_names: Option<Instant>,
    events: Vec<Event>,
    /// Events dropped past [`EVENTS_MAX`].
    events_dropped: u64,
    /// A guest's reliable stream with the host, and the host's number for it once heard.
    rel: Reliable,
    host_epoch: Option<u16>,
    /// Bytes sent to and heard from each address (the perf lines), at most
    /// [`TRAFFIC_ADDRS`] addresses.
    sent: RefCell<HashMap<SocketAddr, u64>>,
    heard: HashMap<SocketAddr, u64>,
    /// The host's checks on its guests' messages.
    guard: Guard,
    /// The world's messages, both ways.
    world: WorldWire,
    /// When this session began (the clock of [`Msg::Actors`]).
    started: Instant,
}

impl Session {
    /// Host over `transport`, letting in the guests `admission` admits.
    pub fn host_on(transport: Box<dyn Transport>, admission: Box<dyn Admission>) -> Self {
        Self::with(transport, admission, Role::Host)
    }

    /// Join over `transport` the host at the first of `candidates` that answers, proving
    /// the lobby with `admission`. `lobby` names the lobby asked for (0 for none), `name` is
    /// shown to the others. `token` names this guest to the host across dropouts and
    /// restarts ([`new_token`] for a new one).
    pub fn join_on(transport: Box<dyn Transport>, admission: Box<dyn Admission>, candidates: &[SocketAddr], lobby: u32, name: &str, token: u64) -> Self {
        let role = Role::Guest {
            candidates: candidates.to_vec(),
            host: None,
            token,
            lobby,
            name: crate::guard::clean_name(name),
            id: None,
            last_heard: Instant::now(),
            last_hello: None,
            host_lost: false,
            asked: false,
            gone: false,
        };
        Self::with(transport, admission, role)
    }

    fn with(transport: Box<dyn Transport>, admission: Box<dyn Admission>, role: Role) -> Self {
        Self {
            transport,
            admission,
            role,
            peers: Vec::new(),
            state: None,
            seq: 0,
            last_send: None,
            look: None,
            last_look: None,
            lobby: None,
            own_name: String::new(),
            last_names: None,
            events: Vec::new(),
            events_dropped: 0,
            rel: Reliable::new(),
            host_epoch: None,
            sent: RefCell::new(HashMap::new()),
            heard: HashMap::new(),
            guard: Guard::default(),
            world: WorldWire { hit_seq: new_token() as u16, ..WorldWire::default() },
            started: Instant::now(),
        }
    }

    /// A guest's host is now at `to` (the host's game came back on another port): Hello
    /// goes there at once, with this guest's token, so the host knows it again. Nothing
    /// for a host, nor when `to` is the host already. True when the host moved.
    pub fn move_host(&mut self, to: SocketAddr) -> bool {
        let Role::Guest { candidates, host, last_hello, asked, gone, .. } = &mut self.role else { return false };
        if *host == Some(to) || (host.is_none() && candidates.as_slice() == [to]) || *gone {
            return false;
        }
        *candidates = vec![to];
        *host = Some(to);
        *asked = true;
        *last_hello = None;
        true
    }

    /// The host's own name, for the guests ([`Msg::Names`]).
    pub fn set_name(&mut self, name: &str) {
        let name = crate::guard::clean_name(name);
        if name != self.own_name {
            self.own_name = name;
            self.last_names = None;
        }
    }

    /// A host's lobby number: from now on only Hellos naming it (or none) are answered.
    pub fn set_lobby(&mut self, key: u32) {
        self.lobby = Some(key);
    }

    /// What the host's game knows of forms ([`FormCheck`]), for its checks on the guests'
    /// states; None: only the numeric checks.
    pub fn set_forms(&mut self, forms: Option<Arc<dyn FormCheck>>) {
        self.guard.set_forms(forms);
    }

    /// A check of the host's on guests' messages of `kind` ([`crate::guard::Rule`]).
    pub fn add_rule(&mut self, kind: Kind, rule: Box<dyn Rule>) {
        self.guard.add_rule(kind, rule);
    }

    /// The rate the host lets each guest send messages of `kind` at.
    pub fn set_rate(&mut self, kind: Kind, rate: Rate) {
        self.guard.set_rate(kind, rate);
    }

    /// What the host's checks dropped, by kind and reason, and each guest's bad messages.
    pub fn guard_stats(&self) -> Stats {
        self.guard.stats()
    }

    /// Guest `id`'s bad messages so far (dropped by a check other than the rate, or not
    /// parsing), host only: for a kick policy.
    pub fn bad_messages(&self, id: u8) -> u64 {
        self.guard.bad(id)
    }

    /// Events dropped because the game did not take them ([`EVENTS_MAX`]).
    pub fn events_dropped(&self) -> u64 {
        self.events_dropped
    }

    pub fn is_host(&self) -> bool {
        matches!(self.role, Role::Host)
    }

    /// This game's peer id: 0 for the host, the host's answer for a guest.
    pub fn id(&self) -> Option<u8> {
        match &self.role {
            Role::Host => Some(0),
            Role::Guest { id, .. } => *id,
        }
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.transport.local_addr()
    }

    /// The host's address once one of the candidates answered (guests only).
    pub fn host_addr(&self) -> Option<SocketAddr> {
        match &self.role {
            Role::Guest { host, .. } => *host,
            Role::Host => None,
        }
    }

    /// This game's newest player state; [`Session::tick`] sends it at [`SEND_HZ`].
    pub fn set_state(&mut self, state: PlayerState) {
        self.state = Some(state);
    }

    /// This game's player's look, sent every [`LOOK_EVERY`] (and at once when it changes);
    /// one longer than [`LOOK_MAX`] is not sent.
    pub fn set_look(&mut self, look: Vec<u8>) {
        if look.len() > LOOK_MAX {
            return;
        }
        if self.look.as_ref() != Some(&look) {
            self.last_look = None;
        }
        self.look = Some(look);
    }

    /// Queue `item` on the reliable stream: a host's to guest `to` (None: every guest), a
    /// guest's to the host (`to` is ignored). False when a stream refused it, full
    /// ([`crate::rel::MAX_UNACKED`]): the other side is not acknowledging, and the layer
    /// above sends its whole state again when the stream starts again
    /// ([`Event::RelReset`]).
    #[must_use = "a full stream refuses the item"]
    pub fn send_reliable(&mut self, to: Option<u8>, item: Vec<u8>) -> bool {
        if self.is_host() {
            let mut all = true;
            for p in self.peers.iter_mut().filter(|p| to.is_none_or(|t| t == p.id)) {
                all &= p.rel.push(item.clone());
            }
            all
        } else {
            self.rel.push(item)
        }
    }

    /// The host's NPCs as it draws them now; [`Session::tick`] sends them at [`NPC_HZ`]
    /// (host only).
    pub fn set_npcs(&mut self, npcs: Vec<NpcState>) {
        self.world.npcs = npcs;
    }

    /// A guest's hit on the host's world, for the host to apply ([`COPIES`] times, at
    /// once; nothing before the host answered, nor from the host).
    pub fn send_hit(&mut self, hit: HitView) {
        let Role::Guest { host: Some(host), id: Some(_), .. } = &self.role else { return };
        let host = *host;
        self.world.hit_seq = self.world.hit_seq.wrapping_add(1);
        let msg = Msg::Hit { id: self.world.hit_seq, by: 0, hit };
        for _ in 0..COPIES {
            self.sock_send(&msg, host);
        }
    }

    /// The host's NPC `by`'s hit on guest `id`'s player (0: none dealt it), for that
    /// guest to take (as [`Session::send_hit`]; host only).
    pub fn send_hit_to(&mut self, id: u8, by: u32, hit: HitView) {
        if !self.is_host() {
            return;
        }
        let Some(addr) = self.peers.iter().find(|p| p.id == id).and_then(|p| p.addr) else { return };
        self.world.hit_seq = self.world.hit_seq.wrapping_add(1);
        let msg = Msg::Hit { id: self.world.hit_seq, by, hit };
        for _ in 0..COPIES {
            self.sock_send(&msg, addr);
        }
    }

    /// The host's game clock now; [`Session::tick`] sends it every [`CLOCK_EVERY`].
    pub fn set_clock(&mut self, clock: ClockView) {
        self.world.clock = Some(clock);
    }

    /// The host's scene now; [`Session::tick`] sends it when it changes and every
    /// [`SCENE_EVERY`]. Past its bounds it is cut ([`SceneView`]).
    pub fn set_scene(&mut self, mut scene: SceneView) {
        scene.mods.truncate(SCENE_MODS_MAX);
        scene.worn.truncate(SCENE_MODS_MAX);
        scene.swaps.retain(|(_, s, t)| s.len() <= SWAP_TEXT_MAX && t.len() <= SWAP_TEXT_MAX);
        scene.swaps.truncate(SCENE_SWAPS_MAX);
        while encode(&Msg::Scene(scene.clone())).len() > SCENE_BYTES_MAX && scene.swaps.pop().is_some() {}
        self.world.scene = scene;
    }

    /// The lines the host's NPCs are saying now; [`Session::tick`] sends them every
    /// [`LINES_EVERY`], and at once when a line starts or ends. At most [`LINES_MAX`].
    pub fn set_lines(&mut self, mut lines: Vec<LineView>) {
        lines.truncate(LINES_MAX);
        self.world.lines = lines;
    }

    /// One of the host's lists now (`list`, [`Msg::Presence`]); [`Session::tick`] sends a
    /// page of each every [`PRESENCE_EVERY`], from the first page again when it changed.
    pub fn set_presence(&mut self, list: u8, refs: Vec<(u32, bool)>) {
        let Some(l) = self.world.lists.get_mut(list as usize) else { return };
        if refs != l.refs {
            l.refs = refs;
            l.serial = l.serial.wrapping_add(1);
            l.page = 0;
        }
    }

    /// What the host's NPCs wear and hold now ([`Msg::Wear`]): each one's reference and
    /// its worn armour and weapon. [`Session::tick`] sends a page every [`WEAR_EVERY`],
    /// round and round (a lost page comes again); an entry has at most [`WEAR_ITEMS`].
    pub fn set_wear(&mut self, mut npcs: Vec<(u32, Vec<u32>)>) {
        for (_, items) in &mut npcs {
            items.truncate(WEAR_ITEMS);
        }
        self.world.wear = npcs;
    }

    /// This player opened (`open`) or closed door `reference` (the host: or its world
    /// did): to the host, or from the host to every guest ([`COPIES`] times).
    pub fn send_door(&mut self, reference: u32, open: bool) {
        self.to_all_copies(&Msg::Door { reference, open });
    }

    /// This player unlocked `reference` (a terminal: hacked): to the host, or from the
    /// host to every guest ([`COPIES`] times).
    pub fn send_unlock(&mut self, reference: u32, terminal: bool) {
        self.to_all_copies(&Msg::Unlock { reference, terminal });
    }

    /// This player revives player `target` (down): to the host (its game passes another
    /// guest's on, [`Session::relay_revive`]), or from the host to that guest ([`COPIES`]
    /// times).
    pub fn send_revive(&mut self, target: u8, perks: u8) {
        let Some(by) = self.id() else { return };
        let msg = Msg::Revive { target, by, perks };
        let to = match &self.role {
            Role::Host => self.peers.iter().find(|p| p.id == target).and_then(|p| p.addr),
            Role::Guest { host, .. } => *host,
        };
        if let Some(addr) = to {
            for _ in 0..COPIES {
                self.sock_send(&msg, addr);
            }
        }
    }

    /// Guest `except`'s door [`Event::Door`], once the game's own checks passed: to every
    /// other guest ([`COPIES`] times; host only).
    pub fn relay_door(&mut self, except: u8, reference: u32, open: bool) {
        self.relay_from(except, &Msg::Door { reference, open });
    }

    /// Guest `except`'s unlock [`Event::Unlocked`], once the game's own checks passed: to
    /// every other guest ([`COPIES`] times; host only).
    pub fn relay_unlock(&mut self, except: u8, reference: u32, terminal: bool) {
        self.relay_from(except, &Msg::Unlock { reference, terminal });
    }

    /// Guest `by`'s revive of guest `target` [`Event::ReviveAsked`], once the game's own
    /// checks passed: to that guest only, `by` kept ([`COPIES`] times; host only).
    pub fn relay_revive(&mut self, target: u8, by: u8, perks: u8) {
        if !self.is_host() || target == 0 || target == by {
            return;
        }
        let Some(addr) = self.peers.iter().find(|p| p.id == target).and_then(|p| p.addr) else { return };
        let msg = Msg::Revive { target, by, perks: perks & revive_perk::KNOWN };
        for _ in 0..COPIES {
            self.sock_send(&msg, addr);
        }
    }

    /// Count a bad message against guest `id`: one the game's own checks refused, as the
    /// guard's refusals count ([`Session::bad_messages`]; host only).
    pub fn note_bad(&mut self, id: u8) {
        if self.is_host() {
            self.guard.note_bad(id);
        }
    }

    /// The host's moving actors for guest `to`, sent now (host only).
    pub fn send_actors(&mut self, to: u8, poses: &[ActorPose]) {
        if !self.is_host() {
            return;
        }
        let Some(addr) = self.peers.iter().find(|p| p.id == to).and_then(|p| p.addr) else { return };
        self.world.actor_seq = self.world.actor_seq.wrapping_add(1);
        let ms = self.started.elapsed().as_millis() as u32;
        for chunk in poses.chunks(ACTORS_PER_PACKET) {
            self.sock_send(&Msg::Actors { seq: self.world.actor_seq, ms, poses: chunk.to_vec() }, addr);
        }
    }

    /// Bytes sent to and heard from each player (the host: 0) so far, with the items
    /// waiting on its reliable stream and those sent again.
    /// The host's guests as the lobby list shows them (docs/design/coop-networking.md §10):
    /// each one's id, the address the host knows it at (the transport's: a relayed guest's
    /// stand-in, `crate::stack::Path::of`) and the reliable stream's round trip, once
    /// measured. Empty on a guest.
    pub fn paths(&self) -> Vec<(u8, Option<SocketAddr>, Option<std::time::Duration>)> {
        match &self.role {
            Role::Host => self.peers.iter().map(|p| (p.id, p.addr, p.rel.rtt)).collect(),
            Role::Guest { .. } => Vec::new(),
        }
    }

    pub fn traffic(&self) -> Vec<Traffic> {
        let sent = self.sent.borrow();
        let of = |a: Option<SocketAddr>| a.map_or((0, 0), |a| (sent.get(&a).copied().unwrap_or(0), self.heard.get(&a).copied().unwrap_or(0)));
        match &self.role {
            Role::Host => self
                .peers
                .iter()
                .map(|p| {
                    let (out, inb) = of(p.addr);
                    Traffic { id: p.id, sent: out, heard: inb, pending: p.rel.pending(), resent: p.rel.resent }
                })
                .collect(),
            Role::Guest { host, .. } => {
                let (out, inb) = of(*host);
                vec![Traffic { id: 0, sent: out, heard: inb, pending: self.rel.pending(), resent: self.rel.resent }]
            }
        }
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Read every waiting packet, send this game's state when it is due, say Hello while
    /// a guest has no answer, and mark quiet peers lost or gone.
    pub fn tick(&mut self, now: Instant) {
        self.receive(now);
        self.check_peers(now);
        if let Role::Guest { candidates, host, token, lobby, name, id, last_heard, last_hello, host_lost, asked, gone } = &mut self.role {
            if *gone {
                return;
            }
            let silent = now.saturating_duration_since(*last_heard);
            if id.is_some() && !*host_lost && silent >= LOST_AFTER {
                *host_lost = true;
                self.events.push(Event::HostLost);
            }
            if silent >= GRACE {
                *gone = true;
                self.events.push(Event::HostGone);
                return;
            }
            // Hello until welcomed, and again while the host is lost or asks who this is:
            // the host may have dropped this address, or restarted and forgotten everyone.
            if (id.is_none() || *host_lost || *asked) && last_hello.is_none_or(|t| now.saturating_duration_since(t) >= HELLO_EVERY) {
                *asked = false;
                let hello = encode(&Msg::Hello { token: *token, lobby: *lobby, proof: self.admission.hello(), name: name.clone() });
                // Until one answers, every address the lobby gave; then that one.
                for to in host.iter().chain(candidates.iter().filter(|_| host.is_none())) {
                    let _ = self.transport.send_to(&hello, *to);
                }
                *last_hello = Some(now);
            }
        }
        let due = self.last_send.is_none_or(|t| now.saturating_duration_since(t) >= Duration::from_secs(1) / SEND_HZ);
        if let (true, Some(id)) = (due, self.id()) {
            let msg = match self.state {
                Some(state) => {
                    self.seq = self.seq.wrapping_add(1);
                    Msg::State { id, seq: self.seq, state }
                }
                None => Msg::Ping,
            };
            self.to_all(&msg);
            self.last_send = Some(now);
        }
        if self.is_host() && !self.peers.is_empty() && self.last_names.is_none_or(|t| now.saturating_duration_since(t) >= NAMES_EVERY) {
            let names = std::iter::once((0, self.own_name.clone())).chain(self.peers.iter().map(|p| (p.id, p.name.clone()))).collect();
            self.to_all(&Msg::Names { names });
            self.last_names = Some(now);
        }
        if self.is_host() && !self.peers.is_empty() {
            self.send_world(now);
        }
        // The reliable streams: what is due, and acknowledgements.
        if self.is_host() {
            let mut out = Vec::new();
            for p in self.peers.iter_mut() {
                if let Some(addr) = p.addr.filter(|_| !p.lost) {
                    for frame in p.rel.frames(now, REL_PACKETS) {
                        out.push((Msg::Rel { epoch: p.epoch, frame }, addr));
                    }
                }
            }
            for (msg, addr) in out {
                self.sock_send(&msg, addr);
            }
        } else if let (Some(epoch), Role::Guest { host: Some(host), id: Some(_), .. }) = (self.host_epoch, &self.role) {
            let host = *host;
            for frame in self.rel.frames(now, REL_PACKETS) {
                self.sock_send(&Msg::Rel { epoch, frame }, host);
            }
        }
        let look_due = self.last_look.is_none_or(|t| now.saturating_duration_since(t) >= LOOK_EVERY);
        if let (true, Some(look), Some(id)) = (look_due, self.look.clone(), self.id()) {
            self.to_all(&Msg::Look { id, look });
            self.last_look = Some(now);
        }
    }

    /// The host's world, as each part is due (module notes).
    fn send_world(&mut self, now: Instant) {
        let since = |t: Instant| now.saturating_duration_since(t);
        let w = &mut self.world;
        let mut out = Vec::new();
        if !w.npcs.is_empty() && w.last_npcs.is_none_or(|t| since(t) >= Duration::from_secs(1) / NPC_HZ) {
            w.npc_seq = w.npc_seq.wrapping_add(1);
            out.extend(w.npcs.chunks(NPCS_PER_PACKET).map(|c| Msg::Npcs { seq: w.npc_seq, npcs: c.to_vec() }));
            w.last_npcs = Some(now);
        }
        if let Some(c) = w.clock.filter(|_| w.last_clock.is_none_or(|t| since(t) >= CLOCK_EVERY)) {
            out.push(Msg::Clock(c));
            w.last_clock = Some(now);
        }
        if w.last_scene.as_ref().is_none_or(|(t, sent)| *sent != w.scene || since(*t) >= SCENE_EVERY) {
            out.push(Msg::Scene(w.scene.clone()));
            w.last_scene = Some((now, w.scene.clone()));
        }
        if w.last_presence.is_none_or(|t| since(t) >= PRESENCE_EVERY) {
            for (list, l) in w.lists.iter_mut().enumerate() {
                let pages = l.refs.len().div_ceil(PRESENCE_PAGE).max(1);
                let page = l.page % pages;
                let refs = l.refs.iter().skip(page * PRESENCE_PAGE).take(PRESENCE_PAGE).copied().collect();
                out.push(Msg::Presence { list: list as u8, serial: l.serial, page: page as u16, pages: pages.min(u16::MAX as usize) as u16, refs });
                l.page = (page + 1) % pages;
            }
            w.last_presence = Some(now);
        }
        if !w.wear.is_empty() && w.last_wear.is_none_or(|t| since(t) >= WEAR_EVERY) {
            let pages = w.wear.len().div_ceil(WEAR_PAGE);
            let page = w.wear_page % pages;
            out.push(Msg::Wear { npcs: w.wear.iter().skip(page * WEAR_PAGE).take(WEAR_PAGE).cloned().collect() });
            w.wear_page = (page + 1) % pages;
            w.last_wear = Some(now);
        }
        let said: Vec<(u32, u32)> = w.lines.iter().map(|l| (l.speaker, l.info)).collect();
        let lines_due = match &w.last_lines {
            None => !said.is_empty(),
            Some((t, sent)) => *sent != said || (!said.is_empty() && since(*t) >= LINES_EVERY),
        };
        if lines_due {
            w.lines_seq = w.lines_seq.wrapping_add(1);
            out.push(Msg::Lines { seq: w.lines_seq, lines: w.lines.clone() });
            w.last_lines = Some((now, said));
        }
        for msg in out {
            self.to_all(&msg);
        }
    }

    /// To every guest (the host), or to the host (a guest), [`COPIES`] times.
    fn to_all_copies(&self, msg: &Msg) {
        for _ in 0..COPIES {
            self.to_all(msg);
        }
    }

    /// From the host to every guest but `id`, [`COPIES`] times.
    fn relay_from(&self, id: u8, msg: &Msg) {
        if !self.is_host() {
            return;
        }
        for addr in self.peers.iter().filter(|o| o.id != id).filter_map(|o| o.addr) {
            for _ in 0..COPIES {
                self.sock_send(msg, addr);
            }
        }
    }

    /// To every guest (the host), or to the host (a guest).
    fn to_all(&self, msg: &Msg) {
        match &self.role {
            Role::Host => {
                for addr in self.peers.iter().filter_map(|p| p.addr) {
                    self.sock_send(msg, addr);
                }
            }
            Role::Guest { host: Some(host), .. } => self.sock_send(msg, *host),
            Role::Guest { .. } => {}
        }
    }

    fn sock_send(&self, msg: &Msg, to: SocketAddr) {
        let bytes = encode(msg);
        let known = |a: SocketAddr| known_addr(&self.role, &self.peers, a);
        tally(&mut self.sent.borrow_mut(), to, bytes.len() as u64 + UDP_OVERHEAD, known);
        let _ = self.transport.send_to(&bytes, to);
    }

    fn receive(&mut self, now: Instant) {
        let mut buf = [0u8; 1500];
        // A transport error other than "nothing waiting" ends this tick's reading.
        while let Ok(Some((n, from))) = self.transport.recv_from(&mut buf) {
            self.handle(&buf[..n], from, now);
        }
    }

    fn handle(&mut self, bytes: &[u8], from: SocketAddr, now: Instant) {
        let known = |a: SocketAddr| known_addr(&self.role, &self.peers, a);
        tally(&mut self.heard, from, bytes.len() as u64 + UDP_OVERHEAD, known);
        let Some(msg) = decode(bytes) else {
            if let (true, Some(id)) = (self.is_host(), self.peer_at(from)) {
                self.guard.note_bad(id);
            }
            return;
        };
        if self.is_host() {
            self.host_handle(msg, from, now);
        } else {
            self.guest_handle(msg, from, now);
        }
    }

    /// The guest at address `from` (host only).
    fn peer_at(&self, from: SocketAddr) -> Option<u8> {
        self.peers.iter().find(|p| p.addr == Some(from)).map(|p| p.id)
    }

    /// Note an event for the game (module notes: bounded).
    fn push_event(&mut self, e: Event) {
        if let Event::State { id, .. } | Event::Look { id, .. } = &e {
            let (id, state) = (*id, matches!(e, Event::State { .. }));
            // The same player's last one still waiting, unless something else about the
            // player came after it.
            for old in self.events.iter_mut().rev() {
                let same = match old {
                    Event::State { id: i, .. } => state && *i == id,
                    Event::Look { id: i, .. } => !state && *i == id,
                    _ => false,
                };
                if same {
                    *old = e;
                    return;
                }
                if old.about() == Some(id) && !matches!(old, Event::State { .. } | Event::Look { .. }) {
                    break;
                }
            }
        }
        if e.newest_only() {
            let kind = std::mem::discriminant(&e);
            if let Some(old) = self.events.iter_mut().find(|o| std::mem::discriminant(&**o) == kind) {
                *old = e;
                return;
            }
        }
        // A reliable item is never dropped: frames are not taken while the events are full.
        if self.events.len() >= EVENTS_MAX && !matches!(e, Event::Reliable { .. }) {
            self.events_dropped += 1;
            return;
        }
        self.events.push(e);
    }

    fn host_handle(&mut self, mut msg: Msg, from: SocketAddr, now: Instant) {
        // The sender is the guest its address belongs to, whatever id the message names.
        let peer = self.peer_at(from);
        if self.guard.check(from, peer, &mut msg, now) == Verdict::Accept {
            self.host_take(msg, peer, from, now);
        }
    }

    /// A guest's message the guard accepted.
    fn host_take(&mut self, msg: Msg, peer: Option<u8>, from: SocketAddr, now: Instant) {
        match msg {
            Msg::Hello { token, lobby, proof, name } => {
                // Another lobby's guest asking around its network.
                if lobby != 0 && self.lobby.is_some_and(|k| k != lobby) {
                    return;
                }
                // The lobby's proof before anything else, a known token's too.
                match self.admission.admits_from(proof, token, from) {
                    Admit::Yes => {}
                    Admit::No => {
                        self.guard.note(Kind::Hello, Reason::Secret, peer);
                        return;
                    }
                    Admit::Banned => {
                        self.guard.note(Kind::Hello, Reason::Secret, peer);
                        self.sock_send(&Msg::Kicked { banned: true }, from);
                        return;
                    }
                }
                // A known token is a guest coming back, from wherever it now is; a guest
                // already at this address with a new token is the same guest started again
                // (it keeps its slot: one address never holds two).
                let slot = self.peers.iter().position(|p| p.token == token).or_else(|| peer.and_then(|id| self.peers.iter().position(|p| p.id == id)));
                if let Some(p) = slot.map(|i| &mut self.peers[i]) {
                    p.token = token;
                    p.addr = Some(from);
                    p.last_heard = now;
                    // A game started again counts its states from the start.
                    p.last_seq = None;
                    p.name = name;
                    let id = p.id;
                    // A Hello starts the reliable stream again (the guest may have started
                    // again, or lost what was on the way).
                    p.epoch = p.epoch.wrapping_add(1);
                    p.rel = Reliable::new();
                    // Its newest hits' ids are kept, so a late copy is not taken twice: a
                    // game started again numbers from a new random start, not likely
                    // among them [design].
                    let back = std::mem::take(&mut p.lost);
                    self.last_names = None;
                    self.guard.forget(id);
                    self.push_event(Event::RelReset { id });
                    if back {
                        self.push_event(Event::Back(id));
                    }
                    self.sock_send(&Msg::Welcome { id, proof: self.admission.answer(token, Some(id)) }, from);
                    return;
                }
                let Some(id) = (1..=MAX_GUESTS as u8).find(|i| self.peers.iter().all(|p| p.id != *i)) else {
                    self.sock_send(&Msg::Full { proof: self.admission.answer(token, None) }, from);
                    return;
                };
                // A stream number unlike a guest's last one in this slot, and unlike the
                // one a host game before this one gave it (a guest coming back to a host
                // started again kept that one).
                let epoch = ((token as u16) ^ (new_token() as u16)) | 1;
                self.guard.drop_guest(id);
                self.peers.push(Peer { addr: Some(from), token, name: name.clone(), epoch, ..Peer::new(id, now) });
                self.last_names = None;
                self.sock_send(&Msg::Welcome { id, proof: self.admission.answer(token, Some(id)) }, from);
                self.push_event(Event::Joined { id, name });
                self.push_event(Event::RelReset { id });
            }
            Msg::State { seq, state, .. } => {
                let Some(p) = self.peers.iter_mut().find(|p| Some(p.id) == peer) else {
                    self.sock_send(&Msg::Who, from);
                    return;
                };
                p.last_heard = now;
                let id = p.id;
                let back = std::mem::take(&mut p.lost);
                let stale = p.last_seq.is_some_and(|s| !fresh(seq, s));
                if !stale {
                    p.last_seq = Some(seq);
                }
                if back {
                    self.push_event(Event::Back(id));
                }
                if stale {
                    return;
                }
                let relay = Msg::State { id, seq, state };
                for addr in self.peers.iter().filter(|o| o.id != id).filter_map(|o| o.addr) {
                    self.sock_send(&relay, addr);
                }
                self.push_event(Event::State { id, state });
            }
            Msg::Ping => {
                let Some(p) = self.peers.iter_mut().find(|p| Some(p.id) == peer) else {
                    self.sock_send(&Msg::Who, from);
                    return;
                };
                p.last_heard = now;
                let id = p.id;
                if std::mem::take(&mut p.lost) {
                    self.push_event(Event::Back(id));
                }
            }
            Msg::Look { look, .. } => {
                let Some(p) = self.peers.iter_mut().find(|p| Some(p.id) == peer) else { return };
                let id = p.id;
                if p.take_look(look.clone()) {
                    self.push_event(Event::Look { id, look: look.clone() });
                }
                // Each time (it comes again every LOOK_EVERY): a guest who joined since has it.
                let relay = Msg::Look { id, look };
                for addr in self.peers.iter().filter(|o| o.id != id).filter_map(|o| o.addr) {
                    self.sock_send(&relay, addr);
                }
            }
            Msg::Bye { .. } => {
                if let Some(id) = peer {
                    self.drop_peer(id);
                }
            }
            Msg::Rel { epoch, frame } => {
                // Not taken while the game is not taking its events: unacknowledged, the
                // guest sends it again.
                if self.events.len() >= EVENTS_MAX {
                    return;
                }
                let Some(p) = self.peers.iter_mut().find(|p| Some(p.id) == peer) else { return };
                // A frame of an older stream.
                if epoch != p.epoch {
                    return;
                }
                p.last_heard = now;
                let id = p.id;
                for item in p.rel.receive_at(frame, now) {
                    // Each item passes the host's rules for its kind before the game has it.
                    if self.guard.admit_item(id, &item, now) == Verdict::Accept {
                        self.push_event(Event::Reliable { id, item });
                    }
                }
            }
            Msg::Hit { id: hit_id, hit, .. } => {
                let Some(p) = self.peers.iter_mut().find(|p| Some(p.id) == peer) else { return };
                if p.hits.contains(&hit_id) {
                    return;
                }
                if p.hits.len() >= HITS_KEPT {
                    p.hits.pop_front();
                }
                p.hits.push_back(hit_id);
                let id = p.id;
                self.push_event(Event::Hit { id, by: 0, hit });
            }
            // For the host's game only: the checks that need its world (loaded, in reach,
            // locked) run there, and it passes on what passes ([`Session::relay_unlock`],
            // [`Session::relay_door`]) [design].
            Msg::Unlock { reference, terminal } => {
                let Some(p) = self.peers.iter_mut().find(|p| Some(p.id) == peer) else { return };
                if !p.copies.first(unlock_key(reference, terminal), now) {
                    return;
                }
                let id = p.id;
                self.push_event(Event::Unlocked { id, reference, terminal });
            }
            Msg::Door { reference, open } => {
                let Some(p) = self.peers.iter_mut().find(|p| Some(p.id) == peer) else { return };
                if !p.copies.first(door_key(reference, open), now) {
                    return;
                }
                let id = p.id;
                self.push_event(Event::Door { id, reference, open });
            }
            Msg::Revive { target, perks, .. } => {
                // The reviver is the sender, whatever `by` says (`crate::world` drops one
                // that names another).
                let Some(p) = self.peers.iter_mut().find(|p| Some(p.id) == peer) else { return };
                if !p.copies.first(revive_key(target, p.id, perks), now) {
                    return;
                }
                let by = p.id;
                // Another guest's revive waits for the game's checks (is the target down,
                // in reach), [`Session::relay_revive`] [design].
                if target == 0 {
                    self.push_event(Event::Revived { by, perks });
                } else if target != by && self.peers.iter().any(|p| p.id == target) {
                    self.push_event(Event::ReviveAsked { by, target, perks });
                }
            }
            // Dropped by the guard: only the host sends these.
            Msg::Welcome { .. } | Msg::Full { .. } | Msg::Who | Msg::Kicked { .. } | Msg::Names { .. } => {}
            Msg::Npcs { .. } | Msg::Lines { .. } | Msg::Presence { .. } | Msg::Wear { .. } | Msg::Clock(_) | Msg::Scene(_) | Msg::Actors { .. } => {}
        }
    }

    /// The host removes guest `id` (Kick): it is told so ([`BYE_COPIES`] times) and dropped;
    /// with `ban`, the admission keeps it out while the lobby lasts ([`Admission::ban`]).
    /// False when this is no host or `id` is no guest, and with `ban` when the admission
    /// cannot ban (the guest is kicked all the same).
    pub fn kick(&mut self, id: u8, ban: bool) -> bool {
        if !self.is_host() {
            return false;
        }
        let Some(addr) = self.peers.iter().find(|p| p.id == id).map(|p| p.addr) else { return false };
        let banned = match addr {
            Some(a) if ban => self.admission.ban(a),
            _ => false,
        };
        if let Some(a) = addr {
            for _ in 0..BYE_COPIES {
                self.sock_send(&Msg::Kicked { banned }, a);
            }
        }
        self.drop_peer(id);
        self.last_names = None;
        !ban || banned
    }

    /// The host drops guest `id`: the others are told, and the game.
    fn drop_peer(&mut self, id: u8) {
        self.peers.retain(|p| p.id != id);
        self.guard.drop_guest(id);
        self.relay_bye(id);
        self.push_event(Event::Left(id));
    }

    /// A guest welcomed (again): what it heard from the host so far counts from the start
    /// again: the reliable stream both ways (the host's end is new too) and the host's
    /// epoch.
    fn hear_anew(&mut self) {
        // Before any stream was heard (the first Welcome), what is queued keeps (as the
        // first stream keeps it, `Msg::Rel`).
        if self.host_epoch.take().is_some() {
            self.rel = Reliable::new();
        }
        // A host started again numbers its rounds from 1; its hits from a new random
        // start, so the newest hits heard are kept (a late copy is not taken twice) [design].
        self.world.heard_npcs = None;
        self.world.heard_lines = None;
        self.world.heard_actors = None;
    }

    fn guest_handle(&mut self, msg: Msg, from: SocketAddr, now: Instant) {
        let Role::Guest { host, token, id, last_heard, host_lost, gone, .. } = &mut self.role else { return };
        if *gone {
            return;
        }
        // An answer must prove the lobby's secret: one that does not is forged.
        let proven = match &msg {
            Msg::Welcome { id: given, proof } => self.admission.answered(*token, Some(*given), *proof),
            Msg::Full { proof } => self.admission.answered(*token, None, *proof),
            _ => true,
        };
        if !proven {
            return;
        }
        // Before an answer, the address whose answer proves the secret is the host (an
        // answer may come from another address than the one the Hello went to); then only it.
        match *host {
            Some(h) if h != from => return,
            Some(_) => {}
            None if matches!(msg, Msg::Welcome { .. } | Msg::Full { .. } | Msg::Kicked { banned: true }) => *host = Some(from),
            None => return,
        }
        *last_heard = now;
        let back = std::mem::take(host_lost);
        let me = *id;
        if back {
            self.push_event(Event::HostBack);
        }
        match msg {
            Msg::Welcome { id: given, .. } => {
                let Role::Guest { id, .. } = &mut self.role else { return };
                if *id != Some(given) {
                    // First welcome, or the host restarted and gave a new id: everyone it
                    // knew is announced again by their states.
                    *id = Some(given);
                    for p in std::mem::take(&mut self.peers) {
                        self.push_event(Event::Left(p.id));
                    }
                    self.push_event(Event::Welcomed(given));
                }
                // A Welcome answers a Hello, and the host starts this guest's stream again
                // for each one (a known token), or never had it (a host started again): so
                // does this game.
                self.hear_anew();
            }
            Msg::Full { .. } => self.push_event(Event::Refused),
            Msg::Kicked { banned } => {
                let Role::Guest { gone, .. } = &mut self.role else { return };
                *gone = true;
                for p in std::mem::take(&mut self.peers) {
                    self.push_event(Event::Left(p.id));
                }
                self.push_event(Event::Kicked { banned });
            }
            Msg::Who => {
                let Role::Guest { asked, last_hello, .. } = &mut self.role else { return };
                *asked = true;
                *last_hello = None;
            }
            Msg::State { id: peer, seq, state } => {
                if Some(peer) == me || peer as usize > MAX_GUESTS {
                    return;
                }
                let joined = self.guest_peer(peer, now);
                let p = self.peers.iter_mut().find(|p| p.id == peer).expect("just made");
                p.last_heard = now;
                let back = std::mem::take(&mut p.lost);
                let stale = p.last_seq.is_some_and(|s| !fresh(seq, s));
                if !stale {
                    p.last_seq = Some(seq);
                }
                if joined {
                    self.push_event(Event::Joined { id: peer, name: String::new() });
                }
                if back {
                    self.push_event(Event::Back(peer));
                }
                if !stale {
                    self.push_event(Event::State { id: peer, state });
                }
            }
            Msg::Look { id: peer, look } => {
                if Some(peer) == me || peer as usize > MAX_GUESTS {
                    return;
                }
                let joined = self.guest_peer(peer, now);
                let p = self.peers.iter_mut().find(|p| p.id == peer).expect("just made");
                let new = p.take_look(look.clone());
                if joined {
                    self.push_event(Event::Joined { id: peer, name: String::new() });
                }
                if new {
                    self.push_event(Event::Look { id: peer, look });
                }
            }
            Msg::Bye { id: 0 } => {
                let Role::Guest { gone, .. } = &mut self.role else { return };
                *gone = true;
                for p in std::mem::take(&mut self.peers) {
                    self.push_event(Event::Left(p.id));
                }
                self.push_event(Event::HostGone);
            }
            Msg::Bye { id: peer } => {
                if let Some(i) = self.peers.iter().position(|p| p.id == peer) {
                    self.peers.remove(i);
                    self.push_event(Event::Left(peer));
                }
            }
            Msg::Names { names } => {
                for (peer, name) in names {
                    if Some(peer) == me || name.is_empty() || peer as usize > MAX_GUESTS {
                        continue;
                    }
                    if self.guest_peer(peer, now) {
                        self.push_event(Event::Joined { id: peer, name: String::new() });
                    }
                    let p = self.peers.iter_mut().find(|p| p.id == peer).expect("just made");
                    if p.name != name {
                        p.name = name.clone();
                        self.push_event(Event::Named { id: peer, name });
                    }
                }
            }
            Msg::Rel { epoch, frame } => {
                // Not taken while the game is not taking its events (module notes).
                if self.events.len() >= EVENTS_MAX {
                    return;
                }
                // A new stream (the host heard this guest's Hello): what was on the old
                // one is gone; a frame of one of the last few streams, late on the way, is
                // dropped. Any other number is a new stream (a host game started again
                // numbers its own), so a Welcome lost on the way does not leave this guest
                // on the old one.
                match self.host_epoch {
                    Some(e) if e == epoch => {}
                    Some(e) if (1..=EPOCHS_LATE).contains(&e.wrapping_sub(epoch)) => return,
                    was => {
                        // The first stream keeps what was queued before it was heard
                        // (nothing of it went yet).
                        if was.is_some() {
                            self.rel = Reliable::new();
                        }
                        self.host_epoch = Some(epoch);
                        self.push_event(Event::RelReset { id: 0 });
                    }
                }
                for item in self.rel.receive_at(frame, now) {
                    self.push_event(Event::Reliable { id: 0, item });
                }
            }
            Msg::Npcs { seq, npcs } => {
                // A round's packets share its number; an older round is dropped.
                if self.world.heard_npcs.is_some_and(|l| seq != l && !fresh(seq, l)) {
                    return;
                }
                self.world.heard_npcs = Some(seq);
                self.push_event(Event::Npcs(npcs));
            }
            // A hit on this guest's player, from the host (player 0), once.
            Msg::Hit { id: hit_id, by, hit } => {
                let heard = &mut self.world.heard_hits;
                if heard.contains(&hit_id) {
                    return;
                }
                if heard.len() >= HITS_KEPT {
                    heard.pop_front();
                }
                heard.push_back(hit_id);
                self.push_event(Event::Hit { id: 0, by, hit });
            }
            Msg::Actors { seq, ms, poses } => {
                // An older round is dropped; one far behind the last is a host started
                // again (as the NPC rounds and states take it, `fresh`).
                if self.world.heard_actors.is_some_and(|h| seq != h && !fresh(seq, h)) {
                    return;
                }
                self.world.heard_actors = Some(seq);
                let here_ms = self.started.elapsed().as_millis() as u32;
                self.push_event(Event::Actors { ms, here_ms, poses });
            }
            Msg::Lines { seq, lines } => {
                if self.world.heard_lines.is_some_and(|l| !fresh(seq, l)) {
                    return;
                }
                self.world.heard_lines = Some(seq);
                self.push_event(Event::Lines(lines));
            }
            Msg::Unlock { reference, terminal } => {
                if self.world.heard_copies.first(unlock_key(reference, terminal), now) {
                    self.push_event(Event::Unlocked { id: 0, reference, terminal });
                }
            }
            Msg::Door { reference, open } => {
                if self.world.heard_copies.first(door_key(reference, open), now) {
                    self.push_event(Event::Door { id: 0, reference, open });
                }
            }
            Msg::Clock(c) => self.push_event(Event::Clock(c)),
            Msg::Scene(v) => self.push_event(Event::Scene(v)),
            Msg::Presence { list, serial, page, pages, refs } => self.push_event(Event::Presence { list, serial, page, pages, refs }),
            Msg::Wear { npcs } => self.push_event(Event::Wear(npcs)),
            Msg::Revive { target, by, perks } => {
                if Some(target) == me && self.world.heard_copies.first(revive_key(target, by, perks), now) {
                    self.push_event(Event::Revived { by, perks });
                }
            }
            // A guest's messages, for the host only.
            Msg::Hello { .. } | Msg::Ping => {}
        }
    }

    /// A guest's record of player `peer`, made if new (true).
    fn guest_peer(&mut self, peer: u8, now: Instant) -> bool {
        if self.peers.iter().any(|p| p.id == peer) {
            return false;
        }
        self.peers.push(Peer::new(peer, now));
        true
    }

    fn relay_bye(&self, id: u8) {
        if self.is_host() {
            self.to_all(&Msg::Bye { id });
        }
    }

    /// Quiet peers become lost, and lost ones past [`GRACE`] are dropped.
    fn check_peers(&mut self, now: Instant) {
        let mut gone = Vec::new();
        let mut lost = Vec::new();
        for p in &mut self.peers {
            let silent = now.saturating_duration_since(p.last_heard);
            if silent >= GRACE {
                gone.push(p.id);
            } else if !p.lost && silent >= LOST_AFTER {
                p.lost = true;
                lost.push(p.id);
            }
        }
        for id in lost {
            self.push_event(Event::Lost(id));
        }
        for id in gone {
            if self.is_host() {
                self.drop_peer(id);
            } else {
                self.peers.retain(|p| p.id != id);
                self.push_event(Event::Left(id));
            }
        }
    }

    /// Say goodbye, so the others drop this player at once rather than after [`GRACE`]:
    /// [`BYE_COPIES`] times, [`BYE_GAP`] apart, so one lost packet does not leave the
    /// others waiting (a host that ends the lobby must reach every guest).
    fn bye(&self) {
        for i in 0..BYE_COPIES {
            if i > 0 {
                std::thread::sleep(BYE_GAP);
            }
            match &self.role {
                Role::Host => self.relay_bye(0),
                Role::Guest { host: Some(host), id: Some(id), gone: false, .. } => self.sock_send(&Msg::Bye { id: *id }, *host),
                Role::Guest { .. } => return,
            }
        }
    }
}

/// Whether `a` is a current peer's address: a host's guest, a guest's host.
fn known_addr(role: &Role, peers: &[Peer], a: SocketAddr) -> bool {
    match role {
        Role::Host => peers.iter().any(|p| p.addr == Some(a)),
        Role::Guest { host, .. } => *host == Some(a),
    }
}

/// Count `n` bytes for `to`, keeping at most [`TRAFFIC_ADDRS`] addresses: when a new one
/// comes to a full map, those not of a current peer (`known`) are forgotten first, and
/// if none can be, the new one is not counted.
fn tally(map: &mut HashMap<SocketAddr, u64>, to: SocketAddr, n: u64, known: impl Fn(SocketAddr) -> bool) {
    if !map.contains_key(&to) && map.len() >= TRAFFIC_ADDRS {
        map.retain(|a, _| known(*a));
        if map.len() >= TRAFFIC_ADDRS {
            return;
        }
    }
    *map.entry(to).or_default() += n;
}

/// A [`Session`] on its own thread, so the link keeps beating while the game's frame
/// stalls (a cell loading, a save). Dropping it says Bye and stops the thread.
pub struct Link {
    session: Arc<Mutex<Session>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// Stop without a Bye ([`Link::abandon`]).
    quiet: bool,
}

impl Link {
    /// Run `session` on a thread of its own; an error if the thread cannot be started.
    pub fn new(session: Session) -> std::io::Result<Self> {
        let session = Arc::new(Mutex::new(session));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (session, stop) = (session.clone(), stop.clone());
            std::thread::Builder::new()
                .name("coop-net".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        if let Ok(mut s) = session.lock() {
                            s.tick(Instant::now());
                        }
                        std::thread::sleep(TICK);
                    }
                })
                ?
        };
        Ok(Self { session, stop, thread: Some(thread), quiet: false })
    }

    fn with<R>(&self, f: impl FnOnce(&mut Session) -> R) -> R {
        let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut s)
    }

    pub fn is_host(&self) -> bool {
        self.with(|s| s.is_host())
    }

    /// [`Session::kick`].
    pub fn kick(&self, id: u8, ban: bool) -> bool {
        self.with(|s| s.kick(id, ban))
    }

    pub fn id(&self) -> Option<u8> {
        self.with(|s| s.id())
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.with(|s| s.local_addr())
    }

    /// [`Session::move_host`].
    pub fn move_host(&self, to: SocketAddr) -> bool {
        self.with(|s| s.move_host(to))
    }

    pub fn set_state(&self, state: PlayerState) {
        self.with(|s| s.set_state(state));
    }

    pub fn set_look(&self, look: Vec<u8>) {
        self.with(|s| s.set_look(look));
    }

    pub fn set_lobby(&self, key: u32) {
        self.with(|s| s.set_lobby(key));
    }

    pub fn set_name(&self, name: &str) {
        self.with(|s| s.set_name(name));
    }

    /// [`Session::set_forms`].
    pub fn set_forms(&self, forms: Option<Arc<dyn FormCheck>>) {
        self.with(|s| s.set_forms(forms));
    }

    /// [`Session::add_rule`].
    pub fn add_rule(&self, kind: Kind, rule: Box<dyn Rule>) {
        self.with(|s| s.add_rule(kind, rule));
    }

    /// [`Session::set_rate`].
    pub fn set_rate(&self, kind: Kind, rate: Rate) {
        self.with(|s| s.set_rate(kind, rate));
    }

    /// [`Session::guard_stats`].
    pub fn guard_stats(&self) -> Stats {
        self.with(|s| s.guard_stats())
    }

    /// [`Session::bad_messages`].
    pub fn bad_messages(&self, id: u8) -> u64 {
        self.with(|s| s.bad_messages(id))
    }

    /// [`Session::send_reliable`].
    #[must_use = "a full stream refuses the item"]
    pub fn send_reliable(&self, to: Option<u8>, item: Vec<u8>) -> bool {
        self.with(|s| s.send_reliable(to, item))
    }

    pub fn traffic(&self) -> Vec<Traffic> {
        self.with(|s| s.traffic())
    }

    /// [`Session::paths`].
    pub fn paths(&self) -> Vec<(u8, Option<SocketAddr>, Option<std::time::Duration>)> {
        self.with(|s| s.paths())
    }

    pub fn events(&self) -> Vec<Event> {
        self.with(|s| s.take_events())
    }

    /// The host's address once one of the candidates answered (guests only).
    pub fn host_addr(&self) -> Option<SocketAddr> {
        self.with(|s| s.host_addr())
    }

    /// [`Session::set_npcs`].
    pub fn set_npcs(&self, npcs: Vec<NpcState>) {
        self.with(|s| s.set_npcs(npcs));
    }

    /// [`Session::send_hit`].
    pub fn send_hit(&self, hit: HitView) {
        self.with(|s| s.send_hit(hit));
    }

    /// [`Session::send_hit_to`].
    pub fn send_hit_to(&self, id: u8, by: u32, hit: HitView) {
        self.with(|s| s.send_hit_to(id, by, hit));
    }

    /// [`Session::set_clock`].
    pub fn set_clock(&self, clock: ClockView) {
        self.with(|s| s.set_clock(clock));
    }

    /// [`Session::set_scene`].
    pub fn set_scene(&self, scene: SceneView) {
        self.with(|s| s.set_scene(scene));
    }

    /// [`Session::set_lines`].
    pub fn set_lines(&self, lines: Vec<LineView>) {
        self.with(|s| s.set_lines(lines));
    }

    /// [`Session::set_presence`].
    pub fn set_presence(&self, list: u8, refs: Vec<(u32, bool)>) {
        self.with(|s| s.set_presence(list, refs));
    }

    /// [`Session::set_wear`].
    pub fn set_wear(&self, npcs: Vec<(u32, Vec<u32>)>) {
        self.with(|s| s.set_wear(npcs));
    }

    /// [`Session::send_door`].
    pub fn send_door(&self, reference: u32, open: bool) {
        self.with(|s| s.send_door(reference, open));
    }

    /// [`Session::send_unlock`].
    pub fn send_unlock(&self, reference: u32, terminal: bool) {
        self.with(|s| s.send_unlock(reference, terminal));
    }

    /// [`Session::send_revive`].
    pub fn send_revive(&self, target: u8, perks: u8) {
        self.with(|s| s.send_revive(target, perks));
    }

    /// [`Session::relay_door`].
    pub fn relay_door(&self, except: u8, reference: u32, open: bool) {
        self.with(|s| s.relay_door(except, reference, open));
    }

    /// [`Session::relay_unlock`].
    pub fn relay_unlock(&self, except: u8, reference: u32, terminal: bool) {
        self.with(|s| s.relay_unlock(except, reference, terminal));
    }

    /// [`Session::relay_revive`].
    pub fn relay_revive(&self, target: u8, by: u8, perks: u8) {
        self.with(|s| s.relay_revive(target, by, perks));
    }

    /// [`Session::note_bad`].
    pub fn note_bad(&self, id: u8) {
        self.with(|s| s.note_bad(id));
    }

    /// [`Session::send_actors`].
    pub fn send_actors(&self, to: u8, poses: &[ActorPose]) {
        self.with(|s| s.send_actors(to, poses));
    }

    /// Stop without saying Bye: the game starts again (to load a save) and comes back
    /// with the same token before the host even marks it lost.
    pub fn abandon(mut self) {
        self.quiet = true;
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if !self.quiet {
            self.with(|s| s.bye());
        }
    }
}

/// `a` is newer than `b`, allowing for the counter wrapping.
fn newer(a: u32, b: u32) -> bool {
    a != b && a.wrapping_sub(b) < u32::MAX / 2
}

/// A state to take after `last`: newer, or so much older that its game started again
/// (a load restarts it, and its count with it) rather than a packet late on the way.
fn fresh(seq: u32, last: u32) -> bool {
    newer(seq, last) || last.wrapping_sub(seq) > RESTARTED
}

/// A session token no other guest will have: from the operating system's random source,
/// else std's per-process random hash keys mixed with the time. Never 0.
pub fn new_token() -> u64 {
    if let Some(t) = getrandom::u64().ok().filter(|&t| t != 0) {
        return t;
    }
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    h.write_u32(std::process::id());
    h.finish().max(1)
}

fn put_name(b: &mut Vec<u8>, name: &str) {
    let name = crate::guard::clean_name(name);
    b.push(name.len() as u8);
    b.extend_from_slice(name.as_bytes());
}

fn encode(msg: &Msg) -> Vec<u8> {
    let mut b = Vec::with_capacity(64);
    b.extend_from_slice(&MAGIC);
    b.push(VERSION);
    match msg {
        Msg::Hello { token, lobby, proof, name } => {
            b.push(1);
            b.extend_from_slice(&token.to_le_bytes());
            b.extend_from_slice(&lobby.to_le_bytes());
            b.extend_from_slice(&proof.to_le_bytes());
            put_name(&mut b, name);
        }
        Msg::Welcome { id, proof } => {
            b.extend_from_slice(&[2, *id]);
            b.extend_from_slice(&proof.to_le_bytes());
        }
        Msg::Full { proof } => {
            b.push(3);
            b.extend_from_slice(&proof.to_le_bytes());
        }
        Msg::Who => b.push(6),
        Msg::Kicked { banned } => b.extend_from_slice(&[40, u8::from(*banned)]),
        Msg::Ping => b.push(7),
        Msg::Names { names } => {
            let names: Vec<_> = names.iter().take(MAX_GUESTS + 1).collect();
            b.extend_from_slice(&[21, names.len() as u8]);
            for (id, name) in names {
                b.push(*id);
                put_name(&mut b, name);
            }
        }
        Msg::Look { id, look } => {
            let look = &look[..look.len().min(LOOK_MAX)];
            b.extend_from_slice(&[8, *id]);
            b.extend_from_slice(&(look.len() as u16).to_le_bytes());
            b.extend_from_slice(look);
        }
        Msg::State { id, seq, state } => {
            b.extend_from_slice(&[4, *id]);
            b.extend_from_slice(&seq.to_le_bytes());
            let (kind, form) = match state.place {
                Place::Interior(f) => (0u8, f),
                Place::Exterior(f) => (1u8, f),
            };
            b.push(kind);
            b.extend_from_slice(&form.to_le_bytes());
            for v in state.feet.iter().chain([&state.yaw, &state.pitch]) {
                b.extend_from_slice(&v.to_le_bytes());
            }
            b.push(state.flags);
            b.extend_from_slice(&state.weapon.to_le_bytes());
            b.extend_from_slice(&state.attacks.to_le_bytes());
            b.push(state.attack_kind);
            b.extend_from_slice(&state.reloads.to_le_bytes());
            let worn = state.worn();
            b.push(worn.len() as u8);
            for w in worn {
                b.extend_from_slice(&w.to_le_bytes());
            }
            b.push(state.pipboy);
            b.extend_from_slice(&state.furniture.to_le_bytes());
            b.push(state.more);
            let e = &state.emote;
            b.extend_from_slice(&[e.id, e.variant, e.phase, e.step]);
            b.extend_from_slice(&e.t.to_le_bytes());
        }
        Msg::Bye { id } => b.extend_from_slice(&[5, *id]),
        Msg::Rel { epoch, frame } => {
            b.push(30);
            b.extend_from_slice(&epoch.to_le_bytes());
            b.extend_from_slice(&frame.ack.to_le_bytes());
            b.extend_from_slice(&frame.first.to_le_bytes());
            b.extend_from_slice(&(frame.items.len() as u16).to_le_bytes());
            for item in &frame.items {
                b.extend_from_slice(&(item.len() as u16).to_le_bytes());
                b.extend_from_slice(item);
            }
        }
        Msg::Npcs { seq, npcs } => {
            b.push(9);
            b.extend_from_slice(&seq.to_le_bytes());
            let npcs = &npcs[..npcs.len().min(NPCS_PER_PACKET)];
            b.push(npcs.len() as u8);
            for n in npcs {
                b.extend_from_slice(&n.reference.to_le_bytes());
                for v in n.feet.iter().chain([&n.heading, &n.pitch]) {
                    b.extend_from_slice(&v.to_le_bytes());
                }
                b.push(n.flags);
                b.extend_from_slice(&n.weapon.to_le_bytes());
                b.push(n.attacks);
                b.extend_from_slice(&n.furniture.to_le_bytes());
                b.extend_from_slice(&n.base.to_le_bytes());
                b.extend_from_slice(&((n.level & 0x7FFF) | if n.hostile { 0x8000 } else { 0 }).to_le_bytes());
                b.push(n.rolls.min(127) | if n.all_levels { 128 } else { 0 });
                b.extend_from_slice(&n.space.to_le_bytes());
            }
        }
        Msg::Hit { id, by, hit } => {
            b.push(10);
            b.extend_from_slice(&id.to_le_bytes());
            b.extend_from_slice(&by.to_le_bytes());
            b.extend_from_slice(&hit.target.to_le_bytes());
            b.extend_from_slice(&hit.dealt.to_le_bytes());
            b.extend_from_slice(&hit.weapon.to_le_bytes());
            b.extend_from_slice(&hit.location.to_le_bytes());
            b.push(u8::from(hit.critical));
            match hit.limb {
                Some(l) => {
                    b.push(1);
                    b.extend_from_slice(&l.condition.to_le_bytes());
                    b.extend_from_slice(&l.health_percent.to_le_bytes());
                    b.extend_from_slice(&l.damage.to_le_bytes());
                    b.push(u8::from(l.points));
                }
                None => b.push(0),
            }
        }
        Msg::Unlock { reference, terminal } => {
            b.push(12);
            b.extend_from_slice(&reference.to_le_bytes());
            b.push(u8::from(*terminal));
        }
        Msg::Clock(c) => {
            b.push(13);
            for v in [c.year, c.month, c.day, c.hour, c.days_passed] {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        Msg::Revive { target, by, perks } => b.extend_from_slice(&[14, *target, *by, *perks]),
        Msg::Lines { seq, lines } => {
            b.push(17);
            b.extend_from_slice(&seq.to_le_bytes());
            let lines = &lines[..lines.len().min(LINES_MAX)];
            b.push(lines.len() as u8);
            for l in lines {
                for v in [l.speaker, l.target, l.topic, l.info] {
                    b.extend_from_slice(&v.to_le_bytes());
                }
                b.extend_from_slice(&l.remaining.to_le_bytes());
                b.push(l.flags);
                b.extend_from_slice(&l.conversation.to_le_bytes());
            }
        }
        Msg::Presence { list, serial, page, pages, refs } => {
            b.extend_from_slice(&[19, *list]);
            b.extend_from_slice(&serial.to_le_bytes());
            b.extend_from_slice(&page.to_le_bytes());
            b.extend_from_slice(&pages.to_le_bytes());
            let refs = &refs[..refs.len().min(PRESENCE_PAGE)];
            b.extend_from_slice(&(refs.len() as u16).to_le_bytes());
            for (r, on) in refs {
                b.extend_from_slice(&r.to_le_bytes());
                b.push(u8::from(*on));
            }
        }
        Msg::Door { reference, open } => {
            b.push(20);
            b.extend_from_slice(&reference.to_le_bytes());
            b.push(u8::from(*open));
        }
        Msg::Wear { npcs } => {
            b.push(23);
            let npcs = &npcs[..npcs.len().min(WEAR_PAGE)];
            b.push(npcs.len() as u8);
            for (r, items) in npcs {
                b.extend_from_slice(&r.to_le_bytes());
                let items = &items[..items.len().min(WEAR_ITEMS)];
                b.push(items.len() as u8);
                for i in items {
                    b.extend_from_slice(&i.to_le_bytes());
                }
            }
        }
        Msg::Scene(v) => {
            let mods = &v.mods[..v.mods.len().min(SCENE_MODS_MAX)];
            b.extend_from_slice(&[24, v.locks, mods.len() as u8]);
            for (m, at) in mods {
                b.extend_from_slice(&m.to_le_bytes());
                b.extend_from_slice(&at.to_le_bytes());
            }
            let worn = &v.worn[..v.worn.len().min(SCENE_MODS_MAX)];
            b.push(worn.len() as u8);
            for (item, locked) in worn {
                b.extend_from_slice(&item.to_le_bytes());
                b.push(u8::from(*locked));
            }
            let swaps: Vec<_> = v.swaps.iter().filter(|(_, s, t)| s.len() <= SWAP_TEXT_MAX && t.len() <= SWAP_TEXT_MAX).take(SCENE_SWAPS_MAX).collect();
            b.push(swaps.len() as u8);
            for (on, shape, texture) in swaps {
                b.extend_from_slice(&on.to_le_bytes());
                for s in [shape, texture] {
                    b.push(s.len() as u8);
                    b.extend_from_slice(s.as_bytes());
                }
            }
        }
        Msg::Actors { seq, ms, poses } => {
            b.push(31);
            b.extend_from_slice(&seq.to_le_bytes());
            b.extend_from_slice(&ms.to_le_bytes());
            let poses = &poses[..poses.len().min(ACTORS_PER_PACKET)];
            b.push(poses.len() as u8);
            for p in poses {
                p.write(&mut b);
            }
        }
    }
    b
}

/// A message as [`encode`] wrote it; None for anything else. Every list has its bound,
/// and every float read must be finite.
fn decode(b: &[u8]) -> Option<Msg> {
    let rest = b.strip_prefix(&MAGIC)?;
    let (&version, rest) = rest.split_first()?;
    if version != VERSION {
        return None;
    }
    let (&kind, rest) = rest.split_first()?;
    let mut r = Reader(rest);
    let msg = match kind {
        1 => {
            let (token, lobby, proof) = (r.u64()?, r.u32()?, r.u64()?);
            Msg::Hello { token, lobby, proof, name: r.name()? }
        }
        2 => Msg::Welcome { id: r.u8()?, proof: r.u64()? },
        3 => Msg::Full { proof: r.u64()? },
        4 => {
            let id = r.u8()?;
            let seq = r.u32()?;
            let place = match (r.u8()?, r.u32()?) {
                (0, f) => Place::Interior(f),
                (1, f) => Place::Exterior(f),
                _ => return None,
            };
            let feet = [r.finite()?, r.finite()?, r.finite()?];
            let (yaw, pitch) = (r.finite()?, r.finite()?);
            let mut state = PlayerState::at(place, feet, yaw);
            state.pitch = pitch;
            state.flags = r.u8()?;
            state.weapon = r.u32()?;
            state.attacks = r.u16()?;
            state.attack_kind = r.u8()?;
            state.reloads = r.u16()?;
            let n = r.u8()? as usize;
            if n > MAX_WORN {
                return None;
            }
            for w in &mut state.worn[..n] {
                *w = r.u32()?;
            }
            state.worn_len = n as u8;
            state.pipboy = r.u8()?;
            state.furniture = r.u32()?;
            state.more = r.u8()?;
            state.emote = EmoteState { id: r.u8()?, variant: r.u8()?, phase: r.u8()?, step: r.u8()?, t: r.finite()? };
            Msg::State { id, seq, state }
        }
        5 => Msg::Bye { id: r.u8()? },
        6 => Msg::Who,
        40 => Msg::Kicked { banned: r.u8()? != 0 },
        7 => Msg::Ping,
        8 => {
            let id = r.u8()?;
            let len = r.u16()? as usize;
            if len > LOOK_MAX {
                return None;
            }
            Msg::Look { id, look: r.take(len)?.to_vec() }
        }
        21 => {
            let n = r.u8()? as usize;
            if n > MAX_GUESTS + 1 {
                return None;
            }
            let mut names = Vec::with_capacity(n);
            for _ in 0..n {
                let id = r.u8()?;
                names.push((id, r.name()?));
            }
            Msg::Names { names }
        }
        30 => {
            let epoch = r.u16()?;
            let ack = r.u32()?;
            let first = r.u32()?;
            let n = r.u16()? as usize;
            if n > REL_ITEMS_MAX {
                return None;
            }
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                let len = r.u16()? as usize;
                if len > ITEM_MAX {
                    return None;
                }
                items.push(r.take(len)?.to_vec());
            }
            Msg::Rel { epoch, frame: Frame { ack, first, items } }
        }
        9 => {
            let seq = r.u32()?;
            let n = r.u8()? as usize;
            if n > NPCS_PER_PACKET {
                return None;
            }
            let mut npcs = Vec::with_capacity(n);
            for _ in 0..n {
                let reference = r.u32()?;
                let feet = [r.finite()?, r.finite()?, r.finite()?];
                let (heading, pitch) = (r.finite()?, r.finite()?);
                let (flags, weapon, attacks, furniture, base, level, look, space) = (r.u8()?, r.u32()?, r.u8()?, r.u32()?, r.u32()?, r.u16()?, r.u8()?, r.u32()?);
                npcs.push(NpcState { reference, feet, heading, pitch, flags, weapon, attacks, furniture, base, level: level & 0x7FFF, all_levels: look & 128 != 0, rolls: look & 127, hostile: level & 0x8000 != 0, space });
            }
            Msg::Npcs { seq, npcs }
        }
        10 => {
            let (id, by) = (r.u16()?, r.u32()?);
            let (target, dealt, weapon, location, critical) = (r.u32()?, r.finite()?, r.u32()?, r.i32()?, r.u8()? != 0);
            let limb = match r.u8()? {
                0 => None,
                1 => Some(LimbView { condition: r.u16()?, health_percent: r.finite64()?, damage: r.finite64()?, points: r.u8()? != 0 }),
                _ => return None,
            };
            Msg::Hit { id, by, hit: HitView { target, dealt, weapon, location, critical, limb } }
        }
        12 => Msg::Unlock { reference: r.u32()?, terminal: r.u8()? != 0 },
        13 => Msg::Clock(ClockView { year: r.finite()?, month: r.finite()?, day: r.finite()?, hour: r.finite()?, days_passed: r.finite()? }),
        14 => Msg::Revive { target: r.u8()?, by: r.u8()?, perks: r.u8()? },
        17 => {
            let seq = r.u32()?;
            let n = r.u8()? as usize;
            if n > LINES_MAX {
                return None;
            }
            let mut lines = Vec::with_capacity(n);
            for _ in 0..n {
                let (speaker, target, topic, info) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
                let remaining = r.finite()?;
                lines.push(LineView { speaker, target, topic, info, remaining, flags: r.u8()?, conversation: r.u16()? });
            }
            Msg::Lines { seq, lines }
        }
        19 => {
            let (list, serial, page, pages, n) = (r.u8()?, r.u32()?, r.u16()?, r.u16()?, r.u16()? as usize);
            if n > PRESENCE_PAGE || page >= pages || list as usize >= LISTS {
                return None;
            }
            let mut refs = Vec::with_capacity(n);
            for _ in 0..n {
                refs.push((r.u32()?, r.u8()? != 0));
            }
            Msg::Presence { list, serial, page, pages, refs }
        }
        20 => Msg::Door { reference: r.u32()?, open: r.u8()? != 0 },
        23 => {
            let n = r.u8()? as usize;
            if n > WEAR_PAGE {
                return None;
            }
            let mut npcs = Vec::with_capacity(n);
            for _ in 0..n {
                let reference = r.u32()?;
                let k = r.u8()? as usize;
                if k > WEAR_ITEMS {
                    return None;
                }
                npcs.push((reference, (0..k).map(|_| r.u32()).collect::<Option<Vec<_>>>()?));
            }
            Msg::Wear { npcs }
        }
        24 => {
            let locks = r.u8()?;
            let n = r.u8()? as usize;
            if n > SCENE_MODS_MAX {
                return None;
            }
            let mut mods = Vec::with_capacity(n);
            for _ in 0..n {
                mods.push((r.u32()?, r.finite()?));
            }
            let n = r.u8()? as usize;
            if n > SCENE_MODS_MAX {
                return None;
            }
            let mut worn = Vec::with_capacity(n);
            for _ in 0..n {
                worn.push((r.u32()?, r.u8()? != 0));
            }
            let n = r.u8()? as usize;
            if n > SCENE_SWAPS_MAX {
                return None;
            }
            let mut swaps = Vec::with_capacity(n);
            for _ in 0..n {
                let on = r.u32()?;
                let (shape, texture) = (r.text(SWAP_TEXT_MAX)?, r.text(SWAP_TEXT_MAX)?);
                swaps.push((on, shape, texture));
            }
            Msg::Scene(SceneView { locks, mods, worn, swaps })
        }
        31 => {
            let (seq, ms) = (r.u32()?, r.u32()?);
            let n = r.u8()? as usize;
            if n > ACTORS_PER_PACKET {
                return None;
            }
            let mut pr = crate::repl::Reader(r.take(n * ACTOR_BYTES)?);
            let poses = (0..n).map(|_| ActorPose::read(&mut pr)).collect::<Option<Vec<_>>>()?;
            Msg::Actors { seq, ms, poses }
        }
        _ => return None,
    };
    r.0.is_empty().then_some(msg)
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes(b.try_into().unwrap()))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    }
    fn i32(&mut self) -> Option<i32> {
        self.take(4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
    }
    /// A float, None unless finite.
    fn finite(&mut self) -> Option<f32> {
        self.take(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).filter(|v| v.is_finite())
    }
    /// A double, None unless finite.
    fn finite64(&mut self) -> Option<f64> {
        self.take(8).map(|b| f64::from_le_bytes(b.try_into().unwrap())).filter(|v| v.is_finite())
    }
    /// A text: its length byte (at most `max`), then its bytes.
    fn text(&mut self, max: usize) -> Option<String> {
        let len = self.u8()? as usize;
        if len > max {
            return None;
        }
        Some(String::from_utf8_lossy(self.take(len)?).into_owned())
    }
    /// A name: its length byte (at most [`NAME_MAX`]), then its bytes.
    fn name(&mut self) -> Option<String> {
        let len = self.u8()? as usize;
        if len > NAME_MAX {
            return None;
        }
        Some(String::from_utf8_lossy(self.take(len)?).into_owned())
    }
}

#[cfg(test)]
impl Session {
    /// A host on loopback UDP `port` (0 picks a free one), for tests.
    fn host(port: u16, secret: crate::seam::SharedSecret) -> std::io::Result<Self> {
        Ok(Self::host_on(Box::new(crate::seam::loopback::Udp::host(port)?), Box::new(secret)))
    }

    /// A guest on loopback UDP, for tests.
    fn join(candidates: &[SocketAddr], lobby: u32, secret: crate::seam::SharedSecret, name: &str, token: u64) -> std::io::Result<Self> {
        Ok(Self::join_on(Box::new(crate::seam::loopback::Udp::guest()?), Box::new(secret), candidates, lobby, name, token))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seam::memory::{pair, Hub};
    use crate::seam::SharedSecret;

    /// The three copies of one door, unlock or revive are taken once; the other way, or
    /// the same again after the window, is new.
    #[test]
    fn the_copies_of_a_door_are_taken_once() {
        let mut c = Copies::default();
        let t = Instant::now();
        assert!(c.first(door_key(0x00AB_C001, true), t));
        assert!(!c.first(door_key(0x00AB_C001, true), t));
        assert!(!c.first(door_key(0x00AB_C001, true), t + Duration::from_millis(5)));
        assert!(c.first(door_key(0x00AB_C001, false), t + Duration::from_millis(400)), "closed again");
        assert!(c.first(unlock_key(0x00AB_C001, true), t), "an unlock of the same reference");
        assert!(c.first(revive_key(2, 3, 0), t));
        assert!(!c.first(revive_key(2, 3, 0), t));
        assert!(c.first(revive_key(2, 4, 0), t), "another reviver");
        assert!(c.first(door_key(0x00AB_C001, true), t + COPY_WINDOW), "after the window");
    }

    /// A door that goes open, close, open inside the window is three events: the second
    /// open is not a copy of the first.
    #[test]
    fn a_door_opened_closed_opened_inside_the_window_is_three_events() {
        let mut c = Copies::default();
        let t = Instant::now();
        assert!(c.first(door_key(0x00AB_C002, true), t));
        assert!(c.first(door_key(0x00AB_C002, false), t + Duration::from_millis(200)));
        assert!(c.first(door_key(0x00AB_C002, true), t + Duration::from_millis(400)), "opened again");
        assert!(!c.first(door_key(0x00AB_C002, true), t + Duration::from_millis(405)), "its copy");
        assert!(!c.first(door_key(0x00AB_C002, true), t + Duration::from_millis(410)), "and its second copy");
    }

    fn state(x: f32) -> PlayerState {
        PlayerState::at(Place::Exterior(0xA001), [x, 2.0, 3.0], 0.5)
    }

    fn secret() -> SharedSecret {
        SharedSecret::new(0x5EC2_E7AB_CDEF_0123).unwrap()
    }

    #[test]
    fn messages_round_trip() {
        for msg in [
            Msg::Hello { token: 0xDEAD_BEEF_0123, lobby: 0x1234_5678, proof: 0x0123_4567_89AB_CDEF, name: "Courier".into() },
            Msg::Welcome { id: 2, proof: u64::MAX },
            Msg::Full { proof: 7 },
            Msg::Names { names: vec![(0, "Alpha".into()), (2, "Bravo".into()), (3, String::new())] },
            Msg::Who,
            Msg::Ping,
            Msg::Look { id: 2, look: vec![1, 2, 3, 250] },
            Msg::Rel { epoch: 3, frame: Frame { ack: 9, first: u32::MAX, items: vec![vec![1, 2], Vec::new(), vec![7; ITEM_MAX]] } },
            Msg::State { id: 1, seq: 7, state: state(-12.5) },
            Msg::State { id: 0, seq: u32::MAX, state: PlayerState { place: Place::Interior(0xA002), ..state(0.0) } },
            Msg::State {
                id: 2,
                seq: 9,
                state: PlayerState {
                    pitch: -0.3,
                    flags: flag::DRAWN | flag::SIGHTS,
                    weapon: 0xA003,
                    attacks: 65535,
                    attack_kind: attack::POWER_LEFT,
                    reloads: 3,
                    worn: [1, 2, 3, 0, 0, 0, 0, 0, 0, 0],
                    worn_len: 3,
                    pipboy: 2,
                    furniture: 0x0011_C601,
                    more: more_flag::SWIMMING,
                    emote: EmoteState { id: 5, variant: 1, phase: emote::STAY, step: 2, t: 3.25 },
                    ..state(4.0)
                },
            },
            Msg::Bye { id: 3 },
        ] {
            assert_eq!(decode(&encode(&msg)), Some(msg));
        }
    }

    fn npc(reference: u32) -> NpcState {
        NpcState { reference, feet: [1.5, -2.0, 300.25], heading: 1.0, pitch: -0.25, flags: npc_flag::MOVING | npc_flag::DRAWN, weapon: 0x0E37, attacks: 200, furniture: 0, base: 0x00AB_C104, level: 20, all_levels: true, rolls: 3, hostile: true, space: 0xDA726 }
    }

    fn hit(target: u32) -> HitView {
        HitView { target, dealt: 12.5, weapon: 0x0E37, location: 1, critical: true, limb: Some(LimbView { condition: 25, health_percent: 37.5, damage: 6.25, points: true }) }
    }

    fn line() -> LineView {
        LineView { speaker: 0x00AB_C106, target: LineView::HOST, topic: 0x0011_2233, info: 0x0011_2244, remaining: 3.25, flags: LineView::SAY_TO | LineView::TWO_D, conversation: 2 }
    }

    fn scene() -> SceneView {
        SceneView { locks: 0x7F, mods: vec![(0x0101_013B, 0.25), (0x00AB_C101, 3.5)], worn: vec![(0x0100_F3A4, true), (0x0100_6E8F, false)], swaps: vec![(0x0101_38B2, "screen01:0".into(), "textures/slide03.dds".into())] }
    }

    fn pose(reference: u32) -> ActorPose {
        ActorPose { reference, feet: [100.0, -200.0, 50.0], heading: 1.0, pitch: 0.0, flags: npc_flag::MOVING, attacks: 3 }.quantised()
    }

    #[test]
    fn world_messages_round_trip() {
        for msg in [
            Msg::Npcs { seq: 7, npcs: Vec::new() },
            Msg::Npcs { seq: u32::MAX, npcs: (0..NPCS_PER_PACKET as u32).map(npc).collect() },
            Msg::Hit { id: 65_000, by: 0, hit: hit(0x00AB_C104) },
            Msg::Hit { id: 1, by: 0x00AB_C105, hit: HitView { target: 7, dealt: 0.0, weapon: 0, location: HitView::OBJECT, critical: false, limb: None } },
            Msg::Unlock { reference: 0x0001_2345, terminal: true },
            Msg::Clock(ClockView { year: 2281.0, month: 9.0, day: 19.0, hour: 13.25, days_passed: 6.05 }),
            Msg::Scene(SceneView::default()),
            Msg::Scene(scene()),
            Msg::Lines { seq: 3, lines: Vec::new() },
            Msg::Lines { seq: u32::MAX, lines: vec![line(); LINES_MAX] },
            Msg::Presence { list: LIST_ENABLED, serial: 7, page: 0, pages: 1, refs: Vec::new() },
            Msg::Presence { list: LIST_OPEN, serial: 8, page: 2, pages: 3, refs: vec![(0x00AB_C107, true); PRESENCE_PAGE] },
            Msg::Wear { npcs: vec![(0x0010_C0DE, vec![0x00AB_C102, 0x00AB_C103]), (0xFF00_0643, Vec::new())] },
            Msg::Wear { npcs: vec![(1, vec![2; WEAR_ITEMS]); WEAR_PAGE] },
            Msg::Door { reference: 0x0010_C0DE, open: true },
            Msg::Revive { target: 2, by: 0, perks: revive_perk::CURE_RADS },
            Msg::Actors { seq: 4, ms: 123_456, poses: Vec::new() },
            Msg::Actors { seq: 5, ms: 0, poses: (0..ACTORS_PER_PACKET as u32).map(pose).collect() },
        ] {
            let bytes = encode(&msg);
            assert!(bytes.len() <= 1500, "{} bytes: {msg:?}", bytes.len());
            assert_eq!(decode(&bytes), Some(msg));
        }
    }

    /// The world's floats must be finite: a message with one that is not does not parse.
    #[test]
    fn world_messages_not_finite_do_not_parse() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let npcs = |n: NpcState| encode(&Msg::Npcs { seq: 1, npcs: vec![n] });
            assert!(decode(&npcs(NpcState { feet: [0.0, bad, 0.0], ..npc(1) })).is_none());
            assert!(decode(&npcs(NpcState { heading: bad, ..npc(1) })).is_none());
            assert!(decode(&npcs(NpcState { pitch: bad, ..npc(1) })).is_none());
            assert!(decode(&encode(&Msg::Hit { id: 1, by: 0, hit: HitView { dealt: bad, ..hit(1) } })).is_none());
            let limb = |health_percent: f64, damage: f64| HitView { limb: Some(LimbView { condition: 1, health_percent, damage, points: false }), ..hit(1) };
            assert!(decode(&encode(&Msg::Hit { id: 1, by: 0, hit: limb(f64::from(bad), 1.0) })).is_none());
            assert!(decode(&encode(&Msg::Hit { id: 1, by: 0, hit: limb(1.0, f64::from(bad)) })).is_none());
            assert!(decode(&encode(&Msg::Clock(ClockView { hour: bad, ..ClockView::default() }))).is_none());
            assert!(decode(&encode(&Msg::Lines { seq: 1, lines: vec![LineView { remaining: bad, ..line() }] })).is_none());
            assert!(decode(&encode(&Msg::Scene(SceneView { mods: vec![(1, bad)], ..SceneView::default() }))).is_none());
        }
    }

    /// Every world list on the wire has its bound: one past it does not parse.
    #[test]
    fn world_lists_past_their_bounds_do_not_parse() {
        let header = |kind: u8| [&MAGIC[..], &[VERSION, kind]].concat();
        // One NPC too many.
        let mut b = encode(&Msg::Npcs { seq: 1, npcs: (0..NPCS_PER_PACKET as u32).map(npc).collect() });
        b[10] = NPCS_PER_PACKET as u8 + 1;
        let extra = b[11..11 + 45].to_vec();
        b.extend_from_slice(&extra);
        assert!(decode(&b).is_none());
        // One line too many.
        let mut b = encode(&Msg::Lines { seq: 1, lines: vec![line(); LINES_MAX] });
        b[10] = LINES_MAX as u8 + 1;
        let extra = b[11..11 + 23].to_vec();
        b.extend_from_slice(&extra);
        assert!(decode(&b).is_none());
        // A presence page too long, a page past its count, a list this build does not have.
        let presence = |list: u8, page: u16, pages: u16, n: u16| {
            let mut b = header(19);
            b.push(list);
            b.extend_from_slice(&1u32.to_le_bytes());
            b.extend_from_slice(&page.to_le_bytes());
            b.extend_from_slice(&pages.to_le_bytes());
            b.extend_from_slice(&n.to_le_bytes());
            b.extend(std::iter::repeat_n([1, 0, 0, 0, 1], n as usize).flatten());
            b
        };
        assert!(decode(&presence(LIST_OPEN, 0, 1, PRESENCE_PAGE as u16)).is_some());
        assert!(decode(&presence(LIST_OPEN, 0, 1, PRESENCE_PAGE as u16 + 1)).is_none());
        assert!(decode(&presence(LIST_OPEN, 1, 1, 0)).is_none());
        assert!(decode(&presence(LISTS as u8, 0, 1, 0)).is_none());
        // A wear page of too many NPCs, an NPC of too many items.
        let wear = |n: usize, k: usize| {
            let mut b = header(23);
            b.push(n as u8);
            for _ in 0..n {
                b.extend_from_slice(&1u32.to_le_bytes());
                b.push(k as u8);
                b.extend(std::iter::repeat_n(2u32.to_le_bytes(), k).flatten());
            }
            b
        };
        assert!(decode(&wear(WEAR_PAGE, WEAR_ITEMS)).is_some());
        assert!(decode(&wear(WEAR_PAGE + 1, 0)).is_none());
        assert!(decode(&wear(1, WEAR_ITEMS + 1)).is_none());
        // A scene of too many modifiers, worn items or swaps, or a swap's text too long.
        let mut b = header(24);
        b.extend_from_slice(&[0, SCENE_MODS_MAX as u8 + 1]);
        b.extend(std::iter::repeat_n([0u8; 8], SCENE_MODS_MAX + 1).flatten());
        b.extend_from_slice(&[0, 0]);
        assert!(decode(&b).is_none());
        let mut b = header(24);
        b.extend_from_slice(&[0, 0, SCENE_MODS_MAX as u8 + 1]);
        b.extend(std::iter::repeat_n([0u8; 5], SCENE_MODS_MAX + 1).flatten());
        b.push(0);
        assert!(decode(&b).is_none());
        let mut b = header(24);
        b.extend_from_slice(&[0, 0, 0, SCENE_SWAPS_MAX as u8 + 1]);
        b.extend(std::iter::repeat_n([0u8; 6], SCENE_SWAPS_MAX + 1).flatten());
        assert!(decode(&b).is_none());
        let mut b = header(24);
        b.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0, SWAP_TEXT_MAX as u8 + 1]);
        b.extend_from_slice(&[b'a'; SWAP_TEXT_MAX + 1]);
        b.push(0);
        assert!(decode(&b).is_none());
        // Too many actor poses.
        let mut b = encode(&Msg::Actors { seq: 1, ms: 0, poses: (0..ACTORS_PER_PACKET as u32).map(pose).collect() });
        b[14] = ACTORS_PER_PACKET as u8 + 1;
        b.extend_from_slice(&[0; ACTOR_BYTES]);
        assert!(decode(&b).is_none());
        // A hit's limb tag other than 0 or 1.
        let mut b = encode(&Msg::Hit { id: 1, by: 0, hit: HitView { limb: None, ..hit(1) } });
        *b.last_mut().unwrap() = 2;
        assert!(decode(&b).is_none());
        // The parts of the old protocol this one left out (another piece's) do not parse.
        for kind in [11, 15, 16, 18, 25, 26, 27] {
            let mut other = encode(&Msg::Ping);
            other[5] = kind;
            other.extend_from_slice(&[0; 16]);
            assert!(decode(&other).is_none(), "kind {kind}");
        }
    }

    /// Guest 1's address as a raw endpoint, after it was welcomed and its state seen.
    fn host_with_raw_guest() -> (Hub, Session, Session, Instant) {
        let hub = Hub::new();
        let mut host = Session::host_on(Box::new(hub.endpoint(addr(1))), Box::new(secret()));
        let mut a = Session::join_on(Box::new(hub.endpoint(addr(2))), Box::new(secret()), &[addr(1)], 0, "A", 1);
        let mut clock = Instant::now();
        a.set_state(state(1.0));
        run(&mut [&mut host, &mut a], &mut clock, Duration::from_millis(10), |seen| welcomed(&seen[1]) && seen[0].iter().any(|e| matches!(e, Event::State { id: 1, .. })));
        (hub, host, a, clock)
    }

    /// The world's messages only the host sends are dropped from a guest, and counted
    /// against it.
    #[test]
    fn a_guests_world_messages_for_guests_are_dropped() {
        let (hub, mut host, _a, mut clock) = host_with_raw_guest();
        let raw = hub.endpoint(addr(2));
        let msgs = [
            Msg::Npcs { seq: 1, npcs: vec![npc(1)] },
            Msg::Lines { seq: 1, lines: vec![line()] },
            Msg::Presence { list: LIST_ENABLED, serial: 1, page: 0, pages: 1, refs: vec![(1, true)] },
            Msg::Wear { npcs: vec![(1, vec![2])] },
            Msg::Clock(ClockView::default()),
            Msg::Scene(scene()),
            Msg::Actors { seq: 1, ms: 0, poses: vec![pose(1)] },
        ];
        for msg in &msgs {
            raw.send_to(&encode(msg), addr(1)).unwrap();
        }
        clock += Duration::from_millis(10);
        host.tick(clock);
        let events = host.take_events();
        assert!(!events.iter().any(|e| matches!(e, Event::Npcs(_) | Event::Lines(_) | Event::Presence { .. } | Event::Wear(_) | Event::Clock(_) | Event::Scene(_) | Event::Actors { .. })), "{events:?}");
        let stats = host.guard_stats();
        for kind in [Kind::Npcs, Kind::Lines, Kind::Presence, Kind::Wear, Kind::Clock, Kind::Scene, Kind::Actors] {
            assert!(stats.dropped.contains(&(kind, Reason::HostOnly, 1)), "{kind:?}: {stats:?}");
        }
        assert_eq!(host.bad_messages(1), msgs.len() as u64);
    }

    /// A guest's unlock and door reach the host's game only once the host's checks accept
    /// them, and the other guests only once the game passes them on (never the sender).
    #[test]
    fn an_unlock_is_passed_on_only_after_it_is_accepted() {
        /// Drops anything about reference 0xBAD.
        struct NotBad;
        impl Rule for NotBad {
            fn check(&self, _: &crate::guard::Ctx, subject: &crate::guard::Subject) -> Verdict {
                match subject {
                    crate::guard::Subject::Msg(Msg::Unlock { reference: 0xBAD, .. } | Msg::Door { reference: 0xBAD, .. }) => Verdict::Drop(Reason::Rule("bad")),
                    _ => Verdict::Accept,
                }
            }
        }
        let hub = Hub::new();
        let mut host = Session::host_on(Box::new(hub.endpoint(addr(1))), Box::new(secret()));
        host.add_rule(Kind::Unlock, Box::new(NotBad));
        host.add_rule(Kind::Door, Box::new(NotBad));
        let mut a = Session::join_on(Box::new(hub.endpoint(addr(2))), Box::new(secret()), &[addr(1)], 0, "A", 1);
        let mut b = Session::join_on(Box::new(hub.endpoint(addr(3))), Box::new(secret()), &[addr(1)], 0, "B", 2);
        let mut clock = Instant::now();
        let step = Duration::from_millis(10);
        run(&mut [&mut host, &mut a, &mut b], &mut clock, step, |seen| welcomed(&seen[1]) && welcomed(&seen[2]));
        a.send_unlock(0xBAD, false);
        a.send_door(0xBAD, true);
        a.send_unlock(0x600D, true);
        a.send_door(0x600D, false);
        let ida = a.id().unwrap();
        let mut seen = settle(&mut [&mut host, &mut a, &mut b], &mut clock, step);
        assert!(seen[0].contains(&Event::Unlocked { id: ida, reference: 0x600D, terminal: true }));
        assert!(seen[0].contains(&Event::Door { id: ida, reference: 0x600D, open: false }));
        assert!(!seen[0].iter().any(|e| matches!(e, Event::Unlocked { reference: 0xBAD, .. } | Event::Door { reference: 0xBAD, .. })), "{:?}", seen[0]);
        // Nothing reaches the other guest before the game passes it on.
        assert!(!seen[2].iter().any(|e| matches!(e, Event::Unlocked { .. } | Event::Door { .. })), "{:?}", seen[2]);
        assert!(host.guard_stats().dropped.contains(&(Kind::Unlock, Reason::Rule("bad"), COPIES as u64)));
        // The game's checks passed: the others have it, the sender does not hear its own.
        host.relay_unlock(ida, 0x600D, true);
        host.relay_door(ida, 0x600D, false);
        seen = settle(&mut [&mut host, &mut a, &mut b], &mut clock, step);
        assert!(seen[2].contains(&Event::Unlocked { id: 0, reference: 0x600D, terminal: true }), "{:?}", seen[2]);
        assert!(seen[2].contains(&Event::Door { id: 0, reference: 0x600D, open: false }), "{:?}", seen[2]);
        assert!(!seen[1].iter().any(|e| matches!(e, Event::Unlocked { .. } | Event::Door { .. })), "{:?}", seen[1]);
        // A guest cannot pass anything on.
        b.relay_unlock(0, 0x0BAD, true);
        b.relay_door(0, 0x0BAD, true);
        seen = settle(&mut [&mut host, &mut a, &mut b], &mut clock, step);
        assert!(!seen[1].iter().any(|e| matches!(e, Event::Unlocked { .. } | Event::Door { .. })), "{:?}", seen[1]);
    }

    /// A guest's revive of another guest reaches the host's game, and its target only once
    /// the game passes it on, from its reviver.
    #[test]
    fn a_guests_revive_of_another_waits_for_the_host() {
        let hub = Hub::new();
        let mut host = Session::host_on(Box::new(hub.endpoint(addr(1))), Box::new(secret()));
        let mut a = Session::join_on(Box::new(hub.endpoint(addr(2))), Box::new(secret()), &[addr(1)], 0, "A", 1);
        let mut b = Session::join_on(Box::new(hub.endpoint(addr(3))), Box::new(secret()), &[addr(1)], 0, "B", 2);
        let mut clock = Instant::now();
        let step = Duration::from_millis(10);
        run(&mut [&mut host, &mut a, &mut b], &mut clock, step, |seen| welcomed(&seen[1]) && welcomed(&seen[2]));
        let (ida, idb) = (a.id().unwrap(), b.id().unwrap());
        a.send_revive(idb, 0xFF);
        let mut seen = settle(&mut [&mut host, &mut a, &mut b], &mut clock, step);
        assert!(seen[0].contains(&Event::ReviveAsked { by: ida, target: idb, perks: revive_perk::KNOWN }), "{:?}", seen[0]);
        assert!(!seen[0].iter().any(|e| matches!(e, Event::Revived { .. })), "{:?}", seen[0]);
        assert!(!seen[2].iter().any(|e| matches!(e, Event::Revived { .. })), "{:?}", seen[2]);
        host.relay_revive(idb, ida, revive_perk::KNOWN);
        seen = settle(&mut [&mut host, &mut a, &mut b], &mut clock, step);
        assert!(seen[2].contains(&Event::Revived { by: ida, perks: revive_perk::KNOWN }), "{:?}", seen[2]);
        assert!(!seen[1].iter().any(|e| matches!(e, Event::Revived { .. })), "{:?}", seen[1]);
    }

    /// What the host's game refuses counts against the guest as the guard's refusals do.
    #[test]
    fn the_games_refusals_count_against_the_guest() {
        let (_hub, mut host, mut a, _clock) = host_with_raw_guest();
        let before = host.bad_messages(1);
        host.note_bad(1);
        host.note_bad(1);
        assert_eq!(host.bad_messages(1), before + 2);
        // A guest counts nothing.
        a.note_bad(1);
        assert_eq!(a.bad_messages(1), 0);
    }

    /// A guest's hits heard before its Hello again are not taken again after it.
    #[test]
    fn a_hits_late_copy_after_a_hello_is_not_taken_twice() {
        let (hub, mut host, a, mut clock) = host_with_raw_guest();
        let raw = hub.endpoint(addr(2));
        let shot = encode(&Msg::Hit { id: 5, by: 0, hit: hit(0x00AB_C104) });
        raw.send_to(&shot, addr(1)).unwrap();
        clock += Duration::from_millis(10);
        host.tick(clock);
        assert_eq!(host.take_events().iter().filter(|e| matches!(e, Event::Hit { .. })).count(), 1);
        let Role::Guest { token, .. } = &a.role else { panic!("a guest") };
        raw.send_to(&encode(&Msg::Hello { token: *token, lobby: 0, proof: a.admission.hello(), name: "A".into() }), addr(1)).unwrap();
        clock += Duration::from_millis(10);
        host.tick(clock);
        assert!(host.take_events().contains(&Event::RelReset { id: 1 }));
        raw.send_to(&shot, addr(1)).unwrap();
        clock += Duration::from_millis(10);
        host.tick(clock);
        assert!(!host.take_events().iter().any(|e| matches!(e, Event::Hit { .. })));
    }

    /// Ten steps of every session, with what each heard.
    fn settle(sessions: &mut [&mut Session], clock: &mut Instant, step: Duration) -> Vec<Vec<Event>> {
        let mut seen = vec![Vec::new(); sessions.len()];
        for _ in 0..10 {
            *clock += step;
            for (s, seen) in sessions.iter_mut().zip(seen.iter_mut()) {
                s.tick(*clock);
                seen.extend(s.take_events());
            }
        }
        seen
    }

    /// A guest's hit reaches the host once for its copies; the host's NPC's hit reaches
    /// its guest once, with who dealt it.
    #[test]
    fn hits_are_taken_once_each_way() {
        let (h, g) = pair(addr(1), addr(2));
        let mut host = Session::host_on(Box::new(h), Box::new(secret()));
        let mut a = Session::join_on(Box::new(g), Box::new(secret()), &[addr(1)], 0, "A", 1);
        let mut clock = Instant::now();
        let step = Duration::from_millis(10);
        run(&mut [&mut host, &mut a], &mut clock, step, |seen| welcomed(&seen[1]));
        a.send_hit(hit(0x00AB_C104));
        a.send_hit(hit(0x00AB_C105));
        host.send_hit_to(1, 0x0010_AAAA, hit(0x7));
        let mut seen = [Vec::new(), Vec::new()];
        for _ in 0..10 {
            clock += step;
            for (s, seen) in [&mut host, &mut a].into_iter().zip(seen.iter_mut()) {
                s.tick(clock);
                seen.extend(s.take_events());
            }
        }
        let hits: Vec<&Event> = seen[0].iter().filter(|e| matches!(e, Event::Hit { .. })).collect();
        assert_eq!(hits, vec![&Event::Hit { id: 1, by: 0, hit: hit(0x00AB_C104) }, &Event::Hit { id: 1, by: 0, hit: hit(0x00AB_C105) }]);
        let hurt: Vec<&Event> = seen[1].iter().filter(|e| matches!(e, Event::Hit { .. })).collect();
        assert_eq!(hurt, vec![&Event::Hit { id: 0, by: 0x0010_AAAA, hit: hit(0x7) }]);
    }

    /// A revive's reviver is its sender, whatever the message names, and its perks keep
    /// the bits this build knows.
    #[test]
    fn a_revive_is_from_its_sender() {
        let (hub, mut host, _a, mut clock) = host_with_raw_guest();
        let raw = hub.endpoint(addr(2));
        raw.send_to(&encode(&Msg::Revive { target: 0, by: 4, perks: 0xFF }), addr(1)).unwrap();
        clock += Duration::from_millis(10);
        host.tick(clock);
        assert!(host.take_events().contains(&Event::Revived { by: 1, perks: revive_perk::KNOWN }));
    }

    /// The host's world reaches its guest: the NPCs, the clock, the scene, the lines, the
    /// presence pages and what its NPCs wear; and the actor poses sent to it.
    #[test]
    fn the_hosts_world_reaches_its_guest() {
        let (h, g) = pair(addr(1), addr(2));
        let mut host = Session::host_on(Box::new(h), Box::new(secret()));
        let mut a = Session::join_on(Box::new(g), Box::new(secret()), &[addr(1)], 0, "A", 1);
        let mut clock = Instant::now();
        let step = Duration::from_millis(10);
        run(&mut [&mut host, &mut a], &mut clock, step, |seen| welcomed(&seen[1]));
        host.set_npcs((0..30).map(npc).collect());
        host.set_clock(ClockView { hour: 13.5, ..ClockView::default() });
        host.set_scene(scene());
        host.set_lines(vec![line()]);
        host.set_presence(LIST_OPEN, vec![(0x10, true)]);
        host.set_wear(vec![(0x10, vec![1, 2])]);
        host.send_actors(1, &[pose(5)]);
        let seen = run(&mut [&mut host, &mut a], &mut clock, step, |seen| {
            let s = &seen[1];
            s.iter().filter_map(|e| if let Event::Npcs(n) = e { Some(n.len()) } else { None }).sum::<usize>() >= 30
                && s.contains(&Event::Clock(ClockView { hour: 13.5, ..ClockView::default() }))
                && s.contains(&Event::Scene(scene()))
                && s.contains(&Event::Lines(vec![line()]))
                && s.iter().any(|e| matches!(e, Event::Presence { list: LIST_OPEN, refs, .. } if refs == &vec![(0x10, true)]))
                && s.contains(&Event::Wear(vec![(0x10, vec![1, 2])]))
                && s.iter().any(|e| matches!(e, Event::Actors { poses, .. } if poses == &vec![pose(5)]))
        });
        assert!(!seen[0].iter().any(|e| matches!(e, Event::Npcs(_) | Event::Clock(_))));
    }

    #[test]
    fn bad_packets_are_dropped() {
        let good = encode(&Msg::State { id: 1, seq: 1, state: state(1.0) });
        assert!(decode(&good[..good.len() - 1]).is_none(), "short");
        let mut long = good.clone();
        long.push(0);
        assert!(decode(&long).is_none(), "trailing bytes");
        let mut wrong = good.clone();
        wrong[4] = VERSION + 1;
        assert!(decode(&wrong).is_none(), "other version");
        for bad in [f32::NAN, f32::INFINITY] {
            assert!(decode(&encode(&Msg::State { id: 1, seq: 1, state: state(bad) })).is_none(), "feet not finite");
            assert!(decode(&encode(&Msg::State { id: 1, seq: 1, state: PlayerState { yaw: bad, ..state(0.0) } })).is_none(), "yaw not finite");
            assert!(decode(&encode(&Msg::State { id: 1, seq: 1, state: PlayerState { pitch: bad, ..state(0.0) } })).is_none(), "pitch not finite");
        }
        // A kind this protocol does not have does not parse.
        let mut other = encode(&Msg::Ping);
        other[5] = 99;
        assert!(decode(&other).is_none(), "an unknown kind");
    }

    /// Every list on the wire has its bound: one past it does not parse.
    #[test]
    fn lists_past_their_bounds_do_not_parse() {
        let header = |kind: u8| [&MAGIC[..], &[VERSION, kind]].concat();
        // A name one byte too long.
        let mut hello = header(1);
        hello.extend_from_slice(&[0; 20]);
        hello.push(NAME_MAX as u8 + 1);
        hello.extend_from_slice(&[b'a'; NAME_MAX + 1]);
        assert!(decode(&hello).is_none());
        // Too many names.
        let mut names = header(21);
        names.push(MAX_GUESTS as u8 + 2);
        for i in 0..MAX_GUESTS as u8 + 2 {
            names.extend_from_slice(&[i, 0]);
        }
        assert!(decode(&names).is_none());
        // A look too long.
        let mut look = header(8);
        look.push(1);
        look.extend_from_slice(&(LOOK_MAX as u16 + 1).to_le_bytes());
        look.extend_from_slice(&vec![0; LOOK_MAX + 1]);
        assert!(decode(&look).is_none());
        // Too many worn items.
        let mut s = encode(&Msg::State { id: 1, seq: 1, state: state(1.0) });
        let worn_at = 4 + 1 + 1 + 1 + 4 + 1 + 4 + 20 + 1 + 4 + 2 + 1 + 2;
        assert_eq!(s[worn_at], 0);
        s[worn_at] = MAX_WORN as u8 + 1;
        assert!(decode(&s).is_none());
        // A reliable frame of too many items, or an item too long.
        let rel = |items: Vec<Vec<u8>>| encode(&Msg::Rel { epoch: 0, frame: Frame { ack: 0, first: 0, items } });
        assert!(decode(&rel(vec![Vec::new(); REL_ITEMS_MAX])).is_some());
        assert!(decode(&rel(vec![Vec::new(); REL_ITEMS_MAX + 1])).is_none());
        assert!(decode(&rel(vec![vec![0; ITEM_MAX + 1]])).is_none());
    }

    #[test]
    fn sequence_wraps() {
        assert!(newer(1, 0));
        assert!(newer(0, u32::MAX));
        assert!(!newer(5, 5));
        assert!(!newer(4, 5));
    }

    /// Ticks every session with a shared fake clock until `done` holds (real sockets on
    /// this machine, so packets take a moment to arrive).
    fn run(sessions: &mut [&mut Session], clock: &mut Instant, step: Duration, mut done: impl FnMut(&[Vec<Event>]) -> bool) -> Vec<Vec<Event>> {
        let mut seen: Vec<Vec<Event>> = vec![Vec::new(); sessions.len()];
        for _ in 0..400 {
            *clock += step;
            for (s, seen) in sessions.iter_mut().zip(&mut seen) {
                s.tick(*clock);
                seen.extend(s.take_events());
            }
            if done(&seen) {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("never settled: {seen:?}");
    }

    fn welcomed(seen: &[Event]) -> bool {
        seen.iter().any(|e| matches!(e, Event::Welcomed(_)))
    }

    fn host_and(n: usize) -> (Session, Vec<Session>, SocketAddr) {
        let host = Session::host(0, secret()).unwrap();
        let addr: SocketAddr = format!("127.0.0.1:{}", host.local_addr().unwrap().port()).parse().unwrap();
        let guests = (0..n).map(|i| Session::join(&[addr], 0, secret(), &format!("G{i}"), new_token()).unwrap()).collect();
        (host, guests, addr)
    }

    fn addr(n: u8) -> SocketAddr {
        SocketAddr::from(([10, 0, 0, n], 47_960))
    }

    /// A host and a guest over the in-memory transport: the guest is welcomed, and each
    /// sees the other's state and name.
    #[test]
    fn a_guest_joins_over_a_transport_of_its_own() {
        let (h, g) = pair(addr(1), addr(2));
        let mut host = Session::host_on(Box::new(h), Box::new(secret()));
        let mut guest = Session::join_on(Box::new(g), Box::new(secret()), &[addr(1)], 0, "Guest", 42);
        host.set_name("Host");
        host.set_state(state(1.0));
        guest.set_state(state(2.0));
        let mut clock = Instant::now();
        let seen = run(&mut [&mut host, &mut guest], &mut clock, Duration::from_millis(10), |seen| {
            seen[0].iter().any(|e| matches!(e, Event::State { id: 1, .. })) && seen[1].iter().any(|e| matches!(e, Event::State { id: 0, .. })) && seen[1].iter().any(|e| matches!(e, Event::Named { id: 0, .. }))
        });
        assert!(seen[0].contains(&Event::Joined { id: 1, name: "Guest".into() }));
        assert!(seen[1].contains(&Event::Welcomed(1)));
        assert!(seen[1].contains(&Event::Named { id: 0, name: "Host".into() }));
        assert_eq!(guest.host_addr(), Some(addr(1)));
    }

    /// A Hello with the wrong secret, or none, is never answered.
    #[test]
    fn a_wrong_or_zero_secret_is_not_let_in() {
        struct Zero;
        impl Admission for Zero {
            fn hello(&self) -> u64 {
                0
            }
            fn admits(&self, _: u64) -> bool {
                false
            }
            fn answer(&self, _: u64, _: Option<u8>) -> u64 {
                0
            }
        }
        let hub = Hub::new();
        let mut host = Session::host_on(Box::new(hub.endpoint(addr(1))), Box::new(secret()));
        let wrong = SharedSecret::new(secret().value() ^ 1).unwrap();
        let mut a = Session::join_on(Box::new(hub.endpoint(addr(2))), Box::new(wrong), &[addr(1)], 0, "A", 1);
        let mut b = Session::join_on(Box::new(hub.endpoint(addr(3))), Box::new(Zero), &[addr(1)], 0, "B", 2);
        let mut clock = Instant::now();
        for _ in 0..200 {
            clock += Duration::from_millis(10);
            for s in [&mut host, &mut a, &mut b] {
                s.tick(clock);
            }
        }
        assert!(host.peers.is_empty());
        assert!(host.take_events().is_empty());
        assert_eq!(a.id(), None);
        assert_eq!(b.id(), None);
        assert!(host.guard_stats().dropped.iter().any(|&(k, r, n)| k == Kind::Hello && r == Reason::Secret && n > 0));
    }

    /// A Welcome that does not prove the secret (from a stranger, or a host of another
    /// lobby) is dropped; the guest stays unanswered and takes no host.
    #[test]
    fn a_forged_welcome_is_dropped() {
        let hub = Hub::new();
        let forger = hub.endpoint(addr(9));
        let mut guest = Session::join_on(Box::new(hub.endpoint(addr(2))), Box::new(secret()), &[addr(1)], 0, "A", 77);
        let other = SharedSecret::new(1).unwrap();
        let mut clock = Instant::now();
        for proof in [0, 12345, other.answer(77, Some(1)), secret().answer(78, Some(1))] {
            forger.send_to(&encode(&Msg::Welcome { id: 1, proof }), addr(2)).unwrap();
            forger.send_to(&encode(&Msg::Full { proof }), addr(2)).unwrap();
            clock += Duration::from_millis(10);
            guest.tick(clock);
        }
        assert_eq!(guest.id(), None);
        assert_eq!(guest.host_addr(), None);
        assert!(guest.take_events().is_empty());
        // The real proof is taken.
        forger.send_to(&encode(&Msg::Welcome { id: 1, proof: secret().answer(77, Some(1)) }), addr(2)).unwrap();
        guest.tick(clock + Duration::from_millis(10));
        assert_eq!(guest.id(), Some(1));
        // A known host's forged Welcome later is dropped too.
        forger.send_to(&encode(&Msg::Welcome { id: 3, proof: 5 }), addr(2)).unwrap();
        guest.tick(clock + Duration::from_millis(20));
        assert_eq!(guest.id(), Some(1));
    }

    /// One address never holds two slots: Hellos with new tokens from the same address take
    /// the one slot over, they do not fill the lobby.
    #[test]
    fn hellos_with_new_tokens_from_one_address_keep_one_slot() {
        let hub = Hub::new();
        let mut host = Session::host_on(Box::new(hub.endpoint(addr(1))), Box::new(secret()));
        let raw = hub.endpoint(addr(2));
        let mut clock = Instant::now();
        for token in 1..=(MAX_GUESTS as u64 + 3) {
            raw.send_to(&encode(&Msg::Hello { token, lobby: 0, proof: secret().hello(), name: "A".into() }), addr(1)).unwrap();
            clock += Duration::from_millis(10);
            host.tick(clock);
        }
        assert_eq!(host.peers.len(), 1);
        // Another address is another guest.
        let other = hub.endpoint(addr(3));
        other.send_to(&encode(&Msg::Hello { token: 99, lobby: 0, proof: secret().hello(), name: "B".into() }), addr(1)).unwrap();
        host.tick(clock + Duration::from_millis(10));
        assert_eq!(host.peers.len(), 2);
    }

    /// A guest's token is honoured only with the lobby's secret: a stranger holding a
    /// known token but the wrong secret does not take the guest's slot or address.
    #[test]
    fn a_known_token_with_the_wrong_secret_is_not_let_back() {
        let hub = Hub::new();
        let mut host = Session::host_on(Box::new(hub.endpoint(addr(1))), Box::new(secret()));
        let mut a = Session::join_on(Box::new(hub.endpoint(addr(2))), Box::new(secret()), &[addr(1)], 0, "A", 555);
        let mut clock = Instant::now();
        run(&mut [&mut host, &mut a], &mut clock, Duration::from_millis(10), |seen| welcomed(&seen[1]));
        let wrong = SharedSecret::new(9).unwrap();
        let mut thief = Session::join_on(Box::new(hub.endpoint(addr(3))), Box::new(wrong), &[addr(1)], 0, "A", 555);
        for _ in 0..100 {
            clock += Duration::from_millis(10);
            host.tick(clock);
            thief.tick(clock);
        }
        assert_eq!(thief.id(), None);
        assert_eq!(host.peers.len(), 1);
        assert_eq!(host.peers[0].addr, Some(addr(2)));
        assert!(!host.take_events().iter().any(|e| matches!(e, Event::RelReset { .. } | Event::Back(_))));
    }

    /// The host's checks run before it acts: a guest's host-only messages are dropped
    /// and counted against it, a teleporting state is not passed on, and the sender is
    /// known by its address, not by the id its message names.
    #[test]
    fn the_host_checks_its_guests_messages() {
        let hub = Hub::new();
        let mut host = Session::host_on(Box::new(hub.endpoint(addr(1))), Box::new(secret()));
        let mut a = Session::join_on(Box::new(hub.endpoint(addr(2))), Box::new(secret()), &[addr(1)], 0, "A", 1);
        let mut b = Session::join_on(Box::new(hub.endpoint(addr(3))), Box::new(secret()), &[addr(1)], 0, "B", 2);
        let mut clock = Instant::now();
        a.set_state(state(1.0));
        run(&mut [&mut host, &mut a, &mut b], &mut clock, Duration::from_millis(10), |seen| welcomed(&seen[1]) && welcomed(&seen[2]) && seen[2].iter().any(|e| matches!(e, Event::State { id: 1, .. })));
        // Guest 1's address now leads to this raw endpoint.
        let raw = hub.endpoint(addr(2));
        for msg in [Msg::Welcome { id: 1, proof: 0 }, Msg::Full { proof: 0 }, Msg::Who, Msg::Names { names: vec![(0, "Me".into())] }] {
            raw.send_to(&encode(&msg), addr(1)).unwrap();
        }
        raw.send_to(b"not a packet", addr(1)).unwrap();
        // A state naming guest 2, far away in the same place.
        raw.send_to(&encode(&Msg::State { id: 2, seq: 1000, state: state(90_000.0) }), addr(1)).unwrap();
        // A plausible one naming guest 2: it is guest 1's.
        raw.send_to(&encode(&Msg::State { id: 2, seq: 1001, state: state(5.0) }), addr(1)).unwrap();
        clock += Duration::from_millis(10);
        host.tick(clock);
        let events = host.take_events();
        assert!(events.contains(&Event::State { id: 1, state: state(5.0) }), "{events:?}");
        assert!(!events.iter().any(|e| matches!(e, Event::State { id, state } if *id == 2 || state.feet[0] > 1000.0)), "{events:?}");
        assert_eq!(host.bad_messages(1), 6, "{:?}", host.guard_stats());
        assert_eq!(host.bad_messages(2), 0);
        clock += Duration::from_millis(10);
        b.tick(clock);
        let at_b = b.take_events();
        assert!(!at_b.iter().any(|e| matches!(e, Event::State { state, .. } if state.feet[0] > 1000.0)), "{at_b:?}");
    }

    /// A rule a layer above adds runs on each reliable item by its kind (its first byte),
    /// after the stream delivered it and before the game has it.
    #[test]
    fn rules_judge_each_reliable_item_by_its_kind() {
        struct NoSevens;
        impl Rule for NoSevens {
            fn check(&self, _: &crate::guard::Ctx, subject: &crate::guard::Subject) -> Verdict {
                match subject {
                    crate::guard::Subject::Item(item) if item.get(1) == Some(&7) => Verdict::Drop(Reason::Rule("no sevens")),
                    _ => Verdict::Accept,
                }
            }
        }
        let (h, g) = pair(addr(1), addr(2));
        let mut host = Session::host_on(Box::new(h), Box::new(secret()));
        host.add_rule(Kind::Item(5), Box::new(NoSevens));
        let mut a = Session::join_on(Box::new(g), Box::new(secret()), &[addr(1)], 0, "A", 1);
        let mut clock = Instant::now();
        let step = Duration::from_millis(10);
        run(&mut [&mut host, &mut a], &mut clock, step, |seen| welcomed(&seen[1]));
        // The guest's stream goes once it has heard the host's.
        assert!(host.send_reliable(Some(1), vec![1]));
        run(&mut [&mut host, &mut a], &mut clock, step, |seen| seen[1].iter().any(|e| matches!(e, Event::Reliable { .. })));
        for item in [vec![5, 1], vec![5, 7], vec![6, 7], vec![5, 2]] {
            assert!(a.send_reliable(None, item));
        }
        let seen = run(&mut [&mut host, &mut a], &mut clock, step, |seen| seen[0].iter().filter(|e| matches!(e, Event::Reliable { .. })).count() == 3);
        let items: Vec<Vec<u8>> = seen[0].iter().filter_map(|e| if let Event::Reliable { item, .. } = e { Some(item.clone()) } else { None }).collect();
        assert_eq!(items, vec![vec![5, 1], vec![6, 7], vec![5, 2]]);
        assert!(host.guard_stats().dropped.contains(&(Kind::Item(5), Reason::Rule("no sevens"), 1)));
        assert_eq!(host.bad_messages(1), 1);
    }

    /// The per-address byte counts keep to [`TRAFFIC_ADDRS`] however many addresses send,
    /// keeping the guests'.
    #[test]
    fn traffic_counts_stay_bounded() {
        let hub = Hub::new();
        let mut host = Session::host_on(Box::new(hub.endpoint(addr(1))), Box::new(secret()));
        let mut a = Session::join_on(Box::new(hub.endpoint(addr(2))), Box::new(secret()), &[addr(1)], 0, "A", 1);
        let mut clock = Instant::now();
        run(&mut [&mut host, &mut a], &mut clock, Duration::from_millis(10), |seen| welcomed(&seen[1]));
        for i in 0..500u32 {
            let from = hub.endpoint(SocketAddr::from(([10, 9, (i >> 8) as u8, i as u8], 1)));
            from.send_to(b"noise", addr(1)).unwrap();
            from.send_to(&encode(&Msg::Ping), addr(1)).unwrap();
        }
        clock += Duration::from_millis(10);
        host.tick(clock);
        a.tick(clock);
        host.tick(clock);
        assert!(host.heard.len() <= TRAFFIC_ADDRS && host.sent.borrow().len() <= TRAFFIC_ADDRS);
        assert!(host.heard.contains_key(&addr(2)));
        assert!(host.traffic().iter().any(|t| t.id == 1 && t.heard > 0 && t.sent > 0));
    }

    /// Waiting events stay bounded when the game takes none: a player's state replaces
    /// the one before it.
    #[test]
    fn a_waiting_state_is_replaced_by_the_newer() {
        let (h, _) = pair(addr(1), addr(2));
        let mut s = Session::host_on(Box::new(h), Box::new(secret()));
        s.push_event(Event::Joined { id: 1, name: String::new() });
        for i in 0..10_000 {
            s.push_event(Event::State { id: 1, state: state(i as f32) });
            s.push_event(Event::Look { id: 1, look: vec![i as u8] });
        }
        assert_eq!(s.events.len(), 3);
        assert_eq!(s.events[1], Event::State { id: 1, state: state(9_999.0) });
        // After the player left and came back, a new state does not jump ahead of that.
        s.push_event(Event::Left(1));
        s.push_event(Event::State { id: 1, state: state(1.0) });
        assert_eq!(s.events.last(), Some(&Event::State { id: 1, state: state(1.0) }));
        for _ in 0..EVENTS_MAX * 2 {
            s.push_event(Event::Lost(2));
        }
        assert_eq!(s.events.len(), EVENTS_MAX);
        assert!(s.events_dropped() > 0);
    }

    /// The reliable streams: the host's items reach a guest once and in order, the
    /// guest's reach the host, and the traffic is counted.
    #[test]
    fn the_reliable_stream_reaches_its_guest() {
        let (mut host, mut guests, _) = host_and(2);
        let (a, b) = guests.split_at_mut(1);
        let (a, b) = (&mut a[0], &mut b[0]);
        let mut clock = Instant::now();
        let step = Duration::from_millis(10);
        let seen = run(&mut [&mut host, a, b], &mut clock, step, |seen| welcomed(&seen[1]) && welcomed(&seen[2]));
        assert!(seen[0].iter().filter(|e| matches!(e, Event::RelReset { .. })).count() >= 2);
        let ida = a.id().unwrap();
        for i in 0..50u8 {
            assert!(host.send_reliable(Some(ida), vec![7, i]));
        }
        assert!(host.send_reliable(None, vec![8]));
        assert!(a.send_reliable(None, vec![9, 9]));
        let seen = run(&mut [&mut host, a, b], &mut clock, step, |seen| {
            seen[1].iter().filter(|e| matches!(e, Event::Reliable { .. })).count() == 51 && seen[0].iter().any(|e| matches!(e, Event::Reliable { .. })) && seen[2].iter().any(|e| matches!(e, Event::Reliable { .. }))
        });
        let at_a: Vec<Vec<u8>> = seen[1].iter().filter_map(|e| if let Event::Reliable { id: 0, item } = e { Some(item.clone()) } else { None }).collect();
        let mut want: Vec<Vec<u8>> = (0..50u8).map(|i| vec![7, i]).collect();
        want.push(vec![8]);
        assert_eq!(at_a, want);
        assert!(seen[0].iter().any(|e| *e == Event::Reliable { id: ida, item: vec![9, 9] }));
        assert!(seen[2].iter().any(|e| *e == Event::Reliable { id: 0, item: vec![8] }));
        let t = host.traffic();
        assert!(t.iter().any(|t| t.id == ida && t.sent > 0 && t.heard > 0));
        // Everything is acknowledged in the end.
        for _ in 0..40 {
            clock += step;
            for s in [&mut host, &mut *a, &mut *b] {
                s.tick(clock);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(host.traffic().iter().all(|t| t.pending == 0), "{:?}", host.traffic());
    }

    #[test]
    fn guests_learn_every_players_name() {
        let (mut host, mut guests, _) = host_and(2);
        host.set_name("Alpha");
        let (a, b) = guests.split_at_mut(1);
        let (a, b) = (&mut a[0], &mut b[0]);
        let mut clock = Instant::now();
        let step = Duration::from_millis(20);
        let named = |v: &[Event]| v.iter().filter_map(|e| if let Event::Named { id, name } = e { Some((*id, name.clone())) } else { None }).collect::<Vec<_>>();
        let seen = run(&mut [&mut host, a, b], &mut clock, step, |seen| named(&seen[1]).len() >= 2 && named(&seen[2]).len() >= 2);
        let (ida, idb) = (a.id().unwrap(), b.id().unwrap());
        let sorted = |mut v: Vec<(u8, String)>| {
            v.sort();
            v
        };
        assert_eq!(sorted(named(&seen[1])), sorted(vec![(0, "Alpha".into()), (idb, "G1".into())]));
        assert_eq!(sorted(named(&seen[2])), sorted(vec![(0, "Alpha".into()), (ida, "G0".into())]));
        assert!(named(&seen[0]).is_empty(), "the host has the names from the guests' Hello");
    }

    #[test]
    fn a_host_answers_only_its_own_lobby() {
        let mut other = Session::host(0, secret()).unwrap();
        other.set_lobby(7);
        let mut mine = Session::host(0, secret()).unwrap();
        mine.set_lobby(9);
        let at = |s: &Session| -> SocketAddr { format!("127.0.0.1:{}", s.local_addr().unwrap().port()).parse().unwrap() };
        let mut a = Session::join(&[at(&other), at(&mine)], 9, secret(), "A", new_token()).unwrap();
        let mut clock = Instant::now();
        run(&mut [&mut other, &mut mine, &mut a], &mut clock, Duration::from_millis(10), |seen| welcomed(&seen[2]));
        assert_eq!(a.host_addr(), Some(at(&mine)));
        assert!(other.peers.is_empty());
    }

    /// A lobby's first address may lead nowhere: the guest joins through the one that
    /// answers.
    #[test]
    fn a_guest_joins_through_the_address_that_answers() {
        let mut host = Session::host(0, secret()).unwrap();
        let port = host.local_addr().unwrap().port();
        let dead: SocketAddr = "127.0.0.1:9".parse().unwrap();
        let live: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let mut a = Session::join(&[dead, live], 0, secret(), "A", new_token()).unwrap();
        let mut clock = Instant::now();
        run(&mut [&mut host, &mut a], &mut clock, Duration::from_millis(10), |seen| welcomed(&seen[1]));
        assert_eq!(a.host_addr(), Some(live));
    }

    /// Each player's look reaches the others, the host's too, once each until it changes.
    #[test]
    fn looks_reach_everyone_once() {
        let (mut host, mut g, _) = host_and(2);
        let mut b = g.pop().unwrap();
        let mut a = g.pop().unwrap();
        let mut clock = Instant::now();
        let step = Duration::from_millis(10);
        run(&mut [&mut host, &mut a, &mut b], &mut clock, step, |seen| seen[1..].iter().all(|s| welcomed(s)));
        let ida = a.id().unwrap();
        host.set_look(vec![0, 7]);
        a.set_look(vec![1, 2, 3]);
        let seen = run(&mut [&mut host, &mut a, &mut b], &mut clock, step, |seen| {
            seen[0].contains(&Event::Look { id: ida, look: vec![1, 2, 3] })
                && seen[1].contains(&Event::Look { id: 0, look: vec![0, 7] })
                && seen[2].contains(&Event::Look { id: ida, look: vec![1, 2, 3] })
                && seen[2].contains(&Event::Look { id: 0, look: vec![0, 7] })
        });
        // Sent again every LOOK_EVERY, reported once.
        let mut more = seen;
        for _ in 0..3 {
            clock += LOOK_EVERY;
            for s in [&mut host, &mut a, &mut b] {
                s.tick(clock);
            }
            std::thread::sleep(Duration::from_millis(5));
            for (s, seen) in [&mut host, &mut a, &mut b].into_iter().zip(more.iter_mut()) {
                s.tick(clock);
                seen.extend(s.take_events());
            }
        }
        let looks = more[2].iter().filter(|e| matches!(e, Event::Look { .. })).count();
        assert_eq!(looks, 2, "{:?}", more[2]);
    }

    /// A game started again counts its states from 1: they are taken, not dropped as late.
    #[test]
    fn a_restarted_count_is_fresh() {
        assert!(fresh(8, 7) && !fresh(7, 8) && !fresh(7, 7));
        assert!(fresh(1, 5000));
        assert!(fresh(0, u32::MAX));
    }

    /// A guest whose packets stop is lost, not dropped, and gets its own id back when it
    /// returns from a new address (a new socket, as after a router changed its port).
    #[test]
    fn a_quiet_guest_keeps_its_slot_and_comes_back() {
        let (mut host, mut g, addr) = host_and(1);
        let mut a = g.pop().unwrap();
        let mut clock = Instant::now();
        let step = Duration::from_millis(10);
        a.set_state(state(1.0));
        run(&mut [&mut host, &mut a], &mut clock, step, |seen| seen[0].iter().any(|e| matches!(e, Event::State { .. })));
        let id = a.id().unwrap();

        // The guest goes silent past LOST_AFTER: only the host ticks.
        clock += LOST_AFTER + Duration::from_secs(1);
        host.tick(clock);
        let events = host.take_events();
        assert!(events.contains(&Event::Lost(id)), "{events:?}");
        assert!(!events.contains(&Event::Left(id)), "dropped too soon: {events:?}");

        // It comes back on a fresh socket with the same token.
        let Role::Guest { token, .. } = a.role else { unreachable!() };
        let mut back = Session::join(&[addr], 0, secret(), "G0", token).unwrap();
        drop(a);
        back.set_state(state(2.0));
        let seen = run(&mut [&mut host, &mut back], &mut clock, step, |seen| seen[0].contains(&Event::Back(id)) && seen[0].contains(&Event::State { id, state: state(2.0) }));
        assert_eq!(back.id(), Some(id), "same slot");
        assert!(!seen[0].iter().any(|e| matches!(e, Event::Joined { .. })), "not a new guest: {:?}", seen[0]);
    }

    /// The host's game closed and started again (a new session on the same port, with
    /// the same lobby, knowing nobody) while a guest stayed in. The guest says Hello, is
    /// welcomed as the same player, and hears the new host's reliable items, and the new
    /// host hears its own.
    #[test]
    fn a_guest_back_with_a_host_started_again_hears_its_stream() {
        let (mut host, mut g, addr) = host_and(1);
        let mut a = g.pop().unwrap();
        let mut clock = Instant::now();
        let step = Duration::from_millis(10);
        run(&mut [&mut host, &mut a], &mut clock, step, |seen| welcomed(&seen[1]));
        let id = a.id().unwrap();
        // The old host's stream, well along.
        for i in 0..30u8 {
            assert!(host.send_reliable(Some(id), vec![1, i]));
        }
        run(&mut [&mut host, &mut a], &mut clock, step, |seen| seen[1].iter().filter(|e| matches!(e, Event::Reliable { .. })).count() == 30);

        // The host's game closes; another starts on the same port.
        drop(host);
        let mut host = Session::host(addr.port(), secret()).unwrap();
        clock += LOST_AFTER + Duration::from_secs(1);
        a.tick(clock);
        assert!(a.take_events().contains(&Event::HostLost));
        run(&mut [&mut host, &mut a], &mut clock, step, |seen| seen[1].contains(&Event::HostBack) && seen[0].iter().any(|e| matches!(e, Event::Joined { .. })));
        assert_eq!(a.id(), Some(id), "the same player again");

        for i in 0..5u8 {
            assert!(host.send_reliable(Some(id), vec![2, i]));
        }
        assert!(a.send_reliable(None, vec![9]));
        let seen = run(&mut [&mut host, &mut a], &mut clock, step, |seen| {
            seen[1].iter().filter(|e| matches!(e, Event::Reliable { item, .. } if item[0] == 2)).count() == 5 && seen[0].iter().any(|e| *e == Event::Reliable { id, item: vec![9] })
        });
        let items: Vec<Vec<u8>> = seen[1].iter().filter_map(|e| if let Event::Reliable { item, .. } = e { Some(item.clone()) } else { None }).collect();
        assert_eq!(items, (0..5u8).map(|i| vec![2, i]).collect::<Vec<_>>(), "the new host's items, once and in order");
        assert!(seen[1].iter().any(|e| matches!(e, Event::RelReset { id: 0 })), "the guest starts the host's stream again");
    }

    /// A guest that stops hearing the host reports it lost, keeps saying Hello, and
    /// reports it back when the host answers; past GRACE it gives up.
    #[test]
    fn a_guest_rides_out_a_quiet_host() {
        let (mut host, mut g, _) = host_and(1);
        let mut a = g.pop().unwrap();
        let mut clock = Instant::now();
        let step = Duration::from_millis(10);
        host.set_state(state(5.0));
        run(&mut [&mut host, &mut a], &mut clock, step, |seen| seen[1].iter().any(|e| matches!(e, Event::State { .. })));

        clock += LOST_AFTER + Duration::from_secs(1);
        a.tick(clock);
        let events = a.take_events();
        assert!(events.contains(&Event::HostLost) && events.contains(&Event::Lost(0)), "{events:?}");
        assert!(!events.contains(&Event::HostGone), "{events:?}");

        run(&mut [&mut host, &mut a], &mut clock, step, |seen| seen[1].contains(&Event::HostBack) && seen[1].contains(&Event::Back(0)));

        clock += GRACE + Duration::from_secs(1);
        a.tick(clock);
        assert!(a.take_events().contains(&Event::HostGone));
    }
}
