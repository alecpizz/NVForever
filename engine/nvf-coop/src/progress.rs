//! Shared progress on the reliable stream ([`crate::rel`], `net::Session::send_reliable`):
//! the host's quest rewards, stage offers, kill credit and story for the guests, and a
//! guest's few answers to the host. Nothing here knows about the game: the game side
//! (`nvf_play::coop_progress`, `fnv_game::coop_joint`) gives and takes the values
//! (docs/design/coop-shared-progress.md).
//!
//! An item is a kind byte ([`kind`], 0x30-0x3F are this module's), then its fields,
//! little-endian. A [`Progress`] item has a tag byte after the kind.
//!
//! **What the host takes from a guest** ([`HostChecks`], one rule per kind in piece 1's
//! guard, run before the host's game sees the item):
//!
//! | Kind | Check |
//! |---|---|
//! | [`kind::SAVE`] | a known save kind; at most one a minute per guest ([`SAVE_RATE`]). The host's game never makes a manual save of it: at most it brings its own autosave forward, once per [`SAVE_FORWARD_EVERY`] ([`SaveForward`]). |
//! | [`kind::JOINT_CHOICE`] | a character ID that is not 0, and one character per guest while it is connected; at most one a second ([`JOINT_RATE`]). |
//! | [`kind::STAGE_RESULT`] | it answers a stage offer the host sent that guest that is still open ([`HostChecks::offered`]); at most ten a second ([`STAGE_RATE`]). |
//! | every other kind of the range | only the host sends it: dropped, counted as a bad message. |
//!
//! Every item must parse to its end with no bytes left over. A guest takes this module's
//! items only from the host ([`from_host`]).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::guard::{Ctx, Kind, Rate, Reason, Rule, Subject, Verdict};
use crate::repl::Reader;
use crate::story::{self, StoryEntry, StoryPager, StoryValue};

/// This module's reliable item kinds.
pub mod kind {
    /// Host to guest: a [`super::Progress`] only the host sends.
    pub const PROGRESS: u8 = 0x30;
    /// Guest to host, or host to guest: [`super::Progress::Save`].
    pub const SAVE: u8 = 0x31;
    /// Guest to host: [`super::Progress::JointChoice`].
    pub const JOINT_CHOICE: u8 = 0x32;
    /// Guest to host: [`super::Progress::StageResult`].
    pub const STAGE_RESULT: u8 = 0x33;
    /// Host to guest: [`super::QuestView`].
    pub const QUEST: u8 = 0x34;
    /// Host to guest: [`super::LevelView`].
    pub const LEVEL: u8 = 0x35;
    /// Host to guest: a page of the host's story ([`crate::story`]).
    pub const STORY: u8 = 0x36;
    /// The range this module holds (0x37-0x3F not in use yet).
    pub const FIRST: u8 = 0x30;
    pub const LAST: u8 = 0x3F;
}

/// How often the host sends its active quest when it has not changed.
pub const QUEST_EVERY: Duration = Duration::from_secs(2);
/// How often the host sends its level and XP when they have not changed.
pub const LEVEL_EVERY: Duration = Duration::from_secs(2);
/// How often the host sends a page of its story.
pub const STORY_EVERY: Duration = Duration::from_millis(200);
/// Objectives a [`QuestView`] carries at most.
pub const QUEST_OBJECTIVES: usize = 64;
/// Stage offers a guest has open at most; the oldest goes first [design].
pub const OFFERS_OPEN: usize = 64;
/// A guest's saves the host takes [design: a minute apart in the long run].
pub const SAVE_RATE: Rate = Rate { burst: 1.0, per_sec: 1.0 / 60.0 };
/// A guest's join choices the host takes [design].
pub const JOINT_RATE: Rate = Rate { burst: 2.0, per_sec: 1.0 };
/// A guest's stage answers the host takes [design: a whole quest's stages at once].
pub const STAGE_RATE: Rate = Rate { burst: 10.0, per_sec: 10.0 };
/// How often a guest's save brings the host's own autosave forward at most [design].
pub const SAVE_FORWARD_EVERY: Duration = Duration::from_secs(120);

