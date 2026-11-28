//! Hits and deaths of placed actors on screen (docs/design/combat.md §5.6–5.7, step C5):
//! `nvf_play::combat` reports each hit ([`ActorHit`]); a living actor plays the hit
//! reaction for where it was hit (step C8) once over its other clips, when the hit
//! cripples a body part, or lands on a crippled one (or anywhere while its torso is
//! crippled, docs/spec-notes/crippled-limbs.md §3) and the actor's last reaction ended
//! long enough ago (`fnv_combat::damage::staggers`) [design: the owner's rule]. A hit
//! that forces the reaction ([`ForcedHitReaction`]: Khan Trick, perk-entry-points.md
//! §11.4) plays it whatever it crippled and however recent the last one was.
//!
//! A dead actor whose skeleton has a ragdoll becomes a [`crate::ragdoll::Corpse`]: killed
//! in sight on loaded ground, its pose stops where it is and the ragdoll drops it
//! (`ragdoll.rs`, docs/spec-notes/ragdoll.md §4). Until its ragdoll is in the world (the
//! ground under it is not loaded, or it was dead when drawn), and for skeletons without a
//! ragdoll, the earlier presentation holds: the death pose (played once, or straight at
//! its end for an actor that is dead when it is drawn); without a death pose the pose
//! freezes and the body is laid on its back about its feet [design]. A corpse whose
//! ragdoll then fails to build is shown the same way (`ragdoll::unbuilt`, #2347).
//!
//! A corpse laid out by a new game (`Game::starts_dead`) whose reference stores the pose
//! its designer left it in (`XRGD`, `authored_pose.rs`) is put in that pose at rest
//! instead of falling (docs/spec-notes/dead-on-spawn.md §5); on a skeleton the poses'
//! reading was not checked on (creatures) it starts in that pose awake and settles (#3408).

use crate::limbs::ActorRig;
use crate::world::WorldData;
use crate::locomotion::{Dying, Flinch, Locomotion};
use crate::ragdoll::Ragdolls;
use bevy::prelude::*;
use nvf_assets::data::GameData;
use nvf_play::combat::{is_dead, ActorHit, Combat, ForcedHitReaction};
use nvf_play::phys::PhysState;
use nvf_play::script::ScriptState;
use nvf_play::RefVisual;

pub struct DeathsPlugin;

impl Plugin for DeathsPlugin {
    fn build(&self, app: &mut App) {
        // [design, #2531] Consume test kills this frame so PostUpdate can start their
        // ragdolls before the next Physics volume sync and Activation dismemberment.
        app.add_systems(Update, show_hits_and_deaths.after(nvf_play::PlaySet::Activation).after(nvf_play::knockouts::TestHitSet));
    }
}

/// Seconds that count as "the end" of any death pose (it is clamped to the clip).
pub(crate) const END: f32 = 1.0e6;

/// The death pose of an actor shown dead without a ragdoll, if its clips have one; it
/// varies by reference [design].
pub(crate) fn death_clip(lo: Option<&Locomotion>, form_id: u32) -> Option<std::sync::Arc<nvf_assets::anim_play::BoundClip>> {
    lo.filter(|lo| !lo.set.death.is_empty()).map(|lo| lo.set.death[(form_id as usize) % lo.set.death.len()].clone())
}

/// An actor shown dead without a ragdoll or a death pose: its body is laid on its back
/// about its feet [design].
pub(crate) fn lay_on_back(t: &mut Transform) {
    t.rotate_local_x(-std::f32::consts::FRAC_PI_2);
}

/// What [`show_hits_and_deaths`] reads of an actor that is not yet shown dead.
type Living = (
    Entity,
    &'static RefVisual,
    &'static ActorRig,
    Option<&'static Locomotion>,
    Option<&'static Flinch>,
    &'static mut Transform,
    &'static GlobalTransform,
    Option<&'static crate::outfits::Dressed>,
);

