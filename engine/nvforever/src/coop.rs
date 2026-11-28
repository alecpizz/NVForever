//! Co-op, the game side of the network core (`nvf_coop`): hosting and joining a lobby from
//! the Co-op screens (`nvf_play::lobby`), this player's state sent to the others, the
//! other players drawn, and the host's world shared with its guests. `--host <port>` and
//! `--join <code>` do the same from the command line.
//!
//! **Shared progress** (`progress`, `story`; docs/design/coop-shared-progress.md): quest
//! rewards, stage offers, kill credit, saves and the host's quest, level and story go
//! between the games as `nvf_coop::progress` items on the reliable stream, checked on the
//! host by the rules it registers with its link (`host_progress_rules`) and its world (`host_forms`).
//!
//! **Off by default.** Nothing here opens a socket until the Co-op switch is on and a
//! lobby is created or joined [design]. Without a link every system here returns at once,
//! so single player is as it was.
//!
//! **The connection** ([`connect`]): the internet transport (`nvf_coop::stack`) under the
//! session, every packet sealed with Noise and each player known by its key. A host's
//! lobby gets an online code from the matchmaking service (else, LAN only or the service
//! unreachable, a code of its address, with the router asked to open the port); a guest
//! joins by either code or by an address. The host lets in a player joining by online
//! code when it is asked to (the Co-op Lobby), and can remove a guest (Kick), or keep it
//! out for the lobby's life (Ban). The host's checks of every guest message ([`HostForms`],
//! `nvf_coop::guard`) come first. The host's lobby list says how each guest is connected
//! and its ping (`Link::paths`). A load keeps the lobby ([`connect`]: the new run hosts it
//! again, with the same code).
//!
//! **The lobby browser** ([`browse`], docs/design/coop-lobby-browser.md): the host's lobby
//! may be public (listed, with a title, its place kept current) and may have a password; the
//! Join screen's Browse tab lists the public lobbies, keeps favourite hosts and reports a
//! lobby. Test aids for the online path in one game: [`test_service`].
//!
//! **The host's world.** The host runs the world: its NPCs are the only ones that think
//! (packages, fights, deaths). It sends each NPC it draws (where, facing, weapon out,
//! sneaking, aim, attacks, dead; [`share_npcs`]), and a guest's game turns its own NPCs'
//! AI off and draws them there ([`follow_npcs`], `Residents::remote`). A guest's hits on
//! NPCs are not applied in its game: they go to the host, which checks them against its
//! own world ([`checks`]) and applies them as that guest's actor's
//! (`nvf_play::combat::RemoteHit`), so the NPC reacts once, in the one world, and every
//! game sees it.
//!
//! **The other players in the host's world.** On the host each guest is also an actor of
//! the world ([`stand_ins`]): a reference created from the player's record, its AI off,
//! that the network puts where the guest is (disabled while the guest is elsewhere, lost
//! or dead; deleted when it leaves, and never saved, `fnv_game::Game::coop_actors`). The
//! world's actors take it for the player (`Game::as_player`): they detect it, hear its
//! shots ([`PuppetShot`]), attack it whenever they would attack the player, and fight it
//! back when its hits (applied as that actor's) provoke them. Their hits on it do nothing
//! on the host: they go to its guest (`CombatStats::coop_hits`), whose game takes them on
//! its own player, with its own armour.
//!
//! **A guest's own game.** As a guest is welcomed, its own game is kept as it is then
//! (`nvf_play::save::KeepSolo`, `SoloBase`), and nothing of the host's world reaches it
//! (the host's NPCs and clock) until it is: each save it makes in the host's world is its
//! own game where it left it, with the character as it is now [design].
//!
//! **What a guest does.** It picks locks and hacks terminals, and so does the host: what
//! either unlocks is unlocked for everyone (`nvf_play::lockpick::LockOpened`), once the
//! host's checks find the lock there and the guest at it ([`checks::unlock`]).
//!
//! Every game sends its player's place, feet and heading over `nvf_coop::net` (the host
//! passes them between guests), and draws the other players as NPCs the network moves:
//! engine-only puppets with a reference no load order has ([`PUPPET_REF`]), so no script,
//! AI package or physics body ever sees or fights them (the same drawing as the
//! `--npc-travel` travellers, `nav::show_actor`).
//!
//! **The host's clock.** The host sends its game clock ([`net::ClockView`]); a guest sets
//! its own to it when they differ, so the time of day, and what NPCs do by it, is the
//! same in every game.
//!
//! **The host's world on a guest.** An NPC the host draws where the guest is, the guest
//! draws there too: one the host's scripts enabled is enabled in the guest's world, and
//! one it moved is moved there ([`follow_npcs`]). An actor the host's scripts made (a
//! dynamic reference) is made in the guest's world under the host's number, of the host's
//! base, and every NPC's leveled picks are read at the host's level and rolls, so each
//! looks the same on every screen ([`follow_looks`]); the guest's own created references
//! are numbered above the host's ([`GUEST_CREATED`]). The host's world wins whatever the
//! guest's save did: an NPC dead in the guest's game but alive in the host's stands up
//! again ([`place_like_host`]), and an actor the guest's game draws near the host that the
//! host's world does not have is disabled for as long as the guest is in the host's world
//! ([`not_in_hosts_world`]). What the host's scripts enabled or disabled that is not an
//! NPC it draws goes as its world's presence: every placed reference whose own enabled
//! flag differs from its plugin's ([`presence_of`], `net::Msg::Presence`); a guest takes
//! those flags, and once it has the whole list puts back its plugin's flag on what only
//! its own game changed ([`follow_presence`]). Doors go the same way ([`share_doors`]):
//! one a player opens or closes goes to everyone at once (`net::Msg::Door`), and the
//! host's doors whose open flag differs from their plugin's go as a list, for a guest who
//! joins or arrives later. The host's script scenes go too ([`scene`]): a guest is held
//! as the host's scripts hold the host, sees the same fades and is brought beside the
//! host. None of it reaches the guest's own saves.
//!
//! **What the host's NPCs say.** The lines the host's NPCs are saying (`Game::speech`),
//! and the NPC's lines of the host's own conversation ([`conversation_line`]), go to the
//! guests ([`net::LineView`]), whose games say them as the host's does, voice and
//! subtitles, timed by the host and without their scripts (`Game::set_remote_speech`).
//!
//! **What they do.** Each state carries the weapon in hand, whether it is out, aimed or
//! down the sights, sneaking, in the air, what is worn, and counters of the attacks and
//! reloads begun: the puppet draws and holsters, crouches, aims (as a fighter does,
//! `npc_combat::Fighters::set_puppet`) and plays one attack or reload for each the counter
//! went up, so a lost packet loses none. A shot is heard where it is fired (the gun's
//! sound, near or distant, as an NPC's: [`shot_sound`]). A puppet's head is left to its
//! animation: it does not turn to look at the player.
//!
//! **How they look.** Each game sends its player's look from the character creator
//! (race, sex, hair, eyes, face; [`encode_look`]); the others draw that player from the
//! player's own record with it (`ActorWorld::looks`), as this game draws its own body.
//!
//! **Down, not dead.** A player whose health runs out in co-op is down ([`downed`]): the
//! body drops as a ragdoll (here and on the others' screens) and the view stays with it,
//! without the death camera's slow motion (`death_cam::DeathCam::downed_in_coop`) but
//! fading to black, where the downed screen says how to respawn or that someone revives
//! (`nvf_play::coop_downed`). Another player who aims at the body where it lies on their
//! screen gets "Revive" over the name (`nvf_play::coop_mates`), and pressing Activate
//! revives them with a Stimpak of their own: the downed player, healed
//! (`Game::resurrect`), gets up from where the body lies, the camera following the body
//! (`player_knockdown.rs`); on the others' screens its puppet plays the get-up clip from
//! its own ragdoll's pose ([`revive`]). After [`RESPAWN_AFTER`] a downed player (the host
//! too, alone in the lobby or not) can press Activate to stand up again beside another
//! player up, else where it lies.
//!
//! **Dropouts.** A player whose link goes quiet stays where it was last seen, frozen,
//! while the link reconnects; only a player who quit or stayed away past the grace is
//! removed.
//!
//! Log: `co-op: hosting on UDP port <p> ...`, `co-op: lobby code <code> ...` and the rest of
//! [`connect`]'s, `co-op: welcomed as player <n>`, `co-op: <name> joined`, `co-op:
//! player <n> left`, `co-op: player <n> at <place> feet (x, y, z) yaw <deg>` (the first
//! state from each player, then one every [`STATE_LOG_EVERY`] seconds), `co-op: not
//! joined: <why>` (what the join screen says), `co-op: turned away <n> Hello(s) without
//! the lobby's secret` (the host, at most once a second), `co-op check: <reason>` (a
//! guest's message the host refused).

use crate::actors::ActorWorld;
use crate::attack_anims::{Move, PlayAttack, Power, SwingPick};
use crate::locomotion::{Dying, Locomotion};
use crate::nav::{show_actor, Who};
use crate::npc_combat::{Fighters, PuppetPose};
use crate::outfits::{Equipment, PuppetEquipment};
use crate::residents::{RemotePose, Residents};
use crate::weapons::WeaponHold;
use crate::world::{Streamer, WorldData, WorldRoot};
use bevy::prelude::*;
use fnv_util::words::{tr, trf};
use fnv_actor::weapon::AnimType;
use fnv_game::chargen::Look;
use fnv_sim::FormId;
use nvf_assets::actor::ActorParts;
use nvf_assets::model::ModelCtx;
use nvf_coop::guard::FormCheck;
use nvf_coop::net::{self, attack, flag, npc_flag, Event, HitView, LimbView, Link, NpcState, Place, PlayerState};
use nvf_play::combat::{ActorHit, CombatStats, RelayedHit, RemoteHit};
use nvf_play::coop_mates::{Mate, SocialMates};
use nvf_play::loading::{Loading, TravelRequest};
use nvf_play::lobby::{Browse, CoopSwitch, HostOptions, Hosting, Joining, LobbyRequest, LobbyState};
use nvf_play::lockpick::{LockOpened, OpenedBy};
use nvf_play::player::Player;
use nvf_play::sfx::{PlaySound, Sfx};
use nvf_play::{Location, PlayState, RefVisual, Travel};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

mod browse;
mod connect;
pub(crate) mod net_settings;
mod test_service;
mod areas;
pub(crate) mod checks;
mod far_combat;
mod leave;
mod progress;
mod rejoin;
mod replication;
mod revive;
mod scene;
mod story;
mod syncing;

pub use connect::set_load_order;
pub use leave::HoldLeave;

/// How far behind the newest state the others are drawn, so there are two to blend.
const DELAY: f32 = 0.1;
/// After a leash trip, the next waits this long (the trip's loading screen comes first).
const LEASH_EVERY: f32 = 5.0;
/// The puppets' references: plugin index FE, which no FNV load order uses.
const PUPPET_REF: u32 = 0xFE00_C000;
/// The record a player is drawn from: the player's own, as this game draws its own body
/// (`player_body.rs`).
const PLAYER_BASE: u32 = FormId::PLAYER_BASE.0;
/// A guest takes the host's NPCs as being where it is only this long after the host
/// came there (seconds): before, they may be the place it left.
const HOST_SETTLE: f32 = 1.0;
/// Where a shot is heard from, above the shooter's feet (about the gun's height).
const GUN_HEIGHT: f32 = 100.0;
/// A guest takes the host's clock when they differ by more than this (game hours: three
/// game minutes, six real seconds at TimeScale 30) [design].
const CLOCK_SLACK: f32 = 0.05;
/// A downed player can respawn after this long (seconds) [design].
const RESPAWN_AFTER: f32 = 60.0;
/// Env var (a test aid): `1` starts a session with Friendly Fire on.
pub const FRIENDLY_FIRE_ENV: &str = "NVF_COOP_FRIENDLY_FIRE";
/// Another player up within this distance (same place) can come and revive a downed one
/// [design].
const ALONE_NEAR: f32 = 8192.0;
/// A puppet holds its aim this long after an attack, aimed or not (seconds).
const AIM_AFTER_ATTACK: f32 = 1.5;
/// Env var: a guest's game a load restarts goes to its host again (`rejoin`).
pub const REJOIN_ENV: &str = "NVF_COOP_REJOIN_HOST";
/// Env var: a guest's game a load restarts shows the syncing screen until the host's
/// world is in (`syncing`).
pub const SYNC_ENV: &str = "NVF_COOP_SYNC_SCREEN";

/// The port a lobby opens on, UDP [design] (`nvf_coop::code::DEFAULT_PORT`).
const DEFAULT_PORT: u16 = nvf_coop::code::DEFAULT_PORT;
/// A guest looking for its host gives up after this long without an answer (seconds).
const FIND_FOR: f32 = 15.0;
/// Another player's state is logged when first heard, then once this often (seconds).
const STATE_LOG_EVERY: f32 = 10.0;
/// The host looks at the turned-away Hellos this often (seconds).
const DROPS_EVERY: f32 = 1.0;

/// What a lobby is asked for at start (the internet transport on top adds the command-line flags).
#[derive(Clone)]
pub struct Config {
    /// Host on this UDP port.
    pub host: Option<u16>,
    /// Join the lobby this code names.
    pub join: Option<String>,
    /// The name the other players see.
    pub name: String,
}

pub struct CoopPlugin(pub Config);

#[derive(Resource)]
pub(crate) struct Coop {
    /// None while no lobby is open or joined.
    link: Option<Link>,
    name: String,
    peers: BTreeMap<u8, Peer>,
    /// The host's newest state (guests only).
    host: Option<PlayerState>,
    host_lost: bool,
    /// When the last leash trip was asked for (seconds of `Time`).
    last_leash: Option<f32>,
    /// The host's code, once hosting.
    code: Option<String>,
    note: Option<String>,
    /// The transport's control ([`connect`]): the service's events, join answers.
    control: Option<nvf_coop::stack::Control>,
    /// The host's port, and whether its router opened it for an address code.
    port: u16,
    opened: bool,
    /// The router's answer on the way (an address code), and an online code asked for.
    reaching: Option<std::sync::Mutex<std::sync::mpsc::Receiver<nvf_coop::reach::Reach>>>,
    online_pending: bool,
    /// The host's: players joining by online code waiting for its answer (their number in
    /// the service's lobby, name).
    asking: Vec<(u32, String)>,
    /// A guest's: the host was asked to let it in, and has not answered.
    awaiting_host: bool,
    /// The host's online lobby's secret (it takes the code back after a load's restart),
    /// and whether this run handed the lobby to the run a load starts ([`connect::hand_over`]).
    secret: Option<String>,
    handed_off: bool,
    /// The lobby browser ([`browse`]): the host's choices, the password typed to join, the
    /// Browse tab's request, its list and when it last asked, a report on its way (and its
    /// code) and the last one's answer, the favourite hosts, the host's place for its
    /// listing (and when it was looked up) and the listing last sent.
    host_options: HostOptions,
    join_password: String,
    browse_job: Option<browse::BrowseJob>,
    browse: Browse,
    browse_at: f32,
    report_job: Option<browse::ReportJob>,
    report_code: String,
    report_note: Option<(String, String)>,
    favourites: Vec<String>,
    place: String,
    place_at: f32,
    listing_sent: Option<(bool, String, String, bool)>,
    joining: Joining,
    finding_since: Option<f32>,
    /// The host's checks were given the load order and the world's rules.
    forms_set: bool,
    /// The host's checks on shared progress are on its link ([`host_progress_rules`]).
    rules_set: bool,
    /// When each player's state was last logged.
    state_logged: BTreeMap<u8, f32>,
    /// The host's Hellos turned away for the secret, as of the last look, and when.
    hello_drops: (u64, f32),
    /// A guest's: the host's NPCs as heard, by reference.
    npcs: HashMap<u32, NpcTrack>,
    /// A host's: the attacks each of its NPCs began, counted (wrapping).
    npc_attacks: HashMap<u32, u8>,
    /// A guest following the host's world: whether this game's actors' AI was off before.
    ai_was_off: Option<bool>,
    /// A joining guest: its own world logic is held until it is flagged a guest
    /// ([`hold_joining_world`]).
    joining_held: bool,
    /// A guest's: when (seconds of play) the host's clock was last heard, for
    /// [`guest_hours_passed`].
    clock_heard: Option<f32>,
    /// A guest's: the host's place, and since when it has been there (seconds of `Time`).
    host_place: Option<(Place, f32)>,
    /// A guest's: welcomed by this host and not yet taken beside it (`rejoin`); the
    /// syncing screen holds its ask meanwhile (`syncing`).
    join_trip: bool,
    /// This player is down: since when (seconds of `Time`), and whether it was told it
    /// can respawn.
    downed_at: Option<f32>,
    told_respawn: bool,
    /// Down with no other player up near ([`ALONE_NEAR`]).
    alone_down: bool,
    /// The lobby's Friendly Fire rule: set by the host, sent to the guests
    /// (`replication`); off, players' hits on each other (and their companions') are
    /// dropped by the host.
    pub(crate) friendly_fire: bool,
    /// The downed player this one was last told it can revive.
    revive_offer: Option<u8>,
    /// Revived by this player (name), shown on the black until it lifts.
    revived_by: Option<String>,
    /// References whose every body in the world is taken away on the next frame ([`draw`]:
    /// players who left), and when the bodies were last checked for strays.
    strays: Vec<u32>,
    swept_at: f32,
    /// A guest's: its own actors near the host that the host's world does not have, since
    /// when each has not been heard of, and those it disabled ([`not_in_hosts_world`]).
    unheard: HashMap<u32, f32>,
    hidden: HashSet<FormId>,
    /// A guest's: references the host's scripts disabled (and have not enabled since)
    /// ([`place_like_host`]).
    host_disabled: HashSet<u32>,
    /// A guest's: the host's world's presence as heard ([`follow_presence`]), and the
    /// enabled flags it changed in this world, as they were (put back on leaving).
    presence: HostPresence,
    presence_was: HashMap<FormId, bool>,
    /// A host's: its world's presence serial when the list was last made, and when.
    presence_made: Option<(u64, f32)>,
    /// Doors ([`share_doors`]): each door's open flag as last seen here, the doors this
    /// player used lately (and when), the doors others opened or closed (who, the door,
    /// open) not yet applied; a guest's: the host's doors' list as heard and the flags it
    /// changed here, as they were; a host's: when its list was last made.
    doors_seen: HashMap<u32, bool>,
    door_used: HashMap<u32, f32>,
    doors_in: Vec<(u8, u32, bool)>,
    doors_heard: HostPresence,
    doors_was: HashMap<FormId, bool>,
    doors_made: Option<f32>,
    /// A guest's: the host's level, all-levels flag and rolls taken for each NPC
    /// ([`follow_looks`]), and whether its own created references are numbered from
    /// [`GUEST_CREATED`] yet.
    looks_taken: HashMap<u32, (u16, bool, u8)>,
    numbered: bool,
    /// A guest: what the host's game draws each of its NPCs wearing and holding
    /// (`net::Msg::Wear`), and what this game last dressed each in ([`follow_looks`]).
    wear_heard: HashMap<u32, Vec<u32>>,
    wear_taken: HashMap<u32, Vec<u32>>,
    /// A guest's: the lines the host's NPCs are saying, and when they came.
    host_lines: Option<(f32, Vec<net::LineView>)>,
    /// Replication (`replication`), and its newest perf line for the corner readout.
    repl: replication::Repl,
    areas: areas::Areas,
    /// The host's fights of its undrawn actors with guests away from it.
    far: far_combat::FarFights,
    /// The host's: the NPCs it sent last round, and since when each has not been drawn
    /// ([`share_npcs`]: sent on a while from where its AI has it).
    sent_npcs: HashMap<u32, Option<f32>>,
    repl_hud: Option<String>,
    /// A guest's: the host ended the lobby (told, or gone too long), not yet acted on
    /// ([`leave::guest_leaves`]).
    ended: bool,
    /// A guest's way out while it autosaves ([`leave`]).
    leaving: leave::Leaving,
    /// A guest's way out is done (autosaved, the link stopped) and the main menu is on its
    /// way: this game still stands in the host's world until then ([`follow_npcs`]).
    parting: bool,
    /// The host's script scenes ([`scene::share_scene`]).
    scene: scene::Shared,
    /// The host's checks of what guests send against its world ([`checks`]): what each
    /// refused, counted, and the guests' drops and forced resends it keeps count of.
    checks: checks::HostChecks,
    /// Shared progress's state on this side (`progress`).
    progress: progress::Shared,
    /// The host's story for the guests (`story`).
    story: story::Shared,
}

impl Coop {
    fn new(name: String) -> Coop {
        Coop {
            link: None,
            name,
            peers: BTreeMap::new(),
            host: None,
            host_lost: false,
            last_leash: None,
            code: None,
            note: None,
            control: None,
            port: DEFAULT_PORT,
            opened: false,
            reaching: None,
            online_pending: false,
            asking: Vec::new(),
            awaiting_host: false,
            secret: None,
            handed_off: false,
            host_options: HostOptions::default(),
            join_password: String::new(),
            browse_job: None,
            browse: Browse::Idle,
            browse_at: f32::NEG_INFINITY,
            report_job: None,
            report_code: String::new(),
            report_note: None,
            favourites: Vec::new(),
            place: String::new(),
            place_at: f32::NEG_INFINITY,
            listing_sent: None,
            joining: Joining::Idle,
            finding_since: None,
            forms_set: false,
            rules_set: false,
            state_logged: BTreeMap::new(),
            hello_drops: (0, 0.0),
            npcs: HashMap::new(),
            npc_attacks: HashMap::new(),
            ai_was_off: None,
            joining_held: false,
            clock_heard: None,
            host_place: None,
            join_trip: false,
            downed_at: None,
            told_respawn: false,
            alone_down: false,
            friendly_fire: std::env::var(FRIENDLY_FIRE_ENV).is_ok_and(|v| v == "1"),
            revive_offer: None,
            revived_by: None,
            strays: Vec::new(),
            swept_at: 0.0,
            unheard: HashMap::new(),
            hidden: HashSet::new(),
            host_disabled: HashSet::new(),
            presence: HostPresence::default(),
            presence_was: HashMap::new(),
            presence_made: None,
            doors_seen: HashMap::new(),
            door_used: HashMap::new(),
            doors_in: Vec::new(),
            doors_heard: HostPresence::default(),
            doors_was: HashMap::new(),
            doors_made: None,
            looks_taken: HashMap::new(),
            numbered: false,
            wear_heard: HashMap::new(),
            wear_taken: HashMap::new(),
            host_lines: None,
            repl: Default::default(),
            areas: Default::default(),
            far: Default::default(),
            sent_npcs: HashMap::new(),
            repl_hud: None,
            ended: false,
            leaving: Default::default(),
            parting: false,
            scene: Default::default(),
            checks: Default::default(),
            progress: Default::default(),
            story: Default::default(),
        }
    }
}

impl Plugin for CoopPlugin {
    fn build(&self, app: &mut App) {
        net_settings::register(app);
        test_service::start_local_service();
        let c = &self.0;
        let mut coop = Coop::new(c.name.clone());
        coop.favourites = browse::load_favourites();
        // A lobby a load restarted, or a scripted run's, takes its choices from the
        // environment (the lobby screen sets them otherwise).
        coop.host_options = connect::options_from_env();
        coop.join_password = coop.host_options.password.clone();
        // The main menu's run after a lobby ended neither hosts nor joins, whatever the
        // command line it was started with says (`leave`).
        let ended = std::env::var_os(leave::NO_LOBBY_ENV).is_some();
        std::env::remove_var(leave::NO_LOBBY_ENV);
        if ended {
            info!("co-op: the lobby ended in the run before: not hosting or joining");
        } else if let Some(port) = c.host {
            coop.start_hosting(port, None);
        } else if let Some((port, again)) = connect::handed_over() {
            info!("co-op: hosting the lobby on UDP port {port} again (a load restarted the game, or {} is set)", connect::HOST_ENV);
            coop.start_hosting(port, again);
        } else if let Some(join) = c.join.as_deref() {
            coop.start_joining(join);
        }
        // The command line's lobby turns the switch on for this run (not kept).
        let on = coop.link.is_some();
        app.insert_resource(coop)
            .init_resource::<HoldLeave>()
            .add_message::<PuppetShot>()
            .add_plugins(nvf_play::coop_progress::CoopProgressPlugin)
            .add_systems(
                PostStartup,
                (move |mut s: ResMut<CoopSwitch>| {
                    if on && !s.on {
                        s.on = true;
                    }
                })
                .after(nvf_play::lobby::load_switch),
            )
            .add_systems(
                Update,
                (
                    host_progress_rules,
                    host_forms,
                    replication::frame_clock,
                    connect::poll,
                    lobby,
                    send,
                    take_events,
                    share_doors,
                    social_mates,
                    share_npcs,
                    areas::coop_areas,
                    replication::replicate,
                    follow_looks,
                    follow_npcs,
                    (leash, rejoin::rejoin_host, syncing::syncing).chain(),
                    scene::share_scene,
                    downed,
                    revive::bodies,
                    replication::hud,
                )
                    .chain()
                    .after(nvf_play::PlaySet::Physics),
            )
            .add_systems(Update, hold_joining_world.before(nvf_play::PlaySet::Scripts))
            .add_systems(Update, (track_place, hand_over_on_load))
            .add_systems(Update, (replication::dismember_out.before(replication::replicate), replication::dismember_in.after(replication::replicate)))
            .add_systems(Update, (replication::corpses_out.before(replication::replicate), replication::corpses_in.after(replication::replicate)))
            .add_systems(Update, far_combat::far_fights.after(areas::coop_areas).before(replication::replicate))
            // On the slide marker before the camera is placed, so the frame shows its view.
            .add_systems(Update, scene::slide_spot.in_set(nvf_play::PlaySet::Physics).before(nvf_play::control::place_camera))
            .add_systems(Update, (leave::guest_leaves.before(crate::handoff::leave), leave::tell_ended))
            .add_systems(Last, leave::host_ends)
            .add_systems(Update, (stand_ins, draw, scene::hide_players_in_scene, swimmers).chain().after(nvf_play::PlaySet::Sync).before(crate::locomotion::walk_test))
            .add_systems(Update, (progress::progress, story::share_story).chain().after(take_events));
    }
}

/// On a co-op puppet's root: the systems that would drive an NPC from the simulation
/// (drawing its weapon in alarm, sneaking) leave it to the network.
#[derive(Component)]
pub struct Puppet;

/// On a guest's NPC that the host draws: as [`Puppet`], the systems that would drive it
/// from this game's simulation leave it to the network.
#[derive(Component)]
pub struct Remote;

/// Another player fired a shot (its puppet's attack with a gun), for the world's actors
/// to hear as they hear the player's (`detection`).
#[derive(Message, Clone, Copy, Debug)]
pub struct PuppetShot {
    pub shooter: FormId,
    pub weapon: FormId,
}

