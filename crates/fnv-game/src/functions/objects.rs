//! Objects in the world: references created at run time, ownership, destruction,
//! regions, scale, headings and other per-reference state. docs/spec-notes/scripts.md
//! §7.6.

use super::{base_of, none, yes, Args, Ctx};
use crate::game::{Game, InstKey};
use crate::trace::EventKind;
use fnv_script::vm::Value;
use fnv_sim::FormId;

fn is_item(g: &Game, base: FormId) -> bool {
    g.world.forms.item(base).is_some()
}

/// Record a reference created at run time: traced, and its script (if any) runs from
/// the next frame like a placed one; in a loaded cell its OnLoad runs at the start of
/// the next frame, as after Enable [hypothesis].
pub fn note_created(g: &mut Game, r: FormId) {
    let (base, cell, pos) = match g.world.peek(r) {
        Some(s) => (s.base, s.cell, s.pos),
        None => return,
    };
    // An actor whose base stands for another's carried items (a leveled actor list, a
    // template's Inventory source) carries that actor's, as a placed one does
    // (`GameData::inventory_sources`): the pick follows the reference as the engine draws it.
    if let Some(source) = g.template_source(base, r, |d| d.template_inventory).map(|(s, _)| s).filter(|&s| s != base) {
        if let Err(e) = g.world.give_items_of(r, source) {
            tracing::warn!("created {r}: the items of {source}: {e}");
        }
    }
    g.maybe_apply_npc_weapon_mods(r);
    g.emit(EventKind::Placed { reference: r, base, cell, pos });
    if g.ref_script(r).is_some() {
        g.moved.insert(r);
        g.note_enabled(r);
    } else if g.handled("onload") {
        g.note_enabled(r);
    }
}

impl Game<'_> {
    /// Explicit preview admission for a direct non-actor scene clone. Preserve its
    /// alias identity and pre-placement model for both drawing and collision.
    fn place_scene_base(&mut self, base: FormId, cell: Option<FormId>, pos: [f32; 3], rot: [f32; 3], count: i32) -> Result<FormId, String> {
        let Some(original) = self.clones.get(&base).copied() else {
            return self.world.place(base, cell, pos, rot, count).map_err(|e| e.to_string());
        };
        if self.clones.contains_key(&original) {
            return Err("placing a clone of a temporary clone is not supported yet".into());
        }
        if self.base_script(base).is_some() || self.base_script(original).is_some() {
            return Err("placing a temporary scene base with an attached script is not supported yet".into());
        }
        // Pre-clone original `MODL/` overrides are snapshotted onto `base` at
        // `TempCloneForm` creation (`Game::snapshot_model_path`), so read only `base`'s
        // own `MODL/` entry and never live-inherit post-clone original edits.
        // Preserve explicit empty as distinct from absent authored metadata.
        let model_override = match self.field_overrides.get(&(base, "MODL/".into())) {
            Some(fnv_script::vm::XVal::Str(path)) => Some(path.clone()),
            _ => None,
        };
        let descriptor = fnv_sim::world::TemporarySceneBase { original, model_override };
        self.world.place_temporary_scene_base(base, descriptor, cell, pos, rot, count).map_err(|e| e.to_string())
    }

    /// [`Game::create_at`] under the FormID `id` (a co-op guest's copy of an actor the
    /// host's game created, numbered as the host numbers it: `World::place_as`), its
    /// leveled lists read at `level` (the host's), so its pick, drawn look and carried
    /// items are the host's: they are seeded by the reference [design].
    pub fn create_as(&mut self, id: FormId, base: FormId, cell: Option<FormId>, pos: [f32; 3], rot: [f32; 3], level: Option<fnv_sim::zones::LevelAt>) -> Result<FormId, String> {
        let r = self.world.place_as(id, base, cell, pos, rot).map_err(|e| e.to_string())?;
        if let Some(at) = level {
            self.world.set_level(r, at);
        }
        note_created(self, r);
        Ok(r)
    }

    /// A new reference of `base` at `pos` in `cell` (with angles `rot`), made and announced
    /// as PlaceAtMe makes its references ([`place_at_me`]), at a place the caller chooses:
    /// the stress tests' battles (nvf-play `stress.rs`) put fighters where no reference
    /// stands. A created actor (or leveled actor list) becomes a resident as PlaceAtMe's do.
    pub fn create_at(&mut self, base: FormId, cell: Option<FormId>, pos: [f32; 3], rot: [f32; 3]) -> Result<FormId, String> {
        let r = self.place_scene_base(base, cell, pos, rot, 1)?;
        note_created(self, r);
        Ok(r)
    }
}

