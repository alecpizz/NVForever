//! Fights away from the host: the host's actors that it simulates around a
//! guest away from it and does not draw (`super::areas`, `Residents::simulated_undrawn`)
//! fight that guest's actor here (its stand-in, `super::stand_ins`), so a guest out in the
//! Mojave is attacked as the host would be [design: fights should still happen].
//!
//! The drawn combat AI (`crate::npc_combat`) needs what only drawn actors have: a rig, a
//! held weapon, collision for its sight lines and projectiles. Collision exists around the
//! host only (`nvf_play::phys`, docs/design/detection.md), so these fights are settled by
//! numbers instead [design: the original has no fights out of the loaded area to
//! compare with]:
//! - **Noticing**: an actor that would attack the player (`Game::would_attack`, as for
//!   the player, whose place a guest's actor takes) and stands within [`SIGHT`] of a
//!   guest in the same space starts combat with that guest's actor (`Game::start_combat`)
//!   [design: no sight line, there is no collision there to cast it through].
//!   One with an aggro radius that does not attack on sight (a worker ant: Aggressive,
//!   radius 512) attacks a guest inside half of it, as `fnv_game::aggro` has it attack
//!   without warning there; the warnings further out need detection and are not given.
//! - **Closing in**: its weapon's reach goes to the AI that moves actors
//!   (`Awarenesses::engage`, `crate::npc_combat::engage`), which walks it in on the
//!   navmesh as for a drawn fighter.
//! - **Attacking**: within reach, one attack every [`attack_every`] (a gun's shots per
//!   second, a blow's [`BLOW_SECONDS`]); a shot hits by [`hit_chance`], a blow always.
//!   A hit deals the weapon's base damage (all its projectiles), a creature's own attack
//!   damage, and goes to the guest's game as every hit on its actor does
//!   (`CombatStats::coop_hits`). The guest's copy plays the attack (`npc_attacks`) and
//!   shows the weapon ([`FarFighter::weapon`]).
//! - **Not through the land**: a hit lands only when the land does not rise between the
//!   two ([`land_between`]); an actor the AI has under the land is put on it first
//!   (`crate::actor_feet::on_the_land`). A guest that is down is no one's target (creatures
//!   under the ground went on killing a guest otherwise, and the hits went on while it was
//!   down) [observed: buckallobill, in game, FNV 1.4.0.525].
//! - The guest's hits on these actors land on the host already (`Game::hit_object`) and
//!   make them fight back by the same rule.
//!
//! Log: `co-op far fights: <actor> fights player <n> ...` once per fight, and a count every
//! [`LOG_EVERY`] while any goes on.

use super::*;
use crate::npc_combat::{engage, gun_reach};
use fnv_nav::Space;
use nvf_play::combat::{Combat, CombatStats, RelayedHit};

/// How near a guest a far actor notices it, game units [design: about a
/// cell's half, the distance a drawn actor in the open notices a player in daylight].
pub const SIGHT: f32 = 2000.0;
/// A blow's pace when the weapon gives none, seconds [design].
pub const BLOW_SECONDS: f32 = 1.5;
/// A creature's strike distance with its body in, game units, when its record gives no
/// reach [design].
pub const CREATURE_REACH: f32 = 150.0;
/// How often the fights are looked at, seconds.
const STEP: f32 = 0.25;
const LOG_EVERY: f32 = 10.0;
/// Where along the line between two actors the land is looked at: its eye, its target's
/// chest, and this many points between [design].
const LAND_STEPS: usize = 8;
/// The height of an attack's line over the feet, game units [design: about a
/// creature's or a person's chest].
const LINE_UP: f32 = 60.0;
/// How far the land must rise over the line to stop it, game units [design].
const LAND_CLEAR: f32 = 16.0;

/// One far actor's fight.
#[derive(Debug, Clone, Default)]
pub(super) struct FarFighter {
    /// The weapon it fights with (None: a creature's own attack, or fists).
    pub weapon: Option<FormId>,
    gun: bool,
    /// When it may attack next (host seconds).
    next: f32,
    pub target_player: u8,
    /// How far it was from its target at the last step (for the log).
    distance: f32,
    /// Whether the land in the way was logged for this fight.
    blocked_logged: bool,
}

