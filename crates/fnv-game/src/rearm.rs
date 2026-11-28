//! NPCs take up weapons again (docs/design/combat-ai.md §22; owner request 2026-10-25):
//! the rank of a weapon for an actor and the taking of what it fetches. Where an actor
//! looks and how it gets there is `fnv_ai::rearm`. The rules are **[design]** unless
//! tagged: combat is not a parity target (docs/design/combat.md).
//!
//! - **Who** ([`Game::rearms`]): a living NPC, or a Super Mutant creature (CREA type 4,
//!   the creatures that draw the people's weapons, nvforever `outfits.rs`).
//! - **Rank** ([`Rank`], [`Game::weapon_rank`]): the actor's own class first (ranged or
//!   melee by its best skill of each; a creature has none), then damage per second
//!   ([`dps`]). Unusable weapons have no rank: not drawable, Player Only or Not Used In
//!   Normal Combat, broken, ruled out by its combat style's Weapon Restrictions, grenades,
//!   mines and thrown weapons (never a primary weapon, `crate::style_weapon`; NPCs throw
//!   grenades by their own rules, nvforever `npc_grenades`), and guns without ammunition
//!   the actor carries or can find ("they need ammo associated to be able to use it",
//!   owner 2026-10-25). The weapon held now keeps its rank without ammunition: NPCs fire
//!   without spending any (owner 2026-10-17).
//! - **Distance** ([`Game::take_up_for_distance`], owner 2026-10-25: "the incinerator would
//!   be better at a distance and the flamer up close"): a weapon with a flame or an
//!   explosive projectile fires only within its reach (WEAP max range times the style's
//!   Ranged Weapon Max Range Mult unless Range - Fixed, never beyond its projectile's
//!   range) and, explosive, only beyond its explosion's radius plus [`BLAST_MARGIN`]
//!   (the combat AI's own limits, nvforever `npc_combat::gun_reach`); other weapons work
//!   at every distance. A fighter whose weapon does not work at its target's distance
//!   holds the best weapon of its own class it carries that does. The weapon's own
//!   projectile is read (an ammunition's projectile is not).
//! - **Too close for its blast** (#2745: Boomers in the Nellis barracks stood with their
//!   grenade launchers and let the player kill them): a fighter whose explosive weapon
//!   does not fire because its target is inside the blast, with no weapon of its own
//!   class that does, first steps back (the combat AI's own rule). Still too close after
//!   [`CORNERED_SECONDS`] (the room left it no way back), it holds the best weapon of any
//!   class it carries that fires there; with none, it puts the explosive weapon away and
//!   fights with its fists. Once the target is clear of the blast again (by
//!   [`RANGE_SLACK`], after [`RANGE_SWAP_SECONDS`]), or the fight is over, it holds the
//!   explosive weapon again. A combat style with Ignore Damaging Self fires it anyway, so
//!   it keeps it.
//! - **Taking** ([`Game::take_leg`]): a weapon on the ground is picked up, one carried by
//!   a dead actor taken from it (condition kept); ammunition is copied, one round, and
//!   the source keeps its own ("npcs clone the ammo too, they dont take it from the
//!   existing container or source", owner 2026-10-25).

use crate::functions::actor as fa;
use crate::game::{ActMode, Game};
use fnv_sim::forms::{Item, ItemKind};
use fnv_sim::inventory::{condition_mult, Pick};
use fnv_sim::FormId;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// A melee blow every this many seconds over its Attack Mult: the combat AI's strike
/// pace (nvforever `npc_combat::STRIKE_SECONDS`).
pub const MELEE_STRIKE_SECONDS: f32 = 1.2;

/// The pick-up idles [observed: FalloutNV.esm, children of `NPCPickUpItem` 0x00069B6E
/// and the super mutants' `SBGroundGrab` 0x000612AC].
pub const PICKUP_IDLE: &str = "LoosePickUpGroundObject";
pub const MUTANT_PICKUP_IDLE: &str = "SBGroundGrab";

/// CREA DATA type Super Mutant [documented: xEdit wbCreatureTypeEnum].
pub const SUPER_MUTANT: i64 = 4;

