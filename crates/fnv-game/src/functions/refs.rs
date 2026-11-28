//! Reference functions: enable state, position, identity, locks, activation, lists.

use super::{base_of, none, yes, Args, Ctx};
use crate::game::{ActMode, Game};
use crate::trace::EventKind;
use fnv_script::vm::{ArgValue, Value, XVal};
use fnv_sim::FormId;

/// `ref.Enable [FadeIn]`: the reference is drawn and its actor's AI runs again; it fades
/// in unless FadeIn is 0 [documented: GECK wiki Enable, "Default = 1"]. The fade is the
/// engine's (`nvf_play::fade`); GetDisabled reads 0 at once [observed: capture
/// `disable_fade_getdisabled`, 2026-10-18, the game loop after Enable]. An Enable while a
/// `Disable 1` holds the reference ([`FADE_OUT_HOLD`]) ends the hold and fades it back.
pub fn enable(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if has_enable_parent(g, r) {
        return none();
    }
    enable_ref(g, r, a.num_or(0, 1.0) != 0.0)
}

/// Enable `r` (its own flag), fading in or not: Enable without the enable-parent test
/// (ShowOff's EnableAlt, `nvse_showoff4`).
pub fn enable_ref(g: &mut Game, r: FormId, fade: bool) -> Result<Value, String> {
    if g.script_looks.fading_out.remove(&r).is_some() {
        g.emit(EventKind::RefEnabled { reference: r, fade });
        return none();
    }
    let was = g.world.get(r).map_err(|e| e.to_string())?.enabled();
    if !was {
        if !g.enable_event_active.insert(r) {
            return none();
        }
        // The extender's OnEnable fires PRE-update (GetDisabled still 1) with null
        // calling reference (GetSelfAlt == 0) and args (ref, 0) [observed: 2026-11-24,
        // Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, JIP LN 57.30, GSDocMitchellHouse
        // (0x00103DF9), 10 official base masters,
        // `tools/capture/nvse/catalogs/wave6d1_on_enable_disable_pre_update.json` probe
        // `w6d1p2_07_on_enable_tick`:
        // `PROBE|w6d1p2_07_on_enable_tick|dis=1->0|en(n1=1,n2=1,s0=1,sR=0,a1R=1,a2z=1,inDis=1)`;
        // no DLL, PDB, or third-party source consulted].
        g.dispatch_event("OnEnable", vec![fnv_script::vm::XVal::Ref(r.0), fnv_script::vm::XVal::Num(0.0)], None);
        g.enable_event_active.remove(&r);
    }
    let still_disabled = !g.world.get(r).map_err(|e| e.to_string())?.enabled();
    g.world.set_enabled(r, true).map_err(|e| e.to_string())?;
    if still_disabled {
        g.emit(EventKind::RefEnabled { reference: r, fade });
        g.note_enabled(r);
    }
    none()
}

/// How long a `Disable 1` leaves its reference enabled (GetDisabled 0) while it fades
/// out, seconds; then it is disabled [observed: with the player 110 units away facing the
/// Prospector Saloon's terminal, GetDisabled turned 1 after 111 game loops, 1.96–1.97 s
/// of the game's clock, capture `disable_fade_long`, 2026-10-18; a plain Disable read 1
/// the next loop, capture `disable_fade_getdisabled`]. That capture's own `Disable 1`,
/// from the cell's arrival spot, read 0 for all 240 loops sampled (about 3.5 s): not
/// explained [hypothesis: the original fades only what it draws, and the terminal was
/// not in view]; here the hold is the same wherever the player looks.
pub const FADE_OUT_HOLD: f64 = 2.0;

/// `ref.Disable [FadeOut]`: the reference is no longer drawn; it fades out only when
/// FadeOut is non-zero [documented: GECK wiki Disable, "Default = 0"]. A plain Disable
/// reads GetDisabled 1 at once [observed]; with the fade the reference stays enabled for
/// [`FADE_OUT_HOLD`] while the engine fades it out, then is disabled (`Game::tick`,
/// `ScriptLooks::disabled_after_fade`). A plain Disable during the hold disables at once.
pub fn disable(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if has_enable_parent(g, r) {
        return none();
    }
    disable_ref(g, r, a.num_or(0, 0.0) != 0.0)
}

