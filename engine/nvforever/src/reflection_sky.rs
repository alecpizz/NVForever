//! The world camera's reflection cube: what authored materials reflect (#2981 reflections;
//! docs/spec-notes/pbr-materials.md "Reflections on authored materials") [design].
//! NVForever's own rendering: the original draws no ambient reflection (docs/design/layer3.md),
//! and nothing here describes it. The cube pair, its upload and Bevy's component are
//! nvf_assets `reflection_cube.rs`; this module decides whether the world camera (`FlyCam`)
//! gets one and writes its content.
//!
//! - **Whether** ([`decide`]): the run's Reflections on authored materials row on, the
//!   Authored materials row on, the mount holding at least one authored set
//!   (nvf_assets `authored::mount_has_sets`) and the reflection gate installed in
//!   `pbr_functions` (nvforever lights.rs). Then one cube is made for the world camera and no
//!   other; slice 1's rule stays: Bevy's `EnvironmentMapLight` goes on only once the first
//!   fill is on the GPU (`reflection_cube::attach_lights`). Otherwise no cube is allocated.
//! - **What** ([`SkyContent`]), per world direction d and level, with e the sine of d's
//!   elevation (Bevy's +Y is up):
//!   - outdoors, above the horizon: the sky dome's colour at e, the weather's horizon,
//!     sky-lower and sky-upper colours (blended across a weather fade,
//!     `SkyState::color`) mixed by the dome's own weights for that height ([`DomeProfile`],
//!     read from `atmosphere.nif`'s vertices when the sky is made) the way the dome mixes
//!     them (sky.rs `dome_color`), times the sky's display scale (`SkyState::sky_scale`);
//!     below it lit ground, an assumed albedo [`GROUND_ALBEDO`] under the light falling on
//!     level ground (the ambient plus the directional light times the sine of its
//!     elevation, as the fork lights a diffuse surface); and a broad lobe toward the
//!     directional light in its colour and strength (the light as update_sky aims it, the
//!     moon's at night), no disc;
//!   - indoors: the cell's ambient colour above, ground of [`GROUND_ALBEDO`] under the
//!     ambient below, and a band of the cell's fog colour at the horizon ([`FOG_BAND`]), as
//!     the water's indoor sky takes the fog colour (water.rs `update_water_sky`);
//!   - each level is the same function with a wider kernel, not a filter of the level above:
//!     each hemisphere's colour mixes toward that hemisphere's mean by level / 5, the edge
//!     between sky and ground widens ([`HORIZON_EDGE`] to [`HORIZON_EDGE_TOP`]), and the
//!     lobe widens with its total kept ([`SUN_LOBE_POWER`] halved per level).
//!
//!   No clouds, no sun disc, no scene objects, no stars or moon disc.
//! - **Indoors, the analytic room** (stage 2 version 1; nvf_assets `reflection_room.rs`):
//!   with a lamp in reach or a room box better than the default, the interior content is a
//!   lit box around the eye instead ([`Content::Room`]): the box from the room marker the eye
//!   is in ([`marker_box`], the test rooms.rs `cull` makes for `Rooms::camera_room`), else
//!   from six physics rays from the eye in play mode ([`ray_box`]), else a default box
//!   ([`default_box`]); the lamps the cell's placed lights that are on, within
//!   [`LAMP_DISTANCE`] of the eye and whose sphere reaches the box, the nearest
//!   [`MAX_LAMPS`] ([`pick_lamps`]); the cell's ambient, directional light and fog. With no
//!   lamp and only the default box, stage 1's interior content above, exactly.
//! - **Outdoors near lamps** ([`Content::SkyAndGround`]): stage 1's content plus what the
//!   placed lamps whose sphere reaches the ground near the eye add to level ground under it,
//!   and their lobes (nvf_assets `reflection_room::GroundLamps`).
//! - **Units.** A texel is the value the thing it stands for writes into the view's HDR
//!   target before tone mapping, so a perfect mirror (white, metallic 1, roughness 0:
//!   Bevy's environment term is then texel x intensity x view.exposure, intensity
//!   `light_unit()` = 1 / the default exposure) writes what the thing itself writes, and the
//!   tone mapping and grade that follow treat both alike:
//!   - the sky ([`sky_texel`]): sky.wesl writes the dome's linear colour times
//!     `SkyLight.params.x` (= `SkyState::sky_scale`, sky.rs:1101 and :1117) through
//!     the shared tonemapper input (nvf_assets tone_input.wesl `to_tonemap_input`), which is the identity while the main view
//!     shows display values clipped (lights.rs `display_clip`, the default; GAME_CLIP,
//!     lights.rs:70) and the inverse of the Tony McMapface table otherwise (`NVF_TONEMAP=tony`);
//!   - lit ground ([`ground_texel`]): the fork's display-value sum writes a diffuse surface as
//!     the linear colour of min(diffuse's display value x (ambient + direct), 1)
//!     (lights/display_shader.rs `DISPLAY_EDITS`, then x exposure / exposure), with no
//!     inverse tone map;
//!   - indoors ([`texel`]): no sky is drawn, so the ambient-matched identity stays: a
//!     display value v (with the ambient's share) is v x max(v, 1e-6)^1.2, so a direction
//!     coloured like the ambient reflects the converted ambient (nvf_assets
//!     `ReflectionCube`, "Units"). Outdoors the two rules differ: an ambient-coloured sky
//!     gives (linear colour x sky scale) by the sky rule and (0.9 x display value)^2.2 by the
//!     ambient rule, and the sky rule is the one used there.
//!
//!   The lobe is the directional light's display colour times its strength in display units
//!   (illuminance / (pi x light_unit), as sky.rs sets it), taken to linear light by [`texel`]
//!   and added to the sky in the sky's units. Sources add in linear light.
//! - **When**: checked at most every [`CHECK_INTERVAL`] seconds of real time and rewritten
//!   only when an input moved by more than a step from the inputs of the last fill
//!   ([`Inputs::moved`]: a colour by [`COLOUR_STEP`] in display value, the light's direction
//!   by [`SUN_STEP_DEGREES`], indoors or out, the interior cell): a weather fade or the
//!   clock rewrites it at most four times a second. With a room or ground lamps, also the
//!   eye moved by [`CENTRE_STEP`], the box by [`BOX_STEP`], the lamp set, or a lamp's
//!   colour (past the colour step), place or radius (past [`LAMP_STEP`]). Indoors against
//!   outdoors, another interior cell, another room marker and a new dome profile skip the
//!   wait and refill at once (code review 1, finding 6), as does losing the dome profile
//!   outdoors or replacing the cube's camera. One log line at the first fill of each cube,
//!   one at the first room fill of each interior (the box's source, its size, the lamps,
//!   the worker's fill time and the main thread's gather and apply times), one summary at
//!   exit (`reflection sky summary: ...`, with the room fills, the refills by reason, the
//!   main thread's slowest gather and apply and the fills discarded for a replaced camera or
//!   cube). The gather time starts after the per-frame room-marker search for the wait's
//!   key, which runs every frame either way.
//! - **Where** (maintainer decision 2026-11-22, #3238): the main thread only gathers the
//!   inputs (the sky's state, the lamps, the room marker or the six rays, which need the
//!   physics world) into owned data ([`FillRequest`]); the fill itself ([`fill`]: the
//!   content and every level's texels, a pure function of the inputs) runs on Bevy's async
//!   compute pool. Each frame the producer polls it ([`FillQueue`]); the cube keeps the last
//!   finished fill until the new one is done, and the main thread then only moves the
//!   finished bytes into the cube (`ReflectionCube::set_levels`; the render world uploads
//!   them as before). At most one fill runs: inputs that move past a step while it runs are
//!   kept as the one waiting request (a newer one replaces it). When the running fill
//!   finishes, it is applied all the same (it is newer than what the cube shows) and the
//!   waiting request starts in the same frame, so no work is thrown away and a stream of
//!   changes cannot keep the cube from updating. So the cube shows a change one fill later
//!   than before (the fill's time, a few milliseconds, then the next frame's upload), two
//!   with a fill already running [design]. Each request names the camera and the cube it
//!   was asked for (none: a cube is made on applying it); a fill whose camera or cube was
//!   replaced meanwhile is discarded, and the replacement's change of target asks its own
//!   fill. A light inherited from an older cube is removed as soon as the camera or cube
//!   changes, not when the new fill lands: any light of a cube a fill went into, while the
//!   camera holds another cube or none. The hemisphere means (a pass over the room's
//!   level-0 texels) are taken only for fills asked while the camera's cube was not the
//!   last one filled (its first, whose log line quotes them, any asked behind it, and the
//!   first after a camera or cube comes back from an earlier replacement). Until the first fill of a
//!   camera is done the camera has no cube and no `EnvironmentMapLight`, as before the first
//!   upload in stage 1; on entering an interior the outdoor cube stays until the room's
//!   first fill is done.

