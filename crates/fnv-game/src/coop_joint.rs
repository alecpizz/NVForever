//! Joint progression in co-op (docs/design/coop-shared-progress.md): a guest that chose to share
//! quest progress **claims** the stages of the host's side and DLC quests it is there for
//! into its own save, stage by stage, and its own world replays them on its next load.
//!
//! - **Who is who.** Every character has a stable [`CharacterId`]
//!   (`PlayerState::character_id`), made the first time it joins another player's game
//!   ([`Game::ensure_character_id`]; a solo save never carries one) and sent with its join
//!   choice, so a slot number never names a character across sessions [design:
//!   coop-shared-progress.md §3.2].
//! - **Host: presence.** [`Game::set_stage`] calls [`Game::coop_joint_stage`]: for a
//!   claimable quest (any but a player's own, [`is_per_player`]; the base-game main story
//!   too, docs/design/coop-quest-sync.md §2) every connected guest with joint progression
//!   on that does not sit out the quest's line ([`JointGuest::sat_out`],
//!   `crate::coop_questlines`) ([`Game::coop_joint_guests`]) is written into
//!   [`SessionPresence`] (saved in the host's game), and one [`StageOffer`] is queued for
//!   the engine to send ([`Game::take_joint_offers`]) [design: coop-shared-progress.md §3.1, §3.2].
//! - **Guest: the check.** [`check`] decides from the guest's own game as it was at join
//!   (`SoloBase`) and its [`ClaimLedger`]: the quest's prerequisites at its first claimed
//!   stage, and continuity (every earlier stage of the host's path claimed or done in its
//!   own game) [design: coop-shared-progress.md §3.1.4-5, §3.4]. A stage taken goes into the ledger, which lives in
//!   the character and so lands in its own save.
//! - **Replay.** When the character's own world loads (solo or hosting),
//!   [`Game::replay_claims`] applies the ledger before the first frame: each claimed stage
//!   in order, through the dialogue line that set it on the host when one did (the line's
//!   result script carries choices no stage records: Rex's brain), else
//!   `SetStage`. A replayed script's SayTo says nothing, but the line it would say has its
//!   result scripts run at once ([`Game::coop_replay_say_to`]), so what the host's world
//!   did through spoken lines happens in the guest's own: a companion handed over is hired
//!   (the King's line SayTos Rex, whose line SayTos the player the followers' hire topic)
//!   [observed: FalloutNV.esm, the INFO result scripts of VMS24 and VNPCFollowers'
//!   FollowersHired, `fnvtool script decode`; design: no list of lines is kept].
//!   Rewards already given live are muted ([`Game::coop_replaying`]) [design:
//!   coop-shared-progress.md §3.5].

use crate::game::Game;
use crate::quest::QuestState;
use fnv_sim::FormId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A character's stable identity across sessions and hosts [design: coop-shared-progress.md §3.2]. 0 is "none
/// yet" (an older save): [`new_character_id`] gives one on load.
pub type CharacterId = u64;

/// A fresh random [`CharacterId`], never 0 [design: 64 random bits from the standard
/// library's per-process hasher keys and the clock; no new dependency].
pub fn new_character_id() -> CharacterId {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    h.write_u32(std::process::id());
    h.finish().max(1)
}

/// FalloutNV.esm's base-game main story. It is claimable like any other quest; a guest
/// that wants it host only sits out its lines (docs/design/coop-quest-sync.md §2)
/// [design]. Kept for the logs and tools. Read from the ESM's QUST records and the Fallout Wiki's main quests
/// [observed: editor IDs in FalloutNV.esm; documented: wiki main-quest list, from memory,
/// to recheck against an export]. Three use the side-quest prefix (VMS23,
/// VMS29, VMS35), so the list is explicit. Support quests of the main story (timers, end
/// states, Yes Man's positioning) are in it too: host world only.
pub const MAIN_STORY: &[&str] = &[
    // Opening.
    "VCG00", "VCG01", "VCG01Test", "VCG02", "VCG03", "VCG04", "VMQ01", "VMQTops", "BennyToArenaTimer",
    // House.
    "VMQHouse", "VMQHouse1", "VMQHouse2", "VMQHouse3", "VMQHouse4", "VMQHouse5", "VMQHouse6", "VMQHouse7", "VMQHouse8",
    "VMQHouseFail", "VMQHouseLockdown", "VHDHouseBattle",
    // Yes Man.
    "VMQYesMan01", "VMQYesMan01a", "VMQYesMan02", "VMQYesMan03", "VMQYesManFailsafe", "VMQYesManSupport",
    "VHDIndependentBattle",
    // NCR.
    "VMS23", "VMS29", "VMQ05", "VMQ03a", "VMQ03aTimer", "vHDBattleController", "VMQNCRFail", "VHDKimballSpeech",
    "VHDAssassinationDialog", "VHDOliverDeathTimer", "VHDOliverRetreatTimer",
    // Legion.
    "VMQ02", "VMS35", "VMQ03b", "VMQ03bTimer", "VMQ04", "VHDLegionBattle", "VMQCaesarFail",
    // The end.
    "VMQ06", "VMQEndStates", "VMQEndStatesFreeside", "VMQEndStatesTheStrip",
];

