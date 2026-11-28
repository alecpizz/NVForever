//! The Wait / Sleep menu as plain data (docs/spec-notes/wait-menu.md): the hours chosen,
//! the hours still to pass once accepted, and the lines the menu shows. No Bevy.

use bevy::prelude::KeyCode;
use fnv_sim::clock::Calendar;

/// The most hours one wait takes [observed: the slider of the owner's screenshot has 24
/// hour marks; "at most 24 at a time", docs/spec-notes/simulation.md §6].
pub const MAX_HOURS: u32 = 24;

/// The slider's page step, hours [documented: the interface file's slider has a jump
/// size of 6 next to a step size of 1].
pub const JUMP: i32 = 6;

/// What the player does in the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Put the slider on this many hours.
    Hours(u32),
    /// Move the slider by so many hours (one for a step, [`JUMP`] for a page).
    Step(i32),
    /// Wait (or sleep) the chosen hours.
    Accept,
    /// Close the menu; while hours pass, stop the wait there.
    Cancel,
}

/// The hours of an accepted wait.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Passing {
    pub asked: u32,
    pub left: u32,
    /// Seconds since the last hour passed.
    since: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// The player lies on a bed: the menu is a Sleep menu.
    pub sleep: bool,
    /// The slider, 1 to [`MAX_HOURS`].
    pub hours: u32,
    pub passing: Option<Passing>,
}

/// The menu's lines.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub title: String,
    /// "22 hours (8:34)": the hours chosen (or still to pass) and the time of day at
    /// their end.
    pub hours: String,
    /// "Saturday, 19.10.81, 10:34": the game's date and time now.
    pub date: String,
    /// The slider mark the marker stands on.
    pub marker: u32,
    pub accept: String,
    /// The verb alone ("Wait", "Sleep"), for the menu file's Wait button, which shows its
    /// key hint itself (`crate::tiles::wait_menu`).
    pub action: String,
    pub cancel: String,
    /// Not while the hours pass.
    pub can_accept: bool,
}

/// The hours of a place on the slider's length, 0 at its first end mark and 1 at its
/// last: the hour marks 1 to 24 are 25 steps apart from end to end.
pub fn hours_at(part: f32) -> u32 {
    ((part * (MAX_HOURS + 1) as f32).round() as i64).clamp(1, MAX_HOURS as i64) as u32
}

impl Model {
    /// The menu as it opens: one hour chosen [engineering choice: what the original
    /// opens on is not captured].
    pub fn open(sleep: bool) -> Model {
        Model { sleep, hours: 1, passing: None }
    }

    /// Apply a command; returns whether the menu closes. The slider cannot be moved
    /// while the hours pass.
    pub fn apply(&mut self, cmd: Command) -> bool {
        match (cmd, self.passing.is_some()) {
            (Command::Cancel, _) => return true,
            (Command::Hours(h), false) => self.hours = h.clamp(1, MAX_HOURS),
            (Command::Step(d), false) => self.hours = (self.hours as i64 + d as i64).clamp(1, MAX_HOURS as i64) as u32,
            (Command::Accept, false) => self.passing = Some(Passing { asked: self.hours, left: self.hours, since: 0.0 }),
            (_, true) => {}
        }
        false
    }

    /// A script makes the player sleep `hours` (`SetPCSleepHours`): they pass as if
    /// accepted, at most [`MAX_HOURS`]; none for 0. A sleep already passing takes the new
    /// count.
    pub fn sleep_for(&mut self, hours: u32) {
        let hours = hours.min(MAX_HOURS);
        self.sleep = true;
        self.hours = hours.max(1);
        self.passing = (hours > 0).then_some(Passing { asked: hours, left: hours, since: 0.0 });
    }

    /// `dt` seconds on while the hours pass, one hour every `hour_seconds`: whether an
    /// hour passed now (at most one a frame, so each is seen on its own).
    pub fn tick(&mut self, dt: f32, hour_seconds: f32) -> bool {
        let Some(p) = self.passing.as_mut().filter(|p| p.left > 0) else { return false };
        p.since += dt.max(0.0);
        if p.since < hour_seconds {
            return false;
        }
        p.since = (p.since - hour_seconds).min(hour_seconds);
        p.left -= 1;
        true
    }

    /// The hours still to pass, all at once (behind a trip's loading screen): the hour
    /// numbers they are, in order, from the first still to pass to the last asked; `None`
    /// when nothing passes.
    pub fn take_left(&mut self) -> Option<std::ops::RangeInclusive<u32>> {
        let p = self.passing.as_mut().filter(|p| p.left > 0)?;
        let first = p.asked - p.left + 1;
        p.left = 0;
        Some(first..=p.asked)
    }

