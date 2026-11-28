//! Karma, ownership and crime (docs/spec-notes/crime-karma.md; owner's playtest list 2 of
//! 2026-10-12, "FACTION KARMA" and "CRIME").
//!
//! - **Karma** is the player's actor value Karma (23). An actor's alignment is its Karma
//!   against the globals `KarmaVeryEvil` (−750), `KarmaEvil` (−250), `KarmaGood` (250)
//!   and `KarmaVeryGood` (750) [observed: FalloutNV.esm; using them as the bounds:
//!   hypothesis by their names, §1]. Every change goes through [`Game::change_karma`],
//!   which leaves a [`Note::Karma`] for the engine's corner message ("A corner message is
//!   shown … indicating the change" [documented: GECK wiki RewardKarma]).
//! - **Ownership** (§2): a reference's own owner (XOWN, a faction with the rank XRNK
//!   needs), else its cell's (XOWN/XRNK of the CELL, or SetCellOwnership) [documented:
//!   GECK wiki IsOwner, "inherited ownership from the cell"; Reference, "Faction Rank
//!   required to own the reference"]. The player owns what is owned by the player's base
//!   or by a faction the player is in at the rank asked for.
//! - **Crimes** (§3) [documented: FalloutNV.esm message `HelpCrime`]: minor crimes (theft,
//!   and "trespass": opening an owned lock) make only the victim hostile, and only when
//!   it sees the crime; major crimes (assault, murder) the victim and the victim's
//!   friends. Who sees: an actor that has the player detected ([`crate::detection`],
//!   band Detected) [hypothesis: "sees" is detection]. A reacting actor becomes a
//!   **crime enemy** of the player (GetActorCrimePlayerEnemy): it attacks the player
//!   whatever its aggression ([`Game::crime_enemy`], read by `Game::would_attack`)
//!   [design, as an assaulted actor's grudge].
//! - **Crimes expire** (§3.7): a crime enemy lasts `iCoolDownTimerSetting` (3) game days
//!   from the last crime that made it [documented: GECK wiki Category:Crime; the default,
//!   Complete List of GameSettings]. The player's enemy flag with a faction that a crime
//!   set lasts [`FACTION_FLAG_DAYS`] (0.18 game hours) from the last crime that set it
//!   [observed: observer check 203, a seen murder's flags on VCaesarsLegionFaction and
//!   ArmorVCaesarsLegionFaction; that every crime's flag lasts as long: hypothesis;
//!   owner's ruling on #2498, owner-review item 65]. A flag a script set (SetPCEnemyOfFaction)
//!   stays until a script clears it [design]. [`Game::expire_crimes`] ends them, read
//!   against GameDaysPassed whenever hostility is asked afresh, so time passing by any
//!   route (play, waiting, sleeping, travel, a script setting the clock) counts (#2467).
//! - **Karma for crimes and kills** (§4): stealing, and opening an owned lock, cost
//!   `fKarmaModStealing` (−5) unless the owner is Evil (an Evil faction, or an owning
//!   actor of Evil alignment) or a faction that does not track crime [documented: GECK
//!   wiki Category:Factions, Evil and Track Crime; Reference, "karma gain/loss is affected
//!   by the karma level of the owning actor"; the lock: design]; killing an Evil actor
//!   gives `fKarmaModKillingEvilActor` (100) and a Very Good one takes as much
//!   [observed: crime-karma.md §4.1, capture o6_02], a Very Evil one gives it and a Good
//!   one takes it [hypothesis], judged by the placed base's own ACBS karma, not a
//!   template's ([`Game::kill_alignment`]) [hypothesis, §4.1]; murdering a Neutral NPC
//!   takes a quarter of it [design]. NPCs only [design].
//! - **Trespassing** in a cell (§3.3): in an owned cell without the Public place flag
//!   that the player does not own [documented: GECK wiki SetCellPublicFlag, "Cells which
//!   are public cannot have trespassing"]; an owner who sees the player there warns
//!   (GuardTrespass) every `fAITrespassWarningTimer` (5 s) up to `iGuardWarnings` (2)
//!   times, then attacks [documented for Guard packages, GECK wiki Guard Package;
//!   hypothesis: the same for owned cells]. While it warns (it has warned and sees the
//!   player at the tick) it is alerted: it draws its weapon, stops and faces the
//!   player (`Game::warning_to`; `crate::aggro`, "The look") [hypothesis; owner
//!   2026-10-24: warning guards draw their weapon]. Guard packages' own warnings are not
//!   built.
//!   A door whose XTEL is flagged No Alarm
//!   sends "no trespass alarm … when the player uses the door" [documented: GECK wiki
//!   Reference, Teleport]: arriving through one, the player is not trespassing in the
//!   cell arrived in until leaving it ([`Game::arrived_through`]) [hypothesis: the alarm
//!   is the owners' warnings and attack; docs/owner-review.md].
//!
//! The engine tells the game what the player did ([`Game::player_stole`],
//! [`Game::player_opened_lock`], [`Game::crime_tick`]); kills and hits come through
//! `Game::kill` and `Game::attacked_by`.

use crate::detection::Band;
use crate::functions::actor::{actor_value, faction_rank, factions_of};
use crate::functions::{base_of, none, yes, Args, Ctx};
use crate::game::Game;
use crate::provocation::{ALLY, FRIEND};
use fnv_script::vm::Value;
use fnv_sim::{FormId, World};
use std::collections::BTreeMap;

const KARMA: u16 = crate::actor_values::KARMA;

/// FACT flags as [`crate::data::GameData::faction_flags`] keeps them.
pub mod faction_flags {
    pub const HIDDEN: u16 = 1;
    pub const EVIL: u16 = 2;
    pub const SPECIAL_COMBAT: u16 = 4;
    pub const TRACK_CRIME: u16 = 256;
    pub const ALLOW_SELL: u16 = 512;
}

/// CELL DATA flag "Public place" [documented: xEdit wbDefinitionsFNV.pas, schema
/// `world.json`].
pub const CELL_PUBLIC: u8 = 0x20;
pub const CELL_INTERIOR: u8 = 0x01;
/// The record header flag "Off limits" of an interior cell [documented: GECK wiki
/// GetFormFlag, bit 17].
pub const FORM_OFF_LIMITS: u32 = 0x0002_0000;
/// The record header flag "Can't wait" of a cell [documented: GECK wiki GetFormFlag,
/// bit 19] (docs/spec-notes/cells-refs.md §1).
pub const FORM_CANT_WAIT: u32 = 0x0008_0000;

/// What a cell's record says about who owns it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CellDef {
    pub owner: Option<FormId>,
    pub rank: Option<i32>,
    pub public: bool,
    pub interior: bool,
    pub off_limits: bool,
    /// "Can't wait": the Wait menu refuses in it.
    pub cant_wait: bool,
}

impl CellDef {
    /// From the CELL's subrecords: DATA (flags), XOWN, XRNK, and the record header flags.
    pub fn from_raw(subs: &[fnv_esm::Subrecord], header_flags: u32, map: &dyn Fn(u32) -> Option<u32>) -> CellDef {
        let word = |sig: &[u8; 4]| subs.iter().find(|s| &s.sig == sig).and_then(|s| <[u8; 4]>::try_from(s.data.get(..4)?).ok()).map(u32::from_le_bytes);
        let flags = subs.iter().find(|s| &s.sig == b"DATA").and_then(|s| s.data.first().copied()).unwrap_or(0);
        CellDef {
            owner: word(b"XOWN").filter(|&l| l != 0).and_then(map).map(FormId),
            rank: word(b"XRNK").map(|r| r as i32),
            public: flags & CELL_PUBLIC != 0,
            interior: flags & CELL_INTERIOR != 0,
            off_limits: header_flags & FORM_OFF_LIMITS != 0,
            cant_wait: header_flags & FORM_CANT_WAIT != 0,
        }
    }

    /// Whether the cell is worth keeping: it has an owner or one of the flags.
    pub fn matters(&self) -> bool {
        self.owner.is_some() || self.public || self.off_limits || self.cant_wait
    }
}

