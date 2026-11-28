//! Caravan, the card game (docs/spec-notes/caravan.md): the cards (CCRD) and decks (CDCK),
//! the cards the player owns and the deck they built, the bet, the game's rules and the
//! opponent. The engine draws the menu (nvf-play `caravan.rs`).
//!
//! A game is two [`Side`]s, the player's ([`PLAYER`]) and the opponent's ([`OPPONENT`]),
//! each with a deck, a hand and three caravans, one per track. [`Game::apply`] carries out
//! a [`Move`] and says what happened as [`Event`]s; [`choose`] is the opponent's choice.

use crate::game::Game as Rules;
use fnv_sim::FormId;
use rand_chacha::ChaCha8Rng;
use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The menu's code to scripts [documented: GECK wiki GetMenuTargetRef; caravan.md §1].
pub const MENU_CODE: u16 = 1083;
/// The fewest cards in a deck [documented: HelpCaravanDeckBuilding; owner ruling].
pub const MIN_DECK: usize = 30;
/// Cards each side draws at the start [documented: HelpCaravanStartingCaravans].
pub const HAND: usize = 8;
pub const TRACKS: usize = 3;
pub const PLAYER: usize = 0;
pub const OPPONENT: usize = 1;
/// A caravan is sold at these values [documented: HelpCaravanContractWar].
pub const SOLD_MIN: u32 = 21;
pub const SOLD_MAX: u32 = 26;
/// The help messages (MESG) [observed: FalloutNV.esm].
pub const HELP_BETTING: FormId = FormId(0x0000_0186);
pub const HELP_DECK: FormId = FormId(0x0000_0187);
pub const HELP_STARTING: FormId = FormId(0x0000_0188);
pub const HELP_WAR: FormId = FormId(0x0000_0189);
/// The caps (the currency bet) [documented: FormID 0000000F].
pub const CAPS: FormId = FormId(0xF);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Suit {
    #[default]
    Hearts,
    Spades,
    Diamonds,
    Clubs,
    Joker,
}

impl Suit {
    /// From the card's first INTV [documented: xEdit, suit 1–5].
    pub fn from_code(c: u32) -> Option<Suit> {
        Some(match c {
            1 => Suit::Hearts,
            2 => Suit::Spades,
            3 => Suit::Diamonds,
            4 => Suit::Clubs,
            5 => Suit::Joker,
            _ => return None,
        })
    }

    pub fn letter(self) -> char {
        match self {
            Suit::Hearts => 'h',
            Suit::Spades => 's',
            Suit::Diamonds => 'd',
            Suit::Clubs => 'c',
            Suit::Joker => 'j',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Rank {
    /// An ace (1) or a numbered card (2–10).
    Num(u8),
    Jack,
    Queen,
    King,
    #[default]
    Joker,
}

impl Rank {
    /// From the card's second INTV [documented: xEdit, 1 Ace, 2–10, 12 Jack, 13 Queen, 14
    /// King, 15 Joker].
    pub fn from_code(c: u32) -> Option<Rank> {
        Some(match c {
            1..=10 => Rank::Num(c as u8),
            12 => Rank::Jack,
            13 => Rank::Queen,
            14 => Rank::King,
            15 => Rank::Joker,
            _ => return None,
        })
    }

    pub fn number(self) -> Option<u32> {
        match self {
            Rank::Num(n) => Some(n as u32),
            _ => None,
        }
    }

    pub fn is_face(self) -> bool {
        self.number().is_none()
    }

    pub fn short(self) -> String {
        match self {
            Rank::Num(1) => "A".into(),
            Rank::Num(n) => n.to_string(),
            Rank::Jack => "J".into(),
            Rank::Queen => "Q".into(),
            Rank::King => "K".into(),
            Rank::Joker => "Jo".into(),
        }
    }
}

/// A card in play: its form (for its name and textures), suit and rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Card {
    pub form: FormId,
    pub suit: Suit,
    pub rank: Rank,
}

impl Default for Card {
    fn default() -> Card {
        Card::new(FormId(0), Suit::default(), Rank::default())
    }
}

impl Card {
    pub fn new(form: FormId, suit: Suit, rank: Rank) -> Card {
        Card { form, suit, rank }
    }

    /// `A`, `2`…`10`, `J`, `Q`, `K` then the suit letter (`10h`, `Ks`), or `Jo` (a joker);
    /// the form is 0 (tests and the test aid).
    pub fn parse(s: &str) -> Option<Card> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("jo") {
            return Some(Card::new(FormId(0), Suit::Joker, Rank::Joker));
        }
        let (r, su) = s.split_at(s.len().checked_sub(1)?);
        let suit = match su.to_ascii_lowercase().as_str() {
            "h" => Suit::Hearts,
            "s" => Suit::Spades,
            "d" => Suit::Diamonds,
            "c" => Suit::Clubs,
            _ => return None,
        };
        let rank = match r.to_ascii_uppercase().as_str() {
            "A" => Rank::Num(1),
            "J" => Rank::Jack,
            "Q" => Rank::Queen,
            "K" => Rank::King,
            n => Rank::Num(n.parse::<u8>().ok().filter(|n| (2..=10).contains(n))?),
        };
        Some(Card::new(FormId(0), suit, rank))
    }

    pub fn short(&self) -> String {
        match self.rank {
            Rank::Joker => "Jo".into(),
            r => format!("{}{}", r.short(), self.suit.letter()),
        }
    }
}

/// A caravan card's record (CCRD; caravan.md §2).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CardDef {
    pub name: String,
    pub suit: Suit,
    pub rank: Rank,
    /// TX00, TX01: the face and the back, under `textures\`.
    pub face: Option<String>,
    pub back: Option<String>,
}

impl CardDef {
    /// From the record's subrecords [documented: schemas/fnv CCRD, xEdit]; `None` when its
    /// suit or value cannot be read.
    pub fn from_raw(subs: &[fnv_esm::Subrecord]) -> Option<CardDef> {
        let mut c = CardDef::default();
        let mut intv = Vec::new();
        for s in subs {
            match &s.sig {
                b"FULL" => c.name = zstring(&s.data),
                b"TX00" => c.face = Some(zstring(&s.data)).filter(|p| !p.is_empty()),
                b"TX01" => c.back = Some(zstring(&s.data)).filter(|p| !p.is_empty()),
                b"INTV" => intv.extend(u32_at(&s.data, 0)),
                _ => {}
            }
        }
        c.suit = Suit::from_code(*intv.first()?)?;
        c.rank = Rank::from_code(*intv.get(1)?)?;
        Some(c)
    }
}

/// A Caravan deck's record (CDCK; caravan.md §2).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeckDef {
    pub name: String,
    /// One per CARD subrecord, in the record's order.
    pub cards: Vec<FormId>,
}

impl DeckDef {
    /// `map` turns a file's FormID into the load order's [observed: CARD is a CCRD FormID,
    /// u32].
    pub fn from_raw(subs: &[fnv_esm::Subrecord], map: &dyn Fn(u32) -> Option<u32>) -> DeckDef {
        let mut d = DeckDef::default();
        for s in subs {
            match &s.sig {
                b"FULL" => d.name = zstring(&s.data),
                b"CARD" => d.cards.extend(u32_at(&s.data, 0).filter(|&x| x != 0).and_then(map).map(FormId)),
                _ => {}
            }
        }
        d
    }
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).and_then(|x| <[u8; 4]>::try_from(x).ok()).map(u32::from_le_bytes)
}

fn zstring(b: &[u8]) -> String {
    // In this run's code page (fnv_util::text, #1439).
    fnv_util::text::decode_z(b)
}

/// The player's cards, deck and record against each opponent (saved with the game).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CaravanState {
    /// Every card AddCardToPlayer gave, in the order given (a form given twice is here
    /// twice).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub cards: Vec<FormId>,
    /// The last deck built (caravan.md §4).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub deck: Vec<FormId>,
    /// The help messages shown so far (bits of [`Help`]).
    #[serde(skip_serializing_if = "is_zero", default)]
    pub help: u8,
    /// By opponent (the actor).
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub records: BTreeMap<FormId, Record>,
}

impl CaravanState {
    pub fn is_empty(&self) -> bool {
        *self == CaravanState::default()
    }
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

/// The player's games against one opponent (caravan.md §7).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub won: u32,
    pub lost: u32,
    /// Caps won and lost over every game.
    pub winnings: u64,
    pub losses: u64,
    pub biggest_pot: u32,
}

impl Record {
    pub fn net(&self) -> i64 {
        self.winnings as i64 - self.losses as i64
    }
}

/// A help message, shown once over the screen it explains (caravan.md §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Help {
    Betting,
    Deck,
    Starting,
    War,
}

impl Help {
    fn bit(self) -> u8 {
        1 << self as u8
    }

    pub fn message(self) -> FormId {
        match self {
            Help::Betting => HELP_BETTING,
            Help::Deck => HELP_DECK,
            Help::Starting => HELP_STARTING,
            Help::War => HELP_WAR,
        }
    }
}

/// The help message's title and text the first time it is asked for, else `None`.
pub fn first_help(g: &mut Rules, which: Help) -> Option<(String, String)> {
    if g.player.caravan.help & which.bit() != 0 {
        return None;
    }
    let m = g.data.messages.get(&which.message())?;
    let text = m.text.clone()?;
    g.player.caravan.help |= which.bit();
    Some((m.title.clone().unwrap_or_default(), text))
}