/// `ref.PlaceAtMe object count [distance direction]`: new references of `object` at
/// the calling reference's cell and position, with its angles [documented: catalogue
/// notes, the copy of the caller's angles]. The distance and direction do nothing and
/// the object lands near the caller [documented bug]; here exactly at the caller's
/// position [hypothesis]. An inventory item makes one reference holding the count;
/// any other form makes `count` references [hypothesis]. A leveled actor list is placed
/// as the list itself (resolving it into an actor is Layer 6 actor spawning). Returns
/// the last reference made.
pub fn place_at_me(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let at = ctx.subject_or_player();
    let base = a.form(0)?;
    let count = a.num_or(1, 1.0).max(1.0) as i32;
    let s = g.world.get(at).map_err(|e| e.to_string())?;
    let (cell, pos, mut rot) = (s.cell, s.pos, s.rot);
    // The player's X angle is its view's pitch (`refs::get_angle`): what is placed at it
    // stands upright [hypothesis: the pitch was never written before 2026-10-19].
    if at == FormId::PLAYER_REF {
        rot = [0.0, 0.0, rot[2]];
    }
    let (refs, each) = if is_item(g, base) { (1, count) } else { (count, 1) };
    // A placed explosion goes off where it is placed [observed: jip-hits and showoff1, a
    // MineFragExplosion PlaceAtMe'd at the player hit the player; hypothesis: that it is
    // nobody's blast].
    let explodes = g.data.sig(base) == Some(*b"EXPL");
    let mut last = None;
    for _ in 0..refs {
        let r = g.place_scene_base(base, cell, pos, rot, each)?;
        note_created(g, r);
        if explodes {
            g.combat_requests.push(crate::combat::CombatRequest::Explode { explosion: base, at: pos, source: Some(at) });
        }
        last = Some(r);
    }
    Ok(last.map(|r| Value::Ref(r.0)).unwrap_or(Value::NONE))
}

/// `PlaceAtReticle object count [minDistance] [maxDistance]`: places the object at the
/// spot the crosshair points at [documented: GECK wiki PlaceAtReticle; minDistance is
/// marked unused there and is ignored]. The spot is where the crosshair's ray (told by the
/// engine each frame, [`super::senses::Reticle`]) first hits the world. Made as PlaceAtMe
/// makes references (an item is one stack of `count`, anything else `count` references),
/// in the player's cell, turned to the player's heading, and returned [hypothesis: the
/// heading and the cell]. maxDistance is clamped to 1-50000 [documented: the wiki's
/// range]; left out, 50000 [hypothesis]. Nothing hit within it, or no crosshair told (a
/// headless run): nothing is placed and 0 is returned [hypothesis].
pub fn place_at_reticle(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let base = a.form(0)?;
    let count = a.num_or(1, 1.0).max(1.0) as i32;
    let max = (a.num_or(3, super::senses::RETICLE_REACH as f64) as f32).clamp(1.0, super::senses::RETICLE_REACH);
    let Some(pos) = g.senses.reticle.and_then(|r| r.point(max)) else {
        tracing::info!(base = %g.data.name(base), "PlaceAtReticle: the crosshair hits nothing within {max}");
        return Ok(Value::NONE);
    };
    let s = g.world.get(FormId::PLAYER_REF).map_err(|e| e.to_string())?;
    let (cell, rot) = (s.cell, [0.0, 0.0, s.rot[2]]);
    let (refs, each) = if is_item(g, base) { (1, count) } else { (count, 1) };
    let mut last = None;
    for _ in 0..refs {
        let r = g.place_scene_base(base, cell, pos, rot, each)?;
        note_created(g, r);
        last = Some(r);
    }
    Ok(last.map(|r| Value::Ref(r.0)).unwrap_or(Value::NONE))
}