    /// The hours the slider shows: those chosen, or those left while they pass.
    pub fn marker(&self) -> u32 {
        self.passing.map_or(self.hours, |p| p.left)
    }

    /// What the menu shows at the game time `now`.
    pub fn view(&self, words: &Words, now: &Calendar) -> View {
        let hours = self.marker();
        View {
            title: if self.sleep { words.sleep_title.clone() } else { words.wait_title.clone() },
            hours: words.hours_line(hours, minute_of_day(now)),
            date: words.date_line(now),
            marker: hours,
            // E and X, with a controller A (Enter) and B (Esc) [#1662].
            accept: format!("{} {}", if self.sleep { &words.sleep } else { &words.wait }, crate::pad::hint(&[KeyCode::KeyE, KeyCode::Enter], WAIT_AT)),
            action: if self.sleep { words.sleep.clone() } else { words.wait.clone() },
            cancel: format!("{} {}", words.cancel, crate::pad::hint(&[KeyCode::KeyX, KeyCode::Escape], WAIT_AT)),
            can_accept: self.passing.is_none(),
        }
    }
}

/// The wait menu, for its hints.
const WAIT_AT: crate::pad::MenuAt = crate::pad::MenuAt { code: Some(crate::script::MENU_SLEEP_WAIT), tab: 0 };

/// The minute of the day of a calendar's hour (GameHour is in hours, with a fraction).
pub fn minute_of_day(c: &Calendar) -> u32 {
    // A hundredth of a minute on: an hour that is a rounded sum (`10.566667 + 22`) must
    // not read a minute early.
    ((c.hour as f64 * 60.0 + 0.01).floor() as i64).rem_euclid(24 * 60) as u32
}

/// "8:34", "10:34", "0:05" [observed: the screenshot's two times; hypothesis: no leading
/// zero on any hour, 24-hour clock].
pub fn clock(minute_of_day: u32) -> String {
    format!("{}:{:02}", minute_of_day / 60 % 24, minute_of_day % 60)
}

