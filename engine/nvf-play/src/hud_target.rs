//! Whose health the HUD's enemy health bar shows (docs/spec-notes/hud.md §8; the bar is
//! drawn by `crate::hud`).
//!
//! - **Under the crosshair**: a ray along the crosshair's line, through the collision
//!   world up to [`AIM_RANGE`], to the first thing it meets; an actor's hit volume or
//!   capsule (`Owner::Actor`) makes that actor the one aimed at.
//! - **Shown** by [`why_shown`], the rule captured in the original (hud.md §8, private
//!   jobs hud_enemy4 and hud_enemy5, with hud_enemy2/3) [observed]:
//!   1. never while the actor's Health is at its full value, neutral or hostile, aimed
//!      at, picked or in combat;
//!   2. out of combat, for a hurt actor under the crosshair's ray, hostile or not, the
//!      weapon not drawn (none was captured with it drawn);
//!   3. out of combat, not while that actor is the activation target (the pick,
//!      `crate::activate::Target`) nor while any activation prompt is up;
//!   4. in combat with the player (its combat target is the player), once hurt, aimed
//!      at or not and picked or not, until it dies; the meter follows its health;
//!   5. after the ray leaves it (out of combat), for [`LINGER_SECONDS`] more, unless it
//!      becomes the pick or a prompt comes up first, which ends the linger.
//!   - Kept from before, not captured [hypothesis]: for [`HIT_SECONDS`] after the player
//!     hits an actor, aimed at or not, prompt or not (now only while it is hurt). Never
//!     for a dead actor [observed: hud_enemy5 b4].
//!   - Never for an actor no longer here [design]: disabled, deleted or out of the
//!     loaded cells ([`Case::gone`]), as a script's Disable or a load door leaves one the
//!     bar held or the player hit. Behind a loading screen the bar forgets what it held
//!     and the player's hits ([`Memory::enter`]).
//!   - Not built [observed, open]: the bar shows about 1 s after a hurt in combat but
//!     within 0.25 s out of it (hud_enemy4 g4, hud_enemy3 g2; hud_enemy5 a1, a3).
//! - **One actor at a time** [design]: the first of [`candidates`] that [`why_shown`]
//!   shows: the one aimed at, else the one the player hit last, else a hurt actor
//!   fighting the player (the one the bar holds first, then the nearest within
//!   [`AIM_RANGE`]), else the one the bar lingers on.
//! - The health is the actor's Health over its full value; the name is the one the
//!   crosshair's prompt gives it (`SimState::ref_name`).
//! - Living Anatomy (See Enemy Health, docs/spec-notes/perk-entry-points.md §9): "Enemy
//!   Health and DT values are displayed with their health bar" [documented: GECK wiki
//!   Entry Point]: the actor's health, full health and DT (its armour's and its own,
//!   `crate::combat::target_armor`) on a line of their own under its name,
//!   "Health 250  DT 15.0" ([`TargetHealth::anatomy_line`], our own form, #2984), centred,
//!   in the same red and font [observed: the place, colour and font on the owner's
//!   screenshot 2026-10-24 of the original].

use crate::combat::{ActorHit, Combat};
use crate::phys::PhysState;
use crate::player::Player;
use crate::script::{with_world, ScriptState};
use crate::sim::SimState;
use crate::{PlaySet, PlayState};
use bevy::prelude::*;
use fnv_phys::{layers, Filter, Owner};
use fnv_sim::FormId;
use std::collections::HashMap;

/// How far the crosshair's ray looks for an actor [hypothesis: no source gives the
/// original's range; a long rifle shot].
pub const AIM_RANGE: f32 = 8192.0;
/// Seconds an actor the player hit counts as the target [hypothesis].
pub const HIT_SECONDS: f32 = 5.0;
/// Seconds the bar stays on an actor after the crosshair's ray left it, out of combat
/// [observed for a hostile: private job hud_enemy5 b2, a hurt restrained gecko, shown to
/// 5.52 s after the turn]. For a neutral this is a known divergence [observed: a hurt
/// settler's bar was still up 16-18 s after the turn in hud_enemy5 a1-a3a when the watch
/// ended, so the original's neutral linger is at least that]: what makes the two differ
/// (hostility, or how long the bar was up before) is not known, so the hostile's figure
/// stands for both until a longer watch settles it (docs/handoff/observer-checks.md).
pub const LINGER_SECONDS: f32 = 5.5;

