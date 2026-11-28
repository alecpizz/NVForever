//! An actor's authored abilities and the enchantments of what it wears, as active
//! effects (docs/spec-notes/aid-items.md §13).
//!
//! - Authored abilities ([`Game::give_abilities`]): the actor effects of type Ability in
//!   the SPLO list of the actor's record (or of the template that supplies its "Actor
//!   Effect List") and of its race join the actor's spell list once, the first time
//!   the actor is processed (loaded, or asked about by a script), so RemoveSpell takes
//!   them away and they are saved with it. A race change (the character creator) swaps
//!   the race's abilities. The abilities in the list are active while the actor is
//!   loaded ([`Game::sync_abilities`]).
//! - Perks' abilities ([`Game::sync_perk_abilities`], `crate::perks`): the Ability
//!   effects of the player's perks and traits join the player's list while the perk is
//!   held, and leave it with the perk; a teammate's teammate perks give theirs to the
//!   teammate while it is one ([`Game::held_perks`]).
//! - The engine's own ability ([`ENGINE_ABILITIES`]): `HeadCrippleEffect` joins the list
//!   of every actor but the player with its authored ones (docs/spec-notes/crippled-limbs.md
//!   §2).
//! - Worn enchantments ([`Game::sync_worn`]): the object effect (ENCH of type Apparel)
//!   of each equipped armour piece is active on the loaded wearer while it is equipped,
//!   and ends when it is unequipped or leaves the inventory.
//! - Effects of abilities and worn enchantments whose conditions fail stay listed but
//!   inactive (no value change, no script) and their conditions are checked again
//!   every second ([`Game::recheck_conditions`]). So are those of a spell's effects that
//!   last (a duration above 0), whether their conditions passed when it was cast or not;
//!   an instant one that fails is dropped: the masters' `PlayerConcussed`, which
//!   `GenericScript` casts on the player while the head is whole and keeps on for ten
//!   years, takes Perception only while the head is crippled [hypothesis: as the
//!   abilities'; docs/spec-notes/crippled-limbs.md §2].

use crate::game::Game;
use fnv_sim::FormId;
use std::collections::BTreeSet;

/// Incremental cache for [`Game::effects_actors`]: authored placements in loaded cells,
/// placed actors whose `RefState.cell` differs from their placement, live created actors,
/// and actors with dispelled abilities. Kept in sync with `World::take_touched_refs` and
/// [`Game::note_dispelled_ability`] so each frame stays proportional to loaded/relocated
/// actors instead of scanning every `RefExtra` and created reference in a long save [design].
pub(crate) struct PlacedEffectActors<'a> {
    placed: &'a fnv_sim::RefTable,
    world_id: u64,
    cells: BTreeSet<FormId>,
    actors: BTreeSet<FormId>,
    relocated_actors: BTreeSet<FormId>,
    created_actors: BTreeSet<FormId>,
    dispelled_actors: BTreeSet<FormId>,
}

/// SPEL SPIT type "Ability" [documented: schema `combat.json` SPIT].
pub const SPELL_ABILITY: i64 = 4;
/// ENCH ENIT type "Weapon" [documented: schema `combat.json`; observed: all 59 WEAP EITM of
/// FalloutNV.esm name one of this type].
pub const ENCHANTMENT_WEAPON: u32 = 2;
/// ENCH ENIT type "Apparel" [documented: schema `combat.json`; observed: all 186 ARMO EITM of
/// FalloutNV.esm name one of this type, all 59 WEAP EITM one of type 2 Weapon].
pub const ENCHANTMENT_APPAREL: u32 = 3;
/// Seconds between checks of an ability's or a worn enchantment's conditions
/// [hypothesis: no source gives the interval].
pub const CONDITION_INTERVAL: f64 = 1.0;

/// Abilities no record lists that every actor but the player has: "Head Crippled",
/// Perception −4 while its Perception Condition is at 0 or below [observed: FalloutNV.esm
/// SPEL `HeadCrippleEffect` 0x00067572, type Ability, effect Reduced Perception 4 with
/// the condition GetActorValue PerceptionCondition <= 0; no RACE or NPC_ lists it;
/// hypothesis: the engine gives it to the actors, as the game's help text says the
/// head's crippling "applies to you as well as to your enemies"]. Not the player: the
/// masters' `GenericScript` (SCPT 0x00032DF1) casts `PlayerConcussed` on the player
/// whenever the player lacks its Concussion effect, and that spell carries the same
/// Perception −4 under the same condition, so the player's comes from the data
/// [observed] (docs/spec-notes/crippled-limbs.md §2).
pub const ENGINE_ABILITIES: [&str; 1] = ["HeadCrippleEffect"];