/// The day of the week of a game date, 0 Sunday to 6 Saturday: the days since 13 October
/// 2281, the date GameDaysPassed counts from [documented: GECK wiki GetGameDaysPassed],
/// counted on the game's calendar (365 days every year, docs/spec-notes/simulation.md
/// §5b), with that day a Sunday [hypothesis: it makes 19 October 2281 a Saturday, as the
/// owner's screenshot shows it; on the Gregorian calendar that date is a Wednesday. That
/// the weekday changes with the date at midnight is assumed. wait-menu.md §4].
pub fn weekday(c: &Calendar) -> u32 {
    const BEFORE: [i64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let day_number = |year: i64, month: i64, day: i64| year * 365 + BEFORE[month.clamp(0, 11) as usize] + day - 1;
    (day_number(c.year as i64, c.month as i64, c.day as i64) - day_number(2281, 9, 13)).rem_euclid(7) as u32
}

/// The menu's wording. The original takes it from game settings that are named in its
/// documentation but have no record in the game's plugins (wait-menu.md §3), so the
/// defaults here are what the owner's screenshot shows; a plugin that does set one of
/// them is followed.
#[derive(Debug, Clone, PartialEq, bevy::prelude::Resource)]
pub struct Words {
    pub wait_title: String,
    pub sleep_title: String,
    pub wait: String,
    pub sleep: String,
    pub cancel: String,
    pub hour: String,
    pub hours: String,
    pub weekdays: [String; 7],
}

impl Default for Words {
    fn default() -> Self {
        let s = String::from;
        Words {
            // [observed: the screenshot] the title, "Wait", "Cancel", "hours" and
            // "Saturday"; [hypothesis] the sleep title (the wait title with the other
            // verb), "Sleep", "hour" for one and the other days' names.
            wait_title: s("How long would you like to wait?"),
            sleep_title: s("How long would you like to sleep?"),
            wait: s("Wait"),
            sleep: s("Sleep"),
            cancel: s("Cancel"),
            hour: s("hour"),
            hours: s("hours"),
            weekdays: ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"].map(String::from),
        }
    }
}

impl Words {
    /// Take each string from the game setting that holds it (`get` by Editor ID), where
    /// there is one.
    pub fn from_settings(get: impl Fn(&str) -> Option<String>) -> Words {
        let mut w = Words::default();
        for (name, slot) in SETTINGS.iter().zip(w.slots()) {
            if let Some(v) = name.and_then(&get) {
                *slot = v;
            }
        }
        w
    }

    fn slots(&mut self) -> Vec<&mut String> {
        let mut v = vec![&mut self.wait_title, &mut self.sleep_title, &mut self.wait, &mut self.sleep, &mut self.cancel, &mut self.hour, &mut self.hours];
        v.extend(self.weekdays.iter_mut());
        v
    }

    /// "22 hours (8:34)", "1 hour (11:34)": the hours and the time of day they end at.
    pub fn hours_line(&self, hours: u32, now_minute: u32) -> String {
        let unit = if hours == 1 { &self.hour } else { &self.hours };
        format!("{hours} {unit} ({})", clock((now_minute + hours * 60) % (24 * 60)))
    }

    /// "Saturday, 19.10.81, 10:34": weekday, day.month.year, time [observed: the
    /// screenshot; the month is GameMonth + 1 (GameMonth 9 is October, simulation.md
    /// §5b), the year its last two digits (hypothesis)].
    pub fn date_line(&self, c: &Calendar) -> String {
        let date = styled_date(c).unwrap_or_else(|| format!("{}.{}.{:02}", c.day as i64, c.month as i64 + 1, (c.year as i64).rem_euclid(100)));
        format!("{}, {date}, {}", self.weekdays[weekday(c) as usize], clock(minute_of_day(c)))
    }
}

/// The player's iDateFormat tweak [design: docs/design/tweaks.md]: 0 as the game writes each date, 1 day first
/// (19.10.81), 2 month first (10.19.81), 3 year first (2281-10-19). Set by
/// `crate::tweaks::sync`; read by the wait menu and the Pip-Boy's heading.
static DATE_STYLE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// The names of the date styles, in their order.
pub const DATE_STYLES: [&str; 4] = ["As the game", "Day first (19.10.81)", "Month first (10.19.81)", "Year first (2281-10-19)"];

pub fn set_date_style(style: u8) {
    DATE_STYLE.store(style, std::sync::atomic::Ordering::Relaxed);
}

/// The date in the chosen style, or `None` to write it as the game does there.
pub fn styled_date(c: &Calendar) -> Option<String> {
    date_in(c, DATE_STYLE.load(std::sync::atomic::Ordering::Relaxed))
}

/// The date in `style` ([`DATE_STYLES`]); `None` for the game's own.
pub fn date_in(c: &Calendar, style: u8) -> Option<String> {
    let (d, m, y) = (c.day as i64, c.month as i64 + 1, c.year as i64);
    match style {
        1 => Some(format!("{d}.{m}.{:02}", y.rem_euclid(100))),
        2 => Some(format!("{m}.{d}.{:02}", y.rem_euclid(100))),
        3 => Some(format!("{y}-{m:02}-{d:02}")),
        _ => None,
    }
}

/// The game settings of [`Words`]' strings, in [`Words::slots`] order [documented: GECK
/// wiki "Settings" and ShowSleepWaitMenu name them; the interface file names `sCancel`];
/// `None` where no setting is known.
const SETTINGS: [Option<&str>; 14] = [
    Some("sHowManyWait"),
    None,
    Some("sWait"),
    Some("sSleep"),
    Some("sCancel"),
    None,
    None,
    Some("sDaySunday"),
    Some("sDayMonday"),
    Some("sDayTuesday"),
    Some("sDayWednesday"),
    Some("sDayThursday"),
    Some("sDayFriday"),
    Some("sDaySaturday"),
];

/// Why the menu does not open (wait-menu.md §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A script turned waiting off (`EnableFastTravel`'s canWait).
    Forbidden,
    UnderWater,
    /// The player is in the air (jumping or falling).
    Falling,
    /// The player is taking radiation damage. Never given yet: nothing irradiates the
    /// player from the world (`refusal` keeps the check empty).
    Radiation,
    /// An actor in a combat state hunts the player (IsInCombat on the player).
    InCombat,
    HostilesNear,
    /// The player is in an owned cell that is not theirs and not public.
    Trespassing,
    /// Sleeping only: the bed is someone else's.
    OwnedBed,
    /// The player's Wait Only Seated tweak, and the player does not sit [design: docs/design/tweaks.md].
    NotSeated,
}

