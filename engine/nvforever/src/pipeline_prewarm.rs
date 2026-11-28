//! Under a start's or a travel's loading screen, every mesh of the loaded cells is drawn
//! whatever the view's frustum, so the pipelines of what is behind the camera are compiled
//! before the screen comes down instead of while the player turns and walks.
//!
//! Bevy specializes a mesh's material pipelines only for the views that see it [documented:
//! bevy_pbr 0.20.1 `material.rs` `specialize_material_meshes`, over each view's visible
//! entities; the shadows' `specialize_shadows` over each light view's], and a draw whose
//! pipeline is not ready is skipped (`pipeline_gate.rs`). So on the first walk after a screen
//! the meshes coming into view asked for theirs: 45 pipelines in the first seconds at Novac
//! 22:00 (the material, prepass, blended and sun shadow variants), each drawn only once it
//! was ready [observed 2026-11-19, NVF_PLAY_TEST_PIPELINE_LOG, before this module].
//!
//! While the screen is up each mesh without `NoFrustumCulling` gets it, with [`Prewarmed`]
//! to say so, and loses both once the screen only settles before coming down (everything in,
//! its pipelines ready: `Loading::settling`), before that frame's culling, so the first
//! frame seen is culled. The camera's own settings make
//! the view's key, so the variants asked for are those its later frames use; the sun's
//! cascades see the meshes too, for the shadow variants. Meshes hidden otherwise (Visibility,
//! a visibility range: the draw distance, LOD) stay hidden, so what lies beyond them is not
//! warmed. The screen waits for these pipelines like the others (`world::loading_gate`).
//! `NVF_PIPELINE_PREWARM=0` turns it off for comparing.
//!
//! What it gave at Novac [observed 2026-11-19, the same walk, NVF_PLAY_TEST_PIPELINE_DEFS=all]:
//! about 4,800 meshes drawn unculled, 6 more pipelines asked under the screen (152 to 158)
//! and 43 instead of 45 after it. Those after it differ from the screen's in what the
//! frustum does not decide: meshes coming within their visibility range as the player walks
//! (the draw distance, the fade-in's dither, `VISIBILITY_RANGE_DITHER`), actors streamed in
//! (`SKINNED`), and those meshes' material flags (`MAY_DISCARD`, `VERTEX_COLORS`). Widening
//! the visibility ranges under the screen would reach them, but the game's own fade-in and
//! draw distance drive those ranges (fade.rs, draw_distance.rs), so it is not done here.
//!
//! An entity given `NoFrustumCulling` by other code while marked would lose it at the end;
//! none is (the game's own users add it when they spawn: particles, effects, the player's
//! body, which this leaves alone).

use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;
use nvf_play::loading::Loading;

/// A mesh drawn unculled under the loading screen by this module.
#[derive(Component)]
pub struct Prewarmed;

pub struct PipelinePrewarmPlugin;

impl Plugin for PipelinePrewarmPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("NVF_PIPELINE_PREWARM").is_ok_and(|v| v == "0") {
            info!("pipeline prewarm: off (NVF_PIPELINE_PREWARM=0)");
            return;
        }
        // Before this frame's culling, so the frame the screen comes down on is culled
        // already (the marks' removal is applied at the sync point Bevy puts before it).
        app.init_resource::<PrewarmState>().add_systems(
            PostUpdate,
            screen_up
                .pipe(prewarm)
                .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        );
    }
}

#[derive(Resource, Default)]
struct PrewarmState {
    /// Meshes marked under the screen now up.
    marked: usize,
    /// The screen now up settled once and its marks came off: none again until it is down.
    released: bool,
}

/// The screen as prewarming sees it: whether one that warms is up (a start's or a
/// travel's, not a cover's: a save loading in a new run, the main menu), and whether it
/// only settles before coming down (`Loading::settling`: everything is in and its
/// pipelines are ready).
fn screen_up(loading: Option<Res<Loading>>) -> (bool, bool) {
    loading.map_or((false, false), |l| (l.starting() || l.travelling(), l.settling()))
}

