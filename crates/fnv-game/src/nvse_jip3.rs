//! JIP LN, JohnnyGuitar and LU functions the owner's mod lists call that no earlier set
//! built (the third batch, 2026-10-27; docs/handoff/modlist-function-counts.json): ash
//! piles, ragdolls, the compass and nearby hostiles, quests, the player's doors and
//! cells, effect shaders and inventory references moved [documented: their pages in the
//! wiki export; observed where a set is named].

use crate::game::{Game, RunCtx};
use fnv_script::vm::XVal;
use fnv_sim::FormId;
use std::collections::HashMap;

/// This file's session state (not saved).
#[derive(Debug, Default)]
pub struct Jip3State {
    /// The last exterior door the player used (GetPCLastExteriorDoor).
    pub last_exterior_door: Option<FormId>,
    /// The player's cell when each script last asked GetPCMovedCell, by the script's
    /// calling reference and quest.
    pub moved_seen: HashMap<(Option<FormId>, Option<FormId>), Option<FormId>>,
    /// JohnnyGuitar's event handlers set here: (event, function, filter form or list).
    pub jg_handlers: Vec<(JgEvent, XVal, Option<FormId>)>,
    /// FreezeTime's mode: game time stands still (`Game::tick`).
    pub time_frozen: bool,
    /// The quantized game clock at the last RenderUpdate dispatch: its handlers'
    /// GetSecondsPassed is the time since (docs/spec-notes/jip-callback-clock.md).
    pub render_clock: Option<f64>,
    /// Transient `RefreshItemsList` request for the engine's open item/recipe menu.
    pub refresh_items_list: bool,
}

/// The JohnnyGuitar events whose handlers this file keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JgEvent {
    /// Every frame (SetJohnnyOnRenderUpdateEventHandler).
    RenderUpdate,
    /// An actor is killed (SetJohnnyOnDyingEventHandler).
    Dying,
    /// A quest is completed (SetJohnnyOnCompleteQuestEventHandler).
    CompleteQuest,
    /// A challenge is completed (SetJohnnyOnChallengeCompleteEventHandler).
    ChallengeComplete,
}

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(r) if *r != 0 => Some(FormId(*r)),
        _ => None,
    }
}

