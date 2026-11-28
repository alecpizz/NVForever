//! Player functions: experience, perks, notes, tag skills, achievements, controls.

use super::{none, yes, Args, Ctx};
use crate::game::{FastTravel, Game};
use fnv_script::vm::Value;
use fnv_sim::FormId;

pub fn reward_xp(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    // Muted while co-op replays claimed stages: given live already, or a scene for a
    // player who wasn't there (`crate::coop_joint`) [design].
    if g.coop_replaying {
        return none();
    }
    let n = a.num(0)?.trunc();
    // The revision's XP given back: the experience was never taken (`crate::revision`).
    if crate::revision::keeps(ctx, FormId::PLAYER_REF, || format!("the experience ({n} XP not given again)")) {
        return none();
    }
    crate::experience::award(g, crate::experience::Source::Script, n);
    // For co-op guests: part of a completed quest's reward, or script XP of its own
    // (`crate::coop_rewards`). A challenge's reward is the host's own.
    if !matches!(ctx.inst, Some(crate::game::InstKey::Challenge { .. })) {
        g.coop_rewards.gain(g.frame, n, 0);
    }
    none()
}

/// Perks are kept per actor: the player's own (`Player::perks`, traits included), and an
/// NPC's or creature's given to it with `<actor>.AddPerk perk` (`RefExtra::perks`). On
/// the player, a non-zero forTeammates (the second parameter) asks about, gives or takes
/// the perk of the player's teammates instead [documented: GECK wiki AddPerk and HasPerk].
/// HasPerk on another actor reads what it holds ([`Game::held_perks`]: its own perks and,
/// while it is a teammate, the teammate perks) [documented: GECK wiki AddPerk, "adds a
/// perk to the calling actor"; observed: FalloutNV.esm's companion hire lines call
/// `<companion>.AddPerk CompanionSuite`, the firing scripts `RemovePerk CompanionSuite`,
/// and mine and trap scripts ask `ActionRef.HasPerk LightStep` and `Self.HasPerk
/// Stonewall` of NPCs; hypothesis: an NPC's perk has no level-up side (no Quest + Stage
/// effects run for it), and the forTeammates flag means nothing on an NPC].
pub fn has_perk(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    let perk = a.form(0)?;
    if r != FormId::PLAYER_REF {
        return yes(g.held_perks(r).iter().any(|&(p, _)| p == perk));
    }
    if a.num_or(1, 0.0) != 0.0 {
        return yes(g.player.teammate_perks.contains(&perk));
    }
    yes(g.player.perks.contains_key(&perk))
}

pub fn add_perk(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    let perk = a.form(0)?;
    if r != FormId::PLAYER_REF {
        let max = g.data.perks.get(&perk).map_or(1, |p| p.ranks.max(1));
        let rank = g.extra.entry(r).or_default().perks.entry(perk).or_default();
        *rank = rank.saturating_add(1).min(max);
        tracing::info!("perks: {} gains {} (rank {rank})", g.data.name(r), g.data.name(perk));
        return none();
    }
    // A companion perk (a personal quest's) is the host's alone [design]: a co-op guest's game never
    // gives it, nor its own world replaying what it claimed as a guest (`crate::coop_uniques`).
    let teammates = a.num_or(1, 0.0) != 0.0;
    if !teammates && (g.coop_guest || g.coop_replaying) && g.coop_uniques().is_companion_perk(perk) {
        tracing::info!("co-op: {} is a companion perk, the host's alone: not given to this guest", g.data.name(perk));
        return none();
    }
    if teammates {
        g.player.teammate_perks.insert(perk);
    } else {
        let before = g.player.perks.get(&perk).copied().unwrap_or(0);
        crate::perks::gain_rank(g, perk);
        let after = g.player.perks.get(&perk).copied().unwrap_or(0);
        if after != before {
            if g.story_counts(ctx) {
                g.coop_story.record_perk(perk, before);
                tracing::info!("co-op story: {} gave the player perk {}", ctx.unit, g.data.name(perk));
            }
            g.perk_event(perk, true, before, after);
        }
    }
    none()
}