/// The host's far fights.
#[derive(Default)]
pub(super) struct FarFights {
    pub fighters: HashMap<u32, FarFighter>,
    last_step: f32,
    last_log: f32,
    hits: u32,
    misses: u32,
    /// Attacks the land stopped.
    blocked: u32,
    rng: u64,
}

/// Seconds between attacks: a gun's shots per second (its fire rate when automatic), a
/// blow's [`BLOW_SECONDS`] over its attack speed.
pub fn attack_every(gun: bool, automatic: bool, fire_rate: f32, shots_per_sec: f32, attack_mult: f32) -> f32 {
    if gun {
        let per = if automatic && fire_rate > 0.0 { fire_rate } else if shots_per_sec > 0.0 { shots_per_sec } else { 1.0 };
        (1.0 / per).max(0.1)
    } else {
        BLOW_SECONDS / attack_mult.max(0.25)
    }
}

/// The chance a shot from `distance` hits, for a gun of range `max_range`
/// [design: 85 % at point blank, falling to 35 % at its range].
pub fn hit_chance(distance: f32, max_range: f32) -> f32 {
    let r = if max_range > 0.0 { (distance / max_range).clamp(0.0, 1.0) } else { 0.5 };
    0.85 - 0.5 * r
}

/// Whether the land rises between an attacker at `from` and its target at `to` (feet), on
/// the line [`LINE_UP`] over both, by more than [`LAND_CLEAR`]: the far actors have no
/// collision to cast a sight line through, but the land's heights are in the data.
pub fn land_between(from: [f32; 3], to: [f32; 3], land: impl Fn(f32, f32) -> Option<f32>) -> bool {
    (0..=LAND_STEPS).any(|i| {
        let t = i as f32 / LAND_STEPS as f32;
        let at = |k: usize| from[k] + (to[k] - from[k]) * t;
        land(at(0), at(1)).is_some_and(|g| g > at(2) + LINE_UP + LAND_CLEAR)
    })
}

fn space_of(place: Place) -> Space {
    match place {
        Place::Interior(c) => Space::Interior(c),
        Place::Exterior(w) => Space::World(w),
    }
}