/// The actor whose health the bar shows: reference, name, Health as 0 to 1 of its full
/// value, and whether it is hostile (drawn in the HUD's alternative colour).
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct HudTarget(pub Option<TargetHealth>);

/// The actor under the crosshair (the same ray, [`AIM_RANGE`]) is alive and hostile to
/// the player, by the same rule: the crosshair turns red (`crate::reticle`).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Default)]
pub struct AimHostile(pub bool);

#[derive(Debug, Clone, PartialEq)]
pub struct TargetHealth {
    pub form_id: u32,
    pub name: String,
    pub health: f32,
    /// Colour follows the shown actor [observed: private job hud_enemy3, n2_away,
    /// g2_gecko_half]: in combat with the player or `Game::would_attack`. Only the
    /// colour; whether it shows is [`why_shown`]'s, where hostility is no input [design].
    pub hostile: bool,
    /// With Living Anatomy: its health and DT.
    pub anatomy: Option<(f32, f32)>,
    /// The target's DR for the line too (the Living Anatomy DR tweak, `crate::tweaks`).
    pub anatomy_dr: Option<f32>,
    /// In VATS: its health as a share once the damage in sight is taken off
    /// (`crate::vats_mode::VatsMode::expected_damage`); the ticks between this and
    /// `health` are drawn faint. `None` outside VATS or with no damage in sight.
    pub after: Option<f32>,
}

impl TargetHealth {
    /// With Living Anatomy, the line under the name: "Health <health>  DT <dt>", the health
    /// whole and the DT to one place [design: NVForever's own form, #2984; "Health" as the
    /// perk's own description says it (LivingAnatomy 0x00146099 DESC), DT as the built-in
    /// label of `sInventoryDamageThreshold`].
    pub fn anatomy_line(&self) -> Option<String> {
        // The player's tweak adds the DR, whole [design: docs/design/tweaks.md].
        self.anatomy.map(|(h, dt)| match self.anatomy_dr {
            Some(dr) => format!("Health {:.0}  DT {:.1}  DR {:.0}", h.max(0.0), dt, dr),
            None => format!("Health {:.0}  DT {:.1}", h.max(0.0), dt),
        })
    }
}

/// What decides whether one actor gets the bar this frame ([`why_shown`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Case {
    /// Its Health is below its full value (the meter would not be full).
    pub hurt: bool,
    pub dead: bool,
    /// No longer here: disabled, deleted or out of the loaded cells (`Game::present`,
    /// `Game::loaded_cells`).
    pub gone: bool,
    /// Under the crosshair's ray now.
    pub aimed: bool,
    /// In combat with the player: its combat target is the player.
    pub fighting: bool,
    /// It is the activation target (`crate::activate::Target`; the original's
    /// GetCrosshairRef).
    pub picked: bool,
    /// An activation prompt is up under the crosshair, for it or for anything else.
    pub prompt: bool,
    /// Seconds since the player last hit it.
    pub since_hit: Option<f32>,
    /// The bar is on it: seconds since the crosshair's ray last was ([`held_age`]).
    pub since_aimed: Option<f32>,
}

/// Why an actor gets the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The player hit it within [`HIT_SECONDS`].
    Hit,
    /// It fights the player.
    Fighting,
    /// It is under the crosshair's ray.
    Aimed,
    /// The ray left it less than [`LINGER_SECONDS`] ago.
    Linger,
}