pub fn remove_perk(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    let perk = a.form(0)?;
    if r != FormId::PLAYER_REF {
        if g.extra.get_mut(&r).and_then(|x| x.perks.remove(&perk)).is_some() {
            tracing::info!("perks: {} loses {}", g.data.name(r), g.data.name(perk));
            // Its abilities end now (the per-frame sync only looks at actors that hold
            // perks or are teammates).
            g.sync_perk_abilities_of(r);
        }
        return none();
    }
    if crate::revision::keeps_perk(g, ctx, FormId::PLAYER_REF, perk) {
        return none();
    }
    if a.num_or(1, 0.0) != 0.0 {
        g.player.teammate_perks.remove(&perk);
    } else if let Some(before) = g.player.perks.remove(&perk) {
        if g.story_counts(ctx) {
            g.coop_story.record_perk(perk, before);
            tracing::info!("co-op story: {} took perk {} from the player", ctx.unit, g.data.name(perk));
        }
        g.perk_event(perk, false, 0, 0);
    }
    none()
}

/// `GetHasNote note`: the player has the note: added with AddNote, or carried as an
/// item (AddItem, picked up) [observed: `player.AddItem` of a holotape made GetHasNote 1
/// while GetItemCount of it read 0, capture `fa_batch3_actions`; the GECK wiki
/// GetItemCount notes say it "does not work with Notes"].
pub fn get_has_note(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let note = a.form(0)?;
    let carried = g.world.item_count(FormId::PLAYER_REF, note).unwrap_or(0) > 0;
    yes(carried || g.player.notes.contains(&note))
}

pub fn add_note(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    g.file_note(a.form(0)?);
    none()
}

pub fn is_hardcore(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    yes(g.player.hardcore.unwrap_or(g.world.config().hardcore))
}

pub fn is_player_tag_skill(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    yes(g.player.tag_skills.contains(&a.code(0)?))
}

/// `SetPlayerTagSkill skill index`: the skill becomes the tag skill in slot `index`
/// (0–3), and the skill that slot held before, if no other slot holds it, is no
/// longer tagged; an index above 3 does nothing [documented: GECK wiki
/// SetPlayerTagSkill, "Sets a specified skill to be the tag skill in a specified index
/// (0-3)", "an index higher than 3 seems to have no effect"]. Without an index the skill
/// is tagged without a slot [hypothesis: the index is required].
pub fn set_player_tag_skill(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let skill = a.code(0)?;
    let p = &mut g.player;
    if let Ok(slot) = a.num(1) {
        if !(0.0..4.0).contains(&slot) {
            return none();
        }
        if let Some(old) = p.tag_slots.insert(slot as u8, skill) {
            if old != skill && !p.tag_slots.values().any(|&s| s == old) {
                p.tag_skills.remove(&old);
            }
        }
    }
    p.tag_skills.insert(skill);
    none()
}

pub fn add_achievement(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    // Muted while co-op replays claimed stages: given live already, or a scene for a
    // player who wasn't there (`crate::coop_joint`) [design].
    if g.coop_replaying {
        return none();
    }
    g.player.achievements.insert(a.int(0)?);
    none()
}

/// The seven control groups: movement, Pip-Boy, fighting, POV, looking, rollover text,
/// sneaking. With no parameters `DisablePlayerControls` disables the first four and
/// `EnablePlayerControls` enables all seven [documented: GECK wiki pages of both].
/// A parameter left out of a short list takes `missing` (docs/spec-notes/controls.md §5.2).
fn controls(a: &Args, default: [bool; 7], missing: bool) -> [bool; 7] {
    if a.is_empty() {
        return default;
    }
    let mut out = [false; 7];
    for (i, o) in out.iter_mut().enumerate() {
        *o = if i < a.len() { a.num_or(i, 0.0) != 0.0 } else { missing };
    }
    out
}

pub fn disable_player_controls(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    // A left-out parameter is 0: the group is left as it is [hypothesis; §5.2 has a
    // capture that does not fit any simple rule].
    let which = controls(a, [true, true, true, true, false, false, false], false);
    // A co-op guest runs no world scripts but what it sets off itself (a door's
    // OnActivate): their locks would never be taken off here, where the script that does
    // so runs only in the host's world. A guest is held only by the host's scenes
    // (`Game::coop_host_locks`) [design].
    if g.coop_guest {
        tracing::info!("co-op: {} would disable this guest's controls {which:?}: not here (only the host's scenes hold a guest)", ctx.unit);
        return none();
    }
    let story = g.is_story_caller(ctx);
    for ((d, s), w) in g.player.controls_disabled.iter_mut().zip(g.story_locked.iter_mut()).zip(which) {
        *d |= w;
        *s |= w && story;
    }
    // Who holds the controls, for the players' reports (#3044).
    if which.iter().any(|w| *w) {
        g.player.controls_disabled_by = Some(crate::game::ControlsCaller { unit: ctx.unit.to_string(), quest: ctx.quest, reference: ctx.on.or(ctx.self_ref) });
    }
    none()
}

