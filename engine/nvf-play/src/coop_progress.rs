//! Shared co-op progression:
//!
//! - **Quest rewards.** When the host completes a quest, each guest gets that quest's XP
//!   and caps (`fnv_game::coop_rewards`: the host's game measures what its scripts gave
//!   for it; a guest takes it with `Game::take_coop_reward`). Item rewards stay the
//!   host's: they belong to a later "synced progression" option, not built.
//! - **Quests ended**. Each quest the host completes or fails (a Fail Quest
//!   stage, FailQuest) reaches each guest at once and in order ([`Shared::QuestEnded`]),
//!   apart from the completion's reward: its "Quest completed" or "Quest failed" line
//!   ("Quest failed" is NVForever's own words with no sound: the data has neither,
//!   docs/spec-notes/quest-messages.md §5) and a done row in the guest's quest list,
//!   beside the host's active quest (`Game::coop_host_ended`).
//!   Objectives have no failed state in this game (no SetObjectiveFailed among the
//!   functions [observed: schemas/script/functions.json]), so none is sent.
//! - **A save by anyone saves everyone** (anti-exploit: nobody keeps a save the others
//!   lack). A save this game makes is announced ([`Shared::Save`] out), and a save
//!   another player made is followed: this game saves the same kind as soon as saving is
//!   allowed (`save::not_now`). A followed save is not announced again, and an autosave
//!   (or system save) within [`AUTO_QUIET`] seconds of the last shared save is not
//!   announced either, so a trip that autosaves in every game does not set off rounds of
//!   saves. A guest's save is still its own game where it left it, with the character as
//!   it is now (`save::SoloBase`).
//! - **Everyone down: back to the last shared save.** When every player is down at once
//!   (the others lost or down: nobody left to revive) for [`DOWN_HOLD`] seconds, the
//!   host tells every game to load its last shared save ([`Shared::Reload`]) and loads
//!   its own [`RELOAD_DELAY`] seconds later, so the message is out before its game starts
//!   again. A load restarts the game with its lobby (nvforever `coop`), so the guests
//!   come back into the host's world as it was at that save. The last shared save's path
//!   is kept in [`SHARED_ENV`] across those restarts. A guest's save is its own game where
//!   it left it, so its new run is taken beside the host ([`REJOIN_ENV`]).
//!
//! - **A guest down far from everyone: its own last save.** A guest whose health runs out
//!   with no other player up near it (nobody to revive it; nvforever `coop` decides,
//!   [`CoopProgress::alone_down`]) loads its last save after [`ALONE_HOLD`] seconds, as a
//!   player alone does (observed: buckallobill, in game, FNV 1.4.0.525). The load restarts its game, which rejoins the
//!   host's session as the same player. Without a save it respawns by another player.
//!
//! nvforever's `coop` moves the messages: it sets [`CoopProgress::role`] and
//! [`CoopProgress::everyone_down`], puts what it heard in `inbox` and sends `outbox`.

use crate::save::{LoadRequest, SaveConfig, SaveDone, SaveRequest};
use crate::script::ScriptState;
use bevy::prelude::*;
use fnv_util::words::{tr, trf};
use fnv_save::SaveKind;
use fnv_sim::FormId;
use std::path::{Path, PathBuf};

/// An autosave or system save this soon (seconds) after the last shared save is not
/// announced.
pub const AUTO_QUIET: f64 = 20.0;
/// Everyone must stay down this long before the host reloads (a revive in flight).
pub const DOWN_HOLD: f64 = 3.0;
/// The host loads this long after telling the guests to.
pub const RELOAD_DELAY: f64 = 0.5;
/// A guest down with nobody near loads its last save after this long (seconds).
pub const ALONE_HOLD: f64 = 5.0;
/// The last shared save's path, kept across the restart a load makes.
pub const SHARED_ENV: &str = "NVF_COOP_SHARED_SAVE";
/// Set on a guest that loads for everyone being down: its new run goes beside the host
/// (a guest's save is its own game where it left it; nvforever `coop::rejoin`).
pub const REJOIN_ENV: &str = "NVF_COOP_REJOIN_HOST";
/// Set for the run a load starts when that run must hold the syncing screen until it has
/// the host's world (a first join, or a save loaded after going down;
/// nvforever `coop::syncing`). Not set for ordinary loads.
pub const SYNC_ENV: &str = "NVF_COOP_SYNC_SCREEN";

/// This game's part in a co-op session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    #[default]
    Solo,
    Host,
    Guest,
}

/// Shared progression between the games (nvf-coop `progress::Progress`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shared {
    Reward { quest: u32, xp: u32, caps: u32 },
    Save { stamp: u32, kind: SaveKind },
    Reload { stamp: u32 },
    /// A hand-out of the host's scripts (`fnv_game::coop_rewards`):
    /// `quest` 0 and `stage` `u32::MAX` for none (`nvf_coop::progress::Progress::Gift`).
    Gift { batch: u32, item: u32, count: u32, quest: u32, stage: u32 },
    /// The host's quest completed or failed (`nvf_coop::progress::Progress::QuestEnded`).
    QuestEnded { quest: u32, failed: bool },
    /// Joint progression (`fnv_game::coop_joint`): a guest's join choice and character.
    /// `sat_out`: the questlines it sits out (`fnv_game::coop_questlines::Mask`).
    JointChoice { character: u64, on: bool, sat_out: u64 },
    /// The host set a claimable stage, for the players whose bit is set in `to`.
    Stage { quest: u32, stage: i32, line: u32, line_end: bool, prior: Vec<i32>, to: u8 },
    StageResult { quest: u32, stage: i32, taken: bool },
    /// Kill credit for player `to` (coop-shared-progress.md §4.2).
    KillCredit { to: u8, target: u32, target_base: u32, npc: bool, xp: u32, karma: i32, killer: bool, infamy: Vec<(u32, i32)> },
    HitCredit { to: u8, target: u32, weapon: u32, part: u32, dealt: u32, critical: bool, crippled: bool },
    /// `what` for player `to` alone: the host's catch-up for a player that came back
    /// ([`Returns`]; nvf-coop `progress::Progress::For`).
    For { to: u8, what: Box<Shared> },
}

/// The mailbox between this module and nvforever's `coop`.
#[derive(Resource, Debug, Default)]
pub struct CoopProgress {
    pub role: Role,
    /// Every player is down now (the host's view; module notes).
    pub everyone_down: bool,
    /// A guest: this player is down with no other player up near it (module notes).
    pub alone_down: bool,
    /// Heard from player `id`.
    pub inbox: Vec<(u8, Shared)>,
    /// To send.
    pub outbox: Vec<Shared>,
    pub saves: SharedSaves,
    /// A guest: the hand-out batch being taken, and whether it had Caravan cards before it.
    pub gift_batch: Option<(u32, bool)>,
    /// A guest: hand-outs taken, waiting for their corner lines ([`HandOuts`]).
    pub hand_outs: HandOuts,
    /// This player's id in the session (nvforever `coop` sets it).
    pub my_id: Option<u8>,
    /// A guest: its join choice for joint progression (the join question, coop-shared-progress.md §3.1);
    /// `None` until chosen. Sent to the host once with the character's ID.
    pub joint_on: Option<bool>,
    pub joint_sent: bool,
    /// A guest: the join question is up, for the screen that asks it; and the answer it
    /// gave ([`joint_question`] takes it). Until a screen asks, [`JOINT_ENV`] or the
    /// lobby's Quest Sync toggle answer it, and nothing is claimed unanswered.
    pub joint_ask: bool,
    pub joint_answer: Option<bool>,
    /// The host: the story hold the corner line was shown for (the NPC, when the hold began), so one
    /// hold says it once ([`story_wait_notice`]).
    pub wait_told: Option<(FormId, u64)>,
    /// A guest: the syncing screen is up (the world sync sets it); the question waits.
    pub syncing: bool,
    /// The host: each guest's character and choice ([`Shared::JointChoice`]), by id.
    pub joint_peers: std::collections::BTreeMap<u8, (u64, bool)>,
    /// The host: the questlines each guest sits out ([`Shared::JointChoice`]), by id
    /// (docs/design/coop-quest-sync.md §5).
    pub joint_sat_out: std::collections::BTreeMap<u8, u64>,
    /// A guest: the choice last sent to the host (sync on, lines sat out); sent again
    /// when the lobby's toggle or Questlines menu changes it.
    pub joint_sent_as: Option<(bool, u64)>,
    /// A guest: when the last [`Shared::JointChoice`] went; the host lets one in a second
    /// through ([`JOINT_EVERY`]), so a quicker change waits here and its latest goes.
    pub joint_sent_at: Option<std::time::Instant>,
    /// A guest: `NVF_COOP_JOINT` answered the join question this session (once, so the
    /// lobby's toggle can change it after).
    pub joint_env_taken: bool,
    /// The host: each other player's actor in this world, by id (nvforever `coop`).
    pub peer_actors: std::collections::BTreeMap<u8, FormId>,
    /// The other players in the session now, connected or not, and those heard from
    /// now (not lost), by id (nvforever `coop`).
    pub present: std::collections::BTreeSet<u8>,
    pub connected: std::collections::BTreeSet<u8>,
    /// The other players' names, by id (nvforever `coop`).
    pub peer_names: std::collections::BTreeMap<u8, String>,
    /// The host: the players that dropped, and what they would have got ([`Returns`]).
    pub returns: Returns,
    /// A guest: the quests it was told the host went on without it in.
    pub unshared_told: std::collections::BTreeSet<u32>,
    /// A guest: the host's stage offers heard before this game was kept as its own
    /// (`save::SoloBase`, right after joining, when a catch-up comes), and since when.
    /// Taken in order once it is kept, at most [`HOLD_FOR`] later (design:
    /// a returning guest's catch-up stages were lost this way).
    pub held: Vec<Shared>,
    pub held_since: Option<std::time::Instant>,
}

