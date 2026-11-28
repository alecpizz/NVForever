//! Roulette at the casinos' tables (docs/spec-notes/roulette.md): the wheel's 38 pockets,
//! the places a bet can go (the table's model's bet points), what each pays, the bets on
//! the table, the spin and the paying out. The casino record, the player's earnings and
//! the winnings levels are blackjack's (`crate::blackjack`); the engine draws the table and
//! the wheel (nvf-play `roulette`, nvforever `roulette_view.rs`).

use crate::game::Game;
use fnv_sim::FormId;
use rand_chacha::ChaCha8Rng;
use rand_core::RngCore;

/// The help message shown the first time the menu opens (roulette.md §5) [observed:
/// FalloutNV.esm MESG `HelpRoulette`, flags Message Box and Auto Display].
pub const HELP_MESSAGE: FormId = FormId(0x0017_2095);
/// The menu's code to scripts (roulette.md §1) [documented: GECK wiki MenuMode /
/// `fnv_ui::value::MENU_CLASSES`, `RouletteMenu = 1082`].
pub const MENU_CODE: u16 = 1082;
/// The double zero's pocket number (0 to 36 are their own).
pub const DOUBLE_ZERO: u8 = 37;
/// Pockets on the wheel [documented: the help message, "38 colored and numbered pockets"].
pub const POCKETS: usize = 38;
/// The wheel's pockets in the order its picture puts them, from the picture's left edge
/// (`textures\terminals\nv_roulette\nv_roulette03.dds`, 38 equal cells along its width)
/// [observed: the picture]; the American wheel's order run backwards.
pub const WHEEL_STRIP: [u8; POCKETS] = [
    35, 23, 4, 16, 33, 21, 6, 18, 31, 19, 8, 12, 29, 25, 10, 27, DOUBLE_ZERO, 1, 13, 36, 24, 3, 15, 34, 22, 5, 17, 32, 20, 7, 11, 30, 26, 9, 28, 0, 2, 14,
];
/// The red numbers [observed: the wheel's picture and the felt (the owner's screenshot)].
pub const REDS: [u8; 18] = [1, 3, 5, 7, 9, 12, 14, 16, 18, 19, 21, 23, 25, 27, 30, 32, 34, 36];
/// The bet points of `meshes\terminals\nv_roulette\nv_roulette-points.nif`: a grid of six
/// rows (`A`–`F`) by 24 columns, two units apart across and 2.67 along, and fifteen
/// outside places (`S01`–`S15`) [observed: the file's nodes].
pub const GRID_ROWS: [char; 6] = ['A', 'B', 'C', 'D', 'E', 'F'];
pub const GRID_COLUMNS: u8 = 24;
pub const OUTSIDE_SPOTS: u8 = 15;

/// A pocket's name: its number, "00" for the double zero.
pub fn pocket_name(p: u8) -> String {
    if p == DOUBLE_ZERO {
        "00".into()
    } else {
        p.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colour {
    Red,
    Black,
    Green,
}

impl Colour {
    pub fn name(self) -> &'static str {
        match self {
            Colour::Red => "Red",
            Colour::Black => "Black",
            Colour::Green => "Green",
        }
    }
}

pub fn colour(p: u8) -> Colour {
    if p == 0 || p == DOUBLE_ZERO {
        Colour::Green
    } else if REDS.contains(&p) {
        Colour::Red
    } else {
        Colour::Black
    }
}

/// The kinds of bet [documented: the help message's list].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    StraightUp,
    Split,
    Street,
    SixLine,
    Corner,
    Basket,
    TopLine,
    Low,
    High,
    Red,
    Black,
    Even,
    Odd,
    Dozen(u8),
    Column(u8),
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::StraightUp => "Straight-Up",
            Kind::Split => "Split",
            Kind::Street => "Street",
            Kind::SixLine => "Six Line",
            Kind::Corner => "Corner",
            Kind::Basket => "Basket",
            Kind::TopLine => "Top Line",
            Kind::Low => "1-18",
            Kind::High => "19-36",
            Kind::Red => "Red",
            Kind::Black => "Black",
            Kind::Even => "Even",
            Kind::Odd => "Odd",
            Kind::Dozen(1) => "1st Dozen",
            Kind::Dozen(2) => "2nd Dozen",
            Kind::Dozen(_) => "3rd Dozen",
            Kind::Column(1) => "1st Column",
            Kind::Column(2) => "2nd Column",
            Kind::Column(_) => "3rd Column",
        }
    }
}

