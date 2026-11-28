//! Sun visibility (#2670 cloud shadows, #2671 terrain shadows; docs/design/sun-visibility.md):
//! one factor V in [0, 1] on the outdoor sun's direct light, through which other modules
//! darken it. V = K * Vsolid: K the clouds' transmittance, Vsolid the solid occluders'
//! visibility, min(C, T) of the cascade shadow C and the terrain's T, the darker of the two
//! at every distance (sun_visibility.wesl).
//!
//! Producers write [`SunVisibility`] (cloud shadows: `cloud_enabled` and `cloud`; terrain
//! shadows: `terrain_enabled` and `terrain`). Two things are kept apart there: whether a
//! feature is switched on (`cloud_enabled`, `terrain_enabled`, which alone decide the views'
//! markers, the layout key bit, the pipeline key bits and the shader defs) and whether its
//! producer has a valid image to show (`cloud`, `terrain`, which only fill the uniform and
//! the texture). While Cloud shadows is on and no image is published (indoors, a texture
//! still being read, a weather without cover, a worldspace just changed) the cloud input is
//! neutral (strength 0 and the white fallback texture: K = 1); while Terrain shadows is on
//! and no map is published (indoors, no heightfield yet, the light at the horizon, the
//! first map still computing) the terrain input is neutral (mask bit clear: T = 1 and
//! Vsolid = C, and a stand-in texture that casts nothing). So the pipelines stay as they
//! are: only the switches change them.
//!
//! While nothing is switched on (and no debug view asked for) the plumbing is off: no view
//! has [`NvfSunVisibilityBindings`], so the mesh view layout and the shader defs are
//! upstream's, no GPU buffer, sampler or image is made, and the systems here stop at their
//! run conditions. What the off path still costs is in docs/design/sun-visibility.md §3.
//!
//! When on:
//! - the sun light ([`crate::sky::Sun`]) outdoors gets [`OutdoorSunLight`], extracted as
//!   bevy_pbr's `NvfOutdoorSun`, which sets `DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT`
//!   in its GPU flags: V touches that light only, never the first-person arms' shade light
//!   (player_body.rs), a menu's lights or an interior's directional light (the same `Sun`
//!   entity indoors, unmarked there). The light in the air (#3032 stage 3,
//!   `medium_air_demand::AirWanted`) asks for this marker alone, to find that light in the
//!   render world: with the surface switches off it gets the marker and nothing else here
//!   (no view bindings, no shader defs, no GPU parts);
//! - the world camera, the water reflection camera and a scope's lens camera get
//!   [`SunVisibilityView`], extracted as bevy_pbr's `NvfSunVisibilityBindings` (a uniform,
//!   the cloud texture (filterable), the terrain map (unfilterable, read with loads), a
//!   sampler at view bindings 39-42, and since #3032 stage 2i M2, while the Volumetric clouds row
//!   is on ([`volume_layout`]), the volumetric clouds' light volume's near and far front sets at
//!   45-46, read through the shared reader light_volume_reader.wesl for the cloud shadows from the
//!   volume, [`CloudSource`]), which gives their
//!   pipelines the shader defs `NVF_SUN_VISIBILITY` (+ `NVF_CLOUD_VISIBILITY`,
//!   `NVF_TERRAIN_VISIBILITY`) and their bind group the bindings (engine/vendor/README.md);
//! - [`HOOK_EDITS`], patched into `apply_pbr_lighting` after lights.rs's display-value
//!   edits, put V in the directional light loop in the place of the sun's shadow, outside
//!   the shadow-receiver condition, so `direct_light`, the display-value `nvf_direct` and
//!   the transmitted lobe all take it, receivers or not (leaves, tree cards, water).
//!
//! The player's switches are cloud_shadows.rs's (the settings row Cloud shadows) and
//! terrain_shadows.rs's (the row Terrain shadows). Test aids:
//! `NVF_PLAY_DEBUG_SUN_VISIBILITY=cloud|terrain|combined`
//! draws K, T or V alone in place of the lit colour (T is 1 everywhere unless Terrain
//! shadows is on; before ambient and fog; the image
//! space grade still follows, `--no-grade` leaves it out), and
//! `NVF_PLAY_DEBUG_SUN_VISIBILITY_TEST=checker` fills `cloud` with a generated
//! checkerboard ([`checker_image`]).

use bevy::asset::{load_internal_asset, uuid_handle, RenderAssetUsages};
use bevy::pbr::{MeshPipelineKey, MeshPipelineViewLayoutKey, MeshPipelineViewLayouts, NvfOutdoorSun, NvfSunVisibilityBindings};
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    AsBindGroup, BindGroupLayoutEntry, BindingType, BufferBindingType, ShaderStages,
    AddressMode, Extent3d, FilterMode, Sampler, SamplerDescriptor, ShaderType, TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor, TextureViewDimension, UniformBuffer,
};

use crate::volumetric_clouds_lightvol::LightVolumeWindow;
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::sync_world::RenderEntity;
use bevy::render::texture::{FallbackImage, GpuImage};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use bevy::shader::Shader;

use crate::fly::FlyCam;
use crate::reflection::ReflectionCamera;
use crate::scope_view::LensCamera;
use crate::sky::{SkyState, Sun};

/// sun_visibility.wesl, imported by the patched `apply_pbr_lighting` as
/// `nvforever::sun_visibility`.
const SHADER: Handle<Shader> = uuid_handle!("5b0f3c52-8f0e-4d55-9d77-2a61c4b7e3a9");
/// light_volume_reader.wesl, imported by sun_visibility.wesl as `nvforever::light_volume_reader`
/// (#3032 stage 2i M2: the light volume's shared reader).
const READER_SHADER: Handle<Shader> = uuid_handle!("8d4e6a1f-3b72-4c09-a5e8-61f2d93c7b40");

/// What darkens the sun's direct light; producers write it, everything else reads it.
#[derive(Resource, Clone, Default, Debug, PartialEq)]
pub struct SunVisibility {
    /// Cloud shadows (#2670) are switched on: the views take the cloud variant and keep it
    /// whether or not `cloud` is set (module notes).
    pub cloud_enabled: bool,
    /// Cloud shadows (#2670): K, while the producer has a valid image; `None` with
    /// `cloud_enabled` packs a neutral cloud (K = 1).
    pub cloud: Option<CloudVisibility>,
    /// Terrain shadows (#2671) are switched on: the views take the terrain variant and keep
    /// it whether or not `terrain` is set (terrain_shadows.rs `feed_sun_visibility`).
    pub terrain_enabled: bool,
    /// Terrain shadows (#2671): T, while the producer has a published map; `None` with
    /// `terrain_enabled` packs a neutral terrain (T = 1, Vsolid = C).
    pub terrain: Option<TerrainVisibility>,
    /// The debug view (`NVF_PLAY_DEBUG_SUN_VISIBILITY`).
    pub debug: DebugMode,
    /// Where the cloud shadows come from (#3032 stage 2i M2; cloud_shadows.rs
    /// `NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE`): the light volume where it has data, else the
    /// weather's layers (the default); only the layers; only the volume.
    pub cloud_source: CloudSource,
    /// The cloud shadows' strength (cloud_shadows.rs's calibration, the same setting the
    /// weather's layers use), for the shadow from the light volume, which needs it whether or
    /// not a layer image is published.
    pub cloud_strength: f32,
}

/// Where the cloud shadows come from (#3032 stage 2i M2; `NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CloudSource {
    /// The volumetric clouds' light volume where its completed fronts have data, the weather's
    /// layers elsewhere and whenever it has none (the volumetric clouds off, the light volume off,
    /// no front swept yet), mixed over the windows' guard bands and while a level fades in.
    #[default]
    Auto,
    /// The weather's layers only (the shadows before stage 2i M2).
    Weather,
    /// The light volume only: no cloud shadow where it has no data (for paired pictures).
    Volume,
    /// Debug views of the reader on the receivers, in place of K (with
    /// `NVF_PLAY_DEBUG_SUN_VISIBILITY=cloud` they show as grey): the covered share (presence and
    /// guard bands included; 0 no data) ...
    Share,
    /// ... and the optical depth over 8 (0 where no data).
    Depth,
}

impl CloudSource {
    /// `weather`, `volume` or `auto`; `None` for anything else.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Some(Self::Auto),
            "weather" | "layers" => Some(Self::Weather),
            "volume" => Some(Self::Volume),
            "share" => Some(Self::Share),
            "depth" => Some(Self::Depth),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Weather => "weather",
            Self::Volume => "volume",
            Self::Share => "share",
            Self::Depth => "depth",
        }
    }
}

impl SunVisibility {
    /// Whether the plumbing is on: a feature is switched on or set, or a debug view is
    /// asked for (a debug view alone shows V = C, the cascade shadow).
    pub fn active(&self) -> bool {
        self.cloud_on() || self.terrain_on() || self.debug != DebugMode::Off
    }

    /// Whether the views take the cloud variant (`NVF_CLOUD_VISIBILITY`): Cloud shadows is
    /// switched on, or a cloud is set (the test producer sets one without the switch).
    pub fn cloud_on(&self) -> bool {
        self.cloud_enabled || self.cloud.is_some()
    }

    /// Whether the views take the terrain variant (`NVF_TERRAIN_VISIBILITY`): Terrain
    /// shadows is switched on, or a map is set.
    pub fn terrain_on(&self) -> bool {
        self.terrain_enabled || self.terrain.is_some()
    }

    /// The switches the log line names.
    fn mask(&self) -> (bool, bool, DebugMode) {
        (self.cloud_on(), self.terrain_on(), self.debug)
    }
}

/// The cloud transmittance projected onto the ground along the light (Bevy world axes,
/// game units). A receiver at P lit from L (unit, towards the light) looks up the texture
/// where P + t L meets the plane y = `plane_height`, at uv = hit.xz * `uv_scale` +
/// `uv_offset` (the sampler repeats), and K = 1 - `strength` * (1 - texel). A receiver at
/// or above the plane has K = 1; near the horizon the shadow fades out
/// ([`CLOUD_FADE_FROM`], [`CLOUD_FADE_TO`]).
#[derive(Clone, Debug, PartialEq)]
pub struct CloudVisibility {
    /// Transmittance in the red channel (1 clear, 0 opaque): a filterable float format,
    /// R8Unorm meant; the image's own sampler is not used.
    pub image: Handle<Image>,
    pub uv_scale: Vec2,
    pub uv_offset: Vec2,
    pub plane_height: f32,
    /// 0..1.
    pub strength: f32,
}

/// The terrain's shadow-height map (#2671; terrain_shadows.rs produces it): a receiver
/// below the map's height S at its xz is in the terrain's shadow
/// (sun_visibility.wesl `nvf_terrain_visibility`, mirrored by [`terrain_visibility`]).
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainVisibility {
    /// `Rgba32Float`, `side` × `side`, row 0 the southernmost: R the shadow height S, G the
    /// caster's horizontal distance, B the ground's relief (world units). Read with loads
    /// only (binding 41 is unfilterable); the image's own sampler is not used.
    pub image: Handle<Image>,
    /// World xz to map uv (uv = xz * scale + offset); outside 0..1 the map says nothing.
    pub uv_scale: Vec2,
    pub uv_offset: Vec2,
    /// World units per texel.
    pub texel: f32,
    /// Texels per side.
    pub side: u32,
}

/// The terrain receiver's constants (sun_visibility.wesl `NVF_TERRAIN_*`, word for word;
/// docs/design/terrain-shadows.md "T2: the receivers") [engineering choices]: the penumbra's
/// height per unit of caster distance (about the sun's apparent diameter, 0.53°, 0.0093 rad,
/// rounded) ...
pub const TERRAIN_PENUMBRA: f32 = 0.01;
/// ... its least height in texels of the map ...
pub const TERRAIN_SOFT_MIN: f32 = 0.25;
/// ... the texels of S's fall along the light added to it (no stair steps of the grid) ...
pub const TERRAIN_SOFT_TEXELS: f32 = 0.5;
/// ... the most tan(elevation) that term takes ...
pub const TERRAIN_TAN_MAX: f32 = 1.0;
/// ... the receiver's bias: this many times the ground's relief, plus a constant in world
/// units ...
pub const TERRAIN_BIAS_RELIEF: f32 = 2.0;
pub const TERRAIN_BIAS_MIN: f32 = 16.0;
/// ... and the texels over which T fades to 1 at the map's edge.
pub const TERRAIN_EDGE_TEXELS: f32 = 4.0;

/// `smoothstep(0, 1, t)` of an already clamped `t`.
fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// CPU mirror of sun_visibility.wesl `nvf_terrain_lit`: how lit a receiver at height `y`
/// (bias included) is against one texel `s` (R S, G caster distance) with `extra` added to
/// the penumbra.
#[cfg_attr(not(test), allow(dead_code))] // the receivers' formulas, which the tests hold
pub fn terrain_lit(s: Vec4, y: f32, extra: f32, texel: f32) -> f32 {
    let band = (s.y * TERRAIN_PENUMBRA).max(TERRAIN_SOFT_MIN * texel) + extra;
    smooth(((y - s.x) / band + 1.0).clamp(0.0, 1.0))
}

/// CPU mirror of sun_visibility.wesl `nvf_terrain_visibility`, line for line: T at `p` (Bevy
/// world), the light from `l` (unit, towards the light), with the packed uniform `u` and
/// `load` giving the map's texel at (column, row) (only called with indices inside the map).
#[cfg_attr(not(test), allow(dead_code))]
pub fn terrain_visibility(u: &SunVisibilityUniform, load: impl Fn(IVec2) -> Vec4, p: Vec3, l: Vec3) -> f32 {
    if u.mode.x & MASK_TERRAIN == 0 {
        return 1.0;
    }
    let (texel, side) = (u.terrain.z, u.terrain.w);
    let x = (Vec2::new(p.x, p.z) * Vec2::new(u.terrain_uv.x, u.terrain_uv.y) + Vec2::new(u.terrain_uv.z, u.terrain_uv.w)) * side;
    let inside = x.x.min(side - x.x).min(x.y.min(side - x.y));
    let edge = smooth((inside / TERRAIN_EDGE_TEXELS).clamp(0.0, 1.0));
    if edge <= 0.0 {
        return 1.0;
    }
    let c = x - Vec2::splat(0.5);
    let base = c.floor();
    let f = c - base;
    let last = IVec2::splat(side as i32 - 1);
    let i0 = base.as_ivec2().clamp(IVec2::ZERO, last);
    let i1 = (base.as_ivec2() + IVec2::ONE).clamp(IVec2::ZERO, last);
    let (t00, t10, t01, t11) = (load(i0), load(IVec2::new(i1.x, i0.y)), load(IVec2::new(i0.x, i1.y)), load(i1));
    let (w00, w10, w01, w11) = ((1.0 - f.x) * (1.0 - f.y), f.x * (1.0 - f.y), (1.0 - f.x) * f.y, f.x * f.y);
    let relief = w00 * t00.z + w10 * t10.z + w01 * t01.z + w11 * t11.z;
    let y = p.y + TERRAIN_BIAS_MIN + TERRAIN_BIAS_RELIEF * relief;
    let tan_e = (l.y / Vec2::new(l.x, l.z).length().max(1.0e-4)).clamp(0.0, TERRAIN_TAN_MAX);
    let extra = TERRAIN_SOFT_TEXELS * texel * tan_e;
    let lit = w00 * terrain_lit(t00, y, extra, texel) + w10 * terrain_lit(t10, y, extra, texel) + w01 * terrain_lit(t01, y, extra, texel) + w11 * terrain_lit(t11, y, extra, texel);
    1.0 + (lit - 1.0) * edge
}

/// CPU mirror of sun_visibility.wesl `nvf_solid_visibility` (terrain variant): Vsolid from
/// the cascade shadow `cascade`, T `terrain` and the view depth `view_z`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn solid_visibility(u: &SunVisibilityUniform, cascade: f32, terrain: f32) -> f32 {
    if u.mode.x & MASK_TERRAIN == 0 {
        return cascade;
    }
    cascade.min(terrain)
}

/// What the debug view draws in place of the lit colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DebugMode {
    #[default]
    Off = 0,
    /// K.
    Cloud = 1,
    /// T.
    Terrain = 2,
    /// V = K * Vsolid.
    Combined = 3,
}

impl DebugMode {
    /// From `NVF_PLAY_DEBUG_SUN_VISIBILITY`; `None` for a value it does not know.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "0" | "off" | "false" | "no" => Some(Self::Off),
            "cloud" => Some(Self::Cloud),
            "terrain" => Some(Self::Terrain),
            "combined" | "1" | "on" => Some(Self::Combined),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Cloud => "cloud",
            Self::Terrain => "terrain",
            Self::Combined => "combined",
        }
    }
}

/// The sine of the light's elevation below which clouds cast no shadow, and from which
/// their shadow is whole: about 1.1 and 5.7 degrees [engineering choice: the hit point
/// runs off towards the horizon as the light gets low; sky.rs fades the sun light itself
/// out within 0.1 radians of the horizon].
pub const CLOUD_FADE_FROM: f32 = 0.02;
pub const CLOUD_FADE_TO: f32 = 0.1;

/// `NvfSunVisibility` of sun_visibility.wesl (field for field).
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct SunVisibilityUniform {
    pub cloud_uv: Vec4,
    pub cloud: Vec4,
    pub terrain_uv: Vec4,
    pub terrain: Vec4,
    pub mode: UVec4,
    /// The cloud from the light volume: x its strength, y and z the low-sun band (as `cloud.zw`).
    pub volume: Vec4,
    /// The light volume's fronts (light_volume_reader.wesl `LightVolumeWindow`).
    pub light_volume: LightVolumeWindow,
}

/// Feature bits of `mode.x`: a valid image is published (not merely switched on).
pub const MASK_CLOUD: u32 = 1;
pub const MASK_TERRAIN: u32 = 2;
/// The cloud shadow from the light volume where it has data (#3032 stage 2i M2), and with it the
/// volume alone (no weather's layers where it has none: `CloudSource::Volume`).
pub const MASK_CLOUD_VOLUME: u32 = 4;
pub const MASK_CLOUD_VOLUME_ONLY: u32 = 8;
/// Debug views in place of K (`CloudSource::Share`, `CloudSource::Depth`): the reader's covered
/// share, its depth over 8.
pub const MASK_CLOUD_VOLUME_SHARE: u32 = 16;
pub const MASK_CLOUD_VOLUME_DEPTH: u32 = 32;

/// Why the cloud shadows take the weather's layers alone this frame (#3032 stage 2i M2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeatherWhy {
    /// `NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE=weather`.
    Asked,
    /// The Volumetric clouds row is off: the views have the weather-only layout, whatever was asked
    /// ([`volume_layout`]).
    RowOff,
    /// The volumetric sky is not what the player sees: nothing to trace (indoors, under water, a
    /// clear sky), or the medium's buffer not shown (its composing material not prepared, a
    /// hand-back: the flat layers are showing).
    SkyNotShown,
    /// The light volume has no completed front.
    NoFront,
    /// The adapter's fragment stage has too few sampled textures for the light volume's two
    /// arrays: the views have the weather-only layout ([`CloudVolumeRoom`]), whatever was asked.
    NoRoom,
}

