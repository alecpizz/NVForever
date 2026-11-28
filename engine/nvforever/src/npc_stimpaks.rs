//! NPCs use the stimpaks they carry (docs/design/combat-ai.md §21). When and which one is
//! the rules' (`fnv_game::npc_stimpaks`: hurt, in Combat or lately hit, and only a
//! stimpak little of which would go past full health); this module plays it.
//!
//! - **Who is looked at**: drawn people (the `characters\_male` skeleton), every frame,
//!   so the rules see their health fall. One is free to start when no idle a script, a
//!   package or a dialogue line asked for plays on it (fidgets, gestures and cowering
//!   give way), it is not staggered (`locomotion::Flinch`), and it neither reloads nor
//!   has a blow on its way (`npc_combat::Fighters::busy`).
//! - **The animation**: the idle the rules pick (`3rdP_SpecialIdle_Stempak.kf`, 5 s, or
//!   `3rdP_2HH_Stimpak.kf`, 3.8 s, for two-handed heavy weapons [observed: FalloutNV.esm,
//!   the base meshes archive]) plays in the idle slot (`idles.rs`) on the **upper body
//!   only** ([`upper_body`]): its tracks for the accumulation root, the pelvis and the
//!   legs are left out, so the legs keep what the actor's movement plays and it walks,
//!   runs, strafes and takes cover while it injects [design: owner 2026-10-25, "the npc
//!   will stand still for 5 seconds or more so if we can only grab the top half of the
//!   animation so that they can still move"]. The tracks it keeps take priority
//!   [`UPPER_PRIORITY`], above the armed run's arms (36), the aim (35, 50) and the held
//!   shot (55, 56) [observed: those clips' priorities, attack_anims.rs], and the idle is
//!   laid over the fight's pose (`locomotion::drive`); the stempak clip's own arm
//!   priority (30) [observed] lost to the run. The syringe (ANIO `AnimStempax`,
//!   `stempax.nif`) is drawn by `anim_objects.rs` for the stempak idle.
//! - **The injection**: at [`INJECT_SHARE`] of the clip [hypothesis: neither clip has a
//!   text key for it, only `start` and `end`, observed], the rules take one stimpak and
//!   use it. Stopped before that (killed, knocked out, another idle asked for), nothing
//!   is used. The combat AI holds its fire for as long as the idle plays.
//!
//! Log: `npc stimpak: <name> <ref> uses <item> (health <h>/<max>): <idle>` when it starts,
//! `… injects <item>: health <before> -> <after>, moving at <speed> units/s` and `… stopped before the injection`;
//! at the end `npc stimpaks summary: …`.
//!
//! Not done: the two-handed heavy idle draws no syringe (no ANIO names it), the clips'
//! visibility track of the syringe is not played (it shows for the whole idle), NPCs
//! not drawn do not heal.

use crate::idles::ActorIdle;
use crate::limbs::ActorRig;
use crate::locomotion::{Flinch, Locomotion};
use crate::npc_combat::Fighters;
use bevy::prelude::*;
use fnv_game::idles::IdleSource;
use fnv_sim::FormId;
use nvf_assets::anim_play::BoundClip;
use nvf_play::script::{with_world, ScriptState};
use nvf_play::sim::SimState;
use std::collections::{HashMap, HashSet};

/// The share of the clip at which the stimpak takes effect [hypothesis: module notes].
pub const INJECT_SHARE: f32 = 0.5;

/// The track priority of the clip's upper body (module notes) [design].
pub const UPPER_PRIORITY: i32 = 57;

/// Frames a stimpak's idle may take to show before the use is given up (its clip did
/// not load) [engineering choice].
const START_FRAMES: u32 = 3;

/// The nodes below the waist a stimpak idle leaves to the movement [observed: the
/// skeleton's node names, the clips' tracks].
pub(crate) fn lower_body(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "bip01" || n == "bip01 nonaccum" || n == "bip01 pelvis" || ["thigh", "calf", "foot", "toe"].iter().any(|p| n.contains(p))
}

/// `bound` with its lower body left out and the rest at [`UPPER_PRIORITY`] (or its own,
/// when higher): `names` are the skeleton's node names by index.
pub fn upper_body(mut bound: BoundClip, names: &[Option<String>]) -> BoundClip {
    keep_upper(&mut bound.tracks, names);
    bound.root_track = None;
    bound
}

/// [`upper_body`]'s rule on (node index, track, priority) entries.
fn keep_upper<T>(tracks: &mut Vec<(usize, T, i32)>, names: &[Option<String>]) {
    tracks.retain(|(i, _, _)| !names.get(*i).and_then(|n| n.as_deref()).is_some_and(lower_body));
    for t in tracks.iter_mut() {
        t.2 = t.2.max(UPPER_PRIORITY);
    }
}

/// A stimpak being used, as this module follows it.
struct Using {
    idle: u32,
    item: FormId,
    /// The idle has shown; frames waited for it before.
    seen: bool,
    waited: u32,
}

#[derive(Resource, Default)]
pub struct NpcStimpaks {
    using: HashMap<u32, Using>,
    started: u32,
    injected: u32,
    stopped: u32,
    healed: f64,
}

pub struct NpcStimpaksPlugin;

impl Plugin for NpcStimpaksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NpcStimpaks>()
            .add_systems(Update, drive.after(nvf_play::PlaySet::Scripts).after(crate::npc_combat::fight).before(crate::idles::drive))
            .add_systems(Last, report);
    }
}