/// How long a guest holds the host's stage offers while its own game is not kept yet.
pub const HOLD_FOR: std::time::Duration = std::time::Duration::from_secs(120);

/// The host's catch-up for players that drop and come back [design: "when
/// someone reconnects they will get whatever they would've gotten had they been there"].
///
/// - A player (its character's stable ID, [`Shared::JointChoice`]) that no connected id
///   answers for any more, lost or gone, is **away** from that moment.
/// - Everything the host sends its players while one is away (stage offers, quest
///   rewards, hand-outs, quests ended) is kept for it, at most [`Returns::KEEP_MAX`], for
///   as long as the host's game runs.
/// - When it is back (its connection heard again, or a new join with the same
///   character), it gets all of it, in order, for it alone ([`Shared::For`]): a stage
///   offer only when it shares quest progress this time. Its own game's checks decide
///   each one as they would have then, so the quest stays shared.
/// - For [`Returns::WARN_FOR`] after a player dropped, the host is told when it moves a
///   quest on or turns one in: a warning, never a block (design: "it should warn the host,
///   not prevent").
#[derive(Debug, Default)]
pub struct Returns {
    away: std::collections::BTreeMap<u64, Away>,
}

#[derive(Debug)]
struct Away {
    name: String,
    /// It shared quest progress when it dropped.
    on: bool,
    since: std::time::Instant,
    backlog: Vec<Shared>,
    warned: std::collections::BTreeSet<u32>,
}

/// A player back: its id, name and what it gets.
#[derive(Debug, PartialEq)]
pub struct Back {
    pub id: u8,
    pub name: String,
    pub items: Vec<Shared>,
}

impl Returns {
    /// How long after a drop the host is warned about moving quests on.
    pub const WARN_FOR: std::time::Duration = std::time::Duration::from_secs(60);
    /// At most this much is kept for one player.
    pub const KEEP_MAX: usize = 2000;

    /// Who went away and who came back: `joint` is each player's character and choice by
    /// id, `connected` the ids heard from now. Returns the names of the newly away, and
    /// the players back with their catch-up.
    pub fn track(&mut self, joint: &std::collections::BTreeMap<u8, (u64, bool)>, connected: &std::collections::BTreeSet<u8>, names: &std::collections::BTreeMap<u8, String>, now: std::time::Instant) -> (Vec<String>, Vec<Back>) {
        let here = |ch: u64| joint.iter().find(|(id, (c, _))| *c == ch && connected.contains(id)).map(|(id, (_, on))| (*id, *on));
        let mut gone = Vec::new();
        for (id, &(ch, on)) in joint {
            if here(ch).is_none() && !self.away.contains_key(&ch) {
                let name = names.get(id).cloned().unwrap_or_else(|| trf("Player {n}", &[("n", &(id + 1))]));
                gone.push(name.clone());
                self.away.insert(ch, Away { name, on, since: now, backlog: Vec::new(), warned: Default::default() });
            }
        }
        let mut back = Vec::new();
        let chars: Vec<u64> = self.away.keys().copied().collect();
        for ch in chars {
            let Some((id, on)) = here(ch) else { continue };
            let Some(a) = self.away.remove(&ch) else { continue };
            let items = a
                .backlog
                .into_iter()
                .filter_map(|w| match w {
                    Shared::Stage { quest, stage, line, line_end, prior, .. } if on && id < 8 => Some(Shared::Stage { quest, stage, line, line_end, prior, to: 1 << id }),
                    Shared::Stage { .. } => None,
                    w => Some(w),
                })
                .map(|w| Shared::For { to: id, what: Box::new(w) })
                .collect();
            back.push(Back { id, name: names.get(&id).cloned().unwrap_or(a.name), items });
        }
        (gone, back)
    }

    /// Keeps `what`, just sent to the players here, for each player away; returns the
    /// host's warnings: one per quest and player while that player dropped less than
    /// [`Returns::WARN_FOR`] ago. `quest_name` names a quest the Pip-Boy lists.
    pub fn record(&mut self, what: &Shared, quest_name: impl Fn(u32) -> Option<String>, now: std::time::Instant) -> Vec<String> {
        let quest = match what {
            Shared::Stage { quest, .. } | Shared::QuestEnded { quest, .. } => Some(*quest),
            Shared::Reward { quest, .. } | Shared::Gift { quest, .. } => Some(*quest).filter(|q| *q != 0),
            _ => return Vec::new(),
        };
        let mut warn = Vec::new();
        for a in self.away.values_mut() {
            if a.backlog.len() < Self::KEEP_MAX {
                a.backlog.push(what.clone());
            }
            let Some(q) = quest else { continue };
            let since = now.saturating_duration_since(a.since);
            if since < Self::WARN_FOR && a.warned.insert(q) {
                if let Some(n) = quest_name(q) {
                    warn.push(trf("{name} dropped {secs} s ago: they get {quest} when they are back", &[("name", &a.name), ("secs", &since.as_secs().max(1)), ("quest", &n)]));
                }
            }
        }
        warn
    }

    /// Players away now.
    pub fn away(&self) -> usize {
        self.away.len()
    }

    /// Whether a player away shared quest progress (its stages must still be offered,
    /// `fnv_game::Game::coop_joint_away`).
    pub fn away_sharing(&self) -> bool {
        self.away.values().any(|a| a.on)
    }
}

/// A guest's hand-outs taken and not yet told: their lines ([`gift_lines`]) wait until
/// no hand-out has come for [`HandOuts::HOLD`], so a stack the host's script adds a piece
/// at a time is one line even when the pieces arrive over several frames (design: Doc
/// Mitchell's 22 single 9mm rounds came in two frames 5 ms apart, "9mm Round (7)" then
/// "9mm Round (15)") [design].
#[derive(Debug, Default)]
pub struct HandOuts {
    pending: Vec<(u32, String, i32)>,
    last: Option<std::time::Instant>,
}

impl HandOuts {
    /// How long the lines wait for more of the same hand-out.
    pub const HOLD: std::time::Duration = std::time::Duration::from_millis(500);

    pub fn add(&mut self, now: std::time::Instant, item: u32, name: String, n: i32) {
        self.pending.push((item, name, n));
        self.last = Some(now);
    }

    /// The lines to show now: all of them once [`Self::HOLD`] has passed since the last.
    pub fn due(&mut self, host: &str, now: std::time::Instant) -> Vec<String> {
        match self.last {
            Some(t) if now.duration_since(t) >= Self::HOLD => {
                self.last = None;
                gift_lines(host, &std::mem::take(&mut self.pending))
            }
            _ => Vec::new(),
        }
    }
}

/// What a game does about its own save just written ([`SharedSaves::saved`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Announce {
    /// Tell the others to save too.
    Tell,
    /// One another player asked for: already shared.
    Followed,
    /// An autosave right after a shared save: not told.
    Quiet,
}

/// What the host's all-down watch asks for ([`SharedSaves::down`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownStep {
    Wait,
    /// Tell the guests to load their last shared save.
    TellReload,
    /// Load this game's last shared save.
    Load(PathBuf),
    /// Everyone is down and there is no shared save to go back to.
    NoSave,
}

/// The shared saves' bookkeeping (module notes); pure, so it is tested alone.
#[derive(Debug, Clone, Default)]
pub struct SharedSaves {
    /// Saves another player asked for, still to be asked of the save system.
    pub waiting: Vec<SaveKind>,
    /// Saves asked for because another player saved: not announced when written.
    following: Vec<PathBuf>,
    /// The last shared save, and when it was written (seconds).
    pub last: Option<PathBuf>,
    last_at: Option<f64>,
    /// The host's all-down watch: since when everyone is down, and when the reload was told.
    down_since: Option<f64>,
    told_at: Option<f64>,
    loaded: bool,
    told_no_save: bool,
    /// A guest's watch while down alone: since when, and whether it loaded or said so.
    alone_since: Option<f64>,
    alone_done: bool,
}

impl SharedSaves {
    /// Another player saved `kind`: this game saves it too (once, however often heard
    /// before it could).
    pub fn heard_save(&mut self, kind: SaveKind) {
        if !self.waiting.contains(&kind) {
            self.waiting.push(kind);
        }
    }

    /// A followed save was asked for, to be written at `path`.
    pub fn following(&mut self, path: PathBuf) {
        self.following.push(path);
    }

