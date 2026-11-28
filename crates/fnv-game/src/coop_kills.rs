//! Kill credit in co-op: who earns what for a kill when
//! several players fight (coop-shared-progress.md §4.2) [design].
//!
//! - The killer earns 100 % of the kill's XP, as in the original. Each other player "in
//!   combat with that target" earns [`COOP_KILL_XP_SHARE`] of it, rounded down, at least
//!   1; in combat: the target had that player as a combat target, or that player hit it
//!   within the last [`SHARE_WINDOW`] game seconds. Nobody else, whatever the distance.
//! - The kill's XP is computed once ([`Game::kill_xp`]); its 40 % damage threshold
//!   (`iXPDeathRewardHealthThreshold`) counts every player's damage together, with the
//!   hired companions' as before.
//! - Karma, the kill count and the kill challenges are the killer's only. Hit and damage
//!   challenges count each player's own hits.
//! - A guest's share lives in the guest's own character: the host never gives it to its
//!   own player. It queues a [`KillCredit`] (and a [`HitCredit`] per guest hit) that the
//!   engine drains ([`Game::take_coop_kill_credits`], [`Game::take_coop_hit_credits`]),
//!   sends, and the guest's game takes ([`Game::take_coop_kill_credit`],
//!   [`Game::take_coop_hit_credit`]).
//!
//! The host's quests still see the guest's kill as the player's (GetKiller, OnDeath;
//! [`Game::as_player`]), and murder, infamy and witnesses stay the host world's.

use crate::game::Game;
use fnv_sim::FormId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The share of a kill's XP each other player in combat with the target earns [design:
/// coop-shared-progress.md §4.2; a lobby setting later].
pub const COOP_KILL_XP_SHARE: f64 = 0.5;

/// How recent (game seconds) a player's hit must be for the player to be in combat with
/// the target [design: coop-shared-progress.md §4.2].
pub const SHARE_WINDOW: f64 = 10.0;

/// What a guest earned for a kill on the host, for the guest's own character.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KillCredit {
    /// The guest's actor on the host ([`Game::is_coop_actor`]).
    pub player: FormId,
    /// The killed actor (a reference of the host's world).
    pub target: FormId,
    /// The target's base form, for the guest's challenges and counts.
    pub target_base: Option<FormId>,
    /// Whether the target was an NPC (a person killed) rather than a creature.
    pub npc: bool,
    /// The kill XP before the guest's own perks (`crate::experience::award` adds them).
    pub xp: f64,
    /// The karma change, when the guest killed.
    pub karma: f64,
    /// Whether the guest killed it (the kill count and kill challenges are the killer's).
    pub killer: bool,
    /// The infamy the host's world gave for this kill (a seen murder), by reputation: the
    /// guest's own act, kept for its own save while it has a claimed quest open
    /// (coop-shared-progress.md §3.6, `crate::coop_joint`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub infamy: Vec<(FormId, f64)>,
}

/// A guest's hit on the host, for the guest's hit and damage challenges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HitCredit {
    pub player: FormId,
    pub target: FormId,
    pub weapon: Option<FormId>,
    /// The body part hit (`crate::challenges::body_part`).
    pub part: Option<u32>,
    pub dealt: f64,
    pub critical: bool,
    pub crippled: bool,
}

/// The players' hits per target and the guests' credits waiting for the engine. Not saved.
#[derive(Debug, Clone, Default)]
pub struct CoopKills {
    /// Target → player (the host's `PLAYER_REF` or a guest's actor) → (health damage
    /// dealt, game time of the last hit).
    pub hits: BTreeMap<FormId, BTreeMap<FormId, (f64, f64)>>,
    /// Kill credits for the guests ([`Game::take_coop_kill_credits`]).
    pub credits: Vec<KillCredit>,
    /// The guests' hits ([`Game::take_coop_hit_credits`]).
    pub hits_out: Vec<HitCredit>,
}

/// The other players' share of `xp`: [`COOP_KILL_XP_SHARE`], rounded down, at least 1
/// (none of no XP).
pub fn share_of(xp: f64) -> f64 {
    if xp <= 0.0 {
        return 0.0;
    }
    (xp * COOP_KILL_XP_SHARE).floor().max(1.0)
}