pub fn enable_player_controls(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    // A left-out parameter enables its group, as the form without parameters does
    // [hypothesis: the trigger outside Doc Mitchell's door,
    // VCG01RemoveDocCarebearModeTriggerSCRIPT, gives the controls back with
    // `EnablePlayerControls 1 1 1 1 1 1`, and no other script gives sneaking back after
    // the opening turned it off; read with a left-out 0 the player could never sneak
    // again. controls.md §5.2].
    let which = controls(a, [true; 7], true);
    for ((d, s), w) in g.player.controls_disabled.iter_mut().zip(g.story_locked.iter_mut()).zip(which) {
        if w {
            *d = false;
            *s = false;
        }
    }
    if !g.player.controls_disabled.iter().any(|d| *d) {
        g.player.controls_disabled_by = None;
    }
    none()
}

/// 1 when any of the selected groups is disabled (all groups when none is selected)
/// [documented: catalogue summary], by DisablePlayerControls or any plugin's
/// DisablePlayerControlsAlt [observed: nvse-fifth, an Alt movement lock reads 1].
pub fn get_player_controls_disabled(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let which = controls(a, [true; 7], false);
    yes(g.controls_locked().iter().zip(which).any(|(d, w)| *d && w))
}

/// An actor's current level [documented: GECK wiki GetLevel]: the player's SetLevel,
/// else the configuration's; another actor's SetLevel, else its record's level (fixed,
/// or PC Level Mult with its bounds: `Game::actor_level`). [observed: Sunny Smiles read 2
/// and Trudy 5 in the capture `fa_batch1_queries`.]
pub fn get_level(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    if r != FormId::PLAYER_REF {
        return Ok(Value::Num(g.actor_level(r).round()));
    }
    Ok(Value::Num(g.player.level.unwrap_or(g.world.config().player_level as i32) as f64))
}

/// `RemoveNote note`: the player no longer has it, carried or added.
pub fn remove_note(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let note = a.form(0)?;
    g.player.notes.remove(&note);
    let carried = g.world.item_count(FormId::PLAYER_REF, note).unwrap_or(0);
    if carried > 0 {
        let _ = g.world.remove_item(FormId::PLAYER_REF, note, carried);
    }
    none()
}

pub fn reset_xp(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let xp = g.player.xp;
    if crate::revision::keeps(ctx, FormId::PLAYER_REF, || format!("the experience ({xp} XP)")) {
        return none();
    }
    g.player.xp = 0.0;
    none()
}

/// Axis 0 is infamy, 1 fame [documented: GECK wiki GetReputation]; any other axis is
/// `None`: see [`super::reputation::axis`].
fn rep_axis(a: &Args) -> Result<Option<usize>, String> {
    super::reputation::axis(a, 1)
}

pub fn get_reputation(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = a.form(0)?;
    let Some(i) = rep_axis(a)? else { return Ok(Value::ZERO) };
    Ok(Value::Num(g.player.reputation.get(&r).map(|v| v[i]).unwrap_or(0.0)))
}

/// Sets the amount exactly, with no limit [documented: GECK wiki SetReputation;
/// observed: −3, 2.5 and twice the scale were stored as given, layer5-captures.md
/// §9.3 a]. An axis other than 0 or 1 fails and stops the script
/// ([`super::reputation::write_axis`]).
pub fn set_reputation(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = a.form(0)?;
    let i = super::reputation::write_axis(g, a, 1)?;
    super::reputation::store(g, r, i, a.num(2)?);
    none()
}

pub fn set_hardcore(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    g.set_hardcore(a.num(0)? != 0.0);
    none()
}

