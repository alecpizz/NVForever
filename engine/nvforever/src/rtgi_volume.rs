//! The bounce light of a probe volume on an interior's NIF surfaces (#3009,
//! docs/design/rtgi-volume.md) [design], an engineering choice, not the original game's, and a
//! home-grown one. What is here is the shader that reads a volume and what puts one on the GPU
//! for it. **Nothing in the game asks for one**: there is no setting, only the developer aid
//! `NVF_PLAY_DEBUG_RTGI_MADE=1` ([`made`]). Without a request no view has the bindings, no
//! pipeline reads them, and nothing is made, written, bound or dispatched.
//!
//! The parts, built the way the ambient occlusion's are (ssao_lighting.rs): bevy_pbr's view
//! component `NvfRtgiVolumeBindings` (vendored, engine/vendor/README.md), the NIF surfaces that
//! read it (nvf_assets nif_material.rs `reads_rtgi_volume`), rtgi_volume.wesl, and the text hook
//! that lights.rs patches into `pbr_functions` ([`hook`]).
//!
//! The way of a volume: the main world's [`RtgiRequest`], for one [`Place`]; what each extraction
//! makes of it ([`plan`], a pure function), asked of the device first ([`admit`]) and carried out
//! in [`gpu`], the only code that makes one of its GPU objects or changes the component.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::pbr::{NVF_RTGI_VIEW_BIND_GROUPS, NVF_VIEW_BIND_GROUPS};
use bevy::prelude::*;
use bevy::render::render_resource::ShaderType;
use bevy::render::RenderApp;
use bevy::shader::Shader;
use nvf_rtgi::SolvedVolume;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

mod admit;
mod gpu;
pub mod hook;
mod made;
mod plan;
pub(crate) use hook::hook_patched;

/// rtgi_volume.wesl, imported by the patched `apply_pbr_lighting` and by nvf_assets
/// nif_material.wesl as `nvforever::rtgi_volume`.
const SHADER: Handle<Shader> = uuid_handle!("3c0a7e14-91d2-4d86-b2f5-07c6ea51d9b4");

/// Whether lights.rs has put the hook into `pbr_functions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HookState {
    /// Bevy's `pbr_functions` has not loaded yet.
    Pending,
    Installed,
    /// The source was not as expected: no view may get a volume.
    Failed,
}

static HOOK_STATE: AtomicU8 = AtomicU8::new(HookState::Pending as u8);

/// Record what became of the hook (lights.rs `patch_display_sum`).
pub(crate) fn set_hook_state(state: HookState) {
    HOOK_STATE.store(state as u8, Ordering::Relaxed);
}

/// What became of the hook; a volume may be bound only once it is in place.
pub(crate) fn hook_state() -> HookState {
    match HOOK_STATE.load(Ordering::Relaxed) {
        0 => HookState::Pending,
        1 => HookState::Installed,
        _ => HookState::Failed,
    }
}

/// A visit of an interior: what a volume is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Place {
    cell: u32,
    /// `Streamer::travels`: a travel into the same cell is another visit.
    travels: u64,
    /// The running game's instance, another after a load; none before the scripts run.
    game: Option<u64>,
}

/// Where the player is; none outdoors.
fn place(streamer: Option<&crate::world::Streamer>, script: Option<&nvf_play::script::ScriptState>) -> Option<Place> {
    let streamer = streamer?;
    let game = script.and_then(|s| s.rt.as_ref()).map(|rt| rt.game.image_space_sync_token().0);
    Some(Place { cell: streamer.interior?, travels: streamer.travels, game })
}

/// The light's constants, which no volume has in it (rtgi-volume.md "The light").
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fit {
    b: f32,
    g: f32,
    pay: f32,
    least: f32,
    pull: f32,
}

/// A solved volume, as it is to be shown.
#[derive(Clone)]
struct Solved {
    volume: Arc<SolvedVolume>,
    fit: Fit,
    /// 0 the stock light, 1 all of the new one.
    share: f32,
}

/// Main world: the volume the player's view is to have. Not there by default.
#[derive(Resource, Clone)]
struct RtgiRequest {
    place: Place,
    /// Counts the results of a place: an older one than the one shown is not shown.
    revision: u64,
    /// None: the empty texture, with the stock light's uniform.
    solved: Option<Solved>,
}

/// The sides of the empty texture: one point and its shell.
const EMPTY: [u32; 3] = [3, 3, 12];

impl RtgiRequest {
    /// The stock light through the bindings.
    fn empty(place: Place, revision: u64) -> Self {
        RtgiRequest { place, revision, solved: None }
    }

