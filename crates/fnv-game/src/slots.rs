//! Slot machines at the casinos (docs/spec-notes/slots.md): the reels laid out from the
//! casino record's reel stops, a spin, the payline's payout as the machine's payout screen
//! lists it, and the bet's limits. The casino record, the player's earnings and the
//! winnings levels are `crate::casino`'s; the engine draws the menu (nvf-play `slots`).

use crate::game::Game;
use fnv_sim::FormId;
use rand_chacha::ChaCha8Rng;
use rand_core::RngCore;

/// The help message shown the first time the menu opens (slots.md §5) [observed:
/// FalloutNV.esm MESG `HelpSlots`, flags Message Box and Auto Display].
pub const HELP_MESSAGE: FormId = FormId(0x0017_2096);
/// The menu's code to scripts (slots.md §1) [documented: GECK wiki MenuMode /
/// `fnv_ui::value::MENU_CLASSES`, `SlotMachineMenu = 1080`].
pub const MENU_CODE: u16 = 1080;
/// Symbols on a reel: 1 to 6 and W [documented: GECK wiki GetCasinoReelStops].
pub const SYMBOLS: usize = 7;
/// The winning symbol W, the seventh (index 6) [documented: same page].
pub const W: u8 = 6;
/// Stops on a reel [documented: "must equal 14", same page].
pub const STOPS: usize = 14;
/// Reels.
pub const REELS: usize = 3;
/// What three of each symbol 1 to 6 pay, times the bet [observed: the payout screen of
/// `NV_SlotMachine-Minigame.NIF`, slots.md §3].
pub const THREE_PAY: [u32; 6] = [10, 20, 30, 40, 80, 160];
/// What one, two and three W (cherries) on the line from the left reel on pay, times the
/// bet [observed: the payout screen's last three rows; hypothesis: counted from the left].
pub const W_PAY: [u32; 3] = [2, 5, 10];
/// The symbols' names, by what their pictures show (slots.md §2) [observed: the CSNO ICON
/// order and the textures].
pub const NAMES: [&str; SYMBOLS] = ["Bell", "BAR", "Seven", "Lemon", "Grapes", "Orange", "Cherry"];
const PLURALS: [&str; SYMBOLS] = ["bells", "BARs", "sevens", "lemons", "grapes", "oranges", "cherries"];

/// A symbol's name in the plural ("oranges").
pub fn plural(symbol: u8) -> &'static str {
    PLURALS.get(symbol as usize).copied().unwrap_or("?")
}

/// The reel's stops, the symbol (0 to 6) at each, from the record's counts: dealt out in
/// turn, each symbol while it has stops left (slots.md §3) [hypothesis: the order is not
/// in the data]. Counts that do not add up to 14 are kept as they are (a reel of their
/// sum); none at all gives two of each [documented: the shipped records' 2 2 2 2 2 2 2].
pub fn reel(stops: &[u32; SYMBOLS]) -> Vec<u8> {
    let mut left = *stops;
    if left.iter().all(|&n| n == 0) {
        left = [2; SYMBOLS];
    }
    let mut out = Vec::new();
    while left.iter().any(|&n| n > 0) {
        for (s, n) in left.iter_mut().enumerate() {
            if *n > 0 {
                *n -= 1;
                out.push(s as u8);
            }
        }
    }
    out
}

/// A spin: the stop each reel comes to rest on, each stop as likely [hypothesis].
pub fn spin(rng: &mut ChaCha8Rng, reel_len: usize) -> [usize; REELS] {
    let n = reel_len.max(1) as u32;
    [(rng.next_u32() % n) as usize, (rng.next_u32() % n) as usize, (rng.next_u32() % n) as usize]
}

/// The stop on `reel` that shows `symbol`, the first one (for the test aid).
pub fn stop_of(reel: &[u8], symbol: u8) -> Option<usize> {
    reel.iter().position(|&s| s == symbol)
}