impl Refusal {
    pub const COUNT: usize = 9;
    pub const ALL: [Refusal; Refusal::COUNT] = [
        Refusal::Forbidden,
        Refusal::UnderWater,
        Refusal::Falling,
        Refusal::Radiation,
        Refusal::InCombat,
        Refusal::HostilesNear,
        Refusal::Trespassing,
        Refusal::OwnedBed,
        Refusal::NotSeated,
    ];

    /// For the log.
    pub fn label(self) -> &'static str {
        match self {
            Refusal::Forbidden => "forbidden by a script",
            Refusal::UnderWater => "under water",
            Refusal::Falling => "in the air",
            Refusal::Radiation => "taking radiation damage",
            Refusal::InCombat => "in combat",
            Refusal::HostilesNear => "enemies near",
            Refusal::Trespassing => "trespassing",
            Refusal::OwnedBed => "someone else's bed",
            Refusal::NotSeated => "not seated (tweak)",
        }
    }
}

/// The lines shown instead of the menu. The original takes each from a game setting
/// [documented: GECK wiki "Settings" lists the names; SetPCHasSleepWaitOverride says the
/// flag's text is `sNoWaitInCell`]. Only the two about enemies have a record in
/// FalloutNV.esm [observed], and those are read from it; the defaults here are
/// NVForever's own wording, used where no plugin sets the setting. Falling, radiation and
/// trespassing have no setting named by any source read (wait-menu.md §9): their lines
/// are NVForever's only [owner request 2026-10-17].
#[derive(Debug, Clone, PartialEq, bevy::prelude::Resource)]
pub struct RefusalWords {
    pub forbidden: String,
    pub under_water: String,
    pub falling: String,
    pub radiation: String,
    pub combat_wait: String,
    pub combat_sleep: String,
    pub trespassing: String,
    pub hostiles_wait: String,
    pub hostiles_sleep: String,
    pub owned_bed: String,
    /// NVForever's own: no setting exists for the tweak's refusal.
    pub not_seated: String,
}

impl Default for RefusalWords {
    fn default() -> Self {
        let s = String::from;
        RefusalWords {
            forbidden: s("You cannot wait here."),
            under_water: s("You cannot wait while swimming."),
            falling: s("You cannot wait while falling."),
            radiation: s("You cannot wait while taking radiation damage."),
            combat_wait: s("You cannot wait while in combat."),
            combat_sleep: s("You cannot sleep while in combat."),
            trespassing: s("You cannot wait while trespassing."),
            hostiles_wait: s("You cannot wait with enemies close by."),
            hostiles_sleep: s("You cannot sleep with enemies close by."),
            owned_bed: s("This bed belongs to someone else."),
            not_seated: s("You can only wait while sitting down."),
        }
    }
}

impl RefusalWords {
    pub fn from_settings(get: impl Fn(&str) -> Option<String>) -> RefusalWords {
        let mut w = RefusalWords::default();
        let slots = [
            ("sNoWaitInCell", &mut w.forbidden),
            ("sNoWaitUnderWater", &mut w.under_water),
            ("sNoWaitInCombat", &mut w.combat_wait),
            ("sNoSleepCombat", &mut w.combat_sleep),
            ("sNoWaitHostilActorsNear", &mut w.hostiles_wait),
            ("sNoSleepHostileActorsNear", &mut w.hostiles_sleep),
            ("sNoSleepInOwnedBed", &mut w.owned_bed),
        ];
        for (name, slot) in slots {
            if let Some(v) = get(name) {
                *slot = v;
            }
        }
        w
    }