#[allow(clippy::too_many_arguments)]
fn show_hits_and_deaths(
    time: Res<Time>,
    combat: Option<Res<Combat>>,
    mut commands: Commands,
    (mut hits, mut forced): (MessageReader<ActorHit>, MessageReader<ForcedHitReaction>),
    mut script: Option<ResMut<ScriptState>>,
    data: Option<Res<GameData>>,
    phys: Option<Res<PhysState>>,
    mut ragdolls: ResMut<Ragdolls>,
    mut actors: Query<Living, Without<Dying>>,
    world: Option<Res<WorldData>>,
    actor_world: Option<Res<crate::actors::ActorWorld>>,
) {
    let fresh: Vec<ActorHit> = hits.read().copied().collect();
    let forcing: Vec<ForcedHitReaction> = forced.read().copied().collect();
    let now = time.elapsed_secs();
    let tuning = combat.map(|c| c.tuning.clone()).unwrap_or_default();
    for (e, r, rig, lo, flinch, mut t, gt, dressed) in &mut actors {
        let dead = is_dead(script.as_deref(), r.0);
        if dead {
            let templates = crate::ragdoll::worn_templates(actor_world.as_deref(), dressed.map_or(&[][..], |d| &d.worn));
            let corpse = data.as_deref().and_then(|d| ragdolls.corpse(d, rig, &templates));
            if lo.is_none() && corpse.is_none() {
                continue;
            }
            // Killed by any of this frame's hits on it: a blast kill sends more than one (the
            // round's impact and its blast, or several blasts), and those after the killing
            // hit landed on the corpse (`died` false) (#2218).
            let died_now = died_this_frame(&fresh, r.0);
            // Pushed and killed by the same script (`PushActorAway` then `KillActor`: the
            // Legion sniper and the Ranger on Hoover Dam's tower): it was never shown
            // knocked down, so `knockouts.rs` did not take the push's shove. The body
            // falls as a ragdoll thrown with it, though no hit killed it (#2749).
            let shove = script.as_deref_mut().and_then(|s| s.rt.as_mut()).and_then(|rt| rt.game.take_shove(fnv_sim::FormId(r.0)));
            // Killed in sight on loaded ground: the ragdoll takes the pose as it is.
            let falls = (died_now || shove.is_some()) && corpse.is_some() && phys.as_deref().is_some_and(|p| p.ready_at(nvf_play::space::to_game(gt.translation())));
            let mut corpse = corpse;
            if let Some(v) = shove {
                if let Some(c) = corpse.as_mut().filter(|_| falls) {
                    c.velocity = v;
                }
                info!("knockout: {:08X} thrown at ({:.0}, {:.0}, {:.0}) units/s as it dies{}", r.0, v[0], v[1], v[2], if falls { "" } else { " (no ragdoll here: the shove is lost)" });
            }
            // The death pose varies by reference [design].
            let clip = death_clip(lo.filter(|_| !falls), r.0);
            if clip.is_none() && !falls {
                lay_on_back(&mut t);
            }
            // Laid out by a new game, with the pose its designer left it in: at rest in it.
            if let (false, Some(c), Some(w)) = (died_now, corpse.as_ref(), world.as_deref()) {
                authored(&mut ragdolls, script.as_deref(), w, c, rig, gt);
            }
            let start = if died_now || (falls && shove.is_some()) { 0.0 } else { END };
            let mut ec = commands.entity(e);
            ec.insert(Dying { clip, t: start });
            ec.remove::<Flinch>();
            if let Some(mut c) = corpse {
                // Not falling, it is shown dead without its ragdoll already: a build that
                // fails later leaves it so (`ragdoll::unbuilt`).
                c.laid_out = !falls;
                ec.insert(c);
            }
        } else if let Some(lo) = lo {
            // Of this frame's hits (a shotgun's pellets), the crippling one comes first.
            let mine = || fresh.iter().rev().filter(|h| h.form_id == r.0);
            let since = flinch.map(|f| now - f.at - f.clip.clip.length());
            // A forced reaction (Khan Trick) plays for where it was hit, at once.
            if let Some(f) = forcing.iter().rev().find(|f| f.form_id == r.0) {
                if let Some(c) = lo.set.flinch_for(f.location) {
                    info!("hit reaction: {:08X} staggers for {:.1} s (location {}, forced: {})", r.0, c.clip.length(), f.location, f.by);
                    commands.entity(e).insert(Flinch { clip: c, at: now, forced: true });
                }
                continue;
            }
            // A power attack that broke its block staggers it whatever was hit, with the
            // torso's reaction (combat-rules.md §12) [design].
            if mine().any(|h| h.guard_broken) {
                if let Some(c) = lo.set.flinch_for(0) {
                    info!("hit reaction: {:08X} staggers for {:.1} s (its block broken by a power attack)", r.0, c.clip.length());
                    commands.entity(e).insert(Flinch { clip: c, at: now, forced: false });
                }
                continue;
            }
            let Some(h) = mine().find(|h| h.crippled).or_else(|| mine().find(|h| h.part_crippled)).or_else(|| mine().find(|h| h.torso_crippled)) else { continue };
            if !fnv_combat::damage::staggers(&tuning, h.crippled, h.part_crippled, h.torso_crippled, since) {
                continue;
            }
            if let Some(c) = lo.set.flinch_for(h.location) {
                let why = if h.crippled {
                    "crippled now"
                } else if h.part_crippled {
                    "crippled part hit"
                } else {
                    "torso crippled"
                };
                info!("hit reaction: {:08X} staggers for {:.1} s (location {}, {why})", r.0, c.clip.length(), h.location);
                commands.entity(e).insert(Flinch { clip: c, at: now, forced: false });
            }
        }
    }
}

/// Whether one of the frame's hits killed the actor `form_id`.
fn died_this_frame(hits: &[ActorHit], form_id: u32) -> bool {
    hits.iter().any(|h| h.form_id == form_id && h.died)
}