/// Disable `r` (its own flag), fading out or not: Disable without the enable-parent test
/// (ShowOff's DisableAlt, `nvse_showoff4`).
///
/// The player is never disabled: a script's Disable of the player does nothing
/// [hypothesis: VHDIntakeTowerLegionaryDespawnScript disables whoever enters its trigger,
/// the player too, and the original's Legion route still takes the player through the
/// Power Plant 01 elevator, whose bare `Activate` a disabled actor cannot make;
/// docs/spec-notes/doors.md §6, #2617, observer check 172].
pub fn disable_ref(g: &mut Game, r: FormId, fade: bool) -> Result<Value, String> {
    if r == FormId::PLAYER_REF {
        tracing::info!("script: Disable of the player ignored: the player stays enabled (doors.md §6)");
        return none();
    }
    // A dead actor disabled forgets when it died (GetTimeDead).
    if let Some(e) = g.extra.get_mut(&r) {
        e.died_day = None;
    }
    let held = g.script_looks.fading_out.contains_key(&r);
    if fade && held {
        return none();
    }
    if fade && g.world.get(r).map_err(|e| e.to_string())?.enabled() {
        g.script_looks.fading_out.insert(r, g.time + FADE_OUT_HOLD);
        g.emit(EventKind::RefDisabled { reference: r, fade });
        return none();
    }
    g.script_looks.fading_out.remove(&r);
    let was = g.world.get(r).map_err(|e| e.to_string())?.enabled();
    if was {
        if !g.enable_event_active.insert(r) {
            return none();
        }
        // The extender's OnDisable fires PRE-update (GetDisabled still 0) with
        // GetSelfAlt != ref and args (ref, 0) [observed: 2026-11-24, Fallout: New
        // Vegas 1.4.0.525, xNVSE 6.4.9, JIP LN 57.30, ShowOffNVSE 184,
        // GSDocMitchellHouse (0x00103DF9), 10 official base masters,
        // `tools/capture/nvse/catalogs/wave6d1_on_enable_disable_pre_update.json` probe
        // `w6d1_07_on_enable_on_disable_args`:
        // `PROBE|w6d1_07_on_enable_on_disable_args|dis(n1=1,n2=1,sOk=0,a1Ok=1,a2z=1,inDis=0)|en(n1=0,sOk=0,a1Ok=0,inDis=0)|par(d=0,da=1,ea=0)`;
        // no DLL, PDB, or third-party source consulted]; passing `None` (`GetSelfAlt == 0`)
        // matches `OnEnable` (`s0=1`) [hypothesis].
        g.dispatch_event("OnDisable", vec![fnv_script::vm::XVal::Ref(r.0), fnv_script::vm::XVal::Num(0.0)], None);
        g.enable_event_active.remove(&r);
    }
    let still_enabled = g.world.get(r).map_err(|e| e.to_string())?.enabled();
    g.world.set_enabled(r, false).map_err(|e| e.to_string())?;
    if still_enabled {
        g.emit(EventKind::RefDisabled { reference: r, fade });
    }
    none()
}

/// A reference whose enable state is its enable parent's: Enable and Disable called on
/// it have no effect [documented: GECK wiki Enable and Disable notes, "If Enable is
/// called on a reference with an Enable Parent, it will have no effect"]. A parent that
/// does not exist leaves the reference its own state (`World::is_enabled`), so it does
/// not count.
fn has_enable_parent(g: &Game, r: FormId) -> bool {
    !g.world.keeps_own_enable(r) && g.world.enable_parent(r).is_some_and(|ep| g.world.is_enabled(ep.reference).is_ok())
}

pub fn get_disabled(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(!g.world.is_enabled(r).map_err(|e| e.to_string())?)
}

/// The current cell, position and rotation of a reference.
type Place = (Option<FormId>, [f32; 3], [f32; 3]);

fn place(g: &mut Game, r: FormId) -> Result<Place, String> {
    let s = g.world.get(r).map_err(|e| e.to_string())?;
    Ok((s.cell, s.pos, s.rot))
}

/// The offsets of a MoveTo call (its optional x y z after the marker).
fn move_offsets(a: &Args) -> [f64; 3] {
    [a.num_or(1, 0.0), a.num_or(2, 0.0), a.num_or(3, 0.0)]
}

/// `MoveTo marker [x y z]`: the calling reference goes to the marker's cell and position
/// plus the offsets. Moving the player loads the destination's cells.
pub fn move_to(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let marker = a.form(0)?;
    move_ref(g, r, marker, move_offsets(a))?;
    none()
}

/// `ref.MoveToMarkerWithFade marker [x y z]`: "fades the object out, moves it to the
/// reference and then fades it back in"; it "has to wait for the fade out effect to
/// finish" [documented: GECK wiki MoveToMarkerWithFade]. The reference stays where it is
/// for [`FADE_OUT_HOLD`] while the engine fades it out (`ScriptLooks::fades`), then
/// `Game::tick` makes the move ([`move_ref`], to where the marker is then) and the engine
/// fades it in [hypothesis: the fade-out length is the `Disable 1` one, not measured for
/// this function; capture asked]. A second call during the wait changes the destination
/// and keeps the wait. The player, and a reference that is not drawn (disabled), move at
/// once as by MoveTo [hypothesis: the wiki speaks of the object's fade only].
pub fn move_to_marker_with_fade(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let marker = a.form(0)?;
    let offset = move_offsets(a);
    place(g, marker)?;
    if r == FormId::PLAYER_REF || !g.present(r) {
        move_ref(g, r, marker, offset)?;
        return none();
    }
    start_fade_move(g, r, marker, offset);
    none()
}