/// `DropMe`: the calling inventory item leaves its container and lies in the world
/// beside it; it acts as a return; OnDrop does not run [documented: catalogue notes of
/// DropMe]. Placement as a dropped item (fnv-sim `drop_item`).
pub fn drop_me(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let Some(key @ InstKey::Item { holder, item }) = ctx.inst else {
        return Err("DropMe outside the script of an inventory item".into());
    };
    let made = g.world.drop_item_fresh(holder, item, 1).map_err(|e| e.to_string())?;
    if !made.is_empty() {
        g.emit(EventKind::ItemRemoved { from: holder, item, count: 1, silent: false });
    }
    for r in made {
        note_created(g, r);
    }
    if g.world.item_count(holder, item).unwrap_or(0) == 0 {
        g.instances.remove(&key);
    }
    g.request_return();
    none()
}

/// `actor.Drop item count`: the items fall at the actor's feet (fnv-sim's drop rule).
/// OnDrop is not run [documented: catalogue notes, for the player; hypothesis for other
/// actors].
pub fn drop(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let actor = ctx.subject()?;
    let item = a.form(0)?;
    let count = a.num_or(1, 1.0).max(1.0) as i32;
    let n = g.world.item_count(actor, item).map_err(|e| e.to_string())?.min(count);
    if n <= 0 {
        return none();
    }
    let made = g.world.drop_item(actor, item, n).map_err(|e| e.to_string())?;
    g.emit(EventKind::ItemRemoved { from: actor, item, count: n, silent: false });
    for r in made {
        note_created(g, r);
        // The extender's OnDrop: the actor, then the dropped reference [observed:
        // nvse-events, (player, the stimpak's reference)].
        g.dispatch_event("OnDrop", vec![fnv_script::vm::XVal::Ref(actor.0), fnv_script::vm::XVal::Ref(r.0)], Some(actor));
    }
    none()
}

// ---- ownership -------------------------------------------------------------------

/// `ref.SetOwnership [owner]`: the reference's owner (an NPC or faction base), the
/// player when none is given [documented: catalogue summary]. Kept in the Layer 4
/// reference state (`extra.owner`).
pub fn set_ownership(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let owner = a.opt_form(0).unwrap_or(FormId::PLAYER_BASE);
    g.world.get(r).map_err(|e| e.to_string())?.extra.owner = Some(owner);
    g.showoff.temp_owners.remove(&r);
    none()
}

pub fn clear_ownership(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.world.get(r).map_err(|e| e.to_string())?.extra.owner = None;
    g.showoff.temp_owners.remove(&r);
    none()
}

/// `ref.IsOwner [owner]`: the reference's own owner only (not the cell's); 0 with no
/// owner; the player by default [documented: catalogue notes].
pub fn is_owner(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    // An actor reference stands for its base, as owners are stored [documented: GECK
    // wiki IsOwner, parameter ActorRefID, example `SpecialRef.IsOwner SaraLyonsRef`].
    let want = match a.opt_form(0) {
        Some(FormId::PLAYER_REF) | None => FormId::PLAYER_BASE,
        Some(f) if matches!(super::base_of(g, f).and_then(|b| g.data.sig(b)).as_ref(), Some(b"NPC_") | Some(b"CREA")) => super::base_of(g, f).unwrap_or(f),
        Some(f) => f,
    };
    let owner = g.world.get(r).map_err(|e| e.to_string())?.extra.owner;
    yes(owner == Some(want))
}

/// `SetCellOwnership cell [owner]`, the player by default [documented: catalogue].
pub fn set_cell_ownership(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let cell = a.form(0)?;
    let owner = a.opt_form(1).unwrap_or(FormId::PLAYER_BASE);
    g.forms.cell_owners.insert(cell, owner);
    g.showoff.temp_owners.remove(&cell);
    none()
}

pub fn set_cell_public_flag(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let cell = a.form(0)?;
    g.forms.public_cells.insert(cell, a.num_or(1, 1.0) != 0.0);
    none()
}

// ---- destruction ------------------------------------------------------------------

