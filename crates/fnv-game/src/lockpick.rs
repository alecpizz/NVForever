//! Lockpicking (docs/spec-notes/lockpicking.md): a locked door or container is picked
//! with a bobby pin and a screwdriver. The pin is turned to an angle on a half circle,
//! from the left (0°) over the top to the right (180°); the screwdriver then puts torque
//! on the lock. The nearer the pin is to the lock's sweet spot, the further the lock
//! turns; in the sweet spot it turns all the way (90°) and opens. A lock that stops
//! short strains the pin, which loses health and breaks when it has none left; the next
//! pin from the player's inventory goes in. Forcing the lock opens it or breaks it for
//! good.
//!
//! Pure rules: the engine's menu (`nvf-play` `lockpick`) feeds [`Picking::step`] the
//! pin's angle and whether torque is held, and draws what it reports. What the original
//! does is documented only in outline (its interface file, its help message, its menu
//! code and the scripting functions about locks); every number not tagged otherwise is
//! NVForever's own [design] and listed in docs/owner-review.md.

use crate::functions::actor::actor_value_capped;
use crate::game::Game;
use fnv_sim::FormId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The Bobby Pin: MISC `Lockpick`, FULL "Bobby Pin" [observed: FalloutNV.esm 0x0000000A].
pub const BOBBY_PIN: FormId = FormId(0x0000_000A);

/// The help message the original shows the first time (MESG `HelpLockpicking`, "Message
/// Box" and "Auto Display") [observed: FalloutNV.esm 0x0000017C; the owner's screenshot
/// shows it over the menu the first time, as a tutorial box titled "Lockpicking"].
pub const HELP_MESSAGE: FormId = FormId(0x0000_017C);

/// The lock level of a door or container that only its key opens [documented: GECK wiki
/// GetLockLevel, 255 "impossible"; observed: 172 placed locks, simulation.md §5].
pub const KEY_ONLY: u8 = 255;

/// The pin's angle runs from 0 (pointing left) over 90 (up) to 180 (right) [the owner,
/// 2026-10-11: "a 180 degree from left to right"].
pub const PIN_RANGE: f32 = 180.0;

/// How far the lock turns to open [the owner: "the lock will be able to turn a full 90
/// degrees"; observed: the keyhole upright at rest and lying flat as the lock opened on
/// the owner's screenshots].
pub const FULL_TURN: f32 = 90.0;

/// A new pin's health [documented: GECK wiki SetBobbyPinHealth, "the standard health
/// initialization value of a new bobby pin will start at 1.0"].
pub const PIN_HEALTH: f32 = 1.0;

/// A lock's difficulty [documented: GECK wiki GetLockLevel and Bethsoft Tutorial
/// Non-NPC population: 0 Very Easy, 25 Easy, 50 Average, 75 Hard, 100 Very Hard, 255
/// impossible ("Requires Key")].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    VeryEasy,
    Easy,
    Average,
    Hard,
    VeryHard,
    RequiresKey,
}

impl Level {
    /// The level of an XLOC lock level. Placed locks use only the six values [observed,
    /// simulation.md §5]; a level between two is counted as the harder one, above 100 as
    /// Requires Key [hypothesis: `iLockLevelMax…` settings name such bounds, GECK wiki
    /// Settings; their values are not known].
    pub fn of(level: u8) -> Level {
        match level {
            0 => Level::VeryEasy,
            1..=25 => Level::Easy,
            26..=50 => Level::Average,
            51..=75 => Level::Hard,
            76..=100 => Level::VeryHard,
            _ => Level::RequiresKey,
        }
    }

    /// 0 Very Easy … 4 Very Hard, 5 Requires Key (the terminal numbering, GECK wiki
    /// GetLockLevel; also the XP classes' `iXPLevelPickLock…` defaults 0 to 4,
    /// GECK wiki Experience Settings).
    pub fn index(self) -> usize {
        self as usize
    }

    /// The Lockpick skill needed to try [documented: GECK wiki Bethsoft Tutorial Non-NPC
    /// population, "Skill to Pick": 0, 25, 50, 75, 100 (written for Fallout 3); the owner
    /// asks for the same in NVForever]. `None`: only the key opens it.
    pub fn required_skill(self) -> Option<f32> {
        match self {
            Level::RequiresKey => None,
            l => Some(25.0 * l.index() as f32),
        }
    }