/// The fade out and the wait of a [`move_to_marker_with_fade`] move (also JIP's
/// MoveToFadeDelay, `crate::nvse_jip13`); the player and a reference not drawn move at once.
pub(crate) fn start_fade_move(g: &mut Game, r: FormId, marker: FormId, offset: [f64; 3]) {
    if r == FormId::PLAYER_REF || !g.present(r) {
        let _ = move_ref(g, r, marker, offset);
        return;
    }
    let at = match g.script_looks.fade_moves.get(&r) {
        Some(m) => m.at,
        None => {
            g.script_looks.fades.push((r, false));
            g.time + FADE_OUT_HOLD
        }
    };
    g.script_looks.fade_moves.insert(r, super::misc::FadeMove { marker, offset, at });
}

/// Put `r` at `pos` in `cell` (with `angles` in radians, else its own), told as MoveTo
/// tells a move ([`move_ref`]); JIP's moves and rotations (`crate::nvse_jip13`).
pub(crate) fn put_ref(g: &mut Game, r: FormId, cell: Option<FormId>, pos: [f32; 3], angles: Option<[f32; 3]>) {
    let from = (r == FormId::PLAYER_REF).then(|| g.world.peek(r).map(|p| (p.cell, p.pos))).flatten();
    let cell = cell.map(|c| g.world.placed.exterior_cell_at(c, pos).unwrap_or(c));
    let Ok(s) = g.world.get(r) else { return };
    let moved_cell = s.cell != cell;
    s.cell = cell;
    s.pos = pos;
    if let Some(a) = angles {
        s.rot = a;
    }
    if g.ref_script(r).is_some() {
        g.moved.insert(r);
    }
    if r != FormId::PLAYER_REF {
        g.teleported.insert(r);
        if !crate::ai::is_actor_base(g.world.forms, base_of(g, r).unwrap_or(FormId(0))) {
            g.turned.insert(r);
        }
    }
    g.emit(EventKind::Moved { reference: r, cell, pos });
    if r == FormId::PLAYER_REF && moved_cell {
        g.player_entered(cell);
        g.bring_teammates(from);
    }
}

/// Make the `MoveToMarkerWithFade` moves whose wait has ended (`Game::tick`) and ask the
/// engine to fade those references in.
pub(crate) fn finish_fade_moves(g: &mut Game) {
    let now = g.time;
    let due: Vec<(FormId, super::misc::FadeMove)> = g.script_looks.fade_moves.iter().filter(|(_, m)| m.at <= now).map(|(&r, &m)| (r, m)).collect();
    for (r, m) in due {
        g.script_looks.fade_moves.remove(&r);
        // A marker gone since the call leaves the reference where it is.
        let _ = move_ref(g, r, m.marker, m.offset);
        g.script_looks.fades.push((r, true));
    }
}

/// Move `r` to `marker`'s cell and position plus `offset` (MoveTo and
/// MoveToMarkerWithFade).
pub(crate) fn move_ref(g: &mut Game, r: FormId, marker: FormId, offset: [f64; 3]) -> Result<(), String> {
    let (cell, mut pos, mut rot) = place(g, marker)?;
    // Where the player was, for whoever follows them (`Game::bring_teammates`).
    let from = (r == FormId::PLAYER_REF).then(|| g.world.peek(r).map(|p| (p.cell, p.pos))).flatten();
    for (p, o) in pos.iter_mut().zip(offset) {
        *p += o as f32;
    }
    // The player's X angle is its view's pitch (`get_angle`): a reference moved to the
    // player stands upright [hypothesis: the pitch was never written before 2026-10-19].
    if marker == FormId::PLAYER_REF {
        rot = [0.0, 0.0, rot[2]];
    }
    let cell = cell.map(|c| g.world.placed.exterior_cell_at(c, pos).unwrap_or(c));
    {
        let s = g.world.get(r).map_err(|e| e.to_string())?;
        s.cell = cell;
        s.pos = pos;
        if r != FormId::PLAYER_REF {
            s.rot = rot;
        } else if marker != FormId::PLAYER_REF {
            // The player turns to the marker's heading: the ending moves the player to an
            // XMarkerHeading (0x001639F6, heading 0) whose heading faces the slide
            // (0x001680A7, straight ahead along +Y) [observed: the slide room's
            // references, fnvtool world refs; the owner's playtest 2026-11-12, issue
            // #2226: ours faced 90 degrees away while the original faces the slide]. The
            // GECK wiki's MoveTo page does not say [hypothesis: MoveTo to any reference
            // turns the player to its heading].
            //
            // The view's pitch (its X angle) is the marker's too: both slide markers
            // (the ending's, Honest Hearts' NVDLC02SlideShowMarkerREF 0x0101012F) are
            // level, with the slide's centre 118-121 units over the feet and 118 ahead,
            // and the scripts hold the player's looking there (DisablePlayerControls),
            // so a pitch kept from before left the slide tilted off the screen
            // [observed: the owner's playtest 2026-11-15, #2710: Honest Hearts' intro
            // looked down at the slide; fnvtool world refs of both rooms]
            // [hypothesis: the original levels the view on any MoveTo of the player].
            s.rot[2] = rot[2];
            s.rot[0] = rot[0];
        }
    }
    g.note_relocated(r, cell, pos);
    if r == FormId::PLAYER_REF {
        g.player_entered(cell);
        g.bring_teammates(from);
        // A co-op host's guests go with it (a script's move, not a door's or travel's)
        // [design].
        if let Some(out) = g.coop_effects.as_mut().filter(|o| o.len() < crate::sound::MAX_PENDING) {
            out.push(crate::sound::CoopEffect::PlayerMoved);
        }
    }
    Ok(())
}

