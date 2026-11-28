//! The host's checks on the world's messages a guest sends, as far as they can be made
//! without the host's world: the [`crate::guard::Rule`]s the host's game registers on its
//! session with [`add_world_rules`] (`Link::add_rule` for each). What needs the world
//! (is the reference loaded, a door, within reach) is the game side's, after these.
//!
//! The guard's own checks come first ([`crate::guard`]): each kind's rate (hits, unlocks,
//! doors and revives per guest address; carries, drops and takes, digests and the
//! syncing screen's ask per guest and item kind), the world's host-only messages (Npcs,
//! Lines, Presence, Wear, Clock, Scene, Actors) dropped, a revive's perks cut to the
//! known bits, and every float of a message or a world item finite as it parses.
//!
//! The checks here, each [design]:
//! - **Hit** ([`check_hit`]): `dealt` finite, at least 0 and at most [`HIT_DEALT_MAX`];
//!   `location` -1 (no part), [`HitView::OBJECT`] or a body part, 0 to [`HIT_PART_MAX`];
//!   a limb's values finite, its health 0 to 100 percent; `weapon` 0 or one the host's
//!   [`crate::guard::FormCheck::carriable`] takes; and `weapon` the guest's weapon in hand by its last
//!   accepted state, or the one it held before for [`WEAPON_SWITCH_GRACE`] after a
//!   switch (a shot fired before it may land after it). A guest with no accepted state
//!   has no hits.
//! - **Carry** ([`check_carry`]): the pose finite, and within [`CARRY_REACH`] of the
//!   guest's feet by its last accepted state.
//! - **Drop** (`Loose::Dropped`, [`check_loose`]): `count` 1 to [`DROP_COUNT_MAX`]; the
//!   pose finite and within [`DROP_REACH`] of the guest's feet; `base` a playable
//!   inventory item ([`crate::guard::FormCheck::item`]). The cell is the game side's.
//! - **Take** (`Loose::Took`): nothing here; the game side checks the reference.
//! - **Made, Refused** (`Loose`), and any change ([`crate::repl::kind::CHANGES`]): the
//!   host's only; dropped from a guest.
//! - **Effects**: a guest's effects item holds the syncing screen's ask
//!   (`Effect::Sync { done: false }`) alone; any other effect is the host's only. The
//!   ask's rate is the guard's (one per 5 seconds).
//! - **Digest**: at most [`DIGEST_REFS_MAX`] references an item.
//! - **Revive**: `by` is the sender (the session takes the sender as the reviver either
//!   way; one that names another is a bad message). Another guest's revive is the host
//!   game's [`crate::net::Event::ReviveAsked`]; its target has it only through
//!   [`crate::net::Session::relay_revive`], after the game's checks.
//! - **Unlock, Door**: nothing beyond the rate here; the game side checks the reference,
//!   and only then passes it on ([`crate::net::Session::relay_unlock`],
//!   [`crate::net::Session::relay_door`]; [`crate::net::Session::note_bad`] for a refusal).
//!
//! A refusal's reason is [`Reason::Bounds`] for what does not parse or is not finite,
//! [`Reason::HostOnly`] for the host's own, and otherwise one of [`why`].

use std::time::Duration;

use crate::guard::{Ctx, Kind, LastState, Reason, Rule, Subject, Verdict};
use crate::net::{HitView, Msg};
use crate::repl::{self, kind, Carry, Effect, Loose};

/// The most damage one hit may deal [design: well above any vanilla weapon's critical
/// hit with every bonus on it].
pub const HIT_DEALT_MAX: f32 = 10_000.0;
/// The last body part index a hit's location may name [design: the body part list's
/// slots].
pub const HIT_PART_MAX: i32 = 14;
/// How long after a guest's switch of weapon its hits may still name the one before
/// [design: a slow projectile or a thrown grenade in flight at the switch].
pub const WEAPON_SWITCH_GRACE: Duration = Duration::from_secs(3);
/// How far from the guest's feet a carried object may be, game units [design: grab
/// reach, a throw's flight, and the state's lag].
pub const CARRY_REACH: f32 = 1_500.0;
/// How far from the guest's feet a dropped item may come to rest, game units [design: a
/// drop and a throw radius].
pub const DROP_REACH: f32 = 1_000.0;
/// The most of one item a drop may hold [design: one stack].
pub const DROP_COUNT_MAX: i32 = 65_535;
/// The most references one digest item may name [design: a crowded area's digest goes
/// in several items, each far under this].
pub const DIGEST_REFS_MAX: usize = 1_024;