/// What the automatic source needs to know besides the reader's levels (#3032 stage 2i M2): `Auto` takes the light volume's shadows only where they belong
/// to the sky the player sees: the volumetric clouds drawn (the row on and the medium's buffer
/// shown, the flat sky hidden).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AutoGate {
    /// The Volumetric clouds row is on (off: the weather's layers whatever the aid asks).
    pub row: bool,
    /// The volumetric sky is shown (medium.rs `MediumOutput::ready`: the row on, a trace this frame,
    /// its buffer written whole with the composing material prepared and the proxy's pipeline ready).
    pub sky_shown: bool,
    /// The clouds are traced this frame.
    pub traced: bool,
}

/// What the cloud shadows read in a frame (#3032 stage 2i M2), for the log and the uniform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CloudSourceInUse {
    /// No cloud variant (Cloud shadows off).
    #[default]
    Off,
    /// The weather's layers alone, and why.
    Weather { why: WeatherWhy },
    /// The light volume where its levels are readable (near, far), the layers (or, with the aid
    /// `volume`, no shadow) elsewhere.
    Volume { near: bool, far: bool, only: bool },
    /// A debug view of the reader (`view` [`CloudSource::Share`] or [`CloudSource::Depth`]) in place
    /// of K.
    Debug { near: bool, far: bool, view: CloudSource },
}

impl CloudSourceInUse {
    /// From the switch, the aid, the levels the shared reader has this frame and, for `Auto`, the
    /// gate: the volume's shadows only while the volumetric sky is shown and traced and a level is readable;
    /// else the weather's layers. The aids `volume`, `share` and `depth` take every readable level whether
    /// the sky is shown or not (for paired pictures and debugging). With the row off always the weather's
    /// layers.
    pub fn decide(cloud_on: bool, asked: CloudSource, ready: [bool; 2], gate: AutoGate) -> Self {
        match (cloud_on, asked) {
            (false, _) => Self::Off,
            (true, CloudSource::Weather) => Self::Weather { why: WeatherWhy::Asked },
            (true, _) if !gate.row => Self::Weather { why: WeatherWhy::RowOff },
            (true, CloudSource::Volume) => Self::Volume { near: ready[0], far: ready[1], only: true },
            (true, view @ (CloudSource::Share | CloudSource::Depth)) => Self::Debug { near: ready[0], far: ready[1], view },
            (true, CloudSource::Auto) => {
                if !gate.sky_shown || !gate.traced {
                    Self::Weather { why: WeatherWhy::SkyNotShown }
                } else if !ready[0] && !ready[1] {
                    Self::Weather { why: WeatherWhy::NoFront }
                } else {
                    Self::Volume { near: ready[0], far: ready[1], only: false }
                }
            }
        }
    }

    /// This source in the weather-only layout (no bindings 45-46, [`CloudVolumeRoom`]): whatever
    /// reads the volume (and the automatic source, whatever held it back) takes the weather's
    /// layers instead; `Off` and the aid `weather` stay.
    pub fn without_room(self) -> Self {
        match self {
            Self::Off | Self::Weather { why: WeatherWhy::Asked | WeatherWhy::RowOff } => self,
            _ => Self::Weather { why: WeatherWhy::NoRoom },
        }
    }

    /// The levels this source reads (near, far): none with the weather's layers.
    pub fn levels(self) -> [bool; 2] {
        match self {
            Self::Volume { near, far, .. } | Self::Debug { near, far, .. } => [near, far],
            _ => [false; 2],
        }
    }

    /// The log line's words.
    pub fn line(self) -> String {
        let levels = |near: bool, far: bool| match (near, far) {
            (true, true) => "near and far levels",
            (true, false) => "near level",
            (false, true) => "far level",
            (false, false) => "no level yet",
        };
        match self {
            Self::Off => "off".to_string(),
            Self::Weather { why } => format!(
                "the weather's layers ({})",
                match why {
                    WeatherWhy::Asked => "NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE=weather",
                    WeatherWhy::RowOff => "the Volumetric clouds row is off",
                    WeatherWhy::SkyNotShown => "the volumetric sky is not shown: nothing traced, or the flat layers still showing",
                    WeatherWhy::NoFront => "the light volume has no completed front: light volume off, or not swept yet",
                    WeatherWhy::NoRoom => "this adapter's fragment stage has too few sampled textures for the light volume's arrays: the weather-only layout",
                }
            ),
            Self::Volume { near, far, only: true } => format!("the light volume alone ({}; no shadow where it has no data, NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE=volume)", levels(near, far)),
            Self::Volume { near, far, only: false } => format!("the light volume ({}), the weather's layers where it has no data", levels(near, far)),
            Self::Debug { near, far, view } => format!("a debug view of the light volume's reader in place of K ({}; {}; NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE={})", levels(near, far), if view == CloudSource::Share { "the covered share" } else { "the depth over 8" }, view.name()),
        }
    }
}

/// The packed uniform with the cloud from the light volume (#3032 stage 2i M2) for this frame:
/// `use_` from [`CloudSourceInUse::decide`], `strength` the cloud shadows' strength, `window` the
/// shared reader's struct with its presences. With the weather's layers (or Cloud shadows off) the
/// uniform is `packed` as it came, so the shader takes today's path word for word.
pub fn with_cloud_volume(packed: SunVisibilityUniform, use_: CloudSourceInUse, strength: f32, window: LightVolumeWindow) -> SunVisibilityUniform {
    let mut u = packed;
    let bits = match use_ {
        CloudSourceInUse::Volume { only, .. } => Some(MASK_CLOUD_VOLUME | if only { MASK_CLOUD_VOLUME_ONLY } else { 0 }),
        CloudSourceInUse::Debug { view, .. } => Some(MASK_CLOUD_VOLUME | MASK_CLOUD_VOLUME_ONLY | if view == CloudSource::Share { MASK_CLOUD_VOLUME_SHARE } else { MASK_CLOUD_VOLUME_DEPTH }),
        _ => None,
    };
    if let Some(bits) = bits {
        let strength = if strength.is_finite() { strength.clamp(0.0, 1.0) } else { 0.0 };
        u.mode.x |= bits;
        u.volume = Vec4::new(strength, CLOUD_FADE_FROM, CLOUD_FADE_TO, 0.0);
        u.light_volume = window;
    }
    u
}

/// CPU mirror of sun_visibility.wesl `nvf_cloud_transmittance` after the reader (#3032 stage 2i
/// M2): K for the light's height `up` (its elevation's sine), the reader's result `r` (depth over
/// the covered share, the share) and the weather's K `weather` (asked only where it is used).
#[cfg_attr(not(test), allow(dead_code))]
pub fn cloud_transmittance(u: &SunVisibilityUniform, up: f32, r: Vec2, weather: &dyn Fn() -> f32) -> f32 {
    if u.mode.x & MASK_CLOUD_VOLUME == 0 {
        return weather();
    }
    let only = u.mode.x & MASK_CLOUD_VOLUME_ONLY != 0;
    if u.mode.x & MASK_CLOUD_VOLUME_SHARE != 0 {
        return r.y;
    }
    if u.mode.x & MASK_CLOUD_VOLUME_DEPTH != 0 {
        return if r.y > 0.0 { (r.x / 8.0).clamp(0.0, 1.0) } else { 0.0 };
    }
    if up <= u.volume.y {
        return 1.0;
    }
    let other = if r.y < 1.0 && !only { weather() } else { 1.0 };
    if r.y <= 0.0 {
        return other;
    }
    let k = (1.0 - u.volume.x * (1.0 - (-r.x).exp())).clamp(0.0, 1.0);
    let t = ((up - u.volume.y) / (u.volume.z - u.volume.y)).clamp(0.0, 1.0);
    let faded = 1.0 + (k - 1.0) * (t * t * (3.0 - 2.0 * t));
    other + (faded - other) * r.y
}

/// The neutral cloud (switched on, nothing published): strength 0 and a plane below every
/// receiver, so sun_visibility.wesl's `nvf_cloud_transmittance` returns 1 at its first test
/// and never reaches the texture or `smoothstep`.
pub const NEUTRAL_CLOUD: Vec4 = Vec4::new(f32::MIN, 0.0, CLOUD_FADE_FROM, CLOUD_FADE_TO);

impl SunVisibilityUniform {
    pub fn pack(v: &SunVisibility) -> Self {
        let mut u = Self::default();
        let mut mask = 0;
        if let Some(c) = &v.cloud {
            u.cloud_uv = Vec4::new(c.uv_scale.x, c.uv_scale.y, c.uv_offset.x, c.uv_offset.y);
            u.cloud = Vec4::new(c.plane_height, c.strength.clamp(0.0, 1.0), CLOUD_FADE_FROM, CLOUD_FADE_TO);
            mask |= MASK_CLOUD;
        } else if v.cloud_enabled {
            u.cloud = NEUTRAL_CLOUD;
        }
        // Switched on with no map: the terrain fields stay 0 and the mask bit clear, which
        // sun_visibility.wesl reads as T = 1 and Vsolid = C before touching the texture.
        if let Some(t) = v.terrain.as_ref().filter(|t| t.side > 0 && t.texel > 0.0) {
            u.terrain_uv = Vec4::new(t.uv_scale.x, t.uv_scale.y, t.uv_offset.x, t.uv_offset.y);
            u.terrain = Vec4::new(0.0, 0.0, t.texel, t.side as f32);
            mask |= MASK_TERRAIN;
        }
        u.mode = UVec4::new(mask, v.debug as u32, 0, 0);
        u
    }
}

/// On the light that is the outdoor sun while the plumbing is on (main world).
#[derive(Component, Clone, Copy, Debug)]
pub struct OutdoorSunLight;

/// On the cameras whose views take the sun visibility while the plumbing is on (main
/// world).
#[derive(Component, Clone, Copy, Debug)]
pub struct SunVisibilityView;

/// Test producer: the checkerboard's size in texels, its cells' size in texels and in game
/// units, and its plane (`NVF_PLAY_DEBUG_SUN_VISIBILITY_TEST=checker`).
const CHECKER_TEXELS: u32 = 64;
const CHECKER_CELL_TEXELS: u32 = 8;
pub const CHECKER_CELL: f32 = 2_000.0;
pub const CHECKER_PLANE: f32 = 20_000.0;

/// Whether `NVF_PLAY_DEBUG_SUN_VISIBILITY_TEST`'s value asks for the test producer (only
/// `checker` does): asked here and by cloud_shadows.rs, so the two cannot disagree.
pub fn test_producer_wanted(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.trim().eq_ignore_ascii_case("checker"))
}

/// A transmittance checkerboard: cells of [`CHECKER_CELL_TEXELS`] texels, clear (255) and
/// opaque (0) in turn.
pub fn checker_image() -> Image {
    let n = CHECKER_TEXELS;
    let data = (0..n * n)
        .map(|i| {
            let (x, y) = (i % n, i / n);
            if (x / CHECKER_CELL_TEXELS + y / CHECKER_CELL_TEXELS).is_multiple_of(2) { 255 } else { 0 }
        })
        .collect();
    Image::new(Extent3d { width: n, height: n, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::R8Unorm, RenderAssetUsages::RENDER_WORLD)
}

/// The test producer's cloud: [`checker_image`] with cells of [`CHECKER_CELL`] units,
/// strength 1, on a plane [`CHECKER_PLANE`] units up.
pub fn checker_cloud(image: Handle<Image>) -> CloudVisibility {
    let repeat = CHECKER_CELL * (CHECKER_TEXELS / CHECKER_CELL_TEXELS) as f32;
    CloudVisibility { image, uv_scale: Vec2::splat(1.0 / repeat), uv_offset: Vec2::ZERO, plane_height: CHECKER_PLANE, strength: 1.0 }
}

pub struct SunVisibilityPlugin;

impl Plugin for SunVisibilityPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "sun_visibility.wesl", |s, _| Shader::from_wesl(s, "embedded://nvforever/sun_visibility.wesl"));
        load_internal_asset!(app, READER_SHADER, "light_volume_reader.wesl", |s, _| Shader::from_wesl(s, "embedded://nvforever/light_volume_reader.wesl"));
        if !app.is_plugin_added::<crate::volumetric_clouds_lightvol::gpu::LightVolumeReaderPlugin>() {
            app.add_plugins(crate::volumetric_clouds_lightvol::gpu::LightVolumeReaderPlugin);
        }
        app.init_resource::<SunVisibility>()
            .add_systems(Startup, debug_aids)
            .add_systems(PostUpdate, (log_mask.run_if(resource_changed::<SunVisibility>), mark_targets.run_if(marking_needed)).chain());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.init_resource::<SunVisibilityGpu>().init_resource::<CloudVolumeFrame>().add_systems(ExtractSchedule, extract_sun_visibility.in_set(SunVisibilityExtracted)).add_systems(
            Render,
            prepare_sun_visibility.in_set(CloudShadowsDecided).in_set(RenderSystems::PrepareResourcesFlush).after(crate::volumetric_clouds_lightvol::gpu::publish_light_volume_reader),
        );
    }
}

/// `NVF_PLAY_DEBUG_SUN_VISIBILITY` and `NVF_PLAY_DEBUG_SUN_VISIBILITY_TEST`.
fn debug_aids(mut vis: ResMut<SunVisibility>, mut images: ResMut<Assets<Image>>) {
    if let Ok(value) = std::env::var("NVF_PLAY_DEBUG_SUN_VISIBILITY") {
        match DebugMode::parse(&value) {
            Some(mode) => vis.debug = mode,
            None => warn!("sun visibility: NVF_PLAY_DEBUG_SUN_VISIBILITY={value} unknown (cloud, terrain or combined)"),
        }
    }
    if let Ok(value) = std::env::var("NVF_PLAY_DEBUG_SUN_VISIBILITY_TEST") {
        if test_producer_wanted(Some(&value)) {
            vis.cloud = Some(checker_cloud(images.add(checker_image())));
            info!("sun visibility: test producer: checkerboard clouds, cells {CHECKER_CELL:.0} units, plane {CHECKER_PLANE:.0} units up, strength 1");
        } else if !value.trim().is_empty() {
            warn!("sun visibility: NVF_PLAY_DEBUG_SUN_VISIBILITY_TEST={value} unknown (checker)");
        }
    }
}

/// One line whenever the switches change.
fn log_mask(vis: Res<SunVisibility>, mut last: Local<Option<(bool, bool, DebugMode)>>) {
    let mask = vis.mask();
    if last.unwrap_or((false, false, DebugMode::Off)) != mask {
        let on = |b: bool| if b { "on" } else { "off" };
        info!("sun visibility: cloud {} terrain {} debug {}", on(mask.0), on(mask.1), mask.2.name());
    }
    *last = Some(mask);
}

/// Anything carrying one of the plumbing's markers.
type Marked = Or<(With<OutdoorSunLight>, With<SunVisibilityView>)>;

/// Whether [`mark_targets`] has anything to do: the plumbing is on, the light in the air
/// wants the sun marked, or markers are left to take off.
pub(crate) fn marking_needed(vis: Res<SunVisibility>, air: Option<Res<crate::medium_air_demand::AirWanted>>, marked: Query<(), Marked>) -> bool {
    vis.active() || air.is_some_and(|a| a.0) || !marked.is_empty()
}

type ViewCandidate = (Entity, Has<SunVisibilityView>, Has<FlyCam>, Has<ReflectionCamera>, Has<LensCamera>);
/// The world cameras and the views still marked.
type ViewFilter = Or<(With<FlyCam>, With<ReflectionCamera>, With<LensCamera>, With<SunVisibilityView>)>;
/// The sun light and the lights still marked.
type LightCandidate = (Entity, Has<Sun>, Has<OutdoorSunLight>);
type LightFilter = Or<(With<Sun>, With<OutdoorSunLight>)>;

/// Put [`OutdoorSunLight`] on the sun light while outdoors and the plumbing is on or the
/// light in the air wants it (absent: off), and [`SunVisibilityView`] on the world cameras
/// while the plumbing is on; take them off otherwise.
pub(crate) fn mark_targets(
    mut commands: Commands,
    vis: Res<SunVisibility>,
    air: Option<Res<crate::medium_air_demand::AirWanted>>,
    sky: Option<Res<SkyState>>,
    lights: Query<LightCandidate, LightFilter>,
    views: Query<ViewCandidate, ViewFilter>,
) {
    let active = vis.active();
    let sun_wanted = active || air.is_some_and(|a| a.0);
    let outdoors = sky.is_some_and(|s| s.interior.is_none());
    for (e, sun, marked) in &lights {
        let want = sun_wanted && outdoors && sun;
        if want && !marked {
            commands.entity(e).try_insert(OutdoorSunLight);
        } else if !want && marked {
            commands.entity(e).try_remove::<OutdoorSunLight>();
        }
    }
    for (e, marked, fly, reflection, lens) in &views {
        let want = active && (fly || reflection || lens);
        if want && !marked {
            commands.entity(e).try_insert(SunVisibilityView);
        } else if !want && marked {
            commands.entity(e).try_remove::<SunVisibilityView>();
        }
    }
}

/// Sampled textures kept free on the fragment stage above the fullest pipeline's count with the
/// light volume's two arrays before they are bound (#3032 stage 2i M2)
/// [engineering choice: covers what the count below does not see: a morph-target mesh's texture in
/// the mesh bind group, a material added later, a view key bit a later change sets].
pub const CLOUD_VOLUME_ROOM_MARGIN: u32 = 4;

/// The mesh view layout's sampled textures at its fullest when [`MeshPipelineViewLayouts`] is not
/// there to count them (bevy_pbr mesh_view_bindings.rs `layout_entries` for
/// [`fullest_view_key`]): the point and directional shadow maps (2, 5), the depth and motion
/// prepasses (20, 22), the view transmission
/// texture every 3D camera has (24, nvf-assets nif_material.rs), the sun visibility's cloud and
/// terrain maps (40, 41), the light volume's arrays (45, 46), the ambient occlusion (44) and,
/// on a device without binding arrays, [`REFLECTION_CUBE_TEXTURES`].
pub const FULLEST_VIEW_TEXTURES: u32 = 10 + REFLECTION_CUBE_TEXTURES;
/// A view's reflection cube (nvf-assets reflection_cube.rs: Bevy's `ENVIRONMENT_MAP`) without
/// binding arrays: the diffuse and the specular cube (bevy_pbr environment_map.rs
/// `get_bind_group_layout_entries`; with them they are arrays, counted against another limit).
pub const REFLECTION_CUBE_TEXTURES: u32 = 2;

/// Whether the light volume's two arrays (bindings 45-46) fit the adapter's fragment stage
/// (#3032 stage 2i M2). wgpu counts every sampled texture of a
/// pipeline layout that the fragment stage sees, outside binding arrays, against
/// `max_sampled_textures_per_shader_stage` (wgpu-core binding_model.rs
/// `BindingTypeMaxCountValidator`): the mesh view layout's plus the material's (bindless materials
/// hold theirs in binding arrays, which count against another limit). The fullest pipeline is the
/// fullest view layout a view that takes the arrays can have ([`fullest_view_key`]) with the material
/// drawn on this device holding the most such textures ([`most_textures`]); the arrays are bound only
/// when that count and [`CLOUD_VOLUME_ROOM_MARGIN`] fit the limit, else the views have the
/// weather-only layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloudVolumeRoom {
    /// The adapter's `max_sampled_textures_per_shader_stage`.
    pub limit: u32,
    /// The mesh view layout's sampled textures at its fullest, the two arrays included.
    pub view: u32,
    /// The most sampled textures a world material holds outside binding arrays.
    pub material: u32,
    /// The world's NIF and terrain materials are bindless on this adapter.
    pub bindless: bool,
    /// The arrays are bound.
    pub fits: bool,
}