/// What [`drive`] reads of a drawn actor.
type Looked = (&'static ActorRig, &'static InheritedVisibility, Option<&'static ActorIdle>, Option<&'static Flinch>, Option<&'static Locomotion>);

/// Whether what plays on an actor's idle slot gives way to a stimpak.
fn gives_way(source: IdleSource) -> bool {
    matches!(source, IdleSource::Fidget | IdleSource::HolsteredFidget | IdleSource::FurnitureFidget | IdleSource::CombatIdle | IdleSource::CalmIdle | IdleSource::Gesture | IdleSource::Cower | IdleSource::PackageIdles | IdleSource::MarkerIdles)
}

#[allow(clippy::too_many_arguments)]
fn drive(
    time: Res<Time>,
    mut st: ResMut<NpcStimpaks>,
    mut script: Option<ResMut<ScriptState>>,
    mut sim: ResMut<SimState>,
    fighters: Res<Fighters>,
    actors: Query<Looked>,
) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    if rt.game.menu.is_some() || rt.game.dialogue.is_some() {
        return;
    }
    let clock = time.elapsed_secs();
    let st = &mut *st;
    with_world(rt, &mut sim, |rt| {
        let g = &mut rt.game;
        let mut drawn: HashSet<u32> = HashSet::new();
        for (rig, vis, idle, flinch, lo) in &actors {
            let me = rig.reference;
            if !vis.get() || !rig.skeleton.to_ascii_lowercase().replace('/', "\\").contains("characters\\_male\\") {
                continue;
            }
            drawn.insert(me);
            let a = FormId(me);
            let shown = idle.and_then(|i| i.0.cur.as_ref());
            if let Some(u) = st.using.get_mut(&me) {
                let ours = shown.filter(|p| p.source == IdleSource::Stimpak && p.idle == u.idle && p.group.is_none());
                let name = g.data.name(a);
                match ours {
                    Some(p) => {
                        u.seen = true;
                        let due = p.play.time() >= p.play.keys.length * INJECT_SHARE;
                        if due && g.npc_stimpaks.using.get(&a).is_some_and(|x| !x.injected) {
                            match g.inject_stimpak(a) {
                                Some((before, after)) => {
                                    st.injected += 1;
                                    st.healed += after - before;
                                    let speed = lo.map_or(0.0, |l| l.velocity[0].hypot(l.velocity[1]));
                                    info!("npc stimpak: {name} {me:08X} injects {}: health {before:.0} -> {after:.0}, moving at {speed:.0} units/s", g.data.name(u.item));
                                }
                                None => info!("npc stimpak: {name} {me:08X} no longer carries {} or cannot use it", g.data.name(u.item)),
                            }
                        }
                        if p.play.ended() {
                            g.end_stimpak(a);
                            st.using.remove(&me);
                        }
                    }
                    None if u.seen || u.waited >= START_FRAMES => {
                        if g.npc_stimpaks.using.get(&a).is_some_and(|x| !x.injected) {
                            st.stopped += 1;
                            info!("npc stimpak: {name} {me:08X} stopped before the injection: {} not used", g.data.name(u.item));
                        }
                        g.end_stimpak(a);
                        st.using.remove(&me);
                    }
                    None => u.waited += 1,
                }
                continue;
            }
            let staggered = flinch.is_some_and(|f| clock - f.at < f.clip.clip.length());
            let free = !staggered && !fighters.busy(me) && shown.is_none_or(|p| p.play.ended() || gives_way(p.source));
            let Some((item, idle_form)) = g.stimpak_wanted(a, free) else { continue };
            let (health, max) = g.health_of(a);
            info!("npc stimpak: {} {me:08X} uses {} (health {health:.0}/{max:.0}): {}", g.data.name(a), g.data.name(item), g.data.name(idle_form));
            g.begin_stimpak(a, item, idle_form);
            st.started += 1;
            st.using.insert(me, Using { idle: idle_form.0, item, seen: false, waited: 0 });
        }
        st.using.retain(|a, _| drawn.contains(a));
        g.forget_stimpak_watch(|a| drawn.contains(&a.0));
    });
}

fn report(mut exit: MessageReader<AppExit>, st: Res<NpcStimpaks>) {
    if exit.read().next().is_none() {
        return;
    }
    info!("npc stimpaks summary: {} stimpak idles started, {} injected ({:.0} health restored), {} stopped before the injection", st.started, st.injected, st.healed, st.stopped);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_upper_body_keeps_spine_arms_and_head_above_the_run() {
        let names: Vec<Option<String>> = ["Bip01", "Bip01 NonAccum", "Bip01 Pelvis", "Bip01 L Thigh", "Bip01 R Calf", "Bip01 L Foot", "Bip01 R Toe0", "Bip01 Spine", "Bip01 L Forearm", "Bip01 Head"].iter().map(|n| Some(n.to_string())).collect();
        let mut tracks: Vec<(usize, (), i32)> = (0..names.len()).map(|i| (i, (), if i == 9 { 95 } else { 30 })).collect();
        keep_upper(&mut tracks, &names);
        let kept: Vec<(usize, i32)> = tracks.iter().map(|(i, _, p)| (*i, *p)).collect();
        assert_eq!(kept, vec![(7, UPPER_PRIORITY), (8, UPPER_PRIORITY), (9, 95)]);
    }
}
