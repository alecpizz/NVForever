//! Melee and unarmed swings (docs/design/combat.md §5.4, docs/spec-notes/combat-rules.md
//! §10): when a swing starts, which clip it plays, when it strikes and when the next one
//! may cut in. Headless; the engine feeds the trigger, the movement keys and the clip
//! timing, and deals the blows on [`MeleeEvent::Strike`].
//!
//! - **Combos.** Each attack clip's `a:` text key names the attack group to play next and
//!   its time is when the clip may be cut for it [documented: GECK wiki
//!   NiTextKeyExtraData, "a:R" plays AttackRight, "a:7" Attack7, a blank one a random
//!   attack]. The `AttackRight` clips of the fists and melee weapons carry `a:L` and the
//!   `AttackLeft` ones `a:R` [observed: `h2h`, `1hm`, `2hm` clips of both rigs in the base
//!   meshes archive, 2026-10-11], so a run of swings goes right, left, right…, the fists
//!   alternating hands. A press during a swing starts the next one at the `a:` key (or
//!   when the swing ends, if it is let go later); a press after the swing has ended
//!   starts again from the weapon's own attack group [design].
//! - **Variants.** The melee clips come in `_a` and `_b` forms (`h2hattackleft_a.kf`,
//!   `h2hattackleft_b.kf`) [observed]; each swing picks one at random [hypothesis].
//! - **Power attacks.** Holding Use [`CombatTuning::power_hold`] starts a power attack,
//!   whose clip follows the movement keys held: `AttackPower` standing,
//!   `AttackForwardPower`, `AttackBackPower`, `AttackLeftPower`, `AttackRightPower`
//!   [observed: those clips for `h2h`, `1hm`, `2hm`; owner's account 2026-10-11 of the
//!   original]. Power clips have no `a:` key [observed], so they do not chain. Their
//!   damage multiplier is the `fDamagePowerAttack…Bonus` setting of the direction
//!   [hypothesis: the setting names].

use serde::Serialize;

use crate::tuning::{CombatTuning, Rules};

/// The movement input (−1..1 on an axis) past which a power attack goes that way
/// [design].
pub const MOVE_THRESHOLD: f32 = 0.3;

/// The direction of a power attack, from the movement keys held when it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PowerDir {
    Stand,
    Forward,
    Back,
    Left,
    Right,
}

impl PowerDir {
    /// From the movement input (`forward` +1 W, −1 S; `right` +1 D, −1 A): the larger
    /// axis when it is past [`MOVE_THRESHOLD`], else standing; forward and back win a tie
    /// [design].
    pub fn of(forward: f32, right: f32) -> PowerDir {
        if forward.abs().max(right.abs()) < MOVE_THRESHOLD {
            PowerDir::Stand
        } else if forward.abs() >= right.abs() {
            if forward > 0.0 { PowerDir::Forward } else { PowerDir::Back }
        } else if right > 0.0 {
            PowerDir::Right
        } else {
            PowerDir::Left
        }
    }

    /// The clip file stem (`1hm` + `attackforwardpower` + `.kf`).
    pub fn stem(self) -> &'static str {
        match self {
            PowerDir::Stand => "attackpower",
            PowerDir::Forward => "attackforwardpower",
            PowerDir::Back => "attackbackpower",
            PowerDir::Left => "attackleftpower",
            PowerDir::Right => "attackrightpower",
        }
    }

    /// Every direction, in the order of a combat style's shares (normal, forward, back,
    /// left, right) [documented: GECK wiki Combat Style].
    pub const ALL: [PowerDir; 5] = [PowerDir::Stand, PowerDir::Forward, PowerDir::Back, PowerDir::Left, PowerDir::Right];

    /// The attack group (the clip's sequence name, `AttackLeftPower`) [observed: the
    /// `_male` power clips' sequence names, Fallout - Meshes.bsa; documented: the groups
    /// an EAMT unarmed attack effect names].
    pub fn group(self) -> &'static str {
        match self {
            PowerDir::Stand => "AttackPower",
            PowerDir::Forward => "AttackForwardPower",
            PowerDir::Back => "AttackBackPower",
            PowerDir::Left => "AttackLeftPower",
            PowerDir::Right => "AttackRightPower",
        }
    }

    /// The direction of attack group `group` ([`PowerDir::group`]), any case.
    pub fn of_group(group: &str) -> Option<PowerDir> {
        PowerDir::ALL.into_iter().find(|d| d.group().eq_ignore_ascii_case(group))
    }

    /// The damage multiplier of a power attack this way.
    pub fn damage(self, r: &Rules) -> f32 {
        match self {
            PowerDir::Stand => r.power_attack_stand,
            PowerDir::Forward => r.power_attack_forward,
            PowerDir::Back => r.power_attack_back,
            PowerDir::Left | PowerDir::Right => r.power_attack_side,
        }
    }
}

