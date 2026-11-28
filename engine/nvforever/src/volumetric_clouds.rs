//! Volumetric clouds (#2991, a first prototype; `graphics.rs` switch `volumetric-clouds`, off
//! by default and in every preset; docs/design/graphics-enhanced.md §3.23). An engineering
//! choice for the enhanced look, not a fact about the original game (which draws its cloud
//! layers as flat textures on the sky dome), and never used to explain a difference from it.
//!
//! The weather's own cloud layers, as Cloud shadows composes them (cloud_shadows.rs), become
//! the cover of a slab of cloud above the world that is ray-marched and lit per pixel:
//!
//! - **Where in the frame** [design]: the slab is marched by the volumetric medium's trace
//!   pass (medium.rs, #3032: after the depth prepass, before the main pass, one ray per pixel
//!   of the main pass's render size into the far buffer, through this source's march, the seam
//!   `vc_trace_refined`), and composed as a sky part of its own in the main pass. The sky dome's
//!   own geometry drawn again ([`proxy_mesh`]: its exact vertices, at the dome's transform)
//!   with the depth test Equal ([`draw_on_dome`]), so it draws only where the dome's own
//!   depth is the nearest (distant LOD terrain reaches beyond `sky::SKY_DEPTH_FROM`, so no
//!   fixed distance would do), with a material of our own
//!   ([`CloudVolumeMaterial`], volumetric_clouds.wesl) whose fragment reads the far buffer
//!   where its view ray lies. Drawn in the sky's transparent band ([`SORT_BIAS`]: after the stars, the
//!   moon and the flat cloud layers' sort keys, before the world's blended things),
//!   depth-tested, so a pixel behind the world costs nothing, premultiplied over the dome
//!   (its sun disc and glare), the stars and the moon: thick cloud hides them. The clouds
//!   never pass in front of terrain (a sky-only volume). Before the temporal resolve, so TAA,
//!   FSR and DLSS treat it as they treat the sky. Only the main world camera draws it
//!   ([`VOLUME_LAYER`]); while it is drawn the flat layers move to [`FLAT_CLOUD_LAYER`]
//!   ([`hide_flat_layers`], in the same frame as the volume shows or goes), out of the main
//!   camera's view. The scope and extra cameras see those layers. The water camera sees them
//!   until its own reflected volume is ready. The volume shows only once the medium has written its buffer at the
//!   right size (medium.rs, the handover). The march (volumetric_clouds_march.wesl) is a
//!   module of plain functions the medium's trace imports.
//! - **Amount and placement** (#3032 stage 2a) [design]: the weather
//!   sets **how much** cloud there is (the mean of the absolute cover `1 - Π (1 - alpha ×
//!   weight)` of Cloud shadows' composition, `CloudCoverPublished`, blended across a fade) and
//!   its colours; **where** the clouds are is a slow, non-repeating field of our own in world x
//!   and z, moved by the producer's unwrapped drift (so the clouds travel with the weather's wind
//!   and layer speeds), and the weather's cloud texture pattern is not the clouds' pattern (the
//!   textures' patches are a few thousand units wide; masses wider than tall on them stay a thin
//!   layer). A place holds cloud where the field passes a threshold set so the share of sky
//!   with cloud follows the amount ([`sky_share`], [`place_threshold`], the lean model's
//!   volumetric_clouds_model2.rs `placement_threshold`); how far it passes is how deep inside its mass the
//!   place is, and its local top rises with it, varied by a second slow field (the vigour), over a flat
//!   base. The domain's base is the shadows'
//!   plane (20,000 units above the ground reference, cloud_shadows.rs); [`REFERENCE_HEIGHT`] tall,
//!   bent down over the distance as a shell centred under the camera so the base meets a
//!   horizontal ray at [`HORIZON`] ([`shell_radius`], [`span`]). The ground shadows keep the
//!   weather's texture (they do not match the volume in this round).
//! - **Shape** (Schneider, "The Real-Time Volumetric Cloudscapes of Horizon Zero Dawn",
//!   SIGGRAPH 2015 Advances in Real-Time Rendering: a low-frequency Perlin-Worley base and a
//!   higher-frequency Worley erosion; Bauer 2019, slides 17-18: one 3D lookup carving the shape
//!   the placement and the height profile define [documented: cited publications]): the
//!   lean model (volumetric_clouds_model2.rs): the types' profile over each place's own height against
//!   the generated base noise, the generated Worley bands moving the surface; the textures are made by
//!   our own code when the row is first switched on (cloud_noise.rs), nothing is shipped.
//! - **Light** (Hillaire, "Physically Based Sky, Atmosphere and Cloud Rendering in Frostbite",
//!   SIGGRAPH 2016 course: Beer-Lambert extinction, a two-lobe Henyey-Greenstein phase and the
//!   energy-conserving step integral [documented: cited publication]): the weather's sunlight
//!   colour from the sun's disc direction by day, its night colour from the moon's by night
//!   ([`direct_light`]), shadowed by a short march toward the light; an ambient term from the
//!   weather's cloud layer colours (PNAM, [`ambient_colour`]) brighter toward the top and dark
//!   at the base of thick cover. With
//!   albedo 1 the step integral makes the colour a weighted mean of what the steps emit, so it
//!   stays inside the weather's colours plus the bounded direct term ([`SUN_GAIN`] ×
//!   [`PHASE_CAP`]): an overcast weather stays grey, a dusk weather warm.
//! - **Space**: as sky.wesl: the weather's linear colours times the sky level, premultiplied,
//!   through the same inverse of Tony McMapface when the clip tonemapper is not installed
//!   (once, in the composition). The main view is HDR while the row is on (graphics.rs).
//! - **Follows the weather** from `SkyState` and the cover every frame: no weather names, no
//!   per-weather result kept past a change; fades blend each weather's own cover and its colours
//!   by the live fade each frame ([`blend_covers`]).
//!
//! Off, indoors, or with no cover (a weather without cloud textures, or before the first
//! composition): nothing is drawn and the flat layers stay as they are. Off there is no mesh
//! and no material, the material type's systems wait behind a closed gate (vendored bevy_pbr
//! `MaterialGate`) and the producer is not started for it.
//!
//! Debug aids (read once at the start; development tools, they do not make a run
//! unattended; dev_aids.rs: not in release builds): `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS=<multiplier>` scales the density (0 to
//! [`MAX_DEBUG_DENSITY`]), `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_STEPS=<n>` the march's quality
//! ([`MIN_STEPS`] to [`MAX_STEPS`], [`step_near`]), `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_VIEW=
//! cover|vigour|count` draws the placement field against its threshold or the local height's
//! share, grey, on the bent base, or the density evaluations per ray ([`CloudView`]), `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_HEIGHT=
//! <units>` the domain's height, `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_HORIZON=<units>` where the
//! bent base meets the horizon.

use crate::cloud_shadows::{CloudCoverOutput, CloudCoverPublished, CloudCoverWanted, WeatherCover};
use crate::fly::FlyCam;
use crate::medium::{MediumOutput, MediumRequest, SourceFrame};
use crate::sky::SkyState;
use bevy::camera::visibility::RenderLayers;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialGate, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::camera::TemporalJitter;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::{Shader, ShaderRef};
use fnv_world::sky::{smoothstep, ColorType};

#[path = "volumetric_clouds_air.rs"]
mod air;

/// Render layer of the volume: only the main world camera has it (main.rs), so the
/// reflection and lens cameras never run the march [design].
pub const VOLUME_LAYER: usize = 21;

// --- The constants that trade cost for look, in one place [engineering choice: starting
// values to judge from pictures; none is a fact about the original game]. ---

/// The march's quality value (`NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_STEPS`, [`MIN_STEPS`] to
/// [`MAX_STEPS`]): since #3032 stage 2a it scales the step's length, [`STEP_NEAR`] x
/// `VIEW_STEPS / steps` ([`step_near`]), so 28 is the default and 56 halves the step.
pub const VIEW_STEPS: u32 = 28;
pub const MIN_STEPS: u32 = 4;
pub const MAX_STEPS: u32 = 128;
/// The view step's length near (world units) [engineering choice, checkpoint A's fixed-step
/// march: under half the erosion's cell (940), so a ray straight up through a cloud samples the
/// erosion at least twice a cell; the cost is ignored at this checkpoint].
pub const STEP_NEAR: f32 = 400.0;
/// A ray entering the domain far off takes steps of up to this many footprints there (its
/// spread times the entry distance), but never longer than half the finest detail still there
/// ([`step_bound`]: the erosion's cell while any of it is kept, else the low noise's finer cell,
/// which never fades), so the fixed-step march is a true reference for the look
/// [engineering choice].
pub const STEP_FOOTPRINT: f32 = 2.0;
/// The most density evaluations (view steps and light taps) one ray may ask for, whatever the
/// settings: stage 1's worst case ([`STAGE1_RAY_EVALS`]: 28 view
/// steps, each with 6 light taps) times [`RAY_BUDGET_FACTOR`]. The view steps a ray may take are
/// this over one plus the light taps, less two ([`ray_steps_most`]: 2,785 with 8 taps), shared
/// by its two stretches in proportion to what the spacing bound asks of each ([`ray_plan`]); a ray
/// that would need more takes wider steps than the bound (the march is then capped; the medium
/// logs it once per activation, [`refined_rays_capped`]) [engineering choice: 128 so that no camera
/// position at the default quality is capped (a level ray inside the domain asks about 1,400
/// steps, a ray 2 degrees up from the ground about 470), while the debug settings that ask for
/// more (quality 128, an 80,000-unit domain, the reference mode at scale 1: about 9,100 steps on
/// a level ray) are held to it; the work of a ray is at most 25,088 evaluations against the
/// 147,000 two capped stretches allowed before].
pub const STAGE1_RAY_EVALS: u32 = 28 * (1 + 6);
pub const RAY_BUDGET_FACTOR: u32 = 128;
pub const RAY_BUDGET: u32 = STAGE1_RAY_EVALS * RAY_BUDGET_FACTOR;
/// The view march (#3032 stage 2c, checkpoint C1; march `vc_trace_refined`, CPU mirror
/// [`refined_march`]): the support is probed every
/// [`COARSE_THIN`] units along the ray under a weather whose amount is at most
/// [`COARSE_THIN_UP_TO`], every [`COARSE_FAIR`] above, both with the domain (the masses scale with
/// it), wider where the placement's finest octave is gone ([`coarse_step`]) [engineering choice].
/// A numerical mirror of the fully resolved placement, not committed, along
/// horizontal transects (96 lines of 1,000,000 units sampled every 100, chords cut by the ends left
/// out) misses 0.41 % of the mass chords at amount 0.05 with 800 (1.55 % with 1,600), 0.45 % at 0.3
/// with 1,600 (1.66 % with 3,200), none at 0.8; of the chords of masses wider than 2,000 units (the
/// components of a 300,000-unit square) 0.10 % at 0.05 and 0.33 % at 0.3: under the aim of
/// 0.5 % [hypothesis: the placement of before the banks, not repeated since].
pub const COARSE_THIN: f32 = 800.0;
pub const COARSE_FAIR: f32 = 1_600.0;
pub const COARSE_THIN_UP_TO: f32 = 0.1;
/// The Volumetric clouds row's choices, in its order: Off, then the levels ([`CloudLevel`]).
pub const CLOUD_CHOICES: [&str; 4] = ["Off", "Performance", "Normal", "High"];

/// The level the clouds are drawn at while the row is on [design, 2026-11-23]:
/// the far buffer's scale (medium.rs [`crate::medium::BUFFER_SCALE`], whatever the source) and how
/// far apart the refined march probes the support ([`VolumeInputs::probe`]). The fine step, the tile
/// side and everything else are the same at each. Where the row was an on/off switch, on is Normal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CloudLevel {
    Performance,
    #[default]
    Normal,
    /// The look judged by eye on 2026-11-23 as it was.
    High,
}

impl CloudLevel {
    /// The name the row shows and the INI keeps ([`CLOUD_CHOICES`]).
    pub fn name(self) -> &'static str {
        match self {
            CloudLevel::Performance => "Performance",
            CloudLevel::Normal => "Normal",
            CloudLevel::High => "High",
        }
    }

    /// What the main pass's render size is divided by for the medium's far buffer.
    pub fn buffer_scale(self) -> f64 {
        match self {
            CloudLevel::Performance => 4.0,
            CloudLevel::Normal => 3.0,
            CloudLevel::High => crate::medium::BUFFER_SCALE,
        }
    }

    /// The factor on the refined march's coarse spacing ([`coarse_spacing`]).
    pub fn probe_factor(self) -> f32 {
        match self {
            CloudLevel::Performance | CloudLevel::Normal => 2.0,
            CloudLevel::High => 1.0,
        }
    }
}

/// The most fine cells a block of the refined march holds (march `VC_BLOCK_MOST`): its entry or
/// exit is bracketed by at most four halvings [engineering choice: the coarse spacing over the near
/// step is 2 or 4; far off, where the footprint widens the spacing, a block stays at most 16 cells].
/// With a level's doubled spacing ([`CloudLevel::probe_factor`]) the near block is 16 cells where it
/// was 8 (120-unit cells, 1,920 and 960 units), so four halvings still bracket its ends; where the
/// place width widens the spacing the cap is met sooner and a block is 16 cells where 32 fit: shorter
/// than asked for, never longer [design; the test `the_levels_double_the_probe_spacing_within_the_block_cap`].
pub const REFINE_BLOCK_MOST: u32 = 16;
/// The refined count view's whites per channel (march `VC_COUNT_PROBES`, `VC_COUNT_FINE`,
/// `VC_COUNT_TAPS`): R the occupancy probes (bracketing included), G the fine density samples, B
/// the light taps [engineering choice: in a numerical mirror, not committed, a ray 10 degrees up from the ground
/// probes about 110 to 280 times, samples tens to a few hundred cells, and lit samples take eight
/// taps each].
pub const COUNT_PROBES: f32 = 512.0;
pub const COUNT_FINE: f32 = 256.0;
pub const COUNT_TAPS: f32 = 2_048.0;
/// Taps of the march toward the light, the growth of their intervals from the sample outward,
/// and the longest reach (game units) [engineering choice: eight taps over intervals growing by
/// half each, so the first is about 2 % of the reach (480 units at 24,000) and the last about
/// a third; 24,000 units cross a tall tower's width; the reach is also cut where the light ray
/// leaves the domain's top ([`light_reach`])].
pub const LIGHT_STEPS: u32 = 8;
pub const LIGHT_GROWTH: f32 = 1.5;
pub const LIGHT_REACH: f32 = 24_000.0;
/// The light is marched only where the shape is above this; thinner cloud counts as lit.
pub const LIGHT_THRESHOLD: f32 = 0.02;
/// The march stops once this little of the background shows through.
pub const STOP_TRANSMITTANCE: f32 = 0.02;
/// The domain's height above the cloud shadows' plane (game units, about 70 a metre) that the sizes and the
/// probe spacing are stated at ([`coarse_spacing`] scales with the domain over it) and the tests' fixture
/// [engineering choice]. The domain is the space cloud may occupy; each place's own cloud
/// rises to its mass's height. A run starts at 24,000 ([`START_VALUES`]);
/// `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_HEIGHT` sets it.
pub const REFERENCE_HEIGHT: f32 = 40_000.0;
pub const MIN_DOMAIN: f32 = 16_000.0;
pub const MAX_DOMAIN: f32 = 80_000.0;
/// Where the bent base meets a horizontal ray from the ground reference (game units); the
/// clouds fade out by the distance where a ray enters the domain, from [`HORIZON_FADE_FROM`] of
/// this to all of it (from a camera on the ground about 1.7 to 0 degrees up) [engineering
/// choice]. `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_HORIZON` sets it.
pub const HORIZON: f32 = 400_000.0;
pub const MIN_HORIZON: f32 = 100_000.0;
pub const MAX_HORIZON: f32 = 1_500_000.0;
pub const HORIZON_FADE_FROM: f32 = 0.75;
/// The source's range beyond the horizontal exit through the domain's top from the ground
/// reference: a margin for a camera below that reference ([`source_range`]).
pub const RANGE_MARGIN: f32 = 1.05;
/// The placement (march `vc_place_octave`, `vc_m2_column`): a field of three octaves of
/// 2D lattice value noise in world x and z, the first with a cell of [`PLACE_CELL_PER_DOMAIN`]
/// domain heights (40,000 units at the default), the next [`PLACE_OCTAVES`] times finer (not
/// whole numbers, so the sum never repeats) for irregular outlines, weighted [`PLACE_WEIGHTS`]
/// (the finer octaves raised from 0.26 and 0.12, so fair weather is many heaped clouds
/// in a range of sizes rather than a few giants); a place holds cloud where it passes
/// a threshold of the weather's amount, and its local height grows with how far it passes, nothing
/// at its edge. Cell and height both scale with the domain, so the domain's heights
/// are one cloudscape at different scales: taller and broader, never thinner. A place
/// rounds in over 0.03 of the field over its threshold (march `VC_MASS_EDGE`); every place with cloud is at
/// least [`SHEET`] tall [engineering choice; the sizes measured, graphics-enhanced.md §3.23].
pub const PLACE_CELL_PER_DOMAIN: f32 = 1.0;
/// The second and third octaves' frequencies over the first's, and the three weights.
pub const PLACE_OCTAVES: [f32; 2] = [2.17, 4.73];
/// (The march writes them as literals: the tests hold them equal.)
#[cfg_attr(not(test), allow(dead_code))]
pub const PLACE_WEIGHTS: [f32; 3] = [0.40, 0.35, 0.25];
pub const SHEET: f32 = 1_200.0;
/// The share of sky with cloud for the weather's amount `a`: `1 - (1 - a)^SHARE_POWER` (0.19
/// at 0.1, 0.36 at 0.2, 0.51 at 0.3, 0.75 at 0.5, 0.99 at 0.9, a closed deck at 1), about 1.5
/// times the opacity aimed at ([`target_opacity`]) under a thin weather [engineering choice]. An
/// earlier round tried 1.6 for more sky between fair-weather clouds; at that power the share sat too close to
/// the opacity aimed at (0.155 against 0.121 at amount 0.1), the extinction that reaches it
/// was large and differed between halves of the measured regions (96 against 24), and the
/// table's convergence bound failed, so it is 2 again.
pub const SHARE_POWER: f32 = 2.0;
/// The placement field's distribution, measured: the threshold it passes over each share of
/// the plane ([`PLACE_SHARES`]), from the CPU mirror of the field over three areas of 2,160,000
/// units square (54 first-octave cells a side; with knots down
/// to a share of 1/1024 so thin weather keeps its clouds), the mean; between them the areas
/// differed by up to 0.009 below the share 1 [measured; the test `the_placement_field_follows_its_distribution`
/// holds it]; none at share 0, all of it at 1.
pub const PLACE_SHARES: [f32; 25] = [0.0, 0.000976562, 0.001953125, 0.00390625, 0.0078125, 0.015625, 0.03125, 0.0625, 0.125, 0.1875, 0.25, 0.3125, 0.375, 0.4375, 0.5, 0.5625, 0.625, 0.6875, 0.75, 0.8125, 0.875, 0.9375, 0.96875, 0.984375, 1.0];
pub const PLACE_THRESHOLDS: [f32; 25] = [1.0, 0.8535, 0.8348, 0.8140, 0.7908, 0.7635, 0.7316, 0.6935, 0.6465, 0.6133, 0.5862, 0.5623, 0.5402, 0.5191, 0.4985, 0.4779, 0.4568, 0.4346, 0.4105, 0.3832, 0.3502, 0.3037, 0.2658, 0.2338, 0.0];
/// Each type's profile over a place's own height: eight knots at h = 0, 1/7 .. 1, linear
/// between ([`knots_at`]); a knot list may hold a gap (zeros between two bands). The sheet thins
/// from its lower half; the cumulus is widest low and narrows to a rounded top; the tower stays
/// broad to near its top. The base itself is flat.
pub const KNOTS_SHEET: [f32; 8] = [1.0, 1.0, 0.9, 0.7, 0.45, 0.2, 0.05, 0.0];
pub const KNOTS_CUMULUS: [f32; 8] = [1.0, 1.0, 0.95, 0.85, 0.72, 0.55, 0.32, 0.0];
pub const KNOTS_TOWER: [f32; 8] = [1.0, 1.0, 1.0, 0.97, 0.92, 0.85, 0.65, 0.0];
/// The tower is chosen locally: where a place is deep in its mass (the field over its threshold
/// from [`TOWER_DEPTH`]) and vigorous ([`TOWER_VIGOUR`]) its weight is the weather's times
/// [`TOWER_GAIN`]; nowhere else.
pub const TOWER_DEPTH: [f32; 2] = [0.04, 0.12];
pub const TOWER_VIGOUR: [f32; 2] = [0.55, 0.85];
pub const TOWER_GAIN: f32 = 4.0;
/// The vigour (march `vc_vigour`, [`vigour_scale`]): it scales a mass's height by
/// `VIGOUR_LOW + (VIGOUR_HIGH - VIGOUR_LOW) v^VIGOUR_POWER` (a mild spread; the towers are
/// the types' choice, [`TOWER_VIGOUR`]). Its first octave's cell is [`VIGOUR_CELL_PER_DOMAIN`] domain heights (slower than the
/// masses, so it sets their heights apart without carving them), the second
/// octave [`VIGOUR_OCTAVE`] times finer (not a whole number, so the sum never repeats with the
/// cover's repeat) [engineering choice].
pub const VIGOUR_LOW: f32 = 0.75;
pub const VIGOUR_HIGH: f32 = 1.25;
pub const VIGOUR_POWER: f32 = 1.0;
pub const VIGOUR_CELL_PER_DOMAIN: f32 = 3.0;
pub const VIGOUR_OCTAVE: f32 = 2.13;
/// How far a base may rise (units), on the vigour's slow first-octave lattice: the bases read
/// as one level [engineering choice, A2: "small and slow"].
pub const BASE_RISE: f32 = 600.0;
/// The opacity aimed at ([`target_opacity`]): seen straight up, uniform cover `c` hides on
/// average `1 - (1 - COVER_HOLD × c)^OPACITY` of the sky (0.12 at 0.1, 0.24 at 0.2, 0.57 at
/// 0.5, 0.93 at 0.9): about as much as the flat layers' cover, a little more.
pub const OPACITY: f32 = 1.25;
pub const COVER_HOLD: f32 = 0.98;
/// The weather's amounts (mean covers) of the extinction table ([`EXTINCTION_TABLE`]); between
/// them it is linear, outside them held.
pub const CAL_COVERS: [f32; 8] = [0.005, 0.05, 0.1, 0.2, 0.35, 0.55, 0.75, 0.95];
/// The optical depth per unit of a column's mean shape, by cover ([`CAL_COVERS`]): what the unresolved
/// sheet and the light's column take; the extinction per unit is this over the height it is taken over. The
/// values are 44e923c33's, solved from its carved field so that the mean opacity straight up was
/// [`target_opacity`] [measured there: `the_extinction_table_is_the_accepted_one`].
pub const EXTINCTION_TABLE: [f32; 8] = [5.741, 3.775, 3.560, 2.732, 2.596, 2.709, 3.212, 4.344];

/// The vigour's lattice noise is linearly remapped to 0..1 from this range of its values (two
/// octaves of value noise gather around 0.5) ([`spread`]).
pub const NOISE_LO: f32 = 0.2;
pub const NOISE_HI: f32 = 0.8;
/// The base's rounding in (units).
pub const BASE_ROUND: f32 = 300.0;
/// The phase ([`phase`]): forward and back Henyey-Greenstein g, the back lobe's weight, and
/// the cap on the phase (isotropic = 1).
pub const G_FORWARD: f32 = 0.6;
pub const G_BACK: f32 = -0.25;
pub const BACK_WEIGHT: f32 = 0.3;
pub const PHASE_CAP: f32 = 4.0;
/// The direct light's share of the weather's sunlight colour (times the phase and the
/// transmittance toward the light).
pub const SUN_GAIN: f32 = 0.5;
/// The ambient term's share of the cloud colours.
pub const AMBIENT_GAIN: f32 = 0.7;
/// The direct light grows from nothing at the horizon to full at this elevation (degrees).
pub const LIGHT_FADE_DEG: f32 = 4.0;
/// The most `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS` scales the density by.
pub const MAX_DEBUG_DENSITY: f32 = 4.0;
/// The most the relief's and the erosion's strength aids (`..._SHAPE`, `..._EROSION`) may be (#3032
/// stage 2e: 2 until tuning by eye ended at 2 for both).
pub const MAX_STRENGTH: f32 = 4.0;
/// The most the packed ambient and direct light may hold (linear, per channel): a step emits
/// at most the ambient plus the direct light times [`PHASE_CAP`], which this keeps under the
/// far buffer's half floats (65,504) whatever a mod's sky level is
/// [engineering choice: far above any weather's light, so nothing real is clipped].
pub const MAX_LIGHT: f32 = 30_000.0;

/// The proxy is the sky dome's own mesh at the dome's own transform, drawn with the depth
/// test Equal [design]: the dome is opaque and writes depth, and the vertex position is
/// `@invariant` in both of bevy_pbr's vertex outputs (engine/vendor/README.md, "equal-depth
/// passes"), so the proxy's depth is the dome's to the bit where the dome is the nearest
/// thing drawn (sky) and differs wherever anything of the world lies in front of it, however
/// near the dome. A fixed sphere did not hold (the outdoor far plane is 400,000 units and the
/// distant LOD's level-16 chunks of 65,536 units, selected up to the default 125,000, reach
/// past 200,000, while near the horizon the dome is up to about 343,000 off), and neither did
/// the dome moved in by 0.2 % (terrain in that last gap took cloud colour under the default
/// GreaterEqual test).
/// The volume's transparent sort key: the sky's band (`sky::sorts_with_sky`, which FSR draws
/// into its reactive reference), after the flat cloud layers' keys (bias + 0..7), the moon's
/// and the stars' [design]. Sort only: a material of our own has no raster depth bias.
pub const SORT_BIAS: f32 = crate::sky::SKY_SORT_BIAS + 10.0;
/// The proxy's draw pipeline's label: the pipeline log names it (smoke `volumetric-clouds`) and
/// the medium's handover waits for it (medium.rs).
pub const PIPELINE_LABEL: &str = "volumetric_clouds_pipeline";
/// Frames with no volume material left before the gate closes.
const IDLE_FRAMES: u32 = 3;
/// The cover is logged again when its mean moved by this much.
const LOG_COVER_STEP: f32 = 0.1;
/// A mean cover at or below this is none: the volume is not drawn (and the flat layers stay).
pub const NO_COVER: f32 = 1e-4;

/// Whether a mean cover is any ([`NO_COVER`]); not a number is none.
fn has_cover(mean: f32) -> bool {
    mean > NO_COVER
}

/// Whether a height share lies strictly inside a cloud's own height; not a number does not.
fn inside_slab(h: f32) -> bool {
    h > 0.0 && h < 1.0
}

/// volumetric_clouds_march.wesl, imported by the medium's trace (medium_trace.wesl) as
/// `nvforever::volumetric_clouds_march`.
const MARCH_SHADER: Handle<Shader> = bevy::asset::uuid_handle!("3f7c2a91-5d4e-4b8a-9c16-0e2d7b5a8f43");

// ---------------------------------------------------------------------------------------
// Pure parts (CPU mirrors of the march module's functions, for the tests)
// ---------------------------------------------------------------------------------------

fn unit(x: f32) -> f32 {
    if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 }
}

fn non_negative(x: f32) -> f32 {
    if x.is_finite() { x.max(0.0) } else { 0.0 }
}

/// A profile's value at a place's own height share `h` (0 its base, 1 its local top) from its
/// eight knots (h = 0, 1/7 .. 1), linear between; 0 outside the cloud (march `vc_knots`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn knots_at(h: f32, knots: &[f32; 8]) -> f32 {
    if !inside_slab(h) {
        return 0.0;
    }
    let x = h * 7.0;
    let i = (x.floor() as usize).min(6);
    knots[i] + (knots[i + 1] - knots[i]) * (x - i as f32)
}

/// The profile for the local type weights `w` (sheet, cumulus, tower) (march `vc_profile`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn profile_at(h: f32, w: Vec3) -> f32 {
    w.x * knots_at(h, &KNOTS_SHEET) + w.y * knots_at(h, &KNOTS_CUMULUS) + w.z * knots_at(h, &KNOTS_TOWER)
}

/// The weather's mix of the types (sheet, cumulus, tower) for its amount and its classification
/// bias ([`class_bias`], already blended across a fade): thin weather some sheets among small
/// cumulus, fair weather cumulus, heavy weather a deck of sheet and cumulus with towers; the
/// bias added, held non-negative, normalised. The tower's share never falls as the amount rises
/// under a fixed bias.
pub fn type_mix(amount: f32, bias: Vec3) -> Vec3 {
    let a = unit(amount);
    let sheet = 0.55 * (1.0 - smoothstep(0.05, 0.3, a)) + 0.3 * smoothstep(0.55, 0.9, a);
    let tower = 0.3 * smoothstep(0.35, 0.85, a);
    let cumulus = (1.0 - sheet - tower).max(0.05);
    let b = if bias.is_finite() { bias } else { Vec3::ZERO };
    let raw = (Vec3::new(sheet, cumulus, tower) + b).max(Vec3::ZERO);
    let total = raw.x + raw.y + raw.z;
    if total > 1e-6 { raw / total } else { Vec3::Y }
}

/// A weather classification's bias of the type mix (sheet, cumulus, tower) (DATA flags: 1
/// Pleasant, 2 Cloudy, 4 Rainy, 8 Snow [documented: xEdit; GECK wiki GetWeatherTraitNumeric]):
/// pleasant toward cumulus, cloudy toward the deck and the tower, rainy toward the tower, snow
/// toward the sheet; the bits add; other bits, and none, give no bias (the amount alone). The
/// record's separate rainy flag is the same bit and is not counted again.
pub fn class_bias(classification: u8) -> Vec3 {
    let mut b = Vec3::ZERO;
    if classification & 1 != 0 {
        b += Vec3::new(-0.05, 0.1, -0.05);
    }
    if classification & 2 != 0 {
        b += Vec3::new(0.15, 0.0, 0.15);
    }
    if classification & 4 != 0 {
        b += Vec3::new(0.1, -0.1, 0.3);
    }
    if classification & 8 != 0 {
        b += Vec3::new(0.3, -0.1, 0.0);
    }
    b
}

/// The bias of a sky: the shown weather's, blended with the fading one's by the fade (toward the
/// shown weather) while a fade runs.
pub fn sky_bias(shown: u8, fading: Option<u8>, fade: f32) -> Vec3 {
    let b = class_bias(shown);
    match fading {
        Some(prev) => class_bias(prev).lerp(b, unit(fade)),
        None => b,
    }
}

/// What the volume takes from the cover this frame ([`blend_covers`]):
/// the weather's amount, the live contributors' normalised weights (the live order: the shown
/// weather, then the one fading out), and the tint shares and their weathers the ambient colour
/// is weighted by ([`cloud_parts`]), all from the same weights.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BlendedCover {
    pub amount: f32,
    pub weights: [f32; 2],
    pub layer_cover: [[f32; 4]; 2],
    pub layer_weathers: [Option<u32>; 2],
}

/// Blend the measured opacity and layer tint shares of the requested contributors.
/// CloudCoverOutput::contributors selects the outgoing endpoint during the bounded
/// initial read. Every positive weight here requires a matching, finite measurement.
pub fn blend_covers(live: [(Option<u32>, f32); 2], covers: &[Option<WeatherCover>; 2]) -> Option<BlendedCover> {
    let mut out = BlendedCover::default();
    for (k, ((id, w), cover)) in live.iter().zip(covers).enumerate() {
        let w = unit(*w);
        if w <= 0.0 {
            continue;
        }
        let c = (*cover).filter(|c| Some(c.id) == *id && c.mean.is_finite() && c.shares.iter().all(|s| s.is_finite()))?;
        out.weights[k] = w;
        out.amount += w * unit(c.mean);
        out.layer_cover[k] = c.shares.map(|s| w * non_negative(s));
        out.layer_weathers[k] = Some(c.id);
    }
    (out.weights[0] + out.weights[1] > 0.0).then_some(out)
}

/// The local type weights (march `vc_types`) for the uniform's `mix` (w 1: forced, taken as it
/// is), a place over its threshold by `over` with vigour `vigour`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn local_types(mix: Vec4, over: f32, vigour: f32) -> Vec3 {
    if mix.w > 0.5 {
        return mix.truncate();
    }
    let local = smoothstep(TOWER_VIGOUR[0], TOWER_VIGOUR[1], vigour) * smoothstep(TOWER_DEPTH[0], TOWER_DEPTH[1], over);
    let raw = Vec3::new(mix.x, mix.y, mix.z * TOWER_GAIN * local);
    let total = raw.x + raw.y + raw.z;
    if total > 1e-6 { raw / total } else { Vec3::Y }
}

/// The base rounding in over [`BASE_ROUND`] units at `above` units over it (march `vc_shape`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn base_fade(above: f32) -> f32 {
    smoothstep(0.0, BASE_ROUND.max(1.0), above)
}

/// The low noise linearly remapped to 0..1 from [`NOISE_LO`]..[`NOISE_HI`] (march `vc_spread`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn spread(n: f32) -> f32 {
    unit((unit(n) - NOISE_LO) / (NOISE_HI - NOISE_LO))
}
/// Each band's level-0 mean (what it fades toward as the footprint grows) and the bands' middle
/// half (their 25th and 75th percentiles, spread over 0..1 for the cut) [measured
/// on a numerical mirror of the generator; the test `the_noise_constants_are_the_bakes` holds the
/// pinned bytes' means within 0.01 and quartiles within 0.02].
pub const ERODE_MEANS: [f32; 3] = [0.490, 0.495, 0.485];
pub const NOISE_ERODE: [f32; 2] = [0.365, 0.615];
/// The base noise's middle half: its level-0 values from the 25th to the 75th percentile, what the spread
/// maps to 0.25..0.75 [measured on a numerical mirror of the generator, 0.369 and 0.545;
/// the test `the_noise_constants_are_the_bakes` holds the pinned bytes' quartiles within 0.02].
pub const NOISE_LOBE: [f32; 2] = [0.37, 0.545];

/// How much of a band of cells `cell` units wide a cast width `width` keeps (march
/// `vc_band_keep`): all from four cast widths a cell, none from two.
#[cfg_attr(not(test), allow(dead_code))]
pub fn band_keep(cell: f32, width: f32) -> f32 {
    1.0 - smoothstep(0.25 * cell, 0.5 * cell, width)
}

/// The erosion bands' cells (units): its repeat over 3, 6 and 12.
#[cfg_attr(not(test), allow(dead_code))]
pub fn erode_cells() -> [f32; 3] {
    let r = crate::cloud_noise::EROSION_REPEAT;
    [r / 3.0, r / 6.0, r / 12.0]
}

/// The drift's phase in a noise texture's repeats (x, z), from the producer's unwrapped drift
/// (in its own repeats of `repeat` units) and the texture's repeat `noise_repeat`, reduced in
/// f64 so it stays bounded and exact however far the drift goes.
pub fn noise_phase(drift: [f64; 2], repeat: f32, noise_repeat: f32) -> Vec2 {
    let r = f64::from(noise_repeat.max(1.0));
    let f = |d: f64| if d.is_finite() { ((d * f64::from(repeat)).rem_euclid(r) / r) as f32 } else { 0.0 };
    let v = Vec2::new(f(drift[0]), f(drift[1]));
    // rem_euclid can round up to the repeat itself.
    Vec2::new(if v.x >= 1.0 { 0.0 } else { v.x }, if v.y >= 1.0 { 0.0 } else { v.y })
}

/// `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_SHAPE` / `_EROSION`: a strength 0..[`MAX_STRENGTH`]; unset or
/// unreadable 1 (the scale's "as designed"; a run starts from [`START_VALUES`] instead, [`start_strength`]).
#[cfg(test)] // the tests' reader; a run starts from the START values (stage 2i M3)
pub fn debug_strength(value: Option<&str>) -> f32 {
    start_strength(value, 1.0)
}

/// A strength's start value: the variable's 0..[`MAX_STRENGTH`], `start` when unset or unreadable.
pub fn start_strength(value: Option<&str>, start: f32) -> f32 {
    value.and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).map_or(start, |v| v.clamp(0.0, MAX_STRENGTH))
}

/// What a run starts with when no start variable sets the key (#3032), beside the light's
/// volumetric_clouds_light.rs `START_LIGHT` and volumetric_clouds_wisp.rs `START_WISP`. The keys' scales are
/// unchanged (1 is still "as designed"); a start variable overrides, and for the live keys (SHAPE, EROSION,
/// DENSITY) a tune line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StartValues {
    pub shape: f32,
    pub erosion: f32,
    pub density: f32,
    /// `HEIGHT`: the domain's height, units.
    pub height: f32,
    pub puff: f32,
    /// `LIGHT_TAPS`: the light volume's near taps asked for (volumetric_clouds_lightvol.rs
    /// `lv_near_taps` is the count marched).
    pub taps: u32,
    /// `REFINED_STRIDE`: 1, 2 or 4.
    pub refined_stride: u32,
    pub light_reuse: bool,
    pub air: f32,
}

/// The start values: the look judged by eye on 2026-11-23.
pub const START_VALUES: StartValues = StartValues {
    shape: 1.0,
    erosion: 1.26,
    density: 0.76,
    height: 24_000.0,
    puff: 1.0,
    taps: 2,
    refined_stride: 4,
    light_reuse: true,
    air: 0.82,
};

/// The mean opacity straight up aimed at for uniform cover `cover` ([`OPACITY`]).
#[cfg_attr(not(test), allow(dead_code))]
pub fn target_opacity(cover: f32) -> f32 {
    1.0 - (1.0 - COVER_HOLD * unit(cover)).powf(OPACITY)
}

/// [`EXTINCTION_TABLE`] at `cover`: linear between [`CAL_COVERS`], held outside (march
/// `vc_table`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn table_depth(cover: f32) -> f32 {
    table_depth_of(&EXTINCTION_TABLE, cover)
}

/// A table of the [`CAL_COVERS`] at `cover`: linear between them, held outside (march `vc_table`).
pub fn table_depth_of(table: &[f32; 8], cover: f32) -> f32 {
    let c = unit(cover);
    if c <= CAL_COVERS[0] {
        return table[0];
    }
    for (cs, ks) in CAL_COVERS.windows(2).zip(table.windows(2)) {
        if c <= cs[1] {
            return ks[0] + (ks[1] - ks[0]) * (c - cs[0]) / (cs[1] - cs[0]);
        }
    }
    table[CAL_COVERS.len() - 1]
}

/// The shader def of the trace's view variant (volumetric_clouds_march.wesl: `vc_view_code` reads the
/// view asked for and `vc_count_turn` counts; without it both are nothing).
pub const VIEWS_DEF: &str = "CLOUD_VIEWS";

/// Whether the trace draws its view variant with this uniform: a debug view is asked for.
pub fn view_variant(u: &CloudVolumeUniform) -> bool {
    crate::volumetric_clouds_light::view_code(u) != 0
}

/// What a pipeline's label ends with: nothing for the one a run draws with, `_views` for the trace's view
/// variant.
pub fn variant_suffix(views: bool) -> &'static str {
    if views { "_views" } else { "" }
}

/// The least `SHAPE` a run draws with: 0 and every value under this draw as this (the start variable and
/// the tune file take them, the uniform holds them), so there is no plain envelope; the body with no relief
/// is not a picture of its own.
pub const SHAPE_LEAST: f32 = 0.01;

/// A run's inputs as the shader draws them: `SHAPE` at least [`SHAPE_LEAST`].
fn drawn_inputs(mut i: VolumeInputs) -> VolumeInputs {
    i.shape_aid = i.shape_aid.map(|v| v.max(SHAPE_LEAST));
    i
}

/// The vigour's scale for a spread vigour `v` (0..1):
/// [`VIGOUR_LOW`] rising to [`VIGOUR_HIGH`] as `v^VIGOUR_POWER`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn vigour_scale(v: f32) -> f32 {
    VIGOUR_LOW + (VIGOUR_HIGH - VIGOUR_LOW) * unit(v).max(1e-6).powf(VIGOUR_POWER)
}

/// The share of sky with cloud for the weather's amount (the mean of its composed cover)
/// ([`SHARE_POWER`]): 0 at 0, 1 at 1, rising; not a number is 0.
pub fn sky_share(amount: f32) -> f32 {
    1.0 - (1.0 - unit(amount)).powf(SHARE_POWER)
}

/// The threshold the placement field must pass for the weather's amount: the field's measured
/// distribution ([`PLACE_THRESHOLDS`]) at [`sky_share`], linear between its shares; 1 (above any
/// value the field takes but its very top) at amount 0, 0 (a closed deck) at 1.
pub fn place_threshold(amount: f32) -> f32 {
    threshold_for_share(sky_share(amount))
}

fn threshold_for_share(s: f32) -> f32 {
    for (ss, ts) in PLACE_SHARES.windows(2).zip(PLACE_THRESHOLDS.windows(2)) {
        if s <= ss[1] {
            return ts[0] + (ts[1] - ts[0]) * (s - ss[0]) / (ss[1] - ss[0]);
        }
    }
    0.0
}

/// The placement field from its three octaves' lattice values `noises` and how much of each a
/// footprint keeps (`keeps`, [`octave_keep`]) (march `vc_place_weights`): the kept octaves about their
/// mean 0.5, their spread restored by the ratio of the full weights' length to the kept ones'
/// (so the share of sky a threshold passes stays as octaves fade); 0.5 when none is kept.
#[cfg_attr(not(test), allow(dead_code))]
pub fn place_from(noises: [f32; 3], keeps: [f32; 3]) -> f32 {
    let mut sum = 0.0;
    let (mut full, mut kept) = (0.0f32, 0.0f32);
    for k in 0..3 {
        let keep = unit(keeps[k]);
        sum += PLACE_WEIGHTS[k] * keep * (noises[k] - 0.5);
        full += PLACE_WEIGHTS[k] * PLACE_WEIGHTS[k];
        kept += (PLACE_WEIGHTS[k] * keep) * (PLACE_WEIGHTS[k] * keep);
    }
    if kept.sqrt() <= 1e-4 {
        return 0.5;
    }
    0.5 + sum * full.sqrt() / kept.sqrt()
}

/// The thin cover of a footprint that cannot resolve the masses (march `vc_shape`)
/// at a point `above` units over the place's base: the requested sky share `share` ([`sky_share`])
/// times the local types' profile `w` over one sheet's height ([`SHEET`]), never lifted or carved;
/// 0 above the sheet. At that footprint the field is its mean,
/// 0.5, everywhere (an empty sky under a thin weather's threshold, a full one under a heavy
/// weather's), so this is what such a footprint is expected to hold.
#[cfg_attr(not(test), allow(dead_code))]
pub fn unresolved_sheet(share: f32, above: f32, w: Vec3) -> f32 {
    if !(above > 0.0 && above < SHEET) {
        return 0.0;
    }
    unit(share) * profile_at(above / SHEET, w)
}

/// The placement field's first-octave cells per texture repeat: a cell of
/// [`PLACE_CELL_PER_DOMAIN`] domain heights, for a repeat of `repeat` units.
pub fn place_cells(domain: f32, repeat: f32) -> f32 {
    let r = if repeat.is_finite() && repeat > 1.0 { repeat } else { crate::cloud_shadows::REPEAT_UNITS };
    r / (PLACE_CELL_PER_DOMAIN * domain_height(domain))
}

/// The vigour's first-octave cells per texture repeat: a cell of [`VIGOUR_CELL_PER_DOMAIN`]
/// domain heights, for a repeat of `repeat` units.
pub fn vigour_cells(domain: f32, repeat: f32) -> f32 {
    let r = if repeat.is_finite() && repeat > 1.0 { repeat } else { crate::cloud_shadows::REPEAT_UNITS };
    r / (VIGOUR_CELL_PER_DOMAIN * domain_height(domain))
}

/// The near step's length for the quality value `steps` ([`VIEW_STEPS`] is [`STEP_NEAR`]).
pub fn step_near(steps: u32) -> f32 {
    STEP_NEAR * VIEW_STEPS as f32 / steps.clamp(MIN_STEPS, MAX_STEPS) as f32
}

/// The shell's radius: the base, `above` units over the ground reference, meets a horizontal
/// ray from that reference at `horizon` (`R = horizon² / (2 above)`; `above` held to 1,000 or
/// more, `horizon` to [`MIN_HORIZON`]..[`MAX_HORIZON`]).
pub fn shell_radius(horizon: f32, above: f32) -> f32 {
    let h = if horizon.is_finite() { horizon.clamp(MIN_HORIZON, MAX_HORIZON) } else { HORIZON };
    let a = if above.is_finite() { above.max(1_000.0) } else { crate::cloud_shadows::PLANE_ABOVE_GROUND };
    h * h / (2.0 * a)
}

/// The source's range along a ray: where a horizontal ray from the ground reference leaves the
/// bent domain's top (`sqrt(2 R (above + domain))`), times [`RANGE_MARGIN`].
pub fn source_range(horizon: f32, above: f32, domain: f32) -> f32 {
    let r = shell_radius(horizon, above);
    let a = if above.is_finite() { above.max(1_000.0) } else { crate::cloud_shadows::PLANE_ABOVE_GROUND };
    (2.0 * r * (a + domain_height(domain))).sqrt() * RANGE_MARGIN
}

/// A domain height held to [`MIN_DOMAIN`]..[`MAX_DOMAIN`] (not a number: [`REFERENCE_HEIGHT`]).
pub fn domain_height(domain: f32) -> f32 {
    if domain.is_finite() { domain.clamp(MIN_DOMAIN, MAX_DOMAIN) } else { REFERENCE_HEIGHT }
}

/// The real roots of `a t² + b t + c = 0` (`a >= 0`), cancellation-resistant (march
/// `vc_roots`): `None` when there are none, else (smaller, larger); a vanishing `a` gives the
/// linear equation's one root twice.
#[cfg_attr(not(test), allow(dead_code))]
pub fn roots(a: f32, b: f32, c: f32) -> Option<(f32, f32)> {
    if a <= 1e-12 {
        if b.abs() <= 1e-12 {
            return None;
        }
        let r = -c / b;
        return Some((r, r));
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let q = -0.5 * (b + if b >= 0.0 { 1.0 } else { -1.0 } * disc.sqrt());
    let r1 = q / a;
    let r2 = if q.abs() > 1e-30 { c / q } else { r1 };
    Some((r1.min(r2), r1.max(r2)))
}

/// The bent height of a point `rel` from a camera at height `cam_y`, for a shell of radius `r`
/// (march `vc_bent_y`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn bent_y(rel: Vec3, cam_y: f32, r: f32) -> f32 {
    cam_y + rel.y + (rel.x * rel.x + rel.z * rel.z) / (2.0 * r.max(1.0))
}

/// The stretches of a ray from `origin` along the unit
/// `dir` (its own stretch `t_min..t_max`) inside the bent domain of `u`, within the range; each
/// a miss when its end is not past its start (a ray from inside or above the
/// domain that dips under the base has two; one that clears it is split at its
/// lowest bent height, so the far part's fade is continuous across the tangency).
#[cfg_attr(not(test), allow(dead_code))]
pub fn spans(origin: Vec3, dir: Vec3, t_min: f32, t_max: f32, u: &CloudVolumeUniform) -> [(f32, f32); 2] {
    spans_to(origin, dir, (t_min, t_max), u.slab.x + u.slab.y.max(1.0), u)
}

/// The refined march's stretches (march `vc_refined_spans`): [`spans`] ended at the sampling ceiling for a
/// camera under it, the domain's own from above it.
pub fn lean_spans(origin: Vec3, dir: Vec3, t_min: f32, t_max: f32, u: &CloudVolumeUniform) -> [(f32, f32); 2] {
    let ceiling = crate::volumetric_clouds_model2::sampling_ceiling(u);
    spans_to(origin, dir, (t_min, t_max), if origin.y < ceiling { ceiling } else { u.slab.x + u.slab.y.max(1.0) }, u)
}

/// CPU mirror of the march's `vc_spans_to`: [`spans`] with the domain's bent height ending at `top`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn spans_to(origin: Vec3, dir: Vec3, (t_min, t_max): (f32, f32), top: f32, u: &CloudVolumeUniform) -> [(f32, f32); 2] {
    let (base, y0) = (u.slab.x, origin.y);
    let a = (dir.x * dir.x + dir.z * dir.z) / (2.0 * u.reach.y.max(1.0));
    let b = dir.y;
    let miss = (1.0, 0.0);
    let s = if a <= 1e-12 {
        if b.abs() < 1e-6 {
            if y0 > base && y0 < top { [(0.0, u.slab.z), miss] } else { [miss, miss] }
        } else {
            let (ta, tb) = ((base - y0) / b, (top - y0) / b);
            [(ta.min(tb).max(0.0), ta.max(tb)), miss]
        }
    } else {
        let Some((top_lo, top_hi)) = roots(a, b, y0 - top) else { return [miss, miss] };
        let lo = top_lo.max(0.0);
        match roots(a, b, y0 - base) {
            Some((b_lo, b_hi)) if b_hi > lo && b_lo < top_hi => [(lo, top_hi.min(b_lo)), (b_hi.max(lo), top_hi)],
            // A ray that clears the base split at its lowest bent height.
            _ => {
                let mid = (-b / (2.0 * a)).max(lo).min(top_hi);
                [(lo, mid), (mid, top_hi)]
            }
        }
    };
    let far = t_max.min(u.slab.z);
    s.map(|(t0, t1)| (t0.max(t_min), t1.min(far)))
}

/// The first stretch [`spans`] holds (the nearest the ray meets), or a miss.
#[cfg_attr(not(test), allow(dead_code))]
pub fn span(origin: Vec3, dir: Vec3, t_min: f32, t_max: f32, u: &CloudVolumeUniform) -> (f32, f32) {
    let [first, second] = spans(origin, dir, t_min, t_max, u);
    if first.1 > first.0 { first } else { second }
}

/// The light march's reach from a sample `rel` from the camera (camera height `cam_y`) along
/// the unit light direction `ld` (march `vc_light_first`): to the bent domain's top, at most
/// [`LIGHT_REACH`] (the reach of `u`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn light_reach(rel: Vec3, cam_y: f32, ld: Vec3, u: &CloudVolumeUniform) -> f32 {
    let twice_r = 2.0 * u.reach.y.max(1.0);
    let a = (ld.x * ld.x + ld.z * ld.z) / twice_r;
    let b = ld.y + 2.0 * (rel.x * ld.x + rel.z * ld.z) / twice_r;
    let c = bent_y(rel, cam_y, u.reach.y) - (u.slab.x + u.slab.y.max(1.0));
    match roots(a, b, c) {
        Some((_, hi)) if hi > 0.0 => hi.min(u.reach.x),
        _ => u.reach.x,
    }
}

/// The light march's intervals over `reach` in `n` taps (march `vc_light_first`): the first, each
/// next [`LIGHT_GROWTH`] times the one before, summing to the reach.
#[cfg_attr(not(test), allow(dead_code))]
pub fn light_intervals(reach: f32, n: u32) -> Vec<f32> {
    let n = n.clamp(1, 16);
    let first = reach * (LIGHT_GROWTH - 1.0) / (LIGHT_GROWTH.powi(n as i32) - 1.0);
    (0..n).map(|j| first * LIGHT_GROWTH.powi(j as i32)).collect()
}

/// The view steps a stretch `(t0, t1)` of a ray of `spread` asks for at the spacing bound (march
/// `vc_stretch_steps`): its length over the step, [`STEP_NEAR`] (as the quality value scales it),
/// up to [`STEP_FOOTPRINT`] footprints at the entry, never longer than [`step_bound`] there, also
/// where that is shorter than the near step (the detail bound
/// before the budget's scaling; near the camera the crown's 125 units hold while budget is left);
/// at least 1, at most ten million (a float count).
#[cfg_attr(not(test), allow(dead_code))]
pub fn stretch_steps(t0: f32, t1: f32, spread: f32, u: &CloudVolumeUniform) -> f32 {
    let len = (t1 - t0).max(0.0);
    let step = u.march.x.max(u.flags.w * spread.max(0.0) * t0).min(step_bound(spread.max(0.0) * t0, u)).max(1.0);
    (len / step).ceil().clamp(1.0, 1.0e7)
}

/// The most view steps one ray of the dense reference takes for the light taps of `u` ([`RAY_BUDGET`] in
/// `reach.w`): the budget over one plus the taps, less two (each of the two stretches takes at
/// least one step), at least 1.
#[cfg_attr(not(test), allow(dead_code))]
pub fn ray_steps_most(u: &CloudVolumeUniform) -> f32 {
    let taps = (u.march.y as u32).clamp(1, 16) as f32;
    ((u.reach.w / (1.0 + taps)).floor() - 2.0).max(1.0)
}

/// A stretch's steps once its ray's share is scaled to the budget: `floor(n scale)`, at least 1.
fn scaled_steps(n: f32, scale: f32) -> u32 {
    (n * scale).floor().max(1.0) as u32
}

/// The dense reference's steps for the two stretches of a ray of `spread` (a CPU march of the tests, what
/// the refined march is held against): what the spacing bound asks of each ([`stretch_steps`]; none for a stretch that
/// is empty or faded out at its entry), scaled down together when they pass [`ray_steps_most`]
/// (the steps then wider than the bound), so the ray's evaluations, views and light taps, stay
/// within [`RAY_BUDGET`]; and whether the ray was capped.
#[cfg_attr(not(test), allow(dead_code))]
pub fn ray_plan(stretches: [(f32, f32); 2], spread: f32, u: &CloudVolumeUniform) -> ([u32; 2], bool) {
    let want = stretches.map(|(t0, t1)| if t1 > t0 && horizon_fade(t0, u.slab.w) > 0.0 { stretch_steps(t0, t1, spread, u) } else { 0.0 });
    let most = ray_steps_most(u);
    let total = want[0] + want[1];
    let scale = (most / total.max(1.0)).min(1.0);
    (want.map(|n| if n > 0.0 { scaled_steps(n, scale) } else { 0 }), total > most)
}

/// The march's steps for one stretch `(t0, t1)` alone (a ray with no other, [`ray_plan`]'s
/// arithmetic, whatever its fade): (how many, how long), uniform.
#[cfg_attr(not(test), allow(dead_code))]
pub fn step_plan(t0: f32, t1: f32, spread: f32, u: &CloudVolumeUniform) -> (u32, f32) {
    let len = (t1 - t0).max(0.0);
    let want = stretch_steps(t0, t1, spread, u);
    let n = scaled_steps(want, (ray_steps_most(u) / want.max(1.0)).min(1.0));
    (n, len / n as f32)
}

/// The elevations the capped probes sample (degrees): every [`CAP_PROBE_DEG`] from straight down
/// to straight up (the bent domain is the same at every azimuth around the camera).
pub const CAP_PROBE_DEG: f32 = 0.1;

/// Whether any ray sampled every [`CAP_PROBE_DEG`] of elevation (its direction given) is `capped`.
fn any_ray_capped(capped: impl Fn(Vec3) -> bool) -> bool {
    let steps = (180.0 / CAP_PROBE_DEG).round() as i32;
    (0..=steps).any(|k| {
        let (s, c) = (-90.0 + k as f32 * CAP_PROBE_DEG).to_radians().sin_cos();
        capped(Vec3::new(c, s, 0.0))
    })
}

/// Whether a camera at `origin` (world) casting rays of `spread` has a ray the budget in `u` caps
/// ([`ray_plan`]): a real cap found on rays sampled every [`CAP_PROBE_DEG`] of elevation
/// (a ray 1.8 degrees down fell between 15 probes); a ray
/// between two samples can still be capped unreported.
#[cfg(test)]
pub fn rays_capped(origin: Vec3, spread: f32, u: &CloudVolumeUniform) -> bool {
    any_ray_capped(|d| ray_plan(spans(origin, d, 0.0, crate::medium::RAY_END, u), spread, u).1)
}

/// The refined march's coarse spacing at full resolution for the weather's amount (the uniform's
/// `profile.z`): [`COARSE_THIN`] up to [`COARSE_THIN_UP_TO`], [`COARSE_FAIR`] above, times the
/// domain's height over [`REFERENCE_HEIGHT`] (not a number: the thin spacing). The uniform carries it
/// times the level's factor ([`probe_factor`]).
pub fn coarse_spacing(amount: f32, domain: f32) -> f32 {
    let a = if amount.is_finite() { amount } else { 0.0 };
    (if a <= COARSE_THIN_UP_TO { COARSE_THIN } else { COARSE_FAIR }) * domain_height(domain) / REFERENCE_HEIGHT
}

/// The factor on [`coarse_spacing`] in force for an input's level (`None`: 1, as designed), held to 1..4.
fn probe_factor(probe: Option<f32>) -> f32 {
    probe.filter(|v| v.is_finite()).map_or(1.0, |v| v.clamp(1.0, 4.0))
}

/// The refined march's fine step at `t` along a ray of `spread` (march `vc_fine_step`): the dense
/// march's spacing rule ([`stretch_steps`]) evaluated at `t` instead of the stretch's entry; it never
/// falls as `t` grows.
#[cfg_attr(not(test), allow(dead_code))]
pub fn fine_step(t: f32, spread: f32, u: &CloudVolumeUniform) -> f32 {
    let w = spread.max(0.0) * t;
    u.march.x.max(u.flags.w * w).min(step_bound(w, u)).max(1.0)
}

/// The refined march's coarse spacing at `t` along the unit `dir` of `spread` (march
/// `vc_coarse_step`): the weather's ([`coarse_spacing`], `profile.z`) widened by the placement's
/// footprint on the shell ([`place_width`]) over the finest placement octave's cell once that octave
/// is gone.
#[cfg_attr(not(test), allow(dead_code))]
pub fn coarse_step(dir: Vec3, t: f32, spread: f32, u: &CloudVolumeUniform) -> f32 {
    let rel = dir * t;
    let pw = cloud_place_width(rel, dir, spread.max(0.0) * t, u);
    let finest = 1.0 / (u.map.x * u.field.x * PLACE_OCTAVES[1]).max(1e-9);
    u.profile.z.max(1.0) * (1.0 - u.nerode.w) * (pw / finest).max(1.0)
}

/// Whether the stretch `b..e` of a ray from `origin` along the unit `dir` meets the band where every
/// cloud has density (march `vc_guard`): from the domain's base to its highest base plus a sheet,
/// by the ray's bent height (convex: its least at an end or at its lowest point).
#[cfg_attr(not(test), allow(dead_code))]
pub fn guard(origin: Vec3, dir: Vec3, b: f32, e: f32, u: &CloudVolumeUniform) -> bool {
    let a = (dir.x * dir.x + dir.z * dir.z) / (2.0 * u.reach.y.max(1.0));
    let y = |t: f32| origin.y + dir.y * t + a * t * t;
    let mut lo = y(b).min(y(e));
    if a > 1e-12 {
        lo = lo.min(y((-dir.y / (2.0 * a)).max(b).min(e)));
    }
    lo < guard_top(u) && y(b).max(y(e)) > u.slab.x
}

/// The top of the band [`guard`] protects (march `vc_guard`): [`BASE_RISE`] and a sheet over the base.
#[cfg_attr(not(test), allow(dead_code))]
pub fn guard_top(u: &CloudVolumeUniform) -> f32 {
    u.slab.x + u.height.w + u.field.w
}

/// The most fine cells one ray of the refined march spends for the light taps of `u` (march
/// `vc_trace_refined`): `(budget - 2) / (2 + taps) - 4`, at least 1: each cell costs at most one
/// probe, one sample and its taps, and each stretch one more probe; the plan keeps the cells of both
/// stretches within this plus two (one partial cell each), so a ray spends at most [`RAY_BUDGET`]
/// evaluations, with a margin of two cells for rounding (2,504 cells with 8 taps).
pub fn refined_cells_most(u: &CloudVolumeUniform) -> f32 {
    let taps = (u.march.y as u32).clamp(1, 16) as f32;
    (((u.reach.w - 2.0) / (2.0 + taps)).floor() - 4.0).max(1.0)
}

/// The refined march's plan for a ray's two stretches (march `vc_trace_refined`): how much every
/// fine cell grows (at least 1) so that the cells stay within [`refined_cells_most`] (the dense
/// march's own count at the entries' steps, [`stretch_steps`], bounds them: the fine step never
/// falls along a stretch), and whether the ray is capped.
pub fn refined_plan(stretches: [(f32, f32); 2], spread: f32, u: &CloudVolumeUniform) -> (f32, bool) {
    let want: f32 = stretches.iter().map(|&(t0, t1)| if t1 > t0 && horizon_fade(t0, u.slab.w) > 0.0 { stretch_steps(t0, t1, spread, u) } else { 0.0 }).sum();
    let most = refined_cells_most(u);
    ((want / most).max(1.0), want > most)
}

/// Whether a camera at `origin` (world) casting rays of `spread` has a ray the march's plan
/// ([`refined_plan`], on its own stretches, [`lean_spans`]) caps: a real cap found on rays sampled every
/// [`CAP_PROBE_DEG`] of elevation; a ray between two samples can still be capped unreported. The medium
/// looks once per level and settled weather and logs it once per activation of the row.
pub fn refined_rays_capped(origin: Vec3, spread: f32, u: &CloudVolumeUniform) -> bool {
    any_ray_capped(|d| refined_plan(lean_spans(origin, d, 0.0, crate::medium::RAY_END, u), spread, u).1)
}

/// What a CPU mirror of a march did along one ray ([`refined_march`]; the dense march's mirror in
/// the tests): the transmittance, the weights of the distance, the work, the plan's growth, and every
/// integrated cell, sample and probe in order.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MarchRun {
    pub trans: f32,
    pub hidden: f32,
    pub hidden_t: f32,
    pub probes: u32,
    pub fine: u32,
    pub taps: u32,
    /// The fine samples of blocks integrated by the base band's guard alone.
    pub guarded: u32,
    pub grow: f32,
    pub cells: Vec<(f32, f32)>,
    pub samples: Vec<f32>,
    pub probed: Vec<f32>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl MarchRun {
    /// One minus the transmittance.
    pub fn opacity(&self) -> f32 {
        1.0 - self.trans
    }

    /// The weighted distance (medium.rs `EMPTY_DISTANCE` when nothing was hidden).
    pub fn distance(&self) -> f32 {
        if self.hidden > 1e-6 { self.hidden_t / self.hidden } else { crate::medium::EMPTY_DISTANCE }
    }

    /// Every evaluation: probes, fine samples, light taps.
    pub fn evals(&self) -> u32 {
        self.probes + self.fine + self.taps
    }
}

/// A medium along one ray for the marches' mirrors, by the distance `t`: whether the support holds
/// there (march `vc_m2_support`) and the density (the optical density per unit, `d.x d.y`, and the shape
/// the light's gate reads, `d.x`).
#[cfg_attr(not(test), allow(dead_code))]
pub struct RayField<'a> {
    pub support: &'a dyn Fn(f32) -> bool,
    pub density: &'a dyn Fn(f32) -> (f32, f32),
    /// The density at `t` for a step `dt` long, where the medium's filter follows the march's actual step
    /// (march `vc_detail_filter`'s `dt`, the release stride's merged cells included; #3032, the tests' oracle):
    /// read instead of `density` by both marches' mirrors where given.
    pub stepped: Option<&'a dyn Fn(f32, f32) -> (f32, f32)>,
}

impl RayField<'_> {
    /// The density at `t` for a step `dt` (`stepped` where given, else `density`).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn at(&self, t: f32, dt: f32) -> (f32, f32) {
        match self.stepped {
            Some(f) => f(t, dt),
            None => (self.density)(t),
        }
    }
}

/// One probe of the refined march's mirror, counted.
#[cfg_attr(not(test), allow(dead_code))]
fn probe_at(run: &mut MarchRun, f: &RayField, t: f32) -> bool {
    run.probes += 1;
    run.probed.push(t);
    (f.support)(t)
}

/// Number of original fine cells represented by one interior quadrature sample. Leave one
/// original cell at the far edge. The caller also retains the entry and two measured densities.
fn interior_stride(limit: u32, remaining: u32, fine: f32, t: f32, sigma: f32) -> u32 {
    let max_length = 420.0 + 780.0 * smoothstep(15_000.0, 85_000.0, t);
    let length = max_length.min(0.65 / sigma.max(1e-12));
    limit.min(remaining.saturating_sub(1)).min((length / fine).floor() as u32).max(1)
}

/// CPU mirror of the refined view march (march `vc_trace_refined`, `vc_march_refined`; #3032 stage
/// 2c, checkpoint C1) of a ray from `origin` along the unit `dir` of `spread`, its own stretch
/// `t_min..t_max`, the start offset `offset`, through the medium `f`: the same blocks, probes,
/// halvings, cells, samples, arithmetic, stops and composition as the shader (the colour aside).
#[cfg_attr(not(test), allow(dead_code))]
pub fn refined_march(origin: Vec3, dir: Vec3, spread: f32, (t_min, t_max): (f32, f32), offset: f32, u: &CloudVolumeUniform, f: &RayField) -> MarchRun {
    let stretches = spans(origin, dir, t_min, t_max, u);
    let (grow, _) = refined_plan(stretches, spread, u);
    let blocks = refined_cells_most(u) as usize + 4;
    let taps = (u.march.y as u32).clamp(1, 16);
    let most = REFINE_BLOCK_MOST as f32;
    let mut run = MarchRun { trans: 1.0, grow, ..Default::default() };
    for (t0, t1) in stretches {
        if t1 <= t0 || run.trans < u.reach.z {
            continue;
        }
        let fade = horizon_fade(t0, u.slab.w);
        if fade <= 0.0 {
            continue;
        }
        let ahead = run.trans;
        let (mut mt, mut mh, mut mht) = (1.0f32, 0.0f32, 0.0f32);
        let mut occ_s = probe_at(&mut run, f, t0);
        let mut b = t0;
        let mut stable = false;
        let mut previous_sigma = 0.0_f32;
        let mut done = false;
        for _ in 0..blocks {
            if done || b >= t1 {
                break;
            }
            let fs = fine_step(b, spread, u) * grow;
            let cells = ((coarse_step(dir, b, spread, u) / fs).floor().clamp(1.0, most) as u32).clamp(1, REFINE_BLOCK_MOST);
            let (mut e, mut n) = (b + cells as f32 * fs, cells);
            if e >= t1 {
                e = t1;
                n = (((t1 - b) / fs).ceil().clamp(1.0, most) as u32).clamp(1, cells);
            }
            let occ_e = probe_at(&mut run, f, e);
            let node = |i: u32| if i >= n { e } else { b + i as f32 * fs };
            let both = occ_s && occ_e;
            let guarded = !both && guard(origin, dir, b, e, u);
            let (from, to) = if both || guarded {
                (0, n)
            } else if occ_e || occ_s {
                // The entry (lo empty, hi inside) or the exit (lo inside, hi empty): the half on
                // the side that differs from the far end is kept.
                let (mut lo, mut hi) = (0u32, n);
                for _ in 0..4 {
                    if hi - lo <= 1 {
                        break;
                    }
                    let mid = (lo + hi) / 2;
                    if probe_at(&mut run, f, node(mid)) == occ_e {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                if occ_e { (lo, n) } else { (0, lo + 1) }
            } else {
                (0, 0)
            };
            let allowed = both && u.families.x >= 2.0 && (u.flags.z == 0.0 || u.flags.z == 3.0);
            let mut i = from;
            if !allowed || u.families.x < 4.0 {
                stable = false;
                previous_sigma = 0.0;
            }
            while i < to {
                let c0 = node(i);
                // Allow pairs in an established distant or optically shielded mass even when
                // local variation prevents a four-cell step. Both ends of this block are occupied.
                let deep = allowed && u.families.x >= 4.0 && previous_sigma > 0.0
                    && (c0 >= 55_000.0 || mt * ahead < 0.40);
                let stride = if deep {
                    let limit: u32 = if stable { 4 } else { 2 };
                    let length = (1000.0 + 1000.0 * smoothstep(15_000.0, 85_000.0, c0)).min(0.85 / previous_sigma);
                    limit.min(to - i).min((length / fs).floor() as u32).max(1)
                } else if allowed && stable {
                    interior_stride(u.families.x as u32, to - i, fs, c0, previous_sigma)
                } else { 1 };
                i += stride;
                let dt = node(i) - c0;
                let t = c0 + offset * dt;
                run.fine += 1;
                run.guarded += u32::from(guarded);
                run.cells.push((c0, c0 + dt));
                run.samples.push(t);
                let (sigma, shape) = f.at(t, dt);
                if allowed {
                    let interior = shape > (2.0 * u.march.w).max(0.12) && sigma > 0.0;
                    stable = interior && previous_sigma > 0.0 && (sigma - previous_sigma).abs() <= 0.22 * sigma.max(previous_sigma);
                    previous_sigma = if interior { sigma } else { 0.0 };
                }
                if shape <= 0.0 {
                    continue;
                }
                if shape > u.march.w && u.light_dir.w > 0.0 {
                    run.taps += taps;
                }
                let kept = (-sigma * dt).exp();
                let taken = mt * (1.0 - kept);
                mh += taken;
                mht += taken * t;
                mt *= kept;
                if mt * ahead < u.reach.z {
                    done = true;
                    break;
                }
            }
            occ_s = occ_e;
            b = e;
        }
        run.hidden += ahead * fade * mh;
        run.hidden_t += ahead * fade * mht;
        run.trans *= 1.0 - (1.0 - mt) * fade;
    }
    run
}

/// The placement's footprint at `rel` from the camera on a ray along the unit `dir` whose
/// transverse footprint there is `width`, for a shell of radius `r` (march `vc_place_width`):
/// the distance between neighbouring rays where they meet the bent shell in world
/// x and z, `width / |cos|` of the angle between the ray and the shell's normal (at most 1,000
/// times the width).
#[cfg_attr(not(test), allow(dead_code))]
pub fn place_width(rel: Vec3, dir: Vec3, width: f32, r: f32) -> f32 {
    let r = r.max(1.0);
    let n = Vec3::new(rel.x / r, 1.0, rel.z / r).normalize();
    width / dir.dot(n).abs().max(1e-3)
}

/// Isotropic footprint for filtering placement (march `vc_place_width`). Fair banks use the
/// projected area. An isolated mixed bank resolves the reconstructed texel across that area.
/// As coverage becomes a continuous deck the horizontal base dominates again.
fn cloud_place_width(rel: Vec3, dir: Vec3, width: f32, u: &CloudVolumeUniform) -> f32 {
    let projected = place_width(rel, dir, width, u.reach.y);
    if u.scale.w < 0.72 {
        let deck = smoothstep(0.55, 0.72, u.scale.w);
        let texel = 1.0 + (u.dshape.w - 1.0) * crate::volumetric_clouds_model2::mixed_sheet(u);
        let fair = (width * projected).sqrt() * texel;
        fair + (projected - fair) * deck
    } else {
        projected
    }
}

/// The longest step a stretch entering with a footprint `width` may take (march
/// `vc_stretch_steps`): half the finest retained feature (#3032 stage 2b): one base
/// noise texel at the level the footprint reads (its finest detail spans two), never more than
/// half a sheet, half the finest erosion band kept (volumetric_clouds_model2.rs `erosion_bound`) or half
/// the surface's narrowest transition (`edge_bound`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn step_bound(width: f32, u: &CloudVolumeUniform) -> f32 {
    let lod = (width * u.nlevel.x * u.nscale.x).max(1.0).log2().clamp(0.0, u.nlevel.y);
    (lod.exp2() / (u.nscale.x * u.nlevel.x).max(1e-9)).min(0.5 * u.field.w).min(crate::volumetric_clouds_model2::erosion_bound(width, u)).min(crate::volumetric_clouds_model2::edge_bound(width))
}

/// How much of an octave with cells `cell` units wide a footprint `width` keeps: all while the
/// cell spans two footprints or more, none from one footprint on (the erosion, march
/// `vc_place_octave`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn octave_keep(width: f32, cell: f32) -> f32 {
    1.0 - smoothstep(0.5 * cell, cell, width)
}

/// The vigour field's offset for the producer's unwrapped drift (in texture repeats, f64): in
/// the field's own cells (`cells` a repeat), xy the fraction ([`cell_offset`]; zw 0: the whole
/// cells go to the shader as integers, `CloudVolumeUniform::cells_v` and its neighbours).
pub fn vigour_offset(drift: [f64; 2], cells: f32) -> Vec4 {
    cell_offset(drift, cells).0.extend(0.0).extend(0.0)
}

/// The drift (texture repeats, f64) in a lattice's own cells (`cells` a repeat): the fraction,
/// 0..1, and the whole cells as the lattice's integer, wrapping two's complement
/// (the hash's cells wrap the same way, so the field has no seam and nothing clamps however far
/// the drift goes; it repeats only after 2^32 cells). Not a number: none.
pub fn cell_offset(drift: [f64; 2], cells: f32) -> (Vec2, IVec2) {
    let cells = if cells.is_finite() { f64::from(cells) } else { 0.0 };
    let split = |d: f64| -> (f32, i32) {
        let v = if d.is_finite() { d * cells } else { 0.0 };
        let whole = v.floor();
        let frac = ((v - whole) as f32).clamp(0.0, 1.0 - f32::EPSILON);
        (frac, whole.rem_euclid(4_294_967_296.0) as u64 as u32 as i32)
    };
    let ((fx, wx), (fz, wz)) = (split(drift[0]), split(drift[1]));
    (Vec2::new(fx, fz), IVec2::new(wx, wz))
}

/// The old split's form (fraction in xy, whole cells as floats in zw) for moderate drifts, kept
/// for the tests' mirrors.
#[cfg(test)]
pub fn vigour_offset_whole(drift: [f64; 2], cells: f32) -> Vec4 {
    let (f, w) = cell_offset(drift, cells);
    Vec4::new(f.x, f.y, w.x as f32, w.y as f32)
}


/// The march's fade toward the horizon by the distance `t` where the ray enters the domain:
/// 1 up to [`HORIZON_FADE_FROM`] of `horizon`, 0 at it (march `vc_horizon_fade`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn horizon_fade(t: f32, horizon: f32) -> f32 {
    if !t.is_finite() {
        return 0.0;
    }
    let r = horizon.max(1.0);
    1.0 - smoothstep(HORIZON_FADE_FROM * r, r, t)
}

/// Henyey-Greenstein's phase function times 4π (isotropic = 1) at the cosine `cos` between
/// the view ray and the direction to the light (march `vc_hg`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn hg(cos: f32, g: f32) -> f32 {
    let g2 = g * g;
    let d = (1.0 + g2 - 2.0 * g * cos).max(1e-4);
    (1.0 - g2) / (d * d.sqrt())
}

/// The two lobes mixed and capped at [`PHASE_CAP`].
#[cfg_attr(not(test), allow(dead_code))]
pub fn phase(cos: f32) -> f32 {
    let f = hg(cos, G_FORWARD);
    (f + (hg(cos, G_BACK) - f) * BACK_WEIGHT).min(PHASE_CAP)
}

/// The direct light: the unit direction toward it and its strength (0..1). By day the sun's
/// disc direction (`sun_dir`, Bevy axes), by night the moon's (opposite it, as sky.rs draws
/// the moon disc) when the climate has one; nothing at the horizon, full from
/// [`LIGHT_FADE_DEG`], so the change of direction at the horizon shows no step.
pub fn direct_light(sun_dir: Vec3, sun_elevation_deg: f32, has_moon: bool) -> (Vec3, f32) {
    let dir = if sun_dir.is_finite() { sun_dir.try_normalize().unwrap_or(Vec3::Y) } else { Vec3::Y };
    let e = if sun_elevation_deg.is_finite() { sun_elevation_deg } else { 0.0 };
    if e >= 0.0 {
        (dir, smoothstep(0.0, LIGHT_FADE_DEG, e))
    } else if has_moon {
        (-dir, smoothstep(0.0, LIGHT_FADE_DEG, -e))
    } else {
        (-dir, 0.0)
    }
}

/// The weighted mean of the layers' colours (linear RGB): each `(weight, colour)` a drawn
/// layer's set weight × its texture's mean alpha and its PNAM colour at the hour. No weight:
/// black (there is no cover either).
pub fn ambient_colour(parts: &[(f32, [f32; 3])]) -> [f32; 3] {
    let mut sum = [0.0f32; 3];
    let mut total = 0.0f32;
    for &(w, c) in parts {
        let w = non_negative(w);
        if w <= 0.0 {
            continue;
        }
        for (s, v) in sum.iter_mut().zip(c) {
            *s += w * non_negative(v);
        }
        total += w;
    }
    if total <= 1e-6 {
        return [0.0; 3];
    }
    sum.map(|s| s / total)
}

/// The layers' `(weight, colour)` now: each layer's PNAM colour at the hour (set 0 the
/// current weather, set 1 the one fading out) weighted by its visible share of the cover
/// (`CloudCoverOutput::layer_cover`: alpha × fade weight × what the layers in front of it on
/// the dome let through).
fn cloud_parts(sky: &SkyState, layer_cover: &[[f32; 4]; 2], layer_weathers: &[Option<u32>; 2]) -> Vec<(f32, [f32; 3])> {
    // Each set's weather by the ID it had when the cover was composed: a fade started or
    // turned round since renames today's sets, a weather gone since counts for nothing.
    let by_id = |id: Option<u32>| [Some(&sky.weather), sky.prev.as_ref()].into_iter().flatten().find(|w| Some(w.form_id) == id);
    let mut parts = Vec::with_capacity(8);
    for (set, id) in layer_weathers.iter().enumerate() {
        let Some(w) = by_id(*id) else { continue };
        for (layer, cover) in layer_cover[set].iter().enumerate() {
            parts.push((*cover, sky.cloud_color_for(w, layer)));
        }
    }
    parts
}

/// The volume's depth test: Equal, no depth written. With the dome's exact geometry it passes
/// only where the depth buffer holds the dome's own depth (see [`proxy_mesh`]).
pub fn draw_on_dome(depth: &mut bevy::render::render_resource::DepthStencilState) {
    depth.depth_compare = Some(bevy::render::render_resource::CompareFunction::Equal);
    depth.depth_write_enabled = Some(false);
}

/// The proxy's mesh from the sky dome's: the dome's positions to the bit, its triangles kept,
/// and two vertices no triangle uses that make the mesh's bounds symmetric
/// about the dome's centre (the camera), since the transparent sort measures from the
/// bounds' centre ([`SORT_BIAS`] then orders it among the sky's parts as before). `None`
/// for a mesh without positions or triangles.
pub fn proxy_mesh(dome: &Mesh) -> Option<Mesh> {
    use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
    let Some(VertexAttributeValues::Float32x3(pos)) = dome.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let indices: Vec<u32> = match dome.indices()? {
        Indices::U32(i) => i.clone(),
        Indices::U16(i) => i.iter().map(|&x| u32::from(x)).collect(),
    };
    if pos.is_empty() || indices.is_empty() {
        return None;
    }
    let mut positions: Vec<[f32; 3]> = pos.clone();
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in &positions {
        for (k, c) in v.iter().enumerate() {
            lo[k] = lo[k].min(*c);
            hi[k] = hi[k].max(*c);
        }
    }
    positions.push(hi.map(|c| -c));
    positions.push(lo.map(|c| -c));
    let n = positions.len();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n]);
    m.insert_indices(Indices::U32(indices));
    Some(m)
}

/// volumetric_clouds_march.wesl `CloudVolume`: the same fields in the same order.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType, Reflect)]
pub struct CloudVolumeUniform {
    /// x: the base (Bevy y, the shadows' plane), y: the domain's height ([`REFERENCE_HEIGHT`]),
    /// z: the source's range ([`source_range`]), w: the horizon distance ([`HORIZON`]).
    pub slab: Vec4,
    /// x: the near step's length ([`step_near`]), y: light taps, z: the debug density
    /// multiplier (1 as designed), w: [`LIGHT_THRESHOLD`].
    pub march: Vec4,
    /// x: [`LIGHT_REACH`], y: the shell's radius ([`shell_radius`]), z: [`STOP_TRANSMITTANCE`],
    /// w: [`RAY_BUDGET`] (density evaluations a ray).
    pub reach: Vec4,
    /// x: [`G_FORWARD`], y: [`G_BACK`], z: [`BACK_WEIGHT`], w: [`PHASE_CAP`].
    pub phase: Vec4,
    /// xyz: unit direction to the light, w: its strength 0..1.
    pub light_dir: Vec4,
    /// rgb: the direct light (linear sunlight × sky level × [`SUN_GAIN`]).
    pub light: Vec4,
    /// xy: uv scale (the cover's projection); zw unused.
    pub map: Vec4,
    /// x: [`BASE_ROUND`] (units), y: [`sky_share`] of the amount (an unresolved footprint's
    /// cover, [`unresolved_sheet`]), z: the refined march's coarse spacing ([`coarse_spacing`]); w unused.
    pub profile: Vec4,
    /// x: 1 when the view shows display values clipped (no Tony inverse), y: 1 to rotate the
    /// dither each frame (a temporal mode), z: the debug view ([`CloudView::code`]), w:
    /// [`STEP_FOOTPRINT`].
    pub flags: Vec4,
    /// z: [`NOISE_LO`], w: [`NOISE_HI`]; x, y unused.
    pub tone: Vec4,
    /// The vigour field's offsets ([`vigour_offset`] of `CloudCoverOutput::drift`): its first
    /// octave's and its second's.
    pub vigour: Vec4,
    pub vigour2: Vec4,
    /// x: [`vigour_cells`], y: [`VIGOUR_OCTAVE`], w: [`BASE_RISE`].
    pub height: Vec4,
    /// x: the placement's cells a repeat, y: its threshold (volumetric_clouds_model2.rs
    /// `placement_threshold`), z: the domain's height, w: [`SHEET`].
    pub field: Vec4,
    /// x: [`VIGOUR_LOW`], y: [`VIGOUR_HIGH`], z: [`VIGOUR_POWER`], w: the weather's amount.
    pub scale: Vec4,
    /// The placement field's offsets per octave ([`vigour_offset`] of the drift in each
    /// octave's cells).
    pub place0: Vec4,
    pub place1: Vec4,
    /// xy: placement drift.
    pub place2: Vec4,
    /// The offsets' whole cells ([`cell_offset`]) as the lattice's integer bits: the vigour's two
    /// octaves (x, z each), the placement's first two, its third.
    pub cells_v: UVec4,
    pub cells_p: UVec4,
    pub cells_q: UVec4,
    /// Alternate fair bank placement, one offset per octave. XY are the f32
    /// fraction bits, ZW the wrapping integer cells, both from the same f64 drift.
    pub cross0: UVec4,
    pub cross1: UVec4,
    pub cross2: UVec4,
    /// The weather's type mix ([`type_mix`]; sheet, cumulus, tower), w 1 when forced.
    pub mix: Vec4,
    /// [`TOWER_VIGOUR`], [`TOWER_DEPTH`].
    pub tower: Vec4,
    /// x the fair lobes' strength (`PUFF`), y [`TOWER_GAIN`]; z, w unused.
    pub kind: Vec4,
    /// Per type its knots ([`KNOTS_SHEET`], [`KNOTS_CUMULUS`], [`KNOTS_TOWER`]), four and four.
    pub knot_s0: Vec4,
    pub knot_s1: Vec4,
    pub knot_c0: Vec4,
    pub knot_c1: Vec4,
    pub knot_t0: Vec4,
    pub knot_t1: Vec4,
    /// [`CAL_COVERS`] (the first four, the last four) and [`EXTINCTION_TABLE`] (likewise).
    pub cal_c0: Vec4,
    pub cal_c1: Vec4,
    pub cal_k0: Vec4,
    pub cal_k1: Vec4,
    /// The drift's phase in the noise textures' repeats: xy the base's, zw the erosion's.
    pub nphase: Vec4,
    /// One over the base's and the erosion's repeats, the carve's and the erosion's strengths.
    pub nscale: Vec4,
    /// The base's side and last level, the erosion's side and last level.
    pub nlevel: Vec4,
    /// [`NOISE_LOBE`] (x, y).
    pub nlobe: Vec4,
    /// [`ERODE_MEANS`] (xyz).
    pub nband: Vec4,
    /// [`NOISE_ERODE`] (x, y).
    pub nerode: Vec4,
    // detail (stage 2h)
    /// z: the wisps' strength (volumetric_clouds_wisp.rs `wisp_block`), w the view's reciprocal trace
    /// tile side (1 for a view without tiles); x, y unused.
    pub dshape: Vec4,
    // light volume (stage 2g)
    /// The light volume's block (#3032 stage 2g): zero from [`uniform`], filled in the render
    /// world for the frame's camera (volumetric_clouds_lightvol.rs `lv_fill`).
    pub lv: Vec4,
    pub lv_near: Vec4,
    pub lv_far: Vec4,
    pub lv_size: Vec4,
    pub lv_cam: Vec4,
    // light (stage 2e)
    // volumetric_clouds_light.rs `lighting` packs these.
    /// y: the column depth per unit height share (`table_depth` x density x `MEAN_SHAPE`); z: the ambient's
    /// fall per optical depth (`AMBIENT_FALL`); w: the direct light's weight (the `SUN` aid); x unused.
    pub lmode: Vec4,
    /// rgb: the weighted light from above (linear, bounded); w unused.
    pub lupper: Vec4,
    /// rgb: the weighted ground light; w unused.
    pub llower: Vec4,
    /// x: the second octave's weight (the first's is 1 - x), y: its depth factor, z: its phase's
    /// eccentricity factor, w: the forward lobe's weight aid.
    pub loct: Vec4,
    /// x: the back-scatter floor's length (units) and y its most, both times the aid; z, w: the
    /// light's elevation sines over which a measured depth takes over the light from above.
    pub lfloor: Vec4,
    /// x: the powder darkening's share (the `POWDER` aid times `POWDER_SHARE`, at most 1); y, z, w unused.
    pub lpowder: Vec4,
    /// x: the light's filter cell, a light volume level's spacing (0 here; set in each level's snapshot and
    /// the trace's uniform, volumetric_clouds_lightvol.rs `lv_light_cell`); y, z, w unused.
    pub lcell: Vec4,
    /// Actual diffuse sky at the horizon and at 20 degrees, scaled as the sky dome is.
    /// The w components hold the air strength and the worldspace's ground reference.
    pub air_low: Vec4,
    pub air_high: Vec4,
    /// x: the largest longitudinal stride (REFINED_STRIDE), w: 1 to reuse a neighboring cell's light.
    pub families: Vec4,
}

/// A cloud type forced everywhere (`NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_TYPE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudType {
    Sheet,
    Cumulus,
    Tower,
}

impl CloudType {
    /// Its one-hot weights (sheet, cumulus, tower).
    pub fn weights(self) -> Vec3 {
        match self {
            CloudType::Sheet => Vec3::X,
            CloudType::Cumulus => Vec3::Y,
            CloudType::Tower => Vec3::Z,
        }
    }
}

/// `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_TYPE`: `sheet`, `cumulus` or `tower`; anything else none.
pub fn debug_type(value: Option<&str>) -> Option<CloudType> {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("sheet") => Some(CloudType::Sheet),
        Some("cumulus") => Some(CloudType::Cumulus),
        Some("tower") => Some(CloudType::Tower),
        _ => None,
    }
}

/// What the volume shows instead of the clouds (`NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_VIEW`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CloudView {
    #[default]
    None,
    /// The placement field against its threshold where the ray meets the bent base: dark grey
    /// below it (its value over the threshold, to 0.45), from mid grey to white above it (by how
    /// far inside its mass).
    Cover,
    /// The local occupied height's share of the domain where the ray meets the bent base, grey.
    Vigour,
    /// The evaluations of the ray as RGB: the probes, the density samples and the light taps, white at
    /// [`COUNT_PROBES`], [`COUNT_FINE`], [`COUNT_TAPS`] (read in the reference mode for figures: the
    /// default mode blends it).
    Count,
    /// The generated noise on the bent base (#3032 stage 2b): bands one base repeat wide
    /// alternate across world x, grey the base scalar, colour the erosion's three bands, at the
    /// level the shell's footprint picks (read in the reference mode: the default blends it).
    Noise,
    // Codes 5, 9, 11 and 12 are not used; the light's views (#3032 stage 2e) follow.
    /// The clouds lit by the direct light alone (the ambient zeroed), through the normal march.
    Sun,
    /// The clouds lit by the ambient alone (the direct light zeroed), through the normal march.
    Ambient,
    /// The optical depth toward the light the march measured: per ray the opacity-weighted mean
    /// of its samples' codes, opaque (volumetric_clouds_light.rs [`crate::volumetric_clouds_light::depth_code`]:
    /// grey on a log scale, magenta where the light was skipped).
    Depth,
    /// Integrated camera-ray opacity.
    Opacity,
    /// Visible height above the base of the local cloud body, normalized by its thickness.
    BodyHeight,
}

impl CloudView {
    /// The march's code (`flags.z`, march `vc_view_code`).
    pub fn code(self) -> f32 {
        match self {
            CloudView::None => 0.0,
            CloudView::Cover => 1.0,
            CloudView::Vigour => 2.0,
            CloudView::Count => 3.0,
            CloudView::Noise => 4.0,
            CloudView::Sun => 6.0,
            CloudView::Ambient => 7.0,
            CloudView::Depth => 8.0,
            CloudView::Opacity => 10.0,
            CloudView::BodyHeight => 13.0,
        }
    }
}

/// What the uniform is made from this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VolumeInputs {
    pub plane_height: f32,
    /// The plane's height over the ground reference (cloud_shadows.rs `plane_above_ground`).
    pub above_ground: f32,
    pub uv_scale: Vec2,
    /// The producer's unwrapped drift (`CloudCoverOutput::drift`).
    pub drift: [f64; 2],
    /// The weather's amount: the mean of its composed cover (`CloudCoverOutput::mean_cover`).
    pub amount: f32,
    /// `PUFF`: the weight of the connected fair-weather lobes.
    pub puff: f32,
    /// The classification bias of the sky ([`sky_bias`]) and a type forced everywhere.
    pub bias: Vec3,
    pub forced: Option<CloudType>,
    pub light_dir: Vec3,
    pub light_strength: f32,
    /// Linear RGB.
    pub sunlight: [f32; 3],
    pub ambient: [f32; 3],
    pub sky_scale: f32,
    pub steps: u32,
    pub density: f32,
    /// The domain's height and the horizon distance (the aids or the defaults).
    pub domain: f32,
    pub horizon: f32,
    pub display_clip: bool,
    pub temporal: bool,
    pub view: CloudView,
    /// The level's factor on the refined march's coarse spacing ([`CloudLevel::probe_factor`]; `None`:
    /// 1, as designed).
    pub probe: Option<f32>,
    /// The aids' carve and erosion strengths (`None`: 1, as designed).
    pub shape_aid: Option<f32>,
    pub erosion_aid: Option<f32>,
    // light (stage 2e): the light's aids and the sky's colours and elevation for the ambients.
    pub light_aids: crate::volumetric_clouds_light::LightAids,
    pub sky: crate::volumetric_clouds_light::SkyLight,
    /// The wisps' strength (`None`: 1, as designed).
    pub wisp_aid: Option<f32>,
    /// What MoonlightNVSE.ini's factor adds to the moon's light, 0 without the file (`SkyState::
    /// moon_brightness_multiplier` less 1).
    pub moon_extra: f32,
}

/// Two lattices' whole cells for the drift (x, z of the first, x, z of the second) as the
/// shader's u32 bits ([`cell_offset`]).
fn whole_bits(drift: [f64; 2], a: f32, b: f32) -> UVec4 {
    let (wa, wb) = (cell_offset(drift, a).1, cell_offset(drift, b).1);
    UVec4::new(wa.x as u32, wa.y as u32, wb.x as u32, wb.y as u32)
}

/// The uniform, every value finite and every colour non-negative whatever the inputs (a
/// mod's weather, a broken record).
pub fn uniform(i: &VolumeInputs) -> CloudVolumeUniform {
    let finite = |x: f32| if x.is_finite() { x } else { 0.0 };
    // The light's colours, bounded together (#3032 stage 2e: volumetric_clouds_light.rs `lighting`).
    let lit = crate::volumetric_clouds_light::lighting(i);
    let dir = if i.light_dir.is_finite() { i.light_dir.try_normalize().unwrap_or(Vec3::Y) } else { Vec3::Y };
    let uv_scale = |x: f32| if x.is_finite() && x > 0.0 { x } else { 1.0 / crate::cloud_shadows::REPEAT_UNITS };
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    let domain = domain_height(i.domain);
    let horizon = if i.horizon.is_finite() { i.horizon.clamp(MIN_HORIZON, MAX_HORIZON) } else { HORIZON };
    let repeat = 1.0 / uv_scale(i.uv_scale.x);
    let cells = vigour_cells(domain, repeat);
    let amount = unit(i.amount);
    let cells_p = place_cells(domain, repeat);
    let (place, threshold, place_drift, cross_phase) = {
        // Apply the placement shear to the unwrapped wind in the same world basis as
        // vc_m2_place_pos. The base and detail noise still advect in ordinary world space.
        let sx = f64::from(uv_scale(i.uv_scale.x));
        let sz = f64::from(uv_scale(i.uv_scale.y));
        let shear = f64::from(crate::volumetric_clouds_model2::SHEAR);
        let drift = [i.drift[0] + shear * sx / sz * i.drift[1], i.drift[1] + shear * sz / sx * i.drift[0]];
        let place = cells_p * crate::volumetric_clouds_model2::place_frequency(unit(i.puff));
        let cross = [0.75 * i.drift[0] - 0.29 * sx / sz * i.drift[1], 0.75 * i.drift[1] - 0.29 * sz / sx * i.drift[0]];
        let phases = [1.0, PLACE_OCTAVES[0], PLACE_OCTAVES[1]].map(|octave| {
            let (frac, whole) = cell_offset(cross, place * octave);
            UVec4::new(frac.x.to_bits(), frac.y.to_bits(), whole.x as u32, whole.y as u32)
        });
        (place, crate::volumetric_clouds_model2::placement_threshold(amount), drift, phases)
    };
    let mix = match i.forced {
        Some(t) => t.weights().extend(1.0),
        None => type_mix(amount, i.bias).extend(0.0),
    };
    CloudVolumeUniform {
        slab: Vec4::new(finite(i.plane_height), domain, source_range(horizon, i.above_ground, domain), horizon),
        march: Vec4::new(step_near(i.steps), LIGHT_STEPS as f32, non_negative(i.density), LIGHT_THRESHOLD),
        reach: Vec4::new(LIGHT_REACH, shell_radius(horizon, i.above_ground), STOP_TRANSMITTANCE, RAY_BUDGET as f32),
        phase: Vec4::new(G_FORWARD, G_BACK, BACK_WEIGHT, i.light_aids.phase_cap(crate::volumetric_clouds_light::night_share(i.sky.elevation))),
        light_dir: dir.extend(unit(i.light_strength)),
        light: lit.light.extend(0.0),
        map: Vec4::new(uv_scale(i.uv_scale.x), uv_scale(i.uv_scale.y), 0.0, 0.0),
        profile: Vec4::new(BASE_ROUND, sky_share(amount), coarse_spacing(amount, domain) * probe_factor(i.probe), 0.0),
        flags: Vec4::new(flag(i.display_clip), flag(i.temporal), i.view.code(), STEP_FOOTPRINT),
        tone: Vec4::new(0.0, 0.0, NOISE_LO, NOISE_HI),
        vigour: vigour_offset(i.drift, cells),
        vigour2: vigour_offset(i.drift, cells * VIGOUR_OCTAVE),
        height: Vec4::new(cells, VIGOUR_OCTAVE, 0.0, BASE_RISE),
        field: Vec4::new(place, threshold, domain, SHEET),
        scale: Vec4::new(VIGOUR_LOW, VIGOUR_HIGH, VIGOUR_POWER, amount),
        place0: vigour_offset(place_drift, place),
        place1: vigour_offset(place_drift, place * PLACE_OCTAVES[0]),
        place2: vigour_offset(place_drift, place * PLACE_OCTAVES[1]),
        cells_v: whole_bits(i.drift, cells, cells * VIGOUR_OCTAVE),
        cells_p: whole_bits(place_drift, place, place * PLACE_OCTAVES[0]),
        cells_q: whole_bits(place_drift, place * PLACE_OCTAVES[1], 0.0),
        cross0: cross_phase[0],
        cross1: cross_phase[1],
        cross2: cross_phase[2],
        mix,
        tower: Vec4::new(TOWER_VIGOUR[0], TOWER_VIGOUR[1], TOWER_DEPTH[0], TOWER_DEPTH[1]),
        kind: Vec4::new(unit(i.puff), TOWER_GAIN, 0.0, 0.0),
        knot_s0: Vec4::from_slice(&KNOTS_SHEET[..4]),
        knot_s1: Vec4::from_slice(&KNOTS_SHEET[4..]),
        knot_c0: Vec4::from_slice(&KNOTS_CUMULUS[..4]),
        knot_c1: Vec4::from_slice(&KNOTS_CUMULUS[4..]),
        knot_t0: Vec4::from_slice(&KNOTS_TOWER[..4]),
        knot_t1: Vec4::from_slice(&KNOTS_TOWER[4..]),
        cal_c0: Vec4::from_slice(&CAL_COVERS[..4]),
        cal_c1: Vec4::from_slice(&CAL_COVERS[4..]),
        cal_k0: Vec4::from_slice(&EXTINCTION_TABLE[..4]),
        cal_k1: Vec4::from_slice(&EXTINCTION_TABLE[4..]),
        nphase: {
            let (b, e) = (noise_phase(i.drift, repeat, crate::cloud_noise::BASE_REPEAT), noise_phase(i.drift, repeat, crate::cloud_noise::EROSION_REPEAT));
            Vec4::new(b.x, b.y, e.x, e.y)
        },
        nscale: Vec4::new(
            1.0 / crate::cloud_noise::BASE_REPEAT,
            1.0 / crate::cloud_noise::EROSION_REPEAT,
            i.shape_aid.filter(|v| v.is_finite()).map_or(1.0, |v| v.clamp(0.0, MAX_STRENGTH)),
            i.erosion_aid.filter(|v| v.is_finite()).map_or(1.0, |v| v.clamp(0.0, MAX_STRENGTH)),
        ),
        nlevel: Vec4::new(
            crate::cloud_noise::BASE_SIZE as f32,
            (crate::cloud_noise::level_count(crate::cloud_noise::BASE_SIZE) - 1) as f32,
            crate::cloud_noise::EROSION_SIZE as f32,
            (crate::cloud_noise::level_count(crate::cloud_noise::EROSION_SIZE) - 1) as f32,
        ),
        nlobe: Vec4::new(NOISE_LOBE[0], NOISE_LOBE[1], 0.0, 0.0),
        nband: Vec4::new(ERODE_MEANS[0], ERODE_MEANS[1], ERODE_MEANS[2], 0.0),
        nerode: Vec4::new(NOISE_ERODE[0], NOISE_ERODE[1], 0.0, 0.0),
        dshape: crate::volumetric_clouds_wisp::wisp_block(i.wisp_aid),
        // light volume (stage 2g): filled in the render world (volumetric_clouds_lightvol.rs).
        lv: Vec4::ZERO,
        lv_near: Vec4::ZERO,
        lv_far: Vec4::ZERO,
        lv_size: Vec4::ZERO,
        lv_cam: Vec4::ZERO,
        lmode: lit.lmode,
        lupper: lit.lupper,
        llower: lit.llower,
        loct: lit.loct,
        lfloor: lit.lfloor,
        lpowder: lit.lpowder,
        lcell: Vec4::ZERO,
        air_low: Vec4::ZERO,
        air_high: Vec4::ZERO,
        families: Vec4::ZERO,
    }
}

/// Whether the material must be written: the placement's offset or the vigour's changed at all
/// (they are renewed in 1/4096 steps already), anything else by more than a thousandth (of
/// its size). Each write remakes the material's uniform on the GPU (#897), so not every frame
/// for the sun's creep.
pub fn needs_write(old: &CloudVolumeUniform, new: &CloudVolumeUniform) -> bool {
    let near = |a: Vec4, b: Vec4| (a - b).abs().cmple(Vec4::splat(1e-3) * a.abs().max(Vec4::ONE)).all();
    old.map != new.map
        || old.vigour != new.vigour
        || old.vigour2 != new.vigour2
        || old.height != new.height
        || old.field != new.field
        || old.scale != new.scale
        || old.place0 != new.place0
        || old.place1 != new.place1
        || old.place2 != new.place2
        || old.mix != new.mix
        || old.cells_v != new.cells_v
        || old.cells_p != new.cells_p
        || old.cells_q != new.cells_q
        || old.cross0 != new.cross0 || old.cross1 != new.cross1 || old.cross2 != new.cross2
        || old.tone != new.tone
        || old.cal_k0 != new.cal_k0
        || old.cal_k1 != new.cal_k1
        || old.nphase != new.nphase
        || old.nscale != new.nscale
        || old.nlevel != new.nlevel
        || old.nlobe != new.nlobe
        || old.nband != new.nband
        || old.nerode != new.nerode
        || old.dshape != new.dshape
        || old.lcell != new.lcell
        || !near(old.slab, new.slab)
        || !near(old.march, new.march)
        || !near(old.light_dir, new.light_dir)
        || !near(old.light, new.light)
        || old.flags != new.flags
        || old.reach != new.reach
        || old.phase != new.phase
        || old.profile != new.profile
        || crate::volumetric_clouds_lightvol::lv_changed(old, new)
        // light (stage 2e)
        || old.lmode != new.lmode
        || old.loct != new.loct
        || old.lfloor != new.lfloor
        || old.lpowder != new.lpowder
        || !near(old.lupper, new.lupper)
        || !near(old.air_low, new.air_low)
        || !near(old.air_high, new.air_high)
        || !near(old.families, new.families)
        || old.llower.w != new.llower.w
        || !near(old.llower, new.llower)
}

/// `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS`: the density's scale, a finite number from 0 to
/// [`MAX_DEBUG_DENSITY`]; anything else is 1 (the scale's "as designed"; a run starts from
/// [`START_VALUES`] instead, [`start_density`]).
#[cfg(test)] // the tests' reader; a run starts from the START values (stage 2i M3)
pub fn debug_density(value: Option<&str>) -> f32 {
    start_density(value, 1.0)
}

/// The density scale's start value: the variable's 0..[`MAX_DEBUG_DENSITY`], `start` when unset or
/// unreadable.
pub fn start_density(value: Option<&str>, start: f32) -> f32 {
    value.and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite() && *v >= 0.0).map_or(start, |v| v.min(MAX_DEBUG_DENSITY))
}

/// `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_STEPS`: the march's quality value, held to [`MIN_STEPS`]..
/// [`MAX_STEPS`]; unset or unreadable is [`VIEW_STEPS`] ([`step_near`]).
pub fn debug_steps(value: Option<&str>) -> u32 {
    value.and_then(|v| v.trim().parse::<u32>().ok()).map_or(VIEW_STEPS, |n| n.clamp(MIN_STEPS, MAX_STEPS))
}

/// `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_HEIGHT`: the domain's height in units, held to
/// [`MIN_DOMAIN`]..[`MAX_DOMAIN`]; unset or unreadable is [`REFERENCE_HEIGHT`] (a run starts from
/// [`START_VALUES`] instead, [`start_height`]).
#[cfg(test)] // the tests' reader
pub fn debug_height(value: Option<&str>) -> f32 {
    start_height(value, REFERENCE_HEIGHT)
}

/// The domain height's start value: the variable's, `start` when unset or unreadable.
pub fn start_height(value: Option<&str>, start: f32) -> f32 {
    value.and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).map_or(start, |v| v.clamp(MIN_DOMAIN, MAX_DOMAIN))
}

/// `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_HORIZON`: the horizon distance in units, held to
/// [`MIN_HORIZON`]..[`MAX_HORIZON`]; unset or unreadable is [`HORIZON`].
pub fn debug_horizon(value: Option<&str>) -> f32 {
    value.and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).map_or(HORIZON, |v| v.clamp(MIN_HORIZON, MAX_HORIZON))
}

/// `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_VIEW`: `cover`, `vigour`, `count`, `noise`, `sun`, `ambient`,
/// `depth`, `opacity` or `body-height`; anything else none.
pub fn debug_view(value: Option<&str>) -> CloudView {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("cover") => CloudView::Cover,
        Some("vigour") | Some("vigor") => CloudView::Vigour,
        Some("count") => CloudView::Count,
        Some("noise") => CloudView::Noise,
        Some("sun") => CloudView::Sun,
        Some("ambient") => CloudView::Ambient,
        Some("depth") => CloudView::Depth,
        Some("opacity") => CloudView::Opacity,
        Some("body-height") => CloudView::BodyHeight,
        _ => CloudView::None,
    }
}


/// The authored amount is measured before allocating any high clouds. The type bias
/// already follows weather transitions, including rain, snow and overcast.
pub(crate) fn fair_daylight_weight(amount: f32, bias: Vec3, elevation: f32) -> f32 {
    (1.0 - smoothstep(0.48, 0.70, amount))
        * (1.0 - smoothstep(0.02, 0.12, bias.x.max(bias.z)))
        * smoothstep(6.0, 22.0, elevation)
}

/// `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_REFINED_STRIDE`: the largest permitted longitudinal quadrature
/// group in the refined view march, 1, 2 or 4; `start` when unset or unreadable.
fn start_refined_stride(value: Option<&str>, start: u32) -> u32 {
    match value.map(str::trim) {
        Some("1") => 1,
        Some("2") => 2,
        Some("4") => 4,
        _ => start,
    }
}

/// `NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LIGHT_REUSE`: 1 reuses a neighboring incident-light estimate in
/// resolved cloud interiors, 0 does not; `start` when unset or unreadable.
fn start_light_reuse(value: Option<&str>, start: bool) -> bool {
    match value.map(str::trim) {
        Some("1") => true,
        Some("0") => false,
        _ => start,
    }
}

/// The aids the volume follows: the start configuration ([`START`], read once) or, in a build with the
/// developer aids (dev_aids.rs), the effective values with a tune file's overrides on top
/// (volumetric_clouds_tune.rs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudAids {
    pub density: f32,
    pub steps: u32,
    pub view: CloudView,
    pub forced: Option<CloudType>,
    pub shape: f32,
    pub erosion: f32,
    /// The light's weights (#3032 stage 2e, L2).
    pub light: crate::volumetric_clouds_light::LightAids,
    /// The wisps' strength (live).
    pub wisp: f32,
    /// Start-only.
    pub height: f32,
    pub horizon: f32,
}

impl CloudAids {
    /// The uniform's inputs with these live values ([`inputs`] reads the start
    /// values; the start-only ones are left as they are).
    pub fn apply(&self, i: VolumeInputs) -> VolumeInputs {
        VolumeInputs { density: self.density, steps: self.steps, view: self.view, forced: self.forced, shape_aid: Some(self.shape), erosion_aid: Some(self.erosion), light_aids: self.light, wisp_aid: Some(self.wisp), ..i }
    }
}

/// The run's start configuration: the aids (the live tuning's start, volumetric_clouds_tune.rs) and the
/// start-only strengths, each a start variable's value or [`START_VALUES`]'.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CloudStart {
    pub aids: CloudAids,
    pub puff: f32,
    pub refined_stride: u32,
    pub light_reuse: bool,
    pub air: f32,
}

/// The start configuration from start variables read by `get` (by name), [`START_VALUES`] with none set.
pub fn cloud_start_from(get: impl Fn(&str) -> Option<String>) -> CloudStart {
    let m = START_VALUES;
    let v = |name: &str| get(&format!("NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS{name}"));
    CloudStart {
        aids: CloudAids {
            density: start_density(v("").as_deref(), m.density),
            steps: debug_steps(v("_STEPS").as_deref()),
            view: debug_view(v("_VIEW").as_deref()),
            forced: debug_type(v("_TYPE").as_deref()),
            shape: start_strength(v("_SHAPE").as_deref(), m.shape),
            erosion: start_strength(v("_EROSION").as_deref(), m.erosion),
            light: crate::volumetric_clouds_light::start_light_aids(&get),
            wisp: crate::volumetric_clouds_wisp::start_wisp(&get),
            height: start_height(v("_HEIGHT").as_deref(), m.height),
            horizon: debug_horizon(v("_HORIZON").as_deref()),
        },
        puff: start_strength(v("_PUFF").as_deref(), m.puff).min(1.0),
        refined_stride: start_refined_stride(v("_REFINED_STRIDE").as_deref(), m.refined_stride),
        light_reuse: start_light_reuse(v("_LIGHT_REUSE").as_deref(), m.light_reuse),
        air: start_density(v("_AIR").as_deref(), m.air),
    }
}

/// The start configuration, the variables read once.
static START: std::sync::LazyLock<CloudStart> = std::sync::LazyLock::new(|| cloud_start_from(crate::dev_aids::var));

impl CloudStart {
    /// A sky's uniform inputs with the start configuration's own inputs set.
    fn inputs(&self, sky: VolumeInputs) -> VolumeInputs {
        VolumeInputs { puff: self.puff, domain: self.aids.height, horizon: self.aids.horizon, ..self.aids.apply(sky) }
    }

    /// The frame's uniform from its inputs (the live aids applied): the light reuse and the stride with it.
    fn frame_uniform(&self, input: &VolumeInputs) -> CloudVolumeUniform {
        let mut params = uniform(input);
        params.families.w = f32::from(u8::from(self.light_reuse));
        // X is used only by the refined view integrator.
        params.families.x = if self.refined_stride > 1 { self.refined_stride as f32 } else { 0.0 };
        params
    }
}

/// The aids as read at the start, the live tuning's start configuration (#3032 stage 2e, L1;
/// volumetric_clouds_tune.rs: a tune file's values go on top of these).
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
pub fn start_aids() -> CloudAids {
    START.aids
}

// ---------------------------------------------------------------------------------------
// Material, plugin and systems
// ---------------------------------------------------------------------------------------

/// volumetric_clouds.wesl `CloudCompose`: x 1 when the view shows display values clipped (no
/// Tony inverse); y 1 to hold (the proxy drawn, adding nothing, while its pipeline is asked for
/// and the flat layers still show); z, w unused.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType, Reflect)]
pub struct CloudComposeUniform {
    pub flags: Vec4,
}

/// The composition of this frame's display mode, held or not. `linear`: the buffer holds
/// linear light already (Physical lighting, sky.rs `physical_cloud_light`), which goes to the
/// tonemapper as it is, as the physical dome's does; otherwise display values, mapped for the
/// view's `curve` (tone_input.wesl).
pub fn compose_uniform(curve: crate::lights::ToneCurve, linear: bool, hold: bool) -> CloudComposeUniform {
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    let code = if linear { crate::lights::ToneCurve::Clipped.code() } else { curve.code() };
    CloudComposeUniform { flags: Vec4::new(code, flag(hold), 0.0, 0.0) }
}

/// The volume's material (volumetric_clouds.wesl): the composition's uniform and the medium's
/// far buffer (medium.rs `MediumOutput::image`), read bilinear with the image's own sampler
/// (linear, clamped to the edge).
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct CloudVolumeMaterial {
    #[uniform(0)]
    pub params: CloudComposeUniform,
    #[texture(1)]
    #[sampler(2)]
    pub far: Option<Handle<Image>>,
}

impl Material for CloudVolumeMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://nvforever/volumetric_clouds.wesl".into()
    }

    /// Premultiplied: the buffer holds the march's colour already weighted by its coverage.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    /// The transparent sort only ([`SORT_BIAS`]).
    fn depth_bias(&self) -> f32 {
        SORT_BIAS
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(_: &MaterialPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: MaterialPipelineKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        // Seen from inside the dome mesh: no face culled. The label names the pipeline in the
        // pipeline log (smoke `volumetric-clouds`).
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            draw_on_dome(depth);
        }
        descriptor.label = Some(PIPELINE_LABEL.into());
        Ok(())
    }
}

/// The proxy: the dome's mesh drawn depth-Equal on it ([`proxy_mesh`], [`draw_on_dome`]).
#[derive(Component)]
pub struct CloudVolumeProxy;

/// Whether the volume is drawn this frame: [`hide_flat_layers`] hides the flat cloud layers
/// while it is (the entities stay: cloud_shadows.rs composes from them).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CloudVolumeShown(pub bool);

/// The gate of the volume's material type (vendored bevy_pbr `MaterialGate`): open from the
/// moment the row turns on until no volume material has been left for [`IDLE_FRAMES`]
/// frames after it turned off; closed in a run that never turns it on.
#[derive(Resource, Clone)]
struct VolumeGate(MaterialGate);

pub struct VolumetricCloudsPlugin;

impl Plugin for VolumetricCloudsPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "volumetric_clouds.wesl");
        bevy::asset::load_internal_asset!(app, MARCH_SHADER, "volumetric_clouds_march.wesl", |s, _| Shader::from_wesl(s, "embedded://nvforever/volumetric_clouds_march.wesl"));
        // The light's leaf, which the march imports (#3032 stage 2e).
        bevy::asset::load_internal_asset!(app, crate::volumetric_clouds_light::LIGHT_SHADER, "volumetric_clouds_light.wesl", |s, _| Shader::from_wesl(s, "embedded://nvforever/volumetric_clouds_light.wesl"));
        let gate = MaterialGate::new();
        app.add_plugins((MaterialPlugin::<CloudVolumeMaterial>::gated(gate.clone()), crate::medium::MediumPlugin))
            .insert_resource(VolumeGate(gate))
            .init_resource::<CloudVolumeShown>()
            .init_resource::<CloudCoverWanted>()
            .init_resource::<CloudCoverPublished>()
            .init_resource::<VolumeActive>()
            .init_resource::<FlatLayersMoved>()
            .init_resource::<crate::underwater::CameraSubmerged>()
            .init_resource::<CloudNoise>()
            // While the row is on and on the frame it goes off (its images released); nothing otherwise.
            .add_systems(Update, follow_noise.run_if(noise_in_use).before(follow_volume))
            // After the cover is published.
            .add_systems(Update, (count_sky_jumps, follow_volume).chain().run_if(volume_in_use).after(crate::cloud_shadows::publish_cloud_cover))
            // Once this frame's camera is known to be under water or not, and before the medium's
            // handover reads the request.
            .add_systems(
                PostUpdate,
                trace_above_water.run_if(volume_in_use).after(crate::underwater::underwater_fog).before(crate::medium::follow_medium),
            )
            // After the medium's handover (this frame's camera size), before the material's
            // change is sent to the render world and before visibility is worked out.
            .add_systems(
                PostUpdate,
                (show_volume.run_if(volume_in_use), hide_flat_layers.run_if(flat_layers_follow))
                    .chain()
                    .after(crate::medium::follow_medium)
                    .before(bevy::asset::AssetEventSystems)
                    .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
            );
        // The live tuning (#3032 stage 2e): the tune file looked at before the cuts are counted.
        #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
        app.init_resource::<crate::volumetric_clouds_tune::CloudTune>()
            .add_systems(Update, crate::volumetric_clouds_tune::poll_tune.run_if(volume_in_use).before(count_sky_jumps));
    }
}

/// The generated noise textures (#3032 stage 2b, cloud_noise.rs): made on a thread of their own
/// when the row is first switched on, the images made from the finished bytes; row off releases
/// the images and keeps the finished bytes (2.43 MiB, held at their exact length) so the next
/// activation is an upload, not a bake (the one exception to "off
/// holds nothing"; nothing is held before first use). One job in
/// flight at a time, shared by any number of quick toggles (an activation finds the running job
/// and waits for it, never starts a second beside it); the upload copy is made on that job's
/// thread too; a job that panics ends in a logged failure, which a later activation retries.
/// While the row is off only the packed chains are held, never upload
/// copies too: the job makes its copies only for an active consumer (`want`), and a job that
/// finishes with the row off (one that read `want` just before the switch has made them) is
/// collected then, its bytes kept and its copies dropped.
#[derive(Resource)]
pub struct CloudNoise {
    bytes: Option<std::sync::Arc<crate::cloud_noise::NoiseSet>>,
    /// Whether the row is on, as the job sees it when it would make the upload copies.
    want: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The one job in flight, whichever activation started it: a bake (no bytes yet) or the upload
    /// copy of the kept bytes.
    job: Option<std::thread::JoinHandle<PreparedNoise>>,
    images: Option<[Handle<Image>; 2]>,
    activation: u64,
    on: bool,
    since: Option<std::time::Instant>,
    told_wait: bool,
    /// The activation in which the job failed (a panic, a thread that could not start): not
    /// tried again in it; a later activation tries again.
    failed: Option<u64>,
    /// What makes the bytes ([`crate::cloud_noise::bake`]; tests put a small bake here).
    bake: fn() -> crate::cloud_noise::NoiseSet,
}

impl Default for CloudNoise {
    fn default() -> Self {
        CloudNoise { bytes: None, want: Default::default(), job: None, images: None, activation: 0, on: false, since: None, told_wait: false, failed: None, bake: crate::cloud_noise::bake }
    }
}

/// What the noise's job hands over: the bytes (kept), the two images' upload copies made from them
/// when the row was on as it finished, and whether it baked them.
struct PreparedNoise {
    set: std::sync::Arc<crate::cloud_noise::NoiseSet>,
    images: Option<[Image; 2]>,
    baked: bool,
}

/// The noise's job (on its own thread): the kept bytes, or a bake when there are none, and the
/// images' upload copies while the row is on (`want`; none for a row switched off meanwhile).
fn prepare_noise(kept: Option<std::sync::Arc<crate::cloud_noise::NoiseSet>>, bake: fn() -> crate::cloud_noise::NoiseSet, want: &std::sync::atomic::AtomicBool) -> PreparedNoise {
    let (set, baked) = match kept {
        Some(set) => (set, false),
        None => (std::sync::Arc::new(bake()), true),
    };
    let images = want.load(std::sync::atomic::Ordering::Acquire).then(|| [set.base.image(), set.erosion.image()]);
    PreparedNoise { set, images, baked }
}

/// The noise's bake line, when a finished job baked.
fn log_bake(p: &PreparedNoise) {
    if p.baked {
        info!(
            "volumetric clouds: noise generated: base {0}x{0}x{0} R8 with {1} levels, erosion {2}x{2}x{2} RGBA8 with {3} levels, {4} bytes, {5} workers, {6:.0} ms",
            p.set.base.size,
            p.set.base.levels,
            p.set.erosion.size,
            p.set.erosion.levels,
            p.set.bytes(),
            p.set.workers,
            p.set.millis
        );
    }
}

/// A panic's message, for the log.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "a panic".to_string())
}

/// How long the bake may take before the log says the weather waits for it [design].
const NOISE_WAIT_SECS: f32 = 10.0;

impl CloudNoise {
    /// Both images, once made (the base, the erosion).
    pub fn ids(&self) -> Option<[AssetId<Image>; 2]> {
        self.images.as_ref().map(|[b, e]| [b.id(), e.id()])
    }
}

/// Each frame the row is on (and from the frame it goes off until no job is in flight): the noise
/// textures follow the row (made on its first activation, kept as bytes after it); the main thread
/// only hands finished images to the assets.
fn follow_noise(graphics: Option<Res<crate::graphics::Graphics>>, mut noise: ResMut<CloudNoise>, mut images: ResMut<Assets<Image>>) {
    let on = graphics.is_some_and(|g| g.volumetric_clouds);
    if on != noise.on {
        noise.on = on;
        noise.activation = noise.activation.wrapping_add(1);
        noise.told_wait = false;
        noise.since = on.then(std::time::Instant::now);
        noise.want.store(on, std::sync::atomic::Ordering::Release);
        if !on {
            if let Some([b, e]) = noise.images.take() {
                images.remove(&b);
                images.remove(&e);
            }
        }
    }
    if !on {
        // A finished job is collected, now or when it ends: its bytes kept, its upload copies
        // dropped. A job in flight is kept until then: an activation before it ends takes it (one
        // job shared by quick toggles, never a second beside it).
        if noise.job.as_ref().is_some_and(std::thread::JoinHandle::is_finished) {
            if let Some(job) = noise.job.take() {
                match job.join() {
                    Ok(p) => {
                        log_bake(&p);
                        if noise.bytes.is_none() {
                            noise.bytes = Some(p.set);
                        }
                    }
                    Err(payload) => warn!("volumetric clouds: making the noise textures failed ({}); switching the row on again tries once more", panic_text(payload.as_ref())),
                }
            }
        }
        return;
    }
    // A finished job: its bytes kept, its images handed to the assets (their copies were made on
    // its thread), or its failure logged.
    if noise.job.as_ref().is_some_and(std::thread::JoinHandle::is_finished) {
        if let Some(job) = noise.job.take() {
            match job.join() {
                Ok(p) => {
                    log_bake(&p);
                    if noise.bytes.is_none() {
                        noise.bytes = Some(p.set);
                    }
                    // A job that finished while the row was off made no copies: a new job makes
                    // them from the kept bytes (below).
                    if let (None, Some([b, e])) = (noise.images.as_ref(), p.images) {
                        noise.images = Some([images.add(b), images.add(e)]);
                    }
                }
                Err(payload) => {
                    noise.failed = Some(noise.activation);
                    warn!("volumetric clouds: making the noise textures failed ({}); the weather is shown flat; switching the row off and on again tries once more", panic_text(payload.as_ref()));
                }
            }
        }
    }
    if noise.images.is_some() {
        return;
    }
    let activation = noise.activation;
    if noise.job.is_none() && noise.failed != Some(activation) {
        let (kept, bake, want) = (noise.bytes.clone(), noise.bake, noise.want.clone());
        match std::thread::Builder::new().name("cloud-noise".into()).spawn(move || prepare_noise(kept, bake, &want)) {
            Ok(job) => noise.job = Some(job),
            Err(e) => {
                noise.failed = Some(activation);
                warn!("volumetric clouds: the noise textures' thread could not start ({e}); the weather is shown flat");
            }
        }
    }
    if noise.bytes.is_none() && noise.failed != Some(activation) && !noise.told_wait && noise.since.is_some_and(|t| t.elapsed().as_secs_f32() >= NOISE_WAIT_SECS) {
        noise.told_wait = true;
        info!("volumetric clouds: the noise textures are still being generated after {NOISE_WAIT_SECS:.0} s; the weather is shown flat until they are");
    }
}

/// Whether [`follow_noise`] has anything to do: the row is on, it was on last frame (its images to
/// release) or a job is in flight still (to be collected when it ends); otherwise it does nothing
/// and takes no asset access.
fn noise_in_use(graphics: Option<Res<crate::graphics::Graphics>>, noise: Res<CloudNoise>) -> bool {
    graphics.is_some_and(|g| g.volumetric_clouds) || noise.on || noise.job.is_some()
}

/// Whether [`follow_volume`] has anything to do: the row is on, or something of the volume
/// is left to take down (the proxy, the gate's drain, the producer's request).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
struct VolumeActive(bool);

fn volume_in_use(graphics: Option<Res<crate::graphics::Graphics>>, active: Res<VolumeActive>, wanted: Res<CloudCoverWanted>) -> bool {
    graphics.is_some_and(|g| g.volumetric_clouds) || active.0 || wanted.0
}

/// What [`follow_volume`] keeps between frames.
#[derive(Default)]
struct Follow {
    entity: Option<Entity>,
    material: Option<Handle<CloudVolumeMaterial>>,
    mesh: Option<Handle<Mesh>>,
    /// The dome mesh the proxy's was made from.
    dome_mesh: Option<AssetId<Mesh>>,
    air_dome: Option<air::Dome>,
    /// Frames since the last volume material went (switched off), until the gate closes.
    draining: Option<u32>,
    /// The `on` line was written since the row turned on.
    told_on: bool,
    /// The mean cover (in [`LOG_COVER_STEP`]s) and the weather last logged.
    told_cover: Option<(i32, u32)>,
    /// Why nothing is drawn, as last logged.
    off_reason: Option<&'static str>,
    /// The debug aids were logged.
    aids_told: bool,
    /// The level last logged since the row turned on.
    told_level: Option<CloudLevel>,
}

/// The weather's flat layers while the main volume draws. The scope and extra cameras see them
/// (#3032 decision 9), as does the water camera while its own reflected volume is warming.
pub const FLAT_CLOUD_LAYER: usize = 20;

/// Whether the flat cloud layers are on [`FLAT_CLOUD_LAYER`] ([`hide_flat_layers`] puts them
/// back on the world's layer when the volume goes).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FlatLayersMoved(bool);

fn flat_layers_follow(shown: Res<CloudVolumeShown>, moved: Res<FlatLayersMoved>) -> bool {
    shown.0 || moved.0
}

/// After [`show_volume`] has decided whether the volume draws this frame: while it draws, the
/// flat cloud layers move to [`FLAT_CLOUD_LAYER`] for the auxiliary views,
/// and back to the world's layer (implicit layer 0) in the frame
/// the volume goes, so the main view never has a frame with both or with neither. Their
/// visibility stays the weather's (sky.rs `update_clouds`, both sets of a fade), and their
/// entities stay (cloud_shadows.rs composes from them).
/// The weather's flat cloud layers and their render layers ([`hide_flat_layers`]).
type FlatLayers<'w, 's> = Query<'w, 's, (Entity, Option<&'static RenderLayers>), (With<crate::sky::CloudLayer>, Without<CloudVolumeProxy>)>;

fn hide_flat_layers(mut commands: Commands, shown: Res<CloudVolumeShown>, mut moved: ResMut<FlatLayersMoved>, flat: FlatLayers) {
    let aux = RenderLayers::layer(FLAT_CLOUD_LAYER);
    for (e, layers) in &flat {
        let on_aux = layers.is_some_and(|l| *l == aux);
        if shown.0 && !on_aux {
            commands.entity(e).insert(aux.clone());
        } else if !shown.0 && on_aux {
            commands.entity(e).remove::<RenderLayers>();
        }
    }
    if moved.0 != shown.0 {
        moved.0 = shown.0;
    }
}

/// Each frame: the row followed (the producer asked for the cover, the gate, the proxy made
/// or dropped), the proxy kept on the camera, and the medium asked to trace the slab
/// (medium.rs `MediumRequest`) only outdoors with a cover, from the live sky and the cover;
/// log lines on turning on (the first frame the volume is shown), when the cover moves by a
/// tenth or the weather changes, and once per reason nothing is drawn. Whether the proxy is
/// shown is [`show_volume`]'s, after the medium's handover.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn follow_volume(
    mut commands: Commands,
    graphics: Option<Res<crate::graphics::Graphics>>,
    gate: Res<VolumeGate>,
    mut wanted: ResMut<CloudCoverWanted>,
    mut shown: ResMut<CloudVolumeShown>,
    (cover, refused, empty): (Res<CloudCoverPublished>, Option<Res<crate::cloud_shadows::CloudCoverRefused>>, Option<Res<crate::cloud_shadows::CloudCoverEmpty>>),
    sky: Option<Res<SkyState>>,
    (mut materials, mut meshes): (ResMut<Assets<CloudVolumeMaterial>>, ResMut<Assets<Mesh>>),
    mut request: ResMut<MediumRequest>,
    cam: Query<Has<TemporalJitter>, (With<FlyCam>, Without<CloudVolumeProxy>)>,
    dome: Query<(&Mesh3d, &Transform), (With<crate::sky::SkyDome>, Without<FlyCam>, Without<CloudVolumeProxy>)>,
    mut proxy: Query<&mut Transform, (With<CloudVolumeProxy>, Without<FlyCam>)>,
    mut active: ResMut<VolumeActive>,
    streamer: Option<Res<crate::world::Streamer>>,
    (noise_set, mut state): (Option<Res<CloudNoise>>, Local<Follow>),
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    tune: Option<Res<crate::volumetric_clouds_tune::CloudTune>>,
) {
    let (on, level) = graphics.map_or((false, CloudLevel::default()), |g| (g.volumetric_clouds, g.volumetric_clouds_level));
    if wanted.0 != on {
        wanted.0 = on;
    }
    let busy = on || state.entity.is_some() || state.draining.is_some();
    if active.0 != busy {
        active.0 = busy;
    }
    if !on {
        if request.wanted || request.trace.is_some() {
            *request = MediumRequest { sky_cuts: request.sky_cuts, ..default() };
        }
        if let Some(e) = state.entity.take() {
            commands.entity(e).despawn();
            if let Some(m) = state.material.take() {
                materials.remove(&m);
            }
            if let Some(m) = state.mesh.take() {
                meshes.remove(&m);
            }
            state.dome_mesh = None;
            state.draining = Some(0);
            info!("volumetric clouds: off");
        } else if gate.0.is_open() && state.draining.is_none() {
            // Switched off before the proxy was made (no dome yet, a loading screen): the gate
            // opened all the same, so it drains and closes too.
            state.draining = Some(0);
            active.0 = true;
        }
        if shown.0 {
            shown.0 = false;
        }
        if let Some(frames) = state.draining.as_mut() {
            *frames = if materials.is_empty() { *frames + 1 } else { 0 };
            if *frames >= IDLE_FRAMES {
                gate.0.set_open(false);
                state.draining = None;
                active.0 = false;
                info!("volumetric clouds: idle, materials released");
            }
        }
        state.told_on = false;
        state.told_cover = None;
        state.off_reason = None;
        state.told_level = None;
        return;
    }
    // Asked for again below once it is known there is something to trace this frame.
    // Indoors by the cell the camera is in (the streamer's), not by whether that cell brought
    // lighting of its own.
    let indoors = streamer.as_ref().is_some_and(|s| s.interior.is_some()) || sky.as_deref().is_some_and(|s| s.interior.is_some());
    // A trace is to come once what it needs is there (the dome, the cover): a loading screen waits
    // for it (medium.rs `clouds_wait`). Under water none is ([`trace_above_water`]), nor without a
    // cloud layer to compose a cover from.
    let cloudless = empty.is_some_and(|e| e.0);
    let pending = sky.is_some() && !indoors && !cloudless;
    if !request.wanted || request.trace.is_some() || request.level != level || request.pending != pending {
        *request = MediumRequest { wanted: true, trace: None, sky_cuts: request.sky_cuts, level, pending };
    }
    if state.told_level != Some(level) {
        state.told_level = Some(level);
        info!("volumetric clouds: level {} (far buffer scale {}, probe spacing x{})", level.name(), crate::medium::scale_in_force(level), level.probe_factor());
    }
    state.draining = None;
    if !gate.0.is_open() {
        gate.0.set_open(true);
    }
    if !state.aids_told {
        state.aids_told = true;
        let aids = START.aids;
        let start = START_VALUES;
        // The body's keys at the start, one line either way (a start variable that differs is named below).
        info!("volumetric clouds: start: SHAPE {:.2}, EROSION {:.2}, DENSITY x{:.2} (start defaults: SHAPE {:.2}, EROSION {:.2}, DENSITY x{:.2}; 1 is each scale's as designed)", aids.shape, aids.erosion, aids.density, start.shape, start.erosion, start.density);
        // The near taps in force, not the count asked for (one at least).
        let taps = crate::volumetric_clouds_lightvol::lv_near_taps(&uniform(&drawn_inputs(START.inputs(VolumeInputs::default()))), crate::volumetric_clouds_lightvol::lv_config().taps);
        info!(
            "volumetric clouds: start-only: HEIGHT {:.0}, PUFF {:.2}, AIR {:.2}, LIGHT_TAPS {}, REFINED_STRIDE {}, LIGHT_REUSE {}",
            aids.height, START.puff, START.air, taps, START.refined_stride, u8::from(START.light_reuse)
        );
        if START.aids.density != start.density {
            info!("debug switch NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS: volumetric clouds density x{:.2}", START.aids.density);
        }
        if START.aids.steps != VIEW_STEPS {
            info!("debug switch NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_STEPS: volumetric clouds quality {} (near step {:.0} units)", START.aids.steps, step_near(START.aids.steps));
        }
        match START.aids.view {
            CloudView::None => {}
            CloudView::Cover => info!("debug switch NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_VIEW: the placement field against its threshold drawn as grey on the bent base"),
            CloudView::Vigour => info!("debug switch NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_VIEW: the local height's share of the domain drawn as grey on the bent base"),
            CloudView::Count => info!("debug switch NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_VIEW: the march's work per ray drawn as RGB: red the occupancy probes (white at {COUNT_PROBES:.0}), green the fine density samples (white at {COUNT_FINE:.0}), blue the light taps (white at {COUNT_TAPS:.0})"),
            CloudView::Noise => info!("debug switch NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_VIEW: the generated noise on the bent base (bands of 96,000 units across world x: grey the base, colour the erosion's bands)"),
            CloudView::Sun | CloudView::Ambient | CloudView::Depth | CloudView::Opacity | CloudView::BodyHeight => info!("debug switch NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_VIEW: {}", crate::volumetric_clouds_light::view_text(START.aids.view)),
        }
        if START.aids.height != start.height {
            info!("debug switch NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_HEIGHT: the cloud domain {:.0} units tall", START.aids.height);
        }
        if let Some(t) = START.aids.forced {
            info!("debug switch NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_TYPE: every cloud a {t:?}");
        }
        if START.aids.shape != start.shape {
            info!("debug switch NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_SHAPE: the base relief at {:.2} of its strength (held to {SHAPE_LEAST} at least)", START.aids.shape);
        }
        if START.aids.erosion != start.erosion {
            info!("debug switch NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_EROSION: the erosion at {:.2} of its strength (0: none)", START.aids.erosion);
        }
        // The body and its wisps, one line either way.
        info!("{}", crate::volumetric_clouds_wisp::describe(aids.wisp));
        if START.aids.horizon != HORIZON {
            info!("debug switch NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_HORIZON: the bent base meets the horizon at {:.0} units", START.aids.horizon);
        }
        // The march's figures (#3032 stage 2c, checkpoint C1).
        info!(
            "volumetric clouds: march: the refined march (the support probed every {COARSE_THIN:.0} units up to amount {COARSE_THIN_UP_TO}, {COARSE_FAIR:.0} above, with the domain, times the level's factor, wider where the finest placement octave is gone; entries and exits bracketed to one fine cell; fine steps within half the finest detail at each block, the quality scaling them ({VIEW_STEPS} = 1); the lowest {:.0} units always fine; at most {RAY_BUDGET} evaluations a ray)",
            BASE_RISE + SHEET
        );
        // The light model in force at the start, one line either way (#3032 stage 2e, L2).
        info!("volumetric clouds: light: {}", aids.light.model_text());
    }
    // The aids in force: the start's, with a tune file's values on top where there can be one.
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    let aids = tune.as_deref().map_or(START.aids, |t| t.effective());
    #[cfg(not(any(test, debug_assertions, feature = "dev-aids")))]
    let aids = START.aids;
    let Ok(temporal) = cam.single() else { return };
    // The proxy is the dome's mesh (drawn depth-Equal on it), made again when the dome is
    // (another worldspace).
    let Ok((dome_mesh, dome_t)) = dome.single() else { return };
    if state.dome_mesh != Some(dome_mesh.0.id()) {
        let Some(mesh) = meshes.get(&dome_mesh.0).and_then(proxy_mesh) else { return };
        if START.air > 0.0 {
            state.air_dome = meshes.get(&dome_mesh.0).and_then(air::Dome::new);
        }
        let mesh = meshes.add(mesh);
        if let Some(old) = state.mesh.replace(mesh.clone()) {
            meshes.remove(&old);
        }
        state.dome_mesh = Some(dome_mesh.0.id());
        if let Some(e) = state.entity {
            commands.entity(e).insert(Mesh3d(mesh));
        }
    }
    let Some(entity) = state.entity else {
        // Made hidden; shown once the medium has written its buffer (show_volume).
        let Some(mesh) = state.mesh.clone() else { return };
        let material = materials.add(CloudVolumeMaterial::default());
        let e = commands
            .spawn((
                Name::new("Volumetric clouds"),
                CloudVolumeProxy,
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                *dome_t,
                Visibility::Hidden,
                RenderLayers::layer(VOLUME_LAYER),
                NotShadowCaster,
                NotShadowReceiver,
            ))
            .id();
        state.entity = Some(e);
        state.material = Some(material);
        return;
    };
    let Ok(mut t) = proxy.get_mut(entity) else { return };
    if *t != *dome_t {
        *t = *dome_t;
    }
    // A cover of nothing (a clear night's transparent placeholder layers) is not marched:
    // nothing would come of it, at full cost. The cover view still draws.
    // The amount and the tint shares this frame, from the live weathers' own covers and the live
    // fade's weights once the incoming textures have been measured.
    let blended = match (sky.as_deref(), cover.0.as_ref()) {
        (Some(s), Some(c)) => blend_covers(c.contributors(s), &c.weathers),
        _ => None,
    };
    let reason = match (sky.as_deref(), cover.0.as_ref()) {
        (None, _) => Some("no sky"),
        _ if indoors => Some("indoors"),
        (_, None) if refused.as_ref().is_some_and(|r| r.0.is_some()) => Some("a cloud texture without plain 2D alpha: the weather is shown flat"),
        (_, None) if cloudless => Some("a weather without cloud layers"),
        (_, None) => Some("no cloud cover"),
        _ if blended.is_none() => Some("a live weather's own cover is not composed yet: the weather is shown flat"),
        _ if blended.is_some_and(|b| !has_cover(b.amount)) && !matches!(aids.view, CloudView::Cover | CloudView::Vigour) => Some("a clear sky: the cover is empty"),
        _ => None,
    };
    if let Some(r) = reason {
        // Only a cover still being composed is to come; a refused or an empty one is shown flat.
        let composing = blended.is_none() && (cover.0.is_some() || refused.as_ref().is_none_or(|r| r.0.is_none()));
        request.pending = pending && composing;
        if state.off_reason != reason {
            state.off_reason = reason;
            info!("volumetric clouds: none ({r})");
        }
        return;
    }
    let (Some(sky), Some(c), Some(blended)) = (sky.as_deref(), cover.0.as_ref(), blended) else { return };
    // The live tuning's effective aids on top of the start values (#3032 stage 2e).
    let input = inputs(sky, c, &blended, temporal);
    let input = physical_sun_hue(aids.apply(input), nvf_assets::authored::physical_pbr());
    // The level's probe spacing (the far buffer's scale is the medium's, by the request's level).
    let input = drawn_inputs(VolumeInputs { probe: Some(level.probe_factor()), ..input });
    // A weather the medium cannot use is shown flat, not repaired.
    if !usable_values(sky, c, &input) {
        let r = Some(UNUSABLE);
        request.pending = false;
        if state.off_reason != r {
            state.off_reason = r;
            info!("volumetric clouds: none ({UNUSABLE})");
        }
        return;
    }
    state.off_reason = None;
    let mut params = START.frame_uniform(&input);
    if input.view == CloudView::None && START.air > 0.0 {
        if let (Some(probe), Some(mesh)) = (&state.air_dome, meshes.get(&dome_mesh.0)) {
            probe.write(mesh, &input, &mut params, START.air);
        }
    }
    request.trace = Some(SourceFrame {
        clouds: params,
        composer: state.material.as_ref().map(|m| m.id().untyped()),
        composer_entity: state.entity,
        distance_scale: params.slab.z,
        noise: noise_set.as_ref().and_then(|n| n.ids()),
        weather: sky.prev.is_none().then_some(sky.weather.form_id),
    });
    if shown.0 && !state.told_on {
        state.told_on = true;
        info!(
            "volumetric clouds: on, slab {:.0}..{:.0} (the cloud shadows' plane), quality {} ({}), {LIGHT_STEPS} light steps over at most {LIGHT_REACH:.0}, placed by our own field (amount {:.2}: sky share {:.2}, threshold {:.3}, cells {:.2} a repeat), types sheet/cumulus/tower {:.2}/{:.2}/{:.2}{}, local heights from {SHEET:.0} to {:.0}, domes over the masses times {VIGOUR_LOW}..{VIGOUR_HIGH} by the vigour, flat bases, bent to meet the horizon at {:.0} (shell radius {:.0}), fading from {:.0}, range {:.0}, density x{:.2}, cover {:.2}, traced into the medium's far buffer{}",
            params.slab.x,
            params.slab.x + params.slab.y,
            START.aids.steps,
            format!(
                "the refined march: the support probed every {:.0} units, fine steps of {:.0} units near, up to {STEP_FOOTPRINT:.0} footprints far within the finest detail, at most {RAY_BUDGET} evaluations a ray",
                params.profile.z, params.march.x
            ),
            params.scale.w,
            sky_share(params.scale.w),
            params.field.y,
            params.field.x,
            params.mix.x,
            params.mix.y,
            params.mix.z,
            if params.mix.w > 0.5 { " (forced)" } else { "" },
            params.slab.y,
            params.slab.w,
            params.reach.y,
            HORIZON_FADE_FROM * params.slab.w,
            params.slab.z,
            params.march.z,
            blended.amount,
            match crate::medium::medium_mode() {
                crate::medium::MediumMode::Default if crate::medium::medium_tile() == 1 => ", one ray per texel a frame, start offsets turned at each cast (R2 sequence)",
                crate::medium::MediumMode::Default if crate::medium::medium_tile() == 3 => ", one ray per 3x3 tile a frame, start offsets turned every nine phases (R2 sequence)",
                crate::medium::MediumMode::Default => ", one ray per 2x2 tile a frame, start offsets turned at each cast (R2 sequence)",
                crate::medium::MediumMode::Reference if params.flags.y > 0.5 => ", reference: dither rotated each frame (temporal)",
                crate::medium::MediumMode::Reference => ", reference: fixed dither",
            }
        );
    }
    let told = ((blended.amount / LOG_COVER_STEP).round() as i32, sky.weather.form_id);
    if state.told_cover != Some(told) {
        if state.told_cover.is_some() {
            info!("volumetric clouds: cover {:.2} ({})", blended.amount, sky.weather.edid.as_deref().unwrap_or("?"));
        }
        state.told_cover = Some(told);
    }
}

/// The camera under water this frame (underwater.rs `underwater_fog`, in `PostUpdate` with the
/// camera the renderer draws from): nothing is traced and none is to come, so the flat layers are
/// back in the frame it goes under, not the one after.
fn trace_above_water(submerged: Option<Res<crate::underwater::CameraSubmerged>>, mut request: ResMut<MediumRequest>, mut told: Local<bool>) {
    let under = submerged.is_some_and(|s| s.0);
    if under && (request.trace.is_some() || request.pending) {
        request.trace = None;
        request.pending = false;
        if !std::mem::replace(&mut *told, true) {
            info!("volumetric clouds: none (the camera is under water)");
        }
    } else if !under {
        *told = false;
    }
}

/// After the medium's handover (medium.rs `follow_medium`): the proxy's material given the
/// medium's image and this frame's display mode. The proxy is drawn as soon as an image is
/// bound and a trace asked for, but **held** (its fragment adds nothing) until the medium says
/// its buffer of this frame's size was written whole and the proxy's own pipeline is ready:
/// bevy_pbr asks for a material's pipeline only for a visible entity, so without the hold the
/// flat layers would go some hundreds of milliseconds before the volume could draw.
/// The flat layers are hidden ([`CloudVolumeShown`],
/// [`hide_flat_layers`]) in the frame the hold is released, never before. A new image is never
/// drawn the frame it is swapped in (the medium's epoch moved with it).
pub(super) fn show_volume(
    request: Res<MediumRequest>,
    medium: Res<MediumOutput>,
    mut shown: ResMut<CloudVolumeShown>,
    mut materials: ResMut<Assets<CloudVolumeMaterial>>,
    mut proxy: Query<(&MeshMaterial3d<CloudVolumeMaterial>, &mut Visibility), With<CloudVolumeProxy>>,
) {
    let Ok((material, mut vis)) = proxy.single_mut() else {
        if shown.0 {
            shown.0 = false;
        }
        return;
    };
    let far_changed = materials.get(&material.0).is_some_and(|m| m.far != medium.image);
    let bound = request.trace.is_some() && medium.image.is_some() && !far_changed;
    let drawing = bound && medium.ready;
    let compose = compose_uniform(crate::lights::tone_curve(), nvf_assets::authored::physical_pbr(), !drawing);
    let compose_changed = materials.get(&material.0).is_some_and(|m| m.params.flags.x != compose.flags.x || m.params.flags.y != compose.flags.y);
    if far_changed || compose_changed {
        if let Some(mut m) = materials.get_mut(&material.0) {
            m.far = medium.image.clone();
            m.params.flags.x = compose.flags.x;
            m.params.flags.y = compose.flags.y;
        }
    }
    let want_vis = if bound { Visibility::Inherited } else { Visibility::Hidden };
    if *vis != want_vis {
        *vis = want_vis;
    }
    if shown.0 != drawing {
        shown.0 = drawing;
    }
}

/// The reason logged for a weather the medium cannot use ([`usable_values`]).
const UNUSABLE: &str = "a weather value the volume cannot use (not a finite number): the weather is shown flat";

/// Whether every value the march reads from the weather, on both sides of a fade, and from its
/// cover is a finite number: the fog distances, the light and cloud colours as they reach the
/// uniform, the sky level, the sun's direction, the cover's scale, drift, plane and
/// shares. A weather with any other is shown flat, never repaired into a volume;
/// finite values keep their bounds in [`uniform`] ([`MAX_LIGHT`]) with no range guessed.
pub fn usable_values(sky: &SkyState, c: &CloudCoverOutput, i: &VolumeInputs) -> bool {
    let fading = sky.prev.as_ref().filter(|_| sky.fade < 1.0);
    let fog = std::iter::once(&sky.weather).chain(fading).all(|w| w.fog.iter().all(|v| v.is_finite()))
        && sky.fog_override.is_none_or(|(near, far)| near.is_finite() && far.is_finite());
    let rgb = |v: [f32; 3]| v.iter().all(|x| x.is_finite());
    fog && sky.fade.is_finite()
        && i.plane_height.is_finite()
        && i.above_ground.is_finite()
        && i.drift.iter().all(|v| v.is_finite())
        && i.uv_scale.is_finite()
        && i.light_dir.is_finite()
        && i.light_strength.is_finite()
        && rgb(i.sunlight)
        && rgb(i.ambient)
        && rgb(i.sky.upper) && rgb(i.sky.horizon) && rgb(i.sky.ambient) && i.sky.elevation.is_finite()
        && i.sky_scale.is_finite()
        && c.plane_height.is_finite()
        && c.uv_scale.is_finite()
        && c.drift.iter().all(|v| v.is_finite())
        && c.mean_cover.is_finite()
        && c.layer_cover.iter().flatten().all(|v| v.is_finite())
        && c.weathers.iter().flatten().all(|w| w.mean.is_finite() && w.shares.iter().all(|v| v.is_finite()))
}

/// Each frame the row is on, before [`follow_volume`]: the sky's jumps that drop the medium's
/// history ([`crate::medium::sky_cut`]: a weather snap, indoors and out, another worldspace's
/// dome, the game clock stepping back or further than its time scale predicts), counted into the
/// request (`MediumRequest::sky_cuts`), from the live sky.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn count_sky_jumps(
    sky: Option<Res<SkyState>>,
    (time, real): (Res<Time>, Res<Time<Real>>),
    sim: Option<Res<nvf_play::sim::SimState>>,
    streamer: Option<Res<crate::world::Streamer>>,
    dome: Query<&Mesh3d, (With<crate::sky::SkyDome>, Without<CloudVolumeProxy>)>,
    mut request: ResMut<MediumRequest>,
    mut last: Local<Option<crate::medium::SkyMark>>,
    mut cuts: Local<u32>,
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    tune: Option<Res<crate::volumetric_clouds_tune::CloudTune>>,
) {
    let Some(s) = sky.as_deref() else { return };
    let mark = crate::medium::SkyMark {
        weather: s.weather.form_id,
        fading_from: s.prev.as_ref().filter(|_| s.fade < 1.0).map(|w| w.form_id),
        indoors: streamer.as_ref().is_some_and(|st| st.interior.is_some()) || s.interior.is_some(),
        // The simulation's continuous game time (its days passed, a float) where there is one.
        game_hours: sim.as_ref().and_then(|sim| sim.world.calendar().ok()).map(|c| c.days_passed * 24.0),
        hour: s.hour,
        dome: dome.single().ok().map(|m| m.0.id()),
    };
    // Game hours a real second: the sky's own test scale, else the game's TimeScale (game
    // seconds a real second; the base game's 30 without a simulation).
    let scale = if s.time_scale > 0.0 { s.time_scale } else { sim.as_ref().and_then(|sim| sim.world.time_scale()).unwrap_or(30.0) as f32 / 3600.0 };
    let allowance = crate::medium::clock_allowance(scale, time.delta_secs().max(real.delta_secs()));
    if last.as_ref().is_some_and(|before| crate::medium::sky_cut(before, &mark, allowance)) {
        *cuts = cuts.wrapping_add(1);
    }
    *last = Some(mark);
    let total = *cuts;
    // Each change of a tune file's effective values drops the history once too (#3032 stage 2e).
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    let total = total.wrapping_add(tune.as_ref().map_or(0, |t| t.edits()));
    if request.sky_cuts != total {
        request.sky_cuts = total;
    }
}

/// The sunlight's share of the direct light's colour under Physical lighting (the `SUNCOLOUR`
/// aid's value there): the physical sun's colour carries the hour, warm low and white high, as
/// the ground it lights shows, so the clouds take it whole instead of the weather's cloud
/// colours at its luminance; otherwise they stay grey over a warm evening land [design;
/// observed: Goodsprings at 17:30, 2026-11-26].
pub const PHYSICAL_SUN_COLOUR_SHARE: f32 = 1.0;

/// The inputs with [`PHYSICAL_SUN_COLOUR_SHARE`] under Physical lighting, after the aids (which
/// would set the share back); unchanged otherwise.
fn physical_sun_hue(input: VolumeInputs, physical: bool) -> VolumeInputs {
    if !physical {
        return input;
    }
    let light_aids = crate::volumetric_clouds_light::LightAids { sun_colour: PHYSICAL_SUN_COLOUR_SHARE, ..input.light_aids };
    VolumeInputs { light_aids, ..input }
}

/// The uniform's inputs from the live sky, the cover and this frame's blend of the weathers'
/// own covers (the amount and the tint shares, [`blend_covers`]).
fn inputs(sky: &SkyState, c: &CloudCoverOutput, blended: &BlendedCover, temporal: bool) -> VolumeInputs {
    let (light_dir, light_strength) = direct_light(sky.sun_dir(), sky.sun_elevation(), sky.climate.has_moon());
    // Under Physical lighting the clouds are lit as the dome they lie on is drawn: the
    // surfaces' sun colour and the physical sky's, in linear light (sky.rs
    // `physical_cloud_light`; graphics-enhanced.md §3.25), and composed without the
    // tonemapper's inverse ([`compose_uniform`]).
    let daylight = nvf_assets::authored::physical_pbr().then(|| sky.physical_cloud_light());
    START.inputs(VolumeInputs {
        plane_height: c.plane_height,
        above_ground: crate::cloud_shadows::plane_above_ground(),
        uv_scale: c.uv_scale,
        drift: c.drift,
        amount: blended.amount,
        bias: sky_bias(sky.weather.classification, sky.prev.as_ref().filter(|_| sky.fade < 1.0).map(|w| w.classification), sky.fade),
        light_dir,
        light_strength,
        sunlight: daylight.map_or_else(|| sky.color(ColorType::Sunlight), |d| d.sun),
        ambient: ambient_colour(&cloud_parts(sky, &blended.layer_cover, &blended.layer_weathers)),
        sky_scale: sky.sky_scale(),
        display_clip: crate::lights::display_clip(),
        temporal,
        sky: crate::volumetric_clouds_light::SkyLight {
            upper: daylight.map_or_else(|| sky.color(ColorType::SkyUpper), |d| d.upper),
            horizon: daylight.map_or_else(|| sky.color(ColorType::Horizon), |d| d.horizon),
            ambient: daylight.map_or_else(|| sky.color(ColorType::Ambient), |d| d.ground),
            elevation: sky.sun_elevation(),
        },
        moon_extra: sky.moon_brightness_multiplier() - 1.0,
        ..VolumeInputs::default()
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The launch judged by eye on 2026-11-23, as it was given before its values became the defaults:
    /// the start variables (without the prefix; those of removed parts left out) and the tune file
    /// (its LOWER line at 1; the night's keys are newer than that launch and at their start values here).
    const ACCEPTED_VARS: [(&str, &str); 6] = [
        ("LIGHT_TAPS", "2"), ("HEIGHT", "24000"), ("PUFF", "1"),
        ("REFINED_STRIDE", "4"), ("LIGHT_REUSE", "1"), ("AIR", ".82"),
    ];
    const ACCEPTED_TUNE: &str = "SHAPE=1\nEROSION=1.26\nDENSITY=0.76\nWISP=0.52\nSUN=1.30\nLEVEL=1.12\nOCTAVE=1\nBACKSCATTER=1.65\nFORWARD=1\nUPPER=1\nLOWER=1\nGROUND=1\nOCCLUDE=1\nPOWDER=1\nSUNCOLOUR=0\n";

    /// A run with no start variable and no tune file prepares the uniforms and the light volume's settings
    /// of the accepted launch.
    #[test]
    fn a_default_run_is_the_accepted_launch() {
        use crate::volumetric_clouds_lightvol::{lv_config_from, lv_fill};
        let accepted = |n: &str| {
            let short = n.strip_prefix("NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_")?;
            ACCEPTED_VARS.iter().find(|(k, _)| *k == short).map(|(_, v)| v.to_string())
        };
        let default = cloud_start_from(|_| None);
        let tune = crate::volumetric_clouds_tune::parse_tune(ACCEPTED_TUNE);
        assert!(tune.bad.is_empty(), "{:?}", tune.bad);
        let launch = cloud_start_from(accepted);
        // Over aids of other values, so that every live value is the tune file's.
        let other = CloudAids { shape: 3.0, erosion: 3.0, density: 3.0, wisp: 3.0, light: crate::volumetric_clouds_light::LightAids { weights: [3.0; crate::volumetric_clouds_light::LIGHT_WEIGHTS], sun_colour: 1.0, ..launch.aids.light }, ..launch.aids };
        let launch = CloudStart { aids: other.with(&tune.overrides), ..launch };
        assert_eq!(default, launch);
        assert_eq!((default.puff, default.refined_stride, default.light_reuse, default.air, default.aids.height), (1.0, 4, true, 0.82, 24_000.0));
        let (lv, lv_launch) = (lv_config_from(|_| None), lv_config_from(accepted));
        assert_eq!(lv, lv_launch);
        // The packed uniforms, over fair, overcast, rain and night skies.
        for flags in [0u8, 1, 2, 4, 8] {
            for amount in [0.05f32, 0.29, 0.6, 0.74, 0.95] {
                for elevation in [-10.0f32, 2.0, 12.0, 60.0] {
                    let mut sky = VolumeInputs {
                        plane_height: 20_000.0,
                        above_ground: 20_000.0,
                        drift: [3.25, -1.5],
                        amount,
                        bias: sky_bias(flags, None, 1.0),
                        light_dir: Vec3::new(0.3, 0.8, 0.2),
                        light_strength: 0.9,
                        sunlight: [0.9, 0.8, 0.6],
                        ambient: [0.5, 0.4, 0.3],
                        sky_scale: 1.0,
                        ..Default::default()
                    };
                    sky.sky = crate::volumetric_clouds_light::SkyLight { upper: [0.2, 0.3, 0.8], horizon: [0.6, 0.6, 0.7], ambient: [0.3; 3], elevation };
                    let (a, b) = (default.inputs(sky), launch.inputs(sky));
                    let (mut ua, mut ub) = (default.frame_uniform(&a), launch.frame_uniform(&b));
                    lv_fill(&mut ua, Vec3::new(100.0, 20.0, -50.0), &lv);
                    lv_fill(&mut ub, Vec3::new(100.0, 20.0, -50.0), &lv_launch);
                    assert_eq!(ua, ub, "flags {flags}, amount {amount}, elevation {elevation}");
                    assert_eq!((ua.slab.y, ua.march.z, ua.nscale.z, ua.nscale.w, ua.dshape.z), (24_000.0, 0.76, 1.0, 1.26, 0.52));
                    assert_eq!((ua.kind.x, ua.families.x, ua.families.w, ua.lv.x), (1.0, 4.0, 1.0, 2.0), "PUFF, REFINED_STRIDE, LIGHT_REUSE, LIGHT_TAPS");
                    assert_eq!((ua.lmode.x, ua.lupper.w, ua.llower.w), (0.0, 0.0, 0.0), "the lean light");
                }
            }
        }
        // The start-only readers: every name selects, anything else is the start's.
        assert_eq!((start_refined_stride(Some("1"), 4), start_refined_stride(Some("3"), 4), start_refined_stride(Some("2"), 1)), (1, 4, 2));
        assert_eq!((start_light_reuse(Some("0"), true), start_light_reuse(Some("x"), true), start_light_reuse(Some("1"), false)), (false, true, true));
    }

    #[test]
    fn volume_type_profiles_stay_in_the_cloud() {
        for knots in [&KNOTS_SHEET, &KNOTS_CUMULUS, &KNOTS_TOWER] {
            for h in [-1.0, 0.0, 1.0, 2.0, f32::NAN] {
                assert_eq!(knots_at(h, knots), 0.0, "outside the cloud at {h}");
            }
            let mut last = 2.0;
            for i in 1..100 {
                let g = knots_at(i as f32 / 100.0, knots);
                assert!((0.0..=1.0).contains(&g) && g <= last + 1e-6, "bounded and never wider higher up: {g}");
                last = g;
            }
            assert!(knots_at(0.999, knots) < 0.05, "zero at the top");
        }
        // The cumulus is widest in its lower half and narrows to a rounded top; the tower stays
        // broad higher; the sheet thins soonest.
        let mean = |k: &[f32; 8], lo: f32, hi: f32| (0..50).map(|i| knots_at(lo + (hi - lo) * (i as f32 + 0.5) / 50.0, k)).sum::<f32>() / 50.0;
        assert!(mean(&KNOTS_CUMULUS, 0.0, 0.5) > mean(&KNOTS_CUMULUS, 0.5, 1.0) + 0.2);
        assert!(mean(&KNOTS_TOWER, 0.5, 1.0) > mean(&KNOTS_CUMULUS, 0.5, 1.0) && mean(&KNOTS_CUMULUS, 0.5, 1.0) > mean(&KNOTS_SHEET, 0.5, 1.0));
        // A knot list can hold a gap (two bands with clear air between).
        let gap = [1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0];
        assert_eq!(knots_at(0.45, &gap), 0.0);
        assert!(knots_at(0.1, &gap) > 0.9 && knots_at(0.8, &gap) > 0.9);
        // Mixed profiles: linear in the weights.
        let h = 0.6;
        let w = Vec3::new(0.2, 0.5, 0.3);
        let want = 0.2 * knots_at(h, &KNOTS_SHEET) + 0.5 * knots_at(h, &KNOTS_CUMULUS) + 0.3 * knots_at(h, &KNOTS_TOWER);
        assert!((profile_at(h, w) - want).abs() < 1e-6);
    }

    /// #3032: the volume is drawn only while every live
    /// weather with a positive weight has its own cover, and then the amount and the tint shares
    /// are the live weights times each weather's own mean and shares. Through publication
    /// (`live_covers` of the producer's covers, then `blend_covers`): A alone; a seven-frame fade to
    /// B whose cover is late (flat from the first frame B has a weight until it arrives, then the
    /// exact live blend at once); a reversal (continuous with the weights); a paused frame (the same
    /// output); a fade that finishes before B's job (flat, then B alone exactly); a weather at
    /// weight 0 needs no cover; never a cover under another weather's ID.
    #[test]
    fn the_amount_and_the_tint_follow_the_live_fade_together() {
        use crate::cloud_shadows::{live_covers, WeatherCover};
        let (a, b) = (WeatherCover { id: 1, mean: 0.29, shares: [0.2, 0.09, 0.0, 0.0] }, WeatherCover { id: 2, mean: 0.73, shares: [0.5, 0.0, 0.23, 0.0] });
        let exact = |out: Option<BlendedCover>, live: [(Option<u32>, f32); 2]| {
            let out = out.expect("drawn");
            let mut amount = 0.0;
            for (k, (id, w)) in live.iter().enumerate() {
                let c = [a, b].into_iter().find(|c| Some(c.id) == *id);
                let (w, mean, shares) = (if *w > 0.0 { *w } else { 0.0 }, c.map_or(0.0, |c| c.mean), c.map_or([0.0; 4], |c| c.shares));
                amount += w * mean;
                for (l, s) in shares.iter().enumerate() {
                    assert!((out.layer_cover[k][l] - w * s).abs() < 1e-6, "the tint shares are the live weights' ({live:?})");
                }
            }
            assert!((out.amount - amount).abs() < 1e-6, "the amount is the live weights' ({live:?}): {} against {amount}", out.amount);
            out.amount
        };
        let known_a = vec![a];
        let known_ab = vec![a, b];
        let alone = [(Some(1), 1.0), (None, 0.0)];
        assert_eq!(exact(blend_covers(alone, &live_covers(&known_a, alone)), alone), a.mean);
        // Weight 0: the incoming weather needs no cover yet.
        let start = [(Some(2), 0.0), (Some(1), 1.0)];
        assert_eq!(exact(blend_covers(start, &live_covers(&known_a, start)), start), a.mean);
        // A fade to B, B's cover late: flat from the first frame B has a weight; from its arrival the
        // live blend; a paused frame repeats; the fade turns round at f = 5/7, continuous.
        let mut last = a.mean;
        for frame in 1..=5 {
            let f = frame as f32 / 7.0;
            let live = [(Some(2), f), (Some(1), 1.0 - f)];
            let known = if frame >= 3 { &known_ab } else { &known_a };
            let out = blend_covers(live, &live_covers(known, live));
            if frame < 3 {
                assert_eq!(out, None, "frame {frame}: B has a weight and no cover: the whole sky flat");
                continue;
            }
            last = exact(out, live);
            assert_eq!(blend_covers(live, &live_covers(known, live)), out, "a paused frame: the same");
        }
        assert!((last - (5.0 / 7.0 * b.mean + 2.0 / 7.0 * a.mean)).abs() < 1e-5);
        for k in 2..=7 {
            let f = k as f32 / 7.0;
            let live = [(Some(1), f), (Some(2), 1.0 - f)];
            let now = exact(blend_covers(live, &live_covers(&known_ab, live)), live);
            assert!((now - last).abs() <= (b.mean - a.mean) / 7.0 + 1e-5, "one seventh of the change a frame at most: {last} to {now}");
            last = now;
        }
        assert!((last - a.mean).abs() < 1e-6);
        // A fade that finishes before B's job: B alone and no cover: flat; its cover in: B exactly.
        let b_alone = [(Some(2), 1.0), (None, 0.0)];
        assert_eq!(blend_covers(b_alone, &live_covers(&known_a, b_alone)), None);
        assert_eq!(exact(blend_covers(b_alone, &live_covers(&known_ab, b_alone)), b_alone), b.mean);
        // Never a cover under another weather's ID; not a number: flat.
        assert_eq!(blend_covers(b_alone, &[Some(a), None]), None);
        let nan = WeatherCover { mean: f32::NAN, ..b };
        assert_eq!(blend_covers(b_alone, &[Some(nan), None]), None);
    }

    /// The weather's type mix: from the amount and the classification bias (blended across a
    /// fade), sums to 1, non-negative; fair weather all cumulus; the tower's share never falls as
    /// the amount rises; heavy weather a deck with towers; unknown bits fall back to the amount
    /// alone; rain counted once (the classification's bit, not the separate flag too).
    #[test]
    fn the_type_mix_follows_the_amount_and_the_classification() {
        let mut last_tower = 0.0;
        for k in 0..=100 {
            let a = k as f32 / 100.0;
            let m = type_mix(a, Vec3::ZERO);
            assert!((m.x + m.y + m.z - 1.0).abs() < 1e-5 && m.min_element() >= 0.0, "{a}: {m}");
            assert!(m.z + 1e-6 >= last_tower, "tower propensity monotone: {a}");
            last_tower = m.z;
        }
        assert_eq!(type_mix(0.3, Vec3::ZERO), Vec3::Y, "fair weather: all cumulus");
        let thin = type_mix(0.1, Vec3::ZERO);
        assert!(thin.x > 0.3 && thin.z == 0.0, "thin weather: sheets among small cumulus: {thin}");
        let heavy = type_mix(0.85, Vec3::ZERO);
        assert!(heavy.z > 0.25 && heavy.x > 0.15, "heavy weather: a deck with towers: {heavy}");
        // The classification: cloudy and rainy toward the deck and the tower; rain once.
        assert!(type_mix(0.6, class_bias(2)).z > type_mix(0.6, Vec3::ZERO).z);
        assert!(type_mix(0.6, class_bias(4)).z > type_mix(0.6, class_bias(2)).z);
        assert_eq!(class_bias(0), Vec3::ZERO);
        assert_eq!(class_bias(0xf0), Vec3::ZERO, "bits it does not know: the amount alone");
        assert_eq!(class_bias(4 | 0x80), class_bias(4));
        // A fade blends the two weathers' biases; its ends are each weather's.
        assert_eq!(sky_bias(2, Some(8), 1.0), class_bias(2));
        assert_eq!(sky_bias(2, Some(8), 0.0), class_bias(8));
        assert!((sky_bias(2, Some(8), 0.5) - (class_bias(2) + class_bias(8)) * 0.5).abs().max_element() < 1e-6);
        assert_eq!(sky_bias(2, Some(8), f32::NAN), class_bias(8), "not a number: the fading weather");
        // Extremes: finite and normalised whatever the bias.
        for b in [Vec3::splat(-9.0), Vec3::splat(1e30), Vec3::NAN] {
            let m = type_mix(0.5, b);
            assert!(m.is_finite() && (m.x + m.y + m.z - 1.0).abs() < 1e-4, "{b}: {m}");
        }
        // The local choice: no tower at a mass's shallow parts or under a low vigour; deep in a
        // vigorous mass the tower's weight grows; a forced type is taken as it is.
        let mix = type_mix(0.85, Vec3::ZERO).extend(0.0);
        assert_eq!(local_types(mix, 0.01, 1.0).z, 0.0);
        assert_eq!(local_types(mix, 0.5, 0.1).z, 0.0);
        let deep = local_types(mix, 0.5, 1.0);
        assert!(deep.z > mix.z && (deep.x + deep.y + deep.z - 1.0).abs() < 1e-5, "{deep}");
        assert_eq!(local_types(CloudType::Tower.weights().extend(1.0), 0.0, 0.0), Vec3::Z);
        assert_eq!(debug_type(Some(" Tower ")), Some(CloudType::Tower));
        assert_eq!(debug_type(Some("sheet")), Some(CloudType::Sheet));
        assert_eq!(debug_type(Some("cumulus")), Some(CloudType::Cumulus));
        assert_eq!(debug_type(Some("x")), None);
    }

    /// The target opacity follows the cover, the table is held outside its amounts, the base rounds in
    /// over [`BASE_ROUND`] units and the vigour's noise is spread over 0..1.
    #[test]
    fn the_target_and_the_table_follow_the_cover() {
        assert_eq!(target_opacity(0.0), 0.0);
        let mut last = 0.0;
        for k in 1..=20 {
            let c = k as f32 / 20.0;
            let t = target_opacity(c);
            assert!(t > last && t < 1.0, "cover {c}: {t}");
            last = t;
            assert!(table_depth(c) > 0.0 && table_depth(c).is_finite());
        }
        assert!((target_opacity(0.2) - 0.239).abs() < 1e-3 && (target_opacity(0.9) - 0.931).abs() < 1e-3);
        assert_eq!((base_fade(0.0), base_fade(BASE_ROUND)), (0.0, 1.0));
        const { assert!(BASE_RISE <= 0.05 * MIN_DOMAIN) };
        assert_eq!(table_depth(CAL_COVERS[2]), EXTINCTION_TABLE[2]);
        assert_eq!(table_depth(0.0), EXTINCTION_TABLE[0]);
        assert_eq!(table_depth(1.0), EXTINCTION_TABLE[7]);
        // The vigour's remap: 0..1 from its range, held outside.
        assert_eq!((spread(NOISE_LO), spread(NOISE_HI), spread(0.0), spread(1.0)), (0.0, 1.0, 0.0, 1.0));
    }

    // --- CPU mirror of the march module's field (vc_hash, vc_noise2, vc_vigour, vc_column,
    // vc_shape's carve and vc_density's erosion), for the extinction
    // table and the vigour's tests. ---

    const M_SEED_TOP: u32 = 0x9e37_79b9;
    const M_SEED_TOP2: u32 = 0x7f4a_7c15;

    fn m_hash(x: u32, y: u32, z: u32) -> f32 {
        let mut h = x.wrapping_mul(0x7a3d_1c95) ^ y.wrapping_mul(0x4f1b_bcdd) ^ z.wrapping_mul(0x2b8f_3a61);
        h = (h ^ (h >> 15)).wrapping_mul(0x93c4_67e5);
        h = (h ^ (h >> 13)).wrapping_mul(0xd6e8_feb9);
        h ^= h >> 16;
        (h >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    fn m_noise2(cell: IVec2, f: Vec2, seed: u32) -> f32 {
        let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
        let (x0, x1, z0, z1) = (cell.x as u32, cell.x.wrapping_add(1) as u32, cell.y as u32, cell.y.wrapping_add(1) as u32);
        let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
        mix(mix(m_hash(x0, seed, z0), m_hash(x1, seed, z0), u.x), mix(m_hash(x0, seed, z1), m_hash(x1, seed, z1), u.x), u.y)
    }

    /// march `vc_vigour`: 0..1, for texture coordinates `uv` (world xz times the map), the
    /// first octave's `cells` a repeat, the offsets `off` and `off2` (`vigour_offset`).
    fn m_vigour(uv: Vec2, cells: f32, off: Vec4, off2: Vec4) -> f32 {
        let p = uv * cells + Vec2::new(off.x, off.y);
        let c1 = p.floor();
        let n1 = m_noise2(c1.as_ivec2() + IVec2::new(off.z as i32, off.w as i32), p - c1, M_SEED_TOP);
        let p2 = uv * cells * VIGOUR_OCTAVE + Vec2::new(off2.x, off2.y);
        let c2 = p2.floor();
        let n2 = m_noise2(c2.as_ivec2() + IVec2::new(off2.z as i32, off2.w as i32), p2 - c2, M_SEED_TOP2);
        spread(n1 * 0.6667 + n2 * 0.3333)
    }

    const M_SEED_PLACE: [u32; 3] = [0x51ed_270b, 0x2545_f491, 0x9e6c_63d1];

    /// The placement's three octaves without the lean model's shear and banks, at world (x, z) `xz` for a domain `domain` units tall, the footprint far
    /// under every octave's cell, under the drift `drift` (texture repeats).
    fn m_place(xz: Vec2, domain: f32, drift: [f64; 2]) -> f32 {
        let repeat = crate::cloud_shadows::REPEAT_UNITS;
        let cells = place_cells(domain, repeat);
        let freqs = [1.0, PLACE_OCTAVES[0], PLACE_OCTAVES[1]];
        let mut noises = [0.0f32; 3];
        for k in 0..3 {
            let c = cells * freqs[k];
            let (frac, whole) = cell_offset(drift, c);
            let p = xz / repeat * c + frac;
            let cell = p.floor();
            noises[k] = m_noise2(cell.as_ivec2().wrapping_add(whole), p - cell, M_SEED_PLACE[k]);
        }
        place_from(noises, [1.0; 3])
    }

    /// The extinction table's values: the ones committed at 44e923c33, solved by that commit's test
    /// `volume_extinction_table_is_the_fields` so that its carved density field reached [`target_opacity`]
    /// straight up; that field
    /// is gone, the numbers are kept as the unresolved sheet's and the light column's calibration. The
    /// uniform carries them.
    #[test]
    fn the_extinction_table_is_the_accepted_one() {
        assert_eq!(EXTINCTION_TABLE, [5.741, 3.775, 3.560, 2.732, 2.596, 2.709, 3.212, 4.344]);
        for shape in [None, Some(0.5), Some(f32::NAN)] {
            let u = uniform(&VolumeInputs { amount: 0.4, density: 1.0, shape_aid: shape, ..Default::default() });
            assert_eq!((u.cal_k0, u.cal_k1), (Vec4::from_slice(&EXTINCTION_TABLE[..4]), Vec4::from_slice(&EXTINCTION_TABLE[4..])));
            assert_eq!((u.cal_c0, u.cal_c1), (Vec4::from_slice(&CAL_COVERS[..4]), Vec4::from_slice(&CAL_COVERS[4..])));
        }
        for c in [0.0f32, 0.03, 0.3, 0.97, 1.0, f32::NAN] {
            assert_eq!(table_depth(c).to_bits(), table_depth_of(&EXTINCTION_TABLE, c).to_bits());
        }
    }

    /// The mirror is the shader's: the hash's multipliers, the octaves, the vigour, the lattices, the
    /// types, the bands' keep, the table and the steps.
    #[test]
    fn the_field_mirror_matches_the_march_module() {
        let src = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        for line in [
            "var h = (c.x * 0x7a3d1c95u) ^ (c.y * 0x4f1bbcddu) ^ (c.z * 0x2b8f3a61u);",
            "h = (h ^ (h >> 15u)) * 0x93c467e5u;",
            "h = (h ^ (h >> 13u)) * 0xd6e8feb9u;",
            "h = h ^ (h >> 16u);",
            "return f32(h >> 8u) * (1.0 / 16777216.0);",
            "const VC_SEED_TOP: u32 = 0x9e3779b9u;",
            "const VC_SEED_TOP2: u32 = 0x7f4a7c15u;",
            "let a = mix(corners.x, corners.y, u.x);",
            "let p = pos.xz * v.map.xy * v.height.x + v.vigour.xy;",
            "let n1 = vc_noise2(vec2<i32>(c1) + bitcast<vec2<i32>>(v.cells_v.xy), p - c1, VC_SEED_TOP);",
            "let p2 = pos.xz * v.map.xy * v.height.x * v.height.y + v.vigour2.xy;",
            "let n2 = vc_noise2(vec2<i32>(c2) + bitcast<vec2<i32>>(v.cells_v.zw), p2 - c2, VC_SEED_TOP2);",
            "return vc_spread(n1 * 0.6667 + n2 * 0.3333, v.tone);",
            // And an unresolved footprint's thin cover
            // (`unresolved_sheet`).
            "sheet = clamp(v.profile.y, 0.0, 1.0) * vc_profile(above / sheet_h, col.types, v);",
            "let local = smoothstep(v.tower.x, v.tower.y, vg) * smoothstep(v.tower.z, v.tower.w, over);",
            "let raw = vec3<f32>(v.mix.x, v.mix.y, v.mix.z * v.kind.y * local);",
            "return mix(k[i], k[i + 1u], x - f32(i));",
            "return w.x * vc_knots(h, v.knot_s0, v.knot_s1) + w.y * vc_knots(h, v.knot_c0, v.knot_c1) + w.z * vc_knots(h, v.knot_t0, v.knot_t1);",
            "let p = pos.xz * v.map.xy * cells + off;",
            "return vc_noise2(vec2<i32>(c) + whole, p - c, seed);",
            "let k0 = vc_octave_keep(v.field.x, width, v);",
            "let k1 = vc_octave_keep(v.field.x * VC_PLACE_F1, width, v);",
            "let k2 = vc_octave_keep(v.field.x * VC_PLACE_F2, width, v);",
            "return vec3<f32>(VC_PLACE_W0 * k0, VC_PLACE_W1 * k1, VC_PLACE_W2 * k2);",
            "let kept = length(k);",
            "let gain = length(vec3<f32>(VC_PLACE_W0, VC_PLACE_W1, VC_PLACE_W2)) / kept;",
            "col.edge = smoothstep(0.0, VC_MASS_EDGE, col.over);",
            "const VC_PLACE_F1: f32 = 2.17;",
            "const VC_PLACE_F2: f32 = 4.73;",
            "const VC_PLACE_W0: f32 = 0.40;",
            "const VC_PLACE_W1: f32 = 0.35;",
            "const VC_PLACE_W2: f32 = 0.25;",
            "const VC_SEED_PLACE0: u32 = 0x51ed270bu;",
            "const VC_SEED_PLACE1: u32 = 0x2545f491u;",
            "const VC_SEED_PLACE2: u32 = 0x9e6c63d1u;",
            "const VC_MASS_EDGE: f32 = 0.03;",
            "return 1.0 - smoothstep(0.25 * cell, 0.5 * cell, width);",
            // The surface's transition and the erosion's bands in the bound (`step_bound`).
            "let bound = min(min(min(exp2(lod) / max(v.nscale.x * v.nlevel.x, 1e-9), 0.5 * v.field.w), vc_m2_erosion_bound(spread * t0, v)), vc_m2_edge_bound(spread * t0));",
            "return clamp(ceil(max(t1 - t0, 0.0) / step_len), 1.0, 1.0e7);",
            "return vec3<f32>(pos.x * v.nscale.x + v.nphase.x, (yb - v.slab.x) * v.nscale.x, pos.z * v.nscale.x + v.nphase.y);",
            "return clamp(log2(max(width * size * inv_repeat, 1.0)), 0.0, last);",
            "let fade = smoothstep(0.0, max(v.profile.x, 1.0), above);",
            "return vc_table(c, v) / max(local, 1.0) * v.march.z;",
            "return cam.y + rel.y + dot(rel.xz, rel.xz) / (2.0 * max(v.reach.y, 1.0));",
            "return 1.0 - smoothstep(0.75 * r, r, t);",
            "return 1.0 - smoothstep(0.5 * cell, cell, width);",
            "const VC_LIGHT_GROWTH: f32 = 1.5;",
            "return reach * (VC_LIGHT_GROWTH - 1.0) / (pow(VC_LIGHT_GROWTH, f32(n)) - 1.0);",
            "let step_len = max(min(max(v.march.x, v.flags.w * spread * t0), bound), 1.0);",
        ] {
            assert!(src.contains(line), "the march module no longer has `{line}`: the mirror must follow");
        }
        assert_eq!((HORIZON_FADE_FROM, LIGHT_GROWTH), (0.75, 1.5), "the constants the march writes as literals");
        assert_eq!((PLACE_OCTAVES, PLACE_WEIGHTS), ([2.17, 4.73], [0.40, 0.35, 0.25]), "the placement's constants the march writes as literals");
        assert!(!src.contains("textureLoad") && !src.contains("textureDimensions"), "the march reads no texel of the cover since A3");
        for gone in ["fn vc_noise(", "fn vc_fbm2(", "fn vc_coverage(", "fn vc_erode(", "fn vc_detail_keep(", "fn vc_wrap("] {
            assert!(!src.contains(gone), "{gone}: stage 2a's lattice noise is gone (checkpoint B)");
        }
        let trace = include_str!("medium_trace.wesl").replace("\r\n", "\n");
        assert_eq!(trace.matches("    if view == 1u || view == 2u || view == 4u {").count(), 2, "both trace entries draw the base views and the noise");
    }

    /// The default uniform at the ground: plane 20,000 over the reference at y 0.
    fn ground_uniform(domain: f32, horizon: f32) -> CloudVolumeUniform {
        uniform(&VolumeInputs { plane_height: 20_000.0, above_ground: 20_000.0, domain, horizon, steps: VIEW_STEPS, density: 1.0, sky_scale: 1.0, ..Default::default() })
    }

    /// The shell: the base meets a horizontal ray from the ground at the horizon distance; the
    /// rays' stretches are ordered and within the range on a hit, with an explicit miss
    /// otherwise (t1 <= t0), never a number that is not one.
    #[test]
    fn the_bent_domain_spans_rays_from_overhead_to_the_horizon() {
        let u = ground_uniform(REFERENCE_HEIGHT, HORIZON);
        assert_eq!(u.reach.y, HORIZON * HORIZON / (2.0 * 20_000.0), "R = H² / 2 above");
        let range = u.slab.z;
        assert!((range - (2.0 * u.reach.y * 60_000.0).sqrt() * RANGE_MARGIN).abs() < 1.0);
        let hit = |s: (f32, f32)| s.1 > s.0;
        let at = |deg: f32| {
            let d = Vec3::new(deg.to_radians().cos(), deg.to_radians().sin(), 0.0);
            span(Vec3::ZERO, d, 0.0, crate::medium::RAY_END, &u)
        };
        // Straight up: the flat slab.
        let up = span(Vec3::ZERO, Vec3::Y, 0.0, crate::medium::RAY_END, &u);
        assert!((up.0 - 20_000.0).abs() < 1e-2 && (up.1 - 60_000.0).abs() < 1e-2, "{up:?}");
        // A horizontal ray meets the base at the horizon distance.
        let flat = at(0.0);
        assert!(hit(flat) && (flat.0 - HORIZON).abs() < 0.5, "{flat:?}");
        assert_eq!(horizon_fade(flat.0, HORIZON), 0.0, "faded out at the horizon");
        // One and two degrees up: the shell's entries (design values, ours with
        // R = 4,000,000 about 336,000 and 284,000).
        let one = at(1.0);
        let two = at(2.0);
        assert!(hit(one) && (one.0 - 336_283.0).abs() < 500.0, "{one:?}");
        assert!(hit(two) && (two.0 - 284_000.0).abs() < 1_500.0, "{two:?}");
        assert!(horizon_fade(one.0, HORIZON) > 0.5 && horizon_fade(two.0, HORIZON) == 1.0, "visible at 1 degree, whole at 2");
        // Entries fall as the ray rises; every hit ordered and in range.
        let mut last = f32::MAX;
        for k in 0..=90 {
            let s = at(k as f32);
            assert!(hit(s) && s.0 >= 0.0 && s.1 <= range && s.0 < last, "{k} degrees: {s:?}");
            last = s.0;
        }
        // Below the horizon the base is met beyond the horizon: nothing shows.
        let down = at(-1.0);
        assert!(!hit(down) || horizon_fade(down.0, HORIZON) == 0.0, "{down:?}");
        // From inside the domain: from the ray's start (or its t_min).
        let inside = span(Vec3::new(0.0, 40_000.0, 0.0), Vec3::new(1.0, 0.2, 0.0).normalize(), 0.0, crate::medium::RAY_END, &u);
        assert!(hit(inside) && inside.0 == 0.0, "{inside:?}");
        let later = span(Vec3::new(0.0, 40_000.0, 0.0), Vec3::new(1.0, 0.2, 0.0).normalize(), 500.0, crate::medium::RAY_END, &u);
        assert!(hit(later) && later.0 == 500.0, "{later:?}");
        // From above: looking up misses; straight down crosses the whole domain; a slant enters at
        // the top.
        let above = Vec3::new(0.0, 100_000.0, 0.0);
        assert!(!hit(span(above, Vec3::Y, 0.0, crate::medium::RAY_END, &u)));
        assert!(!hit(span(above, Vec3::X, 0.0, crate::medium::RAY_END, &u)), "a horizontal ray above the domain");
        let fall = span(above, -Vec3::Y, 0.0, crate::medium::RAY_END, &u);
        assert!((fall.0 - 40_000.0).abs() < 1e-2 && (fall.1 - 80_000.0).abs() < 1e-2, "{fall:?}");
        let slant = span(above, Vec3::new(1.0, -1.0, 0.0).normalize(), 0.0, crate::medium::RAY_END, &u);
        assert!(hit(slant) && slant.0 > 40_000.0 * 2f32.sqrt() * 0.99, "{slant:?}");
        // The medium's own stretch cuts it.
        let cut = span(Vec3::ZERO, Vec3::Y, 0.0, 30_000.0, &u);
        assert_eq!(cut, (20_000.0, 30_000.0));
        // Any origin and direction: finite, and a hit is ordered and in range.
        for i in 0..2_000 {
            let a = i as f32 * 2.399_963;
            let e = ((i * 37 % 181) as f32 - 90.0).to_radians();
            let d = Vec3::new(a.cos() * e.cos(), e.sin(), a.sin() * e.cos());
            let o = Vec3::new(((i * 13) % 7) as f32 * 1e5 - 3e5, ((i * 7) % 11) as f32 * 1e4 - 2e4, -4e5);
            let s = span(o, d, 0.0, crate::medium::RAY_END, &u);
            assert!(s.0.is_finite() && s.1.is_finite(), "{o} {d}: {s:?}");
            if hit(s) {
                assert!(s.0 >= 0.0 && s.1 <= range, "{o} {d}: {s:?}");
            }
        }
        // Three heights, and the aid's bounds: the top and the range follow.
        for domain in [24_000.0f32, 40_000.0, 64_000.0, 1.0, 1e9, f32::NAN] {
            let u = ground_uniform(domain, HORIZON);
            let s = span(Vec3::ZERO, Vec3::Y, 0.0, crate::medium::RAY_END, &u);
            assert!((s.1 - 20_000.0 - u.slab.y).abs() < 1e-2 && (MIN_DOMAIN..=MAX_DOMAIN).contains(&u.slab.y));
            assert!(u.slab.z.is_finite() && u.slab.z > HORIZON);
        }
        // The horizon aid: the base meets a horizontal ray where it says.
        for horizon in [MIN_HORIZON, 800_000.0, MAX_HORIZON] {
            let u = ground_uniform(REFERENCE_HEIGHT, horizon);
            let s = span(Vec3::ZERO, Vec3::X, 0.0, crate::medium::RAY_END, &u);
            assert!((s.0 - horizon).abs() < horizon * 1e-5, "{horizon}: {s:?}");
        }
        assert_eq!(shell_radius(f32::NAN, f32::NAN), shell_radius(HORIZON, crate::cloud_shadows::PLANE_ABOVE_GROUND));
        assert!(shell_radius(HORIZON, -5.0).is_finite() && shell_radius(HORIZON, 0.0) > 0.0, "a plane at or under the ground");
        // The quadratic's roots: cancellation-resistant and with an explicit miss.
        assert_eq!(roots(1.0, 0.0, 1.0), None);
        assert_eq!(roots(0.0, 0.0, 1.0), None);
        assert_eq!(roots(0.0, 2.0, -4.0), Some((2.0, 2.0)));
        let (lo, hi) = roots(1.25e-7, 1.0, -20_000.0).expect("roots");
        assert!((hi - 20_000.0).abs() < 60.0 && lo < 0.0, "{lo} {hi}");
    }

    /// #3032: the fade across the shell's tangency. A camera inside the domain whose
    /// ray slightly down just dips under the base (one unit below the height where the ray grazes
    /// it) has a near stretch and a far one entering just past the ray's lowest bent height; one
    /// unit above that height the ray clears the base and is split at its lowest point, so the far
    /// part is faded by its own entry there too, not by the camera's (before, it took fade 1 and a
    /// two-unit move showed a distant deck at once). The direction (1, -0.1, 0), whose
    /// lowest point lies beyond the horizon (fade 0 either side), and a direction whose lowest
    /// point lies within the fade (neither 0 nor 1).
    #[test]
    fn the_fade_is_continuous_across_the_shells_tangency() {
        let u = ground_uniform(REFERENCE_HEIGHT, HORIZON);
        let hit = |s: &(f32, f32)| s.1 > s.0;
        for (dir, within) in [(Vec3::new(1.0, -0.1, 0.0).normalize(), false), (Vec3::new(1.0, -0.085, 0.0).normalize(), true)] {
            let a = (dir.x * dir.x + dir.z * dir.z) / (2.0 * u.reach.y);
            let graze = u.slab.x + dir.y * dir.y / (4.0 * a);
            let low = -dir.y / (2.0 * a);
            let parts = |y: f32| -> Vec<(f32, f32)> { spans(Vec3::new(0.0, y, 0.0), dir, 0.0, crate::medium::RAY_END, &u).into_iter().filter(hit).map(|s| (s.0, horizon_fade(s.0, HORIZON))).collect() };
            let (below, above) = (parts(graze - 1.0), parts(graze + 1.0));
            println!("grazing at {graze:.0}, lowest point {low:.0}: one under {below:?}, one over {above:?}");
            assert!(graze > u.slab.x && graze < u.slab.x + u.slab.y, "the camera inside the domain");
            assert_eq!((below.len(), above.len()), (2, 2), "{below:?} {above:?}");
            assert!(below[0] == (0.0, 1.0) && above[0] == (0.0, 1.0), "the near part from the camera, whole");
            assert!((below[1].0 - low).abs() < 0.02 * low && (above[1].0 - low).abs() < 1e-3 * low, "the far part enters at about the lowest point");
            assert!((below[1].1 - above[1].1).abs() < 0.06, "the far part's fade either side: {} against {}", below[1].1, above[1].1);
            if within {
                assert!(above[1].1 > 0.3 && above[1].1 < 0.9, "{above:?}");
            } else {
                assert_eq!((below[1].1, above[1].1), (0.0, 0.0));
            }
        }
        // An upward ray from inside the domain is not split (its lowest point lies behind it).
        let up = spans(Vec3::new(0.0, 40_000.0, 0.0), Vec3::new(1.0, 0.2, 0.0).normalize(), 0.0, crate::medium::RAY_END, &u);
        assert!(!hit(&up[0]) && hit(&up[1]) && up[1].0 == 0.0, "{up:?}");
        let src = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        assert!(src.contains("let mid = min(max(-b / (2.0 * a), lo), hi);\n            s = vec4<f32>(lo, mid, mid, hi);"));
    }

    /// #3032: a camera crossing the base. One unit below it a ray slightly down
    /// enters the base beyond the horizon (faded out); one unit above it the ray leaves the domain
    /// at once and enters it again at about the same far distance: that second stretch takes its
    /// own fade (none), not the first one's (whole). Also with a nonzero `t_min`, and from above
    /// the domain looking down and level.
    #[test]
    fn a_far_stretch_takes_its_own_fade() {
        let u = ground_uniform(REFERENCE_HEIGHT, HORIZON);
        let dir = Vec3::new(1.0, -0.06, 0.0).normalize();
        let below = spans(Vec3::new(0.0, 19_999.0, 0.0), dir, 0.0, crate::medium::RAY_END, &u);
        let above = spans(Vec3::new(0.0, 20_001.0, 0.0), dir, 0.0, crate::medium::RAY_END, &u);
        let hit = |s: (f32, f32)| s.1 > s.0;
        assert!(!hit(below[0]) && hit(below[1]) && below[1].0 > HORIZON, "{below:?}");
        assert!(hit(above[0]) && above[0].0 == 0.0 && above[0].1 < 100.0, "the near sliver: {above:?}");
        assert!(hit(above[1]) && (above[1].0 - below[1].0).abs() < 100.0, "the far stretch: {above:?} against {below:?}");
        // Each stretch faded by its own entry: the far deck stays faded out across the crossing.
        let faded = |s: [(f32, f32); 2]| -> f32 { s.iter().filter(|x| hit(**x)).map(|x| horizon_fade(x.0, HORIZON)).fold(0.0, f32::max) };
        assert_eq!(horizon_fade(above[1].0, HORIZON), 0.0);
        assert!(faded(below) == 0.0 && horizon_fade(above[0].0, HORIZON) == 1.0);
        // With t_min the stretches are clipped, each on its own.
        let clipped = spans(Vec3::new(0.0, 20_001.0, 0.0), dir, 50.0, crate::medium::RAY_END, &u);
        assert!(clipped[0].0 == 50.0 && clipped[1].0 == above[1].0, "{clipped:?}");
        // From above the domain: looking down at a slant, two stretches (through the top and out
        // the base, then back in far away); level, none.
        let high = Vec3::new(0.0, 100_000.0, 0.0);
        let down = spans(high, Vec3::new(1.0, -0.3, 0.0).normalize(), 0.0, crate::medium::RAY_END, &u);
        assert!(hit(down[0]) && down[0].0 > 0.0, "{down:?}");
        assert!(down[1].1 <= u.slab.z);
        let level = spans(high, Vec3::X, 0.0, crate::medium::RAY_END, &u);
        assert!(!hit(level[0]) && !hit(level[1]), "{level:?}");
        // Ordered: the second stretch never before the first.
        for i in 0..400 {
            let a = i as f32 * 0.7;
            let d = Vec3::new(a.cos(), -0.2 + 0.4 * ((i * 13 % 97) as f32 / 97.0), a.sin()).normalize();
            let o = Vec3::new(0.0, 10_000.0 + (i % 9) as f32 * 10_000.0, 0.0);
            let s = spans(o, d, 0.0, crate::medium::RAY_END, &u);
            if hit(s[0]) && hit(s[1]) {
                assert!(s[1].0 >= s[0].1, "{o} {d}: {s:?}");
            }
        }
        let src = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        assert!(src.contains("s = vec4<f32>(lo, min(hi, rb.y), max(rb.z, lo), hi);"));
        assert!(src.contains("let fade = vc_horizon_fade(t0, v.slab.w);"));
    }

    /// The march's real stretches and steps, with a made-up density that is
    /// empty near the camera and a deck far off: a ray from just above the base that dips under
    /// it marches the near sliver (finding nothing) and then the far stretch with its own steps
    /// and fade; with the far stretch inside the horizon the deck shows, beyond it it does not;
    /// every sample lies in a stretch, none behind the ray's start, and the steps cover each
    /// stretch exactly.
    #[test]
    fn the_march_covers_its_real_stretches() {
        let u = ground_uniform(REFERENCE_HEIGHT, 900_000.0);
        let spread = 2.0 * (59.84f32.to_radians() * 0.5).tan() / 720.0 * 2.0;
        // The dense reference's sample distances and their stretch's fade.
        let samples = |o: Vec3, d: Vec3, t_min: f32, dither: f32| -> Vec<(f32, f32, f32)> {
            let mut out = Vec::new();
            for (t0, t1) in spans(o, d, t_min, crate::medium::RAY_END, &u) {
                if t1 <= t0 {
                    continue;
                }
                let fade = horizon_fade(t0, u.slab.w);
                let (n, dt) = step_plan(t0, t1, spread, &u);
                assert!(((n as f32) * dt - (t1 - t0)).abs() <= 1e-3 * (t1 - t0).max(1.0), "the steps cover the stretch");
                for i in 0..n {
                    out.push((t0 + (i as f32 + dither) * dt, dt, fade));
                }
            }
            out
        };
        // A deck from 300,000 units on (the far stretch), nothing nearer.
        let deck = |t: f32| if t > 300_000.0 { 2e-5 } else { 0.0 };
        let shown = |o: Vec3, d: Vec3, t_min: f32| -> f32 {
            let mut trans = 1.0f32;
            let mut stretch_trans = 1.0f32;
            let mut last_fade = None;
            for (t, dt, fade) in samples(o, d, t_min, 0.5) {
                if last_fade != Some(fade) {
                    trans *= 1.0 - (1.0 - stretch_trans) * last_fade.unwrap_or(0.0);
                    stretch_trans = 1.0;
                    last_fade = Some(fade);
                }
                assert!(t >= t_min, "no sample behind the ray's start");
                stretch_trans *= (-deck(t) * dt).exp();
            }
            trans *= 1.0 - (1.0 - stretch_trans) * last_fade.unwrap_or(0.0);
            1.0 - trans
        };
        // With the horizon at 900,000 the shell's radius is 20,250,000: a ray dipping by 0.01 from
        // half a unit above the base leaves after 50 units and meets the shell again about
        // 405,000 units out, inside the fade's start (0.75 of the horizon). The steeper ray
        // (0.06 down) meets it again beyond the horizon at either setting.
        let dir = Vec3::new(1.0, -0.01, 0.0).normalize();
        let above = Vec3::new(0.0, 20_000.5, 0.0);
        let s = spans(above, dir, 0.0, crate::medium::RAY_END, &u);
        assert!(s[0].1 < 100.0 && s[1].0 > 300_000.0, "a near sliver, a far stretch: {s:?}");
        let opacity = shown(above, dir, 0.0);
        assert!(opacity > 0.5, "the far deck shows through its own stretch (horizon 900,000): {opacity}");
        assert!((shown(above, dir, 50.0) - opacity).abs() < 1e-3, "a nonzero t_min changes nothing past it");
        let near_horizon = ground_uniform(REFERENCE_HEIGHT, HORIZON);
        let steep = Vec3::new(1.0, -0.06, 0.0).normalize();
        let far = spans(Vec3::new(0.0, 20_001.0, 0.0), steep, 0.0, crate::medium::RAY_END, &near_horizon);
        assert_eq!(horizon_fade(far[1].0, near_horizon.slab.w), 0.0, "with the default horizon that stretch is faded out");
    }

    /// The fade follows the entry distance from 0.75 of the horizon to all of it.
    #[test]
    fn volume_fades_into_the_dome_toward_the_horizon() {
        assert_eq!(horizon_fade(0.0, HORIZON), 1.0);
        assert_eq!(horizon_fade(HORIZON_FADE_FROM * HORIZON, HORIZON), 1.0);
        assert_eq!(horizon_fade(HORIZON, HORIZON), 0.0);
        assert_eq!(horizon_fade(f32::INFINITY, HORIZON), 0.0);
        assert_eq!(horizon_fade(f32::NAN, HORIZON), 0.0);
        let mut last = 1.0;
        for i in 0..=100 {
            let f = horizon_fade(i as f32 / 100.0 * HORIZON * 1.2, HORIZON);
            assert!(f <= last + 1e-6 && (0.0..=1.0).contains(&f));
            last = f;
        }
    }

    /// The light ray reaches the domain's top or its longest reach, over growing intervals that
    /// sum to it; the view's steps are the near step, longer far off, never more than the cap.
    #[test]
    fn the_light_and_view_steps_cover_their_stretches() {
        let u = ground_uniform(REFERENCE_HEIGHT, HORIZON);
        // Straight up from low in a cloud: the longest reach; from high: to the top.
        assert_eq!(light_reach(Vec3::new(0.0, 21_000.0, 0.0), 0.0, Vec3::Y, &u), LIGHT_REACH);
        assert!((light_reach(Vec3::new(0.0, 50_000.0, 0.0), 0.0, Vec3::Y, &u) - 10_000.0).abs() < 1.0);
        // A low sun: the longest reach (the bent top is far along it).
        let low_sun = Vec3::new(1.0, 0.02, 0.0).normalize();
        assert_eq!(light_reach(Vec3::new(50_000.0, 25_000.0, 0.0), 0.0, low_sun, &u), LIGHT_REACH);
        for reach in [500.0f32, 10_000.0, LIGHT_REACH] {
            let ds = light_intervals(reach, LIGHT_STEPS);
            assert_eq!(ds.len(), LIGHT_STEPS as usize);
            assert!((ds.iter().sum::<f32>() - reach).abs() < reach * 1e-4);
            assert!(ds.windows(2).all(|w| w[1] > w[0]), "shorter near the sample");
        }
        assert!((light_intervals(LIGHT_REACH, LIGHT_STEPS)[0] - 488.0).abs() < 2.0, "the first tap's interval");
        // The view's steps: 400 units near; the footprint's two widths far; the cap.
        let spread = 2.0 * (59.84f32.to_radians() * 0.5).tan() / 720.0 * 2.0;
        // Near the camera half the surface's narrowest transition
        // (the cumulus's 240 units) bounds the step below the near step (400) while budget is left.
        let edge = crate::volumetric_clouds_model2::edge_bound(0.0);
        assert_eq!(edge, 120.0);
        let (n, dt) = step_plan(20_000.0, 60_000.0, spread, &u);
        assert_eq!((n, dt), (334, 40_000.0 / 334.0), "straight up");
        // #3032 stage 2b: at most a base texel at the entry's level, at most half a sheet.
        // The ray's view steps are held to the budget's share.
        let most = ray_steps_most(&u);
        assert_eq!((most, RAY_BUDGET), (2_785.0, 25_088));
        let most = most as u32;
        let (n, dt) = step_plan(284_000.0, 567_000.0, spread, &u);
        assert!(n < most && (0.5 * erode_cells()[2] - 1.0..=0.5 * SHEET + 1.0).contains(&dt), "2 degrees: {n} of {dt}");
        let (n, dt) = step_plan(320_000.0, 700_000.0, spread, &u);
        assert!(n < most && dt <= 0.5 * SHEET + 1.0, "far: {n} of {dt}");
        // Near, half the surface's narrowest transition bounds the step (below
        // the near step too), the erosion on or off; a little further the finest erosion band
        // kept (500 units, halved, over the wisps' stretch); far off (every band gone, the transition
        // two cast widths) the base texel and half a sheet.
        assert!((step_bound(0.0, &u) - edge).abs() < 1e-3 && step_bound(1e9, &u) <= 0.5 * SHEET);
        assert!((crate::volumetric_clouds_model2::edge_bound(1_000.0) - 1_000.0).abs() < 1e-3, "two cast widths, halved");
        let band = 250.0 / (1.0 + crate::volumetric_clouds_wisp::WISP_AMP * crate::volumetric_clouds_model2::WISP_GRAD);
        assert!((step_bound(200.0, &u) - band).abs() < 1e-2 && band > edge, "{band}");
        let mut off = u;
        off.nscale.w = 0.0;
        assert!((step_bound(0.0, &off) - edge).abs() < 1e-3 && (step_bound(200.0, &off) - 200.0).abs() < 1e-3, "no band, the transition stays");
        assert!((most - 1..=most).contains(&step_plan(0.0, 1e9, spread, &u).0), "held to the budget");
        assert_eq!(step_plan(10.0, 10.0, spread, &u).0, 1, "an empty stretch: one step");
        // The count follows the spacing bound below the budget: a ray 10 degrees up
        // from the ground steps at most the bound at its entry. The level ray from 1,000
        // units over the default base (about 558,000 units of stretch entered at the camera, where
        // the bound is 120 units) asks more than the budget and is capped, its steps wider.
        let ten = spans(Vec3::ZERO, Vec3::new(10.0f32.to_radians().cos(), 10.0f32.to_radians().sin(), 0.0), 0.0, crate::medium::RAY_END, &u);
        let (plan, capped) = ray_plan(ten, spread, &u);
        assert!(!capped && plan[0] + plan[1] < most, "{plan:?}");
        for (t0, t1) in ten.into_iter().filter(|s| s.1 > s.0) {
            let (n, dt) = step_plan(t0, t1, spread, &u);
            assert!(n < most && dt <= step_bound(spread * t0, &u).max(u.march.x) + 1e-3, "{t0}..{t1}: {n} steps of {dt}");
        }
        let level = spans(Vec3::new(0.0, 21_000.0, 0.0), Vec3::X, 0.0, crate::medium::RAY_END, &u);
        let long = level.iter().map(|s| s.1 - s.0).fold(0.0, f32::max);
        assert!(long > 500_000.0, "{level:?}");
        let (plan, capped) = ray_plan(level, spread, &u);
        assert!(capped && plan[0] + plan[1] <= most, "{plan:?}");
        // At the defaults no camera on the ground, below it or well above the domain is capped
        // (a numerical mirror: at most 915 steps from the ground, 1,544 from 10,000, 814 from
        // 100,000); one near or inside the domain is, and the probes find it
        // (3,772 steps 1.3 degrees up from 19,000, 4,956 0.5 degrees down from 21,000, 3,841 5.6
        // degrees down from 45,000, 3,648 8.9 degrees down from 70,000, just over the domain's top).
        for y in [0.0f32, 10_000.0, 100_000.0] {
            assert!(!rays_capped(Vec3::new(0.0, y, 0.0), spread, &u), "the defaults from {y}");
        }
        for y in [19_000.0f32, 21_000.0, 45_000.0, 70_000.0] {
            assert!(rays_capped(Vec3::new(0.0, y, 0.0), spread, &u), "capped from {y}");
        }
        // The settings that ask the most (quality 128, an 80,000-unit domain, the reference mode
        // at scale 1: one ray per pixel of a 1440-pixel-tall render) are held to the budget: no ray
        // asks for more than RAY_BUDGET evaluations (each view step with all its light taps), its
        // steps wider than the bound, and the medium is told (`rays_capped`).
        let reference = 2.0 * (59.84f32.to_radians() * 0.5).tan() / 1440.0;
        let most_u = uniform(&VolumeInputs { plane_height: 20_000.0, above_ground: 20_000.0, domain: MAX_DOMAIN, horizon: HORIZON, steps: MAX_STEPS, density: 1.0, sky_scale: 1.0, ..Default::default() });
        let mut any_capped = false;
        for y in [0.0f32, 21_000.0, 60_000.0, 120_000.0] {
            for deg in [-10.0f32, -1.0, 0.0, 0.5, 1.0, 2.0, 5.0, 30.0, 90.0] {
                let (s, c) = deg.to_radians().sin_cos();
                let stretches = spans(Vec3::new(0.0, y, 0.0), Vec3::new(c, s, 0.0), 0.0, crate::medium::RAY_END, &most_u);
                let (plan, capped) = ray_plan(stretches, reference, &most_u);
                assert!((plan[0] + plan[1]) * (1 + LIGHT_STEPS) <= RAY_BUDGET, "from {y} at {deg} degrees: {plan:?}");
                if capped {
                    any_capped = true;
                    let (k, (t0, t1)) = stretches.iter().copied().enumerate().max_by(|a, b| (a.1 .1 - a.1 .0).total_cmp(&(b.1 .1 - b.1 .0))).expect("two");
                    assert!((plan[k] as f32) < stretch_steps(t0, t1, reference, &most_u), "from {y} at {deg} degrees: capped, fewer and wider steps than the bound asks");
                }
            }
        }
        assert!(any_capped && rays_capped(Vec3::new(0.0, 21_000.0, 0.0), reference, &most_u), "a level ray inside the tall domain at quality 128 is capped");
        let widest = ground_uniform(MAX_DOMAIN, MAX_HORIZON);
        let (n, dt) = step_plan(0.0, widest.slab.z, spread, &widest);
        assert!(widest.slab.z > 3.4e6 && (most - 1..=most).contains(&n) && dt > step_bound(0.0, &widest), "the longest stretch: {} units, {n} steps of {dt}", widest.slab.z);
        // NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_STEPS scales the near step.
        assert_eq!(step_near(VIEW_STEPS), STEP_NEAR);
        assert_eq!(step_near(56), STEP_NEAR / 2.0);
        assert_eq!(step_near(0), step_near(MIN_STEPS));
    }

    /// The footprint (the ray's spread times the distance, one cast width) fades each erosion band
    /// toward its mean from four cast widths a cell to none at two:
    /// at the default buffer of a 2560x1440 view (1280x720, tiles of
    /// 2) the 1,000-unit band is whole to about 78,000 units and gone from about 156,000, the
    /// 2,000-unit band 156,000 and 313,000, the 4,000-unit band 313,000 and 626,000; and the
    /// placement's octaves by their own rule.
    #[test]
    fn the_footprint_fades_the_erosion() {
        let spread = 2.0 * (59.84f32.to_radians() * 0.5).tan() / 720.0 * 2.0;
        let cells = erode_cells();
        assert_eq!(cells, [4_000.0, 2_000.0, 1_000.0]);
        for (k, want) in [(2usize, 78_200.0f32), (1, 156_400.0), (0, 312_800.0)] {
            let (whole, gone) = (0.25 * cells[k] / spread, 0.5 * cells[k] / spread);
            assert!((whole - want).abs() < 0.01 * want && (gone - 2.0 * want).abs() < 0.02 * want, "band {k}: {whole} {gone}");
            assert!(band_keep(cells[k], spread * whole) > 0.9999 && band_keep(cells[k], spread * gone) < 1e-4 && band_keep(cells[k], 0.0) == 1.0);
        }
        let mut last = [1.0f32; 3];
        for i in 0..80 {
            for (k, l) in last.iter_mut().enumerate() {
                let keep = band_keep(cells[k], spread * i as f32 * 10_000.0);
                assert!(keep <= *l + 1e-6 && (0.0..=1.0).contains(&keep));
                *l = keep;
            }
        }
        // The placement's finest octave (40,000 / 4.73 units at the default) is whole out to the
        // source's range at this buffer (its cell spans two footprints out to about 1.3 million
        // units), but fades within the range at scale 4 or at a domain of 24,000; the first octave
        // never does (it would need a footprint of 20,000 units).
        let finest = REFERENCE_HEIGHT * PLACE_CELL_PER_DOMAIN / PLACE_OCTAVES[1];
        assert_eq!(octave_keep(spread * 727_000.0, finest), 1.0);
        assert_eq!(octave_keep(finest, finest), 0.0, "a footprint as wide as the cell: none");
        let scale4 = 2.0 * (59.84f32.to_radians() * 0.5).tan() / 360.0 * 2.0;
        assert!(octave_keep(scale4 * 727_000.0, finest) < 1.0, "scale 4: the finest octave fades within the range");
        assert_eq!(octave_keep(scale4 * 727_000.0, REFERENCE_HEIGHT * PLACE_CELL_PER_DOMAIN), 1.0);
        // As octaves fade the field's spread is restored, so the share of sky stays the amount's.
        let mut values = [Vec::new(), Vec::new(), Vec::new()];
        for i in 0..500 {
            for j in 0..500 {
                let xz = Vec2::new(i as f32, j as f32) * 4_000.0 + Vec2::new(3.0e5, -9.0e5);
                let repeat = crate::cloud_shadows::REPEAT_UNITS;
                let cells = place_cells(REFERENCE_HEIGHT, repeat);
                let n: [f32; 3] = std::array::from_fn(|k| {
                    let c = cells * [1.0, PLACE_OCTAVES[0], PLACE_OCTAVES[1]][k];
                    let p = xz / repeat * c;
                    let cell = p.floor();
                    m_noise2(cell.as_ivec2(), p - cell, M_SEED_PLACE[k])
                });
                values[0].push(place_from(n, [1.0, 1.0, 1.0]));
                values[1].push(place_from(n, [1.0, 1.0, 0.0]));
                values[2].push(place_from(n, [1.0, 0.5, 0.0]));
            }
        }
        for a in [0.1f32, 0.3, 0.5, 0.8] {
            let t = place_threshold(a);
            let shares: Vec<f32> = values.iter().map(|v| v.iter().filter(|&&f| f > t).count() as f32 / v.len() as f32).collect();
            println!("amount {a}: share whole {:.3}, the finest faded {:.3}, two faded {:.3} (aim {:.3})", shares[0], shares[1], shares[2], sky_share(a));
            assert!((shares[1] - shares[0]).abs() < 0.03 && (shares[2] - shares[0]).abs() < 0.04, "amount {a}: {shares:?}");
        }
        assert_eq!(place_from([0.9; 3], [0.0; 3]), 0.5, "nothing kept: the mean");
    }

    /// #3032: the whole cells wrap as the lattice's integers, so a drift past 2^24
    /// cells (where a float's whole numbers end) moves the field on smoothly, either sign: the
    /// field at a drift and a drift a hundredth of a cell further differ by a little, across the
    /// old limit too.
    #[test]
    fn the_drift_has_no_seam_beyond_two_to_the_twenty_fourth() {
        let cells = 1.0f32;
        let field = |d: f64, x: f32| -> f32 {
            let (frac, whole) = cell_offset([d, 0.0], cells);
            let p = Vec2::new(x, 0.3) + frac;
            let c = p.floor();
            m_noise2(c.as_ivec2().wrapping_add(whole), p - c, M_SEED_PLACE[0])
        };
        let limit = f64::from(1u32 << 24);
        for d0 in [limit - 0.5, -limit - 0.5, limit * 200.0 - 0.5, -3.0e9 - 0.5] {
            let mut last = field(d0, 0.2);
            for k in 1..=100 {
                let now = field(d0 + k as f64 * 0.01, 0.2);
                assert!((now - last).abs() < 0.05, "drift {}: {last} to {now}", d0 + k as f64 * 0.01);
                last = now;
            }
        }
        // The whole cells keep counting past the limit (no clamp).
        let (_, a) = cell_offset([limit + 3.5, -limit - 3.5], cells);
        assert_eq!((a.x as i64, a.y as i64), (limit as i64 + 3, -(limit as i64) - 4));
        assert_eq!(cell_offset([f64::NAN, f64::INFINITY], cells), (Vec2::ZERO, IVec2::ZERO));
    }

    /// #3032: the placement's footprint at grazing angles is the distance between
    /// neighbouring rays where they meet the bent base: at 1 degree up from the ground (the base
    /// about 336,000 units off) about ten times the transverse width (about 10,600 units at the
    /// default 2560x1440 buffer, more than the finest placement cell, which then fades), straight
    /// up the transverse width itself.
    #[test]
    fn the_placement_footprint_follows_the_shell_at_grazing_angles() {
        let u = ground_uniform(REFERENCE_HEIGHT, HORIZON);
        let spread = 2.0 * (59.84f32.to_radians() * 0.5).tan() / 720.0 * 2.0;
        let dir = Vec3::new(1.0f32.to_radians().cos(), 1.0f32.to_radians().sin(), 0.0);
        let t = span(Vec3::ZERO, dir, 0.0, crate::medium::RAY_END, &u).0;
        let w = place_width(dir * t, dir, spread * t, u.reach.y);
        println!("1 degree: transverse {:.0}, on the shell {w:.0}", spread * t);
        assert!((w - 10_600.0).abs() < 800.0, "{w}");
        let finest = REFERENCE_HEIGHT * PLACE_CELL_PER_DOMAIN / PLACE_OCTAVES[1];
        assert!(octave_keep(w, finest) < 1.0, "the finest placement octave fades there");
        let up = place_width(Vec3::new(0.0, 20_000.0, 0.0), Vec3::Y, 64.0, u.reach.y);
        assert!((up - 64.0).abs() < 1e-3);
        assert!(place_width(Vec3::new(4e5, 0.0, 0.0), Vec3::X, 100.0, u.reach.y).is_finite());
    }

    /// #3032: a footprint wider than every placement octave (scale 4, 1 degree up
    /// from the ground at 1280x720: about 42,500 units on the shell, past the first octave's
    /// 40,000-unit cell) leaves the field at its mean everywhere, an empty sky under a thin
    /// weather's threshold and a full one under a heavy weather's; the sample then takes the
    /// expected cover of an unresolved footprint, the requested share as a uniform thin cover,
    /// whatever the field (volumetric_clouds_model2.rs `shape` blends it by the resolution).
    #[test]
    fn an_unresolved_footprint_keeps_the_requested_share() {
        let u = ground_uniform(REFERENCE_HEIGHT, HORIZON);
        let spread = 2.0 * (59.84f32.to_radians() * 0.5).tan() / 180.0 * 2.0;
        let dir = Vec3::new(1.0f32.to_radians().cos(), 1.0f32.to_radians().sin(), 0.0);
        let t = span(Vec3::ZERO, dir, 0.0, crate::medium::RAY_END, &u).0;
        let w = place_width(dir * t, dir, spread * t, u.reach.y);
        let first = REFERENCE_HEIGHT * PLACE_CELL_PER_DOMAIN;
        println!("scale 4, 1 degree: entry {t:.0}, footprint on the shell {w:.0}");
        assert!(w > first && horizon_fade(t, HORIZON) > 0.5, "{w}: past the first octave's cell, still shown");
        let res = octave_keep(w, first);
        assert_eq!(res, 0.0);
        assert_eq!(place_from([0.9, 0.1, 0.7], [0.0; 3]), 0.5, "the field at its mean");
        assert!(place_threshold(0.2) > 0.5 && place_threshold(0.8) < 0.5, "empty under a thin weather, full under a heavy one, before");
        // The thin cover is the share times the profile, within the sheet only.
        assert_eq!(unresolved_sheet(0.51, 600.0, Vec3::Y), 0.51 * profile_at(0.5, Vec3::Y));
        assert_eq!((unresolved_sheet(0.51, 0.0, Vec3::Y), unresolved_sheet(0.51, SHEET, Vec3::Y)), (0.0, 0.0), "within the sheet only");
        // The uniform carries the share the march blends toward.
        let i = VolumeInputs { plane_height: 20_000.0, above_ground: 20_000.0, domain: REFERENCE_HEIGHT, horizon: HORIZON, steps: VIEW_STEPS, density: 1.0, sky_scale: 1.0, amount: 0.3, ..Default::default() };
        assert_eq!(uniform(&i).profile.y, sky_share(0.3));
    }

    /// The vigour: 0..1, not repeating with the cover's repeat, moving with the drift (a place
    /// keeps its height while it drifts) and without a seam where the drift passes whole cells,
    /// at negative and large world positions too; its scale of the dome mostly under 1, a few
    /// places twice.
    #[test]
    fn the_vigour_field_moves_with_the_drift_and_does_not_repeat() {
        let cells = vigour_cells(REFERENCE_HEIGHT, crate::cloud_shadows::REPEAT_UNITS);
        assert!((cells - 0.5).abs() < 1e-6, "a cell of three domain heights: {cells}");
        let mut scales = Vec::new();
        let mut differ = 0.0;
        for i in 0..4_000 {
            let uv = Vec2::new((i % 63) as f32 * 0.537 - 14.0, (i / 63) as f32 * 0.811 - 27.0);
            let v = m_vigour(uv, cells, Vec4::ZERO, Vec4::ZERO);
            assert!((0.0..=1.0).contains(&v));
            let s = vigour_scale(v);
            assert!((VIGOUR_LOW..=VIGOUR_HIGH).contains(&s));
            scales.push(s);
            differ += (m_vigour(uv + Vec2::X, cells, Vec4::ZERO, Vec4::ZERO) - v).abs();
        }
        assert!(differ / 4_000.0 > 0.05, "one cover repeat over, another vigour: {}", differ / 4_000.0);
        scales.sort_by(f32::total_cmp);
        let median = scales[scales.len() / 2];
        let towers = scales.iter().filter(|s| **s > 1.15).count() as f32 / scales.len() as f32;
        println!("vigour: median scale {median:.3}, over 1.15 {towers:.3}");
        assert!(median < 1.1, "most places at or near their dome: median {median}");
        assert!(towers > 0.005 && towers < 0.4, "a few places over it: {towers}");
        // Moves with the drift: a place moved by the drift has the vigour the place had; each
        // octave's offset (whole cells and a fraction) gives the same field across a cell's
        // edge, either way.
        for d in [0.0f64, 0.3699, 0.3705, 1.0 / f64::from(cells), -0.25, -3.7, 123.456] {
            let (off, off2) = (vigour_offset_whole([d, -d], cells), vigour_offset_whole([d, -d], cells * VIGOUR_OCTAVE));
            assert!(off.is_finite() && off.x >= 0.0 && off.x < 1.0 && off.z.fract() == 0.0 && off2.w.fract() == 0.0);
            for uv in [Vec2::new(0.1, 0.2), Vec2::new(-5.3, 7.9), Vec2::new(25.0, -25.0)] {
                let moved = m_vigour(uv, cells, off, off2);
                let ideal = m_vigour(uv + Vec2::new(d as f32, -d as f32), cells, Vec4::ZERO, Vec4::ZERO);
                assert!((moved - ideal).abs() < 2e-3, "drift {d} at {uv}: {moved} against {ideal}");
            }
        }
        // Not a number: no offset.
        assert_eq!(vigour_offset([f64::NAN, f64::INFINITY], cells), Vec4::ZERO);
        assert_eq!(vigour_offset([1.0, 1.0], f32::NAN), Vec4::ZERO);
        assert!(vigour_cells(f32::NAN, f32::NAN).is_finite());
    }

    /// #3032 stage 2a, A3: the placement. The share of the plane where the field passes the
    /// threshold follows the weather's amount over a large area (54 first-octave cells a side);
    /// the threshold falls as the amount rises, none at amount 0, a closed deck at 1, not a
    /// number as none; a fade between none and a deck moves the amount, so the share, smoothly.
    #[test]
    fn the_placement_field_follows_its_distribution() {
        assert_eq!(place_threshold(0.0), 1.0);
        assert_eq!(place_threshold(1.0), 0.0);
        assert_eq!(place_threshold(f32::NAN), 1.0, "not a number: none");
        assert_eq!((sky_share(0.0), sky_share(1.0)), (0.0, 1.0));
        let mut last = 2.0;
        for k in 0..=100 {
            let a = k as f32 / 100.0;
            let t = place_threshold(a);
            assert!(t <= last && (0.0..=1.0).contains(&t), "amount {a}: {t}");
            assert!(sky_share(a) + 1e-6 >= target_opacity(a), "amount {a}: the share holds the opacity aimed at");
            last = t;
        }
        assert!(PLACE_SHARES.windows(2).all(|w| w[1] > w[0]) && PLACE_THRESHOLDS.windows(2).all(|w| w[1] < w[0]));
        // The measured share over a large area against the share aimed at.
        let mut values = Vec::with_capacity(720 * 720);
        for i in 0..720 {
            for j in 0..720 {
                let xz = Vec2::new(i as f32, j as f32) * 3_000.0 + Vec2::new(-1.1e6, 4.4e5);
                let f = m_place(xz, REFERENCE_HEIGHT, [0.0; 2]);
                assert!((0.0..=1.0).contains(&f));
                values.push(f);
            }
        }
        for a in [0.004f32, 0.01, 0.05, 0.1, 0.2, 0.5, 0.9] {
            let t = place_threshold(a);
            let got = values.iter().filter(|&&f| f > t).count() as f32 / values.len() as f32;
            println!("amount {a}: sky share {got:.4} against {:.4}", sky_share(a));
            // Thin weather: within 0.4 of its own share (its opacity aimed at is
            // smaller than the share), else within 0.04.
            let tolerance = if a < 0.05 { 0.4 * sky_share(a) } else { 0.04 };
            assert!((got - sky_share(a)).abs() < tolerance, "amount {a}: {got} against {}", sky_share(a));
        }
        assert!(values.iter().all(|&f| f > place_threshold(1.0)), "amount 1: a closed deck");
        assert!(values.iter().all(|&f| f <= place_threshold(0.0)), "amount 0: none");
    }

    /// The placement field does not repeat with the cover's repeat or with its own cells, moves
    /// with the drift per octave (a mass keeps its place while it travels) without a seam where
    /// the drift passes whole cells.
    #[test]
    fn the_placement_moves_with_the_drift_and_does_not_repeat() {
        let repeat = crate::cloud_shadows::REPEAT_UNITS;
        let mut by_repeat = 0.0;
        let mut by_cell = 0.0;
        for i in 0..2_000 {
            let xz = Vec2::new((i % 47) as f32 * 7_919.0 - 2.0e5, (i / 47) as f32 * 6_133.0 + 9.0e4);
            let f = m_place(xz, REFERENCE_HEIGHT, [0.0; 2]);
            by_repeat += (m_place(xz + Vec2::X * repeat, REFERENCE_HEIGHT, [0.0; 2]) - f).abs();
            by_cell += (m_place(xz + Vec2::Y * REFERENCE_HEIGHT * PLACE_CELL_PER_DOMAIN * 5.0, REFERENCE_HEIGHT, [0.0; 2]) - f).abs();
        }
        assert!(by_repeat / 2_000.0 > 0.05 && by_cell / 2_000.0 > 0.05, "{} {}", by_repeat / 2_000.0, by_cell / 2_000.0);
        for d in [0.0f64, 0.3333, 0.6667, 1.0, -0.25, -3.7, 123.456] {
            for xz in [Vec2::new(1_000.0, 2_000.0), Vec2::new(-3.1e5, 4.7e5)] {
                let moved = m_place(xz, REFERENCE_HEIGHT, [d, -d]);
                let ideal = m_place(xz + Vec2::new(d as f32, -d as f32) * repeat, REFERENCE_HEIGHT, [0.0; 2]);
                assert!((moved - ideal).abs() < 2e-3, "drift {d} at {xz}: {moved} against {ideal}");
            }
        }
    }

    /// The noise constants are the pinned bytes': the base noise's middle half at level 0 ([`NOISE_LOBE`],
    /// within 0.02), each erosion band's mean ([`ERODE_MEANS`], within 0.01) and middle half
    /// ([`NOISE_ERODE`], within 0.02).
    #[test]
    fn the_noise_constants_are_the_bakes() {
        let noise = crate::cloud_noise::production();
        let quartiles = |values: &mut dyn Iterator<Item = u8>| -> (f32, f32) {
            let mut hist = [0usize; 256];
            let mut total = 0usize;
            for v in values {
                hist[usize::from(v)] += 1;
                total += 1;
            }
            let quantile = |q: f32| -> f32 {
                let mut acc = 0usize;
                for (v, n) in hist.iter().enumerate() {
                    acc += n;
                    if acc as f32 >= q * total as f32 {
                        return v as f32 / 255.0;
                    }
                }
                1.0
            };
            (quantile(0.25), quantile(0.75))
        };
        const { assert!(NOISE_LOBE[0] < NOISE_LOBE[1]) };
        let (q25, q75) = quartiles(&mut noise.base.bytes[..noise.base.offsets[1]].iter().copied());
        assert!((q25 - NOISE_LOBE[0]).abs() < 0.02 && (q75 - NOISE_LOBE[1]).abs() < 0.02, "the base: {q25} {q75}");
        let l0 = &noise.erosion.bytes[..noise.erosion.offsets[1]];
        for (k, (&want, &mean)) in ERODE_MEANS.iter().zip(&noise.erosion.means).enumerate() {
            let (q25, q75) = quartiles(&mut l0.iter().skip(k).step_by(4).copied());
            assert!((mean - want).abs() < 0.01 && (q25 - NOISE_ERODE[0]).abs() < 0.02 && (q75 - NOISE_ERODE[1]).abs() < 0.02, "band {k}: mean {mean}, quartiles {q25} {q75}");
        }
    }

    /// A bake for the tests that waits until [`RELEASE_BAKE`] is set (so a job is known to be in
    /// flight), counts itself and makes small textures.
    static RELEASE_BAKE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static BAKES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    fn held_bake() -> crate::cloud_noise::NoiseSet {
        while !RELEASE_BAKE.load(std::sync::atomic::Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        BAKES.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        crate::cloud_noise::bake_sized(4, 2, 1)
    }
    fn broken_bake() -> crate::cloud_noise::NoiseSet {
        panic!("a broken bake (a test's)")
    }

    /// Updates `app` until its noise job has finished and been taken (at most a few seconds).
    fn land_noise(app: &mut App) {
        for _ in 0..4_000 {
            if app.world().resource::<CloudNoise>().job.as_ref().is_none_or(std::thread::JoinHandle::is_finished) {
                app.update();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("the noise job did not finish");
    }

    /// #3032 stage 2b: the noise textures follow
    /// the row with one job in flight shared by quick toggles
    /// (off and on during the bake: the same job, never a second bake), its result taken by
    /// whichever activation is on when it lands; off releases the images, never the finished bytes
    /// (held at their exact length); on again with the bytes kept is an upload whose copy is made
    /// on the job's thread (fresh images, no bake); until both images exist the trace request
    /// carries no noise (`CloudNoise::ids`, so the medium writes nothing and the weather stays
    /// flat); and a job that panics ends in a failure that is not polled for ever and that a later
    /// activation retries.
    #[test]
    fn the_noise_follows_the_row_with_one_job_in_flight() {
        use crate::graphics::{Graphics, Preset};
        let mut app = App::new();
        let mut on = Graphics::preset(Preset::Vanilla);
        on.volumetric_clouds = true;
        app.insert_resource(on).init_resource::<Assets<Image>>().insert_resource(CloudNoise { bake: held_bake, ..Default::default() }).add_systems(Update, follow_noise);
        let set = |app: &mut App, row: bool| app.world_mut().resource_mut::<Graphics>().volumetric_clouds = row;
        let thread = |app: &App| app.world().resource::<CloudNoise>().job.as_ref().map(|j| j.thread().id());
        app.update();
        let first = thread(&app).expect("a bake runs");
        assert!(app.world().resource::<CloudNoise>().ids().is_none(), "no images, so no noise in the request");
        // Quick toggles during the bake: the same job, nothing released but the images.
        for _ in 0..3 {
            set(&mut app, false);
            app.update();
            assert_eq!(thread(&app), Some(first), "off: the job in flight is kept");
            set(&mut app, true);
            app.update();
            assert_eq!(thread(&app), Some(first), "on: the same job, no second bake");
        }
        assert_eq!(app.world().resource::<CloudNoise>().activation, 7);
        RELEASE_BAKE.store(true, std::sync::atomic::Ordering::Release);
        land_noise(&mut app);
        let ids = {
            let n = app.world().resource::<CloudNoise>();
            let bytes = n.bytes.as_ref().expect("the bytes kept");
            assert!(n.job.is_none() && bytes.base.bytes.len() == bytes.base.offsets[2] + 1 && bytes.erosion.bytes.len() == 2 * 2 * 2 * 4 + 4, "exact lengths");
            n.ids().expect("both images")
        };
        assert!(app.world().resource::<Assets<Image>>().get(ids[0]).is_some_and(|i| i.texture_descriptor.mip_level_count == 3));
        assert_eq!(BAKES.load(std::sync::atomic::Ordering::Acquire), 1, "one bake for all the toggles");
        // Off: the images go, the bytes stay; on: an upload (the copy made on the job's thread),
        // fresh images, no bake.
        let kept = app.world().resource::<CloudNoise>().bytes.clone().expect("kept");
        set(&mut app, false);
        app.update();
        assert!(app.world().resource::<CloudNoise>().ids().is_none() && app.world().resource::<Assets<Image>>().get(ids[0]).is_none(), "the images released");
        set(&mut app, true);
        app.update();
        land_noise(&mut app);
        let n = app.world().resource::<CloudNoise>();
        let again = n.ids().expect("an upload");
        assert!(again != ids && n.bytes.as_ref().is_some_and(|b| std::sync::Arc::ptr_eq(b, &kept)), "fresh assets from the same bytes");
        assert_eq!(BAKES.load(std::sync::atomic::Ordering::Acquire), 1, "no second bake");
        // A bake that panics: a logged failure, not polled for ever nor retried in its activation;
        // the next activation tries again.
        let mut app = App::new();
        let mut on = Graphics::preset(Preset::Vanilla);
        on.volumetric_clouds = true;
        app.insert_resource(on).init_resource::<Assets<Image>>().insert_resource(CloudNoise { bake: broken_bake, ..Default::default() }).add_systems(Update, follow_noise);
        app.update();
        land_noise(&mut app);
        for _ in 0..3 {
            app.update();
        }
        {
            let n = app.world().resource::<CloudNoise>();
            assert!(n.job.is_none() && n.failed == Some(n.activation) && n.ids().is_none() && n.bytes.is_none(), "failed, and not tried again in this activation");
        }
        app.world_mut().resource_mut::<CloudNoise>().bake = || crate::cloud_noise::bake_sized(4, 2, 1);
        set(&mut app, false);
        app.update();
        set(&mut app, true);
        app.update();
        land_noise(&mut app);
        assert!(app.world().resource::<CloudNoise>().ids().is_some(), "a later activation retried");
    }

    /// A second held bake (its own gate, so it does not race the test above's).
    static RELEASE_BAKE_U5: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    fn held_bake_u5() -> crate::cloud_noise::NoiseSet {
        while !RELEASE_BAKE_U5.load(std::sync::atomic::Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        crate::cloud_noise::bake_sized(4, 2, 1)
    }

    /// While the row is off only the packed chains are held. A
    /// bake that finishes after switch-off makes no upload copies (the next activation makes them
    /// from the kept bytes, no second bake); a job found finished at switch-off is collected then,
    /// its bytes kept and its copies dropped.
    #[test]
    fn the_row_off_holds_only_the_packed_chains() {
        use crate::graphics::{Graphics, Preset};
        let make = |bake: fn() -> crate::cloud_noise::NoiseSet| {
            let mut app = App::new();
            let mut on = Graphics::preset(Preset::Vanilla);
            on.volumetric_clouds = true;
            app.insert_resource(on).init_resource::<Assets<Image>>().insert_resource(CloudNoise { bake, ..Default::default() }).add_systems(Update, follow_noise);
            app
        };
        let set = |app: &mut App, row: bool| app.world_mut().resource_mut::<Graphics>().volumetric_clouds = row;
        let finish = |app: &App| {
            for _ in 0..4_000 {
                if app.world().resource::<CloudNoise>().job.as_ref().is_none_or(std::thread::JoinHandle::is_finished) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            panic!("the noise job did not finish");
        };
        // Off during the bake: the job finishes with no copies.
        let mut app = make(held_bake_u5);
        app.update();
        set(&mut app, false);
        app.update();
        RELEASE_BAKE_U5.store(true, std::sync::atomic::Ordering::Release);
        finish(&app);
        let p = app.world_mut().resource_mut::<CloudNoise>().job.take().expect("the job").join().expect("a bake");
        assert!(p.images.is_none() && p.baked, "the row off: the packed chains alone");
        app.world_mut().resource_mut::<CloudNoise>().bytes = Some(p.set);
        // On again: the copies made from the kept bytes by a new job, no bake.
        set(&mut app, true);
        app.update();
        finish(&app);
        app.update();
        assert!(app.world().resource::<CloudNoise>().ids().is_some(), "the images made for the active row");
        // A job found finished at switch-off: collected then, the bytes kept, no copies held.
        let mut app = make(|| crate::cloud_noise::bake_sized(4, 2, 1));
        app.update();
        finish(&app);
        set(&mut app, false);
        app.update();
        let n = app.world().resource::<CloudNoise>();
        assert!(n.job.is_none() && n.bytes.is_some() && n.ids().is_none(), "collected at switch-off");
    }

    /// A job that read that the row was on and goes on making the upload copies through the
    /// switch-off: collected when it ends, with the row still off, its bytes kept and its copies
    /// dropped with no activation to wait for; then nothing is left to do.
    #[test]
    fn a_job_still_copying_at_switch_off_is_drained() {
        use crate::graphics::{Graphics, Preset};
        use bevy::ecs::system::RunSystemOnce;
        static RELEASE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        fn held() -> crate::cloud_noise::NoiseSet {
            while !RELEASE.load(std::sync::atomic::Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            crate::cloud_noise::bake_sized(4, 2, 1)
        }
        let mut app = App::new();
        let mut on = Graphics::preset(Preset::Vanilla);
        on.volumetric_clouds = true;
        app.insert_resource(on).init_resource::<Assets<Image>>().insert_resource(CloudNoise { bake: held, ..Default::default() }).add_systems(Update, follow_noise.run_if(noise_in_use));
        app.update();
        app.world_mut().resource_mut::<Graphics>().volumetric_clouds = false;
        app.update();
        // The job sees the row as on, as one that looked before the switch does.
        let want = app.world().resource::<CloudNoise>().want.clone();
        want.store(true, std::sync::atomic::Ordering::Release);
        assert!(app.world_mut().run_system_once(noise_in_use).unwrap(), "off with a job in flight: to be collected");
        RELEASE.store(true, std::sync::atomic::Ordering::Release);
        land_noise(&mut app);
        let n = app.world().resource::<CloudNoise>();
        assert!(n.job.is_none() && n.bytes.is_some() && n.ids().is_none() && !n.on, "drained while off");
        assert!(app.world().resource::<Assets<Image>>().is_empty(), "no copy handed to the assets");
        assert!(!app.world_mut().run_system_once(noise_in_use).unwrap(), "nothing left to do");
    }

    /// `follow_noise` runs, and takes the image assets mutably, only
    /// while the row is on and on the frame it goes off (its images to release); never otherwise.
    #[test]
    fn the_noise_does_no_work_while_the_row_is_off() {
        use crate::graphics::{Graphics, Preset};
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(Graphics::preset(Preset::Vanilla));
        world.init_resource::<CloudNoise>();
        assert!(!world.run_system_once(noise_in_use).unwrap(), "off from the start: nothing to do");
        world.resource_mut::<Graphics>().volumetric_clouds = true;
        assert!(world.run_system_once(noise_in_use).unwrap(), "on");
        world.resource_mut::<Graphics>().volumetric_clouds = false;
        world.resource_mut::<CloudNoise>().on = true;
        assert!(world.run_system_once(noise_in_use).unwrap(), "the frame it goes off: its images to release");
        world.resource_mut::<CloudNoise>().on = false;
        assert!(!world.run_system_once(noise_in_use).unwrap(), "off after that: nothing");
        world.remove_resource::<Graphics>();
        assert!(!world.run_system_once(noise_in_use).unwrap(), "no settings at all: nothing");
    }

    #[test]
    fn volume_phase_is_normalised_and_capped() {
        // ∫ HG / 4π over the sphere = 1/2 ∫ hg(μ) dμ over -1..1 (hg is HG × 4π).
        for g in [0.0, G_FORWARD, G_BACK, 0.8] {
            let n = 20_000;
            let sum: f64 = (0..n).map(|i| f64::from(hg(-1.0 + 2.0 * (i as f32 + 0.5) / n as f32, g))).sum::<f64>() * 2.0 / n as f64;
            assert!((sum / 2.0 - 1.0).abs() < 1e-2, "g {g}: {}", sum / 2.0);
        }
        assert!((hg(0.3, 0.0) - 1.0).abs() < 1e-6, "isotropic is 1");
        for i in 0..=100 {
            let p = phase(-1.0 + i as f32 / 50.0);
            assert!(p > 0.0 && p <= PHASE_CAP && p.is_finite());
        }
        assert!(phase(1.0) > phase(0.0), "brighter toward the light");
    }

    #[test]
    fn volume_light_turns_at_the_horizon_without_a_step() {
        let sun = Vec3::new(0.3, 0.2, -0.9).normalize();
        let (d, s) = direct_light(sun, 30.0, true);
        assert!((d - sun).length() < 1e-6 && s == 1.0);
        let (d, s) = direct_light(sun, -30.0, true);
        assert!((d + sun).length() < 1e-6 && s == 1.0, "the moon, opposite the sun");
        assert_eq!(direct_light(sun, 0.0, true).1, 0.0);
        assert!(direct_light(sun, 0.01, true).1 < 1e-3 && direct_light(sun, -0.01, true).1 < 1e-3);
        assert_eq!(direct_light(sun, -30.0, false).1, 0.0, "no moon: no night light");
        let (d, s) = direct_light(Vec3::NAN, f32::NAN, true);
        assert!(d.is_finite() && s.is_finite());
    }

    /// A fade that starts after the cover was composed renames the sets: the composed set 0
    /// (the weather then current, now fading out) keeps its own weather's colours.
    #[test]
    fn volume_colours_follow_the_composed_weathers() {
        let tinted = |id: u32, c: [u8; 3]| fnv_world::sky::Weather { form_id: id, cloud_colors: vec![vec![c; 4]; 4], ..Default::default() };
        let mut sky = SkyState::default();
        (sky.weather, sky.prev, sky.fade) = (tinted(1, [255, 0, 0]), Some(tinted(2, [0, 0, 255])), 0.1);
        let cover = [[1.0, 0.0, 0.0, 0.0], [0.0; 4]];
        let blue = ambient_colour(&cloud_parts(&sky, &cover, &[Some(2), None]));
        assert!(blue[2] > blue[0], "the old weather's cover keeps the old weather's colour: {blue:?}");
        let red = ambient_colour(&cloud_parts(&sky, &cover, &[Some(1), Some(2)]));
        assert!(red[0] > red[2], "{red:?}");
        // A weather no longer shown counts for nothing.
        sky.prev = None;
        assert_eq!(ambient_colour(&cloud_parts(&sky, &cover, &[Some(2), None])), [0.0; 3]);
    }

    #[test]
    fn volume_ambient_is_the_weighted_mean_of_the_layers() {
        assert_eq!(ambient_colour(&[]), [0.0; 3]);
        assert_eq!(ambient_colour(&[(0.0, [1.0, 1.0, 1.0]), (f32::NAN, [1.0; 3]), (-1.0, [1.0; 3])]), [0.0; 3]);
        let c = ambient_colour(&[(1.0, [1.0, 0.0, 0.0]), (3.0, [0.0, 1.0, 0.0])]);
        assert!((c[0] - 0.25).abs() < 1e-6 && (c[1] - 0.75).abs() < 1e-6 && c[2] == 0.0);
        // A colour that is not a number counts as black, never NaN.
        let c = ambient_colour(&[(1.0, [f32::NAN, f32::INFINITY, -1.0])]);
        assert!(c.iter().all(|v| v.is_finite() && *v >= 0.0));
    }

    /// A synthetic extreme weather and cover: every value finite, colours non-negative, the
    /// steps held.
    #[test]
    fn volume_uniform_survives_extreme_inputs() {
        let bad = VolumeInputs {
            plane_height: f32::NAN,
            uv_scale: Vec2::new(0.0, f32::INFINITY),
            light_dir: Vec3::ZERO,
            light_strength: 9.0,
            sunlight: [f32::NAN, -1.0, 1e30],
            ambient: [f32::INFINITY, 0.5, -0.5],
            sky_scale: f32::NAN,
            steps: 100_000,
            density: -4.0,
            above_ground: f32::NAN,
            drift: [f64::NAN, 1e300],
            domain: f32::INFINITY,
            horizon: -1.0,
            ..Default::default()
        };
        let u = uniform(&bad);
        for v in [u.slab, u.march, u.reach, u.phase, u.light_dir, u.light, u.map, u.profile, u.flags, u.tone, u.vigour, u.vigour2, u.height, u.field, u.scale, u.place0, u.place1, u.place2, u.mix, u.tower, u.kind, u.knot_s0, u.knot_s1, u.knot_c0, u.knot_c1, u.knot_t0, u.knot_t1, u.cal_c0, u.cal_c1, u.cal_k0, u.cal_k1, u.nphase, u.nscale, u.nlevel, u.nlobe, u.nband, u.nerode, u.lmode, u.lupper, u.llower, u.loct, u.lfloor, u.lpowder, u.lcell, u.air_low, u.air_high, u.families] {
            assert!(v.is_finite(), "{v}");
        }
        assert!(u.light.min_element() >= 0.0 && u.lupper.min_element() >= 0.0 && u.llower.min_element() >= 0.0);
        assert_eq!(u.march.x, step_near(MAX_STEPS), "the quality value held");
        assert!(u.slab.y == REFERENCE_HEIGHT && u.slab.w == MIN_HORIZON && u.reach.y > 0.0 && u.slab.z > u.slab.w, "the domain and the horizon held: {}", u.slab);
        assert_eq!(u.march.z, 0.0, "a negative density is none");
        assert!((u.light_dir.truncate().length() - 1.0).abs() < 1e-5 && u.light_dir.w == 1.0);
        assert!(u.map.z == 0.0 && u.map.w == 0.0 && u.map.x > 0.0 && u.map.y > 0.0);
        assert_eq!(uniform(&VolumeInputs { steps: 0, ..bad }).march.x, step_near(MIN_STEPS));
        // The direct term is bounded by the weather's sunlight colour.
        let ok = VolumeInputs { sunlight: [1.0, 0.8, 0.5], sky_scale: 1.0, density: 1.0, steps: VIEW_STEPS, ..Default::default() };
        let u = uniform(&ok);
        assert!((u.light.x * PHASE_CAP - SUN_GAIN * PHASE_CAP).abs() < 1e-6);
        assert!(!needs_write(&u, &u));
        let mut moved = u;
        moved.place0.x += 1.0 / 4096.0;
        assert!(needs_write(&u, &moved), "any drift of the placement is written");
        let mut drifted = u;
        drifted.vigour.x += 1.0 / 4096.0;
        assert!(needs_write(&u, &drifted), "any drift of the vigour is written");
        // A finite but huge sky level (a mod's): the light a step can emit stays a finite half float.
        let huge = uniform(&VolumeInputs { sunlight: [1.0; 3], ambient: [1.0; 3], sky_scale: 3e38, ..ok });
        let most = huge.lupper.max_element() + huge.llower.max_element() + huge.light.max_element() * PHASE_CAP;
        assert!(most.is_finite() && most <= 65_504.0, "{most}");
        let mut crept = u;
        crept.light_dir.x += 1e-5;
        assert!(!needs_write(&u, &crept), "the sun's creep is not written each frame");
    }

    #[test]
    fn volume_debug_aids_parse() {
        assert_eq!(debug_density(None), 1.0);
        assert_eq!(debug_density(Some(" 2.5 ")), 2.5);
        assert_eq!(debug_density(Some("9")), MAX_DEBUG_DENSITY);
        assert_eq!(debug_density(Some("-1")), 1.0);
        assert_eq!(debug_density(Some("NaN")), 1.0);
        assert_eq!(debug_steps(None), VIEW_STEPS);
        assert_eq!(debug_steps(Some("16")), 16);
        assert_eq!(debug_steps(Some("1")), MIN_STEPS);
        assert_eq!(debug_steps(Some("1000")), MAX_STEPS);
        assert_eq!(debug_steps(Some("many")), VIEW_STEPS);
        assert_eq!(debug_height(None), REFERENCE_HEIGHT);
        assert_eq!(debug_height(Some("24000")), 24_000.0);
        assert_eq!(debug_height(Some(" 64000 ")), 64_000.0);
        assert_eq!(debug_height(Some("1000")), MIN_DOMAIN);
        assert_eq!(debug_height(Some("1e9")), MAX_DOMAIN);
        assert_eq!(debug_height(Some("tall")), REFERENCE_HEIGHT);
        assert_eq!(debug_horizon(None), HORIZON);
        assert_eq!(debug_horizon(Some("600000")), 600_000.0);
        assert_eq!(debug_horizon(Some("0")), MIN_HORIZON);
        assert_eq!(debug_horizon(Some("inf")), HORIZON);
        assert_eq!(debug_view(Some("cover")), CloudView::Cover);
        assert_eq!(debug_view(Some(" Vigour ")), CloudView::Vigour);
        assert_eq!(debug_view(Some("count")), CloudView::Count);
        assert_eq!(debug_view(Some("x")), CloudView::None);
        assert_eq!([CloudView::None, CloudView::Cover, CloudView::Vigour, CloudView::Count].map(CloudView::code), [0.0, 1.0, 2.0, 3.0]);
        // The defaults.
        assert_eq!((REFERENCE_HEIGHT, HORIZON, HORIZON_FADE_FROM), (40_000.0, 400_000.0, 0.75));
    }

    /// A small dome-like mesh (game axes, z up, off-centre as atmosphere.nif's is).
    fn test_dome() -> Mesh {
        use bevy::mesh::{Indices, PrimitiveTopology};
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[490.0f32, 0.0, -20.0], [-490.0, 0.0, -20.0], [0.0, 490.0, 30.0], [0.0, -490.0, 30.0], [0.0, 0.0, 250.0]]);
        m.insert_indices(Indices::U32(vec![0, 2, 4, 2, 1, 4, 1, 3, 4, 3, 0, 4]));
        m
    }

    /// The invariant the proxy relies on: its triangles are the dome's to the bit (same
    /// positions, same indices; drawn at the dome's transform with the invariant position, its
    /// depth is the dome's), its depth test is Equal (so a ridge anywhere in front of the
    /// dome, even 300 units at 300,000, keeps its colour: the buffer holds the ridge's depth,
    /// not the dome's), and its bounds are centred on the camera (the sort key stays in the
    /// sky's band, after the flat layers').
    #[test]
    fn volume_proxy_is_the_dome_drawn_depth_equal() {
        use bevy::mesh::{Indices, VertexAttributeValues};
        let dome = test_dome();
        let proxy = proxy_mesh(&dome).expect("a proxy");
        let (Some(VertexAttributeValues::Float32x3(d)), Some(VertexAttributeValues::Float32x3(p))) = (dome.attribute(Mesh::ATTRIBUTE_POSITION), proxy.attribute(Mesh::ATTRIBUTE_POSITION)) else { panic!("positions") };
        assert_eq!(p.len(), d.len() + 2, "two bounds-only vertices");
        for (a, b) in d.iter().zip(p) {
            assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits), "the dome's positions to the bit");
        }
        let (Some(Indices::U32(di)), Some(Indices::U32(pi))) = (dome.indices(), proxy.indices()) else { panic!("indices") };
        assert_eq!(di, pi);
        assert!(pi.iter().all(|&i| (i as usize) < d.len()), "no triangle uses the bounds-only vertices");
        // Bevy's bounds are over every position (bevy_mesh `compute_aabb`).
        let (lo, hi) = p.iter().map(|v| Vec3::from_array(*v)).fold((Vec3::MAX, Vec3::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        assert!(((lo + hi) * 0.5).length() < 1e-3, "bounds centred on the camera: {lo} {hi}");
        let mut depth = bevy::render::render_resource::DepthStencilState {
            format: bevy::render::render_resource::TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
            depth_compare: Some(bevy::render::render_resource::CompareFunction::GreaterEqual),
            stencil: Default::default(),
            bias: Default::default(),
        };
        draw_on_dome(&mut depth);
        assert_eq!(depth.depth_compare, Some(bevy::render::render_resource::CompareFunction::Equal));
        assert_eq!(depth.depth_write_enabled, Some(false));
        // Reverse-Z depth (near / distance): a ridge 300 units before a dome 300,000 off holds
        // a depth the dome's equal-depth copy does not match; only the dome's own does.
        let z = |d: f32| 5.0 / d;
        assert_ne!(z(300_000.0).to_bits(), z(299_700.0).to_bits());
        assert!(crate::sky::sorts_with_sky(SORT_BIAS));
        let (ours, last_flat_layer) = (SORT_BIAS, crate::sky::SKY_SORT_BIAS + 7.0);
        assert!(ours > last_flat_layer, "after the flat layers");
        // Nothing to make a proxy of.
        assert!(proxy_mesh(&Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default())).is_none());
    }

    /// The flat layers and the volume change over in the same frame: switched on, the frame
    /// the volume first shows (once the medium's buffer is written: a stand-in render world
    /// answers here) hides the flat layers; switched off, the frame the volume goes shows them
    /// again. Never both, never neither (sky.rs `update_clouds` stood in for by a system that
    /// shows the flat layers each frame, as it does). The stand-in
    /// writes whole only with both noise textures' GPU views (the medium's own rule,
    /// `noise_views` and `write_cause`): with none, the base's alone and the erosion's alone the
    /// flat layers stay; with both the volume takes over.
    #[test]
    fn volume_and_flat_layers_change_over_in_one_frame() {
        use crate::graphics::{Graphics, Preset};
        use crate::medium::{MediumActive, MediumGate, MediumLink, Wanted, Written};
        fn flat_shown(mut flat: Query<&mut Visibility, With<crate::sky::CloudLayer>>) {
            for mut v in &mut flat {
                *v = Visibility::Inherited;
            }
        }
        /// The noise images whose GPU views the stand-in render world has.
        #[derive(Resource, Default)]
        struct GpuViews(Vec<AssetId<Image>>);
        /// The render world after the frame: it wants 64x36 for the epoch it saw and reports a
        /// write of the image it was given, whole when the size is right and both noise views are
        /// on the GPU.
        fn render_stand_in(request: Res<MediumRequest>, output: Res<MediumOutput>, link: Res<MediumLink>, frame: Res<bevy::diagnostic::FrameCount>, views: Res<GpuViews>, cam: Query<Entity, With<FlyCam>>) {
            let Some(trace) = request.trace.as_ref() else { return };
            let size = UVec2::new(64, 36);
            link.set_wanted(Wanted { epoch: output.epoch, size });
            if let Some(have) = output.size {
                let noise = crate::medium::noise_views(trace.noise, |id| views.0.contains(&id).then_some(id)).is_some();
                let drawn = have == size && noise;
                let cause = crate::medium::write_cause(drawn, true, have == size, noise);
                link.set_written(Written { epoch: output.epoch, frame: frame.0, view: cam.single().ok(), size: have, ok: cause == crate::medium::WriteCause::Whole, cause });
            }
        }
        let mut app = App::new();
        app.insert_resource(Graphics::preset(Preset::Vanilla))
            .insert_resource(VolumeGate(MaterialGate::new()))
            .init_resource::<CloudVolumeShown>()
            .init_resource::<CloudCoverWanted>()
            .init_resource::<Assets<CloudVolumeMaterial>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<VolumeActive>()
            .init_resource::<FlatLayersMoved>()
            .init_resource::<MediumRequest>()
            .init_resource::<MediumOutput>()
            .init_resource::<MediumLink>()
            .init_resource::<MediumGate>()
            .init_resource::<MediumActive>()
            .init_resource::<bevy::diagnostic::FrameCount>()
            .init_resource::<GpuViews>()
            .init_resource::<crate::underwater::CameraSubmerged>()
            .insert_resource(crate::sky::SkyState::default())
            .insert_resource(CloudCoverPublished(Some(CloudCoverOutput {
                settling: None,
                uv_scale: Vec2::splat(1.0 / crate::cloud_shadows::REPEAT_UNITS),
                drift: [0.0; 2],
                plane_height: 20_000.0,
                mean_cover: 0.5,
                layer_cover: [[0.5; 4], [0.0; 4]],
                layer_weathers: [Some(0), None],
                weathers: [Some(WeatherCover { id: crate::sky::SkyState::default().weather.form_id, mean: 0.5, shares: [0.5, 0.0, 0.0, 0.0] }), None],
            })))
            .add_systems(Update, (flat_shown, follow_volume, trace_above_water, crate::medium::follow_medium, show_volume, hide_flat_layers, render_stand_in).chain());
        app.world_mut().spawn((FlyCam { yaw: 0.0, pitch: 0.0, speed: 0.0 }, Transform::default()));
        let dome = app.world_mut().resource_mut::<Assets<Mesh>>().add(test_dome());
        app.world_mut().spawn((crate::sky::SkyDome::for_test(), Mesh3d(dome), Transform::from_scale(Vec3::splat(700.0))));
        let flat = app.world_mut().spawn((crate::sky::CloudLayer::for_test(3), Visibility::Inherited)).id();
        // The volume draws when its proxy is visible and not held (a held proxy adds nothing).
        let seen = |app: &mut App| {
            // Seen by the main camera: visible and not moved to the mirror's layer.
            let flat = app.world().get::<Visibility>(flat).copied() != Some(Visibility::Hidden) && app.world().get::<RenderLayers>(flat).is_none();
            let mut q = app.world_mut().query_filtered::<(&Visibility, &MeshMaterial3d<CloudVolumeMaterial>), With<CloudVolumeProxy>>();
            let drawn: Vec<(Visibility, Handle<CloudVolumeMaterial>)> = q.iter(app.world()).map(|(v, m)| (*v, m.0.clone())).collect();
            let materials = app.world().resource::<Assets<CloudVolumeMaterial>>();
            let volume = drawn.iter().any(|(v, m)| *v != Visibility::Hidden && materials.get(m).is_some_and(|m| m.params.flags.y < 0.5));
            (flat, volume)
        };
        // The noise images made (as follow_noise leaves them).
        let small = crate::cloud_noise::bake_sized(4, 2, 1);
        let noise_ids = {
            let mut images = app.world_mut().resource_mut::<Assets<Image>>();
            [images.add(small.base.image()), images.add(small.erosion.image())]
        };
        let ids = [noise_ids[0].id(), noise_ids[1].id()];
        app.insert_resource(CloudNoise { images: Some(noise_ids), ..Default::default() });
        app.update();
        assert_eq!(seen(&mut app), (true, false), "off: the flat layers alone");
        app.world_mut().resource_mut::<Graphics>().volumetric_clouds = true;
        // No noise view on the GPU, the base's alone, the erosion's alone: never whole, the flat
        // layers stay.
        for have in [vec![], vec![ids[0]], vec![ids[1]]] {
            app.world_mut().resource_mut::<GpuViews>().0 = have.clone();
            for frame in 0..5 {
                app.update();
                assert_eq!(seen(&mut app), (true, false), "views {have:?}, frame {frame}: the flat layers stay");
            }
            assert_eq!(app.world().resource::<MediumRequest>().trace.as_ref().and_then(|t| t.noise), Some(ids), "the request carries both images");
        }
        app.world_mut().resource_mut::<GpuViews>().0 = ids.to_vec();
        // Both views: the proxy made, the trace asked for, the buffer made (the proxy shown
        // held), then written: the volume shows within a few frames.
        for frame in 0..5 {
            app.update();
            let (f, v) = seen(&mut app);
            assert!(f != v, "switching on, frame {frame}: one of the two each frame, flat {f} volume {v}");
        }
        assert_eq!(seen(&mut app), (false, true), "on: the volume alone");
        assert_eq!(app.world().get::<RenderLayers>(flat), Some(&RenderLayers::layer(FLAT_CLOUD_LAYER)), "the flat layer moved for the mirror and the lens");
        assert_ne!(app.world().get::<Visibility>(flat), Some(&Visibility::Hidden), "its weather visibility kept");
        assert!(app.world().resource::<CloudCoverWanted>().0);
        let output = app.world().resource::<MediumOutput>().clone();
        assert_eq!(output.size, Some(UVec2::new(64, 36)));
        let mut q = app.world_mut().query::<&MeshMaterial3d<CloudVolumeMaterial>>();
        let material = q.single(app.world()).expect("proxy material").0.clone();
        assert_eq!(app.world().resource::<Assets<CloudVolumeMaterial>>().get(&material).expect("material").far, output.image, "the proxy reads the medium's image");
        // The camera under water: the flat layers in that very frame, and no trace to wait for;
        // above it again the trace is asked for at once, and the volume comes back with it.
        app.world_mut().resource_mut::<crate::underwater::CameraSubmerged>().0 = true;
        app.update();
        assert_eq!(seen(&mut app), (true, false), "under water in the frame the camera goes under");
        {
            let request = app.world().resource::<MediumRequest>();
            assert!(request.wanted && request.trace.is_none() && !request.pending);
        }
        app.world_mut().resource_mut::<crate::underwater::CameraSubmerged>().0 = false;
        app.update();
        assert!(app.world().resource::<MediumRequest>().trace.is_some(), "above water: asked for in that frame");
        for _ in 0..5 {
            app.update();
            let (f, v) = seen(&mut app);
            assert!(f != v, "one of the two each frame, flat {f} volume {v}");
        }
        assert_eq!(seen(&mut app), (false, true), "the volume again");
        // A render world that stops writing whole (a pipeline lost) is not drawn from.
        app.add_systems(PreUpdate, |link: Res<MediumLink>, output: Res<MediumOutput>, cam: Query<Entity, With<FlyCam>>| {
            link.set_written(Written { epoch: output.epoch, frame: 0, view: cam.single().ok(), size: UVec2::new(64, 36), ok: false, cause: crate::medium::WriteCause::ComposerNotReady });
        });
        app.update();
        assert_eq!(seen(&mut app), (true, false), "a write that is not whole hands back to the flat layers");
        app.world_mut().resource_mut::<Graphics>().volumetric_clouds = false;
        app.update();
        assert_eq!(seen(&mut app), (true, false), "off again in the same frame");
        assert!(!app.world().resource::<CloudCoverWanted>().0);
        // A cover of nothing (a clear night): not drawn, the flat layers stay.
        {
            // The volume's amount is the live weathers' own covers: empty them too.
            let mut published = app.world_mut().resource_mut::<CloudCoverPublished>();
            let cover = published.0.as_mut().expect("cover");
            cover.mean_cover = 0.0;
            for w in cover.weathers.iter_mut().flatten() {
                w.mean = 0.0;
                w.shares = [0.0; 4];
            }
        }
        app.world_mut().resource_mut::<Graphics>().volumetric_clouds = true;
        for _ in 0..5 {
            app.update();
            assert_eq!(seen(&mut app), (true, false), "an empty cover is not marched");
        }
        assert!(!app.world().resource::<CloudVolumeShown>().0);
        assert!(app.world().resource::<MediumRequest>().trace.is_none(), "nothing asked of the medium");
        // No cover yet is one to come; a place without cloud layers (cloud_shadows.rs
        // `CloudCoverEmpty`) has none to wait for.
        app.world_mut().resource_mut::<CloudCoverPublished>().0 = None;
        app.update();
        assert!(app.world().resource::<MediumRequest>().pending, "a cover is to come");
        app.insert_resource(crate::cloud_shadows::CloudCoverEmpty(true));
        app.update();
        assert!(!app.world().resource::<MediumRequest>().pending && app.world().resource::<MediumRequest>().trace.is_none(), "nothing to draw, nothing to wait for");
        // The proxy took the dome's transform.
        let mut q = app.world_mut().query_filtered::<&Transform, With<CloudVolumeProxy>>();
        assert_eq!(q.single(app.world()).expect("proxy").scale, Vec3::splat(700.0));
    }

    /// Switched on and off again before the proxy could be made (no sky dome yet): the gate
    /// that opened still closes (the early-toggle hole).
    #[test]
    fn a_row_toggled_before_the_proxy_exists_still_closes_the_gate() {
        use crate::graphics::{Graphics, Preset};
        let mut app = App::new();
        let gate = MaterialGate::new();
        app.insert_resource(Graphics::preset(Preset::Vanilla))
            .insert_resource(VolumeGate(gate.clone()))
            .init_resource::<CloudVolumeShown>()
            .init_resource::<CloudCoverWanted>()
            .init_resource::<CloudCoverPublished>()
            .init_resource::<Assets<CloudVolumeMaterial>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<VolumeActive>()
            .init_resource::<MediumRequest>()
            .add_systems(Update, follow_volume.run_if(volume_in_use));
        app.world_mut().resource_mut::<Graphics>().volumetric_clouds = true;
        app.update();
        assert!(gate.is_open(), "opened before any material exists");
        assert!(app.world().resource::<MediumRequest>().wanted);
        app.world_mut().resource_mut::<Graphics>().volumetric_clouds = false;
        for _ in 0..(IDLE_FRAMES + 2) {
            app.update();
        }
        assert!(!gate.is_open(), "closed again with no proxy ever made");
        assert!(!app.world().resource::<VolumeActive>().0);
        assert_eq!(*app.world().resource::<MediumRequest>(), MediumRequest::default());
    }

    /// volumetric_clouds_march.wesl, plain WGSL, with a test entry point that calls the trace
    /// (the seam the medium calls) and the cover view.
    #[test]
    fn the_march_module_is_valid_wgsl() {
        use wgpu::naga;
        // Without a view, and the view variant (`VIEWS_DEF`).
        for views in [false, true] {
            let source = format!(
                "{}\n@group(0) @binding(0) var<uniform> volume: CloudVolume;\n@group(0) @binding(1) var nb: texture_3d<f32>;\n@group(0) @binding(2) var ns: sampler;\n@group(0) @binding(3) var ne: texture_3d<f32>;\n@fragment\nfn test_entry(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{\n    let ray = MediumRay(vec3<f32>(0.0), normalize(vec3<f32>(0.1, 1.0, 0.2)), 0.0, 1.0e30, 0.003);\n    let r = vc_trace_refined(ray, vc_dither(p.xy, 3u, true), nb, ns, ne, volume);\n    let c = vc_view_sample(ray, vc_view_code(volume), nb, ns, ne, volume);\n    return vec4<f32>(r.colour + c.colour, r.transmittance * c.transmittance + r.hit_distance);\n}}\n",
                crate::volumetric_clouds_lightvol::lv_resolve_variant(&crate::volumetric_clouds_light::plain_march_wgsl(), false, views)
            );
            let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
        }
    }

    /// The view variant is the trace's while a view is asked for, by the code as the march rounds and holds
    /// it; a default run draws without it.
    #[test]
    fn the_view_variant_is_for_a_view() {
        let views = [
            CloudView::None,
            CloudView::Cover,
            CloudView::Vigour,
            CloudView::Count,
            CloudView::Noise,
            CloudView::Sun,
            CloudView::Ambient,
            CloudView::Depth,
            CloudView::Opacity,
            CloudView::BodyHeight,
        ];
        for view in views {
            assert_eq!(view_variant(&uniform(&VolumeInputs { view, ..Default::default() })), view != CloudView::None, "{view:?}");
        }
        for gone in ["detail", "overhead", "face"] {
            assert_eq!(debug_view(Some(gone)), CloudView::None, "{gone}");
        }
        let mut u = uniform(&VolumeInputs::default());
        for (z, want) in [(-1.0f32, false), (0.0, false), (0.49, false), (0.5, true), (13.0, true), (99.0, true)] {
            u.flags.z = z;
            assert_eq!(view_variant(&u), want, "flags.z {z}");
        }
        assert!(!view_variant(&uniform(&drawn_inputs(START.inputs(VolumeInputs::default())))), "a default run draws without a view");
        for (shape, least) in [(0.0f32, SHAPE_LEAST), (-1.0, SHAPE_LEAST), (f32::MIN_POSITIVE, SHAPE_LEAST), (1.0, 1.0), (4.0, 4.0)] {
            let i = drawn_inputs(VolumeInputs { shape_aid: Some(shape), ..Default::default() });
            assert_eq!(uniform(&i).nscale.z, least, "SHAPE {shape}");
        }
        let march = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        assert!(!march.contains("vc_model") && !march.contains("vc_m1_"), "one density model");
        assert!(march.contains("@if(CLOUD_VIEWS) fn vc_view_code(v: CloudVolume) -> u32 {\n    return u32(clamp(v.flags.z, 0.0, 13.0) + 0.5);\n}"));
        assert!(march.contains("@if(!CLOUD_VIEWS) fn vc_view_code(v: CloudVolume) -> u32 {\n    return 0u;\n}"));
        assert_eq!(VIEWS_DEF, "CLOUD_VIEWS");
        assert_eq!((variant_suffix(false), variant_suffix(true)), ("", "_views"));
    }

    /// The march's integral (the first prototype's step integral, now over the bent domain with
    /// no slant correction): the lines that make
    /// the colour, the transmittance and the weighted distance, kept to the letter; the cells
    /// cover the span (each sampled at the start offset within it); the distance is along the
    /// straight world ray.
    #[test]
    fn the_trace_keeps_the_prototypes_arithmetic() {
        // The march with the light's leaf (#3032 stage 2e: the shading lines moved there).
        let src = crate::volumetric_clouds_light::plain_march_wgsl();
        assert!(!src.contains("let slant"), "no slant correction: a real volume needs none");
        for line in [
            "return vc_ray_light(dot(dir, v.light_dir.xyz), v.phase, v.light.rgb, v.light_dir.w, v.loct, v.lpowder.x);",
            "dt = vc_node(b, f, i, n, e) - c0;",
            "t = c0 + offset * dt;",
            "                rel = ray.dir * t;",
            "                width = spread * t;",
            "emitted = vc_incident(d, depth, sampled, ray_light, shading, rel, ray.origin, width, column, v);",
            "return ambient + direct;",
            "return VcShading(vc_view_code(v), v.lmode, v.lupper.rgb, v.llower.rgb, v.loct, v.lfloor, v.light_dir.y);",
            "let kept = exp(-d.x * d.y * dt);",
            "m.hidden_t += taken * t;",
            "let fade = vc_horizon_fade(t0, v.slab.w);",
            "let taken = m.trans * (1.0 - kept);",
            "m.colour += taken * emitted;",
            "colour += trans * fade * m.colour;",
            "trans *= 1.0 - (1.0 - m.trans) * fade;",
            // The distance weighted as the colour is.
            "hidden += trans * fade * m.hidden;",
            "hidden_t += trans * fade * m.hidden_t;",
            "return MediumSample(colour, trans, dist);",
            "return MediumSample(vec3<f32>(0.0), 1.0, -1.0);",
        ] {
            assert!(src.contains(line), "the march no longer has `{line}`");
        }
        assert_eq!(crate::medium::EMPTY_DISTANCE, -1.0, "the march's empty distance is the medium's");
    }

    /// volumetric_clouds.wesl with its imports replaced by stand-ins (the fields it reads of
    /// Bevy's types) and the medium's upsample and nvf_assets' tone input modules pasted in, as
    /// plain WGSL. Only a run of
    /// the game proves the composed shader (the WESL imports, the material bind group).
    #[test]
    fn the_material_shader_is_valid_wgsl() {
        use wgpu::naga;
        let shader = include_str!("volumetric_clouds.wesl").replace("\r\n", "\n");
        assert!(shader.contains("import nvforever::medium_upsample::"), "the upsample module is imported by its path");
        let mut body = String::new();
        let mut in_import = false;
        for line in shader.lines() {
            if in_import || line.starts_with("import ") {
                in_import = !line.trim_end().ends_with(';');
                continue;
            }
            body.push_str(line);
            body.push('\n');
        }
        let body = body.replace("constants::MATERIAL_BIND_GROUP", "2");
        let stand_ins = "struct VertexOutput {\n    @builtin(position) position: vec4<f32>,\n    @location(0) world_position: vec4<f32>,\n};\nstruct View { unjittered_clip_from_world: mat4x4<f32>, world_position: vec3<f32>, };\n@group(0) @binding(0) var<uniform> view: View;\n";
        let source = format!(
            "{stand_ins}{}\n{}\n{body}",
            include_str!("medium_upsample.wesl").replace("\r\n", "\n"),
            include_str!("../../nvf-assets/src/tone_input.wesl").replace("\r\n", "\n")
        );
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(module.entry_points.iter().any(|e| e.name == "fragment"));
        for binding in [
            "@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> compose: CloudCompose;",
            "@group(constants::MATERIAL_BIND_GROUP) @binding(1) var far_light: texture_2d<f32>;",
            "@group(constants::MATERIAL_BIND_GROUP) @binding(2) var far_sampler: sampler;",
        ] {
            assert!(shader.contains(binding), "{binding}: the Rust layout's bindings (#[uniform(0)], #[texture(1)], #[sampler(2)])");
        }
        assert!(shader.contains("view.unjittered_clip_from_world"), "the buffer is read where the unjittered projection puts the fragment");
        assert_eq!(CloudComposeUniform::min_size().get(), 16);
        use crate::lights::ToneCurve;
        assert_eq!(compose_uniform(ToneCurve::Clipped, false, false).flags.x, 1.0);
        assert_eq!(compose_uniform(ToneCurve::TonyMcMapface, false, true).flags.y, 1.0);
        assert_eq!(compose_uniform(ToneCurve::TonyMcMapface, false, false), CloudComposeUniform::default());
        assert_eq!(compose_uniform(ToneCurve::TonyMcMapface, true, false).flags.x, 1.0, "linear light (Physical lighting) skips the inverse");
        let i = VolumeInputs::default();
        assert_eq!(physical_sun_hue(i, false), i);
        assert_eq!(physical_sun_hue(i, true).light_aids.sun_colour_share(), 1.0, "the physical sun's own hue");
        assert!(shader.contains("if compose.flags.y > 0.5 {\n        return vec4<f32>(0.0);"), "held: nothing added (premultiplied zero)");
    }

    /// The volume maps its colour through the sky's own tonemapper input (one table, in
    /// nvf_assets tone_input.wesl), so it lands as the sky does.
    #[test]
    fn the_tonemapper_inverse_is_the_skys() {
        let (sky, ours) = (include_str!("sky.wesl"), include_str!("volumetric_clouds.wesl"));
        assert!(sky.contains("import nvf_assets::tone_input::to_tonemap_input;"));
        assert!(ours.contains("import nvf_assets::tone_input::to_tonemap_input_each;"));
        assert!(!sky.contains("var<private> TONY_IN") && !ours.contains("var<private> TONY_IN"), "no copy of the table");
    }

    /// The uniform's field order is the shader's struct's.
    #[test]
    fn the_uniform_matches_the_shader_struct() {
        let src = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        let start = src.find("struct CloudVolume {").unwrap();
        let body = &src[start..start + src[start..].find('}').unwrap()];
        let fields: Vec<&str> = body.lines().filter_map(|l| l.trim().split_once(": vec4<f32>").map(|(n, _)| n)).collect();
        let cells: Vec<&str> = body.lines().filter_map(|l| l.trim().split_once(": vec4<u32>").map(|(n, _)| n)).collect();
        assert_eq!(cells, ["cells_v", "cells_p", "cells_q", "cross0", "cross1", "cross2"]);
        let after_place2 = body.find("place2: vec4<f32>").expect("place2") < body.find("cells_v: vec4<u32>").expect("cells_v") && body.find("cells_q: vec4<u32>").expect("cells_q") < body.find("mix: vec4<f32>").expect("mix");
        assert!(after_place2, "the whole cells between place2 and mix, as in the Rust struct");
        assert!(body.find("cells_q:").unwrap() < body.find("cross0:").unwrap() && body.find("cross2:").unwrap() < body.find("mix:").unwrap());
        assert_eq!(fields, ["slab", "march", "reach", "phase", "light_dir", "light", "map", "profile", "flags", "tone", "vigour", "vigour2", "height", "field", "scale", "place0", "place1", "place2", "mix", "tower", "kind", "knot_s0", "knot_s1", "knot_c0", "knot_c1", "knot_t0", "knot_t1", "cal_c0", "cal_c1", "cal_k0", "cal_k1", "nphase", "nscale", "nlevel", "nlobe", "nband", "nerode", "dshape", "lv", "lv_near", "lv_far", "lv_size", "lv_cam", "lmode", "lupper", "llower", "loct", "lfloor", "lpowder", "lcell", "air_low", "air_high", "families"]);
        // The light's block (#3032 stage 2e) after the light volume's place, then the density model
        // experiment and the optional cloud-air colours. The Rust and shader layouts must agree.
        // The markers are whole lines of their own.
        let rust = include_str!("volumetric_clouds.rs").replace("\r\n", "\n");
        let lines: Vec<&str> = body.lines().map(str::trim).collect();
        let at = |text: &str| lines.iter().position(|l| *l == text).unwrap_or_else(|| panic!("no line `{text}` in CloudVolume"));
        let (nerode, place_2g, block_2e, lmode) = (at("nerode: vec4<f32>,"), at("// light volume (stage 2g)"), at("// light (stage 2e)"), at("lmode: vec4<f32>,"));
        assert!(nerode < place_2g && place_2g < block_2e && block_2e < lmode, "nerode, the 2g place, the light's marker, its first field, in that order");
        assert!(lines[place_2g + 1..block_2e].iter().all(|l| l.is_empty() || l.starts_with("//") || l.starts_with("lv")), "only the light volume's fields between its marker and the light's");
        let (lfloor, lcell) = (at("lfloor: vec4<f32>,"), at("lcell: vec4<f32>,"));
        assert!(lmode < lfloor && lfloor < lcell, "the light's block, then its filter cell");
        assert_eq!(CloudVolumeUniform::min_size().get(), 16 * (fields.len() + cells.len()) as u64);
        // The detail's block (#3032 stage 2h) marked by its own line in both structs.
        assert!(body.contains("\n    // detail (stage 2h)\n    // z: the wisps"));
        assert!(rust.contains("\n    // detail (stage 2h)\n    /// z: the wisps"));
    }

    /// Every field by its name: set alone and written as the uniform buffer is written, it fills the 16 bytes
    /// the shader's field of that name has, and no other (so two fields swapped in either struct fail).
    #[test]
    fn every_uniform_field_is_where_the_shader_reads_it() {
        use bevy::reflect::structs::GetField;
        use bevy::render::render_resource::encase;
        let src = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        let start = src.find("struct CloudVolume {").unwrap();
        let body = &src[start..start + src[start..].find("\n}\n").unwrap()];
        let fields: Vec<(&str, &str)> = body
            .lines()
            .skip(1)
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("//"))
            .map(|l| l.strip_suffix(',').unwrap_or(l).split_once(": ").unwrap_or_else(|| panic!("a field: {l}")))
            .collect();
        let bytes = |u: &CloudVolumeUniform| -> Vec<u8> {
            let mut b = encase::UniformBuffer::new(Vec::<u8>::new());
            b.write(u).expect("the uniform is written");
            b.into_inner()
        };
        assert_eq!(bytes(&CloudVolumeUniform::default()), vec![0u8; 16 * fields.len()], "the shader's fields are the whole uniform");
        for (slot, (name, kind)) in fields.iter().enumerate() {
            let mut u = CloudVolumeUniform::default();
            let want: Vec<u8> = match *kind {
                "vec4<f32>" => {
                    *u.get_field_mut::<Vec4>(name).unwrap_or_else(|| panic!("no Vec4 field `{name}`")) = Vec4::new(1.0, 2.0, 3.0, 4.0);
                    [1.0f32, 2.0, 3.0, 4.0].iter().flat_map(|v| v.to_le_bytes()).collect()
                }
                "vec4<u32>" => {
                    *u.get_field_mut::<UVec4>(name).unwrap_or_else(|| panic!("no UVec4 field `{name}`")) = UVec4::new(5, 6, 7, 8);
                    [5u32, 6, 7, 8].iter().flat_map(|v| v.to_le_bytes()).collect()
                }
                other => panic!("`{name}`: {other}"),
            };
            let mut all = vec![0u8; 16 * fields.len()];
            all[16 * slot..16 * slot + 16].copy_from_slice(&want);
            assert_eq!(bytes(&u), all, "`{name}` at slot {slot}");
        }
    }

    /// A weather with a value that is not a finite number is shown flat, on either
    /// side of a fade; a finite extreme (a mod's huge sky level) stays, bounded by MAX_LIGHT.
    #[test]
    fn a_weather_with_a_number_that_is_not_finite_is_shown_flat() {
        let cover = CloudCoverOutput {
            settling: None,
            uv_scale: Vec2::splat(1.0 / crate::cloud_shadows::REPEAT_UNITS),
            drift: [0.0; 2],
            plane_height: 20_000.0,
            mean_cover: 0.5,
            layer_cover: [[0.5; 4], [0.0; 4]],
            layer_weathers: [Some(0), None],
            weathers: [Some(WeatherCover { id: 0, mean: 0.5, shares: [0.5, 0.0, 0.0, 0.0] }), None],
        };
        let mut sky = crate::sky::SkyState::default();
        let blended = blend_covers(crate::cloud_shadows::live_weathers(&sky), &cover.weathers).expect("the live weather's cover");
        let ok = inputs(&sky, &cover, &blended, false);
        assert!(usable_values(&sky, &cover, &ok));
        assert!(usable_values(&sky, &cover, &VolumeInputs { sky_scale: 3e38, ..ok }), "finite: kept (and bounded)");
        assert!(!usable_values(&sky, &cover, &VolumeInputs { sky_scale: f32::INFINITY, ..ok }));
        assert!(!usable_values(&sky, &cover, &VolumeInputs { sunlight: [f32::NAN, 0.0, 0.0], ..ok }));
        assert!(!usable_values(&sky, &CloudCoverOutput { plane_height: f32::NAN, ..cover.clone() }, &ok));
        assert!(!usable_values(&sky, &CloudCoverOutput { mean_cover: f32::NAN, ..cover.clone() }, &ok));
        assert!(!usable_values(&sky, &CloudCoverOutput { weathers: [Some(WeatherCover { id: 0, mean: f32::NAN, shares: [0.0; 4] }), None], ..cover.clone() }, &ok), "a weather's own cover that is not a number");
        // #3032 stage 2a: the unwrapped drift, the plane's height over the ground and the fade.
        assert!(!usable_values(&sky, &CloudCoverOutput { drift: [f64::NAN, 0.0], ..cover.clone() }, &ok));
        assert!(!usable_values(&sky, &cover, &VolumeInputs { above_ground: f32::INFINITY, ..ok }));
        assert!(usable_values(&sky, &CloudCoverOutput { drift: [-1.0e6, 3.0e6], ..cover.clone() }, &ok), "a long drift is finite: kept");
        let fade = sky.fade;
        sky.fade = f32::NAN;
        assert!(!usable_values(&sky, &cover, &ok), "a fade that is not a number");
        sky.fade = fade;
        // The weather fading out has a broken fog distance: refused while the fade runs.
        let mut broken = sky.weather.clone();
        broken.fog[2] = f32::NAN;
        sky.prev = Some(broken.clone());
        sky.fade = 0.5;
        assert!(!usable_values(&sky, &cover, &ok), "one side of a fade");
        sky.fade = 1.0;
        assert!(usable_values(&sky, &cover, &ok), "the fade is over");
        sky.prev = None;
        sky.weather = broken;
        assert!(!usable_values(&sky, &cover, &ok), "the shown weather");
    }

    // --- #3032 stage 2c, checkpoint C1: the refined view march (`refined_march`, march
    // `vc_trace_refined`) against the dense march, on made-up media and on the real field. ---

    /// The spread of the default buffer of a 2560x1440 view (1280x720, tiles of 2).
    pub(super) fn c1_spread() -> f32 {
        2.0 * (59.84f32.to_radians() * 0.5).tan() / 720.0 * 2.0
    }

    /// The dense reference: a march in fixed steps through a medium, its plan
    /// (`ray_plan`), `refine` times its steps, the start offset `dither`.
    #[allow(clippy::too_many_arguments)]
    fn m_dense(origin: Vec3, dir: Vec3, spread: f32, (t_min, t_max): (f32, f32), dither: f32, refine: u32, u: &CloudVolumeUniform, f: &RayField) -> MarchRun {
        let stretches = spans(origin, dir, t_min, t_max, u);
        let (plan, _) = ray_plan(stretches, spread, u);
        let taps = (u.march.y as u32).clamp(1, 16);
        let mut run = MarchRun { trans: 1.0, grow: 1.0, ..Default::default() };
        for (k, (t0, t1)) in stretches.into_iter().enumerate() {
            if t1 <= t0 || run.trans < u.reach.z {
                continue;
            }
            let fade = horizon_fade(t0, u.slab.w);
            if fade <= 0.0 {
                continue;
            }
            let ahead = run.trans;
            let n = plan[k].max(1) * refine;
            let dt = (t1 - t0) / n as f32;
            let (mut mt, mut mh, mut mht) = (1.0f32, 0.0f32, 0.0f32);
            for i in 0..n {
                let t = t0 + (i as f32 + dither) * dt;
                run.fine += 1;
                run.samples.push(t);
                let (sigma, shape) = f.at(t, dt);
                if shape <= 0.0 {
                    continue;
                }
                if shape > u.march.w && u.light_dir.w > 0.0 {
                    run.taps += taps;
                }
                let kept = (-sigma * dt).exp();
                let taken = mt * (1.0 - kept);
                mh += taken;
                mht += taken * t;
                mt *= kept;
                if mt * ahead < u.reach.z {
                    break;
                }
            }
            run.hidden += ahead * fade * mh;
            run.hidden_t += ahead * fade * mht;
            run.trans *= 1.0 - (1.0 - mt) * fade;
        }
        run
    }

    /// The default uniform at the ground with the direct light on (so the taps are counted) and the
    /// refined march's coarse spacing `coarse`.
    fn c1_uniform(horizon: f32, coarse: f32) -> CloudVolumeUniform {
        let mut u = ground_uniform(REFERENCE_HEIGHT, horizon);
        u.light_dir.w = 1.0;
        u.profile.z = coarse;
        u
    }

    /// The fine step of the made-up fixtures (half the surface's narrowest transition, held while the
    /// cast width is under it: to 37,500 units) and their coarse spacing, blocks of four cells.
    const C1_STEP: f32 = 120.0;
    const C1_COARSE: f32 = 4.0 * C1_STEP;

    /// [`c1_uniform`] for the made-up media: the erosion off in the spacing (no band bound), so the
    /// fine step is [`C1_STEP`] all along the fixtures' stretches and the blocks hold four cells at
    /// [`C1_COARSE`].
    fn c1_grid_uniform(horizon: f32, coarse: f32) -> CloudVolumeUniform {
        let mut u = c1_uniform(horizon, coarse);
        u.nscale.w = 0.0;
        u
    }

    /// Every integrated cell lies in a stretch of the ray, at or past `t_min`, in order and disjoint
    /// (each accepted length integrated once), its sample inside it, and no probe behind the start.
    fn c1_check_cells(run: &MarchRun, stretches: [(f32, f32); 2], t_min: f32) {
        for w in run.cells.windows(2) {
            assert!(w[1].0 >= w[0].1 - 1e-2, "ordered and disjoint: {:?} then {:?}", w[0], w[1]);
        }
        for (&(lo, hi), &t) in run.cells.iter().zip(&run.samples) {
            assert!(hi > lo && (lo..=hi).contains(&t), "a sample in its cell: {lo}..{hi} at {t}");
            assert!(lo >= t_min - 1e-2, "nothing behind the ray's start: {lo}");
            assert!(stretches.iter().any(|&(t0, t1)| t1 > t0 && lo >= t0 - 1e-2 && hi <= t1 + 1e-2), "{lo}..{hi} outside the stretches {stretches:?}");
        }
        assert!(run.probed.iter().all(|&t| t >= t_min - 1e-2), "no probe behind the ray's start");
    }

    /// A made-up medium from a support interval and a density inside it.
    fn c1_run(origin: Vec3, dir: Vec3, u: &CloudVolumeUniform, inside: &dyn Fn(f32) -> bool, sigma: &dyn Fn(f32) -> f32, offset: f32) -> MarchRun {
        let support = |t: f32| inside(t);
        let density = |t: f32| if inside(t) { (sigma(t), 0.5) } else { (0.0, 0.0) };
        let field = RayField { support: &support, density: &density, stepped: None };
        let run = refined_march(origin, dir, c1_spread(), (0.0, crate::medium::RAY_END), offset, u, &field);
        c1_check_cells(&run, spans(origin, dir, 0.0, crate::medium::RAY_END, u), 0.0);
        run
    }

    /// The grid the made-up fixtures stand on: a ray straight up from inside the domain above its
    /// base band (25,000 units, no guard), stretch 0..35,000, the fine step [`C1_STEP`] all along,
    /// blocks of four cells at the coarse spacing [`C1_COARSE`] (nodes at multiples of it).
    #[test]
    fn the_refined_grid_is_the_near_step_in_blocks_of_the_coarse_spacing() {
        let u = c1_grid_uniform(HORIZON, C1_COARSE);
        let (o, d) = (Vec3::new(0.0, 25_000.0, 0.0), Vec3::Y);
        assert_eq!(spans(o, d, 0.0, crate::medium::RAY_END, &u)[0], (0.0, 35_000.0));
        for t in [0.0f32, 10_000.0, 35_000.0] {
            assert_eq!(fine_step(t, c1_spread(), &u), C1_STEP, "at {t}");
            assert_eq!(coarse_step(d, t, c1_spread(), &u), C1_COARSE, "at {t}");
        }
        assert!(!guard(o, d, 0.0, 35_000.0, &u), "above the base band");
        assert_eq!(refined_plan(spans(o, d, 0.0, crate::medium::RAY_END, &u), c1_spread(), &u), (1.0, false));
        // Empty air: one probe at the start and one per block, nothing integrated.
        let run = c1_run(o, d, &u, &|_| false, &|_| 0.0, 0.5);
        assert_eq!((run.probes, run.fine, run.taps, run.trans), (1 + 73, 0, 0, 1.0));
        let mut nodes = run.probed.clone();
        nodes.sort_by(f32::total_cmp);
        assert_eq!(nodes[..3], [0.0, 480.0, 960.0]);
        assert_eq!(*nodes.last().unwrap(), 35_000.0, "the stretch's end is probed");
        // The coarse spacing follows the amount and the domain.
        assert_eq!((coarse_spacing(0.05, REFERENCE_HEIGHT), coarse_spacing(0.1, REFERENCE_HEIGHT), coarse_spacing(0.3, REFERENCE_HEIGHT)), (COARSE_THIN, COARSE_THIN, COARSE_FAIR));
        assert_eq!(coarse_spacing(0.3, 64_000.0), COARSE_FAIR * 1.6);
        assert_eq!(coarse_spacing(f32::NAN, REFERENCE_HEIGHT), COARSE_THIN);
        assert_eq!(ground_uniform(REFERENCE_HEIGHT, HORIZON).profile.z, coarse_spacing(0.0, REFERENCE_HEIGHT), "the uniform carries it");
        // Far off on a grazing ray the finest placement octave is gone and the spacing widens: past the
        // finest cell (3,844 units) of the fair footprint, the mean of the cast width and its shell one.
        let graze = Vec3::new(2.0f32.to_radians().cos(), 2.0f32.to_radians().sin(), 0.0);
        assert!(coarse_step(graze, 600_000.0, c1_spread(), &u) > C1_COARSE && coarse_step(graze, 100_000.0, c1_spread(), &u) == C1_COARSE);
    }

    /// The row's levels (2026-11-23): High is the spacing as designed, Normal and Performance twice it,
    /// in the uniform (the march multiplies nothing of its own) and so in the coarse step; the fine step
    /// is the same at each. The run's near block is 8 cells at High and 16, the block's most, at twice
    /// the spacing; a cap shortens a block, so the probes are never further apart than asked for.
    #[test]
    fn the_levels_double_the_probe_spacing_within_the_block_cap() {
        assert_eq!(CLOUD_CHOICES[1..], [CloudLevel::Performance, CloudLevel::Normal, CloudLevel::High].map(CloudLevel::name));
        assert_eq!(CloudLevel::default(), CloudLevel::Normal);
        assert_eq!([CloudLevel::Performance, CloudLevel::Normal, CloudLevel::High].map(CloudLevel::buffer_scale), [4.0, 3.0, crate::medium::BUFFER_SCALE]);
        assert_eq!([CloudLevel::Performance, CloudLevel::Normal, CloudLevel::High].map(CloudLevel::probe_factor), [2.0, 2.0, 1.0]);
        assert_eq!([None, Some(f32::NAN), Some(0.0), Some(1.0), Some(2.0), Some(1e9)].map(probe_factor), [1.0, 1.0, 1.0, 1.0, 2.0, 4.0]);
        let at = |probe: Option<f32>, domain: f32, amount: f32| {
            uniform(&VolumeInputs { probe, plane_height: 20_000.0, above_ground: 20_000.0, domain, horizon: HORIZON, steps: VIEW_STEPS, density: 1.0, amount, ..Default::default() })
        };
        let level = |l: CloudLevel| Some(l.probe_factor());
        for domain in [REFERENCE_HEIGHT, 24_000.0] {
            for amount in [0.05, 0.3] {
                let (none, high, normal) = (at(None, domain, amount), at(level(CloudLevel::High), domain, amount), at(level(CloudLevel::Normal), domain, amount));
                assert_eq!(high, none, "High is as designed");
                assert_eq!(normal, at(level(CloudLevel::Performance), domain, amount));
                assert_eq!(normal.profile.z, 2.0 * high.profile.z);
                assert!(needs_write(&high, &normal), "a live change of the level is written");
                let mut same = normal;
                same.profile.z = high.profile.z;
                assert_eq!(same, high, "nothing else of the uniform changes");
                for t in [0.0f32, 20_000.0, 200_000.0] {
                    let dir = Vec3::new(0.6, 0.8, 0.0);
                    assert_eq!(coarse_step(dir, t, c1_spread(), &normal), 2.0 * coarse_step(dir, t, c1_spread(), &high));
                    assert_eq!(fine_step(t, c1_spread(), &normal), fine_step(t, c1_spread(), &high));
                }
            }
        }
        let cells = |u: &CloudVolumeUniform| (coarse_step(Vec3::Y, 0.0, 0.0, u) / fine_step(0.0, 0.0, u)).floor();
        let (high, normal) = (at(level(CloudLevel::High), 24_000.0, 0.3), at(level(CloudLevel::Normal), 24_000.0, 0.3));
        assert_eq!((high.profile.z, normal.profile.z, fine_step(0.0, 0.0, &high)), (960.0, 1_920.0, 120.0));
        assert_eq!((cells(&high), cells(&normal)), (8.0, REFINE_BLOCK_MOST as f32));
    }

    /// A hard edge is found within one fine cell at the entry and at the exit,
    /// the integrated length is the analytic occupied length plus at most one cell per crossing, the
    /// optical depth the analytic one within a cell per edge.
    #[test]
    fn the_refined_march_brackets_a_hard_edge_to_one_cell() {
        let u = c1_grid_uniform(HORIZON, C1_COARSE);
        let (o, d) = (Vec3::new(0.0, 25_000.0, 0.0), Vec3::Y);
        let sigma = 2e-4;
        for offset in [0.02f32, 0.5, 0.98] {
            let run = c1_run(o, d, &u, &|t| (10_250.0..=13_000.0).contains(&t), &|_| sigma, offset);
            let (lo, hi) = (run.cells[0].0, run.cells.last().unwrap().1);
            assert!(lo <= 10_250.0 && 10_250.0 - lo < C1_STEP, "the entry within one cell: {lo}");
            assert!(hi >= 13_000.0 && hi - 13_000.0 < C1_STEP, "the exit within one cell: {hi}");
            let length: f32 = run.cells.iter().map(|c| c.1 - c.0).sum();
            assert!((2_750.0..=2_750.0 + 2.0 * C1_STEP + 1e-2).contains(&length), "integrated {length}");
            assert_eq!((run.fine, run.probes), (24, 1 + 73 + 4), "24 cells (10,200 to 13,080); the start, 73 blocks, two halvings each way");
            assert_eq!(run.taps, LIGHT_STEPS * run.samples.iter().filter(|t| (10_250.0..=13_000.0).contains(*t)).count() as u32, "eight taps on each lit sample");
            let depth = -run.trans.ln();
            assert!((depth - sigma * 2_750.0).abs() <= 2.0 * sigma * C1_STEP + 1e-4, "optical depth {depth}");
        }
    }

    /// A chord between two probes (a puff narrower than the coarse spacing, or a grazing crown's
    /// tangent chord `2 sqrt(2 rho (top - y))`) is found exactly when a probe node lies in it, and a
    /// chord longer than the spacing always (the documented limit: asserted,
    /// not luck).
    #[test]
    fn a_chord_between_two_probes_is_found_exactly_when_a_probe_lands_in_it() {
        let u = c1_grid_uniform(HORIZON, C1_COARSE);
        let (o, d) = (Vec3::new(0.0, 25_000.0, 0.0), Vec3::Y);
        let crown = |rho: f32, below: f32| 2.0 * (2.0 * rho * below).sqrt();
        let mut chords = vec![90.0f32, 210.0, 450.0, 481.0, 2_500.0];
        chords.extend([1.0f32, 10.0, 50.0, 100.0, 300.0].map(|below| crown(5_000.0, below)));
        let (mut found, mut missed) = (0, 0);
        for &len in &chords {
            for k in 0..200 {
                let c = 137.0 * k as f32;
                if c + len > 34_000.0 {
                    break;
                }
                let run = c1_run(o, d, &u, &|t| (c..=c + len).contains(&t), &|_| 1e-4, 0.5);
                let node_in = (0..=72).any(|j| (c..=c + len).contains(&(C1_COARSE * j as f32)));
                assert_eq!(run.fine > 0, node_in, "chord {len} at {c}");
                if len > C1_COARSE {
                    assert!(run.fine > 0, "longer than the spacing: always found ({len} at {c})");
                }
                if node_in { found += 1 } else { missed += 1 }
            }
        }
        println!("chords between probes: {found} found, {missed} missed (shorter than {C1_COARSE} and holding no probe)");
        assert!(missed > 0 && found > 0);
    }

    /// A ray that starts inside cloud integrates from its first cell; the exit is bracketed.
    #[test]
    fn the_refined_march_starts_inside_cloud() {
        let u = c1_grid_uniform(HORIZON, C1_COARSE);
        let run = c1_run(Vec3::new(0.0, 25_000.0, 0.0), Vec3::Y, &u, &|t| (0.0..=2_000.0).contains(&t), &|_| 1e-4, 0.5);
        assert_eq!(run.cells[0].0, 0.0, "from the start");
        let hi = run.cells.last().unwrap().1;
        assert!((2_000.0..=2_000.0 + C1_STEP).contains(&hi), "the exit cell: {hi}");
        assert!(run.opacity() > 0.15);
    }

    /// The base's 300-unit rounding in: a density rising from nothing over 300 units is integrated
    /// from where the support begins, its optical depth the analytic one within a cell per edge.
    #[test]
    fn the_refined_march_keeps_a_soft_ramp() {
        let u = c1_grid_uniform(HORIZON, C1_COARSE);
        let sigma = 1e-4;
        let run = c1_run(Vec3::new(0.0, 25_000.0, 0.0), Vec3::Y, &u, &|t| (9_000.0..=12_000.0).contains(&t), &|t| sigma * smoothstep(9_000.0, 9_300.0, t), 0.5);
        assert!(run.cells[0].0 <= 9_000.0 && run.cells.last().unwrap().1 >= 12_000.0);
        let depth = -run.trans.ln();
        let want = sigma * (3_000.0 - 150.0);
        assert!((depth - want).abs() <= 2.0 * sigma * C1_STEP, "optical depth {depth} against {want}");
    }

    /// The base band (the highest base plus a sheet, `guard`) is always integrated: a 380-unit
    /// sheet in it that no probe meets is found from the ground, while the same sheet higher up
    /// between two probes is missed (the documented limit the guard exists for).
    #[test]
    fn the_base_band_is_always_integrated() {
        let u = c1_grid_uniform(HORIZON, C1_COARSE);
        let (o, d) = (Vec3::ZERO, Vec3::Y);
        assert_eq!(spans(o, d, 0.0, crate::medium::RAY_END, &u)[0], (20_000.0, 60_000.0));
        assert!(guard(o, d, 20_000.0, 20_480.0, &u) && guard(o, d, 21_440.0, 21_920.0, &u) && !guard(o, d, 21_920.0, 22_400.0, &u));
        let low = c1_run(o, d, &u, &|t| (20_050.0..=20_430.0).contains(&t), &|_| 3e-4, 0.5);
        assert!(low.opacity() > 0.08, "the low sheet: {}", low.opacity());
        assert!(!low.probed.iter().any(|t| (20_050.0..=20_430.0).contains(t)), "no probe met it");
        // The probes from the stretch's start at 20,000 every 480: 29,600 and 30,080 bracket a
        // sheet at 29,650..30,030 (380 units, shorter than the spacing, no probe in it).
        let high = c1_run(o, d, &u, &|t| (29_650.0..=30_030.0).contains(&t), &|_| 3e-4, 0.5);
        assert!(!high.probed.iter().any(|t| (29_650.0..=30_030.0).contains(t)) && high.probed.contains(&29_600.0) && high.probed.contains(&30_080.0), "the probes around it: {:?}", high.probed);
        assert_eq!(high.opacity(), 0.0, "between two probes above the band: missed");
        // A level ray's bent height: the band at its lowest point counts.
        let level = Vec3::X;
        assert!(guard(Vec3::new(0.0, 21_000.0, 0.0), level, 0.0, 1_000.0, &u) && !guard(Vec3::new(0.0, 30_000.0, 0.0), level, 0.0, 1_000.0, &u));
        assert!(guard(Vec3::new(0.0, 25_000.0, 0.0), Vec3::new(1.0, -0.2, 0.0).normalize(), 10_000.0, 30_000.0, &u), "a ray dipping through the band");
    }

    /// A split ray: the near sliver and the far stretch, each faded by its
    /// own entry (the far one partly, 0.35), the second marched with the fade-adjusted transmittance in
    /// front, a nonzero t_min; the refined march composes as the dense one.
    #[test]
    fn the_refined_march_composes_a_split_ray_as_the_dense_one() {
        let u = c1_grid_uniform(MAX_HORIZON, COARSE_FAIR);
        let (o, d) = (Vec3::new(0.0, 20_000.5, 0.0), Vec3::new(1.0, -0.012, 0.0).normalize());
        let s = spans(o, d, 0.0, crate::medium::RAY_END, &u);
        let fade = horizon_fade(s[1].0, u.slab.w);
        assert!(s[0].1 < 100.0 && s[1].0 > 1.0e6 && (0.2..0.5).contains(&fade), "{s:?}, far fade {fade}");
        let near = s[0].1;
        let sigma = move |t: f32| if t <= near { 0.01 } else { 1e-4 };
        let support = |_: f32| true;
        let density = move |t: f32| (sigma(t), 0.5);
        let field = RayField { support: &support, density: &density, stepped: None };
        for t_min in [0.0f32, 20.0] {
            let r = refined_march(o, d, c1_spread(), (t_min, crate::medium::RAY_END), 0.5, &u, &field);
            let m = m_dense(o, d, c1_spread(), (t_min, crate::medium::RAY_END), 0.5, 1, &u, &field);
            c1_check_cells(&r, spans(o, d, t_min, crate::medium::RAY_END, &u), t_min);
            let want_near = 1.0 - (-0.01 * (near - t_min)).exp();
            println!("split ray, t_min {t_min}: refined opacity {:.4} distance {:.0}, dense {:.4} {:.0}", r.opacity(), r.distance(), m.opacity(), m.distance());
            assert!((r.opacity() - m.opacity()).abs() < 0.01 && r.opacity() > want_near + 0.1, "the far stretch shows through its own fade");
            assert!((r.distance() - m.distance()).abs() < 1_000.0);
        }
    }

    /// The work bound: the settings that ask the most (quality 128, an
    /// 80,000-unit domain, one ray per pixel of a 1440-pixel-tall render, a level ray inside the
    /// domain, cloud all along that is lit and never opaque) grow every fine cell together, and the
    /// ray spends at most RAY_BUDGET evaluations (probes, fine samples, light taps).
    #[test]
    fn the_refined_march_holds_a_ray_to_the_budget() {
        let mut u = uniform(&VolumeInputs { plane_height: 20_000.0, above_ground: 20_000.0, domain: MAX_DOMAIN, horizon: HORIZON, steps: MAX_STEPS, density: 1.0, sky_scale: 1.0, light_strength: 1.0, amount: 0.3, ..Default::default() });
        u.light_dir.w = 1.0;
        let spread = 2.0 * (59.84f32.to_radians() * 0.5).tan() / 1440.0;
        let (o, d) = (Vec3::new(0.0, 21_000.0, 0.0), Vec3::X);
        let stretches = spans(o, d, 0.0, crate::medium::RAY_END, &u);
        let (grow, capped) = refined_plan(stretches, spread, &u);
        assert!(capped && grow > 1.0 && refined_rays_capped(o, spread, &u), "grow {grow}");
        assert_eq!(refined_cells_most(&u), 2_504.0);
        let support = |_: f32| true;
        let density = |_: f32| (1e-12, 0.03);
        let field = RayField { support: &support, density: &density, stepped: None };
        let run = refined_march(o, d, spread, (0.0, crate::medium::RAY_END), 0.5, &u, &field);
        c1_check_cells(&run, stretches, 0.0);
        println!("capped ray: grow {grow:.2}, {} probes, {} fine, {} taps: {} evaluations (budget {RAY_BUDGET})", run.probes, run.fine, run.taps, run.evals());
        assert!(run.evals() <= RAY_BUDGET && run.fine > 100, "{} evaluations", run.evals());
        for (t0, t1) in stretches.into_iter().filter(|s| s.1 > s.0) {
            for c in run.cells.iter().filter(|c| c.0 >= t0 && c.1 < t1 - 1.0) {
                assert!(c.1 - c.0 >= grow * fine_step(t0, spread, &u) * 0.999, "wider than the bound asks: {c:?}");
            }
        }
        // The defaults: no camera on the ground, below it or far above the domain is capped.
        let def = c1_uniform(HORIZON, COARSE_FAIR);
        for y in [0.0f32, 10_000.0, 100_000.0] {
            assert!(!refined_rays_capped(Vec3::new(0.0, y, 0.0), c1_spread(), &def), "the defaults from {y}");
        }
    }

    pub(super) fn c1_dir(elevation: f32, azimuth: f32) -> Vec3 {
        let (el, az) = (elevation.to_radians(), azimuth.to_radians());
        Vec3::new(el.cos() * az.cos(), el.sin(), el.cos() * az.sin())
    }

    /// On the real field: paired reference (dense) and refined marches, the
    /// production noise bytes, amounts 0.3 and 0.8, rays 2, 10, 45 and 90 degrees up from three
    /// places on the ground (eight azimuths, one straight up), start offsets 0.02, 0.5 and 0.98. The
    /// tolerances are frozen from the dense march's own variation before the refined march is looked
    /// at: three times the larger of the dense-against-half-step and the dense phase (offset to offset)
    /// differences, mean and 95th percentile over the visible pairs (dense opacity 0.02 or more),
    /// capped by ceilings (0.01 and 0.03). Misses (the refined under a quarter of a
    /// visible dense opacity) and the signed bias are reported apart; the distance of substantial hits
    /// (both over 0.3) within the fine step there plus the dense distances' spread over the offsets.
    /// A numerical mirror, not committed (the near crown bound
    /// holding): at 0.3 dense half step 0.0013 / 0.0037 (mean / p95), phase 0.0016 / 0.0047, refined
    /// 0.0013 / 0.0073, no miss; at 0.8 0.0008 / 0.0028, 0.0009 / 0.0032, refined 0.0006 / 0.0030, no
    /// miss.
    #[test]
    fn interior_stride_keeps_the_exit_cell_and_limits_spacing_and_optical_depth() {
        assert_eq!(interior_stride(4, 8, 200.0, 0.0, 0.0001), 2);
        assert_eq!(interior_stride(4, 8, 200.0, 100_000.0, 0.0001), 4);
        assert_eq!(interior_stride(2, 8, 200.0, 100_000.0, 0.0001), 2);
        assert_eq!(interior_stride(4, 3, 200.0, 100_000.0, 0.0001), 2);
        assert_eq!(interior_stride(4, 1, 200.0, 100_000.0, 0.0001), 1);
        assert_eq!(interior_stride(4, 8, 200.0, 100_000.0, 0.002), 1);
    }

    /// The refined march's shader is its mirror's (`flat_stretch`): the support, the fine and coarse steps,
    /// the guard, the blocks and their halvings, the cells and their samples, the plan.
    #[test]
    fn the_refined_march_mirror_matches_the_march_module() {
        let src = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        let body = |head: &str| {
            let march = &src[src.find(head).unwrap_or_else(|| panic!("{head}"))..];
            march[..march.find("\n}\n").unwrap()].to_string()
        };
        assert_eq!(src.matches("fn vc_march_refined(").count(), 1);
        let flat = body("\nfn vc_march_refined(");
        for line in [
            "let turns = 1u + blocks * (5u + VC_BLOCK_MOST * (1u + taps));",
            "f = vc_fine_step(b, spread, v) * grow;",
            "e = b + f32(cells) * f;",
            "at = vc_node(b, f, (lo + hi) / 2u, n, e);",
            "qpwidth = vc_place_width(q, ray.dir, qwidth, v);",
            "q = rel + ld * (s + 0.5 * ds);",
            "let in_domain = !(yb <= v.slab.x || yb >= ceiling);",
            "let fw = select(qwidth, vc_detail_filter(qwidth, dt), viewed);",
            "found = vc_shape_with_column(pos, yb, col, qwidth, qpwidth, nb, ns, ne, viewed, true, fw, v, &prepared);",
            "whole = (occ_s && occ_e) || vc_guard(ray, b, e, v);",
            "settled = whole || occ_s == occ_e || n <= 1u;",
            "if inside == occ_e {\n                    hi = mid;\n                } else {\n                    lo = mid;\n                }\n                halvings += 1u;",
            "settled = hi - lo <= 1u || halvings >= 4u;",
            "i = 0u;\n                c_to = 0u;\n                if whole {\n                    c_to = n;\n                } else if occ_e {\n                    i = lo;\n                    c_to = n;\n                } else if occ_s {\n                    c_to = lo + 1u;\n                }\n                if i > 0u { cache.valid = false; }",
            "lo = 0u;\n            hi = n;\n            halvings = 0u;",
            "interior_block = stride_limit >= 2u && occ_s && occ_e;",
            "ds = vc_light_first(rel, ray.origin, taps, v);",
            "optical += found.x * found.y * ds;",
            "let given = vc_light_handoff(rel + ld * s, ray.origin, optical, ns, v);",
            "if j >= taps || keep <= 0.0 {",
            "if k >= blocks || b >= t1 {",
        ] {
            assert!(flat.contains(line), "the flat refined march no longer has `{line}`: its mirror must follow");
        }
        assert!(body("\nfn vc_detail_filter(").ends_with("    return max(width, max(dt, 0.0));"), "the view's filter: the wider of the cast width and the step");
        // One place for the column and one for the density, whatever the point.
        assert_eq!((flat.matches("vc_m2_column(").count(), flat.matches("vc_shape_with_column(").count()), (1, 1));
        for line in [
            "vc_count_turn(&work, turn);",
            "m.work = work;",
            "let cells = clamp(u32(clamp(floor(vc_coarse_step(ray, b, v) / f), 1.0, f32(VC_BLOCK_MOST))), 1u, VC_BLOCK_MOST);",
            "n = clamp(u32(clamp(ceil((t1 - b) / f), 1.0, f32(VC_BLOCK_MOST))), 1u, cells);",
            "c_to = lo + 1u;",
            "if !interior_block || stride_limit < 4u {",
            "let c0 = vc_node(b, f, i, n, e);",
            "&& (c0 >= 55000.0 || m.trans * ahead < 0.40);",
            "let length_limit = min(1000.0 + 1000.0 * smoothstep(15000.0, 85000.0, c0), 0.85 / previous_sigma);",
            "stride = max(1u, min(min(limit, c_to - i), u32(floor(length_limit / f))));",
            "stride = vc_interior_stride(stride_limit, c_to - i, f, c0, previous_sigma);",
            "dt = vc_node(b, f, i, n, e) - c0;",
            "t = c0 + offset * dt;",
            "let interior = d.x > max(2.0 * v.march.w, 0.12) && sigma > 0.0;",
            "stable = interior && previous_sigma > 0.0 && abs(sigma - previous_sigma) <= 0.22 * max(sigma, previous_sigma);",
            "allow_reuse && vc_reuse_incident(cache, d, t, v.march.w);",
            "cache = VcIncidentCache(emitted, t, d.x * d.y, d.w, sampled && d.x > 2.0 * v.march.w);",
            "if c_to < n { cache.valid = false; }",
            "let taken = m.trans * (1.0 - kept);",
            "if m.trans * ahead < v.reach.z {",
        ] {
            assert!(flat.contains(line), "the flat refined march no longer has `{line}`: its mirror must follow");
        }
        for line in [
            "const VC_BLOCK_MOST: u32 = 16u;",
            "const VC_COUNT_PROBES: f32 = 512.0;",
            "const VC_COUNT_FINE: f32 = 256.0;",
            "const VC_COUNT_TAPS: f32 = 2048.0;",
            "let sheet2 = col.res < 1.0 && above < max(v.field.w, 1.0) && v.profile.y > 0.0;",
            "return max(min(max(v.march.x, v.flags.w * w), bound), 1.0);",
            "let finest = 1.0 / max(v.map.x * v.field.x * VC_PLACE_F2, 1e-9);",
            "return max(v.profile.z, 1.0) * max(pw / finest, 1.0);",
            "return lo < v.slab.x + v.height.w + v.field.w && max(yb, ye) > v.slab.x;",
            "let bound = min(min(min(exp2(lod) / max(v.nscale.x * v.nlevel.x, 1e-9), 0.5 * v.field.w), vc_m2_erosion_bound(w, v)), vc_m2_edge_bound(w));",
            "let most = max(floor((v.reach.w - 2.0) / (2.0 + taps)) - 4.0, 1.0);",
            "let grow = max((want.x + want.y) / most, 1.0);",
            "let m = vc_march_refined(ray, t0, t1, grow, blocks, offset, trans, nb, ns, ne, v);",
            "return vc_count_rgb(work, dist);",
        ] {
            assert!(src.contains(line), "the march module no longer has `{line}`: the mirror must follow");
        }
        assert_eq!((REFINE_BLOCK_MOST, COUNT_PROBES, COUNT_FINE, COUNT_TAPS), (16, 512.0, 256.0, 2_048.0), "the constants the march writes as literals");
    }

    // --- #3032: the flat refined march (march `vc_march_refined`) against the refined march's mirror in
    // nested loops (`refined_march`), on made-up media. ---

    /// What a refined march does, in order: a probe; a cell (its ends) and its sample; a tap toward the
    /// light (the sample's distance, the tap's along the light); the light volume read (the sample's, the
    /// handoff's); a cell shaded (the sample's distance, whether with the cell before's light, the depth
    /// toward the light where it was taken).
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Turn {
        Probe(f32),
        Sample(f32, f32, f32),
        Tap(f32, f32),
        Handoff(f32, f32),
        Shade(f32, bool, Option<f32>),
    }

    /// The weight and depth of each light volume level at a handoff (none: the level is off).
    type FlatLevels = [Option<(f32, f32)>; 2];

    /// A made-up medium for the refined marches: the ray's field, its height share, and the light of a
    /// sample at `t`: the first interval, the optical density at a tap (`t`, along the light), the near
    /// taps asked for, and the levels of a light volume at a handoff (weight and depth; none: no volume).
    struct FlatField<'a> {
        ray: RayField<'a>,
        height: &'a dyn Fn(f32) -> f32,
        first: &'a dyn Fn(f32) -> f32,
        tap: &'a dyn Fn(f32, f32) -> f32,
        near: u32,
        volume: Option<&'a dyn Fn(f32, f32) -> FlatLevels>,
    }

    impl FlatField<'_> {
        /// march `vc_near_count` (at least one near tap).
        fn near(&self, taps: u32) -> u32 {
            if self.volume.is_some() { self.near.max(1).min(taps) } else { taps }
        }

        /// march `vc_light_handoff`.
        fn handoff(&self, t: f32, s: f32, near: f32, log: &mut Vec<Turn>) -> (f32, f32) {
            let (mut optical, mut keep) = (near, 1.0f32);
            if let Some(volume) = self.volume {
                log.push(Turn::Handoff(t, s));
                for (w, depth) in volume(t, s).into_iter().flatten() {
                    if keep > 0.0 {
                        optical += keep * w * depth;
                        keep *= 1.0 - w;
                    }
                }
            }
            (optical, keep)
        }

        /// The light in one piece: the near taps, the handoff, the rest where weight is left.
        fn light(&self, t: f32, taps: u32, log: &mut Vec<Turn>) -> f32 {
            let mut at = vec![(0.0f32, (self.first)(t))];
            for j in 0..taps as usize {
                let (s, ds) = at[j];
                at.push((s + ds, ds * LIGHT_GROWTH));
            }
            let tap = |j: usize, log: &mut Vec<Turn>| {
                let (s, ds) = at[j];
                log.push(Turn::Tap(t, s + 0.5 * ds));
                (self.tap)(t, s + 0.5 * ds) * ds
            };
            let k = self.near(taps) as usize;
            let mut optical = 0.0;
            for j in 0..k {
                optical += tap(j, log);
            }
            if self.volume.is_none() {
                return optical;
            }
            let (mut optical, keep) = self.handoff(t, at[k].0, optical, log);
            if keep > 0.0 {
                let mut rest = 0.0;
                for j in k..taps as usize {
                    rest += tap(j, log);
                }
                optical += keep * rest;
            }
            optical
        }
    }

    /// march `VcIncidentCache`, the light's colour aside.
    #[derive(Clone, Copy, Default)]
    struct FlatCache {
        t: f32,
        extinction: f32,
        height: f32,
        valid: bool,
    }

    /// march `vc_reuse_incident`: the sample at `t` of the optical density `sigma`, the shape `shape` and
    /// the height share `height`.
    fn m_reuse(cache: FlatCache, (sigma, shape): (f32, f32), height: f32, t: f32, threshold: f32) -> bool {
        let distance = t - cache.t;
        let larger = cache.extinction.max(sigma);
        cache.valid && shape > 2.0 * threshold && distance <= 2_500.0 && (height - cache.height).abs() <= 0.10 && (sigma - cache.extinction).abs() <= 0.30 * larger && distance * larger <= 0.45
    }

    /// One stretch of the flat refined march (march `vc_march_refined`), statement for
    /// statement: the turns in order, then the transmittance, the hidden share and its distance.
    #[allow(clippy::too_many_arguments)]
    fn flat_stretch((origin, dir, spread): (Vec3, Vec3, f32), (t0, t1): (f32, f32), grow: f32, blocks: u32, offset: f32, ahead: f32, u: &CloudVolumeUniform, f: &FlatField, log: &mut Vec<Turn>) -> (f32, f32, f32) {
        #[derive(Clone, Copy, PartialEq)]
        enum At {
            Start,
            End,
            Halve,
            Sample,
            Tap,
        }
        let node = |b: f32, fs: f32, i: u32, n: u32, e: f32| if i >= n { e } else { b + i as f32 * fs };
        let (mut mt, mut mh, mut mht) = (1.0f32, 0.0f32, 0.0f32);
        let allow_reuse = u.families.w > 0.5 && u.light_dir.w > 0.0;
        let stride_limit = u.families.x as u32;
        let taps = (u.march.y as u32).clamp(1, 16);
        let near = f.near(taps);
        let turns = 1 + blocks * (5 + REFINE_BLOCK_MOST * (1 + taps));
        let mut cache = FlatCache::default();
        let mut turn = At::Start;
        let (mut b, mut e, mut fs, mut n, mut k) = (t0, t0, 1.0f32, 1u32, 0u32);
        let (mut occ_s, mut occ_e) = (false, false);
        let (mut lo, mut hi, mut halvings) = (0u32, 1u32, 0u32);
        let (mut i, mut c_to) = (0u32, 0u32);
        let (mut interior_block, mut stable, mut previous_sigma) = (false, false, 0.0f32);
        let (mut t, mut dt, mut d, mut cell) = (0.0f32, 0.0f32, (0.0f32, 0.0f32), 0.0f32);
        let (mut j, mut s, mut ds) = (0u32, 0.0f32, 0.0f32);
        let (mut handed, mut optical, mut keep) = (0.0f32, 0.0f32, 1.0f32);
        let mut spent = 0u32;
        loop {
            assert!(spent < turns, "the bound cuts a ray: {spent} turns, {blocks} blocks");
            spent += 1;
            let (mut reuse, mut depth, mut shade, mut next_cell, mut next_block) = (false, None, false, false, false);
            match turn {
                At::Start | At::End | At::Halve => {
                    let at = if turn == At::Halve { node(b, fs, (lo + hi) / 2, n, e) } else { e };
                    log.push(Turn::Probe(at));
                    let inside = (f.ray.support)(at);
                    let (mut whole, mut settled) = (false, false);
                    if turn == At::Start {
                        occ_s = inside;
                        next_block = true;
                    } else if turn == At::End {
                        occ_e = inside;
                        whole = (occ_s && occ_e) || guard(origin, dir, b, e, u);
                        settled = whole || occ_s == occ_e || n <= 1;
                        turn = At::Halve;
                    } else {
                        let mid = (lo + hi) / 2;
                        if inside == occ_e {
                            hi = mid;
                        } else {
                            lo = mid;
                        }
                        halvings += 1;
                        settled = hi - lo <= 1 || halvings >= 4;
                    }
                    if settled {
                        (i, c_to) = if whole {
                            (0, n)
                        } else if occ_e {
                            (lo, n)
                        } else if occ_s {
                            (0, lo + 1)
                        } else {
                            (0, 0)
                        };
                        if i > 0 {
                            cache.valid = false;
                        }
                        interior_block = stride_limit >= 2 && occ_s && occ_e;
                        if !interior_block || stride_limit < 4 {
                            stable = false;
                            previous_sigma = 0.0;
                        }
                        next_cell = true;
                    }
                }
                At::Sample => {
                    log.push(Turn::Sample(cell, cell + dt, t));
                    d = f.ray.at(t, dt);
                    let (sigma, shape) = d;
                    if interior_block {
                        let interior = shape > (2.0 * u.march.w).max(0.12) && sigma > 0.0;
                        stable = interior && previous_sigma > 0.0 && (sigma - previous_sigma).abs() <= 0.22 * sigma.max(previous_sigma);
                        previous_sigma = if interior { sigma } else { 0.0 };
                    }
                    if shape <= 0.0 {
                        cache.valid = false;
                        next_cell = true;
                    } else {
                        reuse = allow_reuse && m_reuse(cache, d, (f.height)(t), t, u.march.w);
                        if !reuse && shape > u.march.w && u.light_dir.w > 0.0 {
                            (j, s, ds) = (0, 0.0, (f.first)(t));
                            (optical, keep) = (0.0, 1.0);
                            turn = At::Tap;
                        } else {
                            shade = true;
                        }
                    }
                }
                At::Tap => {
                    let at = s + 0.5 * ds;
                    log.push(Turn::Tap(t, at));
                    optical += (f.tap)(t, at) * ds;
                    s += ds;
                    ds *= LIGHT_GROWTH;
                    j += 1;
                    if j == near {
                        (handed, keep) = f.handoff(t, s, optical, log);
                        optical = 0.0;
                    }
                    if j >= taps || keep <= 0.0 {
                        depth = Some(handed + keep * optical);
                        shade = true;
                    }
                }
            }
            if shade {
                let (sigma, shape) = d;
                log.push(Turn::Shade(t, reuse, depth));
                if reuse {
                    cache.valid = false;
                } else if allow_reuse {
                    cache = FlatCache { t, extinction: sigma, height: (f.height)(t), valid: depth.is_some() && shape > 2.0 * u.march.w };
                }
                let kept = (-sigma * dt).exp();
                let taken = mt * (1.0 - kept);
                mh += taken;
                mht += taken * t;
                mt *= kept;
                if mt * ahead < u.reach.z {
                    break;
                }
                next_cell = true;
            }
            if next_cell {
                if i < c_to {
                    let c0 = node(b, fs, i, n, e);
                    let mut stride = 1;
                    if interior_block && stride_limit >= 4 && previous_sigma > 0.0 && (c0 >= 55_000.0 || mt * ahead < 0.40) {
                        let limit: u32 = if stable { 4 } else { 2 };
                        let length = (1000.0 + 1000.0 * smoothstep(15_000.0, 85_000.0, c0)).min(0.85 / previous_sigma);
                        stride = limit.min(c_to - i).min((length / fs).floor() as u32).max(1);
                    } else if interior_block && stable {
                        stride = interior_stride(stride_limit, c_to - i, fs, c0, previous_sigma);
                    }
                    i += stride;
                    cell = c0;
                    dt = node(b, fs, i, n, e) - c0;
                    t = c0 + offset * dt;
                    turn = At::Sample;
                } else {
                    if c_to < n {
                        cache.valid = false;
                    }
                    occ_s = occ_e;
                    b = e;
                    next_block = true;
                }
            }
            if next_block {
                if k >= blocks || b >= t1 {
                    break;
                }
                k += 1;
                fs = fine_step(b, spread, u) * grow;
                let most = REFINE_BLOCK_MOST as f32;
                let cells = ((coarse_step(dir, b, spread, u) / fs).floor().clamp(1.0, most) as u32).clamp(1, REFINE_BLOCK_MOST);
                (e, n) = (b + cells as f32 * fs, cells);
                if e >= t1 {
                    e = t1;
                    n = (((t1 - b) / fs).ceil().clamp(1.0, most) as u32).clamp(1, cells);
                }
                (lo, hi, halvings) = (0, n, 0);
                turn = At::End;
            }
        }
        (mt, mh, mht)
    }

    /// The flat march of a ray (march `vc_trace_refined` over `flat_stretch`): the turns, and the ray's
    /// transmittance, hidden share and its distance.
    fn flat_march(ray: (Vec3, Vec3, f32), offset: f32, u: &CloudVolumeUniform, f: &FlatField) -> (Vec<Turn>, [f32; 3]) {
        let stretches = spans(ray.0, ray.1, 0.0, crate::medium::RAY_END, u);
        let (grow, _) = refined_plan(stretches, ray.2, u);
        let blocks = refined_cells_most(u) as u32 + 4;
        let (mut log, mut trans, mut hidden, mut hidden_t) = (Vec::new(), 1.0f32, 0.0f32, 0.0f32);
        for (t0, t1) in stretches {
            let fade = horizon_fade(t0, u.slab.w);
            if t1 <= t0 || trans < u.reach.z || fade <= 0.0 {
                continue;
            }
            let (mt, mh, mht) = flat_stretch(ray, (t0, t1), grow, blocks, offset, trans, u, f, &mut log);
            hidden += trans * fade * mh;
            hidden_t += trans * fade * mht;
            trans *= 1.0 - (1.0 - mt) * fade;
        }
        (log, [trans, hidden, hidden_t])
    }

    /// The turns the refined march's mirror takes: its probes and samples in order, and after each sample
    /// the light taken there: the light of the cell before where that cell adjoins it (nothing skipped
    /// between them), was itself lit in full, and may be reused; else the march toward the light. The flat
    /// march's cache says the same of the adjoining cell at its own five places (an entry past the first
    /// cell, an empty sample, a reuse, an exit before the last cell, a new stretch).
    fn nested_turns(ray: (Vec3, Vec3, f32), offset: f32, u: &CloudVolumeUniform, f: &FlatField) -> (Vec<Turn>, MarchRun) {
        let order = std::cell::RefCell::new(Vec::new());
        let support = |t: f32| {
            order.borrow_mut().push((t, None));
            (f.ray.support)(t)
        };
        let stepped = |t: f32, dt: f32| {
            order.borrow_mut().push((t, Some(dt)));
            f.ray.at(t, dt)
        };
        let density = |_: f32| -> (f32, f32) { unreachable!() };
        let run = refined_march(ray.0, ray.1, ray.2, (0.0, crate::medium::RAY_END), offset, u, &RayField { support: &support, density: &density, stepped: Some(&stepped) });
        let second = spans(ray.0, ray.1, 0.0, crate::medium::RAY_END, u)[1];
        let taps = (u.march.y as u32).clamp(1, 16);
        let mut log = Vec::new();
        let mut cells = run.cells.iter();
        let mut before: Option<(f32, bool, FlatCache)> = None;
        for (t, dt) in order.into_inner() {
            let Some(dt) = dt else {
                log.push(Turn::Probe(t));
                continue;
            };
            let &(lo, hi) = cells.next().expect("a cell for each sample");
            log.push(Turn::Sample(lo, hi, t));
            let (sigma, shape) = f.ray.at(t, dt);
            if shape <= 0.0 {
                before = None;
                continue;
            }
            let stretch = second.1 > second.0 && lo >= second.0;
            let cache = before.filter(|&(end, s, _)| s == stretch && (end - lo).abs() < 0.25).map_or(FlatCache::default(), |(_, _, c)| c);
            let height = (f.height)(t);
            let reuse = u.families.w > 0.5 && u.light_dir.w > 0.0 && m_reuse(cache, (sigma, shape), height, t, u.march.w);
            let depth = (!reuse && shape > u.march.w && u.light_dir.w > 0.0).then(|| f.light(t, taps, &mut log));
            log.push(Turn::Shade(t, reuse, depth));
            before = (!reuse).then_some((hi, stretch, FlatCache { t, extinction: sigma, height, valid: depth.is_some() && shape > 2.0 * u.march.w }));
        }
        assert!(cells.next().is_none());
        (log, run)
    }

    /// The flat march takes the refined march's turns: over many rays and made-up media (blocks whole by
    /// their ends and by the guard, entered, left and empty, of one cell up to sixteen, clipped at the
    /// stretch's end; hollow and unlit cells; the strides and the reuse on and off; the light volume none,
    /// whole, in part and empty, its near taps 0 to all), the same probes, cells, samples, taps, handoffs,
    /// reuse and light depths in the same order, and the same transmittance and distance, to the bit;
    /// never cut by its bound of turns.
    #[test]
    fn the_flat_march_takes_the_refined_marchs_turns() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 40) as f32 / (1u64 << 24) as f32
        };
        // A value in 0..1 of a place.
        let hash = |x: f32, salt: u32| {
            let mut h = x.to_bits() ^ salt.wrapping_mul(0x9e37_79b9);
            h = (h ^ (h >> 16)).wrapping_mul(0x85eb_ca6b);
            h = (h ^ (h >> 13)).wrapping_mul(0xc2b2_ae35);
            ((h ^ (h >> 16)) >> 8) as f32 / (1u32 << 24) as f32
        };
        // What the media reached: stops, cells of a block the guard alone integrates, merged cells, reuses,
        // brackets of four halvings, one-cell exits, lights ended at the handoff, and taken on from it.
        let mut seen = [0u32; 8];
        for case in 0..1_200u32 {
            let mut pick = |of: &[f32]| of[((next() * of.len() as f32) as usize).min(of.len() - 1)];
            let mut u = c1_grid_uniform(HORIZON, pick(&[1.0, 2.0, 4.0, 8.0, 16.0]) * C1_STEP);
            u.families.x = pick(&[0.0, 2.0, 3.0, 4.0]);
            u.families.w = pick(&[0.0, 1.0]);
            u.light_dir.w = pick(&[0.0, 0.5, 1.0, 1.0]);
            u.march.y = pick(&[1.0, 2.0, 8.0, 16.0]);
            let origin = Vec3::new(0.0, pick(&[0.0, 19_000.0, 25_000.0, 45_000.0, 70_000.0]), 0.0);
            let up = pick(&[-0.4, -0.05, 0.0, 0.05, 0.3, 1.0, 1.0]);
            let dir = if up == 1.0 { Vec3::Y } else { Vec3::new(1.0, up, 0.3).normalize() };
            let offset = pick(&[0.0, 0.02, 0.5, 0.98]);
            let (length, share) = (pick(&[300.0, 1_500.0, 6_000.0, 30_000.0]), pick(&[0.0, 0.2, 0.5, 0.9]));
            let (sigma0, shape0, vary) = (pick(&[1e-6, 2e-5, 2e-4, 1e-3]), pick(&[0.015, 0.03, 0.4]), pick(&[0.05, 0.5]));
            let (near, levels) = (pick(&[0.0, 1.0, 2.0, 9.0]) as u32, pick(&[0.0, 1.0, 2.0, 3.0]) as u32);
            let support = |t: f32| hash((t / length).floor(), case) < share;
            let density = |_: f32| -> (f32, f32) { unreachable!() };
            let stepped = |t: f32, dt: f32| {
                if !support(t) || hash((t / 900.0).floor(), case ^ 1) < 0.15 {
                    return (0.0, 0.0);
                }
                (sigma0 * (1.0 + vary * hash(t, 2)) + 1e-10 * dt, shape0 * (1.0 + vary * hash(t, 3)))
            };
            let height = |t: f32| 0.5 + 0.15 * hash(t, 4);
            let first = |t: f32| 50.0 + 500.0 * hash(t, 5);
            let tap = |t: f32, s: f32| if hash(t + s, 6) < 0.3 { 0.0 } else { 2e-4 * hash(t - s, 7) };
            let volume = |t: f32, s: f32| {
                let level = |k: u32| {
                    let w = (3.0 * hash(t + s, 8 + k) - 1.0).clamp(0.0, 1.0);
                    (levels & (1 + k) != 0).then(|| (w, 3.0 * hash(t - s, 10 + k)))
                };
                [level(0), level(1)]
            };
            let field = FlatField {
                ray: RayField { support: &support, density: &density, stepped: Some(&stepped) },
                height: &height,
                first: &first,
                tap: &tap,
                near,
                volume: if case % 3 == 0 { None } else { Some(&volume) },
            };
            let ray = (origin, dir, c1_spread());
            let (want, run) = nested_turns(ray, offset, &u, &field);
            let (got, end) = flat_march(ray, offset, &u, &field);
            if let Some(k) = (0..want.len().max(got.len())).find(|&k| want.get(k) != got.get(k)) {
                panic!("case {case}: turn {k} is {:?}, the refined march's {:?}", got.get(k), want.get(k));
            }
            assert_eq!(end.map(f32::to_bits), [run.trans, run.hidden, run.hidden_t].map(f32::to_bits), "case {case}");
            // The work the mirror counts: every probe, sample and (no reuse, no volume) tap.
            let count = |of: fn(&Turn) -> bool| got.iter().filter(|x| of(x)).count() as u32;
            assert_eq!((count(|x| matches!(x, Turn::Probe(_))), count(|x| matches!(x, Turn::Sample(..)))), (run.probes, run.fine), "case {case}");
            if u.families.w == 0.0 && field.volume.is_none() {
                assert_eq!(count(|x| matches!(x, Turn::Tap(..))), run.taps, "case {case}");
            }
            seen[0] += u32::from(run.trans < u.reach.z);
            seen[1] += run.guarded;
            seen[2] += run.cells.iter().filter(|c| c.1 - c.0 > 1.5 * fine_step(c.0, c1_spread(), &u) * run.grow).count() as u32;
            seen[3] += count(|x| matches!(x, Turn::Shade(_, true, _)));
            for w in got.windows(6) {
                if let [Turn::Probe(p), Turn::Probe(a), Turn::Probe(b), Turn::Probe(c), Turn::Probe(d), Turn::Sample(..)] = w {
                    seen[4] += u32::from(a < p && b < p && c < p && d < p);
                }
            }
            seen[5] += u32::from(u.profile.z == C1_STEP && run.fine > 0);
            for w in got.windows(2) {
                seen[6] += u32::from(matches!(w, [Turn::Handoff(..), Turn::Shade(..)]));
                seen[7] += u32::from(matches!(w, [Turn::Handoff(..), Turn::Tap(..)]));
            }
        }
        println!("stops, guarded cells, merged cells, reuses, four halvings, one-cell rays, lights ended at and taken on from the handoff: {seen:?}");
        assert!(seen.iter().all(|&x| x > 20), "the media reach every case: {seen:?}");
    }

    // #3032 stage 2g: the light volume.
    include!("volumetric_clouds_lightvol_tests.rs");
    // #3032: the lean model's support, march plans, light and density on the same mirror.
    include!("volumetric_clouds_model2_tests.rs");
}
