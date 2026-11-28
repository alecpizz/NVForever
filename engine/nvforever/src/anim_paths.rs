//! kNVSE's SetActorAnimationPath on the drawn actors, slice 2 of
//! docs/design/knvse-anim.md: an actor with replacement paths plays from its own copy of
//! the standing set, the replaced moves taken from the script's files
//! (`ClipSet::with_replacements`); when the script takes them back the shared set
//! returns. The paths come from the game (`fnv_game::nvse_anim::AnimPaths`), after the
//! files kNVSE's AnimGroupOverride folders give the actor (`Game::anim_overrides`,
//! `fnv_game::nvse_anim_folders`).

use crate::limbs::ActorRig;
use crate::locomotion::{Locomotion, ALERT_STANCE, SNEAK_STANCE, SWIM_STANCES};
use bevy::prelude::*;
use fnv_sim::FormId;
use nvf_assets::actor::{ActorCtx, ActorParts};
use nvf_assets::model::ModelCtx;
use nvf_play::script::ScriptState;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub struct AnimPathsPlugin;

impl Plugin for AnimPathsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActorAnimFiles>().add_systems(Update, apply.before(crate::locomotion::drive));
    }
}

/// The third-person files each drawn actor's kNVSE replacements give it now (folders and
/// SetActorAnimationPath, `Game::anim_overrides`), by reference, for the players of clips
/// other than locomotion: `attack_anims` takes an NPC's attack, reload and aim-form clips
/// from them ([`replaced`]). Empty for an actor with none.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct ActorAnimFiles(pub HashMap<u32, Vec<String>>);

/// The replacement among `files` for the clip at `path` (`meshes\…`): the last file of
/// the same file name, any case, as a `meshes\` path [hypothesis, as `idles::clip_path_for`:
/// a kNVSE replacement shares the replaced file's name, and of two the later wins].
pub fn replaced(files: &[String], path: &str) -> Option<String> {
    let name = |p: &str| p.rsplit(['\\', '/']).next().map(str::to_ascii_lowercase);
    let want = name(path)?;
    files.iter().rev().find(|f| name(f).as_deref() == Some(want.as_str())).map(|f| fnv_actor::idle::meshes_path(f))
}

/// Original shared posture sets saved before applying per-actor kNVSE overrides.
#[derive(Clone)]
struct SharedSets {
    stand: Arc<crate::locomotion::ClipSet>,
    sneak: Option<Arc<crate::locomotion::ClipSet>>,
    hurt: Option<Arc<crate::locomotion::ClipSet>>,
    swim: Option<Arc<crate::locomotion::ClipSet>>,
    alert: Option<Arc<crate::locomotion::ClipSet>>,
}

/// Per actor: the paths applied and the shared posture sets to go back to.
type Applied = HashMap<u32, (Vec<String>, SharedSets)>;