/// `ShowMap marker [canTravel]`: the marker goes on the world map, and with canTravel
/// becomes a fast-travel target [documented: GECK wiki ShowMap].
pub fn show_map(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let m = a.form(0)?;
    g.player.map_markers.insert(m);
    if g.marker_flag_override(m, crate::nvse_lu2::MARKER_VISIBLE) == Some(false) {
        g.clear_marker_flag_override(m, crate::nvse_lu2::MARKER_VISIBLE);
    }
    if a.num_or(1, 0.0) != 0.0 {
        g.player.travel_markers.insert(m);
        if g.marker_flag_override(m, crate::nvse_lu2::MARKER_TRAVEL) == Some(false) {
            g.clear_marker_flag_override(m, crate::nvse_lu2::MARKER_TRAVEL);
        }
    }
    none()
}

/// Whether a marker is on the map by the scripts' word (ShowMap; ShowAllMapMarkers),
/// with its "Show All" Hidden flag. Markers the player found, or authored Visible, are
/// the engine's to add.
impl Game<'_> {
    pub fn map_marker_shown(&self, marker: FormId, hidden: bool) -> bool {
        self.player.map_markers.contains(&marker) || self.player.all_markers.is_some_and(|a| a.shown && !(a.skip_hidden && hidden))
    }

    /// The map markers the player has found (the engine's discovery, kept in its save
    /// too): they are on the map and fast-travel targets.
    pub fn set_found_markers(&mut self, found: impl IntoIterator<Item = FormId>) {
        self.player.found_markers = found.into_iter().collect();
    }
}

/// `marker.GetMapMarkerVisible`: 0 not on the map, 1 on it without fast travel, 2 on it
/// and a fast-travel target [documented: GECK wiki GetMapMarkerVisible, "Marker State"].
/// Before discovery, authored `FNAM` flags read 0 (`0`), 1 (`1` Visible), 2 (`3`
/// Visible | Can Travel To, e.g. Honest Hearts' Southern Passage), and 0 (`2` Can Travel
/// To alone); `ShowMap 0` gives 1 (and leaves a marker already at 2 at 2), `ShowMap 1`
/// and player discovery give 2 (and set `GetMapMarkerTravel` to 1); `SetMapMarkerVisible`
/// sets/clears only bit 0, `SetMapMarkerTravel 1` alone keeps 0 while setting travel,
/// `SetMapMarkerTravel 0` drops 2 to 1, and `SetMapMarkerHidden 1` does not zero it
/// [observed: observer check 252 (#3468, `o31_mapmarker_visible` control + probe plugin)
/// and the 2026-11-22 live map-marker probe documented in `docs/spec-notes/ext-lutana.md` §9].
pub fn get_map_marker_visible(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let flags = g.marker_flags(r);
    let hidden = flags.is_some_and(|f| f & crate::nvse_lu2::MARKER_HIDDEN != 0);
    let visible_flag = flags.is_some_and(|f| f & crate::nvse_lu2::MARKER_VISIBLE != 0);
    let script_visible = g.marker_flag_override(r, crate::nvse_lu2::MARKER_VISIBLE);
    let found = g.player.found_markers.contains(&r);
    let shown = g.map_marker_shown(r, hidden);
    let visible = match script_visible {
        Some(false) => shown,
        _ => found || shown || visible_flag,
    };
    if !visible {
        return Ok(Value::Num(0.0));
    }
    let travel = if flags.is_some() {
        g.marker_travel(r)
    } else {
        found || (shown && (g.player.travel_markers.contains(&r) || g.player.all_markers.is_some_and(|a| a.shown && a.travel)))
    };
    Ok(Value::Num(if travel { 2.0 } else { 1.0 }))
}

/// `ShowAllMapMarkers show [canTravel] [skipHidden]`: every map marker on the world map
/// (1) or off it (0); the optional flags default to 1 [documented: GECK wiki
/// ShowAllMapMarkers, "1, 1, 1 … same as first example"]. Hiding takes travel away
/// whatever canTravel says, and showing with canTravel 0 does not take it from markers
/// that had it [documented]; the Pip-Boy's fast travel reads `travel`
/// (`nvf_play::pipboy::data::travel`). The
/// Explorer perk's quest calls `ShowAllMapMarkers 1 0 0` [observed: QUST 0x000038B2
/// stage 50]. Hiding forgets the markers ShowMap showed; markers the player found and
/// those authored Visible stay on the map [hypothesis: no shipped script hides them,
/// and the engine keeps discovery outside the game state].
pub fn show_all_map_markers(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let shown = a.num(0)? != 0.0;
    let travel = shown && (a.num_or(1, 1.0) != 0.0 || g.player.all_markers.is_some_and(|m| m.travel));
    let skip_hidden = a.num_or(2, 1.0) != 0.0;
    if !shown {
        g.player.map_markers.clear();
    }
    g.player.all_markers = Some(crate::game::AllMarkers { shown, travel, skip_hidden });
    none()
}