/// Whether `q` is base-game main story: a FalloutNV.esm quest (load-order index 0, the
/// first master) whose editor ID is in [`MAIN_STORY`]. Every DLC quest, main line
/// included, is claimable [design].
pub fn is_main_story(g: &Game, q: FormId) -> bool {
    q.0 >> 24 == 0 && g.data.quests.get(&q).and_then(|d| d.edid.as_deref()).is_some_and(|e| MAIN_STORY.iter().any(|m| m.eq_ignore_ascii_case(e)))
}

/// Quests whose stages are each player's own, never shared or replayed: Old World Blues'
/// implant quest, whose stages the Sink's Auto-Doc sets when a player gets Implant GRX
/// [design: the Auto-Doc is per player; each player gets it by
/// using the Auto-Doc itself] [observed: OldWorldBlues.esm QUST NVDLC03GenericPerkQUEST,
/// stages 10 and 11 set its rank and its script keeps the implant stocked].
pub const PER_PLAYER: &[&str] = &["NVDLC03GenericPerkQUEST"];

/// How many SayTo lines deep a replay follows ([`Game::coop_replay_say_to`]) [design: Rex's
/// hand-over is two deep; the bound stops a line that SayTos its own topic].
pub const REPLAY_SAY_DEPTH: u8 = 4;

/// Whether `q`'s stages are each player's own ([`PER_PLAYER`], in any plugin).
pub fn is_per_player(g: &Game, q: FormId) -> bool {
    g.data.quests.get(&q).and_then(|d| d.edid.as_deref()).is_some_and(|e| PER_PLAYER.iter().any(|m| m.eq_ignore_ascii_case(e)))
}

/// Which characters were connected (with joint progression on) when the host set each
/// stage, kept in the host's save across sessions [design: coop-shared-progress.md §3.2].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPresence {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stages: BTreeMap<FormId, BTreeMap<i32, BTreeSet<CharacterId>>>,
}

impl SessionPresence {
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }
    pub fn was_there(&self, q: FormId, stage: i32, who: CharacterId) -> bool {
        self.stages.get(&q).and_then(|s| s.get(&stage)).is_some_and(|c| c.contains(&who))
    }
}

/// The dialogue line whose result script set a stage: the INFO, and whether it was its
/// end script (else its begin script).
pub type Line = (FormId, bool);

/// One stage a guest claimed, with the host's line that set it, if a line did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimedStage {
    pub stage: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<Line>,
}

/// A character's claimed stages, by quest in claim order (coop-shared-progress.md §3.5): written as each is
/// taken, replayed in its own world and then cleared. `done` keeps what was replayed,
/// for the Pip-Boy and the logs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClaimLedger {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stages: BTreeMap<FormId, Vec<ClaimedStage>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub done: BTreeMap<FormId, BTreeSet<i32>>,
    /// Reputation this character's own acts changed in another player's world while it
    /// had a claimed quest open (a seen murder's infamy: coop-shared-progress.md §3.6), by reputation and
    /// axis (0 the one `crate::functions::crime_infamy` moves); replayed with the stages.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub reputation: BTreeMap<FormId, [f64; 2]>,
    /// Map markers this character found in another player's world while sharing quest
    /// progress : found in its own save at its next load, without the notice.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub markers: BTreeSet<FormId>,
}

