//! What each actor's body is doing, as the engine tells the rules every frame
//! (docs/design/script-stubs.md §3.4): the special animation action (GetAnimAction), the
//! way it asks to move (IsMoving), the movement group its body plays (IsAnimPlaying) and,
//! for the player, the actions IsPlayerActionActive asks about.
//!
//! The engine animates; this module keeps the latest snapshot per actor
//! ([`AnimState`], `Senses::anim`) and the rules that turn an attack's or a reload's time
//! into GetAnimAction's codes ([`shot_action`], [`swing_action`]), so the engine's player
//! side (`nvf-play` combat) and NPC side (`nvforever` anim_state.rs) number them alike. A
//! snapshot holds until the engine tells the next one or forgets the actor (it left the
//! view: unloaded, hidden, dying), never by age: a clock-based expiry read Chet's
//! snapshot as gone while the console was open (smoke `anim-state`, 2026-10-18). With
//! no engine (a headless run) nothing is told and every actor reads as doing nothing
//! [engineering choice].
//!
//! The codes are New Vegas's [documented: GECK wiki GetAnimAction, the table "revised for
//! GECK NV"], checked against the AI observer's campaigns [observed: `anim_action` of the
//! Goodsprings gunfight and the melee-spacing runs, C:\fnv-private\capture,
//! docs/spec-notes/anim-state.md]. 10 and 11 are never answered; 12 (the player's
//! landing) and 17, which no source explains, as hypotheses (notes of [`AnimAction`]).

use crate::functions::{none, yes, Args, Ctx};
use crate::game::Game;
use fnv_script::vm::Value;
use fnv_sim::FormId;

/// GetAnimAction's codes [documented: GECK wiki GetAnimAction, New Vegas table]. Never
/// answered: 10 (dodge) and 11 (wait for the lower body): the page says 10–12 "may be
/// left overs", and no actor of the observer's fights read them [observed: Goodsprings
/// gunfight and the 15 melee-spacing runs, docs/spec-notes/anim-state.md §1]. 12 the
/// player read after a jump ([`AnimAction::WaitForSpecialIdle`]). 17, which the observer saw on .357 revolvers before
/// a reload's 9, has no documented meaning: NVForever answers it while a round-by-round
/// reload opens the weapon ([`AnimAction::ReloadOpen`], hypothesis; the observer
/// campaign `anim_actions` settles it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimAction {
    /// 0: drawing the weapon.
    Equip,
    /// 1: putting it away.
    Unequip,
    /// 2: "firing full auto or doing a melee swing"; also the start of a single shot
    /// [observed: 2 for about four frames before each semi-automatic shot's 3].
    Attack,
    /// 3: "returned when firing semi auto" [documented]: the frame of the shot
    /// [observed: one frame for the .357 revolver and the single shotgun; the bolt-action
    /// varmint and hunting rifles held it about 0.45 s, through the bolt, which is not
    /// reproduced: hypothesis left].
    FollowThrough,
    /// 4: "returning to the static animation after the attack" [documented]: the rest
    /// of a shot's or a swing's clip [observed: about 0.33 s after a shot, after each
    /// creature's and the baseball bat's 2], and "drawing another throw-able weapon after
    /// one was just thrown" [documented].
    AttackReturn,
    /// 5: "gearing up to throw" [documented]: a thrown weapon's clip until it leaves the
    /// hand ([`throw_action`]).
    ThrowWindUp,
    /// 6: "the throwable weapon has left the player's hand" [documented]: for
    /// [`THROWN_SECONDS`] after the throw [observed: troopers throwing dynamite read 5
    /// for 52–58 samples, then 6 for 11–40 (median 36), then 4 or −1; campaign
    /// `anim_actions`, 2026-10-18, docs/spec-notes/anim-state.md §1].
    Thrown,
    /// 7: blocking.
    Block,
    /// 8: "staggered by an npc that is blocking" [documented]: the attacker's recoil.
    Stagger,
    /// 9: reloading [documented; observed: about two seconds after a magazine's last
    /// shot, every gun of the gunfight].
    Reload,
    /// 9: weapon jam (`GetAnimAction == 9`, `IsAnimPlayingEx 4 == 0`, fires JIP's
    /// `SetOnReloadWeaponEventHandler`, and `GetSequenceAnimGroup 4 == 200` `JamA` for
    /// 10mm Pistol) [observed: Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, JIP LN 57.30,
    /// JohnnyGuitarNVSE 528, ShowOffNVSE Plugin 184, 2026-11-23/24, GSDocMitchellHouse
    /// (0x00103DF9), 10 official base masters, probes `w1_3632_onfireweapon_and_forcejam`
    /// (#3679) and `w6t2_14_jip_on_reload_and_seq_anim_group`
    /// (`tools/capture/nvse/catalogs/wave6c_jip_on_reload_and_seq_group.json`:
    /// `fj=1,relHit=1,s0=0,s4(pre=24,t0=200,t1=200,cb=200)`); hypothesis: natural vanilla
    /// weapon-condition jams (`Phase::Jammed`) likewise report `9` instead of `13`].
    Jam,
    /// 12: ANIM_ACTION_WAIT_FOR_SPECIAL_IDLE [documented: the name]. No NPC of the
    /// campaign `anim_actions` read 10–12 [observed]. The player read 12
    /// for about 49 frames, starting about 50 frames after a jump [observed: capture
    /// `player_actions_anim`, 2026-10-18]: NVForever answers it while the player's landing
    /// clip plays [hypothesis: that the landing is what 12 marks; the timing fits a jump's
    /// time in the air].
    WaitForSpecialIdle,
    /// 13: "every action not included in the ones above": furniture, PlayIdle idles,
    /// staggering from crippled limbs, getting up [documented].
    ForceScriptAnim,
    /// 17 (not in the page's table): a round-by-round reload opening the weapon (the
    /// `reloadxstart` clip of the .357 revolver and the lever-actions), before its rounds'
    /// 9. Where it falls is [observed: campaign `anim_actions`, 2026-10-18: every reload of
    /// the .357 (113–115 samples) and of the hunting revolver (75–81) read 17 between the
    /// last shot's 4 and the reload's 9; the 9mm pistol went from 4 straight to 9]; that
    /// it is the opening is [hypothesis]: the opening clip `1hpreloadxstart.kf` lasts
    /// 1.07 s, shorter than the 1.3–1.9 s the original held 17 [observed: `fnvtool nif
    /// anim`].
    ReloadOpen,
}

