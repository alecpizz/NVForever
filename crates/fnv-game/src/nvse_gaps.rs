//! The most called extender functions the mod lists use that no earlier set built
//! (docs/handoff/modlist-function-counts.json) [documented: their pages; observed: the
//! peer's gaps2, gaps3 and gaps4 sets in the original game]. Record reads come from the
//! schema's decoded fields (`GameData::record_fields`, enums as their numbers).

use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use serde_json::Value as Json;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Number of JIP LN weapon sound slots (`0..=13`: `SNAM` 0/1, `XNAM`, `NAM7`, `TNAM`,
/// `NAM6`, `UNAM`, `NAM9`, `NAM8`, `WMS1` 0/1, `WMS2`, `YNAM`, `ZNAM`)
/// [documented: GECK wiki GetWeaponSound / SetWeaponSound].
pub const WEAPON_SOUND_SLOTS: usize = 14;

/// A weapon's sound slots as scripts changed them this session
/// ([`Game::weapon_sound_changes`]), indexed by JIP `soundID` `0..=13`.
///
/// - `None`: script left this slot at its authored record value.
/// - `Some(None)`: script cleared this slot (`SetWeaponSound weap id` / null).
/// - `Some(Some(sound))`: script replaced this slot with `sound` (a `SOUN` or cloned `SOUN`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WeaponSoundChange {
    pub slots: [Option<Option<FormId>>; WEAPON_SOUND_SLOTS],
}

/// Biped equipment slot index for the equipped weapon (`ReloadEquippedModels 5`)
/// [documented: ReloadEquippedModels, biped slot table].
pub const EQUIP_SLOT_WEAPON: u8 = 5;
/// Bit mask for the weapon equipment slot (`1 << 5`).
pub const EQUIP_SLOT_WEAPON_BIT: u32 = 1 << EQUIP_SLOT_WEAPON;
/// Bit mask covering all 20 biped equipment slots (`0..=19`) when `ReloadEquippedModels`
/// is called without a slot argument [documented: ReloadEquippedModels].
pub const ALL_EQUIP_SLOTS: u32 = (1 << 20) - 1;
/// Bound on queued non-player `ReloadEquippedModels` requests when no renderer drains them
/// [design].
const EQUIP_RELOAD_QUEUE_LIMIT: usize = 1024;

/// A placed radio station, as the engine reads it.
#[derive(Debug, Clone)]
pub struct RadioSite {
    pub station: FormId,
    pub initially_disabled: bool,
    /// On the Pip-Boy's list (the station has no Non-Pipboy flag).
    pub on_pipboy: bool,
    pub broadcast: fnv_world::radio::Broadcast,
}

/// This file's session state.
#[derive(Debug, Default)]
pub struct GapsState {
    /// SetCombatDisabled's actors.
    pub combat_disabled: HashSet<FormId>,
    /// The Pip-Boy light, and when a script turned it off: the light reads off from the
    /// next frame on [observed: gaps2, `TogglePipBoyLight 0` still reads 1 in its frame
    /// and 0 in the next; on reads 1 at once].
    pub pipboy_light: bool,
    pub light_off_at: Option<f64>,
    /// SetRefName's names, by reference; GetName does not read them [observed: gaps4].
    pub ref_names: HashMap<FormId, String>,
    /// SetActorGravityMult's multipliers.
    pub gravity: HashMap<FormId, f64>,
    /// ToggleBipedSlotVisibility's hidden slots.
    pub hidden_slots: HashSet<u32>,
    /// References whose activation SetInteractionDisabled turned off.
    pub interaction_disabled: HashSet<FormId>,
    /// SetNthEffectBase's bases: (item, effect index).
    pub effect_bases: HashMap<(FormId, usize), FormId>,
    /// Actors whose special idle scripts stopped (StopIdle, ForceStopIdle), for the
    /// engine to take.
    pub idle_stops: Vec<FormId>,
    /// POV-scoped `ForceStopIdle` requests (`1` third-person, `2` first-person), for the
    /// engine to take without stopping the other player rig [documented: ForceStopIdle].
    pub idle_stop_pov: Vec<(FormId, crate::idles::IdlePov)>,
    /// The placed radio stations, from the engine; `None` until the engine sets them.
    pub radio_sites: Option<Vec<RadioSite>>,
    /// The armed mines of the loaded cells that someone other than the player put down,
    /// from the engine's mines at their last frame.
    pub armed_mines: HashSet<FormId>,
    /// Seconds each actor's body has been off the ground, from the engine's physics at
    /// the last step (absent on the ground).
    pub air_times: HashMap<FormId, f32>,
    /// The third JIP batch's state (`crate::nvse_jip3`).
    pub jip3: crate::nvse_jip3::Jip3State,
    /// The fourth JIP batch's state (`crate::nvse_jip4`).
    pub jip4: crate::nvse_jip4::Jip4State,
    /// The shots in flight, for the projectile functions (`crate::nvse_proj`).
    pub projs: crate::nvse_proj::ProjState,
    /// The sounds scripts played, for the sound functions (`crate::nvse_sound`).
    pub sounds: crate::nvse_sound::SoundState,
    /// PushObject's pushes, for the engine to take: (the reference, the push's origin,
    /// its force).
    pub pushes: Vec<(FormId, [f32; 3], f32)>,
    /// ToggleFirstPerson's and TogglePlayerSneaking's asks, for the engine to take.
    pub view_request: Option<bool>,
    pub sneak_request: Option<bool>,
    /// JohnnyGuitar's actor value change handlers: (the function, the code it waits for,
    /// whether it takes the values as they are rather than rounded: flag bit 0).
    pub av_handlers: Vec<(XVal, Option<u16>, bool)>,
    /// An actor value change handler runs: the changes it makes are not told again.
    pub av_telling: bool,
    /// SetMapMarkerName's names, by marker reference.
    pub marker_names: HashMap<FormId, String>,
    /// JohnnyGuitar's quest handlers: (on start, the function, the quest it waits for).
    pub quest_handlers: Vec<(bool, XVal, Option<FormId>)>,
    /// JohnnyGuitar's perk handlers: (on add, the function, the perk it waits for).
    pub perk_handlers: Vec<(bool, XVal, Option<FormId>)>,
    /// The angles GetAngle still reads after a SetAngle this frame: (reference, axis) ->
    /// degrees.
    pub angles_shown: HashMap<(FormId, usize), f64>,
    /// SetMapMarkerRep's reputations, by marker reference (`None`: cleared).
    pub marker_reputations: HashMap<FormId, Option<FormId>>,
    /// SetRecipeCategory's and SetRecipeSubcategory's forms: (recipe, sub).
    pub recipe_categories: HashMap<(FormId, bool), FormId>,
    /// The music type a script last asked for (PlayMusic), and whether the engine plays
    /// any music.
    pub music_type: Option<FormId>,
    pub music_any: bool,
    /// The player's always-run switch as the engine tells it, and SetAlwaysRun's ask.
    pub always_run: bool,
    pub always_run_request: Option<bool>,
    /// RunScriptSnippet's snippets waiting: (frames left, source, reference).
    pub snippets: Vec<(u32, String, FormId)>,
    /// ToggleItemUnique's unique weapon/armor base forms (excluded as repair-part sources).
    pub unique_items: HashSet<FormId>,
    /// Session weapon-sound overrides (`SetWeaponSound`, `SetPickupSound`/`SetPutdownSound`
    /// on `WEAP`, plus `TempCloneForm` snapshots): `(weapon, slot 0..14) -> Some(SOUN)` or
    /// `None` when cleared.
    pub weapon_sounds: HashMap<(FormId, u8), Option<FormId>>,
    /// Transient one-shot `ReloadEquippedModels` slot masks by actor (not saved) [design].
    pub equipped_model_reloads: BTreeMap<FormId, u32>,
}

impl GapsState {
    pub(crate) fn queue_equipped_model_reload(&mut self, actor: FormId, mask: u32) {
        if mask == 0 {
            return;
        }
        let map = &mut self.equipped_model_reloads;
        if actor != FormId::PLAYER_REF && !map.contains_key(&actor) {
            let non_player = map.len().saturating_sub(usize::from(map.contains_key(&FormId::PLAYER_REF)));
            if non_player >= EQUIP_RELOAD_QUEUE_LIMIT {
                if let Some(evict) = map.keys().copied().find(|&id| id != FormId::PLAYER_REF) {
                    map.remove(&evict);
                }
            }
        }
        *map.entry(actor).or_insert(0) |= mask;
    }

    pub(crate) fn take_player_equipped_model_reload(&mut self) -> u32 {
        self.equipped_model_reloads.remove(&FormId::PLAYER_REF).unwrap_or(0)
    }

    pub(crate) fn take_npc_equipped_model_reloads(&mut self) -> BTreeMap<FormId, u32> {
        let player = self.equipped_model_reloads.remove(&FormId::PLAYER_REF);
        let npcs = std::mem::take(&mut self.equipped_model_reloads);
        if let Some(mask) = player {
            self.equipped_model_reloads.insert(FormId::PLAYER_REF, mask);
        }
        npcs
    }
}

impl Game<'_> {
    /// Queue a one-shot `ReloadEquippedModels` slot bitmask for `actor` (OR-coalescing
    /// repeated calls in the same frame; `PLAYER_REF` is never evicted) [design].
    pub(crate) fn queue_equipped_model_reload(&mut self, actor: FormId, mask: u32) {
        self.gaps.queue_equipped_model_reload(actor, mask);
    }

    /// Pending `ReloadEquippedModels` slot bitmask for `actor` (`0` when none).
    pub fn equipped_model_reload_mask(&self, actor: FormId) -> u32 {
        self.gaps.equipped_model_reloads.get(&actor).copied().unwrap_or(0)
    }

    /// Consume the pending `ReloadEquippedModels` slot bitmask for the player (`0` if none).
    pub fn take_player_equipped_model_reload(&mut self) -> u32 {
        self.gaps.take_player_equipped_model_reload()
    }

    /// Consume all pending non-player `ReloadEquippedModels` requests for NPC/creature systems.
    pub fn take_npc_equipped_model_reloads(&mut self) -> BTreeMap<FormId, u32> {
        self.gaps.take_npc_equipped_model_reloads()
    }

    /// Consume all pending `ReloadEquippedModels` requests (player and NPCs).
    pub fn take_equipped_model_reloads(&mut self) -> BTreeMap<FormId, u32> {
        std::mem::take(&mut self.gaps.equipped_model_reloads)
    }
}

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        XVal::Num(x) if *x >= 1.0 && x.fract() == 0.0 => Some(FormId(*x as u32)),
        _ => None,
    }
}

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x,
        _ => dflt,
    }
}