    /// The name the menu shows: the level's name as the GECK wiki's GetLockLevel and the
    /// Bethsoft tutorial name the levels [documented: see [`Level`]], not read off a screen
    /// (#2984).
    pub fn name(self) -> &'static str {
        match self {
            Level::VeryEasy => "Very Easy",
            Level::Easy => "Easy",
            Level::Average => "Average",
            Level::Hard => "Hard",
            Level::VeryHard => "Very Hard",
            Level::RequiresKey => "Requires Key",
        }
    }

    const XP_SETTINGS: [(&'static str, f64); 5] = [
        ("iXPRewardPickLockVeryEasy", 20.0),
        ("iXPRewardPickLockEasy", 30.0),
        ("iXPRewardPickLockAverage", 40.0),
        ("iXPRewardPickLockHard", 50.0),
        ("iXPRewardPickLockVeryHard", 60.0),
    ];
}

/// The numbers of the game. The original's interface file has tuning knobs for a "Sweet
/// Spot" and a "Concentric" value (debug buttons of `menus\lockpick_menu.xml`)
/// [documented]: the sweet spot where the lock turns fully and a wider ring around it
/// where it turns part way. Their sizes and the rest are NVForever's [design].
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// The sweet spot's whole width in degrees at the lock's own skill, Very Easy to
    /// Very Hard [design; the owner: "lock difficulty determines how small the sweet
    /// spot angle is"].
    pub sweet: [f32; 5],
    /// Every point of Lockpick above what the lock needs widens the sweet spot by this
    /// part of its width (so 100 points over doubles it) [design].
    pub widen_per_point: f32,
    /// The ring around the sweet spot, degrees beyond its edge on each side, in which the
    /// lock turns part way, less the further out [design].
    pub concentric: f32,
    /// The least the lock gives outside the ring, degrees, so that torque always shows a
    /// little play [design].
    pub play: f32,
    /// How fast the lock turns under torque and springs back without, degrees a second
    /// [design].
    pub turn_speed: f32,
    pub return_speed: f32,
    /// Seconds a pin lasts held against a lock that will not turn, at the lock's own
    /// skill, and the seconds more for every point of Lockpick above it [design].
    pub pin_seconds: f32,
    pub pin_seconds_per_point: f32,
    /// After a pin breaks, seconds until the next goes in [hypothesis: the length of the
    /// pin model's break, its `Backward` (0.97 s, the pin twisting) and `Left` (0.47 s, the
    /// long piece flying off) sequences, observed in `bobbypin01.nif`; lockpicking.md §2.1].
    pub new_pin_seconds: f32,
    /// The chance to force the lock: this many percent a point of Lockpick above what
    /// the lock needs, at most `force_max` [hypothesis: fits the one value seen, "Force
    /// Lock [15%]" at Lockpick 30 on an Easy lock (25), on the owner's screenshots].
    pub force_per_point: f32,
    pub force_max: f32,
    /// NVForever's tweak (docs/design/tweaks.md): percent added to the force chance with
    /// the pin on the sweet spot's middle, less the further it is, none 90° away.
    pub force_near_bonus: f32,
    /// NVForever's tweak (docs/design/tweaks.md): a failed Force Lock breaks the pin, not
    /// the lock.
    pub force_spares_lock: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            sweet: [30.0, 20.0, 12.0, 8.0, 5.0],
            widen_per_point: 0.01,
            concentric: 45.0,
            play: 3.0,
            turn_speed: 120.0,
            return_speed: 240.0,
            pin_seconds: 1.0,
            pin_seconds_per_point: 0.02,
            new_pin_seconds: 1.45,
            force_per_point: 3.0,
            force_max: 95.0,
            force_near_bonus: 0.0,
            force_spares_lock: false,
        }
    }
}

/// Why the lockpicking menu does not open.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Refusal {
    /// Level 255: only its key opens it.
    RequiresKey,
    /// The lock was broken by forcing it: only its key opens it now [documented: the
    /// help message, "if you fail the lock will be permanently broken and can only be
    /// opened with a key"].
    Broken,
    /// The player's Lockpick is below what the lock needs.
    Skill { needed: f32 },
    /// No bobby pin in the inventory [the owner: "this needs bobby pins from the misc
    /// items to attempt"].
    NoPins,
}