use bevy::light::EnvironmentMapLight;
use bevy::prelude::*;
use bevy::tasks::{futures::check_ready, AsyncComputeTaskPool, Task, TaskPool};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use nvf_assets::reflection_cube::{self, GateState, ReflectionCube, SPECULAR_LEVELS};
use nvf_assets::reflection_room::{self, FilteredCube, GroundLamps, Lamp, RoomBox, RoomScene, Spot};
use std::f32::consts::PI;

use crate::fly::FlyCam;
use crate::sky::{SkyState, Sun};
use fnv_world::sky::ColorType;

/// The assumed albedo of the ground below the horizon, as display values (texture bytes /
/// 255), a neutral warm grey; the shader's diffuse colour is its linear value (about 0.089,
/// 0.073, 0.051) [design: the first runs' sunlit Goodsprings sand read about (70, 58, 32) on
/// screen; not measured against anything else].
pub const GROUND_ALBEDO: [f32; 3] = [0.33, 0.30, 0.25];
/// Indoors, the fog band's half-width in sine of elevation (about 11.5 degrees), a Gaussian
/// weight peaking at 1 on the horizon [design].
pub const FOG_BAND: f32 = 0.2;
/// The lobe toward the directional light: its peak at level 0 as a share of the light's
/// converted colour [design].
pub const SUN_LOBE_PEAK: f32 = 0.5;
/// The lobe's power at level 0 (cos^16 halves at about 16.5 degrees); halved at each level,
/// the peak scaled by (power + 1) / (SUN_LOBE_POWER + 1) so the lobe keeps its total [design].
pub const SUN_LOBE_POWER: f32 = 16.0;
/// The half-width (sine of elevation) of the edge between ground and sky at level 0, about a
/// texel of the 32-texel face [design].
pub const HORIZON_EDGE: f32 = 0.05;
/// The edge's half-width at the last level [design].
pub const HORIZON_EDGE_TOP: f32 = 0.5;
/// A colour input counts as changed past this step in display value [design].
pub const COLOUR_STEP: f32 = 1.0 / 128.0;
/// The directional light's direction counts as changed past this angle [design].
pub const SUN_STEP_DEGREES: f32 = 3.0;
/// Real seconds between two checks of the inputs [design: at most four fills a second].
pub const CHECK_INTERVAL: f64 = 0.25;
/// Samples of each hemisphere's profile over the sine of elevation 0..1.
const SAMPLES: usize = 33;
/// The room and the ground lamps are drawn around the eye: they are refilled once the eye
/// moved this far from the last fill's, game units [design: the stage 2 design's starting
/// guess (Q3); whether a refill shows as a jump is to judge in pictures].
pub const CENTRE_STEP: f32 = 64.0;
/// A box counts as changed when a centre or a half extent moved past this, game units
/// [design].
pub const BOX_STEP: f32 = 32.0;
/// A lamp counts as changed when its place or radius moved past this, game units [design].
pub const LAMP_STEP: f32 = 8.0;
/// At most this many lamps light the room or the ground, the nearest to the eye [design:
/// the level-0 fill costs 6,144 texels x the lamps].
pub const MAX_LAMPS: usize = 16;
/// Lamps farther from the eye than this are left out, game units [design].
pub const LAMP_DISTANCE: f32 = 4096.0;
/// Outdoors a lamp lights the cube's ground when its sphere comes this close to the eye,
/// game units [design].
pub const OUTDOOR_LAMP_REACH: f32 = 512.0;
/// The physics rays that measure a room reach this far, game units [design].
pub const ROOM_RAY_REACH: f32 = 2048.0;
/// Of the six rays, at least this many must hit for the rays' box to stand [design: one
/// open doorway or stairwell allowed].
pub const ROOM_RAY_MIN_HITS: usize = 5;
/// A ray that misses puts its wall this far from the eye, game units [design: the default
/// box's half width].
pub const ROOM_RAY_MISS: f32 = DEFAULT_HALF_WIDTH;
/// The smallest half extent of a box, game units [design: an eye against a wall still sees
/// a room].
pub const MIN_HALF: f32 = 16.0;
/// The default box: half its width and depth, and its height, game units, its floor
/// [`EYE_HEIGHT`] under the eye [design: a modest room].
pub const DEFAULT_HALF_WIDTH: f32 = 256.0;
pub const DEFAULT_HEIGHT: f32 = 320.0;
/// The eye's height over the floor where nothing measures it, game units [design: about a
/// standing character's eyes].
pub const EYE_HEIGHT: f32 = 120.0;
/// Outdoors the ground under the eye is looked for this far down, game units [design].
pub const GROUND_RAY_REACH: f32 = 1024.0;

/// The units identity (module notes): a display value `v` (per channel, with its light's
/// share) as the cube's linear texel, `v x max(v, 1e-6)^1.2`, the fork's world conversion of
/// the ambient highlight (lights/display_shader.rs `nvf_ambient_ratio`). With the cube's
/// intensity `light_unit()` and the default exposure E = 1 / light_unit(), Bevy reflects
/// `texel x intensity x E` = this, so a direction whose display colour is the view's exposed
/// ambient A reflects A x max(A, 1e-6)^1.2, about A^2.2. Negative input is held at 0.
pub fn texel(display: Vec3) -> Vec3 {
    display.map(|a| {
        let a = a.max(0.0);
        a * a.max(1e-6).powf(1.2)
    })
}

/// Tony McMapface's response to grey and its inverse, the shared tonemapper input's
/// (nvf_assets tone_input.rs, the numbers of tone_input.wesl that sky.wesl uses).
pub use nvf_assets::tone_input::tony_inverse;
#[cfg(test)]
pub use nvf_assets::tone_input::{tony_forward, TONY_IN, TONY_OUT};

/// The sky's units (module notes): the texel for a sky colour `scaled`, linear and already
/// times the sky's display scale, as sky.wesl writes it into the HDR target: `scaled` itself
/// while the main view shows display values clipped (`clip`, the default), else
/// [`tony_inverse`] per channel, so that the view's tone map gives back the colour the dome
/// shows. A perfect mirror, whose output is texel x intensity x view.exposure = texel at the
/// default exposure, then shows the dome as drawn. Negative input is held at 0.
pub fn sky_texel(scaled: Vec3, clip: bool) -> Vec3 {
    let scaled = scaled.max(Vec3::ZERO);
    if clip { scaled } else { scaled.map(tony_inverse) }
}

/// Lit ground's units (module notes): what the fork's display-value sum writes for a diffuse
/// surface of linear colour `albedo` under the display-value light `light` (ambient +
/// direct, lights/display_shader.rs `DISPLAY_EDITS`): the linear colour of min(albedo's
/// display value x light, 1) per channel, no inverse tone map (the surface itself is
/// tone-mapped like the mirror).
pub fn ground_texel(albedo: Vec3, light: Vec3) -> Vec3 {
    reflection_room::lit_texel(albedo, light)
}

/// [`GROUND_ALBEDO`] as the shader's linear diffuse colour.
pub fn ground_albedo() -> Vec3 {
    Vec3::from_array(GROUND_ALBEDO.map(crate::lodgen::srgb_to_linear))
}

/// The dome's weights of the horizon, sky-lower and sky-upper colours by height
/// ([`DOME_PROFILE_BINS`] equal steps of the sine of elevation from the horizon to the
/// zenith, equal solid angles): the mean of `atmosphere.nif`'s vertex colours in each step,
/// as sky.rs colours the dome from them (`dome_color`). Inserted when the sky is made.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct DomeProfile(pub [[f32; 3]; DOME_PROFILE_BINS]);

pub const DOME_PROFILE_BINS: usize = 16;

impl DomeProfile {
    /// From the dome's vertices (game space, z up, centred on the camera as drawn) and their
    /// colours (red horizon, green sky-lower, blue sky-upper). A step no vertex falls in takes
    /// the steps around it, linearly; with no vertex above the horizon, [`fallback`](Self::fallback).
    pub fn from_vertices(positions: &[[f32; 3]], colours: &[[f32; 4]]) -> DomeProfile {
        let mut sum = [[0.0f32; 3]; DOME_PROFILE_BINS];
        let mut count = [0u32; DOME_PROFILE_BINS];
        for (p, c) in positions.iter().zip(colours) {
            let len = Vec3::from_array(*p).length();
            if len <= 0.0 || !len.is_finite() {
                continue;
            }
            let e = p[2] / len;
            if e < 0.0 {
                continue;
            }
            let bin = ((e * DOME_PROFILE_BINS as f32) as usize).min(DOME_PROFILE_BINS - 1);
            for k in 0..3 {
                sum[bin][k] += c[k];
            }
            count[bin] += 1;
        }
        let filled: Vec<usize> = (0..DOME_PROFILE_BINS).filter(|&i| count[i] > 0).collect();
        if filled.is_empty() {
            return DomeProfile::fallback();
        }
        let mean = |i: usize| sum[i].map(|s| s / count[i] as f32);
        DomeProfile(std::array::from_fn(|i| {
            if count[i] > 0 {
                return mean(i);
            }
            let below = filled.iter().rev().find(|&&j| j < i).copied();
            let above = filled.iter().find(|&&j| j > i).copied();
            match (below, above) {
                (Some(a), Some(b)) => {
                    let t = (i - a) as f32 / (b - a) as f32;
                    let (wa, wb) = (mean(a), mean(b));
                    std::array::from_fn(|k| wa[k] + (wb[k] - wa[k]) * t)
                }
                (Some(a), None) => mean(a),
                (None, Some(b)) => mean(b),
                (None, None) => [0.0, 1.0, 0.0],
            }
        }))
    }