/// AddCardToPlayer: a card form for the player's collection; `false` when it is not a
/// caravan card.
pub fn add_card(g: &mut Rules, card: FormId) -> bool {
    if !g.data.caravan_cards.contains_key(&card) {
        return false;
    }
    g.player.caravan.cards.push(card);
    true
}

/// The cards the deck builder offers: each form owned once, in suit-blind rank order
/// (aces first, jokers last) as the screenshots show [observed].
pub fn owned(g: &Rules) -> Vec<FormId> {
    let mut out: Vec<FormId> = Vec::new();
    for &c in &g.player.caravan.cards {
        if !out.contains(&c) && g.data.caravan_cards.contains_key(&c) {
            out.push(c);
        }
    }
    let key = |f: &FormId| g.data.caravan_cards.get(f).map(|d| (d.rank, d.suit)).unwrap_or_default();
    out.sort_by_key(|f| (key(f), f.0));
    out
}

/// The cards of forms, those that are caravan cards.
pub fn cards_of(g: &Rules, forms: &[FormId]) -> Vec<Card> {
    forms.iter().filter_map(|&f| g.data.caravan_cards.get(&f).map(|d| Card::new(f, d.suit, d.rank))).collect()
}

/// The saved deck, the cards no longer owned taken out.
pub fn saved_deck(g: &Rules) -> Vec<FormId> {
    let own = owned(g);
    let mut out: Vec<FormId> = Vec::new();
    for &c in &g.player.caravan.deck {
        if own.contains(&c) && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// A random deck: between [`MIN_DECK`] cards and every card owned [design; caravan.md §4].
pub fn randomize(owned: &[FormId], rng: &mut ChaCha8Rng) -> Vec<FormId> {
    let mut all = owned.to_vec();
    shuffle(&mut all, rng);
    let lo = MIN_DECK.min(all.len());
    let n = lo + (rng.next_u32() as usize) % (all.len() - lo + 1);
    all.truncate(n);
    all
}

pub fn shuffle<T>(v: &mut [T], rng: &mut ChaCha8Rng) {
    for i in (1..v.len()).rev() {
        let j = (rng.next_u32() as usize) % (i + 1);
        v.swap(i, j);
    }
}

/// A request from ShowCaravanMenu (caravan.md §1); the engine opens the menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaravanRequest {
    /// The calling reference: the opponent.
    pub opponent: FormId,
    pub deck: FormId,
    /// 0–3; not used (the deck's face cards set the skill, §6).
    pub difficulty: i32,
    /// The most of its currency the opponent bets.
    pub max_bet_share: f32,
}

/// The bet (caravan.md §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bet {
    /// The opponent's bid.
    pub bid: u32,
    /// The player's ante, what each side puts in.
    pub ante: u32,
    /// The player's and the opponent's currency.
    pub funds: u32,
    pub opponent_funds: u32,
}

impl Bet {
    pub fn new(funds: u32, opponent_funds: u32, share: f32) -> Bet {
        let bid = (opponent_funds as f32 * share.clamp(0.0, 1.0)).floor() as u32;
        Bet { bid, ante: 0, funds, opponent_funds }
    }

    /// The most the ante can be: the opponent raises to half again its bid [design].
    pub fn max(&self) -> u32 {
        (self.bid + self.bid / 2).min(self.funds).min(self.opponent_funds)
    }

    pub fn can_match(&self) -> bool {
        self.ante != self.bid.min(self.max())
    }

    pub fn auto_match(&mut self) -> bool {
        let before = self.ante;
        self.ante = self.bid.min(self.max());
        self.ante != before
    }

    /// A tenth of the bid at a time [design].
    pub fn step(&self) -> u32 {
        (self.bid / 10).max(1)
    }

    pub fn can_raise(&self) -> bool {
        self.ante < self.max()
    }

    pub fn raise(&mut self) -> bool {
        let before = self.ante;
        self.ante = (self.ante + self.step()).min(self.max());
        self.ante != before
    }

    /// Accept is dimmed at an ante of 0 [observed], unless the opponent has nothing to
    /// bid [design].
    pub fn can_accept(&self) -> bool {
        self.ante > 0 || self.max() == 0
    }

    pub fn pot(&self) -> u32 {
        self.ante * 2
    }
}

/// A card on a caravan with the face cards put on it, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Placed {
    pub card: Card,
    pub faces: Vec<Card>,
}

impl Placed {
    pub fn base(&self) -> u32 {
        self.card.rank.number().unwrap_or(0)
    }

    /// The card's worth: each King on it doubles it [owner ruling 2026-10-19].
    pub fn value(&self) -> u32 {
        let kings = self.faces.iter().filter(|f| f.rank == Rank::King).count() as u32;
        self.base() << kings.min(16)
    }

    fn queens(&self) -> impl Iterator<Item = &Card> {
        self.faces.iter().filter(|f| f.rank == Rank::Queen)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
}

impl Dir {
    fn flip(self) -> Dir {
        match self {
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
        }
    }
}

/// One side's pile on a track.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Caravan {
    pub cards: Vec<Placed>,
}

impl Caravan {
    pub fn value(&self) -> u32 {
        self.cards.iter().map(Placed::value).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }

    /// The last card's suit, or the last Queen's on it.
    pub fn suit(&self) -> Option<Suit> {
        let last = self.cards.last()?;
        Some(last.queens().last().map_or(last.card.suit, |q| q.suit))
    }

    /// Up or down by the last two cards, reversed by each Queen on the last.
    pub fn direction(&self) -> Option<Dir> {
        let n = self.cards.len();
        if n < 2 {
            return None;
        }
        let (a, b) = (self.cards[n - 2].base(), self.cards[n - 1].base());
        let d = if b > a { Dir::Up } else { Dir::Down };
        Some(if self.cards[n - 1].queens().count() % 2 == 1 { d.flip() } else { d })
    }

    /// Whether a numbered card or ace can go on the end (caravan.md §5).
    pub fn accepts(&self, card: Card) -> bool {
        let Some(n) = card.rank.number() else { return false };
        let Some(last) = self.cards.last() else { return true };
        if n == last.base() {
            return false;
        }
        if self.suit() == Some(card.suit) {
            return true;
        }
        match self.direction() {
            None => true,
            Some(Dir::Up) => n > last.base(),
            Some(Dir::Down) => n < last.base(),
        }
    }

    pub fn sold(&self) -> bool {
        (SOLD_MIN..=SOLD_MAX).contains(&self.value())
    }

    pub fn over(&self) -> bool {
        self.value() > SOLD_MAX
    }

