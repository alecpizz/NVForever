//! Blackjack at the casinos' tables (docs/spec-notes/blackjack.md): the casino records
//! (CSNO), the shoe, the hands, a round's choices, the dealer's play and the paying out;
//! the player's earnings per casino and the winnings levels the casinos' scripts read. The
//! engine draws the table and moves the chips (nvf-play `blackjack`).

use crate::game::Game;
use fnv_sim::FormId;

pub use crate::casino::{add_earnings, opening, winnings_level, CasinoDef, Closed};

use rand_chacha::ChaCha8Rng;
use rand_core::RngCore;

/// The help message shown the first time the menu opens (blackjack.md §5) [observed:
/// FalloutNV.esm MESG `HelpBlackjack`, flags Message Box and Auto Display].
pub const HELP_MESSAGE: FormId = FormId(0x0017_2094);
/// The menu's code to scripts (blackjack.md §1) [documented: GECK wiki MenuMode /
/// `fnv_ui::value::MENU_CLASSES`, `BlackJackMenu = 1081`].
pub const MENU_CODE: u16 = 1081;
/// The most cards a hand can hold on the table: the models have seven places
/// (`Hand1_01`–`07`, `Dealer_01`–`07`) [observed].
pub const MAX_CARDS: usize = 7;

/// A card: rank 1 (ace) to 13 (king), suit 0 to 3 (clubs, diamonds, hearts, spades).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Card(pub u8);

impl Card {
    pub fn new(rank: u8, suit: u8) -> Card {
        debug_assert!((1..=13).contains(&rank) && suit < 4);
        Card(suit * 13 + rank - 1)
    }

    pub fn rank(self) -> u8 {
        self.0 % 13 + 1
    }

    pub fn suit(self) -> u8 {
        self.0 / 13
    }

    /// Its count: 2 to 10 their number, faces 10, an ace 1 (11 is the hand's choice).
    pub fn points(self) -> u32 {
        (self.rank() as u32).min(10)
    }

    /// `h01` (ace of hearts) to `s13` (king of spades), as in the card pictures' names.
    pub fn texture_code(self) -> String {
        format!("{}{:02}", ['c', 'd', 'h', 's'][self.suit() as usize], self.rank())
    }

    /// `"Ah"`, `"10s"`, `"Kd"`: for the log and the tests.
    pub fn short(self) -> String {
        let r = match self.rank() {
            1 => "A".to_string(),
            11 => "J".into(),
            12 => "Q".into(),
            13 => "K".into(),
            n => n.to_string(),
        };
        format!("{r}{}", ['c', 'd', 'h', 's'][self.suit() as usize])
    }

    pub fn parse(s: &str) -> Option<Card> {
        let (r, su) = s.split_at(s.len().checked_sub(1)?);
        let rank = match r {
            "A" | "a" => 1,
            "J" | "j" => 11,
            "Q" | "q" => 12,
            "K" | "k" => 13,
            "T" | "t" => 10,
            n => n.parse().ok().filter(|n| (2..=10).contains(n))?,
        };
        let suit = "cdhs".find(su)? as u8;
        Some(Card::new(rank, suit))
    }
}

/// A hand's total and whether an ace counts 11 in it (soft).
pub fn total(cards: &[Card]) -> (u32, bool) {
    let hard: u32 = cards.iter().map(|c| c.points()).sum();
    if cards.iter().any(|c| c.rank() == 1) && hard + 10 <= 21 {
        (hard + 10, true)
    } else {
        (hard, false)
    }
}

/// An ace and a ten-card as a hand's first two.
pub fn natural(cards: &[Card]) -> bool {
    cards.len() == 2 && total(cards).0 == 21
}

/// 0 to 1 from the generator.
pub fn unit(rng: &mut ChaCha8Rng) -> f32 {
    (rng.next_u32() >> 8) as f32 / (1u32 << 24) as f32
}

/// The cards to deal from: the casino's decks, shuffled again once the casino's share of
/// it is dealt (checked between rounds; blackjack.md §3).
#[derive(Debug, Clone)]
pub struct Shoe {
    cards: Vec<Card>,
    next: usize,
    share: f32,
    /// Cards to deal first, before the shuffled ones (the test aid).
    stacked: Vec<Card>,
}