/// The host's side, each frame.
#[allow(clippy::type_complexity)]
pub(super) fn far_fights(
    time: Res<Time>,
    mut coop: ResMut<Coop>,
    streamer: Res<Streamer>,
    (mut script, residents, world, sim): (Option<ResMut<nvf_play::script::ScriptState>>, Option<Res<Residents>>, Option<Res<WorldData>>, Option<Res<nvf_play::sim::SimState>>),
    (combat, mut stats): (Option<Res<Combat>>, ResMut<CombatStats>),
) {
    let coop = &mut *coop;
    let hosting = coop.link.as_ref().is_some_and(|l| l.is_host());
    let game = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| &mut rt.game);
    let (Some(game), Some(residents), Some(world), Some(sim), Some(combat), true) = (game, residents, world, sim, combat, hosting) else {
        if !coop.far.fighters.is_empty() {
            coop.far = FarFights::default();
        }
        return;
    };
    let now = time.elapsed_secs();
    if now - coop.far.last_step < STEP {
        return;
    }
    coop.far.last_step = now;
    if coop.far.rng == 0 {
        coop.far.rng = 0x9E37_79B9_7F4A_7C15;
    }
    // The guests' actors here, where they are.
    let guests: Vec<(u8, FormId, Space, [f32; 3])> = coop
        .peers
        .iter()
        // One that is down is no one's target until it is up again.
        .filter(|(_, p)| !p.lost && !p.down)
        .filter_map(|(&id, p)| Some((id, p.actor?, p.samples.back().map(|(_, s)| (space_of(s.place), s.feet))?)))
        .map(|(id, a, (sp, f))| (id, a, sp, f))
        .collect();
    let undrawn = if guests.is_empty() { Vec::new() } else { residents.simulated_undrawn(&world, &streamer) };
    let far = &mut coop.far;
    let mut still = HashSet::new();
    for (r, _, space, feet, _, _) in undrawn {
        let me = FormId(r);
        if game.is_dead(me) {
            continue;
        }
        let dist = |f: [f32; 3]| ((f[0] - feet[0]).powi(2) + (f[1] - feet[1]).powi(2) + (f[2] - feet[2]).powi(2)).sqrt();
        // Its target: the guest it fights now, else the nearest one it would attack.
        let current = game.combat_target_of(me).and_then(|t| guests.iter().find(|g| g.1 == t).copied());
        let target = match current {
            Some(g) => Some(g),
            None => {
                let near = guests.iter().filter(|g| g.2 == space && dist(g.3) <= SIGHT).min_by(|a, b| dist(a.3).total_cmp(&dist(b.3))).copied();
                // One that does not attack on sight but has an aggro radius attacks a guest
                // inside its no-warning share of it (`fnv_game::aggro`; the warnings
                // themselves need detection, which these actors have not).
                let aggro = |g: &fnv_game::game::Game<'_>, d: f32| g.aggro_radius(me).is_some_and(|r| f64::from(d) <= r * fnv_game::aggro::AGGRO_ATTACK_SHARE);
                match near {
                    Some(g) if game.would_attack(me, FormId::PLAYER_REF) || aggro(&*game, dist(g.3)) => {
                        game.start_combat(me, g.1);
                        Some(g)
                    }
                    _ => None,
                }
            }
        };
        let Some((pid, target, tspace, tfeet)) = target else { continue };
        if tspace != space {
            continue;
        }
        // Detection measures drawn actors only (docs/design/detection.md): this one's
        // view of its target is kept here, seen where the guest stands, so the AI closes
        // in on where it is now, not where the fight began.
        if let Some(t) = game.awareness.actors.get_mut(&me).and_then(|a| a.targets.get_mut(&target)) {
            t.at = tfeet;
            t.detected = Some(game.time);
            t.hidden_since = None;
        }
        still.insert(r);
        let f = far.fighters.entry(r).or_insert_with(|| {
            let weapon = sim.world.equipment(me).unwrap_or_default().into_iter().find(|w| combat.forms.weapons.contains_key(w));
            let gun = weapon.is_some_and(|w| combat.forms.weapons.get(&w).is_some_and(|ws| crate::npc_combat::is_gun(&combat, ws, w)));
            let own = if weapon.is_none() { game.creature_attack(me).map_or("fists".to_string(), |c| format!("its own attack ({} damage)", c.0)) } else { String::new() };
            info!("co-op far fights: {me} fights player {} away from the host, with {}", pid + 1, weapon.map_or(own, |w| w.to_string()));
            FarFighter { weapon, gun, next: now, target_player: pid, distance: 0.0, blocked_logged: false }
        });
        f.target_player = pid;
        let ws = f.weapon.or(combat.fists_id()).and_then(|w| combat.forms.weapons.get(&w));
        // A creature's own attack, as the drawn combat AI reads it (`Game::creature_attack`:
        // the leveled pick's or template's CREA DATA Damage and RNAM reach).
        let creature = game.creature_attack(me).filter(|_| f.weapon.is_none());
        let creature_hit = creature.map(|c| c.0);
        // How near it wants to be: a gun's reach, a blow's or a creature's.
        let (strike, gun_range) = if f.gun {
            let ws = ws.expect("a gun has stats");
            let reach = gun_reach(ws.max_range, 1.0, ws.max_range, 0.0);
            game.awareness.engage.insert(me, engage(None, Some(reach)));
            (ws.max_range.max(500.0), ws.max_range)
        } else {
            let reach = match creature_hit {
                Some(_) => creature.and_then(|c| c.1).filter(|r| *r > 0.0).map_or(CREATURE_REACH, |r| (r as f32).max(CREATURE_REACH)),
                None => ws.map_or(CREATURE_REACH, |w| (w.reach * 100.0).max(CREATURE_REACH)),
            };
            game.awareness.engage.insert(me, engage(Some(reach), None));
            (reach + 40.0, 0.0)
        };
        let d = dist(tfeet);
        f.distance = d;
        if d > strike || now < f.next {
            continue;
        }
        f.next = now + ws.map_or(BLOW_SECONDS, |w| attack_every(f.gun, w.automatic, w.fire_rate, w.attack_shots_per_sec, w.attack_mult));
        // Not through the land: the attack is held, as a drawn one's sight line would be.
        if let Space::World(ws) = space {
            if land_between(feet, tfeet, |x, y| crate::actor_feet::land_height_in(&world, ws, x, y)) {
                far.blocked += 1;
                if !f.blocked_logged {
                    f.blocked_logged = true;
                    info!("co-op far fights: {me} holds its attack on player {}: the land is between them ({:.0} units, feet z {:.0} and {:.0})", pid + 1, d, feet[2], tfeet[2]);
                }
                continue;
            }
        }
        *coop.npc_attacks.entry(r).or_default() = coop.npc_attacks.get(&r).copied().unwrap_or(0).wrapping_add(1);
        let lands = !f.gun || crate::npc_combat::next_unit(&mut far.rng) < hit_chance(d, gun_range);
        if !lands {
            far.misses += 1;
            continue;
        }
        let dealt = match (creature_hit, ws) {
            (Some(c), _) => (c as f32).max(1.0),
            (None, Some(w)) => (w.base_damage.max(1) * w.projectile_count.max(1) as i32) as f32,
            (None, None) => 1.0,
        };
        far.hits += 1;
        stats.coop_hits.push((Some(me), RelayedHit { target, dealt, weapon: f.weapon, location: 0, critical: false, limb: None }));
    }
    // Fights that ended (its target gone, it dead or out of the area): the AI holds again.
    for r in far.fighters.keys().copied().filter(|r| !still.contains(r)).collect::<Vec<_>>() {
        far.fighters.remove(&r);
        game.awareness.engage.remove(&FormId(r));
    }
    if !far.fighters.is_empty() && now - far.last_log >= LOG_EVERY {
        far.last_log = now;
        let mut each: Vec<String> = far.fighters.iter().map(|(r, f)| format!("{:#010X} {:.0} away", r, f.distance)).collect();
        each.sort();
        info!("co-op far fights: {} actors fighting guests away from the host; {} hits, {} misses, {} held by the land so far; {}", far.fighters.len(), far.hits, far.misses, far.blocked, each.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_far_guns_pace_and_aim() {
        // A semi-automatic at 2 shots a second, an automatic at its fire rate, a blow.
        assert!((attack_every(true, false, 0.0, 2.0, 1.0) - 0.5).abs() < 1e-6);
        assert!((attack_every(true, true, 10.0, 0.0, 1.0) - 0.1).abs() < 1e-6);
        assert!((attack_every(false, false, 0.0, 0.0, 1.0) - BLOW_SECONDS).abs() < 1e-6);
        // Nearer hits more often; never 0 or 1.
        assert!(hit_chance(100.0, 2000.0) > hit_chance(1800.0, 2000.0));
        assert!(hit_chance(0.0, 2000.0) < 1.0 && hit_chance(5000.0, 2000.0) > 0.0);
    }

    #[test]
    fn the_land_stops_a_far_attack() {
        let flat = |_: f32, _: f32| Some(0.0);
        // On flat land, two actors standing on it: nothing between them.
        assert!(!land_between([0.0, 0.0, 0.0], [300.0, 0.0, 0.0], flat));
        // A lakelurk 200 units under the land: the land is over its line.
        assert!(land_between([0.0, 0.0, -200.0], [300.0, 0.0, 0.0], flat));
        // A ridge between them.
        let ridge = |x: f32, _: f32| Some(if (100.0..200.0).contains(&x) { 300.0 } else { 0.0 });
        assert!(land_between([0.0, 0.0, 0.0], [300.0, 0.0, 0.0], ridge));
        // No land there (out of the worldspace's grid): not held.
        assert!(!land_between([0.0, 0.0, -200.0], [300.0, 0.0, 0.0], |_, _| None));
    }

    #[test]
    fn an_actor_under_the_land_is_put_on_it() {
        use crate::actor_feet::{on_the_land, BELOW_LAND};
        assert_eq!(on_the_land([1.0, 2.0, -500.0], Some(-100.0)), [1.0, 2.0, -100.0]);
        // Within the land mesh's error, or above it (swimming, on a rock): left.
        assert_eq!(on_the_land([1.0, 2.0, -100.0 - BELOW_LAND + 1.0], Some(-100.0))[2], -100.0 - BELOW_LAND + 1.0);
        assert_eq!(on_the_land([1.0, 2.0, 50.0], Some(-100.0))[2], 50.0);
        assert_eq!(on_the_land([1.0, 2.0, -500.0], None)[2], -500.0);
    }
}