    fn all_cards(&self) -> impl Iterator<Item = &Card> {
        self.cards.iter().flat_map(|p| std::iter::once(&p.card).chain(p.faces.iter()))
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Side {
    /// The cards still to draw; the next is the last.
    pub deck: Vec<Card>,
    pub hand: Vec<Card>,
    pub caravans: [Caravan; TRACKS],
}

impl Side {
    pub fn new(mut deck: Vec<Card>, rng: &mut ChaCha8Rng) -> Side {
        shuffle(&mut deck, rng);
        let mut s = Side { deck, ..Default::default() };
        for _ in 0..HAND {
            s.draw();
        }
        s
    }

    fn draw(&mut self) -> Option<Card> {
        let c = self.deck.pop()?;
        self.hand.push(c);
        Some(c)
    }

    pub fn started(&self) -> bool {
        self.caravans.iter().all(|c| !c.is_empty())
    }

    /// Cards in the hand and the deck.
    pub fn cards_left(&self) -> usize {
        self.hand.len() + self.deck.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Each side starts its three caravans.
    Opening,
    /// The contract war.
    War,
    Over,
}

/// Why the game ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// All three tracks sold.
    Sold,
    /// The loser had no cards left (owner ruling).
    OutOfCards,
    /// The loser gave up (Cancel).
    Forfeit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// A card from the hand onto a caravan: a numbered card on the end of one of the
    /// mover's own (`on` is `None`); a face card on the card `on` of any caravan.
    Play { card: usize, side: usize, track: usize, on: Option<usize> },
    /// A card from the hand thrown away; another drawn.
    Discard { card: usize },
    /// Every card of one of the mover's caravans thrown away.
    DiscardTrack { track: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Illegal {
    Over,
    NotYourTurn,
    NoSuchCard,
    NoSuchTarget,
    /// Numbered cards go on the mover's own caravans.
    NotYours,
    /// The opening allows numbered cards on empty caravans, and discards.
    Opening,
    /// The caravan does not take that card (direction, suit, a repeated value).
    Order,
    /// A Queen goes on a caravan's last card.
    QueenNotLast,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Played { by: usize, card: Card, side: usize, track: usize },
    /// Cards that left the table (a Jack's, a Joker's).
    Removed { side: usize, track: usize, cards: Vec<Card> },
    Drew { side: usize, card: Card },
    Discarded { side: usize, card: Card },
    TrackDiscarded { side: usize, track: usize, cards: Vec<Card> },
    OpeningOver,
    Over { winner: usize, end: End },
}

#[derive(Debug, Clone)]
pub struct Game {
    pub sides: [Side; 2],
    pub turn: usize,
    pub phase: Phase,
    pub winner: Option<(usize, End)>,
    pub moves: u32,
}

impl Game {
    /// Both decks shuffled and 8 cards drawn each; the opponent goes first [observed].
    pub fn new(player: Vec<Card>, opponent: Vec<Card>, rng: &mut ChaCha8Rng) -> Game {
        let p = Side::new(player, rng);
        let o = Side::new(opponent, rng);
        Game { sides: [p, o], turn: OPPONENT, phase: Phase::Opening, winner: None, moves: 0 }
    }

    pub fn other(side: usize) -> usize {
        1 - side
    }

    /// The track's winner: the side whose caravan is sold and higher, or sold alone; none
    /// while neither is sold or both are at the same value.
    pub fn track_winner(&self, track: usize) -> Option<usize> {
        let (a, b) = (&self.sides[PLAYER].caravans[track], &self.sides[OPPONENT].caravans[track]);
        match (a.sold(), b.sold()) {
            (true, true) if a.value() > b.value() => Some(PLAYER),
            (true, true) if b.value() > a.value() => Some(OPPONENT),
            (true, false) => Some(PLAYER),
            (false, true) => Some(OPPONENT),
            _ => None,
        }
    }

    pub fn legal(&self, by: usize, m: Move) -> Result<(), Illegal> {
        if self.phase == Phase::Over {
            return Err(Illegal::Over);
        }
        if by != self.turn {
            return Err(Illegal::NotYourTurn);
        }
        let me = &self.sides[by];
        match m {
            Move::Discard { card } => me.hand.get(card).map(|_| ()).ok_or(Illegal::NoSuchCard),
            Move::DiscardTrack { track } => {
                if self.phase == Phase::Opening {
                    return Err(Illegal::Opening);
                }
                match me.caravans.get(track) {
                    Some(c) if !c.is_empty() => Ok(()),
                    _ => Err(Illegal::NoSuchTarget),
                }
            }
            Move::Play { card, side, track, on } => {
                let c = *me.hand.get(card).ok_or(Illegal::NoSuchCard)?;
                let target = self.sides.get(side).and_then(|s| s.caravans.get(track)).ok_or(Illegal::NoSuchTarget)?;
                if c.rank.is_face() {
                    if self.phase == Phase::Opening {
                        return Err(Illegal::Opening);
                    }
                    let i = on.ok_or(Illegal::NoSuchTarget)?;
                    if i >= target.cards.len() {
                        return Err(Illegal::NoSuchTarget);
                    }
                    if c.rank == Rank::Queen && i + 1 != target.cards.len() {
                        return Err(Illegal::QueenNotLast);
                    }
                    return Ok(());
                }
                if side != by {
                    return Err(Illegal::NotYours);
                }
                if self.phase == Phase::Opening {
                    return if target.is_empty() { Ok(()) } else { Err(Illegal::Opening) };
                }
                if target.accepts(c) {
                    Ok(())
                } else {
                    Err(Illegal::Order)
                }
            }
        }
    }

    /// Every legal move for the side to move.
    pub fn moves_for(&self, by: usize) -> Vec<Move> {
        let mut out = Vec::new();
        if self.phase == Phase::Over || by != self.turn {
            return out;
        }
        let me = &self.sides[by];
        for (i, c) in me.hand.iter().enumerate() {
            if c.rank.is_face() {
                for (side, s) in self.sides.iter().enumerate() {
                    for (track, pile) in s.caravans.iter().enumerate() {
                        for on in 0..pile.cards.len() {
                            out.push(Move::Play { card: i, side, track, on: Some(on) });
                        }
                    }
                }
            } else {
                for track in 0..TRACKS {
                    out.push(Move::Play { card: i, side: by, track, on: None });
                }
            }
            out.push(Move::Discard { card: i });
        }
        for track in 0..TRACKS {
            out.push(Move::DiscardTrack { track });
        }
        out.retain(|&m| self.legal(by, m).is_ok());
        out
    }

    pub fn apply(&mut self, by: usize, m: Move) -> Result<Vec<Event>, Illegal> {
        self.legal(by, m)?;
        let mut ev = Vec::new();
        let opening = self.phase == Phase::Opening;
        match m {
            Move::Discard { card } => {
                let c = self.sides[by].hand.remove(card);
                ev.push(Event::Discarded { side: by, card: c });
                self.draw(by, &mut ev);
            }
            Move::DiscardTrack { track } => {
                let pile = std::mem::take(&mut self.sides[by].caravans[track]);
                ev.push(Event::TrackDiscarded { side: by, track, cards: pile.all_cards().copied().collect() });
            }
            Move::Play { card, side, track, on } => {
                let c = self.sides[by].hand.remove(card);
                ev.push(Event::Played { by, card: c, side, track });
                match (c.rank, on) {
                    (Rank::Num(_), _) => self.sides[side].caravans[track].cards.push(Placed { card: c, faces: Vec::new() }),
                    (Rank::Jack, Some(i)) => {
                        let gone = self.sides[side].caravans[track].cards.remove(i);
                        let mut cards = vec![gone.card];
                        cards.extend(gone.faces);
                        cards.push(c);
                        ev.push(Event::Removed { side, track, cards });
                    }
                    (Rank::Joker, Some(i)) => {
                        self.sides[side].caravans[track].cards[i].faces.push(c);
                        self.joker((side, track, i), &mut ev);
                    }
                    (_, Some(i)) => self.sides[side].caravans[track].cards[i].faces.push(c),
                    _ => {}
                }
                if !opening {
                    self.draw(by, &mut ev);
                }
            }
        }
        self.moves += 1;
        if opening && self.sides.iter().all(Side::started) {
            self.phase = Phase::War;
            ev.push(Event::OpeningOver);
        }
        if self.phase == Phase::War {
            let won: Vec<Option<usize>> = (0..TRACKS).map(|t| self.track_winner(t)).collect();
            if won.iter().all(Option::is_some) {
                let player = won.iter().filter(|w| **w == Some(PLAYER)).count();
                let winner = if player >= 2 { PLAYER } else { OPPONENT };
                self.finish(winner, End::Sold, &mut ev);
                return Ok(ev);
            }
        }
        self.pass_turn(by, &mut ev);
        Ok(ev)
    }

    /// The side gives up.
    pub fn forfeit(&mut self, side: usize) -> Vec<Event> {
        let mut ev = Vec::new();
        if self.phase != Phase::Over {
            self.finish(Game::other(side), End::Forfeit, &mut ev);
        }
        ev
    }

    fn finish(&mut self, winner: usize, end: End, ev: &mut Vec<Event>) {
        self.phase = Phase::Over;
        self.winner = Some((winner, end));
        ev.push(Event::Over { winner, end });
    }

    /// The other side moves, unless in the opening it has started its three caravans and
    /// this side has not; a side with no cards left loses (owner ruling).
    fn pass_turn(&mut self, by: usize, ev: &mut Vec<Event>) {
        let next = Game::other(by);
        self.turn = if self.phase == Phase::Opening && self.sides[next].started() && !self.sides[by].started() { by } else { next };
        let t = self.turn;
        if self.sides[t].hand.is_empty() {
            self.finish(Game::other(t), End::OutOfCards, ev);
        }
    }

    fn draw(&mut self, side: usize, ev: &mut Vec<Event>) {
        if let Some(card) = self.sides[side].draw() {
            ev.push(Event::Drew { side, card });
        }
    }

    /// Preview all six caravan values and track winners after applying `m` for `by`
    /// under `bPreviewMoveResult = 1` [documented: Better Caravan v1.9
    /// (`https://www.nexusmods.com/newvegas/mods/81002`), shipped
    /// `Data\NVSE\Plugins\BetterCaravan.ini` comment:
    /// `"; show the resulting caravan values when hovering over a move"`; plugin DLL and
    /// source code were never consulted], implemented via NVForever's own non-mutating
    /// clone-preview helper (`preview_move`) without mutating `self` [design: NVForever's
    /// non-mutating clone-preview helper, not Better Caravan's implementation].
    pub fn preview_move(&self, by: usize, m: Move) -> Option<MovePreview> {
        let mut after = self.clone();
        after.apply(by, m).ok()?;
        let values = [
            std::array::from_fn(|t| after.sides[PLAYER].caravans[t].value()),
            std::array::from_fn(|t| after.sides[OPPONENT].caravans[t].value()),
        ];
        let winners = std::array::from_fn(|t| after.track_winner(t));
        Some(MovePreview { values, winners })
    }

    /// A Joker on the card at `at` (side, track, place): on an ace every other numbered
    /// card of its suit, on 2–10 every other card of its value, leaves the table with its
    /// face cards (caravan.md §5).
    fn joker(&mut self, at: (usize, usize, usize), ev: &mut Vec<Event>) {
        let target = self.sides[at.0].caravans[at.1].cards[at.2].card;
        let hit = |c: &Card| match target.rank {
            Rank::Num(1) => c.suit == target.suit,
            r => c.rank == r,
        };
        for (side, s) in self.sides.iter_mut().enumerate() {
            for (track, caravan) in s.caravans.iter_mut().enumerate() {
                let pile = std::mem::take(&mut caravan.cards);
                let mut gone = Vec::new();
                let mut kept = Vec::with_capacity(pile.len());
                for (i, p) in pile.into_iter().enumerate() {
                    if (side, track, i) != at && hit(&p.card) {
                        gone.push(p.card);
                        gone.extend(p.faces);
                    } else {
                        kept.push(p);
                    }
                }
                caravan.cards = kept;
                if !gone.is_empty() {
                    ev.push(Event::Removed { side, track, cards: gone });
                }
            }
        }
    }
}

/// How well an opponent plays, from 0 to 1, by the share of face cards in its deck: 10 %
/// or fewer → 0, 35 % or more → 1 [design; owner ruling 2026-10-19; caravan.md §6].
pub fn skill(deck: &[Card]) -> f32 {
    if deck.is_empty() {
        return 0.0;
    }
    let faces = deck.iter().filter(|c| c.rank.is_face()).count() as f32;
    ((faces / deck.len() as f32 - 0.10) / 0.25).clamp(0.0, 1.0)
}

/// How good the position is for `side` [design].
pub fn evaluate(g: &Game, side: usize) -> f32 {
    if let Some((w, _)) = g.winner {
        return if w == side { 1.0e6 } else { -1.0e6 };
    }
    let other = Game::other(side);
    let mut s = 0.0;
    for t in 0..TRACKS {
        let (mine, theirs) = (&g.sides[side].caravans[t], &g.sides[other].caravans[t]);
        let (o, th) = (mine.value(), theirs.value());
        if mine.sold() && (!theirs.sold() || o > th) {
            s += 100.0 + o as f32;
        } else if mine.sold() && theirs.sold() && o == th {
            s += 40.0;
        } else if mine.sold() {
            s += 30.0;
        } else if mine.over() {
            s -= 30.0 + 4.0 * (o - SOLD_MAX) as f32;
        } else {
            s += 2.0 * o as f32;
        }
        if theirs.sold() && (!mine.sold() || th > o) {
            s -= 60.0;
        }
    }
    s + 0.5 * g.sides[side].cards_left() as f32
}

/// Resulting caravan values and track winners from hovering/previewing a legal move
/// under `bPreviewMoveResult = 1` [documented: Better Caravan v1.9
/// (`https://www.nexusmods.com/newvegas/mods/81002`), shipped
/// `Data\NVSE\Plugins\BetterCaravan.ini` comment:
/// `"; show the resulting caravan values when hovering over a move"`; plugin DLL and
/// source code were never consulted]; this `MovePreview` Rust struct representation is
/// NVForever's own helper data structure [design: NVForever's `MovePreview` Rust struct
/// representation].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovePreview {
    /// Resulting caravan values `[side][track]`.
    pub values: [[u32; TRACKS]; 2],
    /// Resulting track winner per track (`Some(PLAYER)`, `Some(OPPONENT)`, or `None`).
    pub winners: [Option<usize>; TRACKS],
}

/// How `iModCardMode` modifies the player's deck at game start when `iModCardCount > 0`
/// [documented: Better Caravan v1.9 (`https://www.nexusmods.com/newvegas/mods/81002`),
/// shipped `Data\NVSE\Plugins\BetterCaravan.ini` comments: `0 = add`, `1 = remove`,
/// `2 = replace`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ModCardMode {
    #[default]
    Add,
    Remove,
    Replace,
}

impl ModCardMode {
    fn from_ini(n: i64) -> Self {
        match n {
            1 => Self::Remove,
            2 => Self::Replace,
            _ => Self::Add,
        }
    }
}

/// Clean-room configuration parsed from `Data\NVSE\Plugins\BetterCaravan.ini`
/// [documented: Better Caravan v1.9 (`https://www.nexusmods.com/newvegas/mods/81002`),
/// shipped `Data\NVSE\Plugins\BetterCaravan.ini`, section `[Better Caravan]` and its 16
/// commented keys; plugin DLL and source code were never consulted], with an NVForever
/// fallback lookup at `Data\Config\BetterCaravan.ini` [design: NVForever fallback path,
/// not stated in Better Caravan v1.9].
///
/// Note that `bAiFixes` and `bImprovedAI` drive NVForever's own clean-room scoring
/// heuristics in [`choose_with_config`], not Better Caravan's AI implementation [design].
///
/// When neither INI file exists, every option defaults to off (`false` / `0` / `None`) so
/// unmodded vanilla Caravan rules, visuals, bindings, and AI remain unchanged [design].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BetterCaravanConfig {
    pub ai_fixes: bool,
    pub improved_ai: bool,
    pub preview_move_result: bool,
    pub highlight_winning_caravans: bool,
    pub mod_card_mode: ModCardMode,
    pub mod_card_count: usize,
    pub debug: bool,
    pub view_npc_cards: bool,
    pub description_jack: Option<String>,
    pub description_queen: Option<String>,
    pub description_king: Option<String>,
    pub description_joker: Option<String>,
    pub key_up: Option<String>,
    pub key_down: Option<String>,
    pub key_left: Option<String>,
    pub key_right: Option<String>,
}

/// VFS lookup paths for `BetterCaravan.ini` in priority order:
/// `NVSE\Plugins\BetterCaravan.ini` [documented: Better Caravan v1.9
/// (`https://www.nexusmods.com/newvegas/mods/81002`), archive install path] followed by
/// `Config\BetterCaravan.ini` [design: NVForever fallback lookup path, not in Better
/// Caravan v1.9's shipped layout].
pub const BETTER_CARAVAN_INI_PATHS: [&str; 2] = [
    "NVSE\\Plugins\\BetterCaravan.ini",
    "Config\\BetterCaravan.ini",
];

impl BetterCaravanConfig {
    /// Load `BetterCaravan.ini` from `g.mod_files` / Data VFS (`NVSE\Plugins\BetterCaravan.ini`,
    /// falling back to `Config\BetterCaravan.ini`). Returns `(Default, None)` when absent.
    pub fn load_from_game(g: &Rules) -> (Self, Option<&'static str>) {
        Self::load_from_mod_files(&g.mod_files)
    }