/// How far behind the host's newest NPC states a guest draws them: two rounds.
const NPC_DELAY: f32 = 2.0 / net::NPC_HZ as f32;
/// A guest stops following an NPC the host has not drawn for this long (seconds); it
/// stays where it was last drawn.
const NPC_FORGET: f32 = 3.0;
/// Between two of the host's states of an NPC, a move longer than this (units) and
/// faster than it runs (at most [`NPC_JUMP_SPEED`], units/s) is a script's move, put there at once, not
/// walked [design: no actor of the game runs near 2000 units/s; a sprinting player is
/// a few hundred].
const NPC_JUMP: f32 = 64.0;
const NPC_JUMP_SPEED: f32 = 2000.0;
/// How much faster than its run speed an NPC may move between the host's states before
/// it counts as a jump [design: a run's speed-ups and the states' jitter; the fastest
/// runner seen, a cazador, runs 955 units/s, observed: creature pace logs].
const NPC_RUN_ROOM: f32 = 1.5;
/// States further apart in time than this (seconds) with a move of more than
/// [`NPC_JUMP`] between them: the host did not send it meanwhile (faded out, disabled for
/// the move), so it is put there too. Two rounds of the slowest rate the host sends an
/// actor's pose at (3 Hz, far from the guest: `repl::pose_hz`): at three rounds of
/// [`net::NPC_HZ`] (0.35 s) every pose of a far actor running came after a "gap", and a
/// guest held a far group still and put them 131-166 units on about every 0.36 s
/// [observed: buckallobill, in game, FNV 1.4.0.525].
const NPC_GAP: f32 = 2.0 / SLOWEST_POSE_HZ;
/// How long (seconds) a guest carries a moving NPC on past its newest state, waiting for
/// the next: one round of the slowest pose rate [design].
const NPC_CARRY_ON: f32 = 1.0 / SLOWEST_POSE_HZ;
/// The slowest rate (Hz) the host sends an actor's pose to a guest at (`repl::pose_hz`).
const SLOWEST_POSE_HZ: f32 = 3.0;
/// How many of its own rounds behind its newest state a guest draws an NPC whose states
/// come further apart than [`NPC_DELAY`] allows for, up to [`NPC_DELAY_MAX`] seconds: drawn
/// between two states, not held at the newest until the next comes [design].
const NPC_DELAY_ROUNDS: f32 = 1.5;
const NPC_DELAY_MAX: f32 = 0.6;
/// The time (seconds) a guest takes to go over to another delay when an NPC's states come
/// faster or slower (it comes nearer or into view): the drawn copy runs a little slower
/// or faster meanwhile instead of jumping back or on [design].
const NPC_DELAY_EASE: f32 = 1.0;
/// The least time between two of an NPC's states on a guest, in rounds of
/// [`net::NPC_HZ`] [design: under one round, so stamps on time do not drift later than
/// they came; [`NPC_DELAY`] is two rounds, so a spaced state is still blended to].
const NPC_SPACING: f32 = 0.75;
/// The host goes on sending an NPC it stopped drawing this long (seconds), from where its
/// AI has it, longer than a guest's [`NPC_FORGET`] would keep it ([`share_npcs`]).
const NPC_LINGER: f32 = 5.0;
/// For this long after a guest first hears an NPC (seconds), a state with no furniture
/// does not take it off the furniture it is on here (a host after a load
/// sends an actor it draws as off for the frame before it seats it) [design].
const NPC_FIRST_HOLD: f32 = 2.0;

/// The furniture a guest has one of the host's NPCs on: the host's (`heard`: furniture
/// and on it), or, when the host says none within [`NPC_FIRST_HOLD`] of its first state
/// (`young`), the one it is on here (`here`) [design].
fn furniture_here(heard: Option<(u32, bool)>, young: bool, here: Option<u32>) -> Option<(u32, bool)> {
    heard.or_else(|| here.filter(|_| young).map(|f| (f, true)))
}

/// The furniture the host sends for an NPC it draws: the one it is on (`on`: furniture,
/// seated, getting off), none while it gets off, else the one it is being put on at
/// once (`putting_on`, [`Residents::being_put_on`]) as on it.
fn sent_furniture(on: Option<(u32, bool, bool)>, putting_on: Option<u32>) -> (u32, bool) {
    match on {
        Some((_, _, true)) => (0, false),
        Some((f, seated, false)) => (f, seated),
        None => putting_on.map_or((0, false), |f| (f, true)),
    }
}
/// How often every body of the other players is checked for strays (seconds, [`draw`]).
const SWEEP_EVERY: f32 = 1.0;
/// A guest's own actor this near the host (game units, across the floor), which the
/// host's NPCs have not included for [`HIDE_AFTER`] seconds (and the host has been in
/// the place that long), is not in the host's world there ([`not_in_hosts_world`]).
/// Well inside the cells the host draws around it.
const HIDE_REACH: f32 = 3000.0;
const HIDE_AFTER: f32 = 3.0;
/// A guest's own references created at run time are numbered from here, above the
/// host's, which it makes under the host's numbers ([`follow_looks`]).
const GUEST_CREATED: u32 = 0xFF80_0000;

/// One of the host's NPCs as a guest has heard it.
#[derive(Default)]
struct NpcTrack {
    samples: VecDeque<(f32, NpcState)>,
    /// How far behind its newest state it is drawn (seconds; 0 until first drawn):
    /// [`eased_delay`].
    delay: f32,
    /// When its first state was heard (seconds of `Time`): a new host's first states
    /// do not take it off its furniture here ([`furniture_here`]).
    first_heard: Option<f32>,
    /// The attack counter played up to (None until the first state).
    played: Option<u8>,
    attacked_at: Option<f32>,
}

/// The host's NPCs whose every state heard and pose drawn a guest logs
/// (`NVF_COOP_NPC_TRACE`: hex references, comma-separated), to see how the states of one
/// come in (the Dead Horses put about 150 units on every 0.36 s on a guest).
fn traced(r: u32) -> bool {
    static LIST: std::sync::OnceLock<Vec<u32>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| std::env::var("NVF_COOP_NPC_TRACE").map(|v| v.split(',').filter_map(|s| u32::from_str_radix(s.trim(), 16).ok()).collect()).unwrap_or_default())
        .contains(&r)
}

/// How far behind its newest state a guest draws an NPC this frame (seconds): from
/// `delay`, eased over [`NPC_DELAY_EASE`] towards [`NPC_DELAY_ROUNDS`] of the time between
/// its states (`spacing`), at least [`NPC_DELAY`] and at most [`NPC_DELAY_MAX`].
fn eased_delay(delay: f32, spacing: f32, dt: f32) -> f32 {
    let target = (spacing * NPC_DELAY_ROUNDS).clamp(NPC_DELAY, NPC_DELAY_MAX);
    if delay <= 0.0 {
        return target;
    }
    delay + (target - delay) * (dt / NPC_DELAY_EASE).clamp(0.0, 1.0)
}

impl NpcTrack {
    /// The time between its states as they came (seconds; [`NPC_DELAY`]'s round when
    /// fewer than two).
    fn spacing(&self) -> f32 {
        match (self.samples.front(), self.samples.back()) {
            (Some((a, _)), Some((b, _))) if self.samples.len() > 1 => (b - a) / (self.samples.len() - 1) as f32,
            _ => 1.0 / net::NPC_HZ as f32,
        }
    }

    /// A state heard at `now`. States heard in one frame (one late, the next on time) are
    /// spaced [`NPC_SPACING`] of a round apart, not given the same time: blended over no
    /// time, the second was put on at once, and a runner at about 516 units/s on a guest
    /// was put 48-56 units on in one frame again and again [observed: buckallobill, in game, FNV 1.4.0.525].
    fn heard(&mut self, now: f32, n: NpcState) {
        self.first_heard.get_or_insert(now);
        let at = self.samples.back().map_or(now, |&(last, _)| now.max(last + NPC_SPACING / net::NPC_HZ as f32));
        self.samples.push_back((at, n));
        while self.samples.len() > 8 {
            self.samples.pop_front();
        }
    }

    /// Where to draw it at `t`: feet and heading blended between the states either side.
    /// `top`: the fastest it moves walking or running (units/s); a move faster than that
    /// (and longer than [`NPC_JUMP`]) is a jump.
    /// Past its newest state while it was moving: carried on as it went between its last
    /// two states, for at most [`NPC_CARRY_ON`] seconds, rather than held there until the
    /// next comes. When its states come further apart all at once (from 10 Hz to 3 Hz as
    /// it goes far from the guest), its delay is still short and the newest state is
    /// passed: a guest held an NPC and put it 22-65 units on about every 0.35 s for a
    /// second or two [observed: buckallobill, in game, FNV 1.4.0.525].
    fn carried_on(&self, t: f32, top: f32) -> Option<([f32; 3], f32, bool)> {
        let n = self.samples.len();
        let ((ta, a), (tb, b)) = (*self.samples.get(n.checked_sub(2)?)?, *self.samples.back()?);
        let span = tb - ta;
        let gap = (0..3).map(|k| (b.feet[k] - a.feet[k]).powi(2)).sum::<f32>().sqrt();
        if !b.has(npc_flag::MOVING) || a.space != b.space || span <= 1e-3 || span > NPC_GAP || gap / span > top {
            return None;
        }
        let k = (t - tb).clamp(0.0, NPC_CARRY_ON) / span;
        Some((std::array::from_fn(|i| b.feet[i] + (b.feet[i] - a.feet[i]) * k), b.heading, true))
    }

    fn pose(&self, t: f32, top: f32) -> Option<([f32; 3], f32, bool)> {
        let last = self.samples.back()?.1;
        let (a, b, f) = match self.samples.iter().position(|(at, _)| *at > t) {
            None => return Some(self.carried_on(t, top).unwrap_or((last.feet, last.heading, false))),
            Some(0) => return Some((self.samples[0].1.feet, self.samples[0].1.heading, false)),
            Some(i) => {
                let (ta, a) = self.samples[i - 1];
                let (tb, b) = self.samples[i];
                // Put there, not walked there: a script's move between two states
                // (MoveTo, MoveToMarkerWithFade) or a trip into another space is drawn
                // where it ends once its time comes. Blended, Kimball's fade move at the
                // Hoover Dam slid across on a guest at 1069 units/s.
                let gap = (0..3).map(|k| (b.feet[k] - a.feet[k]).powi(2)).sum::<f32>().sqrt();
                if a.space != b.space || (gap > NPC_JUMP && (gap / (tb - ta).max(1e-4) > top || tb - ta > NPC_GAP)) {
                    return Some(if t < tb { (a.feet, a.heading, false) } else { (b.feet, b.heading, false) });
                }
                (a, b, ((t - ta) / (tb - ta).max(1e-4)).clamp(0.0, 1.0))
            }
        };
        let feet = std::array::from_fn(|k| a.feet[k] + (b.feet[k] - a.feet[k]) * f);
        let turn = (b.heading - a.heading + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        Some((feet, a.heading + turn * f, b.has(npc_flag::MOVING)))
    }
}

impl Coop {
    fn is_host(&self) -> bool {
        self.link.as_ref().is_some_and(Link::is_host)
    }

    pub(crate) fn is_guest(&self) -> bool {
        self.link.as_ref().is_some_and(|l| !l.is_host())
    }

    /// Open a lobby on `port` ([`connect::host_link`]) with the host's choices: an online
    /// code is asked for, else the router, for an address code. `again`: the online code and
    /// secret of the lobby a load restarted, taken back; the address code of one is kept.
    fn start_hosting(&mut self, port: u16, again: Option<(String, String)>) {
        match connect::host_link(port, &self.name, &self.host_options, &self.place, self.friendly_fire, again.as_ref()) {
            Ok((link, control)) => {
                self.link = Some(link);
                self.port = port;
                self.listing_sent = None;
                let online = nvf_coop::signal_io::service().is_some();
                // The address code a load's restart carried (the router's port is still open).
                let kept = std::env::var(connect::CODE_ENV).ok().and_then(|c| Some((nvf_coop::code::Lobby::decode(&c)?, c))).filter(|(l, _)| l.port == port && !online);
                match (again, kept) {
                    (Some((code, secret)), _) if online => {
                        self.code = Some(code);
                        self.secret = Some(secret);
                        self.online_pending = false;
                    }
                    (_, Some((lobby, code))) => {
                        if let Some(l) = &self.link {
                            l.set_lobby(lobby.key());
                        }
                        info!("co-op: lobby code {code} again (a load restarted the game)");
                        self.code = Some(code);
                        self.opened = std::env::var_os(connect::OPENED_ENV).is_some();
                    }
                    _ => {
                        self.online_pending = online;
                        if !online {
                            self.reaching = Some(connect::reach_out(port));
                        }
                    }
                }
                self.control = Some(control);
            }
            Err(why) => {
                warn!("co-op: {why}");
                self.note = Some(why);
            }
        }
    }

    fn stop_hosting(&mut self) {
        // Dropping the link says goodbye to the guests.
        self.link = None;
        self.control = None;
        if self.opened {
            connect::close_router(self.port);
        }
        self.opened = false;
        self.reaching = None;
        self.online_pending = false;
        self.asking.clear();
        self.forms_set = false;
        self.rules_set = false;
        self.hello_drops = (0, 0.0);
        self.state_logged.clear();
        self.code = None;
        self.note = None;
        self.secret = None;
        self.listing_sent = None;
        connect::forget_lobby();
        self.forget_peers();
        self.progress.reset();
        self.story = Default::default();
    }

    /// Join the lobby `at` names ([`connect::guest_link`]), with the password typed.
    fn start_joining(&mut self, at: &str) {
        match connect::guest_link(at, &self.name, &self.join_password) {
            Ok((link, control)) => {
                self.link = Some(link);
                self.control = Some(control);
                self.joining = Joining::Finding;
                self.finding_since = None;
                self.awaiting_host = false;
            }
            Err(why) => self.fail(&why),
        }
    }

    /// A join that did not happen: what the join screen says, logged too.
    fn fail(&mut self, why: &str) {
        info!("co-op: not joined: {why}");
        self.joining = Joining::Failed(why.to_string());
    }

    fn stop_joining(&mut self) {
        self.link = None;
        self.control = None;
        self.awaiting_host = false;
        self.state_logged.clear();
        self.joining = Joining::Idle;
        self.host = None;
        self.host_lost = false;
        self.forget_peers();
        self.progress.reset();
        self.story = Default::default();
    }

    /// The other players are gone from this game: every body of theirs is taken away on
    /// the next frame, and their aims with them ([`draw`]).
    fn forget_peers(&mut self) {
        for (id, p) in std::mem::take(&mut self.peers) {
            self.strays.extend(p.references(id));
        }
    }
}

/// The host's own load order, for the checks of what guests send (`nvf_coop::guard`): a
/// weapon must be a weapon, a worn item armour, furniture a placed furniture reference,
/// a place a cell or worldspace the host has, and an item one a player can carry.
struct HostForms {
    forms: &'static fnv_sim::forms::FormStore,
    refs: &'static fnv_sim::refs::RefTable,
}

impl HostForms {
    fn sig(&self, form: u32) -> Option<[u8; 4]> {
        self.forms.sig(FormId(form))
    }
}

/// The records a player can carry, drop and pick up (`FormCheck::item`): the record
/// types of inventory objects, with casino chips and caps (CHIP, CMNY) and weapon mods
/// (IMOD) [design].
const ITEM_SIGS: [[u8; 4]; 11] = [*b"ALCH", *b"AMMO", *b"ARMO", *b"BOOK", *b"KEYM", *b"MISC", *b"WEAP", *b"NOTE", *b"CHIP", *b"CMNY", *b"IMOD"];

impl FormCheck for HostForms {
    fn carriable(&self, form: u32) -> bool {
        self.sig(form) == Some(*b"WEAP")
    }
    fn wearable(&self, form: u32) -> bool {
        self.sig(form) == Some(*b"ARMO")
    }
    fn furniture(&self, reference: u32) -> bool {
        self.refs.get(FormId(reference)).is_some_and(|r| self.sig(r.base.0) == Some(*b"FURN"))
    }
    fn place(&self, place: Place) -> bool {
        match place {
            Place::Interior(cell) => self.sig(cell) == Some(*b"CELL"),
            Place::Exterior(world) => self.sig(world) == Some(*b"WRLD"),
        }
    }
    fn item(&self, form: u32) -> bool {
        self.sig(form).is_some_and(|s| ITEM_SIGS.contains(&s))
    }
}

/// Give the host's checks its load order once the world is loaded, and the world's rules
/// (`nvf_coop::world`) once a lobby is hosted.
fn host_forms(mut coop: ResMut<Coop>, sim: Option<Res<nvf_play::sim::SimState>>) {
    if coop.forms_set || !coop.is_host() {
        return;
    }
    let Some(sim) = sim else { return };
    let check: Arc<dyn FormCheck> = Arc::new(HostForms { forms: sim.forms, refs: sim.refs });
    let coop = &mut *coop;
    if let Some(link) = &coop.link {
        link.set_forms(Some(check));
        nvf_coop::world::add_world_rules(&mut |kind, rule| link.add_rule(kind, rule));
        coop.forms_set = true;
        info!("co-op: the host checks guests' forms by its load order, and their world messages by its rules");
    }
}

/// The host's checks on what guests send of shared progress, added as soon as the lobby's
/// link is there: not after the world loads, so no guest item passes unchecked meanwhile
/// (a kind with no rule is let through). Made again for each lobby the host opens
/// ([`Coop::stop_hosting`] forgets them).
fn host_progress_rules(mut coop: ResMut<Coop>) {
    if coop.rules_set || !coop.is_host() {
        return;
    }
    let coop = &mut *coop;
    if let Some(link) = &coop.link {
        coop.progress.checks.set_lines(fnv_game::coop_questlines::LINES.len());
        for (kind, rule) in coop.progress.checks.rules() {
            link.add_rule(kind, rule);
        }
        coop.rules_set = true;
    }
}

/// The lobby screens' asks, and what they show.
fn lobby(
    time: Res<Time<Real>>,
    mut coop: ResMut<Coop>,
    mut asks: MessageReader<LobbyRequest>,
    mut state: ResMut<LobbyState>,
    switch: Res<CoopSwitch>,
    (progress, script): (Option<Res<nvf_play::coop_progress::CoopProgress>>, Option<Res<nvf_play::script::ScriptState>>),
) {
    let now = time.elapsed_secs();
    let coop = &mut *coop;
    for ask in asks.read() {
        match ask {
            LobbyRequest::Create(_) if !switch.on => info!("co-op: the Co-op switch is off: no lobby"),
            LobbyRequest::Create(_) if coop.is_guest() => info!("co-op: in another player's lobby: leave it to open one"),
            LobbyRequest::Create(o) if coop.link.is_none() => {
                coop.host_options = o.clone();
                coop.start_hosting(DEFAULT_PORT, None);
            }
            LobbyRequest::Create(_) => {}
            // One in flight, or one asked less than 2 s ago (the service's limit): the list
            // shown stays.
            LobbyRequest::Browse(_) if coop.browse_job.is_some() || now - coop.browse_at < browse::BROWSE_EVERY => {}
            LobbyRequest::Browse(q) => match browse::start(q) {
                Some(job) => {
                    coop.browse_at = now;
                    coop.browse_job = Some(job);
                    coop.browse = Browse::Loading;
                }
                None => coop.browse = Browse::Failed(tr("The lobby browser needs the matchmaking service: Co-op connection is LAN only, or the service is turned off.").into()),
            },
            LobbyRequest::Report { code } => match browse::report(code) {
                Some(job) => {
                    coop.report_job = Some(job);
                    coop.report_code = code.clone();
                }
                None => coop.report_note = Some((code.clone(), tr("The report needs the matchmaking service.").into())),
            },
            LobbyRequest::Favourite { host_fp, on } => {
                browse::set_favourite(&mut coop.favourites, host_fp, *on);
                info!("co-op: host {host_fp} {} the favourites", if *on { "added to" } else { "taken off" });
                if let Browse::Ready(rows) = &mut coop.browse {
                    for r in rows.iter_mut().filter(|r| r.host_fp == *host_fp) {
                        r.favourite = *on;
                    }
                }
            }
            LobbyRequest::Close if coop.is_host() => {
                coop.stop_hosting();
                info!("co-op: lobby closed");
            }
            LobbyRequest::Close => {}
            LobbyRequest::Leave if coop.is_guest() => {
                coop.stop_joining();
                info!("co-op: left the lobby");
            }
            LobbyRequest::Leave => {}
            LobbyRequest::Find { .. } if !switch.on => info!("co-op: the Co-op switch is off: not joining"),
            LobbyRequest::Find { .. } if coop.is_host() => info!("co-op: close your own lobby to join another"),
            LobbyRequest::Find { code, password } => {
                coop.join_password = password.clone();
                coop.stop_joining();
                coop.start_joining(code);
            }
            LobbyRequest::Kick { id, ban } if coop.is_host() => {
                let done = coop.link.as_ref().is_some_and(|l| l.kick(*id, *ban));
                let who = coop.peers.get(id).map_or_else(|| label(*id, ""), |p| label(*id, &p.name));
                match (done, *ban) {
                    (true, true) => info!("co-op: {who} (player {id}) kicked and banned while this lobby lasts"),
                    (true, false) => info!("co-op: {who} (player {id}) kicked"),
                    (false, _) => info!("co-op: {who} (player {id}): not kicked (no such guest, or no key to ban)"),
                }
            }
            LobbyRequest::Kick { .. } => {}
            LobbyRequest::Admit { peer, yes } if coop.is_host() => {
                if let Some(c) = &coop.control {
                    c.admit(*peer, *yes);
                }
                coop.asking.retain(|(p, _)| p != peer);
            }
            LobbyRequest::Admit { .. } => {}
            // A guest's quest sync choices: `nvf_play::coop_progress` keeps them.
            LobbyRequest::QuestSync(_) | LobbyRequest::SitOut { .. } => {}
            LobbyRequest::Cancel => {
                if coop.joining == Joining::Finding {
                    coop.stop_joining();
                }
                if matches!(coop.joining, Joining::Failed(_)) {
                    coop.joining = Joining::Idle;
                }
            }
        }
    }
    // The Browse tab's answer, and a report's.
    if let Some(b) = coop.browse_job.as_ref().and_then(|j| browse::poll(j, &connect::load_order(), &coop.favourites)) {
        coop.browse = b;
        coop.browse_job = None;
    }
    if let Some(r) = coop.report_job.as_ref().and_then(browse::report_done) {
        coop.report_job = None;
        let said = match r {
            Ok(()) => tr("Lobby reported. Thank you.").to_string(),
            Err(why) => trf("The report did not go through: {why}", &[("why", &why)]),
        };
        coop.report_note = Some((coop.report_code.clone(), said));
    }
    // The host's listing follows its place and friendly fire (sent when it changes).
    if coop.is_host() && browse::listed(&coop.host_options) {
        let now_listing = (coop.host_options.public, browse::title(&coop.host_options, &coop.name), coop.place.clone(), coop.friendly_fire);
        if coop.listing_sent.as_ref() != Some(&now_listing) {
            if let Some(c) = &coop.control {
                c.set_listing(now_listing.0, &now_listing.1, &now_listing.2, now_listing.3);
            }
            coop.listing_sent = Some(now_listing);
        }
    }
    // A guest's search: answered (`take_events`), or given up.
    if coop.joining == Joining::Finding {
        let since = *coop.finding_since.get_or_insert(now);
        if coop.link.as_ref().is_some_and(|l| l.id().is_some()) {
            coop.joining = Joining::Found;
        } else if connect::waited_too_long(coop, since, now) {
            coop.link = None;
            coop.control = None;
            coop.fail(tr("No answer from the host: check the code, and that their lobby is open."));
        }
    }
    let hosting = match (&coop.code, coop.is_host()) {
        (Some(code), true) => Hosting::Open { code: code.clone(), note: coop.note.clone() },
        (None, true) => Hosting::Open { code: tr("(asking for a code...)").into(), note: coop.note.clone() },
        _ => Hosting::Off,
    };
    let mut players = Vec::new();
    if coop.link.is_some() {
        // Who shares quest progress and the questlines each sits out
        // (docs/design/coop-quest-sync.md §5): a guest knows its own, the host every guest's.
        let own_sat_out = script.as_deref().and_then(|s| s.rt.as_ref()).map_or(0, |rt| rt.game.player.quest_sync.mask());
        let sharing = |id: Option<u8>| -> String {
            let Some(p) = progress.as_deref() else { return String::new() };
            let (on, sat_out) = match id {
                None => (p.joint_on, own_sat_out),
                Some(id) => (p.joint_peers.get(&id).map(|(_, on)| *on), p.joint_sat_out.get(&id).copied().unwrap_or(0)),
            };
            sharing_label(on, sat_out)
        };
        players.push(if coop.is_host() { format!("{}{}", coop.name, tr(" (host, you)")) } else { format!("{}{}{}", coop.name, tr(" (you)"), sharing(None)) });
        // The host's line per guest says how it is connected and its ping
        // (docs/design/coop-networking.md §10).
        let paths = if coop.is_host() { coop.link.as_ref().map(Link::paths).unwrap_or_default() } else { Vec::new() };
        for (&id, p) in &coop.peers {
            let n = label(id, &p.name);
            // The host is player 0: marked on a guest's list.
            let n = if id == 0 && coop.is_guest() && n != tr("The host") { format!("{n}{}", tr(" (host)")) } else { n };
            let n = if coop.is_host() { format!("{n}{}", sharing(Some(id))) } else { n };
            let n = match paths.iter().find(|(i, ..)| *i == id) {
                Some((_, addr, rtt)) => format!("{n}{}", path_label(addr.map(nvf_coop::stack::Path::of), *rtt)),
                None => n,
            };
            players.push(if p.lost { format!("{n}{}", tr(" (reconnecting)")) } else { n });
        }
    }
    let guests = if coop.is_host() { coop.peers.iter().map(|(&id, p)| (id, label(id, &p.name))).collect() } else { Vec::new() };
    let asks = if coop.is_host() { coop.asking.clone() } else { Vec::new() };
    let host = coop.is_host();
    state.set_if_neq(LobbyState {
        hosting,
        players,
        joining: coop.joining.clone(),
        guest: coop.is_guest(),
        guests,
        asks,
        public: host && coop.host_options.public,
        locked: host && !coop.host_options.password.is_empty(),
        browse: coop.browse.clone(),
        report_note: coop.report_note.clone(),
    });
}

/// A guest's connection on the host's lobby list: how it is connected, and the reliable
/// stream's round trip once measured [design: our own words].
fn path_label(path: Option<nvf_coop::stack::Path>, rtt: Option<std::time::Duration>) -> String {
    use nvf_coop::stack::Path;
    let word = path.map(|p| match p {
        Path::Lan => tr("local network"),
        Path::Direct => tr("direct"),
        Path::Relayed => tr("through another player"),
        Path::HostedRelay => tr("through a relay"),
    });
    match (word, rtt) {
        (Some(w), Some(r)) => trf(" ({path}, {ms} ms)", &[("path", &w), ("ms", &r.as_millis())]),
        (Some(w), None) => format!(" ({w})"),
        (None, Some(r)) => trf(" ({ms} ms)", &[("ms", &r.as_millis())]),
        (None, None) => String::new(),
    }
}

/// The host's place for its lobby browser listing, looked up every
/// [`browse::LISTING_EVERY`] seconds while it hosts a listed or locked lobby.
fn track_place(world: &mut World) {
    let now = world.resource::<Time>().elapsed_secs();
    let due = world.get_resource::<Coop>().is_some_and(|c| c.is_host() && browse::listed(&c.host_options) && now - c.place_at >= browse::LISTING_EVERY);
    if !due {
        return;
    }
    let place = browse::place_now(world).unwrap_or_default();
    let mut coop = world.resource_mut::<Coop>();
    coop.place_at = now;
    if coop.place != place {
        coop.place = place;
    }
}

/// A load restarts the game (`crate::handoff`): a host hands its lobby to the new run
/// ([`connect::hand_over`]); when the new run fails and this one stays, it hosts again.
fn hand_over_on_load(mut coop: ResMut<Coop>, handoff: Option<Res<crate::handoff::Handoff>>) {
    let Some(h) = handoff else { return };
    let coop = &mut *coop;
    if h.leaving() && h.loading() && coop.is_host() && !coop.handed_off {
        connect::hand_over(coop);
    } else if !h.leaving() && coop.handed_off {
        coop.handed_off = false;
        info!("co-op: the new run did not take over: hosting the lobby again");
        let again = connect::handed_over().and_then(|(_, a)| a);
        coop.start_hosting(coop.port, again);
    }
}

/// A player's quest sharing on the lobby list (docs/design/coop-quest-sync.md §5): sharing,
/// and the questlines sat out (two by name, then how many more, so six players' lines stay
/// on the screen); not sharing; or nothing before it chose [design: our own words].
fn sharing_label(on: Option<bool>, sat_out: u64) -> String {
    match on {
        None => String::new(),
        Some(false) => tr(" (not sharing quests)").into(),
        Some(true) if sat_out == 0 => tr(" (sharing quests)").into(),
        Some(true) => {
            let names = fnv_game::coop_questlines::names(sat_out);
            let shown = names.iter().take(2).map(|n| tr(n)).collect::<Vec<_>>().join(", ");
            let more = if names.len() > 2 { format!(" +{}", names.len() - 2) } else { String::new() };
            trf(" (sharing quests, sitting out {names})", &[("names", &format!("{shown}{more}"))])
        }
    }
}

fn label(id: u8, name: &str) -> String {
    match (id, name.is_empty()) {
        (0, true) => tr("The host").into(),
        (_, true) => trf("Player {n}", &[("n", &(id + 1))]),
        _ => name.to_string(),
    }
}

fn place_of(loc: &Location) -> Place {
    loc.interior.map_or(Place::Exterior(loc.world), Place::Interior)
}

/// The main menu is up: this game has no player in the world yet.
fn at_main_menu(m: Option<&nvf_play::main_menu::MainMenu>) -> bool {
    m.is_some_and(|m| m.open)
}

/// A player in the lobby besides this one.
#[derive(Default)]
struct Peer {
    name: String,
    /// States as they came (seconds of `Time`, state), oldest first.
    samples: VecDeque<(f32, PlayerState)>,
    lost: bool,
    spawned: bool,
    root: Option<Entity>,
    /// The space the puppet was drawn in.
    drawn_in: Option<Place>,
    failed: bool,
    /// The attack and reload counters played up to (None until the first state: those
    /// before it are not played).
    played: Option<(u16, u16)>,
    /// When its last attack was played (seconds of `Time`).
    attacked_at: Option<f32>,
    /// Its look, once heard: `Some(None)` for the player record's own (no character
    /// creator look).
    look: Option<Option<Look>>,
    /// Its character's name, once its look brought one: then its `name`, whatever
    /// co-op name it joined with.
    character: Option<String>,
    /// On the host: its actor in this world ([`stand_ins`]).
    actor: Option<FormId>,
    /// Down, as last seen ([`downed`]); drawn anew (standing) on the next frame.
    down: bool,
    redraw: bool,
    /// Every body of this player in the world is taken away before it is drawn again
    /// ([`Peer::undraw`]).
    purge: bool,
    /// Down: where its puppet's body lies here (pelvis, game space; [`revive`]).
    body: Option<[f32; 3]>,
    /// Revived: its puppet getting up from where the body lay ([`revive`]).
    rising: Option<revive::Rising>,
}

impl Peer {
    /// The reference its puppet is drawn with: its actor's on the host, else
    /// [`PUPPET_REF`] + its id.
    fn reference(&self, id: u8) -> u32 {
        self.actor.map_or(PUPPET_REF + id as u32, |r| r.0)
    }