    /// This game wrote the save at `path` (`auto`: an autosave or system save) at `now`.
    pub fn saved(&mut self, path: &Path, auto: bool, now: f64) -> Announce {
        let what = if let Some(i) = self.following.iter().position(|p| p == path) {
            self.following.remove(i);
            Announce::Followed
        } else if auto && self.last_at.is_some_and(|t| now - t < AUTO_QUIET) {
            Announce::Quiet
        } else {
            Announce::Tell
        };
        if what != Announce::Quiet {
            self.last = Some(path.to_path_buf());
            self.last_at = Some(now);
        }
        what
    }

    /// The host's watch, each frame: `all_down` now, at `now` (seconds).
    pub fn down(&mut self, all_down: bool, now: f64) -> DownStep {
        if !all_down {
            self.down_since = None;
            self.told_at = None;
            self.loaded = false;
            self.told_no_save = false;
            return DownStep::Wait;
        }
        let since = *self.down_since.get_or_insert(now);
        if self.loaded || now - since < DOWN_HOLD {
            return DownStep::Wait;
        }
        let Some(path) = self.last.clone() else {
            if self.told_no_save {
                return DownStep::Wait;
            }
            self.told_no_save = true;
            return DownStep::NoSave;
        };
        match self.told_at {
            None => {
                self.told_at = Some(now);
                DownStep::TellReload
            }
            Some(t) if now - t >= RELOAD_DELAY => {
                self.loaded = true;
                DownStep::Load(path)
            }
            Some(_) => DownStep::Wait,
        }
    }
}

impl SharedSaves {
    /// A guest's watch, each frame: down with nobody near (`alone`) at `now`.
    pub fn alone(&mut self, alone: bool, now: f64) -> DownStep {
        if !alone {
            self.alone_since = None;
            self.alone_done = false;
            return DownStep::Wait;
        }
        let since = *self.alone_since.get_or_insert(now);
        if self.alone_done || now - since < ALONE_HOLD {
            return DownStep::Wait;
        }
        self.alone_done = true;
        match self.last.clone() {
            Some(path) => DownStep::Load(path),
            None => DownStep::NoSave,
        }
    }
}

/// Every player is down: this one, and each other one down or lost (nobody left to
/// revive); `others` is (down, lost) per other player, and there must be one.
pub fn everyone_down(me_down: bool, others: impl IntoIterator<Item = (bool, bool)>) -> bool {
    let mut any = false;
    for (down, lost) in others {
        any = true;
        if !down && !lost {
            return false;
        }
    }
    me_down && any
}

/// The save request a followed save of `kind` makes (a manual save gets a new name).
fn follow_request(kind: SaveKind, cfg: &SaveConfig) -> SaveRequest {
    let name = match kind {
        SaveKind::Quick => fnv_save::QUICK_NAME.to_string(),
        SaveKind::Auto => fnv_save::AUTO_NAME.to_string(),
        SaveKind::System => fnv_save::SYSTEM_NAME.to_string(),
        SaveKind::Manual => crate::pause::new_save_name(cfg),
    };
    SaveRequest { name, kind }
}

fn stamp() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u32)
}

pub struct CoopProgressPlugin;

impl Plugin for CoopProgressPlugin {
    fn build(&self, app: &mut App) {
        let mut p = CoopProgress::default();
        p.saves.last = std::env::var_os(SHARED_ENV).map(PathBuf::from).filter(|p| p.exists());
        app.insert_resource(p).add_systems(Update, (quest_sync_asks, take_inbox, joint_question, rewards, announce_saves, story_wait_notice, all_down, alone_down, quest_sync_view).chain().in_set(crate::PlaySet::Ui)).add_systems(Update, follow_saves.after(crate::PlaySet::Ui));
    }
}

/// The corner line for a quest reward the host's completion brought: the quest by its shown
/// name, or "a quest" when it has none (an editor ID is not for the player's eyes).
fn reward_line(host: &str, quest: Option<&str>, xp: impl std::fmt::Display, caps: u32) -> String {
    let caps = if caps > 0 { trf(", +{n} caps", &[("n", &caps)]) } else { String::new() };
    match quest {
        Some(q) => trf("{host} completed {quest}: +{xp} XP{caps}", &[("host", &host), ("quest", &q), ("xp", &xp), ("caps", &caps)]),
        None => trf("{host} completed a quest: +{xp} XP{caps}", &[("host", &host), ("xp", &xp), ("caps", &caps)]),
    }
}

/// The name a hand-out's line shows: the item's FULL, as the player sees it elsewhere;
/// its Editor ID only when it has none (design showed an editor ID there).
fn item_label(full_names: &std::collections::HashMap<FormId, String>, item: FormId, edid: impl FnOnce() -> String) -> String {
    full_names.get(&item).filter(|n| !n.is_empty()).cloned().unwrap_or_else(edid)
}

fn say(corner: &mut Option<ResMut<crate::corner_messages::CornerMessages>>, n: String) {
    info!("co-op: {n}");
    if let Some(c) = corner.as_mut() {
        c.push(n.into());
    }
}

