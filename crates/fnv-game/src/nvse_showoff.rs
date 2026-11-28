//! ShowOff functions the mod lists call that no earlier file built
//! (docs/handoff/modlist-function-counts.json; docs/spec-notes/ext-showoff.md)
//! [documented: their pages; observed: showoff1 in the original game, 2026-10-27].
//! ShowOff:OnAdd fires where items reach a container (`Game::item_event`),
//! ShowOff:OnExplosionHit where the engine's blast reaches an actor
//! ([`Game::explosion_hit`]), and the aux timers' stop handlers where a timer runs out
//! (`Game::tick`) or is stopped with its event.

use crate::game::{Game, RunCtx};
use fnv_script::vm::XVal;
use fnv_sim::FormId;

/// The events' names as SetEventHandler takes them, lower case.
pub const ON_ADD: &str = "showoff:onadd";
pub const ON_EXPLOSION_HIT: &str = "showoff:onexplosionhit";
pub const ON_PRE_SCRIPTED_ACTIVATE: &str = "showoff:onprescriptedactivate";
pub const ON_PRE_ACTIVATE_INVENTORY_ITEM: &str = "showoff:onpreactivateinventoryitem";
pub const ON_PRE_ACTIVATE_INVENTORY_ITEM_ALT: &str = "showoff:onpreactivateinventoryitemalt";
pub const ON_POV_CHANGE: &str = "showoff:onpovchange";
pub const ON_COMPLETE_OBJECTIVE: &str = "showoff:oncompleteobjective";
pub const ON_DISPLAY_OBJECTIVE: &str = "showoff:ondisplayobjective";
pub const ON_WEAPON_UNHOLSTER: &str = "showoff:onweaponunholster";
pub const ON_FIRE_WEAPON: &str = "showoff:onfireweapon";

/// Safety cap on unconsumed `ForceWeaponJamAnim` requests in headless runs where no engine
/// drains `ShowOffState::force_jam_requests` [design].
pub const MAX_FORCE_JAM_REQUESTS: usize = 64;

/// Which ShowOff aux timer handler list a deferred registration/removal targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuxTimerHandlerKind {
    Start,
    Stop,
    Update,
}