impl Shoe {
    pub fn new(decks: u32, share: f32, rng: &mut ChaCha8Rng) -> Shoe {
        let decks = decks.max(1);
        let mut s = Shoe { cards: (0..decks).flat_map(|_| (0..52).map(Card)).collect(), next: 0, share, stacked: Vec::new() };
        s.shuffle(rng);
        s
    }

    pub fn shuffle(&mut self, rng: &mut ChaCha8Rng) {
        for i in (1..self.cards.len()).rev() {
            let j = (rng.next_u32() as usize) % (i + 1);
            self.cards.swap(i, j);
        }
        self.next = 0;
    }

    /// Whether the casino's share has been dealt (a share of 0 or less: never; 1 or more:
    /// when the shoe is empty).
    pub fn due(&self) -> bool {
        self.share > 0.0 && self.next as f32 >= self.share.min(1.0) * self.cards.len() as f32
    }

    pub fn len(&self) -> usize {
        self.cards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    pub fn dealt(&self) -> usize {
        self.next
    }

    /// Cards that come out next, in order (the test aid).
    pub fn stack(&mut self, cards: &[Card]) {
        self.stacked.extend(cards.iter().rev());
    }

    fn draw(&mut self, rng: &mut ChaCha8Rng) -> Card {
        if let Some(c) = self.stacked.pop() {
            return c;
        }
        if self.next >= self.cards.len() {
            self.shuffle(rng);
        }
        self.next += 1;
        self.cards[self.next - 1]
    }
}

/// How a hand ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A natural, paid at the payout ratio.
    Blackjack,
    Win,
    Push,
    Lose,
    Bust,
    Surrendered,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Blackjack => "blackjack",
            Outcome::Win => "win",
            Outcome::Push => "push",
            Outcome::Lose => "lose",
            Outcome::Bust => "bust",
            Outcome::Surrendered => "surrendered",
        }
    }
}

/// One of the player's hands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hand {
    pub cards: Vec<Card>,
    pub bet: u32,
    pub doubled: bool,
    /// Made by a split (a natural in it counts as 21, not blackjack).
    pub from_split: bool,
    pub stood: bool,
    pub surrendered: bool,
    pub outcome: Option<Outcome>,
    /// Chips paid back for it, its bet included.
    pub paid: u32,
}

impl Hand {
    pub fn total(&self) -> (u32, bool) {
        total(&self.cards)
    }

    pub fn bust(&self) -> bool {
        self.total().0 > 21
    }

    /// Done: stood, bust, surrendered, or 21.
    pub fn done(&self) -> bool {
        self.stood || self.surrendered || self.total().0 >= 21
    }

    fn natural(&self) -> bool {
        !self.from_split && natural(&self.cards)
    }
}

/// Where a round is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Between rounds: the bet is set, then Deal.
    Betting,
    /// The player plays the active hand.
    Playing,
    /// The round is settled; the cards stay until the next deal.
    Over,
}

/// The player's choices in a round (the menu's play buttons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Hit,
    Stay,
    Double,
    Split,
    Switch,
    Surrender,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Action::Hit => "hit",
            Action::Stay => "stay",
            Action::Double => "double down",
            Action::Split => "split",
            Action::Switch => "switch hands",
            Action::Surrender => "surrender",
        }
    }
}

/// Which choices the active hand has now, and the chips each takes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Legal {
    pub hit: bool,
    pub stay: bool,
    pub double: bool,
    pub split: bool,
    pub switch: bool,
    pub surrender: bool,
}

impl Legal {
    pub fn allows(&self, a: Action) -> bool {
        match a {
            Action::Hit => self.hit,
            Action::Stay => self.stay,
            Action::Double => self.double,
            Action::Split => self.split,
            Action::Switch => self.switch,
            Action::Surrender => self.surrender,
        }
    }
}

/// Whose card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    /// The player's hand (0, or 1 after a split).
    Hand(usize),
    Dealer,
}

/// What happened, in order, for the screen, the sounds and the log.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Shuffled,
    /// A card dealt: to whom, its place in that hand (0-based), face up or not.
    Card { seat: Seat, index: usize, card: Card, up: bool },
    /// A hand split: its second card moved to hand 1.
    Split,
    /// The dealer's hole card turned over.
    Reveal(Card),
    /// The player moved to another hand.
    Active(usize),
    /// A hand ended early (bust, surrendered).
    HandOver { hand: usize, outcome: Outcome },
    /// The round paid out: each hand's outcome and chips back, the dealer's total, and the
    /// player's net (chips back less chips staked).
    Settled { outcomes: Vec<(Outcome, u32)>, dealer: u32, net: i64 },
}

