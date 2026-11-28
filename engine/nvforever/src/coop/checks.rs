//! The host's checks of a guest's world messages against its own world [design].
//!
//! The network core already drops what no world is needed to refuse: a message too
//! large, too often, or naming a form that is not what it says by the host's load order
//! (`nvf_coop::guard`, `nvf_coop::world`). What is left needs the host's world: is the
//! thing a guest hit, unlocked, opened, revived, dropped, took or carried there, loaded,
//! of the right kind, and within the guest's reach from where the guest last stood? Each
//! check here is a pure function over a small view of that world ([`HostWorld`]),
//! implemented on the host's simulation ([`SimWorld`]) and on a fake world in the tests.
//! `coop.rs` and `replication.rs` call them before acting on a guest's message.
//!
//! **On a refusal** the message is dropped, the reason is counted
//! ([`HostChecks::refused`], shown with the replication readout) and logged at most once a
//! second per reason (`co-op check: <reason>`). The link has no way yet to mark a guest
//! for bad messages from here, so only the count is kept.
//!
//! **Where the guest is** comes from its newest state the link accepted (`Coop::peers`),
//! which `nvf_coop::guard` already checked against its speed. The checks allow slack for
//! the state's age ([`SLACK`]).
//!
//! The reaches are generous on purpose: a check that refuses an honest hit is worse than
//! one that lets an odd one by. They are bounds, not the game's own numbers [design].

use nvf_coop::net::HitView;
use nvf_coop::repl::Carry;
use std::collections::{BTreeMap, HashMap, VecDeque};

/// Units added to every reach: the guest's state is up to a few frames old, and the
/// thing's place is the host's copy of it [design].
pub const SLACK: f32 = 256.0;
/// How far a hit with no weapon (unarmed) or one this world has no data for may land
/// from the guest (units) [design: well past any shotgun's or rifle's range in the
/// load order, far under the whole of a loaded exterior area].
pub const HIT_RANGE_MAX: f32 = 12_000.0;
/// A melee weapon's reach before its own multiplier (units), when the world gives none
/// [design: fCombatDistance's usual value, as `fnv_combat::CombatTuning::combat_distance`].
pub const MELEE_REACH: f32 = 140.0;
/// How far beyond its reach a melee hit may land: lunges, power attacks and the guest's
/// own movement since its state [design].
pub const MELEE_LUNGE: f32 = 300.0;
/// What a hit may deal at most, over its weapon's base damage times its projectiles and
/// its critical multiplier: skills, perks, sneak attacks, ammo, mods and difficulty
/// raise it [design: generous, the guard's own bound `nvf_coop::world::HIT_DEALT_MAX`
/// still caps it].
pub const DAMAGE_BONUS: f32 = 12.0;
/// What an unarmed hit may deal at most [design: about a strong character's power
/// attack with the unarmed perks, times a sneak critical].
pub const UNARMED_DEALT_MAX: f32 = 400.0;
/// How far a player can activate (open, unlock, hack) from (units) [design: about
/// iActivatePickLength's 150 and a crosshair over a large door's middle, with room].
pub const ACTIVATE_REACH: f32 = 300.0;
/// How far a reviver may be from the downed body (units) [design: the crosshair's
/// revive reach, `nvf_play::coop_mates::REVIVE_REACH`, with room].
pub const REVIVE_REACH: f32 = 400.0;
/// How far a guest may pick up a player's dropped item from (units) [design].
pub const TAKE_REACH: f32 = 300.0;
/// The most items a guest's drops may keep lying in the host's world; the oldest goes
/// when another comes [design].
pub const DROPS_KEPT: usize = 256;
/// How far one carry message may move an object (units) [design: a thrown object's
/// flight between two messages at the carry rate, with room].
pub const CARRY_STEP_MAX: f32 = 600.0;
/// A carry's last place counts for this long (seconds); after it a carry is measured
/// from where the object lies [design].
pub const CARRY_FRESH: f32 = 2.0;
/// How many references a guest's digests may have the host send again in a second
/// [design: a digest's references at the guard's digest rate, one in four found
/// different].
pub const FORCED_PER_SECOND: u32 = 256;