/// The world rules' reasons ([`Reason::Rule`]).
pub mod why {
    pub const NO_STATE: &str = "no accepted state";
    pub const HIT_DEALT: &str = "hit damage out of bounds";
    pub const HIT_LOCATION: &str = "hit location unknown";
    pub const HIT_LIMB: &str = "hit limb out of bounds";
    pub const HIT_WEAPON_FORM: &str = "hit weapon not a weapon";
    pub const HIT_WEAPON_HELD: &str = "hit weapon not in hand";
    pub const CARRY_REACH: &str = "carry out of reach";
    pub const DROP_COUNT: &str = "drop count out of bounds";
    pub const DROP_REACH: &str = "drop out of reach";
    pub const DROP_ITEM: &str = "drop not an item";
    pub const EFFECTS_ONE: &str = "effects item not one ask";
    pub const DIGEST_REFS: &str = "digest too long";
    pub const REVIVE_BY: &str = "revive by another";
}

/// Register the world's checks: `add` is the host's [`crate::net::Link::add_rule`] (or
/// [`crate::net::Session::add_rule`], [`crate::guard::Guard::add_rule`]).
pub fn add_world_rules(add: &mut impl FnMut(Kind, Box<dyn Rule>)) {
    add(Kind::Hit, Box::new(HitRule));
    add(Kind::Revive, Box::new(ReviveRule));
    add(Kind::Item(kind::CARRY), Box::new(CarryRule));
    add(Kind::Item(kind::LOOSE), Box::new(LooseRule));
    add(Kind::Item(kind::EFFECTS), Box::new(EffectsRule));
    add(Kind::Item(kind::DIGEST), Box::new(DigestRule));
    add(Kind::Item(kind::CHANGES), Box::new(HostOnlyItem));
}

fn rule(why: &'static str) -> Verdict {
    Verdict::Drop(Reason::Rule(why))
}

fn finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Whether the guest of `last` may name `weapon` in a hit at `now`: the one in hand, or
/// the one before within [`WEAPON_SWITCH_GRACE`] of the switch.
fn holds(last: &LastState, weapon: u32, now: std::time::Instant) -> bool {
    weapon == last.weapon || last.weapon_before.is_some_and(|(w, at)| w == weapon && now.saturating_duration_since(at) <= WEAPON_SWITCH_GRACE)
}

/// A guest's hit on the host's world (module notes).
pub fn check_hit(ctx: &Ctx, hit: &HitView) -> Verdict {
    if !(hit.dealt.is_finite() && (0.0..=HIT_DEALT_MAX).contains(&hit.dealt)) {
        return rule(why::HIT_DEALT);
    }
    if !(hit.location == HitView::NO_PART || hit.location == HitView::OBJECT || (0..=HIT_PART_MAX).contains(&hit.location)) {
        return rule(why::HIT_LOCATION);
    }
    if let Some(l) = hit.limb {
        if !(l.damage.is_finite() && l.health_percent.is_finite() && (0.0..=100.0).contains(&l.health_percent)) {
            return rule(why::HIT_LIMB);
        }
    }
    if hit.weapon != 0 && ctx.forms.is_some_and(|f| !f.carriable(hit.weapon)) {
        return rule(why::HIT_WEAPON_FORM);
    }
    let Some(last) = ctx.last else { return rule(why::NO_STATE) };
    if !holds(&last, hit.weapon, ctx.now) {
        return rule(why::HIT_WEAPON_HELD);
    }
    Verdict::Accept
}

/// An object a guest's physics moves (module notes).
pub fn check_carry(ctx: &Ctx, c: &Carry) -> Verdict {
    if !finite(&c.pos) || !finite(&c.rot) {
        return Verdict::Drop(Reason::Bounds);
    }
    let Some(last) = ctx.last else { return rule(why::NO_STATE) };
    if distance(c.pos, last.feet) > CARRY_REACH {
        return rule(why::CARRY_REACH);
    }
    Verdict::Accept
}