/// What an attack clip's `a:` key names to play next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum NextAttack {
    Right,
    Left,
    /// `Attack3` … `Attack9`.
    Numbered(u8),
    /// A blank key: any attack.
    Any,
}

impl NextAttack {
    /// The key's text after `a:` (`L`, `R`, `7`, or nothing), any case.
    pub fn parse(value: &str) -> Option<NextAttack> {
        let v = value.trim();
        match v.to_ascii_lowercase().as_str() {
            "" => Some(NextAttack::Any),
            "l" => Some(NextAttack::Left),
            "r" => Some(NextAttack::Right),
            n => n.parse().ok().map(NextAttack::Numbered),
        }
    }

    /// The attack group's clip stem without a variant (`attackleft`, `attack7`); `roll`
    /// (0..1) picks right or left for [`NextAttack::Any`] [design: the two every melee
    /// type has].
    pub fn stem(self, roll: f32) -> String {
        match self {
            NextAttack::Right => "attackright".to_string(),
            NextAttack::Left => "attackleft".to_string(),
            NextAttack::Numbered(n) => format!("attack{n}"),
            NextAttack::Any => if roll < 0.5 { "attackright" } else { "attackleft" }.to_string(),
        }
    }
}

/// The clip stem of a normal swing of attack group `group` (`attackright`): its `_a` or
/// `_b` form by `roll` (0..1). The engine falls back to the plain and `_a` forms where a
/// group has no `_b` (`h2hattack5_a.kf` only, `h2hattack6.kf` plain).
pub fn swing_stem(group: &str, roll: f32) -> String {
    format!("{group}{}", if roll < 0.5 { "_a" } else { "_b" })
}

/// One swing's timing, seconds from its start.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SwingTiming {
    pub len: f32,
    /// The strike (the clip's `Hit` key).
    pub hit: f32,
    /// The clip's `a:` key: from when the next swing may cut in, and what it plays.
    pub chain: Option<(f32, NextAttack)>,
}

impl SwingTiming {
    /// From the clip's length and keys, played `speed` times as fast (the weapon's attack
    /// × animation multipliers, as `attack::Timing`). Without a clip: `fallback` seconds,
    /// striking at 40 % of it [design].
    pub fn of(len: Option<f32>, hit: Option<f32>, chain: Option<(f32, NextAttack)>, speed: f32, fallback: f32) -> SwingTiming {
        let speed = speed.max(0.01);
        match len.filter(|l| *l > 0.0) {
            Some(l) => {
                let len = l / speed;
                let hit = hit.map_or(0.4 * len, |h| (h / speed).min(len));
                SwingTiming { len, hit, chain: chain.map(|(t, n)| ((t / speed).clamp(hit, len), n)) }
            }
            None => SwingTiming { len: fallback, hit: 0.4 * fallback, chain: None },
        }
    }
}

/// The swing in progress.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Swing {
    /// The clip stem it plays (`attackleft_b`, `attackforwardpower`).
    pub stem: String,
    /// A power attack, and which way.
    pub power: Option<PowerDir>,
    pub t: f32,
    pub timing: SwingTiming,
    /// The timing is the clip's (see [`Melee::set_timing`]).
    pub timed: bool,
    pub struck: bool,
}

/// One tick's input.
#[derive(Debug, Clone, Copy)]
pub struct MeleeInput<'a> {
    /// Use held.
    pub trigger: bool,
    pub dt: f32,
    /// The movement keys (as [`PowerDir::of`]).
    pub forward: f32,
    pub right: f32,
    /// The weapon's own attack group (`attackright`), the first swing of a run.
    pub first: &'a str,
    /// Random rolls in 0..1 for the variant and a blank `a:` key.
    pub rolls: [f32; 2],
    /// The weapon's power attack override (WEAP DNAM, `attackcustom3power`;
    /// `fnv_combat::data::WeaponStats::power_attack_override`): the standing power
    /// attack plays it instead of `attackpower` [hypothesis: which power attacks it
    /// replaces is not documented; the directional ones keep their own clips].
    pub power_override: Option<&'a str>,
    /// Block is held: new swings do not start while blocking unless `instant_power` is
    /// also set [observed: player report in #3753; design: pending an original-game
    /// capture of mid-block edge cases].
    pub blocking: bool,
    /// NVForever's Power Attack If Blocking tweak (docs/design/tweaks.md): a press of Use
    /// starts a power attack at once, with no hold.
    pub instant_power: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum MeleeEvent {
    /// A swing started; `chained`: it goes on from the last one's `a:` key.
    Started { stem: String, power: Option<PowerDir>, chained: bool },
    /// The swing reached its `Hit` key: deal the blow.
    Strike { power: Option<PowerDir> },
    /// The swing ended with no other following it.
    Ended,
}

