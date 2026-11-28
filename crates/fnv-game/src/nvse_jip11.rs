//! JIP LN NVSE, batch 11 (#1006): actors. Combat allies and targets, detection (the
//! values, the detected actors, a forced value), packages, the actor's alpha, gravity,
//! light, breath, movement, velocity, leveled list and unarmed effect, base actor values
//! of a base form, kill XP, radiation at an NPC, falls, active and temporary effects and
//! effect lists, the equipped items (the first of a list, scope and silencer, slot
//! conflicts, no-unequip), armour DR and DT by condition and whether a hit got through,
//! base faction ranks, ClearDeadActors, the hardcore stages' effects and the weapon
//! condition penalty [documented: each page in the wiki export;
//! docs/spec-notes/jip-actors-11.md].

use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::{BTreeMap, BTreeSet};

/// What this batch keeps, for the session [hypothesis: JIP's base-form changes are not
/// saved; the pages say nothing].
#[derive(Debug, Default)]
pub struct Jip11State {
    /// ForceActorDetectionValue: the actor's value, and whether it is a modifier.
    pub forced_detection: BTreeMap<FormId, (f64, bool)>,
    /// The light level at each actor the last detection round measured (0-100).
    pub light: BTreeMap<FormId, f64>,
    /// The player's breath left, seconds, and whether under water (the engine writes it).
    pub player_breath: Option<(f32, bool)>,
    /// SetActorDiveBreath's seconds, for the engine to take.
    pub set_breath: Option<f32>,
    /// ResetFallTime's actors, for the engine to take.
    pub fall_resets: BTreeSet<FormId>,
    /// Engine-reported remaining fall time in seconds, by actor (GetFallTimeRemaining).
    pub fall_time_remaining: BTreeMap<FormId, f32>,
    /// SetActorUnarmedEffect's effects, by the actor base that supplies its effects.
    pub unarmed: BTreeMap<FormId, FormId>,
    /// RemoveNthEffect's removed entries, by magic item: record indices.
    pub removed_effects: BTreeMap<FormId, BTreeSet<usize>>,
    /// Whether the last hit on each actor got through its DT (GetArmourPenetrated).
    pub penetrated: BTreeMap<FormId, bool>,
    /// SetBaseFactionRank's ranks, by (the base that supplies the factions, faction).
    pub base_ranks: BTreeMap<(FormId, FormId), i8>,
    /// SetHardcoreStageEffect's effects, by stage record (`None`: no effect).
    pub stage_effects: BTreeMap<FormId, Option<FormId>>,
    /// SetConditionDamagePenalty's multiplier.
    pub condition_penalty: Option<f32>,
}