/// Why a choice was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NotNow,
    /// Not enough chips for the bet (or for doubling or splitting it).
    Chips,
    /// The bet outside the table's limits.
    Limits,
}

/// The table's rules for a game (the casino's, with the menu call's limits).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rules {
    pub min_bet: u32,
    pub max_bet: u32,
    pub payout: f32,
    pub stay_soft17: bool,
}

/// A game at the table.
#[derive(Debug, Clone)]
pub struct Table {
    pub rules: Rules,
    pub shoe: Shoe,
    pub phase: Phase,
    pub hands: Vec<Hand>,
    pub active: usize,
    pub dealer: Vec<Card>,
    pub hole_shown: bool,
    pub rounds: u32,
    rng: ChaCha8Rng,
}

impl Table {
    pub fn new(rules: Rules, decks: u32, share: f32, mut rng: ChaCha8Rng) -> Table {
        let shoe = Shoe::new(decks, share, &mut rng);
        Table { rules, shoe, phase: Phase::Betting, hands: Vec::new(), active: 0, dealer: Vec::new(), hole_shown: false, rounds: 0, rng }
    }

    /// The bet one step up or down from `bet` (blackjack.md §5: 1 under 10, 5 under 50, 25
    /// from there [design]), within the table's limits and `chips`.
    pub fn step_bet(&self, bet: u32, up: bool, chips: u32) -> u32 {
        let step = |b: u32| if b < 10 { 1 } else if b < 50 { 5 } else { 25 };
        let next = if up {
            let s = step(bet);
            (bet / s + 1) * s
        } else {
            let s = step(bet.saturating_sub(1));
            bet.saturating_sub(1) / s * s
        };
        self.clamp_bet(next, chips)
    }

    /// A bet held within the table's limits and the chips carried (the minimum even when
    /// fewer chips are carried; the deal is refused then).
    pub fn clamp_bet(&self, bet: u32, chips: u32) -> u32 {
        bet.min(self.rules.max_bet).min(chips).max(self.rules.min_bet)
    }

    pub fn max_bet(&self, chips: u32) -> u32 {
        self.clamp_bet(self.rules.max_bet, chips)
    }

    /// Chips staked on the table now (every hand's bet).
    pub fn staked(&self) -> u32 {
        self.hands.iter().map(|h| h.bet).sum()
    }

    /// Deal a round with this bet; `chips` the chips the player carries (the bet included).
    pub fn deal(&mut self, bet: u32, chips: u32) -> Result<Vec<Event>, Refusal> {
        if self.phase == Phase::Playing {
            return Err(Refusal::NotNow);
        }
        if bet < self.rules.min_bet || bet > self.rules.max_bet {
            return Err(Refusal::Limits);
        }
        if bet > chips {
            return Err(Refusal::Chips);
        }
        let mut ev = Vec::new();
        if self.shoe.due() {
            self.shoe.shuffle(&mut self.rng);
            ev.push(Event::Shuffled);
        }
        self.rounds += 1;
        self.hands = vec![Hand { bet, ..Default::default() }];
        self.dealer.clear();
        self.active = 0;
        self.hole_shown = false;
        self.phase = Phase::Playing;
        for (seat, up) in [(Seat::Hand(0), true), (Seat::Dealer, true), (Seat::Hand(0), true), (Seat::Dealer, false)] {
            self.give(seat, up, &mut ev);
        }
        // A natural: the hole card is turned at once and the round settles.
        if self.hands[0].natural() {
            self.finish(&mut ev);
        }
        Ok(ev)
    }

    fn give(&mut self, seat: Seat, up: bool, ev: &mut Vec<Event>) {
        let card = self.shoe.draw(&mut self.rng);
        let index = match seat {
            Seat::Hand(h) => {
                self.hands[h].cards.push(card);
                self.hands[h].cards.len() - 1
            }
            Seat::Dealer => {
                self.dealer.push(card);
                self.dealer.len() - 1
            }
        };
        ev.push(Event::Card { seat, index, card, up });
    }