/// What the other players sent: rewards taken, saves to follow, the reload.
fn take_inbox(
    mut p: ResMut<CoopProgress>,
    script: Option<ResMut<ScriptState>>,
    mut corner: Option<ResMut<crate::corner_messages::CornerMessages>>,
    mut load: MessageWriter<LoadRequest>,
    mut quest_lines: Option<ResMut<crate::quest_messages::QuestMessages>>,
    (mut sim, solo): (Option<ResMut<crate::sim::SimState>>, Option<Res<crate::save::SoloBase>>),
) {
    let p = &mut *p;
    let mut script = script;
    // The host by its name in the lobby (nvforever `coop`).
    let host_name = p.peer_names.get(&0).filter(|n| !n.is_empty()).cloned();
    let now = std::time::Instant::now();
    if p.role != Role::Guest {
        p.unshared_told.clear();
    }
    // The host's catch-up for this player ([`Returns`]): taken as if heard now, in order.
    let mut caught_up = 0;
    let (role, me) = (p.role, p.my_id);
    let inbox: Vec<(u8, Shared)> = std::mem::take(&mut p.inbox)
        .into_iter()
        .filter_map(|(id, w)| match w {
            Shared::For { to, what } if id == 0 && role == Role::Guest && me == Some(to) => {
                caught_up += 1;
                Some((0, *what))
            }
            Shared::For { .. } => None,
            w => Some((id, w)),
        })
        .collect();
    // Stage offers held until this game is kept as the guest's own: first, in order.
    let kept = solo.is_some();
    let mut inbox = inbox;
    if role != Role::Guest {
        p.held.clear();
        p.held_since = None;
    } else if kept && !p.held.is_empty() {
        info!("co-op: {} held stage offers taken now this game is kept as its own", p.held.len());
        let mut all: Vec<(u8, Shared)> = std::mem::take(&mut p.held).into_iter().map(|w| (0, w)).collect();
        all.append(&mut inbox);
        inbox = all;
        p.held_since = None;
    }
    if caught_up > 0 {
        let host = host_name.as_deref().unwrap_or(tr("The host"));
        say(&mut corner, trf("Back with {host}: caught up on what you missed", &[("host", &host)]));
        info!("co-op: {caught_up} catch-up messages from the host");
    }
    for (id, what) in inbox {
        match what {
            Shared::Reward { quest, xp, caps } if p.role == Role::Guest && id == 0 => {
                let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else {
                    warn!("co-op: a quest reward came before the scripts run: {xp} XP, {caps} caps lost");
                    continue;
                };
                let name = rt.game.data.name(FormId(quest));
                let host = host_name.as_deref().unwrap_or("The host").to_string();
                // Sync off, or the quest's line sat out (docs/design/coop-quest-sync.md §3).
                if !rt.game.coop_takes_quest((FormId(quest) != fnv_game::coop_rewards::SCRIPT_XP).then_some(FormId(quest))) {
                    info!("co-op: the host's reward for {name} ({xp} XP, {caps} caps): not taken, this player does not sync it");
                    continue;
                }
                // Completed in this player's own game: its reward was this player's then
                // (`fnv_game::coop_rewards::REPEAT_DONE_REWARDS`).
                if rt.game.coop_reward_done_here(FormId(quest)) {
                    info!("co-op: the host completed {name} ({xp} XP, {caps} caps): this player completed it in its own game");
                    continue;
                }
                // Into the played world (`crate::script::with_world`): outside it the
                // runtime holds a stand-in, whose caps no menu shows (a guest's +50 caps
                // never reached its Pip-Boy [design: a found bug]).
                let taken = match sim.as_deref_mut() {
                    Some(sim) => crate::script::with_world(rt, sim, |rt| rt.game.take_coop_reward(xp as f64, caps as i32)),
                    None => Err("no simulation world".to_string()),
                };
                match taken {
                    // Script XP no completion claimed (`fnv_game::coop_rewards::SCRIPT_XP`).
                    Ok(gained) if FormId(quest) == fnv_game::coop_rewards::SCRIPT_XP => say(&mut corner, trf("+{xp} XP, earned with {host}", &[("xp", &gained), ("host", &host)])),
                    // The host by its character's name (its name in the lobby).
                    // The quest's COMPLETED line came with its own message ([`Shared::QuestEnded`]).
                    // By the quest's shown name (its FULL), never its editor ID ("VMS19" on
                    // screen [design: a found bug]); a quest with none: "a quest".
                    Ok(gained) => {
                        let shown = rt.game.data.quests.get(&FormId(quest)).and_then(|d| d.name.clone()).filter(|n| !n.trim().is_empty());
                        say(&mut corner, reward_line(&host, shown.as_deref(), gained, caps))
                    }
                    Err(e) => warn!("co-op: the reward for {name} could not be given: {e}"),
                }
            }
            Shared::Gift { batch, item, count, quest, stage } if p.role == Role::Guest && id == 0 => {
                let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else {
                    warn!("co-op: a hand-out came before the scripts run: {count} of {} lost", FormId(item));
                    continue;
                };
                let name = item_label(&rt.game.data.full_names, FormId(item), || rt.game.data.name(FormId(item)));
                if !rt.game.coop_takes_quest((quest != 0).then_some(FormId(quest))) {
                    info!("co-op: the host's hand-out {name} ({count}): not taken, this player does not sync {}", rt.game.data.name(FormId(quest)));
                    continue;
                }
                // Caravan cards (Ringo's deck: 54 of them) join the player's cards; one
                // corner line for the first, not one per card.
                let card = rt.game.data.caravan_cards.contains_key(&FormId(item));
                let first_card = card && rt.game.player.caravan.cards.is_empty();
                let gift = fnv_game::coop_rewards::Gift { batch, item: FormId(item), count: count as i32, quest: (quest != 0).then_some(FormId(quest)), stage: (stage != u32::MAX).then_some(stage as i32) };
                let done_here = rt.game.coop_gift_done_here(&gift);
                // Into the played world, as the reward above; an online guest keeps its
                // own.
                let every = false;
                let taken = match sim.as_deref_mut() {
                    Some(sim) => crate::script::with_world(rt, sim, |rt| rt.game.take_coop_gift_as(&gift, &mut p.gift_batch, every)),
                    None => Err("no simulation world".to_string()),
                };
                match taken {
                    Ok(0) if done_here => info!("co-op: the host's hand-out {name} ({count}): this player did {} in its own game", rt.game.data.name(FormId(quest))),
                    Ok(0) => info!("co-op: the host's hand-out {name} ({count}): this player has its own"),
                    Ok(_) if card && !first_card => {}
                    Ok(_) if card => say(&mut corner, trf("{host} handed out Caravan cards", &[("host", &host_name.as_deref().unwrap_or(tr("The host")))])),
                    Ok(n) => {
                        debug!("co-op: the host's hand-out {name}: {n} taken (batch {batch})");
                        p.hand_outs.add(now, item, name, n);
                    }
                    Err(e) => warn!("co-op: the hand-out {name} could not be given: {e}"),
                }
            }
            // The host's quest completed or failed: listed as done, and its line, in the
            // order the host's quests ended (design: the host showed 10
            // failed quests, the guest none; another: VMS39, completed and failed a second
            // apart, showed neither). Come Fly With Me, not the host's active
            // quest, showed no COMPLETED line on the guest before such lines were sent.
            Shared::QuestEnded { quest, failed } if p.role == Role::Guest && id == 0 => {
                let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else {
                    warn!("co-op: the host's quest {} ended before the scripts run", FormId(quest));
                    continue;
                };
                let name = rt.game.data.name(FormId(quest));
                let how = if failed { "failed" } else { "completed" };
                if !rt.game.coop_host_quest_ended(FormId(quest), failed) {
                    info!("co-op: the host {how} {name}: told already");
                    continue;
                }
                // Listed as the host's world has it, but not this player's quest to be told
                // of (docs/design/coop-quest-sync.md §3).
                if !rt.game.coop_takes_quest(Some(FormId(quest))) {
                    info!("co-op: the host {how} {name}: no line, this player does not sync it");
                    continue;
                }
                let quest_name = rt.game.data.quests.get(&FormId(quest)).and_then(|q| q.name.clone()).filter(|n| !n.trim().is_empty());
                match (quest_lines.as_deref_mut(), quest_name) {
                    (Some(lines), Some(qn)) => {
                        let n = if failed { crate::script_events::QuestNotice::Failed(qn) } else { crate::script_events::QuestNotice::Completed(qn) };
                        if !lines.holds(&n) {
                            info!("co-op: the host's quest: {}", crate::quest_messages::describe(&n));
                            lines.push(n);
                        }
                    }
                    // A quest the Pip-Boy does not list (no name) shows no line in the
                    // host's game either (docs/spec-notes/quest-messages.md §5).
                    _ => info!("co-op: the host {how} {name}: no quest line"),
                }
            }
            Shared::Save { kind, .. } if p.role != Role::Solo => {
                info!("co-op: player {} saved ({kind:?}): this game saves too", id + 1);
                p.saves.heard_save(kind);
            }
            Shared::Reload { .. } if p.role == Role::Guest && id == 0 => match p.saves.last.clone() {
                Some(path) => {
                    say(&mut corner, tr("Everyone is down: back to the last shared save").into());
                    // The save is this game's own spot: the new run goes beside the host
                    // (nvforever `coop::rejoin`).
                    std::env::set_var(REJOIN_ENV, "1");
                    std::env::set_var(SYNC_ENV, "1");
                    load.write(LoadRequest { path });
                }
                None => say(&mut corner, tr("Everyone is down, and this game has no shared save to go back to").into()),
            },
            // Joint progression (`fnv_game::coop_joint`; coop-shared-progress.md §3).
            Shared::JointChoice { character, on, sat_out } if p.role == Role::Host && id != 0 => {
                let out = fnv_game::coop_questlines::names(sat_out);
                let sitting = if out.is_empty() { String::new() } else { format!(", sitting out {}", out.join(", ")) };
                info!("co-op: player {} shares quest progress: {} (character {character:016x}){sitting}", id + 1, if on { "yes" } else { "no" });
                let gone: Vec<u8> = p.joint_peers.iter().filter(|(other, (c, _))| **other != id && *c == character).map(|(o, _)| *o).collect();
                for o in gone {
                    p.joint_peers.remove(&o);
                    p.joint_sat_out.remove(&o);
                }
                p.joint_peers.insert(id, (character, on));
                p.joint_sat_out.insert(id, sat_out);
            }
            Shared::StageResult { quest, stage, taken } if p.role == Role::Host => {
                info!("co-op: player {} {} {} stage {stage}", id + 1, if taken { "claimed" } else { "turned down" }, FormId(quest));
            }
            Shared::Stage { quest, stage, line, line_end, prior, to } if p.role == Role::Guest && id == 0 => {
                if !p.my_id.is_some_and(|m| m < 8 && to & (1 << m) != 0) || p.joint_on != Some(true) {
                    continue;
                }
                // Not kept as its own yet (just joined): held, in order, until it is.
                if !kept || script.as_deref().is_none_or(|s| s.rt.is_none()) {
                    let since = *p.held_since.get_or_insert(now);
                    if now.saturating_duration_since(since) < HOLD_FOR {
                        p.held.push(Shared::Stage { quest, stage, line, line_end, prior, to });
                    } else {
                        warn!("co-op: the host set {} stage {stage}: this game was not kept as its own in {} s, so nothing is claimed", FormId(quest), HOLD_FOR.as_secs());
                    }
                    continue;
                }
                let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { continue };
                // The guest's own game as it was at join (`save::SoloBase`): its quests decide.
                let Some(own) = solo.as_deref().and_then(|s| s.0.as_deref()).map(|s| &s.game.quests) else {
                    warn!("co-op: the host set {} stage {stage}: this game's own save is not kept, so nothing is claimed", FormId(quest));
                    continue;
                };
                let q = FormId(quest);
                // A line this player sits out: its own save keeps its own stage (the host
                // leaves it out of the offer; one sent before the choice reached it ends here).
                if !rt.game.coop_takes_quest(Some(q)) {
                    info!("co-op: the host set {} stage {stage}: not claimed (this player sits its line out)", rt.game.data.name(q));
                    p.outbox.push(Shared::StageResult { quest, stage, taken: false });
                    continue;
                }
                let offer = fnv_game::coop_joint::StageOffer { quest: q, stage, prior, line: (line != 0).then_some((FormId(line), line_end)), to: Vec::new() };
                let name = rt.game.data.quests.get(&q).and_then(|d| d.name.clone()).filter(|n| !n.trim().is_empty());
                let taken = rt.game.take_joint_offer(own, &offer);
                match &taken {
                    Ok(()) => {
                        info!("co-op: claimed {} stage {stage} for this character's own game", rt.game.data.name(q));
                        let first = rt.game.player.claims.stages.get(&q).is_some_and(|s| s.len() == 1);
                        if let (true, Some(n)) = (first, name) {
                            say(&mut corner, trf("Shared quest progress: {quest}", &[("quest", &n)]));
                        }
                    }
                    Err(why) => {
                        info!("co-op: the host set {} stage {stage}: not claimed ({why})", rt.game.data.name(q));
                        // The host went on without this player [design]: told
                        // once per quest; rewards still come, the quest is not shared.
                        if let (fnv_game::coop_joint::TurnedDown::Missed(_), Some(n)) = (why, name) {
                            if p.unshared_told.insert(quest) {
                                let host = host_name.as_deref().unwrap_or(tr("The host"));
                                say(&mut corner, trf("{host} went on without you: {quest} is no longer shared. You still get XP and caps", &[("host", &host), ("quest", &n)]));
                            }
                        }
                    }
                }
                p.outbox.push(Shared::StageResult { quest, stage, taken: taken.is_ok() });
            }
            Shared::KillCredit { to, target, target_base, npc, xp, karma, killer, infamy } if p.role == Role::Guest && id == 0 && p.my_id == Some(to) => {
                let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { continue };
                let c = fnv_game::coop_kills::KillCredit {
                    player: FormId::PLAYER_REF,
                    target: FormId(target),
                    target_base: (target_base != 0).then_some(FormId(target_base)),
                    npc,
                    xp: f64::from(xp),
                    karma: f64::from(karma),
                    killer,
                    infamy: infamy.iter().map(|(r, d)| (FormId(*r), f64::from(*d))).collect(),
                };
                let got = rt.game.take_coop_kill_credit(&c);
                info!("co-op: kill credit for {}: +{got} XP, karma {karma:+}{}", FormId(target), if killer { " (the kill)" } else { "" });
                if got > 0.0 {
                    say(&mut corner, trf("+{xp} XP", &[("xp", &got)]));
                }
            }
            Shared::HitCredit { to, target, weapon, part, dealt, critical, crippled } if p.role == Role::Guest && id == 0 && p.my_id == Some(to) => {
                let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { continue };
                let h = fnv_game::coop_kills::HitCredit {
                    player: FormId::PLAYER_REF,
                    target: FormId(target),
                    weapon: (weapon != 0).then_some(FormId(weapon)),
                    part: (part != u32::MAX).then_some(part),
                    dealt: f64::from(dealt),
                    critical,
                    crippled,
                };
                rt.game.take_coop_hit_credit(&h);
            }
            other => debug!("co-op: dropped {other:?} from player {}", id + 1),
        }
    }
    let host = host_name.as_deref().unwrap_or(tr("The host"));
    for line in p.hand_outs.due(host, now) {
        say(&mut corner, line);
    }
}