/// A guest's dropped items, or its take (module notes).
pub fn check_loose(ctx: &Ctx, l: &Loose) -> Verdict {
    match *l {
        Loose::Dropped { base, count, pos, rot, .. } => {
            if !(1..=DROP_COUNT_MAX).contains(&count) {
                return rule(why::DROP_COUNT);
            }
            if !finite(&pos) || !finite(&rot) {
                return Verdict::Drop(Reason::Bounds);
            }
            if ctx.forms.is_some_and(|f| !f.item(base)) {
                return rule(why::DROP_ITEM);
            }
            let Some(last) = ctx.last else { return rule(why::NO_STATE) };
            if distance(pos, last.feet) > DROP_REACH {
                return rule(why::DROP_REACH);
            }
            Verdict::Accept
        }
        // The game side checks the reference.
        Loose::Took { .. } => Verdict::Accept,
        Loose::Made { .. } | Loose::Refused { .. } => Verdict::Drop(Reason::HostOnly),
    }
}

/// A guest's effects (module notes).
pub fn check_effects(effects: &[Effect]) -> Verdict {
    match effects {
        [Effect::Sync { done: false, .. }] => Verdict::Accept,
        [_] => Verdict::Drop(Reason::HostOnly),
        _ => rule(why::EFFECTS_ONE),
    }
}

/// The item a rule judges, unpacked; a message or an item that does not parse is out of
/// bounds.
fn item<T>(subject: &Subject, unpack: impl Fn(&[u8]) -> Option<T>) -> Result<T, Verdict> {
    match subject {
        Subject::Item(b) => unpack(b).ok_or(Verdict::Drop(Reason::Bounds)),
        Subject::Msg(_) => Err(Verdict::Drop(Reason::Bounds)),
    }
}

/// [`check_hit`] on [`Kind::Hit`].
pub struct HitRule;

impl Rule for HitRule {
    fn check(&self, ctx: &Ctx, subject: &Subject) -> Verdict {
        match subject {
            Subject::Msg(Msg::Hit { hit, .. }) => check_hit(ctx, hit),
            _ => Verdict::Drop(Reason::Bounds),
        }
    }
}

/// A revive's `by` is its sender, on [`Kind::Revive`].
pub struct ReviveRule;

impl Rule for ReviveRule {
    fn check(&self, ctx: &Ctx, subject: &Subject) -> Verdict {
        match subject {
            Subject::Msg(Msg::Revive { by, .. }) if ctx.from_id == Some(*by) => Verdict::Accept,
            Subject::Msg(Msg::Revive { .. }) => rule(why::REVIVE_BY),
            _ => Verdict::Drop(Reason::Bounds),
        }
    }
}

/// [`check_carry`] on [`kind::CARRY`] items.
pub struct CarryRule;

impl Rule for CarryRule {
    fn check(&self, ctx: &Ctx, subject: &Subject) -> Verdict {
        match item(subject, repl::unpack_carry) {
            Ok(c) => check_carry(ctx, &c),
            Err(v) => v,
        }
    }
}

/// [`check_loose`] on [`kind::LOOSE`] items.
pub struct LooseRule;

impl Rule for LooseRule {
    fn check(&self, ctx: &Ctx, subject: &Subject) -> Verdict {
        match item(subject, repl::unpack_loose) {
            Ok(l) => check_loose(ctx, &l),
            Err(v) => v,
        }
    }
}

/// [`check_effects`] on [`kind::EFFECTS`] items.
pub struct EffectsRule;

impl Rule for EffectsRule {
    fn check(&self, _: &Ctx, subject: &Subject) -> Verdict {
        match item(subject, repl::unpack_effects) {
            Ok(e) => check_effects(&e),
            Err(v) => v,
        }
    }
}

/// At most [`DIGEST_REFS_MAX`] references, on [`kind::DIGEST`] items.
pub struct DigestRule;

impl Rule for DigestRule {
    fn check(&self, _: &Ctx, subject: &Subject) -> Verdict {
        match item(subject, repl::unpack_digest) {
            Err(v) => v,
            Ok(d) if d.refs.len() > DIGEST_REFS_MAX => rule(why::DIGEST_REFS),
            Ok(_) => Verdict::Accept,
        }
    }
}

/// An item kind only the host sends.
pub struct HostOnlyItem;