impl Game<'_> {
    /// The race whose abilities the actor has: the character creator's choice for the
    /// player, else the record's (template flag "Traits").
    pub fn ability_race(&self, r: FormId) -> Option<FormId> {
        // AgeRace's change included (`crate::appearance`).
        self.actor_race(r)
    }

    /// The abilities among a list of actor effects: type Ability only. An SPLO entry of
    /// type Actor Effect (Ruthless, the Tunnelers' DT spells, SporeCarrierRegen, auras
    /// with an area) is not on the actor [observed 2026-10-30, capture o4_00_probe:
    /// placed copies of a Tunneler, a Hulking Tunneler, a spore carrier and a Hoover Dam
    /// legionary read IsSpellTarget 0 for their Actor Effect spells and their DT, Heal Rate
    /// and Melee Damage at the base value, while the spore carrier's Ability
    /// SporeCarrierPoisonResist read IsSpellTarget 1 and Poison Resistance base 20 + 100;
    /// docs/spec-notes/area-effects.md §3].
    fn abilities_of<'s>(&'s self, spells: &'s [FormId]) -> impl Iterator<Item = FormId> + 's {
        spells.iter().copied().filter(|&s| self.spell_type(s) == Some(SPELL_ABILITY))
    }

    /// The abilities of the actor's record: the SPLO list of the record that supplies
    /// its actor effects (template flag "Actor Effect List") [documented: schema ACBS
    /// template flags; observed: GSVictorRef's base Victor sets the flag and its
    /// template VSecuritronMk1TEMPLATE lists the same three abilities].
    pub fn record_abilities(&self, r: FormId) -> Vec<FormId> {
        self.def_following(r, |d| d.template_spells).map_or_else(Vec::new, |d| self.abilities_of(&d.spells).collect())
    }

    /// The abilities of a race (RACE SPLO).
    pub fn race_abilities(&self, race: Option<FormId>) -> Vec<FormId> {
        race.and_then(|r| self.data.race_spells.get(&r)).map_or_else(Vec::new, |l| self.abilities_of(l).collect())
    }

    /// Whether an actor's abilities and worn enchantments are on it: the player always,
    /// another actor while it is enabled in a loaded cell [observed: capture abilities-1:
    /// GSVictorRef, RexREF and LilyRef out of the loaded area read IsSpellTarget 0 and no
    /// modifier for their abilities, Victor loaded 1 and DT 20; hypothesis for disabled
    /// actors].
    pub fn effects_loaded(&self, r: FormId) -> bool {
        r == FormId::PLAYER_REF || (self.present(r) && self.cell_of(r).is_some_and(|c| self.loaded_cells.contains(&c)))
    }

    /// Put an actor's authored abilities in its spell list the first time it is
    /// processed, and its race's again after a race change. Not for references that are
    /// no actor. The effects follow in [`Game::sync_abilities`].
    pub fn give_abilities(&mut self, r: FormId) {
        if r != FormId::PLAYER_REF && self.def_following(r, |d| d.template_spells).is_none() {
            return;
        }
        let race = self.ability_race(r);
        let (given, had_race) = self.extra.get(&r).map_or((false, None), |e| (e.abilities_given, e.ability_race));
        if given && had_race == race {
            return;
        }
        let own = self.record_abilities(r);
        // A new race: the old race's abilities go (unless the record has them too).
        let gone: Vec<FormId> = if given { self.race_abilities(had_race).into_iter().filter(|s| !own.contains(s)).collect() } else { Vec::new() };
        let mut add = if given { Vec::new() } else { own };
        add.extend(self.race_abilities(race));
        if !given && r != FormId::PLAYER_REF {
            add.extend(ENGINE_ABILITIES.iter().filter_map(|e| self.data.by_edid(e)).filter(|s| self.data.spell_types.get(s) == Some(&SPELL_ABILITY)));
        }
        let e = self.extra.entry(r).or_default();
        e.abilities_given = true;
        e.ability_race = race;
        e.spells.retain(|s| !gone.contains(s));
        for spell in add {
            if !e.spells.contains(&spell) {
                e.spells.push(spell);
            }
        }
        for spell in gone {
            self.dispel(r, None, |e| e.source == spell && e.worn.is_none());
        }
    }

    /// What `r`'s active abilities add to actor value `code`, together.
    pub fn ability_value(&self, r: FormId, code: u16) -> f64 {
        self.effects.iter().filter(|e| e.target == r && !e.inactive && self.is_ability_effect(e)).filter_map(|e| e.change.as_ref()).filter(|c| c.code == code).map(|c| c.amount).sum()
    }

    /// Whether an effect is an ability's (an effect of a listed spell of type Ability).
    pub fn is_ability_effect(&self, e: &crate::game::ActiveEffect) -> bool {
        e.permanent && e.worn.is_none() && self.spell_type(e.source) == Some(SPELL_ABILITY)
    }

    /// The abilities in an actor's spell list are active while it is loaded: each one
    /// without its effects starts (unless a Dispel took it off while loaded), and they
    /// all end when the actor is not loaded [observed: abilities-1; hypothesis: an
    /// ability added with AddSpell to an actor out of the loaded area waits for it the
    /// same way, and a dispelled one comes back when the actor is loaded again].
    pub fn sync_abilities(&mut self, r: FormId) {
        if !self.effects_loaded(r) {
            let mut ids: Vec<u32> = Vec::new();
            for e in &self.effects {
                if e.target == r && self.is_ability_effect(e) {
                    ids.push(e.id);
                }
            }
            if !ids.is_empty() {
                self.dispel(r, None, |e| ids.contains(&e.id));
            }
            if let Some(x) = self.extra.get_mut(&r) {
                x.dispelled.clear();
            }
            return;
        }
        let Some(x) = self.extra.get(&r) else { return };
        let mut missing: Vec<FormId> = Vec::new();
        for &s in &x.spells {
            if self.spell_type(s) == Some(SPELL_ABILITY)
                && !x.dispelled.contains(&s)
                && !self.effects.iter().any(|e| e.target == r && e.source == s && e.permanent && e.worn.is_none())
            {
                missing.push(s);
            }
        }
        for spell in missing {
            self.apply_magic_item(r, spell, None, true);
        }
    }

    /// Record that `r` has at least one dispelled ability in `RefExtra::dispelled` so the
    /// incremental [`Game::effects_actors`] cache tracks it until unloaded.
    pub(crate) fn note_dispelled_ability(&mut self, r: FormId) {
        if let Some(cache) = self.placed_effect_actors.as_mut() {
            cache.dispelled_actors.insert(r);
        }
    }

    /// Session `SetObjectEffect` override for `form` (or its already-uncloned `orig`),
    /// avoiding heap allocation when `field_overrides` is empty and skipping a duplicate
    /// lookup when `form` is not a clone.
    fn object_effect_override_with_orig(&self, form: FormId, orig: FormId) -> Option<Option<FormId>> {
        if self.field_overrides.is_empty() {
            return None;
        }
        let val = self.field_overrides.get(&(form, "EITM/".to_string())).or_else(|| {
            (orig != form).then(|| self.field_overrides.get(&(orig, "EITM/".to_string()))).flatten()
        })?;
        match val {
            fnv_script::vm::XVal::Ref(0) => Some(None),
            fnv_script::vm::XVal::Ref(id) => Some(Some(FormId(*id))),
            _ => None,
        }
    }

    /// Session `SetObjectEffect` override for `form` (or its `TempCloneForm` original):
    /// `None` when untouched this session, `Some(None)` when explicitly cleared to 0,
    /// `Some(Some(ench))` when overridden to an enchantment FormID.
    pub fn object_effect_override(&self, form: FormId) -> Option<Option<FormId>> {
        self.object_effect_override_with_orig(form, self.unclone(form))
    }

    /// Effective object effect (`EITM`) of a form (`ARMO`, `WEAP`, `EXPL`, …), preferring
    /// a live `SetObjectEffect` session override on the form or its `TempCloneForm`
    /// original before falling back to authored record data. Returns `None` when cleared
    /// to 0 by `SetObjectEffect`.
    pub fn object_effect(&self, form: FormId) -> Option<FormId> {
        let orig = self.unclone(form);
        if let Some(override_effect) = self.object_effect_override_with_orig(form, orig) {
            return override_effect;
        }
        if let Some(&ench) = self.data.armor_enchantments.get(&orig) {
            return Some(ench);
        }
        // `GameData::build` indexes every authored `ARMO` `EITM` into `armor_enchantments`,
        // so a miss on an `ARMO` needs no `record_field` lookup.
        if self.data.sig(orig) == Some(*b"ARMO") {
            return None;
        }
        match self.data.record_field(orig, "EITM", "") {
            Some(serde_json::Value::String(s)) if s.starts_with("0x") => {
                u32::from_str_radix(&s[2..], 16).ok().filter(|&id| id != 0).map(FormId)
            }
            _ => None,
        }
    }

    /// Snapshot `source`'s effective object effect (`EITM`) onto its `TempCloneForm`
    /// `clone` so pre-clone overrides carry over while post-clone edits on `source` stay
    /// isolated from `clone` (including clone-of-clone chains) [design].
    pub(crate) fn snapshot_object_effect(&mut self, source: FormId, clone: FormId) {
        let orig = self.unclone(source);
        if !matches!(self.data.sig(orig).as_ref(), Some(b"ARMO" | b"WEAP" | b"EXPL")) {
            return;
        }
        let eff = self.object_effect(source);
        self.field_overrides.insert((clone, "EITM/".into()), fnv_script::vm::XVal::Ref(eff.map_or(0, |e| e.0)));
    }

    /// Apply a `SetObjectEffect` override for an `ARMO`, `WEAP`, or `EXPL` form (including
    /// `TempCloneForm` clones) [documented: GECK wiki SetObjectEffect]. Refuses unsupported
    /// base types or mismatched enchantment kinds without mutating state. For `ARMO`, clears
    /// stale worn-dispel state on change and resyncs loaded actors' worn enchantments.
    pub(crate) fn set_object_effect(&mut self, form: FormId, effect: Option<FormId>) -> bool {
        let orig = self.unclone(form);
        let want = match self.data.sig(orig).as_ref() {
            Some(b"ARMO") => ENCHANTMENT_APPAREL,
            Some(b"WEAP" | b"EXPL") => ENCHANTMENT_WEAPON,
            _ => return false,
        };
        if !effect.is_none_or(|e| self.data.enchantment_types.get(&e) == Some(&want)) {
            return false;
        }
        let previous = self.object_effect(form);
        self.field_overrides.insert((form, "EITM/".into()), fnv_script::vm::XVal::Ref(effect.map_or(0, |e| e.0)));
        if self.data.sig(orig) == Some(*b"ARMO") && previous != effect {
            let clones: Vec<FormId> = self.extra.values().flat_map(|x| x.worn_dispelled.iter().copied()).collect();
            let key = "EITM/".to_string();
            let matching: std::collections::HashSet<FormId> = clones
                .into_iter()
                .filter(|&item| item == form || (self.unclone(item) == form && !self.field_overrides.contains_key(&(item, key.clone()))))
                .collect();
            if !matching.is_empty() {
                for x in self.extra.values_mut() {
                    x.worn_dispelled.retain(|item| !matching.contains(item));
                }
            }
            self.sync_loaded_actor_effects();
        }
        true
    }

    /// The enchanted armour an actor wears: (item, its Apparel enchantment). Reads the
    /// inventory as it is; one not yet made (an actor never loaded) wears nothing.
    pub fn worn_enchantments(&self, r: FormId) -> Vec<(FormId, FormId)> {
        let Some(inv) = self.world.peek(r).and_then(|s| s.inventory.as_ref()) else { return Vec::new() };
        let mut out: Vec<(FormId, FormId)> = Vec::new();
        for item in inv.equipped() {
            let orig = self.unclone(item);
            if self.data.sig(orig) != Some(*b"ARMO") {
                continue;
            }
            let ench = match self.object_effect_override_with_orig(item, orig) {
                Some(override_effect) => override_effect,
                None => self.data.armor_enchantments.get(&orig).copied(),
            };
            if let Some(ench) = ench.filter(|e| self.data.enchantment_types.get(e) == Some(&ENCHANTMENT_APPAREL) && self.data.effect_items.contains_key(e)) {
                out.push((item, ench));
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// The worn enchantments' effects follow what the actor has equipped: an item's
    /// enchantment starts when it is worn and ends (its finish blocks run) when it is
    /// not [documented: GECK wiki ActorValue, armour's object effects alter stats "until
    /// you take the armor off"; hypothesis: they begin at once].
    ///
    /// An enchantment a Dispel took off stays off while that item stays equipped
    /// [observed: abilities-1, the gambler hat's enchantment dispelled while worn was
    /// gone until the hat was equipped again]; on an actor not loaded none is active
    /// [hypothesis: as abilities].
    pub fn sync_worn(&mut self, r: FormId) {
        let worn = self.worn_enchantments(r);
        let has_dispelled = if let Some(x) = self.extra.get_mut(&r) {
            if !x.worn_dispelled.is_empty() {
                x.worn_dispelled.retain(|i| worn.iter().any(|w| w.0 == *i));
            }
            !x.worn_dispelled.is_empty()
        } else {
            false
        };
        if worn.is_empty() && !self.effects.iter().any(|e| e.target == r && e.worn.is_some()) {
            return;
        }
        let loaded = self.effects_loaded(r);
        let wanted: Vec<(FormId, FormId)> = if !loaded {
            Vec::new()
        } else if !has_dispelled {
            worn
        } else {
            let off = self.extra.get(&r).map(|x| x.worn_dispelled.as_slice()).unwrap_or(&[]);
            worn.into_iter().filter(|w| !off.contains(&w.0)).collect()
        };
        let mut have: Vec<(FormId, FormId)> = self.effects.iter().filter(|e| e.target == r).filter_map(|e| Some((e.worn?, e.source))).collect();
        have.sort();
        have.dedup();
        for &(item, ench) in &have {
            if !wanted.contains(&(item, ench)) {
                self.dispel(r, None, |e| e.worn == Some(item) && e.source == ench);
            }
        }
        for &(item, ench) in &wanted {
            if !have.contains(&(item, ench)) {
                let mods = crate::aid::Mods { worn: Some(item), ..crate::aid::Mods::default() };
                self.apply_magic_item_with(r, ench, Some(r), true, &mods);
            }
        }
    }

    /// An actor's abilities and worn enchantments brought up to date: before a script
    /// asks about its effects or values, and every frame for the loaded actors. Inside
    /// the effects' own start (a value read while a modifier is put in place) it does
    /// nothing.
    pub fn sync_actor_effects(&mut self, r: FormId) {
        if self.syncing_effects {
            return;
        }
        self.syncing_effects = true;
        self.give_abilities(r);
        if self.wants_perk_sync(r) {
            self.sync_perk_abilities_of(r);
        }
        if r == FormId::PLAYER_REF {
            self.sync_perk_weights();
        }
        self.sync_abilities(r);
        self.sync_worn(r);
        self.syncing_effects = false;
    }

    /// Every frame: the player, the actors in the loaded cells and those that still have
    /// abilities' or worn enchantments' effects (to end them when they leave).
    pub(crate) fn sync_loaded_actor_effects(&mut self) {
        for a in self.effects_actors() {
            self.sync_actor_effects(a);
        }
    }

    /// The actors [`Game::sync_loaded_actor_effects`] keeps up to date: the player, the
    /// loaded ones among those the packages cover, the targets of abilities' and worn
    /// enchantments' effects, and those with an ability dispelled while loaded (whose
    /// dispel ends when they are not, `sync_abilities`).
    ///
    /// Not every actor that ever chose a package (#1992): [`Game::package_actors`] keeps
    /// those for the periodic choice, so a long playthrough synced thousands of actors
    /// out of the loaded area every frame, each a no-op once its effects had ended.
    pub(crate) fn effects_actors(&mut self) -> BTreeSet<FormId> {
        // The loaded part of `package_actors`, built without scanning every `RefExtra` or
        // created reference each frame: authored actors in loaded cells, placed actors
        // whose `RefState.cell` differs from their placement, moved/created actors, active
        // effect targets, and actors with dispelled abilities.
        let placed = self.world.placed;
        let forms = self.world.forms;
        let world_id = self.world.instance_id();
        let is_actor = |r: &FormId| placed.get(*r).is_some_and(|p| matches!(&p.sig, b"ACHR" | b"ACRE"));
        let mut cache = match self.placed_effect_actors.take() {
            Some(mut saved) if std::ptr::eq(saved.placed, placed) && saved.world_id == world_id => {
                if saved.cells != self.loaded_cells {
                    saved.cells = self.loaded_cells.clone();
                    saved.actors = self.loaded_cells.iter().filter_map(|c| placed.by_cell.get(c)).flatten().copied().filter(is_actor).collect();
                }
                for r in self.world.take_touched_refs() {
                    if r >= FormId::FIRST_DYNAMIC {
                        if self.world.peek(r).is_some_and(|s| !s.deleted() && crate::ai::is_actor_base(forms, s.base)) {
                            saved.created_actors.insert(r);
                        } else {
                            saved.created_actors.remove(&r);
                        }
                    } else if let Some(p) = placed.get(r).filter(|p| matches!(&p.sig, b"ACHR" | b"ACRE")) {
                        if self.world.peek(r).is_some_and(|s| s.cell != p.cell) {
                            saved.relocated_actors.insert(r);
                        } else {
                            saved.relocated_actors.remove(&r);
                        }
                    }
                }
                saved
            }
            _ => {
                let _ = self.world.take_touched_refs();
                let actors = self.loaded_cells.iter().filter_map(|c| placed.by_cell.get(c)).flatten().copied().filter(is_actor).collect();
                let mut relocated_actors = BTreeSet::new();
                let mut created_actors = BTreeSet::new();
                for (r, s) in self.world.changed() {
                    if r >= FormId::FIRST_DYNAMIC {
                        if !s.deleted() && crate::ai::is_actor_base(forms, s.base) {
                            created_actors.insert(r);
                        }
                    } else if let Some(p) = placed.get(r).filter(|p| matches!(&p.sig, b"ACHR" | b"ACRE")) {
                        if s.cell != p.cell {
                            relocated_actors.insert(r);
                        }
                    }
                }
                let dispelled_actors = self.extra.iter().filter(|(_, x)| !x.dispelled.is_empty()).map(|(&r, _)| r).collect();
                PlacedEffectActors { placed, world_id, cells: self.loaded_cells.clone(), actors, relocated_actors, created_actors, dispelled_actors }
            }
        };
        let mut actors = cache.actors.clone();
        actors.extend(self.moved.iter().filter(|r| is_actor(r)));
        actors.extend(cache.created_actors.iter().copied());
        for &r in &cache.relocated_actors {
            if self.extra.get(&r).is_some_and(|e| e.current_package.is_some()) && self.cell_of(r).is_some_and(|c| self.loaded_cells.contains(&c)) {
                actors.insert(r);
            }
        }
        actors.retain(|r| self.effects_loaded(*r));
        actors.insert(FormId::PLAYER_REF);
        actors.extend(self.effects.iter().filter(|e| e.worn.is_some() || self.is_ability_effect(e)).map(|e| e.target));
        cache.dispelled_actors.retain(|r| self.extra.get(r).is_some_and(|x| !x.dispelled.is_empty()));
        actors.extend(cache.dispelled_actors.iter().copied());
        self.placed_effect_actors = Some(cache);
        actors
    }

    /// An inactive effect whose conditions now pass takes hold (value change, archetype,
    /// ScriptEffectStart); an active one whose conditions fail lets go (its modifier
    /// leaves its pool, ScriptEffectFinish runs) and stays listed [hypothesis: the
    /// effect is suspended, not removed; aid-items.md §13].
    pub(crate) fn recheck_conditions(&mut self) {
        if self.time + 1e-9 < self.condition_check_due {
            return;
        }
        self.condition_check_due = self.time + CONDITION_INTERVAL;
        let data = self.data;
        let ids: Vec<(u32, FormId, FormId, usize, Option<FormId>, bool)> = self
            .effects
            .iter()
            .filter(|e| data.effect_items.get(&e.source).and_then(|l| l.get(e.index)).is_some_and(|i| !i.conditions.is_empty()))
            .map(|e| (e.id, e.target, e.source, e.index, e.caster, e.inactive))
            .collect();
        for (id, target, source, index, caster, inactive) in ids {
            let Some(item) = data.effect_items.get(&source).and_then(|l| l.get(index)) else { continue };
            let label = format!("{} effect {index}", data.name(source));
            let pass = self.conditions_pass(&item.conditions, Some(target), caster, &label);
            if pass && inactive {
                self.activate_effect(id);
            } else if !pass && !inactive {
                self.deactivate_effect(id);
            }
        }
    }

    pub(crate) fn activate_effect(&mut self, id: u32) {
        let Some(e) = self.effects.iter_mut().find(|e| e.id == id) else { return };
        e.inactive = false;
        self.start_change(id);
        self.effect_archetype_start(id);
        self.effect_started(id);
    }

    pub(crate) fn deactivate_effect(&mut self, id: u32) {
        self.effect_finished(id);
        self.end_change(id);
        if let Some(e) = self.effects.iter_mut().find(|e| e.id == id) {
            e.inactive = true;
        }
    }
}

/// #1992: the frame's effects sync on a long playthrough's state, measured on the
/// owner's FalloutNV.esm: every placed actor has chosen a package (as after many hours
/// played) while one cell is loaded. Times one frame's sync over the actors it covered
/// before (every package actor, the player, the effects' targets) and over
/// [`Game::effects_actors`]. Needs `NVF_DATA` (the game's Data folder); run with
/// `cargo test --release -p fnv-game effects_sync_cost -- --ignored --nocapture`.
#[cfg(test)]
mod effects_sync_cost {
    use crate::game::{Game, GameConfig};
    use crate::GameData;
    use fnv_esm::{LoadOrder, Plugin, Schema};
    use fnv_script::FunctionTable;
    use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
    use std::collections::BTreeSet;
    use std::time::Instant;

    fn frames(g: &mut Game<'_>, actors: &BTreeSet<FormId>, n: usize) -> f64 {
        let t = Instant::now();
        for _ in 0..n {
            for &a in actors {
                g.sync_actor_effects(a);
            }
        }
        t.elapsed().as_secs_f64() * 1000.0 / n as f64
    }

    #[test]
    #[ignore]
    fn effects_sync_cost() {
        let Ok(data_dir) = std::env::var("NVF_DATA") else { return };
        let plugins = vec![Plugin::open(std::path::Path::new(&data_dir).join("FalloutNV.esm")).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let schema = Schema::load_dir(root.join("schemas/fnv")).unwrap();
        let p = root.join("schemas/script/functions.json");
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let forms = FormStore::build(&lo, &schema);
        let refs = RefTable::build(&lo, &schema);
        let data = GameData::build(&lo, &schema, FunctionTable::load(&p).unwrap(), &j, &refs);
        let mut g = Game::new(&data, World::new(&forms, &refs, SimConfig::default()), GameConfig::default());
        let actors: Vec<FormId> = refs.by_cell.values().flatten().copied().filter(|r| refs.get(*r).is_some_and(|p| matches!(&p.sig, b"ACHR" | b"ACRE"))).collect();
        // The cell with the most actors is the loaded one.
        let cell = refs.by_cell.iter().max_by_key(|(_, v)| v.iter().filter(|r| actors.contains(r)).count()).map(|(c, _)| *c).unwrap();
        g.loaded_cells.insert(cell);
        let package = FormId(0x0001_0001);
        for &a in &actors {
            g.extra.entry(a).or_default().current_package = Some(package);
        }
        // Settle once (abilities given, those out of the loaded cell ended).
        let mut before = g.package_actors();
        before.insert(FormId::PLAYER_REF);
        for &a in &before {
            g.sync_actor_effects(a);
        }
        let after = g.effects_actors();
        // The same loaded actors as `package_actors` gives, with the player and the
        // effects' targets.
        let mut expected: BTreeSet<FormId> = g.package_actors().into_iter().filter(|r| g.effects_loaded(*r)).collect();
        expected.insert(FormId::PLAYER_REF);
        assert!(expected.is_subset(&after), "{:?}", expected.difference(&after).collect::<Vec<_>>());
        let (old_ms, new_ms) = (frames(&mut g, &before, 5), frames(&mut g, &after, 5));
        let t = Instant::now();
        for _ in 0..5 {
            std::hint::black_box(g.effects_actors());
        }
        let set_ms = t.elapsed().as_secs_f64() * 1000.0 / 5.0;
        println!("effects sync: the set itself takes {set_ms:.2} ms to build");
        println!(
            "effects sync, one frame: {} actors {old_ms:.2} ms -> {} actors {new_ms:.2} ms ({} placed actors with a package, {} effects)",
            before.len(),
            after.len(),
            actors.len(),
            g.effects.len()
        );
        assert!(after.len() < before.len());
        assert!(after.contains(&FormId::PLAYER_REF));
    }
}

#[cfg(test)]
#[path = "abilities_fast_path_tests.rs"]
mod fast_path_tests;