/// The corner lines of hand-outs taken together (item, name, count given; [`HandOuts`]):
/// one per item, its counts added up, in the order the items first came. A script that adds a
/// stack a round at a time (Doc Mitchell's 9mm rounds: 25 AddItem calls of 1 [observed: buckallobill, in game, FNV 1.4.0.525])
/// was one uncounted "9mm Round" line. The count in brackets after the name, as this
/// game's own item lines have it (`script_events`) [design: the original's wording for a
/// counted hand-out is not in the data files; observer-checks.md].
fn gift_lines(host: &str, handed: &[(u32, String, i32)]) -> Vec<String> {
    let mut order: Vec<(u32, &str, i32)> = Vec::new();
    for (item, name, n) in handed {
        match order.iter_mut().find(|(i, _, _)| i == item) {
            Some(o) => o.2 += n,
            None => order.push((*item, name, *n)),
        }
    }
    order.into_iter().map(|(_, name, n)| if n > 1 { trf("{host} handed out {item} ({n})", &[("host", &host), ("item", &name), ("n", &n)]) } else { trf("{host} handed out {item}", &[("host", &host), ("item", &name)]) }).collect()
}

/// `NVF_COOP_JOINT=1` / `0`: the join question answered without asking (test runs).
pub const JOINT_ENV: &str = "NVF_COOP_JOINT";

/// A guest's quest progression sync (docs/design/coop-quest-sync.md §1). The join
/// question (coop-shared-progress.md §3.1), asked once the syncing screen is down
/// ([`CoopProgress::joint_ask`]) and answered ([`CoopProgress::joint_answer`]), is kept
/// with the character (`QuestSync::on`): a character that answered is not asked again, and
/// the lobby's toggle ([`quest_sync_asks`]) changes it at any time. A host or a solo game
/// forgets this run's answer.
fn joint_question(mut p: ResMut<CoopProgress>, script: Option<ResMut<ScriptState>>) {
    let mut script = script;
    let rt = script.as_deref_mut().and_then(|s| s.rt.as_mut());
    if p.role != Role::Guest {
        p.joint_on = None;
        p.joint_env_taken = false;
        p.joint_ask = false;
        p.joint_answer = None;
        return;
    }
    // The test aid answers the question once a join, as a pressed answer would; the
    // lobby's toggle changes the choice after that.
    if !p.joint_env_taken && rt.is_some() {
        if let Some(on) = std::env::var(JOINT_ENV).ok().filter(|v| v == "0" || v == "1").map(|v| v == "1") {
            info!("co-op: share quest progress: {} ({JOINT_ENV})", if on { "yes" } else { "no" });
            p.joint_env_taken = true;
            p.joint_answer = Some(on);
        }
    }
    if let Some(yes) = p.joint_answer.take() {
        info!("co-op: share quest progress: {}", if yes { "yes" } else { "no" });
        p.joint_on = Some(yes);
        p.joint_ask = false;
        match rt {
            Some(rt) => rt.game.player.quest_sync.on = Some(yes),
            None => warn!("co-op: share quest progress answered before the scripts run: for this session only"),
        }
        return;
    }
    // The character's own choice (an earlier answer, or the lobby's toggle just now).
    if let Some(on) = rt.as_ref().and_then(|rt| rt.game.player.quest_sync.on) {
        if p.joint_on != Some(on) {
            info!("co-op: share quest progress: {} (this character's choice)", if on { "yes" } else { "no" });
            p.joint_on = Some(on);
        }
        p.joint_ask = false;
        return;
    }
    if p.joint_on.is_some() {
        return;
    }
    if !p.joint_ask && !p.syncing && p.my_id.is_some() {
        info!("co-op: asking whether to share quest progress this session");
        p.joint_ask = true;
    }
}

/// The lobby's Quest Sync toggle and Questlines menu (`crate::lobby`): kept on this
/// guest's character (docs/design/coop-quest-sync.md §1), from where [`joint_question`] and
/// [`rewards`] take them.
fn quest_sync_asks(mut asks: MessageReader<crate::lobby::LobbyRequest>, p: Res<CoopProgress>, script: Option<ResMut<ScriptState>>) {
    let mut script = script;
    for ask in asks.read() {
        let (on, line) = match ask {
            crate::lobby::LobbyRequest::QuestSync(on) => (Some(*on), None),
            crate::lobby::LobbyRequest::SitOut { line, out } => (None, Some((*line, *out))),
            _ => continue,
        };
        if p.role != Role::Guest {
            continue;
        }
        let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else {
            warn!("co-op: quest progression sync changed before the scripts run: not kept");
            continue;
        };
        let sync = &mut rt.game.player.quest_sync;
        if let Some(on) = on {
            info!("co-op: quest progression sync {} (the lobby)", if on { "on" } else { "off" });
            sync.on = Some(on);
        }
        if let Some((i, out)) = line {
            sync.set_sat_out(i, out);
            let name = fnv_game::coop_questlines::LINES.get(i).map_or("?", |l| l.name);
            info!("co-op: questline {name}: {}", if out { "sitting out" } else { "synced" });
        }
    }
}

/// What the lobby's guest rows show ([`crate::lobby::QuestSyncView`]), while it is open:
/// the choice, and the shared quests part way through for its warning (design §4).
fn quest_sync_view(
    p: Res<CoopProgress>,
    screen: Option<Res<crate::lobby::LobbyScreen>>,
    view: Option<ResMut<crate::lobby::QuestSyncView>>,
    script: Option<Res<ScriptState>>,
    solo: Option<Res<crate::save::SoloBase>>,
) {
    let Some(mut view) = view else { return };
    let open = screen.is_some_and(|s| s.is_open());
    let rt = script.as_deref().and_then(|s| s.rt.as_ref());
    let next = match rt {
        Some(rt) if open && p.role == Role::Guest => {
            let empty = std::collections::BTreeMap::new();
            let own = solo.as_deref().and_then(|s| s.0.as_deref()).map_or(&empty, |s| &s.game.quests);
            let busy = rt.game.coop_sync_in_progress(own, 0);
            let mut busy_by_line = Vec::new();
            if !busy.is_empty() {
                for i in 0..fnv_game::coop_questlines::LINES.len() {
                    let n = rt.game.coop_sync_in_progress(own, fnv_game::coop_questlines::bit(i));
                    if !n.is_empty() {
                        busy_by_line.push((i, n));
                    }
                }
            }
            crate::lobby::QuestSyncView { guest: true, on: p.joint_on.or(rt.game.player.quest_sync.on), sat_out: rt.game.player.quest_sync.mask(), busy, busy_by_line }
        }
        _ => crate::lobby::QuestSyncView::default(),
    };
    view.set_if_neq(next);
}