/// A place a bet can go: the bet point's name, the kind and the numbers it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spot {
    pub point: String,
    pub kind: Kind,
    pub numbers: Vec<u8>,
}

impl Spot {
    /// The chips a winning bet is paid for each chip staked, the stake coming back too:
    /// 36 over the numbers covered, less one, as a real double-zero wheel pays (35 to 1
    /// straight up … 1 to 1 on red); the top line's five pay 6 to 1 [hypothesis: the
    /// original's payouts are not in the plugins; these are the American table's].
    pub fn payout(&self) -> u32 {
        match self.kind {
            Kind::TopLine => 6,
            _ => 36 / self.numbers.len().max(1) as u32 - 1,
        }
    }

    pub fn covers(&self, pocket: u8) -> bool {
        self.numbers.contains(&pocket)
    }

    /// The bet's name for the menu's "Bet:" line: the kind, then the numbers of an inside
    /// bet ("Split 17-20") [design].
    pub fn label(&self) -> String {
        self.label_with(&|kind| kind)
    }

    /// [`Spot::label`] with the kind's name put through `word` (the screen's language,
    /// #3146); the numbers are the same in every language.
    pub fn label_with(&self, word: &dyn Fn(&'static str) -> &'static str) -> String {
        match self.kind {
            Kind::StraightUp | Kind::Split | Kind::Street | Kind::SixLine | Kind::Corner | Kind::Basket => {
                let mut n = self.numbers.clone();
                // 0 and 00 first, then the numbers in order.
                n.sort_by_key(|&p| match p {
                    0 => 0,
                    DOUBLE_ZERO => 1,
                    _ => p as u16 + 1,
                });
                let names: Vec<String> = if matches!(self.kind, Kind::Street | Kind::SixLine) {
                    vec![pocket_name(n[0]), pocket_name(n[n.len() - 1])]
                } else {
                    n.iter().map(|&p| pocket_name(p)).collect()
                };
                format!("{} {}", word(self.kind.name()), names.join("-"))
            }
            _ => word(self.kind.name()).to_string(),
        }
    }
}

/// The numbers of a column of the layout (1 to 12, from the zeros) in the near, middle or
/// far row (1 to 3): the near row holds 1, 4, 7 …, the far 3, 6, 9 … [observed: the felt
/// on the owner's screenshot; the points' rows run from the outside bets' side].
fn number(col: u8, row: u8) -> u8 {
    3 * (col - 1) + row
}

/// A bet point's spot, by its node name (roulette.md §3) [observed: the points' places
/// over the felt; hypothesis: which bet each place is, read from where it lies]:
///
/// - The grid's even columns (2, 4 … 24) are the twelve columns of numbers, the odd ones
///   the lines between (column 1 the line along the zeros). Rows B, D and F are the three
///   rows of numbers (near to far), C and E the lines between them, A the near edge.
/// - On a number: straight up; on a line between two: a split; where four meet: a
///   corner; on the near edge: a street (on a column) or a six line (on a line).
/// - Along the zeros: 0-1 and 00-3 splits, the baskets 0-1-2, 0-00-2 and 00-2-3, and the
///   top line at the near corner.
/// - `S01`–`S03` beyond the zeros: 00, the 0-00 split and 0; `S04`, `S07`, `S10`: the
///   dozens; `S05`, `S06`, `S08`, `S09`, `S11`, `S12`: 1-18, even, red, black, odd, 19-36;
///   `S13`–`S15` at the far end: the columns' 2 to 1 (near, middle, far row).
pub fn spot(point: &str) -> Option<Spot> {
    let p = point.trim().to_ascii_uppercase();
    let (head, num) = p.split_at(1);
    let n: u8 = num.parse().ok()?;
    let mk = |kind: Kind, mut numbers: Vec<u8>| {
        numbers.sort_unstable();
        Some(Spot { point: p.clone(), kind, numbers })
    };
    let z = DOUBLE_ZERO;
    if head == "S" {
        return match n {
            1 => mk(Kind::StraightUp, vec![z]),
            2 => mk(Kind::Split, vec![0, z]),
            3 => mk(Kind::StraightUp, vec![0]),
            4 | 7 | 10 => {
                let d = (n - 1) / 3;
                mk(Kind::Dozen(d), (12 * (d - 1) + 1..=12 * d).collect())
            }
            5 => mk(Kind::Low, (1..=18).collect()),
            6 => mk(Kind::Even, (1..=36).filter(|x: &u8| x.is_multiple_of(2)).collect()),
            8 => mk(Kind::Red, REDS.to_vec()),
            9 => mk(Kind::Black, (1..=36).filter(|x| !REDS.contains(x)).collect()),
            11 => mk(Kind::Odd, (1..=36).filter(|x: &u8| !x.is_multiple_of(2)).collect()),
            12 => mk(Kind::High, (19..=36).collect()),
            13..=15 => {
                let row = n - 12;
                mk(Kind::Column(row), (1..=12).map(|c| number(c, row)).collect())
            }
            _ => None,
        };
    }
    let row = GRID_ROWS.iter().position(|c| head.starts_with(*c))? as u8;
    if !(1..=GRID_COLUMNS).contains(&n) {
        return None;
    }
    // Rows: 0 the near edge, 1/3/5 the number rows, 2/4 the lines between them.
    if n.is_multiple_of(2) {
        let col = n / 2;
        return match row {
            0 => mk(Kind::Street, (1..=3).map(|r| number(col, r)).collect()),
            1 | 3 | 5 => mk(Kind::StraightUp, vec![number(col, row / 2 + 1)]),
            _ => mk(Kind::Split, vec![number(col, row / 2), number(col, row / 2 + 1)]),
        };
    }
    if n == 1 {
        // Along the zeros: 0 beside the near row and half the middle one, 00 beside the
        // other half and the far row.
        return match row {
            0 => mk(Kind::TopLine, vec![0, z, 1, 2, 3]),
            1 => mk(Kind::Split, vec![0, 1]),
            2 => mk(Kind::Basket, vec![0, 1, 2]),
            3 => mk(Kind::Basket, vec![0, z, 2]),
            4 => mk(Kind::Basket, vec![z, 2, 3]),
            _ => mk(Kind::Split, vec![z, 3]),
        };
    }
    // A line between the columns `left` and `left + 1`.
    let left = (n - 1) / 2;
    match row {
        0 => mk(Kind::SixLine, (1..=3).flat_map(|r| [number(left, r), number(left + 1, r)]).collect()),
        1 | 3 | 5 => {
            let r = row / 2 + 1;
            mk(Kind::Split, vec![number(left, r), number(left + 1, r)])
        }
        _ => {
            let r = row / 2;
            mk(Kind::Corner, vec![number(left, r), number(left + 1, r), number(left, r + 1), number(left + 1, r + 1)])
        }
    }
}

/// Every bet point's name: the grid row by row, then the outside places.
pub fn points() -> Vec<String> {
    let mut out: Vec<String> = GRID_ROWS.iter().flat_map(|r| (1..=GRID_COLUMNS).map(move |c| format!("{r}{c:02}"))).collect();
    out.extend((1..=OUTSIDE_SPOTS).map(|n| format!("S{n:02}")));
    out
}

/// Where a bet point lies on the felt, in the points model's space (game units): the grid
/// from (−24, −2.67) two across and 2.67 along per step, the outside places where the file
/// puts them [observed: `nv_roulette-points.nif`'s nodes]. The engine reads the file
/// itself; this is for choosing a neighbour with the keys.
pub fn place(point: &str) -> Option<(f32, f32)> {
    let p = point.trim().to_ascii_uppercase();
    let (head, num) = p.split_at(1);
    let n: u8 = num.parse().ok()?;
    if head == "S" {
        const OUTSIDE: [(f32, f32); 15] = [
            (-26.0, 9.33),
            (-26.0, 5.33),
            (-26.0, 1.33),
            (-16.0, -5.33),
            (-20.0, -10.67),
            (-12.0, -10.67),
            (0.0, -5.33),
            (-4.0, -10.67),
            (4.0, -10.67),
            (16.0, -5.33),
            (12.0, -10.67),
            (20.0, -10.67),
            (26.0, 0.0),
            (26.0, 5.33),
            (26.0, 10.67),
        ];
        return OUTSIDE.get((n as usize).checked_sub(1)?).copied();
    }
    let row = GRID_ROWS.iter().position(|c| head.starts_with(*c))?;
    (1..=GRID_COLUMNS).contains(&n).then(|| (-24.0 + 2.0 * (n - 1) as f32, -2.67 + 2.67 * row as f32))
}

/// The bet point nearest `from` in the direction (`dx`, `dy`) (one of them ±1, along the
/// felt: +x towards 36, +y away from the player), within a cone of 60° either side
/// [design]; none past the edge.
pub fn neighbour(from: &str, dx: f32, dy: f32) -> Option<String> {
    let (x0, y0) = place(from)?;
    let mut best: Option<(f32, String)> = None;
    for p in points() {
        let Some((x, y)) = place(&p) else { continue };
        let (vx, vy) = (x - x0, y - y0);
        let along = vx * dx + vy * dy;
        if along <= 0.01 {
            continue;
        }
        let across = (vx * dy - vy * dx).abs();
        if across > along * 1.75 {
            continue;
        }
        // Straight ahead first, then nearest.
        let cost = along + 2.0 * across;
        if best.as_ref().is_none_or(|(c, _)| cost < *c) {
            best = Some((cost, p));
        }
    }
    best.map(|(_, p)| p)
}

/// The help message to show as the menu opens, the first time only (as lockpicking's).
pub fn first_help(g: &mut Game) -> Option<(String, String)> {
    // The player's tweak: no first-time help [design: docs/design/tweaks.md].
    if g.player.roulette_help_shown || g.tweaks.no_tutorials {
        return None;
    }
    let m = g.data.messages.get(&HELP_MESSAGE)?;
    let text = m.text.clone()?;
    g.player.roulette_help_shown = true;
    Some((m.title.clone().unwrap_or_default(), text))
}

/// A spun pocket: each pocket as likely [hypothesis].
pub fn spin(rng: &mut ChaCha8Rng) -> u8 {
    (rng.next_u32() % POCKETS as u32) as u8
}

/// Why a bet was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No such bet point.
    NoSpot,
    /// More than the player's chips on the table.
    Chips,
    /// Over the table's maximum on one spot.
    Limit,
    /// No bet there to take back.
    NoBet,
    /// No bets on the table to spin for.
    NoBets,
}