impl Game<'_> {
    /// `attacker` hit `target` for `dealt` health: noted when the attacker is a player.
    pub(crate) fn note_player_hit(&mut self, target: FormId, attacker: FormId, dealt: f64) {
        if attacker != FormId::PLAYER_REF && !self.is_coop_actor(attacker) {
            return;
        }
        let now = self.time;
        let e = self.coop_kills.hits.entry(target).or_default().entry(attacker).or_default();
        e.0 += dealt;
        e.1 = now;
    }

    /// Whether `player` was in combat with `target`: its combat target, or a hit within
    /// [`SHARE_WINDOW`].
    pub fn in_combat_with(&self, player: FormId, target: FormId) -> bool {
        let targets = self.awareness.get(target).is_some_and(|a| a.state.in_combat() && a.targets.contains_key(&player));
        let hit = self.coop_kills.hits.get(&target).and_then(|m| m.get(&player)).is_some_and(|&(_, at)| self.time - at <= SHARE_WINDOW);
        targets || hit
    }

    /// A guest's actor killed `target` (`Game::die`, while it still lives): its karma
    /// change and its kill go to the guest's queue, not to the host's player.
    pub(crate) fn coop_guest_killed(&mut self, target: FormId, guest: FormId, karma: f64, infamy: Vec<(FormId, f64)>) {
        let base = crate::functions::base_of(self, target);
        let npc = base.and_then(|b| self.data.actors.get(&b)).is_some_and(|d| &d.sig == b"NPC_");
        self.coop_kills.credits.push(KillCredit { player: guest, target, target_base: base, npc, xp: 0.0, karma, killer: true, infamy });
    }

    /// `target` died by `killer`'s hand (a hit or an effect): the kill's XP for every
    /// player owed some. The host's part is awarded now and given back (after its perks);
    /// the guests' parts are queued. Nothing when the killer is no player nor a
    /// companion earning the player XP ([`Game::kill_earns_xp`]).
    pub(crate) fn kill_credit(&mut self, target: FormId, killer: Option<FormId>) -> f64 {
        let host_xp = self.kill_credit_with(target, killer);
        self.coop_kills.hits.remove(&target);
        host_xp
    }

    fn kill_credit_with(&mut self, target: FormId, killer: Option<FormId>) -> f64 {
        let Some(killer) = killer else { return 0.0 };
        let guest_killer = self.is_coop_actor(killer);
        if !guest_killer && !self.kill_earns_xp(killer) {
            return 0.0;
        }
        let xp = self.kill_xp(target);
        let share = share_of(xp);
        // The host: all of it for its own (or its companion's) kill, a share in combat.
        let host = if !guest_killer {
            xp
        } else if self.in_combat_with(FormId::PLAYER_REF, target) {
            share
        } else {
            0.0
        };
        let guests: Vec<FormId> = self.coop_actors.keys().copied().collect();
        let base = crate::functions::base_of(self, target);
        let npc = base.and_then(|b| self.data.actors.get(&b)).is_some_and(|d| &d.sig == b"NPC_");
        for g in guests {
            if g == killer {
                // Its kill's credit was queued by `Game::die`, with the karma.
                match self.coop_kills.credits.iter_mut().rev().find(|c| c.player == g && c.target == target && c.killer) {
                    Some(c) => c.xp = xp,
                    None => self.coop_kills.credits.push(KillCredit { player: g, target, target_base: base, npc, xp, karma: 0.0, killer: true, infamy: Vec::new() }),
                }
            } else if share > 0.0 && self.in_combat_with(g, target) {
                self.coop_kills.credits.push(KillCredit { player: g, target, target_base: base, npc, xp: share, karma: 0.0, killer: false, infamy: Vec::new() });
            }
        }
        crate::experience::award(self, crate::experience::Source::Kill, host)
    }

    /// The guests' kill credits since the last call, for the engine to send.
    pub fn take_coop_kill_credits(&mut self) -> Vec<KillCredit> {
        std::mem::take(&mut self.coop_kills.credits)
    }

    /// The guests' hits since the last call, for the engine to send.
    pub fn take_coop_hit_credits(&mut self) -> Vec<HitCredit> {
        std::mem::take(&mut self.coop_kills.hits_out)
    }

    /// A guest takes its credit for a kill on the host: the XP (with its own perks), the
    /// karma, and for its own kill the kill count and kill challenges. Gives back the XP
    /// gained.
    pub fn take_coop_kill_credit(&mut self, c: &KillCredit) -> f64 {
        let gained = crate::experience::award(self, crate::experience::Source::Kill, c.xp);
        if c.karma != 0.0 {
            let name = c.target_base.map(|b| self.data.name(b)).unwrap_or_default();
            self.change_karma(c.karma, format!("killing {name} (co-op)"));
        }
        if c.killer {
            use crate::misc_stats::{CREATURES_KILLED, PEOPLE_KILLED, TOTAL_THINGS_KILLED};
            self.count_stat(if c.npc { PEOPLE_KILLED } else { CREATURES_KILLED }, 1.0);
            self.count_stat(TOTAL_THINGS_KILLED, 1.0);
            // The target is a reference of the same world; one only the host has (spawned
            // there) still counts by the challenges that ask for no form [hypothesis].
            self.challenge_kill(c.target);
        }
        if c.killer {
            self.coop_note_reputation(&c.infamy);
        }
        gained
    }

    /// A guest takes its hit on the host for its hit and damage challenges.
    pub fn take_coop_hit_credit(&mut self, h: &HitCredit) {
        self.challenge_hit(h.target, h.weapon, h.part, h.dealt, h.critical, h.crippled);
    }
}
