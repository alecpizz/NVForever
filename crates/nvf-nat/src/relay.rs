//! Choosing another player to relay through, when a guest cannot reach the host directly
//! (#3795 section 8). Sans-IO, like the rest of the crate: this decides which guest relays for
//! whom, when to try one, and when to give up. The forwarding itself is `nvf-coop`'s
//! (`Msg::Relayed`), and the packets stay end-to-end Noise from the guest to the host, so the
//! relay sees only ciphertext.
//!
//! Names follow section 8: C is the guest with no direct path, H the host, A a guest that has
//! one and might forward C's packets.
//!
//! - [`Planner`] runs on C. It starts when C's direct checks fail ([`crate::ice::Action::Failed`]).
//!   It asks helpers through the matchmaking service (C has no other channel: `nvf-signal`'s
//!   `relay_ask`), runs checks (the same [`crate::ice::Agent`]) against each one that accepts,
//!   and picks the best one reached. When that relay goes quiet, it tries the next one. It gives
//!   up with a reason the game can show (section 10).
//! - [`Policy`] runs on A. It answers a relay request from the player's setting and A's own
//!   state.
//!
//! [design throughout: #3795 section 8, with this module's own timings]

use std::time::{Duration, Instant};

/// A player's number in the lobby (the service's `peer`).
pub type PlayerId = u32;

/// A guest with a direct path to the host, which could relay: the service's `helpers` list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Helper {
    pub id: PlayerId,
    /// The helper's own round trip to the host, if known: better helpers first.
    pub host_rtt: Option<Duration>,
}

/// A helper's answer to a relay request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Accept,
    Refuse(Refusal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The helper's "Relay for other players" setting is off.
    Off,
    /// It already relays as many players as it will.
    Busy,
    /// Its own path to the host is relayed or gone: it cannot help.
    NoDirectPath,
}

impl Refusal {
    /// The `reason` string of the service's `relay_answer`.
    pub fn as_wire(self) -> &'static str {
        match self {
            Refusal::Off => "off",
            Refusal::Busy => "busy",
            Refusal::NoDirectPath => "no_direct_path",
        }
    }

    /// Reads a `reason`; an unknown one counts as busy.
    pub fn from_wire(s: &str) -> Self {
        match s {
            "off" => Refusal::Off,
            "no_direct_path" => Refusal::NoDirectPath,
            _ => Refusal::Busy,
        }
    }
}

/// A helper's side: whether to relay for someone who asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    /// The "Relay for other players" setting (#3795 section 10). On by default, as the spec
    /// recommends (section 15.4, still the owner's call).
    pub allow: bool,
    /// At most this many players relayed at once. Each costs about 15 KB/s both ways
    /// (section 8, Goodsprings); 2 keeps a home upload comfortable [design].
    pub max_relayed: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Policy { allow: true, max_relayed: 2 }
    }
}

impl Policy {
    /// The answer to a relay request, given how many players this game relays now and
    /// whether its own path to the host is direct.
    pub fn answer(&self, relaying_now: usize, my_path_direct: bool) -> Answer {
        if !self.allow {
            Answer::Refuse(Refusal::Off)
        } else if !my_path_direct {
            Answer::Refuse(Refusal::NoDirectPath)
        } else if relaying_now >= self.max_relayed {
            Answer::Refuse(Refusal::Busy)
        } else {
            Answer::Accept
        }
    }
}

/// Timings for [`Planner`].
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// At most this many helpers asked or being checked at once (best host round trip first).
    pub parallel: usize,
    /// No answer to a relay request by then counts as a refusal.
    pub answer_timeout: Duration,
    /// After choosing a helper, the first round trip to the host must come back through it by
    /// then, or the relay counts as lost.
    pub confirm_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config { parallel: 3, answer_timeout: Duration::from_secs(2), confirm_timeout: Duration::from_secs(3) }
    }
}

/// What the planner asks the caller to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Ask this helper to relay, through the service (`relay_ask`, with C's candidates); then
    /// call [`Planner::on_answer`].
    Ask { helper: PlayerId },
    /// It accepted: run connectivity checks against the candidates it sent, then call
    /// [`Planner::on_reach`]. (The helper runs its side of the checks on accepting.)
    TryReach { helper: PlayerId },
    /// Relay through this helper from now on. Call [`Planner::on_confirmed`] when the first
    /// round trip to the host comes back through it.
    Chosen { helper: PlayerId },
    /// The relay through this helper stopped working. Stop using it; another try follows if
    /// any helper is left.
    Lost { helper: PlayerId },
    /// No relay can be had. The game says why (section 10) and stays as it is.
    GiveUp(GiveUp),
}