/// Who owns something, and where that came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ownership {
    /// An NPC base or a faction.
    pub owner: FormId,
    /// For a faction: the rank needed to count as an owner.
    pub rank: Option<i32>,
    /// Inherited from the cell.
    pub from_cell: bool,
}

/// An alignment [documented: GECK wiki GetIsAlignment, "VeryGood, Good, Neutral, Evil,
/// VeryEvil"].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Alignment {
    VeryEvil,
    Evil,
    Neutral,
    Good,
    VeryGood,
}

impl Alignment {
    pub fn name(self) -> &'static str {
        match self {
            Alignment::VeryEvil => "Very Evil",
            Alignment::Evil => "Evil",
            Alignment::Neutral => "Neutral",
            Alignment::Good => "Good",
            Alignment::VeryGood => "Very Good",
        }
    }

    /// Karma against the bounds (very evil, evil, good, very good): at or beyond a bound
    /// is that side [hypothesis].
    pub fn of(karma: f64, b: [f64; 4]) -> Alignment {
        match karma {
            k if k <= b[0] => Alignment::VeryEvil,
            k if k <= b[1] => Alignment::Evil,
            k if k >= b[3] => Alignment::VeryGood,
            k if k >= b[2] => Alignment::Good,
            _ => Alignment::Neutral,
        }
    }

    /// GetIsAlignment's parameter code: 0 Good, 1 Neutral, 2 Evil, 3 Very Good, 4 Very
    /// Evil [hypothesis: xEdit's alignment enumeration as remembered; not checked].
    pub fn from_code(c: u16) -> Option<Alignment> {
        Some(match c {
            0 => Alignment::Good,
            1 => Alignment::Neutral,
            2 => Alignment::Evil,
            3 => Alignment::VeryGood,
            4 => Alignment::VeryEvil,
            _ => return None,
        })
    }

    pub fn evil(self) -> bool {
        self <= Alignment::Evil
    }

    pub fn good(self) -> bool {
        self >= Alignment::Good
    }
}

/// The crimes (§3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// Taking an owned item.
    Theft,
    /// Opening an owned lock (picked or forced) ("Trespass" in the game's help).
    Lock,
    /// Being in an owned, not public cell.
    Trespass,
    Assault,
    Murder,
    /// Caught picking an actor's pocket (`crate::pickpocket`).
    Pickpocket,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Theft => "theft",
            Kind::Lock => "opening an owned lock",
            Kind::Trespass => "trespassing",
            Kind::Assault => "assault",
            Kind::Murder => "murder",
            Kind::Pickpocket => "pickpocketing",
        }
    }

    /// Assault and murder are major crimes [documented: HelpCrime].
    pub fn major(self) -> bool {
        matches!(self, Kind::Assault | Kind::Murder)
    }

    /// The topic a witness says [documented: GECK wiki Combat Tab, Steal, Murder,
    /// GuardTrespass; Assault is provocation's]. None: it attacks at once [documented:
    /// HelpCrime, an opened lock].
    pub fn topic(self) -> Option<&'static str> {
        match self {
            Kind::Theft => Some("Steal"),
            Kind::Murder => Some("Murder"),
            Kind::Trespass => Some("GuardTrespass"),
            Kind::Pickpocket => Some("Pickpocket"),
            Kind::Lock | Kind::Assault => None,
        }
    }

    /// The topic a witness that ignores crime says instead: "the same as …, but the actor
    /// doesn't regard it as a crime" [documented: GECK wiki Combat Tab, AssaultNoCrime,
    /// MurderNoCrime, PickpocketNoCrime, StealNoCrime, TrespassNoCrime; #974]. None for an
    /// opened lock (no such topic).
    pub fn no_crime_topic(self) -> Option<&'static str> {
        match self {
            Kind::Theft => Some("StealNoCrime"),
            Kind::Murder => Some("MurderNoCrime"),
            Kind::Trespass => Some("TrespassNoCrime"),
            Kind::Pickpocket => Some("PickpocketNoCrime"),
            Kind::Assault => Some("AssaultNoCrime"),
            Kind::Lock => None,
        }
    }
}

/// What happened, for the engine's log, corner messages and summary.
#[derive(Debug, Clone, PartialEq)]
pub enum Note {
    /// The player's karma changed by `change` to `now`.
    Karma { change: f64, now: f64, why: String },
    /// A crime: against `victim` (an actor, an NPC base or a faction), seen by `seen_by`,
    /// who turned on the player: `hostile`.
    Crime { kind: Kind, victim: Option<FormId>, seen_by: Vec<FormId>, hostile: Vec<FormId> },
    /// `actor` warned the trespassing player (`level` warnings so far).
    Warned { actor: FormId, level: u32 },
    /// The player is now an enemy of this crime-tracking faction.
    FactionEnemy { faction: FormId },
    /// The crimes that made the player an enemy of `faction` expired: the last is `days`
    /// game days old (`Game::expire_crimes`).
    FactionEnemyEnded { faction: FormId, days: f64 },
    /// The crimes that made `actor` a crime enemy expired: the last is `days` game days old.
    CrimeEnemyEnded { actor: FormId, days: f64 },
    /// The player started or stopped trespassing in `cell` (owned by `owner`).
    Trespassing { cell: FormId, owner: FormId, now: bool },
    /// The player came through `door`, flagged No Alarm, into `cell`: no trespass alarm
    /// there until the player leaves it.
    NoAlarm { door: FormId, cell: FormId },
    /// A seen theft or kill raised the player's infamy with `reputation`
    /// (`functions::crime_infamy`).
    Infamy { reputation: FormId, before: f64, after: f64 },
}

/// The settings of karma and crime (§1, §4) with the values FalloutNV.esm gives them
/// [observed 2026-10-12]; the wiki's default for the trespass timer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub stealing: f64,
    pub killing_evil: f64,
    pub guard_warnings: f64,
    pub warning_timer: f64,
    /// KarmaVeryEvil, KarmaEvil, KarmaGood, KarmaVeryGood.
    pub bounds: [f64; 4],
    /// iCoolDownTimerSetting: game days a crime enemy (an actor) keeps the crime
    /// [documented: GECK wiki Category:Crime,
    /// 3 by default; Complete List of GameSettings, 3; FalloutNV.esm does not set it,
    /// observed].
    pub cool_down_days: f64,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { stealing: -5.0, killing_evil: 100.0, guard_warnings: 2.0, warning_timer: 5.0, bounds: [-750.0, -250.0, 250.0, 750.0], cool_down_days: 3.0 }
    }
}

impl Settings {
    pub fn read(g: &Game) -> Settings {
        let d = Settings::default();
        let s = |name: &str, v: f64| crate::combat::setting(g, name, v);
        let global = |name: &str, v: f64| g.world.forms.global_by_edid(name).and_then(|(id, _)| g.world.global(id).ok()).map_or(v, f64::from);
        Settings {
            stealing: s("fKarmaModStealing", d.stealing),
            killing_evil: s("fKarmaModKillingEvilActor", d.killing_evil),
            guard_warnings: s("iGuardWarnings", d.guard_warnings),
            warning_timer: s("fAITrespassWarningTimer", d.warning_timer),
            cool_down_days: s("iCoolDownTimerSetting", d.cool_down_days),
            bounds: [
                global("KarmaVeryEvil", d.bounds[0]),
                global("KarmaEvil", d.bounds[1]),
                global("KarmaGood", d.bounds[2]),
                global("KarmaVeryGood", d.bounds[3]),
            ],
        }
    }
}

/// Game days the player's enemy flag with a faction lasts after the last crime that set
/// it: 0.18 game hours. In the original a seen murder at Cottonwood Cove flagged both
/// VCaesarsLegionFaction and ArmorVCaesarsLegionFaction, and both read 0 again about
/// 0.18 game hours (11 minutes) later, with no cell change and no script clearing them,
/// while the witness kept fighting the player [observed: docs/handoff/observer-checks.md
/// check 203, capture `karl_murder2`, 2026-11-15]. That the same span holds for every
/// crime-set flag (a theft's, the Evil faction's area alert) is a [hypothesis]: only a
/// murder's was timed. Owner's ruling on #2498 (owner-review item 65): "Short-lived flag".
pub const FACTION_FLAG_DAYS: f64 = 0.18 / 24.0;