/// Each frame: the files that apply to each drawn actor now. SetActorAnimationPath's
/// conditional paths count while their condition holds (`Game::anim_overrides_now`, run
/// only for actors that have one), with the world put back.
#[allow(clippy::too_many_arguments)]
fn apply(
    mut script: Option<ResMut<ScriptState>>,
    sim: Option<ResMut<nvf_play::sim::SimState>>,
    parts: Option<ResMut<ActorParts>>,
    mut models: ModelCtx,
    mut actors: Query<(&ActorRig, &mut Locomotion)>,
    mut applied: Local<Applied>,
    mut files: Option<ResMut<ActorAnimFiles>>,
) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let forced = rt.game.take_npc_anim_refreshes();
    if rt.game.anim_paths.actor.is_empty() && rt.game.anim_paths.weapon.is_empty() && rt.game.anim_folders.active.is_empty() && applied.is_empty() && forced.is_empty() {
        if let Some(f) = files.as_mut().filter(|f| !f.0.is_empty()) {
            f.0.clear();
        }
        return;
    }
    let Some(mut parts) = parts else { return };
    let conditional: HashSet<u32> = rt.game.anim_paths.conditions.keys().filter(|(_, first, _)| !first).map(|(a, ..)| a.0).collect();
    let mut wanted: HashMap<u32, Vec<String>> = actors.iter().map(|(rig, _)| (rig.reference, Vec::new())).collect();
    let mut run = |rt: &mut nvf_play::script::Runtime| {
        for (actor, paths) in wanted.iter_mut() {
            let id = FormId(*actor);
            let is_forced = forced.contains(&id);
            if is_forced {
                rt.game.refresh_anim_folders_for(id);
            }
            *paths = if is_forced || conditional.contains(actor) { rt.game.anim_overrides_now(id, false) } else { rt.game.anim_overrides(id, false) };
        }
    };
    match sim {
        // The world put back: the conditions and the weapon in hand read it.
        Some(mut sim) => nvf_play::script::with_world(rt, &mut sim, run),
        None => run(rt),
    }
    // The files for the other clip players, written only when they change.
    if let Some(f) = files.as_mut() {
        let now: HashMap<u32, Vec<String>> = wanted.iter().filter(|(_, p)| !p.is_empty()).map(|(a, p)| (*a, p.clone())).collect();
        if f.0 != now {
            f.0 = now;
        }
    }
    let mut ctx = ActorCtx { cache: &mut parts, models: &mut models };
    for (rig, mut lo) in &mut actors {
        let paths = wanted.remove(&rig.reference).unwrap_or_default();
        let now = applied.get(&rig.reference).map(|(p, _)| p.clone()).unwrap_or_default();
        let force = forced.contains(&FormId(rig.reference));
        if !force && paths == now {
            continue;
        }
        let shared = match applied.get(&rig.reference) {
            Some((_, s)) => s.clone(),
            None => SharedSets {
                stand: lo.stand_set(),
                sneak: lo.sneak.clone(),
                hurt: lo.hurt.clone(),
                swim: lo.swim.clone(),
                alert: lo.alert.clone(),
            },
        };
        if paths.is_empty() {
            info!("anim paths: {:08X} plays its shared set again", rig.reference);
            lo.replace_posture_sets(shared.stand, shared.sneak, shared.hurt, shared.swim, shared.alert);
            applied.remove(&rig.reference);
            continue;
        }
        let (stand, n_stand) = shared.stand.with_replacements(&mut ctx, &rig.skel, &paths);
        let mut total = n_stand;
        let replace_opt = |opt: &Option<Arc<crate::locomotion::ClipSet>>, stances: &[&str], ctx: &mut ActorCtx, total: &mut usize| -> Option<Arc<crate::locomotion::ClipSet>> {
            let base = opt.as_ref()?;
            let (set, n) = base.with_replacements_for_stances(ctx, &rig.skel, &paths, stances);
            if n > 0 {
                *total += n;
                Some(Arc::new(set))
            } else {
                Some(base.clone())
            }
        };
        let sneak = replace_opt(&shared.sneak, &[SNEAK_STANCE], &mut ctx, &mut total);
        let hurt = replace_opt(&shared.hurt, &["hurtmt"], &mut ctx, &mut total);
        let swim = replace_opt(&shared.swim, &SWIM_STANCES, &mut ctx, &mut total);
        let alert = replace_opt(&shared.alert, &[ALERT_STANCE], &mut ctx, &mut total);
        info!("anim paths: {:08X} plays {total} replaced move(s) from {} file(s)", rig.reference, paths.len());
        lo.replace_posture_sets(Arc::new(stand), sneak, hurt, swim, alert);
        applied.insert(rig.reference, (paths, shared));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locomotion::ClipSet;
    use fnv_game::game::RunCtx;
    use fnv_script::vm::XVal;
    use nvf_assets::actor::PreparedSkeleton;

    #[test]
    fn a_replacement_is_the_last_file_of_the_clips_name_in_any_case() {
        let files = vec![
            "AnimGroupOverride\\WWPSR\\_Male\\2haAttack3.kf".to_string(),
            "AnimGroupOverride\\WWPSR\\_Male\\2haReloadF.kf".to_string(),
            "meshes/AnimGroupOverride/Later/_male/2haattack3.KF".to_string(),
        ];
        assert_eq!(replaced(&files, "meshes\\characters\\_male\\2haattack3.kf").as_deref(), Some("meshes\\animgroupoverride\\later\\_male\\2haattack3.kf"));
        assert_eq!(replaced(&files, "meshes\\characters\\_male\\2hareloadf.kf").as_deref(), Some("meshes\\animgroupoverride\\wwpsr\\_male\\2hareloadf.kf"));
        assert_eq!(replaced(&files, "meshes\\characters\\_male\\2haattack3up.kf"), None, "no file of that name: the clip itself");
        assert_eq!(replaced(&[], "meshes\\characters\\_male\\2haattack3.kf"), None);
    }

    #[test]
    fn the_files_are_published_for_the_other_clip_players_and_cleared() {
        let (mut app, _files) = crate::outfits::tests::asset_fixture();
        let npc = FormId(0x2000);
        app.world_mut().spawn((
            ActorRig {
                reference: npc.0,
                base: crate::outfits::tests::OTHER.0,
                body: crate::outfits::tests::OTHER.0,
                skeleton: "skeleton.nif".into(),
                skel: Arc::new(PreparedSkeleton::default()),
                nodes: vec![],
                base_scale: 1.0,
            },
            Locomotion::new(Arc::new(ClipSet::default()), 0.0),
        ));
        let mut script = ScriptState::over_plugins(&[]);
        script.rt.as_mut().unwrap().game.anim_folders.active.insert((npc, false), vec!["AnimGroupOverride\\M\\_male\\2haattack3.kf".into()]);
        app.insert_resource(script);
        app.init_resource::<ActorAnimFiles>();
        app.add_systems(Update, apply);
        app.update();
        assert_eq!(app.world().resource::<ActorAnimFiles>().0.get(&npc.0).map(Vec::len), Some(1));
        app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.anim_folders.active.clear();
        app.update();
        app.update();
        assert!(app.world().resource::<ActorAnimFiles>().0.is_empty(), "no replacements left: nothing published");
    }

    #[test]
    fn refresh_anim_data_forces_npc_locomotion_stand_rebuild_one_shot() {
        let (mut app, _files) = crate::outfits::tests::asset_fixture();
        let npc = FormId(0x2000);
        let shared = Arc::new(ClipSet::default());
        let skel = Arc::new(PreparedSkeleton::default());
        let actor = app
            .world_mut()
            .spawn((
                ActorRig {
                    reference: npc.0,
                    base: crate::outfits::tests::OTHER.0,
                    body: crate::outfits::tests::OTHER.0,
                    skeleton: "skeleton.nif".into(),
                    skel,
                    nodes: vec![],
                    base_scale: 1.0,
                },
                Locomotion::new(shared.clone(), 0.0),
            ))
            .id();

        let mut script = ScriptState::over_plugins(&[]);
        // Give the NPC an active folder override so `anim_overrides` returns a non-empty list.
        script
            .rt
            .as_mut()
            .unwrap()
            .game
            .anim_folders
            .active
            .insert((npc, false), vec!["mtidle.kf".into()]);
        app.insert_resource(script);
        app.add_systems(Update, apply);

        // Frame 1: initial override builds a fresh ClipSet Arc.
        app.update();
        let after1 = app.world().get::<Locomotion>(actor).unwrap().stand_set();
        assert!(!Arc::ptr_eq(&shared, &after1), "initial override replaces shared ClipSet");

        // Frame 2: unchanged paths keep the exact same Arc.
        app.update();
        let after2 = app.world().get::<Locomotion>(actor).unwrap().stand_set();
        assert!(Arc::ptr_eq(&after1, &after2), "steady state does not rebuild ClipSet");

        // Trigger RefreshAnimData on the NPC without changing the path list.
        assert_eq!(
            app.world_mut()
                .resource_mut::<ScriptState>()
                .rt
                .as_mut()
                .unwrap()
                .game
                .run_query(
                    "Begin Function {}\nSetFunctionValue (RefreshAnimData)\nEnd",
                    RunCtx { self_ref: Some(npc), ..Default::default() },
                ),
            Ok(XVal::Num(0.0))
        );
        assert!(app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.anim_refresh_pending(npc));

        // Frame 3: forced refresh rebuilds the ClipSet Arc and consumes the one-shot flag.
        app.update();
        let after3 = app.world().get::<Locomotion>(actor).unwrap().stand_set();
        assert!(!Arc::ptr_eq(&after2, &after3), "RefreshAnimData forces ClipSet rebuild even with identical paths");
        assert!(!app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.anim_refresh_pending(npc));

        // Frame 4: strictly one-shot.
        app.update();
        let after4 = app.world().get::<Locomotion>(actor).unwrap().stand_set();
        assert!(Arc::ptr_eq(&after3, &after4), "no spurious rebuild on following frame");
    }
}