/// What a line pays, times the bet: three of a symbol 1 to 6 its number, else the W from
/// the left reel on (slots.md §3); 0 for nothing.
pub fn multiple(line: [u8; REELS]) -> u32 {
    if line[0] == line[1] && line[1] == line[2] && line[0] != W {
        return THREE_PAY.get(line[0] as usize).copied().unwrap_or(0);
    }
    let ws = line.iter().take_while(|&&s| s == W).count();
    if ws == 0 {
        0
    } else {
        W_PAY[ws - 1]
    }
}

/// The line's win in words ("Three oranges", "One cherry"), none for a loss.
pub fn win_name(line: [u8; REELS]) -> Option<String> {
    win_words(line).map(str::to_string)
}

/// Three of a kind, each as one whole text, so a language file can have a line for it
/// (#3146: a text put together from "Three" and a plural cannot be translated).
const THREES: [&str; SYMBOLS] = ["Three bells", "Three BARs", "Three sevens", "Three lemons", "Three grapes", "Three oranges", "Three cherries"];

/// [`win_name`] as one of a fixed set of texts, each a key of the language files.
pub fn win_words(line: [u8; REELS]) -> Option<&'static str> {
    if multiple(line) == 0 {
        return None;
    }
    if line[0] == line[1] && line[1] == line[2] {
        return Some(THREES.get(line[0] as usize).copied().unwrap_or("?"));
    }
    let ws = line.iter().take_while(|&&s| s == W).count();
    Some(if ws == 1 { "One cherry" } else { "Two cherries" })
}

/// The payout list's rows: the symbols each row shows and its multiple (slots.md §3).
pub fn payout_rows() -> Vec<(Vec<u8>, u32)> {
    let mut rows: Vec<(Vec<u8>, u32)> = (0..6u8).map(|s| (vec![s; 3], THREE_PAY[s as usize])).collect();
    for (n, pay) in W_PAY.iter().enumerate() {
        rows.push((vec![W; n + 1], *pay));
    }
    rows
}

/// The machine's limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub min_bet: u32,
    pub max_bet: u32,
}

impl Limits {
    pub fn new(min_bet: u32, max_bet: u32) -> Limits {
        let min_bet = min_bet.max(1);
        Limits { min_bet, max_bet: max_bet.max(min_bet) }
    }

    /// The largest bet with `chips` carried (the minimum when fewer).
    pub fn top(&self, chips: u32) -> u32 {
        self.max_bet.min(chips).max(self.min_bet)
    }

    pub fn clamp(&self, bet: u32, chips: u32) -> u32 {
        bet.clamp(self.min_bet, self.top(chips))
    }

    /// One step up or down: by 1 under 10, 5 under 50, 25 from there (as blackjack's,
    /// blackjack.md §5) [design].
    pub fn step(&self, bet: u32, up: bool, chips: u32) -> u32 {
        let step = |b: u32| if b < 10 { 1 } else if b < 50 { 5 } else { 25 };
        let next = if up { bet + step(bet) } else { bet.saturating_sub(step(bet.saturating_sub(1))) };
        self.clamp(next, chips)
    }
}

/// The help message to show as the menu opens, the first time only (as roulette's).
pub fn first_help(g: &mut Game) -> Option<(String, String)> {
    // The player's tweak: no first-time help [design: docs/design/tweaks.md].
    if g.player.slots_help_shown || g.tweaks.no_tutorials {
        return None;
    }
    let m = g.data.messages.get(&HELP_MESSAGE)?;
    let text = m.text.clone()?;
    g.player.slots_help_shown = true;
    Some((m.title.clone().unwrap_or_default(), text))
}

/// A win's celebration in seconds for a line paying `multiple` times the bet: 0 on a
/// loss, 0.8 s at 2 x growing with the prize to 3 s from about 90 x [design: length scales with the prize, up to 3 seconds, small wins get a short one;
/// the curve].
pub fn celebration(multiple: u32) -> f32 {
    if multiple == 0 {
        return 0.0;
    }
    (0.4 + 0.4 * (multiple as f32).log2()).clamp(0.8, 3.0)
}

