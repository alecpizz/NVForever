//! Cloud shadows (#2670, stage C1): the producer of the cloud transmittance the sun's
//! direct light is multiplied by. An enhanced-look engineering choice, not a parity item
//! (`docs/design/graphics-enhanced.md`); the original casts no cloud shadows.
//!
//! What the weather's clouds are today (sky.rs, fnv_world::sky):
//! - Four dome shapes from `meshes\sky\clouds.nif`, each drawn twice (two sets): set 0
//!   shows the current weather's layer at the fade's alpha, set 1 the previous weather's at
//!   `1 - fade`; with no fade running only set 0 shows, at alpha 1 (sky.rs `update_clouds`).
//!   A layer's on-screen alpha is texture alpha × the shape's vertex alpha (a fade towards
//!   the horizon) × that weight (sky.rs `setup_sky`, sky.wesl `fragment`).
//! - Each layer scrolls in U only, by `ONAM speed / 25,600` texture repeats per elapsed
//!   real second, each weather at its own speeds (fnv_world::sky `CLOUD_SCROLL_PER_UNIT`,
//!   a hypothesis there).
//! - The dome follows the camera and is hidden indoors (sky.rs `update_clouds`).
//!
//! What this module makes from them:
//! - **Composition** [engineering choice]: one 512 × 512 `R8Unorm` repeat-wrapped image
//!   holding the transmission `Π (1 - alpha × weight)` over the drawn layers of both sets
//!   (1 = fully lit), from each texture's ALPHA (decoded from the DDS file's top level by
//!   `nvf_assets::texture::alpha_map`: the GPU copies the texture cache makes keep no CPU
//!   data, `RenderAssetUsages::RENDER_WORLD`). Composed on the compute pool, never on the
//!   main thread. The shape's vertex alpha is left out: it fades the layer towards the
//!   dome's horizon, a property of where on the dome a texel is drawn, which a world plane
//!   does not have [engineering choice].
//! - **Relative transmittance** [engineering choice, from the contributor's play-test of
//!   2026-11-17: under an overcast weather the product is nearly uniform, so the switch
//!   dimmed the whole scene evenly, over a sun colour the weather already darkens]: the
//!   product is divided by a clear reference taken from the map itself, `K_rel = min(K /
//!   K_ref, 1)`, `K_ref` the [`REFERENCE_PERCENTILE`] (90th) percentile of the 512 × 512
//!   values ([`percentile`], a 256-bin histogram). The feature darkens only where there is
//!   more cloud than elsewhere and never dims overall: uniform cover of any density is all
//!   1; scattered cloud on clear sky (most texels 1) is unchanged within rounding; heavy
//!   cloud with gaps reads the gaps fully lit and the cloud darker by its ratio to them.
//!   Under [`REFERENCE_FLOOR`] the darkening is scaled down in proportion to the reference,
//!   to none at 0 (an opaque map casts nothing). `NVF_PLAY_DEBUG_CLOUD_REFERENCE=<percentile
//!   0..100 | off>` overrides the percentile; `off` gives the absolute product.
//!   `K_ref` changes with the map. The scrolling's reference layer (below) is never
//!   shifted inside the image, so with one layer, or all at one speed, the map and `K_ref`
//!   stay put while it scrolls;
//!   layers at other speeds shift against it, which changes the product's histogram along a
//!   period of `1 / |speed difference|` seconds (NVWastelandClear's 52 against 65: 1,969 s),
//!   a quarter texel per composition; during a weather fade `K_ref` moves with the weights
//!   (a 64th per composition at most). To keep a step between two compositions from showing
//!   as a "breathing" of the whole pattern, each composition moves the reference it divides
//!   by [`REFERENCE_SMOOTHING`] of the way towards its map's own ([`smoothed_reference`]),
//!   composing again at the rate cap until it is within [`REFERENCE_SETTLE`]; a
//!   discontinuity (below, "Validity") starts it from the next map's own. That this is not
//!   seen is a hypothesis: nothing has been run with it.
//! - **Scrolling**: the reference layer (the heaviest) scrolls by `uv_offset`, every frame,
//!   without recomposing; layers moving at other speeds (one weather's layers differ, and
//!   two weathers during a fade) are shifted against it inside the image, recomposed when
//!   one has moved by [`RECOMPOSE_STEP_TEXELS`] and at most every [`MIN_RECOMPOSE_SECS`].
//!   Arithmetic, not a measurement: at the largest possible speed difference (ONAM 255
//!   against 0: 255 / 25,600 = 0.00996 repeats/s, 5.1 texels/s at 512 texels) a layer
//!   crosses the quarter-texel step about 20 times a second; at NVWastelandClear's 65
//!   against 0 (0.00254 repeats/s, 1.3 texels/s) about 5.2 times. Both are held to the
//!   rate cap: at most 4 composition starts a second.
//! - **Projection** (consumed by the sun-visibility shader, C0): `hit = P + L ×
//!   (plane_height - P.y) / L.y`, `uv = hit.xz × uv_scale + uv_offset`
//!   ([`projected_uv`] is the reference). World-anchored: no term follows the camera, so
//!   the pattern moves only with the layers' drift. The dome is camera-relative and its UV
//!   layout is the mesh's, so the ground pattern cannot register with the dome exactly
//!   [hypothesis, fork-notes astra-plan §4]; the plane, the repeat and the strength are
//!   constants to calibrate from pictures.
//! - **Drift direction** [engineering choice]: the dome's +U offset (which moves its
//!   clouds towards -U on the mesh) is mapped to the weather's wind direction
//!   (`tree::wind_vector`: fixed, or a script's SetWindDirection), so the ground pattern
//!   drifts downwind at the layers' own speeds. The wind's displacement is accumulated
//!   ([`WindPhase`]: the direction of each moment times the time passed, in f64), so a
//!   direction change turns the drift from then on and never moves the pattern already
//!   drifted, nor the layers against each other. One texture repeat per [`REPEAT_UNITS`]
//!   (a lead, not a mechanism: the GECK wiki's Weather page says the wind speed "affects
//!   cloud, grass, tree movement" [documented: geck-wiki snapshot 2026-10-28, Weather]).
//!   The texture's own axes stay on world X/Z (a rotated repeat would not tile).
//!
//! - **Validity** [engineering choice; fork-notes review of C0/C1, finding 2]: jobs carry a
//!   discontinuity generation, advanced when the worldspace changes and when the weather
//!   set is replaced (a new weather, a fade turned round; not the fade's ordinary progress
//!   nor its end). A composition of an older generation is dropped before it touches the
//!   image; a worldspace change, and a weather replaced at once (no fade), also clear the
//!   output, which is neutral until the new composition is in. A fade's ordinary progress
//!   is followed with the bounded lag of the rate cap.
//!
//! - **The volume's cover** [design, #2991]: Volumetric clouds (volumetric_clouds.rs) need
//!   the absolute cover `1 - Π (1 - alpha × weight)`, which the relative image cannot give
//!   back. While it wants it ([`CloudCoverWanted`]) the producer runs whatever Cloud shadows
//!   says, each composition also returns the absolute product ([`compose_parts`] with a
//!   [`CoverInput`]: the relative texels are the same either way), written to a second image
//!   published as [`CloudCoverPublished`] with the same projection, with each layer's visible
//!   share (its alpha times what the layers in front of it on the dome let through) for the
//!   volume's colour weights and noise drift. A weather with an active layer whose texture
//!   cannot give plain 2D alpha (no alpha channel, a cube map or volume, a cut-off or
//!   unsupported file, none in the archives: [`volume_alpha_ok`]) gets no absolute cover, whole,
//!   also while it is one side of a fade, so the volume shows it the flat way
//!   ([`CloudCoverRefused`] says why; the mod rule: never a wrong sky). The
//!   shadows' own use of such a texture is unchanged, and no texture is decoded whole to learn
//!   it: the header says. With Cloud shadows off the relative image is neither made nor published, so nothing
//!   of the shadow path switches on; with the volume off nothing of this is made.
//!
//! It applies outdoors only, day and night alike (at night the directional light is the
//! moon's, attenuated the same way, no strength added); off, indoors, or with no drawn,
//! textured cloud layer, the output is `None`. While the switch is on, `None` does not take
//! the pipelines' cloud variant away: sun_visibility.rs keeps the views on it with a
//! neutral cloud (K = 1), so only the switch changes pipelines ([`follow_switch`]). The
//! switch is the settings row Cloud shadows (`Graphics::cloud_shadows`, `cloud-shadows`
//! in `--graphics-only`; off by default and in every preset); `NVF_PLAY_DEBUG_CLOUD_SHADOWS=1`
//! turns it on whatever the row says. Off, nothing runs beyond the run conditions and no
//! image is held.
//!
//! Test aids: `NVF_PLAY_DEBUG_CLOUD_SHADOW=<plane height>,<repeat>,<strength>[,<direction
//! deg>]` overrides the calibration constants (direction in game degrees from +x towards
//! +y, in place of the wind's), and `NVF_PLAY_DEBUG_CLOUD_PHASE=<seconds>` freezes the
//! clouds' scroll clock, the dome's and this one ([`cloud_seconds`]), for screenshots;
//! `NVF_PLAY_DEBUG_CLOUD_REFERENCE=<percentile 0..100 | off>` sets the clear reference's
//! percentile, or turns the relative rule off (the absolute product, as before it).
//!
//! - **The light volume's shadow** [design, #3032 stage 2i M2; docs/design/graphics-enhanced.md
//!   §3.23, "Ground shadows from the light volume"]: while the volumetric clouds' light volume has
//!   completed fronts, sun_visibility.wesl takes K from its optical depth (`exp(-depth)`, this
//!   module's strength and low-sun band) where it has data and from this module's layers elsewhere
//!   ([`crate::sun_visibility::CloudSource`]). Nothing here changes for it: the layers' image, the
//!   absolute cover and the plane are made as before; this module hands over only the strength and
//!   the aid `NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE=weather|volume|auto|share|depth` ([`parse_source`];
//!   dev_aids.rs: not in release builds).

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{futures::check_ready, AsyncComputeTaskPool, Task};
use fnv_world::sky::Weather;
use nvf_assets::GameData;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

/// Side of the transmittance image in texels [engineering choice, astra-plan §4: 256 KB,
/// one texel about 117 units at [`REPEAT_UNITS`]].
pub const MAP_SIZE: usize = 512;
/// Height of the cloud plane above the worldspace's ground reference, game units
/// [engineering choice: to be calibrated from pictures].
pub const PLANE_ABOVE_GROUND: f32 = 20_000.0;
/// World size of one texture repeat on the plane, game units [engineering choice: the
/// contributor's pick in play on 2026-11-17 among 60,000, 40,000, 30,000 and 20,000,
/// docs/design/sun-visibility.md §8; to be refined].
pub const REPEAT_UNITS: f32 = 60_000.0;
/// How much of the sun's direct light a fully opaque cloud takes away: K = mix(1, T,
/// strength) [engineering choice: the contributor's pick in play on 2026-11-17 among 0.6,
/// 0.7, 0.8 and 1.0, docs/design/sun-visibility.md §8; to be refined].
pub const STRENGTH: f32 = 0.6;
/// The ground reference (the camera's height when the worldspace's first output is made)
/// is rounded to this, game units [engineering choice: one LAND height step band, so a
/// start a few units higher gives the same plane].
const GROUND_SNAP: f32 = 1024.0;
/// A layer shifted against the reference by this many texels since the last composition
/// is composed again [engineering choice: a quarter texel, about 29 units on the ground;
/// hypothesis, not looked at: too small to see as a step in a soft cloud edge].
pub const RECOMPOSE_STEP_TEXELS: f64 = 0.25;
/// Least real time between two compositions, seconds [engineering choice: 4 a second at
/// most, each a 256 KB upload; hypothesis, unmeasured: a composition takes a few
/// milliseconds of one compute-pool thread].
pub const MIN_RECOMPOSE_SECS: f64 = 0.25;
/// A layer weight moved by this much is composed again [engineering choice: a 30 s
/// weather fade then composes about 64 times, under the rate cap].
const WEIGHT_STEP: f32 = 1.0 / 64.0;
/// Weights are logged again when one moved by this much [engineering choice: four lines
/// over a fade].
const LOG_WEIGHT_STEP: f32 = 0.25;
/// The published offset is renewed when it moved by this much of a repeat (1/8 texel,
/// about 15 units) [engineering choice: the consumer's uniform is not touched every frame
/// for a drift that small; hypothesis, not looked at: invisible].
const PUBLISH_OFFSET_STEP: f32 = 1.0 / 4096.0;
/// The percentile of the composed transmittance taken as the map's clear reference
/// (module notes, "Relative transmittance") [engineering choice: to be calibrated from
/// pictures; `NVF_PLAY_DEBUG_CLOUD_REFERENCE` overrides it].
pub const REFERENCE_PERCENTILE: f32 = 90.0;
/// A reference under this scales the darkening down in proportion, to none at 0: a map
/// whose clearest tenth still lets less than this through is cover with no clear gaps, and
/// dividing by a tiny reference would blow the alpha's 8-bit steps up into speckle
/// [engineering choice: to be calibrated from pictures].
pub const REFERENCE_FLOOR: f32 = 0.1;
/// Each composition moves the reference it uses this share of the way from the previous
/// composition's to the map's own [engineering choice: about four compositions' lag, a
/// second at the rate cap; hypothesis, not looked at: no visible breathing].
pub const REFERENCE_SMOOTHING: f32 = 0.25;
/// Within this of the map's own reference the smoothed one takes it whole: half a step of
/// the 8-bit image [engineering choice]. Until then the map is composed again (at the rate
/// cap) so the reference settles even when nothing else moves.
const REFERENCE_SETTLE: f32 = 0.5 / 255.0;
/// Below this height of the direction to the light (sine of its elevation) the plane is
/// not reached [engineering choice: the light has faded out within 0.1 rad of the horizon,
/// sky.rs `HORIZON_FADE`].
#[cfg_attr(not(test), allow(dead_code))] // used by `projected_uv`, the shader contract's reference
pub const MIN_LIGHT_Y: f32 = 0.02;

/// Whether cloud shadows are made: the settings row (`Graphics::cloud_shadows`, off by
/// default), or `forced` by `NVF_PLAY_DEBUG_CLOUD_SHADOWS=1` whatever the row says.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CloudShadows {
    pub enabled: bool,
    /// `NVF_PLAY_DEBUG_CLOUD_SHADOWS=1`.
    pub forced: bool,
}

/// The settings row (`Graphics::cloud_shadows`) or the debug override decide the switch.
pub fn follow_graphics(g: Res<crate::graphics::Graphics>, mut shadows: ResMut<CloudShadows>) {
    let want = shadows.forced || g.cloud_shadows;
    if shadows.enabled != want {
        shadows.enabled = want;
    }
}

/// The switch into the sun-visibility plumbing: `cloud_enabled` decides the views' cloud
/// variant, whether or not an image is published (sun_visibility.rs module notes).
pub fn follow_switch(shadows: Res<CloudShadows>, mut sink: ResMut<crate::sun_visibility::SunVisibility>) {
    if sink.cloud_enabled != shadows.enabled {
        sink.cloud_enabled = shadows.enabled;
    }
}

/// `NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE=weather|volume|auto|share|depth` (#3032 stage 2i M2; auto when unset,
/// empty or unknown, with a warning for an unknown value): where the cloud shadows come from
/// (sun_visibility.rs [`crate::sun_visibility::CloudSource`]).
pub fn parse_source(value: Option<&str>) -> crate::sun_visibility::CloudSource {
    use crate::sun_visibility::CloudSource;
    match value {
        None => CloudSource::Auto,
        Some(v) => CloudSource::parse(v).unwrap_or_else(|| {
            warn!("cloud shadows: NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE={v} unknown (weather, volume, auto, share or depth); auto");
            CloudSource::Auto
        }),
    }
}

/// Whether [`hand_over_source`] is due: not done yet, and the light volume asked for (the Volumetric
/// clouds row and Cloud shadows on).
fn source_due(shadows: Res<CloudShadows>, graphics: Option<Res<crate::graphics::Graphics>>, mut done: Local<bool>) -> bool {
    let due = !*done && shadows.enabled && graphics.is_some_and(|g| g.volumetric_clouds);
    *done |= due;
    due
}

/// Hands the sun-visibility plumbing the source the aid asks for and the strength in force (the
/// shadow from the light volume takes the same strength as the weather's layers, and needs it
/// whether or not a layer image is published). Once, when the light volume is first asked for
/// (neither changes in a run).
fn hand_over_source(sink: Option<ResMut<crate::sun_visibility::SunVisibility>>) {
    let Some(mut sink) = sink else { return };
    let value = crate::dev_aids::var("NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE");
    let source = parse_source(value.as_deref());
    if value.is_some() {
        info!("cloud shadows: source asked for: {} (NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE)", source.name());
    }
    let strength = calibration().strength;
    if sink.cloud_source != source || sink.cloud_strength != strength {
        sink.cloud_source = source;
        sink.cloud_strength = strength;
    }
}

/// Run condition: the producer runs only while cloud shadows are on.
pub fn cloud_shadows_on(s: Option<Res<CloudShadows>>) -> bool {
    s.is_some_and(|s| s.enabled)
}

/// Volumetric clouds (volumetric_clouds.rs, #2991) want the absolute cover: the producer runs
/// for them too, and keeps a second, absolute image ([`CloudCoverPublished`]). Nothing of the
/// shadow path follows from it: the relative output stays `None` while Cloud shadows is off
/// [design].
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CloudCoverWanted(pub bool);

/// Run condition of the producer: Cloud shadows on, or the volume wanting the cover.
pub fn producer_wanted(s: Option<Res<CloudShadows>>, v: Option<Res<CloudCoverWanted>>) -> bool {
    cloud_shadows_on(s) || v.is_some_and(|v| v.0)
}

/// Run condition of [`publish_cloud_cover`]: the volume wants the cover, or one (or the word
/// that there is none to come) is still published.
fn cover_in_use(wanted: Res<CloudCoverWanted>, cover: Res<CloudCoverPublished>, empty: Res<CloudCoverEmpty>) -> bool {
    wanted.0 || cover.0.is_some() || empty.0
}

/// Run condition of the release: either consumer's switch changed.
fn switches_changed(s: Res<CloudShadows>, v: Res<CloudCoverWanted>) -> bool {
    s.is_changed() || v.is_changed()
}

/// What the volume takes from the composition (volumetric_clouds.rs): the mean of the absolute
/// cover `1 - Π (1 - alpha × weight)`, the layers' visible shares and their weathers, and the
/// drift, with the shadows' projection. No image: the volume reads no texel of the cover since
/// #3032 A3, so none is quantized, stored, uploaded or bound.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudCoverOutput {
    pub uv_scale: Vec2,
    /// The volume's drift in the same texture coordinates (texture repeats, f64, unwrapped): the drift
    /// of the layers that carry the cover, weighted by their visible share ([`cover_scroll`]),
    /// accumulated frame by frame ([`advance_drift`]), so it has no jump when a weather fade changes which
    /// layer is the shadows' reference (their `uv_offset` does, and the composition makes up for it in the
    /// image; nothing would for the volume's field) nor where a wrapped offset does.
    pub drift: [f64; 2],
    /// Bevy y of the cloud plane (the volume's base).
    pub plane_height: f32,
    /// The mean of `1 - T` over the image, 0..1 (logged by the volume).
    pub mean_cover: f32,
    /// Per set (0 the current weather, 1 the one fading out) and WTHR layer, the layer's mean
    /// visible share over the map, 0..1: its alpha times its fade weight times what the
    /// layers drawn in front of it on the dome let through ([`compose_cover`]); 0 for none,
    /// no alpha, or not drawn. The volume weights the layers' PNAM colours by it.
    pub layer_cover: [[f32; 4]; 2],
    /// The weathers (form IDs) sets 0 and 1 of `layer_cover` showed when it was composed: a
    /// fade that starts or turns round after it renames the sets, so the volume takes each
    /// set's colours from the weather of this ID, not from today's set.
    pub layer_weathers: [Option<u32>; 2],
    /// The live sky's weathers' own covers: the shown weather's, then the
    /// one fading out's (while a fade runs), each composed at full weight from that weather's
    /// layers alone and verified textures, `None` for a weather not composed yet. The volume
    /// drives its amount and its tint shares from these and the live fade each frame.
    pub weathers: [Option<WeatherCover>; 2],
    /// Incoming/outgoing IDs while the first cover measurement of a fade is in flight.
    pub settling: Option<(u32, u32)>,
}

// The old endpoint can differ from the exact blend by at most this share of its
// full range. Also bound the real wait when the game clock is stopped.
const SETTLING_WEIGHT: f32 = 0.05;
const SETTLING_TIME: std::time::Duration = std::time::Duration::from_secs(1);

/// Wall-clock limit for holding the outgoing volume while a new weather's cover is prepared.
#[derive(Default)]
pub struct CoverWait(Option<((u32, u32), std::time::Instant)>);

impl CoverWait {
    fn within_grace(&mut self, pair: Option<(u32, u32)>, now: std::time::Instant) -> Option<(u32, u32)> {
        if self.0.map(|(old, _)| old) != pair {
            self.0 = pair.map(|p| (p, now));
        }
        self.0.and_then(|(p, start)| (now.duration_since(start) <= SETTLING_TIME).then_some(p))
    }
}

impl CloudCoverOutput {
    /// Keep the measured outgoing endpoint for the initial, bounded part of a
    /// fade. Preserve its identity so its actual PNAM layers supply the tint.
    pub fn contributors(&self, sky: &crate::sky::SkyState) -> [(Option<u32>, f32); 2] {
        let live = live_weathers(sky);
        if self.settling.is_some_and(|(incoming, outgoing)| live[0].0 == Some(incoming) && live[1].0 == Some(outgoing)) && live[0].1 <= SETTLING_WEIGHT {
            return [(live[0].0, 0.0), (live[1].0, 1.0)];
        }
        live
    }
}

/// One weather's own cover ([`CloudCoverOutput::weathers`]): its form ID,
/// the mean of `1 - Π (1 - alpha)` over its own drawn layers at full weight (the dome's layers of
/// that weather alone, at the drift's shifts of its composition), and per WTHR layer its mean
/// visible share among them ([`compose_cover`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeatherCover {
    pub id: u32,
    pub mean: f32,
    pub shares: [f32; 4],
}

/// The published cover: `None` unless the volume wants it and a composition for it is in.
#[derive(Resource, Default, Debug)]
pub struct CloudCoverPublished(pub Option<CloudCoverOutput>);