    /// The fallback sphere's rule (sky.rs `fallback_dome`): sky-lower to sky-upper over the
    /// lower half of the upper hemisphere, t = sqrt(e / 0.5).
    pub fn fallback() -> DomeProfile {
        DomeProfile(std::array::from_fn(|i| {
            let e = (i as f32 + 0.5) / DOME_PROFILE_BINS as f32;
            let t = (e / 0.5).clamp(0.0, 1.0).sqrt();
            [0.0, 1.0 - t, t]
        }))
    }

    /// The weights at sine of elevation `e` (held to 0..1), linear between the steps' centres.
    pub fn weights(&self, e: f32) -> [f32; 3] {
        let x = (e.clamp(0.0, 1.0) * DOME_PROFILE_BINS as f32 - 0.5).clamp(0.0, (DOME_PROFILE_BINS - 1) as f32);
        let i = (x.floor() as usize).min(DOME_PROFILE_BINS - 2);
        let t = x - i as f32;
        let (a, b) = (self.0[i], self.0[i + 1]);
        std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t)
    }
}

/// What the cube is made from (module notes). Colours are display values without the
/// ambient's share (the content applies it), except the dome colours, linear as
/// `SkyState::color` gives them, and the light, already in display units with its strength.
#[derive(Clone, Debug, PartialEq)]
pub enum Inputs {
    Outdoors {
        horizon: Vec3,
        lower: Vec3,
        upper: Vec3,
        ambient: Vec3,
        /// The directional light's display colour times its strength.
        sun: Vec3,
        /// Toward the light (unit, Bevy world).
        sun_dir: Vec3,
        /// The sky's display scale (`SkyState::sky_scale`, sky.wesl's `params.x`).
        sky_scale: f32,
        /// The main view shows display values clipped (`lights::display_clip`): the sky is
        /// written without its inverse tone map.
        clip: bool,
        /// Placed lamps lighting the ground near the eye; `None` with none in reach.
        ground: Option<GroundInputs>,
    },
    Indoors {
        /// The interior cell (world.rs `Streamer::interior`).
        cell: Option<u32>,
        ambient: Vec3,
        fog: Vec3,
        /// The analytic room; `None` (stage 1's content) with no lamp in reach and only the
        /// default box.
        room: Option<RoomInputs>,
    },
}

/// Where a room's box came from (module notes, "Indoors").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxSource {
    /// The room marker (its reference) the eye is in.
    Marker(u32),
    /// Physics rays from the eye.
    Rays,
    /// The default box around the eye.
    Default,
}

impl std::fmt::Display for BoxSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoxSource::Marker(id) => write!(f, "room marker {id:08X}"),
            BoxSource::Rays => write!(f, "rays"),
            BoxSource::Default => write!(f, "default"),
        }
    }
}

/// A lamp and the entity it came from (its bits), for comparing lamp sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LampInput {
    pub id: u64,
    pub lamp: Lamp,
}

/// The analytic room's inputs (Bevy world space; colours display values without shares,
/// the directional light already times its strength).
#[derive(Clone, Debug, PartialEq)]
pub struct RoomInputs {
    pub eye: Vec3,
    pub source: BoxSource,
    pub room: RoomBox,
    /// Sorted by id.
    pub lamps: Vec<LampInput>,
    pub sun: Vec3,
    pub sun_dir: Vec3,
    /// The cell's fog near, far and power; `None` with the player's no-interior-fog tweak.
    pub fog_range: Option<[f32; 3]>,
}

/// The ground lamps' inputs outdoors (Bevy world space): the eye, the ground's height under
/// it and the lamps, sorted by id.
#[derive(Clone, Debug, PartialEq)]
pub struct GroundInputs {
    pub eye: Vec3,
    pub ground: f32,
    pub lamps: Vec<LampInput>,
}

/// Why the cube was refilled (the exit summary counts them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refill {
    /// Indoors against outdoors.
    Mode,
    /// Another interior cell.
    Cell,
    /// Another room marker or box source, or the room came or went.
    Room,
    /// The box moved past [`BOX_STEP`].
    Box,
    /// The eye moved past [`CENTRE_STEP`].
    Moved,
    /// The lamp set, or a lamp's colour, place, radius or cone.
    Lamps,
    /// The sky's, ambient's, fog's or directional light's colours or direction.
    Light,
    /// A new dome profile.
    Profile,
}

impl Refill {
    pub const ALL: [Refill; 8] = [Refill::Mode, Refill::Cell, Refill::Room, Refill::Box, Refill::Moved, Refill::Lamps, Refill::Light, Refill::Profile];

    pub fn name(self) -> &'static str {
        match self {
            Refill::Mode => "mode",
            Refill::Cell => "cell",
            Refill::Room => "room",
            Refill::Box => "box",
            Refill::Moved => "moved",
            Refill::Lamps => "lamps",
            Refill::Light => "light",
            Refill::Profile => "profile",
        }
    }
}

fn colour_moved(a: Vec3, b: Vec3) -> bool {
    (a - b).abs().max_element() > COLOUR_STEP
}

/// Whether two lamp sets differ: another set of lamps, or a lamp's colour past
/// [`COLOUR_STEP`], its place or radius past [`LAMP_STEP`], its cone's direction past
/// [`SUN_STEP_DEGREES`] or its angles.
pub fn lamps_moved(a: &[LampInput], b: &[LampInput]) -> bool {
    a.len() != b.len()
        || a.iter().zip(b).any(|(x, y)| {
            let (p, q) = (x.lamp, y.lamp);
            x.id != y.id
                || colour_moved(p.colour, q.colour)
                || p.position.distance(q.position) > LAMP_STEP
                || (p.radius - q.radius).abs() > LAMP_STEP
                || match (p.spot, q.spot) {
                    (None, None) => false,
                    (Some(s), Some(t)) => s.direction.dot(t.direction) < SUN_STEP_DEGREES.to_radians().cos() || (s.scale - t.scale).abs() > 1e-3 * t.scale.abs().max(1.0) || (s.offset - t.offset).abs() > 1e-3 * t.offset.abs().max(1.0),
                    _ => true,
                }
        })
}

fn box_moved(a: &RoomBox, b: &RoomBox) -> bool {
    a.centre.distance(b.centre) > BOX_STEP
        || (a.half - b.half).abs().max_element() > BOX_STEP
        || (0..3).any(|axis| a.axes.col(axis).dot(b.axes.col(axis)) < SUN_STEP_DEGREES.to_radians().cos())
}

impl RoomInputs {
    fn change(&self, last: &RoomInputs) -> Option<Refill> {
        let lit = self.sun.max_element() > COLOUR_STEP || last.sun.max_element() > COLOUR_STEP;
        let fog = match (self.fog_range, last.fog_range) {
            (None, None) => false,
            (Some(a), Some(b)) => (a[0] - b[0]).abs() > 1.0 || (a[1] - b[1]).abs() > 1.0 || (a[2] - b[2]).abs() > 0.01,
            _ => true,
        };
        if self.source != last.source {
            Some(Refill::Room)
        } else if box_moved(&self.room, &last.room) {
            Some(Refill::Box)
        } else if self.eye.distance(last.eye) > CENTRE_STEP {
            Some(Refill::Moved)
        } else if lamps_moved(&self.lamps, &last.lamps) {
            Some(Refill::Lamps)
        } else if colour_moved(self.sun, last.sun) || (lit && self.sun_dir.dot(last.sun_dir) < SUN_STEP_DEGREES.to_radians().cos()) || fog {
            Some(Refill::Light)
        } else {
            None
        }
    }
}

impl GroundInputs {
    fn change(&self, last: &GroundInputs) -> Option<Refill> {
        if lamps_moved(&self.lamps, &last.lamps) {
            Some(Refill::Lamps)
        } else if self.eye.distance(last.eye) > CENTRE_STEP || (self.ground - last.ground).abs() > BOX_STEP {
            Some(Refill::Moved)
        } else {
            None
        }
    }
}

impl Inputs {
    /// Whether these inputs differ from `last` (the last fill's) by more than a step
    /// (module notes, "When"): see [`change`](Self::change).
    #[cfg(test)]
    fn moved(&self, last: &Inputs) -> bool {
        self.change(last).is_some()
    }