/// A gun with an explosive projectile fires only this far beyond its explosion's radius
/// (nvforever `npc_combat::BLAST_MARGIN`).
pub const BLAST_MARGIN: f32 = 64.0;

/// A swap for the target's distance wants the new weapon to work this far inside its
/// limits, game units, so that it is not out of them again at once [design].
pub const RANGE_SLACK: f32 = 64.0;

/// At least this long between two swaps for the target's distance, seconds [design].
pub const RANGE_SWAP_SECONDS: f64 = 3.0;

/// A fighter whose target stays inside its explosive weapon's blast this long, though it
/// steps back, takes up another way to attack (module notes, "Too close"), seconds
/// [design].
pub const CORNERED_SECONDS: f64 = 2.0;

/// What [`Game::take_up_for_distance`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeSwap {
    /// It holds this weapon now.
    Took(FormId),
    /// It put this explosive weapon away and fights with its fists.
    PutAway(FormId),
}

/// Copied ammunition: this many rounds [design: enough to make the gun usable, as NPCs
/// spend none].
pub const AMMO_COPIED: i32 = 1;

/// The weapon skills by class, as actor value codes [documented: xEdit ActorValue enum].
const RANGED_SKILLS: [u16; 4] = [33, 34, 35, 41];
const MELEE_SKILLS: [u16; 2] = [38, 45];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Ranged,
    Melee,
}

/// How good a weapon is for an actor, compared field by field (module notes).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rank {
    /// Its class is the actor's (always for an actor without one).
    pub own_class: bool,
    pub dps: f32,
}

impl Rank {
    /// Whether this rank is above `other` (`None`: no usable weapon, the fists).
    pub fn beats(&self, other: Option<Rank>) -> bool {
        let Some(o) = other else { return true };
        self.own_class.cmp(&o.own_class).then(self.dps.total_cmp(&o.dps)).is_gt()
    }
}

/// The class of a weapon: by its skill, else ranged when it fires a projectile.
pub fn weapon_class(item: &Item) -> Class {
    match item.weapon_traits.and_then(|t| t.skill) {
        Some(s) if MELEE_SKILLS.contains(&s) => Class::Melee,
        Some(s) if RANGED_SKILLS.contains(&s) => Class::Ranged,
        _ if item.projectile.is_some() => Class::Ranged,
        _ => Class::Melee,
    }
}

/// A weapon's damage per second at `condition` (module notes): the damage the outfitting
/// ranks by times the condition's share, times attacks per second.
pub fn dps(item: &Item, condition: f32) -> f32 {
    let t = item.weapon_traits.unwrap_or_default();
    let per_attack = item.weapon_damage.unwrap_or(0.0).max(0.0) * condition_mult(condition);
    let per_second = if weapon_class(item) == Class::Melee {
        t.attack_mult.max(0.25) / MELEE_STRIKE_SECONDS
    } else if t.automatic && t.fire_rate > 0.0 {
        t.fire_rate
    } else if t.shots_per_sec > 0.0 {
        t.shots_per_sec
    } else {
        1.0
    };
    per_attack * per_second
}

/// Where an item of a fetch comes from and how it is had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Take {
    /// A reference of the item lying in the world: picked up.
    Ground,
    /// From a dead actor's inventory.
    FromBody,
    /// Ammunition copied from any source (ground, body, container).
    CopyAmmo,
}

/// One stop of a fetch.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Leg {
    /// The reference gone to: the item itself, a body or a container.
    pub from: FormId,
    /// The base item had there.
    pub item: FormId,
    pub take: Take,
    pub at: [f32; 3],
}

/// An actor on its way to a better weapon (and its ammunition).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fetch {
    /// The weapon it will hold.
    pub weapon: FormId,
    pub rank: Rank,
    /// What is left to go to, the next first.
    pub legs: Vec<Leg>,
    pub since: f64,
    /// Standing for the pick-up idle until then (game time).
    pub busy_until: Option<f64>,
}