pub fn get_self(_g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    Ok(ctx.self_ref.map(|r| Value::Ref(r.0)).unwrap_or(Value::NONE))
}

/// 1 when the caller's base form is the given form (or the caller is that form). In a
/// condition a null form (FormID 0, as on INFO 0x000FF5BA) is no one's: 0 [observed
/// 2026-10-30, capture o4_00_probe: a placed copy of Papa Khan told to SayTo GREETING
/// passed over 0x000FF5BA, `GetIsID <null> == 1` its only condition, for a line 308
/// places further on; observer-checks.md check 35].
pub fn get_is_id(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    if ctx.in_condition && a.is_null_form(0) {
        return yes(false);
    }
    let r = ctx.subject()?;
    let id = a.form(0)?;
    yes(r == id || base_of(g, r) == Some(id))
}

pub fn get_is_reference(_g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    yes(ctx.subject()? == a.form(0)?)
}

/// 1 when the caller's cell's Editor ID begins with the given cell's [documented:
/// catalogue summary].
///
/// A co-op random encounter's `player.GetInCell`: whether any player is there
/// (`crate::coop`, the encounter notes).
pub fn get_in_cell(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let want = a.form(0)?;
    if r == FormId::PLAYER_REF && ctx.self_ref.is_some_and(|s| g.coop_encounter(s)) {
        return yes(g.any_player(|g, p| in_cell(g, p, want)));
    }
    yes(in_cell(g, r, want))
}

fn in_cell(g: &Game, r: FormId, want: FormId) -> bool {
    let Some(cell) = g.cell_of(r) else { return false };
    if cell == want {
        return true;
    }
    let (Some(have), Some(w)) = (g.data.edid_of.get(&cell), g.data.edid_of.get(&want)) else { return false };
    have.to_ascii_lowercase().starts_with(&w.to_ascii_lowercase())
}

pub fn get_in_worldspace(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let ws = a.form(0)?;
    let cell = g.cell_of(r);
    yes(cell.and_then(|c| g.world.placed.world_of(c)) == Some(ws))
}

/// `ref.GetInZone zone`: 1 when the calling reference is in a cell attached to the
/// encounter zone [documented: GECK wiki GetInZone, "Returns 1 if FredRef is currently in
/// a cell attached to the ParadiseFallsZone"]: its current cell's effective zone
/// (`World::cell_zone_of`: session `SetCellEncounterZone`, else CELL XEZN, else
/// `SetWorldspaceEncounterZone` / WRLD XEZN; docs/spec-notes/encounter-zones.md §1)
/// [hypothesis: a reference's own XEZN does not count; the wiki speaks of cells].
pub fn get_in_zone(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let zone = a.form(0)?;
    yes(g.cell_of(r).and_then(|c| g.world.cell_zone_of(c)) == Some(zone))
}

pub fn get_in_same_cell(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let t = a.form(0)?;
    let (c1, c2) = (g.cell_of(r), g.cell_of(t));
    yes(c1.is_some() && c1 == c2)
}

pub fn is_in_interior(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let cell = g.cell_of(r);
    yes(cell.is_some_and(|c| g.world.placed.world_of(c).is_none()))
}