/// Why no relay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GiveUp {
    /// No other guest has a direct path (a 2-player lobby, or nobody reaches the host).
    NoHelpers,
    /// Every helper said no (settings off, busy) or did not answer.
    AllRefused,
    /// Helpers accepted, but none could be reached either.
    NoneReachable,
    /// The relays that were chosen all stopped working.
    RelaysFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Fresh,
    Asked(Instant),
    Trying,
    Reached(Duration),
    Refused,
    Unreachable,
    /// Chosen, then lost: not tried again in this search.
    Failed,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    helper: Helper,
    stage: Stage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Searching,
    Confirming { helper: PlayerId, since: Instant },
    Relayed { helper: PlayerId },
    GaveUp,
}

/// C's relay choice. See the module notes.
pub struct Planner {
    cfg: Config,
    entries: Vec<Entry>,
    phase: Phase,
    out: Vec<Action>,
}

impl Planner {
    pub fn new(cfg: Config) -> Self {
        Planner { cfg, entries: Vec::new(), phase: Phase::Idle, out: Vec::new() }
    }

    /// The guests that could relay (the service's `helpers` list, with host round trips when
    /// known). A helper that drops out of the list while relaying is lost; a new one is asked
    /// in a search still going.
    pub fn set_helpers(&mut self, helpers: &[Helper], now: Instant) {
        let old = std::mem::take(&mut self.entries);
        self.entries = helpers
            .iter()
            .map(|&h| match old.iter().find(|e| e.helper.id == h.id) {
                Some(e) => Entry { helper: h, stage: e.stage },
                None => Entry { helper: h, stage: Stage::Fresh },
            })
            .collect();
        if let Some(current) = self.current() {
            if self.entry(current).is_none() {
                self.lose(current, now);
            }
        }
    }

    /// The direct path failed: start looking for a relay. Again after [`Action::GiveUp`],
    /// for a new try (a new helper appeared, say): every helper is asked afresh.
    pub fn start(&mut self, _now: Instant) {
        if matches!(self.phase, Phase::Idle | Phase::GaveUp) {
            for e in &mut self.entries {
                e.stage = Stage::Fresh;
            }
            self.phase = Phase::Searching;
        }
    }

    /// `helper` answered the relay request (`relay_answer`).
    pub fn on_answer(&mut self, helper: PlayerId, answer: Answer) {
        let Some(e) = self.entry_mut(helper) else { return };
        if !matches!(e.stage, Stage::Asked(_)) {
            return;
        }
        match answer {
            Answer::Accept => {
                e.stage = Stage::Trying;
                self.out.push(Action::TryReach { helper });
            }
            Answer::Refuse(_) => e.stage = Stage::Refused,
        }
    }

    /// The checks to `helper` finished: reached (with the round trip) or not.
    pub fn on_reach(&mut self, helper: PlayerId, rtt: Option<Duration>) {
        if let Some(e) = self.entry_mut(helper) {
            if e.stage == Stage::Trying {
                e.stage = match rtt {
                    Some(rtt) => Stage::Reached(rtt),
                    None => Stage::Unreachable,
                };
            }
        }
    }

    /// The first round trip to the host came back through the chosen helper.
    pub fn on_confirmed(&mut self, helper: PlayerId) {
        if let Phase::Confirming { helper: h, .. } = self.phase {
            if h == helper {
                self.phase = Phase::Relayed { helper };
            }
        }
    }

    /// The relayed path went quiet (`net.rs`'s lost timer) or the helper stopped relaying.
    pub fn on_relay_lost(&mut self, helper: PlayerId, now: Instant) {
        if self.current() == Some(helper) {
            self.lose(helper, now);
        }
    }

    /// The helper being relayed through, once chosen.
    pub fn current(&self) -> Option<PlayerId> {
        match self.phase {
            Phase::Confirming { helper, .. } | Phase::Relayed { helper } => Some(helper),
            _ => None,
        }
    }

    fn lose(&mut self, helper: PlayerId, _now: Instant) {
        self.out.push(Action::Lost { helper });
        if let Some(e) = self.entry_mut(helper) {
            e.stage = Stage::Failed;
        }
        self.phase = Phase::Searching;
    }