impl CloudVolumeRoom {
    pub fn new(limit: u32, view: u32, material: u32, bindless: bool) -> Self {
        let fits = view.saturating_add(material).saturating_add(CLOUD_VOLUME_ROOM_MARGIN) <= limit;
        CloudVolumeRoom { limit, view, material, bindless, fits }
    }

    /// The fullest pipeline's sampled textures with the arrays.
    pub fn needed(&self) -> u32 {
        self.view.saturating_add(self.material)
    }

    /// The start's one log line.
    pub fn line(&self) -> String {
        let counts = format!(
            "the fullest pipeline samples {} textures in its fragment stage with them ({} the view's, {} the material's, materials {}), {} more kept free, the adapter allows {}",
            self.needed(),
            self.view,
            self.material,
            if self.bindless { "bindless" } else { "not bindless" },
            CLOUD_VOLUME_ROOM_MARGIN,
            self.limit
        );
        if self.fits {
            format!("the light volume's arrays (bindings 45-46) are bound: {counts}")
        } else {
            format!("the weather-only layout, no light volume arrays (bindings 45-46), the shadows from the weather's layers: {counts}")
        }
    }
}

/// The mesh view layout key at its fullest in this game of a view with the light volume's arrays:
/// every key bit such a view can have (graphics.rs: the depth prepass always, motion vectors with a
/// temporal method; the transmission texture
/// on every 3D camera; fog; the reflection cube of authored materials; the sun visibility and the
/// ambient occlusion in the lighting). Not the tonemapping in the shader (a table more): the arrays
/// come with the Volumetric clouds row, which keeps the main view HDR, and the mirror's and the
/// lens's cameras always are.
pub fn fullest_view_key() -> MeshPipelineViewLayoutKey {
    MeshPipelineViewLayoutKey::from(
        MeshPipelineKey::DEPTH_PREPASS
            | MeshPipelineKey::MOTION_VECTOR_PREPASS
            | MeshPipelineKey::ENVIRONMENT_MAP
            | MeshPipelineKey::VIEW_TRANSMISSION_TEXTURE
            | MeshPipelineKey::DISTANCE_FOG
            | MeshPipelineKey::NVF_SUN_VISIBILITY
            | MeshPipelineKey::NVF_CLOUD_VISIBILITY
            | MeshPipelineKey::NVF_TERRAIN_VISIBILITY
            | MeshPipelineKey::NVF_CLOUD_VOLUME_VISIBILITY
            | MeshPipelineKey::NVF_AMBIENT_OCCLUSION,
    )
}

/// Whether a layout entry counts against `max_sampled_textures_per_shader_stage` for the fragment
/// stage (a sampled texture the fragment stage sees, not in a binding array).
pub fn counts_as_sampled(e: &BindGroupLayoutEntry) -> bool {
    e.count.is_none() && e.visibility.contains(ShaderStages::FRAGMENT) && matches!(e.ty, BindingType::Texture { .. })
}

/// What the fragment stage sees of the bindings that wgpu limits by number, binding arrays left out
/// (they count against other limits).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StageCounts {
    /// Sampled ([`counts_as_sampled`]).
    pub textures: u32,
    pub samplers: u32,
    pub uniforms: u32,
}

impl StageCounts {
    pub fn of<'a>(entries: impl IntoIterator<Item = &'a BindGroupLayoutEntry>) -> Self {
        let mut out = StageCounts::default();
        for e in entries.into_iter().filter(|e| e.count.is_none() && e.visibility.contains(ShaderStages::FRAGMENT)) {
            match e.ty {
                _ if counts_as_sampled(e) => out.textures += 1,
                BindingType::Sampler(_) => out.samplers += 1,
                BindingType::Buffer { ty: BufferBindingType::Uniform, .. } => out.uniforms += 1,
                _ => {}
            }
        }
        out
    }

    /// With another group's.
    pub fn plus(self, other: StageCounts) -> Self {
        StageCounts { textures: self.textures.saturating_add(other.textures), samplers: self.samplers.saturating_add(other.samplers), uniforms: self.uniforms.saturating_add(other.uniforms) }
    }
}

/// The mesh view layout's bindings under `key`, from bevy_pbr's own layouts.
pub(crate) fn view_counts(layouts: &MeshPipelineViewLayouts, key: MeshPipelineViewLayoutKey) -> StageCounts {
    let layout = layouts.get_view_layout(key);
    StageCounts::of(layout.main_layout.entries.iter().chain(layout.binding_array_layout.entries.iter()))
}

/// A material's bindings outside binding arrays on this device, and whether it is bindless here.
fn material_counts<M: AsBindGroup>(device: &RenderDevice) -> (StageCounts, bool) {
    let bindless = bevy::render::material_bind_groups::material_uses_bindless_resources::<M>(device);
    (StageCounts::of(&M::bind_group_layout_entries(device, !bindless)), bindless)
}

/// The most textures of the materials drawn on a device: `others` always, the folded terrain
/// (`terrain`: its textures outside binding arrays, whether it is bindless) only where it is
/// bindless (terrain.rs `TerrainMaterials::folds`; elsewhere the land's quadrants and layers are
/// NIF materials).
pub fn most_textures(terrain: (u32, bool), others: &[u32]) -> u32 {
    others.iter().copied().chain(terrain.1.then_some(terrain.0)).max().unwrap_or(0)
}

/// The most bindings of each kind that a material drawn in the world's view has on this device
/// ([`most_textures`], whose rule is every kind's), and whether the world's NIF and terrain
/// materials are bindless.
pub(crate) fn world_material_counts(device: &RenderDevice) -> (StageCounts, bool) {
    let terrain = material_counts::<crate::terrain::TerrainMaterial>(device);
    let (nif, nif_bindless) = material_counts::<nvf_assets::nif_material::NifMaterial>(device);
    let others = [
        nif,
        material_counts::<nvf_assets::nif_surface_blend::NifBlendMaterial>(device).0,
        material_counts::<StandardMaterial>(device).0,
        material_counts::<crate::sky::SkyMaterial>(device).0,
        material_counts::<crate::grass::GrassMaterial>(device).0,
        material_counts::<crate::tree::LeafMaterial>(device).0,
        material_counts::<crate::tree::BarkMaterial>(device).0,
        material_counts::<crate::tree::BillboardMaterial>(device).0,
        material_counts::<crate::water::WaterMaterial>(device).0,
        material_counts::<crate::volumetric_clouds::CloudVolumeMaterial>(device).0,
        material_counts::<crate::distant_lights::GlowMaterial>(device).0,
    ];
    let most = |of: fn(StageCounts) -> u32| most_textures((of(terrain.0), terrain.1), &others.map(of));
    (StageCounts { textures: most(|c| c.textures), samplers: most(|c| c.samplers), uniforms: most(|c| c.uniforms) }, terrain.1 && nif_bindless)
}

/// [`CloudVolumeRoom`] on this device: the fullest view layout counted from bevy_pbr's own
/// layouts ([`FULLEST_VIEW_TEXTURES`] without them), the world's materials counted from their
/// derived layouts as this device makes them.
fn measure_room(device: &RenderDevice, layouts: Option<&MeshPipelineViewLayouts>) -> CloudVolumeRoom {
    let view = layouts.map_or(FULLEST_VIEW_TEXTURES, |l| view_counts(l, fullest_view_key()).textures);
    let (material, bindless) = world_material_counts(device);
    CloudVolumeRoom::new(device.limits().max_sampled_textures_per_shader_stage, view, material.textures, bindless)
}

/// Render world: the shared uniform buffer and sampler (made the first time the plumbing
/// is on, then kept), and what this frame's extraction asked for.
#[derive(Resource, Default)]
pub(crate) struct SunVisibilityGpu {
    parts: Option<GpuParts>,
    uniform: SunVisibilityUniform,
    cloud: Option<AssetId<Image>>,
    terrain: Option<AssetId<Image>>,
    /// The views' cloud variant, the source asked for and the strength (#3032 stage 2i M2).
    cloud_on: bool,
    source: CloudSource,
    strength: f32,
    /// The automatic source's view of the sky this frame.
    gate: AutoGate,
    /// Whether the light volume's arrays fit this adapter (measured once, when the layout with them
    /// is first wanted): the views' `cloud_volume` is the cloud variant, the row and this.
    room: Option<CloudVolumeRoom>,
    /// What is held for the layout with the light volume's arrays.
    volume: VolumeLayout,
}

impl SunVisibilityGpu {
    /// Whether the views' layout has the light volume's arrays (bindings 45-46) in this frame, for
    /// an extraction after [`SunVisibilityExtracted`].
    pub(crate) fn light_volume_bound(&self) -> bool {
        self.volume.on
    }
}

/// [`extract_sun_visibility`]'s set in the extraction.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SunVisibilityExtracted;

/// What the plumbing holds for the views' layout with the light volume's arrays, all of it gone with
/// the layout or the last view (`V` the stand-in's view, another type in the tests).
struct VolumeLayout<V = TextureView> {
    /// The views have that layout.
    on: bool,
    /// The bindings' stand-in ([`light_fallback`]), while they have.
    stand_in: Option<V>,
    /// They had it in the last frame and have it no more: their views are still to be let go.
    release: bool,
    /// The source last named in the log, while the light volume is asked for ([`volume_wanted`]).
    told: Option<CloudSourceInUse>,
}

impl<V> Default for VolumeLayout<V> {
    fn default() -> Self {
        VolumeLayout { on: false, stand_in: None, release: false, told: None }
    }
}

impl<V> VolumeLayout<V> {
    /// The layout of this frame, the stand-in made or dropped with it.
    fn set(&mut self, on: bool, make: impl FnOnce() -> V) {
        if !on {
            self.release |= self.on;
            self.stand_in = None;
        } else if self.stand_in.is_none() {
            self.stand_in = Some(make());
        }
        self.on = on;
    }

    /// No view takes the sun visibility: nothing is held, and the next one starts anew.
    fn leave(&mut self) {
        if self.on || self.release || self.stand_in.is_some() || self.told.is_some() {
            *self = VolumeLayout::default();
        }
    }
}

/// Whether the cloud shadows may come from the light volume at all: Cloud shadows and the Volumetric
/// clouds row on. With either off nothing of it is asked, measured, bound or named in the log.
pub fn volume_wanted(cloud_on: bool, row: bool) -> bool {
    cloud_on && row
}

/// Whether the views' layout has the light volume's arrays (bindings 45-46, the pipelines the
/// reader): [`volume_wanted`], and room on the adapter (`fits`, asked
/// only then). With the row off the layout, the pipelines and the shader are the weather's alone.
pub fn volume_layout(cloud_on: bool, row: bool, fits: impl FnOnce() -> bool) -> bool {
    volume_wanted(cloud_on, row) && fits()
}

/// The source to name in the log this frame, if any: a change while the light volume is asked for
/// (`wanted`, [`volume_wanted`]), and once the one left when it is not any more, where that is
/// another; nothing else. `told` is the one named last, none while it is not asked for.
fn source_told(told: &mut Option<CloudSourceInUse>, now: CloudSourceInUse, wanted: bool) -> Option<CloudSourceInUse> {
    let last = std::mem::replace(told, wanted.then_some(now));
    (last != Some(now) && (last.is_some() || wanted)).then_some(now)
}

/// What the surfaces' cloud shadow is this frame, for the other readers of the sun's visibility
/// (the light in the air, medium_air_grid.rs): one decision, one window and one strength for the
/// ground and the shafts (#3032). Written by [`prepare_sun_visibility`] each frame the light's render
/// side is open (else the default, [`CloudSourceInUse::Off`], as while no view takes the sun
/// visibility); read after it ([`CloudShadowsDecided`]), in `RenderSystems::PrepareResourcesFlush` or later.
#[derive(Resource, Clone, Default)]
pub(crate) struct CloudVolumeFrame {
    /// The surfaces' source.
    pub(crate) source: CloudSourceInUse,
    /// The light volume may become the source as soon as it has a front ([`volume_expected`]): a
    /// reader with a pipeline of its own asks for it now.
    pub(crate) expected: bool,
    /// The surfaces' uniform as written (its `volume` and `light_volume` count with
    /// [`MASK_CLOUD_VOLUME`] only).
    pub(crate) uniform: SunVisibilityUniform,
    /// The near and far levels the surfaces bind (stand-ins for a level of presence 0), while the
    /// light volume is in their layout.
    pub(crate) levels: Option<[TextureView; 2]>,
}

impl CloudVolumeFrame {
    /// Nothing for anybody (and nothing written while that is so already).
    fn clear(&mut self) {
        if self.source != CloudSourceInUse::Off || self.levels.is_some() {
            *self = CloudVolumeFrame::default();
        }
    }
}

/// [`prepare_sun_visibility`]'s set in the render schedule: a reader of [`CloudVolumeFrame`] runs after
/// it.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CloudShadowsDecided;

/// Whether the light volume can become the cloud shadows' source once it has a front: Cloud shadows
/// on, the light volume in the layout ([`volume_layout`]: with the Volumetric clouds row off it never
/// is), and the aid asks for the volume or, for `Auto`, the clouds are traced. The aids `volume`,
/// `share` and `depth` expect it whatever is traced, as [`CloudSourceInUse::decide`] takes the volume
/// for them.
pub fn volume_expected(cloud_on: bool, asked: CloudSource, traced: bool, room: bool) -> bool {
    cloud_on
        && room
        && match asked {
            CloudSource::Weather => false,
            CloudSource::Auto => traced,
            CloudSource::Volume | CloudSource::Share | CloudSource::Depth => true,
        }
}

struct GpuParts {
    buffer: UniformBuffer<SunVisibilityUniform>,
    sampler: Sampler,
    /// The terrain binding's stand-in ([`terrain_fallback`]).
    terrain_fallback: TextureView,
    written: Option<SunVisibilityUniform>,
}