    /// Why these inputs differ from `last` (the last fill's) by more than a step, if they do
    /// (module notes, "When"): indoors against outdoors, another cell, the room (its source,
    /// box, eye, lamps, directional light, fog range), the ground lamps, a colour channel by
    /// more than [`COLOUR_STEP`] in display value, the light's direction by more than
    /// [`SUN_STEP_DEGREES`] while either light is above a step.
    pub fn change(&self, last: &Inputs) -> Option<Refill> {
        let dome = |v: Vec3| v.map(crate::lights::linear_to_display);
        match (self, last) {
            (
                Inputs::Outdoors { horizon, lower, upper, ambient, sun, sun_dir, sky_scale, clip, ground },
                Inputs::Outdoors { horizon: h0, lower: l0, upper: u0, ambient: a0, sun: s0, sun_dir: d0, sky_scale: k0, clip: c0, ground: g0 },
            ) => {
                let lit = sun.max_element() > COLOUR_STEP || s0.max_element() > COLOUR_STEP;
                let light = clip != c0
                    || (sky_scale - k0).abs() > COLOUR_STEP * k0.abs().max(1.0)
                    || colour_moved(dome(*horizon), dome(*h0))
                    || colour_moved(dome(*lower), dome(*l0))
                    || colour_moved(dome(*upper), dome(*u0))
                    || colour_moved(*ambient, *a0)
                    || colour_moved(*sun, *s0)
                    || (lit && sun_dir.dot(*d0) < SUN_STEP_DEGREES.to_radians().cos());
                match (ground, g0) {
                    (Some(g), Some(g0)) => g.change(g0).or(light.then_some(Refill::Light)),
                    (None, None) => light.then_some(Refill::Light),
                    _ => Some(Refill::Lamps),
                }
            }
            (Inputs::Indoors { cell, ambient, fog, room }, Inputs::Indoors { cell: c0, ambient: a0, fog: f0, room: r0 }) => {
                if cell != c0 {
                    return Some(Refill::Cell);
                }
                let room = match (room, r0) {
                    (Some(r), Some(r0)) => r.change(r0),
                    (None, None) => None,
                    (Some(r), None) | (None, Some(r)) => Some(if r.source == BoxSource::Default { Refill::Lamps } else { Refill::Room }),
                };
                room.or((colour_moved(*ambient, *a0) || colour_moved(*fog, *f0)).then_some(Refill::Light))
            }
            _ => Some(Refill::Mode),
        }
    }

    pub fn indoors(&self) -> bool {
        matches!(self, Inputs::Indoors { .. })
    }
}

/// The cube's content for one set of inputs (module notes, "What"): each hemisphere's
/// colour by sine of elevation as converted texels, their means, and the lobe.
#[derive(Clone, Debug)]
pub struct SkyContent {
    /// Texels at sine of elevation i / (SAMPLES - 1), above the horizon.
    up: [Vec3; SAMPLES],
    /// The same below the horizon (index by the sine of the depression).
    down: [Vec3; SAMPLES],
    up_mean: Vec3,
    down_mean: Vec3,
    /// The lobe's converted colour at its level-0 peak.
    sun: Vec3,
    sun_dir: Vec3,
}

/// The mean of a profile sampled evenly over the sine of elevation (equal solid angles):
/// the trapezoid rule.
fn hemisphere_mean(p: &[Vec3; SAMPLES]) -> Vec3 {
    let inner: Vec3 = p[1..SAMPLES - 1].iter().copied().sum();
    (inner + 0.5 * (p[0] + p[SAMPLES - 1])) / (SAMPLES - 1) as f32
}