fn num(args: &[XVal], k: usize, dflt: f64) -> f64 {
    match args.get(k) {
        Some(XVal::Num(n)) => *n,
        _ => dflt,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(if b { 1.0 } else { 0.0 })
}

/// The answer of a function with output variables (`fnv_script::vm`'s out_call): the
/// return value, then the outputs.
fn outputs(g: &mut Game, ret: XVal, outs: &[f64]) -> XVal {
    let entries = std::iter::once(ret).chain(outs.iter().map(|v| XVal::Num(*v))).enumerate().map(|(i, v)| (fnv_script::vm::XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(fnv_script::vm::ArrayKind::List, entries))
}

/// The way a pitch `x` and heading `z` (radians) look: x positive down, z clockwise from
/// north (+y), as the references' angles are kept.
fn facing(x: f32, z: f32) -> [f32; 3] {
    [z.sin() * x.cos(), z.cos() * x.cos(), -x.sin()]
}

/// The default reach of a cast with no `maxRange` given (`GetPointRayCastPos`: `50_000.0`
/// [observed: 2026-11-24, `w6_02_point_raycast_miss_and_range`]; `GetRayCastPos` /
/// `GetRayCastRef` remain [hypothesis]).
const RAY_REACH: f32 = 50_000.0;

impl Game<'_> {
    /// JohnnyGuitar's handlers for `ev` on `subject`: each whose filter is none, the form
    /// itself or a form list holding it runs with the form as its argument (no argument for
    /// RenderUpdate) [documented: the SetJohnnyOn…EventHandler pages; hypothesis: the
    /// arguments].
    pub(crate) fn jg_event(&mut self, ev: JgEvent, subject: Option<FormId>) {
        // A RenderUpdate handler runs every frame as a function, with no script instance
        // of its own: its GetSecondsPassed (and its nested calls') is the game-clock time
        // since the previous dispatch, as a main-loop callback's (jip-callback-clock.md)
        // [hypothesis: B42 True Leaning eases its lean by GetSecondsPassed in its handler;
        // observed: mods_reference ln_B42Leaning.txt]. A clock that went back (another
        // session) gives 0.
        let seconds = (ev == JgEvent::RenderUpdate).then(|| {
            let now = crate::functions::clock_now(self.time);
            let last = self.gaps.jip3.render_clock.replace(now);
            last.map_or(0.0, |l| (now - l).max(0.0))
        });
        if self.gaps.jip3.jg_handlers.iter().all(|(e, _, _)| *e != ev) {
            return;
        }
        let hs: Vec<(XVal, Option<FormId>)> = self.gaps.jip3.jg_handlers.iter().filter(|(e, _, _)| *e == ev).map(|(_, f, l)| (f.clone(), *l)).collect();
        for (f, filter) in hs {
            let pass = match (filter, subject) {
                (None, _) => true,
                (Some(l), Some(s)) => l == s || crate::functions::base_of(self, s) == Some(l) || crate::functions::list_members(self, l).contains(&s),
                (Some(_), None) => false,
            };
            if !pass {
                continue;
            }
            let args = subject.map(|s| vec![XVal::Ref(s.0)]).unwrap_or_default();
            if let Err(error) = self.run_function_with_seconds(&f, None, args, None, (RunCtx::default(), seconds)) {
                self.handler_failed("JohnnyGuitar event handler".into(), error);
            }
        }
    }

    /// The player used door `door`: kept when it stands outside (GetPCLastExteriorDoor).
    pub(crate) fn note_player_door(&mut self, door: FormId) {
        let outside = self.cell_of(door).is_some_and(|c| self.world.placed.world_of(c).is_some());
        if outside {
            self.gaps.jip3.last_exterior_door = Some(door);
        }
    }

    /// Whether a script has requested `RefreshItemsList` since the engine last consumed it.
    pub fn refresh_items_list_pending(&self) -> bool {
        self.gaps.jip3.refresh_items_list
    }

    /// Consume a pending `RefreshItemsList` request (one-shot: returns `true` once, then
    /// `false` until a script calls `RefreshItemsList` again).
    pub fn take_refresh_items_list(&mut self) -> bool {
        std::mem::take(&mut self.gaps.jip3.refresh_items_list)
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // The actor an ash or goo pile came from [documented: GetAshPileSource].
        "getashpilesource" => XVal::Ref(subject.and_then(|p| g.ash_pile_source(p)).map_or(0, |a| a.0)),
        // Knocked down (unconscious, pushed away) or dead and lying as a body
        // [documented: GetIsRagdolled; hypothesis: a body counts].
        "getisragdolled" => yes(subject.is_some_and(|a| g.knockout(a).is_some() || g.is_dead(a))),
        // JohnnyGuitar: among the compass's hostiles (`nvse_hud`) [documented:
        // IsCompassHostile].
        "iscompasshostile" => yes(subject.is_some_and(|a| g.compass_hostiles().iter().any(|(r, _)| *r == a))),
        // JohnnyGuitar: a hostile within fHostileActorExteriorDistance (interior:
        // ...InteriorDistance) of the player [documented: IsHostilesNearby].
        "ishostilesnearby" => {
            let inside = g.cell_of(FormId::PLAYER_REF).is_some_and(|c| g.world.placed.world_of(c).is_none());
            let key = if inside { "fHostileActorInteriorDistance" } else { "fHostileActorExteriorDistance" };
            let reach = g.world.setting_f32(key).unwrap_or(0.0);
            yes(g.compass_hostiles().iter().any(|(_, d)| *d <= reach))
        }
        // `quest`: failed, as a Fail Quest stage leaves it (also read as completed)
        // [documented: FailQuest; the completed flag as `Game::set_stage` sets it].
        "failquest" => {
            if let Some(q) = form(args, 0) {
                let s = g.quest(q);
                let first = !s.failed;
                s.failed = true;
                s.completed = true;
                if first {
                    g.emit(crate::trace::EventKind::QuestFailed { quest: q });
                }
            }
            zero
        }
        "getpclastexteriordoor" => XVal::Ref(g.gaps.jip3.last_exterior_door.map_or(0, |d| d.0)),
        // 1 when the player's cell changed since this script last asked; the first call
        // reads 0 [documented: GetPCMovedCell; observed: jip3a, 0 then 0 in one cell].
        "getpcmovedcell" => {
            let now = g.cell_of(FormId::PLAYER_REF);
            let key = (ctx.self_ref, ctx.quest);
            let before = g.gaps.jip3.moved_seen.insert(key, now);
            yes(before.is_some_and(|b| b != now))
        }
        // `shader`: how many times it plays on the reference, from the scripts' plays
        // [documented: HasEffectShader; hypothesis: a magic effect's shader not counted].
        "haseffectshader" => {
            let (Some(r), Some(s)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            XVal::Num(g.script_looks.shaders.iter().filter(|p| p.on == r && p.shader == s).count() as f64)
        }
        // JohnnyGuitar: interacting with it is a crime or an attack: an actor hostile to
        // the player, or something someone else owns [documented: IsCrimeOrEnemy, "the
        // same checks that are used to determine red color of the crosshair"].
        "iscrimeorenemy" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let enemy = g.crime_enemy(r) || crate::rest::hostile_to_player(g, r);
            yes(enemy || g.owned_by_other(r).is_some())
        }
        // `[count] [clearOwner] [target]`: the inventory reference's items leave their
        // container, to `target` when given [documented: RemoveMeIRAlt; count <= 0 or
        // none is the whole stack].
        "removemeiralt" => {
            let Some((c, item)) = subject.and_then(|r| g.temp_item(r)) else { return Some(Ok(zero)) };
            let have = g.world.inventory_mut(c).map(|inv| inv.count(item)).unwrap_or(0);
            let want = num(args, 0, 0.0) as i32;
            let n = if want <= 0 { have } else { want.min(have) };
            let gone = g.world.remove_item(c, item, n).unwrap_or(0);
            if let (Some(to), true) = (form(args, 2), gone > 0) {
                let _ = g.world.add_item(to, item, gone);
            }
            zero
        }
        // `[count] [clearOwner]`: the inventory reference's items dropped beside their
        // container, as Drop does [documented: DropMeAlt].
        "dropmealt" => {
            let Some((c, item)) = subject.and_then(|r| g.temp_item(r)) else { return Some(Ok(zero)) };
            let have = g.world.inventory_mut(c).map(|inv| inv.count(item)).unwrap_or(0);
            let want = num(args, 0, 0.0) as i32;
            let n = if want <= 0 { have } else { want.min(have) };
            if n > 0 {
                if let Err(e) = crate::functions::console::call_words(g, Some(c), "Drop", &[format!("{:08X}", item.0), n.to_string()], "DropMeAlt") {
                    return Some(Err(e));
                }
            }
            zero
        }
        // `container [clearOwner]`: a placed item goes into the container [documented:
        // MoveToContainer].
        "movetocontainer" => {
            let (Some(r), Some(to)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let Some(item) = g.world.peek(r).map(|s| s.base) else { return Some(Ok(zero)) };
            let count = g.world.peek(r).map_or(1, |s| s.count.max(1));
            if g.world.add_item(to, item, count).is_ok() && g.world.set_enabled(r, false).is_ok() {
                let _ = g.world.set_deleted(r, true);
            }
            zero
        }
        // LU `flag [actorBase]`: a Template Data flag (1 Traits … 512 Script). Given a base
        // as the argument it reads 0, with or without a calling reference; on a reference,
        // its base's ACBS template flags when the base has a template (TPLT) [documented:
        // GetActorTemplateFlag; observed: jip3a, six bases as the argument all 0; jip3b,
        // a placed PowderGangerVault19TEMPLATE reads 942 on the reference (its base is one
        // the game made, not the record, whose flags are 128) and 0 with the base given;
        // hypothesis: references placed from the plugins read their record's flags].
        "getactortemplateflag" => {
            if form(args, 1).is_some() {
                return Some(Ok(zero));
            }
            let Some(b) = subject.map(|r| crate::functions::base_of(g, r).unwrap_or(r)) else { return Some(Ok(zero)) };
            if g.data.actors.get(&b).and_then(|a| a.template).is_none_or(|t| t.0 == 0) {
                return Some(Ok(zero));
            }
            let flags = crate::nvse::record_value(g, b, "ACBS", "template_flags").and_then(|v| v.num().ok()).unwrap_or(0.0) as u64;
            yes(flags & (num(args, 0, 0.0) as u64) != 0)
        }
        // The base's Immobile flag (CREA ACBS bit 23) [documented: GetIsImmobile, "incapable of
        // moving"; the bit, the schema's ACBS flags].
        "getisimmobile" => {
            let Some(b) = form(args, 0).or(subject).map(|r| g.actor_flags_target(r)) else { return Some(Ok(zero)) };
            if g.data.sig(g.unclone(b)) == Some(*b"NPC_") {
                return Some(Ok(zero));
            }
            yes(g.actor_base_flags_raw(b) & (1 << crate::actor_values::acbs::IMMOBILE) != 0)
        }
        // LU `list`: the calling actor's race is in the form list [documented: IsRaceInList].
        "israceinlist" => {
            let (Some(a), Some(l)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            let race = g.actor_race(a);
            yes(race.is_some_and(|r| crate::functions::list_members(g, l).contains(&r)))
        }
        // `menu`: the reference the open menu is about [documented: GetMenuTargetRef]: the
        // dialogue menu's speaker (1009); the container (1008), the merchant (1053), the
        // item being repaired (1035) and the weapon being modded (1061, an inventory
        // reference), as the engine reports them (`Game::set_menu_picks`). The other codes
        // read 0 [hypothesis: not kept yet].
        "getmenutargetref" => {
            let code = num(args, 0, 0.0) as u16;
            match code {
                crate::game::menus::DIALOGUE => XVal::Ref(g.dialogue.as_ref().map(|d| d.actor).filter(|_| g.menu_open(code)).map_or(0, |r| r.0)),
                _ => pick_val(g, g.menu_picks(code).target),
            }
        }
        // The form highlighted in the menu on top [documented: GetSelectedItemRef]: an
        // inventory reference in the inventory (1002), container (1008), repair (1035),
        // barter (1053) and mod (1061) menus; 0 in any other [hypothesis: the DATA and
        // STATS pages' base forms are not reported yet].
        "getselecteditemref" => {
            let p = g.top_menu().and_then(|c| g.menu_picks(c).selected);
            pick_val(g, p)
        }
        // `quest seconds`: the quest's script runs after `seconds` more (0: next frame);
        // returns `0` (not `seconds`) [documented: SetDelayElapsed; observed 2026-11-24:
        // Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 10 official base
        // masters, `GSDocMitchellHouse` (`0x00103DF9`), isolated clean-room staging copy
        // (read-only `PrintToConsole` probe; zero writes to game install or Saves),
        // committed probe `w6t2_31_xnvse_items_ammo_rounds_and_ingrid`
        // (`tools/capture/nvse/catalogs/wave6_t2_set_delay_elapsed.json`):
        // `PROBE|w6t2_31_xnvse_items_ammo_rounds_and_ingrid|ammo(u=0,0=0,5=0)|grid(def=1,0=1)|del=0.00|tf(ref=0,arg=0)`;
        // zero DLL, PDB, or third-party source consulted].
        "setdelayelapsed" => {
            let Some(q) = form(args, 0) else { return Some(Ok(zero)) };
            let secs = num(args, 1, 0.0).max(0.0);
            let delay = g.quests.get(&q).and_then(|s| s.delay).or_else(|| g.data.quests.get(&q).map(|d| d.script_delay())).unwrap_or(0.0);
            let s = g.quest(q);
            s.since_run = (delay - secs).max(0.0);
            zero
        }
        // Request that the engine rebuild the open item/recipe menu's displayed rows on
        // the next UI sync without closing the menu [documented: RefreshItemsList].
        "refreshitemslist" => {
            g.gaps.jip3.refresh_items_list = true;
            zero
        }
        // `mode`: 0 god mode (1 while on), 1 actors' AI (1 while it runs, 0 after `tai`),
        // 2 combat AI (1 by default while enabled, matching mode 1), 3 collision (0 by
        // default), 4 borders (0 by default), out-of-range modes 0 [documented:
        // GetDebugModeState; observed: jip3a, 0 then 1 across `tgm`, 1 then 0 then 1
        // across `tai`; observed 2026-11-24: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9
        // + JIP LN 57.30, 10 official base masters, `GSDocMitchellHouse` (`0x00103DF9`),
        // isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes
        // to game install or Saves), committed probe
        // `w6t2_24_jip_fall_time_and_is_in_air_edges`
        // (`tools/capture/nvse/catalogs/wave6_t2_debug_mode_state.json`):
        // `PROBE|w6t2_24_jip_fall_time_and_is_in_air_edges|dbg(0,1,1,0,0,0,tgm=1)|fall=0.952,air=0`;
        // zero DLL, PDB, or third-party source consulted; hypothesis: toggling `tcai`,
        // `tcl`, or `tb` at runtime is not yet tracked in `Toggles`].
        "getdebugmodestate" => match num(args, 0, -1.0) as i64 {
            0 => yes(g.toggles.god_mode),
            1 => yes(!g.toggles.actors_ai_off),
            2 => yes(true),
            _ => zero,
        },
        // `xVar yVar zVar [local]`: the reference's Euler angles in degrees; `getLocal=0`
        // (default) matches `GetAngle`, while `getLocal=1` decomposes the inverse/local
        // matrix `Rx(-x) * Ry(-y) * Rz(-z)` into ZYX Euler angles [documented: GetAngleEx;
        // observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
        // `w6_t1_02_angle_ex_and_heading` (`tools/capture/nvse/catalogs/wave6_t1_jip_angles.json`,
        // `docs/spec-notes/jip-angles-ex.md`)].
        "getangleex" => {
            let Some((_, rot)) = subject.and_then(|r| g.ref_pos_rot(r)) else {
                return Some(Ok(outputs(g, zero, &[])));
            };
            let local = num(args, 0, 0.0) != 0.0;
            let deg = if local {
                local_euler_degrees(rot)
            } else {
                [f64::from(rot[0].to_degrees()), f64::from(rot[1].to_degrees()), f64::from(rot[2].to_degrees())]
            };
            outputs(g, zero, &deg)
        }
        // `x0 y0 z0 x1 y1 z1 → pitch heading`: the angles, in degrees, of the way from the
        // first point to the second — pitch positive UP (`+Z`), heading counter-clockwise
        // from north (`+Y`) in `(-180, 180]` (`E = -90`, `W = +90`, `S = +180`), the right-handed Euler
        // `Rz(heading) * Rx(pitch) * +Y` [documented: GetAnglesBetweenPoints; observed:
        // Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
        // `w6_t1_01_angles_between_points` (`tools/capture/nvse/catalogs/wave6_t1_jip_angles.json`,
        // `docs/spec-notes/jip-angles-ex.md`)].
        "getanglesbetweenpoints" => {
            let p: Vec<f64> = (0..6).map(|k| num(args, k, 0.0)).collect();
            let [pitch, heading] = angles_between_points([p[0], p[1], p[2]], [p[3], p[4], p[5]]);
            outputs(g, zero, &[pitch, heading])
        }
        // `x y z rotX rotZ → x y z [flags] [range]`: casts from `(x, y, z)` along
        // right-handed Euler `Rz(rotZ) · Rx(rotX) · +Y` (`[-sin(rotZ)·cos(rotX),
        // cos(rotZ)·cos(rotX), sin(rotX)]`, i.e. `facing(-rotX, -rotZ)` — pitch positive
        // up, yaw counter-clockwise from +Y). Returns `1` on both geometry hits and
        // open-sky misses, writing the hit point on hit or `origin + dir * reach` on
        // miss; omits `maxRange` → `50_000.0`, while explicit `0` writes `origin` and
        // explicit `-1` casts `-1` unit along `-dir`; `collisionFlag` `0`/`1`/`6`/`-1`
        // hit terrain and actors, while `8`/`19` hit terrain and interior statics while
        // skipping actors [documented: GECK wiki GetPointRayCastPos; observed: 2026-11-24,
        // FNV 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, committed probes
        // `w6_01_point_raycast_angles`, `w6_02_point_raycast_miss_and_range`, and
        // `w6_03_point_raycast_collision_flags` in
        // `tools/capture/nvse/catalogs/wave6_point_raycast.json`; see
        // `docs/spec-notes/jip-point-raycast.md`]. Routing `8 | 19` through
        // `CastKind::CameraClearance` (also excluding dynamic clutter) and unprobed
        // flags through `CastKind::World`, plus returning `1` headless without a physics
        // caster, remain [design] / [hypothesis].
        "getpointraycastpos" => {
            let o = [num(args, 0, 0.0) as f32, num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32];
            let dir = facing(-(num(args, 3, 0.0) as f32).to_radians(), -(num(args, 4, 0.0) as f32).to_radians());
            let flag = num(args, 5, 0.0) as i64;
            let kind = match flag {
                8 | 19 => crate::ray::CastKind::CameraClearance,
                _ => crate::ray::CastKind::World,
            };
            let reach = num(args, 6, f64::from(RAY_REACH)) as f32;
            let to = [o[0] + dir[0] * reach, o[1] + dir[1] * reach, o[2] + dir[2] * reach];
            let hit = (reach != 0.0).then(|| crate::ray::cast_hit(o, to, kind)).flatten();
            let pt = hit.map_or(to, |h| h.point);
            outputs(g, XVal::Num(1.0), &[f64::from(pt[0]), f64::from(pt[1]), f64::from(pt[2])])
        }
        // `xGlobal yGlobal zGlobal [offset] [flags] [node] [range]` and `[flags] [node]`:
        // a cast from the reference along its facing (an actor from its eyes) [documented:
        // GetRayCastPos, GetRayCastRef; hypothesis: the root node's place and the actor's
        // eye 64 above its feet; the node names not read].
        "getraycastpos" | "getraycastref" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let Some((pos, rot)) = g.ref_pos_rot(r) else { return Some(Ok(zero)) };
            let actor = r == FormId::PLAYER_REF || g.data.actors.contains_key(&crate::functions::base_of(g, r).unwrap_or(r));
            let dir = facing(rot[0], rot[2]);
            let pos_ray = name == "getraycastpos";
            let offset = if pos_ray { num(args, 3, 0.0) as f32 } else { 0.0 };
            let reach = if pos_ray { Some(num(args, 6, 0.0) as f32).filter(|r| *r > 0.0).unwrap_or(RAY_REACH) } else { RAY_REACH };
            let base = [pos[0], pos[1], pos[2] + if actor { crate::ray::ACTOR_RAY_EYE_HEIGHT } else { 0.0 }];
            let o = [base[0] + dir[0] * offset, base[1] + dir[1] * offset, base[2] + dir[2] * offset];
            let to = [o[0] + dir[0] * reach, o[1] + dir[1] * reach, o[2] + dir[2] * reach];
            let hit = crate::ray::cast_hit(o, to, crate::ray::CastKind::World);
            if pos_ray {
                if let Some(h) = hit {
                    for (k, v) in h.point.iter().enumerate() {
                        if let Some(gl) = form(args, k) {
                            let _ = g.world.set_global(gl, *v);
                        }
                    }
                }
                yes(hit.is_some())
            } else {
                XVal::Ref(hit.and_then(|h| h.form).unwrap_or(0))
            }
        }
        // LU `list type [modIndex] [noClear]`: the list emptied (unless noClear), then every
        // loaded form of the type added, from one plugin when modIndex is given; type 57
        // (cells) only interiors [documented: GetLoadedType].
        "getloadedtype" => {
            let Some(l) = form(args, 0) else { return Some(Ok(zero)) };
            let code = num(args, 1, -1.0) as i32;
            let index = num(args, 2, -1.0) as i64;
            let keep = num(args, 3, 0.0) != 0.0;
            let mut forms = g.data.forms_of_type(code);
            if index >= 0 {
                forms.retain(|f| i64::from(f.0 >> 24) == index);
            }
            if code == 57 {
                forms.retain(|c| g.world.placed.world_of(*c).is_none());
            }
            let list = g.edited_list(l);
            if !keep {
                list.clear();
            }
            for f in forms {
                if !list.contains(&f) {
                    list.push(f);
                }
            }
            zero
        }
        // The effects with a duration on the actor, each as [source, base effect,
        // magnitude, seconds left, caster, duration] [documented: GetTempEffects; hypothesis:
        // the order is the effects' own].
        "gettempeffects" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let rows: Vec<Vec<XVal>> = g
                .effects
                .iter()
                .filter(|e| e.target == a && !e.permanent && e.duration > 0.0)
                .map(|e| {
                    vec![
                        XVal::Ref(e.source.0),
                        XVal::Ref(e.effect.0),
                        XVal::Num(f64::from(e.magnitude)),
                        XVal::Num((e.duration - e.elapsed).max(0.0)),
                        XVal::Ref(e.caster.map_or(0, |c| c.0)),
                        XVal::Num(e.duration),
                    ]
                })
                .collect();
            // None listed gives no array [observed: gaps7 tempeffects.player, ar_Size -1;
            // L5.ext rerun, #2040].
            if rows.is_empty() {
                return Some(Ok(zero));
            }
            let mut outer = Vec::with_capacity(rows.len());
            for (i, r) in rows.into_iter().enumerate() {
                let inner = r.into_iter().enumerate().map(|(k, v)| (fnv_script::vm::XKey::Num(k as f64), v)).collect();
                outer.push((fnv_script::vm::XKey::Num(i as f64), XVal::Array(g.ext.new_array(fnv_script::vm::ArrayKind::List, inner))));
            }
            XVal::Array(g.ext.new_array(fnv_script::vm::ArrayKind::List, outer))
        }
        // `[on]`: game time stands still while on; with no argument only reads it
        // [documented: FreezeTime; hypothesis: the engine's animations and physics run on].
        "freezetime" => {
            if let Some(XVal::Num(n)) = args.first() {
                g.gaps.jip3.time_frozen = *n != 0.0;
            }
            yes(g.gaps.jip3.time_frozen)
        }
        // `set|remove function flag [filter]`: JohnnyGuitar's handlers kept here
        // (`Game::jg_event`) [documented: the SetJohnnyOn…EventHandler pages].
        // `XMarker` (`0x0000_003B`) as wildcard filter is [observed 2026-11-24:
        // `w6t2p2_09_jg_on_dying_all_filters`] for `JgEvent::Dying` and [hypothesis] for
        // `JgEvent::CompleteQuest` and `JgEvent::ChallengeComplete`.
        "setjohnnyonrendereventhandler" | "setjohnnyonrenderupdateeventhandler" | "setjohnnyondyingeventhandler" | "setjohnnyoncompletequesteventhandler" | "setjohnnyonchallengecompleteeventhandler" => {
            let ev = match name {
                "setjohnnyondyingeventhandler" => JgEvent::Dying,
                "setjohnnyoncompletequesteventhandler" => JgEvent::CompleteQuest,
                "setjohnnyonchallengecompleteeventhandler" => JgEvent::ChallengeComplete,
                _ => JgEvent::RenderUpdate,
            };
            let Some(f) = args.get(1).cloned() else { return Some(Ok(zero)) };
            let filter = form(args, 3).filter(|&id| id != FormId(0x0000_003B));
            g.gaps.jip3.jg_handlers.retain(|(e, h, l)| !(*e == ev && *h == f && *l == filter));
            if num(args, 0, 1.0) != 0.0 {
                g.gaps.jip3.jg_handlers.push((ev, f, filter));
            }
            zero
        }
        _ => return None,
    };
    Some(Ok(v))
}

/// A menu's pick as a script sees it: an inventory reference for an item in a container,
/// else the form.
fn pick_val(g: &mut Game, p: Option<crate::game::MenuPick>) -> XVal {
    match p {
        Some(crate::game::MenuPick::Item { owner, item }) => XVal::Ref(g.temp_ref(owner, item).0),
        Some(crate::game::MenuPick::Form(f)) => XVal::Ref(f.0),
        None => XVal::Ref(0),
    }
}

/// `GetAnglesBetweenPoints` `(pitch, heading)` in degrees: pitch up-positive (`+Z`),
/// heading counter-clockwise from north (`+Y`) in `(-180, 180]` [observed:
/// `w6_t1_01_angles_between_points`, `docs/spec-notes/jip-angles-ex.md`].
pub fn angles_between_points(p0: [f64; 3], p1: [f64; 3]) -> [f64; 2] {
    let (dx, dy, dz) = (p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]);
    let pitch = dz.atan2(dx.hypot(dy)).to_degrees() + 0.0;
    let heading = (-dx + 0.0).atan2(dy).to_degrees() + 0.0;
    [pitch, heading]
}