impl AnimAction {
    pub fn code(self) -> i32 {
        match self {
            AnimAction::Equip => 0,
            AnimAction::Unequip => 1,
            AnimAction::Attack => 2,
            AnimAction::FollowThrough => 3,
            AnimAction::AttackReturn => 4,
            AnimAction::ThrowWindUp => 5,
            AnimAction::Thrown => 6,
            AnimAction::Block => 7,
            AnimAction::Stagger => 8,
            AnimAction::Reload | AnimAction::Jam => 9,
            AnimAction::WaitForSpecialIdle => 12,
            AnimAction::ForceScriptAnim => 13,
            AnimAction::ReloadOpen => 17,
        }
    }
}

/// IsMoving's codes [documented: GECK wiki IsMoving].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MoveDir {
    #[default]
    None,
    Forward,
    Backward,
    Left,
    Right,
}

impl MoveDir {
    pub fn code(self) -> u8 {
        match self {
            MoveDir::None => 0,
            MoveDir::Forward => 1,
            MoveDir::Backward => 2,
            MoveDir::Left => 3,
            MoveDir::Right => 4,
        }
    }

    /// The way of a movement in the actor's own axes (`right`, `forward`, any units):
    /// the larger of the two, forward and back on a tie [hypothesis: the page gives one
    /// code for a diagonal]; none when both are below `min`.
    pub fn of(right: f32, forward: f32, min: f32) -> MoveDir {
        if right.abs() < min && forward.abs() < min {
            MoveDir::None
        } else if forward.abs() >= right.abs() {
            if forward > 0.0 {
                MoveDir::Forward
            } else {
                MoveDir::Backward
            }
        } else if right > 0.0 {
            MoveDir::Right
        } else {
            MoveDir::Left
        }
    }
}