    /// The references its bodies can have: its puppet's, and its actor's on the host.
    fn references(&self, id: u8) -> impl Iterator<Item = u32> {
        std::iter::once(PUPPET_REF + id as u32).chain(self.actor.map(|r| r.0))
    }

    /// Take its body away, to be drawn anew. Not only the root it knows: a body spawned
    /// in the frames before its root is found (or one left lying when it got up) would
    /// stay behind, frozen where it was; [`draw`] takes every body of its references away
    /// first ([`Peer::purge`]).
    fn undraw(&mut self) {
        self.root = None;
        self.spawned = false;
        self.drawn_in = None;
        self.purge = true;
    }

    /// The newest state, when it has one.
    fn newest(&self) -> Option<PlayerState> {
        self.samples.back().map(|&(_, s)| s)
    }

    /// The pose to draw at `t`: blended between the states either side of it.
    fn pose(&self, t: f32) -> Option<(Place, [f32; 3], f32, bool)> {
        let last = self.samples.back()?.1;
        let i = self.samples.iter().position(|(at, _)| *at > t);
        let (a, b, f) = match i {
            None => return Some((last.place, last.feet, last.yaw, false)),
            Some(0) => {
                let s = self.samples[0].1;
                return Some((s.place, s.feet, s.yaw, false));
            }
            Some(i) => {
                let (ta, a) = self.samples[i - 1];
                let (tb, b) = self.samples[i];
                (a, b, ((t - ta) / (tb - ta).max(1e-4)).clamp(0.0, 1.0))
            }
        };
        if a.place != b.place {
            return Some((b.place, b.feet, b.yaw, false));
        }
        let feet = std::array::from_fn(|k| a.feet[k] + (b.feet[k] - a.feet[k]) * f);
        let turn = (b.yaw - a.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let moved = (0..3).map(|k| (b.feet[k] - a.feet[k]).powi(2)).sum::<f32>() > 0.25;
        Some((b.place, feet, a.yaw + turn * f, moved && !self.lost))
    }
}

/// The other players for the crosshair's revive and the downed screen
/// (`nvf_play::coop_mates`): empty outside co-op.
fn social_mates(coop: Res<Coop>, loc: Option<Res<Location>>, mates: Option<ResMut<SocialMates>>) {
    let Some(mut mates) = mates else { return };
    let want = match (coop.link.as_ref(), loc.as_deref()) {
        (Some(link), Some(loc)) => {
            let here = place_of(loc);
            SocialMates {
                me: link.id(),
                interior: loc.interior.is_some(),
                mates: coop
                    .peers
                    .iter()
                    .filter_map(|(&id, p)| {
                        let s = p.newest()?;
                        Some(Mate { id, name: label(id, &p.name), reference: p.reference(id), here: s.place == here, feet: s.feet, yaw: s.yaw, down: s.has(flag::DOWN), body: p.body.filter(|_| s.has(flag::DOWN)), lost: p.lost })
                    })
                    .collect(),
            }
        }
        _ => SocialMates::default(),
    };
    if *mates != want {
        *mates = want;
    }
}

/// What [`send`] reads of this player besides its place.
#[derive(bevy::ecs::system::SystemParam)]
struct Doing<'w> {
    control: Option<Res<'w, nvf_play::control::PlayerControl>>,
    anim: Option<Res<'w, nvf_play::combat::WeaponAnim>>,
    combat: Option<Res<'w, nvf_play::combat::Combat>>,
    zoom: Option<Res<'w, nvf_play::aim_zoom::AimZoom>>,
    phys: Option<Res<'w, nvf_play::phys::PhysState>>,
    actor_world: Option<Res<'w, ActorWorld>>,
    sim: Option<Res<'w, nvf_play::sim::SimState>>,
    script: Option<Res<'w, nvf_play::script::ScriptState>>,
    /// The furniture holding the player (`nvf_play::furniture`).
    seat: Option<Res<'w, nvf_play::furniture::PlayerSeat>>,
    knockdown: Option<Res<'w, crate::player_knockdown::PlayerKnockdown>>,
}

/// This player's state, for the others: where it is, what it holds and wears, what it
/// does, and its look.
fn send(coop: Res<Coop>, player: Option<Res<Player>>, loc: Option<Res<Location>>, state: Option<Res<State<PlayState>>>, main_menu: Option<Res<nvf_play::main_menu::MainMenu>>, doing: Doing, mut was_lying: Local<bool>) {
    let (Some(link), Some(player), Some(loc)) = (coop.link.as_ref(), player, loc) else { return };
    // At the main menu there is no player in the world yet.
    if at_main_menu(main_menu.as_deref()) {
        return;
    }
    // Behind a loading screen the place is the one being left: keep the last state sent.
    if state.as_ref().is_some_and(|s| *s.get() == PlayState::Loading) {
        return;
    }
    let mut s = PlayerState::at(place_of(&loc), player.feet, player.heading);
    s.pitch = player.pitch;
    let control = doing.control.as_deref();
    let out = control.is_some_and(|c| c.weapon_out);
    let aiming = out && control.is_some_and(|c| c.aiming);
    let mut flags = 0;
    flags |= if out { flag::DRAWN } else { 0 };
    flags |= if aiming { flag::AIMING } else { 0 };
    flags |= if control.is_some_and(|c| c.sneak) { flag::SNEAKING } else { 0 };
    flags |= if doing.phys.as_ref().and_then(|p| p.player.as_ref()).is_some_and(|c| !c.grounded) { flag::AIR } else { 0 };
    let sights = match (doing.combat.as_deref(), doing.anim.as_deref()) {
        (Some(c), Some(a)) => c.player_sights_up(a, aiming),
        _ => false,
    } || doing.zoom.as_ref().is_some_and(|z| z.aimed);
    flags |= if sights && out { flag::SIGHTS } else { 0 };
    flags |= if doing.script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.is_dead(FormId::PLAYER_REF)) { flag::DOWN } else { 0 };
    let lying = doing.knockdown.as_ref().is_some_and(|k| k.lying());
    flags |= if lying { flag::LYING } else { 0 };
    if lying != *was_lying {
        *was_lying = lying;
        // The others' copies of this player get up when this ends (`revive::bodies`).
        info!("co-op: sent: this player's body {}", if lying { "lies" } else { "gets up" });
    }
    s.flags = flags;
    if doing.phys.as_ref().and_then(|p| p.player.as_ref()).is_some_and(|c| c.swimming) {
        s.more |= net::more_flag::SWIMMING;
    }
    let worn = match (doing.actor_world.as_deref(), doing.sim.as_deref()) {
        (Some(w), Some(sim)) => Equipment::of(w, Some(&sim.world), FormId::PLAYER_REF.0),
        _ => None,
    };
    let weapon = doing.combat.as_ref().and_then(|c| c.player_weapon()).or(worn.as_ref().and_then(|e| e.weapon));
    s.weapon = weapon.map_or(0, |w| w.0);
    for (slot, w) in s.worn.iter_mut().zip(worn.iter().flat_map(|e| e.worn.iter())) {
        *slot = w.0;
        s.worn_len += 1;
    }
    // Getting off is sent as off: the others' puppet starts its getting-off clip with this
    // player's, not once it has ended [observed: buckallobill, in game, FNV 1.4.0.525: 3.4 s
    // late on both screens otherwise].
    s.furniture = doing.seat.as_deref().filter(|seat| !seat.getting_off).and_then(|seat| seat.on).unwrap_or(0);
    if let Some(a) = doing.anim.as_deref() {
        s.attacks = a.attacks as u16;
        s.reloads = a.reloads as u16;
        s.attack_kind = attack_kind(a.swing.as_deref());
    }
    link.set_state(s);
    // The link sends it again only when it changes (and now and then).
    let rt = doing.script.as_deref().and_then(|s| s.rt.as_ref());
    let look = rt.and_then(|rt| rt.game.player.look.as_ref());
    // The character's own name (the name menu's), for the others to call this player by.
    let name = rt.and_then(|rt| rt.game.player.name.as_deref()).unwrap_or_default();
    link.set_look(encode_look(look, name));
}

/// A look as it goes over the link: 0 for the player record's own; else 1, the race, sex,
/// hair (0 for bald), hair colour (a flag and RGB), eyes (0 for the race's), the head
/// parts, and the three FaceGen lists, each list a count and its items. After it the
/// character's name: its length and its UTF-8 bytes (at most [`NAME_MAX`]; 0 for none).
pub fn encode_look(look: Option<&Look>, name: &str) -> Vec<u8> {
    let mut b = look_only(look);
    let mut end = name.len().min(NAME_MAX);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    b.push(end as u8);
    b.extend_from_slice(&name.as_bytes()[..end]);
    b
}

/// The longest character name sent (bytes) [design: the name menu's names fit].
pub const NAME_MAX: usize = 48;

fn look_only(look: Option<&Look>) -> Vec<u8> {
    let Some(l) = look else { return vec![0] };
    let mut b = vec![1];
    let id = |b: &mut Vec<u8>, f: Option<FormId>| b.extend_from_slice(&f.map_or(0, |f| f.0).to_le_bytes());
    id(&mut b, Some(l.race));
    b.push(l.female as u8);
    id(&mut b, l.hair);
    match l.hair_color {
        Some(c) => b.extend_from_slice(&[1, c[0], c[1], c[2]]),
        None => b.extend_from_slice(&[0, 0, 0, 0]),
    }
    id(&mut b, l.eyes);
    let parts = &l.head_parts[..l.head_parts.len().min(16)];
    b.push(parts.len() as u8);
    for p in parts {
        id(&mut b, Some(*p));
    }
    for list in [&l.fggs, &l.fgga, &l.fgts] {
        let list = &list[..list.len().min(64)];
        b.push(list.len() as u8);
        for v in list {
            b.extend_from_slice(&v.to_le_bytes());
        }
    }
    b
}

/// [`encode_look`] read back: the look and the name (empty for none, and from a game that
/// sent no name); None for one that is not a look. Anything after the name (a later
/// game's additions) is left unread.
pub fn decode_look(b: &[u8]) -> Option<(Option<Look>, String)> {
    let mut rd = Reader(b.get(1..)?);
    let look = match *b.first()? {
        0 => None,
        1 => Some(read_look(&mut rd)?),
        _ => return None,
    };
    if rd.0.is_empty() {
        return Some((look, String::new()));
    }
    let n = rd.u8()? as usize;
    let s = rd.0.get(..n)?;
    Some((look, String::from_utf8_lossy(s).into_owned()))
}

fn read_look(rd: &mut Reader) -> Option<Look> {
    let race = FormId(rd.u32()?);
    let female = rd.u8()? != 0;
    let hair = Some(rd.u32()?).filter(|&h| h != 0).map(FormId);
    let c = [rd.u8()?, rd.u8()?, rd.u8()?, rd.u8()?];
    let hair_color = (c[0] != 0).then_some([c[1], c[2], c[3]]);
    let eyes = Some(rd.u32()?).filter(|&e| e != 0).map(FormId);
    let n = rd.u8()? as usize;
    let head_parts = (0..n).map(|_| rd.u32().map(FormId)).collect::<Option<Vec<_>>>()?;
    let list = |rd: &mut Reader| -> Option<Vec<f32>> {
        let n = rd.u8()? as usize;
        (0..n).map(|_| rd.u32().map(f32::from_bits)).collect()
    };
    let (fggs, fgga, fgts) = (list(rd)?, list(rd)?, list(rd)?);
    Some(Look { race, female, hair, hair_color, eyes, head_parts, fggs, fgga, fgts })
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn u8(&mut self) -> Option<u8> {
        let (&v, rest) = self.0.split_first()?;
        self.0 = rest;
        Some(v)
    }

    fn u32(&mut self) -> Option<u32> {
        let v = u32::from_le_bytes(self.0.get(..4)?.try_into().ok()?);
        self.0 = &self.0[4..];
        Some(v)
    }
}

/// What the newest attack was, from the melee swing's clip stem (`attackleft_b`,
/// `attackforwardpower`; none for a shot).
fn attack_kind(swing: Option<&str>) -> u8 {
    let Some(stem) = swing.map(str::to_ascii_lowercase) else { return attack::SHOT };
    if !stem.contains("power") {
        return attack::SWING;
    }
    if stem.contains("forward") {
        attack::POWER_FORWARD
    } else if stem.contains("back") {
        attack::POWER_BACK
    } else if stem.contains("left") {
        attack::POWER_LEFT
    } else if stem.contains("right") {
        attack::POWER_RIGHT
    } else {
        attack::POWER
    }
}

/// A weapon swung rather than fired (fists when none).
fn is_melee(world: Option<&ActorWorld>, weapon: Option<FormId>) -> bool {
    let Some(w) = weapon else { return true };
    let kind = world.and_then(|wd| wd.actors.weapons.get(&w)).and_then(|w| w.anim_type.as_deref()).and_then(AnimType::from_name);
    matches!(kind, Some(AnimType::HandToHand | AnimType::Melee1 | AnimType::Melee2))
}

/// A hit as the combat code has it, as the link carries it.
fn hit_view(h: &RelayedHit) -> HitView {
    HitView {
        target: h.target.0,
        dealt: h.dealt,
        weapon: h.weapon.map_or(0, |w| w.0),
        location: h.location,
        critical: h.critical,
        limb: h.limb.map(|l| LimbView { condition: l.condition, health_percent: l.health_percent, damage: l.damage, points: l.points }),
    }
}

/// [`hit_view`] back.
fn relayed_hit(h: &HitView) -> RelayedHit {
    RelayedHit {
        target: FormId(h.target),
        dealt: h.dealt,
        weapon: Some(h.weapon).filter(|&w| w != 0).map(FormId),
        location: h.location,
        critical: h.critical,
        limb: h.limb.map(|l| fnv_game::combat::LimbHit { condition: l.condition, health_percent: l.health_percent, damage: l.damage, points: l.points }),
    }
}

/// A guest's hit as the host applies it: the limb takes no more than `cap` (the guest's
/// number is taken only so far; `checks::limb_cap`). Condition points are not a share of
/// the damage.
fn clamp_limb(mut h: RelayedHit, cap: f64) -> RelayedHit {
    if let Some(l) = h.limb.as_mut().filter(|l| !l.points) {
        l.damage = l.damage.clamp(0.0, cap);
    }
    h
}

/// Guest `id` as the host's checks take it: where its newest state put it.
fn guest_at(coop: &Coop, id: u8) -> Option<checks::Guest> {
    let s = coop.peers.get(&id)?.newest()?;
    Some(checks::Guest { space: space_of(s.place), feet: s.feet })
}

/// A place as the network names a space (`NpcState::space`): the cell indoors, the
/// worldspace outdoors.
fn space_of(place: Place) -> u32 {
    match place {
        Place::Interior(c) | Place::Exterior(c) => c,
    }
}

/// A guest's message the host's checks refused (module notes of [`checks`]): counted,
/// and logged at most once a second per reason.
fn refused(coop: &mut Coop, id: u8, why: &'static str, now: f32) {
    refused_by(&mut coop.checks, id, why, now);
}

/// [`refused`] with only the checks' record at hand (`replication`). The guest is marked
/// on the link for it at the frame's end ([`note_bad`]).
fn refused_by(checks: &mut checks::HostChecks, id: u8, why: &'static str, now: f32) {
    checks.bad.push(id);
    if checks.refuse(why, now) {
        warn!("co-op check: player {}'s message dropped: {why} ({} refused in all)", id + 1, checks.total());
    }
}

/// The guests whose messages the checks refused are marked on the link
/// (`Link::note_bad`), which keeps their tally.
pub(super) fn note_bad(coop: &mut Coop) {
    let bad = std::mem::take(&mut coop.checks.bad);
    if let Some(link) = coop.link.as_mut() {
        for id in bad {
            link.note_bad(id);
        }
    }
}

/// What the link heard: who joined, left, came back; the others' states and looks; the
/// host's world for a guest, and a guest's acts in it for the host, checked first.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn take_events(
    mut commands: Commands,
    time: Res<Time>,
    mut coop: ResMut<Coop>,
    mut fighters: ResMut<Fighters>,
    mut corner: Option<ResMut<nvf_play::corner_messages::CornerMessages>>,
    actor_world: Option<Res<ActorWorld>>,
    mut remote_hits: MessageWriter<RemoteHit>,
    (mut sim, mut script, combat, phys): (Option<ResMut<nvf_play::sim::SimState>>, Option<ResMut<nvf_play::script::ScriptState>>, Option<Res<nvf_play::combat::Combat>>, Option<Res<nvf_play::phys::PhysState>>),
    mut opened: MessageReader<LockOpened>,
    (solo, mut knock, creator, player): (Option<Res<nvf_play::save::SoloBase>>, Option<ResMut<crate::player_knockdown::PlayerKnockdown>>, Option<Res<nvf_play::creator::CreatorData>>, Option<Res<Player>>),
    loc: Option<Res<Location>>,
) {
    let now = time.elapsed_secs();
    let coop = &mut *coop;
    // The host: Hellos turned away without the lobby's secret since the last look.
    if coop.is_host() && now - coop.hello_drops.1 >= DROPS_EVERY {
        let total = coop.link.as_ref().map_or(0, |l| hello_secret_drops(&l.guard_stats()));
        if total > coop.hello_drops.0 {
            info!("co-op: turned away {} Hello(s) without the lobby's secret", total - coop.hello_drops.0);
        }
        coop.hello_drops = (total, now);
    }
    let Some(link) = coop.link.as_ref() else {
        opened.clear();
        return;
    };
    // What this player unlocked goes to everyone (module notes).
    for o in opened.read() {
        info!("co-op: this player {} {:#010X}: everyone's is unlocked", o.how.word(), o.reference);
        link.send_unlock(o.reference, o.how == OpenedBy::Hacked);
    }
    let mut rt = script.as_deref_mut().and_then(|s| s.rt.as_mut());
    let is_host = link.is_host();
    let mut notes = Vec::new();
    let mut refused_full = false;
    for e in link.events() {
        match e {
            Event::Welcomed(id) => {
                // Keep this game as the guest's own before the host's world reaches it,
                // and leave the story to the host (`Game::coop_guest`).
                if !is_host {
                    commands.insert_resource(nvf_play::save::KeepSolo);
                    if let Some(rt) = rt.as_deref_mut() {
                        rt.game.coop_guest = true;
                    }
                }
                coop.joining = Joining::Found;
                notes.push(trf("Joined the host's game as player {n}", &[("n", &(id + 1))]));
            }
            Event::Refused => {
                notes.push(tr("The host's game is full").into());
                refused_full = true;
            }
            Event::Joined { id, name } => {
                let p = coop.peers.entry(id).or_default();
                if !name.is_empty() && p.character.is_none() {
                    p.name = name;
                }
                if is_host {
                    notes.push(trf("{name} joined", &[("name", &label(id, &p.name))]));
                }
            }
            Event::State { id, state } => {
                if coop.state_logged.get(&id).is_none_or(|&t| now - t >= STATE_LOG_EVERY) {
                    coop.state_logged.insert(id, now);
                    info!("co-op: {}", state_line(id, &state));
                }
                if id == 0 {
                    coop.host = Some(state);
                    if coop.host_place.is_none_or(|(p, _)| p != state.place) {
                        coop.host_place = Some((state.place, now));
                    }
                }
                let p = coop.peers.entry(id).or_default();
                p.samples.push_back((now, state));
                while p.samples.len() > 16 {
                    p.samples.pop_front();
                }
            }
            Event::Lost(id) => {
                let p = coop.peers.entry(id).or_default();
                p.lost = true;
                // A guest hears of the host's loss as HostLost too: say it once.
                if id != 0 || is_host {
                    notes.push(trf("{name} lost connection, waiting for them", &[("name", &label(id, &p.name))]));
                }
            }
            Event::Back(id) => {
                let p = coop.peers.entry(id).or_default();
                p.lost = false;
                if id != 0 || is_host {
                    notes.push(trf("{name} reconnected", &[("name", &label(id, &p.name))]));
                }
            }
            Event::Named { id, name } => {
                info!("co-op: player {} is {name}", id + 1);
                // Once its character's name came (its look), that is its name.
                let p = coop.peers.entry(id).or_default();
                if p.character.is_none() {
                    p.name = name;
                }
            }
            Event::Look { id, look } => {
                let Some((look, character)) = decode_look(&look) else {
                    warn!("co-op: {} sent a look this game cannot read", label(id, &coop.peers.get(&id).map_or(String::new(), |p| p.name.clone())));
                    continue;
                };
                let reference = coop.peers.get(&id).map_or(PUPPET_REF + id as u32, |p| p.reference(id));
                if let Some(w) = actor_world.as_deref() {
                    if let Ok(mut looks) = w.looks.lock() {
                        match &look {
                            Some(l) => {
                                // Another player's face is no younger than an adult either.
                                let mut a = nvf_play::creator::actor_look(l);
                                if let Some(was) = creator.as_deref().and_then(|c| c.0.adult(&mut a)) {
                                    info!("co-op: player {}'s look read {was:.0} years: drawn at {}", id + 1, fnv_actor::creator::MIN_AGE);
                                }
                                looks.insert(reference, a)
                            }
                            None => looks.remove(&reference),
                        };
                    }
                }
                let p = coop.peers.entry(id).or_default();
                // Called by its character's name from here on (the name menu's), not the
                // co-op name it joined with.
                if !character.is_empty() && p.character.as_deref() != Some(character.as_str()) {
                    info!("co-op: {} is called {character}", label(id, &p.name));
                    p.name = character.clone();
                    p.character = Some(character);
                }
                // The link sends the look again now and then: drawn again only when it changed.
                if p.look.as_ref() != Some(&look) {
                    info!("co-op: {} looks {}", label(id, &p.name), look.as_ref().map_or("as the player record does".to_string(), |l| format!("race {}, {}", l.race, if l.female { "female" } else { "male" })));
                    p.look = Some(look);
                    p.undraw();
                    p.failed = false;
                }
            }
            Event::Left(id) => {
                let reference = coop.peers.get(&id).map_or(PUPPET_REF + id as u32, |p| p.reference(id));
                fighters.set_puppet(reference, None);
                if let Some(w) = actor_world.as_deref() {
                    if let Ok(mut looks) = w.looks.lock() {
                        looks.remove(&reference);
                    }
                }
                if let Some(p) = coop.peers.remove(&id) {
                    if let Some(e) = p.root {
                        commands.entity(e).despawn();
                    }
                    coop.strays.extend(p.references(id));
                    if id != 0 {
                        notes.push(trf("{name} left", &[("name", &label(id, &p.name))]));
                    }
                }
                coop.state_logged.remove(&id);
                coop.checks.forget(id);
                coop.progress.checks.left(id);
                if id == 0 {
                    coop.host = None;
                }
            }
            Event::HostLost => {
                coop.host_lost = true;
                notes.push(tr("Lost the host: reconnecting").into());
            }
            Event::HostBack => {
                coop.host_lost = false;
                notes.push(tr("Reconnected to the host").into());
            }
            Event::HostGone => {
                coop.host = None;
                coop.ended = true;
                notes.push(tr("The host's game has ended").into());
            }
            Event::Kicked { banned } => {
                coop.host = None;
                coop.ended = true;
                notes.push(connect::kicked_note(banned));
            }
            Event::Npcs(list) => {
                for n in list {
                    let track = coop.npcs.entry(n.reference).or_default();
                    // A state after a pause in the host's (more than three rounds) that
                    // puts it far on: where a guest's copy is then put at once
                    // ([`NpcTrack::pose`]), to tell what the host did meanwhile.
                    if let Some(&(last, was)) = track.samples.back() {
                        let far = (0..3).map(|k| (n.feet[k] - was.feet[k]).powi(2)).sum::<f32>().sqrt();
                        if now - last > NPC_GAP && far > NPC_JUMP {
                            info!("co-op: {:08X} heard again after {:.2} s, {far:.0} units from its last state", n.reference, now - last);
                        } else if far > NPC_JUMP && far > NPC_JUMP_SPEED / 2.0 * (now - last).max(1e-3) {
                            info!("co-op: {:08X} heard {far:.0} units on from its last state in {:.2} s", n.reference, now - last);
                        }
                    }
                    track.heard(now, n);
                    if traced(n.reference) {
                        let at = track.samples.back().map_or(now, |(at, _)| *at);
                        info!("co-op trace: {:08X} heard at {now:.3} as {at:.3}: ({:.0}, {:.0}, {:.0}) flags {}", n.reference, n.feet[0], n.feet[1], n.feet[2], n.flags);
                    }
                }
            }
            // A guest's hit on the host's world: checked there, then applied as its
            // actor's (module notes).
            Event::Hit { id, hit, .. } if is_host => {
                let mut h = relayed_hit(&hit);
                let by = coop.peers.get(&id).and_then(|p| p.actor).unwrap_or(FormId::PLAYER_REF);
                // A guest draws the other players as puppets (`PUPPET_REF` + their id)
                // and its hits on them name the puppet: here it is the player itself,
                // the host's player or that guest's actor.
                if is_puppet(h.target.0) {
                    let whose = (h.target.0 - PUPPET_REF) as u8;
                    match if whose == 0 { Some(FormId::PLAYER_REF) } else { coop.peers.get(&whose).and_then(|p| p.actor) } {
                        Some(t) if whose != id => h.target = t,
                        _ => {
                            info!("co-op: player {}'s hit on puppet {} dropped: no such player here", id + 1, h.target);
                            continue;
                        }
                    }
                }
                // Friendly Fire off: a guest's hit on the host or on another player hurts
                // nobody.
                let on_player = h.target == FormId::PLAYER_REF || coop.peers.values().any(|p| p.actor == Some(h.target));
                if on_player && !coop.friendly_fire {
                    info!("co-op: player {}'s hit on player {} dropped: friendly fire is off", id + 1, h.target);
                    continue;
                }
                // The host's world: the target loaded and in reach, the damage the
                // weapon's (`checks::hit`). The host's own player is checked by its feet.
                let verdict = match (sim.as_deref(), guest_at(coop, id)) {
                    (Some(sim), Some(g)) => {
                        let w = checks::SimWorld { sim, combat: combat.as_deref(), phys: phys.as_deref() };
                        checks::hit(&w, &g, &hit_view(&h))
                    }
                    _ => Err(nvf_coop::world::why::NO_STATE),
                };
                if let Err(why) = verdict {
                    refused(coop, id, why, now);
                    continue;
                }
                if let Some(sim) = sim.as_deref() {
                    let w = checks::SimWorld { sim, combat: combat.as_deref(), phys: phys.as_deref() };
                    h = clamp_limb(h, checks::limb_cap(&w, hit.weapon, hit.dealt));
                }
                info!("co-op: player {} ({by}) hit {} for {:.1}", id + 1, h.target, h.dealt);
                remote_hits.write(RemoteHit { by, hit: h });
            }
            // One of the host's actors hit this player there.
            Event::Hit { by, hit, .. } => {
                let h = relayed_hit(&hit);
                info!("co-op: {} hit this player for {:.1} in the host's world", if by == 0 { "something".to_string() } else { FormId(by).to_string() }, h.dealt);
                remote_hits.write(RemoteHit { by: FormId(by), hit: RelayedHit { target: FormId::PLAYER_REF, ..h } });
            }
            // A guest's revive of another guest, on the host: checked against the
            // target's actor here (down, the reviver at its body), then passed on to it.
            Event::ReviveAsked { by, target, perks } => {
                if !is_host {
                    continue;
                }
                let verdict = match (guest_at(coop, by), coop.peers.get(&target)) {
                    (Some(g), Some(t)) => match t.body.or_else(|| t.newest().map(|s| s.feet)).zip(t.newest().map(|s| space_of(s.place))) {
                        Some((feet, space)) => checks::revive(t.down, &g, (space, feet)),
                        None => Err(nvf_coop::world::why::NO_STATE),
                    },
                    _ => Err(nvf_coop::world::why::NO_STATE),
                };
                if let Err(why) = verdict {
                    refused(coop, by, why, now);
                    continue;
                }
                info!("co-op: player {} revives player {}: passed on", by + 1, target + 1);
                if let Some(link) = coop.link.as_mut() {
                    link.relay_revive(target, by, perks);
                }
            }
            Event::Revived { by, perks } => {
                // On the host: checked against its world first (the host's own player
                // down, the reviver at its body). A guest is revived as the host says, or
                // as another guest did once the host passed it on ([`Event::ReviveAsked`]).
                if is_host {
                    let down = rt.as_deref().is_some_and(|rt| rt.game.is_dead(FormId::PLAYER_REF));
                    let body = player.as_deref().zip(loc.as_deref()).map(|(p, l)| (space_of(place_of(l)), p.feet));
                    let verdict = match (guest_at(coop, by), body) {
                        (Some(g), Some(body)) => checks::revive(down, &g, body),
                        _ => Err(nvf_coop::world::why::NO_STATE),
                    };
                    if let Err(why) = verdict {
                        refused(coop, by, why, now);
                        continue;
                    }
                }
                if let Some(rt) = rt.as_deref_mut().filter(|rt| rt.game.is_dead(FormId::PLAYER_REF)) {
                    rt.game.resurrect(FormId::PLAYER_REF, false);
                    // The reviver's co-op perks are the progress side's (piece 3): none
                    // here yet.
                    let _ = perks;
                    // Up from where the body lies (`player_knockdown.rs`).
                    if let Some(k) = knock.as_deref_mut() {
                        k.revived = true;
                    }
                    coop.downed_at = None;
                    let name = label(by, &coop.peers.get(&by).map_or(String::new(), |p| p.name.clone()));
                    coop.revived_by = Some(name.clone());
                    notes.push(trf("{name} revived you", &[("name", &name)]));
                }
            }
            Event::Lines(lines) => coop.host_lines = Some((now, lines)),
            Event::Presence { list: net::LIST_ENABLED, serial, page, pages, refs } => coop.presence.hear(serial, page, pages, refs),
            Event::Presence { list: net::LIST_OPEN, serial, page, pages, refs } => coop.doors_heard.hear(serial, page, pages, refs),
            Event::Presence { .. } => {}
            Event::Wear(npcs) => coop.wear_heard.extend(npcs),
            Event::Door { id, reference, open } => {
                // A guest's door: the host's world has it loaded, unlocked and at hand.
                if is_host {
                    let verdict = match (sim.as_deref(), guest_at(coop, id)) {
                        (Some(sim), Some(g)) => checks::door(&checks::SimWorld { sim, combat: combat.as_deref(), phys: phys.as_deref() }, &g, reference),
                        _ => Err(nvf_coop::world::why::NO_STATE),
                    };
                    if let Err(why) = verdict {
                        refused(coop, id, why, now);
                        continue;
                    }
                    // Checked: the other guests get it too.
                    if let Some(link) = coop.link.as_mut() {
                        link.relay_door(id, reference, open);
                    }
                }
                coop.doors_in.push((id, reference, open));
            }
            Event::Scene(v) => coop.scene.heard = Some((v, now)),
            Event::Clock(c) => {
                // Not before this game is kept as the guest's own.
                if let Some(sim) = sim.as_deref_mut().filter(|_| solo.is_some()) {
                    let ahead = take_clock(&mut sim.world, c);
                    let steady = coop.clock_heard.replace(now).is_some_and(|t| now - t <= CLOCK_STEADY);
                    if let (Some(h), Some(rt)) = (guest_hours_passed(ahead, steady), rt.as_deref_mut()) {
                        let p = nvf_play::script::with_world(rt, sim, |rt| rt.game.pass_hours(h, false));
                        let needs: Vec<String> = p.needs.iter().map(|&(c, a, b)| format!("{c}: {a:.0} -> {b:.0}")).collect();
                        info!("co-op: the host's clock went {h:.2} h ahead (a wait, a sleep or a fast travel): they pass for this player too; needs {}", needs.join(", "));
                    }
                }
            }
            Event::Unlocked { id, reference, terminal } => {
                // A guest's unlock: the host's world has the lock there and the guest at it.
                if is_host {
                    let verdict = match (sim.as_deref(), guest_at(coop, id)) {
                        (Some(sim), Some(g)) => checks::unlock(&checks::SimWorld { sim, combat: combat.as_deref(), phys: phys.as_deref() }, &g, reference, terminal),
                        _ => Err(nvf_coop::world::why::NO_STATE),
                    };
                    if let Err(why) = verdict {
                        refused(coop, id, why, now);
                        continue;
                    }
                    // Checked: the other guests get it too.
                    if let Some(link) = coop.link.as_mut() {
                        link.relay_unlock(id, reference, terminal);
                    }
                }
                if let (Some(rt), Some(sim)) = (rt.as_deref_mut(), sim.as_deref_mut()) {
                    if unlock(sim, &mut rt.game, FormId(reference), terminal) {
                        info!("co-op: {} unlocked {reference:#010X}{}", label(id, &coop.peers.get(&id).map_or(String::new(), |p| p.name.clone())), if terminal { " (a terminal)" } else { "" });
                    }
                }
            }
            // For `replication::replicate`.
            // Shared progress (`progress`).
            Event::Reliable { id, item } if nvf_coop::progress::is_progress_item(&item) => coop.progress.heard.push((id, item)),
            // For `replication::replicate`; a stream started again also sends the host's
            // progress views again from the start.
            e @ (Event::Reliable { .. } | Event::RelReset { .. } | Event::Actors { .. }) => {
                if matches!(e, Event::RelReset { .. }) {
                    coop.progress.views.again();
                }
                coop.repl.inbox.push(e)
            }
        }
    }
    note_bad(coop);
    for n in notes {
        info!("co-op: {n}");
        if let Some(c) = corner.as_mut() {
            c.push(n.into());
        }
    }
    if refused_full {
        coop.link = None;
        coop.fail(tr("The host's game is full."));
    }
}