/// One bet's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paid {
    pub point: String,
    pub stake: u32,
    /// Chips back: the stake and its winnings, or 0.
    pub back: u32,
}

/// The table's limits and the bets on it.
#[derive(Debug, Clone, Default)]
pub struct Table {
    pub min_bet: u32,
    pub max_bet: u32,
    /// The bets placed, in the order they were first placed (a spot once).
    pub bets: Vec<(Spot, u32)>,
    pub spins: u32,
}

impl Table {
    pub fn new(min_bet: u32, max_bet: u32) -> Table {
        let min_bet = min_bet.max(1);
        Table { min_bet, max_bet: max_bet.max(min_bet), bets: Vec::new(), spins: 0 }
    }

    /// The chips on the table.
    pub fn total(&self) -> u32 {
        self.bets.iter().map(|(_, a)| a).sum()
    }

    pub fn on(&self, point: &str) -> u32 {
        self.bets.iter().find(|(s, _)| s.point.eq_ignore_ascii_case(point)).map_or(0, |(_, a)| *a)
    }

    /// The bet the next chips would make: between the minimum and the maximum.
    pub fn clamp_bet(&self, bet: u32) -> u32 {
        bet.clamp(self.min_bet, self.max_bet)
    }

    /// The bet one step up or down: by 1 under 10, 5 under 50, 25 from there (as
    /// blackjack's, blackjack.md §5) [design].
    pub fn step_bet(&self, bet: u32, up: bool) -> u32 {
        let step = |b: u32| if b < 10 { 1 } else if b < 50 { 5 } else { 25 };
        let next = if up { bet + step(bet) } else { bet.saturating_sub(step(bet.saturating_sub(1))) };
        self.clamp_bet(next)
    }