/// Distance in game units between the two references when they are in the same
/// interior cell or the same worldspace; otherwise a large number [hypothesis].
pub fn get_distance(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let t = a.form(0)?;
    // A co-op random encounter's `player.GetDistance X` and `GetDistance player`: the
    // nearest player's (`crate::coop`, the encounter notes).
    if (r == FormId::PLAYER_REF || t == FormId::PLAYER_REF) && ctx.self_ref.is_some_and(|s| g.coop_encounter(s)) {
        let from = if r == FormId::PLAYER_REF { t } else { r };
        if let Some(d) = g.nearest_player_distance(from) {
            return Ok(Value::Num(d as f64));
        }
    }
    let (c1, p1, _) = place(g, r)?;
    let (c2, p2, _) = place(g, t)?;
    let space = |c: Option<FormId>| c.map(|c| g.world.placed.world_of(c).unwrap_or(c));
    if c1.is_none() || space(c1) != space(c2) {
        return Ok(Value::Num(f32::MAX as f64));
    }
    let d: f32 = (0..3).map(|i| (p1[i] - p2[i]).powi(2)).sum::<f32>().sqrt();
    Ok(Value::Num(d as f64))
}

pub fn get_pos(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let i = a.axis(0)?;
    Ok(Value::Num(place(g, r)?.1[i] as f64))
}

/// `SetPos axis value`: the reference moves along one axis [documented: GECK wiki SetPos].
/// What moves it on screen hears of it as of a MoveTo or a Rotate ([`placed_by_script`]).
pub fn set_pos(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let i = a.axis(0)?;
    let v = a.num(1)? as f32;
    let s = g.world.get(r).map_err(|e| e.to_string())?;
    s.pos[i] = v;
    let (cell, pos) = (s.cell, s.pos);
    let cell = cell.map(|c| g.world.placed.exterior_cell_at(c, pos).unwrap_or(c));
    g.world.get(r).map_err(|e| e.to_string())?.cell = cell;
    placed_by_script(g, r, true);
    g.projectile_placed_by_script(r, pos);
    none()
}

/// A script put `r` somewhere new (SetPos, SetAngle): an actor other than the player
/// starts again from there, as after MoveTo (`Game::teleported`); another object is
/// drawn at its new place, as after Rotate (`Game::turned`); the player travels there
/// when its position changed (the `Moved` event the engine turns into a teleport).
/// [hypothesis: the original applies SetPos and SetAngle at once, as their pages
/// describe no delay; the earlier code changed only the stored values, which the AI
/// and the engine overwrote, docs/design/script-function-audit.md.]
fn placed_by_script(g: &mut Game, r: FormId, moved: bool) {
    if g.ref_script(r).is_some() {
        g.moved.insert(r);
    }
    if r == FormId::PLAYER_REF {
        if moved {
            if let Ok((cell, pos, _)) = place(g, r) {
                g.emit(EventKind::Moved { reference: r, cell, pos });
            }
        }
    } else if crate::ai::is_actor_base(g.world.forms, base_of(g, r).unwrap_or(FormId(0))) {
        g.teleported.insert(r);
    } else {
        g.turned.insert(r);
    }
}

/// Angles in degrees in scripts, radians in the world state [documented: catalogue,
/// GetAngle/SetAngle take degrees]. The player's X angle is its view's pitch, -89 looking
/// up and 89 looking at its feet [documented: GECK wiki GetAngle], written by the engine
/// (nvf-play `player::sync_sim`).
/// A SetAngle shows from the next frame [observed: gaps11, GetAngle Z read the old
/// heading in SetAngle's frame].
pub fn get_angle(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let i = a.axis(0)?;
    if let Some(v) = g.gaps.angles_shown.get(&(r, i)) {
        return Ok(Value::Num(*v));
    }
    Ok(Value::Num((place(g, r)?.2[i] as f64).to_degrees()))
}

pub fn set_angle(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let i = a.axis(0)?;
    if !g.gaps.angles_shown.contains_key(&(r, i)) {
        let old = (place(g, r)?.2[i] as f64).to_degrees();
        g.gaps.angles_shown.insert((r, i), old);
    }
    let v = a.num(1)?.to_radians() as f32;
    g.world.get(r).map_err(|e| e.to_string())?.rot[i] = v;
    placed_by_script(g, r, false);
    none()
}

/// The reference's place as the plugins put it, before anything moved it: (position,
/// angles in radians). The player reads 0 on every axis [documented: GetStartingPos,
/// GetStartingAngle; observed: jip4 probe set, the player 0, DocMitchellRef its placement,
/// angle Z 22.918].
fn starting_place(g: &Game, r: FormId) -> ([f32; 3], [f32; 3]) {
    if r == FormId::PLAYER_REF {
        return ([0.0; 3], [0.0; 3]);
    }
    g.world.placed.get(r).map_or(([0.0; 3], [0.0; 3]), |p| (p.pos, p.rot))
}