    /// Of a volume that is what it says (`nvf_rtgi::Reader`).
    fn solved(place: Place, revision: u64, volume: Arc<SolvedVolume>, fit: Fit, share: f32) -> Result<Self, String> {
        nvf_rtgi::Reader::new(&volume)?;
        Ok(RtgiRequest { place, revision, solved: Some(Solved { volume, fit, share }) })
    }

    fn key(&self) -> plan::Key {
        plan::Key { place: self.place, revision: self.revision, solved: self.solved.is_some() }
    }

    /// The texture's sides and bytes.
    fn atlas(&self) -> ([u32; 3], &[u8]) {
        match &self.solved {
            Some(s) => (s.volume.atlas, &s.volume.rgba16f_le),
            None => (EMPTY, &[0; 8 * (EMPTY[0] * EMPTY[1] * EMPTY[2]) as usize]),
        }
    }

    fn uniform(&self) -> RtgiVolumeUniform {
        self.solved.as_ref().map_or_else(RtgiVolumeUniform::empty, |s| RtgiVolumeUniform::new(&s.volume, s.fit, s.share))
    }
}

/// `NvfRtgiVolume` of rtgi_volume.wesl, field for field.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
struct RtgiVolumeUniform {
    origin: Vec4,
    dimensions: Vec4,
    atlas: Vec4,
    fit: Vec4,
    light: Vec4,
}

impl RtgiVolumeUniform {
    fn new(v: &SolvedVolume, fit: Fit, share: f32) -> Self {
        RtgiVolumeUniform {
            origin: Vec3::from(v.origin).extend(1.0 / v.spacing),
            dimensions: UVec3::from(v.dimensions).as_vec3().extend(v.offset),
            atlas: UVec3::from(v.atlas).as_vec3().recip().extend((v.dimensions[2] + 2) as f32),
            fit: Vec4::new(fit.pay, fit.least, nvf_rtgi::THIN, fit.pull),
            light: Vec4::new(fit.b, fit.g, share, 0.0),
        }
    }

    /// The stock light's, whatever is read: b 1, g 0, pull 0, share 0.
    fn empty() -> Self {
        RtgiVolumeUniform {
            origin: Vec3::ZERO.extend(1.0),
            dimensions: Vec3::ONE.extend(0.0),
            atlas: UVec3::from(EMPTY).as_vec3().recip().extend(3.0),
            fit: Vec4::new(0.0, 0.0, nvf_rtgi::THIN, 0.0),
            light: Vec4::new(1.0, 0.0, 0.0, 0.0),
        }
    }
}

/// What was done on the GPU in this run; a clone lives in each world.
#[derive(Resource, Clone, Default)]
struct RtgiCounts(Arc<[AtomicU64; 4]>);

/// What [`RtgiCounts`] counts.
#[derive(Clone, Copy)]
enum Did {
    /// A texture, buffer or sampler made.
    Allocation,
    /// The texture's or the uniform's bytes sent.
    Upload,
    /// A view given the component, or another texture in it.
    Bind,
    /// The readback's compute pass ([`made`]).
    Dispatch,
}

impl RtgiCounts {
    fn add(&self, what: Did, n: u64) {
        self.0[what as usize].fetch_add(n, Ordering::Relaxed);
    }

    fn all(&self) -> [u64; 4] {
        std::array::from_fn(|i| self.0[i].load(Ordering::Relaxed))
    }
}

pub struct RtgiVolumePlugin;

impl Plugin for RtgiVolumePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "rtgi_volume.wesl", |s, _| Shader::from_wesl(s, "embedded://nvforever/rtgi_volume.wesl"));
        let counts = RtgiCounts::default();
        if crate::dev_aids::ON {
            app.insert_resource(counts.clone()).add_systems(Last, report);
        }
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.insert_resource(counts);
        gpu::add(render_app);
        if made::wanted() {
            app.add_plugins(made::MadePlugin);
        }
    }
}

/// The run's last line of it (with the developer aids): the bind groups as bevy_pbr made them.
fn report(mut exit: MessageReader<AppExit>, counts: Res<RtgiCounts>) {
    if exit.read().last().is_none() {
        return;
    }
    let [allocations, uploads, binds, dispatches] = counts.all();
    info!(
        "rtgi: {allocations} allocations, {uploads} uploads, {binds} binds, {dispatches} dispatches; view bind groups {}, with RTGI entries {}",
        NVF_VIEW_BIND_GROUPS.load(Ordering::Relaxed),
        NVF_RTGI_VIEW_BIND_GROUPS.load(Ordering::Relaxed)
    );
}

#[cfg(test)]
mod made_tests;
#[cfg(test)]
mod owner_tests;
#[cfg(test)]
mod tests;