/// Put the pose its reference stores (`XRGD`) where `ragdoll.rs` takes a corpse's resting
/// pose from, for a corpse a new game laid out that has no resting pose yet.
fn authored(ragdolls: &mut Ragdolls, script: Option<&ScriptState>, world: &WorldData, c: &crate::ragdoll::Corpse, rig: &ActorRig, gt: &GlobalTransform) {
    let r = rig.reference;
    if ragdolls.rest.contains_key(&r) {
        return;
    }
    let Some(game) = script.and_then(|s| s.rt.as_ref()).map(|rt| &rt.game) else { return };
    let id = fnv_sim::FormId(r);
    if !game.starts_dead(id) || game.extra.get(&id).is_some_and(|e| e.killer.is_some()) {
        return;
    }
    let Some(p) = game.world.placed.get(id) else { return };
    let Some(d) = world.lo.index.get(&r).and_then(|d| d.last()) else { return };
    let Ok(subs) = world.lo.records[d.plugin as usize][d.ordinal as usize].subrecords() else { return };
    let Some(x) = subs.iter().find(|s| &s.sig == b"XRGD") else { return };
    let bones = crate::authored_pose::parse(&x.data);
    let names: Vec<Option<&str>> = c.data.bodies.iter().map(|b| b.object.node.as_deref()).collect();
    let scale = gt.affine().matrix3.x_axis.length();
    match crate::authored_pose::pose(&names, &rig.skel, &bones, p.pos, p.rot, scale) {
        Ok(pose) => {
            ragdolls.rest.insert(r, pose);
            ragdolls.authored += 1;
            if names.first().copied().flatten() != Some(crate::authored_pose::CHECKED_ROOT) {
                ragdolls.settle_from_stored.insert(r);
            }
        }
        Err(e) => {
            ragdolls.authored_skipped += 1;
            debug!("ragdoll: actor {r:08X}: its stored pose is not used ({e})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kills_reach_death_handling_before_the_next_volume_sync_and_dismemberment() {
        use bevy::ecs::schedule::{NodeId, Schedules};
        use nvf_play::knockouts::{KnockoutsPlugin, TestHitSet};
        use nvf_play::PlaySet;

        let mut app = App::new();
        app.add_plugins((DeathsPlugin, KnockoutsPlugin, nvf_play::hitvol::HitVolumePlugin, crate::ragdoll::RagdollPlugin));
        let mut update = app.world_mut().resource_mut::<Schedules>().remove(Update).unwrap();
        // [design: nvf-play/src/lib.rs::PlayPlugin and combat.rs::CombatPlugin]
        update.configure_sets((PlaySet::Physics, PlaySet::Activation).chain());
        update.add_systems(nvf_play::dismember::resolve.in_set(PlaySet::Activation));
        update.initialize(app.world_mut()).unwrap();
        let systems: Vec<_> = update.systems().unwrap().map(|(key, s)| (key, s.name().to_string())).collect();
        let at = |suffix: &str| systems.iter().position(|(_, n)| n.ends_with(suffix)).unwrap();
        let producer = at("knockouts::test_hits");
        let death = at("deaths::show_hits_and_deaths");
        let sync = systems.iter().position(|(_, n)| n.contains("hitvol::sync_volumes")).unwrap();
        let resolve = at("dismember::resolve");
        assert!(sync < resolve && resolve < producer && producer < death, "{systems:?}");

        // [design, #2531] Require a real dependency, so a coincidental topological
        // order cannot hide the race; also verify the actual producer belongs to it.
        let graph = update.graph();
        let hits = graph.system_sets.get_key(TestHitSet.intern()).unwrap();
        assert!(graph.systems_in_set(TestHitSet.intern()).unwrap().contains(&systems[producer].0));
        assert!(graph.dependency().graph().contains_edge(NodeId::Set(hits), NodeId::System(systems[death].0)));

        // [design: ragdoll.rs::RagdollPlugin] Ragdoll creation runs after this
        // Update's death handling, before the next Update's sync then resolve.
        let mut post = app.world_mut().resource_mut::<Schedules>().remove(PostUpdate).unwrap();
        post.initialize(app.world_mut()).unwrap();
        assert!(post.systems().unwrap().any(|(_, s)| s.name().ends_with("ragdoll::start")));
    }

    fn hit(form_id: u32, died: bool) -> ActorHit {
        ActorHit { form_id, died, knocked_out: false, location: 0, crippled: false, part_crippled: false, torso_crippled: false, by: None, share: 0.5, guard_broken: false, weapon: None }
    }

    /// A grenade rifle round's impact kills and its blast lands on the corpse in the same
    /// frame: the death is fresh, whichever hit came last (#2218).
    #[test]
    fn a_kill_followed_by_a_hit_on_the_corpse_is_a_fresh_death() {
        let hits = [hit(7, true), hit(7, false), hit(9, false)];
        assert!(died_this_frame(&hits, 7));
        assert!(!died_this_frame(&hits, 9));
        assert!(died_this_frame(&[hit(7, false), hit(7, true)], 7));
        assert!(!died_this_frame(&[], 7));
    }
}