impl ClaimLedger {
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty() && self.done.is_empty() && self.reputation.is_empty() && self.markers.is_empty()
    }
    pub fn claimed(&self, q: FormId, stage: i32) -> bool {
        self.stages.get(&q).is_some_and(|s| s.iter().any(|c| c.stage == stage)) || self.done.get(&q).is_some_and(|s| s.contains(&stage))
    }
    fn has_quest(&self, q: FormId) -> bool {
        self.stages.contains_key(&q) || self.done.contains_key(&q)
    }
}

/// A stage the host set, for each connected guest with joint progression on
/// (coop-shared-progress.md §3.1, `nvf_coop::progress::Progress::Stage`). `prior` is the host's earlier stages of the quest, for continuity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageOffer {
    pub quest: FormId,
    pub stage: i32,
    pub prior: Vec<i32>,
    pub line: Option<Line>,
    /// The guests' actors it is for.
    pub to: Vec<FormId>,
}

/// A connected guest as the host knows it: its character and its join choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JointGuest {
    pub character: CharacterId,
    pub on: bool,
    /// The questlines it sits out (`crate::coop_questlines::Mask`): no offer, no presence
    /// for their quests.
    pub sat_out: crate::coop_questlines::Mask,
}

/// Why a guest turned a stage down (coop-shared-progress.md §3.4), for its log and the host's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnedDown {
    /// Its own game completed or failed the quest.
    QuestEnded,
    /// Its own game did that stage already: nothing to write, continuity holds.
    AlreadyDone,
    /// The quest's prerequisites fail in its own game (first claimed stage).
    Prerequisite,
    /// It has neither claimed nor done this earlier stage of the host's path.
    Missed(i32),
}

impl std::fmt::Display for TurnedDown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TurnedDown::QuestEnded => write!(f, "its own game ended the quest"),
            TurnedDown::AlreadyDone => write!(f, "its own game did that stage"),
            TurnedDown::Prerequisite => write!(f, "prerequisite"),
            TurnedDown::Missed(s) => write!(f, "missed stage {s}"),
        }
    }
}

/// The guest's check of one offered stage against its own game's quests (`own`, its
/// `SoloBase`) and its ledger (coop-shared-progress.md §3.1.4-5, §3.4) [design]. `prereq_met` evaluates the
/// quest's prerequisites in its own game; it is asked only at the quest's first claimed
/// stage, and a quest its own game already started passes.
pub fn check(own: &BTreeMap<FormId, QuestState>, ledger: &ClaimLedger, offer: &StageOffer, prereq_met: impl FnOnce() -> bool) -> Result<(), TurnedDown> {
    let mine = own.get(&offer.quest);
    if mine.is_some_and(|s| s.completed || s.failed) {
        return Err(TurnedDown::QuestEnded);
    }
    if mine.is_some_and(|s| s.stages_done.contains(&offer.stage)) || ledger.claimed(offer.quest, offer.stage) {
        return Err(TurnedDown::AlreadyDone);
    }
    let started_here = mine.is_some_and(|s| s.running || !s.stages_done.is_empty());
    if !started_here && !ledger.has_quest(offer.quest) && !prereq_met() {
        return Err(TurnedDown::Prerequisite);
    }
    for &p in &offer.prior {
        if !(mine.is_some_and(|s| s.stages_done.contains(&p)) || ledger.claimed(offer.quest, p)) {
            return Err(TurnedDown::Missed(p));
        }
    }
    Ok(())
}