    pub fn line(&self, why: Refusal, sleep: bool) -> &str {
        match (why, sleep) {
            (Refusal::Forbidden, _) => &self.forbidden,
            (Refusal::UnderWater, _) => &self.under_water,
            (Refusal::Falling, _) => &self.falling,
            (Refusal::Radiation, _) => &self.radiation,
            (Refusal::InCombat, false) => &self.combat_wait,
            (Refusal::InCombat, true) => &self.combat_sleep,
            (Refusal::Trespassing, _) => &self.trespassing,
            (Refusal::HostilesNear, false) => &self.hostiles_wait,
            (Refusal::HostilesNear, true) => &self.hostiles_sleep,
            (Refusal::OwnedBed, _) => &self.owned_bed,
            (Refusal::NotSeated, _) => &self.not_seated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cal(day: f32, month: f32, hour: f32) -> Calendar {
        Calendar { year: 2281.0, month, day, hour, days_passed: 6.44 }
    }

    #[test]
    fn the_lines_read_as_the_screenshot() {
        let w = Words::default();
        // 10:34 on 19 October 2281, 22 hours chosen.
        let now = cal(19.0, 9.0, 10.0 + 34.0 / 60.0);
        let mut m = Model::open(false);
        m.apply(Command::Hours(22));
        let v = m.view(&w, &now);
        assert_eq!(v.hours, "22 hours (8:34)");
        assert_eq!(v.date, "Saturday, 19.10.81, 10:34");
        assert_eq!((v.marker, v.can_accept), (22, true));
        assert_eq!((v.accept.as_str(), v.cancel.as_str()), ("Wait [E]", "Cancel [X]"));
        m.apply(Command::Hours(1));
        assert_eq!(m.view(&w, &now).hours, "1 hour (11:34)");
        // A whole day ends at the same time.
        m.apply(Command::Hours(24));
        assert_eq!(m.view(&w, &now).hours, "24 hours (10:34)");
    }

    #[test]
    fn on_a_bed_it_is_a_sleep_menu() {
        let w = Words::default();
        let v = Model::open(true).view(&w, &cal(19.0, 9.0, 22.0));
        assert_eq!((v.title, v.accept), (w.sleep_title.clone(), "Sleep [E]".to_string()));
        assert_eq!(Model::open(false).view(&w, &cal(19.0, 9.0, 22.0)).title, w.wait_title);
    }

    #[test]
    fn the_slider_stays_within_one_and_twenty_four() {
        let mut m = Model::open(false);
        assert_eq!(m.hours, 1);
        m.apply(Command::Step(-1));
        assert_eq!(m.hours, 1);
        m.apply(Command::Step(5));
        assert_eq!(m.hours, 6);
        m.apply(Command::Step(100));
        assert_eq!(m.hours, 24);
        m.apply(Command::Hours(0));
        assert_eq!(m.hours, 1);
        m.apply(Command::Hours(99));
        assert_eq!(m.hours, 24);
        // The pointer on the slider: its ends are the end marks, the hours between.
        assert_eq!((hours_at(-0.2), hours_at(0.0), hours_at(1.0 / 25.0), hours_at(0.5), hours_at(22.0 / 25.0), hours_at(1.0), hours_at(3.0)), (1, 1, 1, 13, 22, 24, 24));
        assert_eq!(hours_at(0.07), 2);
    }

    #[test]
    fn accepted_hours_pass_one_at_a_time_and_cancel_stops_them() {
        let w = Words::default();
        let mut m = Model::open(false);
        m.apply(Command::Hours(3));
        assert!(!m.tick(10.0, 0.5), "nothing passes before Accept");
        assert!(!m.apply(Command::Accept));
        // The slider is held now.
        m.apply(Command::Hours(9));
        m.apply(Command::Step(1));
        m.apply(Command::Accept);
        assert_eq!(m.passing.map(|p| (p.asked, p.left)), Some((3, 3)));
        assert!(!m.tick(0.3, 0.5));
        assert!(m.tick(0.3, 0.5));
        // The count goes down; the end time stays (the clock has moved on an hour).
        let v = m.view(&w, &cal(19.0, 9.0, 11.0));
        assert_eq!((v.hours.as_str(), v.marker, v.can_accept), ("2 hours (13:00)", 2, false));
        // A long frame passes one hour, not several.
        assert!(m.tick(5.0, 0.5));
        assert!(m.tick(0.5, 0.5));
        assert_eq!(m.passing.map(|p| p.left), Some(0));
        assert!(!m.tick(5.0, 0.5));
        // Cancel closes the menu, before and while hours pass.
        assert!(m.apply(Command::Cancel));
        assert!(Model::open(true).apply(Command::Cancel));
    }

    #[test]
    fn the_weekday_and_date_change_at_midnight() {
        let w = Words::default();
        assert_eq!(weekday(&cal(19.0, 9.0, 10.0)), 6);
        assert_eq!(weekday(&cal(18.0, 9.0, 23.9)), 5);
        // 22 hours from 10:34 on Saturday the 19th: 8:34 on Sunday the 20th.
        let later = cal(19.0, 9.0, 10.0 + 34.0 / 60.0).advance(22.0);
        assert_eq!(w.date_line(&later), "Sunday, 20.10.81, 8:34");
        // Hour by hour, as the menu passes them, ends on the same line.
        let mut c = cal(19.0, 9.0, 10.0 + 34.0 / 60.0);
        for _ in 0..22 {
            c = c.advance(1.0);
        }
        assert_eq!(w.date_line(&c), "Sunday, 20.10.81, 8:34");
        // A whole day: the same time, the next weekday and date.
        let mut d = cal(31.0, 9.0, 23.5);
        for _ in 0..24 {
            d = d.advance(1.0);
        }
        assert_eq!(w.date_line(&d), "Friday, 1.11.81, 23:30");
        assert_eq!(w.date_line(&cal(31.0, 9.0, 23.5)), "Thursday, 31.10.81, 23:30");
        // The iDateFormat tweak's styles.
        let c = cal(19.0, 9.0, 10.5);
        assert_eq!([0, 1, 2, 3].map(|s| date_in(&c, s)), [None, Some("19.10.81".into()), Some("10.19.81".into()), Some("2281-10-19".into())]);
        // Over the year's end: no leap days, the week goes on.
        let eve = Calendar { year: 2281.0, month: 11.0, day: 31.0, hour: 23.0, days_passed: 79.0 };
        let (a, b) = (weekday(&eve), weekday(&eve.advance(2.0)));
        assert_eq!((a + 1) % 7, b);
        assert_eq!(w.date_line(&eve.advance(2.0)), format!("{}, 1.1.82, 1:00", w.weekdays[b as usize]));
        assert_eq!(clock(5), "0:05");
    }

    #[test]
    fn a_script_can_make_the_player_sleep() {
        let mut m = Model::open(false);
        m.sleep_for(3);
        assert!(m.sleep);
        assert_eq!(m.passing.map(|p| (p.asked, p.left)), Some((3, 3)));
        assert!(m.tick(0.5, 0.5));
        // A new count while it passes.
        m.sleep_for(99);
        assert_eq!(m.passing.map(|p| (p.asked, p.left)), Some((MAX_HOURS, MAX_HOURS)));
        // No hours: the Sleep menu, nothing passing.
        let mut m = Model::open(false);
        m.sleep_for(0);
        assert_eq!((m.sleep, m.hours, m.passing), (true, 1, None));
    }

    #[test]
    fn the_hours_left_can_pass_at_once() {
        let mut m = Model::open(false);
        m.sleep_for(3);
        assert!(m.tick(0.5, 0.5));
        // Hour 1 has passed: 2 and 3 are left, and then none.
        assert_eq!(m.take_left(), Some(2..=3));
        assert_eq!(m.passing.map(|p| (p.asked, p.left)), Some((3, 0)));
        assert_eq!(m.take_left(), None);
        assert!(!m.tick(0.5, 0.5));
        // Nothing passing: nothing to take.
        assert_eq!(Model::open(true).take_left(), None);
    }

    #[test]
    fn a_refusal_has_a_line_for_waiting_and_for_sleeping() {
        let own = RefusalWords::default();
        assert_ne!(own.line(Refusal::HostilesNear, false), own.line(Refusal::HostilesNear, true));
        // The flag's text is the same for both.
        assert_eq!(own.line(Refusal::Forbidden, false), own.line(Refusal::Forbidden, true));
        // A plugin's settings are followed.
        let w = RefusalWords::from_settings(|name| (name == "sNoSleepHostileActorsNear").then(|| "Not now.".to_string()));
        assert_eq!(w.line(Refusal::HostilesNear, true), "Not now.");
        assert_eq!(w.line(Refusal::HostilesNear, false), own.hostiles_wait);
        // Combat has a setting each for waiting and sleeping.
        let w = RefusalWords::from_settings(|name| (name == "sNoSleepCombat").then(|| "Fight first.".to_string()));
        assert_eq!(w.line(Refusal::InCombat, true), "Fight first.");
        assert_eq!(w.line(Refusal::InCombat, false), own.combat_wait);
        assert_eq!(Refusal::ALL.len(), Refusal::COUNT);
        for (i, r) in Refusal::ALL.iter().enumerate() {
            assert_eq!(*r as usize, i);
        }
    }

    #[test]
    fn the_game_settings_give_the_words() {
        assert_eq!(Words::from_settings(|_| None), Words::default());
        // A plugin that sets some of them.
        let w = Words::from_settings(|name| match name {
            "sWait" => Some("Warten".into()),
            "sDaySaturday" => Some("Samstag".into()),
            _ => None,
        });
        let v = Model::open(false).view(&w, &cal(19.0, 9.0, 10.0));
        assert_eq!((v.accept.as_str(), v.date.as_str()), ("Warten [E]", "Samstag, 19.10.81, 10:00"));
        assert_eq!(v.title, Words::default().wait_title);
        // The week starts on the day GameDaysPassed counts from.
        assert_eq!(weekday(&cal(13.0, 9.0, 0.0)), 0);
    }
}