fn sample(p: &[Vec3; SAMPLES], e: f32) -> Vec3 {
    let x = e.clamp(0.0, 1.0) * (SAMPLES - 1) as f32;
    let i = (x.floor() as usize).min(SAMPLES - 2);
    p[i].lerp(p[i + 1], x - i as f32)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl SkyContent {
    pub fn new(inputs: &Inputs, profile: &DomeProfile) -> SkyContent {
        let at = |i: usize| i as f32 / (SAMPLES - 1) as f32;
        match inputs {
            Inputs::Outdoors { horizon, lower, upper, ambient, sun, sun_dir, sky_scale, clip, .. } => {
                // The dome as sky.wesl writes it (module notes, "Units").
                let up = std::array::from_fn(|i| {
                    let linear = crate::sky::dome_color(profile.weights(at(i)), lower.to_array(), horizon.to_array(), upper.to_array());
                    sky_texel(Vec3::from_array(linear) * *sky_scale, *clip)
                });
                // Level ground under the ambient and the directional light (N = up), as the
                // display-value sum lights it: the ambient as the view's (display value x
                // share), the light as nvf_directional_display gives it.
                let sun_dir = sun_dir.normalize_or_zero();
                let light = *ambient * crate::sky::AMBIENT_SHARE + *sun * sun_dir.y.max(0.0);
                let ground = ground_texel(ground_albedo(), light);
                let down = [ground; SAMPLES];
                SkyContent { up_mean: hemisphere_mean(&up), down_mean: ground, up, down, sun: sky_texel(texel(*sun) * SUN_LOBE_PEAK, *clip), sun_dir }
            }
            Inputs::Indoors { ambient, fog, .. } => {
                let share = crate::sky::INTERIOR_AMBIENT_SHARE;
                let (sky, ground, fog) = (texel(*ambient * share), ground_texel(ground_albedo(), *ambient * share), texel(*fog * share));
                let band = |e: f32| (-(e / FOG_BAND).powi(2)).exp();
                let up = std::array::from_fn(|i| sky.lerp(fog, band(at(i))));
                let down = std::array::from_fn(|i| ground.lerp(fog, band(at(i))));
                SkyContent { up_mean: hemisphere_mean(&up), down_mean: hemisphere_mean(&down), up, down, sun: Vec3::ZERO, sun_dir: Vec3::ZERO }
            }
        }
    }

    /// The linear texel toward world direction `d` (unit) at `level` (module notes).
    pub fn colour(&self, d: Vec3, level: u32) -> Vec3 {
        let b = (level as f32 / (SPECULAR_LEVELS - 1) as f32).min(1.0);
        let e = d.y.clamp(-1.0, 1.0);
        let up = sample(&self.up, e.max(0.0)).lerp(self.up_mean, b);
        let down = sample(&self.down, (-e).max(0.0)).lerp(self.down_mean, b);
        let w = HORIZON_EDGE + (HORIZON_EDGE_TOP - HORIZON_EDGE) * b;
        let sky = smoothstep(-w, w, e);
        let power = SUN_LOBE_POWER / (1u32 << level.min(16)) as f32;
        let lobe = self.sun * ((power + 1.0) / (SUN_LOBE_POWER + 1.0)) * d.dot(self.sun_dir).max(0.0).powf(power);
        down.lerp(up, sky) + lobe * sky
    }

    /// The hemispheres' means (above, below), which the last level approaches.
    pub fn means(&self) -> (Vec3, Vec3) {
        (self.up_mean, self.down_mean)
    }
}

/// The cube's content (module notes, "What"): stage 1's sky or room colours, the analytic
/// room, or stage 1's sky with the lamps' ground added.
#[derive(Clone, Debug)]
pub enum Content {
    Sky(SkyContent),
    Room(FilteredCube),
    SkyAndGround(SkyContent, FilteredCube),
}

impl Content {
    pub fn new(inputs: &Inputs, profile: &DomeProfile) -> Content {
        match inputs {
            Inputs::Indoors { ambient, fog, room: Some(r), .. } => {
                let scene = room_scene(*ambient, *fog, r);
                Content::Room(FilteredCube::new(|d| scene.radiance(d)))
            }
            Inputs::Outdoors { ambient, sun, sun_dir, ground: Some(g), .. } => {
                let ground = GroundLamps {
                    eye: g.eye,
                    ground: g.ground,
                    albedo: ground_albedo(),
                    base: outdoor_ground_light(*ambient, *sun, *sun_dir),
                    lamps: g.lamps.iter().map(|l| l.lamp).collect(),
                };
                Content::SkyAndGround(SkyContent::new(inputs, profile), FilteredCube::new(|d| ground.added(d)))
            }
            _ => Content::Sky(SkyContent::new(inputs, profile)),
        }
    }

    /// The linear texel toward world direction `d` (unit) at `level`.
    pub fn colour(&self, d: Vec3, level: u32) -> Vec3 {
        match self {
            Content::Sky(s) => s.colour(d, level),
            Content::Room(r) => r.colour(d, level),
            Content::SkyAndGround(s, g) => s.colour(d, level) + g.colour(d, level),
        }
    }

    /// The means above and below the horizon (stage 1's hemisphere means; the room's and
    /// the lamps' level 0, solid-angle weighted).
    pub fn means(&self) -> (Vec3, Vec3) {
        match self {
            Content::Sky(s) => s.means(),
            Content::Room(r) => r.hemisphere_means(0),
            Content::SkyAndGround(s, g) => {
                let ((u, d), (gu, gd)) = (s.means(), g.hemisphere_means(0));
                (u + gu, d + gd)
            }
        }
    }
}

/// The texels the log lines quote, read from a fill's content on the worker (module notes,
/// "When").
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FillSamples {
    pub zenith: Vec3,
    pub horizon: Vec3,
    pub nadir: Vec3,
    /// The means above and below the horizon ([`Content::means`]), only when asked for (the
    /// first fill of a camera or cube logs them; for a room they cost a pass over the 6,144
    /// level-0 texels).
    pub means: Option<(Vec3, Vec3)>,
    /// Indoors with a room and a lamp, the level-0 texel toward the lamp nearest the eye.
    pub nearest_lamp: Option<Vec3>,
}

/// A finished fill: the specular cube's levels as `ReflectionCube` uploads them, the texels
/// the log lines quote and the fill's time on the worker, milliseconds.
#[derive(Clone, Debug)]
pub struct Filled {
    pub levels: Vec<Vec<u8>>,
    pub samples: FillSamples,
    pub ms: f32,
}

/// One fill (module notes, "Where"): the content for `inputs` and every level of the
/// specular cube from it (`reflection_cube::specular_levels`). A pure function of its
/// arguments apart from the time it reports: the same inputs give the same bytes, the bytes
/// the producer wrote on the main thread before the fill moved to a worker. `means`: also
/// take the hemisphere means for the first-fill log line.
pub fn fill(inputs: &Inputs, profile: &DomeProfile, means: bool) -> Filled {
    let started = Instant::now();
    let content = Content::new(inputs, profile);
    let levels = reflection_cube::specular_levels(|d, level| content.colour(d, level));
    let nearest_lamp = match inputs {
        Inputs::Indoors { room: Some(r), .. } => r
            .lamps
            .iter()
            .min_by(|a, b| a.lamp.position.distance(r.eye).total_cmp(&b.lamp.position.distance(r.eye)))
            .map(|l| content.colour((l.lamp.position - r.eye).normalize_or(Vec3::Y), 0)),
        _ => None,
    };
    let samples = FillSamples {
        zenith: content.colour(Vec3::Y, 0),
        horizon: content.colour(Vec3::X, 0),
        nadir: content.colour(Vec3::NEG_Y, 0),
        means: means.then(|| content.means()),
        nearest_lamp,
    };
    Filled { levels, samples, ms: started.elapsed().as_secs_f32() * 1000.0 }
}

/// A fill the producer asked for: what it is made from, owned so the worker can take a copy,
/// and what the producer records when its result is applied.
#[derive(Clone, Debug)]
pub struct FillRequest {
    pub inputs: Inputs,
    /// The dome profile (or its fallback) at the time of asking.
    pub profile: DomeProfile,
    /// Why it was asked; `None` for a new camera or cube with unchanged inputs.
    pub reason: Option<Refill>,
    /// The main thread's time to gather the inputs and compare them, milliseconds.
    pub gather_ms: f32,
    /// The camera the fill is for and the cube it held when asked (`None`: a cube is made
    /// when the fill is applied). A fill is applied only while the camera still holds that
    /// cube; a fill for a replaced camera or cube is discarded.
    pub target: (Entity, Option<AssetId<Image>>),
    /// Take the hemisphere means on the worker: asked while its cube was not the last one
    /// filled (the first, one queued behind it, or the first after a camera or cube comes
    /// back), whose log line quotes them.
    pub means: bool,
}

/// What [`FillQueue::poll`] found.
#[derive(Debug, PartialEq)]
pub enum Polled<R, T> {
    /// No fill runs.
    Idle,
    /// The fill runs on.
    Running,
    /// The fill finished: apply its result. A request waiting for it has been started.
    Done(R, T),
}

/// At most one fill in flight (module notes, "Where"). `R` is the request, `J` the running
/// job (a task in the game, a plain value in the tests); the queue itself spawns nothing and
/// waits for nothing, so its rules are tested without a GPU or a thread pool.
#[derive(Debug)]
pub struct FillQueue<R, J> {
    running: Option<(R, J)>,
    waiting: Option<R>,
}

impl<R, J> Default for FillQueue<R, J> {
    fn default() -> Self {
        FillQueue { running: None, waiting: None }
    }
}

impl<R, J> FillQueue<R, J> {
    /// Ask for a fill: started at once with `spawn` when none runs; otherwise kept as the
    /// waiting request, replacing an older waiting one (the newest inputs win).
    pub fn ask(&mut self, request: R, spawn: impl FnOnce(&R) -> J) {
        if self.running.is_some() {
            self.waiting = Some(request);
        } else {
            let job = spawn(&request);
            self.running = Some((request, job));
        }
    }

    /// Look at the running fill with `ready` (its result once finished). A finished fill is
    /// handed back to apply, even with newer inputs waiting (it is still newer than what the
    /// cube shows); a waiting request is started with `spawn` first.
    pub fn poll<T>(&mut self, ready: impl FnOnce(&mut J) -> Option<T>, spawn: impl FnOnce(&R) -> J) -> Polled<R, T> {
        let Some((_, job)) = self.running.as_mut() else { return Polled::Idle };
        let Some(result) = ready(job) else { return Polled::Running };
        let Some((request, _)) = self.running.take() else { return Polled::Idle };
        if let Some(next) = self.waiting.take() {
            self.ask(next, spawn);
        }
        Polled::Done(request, result)
    }

    /// A fill runs (the tests wait on it).
    #[cfg(test)]
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// The request waiting for the running fill to finish (the tests read it).
    #[cfg(test)]
    pub fn waiting(&self) -> Option<&R> {
        self.waiting.as_ref()
    }

    /// The running and the waiting request, to change in place.
    pub fn requests_mut(&mut self) -> impl Iterator<Item = &mut R> {
        self.running.as_mut().map(|(r, _)| r).into_iter().chain(self.waiting.as_mut())
    }
}

/// Start a fill on Bevy's async compute pool (module notes, "Where") with a copy of the
/// request's inputs and profile. `hold` is the tests' seam: while it is set the fill does
/// not start (never set in the game).
fn spawn_fill(request: &FillRequest, hold: Option<Arc<AtomicBool>>) -> Task<Filled> {
    let (inputs, profile, means) = (request.inputs.clone(), request.profile.clone(), request.means);
    AsyncComputeTaskPool::get_or_init(TaskPool::default).spawn(async move {
        while hold.as_ref().is_some_and(|h| h.load(Ordering::Acquire)) {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        fill(&inputs, &profile, means)
    })
}

/// Remove a light the camera holds from any cube a fill went into ([`ReflectionSky`]'s
/// `filled_cubes`) when the camera now holds another cube or none (`current`), so no camera
/// shows an older cube's reflection while its own fill runs, and `attach_lights` (which
/// skips a camera that has a light) installs the new cube's light once it is uploaded.
/// Other maps stay.
fn drop_inherited_light(state: &ReflectionSky, commands: &mut Commands, entity: Entity, environment: Option<&EnvironmentMapLight>, current: Option<AssetId<Image>>) {
    if environment.is_some_and(|light| {
        let id = light.specular_map.id();
        Some(id) != current && state.filled_cubes.contains(&id)
    }) {
        commands.entity(entity).remove::<EnvironmentMapLight>();
    }
}

/// Stage 1's light on level ground outdoors: the ambient (x share) plus the directional
/// light times the sine of its elevation.
fn outdoor_ground_light(ambient: Vec3, sun: Vec3, sun_dir: Vec3) -> Vec3 {
    ambient * crate::sky::AMBIENT_SHARE + sun * sun_dir.normalize_or_zero().y.max(0.0)
}

/// The analytic room for an interior's inputs: the ambient with the interior ambient share
/// (as the ambient light's brightness carries it, sky.rs `update_sky`), the directional
/// light as gathered, the fog colour as stage 1 converts it.
pub fn room_scene(ambient: Vec3, fog: Vec3, r: &RoomInputs) -> RoomScene {
    let share = crate::sky::INTERIOR_AMBIENT_SHARE;
    RoomScene {
        eye: r.eye,
        room: r.room,
        albedo: reflection_room::Albedo::default(),
        ambient: ambient * share,
        directional: r.sun,
        toward_light: r.sun_dir.normalize_or_zero(),
        lamps: r.lamps.iter().map(|l| l.lamp).collect(),
        fog: r.fog_range.map(|[near, far, power]| reflection_room::Fog { texel: texel(fog * share), near, far, power }),
    }
}

/// Game space to Bevy world space as a rotation (world.rs `game_to_bevy`; an interior's cell
/// sits at the origin, rooms.rs `cull`).
fn bevy_from_game() -> Mat3 {
    Mat3::from_quat(crate::world::game_to_bevy().rotation)
}

/// A box given in game space (centre, axes as columns, half extents) in Bevy space; its
/// axes stay in game order, so its size reads (x, y, height).
pub fn box_from_game(centre: Vec3, axes: Mat3, half: Vec3) -> RoomBox {
    let r = bevy_from_game();
    RoomBox { centre: r * centre, axes: r * axes, half: half.max(Vec3::splat(MIN_HALF)) }
}

/// The room marker the eye (Bevy space) is in, as rooms.rs `cull` finds `camera_room` (the
/// first room whose box holds the eye), with its box in Bevy space: only for the interior
/// the rooms were read for.
pub fn marker_box(rooms: Option<&crate::rooms::Rooms>, cell: Option<u32>, eye: Vec3) -> Option<(u32, RoomBox)> {
    let rooms = rooms?;
    if cell.is_none() || rooms.cell != cell {
        return None;
    }
    let p = crate::world::bevy_to_game(eye);
    rooms.rooms.iter().find(|r| r.contains(p, 0.0)).map(|r| (r.id, box_from_game(r.center, r.rot, r.half)))
}

/// The six directions of the room's rays, game space: +x, -x, +y, -y, up, down.
pub const ROOM_RAY_DIRECTIONS: [[f32; 3]; 6] = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];

/// The rays' box from the distances each of [`ROOM_RAY_DIRECTIONS`] hit at (`None`: a
/// miss), the eye in game space: axis-aligned in game space, a miss putting its wall
/// [`ROOM_RAY_MISS`] away; `None` with fewer than [`ROOM_RAY_MIN_HITS`] hits.
pub fn box_from_rays(eye: Vec3, hits: [Option<f32>; 6]) -> Option<RoomBox> {
    if hits.iter().filter(|h| h.is_some()).count() < ROOM_RAY_MIN_HITS {
        return None;
    }
    let far = |i: usize| hits[i].unwrap_or(ROOM_RAY_MISS).max(0.0);
    let min = eye - Vec3::new(far(1), far(3), far(5));
    let max = eye + Vec3::new(far(0), far(2), far(4));
    Some(box_from_game((min + max) * 0.5, Mat3::IDENTITY, (max - min) * 0.5))
}

/// The six physics rays from the eye (Bevy space) in play mode, past actors, dynamic bodies
/// and what a character passes through (the filter of combat_tactics.rs `room`, actors
/// left out by `no_actors`; the player's own capsule is left out by `PhysState::ray`).
pub fn ray_box(phys: &mut nvf_play::phys::PhysState, eye: Vec3) -> Option<RoomBox> {
    let at = crate::world::bevy_to_game(eye);
    let filter = fnv_phys::Filter { no_actors: true, no_dynamic: true, exclude_layers: fnv_phys::layers::character_ignores(), ..Default::default() };
    let hits = ROOM_RAY_DIRECTIONS.map(|d| phys.ray(at.to_array(), d, ROOM_RAY_REACH, filter.clone()).map(|h| h.distance));
    box_from_rays(at, hits)
}

/// The default box around the eye (Bevy space): [`DEFAULT_HALF_WIDTH`] each way, its floor
/// [`EYE_HEIGHT`] under the eye, [`DEFAULT_HEIGHT`] high.
pub fn default_box(eye: Vec3) -> RoomBox {
    let at = crate::world::bevy_to_game(eye);
    let floor = at.z - EYE_HEIGHT;
    box_from_game(Vec3::new(at.x, at.y, floor + DEFAULT_HEIGHT * 0.5), Mat3::IDENTITY, Vec3::new(DEFAULT_HALF_WIDTH, DEFAULT_HALF_WIDTH, DEFAULT_HEIGHT * 0.5))
}

/// A light's brightness (fade x scale) from its Bevy intensity: the inverse of
/// lights.rs `lumens` (4 pi^2 x brightness x light unit).
pub fn brightness(intensity: f32) -> f32 {
    intensity / (4.0 * PI * PI * crate::sky::light_unit())
}

/// A placed light as a lamp (Bevy world space): its display colour times its steady
/// brightness (a flickering or pulsing light's full intensity, lights.rs `MovingLight`),
/// its range as radius, and a spot light's cone along its forward axis, as Bevy aims it.
pub fn lamp_of(position: Vec3, forward: Vec3, point: Option<&PointLight>, spot: Option<&SpotLight>, full: Option<f32>) -> Option<Lamp> {
    let (color, intensity, radius, cone) = match (spot, point) {
        (Some(s), _) => (s.color, s.intensity, s.range, Some(Spot::new(forward, s.inner_angle, s.outer_angle))),
        (None, Some(p)) => (p.color, p.intensity, p.range, None),
        (None, None) => return None,
    };
    let c = color.to_linear();
    Some(Lamp { position, colour: Vec3::new(c.red, c.green, c.blue) * brightness(full.unwrap_or(intensity)), radius, spot: cone })
}

/// The lamps that light the cube (module notes, "Indoors"): of `candidates` (entity bits,
/// lamp), those with a colour above [`COLOUR_STEP`], within [`LAMP_DISTANCE`] of the eye and
/// passing `keep`, the nearest [`MAX_LAMPS`] (ties by id), sorted by id.
pub fn pick_lamps(candidates: &[(u64, Lamp)], eye: Vec3, keep: impl Fn(&Lamp) -> bool) -> Vec<LampInput> {
    let mut near: Vec<(f32, LampInput)> = candidates
        .iter()
        .filter(|(_, l)| l.colour.max_element() > COLOUR_STEP && l.radius > 0.0 && l.position.distance(eye) <= LAMP_DISTANCE && keep(l))
        .map(|(id, l)| (l.position.distance(eye), LampInput { id: *id, lamp: *l }))
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
    near.truncate(MAX_LAMPS);
    let mut out: Vec<LampInput> = near.into_iter().map(|(_, l)| l).collect();
    out.sort_by_key(|l| l.id);
    out
}

/// The room's inputs from its box and source and the candidate lamps; `None` (stage 1's
/// content) with no lamp reaching the box and only the default box.
pub fn room_inputs(eye: Vec3, source: BoxSource, room: RoomBox, candidates: &[(u64, Lamp)], sun: Vec3, sun_dir: Vec3, fog_range: Option<[f32; 3]>) -> Option<RoomInputs> {
    let lamps = pick_lamps(candidates, eye, |l| l.reaches(&room));
    if lamps.is_empty() && source == BoxSource::Default {
        return None;
    }
    Some(RoomInputs { eye, source, room, lamps, sun, sun_dir, fog_range })
}

/// The ground lamps' inputs outdoors: each lamp's sphere must cross the ground plane
/// and come within [`OUTDOOR_LAMP_REACH`] of the eye.
pub fn ground_inputs(eye: Vec3, ground: f32, candidates: &[(u64, Lamp)]) -> Option<GroundInputs> {
    let lamps = pick_lamps(candidates, eye, |l| {
        (l.position.y - ground).abs() < l.radius && l.position.distance(eye) - l.radius <= OUTDOOR_LAMP_REACH
    });
    (!lamps.is_empty()).then_some(GroundInputs { eye, ground, lamps })
}

/// Why the world camera has no cube, or that it may have one ([`decide`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Not yet known: the reflection gate is still to be installed, or there is no world
    /// camera yet.
    Wait,
    Off(&'static str),
    Allocate,
}

/// The attach conditions (module notes, "Whether"): `row` the run's Reflections on authored
/// materials, `authored` its Authored materials, `has_sets` whether the mount has an authored
/// set (asked only when both rows are on), `gate` the reflection gate's state, `camera`
/// whether the world camera exists.
pub fn decide(row: bool, authored: bool, has_sets: impl FnOnce() -> bool, gate: GateState, camera: bool) -> Decision {
    if !row {
        return Decision::Off("setting");
    }
    if !authored {
        return Decision::Off("Authored materials off");
    }
    if !has_sets() {
        return Decision::Off("no authored sets in the mount");
    }
    match gate {
        GateState::Failed => Decision::Off("reflection gate not installed"),
        GateState::Pending => Decision::Wait,
        GateState::Installed if camera => Decision::Allocate,
        GateState::Installed => Decision::Wait,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Waiting,
    Off(&'static str),
    On,
}

/// The producer's state: the run's two rows (main.rs), the phase, the newest asked fill's
/// inputs, the fill in flight and the counts for the summary.
#[derive(Resource, Default)]
pub struct ReflectionSky {
    row: bool,
    authored: bool,
    phase: Phase,
    /// The inputs of the newest fill asked (running, waiting or applied): later inputs are
    /// compared with these, so a fill in flight is not asked again.
    last: Option<Inputs>,
    /// The camera and cube the last applied fill went into (an inherited light of an older
    /// cube is removed against it).
    target: Option<(Entity, AssetId<Image>)>,
    /// The camera and the cube it held (none: a cube is made when the fill is applied) when
    /// the newest fill was asked: each camera and cube needs a fill even if its lighting
    /// inputs equal the previous ones.
    asked: Option<(Entity, Option<AssetId<Image>>)>,
    /// The fill on the worker and the request waiting for it (module notes, "Where").
    queue: FillQueue<FillRequest, Task<Filled>>,
    /// The dome resource's change tick at the newest fill asked, or absent for the fallback.
    profile_revision: Option<u32>,
    last_check: Option<f64>,
    /// The cheap per-frame key of the last check (indoors, interior cell, room marker): a
    /// change skips the wait ([`CHECK_INTERVAL`]).
    key: Option<(bool, Option<u32>, Option<u32>)>,
    fills_outdoors: u64,
    fills_indoors: u64,
    unchanged: u64,
    /// Room fills by box source: marker, rays, default.
    room_fills: [u64; 3],
    ground_fills: u64,
    /// The slowest room or ground-lamp fill on the worker, milliseconds.
    slowest_ms: f32,
    /// Refills by reason, in [`Refill::ALL`]'s order.
    refills: [u64; 8],
    /// The main thread's slowest check (gathering and comparing the inputs, asking a fill)
    /// and slowest apply (moving a finished fill into the cube), milliseconds.
    slowest_gather_ms: f32,
    slowest_apply_ms: f32,
    /// Finished fills discarded because their camera or cube was replaced meanwhile.
    discarded: u64,
    /// Every specular cube a fill went into (one per camera or cube replacement): a light of
    /// one of them on a camera holding another cube is stale ([`drop_inherited_light`]).
    filled_cubes: std::collections::HashSet<AssetId<Image>>,
    /// The tests' seam: while set, fills started on the worker wait (never set in the game).
    hold: Option<Arc<AtomicBool>>,
    /// The last interior whose room fill was logged, and whether that fill had lamps.
    logged_room_fill: Option<(Option<u32>, bool)>,
    /// The last interior whose fill with stage 1's content was logged.
    logged_plain_cell: Option<Option<u32>>,
}

impl ReflectionSky {
    pub fn new(row: bool, authored: bool) -> ReflectionSky {
        ReflectionSky { row, authored, ..default() }
    }

    /// Log a room's first fill and its first lamp-lit fill if their lights arrive later.
    fn mark_room_fill_for_log(&mut self, cell: Option<u32>, has_lamps: bool) -> bool {
        if self.logged_room_fill.is_some_and(|(last, had_lamps)| last == cell && (had_lamps || !has_lamps)) {
            return false;
        }
        self.logged_room_fill = Some((cell, has_lamps));
        true
    }

    /// The summary line (without its prefix).
    pub fn line(&self) -> String {
        match self.phase {
            Phase::Off(reason) => format!("off ({reason})"),
            Phase::Waiting => "no cube made (waiting for the reflection gate or the world camera)".to_string(),
            Phase::On => {
                let refills: Vec<String> = Refill::ALL.iter().zip(self.refills).map(|(r, n)| format!("{n} {}", r.name())).collect();
                format!(
                    "{} fills ({} outdoors, {} indoors), {} checks skipped as unchanged; {} room fills ({} marker, {} rays, {} default), {} ground-lamp fills, slowest room or lamp fill {:.2} ms; refills: {}; main thread slowest gather {:.2} ms, slowest apply {:.2} ms; {} fills for a replaced camera or cube discarded",
                    self.fills_outdoors + self.fills_indoors,
                    self.fills_outdoors,
                    self.fills_indoors,
                    self.unchanged,
                    self.room_fills.iter().sum::<u64>(),
                    self.room_fills[0],
                    self.room_fills[1],
                    self.room_fills[2],
                    self.ground_fills,
                    self.slowest_ms,
                    refills.join(", "),
                    self.slowest_gather_ms,
                    self.slowest_apply_ms,
                    self.discarded
                )
            }
        }
    }
}

/// What the gathering reads beside the sky (module notes, "Indoors").
pub struct Surroundings<'a> {
    /// The world camera's position (Bevy space).
    pub eye: Vec3,
    pub cell: Option<u32>,
    pub rooms: Option<&'a crate::rooms::Rooms>,
    /// Play mode's physics, for the rays.
    pub phys: Option<&'a mut nvf_play::phys::PhysState>,
    /// The placed lights that are on (entity bits, lamp).
    pub lamps: Vec<(u64, Lamp)>,
    /// The player's no-interior-fog tweak.
    pub no_interior_fog: bool,
}

/// The directional light as display colour x strength (as sky.rs sets its illuminance) and
/// the direction toward it.
fn directional(sun: Option<(&DirectionalLight, &Transform)>) -> (Vec3, Vec3) {
    match sun {
        Some((light, t)) => {
            let colour = light.color.to_linear();
            let strength = light.illuminance / (PI * crate::sky::light_unit());
            (Vec3::new(colour.red, colour.green, colour.blue) * strength, *t.back())
        }
        None => (Vec3::ZERO, Vec3::Y),
    }
}

/// The inputs now (module notes, "What"), from the sky's state, the directional light, the
/// interior cell, the room and the lamps.
fn gather(sky: &SkyState, sun: Option<(&DirectionalLight, &Transform)>, mut s: Surroundings) -> Inputs {
    let (sun, sun_dir) = directional(sun);
    match sky.interior {
        Some(l) => {
            let shown = |v: [u8; 3]| Vec3::from_array(v.map(|b| b as f32 / 255.0));
            let (source, room) = match marker_box(s.rooms, s.cell, s.eye) {
                Some((id, b)) => (BoxSource::Marker(id), b),
                None => match s.phys.as_deref_mut().and_then(|p| ray_box(p, s.eye)) {
                    Some(b) => (BoxSource::Rays, b),
                    None => (BoxSource::Default, default_box(s.eye)),
                },
            };
            let fog_range = (!s.no_interior_fog).then_some([l.fog_near.max(0.0), l.fog_far, l.fog_power]);
            let room = room_inputs(s.eye, source, room, &s.lamps, sun, sun_dir, fog_range);
            Inputs::Indoors { cell: s.cell, ambient: shown(l.ambient), fog: shown(l.fog_color), room }
        }
        None => {
            let c = |t: ColorType| Vec3::from_array(sky.color(t));
            // The ground under the eye: by a ray in play mode, else EYE_HEIGHT under it; only
            // looked for with a lamp near.
            let near = s.lamps.iter().any(|(_, l)| l.position.distance(s.eye) - l.radius <= OUTDOOR_LAMP_REACH);
            let ground = near.then(|| {
                let at = crate::world::bevy_to_game(s.eye);
                let filter = fnv_phys::Filter { no_actors: true, no_dynamic: true, exclude_layers: fnv_phys::layers::character_ignores(), ..Default::default() };
                let below = s.phys.as_deref_mut().and_then(|p| p.ray(at.to_array(), [0.0, 0.0, -1.0], GROUND_RAY_REACH, filter)).map(|h| h.distance);
                ground_inputs(s.eye, s.eye.y - below.unwrap_or(EYE_HEIGHT), &s.lamps)
            });
            Inputs::Outdoors {
                horizon: c(ColorType::Horizon),
                lower: c(ColorType::SkyLower),
                upper: c(ColorType::SkyUpper),
                ambient: c(ColorType::Ambient).map(crate::lights::linear_to_display),
                sun,
                sun_dir,
                sky_scale: sky.sky_scale(),
                clip: crate::lights::display_clip(),
                ground: ground.flatten(),
            }
        }
    }
}

/// The interior lines: the first room fill and the first room fill with lamps, if the
/// lights arrive later. Report a fill with stage 1's content separately.
/// `ms` is the fill's time on the worker, `gather_ms` and `apply_ms` the main thread's.
fn log_interior(state: &mut ReflectionSky, cell: Option<u32>, room: Option<&RoomInputs>, samples: &FillSamples, ms: f32, gather_ms: f32, apply_ms: f32) {
    let name = cell.map_or("unknown".to_string(), |c| format!("{c:08X}"));
    match room {
        Some(r) if state.mark_room_fill_for_log(cell, !r.lamps.is_empty()) => {
            let size = r.room.size();
            info!(
                "reflection sky: room fill in interior {name}: box from {} ({:.0} x {:.0} x {:.0} game units, x y height), {} lamps, level 0 toward the nearest lamp {:.4?}, filled in {ms:.2} ms on a worker (main thread: gather {gather_ms:.2} ms, apply {apply_ms:.2} ms)",
                r.source,
                size.x,
                size.y,
                size.z,
                r.lamps.len(),
                samples.nearest_lamp.unwrap_or(Vec3::ZERO).to_array()
            );
        }
        None if state.logged_plain_cell != Some(cell) => {
            state.logged_plain_cell = Some(cell);
            info!("reflection sky: interior {name}: no lamp in reach and no room box, so stage 1's interior content");
        }
        _ => {}
    }
}

/// The placed lights (lights.rs `SessionLight`, cell-owned) as the room reads them.
type PlacedLights<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static GlobalTransform, Option<&'static PointLight>, Option<&'static SpotLight>, Option<&'static crate::lights::MovingLight>, Option<&'static InheritedVisibility>),
    With<crate::lights::SessionLight>,
>;

type ReflectionCamera<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static GlobalTransform, Option<&'static mut ReflectionCube>, Option<&'static EnvironmentMapLight>),
    With<FlyCam>,