#[derive(Debug, Default)]
pub struct ShowOffState {
    /// Batch 5's state: the level-up and pause menu asks, live explosions, the shot
    /// switches (`nvse_showoff5`).
    pub b5: crate::nvse_showoff5::State,
    /// The item a ShowOff:OnAdd handler is running for (GetAddedItemRefShowOff).
    pub added: Option<FormId>,
    /// The damage of the explosion a ShowOff:OnExplosionHit handler is running for.
    pub explosion_damage: Option<f64>,
    /// SetOnAuxTimerStopHandler's handlers: (the function, the timer's name lower case,
    /// its owner).
    pub timer_stops: Vec<(XVal, String, FormId)>,
    /// SetOnAuxTimerStartHandler's and SetOnAuxTimerUpdateHandler's handlers, as
    /// `timer_stops` (`nvse_showoff3`).
    pub timer_starts: Vec<(XVal, String, FormId)>,
    pub timer_updates: Vec<(XVal, String, FormId)>,
    /// Start, stop, and update handlers set or removed this frame, applied at the next
    /// frame's aux timer update: (kind, set, the function, the name lower case, the owner)
    /// (`nvse_showoff`, `nvse_showoff3`).
    pub pending_handlers: Vec<(AuxTimerHandlerKind, bool, XVal, String, FormId)>,
    /// Each running aux timer's countdown and flags as AuxTimerStart gave them, by owner
    /// and name lower case (AuxTimerTimeToCountdown; the update flag 256).
    pub countdowns: std::collections::HashMap<(FormId, String), (f64, u32)>,
    /// The game time of the last aux timer update (`Game::aux_timer_updates`).
    pub last_update: Option<f64>,
    /// The seeded randomizer's seeds, by the calling plugin lower case.
    pub seeds: std::collections::HashMap<String, crate::nvse_showoff3::Seeded>,
    /// SetAllyTemp's and SetEnemyTemp's reactions: (faction, other) to 0 neutral, 1 enemy,
    /// 2 ally, 3 friend (`functions::actor::reaction`).
    pub temp_reactions: std::collections::HashMap<(FormId, FormId), i64>,
    /// SetRadiationExtraData's levels, by reference (`Game::radiation_markers_in`),
    /// carried across same-run loads in `Game::Session`.
    pub radiation: std::collections::HashMap<FormId, f32>,
    /// The plugin (lower case) that started each aux timer, by owner and name
    /// (ClearShowoffSavedData, `nvse_showoff4`).
    pub timer_mods: std::collections::HashMap<(FormId, String), String>,
    /// SetShowOffOnCornerMessageEventHandler's functions, and the corner messages queued
    /// since the last frame for them: (text, icon path, seconds) (`nvse_showoff4`).
    pub corner_handlers: Vec<XVal>,
    pub corner_pending: Vec<(String, String, f64)>,
    /// SetItemHotkeyIconPath's paths, by item base.
    pub hotkey_icons: std::collections::HashMap<FormId, String>,
    /// FreezeAmmoRegen: no regenerating weapon gains rounds (`fnv_combat::attack::Cycle`).
    pub ammo_regen_frozen: bool,
    /// SetOwnershipTemp's targets (a reference or a cell) and the owner each had before:
    /// a save writes that one (`Game::snapshot`); SetOwnership or SetCellOwnership on the
    /// target drops the entry [documented: SetOwnershipTemp, session persistent].
    pub temp_owners: std::collections::HashMap<FormId, Option<FormId>>,
    /// Re-entrancy guard while `ShowOff:OnPreActivateInventoryItem[Alt]` handlers run,
    /// so a handler calling `EquipItem` / `UseAidItem` does not recurse infinitely [design].
    pub in_pre_activate_item: bool,
    /// Actors whose draw-start `ShowOff:OnWeaponUnholster` has already fired while
    /// `weapon_out` is still `false`, preventing duplicate firing when draw finishes.
    /// Value is `None` while the draw is active (`AnimAction::Equip` or scripted
    /// `SetWeaponOut 1` pending in `weapon_due`), or `Some(frame)` if `Equip` ended
    /// before `weapon_out` became `true` so same-frame completion still deduplicates
    /// while a later-frame cancelled draw clears cleanly [design].
    pub unholster_pending: std::collections::HashMap<FormId, Option<u64>>,
    /// `(frame, [(actor, weapon)])` already dispatched to `ShowOff:OnFireWeapon` this
    /// frame, deduplicating `weapon_fired` -> `projectile_fired` and multi-pellet bursts
    /// [observed: Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE Plugin 184,
    /// 2026-11-23, GSDocMitchellHouse (0x00103DF9), 10 official base masters, probe
    /// `w1_3632_onfireweapon_and_forcejam` in #3679 (`Player.FireWeapon` on 7-pellet
    /// Caravan Shotgun `0x000CD53A` invokes callback once)].
    pub fired_this_frame: (u64, Vec<(FormId, FormId)>),
    /// Actors asked by `ForceWeaponJamAnim` / `ForceJamAnim` to play their weapon's jam
    /// animation (`Game::take_force_jam_requests`, consumed by `nvf_play::combat::fire`).
    pub force_jam_requests: Vec<FormId>,
}

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

