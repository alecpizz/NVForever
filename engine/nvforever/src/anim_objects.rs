//! [design, #2685; docs/spec-notes/idles.md section 10] ANIO attachments use the
//! placed-model rig, material and particle players with the actor idle's external KF.
//! Attach at Prn (root fallback), remove on replacement, retain held CLAMP effects.

use crate::idles::{ActorIdle, Playing};
use crate::limbs::ActorRig;
use crate::world::WorldData;
use bevy::math::Affine3A;
use bevy::prelude::*;
use nvf_assets::actor::rigid_local;
use nvf_assets::anim_play::{BoundClip, Playing as ModelPlaying};
use nvf_assets::model::ModelCtx;
use std::collections::HashMap;
use std::sync::Arc;

/// The animated objects by the IDLE they go with: (editor ID, model) of each.
#[derive(Resource, Default)]
pub struct AnimObjects {
    by_idle: HashMap<u32, Vec<(String, String)>>,
    stats: Stats,
    startup: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Stats {
    /// Objects drawn on actors, idles that had objects, objects whose model did not load
    /// or whose bone the skeleton lacks.
    drawn: u32,
    idles: u32,
    failed: u32,
}

impl AnimObjects {
    pub fn build(world: &WorldData) -> AnimObjects {
        let mut issues = fnv_sim::extract::Issues::default();
        let all = fnv_actor::idle::anim_objects(world.lo, world.schema, &mut issues);
        let mut by_idle: HashMap<u32, Vec<(String, String)>> = HashMap::new();
        let mut usable = 0;
        for a in &all {
            if let (Some(idle), Some(model)) = (a.idle, a.model.clone()) {
                by_idle.entry(idle.0).or_default().push((a.edid.clone().unwrap_or_else(|| a.id.to_string()), model));
                usable += 1;
            }
        }
        let startup = format!("anim objects: {} records, {usable} with an idle and a model, for {} idles", all.len(), by_idle.len());
        AnimObjects { by_idle, stats: Stats::default(), startup: Some(startup) }
    }
}

/// [design, #2685] Complete attachment roots, owned by one idle slot serial.
#[derive(Component)]
pub struct HeldObjects {
    serial: u32,
    entities: Vec<Entity>,
}

/// [design, #2685] Sample the exact end as well as the start of a held CLAMP idle.
fn clip_time(p: &Playing<Arc<BoundClip>>) -> f32 {
    p.clip.clip.start + p.play.time().min(p.play.keys.length)
}

/// [design, #2685] Keep outgoing props during the existing cross-fade, then remove
/// them when that idle disappears from the slot.
fn attachment_idle<'a>(idle: Option<&'a ActorIdle>, objects: &AnimObjects) -> Option<&'a Playing<Arc<BoundClip>>> {
    let has = |p: &&Playing<Arc<BoundClip>>| p.group.is_none() && objects.by_idle.contains_key(&p.idle);
    idle.and_then(|i| i.0.cur.as_ref().filter(has).or(i.0.prev.as_ref().filter(has)))
}

/// Read the records once the world data is there.
pub fn read(mut a: ResMut<AnimObjects>, world: Res<WorldData>) {
    *a = AnimObjects::build(&world);
    if let Some(line) = a.startup.take() {
        info!("{line}");
    }
}

/// The bind transform an object hung from `b` misses while the idle `clip` plays: the
/// node's own bind (local to its parent) when `b` is the clip's accumulation root, which
/// the animator holds at the identity (anim_blend.rs, actors.md §7.2); else the identity.
/// The objects hung from `Bip01` were made against its bind: `aogenericmeal.nif` and
/// `aostudenttestpaper.nif` hold the plate and the paper 42 and 51 units along their
/// root's axes, which only that bind (turned 90° and 67.8 up) puts in front of the
/// seated actor at the table's height; under the identity the plate lay on the floor
/// at his side (#3610) [observed: `fnvtool nif decode` of the two and of
/// `characters\_male\skeleton.nif`; hypothesis: the original hangs them from the bind].
fn held_root_bind(skel: &nvf_assets::actor::PreparedSkeleton, b: usize, clip: &nvf_assets::anim_play::Clip) -> Affine3A {
    let held = clip.accum_root.as_deref().is_some_and(|r| skel.names.get(b).and_then(|n| n.as_deref()).is_some_and(|n| n.eq_ignore_ascii_case(r)));
    if !held {
        return Affine3A::IDENTITY;
    }
    let Some(&world) = skel.bind_world.get(b) else { return Affine3A::IDENTITY };
    match skel.parents.get(b).copied().flatten().and_then(|p| skel.bind_world.get(p)) {
        Some(parent) => parent.inverse() * world,
        None => world,
    }
}