    /// Whether `amount` more can go on `point` with `chips` carried: the spot's stake no
    /// more than the table's maximum, the table's no more than the chips [hypothesis].
    pub fn can_place(&self, point: &str, amount: u32, chips: u32) -> Result<Spot, Refusal> {
        let s = spot(point).ok_or(Refusal::NoSpot)?;
        if amount == 0 || self.total() + amount > chips {
            return Err(Refusal::Chips);
        }
        if self.on(point) + amount > self.max_bet {
            return Err(Refusal::Limit);
        }
        Ok(s)
    }

    /// Put `amount` on `point`: the spot's stake after.
    pub fn place(&mut self, point: &str, amount: u32, chips: u32) -> Result<u32, Refusal> {
        let s = self.can_place(point, amount, chips)?;
        match self.bets.iter_mut().find(|(b, _)| b.point == s.point) {
            Some((_, a)) => {
                *a += amount;
                Ok(*a)
            }
            None => {
                self.bets.push((s, amount));
                Ok(amount)
            }
        }
    }

    /// Take the bet on `point` back: its stake.
    pub fn remove(&mut self, point: &str) -> Result<u32, Refusal> {
        let i = self.bets.iter().position(|(s, _)| s.point.eq_ignore_ascii_case(point)).ok_or(Refusal::NoBet)?;
        Ok(self.bets.remove(i).1)
    }