/// Whether, and why, an actor gets the bar (the module notes' rule, points 1-5).
pub fn why_shown(c: &Case) -> Option<Why> {
    // 1. Never at full health [observed]; never dead [observed: hud_enemy5 b4]; never
    // for one no longer here [design].
    if c.dead || c.gone || !c.hurt {
        return None;
    }
    // The player's own hit, as before [hypothesis: not captured].
    if c.since_hit.is_some_and(|s| s < HIT_SECONDS) {
        return Some(Why::Hit);
    }
    // 4. In combat with the player: aimed at or not, picked or not [observed].
    if c.fighting {
        return Some(Why::Fighting);
    }
    // 3. Out of combat the pick or any prompt hides it, and ends a linger [observed].
    if c.picked || c.prompt {
        return None;
    }
    // 2. Under the ray [observed]; 5. for a while after [observed for a hostile].
    if c.aimed {
        return Some(Why::Aimed);
    }
    c.since_aimed.filter(|s| *s < LINGER_SECONDS).map(|_| Why::Linger)
}

/// Whether an actor gets the bar ([`why_shown`]).
pub fn shown(c: &Case) -> bool {
    why_shown(c).is_some()
}

/// The actor the bar holds and the seconds since the ray was on it, after a frame of `dt`
/// seconds in which the ray is on `aimed`.
pub fn held_age(held: Option<(u32, f32)>, aimed: Option<u32>, dt: f32) -> Option<(u32, f32)> {
    held.map(|(id, age)| (id, if aimed == Some(id) { 0.0 } else { age + dt }))
}

/// What the bar holds once this frame shows `shown`: the same actor keeps its age; one
/// shown under the ray starts at 0, one shown without it (a fighter, a hit) at
/// [`LINGER_SECONDS`], so it does not linger once that ends; nothing shown, nothing held
/// (a linger cut short by the pick or a prompt is over).
pub fn held_next(held: Option<(u32, f32)>, shown: Option<u32>, aimed: Option<u32>) -> Option<(u32, f32)> {
    let id = shown?;
    let age = if aimed == Some(id) { 0.0 } else { held.filter(|h| h.0 == id).map_or(LINGER_SECONDS, |h| h.1) };
    Some((id, age))
}