impl Rule for HostOnlyItem {
    fn check(&self, _: &Ctx, _: &Subject) -> Verdict {
        Verdict::Drop(Reason::HostOnly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::{FormCheck, Guard};
    use crate::net::{LimbView, Place, PlayerState};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Instant;

    const GUN: u32 = 0x0E37;
    const KNIFE: u32 = 0x0E38;
    const BAD: u32 = 0x0BAD;

    struct Forms;

    impl FormCheck for Forms {
        fn carriable(&self, form: u32) -> bool {
            form == GUN || form == KNIFE
        }
        fn wearable(&self, _: u32) -> bool {
            true
        }
        fn furniture(&self, _: u32) -> bool {
            true
        }
        fn item(&self, form: u32) -> bool {
            form != BAD
        }
    }

    fn last(weapon: u32, at: Instant) -> LastState {
        LastState { place: Place::Exterior(0xDA726), feet: [0.0, 0.0, 100.0], weapon, weapon_before: None, at }
    }

    fn ctx<'a>(kind: Kind, last: Option<LastState>, now: Instant, forms: Option<&'a dyn FormCheck>) -> Ctx<'a> {
        Ctx { from_id: Some(1), kind, now, last, forms }
    }

    fn hit() -> HitView {
        HitView { target: 0x00AB_C104, dealt: 25.0, weapon: GUN, location: 1, critical: false, limb: Some(LimbView { condition: 25, health_percent: 50.0, damage: 10.0, points: true }) }
    }

    fn refused(why: &'static str) -> Verdict {
        Verdict::Drop(Reason::Rule(why))
    }

    #[test]
    fn a_plausible_hit_passes() {
        let now = Instant::now();
        let c = ctx(Kind::Hit, Some(last(GUN, now)), now, Some(&Forms));
        assert_eq!(check_hit(&c, &hit()), Verdict::Accept);
        for location in [HitView::NO_PART, HitView::OBJECT, 0, HIT_PART_MAX] {
            assert_eq!(check_hit(&c, &HitView { location, ..hit() }), Verdict::Accept, "{location}");
        }
        assert_eq!(check_hit(&c, &HitView { limb: None, dealt: 0.0, ..hit() }), Verdict::Accept);
        assert_eq!(check_hit(&c, &HitView { dealt: HIT_DEALT_MAX, ..hit() }), Verdict::Accept);
        // Unarmed: weapon 0 with nothing in hand.
        let c = ctx(Kind::Hit, Some(last(0, now)), now, Some(&Forms));
        assert_eq!(check_hit(&c, &HitView { weapon: 0, ..hit() }), Verdict::Accept);
    }

    #[test]
    fn a_hits_damage_out_of_bounds_is_dropped() {
        let now = Instant::now();
        let c = ctx(Kind::Hit, Some(last(GUN, now)), now, Some(&Forms));
        for dealt in [HIT_DEALT_MAX + 1.0, 1.0e9, -1.0, -0.001, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(check_hit(&c, &HitView { dealt, ..hit() }), refused(why::HIT_DEALT), "{dealt}");
        }
    }

    #[test]
    fn a_hits_location_must_be_known() {
        let now = Instant::now();
        let c = ctx(Kind::Hit, Some(last(GUN, now)), now, Some(&Forms));
        for location in [-2, -999, -1001, HIT_PART_MAX + 1, i32::MAX, i32::MIN] {
            assert_eq!(check_hit(&c, &HitView { location, ..hit() }), refused(why::HIT_LOCATION), "{location}");
        }
    }

    #[test]
    fn a_hits_limb_must_be_in_bounds() {
        let now = Instant::now();
        let c = ctx(Kind::Hit, Some(last(GUN, now)), now, Some(&Forms));
        let limb = |health_percent: f64, damage: f64| HitView { limb: Some(LimbView { condition: 25, health_percent, damage, points: false }), ..hit() };
        for (h, d) in [(100.5, 1.0), (-1.0, 1.0), (f64::NAN, 1.0), (50.0, f64::INFINITY), (50.0, f64::NAN)] {
            assert_eq!(check_hit(&c, &limb(h, d)), refused(why::HIT_LIMB), "{h} {d}");
        }
        assert_eq!(check_hit(&c, &limb(0.0, 0.0)), Verdict::Accept);
        assert_eq!(check_hit(&c, &limb(100.0, 0.0)), Verdict::Accept);
    }