/// Why a guest's message was refused (the counter's names and the log's words).
pub mod why {
    pub const HIT_TARGET: &str = "hit on nothing loaded";
    pub const HIT_SPACE: &str = "hit in another space";
    pub const HIT_RANGE: &str = "hit out of the weapon's reach";
    pub const HIT_DEALT: &str = "hit dealt more than its weapon can";
    pub const HIT_KIND: &str = "hit on a thing that takes no hits";
    pub const UNLOCK_TARGET: &str = "unlock of nothing loaded";
    pub const UNLOCK_KIND: &str = "unlock of a thing with no lock";
    pub const UNLOCK_KEY: &str = "unlock of a lock only its key opens";
    pub const UNLOCK_REACH: &str = "unlock out of reach";
    pub const DOOR_TARGET: &str = "door of nothing loaded";
    pub const DOOR_KIND: &str = "door that is not a door";
    pub const DOOR_LOCKED: &str = "door locked";
    pub const DOOR_REACH: &str = "door out of reach";
    pub const REVIVE_UP: &str = "revive of a player who is not down";
    pub const REVIVE_REACH: &str = "revive out of reach";
    pub const DROP_CELL: &str = "drop in another space";
    pub const TAKE_TARGET: &str = "take of no player's dropped item";
    pub const TAKE_REACH: &str = "take out of reach";
    pub const CARRY_TARGET: &str = "carry of nothing loaded";
    pub const CARRY_KIND: &str = "carry of a thing that cannot be carried";
    pub const CARRY_STEP: &str = "carry moved too far at once";
    pub const DIGEST_RESENDS: &str = "digest asks too many resends";
}

/// What a placed thing is, as far as the checks go (by its base record).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThingKind {
    /// An NPC or a creature (a guest's stand-in too).
    Actor,
    Door,
    Container,
    Terminal,
    Furniture,
    Activator,
    /// Something a player can carry (`HostForms::item`'s records).
    Item,
    /// A static, a light, a sound, a marker: nothing a player acts on.
    Other,
}

/// A loaded thing of the host's world: in the world, enabled and not taken away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thing {
    pub kind: ThingKind,
    /// Its space as the network names it (`NpcState::space`): its interior cell, or the
    /// worldspace of its exterior cell.
    pub space: u32,
    pub pos: [f32; 3],
    /// Locked now.
    pub locked: bool,
    /// Its lock opens only with its key (level 255, or broken).
    pub key_only: bool,
    /// A player's dropped item (`RefState::player_drop`).
    pub player_drop: bool,
    /// A loose object the physics moves (a clutter item, a havok-enabled static).
    pub loose: bool,
}

/// A weapon's numbers, as far as a hit's bounds go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weapon {
    pub melee: bool,
    /// Melee: its reach multiplier (×[`MELEE_REACH`]).
    pub reach: f32,
    /// A gun's maximum range (units); 0 for none given.
    pub range: f32,
    /// Its base damage per shot or swing.
    pub damage: f32,
    /// Projectiles per shot (at least 1).
    pub projectiles: f32,
    /// Its critical multiplier (at least 1).
    pub crit_mult: f32,
    /// Its Limb Dmg Mult: a limb takes the hit's damage times this.
    pub limb_mult: f32,
}

/// The view of the host's world the checks need.
pub trait HostWorld {
    /// The thing `reference` is, when it is loaded in the host's world.
    fn thing(&self, reference: u32) -> Option<Thing>;
    /// Weapon `form`'s numbers, when the host's load order has it.
    fn weapon(&self, form: u32) -> Option<Weapon>;
}

/// A guest as the checks take it: where its newest accepted state put it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guest {
    /// Its space, named as [`Thing::space`].
    pub space: u32,
    pub feet: [f32; 3],
}

pub type Verdict = Result<(), &'static str>;

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Within `reach` (plus [`SLACK`]) of the guest, in its space.
fn near(g: &Guest, t: &Thing, reach: f32) -> bool {
    t.space == g.space && dist(g.feet, t.pos) <= reach + SLACK
}

/// A guest's hit (`Msg::Hit`): on a loaded actor, or, for a shot on an object
/// ([`HitView::OBJECT`]), on a loaded placed thing, in the guest's space; within its
/// weapon's reach; dealing no more than the weapon can.
pub fn hit(w: &dyn HostWorld, g: &Guest, h: &HitView) -> Verdict {
    let t = w.thing(h.target).ok_or(why::HIT_TARGET)?;
    let object = h.location == HitView::OBJECT;
    match (object, t.kind) {
        (false, ThingKind::Actor) => {}
        (true, ThingKind::Actor | ThingKind::Other) => return Err(why::HIT_KIND),
        (true, _) => {}
        (false, _) => return Err(why::HIT_KIND),
    }
    if t.space != g.space {
        return Err(why::HIT_SPACE);
    }
    let weapon = (h.weapon != 0).then(|| w.weapon(h.weapon)).flatten();
    let reach = match weapon {
        Some(wp) if wp.melee => MELEE_REACH * wp.reach.max(1.0) + MELEE_LUNGE,
        Some(wp) if wp.range > 0.0 => wp.range.min(HIT_RANGE_MAX),
        // No weapon, or one the load order has but this world cannot resolve: the
        // unarmed reach [design].
        _ => MELEE_REACH + MELEE_LUNGE,
    };
    if dist(g.feet, t.pos) > reach + SLACK {
        return Err(why::HIT_RANGE);
    }
    let most = match weapon {
        Some(wp) => (wp.damage.max(1.0) * wp.projectiles.max(1.0) * wp.crit_mult.max(1.0) * DAMAGE_BONUS).max(UNARMED_DEALT_MAX),
        // Unarmed, or a weapon with no numbers here: the unarmed bound.
        None => UNARMED_DEALT_MAX,
    };
    if h.dealt > most {
        return Err(why::HIT_DEALT);
    }
    Ok(())
}