/// The destruction stage reached after `damage` points: the highest stage index whose
/// health percent the remaining health has fallen to, or 0 when damaged above every
/// stage; −1 without destruction data or damage [documented: GECK wiki
/// GetDestructionStage for −1; hypothesis for the rest].
pub fn destruction_stage(g: &Game, r: FormId) -> i32 {
    let Some(def) = base_of(g, r).and_then(|b| g.destruction(b)) else { return -1 };
    let Some(d) = g.extra.get(&r).and_then(|e| e.damage) else { return -1 };
    if def.health <= 0 {
        return def.stages.iter().map(|s| s.index as i32).max().unwrap_or(0);
    }
    def.stages.iter().filter(|s| crate::combat::destruction_threshold_reached(d, def.health as f64, s.health_percent)).map(|s| s.index as i32).max().unwrap_or(0)
}

/// `ref.DamageObject amount`: object damage on a destructible, not destroyed
/// reference [documented: GECK wiki DamageObject]. Reaching a stage with the Destroy
/// flag marks it destroyed and one with the Disable flag disables it; a stage change
/// runs OnDestructionStageChange [hypothesis: as the stage flags' names say].
pub fn damage_object(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let amount = a.num(0)?;
    if !g.is_destructible(r) {
        return Err(format!("{} has no destruction data", g.data.name(r)));
    }
    // The stages' explosions and models are the engine's: the change waits in
    // `Game::destruction_changes`, as a hit's goes to the engine directly.
    if let Some(c) = g.damage_object(r, amount) {
        if c.after != c.before {
            g.destruction_changes.push((r, c));
        }
    }
    none()
}

/// Disable a reference as `Disable` does (a destruction stage's Disable flag).
pub(crate) fn disable_ref(g: &mut Game, r: FormId) {
    let ctx = Ctx { unit: "destruction".into(), opcode: 0, on: Some(r), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    let _ = super::refs::disable(g, &ctx, &Args(&[]));
}

pub fn get_destruction_stage(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(Value::Num(script_destruction_stage(g, r) as f64))
}

/// What GetDestructionStage gives: the stage reached counted from 1 (DSTD index + 1), 0
/// when damaged above every stage, −1 without destruction data or damage [documented −1:
/// GECK wiki GetDestructionStage, "the current Damage Stage"; hypothesis for the counting,
/// #3053: the masters' scripts compare it with values only a count from 1 reaches, Zeta's
/// generator core (eight stages, index 0-7) disabling itself at 7, its Destroy stage, and
/// testing 8; Point Lookout's Brain (seven stages) bursting at 7; DLC04DarMuckBubble (two
/// stages) testing `>= 2`. The stage's own DSTD damage-stage field fits every case found too;
/// observer check 247 tells them apart]. The rules' own stage numbers stay indices
/// ([`destruction_stage`]), as OnDestructionStageChange's parameter is index + 1 already.
pub fn script_destruction_stage(g: &Game, r: FormId) -> i32 {
    let Some(def) = base_of(g, r).and_then(|b| g.destruction(b)) else { return -1 };
    let Some(d) = g.extra.get(&r).and_then(|e| e.damage) else { return -1 };
    let reached = if def.health <= 0 {
        def.stages.iter().map(|s| i32::from(s.index)).max()
    } else {
        def.stages.iter().filter(|s| crate::combat::destruction_threshold_reached(d, def.health as f64, s.health_percent)).map(|s| i32::from(s.index)).max()
    };
    reached.map_or(0, |i| i + 1)
}

/// Health, stages and the destroyed state back to their defaults [documented: catalogue
/// summary of ClearDestruction].
pub fn clear_destruction(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if let Some(e) = g.extra.get_mut(&r) {
        e.damage = None;
        e.destroyed = false;
    }
    none()
}

// ---- regions -------------------------------------------------------------------------

/// Whether the player stands in a region: in an exterior cell of the region's
/// worldspace, inside one of its areas; never in an interior [documented: GECK wiki
/// IsPlayerInRegion]. A child region's area lies within its parent's, so both test
/// true [documented: same page; hypothesis that the data draws them that way].
pub fn player_in_region(g: &Game, region: FormId) -> Result<bool, String> {
    let def = g.data.regions.get(&region).ok_or_else(|| format!("{} is not a region", g.data.name(region)))?;
    let p = FormId::PLAYER_REF;
    let Some(cell) = g.cell_of(p) else { return Ok(false) };
    let Some(world) = g.world.placed.world_of(cell) else { return Ok(false) };
    if def.world.is_some_and(|w| w != world) {
        return Ok(false);
    }
    let pos = g.world.peek(p).map(|s| s.pos).unwrap_or_default();
    Ok(def.contains(pos[0], pos[1]))
}

pub fn is_player_in_region(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    yes(player_in_region(g, a.form(0)?)?)
}

// ---- other reference state ------------------------------------------------------------

/// `ref.GetHeadingAngle target`: degrees from the caller's facing to the target,
/// clockwise positive, in (−180, 180]; 0 for a caller that is not an actor
/// [documented: catalogue summary and notes]. A z angle turns clockwise from +Y
/// (world.md §4).
pub fn get_heading_angle(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let t = a.form(0)?;
    if r != FormId::PLAYER_REF && !matches!(base_of(g, r).and_then(|b| g.data.sig(b)).as_ref(), Some(b"NPC_") | Some(b"CREA")) {
        return none();
    }
    let me = g.world.get(r).map_err(|e| e.to_string())?.clone();
    let to = g.world.get(t).map_err(|e| e.to_string())?.pos;
    let (dx, dy) = ((to[0] - me.pos[0]) as f64, (to[1] - me.pos[1]) as f64);
    let mut d = dx.atan2(dy).to_degrees() - (me.rot[2] as f64).to_degrees();
    while d > 180.0 {
        d -= 360.0;
    }
    while d <= -180.0 {
        d += 360.0;
    }
    Ok(Value::Num(d))
}

/// 3D is loaded while the reference is in the world, enabled and in a loaded cell
/// [hypothesis].
pub fn has_loaded_3d(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.present(r) && g.cell_of(r).is_some_and(|c| g.loaded_cells.contains(&c)))
}