    fn entry(&self, id: PlayerId) -> Option<&Entry> {
        self.entries.iter().find(|e| e.helper.id == id)
    }

    fn entry_mut(&mut self, id: PlayerId) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.helper.id == id)
    }

    /// What to do now. Call every tick and after each `on_*` call.
    pub fn poll(&mut self, now: Instant) -> Vec<Action> {
        if let Phase::Confirming { helper, since } = self.phase {
            if now.saturating_duration_since(since) >= self.cfg.confirm_timeout {
                self.lose(helper, now);
            }
        }
        if self.phase == Phase::Searching {
            self.search(now);
        }
        std::mem::take(&mut self.out)
    }

    fn search(&mut self, now: Instant) {
        if self.entries.is_empty() {
            self.give_up(GiveUp::NoHelpers);
            return;
        }
        let timeout = self.cfg.answer_timeout;
        for e in &mut self.entries {
            if let Stage::Asked(since) = e.stage {
                if now.saturating_duration_since(since) >= timeout {
                    e.stage = Stage::Refused;
                }
            }
        }
        // The first reached is good enough: waiting for a better one keeps the player waiting
        // [design]. Among several reached at once, the lowest total round trip.
        let best = self
            .entries
            .iter()
            .filter_map(|e| match e.stage {
                Stage::Reached(rtt) => Some((e.helper.id, rtt + e.helper.host_rtt.unwrap_or(Duration::ZERO))),
                _ => None,
            })
            .min_by_key(|&(_, total)| total);
        if let Some((helper, _)) = best {
            self.phase = Phase::Confirming { helper, since: now };
            self.out.push(Action::Chosen { helper });
            return;
        }
        let busy = self.entries.iter().filter(|e| matches!(e.stage, Stage::Asked(_) | Stage::Trying)).count();
        let mut fresh: Vec<usize> = (0..self.entries.len()).filter(|&i| self.entries[i].stage == Stage::Fresh).collect();
        fresh.sort_by_key(|&i| self.entries[i].helper.host_rtt.unwrap_or(Duration::MAX));
        let asked = fresh.len().min(self.cfg.parallel.saturating_sub(busy));
        for &i in &fresh[..asked] {
            self.entries[i].stage = Stage::Asked(now);
            self.out.push(Action::Ask { helper: self.entries[i].helper.id });
        }
        if busy + asked == 0 {
            let any = |st: Stage| self.entries.iter().any(|e| e.stage == st);
            let reason = if any(Stage::Failed) {
                GiveUp::RelaysFailed
            } else if any(Stage::Unreachable) {
                GiveUp::NoneReachable
            } else {
                GiveUp::AllRefused
            };
            self.give_up(reason);
        }
    }

    fn give_up(&mut self, reason: GiveUp) {
        self.phase = Phase::GaveUp;
        self.out.push(Action::GiveUp(reason));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn helper(id: PlayerId, host_rtt: u64) -> Helper {
        Helper { id, host_rtt: Some(ms(host_rtt)) }
    }

    #[test]
    fn policy_and_wire_reasons() {
        let p = Policy::default();
        assert_eq!(p.answer(0, true), Answer::Accept);
        assert_eq!(p.answer(2, true), Answer::Refuse(Refusal::Busy));
        assert_eq!(p.answer(0, false), Answer::Refuse(Refusal::NoDirectPath));
        assert_eq!(Policy { allow: false, ..p }.answer(0, true), Answer::Refuse(Refusal::Off));
        for r in [Refusal::Off, Refusal::Busy, Refusal::NoDirectPath] {
            assert_eq!(Refusal::from_wire(r.as_wire()), r);
        }
        assert_eq!(Refusal::from_wire("something new"), Refusal::Busy);
    }

    #[test]
    fn no_helpers_gives_up_at_once() {
        let t0 = Instant::now();
        let mut p = Planner::new(Config::default());
        assert!(p.poll(t0).is_empty(), "idle until the direct path fails");
        p.start(t0);
        assert_eq!(p.poll(t0), vec![Action::GiveUp(GiveUp::NoHelpers)]);
        assert!(p.poll(t0).is_empty(), "said once");
    }

    #[test]
    fn asks_best_host_links_first_checks_accepters_and_chooses() {
        let t0 = Instant::now();
        let mut p = Planner::new(Config { parallel: 2, ..Config::default() });
        p.set_helpers(&[helper(1, 90), helper(2, 20), helper(3, 40), Helper { id: 4, host_rtt: None }], t0);
        p.start(t0);
        assert_eq!(p.poll(t0), vec![Action::Ask { helper: 2 }, Action::Ask { helper: 3 }]);
        assert!(p.poll(t0).is_empty(), "two in flight already");
        p.on_answer(2, Answer::Refuse(Refusal::Busy));
        assert_eq!(p.poll(t0), vec![Action::Ask { helper: 1 }]);
        p.on_answer(3, Answer::Accept);
        assert_eq!(p.poll(t0), vec![Action::TryReach { helper: 3 }]);
        p.on_answer(3, Answer::Accept);
        assert!(p.poll(t0).is_empty(), "a repeated answer changes nothing");
        p.on_reach(3, Some(ms(100)));
        assert_eq!(p.poll(t0 + ms(50)), vec![Action::Chosen { helper: 3 }]);
        assert_eq!(p.current(), Some(3));
        p.on_confirmed(3);
        assert!(p.poll(t0 + ms(10_000)).is_empty(), "a confirmed relay has no timer here");
    }

    #[test]
    fn timeouts_unreachable_and_confirm_failure() {
        let t0 = Instant::now();
        let mut p = Planner::new(Config::default());
        p.set_helpers(&[helper(1, 10), helper(2, 10), helper(3, 10)], t0);
        p.start(t0);
        assert_eq!(p.poll(t0).len(), 3);
        p.on_answer(1, Answer::Accept);
        p.on_answer(3, Answer::Accept);
        assert_eq!(p.poll(t0), vec![Action::TryReach { helper: 1 }, Action::TryReach { helper: 3 }]);
        p.on_reach(1, None);
        p.on_reach(3, Some(ms(30)));
        assert_eq!(p.poll(t0 + ms(2000)), vec![Action::Chosen { helper: 3 }]);
        // The first round trip never comes back: lost. 2 timed out, 1 unreachable: nobody left.
        assert_eq!(p.poll(t0 + ms(5000)), vec![Action::Lost { helper: 3 }, Action::GiveUp(GiveUp::RelaysFailed)]);
    }

    #[test]
    fn reasons() {
        let t0 = Instant::now();
        let mut p = Planner::new(Config::default());
        p.set_helpers(&[helper(1, 10), helper(2, 10)], t0);
        p.start(t0);
        p.poll(t0);
        p.on_answer(1, Answer::Refuse(Refusal::Off));
        p.on_answer(2, Answer::Refuse(Refusal::Busy));
        assert_eq!(p.poll(t0), vec![Action::GiveUp(GiveUp::AllRefused)]);
        p.start(t0);
        p.poll(t0);
        p.on_answer(1, Answer::Accept);
        p.on_answer(2, Answer::Refuse(Refusal::Busy));
        p.poll(t0);
        p.on_reach(1, None);
        assert_eq!(p.poll(t0), vec![Action::GiveUp(GiveUp::NoneReachable)]);
    }

    #[test]
    fn lost_relay_moves_on_and_a_leaving_helper_counts_as_lost() {
        let t0 = Instant::now();
        let mut p = Planner::new(Config::default());
        p.set_helpers(&[helper(1, 10), helper(2, 50)], t0);
        p.start(t0);
        p.poll(t0);
        for id in [1, 2] {
            p.on_answer(id, Answer::Accept);
            p.on_reach(id, Some(ms(10)));
        }
        p.poll(t0);
        assert_eq!(p.current(), Some(1), "lowest total round trip");
        p.on_confirmed(1);
        p.on_relay_lost(1, t0 + ms(60_000));
        assert_eq!(p.poll(t0 + ms(60_000)), vec![Action::Lost { helper: 1 }, Action::Chosen { helper: 2 }]);
        p.on_confirmed(2);
        // Helper 2 leaves the lobby: lost, and nobody is left.
        p.set_helpers(&[helper(1, 10)], t0 + ms(70_000));
        assert_eq!(p.poll(t0 + ms(70_000)), vec![Action::Lost { helper: 2 }, Action::GiveUp(GiveUp::RelaysFailed)]);
        // A new helper appears: start again, everyone asked afresh.
        p.set_helpers(&[helper(1, 10), helper(5, 5)], t0 + ms(80_000));
        p.start(t0 + ms(80_000));
        assert_eq!(p.poll(t0 + ms(80_000)), vec![Action::Ask { helper: 5 }, Action::Ask { helper: 1 }]);
    }
}