/// The most limb damage a hit that `dealt` with `weapon` may carry: the damage times the
/// weapon's Limb Dmg Mult (the original allows a limb to take more than the hit), or the
/// damage itself for a weapon this world cannot resolve.
pub fn limb_cap(w: &dyn HostWorld, weapon: u32, dealt: f32) -> f64 {
    let mult = (weapon != 0).then(|| w.weapon(weapon)).flatten().map_or(1.0, |wp| wp.limb_mult.max(1.0));
    f64::from(dealt.max(0.0)) * f64::from(mult)
}

/// A guest's unlock (`Msg::Unlock`): a loaded door or container that is locked, or a
/// terminal (`terminal`), within reach to activate. A lock only its key opens is refused:
/// the host does not know a guest's inventory, so it cannot tell the guest holds the key
/// [design].
pub fn unlock(w: &dyn HostWorld, g: &Guest, reference: u32, terminal: bool) -> Verdict {
    let t = w.thing(reference).ok_or(why::UNLOCK_TARGET)?;
    match (terminal, t.kind) {
        (true, ThingKind::Terminal) => {}
        (false, ThingKind::Door | ThingKind::Container) => {
            if !t.locked {
                // Unlocked already (by another player, or the message came twice):
                // nothing to do, and nothing wrong.
                return Ok(());
            }
            if t.key_only {
                return Err(why::UNLOCK_KEY);
            }
        }
        _ => return Err(why::UNLOCK_KIND),
    }
    if !near(g, &t, ACTIVATE_REACH) {
        return Err(why::UNLOCK_REACH);
    }
    Ok(())
}

/// A guest's door (`Msg::Door`): a loaded door, not locked, within reach to activate.
pub fn door(w: &dyn HostWorld, g: &Guest, reference: u32) -> Verdict {
    let t = w.thing(reference).ok_or(why::DOOR_TARGET)?;
    if t.kind != ThingKind::Door {
        return Err(why::DOOR_KIND);
    }
    if t.locked {
        return Err(why::DOOR_LOCKED);
    }
    if !near(g, &t, ACTIVATE_REACH) {
        return Err(why::DOOR_REACH);
    }
    Ok(())
}

/// A guest's revive of a player (`Msg::Revive`): the player down now, and the reviver
/// within reach of where its body lies (`body`: space and place).
pub fn revive(down_now: bool, reviver: &Guest, body: (u32, [f32; 3])) -> Verdict {
    if !down_now {
        return Err(why::REVIVE_UP);
    }
    if reviver.space != body.0 || dist(reviver.feet, body.1) > REVIVE_REACH + SLACK {
        return Err(why::REVIVE_REACH);
    }
    Ok(())
}

/// A guest's drop (`Loose::Dropped`): in the space its last state put it in
/// (`guest_space`: the cell indoors, the worldspace outdoors; None when the host cannot
/// tell: then the guard's reach check stands alone). `space` is the drop's.
pub fn dropped(guest_space: Option<u32>, space: u32) -> Verdict {
    match guest_space {
        Some(c) if c != space => Err(why::DROP_CELL),
        _ => Ok(()),
    }
}

/// A guest's pickup (`Loose::Took`): a player's dropped item lying in the guest's space
/// within [`TAKE_REACH`]. One already taken is not loaded: the caller answers that with
/// `Loose::Refused` before asking this.
pub fn took(w: &dyn HostWorld, g: &Guest, reference: u32) -> Verdict {
    let t = w.thing(reference).filter(|t| t.player_drop).ok_or(why::TAKE_TARGET)?;
    if !near(g, &t, TAKE_REACH) {
        return Err(why::TAKE_REACH);
    }
    Ok(())
}