/// One actor's body as the engine last told it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnimState {
    /// GetAnimAction: the special animation action; `None` reads −1.
    pub action: Option<AnimAction>,
    /// IsMoving: the way the actor asks to move (the player: the movement keys; an NPC:
    /// the way its body walks).
    pub moving: MoveDir,
    /// The movement group its body plays (`Idle`, `Forward`, `FastLeft`: the clip's
    /// sequence name, `fnv_actor::locomotion::Move::group`), for IsAnimPlaying.
    pub group: Option<&'static str>,
    /// The player only: aiming down a gun's sights (IsPlayerActionActive 9).
    pub iron_sights: bool,
    /// The player only: within [`THROW_ACTION_SECONDS`] of a throw's start
    /// (IsPlayerActionActive 2, or 4 for a mine).
    pub throwing: bool,
    /// The player only: the throw is a mine being laid (IsPlayerActionActive 4).
    pub mine: bool,
    /// Its hit reaction plays because a power attack forced it (GetForceHitReaction).
    pub forced_hit: bool,
    /// The file of the clip its body plays, as the engine loaded it (a path under
    /// meshes, `characters\_male\locomotion\mtidle.kf`) (kNVSE's
    /// GetPlayingAnimSequencePaths, IsAnimSequencePlaying); `None` when not told.
    pub clip: Option<String>,
    /// When ([`Game::time`]) it was told (for traces; it does not expire).
    pub at: f64,
}

/// GetAnimAction of a shot `t` seconds into its clip of `len` seconds, the shot leaving
/// at `fire` (0: at the start), with this frame's step `dt`: 2 until the shot (at least
/// the first frame), 3 the frame after, 4 for the rest of the clip, then none
/// [observed: 2 for a few frames, 3 one frame, 4 about 0.33 s on semi-automatic guns;
/// hypothesis: the boundaries are the clip's shot and end]. `repeating`: an automatic
/// weapon's shots keep coming (the trigger held, a looping clip in its loop): 2
/// throughout [documented: 2 is "firing full auto"].
pub fn shot_action(t: f32, fire: f32, len: f32, dt: f32, repeating: bool) -> Option<AnimAction> {
    let shot = fire.max(dt);
    if repeating || t < shot {
        Some(AnimAction::Attack)
    } else if t < shot + dt {
        Some(AnimAction::FollowThrough)
    } else if t < len {
        Some(AnimAction::AttackReturn)
    } else {
        None
    }
}

/// How long IsPlayerActionActive 2 (a grenade) or 4 (a mine) stays 1 from the start of a
/// throw, seconds [observed: 181 game loops on both of the player's grenade throws,
/// capture `player_actions_anim2` 2026-10-18, and on a laid frag mine, AI observer
/// campaign `functions_round` 2026-10-19, there 3.02 s by the game clock from the first
/// loop of its 5 (the mine's clip: 5 for 48 loops, 6 for 30, 4 for 48, then 55 more)].
pub const THROW_ACTION_SECONDS: f32 = 3.0;

/// How long a throw reads 6 after the weapon left the hand when its clip has no `Attach`
/// key, seconds [observed: median 36 samples, 11–40, at about 60 a second; campaign
/// `anim_actions`].
pub const THROWN_SECONDS: f32 = 0.6;

/// GetAnimAction of a throw `t` seconds into its clip of `len` seconds: 5 until the
/// weapon leaves the hand at `release` (the clip's `Release` key; at least one frame,
/// `dt`, in), 6 until the next one is in hand at `attach` (the clip's `Attach` key, else
/// [`THROWN_SECONDS`] after the release), then 4 to the clip's end, then none
/// [documented: GECK wiki GetAnimAction, 5, 6 and 4; observed: the player's grenade
/// throws read 5 for 50–52 loops, 6 for 39–41, 4 for 16–17 (capture
/// `player_actions_anim2`), where `1gtattackthrow.kf` has `Release` at 0.867 s, `Attach`
/// at 1.533 s and ends at 1.7 s].
pub fn throw_action(t: f32, release: f32, attach: Option<f32>, len: f32, dt: f32) -> Option<AnimAction> {
    let release = release.max(dt);
    let back = attach.filter(|a| *a > release).unwrap_or(release + THROWN_SECONDS);
    if t < release {
        Some(AnimAction::ThrowWindUp)
    } else if t < back {
        Some(AnimAction::Thrown)
    } else if t < len {
        Some(AnimAction::AttackReturn)
    } else {
        None
    }
}