/// The light volume bindings' stand-in while a level has no readable front (#3032 stage 2i M2):
/// one texel, two layers of the levels' format, zero (no depth). The shader samples a level only
/// with its presence above 0, which the CPU sets only for a level it binds.
fn light_fallback(device: &RenderDevice, queue: &RenderQueue) -> TextureView {
    let texture = device.create_texture_with_data(
        queue,
        &TextureDescriptor {
            label: Some("nvf_light_volume_fallback"),
            size: Extent3d { width: 1, height: 1, depth_or_array_layers: 2 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: crate::volumetric_clouds_lightvol::gpu::LV_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        &[0u8; 4],
    );
    texture.create_view(&TextureViewDescriptor { dimension: Some(TextureViewDimension::D2Array), ..default() })
}

/// The terrain map's format (terrain_shadows.rs `OUTPUT_FORMAT`): binding 41 is declared
/// unfilterable float for it (engine/vendor/README.md).
pub const TERRAIN_FORMAT: TextureFormat = TextureFormat::Rgba32Float;

/// The terrain binding's stand-in while no map is uploaded: one texel of [`TERRAIN_FORMAT`]
/// that casts nothing (S = -1e30, the producer's `NO_SHADOW`; bevy's white fallback would
/// read S = 1 and shade every receiver below y = 1). Whenever it is bound in place of a
/// requested map, [`usable_uniform`] clears the uniform's terrain bit, so the shader returns
/// T = 1 before any load; the texel only matters if something reads it anyway.
fn terrain_fallback(device: &RenderDevice, queue: &RenderQueue) -> TextureView {
    let texel = [crate::terrain_shadows::NO_SHADOW, 0.0, 0.0, 0.0];
    let bytes: Vec<u8> = texel.iter().flat_map(|v| v.to_le_bytes()).collect();
    let texture = device.create_texture_with_data(
        queue,
        &TextureDescriptor {
            label: Some("nvf_terrain_visibility_fallback"),
            size: Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TERRAIN_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        &bytes,
    );
    texture.create_view(&TextureViewDescriptor::default())
}

/// Mirror the markers into the render world: `NvfOutdoorSun` on the sun light's render
/// entity, `NvfSunVisibilityBindings` on the marked views'. The bindings are inserted,
/// changed and removed only here, as bevy_pbr requires (mesh_view_bindings.rs).
#[allow(clippy::too_many_arguments)]
#[allow(clippy::type_complexity)]
fn extract_sun_visibility(
    mut commands: Commands,
    vis: Extract<Res<SunVisibility>>,
    suns: Extract<Query<RenderEntity, With<OutdoorSunLight>>>,
    views: Extract<Query<RenderEntity, With<SunVisibilityView>>>,
    marked_suns: Query<Entity, With<NvfOutdoorSun>>,
    mut bound: Query<(Entity, &mut NvfSunVisibilityBindings)>,
    mut gpu: ResMut<SunVisibilityGpu>,
    (device, queue, fallback): (Res<RenderDevice>, Res<RenderQueue>, Res<FallbackImage>),
    (medium_output, medium_request): (Extract<Option<Res<crate::medium::MediumOutput>>>, Extract<Option<Res<crate::medium::MediumRequest>>>),
    (layouts, graphics): (Option<Res<MeshPipelineViewLayouts>>, Extract<Option<Res<crate::graphics::Graphics>>>),
) {
    if views.is_empty() {
        gpu.volume.leave();
    }
    let active = vis.active();
    if !active && suns.is_empty() && views.is_empty() && marked_suns.is_empty() && bound.is_empty() {
        return;
    }
    for e in &marked_suns {
        if !suns.iter().any(|s| s == e) {
            commands.entity(e).try_remove::<NvfOutdoorSun>();
        }
    }
    for e in &suns {
        if !marked_suns.contains(e) {
            commands.entity(e).try_insert(NvfOutdoorSun);
        }
    }
    for (e, _) in &bound {
        if !views.iter().any(|v| v == e) {
            commands.entity(e).try_remove::<NvfSunVisibilityBindings>();
        }
    }
    if views.is_empty() {
        return;
    }
    gpu.uniform = SunVisibilityUniform::pack(&vis);
    gpu.cloud = vis.cloud.as_ref().map(|c| c.image.id());
    gpu.terrain = vis.terrain.as_ref().map(|t| t.image.id());
    // The key bits follow the switches, not the publication (module notes).
    let (cloud, terrain) = (vis.cloud_on(), vis.terrain_on());
    (gpu.cloud_on, gpu.source, gpu.strength) = (cloud, vis.cloud_source, vis.cloud_strength);
    // The automatic source's gate (#3032 stage 2i M2): read every frame the light volume is asked
    // for, as the sky's state changes without the switches changing.
    let row = graphics.as_deref().is_some_and(|g| g.volumetric_clouds);
    let wanted = volume_wanted(cloud, row);
    gpu.gate = AutoGate {
        row,
        sky_shown: wanted && medium_output.as_deref().is_some_and(|o| o.ready),
        traced: wanted && medium_request.as_deref().is_some_and(|r| r.trace.is_some()),
    };
    // The light volume's arrays only while the Volumetric clouds row is on and where the adapter
    // has room for them: measured once, constant for the run, so the key bit
    // changes only with the cloud variant and the row.
    let cloud_volume = volume_layout(cloud, row, || {
        gpu.room
            .get_or_insert_with(|| {
                let room = measure_room(&device, layouts.as_deref());
                info!("cloud shadows: {}", room.line());
                room
            })
            .fits
    });
    let uniform = gpu.uniform;
    gpu.volume.set(cloud_volume, || light_fallback(&device, &queue));
    let gpu = &mut *gpu;
    let parts = gpu.parts.get_or_insert_with(|| {
        let mut buffer = UniformBuffer::from(uniform);
        buffer.set_label(Some("nvf_sun_visibility"));
        buffer.write_buffer(&device, &queue);
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("nvf_sun_visibility"),
            address_mode_u: AddressMode::Repeat,
            address_mode_v: AddressMode::Repeat,
            address_mode_w: AddressMode::Repeat,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        });
        GpuParts { buffer, sampler, terrain_fallback: terrain_fallback(&device, &queue), written: Some(uniform) }
    });
    let Some(buffer) = parts.buffer.buffer() else { return };
    for e in &views {
        if let Ok((_, mut b)) = bound.get_mut(e) {
            if b.cloud != cloud || b.terrain != terrain || b.cloud_volume != cloud_volume {
                b.cloud = cloud;
                b.terrain = terrain;
                b.cloud_volume = cloud_volume;
            }
        } else {
            // Out of the layout the light volume's views are not bound: any will do.
            let light = gpu.volume.stand_in.as_ref().unwrap_or(&fallback.d2_array.texture_view);
            commands.entity(e).try_insert(NvfSunVisibilityBindings {
                uniform: buffer.clone(),
                cloud_texture: fallback.d2.texture_view.clone(),
                terrain_texture: parts.terrain_fallback.clone(),
                sampler: parts.sampler.clone(),
                light_near: light.clone(),
                light_far: light.clone(),
                cloud,
                terrain,
                cloud_volume,
            });
        }
    }
}

/// The uniform as written for a frame: the packed one, with the terrain bit cleared when the
/// map it names is not an uploaded GPU image this frame (review of f8d532cca, finding 5).
/// The packed uniform advertises the map's side (512); with the one-texel stand-in bound
/// instead, the shader would clamp its loads to texel 511 of a 1 × 1 texture, whose result
/// is not defined to be the stand-in's texel. Cleared, T = 1 before any load and Vsolid = C
/// (no end-band blend without a usable map), as with nothing published.
pub fn usable_uniform(packed: SunVisibilityUniform, terrain_uploaded: bool) -> SunVisibilityUniform {
    let mut u = packed;
    if !terrain_uploaded {
        u.mode.x &= !MASK_TERRAIN;
    }
    u
}

/// Write the uniform when it changed and point the views at this frame's textures (a
/// texture not uploaded yet: the white fallback for the cloud, which lets the light
/// through, and [`terrain_fallback`] for the terrain, which casts nothing, with the
/// uniform's terrain bit cleared, [`usable_uniform`]). The texture views are compared every
/// frame, so the producer's other target, or a new texture of the same image, is bound from
/// the frame it is prepared on. A view kept from one version to the next (Bevy's `GpuImage`
/// preparation writes a new version of the same size and usage into the existing texture
/// and keeps its view; engine/vendor bevy_render `texture/gpu_image.rs`) needs nothing
/// rebuilt: the bind group holds the view, and the view reads the texture's new contents.
///
/// The cloud from the light volume (#3032 stage 2i M2): the shared reader's levels this frame
/// (`LightVolumeReader`, made after the medium's prepare step, hence this system's place in
/// `RenderSystems::PrepareResourcesFlush`) decide the source ([`CloudSourceInUse::decide`]),
/// named in the log when it changes; with the volume the uniform gets its window and each
/// level's presence (faded in over `LVR_ARRIVAL_SECS`), and bindings 45 and 46 its front sets
/// (the stand-in for a level without one). While it is not asked for ([`volume_wanted`]) none of
/// that runs: the uniform is the packed one, and in the frame the layout goes the views are let go.
/// For the light in the air ([`CloudVolumeFrame`]) the frame's source, uniform and level views are
/// left while that light's render side is open, and then alone.
fn prepare_sun_visibility(
    (mut gpu, mut shared, air): (ResMut<SunVisibilityGpu>, ResMut<CloudVolumeFrame>, Option<Res<crate::medium_air_grid::AirGate>>),
    mut bound: Query<&mut NvfSunVisibilityBindings>,
    images: Res<RenderAssets<GpuImage>>,
    fallback: Res<FallbackImage>,
    reader: Option<Res<crate::volumetric_clouds_lightvol::gpu::LightVolumeReader>>,
    (device, queue): (Res<RenderDevice>, Res<RenderQueue>),
) {
    if bound.is_empty() {
        shared.clear();
        return;
    }
    let gpu = &mut *gpu;
    let Some(parts) = gpu.parts.as_mut() else {
        shared.clear();
        return;
    };
    let uploaded = |id: Option<AssetId<Image>>| id.and_then(|id| images.get(id)).map(|g| &g.texture_view);
    let terrain_map = uploaded(gpu.terrain);
    let mut uniform = usable_uniform(gpu.uniform, terrain_map.is_some());
    let wanted = volume_wanted(gpu.cloud_on, gpu.gate.row);
    // The light volume's views to bind, where they may have changed.
    let mut light: Option<[TextureView; 2]> = None;
    let mut source = None;
    if wanted || gpu.volume.told.is_some() {
        let reader = reader.as_deref().filter(|_| gpu.volume.on);
        let ready = reader.map_or([false; 2], |r| r.ready());
        let use_ = CloudSourceInUse::decide(gpu.cloud_on, gpu.source, ready, gpu.gate);
        let use_ = if gpu.volume.on { use_ } else { use_.without_room() };
        if let Some(told) = source_told(&mut gpu.volume.told, use_, wanted) {
            info!("cloud shadows: source {}", told.line());
        }
        source = Some(use_);
        if let (Some(r), Some(stand_in)) = (reader, gpu.volume.stand_in.as_ref()) {
            // Only the levels the source reads are present (a level left out has presence 0 and its stand-in bound).
            let used = use_.levels();
            let p = r.presence(crate::volumetric_clouds_lightvol::LVR_ARRIVAL_SECS);
            let window = r.window([0, 1].map(|i| if used[i] { p[i] } else { 0.0 }));
            uniform = with_cloud_volume(uniform, use_, gpu.strength, window);
            light = Some([0, 1].map(|i| r.levels[i].as_ref().filter(|_| used[i]).map_or(stand_in, |l| &l.view).clone()));
        }
    }
    if std::mem::take(&mut gpu.volume.release) && light.is_none() {
        light = Some([0, 1].map(|_| fallback.d2_array.texture_view.clone()));
    }
    if air.is_some_and(|gate| gate.is_open()) {
        let source = source.unwrap_or_else(|| CloudSourceInUse::decide(gpu.cloud_on, gpu.source, [false; 2], gpu.gate));
        *shared = CloudVolumeFrame { source, expected: volume_expected(gpu.cloud_on, gpu.source, gpu.gate.traced, gpu.volume.on), uniform, levels: light.clone() };
    } else {
        shared.clear();
    }
    if parts.written != Some(uniform) {
        parts.buffer.set(uniform);
        parts.buffer.write_buffer(&device, &queue);
        parts.written = Some(uniform);
    }
    let cloud = uploaded(gpu.cloud).unwrap_or(&fallback.d2.texture_view);
    let terrain = terrain_map.unwrap_or(&parts.terrain_fallback);
    for mut b in &mut bound {
        if b.cloud_texture.id() != cloud.id() {
            b.cloud_texture = cloud.clone();
        }
        if b.terrain_texture.id() != terrain.id() {
            b.terrain_texture = terrain.clone();
        }
        if let Some([near, far]) = &light {
            if b.light_near.id() != near.id() {
                b.light_near = near.clone();
            }
            if b.light_far.id() != far.id() {
                b.light_far = far.clone();
            }
        }
    }
}

/// The hook in bevy_pbr's `apply_pbr_lighting` [documented:
/// engine/vendor/bevy_pbr/src/render/pbr_functions.wesl (bevy_pbr 0.20.1) with lights.rs
/// `DISPLAY_EDITS` made; the tests here apply both to that file], in order, each at the first
/// occurrence of its anchor after the last edit: (anchor, text, where the text goes).
/// Every statement it adds is under `@if(NVF_SUN_VISIBILITY)`, and the fog block is kept
/// word for word under `@if(DISTANCE_FOG && !NVF_SUN_VISIBILITY)`, so without the shader
/// def the text a pipeline compiles is the display-value patch's.
pub const HOOK_EDITS: [(&str, &str, Place); 6] = [
    (
        "import bevy_render::maths::{E, powsafe};\n",
        "// NVForever sun visibility (nvforever sun_visibility.rs).
@if(NVF_SUN_VISIBILITY) import nvforever::sun_visibility::{nvf_sun_visibility, nvf_sun_visibility_parts, nvf_sun_debug_value, nvf_sun_debug_mode};
",
        Place::After,
    ),
    (
        "    // directional lights (direct)\n",
        "    // NVForever sun visibility: what the debug view shows (nvforever sun_visibility.rs).
    @if(NVF_SUN_VISIBILITY)
    var nvf_sun_debug = 1.0;

",
        Place::Before,
    ),
    (
        "        var light_contrib = lighting::directional_light(i, &lighting_input, enable_diffuse);\n",
        "        // NVForever sun visibility (nvforever sun_visibility.rs): the outdoor sun's shadow
        // becomes V = K * Vsolid, outside the shadow-receiver condition, so that receivers
        // and non-receivers (leaves, tree cards, water) alike take the clouds and terrain.
        @if(NVF_SUN_VISIBILITY)
        if ((*light).flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT) != 0u {
            let nvf_parts = nvf_sun_visibility_parts(in.world_position.xyz, (*light).direction_to_light, shadow, view_z);
            nvf_sun_debug = nvf_sun_debug_value(nvf_parts);
            shadow = nvf_parts.w;
        }
",
        Place::Before,
    ),
    (
        "        let transmitted_light_contrib =\n            lighting::directional_light(i, &transmissive_lighting_input, enable_diffuse);\n",
        "        // NVForever sun visibility: the transmitted lobe takes it too.
        @if(NVF_SUN_VISIBILITY)
        if ((*light).flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT) != 0u {
            transmitted_shadow = nvf_sun_visibility(diffuse_transmissive_lobe_world_position.xyz, (*light).direction_to_light, transmitted_shadow, view_z);
        }
",
        Place::Before,
    ),
    (
        "    return output_color;\n}\n",
        "    // NVForever sun visibility: the debug view draws the factor alone.
    @if(NVF_SUN_VISIBILITY)
    if nvf_sun_debug_mode() != 0u {
        output_color = vec4<f32>(vec3<f32>(nvf_sun_debug), output_color.a);
    }

",
        Place::Before,
    ),
    (FOG_BLOCK, FOG_BLOCK_HOOKED, Place::Replace),
];

/// Where an edit's text goes against its anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Before,
    After,
    Replace,
}

/// `main_pass_post_lighting_processing`'s fog [documented:
/// engine/vendor/bevy_pbr/src/render/pbr_functions.wesl, bevy_pbr 0.20.1].
const FOG_BLOCK: &str = "    @if(DISTANCE_FOG)
    // fog
    if ((pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT) != 0u) {
        output_color = apply_fog(
            view_bindings::fog,
            output_color,
            pbr_input.world_position.xyz,
            view_bindings::view.world_position.xyz,
            pbr_input.frag_coord.xy,
        );
    }
";

/// The same, left out while the sun visibility's debug view is drawn.
const FOG_BLOCK_HOOKED: &str = "    @if(DISTANCE_FOG && !NVF_SUN_VISIBILITY)
    // fog
    if ((pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT) != 0u) {
        output_color = apply_fog(
            view_bindings::fog,
            output_color,
            pbr_input.world_position.xyz,
            view_bindings::view.world_position.xyz,
            pbr_input.frag_coord.xy,
        );
    }
    // NVForever sun visibility: no fog over the debug view (nvforever sun_visibility.rs).
    @if(DISTANCE_FOG && NVF_SUN_VISIBILITY)
    if ((pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT) != 0u && nvf_sun_debug_mode() == 0u) {
        output_color = apply_fog(
            view_bindings::fog,
            output_color,
            pbr_input.world_position.xyz,
            view_bindings::view.world_position.xyz,
            pbr_input.frag_coord.xy,
        );
    }
";