    #[test]
    fn a_hits_weapon_must_be_one_and_in_hand() {
        let now = Instant::now();
        let c = ctx(Kind::Hit, Some(last(GUN, now)), now, Some(&Forms));
        assert_eq!(check_hit(&c, &HitView { weapon: BAD, ..hit() }), refused(why::HIT_WEAPON_FORM));
        // A weapon, but not the one in hand; fists while holding a gun.
        assert_eq!(check_hit(&c, &HitView { weapon: KNIFE, ..hit() }), refused(why::HIT_WEAPON_HELD));
        assert_eq!(check_hit(&c, &HitView { weapon: 0, ..hit() }), refused(why::HIT_WEAPON_HELD));
        // Without the host's form check only the hand is looked at.
        let c = ctx(Kind::Hit, Some(last(BAD, now)), now, None);
        assert_eq!(check_hit(&c, &HitView { weapon: BAD, ..hit() }), Verdict::Accept);
        // No accepted state: no hits.
        let c = ctx(Kind::Hit, None, now, Some(&Forms));
        assert_eq!(check_hit(&c, &hit()), refused(why::NO_STATE));
    }

    #[test]
    fn a_hit_of_the_weapon_before_a_switch_passes_for_a_while() {
        let now = Instant::now();
        let switched = LastState { weapon_before: Some((GUN, now)), ..last(KNIFE, now) };
        let at = |t: Duration| check_hit(&ctx(Kind::Hit, Some(switched), now + t, Some(&Forms)), &hit());
        assert_eq!(at(Duration::ZERO), Verdict::Accept);
        assert_eq!(at(WEAPON_SWITCH_GRACE), Verdict::Accept);
        assert_eq!(at(WEAPON_SWITCH_GRACE + Duration::from_millis(1)), refused(why::HIT_WEAPON_HELD));
    }

    #[test]
    fn a_carry_must_be_near_the_guest() {
        let now = Instant::now();
        let c = ctx(Kind::Item(kind::CARRY), Some(last(0, now)), now, None);
        let carry = |pos: [f32; 3]| Carry { reference: 0x00AB_C108, pos, rot: [0.0, 0.0, 1.0], done: false };
        assert_eq!(check_carry(&c, &carry([100.0, -50.0, 150.0])), Verdict::Accept);
        assert_eq!(check_carry(&c, &carry([CARRY_REACH - 1.0, 0.0, 100.0])), Verdict::Accept);
        assert_eq!(check_carry(&c, &carry([10_000.0, 0.0, 100.0])), refused(why::CARRY_REACH));
        assert_eq!(check_carry(&c, &carry([0.0, 0.0, 100.0 + CARRY_REACH + 1.0])), refused(why::CARRY_REACH));
        for bad in [f32::NAN, f32::INFINITY] {
            assert_eq!(check_carry(&c, &carry([bad, 0.0, 0.0])), Verdict::Drop(Reason::Bounds));
            assert_eq!(check_carry(&c, &Carry { rot: [0.0, bad, 0.0], ..carry([0.0; 3]) }), Verdict::Drop(Reason::Bounds));
        }
        assert_eq!(check_carry(&ctx(Kind::Item(kind::CARRY), None, now, None), &carry([0.0; 3])), refused(why::NO_STATE));
    }

    fn dropped(count: i32, pos: [f32; 3]) -> Loose {
        Loose::Dropped { local: 0xFF80_0001, base: 0x00AB_C109, count, cell: 0x33, pos, rot: [0.0, 0.0, 0.5] }
    }

    #[test]
    fn a_drop_must_be_an_item_near_the_guest() {
        let now = Instant::now();
        let c = ctx(Kind::Item(kind::LOOSE), Some(last(0, now)), now, Some(&Forms));
        let near = [50.0, 50.0, 100.0];
        assert_eq!(check_loose(&c, &dropped(1, near)), Verdict::Accept);
        assert_eq!(check_loose(&c, &dropped(DROP_COUNT_MAX, near)), Verdict::Accept);
        for count in [0, -1, i32::MIN, DROP_COUNT_MAX + 1, 1_000_000] {
            assert_eq!(check_loose(&c, &dropped(count, near)), refused(why::DROP_COUNT), "{count}");
        }
        assert_eq!(check_loose(&c, &dropped(1, [5_000.0, 0.0, 100.0])), refused(why::DROP_REACH));
        assert_eq!(check_loose(&c, &dropped(1, [f32::NAN, 0.0, 0.0])), Verdict::Drop(Reason::Bounds));
        let Loose::Dropped { local, count, cell, pos, rot, .. } = dropped(1, near) else { unreachable!() };
        assert_eq!(check_loose(&c, &Loose::Dropped { local, base: BAD, count, cell, pos, rot }), refused(why::DROP_ITEM));
        assert_eq!(check_loose(&ctx(Kind::Item(kind::LOOSE), None, now, None), &dropped(1, near)), refused(why::NO_STATE));
        // A take is the game side's to check; Made and Refused are the host's only.
        assert_eq!(check_loose(&c, &Loose::Took { reference: 0xFF00_0042 }), Verdict::Accept);
        assert_eq!(check_loose(&c, &Loose::Made { local: 1, host: 2 }), Verdict::Drop(Reason::HostOnly));
        assert_eq!(check_loose(&c, &Loose::Refused { reference: 1, base: 2, count: 3 }), Verdict::Drop(Reason::HostOnly));
    }