/// Guests: go to the host when it is in another space, or too far away outdoors.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn leash(
    time: Res<Time>,
    mut coop: ResMut<Coop>,
    (mut player, mut phys): (Option<ResMut<Player>>, Option<ResMut<nvf_play::phys::PhysState>>),
    loc: Option<Res<Location>>,
    loading: Option<Res<Loading>>,
    main_menu: Option<Res<nvf_play::main_menu::MainMenu>>,
    mut travel: MessageWriter<TravelRequest>,
    solo: Option<Res<nvf_play::save::SoloBase>>,
) {
    // A downed guest stays down where it fell (it respawns by the host, `downed`). Not
    // moved before its own game is kept (module notes).
    if !coop.is_guest() || coop.host_lost || at_main_menu(main_menu.as_deref()) || coop.downed_at.is_some() || solo.is_none() {
        return;
    }
    if loading.is_some_and(|l| l.active()) {
        return;
    }
    let now = time.elapsed_secs();
    // The replication build: each player goes where they like (their own doors and fast
    // travel move only them, and the host simulates each one's surroundings); only a
    // script that moves the host's player (a DLC's start, MoveTo) takes the guests along
    // (`replication`'s Summon).
    let Some((interior, w, feet, yaw)) = coop.repl.summon.take() else { return };
    // Beside the host, not on it: a guest put on the host's feet pushed the host 40 units
    // aside, out of a 250-unit greeting's reach, and the greeting never came
    // [observed: buckallobill, in game, FNV 1.4.0.525].
    // Each guest its own place (every guest landed on one spot at Zion).
    let slot = coop.link.as_ref().and_then(|l| l.id()).unwrap_or(1);
    let feet = scene::beside_slot(feet, yaw, slot);
    coop.last_leash = Some(now);
    // Already in the host's place: put there at once, with no loading screen (the Dead Money summon within the bunker showed a loading screen for a step).
    let there = if interior != 0 { Place::Interior(interior) } else { Place::Exterior(w) };
    if loc.as_deref().map(place_of) == Some(there) {
        if let (Some(player), Some(phys)) = (player.as_deref_mut(), phys.as_deref_mut().filter(|p| p.player.is_some() && p.ready_at(feet))) {
            phys.place_player(feet, 60.0, 200.0);
            let at = phys.player.as_ref().map_or(feet, |c| c.feet);
            player.feet = at;
            player.placed = Some(at);
            player.heading = yaw;
            info!("co-op: summoned: to the host at {there:?} {at:?} (guest place {slot}): put there, same place");
            return;
        }
    }
    let (world, interior) = if interior != 0 { (None, Some(interior)) } else { (Some(w), None) };
    info!("co-op: summoned: to the host at {:?} {:?} {feet:?} (guest place {slot})", world, interior);
    travel.write(TravelRequest(Travel { world, interior, feet, heading_deg: yaw.to_degrees(), pitch_deg: None }));
}

/// Draw each other player where the link last put it, in this game's space only.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    time: Res<Time>,
    mut coop: ResMut<Coop>,
    loc: Option<Res<Location>>,
    actor_world: Option<Res<ActorWorld>>,
    mut ahead: ResMut<crate::readahead::ReadAhead>,
    sim: Option<Res<nvf_play::sim::SimState>>,
    mut parts: ResMut<ActorParts>,
    mut models: ModelCtx,
    root: Query<Entity, With<WorldRoot>>,
    roots: Query<(Entity, &RefVisual, &ChildOf)>,
    mut actors: Query<(&mut Transform, Option<&mut Locomotion>), Without<crate::furniture::OnFurniture>>,
    mut holds: Query<&mut WeaponHold>,
    dressed: Query<(Option<&PuppetEquipment>, Has<Puppet>)>,
    (mut fighters, mut attacks, main_menu, mut shots): (ResMut<Fighters>, MessageWriter<PlayAttack>, Option<Res<nvf_play::main_menu::MainMenu>>, MessageWriter<PuppetShot>),
    (sfx, listener, mut sounds, lying, seats): (Option<Res<Sfx>>, Option<Res<Player>>, MessageWriter<PlaySound>, Query<(), Or<(With<Dying>, With<crate::ragdoll::Corpse>)>>, Query<(Has<crate::furniture::SitOn>, Has<crate::furniture::OnFurniture>, Has<crate::furniture::GetUp>)>),
) {
    let (Ok(world_root), Some(loc)) = (root.single(), loc) else { return };
    if at_main_menu(main_menu.as_deref()) {
        return;
    }
    let now = time.elapsed_secs();
    // Bodies to take away: of players who left, of players drawn anew, and (once a
    // second) any body of a player that is not its puppet: a stray left behind.
    let sweep = now - coop.swept_at >= SWEEP_EVERY;
    if sweep {
        coop.swept_at = now;
    }
    let mut keep: HashMap<u32, Option<Entity>> = coop.strays.drain(..).map(|r| (r, None)).collect();
    // Players gone (left, or the lobby closed): their aims go with their bodies.
    for &r in keep.keys() {
        fighters.set_puppet(r, None);
    }
    for (&id, p) in coop.peers.iter_mut() {
        if std::mem::take(&mut p.purge) {
            keep.extend(p.references(id).map(|r| (r, None)));
        } else if sweep && p.root.is_some() {
            keep.extend(p.references(id).map(|r| (r, p.root)));
        }
    }
    // Bodies despawned this frame are still in the queries until its end: never taken
    // for a puppet's body again (a body found for one frame, then lost, is drawn anew:
    // a puppet spawned again every frame, never dressed or animated).
    let mut gone = despawn_bodies(&mut commands, &roots, world_root, &keep);
    let here = place_of(&loc);
    let dt = time.delta_secs().min(0.1);
    let mut stats = crate::actors::ActorStats::default();
    for (&id, p) in coop.peers.iter_mut() {
        if p.failed {
            continue;
        }
        let Some((place, feet, yaw, moving)) = p.pose(now - DELAY) else { continue };
        let Some(&(_, newest)) = p.samples.back() else { continue };
        // Up again after being down: its body was a ragdoll. And whatever the reason, a
        // player who is up is never drawn lying (a body that missed getting up).
        let lies = p.root.is_some_and(|e| lying.contains(e));
        // Getting up after a revive ([`revive`]): its body plays the clip where it lay.
        if p.rising.is_some() && p.root.is_some() && !p.redraw {
            continue;
        }
        if std::mem::take(&mut p.redraw) || (lies && !newest.has(flag::DOWN)) {
            if lies {
                info!("co-op: {} is up: drawn standing", label(id, &p.name));
            }
            p.undraw();
        }
        // The world's entities go with the space they belong to: after a trip (here or
        // there), or once its entity is gone, the puppet is spawned again.
        if p.root.is_some_and(|e| roots.get(e).is_err()) {
            p.root = None;
            p.undraw();
        } else if p.drawn_in.is_some_and(|d| d != here || d != place) {
            p.undraw();
        }
        let visible = place == here;
        // Not here: no body of it here either (one spawned but not yet found as its root
        // would stay frozen where it was).
        if !visible && (p.spawned || p.root.is_some()) {
            p.undraw();
        }
        // Taken away now, before it is spawned again below.
        if std::mem::take(&mut p.purge) {
            gone.extend(despawn_bodies(&mut commands, &roots, world_root, &p.references(id).map(|r| (r, None)).collect()));
        }
        if p.root.is_some_and(|e| gone.contains(&e)) {
            p.root = None;
        }
        let reference = p.reference(id);
        let base = PLAYER_BASE;
        let who = Who { reference, base, creature: false, scale: 1.0, strafe: true, air: newest.has(flag::AIR), jumped: false, at_once: false };
        let sim_world = sim.as_deref().map(|s| &s.world);
        if let Err(e) = show_actor(&mut commands, world_root, visible, actor_world.as_deref(), sim_world, &mut ahead, &mut stats, &mut parts, &mut models, &roots, &mut actors, who, (&mut p.spawned, &mut p.root), (feet, yaw), moving, dt) {
            warn!("co-op: {} cannot be drawn as {base:#010X}: {e}", label(id, &p.name));
            p.failed = true;
        }
        if p.root.is_some_and(|e| gone.contains(&e)) {
            p.root = None;
        }
        if visible && p.spawned {
            p.drawn_in = Some(here);
        }
        let Some(e) = p.root.filter(|_| visible) else {
            fighters.set_puppet(reference, None);
            continue;
        };
        // The furniture holding the player: its puppet gets on, sits or lies there, and
        // gets off, as an NPC does (`crate::furniture`) [observed: buckallobill, in game,
        // FNV 1.4.0.525: without it players stood on beds, chairs and couches]. Meanwhile
        // the furniture, not the network, places the body (`actors` leaves it out).
        if let Ok((sitting, on, leaving)) = seats.get(e) {
            match newest.furniture {
                0 if on && !leaving => {
                    commands.entity(e).insert(crate::furniture::GetUp);
                    info!("co-op: {} gets off its furniture", label(id, &p.name));
                }
                f if f != 0 && !sitting && !on => {
                    commands.entity(e).insert(crate::furniture::SitOn { furniture: f, stay: None, at_once: false });
                    info!("co-op: {} uses furniture {f:#010X}", label(id, &p.name));
                }
                _ => {}
            }
        }
        if let Some(weapon) = act(&mut commands, e, reference, p, newest, now, actor_world.as_deref(), &mut actors, &mut holds, &dressed, &mut fighters, &mut attacks) {
            shots.write(PuppetShot { shooter: FormId(reference), weapon });
            if let Some(s) = shot_sound(sfx.as_deref(), listener.as_deref(), weapon, feet) {
                debug!("co-op: {} fires {weapon}: sound {}", label(id, &p.name), s.sound);
                sounds.write(s);
            }
        }
    }
}


/// The host: each other player as an actor of this world (module notes), made when it
/// is first seen, put where it is each frame, and enabled only while it is here, in
/// touch and alive; the actors of players who left, or of a game no longer hosted, are
/// deleted. The hits this world's actors land on them go to their games.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn stand_ins(
    time: Res<Time>,
    mut coop: ResMut<Coop>,
    loc: Option<Res<Location>>,
    actor_world: Option<Res<ActorWorld>>,
    mut sim: Option<ResMut<nvf_play::sim::SimState>>,
    mut script: Option<ResMut<nvf_play::script::ScriptState>>,
    (mut stats, main_menu, mut peer_actors): (ResMut<CombatStats>, Option<Res<nvf_play::main_menu::MainMenu>>, Option<ResMut<nvf_play::coop_downed::CoopPeerActors>>),
) {
    let (Some(sim), Some(rt)) = (sim.as_deref_mut(), script.as_deref_mut().and_then(|s| s.rt.as_mut())) else { return };
    let coop = &mut *coop;
    let host = coop.link.as_ref().is_some_and(|l| l.is_host());
    let kept: HashSet<FormId> = if host { coop.peers.values().filter_map(|p| p.actor).collect() } else { HashSet::new() };
    for r in rt.game.coop_actors.keys().copied().collect::<Vec<_>>() {
        if !kept.contains(&r) {
            let _ = sim.world.set_deleted(r, true);
            rt.game.coop_actors.remove(&r);
            rt.game.extra.remove(&r);
            info!("co-op: {r}, a player's actor, is gone");
        }
    }
    // The guests' stand-ins, for the progress side (`nvf_play::coop_downed`).
    if let Some(pa) = peer_actors.as_deref_mut() {
        let want: BTreeMap<u8, FormId> = if host { coop.peers.iter().filter_map(|(&id, p)| Some((id, p.actor?))).collect() } else { BTreeMap::new() };
        if pa.0 != want {
            pa.0 = want;
        }
    }
    if !host {
        stats.coop_hits.clear();
        return;
    }
    if let Some(link) = coop.link.as_ref() {
        for (by, h) in stats.coop_hits.drain(..) {
            let Some((&id, _)) = coop.peers.iter().find(|(_, p)| p.actor == Some(h.target)) else { continue };
            // Friendly Fire off: a player's hit, or a player's companion's, hurts
            // no other player.
            if !coop.friendly_fire && by.is_some_and(|b| b == FormId::PLAYER_REF || rt.game.is_coop_actor(b) || rt.game.is_teammate(b)) {
                info!("co-op: a hit on player {} by {} dropped: friendly fire is off", id + 1, by.map_or(String::new(), |b| b.to_string()));
                continue;
            }
            info!("co-op: {} hit player {} for {:.1}", by.map_or("something".to_string(), |b| b.to_string()), id + 1, h.dealt);
            link.send_hit_to(id, by.map_or(0, |b| b.0), hit_view(&h));
        }
    }
    let Some(loc) = loc.filter(|_| !at_main_menu(main_menu.as_deref())) else { return };
    if let Some(link) = coop.link.as_ref() {
        if let Ok(c) = sim.world.calendar() {
            link.set_clock(net::ClockView { year: c.year, month: c.month, day: c.day, hour: c.hour, days_passed: c.days_passed });
        }
        let players: HashMap<FormId, u8> = coop.peers.iter().filter_map(|(&id, p)| Some((p.actor?, id))).collect();
        let said: Vec<fnv_game::game::Speech> = rt.game.speech.iter().cloned().chain(conversation_line(&rt.game)).collect();
        link.set_lines(lines_out(&said, |r| players.get(&r).copied()));
        // The host's world's presence, made again when it changed (or now and then: a
        // load makes a new world).
        let now = time.elapsed_secs();
        let serial = sim.world.presence_serial();
        if coop.presence_made.is_none_or(|(s, at)| s != serial || now - at >= PRESENCE_REMAKE) {
            coop.presence_made = Some((serial, now));
            // The replication build's change log carries the enabled flags (`replication`).
            let _ = (serial, presence_of);
        }
    }
    let here = place_of(&loc);
    let now = time.elapsed_secs();
    let cell = sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.cell);
    for (&id, p) in coop.peers.iter_mut() {
        let (Some((place, feet, yaw, _)), Some(&(_, newest))) = (p.pose(now - DELAY), p.samples.back()) else { continue };
        let r = match p.actor {
            Some(r) => r,
            None => match sim.world.place(FormId(PLAYER_BASE), cell, feet, [0.0, 0.0, yaw], 1) {
                Ok(r) => {
                    rt.game.coop_actors.insert(r, p.name.clone());
                    info!("co-op: {} is {r} in this world", label(id, &p.name));
                    // Its look and its puppet go under the new reference.
                    if let Some(w) = actor_world.as_deref() {
                        if let Ok(mut looks) = w.looks.lock() {
                            if let Some(l) = looks.remove(&(PUPPET_REF + id as u32)) {
                                looks.insert(r.0, l);
                            }
                        }
                    }
                    p.undraw();
                    p.actor = Some(r);
                    r
                }
                Err(e) => {
                    warn!("co-op: {} cannot be an actor of this world: {e}", label(id, &p.name));
                    continue;
                }
            },
        };
        // In the guest's own cell, where it is (a guest away from the host is in
        // the world there, for the AI the host runs in its surroundings, `areas`).
        let own_cell = match place {
            Place::Interior(c) => Some(FormId(c)),
            // Outdoors: the grid cell under its feet (far away in the host's worldspace too).
            Place::Exterior(w) => sim.world.placed.world_grid.get(&FormId(w)).and_then(|g| g.get(&fnv_world::cell_of(feet[0], feet[1]))).copied().or(if place == here { cell } else { None }),
        };
        if let Ok(s) = sim.world.get(r) {
            s.pos = feet;
            s.rot = [0.0, 0.0, yaw];
            s.cell = own_cell.or(cell);
        }
        let _ = sim.world.set_enabled(r, (place == here || own_cell.is_some()) && !p.lost && !newest.has(flag::DOWN));
        if let Some(n) = rt.game.coop_actors.get_mut(&r).filter(|n| **n != p.name && !p.name.is_empty()) {
            n.clone_from(&p.name);
        }
        let x = rt.game.extra.entry(r).or_default();
        x.ai_off = true;
        x.force_sneak = newest.has(flag::SNEAKING);
    }
}