/// The slack of [`Game::expire_crimes`]: GameDaysPassed is an f32, a few seconds' worth
/// of a day at a few hundred days [design].
const EXPIRY_SLACK_DAYS: f64 = 1e-4;

/// Murdering a Neutral NPC costs this share of `fKarmaModKillingEvilActor` [design].
pub const NEUTRAL_MURDER_SHARE: f64 = 0.25;

/// The karma for the player's kill of an actor of `alignment` (the kill's,
/// [`Game::kill_alignment`]): an Evil or Very Evil NPC gives `fKarmaModKillingEvilActor`
/// [observed: −500 victims gave +100, capture o6_02 2026-10-31, crime-karma.md §4.1;
/// Very Evil: hypothesis, the setting's name], a Good or Very Good one takes as much
/// [observed for Very Good (900): −100, o6_02; Good: hypothesis], a `murder`ed Neutral one a
/// quarter of it [design]. Creatures give none: their karma includes plain animals (a
/// dead radroach is Evil in FalloutNV.esm) [design].
pub fn kill_karma(alignment: Alignment, murder: bool, npc: bool, s: &Settings) -> f64 {
    match alignment {
        _ if !npc => 0.0,
        a if a.evil() => s.killing_evil,
        a if a.good() => -s.killing_evil,
        _ if murder => -s.killing_evil * NEUTRAL_MURDER_SHARE,
        _ => 0.0,
    }
}

/// The run-time side of crime: notes for the engine, trespass warnings, the latest
/// victim. What lasts (counts, crime enemies, the murderer flag) is in
/// `PlayerState`.
#[derive(Debug, Clone, Default)]
pub struct Crimes {
    pub notes: Vec<Note>,
    pub settings: Option<Settings>,
    /// Warnings each actor gave the trespassing player, and when it gave the last.
    pub warnings: BTreeMap<FormId, (u32, f64)>,
    /// Actors warning the trespassing player now: they have warned it and saw it at the
    /// last crime tick, and have not turned on it (`Game::warning_to`).
    pub warning_now: std::collections::BTreeSet<FormId>,
    /// When each witness that ignores crime last said TrespassNoCrime: once per
    /// fAITrespassWarningTimer, as the warnings are paced (#974).
    pub no_crime_trespass: BTreeMap<FormId, f64>,
    /// The cell the player trespasses in, with its owner.
    pub trespassing: Option<(FormId, FormId)>,
    /// The cell the player arrived in through a No Alarm door, while the player is in it
    /// ([`Game::arrived_through`]).
    pub no_alarm_cell: Option<FormId>,
    /// The current crime victim (IsActorAVictim).
    pub victim: Option<FormId>,
}

impl Crimes {
    pub fn take_notes(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.notes)
    }
}