/// Whether a player of Lockpick `skill` with `pins` bobby pins may pick a lock of `level`.
pub fn may_pick(level: Level, skill: f32, pins: i32, broken: bool) -> Result<(), Refusal> {
    if broken {
        return Err(Refusal::Broken);
    }
    let Some(needed) = level.required_skill() else { return Err(Refusal::RequiresKey) };
    if skill < needed {
        return Err(Refusal::Skill { needed });
    }
    if pins <= 0 {
        return Err(Refusal::NoPins);
    }
    Ok(())
}

/// Lockpick points above what the lock needs (0 to 100).
fn margin(level: Level, skill: f32) -> f32 {
    (skill - level.required_skill().unwrap_or(100.0)).clamp(0.0, 100.0)
}

/// The sweet spot's half width, degrees.
pub fn sweet_half_width(level: Level, skill: f32, s: &Settings) -> f32 {
    let base = s.sweet[level.index().min(4)];
    base * (1.0 + s.widen_per_point * margin(level, skill)) / 2.0
}

/// The chance, 0 to 100, that forcing the lock opens it.
pub fn force_chance(level: Level, skill: f32, s: &Settings) -> f32 {
    if level == Level::RequiresKey {
        return 0.0;
    }
    (s.force_per_point * (skill - level.required_skill().unwrap_or(100.0))).clamp(0.0, s.force_max)
}

/// Seconds a pin lasts against a lock that will not turn.
pub fn pin_seconds(level: Level, skill: f32, s: &Settings) -> f32 {
    s.pin_seconds + s.pin_seconds_per_point * margin(level, skill)
}

/// How far the lock turns with the pin `distance` degrees from the sweet spot's middle:
/// all the way inside the sweet spot, then less in a straight line across the ring
/// around it, and only the play beyond [the owner: "the further the lock rotates the
/// closer you are to the spot"; design: the straight line].
pub fn max_turn(distance: f32, half: f32, s: &Settings) -> f32 {
    let d = distance.abs();
    if d <= half {
        return FULL_TURN;
    }
    let part = 1.0 - (d - half) / s.concentric.max(f32::EPSILON);
    (FULL_TURN * part).max(s.play).min(FULL_TURN)
}

/// Where the game is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// A pin is in; the player turns it and puts torque on the lock.
    Picking,
    /// The pin broke this many seconds ago; the next goes in after
    /// [`Settings::new_pin_seconds`], if there is one.
    PinBroken { since: f32 },
    /// The last pin broke.
    OutOfPins,
    /// The lock turned all the way: it is open.
    Open,
    /// Forcing it failed: broken for good.
    LockBroken,
}

/// What a step did, for the engine's sounds and log.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// The lock started turning (torque applied).
    Turning,
    /// The lock stopped short of opening, at this angle: the pin is under strain.
    Stopped { at: f32 },
    /// The pin broke; `left` pins are in the inventory now (one was taken away).
    PinBroke { left: i32 },
    /// The next pin is in.
    NewPin,
    /// The lock turned all the way.
    Opened,
}

/// One go at a lock, from the menu opening to it closing.
#[derive(Debug, Clone, PartialEq)]
pub struct Picking {
    pub level: Level,
    pub skill: f32,
    /// The sweet spot's middle and half width, degrees.
    pub sweet: f32,
    pub half: f32,
    /// The pin's angle (0 left, 90 up, 180 right).
    pub pin: f32,
    /// How far the lock is turned, 0 to 90.
    pub turn: f32,
    /// The pin's health, [`PIN_HEALTH`] new, broken at 0.
    pub health: f32,
    /// Bobby pins in the inventory, the one in the lock included.
    pub pins: i32,
    pub phase: Phase,
    /// Torque was held in the last step.
    pub torque: bool,
    /// The lock stopped short under the torque now held.
    pub stopped: bool,
    pub settings: Settings,
}

