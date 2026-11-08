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

#[derive(Debug, Default)]
pub struct ShowOffState {
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
    /// Start and update handlers set or removed this frame, applied at the next frame's
    /// aux timer update: (start, set, the function, the name lower case, the owner)
    /// (`nvse_showoff3`).
    pub pending_handlers: Vec<(bool, bool, XVal, String, FormId)>,
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
    /// SetRadiationExtraData's levels, by reference (`Game::tick_radiation`).
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
}

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

impl Game<'_> {
    /// ShowOff:OnAdd, after an item reached a container: the handlers get the item's base
    /// and the container, and GetAddedItemRefShowOff gives an inventory reference to it
    /// [observed: showoff1, a stimpak AddItem'd to the player: a reference whose base is
    /// type 47; the two-argument handler got the base (47) and the player; a handler
    /// taking no arguments runs too].
    pub(crate) fn showoff_on_add(&mut self, to: FormId, item: FormId) {
        if !self.handled(ON_ADD) {
            return;
        }
        let r = self.temp_ref(to, item);
        let before = self.showoff.added.replace(r);
        self.dispatch_event("ShowOff:OnAdd", vec![XVal::Ref(item.0), XVal::Ref(to.0)], Some(to));
        self.showoff.added = before;
    }

    /// ShowOff:OnExplosionHit, before a blast's damage reaches an actor: the handlers get
    /// the actor and none, read the damage (GetExplosionHitDamage) and may change it
    /// (SetExplosionHitDamage). Returns the damage to deal [observed: showoff1, a
    /// MineFragExplosion at the player read 100, and set to 5 the player lost 5 Health;
    /// hypothesis: the damage is the blast's before armour, as the engine's `b.damage`].
    pub fn explosion_hit(&mut self, target: FormId, damage: f64) -> f64 {
        if !self.handled(ON_EXPLOSION_HIT) {
            return damage;
        }
        let before = self.showoff.explosion_damage.replace(damage);
        self.dispatch_event("ShowOff:OnExplosionHit", vec![XVal::Ref(target.0), XVal::Ref(0)], Some(target));
        let out = self.showoff.explosion_damage.unwrap_or(damage);
        self.showoff.explosion_damage = before;
        out
    }

    /// The stop handlers of an aux timer that ran out or was stopped with its event: each
    /// gets the timer's name and its owner [documented: SetOnAuxTimerStopHandler; observed:
    /// showoff1, once when a 0.1 s timer ran out and once on AuxTimerStop 1, the owner the
    /// player].
    pub(crate) fn aux_timer_stopped(&mut self, owner: FormId, name: &str) {
        let hs: Vec<XVal> = self.showoff.timer_stops.iter().filter(|(_, n, o)| n == name && *o == owner).map(|(f, _, _)| f.clone()).collect();
        for f in hs {
            let ctx = RunCtx { self_ref: Some(owner), ..RunCtx::default() };
            if let Err(error) = self.run_function(&f, Some(fnv_script::vm::Value::Ref(owner.0)), vec![XVal::Str(name.to_string()), XVal::Ref(owner.0)], None, ctx) {
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
        // Outside a ShowOff:OnExplosionHit handler 0 and nothing [documented: unusable
        // outside it].
        "getexplosionhitdamage" => XVal::Num(g.showoff.explosion_damage.unwrap_or(0.0)),
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
            g.showoff.timer_stops.retain(|(h, n, o)| !(*h == f && *n == timer && *o == owner));
            if set {
                g.showoff.timer_stops.push((f, timer, owner));
            }
            XVal::Num(1.0)
        }
        // 1 when the actor has a weapon out, 0 with it away [observed: showoff1, the 10mm
        // pistol]. A weapon without a jam animation gives 0 [documented]; which weapons have
        // one is not read yet, so every drawn weapon counts [hypothesis], and the engine does
        // not play the animation yet [not built].
        "forceweaponjamanim" | "forcejamanim" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let out = g.extra.get(&a).is_some_and(|e| e.weapon_out) && crate::functions::items::equipped_weapon(g, a).is_some();
            XVal::Num(f64::from(u8::from(out)))
        }
        _ => return None,
    };
    Some(Ok(v))
}