/// The total experience the player needs to have reached `level`:
/// (n − 1)((n − 2)/2 · iXPBumpBase + iXPBase) [documented: GECK wiki Experience
/// Settings]. The settings default to 200 and 150 [documented: same page] when the load
/// order lacks them.
pub fn xp_for_level(g: &Game, level: i32) -> f64 {
    crate::experience::xp_for_level(g, level)
}

/// `GetXPForNextLevel`: the experience the player still needs to reach the next level
/// [documented: GECK wiki, "the amount of XP the player needs before reaching the next
/// level"]: the next level's total ([`xp_for_level`]) less the experience the player
/// has, at least 0 [hypothesis: remaining rather than the total; the Here and Now perk's
/// stage rewards exactly this amount, `QUST 0x000038B2` stage 10, which levels the
/// player up at once only if it is what is left]. The level goes up when the player takes
/// the level-up menu (`crate::experience`).
pub fn get_xp_for_next_level(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let level = g.player.level.unwrap_or(g.world.config().player_level as i32);
    Ok(Value::Num((xp_for_level(g, level + 1) - g.player.xp).max(0.0).round()))
}

/// The player's miscellaneous statistic by code (the source names it as a string; the
/// bytes hold the code, scripts.md §2.4): the counter the rules and the engine keep and
/// ModPCMiscStat adds to (`crate::misc_stats`, docs/spec-notes/misc-stats.md).
pub fn get_pc_misc_stat(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    Ok(Value::Num(g.misc_stat(a.code(0)?)))
}

pub fn mod_pc_misc_stat(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    *g.player.misc_stats.entry(a.code(0)?).or_default() += a.num(1)?;
    none()
}

/// `EnableFastTravel [canTravel canWait keep]`: travel and waiting allowed or not;
/// waiting is forced on whenever travel is allowed; without `keep` an interior →
/// exterior change enables travel again ([`crate::Game::player_entered`])
/// [documented: catalogue notes of EnableFastTravel]. Whether travel is possible at a
/// moment (encumbrance, enemies) is the engine's.
pub fn enable_fast_travel(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let travel = a.num_or(0, 1.0) != 0.0;
    let wait = travel || a.num_or(1, 1.0) != 0.0;
    let keep = a.num_or(2, 0.0) != 0.0;
    // `0 0 1` then `0 0 0`: "Effectively, the second function call is ignored"
    // [documented: GECK wiki EnableFastTravel example]; that a kept ban ignores any later
    // ban without keep is [hypothesis].
    let now = g.player.fast_travel;
    if !travel && !keep && now.keep && now.travel_disabled {
        return none();
    }
    g.player.fast_travel = FastTravel { travel_disabled: !travel, wait_disabled: !wait, keep };
    none()
}

fn challenge(g: &Game, a: &Args) -> Result<FormId, String> {
    let c = a.form(0)?;
    if g.data.challenges.contains_key(&c) {
        Ok(c)
    } else {
        Err(format!("{} is not a challenge", g.data.name(c)))
    }
}

/// A challenge flagged Start Disabled becomes able to progress [documented: catalogue
/// summary of UnlockChallenge]. Progress itself is counted by the challenge system
/// (Layer 6).
pub fn unlock_challenge(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let c = challenge(g, a)?;
    g.player.challenges.entry(c).or_default().unlocked = true;
    none()
}

/// 1 once the challenge was completed (a recurring one at least once) [documented:
/// catalogue summary]. Completion is reported by the challenge system (Layer 6) or a
/// runner input.
pub fn get_challenge_completed(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let c = challenge(g, a)?;
    yes(g.player.challenges.get(&c).is_some_and(|s| s.completed))
}

pub fn remove_recurring_from_challenge(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let c = challenge(g, a)?;
    g.player.challenges.entry(c).or_default().not_recurring = true;
    none()
}

/// `SetInCharGen flag` [documented: catalogue summary]; while it is set no level-up is
/// offered ([`crate::experience::level_up_due`]).
pub fn set_in_chargen(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    g.player.in_chargen = a.num(0)? != 0.0;
    none()
}