impl Game<'_> {
    pub fn crime_settings(&mut self) -> Settings {
        match self.crimes.settings {
            Some(s) => s,
            None => {
                let s = Settings::read(self);
                self.crimes.settings = Some(s);
                s
            }
        }
    }

    // ---- karma ---------------------------------------------------------------------

    /// An actor's alignment by its Karma (module notes). For an NPC base (an owner), its
    /// record's karma.
    pub fn alignment(&mut self, r: FormId) -> Alignment {
        let b = self.crime_settings().bounds;
        let karma = if self.data.actors.contains_key(&r) {
            self.template_source(r, r, |d| d.template_traits).map_or(self.data.actors[&r].karma, |(_, d)| d.karma)
        } else {
            actor_value(self, r, KARMA).unwrap_or(0.0)
        };
        Alignment::of(karma, b)
    }

    /// The alignment a kill of `r` is judged by: its placed base's own ACBS karma (a
    /// leveled base read as drawn for `r`), not a template's whose traits it takes
    /// [hypothesis: an NCRCF convict holding 0 itself and −500 only through its template
    /// gave no karma, a Fiend and a Powder Ganger holding −500 themselves gave +100,
    /// crime-karma.md §4.1]. [`Game::alignment`] when `r` has no actor base.
    pub fn kill_alignment(&mut self, r: FormId) -> Alignment {
        let own = base_of(self, r).filter(|_| r != FormId::PLAYER_REF).and_then(|b| self.template_source(b, r, |_| false)).map(|(_, d)| d.karma);
        match own {
            Some(karma) => Alignment::of(karma, self.crime_settings().bounds),
            None => self.alignment(r),
        }
    }

    /// Change the player's karma by `change`, within iKarmaMin and iKarmaMax when a
    /// plugin defines them [documented: GECK wiki RewardKarma]; leaves a note for the
    /// corner message. Returns the karma after.
    pub fn change_karma(&mut self, change: f64, why: impl Into<String>) -> f64 {
        let p = FormId::PLAYER_REF;
        let before = actor_value(self, p, KARMA).unwrap_or(0.0);
        let mut v = before + change;
        if let Some(max) = self.world.setting_f32("iKarmaMax") {
            v = v.min(max as f64);
        }
        if let Some(min) = self.world.setting_f32("iKarmaMin") {
            v = v.max(min as f64);
        }
        if v != before {
            let _ = crate::functions::actor::shift_base(self, p, KARMA, v - before);
        }
        self.crimes.notes.push(Note::Karma { change: v - before, now: v, why: why.into() });
        v
    }

    // ---- ownership -----------------------------------------------------------------

    /// Who owns a cell: SetCellOwnership's owner, else its record's.
    pub fn cell_ownership(&self, cell: FormId) -> Option<Ownership> {
        if let Some(&o) = self.forms.cell_owners.get(&cell) {
            return Some(Ownership { owner: o, rank: None, from_cell: true });
        }
        let c = self.data.cells.get(&cell)?;
        c.owner.map(|owner| Ownership { owner, rank: c.rank, from_cell: true })
    }

    /// Whether a cell is public: SetCellPublicFlag's, else its record's flag.
    /// Whether the player's cell is flagged "Can't wait" (cells-refs.md §1).
    pub fn cant_wait_here(&self) -> Option<FormId> {
        self.cell_of(FormId::PLAYER_REF).filter(|c| self.data.cells.get(c).is_some_and(|d| d.cant_wait))
    }

    pub fn cell_public(&self, cell: FormId) -> bool {
        self.forms.public_cells.get(&cell).copied().unwrap_or_else(|| self.data.cells.get(&cell).is_some_and(|c| c.public))
    }

    /// The faction rank required to own `r`: SetOwningFactionRequiredRank's override,
    /// else the placed record's XRNK.
    pub fn owner_rank(&self, r: FormId) -> Option<i32> {
        self.other.owner_ranks.get(&r).or_else(|| self.data.owner_ranks.get(&r)).copied()
    }

    /// The reference's own owner (without cell inheritance): live `extra.owner` when
    /// loaded/edited (`SetOwnership`/`ClearOwnership`/`SetOwnershipTemp`), else the
    /// placed record's XOWN.
    pub(crate) fn ref_owner_in(world: &World<'_>, r: FormId) -> Option<FormId> {
        match world.peek(r) {
            Some(s) => s.extra.owner,
            None => world.placed.get(r).and_then(|p| p.owner),
        }
    }

    /// Who owns a reference (module notes): its own owner, else its cell's. A reference
    /// made at run time (a dropped item) has no cell to inherit from [design].
    pub fn ownership(&self, r: FormId) -> Option<Ownership> {
        self.ownership_in(&self.world, r)
    }

    /// [`Game::ownership`] with the references' state of `world` (the engine keeps the
    /// world outside the game between script runs).
    pub fn ownership_in(&self, world: &World<'_>, r: FormId) -> Option<Ownership> {
        if let Some(owner) = Self::ref_owner_in(world, r) {
            return Some(Ownership { owner, rank: self.owner_rank(r), from_cell: false });
        }
        let state = world.peek(r);
        let placed = world.placed.get(r)?;
        let cell = state.and_then(|s| s.cell).or(placed.cell)?;
        self.cell_ownership(cell)
    }

    /// Whether `actor` (the player, or an actor reference) counts as an owner: the owner
    /// is its base, or a faction it is in at the rank asked for [documented: GECK wiki
    /// Reference ownership, Faction Rank; HasOwnership].
    pub fn counts_as_owner(&self, actor: FormId, o: &Ownership) -> bool {
        let base = if actor == FormId::PLAYER_REF { Some(FormId::PLAYER_BASE) } else { base_of(self, actor) };
        if base == Some(o.owner) || o.owner == actor {
            return true;
        }
        self.is_faction(o.owner) && faction_rank(self, actor, o.owner) >= o.rank.unwrap_or(0).max(0)
    }

    fn is_faction(&self, f: FormId) -> bool {
        self.data.sig(f).is_some_and(|s| &s == b"FACT")
    }

    /// The owner of `r` when it is someone other than the player: taking it is theft.
    pub fn owned_by_other(&self, r: FormId) -> Option<Ownership> {
        self.ownership(r).filter(|o| !self.counts_as_owner(FormId::PLAYER_REF, o))
    }

    /// Whether `witness` stands for the owner: it is the owning NPC, or a member of the
    /// owning faction [hypothesis: any rank].
    fn speaks_for(&self, witness: FormId, o: &Ownership) -> bool {
        if base_of(self, witness) == Some(o.owner) || witness == o.owner {
            return true;
        }
        self.is_faction(o.owner) && faction_rank(self, witness, o.owner) >= 0
    }

    /// SendTrespassAlarmAlt on `r`: the alarm goes to the owner of `r` (someone other
    /// than the player) instead of an actor's base: each witness who stands for that
    /// owner counts the trespass and turns on the player, as the trespass warnings' last
    /// step does [documented: SendTrespassAlarmAlt, "uses the calling reference's owner";
    /// hypothesis: the owner's people who see the player are the ones told]. How many
    /// turned.
    pub(crate) fn trespass_alarm_for(&mut self, r: FormId) -> usize {
        let Some(o) = self.owned_by_other(r) else { return 0 };
        let mut turned = 0;
        for w in self.crime_witnesses() {
            if !self.speaks_for(w, &o) {
                continue;
            }
            self.count_crime(w, Kind::Trespass);
            if self.mark_crime_enemy(w) {
                turned += 1;
                self.crimes.notes.push(Note::Crime { kind: Kind::Trespass, victim: Some(o.owner), seen_by: vec![w], hostile: vec![w] });
            }
        }
        turned
    }

    /// Whether stealing from this owner costs karma (module notes).
    pub(crate) fn karma_for_theft_from(&mut self, o: &Ownership) -> bool {
        self.karma_for_theft(o)
    }

    /// `actor` turns on the player for its own crime: counted against it, and it says
    /// the crime's line (`crate::pickpocket`). Whether it turned now.
    pub(crate) fn crime_turn(&mut self, actor: FormId, kind: Kind) -> bool {
        self.count_crime(actor, kind);
        self.turn_on_player(actor, kind)
    }

    /// Effective 16-bit FACT flags for `faction`, checking saved `FormChanges::faction_flags`
    /// overrides before the authored `GameData::faction_flags`.
    pub fn faction_flags(&self, faction: FormId) -> u16 {
        self.forms
            .faction_flags
            .get(&faction)
            .copied()
            .or_else(|| self.data.faction_flags.get(&faction).copied())
            .unwrap_or(0)
    }

    fn karma_for_theft(&mut self, o: &Ownership) -> bool {
        if self.is_faction(o.owner) {
            let f = self.faction_flags(o.owner);
            return f & faction_flags::TRACK_CRIME != 0 && f & faction_flags::EVIL == 0;
        }
        !self.alignment(o.owner).evil()
    }

    // ---- witnesses and crime enemies ------------------------------------------------

    /// The actors that see the player now: they have the player detected (band
    /// Detected, measured within the last second), live, are present, are not the
    /// player's teammates and do not ignore crime.
    pub fn crime_witnesses(&self) -> Vec<FormId> {
        let now = self.time;
        // Those who saw the player while a planted explosive was live, when its kill is
        // judged (`crate::pickpocket`).
        let planted = self.pickpocket.blast_witnesses.iter().copied().filter(|w| !self.detection.pairs.get(&(*w, FormId::PLAYER_REF)).is_some_and(|(d, at)| d.band == Band::Detected && now - at <= 1.0));
        self.detection
            .pairs
            .iter()
            .filter(|((w, t), (d, at))| *t == FormId::PLAYER_REF && *w != FormId::PLAYER_REF && d.band == Band::Detected && now - at <= 1.0)
            .map(|((w, _), _)| *w)
            .chain(planted)
            .filter(|&w| self.present(w) && !self.is_dead(w) && !self.extra.get(&w).is_some_and(|e| e.teammate || e.ignore_crime || e.out_cold()))
            .collect()
    }

    /// The actors that see the player now and would be witnesses ([`Game::crime_witnesses`])
    /// but ignore crime: they say the crime's NoCrime topic instead (#974).
    pub fn crime_ignorers(&self) -> Vec<FormId> {
        let now = self.time;
        self.detection
            .pairs
            .iter()
            .filter(|((w, t), (d, at))| *t == FormId::PLAYER_REF && *w != FormId::PLAYER_REF && d.band == Band::Detected && now - at <= 1.0)
            .map(|((w, _), _)| *w)
            .filter(|&w| self.present(w) && !self.is_dead(w) && self.extra.get(&w).is_some_and(|e| e.ignore_crime && !e.teammate && !e.out_cold()))
            .collect()
    }

    /// `actor`, which ignores crime, says `kind`'s NoCrime topic to the player.
    fn say_no_crime(&mut self, actor: FormId, kind: Kind) {
        if let Some(topic) = kind.no_crime_topic() {
            self.detection_bark(actor, Some(FormId::PLAYER_REF), topic);
        }
    }

    /// Whether `actor` attacks the player because of crimes: the player's crimes made it
    /// an enemy, or the player is an enemy of one of its factions (SetPCEnemyOfFaction,
    /// a witnessed murder) [documented: GECK wiki GetActorCrimePlayerEnemy,
    /// GetActorFactionPlayerEnemy]; never when it ignores crime [documented: IgnoreCrime].
    pub fn crime_enemy(&self, actor: FormId) -> bool {
        if self.extra.get(&actor).is_some_and(|e| e.ignore_crime) {
            return false;
        }
        // A faction's flag yields to a faction disguise (`player_enemy_of_factions`).
        self.player.crime_enemies.contains(&actor)
            || (!self.player.enemy_of.is_empty() && crate::functions::actor::player_enemy_of_factions(self, &factions_of(self, actor), &factions_of(self, FormId::PLAYER_REF)))
    }

    fn count_crime(&mut self, victim: FormId, kind: Kind) {
        let c = self.player.crime_counts.entry(victim).or_default();
        c[kind.major() as usize] += 1;
        self.crimes.victim = Some(victim);
    }

    /// `actor` turns on the player for a crime: it becomes a crime enemy and says the
    /// crime's line.
    fn turn_on_player(&mut self, actor: FormId, kind: Kind) -> bool {
        if !self.mark_crime_enemy(actor) {
            return false;
        }
        if let Some(topic) = kind.topic() {
            self.detection_bark(actor, Some(FormId::PLAYER_REF), topic);
        }
        true
    }

    // ---- the crimes ----------------------------------------------------------------

    /// The player took `count` of `item`: the reference `reference` from the world, or
    /// from the container `container`. Theft when someone else owns it (§3.1): karma
    /// unless the owner is Evil, and the owner's side turns on the player when it sees
    /// it. Returns whether it was theft.
    pub fn player_stole(&mut self, reference: Option<FormId>, container: Option<FormId>, item: FormId, count: i32) -> bool {
        let Some(o) = container.or(reference).and_then(|r| self.owned_by_other(r)) else { return false };
        let s = self.crime_settings();
        if self.karma_for_theft(&o) {
            let name = self.data.name(item);
            self.change_karma(s.stealing, format!("stealing {name} x{count}"));
        }
        // The player's tweak: theft raises no alarm [design: docs/design/tweaks.md].
        if !self.tweaks.stealing_no_alarm {
            self.witnessed(Kind::Theft, &o);
        }
        true
    }

    /// The player picked or forced the lock of `reference` open (lockpicking's crime
    /// hook): when it is owned, karma as for theft [design] and the owner's side that
    /// sees it attacks at once [documented: HelpCrime, "Trespass"]. Returns the owner.
    pub fn player_opened_lock(&mut self, reference: FormId) -> Option<Ownership> {
        let o = self.owned_by_other(reference)?;
        let s = self.crime_settings();
        if self.karma_for_theft(&o) {
            self.change_karma(s.stealing, format!("opening the lock of {reference}"));
        }
        self.witnessed(Kind::Lock, &o);
        Some(o)
    }

    /// A minor crime against `o`: every witness that speaks for the owner turns on the
    /// player; the crime counts against each of them (or the owner when none saw it).
    fn witnessed(&mut self, kind: Kind, o: &Ownership) {
        let seen_by = self.crime_witnesses();
        let owners: Vec<FormId> = seen_by.iter().copied().filter(|&w| self.speaks_for(w, o)).collect();
        // A faction that does not track crime does not remember it [documented: GECK wiki
        // Category:Factions, Track Crime].
        let tracks = !self.is_faction(o.owner) || (self.faction_flags(o.owner) & faction_flags::TRACK_CRIME != 0);
        let mut hostile = Vec::new();
        if tracks {
            for &w in &owners {
                self.count_crime(w, kind);
                if self.turn_on_player(w, kind) {
                    hostile.push(w);
                }
            }
        }
        if owners.is_empty() {
            self.count_crime(o.owner, kind);
        }
        // Those that would speak for the owner but ignore crime say so (#974).
        for w in self.crime_ignorers() {
            if self.speaks_for(w, o) {
                self.say_no_crime(w, kind);
            }
        }
        let seen = !seen_by.is_empty();
        self.crimes.notes.push(Note::Crime { kind, victim: Some(o.owner), seen_by, hostile });
        self.crime_infamy(kind, o.owner, seen);
    }

    /// Infamy with the victim's reputations for a seen theft or kill, noted.
    fn crime_infamy(&mut self, kind: Kind, victim: FormId, seen: bool) {
        for (reputation, before, after) in crate::functions::crime_infamy(self, kind, victim, seen) {
            self.crimes.notes.push(Note::Infamy { reputation, before, after });
        }
    }

    /// The player is about to kill `victim` (`Game::kill`, before it is dead): karma by
    /// its alignment, and murder when it is a non-Evil NPC that was not fighting the
    /// player of its own accord (§3.4, §4).
    pub fn player_kills(&mut self, victim: FormId) {
        self.player_kills_karma(victim, true);
    }

    /// [`Game::player_kills`], giving back the karma change the kill earns. With
    /// `apply_karma` false (a co-op guest's kill) the host's player keeps its karma: the
    /// change is the guest's, for its own character (`crate::coop_kills`); murder, infamy
    /// and the witnesses stay the host world's, as for the player's own kill (coop-shared-progress.md §4.2)
    /// [design].
    pub(crate) fn player_kills_karma(&mut self, victim: FormId, apply_karma: bool) -> f64 {
        if victim == FormId::PLAYER_REF || self.is_dead(victim) {
            return 0.0;
        }
        // Its Evil faction's members in the area turn on the player (`crate::provocation`).
        if !self.provocations.holds(victim, FormId::PLAYER_REF) && !self.would_attack(victim, FormId::PLAYER_REF) {
            self.evil_area_alert(victim);
        }
        let s = self.crime_settings();
        let alignment = self.alignment(victim);
        let npc = base_of(self, victim).and_then(|b| self.data.actors.get(&b)).is_some_and(|d| &d.sig == b"NPC_");
        // Self-defence: it attacked the player by its records, a script or a crime of the
        // player's; a fight the player started (a grudge) is no defence.
        // One fighting the player without a grudge chose that fight (item 22) [design].
        let grudge = self.provocations.holds(victim, FormId::PLAYER_REF);
        let fights_player = self.awareness.get(victim).is_some_and(|a| a.state.in_combat() && a.targets.contains_key(&FormId::PLAYER_REF));
        let aggressor = self.awareness.forced.contains(&(victim, FormId::PLAYER_REF)) || (!grudge && (fights_player || self.would_attack(victim, FormId::PLAYER_REF)));
        let murder = npc && !alignment.evil() && (!aggressor || self.player.crime_enemies.contains(&victim));
        let name = self.data.name(victim);
        // Karma by the victim's own record (§4.1): murder stays judged by its alignment
        // as scripts read it (GetIsAlignment) [design].
        let judged = self.kill_alignment(victim);
        let change = kill_karma(judged, murder, npc, &s);
        if change != 0.0 && apply_karma {
            self.change_karma(change, format!("killing {name} ({})", judged.name()));
        }
        // Any kill of a reputation's people while someone (the victim too) sees the
        // player, fighting or not [documented: Fallout wiki "Reputation (Fallout: New
        // Vegas)", killing members openly; only a kill from a hidden state gives none].
        let seen = !self.crime_witnesses().is_empty();
        self.crime_infamy(Kind::Murder, victim, seen);
        if !murder {
            return change;
        }
        self.player.murderer = true;
        self.count_crime(victim, Kind::Murder);
        let seen_by: Vec<FormId> = self.crime_witnesses().into_iter().filter(|&w| w != victim).collect();
        // The victim's allies and friends that ignore crime say MurderNoCrime (#974).
        for w in self.crime_ignorers() {
            if w != victim && matches!(self.reaction_to(w, victim), ALLY | FRIEND) {
                self.say_no_crime(w, Kind::Murder);
            }
        }
        let mut hostile = Vec::new();
        // A witness who cares, turned now or a crime enemy already.
        let mut cared = false;
        for &w in &seen_by {
            if matches!(self.reaction_to(w, victim), ALLY | FRIEND) {
                cared = true;
                if self.turn_on_player(w, Kind::Murder) {
                    hostile.push(w);
                }
            }
        }
        // A major crime against a crime-tracking faction makes the player its enemy
        // [documented: GECK wiki SetPCEnemyOfFaction; hypothesis: when a witness who
        // cares saw it]. Seen by witnesses already hostile, it is still a crime: the flag's
        // count restarts (§3.7, #2467).
        if cared {
            for f in factions_of(self, victim) {
                let tracks = self.faction_flags(f) & faction_flags::TRACK_CRIME != 0;
                if tracks {
                    self.make_player_enemy_of(f);
                }
            }
        }
        self.crimes.notes.push(Note::Crime { kind: Kind::Murder, victim: Some(victim), seen_by, hostile });
        change
    }

    /// The player is an enemy of `faction` from now on (a crime against it it tracks,
    /// `crate::provocation`'s Evil faction area); noted the first time. The flag lasts
    /// [`FACTION_FLAG_DAYS`] from this crime, a later one restarting the count
    /// ([`Game::expire_crimes`]).
    pub(crate) fn make_player_enemy_of(&mut self, faction: FormId) {
        let day = self.crime_day();
        self.player.enemy_of_crimes.insert(faction, day);
        if self.player.enemy_of.insert(faction) {
            self.crimes.notes.push(Note::FactionEnemy { faction });
            crate::functions::actor::forget_relations(self);
        }
    }

    /// `actor` is a crime enemy from now on, dated by this crime: a later crime restarts
    /// its count. Whether it was not one before.
    fn mark_crime_enemy(&mut self, actor: FormId) -> bool {
        let day = self.crime_day();
        self.player.crime_enemy_since.insert(actor, day);
        self.player.crime_enemies.insert(actor)
    }

    /// GameDaysPassed now; `None` without the time globals (synthetic data).
    pub fn game_days(&self) -> Option<f64> {
        let (id, _) = self.world.forms.global_by_edid(fnv_sim::clock::GAME_DAYS_PASSED)?;
        self.world.global(id).ok().map(f64::from)
    }

    /// The day a crime is dated by: GameDaysPassed, 0 without the clock (nothing then
    /// expires, there being no clock to read).
    fn crime_day(&self) -> f64 {
        self.game_days().unwrap_or(0.0)
    }

    /// SetPCEnemyOfFaction: `on` makes the player an enemy of `faction` until a script
    /// clears it [design: scripts use it for lasting hostility]; off
    /// clears the flag, whoever set it.
    pub(crate) fn script_enemy_of(&mut self, faction: FormId, on: bool) {
        if on {
            self.player.enemy_of_script.insert(faction);
            self.player.enemy_of.insert(faction);
        } else {
            self.clear_enemy_of(faction);
        }
        crate::functions::actor::forget_relations(self);
    }

    /// The player is no longer an enemy of `faction`, set by a crime or a script
    /// (ClearFactionPlayerEnemyFlag and the like). The caller forgets the kept relations.
    pub(crate) fn clear_enemy_of(&mut self, faction: FormId) {
        self.player.enemy_of.remove(&faction);
        self.player.enemy_of_crimes.remove(&faction);
        self.player.enemy_of_script.remove(&faction);
    }

    /// Crimes end (module notes, §3.7): each crime enemy whose last crime is
    /// iCoolDownTimerSetting game days old forgives the player [documented: GECK wiki
    /// Category:Crime], and each faction flag a crime set whose last crime is
    /// [`FACTION_FLAG_DAYS`] old is cleared [observed: observer check 203], unless a script
    /// set it too [design: a script's flag stays]. Read
    /// against GameDaysPassed now, so it holds however the clock moved. Called where
    /// hostility is asked afresh: a detection round's and a combat AI frame's start, the
    /// crime tick, the Wait menu's check and the script functions that read the flags.
    /// Returns whether anything ended.
    pub fn expire_crimes(&mut self) -> bool {
        if self.player.enemy_of_crimes.is_empty() && self.player.crime_enemy_since.is_empty() {
            return false;
        }
        let Some(now) = self.game_days() else { return false };
        let limit = self.crime_settings().cool_down_days;
        let old = |since: f64, limit: f64| now - since + EXPIRY_SLACK_DAYS >= limit;
        let factions: Vec<(FormId, f64)> = self.player.enemy_of_crimes.iter().filter(|(_, &d)| old(d, FACTION_FLAG_DAYS)).map(|(&f, &d)| (f, now - d)).collect();
        let actors: Vec<(FormId, f64)> = self.player.crime_enemy_since.iter().filter(|(_, &d)| old(d, limit)).map(|(&a, &d)| (a, now - d)).collect();
        for &(faction, days) in &factions {
            self.player.enemy_of_crimes.remove(&faction);
            if !self.player.enemy_of_script.contains(&faction) && self.player.enemy_of.remove(&faction) {
                self.crimes.notes.push(Note::FactionEnemyEnded { faction, days });
            }
        }
        for &(actor, days) in &actors {
            self.player.crime_enemy_since.remove(&actor);
            if self.player.crime_enemies.remove(&actor) {
                self.crimes.warnings.remove(&actor);
                self.crimes.notes.push(Note::CrimeEnemyEnded { actor, days });
            }
        }
        let any = !factions.is_empty() || !actors.is_empty();
        if any {
            crate::functions::actor::forget_relations(self);
        }
        any
    }

    /// After a load: a save from before #2467 holds its crime enmities undated. Its crime
    /// enemies (actors) are dated now, so they end iCoolDownTimerSetting game days after
    /// the load. Its faction flags are kept as a script's, never ending by the cooldown,
    /// when a script of the load order can set them: such a save does not say whether a
    /// crime or a quest's SetPCEnemyOfFaction set one, and a quest's hostility must not
    /// end [design]. A flag on a faction no script can set
    /// ([`Game::script_flaggable_factions`]) was a crime's: it is dated now and ends
    /// [`FACTION_FLAG_DAYS`] after the load [design, #3335].
    ///
    /// A save made by a build that kept every undated flag as a script's (#2467 to #3335)
    /// holds such a crime's flag among the script's. Those on a faction no script can set
    /// end at the load [design, #3335: the player report R-833D3F5X held
    /// ArmorVCaesarsLegionFaction so, which no FalloutNV.esm script names to
    /// SetPCEnemyOfFaction, and Karl, in that faction, fought the player's companion on
    /// entering the Great Khan longhouse; the crime behind it was older than the save
    /// it was first kept in, so by observer check 203 long over]. A flag the player set
    /// with the console's SetPCEnemyOfFaction on such a faction ends with them.
    pub(crate) fn date_crimes(&mut self) {
        let now = self.crime_day();
        let undated: Vec<FormId> = self.player.crime_enemies.iter().copied().filter(|a| !self.player.crime_enemy_since.contains_key(a)).collect();
        for a in undated {
            self.player.crime_enemy_since.insert(a, now);
        }
        if self.player.enemy_of.is_empty() {
            return;
        }
        let flaggable = self.script_flaggable_factions();
        let by_crime = |f: &FormId| flaggable.as_ref().is_some_and(|s| !s.contains(f));
        let undated: Vec<FormId> = self.player.enemy_of.iter().copied().filter(|f| !self.player.enemy_of_crimes.contains_key(f) && !self.player.enemy_of_script.contains(f)).collect();
        for f in undated {
            if by_crime(&f) {
                tracing::info!("load: the player's undated flag with faction {} is a crime's (no script sets it): it ends {:.2} game hours from now", self.data.name(f), FACTION_FLAG_DAYS * 24.0);
                self.player.enemy_of_crimes.insert(f, now);
            } else {
                self.player.enemy_of_script.insert(f);
            }
        }
        let stale: Vec<FormId> = self.player.enemy_of_script.iter().copied().filter(|f| by_crime(f) && !self.player.enemy_of_crimes.contains_key(f)).collect();
        for &f in &stale {
            tracing::info!("load: the player's flag with faction {} was an older save's crime, kept as a script's, and no script sets it: it ends (#3335)", self.data.name(f));
            self.player.enemy_of_script.remove(&f);
            if self.player.enemy_of.remove(&f) {
                self.crimes.notes.push(Note::FactionEnemyEnded { faction: f, days: 0.0 });
            }
        }
        if !stale.is_empty() {
            crate::functions::actor::forget_relations(self);
        }
    }

    /// The factions a script of the load order names to SetPCEnemyOfFaction as its first
    /// parameter, a form (any unit: a script, a quest stage's, a dialogue line's, a
    /// package's or a terminal's result script); `None` when a call passes a reference
    /// variable or something the decoder left raw, so that any faction could be one
    /// [design, #3335]. Calls inside extended expressions (`eval`, `let`) are not read:
    /// none in the masters. FalloutNV.esm's 19 calls name 8 factions [observed:
    /// `fnvtool script decompile`, 2026-11-25].
    pub fn script_flaggable_factions(&self) -> Option<std::collections::BTreeSet<FormId>> {
        let op = self.data.table.by_name("setpcenemyoffaction")?;
        let mut out = std::collections::BTreeSet::new();
        let mut unknown = false;
        for c in &self.data.units {
            c.program.walk(&mut |st| {
                let fnv_script::ir::Stmt::Call { call } = st else { return };
                if call.opcode != op {
                    return;
                }
                match call.args.first() {
                    Some(fnv_script::ir::Arg::Ref(r)) => match c.unit.slot(*r) {
                        Some(fnv_script::unit::RefSlot::Form(f)) => {
                            out.insert(FormId(f));
                        }
                        _ => unknown = true,
                    },
                    _ => unknown = true,
                }
            });
        }
        (!unknown).then_some(out)
    }

    /// The player assaulted `victim` (provocation turned it on the player): a major crime
    /// against it [documented: HelpCrime]. Its friends join through assistance
    /// (`crate::provocation`).
    pub fn player_assaulted(&mut self, victim: FormId) {
        // A victim that ignores crime says AssaultNoCrime (#974).
        if self.extra.get(&victim).is_some_and(|e| e.ignore_crime) {
            self.say_no_crime(victim, Kind::Assault);
        }
        self.count_crime(victim, Kind::Assault);
        self.crimes.notes.push(Note::Crime { kind: Kind::Assault, victim: Some(victim), seen_by: Vec::new(), hostile: vec![victim] });
    }

    /// The player ate `victim`'s corpse (Cannibal, `crate::activate_choices`): "a crime
    /// against nature" when seen [documented: PERK Cannibal DESC]; every witness turns
    /// on the player as for an assault [hypothesis]. Returns how many did.
    pub fn player_cannibalized(&mut self, victim: FormId) -> usize {
        let seen_by = self.crime_witnesses();
        let hostile: Vec<FormId> = seen_by.iter().copied().filter(|&w| self.turn_on_player(w, Kind::Assault)).collect();
        let n = hostile.len();
        if !seen_by.is_empty() {
            self.crimes.notes.push(Note::Crime { kind: Kind::Assault, victim: Some(victim), seen_by, hostile });
        }
        n
    }

    /// The player came through door `door` into `cell` (`None`: not known): a door
    /// flagged No Alarm (REFR XTEL) excuses the player's trespassing in that cell until
    /// the player leaves it; any other door ends an earlier excuse (module notes).
    pub fn arrived_through(&mut self, door: FormId, cell: Option<FormId>) {
        self.note_player_door(door);
        let no_alarm = self.world.placed.get(door).and_then(|p| p.teleport).is_some_and(|t| t.no_alarm);
        self.crimes.no_alarm_cell = cell.filter(|_| no_alarm);
        if let Some(cell) = self.crimes.no_alarm_cell {
            self.crimes.notes.push(Note::NoAlarm { door, cell });
        }
    }

    /// Where the player trespasses now: an owned cell, not public, that the player does
    /// not own, unless the player came in through a No Alarm door (module notes).
    pub fn trespass_cell(&self) -> Option<(FormId, Ownership)> {
        let cell = self.cell_of(FormId::PLAYER_REF)?;
        if self.crimes.no_alarm_cell == Some(cell) {
            return None;
        }
        let o = self.cell_ownership(cell)?;
        (!self.cell_public(cell) && !self.counts_as_owner(FormId::PLAYER_REF, &o)).then_some((cell, o))
    }

    /// After a round of detection measurements: trespass warnings and attacks (module
    /// notes). Returns how many actors turned on the player.
    pub fn crime_tick(&mut self) -> usize {
        self.expire_crimes();
        let s = self.crime_settings();
        let now = self.time;
        // The No Alarm door's excuse ends once the player is out of its cell.
        if self.crimes.no_alarm_cell.is_some() && self.crimes.no_alarm_cell != self.cell_of(FormId::PLAYER_REF) {
            self.crimes.no_alarm_cell = None;
        }
        let here = self.trespass_cell();
        let key = here.map(|(c, o)| (c, o.owner));
        if key != self.crimes.trespassing {
            if let Some((cell, owner)) = self.crimes.trespassing {
                self.crimes.notes.push(Note::Trespassing { cell, owner, now: false });
            }
            if let Some((cell, owner)) = key {
                self.crimes.notes.push(Note::Trespassing { cell, owner, now: true });
            }
            self.crimes.trespassing = key;
            self.crimes.warnings.clear();
        }
        self.crimes.warning_now.clear();
        let Some((_, o)) = here else { return 0 };
        let mut turned = 0;
        // The owner's people that ignore crime say TrespassNoCrime, once per warning
        // interval (#974).
        for w in self.crime_ignorers() {
            let last = self.crimes.no_crime_trespass.get(&w).copied().unwrap_or(f64::NEG_INFINITY);
            if self.speaks_for(w, &o) && now - last >= s.warning_timer {
                self.crimes.no_crime_trespass.insert(w, now);
                self.say_no_crime(w, Kind::Trespass);
            }
        }
        for w in self.crime_witnesses() {
            if !self.speaks_for(w, &o) || self.player.crime_enemies.contains(&w) {
                continue;
            }
            let (level, last) = self.crimes.warnings.get(&w).copied().unwrap_or((0, f64::NEG_INFINITY));
            if now - last < s.warning_timer {
                self.crimes.warning_now.insert(w);
                continue;
            }
            if (level as f64) < s.guard_warnings {
                self.crimes.warnings.insert(w, (level + 1, now));
                self.crimes.warning_now.insert(w);
                self.detection_bark(w, Some(FormId::PLAYER_REF), "GuardTrespass");
                self.crimes.notes.push(Note::Warned { actor: w, level: level + 1 });
            } else {
                self.count_crime(w, Kind::Trespass);
                if self.mark_crime_enemy(w) {
                    turned += 1;
                    self.crimes.notes.push(Note::Crime { kind: Kind::Trespass, victim: Some(o.owner), seen_by: vec![w], hostile: vec![w] });
                }
            }
        }
        turned
    }

    /// A crime-enemy actor that StopCombat calmed forgets the player's crimes against it
    /// [design: as a grudge ends].
    pub fn forgive_crimes(&mut self, actor: FormId) {
        self.player.crime_enemies.remove(&actor);
        self.player.crime_enemy_since.remove(&actor);
        self.crimes.warnings.remove(&actor);
    }
}