    /// Load `BetterCaravan.ini` from `mod_files` / Data VFS in priority order.
    pub fn load_from_mod_files(mod_files: &crate::mod_files::ModFiles) -> (Self, Option<&'static str>) {
        for path in BETTER_CARAVAN_INI_PATHS {
            if let Some(text) = mod_files.read_text(path) {
                return (Self::parse(&text), Some(path));
            }
            if let Some(bytes) = crate::mod_files::read_data_file(path) {
                return (Self::parse_bytes(&bytes), Some(path));
            }
        }
        (Self::default(), None)
    }

    /// Parse raw INI bytes (strips UTF-8 BOM if present; falls back to Latin-1 for non-UTF-8
    /// bytes) [design].
    pub fn parse_bytes(bytes: &[u8]) -> Self {
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        match std::str::from_utf8(bytes) {
            Ok(s) => Self::parse(s),
            Err(_) => {
                let text: String = bytes.iter().map(|&b| b as char).collect();
                Self::parse(&text)
            }
        }
    }

    /// Parse `[Better Caravan]` INI text (BOM-safe, case-insensitive section and keys,
    /// `;` and `#` comment lines ignored) [design].
    pub fn parse(text: &str) -> Self {
        let decoded = decode_ini_bom(text);
        let mut cfg = Self::default();
        let mut in_section = false;
        for raw_line in decoded.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix('[') {
                if let Some((sec, _)) = rest.split_once(']') {
                    in_section = sec.trim().eq_ignore_ascii_case("better caravan");
                }
                continue;
            }
            if !in_section {
                continue;
            }
            let Some((raw_key, raw_val)) = line.split_once('=') else {
                continue;
            };
            let key = raw_key.trim().to_ascii_lowercase();
            match key.as_str() {
                "baifixes" => {
                    if let Some(b) = parse_ini_bool(raw_val) {
                        cfg.ai_fixes = b;
                    }
                }
                "bimprovedai" => {
                    if let Some(b) = parse_ini_bool(raw_val) {
                        cfg.improved_ai = b;
                    }
                }
                "bpreviewmoveresult" => {
                    if let Some(b) = parse_ini_bool(raw_val) {
                        cfg.preview_move_result = b;
                    }
                }
                "bhighlightwinningcaravans" => {
                    if let Some(b) = parse_ini_bool(raw_val) {
                        cfg.highlight_winning_caravans = b;
                    }
                }
                "imodcardmode" => {
                    if let Some(n) = parse_ini_int(raw_val) {
                        cfg.mod_card_mode = ModCardMode::from_ini(n);
                    }
                }
                "imodcardcount" => {
                    if let Some(n) = parse_ini_int(raw_val) {
                        cfg.mod_card_count = n.max(0) as usize;
                    }
                }
                "bdebug" => {
                    if let Some(b) = parse_ini_bool(raw_val) {
                        cfg.debug = b;
                    }
                }
                "bviewnpccards" => {
                    if let Some(b) = parse_ini_bool(raw_val) {
                        cfg.view_npc_cards = b;
                    }
                }
                "sdescriptionjack" => cfg.description_jack = parse_ini_string(raw_val),
                "sdescriptionqueen" => cfg.description_queen = parse_ini_string(raw_val),
                "sdescriptionking" => cfg.description_king = parse_ini_string(raw_val),
                "sdescriptionjoker" => cfg.description_joker = parse_ini_string(raw_val),
                "skeyup" => cfg.key_up = parse_ini_token(raw_val),
                "skeydown" => cfg.key_down = parse_ini_token(raw_val),
                "skeyleft" => cfg.key_left = parse_ini_token(raw_val),
                "skeyright" => cfg.key_right = parse_ini_token(raw_val),
                _ => {}
            }
        }
        cfg
    }