/// `GetStartingPos axis` [documented: GECK wiki GetStartingPos].
pub fn get_starting_pos(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let i = a.axis(0)?;
    Ok(Value::Num(f64::from(starting_place(g, r).0[i])))
}

/// `GetStartingAngle axis`, in degrees [documented: GECK wiki GetStartingAngle;
/// observed: `b3_jip4_02_session_time_and_start_angle` (`wave4b_extenders.json`, #3679):
/// `PROBE|b3_jip4_02_session_time_and_start_angle|sess=5.31,docZ=22.92,dynZ=270.00`,
/// dynamic `PlaceAtMe` refs return their creation heading in degrees rather than 0].
pub fn get_starting_angle(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let i = a.axis(0)?;
    let rot = if r == FormId::PLAYER_REF {
        [0.0; 3]
    } else if let Some(p) = g.world.placed.get(r) {
        p.rot
    } else {
        g.world.peek(r).and_then(|s| s.made_rot).unwrap_or([0.0; 3])
    };
    // + 0.0: a stored -0 reads 0, as in the game [observed: jip4, DocMitchellRef Y].
    Ok(Value::Num(f64::from(rot[i]).to_degrees() + 0.0))
}

pub fn get_linked_ref(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    // SetLinkedReference's link first [observed: jip-n7].
    Ok(g.linked_ref(r).map(|l| Value::Ref(l.0)).unwrap_or(Value::NONE))
}

pub fn get_parent_ref(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(g.world.enable_parent(r).map(|e| Value::Ref(e.reference.0)).unwrap_or(Value::NONE))
}

/// The reference is removed (the engine removes it a few frames later; here at once
/// [hypothesis]).
pub fn mark_for_delete(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.extra.entry(r).or_default().marked_for_delete = true;
    g.world.set_deleted(r, true).map_err(|e| e.to_string())?;
    none()
}

pub fn set_destroyed(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.extra.entry(r).or_default().destroyed = a.num_or(0, 1.0) != 0.0;
    none()
}

pub fn get_destroyed(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.extra.get(&r).is_some_and(|e| e.destroyed))
}

pub fn lock(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if is_terminal(g, r) {
        crate::terminal::lock(g, r, a.num_or(0, 0.0) as i32);
        return none();
    }
    // 0, the default, locks without changing the level [documented: GECK wiki Lock,
    // "Lock 0 (default) will lock it but NOT change the lock level"].
    let level = a.num(0).ok().filter(|&l| l != 0.0).map(|l| l.clamp(0.0, 255.0) as u8);
    g.world.lock(r, level).map_err(|e| e.to_string())?;
    none()
}

pub fn unlock(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if is_terminal(g, r) {
        crate::terminal::unlock(g, r);
        return none();
    }
    g.world.unlock(r).map_err(|e| e.to_string())?;
    none()
}

/// A reference whose base is a terminal (TERM): its lock is the terminal's own
/// (`crate::terminal::{lock, unlock, locked, lock_level}`), not the world's lock data.
pub fn is_terminal(g: &Game, r: FormId) -> bool {
    super::base_of(g, r).is_some_and(|b| g.data.terminals.contains_key(&b))
}

/// Whether the reference's cell is one of the rules' loaded cells.
pub fn in_loaded_cell(g: &Game, r: FormId) -> bool {
    g.cell_of(r).is_some_and(|c| g.loaded_cells.contains(&c))
}

/// 0 not locked, 1 locked; 2 for a lock broken by forcing it [documented: GECK wiki
/// GetLocked, "locked out"; docs/spec-notes/lockpicking.md §5]. A terminal answers by
/// `crate::terminal::locked` while its cell is loaded, else 0 [observed, function probes
/// `fp1_queries` / `fp3_terminal` 2026-10-17].
pub fn get_locked(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if is_terminal(g, r) {
        return Ok(Value::Num(if in_loaded_cell(g, r) { crate::terminal::locked(g, r) as f64 } else { 0.0 }));
    }
    let status = g.world.lock_status(r).map_err(|e| e.to_string())?;
    Ok(Value::Num(if status == 1 && g.locks.broken.contains(&g.world.lock_source(r)) { 2.0 } else { status as f64 }))
}

/// 1 for a lock broken by forcing it (`crate::lockpick`) [documented: GECK wiki
/// GetIsLockBroken].
pub fn get_is_lock_broken(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(Value::bool(g.locks.broken.contains(&g.world.lock_source(r))))
}