impl Picking {
    /// A new go: the sweet spot's middle is `spot` of the way along the half circle
    /// (0 to 1, chosen at random by the caller) [hypothesis: each time the menu opens;
    /// whether the original keeps it per lock is not known]. The pin starts pointing up
    /// [observed: the pin straight up as the menu opened on the owner's screenshots].
    pub fn new(level: Level, skill: f32, pins: i32, spot: f32, settings: Settings) -> Picking {
        let half = sweet_half_width(level, skill, &settings);
        // The middle is kept so that the whole sweet spot lies on the half circle.
        let lo = half.min(PIN_RANGE / 2.0);
        let sweet = lo + spot.clamp(0.0, 1.0) * (PIN_RANGE - 2.0 * lo);
        Picking {
            level,
            skill,
            sweet,
            half,
            pin: PIN_RANGE / 2.0,
            turn: 0.0,
            health: PIN_HEALTH,
            pins,
            phase: Phase::Picking,
            torque: false,
            stopped: false,
            settings,
        }
    }

    /// How far the lock can turn with the pin where it is.
    pub fn max_turn(&self) -> f32 {
        max_turn(self.pin - self.sweet, self.half, &self.settings)
    }

    pub fn in_sweet_spot(&self) -> bool {
        (self.pin - self.sweet).abs() <= self.half
    }

    /// Move the pin to `angle`: only with a pin in, no torque held and the lock back at
    /// rest [design: the pin is held by the turned cylinder]. Whether it moved.
    pub fn set_pin(&mut self, angle: f32) -> bool {
        if self.phase != Phase::Picking || self.torque || self.turn > 0.0 {
            return false;
        }
        let a = angle.clamp(0.0, PIN_RANGE);
        let moved = a != self.pin;
        self.pin = a;
        moved
    }

    /// Advance `dt` seconds with torque held or not.
    pub fn step(&mut self, dt: f32, torque: bool) -> Vec<Event> {
        let mut out = Vec::new();
        let s = self.settings.clone();
        match self.phase {
            Phase::PinBroken { since } => {
                let since = since + dt;
                self.turn = (self.turn - s.return_speed * dt).max(0.0);
                if since >= s.new_pin_seconds {
                    self.health = PIN_HEALTH;
                    self.phase = Phase::Picking;
                    self.torque = false;
                    out.push(Event::NewPin);
                } else {
                    self.phase = Phase::PinBroken { since };
                }
                return out;
            }
            Phase::OutOfPins => {
                self.turn = (self.turn - s.return_speed * dt).max(0.0);
                return out;
            }
            Phase::Open | Phase::LockBroken => return out,
            Phase::Picking => {}
        }
        if torque && !self.torque {
            out.push(Event::Turning);
        }
        self.torque = torque;
        if !torque {
            self.stopped = false;
            self.turn = (self.turn - s.return_speed * dt).max(0.0);
            return out;
        }
        let limit = self.max_turn();
        if self.turn < limit {
            self.turn = (self.turn + s.turn_speed * dt).min(limit);
            if self.turn >= FULL_TURN {
                self.phase = Phase::Open;
                out.push(Event::Opened);
                return out;
            }
            if self.turn < limit {
                return out;
            }
        }
        // Against the stop: the pin strains [the owner: "applying torque while not in
        // the sweet spot, when the lock stops turning, will drain the health of the pin"].
        if !self.stopped {
            self.stopped = true;
            out.push(Event::Stopped { at: self.turn });
        }
        self.health -= dt / pin_seconds(self.level, self.skill, &s).max(f32::EPSILON);
        if self.health <= 0.0 {
            self.health = 0.0;
            self.pins = (self.pins - 1).max(0);
            self.stopped = false;
            out.push(Event::PinBroke { left: self.pins });
            self.phase = if self.pins > 0 { Phase::PinBroken { since: 0.0 } } else { Phase::OutOfPins };
        }
        out
    }

    /// The chance to force this lock, 0 to 100.
    pub fn force_chance(&self) -> f32 {
        let base = force_chance(self.level, self.skill, &self.settings);
        // The tweak's bonus near the sweet spot [design: docs/design/tweaks.md].
        let near = (1.0 - (self.pin - self.sweet).abs() / 90.0).clamp(0.0, 1.0);
        if self.settings.force_near_bonus <= 0.0 || self.level == Level::RequiresKey {
            return base;
        }
        (base + self.settings.force_near_bonus * near).clamp(0.0, self.settings.force_max)
    }