/// A host's NPCs as it draws them, for the guests (module notes).
#[allow(clippy::type_complexity)]
fn share_npcs(
    mut coop: ResMut<Coop>,
    actors: Query<(&RefVisual, &Transform, Option<&Locomotion>, Option<&WeaponHold>, Has<Dying>, &InheritedVisibility, Option<&crate::furniture::OnFurniture>, Option<&crate::outfits::Dressed>), (With<crate::limbs::ActorRig>, Without<Puppet>)>,
    fighters: Res<Fighters>,
    mut attacks: MessageReader<PlayAttack>,
    (mut told, mut sent_at): (Local<usize>, Local<HashMap<u32, [f32; 3]>>),
    (mut sim, actor_world, time): (Option<ResMut<nvf_play::sim::SimState>>, Option<Res<ActorWorld>>, Res<Time>),
    (residents, streamer, world, mut script): (Option<Res<Residents>>, Res<Streamer>, Option<Res<WorldData>>, Option<ResMut<nvf_play::script::ScriptState>>),
) {
    let coop = &mut *coop;
    let Some(link) = coop.link.as_ref().filter(|l| l.is_host()) else {
        attacks.clear();
        return;
    };
    for a in attacks.read().filter(|a| matches!(a.kind, Move::Attack)) {
        let c = coop.npc_attacks.entry(a.actor).or_default();
        *c = c.wrapping_add(1);
    }
    if coop.peers.is_empty() {
        coop.repl.host_npcs.clear();
        return;
    }
    // The other players' actors are drawn as puppets here (the puppet only says so once
    // it is first posed, `act`): a guest's own stand-in, sent as an NPC, stood frozen where
    // it was first drawn.
    let players: HashSet<u32> = coop.peers.values().filter_map(|p| p.actor).map(|r| r.0).collect();
    // The residents simulated round a guest away from the host and not drawn (`areas`).
    let undrawn = match (residents.as_deref(), world.as_deref()) {
        (Some(res), Some(world)) => res.simulated_undrawn(world, &streamer),
        _ => Vec::new(),
    };
    // Hostile to the players in this game: fighting one or ready to attack one, as this
    // game's compass judges it for its own player. A guest's game runs none of the
    // host's scripts (StartCombat, SetEnemy, a faction's turn) and has no fights of its
    // own, so its compass shows what the host's world says (a faction turned on the
    // host showed 0 hostile on the guest's compass otherwise) [observed: buckallobill, in game, FNV 1.4.0.525].
    let mut hostile: HashSet<u32> = HashSet::new();
    // The weapon a weapon use (a Use Weapon package, the UseWeapon function) has it fire:
    // the combat AI's pose has none for it, so a guest drew its shots as punches with no
    // gun in its hands [observed: buckallobill, in game, FNV 1.4.0.525].
    let mut using: HashMap<u32, FormId> = HashMap::new();
    if let (Some(rt), Some(sim)) = (script.as_deref_mut().and_then(|s| s.rt.as_mut()), sim.as_deref_mut()) {
        let refs: Vec<u32> = actors.iter().map(|(v, ..)| v.0).filter(|r| !is_puppet(*r) && !players.contains(r)).chain(undrawn.iter().map(|u| u.0)).collect();
        let others: Vec<FormId> = players.iter().map(|&p| FormId(p)).collect();
        nvf_play::script::with_world(rt, sim, |rt| {
            let g = &mut rt.game;
            for r in refs {
                let id = FormId(r);
                if g.is_dead(id) {
                    continue;
                }
                let fighting = g.awareness.get(id).and_then(|a| a.combat_target()).is_some_and(|(t, _)| t == FormId::PLAYER_REF || others.contains(&t));
                if fighting || g.would_attack(id, FormId::PLAYER_REF) {
                    hostile.insert(r);
                }
                if let Some(w) = g.active_weapon_use(id).and_then(|u| u.weapon) {
                    using.insert(r, w);
                }
            }
        });
    }
    // What decides each one's look: its base, and the level and rolls its leveled picks
    // were drawn with ([`follow_looks`]).
    let rolls = actor_world.as_deref().and_then(|w| w.rolls.lock().ok().map(|r| r.clone())).unwrap_or_default();
    let mut npcs = Vec::new();
    let mut wear: Vec<(u32, Vec<u32>)> = Vec::new();
    // Knocked down and lying here: the guests' copies lie too.
    let knocked = |r: u32| script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.knockout(FormId(r)).is_some_and(|k| k.lying()));
    // A knocked-down body lies and gets up as a `Dying` too (`knockouts.rs`): only the
    // dead are sent as dead, or a guest's copy died with it and was raised again when it
    // stood up, without its get-up.
    let out_cold = |r: u32| script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.knockout(FormId(r)).is_some() && !rt.game.is_dead(FormId(r)));
    for (v, t, lo, hold, dying, seen, on, dressed) in &actors {
        // The dead too, out of sight or not: a guest lays its own body down.
        if (!seen.get() && !dying) || is_puppet(v.0) || players.contains(&v.0) || !feet_placed(t.translation.to_array()) {
            continue;
        }
        let r = v.0;
        let pitch = fighters.pitch(r);
        let mut flags = 0;
        flags |= if lo.is_some_and(|l| l.velocity[0].hypot(l.velocity[1]) > 1.0) { npc_flag::MOVING } else { 0 };
        flags |= if hold.is_some_and(|h| h.drawn) { npc_flag::DRAWN } else { 0 };
        flags |= if lo.is_some_and(|l| l.sneaking) { npc_flag::SNEAKING } else { 0 };
        flags |= if dying && !out_cold(r) { npc_flag::DEAD } else { 0 };
        flags |= if knocked(r) { npc_flag::KNOCKED } else { 0 };
        flags |= if pitch.is_some() { npc_flag::AIMING } else { 0 };
        flags |= if fighters.sights(r) { npc_flag::SIGHTS } else { 0 };
        // Getting off, or asked to while still getting on, is sent as off: a guest starts
        // its own getting-off clip with the host's, not once the host's has ended (then it
        // stood up 3.4 s late and was put 187 units on where the host had walked it
        // meanwhile) [observed: buckallobill, in game, FNV 1.4.0.525].
        // Drawn but not seated yet while it is put on at once: sent as on.
        let (furniture, seated) = sent_furniture(on.map(|f| (f.furniture(), f.seated(), f.leaving())), residents.as_deref().and_then(|res| res.being_put_on(r)));
        flags |= if seated { npc_flag::SEATED } else { 0 };
        // Drawn here further on than last frame than any run goes (a guest
        // put the Dead Horses 153-160 units on with no pause in these states).
        if let Some(was) = sent_at.insert(r, t.translation.to_array()) {
            let far = (t.translation.x - was[0]).hypot(t.translation.y - was[1]);
            if far > NPC_JUMP {
                info!("co-op: {r:08X} drawn here {far:.0} units on from last frame (sent so){}", if furniture != 0 { ", on furniture" } else { "" });
            }
        }
        let level = actor_world.as_deref().map_or(fnv_sim::zones::LevelAt::plain(0), |w| w.level(r));
        npcs.push(NpcState {
            reference: r,
            feet: t.translation.to_array(),
            heading: crate::nav::tr_heading(t),
            pitch: pitch.unwrap_or(0.0),
            flags,
            // Its fighting pose's weapon, else its weapon use's, else the one drawn on it.
            weapon: fighters.weapon(r).or_else(|| using.get(&r).copied()).or_else(|| dressed.and_then(|d| d.weapon).filter(|_| hold.is_some_and(|h| h.drawn))).map_or(0, |w| w.0),
            attacks: coop.npc_attacks.get(&r).copied().unwrap_or(0),
            furniture,
            base: sim.as_deref().and_then(|s| s.base_of(r)).map_or(0, |b| b.0),
            level: level.level,
            all_levels: level.all_levels,
            rolls: rolls.get(&r).copied().unwrap_or(0).min(127) as u8,
            hostile: hostile.contains(&r),
            // Where its AI has it first: a script's MoveTo of a resident moves the walker,
            // and the reference's cell can still name where it was (Jeannie May, moved
            // out of her house at Novac, was sent as inside it and hidden on the guest).
            space: residents
                .as_deref()
                .and_then(|res| res.walker_of(r))
                .map(|(space, ..)| match space {
                    fnv_nav::Space::Interior(c) | fnv_nav::Space::World(c) => c,
                })
                .or_else(|| sim.as_deref().map(|s| space_of_ref(&s.world, FormId(r))))
                .unwrap_or(0),
        });
        // What it is drawn wearing and holding: each game rolls carried items from its
        // own random numbers, so the guests dress their copies in this.
        if let Some(d) = dressed {
            wear.push((r, d.worn.iter().map(|i| i.0).chain(d.weapon.map(|w| w.0)).collect()));
        }
    }
    // The residents the host simulates in a guest's surroundings away from it and does
    // not draw (`areas`): where its AI has them.
    {
        let game = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
        for (r, base, space, feet, heading, walking) in undrawn {
            let dead = game.is_some_and(|g| g.is_dead(FormId(r)));
            let level = actor_world.as_deref().map_or(fnv_sim::zones::LevelAt::plain(0), |w| w.level(r));
            let mut flags = if walking { npc_flag::MOVING } else { 0 };
            flags |= if dead { npc_flag::DEAD } else { 0 };
            flags |= if knocked(r) { npc_flag::KNOCKED } else { 0 };
            flags |= if fighters.weapon(r).is_some() && fighters.pitch(r).is_some() { npc_flag::AIMING } else { 0 };
            // One fighting a guest away from the host (`far_combat`): its weapon out.
            let far = coop.far.fighters.get(&r);
            flags |= if far.is_some_and(|f| f.weapon.is_some()) { npc_flag::AIMING } else { 0 };
            // On the furniture its AI has it on: the crucified at Nipton stay on their
            // crosses on a guest while the host is in the general store (sent
            // as none, they were taken off and put back on at once when it came out).
            let furniture = residents.as_deref().and_then(|res| res.furniture_of(r)).filter(|_| !dead);
            flags |= if furniture.is_some() { npc_flag::SEATED } else { 0 };
            npcs.push(NpcState {
                reference: r,
                feet,
                heading,
                pitch: 0.0,
                flags,
                weapon: fighters.weapon(r).or(far.and_then(|f| f.weapon)).map_or(0, |w| w.0),
                attacks: coop.npc_attacks.get(&r).copied().unwrap_or(0),
                furniture: furniture.unwrap_or(0),
                base: sim.as_deref().and_then(|s| s.base_of(r)).map_or(base, |b| b.0),
                level: level.level,
                all_levels: level.all_levels,
                rolls: rolls.get(&r).copied().unwrap_or(0).min(127) as u8,
                hostile: hostile.contains(&r),
                space: match space {
                    fnv_nav::Space::Interior(c) | fnv_nav::Space::World(c) => c,
                },
            });
        }
    }
    // One the host stopped drawing (gone through a load door, out of the loaded cells)
    // goes on being sent for a while from where its AI has it, so a guest stops drawing
    // it where it was last seen (Benny went through the Tops' elevator door on
    // the host and stood at the elevator on the guest for good).
    let now_secs = time.elapsed_secs();
    let sending: HashSet<u32> = npcs.iter().map(|n| n.reference).collect();
    coop.sent_npcs.retain(|r, since| {
        if sending.contains(r) {
            if since.take().is_some() {
                info!("co-op: {r:08X} drawn here again: its own state goes to the guests");
            }
            return true;
        }
        if since.is_none() {
            info!("co-op: {r:08X} not drawn here: sent from where its AI has it for {NPC_LINGER} s");
        }
        now_secs - *since.get_or_insert(now_secs) < NPC_LINGER
    });
    if let Some(res) = residents.as_deref() {
        for (&r, _) in coop.sent_npcs.iter().filter(|(r, _)| !sending.contains(r)) {
            let Some((space, feet, heading)) = res.walker_of(r) else { continue };
            let dead = script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.is_dead(FormId(r)));
            let level = actor_world.as_deref().map_or(fnv_sim::zones::LevelAt::plain(0), |w| w.level(r));
            // Its furniture as its AI has it, as for the undrawn above.
            let furniture = res.furniture_of(r).filter(|_| !dead);
            npcs.push(NpcState {
                reference: r,
                feet,
                heading,
                pitch: 0.0,
                flags: if dead { npc_flag::DEAD } else if furniture.is_some() { npc_flag::SEATED } else { 0 },
                weapon: 0,
                attacks: coop.npc_attacks.get(&r).copied().unwrap_or(0),
                furniture: furniture.unwrap_or(0),
                base: sim.as_deref().and_then(|s| s.base_of(r)).map_or(0, |b| b.0),
                level: level.level,
                all_levels: level.all_levels,
                rolls: rolls.get(&r).copied().unwrap_or(0).min(127) as u8,
                hostile: false,
                space: match space {
                    fnv_nav::Space::Interior(c) | fnv_nav::Space::World(c) => c,
                },
            });
        }
    }
    for r in sending {
        coop.sent_npcs.insert(r, None);
    }
    // The replication build dresses the guests' NPCs by the change log's Look (the
    // host's worn list, `replication`); two senders could disagree and the NPC would
    // flip between them, so this one stays quiet here.
    drop(wear);
    link.set_wear(Vec::new());
    if npcs.len().abs_diff(*told) >= 5 || (*told == 0) != npcs.is_empty() {
        *told = npcs.len();
        info!("co-op: sending the guests {} NPCs", npcs.len());
    }
    // The replication build sends them on its compact stream (`replication`).
    coop.repl.host_npcs = npcs;
}

/// A guest's copies of the host's NPCs, made and drawn as the host's game made and draws
/// them (module notes): an actor the host's game created (a dynamic reference: Freeside's
/// thugs, PlaceAtMe'd by a script) is made here under the host's number, of the host's
/// base; and each NPC's leveled picks are read at the host's level with the host's rolls
/// (the picks are seeded by the reference, so the same number, level and rolls pick the
/// same: `fnv_actor::resolve`), and drawn again when that changes what is drawn. Before,
/// a guest rolled its own picks at its own player's level (NPCs that looked different)
/// and never drew the host's created actors [observed: buckallobill, in game, FNV 1.4.0.525].
/// Each is also dressed in what the host's game draws it wearing and holding
/// (`net::Msg::Wear`, `fnv_sim::World::dress_as`): carried items, outfits among them, are
/// rolled from each game's own random numbers, not seeded by the reference, so the same
/// NPC came out in another suit even at the same level.
#[allow(clippy::type_complexity)]
fn follow_looks(
    mut commands: Commands,
    mut coop: ResMut<Coop>,
    (mut script, mut sim, actor_world, mut residents): (Option<ResMut<nvf_play::script::ScriptState>>, Option<ResMut<nvf_play::sim::SimState>>, Option<Res<ActorWorld>>, Option<ResMut<Residents>>),
    main_menu: Option<Res<nvf_play::main_menu::MainMenu>>,
    solo: Option<Res<nvf_play::save::SoloBase>>,
) {
    let coop = &mut *coop;
    let guest = coop.link.as_ref().is_some_and(|l| !l.is_host() && l.id().is_some()) && !at_main_menu(main_menu.as_deref()) && solo.is_some();
    if !guest {
        coop.looks_taken.clear();
        coop.numbered = false;
        coop.wear_heard.clear();
        coop.wear_taken.clear();
        return;
    }
    let (Some(script), Some(sim), Some(aw)) = (script.as_deref_mut(), sim.as_deref_mut(), actor_world.as_deref()) else { return };
    let Some(rt) = script.rt.as_mut() else { return };
    if !coop.numbered {
        sim.world.number_created_from(GUEST_CREATED);
        coop.numbered = true;
        info!("co-op: this game's own created references are numbered from {GUEST_CREATED:#010X}, the host's keep theirs");
    }
    let newest: Vec<NpcState> = coop.npcs.values().filter_map(|t| t.samples.back().map(|&(_, s)| s)).collect();
    let cell = sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.cell);
    for s in newest {
        let r = FormId(s.reference);
        let key = (s.level, s.all_levels, s.rolls);
        if s.base == 0 || coop.looks_taken.get(&r.0) == Some(&key) {
            continue;
        }
        let name = rt.game.data.name(FormId(s.base));
        if r.is_dynamic() {
            match sim.world.peek(r).map(|st| st.base) {
                None => {
                    let made = nvf_play::script::with_world(rt, sim, |rt| rt.game.create_as(r, FormId(s.base), cell, s.feet, [0.0, 0.0, s.heading], None));
                    match made {
                        Ok(_) => info!("co-op: {name} {r} made here as the host's game made it"),
                        Err(e) => warn!("co-op: the host's {name} {r} cannot be made here: {e}"),
                    }
                }
                Some(b) if b.0 != s.base => {
                    warn!("co-op: the host's {r} is {name}, this game's own {r} is {}: it is not drawn here", rt.game.data.name(b));
                    continue;
                }
                Some(_) => {}
            }
        }
        // Taken only once it is known here, so one heard before is taken again.
        let Some(base) = sim.base_of(r.0) else { continue };
        coop.looks_taken.insert(r.0, key);
        let drawn = |aw: &ActorWorld| aw.resolve(base.0, r.0).ok().map(|a| (a.npc, a.female, a.race, a.worn));
        let before = drawn(aw);
        let mut changed = false;
        let level = fnv_sim::zones::LevelAt { level: s.level, all_levels: s.all_levels };
        if s.level != 0 && aw.level(r.0) != level {
            // A placed actor's level is the world's (`zones::sync_levels` copies it); a
            // created one is drawn at the display level in both games.
            if !r.is_dynamic() {
                sim.world.set_level(r, level);
            }
            if let Ok(mut l) = aw.levels.lock() {
                l.insert(r.0, level);
            }
            changed = true;
        }
        let rolls = u32::from(s.rolls);
        if aw.rolls.lock().map_or(0, |m| m.get(&r.0).copied().unwrap_or(0)) != rolls {
            if let Ok(mut m) = aw.rolls.lock() {
                m.insert(r.0, rolls);
            }
            rt.game.extra.entry(r).or_default().rolls = rolls;
            changed = true;
        }
        let after = drawn(aw);
        if changed && after != before {
            let again = residents.as_deref_mut().is_some_and(|res| res.redraw(&mut commands, r.0));
            let what = after.as_ref().map_or_else(|| "nothing".to_string(), |a| rt.game.data.name(a.0));
            info!("co-op: {name} {r} takes the host's leveled pick ({what}, level {}, rolls {rolls}){}", s.level, if again { ": drawn again" } else { "" });
        }
    }
    // Dressed as the host's game draws it: the outfits follow the inventory,
    // so it is drawn again in it (`outfits::follow_inventory`).
    let heard: Vec<(u32, Vec<u32>)> = coop.wear_heard.iter().filter(|(r, w)| coop.wear_taken.get(*r) != Some(*w)).map(|(r, w)| (*r, w.clone())).collect();
    for (r, items) in heard {
        let id = FormId(r);
        // An actor the host's game made, not made here yet: later.
        if sim.world.peek(id).is_none() && sim.world.placed.get(id).is_none() {
            continue;
        }
        let ids: Vec<FormId> = items.iter().map(|&i| FormId(i)).collect();
        match sim.world.dress_as(id, &ids) {
            Ok(true) => {
                let worn: Vec<String> = ids.iter().map(|&i| rt.game.data.name(i)).collect();
                info!("co-op: {} {id} dressed as the host's game has it: {}", rt.game.data.name(id), worn.join(", "));
            }
            Ok(false) => {}
            Err(e) => warn!("co-op: {id} cannot be dressed as the host's game has it: {e}"),
        }
        coop.wear_taken.insert(r, items);
    }
}

/// Whether a joining guest's own world logic is held: from the host's welcome
/// (`joining`, `nvf_play::save::KeepSolo`) until the game is first flagged a guest
/// (`seen_guest`; [`follow_npcs`] then turns it off itself), while the link is a guest's.
fn holds_joining_world(joining: bool, seen_guest: bool, link_guest: bool) -> bool {
    joining && !seen_guest && link_guest
}

/// A joining guest runs none of its own world logic from the host's welcome: it is
/// flagged a guest ([`follow_npcs`]) only once its own game was kept
/// (`nvf_play::save::SoloBase`), after the first frame past the loading screen, and that
/// frame ran its own quests [observed: buckallobill, in game, FNV 1.4.0.525: a DLC's
/// start quest moved a joining guest out of its starting cell, so the host's later move
/// left it too far away]. Its triggers are held the same way
/// (`script_trigger::held_for_guest`).
fn hold_joining_world(mut coop: ResMut<Coop>, joining: Option<Res<nvf_play::save::KeepSolo>>, mut script: Option<ResMut<nvf_play::script::ScriptState>>, mut seen_guest: Local<bool>) {
    let Some(g) = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| &mut rt.game) else { return };
    *seen_guest |= g.coop_guest;
    let hold = holds_joining_world(joining.is_some(), *seen_guest, coop.is_guest());
    if hold {
        if !coop.joining_held {
            info!("replication: joining: this game's own world logic is held until the host's world reaches it");
        }
        coop.joining_held = true;
        g.world_logic_off = true;
    } else if coop.joining_held && !g.coop_guest {
        info!("replication: joining ended before the host's world reached this game: its own world logic runs again");
        coop.joining_held = false;
        g.world_logic_off = false;
    }
}

/// A guest's game following the host's world (module notes): its actors' AI off, its
/// hits on them sent to the host, and its NPCs drawn as the host draws them.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn follow_npcs(
    mut commands: Commands,
    time: Res<Time>,
    mut coop: ResMut<Coop>,
    (mut residents, mut stats, mut script): (Option<ResMut<Residents>>, ResMut<CombatStats>, Option<ResMut<nvf_play::script::ScriptState>>),
    mut actors: Query<(Entity, &RefVisual, Option<&mut WeaponHold>, Option<&mut Locomotion>, Has<Remote>, Has<Dying>, &Transform), (With<crate::limbs::ActorRig>, Without<Puppet>)>,
    mut fighters: ResMut<Fighters>,
    actor_world: Option<Res<ActorWorld>>,
    (mut attacks, mut hits, mut fired, combat, mut changed): (MessageWriter<PlayAttack>, MessageWriter<ActorHit>, MessageWriter<nvf_play::combat::ShotFired>, Option<Res<nvf_play::combat::Combat>>, MessageWriter<nvf_play::RefChanged>),
    main_menu: Option<Res<nvf_play::main_menu::MainMenu>>,
    mut told: Local<usize>,
    (sfx, listener, mut sounds, mut sim, loc): (Option<Res<Sfx>>, Option<Res<Player>>, MessageWriter<PlaySound>, Option<ResMut<nvf_play::sim::SimState>>, Option<Res<Location>>),
    solo: Option<Res<nvf_play::save::SoloBase>>,
    mut knocked: Local<HashSet<u32>>,
    mut streamed_since: Local<Option<f32>>,
) {
    let now = time.elapsed_secs();
    let coop = &mut *coop;
    // The host's NPCs reach this game once it is kept as the guest's own (module notes).
    let guest = coop.link.as_ref().is_some_and(|l| !l.is_host() && l.id().is_some()) && !at_main_menu(main_menu.as_deref()) && solo.is_some();
    // A guest on its way out (the host ended the lobby, or it chose the main menu) still
    // stands in the host's world while it autosaves and until the main menu is up: it
    // stays a guest's world, running none of its own world logic, scripts or triggers.
    // Before, the link's end made it its own world at once, and its player and the host's
    // ambushers standing in a trigger volume ran the trigger's quest script there
    // [observed: buckallobill, in game, FNV 1.4.0.525].
    let parting = (coop.ended || !matches!(coop.leaving, leave::Leaving::No) || coop.parting) && !at_main_menu(main_menu.as_deref());
    if !guest && parting && coop.ai_was_off.is_some() {
        return;
    }
    let game = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| &mut rt.game);
    if !guest {
        // Left the lobby: this game's world is its own again.
        if let Some(was) = coop.ai_was_off.take() {
            coop.parting = false;
            stats.relay = None;
            if let Some(r) = residents.as_deref_mut() {
                r.remote = None;
            }
            if let Some(g) = game {
                g.toggles.actors_ai_off = was;
                g.coop_guest = false;
                g.world_logic_off = false;
                g.set_remote_speech(Vec::new(), |_| false);
            }
            // What the host's world hid of this one's is back.
            if let Some(sim) = sim.as_deref_mut() {
                for r in coop.hidden.drain() {
                    let _ = sim.world.set_enabled(r, true);
                }
                for (r, was) in coop.presence_was.drain() {
                    let _ = sim.world.set_enabled(r, was);
                }
            }
            coop.presence_was.clear();
            coop.presence = HostPresence::default();
            coop.unheard.clear();
            coop.host_lines = None;
            for (e, v, _, _, marked, _, _) in &mut actors {
                if marked {
                    commands.entity(e).remove::<Remote>();
                    fighters.set_puppet(v.0, None);
                }
            }
            coop.npcs.clear();
            info!("co-op: this game runs its own NPCs again");
        }
        return;
    }
    let Some(g) = game else { return };
    // Where the host's story put the NPCs it draws here, they are here too.
    let here = loc.as_deref().map(place_of);
    let settled = coop.host_place.filter(|(p, _)| Some(*p) == here).map(|(_, since)| since);
    // The host's disables come before its NPC samples (the Strip crowd the
    // host's scene disabled was enabled here again from the samples sent just before, and
    // 6 of 7 stayed standing): an NPC the host disabled is forgotten here, so the disable
    // below reaches it, and no older sample shows it again until the host enables it.
    for &(r, on) in &coop.presence.fresh {
        if on {
            coop.host_disabled.remove(&r);
        } else {
            coop.host_disabled.insert(r);
            if coop.npcs.remove(&r).is_some() {
                info!("co-op: the host disabled {}: not followed here any more", g.data.name(FormId(r)));
            }
        }
    }
    if let (Some(sim), Some(since)) = (sim.as_deref_mut(), settled) {
        for r in place_like_host(&mut sim.world, g, &coop.npcs, &coop.host_disabled, since + HOST_SETTLE, now) {
            coop.hidden.remove(&r);
            coop.unheard.remove(&r.0);
        }
    }
    // What the host's scripts enabled or disabled, all over its world (module notes); the
    // NPCs the host draws and the actors hidden here are left to the rest.
    if let Some(sim) = sim.as_deref_mut() {
        let (npcs, hidden) = (&coop.npcs, &coop.hidden);
        coop.repl.enabled_heard.retain(|_, at| now - *at < STREAM_WINS);
        let streamed = &coop.repl.enabled_heard;
        let skip = |r: FormId| npcs.contains_key(&r.0) || hidden.contains(&r) || streamed.contains_key(&r.0);
        let fresh = std::mem::take(&mut coop.presence.fresh);
        let mut flipped = follow_presence(&mut sim.world, Flag::Enabled, &fresh, &skip, &mut coop.presence_was);
        if !flipped.is_empty() {
            info!("co-op: {} references enabled or disabled as in the host's world", flipped.len());
        }
        if let Some(host) = coop.presence.whole.take() {
            let own = undo_own_presence(&mut sim.world, Flag::Enabled, &host, &skip, &mut coop.presence_was);
            if !own.is_empty() {
                info!("co-op: {} references this game alone enabled or disabled take the plugins' flags again", own.len());
            }
            flipped.extend(own);
        }
        // Their collision and their enable children's follow, as a script's Enable and
        // Disable here would have them (`nvf_play::RefChanged`): Sunny's bottles, enabled
        // with their marker on the host, were drawn here with no collision body, so a
        // guest's shots went through them to the fence [observed: buckallobill, in game, FNV 1.4.0.525].
        if !flipped.is_empty() {
            for r in with_enable_children(sim.refs, flipped) {
                changed.write(nvf_play::RefChanged::new(r.0));
            }
        }
    }
    // This game's own actors near the host that the host's world does not have: hidden
    // (module notes). Only once the host's NPCs come, and the host has been here a while.
    // Away from the host too (a guest's own save kept the Honest Hearts caravan
    // alive at Northern Passage, dead in Zion in the host's world): round this player,
    // once the host has streamed its actors of this place near here for a while (the
    // undrawn residents round away guests, `share_npcs`), so the host simulates this area.
    let mine = listener.as_deref().map(|p| p.feet);
    let my_space = here.map_or(0, |p| match p {
        Place::Interior(c) | Place::Exterior(c) => c,
    });
    let streamed_here = mine.is_some_and(|me| {
        my_space != 0 && coop.npcs.values().filter_map(|t| t.samples.back()).any(|(_, n)| n.space == my_space && (n.feet[0] - me[0]).hypot(n.feet[1] - me[1]) <= HIDE_REACH)
    });
    let away = settled.is_none() && streamed_here;
    if away {
        streamed_since.get_or_insert(now);
    } else {
        *streamed_since = None;
    }
    let centre = match (settled.filter(|s| now - s >= HIDE_AFTER), coop.host, *streamed_since, mine) {
        (Some(_), Some(host), _, _) => Some(host.feet),
        (None, _, Some(since), Some(me)) if now - since >= HIDE_AFTER => Some(me),
        _ => None,
    };
    match (sim.as_deref_mut(), centre) {
        (Some(sim), Some(centre)) if !coop.npcs.is_empty() => {
            let drawn: Vec<(u32, [f32; 3])> = actors.iter().map(|(_, v, _, _, _, _, t)| (v.0, t.translation.to_array())).collect();
            let heard: HashSet<u32> = coop.npcs.keys().copied().collect();
            let missing = not_in_hosts_world(&drawn, &heard, centre, HIDE_REACH);
            coop.unheard.retain(|r, _| missing.contains(r));
            for r in missing {
                let since = *coop.unheard.entry(r).or_insert(now);
                if now - since < HIDE_AFTER || !coop.hidden.insert(FormId(r)) {
                    continue;
                }
                info!("co-op: {} is not in the host's world here: hidden{}", g.data.name(FormId(r)), if away { " (away from the host)" } else { "" });
                let _ = sim.world.set_enabled(FormId(r), false);
            }
        }
        _ => coop.unheard.clear(),
    }
    // The lines the host's NPCs are saying, as long as each lasts.
    let me = coop.link.as_ref().and_then(|l| l.id());
    let lines = coop.host_lines.as_ref().map_or_else(Vec::new, |(at, lines)| lines_in(lines, now - at, me));
    let npcs = &coop.npcs;
    g.set_remote_speech(lines, |r| npcs.contains_key(&r.0));
    if coop.ai_was_off.is_none() {
        coop.ai_was_off = Some(g.toggles.actors_ai_off);
        stats.relay = Some(Vec::new());
        info!("co-op: the host runs the world: this game's NPCs follow the host's");
    }
    // Kept off (a console `tai` would turn it back on).
    g.toggles.actors_ai_off = true;
    g.coop_guest = true;
    // Replication (phase 4): the host runs the world's logic, this game draws it (held
    // already while joining, [`hold_joining_world`]).
    if !g.world_logic_off || std::mem::take(&mut coop.joining_held) {
        g.world_logic_off = true;
        info!("replication: this guest runs no world logic; the host's world is drawn");
    }
    if let (Some(out), Some(link)) = (stats.relay.as_mut(), coop.link.as_ref()) {
        for h in out.drain(..) {
            info!("co-op: the host gets this game's hit on {} for {:.1}", h.target, h.dealt);
            link.send_hit(hit_view(&h));
        }
    }
    coop.npcs.retain(|_, t| t.samples.back().is_some_and(|(at, _)| now - at < NPC_FORGET));
    let dt = time.delta_secs();
    let poses: HashMap<u32, RemotePose> = coop
        .npcs
        .iter_mut()
        .filter_map(|(&r, t)| {
            // Its furniture as the host has it now (a chair, a bed, a cross: module notes).
            // Not a dead one's: a shopkeeper killed on the host sat down on his chair again
            // on a guest [observed: buckallobill, in game, FNV 1.4.0.525].
            let heard = t.samples.back().map(|(_, s)| s).filter(|s| s.furniture != 0 && !s.has(npc_flag::DEAD)).map(|s| (s.furniture, s.has(npc_flag::SEATED)));
            // A new host's first states (after a load) keep it on the furniture it is on
            // here until it says so too.
            let dead = t.samples.back().is_some_and(|(_, s)| s.has(npc_flag::DEAD));
            let young = !dead && t.first_heard.is_some_and(|f| now - f < NPC_FIRST_HOLD);
            let furniture = furniture_here(heard, young, residents.as_deref().and_then(|res| res.furniture_of(r)));
            let space = t.samples.back().map_or(0, |(_, s)| s.space);
            // Faster than it runs (with room for a run's speed-ups) is a jump: enables and
            // first states were blended from where they were placed at 1165-1399 units/s
            // otherwise [observed: buckallobill, in game, FNV 1.4.0.525].
            let top = residents.as_deref().and_then(|res| res.top_speed(r)).map_or(NPC_JUMP_SPEED, |run| (run * NPC_RUN_ROOM).min(NPC_JUMP_SPEED));
            t.delay = eased_delay(t.delay, t.spacing(), dt);
            let pose = t.pose(now - t.delay, top);
            if let (true, Some((feet, _, moving))) = (traced(r), pose) {
                info!("co-op trace: {r:08X} drawn at {now:.3} for {:.3} (top {top:.0}): ({:.0}, {:.0}, {:.0}) moving {moving}", now - t.delay, feet[0], feet[1], feet[2]);
            }
            pose.map(|(feet, heading, moving)| (r, RemotePose { feet, heading, moving, furniture, space }))
        })
        .collect();
    if poses.len().abs_diff(*told) >= 5 || (*told == 0) != poses.is_empty() {
        *told = poses.len();
        let mut some: Vec<String> = poses.keys().take(8).map(|r| format!("{r:08X}")).collect();
        some.sort();
        info!("co-op: following {} of the host's NPCs ({}...)", poses.len(), some.join(", "));
    }
    if let Some(r) = residents.as_deref_mut() {
        r.remote = Some(poses);
    }
    let mut bodies = HashSet::new();
    for (e, v, hold, lo, marked, dying, _) in &mut actors {
        let r = v.0;
        let Some(t) = coop.npcs.get_mut(&r) else {
            if marked {
                commands.entity(e).remove::<Remote>();
                fighters.set_puppet(r, None);
            }
            continue;
        };
        let Some(&(_, s)) = t.samples.back() else { continue };
        if !marked {
            commands.entity(e).insert(Remote);
        }
        if !dying {
            bodies.insert(r);
        }
        let drawn = s.has(npc_flag::DRAWN);
        if let Some(mut h) = hold {
            if h.drawn != drawn {
                h.drawn = drawn;
            }
        }
        if let Some(mut lo) = lo {
            lo.set_sneaking(s.has(npc_flag::SNEAKING));
            // The host's NPC sets off when its samples say so, with no warning here: its
            // walk starts at once, fading from a standing fade, as a creature's does
            // (Vulpes at Nipton slid at 69 units/s on Idle as his walk-up
            // began on the guest) [design].
            if !lo.blend.set_off_at_once {
                lo.blend.set_off_at_once = true;
            }
        }
        if s.has(npc_flag::DEAD) {
            fighters.set_puppet(r, None);
            continue;
        }
        let attacked = t.played.is_some_and(|a| a != s.attacks);
        t.played = Some(s.attacks);
        if attacked {
            t.attacked_at = Some(now);
        }
        let weapon = (s.weapon != 0).then_some(FormId(s.weapon));
        let sights = s.has(npc_flag::SIGHTS);
        let fighting = drawn && (s.has(npc_flag::AIMING) || attacked || t.attacked_at.is_some_and(|at| now - at < AIM_AFTER_ATTACK));
        fighters.set_puppet(r, fighting.then_some(PuppetPose { pitch: s.pitch, weapon, sights, crouched: s.has(npc_flag::SNEAKING) }));
        if fighting && attacked {
            let melee = is_melee(actor_world.as_deref(), weapon);
            attacks.write(PlayAttack { actor: r, weapon, melee, sights: sights && !melee, kind: Move::Attack, power: None, swing: melee.then(SwingPick::default) });
            // Its shot, heard as the host's NPC's is there (no shot is fired here).
            if let Some(snd) = weapon.filter(|_| !melee).and_then(|w| shot_sound(sfx.as_deref(), listener.as_deref(), w, s.feet)) {
                sounds.write(snd);
            }
            // And its muzzle flash at its gun's muzzle (`shot_fx`: Boone's shot
            // at Jeannie May showed nothing on the guest). Only the flash: no shot is
            // fired here, the host's hit comes as the host deals it.
            let projectile = weapon.filter(|_| !melee).and_then(|w| combat.as_deref()?.forms.chain(w, None)?.projectile);
            if let (Some(w), Some(projectile)) = (weapon, projectile) {
                let eye = [s.feet[0], s.feet[1], s.feet[2] + 100.0];
                let (sy, cy) = s.heading.sin_cos();
                let (sp, cp) = s.pitch.sin_cos();
                fired.write(nvf_play::combat::ShotFired { projectile, weapon: w, shooter: FormId(r), at: Some(eye), dir: Some([sy * cp, cy * cp, -sp]), mods: 0 });
            }
        }
    }
    // Deaths: dead here too; a body drawn now falls as the host's did (`deaths`).
    for (&r, t) in &coop.npcs {
        let Some(&(_, s)) = t.samples.back() else { continue };
        if !s.has(npc_flag::DEAD) || g.is_dead(FormId(r)) {
            continue;
        }
        g.extra.entry(FormId(r)).or_default().dead = true;
        if bodies.contains(&r) {
            hits.write(ActorHit { form_id: r, died: true, knocked_out: false, location: -1, crippled: false, part_crippled: false, torso_crippled: false, by: None, share: 1.0, guard_broken: false, weapon: None });
        }
    }
    // Knockdowns: lying here while the host's lies, held down (this
    // game's own clock would get it up after its time), and getting up when the host's
    // does; `knockouts.rs` draws the fall and the get-up from the game's knockout.
    knocked.retain(|r| coop.npcs.contains_key(r));
    for (&r, t) in &coop.npcs {
        let Some(&(_, s)) = t.samples.back() else { continue };
        let id = FormId(r);
        if g.is_dead(id) {
            knocked.remove(&r);
            continue;
        }
        let lying = g.knockout(id).is_some_and(|k| k.lying());
        if s.has(npc_flag::KNOCKED) {
            if !lying {
                let at = g.time;
                let e = g.extra.entry(id).or_default();
                e.knocked_out = Some(fnv_game::unconscious::KnockOut { cause: fnv_game::unconscious::Cause::KnockedDown, at, facing_up: None, held: false, rising: None, pushed: false, shove: None });
                e.unconscious = true;
                knocked.insert(r);
                info!("co-op: {} is knocked down in the host's world: down here too", g.data.name(id));
            }
        } else if knocked.remove(&r) {
            let at = g.time;
            let e = g.extra.entry(id).or_default();
            e.unconscious = false;
            if let Some(k) = e.knocked_out.as_mut().filter(|k| k.lying()) {
                k.rising = Some(at);
                info!("co-op: {} gets up in the host's world: up here too", g.data.name(id));
            }
        }
    }
}