impl Game<'_> {
    /// Whether this character has a claimed quest still running here: its acts then count
    /// in its own save too (coop-shared-progress.md §3.6). A simplification [design]: any open claim, not
    /// only the quest the act belongs to (no data ties a kill to a quest).
    pub fn coop_claim_open(&self) -> bool {
        self.player.claims.stages.keys().any(|q| self.quests.get(q).is_none_or(|s| !s.completed && !s.failed))
    }

    /// A guest's own act changed reputations in the host's world (`crate::coop_kills`):
    /// kept in the ledger for its own save while a claim is open.
    pub fn coop_note_reputation(&mut self, changes: &[(FormId, f64)]) {
        if changes.is_empty() || !self.coop_claim_open() {
            return;
        }
        for (r, d) in changes {
            self.player.claims.reputation.entry(*r).or_default()[0] += d;
        }
    }

    /// Debug console `coop.presence <quest>`: who this host's world saw at each stage of
    /// `q` (character IDs).
    pub fn coop_presence_lines(&self, q: FormId) -> Vec<String> {
        let name = self.data.name(q);
        match self.coop_presence.stages.get(&q) {
            None => vec![format!("{name}: no stage set with a guest sharing quest progress")],
            Some(stages) => std::iter::once(format!("{name}: {} stage(s) with guests sharing", stages.len()))
                .chain(stages.iter().map(|(s, who)| format!("  stage {s}: {}", who.iter().map(|c| format!("{c:016x}")).collect::<Vec<_>>().join(", "))))
                .collect(),
        }
    }

    /// Debug console `coop.ledger`: this character's ID and its claimed stages, waiting
    /// and replayed .
    pub fn coop_ledger_lines(&self) -> Vec<String> {
        let l = &self.player.claims;
        let mut out = vec![format!("Character {:016x}: {} quest(s) waiting, {} replayed", self.player.character_id, l.stages.len(), l.done.len())];
        for (q, s) in &l.stages {
            let stages: Vec<String> = s.iter().map(|c| if c.line.is_some() { format!("{} (+line)", c.stage) } else { c.stage.to_string() }).collect();
            out.push(format!("  waiting {}: {}", self.data.name(*q), stages.join(", ")));
        }
        for (q, s) in &l.done {
            out.push(format!("  replayed {}: {}", self.data.name(*q), s.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")));
        }
        out
    }

    /// The host's hook in [`Game::set_stage`], after the stage is marked done (module
    /// notes). Nothing while no guest has joint progression on and none that had it is
    /// away ([`Game::coop_joint_away`]).
    pub fn coop_joint_stage(&mut self, q: FormId, stage: i32) {
        if self.coop_replaying || !(self.coop_joint_away || self.coop_joint_guests.values().any(|j| j.on)) || is_per_player(self, q) {
            return;
        }
        // The quest's lines, read only when a guest sits one out.
        let lines = if self.coop_joint_guests.values().any(|j| j.on && j.sat_out != 0) { self.coop_questlines().lines_of(q) } else { 0 };
        let to: Vec<FormId> = self.coop_joint_guests.iter().filter(|(_, j)| j.on && j.sat_out & lines == 0).map(|(a, _)| *a).collect();
        // Every guest here sits it out and none sharing is away: nothing to offer.
        if to.is_empty() && !self.coop_joint_away {
            return;
        }
        let who: Vec<CharacterId> = to.iter().filter_map(|a| self.coop_joint_guests.get(a)).map(|j| j.character).filter(|c| *c != 0).collect();
        // Nobody here (a sharing player away): no presence line for the stage.
        if !who.is_empty() {
            self.coop_presence.stages.entry(q).or_default().entry(stage).or_default().extend(who);
        }
        let prior: Vec<i32> = self.quests.get(&q).map(|s| s.stages_done.iter().copied().filter(|s| *s != stage).collect()).unwrap_or_default();
        self.coop_joint_out.push(StageOffer { quest: q, stage, prior, line: self.coop_line, to });
    }

    /// The host's stage offers since the last call, for the engine to send.
    pub fn take_joint_offers(&mut self) -> Vec<StageOffer> {
        std::mem::take(&mut self.coop_joint_out)
    }

    /// A guest takes or turns down an offered stage (module notes). `own` is its own
    /// game's quests as at join. A stage taken is written into the character's ledger.
    pub fn take_joint_offer(&mut self, own: &BTreeMap<FormId, QuestState>, offer: &StageOffer) -> Result<(), TurnedDown> {
        // The prerequisites are the data's (`crate::coop_prereqs`).
        let prereq = || self.coop_prereqs().met(offer.quest, own);
        check(own, &self.player.claims, offer, prereq)?;
        self.player.claims.stages.entry(offer.quest).or_default().push(ClaimedStage { stage: offer.stage, line: offer.line });
        Ok(())
    }

    /// Give the character a [`CharacterId`] if it has none (its first join as a guest).
    pub fn ensure_character_id(&mut self) -> CharacterId {
        if self.player.character_id == 0 {
            self.player.character_id = new_character_id();
        }
        self.player.character_id
    }

    /// Replay the ledger's claimed stages in this, the character's own world (module
    /// notes; coop-shared-progress.md §3.5). Returns one line per quest for the HUD ("From co-op: <quest>").
    /// Rewards were given live, so `RewardXP`, caps and items to the player and
    /// achievements are muted while it runs ([`Game::coop_replaying`]).
    pub fn replay_claims(&mut self) -> Vec<String> {
        if self.player.claims.stages.is_empty() && self.player.claims.reputation.is_empty() {
            return Vec::new();
        }
        let claims = std::mem::take(&mut self.player.claims.stages);
        let reputation = std::mem::take(&mut self.player.claims.reputation);
        let mut lines = Vec::new();
        self.coop_replaying = true;
        for (q, stages) in claims {
            let ended = self.quests.get(&q).is_some_and(|s| s.completed || s.failed);
            let mut applied = 0;
            for c in &stages {
                self.player.claims.done.entry(q).or_default().insert(c.stage);
                if ended || self.quests.get(&q).is_some_and(|s| s.stages_done.contains(&c.stage)) {
                    continue;
                }
                if let Some((info, end)) = c.line {
                    self.replay_line(info, end);
                }
                if !self.quests.get(&q).is_some_and(|s| s.stages_done.contains(&c.stage)) {
                    self.set_stage(q, c.stage);
                }
                applied += 1;
            }
            if applied > 0 {
                let name = self.data.quests.get(&q).and_then(|d| d.name.clone().or_else(|| d.edid.clone())).unwrap_or_else(|| format!("{q}"));
                tracing::info!("co-op: replayed {applied} claimed stage(s) of {name}");
                lines.push(fnv_util::words::trf("From co-op: {name}", &[("name", &name)]));
            }
        }
        // The character's own acts' reputation in claimed quests (coop-shared-progress.md §3.6).
        for (r, axes) in reputation {
            for (axis, d) in axes.into_iter().enumerate().filter(|(_, d)| *d != 0.0) {
                crate::functions::reputation_change(self, r, axis, d);
                tracing::info!("co-op: replayed reputation {} axis {axis} {d:+}", self.data.name(r));
            }
        }
        self.coop_replaying = false;
        lines
    }

    /// Run a dialogue line's result script outside a conversation, its speaker the
    /// line's speaker if it names one [hypothesis: the scripts replayed name their refs
    /// explicitly (Rex's surgery lines do), so `self` matters little].
    fn replay_line(&mut self, info: FormId, end: bool) {
        self.replay_line_by(info, end, None);
    }

    /// [`Game::replay_line`] with the speaker known (`Some`: a SayTo's speaker).
    fn replay_line_by(&mut self, info: FormId, end: bool, speaker: Option<FormId>) {
        let Some(def) = self.data.infos.get(&info) else {
            tracing::warn!("co-op: replay: line {info} is not in this load order");
            return;
        };
        let Some(u) = (if end { def.end } else { def.begin }) else { return };
        // The speaker's placed reference: the speaker is a base form, and companions and
        // quest NPCs are unique, placed once [hypothesis for the lines replayed].
        let speaker = speaker.or_else(|| def.speaker.and_then(|b| self.world.placed.refs.iter().find(|(_, p)| p.base == b).map(|(r, _)| *r))).unwrap_or(FormId::PLAYER_REF);
        self.run_result(u, crate::game::RunCtx { self_ref: Some(speaker), action_ref: None, quest: def.quest });
    }

    /// A SayTo while claimed stages replay (`functions::misc::say_to`): nothing is said,
    /// but the line `speaker` would say to `target` (chosen as a SayTo chooses it, its
    /// conditions read in this world now) has its begin and end scripts run at once, so a
    /// SayTo that line makes is followed in turn, at most [`REPLAY_SAY_DEPTH`] deep (module
    /// notes) [design]. A topic with no line for the speaker now does nothing.
    pub fn coop_replay_say_to(&mut self, speaker: FormId, target: FormId, topic: FormId) {
        if self.coop_replay_depth >= REPLAY_SAY_DEPTH {
            tracing::warn!("co-op: replay: {} SayTo {}: more than {REPLAY_SAY_DEPTH} lines deep, not followed", self.data.name(speaker), self.data.name(topic));
            return;
        }
        let Some(info) = self.choose_said_line(speaker, Some(target), topic, true) else {
            tracing::debug!("co-op: replay: {} has no line of {} now", self.data.name(speaker), self.data.name(topic));
            return;
        };
        tracing::info!("co-op: replay: {} SayTo {}: line {info}'s scripts run", self.data.name(speaker), self.data.name(topic));
        self.coop_replay_depth += 1;
        self.replay_line_by(info, false, Some(speaker));
        self.replay_line_by(info, true, Some(speaker));
        self.coop_replay_depth -= 1;
    }
}