    /// Force the lock with a roll in [0, 100): it opens when the roll is under the
    /// chance, else it breaks for good and the pin in it with it [documented: the help
    /// message; design: that the pin is lost]. Only with a pin in.
    pub fn force(&mut self, roll: f32) -> Option<bool> {
        if self.phase != Phase::Picking {
            return None;
        }
        if roll < self.force_chance() {
            self.turn = FULL_TURN;
            self.phase = Phase::Open;
            Some(true)
        } else if self.settings.force_spares_lock {
            // The tweak: only the pin breaks; the next goes in as after a snap [design: docs/design/tweaks.md].
            self.pins = (self.pins - 1).max(0);
            self.health = 0.0;
            self.turn = 0.0;
            self.phase = if self.pins > 0 { Phase::PinBroken { since: 0.0 } } else { Phase::OutOfPins };
            Some(false)
        } else {
            self.pins = (self.pins - 1).max(0);
            self.health = 0.0;
            self.phase = Phase::LockBroken;
            Some(false)
        }
    }
}

/// Lockpicking's part of the game state, kept in the snapshot ([`crate::game::GameSnapshot`]
/// `locks`): the locks broken by forcing them, which stay broken (only their key opens them
/// [documented: the help message]), and whether the help message has been shown.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Locks {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub broken: BTreeSet<FormId>,
    /// Broken locks whose Infiltrator try was spent too: broken again (§7 of
    /// perk-entry-points.md).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub retried: BTreeSet<FormId>,
    /// The help message ([`HELP_MESSAGE`]) has been shown: it comes the first time the menu
    /// opens only [hypothesis: "Auto Display" shows a help message once per game].
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub help_shown: bool,
}

impl Locks {
    pub fn is_empty(&self) -> bool {
        self.broken.is_empty() && self.retried.is_empty() && !self.help_shown
    }
}

/// The help message to show as the menu opens: its title and text the first time (and it
/// counts as shown from then on), else `None`. `None` too without the message record.
pub fn first_help(g: &mut Game) -> Option<(String, String)> {
    // The player's tweak: no first-time help [design: docs/design/tweaks.md].
    if g.locks.help_shown || g.tweaks.no_tutorials {
        return None;
    }
    let m = g.data.messages.get(&HELP_MESSAGE)?;
    let text = m.text.clone()?;
    g.locks.help_shown = true;
    Some((m.title.clone().unwrap_or_default(), text))
}

/// What activating a locked door or container offers the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Opening {
    /// The menu opens.
    Pick { level: Level, skill: f32, pins: i32 },
    Refused(Refusal),
    /// Not locked (or no lock data).
    NotLocked,
}

/// The player's Lockpick skill (0 to 100).
pub fn skill(g: &mut Game) -> f32 {
    actor_value_capped(g, FormId::PLAYER_REF, crate::actor_values::LOCKPICK) as f32
}

/// The bobby pins the player carries.
pub fn pins(g: &mut Game) -> i32 {
    g.world.item_count(FormId::PLAYER_REF, BOBBY_PIN).unwrap_or(0)
}

/// What activating the locked `reference` does for a player of Lockpick `skill`. The key
/// is not looked for: activation opens a lock whose key the player carries before this
/// is asked (`fnv_sim::World::activate`, simulation.md §5).
pub fn opening(g: &mut Game, reference: FormId, skill: f32) -> Opening {
    let Some(lock) = g.world.lock_of(reference).filter(|l| l.locked) else { return Opening::NotLocked };
    let reference = g.world.lock_source(reference);
    let level = Level::of(lock.level);
    let n = pins(g);
    // Infiltrator's Ignore Broken Lock: one more try at a broken lock [documented: GECK
    // wiki Entry Point, "if the value is modified to be non-zero, another chance will be
    // granted"; observed: the perk, "can make one more attempt to pick a broken lock"].
    let broken = g.locks.broken.contains(&reference);
    let again = broken && !g.locks.retried.contains(&reference) && g.player_has_entry(crate::perks::IGNORE_BROKEN_LOCK);
    if again {
        tracing::info!("lockpick: {reference} is broken; Ignore Broken Lock (Infiltrator) gives one more try");
    }
    // The player's tweak: a lock well under the skill opens at once [design: docs/design/tweaks.md]; the door or
    // container is then used with the next press.
    if g.tweaks.skip_easy_locks && !broken && level.required_skill().is_some_and(|n| skill >= n + g.tweaks.skip_lock_margin) {
        tracing::info!("lockpick: {reference} opened without the game (Lockpick {skill}, tweak)");
        opened(g, reference, level, true);
        return Opening::NotLocked;
    }
    // The player's tweak: the skill is not asked for (the lock itself plays as before) [design: docs/design/tweaks.md].
    let asked = if g.tweaks.pick_locks_without_skill { skill.max(100.0) } else { skill };
    match may_pick(level, asked, n, broken && !again) {
        Ok(()) => Opening::Pick { level, skill, pins: n },
        Err(r) => Opening::Refused(r),
    }
}

