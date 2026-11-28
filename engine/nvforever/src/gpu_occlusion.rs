//! Bevy's two-phase GPU occlusion culling on the main camera: the Graphics row Occlusion
//! culling (off by default; graphics pass 2026-11-23, row #3783).
//!
//! **The run's state is fixed at the start** ([`wanted`]): the test aid
//! `NVF_OCCLUSION_CULLING` (`1` on, `0` off) when set, else the row's saved value in
//! `NVForever.ini`, else off. The row keeps the next start's choice ([`OcclusionCullingRow`]),
//! as Authored materials does: the camera's culling and its depth pyramid are set up once,
//! so a change takes effect at the next start.
//!
//! **Where it pays** [observed 2026-11-24, docs/design/frame-rate-audit.md §13]: it skips
//! meshes hidden behind nearer ones before they are drawn (Novac's GPU time 4.96 → 4.31 ms
//! at 4K), but its second culling phase costs render-thread time (Novac's frame 6.96 → 7.18
//! ms on a 9950X3D + RTX 5090, a machine the CPU holds). So it helps where the GPU limits
//! the frame rate (high resolutions, slower GPUs, dense towns) and costs a little where the
//! CPU does.
//!
//! The camera already has a depth prepass, which is all Bevy asks for. What it needs here:
//! first-person shapes (the arms, the held weapon, the seen body) write a depth squeezed
//! into [63/64, 1] that their bounding boxes do not describe (docs/design/first-person-depth.md),
//! so one whose box lies behind a wall would be culled though it is drawn in front of it.
//! Those shapes keep `NoFrustumCulling`, which the vendored mesh_preprocess.wesl also reads
//! as "never occlusion culled" (engine/vendor/README.md).

use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::prelude::*;
use std::sync::OnceLock;

/// The settings key of the Occlusion culling row.
pub const KEY: &str = "occlusion-culling";

/// The row's label.
pub const LABEL: &str = "Occlusion culling";

/// The row's note (at most 200 characters, settings-page.md §0 rule 7).
pub const NOTE: &str = "On: hidden objects are skipped before drawing. Helps when the GPU limits the frame rate; costs some CPU. Takes effect at the next start.";

/// The test aid that overrides the saved row for a run (`1` on, `0` off).
pub const ENV: &str = "NVF_OCCLUSION_CULLING";

/// The run's state (module notes), decided once.
static RUN: OnceLock<bool> = OnceLock::new();

/// Whether this run culls by occlusion: the test aid when set, else the saved row (only for
/// a `--play` run, `read_ini`: without one there is no settings page and the INI is not
/// read, as for Authored materials), else off. Decided at the first call.
pub fn wanted(read_ini: bool) -> bool {
    *RUN.get_or_init(|| {
        let stored = if read_ini { crate::settings::stored_at_start(KEY).and_then(|v| nvf_play::settings::parse_switch(&v)) } else { None };
        decide(std::env::var(ENV).ok().as_deref(), stored)
    })
}

/// [`wanted`]'s rule: the test aid's `1`/`0` first, then the saved row, then off.
fn decide(aid: Option<&str>, stored: Option<bool>) -> bool {
    match aid {
        Some("1") => true,
        Some("0") => false,
        _ => stored.unwrap_or(false),
    }
}

/// The row's value: the next start's choice (module notes).
#[derive(Resource, Default)]
pub struct OcclusionCullingRow(pub bool);

/// Registers the Occlusion culling row (Graphics; nvf-settings-layout places it). Its value
/// before the INI is read is the default (off, or the test aid's): what Defaults puts back
/// (nvf_play::settings `StartStates`). The INI's saved value replaces it at PostStartup.
pub fn add_row(app: &mut App, group: &'static str) {
    app.insert_resource(OcclusionCullingRow(decide(std::env::var(ENV).ok().as_deref(), None)));
    nvf_play::settings::add_setting(
        app,
        nvf_play::settings::Setting {
            key: KEY,
            group,
            label: LABEL,
            note: NOTE,
            switches: "",
            get: |w| w.resource::<OcclusionCullingRow>().0,
            set: |w, on| w.resource_mut::<OcclusionCullingRow>().0 = on,
            slider: None,
        },
    );
}

/// The plugin; `read_ini`: a `--play` run, whose saved row counts ([`wanted`]).
pub struct GpuOcclusionPlugin {
    pub read_ini: bool,
}

impl Plugin for GpuOcclusionPlugin {
    fn build(&self, app: &mut App) {
        let on = wanted(self.read_ini);
        app.add_systems(Startup, move || {
            info!("occlusion culling: {}", if on { "on for the main camera" } else { "off" })
        });
        if on {
            app.add_systems(PostUpdate, (add_to_main_camera, keep_first_person_unculled));
        }
    }
}

/// The main camera (the fly camera, which has the depth prepass) without the culling yet.
type UnculledCamera = (With<crate::fly::FlyCam>, With<Camera3d>, Without<bevy::render::occlusion_culling::OcclusionCulling>);

/// Meshes that can still be frustum (and so occlusion) culled.
type CulledMesh = (With<Mesh3d>, Without<NoFrustumCulling>);

/// The main camera gets the culling once.
fn add_to_main_camera(mut commands: Commands, cams: Query<Entity, UnculledCamera>) {
    for e in &cams {
        commands.entity(e).insert(bevy::render::occlusion_culling::OcclusionCulling);
    }
}

/// Every mesh on the first-person layers keeps `NoFrustumCulling` (re-added when the
/// warm-pose code takes its own off, player_body.rs `WarmUnculled`).
fn keep_first_person_unculled(mut commands: Commands, meshes: Query<(Entity, &RenderLayers), CulledMesh>) {
    let first_person = RenderLayers::from_layers(&[crate::sky::ARMS_LAYER, crate::sky::BODY_LAYER]);
    for (e, layers) in &meshes {
        if layers.intersects(&first_person) {
            commands.entity(e).try_insert(NoFrustumCulling);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_aid_wins_then_the_saved_row_then_off() {
        assert!(decide(Some("1"), Some(false)));
        assert!(!decide(Some("0"), Some(true)));
        assert!(decide(None, Some(true)));
        assert!(!decide(None, Some(false)));
        assert!(!decide(None, None), "off by default");
        assert!(decide(Some("yes"), Some(true)), "an aid value other than 1 or 0 leaves the row's");
    }

    #[test]
    fn the_note_fits_and_says_when_it_applies() {
        assert!(NOTE.chars().count() <= 200, "{} characters", NOTE.chars().count());
        assert!(NOTE.ends_with("Takes effect at the next start."));
    }
}