/// A request from ShowSlotMachineMenuParams (slots.md §1); the engine opens the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotsRequest {
    pub casino: FormId,
    pub min_bet: u32,
    pub max_bet: u32,
    /// 0: the casino's own.
    pub max_winnings: u32,
    /// The reference whose script asked (the slot machine activated), if any: the engine
    /// seats the player at it.
    pub machine: Option<FormId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::SeedableRng;

    #[test]
    fn celebrations_grow_with_the_prize_to_three_seconds() {
        assert_eq!(celebration(0), 0.0);
        assert!((celebration(2) - 0.8).abs() < 1e-5);
        assert_eq!(celebration(160), 3.0);
        let mut last = 0.0;
        for m in [2, 5, 10, 20, 40, 80] {
            let s = celebration(m);
            assert!(s > last && s <= 3.0, "{m}: {s}");
            last = s;
        }
    }

    #[test]
    fn the_record_stops_make_the_reel() {
        let r = reel(&[2; 7]);
        assert_eq!(r.len(), STOPS);
        assert_eq!(r, vec![0, 1, 2, 3, 4, 5, 6, 0, 1, 2, 3, 4, 5, 6]);
        let skew = reel(&[4, 1, 1, 1, 1, 3, 3]);
        assert_eq!(skew.len(), 14);
        assert_eq!(skew.iter().filter(|&&s| s == 0).count(), 4);
        assert_eq!(skew.iter().filter(|&&s| s == W).count(), 3);
        assert_eq!(reel(&[0; 7]).len(), 14);
        assert_eq!(stop_of(&r, 5), Some(5));
    }

    #[test]
    fn the_payout_screen_pays() {
        assert_eq!(multiple([0, 0, 0]), 10);
        assert_eq!(multiple([1, 1, 1]), 20);
        assert_eq!(multiple([2, 2, 2]), 30);
        assert_eq!(multiple([3, 3, 3]), 40);
        assert_eq!(multiple([4, 4, 4]), 80);
        assert_eq!(multiple([5, 5, 5]), 160);
        assert_eq!(multiple([W, 0, 0]), 2);
        assert_eq!(multiple([W, W, 3]), 5);
        assert_eq!(multiple([W, W, W]), 10);
        // Cherries count from the left reel on only.
        assert_eq!(multiple([0, W, W]), 0);
        assert_eq!(multiple([W, 0, W]), 2);
        assert_eq!(multiple([0, 0, 1]), 0);
        assert_eq!(win_name([5, 5, 5]).as_deref(), Some("Three oranges"));
        assert_eq!(win_name([W, 2, 2]).as_deref(), Some("One cherry"));
        assert_eq!(win_name([W, W, W]).as_deref(), Some("Three cherries"));
        assert_eq!(win_name([1, 2, 3]), None);
        // Each three of a kind is the plural's own text (#3146).
        for s in 0..SYMBOLS as u8 {
            assert_eq!(win_words([s; 3]).map(str::to_string), Some(format!("Three {}", plural(s))));
        }
        assert_eq!(payout_rows().len(), 9);
    }

    #[test]
    fn bets_step_within_the_limits_and_the_chips() {
        let l = Limits::new(1, 75);
        assert_eq!(l.step(1, true, 500), 2);
        assert_eq!(l.step(50, true, 500), 75);
        assert_eq!(l.step(75, true, 500), 75);
        assert_eq!(l.step(10, false, 500), 9);
        assert_eq!(l.step(45, true, 30), 30);
        assert_eq!(l.top(500), 75);
        assert_eq!(l.top(20), 20);
        assert_eq!(l.top(0), 1);
        assert_eq!(Limits::new(0, 0), Limits { min_bet: 1, max_bet: 1 });
    }

    #[test]
    fn spins_reach_every_stop() {
        let mut rng = ChaCha8Rng::seed_from_u64(5);
        let mut seen = [0u32; STOPS];
        for _ in 0..1400 {
            for s in spin(&mut rng, STOPS) {
                seen[s] += 1;
            }
        }
        assert!(seen.iter().all(|&n| n > 200));
    }
}