/// The volume's drift after the wind's displacement moved from `last` to `phase` (`None`: the first step, no
/// move): it drifts at the rate `ref_scroll` (the covering layers' [`cover_scroll`], else the shadows'
/// reference layer's), but a change of rate never changes its value; never wrapped, so the fields that ride on
/// it (volumetric_clouds.rs) cross a repeat with no jump. Unchanged when the step is not a finite number.
pub fn advance_drift(offset: [f64; 2], last: Option<[f64; 2]>, phase: [f64; 2], ref_scroll: f64) -> [f64; 2] {
    let Some(last) = last else { return offset };
    let step = |i: usize| offset[i] - (phase[i] - last[i]) * ref_scroll;
    let next = [step(0), step(1)];
    if next.iter().all(|v| v.is_finite()) { next } else { offset }
}

/// The plane's height above the worldspace's ground reference in force
/// (`NVF_PLAY_DEBUG_CLOUD_SHADOW` may set it): the volume bends its domain from it.
pub fn plane_above_ground() -> f32 {
    calibration().plane_above_ground
}

/// The scroll rate the volume's noise drifts at: the layers' rates weighted by their visible
/// share of the cover (`visible`, from [`compose_cover`]); `None` when no layer carries any
/// (a transparent layer never sets the rate, even when it is the shadows' reference).
pub fn cover_scroll(visible: &[f32], scrolls: &[f64]) -> Option<f64> {
    let (mut sum, mut total) = (0.0f64, 0.0f64);
    for (&v, &s) in visible.iter().zip(scrolls) {
        if v.is_finite() && v > 0.0 && s.is_finite() {
            sum += f64::from(v) * s;
            total += f64::from(v);
        }
    }
    (total > 1e-9).then_some(sum / total)
}

/// Whether a DDS file can give the volume its cover ([`volume_alpha_of`] after one parse).
#[cfg_attr(not(test), allow(dead_code))]
pub fn volume_alpha_ok(bytes: &[u8]) -> bool {
    let Ok(dds) = fnv_dds::Dds::parse(bytes) else { return false };
    volume_alpha_of(&dds, nvf_assets::texture::alpha_map_of(&dds).as_ref())
}

/// Whether a parsed texture, with the alpha read from it, can give the volume its cover:
/// the sky's own loader would take it (one set of shape rules,
/// `nvf_assets::texture::colour_shape_ok`: no cube map, sides and mip levels in range; its whole
/// payload there, which `fnv_dds::Dds::parse` checks), it is not a volume texture whatever its
/// depth, its format holds alpha ([`alpha_format`]), and a BC1 (DXT1) file uses its one-bit
/// alpha somewhere: one whose blocks are all opaque carries none and is refused. A DXT5 or ARGB
/// file whose alpha is full everywhere is a true full cover and is taken.
pub fn volume_alpha_of(dds: &fnv_dds::Dds, alpha: Option<&nvf_assets::texture::AlphaMap>) -> bool {
    let Some(alpha) = alpha else { return false };
    let mips = dds.levels.first().map_or(0, |l| l.len() as u32);
    nvf_assets::texture::colour_shape_ok(dds, mips).is_ok()
        && !volume_flagged(&dds.header)
        && row_pitch_ok(dds)
        && alpha_format(dds.format)
        && (dds.format != fnv_dds::Format::Dxt1 || alpha.alpha.iter().any(|&a| a < 255))
}

/// Whether an uncompressed file's declared row pitch (DDSD_PITCH) is its rows' own width in
/// bytes: the parser and the alpha reader take the rows as packed, so a file with padded rows
/// would be read with its padding as pixels. A file that declares none, and a
/// block format, pass [documented: Microsoft, "DDS_HEADER structure", dwPitchOrLinearSize].
pub fn row_pitch_ok(dds: &fnv_dds::Dds) -> bool {
    const DDSD_PITCH: u32 = 0x8;
    let h = &dds.header;
    dds.format.block_bytes().is_some() || h.flags & DDSD_PITCH == 0 || h.pitch_or_linear_size as usize == dds.format.level_size(h.width, 1)
}

/// Whether a DDS header says volume texture: a depth over one, the volume capability
/// (DDSCAPS2_VOLUME) or the depth flag (DDSD_DEPTH) [documented: Microsoft, "DDS_HEADER
/// structure"].
pub fn volume_flagged(h: &fnv_dds::DdsHeader) -> bool {
    h.depth > 1 || h.caps[1] & 0x0020_0000 != 0 || h.flags & 0x0080_0000 != 0
}

/// Whether a DDS format holds alpha (nvf_assets `alpha_map` reads it).
pub fn alpha_format(f: fnv_dds::Format) -> bool {
    use fnv_dds::Format::*;
    matches!(f, Dxt1 | Dxt3 | Dxt5 | A8R8G8B8 | A4R4G4B4 | A8B8G8R8 | A1R5G5B5 | A8L8)
}

/// The volume's word on a weather's cover ([`volume_alpha_ok`]): the first active layer's
/// texture it cannot read, if any; then the whole weather is shown the flat way.
pub fn refused_texture<'a>(textures: impl IntoIterator<Item = &'a str>, unreadable: &HashSet<String>) -> Option<&'a str> {
    textures.into_iter().find(|t| unreadable.contains(*t))
}

/// Whether the shown absolute cover was composed for other weathers (by set) than the sky
/// shows now: its colours are looked up by those weathers, so a fade that started and ended
/// between two compositions (a one-byte transition is about 118 ms, the interval 250 ms) would
/// leave it with a weather gone from the sky and no tint: it is composed again.
pub fn cover_weathers_stale(shown: bool, composed: [Option<u32>; 2], sky: [Option<u32>; 2]) -> bool {
    shown && composed != sky
}

/// A cloud texture still being read after this many real seconds is named in the log once.
const STUCK_READ_SECS: f64 = 10.0;

/// The live sky's contributors (form IDs) and their weights on the dome, as [`active_layers`]
/// weights its sets: the shown weather at the fade and the one fading out at one minus it while
/// a fade runs, the shown weather alone at 1 otherwise.
pub fn live_weathers(sky: &crate::sky::SkyState) -> [(Option<u32>, f32); 2] {
    let fade = if sky.fade.is_finite() { sky.fade.clamp(0.0, 1.0) } else { 1.0 };
    match sky.prev.as_ref().filter(|_| fade < 1.0) {
        Some(p) => [(Some(sky.weather.form_id), fade), (Some(p.form_id), 1.0 - fade)],
        None => [(Some(sky.weather.form_id), 1.0), (None, 0.0)],
    }
}

/// The covers of the live weathers `live` from those composed so far (`known`), in the live
/// order; `None` for a weather not composed yet (a weather that has left the
/// sky is never looked up, so its colours and amount cannot linger).
pub fn live_covers(known: &[WeatherCover], live: [(Option<u32>, f32); 2]) -> [Option<WeatherCover>; 2] {
    live.map(|(id, _)| id.and_then(|id| known.iter().find(|w| w.id == id).copied()))
}

/// Why the volume gets no cover though the producer runs: an active layer's texture it cannot
/// read as plain 2D alpha (its path), for its log line. `None` otherwise.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct CloudCoverRefused(pub Option<String>);

/// The volume wants the cover and there is nothing to compose one from: the weather outdoors
/// has no cloud layer. Not a cover still to come, and no refusal.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CloudCoverEmpty(pub bool);

/// Publish the absolute cover while it is wanted and in, its drift renewed in the same
/// steps as the shadows' offset ([`PUBLISH_OFFSET_STEP`]).
pub fn publish_cloud_cover(
    wanted: Res<CloudCoverWanted>,
    work: Res<CloudShadowWork>,
    mut cover: ResMut<CloudCoverPublished>,
    (refused, empty): (Option<ResMut<CloudCoverRefused>>, Option<ResMut<CloudCoverEmpty>>),
    sky: Option<Res<crate::sky::SkyState>>,
    mut waiting: Local<CoverWait>,
) {
    let refusal = work.cover_refused.clone().filter(|_| wanted.0);
    // The live weathers' own covers, looked up now, at publication (after a job finished in this
    // frame's producer). A new weather has no measurement until its composition finishes.
    let live = sky.as_deref().map_or([(None, 0.0), (None, 0.0)], live_weathers);
    let weathers = live_covers(&work.weather_covers, live);
    let pair = match (live, weathers) {
        ([(Some(to), share), (Some(from), _)], [None, Some(_)])
            if wanted.0 && work.absolute_shown && refusal.is_none() && share <= SETTLING_WEIGHT => Some((to, from)),
        _ => None,
    };
    let settling = waiting.within_grace(pair, std::time::Instant::now());
    if let Some(mut r) = refused {
        if r.0 != refusal {
            r.0 = refusal.clone();
        }
    }
    if let Some(mut e) = empty {
        let none = wanted.0 && work.cover_empty;
        if e.0 != none {
            e.0 = none;
        }
    }
    let want = match (wanted.0 && work.absolute_shown && refusal.is_none() && (!work.cover_pending || settling.is_some()), work.cover_at) {
        (true, Some((uv_scale, plane_height))) => {
            Some(CloudCoverOutput {
                uv_scale,
                drift: work.drift,
                plane_height,
                mean_cover: work.cover_mean,
                layer_cover: work.layer_cover,
                layer_weathers: work.layer_weathers,
                weathers,
                settling,
            })
        }
        _ => None,
    };
    let renew = match (&cover.0, &want) {
        (None, None) => false,
        (Some(p), Some(w)) => {
            let moved = (p.drift[0] - w.drift[0]).abs().max((p.drift[1] - w.drift[1]).abs()) as f32;
            p.uv_scale != w.uv_scale
                || p.plane_height != w.plane_height
                || p.mean_cover != w.mean_cover
                || p.layer_cover != w.layer_cover
                || p.layer_weathers != w.layer_weathers
                || p.settling != w.settling
                || p.weathers != w.weathers
                || moved >= PUBLISH_OFFSET_STEP
        }
        _ => true,
    };
    if renew {
        cover.0 = want;
    }
}

/// What the sun-visibility plumbing samples: the fields of C0's `CloudVisibility`.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudShadowOutput {
    /// `R8Unorm`, 1 = fully lit (sampled repeating and linear by the plumbing's sampler).
    pub image: Handle<Image>,
    /// World (Bevy x, z) to texture coordinates: 1 / [`REPEAT_UNITS`] on both axes.
    pub uv_scale: Vec2,
    /// The reference layer's drift, 0..1 on both axes.
    pub uv_offset: Vec2,
    /// Bevy y of the cloud plane.
    pub plane_height: f32,
    pub strength: f32,
}

/// The published output: `None` when off, indoors, or with no textured cloud layer.
#[derive(Resource, Default, Debug)]
pub struct CloudShadowPublished(pub Option<CloudShadowOutput>);

/// Copy the published output into the sun-visibility resource when it changed (the
/// plumbing of sun_visibility.rs samples it on the sun's light).
pub fn publish_cloud_shadows(published: Res<CloudShadowPublished>, mut sink: ResMut<crate::sun_visibility::SunVisibility>) {
    if published.is_changed() {
        sink.cloud = published.0.as_ref().map(|c| crate::sun_visibility::CloudVisibility { image: c.image.clone(), uv_scale: c.uv_scale, uv_offset: c.uv_offset, plane_height: c.plane_height, strength: c.strength });
    }
}

/// Whether sun_visibility.rs's test producer owns the cloud slot
/// (`NVF_PLAY_DEBUG_SUN_VISIBILITY_TEST=checker`, [`crate::sun_visibility::test_producer_wanted`]):
/// the weather's clouds then stay out of it. Any other value starts no test producer and
/// leaves the slot to the weather's clouds.
fn test_producer_absent() -> bool {
    !crate::sun_visibility::test_producer_wanted(std::env::var("NVF_PLAY_DEBUG_SUN_VISIBILITY_TEST").ok().as_deref())
}

pub struct CloudShadowsPlugin;

impl Plugin for CloudShadowsPlugin {
    fn build(&self, app: &mut App) {
        let forced = nvf_play::dialogue::debug_switch(std::env::var("NVF_PLAY_DEBUG_CLOUD_SHADOWS").ok().as_deref());
        if forced {
            info!("cloud shadows: on by NVF_PLAY_DEBUG_CLOUD_SHADOWS, whatever the settings row says");
        }
        app.insert_resource(CloudShadows { enabled: forced, forced })
            .init_resource::<CloudShadowPublished>()
            .init_resource::<CloudShadowWork>()
            .init_resource::<CloudCoverWanted>()
            .init_resource::<CloudCoverPublished>()
            .init_resource::<CloudCoverRefused>()
            .init_resource::<CloudCoverEmpty>()
            .add_systems(
                Update,
                (
                    follow_graphics.run_if(resource_exists_and_changed::<crate::graphics::Graphics>),
                    follow_switch.run_if(resource_changed::<CloudShadows>),
                    hand_over_source.run_if(source_due),
                    produce_cloud_shadows.run_if(producer_wanted),
                    release_cloud_shadows.run_if(switches_changed),
                    publish_cloud_cover.run_if(cover_in_use),
                )
                    .chain()
                    .after(crate::sky::update_clouds),
            )
            .add_systems(Update, publish_cloud_shadows.after(release_cloud_shadows).run_if(test_producer_absent));
    }
}

// ---------------------------------------------------------------------------------------
// Test aids and calibration
// ---------------------------------------------------------------------------------------

/// Seconds of the clouds' scroll clock: elapsed time, or the frozen phase
/// `NVF_PLAY_DEBUG_CLOUD_PHASE=<seconds>` (the dome, sky.rs `update_clouds`, uses it too).
pub fn cloud_seconds(time: &Time) -> f64 {
    frozen_phase().unwrap_or_else(|| time.elapsed_secs_f64())
}

fn frozen_phase() -> Option<f64> {
    static PHASE: OnceLock<Option<f64>> = OnceLock::new();
    *PHASE.get_or_init(|| {
        let phase = parse_phase(std::env::var("NVF_PLAY_DEBUG_CLOUD_PHASE").ok().as_deref());
        if let Some(p) = phase {
            info!("clouds: scroll clock frozen at {p} s (NVF_PLAY_DEBUG_CLOUD_PHASE)");
        }
        phase
    })
}

fn parse_phase(value: Option<&str>) -> Option<f64> {
    value.and_then(|v| v.trim().parse::<f64>().ok()).filter(|p| p.is_finite() && *p >= 0.0)
}

/// The projection's calibration ([`PLANE_ABOVE_GROUND`], [`REPEAT_UNITS`], [`STRENGTH`]),
/// and a drift direction in place of the wind's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Calibration {
    pub plane_above_ground: f32,
    pub repeat: f32,
    pub strength: f32,
    /// Game degrees from +x towards +y.
    pub direction_deg: Option<f32>,
}

impl Default for Calibration {
    fn default() -> Self {
        Self { plane_above_ground: PLANE_ABOVE_GROUND, repeat: REPEAT_UNITS, strength: STRENGTH, direction_deg: None }
    }
}

/// `<height>,<repeat>,<strength>[,<direction deg>]`; an empty or unreadable part keeps
/// its default.
pub fn parse_calibration(value: Option<&str>) -> Calibration {
    let mut c = Calibration::default();
    let Some(value) = value else { return c };
    let parts: Vec<Option<f32>> = value.split(',').map(|p| p.trim().parse::<f32>().ok().filter(|v| v.is_finite())).collect();
    let part = |i: usize| parts.get(i).copied().flatten();
    if let Some(h) = part(0) {
        c.plane_above_ground = h;
    }
    if let Some(r) = part(1).filter(|r| *r > 1.0) {
        c.repeat = r;
    }
    if let Some(s) = part(2) {
        c.strength = s.clamp(0.0, 1.0);
    }
    c.direction_deg = part(3);
    c
}

fn calibration() -> Calibration {
    static CAL: OnceLock<Calibration> = OnceLock::new();
    *CAL.get_or_init(|| parse_calibration(std::env::var("NVF_PLAY_DEBUG_CLOUD_SHADOW").ok().as_deref()))
}

/// `NVF_PLAY_DEBUG_CLOUD_REFERENCE`: a percentile 0..100 in place of
/// [`REFERENCE_PERCENTILE`], or `off` (`None`) for the absolute transmittance; unset,
/// empty or unreadable keeps the default.
pub fn parse_reference(value: Option<&str>) -> Option<f32> {
    match value.map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("off") => None,
        Some(v) => Some(v.parse::<f32>().ok().filter(|p| (0.0..=100.0).contains(p)).unwrap_or(REFERENCE_PERCENTILE)),
        None => Some(REFERENCE_PERCENTILE),
    }
}

fn reference_percentile() -> Option<f32> {
    static PCT: OnceLock<Option<f32>> = OnceLock::new();
    *PCT.get_or_init(|| {
        let value = std::env::var("NVF_PLAY_DEBUG_CLOUD_REFERENCE").ok();
        let pct = parse_reference(value.as_deref());
        if value.is_some() {
            match pct {
                Some(p) => info!("cloud shadows: reference at the {p} percentile (NVF_PLAY_DEBUG_CLOUD_REFERENCE)"),
                None => info!("cloud shadows: absolute transmittance, no reference (NVF_PLAY_DEBUG_CLOUD_REFERENCE=off)"),
            }
        }
        pct
    })
}

// ---------------------------------------------------------------------------------------
// Pure parts
// ---------------------------------------------------------------------------------------

/// One cloud layer to compose: its texture, its transition weight and its scroll rate.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerInput {
    pub texture: String,
    pub weight: f32,
    /// Texture repeats per real second (fnv_world::sky `Weather::cloud_scroll`).
    pub scroll: f64,
}

/// The textured layers the dome draws, weighted as sky.rs `update_clouds` weights its two
/// sets: the current weather at `fade` and the previous at `1 - fade` while a fade runs,
/// the current alone at 1 otherwise. `drawn`: the layers the dome has a shape for.
#[cfg(test)]
pub fn active_layers(current: &Weather, prev: Option<&Weather>, fade: f32, drawn: [bool; 4]) -> Vec<LayerInput> {
    active_layers_with(current, prev, fade, drawn, |w, layer| f64::from(w.cloud_scroll(layer)))
}

/// [`active_layers`] with a caller-supplied per-layer scroll rate (so `CloudUpgrade.ini`
/// modulation stays in sync with the sky dome's clouds when active).
pub fn active_layers_with(
    current: &Weather,
    prev: Option<&Weather>,
    fade: f32,
    drawn: [bool; 4],
    scroll_of: impl Fn(&Weather, usize) -> f64,
) -> Vec<LayerInput> {
    let fade = fade.clamp(0.0, 1.0);
    let sets: [(Option<&Weather>, f32); 2] = match prev {
        None => [(Some(current), 1.0), (None, 0.0)],
        Some(p) => [(Some(current), fade), (Some(p), 1.0 - fade)],
    };
    let mut out = Vec::new();
    for (w, weight) in sets {
        let Some(w) = w else { continue };
        if weight <= 0.0 {
            continue;
        }
        for (layer, on) in drawn.iter().enumerate() {
            let Some(texture) = w.cloud_textures[layer].as_ref().filter(|_| *on) else { continue };
            out.push(LayerInput { texture: texture.clone(), weight, scroll: scroll_of(w, layer) });
        }
    }
    out
}

/// For each of [`active_layers`]' layers, in its order, its (set, WTHR layer): set 0 the
/// current weather, 1 the one fading out. The dome draws layer `l` of set `s` at sort rank
/// `2 l + s` (sky.rs `setup_sky`): a higher rank in front.
pub fn active_slots(current: &Weather, prev: Option<&Weather>, fade: f32, drawn: [bool; 4]) -> Vec<(usize, usize)> {
    let fade = fade.clamp(0.0, 1.0);
    let sets: [(Option<&Weather>, f32); 2] = match prev {
        None => [(Some(current), 1.0), (None, 0.0)],
        Some(p) => [(Some(current), fade), (Some(p), 1.0 - fade)],
    };
    let mut out = Vec::new();
    for (set, (w, weight)) in sets.into_iter().enumerate() {
        let Some(w) = w else { continue };
        if weight <= 0.0 {
            continue;
        }
        for (layer, on) in drawn.iter().enumerate() {
            if *on && w.cloud_textures[layer].is_some() {
                out.push((set, layer));
            }
        }
    }
    out
}