/// GetAnimAction of a melee swing (or a creature's blow) `t` seconds into its clip of
/// `len` seconds that strikes at `hit`: 2 until the strike, 4 after it [observed: the
/// creatures' and the baseball bat's blows read 2, then 4, without 3; hypothesis: the
/// boundary is the clip's `Hit` key].
pub fn swing_action(t: f32, hit: f32, len: f32) -> Option<AnimAction> {
    if t < hit {
        Some(AnimAction::Attack)
    } else if t < len {
        Some(AnimAction::AttackReturn)
    } else {
        None
    }
}

fn is_reload_action(action: Option<AnimAction>) -> bool {
    matches!(action, Some(AnimAction::Reload | AnimAction::ReloadOpen))
}

impl Game<'_> {
    /// The engine tells what an actor's body does now (every frame, for the actors it
    /// draws and the player). Entering a reload action (`Reload` or `ReloadOpen`) from a
    /// non-reload action fires JIP's `SetOnReloadWeaponEventHandler` event once per
    /// reload with the actor's equipped or held weapon [documented:
    /// SetOnReloadWeaponEventHandler; hypothesis: `ReloadOpen` -> `Reload` is one reload].
    /// Also fires JIP's `SetOnAnimActionEventHandler` (`animaction`) on entry into a new
    /// action and `SetOnPlayGroupEventHandler` (`playgroup`) on entry into a new named
    /// locomotion group, after recording the snapshot so handlers querying `GetAnimAction`
    /// or `IsAnimPlaying` see the live state.
    pub fn set_anim_state(&mut self, actor: FormId, mut s: AnimState) {
        let prev = self.senses.anim.get(&actor);
        let prev_action = prev.and_then(|p| p.action);
        let prev_group = prev.and_then(|p| p.group);
        let was_reloading = is_reload_action(prev_action);
        let next_action = s.action;
        let next_group = s.group;
        let now_reloading = is_reload_action(next_action);
        let enter_jam = prev_action != Some(AnimAction::Jam) && next_action == Some(AnimAction::Jam);
        s.at = self.time;
        self.senses.anim.insert(actor, s);
        if (!was_reloading && now_reloading) || enter_jam {
            let weapon = crate::functions::items::equipped_weapon(self, actor)
                .or_else(|| self.extra.get(&actor).and_then(|e| e.held_weapon));
            if let Some(weapon) = weapon {
                self.weapon_reloaded(actor, weapon);
            }
        }
        if next_action != prev_action {
            if prev_action == Some(AnimAction::Equip)
                && !self.extra.get(&actor).is_some_and(|e| e.weapon_out)
                && !self.hud.weapon_due.get(&actor).is_some_and(|(o, _, _)| *o)
            {
                if next_action == Some(AnimAction::Unequip) {
                    self.showoff.unholster_pending.remove(&actor);
                } else if let Some(slot) = self.showoff.unholster_pending.get_mut(&actor) {
                    *slot = Some(self.frame);
                }
            }
            if let Some(action) = next_action {
                if action == AnimAction::Equip {
                    self.showoff_begin_unholster(actor);
                } else if action == AnimAction::Unequip {
                    self.showoff.unholster_pending.remove(&actor);
                }
                self.fire_anim_action(actor, action.code());
            }
        }
        if next_group != prev_group {
            if let Some(code) = next_group.and_then(|name| self.anim_group_code(name)) {
                self.fire_play_group(actor, code);
            }
        }
    }

    /// The actor left the engine's view: forget its body's state.
    pub fn forget_anim_state(&mut self, actor: FormId) {
        self.senses.anim.remove(&actor);
        self.showoff.unholster_pending.remove(&actor);
    }

    /// The actor's body as last told, until the engine forgets it.
    pub fn anim_state(&self, actor: FormId) -> Option<&AnimState> {
        self.senses.anim.get(&actor)
    }
}