fn text(args: &[XVal], k: usize) -> String {
    match args.get(k) {
        Some(XVal::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn parse_json_form(v: Option<&Json>) -> Option<FormId> {
    let s = v?.as_str()?.strip_prefix("0x")?;
    let id = u32::from_str_radix(s, 16).ok()?;
    (id != 0).then_some(FormId(id))
}

fn parse_weapon_sound_slot(args: &[XVal], k: usize) -> Option<u8> {
    match args.get(k)? {
        XVal::Num(x) if x.is_finite() && x.fract() == 0.0 && (0.0..WEAPON_SOUND_SLOTS as f64).contains(x) => Some(*x as u8),
        _ => None,
    }
}

/// Parses `SetWeaponSound`'s optional `newSound` argument:
/// - `Some(None)` when omitted or explicitly null (`0`) -> clear slot.
/// - `Some(Some(id))` when a non-zero form/integer ID is supplied -> validate as `SOUN`.
/// - `None` when a malformed non-null argument (string, negative/fractional number, etc.) is
///   supplied -> reject without mutating state.
fn parse_weapon_sound_arg(args: &[XVal], k: usize) -> Option<Option<FormId>> {
    match args.get(k) {
        None | Some(XVal::Ref(0)) => Some(None),
        Some(XVal::Num(x)) if *x == 0.0 => Some(None),
        Some(XVal::Ref(r)) => Some(Some(FormId(*r))),
        Some(XVal::Num(x)) if x.is_finite() && *x >= 1.0 && *x <= u32::MAX as f64 && x.fract() == 0.0 => Some(Some(FormId(*x as u32))),
        _ => None,
    }
}

fn json_num(v: Option<&Json>) -> f64 {
    v.and_then(|v| v.get("value").filter(|_| v.get("set").is_some()).unwrap_or(v).as_f64()).unwrap_or(0.0)
}

/// A list array of the values.
fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// The bit `bit` of a flags number.
fn bit(flags: f64, bit: f64) -> XVal {
    yes((0.0..64.0).contains(&bit) && (flags as u64) >> (bit as u64) & 1 != 0)
}

impl Game<'_> {
    /// JohnnyGuitar's perk handlers on the player gaining or losing a perk: (perk, rank
    /// before, rank after) on a gain, (perk) on a loss [observed: plugins3, Toughness
    /// gave (Toughness, 0, 1) and then Toughness].
    pub(crate) fn perk_event(&mut self, perk: FormId, gained: bool, before: u8, after: u8) {
        let hs: Vec<XVal> = self
            .gaps
            .perk_handlers
            .iter()
            .filter(|(add, _, p)| *add == gained && p.is_none_or(|p| p == perk || crate::functions::list_members(self, p).contains(&perk)))
            .map(|(_, f, _)| f.clone())
            .collect();
        for f in hs {
            let args = if gained { vec![XVal::Ref(perk.0), XVal::Num(f64::from(before)), XVal::Num(f64::from(after))] } else { vec![XVal::Ref(perk.0)] };
            if let Err(error) = self.run_function(&f, None, args, None, RunCtx::default()) {
                self.handler_failed("JohnnyGuitar perk handler".into(), error);
            }
        }
    }

    /// JohnnyGuitar's actor value change handlers on a change of one of the player's
    /// values: (code, old value, new value), truncated to whole numbers unless the
    /// handler was set with flag bit 0 [documented: SetOnActorValueChangeEventHandler;
    /// observed 2026-11-24: `200.0 -> 199.4` fires `200.0 -> 199.0` and `199.4 -> 198.8`
    /// fires `199.0 -> 198.0`] (`functions::actor` tells them).
    pub(crate) fn av_event(&mut self, _actor: FormId, code: u16, old: f64, new: f64) {
        let hs: Vec<(XVal, bool)> = self.gaps.av_handlers.iter().filter(|(_, c, _)| c.is_none_or(|c| c == code)).map(|(f, _, exact)| (f.clone(), *exact)).collect();
        let was = std::mem::replace(&mut self.gaps.av_telling, true);
        for (f, exact) in hs {
            let (o, n) = if exact { (old, new) } else { (old.trunc(), new.trunc()) };
            if !exact && o == n {
                continue;
            }
            let args = vec![XVal::Num(f64::from(code)), XVal::Num(o), XVal::Num(n)];
            if let Err(error) = self.run_function(&f, None, args, None, RunCtx::default()) {
                self.handler_failed("JohnnyGuitar actor value handler".into(), error);
            }
        }
        self.gaps.av_telling = was;
    }

    /// JohnnyGuitar's quest handlers on a quest starting or stopping: (quest)
    /// [documented: SetJohnnyOnStartQuestEventHandler; hypothesis: the arguments].
    pub(crate) fn quest_event(&mut self, quest: FormId, started: bool) {
        let hs: Vec<XVal> = self
            .gaps
            .quest_handlers
            .iter()
            .filter(|(s, _, q)| *s == started && q.is_none_or(|q| q == quest || crate::functions::list_members(self, q).contains(&quest)))
            .map(|(_, f, _)| f.clone())
            .collect();
        for f in hs {
            if let Err(error) = self.run_function(&f, None, vec![XVal::Ref(quest.0)], None, RunCtx::default()) {
                self.handler_failed("JohnnyGuitar quest handler".into(), error);
            }
        }
    }

    /// RunScriptSnippet's snippets whose delay has passed run (`Game::tick`).
    pub(crate) fn run_due_snippets(&mut self) {
        self.gaps.angles_shown.clear();
        if self.gaps.snippets.is_empty() {
            return;
        }
        for s in &mut self.gaps.snippets {
            s.0 = s.0.saturating_sub(1);
        }
        let (due, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut self.gaps.snippets).into_iter().partition(|s| s.0 == 0);
        self.gaps.snippets = wait;
        for (_, src, on) in due {
            run_snippet(self, &src, on);
        }
    }

    /// The Pip-Boy light as scripts see it (`GapsState::light_off_at`).
    fn pipboy_light(&mut self) -> bool {
        if self.gaps.light_off_at.is_some_and(|t| self.time > t) {
            self.gaps.pipboy_light = false;
            self.gaps.light_off_at = None;
        }
        self.gaps.pipboy_light
    }
}

fn is_actor_ref(g: &Game, r: FormId) -> bool {
    r == FormId::PLAYER_REF
        || crate::functions::base_of(g, r)
            .and_then(|b| g.data.sig(b))
            .is_some_and(|s| s == *b"NPC_" || s == *b"CREA")
}

fn inv_extra(g: &Game, holder: FormId, item: FormId, equipped_only: bool) -> (Option<f32>, u8) {
    g.world
        .peek(holder)
        .and_then(|s| s.inventory.as_ref())
        .and_then(|inv| {
            inv.groups()
                .find(|x| x.0 == item && (!equipped_only || x.1.equipped))
                .map(|x| (x.1.condition, x.1.mods))
        })
        .unwrap_or((None, 0))
}

fn world_extra(g: &Game, r: FormId) -> (Option<f32>, u8) {
    match g.world.peek(r) {
        Some(s) => (s.extra.condition, s.extra.mods),
        None => (g.world.placed.get(r).and_then(|p| p.health), 0),
    }
}

/// Resolve `(actor, weapon base, condition, mod flags)` for `GetCalculatedWeaponDamage`
/// and `GetCalculatedWeaponDPS` [documented: GECK wiki GetCalculatedWeaponDamage, an
/// inventory reference uses its holder's stats when in an actor's inventory and the
/// player's otherwise, adjusted by condition and attached weapon mods].
fn weapon_target(g: &mut Game, subject: Option<FormId>, args: &[XVal]) -> (FormId, FormId, Option<f32>, u8) {
    let subject_actor = subject.filter(|&r| is_actor_ref(g, r)).unwrap_or(FormId::PLAYER_REF);
    if let Some(arg) = form(args, 0) {
        if let Some((holder, item)) = g.temp_item(arg) {
            let (cond, mods) = inv_extra(g, holder, item, false);
            let actor = if is_actor_ref(g, holder) { holder } else { FormId::PLAYER_REF };
            return (actor, item, cond, mods);
        }
        if g.data.sig(arg) != Some(*b"WEAP") {
            if let Some(base) = crate::functions::base_of(g, arg).filter(|&b| g.data.sig(b) == Some(*b"WEAP")) {
                let (cond, mods) = world_extra(g, arg);
                return (subject_actor, base, cond, mods);
            }
        }
        return (subject_actor, arg, None, 0);
    }
    let Some(r) = subject else {
        return (FormId::PLAYER_REF, FormId(0), None, 0);
    };
    if let Some((holder, item)) = g.temp_item(r) {
        let (cond, mods) = inv_extra(g, holder, item, false);
        let actor = if is_actor_ref(g, holder) { holder } else { FormId::PLAYER_REF };
        return (actor, item, cond, mods);
    }
    if let Some(base) = crate::functions::base_of(g, r).filter(|&b| g.data.sig(b) == Some(*b"WEAP")) {
        let (cond, mods) = world_extra(g, r);
        return (FormId::PLAYER_REF, base, cond, mods);
    }
    match crate::functions::items::equipped_weapon(g, r) {
        Some(w) => {
            let (cond, mods) = inv_extra(g, r, w, true);
            (r, w, cond, mods)
        }
        None => (r, FormId(0), None, 0),
    }
}

/// Attached weapon mod adjustments from WEAP DNAM `effect_mod1..3` / `value_a_mod1..3`
/// (matching `fnv_combat::data::WeaponStats::modded`): additive base damage bonus
/// (`Increase Weapon Damage`, 1) and multiplicative attack-rate factor (`Increase Rate of
/// Fire`, 8).
fn weapon_mod_bonuses(g: &Game, w: FormId, mods: u8) -> (f64, f64) {
    let mut damage_bonus = 0.0;
    let mut rate_mult = 1.0;
    if mods == 0 {
        return (damage_bonus, rate_mult);
    }
    for slot in 1u8..=3 {
        if mods & (1 << (slot - 1)) == 0 {
            continue;
        }
        let val = crate::nvse::record_value(g, w, "DNAM", &format!("value_a_mod{slot}")).and_then(|v| v.num().ok()).unwrap_or(0.0);
        match crate::nvse::record_value(g, w, "DNAM", &format!("effect_mod{slot}")) {
            Some(XVal::Str(s)) if s == "Increase Weapon Damage" => damage_bonus += val,
            Some(XVal::Num(n)) if n as i64 == 1 => damage_bonus += val,
            Some(XVal::Str(s)) if s == "Increase Rate of Fire" => rate_mult *= (1.0 + val).max(0.0),
            Some(XVal::Num(n)) if n as i64 == 8 => rate_mult *= (1.0 + val).max(0.0),
            _ => {}
        }
    }
    (damage_bonus, rate_mult)
}

/// A weapon's damage by JohnnyGuitar's rules: the base damage (plus attached `Increase
/// Weapon Damage` mods, scaled by condition under `SetConditionDamagePenalty` and
/// Stewie's condition tweaks) times 0.5 + skill / 200 (the skill counted up to 100), and
/// the damage of a hit adding the actor's Melee Damage to a melee weapon's and Unarmed
/// Damage to an unarmed one's, whose hit takes no skill factor. Returns (the skill-scaled
/// base, the hit) [observed: gaps6 and gaps9, the pistol at Guns 17/67/117 (the last
/// capped), the knife's 12.05 = 15 × 0.67 + 2, brass knuckles 19.35 = 18 + 1.35 at
/// Unarmed 17, fists 1.35; documented: GECK wiki GetCalculatedWeaponDamage for condition
/// and attached weapon mods].
fn weapon_damage(g: &mut Game, actor: FormId, w: FormId, condition: Option<f32>, mods: u8) -> (f64, f64) {
    use crate::actor_values::{MELEE_DAMAGE, MELEE_WEAPONS, UNARMED, UNARMED_DAMAGE};
    let (mod_damage, _) = weapon_mod_bonuses(g, w, mods);
    let mut damage = crate::nvse::record_value(g, w, "DATA", "base_damage").and_then(|v| v.num().ok()).unwrap_or(0.0) + mod_damage;
    if let Some(c) = condition {
        if !g.tweaks.no_condition_damage_penalty {
            let threshold = if g.tweaks.no_condition_damage_buffer { 1.0 } else { f64::from(fnv_sim::inventory::CONDITION_DAMAGE_THRESHOLD) };
            let penalty = f64::from(g.condition_damage_penalty());
            let factor = (1.0 - (threshold - f64::from(c).clamp(0.0, 1.0)).max(0.0) * penalty).max(0.0);
            damage *= factor;
        }
    }
    let skill_code = g.data.weapon_skills.get(&w).map(|&s| s as u16);
    let skill = skill_code.and_then(|s| crate::functions::actor::actor_value(g, actor, s).ok()).unwrap_or(0.0);
    let scaled = damage * (0.5 + skill.clamp(0.0, 100.0) / 200.0);
    let hit = match skill_code {
        Some(MELEE_WEAPONS) => scaled + crate::functions::actor::actor_value(g, actor, MELEE_DAMAGE).unwrap_or(0.0),
        Some(UNARMED) => damage + crate::functions::actor::actor_value(g, actor, UNARMED_DAMAGE).unwrap_or(0.0),
        _ => scaled,
    };
    (scaled, hit)
}

/// Run a snippet's lines on a reference, as the console runs a line; an error is logged.
fn run_snippet(g: &mut Game, src: &str, on: FormId) {
    if let Err(e) = g.run_source(src, RunCtx { self_ref: Some(on), ..Default::default() }) {
        tracing::warn!("RunScriptSnippet: {e}");
    }
}

/// IPDS DATA's materials in GetImpactDataSetForm's order [observed: gaps10, the 40mm
/// grenade's set: 0–2 stone, dirt, grass, 4 metal, 8 water, 9 hollow metal; 10 and 11
/// none on it].
const IMPACT_MATERIALS: [&str; 12] = ["stone", "dirt", "grass", "glass", "metal", "wood", "organic", "cloth", "water", "hollow_metal", "organic_bug", "organic_glow"];

/// MessageExAlt's default faces, `#0`–`#6` [documented: MessageExAlt].
const FACES: [&str; 7] = ["sad", "neutral", "angry", "thinking", "very_happy", "surprised", "in_pain"];

/// A MessageExAlt text's leading `icon|` and `$sound|` parts and the text after them.
fn split_styled(t: &str) -> (String, Option<String>, Option<String>) {
    let (mut icon, mut sound) = (None, None);
    let mut rest = t;
    while let Some((head, tail)) = rest.split_once('|') {
        let h = head.trim();
        if let Some(s) = h.strip_prefix('$') {
            sound = Some(s.to_string());
        } else if let Some(k) = h.strip_prefix('#').and_then(|d| d.parse::<usize>().ok()) {
            icon = FACES.get(k).map(|f| format!("Interface\\Icons\\Message Icons\\glow_message_vaultboy_{f}.dds"));
        } else if h.to_ascii_lowercase().ends_with(".dds") {
            icon = Some(h.to_string());
        } else {
            break;
        }
        rest = tail;
    }
    (rest.to_string(), icon, sound)
}

/// WEAP sound IDs of GetWeaponSound / SetWeaponSound (`0..=13`): the subrecord and which
/// of its repeats [documented: GECK wiki GetWeaponSound, SetWeaponSound; xEdit
/// wbDefinitionsFNV.pas WEAP; observed: gaps4, 0–2 the 10mm pistol's two SNAM then XNAM,
/// 3 none on it].
fn weapon_sound_sub(id: i64) -> Option<(&'static str, usize)> {
    Some(match id {
        0 => ("SNAM", 0),
        1 => ("SNAM", 1),
        2 => ("XNAM", 0),
        3 => ("NAM7", 0),
        4 => ("TNAM", 0),
        5 => ("NAM6", 0),
        6 => ("UNAM", 0),
        7 => ("NAM9", 0),
        8 => ("NAM8", 0),
        9 => ("WMS1", 0),
        10 => ("WMS1", 1),
        11 => ("WMS2", 0),
        12 => ("YNAM", 0),
        13 => ("ZNAM", 0),
        _ => return None,
    })
}

impl Game<'_> {
    /// Whether `form` (or its `TempCloneForm` origin) is a `WEAP` base record.
    pub(crate) fn is_weapon_form(&self, form: FormId) -> bool {
        self.data.sig(self.unclone(form)) == Some(*b"WEAP")
    }

    /// Whether `form` (or its `TempCloneForm` origin) is a `SOUN` base record.
    pub(crate) fn is_sound_form(&self, form: FormId) -> bool {
        self.data.sig(self.unclone(form)) == Some(*b"SOUN")
    }

    /// Resolves `form` (a `WEAP` base form, `TempCloneForm` `WEAP`, placed weapon `REFR`, or
    /// inventory weapon ref) to the `WEAP` form that owns its sound slots.
    pub(crate) fn weapon_form(&self, form: FormId) -> Option<FormId> {
        let resolved = crate::functions::base_of(self, form).unwrap_or(form);
        self.is_weapon_form(resolved).then_some(resolved)
    }

    /// Effective `SOUN` assigned to weapon `weapon` at JIP sound slot `slot` (`0..=13`):
    /// session override (`SetWeaponSound`, `SetPickupSound`, `SetPutdownSound`, or
    /// `TempCloneForm` snapshot) first, else the authored `WEAP` record's subrecord.
    pub fn weapon_sound(&self, weapon: FormId, slot: u8) -> Option<FormId> {
        let (sub, k) = weapon_sound_sub(i64::from(slot))?;
        let w = self.weapon_form(weapon)?;
        if let Some(&ov) = self.gaps.weapon_sounds.get(&(w, slot)) {
            return ov;
        }
        let orig = self.unclone(w);
        parse_json_form(self.data.record_all(orig, sub).get(k).copied())
    }

    /// Session weapon-sound override directly stored for `weapon` and `slot` (`0..=13`),
    /// including overrides snapshotted at `TempCloneForm` creation time:
    /// - `None`: no session override on this weapon/slot.
    /// - `Some(None)`: cleared this session.
    /// - `Some(Some(sound))`: overridden to `sound` this session.
    pub fn weapon_sound_override(&self, weapon: FormId, slot: u8) -> Option<Option<FormId>> {
        self.gaps.weapon_sounds.get(&(weapon, slot)).copied()
    }

    /// Override or clear (`None`) JIP sound slot `slot` (`0..=13`) on `weapon` for the
    /// session. Returns `true` when `weapon` resolves to a `WEAP`, `slot < 14`, and `sound`
    /// is either `None` or a `SOUN` (or `TempCloneForm` `SOUN`); returns `false` without
    /// mutating state otherwise.
    pub fn set_weapon_sound(&mut self, weapon: FormId, slot: u8, sound: Option<FormId>) -> bool {
        if weapon_sound_sub(i64::from(slot)).is_none() {
            return false;
        }
        let Some(w) = self.weapon_form(weapon) else { return false };
        if let Some(s) = sound {
            if !self.is_sound_form(s) {
                return false;
            }
        }
        self.gaps.weapon_sounds.insert((w, slot), sound);
        true
    }

    /// Snapshot any active session weapon-sound overrides (`0..=13`) from `source` onto
    /// `clone` at `TempCloneForm` creation time. Pre-clone edits on `source` carry onto
    /// `clone`, while later edits on `source` stay isolated from already-created clones.
    pub(crate) fn snapshot_weapon_sounds(&mut self, source: FormId, clone: FormId) {
        if !self.is_weapon_form(source) {
            return;
        }
        for slot in 0..WEAPON_SOUND_SLOTS as u8 {
            if let Some(&ov) = self.gaps.weapon_sounds.get(&(source, slot)) {
                self.gaps.weapon_sounds.insert((clone, slot), ov);
            }
        }
    }

    /// The weapons whose sound slots scripts changed this session (`SetWeaponSound`,
    /// `SetPickupSound`, `SetPutdownSound`, plus `TempCloneForm` snapshots), for an engine
    /// consumer to apply over authored `WeaponFx` / item sounds
    /// (docs/spec-notes/weapon-mods.md §9).
    pub fn weapon_sound_changes(&self) -> BTreeMap<FormId, WeaponSoundChange> {
        let mut out: BTreeMap<FormId, WeaponSoundChange> = BTreeMap::new();
        for (&(weapon, slot), &sound) in &self.gaps.weapon_sounds {
            let idx = usize::from(slot);
            if idx < WEAPON_SOUND_SLOTS {
                out.entry(weapon).or_default().slots[idx] = Some(sound);
            }
        }
        out
    }

    /// A number that changes whenever [`Game::weapon_sound_changes`] would give something
    /// else, read without building the map: an engine consumer can poll it every frame.
    pub fn weapon_sound_stamp(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut n = 0u64;
        let mut sum = 0u64;
        for (&(weapon, slot), &sound) in &self.gaps.weapon_sounds {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (weapon.0, slot, sound.map(|f| f.0)).hash(&mut h);
            // The map has no order: a sum of the entries' hashes does not depend on one.
            sum = sum.wrapping_add(h.finish());
            n += 1;
        }
        sum ^ n.rotate_left(32)
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    let zero = XVal::Num(0.0);
    let v = match name {
        // Ordinary source calls use the current physical caster, never a stale
        // CPTH room (vats-script-clearance.md). Generic receiver policy is unchanged.
        name if crate::vats::handles_area_query(name) =>
            XVal::Num(crate::vats::script_area(g, subject, name == "getvatsleftareafree")),
        // ---- magic items and effects ----
        // The item's effects in record order [observed: gaps2, the stimpak 4, the first
        // RestoreHealth, past the end 0].
        // Less RemoveNthEffect's (`crate::nvse_jip11`).
        // AddNewEffect's entries too (`crate::nvse_jip14`).
        // One index space for every Nth-effect function (`Game::effect_slot`, #2016).
        "getnumeffects" => XVal::Num(form(args, 0).map_or(0, |f| g.effect_count(f)) as f64),
        "getntheffectbase" => {
            use crate::nvse_jip14::EffectSlot;
            let f = form(args, 0).unwrap_or(FormId(0));
            let slot = usize::try_from(num(args, 1, -1.0) as i64).ok().and_then(|i| g.effect_slot(f, i));
            XVal::Ref(match slot {
                Some(EffectSlot::Added { back, at }) => g.added_effect(f, back, at).map_or(0, |e| e.effect.0),
                Some(EffectSlot::Own(r)) => {
                    let e = g.data.effect_items.get(&g.unclone(f)).and_then(|l| l.get(r));
                    e.map_or(0, |e| g.gaps.effect_bases.get(&(f, r)).copied().unwrap_or(e.effect).0)
                }
                None => 0,
            })
        }
        // Returns 0 (not the old base), for the session [observed: gaps10, the stimpak's
        // first effect set to Silver Sting's reads 00033595, and back].
        "setntheffectbase" => {
            use crate::nvse_jip14::EffectSlot;
            let (Some(f), Some(b)) = (form(args, 0), form(args, 2)) else { return Some(Ok(zero)) };
            if g.data.sig(g.unclone(b)) != Some(*b"MGEF") {
                return Some(Ok(XVal::Ref(0)));
            }
            match usize::try_from(num(args, 1, -1.0) as i64).ok().and_then(|i| g.effect_slot(f, i)) {
                Some(EffectSlot::Own(r)) => {
                    g.gaps.effect_bases.insert((f, r), b);
                }
                Some(EffectSlot::Added { back, at }) => {
                    if let Some(e) = g.added_effect_mut(f, back, at) {
                        e.effect = b;
                    }
                }
                None => {}
            }
            XVal::Ref(0)
        }
        // ENIT flags, bit by bit [observed: gaps3, the stimpak's flags 5 read 1 0 1].
        // An actor effect's SPIT, an object effect's or ingestible's ENIT; SetEffectFlag's
        // change first (`crate::nvse_jip8`).
        "geteffectflag" => {
            let flags = form(args, 0).map_or(0, |f| g.effect_flags(f));
            bit(f64::from(flags), num(args, 1, -1.0))
        }
        // MGEF DATA [observed: gaps3, RestoreHealth archetype 0, actor value 16, flags
        // 112 so bits 0–3 read 0].
        "getbaseeffectarchetype" => XVal::Num(form(args, 0).and_then(|f| g.magic_effect_def(f)).map_or(0, |m| m.archetype) as f64),
        "getbaseeffectav" => XVal::Num(form(args, 0).and_then(|f| g.magic_effect_def(f)).and_then(|m| m.actor_value).map_or(-1.0, f64::from)),
        "getbaseeffectflag" => {
            // SetBaseEffectFlag's change first (`crate::nvse_jip8`).
            let flags = form(args, 0).map_or(0, |f| g.base_effect_flags(f));
            bit(f64::from(flags), num(args, 1, -1.0))
        }
        // ---- ammunition ----
        // AMMO RCIL in record order [observed: gaps2, 10mm JHP hand load 2, the first
        // 00160C45; plain 10mm 0].
        // AddAmmoEffect's and RemoveAmmoEffect's changes first (`crate::nvse_jip7`).
        "getnumammoeffects" => XVal::Num(form(args, 0).map_or(0, |f| g.ammo_effects(f).len()) as f64),
        "getnthammoeffect" => {
            let i = num(args, 1, 0.0);
            let all = form(args, 0).map(|f| g.ammo_effects(f)).unwrap_or_default();
            XVal::Ref(if i >= 0.0 { all.get(i as usize).map_or(0, |f| f.0) } else { 0 })
        }
        // AMEF DATA: 0 the type, 1 the operation, 2 the value [observed: gaps2, the DT
        // reduction (DT Mod, Subtract, 2) reads 2 2 2, trait 3 reads 0].
        "getammoeffecttraitnumeric" => {
            let Some(eff) = form(args, 0).and_then(|f| g.ammo_effect_traits(f)) else {
                return Some(Ok(zero));
            };
            match num(args, 1, -1.0) as i64 {
                0 => XVal::Num(f64::from(eff.effect_type)),
                1 => XVal::Num(f64::from(eff.operation)),
                2 => XVal::Num(crate::nvse::record_float(f64::from(eff.value))),
                _ => zero,
            }
        }
        // ---- globals ----
        // LU's take the global by name too [observed: plugins3, `GetGlobalValue
        // TimeScale`].
        "getglobalvariable" | "getglobalvalue" => {
            let f = form(args, 0).or_else(|| g.data.by_edid(&text(args, 0)));
            XVal::Num(f.and_then(|f| g.world.global(f).ok()).map_or(0.0, f64::from))
        }
        // LU's: returns 0, not the old value the page gives [observed: plugins3].
        "setglobalvalue" => {
            if let Some(f) = form(args, 0).or_else(|| g.data.by_edid(&text(args, 0))) {
                let _ = g.world.set_global(f, num(args, 1, 0.0) as f32);
            }
            zero
        }
        // `set|remove function flag [perk]` [documented: SetJohnnyOnAddPerkEventHandler;
        // observed: plugins3].
        "setjohnnyonaddperkeventhandler" | "setjohnnyonremoveperkeventhandler" => {
            let add = name == "setjohnnyonaddperkeventhandler";
            let Some(f) = args.get(1).cloned() else { return Some(Ok(zero)) };
            let perk = form(args, 3).filter(|&id| id != FormId(0x0000_003B));
            g.gaps.perk_handlers.retain(|(a, h, p)| !(*a == add && *h == f && *p == perk));
            if num(args, 0, 1.0) != 0.0 {
                g.gaps.perk_handlers.push((add, f, perk));
            }
            zero
        }
        // Returns 0 [observed: gaps3, TimeScale set to 12 reads back 12].
        "setglobalvariable" => {
            if let Some(f) = form(args, 0) {
                let _ = g.world.set_global(f, num(args, 1, 0.0) as f32);
            }
            zero
        }
        // ---- perks ----
        // PERK DATA: 0 trait, 1 playable, 2 hidden [documented: GECK wiki GetPerkFlag /
        // SetPerkFlag; observed: gaps2, Toughness reads 0 1 0 0 0; SetPerkFlag returns 0,
        // gaps7]. Stored on `Game::perk_def` (`lu3.perk_edits`) so level-up, trait sheet,
        // ITR eligibility, GetAllPerks, and revision checks read the change.
        "getperkflag" | "setperkflag" => {
            let f = form(args, 0).unwrap_or(FormId(0));
            let k = num(args, 1, -1.0) as i64;
            if name == "setperkflag" {
                if matches!(k, 0..=2) {
                    if let Some(p) = g.perk_def_mut(f) {
                        let on = num(args, 2, 0.0) != 0.0;
                        match k {
                            0 => p.is_trait = on,
                            1 => p.playable = on,
                            2 => p.hidden = on,
                            _ => {}
                        }
                    }
                }
                return Some(Ok(zero));
            }
            let Some(p) = g.perk_def(f) else { return Some(Ok(zero)) };
            let now = match k {
                0 => p.is_trait,
                1 => p.playable,
                2 => p.hidden,
                _ => return Some(Ok(zero)),
            };
            yes(now)
        }
        // [observed: gaps3, Toughness 2 entries]
        "getperkentrycount" => XVal::Num(form(args, 0).and_then(|f| g.perk_def(f)).map_or(0, |p| p.entries.len()) as f64),
        // Entry point 1, ability 5, quest stage 6; past the entries 0 [observed: gaps3 and
        // gaps5, Toughness/Lead Belly/Educated/Commando 1, Rad Resistance and Small Frame 5,
        // Intense Training 6].
        "getnthperkentrytype" => {
            let e = form(args, 0).and_then(|f| crate::nvse_lu3::nth_entry_of(g, f, num(args, 1, -1.0)));
            XVal::Num(match e.map(|e| &e.effect) {
                Some(crate::data::PerkEffect::EntryPoint(_)) => 1.0,
                Some(crate::data::PerkEffect::Ability(_)) => 5.0,
                Some(crate::data::PerkEffect::QuestStage { .. }) => 6.0,
                _ => 0.0,
            })
        }
        // The entry's string: an activate choice's button label (EPF2) [documented:
        // GetNthPerkEntryString, "Activate Prompt" for type 4]; "" for an entry without
        // one [observed: jip-forms perkentry.0, ""].
        "getnthperkentrystring" => {
            let e = form(args, 0).and_then(|f| crate::nvse_lu3::nth_entry_of(g, f, num(args, 1, -1.0)));
            XVal::Str(match e.map(|e| &e.effect) {
                Some(crate::data::PerkEffect::EntryPoint(d)) => d.button.clone().unwrap_or_default(),
                _ => String::new(),
            })
        }
        // The entry point's code [observed: gaps4, Toughness's first 56].
        "getnthperkentryfunction" => {
            let e = form(args, 0).and_then(|f| crate::nvse_lu3::nth_entry_of(g, f, num(args, 1, -1.0)));
            XVal::Num(match e.map(|e| &e.effect) {
                Some(crate::data::PerkEffect::EntryPoint(d)) => f64::from(d.point),
                _ => -1.0,
            })
        }
        // `av [type]`: the modifiers, not the damage [observed: gaps5, Health reads 0 on
        // every filter before and after DamageAV 30]; 0 the permanent one, 1 the
        // temporary one, none both [hypothesis: the filters' meanings].
        "getactorvaluemodifier" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let code = match args.first() {
                Some(XVal::Num(c)) => *c as u16,
                Some(XVal::Str(n)) => match crate::functions::av_code(g, n) {
                    Ok(c) => c,
                    Err(_) => return Some(Ok(zero)),
                },
                _ => return Some(Ok(zero)),
            };
            let parts = crate::functions::actor::actor_value_parts(g, a, code).map(|p| p[1]).unwrap_or(0.0);
            let temp = crate::functions::actor::temporary(g, a, code);
            XVal::Num(match args.get(1) {
                Some(XVal::Num(t)) if *t == 0.0 => parts,
                Some(XVal::Num(t)) if *t == 1.0 => temp,
                Some(XVal::Num(_)) => 0.0,
                _ => parts + temp,
            })
        }
        // ---- base forms ----
        // CREA DATA type; not a creature -1 [observed: gaps2, the street brahmin 1
        // (Mutated Animal), Doc -1].
        "getcreaturetype" => {
            let b = form(args, 0).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            if g.data.sig(b) == Some(*b"CREA") { XVal::Num(json_num(g.data.record_field(b, "DATA", "type"))) } else { XVal::Num(-1.0) }
        }
        // [observed: gaps4, the street brahmin's PNAM 00015833]
        "getbodypartdata" => {
            let b = form(args, 0).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            // SetBodyPartData's change first (`crate::nvse_jip8`).
            match crate::nvse::record_value(g, b, "PNAM", "") {
                Some(XVal::Ref(r)) if g.data.sig(b) == Some(*b"CREA") => XVal::Ref(r),
                _ => XVal::Ref(0),
            }
        }
        // YNAM [observed: gaps2, the 10mm pistol 000864D5, the stimpak none].
        // WEAP shares live slot 12 with GetWeaponSound / SetWeaponSound.
        "getpickupsound" => match form(args, 0) {
            Some(f) if g.is_weapon_form(f) => XVal::Ref(g.weapon_sound(f, 12).map_or(0, |s| s.0)),
            Some(f) => match crate::nvse::record_value(g, f, "YNAM", "") {
                Some(XVal::Ref(r)) => XVal::Ref(r),
                _ => XVal::Ref(0),
            },
            None => XVal::Ref(0),
        },
        // DEST stages [documented: GetDestructionDataNumStages; observed: gaps3, the 10mm
        // pistol 0]. A reference reads its base's, as `nvse_jip6`.
        "getdestructiondatanumstages" => {
            XVal::Num(form(args, 0).map(|f| base(g, f)).and_then(|b| g.destruction(b)).map_or(0, |d| d.stages.len()) as f64)
        }
        // ---- weapons ----
        // ShowOff's index into DNAM flags1 [observed: gaps4, the 10mm pistol reads 0 on
        // 0–5 and SetWeaponFlag 5 1 reads back 1; documented: WeaponHasFlag /
        // SetWeaponFlag aliases GetWeaponHasFlag / SetWeaponHasFlag, calling reference
        // weaponRefr or optional weapon baseForm].
        "weaponhasflag" | "getweaponhasflag" | "setweaponflag" | "setweaponhasflag" => {
            let set = matches!(name, "setweaponflag" | "setweaponhasflag");
            let resolve_weapon = |g: &Game, f: FormId| -> Option<FormId> {
                if let Some((_, it)) = g.temp_item(f) {
                    return (g.data.sig(g.unclone(it)) == Some(*b"WEAP")).then_some(it);
                }
                if g.data.sig(g.unclone(f)) == Some(*b"WEAP") {
                    return Some(f);
                }
                crate::functions::base_of(g, f).filter(|&b| g.data.sig(g.unclone(b)) == Some(*b"WEAP"))
            };
            let w = match form(args, if set { 2 } else { 1 }) {
                Some(f) => resolve_weapon(g, f),
                None => subject.and_then(|r| resolve_weapon(g, r).or_else(|| crate::functions::items::equipped_weapon(g, r))),
            };
            let Some(w) = w else { return Some(Ok(zero)) };
            let flags = match crate::nvse::record_value(g, w, "DNAM", "flags1") {
                Some(XVal::Num(x)) => x as u64,
                _ => 0,
            };
            let k = num(args, 0, -1.0);
            if !(0.0..32.0).contains(&k) {
                return Some(Ok(zero));
            }
            if !set {
                return Some(Ok(yes(flags >> (k as u64) & 1 != 0)));
            }
            let new = if num(args, 1, 0.0) != 0.0 { flags | 1 << (k as u64) } else { flags & !(1 << (k as u64)) };
            g.field_overrides.insert((w, "DNAM/flags1".into()), XVal::Num(new as f64));
            g.playable_overrides.remove(&w);
            zero
        }
        "getweaponsound" | "setweaponsound" => {
            let Some(w) = form(args, 0) else {
                return Some(Ok(if name == "setweaponsound" { zero } else { XVal::Ref(0) }));
            };
            let Some(slot) = parse_weapon_sound_slot(args, 1) else {
                return Some(Ok(if name == "setweaponsound" { zero } else { XVal::Ref(0) }));
            };
            if name == "setweaponsound" {
                if let Some(sound) = parse_weapon_sound_arg(args, 2) {
                    g.set_weapon_sound(w, slot, sound);
                }
                return Some(Ok(zero));
            }
            XVal::Ref(g.weapon_sound(w, slot).map_or(0, |s| s.0))
        }
        // JohnnyGuitar's damage and DPS (`weapon_damage`, `weapon_target`).
        "getcalculatedweapondamage" => {
            let (a, w, cond, mods) = weapon_target(g, subject, args);
            XVal::Num(weapon_damage(g, a, w, cond, mods).1)
        }
        // The record's attack rate (scaled by attached `Increase Rate of Fire` mods) ×
        // (the skill-scaled base, without the Melee or Unarmed Damage bonus, + Critical
        // Chance % × CRDT damage × CRDT multiplier × `Tweaks::crit_damage_scale`), for
        // every kind of weapon [observed: gaps7 and gaps9, the pistol, combat knife,
        // machete, sledgehammer and brass knuckles; the critical part doubles with
        // Critical Chance and does not change with the skill; fists 0].
        "getcalculatedweapondps" => {
            let (a, w, cond, mods) = weapon_target(g, subject, args);
            let (_, rate_mult) = weapon_mod_bonuses(g, w, mods);
            let shots = json_num(g.data.record_field(w, "DNAM", "attack_shots_per_sec")) * rate_mult;
            let chance = crate::functions::actor::actor_value(g, a, crate::actor_values::CRITICAL_CHANCE).unwrap_or(0.0).max(0.0) / 100.0;
            let crit = crate::nvse::record_value(g, w, "CRDT", "critical_damage").and_then(|v| v.num().ok()).unwrap_or(0.0)
                * crate::nvse::record_value(g, w, "CRDT", "crit_percent_mult").and_then(|v| v.num().ok()).unwrap_or(0.0)
                * g.tweaks.crit_damage_scale;
            XVal::Num((weapon_damage(g, a, w, cond, mods).0 + chance * crit) * shots)
        }
        // DESC, which GetFormDescription reads (the VM's); the VM formats it; returns 1
        // and holds for the session [observed: gaps6, "NVF desc %g" 3 reads "NVF desc 3"].
        "setformdescription" => {
            let Some(f) = form(args, 0) else { return Some(Ok(zero)) };
            g.field_overrides.insert((f, "DESC/".into()), XVal::Str(text(args, 1)));
            XVal::Num(1.0)
        }
        // The value GetValue reads (DATA value); returns 0 [observed: gaps6, 999 reads
        // back].
        "setvaluealt" => {
            if let Some(f) = form(args, 0) {
                g.field_overrides.insert((f, "DATA/value".into()), XVal::Num(num(args, 1, 0.0)));
            }
            zero
        }
        // EITM; SetObjectEffect takes only an enchantment of the item's kind: Apparel (3) on
        // ARMO, Weapon (2) on WEAP or EXPL [documented: GECK wiki SetObjectEffect; observed:
        // gaps6, an apparel enchantment on the pistol is refused].
        "setobjecteffect" => {
            if let Some(f) = form(args, 0) {
                let effect = match args.get(1) {
                    None | Some(XVal::Ref(0)) => Some(None),
                    Some(XVal::Num(x)) if *x == 0.0 => Some(None),
                    Some(_) => form(args, 1).map(Some),
                };
                if let Some(effect) = effect {
                    g.set_object_effect(f, effect);
                }
            }
            zero
        }
        // PROJ DATA flags bit by bit, and the explosion [observed: gaps6, the 10mm
        // projectile's flags 141 read 1 0 1 1 0 0; a null explosion does not clear it].
        "getprojectileflag" | "setprojectileflag" => {
            let Some(f) = form(args, 0).filter(|&f| g.data.sig(g.unclone(f)) == Some(*b"PROJ")) else {
                return Some(Ok(zero));
            };
            let flags = match crate::nvse::record_value(g, f, "DATA", "flags") {
                Some(XVal::Num(x)) => x as u64,
                _ => 0,
            };
            let k = num(args, 1, -1.0);
            if !(0.0..32.0).contains(&k) {
                return Some(Ok(zero));
            }
            if name == "getprojectileflag" {
                return Some(Ok(yes(flags >> (k as u64) & 1 != 0)));
            }
            let new = if num(args, 2, 0.0) != 0.0 { flags | 1 << (k as u64) } else { flags & !(1 << (k as u64)) };
            g.field_overrides.insert((f, "DATA/flags".into()), XVal::Num(new as f64));
            zero
        }
        "getprojectileexplosion" => XVal::Ref(form(args, 0).and_then(|f| g.projectile_traits(f)).and_then(|t| t.explosion).map_or(0, |e| e.0)),
        "setprojectileexplosion" => {
            if let (Some(f), Some(e)) = (form(args, 0), form(args, 1)) {
                if g.data.sig(g.unclone(f)) == Some(*b"PROJ") && g.data.sig(g.unclone(e)) == Some(*b"EXPL") {
                    g.field_overrides.insert((f, "DATA/explosion".into()), XVal::Ref(e.0));
                }
            }
            zero
        }
        // 0 for the player and the loaded actors, 3 for the rest [observed: gaps6, the
        // player 0, Doc out of the loaded cells 3].
        "getactorprocessinglevel" => {
            let a = subject.unwrap_or(FormId(0));
            XVal::Num(if a == FormId::PLAYER_REF || g.package_actors().contains(&a) { 0.0 } else { 3.0 })
        }
        // `persistent:0/1`: sets or clears the persistent record flag (0x400) on the
        // calling reference (including `FormId::PLAYER_REF`) for the session
        // [documented: GECK wiki SetPersistent; observed: gaps6 returns 0; Fallout: New
        // Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse`
        // (`0x00103DF9`), 10 official base masters, `w4d_06_player_setpersistent_0`
        // (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679):
        // `PROBE|w4d_06_player_setpersistent_0|p0=1,after0=0,after1=1`; no DLL or source
        // consulted; hypothesis: session-only lifetime across save/load].
        "setpersistent" => {
            if let Some(r) = subject {
                let on = num(args, 0, 0.0) != 0.0;
                let cur = crate::nvse_other::record_flags_of(g, r)
                    | if g.persistent(r) { fnv_sim::refs::FLAG_PERSISTENT } else { 0 };
                let next = if on {
                    cur | fnv_sim::refs::FLAG_PERSISTENT
                } else {
                    cur & !fnv_sim::refs::FLAG_PERSISTENT
                };
                g.other.flags.insert(r, next);
                g.other.persistent_overrides.insert(r, on);
            }
            zero
        }
        // `limb`: the limb's condition to 0 [observed: gaps6, 2 (the left arm) leaves Left
        // Attack Condition 0]; 0 head, 1 torso, 3 right arm, 4–5 legs [hypothesis: the
        // order of the condition values].
        "cripplelimb" => {
            let a = subject.unwrap_or(FormId(0));
            // Perception, Endurance, Left/Right Attack, Left/Right Mobility Condition
            // (25–30).
            let code = match num(args, 0, -1.0) as i64 {
                k @ 0..=5 => 25 + k as u16,
                _ => return Some(Ok(zero)),
            };
            let now = crate::functions::actor::actor_value(g, a, code).unwrap_or(0.0);
            crate::functions::actor::add_damage(g, a, code, now.max(0.0));
            zero
        }
        // ---- containers and inventories ----
        // `item count base` on the base's carried items [documented: BaseRemoveItem].
        "baseremoveitem" | "baseadditem" => {
            let (Some(item), Some(b)) = (form(args, 0), form(args, 2)) else { return Some(Ok(zero)) };
            let n = num(args, 1, 1.0).max(1.0) as i32;
            let add = name == "baseadditem";
            g.world.edit_base_entries(b, |e| {
                if add {
                    e.push(fnv_sim::forms::Entry::new(item, n));
                    return;
                }
                let mut left = n;
                for x in e.iter_mut().filter(|x| x.item == item) {
                    let take = left.min(x.count);
                    x.count -= take;
                    left -= take;
                }
                e.retain(|x| x.count > 0);
            });
            zero
        }
        // The weight of the carried items, with 1 only the playable ones [observed:
        // gaps2, the player 93 either way].
        "getinventoryweight" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            XVal::Num(g.world.carried_weight(a).unwrap_or(0.0))
        }
        // The base items carried, one each, with a type code only that type [observed:
        // gaps2, 27; 40 (weapons) 8].
        "getallitems" => {
            let a = subject.unwrap_or(FormId(0));
            let want = num(args, 0, 0.0) as i32;
            let items: Vec<FormId> = g.world.inventory_mut(a).map(|inv| inv.iter().filter(|&(_, n)| n > 0).map(|(i, _)| i).collect()).unwrap_or_default();
            let items: Vec<XVal> = items
                .into_iter()
                .filter(|&i| want == 0 || g.data.sig(i).and_then(|s| crate::functions::items::form_type_code(&s)) == Some(want))
                .map(|i| XVal::Ref(i.0))
                .collect();
            list(g, items)
        }
        // The inventory reference of what is in the slot [observed: gaps2, an empty slot
        // reads 0].
        "getequippeditemref" => {
            let a = subject.unwrap_or(FormId(0));
            match crate::nvse::call(g, ctx, "getequippedobject", subject, args) {
                Some(Ok(XVal::Ref(i))) if i != 0 => XVal::Ref(g.temp_ref(a, FormId(i)).0),
                _ => XVal::Ref(0),
            }
        }
        // `item [count] [clearOwnership]`: as Drop [documented: DropAlt; observed: gaps3,
        // nine stimpaks leave eight].
        "dropalt" => {
            let (Some(a), Some(item)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let n = g.world.item_count(a, item).unwrap_or(0).min(num(args, 1, 1.0).max(1.0) as i32);
            if n > 0 {
                if let Ok(made) = g.world.drop_item(a, item, n) {
                    g.emit(crate::trace::EventKind::ItemRemoved { from: a, item, count: n, silent: false });
                    for r in made {
                        crate::functions::objects::note_created(g, r);
                    }
                }
            }
            zero
        }
        // ---- references and actors ----
        // On the full name of the base [observed: gaps2, GetName reads it on the
        // reference and the base].
        "setfullnamealt" => {
            let b = form(args, 1).or(subject).map(|f| base(g, f));
            if let Some(b) = b {
                g.name_overrides.insert(b, text(args, 0));
            }
            zero
        }
        "setrefname" => {
            if let Some(r) = subject {
                g.gaps.ref_names.insert(r, text(args, 0));
            }
            zero
        }
        // Disables/enables combat for the calling actor: combat-disabled actors never
        // enter combat (though others may still attack them, and UseWeapon / Use Weapon
        // packages still work); reset on every save load [documented: GECK wiki
        // SetCombatDisabled]. Returns 0 either way [observed: gaps2]; GetCombatDisabled
        // reads it [observed: gaps3]. Enabling SetCombatDisabled 1 on an actor already
        // in combat leaves IsInCombat == 1 in the same frame and ends active combat on
        // subsequent AI ticks (`Game::settle_awareness` in `awareness.rs`) [observed:
        // Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
        // `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated
        // clean-room staging copy (read-only `PrintToConsole` probe; zero writes to game
        // install or Saves), `core_20_set_combat_disabled_stops_combat`
        // (`wave4a_extenders.json`, #3679): `1,1,1` and
        // `w4h_05_set_combat_disabled_multitick` (`wave4h_reconcile.json`, PR #3803 /
        // #3802): `PROBE|w4h_05_set_combat_disabled_multitick|pre=1|t0(cd=1,inCmb=1)|t2(cd=1,inCmb=0)|t5(cd=1,inCmb=0)`;
        // zero DLL, PDB, or third-party source consulted].
        "setcombatdisabled" => {
            if let Some(r) = subject {
                if num(args, 0, 0.0) != 0.0 {
                    g.gaps.combat_disabled.insert(r);
                } else {
                    g.gaps.combat_disabled.remove(&r);
                }
            }
            zero
        }
        "getcombatdisabled" => yes(subject.is_some_and(|r| g.combat_disabled(r))),
        // The reference's (or the base's) script has a block of that ID [observed: gaps2,
        // Doc's GameMode (0) reads 1; hypothesis: the IDs are the compiled block codes].
        "hasscriptblock" => {
            let s = form(args, 1).or(subject).and_then(|f| g.ref_script(f).or_else(|| g.base_script(base(g, f))));
            let ev = num(args, 0, -1.0);
            yes(ev >= 0.0 && s.and_then(|s| g.data.script(s)).is_some_and(|u| u.has_block(ev as u16)))
        }
        // In other cells the distance is not a number to compare [observed: gaps2,
        // Floor of it reads -2147483648]: the largest float [hypothesis].
        "getdistance2d" => {
            let (Some(a), Some(b)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let same = match (g.cell_of(a), g.cell_of(b)) {
                (Some(x), Some(y)) => x == y || (g.world.placed.world_of(x).is_some() && g.world.placed.world_of(x) == g.world.placed.world_of(y)),
                _ => false,
            };
            match (same, g.ref_pos_rot(a), g.ref_pos_rot(b)) {
                (true, Some((p, _)), Some((q, _))) => XVal::Num(f64::from((p[0] - q[0]).hypot(p[1] - q[1]))),
                _ => XVal::Num(f64::from(f32::MAX)),
            }
        }
        // The player's teammates; none is no array [observed: gaps2, ar_Null].
        "getteammates" => {
            let t: Vec<XVal> = g.extra.iter().filter(|(_, e)| e.teammate).map(|(r, _)| XVal::Ref(r.0)).collect();
            if t.is_empty() { zero } else { list(g, t) }
        }
        // 0 the actors in the loaded cells, 3 the others; 1 and 2 none here, no array
        // [observed: gaps3, 16, ar_Null, ar_Null, over 1000; hypothesis: NVForever runs
        // every loaded actor fully]. Not disabled ones, which have no process [observed:
        // ai-observer-jip.md, GetActorProcessingLevel -1 when disabled; L5.ext rerun,
        // #2040: 16 in the start cell 0xDEB16, where the loaded actors are 20 with 4
        // disabled Golden Geckos; hypothesis: the same at level 3].
        "getactorsbyprocessinglevel" => {
            let level = num(args, 0, -1.0) as i64;
            let on = |r: &FormId| g.world.is_enabled(*r).unwrap_or(false);
            let actors: Vec<XVal> = match level {
                0 => loaded_processing_actors(g).into_iter().filter(|r| on(r)).map(|r| XVal::Ref(r.0)).collect(),
                3 => {
                    let high = loaded_processing_actors(g);
                    g.world.placed.refs.iter().filter(|(r, p)| matches!(&p.sig, b"ACHR" | b"ACRE") && !high.contains(r) && on(r)).map(|(r, _)| XVal::Ref(r.0)).collect()
                }
                _ => Vec::new(),
            };
            if actors.is_empty() { zero } else { list(g, actors) }
        }
        // The references of a base: in the loaded cells, persistent ones anywhere
        // [observed: gaps4, no loaded stimpak 0, Doc's base with exteriors 1; hypothesis:
        // the persistent ones count].
        "getformrefs" => {
            let Some(b) = form(args, 0) else { return Some(Ok(zero)) };
            let loaded: Vec<FormId> = g.loaded_cells.iter().copied().collect();
            let mut refs: Vec<FormId> = loaded.iter().flat_map(|&c| g.refs_in_cell(c, false)).filter(|&r| base(g, r) == b).collect();
            refs.extend(g.world.placed.refs.iter().filter(|&(&r, p)| p.base == b && g.persistent(r)).map(|(&r, _)| r));
            refs.extend(g.other.persistent_overrides.keys().copied().filter(|&r| {
                g.world.placed.get(r).is_none()
                    && g.world.peek(r).is_some_and(|s| s.base == b && !s.deleted())
                    && g.persistent(r)
            }));
            refs.sort();
            refs.dedup();
            let refs = refs.into_iter().map(|r| XVal::Ref(r.0)).collect();
            list(g, refs)
        }
        "isincombatwith" => yes(subject.zip(form(args, 0)).is_some_and(|(a, t)| g.combat_target_of(a) == Some(t))),
        "setactorgravitymult" => {
            if let Some(r) = subject {
                g.gaps.gravity.insert(r, num(args, 0, 1.0));
            }
            zero
        }
        // OBND size on the axis times the reference's scale [observed: gaps7, Doc 46 ×
        // 34 × 132, the Nuka-Cola machine 90 × 70 × 184].
        "getobjectdimensions" => {
            let f = form(args, 1).or(subject).unwrap_or(FormId(0));
            let b = base(g, f);
            let scale = if f != b { g.world.peek(f).and_then(|s| s.scale).or_else(|| g.world.placed.get(f).and_then(|p| p.scale)).unwrap_or(1.0) } else { 1.0 };
            let k = match args.first() {
                Some(XVal::Str(s)) => match s.to_ascii_uppercase().as_str() { "X" => 0, "Y" => 1, _ => 2 },
                Some(XVal::Num(n)) => match *n as i64 { 88 => 0, 89 => 1, _ => 2 },
                _ => 2,
            };
            XVal::Num(g.data.object_bounds.get(&b).map_or(0.0, |o| f64::from(o[k + 3] - o[k]) * f64::from(scale)))
        }
        // None to list gives no array [observed: gaps7, ar_Null for the player's perks,
        // followers, temporary effects and detecting actors, Doc's dropped weapons].
        "getplayerperks" => {
            let mut p: Vec<FormId> = g.player.perks.keys().copied().collect();
            p.sort();
            if p.is_empty() { zero } else { let p = p.into_iter().map(|f| XVal::Ref(f.0)).collect(); list(g, p) }
        }
        // [documented: GetDroppedRefs] Associated world weapons, not carried inventory
        // handles. Keep the ordinary world's enabled/deleted filtering and ordering.
        "getdroppedrefs" => {
            let holder = subject.filter(|&r| crate::functions::base_of(g, r).is_some_and(|b| g.template_source(b, r, |_| false).is_some()));
            let refs = holder.map(|holder| {
                g.world.dropped_beside(holder).into_iter().filter(|&r| {
                    g.world.peek(r).is_some_and(|s| g.data.sig(g.unclone(s.base)) == Some(*b"WEAP"))
                }).map(|r| XVal::Ref(r.0)).collect::<Vec<_>>()
            }).unwrap_or_default();
            if refs.is_empty() { zero } else { list(g, refs) }
        }
        // Actors currently following the calling reference (`Follow` / `Accompany` package)
        // [documented: GECK wiki GetFollowers; observed: gaps7, ar_Null when empty].
        "getfollowers" => {
            let followers = subject.map(|leader| g.followers_of(leader)).unwrap_or_default();
            if followers.is_empty() {
                zero
            } else {
                list(g, followers.into_iter().map(|f| XVal::Ref(f.0)).collect())
            }
        }
        // Actors currently detecting the calling actor (`Band::Detected` in `g.detection.pairs`)
        // [documented: GECK wiki GetDetectingActors; observed: gaps7, ar_Null when empty].
        "getdetectingactors" => {
            let detectors = subject.map(|target| g.detecting_actors(target)).unwrap_or_default();
            if detectors.is_empty() {
                zero
            } else {
                list(g, detectors.into_iter().map(|f| XVal::Ref(f.0)).collect())
            }
        }
        // The first member of the list the actor carries [observed: gaps7, the player
        // with the 10mm ammo list gives Ammo10mm, with a list it has none of 0].
        "pickfromlist" => {
            let a = subject.unwrap_or(FormId(0));
            let l = form(args, 0).unwrap_or(FormId(0));
            let members = crate::functions::list_members(g, l);
            XVal::Ref(members.into_iter().find(|&m| g.world.item_count(a, m).unwrap_or(0) > 0).map_or(0, |m| m.0))
        }
        // All its effects hostile and detrimental (MGEF flags 1 and 4) [documented:
        // GetIsPoison; observed: gaps7, Silver Sting 1, the stimpak 0].
        "getispoison" => {
            let f = form(args, 0).unwrap_or(FormId(0));
            let effects = g.data.effect_items.get(&g.unclone(f)).map(Vec::as_slice).unwrap_or_default();
            yes(!effects.is_empty() && effects.iter().all(|e| g.magic_effect_def(e.effect).is_some_and(|m| m.flags & 0x5 == 0x5)))
        }
        "isarmoraddon" => yes(form(args, 0).is_some_and(|f| g.data.sig(f) == Some(*b"ARMA"))),
        // A form type an inventory holds [documented: GetIsItem].
        "getisitem" => {
            let f = form(args, 0).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            yes(g.data.sig(f).is_some_and(|s| crate::functions::items::form_type_code(&s).is_some() || matches!(&s, b"CCRD" | b"CHIP" | b"CMNY" | b"IMOD")))
        }
        // DEST health; `object` is a base form (a reference reads its base) [documented:
        // GECK wiki GetDestructionDataHealth / SetDestructionDataHealth; observed: gaps8,
        // SetDestructionDataHealth returns 0 and 75 reads back]. Stored on
        // `Game::destruction` (`other.jip6.destruction`, carried across same-run loads via
        // `Game::Session`) so `damage_object` and `GetDestructionStage` read the change.
        "getdestructiondatahealth" => {
            let b = form(args, 0).map(|f| base(g, f)).unwrap_or(FormId(0));
            XVal::Num(g.destruction(b).map_or(0, |d| d.health) as f64)
        }
        "setdestructiondatahealth" => {
            let hp = num(args, 1, 0.0);
            if hp.is_finite() {
                if let Some(d) = form(args, 0).map(|f| base(g, f)).and_then(|b| g.destruction_mut(b)) {
                    d.health = hp as i32;
                }
            }
            zero
        }
        // CONT's Respawns flag; SetContainerRespawns takes the value first and returns 0
        // [observed: gaps8]. Stored on `World` so cell resets (`World::reset_containers`),
        // `ResetInterior`, and merchant restocks (`World::restocks`) all read it.
        "getcontainerrespawns" => {
            let b = form(args, 0).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            yes(g.world.container_respawns(b))
        }
        "setcontainerrespawns" => {
            if let Some(b) = form(args, 1).or(subject).map(|f| base(g, f)) {
                g.world.set_container_respawns(b, num(args, 0, 0.0) != 0.0);
            }
            zero
        }
        // On the reference, not its base [observed: gaps8, Doc 0 then 1 then 0, the base
        // 0].
        "getinteractiondisabled" => yes(subject.is_some_and(|r| g.gaps.interaction_disabled.contains(&r))),
        "setinteractiondisabled" => {
            if let Some(r) = subject {
                if num(args, 0, 0.0) != 0.0 {
                    g.gaps.interaction_disabled.insert(r);
                } else {
                    g.gaps.interaction_disabled.remove(&r);
                }
            }
            zero
        }
        // ---- worldspaces and climates ----
        // WRLD CNAM; the setter for the session [observed: gaps8, WastelandNV 0008809B,
        // set to the Strip's and back].
        "getworldspaceclimate" => XVal::Ref(form(args, 0).and_then(|w| g.worldspace_climate(w)).map_or(0, |c| c.0)),
        "setworldspaceclimate" => {
            if let (Some(w), Some(c)) = (form(args, 0), form(args, 1)) {
                if g.data.sig(g.unclone(w)) == Some(*b"WRLD") && g.data.sig(g.unclone(c)) == Some(*b"CLMT") {
                    g.field_overrides.insert((w, "CNAM/".into()), XVal::Ref(c.0));
                }
            }
            zero
        }
        // The player's worldspace's climate; none indoors [observed: gaps4 and gaps8,
        // 0008809B in the Mojave, unchanged by RefreshCurrentClimate]. ForceClimate's
        // or SetCurrentClimate's when a script set one (`crate::nvse_jip12`).
        "getcurrentclimate" => XVal::Ref(crate::nvse_jip12::current_climate(g).map_or(0, |c| c.0)),
        "refreshcurrentclimate" => zero,
        // WRLD DATA flags bit by bit [observed: gaps8, WastelandNV only 7].
        "getworldspaceflag" | "setworldspaceflag" => {
            let Some(w) = form(args, 0) else { return Some(Ok(zero)) };
            let flags = match crate::nvse::record_value(g, w, "DATA", "") {
                Some(XVal::Num(x)) => x as u64,
                _ => 0,
            };
            let k = num(args, 1, -1.0);
            if !(0.0..16.0).contains(&k) {
                return Some(Ok(zero));
            }
            if name == "setworldspaceflag" {
                let new = if num(args, 2, 0.0) != 0.0 { flags | 1 << (k as u64) } else { flags & !(1 << (k as u64)) };
                g.field_overrides.insert((w, "DATA/".into()), XVal::Num(new as f64));
                return Some(Ok(zero));
            }
            yes(flags >> (k as u64) & 1 != 0)
        }
        // CLMT GNAM and MODL [observed: gaps8, the Strip's sky\NV_sunglare.dds and
        // Sky\Stars.nif; a set texture reads back].
        "getclimatesunglaretexture" | "getclimatesuntexture" | "getclimatenightskymodel" => {
            let sub = match name {
                "getclimatesunglaretexture" => "GNAM",
                "getclimatesuntexture" => "FNAM",
                _ => "MODL",
            };
            match form(args, 0).and_then(|c| crate::nvse::record_value(g, c, sub, "")) {
                Some(XVal::Str(s)) => XVal::Str(s),
                _ => XVal::Str(String::new()),
            }
        }
        "setclimatesunglaretexture" | "setclimatesuntexture" | "setclimatenightskymodel" => {
            let sub = match name {
                "setclimatesunglaretexture" => "GNAM/",
                "setclimatesuntexture" => "FNAM/",
                _ => "MODL/",
            };
            if let Some(c) = form(args, 0) {
                g.field_overrides.insert((c, sub.into()), XVal::Str(text(args, 1)));
            }
            zero
        }
        // ARMO TNAM; setting none leaves it [observed: gaps8, leather 0013D3B5, the
        // hazmat suit's none does not clear it].
        "getarmoraudiotemplate" => match form(args, 0).and_then(|f| crate::nvse::record_value(g, f, "TNAM", "")) {
            Some(XVal::Ref(t)) => XVal::Ref(t),
            _ => XVal::Ref(0),
        },
        "setarmoraudiotemplate" => {
            if let (Some(f), Some(t)) = (form(args, 0), form(args, 1)) {
                g.field_overrides.insert((f, "TNAM/".into()), XVal::Ref(t.0));
            }
            zero
        }
        // `armor slot "path"` (formatted by the VM): 0 male biped MODL, 1 female MOD3, 2
        // male world MOD2, 3 female world MOD4, as GetBipedModelPath reads them
        // [documented: SetBipedModelPathAlt; observed: gaps8, the set path reads back].
        "setbipedmodelpathalt" => {
            let f = form(args, 0).unwrap_or(FormId(0));
            let sub = match num(args, 1, -1.0) as i64 {
                0 => "MODL/",
                1 => "MOD3/",
                2 => "MOD2/",
                3 => "MOD4/",
                _ => return Some(Ok(zero)),
            };
            g.field_overrides.insert((f, sub.into()), XVal::Str(text(args, 2)));
            zero
        }
        // The base's MODL [observed: gaps7, the dog's creature skeleton, Doc's
        // Characters\_Male\Skeleton.NIF]; a leveled or templated actor's as NX reads it
        // (`crate::nvse_nx::skeleton_model`).
        "getskeletonmodel" => XVal::Str(form(args, 0).or(subject).map(|f| crate::nvse_nx::skeleton_model(g, f)).unwrap_or_default()),
        "getownerofcell" => XVal::Ref(form(args, 0).and_then(|c| g.cell_ownership(c)).map_or(0, |o| o.owner.0)),
        // PKDT general flags bit by bit [observed: gaps7, Doc's sandbox 9 and 21 set];
        // SetPackageFlag returns 0, for the session [observed: gaps8], through
        // `Game::change_package` so every reader of `Game::package_def` sees it.
        "getpackageflag" | "setpackageflag" => {
            let p = form(args, 0).unwrap_or(FormId(0));
            let k = num(args, 1, -1.0);
            if !(0.0..32.0).contains(&k) {
                return Some(Ok(zero));
            }
            let Some(def) = g.package_def(&p) else { return Some(Ok(zero)) };
            let flags = def.flags;
            let old = flags >> (k as u32) & 1;
            if name == "setpackageflag" {
                let new = if num(args, 2, 0.0) != 0.0 { flags | 1 << (k as u32) } else { flags & !(1 << (k as u32)) };
                g.change_package(p, |d| d.flags = new);
                return Some(Ok(zero));
            }
            XVal::Num(f64::from(old))
        }
        // PERK DATA min level; SetPerkLevel for the session [documented: GECK wiki
        // GetPerkLevel / SetPerkLevel; observed: gaps7, Bloody Mess 6, set to 8 and back].
        // Stored on `Game::perk_def` (`lu3.perk_edits`) so level-up, requirement lines,
        // and ITR eligibility read the change.
        "getperklevel" => {
            let p = form(args, 0).unwrap_or(FormId(0));
            XVal::Num(g.perk_def(p).map_or(0.0, |d| f64::from(d.min_level)))
        }
        "setperklevel" => {
            if let Some(p) = form(args, 0).and_then(|p| g.perk_def_mut(p)) {
                p.min_level = num(args, 1, 0.0).clamp(0.0, 255.0) as u8;
            }
            zero
        }
        // The explosion's form traits, 0 light … 6 image-space modifier as GetExplosion-
        // TraitForm reads them [observed: gaps7]. Only EXPL forms (and their TempCloneForm
        // clones) are accepted [design]. Omitting the 3rd argument clears any slot 0..=6
        // to 0, whereas passing an explicit null ref (`0`) is a no-op across all slots
        // [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
        // GSDocMitchellHouse (0x00103DF9), 10 official base masters,
        // `w4h_02_explosion_slot4_reconcile` (`tools/capture/nvse/catalogs/wave4h_reconcile.json`,
        // #3803): `PROBE|w4h_02_explosion_slot4_reconcile|esmPop4=0/223|s4(0=00000000,set=0000003B,null=0000003B,lit0(0)=0000003B,omit(1)=00000000)|s0(0=00014BFF,null=00014BFF,omit=00000000)|s5(0=001758E2,null=001758E2,omit=00000000)|s6(0=00019CED,null=00019CED,omit=00000000)`;
        // hypothesis: slots 1, 2, 3 follow the same null-no-op / omitted-clears rule].
        // For non-null replacement forms, typed slots clear to 0 on a wrong-type form
        // [observed: `jip_24_expl_trait_form_type_check`
        // (`tools/capture/nvse/catalogs/wave4a_extenders.json`):
        // `PROBE|jip_24_expl_trait_form_type_check|s0:00014BFF->00000000(eq0=1,eqSt=0)|s4:00000000->00015169(eqSt=1)`;
        // hypothesis: slots 1, 2, 3, 5, 6 follow the same clear-on-mismatch rule], while
        // slot 4 (INAM) accepts any non-null form without type checking [observed:
        // `w4d_27_expl_inam_placeable` (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679):
        // `PROBE|w4d_27_expl_inam_placeable|f0=00000000|stat=0000003B|misc=0000000F|alch=00015169|fac=0001B2A4|clr=0001B2A4`;
        // no DLL or source consulted].
        "setexplosiontraitform" => {
            let Some(f) = form(args, 0).filter(|&f| g.data.sig(g.unclone(f)) == Some(*b"EXPL")) else {
                return Some(Ok(zero));
            };
            let (key, want): (&str, Option<&[u8; 4]>) = match num(args, 1, -1.0) as i64 {
                0 => ("DATA/light", Some(b"LIGH")),
                1 => ("DATA/sound_1", Some(b"SOUN")),
                2 => ("DATA/sound_2", Some(b"SOUN")),
                3 => ("DATA/impact_dataset", Some(b"IPDS")),
                4 => ("INAM/", None),
                5 => ("EITM/", Some(b"ENCH")),
                6 => ("MNAM/", Some(b"IMAD")),
                _ => return Some(Ok(zero)),
            };
            let next = if args.len() < 3 {
                0
            } else if let Some(e) = form(args, 2) {
                if want.is_none_or(|sig| g.data.sig(g.unclone(e)) == Some(*sig)) {
                    e.0
                } else {
                    0
                }
            } else {
                return Some(Ok(zero));
            };
            g.field_overrides.insert((f, key.into()), XVal::Ref(next));
            zero
        }
        // EXPL DATA flags; returns 0 [observed: gaps7, 0 both ways]. Only EXPL forms (and
        // their TempCloneForm clones) and flag IDs 1..32 (matching GetExplosionFlag) are
        // accepted [design].
        "setexplosionflag" => {
            let Some(f) = form(args, 0).filter(|&f| g.data.sig(g.unclone(f)) == Some(*b"EXPL")) else {
                return Some(Ok(zero));
            };
            let flags = match crate::nvse::record_value(g, f, "DATA", "flags") {
                Some(XVal::Num(x)) => x as u64,
                _ => 0,
            };
            let k = num(args, 1, -1.0);
            if !(1.0..32.0).contains(&k) {
                return Some(Ok(zero));
            }
            let new = if num(args, 2, 0.0) != 0.0 { flags | 1 << (k as u64) } else { flags & !(1 << (k as u64)) };
            g.field_overrides.insert((f, "DATA/flags".into()), XVal::Num(new as f64));
            zero
        }
        // The leveled list a placed actor came from, else its own base [documented:
        // GetLeveledActorBase; observed: gaps7, Doc his base 00104C0C].
        "getleveledactorbase" => {
            let b = subject.map(|r| g.world.placed.get(r).map_or_else(|| base(g, r), |p| p.base));
            XVal::Ref(b.map_or(0, |b| b.0))
        }
        // An actor can move [documented: IsMobile; observed: gaps7, Doc 1].
        "ismobile" => yes(subject.is_some_and(|r| crate::ai::is_actor_base(g.world.forms, base(g, r)))),
        // A null form when none is set [observed: plugins2, 00000000].
        "getcustommapmarker" => XVal::Ref(0),
        // `[base]`: the base's item list, one entry each [observed: gaps7, the Nuka-Cola
        // machine 3, Doc 7; hypothesis: each element the item].
        "getbaseitems" => {
            let b = form(args, 0).or(subject).map(|f| base(g, f)).unwrap_or(FormId(0));
            let items: Vec<XVal> = g.world.base_entries(b).unwrap_or_default().iter().map(|e| XVal::Ref(e.item.0)).collect();
            list(g, items)
        }
        // `typeCode [modIndex]`: the loaded forms of the type in the game's order
        // (`GameData::forms_of_type`), of one plugin when given [observed: gaps1, weapons
        // 496 in all, 265 of FalloutNV.esm; aid 226 and 189].
        "getloadedtypearray" => {
            let code = num(args, 0, -1.0) as i32;
            let plugin = args.get(1).map(|_| num(args, 1, 0.0) as u32);
            let forms: Vec<FormId> = g.data.forms_of_type(code).into_iter().filter(|f| plugin.is_none_or(|p| f.0 >> 24 == p)).collect();
            let forms = forms.into_iter().map(|f| XVal::Ref(f.0)).collect();
            list(g, forms)
        }
        // Carried weight over capacity, in percent [observed: gaps7, the player 46.50].
        "getencumbrancerate" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let w = g.world.carried_weight(a).unwrap_or(0.0);
            let cap = crate::functions::actor::actor_value(g, a, crate::actor_values::CARRY_WEIGHT).unwrap_or(0.0);
            XVal::Num(if cap > 0.0 { w / cap * 100.0 } else { 0.0 })
        }
        "gethardcoretracking" => yes(!g.jip2.hardcore_tracking_off),
        // `frames "source"` (formatted by the VM, `%r` a new line): the lines run on the
        // calling reference, the player when there is none, at once for 0, else that many
        // frames later [documented: RunScriptSnippet; observed: gaps10, "ModAV Luck 1"
        // raised the player's Luck at once].
        "runscriptsnippet" => {
            let delay = num(args, 0, 0.0).max(0.0) as u32;
            let src = text(args, 1);
            let on = subject.unwrap_or(FormId::PLAYER_REF);
            if delay == 0 {
                run_snippet(g, &src, on);
            } else {
                g.gaps.snippets.push((delay, src, on));
            }
            zero
        }
        // IPDS DATA by material and IPCT MODL [observed: gaps10].
        "getimpactdatasetform" | "setimpactdatasetform" => {
            let f = form(args, 0).unwrap_or(FormId(0));
            let Some(m) = IMPACT_MATERIALS.get(num(args, 1, -1.0).max(-1.0) as usize).filter(|_| num(args, 1, -1.0) >= 0.0) else { return Some(Ok(XVal::Ref(0))) };
            if name == "setimpactdatasetform" {
                g.field_overrides.insert((f, format!("DATA/{m}")), XVal::Ref(form(args, 2).map_or(0, |x| x.0)));
                return Some(Ok(zero));
            }
            match crate::nvse::record_value(g, f, "DATA", m) {
                Some(XVal::Ref(r)) => XVal::Ref(r),
                _ => XVal::Ref(0),
            }
        }
        "getimpactdatamodel" | "getweaponshellcasingmodel" => {
            let f = form(args, 0).unwrap_or(FormId(0));
            let sub = if name == "getimpactdatamodel" { "MODL" } else { "MOD2" };
            match crate::nvse::record_value(g, f, sub, "") {
                Some(XVal::Str(s)) => XVal::Str(s),
                _ => XVal::Str(String::new()),
            }
        }
        "setimpactdatamodel" | "setweaponshellcasingmodel" => {
            if let Some(f) = form(args, 0) {
                let sub = if name == "setimpactdatamodel" { "MODL/" } else { "MOD2/" };
                g.field_overrides.insert((f, sub.into()), XVal::Str(text(args, 1)));
            }
            zero
        }
        // WRLD INAM [observed: gaps10, WastelandNV 0008809D].
        "getworldspaceimagespace" => XVal::Ref(form(args, 0).and_then(|w| g.worldspace_image_space(w)).map_or(0, |i| i.0)),
        "setworldspaceimagespace" => {
            if let (Some(w), Some(i)) = (form(args, 0), form(args, 1)) {
                if g.data.sig(g.unclone(w)) == Some(*b"WRLD") && g.data.sig(g.unclone(i)) == Some(*b"IMGS") {
                    let next = XVal::Ref(i.0);
                    let prev = g.field_overrides.insert((w, "INAM/".into()), next.clone());
                    if prev != Some(next) {
                        g.traits.note_image_space_change();
                    }
                }
            }
            zero
        }
        // `x y z [translate]`: the position, or with translate a move in the reference's
        // own frame, +Y ahead: world ΔX = x cos h + y sin h, ΔY = y cos h − x sin h for
        // the heading h [observed: gaps11, Doc at 186.36° and the player at 0° and 90°].
        "setposex" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let Some((p, rot)) = g.ref_pos_rot(r) else { return Some(Ok(zero)) };
            let d = [num(args, 0, 0.0) as f32, num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32];
            let to = if num(args, 3, 0.0) != 0.0 {
                let (s, c) = rot[2].sin_cos();
                [p[0] + d[0] * c + d[1] * s, p[1] - d[0] * s + d[1] * c, p[2] + d[2]]
            } else {
                d
            };
            if let Ok(st) = g.world.get(r) {
                st.pos = to;
            }
            g.moved.insert(r);
            g.teleported.insert(r);
            zero
        }
        // No tilt shows on a person [observed: gaps10, Doc reads 0 before and after
        // SetActorTiltAngle X 20].
        "getactortiltangle" => zero,
        "setactortiltangle" => zero,
        // On an inventory reference: its condition, 1 as a fraction of the base health;
        // returns 1 [observed: gaps11, 50 reads 50; 0.5 1 reads 40 of the pistol's 80].
        "setitemrefcurrenthealth" | "setweaponrefmodflags" => {
            let Some((c, it)) = subject.and_then(|r| g.temp_item(r)) else { return Some(Ok(zero)) };
            let full = crate::nvse::record_value(g, it, "DATA", "health").and_then(|v| v.num().ok()).unwrap_or(0.0);
            let v = num(args, 0, 0.0);
            let ok = g.world.inventory_mut(c).is_ok_and(|inv| {
                let Some((_, e, _)) = inv.groups().find(|(i, _, _)| *i == it) else { return false };
                if name == "setweaponrefmodflags" {
                    inv.set_mods_one(it, e, v.clamp(0.0, 255.0) as u8)
                } else {
                    let cond = if num(args, 1, 0.0) != 0.0 { v } else if full > 0.0 { v / full } else { 1.0 };
                    inv.set_condition_one(it, e, cond as f32)
                }
            });
            yes(ok)
        }
        // The magazine of the actor (the player's as the engine tells it); an inventory
        // reference has none and SetCurrentAmmoRounds on one does nothing [observed:
        // gaps11, the item 0, the player 12; the player set to 3 reads 3].
        "getcurrentammorounds" | "setcurrentammorounds" => {
            let Some(a) = subject.filter(|&r| g.temp_item(r).is_none()) else { return Some(Ok(zero)) };
            if a != FormId::PLAYER_REF {
                return Some(Ok(zero));
            }
            let call = if name == "setcurrentammorounds" { "setplayercurrentammorounds" } else { "getplayercurrentammorounds" };
            return crate::nvse::call(g, ctx, call, None, args);
        }
        // ---- keywords ----
        // The base forms with the keyword [observed: gaps3, one after AssignKeyword, an
        // unknown keyword an empty array].
        "getkeywordforms" => {
            let k = text(args, 0).to_ascii_lowercase();
            let mut forms: Vec<FormId> = g.jip2.keywords.iter().filter(|(_, s)| s.contains(&k)).map(|(&f, _)| f).collect();
            forms.sort();
            let forms = forms.into_iter().map(|f| XVal::Ref(f.0)).collect();
            list(g, forms)
        }
        // ---- cells, climates, recipes ----
        // CELL DATA flags bit by bit [observed: gaps3, Doc's house (Interior, Public)
        // reads 1 0 0 0].
        // 0-7 DATA, 8-16 the lighting template inherit flags (LNAM bits 0-8)
        // [documented: GECK wiki GetCellFlag; hypothesis: LNAM's order]; SetCellFlag's change
        // first (`crate::nvse_jip8`). Require a CELL form so non-CELL records with a DATA or
        // LNAM subrecord (such as WRLD, #3757) return 0 rather than leaking their own flags
        // [documented: GECK wiki GetCellFlag takes `cell:ref`; hypothesis: original JIP LN
        // return on a WRLD with bit 0 set pending runtime probe].
        "getcellflag" => {
            let Some(c) = form(args, 0).filter(|&f| g.data.sig(g.unclone(f)) == Some(*b"CELL")) else {
                return Some(Ok(zero));
            };
            let k = num(args, 1, -1.0);
            if !(0.0..=16.0).contains(&k) || k.fract() != 0.0 {
                return Some(Ok(zero));
            }
            let (sub, k) = if k >= 8.0 { ("LNAM", k - 8.0) } else { ("DATA", k) };
            let flags = crate::nvse::record_value(g, c, sub, "").and_then(|v| v.num().ok()).unwrap_or(0.0);
            bit(flags, k)
        }
        // XCIM, with SetCellImageSpace first [observed: gaps4, Doc's house 0001507A].
        "getcellimagespace" => XVal::Ref(form(args, 0).and_then(|c| g.cell_image_space(c)).map_or(0, |i| i.0)),
        // CLMT WLST [observed: gaps4, the Strip's climate 1 weather, 00161241, global
        // none, past the end 0].
        // ClimateAddWeatherType's and ClimateRemoveWeatherType's changes first
        // (`crate::nvse_jip7`).
        "getclimatenumweathertypes" => XVal::Num(form(args, 0).map_or(0, |c| g.climate_weathers(c).len()) as f64),
        "getclimatenthweathertype" | "getclimatenthweatherglobal" | "getclimatenthweatherchance" => {
            let i = num(args, 1, -1.0);
            let all = form(args, 0).map(|c| g.climate_weathers(c)).unwrap_or_default();
            let e = if i >= 0.0 { all.get(i as usize).copied() } else { None };
            match name {
                "getclimatenthweathertype" => XVal::Ref(e.map_or(0, |e| e.weather.0)),
                "getclimatenthweatherglobal" => XVal::Ref(e.and_then(|e| e.global).map_or(0, |f| f.0)),
                _ => XVal::Num(e.map_or(0, |e| e.chance) as f64),
            }
        }
        // Night outside the climate's sunrise begin to sunset end (TNAM, ten-minute steps)
        // [observed: gaps4, the Strip's climate at 10 reads 0; hypothesis: the edges, and
        // an omitted climate falls back to the current climate].
        "isnight" => {
            let c = form(args, 0).or_else(|| crate::nvse_jip12::current_climate(g)).unwrap_or(FormId(0));
            let c = g.unclone(c);
            let hour = g.world.global(FormId(0x38)).map_or(12.0, f64::from);
            let t = |k: &str| json_num(g.data.record_field(c, "TNAM", k)) / 6.0;
            let (rise, set) = (t("sunrise_begin"), t("sunset_end"));
            yes(rise != set && (hour < rise || hour >= set))
        }
        "getrecipeoutputforms" => {
            let outs: Vec<XVal> = form(args, 0).and_then(|f| crate::nvse_lutana::recipe(g, f)).map(|r| r.outputs.iter().map(|(f, _)| XVal::Ref(f.0)).collect()).unwrap_or_default();
            list(g, outs)
        }
        "getactivequest" => XVal::Ref(g.ui.active_quest.map_or(0, |q| q.0)),
        // ---- the player and the interface ----
        // `seconds "[icon|][$sound|]text"`: a corner message; the icon a texture under the
        // textures folder or `#0`–`#6` the page's default faces, the sound a SOUN's Editor
        // ID after `$` [documented: MessageExAlt].
        "messageexalt" => {
            let seconds = num(args, 0, 2.0) as f32;
            let (text, icon, sound) = split_styled(&text(args, 1));
            if let Some(s) = sound.and_then(|s| g.data.by_edid(&s)) {
                g.ask_sound(crate::sound::SoundRequest { sound: s, at: None, system: true, id: 0, point: None });
            }
            if !text.trim().is_empty() {
                g.count_corner_message();
            }
            g.emit(crate::trace::EventKind::NoteShown { text, icon, seconds });
            zero
        }
        // [observed: gaps2, off at the start; on at once, off from the next frame]
        "togglepipboylight" => {
            match args.first() {
                Some(XVal::Num(x)) if *x != 0.0 => {
                    g.gaps.pipboy_light = true;
                    g.gaps.light_off_at = None;
                }
                Some(XVal::Num(_)) if g.gaps.pipboy_light && g.gaps.light_off_at.is_none() => g.gaps.light_off_at = Some(g.time),
                _ => {}
            }
            yes(g.pipboy_light())
        }
        // `slot [toggle]`: the slot's visibility, 1 shown [observed: gaps3, slot 2 reads 1].
        "togglebipedslotvisibility" => {
            let slot = num(args, 0, -1.0).max(0.0) as u32;
            if let Some(XVal::Num(t)) = args.get(1) {
                if *t == 0.0 {
                    g.gaps.hidden_slots.insert(slot);
                } else {
                    g.gaps.hidden_slots.remove(&slot);
                }
            }
            yes(!g.gaps.hidden_slots.contains(&slot))
        }
        // The view changes at the engine's next frame [observed: engine1, third person
        // reads from the next frame, first person after one to three]; sneaking at once,
        // IsSneaking reading it in the same frame [observed: engine1].
        "togglefirstperson" => {
            g.gaps.view_request = Some(num(args, 0, 1.0) != 0.0);
            zero
        }
        "toggleplayersneaking" => {
            let on = num(args, 0, 1.0) != 0.0;
            g.gaps.sneak_request = Some(on);
            let mut m = g.motion(FormId::PLAYER_REF);
            m.sneaking = on;
            g.set_motion(FormId::PLAYER_REF, m);
            zero
        }
        // `x1 y1 z1 x2 y2 z2 [flag]`: the distance from the first point to the first thing
        // hit on the way to the second, 0 for none (`crate::ray`, the engine's physics, no
        // terrain) [observed: engine1, 997 and 1963 to the walls by the airport, 0 with no
        // hit or a zero-length cast; engine2 and engine3, flags 0–4 read alike].
        "getp2praycastrange" => {
            let p: Vec<f32> = (0..6).map(|k| num(args, k, 0.0) as f32).collect();
            let (a, b) = ([p[0], p[1], p[2]], [p[3], p[4], p[5]]);
            if a == b {
                return Some(Ok(zero));
            }
            let hit = crate::ray::cast(a, b, crate::ray::CastKind::Scene);
            tracing::debug!("GetP2PRayCastRange {a:?} -> {b:?}: {hit:?}");
            XVal::Num(hit.map_or(0.0, f64::from))
        }
        // 2 playing, 0 not (1 paused is not told apart); a type from the frame PlayMusic
        // asks for it, none given any music [documented: IsMusicPlaying; observed:
        // engine2, musDocMitchellHouse reads 2 in PlayMusic's frame, the place's music 2].
        "ismusicplaying" => {
            let on = match form(args, 0) {
                Some(t) => g.gaps.music_type == Some(t),
                None => g.gaps.music_any || g.gaps.music_type.is_some(),
            };
            XVal::Num(if on { 2.0 } else { 0.0 })
        }
        // `x y`: the terrain's height there, from the engine's loaded terrain; outside it
        // -2048 [observed: gaps7, 60.3 at the player; L5.ext rerun, #2040: `Floor` of it far
        // outside the loaded cells is -2048, so -2048 to -2047; hypothesis: -2048 itself].
        // `x y z force [origin]`: the reference's loose body is pushed away from the
        // origin (the reference's place, or `origin`'s, moved by x y z), towards it for a
        // negative force; the engine applies it at its next frame [documented: PushObject;
        // observed: phys2, the speed grows with the force, `nvf_play::script`].
        "pushobject" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let at = form(args, 4).unwrap_or(r);
            if let Some((p, _)) = g.ref_pos_rot(at) {
                let origin = [p[0] + num(args, 0, 0.0) as f32, p[1] + num(args, 1, 0.0) as f32, p[2] + num(args, 2, 0.0) as f32];
                g.gaps.pushes.push((r, origin, num(args, 3, 0.0) as f32));
            }
            zero
        }
        // JohnnyGuitar: the enabled Pip-Boy stations that broadcast everywhere, wherever
        // the reference stands, in FormID order; none is an empty array [documented:
        // GetAvailableRadios; observed: phys2, phys3, radio1, the same 5 inside the
        // nvftest interior and outdoors at Goodsprings: Radio New Vegas, Mojave Music
        // Radio and the three add-ons' broadcasts; not the radius stations nor the
        // everywhere stations off the Pip-Boy (Gomorrah's lounge music, EDE's frequency);
        // hypothesis: the order, which was not FormID order].
        "getavailableradios" => {
            if subject.is_none() {
                return Some(Ok(zero));
            }
            let mut found: Vec<u32> = g
                .gaps
                .radio_sites
                .iter()
                .flatten()
                .filter(|s| s.on_pipboy && matches!(s.broadcast.range, fnv_world::radio::BroadcastRange::Everywhere))
                .filter(|s| g.world.is_enabled(s.station).unwrap_or(!s.initially_disabled))
                .map(|s| s.station.0)
                .collect();
            found.sort_unstable();
            list(g, found.into_iter().map(XVal::Ref).collect())
        }
        // A live mine that is not going off, 0 for the player's own mines and anything
        // else [documented: GetMineArmed, "always returns 0 on player-placed mines";
        // observed: phys1, phys2, a placed frag mine read 1 from its first frame, the
        // player 0; hypothesis: a burning fuse reads 0].
        "getminearmed" => yes(subject.is_some_and(|r| g.gaps.armed_mines.contains(&r))),
        // Seconds since the actor's body left the ground, 0 on it [documented:
        // GetFallTimeElapsed; observed: phys3, rising 0.094 every 5 frames from a SetPos
        // lift of 600 to 1.297, 0 from the landing on; hypothesis: a jump counts too].
        "getfalltimeelapsed" => XVal::Num(f64::from(subject.and_then(|a| g.gaps.air_times.get(&a)).copied().unwrap_or(0.0))),
        // `navmesh`: the switches DisableNavmesh and EnableNavMesh keep, the navmesh named
        // by reference (`functions::navigation`) [documented: DisableNavMeshAlt,
        // EnableNavMeshAlt].
        "disablenavmeshalt" | "enablenavmeshalt" => {
            if let Some(m) = form(args, 0) {
                g.nav_switches.navmeshes.insert(m, name == "enablenavmeshalt");
            }
            zero
        }
        // `thing [scan]`: the player's theft of the item or container reference, as taking
        // it would be (`crime`: karma unless the owner is Evil, the owner's side turns on
        // the player when it sees it) [documented: SendStealingAlarm]. With `scan` 1 no
        // alarm, as nothing here marks items stolen; other thieves send none [hypothesis:
        // NVForever's crimes are the player's].
        "sendstealingalarm" => {
            let thief = subject.unwrap_or(FormId::PLAYER_REF);
            if let (Some(r), true, false) = (form(args, 0), thief == FormId::PLAYER_REF, num(args, 1, 0.0) != 0.0) {
                let item = base(g, r);
                g.player_stole(Some(r), None, item, 1);
            }
            zero
        }
        "getterrainheight" => {
            let (x, y) = (num(args, 0, 0.0) as f32, num(args, 1, 0.0) as f32);
            let top = 100_000.0f32;
            match crate::ray::cast([x, y, top], [x, y, -top], crate::ray::CastKind::Terrain) {
                Some(d) => XVal::Num(f64::from(top - d)),
                None => XVal::Num(-2048.0),
            }
        }
        // `[max]`: the distance along the crosshair to what it meets (the engine's
        // crosshair ray) [observed: gaps8, 93.84 on the save]; -1 for nothing there
        // [documented: GetReticleRange, -1 when the point is invalid].
        "getreticlerange" => {
            let hit = g.senses.reticle.as_ref().and_then(|r| r.hit);
            let max = num(args, 0, 0.0);
            XVal::Num(hit.map_or(-1.0, |h| f64::from(h).min(if max > 0.0 { max } else { f64::MAX })))
        }
        // An attack is on the way: the body's attack, follow-through or return action
        // [documented: IsAttacking; observed: gaps4, 0 standing; hypothesis: the actions].
        "isattacking" => {
            use crate::anim_state::AnimAction;
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            yes(g.senses.anim.get(&a).and_then(|s| s.action).is_some_and(|x| matches!(x, AnimAction::Attack | AnimAction::FollowThrough | AnimAction::AttackReturn)))
        }
        // [documented: IsPCInCombat] the player is hunted (`awareness`).
        "ispcincombat" => yes(g.awareness.hunted(FormId::PLAYER_REF)),
        // `[equipSlot]`: reloads the calling actor's equipped weapon/armour 3D models on
        // the next engine frame (all slots `0..=19` when omitted; single biped slot bit
        // when `0..=19` is given; alias `ReloadModels`) [documented: ReloadEquippedModels].
        "reloadequippedmodels" | "reloadmodels" => {
            if let Some(r) = subject.or(ctx.self_ref) {
                let mask = match args.first() {
                    None => ALL_EQUIP_SLOTS,
                    Some(XVal::Num(n)) if n.is_finite() => {
                        let slot = *n as i64;
                        if (0..20).contains(&slot) { 1u32 << (slot as u32) } else { 0 }
                    }
                    _ => 0,
                };
                g.queue_equipped_model_reload(r, mask);
            }
            zero
        }
        // Whether the program may use more than 2 GB (the 4 GB patch). The original
        // without the patch answers 0 [observed: gaps4]; NVForever is a 64-bit program with
        // no such limit, so it answers 1 [design, owner 2026-11-08: TTW's start-up check
        // `GetIsLAA == 0` showed "4GB patch missing" (TTW4GBMissingMessage)].
        "getislaa" => XVal::Num(1.0),
        // The open menu's code, 0 in game mode [observed: gaps3, 0].
        "getactivemenumode" => XVal::Num(g.menu.or_else(|| g.menu_open(crate::game::menus::START).then_some(crate::game::menus::START)).map_or(0.0, f64::from)),
        // [observed: gaps3, 1]
        "getglobaltimemultiplier" => XVal::Num(g.ui.time_multiplier.unwrap_or(1.0)),
        // Activation-range object water, through the same effective ACTI/WNAM
        // accessor as GetActivatorWaterType. This does not detect open surfaces.
        "getcrosshairwater" => {
            let base = g.senses.crosshair_ref
                .and_then(|r| crate::functions::base_of(g, r))
                .filter(|&b| g.data.sig(b) == Some(*b"ACTI"));
            match base.and_then(|b| crate::nvse::record_value(g, b, "WNAM", "")) {
                Some(XVal::Ref(water)) => XVal::Ref(water),
                _ => XVal::Ref(0),
            }
        }
        // Return 0 [observed: gaps4].
        "stopsoundalt" => zero,
        // An unknown patch [observed: gaps2 and gaps4, 0 for both].
        "getoptionalpatch" | "setoptionalpatch" => zero,
        _ => return None,
    };
    Some(Ok(v))
}