/// Shared progression (docs/design/coop-shared-progress.md).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    /// The host completed `quest` (FormID in the load order every game runs): each guest
    /// gets its `xp` (as `RewardXP` gives it) and `caps`.
    Reward { quest: u32, xp: u32, caps: u32 },
    /// The sender saved. From the host: every guest saves its own game the same `kind`
    /// ([`Progress::MANUAL`], [`Progress::QUICK`], [`Progress::AUTO`],
    /// [`Progress::SYSTEM`]). From a guest: the host may bring its autosave forward
    /// ([`SaveForward`]). `stamp` names the save, for the logs.
    Save { stamp: u32, kind: u8 },
    /// Every player is down with nobody left to revive: every game loads its last save.
    Reload { stamp: u32 },
    /// A hand-out of the host's result scripts (`count` of `item`; `batch` groups one
    /// script's): every guest takes it as its rules say (`fnv_game::coop_rewards`).
    /// `quest` (0: none) and `stage` ([`Progress::NO_STAGE`]: none) name the result
    /// script's quest stage, so a guest that did it in its own game keeps its own [design].
    Gift { batch: u32, item: u32, count: u32, quest: u32, stage: u32 },
    /// The host's `quest` ended: completed, or `failed`.
    QuestEnded { quest: u32, failed: bool },
    /// A guest's join choice for joint progression and its character's stable ID
    /// (`fnv_game::coop_joint`), and the questlines it sits out (`sat_out`, a bit per row
    /// of `fnv_game::coop_questlines::LINES`; every player runs the same build).
    /// Guest to host.
    JointChoice { character: u64, on: bool, sat_out: u64 },
    /// The host set `stage` of a claimable `quest` (`fnv_game::coop_joint::StageOffer`),
    /// for the players whose bit is set in `to` (bit = player id). `line` is the INFO whose
    /// result script set it (0: none), its end script when `line_end`; `prior` the host's
    /// earlier stages of the quest.
    Stage { quest: u32, stage: i32, line: u32, line_end: bool, prior: Vec<i32>, to: u8 },
    /// A guest took or turned down a [`Progress::Stage`]. Guest to host.
    StageResult { quest: u32, stage: i32, taken: bool },
    /// Kill credit for player `to`: `xp` of the kill of `target`, its `karma`, and whether
    /// `to` was the killer (kill count, challenges). `infamy`: the guest's own seen-murder
    /// infamy by reputation (whole points).
    KillCredit { to: u8, target: u32, target_base: u32, npc: bool, xp: u32, karma: i32, killer: bool, infamy: Vec<(u32, i32)> },
    /// A guest's own hit, for its hit and damage challenges (`weapon` 0 and `part`
    /// `u32::MAX` for none).
    HitCredit { to: u8, target: u32, weapon: u32, part: u32, dealt: u32, critical: bool, crippled: bool },
    /// `what`, for player `to` alone: the host's catch-up for a player that dropped and
    /// came back, what it would have got had it been there [design]. Never nested.
    For { to: u8, what: Box<Progress> },
}

impl Progress {
    pub const MANUAL: u8 = 0;
    pub const QUICK: u8 = 1;
    pub const AUTO: u8 = 2;
    pub const SYSTEM: u8 = 3;
    /// [`Progress::Gift`]'s stage when the script was no quest stage's.
    pub const NO_STAGE: u32 = u32::MAX;

    /// Only the host sends it.
    pub fn host_only(&self) -> bool {
        !matches!(self, Progress::Save { .. } | Progress::JointChoice { .. } | Progress::StageResult { .. })
    }

    /// Its item kind.
    pub fn kind(&self) -> u8 {
        match self {
            Progress::Save { .. } => kind::SAVE,
            Progress::JointChoice { .. } => kind::JOINT_CHOICE,
            Progress::StageResult { .. } => kind::STAGE_RESULT,
            _ => kind::PROGRESS,
        }
    }

    /// The reliable item.
    pub fn encode(&self) -> Vec<u8> {
        let mut b = vec![self.kind()];
        put_progress(&mut b, self);
        b
    }
}

/// The quest the host has active, as its quest list shows it: the guests show it in place
/// of their own. `quest` 0: none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestView {
    pub quest: u32,
    pub completed: bool,
    pub failed: bool,
    /// The objectives shown or done, by index, with whether each is done (at most
    /// [`QUEST_OBJECTIVES`]).
    pub objectives: Vec<(i32, bool)>,
}

/// The host's player's level and XP (whole XP, as awards give it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LevelView {
    pub level: u16,
    pub xp: u32,
}

/// An item of this module, as [`decode`] reads it.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Progress(Progress),
    Quest(QuestView),
    Level(LevelView),
    Story(Vec<StoryEntry>),
}

/// Whether a reliable item is one of this module's (by its kind byte).
pub fn is_progress_item(item: &[u8]) -> bool {
    item.first().is_some_and(|k| (kind::FIRST..=kind::LAST).contains(k))
}

pub fn encode_quest(q: &QuestView) -> Vec<u8> {
    let mut b = vec![kind::QUEST];
    b.extend_from_slice(&q.quest.to_le_bytes());
    b.push(u8::from(q.completed) | (u8::from(q.failed) << 1));
    let list = &q.objectives[..q.objectives.len().min(QUEST_OBJECTIVES)];
    b.push(list.len() as u8);
    for (i, done) in list {
        b.extend_from_slice(&i.to_le_bytes());
        b.push(u8::from(*done));
    }
    b
}

pub fn encode_level(l: &LevelView) -> Vec<u8> {
    let mut b = vec![kind::LEVEL];
    b.extend_from_slice(&l.level.to_le_bytes());
    b.extend_from_slice(&l.xp.to_le_bytes());
    b
}

/// A page of the host's story ([`StoryPager::page`]; at most [`story::PAGE_BYTES`] of
/// entries, so one item).
pub fn encode_story(entries: &[StoryEntry]) -> Vec<u8> {
    let mut b = vec![kind::STORY];
    b.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for e in entries {
        match e {
            StoryEntry::Quest { quest, stage, running, ended, done, failed, objectives, seen } => {
                b.push(0);
                b.extend_from_slice(&quest.to_le_bytes());
                b.extend_from_slice(&stage.to_le_bytes());
                b.push(u8::from(*running) | (u8::from(*ended) << 1) | (u8::from(*failed) << 2));
                let done = &done[done.len().saturating_sub(story::STAGES)..];
                b.push(done.len() as u8);
                for d in done {
                    b.extend_from_slice(&d.to_le_bytes());
                }
                let objectives = &objectives[..objectives.len().min(story::OBJECTIVES)];
                b.push(objectives.len() as u8);
                for (i, done) in objectives {
                    b.extend_from_slice(&i.to_le_bytes());
                    b.push(u8::from(*done));
                }
                b.extend_from_slice(&seen.to_le_bytes());
            }
            StoryEntry::Var { quest, index, value } => {
                b.push(1);
                b.extend_from_slice(&quest.to_le_bytes());
                b.extend_from_slice(&index.to_le_bytes());
                match value {
                    StoryValue::Num(v) => {
                        b.push(0);
                        b.extend_from_slice(&v.to_bits().to_le_bytes());
                    }
                    StoryValue::Ref(r) => {
                        b.push(1);
                        b.extend_from_slice(&r.to_le_bytes());
                    }
                }
            }
            StoryEntry::Swap { reference, shape, texture } => {
                b.push(2);
                b.extend_from_slice(&reference.to_le_bytes());
                for t in [story::swap_text(shape), story::swap_text(texture)] {
                    b.push(t.len() as u8);
                    b.extend_from_slice(t.as_bytes());
                }
            }
        }
    }
    b
}