/// `actor.GetAnimAction`: the special animation action the actor is doing, −1 for none
/// [documented: GECK wiki GetAnimAction; observed: −1 for the player and Sunny Smiles at
/// rest (capture `fa_batch1_queries`), 0 for Ringo drawing his weapon 3 frames after
/// SetAlert 1 and −1 once it was out (`fa_batch3_actions`)]. Read from the engine's
/// snapshot ([`Game::anim_state`]); an actor it does not draw reads −1.
pub fn get_anim_action(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let code = g.anim_state(r).and_then(|s| s.action).map_or(-1, AnimAction::code);
    Ok(Value::Num(code as f64))
}

/// `actor.GetForceHitReaction`: 1 while the actor plays a hit reaction a power attack
/// forced [documented: GECK wiki GetForceHitReaction, "set … to 1 if an actor was hit by
/// the following power attacks: Khan Trick (AttackCustom4Power and AttackCustom5Power),
/// and the uppercut (Attack6)"]. The engine tells it with the body's state
/// ([`AnimState::forced_hit`]: from the hit until the reaction's clip ends) [hypothesis:
/// how long the original keeps it set]. Its only uses are the conditions of the idles
/// `3rdPersonHitReactionsForce` and `3rdPCrouchedHitForce` [observed: FalloutNV.esm];
/// the engine picks the forced reaction's clip itself (`nvforever` deaths.rs).
pub fn get_force_hit_reaction(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(Value::bool(g.anim_state(r).is_some_and(|s| s.forced_hit)))
}