    #[test]
    fn a_guest_sends_only_the_syncing_ask() {
        assert_eq!(check_effects(&[Effect::Sync { serial: 1, done: false }]), Verdict::Accept);
        assert_eq!(check_effects(&[Effect::Sync { serial: 1, done: true }]), Verdict::Drop(Reason::HostOnly));
        assert_eq!(check_effects(&[Effect::Rules { friendly_fire: true }]), Verdict::Drop(Reason::HostOnly));
        assert_eq!(check_effects(&[Effect::Summon { interior: 1, world: 0, pos: [0.0; 3], yaw: 0.0 }]), Verdict::Drop(Reason::HostOnly));
        assert_eq!(check_effects(&[]), refused(why::EFFECTS_ONE));
        assert_eq!(check_effects(&[Effect::Sync { serial: 1, done: false }, Effect::Sync { serial: 2, done: false }]), refused(why::EFFECTS_ONE));
    }

    // ---- through the guard -------------------------------------------------------

    fn addr() -> SocketAddr {
        SocketAddr::from(([10, 0, 0, 1], 5000))
    }

    /// A guard with the world's rules and the host's forms, guest 1 standing at the
    /// origin holding `weapon`.
    fn guard(weapon: u32, now: Instant) -> Guard {
        let mut g = Guard::new(Some(Arc::new(Forms)));
        add_world_rules(&mut |k, r| g.add_rule(k, r));
        let mut s = PlayerState::at(Place::Exterior(0xDA726), [0.0, 0.0, 100.0], 0.0);
        s.weapon = weapon;
        assert_eq!(g.check(addr(), Some(1), &mut Msg::State { id: 1, seq: 1, state: s }, now), Verdict::Accept);
        g
    }

    #[test]
    fn the_world_rules_judge_through_the_guard() {
        let now = Instant::now();
        let mut g = guard(GUN, now);
        let msg = |h: HitView| Msg::Hit { id: 1, by: 0, hit: h };
        assert_eq!(g.check(addr(), Some(1), &mut msg(hit()), now), Verdict::Accept);
        // An over-cap hit is dropped, counted and held against the guest.
        assert_eq!(g.check(addr(), Some(1), &mut msg(HitView { dealt: 1.0e6, ..hit() }), now), refused(why::HIT_DEALT));
        assert_eq!(g.check(addr(), Some(1), &mut msg(HitView { weapon: KNIFE, ..hit() }), now), refused(why::HIT_WEAPON_HELD));
        assert!(g.stats().dropped.contains(&(Kind::Hit, Reason::Rule(why::HIT_DEALT), 1)));
        assert_eq!(g.bad(1), 2);
        // A switch of weapon in a later state: the gun still lands for a while.
        let mut s = PlayerState::at(Place::Exterior(0xDA726), [1.0, 0.0, 100.0], 0.0);
        s.weapon = KNIFE;
        let later = now + Duration::from_millis(50);
        assert_eq!(g.check(addr(), Some(1), &mut Msg::State { id: 1, seq: 2, state: s }, later), Verdict::Accept);
        assert_eq!(g.check(addr(), Some(1), &mut msg(hit()), later), Verdict::Accept);
        assert_eq!(g.check(addr(), Some(1), &mut msg(HitView { weapon: KNIFE, ..hit() }), later), Verdict::Accept);
        let much_later = later + WEAPON_SWITCH_GRACE + Duration::from_secs(1);
        assert_eq!(g.check(addr(), Some(1), &mut msg(hit()), much_later), refused(why::HIT_WEAPON_HELD));
    }