/// A shot of `weapon` fired by someone standing at `feet`, heard here: its 3D shoot sound,
/// or its distant one beyond the 3D sound's reach, as `nvf_play::combat` plays an NPC's
/// (a puppet's gun mods are not known: the plain sound).
fn shot_sound(sfx: Option<&Sfx>, listener: Option<&Player>, weapon: FormId, feet: [f32; 3]) -> Option<PlaySound> {
    let sfx = sfx?;
    let shoot = sfx.forms.weapons.get(&weapon)?.shoot_sounds(false);
    let near_max = shoot.near.and_then(|n| sfx.forms.sounds.get(&n)).map_or(0.0, |n| n.max_distance);
    let at = [feet[0], feet[1], feet[2] + GUN_HEIGHT];
    let distance = listener.map_or(0.0, |p| {
        let e = p.eye();
        ((e[0] - at[0]).powi(2) + (e[1] - at[1]).powi(2) + (e[2] - at[2]).powi(2)).sqrt()
    });
    let (sound, _) = shoot.heard_at(distance, near_max);
    Some(PlaySound { sound: sound?, at: Some(at) })
}

/// A guest's hours passed with the host's clock: those it went `ahead` (from
/// [`take_clock`]) once they are at least [`CLOCK_JUMP`] and the clock was heard steadily
/// before (`steady`: not the first clock of a join, whose difference is the guest's own
/// save's time), at most [`CLOCK_JUMP_MAX`]. The host's wait, sleep or fast travel moves
/// the clock, and the hours count for every player's effects and Hardcore needs, each
/// on its own character: only the one who slept recovers from sleep (before, a guest's needs never saw the host's hours, and the host's 8 hours slept left a guest's needs where they were [observed: buckallobill, in game, FNV 1.4.0.525])
/// [design: the original has one player; its waits raise the needs hour by hour,
/// `fnv_game::needs`]. A guest's own wait is pulled back by the host's clock and counts
/// once, when it waits (`nvf_play::wait`).
fn guest_hours_passed(ahead: f64, steady: bool) -> Option<f64> {
    (steady && ahead >= CLOCK_JUMP).then_some(ahead.min(CLOCK_JUMP_MAX))
}

/// A forward jump of the host's clock this big (game hours) is time passed for a guest:
/// a quarter hour, 30 s of play at TimeScale 30, well above the drift [`CLOCK_SLACK`]
/// corrects.
const CLOCK_JUMP: f64 = 0.25;
/// At most this much at once (game hours): three days, a long fast travel's worth.
const CLOCK_JUMP_MAX: f64 = 72.0;
/// The host's clock comes this often or more (seconds); a gap longer than this, a
/// guest's next clock is a fresh start, not hours passed.
const CLOCK_STEADY: f32 = 10.0;

/// A guest takes the host's clock when they differ by more than [`CLOCK_SLACK`]. The game
/// hours the guest's clock went forward (negative: back; 0: kept).
fn take_clock(world: &mut fnv_sim::World, c: net::ClockView) -> f64 {
    let Ok(own) = world.calendar() else { return 0.0 };
    // By the date and hour, not GameDaysPassed alone: a script (or the console) can set
    // GameHour without it.
    let same_day = (own.year, own.month, own.day) == (c.year, c.month, c.day);
    let off = if same_day { c.hour - own.hour } else { (c.days_passed - own.days_passed) * 24.0 };
    if same_day && off.abs() <= CLOCK_SLACK && (c.days_passed - own.days_passed).abs() * 24.0 <= CLOCK_SLACK {
        return 0.0;
    }
    if off.abs() >= 0.5 {
        info!("co-op: the host's clock: day {} {:02}:{:02} (this game was {off:+.1} h off)", c.day, c.hour as u32, (c.hour.fract() * 60.0) as u32);
    }
    let cal = fnv_sim::clock::Calendar { year: c.year, month: c.month, day: c.day, hour: c.hour, days_passed: c.days_passed };
    if let Err(e) = world.set_calendar(cal) {
        warn_once!("co-op: cannot take the host's clock: {e}");
        return 0.0;
    }
    f64::from(off)
}

/// A host makes its presence list again at least this often (seconds).
const PRESENCE_REMAKE: f32 = 2.0;
/// A reference's enabled flag the replication stream set is the host's newest for this
/// long (seconds): the presence list, made up to [`PRESENCE_REMAKE`] before and paged at
/// `net::PRESENCE_EVERY`, does not change it in that time.
const STREAM_WINS: f32 = 3.0 * PRESENCE_REMAKE;

/// A guest's view of the host's world's presence (`net::Msg::Presence`), page by page.
#[derive(Default)]
struct HostPresence {
    serial: u32,
    pages: u16,
    got: HashSet<u16>,
    refs: HashSet<u32>,
    /// Flags heard and not yet applied.
    fresh: Vec<(u32, bool)>,
    /// The whole list (every page of one serial), once, for [`undo_own_presence`].
    whole: Option<HashSet<u32>>,
    /// A whole list has come since this was made (the syncing screen waits for one).
    complete: bool,
}

impl HostPresence {
    fn hear(&mut self, serial: u32, page: u16, pages: u16, refs: Vec<(u32, bool)>) {
        if serial != self.serial || pages != self.pages {
            (self.serial, self.pages) = (serial, pages);
            self.got.clear();
            self.refs.clear();
        }
        self.refs.extend(refs.iter().map(|r| r.0));
        self.fresh.extend(refs);
        if self.got.insert(page) && self.got.len() == pages as usize {
            self.complete = true;
            self.whole = Some(self.refs.clone());
        }
    }
}

/// The flag a host's list carries (`net::LIST_ENABLED`, `net::LIST_OPEN`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flag {
    /// A reference's own enabled flag (its enable parent's is resolved apart).
    Enabled,
    /// A door's open flag (a door that opens in place: a load door has none).
    Open,
}

impl Flag {
    /// A placed reference's flag as its plugin gives it; `None` for what takes no part
    /// in the list (the player, references made at run time, unknown ones, load doors).
    fn authored(self, world: &fnv_sim::World, r: FormId) -> Option<bool> {
        if r == FormId::PLAYER_REF || r.0 >= 0xFF00_0000 {
            return None;
        }
        let p = world.placed.get(r)?;
        match self {
            Flag::Enabled => Some(!p.initially_disabled()),
            Flag::Open => p.teleport.is_none().then_some(p.open_by_default),
        }
    }

    fn of(self, st: &fnv_sim::world::RefState) -> bool {
        match self {
            Flag::Enabled => st.enabled(),
            Flag::Open => st.open,
        }
    }

    fn set(self, world: &mut fnv_sim::World, r: FormId, on: bool) {
        match self {
            Flag::Enabled => {
                let _ = world.set_enabled(r, on);
            }
            Flag::Open => {
                if let Ok(st) = world.get(r) {
                    st.open = on;
                }
            }
        }
    }
}

/// A host's list (module notes): each placed reference whose `flag` differs from its
/// plugin's, with the flag, in FormID order.
fn presence_of(world: &fnv_sim::World, flag: Flag) -> Vec<(u32, bool)> {
    world.changed().filter(|(r, st)| flag.authored(world, *r).is_some_and(|a| a != flag.of(st))).map(|(r, st)| (r.0, flag.of(st))).collect()
}

/// `refs` and every reference that follows one of them as its enable parent, or follows
/// one of those in turn: what an Enable or Disable of `refs` changes.
/// The parents' children are read once from the game's table (it is the same all run);
/// a test's own table is read each time.
fn with_enable_children(placed: &fnv_sim::refs::RefTable, refs: Vec<FormId>) -> Vec<FormId> {
    fn children_of(placed: &fnv_sim::refs::RefTable) -> HashMap<FormId, Vec<FormId>> {
        let mut children: HashMap<FormId, Vec<FormId>> = HashMap::new();
        for (id, p) in &placed.refs {
            if let Some(ep) = p.enable_parent {
                children.entry(ep.reference).or_default().push(*id);
            }
        }
        children
    }
    static GAME: std::sync::OnceLock<(usize, HashMap<FormId, Vec<FormId>>)> = std::sync::OnceLock::new();
    let key = placed as *const _ as usize;
    let mine;
    let children = match GAME.get_or_init(|| (key, children_of(placed))) {
        (k, c) if *k == key => c,
        _ => {
            mine = children_of(placed);
            &mine
        }
    };
    let mut seen: HashSet<FormId> = refs.iter().copied().collect();
    let mut out = refs.clone();
    let mut todo = refs;
    while let Some(p) = todo.pop() {
        for &c in children.get(&p).map_or(&[][..], |v| v.as_slice()) {
            if seen.insert(c) {
                out.push(c);
                todo.push(c);
            }
        }
    }
    out
}

/// A guest takes the host's flags (`refs`), except for what `skip` holds; what it changes
/// is kept in `was` as it was first. Returns what it changed.
fn follow_presence(world: &mut fnv_sim::World, flag: Flag, refs: &[(u32, bool)], skip: &dyn Fn(FormId) -> bool, was: &mut HashMap<FormId, bool>) -> Vec<FormId> {
    let mut changed = Vec::new();
    for &(r, on) in refs {
        let r = FormId(r);
        if skip(r) || flag.authored(world, r).is_none() {
            continue;
        }
        let Ok(st) = world.get(r) else { continue };
        let now = flag.of(st);
        if now != on {
            was.entry(r).or_insert(now);
            flag.set(world, r, on);
            changed.push(r);
        }
    }
    changed
}

/// A guest with the host's whole list (`host`): what its own game alone changed (not on
/// the list, not in `skip`) takes its plugin's flag again, kept in `was` as it was first.
/// Returns what it changed.
fn undo_own_presence(world: &mut fnv_sim::World, flag: Flag, host: &HashSet<u32>, skip: &dyn Fn(FormId) -> bool, was: &mut HashMap<FormId, bool>) -> Vec<FormId> {
    let own: Vec<(FormId, bool)> = world
        .changed()
        .filter(|(r, _)| !host.contains(&r.0) && !skip(*r))
        .filter_map(|(r, st)| flag.authored(world, r).filter(|a| *a != flag.of(st)).map(|a| (r, a)))
        .collect();
    for &(r, authored) in &own {
        was.entry(r).or_insert(!authored);
        flag.set(world, r, authored);
    }
    own.into_iter().map(|(r, _)| r).collect()
}

/// A host makes its doors' list again this often (seconds); the doors loaded here go at
/// once when they move.
const DOORS_REMAKE: f32 = 0.5;
/// A door this player used is its own this long (seconds): what this game does to it is
/// sent, and the host's list does not undo it before the host has heard.
const DOOR_HELD: f32 = 2.0;

/// A door's open flag here: its state's, or as its plugin places it.
fn door_open(world: &fnv_sim::World, r: FormId) -> Option<bool> {
    world.peek(r).map(|s| s.open).or_else(|| Flag::Open.authored(world, r))
}

/// Doors in co-op (module notes): what others opened or closed is done here; a door
/// loaded here that moved goes to everyone (a guest: only one its player used lately); the
/// host makes its list, a guest follows it.
fn share_doors(
    mut coop: ResMut<Coop>,
    time: Res<Time>,
    doors: Option<Res<nvf_play::doors::Doors>>,
    sim: Option<ResMut<nvf_play::sim::SimState>>,
    mut used: MessageReader<nvf_play::world_sounds::Activated>,
) {
    let now = time.elapsed_secs();
    let coop = &mut *coop;
    let (Some(link), Some(mut sim)) = (coop.link.as_ref(), sim) else {
        used.clear();
        if coop.link.is_none() && !coop.doors_seen.is_empty() {
            coop.doors_seen.clear();
            coop.door_used.clear();
            coop.doors_in.clear();
            coop.doors_heard = HostPresence::default();
            coop.doors_was.clear();
            coop.doors_made = None;
        }
        return;
    };
    let is_host = link.is_host();
    for u in used.read() {
        coop.door_used.insert(u.reference, now);
    }
    coop.door_used.retain(|_, at| now - *at < DOOR_HELD);
    for (id, r, open) in coop.doors_in.drain(..) {
        let r = FormId(r);
        if door_open(&sim.world, r).is_some_and(|was| was != open) {
            if !is_host {
                coop.doors_was.entry(r).or_insert(!open);
            }
            Flag::Open.set(&mut sim.world, r, open);
            info!("co-op: {} {} door {:#010X}", if id == 0 { "the host".to_string() } else { label(id, &coop.peers.get(&id).map_or(String::new(), |p| p.name.clone())) }, if open { "opened" } else { "closed" }, r.0);
        }
        coop.doors_seen.insert(r.0, open);
    }
    if is_host {
        if coop.doors_made.is_none_or(|at| now - at >= DOORS_REMAKE) {
            coop.doors_made = Some(now);
            // The replication build's change log carries the open flags (`replication`).
        }
    } else {
        let used = &coop.door_used;
        let skip = |r: FormId| used.contains_key(&r.0);
        let fresh = std::mem::take(&mut coop.doors_heard.fresh);
        let mut n = follow_presence(&mut sim.world, Flag::Open, &fresh, &skip, &mut coop.doors_was).len();
        if let Some(host) = coop.doors_heard.whole.take() {
            n += undo_own_presence(&mut sim.world, Flag::Open, &host, &skip, &mut coop.doors_was).len();
        }
        if n > 0 {
            info!("co-op: {n} doors opened or closed as in the host's world");
        }
    }
    // The doors loaded here that moved: this game's own doing (what came from the others
    // is noted as seen above, and what the host's list did is not this player's).
    let Some(doors) = doors else { return };
    for id in doors.ids() {
        let Some(open) = door_open(&sim.world, FormId(id)) else { continue };
        let moved = coop.doors_seen.insert(id, open).is_some_and(|was| was != open);
        if moved && (is_host || coop.door_used.contains_key(&id)) {
            info!("co-op: this {} {} door {id:#010X}: everyone's does", if is_host { "host's world" } else { "player" }, if open { "opened" } else { "closed" });
            link.send_door(id, open);
        }
    }
}

/// Whether an actor's feet are a place: a body just drawn is at the origin until it is
/// put where it stands, and a sample of it then would put the guest's copy there (NPCs
/// revived on a guest stood at (0, 0, 0), enabled on the host that frame) [observed: buckallobill, in game, FNV 1.4.0.525]
/// [hypothesis: no actor stands at the exact origin].
fn feet_placed(feet: [f32; 3]) -> bool {
    feet != [0.0; 3]
}

/// A guest in the host's place: each NPC the host draws, enabled here and put in this
/// cell when the host's story enabled it or moved it (module notes). Only states heard
/// after `after` and lately: the ones before are the place the host left (the host's NPCs
/// there are forgotten only after [`NPC_FORGET`]). Actors the host's scripts made (`FF`
/// references) are this game's own numbers here: left alone. One dead in this game but
/// alive in the host's stands up again (`Game::resurrect`). Returns those it enabled.
fn place_like_host(world: &mut fnv_sim::World, g: &mut fnv_game::Game, npcs: &HashMap<u32, NpcTrack>, host_disabled: &HashSet<u32>, after: f32, now: f32) -> Vec<FormId> {
    let mut enabled = Vec::new();
    let Some(cell) = world.peek(FormId::PLAYER_REF).and_then(|s| s.cell) else { return enabled };
    let player_space = world.placed.world_of(cell);
    for (&r, t) in npcs {
        let Some(&(at, s)) = t.samples.back() else { continue };
        if at < after || now - at > 1.0 || r >= 0xFF00_0000 || s.has(npc_flag::DEAD) || host_disabled.contains(&r) || !feet_placed(s.feet) {
            continue;
        }
        let id = FormId(r);
        let Ok(st) = world.get(id) else { continue };
        if st.deleted() {
            continue;
        }
        if g.is_dead(id) {
            let was = world.peek(id).map(|st| st.pos);
            info!(
                "co-op: {} is alive in the host's world: up again here (the host's sample: feet ({:.0}, {:.0}, {:.0}), space {:08X}; here {})",
                g.data.name(id),
                s.feet[0],
                s.feet[1],
                s.feet[2],
                s.space,
                was.map_or_else(|| "no state".to_string(), |p| format!("at ({:.0}, {:.0}, {:.0})", p[0], p[1], p[2]))
            );
            g.coop_revive(id);
            // It stands up where the host has it: its place here may be the corpse's own,
            // or none at all (NPCs dead in the guest's save were revived standing at
            // (0, 0, 0)) [observed: buckallobill, in game, FNV 1.4.0.525]. In this player's space only;
            // another space is left to the rest below.
            let my_space = player_space.map_or(cell.0, |w| w.0);
            if s.space == 0 || s.space == my_space {
                if let Ok(st) = world.get(id) {
                    st.pos = s.feet;
                    st.rot = [0.0, 0.0, s.heading];
                }
            }
        }
        // Shown already, by its own flag or its enable parent's: left as it is. One
        // shown through its enable parent keeps its own flag as the host's does (the
        // parent's change brings it): setting the flag here made every digest find it
        // unlike the host's, which disabled it again and the next sample enabled it
        // (a raid's actors, children of its markers, were sent again every 6-9 s) [observed: buckallobill, in game, FNV 1.4.0.525].
        let shown = world.is_enabled(id).unwrap_or(false);
        let by_parent = world.placed.get(id).is_some_and(|p| p.enable_parent.is_some());
        let enable = !shown && !by_parent;
        let Ok(st_cell) = world.get(id).map(|st| st.cell) else { continue };
        // Where the host has it (`NpcState::space`, protocol 129): another interior than
        // this player's is that interior; this player's space (or none told) is the cell
        // here at its feet. One in another worldspace is left to the drawing
        // (`residents::Core::follow`). Putting every one of them in this player's cell
        // fought the drawing every frame: an NPC gone indoors on the host was put back in
        // the street and taken in again 120 times a second while the guest stood outside
        // [observed: buckallobill, in game, FNV 1.4.0.525].
        let my_space = player_space.map_or(cell.0, |w| w.0);
        let target = if s.space == 0 || s.space == my_space {
            world.placed.exterior_cell_at(cell, s.feet).unwrap_or(cell)
        } else if world.placed.world_grid.contains_key(&FormId(s.space)) {
            if enable {
                let _ = world.set_enabled(id, true);
                enabled.push(id);
                info!("co-op: {id} enabled as in the host's world");
            }
            continue;
        } else {
            FormId(s.space)
        };
        // Elsewhere: another interior, or another worldspace, than where it belongs.
        let elsewhere = st_cell.is_none_or(|c| c != target && ((s.space != 0 && s.space != my_space) || player_space.is_none() || world.placed.world_of(c) != player_space));
        if elsewhere {
            let to = target;
            if let Ok(st) = world.get(id) {
                st.cell = Some(to);
                st.pos = s.feet;
                st.rot = [0.0, 0.0, s.heading];
            }
            g.teleported.insert(id);
        }
        if enable {
            let _ = world.set_enabled(id, true);
            enabled.push(id);
        }
        if enable || elsewhere {
            info!("co-op: {id} {} as in the host's world", match (enable, elsewhere) { (true, true) => "enabled and brought here", (true, false) => "enabled", _ => "brought here" });
        }
    }
    enabled
}