// ---- script functions ----------------------------------------------------------------

/// `RewardKarma value` [documented: GECK wiki RewardKarma].
pub fn reward_karma(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let v = a.num(0)?;
    g.change_karma(v, "a script (RewardKarma)");
    none()
}

/// `actor.GetIsAlignment alignment` (module notes, [`Alignment::from_code`]).
pub fn get_is_alignment(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let want = Alignment::from_code(a.code(0)?).ok_or("unknown alignment code")?;
    yes(g.alignment(r) == want)
}

/// `IsPCAMurderer` [documented: "the player has ever murdered a non-Evil NPC"].
pub fn is_pc_a_murderer(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    yes(g.player.murderer)
}

/// `actor.GetMinorCrimeCount` [documented].
pub fn get_minor_crime_count(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(Value::Num(g.player.crime_counts.get(&r).map_or(0, |c| c[0]) as f64))
}

/// `actor.GetMajorCrimeCount` [documented].
pub fn get_major_crime_count(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(Value::Num(g.player.crime_counts.get(&r).map_or(0, |c| c[1]) as f64))
}

/// `actor.GetActorCrimePlayerEnemy` [documented: an enemy because of crimes against that
/// actor].
pub fn get_actor_crime_player_enemy(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.expire_crimes();
    yes(g.player.crime_enemies.contains(&r))
}