/// A layer's alpha resampled to [`MAP_SIZE`]² (box filter when the source is larger,
/// wrapped bilinear when smaller), rows along V from the top.
pub fn resample_alpha(src: &[u8], width: usize, height: usize) -> Vec<u8> {
    let n = MAP_SIZE;
    if width == 0 || height == 0 || src.len() < width * height {
        return vec![0; n * n];
    }
    let (xs, ys) = (axis_taps(width, n), axis_taps(height, n));
    let mut out = vec![0u8; n * n];
    for (y, ty) in ys.iter().enumerate() {
        for (x, tx) in xs.iter().enumerate() {
            let mut acc = 0.0f32;
            for &(sy, wy) in ty {
                for &(sx, wx) in tx {
                    acc += f32::from(src[sy * width + sx]) * wx * wy;
                }
            }
            out[y * n + x] = acc.round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// For each of `n` target texels along an axis, the source texels and their weights.
fn axis_taps(src: usize, n: usize) -> Vec<Vec<(usize, f32)>> {
    if src >= n {
        let r = src as f32 / n as f32;
        (0..n)
            .map(|i| {
                let (a, b) = (i as f32 * r, (i + 1) as f32 * r);
                let mut taps = Vec::new();
                let mut k = a.floor() as usize;
                while (k as f32) < b && k < src {
                    let (lo, hi) = ((k as f32).max(a), ((k + 1) as f32).min(b));
                    if hi > lo {
                        taps.push((k, (hi - lo) / r));
                    }
                    k += 1;
                }
                taps
            })
            .collect()
    } else {
        (0..n)
            .map(|i| {
                let p = (i as f32 + 0.5) * src as f32 / n as f32 - 0.5;
                let f = p.floor();
                let k0 = (f as i64).rem_euclid(src as i64) as usize;
                vec![(k0, 1.0 - (p - f)), ((k0 + 1) % src, p - f)]
            })
            .collect()
    }
}

/// One layer as composed: its [`MAP_SIZE`]² alpha (`None`: missing, not loaded, or without
/// alpha: no cover), its weight, and where it is sampled against the image, in texture
/// coordinates (any value; wrapped).
#[derive(Clone, Debug)]
pub struct ComposeLayer {
    pub alpha: Option<Arc<[u8]>>,
    pub weight: f32,
    pub shift: [f64; 2],
}

/// The transmittance image, absolute: texel (x, y) is `Π (1 - alpha_i(uv + shift_i) ×
/// weight_i)` at its centre uv, each layer sampled bilinearly and wrapped; 255 = fully lit.
#[cfg_attr(not(test), allow(dead_code))] // the producer composes through `compose_parts`
pub fn compose(layers: &[ComposeLayer]) -> Vec<u8> {
    quantize(&compose_transmission(layers))
}

/// 0..1 to the image's 8 bits.
fn quantize(t: &[f32]) -> Vec<u8> {
    t.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect()
}

/// [`compose`]'s product before it is rounded to 8 bits, 0..1 per texel.
pub fn compose_transmission(layers: &[ComposeLayer]) -> Vec<f32> {
    let n = MAP_SIZE;
    let mut t = vec![1.0f32; n * n];
    for l in layers {
        let weight = l.weight.clamp(0.0, 1.0);
        let Some(a) = l.alpha.as_deref().filter(|a| a.len() == n * n) else { continue };
        if weight <= 0.0 {
            continue;
        }
        // Texel x samples the layer at texel coordinate x + s (centres at integers).
        let split = |s: f64| {
            let s = s.rem_euclid(1.0) * n as f64;
            let f = s.floor();
            ((f as usize) % n, (s - f) as f32)
        };
        let ((ix, fx), (iy, fy)) = (split(l.shift[0]), split(l.shift[1]));
        for y in 0..n {
            let (r0, r1) = (((y + iy) % n) * n, ((y + iy + 1) % n) * n);
            let row = &mut t[y * n..(y + 1) * n];
            for (x, v) in row.iter_mut().enumerate() {
                let (c0, c1) = ((x + ix) % n, (x + ix + 1) % n);
                let top = f32::from(a[r0 + c0]) + (f32::from(a[r0 + c1]) - f32::from(a[r0 + c0])) * fx;
                let bottom = f32::from(a[r1 + c0]) + (f32::from(a[r1 + c1]) - f32::from(a[r1 + c0])) * fx;
                let alpha = (top + (bottom - top) * fy) / 255.0;
                *v *= 1.0 - alpha * weight;
            }
        }
    }
    t
}

/// The `pct` percentile (0..100) of the transmittance `t`, from a 256-bin histogram of its
/// 8-bit values: the largest value inside the bin where the count reaches `pct` % of the
/// texels (so a uniform map's reference is its own value exactly). 0 for an empty map.
pub fn percentile(t: &[f32], pct: f32) -> f32 {
    if t.is_empty() {
        return 0.0;
    }
    let bin = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as usize;
    let mut hist = [0usize; 256];
    for &v in t {
        hist[bin(v)] += 1;
    }
    let need = ((pct.clamp(0.0, 100.0) / 100.0 * t.len() as f32).ceil() as usize).clamp(1, t.len());
    let mut count = 0;
    let mut at = 255;
    for (b, n) in hist.iter().enumerate() {
        count += n;
        if count >= need {
            at = b;
            break;
        }
    }
    t.iter().filter(|&&v| bin(v) == at).fold(0.0f32, |m, &v| m.max(v.clamp(0.0, 1.0)))
}

/// The reference a composition uses: the map's own, `raw`, approached from the previous
/// composition's (`prev`, `None` after a discontinuity: the map's own at once) by
/// [`REFERENCE_SMOOTHING`] of the way, taken whole within [`REFERENCE_SETTLE`].
pub fn smoothed_reference(prev: Option<f32>, raw: f32) -> f32 {
    match prev {
        Some(p) if (raw - p).abs() > REFERENCE_SETTLE => p + (raw - p) * REFERENCE_SMOOTHING,
        _ => raw,
    }
}

/// The relative transmittance `min(K / reference, 1)` in 8 bits (module notes, "Relative
/// transmittance"). Under [`REFERENCE_FLOOR`] the darkening `1 - min(K / reference, 1)` is
/// scaled by `reference / REFERENCE_FLOOR`, so it fades out as the map's clearest texels
/// close up too, and a reference of 0 (an opaque map) casts nothing.
pub fn relative(t: &[f32], reference: f32) -> Vec<u8> {
    if reference <= 0.0 {
        return vec![255; t.len()];
    }
    let depth = (reference / REFERENCE_FLOOR).min(1.0);
    t.iter()
        .map(|&k| {
            let r = (k / reference).min(1.0);
            ((1.0 - (1.0 - r) * depth).clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect()
}

/// The reference a composition was made with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reference {
    /// The map's own percentile.
    pub raw: f32,
    /// What the map was divided by ([`smoothed_reference`]).
    pub used: f32,
}

/// A finished composition: the image's texels, and its reference (`None`: absolute, the
/// rule switched off by `NVF_PLAY_DEBUG_CLOUD_REFERENCE=off`).
#[derive(Clone, Debug)]
pub struct Composition {
    pub texels: Vec<u8>,
    pub reference: Option<Reference>,
    /// The absolute product in 8 bits and its mean cover, when asked for (the volume).
    pub absolute: Option<Absolute>,
}

/// The volume's cover's layers at a fixed sampling phase: every
/// shift 0, so each weather's own mean (and the composition's) is a property of the weather's
/// textures and weights, not of where the layers' scrolls happen to stand, and a new composition
/// of the same weathers returns the same numbers (two half-opaque layers cover more of the sky as
/// their offsets separate; a recomposition must not step the amount).
pub fn fixed_phase(layers: Vec<ComposeLayer>) -> Vec<ComposeLayer> {
    layers.into_iter().map(|l| ComposeLayer { shift: [0.0; 2], ..l }).collect()
}

/// The volume's cover of a composition: the mean of `1 - T` of the absolute transmittance and
/// per cover layer its mean visible share ([`compose_cover`]); no texels.
#[derive(Clone, Debug)]
pub struct Absolute {
    pub mean_cover: f32,
    pub visible: Vec<f32>,
    /// Per set (0 the current weather, 1 the one fading out) the same at full weight from that
    /// set's layers alone: its mean cover and each layer's visible share (0 for the other set's
    /// layers) (a weather's own cover, [`WeatherCover`]).
    pub sets: [(f32, Vec<f32>); 2],
}

/// What the volume's cover is composed from: the shadows' layers (a weather with a texture
/// the volume cannot read gets none: [`volume_alpha_ok`]), each layer's draw rank on the dome
/// (`2 × layer + set`, [`active_slots`]; higher in front) and its set.
#[derive(Clone, Debug)]
pub struct CoverInput {
    pub layers: Vec<ComposeLayer>,
    pub ranks: Vec<u8>,
    pub sets: Vec<u8>,
}

/// The flat layers' own mean transmittance, `Π (1 - mean alpha × weight)` over the drawn layers (each
/// its texture's mean alpha and its weight on the dome): the mean of the absolute product over every
/// offset of the layers against each other, so it does not move as they drift, and it has nothing of
/// the image's clear percentile in it (uniform cover of any density is a fully lit image). 1 for
/// none. The light in the air's deck's share reads it (medium_air_grid.rs `flat_deck`) [design,
/// 2026-11-24].
pub fn flat_transmittance(layers: impl Iterator<Item = (f32, f32)>) -> f32 {
    layers.map(|(alpha, weight)| 1.0 - (alpha * weight).clamp(0.0, 1.0)).product()
}

/// The flat layers' deck ([`CloudShadowWork::deck`]): their own mean transmittance and the cloud plane's
/// height (Bevy y), under which it stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlatDeck {
    pub transmittance: f32,
    pub plane_height: f32,
}

/// The mean of a texture's alpha (0..1), 0 for an empty one.
fn mean_alpha(a: &[u8]) -> f32 {
    if a.is_empty() {
        return 0.0;
    }
    (a.iter().map(|&v| u64::from(v)).sum::<u64>() as f64 / (a.len() as f64 * 255.0)) as f32
}

/// Compose `layers` relative to the `pct` percentile of their product (absolute when
/// `None`), the reference smoothed from `prev` (the last composition's, [`smoothed_reference`])
/// and never above this map's own: a falling reference is taken at once, so cover that has
/// become uniform (a weather fading in) casts nothing in its first composition, and only a
/// rising one is smoothed.
#[cfg_attr(not(test), allow(dead_code))] // the producer composes through `compose_parts`
pub fn compose_relative(layers: &[ComposeLayer], pct: Option<f32>, prev: Option<f32>) -> Composition {
    compose_parts(layers, pct, prev, true, None)
}

/// The parts a composition is asked for: the shadows' relative image from `layers` (as
/// [`compose_relative`]; empty texels when `with_relative` is false: Cloud shadows off) and
/// the volume's cover from `cover` (when given).
pub fn compose_parts(layers: &[ComposeLayer], pct: Option<f32>, prev: Option<f32>, with_relative: bool, cover: Option<&CoverInput>) -> Composition {
    let absolute = cover.map(compose_cover);
    if !with_relative {
        return Composition { texels: Vec::new(), reference: None, absolute };
    }
    let t = compose_transmission(layers);
    let Some(pct) = pct else { return Composition { texels: quantize(&t), reference: None, absolute } };
    let raw = percentile(&t, pct);
    let used = smoothed_reference(prev, raw).min(raw);
    Composition { texels: relative(&t, used), reference: Some(Reference { raw, used }), absolute }
}

/// A layer's alpha (0..1) at every texel of the image, sampled as [`compose_transmission`]
/// samples it; `None` for a layer it leaves out (no alpha, or no weight).
fn sampled_alpha(l: &ComposeLayer) -> Option<Vec<f32>> {
    let n = MAP_SIZE;
    let a = l.alpha.as_deref().filter(|a| a.len() == n * n)?;
    if l.weight.clamp(0.0, 1.0) <= 0.0 {
        return None;
    }
    let split = |s: f64| {
        let s = s.rem_euclid(1.0) * n as f64;
        let f = s.floor();
        ((f as usize) % n, (s - f) as f32)
    };
    let ((ix, fx), (iy, fy)) = (split(l.shift[0]), split(l.shift[1]));
    let mut out = vec![0.0f32; n * n];
    for y in 0..n {
        let (r0, r1) = (((y + iy) % n) * n, ((y + iy + 1) % n) * n);
        for (x, v) in out[y * n..(y + 1) * n].iter_mut().enumerate() {
            let (c0, c1) = ((x + ix) % n, (x + ix + 1) % n);
            let top = f32::from(a[r0 + c0]) + (f32::from(a[r0 + c1]) - f32::from(a[r0 + c0])) * fx;
            let bottom = f32::from(a[r1 + c0]) + (f32::from(a[r1 + c1]) - f32::from(a[r1 + c0])) * fx;
            *v = (top + (bottom - top) * fy) / 255.0;
        }
    }
    Some(out)
}

/// The volume's cover: the absolute transmittance `Π (1 - alpha × weight)` (the same
/// arithmetic as [`compose_transmission`]), its mean cover, and each layer's mean visible
/// share: per texel its `alpha × weight` times the transmission of the layers in front of it
/// on the dome (higher rank), as the flat layers are blended over each other.
pub fn compose_cover(cover: &CoverInput) -> Absolute {
    let alphas: Vec<Option<(Vec<f32>, f32)>> = cover.layers.iter().map(|l| sampled_alpha(l).map(|a| (a, l.weight.clamp(0.0, 1.0)))).collect();
    let all: Vec<Option<(&[f32], f32)>> = alphas.iter().map(|s| s.as_ref().map(|(a, w)| (a.as_slice(), *w))).collect();
    let (mean, visible) = cover_parts(&all, &cover.ranks);
    // Each set at full weight from its own layers.
    let set = |s: u8| -> (f32, Vec<f32>) {
        let own: Vec<Option<(&[f32], f32)>> = alphas.iter().enumerate().map(|(i, a)| a.as_ref().filter(|_| cover.sets.get(i) == Some(&s)).map(|(a, _)| (a.as_slice(), 1.0))).collect();
        cover_parts(&own, &cover.ranks)
    };
    Absolute { mean_cover: mean, visible, sets: [set(0), set(1)] }
}

/// The mean cover of `layers` (each its sampled alpha and weight, or none) and each one's mean
/// visible share, drawn over each other by `ranks` ([`compose_cover`]).
fn cover_parts(layers: &[Option<(&[f32], f32)>], ranks: &[u8]) -> (f32, Vec<f32>) {
    let n = MAP_SIZE;
    let mut t = vec![1.0f32; n * n];
    for (a, weight) in layers.iter().flatten() {
        for (v, &alpha) in t.iter_mut().zip(a.iter()) {
            *v *= 1.0 - alpha * weight;
        }
    }
    let mut order: Vec<usize> = (0..layers.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(ranks.get(i).copied().unwrap_or(0)));
    let mut front = vec![1.0f32; n * n];
    let mut visible = vec![0.0f32; layers.len()];
    for i in order {
        let Some((a, weight)) = &layers[i] else { continue };
        let mut sum = 0.0f64;
        for (f, &alpha) in front.iter_mut().zip(a.iter()) {
            let c = alpha * weight;
            sum += f64::from(c * *f);
            *f *= 1.0 - c;
        }
        visible[i] = (sum / (n * n) as f64) as f32;
    }
    (mean_cover(&t), visible)
}

/// The mean of `1 - T` over a transmittance map, 0..1 (0 for an empty one).
pub fn mean_cover(t: &[f32]) -> f32 {
    if t.is_empty() {
        return 0.0;
    }
    (t.iter().map(|&v| f64::from(1.0 - v.clamp(0.0, 1.0))).sum::<f64>() / t.len() as f64) as f32
}

/// The wind's accumulated displacement: the integral of the drift direction (Bevy x, z,
/// unit) over the cloud clock's seconds, in f64. Every layer's drift is its scroll rate
/// times this, so a direction change alters the velocity from that moment only: the
/// pattern already drifted and the layers' phases against each other stay where they are.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindPhase {
    /// The clock at the last step (`None` before the first).
    last: Option<f64>,
    /// Seconds along x and z.
    pub phase: [f64; 2],
}

impl WindPhase {
    /// Advance to `secs` with the direction now, `dir`; the first step counts from 0 (as
    /// if the wind had always blown this way, which a frozen clock keeps).
    pub fn advance(&mut self, secs: f64, dir: Vec2) -> [f64; 2] {
        let dt = secs - self.last.unwrap_or(0.0);
        self.phase[0] += dt * f64::from(dir.x);
        self.phase[1] += dt * f64::from(dir.y);
        self.last = Some(secs);
        self.phase
    }
}

/// The texture-space drift of a layer scrolling `scroll` repeats per second after the
/// wind's displacement `phase` (texture u, v = world x, z), wrapped to 0..1 on each axis.
/// Worked unwrapped in f64, so a direction off the axes has no jump when the drift passes
/// a whole repeat.
pub fn drift(phase: [f64; 2], scroll: f64) -> [f64; 2] {
    [(phase[0] * scroll).rem_euclid(1.0), (phase[1] * scroll).rem_euclid(1.0)]
}

/// `uv_offset` for the reference layer: the ground pattern moves downwind as the dome's
/// offset grows, so its texture offset runs against the wind's displacement.
pub fn uv_offset(phase: [f64; 2], ref_scroll: f64) -> Vec2 {
    let [u, v] = drift([-phase[0], -phase[1]], ref_scroll);
    Vec2::new(u as f32, v as f32)
}

/// Where layer `scroll` is sampled against the image composed for the reference layer
/// `ref_scroll`: its drift beyond the reference's.
pub fn layer_shift(phase: [f64; 2], scroll: f64, ref_scroll: f64) -> [f64; 2] {
    drift([-phase[0], -phase[1]], scroll - ref_scroll)
}

/// Distance between two wrapped texture coordinates, in texels.
fn wrapped_texels(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(1.0);
    d.min(1.0 - d) * MAP_SIZE as f64
}

// The shader contract's reference: C0's shader implements it, its tests may call it.
#[cfg_attr(not(test), allow(dead_code))]
/// Reference of the shader contract (C0): where a receiver at Bevy `world` samples the
/// image, for `to_light` the unit direction towards the light. `None`: the light is at or
/// below [`MIN_LIGHT_Y`], or the receiver is at or above the plane (fully lit), as
/// sun_visibility.wesl's `P.y >= height`. A UV reference only: the shader's fade near the
/// horizon (`CLOUD_FADE_FROM` to `CLOUD_FADE_TO`) is not modelled here.
pub fn projected_uv(world: Vec3, to_light: Vec3, plane_height: f32, uv_scale: Vec2, uv_offset: Vec2) -> Option<Vec2> {
    if to_light.y <= MIN_LIGHT_Y || world.y >= plane_height {
        return None;
    }
    let t = (plane_height - world.y) / to_light.y;
    let hit = world + to_light * t;
    Some(Vec2::new(hit.x, hit.z) * uv_scale + uv_offset)
}

/// The drift direction (Bevy x, z, unit): `direction_deg` (game degrees) when given, else
/// the weather's wind (`tree::wind_vector`).
fn drift_direction(sky: &crate::sky::SkyState, direction_deg: Option<f32>) -> Vec2 {
    let d = match direction_deg {
        // Game (x, y) is Bevy (x, -z).
        Some(deg) => Vec2::new(deg.to_radians().cos(), -deg.to_radians().sin()),
        None => {
            let w = crate::tree::wind_vector(sky);
            Vec2::new(w.x, w.z)
        }
    };
    d.try_normalize().unwrap_or(Vec2::X)
}

// ---------------------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------------------

/// What a read of a cloud texture gives ([`load_alpha`]): its alpha, the volume's look at it, the
/// alpha's mean ([`mean_alpha`]; 0 with none).
type AlphaRead = (Option<Arc<[u8]>>, Option<bool>, f32);

/// A cloud texture's alpha: being read, or read (`None`: missing or without alpha).
enum Alpha {
    /// The alpha, and whether the volume can read the texture ([`volume_alpha_ok`]; `None`: not
    /// looked at, the volume did not want the cover when the read started).
    Loading(Task<AlphaRead>),
    /// Read without that look: all Cloud shadows need. While the volume wants the cover, the file
    /// is read once more for it.
    Unchecked(Option<Arc<[u8]>>, Option<Task<AlphaRead>>),
    Ready(Option<Arc<[u8]>>),
}

/// What an image holds (or a running job will).
#[derive(Clone, Debug, PartialEq)]
struct Composed {
    layers: Vec<ComposedLayer>,
    ref_scroll: f64,
    /// The drift direction when composed (logged).
    dir: Vec2,
    /// The reference the image was made with (`None`: absolute, or not composed yet).
    reference: Option<Reference>,
}

#[derive(Clone, Debug, PartialEq)]
struct ComposedLayer {
    texture: String,
    weight: f32,
    /// The texture's alpha was in.
    covered: bool,
    /// Repeats per second (logged).
    scroll: f64,
    shift: [f64; 2],
}

impl Composed {
    /// Whether `want` differs from this enough to compose again.
    fn differs(&self, want: &Composed) -> bool {
        // The direction is not compared: a change of it moves the shifts on from where they
        // are (WindPhase), so they say when to compose again.
        self.layers.len() != want.layers.len()
            || (self.ref_scroll - want.ref_scroll).abs() > 1e-9
            || self.layers.iter().zip(&want.layers).any(|(a, b)| {
                a.texture != b.texture
                    || a.covered != b.covered
                    || (a.weight - b.weight).abs() >= WEIGHT_STEP
                    || (a.weight != b.weight && b.weight >= 1.0)
                    || wrapped_texels(a.shift[0], b.shift[0]) >= RECOMPOSE_STEP_TEXELS
                    || wrapped_texels(a.shift[1], b.shift[1]) >= RECOMPOSE_STEP_TEXELS
            })
    }

    /// Whether the volume's cover must be composed again for `want`:
    /// as [`Composed::differs`] less the layers' shifts, since the cover is composed at a
    /// fixed sampling phase ([`fixed_phase`]) and does not change as the layers drift.
    fn cover_differs(&self, want: &Composed) -> bool {
        self.layers.len() != want.layers.len()
            || (self.ref_scroll - want.ref_scroll).abs() > 1e-9
            || self.layers.iter().zip(&want.layers).any(|(a, b)| {
                a.texture != b.texture || a.covered != b.covered || (a.weight - b.weight).abs() >= WEIGHT_STEP || (a.weight != b.weight && b.weight >= 1.0) || a.scroll != b.scroll
            })
    }

    /// The reference used has not reached the map's own yet ([`REFERENCE_SETTLE`]).
    fn unsettled(&self) -> bool {
        self.reference.is_some_and(|r| r.used != r.raw)
    }
}

struct Job {
    task: Task<Composition>,
    landed: Landed,
}

/// What a composition was started for (its [`Job`] less the task): what it composes, its
/// generation and, for the volume's cover, its layers' slots, weathers and set weights.
struct Landed {
    composed: Composed,
    /// [`CloudShadowWork::generation`] when it started.
    generation: u64,
    /// Each layer's (set, WTHR layer), for the volume's colour weights (empty without it).
    slots: Vec<(usize, usize)>,
    /// The weathers of sets 0 and 1 when it started (form IDs), and their weights on the dome
    /// then (a set at weight 0 has no layers in the job, so no cover of its own from it).
    weathers: [Option<u32>; 2],
    set_weights: [f32; 2],
}

/// (current weather, the weather fading out) by form ID.
type WeatherSet = (u32, Option<u32>);

/// Whether going from `old` to `new` replaced the weather set (a discontinuity): a new
/// current weather, or another weather fading out. A fade's end (the faded-out one gone,
/// the current kept) is not one.
pub fn weather_replaced(old: WeatherSet, new: WeatherSet) -> bool {
    old.0 != new.0 || (new.1.is_some() && new.1 != old.1)
}

/// The producer's working state (private to this module).
#[derive(Resource, Default)]
pub struct CloudShadowWork {
    image: Option<Handle<Image>>,
    alphas: HashMap<String, Alpha>,
    /// The mean alpha of each texture read ([`mean_alpha`], worked out by the read).
    alpha_means: HashMap<String, f32>,
    /// The drawn layers' deck while Cloud shadows is on ([`CloudShadowWork::deck`]).
    flat: Option<FlatDeck>,
    job: Option<Job>,
    /// What the image holds.
    shown: Option<Composed>,
    /// The volume's cover composition in flight, in a slot of its own
    /// (Cloud shadows' `job` never waits for it), and what the cover was composed from.
    cover_job: Option<Job>,
    cover_shown: Option<Composed>,
    /// When the last composition started (cloud clock's real time).
    composed_at: f64,
    /// (worldspace, Bevy y of the ground reference).
    ground: Option<(Option<u32>, f32)>,
    /// Why the output is `None`, as last logged.
    off_reason: Option<&'static str>,
    /// Layer count, weights (in [`LOG_WEIGHT_STEP`] steps) and textures last logged.
    logged: Option<(usize, Vec<i32>, Vec<String>)>,
    /// The discontinuity generation (module notes, "Validity").
    generation: u64,
    /// The weather set last seen.
    weather_set: Option<WeatherSet>,
    /// The wind's accumulated displacement.
    wind: WindPhase,
    /// Whether a composition for the volume ([`CloudCoverWanted`]) is in (its mean, shares and
    /// weathers below).
    absolute_shown: bool,
    /// Whether the relative image holds the shown composition (with Cloud shadows off a
    /// composition makes none).
    relative_shown: bool,
    /// Its mean cover.
    cover_mean: f32,
    /// The textures the volume cannot read as plain 2D alpha ([`volume_alpha_ok`]).
    unreadable: HashSet<String>,
    /// The active layer's texture that keeps the volume's cover back ([`CloudCoverRefused`]).
    cover_refused: Option<String>,
    /// There is no cloud layer to compose the volume's cover from ([`CloudCoverEmpty`]).
    cover_empty: bool,
    /// The weathers' own covers composed so far, at most the last job's
    /// two: a weather whose textures are still being read has none until a job composes it.
    weather_covers: Vec<WeatherCover>,
    /// An active layer of either weather is still being read. Its measurement is
    /// unavailable; publication can briefly hold the known outgoing endpoint.
    cover_pending: bool,
    /// When the last composition started by the real clock (the volume's cover recomposes on
    /// it, so a paused game does not stretch the interval; the shadows keep `composed_at`).
    composed_at_real: f64,
    /// The volume's pending reads: since when (real seconds) each has been read and whether it
    /// was logged as stuck. Empty with the volume off.
    pending_reads: HashMap<String, (f64, bool)>,
    /// [`CloudCoverOutput::layer_cover`] and its weathers, of the shown composition.
    layer_cover: [[f32; 4]; 2],
    layer_weathers: [Option<u32>; 2],
    /// The covering layers' drift rate ([`cover_scroll`]) of the shown composition.
    noise_scroll: Option<f64>,
    /// The projection now (uv scale, plane height), while the volume wants it.
    cover_at: Option<(Vec2, f32)>,
    /// The volume's drift ([`advance_drift`]) and the wind's displacement it was last advanced to
    /// (`None` while the volume does not want the cover).
    drift: [f64; 2],
    drift_phase: Option<[f64; 2]>,
}

impl CloudShadowWork {
    /// The flat layers' own mean transmittance ([`flat_transmittance`]) and their plane while Cloud
    /// shadows is on and the dome draws layers whose alpha is read: with the dome's own weights each
    /// frame, so through a weather fade it goes from the one weather's to the other's as the dome's
    /// layers do, with no smoothing and no composition behind it. While a texture of a fade is being
    /// read the last one stays; a weather replaced at once and another worldspace clear it with the
    /// output ([`CloudShadowWork::clear_output`]).
    pub fn deck(&self) -> Option<FlatDeck> {
        self.flat
    }

    /// [`CloudShadowWork::deck`] for the drawn `layers` this frame.
    fn follow_deck(&mut self, layers: &[LayerInput], plane_height: f32, on: bool, loading: bool) {
        if !on {
            self.flat = None;
            return;
        }
        if loading {
            return;
        }
        let transmittance = flat_transmittance(layers.iter().filter_map(|l| Some((*self.alpha_means.get(&l.texture)?, l.weight))));
        self.flat = Some(FlatDeck { transmittance, plane_height });
    }

    /// Drop the image, the composition and what it showed, and publish `None` (the
    /// plumbing then gives a neutral cloud while the switch stays on).
    fn clear_output(&mut self, published: &mut CloudShadowPublished, images: &mut Assets<Image>) {
        if let Some(h) = self.image.take() {
            images.remove(&h);
        }
        self.relative_shown = false;
        self.drop_absolute();
        self.flat = None;
        self.job = None;
        self.shown = None;
        self.logged = None;
        if published.0.is_some() {
            published.0 = None;
        }
    }

    /// Drop the volume's cover (it is then unpublished, [`publish_cloud_cover`]).
    fn drop_absolute(&mut self) {
        self.absolute_shown = false;
        self.cover_at = None;
        self.noise_scroll = None;
        self.weather_covers.clear();
        self.cover_job = None;
        self.cover_shown = None;
    }

    /// A discontinuity: results of jobs started before it are dropped (the running job's
    /// handle too, and [`CloudShadowWork::install`] rejects an older generation).
    fn advance_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.job = None;
        self.cover_job = None;
        // The reference starts again from the next composition's own (no smoothing across).
        if let Some(shown) = self.shown.as_mut() {
            shown.reference = None;
        }
    }

    /// The worldspace now (`ws`): another than the ground reference's is a discontinuity
    /// that clears the output and the reference. True when it did.
    fn note_world(&mut self, ws: Option<u32>, published: &mut CloudShadowPublished, images: &mut Assets<Image>) -> bool {
        if !self.ground.is_some_and(|(w, _)| w != ws) {
            return false;
        }
        self.advance_generation();
        self.clear_output(published, images);
        self.ground = None;
        info!("cloud shadows: another worldspace: output cleared until it is composed again");
        true
    }

    /// The weather set now: replaced, it is a discontinuity; replaced at once (no fade),
    /// the dome changed at once too, and the output is cleared. True when replaced.
    fn note_weather(&mut self, set: WeatherSet, published: &mut CloudShadowPublished, images: &mut Assets<Image>) -> bool {
        let replaced = self.weather_set.is_some_and(|old| weather_replaced(old, set));
        self.weather_set = Some(set);
        if replaced {
            self.advance_generation();
            if set.1.is_none() {
                self.clear_output(published, images);
            }
        }
        replaced
    }

    /// A finished composition of `generation` into the image: dropped (false) when it is
    /// of an older generation or the image is gone.
    fn install(&mut self, generation: u64, composed: Composed, texels: Vec<u8>, images: &mut Assets<Image>) -> bool {
        if generation != self.generation {
            return false;
        }
        let Some(mut img) = self.image.as_ref().and_then(|h| images.get_mut(h)) else { return false };
        img.data = Some(texels);
        self.shown = Some(composed);
        true
    }

    /// Cloud shadows' finished composition (from their own slot):
    /// into the image with its reference ([`CloudShadowWork::install`]); dropped (false) when of an
    /// older generation or the image is gone.
    fn land_shadows(&mut self, landed: Landed, done: Composition, images: &mut Assets<Image>) -> bool {
        let composed = Composed { reference: done.reference, ..landed.composed };
        let installed = self.install(landed.generation, composed, done.texels, images);
        if installed {
            self.relative_shown = true;
        }
        installed
    }

    /// The volume's finished cover composition (from its own slot): what it was composed from and
    /// its cover ([`CloudShadowWork::install_cover`]); Cloud shadows' state untouched; dropped
    /// (false) when of an older generation.
    fn land_cover(&mut self, landed: Landed, done: Composition) -> bool {
        if landed.generation != self.generation {
            return false;
        }
        let scrolls: Vec<f64> = landed.composed.layers.iter().map(|l| l.scroll).collect();
        self.install_cover(done.absolute, &landed.slots, landed.weathers, landed.set_weights, &scrolls);
        self.cover_shown = Some(landed.composed);
        true
    }

    /// A finished composition's absolute cover for the volume: its mean, its layers' visible
    /// shares (by set and layer) and the weathers it was composed for, and each weather's own
    /// cover (a set that had a weight on the dome when the job started), which
    /// [`publish_cloud_cover`] looks up by the live sky's weathers at publication:
    /// the covers of weathers outside this job are dropped.
    fn install_cover(&mut self, absolute: Option<Absolute>, slots: &[(usize, usize)], weathers: [Option<u32>; 2], set_weights: [f32; 2], scrolls: &[f64]) {
        self.absolute_shown = false;
        let Some(abs) = absolute else { return };
        self.cover_mean = abs.mean_cover;
        self.absolute_shown = true;
        let mut cover = [[0.0f32; 4]; 2];
        for (&(set, layer), &v) in slots.iter().zip(&abs.visible) {
            if let Some(c) = cover.get_mut(set).and_then(|s| s.get_mut(layer)) {
                *c += v;
            }
        }
        self.layer_cover = cover;
        self.layer_weathers = weathers;
        self.noise_scroll = cover_scroll(&abs.visible, scrolls);
        self.weather_covers.retain(|w| weathers.contains(&Some(w.id)));
        for (s, (id, (mean, visible))) in weathers.iter().zip(&abs.sets).enumerate() {
            let Some(id) = *id else { continue };
            if set_weights[s] <= 0.0 {
                continue;
            }
            let mut shares = [0.0f32; 4];
            for (&(set, layer), &v) in slots.iter().zip(visible) {
                if let Some(c) = shares.get_mut(layer).filter(|_| set == s) {
                    *c += v;
                }
            }
            self.weather_covers.retain(|w| w.id != id);
            self.weather_covers.push(WeatherCover { id, mean: *mean, shares });
        }
    }

    /// Drop the image and the composition (and the alphas unless `keep_alphas`), and
    /// publish `None` (logged once per reason).
    fn go_off(&mut self, reason: &'static str, keep_alphas: bool, published: &mut CloudShadowPublished, images: &mut Assets<Image>) {
        self.clear_output(published, images);
        if !keep_alphas {
            self.alphas.clear();
            self.alpha_means.clear();
            self.unreadable.clear();
            self.cover_refused = None;
            self.cover_pending = false;
            self.pending_reads.clear();
        }
        if self.off_reason != Some(reason) {
            self.off_reason = Some(reason);
            info!("cloud shadows: none ({reason})");
        }
    }
}

/// The image, fully lit. Its sampler is left at the default: sun_visibility.rs samples it
/// with the plumbing's own repeating, linear sampler, and a sampler of the image's own
/// would be made again at every upload (bevy_render `texture/gpu_image.rs`).
fn new_image() -> Image {
    Image::new_fill(
        Extent3d { width: MAP_SIZE as u32, height: MAP_SIZE as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[255],
        TextureFormat::R8Unorm,
        RenderAssetUsages::default(),
    )
}

/// A texture's alpha, once read (`None` too while it is being read).
fn alpha_of(alphas: &HashMap<String, Alpha>, path: &str) -> Option<Arc<[u8]>> {
    match alphas.get(path) {
        Some(Alpha::Ready(a) | Alpha::Unchecked(a, _)) => a.clone(),
        _ => None,
    }
}

/// Read and decode one cloud texture's alpha off the main thread; with it, for the `volume`
/// alone, whether that can read it ([`volume_alpha_ok`]: it may look through the alpha); and the
/// alpha's mean.
fn load_alpha(data: &GameData, path: &str, volume: bool) -> Task<AlphaRead> {
    let (data, path) = (data.clone(), path.to_string());
    nvf_assets::read_pool::read_pool().spawn(async move {
        let resolved = nvf_assets::texture::resolved_path(&path, |q| data.vfs.exists(q));
        let Some(bytes) = data.read(&resolved) else {
            debug!("cloud shadows: {path}: in no archive: no cover");
            return (None, volume.then_some(false), 0.0);
        };
        // One parse for the alpha and the volume's checks.
        let Ok(dds) = fnv_dds::Dds::parse(&bytes) else {
            info!("cloud shadows: {path}: no alpha channel read: no cover");
            return (None, volume.then_some(false), 0.0);
        };
        let map = nvf_assets::texture::alpha_map_of(&dds);
        let volume = volume.then(|| volume_alpha_of(&dds, map.as_ref()));
        let Some(map) = map else {
            info!("cloud shadows: {path}: no alpha channel read: no cover");
            return (None, volume, 0.0);
        };
        let alpha = resample_alpha(&map.alpha, map.width, map.height);
        let mean = mean_alpha(&alpha);
        (Some(Arc::from(alpha)), volume, mean)
    })
}

/// Make, compose and publish the cloud transmittance (runs while [`CloudShadows`] is on, or
/// the volume wants the cover: [`producer_wanted`]). The relative output is published only
/// while Cloud shadows is on; the absolute image is made only while the volume wants it.
#[allow(clippy::too_many_arguments)]
pub fn produce_cloud_shadows(
    mut work: ResMut<CloudShadowWork>,
    mut published: ResMut<CloudShadowPublished>,
    mut images: ResMut<Assets<Image>>,
    sky: Option<Res<crate::sky::SkyState>>,
    data: Option<Res<GameData>>,
    world: Option<Res<crate::world::WorldData>>,
    (time, real): (Res<Time>, Res<Time<Real>>),
    cam: Query<&Transform, With<crate::fly::FlyCam>>,
    (dome, finished): (Query<&crate::sky::CloudLayer>, Query<(), With<crate::sky::SkyDome>>),
    switches: (Option<Res<CloudShadows>>, Option<Res<CloudCoverWanted>>),
) {
    let work = &mut *work;
    work.cover_empty = false;
    let shadows_on = switches.0.is_some_and(|s| s.enabled);
    let cover_on = switches.1.is_some_and(|v| v.0);
    let Some(sky) = sky else { return };
    // The wind's displacement runs on whatever happens below (indoors too).
    let cal = calibration();
    let secs = cloud_seconds(&time);
    let dir = drift_direction(&sky, cal.direction_deg);
    let phase = work.wind.advance(secs, dir);
    if cover_on {
        // The covering layers' rate (a transparent reference layer never sets it).
        let rate = work.noise_scroll.or(work.cover_shown.as_ref().or(work.shown.as_ref()).map(|s| s.ref_scroll)).unwrap_or(0.0);
        work.drift = advance_drift(work.drift, work.drift_phase, phase, rate);
        work.drift_phase = Some(phase);
    } else {
        work.drift_phase = None;
    }
    // Discontinuities first: nothing older than them reaches the image.
    let ws = world.as_deref().map(|w| w.world);
    work.note_world(ws, &mut published, &mut images);
    work.note_weather((sky.weather.form_id, sky.prev.as_ref().map(|p| p.form_id)), &mut published, &mut images);
    if sky.interior.is_some() {
        work.go_off("indoors", false, &mut published, &mut images);
        return;
    }
    let Some(data) = data else { return };
    let mut drawn = [false; 4];
    for cl in &dome {
        if let Some(d) = drawn.get_mut(cl.layer()) {
            *d = true;
        }
    }
    let layers = active_layers_with(&sky.weather, sky.prev.as_ref(), sky.fade, drawn, |w, layer| f64::from(sky.cloud_scroll(w, layer)));
    if layers.is_empty() {
        // Nothing to compose for the volume, rather than a sky still being set up: its dome and
        // its cloud shapes (none where `clouds.nif` is missing or has none) are spawned together
        // (sky.rs `setup_sky`). Without a texture there is nothing either way.
        let textured = |w: &Weather| w.cloud_textures.iter().any(Option::is_some);
        work.cover_empty = cover_on && (!finished.is_empty() || !std::iter::once(&sky.weather).chain(&sky.prev).any(textured));
        work.go_off("no cloud layers", false, &mut published, &mut images);
        return;
    }
    // The ground reference: the camera's height when the worldspace's first output is
    // made, held until the worldspace changes (`note_world`), so the plane never follows
    // the camera.
    if work.ground.is_none() {
        let Ok(cam) = cam.single() else { return };
        work.ground = Some((ws, (cam.translation.y / GROUND_SNAP).round() * GROUND_SNAP));
    }
    let ground = work.ground.map_or(0.0, |(_, g)| g);

    // A fade announces its destination before the dome draws it. Get the reads
    // going now; zero-weight layers never enter the terrain shadow composition.
    let prefetch = if cover_on && sky.prev.is_some() && sky.fade == 0.0 {
        active_layers_with(&sky.weather, None, 1.0, drawn, |w, layer| f64::from(sky.cloud_scroll(w, layer)))
    } else {
        Vec::new()
    };
    for l in layers.iter().chain(&prefetch) {
        work.alphas.entry(l.texture.clone()).or_insert_with(|| Alpha::Loading(load_alpha(&data, &l.texture, cover_on)));
    }
    work.alphas.retain(|path, _| layers.iter().chain(&prefetch).any(|l| &l.texture == path));
    work.alpha_means.retain(|path, _| layers.iter().chain(&prefetch).any(|l| &l.texture == path));
    if cover_on {
        work.unreadable.retain(|path| layers.iter().chain(&prefetch).any(|l| &l.texture == path));
    }
    for (path, a) in work.alphas.iter_mut() {
        let read = match a {
            Alpha::Loading(task) => check_ready(task),
            // Read for Cloud shadows alone, and now the volume wants it: looked at for that too,
            // the shadows going on with what they have.
            Alpha::Unchecked(None, _) if cover_on => Some((None, Some(false), 0.0)),
            Alpha::Unchecked(_, task) if cover_on => check_ready(task.get_or_insert_with(|| load_alpha(&data, path, true))),
            _ => None,
        };
        if let Some((Some(_), _, mean)) = &read {
            // The first read's, as the alpha the shadows keep.
            work.alpha_means.entry(path.clone()).or_insert(*mean);
        }
        match read {
            None => {}
            Some((alpha, None, _)) => *a = Alpha::Unchecked(alpha, None),
            Some((read, Some(ok), _)) => {
                // What the shadows have stays theirs, also when the second read fails.
                let alpha = if let Alpha::Unchecked(first, _) = a { first.take() } else { read };
                if ok {
                    work.unreadable.remove(path);
                } else {
                    work.unreadable.insert(path.clone());
                }
                *a = Alpha::Ready(alpha);
            }
        }
    }
    let loading = layers.iter().any(|l| matches!(work.alphas.get(&l.texture), Some(Alpha::Loading(_))));
    // For the volume only (with Cloud shadows alone none of this exists): a read that never
    // finishes keeps its weather flat for the volume, and its path is logged once after
    // STUCK_READ_SECS of real time, each pending read with its own start and flag.
    if cover_on {
        let real_now = real.elapsed_secs_f64();
        let alphas = &work.alphas;
        work.pending_reads.retain(|path, _| !matches!(alphas.get(path), None | Some(Alpha::Ready(_))));
        for (path, a) in alphas {
            if matches!(a, Alpha::Ready(_)) {
                continue;
            }
            let (since, told) = work.pending_reads.entry(path.clone()).or_insert((real_now, false));
            if !*told && real_now - *since >= STUCK_READ_SECS {
                *told = true;
                info!("cloud shadows: {path}: still being read after {STUCK_READ_SECS:.0} s; the volume shows the weather flat until it is");
            }
        }
    } else if !work.pending_reads.is_empty() {
        work.pending_reads.clear();
    }
    // The volume's word on this weather, both sides of a fade (once every texture is read).
    // (None of the volume's bookkeeping while its cover is not wanted.)
    let refused = if cover_on { refused_texture(layers.iter().map(|l| l.texture.as_str()), &work.unreadable).map(str::to_string) } else { None };
    // Only a finished job with every active layer validated can install a weather's
    // own measurement. Publication may briefly hold the outgoing endpoint while a fade starts.
    let pending = cover_on && layers.iter().any(|l| !matches!(work.alphas.get(&l.texture), Some(Alpha::Ready(_))));
    if work.cover_pending != pending {
        work.cover_pending = pending;
    }
    if refused != work.cover_refused {
        if let Some(path) = &refused {
            info!("cloud shadows: {path}: no plain 2D alpha for the volume: no absolute cover, the weather is shown flat");
        }
        work.cover_refused = refused;
    }
    work.follow_deck(&layers, ground + cal.plane_above_ground, shadows_on, loading);
    if !loading && layers.iter().all(|l| alpha_of(&work.alphas, &l.texture).is_none()) {
        // Kept: the textures are not read again while the weather stays.
        work.go_off("no cloud alpha", true, &mut published, &mut images);
        return;
    }
    // While a texture is read, what is shown stays (a weather change), or nothing yet.
    if loading && work.shown.is_none() && work.cover_shown.is_none() {
        return;
    }

    // The reference: the heaviest covering layer (the first of equals).
    let reference = layers
        .iter()
        .filter(|l| alpha_of(&work.alphas, &l.texture).is_some())
        .fold(None::<&LayerInput>, |best, l| if best.is_none_or(|b| l.weight > b.weight) { Some(l) } else { best })
        .unwrap_or(&layers[0]);
    let ref_scroll = reference.scroll;
    let want = Composed {
        layers: layers
            .iter()
            .map(|l| ComposedLayer {
                texture: l.texture.clone(),
                weight: l.weight,
                covered: alpha_of(&work.alphas, &l.texture).is_some(),
                scroll: l.scroll,
                shift: layer_shift(phase, l.scroll, ref_scroll),
            })
            .collect(),
        ref_scroll,
        dir,
        reference: None,
    };

    // Finished compositions, each from its own slot: Cloud
    // shadows' into their image, the volume's cover into its data, each unless a discontinuity came
    // after its start; neither waits for, delays or replaces the other. The shadows' keeps its own
    // guard of a2a2481ce on its own state: while a texture is
    // read and no shadows image is installed, nothing lands there, whatever the volume's cover holds.
    if !loading || work.shown.is_some() {
        if let Some(job) = work.job.as_mut() {
            if let Some(done) = check_ready(&mut job.task) {
                if let Some(job) = work.job.take() {
                    if !work.land_shadows(job.landed, done, &mut images) {
                        debug!("cloud shadows: a composition of an earlier weather or worldspace dropped");
                    }
                }
            }
        }
    }
    // A cover lands only while the volume wants it: a job
    // unfinished at switch-off is dropped, never installed later or reused by a new activation.
    if !cover_on {
        if work.cover_job.is_some() {
            work.cover_job = None;
        }
    } else if let Some(job) = work.cover_job.as_mut() {
        if let Some(done) = check_ready(&mut job.task) {
            if let Some(job) = work.cover_job.take() {
                if !work.land_cover(job.landed, done) {
                    debug!("cloud shadows: a cover composition of an earlier weather or worldspace dropped");
                }
            }
        }
    }
    let now = time.elapsed_secs_f64();
    let now_real = real.elapsed_secs_f64();
    // Cloud shadows' composition, exactly as with the volume off (a2a2481ce): when what is
    // wanted differs from what the image holds, its reference has not settled or the image does
    // not hold it, one at a time, at most every MIN_RECOMPOSE_SECS of the game's clock. No reason
    // of the volume's starts, delays or drops one.
    if shadows_on {
        let due = work.shown.as_ref().is_none_or(|s| s.differs(&want) || s.unsettled()) || !work.relative_shown;
        let shadows_due = work.shown.is_none() || now - work.composed_at >= MIN_RECOMPOSE_SECS;
        if due && !loading && work.job.is_none() && shadows_due {
            if work.image.is_none() {
                work.image = Some(images.add(new_image()));
            }
            let input: Vec<ComposeLayer> =
                layers.iter().zip(&want.layers).map(|(l, w)| ComposeLayer { alpha: alpha_of(&work.alphas, &l.texture), weight: l.weight, shift: w.shift }).collect();
            let (pct, prev) = (reference_percentile(), work.shown.as_ref().and_then(|s| s.reference).map(|r| r.used));
            let task = AsyncComputeTaskPool::get().spawn(async move { compose_parts(&input, pct, prev, true, None) });
            work.job = Some(Job { task, landed: Landed { composed: want.clone(), generation: work.generation, slots: Vec::new(), weathers: [None, None], set_weights: [0.0; 2] } });
            work.composed_at = now;
        }
    }
    // The volume's cover, in its own slot on the real clock (a paused game does not stretch its
    // interval): when what is wanted differs from what it was composed from, its weathers are no
    // longer the sky's, a live weather with a weight has no cover of its own yet
    // (composed at once) or the absolute result is missing (composed at once once the
    // refusal and the pending reads have cleared); never while a texture of either weather is
    // refused or read.
    if cover_on {
        let live = live_weathers(&sky);
        let open = work.cover_refused.is_none();
        let uncomposed = open && live.iter().any(|&(id, w)| w > 0.0 && id.is_some_and(|id| !work.weather_covers.iter().any(|c| c.id == id)));
        let missing = open && !pending && !work.absolute_shown;
        let stale = cover_weathers_stale(work.absolute_shown, work.layer_weathers, [Some(sky.weather.form_id), sky.prev.as_ref().map(|p| p.form_id)]);
        let due = open && (work.cover_shown.as_ref().is_none_or(|s| s.cover_differs(&want)) || uncomposed || missing || stale);
        let urgent = uncomposed || missing || work.cover_shown.is_none();
        if due && !pending && work.cover_job.is_none() && (urgent || now_real - work.composed_at_real >= MIN_RECOMPOSE_SECS) {
            let input: Vec<ComposeLayer> =
                layers.iter().zip(&want.layers).map(|(l, w)| ComposeLayer { alpha: alpha_of(&work.alphas, &l.texture), weight: l.weight, shift: w.shift }).collect();
            let slots = active_slots(&sky.weather, sky.prev.as_ref(), sky.fade, drawn);
            let weathers = [Some(sky.weather.form_id), sky.prev.as_ref().map(|p| p.form_id)];
            let cover = CoverInput {
                ranks: slots.iter().map(|&(set, layer)| (2 * layer + set) as u8).collect(),
                sets: slots.iter().map(|&(set, _)| set as u8).collect(),
                layers: fixed_phase(input),
            };
            let task = AsyncComputeTaskPool::get().spawn(async move { compose_parts(&[], None, None, false, Some(&cover)) });
            let set_weights = match sky.prev.as_ref() {
                Some(_) => [sky.fade.clamp(0.0, 1.0), 1.0 - sky.fade.clamp(0.0, 1.0)],
                None => [1.0, 0.0],
            };
            work.cover_job = Some(Job { task, landed: Landed { composed: want.clone(), generation: work.generation, slots, weathers, set_weights } });
            work.composed_at_real = now_real;
        }
    }

    // Publish what the images hold, with the reference layer's drift now (what Cloud shadows'
    // image holds, or with them off what the volume's cover was composed from).
    let Some(shown) = work.shown.as_ref().or(work.cover_shown.as_ref()) else { return };
    let uv_scale = Vec2::splat(1.0 / cal.repeat);
    let offset_now = uv_offset(phase, shown.ref_scroll);
    let plane_height = ground + cal.plane_above_ground;
    // The shadows' output only while Cloud shadows is on and its image holds the shown
    // composition (with the volume alone nothing of the shadow path may switch on:
    // sun_visibility.rs `cloud_on`).
    if let Some(image) = work.image.as_ref().filter(|_| shadows_on && work.relative_shown) {
        let out = CloudShadowOutput { image: image.clone(), uv_scale, uv_offset: offset_now, plane_height, strength: cal.strength };
        let renew = match &published.0 {
            None => true,
            Some(p) => {
                let moved = (p.uv_offset - out.uv_offset).abs();
                let moved = moved.min(Vec2::ONE - moved);
                p.image != out.image || p.uv_scale != out.uv_scale || p.plane_height != out.plane_height || p.strength != out.strength || moved.max_element() >= PUBLISH_OFFSET_STEP
            }
        };
        if renew {
            published.0 = Some(out);
        }
    }
    work.cover_at = cover_on.then_some((uv_scale, plane_height));
    work.off_reason = None;

    // Log when the layers, their weights (in quarter steps) or the projection change: from Cloud
    // shadows' installed composition while they are on (an
    // early cover must not take the layer key and leave the shadows' real reference unlogged), else
    // from the volume's.
    let logged_from = if shadows_on { work.shown.as_ref() } else { work.cover_shown.as_ref() };
    let Some(shown) = logged_from else { return };
    let offset_now = uv_offset(phase, shown.ref_scroll);
    let key = (
        shown.layers.len(),
        shown.layers.iter().map(|l| (l.weight / LOG_WEIGHT_STEP).round() as i32).collect::<Vec<_>>(),
        shown.layers.iter().map(|l| l.texture.clone()).collect::<Vec<_>>(),
    );
    if work.logged.as_ref() != Some(&key) {
        let weights: Vec<String> = shown.layers.iter().map(|l| format!("{:.2}", l.weight)).collect();
        let speeds: Vec<String> = shown.layers.iter().map(|l| format!("{:.5}", l.scroll)).collect();
        let textures: Vec<&str> = shown.layers.iter().map(|l| l.texture.as_str()).collect();
        let reference = shown.reference.map_or_else(|| "off".to_string(), |r| format!("{:.2}", r.used));
        info!(
            "cloud shadows: layers {} weights [{}] speeds [{}] offset ({:.4},{:.4}) strength {:.2} reference {} plane {:.0} (ground {:.0}) repeat {:.0} drift ({:.3},{:.3}) textures [{}]",
            shown.layers.len(),
            weights.join(", "),
            speeds.join(", "),
            offset_now.x,
            offset_now.y,
            cal.strength,
            reference,
            plane_height,
            ground,
            cal.repeat,
            shown.dir.x,
            shown.dir.y,
            textures.join(", ")
        );
        work.logged = Some(key);
    }
}

/// Switched off: drop the image, the tasks and the output. With one consumer still on, only
/// the other's part goes: the shadows' published output (the volume keeps the producer), or
/// the volume's absolute image (Cloud shadows keeps it).
pub fn release_cloud_shadows(
    shadows: Res<CloudShadows>,
    mut work: ResMut<CloudShadowWork>,
    mut published: ResMut<CloudShadowPublished>,
    mut images: ResMut<Assets<Image>>,
    wanted: Option<Res<CloudCoverWanted>>,
) {
    let volume = wanted.is_some_and(|w| w.0);
    if !shadows.enabled && !volume {
        if work.image.is_some() || published.0.is_some() || !work.alphas.is_empty() || work.job.is_some() || work.cover_job.is_some() {
            work.go_off("switched off", false, &mut published, &mut images);
            work.ground = None;
        }
    } else if !shadows.enabled {
        // The volume alone: the shadows' output and their relative image go.
        if published.0.is_some() {
            published.0 = None;
        }
        if let Some(h) = work.image.take() {
            images.remove(&h);
        }
        work.relative_shown = false;
        // Their composition goes with their image (the volume's cover keeps its own).
        work.job = None;
        work.shown = None;
    }
    if !volume {
        work.cover_empty = false;
        for a in work.alphas.values_mut() {
            if let Alpha::Unchecked(_, task) = a {
                *task = None;
            }
        }
    }
    // The volume off: its cover slot goes whole, whatever state it is in
    // (an unfinished first job too, so a later activation starts from nothing).
    if !volume && (work.absolute_shown || work.cover_at.is_some() || work.cover_job.is_some() || work.cover_shown.is_some() || !work.weather_covers.is_empty() || work.noise_scroll.is_some()) {
        work.drop_absolute();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weather(textures: [Option<&str>; 4], speeds: [u8; 4]) -> Weather {
        Weather { cloud_textures: textures.map(|t| t.map(str::to_string)), cloud_speeds: speeds, ..Default::default() }
    }

    #[test]
    fn cloud_transition_endpoints() {
        let now = weather([Some("now0.dds"), None, None, Some("now3.dds")], [10, 0, 0, 20]);
        let was = weather([None, Some("was1.dds"), None, None], [0, 30, 0, 0]);
        let all = [true; 4];
        // No fade: the current weather alone, at 1.
        let l = active_layers(&now, None, 0.3, all);
        assert_eq!(l.iter().map(|l| (l.texture.as_str(), l.weight)).collect::<Vec<_>>(), [("now0.dds", 1.0), ("now3.dds", 1.0)]);
        // Fade 0: only the previous set; fade 1: only the current.
        let start = active_layers(&now, Some(&was), 0.0, all);
        assert_eq!(start.len(), 1);
        assert_eq!((start[0].texture.as_str(), start[0].weight), ("was1.dds", 1.0));
        let end = active_layers(&now, Some(&was), 1.0, all);
        assert_eq!(end.iter().map(|l| l.texture.as_str()).collect::<Vec<_>>(), ["now0.dds", "now3.dds"]);
        assert!(end.iter().all(|l| l.weight == 1.0));
        // Midway: both, weighted fade and 1 - fade, each at its own weather's speed.
        let mid = active_layers(&now, Some(&was), 0.25, all);
        assert_eq!(mid.len(), 3);
        assert_eq!(mid[0].weight, 0.25);
        assert_eq!(mid[2].weight, 0.75);
        assert_eq!(mid[2].scroll, f64::from(was.cloud_scroll(1)));
        // A layer the dome has no shape for is not composed.
        let l = active_layers(&now, None, 1.0, [true, true, true, false]);
        assert_eq!(l.len(), 1);
    }

    fn constant(a: u8) -> Option<Arc<[u8]>> {
        Some(Arc::from(vec![a; MAP_SIZE * MAP_SIZE]))
    }

    /// The flat layers' own transmittance: the mean of the absolute product over every offset of the
    /// layers (here all 512 along a row, whole texels), free of the drift and of the image's reference
    /// (uniform opaque cover: a fully lit image and 0 here); through a fade it goes from the one
    /// weather's to the other's with the dome's weights, with no step.
    #[test]
    fn the_flat_transmittance_is_absolute_and_free_of_the_drift() {
        assert_eq!(flat_transmittance(std::iter::empty()), 1.0);
        assert_eq!(flat_transmittance([(1.0, 1.0), (0.2, 1.0)].into_iter()), 0.0);
        assert!((flat_transmittance([(0.5, 1.0), (0.4, 0.5)].into_iter()) - 0.4).abs() < 1e-6);
        assert_eq!(mean_alpha(&[]), 0.0);
        assert!((mean_alpha(&[0, 255, 51, 204]) - 0.5).abs() < 1e-6);
        // An opaque sheet: the relative image is all lit.
        let opaque: Arc<[u8]> = Arc::from(vec![255u8; MAP_SIZE * MAP_SIZE]);
        let sheet = compose_relative(&[ComposeLayer { alpha: Some(opaque.clone()), weight: 1.0, shift: [0.0; 2] }], Some(90.0), None);
        assert!(sheet.texels.iter().all(|&t| t == 255) && flat_transmittance(std::iter::once((mean_alpha(&opaque), 1.0))) == 0.0);
        // Two layers of stripes across U: the product's mean moves with their offset, its mean over
        // the offsets is this.
        let row = |x: usize, f: usize| if (x * f / MAP_SIZE).is_multiple_of(2) { 0.8f64 } else { 0.1 };
        let mean = |f: usize| (0..MAP_SIZE).map(|x| row(x, f)).sum::<f64>() / MAP_SIZE as f64;
        let product = |s: usize| (0..MAP_SIZE).map(|x| (1.0 - row(x, 4)) * (1.0 - row((x + s) % MAP_SIZE, 4))).sum::<f64>() / MAP_SIZE as f64;
        assert!((product(0) - product(MAP_SIZE / 8)).abs() > 0.05, "the drift moves the product's mean");
        let over = (0..MAP_SIZE).map(product).sum::<f64>() / MAP_SIZE as f64;
        let flat = flat_transmittance([(mean(4) as f32, 1.0), (mean(4) as f32, 1.0)].into_iter());
        assert!((f64::from(flat) - over).abs() < 1e-5, "{flat} {over}");
        // The producer's: a thin weather's layer against a thick one's over a fade.
        let mut work = CloudShadowWork::default();
        // (The means as the reads leave them.)
        work.alpha_means.insert("thin".into(), mean_alpha(&[51; 16]));
        work.alpha_means.insert("thick".into(), mean_alpha(&[255; 16]));
        let layers = |fade: f32| -> Vec<LayerInput> {
            let now = weather([Some("thick"), None, None, None], [0; 4]);
            let was = weather([Some("thin"), None, None, None], [0; 4]);
            active_layers(&now, Some(&was), fade, [true; 4])
        };
        let mut last = None;
        for i in 0..=100 {
            let fade = i as f32 / 100.0;
            work.follow_deck(&layers(fade), 900.0, true, false);
            let deck = work.deck().expect("read");
            assert_eq!(deck.plane_height, 900.0);
            let t = deck.transmittance;
            assert!((t - (1.0 - fade) * (1.0 - 0.2 * (1.0 - fade))).abs() < 1e-5, "{fade}: {t}");
            assert!(last.is_none_or(|l: f32| l >= t && l - t < 0.013), "{fade}: {last:?} {t}");
            last = Some(t);
        }
        // A read in flight keeps the last one; Cloud shadows off has none.
        work.follow_deck(&layers(0.0), 900.0, true, true);
        assert_eq!(work.deck().map(|d| d.transmittance), last);
        // A fade turned round, or one that starts, keeps it; a weather replaced at once and another
        // worldspace clear it, until the next layers are read.
        let (mut published, mut images) = (CloudShadowPublished::default(), Assets::<Image>::default());
        work.note_weather((1, Some(2)), &mut published, &mut images);
        work.note_weather((2, Some(1)), &mut published, &mut images);
        assert!(work.deck().is_some(), "an ordinary fade");
        work.note_weather((3, None), &mut published, &mut images);
        assert_eq!(work.deck(), None, "replaced at once");
        work.follow_deck(&layers(0.0), 900.0, true, true);
        assert_eq!(work.deck(), None, "still being read");
        work.follow_deck(&layers(0.0), 900.0, true, false);
        work.ground = Some((Some(7), 0.0));
        assert!(work.note_world(Some(8), &mut published, &mut images));
        assert_eq!(work.deck(), None, "another worldspace");
        work.follow_deck(&layers(0.0), 900.0, true, false);
        work.follow_deck(&layers(0.0), 900.0, false, false);
        assert_eq!(work.deck(), None, "Cloud shadows off");
    }

    #[test]
    fn cloud_transmission_is_product() {
        let a = 128.0 / 255.0;
        let out = compose(&[
            ComposeLayer { alpha: constant(128), weight: 1.0, shift: [0.0; 2] },
            ComposeLayer { alpha: constant(128), weight: 0.5, shift: [0.3, 0.7] },
        ]);
        let want = ((1.0 - a) * (1.0 - 0.5 * a) * 255.0f32).round() as u8;
        assert!(out.iter().all(|&t| t == want), "{} vs {want}", out[0]);
        // Opaque at full weight is black; at weight 0 the layer is not there.
        assert!(compose(&[ComposeLayer { alpha: constant(255), weight: 1.0, shift: [0.0; 2] }]).iter().all(|&t| t == 0));
        assert!(compose(&[ComposeLayer { alpha: constant(255), weight: 0.0, shift: [0.0; 2] }]).iter().all(|&t| t == 255));
    }

    /// An alpha map of `cloud` with the rows `gap(y)` at `gap_alpha`.
    fn rows(cloud: u8, gap_alpha: u8, gap: impl Fn(usize) -> bool) -> Option<Arc<[u8]>> {
        let n = MAP_SIZE;
        Some(Arc::from((0..n * n).map(|i| if gap(i / n) { gap_alpha } else { cloud }).collect::<Vec<u8>>()))
    }

    fn relative_of(layers: &[ComposeLayer]) -> Composition {
        compose_relative(layers, Some(REFERENCE_PERCENTILE), None)
    }

    /// Uniform cover of any density is its own reference: nothing darkens (the play-test of
    /// 2026-11-17: an overcast weather dimmed the whole scene evenly).
    #[test]
    fn cloud_uniform_cover_casts_nothing() {
        for alpha in [0u8, 40, 128, 230, 254, 255] {
            for weight in [1.0, 0.5] {
                let one = [ComposeLayer { alpha: constant(alpha), weight, shift: [0.0; 2] }];
                assert!(relative_of(&one).texels.iter().all(|&t| t == 255), "alpha {alpha} weight {weight}");
                let two = [one[0].clone(), ComposeLayer { alpha: constant(alpha / 2), weight, shift: [0.4, 0.1] }];
                assert!(relative_of(&two).texels.iter().all(|&t| t == 255), "two layers, alpha {alpha} weight {weight}");
            }
        }
        // Uniform cover after a clearer map (a weather fading in): the earlier, higher
        // reference must not dim it.
        let grey = [ComposeLayer { alpha: constant(153), weight: 1.0, shift: [0.0; 2] }];
        let after_clear = compose_relative(&grey, Some(REFERENCE_PERCENTILE), Some(0.8));
        assert!(after_clear.texels.iter().all(|&t| t == 255), "a higher earlier reference dims uniform cover");
        let r = after_clear.reference.expect("relative");
        assert_eq!(r.used, r.raw);
        // The absolute rule (the debug override `off`) still dims it.
        let dense = [ComposeLayer { alpha: constant(200), weight: 1.0, shift: [0.0; 2] }];
        assert!(compose_relative(&dense, None, None).texels.iter().all(|&t| t == 55));
        assert_eq!(compose_relative(&dense, None, None).reference, None);
    }

    /// Scattered cloud on clear sky: most texels are clear, so the reference is 1 and the
    /// map is the absolute one within rounding.
    #[test]
    fn cloud_scattered_cover_is_kept() {
        // A 5 % band of opaque cloud and a soft edge below it, the rest clear.
        let n = MAP_SIZE;
        let alpha: Vec<u8> = (0..n * n)
            .map(|i| match i / n {
                y if y < 26 => 255,
                y if y < 40 => (255 - (y - 25) * 18) as u8,
                _ => 0,
            })
            .collect();
        let layers = [ComposeLayer { alpha: Some(Arc::from(alpha)), weight: 0.8, shift: [0.25, 0.0] }];
        let (absolute, rel) = (compose(&layers), relative_of(&layers));
        assert_eq!(rel.reference.map(|r| r.raw), Some(1.0));
        assert!(absolute.iter().zip(&rel.texels).all(|(&a, &r)| a.abs_diff(r) <= 1));
        assert!(rel.texels.iter().any(|&t| t < 60), "the cloud still darkens");
    }

    /// Heavy cloud with gaps: the gaps are the reference and read fully lit, the cloud is
    /// darker by its ratio to them.
    #[test]
    fn cloud_gaps_become_the_reference() {
        let n = MAP_SIZE;
        // One row in five a thinner gap (20 % of the texels, above the 90th percentile).
        let layers = [ComposeLayer { alpha: rows(200, 100, |y| y.is_multiple_of(5)), weight: 1.0, shift: [0.0; 2] }];
        let rel = relative_of(&layers);
        let (cloud, gap) = (1.0 - 200.0 / 255.0f32, 1.0 - 100.0 / 255.0f32);
        let r = rel.reference.expect("relative");
        assert!((r.raw - gap).abs() < 1e-6 && r.used == r.raw, "{r:?}");
        assert!((0..n).step_by(5).all(|y| rel.texels[y * n..(y + 1) * n].iter().all(|&t| t == 255)), "gaps fully lit");
        let want = (cloud / gap * 255.0).round() as u8;
        assert!(rel.texels[n..2 * n].iter().all(|&t| t.abs_diff(want) <= 1), "{} vs {want}", rel.texels[n]);
        // Absolute, the gaps were dimmed too.
        assert!(compose(&layers)[0] < 160);
    }

    /// Cover with no clear gaps: under the floor the darkening fades out in proportion
    /// (no speckle from the alpha's 8-bit steps), and an opaque map casts nothing.
    #[test]
    fn cloud_reference_floor_fades_out() {
        let layers = [ComposeLayer { alpha: rows(255, 250, |y| y.is_multiple_of(5)), weight: 1.0, shift: [0.0; 2] }];
        let rel = relative_of(&layers);
        let r = rel.reference.expect("relative");
        assert!(r.raw < REFERENCE_FLOOR && r.raw > 0.0, "{r:?}");
        let least = *rel.texels.iter().min().unwrap();
        // An opaque texel's darkening is `reference / REFERENCE_FLOOR`, not all of it.
        let want = ((1.0 - r.used / REFERENCE_FLOOR) * 255.0).round() as u8;
        assert!(least.abs_diff(want) <= 1 && least > 200, "{least} vs {want}");
        assert!(relative(&[0.0; 16], 0.0).iter().all(|&t| t == 255));
    }

    /// The reference approaches the map's own over compositions, takes it within the
    /// settle step, and starts again at a discontinuity.
    #[test]
    fn cloud_reference_smooths_and_resets() {
        assert_eq!(smoothed_reference(None, 0.4), 0.4);
        assert!((smoothed_reference(Some(0.8), 0.4) - 0.7).abs() < 1e-6);
        assert_eq!(smoothed_reference(Some(0.4 + REFERENCE_SETTLE * 0.5), 0.4), 0.4);
        let mut used = 1.0;
        let mut steps = 0;
        while used != 0.3 {
            used = smoothed_reference(Some(used), 0.3);
            steps += 1;
            assert!(steps < 40, "settles");
        }
        // The percentile of a map: values 0, 0.01, ... 0.99.
        let t: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        assert!((percentile(&t, 90.0) - 0.89).abs() < 1e-6);
        assert!((percentile(&t, 100.0) - 0.99).abs() < 1e-6);
        assert_eq!(percentile(&t, 0.0), 0.0);
        // A composition still moving towards its reference is due again; a discontinuity
        // forgets it (the next composition takes its map's own at once).
        let mut work = CloudShadowWork { shown: Some(Composed { reference: Some(Reference { raw: 0.3, used: 0.5 }), ..empty_composed() }), ..Default::default() };
        assert!(work.shown.as_ref().unwrap().unsettled());
        let (mut images, mut published) = (Assets::<Image>::default(), CloudShadowPublished::default());
        work.note_weather((1, None), &mut published, &mut images);
        assert!(work.note_weather((2, Some(1)), &mut published, &mut images));
        assert_eq!(work.shown.as_ref().and_then(|s| s.reference), None);
        // The debug override.
        assert_eq!(parse_reference(None), Some(REFERENCE_PERCENTILE));
        assert_eq!(parse_reference(Some(" OFF ")), None);
        assert_eq!(parse_reference(Some("75")), Some(75.0));
        assert_eq!(parse_reference(Some("150")), Some(REFERENCE_PERCENTILE));
        assert_eq!(parse_reference(Some("soon")), Some(REFERENCE_PERCENTILE));
    }

    #[test]
    fn cloud_missing_texture_is_clear() {
        assert!(compose(&[]).iter().all(|&t| t == 255));
        let missing = ComposeLayer { alpha: None, weight: 1.0, shift: [0.0; 2] };
        let wrong_size = ComposeLayer { alpha: Some(Arc::from(vec![255u8; 16])), weight: 1.0, shift: [0.0; 2] };
        assert!(compose(&[missing, wrong_size]).iter().all(|&t| t == 255));
        // An unreadable source resamples to no cover.
        assert!(resample_alpha(&[], 0, 0).iter().all(|&a| a == 0));
    }

    #[test]
    fn cloud_shift_samples_ahead() {
        // One opaque texel at (10, 20); a shift of +2 texels in u brings it to (8, 20).
        let n = MAP_SIZE;
        let mut a = vec![0u8; n * n];
        a[20 * n + 10] = 255;
        let out = compose(&[ComposeLayer { alpha: Some(Arc::from(a)), weight: 1.0, shift: [2.0 / n as f64, 0.0] }]);
        assert_eq!(out[20 * n + 8], 0);
        assert_eq!(out.iter().filter(|&&t| t != 255).count(), 1);
        // A whole repeat is no shift.
        let mut b = vec![0u8; n * n];
        b[3 * n + 5] = 255;
        let out = compose(&[ComposeLayer { alpha: Some(Arc::from(b)), weight: 1.0, shift: [1.0, -2.0] }]);
        assert_eq!(out[3 * n + 5], 0);
    }

    #[test]
    fn cloud_resample_keeps_mean() {
        // 1024 wide stripes of 0 and 255 average to the middle; a 256 source keeps its value.
        let src: Vec<u8> = (0..1024 * 1024).map(|i| if i % 2 == 0 { 0 } else { 255 }).collect();
        let out = resample_alpha(&src, 1024, 1024);
        assert!(out.iter().all(|&a| a == 128 || a == 127), "{}", out[0]);
        let flat = resample_alpha(&[200u8; 256 * 128], 256, 128);
        assert!(flat.iter().all(|&a| a == 200));
    }

    #[test]
    fn cloud_projection_accounts_for_height() {
        let (scale, offset, plane) = (Vec2::splat(1.0 / REPEAT_UNITS), Vec2::new(0.25, 0.5), 20_000.0);
        let to_light = Vec3::new(0.5, 0.8, -0.2).normalize();
        // Two receivers on one ray towards the light share one point of the plane.
        let low = Vec3::new(1_000.0, 0.0, -3_000.0);
        let high = low + to_light * 900.0;
        let (a, b) = (projected_uv(low, to_light, plane, scale, offset).unwrap(), projected_uv(high, to_light, plane, scale, offset).unwrap());
        assert!((a - b).length() < 1e-5, "{a} {b}");
        // A raised receiver straight above the low one does not.
        let above = projected_uv(low + Vec3::Y * 900.0, to_light, plane, scale, offset).unwrap();
        assert!((above - a).length() > 1e-3);
        // Straight overhead light: x, z scaled plus the offset; the camera plays no part.
        let up = projected_uv(low, Vec3::Y, plane, scale, offset).unwrap();
        assert!((up - (Vec2::new(low.x, low.z) * scale + offset)).length() < 1e-6);
        // Above the plane, exactly on it (the shader's `P.y >= height`), or the light at
        // the horizon: not reached; just under the plane: reached.
        assert!(projected_uv(Vec3::Y * 25_000.0, to_light, plane, scale, offset).is_none());
        assert!(projected_uv(Vec3::new(10.0, plane, 20.0), to_light, plane, scale, offset).is_none(), "on the plane, as the shader");
        assert!(projected_uv(Vec3::new(10.0, plane - 1.0, 20.0), to_light, plane, scale, offset).is_some());
        assert!(projected_uv(low, Vec3::X, plane, scale, offset).is_none());
    }

    /// The wind's displacement after `secs` seconds of one direction.
    fn steady(secs: f64, wind: Vec2) -> [f64; 2] {
        WindPhase::default().advance(secs, wind)
    }

    /// Wrapped distance between two offsets, per axis.
    fn wrapped(a: Vec2, b: Vec2) -> Vec2 {
        let d = (a - b).abs();
        d.min(Vec2::ONE - d)
    }

    #[test]
    fn cloud_drift_follows_wind_without_jumps() {
        let wind = Vec2::new(0.6, -0.8);
        let scroll = 65.0 / 25_600.0;
        // Across a whole repeat of drift the offset moves smoothly (wrapped per axis).
        let one = 1.0 / scroll;
        for secs in [0.0, one - 0.5, one, 7.3 * one, 36_000.0] {
            let (a, b) = (uv_offset(steady(secs, wind), scroll), uv_offset(steady(secs + 0.5, wind), scroll));
            assert!(wrapped(a, b).length() < 0.5 * scroll as f32 * 1.01 + 1e-6, "{secs}: {a} {b}");
        }
        // The pattern moves downwind: a texel of the image lands further along the wind.
        let (t0, t1) = (uv_offset(steady(10.0, wind), scroll), uv_offset(steady(20.0, wind), scroll));
        let world_move = -(t1 - t0) * REPEAT_UNITS;
        assert!(world_move.dot(wind) > 0.0);
        // Layers at the reference's speed are not shifted; others by their extra drift.
        assert_eq!(layer_shift(steady(1234.5, wind), scroll, scroll), [0.0, 0.0]);
        let s = layer_shift(steady(100.0, wind), 2.0 * scroll, scroll);
        let o = uv_offset(steady(100.0, wind), scroll);
        assert!(((s[0] as f32) - o.x).abs() < 1e-5 && ((s[1] as f32) - o.y).abs() < 1e-5);
    }

    /// A SetWindDirection late in a session turns the drift from then on: the published
    /// offset and the layers' shifts against each other are where they were at the change,
    /// and later move along the new direction only.
    #[test]
    fn cloud_wind_change_keeps_phase() {
        let scroll = 65.0 / 25_600.0;
        let other = 2.5 * scroll;
        let (east, north) = (Vec2::X, Vec2::new(0.0, -1.0));
        let mut wind = WindPhase::default();
        wind.advance(0.0, east);
        let before = wind.advance(36_000.0, east);
        let (offset_before, shift_before) = (uv_offset(before, scroll), layer_shift(before, other, scroll));
        // The direction changes at this moment: nothing moves.
        let at_change = wind.advance(36_000.0, north);
        assert_eq!(uv_offset(at_change, scroll), offset_before);
        assert_eq!(layer_shift(at_change, other, scroll), shift_before);
        // Ten seconds on: moved by ten seconds' drift along the new direction alone.
        let later = wind.advance(36_010.0, north);
        let moved = uv_offset(later, scroll) - offset_before;
        assert!(wrapped(uv_offset(later, scroll), offset_before).x < 1e-5, "no drift along the old direction: {moved}");
        assert!((wrapped(uv_offset(later, scroll), offset_before).y - (10.0 * scroll) as f32).abs() < 1e-5, "{moved}");
        // The old rule (all the time passed times the current direction) would have jumped.
        let old_rule = uv_offset(steady(36_000.0, north), scroll);
        assert!(wrapped(old_rule, offset_before).length() > 0.05, "{old_rule} {offset_before}");
    }

    #[test]
    fn cloud_recompose_needs_a_material_change() {
        let layer = |weight: f32, shift: [f64; 2]| ComposedLayer { texture: "a.dds".into(), weight, covered: true, scroll: 0.001, shift };
        let base = Composed { layers: vec![layer(0.5, [0.0, 0.0])], ref_scroll: 0.001, dir: Vec2::X, reference: None };
        let mut same = base.clone();
        same.layers[0].shift = [0.1 / MAP_SIZE as f64, 0.0];
        same.layers[0].weight = 0.5 + WEIGHT_STEP * 0.5;
        assert!(!base.differs(&same));
        let mut moved = base.clone();
        moved.layers[0].shift = [1.0 - 0.3 / MAP_SIZE as f64, 0.0];
        assert!(base.differs(&moved));
        let mut done = base.clone();
        done.layers[0].weight = 1.0;
        assert!(base.differs(&done));
        let mut other = base.clone();
        other.layers[0].texture = "b.dds".into();
        assert!(base.differs(&other));
    }

    /// The real producer path through a door (finding 1 of the C0/C1 review): outdoors
    /// with a published image, indoors (the producer clears its output), and outdoors
    /// again before anything is composed (no game data here: nothing published). While
    /// Cloud shadows is on the views keep their markers and the cloud variant, the cloud
    /// going neutral meanwhile; only switching off takes them away.
    #[test]
    fn cloud_bindings_survive_doors() {
        use crate::sun_visibility::{mark_targets, marking_needed, OutdoorSunLight, SunVisibility, SunVisibilityUniform, SunVisibilityView, NEUTRAL_CLOUD};
        let mut app = App::new();
        app.insert_resource(CloudShadows { enabled: true, forced: false })
            .init_resource::<CloudShadowPublished>()
            .init_resource::<CloudShadowWork>()
            .init_resource::<SunVisibility>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Time>()
            .init_resource::<Time<Real>>()
            .insert_resource(crate::sky::SkyState::default())
            .add_systems(
                Update,
                (
                    follow_switch.run_if(resource_changed::<CloudShadows>),
                    produce_cloud_shadows.run_if(cloud_shadows_on),
                    release_cloud_shadows.run_if(resource_changed::<CloudShadows>),
                    publish_cloud_shadows,
                    mark_targets.run_if(marking_needed),
                )
                    .chain(),
            );
        let sun = app.world_mut().spawn((crate::sky::Sun, DirectionalLight::default())).id();
        let cam = app.world_mut().spawn((crate::fly::FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }, Transform::default())).id();
        // Outdoors, an image published (as the producer leaves it after a composition).
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(new_image());
        app.world_mut().resource_mut::<CloudShadowPublished>().0 =
            Some(CloudShadowOutput { image, uv_scale: Vec2::splat(1.0 / REPEAT_UNITS), uv_offset: Vec2::ZERO, plane_height: PLANE_ABOVE_GROUND, strength: STRENGTH });
        app.update();
        app.update();
        let vis = app.world().resource::<SunVisibility>().clone();
        assert!(vis.cloud_enabled && vis.cloud.is_some() && vis.cloud_on());
        assert!(app.world().get::<OutdoorSunLight>(sun).is_some());
        assert!(app.world().get::<SunVisibilityView>(cam).is_some());

        // Indoors: the producer clears its output; the views keep the cloud variant with a
        // neutral cloud, and the light (the cell's own now) is unmarked.
        app.world_mut().resource_mut::<crate::sky::SkyState>().interior = Some(Default::default());
        app.update();
        app.update();
        let vis = app.world().resource::<SunVisibility>().clone();
        assert!(app.world().resource::<CloudShadowPublished>().0.is_none(), "the producer's door path cleared it");
        assert!(vis.cloud.is_none() && vis.cloud_on(), "switched on, nothing published");
        assert_eq!(SunVisibilityUniform::pack(&vis).cloud, NEUTRAL_CLOUD);
        assert!(app.world().get::<SunVisibilityView>(cam).is_some(), "bindings kept through the door");
        assert!(app.world().get::<OutdoorSunLight>(sun).is_none());

        // Outdoors again, nothing composed yet: still the same views, the sun marked again.
        app.world_mut().resource_mut::<crate::sky::SkyState>().interior = None;
        app.update();
        app.update();
        assert!(app.world().resource::<SunVisibility>().cloud_on());
        assert!(app.world().get::<SunVisibilityView>(cam).is_some());
        assert!(app.world().get::<OutdoorSunLight>(sun).is_some());

        // Switched off: everything goes.
        app.world_mut().resource_mut::<CloudShadows>().enabled = false;
        app.update();
        app.update();
        assert!(!app.world().resource::<SunVisibility>().active());
        assert!(app.world().get::<SunVisibilityView>(cam).is_none());
        assert!(app.world().get::<OutdoorSunLight>(sun).is_none());
    }

    fn empty_composed() -> Composed {
        Composed { layers: Vec::new(), ref_scroll: 0.001, dir: Vec2::X, reference: None }
    }

    /// A composition started before a discontinuity (worldspace or weather set) never
    /// reaches the image; one of the current generation does.
    #[test]
    fn cloud_stale_job_is_dropped() {
        let mut images = Assets::<Image>::default();
        let mut work = CloudShadowWork { image: Some(images.add(new_image())), ..Default::default() };
        let mut published = CloudShadowPublished::default();
        let started = work.generation;
        // A weather replaced by a fade: a discontinuity (no clearing: the fade starts from
        // what the dome shows).
        assert!(!work.note_weather((1, None), &mut published, &mut images), "the first weather seen is no change");
        assert!(work.note_weather((2, Some(1)), &mut published, &mut images));
        assert_ne!(work.generation, started);
        assert!(work.image.is_some(), "a faded change keeps the image");
        // The old job finishes now: dropped before it touches the image.
        let stale = vec![0u8; MAP_SIZE * MAP_SIZE];
        assert!(!work.install(started, empty_composed(), stale, &mut images));
        let data = images.get(work.image.as_ref().unwrap()).unwrap().data.clone().unwrap();
        assert!(data.iter().all(|&t| t == 255), "the image is untouched");
        assert!(work.shown.is_none());
        // One of the current generation goes in.
        assert!(work.install(work.generation, empty_composed(), vec![7u8; MAP_SIZE * MAP_SIZE], &mut images));
        assert_eq!(images.get(work.image.as_ref().unwrap()).unwrap().data.as_ref().unwrap()[0], 7);
        assert!(work.shown.is_some());
        // A fade's progress and its end are no discontinuity; turning it round, another
        // weather, or a weather forced at once are.
        assert!(!weather_replaced((2, Some(1)), (2, Some(1))));
        assert!(!weather_replaced((2, Some(1)), (2, None)), "the fade ended");
        assert!(weather_replaced((2, Some(1)), (1, Some(2))), "turned round");
        assert!(weather_replaced((2, None), (3, Some(2))));
        assert!(weather_replaced((2, None), (3, None)), "forced at once");
        // Forced at once: the output is cleared too (neutral until composed again).
        published.0 = Some(CloudShadowOutput { image: work.image.clone().unwrap(), uv_scale: Vec2::ONE, uv_offset: Vec2::ZERO, plane_height: 0.0, strength: 1.0 });
        assert!(work.note_weather((3, None), &mut published, &mut images));
        assert!(published.0.is_none() && work.image.is_none() && work.shown.is_none());
    }

    /// Another worldspace clears the output and the ground reference and drops the running
    /// composition; the same worldspace again changes nothing.
    #[test]
    fn cloud_world_change_clears_output() {
        let mut images = Assets::<Image>::default();
        let image = images.add(new_image());
        let mut work = CloudShadowWork { image: Some(image.clone()), shown: Some(empty_composed()), ground: Some((Some(1), 1024.0)), ..Default::default() };
        let mut published = CloudShadowPublished(Some(CloudShadowOutput { image: image.clone(), uv_scale: Vec2::ONE, uv_offset: Vec2::ZERO, plane_height: 21_024.0, strength: STRENGTH }));
        assert!(!work.note_world(Some(1), &mut published, &mut images), "the same worldspace");
        assert!(published.0.is_some() && work.image.is_some());
        let before = work.generation;
        assert!(work.note_world(Some(2), &mut published, &mut images));
        assert!(published.0.is_none(), "nothing of the old world is shown");
        assert!(work.image.is_none() && work.shown.is_none() && work.job.is_none());
        assert!(images.get(&image).is_none(), "the old image is gone");
        assert_eq!(work.ground, None, "the new world gets its own plane");
        assert_ne!(work.generation, before);
        assert!(!work.install(before, empty_composed(), vec![0; MAP_SIZE * MAP_SIZE], &mut images), "a job of the old world");
    }

    /// The settings row turns it on and off; the debug override holds it on whatever the
    /// row says.
    #[test]
    fn cloud_switch_follows_the_row_or_the_override() {
        use crate::graphics::{Graphics, Preset};
        let mut app = App::new();
        app.insert_resource(Graphics::preset(Preset::Vanilla))
            .insert_resource(CloudShadows::default())
            .init_resource::<crate::sun_visibility::SunVisibility>()
            .add_systems(Update, (follow_graphics.run_if(resource_exists_and_changed::<Graphics>), follow_switch.run_if(resource_changed::<CloudShadows>)).chain());
        app.update();
        assert!(!app.world().resource::<CloudShadows>().enabled, "off by default");
        app.world_mut().resource_mut::<Graphics>().cloud_shadows = true;
        app.update();
        assert!(app.world().resource::<CloudShadows>().enabled);
        assert!(app.world().resource::<crate::sun_visibility::SunVisibility>().cloud_enabled, "into the plumbing");
        app.world_mut().resource_mut::<Graphics>().cloud_shadows = false;
        app.update();
        assert!(!app.world().resource::<crate::sun_visibility::SunVisibility>().cloud_enabled);
        app.world_mut().resource_mut::<CloudShadows>().forced = true;
        app.world_mut().resource_mut::<Graphics>().set_changed();
        app.update();
        assert!(app.world().resource::<CloudShadows>().enabled, "the override wins over the row");
    }

    #[test]
    fn cloud_calibration_parses() {
        assert_eq!(parse_calibration(None), Calibration::default());
        let c = parse_calibration(Some("15000, 40000,0.8"));
        assert_eq!((c.plane_above_ground, c.repeat, c.strength, c.direction_deg), (15_000.0, 40_000.0, 0.8, None));
        let c = parse_calibration(Some(",,2,90"));
        assert_eq!((c.plane_above_ground, c.repeat, c.strength, c.direction_deg), (PLANE_ABOVE_GROUND, REPEAT_UNITS, 1.0, Some(90.0)));
        assert_eq!(parse_calibration(Some("x,0,y")).repeat, REPEAT_UNITS);
        assert_eq!(parse_phase(Some(" 120.5 ")), Some(120.5));
        assert_eq!(parse_phase(Some("-1")), None);
        assert_eq!(parse_phase(Some("soon")), None);
    }

    /// #2991: the volume's absolute cover is `compose`'s product, the relative texels stay
    /// what `compose_relative` gives, and the mean cover is the mean of `1 - T`.
    #[test]
    fn cloud_absolute_cover_beside_the_relative() {
        let layers = [ComposeLayer { alpha: rows(200, 20, |y| y % 4 == 0), weight: 1.0, shift: [0.0; 2] }];
        let cover = CoverInput { layers: layers.to_vec(), ranks: vec![6], sets: vec![0] };
        for pct in [Some(REFERENCE_PERCENTILE), None] {
            let with = compose_parts(&layers, pct, None, true, Some(&cover));
            let without = compose_relative(&layers, pct, None);
            assert_eq!(with.texels, without.texels, "the relative image is the same either way");
            assert!(without.absolute.is_none());
            let abs = with.absolute.expect("asked for");
            let texels = compose(&layers);
            let from_texels = texels.iter().map(|&t| 1.0 - f32::from(t) / 255.0).sum::<f32>() / texels.len() as f32;
            assert!((abs.mean_cover - from_texels).abs() < 2.0 / 255.0, "the mean of compose's product");
            let want = 0.75 * 200.0 / 255.0 + 0.25 * 20.0 / 255.0;
            assert!((abs.mean_cover - want).abs() < 1e-3, "{} vs {want}", abs.mean_cover);
            assert!((abs.visible[0] - want).abs() < 1e-3, "one layer is all of the cover");
        }
        // With Cloud shadows off no relative image is made, the cover still is.
        let cover_only = compose_parts(&layers, Some(REFERENCE_PERCENTILE), None, false, Some(&cover));
        assert!(cover_only.texels.is_empty() && cover_only.reference.is_none() && cover_only.absolute.is_some());
        assert_eq!(mean_cover(&[]), 0.0);
    }

    /// #3032, the mod rule (replacing the prototype's "an RGB texture covers fully"):
    /// a texture without plain 2D alpha gives the volume nothing, and a weather with one among
    /// its active layers, on either side of a fade, gets no absolute cover at all (shown
    /// flat). Told from the header alone: an RGB file, an alpha cube map, a cut-off file and
    /// garbage are refused; a whole 2D file with alpha is taken.
    #[test]
    fn cloud_textures_the_volume_cannot_read_leave_the_weather_flat() {
        let header = |flags: u32, fourcc: &[u8; 4], bits: u32, masks: [u32; 4], caps2: u32, payload: usize| {
            let mut d = vec![0u8; 128];
            let put = |d: &mut Vec<u8>, o: usize, v: u32| d[o..o + 4].copy_from_slice(&v.to_le_bytes());
            d[..4].copy_from_slice(b"DDS ");
            put(&mut d, 4, 124);
            put(&mut d, 8, 0x1007);
            put(&mut d, 12, 1);
            put(&mut d, 16, 1);
            put(&mut d, 28, 1);
            put(&mut d, 76, 32);
            put(&mut d, 80, flags);
            d[84..88].copy_from_slice(fourcc);
            put(&mut d, 88, bits);
            for (k, m) in masks.into_iter().enumerate() {
                put(&mut d, 92 + 4 * k, m);
            }
            put(&mut d, 112, caps2);
            d.resize(128 + payload, 0x80);
            d
        };
        let rgb = |caps2: u32, payload: usize| header(0x40, &[0u8; 4], 24, [0x00FF_0000, 0xFF00, 0xFF, 0], caps2, payload);
        let argb = |payload: usize| header(0x41, &[0u8; 4], 32, [0x00FF_0000, 0xFF00, 0xFF, 0xFF00_0000], 0, payload);
        let cube = 0x200 | 0xFC00;
        assert!(!volume_alpha_ok(&rgb(0, 3)), "a 2D RGB texture: no alpha");
        assert!(!volume_alpha_ok(&rgb(cube, 18)), "an RGB cube map");
        assert!(!volume_alpha_ok(&header(0x4, b"DXT5", 0, [0; 4], cube, 6 * 16)), "a DXT5 cube map (alpha, but not 2D)");
        assert!(!volume_alpha_ok(&argb(2)), "a cut-off file");
        assert!(!volume_alpha_ok(b"not a dds"));
        assert!(volume_alpha_ok(&argb(4)), "a whole 2D texture with alpha");
        assert!(volume_alpha_ok(&header(0x4, b"DXT5", 0, [0; 4], 0, 16)), "a 2D DXT5 texture");
        // The RGB file's alpha is not decoded at all (alpha_map refuses before allocating),
        // and the shadows' own reading of an alpha cube map's first face stays as it was.
        assert!(nvf_assets::texture::alpha_map(&rgb(0, 3)).is_none());
        assert!(nvf_assets::texture::alpha_map(&header(0x4, b"DXT5", 0, [0; 4], cube, 6 * 16)).is_some());
        for f in [fnv_dds::Format::Dxt1, fnv_dds::Format::Dxt5, fnv_dds::Format::A8R8G8B8, fnv_dds::Format::A8L8] {
            assert!(alpha_format(f), "{f:?}");
        }
        for f in [fnv_dds::Format::R8G8B8, fnv_dds::Format::X8R8G8B8, fnv_dds::Format::L8, fnv_dds::Format::Bc5U] {
            assert!(!alpha_format(f), "{f:?}");
        }
        // Mixed layers (a fade from a weather with readable layers to one with an RGB layer):
        // the whole cover is refused, whichever side the bad one is on.
        let bad: HashSet<String> = ["sky/rgb.dds".to_string()].into_iter().collect();
        assert_eq!(refused_texture(["sky/a.dds", "sky/rgb.dds", "sky/b.dds"], &bad), Some("sky/rgb.dds"));
        assert_eq!(refused_texture(["sky/a.dds", "sky/b.dds"], &bad), None);
        assert_eq!(refused_texture(Vec::<&str>::new(), &bad), None);
    }

    /// #2991: a layer hidden by an opaque layer in front of it on the dome tints nothing:
    /// the visible shares follow the dome's draw order.
    #[test]
    fn cloud_visible_share_follows_the_draw_order() {
        let opaque = ComposeLayer { alpha: constant(255), weight: 1.0, shift: [0.0; 2] };
        let half = ComposeLayer { alpha: constant(128), weight: 1.0, shift: [0.0; 2] };
        let abs = compose_cover(&CoverInput { layers: vec![opaque.clone(), opaque.clone()], ranks: vec![0, 2], sets: vec![0, 0] });
        assert!(abs.visible[0].abs() < 1e-6 && (abs.visible[1] - 1.0).abs() < 1e-6, "{:?}", abs.visible);
        let abs = compose_cover(&CoverInput { layers: vec![half.clone(), opaque.clone()], ranks: vec![2, 0], sets: vec![0, 0] });
        let a = 128.0 / 255.0;
        assert!((abs.visible[0] - a).abs() < 1e-4 && (abs.visible[1] - (1.0 - a)).abs() < 1e-4, "{:?}", abs.visible);
        // The slots and ranks follow active_layers' order.
        let now = weather([Some("a.dds"), None, None, Some("b.dds")], [52, 0, 0, 65]);
        let was = weather([None, Some("c.dds"), None, None], [0, 10, 0, 0]);
        let slots = active_slots(&now, Some(&was), 0.5, [true; 4]);
        assert_eq!(slots, [(0, 0), (0, 3), (1, 1)]);
        assert_eq!(slots.len(), active_layers(&now, Some(&was), 0.5, [true; 4]).len());
    }

    /// #2991: the noise drifts with the layers that carry the cover, never with a
    /// transparent one (NVWastelandClear's layer 0 at ONAM 52 is the shadows' reference,
    /// its layer 3 at 65 the cover).
    #[test]
    fn cloud_noise_follows_the_covering_layers() {
        let (s52, s65) = (52.0 / 25_600.0, 65.0 / 25_600.0);
        assert_eq!(cover_scroll(&[0.0, 0.6], &[s52, s65]), Some(s65));
        let mixed = cover_scroll(&[0.25, 0.75], &[s52, s65]).unwrap();
        assert!((mixed - (0.25 * s52 + 0.75 * s65)).abs() < 1e-12);
        assert_eq!(cover_scroll(&[0.0, 0.0], &[s52, s65]), None);
        assert_eq!(cover_scroll(&[f32::NAN], &[s52]), None);
        // Through compose_cover: a transparent first layer has no visible share.
        let clear = ComposeLayer { alpha: constant(0), weight: 1.0, shift: [0.0; 2] };
        let cloud = ComposeLayer { alpha: constant(150), weight: 1.0, shift: [0.0; 2] };
        let abs = compose_cover(&CoverInput { layers: vec![clear, cloud], ranks: vec![0, 6], sets: vec![0, 0] });
        assert_eq!(cover_scroll(&abs.visible, &[s52, s65]), Some(s65));
    }

    /// #2991: through a weather fade between layers at different speeds the producer moves
    /// its reference to the heavier layer; `uv_offset` jumps there (the composition makes up
    /// for it in the image), the volume's drift does not: every frame it moves by no
    /// more than the faster layer's drift.
    #[test]
    fn the_volumes_drift_has_no_jump_when_the_reference_changes() {
        let (slow, fast) = (0.002, 0.006);
        let mut wind = WindPhase::default();
        let (mut drift, mut last) = ([0.3, 0.7], None);
        let mut prev: Option<([f64; 2], Vec2)> = None;
        let mut jumped = false;
        for frame in 0..600 {
            let secs = 1_000.0 + f64::from(frame) / 60.0;
            let phase = wind.advance(secs, Vec2::new(0.6, -0.8));
            // The fading-in layer becomes the heavier one halfway.
            let ref_scroll = if frame < 300 { slow } else { fast };
            drift = advance_drift(drift, last, phase, ref_scroll);
            last = Some(phase);
            let offset = uv_offset(phase, ref_scroll);
            if let Some((d0, o0)) = prev {
                let most = fast / 60.0 + 1e-9;
                assert!((drift[0] - d0[0]).abs() <= most && (drift[1] - d0[1]).abs() <= most, "frame {frame}: the drift jumped");
                let o = (offset - o0).abs();
                jumped |= o.min(Vec2::ONE - o).max_element() > 1e-3;
            }
            prev = Some((drift, offset));
        }
        assert!(jumped, "the reference's own offset does jump at the switch (what the drift must not follow)");
    }

    /// #2991: with only the volume on, the shadows' output stays out of the sun visibility
    /// (nothing of the shadow path turns on), the cover is published while its image is in,
    /// and switching the volume off drops the image and the cover.
    #[test]
    fn cloud_cover_alone_switches_no_shadow_on() {
        use crate::sun_visibility::SunVisibility;
        let mut app = App::new();
        app.insert_resource(CloudShadows { enabled: false, forced: false })
            .insert_resource(CloudCoverWanted(true))
            .init_resource::<CloudShadowPublished>()
            .init_resource::<CloudCoverPublished>()
            .init_resource::<CloudShadowWork>()
            .init_resource::<SunVisibility>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Time>()
            .init_resource::<Time<Real>>()
            .insert_resource(crate::sky::SkyState::default())
            .add_systems(
                Update,
                (
                    follow_switch.run_if(resource_changed::<CloudShadows>),
                    produce_cloud_shadows.run_if(producer_wanted),
                    release_cloud_shadows.run_if(switches_changed),
                    publish_cloud_cover,
                    publish_cloud_shadows,
                )
                    .chain(),
            );
        // As a composition leaves them: both images in, and the shadows' output left from
        // when Cloud shadows was on.
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(new_image());
        {
            let mut work = app.world_mut().resource_mut::<CloudShadowWork>();
            work.image = Some(image.clone());
            work.absolute_shown = true;
            work.cover_mean = 0.4;
            work.cover_at = Some((Vec2::splat(1.0 / REPEAT_UNITS), PLANE_ABOVE_GROUND));
        }
        app.world_mut().resource_mut::<CloudShadowPublished>().0 = Some(CloudShadowOutput { image, uv_scale: Vec2::ONE, uv_offset: Vec2::ZERO, plane_height: 0.0, strength: STRENGTH });
        app.update();
        app.update();
        let vis = app.world().resource::<SunVisibility>();
        assert!(!vis.active() && vis.cloud.is_none(), "the volume alone switches no shadow on");
        assert!(app.world().resource::<CloudShadowPublished>().0.is_none());
        let cover = app.world().resource::<CloudCoverPublished>().0.clone().expect("the cover is published");
        assert_eq!(cover.mean_cover, 0.4);
        // The volume off: its cover goes.
        app.world_mut().resource_mut::<CloudCoverWanted>().0 = false;
        app.update();
        assert!(app.world().resource::<CloudCoverPublished>().0.is_none());
        assert!(!app.world().resource::<CloudShadowWork>().absolute_shown);
    }

    /// #3032: a weather the volume cannot read publishes no cover, and says why.
    #[test]
    fn a_refused_cover_is_not_published() {
        let mut app = App::new();
        app.insert_resource(CloudCoverWanted(true))
            .init_resource::<CloudCoverPublished>()
            .init_resource::<CloudCoverRefused>()
            .init_resource::<CloudShadowWork>()
            .init_resource::<Assets<Image>>()
            .add_systems(Update, publish_cloud_cover);
        {
            let mut work = app.world_mut().resource_mut::<CloudShadowWork>();
            work.absolute_shown = true;
            work.cover_at = Some((Vec2::splat(1.0 / REPEAT_UNITS), PLANE_ABOVE_GROUND));
            work.cover_refused = Some("sky/rgb.dds".into());
        }
        app.update();
        assert!(app.world().resource::<CloudCoverPublished>().0.is_none());
        assert_eq!(app.world().resource::<CloudCoverRefused>().0.as_deref(), Some("sky/rgb.dds"));
        app.world_mut().resource_mut::<CloudShadowWork>().cover_refused = None;
        app.update();
        assert!(app.world().resource::<CloudCoverPublished>().0.is_some());
        assert_eq!(app.world().resource::<CloudCoverRefused>().0, None);
    }

    /// #2991: a cover composed for weathers the sky no longer shows (a short fade
    /// begun and ended between compositions, or its sets swapped) is composed again.
    #[test]
    fn a_cover_of_other_weathers_is_composed_again() {
        assert!(!cover_weathers_stale(true, [Some(1), None], [Some(1), None]));
        assert!(cover_weathers_stale(true, [Some(1), None], [Some(2), None]), "a fade begun and ended: same textures, another weather");
        assert!(cover_weathers_stale(true, [Some(2), Some(1)], [Some(1), Some(2)]), "the fade turned round: the sets swapped");
        assert!(!cover_weathers_stale(false, [Some(1), None], [Some(2), None]), "no cover shown: composed anyway");
    }

    /// A 1x1 DDS file: pixel-format flags, FourCC, bits, masks, caps2, mip count and payload.
    fn tiny_dds(flags: u32, fourcc: &[u8; 4], bits: u32, masks: [u32; 4], caps2: u32, mips: u32, payload: &[u8]) -> Vec<u8> {
        let mut d = vec![0u8; 128];
        let put = |d: &mut Vec<u8>, o: usize, v: u32| d[o..o + 4].copy_from_slice(&v.to_le_bytes());
        d[..4].copy_from_slice(b"DDS ");
        put(&mut d, 4, 124);
        put(&mut d, 8, 0x1007);
        put(&mut d, 12, 1);
        put(&mut d, 16, 1);
        put(&mut d, 28, mips);
        put(&mut d, 76, 32);
        put(&mut d, 80, flags);
        d[84..88].copy_from_slice(fourcc);
        put(&mut d, 88, bits);
        for (k, m) in masks.into_iter().enumerate() {
            put(&mut d, 92 + 4 * k, m);
        }
        put(&mut d, 112, caps2);
        d.extend_from_slice(payload);
        d
    }

    /// The volume takes only what the sky's loader takes, and alpha that is
    /// really there.
    #[test]
    fn the_volume_reads_only_files_the_sky_takes_with_real_alpha() {
        const ARGB: [u32; 4] = [0x00FF_0000, 0xFF00, 0xFF, 0xFF00_0000];
        // A BC1 block whose colours are in opaque order (c0 > c1): every texel opaque.
        let opaque_bc1 = tiny_dds(0x4, b"DXT1", 0, [0; 4], 0, 1, &[0xFF, 0xFF, 0x00, 0x00, 0, 0, 0, 0]);
        assert!(!volume_alpha_ok(&opaque_bc1), "a BC1 file with no transparent texel carries no alpha");
        // A BC1 block in cut-out order (c0 <= c1) using index 3: transparent texels.
        let cutout_bc1 = tiny_dds(0x4, b"DXT1", 0, [0; 4], 0, 1, &[0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
        assert!(volume_alpha_ok(&cutout_bc1), "a BC1 file that uses its one-bit alpha is read");
        // A DXT5 block whose alpha is full everywhere: a true full cover.
        let full_dxt5 = tiny_dds(0x4, b"DXT5", 0, [0; 4], 0, 1, &[255, 255, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0]);
        assert!(volume_alpha_ok(&full_dxt5));
        let full_argb = tiny_dds(0x41, &[0u8; 4], 32, ARGB, 0, 1, &[10, 20, 30, 255]);
        assert!(volume_alpha_ok(&full_argb), "an ARGB file at full alpha is a full cover");
        // Two complete levels on a 1x1 texture: the parser takes it, the sky's loader does not.
        let two_mips = tiny_dds(0x41, &[0u8; 4], 32, ARGB, 0, 2, &[10, 20, 30, 128, 10, 20, 30, 128]);
        assert!(fnv_dds::Dds::parse(&two_mips).is_ok());
        assert!(nvf_assets::texture::dds_image(&two_mips, true, true).is_err());
        assert!(!volume_alpha_ok(&two_mips), "what the sky's loader rejects the volume never takes");
        // A volume-flagged file of depth one.
        let volume = tiny_dds(0x41, &[0u8; 4], 32, ARGB, 0x0020_0000, 1, &[10, 20, 30, 128]);
        assert!(!volume_alpha_ok(&volume), "a volume texture whatever its depth");
        // The same file without the flag is taken, and the sky's loader takes it too.
        let plain = tiny_dds(0x41, &[0u8; 4], 32, ARGB, 0, 1, &[10, 20, 30, 128]);
        assert!(volume_alpha_ok(&plain) && nvf_assets::texture::dds_image(&plain, true, true).is_ok());
    }

    /// An uncompressed file whose declared row pitch pads its rows is refused (a
    /// 1x2 A8L8 made for the test); the same file with packed rows is read.
    #[test]
    fn a_padded_row_pitch_is_refused_for_the_volume() {
        let a8l8 = |pitch: u32, payload: &[u8]| {
            let mut d = tiny_dds(0x20001, &[0u8; 4], 16, [0xFF, 0, 0, 0xFF00], 0, 1, payload);
            let put = |d: &mut Vec<u8>, o: usize, v: u32| d[o..o + 4].copy_from_slice(&v.to_le_bytes());
            put(&mut d, 8, 0x1007 | 0x8);
            put(&mut d, 12, 2);
            put(&mut d, 20, pitch);
            d
        };
        // Packed: two texels of two bytes each, one per row.
        let packed = a8l8(2, &[40, 128, 60, 200]);
        assert!(volume_alpha_ok(&packed), "a pitch of the row's own width");
        // Padded rows of four bytes: four bytes the parser slices as two packed texels.
        let padded = a8l8(4, &[40, 128, 0, 0]);
        assert!(fnv_dds::Dds::parse(&padded).is_ok());
        assert!(!volume_alpha_ok(&padded), "padding would be read as the second row");
    }

    /// The live weathers and their weights
    /// on the dome; each live weather's own cover looked up by its form ID, none for a weather
    /// gone from the sky or not composed yet.
    #[test]
    fn the_live_weathers_own_covers_are_looked_up_by_id() {
        let mut sky = crate::sky::SkyState::default();
        let a = sky.weather.form_id;
        assert_eq!(live_weathers(&sky), [(Some(a), 1.0), (None, 0.0)]);
        let mut b = sky.weather.clone();
        b.form_id = a.wrapping_add(1);
        sky.prev = Some(b.clone());
        sky.fade = 0.25;
        assert_eq!(live_weathers(&sky), [(Some(a), 0.25), (Some(b.form_id), 0.75)]);
        sky.fade = 1.0;
        assert_eq!(live_weathers(&sky), [(Some(a), 1.0), (None, 0.0)], "the fade over");
        let known = [WeatherCover { id: a, mean: 0.3, shares: [0.3, 0.0, 0.0, 0.0] }];
        assert_eq!(live_covers(&known, [(Some(a), 0.5), (Some(b.form_id), 0.5)]), [Some(known[0]), None], "the other not composed yet");
        assert_eq!(live_covers(&known, [(Some(b.form_id), 1.0), (None, 0.0)]), [None, None], "a weather gone from the sky is not looked up");
    }

    /// A finished job installs each weather's own cover (a set with a weight on the
    /// dome when it started; covers of weathers outside the job dropped); publication carries the
    /// live weathers' covers by ID: a weather that has left the sky is not carried, and the cover
    /// is still published for the one that stays.
    #[test]
    fn the_weathers_own_covers_are_installed_and_published_by_id() {
        let mut app = App::new();
        app.insert_resource(CloudCoverWanted(true))
            .init_resource::<CloudCoverPublished>()
            .init_resource::<CloudShadowWork>()
            .init_resource::<Assets<Image>>()
            .insert_resource(crate::sky::SkyState::default())
            .add_systems(Update, publish_cloud_cover);
        let live = app.world().resource::<crate::sky::SkyState>().weather.form_id;
        let gone = live.wrapping_add(1);
        let job = |visible: Vec<f32>, sets: [(f32, Vec<f32>); 2]| Absolute { mean_cover: 0.5, visible, sets };
        {
            let mut work = app.world_mut().resource_mut::<CloudShadowWork>();
            work.cover_at = Some((Vec2::splat(1.0 / REPEAT_UNITS), PLANE_ABOVE_GROUND));
            // A fade's job: set 0 the live weather, set 1 the one that has just ended.
            work.install_cover(Some(job(vec![0.3, 0.2], [(0.35, vec![0.35, 0.0]), (0.25, vec![0.0, 0.25])])), &[(0, 0), (1, 0)], [Some(live), Some(gone)], [0.5, 0.5], &[0.0, 0.0]);
        }
        assert_eq!(app.world().resource::<CloudShadowWork>().weather_covers.len(), 2);
        app.update();
        let cover = app.world().resource::<CloudCoverPublished>().0.clone().expect("published for the weather that stays");
        assert_eq!(cover.weathers, [Some(WeatherCover { id: live, mean: 0.35, shares: [0.35, 0.0, 0.0, 0.0] }), None], "the departed weather is not carried");
        // A set at weight 0 when the job started gets no new cover from it (its earlier one stays);
        // the covers of weathers outside the job go.
        app.world_mut().resource_mut::<CloudShadowWork>().install_cover(Some(job(vec![0.4], [(0.4, vec![0.4]), (0.0, vec![0.0])])), &[(0, 0)], [Some(gone), Some(live)], [0.0, 1.0], &[0.0]);
        let known = app.world().resource::<CloudShadowWork>().weather_covers.clone();
        let find = |id: u32| known.iter().find(|w| w.id == id).copied();
        assert_eq!((known.len(), find(live), find(gone)), (2, Some(WeatherCover { id: live, mean: 0.0, shares: [0.0; 4] }), Some(WeatherCover { id: gone, mean: 0.25, shares: [0.25, 0.0, 0.0, 0.0] })));
        app.world_mut().resource_mut::<CloudShadowWork>().install_cover(Some(job(vec![0.4], [(0.4, vec![0.4]), (0.0, vec![0.0])])), &[(0, 0)], [Some(live), None], [1.0, 0.0], &[0.0]);
        let known = app.world().resource::<CloudShadowWork>().weather_covers.clone();
        assert_eq!(known, vec![WeatherCover { id: live, mean: 0.4, shares: [0.4, 0.0, 0.0, 0.0] }], "the weather outside the job dropped");
    }

    /// Pending reads can carry only a validated outgoing endpoint at the very start of a fade.
    #[test]
    fn a_pending_texture_can_only_hold_the_measured_start_of_a_fade() {
        let mut app = App::new();
        app.insert_resource(CloudCoverWanted(true))
            .init_resource::<CloudCoverPublished>()
            .init_resource::<CloudShadowWork>()
            .init_resource::<Assets<Image>>()
            .insert_resource(crate::sky::SkyState::default())
            .add_systems(Update, publish_cloud_cover);
        let live = app.world().resource::<crate::sky::SkyState>().weather.form_id;
        {
            let mut work = app.world_mut().resource_mut::<CloudShadowWork>();
            work.cover_at = Some((Vec2::splat(1.0 / REPEAT_UNITS), PLANE_ABOVE_GROUND));
            work.install_cover(Some(Absolute { mean_cover: 0.4, visible: vec![0.4], sets: [(0.4, vec![0.4]), (0.0, vec![0.0])] }), &[(0, 0)], [Some(live), None], [1.0, 0.0], &[0.0]);
            work.cover_pending = true;
        }
        app.update();
        assert!(app.world().resource::<CloudCoverPublished>().0.is_none(), "a texture still being read: nothing published");
        assert_eq!(app.world().resource::<CloudShadowWork>().weather_covers.len(), 1, "the weather's own cover kept");
        app.world_mut().resource_mut::<CloudShadowWork>().cover_pending = false;
        app.update();
        let cover = app.world().resource::<CloudCoverPublished>().0.clone().expect("read: published");
        assert_eq!(cover.weathers[0].map(|w| w.mean), Some(0.4));
        let target = live.wrapping_add(1);
        {
            let mut sky = app.world_mut().resource_mut::<crate::sky::SkyState>();
            sky.prev = Some(sky.weather.clone());
            sky.weather.form_id = target;
            sky.fade = 0.01;
        }
        app.world_mut().resource_mut::<CloudShadowWork>().cover_pending = true;
        app.update();
        let sky = app.world().resource::<crate::sky::SkyState>();
        let held = app.world().resource::<CloudCoverPublished>().0.as_ref().expect("hold measured outgoing endpoint");
        assert_eq!(held.contributors(sky), [(Some(target), 0.0), (Some(live), 1.0)]);
        assert_eq!(held.weathers[0], None, "no target measurement invented");
        app.world_mut().resource_mut::<CloudShadowWork>().cover_refused = Some("unsupported".into());
        app.update();
        assert!(app.world().resource::<CloudCoverPublished>().0.is_none(), "refusal takes priority immediately");
        app.world_mut().resource_mut::<CloudShadowWork>().cover_refused = None;
        app.world_mut().resource_mut::<crate::sky::SkyState>().fade = 0.06;
        app.update();
        assert!(app.world().resource::<CloudCoverPublished>().0.is_none(), "unknown appreciable contributor");
        let mut work = app.world_mut().resource_mut::<CloudShadowWork>();
        work.weather_covers.push(WeatherCover { id: target, mean: 0.7, shares: [0.7, 0.0, 0.0, 0.0] });
        work.cover_pending = false;
        app.update();
        let ready = app.world().resource::<CloudCoverPublished>().0.as_ref().expect("both endpoints measured");
        let sky = app.world().resource::<crate::sky::SkyState>();
        assert_eq!(ready.contributors(sky), live_weathers(sky), "resume exact live weights");
    }

    #[test]
    fn stopped_weather_still_exhausts_the_cover_read_grace() {
        let mut wait = CoverWait::default();
        let start = std::time::Instant::now();
        let pair = Some((2, 1));
        assert_eq!(wait.within_grace(pair, start), pair);
        assert_eq!(wait.within_grace(pair, start + SETTLING_TIME * 2), None);
        assert_eq!(wait.within_grace(pair, start + SETTLING_TIME * 3), None);
    }

    /// #3032 stage 2a: the unwrapped drift crosses
    /// a repeat in either direction without a jump, follows a change of rate (a fade turning
    /// round) without a jump, and ignores a step that is not a number.
    #[test]
    fn the_unwrapped_drift_crosses_a_repeat_without_a_jump() {
        let mut wind = WindPhase::default();
        let (mut drift, mut last) = ([0.1, 0.8], None);
        let mut crossed = [false; 2];
        let mut prev = drift;
        for frame in 0..1200 {
            let secs = 500.0 + f64::from(frame) / 60.0;
            let phase = wind.advance(secs, Vec2::new(0.6, -0.8));
            // The rate turns round halfway (a fade reversing to a layer drifting the other way).
            let rate = if frame < 600 { 0.05 } else { -0.05 };
            drift = advance_drift(drift, last, phase, rate);
            last = Some(phase);
            for i in 0..2 {
                assert!((drift[i] - prev[i]).abs() <= 0.05 / 60.0 + 1e-9, "frame {frame}: the drift jumped");
                crossed[i] |= drift[i].floor() != prev[i].floor();
            }
            prev = drift;
        }
        assert!(crossed[0] && crossed[1], "both axes crossed a repeat");
        assert_eq!(advance_drift([3.25, -7.5], None, [9.0, 9.0], 1.0), [3.25, -7.5], "the first step does not move it");
        assert_eq!(advance_drift([3.25, -7.5], Some([0.0, 0.0]), [f64::NAN, 0.0], 1.0), [3.25, -7.5]);
        assert!((advance_drift([0.95, 0.0], Some([0.0, 0.0]), [-1.0, 0.0], 0.1)[0] - 1.05).abs() < 1e-12, "past one repeat, not wrapped");
    }

    /// The producer and the publisher over a fade of weathers 1 and 2 (layer 0 each, both read),
    /// both rows on, on a clock that never moves (a paused game).
    fn fade_app(dir_tag: &str) -> App {
        bevy::tasks::AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
        let dir = std::env::temp_dir().join(format!("nvf-cloud-{dir_tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut sky = crate::sky::SkyState::default();
        let mut a = weather([Some("sky/a.dds"), None, None, None], [10, 0, 0, 0]);
        a.form_id = 1;
        let mut b = weather([Some("sky/b.dds"), None, None, None], [14, 0, 0, 0]);
        b.form_id = 2;
        sky.weather = a;
        sky.prev = Some(b);
        sky.fade = 0.5;
        let mut app = App::new();
        app.insert_resource(CloudShadows { enabled: true, forced: false })
            .insert_resource(CloudCoverWanted(true))
            .init_resource::<CloudShadowPublished>()
            .init_resource::<CloudCoverPublished>()
            .init_resource::<CloudShadowWork>()
            .init_resource::<Assets<Image>>()
            .init_resource::<Time>()
            .init_resource::<Time<Real>>()
            .insert_resource(sky)
            .insert_resource(GameData::new(Arc::new(fnv_vfs::Vfs::open(&dir, &[]).unwrap()), Arc::new(fnv_nif::Schema::default())))
            .add_systems(Update, (produce_cloud_shadows, publish_cloud_cover).chain());
        app.world_mut().spawn(crate::sky::CloudLayer::for_test(0));
        {
            let mut work = app.world_mut().resource_mut::<CloudShadowWork>();
            work.alphas.insert("sky/a.dds".into(), Alpha::Ready(constant(120)));
            work.alphas.insert("sky/b.dds".into(), Alpha::Ready(constant(60)));
            work.weather_set = Some((1, Some(2)));
            work.ground = Some((None, 0.0));
        }
        app
    }

    /// Updates `app` until `done` holds (the compositions run on the compute pool), at most a
    /// few seconds.
    fn update_until(app: &mut App, done: impl Fn(&App) -> bool) -> bool {
        for _ in 0..4_000 {
            app.update();
            if done(app) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        false
    }

    /// A composition that lands without the volume's cover while a
    /// texture of the fade is refused clears the cover (the weathers' own covers composed before
    /// stay); once the refusal clears, inside the shadows' interval and on a clock that does not
    /// move, the missing cover is composed at once and published, never flat for the whole pause.
    #[test]
    fn a_cover_lost_to_a_refusal_comes_back_in_a_paused_game() {
        let mut app = fade_app("t10");
        {
            let mut work = app.world_mut().resource_mut::<CloudShadowWork>();
            work.weather_covers = vec![WeatherCover { id: 1, mean: 0.47, shares: [0.47, 0.0, 0.0, 0.0] }, WeatherCover { id: 2, mean: 0.24, shares: [0.24, 0.0, 0.0, 0.0] }];
            work.unreadable.insert("sky/b.dds".into());
        }
        // Refused: the shadows compose, the volume's cover does not, nothing is published.
        assert!(update_until(&mut app, |app| {
            let w = app.world().resource::<CloudShadowWork>();
            w.shown.is_some() && w.job.is_none()
        }));
        for _ in 0..5 {
            app.update();
        }
        {
            let w = app.world().resource::<CloudShadowWork>();
            assert!(w.cover_refused.is_some() && !w.absolute_shown && w.weather_covers.len() == 2, "refused: no cover, the weathers' own kept");
            assert_eq!(w.composed_at, 0.0, "the clock does not move");
        }
        assert!(app.world().resource::<CloudCoverPublished>().0.is_none());
        // The refusal clears (the fade is a valid pair again): the cover is composed and published
        // although the shadows are not due on the frozen clock.
        app.world_mut().resource_mut::<CloudShadowWork>().unreadable.clear();
        assert!(update_until(&mut app, |app| app.world().resource::<CloudCoverPublished>().0.is_some()), "the cover comes back in the pause");
        let cover = app.world().resource::<CloudCoverPublished>().0.clone().expect("published");
        assert!(cover.weathers.iter().all(Option::is_some) && cover.mean_cover > 0.0, "{:?}", cover.weathers);
    }

    /// A place outdoors whose weather has no cloud layer: the producer publishes that there is
    /// nothing to compose, neither a cover nor a refusal, so nothing waits for one; a dome whose
    /// layers are not there yet says nothing, and the word goes with the volume.
    #[test]
    fn a_cloudless_place_is_published_as_nothing_to_draw() {
        let mut app = fade_app("empty");
        app.init_resource::<CloudCoverRefused>().init_resource::<CloudCoverEmpty>();
        app.update();
        let empty = |app: &App| app.world().resource::<CloudCoverEmpty>().0;
        assert!(!empty(&app), "a cover is to come");
        {
            let mut sky = app.world_mut().resource_mut::<crate::sky::SkyState>();
            sky.weather.cloud_textures = [None, None, None, None];
            sky.prev = None;
            sky.fade = 1.0;
        }
        app.update();
        assert!(empty(&app), "no cloud layer");
        assert!(app.world().resource::<CloudCoverPublished>().0.is_none() && app.world().resource::<CloudCoverRefused>().0.is_none());
        // A layer the finished dome has no shape for is none either; while the sky is being set up
        // (no dome yet) one may come.
        app.world_mut().resource_mut::<crate::sky::SkyState>().weather.cloud_textures[2] = Some("sky/c.dds".into());
        app.update();
        assert!(!empty(&app), "no dome yet");
        let sky_dome = app.world_mut().spawn(crate::sky::SkyDome::for_test()).id();
        app.update();
        assert!(empty(&app), "the dome draws layer 0 alone");
        let mut layers = app.world_mut().query_filtered::<Entity, With<crate::sky::CloudLayer>>();
        let dome: Vec<Entity> = layers.iter(app.world()).collect();
        for e in dome {
            app.world_mut().despawn(e);
        }
        app.update();
        assert!(empty(&app), "a finished dome without a cloud shape (no clouds.nif) draws none, whatever the weather names");
        assert!(app.world().resource::<CloudCoverPublished>().0.is_none() && app.world().resource::<CloudCoverRefused>().0.is_none());
        app.world_mut().despawn(sky_dome);
        app.update();
        assert!(!empty(&app), "the sky is being set up again");
        app.world_mut().resource_mut::<crate::sky::SkyState>().weather.cloud_textures[2] = None;
        app.update();
        assert!(empty(&app));
        app.world_mut().resource_mut::<CloudCoverWanted>().0 = false;
        app.update();
        assert!(!empty(&app), "off: nothing said");
    }

    /// A texture read for Cloud shadows alone is not looked at for the volume; when the volume
    /// wants the cover it is, once, before any cover is composed from it, and the shadows keep
    /// what they read meanwhile.
    #[test]
    fn the_volumes_look_at_a_texture_waits_for_the_volume() {
        let mut app = fade_app("unchecked");
        app.world_mut().resource_mut::<CloudCoverWanted>().0 = false;
        let dir = std::env::temp_dir().join(format!("nvf-cloud-unchecked-{}", std::process::id()));
        let files = dir.join("textures").join("sky");
        std::fs::create_dir_all(&files).unwrap();
        // An opaque BC1 file (the volume refuses it) and one with transparent texels.
        std::fs::write(files.join("a.dds"), tiny_dds(0x4, b"DXT1", 0, [0; 4], 0, 1, &[0xFF, 0xFF, 0x00, 0x00, 0, 0, 0, 0])).unwrap();
        std::fs::write(files.join("b.dds"), tiny_dds(0x4, b"DXT1", 0, [0; 4], 0, 1, &[0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF])).unwrap();
        app.insert_resource(GameData::new(Arc::new(fnv_vfs::Vfs::open(&dir, &[]).unwrap()), Arc::new(fnv_nif::Schema::default())));
        app.world_mut().resource_mut::<CloudShadowWork>().alphas.clear();
        fn work(app: &App) -> &CloudShadowWork {
            app.world().resource::<CloudShadowWork>()
        }
        assert!(update_until(&mut app, |app| work(app).shown.is_some()), "the shadows compose");
        assert!(work(&app).alphas.values().all(|a| matches!(a, Alpha::Unchecked(Some(_), None))), "read, and not looked at");
        assert!(work(&app).unreadable.is_empty());
        let before = shadows_state(&app);
        app.world_mut().resource_mut::<CloudCoverWanted>().0 = true;
        app.update();
        assert!(work(&app).cover_job.is_none() && alpha_of(&work(&app).alphas, "sky/a.dds").is_some(), "no cover before the look, the alpha kept");
        assert!(update_until(&mut app, |app| work(app).alphas.values().all(|a| matches!(a, Alpha::Ready(Some(_))))), "looked at");
        app.update();
        assert_eq!(work(&app).cover_refused.as_deref(), Some("sky/a.dds"));
        assert!(!work(&app).unreadable.contains("sky/b.dds") && work(&app).cover_job.is_none() && !work(&app).absolute_shown);
        assert_eq!(shadows_state(&app), before, "the shadows as they were");
        // A second read that fails refuses the volume and leaves the shadows their alpha.
        {
            let mut work = app.world_mut().resource_mut::<CloudShadowWork>();
            work.unreadable.clear();
            work.alphas.insert("sky/b.dds".into(), Alpha::Unchecked(constant(60), None));
        }
        std::fs::remove_file(files.join("b.dds")).unwrap();
        assert!(update_until(&mut app, |app| matches!(work(app).alphas.get("sky/b.dds"), Some(Alpha::Ready(_)))), "read again");
        assert!(alpha_of(&work(&app).alphas, "sky/b.dds").is_some_and(|a| a.iter().all(|&v| v == 60)), "the first alpha kept");
        app.update();
        assert_eq!(work(&app).cover_refused.as_deref(), Some("sky/b.dds"));
    }

    /// Waits until every composition in flight in `app`'s producer has finished, so each lands on
    /// the frame after it started (the sequences below do not depend on the pool's timing).
    fn settle_jobs(app: &App) {
        for _ in 0..4_000 {
            let w = app.world().resource::<CloudShadowWork>();
            if w.job.as_ref().is_none_or(|j| j.task.is_finished()) && w.cover_job.as_ref().is_none_or(|j| j.task.is_finished()) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("a composition did not finish");
    }

    /// Cloud shadows' state: when they last composed, the job in flight, what the image holds (the
    /// composition, its reference), the log's state, and the
    /// image's full bytes.
    type ShadowsState = (String, Option<Vec<u8>>);

    fn shadows_state(app: &App) -> ShadowsState {
        let w = app.world().resource::<CloudShadowWork>();
        let bytes = w.image.as_ref().and_then(|h| app.world().resource::<Assets<Image>>().get(h)).and_then(|i| i.data.clone());
        (format!("{:?} {:?} {:?} {} {:?}", w.composed_at, w.job.as_ref().map(|j| &j.landed.composed), w.shown, w.relative_shown, w.logged), bytes)
    }

    /// Waits until Cloud shadows' composition in flight (not the volume's cover) has finished.
    fn settle_shadows(app: &App) {
        for _ in 0..4_000 {
            if app.world().resource::<CloudShadowWork>().job.as_ref().is_none_or(|j| j.task.is_finished()) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("a shadows composition did not finish");
    }

    /// A patterned alpha (no two neighbouring texels alike), so a shift or a swapped layer shows in
    /// the bytes.
    fn patterned(seed: usize) -> Option<Arc<[u8]>> {
        Some(Arc::from((0..MAP_SIZE * MAP_SIZE).map(|i| ((i * 37 + (i / MAP_SIZE) * 11 + seed * 53) % 251) as u8).collect::<Vec<u8>>()))
    }

    /// Cloud shadows have their own job slot and due predicate, so
    /// with both rows on their compositions (start times, inputs, landings, the image's bytes) over
    /// a scripted fade on the game's clock are exactly those with the volume off. The volume's
    /// cover composed at once for a live weather that lacks one lands beside them and leaves their
    /// installed composition, smoothing reference, `composed_at` and image as they were. With
    /// patterned alpha, the two slots' landings staggered (the shadows'
    /// composition always waited for, the cover's only every third frame, so it lands before, with or
    /// after theirs), and the full image bytes and the log's state compared frame by frame.
    #[test]
    fn the_shadows_compose_as_with_the_volume_off() {
        let run = |volume: bool| -> (App, Vec<ShadowsState>) {
            let mut app = fade_app(if volume { "t11-on" } else { "t11-off" });
            app.world_mut().resource_mut::<CloudCoverWanted>().0 = volume;
            {
                let mut work = app.world_mut().resource_mut::<CloudShadowWork>();
                work.alphas.insert("sky/a.dds".into(), Alpha::Ready(patterned(1)));
                work.alphas.insert("sky/b.dds".into(), Alpha::Ready(patterned(2)));
            }
            let mut seen = Vec::new();
            for frame in 0..60u32 {
                app.world_mut().resource_mut::<crate::sky::SkyState>().fade = 0.2 + 0.6 * frame as f32 / 60.0;
                app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(50));
                app.update();
                if frame.is_multiple_of(3) {
                    settle_jobs(&app);
                } else {
                    settle_shadows(&app);
                }
                seen.push(shadows_state(&app));
            }
            settle_jobs(&app);
            (app, seen)
        };
        let (_, off) = run(false);
        let (mut app, on) = run(true);
        let compositions = off.windows(2).filter(|w| w[0] != w[1]).count();
        assert!(compositions >= 6, "the fade recomposes the shadows: {compositions} changes");
        assert!(off.iter().any(|s| s.1.as_ref().is_some_and(|b| b.windows(2).any(|w| w[0] != w[1]))), "patterned bytes in the image");
        for (k, (a, b)) in on.iter().zip(&off).enumerate() {
            assert!(a == b, "frame {k}: the shadows with the volume on {} against off {}", a.0, b.0);
        }
        assert!(app.world().resource::<CloudShadowWork>().absolute_shown, "the volume's cover was composed beside them");
        // A live weather without its own cover: composed at once (the clock held), the shadows'
        // state untouched by its landing.
        app.update();
        settle_jobs(&app);
        app.update();
        let before = shadows_state(&app);
        app.world_mut().resource_mut::<CloudShadowWork>().weather_covers.retain(|c| c.id != 2);
        app.update();
        assert!(app.world().resource::<CloudShadowWork>().cover_job.is_some(), "the uncomposed weather's cover starts at once");
        settle_jobs(&app);
        app.update();
        let w = app.world().resource::<CloudShadowWork>();
        assert!(w.cover_job.is_none() && w.weather_covers.iter().any(|c| c.id == 2), "landed");
        assert_eq!(shadows_state(&app), before, "the shadows' composition, reference, clock and image as they were");
    }

    /// The shadows' first image lands when it would with the
    /// volume off. With the volume's cover installed and no shadows image yet, a shadows composition
    /// that finishes while a texture is being read does not land (a2a2481ce's guard on the shadows'
    /// own state); it lands once the read is over. The read must be of an active layer's texture:
    /// the producer forgets every alpha no active layer names before it computes `loading`, so a read
    /// of any other path is invisible to the guard (the first version of this test read such a path
    /// and the composition landed, as it should have).
    #[test]
    fn the_first_shadows_image_waits_for_a_read_whatever_the_cover_holds() {
        let mut app = fade_app("u1");
        app.world_mut().resource_mut::<CloudShadows>().enabled = false;
        assert!(update_until(&mut app, |app| app.world().resource::<CloudShadowWork>().cover_shown.is_some()), "the volume's cover first");
        assert!(app.world().resource::<CloudShadowWork>().shown.is_none());
        app.world_mut().resource_mut::<CloudShadows>().enabled = true;
        app.update();
        assert!(app.world().resource::<CloudShadowWork>().job.is_some(), "the shadows' first composition starts");
        let stuck = bevy::tasks::AsyncComputeTaskPool::get().spawn(async { std::future::pending::<AlphaRead>().await });
        // The shown weather's layer texture (an active layer) is being read again.
        app.world_mut().resource_mut::<CloudShadowWork>().alphas.insert("sky/a.dds".into(), Alpha::Loading(stuck));
        settle_jobs(&app);
        app.update();
        {
            let w = app.world().resource::<CloudShadowWork>();
            assert!(matches!(w.alphas.get("sky/a.dds"), Some(Alpha::Loading(_))), "the read is an active layer's: the producer keeps it, so `loading` sees it");
            assert!(w.job.as_ref().is_some_and(|j| j.task.is_finished()), "the composition has finished");
            assert!(w.shown.is_none(), "a read in progress and no shadows image: the finished composition does not land");
        }
        app.world_mut().resource_mut::<CloudShadowWork>().alphas.insert("sky/a.dds".into(), Alpha::Ready(constant(120)));
        app.update();
        assert!(app.world().resource::<CloudShadowWork>().shown.is_some(), "the read over: it lands");
    }

    /// The volume switched off while its first cover job is
    /// unfinished (Cloud shadows still on): the slot is dropped whole, nothing of that job installs
    /// later, and a new activation composes its own.
    #[test]
    fn a_cover_job_unfinished_at_switch_off_never_lands() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = fade_app("u2");
        app.update();
        assert!(app.world().resource::<CloudShadowWork>().cover_job.is_some(), "the first cover job runs");
        app.world_mut().resource_mut::<CloudCoverWanted>().0 = false;
        settle_jobs(&app);
        app.world_mut().run_system_once(release_cloud_shadows).unwrap();
        for _ in 0..3 {
            app.update();
            settle_jobs(&app);
        }
        app.world_mut().run_system_once(release_cloud_shadows).unwrap();
        {
            let w = app.world().resource::<CloudShadowWork>();
            assert!(w.cover_job.is_none() && w.cover_shown.is_none() && !w.absolute_shown && w.weather_covers.is_empty(), "the slot dropped whole");
        }
        app.world_mut().resource_mut::<CloudCoverWanted>().0 = true;
        app.update();
        assert!(app.world().resource::<CloudShadowWork>().cover_job.is_some(), "a new activation composes its own");
        assert!(update_until(&mut app, |app| app.world().resource::<CloudShadowWork>().cover_shown.is_some()));
    }

    /// The volume's cover is composed at a fixed sampling phase, so
    /// each weather's own mean is a property of the weather: as the layers drift (two half-opaque
    /// checkered layers per weather, scrolling at different rates, cover more of the sky as their
    /// offsets separate) and the fade moves, recompositions landing mid-fade and at rest return the
    /// same means, and drift alone starts none.
    #[test]
    fn a_recomposition_returns_the_same_means() {
        let mut app = fade_app("t12");
        let checker = |cell: usize| -> Option<Arc<[u8]>> {
            let n = MAP_SIZE;
            Some(Arc::from((0..n * n).map(|i| if ((i % n) / cell + (i / n) / cell).is_multiple_of(2) { 255 } else { 0 }).collect::<Vec<u8>>()))
        };
        {
            let mut sky = app.world_mut().resource_mut::<crate::sky::SkyState>();
            sky.weather.cloud_textures[1] = Some("sky/a1.dds".into());
            sky.weather.cloud_speeds = [10, 40, 0, 0];
            if let Some(b) = sky.prev.as_mut() {
                b.cloud_textures[1] = Some("sky/b1.dds".into());
                b.cloud_speeds = [14, 60, 0, 0];
            }
        }
        app.world_mut().spawn(crate::sky::CloudLayer::for_test(1));
        {
            let mut work = app.world_mut().resource_mut::<CloudShadowWork>();
            for (path, cell) in [("sky/a.dds", 16), ("sky/a1.dds", 16), ("sky/b.dds", 24), ("sky/b1.dds", 24)] {
                work.alphas.insert(path.into(), Alpha::Ready(checker(cell)));
            }
        }
        let means = |app: &App| -> Vec<(u32, f32)> {
            let mut m: Vec<(u32, f32)> = app.world().resource::<CloudShadowWork>().weather_covers.iter().map(|c| (c.id, c.mean)).collect();
            m.sort_by_key(|c| c.0);
            m
        };
        let step = |app: &mut App, game: f32, real: f32| {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(game));
            app.world_mut().resource_mut::<Time<Real>>().advance_by(std::time::Duration::from_secs_f32(real));
            app.update();
            settle_jobs(app);
            app.update();
        };
        step(&mut app, 0.0, 0.0);
        let first = means(&app);
        assert_eq!(first.len(), 2, "{first:?}");
        // Drift alone (the fade held, the real clock past the interval): nothing to compose again.
        step(&mut app, 3.0, 0.5);
        assert!(app.world().resource::<CloudShadowWork>().cover_job.is_none());
        assert_eq!(means(&app), first);
        // Mid-fade: the layers drifted on and the fade moved; each recomposition lands with the same
        // means.
        for (k, fade) in [0.3f32, 0.6, 0.8].into_iter().enumerate() {
            app.world_mut().resource_mut::<crate::sky::SkyState>().fade = fade;
            step(&mut app, 1.7 + k as f32, 0.5);
            let w = app.world().resource::<CloudShadowWork>();
            assert_eq!(w.cover_shown.as_ref().map(|c| c.layers[0].weight), Some(fade), "recomposed at fade {fade}");
            assert_eq!(means(&app), first, "fade {fade}");
        }
        // At rest (the fade over, weather 1 alone): its own mean unchanged.
        {
            let mut sky = app.world_mut().resource_mut::<crate::sky::SkyState>();
            sky.prev = None;
            sky.fade = 1.0;
        }
        step(&mut app, 2.3, 0.5);
        let rest = means(&app);
        assert_eq!(rest, vec![first[0]], "at rest: the one weather, the same mean");
    }
}