/// The host: rewards of the quests it completed, for the guests.
fn rewards(mut p: ResMut<CoopProgress>, script: Option<ResMut<ScriptState>>, mut corner: Option<ResMut<crate::corner_messages::CornerMessages>>) {
    let Some(mut st) = script else { return };
    let Some(rt) = st.rt.as_mut() else { return };
    let host = p.role == Role::Host;
    if rt.game.coop_rewards.on != host {
        rt.game.coop_rewards = fnv_game::coop_rewards::RewardLedger::default();
        rt.game.coop_rewards.on = host;
    }
    // Joint progression (`fnv_game::coop_joint`): a guest tells the host its choice and
    // character once; the host keeps the game's guest list current and sends its offers.
    if p.role != Role::Guest {
        p.joint_sent = false;
        p.joint_sent_as = None;
        p.joint_sent_at = None;
    }
    if p.role != Role::Host {
        p.joint_peers.clear();
        p.joint_sat_out.clear();
        p.returns = Returns::default();
    }
    rt.game.coop_joint_on = p.role == Role::Guest && p.joint_on == Some(true);
    // Sent once, and again whenever the lobby changes it (docs/design/coop-quest-sync.md §5).
    if p.role == Role::Guest && p.my_id.is_some() {
        if let Some(on) = p.joint_on {
            let sat_out = rt.game.player.quest_sync.mask();
            let now = std::time::Instant::now();
            if joint_choice_due(p.joint_sent, p.joint_sent_as, (on, sat_out), p.joint_sent_at, now) {
                let character = rt.game.ensure_character_id();
                p.outbox.push(Shared::JointChoice { character, on, sat_out });
                p.joint_sent = true;
                p.joint_sent_as = Some((on, sat_out));
                p.joint_sent_at = Some(now);
            }
        }
    }
    if !host {
        rt.game.coop_joint_away = false;
        return;
    }
    // Players that dropped and came back ([`Returns`]).
    let now = std::time::Instant::now();
    {
        let p = &mut *p;
        let (gone, back) = p.returns.track(&p.joint_peers, &p.connected, &p.peer_names, now);
        for n in gone {
            say(&mut corner, trf("{name} dropped: what they miss reaches them when they are back", &[("name", &n)]));
        }
        for b in back {
            info!("co-op: {} is back: {} catch-up messages", b.name, b.items.len());
            if !b.items.is_empty() {
                say(&mut corner, trf("{name} is back: caught up on what they missed", &[("name", &b.name)]));
            }
            p.outbox.extend(b.items);
        }
        // Players that left: their entries go (their character waits in `returns`).
        let present = p.present.clone();
        p.joint_peers.retain(|id, _| present.contains(id));
        p.joint_sat_out.retain(|id, _| present.contains(id));
    }
    rt.game.coop_joint_away = p.returns.away_sharing();
    let before = p.outbox.len();
    rt.game.coop_joint_guests = p
        .joint_peers
        .iter()
        .filter(|(id, _)| p.connected.contains(id))
        .filter_map(|(id, &(character, on))| p.peer_actors.get(id).map(|a| (*a, fnv_game::coop_joint::JointGuest { character, on, sat_out: p.joint_sat_out.get(id).copied().unwrap_or(0) })))
        .collect();
    for o in rt.game.take_joint_offers() {
        let to = o.to.iter().filter_map(|a| p.peer_actors.iter().find(|(_, x)| *x == a).map(|(id, _)| *id)).filter(|id| *id < 8).fold(0u8, |m, id| m | (1 << id));
        info!("co-op: {} stage {}: offered to the players sharing quest progress", rt.game.data.name(o.quest), o.stage);
        let (line, line_end) = o.line.map_or((0, false), |(i, e)| (i.0, e));
        p.outbox.push(Shared::Stage { quest: o.quest.0, stage: o.stage, line, line_end, prior: o.prior, to });
    }
    // Kill and hit credit for the guests (`fnv_game::coop_kills`, coop-shared-progress.md §4.2).
    let id_of = |a: FormId| p.peer_actors.iter().find(|(_, x)| **x == a).map(|(id, _)| *id);
    let mut credits = Vec::new();
    for c in rt.game.take_coop_kill_credits() {
        let Some(to) = id_of(c.player) else { continue };
        credits.push(Shared::KillCredit { to, target: c.target.0, target_base: c.target_base.map_or(0, |b| b.0), npc: c.npc, xp: c.xp.max(0.0).round() as u32, karma: c.karma.round() as i32, killer: c.killer, infamy: c.infamy.iter().map(|(r, d)| (r.0, d.round() as i32)).collect() });
    }
    for h in rt.game.take_coop_hit_credits() {
        let Some(to) = id_of(h.player) else { continue };
        credits.push(Shared::HitCredit { to, target: h.target.0, weapon: h.weapon.map_or(0, |w| w.0), part: h.part.unwrap_or(u32::MAX), dealt: h.dealt.max(0.0).round() as u32, critical: h.critical, crippled: h.crippled });
    }
    p.outbox.extend(credits);
    for g in std::mem::take(&mut rt.game.coop_rewards.gifts) {
        let of = match (g.quest, g.stage) {
            (Some(q), Some(s)) => format!(" by {} stage {s}", rt.game.data.name(q)),
            (Some(q), None) => format!(" by {}", rt.game.data.name(q)),
            _ => String::new(),
        };
        info!("co-op: a script handed the player {} ({}){of}: to the guests too", rt.game.data.name(g.item), g.count);
        p.outbox.push(Shared::Gift { batch: g.batch, item: g.item.0, count: g.count.max(0) as u32, quest: g.quest.map_or(0, |q| q.0), stage: g.stage.map_or(u32::MAX, |s| s as u32) });
    }
    for (q, failed) in std::mem::take(&mut rt.game.coop_rewards.ended) {
        info!("co-op: {} {}: to the guests", rt.game.data.name(q), if failed { "failed" } else { "completed" });
        p.outbox.push(Shared::QuestEnded { quest: q.0, failed });
    }
    for r in rt.game.coop_rewards.due(rt.game.frame) {
        if r.quest == fnv_game::coop_rewards::SCRIPT_XP {
            info!("co-op: {} XP from the host's scripts, no quest completed: to the guests", r.xp);
        } else {
            info!("co-op: {} completed: {} XP and {} caps to the guests", rt.game.data.name(r.quest), r.xp, r.caps);
        }
        p.outbox.push(Shared::Reward { quest: r.quest.0, xp: r.xp.max(0.0) as u32, caps: r.caps.max(0) as u32 });
    }
    // Kept for the players away, and the host warned while one dropped just now.
    if p.returns.away() > 0 {
        let sent: Vec<Shared> = p.outbox[before..].to_vec();
        let name = |q: u32| rt.game.data.quests.get(&FormId(q)).and_then(|d| d.name.clone()).filter(|n| !n.trim().is_empty());
        for w in &sent {
            for line in p.returns.record(w, name, now) {
                say(&mut corner, line);
            }
        }
    }
}

/// This game's saves: announced to the others unless followed or quiet.
fn announce_saves(mut p: ResMut<CoopProgress>, mut done: MessageReader<SaveDone>, cfg: Res<SaveConfig>, time: Res<Time>) {
    let now = time.elapsed_secs_f64();
    for d in done.read() {
        if d.result.is_err() || p.role == Role::Solo {
            continue;
        }
        let slot = |n: &str| cfg.path_of(n).ok();
        let kind = if Some(&d.path) == slot(fnv_save::QUICK_NAME).as_ref() {
            SaveKind::Quick
        } else if Some(&d.path) == slot(fnv_save::AUTO_NAME).as_ref() {
            SaveKind::Auto
        } else if Some(&d.path) == slot(fnv_save::SYSTEM_NAME).as_ref() {
            SaveKind::System
        } else {
            SaveKind::Manual
        };
        let what = p.saves.saved(&d.path, matches!(kind, SaveKind::Auto | SaveKind::System), now);
        if what != Announce::Quiet {
            std::env::set_var(SHARED_ENV, &d.path);
        }
        if what == Announce::Tell {
            let stamp = stamp();
            info!("co-op: saved {} ({kind:?}): every game saves too (shared save {stamp:08x})", d.path.display());
            p.outbox.push(Shared::Save { stamp, kind });
        }
    }
}

/// Saves another player made, written here as soon as saving is allowed.
fn follow_saves(world: &mut World) {
    if world.get_resource::<CoopProgress>().is_none_or(|p| p.saves.waiting.is_empty()) || world.contains_resource::<crate::save::PendingEngine>() {
        return;
    }
    if crate::save::not_now(world).is_some() {
        return;
    }
    let cfg = world.resource::<SaveConfig>().clone();
    let kinds = std::mem::take(&mut world.resource_mut::<CoopProgress>().saves.waiting);
    for kind in kinds {
        let r = follow_request(kind, &cfg);
        match cfg.path_of(&r.name) {
            Ok(path) => {
                info!("save: a co-op save: {:?} because another player saved", r.name);
                world.resource_mut::<CoopProgress>().saves.following(path);
                world.write_message(r);
            }
            Err(e) => warn!("save: a co-op save cannot be made: {e}"),
        }
    }
}

/// What the host's corner says while a story NPC waits for the other players
/// (`fnv_game::coop::StoryWait`): their names.
fn wait_line(names: &[String]) -> String {
    trf("Waiting for {names} to come near", &[("names", &names.join(", "))])
}