/// The reference's scale: SetScale's, else its placement's (XSCL), else 1
/// [documented: catalogue summary of GetScale].
pub fn get_scale(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    Ok(Value::Num(scale_of(g, ctx.subject()?)))
}

/// A reference's scale as GetScale reads it: SetScale's (on the reference's state; a save
/// from before 2026-10-18 kept it in `ExtraState::scale`), else its placement's, else 1.
pub fn scale_of(g: &Game, r: FormId) -> f64 {
    let set = g.world.peek(r).and_then(|s| s.scale).map(f64::from).or_else(|| g.extra.get(&r).and_then(|e| e.scale));
    set.or_else(|| g.world.placed.get(r).and_then(|p| p.scale).map(f64::from)).unwrap_or(1.0)
}

/// `SetScale scale`: the reference is drawn, and collides, at `scale` in place of its
/// placement's (docs/spec-notes/misc-stats.md §5). The scale is kept on the reference's
/// state, which saves keep; the reference joins [`Game::turned`] so the engine redraws it
/// this frame, and a `Scaled` event makes its collision follow.
pub fn set_scale(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let s = a.num(0)?.max(0.0);
    g.world.get(r).map_err(|e| e.to_string())?.scale = Some(s as f32);
    if let Some(e) = g.extra.get_mut(&r) {
        e.scale = None;
    }
    g.turned.insert(r);
    g.emit(crate::trace::EventKind::Scaled { reference: r, scale: s });
    none()
}

/// The lock level of a door or container (0, 25, 50, 75, 100, 255) [documented:
/// catalogue notes]; a load door without lock data of its own reads its other side's
/// [observed, function probes 2026-10-17: the gas station's outer door has no XLOC and read
/// 255, the level of the XLOC on the door inside it links to, loaded or not]; 0 without a
/// lock [observed: a crate, a sack, VHostetlerRoomDoorREF]. A terminal reads its own lock
/// (`crate::terminal::lock_level`) while its cell is loaded, else 0 [observed].
pub fn get_lock_level(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if super::refs::is_terminal(g, r) {
        let l = if super::refs::in_loaded_cell(g, r) { crate::terminal::lock_level(g, r) } else { 0 };
        return Ok(Value::Num(l as f64));
    }
    g.world.get(r).map_err(|e| e.to_string())?;
    let l = g.world.lock_of(r).map_or(0, |l| l.level);
    Ok(Value::Num(l as f64))
}