/// A guest's carry (`Carry`): a loaded loose object (never an actor, door, furniture or
/// activator, nor a static the physics does not move) in the guest's space, moved no
/// further than [`CARRY_STEP_MAX`] since its last carry message (`last`).
pub fn carry(w: &dyn HostWorld, g: &Guest, c: &Carry, last: Option<[f32; 3]>) -> Verdict {
    let t = w.thing(c.reference).ok_or(why::CARRY_TARGET)?;
    let carriable = match t.kind {
        ThingKind::Item => true,
        ThingKind::Other => t.loose,
        _ => false,
    };
    if !carriable {
        return Err(why::CARRY_KIND);
    }
    if t.space != g.space {
        return Err(why::CARRY_TARGET);
    }
    let from = last.unwrap_or(t.pos);
    if dist(from, c.pos) > CARRY_STEP_MAX {
        return Err(why::CARRY_STEP);
    }
    Ok(())
}

/// What the host keeps for its checks: the refusals counted, each guest's drops lying in
/// its world, the forced resends each guest's digests asked for this second, and the
/// last place each carried object was put.
#[derive(Default)]
pub struct HostChecks {
    /// Refusals by reason.
    pub refused: BTreeMap<&'static str, u64>,
    /// When each reason was last logged (seconds).
    logged: HashMap<&'static str, f32>,
    drops: HashMap<u8, VecDeque<u32>>,
    forced: HashMap<u8, (f32, u32)>,
    /// Where each guest last put each object it carries, and when (seconds).
    carried: HashMap<(u8, u32), (f32, [f32; 3])>,
    /// The guests refused since the link last heard of it (`Link::note_bad`).
    pub bad: Vec<u8>,
}

impl HostChecks {
    /// A guest's message refused for `why` at `now` (seconds): counted; whether to log it
    /// (at most once a second per reason).
    pub fn refuse(&mut self, why: &'static str, now: f32) -> bool {
        *self.refused.entry(why).or_default() += 1;
        let log = self.logged.get(why).is_none_or(|&t| now - t >= 1.0);
        if log {
            self.logged.insert(why, now);
        }
        log
    }

    /// Refusals in all.
    pub fn total(&self) -> u64 {
        self.refused.values().sum()
    }

    /// Guest `id`'s drop `made` lies in the host's world now; the oldest of its drops
    /// to take away, once it has more than [`DROPS_KEPT`].
    pub fn dropped(&mut self, id: u8, made: u32) -> Option<u32> {
        let d = self.drops.entry(id).or_default();
        d.push_back(made);
        (d.len() > DROPS_KEPT).then(|| d.pop_front()).flatten()
    }

    /// A player's drop taken away (picked up by anyone): no longer counted.
    pub fn taken(&mut self, reference: u32) {
        for d in self.drops.values_mut() {
            d.retain(|&r| r != reference);
        }
    }

    /// Whether guest `id`'s digest may have one more reference sent again at `now`
    /// (seconds): at most [`FORCED_PER_SECOND`] a second.
    pub fn force(&mut self, id: u8, now: f32) -> bool {
        let (since, n) = self.forced.entry(id).or_insert((now, 0));
        if now - *since >= 1.0 {
            *since = now;
            *n = 0;
        }
        if *n >= FORCED_PER_SECOND {
            return false;
        }
        *n += 1;
        true
    }

    /// Where guest `id` last put `reference` while carrying it, if that was within
    /// [`CARRY_FRESH`] of `now` (seconds); an older one is not a step to measure from.
    pub fn last_carry(&self, id: u8, reference: u32, now: f32) -> Option<[f32; 3]> {
        self.carried.get(&(id, reference)).filter(|(at, _)| now - at < CARRY_FRESH).map(|&(_, pos)| pos)
    }

    /// Guest `id` carried `reference` to `pos` at `now`; `done`: it put it down.
    pub fn note_carry(&mut self, id: u8, reference: u32, pos: [f32; 3], done: bool, now: f32) {
        if done {
            self.carried.remove(&(id, reference));
        } else {
            self.carried.insert((id, reference), (now, pos));
        }
    }

    /// A guest left: what was kept for it goes (its drops stay in the world).
    pub fn forget(&mut self, id: u8) {
        self.drops.remove(&id);
        self.forced.remove(&id);
        self.carried.retain(|&(g, _), _| g != id);
    }
}