    /// Face-card reminder configured for `rank`, if any.
    pub fn face_description(&self, rank: Rank) -> Option<&str> {
        match rank {
            Rank::Jack => self.description_jack.as_deref(),
            Rank::Queen => self.description_queen.as_deref(),
            Rank::King => self.description_king.as_deref(),
            Rank::Joker => self.description_joker.as_deref(),
            Rank::Num(_) => None,
        }
    }
}

fn decode_ini_bom(text: &str) -> std::borrow::Cow<'_, str> {
    if let Some(rest) = text.strip_prefix('\u{feff}') {
        return std::borrow::Cow::Borrowed(rest);
    }
    // `ModFiles::read_text` decodes raw bytes as Latin-1, so a UTF-8 BOM arrives as three
    // Latin-1 code points (`\u{00EF}\u{00BB}\u{00BF}`).
    if let Some(rest) = text.strip_prefix("\u{00ef}\u{00bb}\u{00bf}") {
        if rest.chars().all(|c| (c as u32) <= 0xFF) {
            let bytes: Vec<u8> = rest.chars().map(|c| c as u8).collect();
            if let Ok(utf8) = String::from_utf8(bytes) {
                return std::borrow::Cow::Owned(utf8);
            }
        }
        return std::borrow::Cow::Borrowed(rest);
    }
    std::borrow::Cow::Borrowed(text)
}

fn strip_inline_comment(val: &str) -> &str {
    val.split([';', '#']).next().unwrap_or(val).trim()
}

fn parse_ini_bool(raw: &str) -> Option<bool> {
    let v = strip_inline_comment(raw).trim_matches('"').trim();
    if v.is_empty() {
        return None;
    }
    if v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes") || v.eq_ignore_ascii_case("on") {
        return Some(true);
    }
    if v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("no") || v.eq_ignore_ascii_case("off") {
        return Some(false);
    }
    v.parse::<f64>().ok().filter(|n| n.is_finite()).map(|n| n != 0.0)
}

fn parse_ini_int(raw: &str) -> Option<i64> {
    let v = strip_inline_comment(raw).trim_matches('"').trim();
    if v.is_empty() {
        return None;
    }
    v.parse::<i64>().ok().or_else(|| v.parse::<f64>().ok().filter(|n| n.is_finite()).map(|n| n.trunc() as i64))
}

fn parse_ini_string(raw: &str) -> Option<String> {
    let v = raw.trim();
    let v = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(v).trim();
    (!v.is_empty()).then(|| v.to_string())
}

fn parse_ini_token(raw: &str) -> Option<String> {
    let v = strip_inline_comment(raw);
    let v = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(v).trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// Randomly modify the player's dealt deck at minigame start according to BetterCaravan's
/// `iModCardMode` and `iModCardCount` [design].
///
/// When `count == 0`, returns `deck` unchanged without advancing `rng`.
/// For [`ModCardMode::Remove`], preserves at least [`MIN_DECK`] cards whenever `deck`
/// starts with at least [`MIN_DECK`] cards (or `1` card for smaller synthetic decks).
pub fn apply_deck_mod(deck: &[FormId], owned: &[FormId], mode: ModCardMode, count: usize, rng: &mut ChaCha8Rng) -> Vec<FormId> {
    let mut out = deck.to_vec();
    if count == 0 {
        return out;
    }
    match mode {
        ModCardMode::Add => {
            let mut pool: Vec<FormId> = owned.iter().copied().filter(|f| !out.contains(f)).collect();
            shuffle(&mut pool, rng);
            let n = count.min(pool.len());
            out.extend(pool.into_iter().take(n));
        }
        ModCardMode::Remove => {
            let floor = if out.len() >= MIN_DECK { MIN_DECK } else { 1.min(out.len()) };
            let removable = out.len().saturating_sub(floor);
            let n = count.min(removable);
            for _ in 0..n {
                let idx = (rng.next_u32() as usize) % out.len();
                out.remove(idx);
            }
        }
        ModCardMode::Replace => {
            let mut pool: Vec<FormId> = owned.iter().copied().filter(|f| !out.contains(f)).collect();
            shuffle(&mut pool, rng);
            let n = count.min(out.len()).min(pool.len());
            let mut indices: Vec<usize> = (0..out.len()).collect();
            shuffle(&mut indices, rng);
            for (slot, replacement) in indices.into_iter().take(n).zip(pool.into_iter().take(n)) {
                out[slot] = replacement;
            }
        }
    }
    out
}

/// The side's choice: the best move by [`evaluate`] after it, with noise and fewer attacks
/// the lower the skill [design; caravan.md §6]. `None` when the side cannot move.
pub fn choose(g: &Game, side: usize, skill: f32, rng: &mut ChaCha8Rng) -> Option<Move> {
    let skill = skill.clamp(0.0, 1.0);
    let mut best: Option<(f32, Move)> = None;
    for m in g.moves_for(side) {
        let mut after = g.clone();
        if after.apply(side, m).is_err() {
            continue;
        }
        let noise = (rng.next_u32() as f32 / u32::MAX as f32 * 2.0 - 1.0) * 40.0 * (1.0 - skill);
        let attack = matches!(m, Move::Play { side: s, on: Some(_), .. } if s != side);
        let mut score = evaluate(&after, side) + noise;
        if attack {
            score -= 25.0 * (1.0 - skill);
        }
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, m));
        }
    }
    best.map(|(_, m)| m)
}