/// What re-arming keeps. Not saved.
#[derive(Debug, Default)]
pub struct Rearms {
    /// The fetch each actor is on; the combat AI holds its fire meanwhile.
    pub fetches: BTreeMap<FormId, Fetch>,
    /// References each actor gave up on in this fight (no way there, gone, too slow).
    pub given_up: BTreeMap<FormId, BTreeSet<FormId>>,
    /// When each actor looks again (game time).
    pub next_look: BTreeMap<FormId, f64>,
    /// When each actor last swapped weapons for its target's distance (game time).
    pub range_swapped: BTreeMap<FormId, f64>,
    /// Since when each actor's target has been inside its explosive weapon's blast, with
    /// no weapon of its own class that fires there (game time; module notes, "Too close").
    pub too_close_since: BTreeMap<FormId, f64>,
    /// The explosive weapon each actor put away to fight with its fists, until its target
    /// is clear of the blast or the fight is over.
    pub put_away: BTreeMap<FormId, FormId>,
}

impl Rearms {
    /// Whether a fetch other than `actor`'s goes to `r`.
    pub fn claimed(&self, actor: FormId, r: FormId) -> bool {
        self.fetches.iter().any(|(a, f)| *a != actor && f.legs.iter().any(|l| l.from == r && l.take != Take::CopyAmmo))
    }

    /// Forget `actor`'s fetch and what it gave up (its fight is over, it died).
    pub fn calm(&mut self, actor: FormId) {
        self.fetches.remove(&actor);
        self.given_up.remove(&actor);
        self.next_look.remove(&actor);
        self.range_swapped.remove(&actor);
        self.too_close_since.remove(&actor);
    }
}