/// A [`Progress`]'s tag and fields.
fn put_progress(b: &mut Vec<u8>, what: &Progress) {
    match what {
        Progress::Reward { quest, xp, caps } => {
            b.push(0);
            for v in [quest, xp, caps] {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        Progress::Save { stamp, kind } => {
            b.push(1);
            b.extend_from_slice(&stamp.to_le_bytes());
            b.push(*kind);
        }
        Progress::Reload { stamp } => {
            b.push(2);
            b.extend_from_slice(&stamp.to_le_bytes());
        }
        Progress::Gift { batch, item, count, quest, stage } => {
            b.push(3);
            for v in [batch, item, count, quest, stage] {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        Progress::QuestEnded { quest, failed } => {
            b.push(4);
            b.extend_from_slice(&quest.to_le_bytes());
            b.push(u8::from(*failed));
        }
        Progress::JointChoice { character, on, sat_out } => {
            b.push(5);
            b.extend_from_slice(&character.to_le_bytes());
            b.push(u8::from(*on));
            b.extend_from_slice(&sat_out.to_le_bytes());
        }
        Progress::Stage { quest, stage, line, line_end, prior, to } => {
            b.push(6);
            b.extend_from_slice(&quest.to_le_bytes());
            b.extend_from_slice(&stage.to_le_bytes());
            b.extend_from_slice(&line.to_le_bytes());
            b.push(u8::from(*line_end));
            b.push(*to);
            let n = prior.len().min(255);
            b.push(n as u8);
            for s in &prior[..n] {
                b.extend_from_slice(&s.to_le_bytes());
            }
        }
        Progress::StageResult { quest, stage, taken } => {
            b.push(7);
            b.extend_from_slice(&quest.to_le_bytes());
            b.extend_from_slice(&stage.to_le_bytes());
            b.push(u8::from(*taken));
        }
        Progress::KillCredit { to, target, target_base, npc, xp, karma, killer, infamy } => {
            b.push(8);
            b.push(*to);
            b.extend_from_slice(&target.to_le_bytes());
            b.extend_from_slice(&target_base.to_le_bytes());
            b.push(u8::from(*npc));
            b.extend_from_slice(&xp.to_le_bytes());
            b.extend_from_slice(&karma.to_le_bytes());
            b.push(u8::from(*killer));
            let n = infamy.len().min(255);
            b.push(n as u8);
            for (r, d) in &infamy[..n] {
                b.extend_from_slice(&r.to_le_bytes());
                b.extend_from_slice(&d.to_le_bytes());
            }
        }
        Progress::HitCredit { to, target, weapon, part, dealt, critical, crippled } => {
            b.push(9);
            b.push(*to);
            for v in [target, weapon, part, dealt] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            b.push(u8::from(*critical));
            b.push(u8::from(*crippled));
        }
        Progress::For { to, what } => {
            b.push(10);
            b.push(*to);
            // One level only: a For inside a For is sent as its own inside.
            match &**what {
                Progress::For { what, .. } => put_progress(b, what),
                w => put_progress(b, w),
            }
        }
    }
}

fn i32_of(r: &mut Reader) -> Option<i32> {
    r.u32().map(|v| v as i32)
}

fn u64_of(r: &mut Reader) -> Option<u64> {
    Some(u64::from(r.u32()?) | (u64::from(r.u32()?) << 32))
}

fn flag(r: &mut Reader) -> Option<bool> {
    match r.u8()? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// A [`Progress`] as [`put_progress`] wrote it.
fn get_progress(r: &mut Reader) -> Option<Progress> {
    Some(match r.u8()? {
        0 => Progress::Reward { quest: r.u32()?, xp: r.u32()?, caps: r.u32()? },
        1 => Progress::Save { stamp: r.u32()?, kind: r.u8()? },
        2 => Progress::Reload { stamp: r.u32()? },
        3 => Progress::Gift { batch: r.u32()?, item: r.u32()?, count: r.u32()?, quest: r.u32()?, stage: r.u32()? },
        4 => Progress::QuestEnded { quest: r.u32()?, failed: flag(r)? },
        5 => Progress::JointChoice { character: u64_of(r)?, on: flag(r)?, sat_out: u64_of(r)? },
        6 => {
            let (quest, stage, line, line_end, to) = (r.u32()?, i32_of(r)?, r.u32()?, flag(r)?, r.u8()?);
            let n = r.u8()?;
            let prior = (0..n).map(|_| i32_of(r)).collect::<Option<Vec<_>>>()?;
            Progress::Stage { quest, stage, line, line_end, prior, to }
        }
        7 => Progress::StageResult { quest: r.u32()?, stage: i32_of(r)?, taken: flag(r)? },
        8 => {
            let (to, target, target_base, npc, xp, karma, killer) = (r.u8()?, r.u32()?, r.u32()?, flag(r)?, r.u32()?, i32_of(r)?, flag(r)?);
            let n = r.u8()?;
            let infamy = (0..n).map(|_| Some((r.u32()?, i32_of(r)?))).collect::<Option<Vec<_>>>()?;
            Progress::KillCredit { to, target, target_base, npc, xp, karma, killer, infamy }
        }
        9 => Progress::HitCredit { to: r.u8()?, target: r.u32()?, weapon: r.u32()?, part: r.u32()?, dealt: r.u32()?, critical: flag(r)?, crippled: flag(r)? },
        10 => {
            let to = r.u8()?;
            match get_progress(r)? {
                Progress::For { .. } => return None,
                w => Progress::For { to, what: Box::new(w) },
            }
        }
        _ => return None,
    })
}

fn get_story(r: &mut Reader) -> Option<Vec<StoryEntry>> {
    let n = r.u16()? as usize;
    if n > story::PAGE_BYTES {
        return None;
    }
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        entries.push(match r.u8()? {
            0 => {
                let (quest, stage, flags, k) = (r.u32()?, r.u16()? as i16, r.u8()?, r.u8()? as usize);
                if k > story::STAGES || flags > 7 {
                    return None;
                }
                let done = (0..k).map(|_| r.u16().map(|d| d as i16)).collect::<Option<Vec<_>>>()?;
                let o = r.u8()? as usize;
                if o > story::OBJECTIVES {
                    return None;
                }
                let objectives = (0..o).map(|_| Some((r.u16()? as i16, flag(r)?))).collect::<Option<Vec<_>>>()?;
                StoryEntry::Quest { quest, stage, running: flags & 1 != 0, ended: flags & 2 != 0, done, failed: flags & 4 != 0, objectives, seen: r.u32()? }
            }
            1 => {
                let (quest, index) = (r.u32()?, r.u16()?);
                let value = match r.u8()? {
                    0 => StoryValue::Num(f64::from_bits(u64_of(r)?)),
                    1 => StoryValue::Ref(r.u32()?),
                    _ => return None,
                };
                StoryEntry::Var { quest, index, value }
            }
            2 => {
                let reference = r.u32()?;
                let n = r.u8()? as usize;
                let shape = String::from_utf8_lossy(r.take(n)?).into_owned();
                let n = r.u8()? as usize;
                let texture = String::from_utf8_lossy(r.take(n)?).into_owned();
                StoryEntry::Swap { reference, shape, texture }
            }
            _ => return None,
        });
    }
    Some(entries)
}

/// One of this module's items, read to its end; None: not one, cut short, a field out of
/// its bounds, or bytes left over.
pub fn decode(item: &[u8]) -> Option<Item> {
    let (&k, rest) = item.split_first()?;
    let mut r = Reader(rest);
    let out = match k {
        kind::PROGRESS | kind::SAVE | kind::JOINT_CHOICE | kind::STAGE_RESULT => {
            let p = get_progress(&mut r)?;
            if p.kind() != k {
                return None;
            }
            Item::Progress(p)
        }
        kind::QUEST => {
            let quest = r.u32()?;
            let flags = r.u8()?;
            let n = r.u8()? as usize;
            if n > QUEST_OBJECTIVES || flags > 3 {
                return None;
            }
            let objectives = (0..n).map(|_| Some((i32_of(&mut r)?, flag(&mut r)?))).collect::<Option<Vec<_>>>()?;
            Item::Quest(QuestView { quest, completed: flags & 1 != 0, failed: flags & 2 != 0, objectives })
        }
        kind::LEVEL => Item::Level(LevelView { level: r.u16()?, xp: r.u32()? }),
        kind::STORY => Item::Story(get_story(&mut r)?),
        _ => return None,
    };
    r.0.is_empty().then_some(out)
}

/// A guest's side: one of this module's items heard from player `id`, taken only from the
/// host (0).
pub fn from_host(id: u8, item: &[u8]) -> Option<Item> {
    if id != 0 {
        return None;
    }
    decode(item)
}

// ---- the host's checks ---------------------------------------------------------------

#[derive(Default)]
struct Checks {
    /// Each guest's character, from its first join choice while connected.
    characters: HashMap<u8, u64>,
    /// The stage offers each guest has open (quest, stage), oldest first.
    offers: HashMap<u8, VecDeque<(u32, i32)>>,
    /// Token buckets: (guest, kind) → (tokens, when last filled).
    buckets: HashMap<(u8, u8), (f32, Instant)>,
    /// The questlines a join choice may sit out: rows of `fnv_game::coop_questlines::LINES`
    /// ([`HostChecks::set_lines`]; 0 until set, so any line sat out is refused).
    lines: u32,
}

impl Checks {
    fn within(&mut self, id: u8, kind: u8, rate: Rate, now: Instant) -> bool {
        let b = self.buckets.entry((id, kind)).or_insert((rate.burst, now));
        b.0 = (b.0 + now.saturating_duration_since(b.1).as_secs_f32() * rate.per_sec).min(rate.burst);
        b.1 = now;
        if b.0 >= 1.0 {
            b.0 -= 1.0;
            true
        } else {
            false
        }
    }
}

/// The host's checks on what a guest sends of shared progress (module notes): the rules
/// for piece 1's guard ([`HostChecks::rules`]), and what they judge by, which the host's
/// game keeps current ([`HostChecks::offered`], [`HostChecks::left`]).
#[derive(Clone, Default)]
pub struct HostChecks {
    state: Arc<Mutex<Checks>>,
}

impl HostChecks {
    fn lock(&self) -> MutexGuard<'_, Checks> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// One rule per kind of the range, to register with `Session::add_rule` /
    /// `Link::add_rule` (or `Guard::add_rule`).
    pub fn rules(&self) -> Vec<(Kind, Box<dyn Rule>)> {
        (kind::FIRST..=kind::LAST).map(|k| (Kind::Item(k), Box::new(ProgressRule { checks: self.clone(), kind: k }) as Box<dyn Rule>)).collect()
    }

    /// The host sent a [`Progress::Stage`] for the players in `to` (bit = player id): each
    /// may answer it once.
    pub fn offered(&self, to: u8, quest: u32, stage: i32) {
        let mut c = self.lock();
        for id in (1..8u8).filter(|id| to & (1 << id) != 0) {
            let open = c.offers.entry(id).or_default();
            if !open.contains(&(quest, stage)) {
                if open.len() >= OFFERS_OPEN {
                    open.pop_front();
                }
                open.push_back((quest, stage));
            }
        }
    }

    /// The number of questlines this build has (`fnv_game::coop_questlines::LINES`): a
    /// join choice sitting out a line past them is refused.
    pub fn set_lines(&self, n: usize) {
        self.lock().lines = n.min(64) as u32;
    }

    /// Guest `id` left: its character and open offers go.
    pub fn left(&self, id: u8) {
        let mut c = self.lock();
        c.characters.remove(&id);
        c.offers.remove(&id);
        c.buckets.retain(|(g, _), _| *g != id);
    }

    /// Guest `id`'s character, from its join choice.
    pub fn character(&self, id: u8) -> Option<u64> {
        self.lock().characters.get(&id).copied()
    }

    fn judge(&self, id: u8, k: u8, item: &[u8], now: Instant) -> Verdict {
        let Some(Item::Progress(p)) = decode(item) else {
            // A host's kind, or one not in use, from a guest.
            if matches!(k, kind::SAVE | kind::JOINT_CHOICE | kind::STAGE_RESULT) {
                return Verdict::Drop(Reason::Bounds);
            }
            return Verdict::Drop(Reason::HostOnly);
        };
        if p.host_only() {
            return Verdict::Drop(Reason::HostOnly);
        }
        let mut c = self.lock();
        match p {
            Progress::Save { kind: save, .. } => {
                if save > Progress::SYSTEM {
                    return Verdict::Drop(Reason::Rule("save kind"));
                }
                if !c.within(id, k, SAVE_RATE, now) {
                    return Verdict::Drop(Reason::Rate);
                }
            }
            Progress::JointChoice { character, sat_out, .. } => {
                if character == 0 {
                    return Verdict::Drop(Reason::Rule("joint choice: no character"));
                }
                let known = if c.lines >= 64 { u64::MAX } else { (1u64 << c.lines) - 1 };
                if sat_out & !known != 0 {
                    return Verdict::Drop(Reason::Rule("joint choice: no such questline"));
                }
                if c.characters.get(&id).is_some_and(|had| *had != character) {
                    return Verdict::Drop(Reason::Rule("joint choice: another character"));
                }
                if !c.within(id, k, JOINT_RATE, now) {
                    return Verdict::Drop(Reason::Rate);
                }
                c.characters.insert(id, character);
            }
            Progress::StageResult { quest, stage, .. } => {
                if !c.within(id, k, STAGE_RATE, now) {
                    return Verdict::Drop(Reason::Rate);
                }
                let Some(open) = c.offers.get_mut(&id) else { return Verdict::Drop(Reason::Rule("stage result: no offer")) };
                let Some(at) = open.iter().position(|o| *o == (quest, stage)) else { return Verdict::Drop(Reason::Rule("stage result: no offer")) };
                open.remove(at);
            }
            _ => return Verdict::Drop(Reason::HostOnly),
        }
        Verdict::Accept
    }
}

struct ProgressRule {
    checks: HostChecks,
    kind: u8,
}

impl Rule for ProgressRule {
    fn check(&self, ctx: &Ctx, subject: &Subject) -> Verdict {
        let (Some(id), Subject::Item(item)) = (ctx.from_id, subject) else { return Verdict::Drop(Reason::HostOnly) };
        self.checks.judge(id, self.kind, item, ctx.now)
    }
}

/// The host's side of a guest's save: whether it brings the host's own autosave forward
/// now (at most once per [`SAVE_FORWARD_EVERY`] for the whole lobby). The host never makes
/// a manual or quick save of a guest's.
#[derive(Debug, Default)]
pub struct SaveForward {
    last: Option<Instant>,
}

impl SaveForward {
    pub fn take(&mut self, now: Instant) -> bool {
        if self.last.is_some_and(|t| now.saturating_duration_since(t) < SAVE_FORWARD_EVERY) {
            return false;
        }
        self.last = Some(now);
        true
    }
}

// ---- the host's views ---------------------------------------------------------------

/// The host's quest, level and story for the guests: what is due to send now
/// ([`HostViews::due`]), each to every guest.
#[derive(Debug, Default)]
pub struct HostViews {
    quest: Option<QuestView>,
    last_quest: Option<(Instant, QuestView)>,
    level: Option<LevelView>,
    last_level: Option<(Instant, LevelView)>,
    story: StoryPager,
    last_story: Option<Instant>,
}

impl HostViews {
    /// The host's active quest now: sent when it changes and every [`QUEST_EVERY`].
    pub fn set_quest(&mut self, quest: QuestView) {
        self.quest = Some(quest);
    }

    /// The host's level and XP now: sent when they change and every [`LEVEL_EVERY`].
    pub fn set_level(&mut self, level: LevelView) {
        self.level = Some(level);
    }

    /// The host's whole story now: a page every [`STORY_EVERY`] ([`crate::story`]).
    pub fn set_story(&mut self, entries: Vec<StoryEntry>) {
        self.story.set(entries);
    }

    /// Everything sent again from the start (a guest joined, or a stream started again).
    pub fn again(&mut self) {
        self.last_quest = None;
        self.last_level = None;
    }

    /// The items due at `now`.
    pub fn due(&mut self, now: Instant) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        if let Some(q) = &self.quest {
            if self.last_quest.as_ref().is_none_or(|(t, sent)| sent != q || now.saturating_duration_since(*t) >= QUEST_EVERY) {
                out.push(encode_quest(q));
                self.last_quest = Some((now, q.clone()));
            }
        }
        if let Some(l) = self.level {
            if self.last_level.is_none_or(|(t, sent)| sent != l || now.saturating_duration_since(t) >= LEVEL_EVERY) {
                out.push(encode_level(&l));
                self.last_level = Some((now, l));
            }
        }
        if !self.story.is_empty() && self.last_story.is_none_or(|t| now.saturating_duration_since(t) >= STORY_EVERY) {
            out.push(encode_story(&self.story.page()));
            self.last_story = Some(now);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::Guard;

    fn all() -> Vec<Progress> {
        vec![
            Progress::Reward { quest: 0x0010_4C1D, xp: 250, caps: 100 },
            Progress::Save { stamp: u32::MAX, kind: Progress::SYSTEM },
            Progress::Reload { stamp: 7 },
            Progress::Gift { batch: 3, item: 0x0011_A208, count: 24, quest: 0x0010_4C1E, stage: 50 },
            Progress::Gift { batch: 4, item: 0x0015_9E04, count: 1, quest: 0, stage: Progress::NO_STAGE },
            Progress::QuestEnded { quest: 0x0010_4C1F, failed: true },
            Progress::JointChoice { character: 0x0123_4567_89AB_CDEF, on: true, sat_out: 0b1010 },
            Progress::Stage { quest: 0x0011_76B8, stage: 100, line: 0x0013_E51E, line_end: true, prior: vec![1, 10, 20, 50], to: 0b0110 },
            Progress::StageResult { quest: 0x0011_76B8, stage: -1, taken: false },
            Progress::KillCredit { to: 1, target: 0x0010_4C1D, target_base: 0x0010_4C1E, npc: true, xp: 10, karma: -5, killer: true, infamy: vec![(0x0010_4C2A, 30)] },
            Progress::HitCredit { to: 2, target: 0x0010_4C1D, weapon: 0x000E_3778, part: u32::MAX, dealt: 12, critical: true, crippled: false },
            Progress::For { to: 2, what: Box::new(Progress::Reward { quest: 0x0011_76B8, xp: 300, caps: 50 }) },
        ]
    }

    #[test]
    fn every_progress_comes_back_as_it_was_sent() {
        for p in all() {
            let item = p.encode();
            assert!(is_progress_item(&item));
            assert_eq!(decode(&item), Some(Item::Progress(p.clone())), "{p:?}");
        }
    }

    #[test]
    fn a_for_inside_a_for_is_sent_as_its_inside_and_never_read_nested() {
        let inner = Progress::Reload { stamp: 1 };
        let nested = Progress::For { to: 1, what: Box::new(Progress::For { to: 2, what: Box::new(inner.clone()) }) };
        assert_eq!(decode(&nested.encode()), Some(Item::Progress(Progress::For { to: 1, what: Box::new(inner) })));
        let mut hand = vec![kind::PROGRESS, 10, 1, 10, 2];
        put_progress(&mut hand, &Progress::Reload { stamp: 1 });
        assert_eq!(decode(&hand), None);
    }

    #[test]
    fn the_views_and_a_story_page_come_back_as_they_were_sent() {
        let q = QuestView { quest: 0x0010_4C1D, completed: false, failed: true, objectives: vec![(10, true), (20, false)] };
        assert_eq!(decode(&encode_quest(&q)), Some(Item::Quest(q)));
        let l = LevelView { level: 12, xp: 9_876 };
        assert_eq!(decode(&encode_level(&l)), Some(Item::Level(l)));
        let page = vec![
            StoryEntry::Quest { quest: 7, stage: 30, running: true, ended: false, done: vec![10, 20], failed: false, objectives: vec![(1, true)], seen: 4 },
            StoryEntry::Var { quest: 7, index: 2, value: StoryValue::Num(1.5) },
            StoryEntry::Var { quest: 7, index: 3, value: StoryValue::Ref(0x0001_2345) },
            StoryEntry::Swap { reference: 9, shape: "Screen".into(), texture: "a\\b.dds".into() },
        ];
        assert_eq!(decode(&encode_story(&page)), Some(Item::Story(page)));
    }

    #[test]
    fn a_cut_short_overlong_or_mislabelled_item_is_not_read() {
        let item = Progress::Reward { quest: 1, xp: 2, caps: 3 }.encode();
        assert_eq!(decode(&item[..item.len() - 1]), None);
        let mut longer = item.clone();
        longer.push(0);
        assert_eq!(decode(&longer), None);
        // A host's kind under a guest's label.
        let mut relabelled = item;
        relabelled[0] = kind::SAVE;
        assert_eq!(decode(&relabelled), None);
        // A flag that is neither 0 nor 1.
        let mut save = Progress::QuestEnded { quest: 1, failed: true }.encode();
        *save.last_mut().unwrap() = 2;
        assert_eq!(decode(&save), None);
        // More objectives than a view carries.
        let mut q = vec![kind::QUEST, 0, 0, 0, 0, 0, (QUEST_OBJECTIVES + 1) as u8];
        q.extend(std::iter::repeat_n(0, (QUEST_OBJECTIVES + 1) * 5));
        assert_eq!(decode(&q), None);
        // A story page with more entries than a page holds.
        assert_eq!(decode(&[kind::STORY, 0xFF, 0xFF]), None);
        // A kind of the range not in use, and one outside it.
        assert_eq!(decode(&[0x3F]), None);
        assert!(!is_progress_item(&[0x2F]));
    }

    #[test]
    fn a_guest_takes_progress_only_from_the_host() {
        let item = Progress::Reward { quest: 1, xp: 2, caps: 3 }.encode();
        assert!(from_host(0, &item).is_some());
        assert_eq!(from_host(2, &item), None);
    }

    fn guard(checks: &HostChecks) -> Guard {
        let mut g = Guard::new(None);
        for (k, r) in checks.rules() {
            g.add_rule(k, r);
        }
        g
    }

    #[test]
    fn a_host_kind_from_a_guest_is_dropped_and_counted_against_it() {
        let checks = HostChecks::default();
        let mut g = guard(&checks);
        let now = Instant::now();
        let kinds = [
            Progress::Reward { quest: 1, xp: 2, caps: 3 }.encode(),
            Progress::Stage { quest: 1, stage: 10, line: 0, line_end: false, prior: vec![], to: 2 }.encode(),
            Progress::KillCredit { to: 1, target: 1, target_base: 1, npc: true, xp: 1, karma: 0, killer: true, infamy: vec![] }.encode(),
            encode_quest(&QuestView::default()),
            encode_level(&LevelView::default()),
            encode_story(&[]),
            vec![0x3F],
        ];
        for item in &kinds {
            assert_eq!(g.admit_item(1, item, now), Verdict::Drop(Reason::HostOnly), "{item:?}");
        }
        assert_eq!(g.bad(1), kinds.len() as u64);
    }

    #[test]
    fn a_guests_save_is_taken_once_a_minute_and_only_of_a_known_kind() {
        let checks = HostChecks::default();
        let mut g = guard(&checks);
        let t = Instant::now();
        let save = |kind| Progress::Save { stamp: 1, kind }.encode();
        assert_eq!(g.admit_item(1, &save(Progress::AUTO), t), Verdict::Accept);
        assert_eq!(g.admit_item(1, &save(Progress::AUTO), t + Duration::from_secs(10)), Verdict::Drop(Reason::Rate));
        // Another guest has its own bucket.
        assert_eq!(g.admit_item(2, &save(Progress::QUICK), t + Duration::from_secs(10)), Verdict::Accept);
        assert_eq!(g.admit_item(1, &save(Progress::MANUAL), t + Duration::from_secs(70)), Verdict::Accept);
        assert_eq!(g.admit_item(1, &save(9), t + Duration::from_secs(200)), Verdict::Drop(Reason::Rule("save kind")));
        // A rate drop is not a bad message; the unknown kind is.
        assert_eq!(g.bad(1), 1);
    }

    #[test]
    fn a_guests_save_brings_the_hosts_autosave_forward_at_most_every_two_minutes() {
        let mut f = SaveForward::default();
        let t = Instant::now();
        assert!(f.take(t));
        assert!(!f.take(t + Duration::from_secs(119)));
        assert!(f.take(t + SAVE_FORWARD_EVERY));
    }

    #[test]
    fn a_join_choice_names_one_character_per_guest_while_it_is_connected() {
        let checks = HostChecks::default();
        let mut g = guard(&checks);
        let t = Instant::now();
        let choice = |character, on| Progress::JointChoice { character, on, sat_out: 0 }.encode();
        assert_eq!(g.admit_item(1, &choice(0, true), t), Verdict::Drop(Reason::Rule("joint choice: no character")));
        assert_eq!(g.admit_item(1, &choice(42, true), t), Verdict::Accept);
        assert_eq!(checks.character(1), Some(42));
        assert_eq!(g.admit_item(1, &choice(42, false), t + Duration::from_secs(1)), Verdict::Accept);
        assert_eq!(g.admit_item(1, &choice(43, true), t + Duration::from_secs(2)), Verdict::Drop(Reason::Rule("joint choice: another character")));
        // Over the rate (two at once, then one a second).
        assert_eq!(g.admit_item(1, &choice(42, true), t + Duration::from_secs(2)), Verdict::Accept);
        assert_eq!(g.admit_item(1, &choice(42, true), t + Duration::from_secs(2)), Verdict::Accept);
        assert_eq!(g.admit_item(1, &choice(42, true), t + Duration::from_secs(2)), Verdict::Drop(Reason::Rate));
        // Once it left, it may come back as another.
        checks.left(1);
        assert_eq!(checks.character(1), None);
        assert_eq!(g.admit_item(1, &choice(43, true), t + Duration::from_secs(3)), Verdict::Accept);
    }

    /// The questlines a join choice sits out must be rows this build has
    /// (docs/design/coop-quest-sync.md §5).
    #[test]
    fn a_join_choice_sits_out_only_questlines_that_exist() {
        let checks = HostChecks::default();
        let mut g = guard(&checks);
        let t = Instant::now();
        let choice = |sat_out| Progress::JointChoice { character: 42, on: true, sat_out }.encode();
        // Not set: none may be sat out; nothing sat out is fine.
        assert_eq!(g.admit_item(1, &choice(1), t), Verdict::Drop(Reason::Rule("joint choice: no such questline")));
        assert_eq!(g.admit_item(1, &choice(0), t), Verdict::Accept);
        checks.set_lines(24);
        assert_eq!(g.admit_item(1, &choice((1 << 24) - 1), t + Duration::from_secs(1)), Verdict::Accept);
        assert_eq!(g.admit_item(1, &choice(1 << 24), t + Duration::from_secs(2)), Verdict::Drop(Reason::Rule("joint choice: no such questline")));
        assert_eq!(g.admit_item(1, &choice(u64::MAX), t + Duration::from_secs(3)), Verdict::Drop(Reason::Rule("joint choice: no such questline")));
        // A full table of 64 takes every bit.
        checks.set_lines(64);
        assert_eq!(g.admit_item(1, &choice(u64::MAX), t + Duration::from_secs(4)), Verdict::Accept);
        // A choice cut short (no lines field) is not read.
        let mut short = Progress::JointChoice { character: 42, on: true, sat_out: 0 }.encode();
        short.truncate(short.len() - 8);
        assert_eq!(decode(&short), None);
    }

    #[test]
    fn a_stage_result_must_answer_an_open_offer_once() {
        let checks = HostChecks::default();
        let mut g = guard(&checks);
        let t = Instant::now();
        let result = |quest, stage| Progress::StageResult { quest, stage, taken: true }.encode();
        assert_eq!(g.admit_item(1, &result(7, 10), t), Verdict::Drop(Reason::Rule("stage result: no offer")));
        // Offered to guests 1 and 3, not 2.
        checks.offered(0b1010, 7, 10);
        assert_eq!(g.admit_item(2, &result(7, 10), t), Verdict::Drop(Reason::Rule("stage result: no offer")));
        assert_eq!(g.admit_item(1, &result(7, 20), t), Verdict::Drop(Reason::Rule("stage result: no offer")));
        assert_eq!(g.admit_item(1, &result(7, 10), t), Verdict::Accept);
        assert_eq!(g.admit_item(1, &result(7, 10), t), Verdict::Drop(Reason::Rule("stage result: no offer")));
        assert_eq!(g.admit_item(3, &result(7, 10), t), Verdict::Accept);
        // A guest that left has no offers open.
        checks.offered(0b0010, 8, 5);
        checks.left(1);
        assert_eq!(g.admit_item(1, &result(8, 5), t), Verdict::Drop(Reason::Rule("stage result: no offer")));
    }

    #[test]
    fn open_offers_are_bounded_and_answers_beyond_the_rate_dropped() {
        let checks = HostChecks::default();
        let mut g = guard(&checks);
        let t = Instant::now();
        for s in 0..(OFFERS_OPEN as i32 + 1) {
            checks.offered(0b0010, 7, s);
        }
        let result = |stage| Progress::StageResult { quest: 7, stage, taken: true }.encode();
        // The oldest went.
        assert_eq!(g.admit_item(1, &result(0), t), Verdict::Drop(Reason::Rule("stage result: no offer")));
        for s in 1..10 {
            assert_eq!(g.admit_item(1, &result(s), t), Verdict::Accept);
        }
        assert_eq!(g.admit_item(1, &result(10), t), Verdict::Drop(Reason::Rate));
        assert_eq!(g.admit_item(1, &result(10), t + Duration::from_secs(1)), Verdict::Accept);
    }

    #[test]
    fn a_malformed_guest_kind_is_out_of_bounds() {
        let checks = HostChecks::default();
        let mut g = guard(&checks);
        assert_eq!(g.admit_item(1, &[kind::SAVE, 1, 0], Instant::now()), Verdict::Drop(Reason::Bounds));
    }

    #[test]
    fn the_host_sends_its_views_when_they_change_and_every_while() {
        let mut v = HostViews::default();
        let t = Instant::now();
        assert!(v.due(t).is_empty());
        v.set_quest(QuestView { quest: 1, ..QuestView::default() });
        v.set_level(LevelView { level: 2, xp: 3 });
        assert_eq!(v.due(t).len(), 2);
        assert!(v.due(t + Duration::from_millis(100)).is_empty());
        v.set_level(LevelView { level: 2, xp: 4 });
        assert_eq!(v.due(t + Duration::from_millis(200)), vec![encode_level(&LevelView { level: 2, xp: 4 })]);
        assert_eq!(v.due(t + QUEST_EVERY).len(), 1);
        v.set_story(vec![StoryEntry::Var { quest: 1, index: 0, value: StoryValue::Ref(5) }]);
        let story = v.due(t + QUEST_EVERY + STORY_EVERY);
        assert!(story.iter().any(|i| i[0] == kind::STORY));
        assert!(!v.due(t + QUEST_EVERY + STORY_EVERY).iter().any(|i| i[0] == kind::STORY));
        v.again();
        assert_eq!(v.due(t + QUEST_EVERY + STORY_EVERY).len(), 2);
    }

    #[test]
    fn a_full_story_page_fits_one_reliable_item() {
        let mut pager = StoryPager::default();
        pager.set((0..400).map(|i| StoryEntry::Var { quest: i, index: 0, value: StoryValue::Num(0.0) }).collect());
        assert!(encode_story(&pager.page()).len() <= crate::rel::ITEM_MAX);
    }
}