/// The host's simulation as [`HostWorld`]: the world's references and the combat data.
pub struct SimWorld<'a> {
    pub sim: &'a nvf_play::sim::SimState,
    pub combat: Option<&'a nvf_play::combat::Combat>,
    /// The physics, for what it moves as one dynamic body (placed clutter, a dropped
    /// item: what `nvf_play::grab` can hold).
    pub phys: Option<&'a nvf_play::phys::PhysState>,
}

impl SimWorld<'_> {
    fn kind_of(&self, base: fnv_sim::FormId) -> ThingKind {
        let Some(sig) = self.sim.forms.sig(base) else { return ThingKind::Other };
        match &sig {
            b"NPC_" | b"CREA" => ThingKind::Actor,
            b"DOOR" => ThingKind::Door,
            b"CONT" => ThingKind::Container,
            b"TERM" => ThingKind::Terminal,
            b"FURN" => ThingKind::Furniture,
            b"ACTI" | b"TACT" => ThingKind::Activator,
            s if super::ITEM_SIGS.contains(s) => ThingKind::Item,
            _ => ThingKind::Other,
        }
    }
}

impl HostWorld for SimWorld<'_> {
    fn thing(&self, reference: u32) -> Option<Thing> {
        let w = &self.sim.world;
        let r = fnv_sim::FormId(reference);
        let s = w.peek(r)?;
        if s.deleted() || !w.is_enabled(r).unwrap_or(false) {
            return None;
        }
        let kind = self.kind_of(s.base);
        let lock = s.lock.as_ref();
        Some(Thing {
            kind,
            space: super::space_of_ref(w, r),
            pos: s.pos,
            locked: lock.is_some_and(|l| l.locked),
            key_only: lock.is_some_and(|l| l.locked && l.level == u8::MAX),
            player_drop: s.player_drop,
            loose: self.phys.is_some_and(|p| p.placed_dynamic.contains_key(&reference) || p.created.contains_key(&reference)),
        })
    }

    fn weapon(&self, form: u32) -> Option<Weapon> {
        let c = self.combat?;
        let wp = c.forms.weapons.get(&fnv_sim::FormId(form))?;
        let melee = matches!(wp.anim_type.as_deref().and_then(fnv_actor::weapon::AnimType::from_name), Some(fnv_actor::weapon::AnimType::HandToHand | fnv_actor::weapon::AnimType::Melee1 | fnv_actor::weapon::AnimType::Melee2));
        let reach = if melee { wp.reach * c.rules.combat_distance.max(MELEE_REACH) / MELEE_REACH } else { wp.reach };
        Some(Weapon { melee, reach, range: wp.max_range, damage: wp.base_damage as f32, projectiles: wp.projectile_count.max(1) as f32, crit_mult: wp.crit_mult.max(1.0), limb_mult: wp.limb_dmg_mult })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world of a few things and weapons, for the checks.
    #[derive(Default)]
    struct Fake {
        things: HashMap<u32, Thing>,
        weapons: HashMap<u32, Weapon>,
    }

    impl HostWorld for Fake {
        fn thing(&self, reference: u32) -> Option<Thing> {
            self.things.get(&reference).copied()
        }
        fn weapon(&self, form: u32) -> Option<Weapon> {
            self.weapons.get(&form).copied()
        }
    }

    const SPACE: u32 = 0xDA726;
    const NPC: u32 = 0x100;
    const DOOR: u32 = 0x101;
    const SAFE: u32 = 0x102;
    const TERMINAL: u32 = 0x103;
    const ROCK: u32 = 0x104;
    const DROP: u32 = 0x105;
    const BED: u32 = 0x106;
    const CAN: u32 = 0x107;
    const PISTOL: u32 = 0x200;
    const KNIFE: u32 = 0x201;

    fn thing(kind: ThingKind, pos: [f32; 3]) -> Thing {
        Thing { kind, space: SPACE, pos, locked: false, key_only: false, player_drop: false, loose: false }
    }

    fn world() -> Fake {
        let mut w = Fake::default();
        w.things.insert(NPC, thing(ThingKind::Actor, [1000.0, 0.0, 0.0]));
        w.things.insert(DOOR, thing(ThingKind::Door, [100.0, 0.0, 0.0]));
        w.things.insert(SAFE, Thing { locked: true, ..thing(ThingKind::Container, [100.0, 0.0, 0.0]) });
        w.things.insert(TERMINAL, thing(ThingKind::Terminal, [100.0, 0.0, 0.0]));
        w.things.insert(ROCK, thing(ThingKind::Other, [50.0, 0.0, 0.0]));
        w.things.insert(DROP, Thing { player_drop: true, ..thing(ThingKind::Item, [100.0, 0.0, 0.0]) });
        w.things.insert(BED, thing(ThingKind::Furniture, [100.0, 0.0, 0.0]));
        w.things.insert(CAN, thing(ThingKind::Item, [100.0, 0.0, 0.0]));
        w.weapons.insert(PISTOL, Weapon { melee: false, reach: 1.0, range: 4000.0, damage: 20.0, projectiles: 1.0, crit_mult: 2.0, limb_mult: 2.0 });
        w.weapons.insert(KNIFE, Weapon { melee: true, reach: 1.0, range: 0.0, damage: 10.0, projectiles: 1.0, crit_mult: 1.0, limb_mult: 1.0 });
        w
    }

    fn guest() -> Guest {
        Guest { space: SPACE, feet: [0.0; 3] }
    }

    fn shot(target: u32, weapon: u32, dealt: f32) -> HitView {
        HitView { target, dealt, weapon, location: 0, critical: false, limb: None }
    }

    #[test]
    fn a_hit_in_reach_passes() {
        let w = world();
        assert_eq!(hit(&w, &guest(), &shot(NPC, PISTOL, 30.0)), Ok(()));
        // A shot on a placed object (its hit blocks): a door, a container.
        assert_eq!(hit(&w, &guest(), &HitView { location: HitView::OBJECT, ..shot(DOOR, PISTOL, 30.0) }), Ok(()));
        // Unarmed, at arm's length.
        let mut w = w;
        w.things.insert(NPC, thing(ThingKind::Actor, [120.0, 0.0, 0.0]));
        assert_eq!(hit(&w, &guest(), &shot(NPC, 0, 15.0)), Ok(()));
    }

    #[test]
    fn a_hit_on_nothing_loaded_is_refused() {
        assert_eq!(hit(&world(), &guest(), &shot(0x999, PISTOL, 30.0)), Err(why::HIT_TARGET));
    }

    #[test]
    fn a_hit_on_what_takes_none_is_refused() {
        let w = world();
        // An actor hit as an object, a static as an actor.
        assert_eq!(hit(&w, &guest(), &HitView { location: HitView::OBJECT, ..shot(NPC, PISTOL, 30.0) }), Err(why::HIT_KIND));
        assert_eq!(hit(&w, &guest(), &shot(ROCK, PISTOL, 30.0)), Err(why::HIT_KIND));
    }

    #[test]
    fn a_hit_in_another_space_is_refused() {
        let g = Guest { space: 0x1234, ..guest() };
        assert_eq!(hit(&world(), &g, &shot(NPC, PISTOL, 30.0)), Err(why::HIT_SPACE));
    }

    #[test]
    fn a_hit_past_the_weapons_reach_is_refused() {
        let w = world();
        // The knife's reach is a few hundred units; the NPC stands 1000 away.
        assert_eq!(hit(&w, &guest(), &shot(NPC, KNIFE, 10.0)), Err(why::HIT_RANGE));
        // Unarmed, from far off.
        assert_eq!(hit(&w, &guest(), &shot(NPC, 0, 10.0)), Err(why::HIT_RANGE));
        // The pistol's range is 4000.
        let g = Guest { feet: [-5000.0, 0.0, 0.0], ..guest() };
        assert_eq!(hit(&w, &g, &shot(NPC, PISTOL, 10.0)), Err(why::HIT_RANGE));
    }

    #[test]
    fn a_hit_dealing_more_than_its_weapon_can_is_refused() {
        let w = world();
        // 20 × 1 × 2 × 12 = 480 at most for the pistol.
        assert_eq!(hit(&w, &guest(), &shot(NPC, PISTOL, 480.0)), Ok(()));
        assert_eq!(hit(&w, &guest(), &shot(NPC, PISTOL, 900.0)), Err(why::HIT_DEALT));
        let mut w = w;
        w.things.insert(NPC, thing(ThingKind::Actor, [120.0, 0.0, 0.0]));
        assert_eq!(hit(&w, &guest(), &shot(NPC, 0, UNARMED_DEALT_MAX + 1.0)), Err(why::HIT_DEALT));
    }

    #[test]
    fn limb_damage_is_bounded_by_the_weapons_limb_multiplier() {
        let w = world();
        assert_eq!(limb_cap(&w, PISTOL, 10.0), 20.0, "limb mult 2");
        assert_eq!(limb_cap(&w, KNIFE, 10.0), 10.0);
        assert_eq!(limb_cap(&w, 0x7777, 10.0), 10.0, "unresolved: the damage");
        assert_eq!(limb_cap(&w, 0, 10.0), 10.0);
    }

    #[test]
    fn a_hit_with_a_weapon_this_world_cannot_resolve_gets_the_unarmed_bounds() {
        const UNKNOWN: u32 = 0x7777;
        let mut w = world();
        // Far off (1000): the unarmed reach, not the longest.
        assert_eq!(hit(&w, &guest(), &shot(NPC, UNKNOWN, 10.0)), Err(why::HIT_RANGE));
        w.things.insert(NPC, thing(ThingKind::Actor, [120.0, 0.0, 0.0]));
        assert_eq!(hit(&w, &guest(), &shot(NPC, UNKNOWN, 15.0)), Ok(()));
        assert_eq!(hit(&w, &guest(), &shot(NPC, UNKNOWN, f32::MAX)), Err(why::HIT_DEALT));
        assert_eq!(hit(&w, &guest(), &shot(NPC, UNKNOWN, UNARMED_DEALT_MAX + 1.0)), Err(why::HIT_DEALT));
    }

    #[test]
    fn an_unlock_at_a_locked_thing_in_reach_passes() {
        let w = world();
        assert_eq!(unlock(&w, &guest(), SAFE, false), Ok(()));
        assert_eq!(unlock(&w, &guest(), TERMINAL, true), Ok(()));
        // Unlocked already: nothing to do, nothing wrong.
        assert_eq!(unlock(&w, &guest(), DOOR, false), Ok(()));
    }

    #[test]
    fn an_unlock_that_does_not_fit_is_refused() {
        let mut w = world();
        assert_eq!(unlock(&w, &guest(), 0x999, false), Err(why::UNLOCK_TARGET));
        assert_eq!(unlock(&w, &guest(), BED, false), Err(why::UNLOCK_KIND));
        // A terminal unlocked as a lock, a safe hacked as a terminal.
        assert_eq!(unlock(&w, &guest(), TERMINAL, false), Err(why::UNLOCK_KIND));
        assert_eq!(unlock(&w, &guest(), SAFE, true), Err(why::UNLOCK_KIND));
        let far = Guest { feet: [2000.0, 0.0, 0.0], ..guest() };
        assert_eq!(unlock(&w, &far, SAFE, false), Err(why::UNLOCK_REACH));
        w.things.insert(SAFE, Thing { locked: true, key_only: true, ..thing(ThingKind::Container, [100.0, 0.0, 0.0]) });
        assert_eq!(unlock(&w, &guest(), SAFE, false), Err(why::UNLOCK_KEY));
    }

    #[test]
    fn a_door_in_reach_passes_and_others_are_refused() {
        let mut w = world();
        assert_eq!(door(&w, &guest(), DOOR), Ok(()));
        assert_eq!(door(&w, &guest(), 0x999), Err(why::DOOR_TARGET));
        assert_eq!(door(&w, &guest(), SAFE), Err(why::DOOR_KIND));
        let far = Guest { feet: [2000.0, 0.0, 0.0], ..guest() };
        assert_eq!(door(&w, &far, DOOR), Err(why::DOOR_REACH));
        w.things.insert(DOOR, Thing { locked: true, ..thing(ThingKind::Door, [100.0, 0.0, 0.0]) });
        assert_eq!(door(&w, &guest(), DOOR), Err(why::DOOR_LOCKED));
    }

    #[test]
    fn a_revive_needs_the_body_down_and_near() {
        assert_eq!(revive(true, &guest(), (SPACE, [200.0, 0.0, 0.0])), Ok(()));
        assert_eq!(revive(false, &guest(), (SPACE, [200.0, 0.0, 0.0])), Err(why::REVIVE_UP));
        assert_eq!(revive(true, &guest(), (SPACE, [5000.0, 0.0, 0.0])), Err(why::REVIVE_REACH));
        assert_eq!(revive(true, &guest(), (0x1234, [200.0, 0.0, 0.0])), Err(why::REVIVE_REACH));
    }

    #[test]
    fn a_drop_goes_in_the_guests_own_cell() {
        assert_eq!(dropped(Some(7), 7), Ok(()));
        assert_eq!(dropped(None, 7), Ok(()));
        assert_eq!(dropped(Some(7), 8), Err(why::DROP_CELL));
    }

    #[test]
    fn a_guests_drops_keep_to_their_number_oldest_out() {
        let mut c = HostChecks::default();
        for r in 0..DROPS_KEPT as u32 {
            assert_eq!(c.dropped(1, 1000 + r), None);
        }
        assert_eq!(c.dropped(1, 5000), Some(1000));
        // Another guest's count is its own.
        assert_eq!(c.dropped(2, 6000), None);
        // A drop taken is no longer counted.
        c.taken(1001);
        assert_eq!(c.dropped(1, 5001), None);
        assert_eq!(c.dropped(1, 5002), Some(1002));
    }

    #[test]
    fn a_take_of_a_near_drop_passes_and_others_are_refused() {
        let w = world();
        assert_eq!(took(&w, &guest(), DROP), Ok(()));
        // An item that is no player's drop, nothing loaded.
        assert_eq!(took(&w, &guest(), CAN), Err(why::TAKE_TARGET));
        assert_eq!(took(&w, &guest(), 0x999), Err(why::TAKE_TARGET));
        let far = Guest { feet: [2000.0, 0.0, 0.0], ..guest() };
        assert_eq!(took(&w, &far, DROP), Err(why::TAKE_REACH));
    }

    fn carried(reference: u32, pos: [f32; 3]) -> Carry {
        Carry { reference, pos, rot: [0.0; 3], done: false }
    }

    #[test]
    fn a_carry_of_a_loose_thing_passes() {
        let mut w = world();
        assert_eq!(carry(&w, &guest(), &carried(CAN, [150.0, 0.0, 0.0]), None), Ok(()));
        w.things.insert(ROCK, Thing { loose: true, ..thing(ThingKind::Other, [50.0, 0.0, 0.0]) });
        assert_eq!(carry(&w, &guest(), &carried(ROCK, [80.0, 0.0, 0.0]), Some([50.0, 0.0, 0.0])), Ok(()));
    }

    #[test]
    fn a_carry_that_does_not_fit_is_refused() {
        let w = world();
        assert_eq!(carry(&w, &guest(), &carried(0x999, [0.0; 3]), None), Err(why::CARRY_TARGET));
        for r in [NPC, DOOR, BED, ROCK] {
            assert_eq!(carry(&w, &guest(), &carried(r, [100.0, 0.0, 0.0]), None), Err(why::CARRY_KIND), "{r:#x}");
        }
        let elsewhere = Guest { space: 0x1234, ..guest() };
        assert_eq!(carry(&w, &elsewhere, &carried(CAN, [100.0, 0.0, 0.0]), None), Err(why::CARRY_TARGET));
        assert_eq!(carry(&w, &guest(), &carried(CAN, [3000.0, 0.0, 0.0]), Some([100.0, 0.0, 0.0])), Err(why::CARRY_STEP));
    }

    #[test]
    fn a_carry_is_measured_from_this_guests_last_fresh_place() {
        let mut c = HostChecks::default();
        c.note_carry(1, CAN, [100.0, 0.0, 0.0], false, 10.0);
        assert_eq!(c.last_carry(1, CAN, 10.5), Some([100.0, 0.0, 0.0]));
        assert_eq!(c.last_carry(2, CAN, 10.5), None, "another guest's carry of it");
        assert_eq!(c.last_carry(1, CAN, 10.0 + CARRY_FRESH), None, "old: from where it lies");
        c.note_carry(1, CAN, [200.0, 0.0, 0.0], false, 11.0);
        c.note_carry(1, CAN, [200.0, 0.0, 0.0], true, 11.1);
        assert_eq!(c.last_carry(1, CAN, 11.2), None, "put down");
        // A guest that left takes its carries with it.
        c.note_carry(1, CAN, [100.0, 0.0, 0.0], false, 12.0);
        c.note_carry(2, ROCK, [100.0, 0.0, 0.0], false, 12.0);
        c.forget(1);
        assert_eq!(c.last_carry(1, CAN, 12.1), None);
        assert_eq!(c.last_carry(2, ROCK, 12.1), Some([100.0, 0.0, 0.0]));
    }

    #[test]
    fn a_guests_forced_resends_are_capped_each_second() {
        let mut c = HostChecks::default();
        for _ in 0..FORCED_PER_SECOND {
            assert!(c.force(1, 0.0));
        }
        assert!(!c.force(1, 0.5));
        // Another guest has its own; the next second, again.
        assert!(c.force(2, 0.5));
        assert!(c.force(1, 1.0));
    }

    #[test]
    fn refusals_are_counted_and_logged_once_a_second() {
        let mut c = HostChecks::default();
        assert!(c.refuse(why::HIT_RANGE, 0.0));
        assert!(!c.refuse(why::HIT_RANGE, 0.5));
        assert!(c.refuse(why::DOOR_REACH, 0.5));
        assert!(c.refuse(why::HIT_RANGE, 1.0));
        assert_eq!(c.refused[why::HIT_RANGE], 3);
        assert_eq!(c.total(), 4);
    }
}