/// `GetInCellParam cell actor`: GetInCell for the given actor [documented: catalogue].
pub fn get_in_cell_param(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let actor = a.form(1)?;
    let c = Ctx { on: Some(actor), ..ctx.clone() };
    super::refs::get_in_cell(g, &c, &Args(&a.0[..1]))
}

/// The container holding the calling inventory item [documented: catalogue summary].
pub fn get_container(_g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    Ok(match ctx.inst {
        Some(InstKey::Item { holder, .. }) => Value::Ref(holder.0),
        _ => Value::NONE,
    })
}

/// `SetMinimalUse flag` on a door, kept for the engine.
pub fn set_minimal_use(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.extra.entry(r).or_default().minimal_use = Some(a.num_or(0, 1.0) != 0.0);
    none()
}

/// `SetCriticalStage` code of GooEnd (0 none, 1 goo start, 2 goo end, 3 disintegrate
/// start, 4 disintegrate end; scripts.md §2.4).
pub const CRITICAL_GOO_END: u16 = 2;
/// `SetCriticalStage` code of DisintegrateEnd.
pub const CRITICAL_DISINTEGRATE_END: u16 = 4;

/// Whether a critical stage culls its reference: GooEnd and DisintegrateEnd "cull its
/// 3D, and remove its Havok data from the world" [documented: GECK wiki
/// SetCriticalStage]. The ragdoll changes of GooStart / DisintegrateStart are not built.
pub fn critical_stage_culls(stage: u16) -> bool {
    matches!(stage, CRITICAL_GOO_END | CRITICAL_DISINTEGRATE_END)
}

/// `SetCriticalStage stage` (codes above): kept for IsInCriticalStage (and saved with the
/// reference's extra state). A stage that culls the reference ([`critical_stage_culls`])
/// is listed for the engine (`ScriptLooks::critical_changed`), which stops drawing it and
/// drops its collision and ragdoll (`Game::culled`); the reference stays enabled
/// (GetDisabled 0) [hypothesis: the wiki speaks only of its 3D and Havok data].
pub fn set_critical_stage(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let stage = a.code(0)?;
    let e = g.extra.entry(r).or_default();
    let was = critical_stage_culls(e.critical_stage);
    e.critical_stage = stage;
    if was != critical_stage_culls(stage) {
        g.script_looks.critical_changed.push(r);
    }
    none()
}

/// `actor.AttachAshPile [type]`: the ash (1, or left out) or goo (2) pile is placed at
/// the actor (`Game::attach_ash_pile`, docs/spec-notes/ash-piles.md §3).
pub fn attach_ash_pile(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let kind = a.num_or(0, 1.0) as i64;
    g.attach_ash_pile(r, kind)?;
    none()
}

pub fn is_in_critical_stage(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.extra.get(&r).map(|e| e.critical_stage).unwrap_or(0) == a.code(0)?)
}

/// `SetBroadcastState state` on a radio station (talking activator) [documented:
/// catalogue summary]; the station's sound is the engine's.
pub fn set_broadcast_state(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.extra.entry(r).or_default().broadcast = Some(a.num(0)? != 0.0);
    none()
}

/// `SetTalkingActivatorActor [actor]`: kept for Layer 6 dialogue (none: back to the
/// activator's own) [documented: catalogue summary].
pub fn set_talking_activator_actor(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let e = g.extra.entry(r).or_default();
    e.talking_actor = a.opt_form(0);
    e.talking_actor_set = true;
    none()
}

/// `ref.PlaceLeveledActorAtMe list [level zone]`: one reference of the leveled actor
/// list at the caller, as PlaceAtMe; the level modifier and encounter zone belong to
/// Layer 6 actor spawning [documented: catalogue summary; hypothesis: the list is
/// resolved into an actor there].
pub fn place_leveled_actor_at_me(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    place_at_me(g, ctx, &Args(&a.0[..1.min(a.len())]))
}