/// Take a broken pin out of the player's inventory.
pub fn pin_broke(g: &mut Game) {
    let _ = g.world.remove_item(FormId::PLAYER_REF, BOBBY_PIN, 1);
}

/// The XP for picking a lock of `level` [documented: GECK wiki Experience Settings,
/// `iXPRewardPickLock<class>` with defaults 20 to 60; observed: FalloutNV.esm sets the
/// same five values].
pub fn xp(g: &Game, level: Level) -> f64 {
    let Some(&(name, default)) = Level::XP_SETTINGS.get(level.index()) else { return 0.0 };
    crate::combat::setting(g, name, default)
}

/// The lock opened: it is unlocked for good [observed: a lock opened with its key stays
/// unlocked, simulation.md §5; hypothesis: the same for a picked or forced lock] and the
/// player gets the XP (picked, not forced [hypothesis]). Returns the owner of the lock,
/// whose lock it was a crime to pick (the crime itself is not built).
pub fn opened(g: &mut Game, reference: FormId, level: Level, picked: bool) -> Option<FormId> {
    let _ = g.world.unlock(reference);
    if picked {
        g.count_stat(crate::misc_stats::LOCKS_PICKED, 1.0);
        let x = xp(g, level);
        crate::experience::award(g, crate::experience::Source::PickLock, x);
    }
    owner(g, reference)
}

/// Forcing failed: the lock is broken for good; GetLocked reads 2 and GetIsLockBroken 1
/// [documented: GECK wiki GetLocked, GetIsLockBroken]. The pin goes too.
pub fn broke(g: &mut Game, reference: FormId) {
    // Broken/second-try state belongs to the applicable lock [hypothesis, #2538].
    let reference = g.world.lock_source(reference);
    // Broken again on Infiltrator's try: that try is spent.
    if !g.locks.broken.insert(reference) {
        g.locks.retried.insert(reference);
    }
    pin_broke(g);
}