    /// Pay the bets for the pocket the ball came to rest in and clear the table: each
    /// bet's result and the player's net (chips back less chips staked).
    pub fn settle(&mut self, pocket: u8) -> (Vec<Paid>, i64) {
        self.spins += 1;
        let paid: Vec<Paid> = self
            .bets
            .drain(..)
            .map(|(s, stake)| {
                let back = if s.covers(pocket) { stake * (s.payout() + 1) } else { 0 };
                Paid { point: s.point, stake, back }
            })
            .collect();
        let net = paid.iter().map(|p| p.back as i64 - p.stake as i64).sum();
        (paid, net)
    }
}

/// A request from ShowRouletteMenuParams (roulette.md §1); the engine opens the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouletteRequest {
    pub casino: FormId,
    pub min_bet: u32,
    pub max_bet: u32,
    /// 0: the casino's own.
    pub max_winnings: u32,
    /// The reference whose script asked (the table activated), if any.
    pub table: Option<FormId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::SeedableRng;

    #[test]
    fn the_strip_holds_every_pocket_once() {
        let mut seen = [false; POCKETS];
        for p in WHEEL_STRIP {
            assert!(!seen[p as usize]);
            seen[p as usize] = true;
        }
        // Neighbours on a double-zero wheel: 0 between 2 and 28, 00 between 27 and 1.
        let at = |p: u8| WHEEL_STRIP.iter().position(|&x| x == p).unwrap();
        assert_eq!(WHEEL_STRIP[at(0) - 1], 28);
        assert_eq!(WHEEL_STRIP[at(0) + 1], 2);
        assert_eq!(WHEEL_STRIP[at(DOUBLE_ZERO) - 1], 27);
        assert_eq!(WHEEL_STRIP[at(DOUBLE_ZERO) + 1], 1);
        assert_eq!(colour(0), Colour::Green);
        assert_eq!(colour(DOUBLE_ZERO), Colour::Green);
        assert_eq!(colour(17), Colour::Black);
        assert_eq!(colour(32), Colour::Red);
    }

    #[test]
    fn every_point_is_a_bet() {
        let all = points();
        assert_eq!(all.len(), 6 * 24 + 15);
        for p in &all {
            let s = spot(p).unwrap_or_else(|| panic!("{p} is no bet"));
            assert!(!s.numbers.is_empty());
            assert!(s.numbers.iter().all(|&n| (n as usize) < POCKETS));
        }
        assert!(spot("G01").is_none());
        assert!(spot("A25").is_none());
        assert!(spot("S16").is_none());
    }

    #[test]
    fn the_points_name_the_bets_of_the_help_message() {
        let s = |p: &str| spot(p).unwrap();
        assert_eq!(s("B02").numbers, vec![1]);
        assert_eq!(s("F24").numbers, vec![36]);
        assert_eq!(s("D12").numbers, vec![17]);
        assert_eq!((s("C12").kind, s("C12").numbers.clone()), (Kind::Split, vec![16, 17]));
        assert_eq!((s("D13").kind, s("D13").numbers.clone()), (Kind::Split, vec![17, 20]));
        assert_eq!((s("C13").kind, s("C13").numbers.clone()), (Kind::Corner, vec![16, 17, 19, 20]));
        assert_eq!((s("A12").kind, s("A12").numbers.clone()), (Kind::Street, vec![16, 17, 18]));
        assert_eq!((s("A13").kind, s("A13").numbers.clone()), (Kind::SixLine, vec![16, 17, 18, 19, 20, 21]));
        assert_eq!((s("A01").kind, s("A01").numbers.len()), (Kind::TopLine, 5));
        assert_eq!(s("C01").numbers, vec![0, 1, 2]);
        assert_eq!(s("D01").numbers, vec![0, 2, DOUBLE_ZERO]);
        assert_eq!(s("E01").numbers, vec![2, 3, DOUBLE_ZERO]);
        assert_eq!(s("S01").numbers, vec![DOUBLE_ZERO]);
        assert_eq!(s("S03").numbers, vec![0]);
        assert_eq!(s("S07").numbers, (13..=24).collect::<Vec<u8>>());
        assert_eq!(s("S13").numbers, vec![1, 4, 7, 10, 13, 16, 19, 22, 25, 28, 31, 34]);
        assert_eq!(s("S15").numbers[0], 3);
        assert_eq!(s("S08").kind, Kind::Red);
    }

    #[test]
    fn payouts_follow_the_numbers_covered() {
        let pays = |p: &str| spot(p).unwrap().payout();
        assert_eq!(pays("B02"), 35);
        assert_eq!(pays("C02"), 17);
        assert_eq!(pays("A02"), 11);
        assert_eq!(pays("C03"), 8);
        assert_eq!(pays("A03"), 5);
        assert_eq!(pays("C01"), 11);
        assert_eq!(pays("A01"), 6);
        assert_eq!(pays("S04"), 2);
        assert_eq!(pays("S13"), 2);
        assert_eq!(pays("S08"), 1);
        assert_eq!(spot("D13").unwrap().label(), "Split 17-20");
        assert_eq!(spot("A13").unwrap().label(), "Six Line 16-21");
        assert_eq!(spot("D01").unwrap().label(), "Basket 0-00-2");
        assert_eq!(spot("S11").unwrap().label(), "Odd");
        // The kind's name in another language, the numbers as they are (#3146).
        assert_eq!(spot("D13").unwrap().label_with(&|w| if w == "Split" { "Geteilt" } else { w }), "Geteilt 17-20");
    }

    #[test]
    fn bets_are_placed_limited_and_paid() {
        let mut t = Table::new(1, 200);
        assert_eq!(t.place("D12", 10, 50), Ok(10));
        assert_eq!(t.place("D12", 5, 50), Ok(15));
        assert_eq!(t.place("S08", 40, 50), Err(Refusal::Chips));
        assert_eq!(t.place("S08", 35, 50), Ok(35));
        assert_eq!(t.total(), 50);
        assert_eq!(t.place("Z1", 1, 500), Err(Refusal::NoSpot));
        let mut big = Table::new(1, 200);
        assert_eq!(big.place("S08", 150, 1000), Ok(150));
        assert_eq!(big.place("S08", 100, 1000), Err(Refusal::Limit));
        // 17 is black: the straight-up wins 35 to 1, red loses.
        let (paid, net) = t.settle(17);
        assert_eq!(paid[0], Paid { point: "D12".into(), stake: 15, back: 15 * 36 });
        assert_eq!(paid[1].back, 0);
        assert_eq!(net, 15 * 35 - 35);
        assert!(t.bets.is_empty());
        assert_eq!(t.spins, 1);
        assert_eq!(t.remove("D12"), Err(Refusal::NoBet));
        // The zeros lose every outside bet.
        let mut z = Table::new(1, 200);
        z.place("S05", 10, 100).unwrap();
        z.place("S12", 10, 100).unwrap();
        assert_eq!(z.settle(DOUBLE_ZERO).1, -20);
    }

    #[test]
    fn bet_steps() {
        let t = Table::new(1, 200);
        assert_eq!(t.step_bet(1, true), 2);
        assert_eq!(t.step_bet(9, true), 10);
        assert_eq!(t.step_bet(10, true), 15);
        assert_eq!(t.step_bet(10, false), 9);
        assert_eq!(t.step_bet(50, true), 75);
        assert_eq!(t.step_bet(50, false), 45);
        assert_eq!(t.step_bet(200, true), 200);
        assert_eq!(t.step_bet(1, false), 1);
    }

    #[test]
    fn keys_walk_the_felt() {
        assert_eq!(place("B02"), Some((-22.0, 0.0)));
        assert_eq!(place("S13"), Some((26.0, 0.0)));
        assert_eq!(neighbour("D12", 0.0, 1.0).as_deref(), Some("E12"));
        assert_eq!(neighbour("D12", 1.0, 0.0).as_deref(), Some("D13"));
        assert_eq!(neighbour("B02", 0.0, -1.0).as_deref(), Some("A02"));
        assert!(neighbour("A02", 0.0, -1.0).is_some_and(|p| p.starts_with('S')));
        assert_eq!(neighbour("F24", 0.0, 1.0), None);
        for p in points() {
            assert!(place(&p).is_some(), "{p}");
        }
    }

    #[test]
    fn spins_reach_every_pocket() {
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let mut seen = [0u32; POCKETS];
        for _ in 0..3800 {
            seen[spin(&mut rng) as usize] += 1;
        }
        assert!(seen.iter().all(|&n| n > 50));
    }
}