/// `Ref.Activate [actor] [flag]`: with no actor the calling reference's action reference
/// (inside OnActivate: whoever activated it) is used, and without one nothing happens;
/// flag 0 runs the default action, bypassing OnActivate; flag 1 runs the OnActivate block
/// instead [documented: GECK wiki Activate].
///
/// Outside OnActivate the reference keeps the action reference of its last activation
/// ([`Game::action_refs`]) [hypothesis: the wiki's "the calling reference's ActionRef";
/// the Sink's appliances in OldWorldBlues.esm (SCPT 0x0100F05A, the Biological Research
/// Station) set a flag in OnActivate and call a bare `Activate` in GameMode once the
/// message box closes, to talk to the player who activated them [observed: `fnvtool
/// script decompile`]; talking-activators.md §2].
pub fn activate(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let target = ctx.subject()?;
    let kept = || g.action_refs.get(&target).copied();
    let actor = a.opt_form(0).or(ctx.action_ref).or_else(kept).ok_or("no action reference")?;
    // "Disabled actors cannot be forced to activate objects via Activate" [documented:
    // GECK wiki Disable notes].
    if matches!(base_sig(g, actor).as_ref(), Some(b"NPC_") | Some(b"CREA")) && !g.world.is_enabled(actor).unwrap_or(true) {
        return none();
    }
    let mode = if a.num_or(1, 0.0) != 0.0 { ActMode::Block } else { ActMode::Default };
    g.activate(actor, target, mode)?;
    none()
}

pub fn get_action_ref(_g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    Ok(ctx.action_ref.map(|r| Value::Ref(r.0)).unwrap_or(Value::NONE))
}

pub fn is_action_ref(_g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    yes(ctx.action_ref.is_some() && ctx.action_ref == a.opt_form(0))
}

fn base_sig(g: &Game, r: FormId) -> Option<[u8; 4]> {
    base_of(g, r).and_then(|b| g.data.sig(b))
}

pub fn is_actor(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(matches!(base_sig(g, r).as_ref(), Some(b"NPC_") | Some(b"CREA")))
}

pub fn get_is_creature(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(base_sig(g, r).as_ref() == Some(b"CREA"))
}

/// The members of a form list: authored, plus those added at run time.
pub fn list_members(g: &Game, list: FormId) -> Vec<FormId> {
    // ListAddForm and the ListRemove… functions keep the whole list for the session.
    if let Some(v) = g.list_edits.get(&list) {
        return v.clone();
    }
    let mut v = g.form_lists.get(&list).cloned().unwrap_or_default();
    v.extend(g.data.form_lists.get(&list).into_iter().flatten().copied());
    v
}

/// The items that can repair `item`: the members of its repair list (WEAP/ARMO `REPL`),
/// authored, overridden by `SetRepairList`, and added at run time (docs/spec-notes/repair.md §5).
pub fn repair_list(g: &Game, item: FormId) -> Vec<FormId> {
    g.effective_repair_list_form(item).map(|l| list_members(g, l)).unwrap_or_default()
}

/// 1 when the caller's base form is in the list.
pub fn is_in_list(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let list = a.form(0)?;
    let b = base_of(g, r);
    let m = list_members(g, list);
    // The base form only: a list holding the reference itself reads 0 [documented: GECK
    // wiki IsInList, "Will return False (0) if the form list contains the object's
    // reference itself, rather than its base form"].
    yes(b.is_some_and(|b| m.contains(&b)))
}

/// Inserts at index 0 [documented: catalogue summary]. A form a script added before moves
/// to index 0; one the record lists stays where it is [documented: GECK wiki
/// AddFormToFormList, "will simply move it to the 0th index (Will do nothing if the form
/// already exists via hard edit)"].
pub fn add_form_to_form_list(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let list = a.form(0)?;
    let f = a.form(1)?;
    if g.data.sig(list).as_ref() != Some(b"FLST") {
        return Err(format!("{} is not a form list", g.data.name(list)));
    }
    // A list the xNVSE functions edited this session gets the form too, at the top.
    if let Some(e) = g.list_edits.get_mut(&list) {
        if !e.contains(&f) {
            e.insert(0, f);
        }
    }
    let added = g.form_lists.entry(list).or_default();
    if let Some(i) = added.iter().position(|&x| x == f) {
        added.remove(i);
        added.insert(0, f);
    } else if !list_members(g, list).contains(&f) {
        g.form_lists.entry(list).or_default().insert(0, f);
    }
    none()
}

/// Opens (1) or closes (0) a door [documented: GECK wiki SetOpenState], or an activator
/// with Open and Close sequences, which plays them (`Game::set_ref_open`).
pub fn set_open_state(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.set_ref_open(r, a.num(0)? != 0.0)?;
    none()
}