impl Game<'_> {
    /// ShowOff:OnAdd, after an item reached a container: the handlers get the item's base
    /// and the container, with the picked-up world reference (or null for `AddItem` /
    /// inventory transfers) as the calling reference (`GetSelfAlt` / `ThisAlt`), and
    /// `GetAddedItemRefShowOff` gives that world reference or an inventory reference
    /// [documented: GECK wiki ShowOff:OnAdd; observed: showoff1, a stimpak AddItem'd to
    /// the player: a reference whose base is type 47; the two-argument handler got the
    /// base (47) and the player; a handler taking no arguments runs too].
    pub(crate) fn showoff_on_add(&mut self, to: FormId, item: FormId, world_ref: Option<FormId>) {
        if !self.handled(ON_ADD) {
            return;
        }
        let r = world_ref.unwrap_or_else(|| self.temp_ref(to, item));
        let before = self.showoff.added.replace(r);
        self.dispatch_event("ShowOff:OnAdd", vec![XVal::Ref(item.0), XVal::Ref(to.0)], world_ref);
        self.showoff.added = before;
    }

    /// ShowOff:OnExplosionHit, before a blast's damage reaches an actor: calling
    /// reference (`GetSelfAlt`) is the live `EXPL` reference (whose `GetBaseObject` is
    /// the `EXPL` form), argument 1 is the hit actor, and argument 2 is the source actor
    /// (or `0` when none [observed: showoff1]). Handlers read the damage
    /// (`GetExplosionHitDamage`) and may change it (`SetExplosionHitDamage`). Returns the
    /// damage to deal [observed: showoff1, MineFragExplosion at the player read 100 and
    /// set to 5 the player lost 5 Health, and argument 2 read 0; observed 2026-11-24:
    /// Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184 + JIP LN 57.30, 10
    /// official base masters, GSDocMitchellHouse (`0x00103DF9`), isolated clean-room
    /// staging copy, committed probe `w6t2_01_showoff_on_explosion_hit`
    /// (`tools/capture/nvse/catalogs/wave6c_showoff_on_explosion_hit.json`):
    /// `PROBE|w6t2_01_showoff_on_explosion_hit|reg=1,n=1,selfIsP=0,tSelf=81,tBase=81,base=0007078B,hitIsP=1,src=00000014,dmg=100.0`;
    /// no DLL or source consulted; hypothesis: the damage is the blast's before armour,
    /// as the engine's `b.damage`].
    pub fn explosion_hit_with_context(
        &mut self,
        explosion_ref: Option<FormId>,
        target: FormId,
        source: Option<FormId>,
        damage: f64,
    ) -> f64 {
        if !self.handled(ON_EXPLOSION_HIT) {
            return damage;
        }
        let before = self.showoff.explosion_damage.replace(damage);
        self.dispatch_event(
            "ShowOff:OnExplosionHit",
            vec![XVal::Ref(target.0), XVal::Ref(source.map_or(0, |s| s.0))],
            explosion_ref,
        );
        let out = self.showoff.explosion_damage.unwrap_or(damage);
        self.showoff.explosion_damage = before;
        out
    }

    /// Convenience wrapper for [`Game::explosion_hit_with_context`] when no live
    /// explosion reference or source actor is supplied.
    pub fn explosion_hit(&mut self, target: FormId, damage: f64) -> f64 {
        self.explosion_hit_with_context(None, target, None, damage)
    }

    /// Resolve an `EXPL` form's live damage (`Game::explosion_traits`, including
    /// `SetExplosionTraitNumeric` trait 1) and run `ShowOff:OnExplosionHit` on `target`
    /// with a placed live explosion reference as calling reference.
    pub fn explosion_hit_damage(&mut self, explosion: FormId, target: FormId) -> Option<f64> {
        let traits = self.explosion_traits(explosion)?;
        let damage = f64::from(traits.damage);
        if !self.handled(ON_EXPLOSION_HIT) {
            return Some(damage);
        }
        let at = self.ref_pos_rot(target).map_or([0.0; 3], |(p, _)| p);
        let expl_ref = self.explosion_placed(explosion, None, at, traits.radius);
        Some(self.explosion_hit_with_context(expl_ref, target, None, damage))
    }

    /// Whether any `ShowOff:OnPreActivateInventoryItem` or
    /// `ShowOff:OnPreActivateInventoryItemAlt` handler is registered.
    pub fn has_pre_activate_inventory_handlers(&self) -> bool {
        self.handled(ON_PRE_ACTIVATE_INVENTORY_ITEM)
            || self.handled(ON_PRE_ACTIVATE_INVENTORY_ITEM_ALT)
    }

    /// `ShowOff:OnPreActivateInventoryItem` followed by
    /// `ShowOff:OnPreActivateInventoryItemAlt` before a player inventory, hotkey,
    /// quick-use, or stats-page item activation. An explicit numeric `SetFunctionValue 0`
    /// prevents vanilla activation (sticky across handlers). Free when no handlers are
    /// registered. See `docs/spec-notes/preactivation-events.md`.
    pub fn pre_activate_inventory_item(
        &mut self,
        actor: FormId,
        item: FormId,
        key: i32,
        equipping: bool,
    ) -> bool {
        if self.showoff.in_pre_activate_item || !self.has_pre_activate_inventory_handlers() {
            return true;
        }
        self.showoff.in_pre_activate_item = true;
        let inv_ref = self.temp_ref(actor, item);
        let mut allowed = true;
        if self.handled(ON_PRE_ACTIVATE_INVENTORY_ITEM) {
            allowed = self.dispatch_pre_event(
                "ShowOff:OnPreActivateInventoryItem",
                Some(actor),
                Some(actor),
                allowed,
                |allowed| {
                    vec![
                        XVal::Ref(item.0),
                        XVal::Ref(inv_ref.0),
                        XVal::Num(if allowed { 1.0 } else { 0.0 }),
                        XVal::Num(f64::from(key)),
                    ]
                },
            );
        }
        if self.handled(ON_PRE_ACTIVATE_INVENTORY_ITEM_ALT) {
            allowed = self.dispatch_pre_event(
                "ShowOff:OnPreActivateInventoryItemAlt",
                Some(actor),
                Some(actor),
                allowed,
                |allowed| {
                    vec![
                        XVal::Ref(item.0),
                        XVal::Ref(inv_ref.0),
                        XVal::Num(if allowed { 1.0 } else { 0.0 }),
                        XVal::Num(f64::from(key)),
                        XVal::Num(if equipping { 1.0 } else { 0.0 }),
                    ]
                },
            );
        }
        self.showoff.in_pre_activate_item = false;
        allowed
    }

    /// `ShowOff:OnPreActivateInventoryItemAlt` for scripted `EquipItem` / `UseAidItem`
    /// [observed 2026-11-24: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184,
    /// 10 official base masters, `GSDocMitchellHouse`, committed probes
    /// `w6t2_02_showoff_pre_activate_inv_alt` and
    /// `w6t2p2_02_pre_activate_inv_alt_allow_vs_omit`
    /// (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`): scripted
    /// `Player.EquipItem` fires `Alt` only (`na=0,al=1`), with `ThisAlt=Player`,
    /// `isRef=1`, `bNotPrevented=1` (`alw=1`), `iKey=0`, `bEquipping=1`].
    /// Free when no `ShowOff:OnPreActivateInventoryItemAlt` handler is registered.
    pub(crate) fn pre_activate_inventory_item_alt(
        &mut self,
        actor: FormId,
        item: FormId,
        key: i32,
        equipping: bool,
    ) -> bool {
        if self.showoff.in_pre_activate_item || !self.handled(ON_PRE_ACTIVATE_INVENTORY_ITEM_ALT) {
            return true;
        }
        self.showoff.in_pre_activate_item = true;
        let inv_ref = self.temp_ref(actor, item);
        let allowed = self.dispatch_pre_event(
            "ShowOff:OnPreActivateInventoryItemAlt",
            Some(actor),
            Some(actor),
            true,
            |allowed| {
                vec![
                    XVal::Ref(item.0),
                    XVal::Ref(inv_ref.0),
                    XVal::Num(if allowed { 1.0 } else { 0.0 }),
                    XVal::Num(f64::from(key)),
                    XVal::Num(if equipping { 1.0 } else { 0.0 }),
                ]
            },
        );
        self.showoff.in_pre_activate_item = false;
        allowed
    }

    /// `ShowOff:OnPreScriptedActivate` before a scripted `Activate` runs its block or
    /// default action. Calling reference is `target`; arguments are `(actor,
    /// run_on_activate_block, allowed)` [observed 2026-11-23: Fallout: New Vegas
    /// 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, 10 official base masters,
    /// `so_31_pre_scripted_activate_args_veto` / `so_31b_pre_scripted_activate_noveto`:
    /// `Activate Player 0` passes `(0, 1)`, `Activate Player 1` passes `(1, 1)`, and
    /// `SetFunctionValue 0` vetoes pickup; no DLL or source consulted].
    /// See `docs/spec-notes/preactivation-events.md`.
    pub(crate) fn pre_scripted_activation_allows(
        &mut self,
        actor: FormId,
        target: FormId,
        run_block: bool,
    ) -> bool {
        if !self.handled(ON_PRE_SCRIPTED_ACTIVATE) {
            return true;
        }
        self.dispatch_pre_event(
            "ShowOff:OnPreScriptedActivate",
            Some(target),
            Some(actor),
            true,
            |allowed| {
                vec![
                    XVal::Ref(actor.0),
                    XVal::Num(if run_block { 1.0 } else { 0.0 }),
                    XVal::Num(if allowed { 1.0 } else { 0.0 }),
                ]
            },
        )
    }

    /// `ShowOff:OnPOVChange`, fired when the player's camera perspective transitions
    /// between first-person (`1`) and third-person (`0`), with calling reference
    /// `Player` (`0x14`) [documented: GECK wiki ShowOff:OnPOVChange; observed 2026-11-24:
    /// `w6t2p2_03_showoff_on_pov_change_toggle`: `s0=0,sP=1`].
    pub(crate) fn showoff_on_pov_change(&mut self, first_person: bool) {
        if !self.handled(ON_POV_CHANGE) {
            return;
        }
        self.dispatch_event(
            "ShowOff:OnPOVChange",
            vec![XVal::Num(if first_person { 1.0 } else { 0.0 })],
            Some(FormId::PLAYER_REF),
        );
    }

    /// `ShowOff:OnDisplayObjective`, fired PRE-update when a quest objective transitions
    /// from hidden to displayed (`GetObjectiveDisplayed` still reads `0` inside the
    /// callback) [documented: GECK wiki ShowOff:OnDisplayObjective; observed: Fallout:
    /// New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE Plugin 184, 2026-11-23,
    /// GSDocMitchellHouse (0x00103DF9), 10 official base masters, probes
    /// `w1_3656_objectives_unholster_imod` (`StartQuest 0x000E282D`,
    /// `SetObjectiveDisplayed 20 1`/`20 1`/`20 0`/`20 1`/`999 1` + `CompleteQuest`) and
    /// `b3_so_ex_06_on_display_complete_objective` (`SetObjectiveDisplayed 0x00104C1C 10
    /// 1` -> `reg=1,hit=1,dispInside=0`) in #3679]. Arguments are `(quest,
    /// objective_index)`; calling reference is `None`. Free when no handler is
    /// registered.
    pub(crate) fn showoff_on_display_objective(&mut self, quest: FormId, index: i32) {
        if !self.handled(ON_DISPLAY_OBJECTIVE) {
            return;
        }
        self.dispatch_event(
            "ShowOff:OnDisplayObjective",
            vec![XVal::Ref(quest.0), XVal::Num(f64::from(index))],
            None,
        );
    }

    /// `ShowOff:OnCompleteObjective`, fired PRE-update when a currently displayed quest
    /// objective transitions from incomplete to completed (`GetObjectiveCompleted` still
    /// reads `0` inside the callback; hidden objectives and completed quests do not fire)
    /// [documented: GECK wiki ShowOff:OnCompleteObjective; observed: Fallout: New Vegas
    /// 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE Plugin 184, 2026-11-23, GSDocMitchellHouse
    /// (0x00103DF9), 10 official base masters, probe `w1_3656_objectives_unholster_imod`
    /// in #3679 (`StartQuest 0x000E282D`, `SetObjectiveCompleted` 20/30 hidden +
    /// `SetObjectiveDisplayed 50 1`/`5 1` + `CompleteAllObjectives` firing 50 then 5 +
    /// `CompleteQuest`)]. Arguments are `(quest, objective_index)`; calling reference is
    /// `None`. Free when no handler is registered.
    pub(crate) fn showoff_on_complete_objective(&mut self, quest: FormId, index: i32) {
        if !self.handled(ON_COMPLETE_OBJECTIVE) {
            return;
        }
        self.dispatch_event(
            "ShowOff:OnCompleteObjective",
            vec![XVal::Ref(quest.0), XVal::Num(f64::from(index))],
            None,
        );
    }

    /// Mark an actor as starting to draw its weapon while `weapon_out == false` and fire
    /// `ShowOff:OnWeaponUnholster` once at draw start (PRE-update, `IsWeaponOut == 0`).
    /// Subsequent draw-completion calls (`set_weapon_out(actor, true)` or `hud_tick`)
    /// consume `unholster_pending` without double-firing [design].
    pub(crate) fn showoff_begin_unholster(&mut self, actor: FormId) {
        if self.extra.get(&actor).is_some_and(|e| e.weapon_out) {
            return;
        }
        // If an earlier Equip ended on a prior frame without completing, treat this as a
        // fresh draw attempt.
        if let Some(Some(ended)) = self.showoff.unholster_pending.get(&actor).copied() {
            if ended < self.frame {
                self.showoff.unholster_pending.remove(&actor);
            }
        }
        if let std::collections::hash_map::Entry::Vacant(e) = self.showoff.unholster_pending.entry(actor) {
            e.insert(None);
            self.showoff_on_weapon_unholster(actor);
        }
    }

    /// `ShowOff:OnWeaponUnholster`, fired at weapon draw start PRE-update so
    /// `IsWeaponOut` still reads `0` inside the callback [documented: GECK wiki
    /// ShowOff:OnWeaponUnholster; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
    /// ShowOffNVSE Plugin 184, 2026-11-23, GSDocMitchellHouse (0x00103DF9), 10 official
    /// base masters, probe `w1_3656_objectives_unholster_imod` in #3679
    /// (`Player.SetWeaponOut 0` / `Player.SetWeaponOut 1` with `Weap10mmPistol`
    /// `0x0000434F` -> `self=0x14, arg1=0x434F, IsWeaponOut=0`; unarmed
    /// `Player.SetWeaponOut 1` -> `self=0x14, arg1=0, IsWeaponOut=0`)].
    /// Calling reference (`GetSelf` / `GetSelfAlt` / `0::`) is `actor`; single argument
    /// (`"first"` / `1::`) is the actor's equipped or engine-held `WEAP` base form, or
    /// `0` when unarmed. Free when no handler is registered.
    pub(crate) fn showoff_on_weapon_unholster(&mut self, actor: FormId) {
        if !self.handled(ON_WEAPON_UNHOLSTER) {
            return;
        }
        let weapon = crate::functions::items::equipped_weapon(self, actor)
            .or_else(|| self.extra.get(&actor).and_then(|e| e.held_weapon))
            .unwrap_or(FormId(0));
        self.dispatch_event("ShowOff:OnWeaponUnholster", vec![XVal::Ref(weapon.0)], Some(actor));
    }

    /// `ShowOff:OnFireWeapon`: fired when an actor fires a ranged weapon. Calling
    /// reference (`0::`) is `actor`; first argument (`1::` / `"first"::`) is `weapon`
    /// base form, firing once per shot even on multi-pellet weapons [documented: GECK
    /// wiki Category:Event Handlers (ShowOff NVSE), SetEventHandlerAlt; observed:
    /// Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE Plugin 184, 2026-11-23,
    /// GSDocMitchellHouse (0x00103DF9), 10 official base masters, probe
    /// `w1_3632_onfireweapon_and_forcejam` in #3679 (`SetEventHandlerAlt
    /// "ShowOff:OnFireWeapon"` with `begin Function { ref rWeap }`, `Player.FireWeapon`
    /// with `Weap10mmPistol` `0x0000434F` and 7-pellet Caravan Shotgun `0x000CD53A` ->
    /// `self=0x14`, `weap=WEAP base`, 7-pellet shotgun fires once); observed 2026-11-24:
    /// committed probe `w6t2p3_04_fire_god_melee`
    /// (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`):
    /// `norm(so=1,jp=1)|god(so=1,jp=0)|melee(so=1,jp=1)` confirming `ShowOff:OnFireWeapon`
    /// still fires for the player in god mode (`ToggleGodMode`) while JIP suppresses;
    /// hypothesis: NPC-actor dispatch, non-actor exclusion, and melee animation types
    /// `0..=2` exclusion during normal combat (scripted `Player.FireWeapon` on a melee
    /// weapon fired both handlers in `w6t2p3_04_fire_god_melee`)]. Free when no handler is
    /// registered.
    pub(crate) fn showoff_on_fire_weapon(&mut self, actor: FormId, weapon: FormId, dedup_same_frame: bool) -> usize {
        if !self.handled(ON_FIRE_WEAPON) || weapon.0 == 0 {
            return 0;
        }
        let is_actor = actor == FormId::PLAYER_REF
            || matches!(
                crate::functions::base_of(self, actor).and_then(|b| self.data.sig(b)).as_ref(),
                Some(b"NPC_") | Some(b"CREA")
            );
        if !is_actor {
            return 0;
        }
        if matches!(self.data.weapon_anim_types.get(&self.unclone(weapon)), Some(0..=2)) {
            return 0;
        }
        if self.showoff.fired_this_frame.0 != self.frame {
            self.showoff.fired_this_frame = (self.frame, Vec::new());
        }
        let key = (actor, weapon);
        if dedup_same_frame && self.showoff.fired_this_frame.1.contains(&key) {
            return 0;
        }
        if !self.showoff.fired_this_frame.1.contains(&key) {
            self.showoff.fired_this_frame.1.push(key);
        }
        self.dispatch_event("ShowOff:OnFireWeapon", vec![XVal::Ref(weapon.0)], Some(actor))
    }

    /// Take queued `ForceWeaponJamAnim` / `ForceJamAnim` actor requests for the engine's
    /// combat playback (`nvf_play::combat::fire`).
    pub fn take_force_jam_requests(&mut self) -> Vec<FormId> {
        std::mem::take(&mut self.showoff.force_jam_requests)
    }

    /// Whether `actor` has a queued `ForceWeaponJamAnim` / `ForceJamAnim` request not yet
    /// consumed by `nvf_play::combat::fire`, so post-script animation sync preserves
    /// `AnimAction::Jam` without clobbering it back to `None`/`Reload` and double-firing
    /// `SetOnReloadWeaponEventHandler`.
    pub fn has_pending_force_jam(&self, actor: FormId) -> bool {
        self.showoff.force_jam_requests.contains(&actor)
    }

    /// The stop handlers of an aux timer that ran out or was stopped with its event: each
    /// gets the timer's name and its owner with null calling reference (`GetSelfAlt == 0`)
    /// [documented: SetOnAuxTimerStopHandler; observed: 2026-11-24, Fallout: New Vegas
    /// 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, GSDocMitchellHouse (0x00103DF9),
    /// 10 official base masters,
    /// `tools/capture/nvse/catalogs/wave6d1_aux_timer_stop_deferred_and_self.json` probe
    /// `w6d1p3_27_aux_timer_stop`: `same=0,noev=0,next=1,remSame=1|nm=[sNm],ownP=1,s0=1,sP=0`;
    /// no DLL, PDB, or third-party source consulted].
    pub(crate) fn aux_timer_stopped(&mut self, owner: FormId, name: &str) {
        let hs: Vec<XVal> = self.showoff.timer_stops.iter().filter(|(_, n, o)| n == name && *o == owner).map(|(f, _, _)| f.clone()).collect();
        for f in hs {
            let ctx = RunCtx::default();
            if let Err(error) = self.run_function(&f, None, vec![XVal::Str(name.to_string()), XVal::Ref(owner.0)], None, ctx) {
                self.emit(crate::trace::EventKind::ScriptError { unit: "aux timer stop handler".into(), error });
            }
        }
    }
}