    /// The choices the active hand has; `chips` the chips carried beyond those staked.
    pub fn legal(&self, chips: u32) -> Legal {
        if self.phase != Phase::Playing {
            return Legal::default();
        }
        let Some(h) = self.hands.get(self.active) else { return Legal::default() };
        let first_two = h.cards.len() == 2 && !h.doubled;
        let other = 1 - self.active.min(1);
        Legal {
            hit: !h.done() && h.cards.len() < MAX_CARDS,
            stay: !h.done(),
            double: !h.done() && first_two && chips >= h.bet,
            split: !h.done() && first_two && self.hands.len() == 1 && h.cards[0].rank() == h.cards[1].rank() && chips >= h.bet,
            switch: self.hands.len() == 2 && !self.hands[other].done(),
            surrender: !h.done() && first_two && self.hands.len() == 1,
        }
    }

    /// The player's choice; `chips` the chips carried beyond those staked (doubling and
    /// splitting take the bet again from them).
    pub fn act(&mut self, action: Action, chips: u32) -> Result<Vec<Event>, Refusal> {
        let legal = self.legal(chips);
        if !legal.allows(action) {
            let short = matches!(action, Action::Double | Action::Split) && self.phase == Phase::Playing && self.hands.get(self.active).is_some_and(|h| chips < h.bet);
            return Err(if short { Refusal::Chips } else { Refusal::NotNow });
        }
        let mut ev = Vec::new();
        let a = self.active;
        match action {
            Action::Hit => {
                self.give(Seat::Hand(a), true, &mut ev);
                if self.hands[a].bust() {
                    self.hands[a].outcome = Some(Outcome::Bust);
                    ev.push(Event::HandOver { hand: a, outcome: Outcome::Bust });
                }
            }
            Action::Stay => self.hands[a].stood = true,
            Action::Double => {
                let h = &mut self.hands[a];
                h.bet *= 2;
                h.doubled = true;
                self.give(Seat::Hand(a), true, &mut ev);
                let h = &mut self.hands[a];
                h.stood = true;
                if h.bust() {
                    h.outcome = Some(Outcome::Bust);
                    ev.push(Event::HandOver { hand: a, outcome: Outcome::Bust });
                }
            }
            Action::Split => {
                let second = self.hands[0].cards.pop().expect("two cards");
                let bet = self.hands[0].bet;
                self.hands[0].from_split = true;
                self.hands.push(Hand { cards: vec![second], bet, from_split: true, ..Default::default() });
                ev.push(Event::Split);
                self.give(Seat::Hand(0), true, &mut ev);
                self.give(Seat::Hand(1), true, &mut ev);
            }
            Action::Switch => {
                self.active = 1 - a;
                ev.push(Event::Active(self.active));
            }
            Action::Surrender => {
                let h = &mut self.hands[a];
                h.surrendered = true;
                h.outcome = Some(Outcome::Surrendered);
                ev.push(Event::HandOver { hand: a, outcome: Outcome::Surrendered });
            }
        }
        // The active hand done: on to the other one, or the dealer's turn.
        if self.hands[self.active].done() {
            if let Some(next) = (0..self.hands.len()).find(|&i| !self.hands[i].done()) {
                self.active = next;
                ev.push(Event::Active(next));
            } else {
                self.finish(&mut ev);
            }
        }
        Ok(ev)
    }

    /// The dealer's turn and the paying out.
    fn finish(&mut self, ev: &mut Vec<Event>) {
        self.hole_shown = true;
        if let Some(&hole) = self.dealer.get(1) {
            ev.push(Event::Reveal(hole));
        }
        let player_natural = self.hands.len() == 1 && self.hands[0].natural();
        let live = self.hands.iter().any(|h| !h.bust() && !h.surrendered);
        // The dealer draws only against a hand still standing, and not against a natural.
        if live && !player_natural {
            while self.dealer.len() < MAX_CARDS && !dealer_stands(&self.dealer, self.rules.stay_soft17) {
                self.give(Seat::Dealer, true, ev);
            }
        }
        let (dealer, _) = total(&self.dealer);
        let dealer_natural = natural(&self.dealer);
        let payout = self.rules.payout;
        let mut outcomes = Vec::new();
        for h in &mut self.hands {
            let (t, _) = h.total();
            let outcome = if h.surrendered {
                Outcome::Surrendered
            } else if t > 21 {
                Outcome::Bust
            } else if h.natural() {
                if dealer_natural {
                    Outcome::Push
                } else {
                    Outcome::Blackjack
                }
            } else if dealer_natural || (dealer <= 21 && dealer > t) {
                Outcome::Lose
            } else if dealer > 21 || t > dealer {
                Outcome::Win
            } else {
                Outcome::Push
            };
            h.paid = match outcome {
                Outcome::Blackjack => h.bet + (h.bet as f32 * payout).floor() as u32,
                Outcome::Win => 2 * h.bet,
                Outcome::Push => h.bet,
                Outcome::Surrendered => h.bet.div_ceil(2),
                Outcome::Lose | Outcome::Bust => 0,
            };
            h.outcome = Some(outcome);
            outcomes.push((outcome, h.paid));
        }
        let net = self.hands.iter().map(|h| h.paid as i64 - h.bet as i64).sum();
        self.phase = Phase::Over;
        ev.push(Event::Settled { outcomes, dealer, net });
    }