/// Despawn the bodies (actor roots under the world root) of the references in `keep`,
/// except the one each keeps; the bodies despawned.
fn despawn_bodies(commands: &mut Commands, roots: &Query<(Entity, &RefVisual, &ChildOf)>, world_root: Entity, keep: &HashMap<u32, Option<Entity>>) -> HashSet<Entity> {
    let mut gone = HashSet::new();
    if keep.is_empty() {
        return gone;
    }
    for (e, v, parent) in roots {
        if parent.parent() != world_root {
            continue;
        }
        if let Some(k) = keep.get(&v.0).filter(|k| **k != Some(e)) {
            info!("co-op: a body of {:08X} taken away{}", v.0, if k.is_some() { " (a stray: not its puppet)" } else { "" });
            commands.entity(e).try_despawn();
            gone.insert(e);
        }
    }
    gone
}

/// A guest's own actors drawn within `reach` of the host's feet (across the floor) that
/// the host's NPCs (`heard`) do not include: not in the host's world there (module
/// notes). The other players' puppets are not actors of either world.
fn not_in_hosts_world(drawn: &[(u32, [f32; 3])], heard: &HashSet<u32>, host: [f32; 3], reach: f32) -> HashSet<u32> {
    drawn
        .iter()
        .filter(|(r, feet)| !is_puppet(*r) && !heard.contains(r) && (feet[0] - host[0]).hypot(feet[1] - host[1]) <= reach)
        .map(|(r, _)| *r)
        .collect()
}

/// The line the NPC is saying in the host's own conversation, as a line said to the host
/// (the guests hear it from where the NPC stands): the time left is that of the response
/// it is at and of those after it, so a guest's game finds the same response.
fn conversation_line(g: &fnv_game::Game) -> Option<fnv_game::game::Speech> {
    let c = g.dialogue.as_ref()?;
    let line = c.line.as_ref()?;
    let order = g.data.infos.get(&line.info)?.response_order();
    let lengths = g.response_lengths(c.actor, line.info);
    let after: f64 = order.iter().skip(line.response + 1).map(|&r| lengths.get(r).copied().unwrap_or(0.0)).sum();
    let mut s = lines_in(&[net::LineView { speaker: c.actor.0, target: 0, topic: line.topic.0, info: line.info.0, remaining: 1.0, flags: 0, conversation: 0 }], 0.0, None).pop()?;
    s.target = Some(FormId::PLAYER_REF);
    s.remaining = line.remaining.max(0.0) + after;
    s.remote = false;
    Some(s)
}

/// The host's lines as they go to the guests: its player as [`net::LineView::HOST`], a
/// guest's actor (`player`) as that guest; a line by an actor the host's scripts made
/// (a reference no other game has) is not sent, nor a listener of that kind.
fn lines_out(speech: &[fnv_game::game::Speech], player: impl Fn(FormId) -> Option<u8>) -> Vec<net::LineView> {
    let map = |r: FormId| match (r, player(r)) {
        (FormId::PLAYER_REF, _) => Some(net::LineView::HOST),
        (_, Some(id)) => Some(net::LineView::guest(id)),
        (r, None) if r.0 >= 0xFF00_0000 => None,
        (r, None) => Some(r.0),
    };
    use net::LineView as L;
    speech
        .iter()
        .filter(|s| !s.remote)
        // A player's voice (its set's lines and its own grunts) goes to the others by the
        // voice table (`nvf_play::party_quips`), from the speaker's own game, once: a line
        // here with a player as speaker (the host's, or a guest's stand-in here) would be
        // heard twice or from the wrong body.
        .filter(|s| s.speaker != FormId::PLAYER_REF && player(s.speaker).is_none())
        .filter_map(|s| {
            let speaker = map(s.speaker).filter(|r| !is_puppet(*r))?;
            let mut flags = 0;
            for (on, f) in [(s.say_to, L::SAY_TO), (s.greeting, L::GREETING), (s.hello, L::HELLO), (s.force_subtitles, L::FORCE_SUBTITLES), (s.two_d, L::TWO_D)] {
                flags |= if on { f } else { 0 };
            }
            Some(L { speaker, target: s.target.and_then(map).unwrap_or(0), topic: s.topic.0, info: s.info.0, remaining: s.remaining as f32, flags, conversation: s.conversation.unwrap_or(0).min(u16::MAX as u32) as u16 })
        })
        .take(net::LINES_MAX)
        .collect()
}

/// The host's lines as a guest says them, `ago` seconds after they came: each with the
/// time it has left (those over are gone), this player (`me`) for its own id, the
/// others' puppets for theirs.
fn lines_in(lines: &[net::LineView], ago: f32, me: Option<u8>) -> Vec<fnv_game::game::Speech> {
    use net::LineView as L;
    let map = |r: u32| if me.is_some_and(|m| r == L::guest(m)) { FormId::PLAYER_REF } else { FormId(r) };
    lines
        .iter()
        .filter(|l| l.remaining - ago > 0.0)
        .map(|l| fnv_game::game::Speech {
            speaker: map(l.speaker),
            target: (l.target != 0).then(|| map(l.target)),
            topic: FormId(l.topic),
            info: FormId(l.info),
            remaining: (l.remaining - ago) as f64,
            say_to: l.has(L::SAY_TO),
            greeting: l.has(L::GREETING),
            farewell: false,
            conversation: (l.conversation != 0).then_some(l.conversation as u32),
            earlier_topics: Vec::new(),
            hello: l.has(L::HELLO),
            look: fnv_game::game::LineLook::Both,
            response: 0,
            force_subtitles: l.has(L::FORCE_SUBTITLES),
            two_d: l.has(L::TWO_D),
            end_ran: false,
            remote: true,
            leads_on: false,
            through: None,
        })
        .collect()
}

/// The other players swimming in their own games: their bodies here swim too
/// (`swimming.rs`).
fn swimmers(coop: Res<Coop>, mut swim: ResMut<crate::swimming::Swimmers>, mut was: Local<HashMap<u8, bool>>) {
    let mut now = std::collections::HashSet::new();
    for (&id, p) in coop.peers.iter() {
        let swims = !p.lost && p.samples.back().is_some_and(|(_, s)| s.more & nvf_coop::net::more_flag::SWIMMING != 0);
        if swims {
            now.extend(p.references(id));
        }
        if was.insert(id, swims).unwrap_or(false) != swims {
            info!("co-op: {} {} in its own game", label(id, &p.name), if swims { "swims" } else { "is out of the water" });
        }
    }
    if swim.0 != now {
        swim.0 = now;
    }
}

/// Down, revived and respawned (module notes): this player down, the others' puppets
/// lying down while they are, and reviving one in reach.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn downed(
    time: Res<Time>,
    mut coop: ResMut<Coop>,
    (mut script, mut sim): (Option<ResMut<nvf_play::script::ScriptState>>, Option<ResMut<nvf_play::sim::SimState>>),
    player: Option<Res<Player>>,
    loc: Option<Res<Location>>,
    input: nvf_play::bindings::ControlInput,
    mut corner: Option<ResMut<nvf_play::corner_messages::CornerMessages>>,
    mut travel: MessageWriter<TravelRequest>,
    mut hits: MessageWriter<ActorHit>,
    mut cam: Option<ResMut<crate::death_cam::DeathCam>>,
    main_menu: Option<Res<nvf_play::main_menu::MainMenu>>,
    (target, mut screen, kit, mut downed_players): (Res<nvf_play::activate::Target>, ResMut<nvf_play::coop_downed::DownedScreen>, Option<Res<nvf_play::coop_mates::ReviveKit>>, Option<ResMut<nvf_play::coop_downed::DownedPlayers>>),
) {
    let coop = &mut *coop;
    let in_coop = coop.link.is_some();
    if let Some(c) = cam.as_deref_mut().filter(|c| c.downed_in_coop != in_coop) {
        c.downed_in_coop = in_coop;
    }
    // What the downed screen says this frame (set below while this player is down, or
    // revived while the black lifts).
    let mut shown: Option<nvf_play::coop_downed::Downed> = None;
    if cam.as_deref().is_some_and(|c| c.revived_hold()) {
        shown = coop.revived_by.clone().map(|by| nvf_play::coop_downed::Downed::Reviving { by });
    } else if cam.as_deref().is_none_or(|c| !c.down()) {
        coop.revived_by = None;
    }
    let (Some(rt), Some(sim), Some(player), Some(loc)) = (script.as_deref_mut().and_then(|s| s.rt.as_mut()), sim.as_deref_mut(), player, loc) else {
        if screen.0 != shown {
            screen.0 = shown;
        }
        return;
    };
    // A co-op game (a lobby hosted or joined): what fnv-game does only in co-op acts
    // only then (`fnv_game::Game::coop_game`).
    rt.game.coop_game = in_coop;
    if !in_coop || at_main_menu(main_menu.as_deref()) {
        coop.downed_at = None;
        if screen.0.is_some() {
            screen.0 = None;
        }
        if let Some(d) = downed_players.as_deref_mut().filter(|d| d.everyone || d.alone) {
            d.everyone = false;
            d.alone = false;
        }
        return;
    }
    let now = time.elapsed_secs();
    let no_kit = nvf_play::coop_mates::ReviveKit(None);
    downed_body(coop, &mut rt.game, sim, &player, &loc, &input, (&target, kit.as_deref().unwrap_or(&no_kit)), &mut corner, &mut travel, &mut hits, &mut shown, now);
    if screen.0 != shown {
        screen.0 = shown;
    }
    // Who is down, for the progress side (`nvf_play::coop_downed::DownedPlayers`): every
    // player (this one, and each other one down or out of touch), and this one down with
    // no other player up near enough to revive it.
    if let Some(d) = downed_players.as_deref_mut() {
        let me_down = rt.game.is_dead(FormId::PLAYER_REF);
        let everyone = me_down && coop.peers.values().all(|p| p.down || p.lost);
        let alone = me_down && coop.alone_down;
        if d.everyone != everyone || d.alone != alone {
            d.everyone = everyone;
            d.alone = alone;
        }
    }
}

/// [`downed`]'s work once the game runs in a session.
#[allow(clippy::too_many_arguments)]
fn downed_body(
    coop: &mut Coop,
    g: &mut fnv_game::Game<'static>,
    sim: &mut nvf_play::sim::SimState,
    player: &Player,
    loc: &Location,
    input: &nvf_play::bindings::ControlInput,
    (target, kit): (&nvf_play::activate::Target, &nvf_play::coop_mates::ReviveKit),
    corner: &mut Option<ResMut<nvf_play::corner_messages::CornerMessages>>,
    travel: &mut MessageWriter<TravelRequest>,
    hits: &mut MessageWriter<ActorHit>,
    shown: &mut Option<nvf_play::coop_downed::Downed>,
    now: f32,
) {
    let here = place_of(loc);
    let mut say = |n: String| {
        info!("co-op: {n}");
        if let Some(c) = corner.as_mut() {
            c.push(n.into());
        }
    };
    // The others' puppets: down, they lie down as the dead do; up, they are drawn anew.
    for (&id, p) in coop.peers.iter_mut() {
        let Some(&(_, s)) = p.samples.back() else { continue };
        let down = s.has(flag::DOWN);
        // A player up is never dead here, whatever happened while it was down.
        if !down && !p.down && g.is_dead(FormId(p.reference(id))) {
            g.extra.entry(FormId(p.reference(id))).or_default().dead = false;
        }
        if down == p.down {
            continue;
        }
        p.down = down;
        let r = p.reference(id);
        g.extra.entry(FormId(r)).or_default().dead = down;
        if down {
            hits.write(ActorHit { form_id: r, died: true, knocked_out: false, location: -1, crippled: false, part_crippled: false, torso_crippled: false, by: None, share: 1.0, guard_broken: false, weapon: None });
            say(trf("{name} is down: go to them and revive them", &[("name", &label(id, &p.name))]));
        } else {
            // Revived: its body gets up from where it lies ([`revive`]); a body this game
            // cannot raise is drawn anew standing there.
            p.rising = Some(revive::Rising::Start { since: now });
            p.body = None;
            say(trf("{name} is back up", &[("name", &label(id, &p.name))]));
        }
    }
    let pressed = input.just_pressed(nvf_play::bindings::Control::Activate);
    // This player.
    if g.is_dead(FormId::PLAYER_REF) {
        let feet = player.feet;
        let alone = !coop.peers.iter().any(|(_, p)| {
            !p.lost && p.samples.back().is_some_and(|&(_, q)| !q.has(flag::DOWN) && q.place == here && (q.feet[0] - feet[0]).hypot(q.feet[1] - feet[1]) <= ALONE_NEAR)
        });
        if alone != coop.alone_down {
            coop.alone_down = alone;
            info!("co-op: down {}", if alone { "with no other player near enough to revive" } else { "with another player near enough to revive" });
        }
        let since = *coop.downed_at.get_or_insert_with(|| {
            info!("co-op: you are down: another player can revive you with a Stimpak; in {RESPAWN_AFTER:.0} seconds you can respawn");
            now
        });
        if now - since < RESPAWN_AFTER {
            *shown = Some(nvf_play::coop_downed::Downed::Waiting { seconds: (RESPAWN_AFTER - (now - since)).ceil().max(1.0) as u32 });
            return;
        }
        // Beside the nearest player up, else anyone (the host first).
        let dist = |q: &PlayerState| if q.place == here { (q.feet[0] - feet[0]).hypot(q.feet[1] - feet[1]) } else { f32::MAX / 2.0 };
        let target = coop
            .peers
            .iter()
            .filter_map(|(&id, p)| p.samples.back().map(|&(_, q)| (id, p, q)))
            .min_by(|a, b| (a.2.has(flag::DOWN) || a.1.lost, dist(&a.2), a.0).partial_cmp(&(b.2.has(flag::DOWN) || b.1.lost, dist(&b.2), b.0)).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, p, q)| (label(id, &p.name), q));
        if !coop.told_respawn {
            coop.told_respawn = true;
            info!("co-op: you can respawn{}", target.as_ref().map_or(String::new(), |(n, _)| format!(" by {n}")));
        }
        *shown = Some(nvf_play::coop_downed::Downed::Ready { by: target.as_ref().map(|(n, _)| n.clone()) });
        if pressed {
            g.resurrect(FormId::PLAYER_REF, false);
            if let Some((name, q)) = target {
                let (world, interior) = match q.place {
                    Place::Interior(c) => (None, Some(c)),
                    Place::Exterior(w) => (Some(w), None),
                };
                travel.write(TravelRequest(Travel { world, interior, feet: q.feet, heading_deg: q.yaw.to_degrees(), pitch_deg: None }));
                say(trf("You respawned by {name}", &[("name", &name)]));
            } else {
                say(tr("You respawned").into());
            }
            coop.downed_at = None;
            coop.told_respawn = false;
            *shown = None;
        }
        return;
    }
    coop.downed_at = None;
    coop.told_respawn = false;
    coop.alone_down = false;
    // A downed player aimed at where the body lies: the crosshair's "Revive" rollover
    // (`nvf_play::coop_mates`); Activate revives them with a Stimpak.
    let aimed = target.0.as_ref().filter(|t| matches!(t.action, nvf_play::activate::outcome::Action::Revive { .. })).map(|t| t.form_id);
    let near = aimed.and_then(|r| coop.peers.iter().find(|(&id, p)| p.down && !p.lost && p.reference(id) == r)).map(|(&id, p)| (id, label(id, &p.name)));
    let Some((id, name)) = near else {
        coop.revive_offer = None;
        return;
    };
    if coop.revive_offer != Some(id) {
        coop.revive_offer = Some(id);
        info!("co-op: aiming at {name}'s body: Revive");
    }
    if !pressed {
        return;
    }
    // What the revive uses: the crosshair's rollover named it (`ReviveKit`: a Stimpak).
    let Some((item, item_name)) = kit.0.clone() else {
        say(trf("You need a Stimpak to revive {name}", &[("name", &name)]));
        return;
    };
    let have = sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).map_or(0, |i| i.count(item));
    if have < 1 {
        say(trf("You need a Stimpak to revive {name}", &[("name", &name)]));
        return;
    }
    let _ = sim.world.remove_item(FormId::PLAYER_REF, item, 1);
    // The co-op perks that go with a revive are the progress side's (piece 3): none here.
    let perks = 0;
    if let Some(link) = coop.link.as_ref() {
        link.send_revive(id, perks);
    }
    say(trf("You revived {name} ({item} used, {left} left)", &[("name", &name), ("item", &item_name), ("left", &(have - 1))]));
}

/// Where a reference is in this game's world, as [`net::NpcState::space`] tells it: its
/// interior cell, or the worldspace of its exterior cell; 0 when it has no cell.
fn space_of_ref(world: &fnv_sim::World, r: FormId) -> u32 {
    let cell = world.peek(r).and_then(|s| s.cell).or_else(|| world.placed.get(r).and_then(|p| p.cell));
    cell.map_or(0, |c| world.placed.world_of(c).map_or(c.0, |w| w.0))
}

/// Whether `reference` is one of the other players' puppets (never one of the world's).
fn is_puppet(reference: u32) -> bool {
    reference & 0xFFFF_FF00 == PUPPET_REF
}

/// Unlock what another player unlocked: a terminal is hacked, a door or container's lock
/// open. Whether it was still locked here.
fn unlock(sim: &mut nvf_play::sim::SimState, g: &mut fnv_game::Game, r: FormId, terminal: bool) -> bool {
    use fnv_game::terminal::Access;
    if terminal {
        return g.terminals.access.insert(r, Access::Hacked) != Some(Access::Hacked);
    }
    let locked = sim.world.get(r).ok().and_then(|s| s.lock.as_ref()).is_some_and(|l| l.locked);
    let _ = sim.world.unlock(r);
    locked
}

/// Make a drawn puppet do what its player's newest state says (module notes). A new
/// attack with a gun gives the gun, for the shot to be heard.
#[allow(clippy::too_many_arguments)]
fn act(
    commands: &mut Commands,
    e: Entity,
    reference: u32,
    p: &mut Peer,
    s: PlayerState,
    now: f32,
    world: Option<&ActorWorld>,
    actors: &mut Query<(&mut Transform, Option<&mut Locomotion>), Without<crate::furniture::OnFurniture>>,
    holds: &mut Query<&mut WeaponHold>,
    dressed: &Query<(Option<&PuppetEquipment>, Has<Puppet>)>,
    fighters: &mut Fighters,
    attacks: &mut MessageWriter<PlayAttack>,
) -> Option<FormId> {
    let weapon = (s.weapon != 0).then_some(FormId(s.weapon));
    let gear = Equipment { worn: s.worn().iter().map(|&w| FormId(w)).collect(), weapon, weapon_mods: 0 };
    if let Ok((worn, marked)) = dressed.get(e) {
        if !marked {
            // Its head is the other player's: it does not turn to look at this one.
            commands.entity(e).insert((Puppet, crate::head_look::NoHeadLook));
        }
        if worn.is_none_or(|w| w.0 != gear) {
            commands.entity(e).insert(PuppetEquipment(gear));
        }
    }
    let drawn = s.has(flag::DRAWN);
    let sneaking = s.has(flag::SNEAKING);
    if let Ok(mut hold) = holds.get_mut(e) {
        if hold.drawn != drawn {
            hold.drawn = drawn;
        }
    }
    if let Ok((_, Some(mut lo))) = actors.get_mut(e) {
        lo.set_sneaking(sneaking);
    }
    // New attacks and reloads: one each for whatever the counters went up by.
    let (attacked, reloaded) = match p.played {
        Some((a, r)) => (s.attacks != a, s.reloads != r),
        None => (false, false),
    };
    p.played = Some((s.attacks, s.reloads));
    if attacked {
        p.attacked_at = Some(now);
    }
    let fighting = drawn && (s.has(flag::AIMING) || attacked || p.attacked_at.is_some_and(|t| now - t < AIM_AFTER_ATTACK));
    let sights = s.has(flag::SIGHTS);
    // The attack clips play only on an actor with an aim (attack_anims::start).
    fighters.set_puppet(reference, fighting.then_some(PuppetPose { pitch: s.pitch, weapon, sights, crouched: sneaking }));
    if !fighting {
        return None;
    }
    let melee = is_melee(world, weapon);
    if attacked {
        let power = match s.attack_kind {
            attack::POWER => Some(Power::Normal),
            attack::POWER_FORWARD => Some(Power::Forward),
            attack::POWER_BACK => Some(Power::Back),
            attack::POWER_LEFT => Some(Power::Left),
            attack::POWER_RIGHT => Some(Power::Right),
            _ => None,
        };
        let swing = (melee && power.is_none()).then(SwingPick::default);
        attacks.write(PlayAttack { actor: reference, weapon, melee, sights: sights && !melee, kind: Move::Attack, power: power.filter(|_| melee), swing });
    }
    if reloaded && !melee {
        attacks.write(PlayAttack { actor: reference, weapon, melee: false, sights: false, kind: Move::Reload, power: None, swing: None });
    }
    weapon.filter(|_| attacked && !melee)
}

/// The Hellos the host turned away for a wrong or missing secret.
fn hello_secret_drops(stats: &nvf_coop::guard::Stats) -> u64 {
    use nvf_coop::guard::{Kind, Reason};
    stats.dropped.iter().filter(|(k, r, _)| *k == Kind::Hello && *r == Reason::Secret).map(|(_, _, n)| n).sum()
}