/// [design, #2685] Spawn one complete ANIO tree under Prn, falling back to root.
fn attach_model(
    commands: &mut Commands, rig: &ActorRig, prepared: &nvf_assets::model::PreparedModel,
    model: &str, clip: &Arc<nvf_assets::anim_play::Clip>, prn: Option<&str>, time: f32,
) -> Option<(Entity, HashMap<String, Entity>, String)> {
    let found = prn.and_then(|name| rig.skel.by_name.get(name)).copied();
    if let (Some(name), None) = (prn, found) {
        warn!("anim object: {model}: its parent node {name} is not in the skeleton; hung from the root");
    }
    let b = found.unwrap_or(0);
    let &parent = rig.nodes.get(b)?;
    let bone_name = rig.skel.names[b].clone().unwrap_or_else(|| "<root>".into());
    let local = held_root_bind(&rig.skel, b, clip) * rigid_local(&rig.skel, b, &prepared.root, &Affine3A::IDENTITY);
    let part = prepared.rig.as_ref()?;
    // [design, #2685] Preparation also expands tracks to shapes sharing properties.
    let drawn_clip = part.idle.as_ref().unwrap_or(clip);
    let (root, _, nodes) = part.spawn_playing(commands, parent, Transform::from_matrix(Mat4::from(local)), model, 0.0, Some(drawn_clip));
    let mut player = ModelPlaying::new(model, drawn_clip, |name| nodes.get(name).copied());
    player.time = time;
    player.held = true;
    commands.entity(root).insert(player);
    if prepared.particles {
        commands.entity(root).insert((
            crate::particles::ModelEmitter::placed(model, rig.reference),
            crate::particles::IdleParticleRig { nodes: nodes.clone(), clip: clip.clone() },
        ));
    }
    Some((root, nodes, bone_name))
}

/// [design, #2685] Attach full model hierarchies and synchronize to the idle clock.
pub fn hold(
    mut commands: Commands,
    mut objects: ResMut<AnimObjects>,
    mut models: ModelCtx,
    actors: Query<(Entity, &ActorRig, Option<&ActorIdle>, Option<&HeldObjects>)>,
    mut players: Query<&mut ModelPlaying>,
) {
    for (e, rig, idle, held) in &actors {
        let playing = attachment_idle(idle, &objects);
        let want = playing.map(|p| (p.idle, p.serial));
        if held.map(|h| h.serial) == want.map(|w| w.1) {
            if let (Some(h), Some(p)) = (held, playing) {
                for &root in &h.entities {
                    if let Ok(mut player) = players.get_mut(root) {
                        player.time = clip_time(p);
                    }
                }
            }
            continue;
        }
        if let Some(h) = held {
            for &root in &h.entities {
                commands.entity(root).despawn();
            }
            commands.entity(e).remove::<HeldObjects>();
        }
        let (Some((idle, serial)), Some(p)) = (want, playing) else { continue };
        objects.stats.idles += 1;
        let mut entities = Vec::new();
        for (edid, model) in objects.by_idle[&idle].clone() {
            let clip = &p.clip.clip;
            let Some(prepared) = models.model_for_idle(&model, idle, clip) else {
                objects.stats.failed += 1;
                continue;
            };
            let Some(part) = &prepared.rig else {
                objects.stats.failed += 1;
                continue;
            };
            let strings = models.node_strings(&model, "Prn");
            let prn = part.nodes.first().and_then(|n| n.name.as_ref()).and_then(|name| strings.get(name));
            let Some((root, nodes, bone_name)) = attach_model(&mut commands, rig, &prepared, &model, clip, prn.map(String::as_str), clip_time(p)) else { continue };
            for (name, _) in clip.tracks.iter().filter(|(name, _)| nodes.contains_key(name)) {
                info!("anim object: {:08X} {edid} node {name} follows the idle's track", rig.reference);
            }
            entities.push(root);
            objects.stats.drawn += 1;
            info!("anim object: {:08X} holds {model} on {bone_name} ({edid}, idle {idle:08X})", rig.reference);
        }
        commands.entity(e).insert(HeldObjects { serial, entities });
    }
}