    #[test]
    fn a_revive_must_name_its_sender() {
        let now = Instant::now();
        let mut g = guard(0, now);
        assert_eq!(g.check(addr(), Some(1), &mut Msg::Revive { target: 0, by: 1, perks: 0 }, now), Verdict::Accept);
        assert_eq!(g.check(addr(), Some(1), &mut Msg::Revive { target: 0, by: 2, perks: 0 }, now), refused(why::REVIVE_BY));
        assert_eq!(g.check(addr(), Some(1), &mut Msg::Revive { target: 2, by: 0, perks: 0 }, now), refused(why::REVIVE_BY));
    }

    #[test]
    fn unlocks_and_doors_are_held_to_their_rate() {
        let now = Instant::now();
        let mut g = guard(0, now);
        let unlock = crate::guard::default_rate(Kind::Unlock).unwrap();
        let passed = (0..1000).filter(|_| g.check(addr(), Some(1), &mut Msg::Unlock { reference: 7, terminal: false }, now) == Verdict::Accept).count();
        assert_eq!(passed, unlock.burst as usize);
        let door = crate::guard::default_rate(Kind::Door).unwrap();
        let passed = (0..1000).filter(|_| g.check(addr(), Some(1), &mut Msg::Door { reference: 7, open: true }, now) == Verdict::Accept).count();
        assert_eq!(passed, door.burst as usize);
        // The rate is not held against the guest.
        assert_eq!(g.bad(1), 0);
    }

    #[test]
    fn items_far_or_broken_are_dropped() {
        let now = Instant::now();
        let mut g = guard(0, now);
        let carry = |pos: [f32; 3]| repl::pack_carry(&Carry { reference: 0x00AB_C108, pos, rot: [0.0; 3], done: false });
        assert_eq!(g.admit_item(1, &carry([10.0, 0.0, 100.0]), now), Verdict::Accept);
        assert_eq!(g.admit_item(1, &carry([10_000.0, 0.0, 100.0]), now), refused(why::CARRY_REACH));
        // A NaN does not parse: out of bounds.
        assert_eq!(g.admit_item(1, &carry([f32::NAN, 0.0, 100.0]), now), Verdict::Drop(Reason::Bounds));
        assert_eq!(g.admit_item(1, &repl::pack_loose(&dropped(0, [0.0; 3])), now), refused(why::DROP_COUNT));
        assert_eq!(g.admit_item(1, &repl::pack_loose(&dropped(-3, [0.0; 3])), now), refused(why::DROP_COUNT));
        assert_eq!(g.admit_item(1, &repl::pack_loose(&dropped(1_000_000, [0.0; 3])), now), refused(why::DROP_COUNT));
        assert_eq!(g.admit_item(1, &repl::pack_loose(&dropped(1, [0.0, f32::NAN, 0.0])), now), Verdict::Drop(Reason::Bounds));
        assert_eq!(g.admit_item(1, &repl::pack_loose(&dropped(5, [0.0, 0.0, 50.0])), now), Verdict::Accept);
        assert_eq!(g.admit_item(1, &repl::pack_loose(&Loose::Made { local: 1, host: 2 }), now), Verdict::Drop(Reason::HostOnly));
        // Digests: at most DIGEST_REFS_MAX references.
        let digest = |n: usize| [vec![kind::DIGEST], (0..n as u32).flat_map(|i| [i.to_le_bytes(), 7u32.to_le_bytes()]).flatten().collect()].concat();
        assert_eq!(g.admit_item(1, &digest(DIGEST_REFS_MAX), now), Verdict::Accept);
        assert_eq!(g.admit_item(1, &digest(DIGEST_REFS_MAX + 1), now), refused(why::DIGEST_REFS));
        assert_eq!(g.admit_item(1, &[kind::DIGEST, 1, 2, 3], now), Verdict::Drop(Reason::Bounds));
        // Changes are the host's.
        let change = repl::pack_changes(&[repl::Change { reference: 1, value: repl::Value::Open(true) }], 1100).remove(0);
        assert_eq!(g.admit_item(1, &change, now), Verdict::Drop(Reason::HostOnly));
    }