/// `actor.GetAlarmed`: 1 while the actor is a crime enemy of the player [hypothesis: the
/// wiki says only "alarmed"].
pub fn get_alarmed(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.expire_crimes();
    yes(g.crime_enemy(r))
}

/// `actor.IsActorAVictim`: the current crime victim [documented].
pub fn is_actor_a_victim(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.crimes.victim == Some(r))
}

/// The owner a function's argument names: an actor reference stands for its base, the
/// player for the player's base; none means the player [documented: IsCellOwner,
/// SetOwnership].
fn owner_arg(g: &Game, a: &Args, i: usize) -> FormId {
    match a.opt_form(i) {
        None | Some(FormId::PLAYER_REF) => FormId::PLAYER_BASE,
        Some(f) if g.world.placed.get(f).is_some() => base_of(g, f).unwrap_or(f),
        Some(f) => f,
    }
}

/// `IsCellOwner cell [owner]` [documented].
pub fn is_cell_owner(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let cell = a.form(0)?;
    let want = owner_arg(g, a, 1);
    yes(g.cell_ownership(cell).is_some_and(|o| o.owner == want))
}

/// `actor.IsInMyOwnedCell`: the actor, or one of its factions, owns its cell
/// [documented: notes].
pub fn is_in_my_owned_cell(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    let o = g.cell_of(r).and_then(|c| g.cell_ownership(c));
    yes(o.is_some_and(|o| g.counts_as_owner(r, &Ownership { rank: None, ..o })))
}