/// A player's state as the log shows it: where, the feet rounded, the heading in degrees.
fn state_line(id: u8, s: &PlayerState) -> String {
    let place = match s.place {
        Place::Interior(c) => format!("interior {c:08X}"),
        Place::Exterior(w) => format!("worldspace {w:08X}"),
    };
    let [x, y, z] = s.feet.map(|v| v.round() as i64);
    format!("player {} at {place} feet ({x}, {y}, {z}) yaw {:.0}", id + 1, s.yaw.to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lobby list's quest sharing (docs/design/coop-quest-sync.md §5): the host sees
    /// which questlines each guest sits out.
    #[test]
    fn the_lobby_list_names_the_questlines_a_player_sits_out() {
        use fnv_game::coop_questlines::{bit, index};
        let line = |k: &str| bit(index(k).unwrap());
        assert_eq!(sharing_label(None, 0), "");
        assert_eq!(sharing_label(Some(false), line("house")), " (not sharing quests)");
        assert_eq!(sharing_label(Some(true), 0), " (sharing quests)");
        assert_eq!(sharing_label(Some(true), line("house")), " (sharing quests, sitting out Mr. House)");
        assert_eq!(sharing_label(Some(true), line("house") | line("ncr")), " (sharing quests, sitting out Mr. House, NCR)");
        assert_eq!(sharing_label(Some(true), line("house") | line("ncr") | line("legion") | line("cass")), " (sharing quests, sitting out Mr. House, NCR +2)");
    }

    /// Hosting opens a lobby on the internet transport (LAN only here: no service asked),
    /// and a join of something that is no code says why.
    #[test]
    fn hosting_opens_a_link_and_a_bad_code_says_why() {
        nvf_coop::prefs::set_connection(nvf_coop::prefs::Connection::LanOnly);
        let mut app = App::new();
        app.add_plugins(CoopPlugin(Config { host: Some(0), join: None, name: "Courier".into() }));
        let coop = app.world().resource::<Coop>();
        assert!(coop.link.as_ref().is_some_and(Link::is_host), "{:?}", coop.note);
        assert!(!coop.online_pending && coop.reaching.is_some(), "LAN only: an address code");
        let mut app = App::new();
        app.add_plugins(CoopPlugin(Config { host: None, join: Some("anything".into()), name: "Courier".into() }));
        let coop = app.world().resource::<Coop>();
        assert!(coop.link.is_none());
        assert_eq!(coop.joining, Joining::Failed("That is not a lobby code: check it with the host.".into()));
    }

    struct NoTransport;
    impl nvf_coop::seam::Transport for NoTransport {
        fn send_to(&self, bytes: &[u8], _to: std::net::SocketAddr) -> std::io::Result<usize> {
            Ok(bytes.len())
        }
        fn recv_from(&mut self, _buf: &mut [u8]) -> std::io::Result<Option<(usize, std::net::SocketAddr)>> {
            Ok(None)
        }
        fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
            Ok("127.0.0.1:1".parse().unwrap())
        }
    }

    /// A host's progress rules go on its link at once, with no world loaded, and again for
    /// the next lobby it opens.
    #[test]
    fn a_hosts_progress_rules_go_on_with_its_link_and_again_for_a_second_lobby() {
        let mut app = App::new();
        app.add_plugins(CoopPlugin(Config { host: None, join: None, name: "Courier".into() }));
        let run = |app: &mut App| {
            use bevy::ecs::system::RunSystemOnce;
            app.world_mut().run_system_once(host_progress_rules).unwrap();
        };
        let host = |app: &mut App| {
            let session = net::Session::host_on(Box::new(NoTransport), Box::new(nvf_coop::seam::SharedSecret::new(7).unwrap()));
            app.world_mut().resource_mut::<Coop>().link = Some(Link::new(session).unwrap());
        };
        run(&mut app);
        assert!(!app.world().resource::<Coop>().rules_set, "no link, no rules");
        host(&mut app);
        run(&mut app);
        assert!(app.world().resource::<Coop>().rules_set, "the rules are on before any world is loaded");
        assert!(!app.world().resource::<Coop>().forms_set, "the forms check still waits for the world");
        app.world_mut().resource_mut::<Coop>().stop_hosting();
        assert!(!app.world().resource::<Coop>().rules_set && !app.world().resource::<Coop>().forms_set);
        host(&mut app);
        run(&mut app);
        assert!(app.world().resource::<Coop>().rules_set, "the second lobby has its rules too");
    }

    #[test]
    fn a_state_is_logged_with_its_place_and_rounded_feet() {
        let s = PlayerState::at(Place::Exterior(0x1234), [-68375.4, 4800.6, 8700.0], std::f32::consts::FRAC_PI_2);
        assert_eq!(state_line(1, &s), "player 2 at worldspace 00001234 feet (-68375, 4801, 8700) yaw 90");
    }

    /// The host's wait, sleep or fast travel is time passed for a guest; drift, a clock
    /// set back and the first clock of a join are not.
    #[test]
    fn the_hosts_hours_pass_for_a_guest_but_not_a_join() {
        assert_eq!(guest_hours_passed(1.0, true), Some(1.0), "an hour waited");
        assert_eq!(guest_hours_passed(8.0, true), Some(8.0), "a night slept");
        assert_eq!(guest_hours_passed(0.06, true), None, "drift");
        assert_eq!(guest_hours_passed(-3.0, true), None, "the guest's own wait pulled back");
        assert_eq!(guest_hours_passed(40.0, false), None, "a join: the guest's save's own time");
        assert_eq!(guest_hours_passed(500.0, true), Some(CLOCK_JUMP_MAX));
    }

    /// A joining guest's own quests are held from the host's welcome until it is
    /// flagged a guest (a DLC's start quest moved it on that first frame);
    /// a plain game, or one already a guest (the flag's own hold), or one whose link is
    /// gone, holds nothing here.
    #[test]
    fn a_joining_guest_runs_no_world_logic_before_the_guest_flag() {
        use super::holds_joining_world;
        assert!(holds_joining_world(true, false, true), "welcomed, not yet flagged a guest");
        assert!(!holds_joining_world(false, false, true), "not welcomed yet");
        assert!(!holds_joining_world(true, true, true), "flagged a guest once: follow_npcs holds it");
        assert!(!holds_joining_world(true, false, false), "the link is gone or a host's");
    }

    /// A host sends an actor it draws but is still putting on its cross at
    /// once as on it, not as off; one getting off is sent as off at once, as before.
    #[test]
    fn a_host_sends_one_being_put_on_as_on() {
        let cross = 0x0013_75D8;
        assert_eq!(sent_furniture(None, Some(cross)), (cross, true));
        assert_eq!(sent_furniture(None, None), (0, false));
        assert_eq!(sent_furniture(Some((cross, true, false)), None), (cross, true));
        assert_eq!(sent_furniture(Some((cross, false, false)), None), (cross, false));
        assert_eq!(sent_furniture(Some((cross, true, true)), Some(cross)), (0, false));
    }

    /// A new host's first states with no furniture leave a guest's actor on
    /// the furniture it is on here; once that hold is over, or with the host's furniture,
    /// the host's word goes.
    #[test]
    fn a_new_hosts_first_states_keep_a_guest_on_its_furniture() {
        let cross = 0x0013_75D8;
        assert_eq!(furniture_here(None, true, Some(cross)), Some((cross, true)));
        assert_eq!(furniture_here(None, false, Some(cross)), None);
        assert_eq!(furniture_here(None, true, None), None);
        assert_eq!(furniture_here(Some((0x0010_6192, false)), true, Some(cross)), Some((0x0010_6192, false)));
    }

    fn speech(speaker: u32, target: Option<u32>, remaining: f64) -> fnv_game::game::Speech {
        let mut s = lines_in(&[net::LineView { speaker, target: 0, topic: 0x10, info: 0x11, remaining: 1.0, flags: 0, conversation: 0 }], 0.0, None).remove(0);
        s.target = target.map(FormId);
        s.remaining = remaining;
        s.remote = false;
        s
    }

    /// Two of an NPC's states heard in one frame (one came late) are blended over time,
    /// not put on at once: a runner at about 516 units/s snapped 48-56
    /// units on a guest [observed: buckallobill, in game, FNV 1.4.0.525].
    #[test]
    fn npc_states_heard_in_one_frame_are_blended_not_snapped() {
        let state = |x: f32| net::NpcState { reference: 0x0013_49B6, feet: [x, 0.0, 0.0], heading: 0.0, pitch: 0.0, flags: 0, weapon: 0, attacks: 0, furniture: 0, base: 0, level: 0, all_levels: false, rolls: 0, hostile: false, space: 7 };
        let mut t = NpcTrack::default();
        t.heard(0.0, state(0.0));
        // The next round's state is late: it comes with the one after, in one frame.
        t.heard(0.2, state(52.0));
        t.heard(0.2, state(104.0));
        assert!(t.samples[2].0 > t.samples[1].0, "spaced: {:?}", t.samples.iter().map(|s| s.0).collect::<Vec<_>>());
        // Drawn frame by frame (60 fps) across them, no frame's step is longer than the run's.
        let mut last = t.pose(0.0, 516.0 * NPC_RUN_ROOM).unwrap().0[0];
        let mut at = 0.0;
        while at < 0.4 {
            at += 1.0 / 60.0;
            let x = t.pose(at, 516.0 * NPC_RUN_ROOM).unwrap().0[0];
            assert!(x - last < 48.0, "put {} units on at {at}", x - last);
            last = x;
        }
        assert_eq!(last, 104.0);
        // States on time keep the times they came at.
        let mut ok = NpcTrack::default();
        for k in 0..4 {
            ok.heard(k as f32 * 0.1, state(k as f32 * 52.0));
        }
        assert!(ok.samples.iter().enumerate().all(|(k, s)| (s.0 - k as f32 * 0.1).abs() < 1e-6));
    }

    /// An NPC running about 430 units/s far from the guest, its poses sent at 3 Hz
    /// (every 6 or 7 ticks of 0.05 s): drawn frame by frame, it runs on every frame and
    /// is never held still and put 131-166 units on [observed: buckallobill, in game, FNV 1.4.0.525].
    #[test]
    fn a_far_npc_sent_at_three_hz_runs_smoothly() {
        let state = |x: f32| net::NpcState { reference: 0x0100_B824, feet: [x, 0.0, 0.0], heading: 0.0, pitch: 0.0, flags: npc_flag::MOVING, weapon: 0, attacks: 0, furniture: 0, base: 0, level: 0, all_levels: false, rolls: 0, hostile: false, space: 7 };
        let run = 430.0;
        let top = run * NPC_RUN_ROOM;
        let mut t = NpcTrack::default();
        let (mut now, mut next_sent, mut ticks) = (0.0f32, 0.0f32, 0u32);
        let mut last: Option<f32> = None;
        while now < 6.0 {
            if now >= next_sent {
                // As `replication` pushes a pose: stamped when it came.
                t.samples.push_back((now, state(now * run)));
                while t.samples.len() > 8 {
                    t.samples.pop_front();
                }
                ticks += 1;
                next_sent += if ticks % 2 == 0 { 0.30 } else { 0.35 };
            }
            t.delay = eased_delay(t.delay, t.spacing(), 1.0 / 60.0);
            let x = t.pose(now - t.delay, top).unwrap().0[0];
            if let (Some(l), true) = (last, now > 1.5) {
                let step = x - l;
                assert!(step < 48.0, "put {step:.0} units on at {now:.2} s (delay {:.2})", t.delay);
                assert!(step > 0.0, "held still at {now:.2} s (delay {:.2})", t.delay);
            }
            last = Some(x);
            now += 1.0 / 60.0;
        }
        // Its delay settled at one and a half of its rounds.
        assert!((t.delay - 0.325 * NPC_DELAY_ROUNDS).abs() < 0.05, "delay {}", t.delay);
    }

    /// Running on as it goes far from the guest: its poses come at 10 Hz, then at 3 Hz.
    /// Drawn frame by frame, no frame puts it more than a few frames' run on, and it is
    /// not held still (22-65 unit steps every 0.35 s) [observed: buckallobill, in game, FNV 1.4.0.525].
    #[test]
    fn an_npc_whose_poses_slow_down_runs_on_smoothly() {
        let state = |x: f32| net::NpcState { reference: 0x0100_B824, feet: [x, 0.0, 0.0], heading: 0.0, pitch: 0.0, flags: npc_flag::MOVING, weapon: 0, attacks: 0, furniture: 0, base: 0, level: 0, all_levels: false, rolls: 0, hostile: false, space: 7 };
        let run = 430.0;
        let mut t = NpcTrack::default();
        let (mut now, mut next_sent) = (0.0f32, 0.0f32);
        let mut last: Option<f32> = None;
        let mut biggest = 0.0f32;
        while now < 8.0 {
            if now >= next_sent {
                t.samples.push_back((now, state(now * run)));
                while t.samples.len() > 8 {
                    t.samples.pop_front();
                }
                next_sent += if now < 3.0 { 0.1 } else { 0.35 };
            }
            t.delay = eased_delay(t.delay, t.spacing(), 1.0 / 60.0);
            let x = t.pose(now - t.delay, run * NPC_RUN_ROOM).unwrap().0[0];
            if let (Some(l), true) = (last, now > 0.5) {
                biggest = biggest.max((x - l).abs());
                assert!(x - l > -1.0, "went back {:.0} units at {now:.2} s", l - x);
            }
            last = Some(x);
            now += 1.0 / 60.0;
        }
        // A frame's run is 7 units; a correction when a pose comes stays far under the
        // 48 units a frame that count as a placement.
        assert!(biggest < 24.0, "largest step {biggest:.0}");
    }

    #[test]
    fn the_delay_eases_to_its_rounds_within_bounds() {
        // First drawn: at once.
        assert_eq!(eased_delay(0.0, 0.1, 1.0 / 60.0), NPC_DELAY);
        assert_eq!(eased_delay(0.0, 2.0, 1.0 / 60.0), NPC_DELAY_MAX);
        // From 10 Hz to 3 Hz: a sixtieth of the way in a frame, all of it in a second.
        let d = eased_delay(NPC_DELAY, 0.35, 1.0 / 60.0);
        assert!(d > NPC_DELAY && d < NPC_DELAY + 0.01, "{d}");
        assert!((eased_delay(NPC_DELAY, 0.35, NPC_DELAY_EASE) - 0.525).abs() < 1e-4);
        // The far actor's 3 Hz rounds are no gap.
        const { assert!(NPC_GAP > 0.35 * 1.5) };
    }

    /// A script's move of one of the host's NPCs is drawn as a jump at its time, not a
    /// slide; a walk is blended (a scripted move-to-marker slid on a guest at 1069
    /// units/s) [observed: buckallobill, in game, FNV 1.4.0.525].
    #[test]
    fn an_npc_moved_by_a_script_jumps_and_a_walking_one_is_blended() {
        let state = |x: f32, space: u32| net::NpcState { reference: 0x0012_FCD2, feet: [x, 0.0, 0.0], heading: 0.0, pitch: 0.0, flags: 0, weapon: 0, attacks: 0, furniture: 0, base: 0, level: 0, all_levels: false, rolls: 0, hostile: false, space };
        let track = |a: (f32, f32, u32), b: (f32, f32, u32)| NpcTrack { samples: [(a.0, state(a.1, a.2)), (b.0, state(b.1, b.2))].into(), ..NpcTrack::default() };
        // A walk: 30 units in a round, half way at half the time.
        let walk = track((0.0, 0.0, 7), (0.1, 30.0, 7));
        assert!((walk.pose(0.05, NPC_JUMP_SPEED).unwrap().0[0] - 15.0).abs() < 0.01);
        // A move of 1000 units in a round: where it was, then where it went.
        let moved = track((0.0, 0.0, 7), (0.1, 1000.0, 7));
        assert_eq!(moved.pose(0.05, NPC_JUMP_SPEED).unwrap().0[0], 0.0);
        // Not sent for a while (faded out) and 1000 units on: no slide either.
        let faded = track((0.0, 0.0, 7), (2.0, 1000.0, 7));
        assert_eq!(faded.pose(1.0, NPC_JUMP_SPEED).unwrap().0[0], 0.0);
        // 140 units in a round (1400 units/s) by one that runs 450: a jump; by one
        // that runs 955 (a cazador): its run, blended.
        let enabled = track((0.0, 0.0, 7), (0.1, 140.0, 7));
        assert_eq!(enabled.pose(0.05, 450.0 * NPC_RUN_ROOM).unwrap().0[0], 0.0);
        assert!((enabled.pose(0.05, 955.0 * NPC_RUN_ROOM).unwrap().0[0] - 70.0).abs() < 0.01);
        // Into another space: no blend across.
        let door = track((0.0, 0.0, 7), (0.1, 10.0, 8));
        assert_eq!(door.pose(0.05, NPC_JUMP_SPEED).unwrap().0[0], 0.0);
    }

    /// The host's lines reach a guest as the host says them: its player is the host's
    /// puppet there, a guest's actor that guest's player (or its puppet on another
    /// guest); a line by an actor the host's scripts made is not sent; the time left
    /// counts down from when they came.
    #[test]
    fn the_hosts_lines_reach_a_guest_with_its_people() {
        let stand_in = FormId(0xFF00_0812);
        let mut scene = speech(0x0010_4C3A, Some(0x0010_4C1D), 4.0);
        scene.conversation = Some(3);
        scene.say_to = true;
        let hello = speech(0x0010_4C3A, Some(FormId::PLAYER_REF.0), 2.0);
        let to_guest = speech(0x0010_4C1D, Some(stand_in.0), 1.0);
        let made = speech(0xFF00_0900, None, 5.0);
        let mut own_remote = speech(0x0010_4C1D, None, 5.0);
        own_remote.remote = true;
        let out = lines_out(&[scene.clone(), hello, to_guest, made, own_remote], |r| (r == stand_in).then_some(1));
        assert_eq!(out.len(), 3, "{out:?}");
        assert_eq!((out[1].target, out[2].target), (net::LineView::HOST, net::LineView::guest(1)));
        // On guest 1, half a second later.
        let heard = lines_in(&out, 0.5, Some(1));
        assert_eq!(heard.len(), 3);
        assert!(heard.iter().all(|s| s.remote));
        assert_eq!((heard[0].speaker, heard[0].target, heard[0].conversation, heard[0].say_to), (scene.speaker, scene.target, Some(3), true));
        assert!((heard[0].remaining - 3.5).abs() < 1e-6);
        assert_eq!(heard[1].target, Some(FormId(PUPPET_REF)));
        assert_eq!(heard[2].target, Some(FormId::PLAYER_REF));
        // On guest 2, the line to guest 1 turns to guest 1's puppet; past its end, gone.
        let other = lines_in(&out, 0.5, Some(2));
        assert_eq!(other[2].target, Some(FormId(PUPPET_REF + 1)));
        assert_eq!(lines_in(&out, 1.5, Some(2)).len(), 2);
        assert!(lines_in(&out, 4.5, Some(2)).is_empty());
        // A player's voice goes by the voice table, not here: the host's own lines and its
        // stand-in for a guest are not sent as speakers.
        let host_grunt = speech(FormId::PLAYER_REF.0, None, 1.0);
        let stand_in_line = speech(stand_in.0, None, 1.0);
        assert!(lines_out(&[host_grunt, stand_in_line], |r| (r == stand_in).then_some(1)).is_empty());
    }

    fn placed(id: u32, disabled: bool) -> fnv_sim::refs::PlacedRef {
        fnv_sim::refs::PlacedRef {
            id: FormId(id), sig: *b"REFR", edid: None, base: FormId(0x1F), cell: None, stored_in: None, plugin: 0,
            flags: if disabled { fnv_sim::refs::FLAG_INITIALLY_DISABLED } else { 0 },
            in_persistent_group: false, pos: [0.0; 3], rot: [0.0; 3], scale: None, count: None, ammo: None,
            health: None, owner: None, lock: None, teleport: None, enable_parent: None, linked: None,
            patrol_idle: None, patrol_topic: None, merchant_container: None, marker_reputation: None,
            open_by_default: false, primitive: None, activate_parents: Vec::new(), parent_activate_only: false,
            prompt: None, radiation: None, radius: None, encounter_zone: None, level_modifier: None, map_marker: None, radio: None,
        }
    }

    /// The host's world's presence reaches a guest's (module notes): the host's own flags
    /// that differ from the plugins' (Sunny's bottles, enabled by their disabled parent
    /// marker; a crate it disabled) are taken; once the whole list is in, what only the
    /// guest's game changed (its own done quest's sign) takes the plugins' flag again,
    /// except what is skipped (the NPCs the host draws); leaving puts it all back.
    #[test]
    fn the_hosts_world_presence_wins_on_a_guest() {
        const BOTTLE: u32 = 0x0010_A202;
        const PARENT: u32 = 0x0016_8A92;
        const CRATE: u32 = 0x0010_0001;
        const SIGN: u32 = 0x0010_0002;
        const GECKO: u32 = 0x0010_0003;
        let mut table = fnv_sim::RefTable::default();
        let mut bottle = placed(BOTTLE, false);
        bottle.enable_parent = Some(fnv_sim::refs::EnableParent { reference: FormId(PARENT), opposite: false, pop_in: false });
        for p in [bottle, placed(PARENT, true), placed(CRATE, false), placed(SIGN, true), placed(GECKO, true)] {
            table.refs.insert(p.id, p);
        }
        let forms = fnv_sim::FormStore::default();
        let mut host = fnv_sim::World::new(&forms, &table, Default::default());
        host.set_enabled(FormId(PARENT), true).unwrap();
        host.set_enabled(FormId(CRATE), false).unwrap();
        let _ = host.get(FormId(BOTTLE)).unwrap();
        let list = presence_of(&host, Flag::Enabled);
        assert_eq!(list, vec![(CRATE, false), (PARENT, true)]);

        let mut guest = fnv_sim::World::new(&forms, &table, Default::default());
        guest.set_enabled(FormId(SIGN), true).unwrap();
        guest.set_enabled(FormId(GECKO), true).unwrap();
        assert!(!guest.is_enabled(FormId(BOTTLE)).unwrap());
        let mut heard = HostPresence::default();
        heard.hear(1, 0, 1, list);
        let skip = |r: FormId| r == FormId(GECKO);
        let mut was = HashMap::new();
        let flipped = follow_presence(&mut guest, Flag::Enabled, &std::mem::take(&mut heard.fresh), &skip, &mut was);
        assert_eq!(flipped.len(), 2);
        assert!(guest.is_enabled(FormId(BOTTLE)).unwrap(), "the bottles come with their parent");
        // Their collision is asked for too [observed: buckallobill, in game: a guest's shots went through bottles drawn with no body].
        let mut told = with_enable_children(&table, flipped);
        told.sort();
        assert_eq!(told, vec![FormId(CRATE), FormId(BOTTLE), FormId(PARENT)]);
        assert!(!guest.is_enabled(FormId(CRATE)).unwrap());
        assert_eq!(undo_own_presence(&mut guest, Flag::Enabled, &heard.whole.take().unwrap(), &skip, &mut was).len(), 1);
        assert!(!guest.is_enabled(FormId(SIGN)).unwrap());
        assert!(guest.is_enabled(FormId(GECKO)).unwrap(), "skipped");
        // Leaving co-op: as it was.
        for (r, on) in was.drain() {
            guest.set_enabled(r, on).unwrap();
        }
        assert!(guest.is_enabled(FormId(SIGN)).unwrap() && guest.is_enabled(FormId(CRATE)).unwrap() && !guest.is_enabled(FormId(BOTTLE)).unwrap());
    }

    /// [observed: buckallobill, in game, FNV 1.4.0.525] A guest whose own game finished Ghost Town
    /// Gunfight (the Powder Gangers killed, and disabled as the plugin places them) joins
    /// a host mid-quest. The ganger the host has alive and drawn here is up again and
    /// enabled on the guest; one dead on the host is left to the deaths (dead here too);
    /// one the host disabled is not brought back.
    #[test]
    fn a_guests_own_dead_gangers_are_alive_as_in_the_hosts_world() {
        const CELL: u32 = 0x0010_C000;
        const ALIVE: u32 = 0x0010_4C70;
        const KILLED: u32 = 0x0010_4C72;
        const GONE: u32 = 0x0010_4C73;
        const UNPLACED: u32 = 0x0010_4C75;
        let mut table = fnv_sim::RefTable::default();
        for (id, disabled) in [(FormId::PLAYER_REF.0, false), (ALIVE, true), (KILLED, true), (GONE, true), (UNPLACED, true)] {
            let mut p = placed(id, disabled);
            p.cell = Some(FormId(CELL));
            table.refs.insert(p.id, p);
        }
        let forms = fnv_sim::FormStore::default();
        let mut world = fnv_sim::World::new(&forms, &table, Default::default());
        let _ = world.get(FormId::PLAYER_REF).unwrap();
        let empty = crate::outfits::tests::empty_world_data();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
        let json = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let functions = fnv_script::FunctionTable::load(&path).unwrap();
        let no_refs: &'static fnv_sim::RefTable = Box::leak(Box::default());
        let no_forms: &'static fnv_sim::FormStore = Box::leak(Box::default());
        let data: &'static fnv_game::GameData = Box::leak(Box::new(fnv_game::GameData::build(empty.lo, empty.schema, functions, &json, no_refs)));
        let mut g = fnv_game::Game::new(data, fnv_sim::World::new(no_forms, no_refs, Default::default()), Default::default());
        // The guest's own game: every ganger killed.
        for r in [ALIVE, KILLED, GONE, UNPLACED] {
            g.extra.entry(FormId(r)).or_default().dead = true;
        }
        let sample = |r: u32, flags: u8| net::NpcState { reference: r, feet: [10.0, 20.0, 30.0], heading: 0.0, pitch: 0.0, flags, weapon: 0, attacks: 0, furniture: 0, base: 0, level: 0, all_levels: false, rolls: 0, hostile: true, space: 0 };
        let mut npcs = HashMap::new();
        for (r, flags) in [(ALIVE, 0), (KILLED, npc_flag::DEAD), (GONE, 0)] {
            let mut t = NpcTrack::default();
            t.samples.push_back((10.0, sample(r, flags)));
            npcs.insert(r, t);
        }
        // One the host drew this frame, its body not put where it stands yet: at the origin.
        let mut t = NpcTrack::default();
        t.samples.push_back((10.0, net::NpcState { feet: [0.0; 3], ..sample(UNPLACED, 0) }));
        npcs.insert(UNPLACED, t);
        let host_disabled = HashSet::from([GONE]);
        let enabled = place_like_host(&mut world, &mut g, &npcs, &host_disabled, 5.0, 10.2);
        assert_eq!(enabled, vec![FormId(ALIVE)]);
        assert!(world.is_enabled(FormId(ALIVE)).unwrap() && !g.is_dead(FormId(ALIVE)), "alive and drawn as in the host's world");
        assert!(!world.is_enabled(FormId(KILLED)).unwrap() && g.is_dead(FormId(KILLED)), "dead on the host: left to the deaths");
        assert!(!world.is_enabled(FormId(GONE)).unwrap(), "the host disabled it");
        assert!(!world.is_enabled(FormId(UNPLACED)).unwrap() && g.is_dead(FormId(UNPLACED)), "a sample at the origin is no place: left for the next");
        // Revived where the host has it, not where the corpse (or nothing) put it here.
        assert_eq!(world.peek(FormId(ALIVE)).map(|s| s.pos), Some([10.0, 20.0, 30.0]), "stands up at the host's feet");
    }

    /// The host's doors reach a guest's: a door the host opened opens, one only the
    /// guest's game opened closes again, except the one its player just used; a load door
    /// takes no part.
    #[test]
    fn the_hosts_doors_win_on_a_guest() {
        const OPENED: u32 = 0x0010_0D01;
        const OWN: u32 = 0x0010_0D02;
        const USED: u32 = 0x0010_0D03;
        const LOAD: u32 = 0x0010_0D04;
        let mut table = fnv_sim::RefTable::default();
        let mut load = placed(LOAD, false);
        load.teleport = Some(fnv_sim::refs::Teleport { door: FormId(0x0010_0D05), pos: [0.0; 3], rot: [0.0; 3], no_alarm: false });
        for p in [placed(OPENED, false), placed(OWN, false), placed(USED, false), load] {
            table.refs.insert(p.id, p);
        }
        let forms = fnv_sim::FormStore::default();
        let mut host = fnv_sim::World::new(&forms, &table, Default::default());
        host.get(FormId(OPENED)).unwrap().open = true;
        host.get(FormId(LOAD)).unwrap().open = true;
        let list = presence_of(&host, Flag::Open);
        assert_eq!(list, vec![(OPENED, true)]);

        let mut guest = fnv_sim::World::new(&forms, &table, Default::default());
        guest.get(FormId(OWN)).unwrap().open = true;
        guest.get(FormId(USED)).unwrap().open = true;
        let mut heard = HostPresence::default();
        heard.hear(1, 0, 1, list);
        let skip = |r: FormId| r == FormId(USED);
        let mut was = HashMap::new();
        assert_eq!(follow_presence(&mut guest, Flag::Open, &std::mem::take(&mut heard.fresh), &skip, &mut was).len(), 1);
        assert_eq!(undo_own_presence(&mut guest, Flag::Open, &heard.whole.take().unwrap(), &skip, &mut was).len(), 1);
        let open = |r: u32| door_open(&guest, FormId(r)).unwrap();
        assert!(open(OPENED) && !open(OWN) && open(USED));
        assert_eq!(door_open(&guest, FormId(LOAD)), None, "a load door");
    }

    /// A guest has the host's whole list only once every page of one serial came; a new
    /// serial starts again.
    #[test]
    fn the_whole_presence_list_is_every_page_of_one_serial() {
        let mut p = HostPresence::default();
        p.hear(4, 0, 2, vec![(1, true)]);
        assert!(p.whole.is_none());
        p.hear(5, 1, 2, vec![(2, true)]);
        assert!(p.whole.is_none(), "another serial");
        p.hear(5, 0, 2, vec![(3, false)]);
        assert_eq!(p.whole.take(), Some(HashSet::from([2, 3])));
        assert_eq!(p.fresh.len(), 3);
    }

    /// A guest hides its own actors near the host that the host's world does not draw;
    /// not the host's NPCs, not those far off, not the other players' puppets.
    #[test]
    fn what_the_host_does_not_have_near_it_is_hidden() {
        let host = [1000.0, 1000.0, 50.0];
        let drawn = [
            (0x0010_4C3A, [1100.0, 900.0, 0.0]),
            (0x0010_4C1D, [1200.0, 1000.0, 0.0]),
            (0x0010_5000, [9000.0, 1000.0, 0.0]),
            (PUPPET_REF, [1000.0, 1000.0, 50.0]),
            (0xFF00_0123, [1000.0, 1500.0, 0.0]),
        ];
        let heard: HashSet<u32> = [0x0010_4C3A].into();
        let hidden = not_in_hosts_world(&drawn, &heard, host, HIDE_REACH);
        assert_eq!(hidden, [0x0010_4C1D, 0xFF00_0123].into());
    }

    #[test]
    fn a_guests_limb_damage_is_taken_only_up_to_the_cap() {
        let hit = |damage: f64, points: bool| RelayedHit { target: FormId(1), dealt: 20.0, weapon: None, location: 1, critical: false, limb: Some(fnv_game::combat::LimbHit { condition: 25, health_percent: 10.0, damage, points }) };
        let damage = |h: RelayedHit| h.limb.unwrap().damage;
        assert_eq!(damage(clamp_limb(hit(1e9, false), 40.0)), 40.0);
        assert_eq!(damage(clamp_limb(hit(35.0, false), 40.0)), 35.0, "a multiplier above 1 still works");
        assert_eq!(damage(clamp_limb(hit(-5.0, false), 40.0)), 0.0);
        assert_eq!(damage(clamp_limb(hit(1e9, true), 40.0)), 1e9, "condition points are not a share");
    }

    #[test]
    fn a_hit_goes_over_the_link_and_back() {
        let limb = fnv_game::combat::LimbHit { condition: 27, health_percent: 15.5, damage: 33.25, points: true };
        for h in [
            RelayedHit { target: FormId(0x100), dealt: 12.5, weapon: Some(FormId(0x200)), location: 2, critical: true, limb: Some(limb) },
            RelayedHit { target: FormId(7), dealt: 0.0, weapon: None, location: -1, critical: false, limb: None },
        ] {
            let v = hit_view(&h);
            assert_eq!(v.weapon, h.weapon.map_or(0, |w| w.0));
            assert_eq!(format!("{:?}", relayed_hit(&v)), format!("{h:?}"));
        }
    }

    #[test]
    fn a_look_goes_over_the_link_and_back() {
        let l = Look {
            race: FormId(0x19),
            female: true,
            hair: Some(FormId(0x0004_2A1B)),
            hair_color: Some([120, 80, 40]),
            eyes: None,
            head_parts: vec![FormId(0x1234)],
            fggs: (0..50).map(|i| i as f32 * 0.1).collect(),
            fgga: vec![-0.5; 30],
            fgts: (0..50).map(|i| -(i as f32)).collect(),
        };
        let name = "\u{d1}".repeat(40);
        let b = encode_look(Some(&l), &name);
        assert!(b.len() <= net::LOOK_MAX, "{} bytes", b.len());
        // Cut to whole characters within NAME_MAX bytes.
        assert_eq!(decode_look(&b), Some((Some(l.clone()), "\u{d1}".repeat(NAME_MAX / 2))));
        assert_eq!(decode_look(&encode_look(None, "")), Some((None, String::new())));
        assert_eq!(decode_look(&encode_look(None, "Cass")), Some((None, "Cass".into())));
        // A look from a game that sends no name.
        assert_eq!(decode_look(&look_only(Some(&l))), Some((Some(l), String::new())));
        assert_eq!(decode_look(&b[..b.len() - 1]), None);
        assert_eq!(decode_look(&[9]), None);
    }

    #[test]
    fn players_are_named_by_slot_until_their_names_arrive() {
        assert_eq!(label(0, ""), "The host");
        assert_eq!(label(2, ""), "Player 3");
        assert_eq!(label(2, "Boone"), "Boone");
    }

    #[test]
    fn an_item_is_one_of_the_carried_records() {
        assert!(ITEM_SIGS.contains(b"WEAP") && ITEM_SIGS.contains(b"CMNY") && ITEM_SIGS.contains(b"IMOD"));
        assert!(!ITEM_SIGS.contains(b"STAT") && !ITEM_SIGS.contains(b"NPC_"));
    }
}