/// The swing state of one wielder.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Melee {
    pub swing: Option<Swing>,
    /// Seconds Use has been held since it was pressed (until a power attack takes it, or
    /// it is let go).
    held: Option<f32>,
    /// Use was tapped: a normal swing waits for its moment.
    tap: bool,
    /// Use was held long enough: a power attack waits for its moment.
    power: bool,
    /// The waiting press came during a swing, so the run goes on.
    chained: bool,
    /// What the ended swing's `a:` key named, for a press made during it that starts
    /// after it ended.
    carry: Option<NextAttack>,
    was_trigger: bool,
}

impl Melee {
    /// The weapon is not ready (drawing, holstered, changed): everything stops.
    pub fn reset(&mut self) {
        *self = Melee { was_trigger: self.was_trigger, ..Melee::default() };
    }

    /// The clip's timing for the swing playing `stem`: taken once, by the swing that
    /// plays it, keeping the time already run.
    pub fn set_timing(&mut self, stem: &str, timing: SwingTiming) {
        if let Some(s) = self.swing.as_mut().filter(|s| !s.timed && s.stem == stem) {
            s.timing = timing;
            s.timed = true;
        }
    }

    /// Advance by one tick. `provisional`: the timing a new swing starts with until
    /// [`Melee::set_timing`] gives its clip's.
    pub fn tick(&mut self, input: MeleeInput, tuning: &CombatTuning, provisional: SwingTiming) -> Vec<MeleeEvent> {
        let mut events = Vec::new();
        let pressed = input.trigger && !self.was_trigger;
        let released = !input.trigger && self.was_trigger;
        self.was_trigger = input.trigger;
        if input.blocking && !input.instant_power {
            // Holding Block with the Power Attack From a Block tweak off suppresses
            // starting or queueing a swing until Block ends (#3753).
            self.held = None;
            self.tap = false;
            self.power = false;
        } else {
            if pressed {
                self.held = Some(0.0);
                self.chained = self.swing.is_some();
                if input.instant_power {
                    self.held = None;
                    self.power = true;
                }
            } else if let Some(h) = self.held.as_mut().filter(|_| input.trigger) {
                *h += input.dt;
            }
            if self.held.is_some_and(|h| h >= tuning.power_hold) {
                self.held = None;
                self.power = true;
            }
            if released && self.held.take().is_some() {
                self.tap = true;
            }
        }

        if let Some(s) = self.swing.as_mut() {
            s.t += input.dt;
            if !s.struck && s.t >= s.timing.hit {
                s.struck = true;
                events.push(MeleeEvent::Strike { power: s.power });
            }
        }
        // Whether a new swing may start now, and what the current one names next.
        let (free, ended, next) = match &self.swing {
            None => (true, false, self.carry),
            Some(s) => {
                let ended = s.t >= s.timing.len;
                let chain = s.timing.chain.filter(|_| s.power.is_none());
                (ended || (s.struck && chain.is_some_and(|(at, _)| s.t >= at)), ended, chain.map(|(_, n)| n))
            }
        };
        if free && (self.power || self.tap) {
            let power = self.power.then(|| PowerDir::of(input.forward, input.right));
            let next = next.filter(|_| self.chained && power.is_none());
            let stem = match (power, input.power_override) {
                (Some(PowerDir::Stand), Some(o)) => o.to_string(),
                (Some(d), _) => d.stem().to_string(),
                (None, _) => swing_stem(&next.map_or_else(|| input.first.to_string(), |n| n.stem(input.rolls[1])), input.rolls[0]),
            };
            events.push(MeleeEvent::Started { stem: stem.clone(), power, chained: next.is_some() });
            self.swing = Some(Swing { stem, power, t: 0.0, timing: provisional, timed: false, struck: false });
            (self.tap, self.power, self.chained, self.carry) = (false, false, false, None);
        } else if ended {
            // A press made during the swing that has not started yet keeps the run going.
            let waiting = self.chained && (self.held.is_some() || self.tap || self.power);
            self.carry = if waiting { next } else { None };
            self.chained = waiting;
            self.swing = None;
            events.push(MeleeEvent::Ended);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    /// `h2hattackright_a.kf` (third person): Hit 0.30, a:L 0.63, end 1.43 [observed].
    fn right() -> SwingTiming {
        SwingTiming::of(Some(1.43), Some(0.30), Some((0.63, NextAttack::Left)), 1.0, 1.0)
    }

    /// `h2hattackleft_a.kf`: Hit 0.33, a:R 0.63, end 0.90 [observed].
    fn left() -> SwingTiming {
        SwingTiming::of(Some(0.90), Some(0.33), Some((0.63, NextAttack::Right)), 1.0, 1.0)
    }

    /// `h2hattackforwardpower.kf`: Hit 0.73, end 1.60, no `a:` key [observed].
    fn power() -> SwingTiming {
        SwingTiming::of(Some(1.60), Some(0.73), None, 1.0, 1.0)
    }

    /// Runs `secs` with Use held as `trigger` says (by time since the start of the run)
    /// and the clip timing the engine would report for each stem.
    fn run(m: &mut Melee, secs: f32, forward: f32, trigger: impl Fn(f32) -> bool) -> Vec<MeleeEvent> {
        run_with(m, secs, forward, None, trigger)
    }

    /// [`run`] with a weapon's power attack override.
    fn run_with(m: &mut Melee, secs: f32, forward: f32, power_override: Option<&str>, trigger: impl Fn(f32) -> bool) -> Vec<MeleeEvent> {
        let t = CombatTuning::default();
        let mut out = Vec::new();
        let mut now = 0.0;
        let mut i = 0u32;
        while now < secs {
            let input = MeleeInput { trigger: trigger(now), dt: DT, forward, right: 0.0, first: "attackright", rolls: [0.25, (i % 2) as f32 * 0.6], power_override, blocking: false, instant_power: false };
            out.extend(m.tick(input, &t, SwingTiming::of(None, None, None, 1.0, 1.0)));
            if let Some(s) = m.swing.clone() {
                let timing = if s.stem.starts_with("attackright_") { right() } else if s.stem.starts_with("attackleft_") { left() } else { power() };
                m.set_timing(&s.stem, timing);
            }
            now += DT;
            i += 1;
        }
        out
    }

    fn started(events: &[MeleeEvent]) -> Vec<(String, bool)> {
        events.iter().filter_map(|e| if let MeleeEvent::Started { stem, chained, .. } = e { Some((stem.clone(), *chained)) } else { None }).collect()
    }

    #[test]
    fn a_keys_parse() {
        assert_eq!(NextAttack::parse("L"), Some(NextAttack::Left));
        assert_eq!(NextAttack::parse("r"), Some(NextAttack::Right));
        assert_eq!(NextAttack::parse("7"), Some(NextAttack::Numbered(7)));
        assert_eq!(NextAttack::parse(""), Some(NextAttack::Any));
        assert_eq!(NextAttack::parse("x"), None);
        assert_eq!(NextAttack::Numbered(4).stem(0.0), "attack4");
        assert_eq!(swing_stem("attackleft", 0.7), "attackleft_b");
    }

    #[test]
    fn power_direction_follows_the_keys() {
        assert_eq!(PowerDir::of(0.0, 0.0), PowerDir::Stand);
        assert_eq!(PowerDir::of(1.0, 0.0), PowerDir::Forward);
        assert_eq!(PowerDir::of(-1.0, 0.5), PowerDir::Back);
        assert_eq!(PowerDir::of(0.2, -1.0), PowerDir::Left);
        assert_eq!(PowerDir::of(0.0, 1.0), PowerDir::Right);
        assert_eq!(PowerDir::of(1.0, 1.0), PowerDir::Forward);
        assert_eq!(PowerDir::Back.stem(), "attackbackpower");
    }

    #[test]
    fn taps_alternate_right_and_left() {
        // Four quick taps, 0.1 s each, every 0.5 s: the ones made during a swing wait
        // for its `a:` key and go on with the group it names.
        let mut m = Melee::default();
        let ev = run(&mut m, 4.0, 0.0, |t| t < 2.0 && (t % 0.5) < 0.1);
        let s = started(&ev);
        let groups: Vec<&str> = s.iter().map(|(stem, _)| stem.split('_').next().unwrap()).collect();
        assert_eq!(groups, ["attackright", "attackleft", "attackright", "attackleft"], "{s:?}");
        assert!(!s[0].1 && s[1..].iter().all(|(_, c)| *c), "{s:?}");
        assert_eq!(ev.iter().filter(|e| matches!(e, MeleeEvent::Strike { .. })).count(), 4);
        assert_eq!(ev.last(), Some(&MeleeEvent::Ended));
    }

    #[test]
    fn the_blocking_tweak_makes_a_tap_a_power_attack() {
        // A tap far shorter than the power hold: a plain swing without the tweak (not
        // blocking), the standing power attack at once with the tweak while blocking.
        let t = CombatTuning::default();
        for (blocking, instant, want) in [(false, false, None), (true, true, Some(PowerDir::Stand))] {
            let mut m = Melee::default();
            let input = |trigger| MeleeInput { trigger, dt: DT, forward: 0.0, right: 0.0, first: "attackright", rolls: [0.0, 0.0], power_override: None, blocking, instant_power: instant };
            let mut ev = m.tick(input(true), &t, SwingTiming::of(None, None, None, 1.0, 1.0));
            ev.extend(m.tick(input(false), &t, SwingTiming::of(None, None, None, 1.0, 1.0)));
            let power = ev.iter().find_map(|e| match e {
                MeleeEvent::Started { power, .. } => Some(*power),
                _ => None,
            });
            assert_eq!(power, Some(want), "blocking {blocking}, instant {instant}: {ev:?}");
        }
    }

    #[test]
    fn blocking_without_tweak_suppresses_taps_and_holds_until_block_ends() {
        let t = CombatTuning::default();
        let prov = SwingTiming::of(None, None, None, 1.0, 1.0);
        let mk = |trigger, blocking| MeleeInput { trigger, dt: DT, forward: 0.0, right: 0.0, first: "attackright", rolls: [0.0, 0.0], power_override: None, blocking, instant_power: false };
        let mut m = Melee::default();

        // Tap while blocking: neither press nor release starts a swing.
        assert!(m.tick(mk(false, true), &t, prov).is_empty());
        assert!(m.tick(mk(true, true), &t, prov).is_empty());
        assert!(m.tick(mk(false, true), &t, prov).is_empty());
        assert!(m.swing.is_none());

        // Hold past power_hold while blocking, then drop Block while still holding Use:
        // no swing starts during Block or from the suppressed hold after Block ends.
        let mut held = 0.0f32;
        while held < t.power_hold + 0.3 {
            assert!(m.tick(mk(true, true), &t, prov).is_empty());
            held += DT;
        }
        assert!(m.tick(mk(true, false), &t, prov).is_empty());
        assert!(m.tick(mk(false, false), &t, prov).is_empty());
        assert!(m.swing.is_none());

        // Fresh tap after Block ends starts a normal swing.
        assert!(m.tick(mk(true, false), &t, prov).is_empty());
        let ev = m.tick(mk(false, false), &t, prov);
        assert!(ev.iter().any(|e| matches!(e, MeleeEvent::Started { power: None, .. })), "{ev:?}");
    }

    #[test]
    fn raising_block_mid_charge_cancels_pending_attack() {
        let t = CombatTuning::default();
        let prov = SwingTiming::of(None, None, None, 1.0, 1.0);
        let mk = |trigger, blocking| MeleeInput { trigger, dt: DT, forward: 0.0, right: 0.0, first: "attackright", rolls: [0.0, 0.0], power_override: None, blocking, instant_power: false };
        let mut m = Melee::default();
        assert!(m.tick(mk(true, false), &t, prov).is_empty());
        assert!(m.tick(mk(true, true), &t, prov).is_empty());
        assert!(m.tick(mk(false, true), &t, prov).is_empty());
        assert!(m.tick(mk(false, false), &t, prov).is_empty());
        assert!(m.swing.is_none());
    }

    #[test]
    fn a_press_waits_for_the_a_key() {
        // Tapped at 0.2 s into the right swing: the left one starts at its a:L (0.63 s
        // after the right one started), not at its end (1.43 s).
        let mut m = Melee::default();
        let mut starts = Vec::new();
        let t = CombatTuning::default();
        let mut now = 0.0f32;
        while now < 1.5 {
            let trigger = now < 0.05 || (0.25..0.3).contains(&now);
            let input = MeleeInput { trigger, dt: DT, forward: 0.0, right: 0.0, first: "attackright", rolls: [0.0, 0.0], power_override: None, blocking: false, instant_power: false };
            for e in m.tick(input, &t, SwingTiming::of(None, None, None, 1.0, 1.0)) {
                if let MeleeEvent::Started { stem, .. } = e {
                    starts.push((now, stem));
                }
            }
            if let Some(s) = m.swing.clone() {
                m.set_timing(&s.stem, if s.stem.starts_with("attackright") { right() } else { left() });
            }
            now += DT;
        }
        assert_eq!(starts.len(), 2, "{starts:?}");
        let gap = starts[1].0 - starts[0].0;
        assert!((gap - 0.63).abs() < 2.0 * DT, "{starts:?}");
        assert_eq!(starts[1].1, "attackleft_a");
    }

    #[test]
    fn a_late_press_starts_a_new_run() {
        // Tapped again after the right swing has ended: right again.
        let mut m = Melee::default();
        let s = started(&run(&mut m, 4.0, 0.0, |t| t < 0.05 || (2.0..2.05).contains(&t)));
        assert_eq!(s.len(), 2);
        assert!(s.iter().all(|(stem, chained)| stem.starts_with("attackright") && !chained), "{s:?}");
    }

    #[test]
    fn power_attack_groups_go_both_ways() {
        for d in PowerDir::ALL {
            assert_eq!(PowerDir::of_group(d.group()), Some(d));
            // The clip's stem is the group in lower case.
            assert_eq!(d.stem(), d.group().to_ascii_lowercase());
        }
        assert_eq!(PowerDir::of_group("attackleftpower"), Some(PowerDir::Left));
        assert_eq!(PowerDir::of_group("AttackRight"), None);
    }

    #[test]
    fn holding_makes_a_power_attack_in_the_direction_moved() {
        let mut m = Melee::default();
        let ev = run(&mut m, 2.5, 1.0, |t| t < 1.0);
        let s: Vec<_> = ev.iter().filter_map(|e| if let MeleeEvent::Started { stem, power, .. } = e { Some((stem.clone(), *power)) } else { None }).collect();
        assert_eq!(s, [("attackforwardpower".to_string(), Some(PowerDir::Forward))]);
        assert!(ev.contains(&MeleeEvent::Strike { power: Some(PowerDir::Forward) }));
        // Standing still: the standing one.
        let mut m = Melee::default();
        let s = started(&run(&mut m, 2.5, 0.0, |t| t < 1.0));
        assert_eq!(s, [("attackpower".to_string(), false)]);
    }

    #[test]
    fn a_power_attack_override_replaces_the_standing_one() {
        // The Machete Gladius's AttackCustom3Power: standing, the override plays; moving,
        // the direction's own.
        let mut m = Melee::default();
        let s = started(&run_with(&mut m, 2.5, 0.0, Some("attackcustom3power"), |t| t < 1.0));
        assert_eq!(s, [("attackcustom3power".to_string(), false)]);
        let mut m = Melee::default();
        let ev = run_with(&mut m, 2.5, 1.0, Some("attackcustom3power"), |t| t < 1.0);
        assert!(ev.iter().any(|e| matches!(e, MeleeEvent::Started { stem, power: Some(PowerDir::Forward), .. } if stem == "attackforwardpower")));
        assert!(ev.contains(&MeleeEvent::Strike { power: Some(PowerDir::Forward) }));
    }

    #[test]
    fn a_power_attack_cuts_in_at_the_a_key_but_is_not_cut() {
        // Tap, then hold during the right swing: the power attack starts at a:L; a tap
        // during the power attack waits for its end and starts a new run.
        let mut m = Melee::default();
        let s = started(&run(&mut m, 4.5, 0.0, |t| t < 0.05 || (0.1..1.0).contains(&t) || (1.5..1.55).contains(&t)));
        assert_eq!(s.iter().map(|(x, _)| x.as_str()).collect::<Vec<_>>(), ["attackright_a", "attackpower", "attackright_a"], "{s:?}");
    }

    #[test]
    fn clip_timing_scales_with_speed() {
        let t = SwingTiming::of(Some(1.2), Some(0.3), Some((0.6, NextAttack::Left)), 2.0, 1.0);
        assert_eq!(t, SwingTiming { len: 0.6, hit: 0.15, chain: Some((0.3, NextAttack::Left)) });
        let f = SwingTiming::of(None, None, None, 1.0, 0.8);
        assert!((f.hit - 0.32).abs() < 1e-6 && f.chain.is_none());
    }
}