/// Marks the meshes while a warming screen is up; takes the marks off once it settles,
/// so the frame it comes down on is culled already, and for good: removing them can make
/// the screen wait a frame more (it starts settling again), and marking again then would
/// draw everything unculled once more [observed 2026-11-20: without this, the marks came
/// off and went back on one frame apart at the `GSDocMitchellHouse` start of smoke's
/// pipeline-cache scenarios, two "pipeline prewarm" lines 12 ms apart].
fn prewarm(
    In((screen, settling)): In<(bool, bool)>,
    mut commands: Commands,
    mut state: ResMut<PrewarmState>,
    unmarked: Query<Entity, (With<Mesh3d>, Without<NoFrustumCulling>)>,
    marked: Query<Entity, With<Prewarmed>>,
    gate: Option<Res<crate::pipeline_gate::PipelineGate>>,
) {
    if !screen {
        state.released = false;
    }
    if screen && settling {
        state.released = true;
    }
    if screen && !state.released {
        for e in &unmarked {
            // A solid core (solid_cores.rs) may go before this is applied.
            commands.entity(e).try_insert((NoFrustumCulling, Prewarmed));
            state.marked += 1;
        }
    } else if state.marked > 0 {
        for e in &marked {
            commands.entity(e).try_remove::<(NoFrustumCulling, Prewarmed)>();
        }
        info!(
            "pipeline prewarm: {} meshes drawn unculled under the screen; {} pipelines asked by then",
            state.marked,
            gate.map_or(0, |g| g.asked())
        );
        state.marked = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_meshes_while_the_screen_is_up_and_clears_them_after() {
        let mut app = test_app();
        let mesh = app.world_mut().spawn(Mesh3d::default()).id();
        let own = app.world_mut().spawn((Mesh3d::default(), NoFrustumCulling)).id();
        let not_a_mesh = app.world_mut().spawn(Transform::default()).id();
        app.update();
        assert!(!app.world().entity(mesh).contains::<Prewarmed>(), "no loading screen: nothing");

        app.insert_resource(Screen(true, false));
        app.update();
        assert!(app.world().entity(mesh).contains::<NoFrustumCulling>());
        assert!(app.world().entity(mesh).contains::<Prewarmed>());
        assert!(!app.world().entity(own).contains::<Prewarmed>(), "its own NoFrustumCulling: left alone");
        assert!(!app.world().entity(not_a_mesh).contains::<NoFrustumCulling>());

        app.insert_resource(Screen(false, false));
        app.update();
        assert!(!app.world().entity(mesh).contains::<NoFrustumCulling>(), "the screen is down: culled again");
        assert!(!app.world().entity(mesh).contains::<Prewarmed>());
        assert!(app.world().entity(own).contains::<NoFrustumCulling>(), "another's NoFrustumCulling stays");
    }

    #[derive(Resource)]
    struct Screen(bool, bool);

    fn test_app() -> App {
        let mut app = App::new();
        app.init_resource::<PrewarmState>()
            .insert_resource(Screen(false, false))
            .add_systems(Update, (|s: Res<Screen>| (s.0, s.1)).pipe(prewarm));
        app
    }

    #[test]
    fn the_marks_come_off_once_the_screen_settles_and_stay_off_until_the_next() {
        let mut app = test_app();
        let mesh = app.world_mut().spawn(Mesh3d::default()).id();
        app.insert_resource(Screen(true, false));
        app.update();
        assert!(app.world().entity(mesh).contains::<Prewarmed>());

        app.insert_resource(Screen(true, true));
        app.update();
        assert!(!app.world().entity(mesh).contains::<NoFrustumCulling>(), "settling: culled before the screen drops");

        app.insert_resource(Screen(true, false));
        app.update();
        assert!(
            !app.world().entity(mesh).contains::<Prewarmed>(),
            "waiting again after it settled once: not marked again"
        );

        app.insert_resource(Screen(false, false));
        app.update();
        app.insert_resource(Screen(true, false));
        app.update();
        assert!(app.world().entity(mesh).contains::<Prewarmed>(), "the next screen warms again");
    }
}