impl Game<'_> {
    fn base_sig(&self, r: FormId) -> Option<[u8; 4]> {
        crate::functions::base_of(self, r).and_then(|b| self.data.sig(b))
    }

    /// Whether `actor` takes up weapons by these rules (module notes).
    pub fn rearms(&self, actor: FormId) -> bool {
        if actor == FormId::PLAYER_REF || self.extra.get(&actor).is_some_and(|e| e.dead) {
            return false;
        }
        match self.base_sig(actor) {
            Some(s) if &s == b"NPC_" => true,
            Some(s) if &s == b"CREA" => {
                let Some(base) = crate::functions::base_of(self, actor) else { return false };
                self.template_source(base, actor, |d| d.template_base).is_some_and(|(_, d)| d.creature_type == Some(SUPER_MUTANT))
            }
            _ => false,
        }
    }

    /// The class of `actor`'s weapons: ranged when its best ranged skill is at least its
    /// best melee skill; `None` for a creature.
    pub fn actor_class(&mut self, actor: FormId) -> Option<Class> {
        if self.base_sig(actor) != Some(*b"NPC_") {
            return None;
        }
        let mut best = |skills: &[u16]| skills.iter().map(|&s| fa::actor_value(self, actor, s).unwrap_or(0.0)).fold(f64::MIN, f64::max);
        let ranged = best(&RANGED_SKILLS);
        let melee = best(&MELEE_SKILLS);
        Some(if ranged >= melee { Class::Ranged } else { Class::Melee })
    }

    /// The AMMO forms `weapon` accepts: its NAM0, or the AMMO entries of that form list.
    /// Empty for a weapon that fires none (melee, thrown).
    pub fn accepted_ammo(&self, weapon: FormId) -> Vec<FormId> {
        let Some(it) = self.world.forms.item(weapon) else { return Vec::new() };
        let Some(ammo) = it.weapon_traits.and_then(|t| t.ammo).filter(|_| !it.thrown) else { return Vec::new() };
        let is_ammo = |f: &FormId| self.world.forms.item(*f).is_some_and(|i| i.kind == ItemKind::Ammo);
        if is_ammo(&ammo) {
            return vec![ammo];
        }
        crate::functions::list_members(self, ammo).into_iter().filter(is_ammo).collect()
    }

    /// Whether `holder`'s inventory has any of `ammo`.
    pub fn carries_any(&self, holder: FormId, ammo: &[FormId]) -> bool {
        self.world.peek(holder).and_then(|s| s.inventory.as_ref()).is_some_and(|inv| ammo.iter().any(|a| inv.count(*a) > 0))
    }

    /// The rank of `weapon` at `condition` for `actor`; `None` when it cannot use it.
    /// `ammo_found`: ammunition for it is carried or within reach (ignored for a weapon
    /// that needs none).
    pub fn weapon_rank(&mut self, actor: FormId, weapon: FormId, condition: f32, ammo_found: bool) -> Option<Rank> {
        let it = self.world.forms.item(weapon).filter(|i| i.kind == ItemKind::Weapon)?.clone();
        let restriction = self.combat_style(actor).map_or(0, |s| s.weapon_restrictions);
        let reach = crate::style_weapon::reach_of(self.data.weapon_anim_types.get(&weapon).copied());
        if !it.drawable || it.not_for_npcs || condition <= 0.0 || !crate::style_weapon::allowed(restriction, reach) {
            return None;
        }
        if !ammo_found && !self.accepted_ammo(weapon).is_empty() {
            return None;
        }
        let own_class = self.actor_class(actor).is_none_or(|c| c == weapon_class(&it));
        Some(Rank { own_class, dps: dps(&it, condition) })
    }

    /// The weapon `actor` holds by the rules and its condition.
    pub fn held_weapon(&self, actor: FormId) -> Option<(FormId, f32)> {
        let items = self.world.equipment(actor)?;
        let w = items.into_iter().find(|i| self.world.forms.item(*i).is_some_and(|it| it.kind == ItemKind::Weapon))?;
        let condition = self.world.peek(actor).and_then(|s| s.inventory.as_ref()).and_then(|inv| inv.condition_in_use(w)).unwrap_or(1.0);
        Some((w, condition))
    }

    /// The rank of the weapon `actor` holds, which needs no ammunition to count.
    pub fn held_rank(&mut self, actor: FormId) -> Option<Rank> {
        let (w, condition) = self.held_weapon(actor)?;
        self.weapon_rank(actor, w, condition, true)
    }

    /// The best usable weapon `actor` carries, its best unit's condition and rank.
    pub fn best_carried(&mut self, actor: FormId) -> Option<(FormId, f32, Rank)> {
        let units: Vec<(FormId, f32)> = {
            let inv = self.world.peek(actor).and_then(|s| s.inventory.as_ref())?;
            let mut best: BTreeMap<FormId, f32> = BTreeMap::new();
            // A live unit (a planted explosive, `Extra::live`) is not its to hold (#1640).
            for (item, extra, n) in inv.groups() {
                if n > 0 && !extra.broken() && !extra.live {
                    let c = best.entry(item).or_insert(0.0);
                    *c = c.max(extra.condition_or_whole());
                }
            }
            best.into_iter().collect()
        };
        let mut out: Option<(FormId, f32, Rank)> = None;
        for (w, condition) in units {
            let ammo = self.accepted_ammo(w);
            let found = ammo.is_empty() || self.carries_any(actor, &ammo);
            if let Some(r) = self.weapon_rank(actor, w, condition, found) {
                if out.is_none_or(|(_, _, o)| r.beats(Some(o))) {
                    out = Some((w, condition, r));
                }
            }
        }
        out
    }

    /// `actor`, holding no usable weapon (it broke, fell, was taken), holds the best one it
    /// carries. One that holds a usable weapon keeps it: the outfitting's choice stands
    /// against its other carried weapons (Cook-Cook keeps his Incinerator over his Flamer,
    /// observed in the original, actors.md §9.3). Returns the weapon put in its hands.
    pub fn take_up_best_carried(&mut self, actor: FormId) -> Option<FormId> {
        // Fists by choice (module notes, "Too close"): not an empty hand to fill.
        if self.held_rank(actor).is_some() || self.rearms.put_away.contains_key(&actor) {
            return None;
        }
        let (w, condition, _) = self.best_carried(actor)?;
        self.hold(actor, w, condition).then_some(w)
    }

    /// `actor` holds `weapon`, which it fetched, when its best unit ranks above the weapon
    /// it holds. Whether it does.
    pub fn take_up_fetched(&mut self, actor: FormId, weapon: FormId) -> bool {
        let Some((condition, rank)) = self.best_carried_unit(actor, weapon) else { return false };
        rank.beats(self.held_rank(actor)) && self.hold(actor, weapon, condition)
    }

    /// The best usable unit of `weapon` that `actor` carries: its condition and rank.
    fn best_carried_unit(&mut self, actor: FormId, weapon: FormId) -> Option<(f32, Rank)> {
        let condition = {
            let inv = self.world.peek(actor).and_then(|s| s.inventory.as_ref())?;
            inv.groups().filter(|(i, e, n)| *i == weapon && *n > 0 && !e.broken() && !e.live).map(|(_, e, _)| e.condition_or_whole()).fold(None, |m: Option<f32>, c| Some(m.map_or(c, |m| m.max(c))))?
        };
        let ammo = self.accepted_ammo(weapon);
        let found = ammo.is_empty() || self.carries_any(actor, &ammo);
        Some((condition, self.weapon_rank(actor, weapon, condition, found)?))
    }

    /// Equip the unit of `weapon` at `condition` (outfitted first, so what it holds is what
    /// is equipped, `World::equipment`). Whether it was.
    fn hold(&mut self, actor: FormId, weapon: FormId, condition: f32) -> bool {
        if self.held_weapon(actor).is_some_and(|(h, c)| h == weapon && c == condition) {
            return false;
        }
        let _ = self.world.outfit(actor);
        self.maybe_apply_npc_weapon_mods(actor);
        self.world.equip_at(actor, weapon, Pick::at(Some(condition))).or_else(|_| self.world.equip(actor, weapon)).is_ok()
    }

    /// The distances at which `weapon` fires for `actor` (module notes, "Distance"):
    /// (nearest, farthest); `None` for a weapon that works at every distance.
    pub fn firing_band(&self, actor: FormId, weapon: FormId) -> Option<(f32, f32)> {
        let t = self.world.forms.item(weapon)?.weapon_traits?;
        if !t.flame && t.blast <= 0.0 {
            return None;
        }
        let mult = if t.range_fixed { 1.0 } else { self.combat_style(actor).map_or(1.0, |s| if s.range_mult.1 > 0.0 { s.range_mult.1 } else { 1.0 }) };
        let weapon_reach = (t.max_range > 0.0).then_some(t.max_range * mult);
        let projectile = (t.projectile_range > 0.0).then_some(t.projectile_range);
        let far = match (weapon_reach, projectile) {
            (Some(w), Some(p)) => w.min(p),
            (w, p) => w.or(p).unwrap_or(f32::INFINITY),
        };
        let near = if t.blast > 0.0 { t.blast + BLAST_MARGIN } else { 0.0 };
        Some((near, far))
    }

    /// Whether `weapon` fires at a target `distance` away, `slack` inside its limits.
    pub fn works_at(&self, actor: FormId, weapon: FormId, distance: f32, slack: f32) -> bool {
        self.firing_band(actor, weapon).is_none_or(|(near, far)| distance >= near + slack && distance <= far - slack)
    }

    /// `actor` fights a target `distance` away with a weapon that does not fire at that
    /// distance (a flamer at a far target, an incinerator at a near one): it holds the
    /// best weapon of its own class it carries that does, by [`RANGE_SLACK`], at most
    /// every [`RANGE_SWAP_SECONDS`]. An explosive weapon with its target inside the blast
    /// gives way to one of any class, or to the fists, after [`CORNERED_SECONDS`]; the
    /// fists give way to it again once the target is clear (module notes, "Too close").
    pub fn take_up_for_distance(&mut self, actor: FormId, distance: f32) -> Option<RangeSwap> {
        let recently = self.rearms.range_swapped.get(&actor).is_some_and(|t| self.time - t < RANGE_SWAP_SECONDS);
        let Some((held, _)) = self.held_weapon(actor) else {
            // Its fists, for an explosive weapon put away: that again once it fires here.
            let put = *self.rearms.put_away.get(&actor)?;
            if recently || !self.works_at(actor, put, distance, RANGE_SLACK) {
                return None;
            }
            self.rearms.put_away.remove(&actor);
            let (condition, _) = self.best_carried_unit(actor, put)?;
            return self.hold(actor, put, condition).then(|| {
                self.rearms.range_swapped.insert(actor, self.time);
                RangeSwap::Took(put)
            });
        };
        if self.works_at(actor, held, distance, 0.0) {
            self.rearms.too_close_since.remove(&actor);
            return None;
        }
        if recently {
            return None;
        }
        let class = match self.actor_class(actor) {
            Some(c) => c,
            None => weapon_class(self.world.forms.item(held)?),
        };
        if let Some(w) = self.best_working_carried(actor, held, distance, Some(class)) {
            self.rearms.too_close_since.remove(&actor);
            return Some(RangeSwap::Took(w));
        }
        // Too close for its explosive weapon's blast (not too far for its reach), with a
        // style that minds its own blast.
        let too_close = self.firing_band(actor, held).is_some_and(|(near, _)| near > 0.0 && distance < near)
            && !self.combat_style(actor).is_some_and(|s| s.ignore_damaging_self);
        if !too_close {
            self.rearms.too_close_since.remove(&actor);
            return None;
        }
        let now = self.time;
        let since = *self.rearms.too_close_since.entry(actor).or_insert(now);
        if now - since < CORNERED_SECONDS {
            return None;
        }
        self.rearms.too_close_since.remove(&actor);
        if let Some(w) = self.best_working_carried(actor, held, distance, None) {
            return Some(RangeSwap::Took(w));
        }
        if !self.world.unequip(actor, held).unwrap_or(false) {
            return None;
        }
        self.rearms.put_away.insert(actor, held);
        self.rearms.range_swapped.insert(actor, now);
        Some(RangeSwap::PutAway(held))
    }

    /// `actor` holds the best weapon it carries other than `held` that fires at
    /// `distance` by [`RANGE_SLACK`], of `class` when given. Returns it.
    fn best_working_carried(&mut self, actor: FormId, held: FormId, distance: f32, class: Option<Class>) -> Option<FormId> {
        let carried: Vec<FormId> = {
            let inv = self.world.peek(actor).and_then(|s| s.inventory.as_ref())?;
            inv.free_iter().filter(|(i, n)| *n > 0 && *i != held).map(|(i, _)| i).collect()
        };
        let mut best: Option<(FormId, f32, Rank)> = None;
        for w in carried {
            if self.world.forms.item(w).is_none_or(|it| it.kind != ItemKind::Weapon || class.is_some_and(|c| weapon_class(it) != c)) || !self.works_at(actor, w, distance, RANGE_SLACK) {
                continue;
            }
            let Some((condition, rank)) = self.best_carried_unit(actor, w) else { continue };
            if best.is_none_or(|(_, _, b)| rank.beats(Some(b))) {
                best = Some((w, condition, rank));
            }
        }
        let (w, condition, _) = best?;
        self.hold(actor, w, condition).then(|| {
            self.rearms.range_swapped.insert(actor, self.time);
            w
        })
    }

    /// `actor`'s fight is over (or a package holds it): it forgets what re-arming kept
    /// ([`Rearms::calm`]) and holds again an explosive weapon it put away for its fists.
    pub fn calm_rearm(&mut self, actor: FormId) {
        if let Some(put) = self.rearms.put_away.remove(&actor) {
            if self.held_weapon(actor).is_none() {
                if let Some((condition, _)) = self.best_carried_unit(actor, put) {
                    self.hold(actor, put, condition);
                }
            }
        }
        self.rearms.calm(actor);
    }

    /// The pick-up idle for `actor` (module notes), when the records have it.
    pub fn pickup_idle(&self, actor: FormId) -> Option<FormId> {
        let edid = if self.base_sig(actor) == Some(*b"CREA") { MUTANT_PICKUP_IDLE } else { PICKUP_IDLE };
        self.data.by_edid(edid)
    }

    /// Whether `leg` can still be had: its reference is there, a ground item unowned
    /// (or owned by the actor), a body dead and still carrying the item, a container
    /// unlocked and holding it.
    pub fn leg_open(&self, actor: FormId, leg: &Leg) -> bool {
        let Some(s) = self.world.peek(leg.from).filter(|s| s.enabled() && !s.deleted()) else { return false };
        match leg.take {
            Take::Ground => s.base == leg.item,
            Take::FromBody => self.extra.get(&leg.from).is_some_and(|e| e.dead) && self.carries_any(leg.from, &[leg.item]),
            Take::CopyAmmo if s.base == leg.item => true,
            Take::CopyAmmo => !s.lock.as_ref().is_some_and(|l| l.locked) && self.carries_any(leg.from, &[leg.item]) && actor != leg.from,
        }
    }

    /// `actor` has reached `leg`: it takes the weapon or copies the ammunition. Whether it
    /// got it.
    pub fn take_leg(&mut self, actor: FormId, leg: &Leg) -> bool {
        if !self.leg_open(actor, leg) {
            return false;
        }
        match leg.take {
            Take::Ground => self.activate(actor, leg.from, ActMode::Default).is_ok() && self.carries_any(actor, &[leg.item]),
            Take::FromBody => self.world.transfer(leg.from, actor, leg.item, 1).is_ok_and(|n| n == 1),
            Take::CopyAmmo => self.world.add_item(actor, leg.item, AMMO_COPIED).is_ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_sim::forms::WeaponTraits;

    fn weapon(damage: f32, skill: u16, traits: WeaponTraits, projectile: bool) -> Item {
        Item {
            kind: ItemKind::Weapon,
            value: None,
            money: None,
            weight: None,
            slots: None,
            projectile: projectile.then_some(FormId(1)),
            health: Some(100),
            non_playable: false,
            cant_drop: false,
            armor_class: Default::default(),
            power_armor: false,
            armor_dr: None,
            armor_dt: None,
            weapon_damage: Some(damage),
            weight_mods: [0.0; 3],
            drawable: true,
            not_for_npcs: false,
            thrown: false,
            quest_item: false,
            icon: None,
            placed_count: None,
            teaches: None,
            cant_be_taken: false,
            modulates_voice: false,
            weapon_traits: Some(WeaponTraits { skill: Some(skill), ..traits }),
        }
    }

    #[test]
    fn damage_per_second_by_kind_and_condition() {
        // A semi-automatic pistol: 22 a shot, 2 shots a second.
        let pistol = weapon(22.0, 41, WeaponTraits { shots_per_sec: 2.0, ..Default::default() }, true);
        assert_eq!(dps(&pistol, 1.0), 44.0);
        // Below 75 % condition the damage falls: at half, 0.8325 of it.
        assert!((dps(&pistol, 0.5) - 44.0 * 0.8325).abs() < 1e-3);
        // An automatic weapon fires at its Fire Rate.
        let smg = weapon(14.0, 41, WeaponTraits { automatic: true, fire_rate: 10.0, shots_per_sec: 1.0, ..Default::default() }, true);
        assert_eq!(dps(&smg, 1.0), 140.0);
        // A melee weapon strikes every 1.2 s over its Attack Mult.
        let iron = weapon(15.0, 38, WeaponTraits { attack_mult: 1.2, ..Default::default() }, false);
        assert!((dps(&iron, 1.0) - 15.0).abs() < 1e-4);
    }

    #[test]
    fn the_class_comes_first_then_damage_per_second() {
        let gun = Rank { own_class: true, dps: 10.0 };
        let club = Rank { own_class: false, dps: 500.0 };
        assert!(gun.beats(Some(club)), "a gunman keeps a weak gun over a strong club");
        assert!(club.beats(None), "anything usable before the fists");
        assert!(Rank { dps: 11.0, ..gun }.beats(Some(gun)));
        assert!(!gun.beats(Some(gun)), "an equal weapon is not better");
    }

    #[test]
    fn the_class_of_a_weapon() {
        assert_eq!(weapon_class(&weapon(1.0, 45, Default::default(), false)), Class::Melee);
        assert_eq!(weapon_class(&weapon(1.0, 34, Default::default(), true)), Class::Ranged);
        let mut odd = weapon(1.0, 32, Default::default(), true);
        assert_eq!(weapon_class(&odd), Class::Ranged, "no weapon skill: by its projectile");
        odd.projectile = None;
        assert_eq!(weapon_class(&odd), Class::Melee);
    }
}