/// Who owns a door or container: its own owner (XOWN), else its cell's (its record's, or
/// SetCellOwnership's) [documented: GECK wiki ownership of references and cells;
/// `crate::crime::Game::ownership`].
pub fn owner(g: &Game, reference: FormId) -> Option<FormId> {
    g.ownership(reference).map(|o| o.owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s() -> Settings {
        Settings::default()
    }

    #[test]
    fn levels_and_skills() {
        assert_eq!(Level::of(0), Level::VeryEasy);
        assert_eq!(Level::of(25), Level::Easy);
        assert_eq!(Level::of(50), Level::Average);
        assert_eq!(Level::of(75), Level::Hard);
        assert_eq!(Level::of(100), Level::VeryHard);
        assert_eq!(Level::of(255), Level::RequiresKey);
        assert_eq!(Level::Hard.required_skill(), Some(75.0));
        assert_eq!(Level::RequiresKey.required_skill(), None);
        assert_eq!(may_pick(Level::Easy, 30.0, 7, false), Ok(()));
        assert_eq!(may_pick(Level::Average, 30.0, 7, false), Err(Refusal::Skill { needed: 50.0 }));
        assert_eq!(may_pick(Level::Easy, 30.0, 0, false), Err(Refusal::NoPins));
        assert_eq!(may_pick(Level::RequiresKey, 100.0, 7, false), Err(Refusal::RequiresKey));
        assert_eq!(may_pick(Level::VeryEasy, 100.0, 7, true), Err(Refusal::Broken));
    }

    #[test]
    fn harder_locks_have_smaller_sweet_spots_and_skill_widens_them() {
        let w: Vec<f32> = [Level::VeryEasy, Level::Easy, Level::Average, Level::Hard, Level::VeryHard]
            .iter()
            .map(|&l| sweet_half_width(l, l.required_skill().unwrap(), &s()))
            .collect();
        assert!(w.windows(2).all(|p| p[0] > p[1]), "{w:?}");
        assert_eq!(sweet_half_width(Level::Easy, 25.0, &s()), 10.0);
        // 50 points above an Easy lock: one and a half times as wide.
        assert_eq!(sweet_half_width(Level::Easy, 75.0, &s()), 15.0);
    }

    #[test]
    fn the_lock_turns_further_the_nearer_the_pin() {
        let st = s();
        assert_eq!(max_turn(0.0, 10.0, &st), FULL_TURN);
        assert_eq!(max_turn(-10.0, 10.0, &st), FULL_TURN);
        let near = max_turn(15.0, 10.0, &st);
        let far = max_turn(40.0, 10.0, &st);
        assert!(near < FULL_TURN && far < near, "{near} {far}");
        // Beyond the ring: only the play.
        assert_eq!(max_turn(100.0, 10.0, &st), st.play);
        // Halfway across the ring: half a turn.
        assert!((max_turn(10.0 + 22.5, 10.0, &st) - 45.0).abs() < 1e-4);
    }

    #[test]
    fn the_tweak_adds_to_force_lock_near_the_sweet_spot() {
        // Lockpick 30 on an Easy lock: 15% plain; with a 40% bonus, 55% on the middle,
        // 35% 45 degrees off, 15% at 90 and beyond.
        let settings = Settings { force_near_bonus: 40.0, ..s() };
        let mut p = Picking::new(Level::Easy, 30.0, 3, 0.5, settings);
        let middle = p.sweet;
        for (off, want) in [(0.0, 55.0), (45.0, 35.0), (90.0, 15.0)] {
            p.pin = middle + off;
            assert!((p.force_chance() - want).abs() < 1e-3, "{off}: {}", p.force_chance());
        }
        assert_eq!(Picking::new(Level::Easy, 30.0, 3, 0.5, s()).force_chance(), 15.0);
    }

    #[test]
    fn the_tweak_keeps_the_lock_when_forcing_fails() {
        let mut p = Picking::new(Level::Easy, 30.0, 2, 0.5, Settings { force_spares_lock: true, ..s() });
        assert_eq!(p.force(99.0), Some(false));
        assert!(matches!(p.phase, Phase::PinBroken { .. }), "{:?}", p.phase);
        assert_eq!(p.pins, 1);
        let mut q = Picking::new(Level::Easy, 30.0, 2, 0.5, s());
        assert_eq!(q.force(99.0), Some(false));
        assert_eq!(q.phase, Phase::LockBroken);
    }

    #[test]
    fn force_chance_fits_the_screenshot() {
        assert_eq!(force_chance(Level::Easy, 30.0, &s()), 15.0);
        assert_eq!(force_chance(Level::Easy, 25.0, &s()), 0.0);
        assert_eq!(force_chance(Level::VeryEasy, 100.0, &s()), 95.0);
        assert_eq!(force_chance(Level::RequiresKey, 100.0, &s()), 0.0);
    }

    fn run(p: &mut Picking, secs: f32, torque: bool) -> Vec<Event> {
        let mut out = Vec::new();
        let dt = 1.0 / 60.0;
        let mut t = 0.0;
        while t < secs {
            out.extend(p.step(dt, torque));
            t += dt;
        }
        out
    }

    /// Run until an event `stop` says so (or `secs` are up).
    fn run_until(p: &mut Picking, secs: f32, torque: bool, stop: impl Fn(&Event) -> bool) -> Vec<Event> {
        let mut out = Vec::new();
        let dt = 1.0 / 60.0;
        let mut t = 0.0;
        while t < secs {
            let ev = p.step(dt, torque);
            let done = ev.iter().any(&stop);
            out.extend(ev);
            if done {
                break;
            }
            t += dt;
        }
        out
    }

    #[test]
    fn in_the_sweet_spot_the_lock_opens() {
        let mut p = Picking::new(Level::Easy, 30.0, 3, 0.5, s());
        assert_eq!(p.pin, 90.0);
        assert!(p.set_pin(p.sweet + p.half * 0.5));
        assert!(p.in_sweet_spot());
        let ev = run(&mut p, 1.0, true);
        assert_eq!(ev.first(), Some(&Event::Turning));
        assert!(ev.contains(&Event::Opened));
        assert_eq!(p.phase, Phase::Open);
        assert_eq!(p.turn, FULL_TURN);
        assert_eq!(p.health, PIN_HEALTH);
        assert_eq!(p.pins, 3);
    }

    #[test]
    fn off_the_spot_the_lock_stops_and_the_pin_wears_and_breaks() {
        let mut p = Picking::new(Level::Easy, 25.0, 2, 0.0, s());
        // The sweet spot lies at the left end; the pin goes 30 degrees past its edge.
        p.set_pin(p.sweet + p.half + 30.0);
        let limit = p.max_turn();
        assert!(limit > 0.0 && limit < FULL_TURN);
        let ev = run(&mut p, 0.6, true);
        assert!(ev.iter().any(|e| matches!(e, Event::Stopped { .. })), "{ev:?}");
        assert!((p.turn - limit).abs() < 1e-3);
        assert!(p.health < PIN_HEALTH && p.health > 0.0, "{}", p.health);
        // The pin cannot move while the lock is turned.
        assert!(!p.set_pin(10.0));
        // Let go: the lock springs back and the pin keeps its wear.
        let worn = p.health;
        run(&mut p, 0.5, false);
        assert_eq!(p.turn, 0.0);
        assert_eq!(p.health, worn);
        // Strain it to breaking: one pin left, a new one goes in after a while.
        let ev = run_until(&mut p, 2.0, true, |e| matches!(e, Event::PinBroke { .. }));
        assert!(ev.contains(&Event::PinBroke { left: 1 }), "{ev:?}");
        assert!(matches!(p.phase, Phase::PinBroken { .. }));
        // Not before its time, even with torque held.
        let ev = run(&mut p, 0.5, true);
        assert!(!ev.contains(&Event::NewPin));
        let ev = run(&mut p, 1.0, false);
        assert!(ev.contains(&Event::NewPin));
        assert_eq!((p.phase, p.health, p.pins), (Phase::Picking, PIN_HEALTH, 1));
        // The last pin breaks: out of pins.
        p.set_pin(p.sweet + p.half + 30.0);
        let ev = run(&mut p, 3.0, true);
        assert!(ev.contains(&Event::PinBroke { left: 0 }));
        assert_eq!(p.phase, Phase::OutOfPins);
    }

    #[test]
    fn skill_makes_pins_last_longer() {
        assert!(pin_seconds(Level::Easy, 75.0, &s()) > pin_seconds(Level::Easy, 25.0, &s()));
    }

    #[test]
    fn the_sweet_spot_lies_on_the_half_circle() {
        for spot in [0.0, 0.3, 1.0] {
            let p = Picking::new(Level::VeryEasy, 100.0, 1, spot, s());
            assert!(p.sweet - p.half >= -1e-4 && p.sweet + p.half <= PIN_RANGE + 1e-4, "{spot}: {} ± {}", p.sweet, p.half);
        }
    }

    #[test]
    fn broken_locks_and_the_help_are_kept_in_the_snapshot_form() {
        let mut l = Locks::default();
        assert!(l.is_empty());
        assert_eq!(serde_json::to_string(&l).unwrap(), "{}");
        l.broken.insert(FormId(0x0010_B2D4));
        l.help_shown = true;
        let back: Locks = serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
        assert_eq!(back, l);
        assert!(!back.is_empty());
    }

    #[test]
    fn forcing_opens_or_breaks() {
        let mut p = Picking::new(Level::Easy, 30.0, 2, 0.5, s());
        assert_eq!(p.force(14.9), Some(true));
        assert_eq!(p.phase, Phase::Open);
        let mut p = Picking::new(Level::Easy, 30.0, 2, 0.5, s());
        assert_eq!(p.force(15.0), Some(false));
        assert_eq!((p.phase, p.pins), (Phase::LockBroken, 1));
        assert_eq!(p.force(0.0), None);
    }
}