pub fn report(mut exit: MessageReader<AppExit>, objects: Res<AnimObjects>) {
    if exit.read().next().is_none() {
        return;
    }
    let s = objects.stats;
    info!("anim objects summary: {} objects drawn for {} idles played, {} could not be drawn", s.drawn, s.idles, s.failed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::{system::RunSystemOnce, world::CommandQueue};
    use fnv_actor::idle_play::{IdleKeys, IdlePlay};
    use fnv_nif::anim::{Channel, Sequence, Track, TrackData, ValueTrack};
    use nvf_assets::anim_play::Clip;
    use nvf_assets::model::{PreparedModel, Rig, RigNode, RigShape};

    /// #3610: a meal hung from `Bip01`, the accumulation root the animator holds at the
    /// identity, gets `Bip01`'s bind back; a hand's object and a clip without that
    /// root do not.
    #[test]
    fn an_object_on_the_held_accumulation_root_gets_its_bind_back() {
        let turned = Affine3A::from_rotation_translation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), Vec3::new(0.0, 0.657, 67.771));
        let hand = Affine3A::from_translation(Vec3::new(20.0, 0.0, 0.0));
        let skel = nvf_assets::actor::PreparedSkeleton {
            names: vec![Some("Scene Root".into()), Some("Bip01".into()), Some("Bip01 R Hand".into())],
            parents: vec![None, Some(0), Some(1)],
            bind_world: vec![Affine3A::IDENTITY, turned, turned * hand],
            ..default()
        };
        let clip = |accum: Option<&str>| Clip::from_sequence(&Sequence {
            block: 0, name: Some("SitTableChairEatA".into()), start_time: 0.0, stop_time: 1.0,
            frequency: 1.0, cycle_type: 0, accum_root_name: accum.map(String::from), text_keys: Vec::new(), tracks: Vec::new(),
        });
        let eat = clip(Some("Bip01"));
        assert!(held_root_bind(&skel, 1, &eat).abs_diff_eq(turned, 1e-5));
        assert_eq!(held_root_bind(&skel, 2, &eat), Affine3A::IDENTITY);
        assert_eq!(held_root_bind(&skel, 1, &clip(None)), Affine3A::IDENTITY);
        // The meal's authored root and node offset [observed: aogenericmeal.nif] land in
        // front of the seated actor at the table's height with the bind, on the floor
        // at his side without it.
        let root = Affine3A::from_mat3(Mat3::from_cols_array(&[
            -0.010611, 0.633814, 0.773413, -0.998467, -0.048737, 0.026242, 0.054326, -0.771949, 0.633359,
        ]).transpose());
        let meal = Vec3::new(-1.215, 41.551, 55.598);
        let placed = (held_root_bind(&skel, 1, &eat) * root).transform_point3(meal);
        assert!(placed.y > 60.0 && placed.z > 60.0 && placed.x.abs() < 5.0, "{placed}");
        let lost = root.transform_point3(meal);
        assert!(lost.z < 10.0, "{lost}");
    }

    #[test]
    fn idle_attaches_full_model_reaches_materials_holds_end_and_detaches_on_replacement() {
        let mut world = World::new();
        crate::particles::model_ctx_resources(&mut world);
        let bones: Vec<_> = (0..2).map(|_| world.spawn(Transform::IDENTITY).id()).collect();
        let skel = nvf_assets::actor::PreparedSkeleton {
            names: vec![Some("Root".into()), Some("OwnedBone".into())],
            by_name: HashMap::from([("Root".into(), 0), ("OwnedBone".into(), 1)]),
            bind_world: vec![Affine3A::IDENTITY; 2], ..default()
        };
        let rig = ActorRig { reference: 7, base: 8, body: 8, skeleton: "owned.nif".into(),
            skel: Arc::new(skel), nodes: bones.clone(), base_scale: 1.0 };
        let actor = world.spawn(rig.clone()).id();
        let mut seq = Sequence {
            block: 0, name: Some("OwnedIdle".into()), start_time: 0.0, stop_time: 2.0,
            frequency: 1.0, cycle_type: 2, accum_root_name: None, text_keys: Vec::new(),
            tracks: vec![Track {
                node_name: Some("OwnedGlow".into()), controller_type: Some("NiMaterialColorController".into()),
                controller_id: Some("SELF_ILLUM".into()), priority: Some(1), data: TrackData::Other("OwnedSynthetic".into()),
                bools: None, values: Some(ValueTrack::Point3(Channel::Static(Some([0.3, 0.5, 0.7])))), interpolator_id: None,
            }],
        };
        for (controller, id, value) in [
            ("NiAlphaController", "", 0.4),
            ("NiTextureTransformController", "0-0-TT_TRANSLATE_U", 0.25),
            ("NiTextureTransformController", "0-0-TT_TRANSLATE_V", 0.75),
            ("BSMaterialEmittanceMultController", "", 2.0),
        ] {
            seq.tracks.push(Track {
                node_name: Some("OwnedGlow".into()), controller_type: Some(controller.into()),
                controller_id: Some(id.into()), priority: Some(1), data: TrackData::Other("OwnedSynthetic".into()),
                bools: None, values: Some(ValueTrack::Float(Channel::Static(Some([value])))), interpolator_id: None,
            });
        }
        let clip = Arc::new(Clip::from_sequence(&seq));
        let prepared = PreparedModel { particles: true, root: Affine3A::IDENTITY, rig: Some(Rig {
            nodes: vec![
                RigNode { name: Some("Effects".into()), parent: None, local: Transform::IDENTITY, hidden: false, billboard: None },
                RigNode { name: Some("OwnedGlow".into()), parent: Some(0), local: Transform::IDENTITY, hidden: false, billboard: None },
            ],
            shapes: vec![RigShape { node: 1, mesh: default(), material: default(), skin: None, morph: None,
                look: ("OwnedGlow".into(), [0.0; 3], 1.0, 1.0), refraction: 0.0, animated: true, block: 0 }],
            idle: None, door: None, groups: Vec::new(), effect: None,
        }), ..default() };
        let mut queue = CommandQueue::default();
        let (root, nodes, bone) = attach_model(&mut Commands::new(&mut queue, &world), &rig, &prepared,
            "owned.nif", &clip, Some("OwnedBone"), 0.0).unwrap();
        queue.apply(&mut world);
        assert_eq!(bone, "OwnedBone");
        assert_eq!(world.get::<ChildOf>(root).unwrap().parent(), bones[1]);
        assert!(world.get::<crate::particles::ModelEmitter>(root).is_some());
        let mats = world.get::<nvf_assets::material_anim::RigMaterials>(root).unwrap();
        let values = nvf_assets::material_anim::sample(&mats.slots, &mats.idle, 2.0, 2.0, false);
        assert_eq!(values[0].colour, [0.3, 0.5, 0.7]);
        assert_eq!(values[0].mult, 2.0);
        assert_eq!(values[0].alpha, 0.4);
        assert_eq!(values[0].uv, [0.25, 0.75]);
        let mut idle = ActorIdle::default();
        let bound = Arc::new(BoundClip::bind(clip.clone(), |_| None));
        let play = IdlePlay::new(IdleKeys::read(0.0, 2.0, &[]), Some(1)).clamped();
        let serial = idle.0.start_group(0, bound.clone(), play);
        let p = idle.0.cur.as_mut().unwrap();
        p.idle = 9;
        p.group = None;
        p.play.advance(100.0);
        world.insert_resource(AnimObjects { by_idle: HashMap::from([(9, vec![("Owned".into(), "owned.nif".into())])]), ..default() });
        world.entity_mut(actor).insert((idle, HeldObjects { serial, entities: vec![root] }));
        world.run_system_once(hold).unwrap();
        assert_eq!(world.get::<ModelPlaying>(root).unwrap().time, 2.0);
        world.get_mut::<ActorIdle>(actor).unwrap().0.start_group(1, bound, play);
        world.run_system_once(hold).unwrap();
        assert!(world.get_entity(root).is_err());
        assert!(world.get_entity(nodes["OwnedGlow"]).is_err());
        let (fallback, _, bone) = attach_model(&mut Commands::new(&mut queue, &world), &rig, &prepared,
            "owned.nif", &clip, Some("MissingBone"), 0.0).unwrap();
        queue.apply(&mut world);
        assert_eq!(bone, "Root");
        assert_eq!(world.get::<ChildOf>(fallback).unwrap().parent(), bones[0]);
    }
}