/// Actors currently in the loaded cells (`GetActorsByProcessingLevel 0`): placed `ACHR`/`ACRE`
/// whose current cell is in `loaded_cells` (including actors moved or walked into a loaded cell)
/// and live created actors in `loaded_cells`, in FormID order. Unlike `Game::package_actors`,
/// this excludes actors in unloaded cells that previously chose a package.
fn loaded_processing_actors(g: &Game) -> std::collections::BTreeSet<FormId> {
    let mut out = std::collections::BTreeSet::new();
    if g.loaded_cells.is_empty() {
        return out;
    }
    let placed = g.world.placed;
    let forms = g.world.forms;
    let is_placed_actor = |r: FormId| placed.get(r).is_some_and(|p| matches!(&p.sig, b"ACHR" | b"ACRE"));
    for c in &g.loaded_cells {
        if let Some(refs) = placed.by_cell.get(c) {
            for &r in refs {
                if is_placed_actor(r) {
                    out.insert(r);
                }
            }
        }
    }
    for (r, s) in g.world.changed() {
        if !s.cell.is_some_and(|c| g.loaded_cells.contains(&c)) {
            continue;
        }
        if r >= FormId::FIRST_DYNAMIC {
            if !s.deleted() && crate::ai::is_actor_base(forms, s.base) {
                out.insert(r);
            }
        } else if is_placed_actor(r) {
            out.insert(r);
        }
    }
    out.retain(|&r| g.cell_of(r).is_some_and(|c| g.loaded_cells.contains(&c)));
    out
}