/// The functions; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let v = match name {
        // Outside a ShowOff:OnAdd handler none [documented: only inside one].
        "getaddeditemrefshowoff" => XVal::Ref(g.showoff.added.map_or(0, |r| r.0)),
        // Outside a ShowOff:OnExplosionHit handler returns -1.0 [observed: 2026-11-24,
        // Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, GSDocMitchellHouse
        // (0x00103DF9), 10 official base masters,
        // `tools/capture/nvse/catalogs/wave6d1_get_explosion_hit_damage_outside.json`
        // probe `w6d1_18_on_explosion_hit_armor_dt`:
        // `dt50(dmg=100.0,loss=75.0)|set30(dmg=100.0,loss=9.0)|rad=192.0,srcP=0,out=-1.0`;
        // no DLL, PDB, or third-party source consulted].
        "getexplosionhitdamage" => XVal::Num(g.showoff.explosion_damage.unwrap_or(-1.0)),
        "setexplosionhitdamage" => {
            if g.showoff.explosion_damage.is_some() {
                g.showoff.explosion_damage = Some(num(args, 0, 0.0));
            }
            XVal::Num(0.0)
        }
        // `SetOnAuxTimerStopHandler 1/0 handler timerName [owner]`, the owner the player when
        // none [documented].
        "setonauxtimerstophandler" | "setontimerstophandler" => {
            let set = num(args, 0, 1.0) != 0.0;
            let Some(f) = args.get(1).cloned() else { return Some(Ok(XVal::Num(0.0))) };
            let timer = match args.get(2) {
                Some(XVal::Str(s)) => s.to_ascii_lowercase(),
                _ => String::new(),
            };
            let owner = match args.get(3) {
                Some(XVal::Ref(r)) if *r != 0 => FormId(*r),
                _ => subject.filter(|_| args.len() < 4).unwrap_or(FormId::PLAYER_REF),
            };
            // Applied at the next frame (`Game::aux_timer_updates`), matching Start/Update
            // [observed: `w6d1p3_27_aux_timer_stop`, `same=0,noev=0,next=1,remSame=1`].
            g.showoff.pending_handlers.push((AuxTimerHandlerKind::Stop, set, f, timer, owner));
            XVal::Num(1.0)
        }
        // 1 when the actor is alive, has a weapon out, and the weapon has a jam animation
        // (including when already jamming); 0 otherwise [documented: GECK wiki
        // ForceWeaponJamAnim (alias ForceJamAnim; .357 revolver has no jam animation and
        // returns 0) and GetWeaponAnimJamTime; observed: showoff1, 10mm pistol away = 0
        // and drawn = 1; observed: FalloutNV.esm WEAP DNAM Weap10mmPistol jam_time = 1.4
        // vs WeapNV357Revolver jam_time = 0.0; observed: Fallout: New Vegas 1.4.0.525,
        // xNVSE 6.4.9, ShowOffNVSE Plugin 184, 2026-11-23/24, GSDocMitchellHouse
        // (0x00103DF9), 10 official base masters, probes `w1_3632_onfireweapon_and_forcejam`
        // (`Player.ForceWeaponJamAnim` on Player 0x14: holstered 10mm `0x0000434F` = 0,
        // drawn 10mm = 1 with `GetAnimAction = 9`, `IsAnimPlayingEx 4 = 0`, repeat call
        // while jamming = 1, drawn Baseball Bat `0x0000421C` = 0, drawn Fists = 0),
        // `so_34_force_weapon_jam_revolver` (`AddItem`/`EquipItem` + draw `.357`
        // `0x0008F216` -> 0 vs 10mm `0x0000434F` -> 1), and
        // `w4d_08_forcejam_melee_god_nonactor` (`door=0|melee(ret=0,act=0)|tgm10mm(ret=1,act=9)`)
        // in #3679; hypothesis: animation_type 3..=9 + DNAM/jam_time > 0 as the
        // rules-side jam-animation check].
        // Queues the actor for `nvf_play::combat::fire` to trigger `Cycle::jam` and
        // `WeaponAnim::jams`.
        "forceweaponjamanim" | "forcejamanim" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let ok = !g.is_dead(a)
                && g.extra.get(&a).is_some_and(|e| e.weapon_out)
                && crate::functions::items::equipped_weapon(g, a).is_some_and(|w| weapon_has_jam_anim(g, w));
            if ok {
                if !g.showoff.force_jam_requests.contains(&a) && g.showoff.force_jam_requests.len() < MAX_FORCE_JAM_REQUESTS {
                    g.showoff.force_jam_requests.push(a);
                }
                let mut st = g.senses.anim.get(&a).cloned().unwrap_or_default();
                st.action = Some(crate::anim_state::AnimAction::Jam);
                g.set_anim_state(a, st);
            }
            XVal::Num(f64::from(u8::from(ok)))
        }
        _ => return None,
    };
    Some(Ok(v))
}

/// Whether `weapon` is a gun with a jam animation [documented: GECK wiki
/// ForceWeaponJamAnim and GetWeaponAnimJamTime; observed: FalloutNV.esm WEAP DNAM
/// WeapNV357Revolver has jam_time = 0.0 while Weap10mmPistol has jam_time = 1.4;
/// hypothesis: checking `DNAM/animation_type` in 3..=9 (guns, excluding melee 0..=2 and
/// thrown/mines 10..=13) and finite `DNAM/jam_time > 0.0`].
fn weapon_has_jam_anim(g: &Game, weapon: FormId) -> bool {
    let anim_type = match crate::nvse::record_value(g, weapon, "DNAM", "animation_type") {
        Some(XVal::Num(n)) if n.is_finite() => Some(n.trunc() as i64),
        _ => g.data.weapon_anim_types.get(&g.unclone(weapon)).copied(),
    };
    if !matches!(anim_type, Some(3..=9)) {
        return false;
    }
    matches!(
        crate::nvse::record_value(g, weapon, "DNAM", "jam_time"),
        Some(XVal::Num(t)) if t.is_finite() && t > 0.0
    )
}