/// `actor.IsTrespassing` (the player only; module notes).
pub fn is_trespassing(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    yes(r == FormId::PLAYER_REF && g.trespass_cell().is_some())
}

/// `actor.GetTrespassWarningLevel target`: the warnings the actor gave the player, by a
/// Guard package, for trespassing or for its aggro radius [documented: "the number of
/// warnings an actor on a Guard Package (or responding to trespassing or responding to an
/// aggro radius violation) has given"]; the larger count of the two kept.
pub fn get_trespass_warning_level(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if a.opt_form(0).is_some_and(|t| t != FormId::PLAYER_REF) {
        return Ok(Value::Num(0.0));
    }
    let trespass = g.crimes.warnings.get(&r).map_or(0, |w| w.0);
    let guard = g.provocations.aggro_warnings.get(&r).map_or(0, |w| w.0);
    Ok(Value::Num(trespass.max(guard) as f64))
}

/// `[reportedTo.]SendTrespassAlarm [reportedTo]`: that actor goes "all the way to fully
/// trespassed" at once [documented: notes] — it turns on the player.
pub fn send_trespass_alarm(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let to = a.opt_form(0).filter(|&f| f != FormId::PLAYER_REF).or(ctx.on.or(ctx.self_ref)).filter(|&f| f != FormId::PLAYER_REF).ok_or("no actor to report to")?;
    g.count_crime(to, Kind::Trespass);
    let hostile = if g.turn_on_player(to, Kind::Trespass) { vec![to] } else { Vec::new() };
    g.crimes.notes.push(Note::Crime { kind: Kind::Trespass, victim: Some(to), seen_by: vec![to], hostile });
    none()
}