    #[test]
    fn a_second_syncing_ask_inside_its_cooldown_is_dropped() {
        let now = Instant::now();
        let mut g = guard(0, now);
        let ask = |serial: u32| repl::pack_effects(&[Effect::Sync { serial, done: false }], 1100).remove(0);
        assert_eq!(g.admit_item(1, &ask(1), now), Verdict::Accept);
        assert_eq!(g.admit_item(1, &ask(2), now + Duration::from_secs(4)), Verdict::Drop(Reason::Rate));
        assert_eq!(g.admit_item(1, &ask(3), now + Duration::from_secs(10)), Verdict::Accept);
        // Any other effect from a guest is the host's only.
        let rules = repl::pack_effects(&[Effect::Rules { friendly_fire: true }], 1100).remove(0);
        assert_eq!(g.admit_item(1, &rules, now + Duration::from_secs(20)), Verdict::Drop(Reason::HostOnly));
        // A guest that says Hello again (its game started again) may ask at once.
        assert_eq!(g.admit_item(1, &ask(4), now + Duration::from_secs(30)), Verdict::Accept);
        g.forget(1);
        assert_eq!(g.admit_item(1, &ask(1), now + Duration::from_secs(31)), Verdict::Accept);
        // Another guest has its own.
        assert_eq!(g.admit_item(2, &ask(1), now + Duration::from_secs(31)), Verdict::Accept);
    }

    // ---- through the session -----------------------------------------------------

    /// Over the in-memory transport: a guest's carry far away and a second syncing ask
    /// never reach the host's game; the near carry and the first ask do.
    #[test]
    fn the_host_session_drops_what_the_world_rules_refuse() {
        use crate::net::{Event, Session};
        use crate::seam::memory::pair;
        use crate::seam::SharedSecret;
        let (h, g) = pair(SocketAddr::from(([10, 0, 0, 1], 1)), SocketAddr::from(([10, 0, 0, 2], 2)));
        let secret = || SharedSecret::new(0x5EC2_E7AB_CDEF_0123).unwrap();
        let mut host = Session::host_on(Box::new(h), Box::new(secret()));
        add_world_rules(&mut |k, r| host.add_rule(k, r));
        let mut a = Session::join_on(Box::new(g), Box::new(secret()), &[SocketAddr::from(([10, 0, 0, 1], 1))], 0, "A", 1);
        a.set_state(PlayerState::at(Place::Exterior(0xDA726), [0.0, 0.0, 100.0], 0.0));
        let mut clock = Instant::now();
        let mut seen: Vec<Event> = Vec::new();
        let tick = |host: &mut Session, a: &mut Session, seen: &mut Vec<Event>, clock: &mut Instant| {
            *clock += Duration::from_millis(10);
            host.tick(*clock);
            a.tick(*clock);
            seen.extend(host.take_events());
            a.take_events();
        };
        // Welcomed, its state taken, and the host's stream heard (the guest's goes then).
        for _ in 0..50 {
            tick(&mut host, &mut a, &mut seen, &mut clock);
        }
        assert!(seen.iter().any(|e| matches!(e, Event::State { id: 1, .. })), "{seen:?}");
        assert!(host.send_reliable(Some(1), vec![kind::CHANGES]));
        for _ in 0..20 {
            tick(&mut host, &mut a, &mut seen, &mut clock);
        }
        let near = repl::pack_carry(&Carry { reference: 0x10, pos: [10.0, 0.0, 100.0], rot: [0.0; 3], done: false });
        let far = repl::pack_carry(&Carry { reference: 0x11, pos: [10_000.0, 0.0, 100.0], rot: [0.0; 3], done: false });
        let ask = repl::pack_effects(&[Effect::Sync { serial: 1, done: false }], 1100).remove(0);
        for item in [near.clone(), far.clone(), ask.clone(), ask.clone()] {
            assert!(a.send_reliable(None, item));
        }
        seen.clear();
        for _ in 0..50 {
            tick(&mut host, &mut a, &mut seen, &mut clock);
        }
        let items: Vec<Vec<u8>> = seen.iter().filter_map(|e| if let Event::Reliable { id: 1, item } = e { Some(item.clone()) } else { None }).collect();
        assert_eq!(items, vec![near, ask]);
        let stats = host.guard_stats();
        assert!(stats.dropped.contains(&(Kind::Item(kind::CARRY), Reason::Rule(why::CARRY_REACH), 1)), "{stats:?}");
        assert!(stats.dropped.contains(&(Kind::Item(kind::EFFECTS), Reason::Rate, 1)), "{stats:?}");
        assert_eq!(host.bad_messages(1), 1);
    }
}