/// The host: its talk with a story NPC is held until every other player is near
/// (`Game::story_gate`): the corner says for whom, once for each hold [design].
fn story_wait_notice(mut p: ResMut<CoopProgress>, script: Option<Res<ScriptState>>, mut corner: Option<ResMut<crate::corner_messages::CornerMessages>>) {
    if p.role != Role::Host {
        return;
    }
    let Some(rt) = script.as_deref().and_then(|s| s.rt.as_ref()) else { return };
    let hold = rt.game.coop_story_wait.as_ref().filter(|w| w.shown(rt.game.time));
    let key = hold.map(|w| (w.actor, w.since.to_bits()));
    if key == p.wait_told {
        return;
    }
    p.wait_told = key;
    if let Some(w) = hold {
        let names: Vec<String> = w.far.iter().filter_map(|a| rt.game.coop_actors.get(a).cloned()).collect();
        if !names.is_empty() {
            say(&mut corner, wait_line(&names));
        }
    }
}

/// The host: everyone down for [`DOWN_HOLD`]: every game back to its last shared save.
fn all_down(mut p: ResMut<CoopProgress>, time: Res<Time>, mut corner: Option<ResMut<crate::corner_messages::CornerMessages>>, mut load: MessageWriter<LoadRequest>) {
    let p = &mut *p;
    if p.role != Role::Host {
        return;
    }
    match p.saves.down(p.everyone_down, time.elapsed_secs_f64()) {
        DownStep::Wait => {}
        DownStep::TellReload => {
            say(&mut corner, tr("Everyone is down: back to the last shared save").into());
            p.outbox.push(Shared::Reload { stamp: stamp() });
        }
        DownStep::Load(path) => {
            info!("co-op: everyone down: loading the last shared save {}", path.display());
            load.write(LoadRequest { path });
        }
        DownStep::NoSave => say(&mut corner, tr("Everyone is down, and there is no shared save to go back to").into()),
    }
}

/// A guest down with nobody near: its own last save (module notes).
fn alone_down(mut p: ResMut<CoopProgress>, time: Res<Time>, mut corner: Option<ResMut<crate::corner_messages::CornerMessages>>, mut load: MessageWriter<LoadRequest>) {
    let p = &mut *p;
    let alone = p.role == Role::Guest && p.alone_down;
    match p.saves.alone(alone, time.elapsed_secs_f64()) {
        DownStep::Load(path) => {
            say(&mut corner, tr("Down with nobody near: back to your last save").into());
            info!("co-op: down far from the other players: loading this game's last save {}", path.display());
            std::env::set_var(SYNC_ENV, "1");
            load.write(LoadRequest { path });
        }
        DownStep::NoSave => say(&mut corner, tr("Down with nobody near, and no save to go back to: respawn by another player").into()),
        _ => {}
    }
}

/// The least time between a guest's [`Shared::JointChoice`] messages: the host's
/// `JOINT_RATE` is a burst of 2 and then one a second, and a message over it is dropped.
const JOINT_EVERY: std::time::Duration = std::time::Duration::from_secs(1);