fn num(args: &[XVal], k: usize) -> Option<f64> {
    match args.get(k) {
        Some(XVal::Num(x)) if x.is_finite() => Some(*x),
        _ => None,
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

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn reff(f: Option<FormId>) -> XVal {
    XVal::Ref(f.map_or(0, |f| f.0))
}

/// A list getter's array of forms; no array when there is nothing to give [observed:
/// the jip10 probes, 2026-11-08, the same rule for JIP's list getters].
fn refs(g: &mut Game, items: impl IntoIterator<Item = FormId>) -> XVal {
    let entries: Vec<(XKey, XVal)> = items.into_iter().enumerate().map(|(i, f)| (XKey::Num(i as f64), XVal::Ref(f.0))).collect();
    if entries.is_empty() {
        return XVal::Array(0);
    }
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

/// An actor value argument: its code, or its name.
fn av(args: &[XVal], k: usize) -> Option<u16> {
    match args.get(k)? {
        XVal::Num(x) if *x >= 0.0 => Some(*x as u16),
        XVal::Str(s) => crate::functions::actor::av_by_name(s),
        _ => None,
    }
}

/// A number field of a record (`sub`, then `path`), with the session's changes.
fn field_num(g: &Game, f: FormId, sub: &str, path: &str) -> Option<f64> {
    match crate::nvse::record_value(g, f, sub, path)? {
        XVal::Num(x) => Some(x),
        XVal::Ref(r) => Some(f64::from(r)),
        _ => None,
    }
}

/// A weapon mod slot's effect (WEAP DNAM `effect_mod1..3`), by the schema's name.
fn mod_effect(g: &Game, w: FormId, slot: u8) -> Option<String> {
    match crate::nvse::record_value(g, w, "DNAM", &format!("effect_mod{slot}"))? {
        XVal::Str(s) => Some(s),
        // WeaponModEffect [documented: the Layer 1 schema, objects.json].
        XVal::Num(x) => Some(
            match x as i64 {
                11 => "Silence",
                14 => "Increase Zoom",
                16 => "Suppressor",
                _ => "",
            }
            .to_string(),
        ),
        _ => None,
    }
}

impl Game<'_> {
    /// SetConditionDamagePenalty's multiplier, else the hard-coded 0.67 [documented].
    pub fn condition_damage_penalty(&self) -> f32 {
        self.other.jip11.condition_penalty.unwrap_or(fnv_sim::inventory::CONDITION_DAMAGE_PENALTY)
    }

    /// Weapon damage condition multiplier with [`Game::condition_damage_penalty`] and
    /// Stewie's `no_condition_damage_penalty` / `no_condition_damage_buffer` tweaks
    /// (matching `fnv_combat::damage` and `engine/nvf-play/src/combat.rs`).
    pub fn weapon_condition_mult(&self, condition: f32) -> f32 {
        if self.tweaks.no_condition_damage_penalty {
            return 1.0;
        }
        let threshold = if self.tweaks.no_condition_damage_buffer {
            1.0
        } else {
            fnv_sim::inventory::CONDITION_DAMAGE_THRESHOLD
        };
        (1.0 - (threshold - condition.clamp(0.0, 1.0)).max(0.0) * self.condition_damage_penalty().max(0.0)).max(0.0)
    }

    /// A hit on `target`: whether its damage before armour got past the target's DT
    /// (GetArmourPenetrated) [documented: "had penetrated the actor's DT, or was
    /// fully-absorbed by it"]. The engine tells it before the hit is dealt, so the hit's
    /// OnHit block reads it.
    pub fn note_armour_hit(&mut self, target: FormId, raw: f64, dt: f64) {
        self.other.jip11.penetrated.insert(target, raw > dt);
    }

    /// The player's breath for GetActorDiveBreath: the seconds left, and whether under
    /// water. Returns SetActorDiveBreath's seconds, if a script set them since.
    pub fn sync_player_breath(&mut self, left: f32, under: bool) -> Option<f32> {
        self.other.jip11.player_breath = Some((left, under));
        self.other.jip11.set_breath.take()
    }

    /// ResetFallTime's actors since the last call.
    pub fn take_fall_resets(&mut self) -> Vec<FormId> {
        std::mem::take(&mut self.other.jip11.fall_resets).into_iter().collect()
    }

    /// Record or clear an actor's remaining fall time in seconds (GetFallTimeRemaining).
    pub fn set_fall_time_remaining(&mut self, actor: FormId, seconds: Option<f32>) {
        match seconds.filter(|s| s.is_finite() && *s > 0.0) {
            Some(s) => {
                self.other.jip11.fall_time_remaining.insert(actor, s);
            }
            None => {
                self.other.jip11.fall_time_remaining.remove(&actor);
            }
        }
    }

    /// Estimated seconds remaining before `actor` lands (GetFallTimeRemaining)
    /// [documented: GECK wiki GetFallTimeRemaining; hypothesis: kinematic time to ground
    /// from ray cast, tracked velocity, and gravity when no direct engine value is set].
    pub fn fall_time_remaining(&self, actor: FormId) -> f64 {
        if let Some(&sec) = self.other.jip11.fall_time_remaining.get(&actor) {
            return f64::from(sec.max(0.0));
        }
        let air_time = self.gaps.air_times.get(&actor).copied().unwrap_or(0.0);
        if air_time <= 0.0 {
            return 0.0;
        }
        let Some(pos) = self.world.peek(actor).map(|s| s.pos).or_else(|| self.world.placed.get(actor).map(|p| p.pos)) else {
            return 0.0;
        };
        let Some(dist) = crate::ray::cast(
            [pos[0], pos[1], pos[2] + 1.0],
            [pos[0], pos[1], pos[2] - 100_000.0],
            crate::ray::CastKind::World,
        ) else {
            return 0.0;
        };
        let height = f64::from((dist - 1.0).max(0.0));
        if height <= 0.0 {
            return 0.0;
        }
        // Standard world gravity: 9.81 m/s^2 * 70 game units/m = 686.7 units/s^2
        // (matching `fnv_combat::projectile` / `fnv-phys`), overridden by SetLocalGravityVector
        // and scaled by SetActorGravityMult.
        const WORLD_GRAVITY: f64 = 686.7;
        let g_base = match self.other.jip13.gravity {
            Some((gv, cell_opt)) if cell_opt.is_none() || cell_opt == self.cell_of(actor) => f64::from(-gv[2]),
            _ => WORLD_GRAVITY,
        };
        let mult = self.gaps.gravity.get(&actor).copied().unwrap_or(1.0);
        let g_eff = g_base * mult;
        let vz = match self.jip2.velocity.get(&actor) {
            Some(&(_, v)) => f64::from(v[2]),
            None if g_eff > 0.0 => -g_eff * f64::from(air_time),
            None => 0.0,
        };
        if g_eff <= 0.0 {
            return if vz < 0.0 { height / (-vz) } else { 0.0 };
        }
        let disc = (vz * vz + 2.0 * g_eff * height).max(0.0);
        ((vz + disc.sqrt()) / g_eff).max(0.0)
    }

    /// The detection value of `target` as ForceActorDetectionValue leaves it: the forced
    /// value, or the value plus the modifier [documented].
    pub fn forced_detection(&self, target: FormId, value: f64) -> f64 {
        match self.other.jip11.forced_detection.get(&target) {
            Some(&(v, true)) => value + v,
            Some(&(v, false)) => v,
            None => value,
        }
    }

    /// The unarmed effect SetActorUnarmedEffect gave the actor base `source`, if any.
    pub(crate) fn unarmed_override(&self, source: FormId) -> Option<FormId> {
        self.other.jip11.unarmed.get(&source).copied()
    }

    /// The record indices of a magic item's effects RemoveNthEffect left, in order.
    pub fn effect_indices(&self, item: FormId) -> Vec<usize> {
        let n = self.data.effect_items.get(&self.unclone(item)).map_or(0, Vec::len);
        let gone = self.other.jip11.removed_effects.get(&item);
        (0..n).filter(|i| !gone.is_some_and(|g| g.contains(i))).collect()
    }

    /// Whether RemoveNthEffect took a magic item's record entry `index` away.
    pub fn effect_removed(&self, item: FormId, index: usize) -> bool {
        self.other.jip11.removed_effects.get(&item).is_some_and(|g| g.contains(&index))
    }

    /// SetBaseFactionRank's rank of the actor base `source` (the one supplying the
    /// factions) in `faction`.
    pub(crate) fn base_rank_override(&self, source: FormId, faction: FormId) -> Option<i8> {
        self.other.jip11.base_ranks.get(&(source, faction)).copied()
    }

    /// SetBaseFactionRank's factions of the actor base `source`.
    pub(crate) fn base_rank_overrides(&self, source: FormId) -> Vec<(FormId, i8)> {
        self.other.jip11.base_ranks.range((source, FormId(0))..=(source, FormId(u32::MAX))).map(|(&(_, f), &r)| (f, r)).collect()
    }

    /// A hardcore stage's effect: SetHardcoreStageEffect's, else the record's.
    pub(crate) fn stage_spell(&self, stage: FormId, record: FormId) -> Option<FormId> {
        self.other.jip11.stage_effects.get(&stage).copied().unwrap_or(Some(record))
    }

    /// The radiation level at `r`'s place, rads a second before resistance: the
    /// radiation markers and zones there, as the player's (`crate::radiation`).
    fn rads_at(&self, r: FormId) -> f64 {
        let place = self.world.peek(r).map(|s| (s.cell, s.pos)).or_else(|| self.world.placed.get(r).map(|p| (p.cell, p.pos)));
        let Some((Some(cell), pos)) = place else { return 0.0 };
        let space = self.world.placed.world_of(cell).unwrap_or(cell);
        let list = self.radiation_markers_in(space);
        let world = &self.world;
        let (m, _) = crate::radiation::marker_level(&list, pos, |id| world.is_enabled(id).unwrap_or(true));
        let (z, _) = crate::radiation::zone_level(&self.senses.rad_zones, space, pos);
        m.max(z)
    }
}

/// The equipped stack's condition of an item reference (an inventory reference, or a
/// placed item): a share of its full health, 1 when it has none.
fn item_condition(g: &mut Game, r: FormId) -> (Option<FormId>, f32) {
    if let Some((c, item)) = g.temp_item(r) {
        let cond = g.world.inventory_mut(c).ok().and_then(|inv| inv.groups().find(|(i, _, _)| *i == item).map(|(_, e, _)| e.condition_or_whole())).unwrap_or(1.0);
        return (Some(item), cond);
    }
    let base = crate::functions::base_of(g, r);
    let cond = g
        .world
        .peek(r)
        .and_then(|s| s.extra.condition)
        .or_else(|| g.world.placed.get(r).and_then(|p| p.health))
        .unwrap_or(1.0);
    (base, cond)
}

/// The JIP batch-11 functions; `None` for any other name.
pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- combat and detection ----
        // The actors the game counts as allies of an actor in combat: those fighting a
        // foe it fights (`Game::combat_group`) [documented; hypothesis: NVForever's
        // combat group is the game's allies]; not in combat, no array.
        "getcombatallies" => {
            let Some(r) = subject.filter(|&r| g.awareness.actors.get(&r).is_some_and(|a| a.state.in_combat())) else { return Some(Ok(XVal::Array(0))) };
            let allies: Vec<FormId> = g.combat_group(r).into_iter().filter(|&a| a != r).collect();
            refs(g, allies)
        }
        // Its potential targets: the actors it has as targets now [documented].
        "getcombattargets" => {
            let targets: Vec<FormId> = subject.and_then(|r| g.awareness.actors.get(&r)).filter(|a| a.state.in_combat()).map(|a| a.targets.keys().copied().collect()).unwrap_or_default();
            refs(g, targets)
        }
        // The actors it detects: the detection rounds' pairs in the Detected band, as
        // GetDetected [documented; hypothesis: the same test].
        "getdetectedactors" => {
            let Some(r) = subject else { return Some(Ok(XVal::Array(0))) };
            let seen: Vec<FormId> = g.detection.pairs.iter().filter(|(&(d, t), (x, _))| d == r && t != r && x.band == crate::detection::Band::Detected).map(|(&(_, t), _)| t).collect();
            refs(g, seen)
        }
        // The latest detection value of the target by the calling actor, a whole number
        // [documented: int]; -100 for a pair not measured [documented: the value a left
        // cell sets; hypothesis: the same for one never measured].
        "getdetectionvalue" => {
            let (Some(r), Some(t)) = (subject, form(args, 0)) else { return Some(Ok(XVal::Num(-100.0))) };
            XVal::Num(g.detection.get(r, t).map_or(-100.0, |d| d.value.round()))
        }
        // `[value] [asModifier]`: none or 0 gives the detection back to the rules
        // [documented].
        "forceactordetectionvalue" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            match num(args, 0).filter(|&v| v != 0.0) {
                Some(v) => {
                    let modifier = num(args, 1).is_some_and(|m| m != 0.0);
                    g.other.jip11.forced_detection.insert(r, (v.round(), modifier));
                }
                None => {
                    g.other.jip11.forced_detection.remove(&r);
                }
            }
            zero
        }
        // ---- packages ----
        // Stuck: a package the actor could not reach is set aside now
        // (`Game::block_package`; see docs/spec-notes/jip-actors-11.md §1).
        "gettargetunreachable" => {
            let now = g.world.calendar().map(|c| c.days_passed).unwrap_or(0.0);
            yes(subject.and_then(|r| g.extra.get(&r)).is_some_and(|e| e.package_blocked.values().any(|&u| u > now)))
        }
        // The stable package: the one the actor's package list chose [documented].
        "getcurrentstablepackage" => {
            let Some(r) = subject else { return Some(Ok(XVal::Ref(0))) };
            reff(g.current_package(r))
        }
        // [decision: NVForever's interrupts (combat, dialogue, flight) are states, not
        // package forms; nothing to return until they are, spec note §6.]
        "getinterruptpackage" => XVal::Ref(0),
        // ---- the actor's traits ----
        // SetActorAlpha's alpha, 1 opaque [documented].
        "getactoralpha" => XVal::Num(f64::from(subject.and_then(|r| g.script_looks.alpha.get(&r).copied()).unwrap_or(1.0))),
        // The breath left, a full breath above water [observed: the jip11 probe, the
        // player on land read 10, the full breath at Endurance 5]; before the engine
        // has told it, the full breath (`Game::breath_rules`). The player's breath only:
        // NVForever's NPCs do not drown [decision: the drowning rule is the player's].
        "getactordivebreath" => {
            if subject != Some(FormId::PLAYER_REF) {
                return Some(Ok(zero));
            }
            let left = match g.other.jip11.player_breath {
                Some((left, _)) => left,
                None => g.breath_rules(FormId::PLAYER_REF).seconds,
            };
            XVal::Num(f64::from(left))
        }
        // Under water only [documented: "If the calling actor is underwater"].
        "setactordivebreath" => {
            if subject == Some(FormId::PLAYER_REF) && g.other.jip11.player_breath.is_some_and(|(_, under)| under) {
                if let Some(s) = num(args, 0) {
                    g.other.jip11.set_breath = Some(s.max(0.0) as f32);
                }
            }
            zero
        }
        // SetActorGravityMult's multiplier; 0 for an actor no script set [observed: the
        // jip11 probe, Doc Mitchell 0, though the page says 1.0 by default].
        "getactorgravitymult" => XVal::Num(subject.and_then(|r| g.gaps.gravity.get(&r).copied()).unwrap_or(0.0)),
        // The leveled list the actor was placed from: its reference's base is the list
        // [documented; observed: NVForever keeps the LVLN/LVLC as the base].
        "getactorleveledlist" => {
            let b = subject.and_then(|r| crate::functions::base_of(g, r));
            reff(b.filter(|&b| matches!(g.data.sig(b).as_ref(), Some(b"LVLN" | b"LVLC"))))
        }
        // The light at the actor as the last detection round measured it, 0-100
        // [documented: the range]; 0 for an actor not measured (see docs/spec-notes/jip-actors-11.md §2).
        "getactorlightamount" => XVal::Num(subject.and_then(|r| g.other.jip11.light.get(&r).copied()).unwrap_or(0.0)),
        // The movement flags [documented: the bit table]. Jumping, flying, falling and
        // sliding are not known game-side: 0 [decision: spec note §6].
        "getactormovementflags" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let m = g.motion(r);
            let mut bits = match g.senses.anim.get(&r).map(|a| a.moving) {
                Some(crate::anim_state::MoveDir::Forward) => 1,
                Some(crate::anim_state::MoveDir::Backward) => 2,
                Some(crate::anim_state::MoveDir::Left) => 4,
                Some(crate::anim_state::MoveDir::Right) => 8,
                _ => 0,
            };
            bits |= match m.turning {
                1 => 16,
                2 => 32,
                _ => 0,
            };
            if m.moving {
                bits |= if m.running { 512 } else { 256 };
            }
            if m.sneaking {
                bits |= 1024;
            }
            if m.swimming {
                bits |= 2048;
            }
            XVal::Num(f64::from(bits))
        }
        // `[baseForm]`: the unarmed attack effect of the base supplying the actor's
        // effects (EITM), SetActorUnarmedEffect's first [documented].
        "getactorunarmedeffect" => {
            let e = match form(args, 0) {
                Some(b) => {
                    let src = g.data.base_template_source(b, |d| d.template_spells);
                    g.unarmed_override(src).or_else(|| g.data.actors.get(&src).and_then(|d| d.unarmed_effect))
                }
                None => subject.and_then(|r| g.unarmed_effect(r)).map(|(e, _)| e),
            };
            reff(e)
        }
        // `effect [baseForm]`: an object effect or actor effect (anything else refused;
        // see docs/spec-notes/jip-actors-11.md §2); 1 when set [documented].
        "setactorunarmedeffect" => {
            let Some(e) = form(args, 0).filter(|&e| matches!(g.data.sig(e).as_ref(), Some(b"ENCH" | b"SPEL"))) else { return Some(Ok(zero)) };
            let Some(b) = form(args, 1).or_else(|| subject.and_then(|r| crate::functions::base_of(g, r))) else { return Some(Ok(zero)) };
            if !g.data.actors.contains_key(&b) {
                return Some(Ok(zero));
            }
            let src = g.data.base_template_source(b, |d| d.template_spells);
            g.other.jip11.unarmed.insert(src, e);
            XVal::Num(1.0)
        }
        // `x y z [local]`: the velocity on the world axes, or the actor's own (x right,
        // y forward) [documented; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
        // JIP LN 57.30, 2026-11-24, `w6_t1_05_velocities_jip`
        // (`tools/capture/nvse/catalogs/wave6_t1_jip_velocities.json`, `jip-actors-11.md`
        // §2): `act0=1(0.0,0.0,0.0)|act1=1(0.0,0.0,0.0)`]; the actor is followed from now
        // on, as GetActorVelocity follows it, so the first call reads 0 (see docs/spec-notes/jip-actors-11.md §2).
        // Note: `Vm::out_call` strips output params `[0, 1, 2]` before dispatch, so
        // optional `getLocal` arrives at `args[0]`.
        "getactorvelocityalt" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let Some((here, rot)) = g.ref_pos_rot(a) else { return Some(Ok(XVal::Num(0.0))) };
            let v = g.jip2.velocity.entry(a).or_insert((here, [0.0; 3])).1;
            let local = num(args, 0).is_some_and(|x| x != 0.0);
            let out = if local {
                let (s, c) = (f64::from(rot[2]).sin(), f64::from(rot[2]).cos());
                let (x, y) = (f64::from(v[0]), f64::from(v[1]));
                [x * c - y * s, x * s + y * c, f64::from(v[2])]
            } else {
                v.map(f64::from)
            };
            let entries = [1.0, out[0], out[1], out[2]].into_iter().enumerate().map(|(i, x)| (XKey::Num(i as f64), XVal::Num(x))).collect();
            XVal::Array(g.ext.new_array(ArrayKind::List, entries))
        }
        // `av [baseForm]`: the base value of the reference, or of the base form through
        // its first placed reference [hypothesis: NVForever computes base values on
        // references]; 0 for a base placed nowhere.
        "getbaseactorvaluealt" => {
            let Some(code) = av(args, 0) else { return Some(Ok(zero)) };
            let r = match form(args, 1) {
                Some(b) if g.world.peek(b).is_some() || b == FormId::PLAYER_REF => Some(b),
                Some(b) => g.world.placed.refs.iter().find(|(_, p)| p.base == b).map(|(&r, _)| r),
                None => subject,
            };
            XVal::Num(r.and_then(|r| g.base_actor_value(r, code).ok()).unwrap_or(0.0))
        }
        // The XP a kill of the actor gives, before the perks' adjustment [documented].
        "getkillxp" => XVal::Num(subject.map_or(0.0, |r| g.kill_xp(r))),
        // Rads a second at the actor, after its Rad Resistance: the player's as
        // GetRadiationLevel, an NPC's from the markers and zones at its place
        // [documented: any NPC; hypothesis: no water for NPCs].
        "getradiationlevelalt" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            if r == FormId::PLAYER_REF {
                XVal::Num(g.radiation().resisted)
            } else {
                let level = g.rads_at(r);
                let resist = if level > 0.0 { crate::functions::actor::actor_value(g, r, crate::actor_values::RAD_RESISTANCE).unwrap_or(0.0) } else { 0.0 };
                XVal::Num(crate::radiation::taken(level, resist, 1.0))
            }
        }
        // ---- falls ----
        // Estimated seconds until the falling actor hits the ground, 0 when grounded
        // [documented: GECK wiki GetFallTimeRemaining; docs/spec-notes/nvse-stubs-persistent-fall-spread-unique.md].
        "getfalltimeremaining" => XVal::Num(subject.map_or(0.0, |r| g.fall_time_remaining(r))),
        // The fall is measured again from where the actor is now [documented]; the
        // engine takes the request (`nvf_play::falls`).
        "resetfalltime" => {
            if let Some(r) = subject {
                g.other.jip11.fall_resets.insert(r);
            }
            zero
        }
        // ---- effects ----
        // `[filter]` 1 object effects, 2 actor effects, 4 ingestibles [documented]: the
        // magic items whose effects are on the actor, once each, in the order they took
        // [hypothesis: the elements are the items, not their base effects].
        "getactiveeffects" => {
            let Some(r) = subject else { return Some(Ok(XVal::Array(0))) };
            let filter = num(args, 0).map_or(7, |f| f as i64).max(0);
            let filter = if filter == 0 { 7 } else { filter };
            let mut out: Vec<FormId> = Vec::new();
            for e in g.effects.iter().filter(|e| e.target == r && !e.inactive) {
                let kind = match g.data.sig(e.source).as_ref() {
                    Some(b"ENCH") => 1,
                    Some(b"SPEL") => 2,
                    Some(b"ALCH" | b"INGR") => 4,
                    _ => 0,
                };
                if kind & filter != 0 && !out.contains(&e.source) {
                    out.push(e.source);
                }
            }
            refs(g, out)
        }
        // `item index`: the entry goes from the item's list; the new count, -1 when
        // there is none or it is the last one [documented].
        // The index is the scripts' list's (`Game::effect_slot`, #2016): AddNewEffect's
        // entries can go too.
        "removentheffect" => {
            use crate::nvse_jip14::EffectSlot;
            let (Some(f), Some(i)) = (form(args, 0), num(args, 1)) else { return Some(Ok(XVal::Num(-1.0))) };
            let count = g.effect_count(f);
            let Some(slot) = usize::try_from(i as i64).ok().and_then(|i| g.effect_slot(f, i)) else { return Some(Ok(XVal::Num(-1.0))) };
            if count < 2 {
                return Some(Ok(XVal::Num(-1.0)));
            }
            match slot {
                EffectSlot::Own(r) => {
                    g.other.jip11.removed_effects.entry(f).or_default().insert(r);
                }
                EffectSlot::Added { back, at } => g.remove_added_effect(f, back, at),
            }
            XVal::Num(g.effect_count(f) as f64)
        }
        // `index` into GetTempEffects' list: that effect is dispelled [documented].
        "removenthtempeffect" => {
            let (Some(r), Some(i)) = (subject, num(args, 0)) else { return Some(Ok(zero)) };
            let temp: Vec<u32> = g.effects.iter().filter(|e| e.target == r && !e.permanent && e.duration > 0.0).map(|e| e.id).collect();
            let Some(&id) = usize::try_from(i as i64).ok().and_then(|i| temp.get(i)) else { return Some(Ok(zero)) };
            yes(g.dispel(r, None, |e| e.id == id) > 0)
        }
        // ---- equipped items ----
        // The first of the list's items the actor has equipped [documented].
        "getequippedex" => {
            let (Some(r), Some(l)) = (subject, form(args, 0)) else { return Some(Ok(XVal::Ref(0))) };
            let on = g.world.equipment(r).unwrap_or_default();
            reff(crate::functions::refs::list_members(g, l).into_iter().find(|i| on.contains(i)))
        }
        // The poison JohnnyGuitar's ApplyWeaponPoison put on the equipped weapon
        // (`crate::nvse_jg10`); 0 for none.
        "getequippedweaponpoison" => XVal::Ref(subject.and_then(|r| g.weapon_poison(r)).map_or(0, |p| p.0)),
        // 1 scoped by default, 2 a scope mod on, 0 neither [documented]: DNAM "Has Scope"
        // without "Scope from Mod", or an Increase Zoom mod attached.
        "isequippedweaponscoped" | "isequippedweaponsilenced" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let Some(w) = crate::functions::items::equipped_weapon(g, r) else { return Some(Ok(zero)) };
            let mods = g.world.peek(r).and_then(|s| s.inventory.as_ref()).map_or(0, |inv| inv.equipped_mods(w));
            let modded = |want: &[&str]| (1..=3u8).any(|s| mods & (1 << (s - 1)) != 0 && mod_effect(g, w, s).is_some_and(|e| want.contains(&e.as_str())));
            let bits = |sub: &str| field_num(g, w, "DNAM", sub).map_or(0, |x| x as u64);
            let (default, by_mod) = if name == "isequippedweaponscoped" {
                (bits("flags1") & (1 << 2) != 0 && bits("flags2") & (1 << 13) == 0, modded(&["Increase Zoom"]))
            } else {
                // VNAM Sound Level 2 "Silent" [documented: the schema's SoundLevel].
                (field_num(g, w, "VNAM", "") == Some(2.0), modded(&["Silence", "Suppressor"]))
            };
            XVal::Num(if default { 1.0 } else if by_mod { 2.0 } else { 0.0 })
        }
        // The armour's biped slots against those of the armour the actor wears
        // [documented].
        "testequippedslots" => {
            let (Some(r), Some(a)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let slots = |g: &Game, f: FormId| field_num(g, f, "BMDT", "biped_flags").map_or(0, |x| x as u64);
            let want = slots(g, a);
            let on = g.world.equipment(r).unwrap_or_default();
            yes(want != 0 && on.into_iter().filter(|&i| g.is_armo(i)).any(|i| slots(g, i) & want != 0))
        }
        // On the inventory reference of an equipped item: the item stays on until a
        // script takes it off (the world's NoUnequip lock, saved) [documented].
        "setnounequip" => {
            let Some((actor, item)) = subject.and_then(|r| g.temp_item(r)) else { return Some(Ok(zero)) };
            let equipped = g.world.peek(actor).and_then(|s| s.inventory.as_ref()).is_some_and(|inv| inv.is_equipped(item));
            if !equipped {
                return Some(Ok(zero));
            }
            let lock = num(args, 0).is_some_and(|x| x != 0.0).then_some(fnv_sim::world::EquipLock::NoUnequip);
            if lock.is_some() || g.world.equip_lock(actor, item) == Some(fnv_sim::world::EquipLock::NoUnequip) {
                let _ = g.world.lock_equip(actor, item, lock);
            }
            zero
        }
        // ---- armour ----
        // DR or DT times the share condition leaves: all of it from half condition up,
        // below `1 − (0.5 − condition) × penalty` clamped at 0 (`Game::armor_condition_mult`:
        // SetArmorConditionPenalty's, else Stewie's `Tweaks::armor_condition_penalty`,
        // default 1) [documented: SetArmorConditionPenalty's formula; design:
        // docs/design/tweaks.md].
        "getarmoreffectivedr" | "getarmoreffectivedt" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let (Some(item), cond) = item_condition(g, r) else { return Some(Ok(zero)) };
            if !g.is_armo(item) {
                return Some(Ok(zero));
            }
            let value = field_num(g, item, "DNAM", if name == "getarmoreffectivedr" { "dr" } else { "dt" }).unwrap_or(0.0);
            XVal::Num(value * g.armor_condition_mult(cond))
        }
        // Whether the actor's last hit got past its DT [documented: in its OnHit].
        "getarmourpenetrated" => yes(subject.and_then(|r| g.other.jip11.penetrated.get(&r).copied()).unwrap_or(false)),
        // ---- factions ----
        // `faction rank [baseActor]`: the base's rank, for the session [documented];
        // a rank below 0 takes the base out (see docs/spec-notes/jip-actors-11.md §5).
        "setbasefactionrank" => {
            let (Some(f), Some(rank)) = (form(args, 0), num(args, 1)) else { return Some(Ok(zero)) };
            let Some(b) = form(args, 2).or_else(|| subject.and_then(|r| crate::functions::base_of(g, r))) else { return Some(Ok(zero)) };
            if !g.data.actors.contains_key(&b) || g.data.sig(f) != Some(*b"FACT") {
                return Some(Ok(zero));
            }
            let src = g.data.base_template_source(b, |d| d.template_factions);
            g.other.jip11.base_ranks.insert((src, f), (rank as i64).clamp(-1, 127) as i8);
            crate::functions::actor::forget_all_factions(g);
            zero
        }
        // ---- the dead ----
        // `[keepRespawning]`: every dead actor in the loaded cells is disabled, except
        // those whose base is a quest item, those carrying one, and (1) those that
        // respawn [documented].
        "cleardeadactors" => {
            let keep_respawn = num(args, 0).is_some_and(|x| x != 0.0);
            let dead: Vec<FormId> = g.extra.iter().filter(|(&r, e)| e.dead && r != FormId::PLAYER_REF).map(|(&r, _)| r).collect();
            for r in dead {
                let Some(s) = g.world.peek(r) else { continue };
                if !s.cell.is_some_and(|c| g.loaded_cells.contains(&c)) || !g.world.is_enabled(r).unwrap_or(false) {
                    continue;
                }
                let base = s.base;
                let carries = s.inventory.as_ref().is_some_and(|inv| inv.groups().any(|(i, _, _)| g.world.is_quest_item(i)));
                if g.world.is_quest_item(base) || carries || (keep_respawn && g.respawns(r)) {
                    continue;
                }
                let _ = crate::functions::refs::disable_ref(g, r, false);
            }
            zero
        }
        // ---- hardcore stages ----
        // A stage record's sickness effect (SetHardcoreStageEffect's first) [documented].
        "gethardcorestageeffect" => {
            let Some(st) = form(args, 0) else { return Some(Ok(XVal::Ref(0))) };
            let record = g.data.need_stages.values().flatten().find(|s| s.stage == st).map(|s| s.spell);
            reff(record.and_then(|sp| g.stage_spell(st, sp)))
        }
        // `stage [effect]`: omitted arg clears the stage's effect to None; explicit null 0
        // and non-SPEL forms are no-ops [observed: `w6d2_30_actor_unarmed_and_hardcore_stage_effect`];
        // the player gets the new one at the next change of stage (see docs/spec-notes/jip-actors-11.md §5).
        "sethardcorestageeffect" => {
            let Some(st) = form(args, 0).filter(|&st| g.data.need_stages.values().flatten().any(|s| s.stage == st)) else { return Some(Ok(zero)) };
            match args.get(1) {
                None => {
                    g.other.jip11.stage_effects.insert(st, None);
                }
                Some(_) => {
                    if let Some(e) = form(args, 1).filter(|&e| g.data.sig(g.unclone(e)) == Some(*b"SPEL")) {
                        g.other.jip11.stage_effects.insert(st, Some(e));
                    }
                }
            }
            zero
        }
        // The multiplier of the weapon damage penalty below 75 % condition [documented];
        // the engine's damage reads it (`Game::condition_damage_penalty`).
        "setconditiondamagepenalty" => {
            if let Some(m) = num(args, 0) {
                g.other.jip11.condition_penalty = Some(m.max(0.0) as f32);
            }
            zero
        }
        _ => return None,
    };
    Some(Ok(v))
}