/// 0 none, 1 open, 2 opening, 3 closed, 4 closing [documented: GECK wiki GetOpenState
/// notes]. An object the engine animates open and closed reads the state it told
/// (`Senses::open_states`); any other object 1 or 3 by its open flag, and an actor 0
/// [observed: a load door, a cash register, crates, a duffle bag, sacks, a metal box, a
/// safe and a dialogue trigger activator in the Goodsprings gas station all read 3, and
/// Ringo 0, captures `fa_batch1_queries` and `fa_batch3_actions`].
pub fn get_open_state(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if let Some(&code) = g.senses.open_states.get(&r) {
        return Ok(Value::Num(code as f64));
    }
    if r == FormId::PLAYER_REF || crate::ai::is_actor_base(g.world.forms, base_of(g, r).unwrap_or(FormId(0))) {
        return Ok(Value::Num(0.0));
    }
    let open = g.world.get(r).map_err(|e| e.to_string())?.open;
    Ok(Value::Num(if open { 1.0 } else { 3.0 }))
}

/// `ref.GetDefaultOpen`: 1 if the calling reference's default state is open, else 0
/// (`0x10D7`, CTDA condition index `215`) [documented: GECK wiki GetDefaultOpen].
pub fn get_default_open(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let open = g.world.check_default_open(r).map_err(|e| e.to_string())?;
    Ok(Value::Num(if open { 1.0 } else { 0.0 }))
}

/// Catalogue-unnumbered vanilla reference functions handled as named source calls
/// (docs/spec-notes/default-open.md).
pub(crate) fn handles_source(name: &str) -> bool {
    name.eq_ignore_ascii_case("SetDefaultOpen")
}

/// Execute a catalogue-unnumbered vanilla reference source call (`SetDefaultOpen`).
/// An explicit empty or unknown receiver fails rather than falling back to `ctx.self_ref`.
pub(crate) fn call_source(
    g: &mut Game,
    ctx: &Ctx,
    receiver: Option<Value>,
    name: &str,
    values: &[XVal],
) -> Option<Result<XVal, String>> {
    if !name.eq_ignore_ascii_case("setdefaultopen") {
        return None;
    }
    Some((|| {
        let r = match receiver {
            Some(value) => value.as_form().map(FormId).ok_or("invalid calling reference")?,
            None => ctx.self_ref.ok_or("SetDefaultOpen requires a calling reference")?,
        };
        if values.len() != 1 {
            return Err("setdefaultopen expects 1 argument".into());
        }
        let arg = match &values[0] {
            XVal::Num(n) if n.is_finite() => ArgValue::Value(Value::Num(*n)),
            _ => return Err("SetDefaultOpen expects a numeric defaultOpen argument".into()),
        };
        let args = [arg];
        let open = Args(&args).int(0)? != 0;
        g.world.set_default_open(r, open).map_err(|e| e.to_string())?;
        Ok(XVal::Num(0.0))
    })())
}

/// A trigger-enter event on the calling trigger, by the given reference [documented:
/// catalogue summary].
pub fn enter_trigger(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let t = ctx.subject()?;
    let by = a.form(0)?;
    g.ref_event(t, crate::game::event::ON_TRIGGER_ENTER, Some(Value::Ref(by.0)), Some(by));
    none()
}

/// `ref.Rotate axis degreesPerSecond`: turns the reference about the world's X, Y or Z
/// axis at that speed [documented: GECK wiki Rotate]. One call turns it for one frame:
/// the shipped turners call it from GameMode every frame [observed: `SCPT 0x0013414C`
/// and its two siblings, `linkedRef.Rotate z ±300/900`], and a reference turns about
/// one axis per frame, the last call of the frame counting [documented: the page's
/// note]. So a call records the turn and [`Game::tick`] applies speed × frame time to
/// the angle at the end of the frame [hypothesis: one frame per call]. The angle is the
/// reference's own about that axis (Z is the heading; X and Y are only "world" for an
/// unturned reference, which the page warns of [documented: "not reliable for complex
/// rotations"]). Collision does not follow [documented: "Collision is not updated with
/// rotation until the cell is reloaded"].
pub fn rotate(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let axis = a.axis(0)?;
    let speed = a.num(1)?;
    g.world.get(r).map_err(|e| e.to_string())?;
    g.rotations.insert(r, (axis, speed));
    none()
}

impl Game<'_> {
    /// Apply this frame's Rotate calls ([`rotate`]), angles kept within ±π.
    pub(crate) fn apply_rotations(&mut self, dt: f64) {
        for (r, (axis, speed)) in std::mem::take(&mut self.rotations) {
            let Ok(s) = self.world.get(r) else { continue };
            let a = s.rot[axis] as f64 + (speed * dt).to_radians();
            s.rot[axis] = (a + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) as f32 - std::f32::consts::PI;
            self.turned.insert(r);
        }
    }
}