/// `[criminal.]SendAssaultAlarm [victim] [faction]`: the victim reacts as to an assault;
/// with the player (or none) as the victim, a present member of the faction does
/// [documented: GECK wiki SendAssaultAlarm, "it will pick an NPC in the BigTown faction
/// to use instead"]. It says Assault "even if … already in combat" [documented: notes].
///
/// With a faction given and no member present and alive, nobody is the victim and
/// nothing happens: the caller is no member, so it is never picked [documented: the
/// wiki's "an NPC in the … faction"; hypothesis: the original does nothing then,
/// observer check 241] (#2926: a freed Techatticup hostage's alarm on the Legion turned
/// the hostage). Only the form without a faction falls back to the calling actor
/// [design, #2926].
pub fn send_assault_alarm(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let named = a.opt_form(0).filter(|&f| f != FormId::PLAYER_REF);
    let caller = ctx.on.or(ctx.self_ref).filter(|&f| f != FormId::PLAYER_REF);
    let victim = match named {
        Some(v) => Some(v),
        // A faction parameter is given (it may read as no form: an empty reference).
        None if a.len() > 1 => {
            let picked = a.opt_form(1).map(|f| faction_member_victim(g, f));
            match picked {
                Some(p) => tracing::info!(
                    "assault alarm: faction {} from {}: victim {}; {} member(s) present and alive, {} not present, {} dead",
                    p.faction,
                    caller.map_or_else(|| "no caller".to_string(), |c| c.to_string()),
                    p.victim.map_or_else(|| "none".to_string(), |v| v.to_string()),
                    p.alive,
                    p.absent,
                    p.dead
                ),
                None => tracing::info!("assault alarm: faction parameter is no form; no victim"),
            }
            let (victim, alive, absent, dead) = picked.map_or((None, 0, 0, 0), |p| (p.victim, p.alive, p.absent, p.dead));
            g.emit(crate::trace::EventKind::AssaultAlarm { caller, faction: picked.map(|p| p.faction), victim, alive, absent, dead });
            victim
        }
        None => Some(caller.ok_or("no victim")?),
    };
    let Some(victim) = victim else {
        return none();
    };
    g.count_crime(victim, Kind::Assault);
    g.mark_crime_enemy(victim);
    g.detection_bark(victim, Some(FormId::PLAYER_REF), "Assault");
    g.crimes.notes.push(Note::Crime { kind: Kind::Assault, victim: Some(victim), seen_by: Vec::new(), hostile: vec![victim] });
    none()
}

/// The member of a faction SendAssaultAlarm picks, and why the others were not.
#[derive(Debug, Clone, Copy)]
struct FactionPick {
    faction: FormId,
    /// The first placed actor (FormID order) in the faction, present and alive.
    victim: Option<FormId>,
    /// Members present and alive.
    alive: usize,
    /// Members disabled or deleted.
    absent: usize,
    /// Members present but dead.
    dead: usize,
}

/// The victim of `SendAssaultAlarm Player <faction>`: the first placed actor in the faction
/// (rank 0 or more, `faction_rank`) that is present and alive, in FormID order [design:
/// the order the pick had before #2926].
fn faction_member_victim(g: &Game, faction: FormId) -> FactionPick {
    let mut pick = FactionPick { faction, victim: None, alive: 0, absent: 0, dead: 0 };
    // `refs` is a BTreeMap: FormID order.
    for (&r, p) in &g.world.placed.refs {
        if !matches!(&p.sig, b"ACHR" | b"ACRE") || faction_rank(g, r, faction) < 0 {
            continue;
        }
        if !g.present(r) {
            pick.absent += 1;
        } else if g.is_dead(r) {
            pick.dead += 1;
        } else {
            pick.alive += 1;
            pick.victim.get_or_insert(r);
        }
    }
    pick
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alignment_bounds() {
        let b = Settings::default().bounds;
        assert_eq!(Alignment::of(0.0, b), Alignment::Neutral);
        assert_eq!(Alignment::of(-249.0, b), Alignment::Neutral);
        assert_eq!(Alignment::of(-250.0, b), Alignment::Evil);
        assert_eq!(Alignment::of(-750.0, b), Alignment::VeryEvil);
        assert_eq!(Alignment::of(250.0, b), Alignment::Good);
        assert_eq!(Alignment::of(1000.0, b), Alignment::VeryGood);
        assert!(Alignment::VeryEvil.evil() && Alignment::Evil.evil() && !Alignment::Neutral.evil());
        assert!(Alignment::Good.good() && !Alignment::Neutral.good());
    }

    #[test]
    fn crime_kinds() {
        assert!(Kind::Murder.major() && Kind::Assault.major());
        assert!(!Kind::Theft.major() && !Kind::Lock.major() && !Kind::Trespass.major());
        assert_eq!(Kind::Theft.topic(), Some("Steal"));
        assert_eq!(Kind::Lock.topic(), None);
    }

    #[test]
    fn cell_flags_from_raw() {
        let sub = |sig: &[u8; 4], data: Vec<u8>| fnv_esm::Subrecord { sig: *sig, data, extended: false };
        let subs = vec![sub(b"DATA", vec![0x21]), sub(b"XOWN", 0x0010_4C78u32.to_le_bytes().to_vec()), sub(b"XRNK", 2i32.to_le_bytes().to_vec())];
        let c = CellDef::from_raw(&subs, FORM_OFF_LIMITS, &Some);
        assert_eq!(c, CellDef { owner: Some(FormId(0x0010_4C78)), rank: Some(2), public: true, interior: true, off_limits: true, cant_wait: false });
        assert!(c.matters());
        // Can't wait (bit 19) alone is kept (Gomorrah00 and eight more in FalloutNV.esm).
        let gomorrah = CellDef::from_raw(&[sub(b"DATA", vec![0x01])], FORM_CANT_WAIT, &Some);
        assert!(gomorrah.cant_wait && !gomorrah.off_limits && gomorrah.matters());
        assert!(!CellDef::from_raw(&[sub(b"DATA", vec![0x01])], 0, &Some).matters());
    }
}