/// `source` (lights.rs's display-value patch made) with [`HOOK_EDITS`]; `None` when an
/// anchor is missing (Bevy changed), and the caller keeps `source`.
pub fn hook_patched(source: &str) -> Option<String> {
    let mut out = String::with_capacity(source.len() + 4096);
    let mut rest = source;
    for (anchor, text, place) in HOOK_EDITS {
        let at = rest.find(anchor)?;
        out.push_str(&rest[..at]);
        match place {
            Place::Before => {
                out.push_str(text);
                out.push_str(anchor);
            }
            Place::After => {
                out.push_str(anchor);
                out.push_str(text);
            }
            Place::Replace => out.push_str(text),
        }
        rest = &rest[at + anchor.len()..];
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::lights::unix_lines;

    fn vendored_functions() -> String {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../vendor/bevy_pbr/src/render/pbr_functions.wesl");
        unix_lines(&std::fs::read_to_string(path).expect("vendored pbr_functions.wesl"))
    }

    /// The source with the display-value patch, then the hook.
    fn hooked() -> (String, String) {
        let display = crate::lights::display_patched(&vendored_functions()).expect("display patch applies");
        let hooked = hook_patched(&display).expect("hook applies");
        (display, hooked)
    }

    /// With nothing enabled the lighting is untouched: no marker goes anywhere, and the
    /// hook adds only statements under `@if(NVF_SUN_VISIBILITY)` (taking them out gives the
    /// display-value patch back word for word), so the shader text without the def is the
    /// old one.
    #[test]
    fn visibility_identity_preserves_lighting() {
        let (display, hooked) = hooked();
        // Each added text is comments and statements under the def.
        for (_, text, place) in HOOK_EDITS {
            if place == Place::Replace {
                continue;
            }
            let code: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")).collect();
            assert!(code[0].starts_with("@if(NVF_SUN_VISIBILITY)"), "{text}");
        }
        // The fog block is kept under `!NVF_SUN_VISIBILITY` and its twin is under the def.
        assert!(FOG_BLOCK_HOOKED.contains(&FOG_BLOCK.replace("@if(DISTANCE_FOG)", "@if(DISTANCE_FOG && !NVF_SUN_VISIBILITY)")));
        assert!(FOG_BLOCK_HOOKED.contains("@if(DISTANCE_FOG && NVF_SUN_VISIBILITY)"));
        // Taking the additions out again gives the display-value patch.
        let mut back = hooked.clone();
        for (anchor, text, place) in HOOK_EDITS {
            match place {
                Place::Before => back = back.replacen(&format!("{text}{anchor}"), anchor, 1),
                Place::After => back = back.replacen(&format!("{anchor}{text}"), anchor, 1),
                Place::Replace => back = back.replacen(text, anchor, 1),
            }
        }
        assert_eq!(back, display);

        // No producer, no debug view: the plumbing is off and nothing is marked.
        let mut app = App::new();
        app.insert_resource(SunVisibility::default()).insert_resource(SkyState::default()).add_systems(Update, mark_targets.run_if(marking_needed));
        let sun = app.world_mut().spawn((Sun, DirectionalLight::default())).id();
        let cam = app.world_mut().spawn(FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }).id();
        app.update();
        assert!(!SunVisibility::default().active());
        assert!(app.world().get::<OutdoorSunLight>(sun).is_none());
        assert!(app.world().get::<SunVisibilityView>(cam).is_none());
        assert_eq!(SunVisibilityUniform::pack(&SunVisibility::default()), SunVisibilityUniform::default());
    }

    /// The light in the air (#3032 stage 3) marks the outdoor sun alone: with both surface
    /// rows off the sun light gets [`OutdoorSunLight`] (render world: `NvfOutdoorSun`), the
    /// views no bindings and the plumbing stays off; not indoors, not the arms' shade light;
    /// the demand off again takes the marker off; an absent or false demand marks nothing.
    #[test]
    fn light_in_the_air_marks_the_outdoor_sun_only() {
        use crate::medium_air_demand::AirWanted;
        let mut app = App::new();
        app.insert_resource(SunVisibility::default()).insert_resource(SkyState::default()).add_systems(Update, mark_targets.run_if(marking_needed));
        let sun = app.world_mut().spawn((Sun, DirectionalLight::default())).id();
        let shade = app.world_mut().spawn(DirectionalLight::default()).id();
        let cam = app.world_mut().spawn(FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }).id();
        app.update();
        assert!(app.world().get::<OutdoorSunLight>(sun).is_none(), "no demand resource: unmarked");
        app.insert_resource(AirWanted(false));
        app.update();
        assert!(app.world().get::<OutdoorSunLight>(sun).is_none(), "demand off: unmarked");

        app.insert_resource(AirWanted(true));
        app.update();
        let w = app.world();
        assert!(w.get::<OutdoorSunLight>(sun).is_some(), "the air marks the outdoor sun");
        assert!(w.get::<OutdoorSunLight>(shade).is_none(), "never the arms' shade light");
        assert!(w.get::<SunVisibilityView>(cam).is_none(), "no view bindings");
        assert!(!w.resource::<SunVisibility>().active(), "the plumbing stays off");

        // Indoors the same light is unmarked; outdoors again, marked.
        app.world_mut().resource_mut::<SkyState>().interior = Some(Default::default());
        app.update();
        assert!(app.world().get::<OutdoorSunLight>(sun).is_none());
        app.world_mut().resource_mut::<SkyState>().interior = None;
        app.update();
        assert!(app.world().get::<OutdoorSunLight>(sun).is_some());

        // The demand off: the marker goes.
        app.insert_resource(AirWanted(false));
        app.update();
        assert!(app.world().get::<OutdoorSunLight>(sun).is_none());
        assert!(app.world().get::<SunVisibilityView>(cam).is_none());
    }

    /// Only the sun light outdoors takes the factor: not the arms' shade light or a menu's
    /// light, not the same light indoors; the world, reflection and scope views take the
    /// bindings, a menu's camera does not; switching off takes every marker off again.
    #[test]
    fn visibility_targets_outdoor_light() {
        let mut app = App::new();
        let images = Handle::<Image>::default();
        app.insert_resource(SunVisibility { cloud: Some(checker_cloud(images)), ..default() })
            .insert_resource(SkyState::default())
            .add_systems(Update, mark_targets.run_if(marking_needed));
        let sun = app.world_mut().spawn((Sun, DirectionalLight::default())).id();
        let shade = app.world_mut().spawn(DirectionalLight::default()).id();
        let world_cam = app.world_mut().spawn(FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }).id();
        let mirror = app.world_mut().spawn(ReflectionCamera).id();
        let lens = app.world_mut().spawn(LensCamera).id();
        let menu = app.world_mut().spawn(Name::new("Lockpicking camera")).id();
        app.update();
        let w = app.world();
        assert!(w.get::<OutdoorSunLight>(sun).is_some());
        assert!(w.get::<OutdoorSunLight>(shade).is_none());
        for cam in [world_cam, mirror, lens] {
            assert!(w.get::<SunVisibilityView>(cam).is_some());
        }
        assert!(w.get::<SunVisibilityView>(menu).is_none());

        // Indoors the same light is the cell's directional light: unmarked; the views keep
        // their bindings while the cloud stays set (the producer's own door path, which
        // clears the cloud, is cloud_shadows.rs `cloud_bindings_survive_doors`).
        app.world_mut().resource_mut::<SkyState>().interior = Some(Default::default());
        app.update();
        assert!(app.world().get::<OutdoorSunLight>(sun).is_none());
        assert!(app.world().get::<SunVisibilityView>(world_cam).is_some());

        // Outdoors again, then everything off.
        app.world_mut().resource_mut::<SkyState>().interior = None;
        app.update();
        assert!(app.world().get::<OutdoorSunLight>(sun).is_some());
        *app.world_mut().resource_mut::<SunVisibility>() = SunVisibility::default();
        app.update();
        let w = app.world();
        assert!(w.get::<OutdoorSunLight>(sun).is_none());
        for cam in [world_cam, mirror, lens] {
            assert!(w.get::<SunVisibilityView>(cam).is_none());
        }
    }

    #[test]
    fn hook_sits_in_the_sun_loop_outside_the_receiver_condition() {
        let (_, out) = hooked();
        let at = |s: &str| out.find(s).unwrap_or_else(|| panic!("{s} missing"));
        let directional = at("    // directional lights (direct)\n");
        let receiver = directional + out[directional..].find("shadow *= in.directional_shadow_factor;\n        }\n").expect("receiver block");
        let hook = at("shadow = nvf_parts.w;");
        let contrib = at("var light_contrib = lighting::directional_light(i, &lighting_input");
        let display = at("nvf_directional_display(i, in.N) * shadow");
        let transmitted_hook = at("transmitted_shadow = nvf_sun_visibility(");
        let transmitted_sum = transmitted_hook + out[transmitted_hook..].find("transmitted_light += transmitted_light_contrib * transmitted_shadow;").expect("transmitted sum");
        let debug = at("output_color = vec4<f32>(vec3<f32>(nvf_sun_debug)");
        let total = at("    // Total light\n");
        // After the receiver condition has closed, before both sums of the light.
        assert!(directional < receiver && receiver < hook && hook < contrib && contrib < display);
        assert!(display < transmitted_hook && transmitted_hook < transmitted_sum);
        assert!(transmitted_sum < total && total < debug);
        assert!(debug < at("@if(DISTANCE_FOG && NVF_SUN_VISIBILITY)"));
        assert!(at("@if(NVF_SUN_VISIBILITY) import nvforever::sun_visibility::") < at("fn nvf_apply_pbr_lighting("));
        assert_eq!(out.matches("nvf_sun_visibility_parts(").count(), 1, "called once");
        // A missing anchor (Bevy changed) leaves the source alone.
        assert!(hook_patched(&vendored_functions().replace("    // directional lights (direct)\n", "")).is_none());
    }

    #[test]
    fn uniform_packing() {
        assert_eq!(SunVisibilityUniform::min_size().get(), 176, "eleven vec4s, as sun_visibility.wesl's struct (the light volume's five among them)");
        let image = Handle::<Image>::default();
        let v = SunVisibility {
            cloud: Some(CloudVisibility { image: image.clone(), uv_scale: Vec2::new(0.5, 0.25), uv_offset: Vec2::new(3.0, 4.0), plane_height: 9_000.0, strength: 1.5 }),
            terrain: Some(TerrainVisibility { image, uv_scale: Vec2::splat(2.0), uv_offset: Vec2::new(-1.0, 1.0), texel: 256.0, side: 512 }),
            debug: DebugMode::Terrain,
            ..default()
        };
        let u = SunVisibilityUniform::pack(&v);
        assert_eq!(u.cloud_uv, Vec4::new(0.5, 0.25, 3.0, 4.0));
        assert_eq!(u.cloud, Vec4::new(9_000.0, 1.0, CLOUD_FADE_FROM, CLOUD_FADE_TO), "strength held to 1");
        assert_eq!(u.terrain_uv, Vec4::new(2.0, 2.0, -1.0, 1.0));
        assert_eq!(u.terrain, Vec4::new(0.0, 0.0, 256.0, 512.0), "xy unused");
        assert_eq!(u.mode, UVec4::new(MASK_CLOUD | MASK_TERRAIN, 2, 0, 0));
        // The WGSL struct names the same fields in the same order.
        let wesl = include_str!("sun_visibility.wesl");
        let fields: Vec<usize> = ["cloud_uv: vec4<f32>", "cloud: vec4<f32>", "terrain_uv: vec4<f32>", "terrain: vec4<f32>", "mode: vec4<u32>", "volume: vec4<f32>", "light_volume: LightVolumeWindow"]
            .iter()
            .map(|f| wesl.find(f).unwrap_or_else(|| panic!("{f}")))
            .collect();
        assert!(fields.windows(2).all(|w| w[0] < w[1]));
        assert_eq!((u.volume, u.light_volume), (Vec4::ZERO, LightVolumeWindow::default()), "packing alone never sets the volume's part");
        // Bindings as bevy_pbr's layout has them (mesh_view_bindings.rs, 39-42, 45-46).
        for b in [
            "@group(0) @binding(39) var<uniform> nvf_sun",
            "@group(0) @binding(40) var nvf_cloud_texture: texture_2d<f32>",
            "@group(0) @binding(41) var nvf_terrain_texture: texture_2d<f32>",
            "@group(0) @binding(42) var nvf_sun_visibility_sampler: sampler",
            "@group(0) @binding(45) var nvf_light_near: texture_2d_array<f32>",
            "@group(0) @binding(46) var nvf_light_far: texture_2d_array<f32>",
        ] {
            assert!(wesl.contains(b), "{b}");
        }
        // Binding 41 is declared unfilterable in the vendored layout (Rgba32Float is not
        // filterable without an optional feature), 40 filterable, 42 a filtering sampler;
        // the terrain map is only ever loaded, never sampled.
        use bevy::render::render_resource::{SamplerBindingType, TextureSampleType};
        let key = MeshPipelineViewLayoutKey::NVF_SUN_VISIBILITY | MeshPipelineViewLayoutKey::NVF_CLOUD_VOLUME;
        let built: Vec<BindGroupLayoutEntry> = bevy::pbr::nvf_sun_visibility_layout_entries(key).into_iter().map(|(b, e)| e.build(b, ShaderStages::FRAGMENT)).collect();
        let ty = |n: u32| built.iter().find(|e| e.binding == n).unwrap_or_else(|| panic!("binding {n}")).ty;
        let texture = |n: u32, filterable: bool, dimension: TextureViewDimension| {
            matches!(ty(n), BindingType::Texture { sample_type: TextureSampleType::Float { filterable: f }, view_dimension: d, multisampled: false } if f == filterable && d == dimension)
        };
        assert!(texture(40, true, TextureViewDimension::D2), "{:?}", ty(40));
        assert!(texture(41, false, TextureViewDimension::D2), "{:?}", ty(41));
        assert!(matches!(ty(42), BindingType::Sampler(SamplerBindingType::Filtering)));
        // The light volume's sets: filterable float arrays (R16Float), with the volume's key bit
        // (the weather-only layout leaves them out: `the_weather_only_layout_leaves_out_bindings_45_and_46`).
        for n in [45, 46] {
            assert!(texture(n, true, TextureViewDimension::D2Array), "{:?}", ty(n));
        }
        let bindings = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../vendor/bevy_pbr/src/render/mesh_view_bindings.rs")).expect("mesh_view_bindings.rs").replace("\r\n", "\n");
        assert!(bindings.contains("(45, &sun.light_near), (46, &sun.light_far)"), "bound in the bind group");
        assert_eq!(crate::volumetric_clouds_lightvol::gpu::LV_FORMAT, TextureFormat::R16Float, "filterable without a feature");
        assert!(!wesl.contains("textureSampleLevel(nvf_terrain_texture") && !wesl.contains("textureSample(nvf_terrain_texture"));
        assert_eq!(TERRAIN_FORMAT, crate::terrain_shadows::OUTPUT_FORMAT, "the stand-in has the producer's format");
        // Eleven vec4s on the CPU too.
        assert_eq!(std::mem::size_of::<SunVisibilityUniform>(), 176);
    }

    /// Switched on with nothing published: the plumbing is on and the views take the cloud
    /// variant (the key bits follow the switch), but the cloud is neutral (K = 1) and the
    /// mask says no image is there; a publication fills it, its loss empties it again
    /// without changing what the views are given.
    #[test]
    fn switched_on_without_a_cloud_is_neutral() {
        let on = SunVisibility { cloud_enabled: true, ..default() };
        assert!(on.active() && on.cloud_on());
        assert_eq!(on.mask(), (true, false, DebugMode::Off), "the log names the switch");
        let u = SunVisibilityUniform::pack(&on);
        assert_eq!(u.cloud, NEUTRAL_CLOUD);
        assert_eq!(u.cloud.y, 0.0, "strength 0");
        assert_eq!(u.mode.x & MASK_CLOUD, 0, "no image published");
        // Published: the image's values and the mask bit; the switch's bits unchanged.
        let published = SunVisibility { cloud: Some(checker_cloud(Handle::default())), ..on.clone() };
        assert_eq!((published.active(), published.cloud_on()), (true, true));
        assert_eq!(SunVisibilityUniform::pack(&published).mode.x & MASK_CLOUD, MASK_CLOUD);
        // Off: nothing.
        assert!(!SunVisibility::default().cloud_on());
    }

    /// With the volume not in use (Cloud shadows off, the aid `weather`, or no readable front) the
    /// uniform is the packed one and the shader's first statement hands K to the weather's
    /// function, whose body is the one K had before stage 2i M2, line for line: the picture is
    /// today's, bit for bit.
    #[test]
    fn cloud_volume_off_is_todays_cloud() {
        let packed = SunVisibilityUniform::pack(&SunVisibility { cloud: Some(checker_cloud(Handle::default())), ..default() });
        let window = LightVolumeWindow { floor: Vec4::new(21_024.0, 4.0, 1.0, 1.0), ..default() };
        for use_ in [CloudSourceInUse::Off, CloudSourceInUse::Weather { why: WeatherWhy::NoFront }, CloudSourceInUse::Weather { why: WeatherWhy::Asked }, CloudSourceInUse::Weather { why: WeatherWhy::SkyNotShown }] {
            assert_eq!(with_cloud_volume(packed, use_, 0.6, window), packed, "{use_:?}");
        }
        assert_eq!(CloudSourceInUse::decide(true, CloudSource::Auto, [false, false], SHOWN), CloudSourceInUse::Weather { why: WeatherWhy::NoFront }, "no front: the layers");
        assert_eq!(CloudSourceInUse::decide(true, CloudSource::Weather, [true, true], SHOWN), CloudSourceInUse::Weather { why: WeatherWhy::Asked });
        assert_eq!(CloudSourceInUse::decide(false, CloudSource::Volume, [true, true], SHOWN), CloudSourceInUse::Off);
        // The mirror: bit clear, the weather's K whatever the reader says.
        for r in [Vec2::ZERO, Vec2::new(3.0, 0.5), Vec2::new(3.0, 1.0)] {
            assert_eq!(cloud_transmittance(&packed, 0.5, r, &|| 0.4375), 0.4375);
        }
        let wesl = include_str!("sun_visibility.wesl").replace("\r\n", "\n");
        let weather = block_after(&wesl, "fn nvf_cloud_weather(P: vec3<f32>, L: vec3<f32>) -> f32");
        let before = "{
    let height = nvf_sun.cloud.x;
    let up = L.y;
    if up <= nvf_sun.cloud.z || P.y >= height {
        return 1.0;
    }
    let hit = P.xz + L.xz * ((height - P.y) / up);
    let uv = hit * nvf_sun.cloud_uv.xy + nvf_sun.cloud_uv.zw;
    let transmittance = textureSampleLevel(nvf_cloud_texture, nvf_sun_visibility_sampler, uv, 0.0).r;
    let k = saturate(1.0 - nvf_sun.cloud.y * (1.0 - transmittance));
    return mix(1.0, k, smoothstep(nvf_sun.cloud.z, nvf_sun.cloud.w, up));
}";
        assert_eq!(weather, before, "the weather's K is the old K word for word");
        let k = block_after(&wesl, "@if(NVF_CLOUD_VISIBILITY && NVF_CLOUD_VOLUME)\nfn nvf_cloud_transmittance(P: vec3<f32>, L: vec3<f32>) -> f32");
        assert_eq!(code_lines(k)[1..4], ["    if (nvf_sun.mode.x & 4u) == 0u {", "        return nvf_cloud_weather(P, L);", "    }"], "the first statement");
        assert!(k.contains("(nvf_sun.mode.x & 8u) != 0u"));
        assert_eq!((MASK_CLOUD_VOLUME, MASK_CLOUD_VOLUME_ONLY), (4, 8));
    }

    /// With the volume in use: where the reader covers the receiver whole, K is the volume's
    /// `1 - strength (1 - exp(-depth))` faded over the low-sun band (the weather's K is not asked
    /// for); where it covers none, the weather's K (no shadow with the aid `volume`); between, the
    /// two mixed by the covered share, continuously; a light under the band casts nothing.
    #[test]
    fn cloud_volume_hands_over_to_the_weather() {
        let packed = SunVisibilityUniform::pack(&SunVisibility { cloud_enabled: true, ..default() });
        let window = LightVolumeWindow { floor: Vec4::new(21_024.0, 4.0, 1.0, 1.0), ..default() };
        let auto = with_cloud_volume(packed, CloudSourceInUse::Volume { near: true, far: true, only: false }, 0.6, window);
        assert_eq!(auto.mode.x & (MASK_CLOUD_VOLUME | MASK_CLOUD_VOLUME_ONLY), MASK_CLOUD_VOLUME);
        assert_eq!(auto.volume, Vec4::new(0.6, CLOUD_FADE_FROM, CLOUD_FADE_TO, 0.0));
        assert_eq!(auto.light_volume, window);
        let never = || -> f32 { panic!("the weather's K was asked for where the volume covers all") };
        let depth = 2.0f32;
        let volume_k = 1.0 - 0.6 * (1.0 - (-depth).exp());
        assert!((cloud_transmittance(&auto, 0.5, Vec2::new(depth, 1.0), &never) - volume_k).abs() < 1e-6);
        assert_eq!(cloud_transmittance(&auto, 0.5, Vec2::ZERO, &|| 0.7), 0.7, "no data: the weather's K");
        let half = cloud_transmittance(&auto, 0.5, Vec2::new(depth, 0.5), &|| 0.7);
        assert!((half - 0.5 * (0.7 + volume_k)).abs() < 1e-6, "{half}");
        let mut prev = cloud_transmittance(&auto, 0.5, Vec2::new(depth, 0.0), &|| 0.7);
        for k in 1..=100 {
            let c = k as f32 / 100.0;
            let now = cloud_transmittance(&auto, 0.5, Vec2::new(depth, c), &|| 0.7);
            assert!((now - prev).abs() <= 0.01 + 1e-6, "continuous in the covered share");
            prev = now;
        }
        assert_eq!(cloud_transmittance(&auto, CLOUD_FADE_FROM, Vec2::new(depth, 1.0), &never), 1.0, "a light at the band's foot casts nothing");
        let mid = cloud_transmittance(&auto, 0.5 * (CLOUD_FADE_FROM + CLOUD_FADE_TO), Vec2::new(depth, 1.0), &never);
        assert!(mid > volume_k && mid < 1.0, "faded in over the band: {mid}");
        let only = with_cloud_volume(packed, CloudSourceInUse::Volume { near: false, far: true, only: true }, 0.6, window);
        assert_eq!(only.mode.x & MASK_CLOUD_VOLUME_ONLY, MASK_CLOUD_VOLUME_ONLY);
        let no_weather = || -> f32 { panic!("the aid `volume` never reads the weather's layers") };
        assert_eq!(cloud_transmittance(&only, 0.5, Vec2::ZERO, &no_weather), 1.0, "no data, no shadow");
        assert!((cloud_transmittance(&only, 0.5, Vec2::new(depth, 0.5), &no_weather) - 0.5 * (1.0 + volume_k)).abs() < 1e-6);
        assert_eq!(CloudSourceInUse::decide(true, CloudSource::Volume, [false, false], SHOWN), CloudSourceInUse::Volume { near: false, far: false, only: true }, "the aid holds with no front");
        assert_eq!(CloudSourceInUse::decide(true, CloudSource::Auto, [false, true], SHOWN), CloudSourceInUse::Volume { near: false, far: true, only: false });
        assert_eq!(with_cloud_volume(packed, CloudSourceInUse::Volume { near: true, far: false, only: false }, f32::NAN, window).volume.x, 0.0, "a strength that is not a number casts nothing");
    }

    /// #3032 stage 2i M2: the light volume's arrays are bound only when the
    /// fullest pipeline's fragment textures with them, plus the margin, fit the adapter's
    /// `max_sampled_textures_per_shader_stage`; else the weather-only layout, where every source but
    /// the aid `weather` takes the weather's layers. The counts here are this build's: the fullest
    /// view layout 10 with the arrays (shadow maps 2, depth and motion prepasses 2,
    /// transmission 1, sun visibility 2 + 2, ambient occlusion 1), two more with a reflection cube
    /// on a device without binding arrays; StandardMaterial's 8 with `pbr_specular_textures`, the
    /// NIF material's 12, the folded terrain's 28 with #3322's and #3846's height maps, drawn where it is bindless
    /// only (its textures are in binding arrays then), and 10
    /// for the largest material that stays non-bindless when the rest are (the sky's 8 + 2).
    #[test]
    fn cloud_volume_room_decides_the_layout() {
        assert_eq!(CLOUD_VOLUME_ROOM_MARGIN, 4);
        let textures = |source: &str, from: &str, to: &str| {
            let s = &source[source.find(from).unwrap_or_else(|| panic!("{from}"))..];
            s[..s.find(to).expect("its end")].matches("#[texture(").count() as u32
        };
        let standard = unix_lines(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../vendor/bevy_pbr/src/pbr_material.rs")).expect("pbr_material.rs"));
        let standard = textures(&standard, "pub struct StandardMaterial {", "\n}\n") + standard.matches("cfg_attr(feature = \"pbr_specular_textures\", texture(").count() as u32;
        let nif = standard + textures(&unix_lines(include_str!("../../nvf-assets/src/nif_material.rs")), "pub struct NifSurface {", "\n}\n");
        let folded = standard + textures(&unix_lines(include_str!("terrain.rs")), "pub struct TerrainLayers {", "\n}\n");
        let sky = standard + textures(&unix_lines(include_str!("sky.rs")), "pub struct SkyDisplay {", "\n}\n");
        assert_eq!((standard, nif, folded, sky), (8, 12, 28, 10));
        // Not bindless: the folded terrain is not drawn; with a reflection cube (no binding arrays)
        // 24 with the arrays, 28 with the margin.
        let others = [nif, standard + 2, standard, sky];
        let material = most_textures((folded, false), &others);
        assert_eq!((FULLEST_VIEW_TEXTURES, material), (12, 12));
        assert!(!CloudVolumeRoom::new(16, FULLEST_VIEW_TEXTURES, material, false).fits, "WebGPU's default 16");
        assert!(!CloudVolumeRoom::new(27, FULLEST_VIEW_TEXTURES, material, false).fits, "one short of the margin");
        assert!(CloudVolumeRoom::new(28, FULLEST_VIEW_TEXTURES, material, false).fits);
        assert!(CloudVolumeRoom::new(26, FULLEST_VIEW_TEXTURES - REFLECTION_CUBE_TEXTURES, material, false).fits, "the cube takes two of them");
        // Bindless, the cubes in binding arrays: 20 with the arrays, 24 with the margin. A bindless
        // terrain counts with what it keeps outside them.
        assert_eq!((most_textures((0, true), &[0, 0, 0, sky]), most_textures((folded, true), &others)), (10, 28));
        let view = FULLEST_VIEW_TEXTURES - REFLECTION_CUBE_TEXTURES;
        assert!(!CloudVolumeRoom::new(23, view, sky, true).fits && CloudVolumeRoom::new(24, view, sky, true).fits);
        assert!(CloudVolumeRoom::new(1 << 20, FULLEST_VIEW_TEXTURES, 28, false).fits, "a desktop adapter's limit");
        // The views that take the arrays are HDR, so none tonemaps in the shader; the cube is
        // counted with the others.
        assert!(fullest_view_key().contains(MeshPipelineViewLayoutKey::ENVIRONMENT_MAP) && !fullest_view_key().contains(MeshPipelineViewLayoutKey::TONEMAP_IN_SHADER));
        assert!(include_str!("graphics.rs").contains("if temporal || g.volumetric_clouds"), "the row keeps the main view HDR");
        for (camera, source) in [("the mirror's", include_str!("reflection.rs")), ("the lens's", include_str!("scope_view.rs"))] {
            assert!(source.contains("Hdr,"), "{camera} camera is HDR");
        }
        assert!(!CloudVolumeRoom::new(u32::MAX - 1, u32::MAX, 1, true).fits, "saturating, no overflow");
        let room = CloudVolumeRoom::new(23, 12, 12, false);
        assert_eq!(room.needed(), 24);
        assert!(room.line().starts_with("the weather-only layout") && room.line().contains("allows 23"), "{}", room.line());
        assert!(CloudVolumeRoom::new(1 << 20, 10, 10, true).line().starts_with("the light volume's arrays (bindings 45-46) are bound"));
        // The source in the weather-only layout.
        let both = [true, true];
        for asked in [CloudSource::Auto, CloudSource::Volume, CloudSource::Share, CloudSource::Depth] {
            assert_eq!(CloudSourceInUse::decide(true, asked, both, SHOWN).without_room(), CloudSourceInUse::Weather { why: WeatherWhy::NoRoom }, "{asked:?}");
        }
        assert_eq!(CloudSourceInUse::decide(true, CloudSource::Weather, both, SHOWN).without_room(), CloudSourceInUse::Weather { why: WeatherWhy::Asked });
        assert_eq!(CloudSourceInUse::Off.without_room(), CloudSourceInUse::Off);
        assert_eq!(CloudSourceInUse::Weather { why: WeatherWhy::NoRoom }.levels(), [false, false]);
    }

    /// The mesh view layout's sun visibility entries (vendored bevy_pbr
    /// `nvf_sun_visibility_layout_entries`, which `layout_entries` uses): none without the key bit,
    /// 39-42 with it (two sampled textures), 45-46 as well only with `NVF_CLOUD_VOLUME` (four); the
    /// pipeline key's volume bit maps to the layout's only beside the sun visibility's; the fullest
    /// key holds them. The bind group and the shader def follow the same component field
    /// (`cloud_volume`), checked in the vendored text.
    #[test]
    fn the_weather_only_layout_leaves_out_bindings_45_and_46() {
        let entries = |key: MeshPipelineViewLayoutKey| bevy::pbr::nvf_sun_visibility_layout_entries(key).into_iter().map(|(b, e)| e.build(b, ShaderStages::FRAGMENT)).collect::<Vec<_>>();
        let bindings = |key| entries(key).iter().map(|e| e.binding).collect::<Vec<_>>();
        let textures = |key| entries(key).iter().filter(|e| counts_as_sampled(e)).count();
        let sun = MeshPipelineViewLayoutKey::NVF_SUN_VISIBILITY;
        let volume = MeshPipelineViewLayoutKey::NVF_CLOUD_VOLUME;
        assert_eq!(bindings(MeshPipelineViewLayoutKey::empty()), Vec::<u32>::new());
        assert_eq!(bindings(volume), Vec::<u32>::new(), "the volume bit alone adds nothing");
        assert_eq!(bindings(sun), [39, 40, 41, 42]);
        assert_eq!(bindings(sun | volume), [39, 40, 41, 42, 45, 46]);
        assert_eq!((textures(sun), textures(sun | volume)), (2, 4));
        let from = |k: MeshPipelineKey| MeshPipelineViewLayoutKey::from(k);
        assert!(from(MeshPipelineKey::NVF_SUN_VISIBILITY | MeshPipelineKey::NVF_CLOUD_VOLUME_VISIBILITY).contains(sun | volume));
        assert!(!from(MeshPipelineKey::NVF_SUN_VISIBILITY).contains(volume));
        assert!(!from(MeshPipelineKey::NVF_CLOUD_VOLUME_VISIBILITY).contains(volume), "not without the sun visibility");
        assert!(fullest_view_key().contains(sun | volume | MeshPipelineViewLayoutKey::NVF_AMBIENT_OCCLUSION | MeshPipelineViewLayoutKey::VIEW_TRANSMISSION_TEXTURE));
        // The key bits stay clear of the packed fields (bits 31 on) and the base mesh key's.
        assert_eq!(MeshPipelineKey::NVF_CLOUD_VOLUME_VISIBILITY.bits(), 1 << 29);
        let vendored = |p: &str| unix_lines(&std::fs::read_to_string(format!("{}/../vendor/bevy_pbr/src/render/{p}", env!("CARGO_MANIFEST_DIR"))).expect("vendored bevy_pbr"));
        let (mesh, view) = (vendored("mesh.rs"), vendored("mesh_view_bindings.rs"));
        assert!(mesh.contains("            if sun.cloud_volume {\n                view_key |= MeshPipelineKey::NVF_CLOUD_VOLUME_VISIBILITY;"));
        assert!(mesh.contains("            if key.contains(MeshPipelineKey::NVF_CLOUD_VOLUME_VISIBILITY) {\n                shader_defs.push(\"NVF_CLOUD_VOLUME\".into());"));
        assert!(view.contains("                if sun.cloud_volume {\n                    layout_key |= MeshPipelineViewLayoutKey::NVF_CLOUD_VOLUME;"));
        assert_eq!(view.matches("(45, &sun.light_near)").count(), 1, "bound in the one place, under cloud_volume");
    }

    /// The automatic source's gate when the volumetric sky is shown.
    const SHOWN: AutoGate = AutoGate { row: true, sky_shown: true, traced: true };

    /// #3032 stage 2i M2: `Auto` takes the light volume's shadows only while the
    /// volumetric sky is what the player sees: the sky shown (row on, buffer written whole, composing
    /// material ready: the flat sky hidden) and traced, and only the levels that are readable; otherwise the
    /// weather's layers. The aids `volume`, `share` and `depth` ask for the row alone.
    #[test]
    fn auto_source_follows_the_sky_the_player_sees() {
        let both = [true, true];
        let volume = CloudSourceInUse::Volume { near: true, far: true, only: false };
        assert_eq!(CloudSourceInUse::decide(true, CloudSource::Auto, both, SHOWN), volume, "all conditions: the volume");
        // Nothing traced, or the flat sky still showing (composing material not ready, a hand-back):
        // MediumOutput::ready false.
        let hidden = AutoGate { sky_shown: false, ..SHOWN };
        let untraced = AutoGate { traced: false, ..SHOWN };
        let neither = AutoGate { row: true, ..AutoGate::default() };
        for gate in [hidden, untraced, neither] {
            assert_eq!(CloudSourceInUse::decide(true, CloudSource::Auto, both, gate), CloudSourceInUse::Weather { why: WeatherWhy::SkyNotShown }, "{gate:?}");
        }
        // No front; one level of two.
        assert_eq!(CloudSourceInUse::decide(true, CloudSource::Auto, [false, false], SHOWN), CloudSourceInUse::Weather { why: WeatherWhy::NoFront });
        let far_only = CloudSourceInUse::decide(true, CloudSource::Auto, [false, true], SHOWN);
        assert_eq!(far_only, CloudSourceInUse::Volume { near: false, far: true, only: false }, "only the readable level");
        assert_eq!(far_only.levels(), [false, true]);
        // The aids ask for no sky shown.
        for gate in [hidden, untraced, neither] {
            assert_eq!(CloudSourceInUse::decide(true, CloudSource::Volume, both, gate), CloudSourceInUse::Volume { near: true, far: true, only: true });
            assert_eq!(CloudSourceInUse::decide(true, CloudSource::Share, both, gate), CloudSourceInUse::Debug { near: true, far: true, view: CloudSource::Share });
            assert_eq!(CloudSourceInUse::decide(true, CloudSource::Weather, both, gate), CloudSourceInUse::Weather { why: WeatherWhy::Asked });
        }
        // With the weather's layers the uniform is the packed one (the old picture, bit for bit).
        let packed = SunVisibilityUniform::pack(&SunVisibility { cloud: Some(checker_cloud(Handle::default())), ..default() });
        let window = LightVolumeWindow { floor: Vec4::new(21_024.0, 4.0, 1.0, 1.0), ..default() };
        let picked = CloudSourceInUse::decide(true, CloudSource::Auto, both, hidden);
        assert_eq!(with_cloud_volume(packed, picked, 0.6, window), packed, "the weather's layers: the old uniform");
        assert_eq!(picked.levels(), [false, false]);
        for why in [WeatherWhy::RowOff, WeatherWhy::SkyNotShown, WeatherWhy::NoFront] {
            assert!(CloudSourceInUse::Weather { why }.line().starts_with("the weather's layers ("), "{why:?}");
        }
    }

    /// The Volumetric clouds row off: the weather's layers whatever the aid asks, in the layout
    /// without the light volume's arrays, with nothing measured and no line in the log; a switch
    /// off names the layers once, and a later switch on what is read then.
    #[test]
    fn with_the_volumetric_clouds_off_the_cloud_shadows_are_the_weathers_alone() {
        let off = AutoGate { row: false, ..SHOWN };
        let row_off = CloudSourceInUse::Weather { why: WeatherWhy::RowOff };
        for asked in [CloudSource::Auto, CloudSource::Volume, CloudSource::Share, CloudSource::Depth] {
            let use_ = CloudSourceInUse::decide(true, asked, [true, true], off);
            assert_eq!((use_, use_.without_room(), use_.levels()), (row_off, row_off, [false; 2]), "{asked:?}");
        }
        assert!(!volume_layout(true, false, || panic!("the adapter is not measured with the row off")));
        assert!(!volume_layout(false, true, || panic!("nor with Cloud shadows off")));
        assert!(!volume_layout(true, true, || false) && volume_layout(true, true, || true));
        let volume = CloudSourceInUse::Volume { near: true, far: true, only: false };
        let mut told = None;
        let mut say = |now, wanted| source_told(&mut told, now, wanted);
        assert_eq!([say(row_off, false), say(CloudSourceInUse::Off, false), say(row_off, false)], [None; 3], "not asked for: no line");
        assert_eq!([say(volume, true), say(volume, true)], [Some(volume), None], "named once");
        assert_eq!([say(row_off, false), say(row_off, false), say(CloudSourceInUse::Off, false)], [Some(row_off), None, None], "the row off: once");
        assert_eq!([say(volume, true), say(CloudSourceInUse::Off, false), say(CloudSourceInUse::Off, false)], [Some(volume), Some(CloudSourceInUse::Off), None], "Cloud shadows off: once");
        assert!(volume_wanted(true, true) && !volume_wanted(true, false) && !volume_wanted(false, true));
    }

    /// The aid `weather` with the row on, off and on again: the source is the same throughout, so
    /// the row going off names nothing, but nothing is remembered while it is off (the frame's
    /// preparation then leaves the source alone) and the next activation names it again.
    #[test]
    fn a_forced_weather_source_is_forgotten_with_the_row_and_named_again() {
        let gate = |row| AutoGate { row, ..SHOWN };
        let mut told = None;
        for activation in 0..2 {
            let on = CloudSourceInUse::decide(true, CloudSource::Weather, [true, true], gate(true));
            assert_eq!([source_told(&mut told, on, true), source_told(&mut told, on, true)], [Some(on), None], "{activation}");
            let off = CloudSourceInUse::decide(true, CloudSource::Weather, [false; 2], gate(false));
            assert_eq!((off, source_told(&mut told, off, false)), (on, None), "{activation}: the same source");
            assert_eq!(told, None, "{activation}: nothing remembered while the row is off");
            assert_eq!(source_told(&mut told, off, false), None);
        }
    }

    /// The layout's own state: the stand-in is made once while the layout is on and goes with it,
    /// the views let go once in the frame it goes; when the last view goes, whatever was on (both
    /// rows off at once), nothing stays, so a later activation names its source again.
    #[test]
    fn the_volumes_layout_leaves_nothing_behind() {
        let volume = CloudSourceInUse::Volume { near: true, far: true, only: false };
        let mut made = 0;
        let mut layout = VolumeLayout::<u32>::default();
        let mut set = |layout: &mut VolumeLayout<u32>, on: bool| layout.set(on, || {
            made += 1;
            made
        });
        set(&mut layout, false);
        assert_eq!((layout.on, layout.stand_in, layout.release), (false, None, false), "off from the start: nothing to let go");
        for _ in 0..3 {
            set(&mut layout, true);
            assert_eq!((layout.on, layout.stand_in, layout.release), (true, Some(1), false));
        }
        set(&mut layout, false);
        assert_eq!((layout.on, layout.stand_in, std::mem::take(&mut layout.release)), (false, None, true), "let go once");
        set(&mut layout, false);
        assert!(!layout.release);
        // On again, its source named, and the last view gone with no frame of it off before.
        set(&mut layout, true);
        assert_eq!(source_told(&mut layout.told, volume, true), Some(volume));
        layout.leave();
        assert_eq!((layout.on, layout.stand_in, layout.release, layout.told), (false, None, false, None));
        set(&mut layout, true);
        assert_eq!((layout.stand_in, source_told(&mut layout.told, volume, true)), (Some(3), Some(volume)), "named again");
        // The extraction leaves it before anything returns early.
        let here = unix_lines(include_str!("sun_visibility.rs"));
        let extract = &here[here.find("\nfn extract_sun_visibility(").expect("the extraction")..];
        assert!(extract.find("gpu.volume.leave();").expect("left there") < extract.find("return;").expect("a return"));
    }

    /// The light volume is expected (the air asks for its variant's pipelines) whenever
    /// [`CloudSourceInUse::decide`] could give it with both fronts, and never with the cloud
    /// shadows off, the aid `weather`, the weather-only layout (the Volumetric clouds row off too), or
    /// (for `Auto`) nothing traced.
    #[test]
    fn the_volume_is_expected_wherever_it_can_become_the_source() {
        for asked in [CloudSource::Auto, CloudSource::Weather, CloudSource::Volume, CloudSource::Share, CloudSource::Depth] {
            for cloud_on in [false, true] {
                for (row, fits) in [(true, true), (true, false), (false, true), (false, false)] {
                    for traced in [false, true] {
                        let gate = AutoGate { row, sky_shown: true, traced };
                        let room = volume_layout(cloud_on, row, || fits);
                        let use_ = CloudSourceInUse::decide(cloud_on, asked, [true, true], gate);
                        let use_ = if room { use_ } else { use_.without_room() };
                        let reads = matches!(use_, CloudSourceInUse::Volume { .. } | CloudSourceInUse::Debug { .. });
                        assert_eq!(volume_expected(cloud_on, asked, traced, room), reads, "{asked:?} on {cloud_on} row {row} fits {fits} traced {traced}");
                    }
                }
            }
        }
        assert!(!volume_expected(true, CloudSource::Auto, false, true), "nothing traced");
        assert_eq!(CloudVolumeFrame::default().source, CloudSourceInUse::Off);
    }

    #[test]
    fn cloud_sources_parse() {
        assert_eq!(CloudSource::parse(" Weather "), Some(CloudSource::Weather));
        assert_eq!(CloudSource::parse("volume"), Some(CloudSource::Volume));
        assert_eq!(CloudSource::parse("auto"), Some(CloudSource::Auto));
        assert_eq!(CloudSource::parse(""), Some(CloudSource::Auto));
        assert_eq!(CloudSource::parse("both"), None);
        assert_eq!(crate::cloud_shadows::parse_source(None), CloudSource::Auto);
        assert_eq!(crate::cloud_shadows::parse_source(Some("weather")), CloudSource::Weather);
        assert_eq!(crate::cloud_shadows::parse_source(Some("sky")), CloudSource::Auto, "unknown: auto");
        assert_eq!(SunVisibility::default().cloud_source, CloudSource::Auto);
        assert!(CloudSourceInUse::Volume { near: true, far: false, only: false }.line().contains("near level"));
    }

    #[test]
    fn debug_modes_parse() {
        assert_eq!(DebugMode::parse("Cloud"), Some(DebugMode::Cloud));
        assert_eq!(DebugMode::parse(" terrain "), Some(DebugMode::Terrain));
        assert_eq!(DebugMode::parse("combined"), Some(DebugMode::Combined));
        assert_eq!(DebugMode::parse("off"), Some(DebugMode::Off));
        assert_eq!(DebugMode::parse("cascade"), None);
        assert!(SunVisibility { debug: DebugMode::Combined, ..default() }.active());
    }

    #[test]
    fn checker_cells_are_2000_units() {
        let image = checker_image();
        let cloud = checker_cloud(Handle::default());
        let data = image.data.as_ref().expect("pixels");
        let n = CHECKER_TEXELS as f32;
        // Texel centre of a world point (xz) through the cloud's uv, repeating.
        let at = |x: f32, z: f32| {
            let uv = Vec2::new(x, z) * cloud.uv_scale + cloud.uv_offset;
            let (u, v) = (((uv.x.rem_euclid(1.0)) * n) as usize, ((uv.y.rem_euclid(1.0)) * n) as usize);
            data[v * CHECKER_TEXELS as usize + u]
        };
        let c = CHECKER_CELL;
        assert_ne!(at(0.5 * c, 0.5 * c), at(1.5 * c, 0.5 * c));
        assert_ne!(at(0.5 * c, 0.5 * c), at(0.5 * c, 1.5 * c));
        assert_eq!(at(0.5 * c, 0.5 * c), at(1.5 * c, 1.5 * c));
        assert_eq!(at(0.1 * c, 0.1 * c), at(0.9 * c, 0.9 * c));
        assert_eq!((cloud.plane_height, cloud.strength), (CHECKER_PLANE, 1.0));
    }

    /// `source` with every `@if(...)` that names one of `known` resolved for those values
    /// (conjunctions only, as every condition this module writes): a statement whose
    /// condition is false is taken out with its attribute; a true one keeps the statement,
    /// its attribute keeping the other names. The statement is what follows the attribute
    /// (on its line or from the next) up to its `;` outside parentheses, its closing
    /// brace, or (a struct member) the comma that ends its line. Comments stay. Conditions
    /// naming none of `known` are left as they are.
    pub(crate) fn resolve_known(source: &str, known: &[(&str, bool)]) -> String {
        let lines: Vec<&str> = source.lines().collect();
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i];
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            let Some(rest) = trimmed.strip_prefix("@if(") else {
                out.push(line.to_string());
                i += 1;
                continue;
            };
            let mut depth = 1;
            let close = rest
                .char_indices()
                .find(|&(_, c)| {
                    match c {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    depth == 0
                })
                .map(|(at, _)| at)
                .unwrap_or_else(|| panic!("an unclosed @if: {line}"));
            let (cond, after) = (&rest[..close], rest[close + 1..].trim_start());
            if !known.iter().any(|(k, _)| cond.contains(k)) {
                out.push(line.to_string());
                i += 1;
                continue;
            }
            assert!(!cond.contains("||"), "conjunctions only: {cond}");
            let mut keep = true;
            let mut left = Vec::new();
            for lit in cond.split("&&").map(str::trim) {
                let (negated, name) = lit.strip_prefix('!').map_or((false, lit), |n| (true, n.trim()));
                match known.iter().find(|(k, _)| *k == name) {
                    Some((_, value)) => keep &= *value != negated,
                    None => left.push(lit),
                }
            }
            // The statement's last line.
            let first = if after.is_empty() { i + 1 } else { i };
            let (mut end, mut braces, mut parens) = (first, 0i32, 0i32);
            'scan: loop {
                assert!(end < lines.len(), "a statement without its end after {line}");
                let text = if end == i { after } else { lines[end] };
                for c in text.split("//").next().unwrap_or("").chars() {
                    match c {
                        '{' => braces += 1,
                        '}' => {
                            braces -= 1;
                            if braces == 0 {
                                break 'scan;
                            }
                        }
                        '(' => parens += 1,
                        ')' => parens -= 1,
                        ';' if braces == 0 && parens == 0 => break 'scan,
                        _ => {}
                    }
                }
                // A struct member ends at its line's closing comma.
                if braces == 0 && parens == 0 && text.split("//").next().unwrap_or("").trim_end().ends_with(',') {
                    break 'scan;
                }
                end += 1;
            }
            let attribute = (!left.is_empty()).then(|| format!("@if({})", left.join(" && ")));
            if keep {
                if first == i {
                    out.push(match &attribute {
                        Some(a) => format!("{indent}{a} {after}"),
                        None => format!("{indent}{after}"),
                    });
                    out.extend(lines[i + 1..=end].iter().map(|l| l.to_string()));
                } else {
                    if let Some(a) = &attribute {
                        out.push(format!("{indent}{a}"));
                    }
                    out.extend(lines[first..=end].iter().map(|l| l.to_string()));
                }
            }
            i = end + 1;
        }
        out.join("\n")
    }

    /// Brackets, braces and parentheses close in order (comments left out).
    pub(crate) fn balanced(text: &str) -> bool {
        let mut stack = Vec::new();
        for c in text.lines().flat_map(|l| l.split("//").next().unwrap_or("").chars()) {
            match c {
                '{' | '(' | '[' => stack.push(c),
                '}' if stack.pop() != Some('{') => return false,
                ')' if stack.pop() != Some('(') => return false,
                ']' if stack.pop() != Some('[') => return false,
                _ => {}
            }
        }
        stack.is_empty()
    }

    /// The block (braces included) of the first `{` after `start` in `text`.
    pub(crate) fn block_after<'a>(text: &'a str, start: &str) -> &'a str {
        let at = text.find(start).unwrap_or_else(|| panic!("{start} missing"));
        let open = at + text[at..].find('{').expect("a block");
        let mut depth = 0;
        let mut in_comment = false;
        let bytes = text.as_bytes();
        for (k, &b) in bytes.iter().enumerate().skip(open) {
            match b {
                b'\n' => in_comment = false,
                b'/' if !in_comment && bytes.get(k + 1) == Some(&b'/') => in_comment = true,
                b'{' if !in_comment => depth += 1,
                b'}' if !in_comment => {
                    depth -= 1;
                    if depth == 0 {
                        return &text[open..=k];
                    }
                }
                _ => {}
            }
        }
        panic!("{start}: the block does not close")
    }

    /// The text without its comments.
    pub(crate) fn code(text: &str) -> String {
        text.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n")
    }

    /// Code lines only (no blank or comment-only line), trimmed at the end.
    pub(crate) fn code_lines(text: &str) -> Vec<&str> {
        text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("//")).collect()
    }

    /// The whole patched `apply_pbr_lighting` (lights.rs `DISPLAY_EDITS`, then
    /// [`HOOK_EDITS`]) in each combination of the shader defs it is written against
    /// (`NVF_SUN_VISIBILITY` on and off, with `DISTANCE_FOG` on, off or left to the
    /// pipeline): every anchor is found, every condition this module wrote resolves,
    /// brackets close in the file, in the function and in the directional light loop, and
    /// what the hook uses is declared before it. Without the def the code lines are the
    /// display-value patch's exactly. What this cannot show (that naga accepts the module
    /// with bevy_pbr's imports resolved, for every other def of a real pipeline) only a
    /// run does: no unit test here resolves bevy_pbr's WESL imports.
    #[test]
    fn patched_lighting_resolves_in_every_variant() {
        let (display, hooked) = hooked();
        assert!(balanced(&display) && balanced(&hooked), "the patched sources close");
        for (_, text, _) in HOOK_EDITS {
            assert!(balanced(text), "each edit's text closes by itself: {text}");
        }
        for fog in [None, Some(false), Some(true)] {
            for on in [false, true] {
                let mut known = vec![("NVF_SUN_VISIBILITY", on)];
                if let Some(f) = fog {
                    known.push(("DISTANCE_FOG", f));
                }
                let r = resolve_known(&hooked, &known);
                let case = format!("NVF_SUN_VISIBILITY {on}, DISTANCE_FOG {fog:?}");
                assert!(!code(&r).contains("NVF_SUN_VISIBILITY"), "{case}: every condition resolved");
                assert!(balanced(&r), "{case}: brackets close");
                let lighting = block_after(&r, "fn nvf_apply_pbr_lighting(");
                assert!(!lighting.contains("\nfn "), "{case}: the lighting function ends where it should");
                let post = block_after(&r, "fn main_pass_post_lighting_processing(");
                assert!(!post.contains("\nfn "), "{case}");
                let fogs = post.matches("apply_fog(").count();
                assert_eq!(fogs, usize::from(fog != Some(false)), "{case}: the fog once, or not at all without DISTANCE_FOG");
                if !on {
                    // Exactly the display-value patch, line for line (comments aside).
                    let want = resolve_known(&display, &known[1..]);
                    assert_eq!(code_lines(&r), code_lines(&want), "{case}");
                    assert!(!code(&r).contains("nvf_sun_"), "{case}");
                    continue;
                }
                let at = |s: &str| r.find(s).unwrap_or_else(|| panic!("{case}: {s} missing"));
                assert!(at("import nvforever::sun_visibility::{nvf_sun_visibility, nvf_sun_visibility_parts, nvf_sun_debug_value, nvf_sun_debug_mode};") < at("fn nvf_apply_pbr_lighting("));
                let wesl = include_str!("sun_visibility.wesl");
                for f in ["fn nvf_sun_visibility(", "fn nvf_sun_visibility_parts(", "fn nvf_sun_debug_value(", "fn nvf_sun_debug_mode("] {
                    assert!(wesl.contains(f), "sun_visibility.wesl has {f}");
                }
                // Declared before use, in the function.
                let (decl, set, shown) = (lighting.find("var nvf_sun_debug = 1.0;"), lighting.find("nvf_sun_debug = nvf_sun_debug_value("), lighting.find("vec3<f32>(nvf_sun_debug)"));
                assert!(decl.is_some() && decl < set && set < shown, "{case}: nvf_sun_debug declared, set, shown");
                let view_z = lighting.find("let view_z").expect("view_z");
                assert!(view_z < lighting.find("nvf_sun_visibility_parts(").expect("the hook"), "{case}: view_z before the hook");
                // The two hooks inside the directional light loop, each after its shadow's
                // declaration; the loop still closes before the function's end.
                let directional = block_after(lighting, "for (var i: u32 = 0u; i < n_directional_lights; i = i + 1u) {");
                let d = |s: &str| directional.find(s).unwrap_or_else(|| panic!("{case}: {s} not in the directional loop"));
                assert!(d("var shadow: f32 = 1.0;") < d("shadow = nvf_parts.w;"));
                assert!(d("shadow = nvf_parts.w;") < d("var light_contrib = lighting::directional_light("));
                assert!(d("var transmitted_shadow: f32 = 1.0;") < d("transmitted_shadow = nvf_sun_visibility("));
                assert_eq!(directional.matches("DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT").count(), 2, "{case}: both hooks test the sun's flag");
                assert_eq!(lighting.matches("nvf_sun_visibility_parts(").count(), 1);
                if fog != Some(false) {
                    assert!(post.contains("nvf_sun_debug_mode() == 0u"), "{case}: no fog over the debug view");
                }
            }
        }
        // The flag the hooks test is bevy_pbr's.
        let types = unix_lines(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../vendor/bevy_pbr/src/render/mesh_view_types.wesl")).expect("mesh_view_types.wesl"));
        assert!(types.contains("const DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT: u32"));
    }

    /// water.wesl's highlight and nif_material.wesl's shade-light algebra (#2670 C2) with
    /// the sun visibility's def on and off: the conditions resolve, brackets close, the
    /// imports name what sun_visibility.wesl and bevy_pbr have, and without the def
    /// nothing of the plumbing is left (water: light 0 and a factor of 1; arms: the
    /// cascade value itself). Neither shader is compiled here (bevy_pbr's imports are not
    /// resolved by any unit test): that they compile is for a run to show.
    #[test]
    fn water_and_arms_edits_resolve() {
        let read = |p: &str| unix_lines(&std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{p}: {e}")));
        let water = read(concat!(env!("CARGO_MANIFEST_DIR"), "/src/water.wesl"));
        let arms = read(concat!(env!("CARGO_MANIFEST_DIR"), "/../nvf-assets/src/nif_material.wesl"));
        let wesl = include_str!("sun_visibility.wesl");
        assert!(wesl.contains("fn nvf_sun_visibility(P: vec3<f32>, L: vec3<f32>, cascade_shadow: f32, view_z: f32) -> f32"));
        for on in [false, true] {
            let known = [("NVF_SUN_VISIBILITY", on)];
            let (w, a) = (resolve_known(&water, &known), resolve_known(&arms, &known));
            for (name, text) in [("water.wesl", &code(&w)), ("nif_material.wesl", &code(&a))] {
                assert!(!text.contains("NVF_SUN_VISIBILITY"), "{name} {on}: every condition resolved");
                assert!(balanced(text), "{name} {on}: brackets close");
                assert_eq!(text.contains("nvf_sun_visibility("), on, "{name} {on}");
                assert_eq!(text.contains("DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT"), on, "{name} {on}");
            }
            // Water: the highlight is light 0 times 1 without the def; with it, the flagged
            // light's, times its visibility once, the reflection image untouched.
            let highlight = block_after(&w, "if lights.n_directional_lights > 0u {");
            assert!(highlight.contains("let sun = lights.directional_lights[sun_index];"));
            assert!(highlight.contains("reflected += spec * sun_light * sun_visibility;"));
            assert_eq!(highlight.contains("sun_visibility = nvf_sun_visibility(pos, sun.direction_to_light, 1.0, view_z);"), on);
            assert_eq!(w.matches("* sun_visibility").count(), 1, "the highlight alone takes it");
            assert_eq!(w.contains("import nvforever::sun_visibility::nvf_sun_visibility;"), on);
            // Arms: the cascade value itself without the def; with it, V for the flagged sun.
            let value = block_after(&a, "fn sun_shadow_value(");
            assert_eq!(code_lines(value).len() == 3, !on, "{value}");
            assert_eq!(value.contains("return nvf_sun_visibility(in.world_position.xyz, lights.directional_lights[sun].direction_to_light, cascade, view_z);"), on);
            let shade = block_after(&a, "fn with_shade_light(");
            for s in ["let v_lit = sun_shadow_value(sun, in, 1.0, view_z);", "let v_own = sun_shadow_value(sun, in, own, view_z);", "let missing = (v_lit - v_own) + share * (1.0 - shade);", "let k = (v_lit + share - sun_shadow_value(sun, in, wanted, view_z)) / missing;"] {
                assert!(shade.contains(s), "{s}");
            }
            assert_eq!(a.contains("@if(!PREPASS_PIPELINE && SHADE_LIGHT) import nvforever::sun_visibility::nvf_sun_visibility;"), on);
        }
    }

    /// #3032 stage 2i M2: the real receivers composed as
    /// the pipeline composes them. bevy_pbr's `pbr_functions` takes lights.rs's display-value
    /// patch and then [`HOOK_EDITS`] (lights.rs's order), every vendored bevy_pbr and bevy_render
    /// module, nvforever's and nvf_assets' are registered under their `embedded://` paths in
    /// Bevy's own shader cache, which resolves the WESL imports (the hook's import of
    /// `nvforever::sun_visibility`, its import of `nvforever::light_volume_reader`), and Naga
    /// parses and validates each composed module: Bevy's StandardMaterial fragment, the NIF
    /// material (with the shade light, which imports the sun visibility itself) and the terrain,
    /// with the sun visibility's defs and `NVF_CLOUD_VOLUME` on and off. With it the module binds
    /// 45 and 46 (group 0); without it, the weather-only layout's, neither. What this cannot show:
    /// the pipeline layout's limits on a device (that is `CloudVolumeRoom`'s), and the other defs
    /// of every real pipeline (the base defs here are nvf-shader-tests' representative set).
    #[test]
    fn receivers_compose_with_the_reader_as_the_pipeline_does() {
        use bevy::asset::Assets;
        use bevy::shader::{ShaderCache, ShaderCacheSource, ShaderDefVal};
        use std::collections::HashMap;
        use wgpu::naga;
        struct Compiled(naga::Module);
        let mut cache: ShaderCache<Compiled, ()> = ShaderCache::new((), |_, source, _| {
            let ShaderCacheSource::Wgsl(wgsl) = source else { panic!("WESL must compose to WGSL") };
            let module = naga::front::wgsl::parse_str(&wgsl).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&wgsl)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&wgsl)));
            Ok(Compiled(module))
        });
        let mut assets = Assets::<Shader>::default();
        let mut ids = HashMap::new();
        let engine = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("engine").to_path_buf();
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let mut paths: Vec<_> = std::fs::read_dir(dir).expect("shader directory").map(|e| e.expect("entry").path()).collect();
            paths.sort();
            for p in paths {
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "wesl") {
                    out.push(p);
                }
            }
        }
        for (package, dir) in [("bevy_pbr", "vendor/bevy_pbr/src"), ("bevy_render", "vendor/bevy_render/src"), ("nvforever", "nvforever/src"), ("nvf_assets", "nvf-assets/src")] {
            let root = engine.join(dir);
            let mut files = Vec::new();
            walk(&root, &mut files);
            for path in files {
                let name = format!("{package}/{}", path.strip_prefix(&root).expect("under the root").to_str().expect("utf-8").replace('\\', "/"));
                let mut source = std::fs::read_to_string(&path).expect("shader source");
                if name == "bevy_pbr/render/pbr_functions.wesl" {
                    let display = crate::lights::display_patched(&unix_lines(&source)).expect("the display patch applies");
                    source = hook_patched(&display).expect("the sun visibility's hook applies");
                }
                let shader = Shader::from_wesl(source, format!("embedded://{name}"));
                let id = assets.add(shader.clone()).id();
                cache.set_shader(id, shader);
                ids.insert(name, id);
            }
        }
        let base = ["VERTEX_POSITIONS", "VERTEX_NORMALS", "VERTEX_TANGENTS", "VERTEX_UVS", "VERTEX_UVS_A", "VERTEX_UVS_B", "VERTEX_COLORS", "VERTEX_OUTPUT_INSTANCE_INDEX", "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3", "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_6"];
        let binds = |m: &naga::Module, binding: u32| m.global_variables.iter().any(|(_, g)| g.binding.as_ref().is_some_and(|b| b.group == 0 && b.binding == binding));
        let mut pipeline = 0;
        for (shader, extra) in [
            ("bevy_pbr/render/pbr.wesl", &[][..]),
            ("nvf_assets/nif_material.wesl", &[][..]),
            ("nvf_assets/nif_material.wesl", &["FIRST_PERSON_DEPTH", "SHADE_LIGHT"][..]),
            ("nvforever/terrain.wesl", &[][..]),
        ] {
            for volume in [false, true] {
                let mut defs: Vec<ShaderDefVal> = base.iter().chain(extra).copied().map(ShaderDefVal::from).collect();
                defs.extend(["NVF_SUN_VISIBILITY", "NVF_CLOUD_VISIBILITY", "NVF_TERRAIN_VISIBILITY"].map(ShaderDefVal::from));
                if volume {
                    defs.push("NVF_CLOUD_VOLUME".into());
                }
                defs.extend([
                    ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 3),
                    ShaderDefVal::UInt("MAX_CASCADES_PER_LIGHT".into(), 4),
                    ShaderDefVal::UInt("MAX_DIRECTIONAL_LIGHTS".into(), 10),
                    ShaderDefVal::UInt("MAX_RECT_LIGHTS".into(), 1),
                ]);
                pipeline += 1;
                let id = *ids.get(shader).unwrap_or_else(|| panic!("{shader} registered"));
                let compiled = cache.get(pipeline, id, &defs).unwrap_or_else(|e| panic!("{shader} {extra:?} volume {volume}: {e}"));
                let m = &compiled.0;
                let case = format!("{shader} {extra:?} volume {volume}");
                assert!(binds(m, 39), "{case}: the sun visibility's uniform");
                assert!(binds(m, 40) && binds(m, 41), "{case}: the cloud and terrain maps");
                assert_eq!(binds(m, 45), volume, "{case}: binding 45");
                assert_eq!(binds(m, 46), volume, "{case}: binding 46");
                assert!(m.entry_points.iter().any(|e| e.stage == naga::ShaderStage::Fragment), "{case}: a fragment entry point");
            }
        }
        // The light in the air's grid (a compute pipeline of its own): the flat variant has its
        // bindings 0-7 alone, the light volume's links the shared reader and adds 8-11 and the
        // mean entry.
        // Each in every walk of the columns, with its workgroup.
        use crate::medium_air_grid::GridWalk;
        for (volume, walk) in [false, true].into_iter().flat_map(|v| GridWalk::ALL.map(|w| (v, w))) {
            let mut defs: Vec<ShaderDefVal> = if volume { vec![crate::medium_air_grid::AIR_VOLUME_DEF.into()] } else { Vec::new() };
            defs.extend(walk.defs().iter().map(|d| ShaderDefVal::from(*d)));
            defs.extend([ShaderDefVal::UInt("MAX_CASCADES_PER_LIGHT".into(), 4), ShaderDefVal::UInt("MAX_DIRECTIONAL_LIGHTS".into(), 10), ShaderDefVal::UInt("MAX_RECT_LIGHTS".into(), 8)]);
            defs.extend(["AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3", "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_6"].map(ShaderDefVal::from));
            pipeline += 1;
            let id = *ids.get("nvforever/medium_air_grid.wesl").expect("the air's grid registered");
            let compiled = cache.get(pipeline, id, &defs).unwrap_or_else(|e| panic!("the air's grid, volume {volume}, {walk:?}: {e}"));
            let m = &compiled.0;
            for binding in 0..8 {
                assert!(binds(m, binding), "the air's grid, volume {volume}: binding {binding}");
            }
            for binding in 8..12 {
                assert_eq!(binds(m, binding), volume, "the air's grid, volume {volume}: binding {binding}");
            }
            assert!(!binds(m, 45) && !binds(m, 46), "none of the surfaces' bindings");
            let entry = |name: &str| m.entry_points.iter().any(|e| e.stage == naga::ShaderStage::Compute && e.name == name);
            assert!(entry("air_grid"), "volume {volume}");
            let chunks = if walk == GridWalk::Serial { 1 } else { 4 };
            assert_eq!(m.entry_points.iter().find(|e| e.name == "air_grid").map(|e| e.workgroup_size), Some([8, 8, chunks]), "volume {volume}, {walk:?}");
            assert_eq!(entry("air_cloud_mean"), volume);
        }
    }

    /// sun_visibility.wesl `source` for the shader defs (cloud, terrain, cloud volume) as one
    /// module: every condition resolved ([`resolve_known`]) and its one import, the light volume's
    /// shared reader (#3032 stage 2i M2, only with `NVF_CLOUD_VOLUME`), linked by putting that
    /// module's text in its place (it declares nothing the sun visibility declares).
    pub(crate) fn sun_variant(source: &str, (cloud, terrain, volume): (bool, bool, bool)) -> String {
        const IMPORT: &str = "import nvforever::light_volume_reader::{LightVolumeWindow, lvr_optical_depth};";
        let reader = include_str!("light_volume_reader.wesl").replace("\r\n", "\n");
        let known = [("NVF_CLOUD_VISIBILITY", cloud), ("NVF_TERRAIN_VISIBILITY", terrain), ("NVF_CLOUD_VOLUME", volume)];
        let resolved = resolve_known(source, &known);
        assert!(!code(&resolved).contains("@if("), "{cloud} {terrain} {volume}: every condition resolved");
        assert_eq!(resolved.lines().filter(|l| l.trim_start().starts_with("import ")).count(), usize::from(volume), "{cloud} {terrain} {volume}: the reader's import only with the volume");
        resolved.lines().map(|l| if l.trim() == IMPORT { reader.clone() } else { l.to_string() }).collect::<Vec<_>>().join("\n")
    }

    /// sun_visibility.wesl in each of its eight variants (cloud, terrain, cloud volume on or off;
    /// the game binds the volume only with the cloud variant, the others are valid too) is valid
    /// WGSL. Without `NVF_CLOUD_VOLUME` (the weather-only layout) nothing names
    /// bindings 45-46, the window or the reader, and K is the weather's.
    #[test]
    fn shader_variants_are_valid_wgsl() {
        use wgpu::naga;
        let source = include_str!("sun_visibility.wesl").replace("\r\n", "\n");
        assert_eq!(source.lines().filter(|l| l.contains("import ") && !l.trim_start().starts_with("//")).count(), 1, "the one import");
        for cloud in [false, true] {
            for terrain in [false, true] {
                for volume in [false, true] {
                    let out = sun_variant(&source, (cloud, terrain, volume));
                    let case = format!("cloud {cloud} terrain {terrain} volume {volume}");
                    let module = naga::front::wgsl::parse_str(&out).unwrap_or_else(|e| panic!("{case}: {}", e.emit_to_string(&out)));
                    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{case}: {e:?}"));
                    assert_eq!(out.matches("fn nvf_cloud_transmittance(").count(), 1, "{case}");
                    assert_eq!(out.matches("fn nvf_terrain_visibility(").count(), 1);
                    assert_eq!(out.matches("fn nvf_solid_visibility(").count(), 1);
                    // The terrain variant reads the map with loads (four), the others never
                    // touch it; the cloud side is untouched by it.
                    assert_eq!(out.matches("textureLoad(nvf_terrain_texture").count(), if terrain { 4 } else { 0 }, "{case}");
                    assert_eq!(out.matches("textureSampleLevel(nvf_cloud_texture").count(), usize::from(cloud), "{case}");
                    assert_eq!(out.matches("fn nvf_cloud_weather(").count(), usize::from(cloud), "{case}");
                    // The light volume: declared with the volume def only, read by the cloud
                    // variant through the shared reader.
                    let declared = |s: &str| code(&out).contains(s);
                    for s in ["@binding(45)", "@binding(46)", "    volume: vec4<f32>,", "light_volume: LightVolumeWindow", "fn lvr_optical_depth("] {
                        assert_eq!(declared(s), volume, "{case}: {s}");
                    }
                    assert_eq!(out.matches("lvr_optical_depth(nvf_light_near, nvf_light_far, nvf_sun_visibility_sampler, nvf_sun.light_volume, P, L)").count(), usize::from(cloud && volume), "{case}");
                    if cloud && !volume {
                        let k = block_after(&out, "fn nvf_cloud_transmittance(P: vec3<f32>, L: vec3<f32>) -> f32");
                        assert_eq!(code_lines(k), ["{", "    return nvf_cloud_weather(P, L);", "}"], "{case}: the weather's K");
                    }
                }
            }
        }
        // The receiver's constants are the CPU mirror's, word for word.
        for (name, value) in [
            ("PENUMBRA", TERRAIN_PENUMBRA),
            ("SOFT_MIN", TERRAIN_SOFT_MIN),
            ("SOFT_TEXELS", TERRAIN_SOFT_TEXELS),
            ("TAN_MAX", TERRAIN_TAN_MAX),
            ("BIAS_RELIEF", TERRAIN_BIAS_RELIEF),
            ("BIAS_MIN", TERRAIN_BIAS_MIN),
            ("EDGE_TEXELS", TERRAIN_EDGE_TEXELS),
        ] {
            let line = format!("const NVF_TERRAIN_{name}: f32 = {value:?};");
            assert!(source.contains(&line), "{line}");
        }
    }

    use crate::sun_heightfield::TEXEL;

    /// Game axes (x east, y north, z up) to Bevy's (x, y up, z south).
    fn bevy_axes(g: Vec3) -> Vec3 {
        Vec3::new(g.x, g.z, -g.y)
    }

    /// Towards the light at `elevation` and `azimuth` (radians, clockwise from north), game axes.
    fn toward_game(elevation: f32, azimuth: f32) -> Vec3 {
        Vec3::new(azimuth.sin() * elevation.cos(), azimuth.cos() * elevation.cos(), elevation.sin())
    }

    /// A terrain map as the GPU pass writes it (terrain_shadows.rs `produce_output`, its CPU
    /// mirror), `side` texels with its south-west corner at game (0, 0), and the uniform the
    /// plumbing packs for it.
    struct TestMap {
        out: Vec<Vec4>,
        side: usize,
        u: SunVisibilityUniform,
    }

    impl TestMap {
        /// From heights (row-major, row 0 the southernmost; texel maxima as T0 keeps them).
        fn produce(heights: &[f32], valid: &[u8], side: usize, light: Vec3) -> Self {
            let (lo, hi) = heights.iter().zip(valid).filter(|(_, &v)| v != 0).fold((f32::MAX, f32::MIN), |(lo, hi), (&h, _)| (lo.min(h), hi.max(h)));
            let p = crate::terrain_shadows::march_params(light, TEXEL, side as u32, lo, hi).expect("a light above the horizon");
            Self::from_output(crate::terrain_shadows::produce_output(heights, valid, &p), side)
        }

        fn from_output(out: Vec<Vec4>, side: usize) -> Self {
            let (uv_scale, uv_offset) = crate::terrain_shadows::TerrainShadows::uv_transform(Vec2::ZERO, TEXEL, side as u32);
            let terrain = TerrainVisibility { image: Handle::default(), uv_scale, uv_offset, texel: TEXEL, side: side as u32 };
            let u = SunVisibilityUniform::pack(&SunVisibility { terrain_enabled: true, terrain: Some(terrain), ..default() });
            Self { out, side, u }
        }

        /// T at game (`x`, `y`) and height `z`, the light towards `light` (game axes).
        fn t(&self, x: f32, y: f32, z: f32, light: Vec3) -> f32 {
            let load = |c: IVec2| {
                assert!(c.x >= 0 && c.y >= 0 && (c.x as usize) < self.side && (c.y as usize) < self.side, "a load outside the map: {c}");
                self.out[c.y as usize * self.side + c.x as usize]
            };
            terrain_visibility(&self.u, load, bevy_axes(Vec3::new(x, y, z)), bevy_axes(light.normalize()))
        }
    }

    /// Planes facing the light at slopes up to 45 degrees, from many directions, under a
    /// sun at 5, 13 (about 07:00) and 30 degrees: never shaded in truth. The map alone
    /// (each texel its highest corner) puts S above the ground on many of them, so without
    /// the bias they would speckle; with it every receiver on the ground is fully lit.
    #[test]
    fn lit_slope_has_no_acne() {
        let side = 32;
        let (mut cases, mut raw_over) = (0, 0);
        for elevation in [5.0f32, 13.0, 30.0] {
            for azimuth in [0.0f32, 30.0, 90.0, 135.0, 200.0, 270.0] {
                let light = toward_game(elevation.to_radians(), azimuth.to_radians());
                for g in [0.05f32, 0.2, 0.5, 1.0] {
                    for dir in (0..360).step_by(45) {
                        let (a, b) = (g * (dir as f32).to_radians().sin(), g * (dir as f32).to_radians().cos());
                        // Facing the light, with a margin: lit in truth.
                        if Vec3::new(-a, -b, 1.0).normalize().dot(light) < 0.05 {
                            continue;
                        }
                        cases += 1;
                        let plane = |x: f32, y: f32| a * x + b * y;
                        let heights: Vec<f32> = (0..side * side)
                            .map(|i| {
                                let (c, r) = ((i % side) as f32, (i / side) as f32);
                                [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].iter().map(|(dx, dy)| plane((c + dx) * TEXEL, (r + dy) * TEXEL)).fold(f32::MIN, f32::max)
                            })
                            .collect();
                        let map = TestMap::produce(&heights, &vec![1; side * side], side, light);
                        let mut x = 8.0f32;
                        while x < 24.0 {
                            let mut y = 8.0f32;
                            while y < 24.0 {
                                let (gx, gy) = (x * TEXEL, y * TEXEL);
                                let ground = plane(gx, gy);
                                let t = map.t(gx, gy, ground, light);
                                assert!(t > 0.9999, "acne: T {t} at texel ({x}, {y}) of a plane rising {g} towards {dir} degrees, sun {elevation} degrees at {azimuth}");
                                let own = map.out[y as usize * side + x as usize].x;
                                raw_over += usize::from(ground < own);
                                y += 0.41;
                            }
                            x += 0.37;
                        }
                    }
                }
            }
        }
        assert!(cases > 300, "{cases}");
        assert!(raw_over > 10_000, "the map alone would shade the ground on the slopes ({raw_over} samples): the bias is what lights them");
    }

    /// A flat-topped ridge 1,000 units high, the sun low in the west (tan 0.2): the valley
    /// east of it is shaded from the ridge's foot (the cliff's foot gets no bias) to about
    /// its shadow's length (1,000 / 0.2 = 5,000 units, 19.5 texels), lit beyond and west of it.
    #[test]
    fn valley_behind_ridge_is_shaded() {
        let side = 64;
        let heights: Vec<f32> = (0..side * side).map(|i| if (20..=23).contains(&(i % side)) { 1_000.0 } else { 0.0 }).collect();
        let light = toward_game(0.2f32.atan(), -std::f32::consts::FRAC_PI_2);
        let map = TestMap::produce(&heights, &vec![1; side * side], side, light);
        let row = 32.3 * TEXEL;
        let t = |x: f32| map.t(x * TEXEL, row, 0.0, light);
        for x in [10.0, 19.0] {
            assert_eq!(t(x), 1.0, "west of the ridge, x {x}");
        }
        for x in [24.2, 24.8, 26.0, 30.0, 35.0, 40.0] {
            assert!(t(x) < 0.01, "behind the ridge, x {x}: {}", t(x));
        }
        for x in [44.0, 46.0, 50.0, 55.0] {
            assert!(t(x) > 0.99, "past the shadow's end, x {x}: {}", t(x));
        }
        // The end is soft and monotone: no stair step from the 256-unit grid.
        let mut last = t(40.0);
        let mut x = 40.0;
        while x < 44.0 {
            x += 0.05;
            let now = t(x);
            assert!(now >= last - 1e-5, "x {x}: {now} after {last}");
            assert!(now - last < 0.1, "x {x}: a step of {} over 0.05 texel", now - last);
            last = now;
        }
        // A receiver up on the ridge top is lit.
        assert_eq!(map.t(21.5 * TEXEL, row, 1_000.0, light), 1.0);
    }

    /// The map's last texels fade T to 1, and outside it T is 1 (no load outside the map).
    #[test]
    fn map_edge_fades_out() {
        let side = 32;
        let map = TestMap::from_output(vec![Vec4::new(10_000.0, 1_000.0, 0.0, 0.0); side * side], side);
        let light = toward_game(0.3, 1.0);
        let mid = 16.0 * TEXEL;
        let t = |x: f32, y: f32| map.t(x * TEXEL, y, 0.0, light);
        assert_eq!(t(16.0, mid), 0.0, "inside, under S everywhere");
        assert_eq!(t(TERRAIN_EDGE_TEXELS, mid), 0.0, "whole from the fade's width on");
        assert!((t(0.5 * TERRAIN_EDGE_TEXELS, mid) - 0.5).abs() < 1e-4, "half way: {}", t(0.5 * TERRAIN_EDGE_TEXELS, mid));
        for x in [0.0, -0.5, -100.0, 32.0, 40.0, 1.0e6] {
            assert_eq!(t(x, mid), 1.0, "x {x}: on or outside the edge");
        }
        assert_eq!(map.t(mid, -3.0 * TEXEL, 0.0, light), 1.0, "south of the map");
        assert_eq!(map.t(mid, 33.0 * TEXEL, 0.0, light), 1.0, "north of the map");
        let mut last = 1.0;
        for k in 0..=40 {
            let now = t(k as f32 * 0.1, mid);
            assert!(now <= last + 1e-6, "{k}: {now} after {last}");
            last = now;
        }
    }

    /// Texels with no terrain (no LAND, not loaded) hold S = -1e30 from the producer: a
    /// receiver over them is lit, in a ridge's shadow too, and the hole casts nothing.
    #[test]
    fn invalid_texels_shade_nothing() {
        let side = 64;
        let mut heights: Vec<f32> = (0..side * side).map(|i| if (10..=13).contains(&(i % side)) { 1_000.0 } else { 0.0 }).collect();
        let mut valid = vec![1u8; side * side];
        for row in 20..=30 {
            for col in 20..=30 {
                heights[row * side + col] = crate::sun_heightfield::INVALID_HEIGHT;
                valid[row * side + col] = 0;
            }
        }
        let light = toward_game(0.2f32.atan(), -std::f32::consts::FRAC_PI_2);
        let map = TestMap::produce(&heights, &valid, side, light);
        for (x, y) in [(22.0, 22.0), (25.5, 25.5), (29.4, 29.4)] {
            assert_eq!(map.t(x * TEXEL, y * TEXEL, 0.0, light), 1.0, "over the hole ({x}, {y})");
        }
        for (x, y) in [(25.0, 10.0), (25.0, 40.0)] {
            assert!(map.t(x * TEXEL, y * TEXEL, 0.0, light) < 0.01, "the ridge's shadow beside the hole ({x}, {y})");
        }
        // Switched on with nothing published: T = 1 before any load.
        let neutral = SunVisibilityUniform::pack(&SunVisibility { terrain_enabled: true, ..default() });
        let never = |c: IVec2| -> Vec4 { panic!("a load with no map published: {c}") };
        assert_eq!(terrain_visibility(&neutral, never, Vec3::new(25.0 * TEXEL, 0.0, -25.0 * TEXEL), bevy_axes(light.normalize())), 1.0);
    }

    /// Vsolid with terrain on: min(C, T), the darker of the two, with no blend by distance
    /// (the maintainer's decision on #2920): an object's cascade shadow stays whole up to the
    /// cascades' end, where Bevy's C is 1 and T alone counts; with sun shadows off (C = 1) T
    /// alone; switched on with nothing published, C exactly. The uniform's band words are 0.
    #[test]
    fn terrain_combine_is_the_darker_of_the_two() {
        let map = TestMap::from_output(vec![Vec4::ZERO; 4], 2);
        let u = map.u;
        assert_eq!(u.mode.x & MASK_TERRAIN, MASK_TERRAIN);
        assert_eq!((u.terrain.x, u.terrain.y), (0.0, 0.0));
        // The darker once, never the product.
        assert_eq!(solid_visibility(&u, 0.5, 0.2), 0.2);
        assert_eq!(solid_visibility(&u, 0.1, 0.2), 0.1);
        // An object's cascade shadow (C 0) on lit terrain stays whole: no fade-out.
        assert_eq!(solid_visibility(&u, 0.0, 1.0), 0.0);
        // Past the cascades (C 1) or with sun shadows off: T.
        assert_eq!(solid_visibility(&u, 1.0, 0.3), 0.3);
        // Switched on, nothing published: the cascades as they were.
        let neutral = SunVisibilityUniform::pack(&SunVisibility { terrain_enabled: true, ..default() });
        assert_eq!(solid_visibility(&neutral, 0.4, 0.0), 0.4);
    }

    /// Switched on with nothing published: the plumbing is on and the views take the
    /// terrain variant (the key bits follow the switch), the terrain fields are 0 and the
    /// mask bit clear (T = 1, Vsolid = C); a publication fills them.
    #[test]
    fn switched_on_without_a_map_is_neutral() {
        let on = SunVisibility { terrain_enabled: true, ..default() };
        assert!(on.active() && on.terrain_on() && !on.cloud_on());
        assert_eq!(on.mask(), (false, true, DebugMode::Off), "the log names the switch");
        let u = SunVisibilityUniform::pack(&on);
        assert_eq!((u.terrain_uv, u.terrain, u.mode.x & MASK_TERRAIN), (Vec4::ZERO, Vec4::ZERO, 0));
        let map = TestMap::from_output(vec![Vec4::ZERO; 4], 2);
        assert_eq!(map.u.mode.x & MASK_TERRAIN, MASK_TERRAIN);
        assert_eq!(map.u.terrain.z, TEXEL);
        assert!(!SunVisibility::default().terrain_on());
    }

    /// Review of f8d532cca, finding 5: a published map whose GPU image is not uploaded this
    /// frame (the one-texel stand-in bound) must not be read as a 512-texel map. The written
    /// uniform's terrain bit is cleared: T = 1 before any load and Vsolid = C at every
    /// distance (no terrain without a usable map); the cloud's part is untouched.
    #[test]
    fn terrain_bit_needs_an_uploaded_map() {
        let (uv_scale, uv_offset) = crate::terrain_shadows::TerrainShadows::uv_transform(Vec2::ZERO, TEXEL, 512);
        let terrain = TerrainVisibility { image: Handle::default(), uv_scale, uv_offset, texel: TEXEL, side: 512 };
        let cloud = checker_cloud(Handle::default());
        let packed = SunVisibilityUniform::pack(&SunVisibility { terrain_enabled: true, terrain: Some(terrain), cloud_enabled: true, cloud: Some(cloud), ..default() });
        assert_eq!(packed.mode.x, MASK_TERRAIN | MASK_CLOUD);
        // Uploaded: as packed.
        assert_eq!(usable_uniform(packed, true), packed);
        // Not uploaded: the terrain bit alone goes.
        let u = usable_uniform(packed, false);
        assert_eq!(u.mode.x, MASK_CLOUD);
        assert_eq!((u.cloud_uv, u.cloud, u.mode.y), (packed.cloud_uv, packed.cloud, packed.mode.y));
        let never = |c: IVec2| -> Vec4 { panic!("a load without an uploaded map: {c}") };
        let light = bevy_axes(toward_game(0.3, 1.0));
        assert_eq!(terrain_visibility(&u, never, Vec3::new(100.0 * TEXEL, 0.0, -100.0 * TEXEL), light), 1.0);
        assert_eq!(solid_visibility(&u, 0.0, 1.0), 0.0, "C kept");
        // Nothing published at all: nothing to clear.
        let neutral = SunVisibilityUniform::pack(&SunVisibility { terrain_enabled: true, ..default() });
        assert_eq!(usable_uniform(neutral, false), neutral);
    }
}