#[cfg(test)]
#[path = "nvse_gaps_proc_level_tests.rs"]
mod proc_level_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weapon_sound_ids_follow_the_observed_order() {
        assert_eq!(weapon_sound_sub(0), Some(("SNAM", 0)));
        assert_eq!(weapon_sound_sub(1), Some(("SNAM", 1)));
        assert_eq!(weapon_sound_sub(2), Some(("XNAM", 0)));
        assert_eq!(weapon_sound_sub(9), Some(("WMS1", 0)));
        assert_eq!(weapon_sound_sub(10), Some(("WMS1", 1)));
        assert_eq!(weapon_sound_sub(11), Some(("WMS2", 0)));
        assert_eq!(weapon_sound_sub(12), Some(("YNAM", 0)));
        assert_eq!(weapon_sound_sub(13), Some(("ZNAM", 0)));
        assert_eq!(weapon_sound_sub(14), None);
    }

    #[test]
    fn styled_messages_take_their_icon_and_sound_off_the_front() {
        let (t, i, s) = split_styled("Interface\\Icons\\a.dds|$UIRepGood|Name is X|Y");
        assert_eq!((t.as_str(), i.as_deref(), s.as_deref()), ("Name is X|Y", Some("Interface\\Icons\\a.dds"), Some("UIRepGood")));
        let (t, i, _) = split_styled("#0|Test");
        assert_eq!(t, "Test");
        assert!(i.is_some_and(|i| i.ends_with("glow_message_vaultboy_sad.dds")));
        assert_eq!(split_styled("plain").0, "plain");
    }

    #[test]
    fn flag_bits() {
        assert_eq!(bit(5.0, 0.0), XVal::Num(1.0));
        assert_eq!(bit(5.0, 1.0), XVal::Num(0.0));
        assert_eq!(bit(5.0, 2.0), XVal::Num(1.0));
        assert_eq!(bit(5.0, -1.0), XVal::Num(0.0));
    }

    #[test]
    fn equip_reload_queue_caps_non_player_entries_and_preserves_player() {
        let mut gaps = GapsState::default();
        // Enqueue player first, then non-player actors spanning both below and above
        // PLAYER_REF (0x14) so PLAYER_REF becomes the smallest remaining key during eviction.
        gaps.queue_equipped_model_reload(FormId::PLAYER_REF, 1 << 2);
        let mut inserted = Vec::new();
        let mut raw = 0x10u32;
        while inserted.len() < EQUIP_RELOAD_QUEUE_LIMIT + 32 {
            let id = FormId(raw);
            raw += 1;
            if id == FormId::PLAYER_REF {
                continue;
            }
            gaps.queue_equipped_model_reload(id, EQUIP_SLOT_WEAPON_BIT);
            inserted.push(id);
        }

        assert_eq!(gaps.equipped_model_reloads.get(&FormId::PLAYER_REF), Some(&(1 << 2)), "PLAYER_REF must never be evicted");
        assert_eq!(gaps.equipped_model_reloads.len(), EQUIP_RELOAD_QUEUE_LIMIT + 1);
        for &evicted in &inserted[..32] {
            assert!(!gaps.equipped_model_reloads.contains_key(&evicted), "earliest non-player entry {evicted:?} evicted");
        }
        for &kept in &inserted[32..] {
            assert_eq!(gaps.equipped_model_reloads.get(&kept), Some(&EQUIP_SLOT_WEAPON_BIT));
        }

        // Re-queueing an existing non-player entry at capacity coalesces without evicting;
        // zero mask is ignored without evicting.
        let sample = inserted[32];
        gaps.queue_equipped_model_reload(sample, 1 << 3);
        gaps.queue_equipped_model_reload(FormId(0x9000), 0);
        assert_eq!(gaps.equipped_model_reloads.len(), EQUIP_RELOAD_QUEUE_LIMIT + 1);
        assert_eq!(gaps.equipped_model_reloads.get(&sample), Some(&(EQUIP_SLOT_WEAPON_BIT | (1 << 3))));
        assert!(!gaps.equipped_model_reloads.contains_key(&FormId(0x9000)));

        // Draining NPCs yields exactly EQUIP_RELOAD_QUEUE_LIMIT entries and leaves PLAYER_REF intact.
        let npcs = gaps.take_npc_equipped_model_reloads();
        assert_eq!(npcs.len(), EQUIP_RELOAD_QUEUE_LIMIT);
        assert!(!npcs.contains_key(&FormId::PLAYER_REF));
        assert_eq!(gaps.take_player_equipped_model_reload(), 1 << 2);
        assert_eq!(gaps.take_player_equipped_model_reload(), 0);

        // Filling 1024 non-player entries first and then queueing PLAYER_REF must not evict any NPC.
        for &id in &inserted[..EQUIP_RELOAD_QUEUE_LIMIT] {
            gaps.queue_equipped_model_reload(id, ALL_EQUIP_SLOTS);
        }
        gaps.queue_equipped_model_reload(FormId::PLAYER_REF, EQUIP_SLOT_WEAPON_BIT);
        assert_eq!(gaps.equipped_model_reloads.len(), EQUIP_RELOAD_QUEUE_LIMIT + 1);
        assert_eq!(gaps.take_npc_equipped_model_reloads().len(), EQUIP_RELOAD_QUEUE_LIMIT);
        assert_eq!(gaps.take_player_equipped_model_reload(), EQUIP_SLOT_WEAPON_BIT);
    }
}
