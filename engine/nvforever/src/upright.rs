//! A standing actor whose body lies over (#3446): an actor of an upright skeleton (its
//! bind pose's pelvis → head line within [`UPRIGHT_DEG`] of up) that is alive, not dying, not
//! knocked out, not a corpse and not on furniture, whose pelvis → head line leans more
//! than [`TIPPED_DEG`] from its root's up, is logged once each time it tips, with the
//! movement clip it plays and the idle that holds it, if any.
//!
//! Players reported NPCs lying flat for a moment while being talked to or in a fight,
//! alive and not unconscious, with nothing in the log (#3446; #2859, fixed by #2860:
//! a turn clip's tilted accumulation root). Nothing named the clip; this line does
//! [design: a diagnostic, it changes no pose]. A body that lies as the game means it to
//! (an unconscious actor, a death, a bed, a chair) is left out above; an idle that
//! lies on the ground is logged with its name, so a reader can tell it apart.
//!
//! Log: `upright: <name> (<ref>) leans <deg>° from upright while standing: <group>
//! playing[, idle <idle>]`, and at exit `upright: <n> tips of standing actors seen`.

use crate::furniture::OnFurniture;
use crate::knockouts::KnockedOut;
use crate::limbs::ActorRig;
use crate::locomotion::{Dying, Locomotion};
use crate::ragdoll::Corpse;
use bevy::prelude::*;
use std::collections::HashMap;

/// How far the pelvis → head line may lean from the root's up before the body counts
/// as lying over, degrees [design: a person bends to about 30° in a sneak or a lunge;
/// lying is near 90°].
pub const TIPPED_DEG: f32 = 60.0;

/// Back under this, the actor stands again and a later tip is logged anew [design].
const UPRIGHT_DEG: f32 = 40.0;

pub struct UprightPlugin;

impl Plugin for UprightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tips>().add_systems(PostUpdate, watch.after(TransformSystems::Propagate)).add_systems(Last, report);
    }
}

#[derive(Resource, Default)]
struct Tips {
    /// The actors tipped now (by reference).
    tipped: HashMap<u32, f32>,
    /// Tips logged since the start.
    seen: usize,
    /// Each skeleton's pelvis and head bone, or `None` without them.
    bones: HashMap<String, Option<(usize, usize)>>,
}

/// The angle between `v` and +Z, degrees; `None` for a vector too short to tell.
pub fn lean_deg(v: Vec3) -> Option<f32> {
    let l = v.length();
    (l > 1e-3).then(|| (v.z / l).clamp(-1.0, 1.0).acos().to_degrees())
}

type Watched = (&'static ActorRig, &'static GlobalTransform, &'static Locomotion, Option<&'static crate::idles::ActorIdle>, Option<&'static Name>);
type Excluded = (Without<Dying>, Without<Corpse>, Without<KnockedOut>, Without<OnFurniture>);

fn watch(mut tips: ResMut<Tips>, actors: Query<Watched, Excluded>, globals: Query<&GlobalTransform>) {
    let tips = &mut *tips;
    for (rig, root, lo, idle, name) in &actors {
        let bones = *tips.bones.entry(rig.skeleton.clone()).or_insert_with(|| {
            let find = |n: &str| rig.skel.by_name.get(n).copied();
            let (p, h) = (find("Bip01 Pelvis")?, find("Bip01 Head")?);
            // Only a skeleton that stands upright in its bind pose: a four-legged one's
            // pelvis -> head line is level (the bighorner's, the dog's).
            let bind = rig.skel.bind_world[h].translation - rig.skel.bind_world[p].translation;
            (lean_deg(Vec3::from(bind))? < UPRIGHT_DEG).then_some((p, h))
        });
        let Some((pelvis, head)) = bones else { continue };
        let (Some(&p), Some(&h)) = (rig.nodes.get(pelvis), rig.nodes.get(head)) else { continue };
        let (Ok(p), Ok(h)) = (globals.get(p), globals.get(h)) else { continue };
        // In the root's own frame (z up, as the skeleton is).
        let to_root = root.affine().inverse();
        let line = to_root.transform_point3(h.translation()) - to_root.transform_point3(p.translation());
        let Some(deg) = lean_deg(line) else { continue };
        let r = rig.reference;
        if tips.tipped.contains_key(&r) {
            if deg < UPRIGHT_DEG {
                tips.tipped.remove(&r);
            }
            continue;
        }
        if deg > TIPPED_DEG {
            tips.tipped.insert(r, deg);
            tips.seen += 1;
            let idle = match idle.and_then(|i| i.0.cur.as_ref()) {
                Some(p) if p.idle != 0 => format!(", idle {:08X}", p.idle),
                Some(p) => format!(", group {:?}", p.group),
                None => String::new(),
            };
            info!("upright: {} ({r:08X}) leans {deg:.0}° from upright while standing: {} playing{idle}", name.map_or("actor", |n| n.as_str()), lo.blend.cur.mv.group());
        }
    }
}

fn report(mut exit: MessageReader<AppExit>, tips: Res<Tips>) {
    if exit.read().next().is_some() {
        info!("upright: {} tips of standing actors seen", tips.seen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lean_is_measured_from_up() {
        assert!(lean_deg(Vec3::Z).unwrap() < 0.1);
        assert!((lean_deg(Vec3::new(1.0, 0.0, 1.0)).unwrap() - 45.0).abs() < 0.1);
        assert!((lean_deg(Vec3::X * 30.0).unwrap() - 90.0).abs() < 0.1);
        assert!(lean_deg(Vec3::ZERO).is_none());
    }
}