/// Whether a guest sends its joint choice now: it has not sent one, or the lobby changed
/// what it last sent, and the last one went at least [`JOINT_EVERY`] ago. A change that
/// comes sooner is held (the choice read each frame is the latest) and goes when due, so
/// the host never drops one and the guest never records one as sent that was not.
fn joint_choice_due(sent: bool, sent_as: Option<(bool, u64)>, want: (bool, u64), sent_at: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    let changed = !sent || sent_as != Some(want);
    changed && sent_at.is_none_or(|at| now.duration_since(at) >= JOINT_EVERY)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fast toggles of the lobby's Quest Sync: the guest sends at most one a second, the
    /// latest one goes when due, and each is marked sent only when it goes.
    #[test]
    fn a_guests_fast_toggles_send_the_latest_once_a_second() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let (mut sent, mut sent_as, mut sent_at) = (false, None, None);
        let mut went = Vec::new();
        // The lobby changes its choice every 100 ms for 3 s.
        for step in 0..30u64 {
            let want = (step % 2 == 0, step);
            let now = at(step * 100);
            if joint_choice_due(sent, sent_as, want, sent_at, now) {
                sent = true;
                sent_as = Some(want);
                sent_at = Some(now);
                went.push((step * 100, want.1));
            }
        }
        assert_eq!(went, vec![(0, 0), (1000, 10), (2000, 20)], "one a second, and the choice read when it goes");
        // A change held at the end of the second goes at once when it is due, then is not resent.
        let want = (false, 30);
        assert!(!joint_choice_due(sent, sent_as, want, sent_at, at(2900)));
        assert!(joint_choice_due(sent, sent_as, want, sent_at, at(3000)));
        assert!(!joint_choice_due(true, Some(want), want, Some(at(3000)), at(9000)), "unchanged: nothing to send");
    }

    fn path(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn one_line_per_item_handed_out_with_its_count() {
        let mut handed = vec![(0x15, "Stimpak".to_string(), 4), (0x0A, "Bobby Pin".to_string(), 6), (0xE3778, "9mm Pistol".to_string(), 1)];
        // Doc Mitchell's 9mm rounds: one AddItem of 1 each, 25 times.
        handed.extend(std::iter::repeat_n((0x4241, "9mm Round".to_string(), 1), 25));
        assert_eq!(
            gift_lines("Host", &handed),
            ["Host handed out Stimpak (4)", "Host handed out Bobby Pin (6)", "Host handed out 9mm Pistol", "Host handed out 9mm Round (25)"]
        );
        // An item that comes again later in the same inbox joins its first line.
        let again = [(1, "A".to_string(), 1), (2, "B".to_string(), 2), (1, "A".to_string(), 2)];
        assert_eq!(gift_lines("Host", &again), ["Host handed out A (3)", "Host handed out B (2)"]);
        assert!(gift_lines("Host", &[]).is_empty());
    }

    #[test]
    fn a_stack_over_two_frames_is_one_line() {
        let t0 = std::time::Instant::now();
        let ms = |n: u64| t0 + std::time::Duration::from_millis(n);
        let mut h = HandOuts::default();
        h.add(t0, 0xE3778, "9mm Pistol".into(), 1);
        for _ in 0..7 {
            h.add(t0, 0x4241, "9mm Round".into(), 1);
        }
        // The next frame: still waiting for more.
        assert!(h.due("Host", ms(5)).is_empty());
        for _ in 0..15 {
            h.add(ms(5), 0x4241, "9mm Round".into(), 1);
        }
        assert!(h.due("Host", ms(400)).is_empty(), "under HOLD since the last piece");
        assert_eq!(h.due("Host", ms(505)), ["Host handed out 9mm Pistol", "Host handed out 9mm Round (22)"]);
        assert!(h.due("Host", ms(2000)).is_empty(), "told once");
    }

    #[test]
    fn hand_outs_show_the_items_name_not_its_editor_id() {
        let mut full = std::collections::HashMap::new();
        full.insert(FormId(0x000E3778), "9mm Pistol".to_string());
        full.insert(FormId(0x0000000F), String::new());
        assert_eq!(item_label(&full, FormId(0x000E3778), || "WeapNV9mmPistol".into()), "9mm Pistol");
        // No name to show: the Editor ID, as before.
        assert_eq!(item_label(&full, FormId(0x00001234), || "SomeItem".into()), "SomeItem");
        assert_eq!(item_label(&full, FormId(0x0000000F), || "Caps001".into()), "Caps001");
    }

    #[test]
    fn own_saves_are_told_followed_ones_are_not() {
        let mut s = SharedSaves::default();
        assert_eq!(s.saved(&path("a.nvs"), false, 0.0), Announce::Tell);
        assert_eq!(s.last, Some(path("a.nvs")));
        // Another player saved: this game follows, and does not tell again.
        s.heard_save(SaveKind::Quick);
        s.heard_save(SaveKind::Quick);
        assert_eq!(s.waiting, [SaveKind::Quick], "one save however often heard");
        s.following(path("quick.nvs"));
        assert_eq!(s.saved(&path("quick.nvs"), false, 100.0), Announce::Followed);
        assert_eq!(s.last, Some(path("quick.nvs")));
        // The same slot written again by this player's own hand: told.
        assert_eq!(s.saved(&path("quick.nvs"), false, 101.0), Announce::Tell);
    }

    #[test]
    fn autosaves_right_after_a_shared_save_are_quiet() {
        let mut s = SharedSaves::default();
        // The first autosave of a session is told.
        assert_eq!(s.saved(&path("auto.nvs"), true, 0.0), Announce::Tell);
        // The trip's autosave in each game: not another round.
        assert_eq!(s.saved(&path("auto.nvs"), true, 5.0), Announce::Quiet);
        // A manual save is always told.
        assert_eq!(s.saved(&path("Save 3.nvs"), false, 6.0), Announce::Tell);
        assert_eq!(s.last, Some(path("Save 3.nvs")));
        assert_eq!(s.saved(&path("auto.nvs"), true, 6.0 + AUTO_QUIET), Announce::Tell);
    }

    #[test]
    fn a_guest_down_alone_loads_its_last_save_once() {
        let mut s = SharedSaves { last: Some(path("q.nvs")), ..Default::default() };
        assert_eq!(s.alone(true, 0.0), DownStep::Wait);
        assert_eq!(s.alone(true, ALONE_HOLD - 0.1), DownStep::Wait);
        assert_eq!(s.alone(true, ALONE_HOLD), DownStep::Load(path("q.nvs")));
        assert_eq!(s.alone(true, ALONE_HOLD + 1.0), DownStep::Wait, "once");
        // Revived (or someone came near): the watch starts over.
        assert_eq!(s.alone(false, 20.0), DownStep::Wait);
        assert_eq!(s.alone(true, 21.0), DownStep::Wait);
        let mut n = SharedSaves::default();
        n.alone(true, 0.0);
        assert_eq!(n.alone(true, ALONE_HOLD), DownStep::NoSave);
    }

    #[test]
    fn everyone_down_needs_every_player() {
        assert!(everyone_down(true, [(true, false), (false, true)]), "a lost player cannot revive");
        assert!(!everyone_down(true, [(true, false), (false, false)]));
        assert!(!everyone_down(false, [(true, false)]));
        assert!(!everyone_down(true, []), "alone: not a co-op wipe");
    }

    #[test]
    fn the_waiting_line_names_the_players_to_come() {
        assert_eq!(wait_line(&["Ann".to_string()]), "Waiting for Ann to come near");
        assert_eq!(wait_line(&["Ann".to_string(), "Bo".to_string()]), "Waiting for Ann, Bo to come near");
    }

    #[test]
    fn all_down_tells_then_loads_once() {
        let mut s = SharedSaves::default();
        s.saved(&path("shared.nvs"), false, 0.0);
        assert_eq!(s.down(true, 10.0), DownStep::Wait);
        assert_eq!(s.down(true, 10.0 + DOWN_HOLD - 0.1), DownStep::Wait);
        // A revive in time: the watch starts again.
        assert_eq!(s.down(false, 12.0), DownStep::Wait);
        assert_eq!(s.down(true, 20.0), DownStep::Wait);
        assert_eq!(s.down(true, 20.0 + DOWN_HOLD), DownStep::TellReload);
        assert_eq!(s.down(true, 20.0 + DOWN_HOLD + 0.1), DownStep::Wait, "the message goes out first");
        assert_eq!(s.down(true, 20.0 + DOWN_HOLD + RELOAD_DELAY), DownStep::Load(path("shared.nvs")));
        assert_eq!(s.down(true, 30.0), DownStep::Wait, "once");
    }

    #[test]
    fn all_down_without_a_shared_save_says_so_once() {
        let mut s = SharedSaves::default();
        s.down(true, 0.0);
        assert_eq!(s.down(true, DOWN_HOLD), DownStep::NoSave);
        assert_eq!(s.down(true, DOWN_HOLD + 1.0), DownStep::Wait);
    }

    #[test]
    fn a_followed_manual_save_gets_a_new_name() {
        let cfg = SaveConfig { dir: None, autosave_travel: true, autosave_fast_travel: true, autosave_wait: true, autosave_sleep: true, autosave_scripts: true };
        assert_eq!(follow_request(SaveKind::Quick, &cfg).name, fnv_save::QUICK_NAME);
        assert_eq!(follow_request(SaveKind::Auto, &cfg).kind, SaveKind::Auto);
        assert_eq!(follow_request(SaveKind::Manual, &cfg).kind, SaveKind::Manual);
    }

    /// A player that drops gets, when back, what the host sent meanwhile, for it alone
    /// [design]; the host is warned once per quest in the first minute only.
    #[test]
    fn a_returning_player_gets_what_it_missed_and_the_host_is_warned_in_the_first_minute() {
        use std::collections::{BTreeMap, BTreeSet};
        use std::time::Duration;
        let t0 = std::time::Instant::now();
        let mut r = Returns::default();
        let joint: BTreeMap<u8, (u64, bool)> = [(1, (0xAAAA, true)), (2, (0xBBBB, true))].into();
        let names: BTreeMap<u8, String> = [(1, "Rowan".to_string()), (2, "Guest".to_string())].into();
        let both: BTreeSet<u8> = [1, 2].into();
        assert_eq!(r.track(&joint, &both, &names, t0), (vec![], vec![]));
        // Player 1 drops.
        let only2: BTreeSet<u8> = [2].into();
        let (gone, back) = r.track(&joint, &only2, &names, t0);
        assert_eq!((gone, back), (vec!["Rowan".to_string()], vec![]));
        assert!(r.away_sharing(), "its stages are still offered while it is away");
        assert_eq!(r.track(&joint, &only2, &names, t0).0, Vec::<String>::new(), "told once");
        let stage = |s: i32, prior: Vec<i32>| Shared::Stage { quest: 0x21A, stage: s, line: 0, line_end: false, prior, to: 0b100 };
        let name = |q: u32| (q == 0x21A).then(|| "Bye Bye Love".to_string());
        let w = r.record(&stage(40, vec![5, 10, 20, 30]), name, t0 + Duration::from_secs(10));
        assert_eq!(w, vec!["Rowan dropped 10 s ago: they get Bye Bye Love when they are back".to_string()]);
        assert!(r.record(&stage(50, vec![5, 10, 20, 30, 40]), name, t0 + Duration::from_secs(12)).is_empty(), "once per quest");
        assert!(r.record(&Shared::Reward { quest: 0x19, xp: 100, caps: 0 }, name, t0 + Duration::from_secs(70)).is_empty(), "past the minute: no warning");
        assert!(r.record(&Shared::Save { stamp: 1, kind: SaveKind::Auto }, name, t0).is_empty());
        // Back with a new id (a new join, same character): all three, in order, for it alone.
        let joint2: BTreeMap<u8, (u64, bool)> = [(3, (0xAAAA, true)), (2, (0xBBBB, true))].into();
        let names2: BTreeMap<u8, String> = [(3, "Rowan".to_string()), (2, "Guest".to_string())].into();
        let now: BTreeSet<u8> = [2, 3].into();
        let (gone, back) = r.track(&joint2, &now, &names2, t0 + Duration::from_secs(80));
        assert!(gone.is_empty());
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].id, 3);
        let to3 = |w: Shared| Shared::For { to: 3, what: Box::new(w) };
        let st = |s: i32, prior: Vec<i32>| Shared::Stage { quest: 0x21A, stage: s, line: 0, line_end: false, prior, to: 0b1000 };
        assert_eq!(back[0].items, vec![to3(st(40, vec![5, 10, 20, 30])), to3(st(50, vec![5, 10, 20, 30, 40])), to3(Shared::Reward { quest: 0x19, xp: 100, caps: 0 })]);
        assert_eq!(r.away(), 0);
        // Back without sharing quest progress: rewards only.
        let (_, _) = r.track(&joint2, &[2].into(), &names2, t0);
        r.record(&stage(60, vec![]), name, t0);
        r.record(&Shared::QuestEnded { quest: 0x21A, failed: false }, name, t0);
        let off: BTreeMap<u8, (u64, bool)> = [(3, (0xAAAA, false))].into();
        let (_, back) = r.track(&off, &[3].into(), &names2, t0);
        assert_eq!(back[0].items, vec![to3(Shared::QuestEnded { quest: 0x21A, failed: false })]);
    }

    /// A reward's corner line names the quest as the player sees it, never by editor ID.
    #[test]
    fn a_reward_line_names_the_quest_by_its_shown_name() {
        assert_eq!(reward_line("Host", Some("G.I. Blues"), 1200, 0), "Host completed G.I. Blues: +1200 XP");
        assert_eq!(reward_line("Host", Some("Bye Bye Love"), 200, 50), "Host completed Bye Bye Love: +200 XP, +50 caps");
        assert_eq!(reward_line("Host", None, 100, 0), "Host completed a quest: +100 XP");
    }

    /// The join question waits for the syncing screen, takes the answer, and is asked
    /// again after the guest has left (coop-shared-progress.md §3.1).
    #[test]
    fn a_guest_is_asked_once_synced_and_again_after_leaving() {
        if std::env::var(JOINT_ENV).is_ok() {
            return;
        }
        let mut app = App::new();
        app.insert_resource(CoopProgress { role: Role::Guest, my_id: Some(1), syncing: true, ..default() }).add_systems(Update, joint_question);
        let p = |app: &App| -> (bool, Option<bool>) {
            let p = app.world().resource::<CoopProgress>();
            (p.joint_ask, p.joint_on)
        };
        app.update();
        assert_eq!(p(&app), (false, None), "not while syncing");
        app.world_mut().resource_mut::<CoopProgress>().syncing = false;
        app.update();
        assert_eq!(p(&app), (true, None));
        app.world_mut().resource_mut::<CoopProgress>().joint_answer = Some(false);
        app.update();
        assert_eq!(p(&app), (false, Some(false)));
        app.update();
        assert_eq!(p(&app), (false, Some(false)), "answered: not asked again");
        app.world_mut().resource_mut::<CoopProgress>().role = Role::Solo;
        app.update();
        assert_eq!(p(&app).1, None);
        app.world_mut().resource_mut::<CoopProgress>().role = Role::Guest;
        app.update();
        assert!(p(&app).0, "a new join asks again");
    }
}