/// `GetAngleEx(..., getLocal=1)` Euler angles (degrees) from world radians `rot`: ZYX
/// decomposition of `Rx(-x) * Ry(-y) * Rz(-z)` [observed:
/// `w6_t1_02_angle_ex_and_heading`, `docs/spec-notes/jip-angles-ex.md`].
pub fn local_euler_degrees(rot: [f32; 3]) -> [f64; 3] {
    let (sx, cx) = (-f64::from(rot[0])).sin_cos();
    let (sy, cy) = (-f64::from(rot[1])).sin_cos();
    let (sz, cz) = (-f64::from(rot[2])).sin_cos();
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    let mul = |a: [[f64; 3]; 3], b: [[f64; 3]; 3]| {
        let mut out = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                out[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
            }
        }
        out
    };
    let m = mul(rx, mul(ry, rz));
    let ox = m[2][1].atan2(m[2][2]).to_degrees() + 0.0;
    let oy = (-m[2][0]).clamp(-1.0, 1.0).asin().to_degrees() + 0.0;
    let oz = m[1][0].atan2(m[0][0]).to_degrees() + 0.0;
    [ox, oy, oz]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_between_points_matches_observed_jip_vectors() {
        let cases: [([f64; 3], [f64; 3], [f64; 2]); 14] = [
            ([0.0, 0.0, 0.0], [0.0, 100.0, 0.0], [0.0, 0.0]),
            ([0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, -90.0]),
            ([0.0, 0.0, 0.0], [-100.0, 0.0, 0.0], [0.0, 90.0]),
            ([0.0, 0.0, 0.0], [0.0, -100.0, 0.0], [0.0, 180.0]),
            ([0.0, 0.0, 0.0], [0.0, 0.0, 100.0], [90.0, 0.0]),
            ([0.0, 0.0, 0.0], [0.0, 0.0, -100.0], [-90.0, 0.0]),
            ([0.0, 0.0, 0.0], [100.0, 100.0, 0.0], [0.0, -45.0]),
            ([0.0, 0.0, 0.0], [-100.0, 100.0, 0.0], [0.0, 45.0]),
            ([0.0, 0.0, 0.0], [100.0, -100.0, 0.0], [0.0, -135.0]),
            ([0.0, 0.0, 0.0], [-100.0, -100.0, 0.0], [0.0, 135.0]),
            ([0.0, 0.0, 0.0], [0.0, 100.0, 100.0], [45.0, 0.0]),
            ([0.0, 0.0, 0.0], [0.0, 100.0, -100.0], [-45.0, 0.0]),
            ([10.0, 20.0, 30.0], [110.0, 20.0, 130.0], [45.0, -90.0]),
            ([50.0, 50.0, 50.0], [50.0, 50.0, 50.0], [0.0, 0.0]),
        ];
        for (p0, p1, want) in cases {
            let got = angles_between_points(p0, p1);
            assert!((got[0] - want[0]).abs() < 1e-4 && (got[1] - want[1]).abs() < 1e-4, "{p0:?}->{p1:?}: got {got:?}, want {want:?}");
        }
    }

    #[test]
    fn get_angle_ex_local_matches_observed_rotations() {
        let cases: [([f32; 3], [f64; 3]); 5] = [
            ([0.0, 0.0, 5.70], [0.0, 0.0, -5.70]),
            ([25.0, 0.0, 0.0], [-25.0, 0.0, 0.0]),
            ([0.0, 0.0, 90.0], [0.0, 0.0, -90.0]),
            ([30.0, 0.0, 90.0], [0.0, -30.0, -90.0]),
            ([25.0, 0.0, 315.0], [-18.25, 17.39, 42.19]),
        ];
        for (deg, want) in cases {
            let got = local_euler_degrees(deg.map(f32::to_radians));
            for i in 0..3 {
                assert!((got[i] - want[i]).abs() < 0.02, "{deg:?} axis {i}: got {got:?}, want {want:?}");
            }
        }
    }
}