>;

/// Make the world camera's cube when [`decide`] allows, then rewrite it when its inputs
/// moved (module notes).
#[allow(clippy::too_many_arguments)]
pub fn produce(
    mut commands: Commands,
    mut state: ResMut<ReflectionSky>,
    data: Option<Res<nvf_assets::GameData>>,
    sky: Option<Res<SkyState>>,
    streamer: Option<Res<crate::world::Streamer>>,
    profile: Option<Res<DomeProfile>>,
    sun: Query<(&DirectionalLight, &Transform), With<Sun>>,
    mut cams: ReflectionCamera,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time<Real>>,
    rooms: Option<Res<crate::rooms::Rooms>>,
    mut phys: Option<ResMut<nvf_play::phys::PhysState>>,
    lights: PlacedLights,
    tweaks: Option<Res<nvf_play::tweaks::TweakSettings>>,
) {
    if matches!(state.phase, Phase::Off(_)) {
        return;
    }
    let Some(sky) = sky else { return };
    let camera = cams.single_mut();
    if state.phase == Phase::Waiting {
        let (row, authored) = (state.row, state.authored);
        match decide(row, authored, || data.as_deref().is_some_and(nvf_assets::authored::mount_has_sets), reflection_cube::gate_state(), camera.is_ok()) {
            Decision::Wait => return,
            Decision::Off(reason) => {
                state.phase = Phase::Off(reason);
                info!("reflection sky: off ({reason}); no reflection cube is made");
                return;
            }
            Decision::Allocate => {}
        }
    }
    let Ok((entity, transform, cube, environment)) = camera else { return };
    let mut current = cube.as_ref().map(|cube| cube.specular.id());
    // A finished fill goes into the cube it was asked for; inputs asked while it ran start on
    // the worker. A fill for another camera or a replaced cube is discarded: the change of
    // target asks a fill of its own (below).
    let hold = state.hold.clone();
    match state.queue.poll(check_ready, |r| spawn_fill(r, hold.clone())) {
        Polled::Done(request, filled) if request.target == (entity, current) => {
            current = Some(apply_fill(&mut state, &mut commands, &mut images, entity, cube, environment, request, filled));
        }
        Polled::Done(..) => state.discarded += 1,
        Polled::Idle | Polled::Running => {}
    }
    let target = (entity, current);
    let target_changed = state.asked != Some(target);
    if target_changed {
        // A new camera or cube shows no older cube's light while its own fill runs.
        drop_inherited_light(&state, &mut commands, entity, environment, current);
    }
    let eye = transform.translation();
    let cell = streamer.as_ref().and_then(|s| s.interior);
    // A new camera or cube, a change of room, or a change to the outdoor dome skips the wait.
    let key = (sky.interior.is_some(), cell, marker_box(rooms.as_deref(), cell, eye).map(|(id, _)| id));
    let profile_revision = profile.as_ref().map(|p| p.last_changed().get());
    let profile_changed = !key.0 && profile_revision != state.profile_revision;
    let prompt = target_changed || state.key != Some(key) || profile_changed;
    let now = time.elapsed_secs_f64();
    if state.phase == Phase::On && !prompt && state.last_check.is_some_and(|t| now - t < CHECK_INTERVAL) {
        return;
    }
    // The main thread's part of a fill: gathering the inputs into owned data and comparing
    // them (module notes, "Where").
    let started = Instant::now();
    state.last_check = Some(now);
    state.key = Some(key);
    let lamps: Vec<(u64, Lamp)> = lights
        .iter()
        .filter(|(.., vis)| vis.is_none_or(|v| v.get()))
        .filter_map(|(e, gt, point, spot, moving, _)| lamp_of(gt.translation(), gt.forward().into(), point, spot, moving.map(|m| m.full)).map(|l| (e.to_bits(), l)))
        .collect();
    let surroundings = Surroundings {
        eye,
        cell,
        rooms: rooms.as_deref(),
        phys: phys.as_deref_mut(),
        lamps,
        no_interior_fog: tweaks.as_ref().is_some_and(|t| t.no_interior_fog),
    };
    let inputs = gather(&sky, sun.single().ok(), surroundings);
    let reason = state.last.as_ref()
        .and_then(|last| inputs.change(last).or(profile_changed.then_some(Refill::Profile)));
    if !target_changed && reason.is_none() {
        state.unchanged += 1;
        state.slowest_gather_ms = state.slowest_gather_ms.max(started.elapsed().as_secs_f32() * 1000.0);
        return;
    }
    let profile = profile.as_deref().cloned().unwrap_or_else(DomeProfile::fallback);
    let gather_ms = started.elapsed().as_secs_f32() * 1000.0;
    // The cube's first fill logs the hemisphere means: a new cube, or a cube no fill went into.
    let means = target.1.is_none_or(|id| state.target != Some((entity, id)));
    let request = FillRequest { inputs: inputs.clone(), profile, reason, gather_ms, target, means };
    let hold = state.hold.clone();
    state.queue.ask(request, |r| spawn_fill(r, hold));
    state.last = Some(inputs);
    state.asked = Some(target);
    state.profile_revision = profile_revision;
    state.phase = Phase::On;
    state.slowest_gather_ms = state.slowest_gather_ms.max(started.elapsed().as_secs_f32() * 1000.0);
}