/// The actors that may get the bar this frame, in order, each once: the one aimed at;
/// the one the player hit last; those fighting the player (`fighters`: reference and
/// distance from the eye) within [`AIM_RANGE`], the one the bar holds first, then the
/// nearest; the one the bar holds (its linger). The first that [`why_shown`] shows gets
/// it.
pub fn candidates(aimed: Option<u32>, hit_last: Option<u32>, fighters: &[(u32, f32)], held: Option<u32>) -> Vec<u32> {
    let mut near: Vec<(bool, f32, u32)> = fighters.iter().filter(|f| f.1 <= AIM_RANGE).map(|&(id, d)| (held != Some(id), d, id)).collect();
    near.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut out: Vec<u32> = Vec::new();
    for id in aimed.into_iter().chain(hit_last).chain(near.into_iter().map(|f| f.2)).chain(held) {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// What the bar keeps from frame to frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Memory {
    /// The actor the bar holds and the seconds since the ray was on it ([`held_age`]).
    pub held: Option<(u32, f32)>,
    /// The actor the linger alone holds, for its log line.
    pub lingering: Option<u32>,
    /// Seconds since the player hit each actor, for [`HIT_SECONDS`].
    pub hit_ago: HashMap<u32, f32>,
}

impl Memory {
    /// Before a frame in `state`: behind a loading screen it forgets everything, so no
    /// linger or hit of the area left carries into the next [design]. A menu or a
    /// conversation only stops its clocks (the frame does not count).
    pub fn enter(&mut self, state: PlayState) {
        if state == PlayState::Loading {
            *self = Memory::default();
        }
    }
}

pub struct HudTargetPlugin;

impl Plugin for HudTargetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudTarget>().init_resource::<AimHostile>().add_systems(Update, aim.in_set(PlaySet::Ui).before(crate::hud::HudSet));
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn aim(
    time: Res<Time>,
    state: Res<State<PlayState>>,
    player: Option<Res<Player>>,
    control: Option<Res<crate::control::PlayerControl>>,
    phys: Option<ResMut<PhysState>>,
    mut script: Option<ResMut<ScriptState>>,
    mut sim: ResMut<SimState>,
    mut hits: MessageReader<ActorHit>,
    mut target: ResMut<HudTarget>,
    mut aim_hostile: ResMut<AimHostile>,
    combat: Option<Res<Combat>>,
    (pick, locks, tweaks, vats): (Option<Res<crate::activate::Target>>, Option<Res<crate::control::ControlLocks>>, Option<Res<crate::tweaks::TweakSettings>>, Option<Res<crate::vats_mode::VatsMode>>),
    mut memory: Local<Memory>,
) {
    // [design, #2659] Classic also holds the playback attack's actor via bar_target.
    // The classic VATS style's selected target has the bar while targets are chosen,
    // hurt or not [observed: the owner's screenshots of the original's VATS, 2026-11-10,
    // a Goodsprings settler and Sunny Smiles at full health under their bars]; so has
    // the actor Dead Eye previews while it marks [design: the fork owner, the same].
    let chosen = vats.as_deref().and_then(|v| v.bar_target());
    let in_sight = vats.as_deref().and_then(|v| v.expected_damage());
    let dt = time.delta_secs();
    memory.enter(*state.get());
    let Memory { held, lingering, hit_ago } = &mut *memory;
    for t in hit_ago.values_mut() {
        *t += dt;
    }
    hit_ago.retain(|_, t| *t < HIT_SECONDS);
    let mut latest = None;
    for h in hits.read() {
        if h.by == Some(FormId::PLAYER_REF) {
            hit_ago.insert(h.form_id, 0.0);
            latest = Some(h.form_id);
        }
    }
    if *state.get() != PlayState::Playing {
        aim_hostile.set_if_neq(AimHostile(false));
        return;
    }
    let Some(player) = player else { return };
    let crosshair = control.as_deref().and_then(|c| c.crosshair);
    let (eye, dir) = crate::control::Crosshair::ray(crosshair.as_ref(), player.eye(), player.look_dir());
    let aimed = phys.and_then(|mut p| {
        let mut filter = p.player.as_ref().map_or_else(Filter::default, |c| Filter::excluding(c.key()));
        filter.exclude_layers.insert(layers::TRIGGER);
        match p.world.queries().ray(eye, dir, AIM_RANGE, &filter)?.owner {
            Owner::Actor { form_id, .. } => Some(form_id),
            _ => None,
        }
    });
    *held = held_age(*held, aimed, dt);
    let kept = *held;
    // The pick, and whether a prompt is up: `crate::reticle` draws one for any pick unless
    // a script has the rollover text off.
    let picked = pick.as_deref().and_then(|p| p.0.as_ref()).map(|t| t.form_id);
    let prompt = picked.is_some() && !locks.as_deref().is_some_and(|l| l.rollover);
    let hit_last = latest.or_else(|| hit_ago.iter().min_by(|a, b| a.1.total_cmp(b.1)).map(|(id, _)| *id));
    let eye = player.eye();
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let hide_cloaked = tweaks.as_ref().is_some_and(|t| t.no_red_crosshair_on_cloaked);
    let (found, foe, anatomy) = with_world(rt, &mut sim, |rt| {
        let foe = aimed.is_some_and(|a| {
            let (r, g) = (FormId(a), &mut rt.game);
            let fighting = g.awareness.get(r).and_then(|x| x.combat_target()).is_some_and(|(t, _)| t == FormId::PLAYER_REF);
            // The player's tweak: an unseen (Invisibility, Chameleon) foe does not give
            // itself away [design: docs/design/tweaks.md].
            let cloaked = hide_cloaked && {
                let av = |g: &mut fnv_game::Game, v: u16| fnv_game::functions::actor_value_capped(g, r, v);
                av(g, fnv_game::detection::INVISIBILITY) >= 1.0 || av(g, fnv_game::detection::CHAMELEON) >= 1.0
            };
            !cloaked && !g.is_dead(r) && (fighting || g.would_attack(r, FormId::PLAYER_REF))
        });
        // In order ([`candidates`]).
        let away = |at: Option<[f32; 3]>| at.map_or(f32::INFINITY, |p| ((p[0] - eye[0]).powi(2) + (p[1] - eye[1]).powi(2) + (p[2] - eye[2]).powi(2)).sqrt());
        let fighters: Vec<(u32, f32)> = rt
            .game
            .awareness
            .actors
            .iter()
            .filter(|(id, a)| **id != FormId::PLAYER_REF && a.combat_target().is_some_and(|(t, _)| t == FormId::PLAYER_REF))
            .map(|(id, a)| (id.0, away(a.at)))
            .collect();
        let found = chosen.into_iter().chain(candidates(aimed, hit_last, &fighters, kept.map(|h| h.0))).find_map(|id| {
            let r = FormId(id);
            let g = &mut rt.game;
            // [design, #2659] Keep the playback victim's empty bar until playback ends.
            if g.is_dead(r) && chosen != Some(id) {
                return None;
            }
            // Still here: enabled, not deleted, in a loaded cell.
            let gone = !(g.present(r) && g.cell_of(r).is_some_and(|c| g.loaded_cells.contains(&c)));
            let fighting = g.awareness.get(r).and_then(|a| a.combat_target()).is_some_and(|(t, _)| t == FormId::PLAYER_REF);
            let max = g.max_health(r);
            // The player's tweak counts a raised Health (Buffout and the like) in the
            // bar's whole [design: docs/design/tweaks.md].
            let max = if g.tweaks.buffed_health_bars { max + fnv_game::functions::actor_value_temporary(g, r, fnv_game::actor_values::HEALTH).max(0.0) } else { max };
            let health = fnv_game::functions::actor_health(g, r).unwrap_or(max);
            let hostile = fighting || g.would_attack(r, FormId::PLAYER_REF);
            let case = Case {
                hurt: health < max,
                dead: false,
                gone,
                aimed: aimed == Some(id),
                fighting,
                picked: picked == Some(id),
                prompt,
                since_hit: hit_ago.get(&id).copied(),
                since_aimed: kept.filter(|h| h.0 == id).map(|h| h.1),
            };
            let why = if chosen == Some(id) && !gone { Why::Aimed } else { why_shown(&case)? };
            Some((id, if max > 0.0 { (health / max).clamp(0.0, 1.0) as f32 } else { 0.0 }, health as f32, hostile, why))
        });
        let anatomy = found.is_some() && rt.game.player_has_entry(fnv_game::perks::SEE_ENEMY_HEALTH);
        (found, foe, anatomy)
    });
    // The player's tweak keeps the crosshair its own colour [design: docs/design/tweaks.md].
    let foe = foe && !tweaks.as_ref().is_some_and(|t| t.no_red_crosshair);
    aim_hostile.set_if_neq(AimHostile(foe));
    let armor = match (found, combat.as_deref()) {
        (Some((id, ..)), Some(c)) if anatomy => Some(crate::combat::target_armor(c, script.as_deref_mut(), &mut sim, FormId(id))),
        _ => None,
    };
    let dt = armor.map(|a| a.0);
    let anatomy_dr = armor.map(|a| a.1).filter(|_| tweaks.as_ref().is_some_and(|t| t.living_anatomy_dr));
    *held = held_next(kept, found.map(|f| f.0), aimed);
    let now = found.map(|(form_id, health, hp, hostile, _)| {
        let name = sim.ref_shown_name(script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game), form_id).unwrap_or_default().to_string();
        // The share after the damage in sight: the health's full value is its figure over
        // its share.
        let after = in_sight.filter(|(a, d)| *a == form_id && *d > 0.0 && health > 0.0 && hp > 0.0).map(|(_, d)| ((hp - d) / (hp / health)).clamp(0.0, 1.0));
        TargetHealth { form_id, name, health, hostile, anatomy: dt.map(|dt| (hp, dt)), anatomy_dr, after }
    });
    let key = |t: &Option<TargetHealth>| t.as_ref().map(|t| (t.form_id, (t.health * 100.0).round() as i32, t.anatomy_line()));
    // The damage in sight, told when it changes by a whole percent.
    let sight = |t: &Option<TargetHealth>| t.as_ref().and_then(|t| t.after.map(|a| (t.form_id, (a * 100.0).round() as i32)));
    if sight(&target.0) != sight(&now) {
        if let Some((t, a)) = now.as_ref().and_then(|t| t.after.map(|a| (t, a))) {
            info!("hud: enemy health {} 0x{:08X}: {:.0} % left if the attacks in sight land", t.name, t.form_id, a * 100.0);
        }
    }
    if key(&target.0) != key(&now) {
        match &now {
            Some(t) => match t.anatomy {
                Some(_) => info!("hud: enemy health {} 0x{:08X} {:.0} % (See Enemy Health: \"{}\")", t.name, t.form_id, t.health * 100.0, t.anatomy_line().unwrap_or_default()),
                None => info!("hud: enemy health {} 0x{:08X} {:.0} %", t.name, t.form_id, t.health * 100.0),
            },
            None => info!("hud: enemy health off"),
        }
    }
    // Once when a bar comes to be held by the linger alone.
    let linger = found.filter(|f| f.4 == Why::Linger).map(|f| f.0);
    if linger != *lingering {
        if let Some(t) = now.as_ref().filter(|_| linger.is_some()) {
            info!("hud: enemy health {} 0x{:08X} held by the linger (the crosshair left it; {LINGER_SECONDS} s)", t.name, t.form_id);
        }
        *lingering = linger;
    }
    target.set_if_neq(HudTarget(now));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn living_anatomy_reads_as_the_original() {
        let t = |anatomy| TargetHealth { form_id: 1, name: "Deathclaw".into(), health: 1.0, hostile: true, anatomy, anatomy_dr: None, after: None };
        assert_eq!(t(Some((250.0, 15.0))).anatomy_line().as_deref(), Some("Health 250  DT 15.0"));
        assert_eq!(t(Some((-3.0, 0.5))).anatomy_line().as_deref(), Some("Health 0  DT 0.5"));
        assert_eq!(t(None).anatomy_line(), None);
        let with_dr = TargetHealth { anatomy_dr: Some(20.4), ..t(Some((250.0, 15.0))) };
        assert_eq!(with_dr.anatomy_line().as_deref(), Some("Health 250  DT 15.0  DR 20"), "the DR tweak");
    }

    /// A hurt actor out of combat: nothing picked, no prompt, not hit, not held.
    fn hurt() -> Case {
        Case { hurt: true, ..Case::default() }
    }

    #[test]
    fn never_at_full_health() {
        // A hostile in combat, aimed at and picked; a restrained hostile under the ray; a
        // neutral with its Talk prompt up; one just hit; one the bar held.
        for c in [
            Case { fighting: true, aimed: true, picked: true, prompt: true, ..Case::default() },
            Case { fighting: true, ..Case::default() },
            Case { aimed: true, ..Case::default() },
            Case { aimed: true, picked: true, prompt: true, ..Case::default() },
            Case { since_hit: Some(0.0), ..Case::default() },
            Case { since_aimed: Some(0.0), ..Case::default() },
        ] {
            assert!(!shown(&c), "{c:?}");
        }
    }

    #[test]
    fn a_hurt_actor_under_the_ray_out_of_combat() {
        // Hostile or not: hostility is no input (a hurt neutral settler, hud_enemy5 a1-a3).
        assert_eq!(why_shown(&Case { aimed: true, ..hurt() }), Some(Why::Aimed));
        // Not under the ray and not held: no bar.
        assert!(!shown(&hurt()));
    }

    #[test]
    fn the_pick_or_any_prompt_hides_it_out_of_combat() {
        assert!(!shown(&Case { aimed: true, picked: true, prompt: true, ..hurt() }));
        // Picked with no prompt (a restrained gecko, hud_enemy5 b2).
        assert!(!shown(&Case { aimed: true, picked: true, ..hurt() }));
        // A prompt for something else.
        assert!(!shown(&Case { aimed: true, prompt: true, ..hurt() }));
    }

    #[test]
    fn in_combat_it_needs_no_aim_until_it_dies() {
        let fighter = Case { fighting: true, ..hurt() };
        assert_eq!(why_shown(&fighter), Some(Why::Fighting));
        // Aimed at and picked, with a prompt up: still shown.
        assert!(shown(&Case { picked: true, prompt: true, aimed: true, ..fighter }));
        assert!(!shown(&Case { dead: true, ..fighter }));
    }

    #[test]
    fn it_lingers_after_the_ray_leaves() {
        assert_eq!(why_shown(&Case { since_aimed: Some(0.1), ..hurt() }), Some(Why::Linger));
        assert!(shown(&Case { since_aimed: Some(LINGER_SECONDS - 0.01), ..hurt() }));
        assert!(!shown(&Case { since_aimed: Some(LINGER_SECONDS), ..hurt() }));
        // Cut short by the pick or a prompt.
        assert!(!shown(&Case { since_aimed: Some(1.0), prompt: true, ..hurt() }));
        assert!(!shown(&Case { since_aimed: Some(1.0), picked: true, ..hurt() }));
    }

    #[test]
    fn the_players_hit_shows_it_as_before() {
        assert_eq!(why_shown(&Case { since_hit: Some(1.0), ..hurt() }), Some(Why::Hit));
        // Aimed at or not, prompt or not, until the time is up.
        assert!(shown(&Case { since_hit: Some(1.0), prompt: true, picked: true, ..hurt() }));
        assert!(!shown(&Case { since_hit: Some(HIT_SECONDS), ..hurt() }));
        assert!(!shown(&Case { since_hit: Some(0.0), dead: true, ..hurt() }));
    }

    /// Frame by frame at 0.1 s: a hurt neutral under the ray for frames 1-10, then the
    /// ray off it; a prompt up over frames `prompt` (for something else). Whether the bar
    /// shows, per frame.
    fn frames(prompt: std::ops::Range<u32>) -> Vec<bool> {
        let id = 7;
        let mut held = None;
        (1..=100)
            .map(|i| {
                let aimed = (i <= 10).then_some(id);
                held = held_age(held, aimed, 0.1);
                let since_aimed = held.filter(|h: &(u32, f32)| h.0 == id).map(|h| h.1);
                let on = shown(&Case { aimed: aimed.is_some(), prompt: prompt.contains(&i), since_aimed, ..hurt() });
                held = held_next(held, on.then_some(id), aimed);
                on
            })
            .collect()
    }

    #[test]
    fn the_linger_runs_out() {
        let f = frames(0..0);
        assert!(f[..10].iter().all(|on| *on));
        // Shown to about LINGER_SECONDS after frame 10, then off for good.
        let last = f.iter().rposition(|on| *on).unwrap() as f32 + 1.0;
        assert!(((last - 10.0) * 0.1 - LINGER_SECONDS).abs() < 0.15, "last shown at frame {last}");
        assert!(f[(last as usize)..].iter().all(|on| !on));
    }

    #[test]
    fn a_prompt_ends_the_linger() {
        // A prompt up over frames 30-34: the bar goes and does not come back after it.
        let f = frames(30..35);
        assert!(f[10..29].iter().all(|on| *on));
        assert!(f[29..].iter().all(|on| !on));
    }

    #[test]
    fn a_fighter_shown_without_aim_does_not_linger() {
        // Shown in combat, never under the ray: held at the linger's end already.
        assert_eq!(held_next(None, Some(3), None), Some((3, LINGER_SECONDS)));
        assert_eq!(held_next(Some((3, 2.0)), Some(3), None), Some((3, 2.0)));
        assert_eq!(held_next(Some((3, 2.0)), Some(3), Some(3)), Some((3, 0.0)));
        assert_eq!(held_next(Some((3, 2.0)), None, None), None);
        assert_eq!(held_next(Some((3, 2.0)), Some(4), None), Some((4, LINGER_SECONDS)));
    }

    #[test]
    fn never_for_an_actor_no_longer_here() {
        // Disabled, deleted or out of the loaded cells: under the ray, fighting, hit or
        // lingering, no bar.
        for c in [
            Case { aimed: true, ..hurt() },
            Case { fighting: true, ..hurt() },
            Case { since_hit: Some(0.0), ..hurt() },
            Case { since_aimed: Some(0.1), ..hurt() },
        ] {
            assert!(shown(&c), "{c:?}");
            assert!(!shown(&Case { gone: true, ..c }), "{c:?}");
        }
    }

    #[test]
    fn a_load_forgets_the_held_bar_and_the_hits() {
        let full = Memory { held: Some((7, 1.0)), lingering: Some(7), hit_ago: HashMap::from([(7, 0.5), (8, 2.0)]) };
        // A menu or a conversation keeps them (their clocks stop).
        for s in [PlayState::Playing, PlayState::Menu, PlayState::Dialogue] {
            let mut m = full.clone();
            m.enter(s);
            assert_eq!(m, full, "{s:?}");
        }
        let mut m = full.clone();
        m.enter(PlayState::Loading);
        assert_eq!(m, Memory::default());
    }

    #[test]
    fn the_candidates_in_order() {
        // Aimed 1, hit 2, fighters 3 (far), 4 (near), the held 5.
        assert_eq!(candidates(Some(1), Some(2), &[(3, 900.0), (4, 300.0)], Some(5)), vec![1, 2, 4, 3, 5]);
        // The fighter the bar holds comes before a nearer one, and only once.
        assert_eq!(candidates(None, None, &[(3, 900.0), (4, 300.0)], Some(3)), vec![3, 4]);
        // A fighter beyond the aim range is no candidate.
        assert_eq!(candidates(None, None, &[(3, AIM_RANGE + 1.0), (4, AIM_RANGE)], None), vec![4]);
        // Each once: aimed and hit the same, held the same as aimed.
        assert_eq!(candidates(Some(1), Some(1), &[(1, 100.0)], Some(1)), vec![1]);
        assert_eq!(candidates(None, None, &[], None), Vec::<u32>::new());
    }

    /// The first of the candidates that [`why_shown`] shows, with each actor's case.
    fn first(order: Vec<u32>, cases: &[(u32, Case)]) -> Option<(u32, Why)> {
        order.into_iter().find_map(|id| why_shown(&cases.iter().find(|c| c.0 == id)?.1).map(|w| (id, w)))
    }

    #[test]
    fn the_first_shown_candidate_gets_the_bar() {
        let fighter = Case { fighting: true, ..hurt() };
        let lingering = Case { since_aimed: Some(1.0), ..hurt() };
        let cases = [
            (1, Case { aimed: true, ..hurt() }),
            (2, Case { since_hit: Some(1.0), ..hurt() }),
            (3, fighter),
            (4, fighter),
            (5, lingering),
        ];
        let order = |aimed, hit, held| candidates(aimed, hit, &[(3, 900.0), (4, 300.0)], held);
        // The one aimed at, before the one hit, the fighters and the linger.
        assert_eq!(first(order(Some(1), Some(2), Some(5)), &cases), Some((1, Why::Aimed)));
        // The one hit, before the fighters.
        assert_eq!(first(order(None, Some(2), Some(5)), &cases), Some((2, Why::Hit)));
        // The nearest fighter, before the linger; the held fighter before a nearer one.
        assert_eq!(first(order(None, None, Some(5)), &cases), Some((4, Why::Fighting)));
        assert_eq!(first(order(None, None, Some(3)), &cases), Some((3, Why::Fighting)));
        // The aimed actor at full health gets none: the next does.
        let full_aimed = [(1, Case { aimed: true, ..Case::default() }), (5, lingering)];
        assert_eq!(first(candidates(Some(1), None, &[], Some(5)), &full_aimed), Some((5, Why::Linger)));
        // A fighter no longer here (a load door, a script's Disable): the linger, then none.
        let gone = [(3, Case { gone: true, ..fighter }), (5, lingering)];
        assert_eq!(first(candidates(None, None, &[(3, 100.0)], Some(5)), &gone), Some((5, Why::Linger)));
        assert_eq!(first(candidates(None, None, &[(3, 100.0)], None), &gone), None);
    }
}