/// `IsPlayerActionActive index`: 1 while the player performs the action [documented:
/// GECK wiki IsPlayerActionActive]. The page's ActionIndex list is a template the wiki
/// export lacks; what the original answered [observed: capture `player_actions_anim2`,
/// 2026-10-18, every game loop]:
/// - 9 while the player aimed a pistol (out of combat), as the page's
///   PLAYER_ACTION_IRON_SIGHTS; not while the player is in combat [documented: the page's
///   notes]. Both shipped calls (Old World Blues) ask 9.
/// - 2 from the start of a grenade throw for [`THROW_ACTION_SECONDS`].
/// - 4 from the start of a mine's throw (a frag mine laid) for the same time [observed:
///   AI observer campaign `functions_round` 2026-10-19; GetLastPlayerAction read 4 over
///   the same loops and 0 before and after; no shipped script or condition asks it].
/// - Nothing (1–10 all 0) while firing a pistol or a revolver, sneaking, firing while
///   sneaking, jumping, swinging a knife, drawing or holstering, though the page says
///   firing stays active about two game minutes: no index answered it.
///
/// 8 is "standing on a table or chair" by the page's example, which NVForever's player
/// never does. Every other index reads 0.
pub fn is_player_action_active(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let index = a.num_or(0, 0.0) as i32;
    let s = g.anim_state(FormId::PLAYER_REF);
    match index {
        2 => yes(s.is_some_and(|s| s.throwing && !s.mine)),
        4 => yes(s.is_some_and(|s| s.throwing && s.mine)),
        9 => {
            let sights = s.is_some_and(|s| s.iron_sights);
            yes(sights && !g.awareness.hunted(FormId::PLAYER_REF))
        }
        _ => none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_semi_automatic_shot_reads_2_then_3_then_4() {
        let dt = 1.0 / 60.0;
        let seq: Vec<Option<i32>> = (0..30).map(|i| shot_action(i as f32 * dt, 0.05, 0.4, dt, false).map(AnimAction::code)).collect();
        assert_eq!(seq[0], Some(2));
        assert_eq!(seq[2], Some(2));
        let first3 = seq.iter().position(|c| *c == Some(3)).unwrap();
        assert_eq!(seq.iter().filter(|c| **c == Some(3)).count(), 1, "3 lasts one frame");
        assert!(seq[first3 + 1..].iter().take_while(|c| c.is_some()).all(|c| *c == Some(4)));
        assert_eq!(shot_action(0.5, 0.05, 0.4, dt, false), None);
    }

    #[test]
    fn a_shot_leaving_at_the_start_still_reads_2_on_its_first_frame() {
        let dt = 1.0 / 60.0;
        assert_eq!(shot_action(0.0, 0.0, 0.4, dt, false), Some(AnimAction::Attack));
        assert_eq!(shot_action(dt, 0.0, 0.4, dt, false), Some(AnimAction::FollowThrough));
        assert_eq!(shot_action(3.0 * dt, 0.0, 0.4, dt, false), Some(AnimAction::AttackReturn));
    }

    #[test]
    fn full_auto_reads_2_throughout() {
        assert_eq!(shot_action(0.3, 0.0, 0.4, 0.016, true), Some(AnimAction::Attack));
    }

    #[test]
    fn a_throw_reads_5_then_6_then_4() {
        let dt = 1.0 / 60.0;
        let seq: Vec<Option<i32>> = (0..100).map(|i| throw_action(i as f32 * dt, 0.9, None, 1.6, dt).map(AnimAction::code)).collect();
        let first6 = seq.iter().position(|c| *c == Some(6)).unwrap();
        assert!(seq[..first6].iter().all(|c| *c == Some(5)), "gearing up until it leaves: {seq:?}");
        let sixes = seq.iter().filter(|c| **c == Some(6)).count();
        assert!((35..=37).contains(&sixes), "6 for about 0.6 s: {sixes}");
        assert_eq!(seq.iter().position(|c| *c == Some(4)), Some(first6 + sixes));
        assert_eq!(throw_action(1.7, 0.9, None, 1.6, dt), None);
        // A clip that ends within 0.6 s of the release ends while it still reads 6.
        assert_eq!(throw_action(0.4, 0.2, None, 0.5, dt), Some(AnimAction::Thrown));
        // The grenade clip's keys: Release 0.867 s, Attach 1.533 s, end 1.7 s, played as the
        // original's player read them (5 for about 52 loops, 6 for about 40, then 4).
        let codes: Vec<i32> = (0..102).filter_map(|i| throw_action(i as f32 * dt, 0.867, Some(1.533), 1.7, dt).map(AnimAction::code)).collect();
        let count = |c: i32| codes.iter().filter(|x| **x == c).count();
        assert!((50..=53).contains(&count(5)), "5 until the release: {codes:?}");
        assert!((39..=41).contains(&count(6)), "6 until the attach: {codes:?}");
        assert!((9..=11).contains(&count(4)), "4 to the clip's end: {codes:?}");
    }

    #[test]
    fn a_swing_reads_2_until_its_hit_then_4() {
        assert_eq!(swing_action(0.1, 0.4, 0.9), Some(AnimAction::Attack));
        assert_eq!(swing_action(0.5, 0.4, 0.9), Some(AnimAction::AttackReturn));
        assert_eq!(swing_action(1.0, 0.4, 0.9), None);
    }

    #[test]
    fn directions_take_the_larger_axis() {
        assert_eq!(MoveDir::of(0.0, 0.0, 1.0), MoveDir::None);
        assert_eq!(MoveDir::of(0.5, -0.2, 1.0), MoveDir::None);
        assert_eq!(MoveDir::of(10.0, 50.0, 1.0).code(), 1);
        assert_eq!(MoveDir::of(10.0, -50.0, 1.0).code(), 2);
        assert_eq!(MoveDir::of(-80.0, 20.0, 1.0).code(), 3);
        assert_eq!(MoveDir::of(80.0, 20.0, 1.0).code(), 4);
        assert_eq!(MoveDir::of(1.0, 1.0, 0.5), MoveDir::Forward);
    }

    #[test]
    fn codes_are_the_new_vegas_table() {
        let all = [
            (AnimAction::Equip, 0),
            (AnimAction::Unequip, 1),
            (AnimAction::Attack, 2),
            (AnimAction::FollowThrough, 3),
            (AnimAction::AttackReturn, 4),
            (AnimAction::ThrowWindUp, 5),
            (AnimAction::Thrown, 6),
            (AnimAction::Block, 7),
            (AnimAction::Stagger, 8),
            (AnimAction::Reload, 9),
            (AnimAction::Jam, 9),
            (AnimAction::WaitForSpecialIdle, 12),
            (AnimAction::ForceScriptAnim, 13),
            (AnimAction::ReloadOpen, 17),
        ];
        for (a, c) in all {
            assert_eq!(a.code(), c);
        }
    }
}