    /// Cards that come out next (the test aid).
    pub fn stack(&mut self, cards: &[Card]) {
        self.shoe.stack(cards);
    }
}

/// Whether the dealer stands on these cards: over 17, or a hard 17 (a soft one too when
/// the casino stays on soft 17).
pub fn dealer_stands(cards: &[Card], stay_soft17: bool) -> bool {
    let (t, soft) = total(cards);
    t > 17 || (t == 17 && (!soft || stay_soft17))
}

/// The help message to show as the menu opens, the first time only (as lockpicking's).
pub fn first_help(g: &mut Game) -> Option<(String, String)> {
    // The player's tweak: no first-time help [design: docs/design/tweaks.md].
    if g.player.blackjack_help_shown || g.tweaks.no_tutorials {
        return None;
    }
    let m = g.data.messages.get(&HELP_MESSAGE)?;
    let text = m.text.clone()?;
    g.player.blackjack_help_shown = true;
    Some((m.title.clone().unwrap_or_default(), text))
}

/// A request from ShowBlackJackMenuParams (blackjack.md §1); the engine opens the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlackjackRequest {
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

    fn cards(s: &str) -> Vec<Card> {
        s.split_whitespace().map(|c| Card::parse(c).unwrap()).collect()
    }

    fn table(stay_soft17: bool) -> Table {
        Table::new(Rules { min_bet: 1, max_bet: 200, payout: 1.5, stay_soft17 }, 3, 0.2, ChaCha8Rng::seed_from_u64(7))
    }

    /// A round dealt from these cards: player, dealer up, player, dealer hole, then more.
    fn dealt(t: &mut Table, s: &str, bet: u32) -> Vec<Event> {
        t.stack(&cards(s));
        t.deal(bet, 1000).unwrap()
    }

    /// A settled round: the outcomes, the dealer's total, the net.
    type Settled = (Vec<(Outcome, u32)>, u32, i64);

    fn settled(ev: &[Event]) -> Option<Settled> {
        ev.iter().find_map(|e| match e {
            Event::Settled { outcomes, dealer, net } => Some((outcomes.clone(), *dealer, *net)),
            _ => None,
        })
    }

    #[test]
    fn cards_read_and_count() {
        assert_eq!(Card::parse("Ah").unwrap().texture_code(), "h01");
        assert_eq!(Card::parse("Ks").unwrap().texture_code(), "s13");
        assert_eq!(Card::parse("10d").unwrap().short(), "10d");
        assert_eq!(total(&cards("Ah 6c")), (17, true));
        assert_eq!(total(&cards("Ah 6c 10d")), (17, false));
        assert_eq!(total(&cards("Ah Ac 9d")), (21, true));
        assert!(natural(&cards("Qs Ad")));
        assert!(!natural(&cards("7s 7d 7c")));
        assert_eq!((0..52).map(Card).filter(|c| c.points() == 10).count(), 16);
    }

    #[test]
    fn a_natural_pays_three_to_two_at_once() {
        let mut t = table(false);
        let ev = dealt(&mut t, "Ah 9c Kd 7s", 10);
        let (o, dealer, net) = settled(&ev).unwrap();
        assert_eq!(o, vec![(Outcome::Blackjack, 25)]);
        assert_eq!((dealer, net), (16, 15));
        assert!(ev.contains(&Event::Reveal(Card::parse("7s").unwrap())));
        assert_eq!(t.phase, Phase::Over);
        // Both naturals: a push.
        let ev = dealt(&mut t, "Ah Ac Kd Qs", 10);
        assert_eq!(settled(&ev).unwrap().0, vec![(Outcome::Push, 10)]);
    }

    #[test]
    fn the_dealer_hits_or_stays_on_soft_17_by_the_casino() {
        // Player 18 stands; dealer A + 6 = soft 17, then a 4 to 21.
        let mut hit = table(false);
        dealt(&mut hit, "10h Ah 8c 6d 4s", 10);
        let ev = hit.act(Action::Stay, 990).unwrap();
        let (o, dealer, net) = settled(&ev).unwrap();
        assert_eq!((o[0].0, dealer, net), (Outcome::Lose, 21, -10));
        let mut stay = table(true);
        dealt(&mut stay, "10h Ah 8c 6d 4s", 10);
        let (o, dealer, net) = settled(&stay.act(Action::Stay, 990).unwrap()).unwrap();
        assert_eq!((o[0].0, dealer, net), (Outcome::Win, 17, 10));
    }

    #[test]
    fn a_bust_loses_and_the_dealer_does_not_draw() {
        let mut t = table(false);
        dealt(&mut t, "10h 5c 6d 9s Kc", 10);
        let ev = t.act(Action::Hit, 990).unwrap();
        assert!(ev.contains(&Event::HandOver { hand: 0, outcome: Outcome::Bust }));
        let (o, dealer, net) = settled(&ev).unwrap();
        assert_eq!((o[0].0, dealer, net), (Outcome::Bust, 14, -10));
        assert_eq!(t.dealer.len(), 2);
    }

    #[test]
    fn doubling_takes_one_card_and_doubles_the_bet() {
        let mut t = table(false);
        dealt(&mut t, "6h 10c 5d 7s Ah 2c", 20);
        assert!(t.legal(980).double);
        assert!(!t.legal(10).double);
        assert_eq!(t.act(Action::Double, 10), Err(Refusal::Chips));
        let ev = t.act(Action::Double, 980).unwrap();
        // 6 + 5 + A = 12: stood; the dealer's 17 wins against 40 staked.
        let (o, dealer, net) = settled(&ev).unwrap();
        assert_eq!((o[0], dealer, net), ((Outcome::Lose, 0), 17, -40));
        assert_eq!(t.hands[0].cards.len(), 3);
    }

    #[test]
    fn a_split_makes_two_hands_played_in_turn() {
        let mut t = table(false);
        // Player 8 8, dealer 10 + 6; the split hands get 3 and 10; dealer draws a 5.
        dealt(&mut t, "8h 10c 8d 6s 3c 10d 9h 5c", 10);
        assert!(t.legal(990).split);
        assert!(!t.legal(990).switch);
        let ev = t.act(Action::Split, 990).unwrap();
        assert!(ev.contains(&Event::Split));
        assert_eq!(t.hands.len(), 2);
        assert_eq!(t.hands[0].cards, cards("8h 3c"));
        assert_eq!(t.hands[1].cards, cards("8d 10d"));
        assert_eq!(t.staked(), 20);
        assert!(t.legal(980).switch);
        assert!(!t.legal(980).split && !t.legal(980).surrender);
        // Hand 0 hits to 20 (9h) and stands; hand 1 stands on 18; dealer 16 + 5 = 21.
        t.act(Action::Hit, 980).unwrap();
        let ev = t.act(Action::Stay, 980).unwrap();
        assert_eq!(ev, vec![Event::Active(1)]);
        let ev = t.act(Action::Stay, 980).unwrap();
        let (o, dealer, net) = settled(&ev).unwrap();
        assert_eq!(dealer, 21);
        assert_eq!(o, vec![(Outcome::Lose, 0), (Outcome::Lose, 0)]);
        assert_eq!(net, -20);
    }

    #[test]
    fn switching_hands_moves_between_the_split_hands() {
        let mut t = table(false);
        dealt(&mut t, "8h 10c 8d 6s 3c 10d", 10);
        t.act(Action::Split, 990).unwrap();
        assert_eq!(t.act(Action::Switch, 980).unwrap(), vec![Event::Active(1)]);
        assert_eq!(t.active, 1);
        t.act(Action::Stay, 980).unwrap();
        assert_eq!(t.active, 0);
    }

    #[test]
    fn surrender_gives_half_back() {
        let mut t = table(false);
        dealt(&mut t, "10h 10c 6d 9s", 25);
        let ev = t.act(Action::Surrender, 975).unwrap();
        let (o, _, net) = settled(&ev).unwrap();
        assert_eq!(o, vec![(Outcome::Surrendered, 13)]);
        assert_eq!(net, -12);
        // The dealer does not draw against a surrendered hand.
        assert_eq!(t.dealer.len(), 2);
    }

    #[test]
    fn a_split_natural_is_21_not_blackjack() {
        let mut t = table(false);
        dealt(&mut t, "Ah 10c Ad 7s Kc 2d", 10);
        t.act(Action::Split, 990).unwrap();
        // Hand 0 is A K: done at 21, hand 1 (A 2) plays on.
        assert_eq!(t.active, 1);
        let ev = t.act(Action::Stay, 980).unwrap();
        let (o, dealer, _) = settled(&ev).unwrap();
        assert_eq!(dealer, 17);
        assert_eq!(o[0], (Outcome::Win, 20));
        assert_eq!(o[1], (Outcome::Lose, 0));
    }

    #[test]
    fn bets_step_within_the_limits_and_the_chips() {
        let t = table(false);
        assert_eq!(t.step_bet(1, true, 1000), 2);
        assert_eq!(t.step_bet(9, true, 1000), 10);
        assert_eq!(t.step_bet(10, true, 1000), 15);
        assert_eq!(t.step_bet(50, true, 1000), 75);
        assert_eq!(t.step_bet(200, true, 1000), 200);
        assert_eq!(t.step_bet(75, false, 1000), 50);
        assert_eq!(t.step_bet(50, false, 1000), 45);
        assert_eq!(t.step_bet(10, false, 1000), 9);
        assert_eq!(t.step_bet(1, false, 1000), 1);
        assert_eq!(t.step_bet(20, true, 22), 22);
        assert_eq!(t.max_bet(1000), 200);
        assert_eq!(t.max_bet(55), 55);
    }

    #[test]
    fn deals_are_refused_outside_the_limits() {
        let mut t = table(false);
        assert_eq!(t.deal(0, 100), Err(Refusal::Limits));
        assert_eq!(t.deal(201, 1000), Err(Refusal::Limits));
        assert_eq!(t.deal(50, 20), Err(Refusal::Chips));
        t.deal(5, 100).unwrap();
        if t.phase == Phase::Playing {
            assert_eq!(t.deal(5, 100), Err(Refusal::NotNow));
        }
    }

    #[test]
    fn the_shoe_is_shuffled_once_its_share_is_dealt() {
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let mut s = Shoe::new(3, 0.2, &mut rng);
        assert_eq!(s.len(), 156);
        let mut seen = std::collections::HashMap::new();
        for _ in 0..31 {
            *seen.entry(s.draw(&mut rng)).or_insert(0) += 1;
        }
        assert!(!s.due());
        s.draw(&mut rng);
        assert!(s.due(), "32 of 156 is past a fifth");
        assert!(seen.values().all(|&n| n <= 3));
        let mut t = table(false);
        let mut shuffles = 0;
        for _ in 0..40 {
            let ev = t.deal(1, 1000).unwrap();
            shuffles += ev.iter().filter(|e| **e == Event::Shuffled).count();
            while t.phase == Phase::Playing {
                t.act(Action::Stay, 1000).unwrap();
            }
        }
        assert!(shuffles >= 3, "{shuffles}");
    }

    #[test]
    fn many_rounds_keep_the_chips_right() {
        let mut t = table(false);
        let mut rng = ChaCha8Rng::seed_from_u64(11);
        let mut chips: i64 = 1_000_000;
        for _ in 0..2000 {
            let bet = 10;
            let ev = t.deal(bet, chips as u32).unwrap();
            let mut net = settled(&ev).map(|s| s.2);
            while t.phase == Phase::Playing {
                let free = (chips as u32).saturating_sub(t.staked());
                let l = t.legal(free);
                let choices: Vec<Action> = [Action::Hit, Action::Stay, Action::Double, Action::Split, Action::Surrender].into_iter().filter(|a| l.allows(*a)).collect();
                let a = choices[(rng.next_u32() as usize) % choices.len()];
                let ev = t.act(a, free).unwrap();
                net = net.or(settled(&ev).map(|s| s.2));
            }
            let net = net.expect("every round settles");
            let staked: i64 = t.hands.iter().map(|h| h.bet as i64).sum();
            let paid: i64 = t.hands.iter().map(|h| h.paid as i64).sum();
            assert_eq!(net, paid - staked);
            assert!(t.hands.iter().all(|h| h.cards.len() <= MAX_CARDS && h.outcome.is_some()));
            chips += net;
        }
    }
}