/// Choose a move honoring [`BetterCaravanConfig::ai_fixes`] and
/// [`BetterCaravanConfig::improved_ai`] [design: NVForever's own scoring heuristics, not
/// Better Caravan's AI implementation; caravan.md §9].
///
/// When both flags are off, delegates directly to [`choose`] so vanilla AI decisions and
/// RNG draws remain bit-for-bit unchanged.
pub fn choose_with_config(g: &Game, side: usize, skill: f32, cfg: &BetterCaravanConfig, rng: &mut ChaCha8Rng) -> Option<Move> {
    if !cfg.ai_fixes && !cfg.improved_ai {
        return choose(g, side, skill, rng);
    }
    let skill = skill.clamp(0.0, 1.0);
    let other = Game::other(side);
    let base_my_wins = (0..TRACKS).filter(|&t| g.track_winner(t) == Some(side)).count();
    let base_other_wins = (0..TRACKS).filter(|&t| g.track_winner(t) == Some(other)).count();
    let mut best: Option<(f32, Move)> = None;
    for m in g.moves_for(side) {
        let mut after = g.clone();
        if after.apply(side, m).is_err() {
            continue;
        }
        if after.winner.is_some_and(|(w, _)| w == side) {
            let my_tracks = (0..TRACKS).filter(|&t| after.track_winner(t) == Some(side)).count() as f32;
            let margin: f32 = (0..TRACKS).map(|t| after.sides[side].caravans[t].value() as f32).sum();
            let score = 1.0e6 + my_tracks * 100.0 + margin;
            if best.is_none_or(|(b, _)| score > b) {
                best = Some((score, m));
            }
            continue;
        }
        if after.winner.is_some_and(|(w, _)| w != side) {
            let score = -1.0e6;
            if best.is_none_or(|(b, _)| score > b) {
                best = Some((score, m));
            }
            continue;
        }

        let mut score = evaluate(&after, side);
        if cfg.ai_fixes {
            for t in 0..TRACKS {
                if after.sides[other].caravans[t].over() {
                    score += 20.0;
                }
                if g.track_winner(t) == Some(side) && after.track_winner(t) != Some(side) {
                    score -= 80.0;
                }
            }
            match m {
                Move::DiscardTrack { track } if !g.sides[side].caravans[track].over() => {
                    score -= 45.0;
                }
                Move::Play { side: target_side, track, on: Some(_), .. } if target_side == other => {
                    let before_o = &g.sides[other].caravans[track];
                    let after_o = &after.sides[other].caravans[track];
                    let helped_win = g.track_winner(track) != Some(other) && after.track_winner(track) == Some(other);
                    let unbusted = before_o.over() && !after_o.over();
                    let boosted_in_range = !after_o.over() && after_o.value() > before_o.value() && after_o.value() >= SOLD_MIN;
                    if helped_win || unbusted || boosted_in_range {
                        score -= 90.0;
                    }
                }
                _ => {}
            }
        }
        if cfg.improved_ai {
            let after_my_wins = (0..TRACKS).filter(|&t| after.track_winner(t) == Some(side)).count();
            let after_other_wins = (0..TRACKS).filter(|&t| after.track_winner(t) == Some(other)).count();
            if base_other_wins >= 2 && after_other_wins < base_other_wins {
                score += 160.0;
            }
            if after_other_wins >= 2 {
                score -= 120.0;
            }
            if after_my_wins >= 2 && after_my_wins > base_my_wins {
                score += 90.0;
            }
            for t in 0..TRACKS {
                if after.track_winner(t) == Some(side) {
                    let v = after.sides[side].caravans[t].value();
                    score += 3.0 * v.saturating_sub(SOLD_MIN) as f32;
                }
            }
        }

        let noise_scale = if cfg.improved_ai { 20.0 } else { 40.0 };
        let attack_penalty = if cfg.improved_ai { 10.0 } else { 25.0 };
        let noise = (rng.next_u32() as f32 / u32::MAX as f32 * 2.0 - 1.0) * noise_scale * (1.0 - skill);
        let attack = matches!(m, Move::Play { side: s, on: Some(_), .. } if s != side);
        score += noise;
        if attack {
            score -= attack_penalty * (1.0 - skill);
        }
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, m));
        }
    }
    best.map(|(_, m)| m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::SeedableRng;

    fn c(s: &str) -> Card {
        Card::parse(s).unwrap()
    }

    fn pile(cards: &[&str]) -> Caravan {
        Caravan { cards: cards.iter().map(|s| Placed { card: c(s), faces: Vec::new() }).collect() }
    }

    /// A game in the contract war with hands and caravans as given, the player to move.
    fn war(hand: &[&str], mine: [&[&str]; 3], theirs: [&[&str]; 3]) -> Game {
        let side = |h: &[&str], cs: [&[&str]; 3]| Side {
            deck: (0..20).map(|_| c("2c")).collect(),
            hand: h.iter().map(|s| c(s)).collect(),
            caravans: [pile(cs[0]), pile(cs[1]), pile(cs[2])],
        };
        Game { sides: [side(hand, mine), side(&["9h", "8h", "7h", "6h", "5h"], theirs)], turn: PLAYER, phase: Phase::War, winner: None, moves: 0 }
    }

    #[test]
    fn cards_read_as_the_records_code_them() {
        assert_eq!(Suit::from_code(4), Some(Suit::Clubs));
        assert_eq!(Rank::from_code(10), Some(Rank::Num(10)));
        assert_eq!(Rank::from_code(14), Some(Rank::King));
        assert_eq!(Rank::from_code(11), None);
        assert_eq!(c("10h").rank, Rank::Num(10));
        assert_eq!(c("As").rank, Rank::Num(1));
        assert_eq!(c("Jo").rank, Rank::Joker);
        assert!(Card::parse("11h").is_none());
        assert_eq!(c("Kd").short(), "Kd");
    }

    #[test]
    fn kings_double_and_double_again() {
        let mut p = Placed { card: c("10h"), faces: vec![c("Ks")] };
        assert_eq!(p.value(), 20);
        p.faces.push(c("Kd"));
        assert_eq!(p.value(), 40);
        p.faces.push(c("Qh"));
        assert_eq!(p.value(), 40);
    }

    #[test]
    fn a_caravan_keeps_its_direction_or_its_suit() {
        let k = pile(&["3h", "7c"]);
        assert_eq!(k.direction(), Some(Dir::Up));
        assert_eq!(k.suit(), Some(Suit::Clubs));
        assert!(k.accepts(c("9h")), "up");
        assert!(!k.accepts(c("5h")), "down, another suit");
        assert!(k.accepts(c("5c")), "down, the same suit");
        assert!(!k.accepts(c("7h")), "the same value as the last");
        assert!(!k.accepts(c("7c")), "the same value even in suit");
        assert!(pile(&[]).accepts(c("Ad")));
        assert!(pile(&["5d"]).accepts(c("2s")), "one card sets no direction");
        assert!(!pile(&["5d"]).accepts(c("5s")));
        assert!(!k.accepts(c("Kd")), "face cards are not added to the end");
    }

    #[test]
    fn a_queen_reverses_the_direction_and_changes_the_suit() {
        let mut k = pile(&["3h", "7c"]);
        k.cards[1].faces.push(c("Qd"));
        assert_eq!(k.direction(), Some(Dir::Down));
        assert_eq!(k.suit(), Some(Suit::Diamonds));
        assert!(k.accepts(c("5h")));
        assert!(!k.accepts(c("9h")));
        assert!(k.accepts(c("9d")));
    }

    #[test]
    fn the_opening_lays_one_card_on_each_empty_caravan() {
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let deck: Vec<Card> = (2..=10).flat_map(|n| ["h", "s", "c", "d"].map(|s| c(&format!("{n}{s}")))).collect();
        let mut g = Game::new(deck.clone(), deck, &mut rng);
        assert_eq!(g.turn, OPPONENT, "the opponent goes first");
        assert_eq!(g.sides[PLAYER].hand.len(), HAND);
        assert_eq!(g.legal(PLAYER, Move::Discard { card: 0 }), Err(Illegal::NotYourTurn));
        assert_eq!(g.legal(OPPONENT, Move::DiscardTrack { track: 0 }), Err(Illegal::Opening));
        for _ in 0..3 {
            for by in [OPPONENT, PLAYER] {
                let track = g.sides[by].caravans.iter().position(Caravan::is_empty).unwrap();
                let ev = g.apply(by, Move::Play { card: 0, side: by, track, on: None }).unwrap();
                assert!(!ev.iter().any(|e| matches!(e, Event::Drew { .. })), "laying a card draws none in the opening");
            }
        }
        assert_eq!(g.phase, Phase::War);
        assert_eq!(g.sides[PLAYER].hand.len(), 5);
        let ev = g.apply(OPPONENT, Move::Discard { card: 0 }).unwrap();
        assert!(ev.iter().any(|e| matches!(e, Event::Drew { side: OPPONENT, .. })));
        assert_eq!(g.sides[OPPONENT].hand.len(), 5);
    }

    #[test]
    fn a_discard_in_the_opening_keeps_the_turn_with_the_side_behind() {
        let mut rng = ChaCha8Rng::seed_from_u64(2);
        let deck: Vec<Card> = (2..=10).flat_map(|n| ["h", "s", "c", "d"].map(|s| c(&format!("{n}{s}")))).collect();
        let mut g = Game::new(deck.clone(), deck, &mut rng);
        g.apply(OPPONENT, Move::Discard { card: 0 }).unwrap();
        for i in 0..3 {
            g.apply(PLAYER, Move::Play { card: 0, side: PLAYER, track: i, on: None }).unwrap();
            if i < 2 {
                g.apply(OPPONENT, Move::Play { card: 0, side: OPPONENT, track: i, on: None }).unwrap();
            }
        }
        assert_eq!(g.turn, OPPONENT, "the player has started all three; the opponent catches up");
        g.apply(OPPONENT, Move::Play { card: 0, side: OPPONENT, track: 2, on: None }).unwrap();
        assert_eq!(g.phase, Phase::War);
        assert_eq!(g.turn, PLAYER);
    }

    #[test]
    fn numbered_cards_go_on_ones_own_caravans_and_face_cards_anywhere() {
        let g = war(&["9h", "Kc", "Qs", "Jd"], [&["3h", "7c"], &["10s"], &["2d"]], [&["5c"], &["6h", "8h"], &["Ad"]]);
        assert_eq!(g.legal(PLAYER, Move::Play { card: 0, side: OPPONENT, track: 0, on: None }), Err(Illegal::NotYours));
        assert!(g.legal(PLAYER, Move::Play { card: 0, side: PLAYER, track: 0, on: None }).is_ok());
        assert!(g.legal(PLAYER, Move::Play { card: 1, side: OPPONENT, track: 1, on: Some(0) }).is_ok());
        assert_eq!(g.legal(PLAYER, Move::Play { card: 2, side: OPPONENT, track: 1, on: Some(0) }), Err(Illegal::QueenNotLast));
        assert!(g.legal(PLAYER, Move::Play { card: 2, side: OPPONENT, track: 1, on: Some(1) }).is_ok());
        assert_eq!(g.legal(PLAYER, Move::Play { card: 3, side: OPPONENT, track: 1, on: None }), Err(Illegal::NoSuchTarget));
    }

    #[test]
    fn a_jack_takes_the_card_and_its_face_cards() {
        let mut g = war(&["Jd"], [&["3h"], &["4h"], &["5h"]], [&["6c", "9c"], &["7h"], &["8h"]]);
        g.sides[OPPONENT].caravans[0].cards[1].faces.push(c("Ks"));
        let ev = g.apply(PLAYER, Move::Play { card: 0, side: OPPONENT, track: 0, on: Some(1) }).unwrap();
        assert_eq!(g.sides[OPPONENT].caravans[0].value(), 6);
        assert!(ev.iter().any(|e| matches!(e, Event::Removed { side: OPPONENT, track: 0, cards } if cards.len() == 3)));
    }

    #[test]
    fn a_joker_on_an_ace_clears_its_suit_and_on_a_number_its_value() {
        let mut g = war(&["Jo", "Jo"], [&["Ah", "4h", "7c"], &["4s"], &["9h"]], [&["4d", "5h"], &["7h"], &["8c"]]);
        g.apply(PLAYER, Move::Play { card: 0, side: PLAYER, track: 0, on: Some(0) }).unwrap();
        assert_eq!(g.sides[PLAYER].caravans[0].cards.len(), 2, "4h went, the ace stays, 7c stays");
        assert!(g.sides[PLAYER].caravans[2].is_empty(), "9h went");
        assert_eq!(g.sides[OPPONENT].caravans[0].cards.len(), 1, "5h went");
        assert!(g.sides[OPPONENT].caravans[1].is_empty(), "7h went");
        g.turn = PLAYER;
        g.apply(PLAYER, Move::Play { card: 0, side: PLAYER, track: 1, on: Some(0) }).unwrap();
        assert_eq!(g.sides[PLAYER].caravans[1].cards.len(), 1, "the 4 the joker is on stays");
        assert!(g.sides[OPPONENT].caravans[0].is_empty(), "4d went");
    }

    #[test]
    fn tracks_sell_between_21_and_26_above_the_other_side() {
        let g = war(&["2c"], [&["10h", "9h", "4h"], &["10s", "8s", "5s"], &["10c", "9c"]], [&["10d", "6d", "5d"], &["10h", "8h", "4h", "2h"], &["10d", "9d", "8d"]]);
        assert_eq!(g.track_winner(0), Some(PLAYER), "23 against 21");
        assert_eq!(g.track_winner(1), Some(OPPONENT), "23 against 24");
        assert_eq!(g.track_winner(2), None, "19 against 27: neither sold");
        let tied = war(&["2c"], [&["10h", "9h", "4h"], &["4s"], &["5c"]], [&["10d", "9d", "4d"], &["7h"], &["5d"]]);
        assert_eq!(tied.track_winner(0), None, "23 against 23 sells neither");
    }

    #[test]
    fn the_game_ends_when_every_track_is_sold() {
        let mut g = war(&["2c"], [&["10h", "9h", "4h"], &["10s", "9s", "5s"], &["10c", "9c"]], [&["10d", "6d", "5d"], &["7h"], &["5d"]]);
        let ev = g.apply(PLAYER, Move::Play { card: 0, side: PLAYER, track: 2, on: None }).unwrap();
        assert_eq!(g.winner, Some((PLAYER, End::Sold)));
        assert!(ev.contains(&Event::Over { winner: PLAYER, end: End::Sold }));
    }

    #[test]
    fn a_side_without_cards_loses() {
        let mut g = war(&["9h"], [&["3h"], &["4h"], &["5h"]], [&["6c"], &["7h"], &["8h"]]);
        g.sides[OPPONENT].hand.clear();
        g.sides[OPPONENT].deck.clear();
        g.apply(PLAYER, Move::Discard { card: 0 }).unwrap();
        assert_eq!(g.winner, Some((PLAYER, End::OutOfCards)));
    }

    #[test]
    fn discarding_a_track_takes_all_its_cards() {
        let mut g = war(&["9h"], [&["3h", "8h"], &["4h"], &["5h"]], [&["6c"], &["7h"], &["8h"]]);
        g.apply(PLAYER, Move::DiscardTrack { track: 0 }).unwrap();
        assert!(g.sides[PLAYER].caravans[0].is_empty());
        assert_eq!(g.sides[PLAYER].hand.len(), 1, "no card drawn");
    }

    #[test]
    fn the_bet_follows_the_bid_and_the_funds() {
        let mut b = Bet::new(18, 216, 0.25);
        assert_eq!(b.bid, 54);
        assert!(!b.can_accept());
        assert!(b.auto_match());
        assert_eq!(b.ante, 18, "the player's funds");
        assert!(!b.can_raise());
        assert_eq!(b.pot(), 36);
        let mut b = Bet::new(500, 200, 0.5);
        b.auto_match();
        assert_eq!(b.ante, 100);
        while b.raise() {}
        assert_eq!(b.ante, 150, "half again the bid");
        let b = Bet::new(500, 0, 0.5);
        assert!(b.can_accept(), "nothing to bid: play for nothing");
    }

    #[test]
    fn more_face_cards_make_a_stronger_opponent() {
        let numbers: Vec<Card> = (0..40).map(|_| c("5h")).collect();
        let mut faces = numbers.clone();
        faces.truncate(26);
        faces.extend((0..14).map(|_| c("Ks")));
        assert_eq!(skill(&numbers), 0.0);
        assert!(skill(&faces) > 0.9);
    }

    #[test]
    fn the_opponent_finishes_games() {
        let deck: Vec<Card> = (1..=10).flat_map(|n| ["h", "s", "c", "d"].map(|s| c(&format!("{}{s}", if n == 1 { "A".into() } else { n.to_string() })))).chain(["Kh", "Qs", "Jd", "Jo"].map(c)).collect();
        for seed in 0..20 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let mut g = Game::new(deck.clone(), deck.clone(), &mut rng);
            let mut turns = 0;
            while g.phase != Phase::Over {
                let by = g.turn;
                let m = choose(&g, by, if by == PLAYER { 0.2 } else { 0.9 }, &mut rng).expect("a move");
                g.apply(by, m).unwrap();
                turns += 1;
                assert!(turns < 500, "seed {seed}: the game never ends");
            }
        }
    }

    #[test]
    fn a_random_deck_has_at_least_thirty_cards() {
        let owned: Vec<FormId> = (0..54).map(FormId).collect();
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        for _ in 0..20 {
            let d = randomize(&owned, &mut rng);
            assert!((MIN_DECK..=54).contains(&d.len()));
        }
    }

    #[test]
    fn better_caravan_absent_ini_preserves_exact_vanilla_behavior() {
        let cfg = BetterCaravanConfig::default();
        assert_eq!(cfg, BetterCaravanConfig::parse(""));
        assert!(!cfg.ai_fixes && !cfg.improved_ai && !cfg.preview_move_result && !cfg.highlight_winning_caravans && !cfg.debug && !cfg.view_npc_cards);
        assert_eq!(cfg.mod_card_count, 0);
        assert_eq!(cfg.face_description(Rank::King), None);

        let empty_files = crate::mod_files::ModFiles::default();
        assert_eq!(BetterCaravanConfig::load_from_mod_files(&empty_files), (BetterCaravanConfig::default(), None));

        // Deck mod with count 0 does not alter deck or advance RNG.
        let deck: Vec<FormId> = (1..=30).map(FormId).collect();
        let owned: Vec<FormId> = (1..=50).map(FormId).collect();
        let mut rng_a = ChaCha8Rng::seed_from_u64(77);
        let mut rng_b = ChaCha8Rng::seed_from_u64(77);
        assert_eq!(apply_deck_mod(&deck, &owned, ModCardMode::Replace, 0, &mut rng_a), deck);
        assert_eq!(rng_a.next_u32(), rng_b.next_u32());

        // AI with both flags off matches vanilla `choose` and RNG stream bit-for-bit.
        let game = war(&["9h", "Kc", "Jd", "5s"], [&["10h", "8h"], &["6s"], &["4c"]], [&["10d", "9d", "5d"], &["8c", "7c"], &["9s"]]);
        for seed in 0..10 {
            let mut r1 = ChaCha8Rng::seed_from_u64(seed);
            let mut r2 = ChaCha8Rng::seed_from_u64(seed);
            let m1 = choose(&game, PLAYER, 0.3, &mut r1);
            let m2 = choose_with_config(&game, PLAYER, 0.3, &cfg, &mut r2);
            assert_eq!(m1, m2);
            assert_eq!(r1.next_u32(), r2.next_u32());
        }
    }

    #[test]
    fn better_caravan_ini_parses_bom_comments_case_and_all_keys() {
        let raw = "\u{feff}; header comment\r\n\
            [Other Section]\r\n\
            bAiFixes = 0\r\n\
            # ignored comment\r\n\
            [bEtTeR cArAvAn] ; trailing comment\r\n\
            baifixes = 1 ; inline comment\r\n\
            BImprovedAI = true\r\n\
            bPreviewMoveResult = 1\r\n\
            bHighlightWinningCaravans = 1\r\n\
            iModCardMode = 2 # replace\r\n\
            iModCardCount = 4\r\n\
            bDebug = 1\r\n\
            bViewNpcCards = 1\r\n\
            sDescriptionJack = Jack - Removes card and attached faces.\r\n\
            sDescriptionQueen = Queen - Reverses direction and suit.\r\n\
            sDescriptionKing = King - Doubles value e.g. 4 + two kings = 16.\r\n\
            sDescriptionJoker = Joker - Clears value, or suit on Ace.\r\n\
            sKeyUp = i ; up\r\n\
            sKeyDown = K\r\n\
            sKeyLeft = j\r\n\
            sKeyRight = l\r\n";

        let cfg = BetterCaravanConfig::parse(raw);
        assert!(cfg.ai_fixes);
        assert!(cfg.improved_ai);
        assert!(cfg.preview_move_result);
        assert!(cfg.highlight_winning_caravans);
        assert_eq!(cfg.mod_card_mode, ModCardMode::Replace);
        assert_eq!(cfg.mod_card_count, 4);
        assert!(cfg.debug);
        assert!(cfg.view_npc_cards);
        assert_eq!(cfg.face_description(Rank::Jack), Some("Jack - Removes card and attached faces."));
        assert_eq!(cfg.face_description(Rank::Queen), Some("Queen - Reverses direction and suit."));
        assert_eq!(cfg.face_description(Rank::King), Some("King - Doubles value e.g. 4 + two kings = 16."));
        assert_eq!(cfg.face_description(Rank::Joker), Some("Joker - Clears value, or suit on Ace."));
        assert_eq!(cfg.face_description(Rank::Num(7)), None);
        assert_eq!(cfg.key_up.as_deref(), Some("i"));
        assert_eq!(cfg.key_down.as_deref(), Some("K"));
        assert_eq!(cfg.key_left.as_deref(), Some("j"));
        assert_eq!(cfg.key_right.as_deref(), Some("l"));

        // Raw UTF-8 BOM bytes parse identically.
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(raw.trim_start_matches('\u{feff}').as_bytes());
        assert_eq!(BetterCaravanConfig::parse_bytes(&bytes), cfg);

        // Negative count clamps to 0; invalid mode falls back to Add; non-finite floats
        // (`NaN`/`inf`) are ignored; empty value clears earlier key.
        let edge = "[Better Caravan]\nbAiFixes = NaN\nbImprovedAI = inf\niModCardMode = 99\niModCardCount = -5\niModCardCount = inf\nsKeyUp = i\nsKeyUp =\n";
        let edge_cfg = BetterCaravanConfig::parse(edge);
        assert!(!edge_cfg.ai_fixes);
        assert!(!edge_cfg.improved_ai);
        assert_eq!(edge_cfg.mod_card_mode, ModCardMode::Add);
        assert_eq!(edge_cfg.mod_card_count, 0);
        assert_eq!(edge_cfg.key_up, None);
    }

    #[test]
    fn better_caravan_load_from_game_prefers_nvse_plugins_over_config_fallback() {
        let bytes = crate::common::tes4();
        let plugins = [fnv_esm::Plugin::parse(&bytes, "BetterCaravanTest.esm".into()).unwrap()];
        let lo = fnv_esm::LoadOrder::new(&plugins).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        let schema = fnv_esm::Schema::load_dir(root.join("fnv")).unwrap();
        let path = root.join("script/functions.json");
        let catalog = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let forms = fnv_sim::FormStore::build(&lo, &schema);
        let refs = fnv_sim::RefTable::build(&lo, &schema);
        let data = crate::GameData::build(&lo, &schema, fnv_script::FunctionTable::load(&path).unwrap(), &catalog, &refs);
        let mut g = crate::game::Game::new(&data, fnv_sim::World::new(&forms, &refs, Default::default()), Default::default());
        assert_eq!(BetterCaravanConfig::load_from_game(&g), (BetterCaravanConfig::default(), None));

        // Config fallback works when NVSE\Plugins copy is absent (including UTF-8 BOM bytes).
        let mut config_bytes = vec![0xEF, 0xBB, 0xBF];
        config_bytes.extend_from_slice(b"[Better Caravan]\nbHighlightWinningCaravans = 1\nsKeyUp = i\n");
        assert!(g.mod_files.write_bytes("Config/BetterCaravan.ini", &config_bytes));
        let (cfg, src) = BetterCaravanConfig::load_from_game(&g);
        assert_eq!(src, Some("Config\\BetterCaravan.ini"));
        assert!(cfg.highlight_winning_caravans);
        assert!(!cfg.preview_move_result);
        assert_eq!(cfg.key_up.as_deref(), Some("i"));

        // NVSE\Plugins\BetterCaravan.ini takes precedence over Config fallback.
        assert!(g.mod_files.write_bytes("NVSE/Plugins/BetterCaravan.ini", b"[Better Caravan]\nbPreviewMoveResult = 1\nbHighlightWinningCaravans = 0\n"));
        let (cfg, src) = BetterCaravanConfig::load_from_game(&g);
        assert_eq!(src, Some("NVSE\\Plugins\\BetterCaravan.ini"));
        assert!(cfg.preview_move_result);
        assert!(!cfg.highlight_winning_caravans);
    }

    #[test]
    fn better_caravan_deck_mod_add_remove_and_replace() {
        let owned: Vec<FormId> = (1..=40).map(FormId).collect();
        let base_34: Vec<FormId> = (1..=34).map(FormId).collect();
        let mut rng = ChaCha8Rng::seed_from_u64(19);

        // Add: adds 4 distinct cards not already in deck.
        let added = apply_deck_mod(&base_34, &owned, ModCardMode::Add, 4, &mut rng);
        assert_eq!(added.len(), 38);
        assert!(added.iter().skip(34).all(|f| (35..=40).contains(&f.0)));

        // Add beyond pool size stops at owned count without duplicates.
        let added_all = apply_deck_mod(&base_34, &owned, ModCardMode::Add, 99, &mut rng);
        assert_eq!(added_all.len(), 40);

        // Remove: removes up to count, never dropping below MIN_DECK (30).
        let removed_2 = apply_deck_mod(&base_34, &owned, ModCardMode::Remove, 2, &mut rng);
        assert_eq!(removed_2.len(), 32);
        let removed_floor = apply_deck_mod(&base_34, &owned, ModCardMode::Remove, 10, &mut rng);
        assert_eq!(removed_floor.len(), MIN_DECK);

        // Replace: preserves deck length and swaps cards from unused owned pool.
        let replaced = apply_deck_mod(&base_34, &owned, ModCardMode::Replace, 3, &mut rng);
        assert_eq!(replaced.len(), base_34.len());
        let new_cards = replaced.iter().filter(|f| !base_34.contains(f)).count();
        assert_eq!(new_cards, 3);
    }

    #[test]
    fn better_caravan_preview_move_handles_numbers_kings_jacks_jokers_and_discard_track() {
        // Numbered card selling track 0 and winning over opponent's 21.
        let g = war(&["6h", "Ks", "Jd", "Jo"], [&["10h", "8h"], &["4s", "7c"], &["5d"]], [&["10d", "6d", "5d"], &["10c", "9c", "4c"], &["7s", "8s"]]);
        let before = g.clone();
        let p_num = g.preview_move(PLAYER, Move::Play { card: 0, side: PLAYER, track: 0, on: None }).unwrap();
        assert_eq!(p_num.values[PLAYER][0], 24);
        assert_eq!(p_num.winners[0], Some(PLAYER));
        assert_eq!(g.sides, before.sides, "preview must not mutate the game");

        // King on opponent's 10c (track 1: 10+9+4=23 -> 20+9+4=33 bust!).
        let p_king = g.preview_move(PLAYER, Move::Play { card: 1, side: OPPONENT, track: 1, on: Some(0) }).unwrap();
        assert_eq!(p_king.values[OPPONENT][1], 33);
        assert_eq!(p_king.winners[1], None);

        // Jack on opponent's 10d (track 0: 10+6+5=21 -> 11).
        let p_jack = g.preview_move(PLAYER, Move::Play { card: 2, side: OPPONENT, track: 0, on: Some(0) }).unwrap();
        assert_eq!(p_jack.values[OPPONENT][0], 11);
        assert_eq!(p_jack.winners[0], None);

        // Joker on player's 7c (track 1) removes opponent's 7s on track 2!
        let p_joker = g.preview_move(PLAYER, Move::Play { card: 3, side: PLAYER, track: 1, on: Some(1) }).unwrap();
        assert_eq!(p_joker.values[PLAYER][1], 11);
        assert_eq!(p_joker.values[OPPONENT][2], 8);

        // DiscardTrack zeroes the chosen track.
        let p_track = g.preview_move(PLAYER, Move::DiscardTrack { track: 0 }).unwrap();
        assert_eq!(p_track.values[PLAYER][0], 0);
    }

    #[test]
    fn better_caravan_ai_fixes_and_improved_ai_detect_wins_and_threats() {
        // Immediate win: OPPONENT already wins tracks 0 & 1 (24 & 25), track 2 is at 18 vs 10.
        // Playing 5h on track 2 wins immediately even at skill 0.0.
        let mut g_win = war(&["2c"], [&["10d", "9d"], &["8d", "7d"], &["10c"]], [&["10h", "9h", "5h"], &["10s", "9s", "6s"], &["10c", "8c"]]);
        g_win.turn = OPPONENT;
        g_win.sides[OPPONENT].hand = vec![c("5c"), c("2h"), c("3d")];
        let cfg_fix = BetterCaravanConfig { ai_fixes: true, ..Default::default() };
        for seed in 0..8 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let m = choose_with_config(&g_win, OPPONENT, 0.0, &cfg_fix, &mut rng).unwrap();
            let mut after = g_win.clone();
            after.apply(OPPONENT, m).unwrap();
            assert_eq!(after.winner, Some((OPPONENT, End::Sold)), "seed {seed}: ai_fixes must always take immediate win");
        }

        // Threat detection: PLAYER holds 2 winning tracks (24 and 25). OPPONENT has a King
        // that can bust PLAYER's 10h on track 0 (24 -> 34) vs weak plays.
        let mut g_threat = war(&["2c"], [&["10h", "9h", "5h"], &["10s", "9s", "6s"], &["6c"]], [&["5d"], &["4d"], &["7d"]]);
        g_threat.turn = OPPONENT;
        g_threat.sides[OPPONENT].hand = vec![c("Kc"), c("2h")];
        let cfg_imp = BetterCaravanConfig { ai_fixes: true, improved_ai: true, ..Default::default() };
        for seed in 0..8 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let m = choose_with_config(&g_threat, OPPONENT, 0.0, &cfg_imp, &mut rng).unwrap();
            assert!(
                matches!(m, Move::Play { card: 0, side: PLAYER, track: 0 | 1, on: Some(_) }),
                "seed {seed}: improved_ai should disrupt player's 2-track lead, got {m:?}"
            );
        }

        // Full games with both AI flags on finish cleanly.
        let deck: Vec<Card> = (1..=10)
            .flat_map(|n| ["h", "s", "c", "d"].map(|s| c(&format!("{}{s}", if n == 1 { "A".into() } else { n.to_string() }))))
            .chain(["Kh", "Qs", "Jd", "Jo"].map(c))
            .collect();
        for seed in 0..20 {
            let mut rng = ChaCha8Rng::seed_from_u64(seed + 100);
            let mut g = Game::new(deck.clone(), deck.clone(), &mut rng);
            let mut turns = 0;
            while g.phase != Phase::Over {
                let by = g.turn;
                let m = choose_with_config(&g, by, 0.5, &cfg_imp, &mut rng).expect("a move");
                g.apply(by, m).unwrap();
                turns += 1;
                assert!(turns < 500, "seed {seed}: improved AI game never ended");
            }
        }
    }
}