/// Move a finished fill into the camera's cube (a new cube when it has none), drop a light
/// inherited from an older cube, count it and log it; the cube's specular image is returned.
/// Only bytes move here: the texels were made on the worker (module notes, "Where").
#[allow(clippy::too_many_arguments)]
fn apply_fill(
    state: &mut ReflectionSky,
    commands: &mut Commands,
    images: &mut Assets<Image>,
    entity: Entity,
    cube: Option<Mut<ReflectionCube>>,
    environment: Option<&EnvironmentMapLight>,
    request: FillRequest,
    filled: Filled,
) -> AssetId<Image> {
    let started = Instant::now();
    let Filled { levels, samples, ms } = filled;
    let specular = if let Some(mut cube) = cube {
        cube.set_levels(levels);
        cube.specular.id()
    } else {
        let mut cube = ReflectionCube::new(images);
        cube.set_levels(levels);
        let specular = cube.specular.id();
        commands.entity(entity).insert(cube);
        // Fills asked for this camera before it had a cube go into the one just made.
        for r in state.queue.requests_mut() {
            if r.target == (entity, None) {
                r.target.1 = Some(specular);
            }
        }
        if state.asked == Some((entity, None)) {
            state.asked = Some((entity, Some(specular)));
        }
        specular
    };
    state.filled_cubes.insert(specular);
    // A camera can retain or inherit the light from an older cube (produce removes it when
    // the target changes; again here for a light added since). Preserve unrelated maps.
    drop_inherited_light(state, commands, entity, environment, Some(specular));
    let apply_ms = started.elapsed().as_secs_f32() * 1000.0;
    state.slowest_apply_ms = state.slowest_apply_ms.max(apply_ms);
    let FillRequest { inputs, reason, gather_ms, .. } = request;
    if let Some(i) = Refill::ALL.iter().position(|&r| Some(r) == reason) {
        state.refills[i] += 1;
    }
    if inputs.indoors() {
        state.fills_indoors += 1;
    } else {
        state.fills_outdoors += 1;
    }
    match &inputs {
        Inputs::Indoors { room: Some(r), .. } => {
            state.room_fills[match r.source {
                BoxSource::Marker(_) => 0,
                BoxSource::Rays => 1,
                BoxSource::Default => 2,
            }] += 1;
            state.slowest_ms = state.slowest_ms.max(ms);
        }
        Inputs::Outdoors { ground: Some(_), .. } => {
            state.ground_fills += 1;
            state.slowest_ms = state.slowest_ms.max(ms);
        }
        _ => {}
    }
    if let Inputs::Indoors { cell, room, .. } = &inputs {
        log_interior(state, *cell, room.as_ref(), &samples, ms, gather_ms, apply_ms);
    }
    let first = state.target != Some((entity, specular));
    state.target = Some((entity, specular));
    if first {
        let (above, below) = samples.means.unwrap_or_default();
        let where_ = if inputs.indoors() { "indoors" } else { "outdoors" };
        info!(
            "reflection sky: first fill of the world camera's cube ({where_}; level 0 zenith {:.4?}, horizon {:.4?}, nadir {:.4?}; mean above {:.4?}, below {:.4?}), attached once on the GPU",
            samples.zenith.to_array(),
            samples.horizon.to_array(),
            samples.nadir.to_array(),
            above.to_array(),
            below.to_array()
        );
    }
    specular
}

/// The exit summary (Last, on the app's exit): one line, `reflection sky summary: ...`.
pub fn report(mut exit: MessageReader<AppExit>, state: Res<ReflectionSky>) {
    if exit.read().last().is_none() {
        return;
    }
    info!("reflection sky summary: {}", state.line());
}

/// The producer and its summary; `row` and `authored` are the run's two rows, fixed at the
/// start (main.rs).
pub struct ReflectionSkyPlugin {
    pub row: bool,
    pub authored: bool,
}

impl Plugin for ReflectionSkyPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ReflectionSky::new(self.row, self.authored))
            .add_systems(Update, produce.after(crate::sky::update_sky))
            .add_systems(Last, report);
    }
}

#[cfg(test)]
mod tests;
