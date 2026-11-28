//! Numeric-only loading-transition evidence; no asset names, identifiers or game data.
//! Stream counters are cumulative; live skin components do not prove visible GPU output.
use bevy::{mesh::skinning::SkinnedMesh, prelude::*};
use nvf_play::{loading::Loading, PlayState};

const MAX_REPORTS: u8 = 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Loading,
    World,
}
impl Phase {
    fn word(self) -> &'static str {
        match self {
            Self::Loading => "LOADING",
            Self::World => "WORLD",
        }
    }
}
#[derive(Resource, Default)]
struct Reports {
    last: Option<Phase>,
    emitted: u8,
    #[cfg(test)]
    lines: Vec<String>,
}
pub(crate) struct AndroidRenderDiagnosticsPlugin;
impl Plugin for AndroidRenderDiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Reports>().add_systems(Last, report);
    }
}
fn phase(play: PlayState, loading: Option<&Loading>) -> Option<Phase> {
    if play == PlayState::Loading || loading.is_some_and(|l| l.active() || l.unseen) {
        Some(Phase::Loading)
    } else if play == PlayState::Playing {
        Some(Phase::World)
    } else {
        None
    }
}
fn line(
    phase: Phase,
    transition: u8,
    no_model: usize,
    error_marker: usize,
    skins: usize,
) -> String {
    format!("render: model_counts_v1 phase={} transition={} refs_no_model={} refs_error_marker={} skinned_meshes={}", phase.word(), transition, no_model, error_marker, skins)
}
fn report(
    play: Option<Res<State<PlayState>>>,
    loading: Option<Res<Loading>>,
    streamer: Option<Res<crate::world::Streamer>>,
    skins: Query<(), With<SkinnedMesh>>,
    mut reports: ResMut<Reports>,
) {
    let (Some(play), Some(streamer)) = (play, streamer) else {
        return;
    };
    let Some(phase) = phase(*play.get(), loading.as_deref()) else {
        return;
    };
    if reports.emitted >= MAX_REPORTS || reports.last == Some(phase) {
        return;
    }
    reports.last = Some(phase);
    reports.emitted += 1;
    let message = line(
        phase,
        reports.emitted,
        streamer.stats.refs_no_model,
        streamer.stats.refs_error_marker,
        skins.iter().count(),
    );
    info!("{message}");
    #[cfg(test)]
    reports.lines.push(message);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn streamer() -> crate::world::Streamer {
        crate::world::Streamer {
            radius: 0,
            loaded: default(),
            fades: default(),
            fade_secs: 0.,
            settled: false,
            arrival: None,
            per_frame: 1,
            spawning: default(),
            dying: default(),
            ahead: default(),
            hide: default(),
            interior: None,
            light_scale: 1.,
            managed: default(),
            manage_actors: false,
            stats: crate::world::StreamStats {
                refs_no_model: 3,
                refs_error_marker: 2,
                ..default()
            },
            buffer: default(),
            centre: None,
            waiting: default(),
            model_overrides: default(),
            coop_areas: Vec::new(),
            travels: 0,
        }
    }
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::state::app::StatesPlugin,
            AndroidRenderDiagnosticsPlugin,
        ));
        app.init_state::<PlayState>().init_resource::<Loading>();
        app
    }
    #[test]
    fn actual_transition_reads_stream_counters_and_live_skin_components_without_frame_spam() {
        let mut app = app();
        app.insert_resource(streamer());
        app.world_mut().resource_mut::<Loading>().unseen = true;
        let skin = app
            .world_mut()
            .spawn(SkinnedMesh {
                inverse_bindposes: default(),
                joints: vec![],
            })
            .id();
        app.world_mut().spawn(SkinnedMesh {
            inverse_bindposes: default(),
            joints: vec![],
        });
        app.world_mut().spawn(Mesh3d::default()); // Static marker-like mesh is not a skin.
        app.update();
        assert_eq!(app.world().resource::<Reports>().lines, ["render: model_counts_v1 phase=LOADING transition=1 refs_no_model=3 refs_error_marker=2 skinned_meshes=2"]);
        app.world_mut()
            .resource_mut::<crate::world::Streamer>()
            .stats
            .refs_no_model = 4;
        for _ in 0..30 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Reports>().lines.len(),
            1,
            "count changes are not per-frame log triggers"
        );
        app.world_mut().despawn(skin);
        app.world_mut().resource_mut::<Loading>().unseen = false;
        app.update();
        assert_eq!(app.world().resource::<Reports>().lines[1], "render: model_counts_v1 phase=WORLD transition=2 refs_no_model=4 refs_error_marker=2 skinned_meshes=1");
        app.world_mut()
            .resource_mut::<NextState<PlayState>>()
            .set(PlayState::Menu);
        app.update();
        app.world_mut()
            .resource_mut::<NextState<PlayState>>()
            .set(PlayState::Playing);
        app.update();
        assert_eq!(
            app.world().resource::<Reports>().lines.len(),
            2,
            "ordinary pause/resume is not another loading report"
        );
    }
    #[test]
    fn absent_streamer_does_not_invent_counts_and_process_budget_is_bounded() {
        let mut app = app();
        app.update();
        assert!(app.world().resource::<Reports>().lines.is_empty());
        app.insert_resource(streamer());
        for n in 0..80 {
            app.world_mut().resource_mut::<Loading>().unseen = n % 2 == 0;
            app.update();
        }
        let reports = app.world().resource::<Reports>();
        assert_eq!(reports.emitted, MAX_REPORTS);
        assert_eq!(reports.lines.len(), usize::from(MAX_REPORTS));
    }
}