pub fn get_in_chargen(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    yes(g.player.in_chargen)
}

/// The player's winnings level (0–4) at a casino (CSNO) [documented: catalogue
/// summaries of GetCasinoWinningsLevel and GetCasinoWinningStage]: set by the casino
/// games (Layer 6) or a runner input; 0 until then. The shared No Casino Bans read
/// view preserves the saved standing (blackjack.md §4).
pub fn get_casino_winnings_level(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    Ok(Value::Num(f64::from(crate::casino::script_winnings_level(g, a.form(0)?))))
}

/// `SetPCCanUsePowerArmor flag`: whether the player may wear power armour (ARMO BMDT
/// "Power Armor"); off at the start. The Power Armor Training perk is a display dummy:
/// its rank sets the Generic quest's stage 80, whose result script calls this with 1
/// [documented: catalogue notes; observed: PERK PowerArmorTraining's Quest + Stage
/// effect `Generic` 0x50, and the stage's script]. The world refuses the armour without
/// it (`fnv_sim::World::set_power_armor_training`), and the engine shows
/// [`refuse_power_armor`]'s box.
pub fn set_pc_can_use_power_armor(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    g.player.power_armor = a.num(0)? != 0.0;
    g.world.set_power_armor_training(g.player.power_armor || g.tweaks.power_armor_needs_no_training);
    none()
}

/// The message the player gets on trying to wear power armour without the training: the
/// masters' `PowerArmorMessage` ("You lack the specialized training needed to move in
/// Power Armor.", a message box) [observed: MESG 00058FEF, the one message of the
/// masters with that text; hypothesis: that the engine shows it on the refusal, as the
/// box flag says it is a box]. It waits for the open menu (the Pip-Boy) to close, as any
/// message box shown under a menu (`Game::show_box`). Returns the message, `None` when
/// the load order has none.
pub fn refuse_power_armor(g: &mut Game) -> Option<FormId> {
    // SetMessageDisabled keeps it from showing (`crate::nvse_jip6`).
    let m = g.data.by_edid("PowerArmorMessage").filter(|&m| !g.message_disabled(m))?;
    let text = g.data.messages.get(&m).and_then(|d| d.text.clone());
    g.emit(crate::trace::EventKind::MessageShown { message: m, text, buttons: Vec::new(), hidden: Vec::new() });
    g.show_box(m);
    Some(m)
}

/// The topic joins the topics the player knows: from then on every actor that can say
/// a line of it offers it in its topic list (`Game::build_choices`)
/// [documented: catalogue summary of AddTopic; owner statement 2026-10-10 for "every
/// actor", not yet captured; docs/spec-notes/dialogue.md §5].
pub fn add_topic(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let topic = a.form(0)?;
    if g.player.topics.insert(topic) {
        g.emit(crate::trace::EventKind::TopicAdded { topic, by: None });
    }
    none()
}

/// 1 when Hardcore mode has been on all game [documented: catalogue summary]: on now
/// and never switched off [hypothesis on what "all game" counts].
pub fn is_always_hardcore(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let on = is_hardcore(g, ctx, a)?.num() != 0.0;
    yes(on && !g.player.hardcore_was_off)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_script::vm::ArgValue;

    fn nums(v: &[f64]) -> Vec<ArgValue> {
        v.iter().map(|&n| ArgValue::Value(Value::Num(n))).collect()
    }

    #[test]
    fn short_parameter_lists() {
        let disable = |v: &[f64]| controls(&Args(&nums(v)), [true, true, true, true, false, false, false], false);
        let enable = |v: &[f64]| controls(&Args(&nums(v)), [true; 7], true);
        assert_eq!(disable(&[]), [true, true, true, true, false, false, false]);
        assert_eq!(disable(&[1.0, 1.0]), [true, true, false, false, false, false, false]);
        assert_eq!(enable(&[]), [true; 7]);
        // The trigger outside Doc Mitchell's door gives sneaking back too.
        assert_eq!(enable(&[1.0; 6]), [true; 7]);
        assert_eq!(enable(&[0.0, 1.0, 1.0]), [false, true, true, true, true, true, true]);
        assert_eq!(enable(&[1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0]), [true, false, false, true, true, true, false]);
    }
}
