//! Shadows of placed lights, the settings row ALCS (experimental) (ALCS: All Lights Cast
//! Shadows; called ALC shadows until 2026-11-24 and Lamp shadows until 2026-11-19: the INI
//! key `light-shadows`, the switch `--light-shadows` and
//! the log lines `light shadows: …` keep that name, for saved settings and the smoke
//! scenarios; docs/design/graphics-enhanced.md §3.17). An addition of NVForever's own, not a fact
//! about the original game: there placed lights cast none, and no LIGH record of the
//! masters sets the Spot Shadow flag [observed: docs/handoff/esm-data-audit-2026-10-25.md,
//! item 6]. Off by default. Built as the sun's shadows are (shadow_rate.rs), [design,
//! #3074]: "look how sun shadows are calculated and do the same", "use the same
//! performance optimizations also for the lamp shadows".
//!
//! - **Which lights: by distance, as the sun's shadows.** Every drawn placed LIGH light
//!   ([`SessionLight`] with a point or spot light) whose light reaches within the sun's
//!   shadow distance of the camera (`sky::SkyState::shadow_distance` × its reach: the
//!   Shadow distance and Longer sun shadows rows) casts, nearest first, up to
//!   [`Tuning::lights`] (a safety cap for the Strip at night). Not by view: a light behind
//!   the camera keeps casting, so turning around draws nothing anew. A light already
//!   casting keeps its place against one [`KEEP`] times nearer.
//! - **Held maps, as the sun's held cascades.** A point light's cube, or a spot light's
//!   map, is drawn when it starts casting and then kept (`bevy::pbr::HoldLightShadowMaps`, a
//!   patched bevy_pbr: engine/vendor/README.md "held light shadow maps"). Between draws the
//!   light's maps are switched off for Bevy (`shadow_maps_enabled` false: no views, no
//!   culling, nothing queued, which is what a shadowed point light costs, about 1 ms a frame
//!   each on the CPU, measured), and the patched renderer still samples them. Each light
//!   keeps a slot while it casts, so others coming and going do not move it: a point light
//!   a cube of the cube texture, a spot light a layer of the sun's texture past its
//!   cascades. Each texture keeps room for its slots, grown in fours ([`grown_room`]; the
//!   spot lights' also for the other shadowed spot lights, the flashlight,
//!   [`MapTextureInputs`]). A texture the renderer makes anew (the room grew, the Shadow
//!   resolution row, the sun's cascades changed) holds no light's maps: its lights draw
//!   again as if new.
//! - **Redrawn where something changed.** A face is marked when, since it was drawn, a
//!   shadow-casting shape moved (its bounds' centre or corner more than [`MOVE_EPS`], so a
//!   turn in place counts), appeared or was shown or hidden with its bounds in the face, a
//!   joint of an actor's skeleton moved that far there or one of its shapes was shown or
//!   hidden, or the light itself moved more than [`LIGHT_MOVE_EPS`] from where its maps were
//!   last drawn in full (all its faces then). A shape or a skeleton that goes (an item
//!   taken, an actor leaving) marks the faces it was in ([`ShapeAt`]). The thresholds lie
//!   under the shadow biases ([`DEPTH_BIAS`]): a caster never stands in the shadow of where
//!   its map still shows it. Every light is redrawn in full every [`Tuning::refresh`] frames
//!   anyway, each at its own time.
//! - **Casters not ready** ([`SETTLE_FRAMES`], [`BUILD_WAIT`]). A light whose maps were
//!   drawn while a caster's mesh or material was still loading, or its shadow pipeline still
//!   being built (the patched renderer tells, `bevy::pbr::nvf_take_unready_shadow_lights`:
//!   engine/vendor/README.md "ALCS unready casters"), draws again in full a little later,
//!   and again while the renderer says so. Until 2026-11-24 only the refresh did: after a
//!   load a room's furniture cast no shadow for up to 24 s.
//! - **In the frame it changed** (the rewrite of 2026-11-24, [design, a play test: "the
//!   shadows do not update real time"]). A light with marked faces in view is switched on to
//!   draw them in that frame, up to [`Tuning::draws`] lights a frame: those that never drew
//!   first, then those whose marked faces waited longest ([`draw_priority`]), so with more
//!   marked lights than draws each takes its turn. Of a light switched on, only the marked
//!   faces are drawn; the others keep their maps. A light that has never drawn casts nothing
//!   until it has. The 10-s performance line counts the lights that waited and the longest
//!   wait. (Until then lights took turns on every frame or every second or fourth, three a
//!   frame at most with the nearest always first, and a skeleton marked only when the box
//!   round all its joints changed by 4 units: a talking actor went dark in its own old
//!   shadow, and a carried thing left its shadow behind.)
//! - **Other lights** ([`ShadowSource`], [design, #3074]: all light sources cast). A
//!   grass fire's light (grass_fire_fx.rs) and a light a script attached to a node (JIP
//!   AttachLight, nif_mirror.rs) rank and count with the placed lights. Muzzle flashes and
//!   explosions' lights (shot_fx.rs) are transient ([`SourceKind::transient`]): chosen apart
//!   from the placed lights, nearest first, when their light reaches within
//!   [`TRANSIENT_REACH`] of the camera (and the sun's shadow distance) and into the camera's
//!   view this frame ([`in_view`]: a new light was not seen last frame), up to
//!   [`Tuning::transients`], with their own [`Tuning::transient_draws`] a frame; so a
//!   flash's cube is drawn in the frame it appears, and a firefight and the lamps do not
//!   starve each other. A transient draws its cube once and holds it for the rest of its
//!   short life (redrawn as recoil moved it, a flash drew 3.5 times on average). The effects' own shapes are `NotShadowCaster` (shot_fx.rs
//!   `spawn_effect`, `spawn_playing_itself`), and the gun a flash sits at is a small shape
//!   it sits against ([`skip_fixtures`]).
//! - **Spot lights** (few among the placed ones) are held as the point lights are: switched
//!   on only on the frames their map draws (marked and their turn come, never-drawn first),
//!   counting against [`Tuning::draws`] (a transient's against [`Tuning::transient_draws`]).
//! - **Fading** ([`FADE_SECS`]). A light's shadow fades in once its maps were first drawn.
//!   A light no longer chosen (out of range, swapped at the cap) is not dropped at once: it
//!   stays held, its slot kept and nothing drawn, while its shadow fades out, and does not
//!   count against [`Tuning::lights`] or [`Tuning::transients`]; then it is dropped
//!   ([`leaving_fade`]). A light despawned is dropped at once. Transients do not fade: a
//!   muzzle flash lasts about a tenth of a second. The fade goes to the renderer in
//!   `HoldLightShadowMaps::fade`, which scales the shadow term (`mix(1, shadow, fade)`).
//! - **What casts**: the sun's layers (`sky::SHADOW_LAYER`, `sky::BODY_LAYER` and the
//!   world's layer 0), so the player's body casts a lamp's shadow as it casts the sun's;
//!   the first-person arms do not.
//! - **Light fixtures cast none** ([`LightFixture`], [`skip_fixtures`]): the shapes of a
//!   model named as a lamp, and of a light's own model, are taken out of every casting
//!   light's maps, as is a small shape a light sits in or against. The game hangs its
//!   lights under their lamps, so a lamp's housing threw its shadow, larger than the lamp,
//!   onto the ceiling over it ([design, #3074]). The sun's shadows keep them.
//! - **Near plane** [`NEAR_Z`]: what lies nearer the light than that casts nothing from it,
//!   so the bulb, shade or flames a light sits in do not black out the room
//!   [engineering choice; `NVF_LIGHT_SHADOW_NEAR` overrides it for a run].
//! - **Maps**: on Quality the cubes are in two textures of two sides ([`cube_sides`]: 1024
//!   and 512): the [`Tuning::near`] lights nearest the camera in the near one, the rest in the
//!   far one ([`near_set`]; engine/vendor/README.md "ALCS far maps"). A light that has to
//!   change draws all its faces in the frame it does ([`LampState::reslot`]). On Performance
//!   one texture, 512 with Shadow resolution 2048 and 1024 with 4096 (`PointLightShadowMap`); a spot light draws one layer of the sun's texture (its
//!   side, so each slot of the spot room costs a layer of 2048 or 4096 texels square).
//! - The diffuse light of the display-value sum takes the shadow
//!   (`lights::display_shader`), as the flashlight's always has. The flashlight is not one
//!   of these lights: it casts whenever it is on (flashlight.rs). Nor are the lights of the
//!   menu and preview scenes (creator_view.rs, vigor_view.rs, poker_view.rs) or the sky's.
//! - Test aid `NVF_PLAY_TEST_LIGHT_SHADOW_HOLD=0`: every casting light draws every face
//!   every frame, for comparing the picture and the cost.
//! - **Performance pass** (2026-11-18, [design, #3074]: "the quality of the shadow can be reduced
//!   with distance"; graphics-enhanced.md §3.17 "Performance pass"; none measured in game
//!   yet), each with an aid that turns it off:
//!   - Steps of detail by the camera's distance to a light's edge ([`STEPS`], hysteresis
//!     [`STEP_HYSTERESIS`]) and the small-caster cut-off ([`cut_off`],
//!     [`CUTOFF_TEXELS`]): a caster under a few texels of the light's map is taken out of
//!     its lists in [`skip_fixtures`] and its moves mark nothing on it; a light stepping
//!     finer after a cut draws again in full. `NVF_LIGHT_SHADOW_CUTOFF=0`.
//!   - A grid of shadow casters hands each drawing lamp its candidates for Bevy's culling
//!     (light_shadow_casters.rs). `NVF_LIGHT_SHADOW_CASTER_GRID=0`.
//!   - A light's shadow fades out over the last [`DISTANCE_FADE`] of the reach
//!     ([`distance_fade`]). `NVF_LIGHT_SHADOW_DISTANCE_FADE=0`.
//!   - Marked faces the camera cannot see stay held until they come into view
//!     ([`faces_in_view`]). `NVF_LIGHT_SHADOW_FACE_CULL=0`.
//!   - Moved shapes are marked through a [`LampGrid`], skipping lamps that draw in full
//!     anyway; lamps are marked changed for Bevy only when their settings change.
//!   - Log: besides the summary, every 10 s `light shadows: renderer over …` (the patched
//!     renderer's light views, events and `prepare_lights` times, and these systems'
//!     times, [`renderer_summary`], [`systems_summary`]) and `light shadows: performance
//!     pass over …` ([`pass_summary`]).
//! - **Static shadow cache** (2026-11-18, [design, #3074]: "Since most of the lights in New Vegas are
//!   static, could we not bake the shadows at least partially?"; graphics-enhanced.md §3.17
//!   "Static shadow cache"; not measured in game yet). Shadow casters are static (the
//!   placed world) or dynamic ([`ShadowDynamic`], [`classify_casters`]: skinned shapes,
//!   actors' and creatures' shapes, the player's body, and any shape once it moves after it
//!   marked the lamps, [`shape_change`]). Each cached point lamp keeps a second cube (the
//!   patched renderer's cache texture, same slots and size: engine/vendor/README.md "static
//!   shadow cache") drawn from the static casters alone. A face whose cache is out of date
//!   ([`LampState::static_dirty`]: a static shape appeared, vanished, was shown, hidden or
//!   moved in it, the light's first draw, a removal of static shapes, a refresh, a finer
//!   step) is drawn into the cache and copied into the sampled cube; a face marked only by
//!   dynamic casters is composited: the cache's face copied in and the dynamic casters drawn
//!   over it, the depth loaded ([`face_modes`]; the casters' lists split in
//!   [`skip_fixtures`], [`keeps_caster`]). A face drawn into the cache whose list held
//!   dynamic casters is composited next, before other lights ([`put_back`]); a dynamic
//!   caster leaving a face marks it (where it was and is, [`merged_sphere`]), and its
//!   composite with none left is the cache copied back. Transients, spot lights and lights
//!   that move keep the plain path ([`plain_path`]). The cache costs as much memory as the
//!   cubes ([`cube_texture_mb`], logged). Aid `NVF_LIGHT_SHADOW_STATIC_CACHE=0`: the plain
//!   path for all, as before. The 10-s performance line ends with its counts.
//! - **Pre-drawn under the loading screen** ([design, #3074], with the cache). While a
//!   loading screen is up (`nvf_play::loading::Loading::active`: a travel, a cover, the start)
//!   and the renderer has the cells around the camera (`Loading::render_busy` false; the
//!   destination's lights are spawned with its cells and chosen by distance to the camera,
//!   which is at the destination by then), every chosen light the camera sees draws all its
//!   marked faces at once, with no budget, no turns and no face culling ([`predrawing`],
//!   [`takes_budget`]). Once none of those has maps left to draw, the chosen lights the
//!   camera does not see draw too, [`PREDRAW_UNSEEN_PER_FRAME`] a frame, nearest first
//!   ([`shown_unseen`]): Bevy culls a point light's shadow casters only for lights a camera
//!   sees (vendored bevy_light `check_point_light_mesh_visibility`), so they are shown to it
//!   through the patched bevy_light's `NvfShadowLightsShown`, which finds their casters and
//!   marks them visible for the renderer (engine/vendor/README.md "shown shadow lights");
//!   their faces go into the static cache as any. The screen waits for it
//!   (`Loading::lamp_shadows_busy`, "ALC shadows" in its phases line) while lamps drew in the
//!   last frame or any chosen lamp still has marked faces, at most [`PREDRAW_CAP_SECS`]
//!   ([`predraw_busy`]). When the screen comes down: `light shadows: <n> lights pre-drawn
//!   under the loading screen (<f> faces, <s> into the static cache); <c> casting, <u> of
//!   them not drawn yet; <o> of those pre-drawn out of view`. Aid
//!   `NVF_LIGHT_SHADOW_PREDRAW=0` (none pre-drawn, in view or not).
//! - **Kept shadow phases** (2026-11-19, the patched bevy_pbr: engine/vendor/README.md "kept
//!   shadow phases"). A held light's shadow phases, pipeline keys and specialized pipelines
//!   outlive its views while it keeps its slot, so switching it on again queues and
//!   specializes only what changed since (Bevy's renderer threw them away with the views and
//!   did all of it again: a share of the 1.4 ms a switch-on cost); and a light re-extracted
//!   with the same place, range, near plane and layers keeps its views as they are. The
//!   renderer line's events `phases reused`, `kept phases wiped` and `views kept` count them;
//!   `kept phases resynced` counts kept phases back in view resynced with their light's whole
//!   visible list, `entities rebinned` entities queued while binned (none expected), `stale
//!   entries requeued` the entries such a phase still held with an input uniform index no
//!   longer theirs: a mesh nothing saw while the lamp's views were away loses its mesh
//!   instance, and has another index when seen again; its old entry drew it with whatever
//!   mesh's transform that index held by then (slabs and shards of shadow around the player
//!   in the Primm casino, 2026-11-25).
//!   Aid `NVF_LIGHT_SHADOW_KEEP_PHASES=0` (phases go with the views, as before).

use bevy::camera::primitives::{Aabb, CubemapFrusta, Frustum, Sphere, CUBE_MAP_FACES};
use bevy::camera::visibility::{CubemapVisibleEntities, RenderLayers, VisibilitySystems, VisibleMeshEntities};
use bevy::ecs::entity::{EntityHashMap, EntityHashSet};
use bevy::light::{CascadeShadowConfig, DirectionalLightShadowMap, NotShadowCaster, NvfShadowLightsShown, PointLightShadowMap, SimulationLightSystems};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::HoldLightShadowMaps;
use bevy::prelude::*;
use bevy::render::extract_resource::ExtractResourcePlugin;
use bevy::transform::TransformSystems;

use crate::lights::{MovingLight, SessionLight};

/// `--light-shadows=<mode>`: the mode lamp shadows start in.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightShadowMode {
    Off,
    Performance,
    Quality,
}

/// The row's choices, in the order the page and the INI keep them (the Sun shadows row's).
/// An Ultra choice (the near lamps' maps at the Shadow resolution row's full side, 2048 or
/// 4096, the far ones at 1024; 1.8 GB for nine lamps) was built and dropped on 2026-11-24:
/// "basically not noticeable even at 4k native" [observed: a play test].
pub const CHOICES: [&str; 3] = ["Off", "Performance", "Quality"];

/// The ALC shadows row's choice: an index into [`CHOICES`] (0: off).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightShadows {
    pub choice: usize,
}

impl LightShadows {
    pub fn of(mode: LightShadowMode) -> Self {
        Self { choice: mode as usize }
    }
}

/// How a mode spends its frame time [engineering choices, measured in
/// graphics-enhanced.md §3.17].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// At most this many lights cast at once, nearest first.
    pub lights: usize,
    /// At most this many lights (point or spot) switched on to draw in a frame: those
    /// whose marked faces waited longest first ([`draw_order`]).
    pub draws: usize,
    /// Every light is drawn again in full every this many frames anyway (a safety net: a
    /// change the marks missed does not stay for good).
    pub refresh: u64,
    /// The point lights nearest the camera kept in the near cube texture, the larger one
    /// ([`cube_sides`]); the rest are in the far one. 0: one texture for all.
    pub near: usize,
    /// At most this many transient lights (muzzle flashes, explosions: [`SourceKind`]) cast
    /// at once, nearest first; they do not count against [`Tuning::lights`].
    pub transients: usize,
    /// At most this many transient lights switched on to draw in a frame, besides
    /// [`Tuning::draws`], so a firefight does not starve the lamps nor the lamps it.
    pub transient_draws: usize,
}

/// The tuning of a choice; `None` for Off.
pub fn tuning(choice: usize) -> Option<Tuning> {
    match choice {
        1 => Some(Tuning { lights: 16, draws: 3, refresh: 3600, near: 0, transients: 1, transient_draws: 1 }),
        2 => Some(Tuning { lights: 32, draws: 8, refresh: 3600, near: 8, transients: 2, transient_draws: 2 }),
        _ => None,
    }
}

/// Lights whose light reaches within this far of the camera count as near (game units,
/// about 21 m) [engineering choice: the room around the player].
pub const NEAR: f32 = 1500.0;

/// A light already casting keeps its place against one this many times nearer.
pub const KEEP: f32 = 1.25;

/// Shadow near plane of a placed light, game units (about 11 cm, as the flashlight's)
/// [engineering choice, 2026-11-18: 16 at first, to clear the bulbs and shades lights sit
/// in; since the fixtures are skipped (`LightFixture`, [`FIXTURE_MARGIN`]) 8 and 4 drew
/// the same pictures in the General Store and the Prospector Saloon, and a wall nearer
/// than 16 let the light through].
pub const NEAR_Z: f32 = 8.0;

/// A shape (its bounds' centre or corner) or a joint of a skeleton counts as moved for the
/// lamps' maps once it is this far from where it last marked them (game units, about 7 mm)
/// [engineering choice, 2026-11-24: under the biases ([`DEPTH_BIAS`]), so a caster never
/// stands in the shadow of where its map still shows it. It was 4 units, of a shape's centre
/// and of the box round all of a skeleton's joints: a head or an arm moving within that box
/// marked nothing, and a talking actor leaning back a few units went dark in its own old
/// shadow until the box changed (playtest videos 2026-11-24, Doc Mitchell)].
pub const MOVE_EPS: f32 = 0.5;

/// A light whose maps were drawn while casters of its could not be drawn yet (a mesh or a
/// material still loading, a shadow pipeline still being built: the renderer's word,
/// `bevy::pbr::nvf_take_unready_shadow_lights`) draws again in full this many frames
/// later, and again while the renderer says so, each time after the next wait; a change
/// in how many are missing starts over. After the last it is left to [`Tuning::refresh`]
/// [engineering choice, 2026-11-24: about 20 s at 150 frames a second in all, for a first
/// run with a new picture mode, whose pipelines are built for seconds. Until then
/// nothing drew such a light again: a room's furniture, loaded a moment after the lamps'
/// maps were drawn under the loading screen, cast no shadow until the refresh, 24 s later
/// (the player's save, loaded: pictures at 0.5 to 42 s)].
pub const SETTLE_FRAMES: [u64; 10] = [4, 8, 16, 32, 64, 128, 256, 512, 1024, 1024];

/// While pipelines are still being built (a first run with a new picture mode builds them
/// for many seconds) a light drawn with no caster known missing draws again every this
/// many frames, at most [`BUILD_WAITS`] times: a caster binned earlier may wait for one of
/// them, and nothing tells which [engineering choice: with [`SETTLE_FRAMES`] alone the
/// furniture's shadows were still missing 12 s into a first run with MSAA, TAA or FSR].
pub const BUILD_WAIT: u64 = 48;
pub const BUILD_WAITS: u8 = 80;

/// A light with no report of unready casters for this many frames starts its
/// [`SETTLE_FRAMES`] steps and [`BUILD_WAITS`] over at the next one (twice the longest step)
/// [engineering choice, 2026-11-25: they were never given back, so a light that had spent
/// them at a loading screen drew no more for casters that came minutes later].
pub const UNREADY_QUIET: u64 = 2048;

/// The frame a light told unready at `frame` draws again, and its next step; `None` once
/// the steps are spent. Under a loading screen (`predraw`) at once: the screen waits for it.
pub fn settle_after(frame: u64, step: u8, predraw: bool) -> Option<(u64, u8)> {
    let wait = *SETTLE_FRAMES.get(usize::from(step))?;
    Some((frame + if predraw { 2 } else { wait }, step + 1))
}

/// The casting lights' shadow biases: `DEPTH_BIAS` game units towards the light (about
/// 1.4 cm), and `NORMAL_BIAS` map texels along the surface normal (Bevy scales it by the
/// texel's size at the fragment) [engineering choice, 2026-11-24. Bevy's defaults were kept
/// until then: its depth bias, 0.08, is a length in world units, 8 cm in its metres and a
/// millimetre in the game's units, so every lit surface shadowed itself in stripes].
/// `NVF_LIGHT_SHADOW_BIAS=<depth>,<normal>` overrides them for a run.
pub const DEPTH_BIAS: f32 = 1.0;
pub const NORMAL_BIAS: f32 = 1.0;

/// A casting light counts as moved once it is this far from where its maps were last
/// marked or drawn in full (game units, about 11 cm) [engineering choice, 2026-11-18: a
/// light an animation or script sways by a few units is not drawn again each frame].
/// Transients are not redrawn at all once drawn.
pub const LIGHT_MOVE_EPS: f32 = MOVE_EPS;

/// Transient lights cast only when their light reaches within this far of the camera
/// (game units, about 43 m), and within the sun's shadow distance [engineering choice: a
/// firefight around the player; farther flashes are small on screen and gone in a tenth of
/// a second].
pub const TRANSIENT_REACH: f32 = 3000.0;

/// A light's shadow fades in over this long once its maps were first drawn, and out over
/// this long once it stops casting (real seconds) [engineering choice: no popping when a
/// light comes into range, leaves it or is swapped at the cap; short enough that a light
/// coming into range shows its shadow at once]. Transients do not fade.
pub const FADE_SECS: f32 = 0.3;

/// On a light other than a placed one that casts lamp shadows (main world): what it is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowSource(pub SourceKind);

/// The lights besides the placed LIGH lights that cast lamp shadows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// A weapon's muzzle flash (shot_fx.rs): transient, moves with the gun.
    MuzzleFlash,
    /// An explosion's fading light (shot_fx.rs): transient, still.
    Explosion,
    /// A burning grass patch's light (grass_fire_fx.rs): ranked with the placed lights.
    GrassFire,
    /// A light a script attached to a node (JIP AttachLight, nif_mirror.rs): ranked with the
    /// placed lights, moves with its node.
    Attached,
}

impl SourceKind {
    /// Lights that live a moment and are chosen apart from the placed ones
    /// ([`Tuning::transients`], [`TRANSIENT_REACH`]).
    pub fn transient(self) -> bool {
        matches!(self, Self::MuzzleFlash | Self::Explosion)
    }
}

/// On a shape of a light fixture: a model named as a lamp ([`is_fixture_model`]) or a
/// light's own model (a LIGH with a model). It casts no lamp shadows; the sun's it still
/// casts. A playtest (2026-11-18, [design, #3074]): a hanging box lamp threw its housing's shadow, larger than the
/// lamp, onto the ceiling above it. The game hangs its lights below their fixtures: in the
/// Goodsprings General Store the light under the box lamp `OffRmLightHanging01` sits 86
/// units under the lamp's reference; over the masters' interiors 3,909 of the 7,543 LIGH
/// references without a model of their own have a lamp model's reference within 160 units,
/// half of them within 89 [observed 2026-11-18: `fnvtool world refs --interiors`].
#[derive(Component)]
pub struct LightFixture;

/// The words in a model's file name that make it a light fixture [engineering choice from
/// the census in [`LightFixture`]: `offrmlight01`, `lightceilingvaultr02on`,
/// `shackhanginglight02`, `lamplightlamp`, `neoclassicalsconce`, …].
const FIXTURE_WORDS: [&str; 7] = ["light", "lamp", "lantern", "chandelier", "sconce", "bulb", "candle"];

/// Whether a model (a path under `meshes\`) is a light fixture: its file name holds one of
/// [`FIXTURE_WORDS`].
pub fn is_fixture_model(path: &str) -> bool {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path).to_ascii_lowercase();
    FIXTURE_WORDS.iter().any(|w| file.contains(w))
}

/// A shape not named as a fixture is one for a light that lies within this far of its box
/// (game units, about 23 cm): a bulb or shade of another name the light sits
/// in [engineering choice].
pub const FIXTURE_MARGIN: f32 = 16.0;

/// Such a shape's bounding sphere is no larger than this (game units, about 1.8 m), so a
/// wall, floor, ceiling or room piece next to a light is never one [engineering choice: the
/// interior kits' pieces are 256 units and more].
pub const FIXTURE_RADIUS: f32 = 128.0;

/// The faces of a point light's cube (bit `i` for face `i`).
const ALL_FACES: u8 = 0b11_1111;

/// On a placed light while it casts shadows.
#[derive(Component)]
pub struct CastingLight;

/// On a shadow-casting shape that moves (the static shadow cache, [`static_cache_on`]): it
/// is left out of the lamps' static caches and drawn over them. Put by [`classify_casters`]
/// on a skinned shape, on every shape of an actor or creature (under an `ActorRig` or a
/// `PlayerRig`) and of the player's body (`sky::SHADOW_LAYER`, `sky::BODY_LAYER`), and by
/// [`shadow_lights`] on any other shape once it moves after it marked the lamps (clutter
/// physics pushed, a door or activator that turns, what a script moves): [`shape_change`].
#[derive(Component)]
pub struct ShadowDynamic;

/// `NVF_LIGHT_SHADOW_STATIC_CACHE=0` (a measuring aid): no static shadow cache; every face
/// drawn is drawn with every caster, as before 2026-11-18.
fn static_cache_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let off = std::env::var("NVF_LIGHT_SHADOW_STATIC_CACHE").is_ok_and(|v| v.trim() == "0");
        if off {
            info!("light shadows: NVF_LIGHT_SHADOW_STATIC_CACHE=0, no static shadow cache: every caster drawn into every face");
        }
        !off
    })
}

/// What a shadow-casting shape's change does to the lamps' maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeChange {
    /// A static shape appeared, vanished, or was shown or hidden (a cell loading, Primm's
    /// scripted mass enable): the static caches of the faces it is in are drawn again.
    Static,
    /// A dynamic shape moved, appeared, vanished, or was shown or hidden: the faces it was and
    /// is in are composited again; no cache is drawn.
    Dynamic,
    /// A static shape moved after it marked the lamps: from now on it is dynamic, and the
    /// caches of the faces it was and is in are drawn again without it.
    Promoted,
}

/// What a shape's change is: `dynamic` it already is ([`ShadowDynamic`]); `shown_or_hidden`
/// its visibility changed (or it is new); `known` it marked the lamps before, from where it
/// is said to have moved.
pub fn shape_change(dynamic: bool, shown_or_hidden: bool, known: bool) -> ShapeChange {
    if dynamic {
        ShapeChange::Dynamic
    } else if shown_or_hidden || !known {
        ShapeChange::Static
    } else {
        ShapeChange::Promoted
    }
}

/// The sphere holding both spheres: a moved caster marks where it was and where it is.
pub fn merged_sphere(a: &Sphere, b: &Sphere) -> Sphere {
    let (ca, cb) = (Vec3::from(a.center), Vec3::from(b.center));
    let d = ca.distance(cb);
    if d + b.radius <= a.radius {
        return *a;
    }
    if d + a.radius <= b.radius {
        return *b;
    }
    let radius = (d + a.radius + b.radius) * 0.5;
    // From a's far side towards b, `radius` along.
    let centre = ca + (cb - ca) * ((radius - a.radius) / d.max(1.0e-6));
    Sphere { center: centre.into(), radius }
}

/// How a light's faces `draw` this frame are drawn: those whose static cache must be drawn
/// again (`static_dirty`) into the cache; the rest composited (the cache copied in, the
/// dynamic casters drawn over). A face whose composite is due (`pending`) takes it first and
/// its cache's redraw waits a frame: a cache redrawn every frame would never show its moving
/// casters. Of the faces drawn into the cache, those that held dynamic casters at their last
/// draw (`dynamic_faces`) are drawn quietly: the cache is not copied into the sampled map,
/// which keeps its picture until the composite of the next frame, so the moving casters'
/// shadows never leave it (until 2026-11-24 they blinked off for a frame at every redraw of
/// a cache, with every periodic refresh). `(to_static, composite, quiet)`; all 0 for a
/// light on the plain path (`cached` false).
pub fn face_modes(draw: u8, static_dirty: u8, pending: u8, dynamic_faces: u8, cached: bool) -> (u8, u8, u8) {
    if !cached {
        return (0, 0, 0);
    }
    let to_static = draw & static_dirty & !pending;
    (to_static, draw & !to_static, to_static & dynamic_faces)
}

/// Which casters a face's list keeps: on the plain path all; drawn into the static cache
/// the static ones; composited the dynamic ones.
pub fn keeps_caster(to_static: bool, composite: bool, dynamic: bool) -> bool {
    match (to_static, composite) {
        (true, _) => !dynamic,
        (false, true) => dynamic,
        _ => true,
    }
}

/// The faces drawn into the static cache this frame (`to_static`) whose lists held dynamic
/// casters (`dynamic_in`): the sampled map lacks them (copied out without them, or, drawn
/// quietly, not copied out at all), so they are composited next, first.
pub fn put_back(to_static: u8, dynamic_in: u8) -> u8 {
    to_static & dynamic_in
}

/// Where a shadow-casting shape was when it last marked the lamps: its bounds' centre and
/// one corner (so a turn in place counts as a move), its bounding radius, and whether it is
/// dynamic ([`ShadowDynamic`], known here from the frame it first moves).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeAt {
    pub centre: Vec3,
    pub corner: Vec3,
    pub radius: f32,
    pub dynamic: bool,
}

impl ShapeAt {
    fn of(t: &GlobalTransform, aabb: &Aabb, dynamic: bool) -> Self {
        let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
        Self { centre: t.transform_point(c), corner: t.transform_point(c + h), radius: h.length() * max_scale(t), dynamic }
    }

    /// Whether the shape, now at `now`, moved more than [`MOVE_EPS`] from here.
    pub fn moved(&self, now: &Self) -> bool {
        let eps = MOVE_EPS * MOVE_EPS;
        self.centre.distance_squared(now.centre) > eps || self.corner.distance_squared(now.corner) > eps
    }

    fn sphere(&self) -> Sphere {
        Sphere { center: self.centre.into(), radius: self.radius }
    }
}

/// Where a skeleton's joints were when it last marked the lamps, their box, and how many of
/// its skinned shapes were shown.
#[derive(Debug, Clone)]
struct SkeletonAt {
    joints: Vec<Vec3>,
    lo: Vec3,
    hi: Vec3,
    shapes: u32,
}

/// A skinned shape's skeleton as the marking keeps it: its whole list of joints, hashed.
/// Shapes with the same list share one entry and are gone over once a frame. Until
/// 2026-11-25 the key was the list's first joint alone, and a body cut into parts has
/// lists that start with the same joint and differ after it (a protectron's four parts:
/// two lists under each of two first joints), so only the first part's joints were
/// watched: the others' (the protectron's hands) marked nothing by themselves and their
/// shadow moved only when the watched part's joints did (playtest 2026-11-25, Primm Slim).
pub fn skeleton_key(joints: &[Entity]) -> u64 {
    use std::hash::BuildHasher;
    bevy::platform::hash::FixedHasher.hash_one(joints)
}

/// Whether any joint moved more than [`MOVE_EPS`] between two poses of a skeleton (or the
/// skeleton changed).
pub fn joints_moved(last: &[Vec3], now: &[Vec3]) -> bool {
    last.len() != now.len() || last.iter().zip(now).any(|(a, b)| a.distance_squared(*b) > MOVE_EPS * MOVE_EPS)
}

/// A light's place in the frame's draws by how many frames its marked faces have waited:
/// the longest wait first, so with more marked lights than [`Tuning::draws`] each gets its
/// turn (until 2026-11-24 the same nearest few drew every frame and the rest never while
/// something kept marking them); a light the camera is inside of counts double.
pub fn draw_priority(waiting: u32, inside: bool) -> u32 {
    waiting.saturating_mul(if inside { 2 } else { 1 })
}

/// `NVF_LIGHT_SHADOW_PREDRAW=0` (a measuring aid): no pre-draw under loading screens; the
/// lamps draw on their budget and turns there as in play, and the screen does not wait for
/// them.
fn predraw_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let off = std::env::var("NVF_LIGHT_SHADOW_PREDRAW").is_ok_and(|v| v.trim() == "0");
        if off {
            info!("light shadows: NVF_LIGHT_SHADOW_PREDRAW=0, no pre-draw under loading screens");
        }
        !off
    })
}

/// The loading screen waits for the pre-draw this long at most, real seconds from when the
/// renderer had its cells [engineering choice: a lamp's maps draw in a frame or two once its
/// light is seen; a shape that kept marking them must not hold the screen for long].
pub const PREDRAW_CAP_SECS: f32 = 3.0;

/// Whether the lamps pre-draw this frame: a mode on, the aid not set, a loading screen up
/// and the renderer done with its cells (before that the lamps would draw again for every
/// cell that comes in).
pub fn predrawing(mode_on: bool, aid_on: bool, screen_up: bool, render_busy: bool) -> bool {
    mode_on && aid_on && screen_up && !render_busy
}

/// Whether the loading screen still waits for the pre-draw: lamps drew this frame (`drew`:
/// their composites may follow) or seen lamps still have marked faces (`waiting`), within
/// [`PREDRAW_CAP_SECS`] of its start (`secs`).
pub fn predraw_busy(drew: bool, waiting: bool, secs: f32) -> bool {
    (drew || waiting) && secs < PREDRAW_CAP_SECS
}

/// Under a loading screen, at most this many lamps the camera does not see are shown to
/// Bevy's culling a frame to draw their maps (nearest first) [engineering choice: a frame of
/// the pre-draw stays short, so the screen's own picture keeps moving; 32 lamps in four
/// frames].
pub const PREDRAW_UNSEEN_PER_FRAME: usize = 8;

/// Which lamps the camera does not see are shown this frame of a pre-draw: none while a lamp
/// in view still has maps to draw (`seen_waiting`: those go first), else the `cap` nearest of
/// `unseen` (`(id, gap)`, the camera's distance to its light's edge) that have maps to draw.
pub fn shown_unseen<T: Copy>(unseen: &mut [(T, f32)], seen_waiting: bool, cap: usize) -> Vec<T> {
    if seen_waiting {
        return Vec::new();
    }
    unseen.sort_by(|a, b| a.1.total_cmp(&b.1));
    unseen.iter().take(cap).map(|u| u.0).collect()
}

/// Whether a light with maps to draw may draw: always without a limit (`unlimited`: the
/// hold aid, a pre-draw), else while the frame's `budget` lasts, which it takes one from.
pub fn takes_budget(budget: &mut usize, unlimited: bool) -> bool {
    if unlimited {
        return true;
    }
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    true
}

/// Whether a light's maps take the plain path: the cache off, every map drawn every frame
/// (the hold aid), a transient (drawn once), a spot light (one map, few among the lamps:
/// [engineering choice, left on the old path]), or a light that moved (its cache would be
/// drawn again each time).
pub fn plain_path(cache_on: bool, hold_all: bool, transient: bool, spot: bool, moved: bool) -> bool {
    !cache_on || hold_all || transient || spot || moved
}

/// What a casting light's maps need (main world).
#[derive(Debug, Clone, Copy)]
struct LampState {
    /// Faces changed since they were last drawn (bit 0 for a spot light).
    dirty: u8,
    /// It has not drawn yet (its maps hold nothing of it).
    fresh: bool,
    /// Frame its maps were last drawn in full.
    refreshed: u64,
    /// A number of its own, for its turn in the staggered schedule.
    phase: u64,
    /// Where the light was when its maps were last marked or drawn in full.
    at: Vec3,
    /// A muzzle flash or an explosion ([`SourceKind::transient`]).
    transient: bool,
    /// A spot light (its map is a layer of the sun's texture), not a point light.
    spot: bool,
    /// How much of its shadow it casts, 0..1 ([`FADE_SECS`]).
    fade: f32,
    /// No longer chosen: held, not drawn, while its fade falls to 0.
    leaving: bool,
    /// Its step of detail by distance ([`STEPS`]), and its maps' texels across a unit at a
    /// unit's distance ([`map_scale`]): together the size under which a caster is left out
    /// of its maps ([`cut_off`]).
    step: u8,
    map_scale: f32,
    /// A caster was left out of its maps since the light last stepped to finer detail: a
    /// step finer draws its maps again so that caster comes back.
    cut_any: bool,
    /// Its shadow's strength by distance, 0..1 ([`distance_fade`]); with [`Self::fade`] it
    /// makes what the renderer gets.
    far_fade: f32,
    /// Marked faces held last frame because the camera could not see them
    /// ([`faces_in_view`]): one coming into view draws before other lights' faces.
    deferred: u8,
    /// The static shadow cache ([`static_cache_on`]): faces whose cache (the static casters'
    /// map) must be drawn again; always within [`Self::dirty`].
    static_dirty: u8,
    /// Faces drawn into the cache whose lists held dynamic casters: the sampled face lacks
    /// them until it is composited, which goes before other lights' turns.
    pending: u8,
    /// Its maps take the plain path, every caster drawn into the sampled map: a transient, a
    /// spot light, a light that moved, or the cache off.
    plain: bool,
    /// This frame's faces drawn into the cache, composited from it, and of the first those
    /// drawn quietly ([`face_modes`]).
    to_static: u8,
    composite: u8,
    quiet: u8,
    /// The faces whose lists held dynamic casters when the light last drew.
    dynamic_faces: u8,
    /// Frames its marked faces in view have waited for a draw ([`draw_priority`]).
    waiting: u32,
    /// In the far cube texture ([`Tuning::near`]), not the near one.
    far: bool,
    /// It changed cube texture ([`RESLOTS_PER_FRAME`]): its new cubes hold nothing of it, so
    /// it draws all its faces at once with every caster (no cache), in the frame of the
    /// change, and its shadow never leaves the picture; the caches follow.
    reslot: bool,
    /// Casters the renderer could not draw into its maps at its last draw, the frame it
    /// draws again in full for them (0: none due) and how often it has ([`SETTLE_FRAMES`]).
    unready: u32,
    /// The frame of the renderer's last such report ([`UNREADY_QUIET`]).
    unready_frame: u64,
    settle_at: u64,
    settle_step: u8,
    /// Redraws while pipelines were being built ([`BUILD_WAITS`]).
    build_waits: u8,
    /// The faces it was told to draw this frame, and whether that was its first draw: given
    /// back in [`skip_fixtures`] when the camera lost sight of the light this frame and the
    /// renderer drew nothing ([`Pass::lost`]).
    drew: u8,
    drew_fresh: bool,
}

/// The casting lights' slots and states.
#[derive(Resource, Default)]
struct LampShadowState {
    lamps: EntityHashMap<LampState>,
    /// Stable slots of the casting point lights in the cube texture (the near one).
    slots: EntityHashMap<u32>,
    /// The cube texture's room, in lights; it only grows while a mode stays on.
    capacity: u32,
    /// The same for the far cube texture ([`Tuning::near`]): a point light has a slot in
    /// one of the two.
    far_slots: EntityHashMap<u32>,
    far_capacity: u32,
    far_maps_key: Option<(usize, u32, bool)>,
    /// The row's choice last frame.
    choice_was: usize,
    /// Stable slots of the casting spot lights past the sun's cascades in its texture, and
    /// the room kept there for them; it only grows while a mode stays on.
    spot_slots: EntityHashMap<u32>,
    spot_capacity: u32,
    /// What the cube texture (its side, its room) and the sun's texture (its side, the spot
    /// room, the cascades) were made for last frame: when either changes the renderer takes
    /// a new texture, which holds no light's maps.
    point_maps_key: Option<(usize, u32, bool)>,
    spot_maps_key: Option<(usize, u32, usize)>,
    frame: u64,
    /// Lights switched on and faces drawn since the last summary, and frames counted.
    switched: u64,
    drawn: u64,
    /// The same for the transient lights, kept apart: those that started casting, were
    /// switched on, and their faces drawn, since the last summary.
    transients_cast: u64,
    transients_switched: u64,
    transients_drawn: u64,
    /// What marked faces since the last summary: shapes, skeletons, removals, the lights
    /// themselves (moved or their full redraw due); and a few of the shapes' names.
    causes: [u64; 4],
    names: Vec<String>,
    /// Where each shape (its bounds' centre) and each skeleton (by its first joint; its joints'
    /// box) was when it last marked the lamps, and the frame: moves under [`MOVE_EPS`] add up until they
    /// reach it.
    shapes_at: EntityHashMap<ShapeAt>,
    skeletons_at: bevy::platform::collections::HashMap<u64, SkeletonAt>,
    /// Every caster in the world was looked at since the row was switched on.
    seeded: bool,
    frames: u64,
    since: f32,
    /// The performance pass's counts since the last summary ([`Pass`]).
    pass: Pass,
    /// What the lamps drew under the loading screen that is up ([`predrawing`]).
    predraw: Predraw,
    /// The point lights' cube side before a mode set its own, given back when the row is
    /// switched off.
    cube_before: Option<usize>,
}

/// The lamps' pre-draw under one loading screen.
#[derive(Debug, Default)]
struct Predraw {
    /// A loading screen was up last frame.
    screen: bool,
    /// Real seconds (`Time<Real>` elapsed) when the pre-draw began under this screen.
    began: Option<f32>,
    /// Lights drawn, their faces, and of those the faces drawn into the static cache.
    lights: EntityHashSet,
    faces: u64,
    static_faces: u64,
    /// Lights drawn while the camera did not see them (shown to Bevy's culling,
    /// [`shown_unseen`]).
    unseen: EntityHashSet,
}

/// What the performance pass's techniques did since the last summary (its own 10-s line).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Pass {
    /// Casters left out of a drawing light's lists by the small-caster cut-off ([`cut_off`]).
    pub cut_casters: u64,
    /// Marks of a moved shape or skeleton on a light it is under the cut-off for, skipped.
    pub cut_marks: u64,
    /// Full redraws for a light stepping to finer detail with casters cut before.
    pub step_redraws: u64,
    /// Casting lights at each step, summed over the frames.
    pub steps: [u64; 3],
    /// The caster grid's counts (light_shadow_casters.rs `take_grid_counts`): lamps served,
    /// candidates over them, meshes filed, those of them listed for every light.
    pub grid: [u64; 4],
    /// Casting lights whose shadow was faded by distance, summed over the frames.
    pub distance_faded: u64,
    /// Marked faces held on their light's turn because the camera could not see them.
    pub faces_out_of_view: u64,
    /// The static shadow cache: faces drawn into it (each then copied into the sampled map),
    /// faces composited from it (copied in, the dynamic casters drawn over), and of the faces
    /// drawn into it those whose dynamic casters were put back by a composite after.
    pub static_faces: u64,
    pub composited: u64,
    pub put_back: u64,
    /// Shapes that moved after they were in the static caches and became dynamic.
    pub promoted: u64,
    /// Faces drawn into the cache quietly (not copied out; composited next).
    pub quiet: u64,
    /// Lights told to draw that the camera lost sight of in the same frame: marked again.
    pub lost: u64,
    /// Lights with marked faces in view that did not draw in a frame (the frame's draws
    /// were spent), summed over the frames, and the longest any waited, in frames.
    pub waited: u64,
    pub waited_longest: u32,
    /// Full redraws of lights whose maps were drawn without all their casters
    /// ([`SETTLE_FRAMES`]).
    pub settled: u64,
    /// Lights that changed cube texture ([`RESLOTS_PER_FRAME`]).
    pub reslots: u64,
    /// The renderer's reports behind them: views drawn with casters missing, and views
    /// drawn with none missing while a pipeline was being built.
    pub unready: [u64; 2],
    /// Lights pre-drawn under loading screens that came down since the last summary.
    pub predrawn: u64,
}

/// A light's shadow fades out over this last fraction of the reach it is chosen within (the
/// sun's shadow distance), instead of ending at it [engineering choice: a lamp leaving the
/// reach or coming into it no longer pops even before its time fade, [`FADE_SECS`]; Godot's
/// lights fade their shadows out by distance the same way (from a research pass)].
pub const DISTANCE_FADE: f32 = 0.2;

/// A light's shadow strength by distance: 1 until the last [`DISTANCE_FADE`] of `reach`,
/// then falling in a straight line to 0 at it; `gap` is the camera's distance to its
/// light's edge.
pub fn distance_fade(gap: f32, reach: f32) -> f32 {
    if reach <= 0.0 {
        return 1.0;
    }
    ((reach - gap) / (DISTANCE_FADE * reach)).clamp(0.0, 1.0)
}

/// `NVF_LIGHT_SHADOW_DISTANCE_FADE=0` (a measuring aid): no fade by distance, a light's
/// shadow ends at the reach as before 2026-11-18.
fn distance_fade_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let off = std::env::var("NVF_LIGHT_SHADOW_DISTANCE_FADE").is_ok_and(|v| v.trim() == "0");
        if off {
            info!("light shadows: NVF_LIGHT_SHADOW_DISTANCE_FADE=0, no fade by distance");
        }
        !off
    })
}

/// The system that chooses the casting lights and switches their maps on to draw
/// (`shadow_lights`), for ordering against it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChooseLamps;

/// How many frames the ALC shadows' systems still run after the row is switched off (or
/// from the start with it off): enough for [`shadow_lights`] to switch every light's maps
/// off and give the renderer an inactive hold, and for the soft edges, contact shadows and
/// caster candidates to be given back, whatever order they run in.
pub const WIND_DOWN_FRAMES: u8 = 3;

/// Frames since [`shadow_lights`] last ran with the row on (saturating at
/// [`WIND_DOWN_FRAMES`]); see [`live`].
#[derive(Resource, Default, Debug)]
pub struct LightShadowsIdle {
    frames: u8,
}

/// Whether the ALC shadows' systems run this frame: with the row on, and for
/// [`WIND_DOWN_FRAMES`] frames after it is off. With the row Off none of them runs (their
/// run condition only), and the renderer's hold is inactive, upstream's paths
/// (bevy_pbr `HoldLightShadowMaps::active`).
pub fn live(settings: Res<LightShadows>, idle: Res<LightShadowsIdle>) -> bool {
    runs(settings.choice, idle.frames)
}

/// [`live`] for a row choice and the idle frames.
pub fn runs(choice: usize, idle_frames: u8) -> bool {
    choice != 0 || idle_frames < WIND_DOWN_FRAMES
}

pub struct LightShadowsPlugin(pub LightShadowMode);

impl Plugin for LightShadowsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LightShadows::of(self.0))
            .init_resource::<LampShadowState>()
            .init_resource::<LightShadowsIdle>()
            .init_resource::<MapTextureInputs>()
            .init_resource::<HoldLightShadowMaps>()
            .init_resource::<NvfShadowLightsShown>()
            .add_plugins(ExtractResourcePlugin::<HoldLightShadowMaps>::default())
            .add_systems(PostUpdate, watch_map_textures.before(shadow_lights).run_if(live))
            .add_systems(PostUpdate, classify_casters.before(shadow_lights).run_if(live))
            .add_systems(
                PostUpdate,
                // Before Bevy resets this frame's view visibility, so the lights' and the
                // shapes' `ViewVisibility` still say what the last frame saw.
                // After the camera's frustum is updated: a transient draws in its first frame
                // when its light reaches into this frame's view.
                shadow_lights
                    .in_set(ChooseLamps)
                    .after(TransformSystems::Propagate)
                    .after(VisibilitySystems::UpdateFrusta)
                    .before(VisibilitySystems::VisibilityPropagate)
                    .before(SimulationLightSystems::AssignLightsToClusters)
                    .before(SimulationLightSystems::UpdateLightFrusta)
                    .run_if(live),
            )
            .add_systems(PostUpdate, skip_fixtures.after(SimulationLightSystems::CheckLightVisibility).run_if(live))
            .add_plugins(crate::light_shadow_casters::LightShadowCastersPlugin);
    }
}

/// What else decides the size of the sun's shadow texture the spot lights' maps are in
/// (main world): the other shadowed spot lights (the flashlight), which take slots of their
/// own past the lamps' (bevy_pbr's patched `prepare_lights`), and the sun's cascades, which
/// come before them.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
struct MapTextureInputs {
    other_spots: u32,
    cascades: usize,
}

/// A directional light draws at most this many cascades (Bevy's `MAX_CASCADES_PER_LIGHT`)
/// [documented: engine/vendor/bevy_pbr/src/render/light.rs].
const MAX_CASCADES: usize = 4;

/// Count what [`MapTextureInputs`] holds. Separate from [`shadow_lights`], whose query
/// holds the lamps' `SpotLight`s mutably.
fn watch_map_textures(
    mut inputs: ResMut<MapTextureInputs>,
    spots: Query<&SpotLight, (Without<SessionLight>, Without<ShadowSource>)>,
    suns: Query<(&DirectionalLight, &CascadeShadowConfig)>,
) {
    let now = MapTextureInputs {
        other_spots: spots.iter().filter(|s| s.shadow_maps_enabled).count() as u32,
        cascades: suns.iter().filter(|(d, _)| d.shadow_maps_enabled).map(|(_, c)| c.bounds.len().min(MAX_CASCADES)).sum(),
    };
    if *inputs != now {
        *inputs = now;
    }
}

/// A light's fade after `dt` real seconds, rising (it casts) or falling (it is leaving),
/// over [`FADE_SECS`]; within 0..1.
pub fn fade_step(fade: f32, rising: bool, dt: f32) -> f32 {
    let step = dt / FADE_SECS;
    (if rising { fade + step } else { fade - step }).clamp(0.0, 1.0)
}

/// A light no longer chosen: its fade after `dt` while it is held fading out, `None` once it
/// is dropped (at once when it despawned, `alive` false; never drew; or is a transient,
/// which does not fade).
pub fn leaving_fade(fade: f32, drawn: bool, transient: bool, alive: bool, dt: f32) -> Option<f32> {
    if !alive || !drawn || transient {
        return None;
    }
    let fade = fade_step(fade, false, dt);
    (fade > 0.0).then_some(fade)
}

/// The room a texture keeps for `slots` and `others` lights besides them: `room`, grown in
/// fours until it holds the highest slot and the others.
pub fn grown_room(room: u32, slots: impl Iterator<Item = u32>, others: u32) -> u32 {
    let needed = slots.map(|s| s + 1).max().unwrap_or(0) + others;
    room.max(needed.div_ceil(4) * 4)
}

/// Whether a texture made anew only grew: more room (`now` over `before`) and the rest of
/// its making the same (`same`: its side, its use).
pub fn regrown(before: u32, now: u32, same: bool) -> bool {
    same && now > before
}

/// Whether a lamp keeps its shadow through its texture's growth ([`regrown`]): one drawn
/// before that the camera sees, which draws again this frame. A lamp never drawn, a
/// transient or one out of sight starts over as in any new texture (nothing of it shows).
pub fn keeps_shadow(grew: bool, fresh: bool, transient: bool, in_sight: bool) -> bool {
    grew && !fresh && !transient && in_sight
}

/// `e`'s slot in `slots`: the one it has, else the lowest free one.
pub fn assign_slot(slots: &mut EntityHashMap<u32>, e: Entity) -> u32 {
    if let Some(&slot) = slots.get(&e) {
        return slot;
    }
    let slot = free_slot(slots.values().copied());
    slots.insert(e, slot);
    slot
}

/// The order of a light among the candidates: the distance from the camera to the edge of
/// its light (0 inside it), then to the light; a casting light's divided by [`KEEP`].
pub fn rank(distance: f32, range: f32, casting: bool) -> (f32, f32) {
    let k = if casting { KEEP } else { 1.0 };
    ((distance - range).max(0.0) / k, distance / k)
}

/// The lights that cast: the `cap` first of `(id, rank)` with a rank's edge distance
/// within `reach`, nearest first; ties keep the order given.
pub fn pick<T: Copy>(candidates: &mut [(T, (f32, f32))], reach: f32, cap: usize) -> Vec<T> {
    candidates.sort_by(|a, b| a.1 .0.total_cmp(&b.1 .0).then(a.1 .1.total_cmp(&b.1 .1)));
    candidates.iter().filter(|c| c.1 .0 <= reach).take(cap).map(|c| c.0).collect()
}

/// How near a transient light's edge must come to the camera to cast: [`TRANSIENT_REACH`],
/// or the sun's shadow distance when that is shorter.
pub fn transient_reach(shadow_reach: f32) -> f32 {
    shadow_reach.min(TRANSIENT_REACH)
}

/// Whether a light's sphere at `at` with `range` reaches into the camera's view (the far
/// plane left out, as Bevy's culling of lights does); without a frustum, yes. A light whose
/// sphere does is one Bevy's culling finds visible this frame (its box holds the sphere), so
/// its maps are drawn.
pub fn in_view(frustum: Option<&Frustum>, at: Vec3, range: f32) -> bool {
    frustum.is_none_or(|f| f.intersects_sphere(&Sphere { center: at.into(), radius: range }, false))
}

/// Whether a light now at `now` moved more than [`LIGHT_MOVE_EPS`] from `drawn_at`, where
/// its maps were last marked or drawn in full.
pub fn light_moved(drawn_at: Vec3, now: Vec3) -> bool {
    drawn_at.distance_squared(now) > LIGHT_MOVE_EPS * LIGHT_MOVE_EPS
}

/// The renderer's side of the 10-s summary, from the patched bevy_pbr's counts over its
/// frames since the last one (engine/vendor/README.md "light view counts"): the point and
/// spot light shadow views alive and run a frame (the flashlight's spot view among them),
/// those a hidden light took with it, what `prepare_lights` did, and the wall time of
/// `extract_lights`, `prepare_lights` and its stretches, mean and longest a frame.
pub fn renderer_summary(c: &bevy::pbr::NvfLightViewCounts) -> String {
    let frames = c.frames.max(1) as f64;
    let ms = |ns: u64| ns as f64 / 1.0e6;
    let events: Vec<String> = bevy::pbr::NVF_EVENTS.iter().zip(c.events).map(|(name, n)| format!("{name} {n}")).collect();
    let times: Vec<String> =
        bevy::pbr::NVF_SECTIONS.iter().enumerate().map(|(i, name)| format!("{name} {:.3}/{:.2}", ms(c.ns[i]) / frames, ms(c.max_ns[i]))).collect();
    format!(
        "light shadows: renderer over {} frames: light views a frame {:.2} alive, {:.2} run, {} despawned with their light hidden; {}; ms a frame mean/max: {}",
        c.frames,
        c.views as f64 / frames,
        c.run as f64 / frames,
        c.hidden,
        events.join(", "),
        times.join(", ")
    )
}

/// The lamp shadows' own systems whose wall time the 10-s summary gives (a measuring aid),
/// in the order of [`Timed`]'s index.
pub const SYSTEMS: [&str; 4] = ["shadow_lights", "skip_fixtures", "soften", "contact"];

static SYSTEM_NS: [std::sync::atomic::AtomicU64; 4] = [const { std::sync::atomic::AtomicU64::new(0) }; 4];
static SYSTEM_MAX_NS: [std::sync::atomic::AtomicU64; 4] = [const { std::sync::atomic::AtomicU64::new(0) }; 4];

/// Times a run of one of [`SYSTEMS`] (by index) until it is dropped.
pub struct Timed(usize, std::time::Instant);

impl Timed {
    pub fn start(system: usize) -> Self {
        Self(system, std::time::Instant::now())
    }
}

impl Drop for Timed {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering::Relaxed;
        let ns = self.1.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        SYSTEM_NS[self.0].fetch_add(ns, Relaxed);
        SYSTEM_MAX_NS[self.0].fetch_max(ns, Relaxed);
    }
}

/// The [`SYSTEMS`]' times since the last call (summed, longest; nanoseconds), and start
/// timing anew.
fn take_system_times() -> ([u64; 4], [u64; 4]) {
    use std::sync::atomic::Ordering::Relaxed;
    (std::array::from_fn(|i| SYSTEM_NS[i].swap(0, Relaxed)), std::array::from_fn(|i| SYSTEM_MAX_NS[i].swap(0, Relaxed)))
}

/// The [`SYSTEMS`]' times over `frames` game frames, mean and longest a frame (ms).
pub fn systems_summary(ns: &[u64; 4], max: &[u64; 4], frames: u64) -> String {
    let frames = frames.max(1) as f64;
    let parts: Vec<String> = SYSTEMS.iter().enumerate().map(|(i, name)| format!("{name} {:.3}/{:.2}", ns[i] as f64 / 1.0e6 / frames, max[i] as f64 / 1.0e6)).collect();
    format!("game systems ms a frame mean/max: {}", parts.join(", "))
}

/// The performance pass's line of the 10-s summary: what each technique did, a frame over
/// `frames` frames, and which are switched off by their test aids.
pub fn pass_summary(p: &Pass, frames: u64) -> String {
    let per = |n: u64| n as f64 / frames.max(1) as f64;
    let on = |b: bool| if b { "on" } else { "OFF" };
    let [served, candidates, filed, everywhere] = p.grid;
    format!(
        "light shadows: performance pass over {frames} frames: lights at steps near/mid/far {:.1}/{:.1}/{:.1} a frame; small-caster cut-off ({}) casters cut {:.1} a frame, marks skipped {:.2} a frame, redraws for a finer step {}; caster grid ({}) {:.2} lamps served a frame, {:.0} candidates each, {filed} meshes filed ({everywhere} for every light); distance fade ({}) {:.1} lights fading a frame; face culling ({}) {:.2} marked faces held out of view a frame; static cache ({}) {:.2} static redraws (faces) a frame, {:.2} dynamic recomposites a frame, {:.2} layer copies a frame, {} faces put back after a static redraw, {} of them drawn quietly, {} shapes became dynamic, {} draws lost out of view; {:.2} marked lights waited a frame for a draw, the longest {} frames; {} redraws for casters not ready ({} views drawn with casters missing, {} while a pipeline was built); {} lights changed cube texture; loading-screen pre-draw ({}) {} lights",
        per(p.steps[0]),
        per(p.steps[1]),
        per(p.steps[2]),
        on(cutoff_on()),
        per(p.cut_casters),
        per(p.cut_marks),
        p.step_redraws,
        on(crate::light_shadow_casters::grid_on()),
        per(served),
        candidates as f64 / served.max(1) as f64,
        on(distance_fade_on()),
        per(p.distance_faded),
        on(face_cull_on()),
        per(p.faces_out_of_view),
        on(static_cache_on()),
        per(p.static_faces),
        per(p.composited),
        // Each face drawn into the cache is copied out after, each composite copied in first.
        per(p.static_faces + p.composited),
        p.put_back,
        p.quiet,
        p.promoted,
        p.lost,
        per(p.waited),
        p.waited_longest,
        p.settled,
        p.unready[0],
        p.unready[1],
        p.reslots,
        on(predraw_on()),
        p.predrawn
    )
}

/// The lowest slot not in `used`.
pub fn free_slot(used: impl Iterator<Item = u32>) -> u32 {
    let mut taken: Vec<u32> = used.collect();
    taken.sort_unstable();
    let mut slot = 0;
    for t in taken {
        if t == slot {
            slot += 1;
        } else if t > slot {
            break;
        }
    }
    slot
}

/// Which of a point light's faces (or a spot light's map, bit 0) a sphere reaches, leaving
/// out (not testing) the faces in `skip`.
fn faces_reached(sphere: &Sphere, cube: Option<&CubemapFrusta>, spot: Option<&Frustum>, skip: u8) -> u8 {
    if let Some(f) = spot {
        return u8::from(skip & 1 == 0 && f.intersects_sphere(sphere, true));
    }
    let Some(cube) = cube else { return ALL_FACES & !skip };
    cube.frusta.iter().enumerate().fold(0, |m, (i, f)| if skip & (1 << i) == 0 && f.intersects_sphere(sphere, true) { m | 1 << i } else { m })
}

/// What [`mark_sphere`] found: whether the caster reached any lamp's sphere, whether it
/// marked (or lies in) any face, and on how many lamps it was under the cut-off and marked
/// nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marked {
    pub reached: bool,
    pub marked: bool,
    pub cut: u32,
}

/// Mark the faces of the `candidates` (indices into `lamps`, with `frusta` alongside) a
/// caster's sphere reaches. A lamp that draws in full anyway (it never drew, or every face
/// is marked) is not tested face by face, nor is a face already marked. With `cut` (the
/// small-caster cut-off on), a caster under a lamp's cut-off where it is now and where it
/// last marked (`old`; none for one shown, hidden or new) marks nothing on that lamp: it is
/// not in its maps. With `static_mark` (a static shape's change, [`ShapeChange`]) the faces'
/// static caches are marked too ([`LampState::static_dirty`]).
#[allow(clippy::too_many_arguments)]
fn mark_sphere(
    sphere: &Sphere,
    old: Option<&Sphere>,
    cut: bool,
    static_mark: bool,
    candidates: impl Iterator<Item = usize>,
    lamps: &[Lamp],
    frusta: &[(Option<CubemapFrusta>, Option<Frustum>)],
    states: &mut EntityHashMap<LampState>,
) -> Marked {
    let (mut reached, mut marked, mut cuts) = (false, false, 0);
    for i in candidates {
        let l = &lamps[i];
        if !reaches(l.at, l.range, sphere) {
            continue;
        }
        reached = true;
        let below = |s: &Sphere| cut_off(s.radius, Vec3::from(s.center).distance(l.at), l.map_scale, l.step);
        if cut && below(sphere) && old.is_none_or(below) {
            cuts += 1;
            continue;
        }
        let Some(s) = states.get_mut(&l.entity) else { continue };
        let (cube, spot) = (frusta[i].0.as_ref(), frusta[i].1.as_ref());
        if l.spot {
            // One map: its cone is tested.
            let faces = faces_reached(sphere, cube, spot, 0);
            marked |= faces != 0;
            s.dirty |= faces;
            if static_mark {
                s.static_dirty |= faces;
            }
            continue;
        }
        // A sphere that reaches a point light's lies in one of its faces at least.
        marked = true;
        // Faces already marked as this mark would are not tested again (the static marks
        // within the faces marked: `static_dirty` lies within `dirty`).
        let have = if static_mark { s.static_dirty } else { s.dirty } & ALL_FACES;
        if !s.fresh && have != ALL_FACES {
            let faces = faces_reached(sphere, cube, spot, have);
            s.dirty |= faces;
            if static_mark {
                s.static_dirty |= faces;
            }
        }
    }
    Marked { reached, marked, cut: cuts }
}

/// The camera's distance to a light's edge (game units) where its maps step down in
/// detail: within the first, step 0 (the room around the player, [`NEAR`]); past it step 1;
/// past the second, step 2 [engineering choice: the next room or street, and beyond].
pub const STEPS: [f32; 2] = [NEAR, 2.0 * NEAR];

/// A light moves to a coarser step once it is this fraction beyond the step's distance, and
/// back to a finer one this fraction within it [engineering choice: walking back and forth
/// over a step's distance does not switch the step, nor draw the maps again, each time].
pub const STEP_HYSTERESIS: f32 = 0.1;

/// Per step, the size (across, in texels of the light's map) under which a caster is left
/// out of a light's map ([`cut_off`]) [engineering choice: at step 0 only what covers less
/// than a texel, which the map cannot show anyway; a farther light is seen from farther
/// away, so a texel of its map covers fewer pixels on screen. Unreal's
/// r.Shadow.RadiusThreshold culls small casters by their projected size too (from a
/// research pass, not checked against its code)].
pub const CUTOFF_TEXELS: [f32; 3] = [1.0, 2.0, 4.0];

/// A light's step of detail for `gap` (the camera's distance to its light's edge), starting
/// from `current`, with hysteresis `h` (a fraction of each step's distance; 0: none).
pub fn stepped(current: u8, gap: f32, h: f32) -> u8 {
    let coarser = STEPS.iter().filter(|b| gap > **b * (1.0 + h)).count() as u8;
    let finer = STEPS.iter().filter(|b| gap > **b * (1.0 - h)).count() as u8;
    current.clamp(coarser, finer)
}

/// A map's texels across one unit at one unit's distance: its side over the tangent of half
/// its field of view (1 for a cube face).
pub fn map_scale(side: f32, half_fov_tan: f32) -> f32 {
    side / half_fov_tan.max(1.0e-3)
}

/// Whether a caster's sphere (`radius`, `distance` from the light) covers less than its
/// step's [`CUTOFF_TEXELS`] across in a map of `scale` ([`map_scale`]).
pub fn cut_off(radius: f32, distance: f32, scale: f32, step: u8) -> bool {
    let texels = radius * scale / distance.max(1.0);
    texels < CUTOFF_TEXELS[usize::from(step.min(2))]
}

/// `NVF_LIGHT_SHADOW_TRACE=1` (a measuring aid): every [`TRACE_FRAMES`] frames a line for
/// each actor skeleton with shown skinned shapes: how often its shapes were looked at and
/// left out as out of the lamps' reach, where they and its first joint stand, and how
/// often it moved and marked a lamp.
fn trace_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NVF_LIGHT_SHADOW_TRACE").is_ok_and(|v| v.trim() == "1"))
}

const TRACE_FRAMES: u64 = 300;

#[derive(Default)]
struct TraceRow {
    mesh: Option<Entity>,
    first: Option<Entity>,
    root: Vec3,
    joint: Vec3,
    shapes: u32,
    out_of_reach: u32,
    moved: u32,
    marked: u32,
}

static TRACE: std::sync::LazyLock<std::sync::Mutex<bevy::platform::collections::HashMap<u64, TraceRow>>> = std::sync::LazyLock::new(Default::default);

/// `NVF_LIGHT_SHADOW_CUTOFF=0` (a measuring aid): no small-caster cut-off; every caster
/// Bevy finds is drawn and marks the maps, as before 2026-11-18.
fn cutoff_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let off = std::env::var("NVF_LIGHT_SHADOW_CUTOFF").is_ok_and(|v| v.trim() == "0");
        if off {
            info!("light shadows: NVF_LIGHT_SHADOW_CUTOFF=0, no small-caster cut-off");
        }
        !off
    })
}

/// A cube face counts as in the camera's view while its pyramid comes within this far of
/// the view's frustum (game units) [engineering choice: a face at the edge of the screen is
/// drawn before it shows much, so a turn of the camera rarely finds it stale].
pub const FACE_VIEW_MARGIN: f32 = 64.0;

/// Which faces of a point light's cube at `light` with `range` can show in a camera's view
/// (bit `i` for face `i` in Bevy's cube face order, `CUBE_MAP_FACES`). A face is out when
/// its pyramid (the light's point and the four corners of its square at the range) lies
/// wholly outside one of the view's side or near planes, moved out by `margin`; the far
/// plane is left out, as Bevy's culling of lights does. Conservative: a face it keeps may
/// still be out of view, a face it drops is not in it.
pub fn faces_in_view(view: &Frustum, light: Vec3, range: f32, margin: f32) -> u8 {
    let planes = &view.half_spaces[..=bevy::shape::ViewFrustum::NEAR_PLANE_IDX];
    let mut faces = 0;
    for (i, face) in CUBE_MAP_FACES.iter().enumerate() {
        let t = face.target;
        // Two axes square to the face's.
        let a = Vec3::new(t.y.abs(), t.z.abs(), t.x.abs());
        let b = t.cross(a);
        let points = [light, light + range * (t + a + b), light + range * (t + a - b), light + range * (t - a + b), light + range * (t - a - b)];
        let out = planes.iter().any(|h| points.iter().all(|p| h.normal_d().dot(p.extend(1.0)) + margin <= 0.0));
        if !out {
            faces |= 1 << i;
        }
    }
    faces
}

/// `NVF_LIGHT_SHADOW_FACE_CULL=0` (a measuring aid): every marked face draws on its turn,
/// in view or not, as before 2026-11-18.
fn face_cull_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let off = std::env::var("NVF_LIGHT_SHADOW_FACE_CULL").is_ok_and(|v| v.trim() == "0");
        if off {
            info!("light shadows: NVF_LIGHT_SHADOW_FACE_CULL=0, faces out of view drawn too");
        }
        !off
    })
}

/// The largest scale along a transform's axes (no decomposition into a rotation).
fn max_scale(t: &GlobalTransform) -> f32 {
    let m = t.affine().matrix3;
    m.x_axis.length().max(m.y_axis.length()).max(m.z_axis.length())
}

/// Shapes looked up through a [`LampGrid`] once more than this many moved in a frame (fewer
/// are tested against every lamp) [engineering choice: the grid costs about as much to
/// build as a few dozen shapes tested against 34 lamps].
pub const GRID_AFTER: usize = 32;

/// A caster sphere up to this radius (game units) is looked up in one cell of a
/// [`LampGrid`]; a larger one is tested against every lamp [engineering choice: nearly every
/// shape is smaller, a building is not].
pub const GRID_MARGIN: f32 = 512.0;

/// A broad phase over the casting lights' spheres for marking: a grid whose cells are as
/// large as the longest reach plus [`GRID_MARGIN`], each lamp listed in every cell its sphere
/// grown by the margin overlaps (at most 27). A caster sphere within the margin reaches
/// only lamps listed in the cell its centre lies in.
pub struct LampGrid {
    cell: f32,
    cells: bevy::platform::collections::HashMap<IVec3, Vec<u16>>,
    all: Vec<u16>,
}

impl LampGrid {
    /// The grid of lamps (centre, range), indexed in the order given.
    pub fn new(lamps: &[(Vec3, f32)]) -> Self {
        let cell = lamps.iter().map(|l| l.1).fold(0.0, f32::max) + GRID_MARGIN;
        let mut cells: bevy::platform::collections::HashMap<IVec3, Vec<u16>> = Default::default();
        for (i, (at, range)) in lamps.iter().enumerate() {
            let r = range + GRID_MARGIN;
            let (lo, hi) = (((*at - r) / cell).floor().as_ivec3(), ((*at + r) / cell).floor().as_ivec3());
            for x in lo.x..=hi.x {
                for y in lo.y..=hi.y {
                    for z in lo.z..=hi.z {
                        cells.entry(IVec3::new(x, y, z)).or_default().push(i as u16);
                    }
                }
            }
        }
        Self { cell, cells, all: (0..lamps.len() as u16).collect() }
    }

    /// The lamps a sphere at `centre` with `radius` may reach (a superset).
    pub fn candidates(&self, centre: Vec3, radius: f32) -> &[u16] {
        if radius > GRID_MARGIN {
            return &self.all;
        }
        self.cells.get(&(centre / self.cell).floor().as_ivec3()).map_or(&[], |v| v.as_slice())
    }
}

/// Whether the light's sphere at `light` with `range` and a caster's sphere meet.
fn reaches(light: Vec3, range: f32, sphere: &Sphere) -> bool {
    Vec3::from(sphere.center).distance_squared(light) <= (range + sphere.radius) * (range + sphere.radius)
}

/// How far `light` lies outside the box `aabb` placed by `to_world` (game units, 0 inside),
/// when the box is small enough to be a fixture ([`FIXTURE_RADIUS`]); `None` for a larger one.
pub fn fixture_gap(light: Vec3, to_world: &bevy::math::Affine3A, aabb: &Aabb) -> Option<f32> {
    let m = to_world.matrix3;
    let scale = Vec3::new(m.x_axis.length(), m.y_axis.length(), m.z_axis.length());
    let half = Vec3::from(aabb.half_extents) * scale;
    let radius = half.length();
    if radius > FIXTURE_RADIUS {
        return None;
    }
    let centre = Vec3::from(to_world.transform_point3a(aabb.center));
    // Far from its sphere: no need for the box.
    let far = centre.distance(light) - radius;
    if far > FIXTURE_MARGIN {
        return Some(far);
    }
    let local = Vec3::from(to_world.inverse().transform_point3a(light.into()) - aabb.center);
    let outside = (local.abs() * scale - half).max(Vec3::ZERO);
    Some(outside.length())
}

/// `NVF_LIGHT_SHADOW_FIXTURES=1` (a test aid): log, once for each light when it starts
/// casting, the shapes taken as its fixture and the small ones near it that are not.
fn log_fixtures() -> bool {
    static LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *LOG.get_or_init(|| std::env::var("NVF_LIGHT_SHADOW_FIXTURES").is_ok_and(|v| v.trim() == "1"))
}

/// `NVF_LIGHT_SHADOW_REFRESH=<frames>` (a measuring aid): every light is drawn again in
/// full this often instead of [`Tuning::refresh`], to measure what switching lights on costs.
fn refresh_override() -> Option<u64> {
    static REFRESH: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *REFRESH.get_or_init(|| {
        let v = std::env::var("NVF_LIGHT_SHADOW_REFRESH").ok()?.trim().parse::<u64>().ok().filter(|v| *v > 0)?;
        info!("light shadows: NVF_LIGHT_SHADOW_REFRESH={v}");
        Some(v)
    })
}

/// `NVF_LIGHT_SHADOW_DRAWS=<lights>` (a measuring aid): this many lights may draw in a
/// frame instead of [`Tuning::draws`], to measure what a light switched on costs.
fn draws_override() -> Option<usize> {
    static DRAWS: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    *DRAWS.get_or_init(|| {
        let v = std::env::var("NVF_LIGHT_SHADOW_DRAWS").ok()?.trim().parse::<usize>().ok()?;
        info!("light shadows: NVF_LIGHT_SHADOW_DRAWS={v}");
        Some(v)
    })
}

/// The near plane in use: [`NEAR_Z`], or `NVF_LIGHT_SHADOW_NEAR` (game units) for a run.
fn near_z() -> f32 {
    static NEAR_PLANE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *NEAR_PLANE.get_or_init(|| match std::env::var("NVF_LIGHT_SHADOW_NEAR").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| *v > 0.0) {
        Some(v) => {
            info!("light shadows: NVF_LIGHT_SHADOW_NEAR={v}");
            v
        }
        None => NEAR_Z,
    })
}

/// The shadow biases of the casting lights ([`DEPTH_BIAS`], [`NORMAL_BIAS`]), or
/// `NVF_LIGHT_SHADOW_BIAS=<depth>,<normal>` (a test aid) for comparing.
fn biases() -> (f32, f32) {
    static BIAS: std::sync::OnceLock<(f32, f32)> = std::sync::OnceLock::new();
    *BIAS.get_or_init(|| {
        let aid = std::env::var("NVF_LIGHT_SHADOW_BIAS").ok().and_then(|v| {
            let (d, n) = v.split_once(',')?;
            Some((d.trim().parse().ok()?, n.trim().parse().ok()?))
        });
        if let Some((d, n)) = aid {
            info!("light shadows: NVF_LIGHT_SHADOW_BIAS depth {d} normal {n}");
        }
        aid.unwrap_or((DEPTH_BIAS, NORMAL_BIAS))
    })
}

/// `NVF_PLAY_TEST_LIGHT_SHADOW_HOLD=0`: nothing held, every casting light draws in full
/// every frame.
fn holding() -> bool {
    static HOLD: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *HOLD.get_or_init(|| {
        let off = std::env::var("NVF_PLAY_TEST_LIGHT_SHADOW_HOLD").is_ok_and(|v| v.trim() == "0");
        if off {
            info!("light shadows: test aid NVF_PLAY_TEST_LIGHT_SHADOW_HOLD=0, every map drawn every frame");
        }
        !off
    })
}

/// The layers a casting light draws its shadow casters from (the sun's, sky.rs).
fn casting_layers() -> RenderLayers {
    RenderLayers::from_layers(&[0, crate::sky::SHADOW_LAYER, crate::sky::BODY_LAYER])
}

/// The sides of the near and the far cube texture (0: no far texture) for a choice
/// ([`CHOICES`]) and the Shadow resolution row's side [engineering choice, 2026-11-24]:
///
/// | Choice | Near | Far |
/// |---|---|---|
/// | Performance | 512 (1024 with Shadow resolution 4096) | none: one texture |
/// | Quality | 1024 | 512 |
///
/// With a lamp close over a caster a 512 map's texels showed as blocks along its shadow's
/// edge; such a lamp is a near one. A far lamp's shadow covers few pixels. A cube costs its
/// side squared: Quality with every lamp at 1024 took 1.4 GB for 28 lamps with their
/// static caches ([`cube_texture_mb`], logged), the near eight at 1024 and the rest at 512
/// take 0.7 GB.
pub fn cube_sides(choice: usize, directional: usize) -> (usize, usize) {
    match choice {
        2 => (1024, 512),
        _ => (if directional >= 4096 { 1024 } else { 512 }, 0),
    }
}

/// Which point lights are kept in the near cube texture: the `cap` nearest the camera,
/// one already there keeping its place against one [`KEEP`] times nearer. `lights`: each
/// with its distance to the camera and whether it is in the near texture now.
pub fn near_set<T: Copy + Eq + std::hash::Hash>(lights: &[(T, f32, bool)], cap: usize) -> std::collections::HashSet<T> {
    let mut ranked: Vec<(T, f32)> = lights.iter().map(|&(e, d, near)| (e, if near { d / KEEP } else { d })).collect();
    ranked.sort_by(|a, b| a.1.total_cmp(&b.1));
    ranked.into_iter().take(cap).map(|(e, _)| e).collect()
}

/// At most this many lights change cube texture in a frame: each draws all its faces in
/// that frame, every caster in them [engineering choice: a pair swapping places].
pub const RESLOTS_PER_FRAME: usize = 2;

/// Megabytes (10^6 bytes) of a cube texture of `cubes` cubes of `side` texels square, 32-bit
/// depth (`CORE_3D_SHADOW_MAP_FORMAT`, Depth32Float): the static shadow cache costs as much
/// again as the sampled cubes.
pub fn cube_texture_mb(side: usize, cubes: u32) -> f64 {
    (side * side * 4 * 6) as f64 * f64::from(cubes) / 1.0e6
}

type Placed<'a> = (
    Entity,
    &'a GlobalTransform,
    &'a InheritedVisibility,
    &'a ViewVisibility,
    Option<&'a mut PointLight>,
    Option<&'a mut SpotLight>,
    Option<&'a MovingLight>,
    Option<&'a CubemapFrusta>,
    Option<&'a Frustum>,
    Has<CastingLight>,
    Option<&'a ShadowSource>,
);

/// The lights that may cast: the placed ones and the [`ShadowSource`]s.
type Casters = Or<(With<SessionLight>, With<ShadowSource>)>;

/// Shapes whose place or visibility changed (skinned ones go by their skeleton).
type MovedFilter = (With<Mesh3d>, Without<NotShadowCaster>, Without<SkinnedMesh>, Or<(Changed<GlobalTransform>, Changed<InheritedVisibility>)>);

type Skinned<'a> = (Entity, &'a SkinnedMesh, &'a GlobalTransform, &'a InheritedVisibility, Option<&'a RenderLayers>);

type MovedCasters<'a> = (
    Entity,
    &'a GlobalTransform,
    &'a Aabb,
    Ref<'a, InheritedVisibility>,
    Option<&'a RenderLayers>,
    Option<&'a Name>,
    Option<&'a ChildOf>,
    Has<ShadowDynamic>,
);

/// Every shadow-casting shape but the skinned ones: looked at once when the row is switched
/// on ([`LampShadowState::seeded`]).
type EveryCaster<'a> = (Entity, &'a GlobalTransform, &'a Aabb, Option<&'a RenderLayers>, Has<ShadowDynamic>);
type EveryCasterFilter = (With<Mesh3d>, Without<NotShadowCaster>, Without<SkinnedMesh>);

/// Shapes new this frame, or whose layers changed, not yet dynamic ([`classify_casters`]).
type NewShapes<'a> = (Entity, Has<SkinnedMesh>, Option<&'a RenderLayers>);
type NewShapesFilter = (With<Mesh3d>, Without<ShadowDynamic>, Or<(Added<Mesh3d>, Changed<RenderLayers>)>);

/// An actor's or creature's rig root, and the player's ragdoll rig.
type Rigs = Or<(With<crate::limbs::ActorRig>, With<crate::ragdoll::PlayerRig>)>;
type NewRigs = Or<(Added<crate::limbs::ActorRig>, Added<crate::ragdoll::PlayerRig>)>;

/// The layers only the player's body is on (sky.rs): its shapes are dynamic.
fn body_layers() -> RenderLayers {
    RenderLayers::from_layers(&[crate::sky::SHADOW_LAYER, crate::sky::BODY_LAYER])
}

/// Mark the shapes that move as [`ShadowDynamic`] from the start: skinned shapes, the
/// player's body's (its layers), and every shape under an actor's or creature's rig (its
/// weapons, hat, eyes: they follow the skeleton), found from a new shape up its ancestors
/// and from a new rig down its descendants. Any other shape becomes dynamic in
/// [`shadow_lights`] once it moves ([`shape_change`]). It runs only while the row is on
/// ([`live`]), so when the row is switched on every shape and rig not yet dynamic is looked
/// at once (the shapes spawned while it was off).
#[allow(clippy::too_many_arguments)]
fn classify_casters(
    mut commands: Commands,
    settings: Res<LightShadows>,
    new: Query<NewShapes, NewShapesFilter>,
    every: Query<NewShapes, (With<Mesh3d>, Without<ShadowDynamic>)>,
    rigs: Query<Entity, NewRigs>,
    rigged: Query<(), Rigs>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    shapes: Query<(), (With<Mesh3d>, Without<ShadowDynamic>)>,
) {
    if settings.choice == 0 {
        return;
    }
    // Switched on (or the game started with it on): every shape, not only the new ones.
    let all = settings.is_changed();
    let body = body_layers();
    let mut marked = EntityHashSet::default();
    let mut look = |(e, skinned, layers): (Entity, bool, Option<&RenderLayers>)| {
        if skinned || layers.is_some_and(|l| l.intersects(&body)) || parents.iter_ancestors(e).any(|a| rigged.contains(a)) {
            marked.insert(e);
        }
    };
    if all {
        every.iter().for_each(&mut look);
    } else {
        new.iter().for_each(&mut look);
    }
    // A new rig's shapes (the whole pass above already found every shape under a rig).
    if !all {
        for rig in &rigs {
            marked.extend(children.iter_descendants(rig).filter(|d| shapes.contains(*d)));
        }
    }
    for e in marked {
        commands.entity(e).try_insert(ShadowDynamic);
    }
}

/// A light this frame: what [`shadow_lights`] works out for it.
struct Lamp {
    entity: Entity,
    at: Vec3,
    range: f32,
    spot: bool,
    seen: bool,
    gap: f32,
    transient: bool,
    /// Its step of detail and map scale ([`LampState::step`]).
    step: u8,
    map_scale: f32,
}

/// What [`shadow_lights`] hands on: the renderer's hold, the loading screen's wait, the
/// lights shown to Bevy's culling and the idle frames ([`live`]); and the textures' inputs.
type Outputs<'w> = (
    ResMut<'w, HoldLightShadowMaps>,
    Res<'w, MapTextureInputs>,
    Option<ResMut<'w, nvf_play::loading::Loading>>,
    ResMut<'w, NvfShadowLightsShown>,
    ResMut<'w, LightShadowsIdle>,
);

/// The weight of the lamps' shadows (`HoldLightShadowMaps::fade`, with each lamp's fade):
/// the renderer takes `1 - weight x (1 - shadow)` of a lamp's light (bevy_pbr shadows.wesl
/// `nvf_weigh_shadow`). At 1 what blocks a lamp takes all of its light. Past 1 a place its
/// map shades in full gets less than none of it, and the display sum
/// (lights/display_shader.rs, held to 0) takes the rest off the other light there, the
/// cell's ambient light and the other lamps: by a tenth of what the lamp would have given
/// at 1.1. The lamps' highlights never go under none.
///
/// 1.1 [design, set by eye in Primm's casino with the display sum's dark values shown by
/// the sRGB curve (a change of its own, lights/display_shader.rs): against a
/// floor no longer crushed, a shadow that took its lamp's light alone read as too light].
/// For a day (2026-11-25) this was a row of the settings page.
pub const SHADOW_STRENGTH: f32 = 1.1;

/// What the casting lights' log line was last written for: the choice, the point and spot
/// lights casting, and the rooms of the near and far cube textures.
type Logged = (usize, usize, usize, u32, u32);

/// Choose the lights that cast, mark what changed in their maps, choose which draw this
/// frame, and set them up for Bevy and the patched renderer.
#[allow(clippy::too_many_arguments)]
fn shadow_lights(
    mut commands: Commands,
    time: Res<Time<Real>>,
    settings: Res<LightShadows>,
    sky: Option<Res<crate::sky::SkyState>>,
    directional: Option<Res<DirectionalLightShadowMap>>,
    mut cube: ResMut<PointLightShadowMap>,
    mut state: ResMut<LampShadowState>,
    (mut out, inputs, mut loading, mut shown, mut idle): Outputs,
    camera: Query<(&GlobalTransform, Option<&Frustum>), crate::fly::WorldCamera>,
    mut lights: Query<Placed, Casters>,
    moved: Query<MovedCasters, MovedFilter>,
    skinned: Query<Skinned, Without<NotShadowCaster>>,
    names: Query<&Name>,
    joints: Query<Ref<GlobalTransform>>,
    (mut removed, every): (RemovedComponents<Mesh3d>, Query<EveryCaster, EveryCasterFilter>),
    mut logged: Local<Option<Logged>>,
) {
    let _timed = Timed::start(0);
    let tune = tuning(settings.choice).map(|t| Tuning { refresh: refresh_override().unwrap_or(t.refresh), draws: draws_override().unwrap_or(t.draws), ..t });
    let reach = sky.as_ref().map_or(0.0, |s| s.shadow_distance * s.shadow_reach.max(1.0));
    let (at, view) = camera.iter().next().map_or((None, None), |(c, f)| (Some(c.translation()), f));
    let state = &mut *state;
    state.frame += 1;
    let frame = state.frame;
    let hold_all = !holding();
    // The static shadow cache: point lamps' faces drawn from the static casters into a cache
    // and the dynamic casters drawn over a copy of it.
    let cache_on = static_cache_on() && tune.is_some();
    // Under a loading screen, once the renderer has its cells: every seen lamp draws its
    // marked faces at once, in or out of view, with no budget, so the shadows are whole when
    // the screen comes down.
    let screen_up = loading.as_ref().is_some_and(|l| l.active());
    let predraw = predrawing(tune.is_some(), predraw_on(), screen_up, loading.as_ref().is_some_and(|l| l.render_busy));
    if predraw && state.predraw.began.is_none() {
        state.predraw.began = Some(time.elapsed_secs());
    }

    // 1. Which lights cast: the placed ones (with grass fires and attached lights) up to
    // their cap; apart from them, the transients the camera sees, up to theirs within a
    // shorter reach.
    // Sets: every light in the world is looked up in them a few times a frame.
    let (chosen, transients): (EntityHashSet, EntityHashSet) = match (tune, at) {
        (Some(t), Some(at)) if reach > 0.0 => {
            let mut placed: Vec<(Entity, (f32, f32))> = Vec::new();
            let mut quick: Vec<(Entity, (f32, f32))> = Vec::new();
            for (e, place, vis, _, point, spot, moving, _, _, casting, source) in &lights {
                if !vis.get() {
                    continue;
                }
                let (range, intensity) = match (&point, &spot) {
                    (Some(p), _) => (p.range, p.intensity),
                    (None, Some(s)) => (s.range, s.intensity),
                    _ => continue,
                };
                if range <= 0.0 || moving.map_or(intensity, |m| m.full) <= 0.0 {
                    continue;
                }
                let r = rank(place.translation().distance(at), range, casting);
                if source.is_some_and(|s| s.0.transient()) {
                    if in_view(view, place.translation(), range) {
                        quick.push((e, r));
                    }
                } else {
                    placed.push((e, r));
                }
            }
            let transients = pick(&mut quick, transient_reach(reach), t.transients);
            let mut chosen = pick(&mut placed, reach, t.lights);
            chosen.extend_from_slice(&transients);
            (chosen.into_iter().collect(), transients.into_iter().collect())
        }
        _ => (EntityHashSet::default(), EntityHashSet::default()),
    };
    let (near_side, far_side) = cube_sides(settings.choice, directional.as_ref().map_or(2048, |d| d.size));
    // Another choice: other sides, so both textures are made anew, with room for what casts
    // now and no more (a room only grows while a choice stays).
    if state.choice_was != settings.choice {
        (state.choice_was, state.capacity, state.far_capacity) = (settings.choice, 0, 0);
    }
    let far_side = if tune.is_some_and(|t| t.near > 0) { far_side } else { 0 };
    if tune.is_some() {
        let side = near_side;
        state.cube_before.get_or_insert(cube.size);
        if cube.size != side {
            cube.size = side;
        }
    } else if let Some(side) = state.cube_before.take() {
        // Off: Bevy's cube side as it was.
        if cube.size != side {
            cube.size = side;
        }
    }
    // Their states and slots. A light no longer chosen is held, its slot kept and nothing
    // drawn, while its shadow fades out; then it loses them, as does a light despawned.
    let mut lamps: Vec<Lamp> = Vec::with_capacity(chosen.len());
    let mut alive = EntityHashSet::default();
    for (e, t, _, seen, point, spot, ..) in &lights {
        if point.is_none() && spot.is_none() {
            continue;
        }
        if !chosen.contains(&e) {
            if state.lamps.contains_key(&e) {
                alive.insert(e);
            }
            continue;
        }
        let is_point = point.is_some();
        // A spot light's map spans its cone at the sun's side; a cube face 90 degrees at the
        // cube's side.
        let scale = match &spot {
            Some(s) if !is_point => map_scale(directional.as_ref().map_or(2048, |d| d.size) as f32, s.outer_angle.tan()),
            _ => map_scale(cube.size as f32, 1.0),
        };
        let range = point.map(|p| p.range).or(spot.map(|s| s.range)).unwrap_or(0.0);
        let transient = transients.contains(&e);
        if transient && !state.lamps.contains_key(&e) {
            state.transients_cast += 1;
        }
        let gap = at.map_or(0.0, |a| (t.translation().distance(a) - range).max(0.0));
        let entry = state.lamps.entry(e).or_insert(LampState {
            dirty: 0,
            fresh: true,
            refreshed: 0,
            phase: e.index_u32() as u64,
            at: t.translation(),
            transient,
            spot: !is_point,
            fade: 0.0,
            leaving: false,
            step: stepped(0, gap, 0.0),
            map_scale: scale,
            cut_any: false,
            far_fade: 1.0,
            deferred: 0,
            static_dirty: ALL_FACES,
            pending: 0,
            plain: plain_path(cache_on, hold_all, transient, !is_point, false),
            to_static: 0,
            composite: 0,
            quiet: 0,
            dynamic_faces: 0,
            waiting: 0,
            far: false,
            reslot: false,
            unready: 0,
            unready_frame: 0,
            settle_at: 0,
            settle_step: 0,
            build_waits: 0,
            drew: 0,
            drew_fresh: false,
        });
        // Its shadow fades out over the last stretch of the reach (not a transient's: it is
        // chosen within a shorter reach and lasts a moment).
        entry.far_fade = if transient || !distance_fade_on() { 1.0 } else { distance_fade(gap, reach) };
        if entry.far_fade < 1.0 {
            state.pass.distance_faded += 1;
        }
        if entry.fresh {
            // Its full redraws fall at a time of its own.
            entry.refreshed = frame.saturating_sub(entry.phase % tune.map_or(1, |t| t.refresh).max(1));
        }
        if entry.leaving {
            // Chosen again while fading out: what changed meanwhile was not marked.
            entry.leaving = false;
            entry.dirty = ALL_FACES;
            entry.static_dirty = ALL_FACES;
        }
        // Its step of detail, with hysteresis; a step finer draws its maps again in full when
        // a caster was cut from them, so it comes back.
        let step = stepped(entry.step, gap, STEP_HYSTERESIS);
        if step < entry.step && entry.cut_any && cutoff_on() {
            entry.dirty = ALL_FACES;
            entry.static_dirty = ALL_FACES;
            entry.cut_any = false;
            state.pass.step_redraws += 1;
        }
        (entry.step, entry.map_scale) = (step, scale);
        state.pass.steps[usize::from(step.min(2))] += 1;
        if !is_point {
            assign_slot(&mut state.spot_slots, e);
        }
        // A transient was chosen only if the camera sees it this frame (a new light's
        // `ViewVisibility` is still false).
        lamps.push(Lamp { entity: e, at: t.translation(), range, spot: !is_point, seen: transient || seen.get(), gap, transient, step, map_scale: scale });
    }
    // The point lights' cube texture: the nearest `near` in the near one, the rest in the
    // far one ([`near_set`]); transients in the near one, apart from the cap. A light that
    // has to change does so a few a frame and draws at once ([`LampState::reslot`]); one
    // the camera does not see is shown to Bevy's culling for that frame (`reslot_unseen`,
    // as the pre-draw's lamps below). Until 2026-11-25 such a lamp kept its place until it
    // came into sight: the near texture stayed full of lamps behind the camera, and those
    // ahead of it stayed in the far one.
    let mut reslot_unseen: Vec<Entity> = Vec::new();
    {
        let near_cap = tune.map_or(0, |t| t.near);
        let tiered = near_cap > 0 && far_side > 0;
        let ranked: Vec<(Entity, f32, bool)> = lamps
            .iter()
            .filter(|l| !l.spot && !l.transient)
            .map(|l| (l.entity, at.map_or(0.0, |a| l.at.distance(a)), state.slots.contains_key(&l.entity)))
            .collect();
        let near = near_set(&ranked, near_cap);
        let mut reslots = 0;
        // Those leaving the near texture first: they make room there.
        let mut order: Vec<&Lamp> = lamps.iter().filter(|l| !l.spot).collect();
        order.sort_by_key(|l| near.contains(&l.entity));
        for l in order {
            let want_far = tiered && !l.transient && !near.contains(&l.entity);
            let Some(s) = state.lamps.get_mut(&l.entity) else { continue };
            let slotted = state.slots.contains_key(&l.entity) || state.far_slots.contains_key(&l.entity);
            if slotted && s.far == want_far {
                continue;
            }
            // The near texture's room for the lamps is the cap's: no slot free, no move.
            let room = want_far || !tiered || state.slots.len() < near_cap + tune.map_or(0, |t| t.transients);
            if slotted && (s.fresh || (reslots < RESLOTS_PER_FRAME && room)) {
                state.slots.remove(&l.entity);
                state.far_slots.remove(&l.entity);
                if !s.fresh {
                    (s.reslot, s.dirty, s.static_dirty, s.pending) = (true, ALL_FACES, ALL_FACES, 0);
                    reslots += 1;
                    state.pass.reslots += 1;
                    if !l.seen {
                        reslot_unseen.push(l.entity);
                    }
                }
            } else if slotted {
                continue;
            }
            s.far = if slotted || room { want_far } else { true };
            assign_slot(if s.far { &mut state.far_slots } else { &mut state.slots }, l.entity);
        }
        // A cube's texels across a unit at a unit's distance follow its texture's side.
        for l in lamps.iter_mut().filter(|l| !l.spot) {
            if let Some(s) = state.lamps.get_mut(&l.entity) {
                let scale = map_scale(if s.far { far_side } else { near_side } as f32, 1.0);
                (l.map_scale, s.map_scale) = (scale, scale);
            }
        }
    }
    // The fades: rising once a light's maps were first drawn (a transient's full at once),
    // falling while it leaves (with a mode on; turned off, every light is dropped at once).
    let dt = time.delta_secs();
    state.lamps.retain(|e, s| {
        if chosen.contains(e) {
            s.fade = if s.transient {
                1.0
            } else if s.fresh {
                0.0
            } else {
                fade_step(s.fade, true, dt)
            };
            return true;
        }
        match leaving_fade(s.fade, !s.fresh, s.transient, tune.is_some() && alive.contains(e), dt) {
            Some(fade) => {
                s.fade = fade;
                s.leaving = true;
                true
            }
            None => false,
        }
    });
    state.slots.retain(|e, _| state.lamps.get(e).is_some_and(|s| !s.spot));
    state.far_slots.retain(|e, _| state.lamps.get(e).is_some_and(|s| !s.spot));
    state.spot_slots.retain(|e, _| state.lamps.get(e).is_some_and(|s| s.spot));
    // The transients' room is kept from the first lamp on: the texture grown for a muzzle
    // flash's slot is a new one, and every lamp in it lost its maps and faded in again at
    // the first shot (2026-11-25).
    state.capacity = match tune {
        Some(t) => grown_room(state.capacity, state.slots.values().copied(), if state.slots.is_empty() { 0 } else { t.transients as u32 }),
        None => 0,
    };
    state.far_capacity = match tune {
        Some(_) if far_side > 0 => grown_room(state.far_capacity, state.far_slots.values().copied(), 0),
        _ => 0,
    };
    // The spot lights' room also holds the other shadowed spot lights (the flashlight), so
    // switching one on or off does not change the texture; none while no lamp spot casts.
    state.spot_capacity = match tune {
        Some(_) if !state.spot_slots.is_empty() || state.spot_capacity > 0 => grown_room(state.spot_capacity, state.spot_slots.values().copied(), inputs.other_spots),
        _ => 0,
    };
    // A texture made anew holds no light's maps: its lights draw again as if new, and those
    // leaving are dropped.
    // The static cache gives the cube texture another use (copied into): another texture.
    let point_key = (cube.size, state.capacity, cache_on);
    let spot_key = (directional.as_ref().map_or(2048, |d| d.size), state.spot_capacity, inputs.cascades);
    let far_key = (far_side, state.far_capacity, cache_on);
    let new_point = state.point_maps_key.is_some_and(|k| k != point_key);
    let new_spot = state.spot_maps_key.is_some_and(|k| k != spot_key);
    // The far texture's first making (no room before) holds nothing to lose.
    let new_far = state.far_maps_key.is_some_and(|k| k != far_key && k.1 > 0);
    // A texture that only grew (more room, all else the same).
    let grew_point = state.point_maps_key.is_some_and(|k| regrown(k.1, point_key.1, (k.0, k.2) == (point_key.0, point_key.2)));
    let grew_spot = state.spot_maps_key.is_some_and(|k| regrown(k.1, spot_key.1, (k.0, k.2) == (spot_key.0, spot_key.2)));
    let grew_far = state.far_maps_key.is_some_and(|k| regrown(k.1, far_key.1, (k.0, k.2) == (far_key.0, far_key.2)));
    (state.point_maps_key, state.spot_maps_key, state.far_maps_key) = match tune {
        Some(_) => (Some(point_key), Some(spot_key), Some(far_key)),
        None => (None, None, None),
    };
    if new_point || new_spot || new_far {
        let in_sight: EntityHashSet = lamps.iter().filter(|l| l.seen).map(|l| l.entity).collect();
        let mut redrawn = 0;
        state.lamps.retain(|e, s| {
            let (made_anew, grew) = if s.spot {
                (new_spot, grew_spot)
            } else if s.far {
                (new_far, grew_far)
            } else {
                (new_point, grew_point)
            };
            if !made_anew {
                return true;
            }
            if s.leaving {
                return false;
            }
            // A texture that only grew: a lamp in sight draws every face again this frame,
            // whatever the frame's draws, and its shadow stays as it was (as a lamp that
            // changed texture, [`LampState::reslot`]). Until 2026-11-25 every lamp of the
            // texture lost its shadow and faded in again whenever a lamp more needed room.
            if keeps_shadow(grew, s.fresh, s.transient, in_sight.contains(e)) {
                (s.reslot, s.dirty, s.static_dirty, s.pending) = (true, ALL_FACES, ALL_FACES, 0);
                redrawn += 1;
                return true;
            }
            (s.fresh, s.reslot, s.dirty, s.static_dirty, s.pending) = (true, false, ALL_FACES, ALL_FACES, 0);
            if !s.transient {
                s.fade = 0.0;
            }
            true
        });
        state.pass.reslots += redrawn;
        state.slots.retain(|e, _| state.lamps.contains_key(e));
        state.far_slots.retain(|e, _| state.lamps.contains_key(e));
        state.spot_slots.retain(|e, _| state.lamps.contains_key(e));
    }

    // 2. What changed in their maps since last frame. Per shape the lamps are found through
    // a grid once more than a few shapes moved ([`LampGrid`]), and a lamp that draws in full
    // anyway (fresh or every face marked) and faces already marked are not tested again: a
    // reference toggled or faded in town changes thousands of shapes in a frame.
    let layers = casting_layers();
    if tune.is_none() {
        // Off: nothing is followed; switched on again, every caster is looked at anew.
        if state.seeded {
            state.shapes_at.clear();
            state.skeletons_at.clear();
            state.seeded = false;
        }
    } else if !state.seeded {
        // Switched on (or the game started with it on): where every caster stands now, so
        // one that goes or first moves later marks where it was.
        state.seeded = true;
        for (e, t, aabb, rl, dynamic) in &every {
            if rl.is_some_and(|l| !l.intersects(&layers)) {
                continue;
            }
            state.shapes_at.entry(e).or_insert_with(|| ShapeAt::of(t, aabb, dynamic));
        }
    }
    let frusta: Vec<(Option<CubemapFrusta>, Option<Frustum>)> = lamps
        .iter()
        .map(|l| match lights.get(l.entity) {
            Ok((_, _, _, _, _, spot, _, cube, frustum, ..)) => (cube.cloned(), if spot.is_some() { frustum.copied() } else { None }),
            Err(_) => (None, None),
        })
        .collect();
    let cut = cutoff_on();
    if tune.is_some() {
        let spheres: Vec<(Vec3, f32)> = lamps.iter().map(|l| (l.at, l.range)).collect();
        let mut grid: Option<LampGrid> = None;
        let mut shapes_seen = 0usize;
        for (e, t, aabb, vis, rl, name, parent, dynamic) in &moved {
            if rl.is_some_and(|l| !l.intersects(&layers)) {
                continue;
            }
            // Shown, hidden or new at once; moved once its bounds' centre or corner (a turn
            // in place) went MOVE_EPS from its last mark.
            let last = state.shapes_at.get(&e).copied();
            // Dynamic once it moved ([`ShapeAt::dynamic`]).
            let dynamic = dynamic || last.is_some_and(|l| l.dynamic);
            let now = ShapeAt::of(t, aabb, dynamic);
            if !vis.is_changed() && !last.is_none_or(|l| l.moved(&now)) {
                continue;
            }
            // A hidden shape that moves is in no map: only where it is is kept.
            if !vis.get() && !vis.is_changed() {
                state.shapes_at.insert(e, now);
                continue;
            }
            let sphere = now.sphere();
            // Where it last marked (a shape shown, hidden or new: none).
            let old = (!vis.is_changed()).then(|| last.map(|l| l.sphere())).flatten();
            // With the static cache: a static shape shown, hidden or new marks the caches
            // where it is; one that moved becomes dynamic and marks the caches where it was
            // and is; a dynamic one marks only the composites, where it was and is.
            let change = shape_change(dynamic, vis.is_changed(), last.is_some());
            let static_mark = cache_on && change != ShapeChange::Dynamic;
            let promoted = change == ShapeChange::Promoted;
            if promoted {
                commands.entity(e).try_insert(ShadowDynamic);
                state.pass.promoted += 1;
            }
            state.shapes_at.insert(e, ShapeAt { dynamic: dynamic || promoted, ..now });
            if lamps.is_empty() {
                continue;
            }
            // Where it was and where it is: its old shadow goes as its new one comes.
            let sphere = match &old {
                Some(o) => merged_sphere(&sphere, o),
                None => sphere,
            };
            shapes_seen += 1;
            if shapes_seen > GRID_AFTER && grid.is_none() {
                grid = Some(LampGrid::new(&spheres));
            }
            let at = Vec3::from(sphere.center);
            let m = match &grid {
                Some(g) => mark_sphere(&sphere, old.as_ref(), cut, static_mark, g.candidates(at, sphere.radius).iter().map(|&i| i as usize), &lamps, &frusta, &mut state.lamps),
                None => mark_sphere(&sphere, old.as_ref(), cut, static_mark, 0..lamps.len(), &lamps, &frusta, &mut state.lamps),
            };
            state.pass.cut_marks += u64::from(m.cut);
            if m.marked {
                state.causes[0] += 1;
                if state.names.len() < 6 {
                    let parent = parent.and_then(|c| names.get(c.parent()).ok());
                    let n = format!("{}/{}", parent.map_or("-", |n| n.as_str()), name.map_or("-", |n| n.as_str()));
                    if !state.names.contains(&n) {
                        state.names.push(n);
                    }
                }
            }
        }
        // An actor's skinned shapes share its skeleton: each skeleton (a list of joints,
        // [`skeleton_key`]) is gone over and marks once a frame, not once for each of its
        // shapes. It marks when any of its joints went MOVE_EPS from where the skeleton last
        // marked, or when one of its shapes was shown or hidden (a limb shot off).
        let mut shown: bevy::platform::collections::HashMap<u64, u32> = default();
        for (_, skin, _, seen, rl) in &skinned {
            if seen.get() && !rl.is_some_and(|l| !l.intersects(&layers)) && !skin.joints.is_empty() {
                *shown.entry(skeleton_key(&skin.joints)).or_default() += 1;
            }
        }
        let mut skeletons_seen: bevy::platform::collections::HashSet<u64> = default();
        // The joints' box, grown by a limb's thickness [engineering choice].
        let box_sphere = |lo: Vec3, hi: Vec3| Sphere { center: ((lo + hi) * 0.5).into(), radius: (hi - lo).length() * 0.5 + 16.0 };
        let mut now: Vec<Vec3> = Vec::new();
        for (mesh, skin, t, seen, rl) in &skinned {
            if !seen.get() || rl.is_some_and(|l| !l.intersects(&layers)) {
                continue;
            }
            let root = t.translation();
            let in_reach = lamps.iter().any(|l| l.at.distance_squared(root) <= (l.range + 512.0) * (l.range + 512.0));
            let Some(&first) = skin.joints.first() else { continue };
            let e = skeleton_key(&skin.joints);
            if trace_on() {
                let joint = joints.get(first).map_or(Vec3::NAN, |g| g.translation());
                let mut trace = TRACE.lock().unwrap_or_else(|e| e.into_inner());
                let row = trace.entry(e).or_insert_with(|| TraceRow { mesh: Some(mesh), first: Some(first), ..default() });
                (row.root, row.joint) = (root, joint);
                row.shapes += 1;
                row.out_of_reach += u32::from(!in_reach);
            }
            if !in_reach {
                continue;
            }
            if !skeletons_seen.insert(e) {
                continue;
            }
            let shapes = shown.get(&e).copied().unwrap_or(0);
            let last = state.skeletons_at.get(&e);
            let recounted = last.is_none_or(|l| l.shapes != shapes);
            now.clear();
            let (mut changed, mut lo, mut hi) = (false, Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for j in skin.joints.iter().filter_map(|&j| joints.get(j).ok()) {
                changed |= j.is_changed();
                let at = j.translation();
                (lo, hi) = (lo.min(at), hi.max(at));
                now.push(at);
            }
            if now.is_empty() || !(recounted || changed && last.is_none_or(|l| joints_moved(&l.joints, &now))) {
                continue;
            }
            let old = last.map(|l| box_sphere(l.lo, l.hi));
            // A skeleton is dynamic: it marks the composites of the faces it was and is in.
            let here = box_sphere(lo, hi);
            let sphere = match &old {
                Some(o) => merged_sphere(&here, o),
                None => here,
            };
            let m = mark_sphere(&sphere, old.as_ref(), cut, false, 0..lamps.len(), &lamps, &frusta, &mut state.lamps);
            state.pass.cut_marks += u64::from(m.cut);
            if m.marked {
                state.causes[1] += 1;
            }
            if trace_on() {
                if let Some(row) = TRACE.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&e) {
                    row.moved += 1;
                    row.marked += u32::from(m.marked);
                }
            }
            match state.skeletons_at.get_mut(&e) {
                Some(l) => {
                    l.joints.clone_from(&now);
                    (l.lo, l.hi, l.shapes) = (lo, hi, shapes);
                }
                None => {
                    state.skeletons_at.insert(e, SkeletonAt { joints: now.clone(), lo, hi, shapes });
                }
            }
        }
        if trace_on() && frame.is_multiple_of(TRACE_FRAMES) {
            let mut trace = TRACE.lock().unwrap_or_else(|e| e.into_inner());
            // How many first joints head more than one list of joints: the lists the old
            // key, the first joint alone, left unwatched ([`skeleton_key`]).
            let mut by_first: EntityHashMap<u32> = EntityHashMap::default();
            for row in trace.values() {
                if let Some(first) = row.first {
                    *by_first.entry(first).or_default() += 1;
                }
            }
            let shared = by_first.values().filter(|n| **n > 1).count();
            let unwatched: u32 = by_first.values().map(|n| n - 1).sum();
            info!(
                "light shadows: trace: {} lists of joints under {} first joints; {shared} first joints head more than one list, {unwatched} lists that a key of the first joint alone would leave unwatched",
                trace.len(),
                by_first.len()
            );
            for (_, row) in trace.drain() {
                let name = row.mesh.and_then(|m| names.get(m).ok()).map_or("-", |n| n.as_str());
                info!(
                    "light shadows: trace: skeleton of {name}: over {TRACE_FRAMES} frames its shapes were looked at {} times, {} out of every lamp's reach by their own place ({:.0}, {:.0}, {:.0}; first joint at {:.0}, {:.0}, {:.0}); it moved {} times and marked {}",
                    row.shapes, row.out_of_reach, row.root.x, row.root.y, row.root.z, row.joint.x, row.joint.y, row.joint.z, row.moved, row.marked
                );
            }
        }
        // A skeleton no longer there (its actor gone, hidden, or out of every lamp's
        // reach) leaves the faces it was in.
        let mut left: Vec<Sphere> = Vec::new();
        state.skeletons_at.retain(|e, l| {
            let stays = skeletons_seen.contains(e);
            if !stays {
                left.push(box_sphere(l.lo, l.hi));
            }
            stays
        });
        for sphere in &left {
            if mark_sphere(sphere, None, false, false, 0..lamps.len(), &lamps, &frusta, &mut state.lamps).marked {
                state.causes[1] += 1;
            }
        }
    }
    // A shape removed (an item taken, a thing killed or broken, a cell unloaded) leaves the
    // faces it was in: a static one their caches too. Until 2026-11-24 every lamp around the
    // camera drew again in full for any mesh despawned anywhere.
    for e in removed.read() {
        let Some(was) = state.shapes_at.remove(&e) else { continue };
        if mark_sphere(&was.sphere(), None, false, cache_on && !was.dynamic, 0..lamps.len(), &lamps, &frusta, &mut state.lamps).marked {
            state.causes[2] += 1;
        }
    }
    // Lights whose maps were drawn without all their casters (the renderer's word, a frame
    // or two after the draw) draw again in full, a little later each time.
    // The renderer reports each face's view on its own: a light's are summed, or two faces
    // with different counts read as a new count on every report and started the steps over
    // each time (the light then drew in full every few frames while anything was missing).
    let mut reports: EntityHashMap<(u32, bool)> = EntityHashMap::default();
    for (e, casters, building) in bevy::pbr::nvf_take_unready_shadow_lights() {
        state.pass.unready[usize::from(casters == 0 && building)] += 1;
        let report = reports.entry(e).or_default();
        (report.0, report.1) = (report.0 + casters, report.1 || building);
    }
    for (e, (casters, building)) in reports {
        let Some(s) = state.lamps.get_mut(&e).filter(|s| !s.transient) else { continue };
        // Reports after a long quiet are a new matter (casters that came later): the steps
        // and the build waits start over, or a light that had spent them drew no more.
        if frame.saturating_sub(s.unready_frame) > UNREADY_QUIET {
            (s.unready, s.settle_step, s.build_waits) = (0, 0, 0);
        }
        s.unready_frame = frame;
        if casters == 0 {
            // None known missing, but pipelines are being built.
            if building && s.settle_at == 0 && s.build_waits < BUILD_WAITS {
                // Under a loading screen sooner (the screen waits), but not every frame:
                // each is a full redraw of the light, and the pipelines take seconds.
                (s.settle_at, s.build_waits) = (frame + if predraw { BUILD_WAIT / 4 } else { BUILD_WAIT }, s.build_waits + 1);
            }
            continue;
        }
        // More missing than at any report before: new casters, the steps start over. Fewer
        // is the faces drawn that frame, or casters now ready: the steps go on.
        if casters > s.unready {
            (s.unready, s.settle_step) = (casters, 0);
        }
        if s.settle_at == 0 {
            if let Some((at, step)) = settle_after(frame, s.settle_step, predraw) {
                (s.settle_at, s.settle_step) = (at, step);
            }
        }
    }
    for s in state.lamps.values_mut() {
        if s.settle_at != 0 && frame >= s.settle_at {
            (s.settle_at, s.dirty, s.static_dirty) = (0, ALL_FACES, ALL_FACES);
            state.pass.settled += 1;
        }
    }
    // The light moved past LIGHT_MOVE_EPS, or its turn for a full redraw came.
    for l in &lamps {
        if let (Some(s), Some(tune)) = (state.lamps.get_mut(&l.entity), tune) {
            let moved = light_moved(s.at, l.at);
            if !l.transient && (moved || frame.saturating_sub(s.refreshed) >= tune.refresh) {
                s.dirty = ALL_FACES;
                s.static_dirty = ALL_FACES;
                s.refreshed = frame;
                s.at = l.at;
                state.causes[3] += 1;
                // A light that moves would draw its cache again each time: the plain path.
                s.plain |= moved;
            }
        }
    }

    // Under a loading screen, the lamps the camera does not see draw too, once those it sees
    // are done: shown to Bevy's culling (vendored bevy_light `NvfShadowLightsShown`), which
    // finds their casters and marks them visible, so the renderer draws their maps; the
    // nearest few a frame. Their faces out of view go into the static cache as any.
    let to_draw = |s: &LampShadowState, e: &Entity| s.lamps.get(e).is_some_and(|s| s.fresh || s.dirty != 0);
    let shown_now = if predraw {
        // Only a lamp in view never drawn keeps the others waiting (it draws this frame,
        // with no budget): one whose faces are marked again every frame (a fan, a flag in
        // its light) kept them waiting for the whole of the pre-draw.
        let seen_waiting = lamps.iter().any(|l| !l.transient && l.seen && state.lamps.get(&l.entity).is_some_and(|s| s.fresh));
        let mut unseen: Vec<(Entity, f32)> = lamps.iter().filter(|l| !l.transient && !l.seen && to_draw(state, &l.entity)).map(|l| (l.entity, l.gap)).collect();
        shown_unseen(&mut unseen, seen_waiting, PREDRAW_UNSEEN_PER_FRAME)
    } else {
        Vec::new()
    };
    // And the lamps out of sight that changed cube texture this frame.
    let mut shown_now = shown_now;
    for e in reslot_unseen {
        if !shown_now.contains(&e) {
            shown_now.push(e);
        }
    }
    for l in lamps.iter_mut().filter(|l| shown_now.contains(&l.entity)) {
        l.seen = true;
    }
    if !shown.lights.is_empty() || !shown_now.is_empty() {
        shown.lights.clear();
        shown.lights.extend(shown_now.iter().copied());
    }

    // 3. Which lights draw this frame: those never drawn first, then the nearest marked ones
    // on their turn; at most `draws` lights (point or spot), and apart from them
    // `transient_draws` transient ones.
    lamps.sort_by(|a, b| b.seen.cmp(&a.seen).then(a.gap.total_cmp(&b.gap)));
    // The faces of each point light that can show in the camera's view (all for a spot
    // light, one never drawn, a transient, or with face culling off): a marked face out of
    // view stays marked, held, until it comes into view.
    let cull = face_cull_on() && !hold_all && !predraw;
    let in_view: Vec<u8> = lamps
        .iter()
        .map(|l| {
            let fresh = state.lamps.get(&l.entity).is_none_or(|s| s.fresh);
            match view {
                Some(v) if cull && !l.spot && !l.transient && !fresh => faces_in_view(v, l.at, l.range, FACE_VIEW_MARGIN),
                _ => ALL_FACES,
            }
        })
        .collect();
    // Never drawn first, then those whose composite is due (their moving casters wait for
    // it), then those with marked faces that came into view, then those whose marked faces
    // waited longest ([`draw_priority`]); the nearest first among equals (as sorted).
    let mut order: Vec<usize> = (0..lamps.len()).collect();
    order.sort_by_key(|&i| {
        let s = state.lamps.get(&lamps[i].entity);
        (
            !s.is_some_and(|s| s.fresh || s.reslot),
            !s.is_some_and(|s| s.pending != 0),
            !s.is_some_and(|s| s.deferred & in_view[i] != 0),
            std::cmp::Reverse(s.map_or(0, |s| draw_priority(s.waiting, lamps[i].gap <= 0.0))),
        )
    });
    let (mut draws, mut transient_draws) = tune.map_or((0, 0), |t| (t.draws, t.transient_draws));
    let mut drawing: EntityHashMap<u8> = EntityHashMap::default();
    out.held.clear();
    out.to_static.clear();
    out.on_static.clear();
    out.static_quiet.clear();
    for i in order {
        let l = &lamps[i];
        let Some(s) = state.lamps.get_mut(&l.entity) else { continue };
        let all = if l.spot { 1 } else { ALL_FACES };
        s.dirty &= all;
        s.static_dirty &= s.dirty;
        s.pending &= s.dirty;
        (s.to_static, s.composite, s.quiet, s.drew, s.drew_fresh) = (0, 0, 0, 0, false);
        if tune.is_none() {
            continue;
        }
        // Every marked face in view draws in the frame it was marked, while the frame's
        // draws last: a moving thing's shadow follows it (until 2026-11-24 lights took
        // turns, every frame or every second or fourth).
        // A transient (a muzzle flash, 0.1 s; an explosion's light, still while it fades)
        // draws once and is held for the rest of its short life: a flash moved by the gun's
        // recoil was drawn 3.5 times on average, measured in the Prospector Saloon gunfight
        // [engineering choice, 2026-11-18].
        let want = if hold_all || s.fresh || s.reslot {
            all
        } else if l.transient {
            0
        } else {
            state.pass.faces_out_of_view += u64::from((s.dirty & !in_view[i]).count_ones());
            s.dirty & in_view[i]
        };
        s.deferred = s.dirty & !in_view[i];
        // Only a light the camera saw last frame (a transient: sees this frame) is drawn by
        // Bevy this frame.
        let budget = if l.transient { &mut transient_draws } else { &mut draws };
        // A light that changed cube texture draws whatever the frame's draws: until it has,
        // its new cubes cast nothing.
        let draw = if want != 0 && l.seen && takes_budget(budget, hold_all || predraw || s.reslot) { want } else { 0 };
        s.waiting = if want != 0 && draw == 0 { s.waiting.saturating_add(1) } else { 0 };
        if s.waiting > 0 && l.seen {
            state.pass.waited += 1;
            state.pass.waited_longest = state.pass.waited_longest.max(s.waiting);
        }
        if draw != 0 {
            // With the static cache: faces whose cache is out of date drawn into it (then
            // copied out, or quietly), the others composited from it.
            // A light that changed cube texture draws every caster into the sampled cubes
            // (no cache this once: `static_dirty` stays, and the caches follow next frame).
            let (to_static, composite, quiet) = if s.reslot {
                (0, 0, 0)
            } else if s.fresh {
                face_modes(draw, all, 0, 0, !s.plain)
            } else {
                face_modes(draw, s.static_dirty, s.pending, s.dynamic_faces, !s.plain)
            };
            (s.to_static, s.composite, s.quiet) = (to_static, composite, quiet);
            if to_static != 0 {
                out.to_static.insert(l.entity, to_static);
            }
            if composite != 0 {
                out.on_static.insert(l.entity, composite);
            }
            if quiet != 0 {
                out.static_quiet.insert(l.entity, quiet);
            }
            state.pass.static_faces += u64::from(to_static.count_ones());
            state.pass.composited += u64::from(composite.count_ones());
            state.pass.quiet += u64::from(quiet.count_ones());
            if predraw && !l.transient {
                state.predraw.lights.insert(l.entity);
                if shown_now.contains(&l.entity) {
                    state.predraw.unseen.insert(l.entity);
                }
                state.predraw.faces += u64::from(draw.count_ones());
                state.predraw.static_faces += u64::from(to_static.count_ones());
            }
            // The caches drawn are up to date (on the plain path there is none: all drawn
            // are). A face composited ahead of its cache's redraw stays marked for it, and
            // one drawn quietly for its composite.
            s.static_dirty &= !(if s.plain { draw } else { to_static });
            s.pending = (s.pending & !draw) | quiet;
            s.dirty = (s.dirty & !draw) | quiet | (s.static_dirty & draw);
            (s.drew, s.drew_fresh) = (draw, s.fresh || s.reslot);
            (s.fresh, s.reslot) = (false, false);
            if draw == all {
                s.at = l.at;
            }
            if l.transient {
                state.transients_drawn += u64::from(draw.count_ones());
                state.transients_switched += u64::from(!l.spot);
            } else {
                state.drawn += u64::from(draw.count_ones());
                state.switched += u64::from(!l.spot);
            }
            drawing.insert(l.entity, draw);
            // A light is switched on only to draw, a point light's other faces held.
            out.held.insert(l.entity, all & !draw);
        }
    }

    // The loading screen waits while the pre-draw draws: lamps drew this frame (their
    // composites may follow) or lamps, seen or not, still have marked faces; at most
    // PREDRAW_CAP_SECS. When the screen comes down, what was pre-drawn is logged.
    let busy = predraw && {
        let drew = lamps.iter().any(|l| !l.transient && drawing.contains_key(&l.entity));
        let waiting = lamps.iter().any(|l| !l.transient && state.lamps.get(&l.entity).is_some_and(|s| s.fresh || s.dirty != 0 || s.settle_at != 0));
        predraw_busy(drew, waiting, state.predraw.began.map_or(0.0, |b| time.elapsed_secs() - b))
    };
    if let Some(loading) = loading.as_mut() {
        if loading.lamp_shadows_busy != busy {
            loading.lamp_shadows_busy = busy;
        }
    }
    if state.predraw.screen && !screen_up {
        let p = std::mem::take(&mut state.predraw);
        if p.began.is_some() {
            let casting: Vec<&LampState> = state.lamps.values().filter(|s| !s.transient && !s.leaving).collect();
            info!(
                "light shadows: {} lights pre-drawn under the loading screen ({} faces, {} into the static cache); {} casting, {} of them not drawn yet; {} of those pre-drawn out of view",
                p.lights.len(),
                p.faces,
                p.static_faces,
                casting.len(),
                casting.iter().filter(|s| s.fresh).count(),
                p.unseen.len()
            );
            state.pass.predrawn += p.lights.len() as u64;
        }
    }
    state.predraw.screen = screen_up;

    // 4. Set the lights up: shadows switched on to draw, the casters' layers, the near plane
    // and biases.
    let (near, bias) = (near_z(), biases());
    let (mut points, mut spots) = (0, 0);
    for (e, _, _, _, point, spot, _, _, _, casting, _) in &mut lights {
        let cast = chosen.contains(&e);
        // The counts logged leave the transients out, or each muzzle flash wrote a line.
        let counted = cast && !transients.contains(&e);
        if let Some(mut p) = point {
            points += usize::from(counted);
            let on = cast && drawing.contains_key(&e);
            if p.shadow_maps_enabled != on {
                p.shadow_maps_enabled = on;
            }
            if cast && p.shadow_map_near_z != near {
                p.shadow_map_near_z = near;
            }
            if cast && (p.shadow_depth_bias, p.shadow_normal_bias) != bias {
                (p.shadow_depth_bias, p.shadow_normal_bias) = bias;
            }
        }
        if let Some(mut s) = spot {
            spots += usize::from(counted);
            let on = cast && drawing.contains_key(&e);
            if s.shadow_maps_enabled != on {
                s.shadow_maps_enabled = on;
            }
            if cast && s.shadow_map_near_z != near {
                s.shadow_map_near_z = near;
            }
            if cast && (s.shadow_depth_bias, s.shadow_normal_bias) != bias {
                (s.shadow_depth_bias, s.shadow_normal_bias) = bias;
            }
        }
        match (cast, casting) {
            (true, false) => {
                commands.entity(e).insert((CastingLight, casting_layers()));
            }
            (false, true) => {
                commands.entity(e).remove::<(CastingLight, RenderLayers)>();
            }
            _ => {}
        }
    }
    if out.slots != state.slots {
        out.slots.clone_from(&state.slots);
    }
    if out.static_cache != cache_on {
        out.static_cache = cache_on;
    }
    if out.point_capacity != state.capacity {
        out.point_capacity = state.capacity;
    }
    if out.far_slots != state.far_slots {
        out.far_slots.clone_from(&state.far_slots);
    }
    if (out.far_capacity, out.far_size) != (state.far_capacity, far_side as u32) {
        (out.far_capacity, out.far_size) = (state.far_capacity, far_side as u32);
    }
    if out.spot_slots != state.spot_slots {
        out.spot_slots.clone_from(&state.spot_slots);
    }
    if out.spot_capacity != state.spot_capacity {
        out.spot_capacity = state.spot_capacity;
    }
    // The fades (a light absent casts as upstream's): in and out over time, times by
    // distance.
    out.fade.clear();
    // By the lamps' weight ([`SHADOW_STRENGTH`]): every lamp is listed, as no fade is 1.
    out.fade.extend(state.lamps.iter().map(|(e, s)| (*e, s.fade * s.far_fade * SHADOW_STRENGTH)));
    // The renderer holds the lamps only while a mode is on; off, its point and spot light
    // paths are upstream's (bevy_pbr `HoldLightShadowMaps::active`).
    if out.active != tune.is_some() {
        out.active = tune.is_some();
    }
    removed.clear();
    idle.frames = if tune.is_some() { 0 } else { idle.frames.saturating_add(1).min(WIND_DOWN_FRAMES) };

    let now = (settings.choice, points, spots, state.capacity, state.far_capacity);
    if *logged != Some(now) {
        *logged = Some(now);
        match tune {
            None => info!("light shadows: off"),
            Some(t) => info!(
                "light shadows: {} cast ({points} point, {spots} spot lights) within {reach:.0} units, {} at most; cubes {}x{}, room for {}; far cubes {far_side}x{far_side}, room for {}; spot maps room for {}; transients (muzzle flashes, explosions) within {:.0} units, {} at most; static shadow cache {}",
                points + spots,
                t.lights,
                cube.size,
                cube.size,
                state.capacity,
                state.far_capacity,
                state.spot_capacity,
                transient_reach(reach),
                t.transients,
                {
                    // The cubes of both textures; the static cache costs as much again.
                    let cubes = cube_texture_mb(cube.size, state.capacity) + cube_texture_mb(far_side, state.far_capacity);
                    if cache_on { format!("on ({cubes:.0} MB), the cubes {cubes:.0} MB more") } else { format!("off, the cubes {cubes:.0} MB") }
                }
            ),
        }
    }
    // A summary of what was drawn, every 10 s of real time. With the mode off, the renderer's
    // counts and the systems' times are dropped, so the first summary after it is switched on
    // counts only its own frames.
    if tune.is_none() {
        bevy::pbr::nvf_take_light_view_counts();
        take_system_times();
    } else {
        state.frames += 1;
        state.since += time.delta_secs();
        if state.since >= 10.0 {
            let per = |n: u64| n as f32 / state.frames as f32;
            info!(
                "light shadows: {:.2} point lights switched on and {:.1} faces drawn a frame over {} frames ({} lights casting); marked a frame by shapes {:.2}, skeletons {:.2}, removals {:.2}, lights {:.2}; shapes such as {}; transients {} cast, {} switched on, {} faces drawn",
                per(state.switched),
                per(state.drawn),
                state.frames,
                state.lamps.values().filter(|l| !l.transient && !l.leaving).count(),
                per(state.causes[0]),
                per(state.causes[1]),
                per(state.causes[2]),
                per(state.causes[3]),
                if state.names.is_empty() { "none".to_string() } else { state.names.join(", ") },
                state.transients_cast,
                state.transients_switched,
                state.transients_drawn
            );
            let (ns, max) = take_system_times();
            info!("{}; {}", renderer_summary(&bevy::pbr::nvf_take_light_view_counts()), systems_summary(&ns, &max, state.frames));
            let (served, candidates, filed, everywhere) = crate::light_shadow_casters::take_grid_counts();
            state.pass.grid = [served, candidates, filed, everywhere];
            info!("{}", pass_summary(&state.pass, state.frames));
            state.pass = Pass::default();
            (state.switched, state.drawn, state.frames, state.since, state.causes) = (0, 0, 0, 0.0, [0; 4]);
            (state.transients_cast, state.transients_switched, state.transients_drawn) = (0, 0, 0);
            state.names.clear();
        }
    }
}

type CasterLists<'a> = (
    Entity,
    &'a GlobalTransform,
    &'a ViewVisibility,
    Option<&'a mut CubemapVisibleEntities>,
    Option<&'a mut VisibleMeshEntities>,
    Option<&'a PointLight>,
    Option<&'a SpotLight>,
);

type Shapes<'a> = (&'a GlobalTransform, &'a Aabb, Has<LightFixture>, Option<&'a nvf_play::RefVisual>, Has<ShadowDynamic>, Has<SkinnedMesh>);

/// Take the light fixtures out of the shapes Bevy found for each casting light's maps this
/// frame: every shape marked [`LightFixture`], and a small shape the light sits in or
/// against ([`FIXTURE_MARGIN`], [`FIXTURE_RADIUS`]). Runs after Bevy's culling of the
/// lights' maps. Only the lights drawing this frame (maps switched on) are filtered: the
/// others' lists are last draw's, which no one reads, and touching them marked them changed,
/// so Bevy extracted every casting light again every frame. Also takes out the casters
/// under the light's small-caster cut-off ([`cut_off`]; `NVF_LIGHT_SHADOW_CUTOFF=0`: none).
fn skip_fixtures(
    mut lights: Query<CasterLists, With<CastingLight>>,
    shapes: Query<Shapes>,
    shown: Res<NvfShadowLightsShown>,
    mut state: ResMut<LampShadowState>,
    mut logged: Local<bevy::ecs::entity::EntityHashSet>,
) {
    let _timed = Timed::start(1);
    let log = log_fixtures();
    let state = &mut *state;
    for (light, t, seen, cube, spot, point_light, spot_light) in &mut lights {
        if !point_light.is_some_and(|p| p.shadow_maps_enabled) && !spot_light.is_some_and(|s| s.shadow_maps_enabled) {
            continue;
        }
        // The light was told to draw on last frame's word that the camera sees it. If the
        // camera lost sight of it this frame, the renderer draws nothing of it: what it was
        // to draw is marked again (until 2026-11-24 it counted as drawn, and later
        // composites found no cache and left the static shadows out of the face).
        if !seen.get() && !shown.lights.contains(&light) {
            if let Some(s) = state.lamps.get_mut(&light) {
                s.dirty |= s.drew;
                if s.drew_fresh {
                    // Its first draw, or its draw after a change of cube texture: its cubes
                    // hold nothing of it yet.
                    (s.fresh, s.static_dirty) = (true, s.drew);
                } else {
                    s.static_dirty |= s.to_static;
                }
                state.pass.lost += 1;
            }
            continue;
        }
        // The light's step and map scale, for the cut-off (none for a light not in the
        // game's states).
        let cut = state.lamps.get(&light).filter(|_| cutoff_on()).map(|s| (s.step, s.map_scale));
        // The static shadow cache: the faces drawn into it this frame (static casters only)
        // and those composited from it (dynamic casters only); the faces whose lists hold
        // dynamic casters.
        let (to_static, composite) = state.lamps.get(&light).map_or((0, 0), |s| (s.to_static, s.composite));
        let mut dynamic_in = 0u8;
        let mut cuts = 0u64;
        let at = t.translation();
        let mut lists: Vec<&mut Vec<Entity>> = Vec::with_capacity(6);
        if let Some(cube) = cube {
            lists.extend(cube.into_inner().iter_mut().map(|v| &mut v.entities));
        }
        if let Some(spot) = spot {
            lists.push(&mut spot.into_inner().entities);
        }
        // For the test aid: the shapes skipped, and the small ones kept near the light.
        let (mut skipped, mut kept): (Vec<Entity>, Vec<Entity>) = (Vec::new(), Vec::new());
        // Bit `face` for the cube's faces in order (a spot light's one list: bit 0).
        for (face, list) in lists.into_iter().enumerate() {
            let bit = 1u8 << face.min(7);
            let (into_cache, over_cache) = (to_static & bit != 0, composite & bit != 0);
            list.retain(|e| {
                let Ok((st, aabb, marked, _, dynamic, skinned)) = shapes.get(*e) else { return true };
                let gap = if marked { None } else { fixture_gap(at, &st.affine(), aabb) };
                let skip = marked || gap.is_some_and(|g| g <= FIXTURE_MARGIN);
                if log {
                    if skip && !skipped.contains(e) {
                        skipped.push(*e);
                    } else if !skip && gap.is_some_and(|g| g <= 4.0 * FIXTURE_MARGIN) && !kept.contains(e) {
                        kept.push(*e);
                    }
                }
                if skip {
                    return false;
                }
                let small = cut.is_some_and(|(step, scale)| {
                    let radius = Vec3::from(aabb.half_extents).length() * max_scale(st);
                    cut_off(radius, st.transform_point(Vec3::from(aabb.center)).distance(at), scale, step)
                });
                cuts += u64::from(small);
                if small {
                    return false;
                }
                let dynamic = dynamic || skinned;
                if dynamic {
                    dynamic_in |= bit;
                }
                keeps_caster(into_cache, over_cache, dynamic)
            });
        }
        if cuts > 0 {
            state.pass.cut_casters += cuts;
            if let Some(s) = state.lamps.get_mut(&light) {
                s.cut_any = true;
            }
        }
        // Faces drawn into the cache that held dynamic casters: composited next, first.
        let back = put_back(to_static, dynamic_in);
        if let Some(s) = state.lamps.get_mut(&light) {
            s.dynamic_faces = dynamic_in;
            (s.dirty, s.pending) = (s.dirty | back, s.pending | back);
        }
        state.pass.put_back += u64::from(back.count_ones());
        if log && (!skipped.is_empty() || !kept.is_empty()) && logged.insert(light) {
            // Each by its reference, whether it is marked, its size and its gap to the light.
            let shown = |v: &[Entity]| {
                let all: Vec<String> = v
                    .iter()
                    .filter_map(|e| shapes.get(*e).ok())
                    .map(|(st, aabb, marked, r, ..)| {
                        let size = Vec3::from(aabb.half_extents) * 2.0 * st.compute_transform().scale.abs();
                        let gap = fixture_gap(at, &st.affine(), aabb).map_or("large".to_string(), |g| format!("{g:.0}"));
                        format!("{:08X}{} {:.0}x{:.0}x{:.0} gap {gap}", r.map_or(0, |r| r.0), if marked { " fixture" } else { "" }, size.x, size.y, size.z)
                    })
                    .collect();
                if all.is_empty() { "none".to_string() } else { all.join(", ") }
            };
            info!("light shadows: light {light} at {:.0} {:.0} {:.0} skips {}; small shapes near it kept: {}", at.x, -at.z, at.y, shown(&skipped), shown(&kept));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_systems_run_with_the_row_on_and_for_the_wind_down_after_it() {
        assert!(runs(1, WIND_DOWN_FRAMES));
        assert!(runs(2, WIND_DOWN_FRAMES));
        assert!(runs(0, 0));
        assert!(runs(0, WIND_DOWN_FRAMES - 1));
        assert!(!runs(0, WIND_DOWN_FRAMES));
    }

    /// The row Off from the start, switched on, switched off again: the systems stop after
    /// the wind-down, and the renderer's hold is active only while the row is on.
    #[test]
    fn with_the_row_off_nothing_runs_and_the_hold_is_inactive() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<PointLightShadowMap>()
            .add_plugins(LightShadowsPlugin(LightShadowMode::Off));
        let side = app.world().resource::<PointLightShadowMap>().size;
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(app.world().resource::<LightShadowsIdle>().frames, WIND_DOWN_FRAMES);
        assert!(!app.world().resource::<HoldLightShadowMaps>().active);

        app.world_mut().insert_resource(LightShadows::of(LightShadowMode::Quality));
        app.update();
        assert_eq!(app.world().resource::<LightShadowsIdle>().frames, 0);
        assert!(app.world().resource::<HoldLightShadowMaps>().active);

        app.world_mut().insert_resource(LightShadows::of(LightShadowMode::Off));
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(app.world().resource::<LightShadowsIdle>().frames, WIND_DOWN_FRAMES);
        assert!(!app.world().resource::<HoldLightShadowMaps>().active);
        // Bevy's cube side given back.
        assert_eq!(app.world().resource::<PointLightShadowMap>().size, side);
    }

    #[test]
    fn the_choices_are_the_sun_shadows_rows_and_each_on_choice_is_tuned() {
        assert_eq!(CHOICES, crate::shadow_rate::SUN_SHADOW_CHOICES);
        assert_eq!(tuning(3), None);
        assert!(tuning(2).unwrap().near > 0 && tuning(1).unwrap().near == 0, "Quality keeps two sizes of maps, Performance one texture");
        assert_eq!(tuning(0), None);
        let (p, q) = (tuning(1).unwrap(), tuning(2).unwrap());
        assert!(q.lights > p.lights && q.draws > p.draws, "Quality draws more");
        assert!(p.draws >= 2, "a room's lamps round one moving actor all draw in its frame");
        assert!(q.transients > p.transients && q.transient_draws >= p.transient_draws, "and more transients");
        assert!(p.transients >= 1 && p.transient_draws >= 1, "Performance casts a muzzle flash too");
        // A near cube's number is its slot and a far one's starts at bevy_pbr's
        // `NVF_FAR_CUBE`: the caps keep every slot under it.
        for t in [p, q] {
            assert!(t.lights + t.transients <= bevy::pbr::NVF_FAR_CUBE, "{t:?}");
        }
        // The lamps' shadows weigh a little over 1, within what the light's flags carry.
        assert!((1.0..=bevy::pbr::NVF_SHADOW_WEIGHT_MAX).contains(&SHADOW_STRENGTH));
        assert_eq!(bevy::pbr::nvf_fade_bits(1.0) >> 24, 55, "a fade of 1 is not 0: that is a light without one");
        assert_eq!(bevy::pbr::nvf_fade_bits(0.0) >> 24, 255);
        assert_eq!(bevy::pbr::nvf_fade_bits(SHADOW_STRENGTH) >> 24, 35);
        assert_eq!(bevy::pbr::nvf_fade_bits(9.0) >> 24, 1, "held to the largest");
        assert_eq!(LightShadows::of(LightShadowMode::Quality).choice, 2);
        assert_eq!(LightShadows::of(LightShadowMode::Off).choice, 0);
    }

    #[test]
    fn lights_are_picked_by_how_near_their_light_reaches_within_the_shadow_distance() {
        // Inside a light (edge distance 0) before one whose light ends 100 units away.
        let mut c = vec![(1, rank(500.0, 400.0, false)), (2, rank(300.0, 400.0, false)), (3, rank(5000.0, 400.0, false))];
        assert_eq!(pick(&mut c, 8000.0, 8), vec![2, 1, 3]);
        assert_eq!(pick(&mut c, 1000.0, 8), vec![2, 1], "beyond the shadow distance: none");
        assert_eq!(pick(&mut c, 8000.0, 1), vec![2], "the cap");
        // A casting light keeps its place against one KEEP times nearer.
        let mut c = vec![(1, rank(1200.0, 400.0, true)), (2, rank(1050.0, 400.0, false))];
        assert_eq!(pick(&mut c, 8000.0, 1), vec![1]);
        let mut c = vec![(1, rank(1200.0, 400.0, true)), (2, rank(700.0, 400.0, false))];
        assert_eq!(pick(&mut c, 8000.0, 1), vec![2]);
    }

    #[test]
    fn transients_are_muzzle_flashes_and_explosions_picked_near_by_their_own_cap() {
        assert!(SourceKind::MuzzleFlash.transient() && SourceKind::Explosion.transient());
        assert!(!SourceKind::GrassFire.transient() && !SourceKind::Attached.transient(), "ranked with the placed lights");
        // The reach: TRANSIENT_REACH, or the sun's shadow distance when shorter.
        assert_eq!(transient_reach(8000.0), TRANSIENT_REACH);
        assert_eq!(transient_reach(2000.0), 2000.0);
        let reach = transient_reach(8000.0);
        // Three flashes and an explosion (range 300 and 800): nearest edge first, the cap,
        // and none whose light ends beyond the reach.
        let mut c = vec![
            (1, rank(2000.0, 300.0, false)),
            (2, rank(600.0, 300.0, false)),
            (3, rank(3500.0, 800.0, false)),
            (4, rank(3400.0, 300.0, false)),
        ];
        assert_eq!(pick(&mut c, reach, 8), vec![2, 1, 3], "the explosion's light reaches 2,700 units: in; the far flash's 3,100: out");
        assert_eq!(pick(&mut c, reach, 2), vec![2, 1], "Quality");
        assert_eq!(pick(&mut c, reach, 1), vec![2], "Performance");
        assert_eq!(pick(&mut c, transient_reach(1500.0), 8), vec![2], "the sun's shadow distance shorter");
    }

    #[test]
    fn a_transient_casts_when_its_light_reaches_into_the_view() {
        // A camera at the origin looking down -z, 90 degrees wide, near plane 10.
        use bevy::camera::CameraProjection;
        let lens = bevy::camera::PerspectiveProjection { fov: std::f32::consts::FRAC_PI_2, aspect_ratio: 1.0, near: 10.0, ..default() };
        let clip = lens.get_clip_from_view();
        let view = Frustum(bevy::shape::ViewFrustum::from_clip_from_world(&clip));
        assert!(in_view(Some(&view), Vec3::new(0.0, 0.0, -1000.0), 100.0), "ahead");
        assert!(!in_view(Some(&view), Vec3::new(0.0, 0.0, 1000.0), 100.0), "behind");
        assert!(in_view(Some(&view), Vec3::new(0.0, 0.0, 1000.0), 1200.0), "behind, its light reaching past the camera");
        assert!(!in_view(Some(&view), Vec3::new(2000.0, 0.0, -100.0), 300.0), "off to the side");
        assert!(in_view(Some(&view), Vec3::new(0.0, 0.0, -1.0e6), 10.0), "no far plane");
        assert!(in_view(None, Vec3::new(0.0, 0.0, 1000.0), 1.0), "no frustum: the distance alone");
    }

    #[test]
    fn a_light_marks_its_maps_in_full_only_once_it_moved_past_the_threshold() {
        let drawn = Vec3::new(100.0, 50.0, 0.0);
        assert!(!light_moved(drawn, drawn));
        assert!(!light_moved(drawn, drawn + Vec3::X * (LIGHT_MOVE_EPS * 0.9)));
        assert!(light_moved(drawn, drawn + Vec3::X * (LIGHT_MOVE_EPS * 1.1)));
        assert!(light_moved(drawn, drawn - Vec3::Y * 40.0), "a running shooter's tenth of a second");
        // Small moves add up: measured from where it was drawn, not from last frame.
        let step = LIGHT_MOVE_EPS * 0.4;
        let path: Vec<Vec3> = (1..=5).map(|i| drawn + Vec3::X * step * i as f32).collect();
        assert_eq!(path.iter().position(|p| light_moved(drawn, *p)), Some(2), "after three steps of 0.4 of it");
        // A held map is sampled from where the light is now: the threshold stays under the
        // depth bias, or a swaying lamp's lit surfaces shadow themselves.
        const { assert!(LIGHT_MOVE_EPS < DEPTH_BIAS && MOVE_EPS < DEPTH_BIAS) };
    }

    #[test]
    fn a_shape_or_a_joint_marks_once_it_moved_past_the_threshold() {
        let aabb = Aabb { center: Vec3::ZERO.into(), half_extents: Vec3::new(10.0, 20.0, 5.0).into() };
        let at = |t: Transform| ShapeAt::of(&GlobalTransform::from(t), &aabb, false);
        let here = at(Transform::from_xyz(100.0, 0.0, 0.0));
        assert!(!here.moved(&here));
        assert!(!here.moved(&at(Transform::from_xyz(100.0 + MOVE_EPS * 0.9, 0.0, 0.0))));
        assert!(here.moved(&at(Transform::from_xyz(100.0 + MOVE_EPS * 1.1, 0.0, 0.0))));
        // A turn in place (a door on its hinge at its centre, a thing rolling): its corner moves.
        assert!(here.moved(&at(Transform::from_xyz(100.0, 0.0, 0.0).with_rotation(Quat::from_rotation_z(0.1)))));
        // The sphere it marks: its bounds' centre and bounding radius, scaled.
        let big = at(Transform::from_xyz(1.0, 2.0, 3.0).with_scale(Vec3::splat(2.0)));
        assert_eq!(Vec3::from(big.sphere().center), Vec3::new(1.0, 2.0, 3.0));
        assert!((big.radius - 2.0 * Vec3::new(10.0, 20.0, 5.0).length()).abs() < 1e-3);

        // Two parts of one body: lists of joints that start alike and differ after.
        let j = |n: u32| Entity::from_raw_u32(n).unwrap();
        assert_ne!(skeleton_key(&[j(1), j(2), j(3)]), skeleton_key(&[j(1), j(2), j(4)]), "the same first joint, another list");
        assert_eq!(skeleton_key(&[j(1), j(2), j(3)]), skeleton_key(&[j(1), j(2), j(3)]), "the same list: one skeleton");
        // A skeleton: any one joint, not the box round all of them (a head turning or an arm
        // lifting inside the box of a standing actor marked nothing until 2026-11-24).
        let pose = vec![Vec3::ZERO, Vec3::new(0.0, 100.0, 0.0), Vec3::new(30.0, 60.0, 0.0), Vec3::new(-30.0, 60.0, 0.0)];
        assert!(!joints_moved(&pose, &pose));
        let mut arm = pose.clone();
        arm[2] = Vec3::new(20.0, 70.0, 0.0);
        assert!(joints_moved(&pose, &arm), "an arm moved inside the joints' box");
        let mut sway = pose.clone();
        sway[1].x += MOVE_EPS * 0.5;
        assert!(!joints_moved(&pose, &sway), "under the threshold");
        assert!(joints_moved(&pose, &pose[..3]), "another skeleton");
    }

    #[test]
    fn a_light_drawn_without_all_its_casters_draws_again_a_little_later_each_time() {
        assert_eq!(settle_after(100, 0, false), Some((104, 1)));
        assert_eq!(settle_after(100, 1, false), Some((108, 2)));
        assert_eq!(settle_after(100, 7, false), Some((612, 8)));
        assert_eq!(settle_after(100, 9, false), Some((1124, 10)));
        assert_eq!(settle_after(100, 10, false), None, "then the refresh");
        assert_eq!(settle_after(100, 3, true), Some((102, 4)), "under a loading screen at once");
        // All of it is over well before the refresh comes.
        let all: u64 = SETTLE_FRAMES.iter().sum();
        assert!(all < tuning(2).unwrap().refresh && all > 600, "{all}");
    }

    #[test]
    fn marked_lights_take_turns_by_how_long_they_waited() {
        // Five lights marked every frame, two draws a frame: each draws within three frames.
        let mut waiting = [0u32; 5];
        let mut longest = 0;
        for _ in 0..40 {
            let mut order: Vec<usize> = (0..5).collect();
            order.sort_by_key(|&i| std::cmp::Reverse(draw_priority(waiting[i], false)));
            for (n, &i) in order.iter().enumerate() {
                waiting[i] = if n < 2 { 0 } else { waiting[i] + 1 };
                longest = longest.max(waiting[i]);
            }
        }
        assert!(longest <= 2, "{longest}");
        // A light the camera is inside of goes before one that waited as long outside.
        assert!(draw_priority(1, true) > draw_priority(1, false));
        assert_eq!(draw_priority(0, true), 0);
        assert_eq!(draw_priority(u32::MAX, true), u32::MAX);
    }

    #[test]
    fn the_renderer_summary_gives_views_events_and_times_a_frame() {
        let mut c = bevy::pbr::NvfLightViewCounts { frames: 4, views: 30, run: 10, hidden: 6, ..default() };
        c.ns[1] = 8_000_000; // prepare_lights: 2 ms a frame
        c.max_ns[1] = 5_000_000;
        c.events[0] = 12;
        let s = renderer_summary(&c);
        assert!(s.starts_with("light shadows: renderer over 4 frames: light views a frame 7.50 alive, 2.50 run, 6 despawned with their light hidden;"), "{s}");
        assert!(s.contains("views spawned 12, views despawned 0"), "{s}");
        assert!(s.contains("extract_lights 0.000/0.00, prepare_lights 2.000/5.00, uniform writer"), "{s}");
        // The smoke scenarios read the static cache's three events together; the kept phases'
        // follow them.
        assert!(s.contains("static cache draws 0, static cache composites 0, static cache misses 0, phases reused 0, kept phases wiped 0, views kept 0, kept phases resynced 0, entities rebinned 0, stale entries requeued 0;"), "{s}");
        // No frames counted (nothing rendered since): no division by zero.
        assert!(renderer_summary(&default()).contains("over 0 frames: light views a frame 0.00 alive"));
        // The game's own systems, mean over the game frames and the longest.
        let s = systems_summary(&[3_000_000, 0, 0, 0], &[2_500_000, 0, 0, 0], 3);
        assert_eq!(s, "game systems ms a frame mean/max: shadow_lights 1.000/2.50, skip_fixtures 0.000/0.00, soften 0.000/0.00, contact 0.000/0.00");
    }

    #[test]
    fn slots_are_the_lowest_free_and_cubes_follow_the_shadow_resolution() {
        assert_eq!(free_slot([].into_iter()), 0);
        assert_eq!(free_slot([0, 1, 3].into_iter()), 2);
        assert_eq!(free_slot([3, 1, 0, 2].into_iter()), 4);
        assert_eq!(free_slot([1, 2].into_iter()), 0);
        assert_eq!(cube_sides(1, 2048), (512, 0));
        assert_eq!(cube_sides(1, 4096), (1024, 0));
        assert_eq!(cube_sides(2, 2048), (1024, 512));
        assert_eq!(cube_sides(2, 4096), (1024, 512));
        // The near eight of 28 lamps at 1024 and the rest at 512, with their caches, against
        // all at 1024.
        let tiered = 2.0 * (cube_texture_mb(1024, 8) + cube_texture_mb(512, 20));
        assert!(tiered < 0.5 * 2.0 * cube_texture_mb(1024, 28), "{tiered}");
        // The nearest stay near; one in the near texture keeps its place against one a
        // little nearer, not against one much nearer.
        let lights = [(1, 100.0, true), (2, 300.0, true), (3, 280.0, false), (4, 900.0, false)];
        assert_eq!(near_set(&lights, 2), [1, 2].into_iter().collect());
        let lights = [(1, 100.0, true), (2, 300.0, true), (3, 200.0, false), (4, 900.0, false)];
        assert_eq!(near_set(&lights, 2), [1, 3].into_iter().collect());
        assert_eq!(near_set(&lights, 8).len(), 4);
    }

    #[test]
    fn a_shadow_fades_in_and_out_over_the_fade_time() {
        let half = FADE_SECS * 0.5;
        assert!((fade_step(0.0, true, half) - 0.5).abs() < 1e-5);
        assert_eq!(fade_step(0.9, true, half), 1.0, "no more than full");
        assert!((fade_step(1.0, false, half) - 0.5).abs() < 1e-5);
        assert_eq!(fade_step(0.2, false, half), 0.0, "no less than none");
        // At 60 frames a second it takes the fade time to come in full.
        let mut fade = 0.0;
        let frames = (0..100).position(|_| {
            fade = fade_step(fade, true, 1.0 / 60.0);
            fade >= 1.0
        });
        let expected = (FADE_SECS * 60.0).round() as usize;
        assert!(frames.is_some_and(|f| (f + 1).abs_diff(expected) <= 1), "{frames:?} frames, {expected} expected (a frame either way: rounding)");
    }

    #[test]
    fn a_light_no_longer_chosen_is_kept_while_its_shadow_fades_out() {
        let dt = 1.0 / 60.0;
        // A drawn lamp, alive: kept, fading, until its fade reaches 0.
        let mut fade = 1.0;
        let mut frames = 0usize;
        while let Some(f) = leaving_fade(fade, true, false, true, dt) {
            assert!(f < fade, "it falls");
            fade = f;
            frames += 1;
        }
        let expected = (FADE_SECS * 60.0).round() as usize;
        assert!((frames + 1).abs_diff(expected) <= 1, "dropped once at 0: {frames} frames kept, {expected} expected");
        // Half faded in when it left: half the time.
        let mut fade = 0.5;
        let mut half = 0;
        while let Some(f) = leaving_fade(fade, true, false, true, dt) {
            fade = f;
            half += 1;
        }
        assert!(half <= frames / 2 + 1, "{half} frames from half, {frames} from full");
        // Dropped at once: despawned, never drawn, a transient.
        assert_eq!(leaving_fade(1.0, true, false, false, dt), None, "despawned");
        assert_eq!(leaving_fade(0.0, false, false, true, dt), None, "never drew");
        assert_eq!(leaving_fade(1.0, true, true, true, dt), None, "a muzzle flash");
    }

    #[test]
    fn a_texture_that_only_grew_keeps_the_shadows_of_the_lamps_in_sight() {
        // More room, the same side and use: grown. Anything else is another texture.
        assert!(regrown(4, 8, true));
        assert!(!regrown(8, 8, true) && !regrown(8, 4, true) && !regrown(4, 8, false));
        // A lamp drawn before that the camera sees draws again at once and keeps its shadow.
        assert!(keeps_shadow(true, false, false, true));
        assert!(!keeps_shadow(false, false, false, true), "another texture: every lamp starts over");
        assert!(!keeps_shadow(true, true, false, true), "never drawn: nothing to keep");
        assert!(!keeps_shadow(true, false, true, true), "a transient draws anyway");
        assert!(!keeps_shadow(true, false, false, false), "out of sight: it cannot draw this frame");
    }

    #[test]
    fn spot_lights_keep_stable_slots_and_their_room_grows_in_fours() {
        let e = |n: u32| Entity::from_raw_u32(n).unwrap();
        let mut slots: EntityHashMap<u32> = EntityHashMap::default();
        assert_eq!(assign_slot(&mut slots, e(10)), 0);
        assert_eq!(assign_slot(&mut slots, e(11)), 1);
        assert_eq!(assign_slot(&mut slots, e(12)), 2);
        assert_eq!(assign_slot(&mut slots, e(10)), 0, "kept");
        // One leaves: the others keep theirs, a new one takes the freed slot.
        slots.remove(&e(10));
        assert_eq!(assign_slot(&mut slots, e(13)), 0);
        assert_eq!((slots[&e(11)], slots[&e(12)]), (1, 2));
        // The room: in fours, never shrinking, with the other shadowed spot lights.
        assert_eq!(grown_room(0, std::iter::empty(), 0), 0);
        assert_eq!(grown_room(0, [0, 1, 2].into_iter(), 0), 4);
        assert_eq!(grown_room(0, [0, 1, 2].into_iter(), 1), 4, "and the flashlight");
        assert_eq!(grown_room(0, [0, 1, 2].into_iter(), 2), 8);
        assert_eq!(grown_room(8, [0].into_iter(), 0), 8, "never shrinks");
        assert_eq!(grown_room(4, [4].into_iter(), 0), 8);
    }

    #[test]
    fn a_sphere_dirties_the_faces_it_reaches() {
        let light = Vec3::ZERO;
        let sphere = |c: Vec3, r: f32| Sphere { center: c.into(), radius: r };
        assert!(reaches(light, 400.0, &sphere(Vec3::new(450.0, 0.0, 0.0), 60.0)));
        assert!(!reaches(light, 400.0, &sphere(Vec3::new(500.0, 0.0, 0.0), 60.0)));
        // Without frusta (a light that never drew) every face counts.
        assert_eq!(faces_reached(&sphere(Vec3::X * 100.0, 10.0), None, None, 0), ALL_FACES);
        assert_eq!(faces_reached(&sphere(Vec3::X * 100.0, 10.0), None, None, 0b11), ALL_FACES & !0b11, "faces skipped are not tested");
    }

    #[test]
    fn the_lamp_grid_finds_every_lamp_a_small_sphere_reaches() {
        let lamps = [(Vec3::ZERO, 400.0), (Vec3::new(5000.0, 0.0, 0.0), 800.0), (Vec3::new(-3000.0, 200.0, 2500.0), 300.0), (Vec3::new(900.0, 0.0, 0.0), 600.0)];
        let grid = LampGrid::new(&lamps);
        // Against the plain test over a lattice of spheres around them: never one missed.
        let mut checked = 0;
        for x in (-4000..=6500).step_by(250) {
            for z in (-1000..=3500).step_by(250) {
                for r in [1.0, 60.0, GRID_MARGIN] {
                    let c = Vec3::new(x as f32, 100.0, z as f32);
                    let s = Sphere { center: c.into(), radius: r };
                    let cands = grid.candidates(c, r);
                    for (i, (at, range)) in lamps.iter().enumerate() {
                        if reaches(*at, *range, &s) {
                            assert!(cands.contains(&(i as u16)), "lamp {i} missed at {c} r {r}");
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(checked > 100, "{checked} reaches checked");
        // Far from every lamp: none; a sphere larger than the margin: all.
        assert!(grid.candidates(Vec3::new(20000.0, 0.0, 0.0), 10.0).is_empty());
        assert_eq!(grid.candidates(Vec3::ZERO, GRID_MARGIN + 1.0).len(), lamps.len());
        assert!(LampGrid::new(&[]).candidates(Vec3::ZERO, 1.0).is_empty(), "no lamps");
    }

    #[test]
    fn a_shape_marks_the_faces_it_reaches_and_a_lamp_drawing_in_full_is_left_alone() {
        let e = |n: u32| Entity::from_raw_u32(n).unwrap();
        let lamps = vec![test_lamp(1, Vec3::ZERO, 0), test_lamp(2, Vec3::new(2000.0, 0.0, 0.0), 0)];
        let frusta = vec![(None, None), (None, None)];
        let mut states: EntityHashMap<LampState> = EntityHashMap::default();
        states.insert(e(1), test_state(false, 0b1));
        states.insert(e(2), test_state(false, 0));
        let mark = |s: &Sphere, c: &[usize], states: &mut EntityHashMap<LampState>| mark_sphere(s, None, false, false, c.iter().copied(), &lamps, &frusta, states);
        let near = Sphere { center: Vec3::new(300.0, 0.0, 0.0).into(), radius: 20.0 };
        assert_eq!(mark(&near, &[0, 1], &mut states), Marked { reached: true, marked: true, cut: 0 });
        assert_eq!(states[&e(1)].dirty, ALL_FACES, "without frusta every face");
        assert_eq!(states[&e(2)].dirty, 0, "the far lamp is not reached");
        let far = Sphere { center: Vec3::new(9000.0, 0.0, 0.0).into(), radius: 20.0 };
        assert_eq!(mark(&far, &[0, 1], &mut states), Marked { reached: false, marked: false, cut: 0 });
        // A fresh lamp draws in full anyway: reached, nothing added.
        states.insert(e(2), test_state(true, 0));
        let by_two = Sphere { center: Vec3::new(2100.0, 0.0, 0.0).into(), radius: 20.0 };
        assert_eq!(mark(&by_two, &[1], &mut states), Marked { reached: true, marked: true, cut: 0 });
        assert_eq!(states[&e(2)].dirty, 0);
    }

    fn test_lamp(n: u32, at: Vec3, step: u8) -> Lamp {
        Lamp { entity: Entity::from_raw_u32(n).unwrap(), at, range: 500.0, spot: false, seen: true, gap: 0.0, transient: false, step, map_scale: map_scale(512.0, 1.0) }
    }

    fn test_state(fresh: bool, dirty: u8) -> LampState {
        LampState {
            dirty,
            fresh,
            refreshed: 0,
            phase: 0,
            at: Vec3::ZERO,
            transient: false,
            spot: false,
            fade: 1.0,
            leaving: false,
            step: 0,
            map_scale: map_scale(512.0, 1.0),
            cut_any: false,
            far_fade: 1.0,
            deferred: 0,
            static_dirty: 0,
            pending: 0,
            plain: false,
            to_static: 0,
            composite: 0,
            quiet: 0,
            dynamic_faces: 0,
            waiting: 0,
            far: false,
            reslot: false,
            unready: 0,
            unready_frame: 0,
            settle_at: 0,
            settle_step: 0,
            build_waits: 0,
            drew: 0,
            drew_fresh: false,
        }
    }

    #[test]
    fn a_face_the_camera_cannot_see_is_held_and_the_others_drawn() {
        // A camera at the origin looking down -z, 90 degrees wide, near plane 10.
        use bevy::camera::CameraProjection;
        let lens = bevy::camera::PerspectiveProjection { fov: std::f32::consts::FRAC_PI_2, aspect_ratio: 1.0, near: 10.0, ..default() };
        let view = Frustum(bevy::shape::ViewFrustum::from_clip_from_world(&lens.get_clip_from_view()));
        // A lamp ahead: every face can show.
        assert_eq!(faces_in_view(&view, Vec3::new(0.0, 0.0, -1000.0), 400.0, 0.0), ALL_FACES);
        // A lamp 500 units behind whose light reaches past the camera: the face pointing
        // further back (+z, index 5) and the side faces lie behind the side planes; the face
        // towards the camera (-z, index 4) shows.
        let behind = faces_in_view(&view, Vec3::new(0.0, 0.0, 500.0), 800.0, 0.0);
        assert_eq!(behind & (1 << 5), 0, "+z, away from the view");
        assert_ne!(behind & (1 << 4), 0, "-z, into the view");
        assert_eq!(behind & 0b1111, 0, "the sides: {behind:06b}");
        // A margin keeps a face just out of view.
        let side = Vec3::new(1100.0, 0.0, -1000.0);
        let near_edge = faces_in_view(&view, side, 400.0, 0.0);
        assert_eq!(near_edge & 1, 0, "+x, out past the right plane");
        assert_ne!(faces_in_view(&view, side, 400.0, 1000.0) & 1, 0, "within a wide margin");
        // Each face's own pyramid: the light's point and four far corners.
        for (i, f) in CUBE_MAP_FACES.iter().enumerate() {
            assert!(f.target.length() > 0.99, "face {i}");
        }
    }

    #[test]
    fn a_shadow_fades_out_over_the_last_stretch_of_the_reach() {
        let reach = 8000.0;
        assert_eq!(distance_fade(0.0, reach), 1.0);
        assert_eq!(distance_fade(reach * (1.0 - DISTANCE_FADE), reach), 1.0, "full up to the stretch");
        assert!((distance_fade(reach * (1.0 - DISTANCE_FADE / 2.0), reach) - 0.5).abs() < 1e-5, "half way through it");
        assert_eq!(distance_fade(reach, reach), 0.0, "none at the reach");
        assert_eq!(distance_fade(reach + 100.0, reach), 0.0);
        assert_eq!(distance_fade(100.0, 0.0), 1.0, "no reach: no fade");
        // Falls steadily as the camera goes away.
        let fades: Vec<f32> = (0..=20).map(|i| distance_fade(reach * 0.75 + i as f32 * 100.0, reach)).collect();
        assert!(fades.windows(2).all(|w| w[1] <= w[0]));
    }

    #[test]
    fn a_light_steps_down_in_detail_with_distance_and_hysteresis() {
        assert_eq!(stepped(0, 0.0, 0.0), 0);
        assert_eq!(stepped(0, NEAR + 1.0, 0.0), 1);
        assert_eq!(stepped(0, 2.0 * NEAR + 1.0, 0.0), 2);
        // Within the hysteresis band it keeps its step, either way.
        let h = STEP_HYSTERESIS;
        assert_eq!(stepped(0, NEAR * (1.0 + h * 0.5), h), 0, "a little past the step's distance: still near");
        assert_eq!(stepped(0, NEAR * (1.0 + h * 1.5), h), 1);
        assert_eq!(stepped(1, NEAR * (1.0 - h * 0.5), h), 1, "a little within: still mid");
        assert_eq!(stepped(1, NEAR * (1.0 - h * 1.5), h), 0);
        assert_eq!(stepped(2, 0.0, h), 0, "came straight in");
        assert_eq!(stepped(0, 1.0e6, h), 2, "jumped far");
        // Walking back and forth over the distance by less than the band: one step change.
        let mut step = 0;
        let mut changes = 0;
        for i in 0..40 {
            let gap = NEAR * (1.0 + if i % 2 == 0 { 0.08 } else { -0.08 }) + if i == 0 { 400.0 } else { 0.0 };
            let s = stepped(step, gap, h);
            changes += usize::from(s != step);
            step = s;
        }
        assert_eq!(changes, 1, "only the first, far-out sample stepped it");
    }

    #[test]
    fn small_casters_are_cut_by_their_texels_in_the_map_more_for_far_lights() {
        let cube = map_scale(512.0, 1.0);
        // A bottle (radius 5) 300 units from the lamp covers 8.5 texels of a 512 face: kept.
        assert!(!cut_off(5.0, 300.0, cube, 0) && !cut_off(5.0, 300.0, cube, 2));
        // A coin (radius 0.5) at 400 units: 0.64 texels, cut at every step.
        assert!(cut_off(0.5, 400.0, cube, 0));
        // A can (radius 3) at 600 units: 2.56 texels: kept near, cut far.
        assert!(!cut_off(3.0, 600.0, cube, 0) && !cut_off(3.0, 600.0, cube, 1) && cut_off(3.0, 600.0, cube, 2));
        // A larger cube map (shadow resolution 4096) shows more.
        assert!(!cut_off(3.0, 600.0, map_scale(1024.0, 1.0), 2));
        // A spot light's 2048 map over a 45-degree half cone (tan 1) is a 2048 face.
        assert_eq!(map_scale(2048.0, 1.0), 2048.0);
        assert!(map_scale(2048.0, 0.5) > map_scale(2048.0, 1.0), "a narrower cone: more texels a unit");
        // A caster at the light itself is never cut.
        assert!(!cut_off(0.5, 0.0, cube, 2));
    }

    #[test]
    fn a_caster_under_the_cut_off_marks_nothing_unless_it_was_over_it() {
        let e = |n: u32| Entity::from_raw_u32(n).unwrap();
        let lamps = vec![test_lamp(1, Vec3::ZERO, 2)];
        let frusta = vec![(None, None)];
        let mut states: EntityHashMap<LampState> = EntityHashMap::default();
        states.insert(e(1), test_state(false, 0));
        let tiny = |x: f32| Sphere { center: Vec3::new(x, 0.0, 0.0).into(), radius: 1.0 };
        // 1 unit at 400: 1.3 texels, under step 2's 4: nothing marked, one cut counted.
        let m = mark_sphere(&tiny(400.0), Some(&tiny(390.0)), true, false, 0..1, &lamps, &frusta, &mut states);
        assert_eq!(m, Marked { reached: true, marked: false, cut: 1 });
        assert_eq!(states[&e(1)].dirty, 0);
        // Shown, hidden or new (no old place): the same.
        assert_eq!(mark_sphere(&tiny(400.0), None, true, false, 0..1, &lamps, &frusta, &mut states).cut, 1);
        // Moved away from where it was over the cut-off (50 units: 10 texels): it was in the
        // map, so it marks.
        let m = mark_sphere(&tiny(400.0), Some(&tiny(50.0)), true, false, 0..1, &lamps, &frusta, &mut states);
        assert_eq!((m.marked, m.cut), (true, 0));
        assert_eq!(states[&e(1)].dirty, ALL_FACES);
        // The aid off: it marks.
        states.insert(e(1), test_state(false, 0));
        assert_eq!(mark_sphere(&tiny(400.0), None, false, false, 0..1, &lamps, &frusta, &mut states).cut, 0);
        assert_eq!(states[&e(1)].dirty, ALL_FACES);
    }

    #[test]
    fn the_performance_pass_line_gives_each_technique_a_frame() {
        let p = Pass {
            cut_casters: 300,
            cut_marks: 20,
            step_redraws: 3,
            steps: [100, 50, 50],
            grid: [50, 25_000, 9000, 40],
            distance_faded: 250,
            faces_out_of_view: 30,
            static_faces: 12,
            composited: 40,
            put_back: 5,
            promoted: 2,
            quiet: 4,
            lost: 1,
            waited: 25,
            waited_longest: 3,
            settled: 6,
            unready: [9, 2],
            reslots: 5,
            predrawn: 7,
        };
        let s = pass_summary(&p, 100);
        assert!(s.starts_with("light shadows: performance pass over 100 frames: lights at steps near/mid/far 1.0/0.5/0.5 a frame;"), "{s}");
        assert!(s.contains("casters cut 3.0 a frame, marks skipped 0.20 a frame, redraws for a finer step 3"), "{s}");
        assert!(s.contains("0.50 lamps served a frame, 500 candidates each, 9000 meshes filed (40 for every light)"), "{s}");
        assert!(s.contains("distance fade (on) 2.5 lights fading a frame"), "{s}");
        assert!(s.contains("face culling (on) 0.30 marked faces held out of view a frame;"), "{s}");
        assert!(
            s.contains("static cache (on) 0.12 static redraws (faces) a frame, 0.40 dynamic recomposites a frame, 0.52 layer copies a frame, 5 faces put back after a static redraw, 4 of them drawn quietly, 2 shapes became dynamic, 1 draws lost out of view; 0.25 marked lights waited a frame for a draw, the longest 3 frames; 6 redraws for casters not ready (9 views drawn with casters missing, 2 while a pipeline was built); 5 lights changed cube texture;"),
            "{s}"
        );
        assert!(s.ends_with("loading-screen pre-draw (on) 7 lights"), "{s}");
        assert!(pass_summary(&Pass::default(), 0).contains("over 0 frames"), "no division by zero");
    }

    #[test]
    fn a_static_shape_that_moves_becomes_dynamic_and_a_dynamic_one_stays_so() {
        // Shown, hidden or new: the static caches where it is.
        assert_eq!(shape_change(false, true, true), ShapeChange::Static, "shown or hidden (Primm's mass enable)");
        assert_eq!(shape_change(false, false, false), ShapeChange::Static, "new: a cell loading");
        // Moved after it marked the lamps: promoted (clutter pushed, a door turning).
        assert_eq!(shape_change(false, false, true), ShapeChange::Promoted);
        // An actor's part, a skinned shape, one promoted before: composites only.
        for (shown, known) in [(false, false), (false, true), (true, false), (true, true)] {
            assert_eq!(shape_change(true, shown, known), ShapeChange::Dynamic);
        }
    }

    #[test]
    fn a_moved_caster_marks_where_it_was_and_where_it_is() {
        let s = |x: f32, r: f32| Sphere { center: Vec3::new(x, 0.0, 0.0).into(), radius: r };
        let holds = |m: &Sphere, a: &Sphere| Vec3::from(m.center).distance(Vec3::from(a.center)) + a.radius <= m.radius + 1e-3;
        for (a, b) in [(s(0.0, 10.0), s(30.0, 10.0)), (s(0.0, 40.0), s(5.0, 2.0)), (s(100.0, 3.0), s(-50.0, 25.0)), (s(7.0, 5.0), s(7.0, 5.0))] {
            let m = merged_sphere(&a, &b);
            assert!(holds(&m, &a) && holds(&m, &b), "{m:?} from {a:?} and {b:?}");
            assert!(m.radius <= (Vec3::from(a.center).distance(Vec3::from(b.center)) + a.radius + b.radius) * 0.5 + 1e-3 || m.radius == a.radius.max(b.radius), "no larger than needed");
        }
        assert_eq!(merged_sphere(&s(0.0, 40.0), &s(5.0, 2.0)).radius, 40.0, "one inside the other: the outer");
        let m = merged_sphere(&s(0.0, 10.0), &s(30.0, 10.0));
        assert!((m.radius - 25.0).abs() < 1e-4 && (Vec3::from(m.center).x - 15.0).abs() < 1e-4, "{m:?}");
    }

    #[test]
    fn faces_with_an_old_cache_are_drawn_into_it_the_rest_composited() {
        // Faces 0-2 drawn this frame; 1 and 4 have an out-of-date cache.
        assert_eq!(face_modes(0b0111, 0b1_0010, 0, 0, true), (0b0010, 0b0101, 0));
        assert_eq!(face_modes(0b0111, 0b1_0010, 0, 0, false), (0, 0, 0), "the plain path: none");
        assert_eq!(face_modes(ALL_FACES, ALL_FACES, 0, 0, true), (ALL_FACES, 0, 0), "a first draw: all into the cache");
        // An actor stands in faces 1 and 2: face 1's cache is drawn quietly (the sampled
        // map keeps its picture, actor and all) and composited next frame.
        assert_eq!(face_modes(0b0111, 0b1_0010, 0, 0b0110, true), (0b0010, 0b0101, 0b0010));
        // That next frame the composite is due (pending) and the cache is marked again (a
        // thing despawns every frame): the composite goes first, the cache waits a frame.
        assert_eq!(face_modes(0b0010, 0b0010, 0b0010, 0b0110, true), (0, 0b0010, 0));
        // Which casters each face's list keeps.
        assert!(keeps_caster(true, false, false) && !keeps_caster(true, false, true), "into the cache: the static ones");
        assert!(!keeps_caster(false, true, false) && keeps_caster(false, true, true), "over the cache: the dynamic ones");
        assert!(keeps_caster(false, false, false) && keeps_caster(false, false, true), "the plain path: all");
        // A face drawn into the cache whose list held an actor: composited again, first; a
        // face composited with no dynamic caster left is the cache copied back, done.
        assert_eq!(put_back(0b0011, 0b0110), 0b0010);
        assert_eq!(put_back(0, ALL_FACES), 0, "composites are not put back");
    }

    #[test]
    fn transients_spot_lights_moving_lights_and_the_aids_take_the_plain_path() {
        assert!(!plain_path(true, false, false, false, false), "a still point lamp is cached");
        assert!(plain_path(false, false, false, false, false), "NVF_LIGHT_SHADOW_STATIC_CACHE=0");
        assert!(plain_path(true, true, false, false, false), "NVF_PLAY_TEST_LIGHT_SHADOW_HOLD=0");
        assert!(plain_path(true, false, true, false, false), "a muzzle flash");
        assert!(plain_path(true, false, false, true, false), "a spot light");
        assert!(plain_path(true, false, false, false, true), "a light that moved");
    }

    #[test]
    fn a_static_mark_marks_the_cache_and_a_dynamic_one_the_composite_only() {
        let e = |n: u32| Entity::from_raw_u32(n).unwrap();
        let lamps = vec![test_lamp(1, Vec3::ZERO, 0)];
        let frusta = vec![(None, None)];
        let mut states: EntityHashMap<LampState> = EntityHashMap::default();
        states.insert(e(1), test_state(false, 0));
        let near = Sphere { center: Vec3::new(200.0, 0.0, 0.0).into(), radius: 20.0 };
        // An actor moved: the faces to composite, no cache.
        mark_sphere(&near, None, false, false, 0..1, &lamps, &frusta, &mut states);
        assert_eq!((states[&e(1)].dirty, states[&e(1)].static_dirty), (ALL_FACES, 0));
        // A static shape shown in the same faces: their caches too, though the faces were
        // marked already.
        mark_sphere(&near, None, false, true, 0..1, &lamps, &frusta, &mut states);
        assert_eq!((states[&e(1)].dirty, states[&e(1)].static_dirty), (ALL_FACES, ALL_FACES));
        // A spot light: its one map.
        let mut spot = test_lamp(2, Vec3::ZERO, 0);
        spot.spot = true;
        use bevy::camera::CameraProjection;
        let lens = bevy::camera::PerspectiveProjection { fov: std::f32::consts::FRAC_PI_2, aspect_ratio: 1.0, near: 8.0, ..default() };
        let cone = Frustum(bevy::shape::ViewFrustum::from_clip_from_world(&lens.get_clip_from_view()));
        states.insert(e(2), test_state(false, 0));
        mark_sphere(&Sphere { center: Vec3::new(0.0, 0.0, -100.0).into(), radius: 20.0 }, None, false, true, 0..1, &[spot], &[(None, Some(cone))], &mut states);
        assert_eq!((states[&e(2)].dirty, states[&e(2)].static_dirty), (1, 1));
    }

    #[test]
    fn under_a_loading_screen_the_lamps_draw_without_a_budget_once_the_cells_are_in() {
        assert!(predrawing(true, true, true, false));
        assert!(!predrawing(true, true, true, true), "cells still coming in: as in play");
        assert!(!predrawing(true, true, false, false), "no screen");
        assert!(!predrawing(false, true, true, false), "ALC shadows off");
        assert!(!predrawing(true, false, true, false), "NVF_LIGHT_SHADOW_PREDRAW=0");
        // The budget: one a light while it lasts; none taken without a limit.
        let mut budget = 2;
        assert!(takes_budget(&mut budget, false) && takes_budget(&mut budget, false));
        assert!(!takes_budget(&mut budget, false), "spent");
        assert_eq!(budget, 0);
        assert!((0..40).all(|_| takes_budget(&mut budget, true)), "the pre-draw: every light");
        assert_eq!(budget, 0, "and takes nothing");
    }

    #[test]
    fn the_screen_waits_for_the_pre_draw_until_nothing_is_left_or_the_cap() {
        assert!(predraw_busy(true, false, 0.5), "lamps drew this frame: their composites may follow");
        assert!(predraw_busy(false, true, 0.5), "lamps, seen or not, with marked faces");
        assert!(!predraw_busy(false, false, 0.5), "done");
        assert!(!predraw_busy(true, true, PREDRAW_CAP_SECS), "the cap");
        assert!(predraw_busy(true, true, PREDRAW_CAP_SECS - 0.01));
    }

    /// The kept shadow phases' re-bin path (vendored bevy_pbr): a lamp's kept phase is back
    /// after frames without a view, its light's visible list made anew (every visible entity
    /// "added"), some binned entities gone from it and a new one in it. Upstream's add panicked
    /// ("Binning main entity … when it was already binned", Novac at 22:00, 2026-11-19).
    #[test]
    fn a_kept_shadow_phase_back_in_view_is_resynced_and_never_bins_twice() {
        use bevy::pbr::{nvf_add_once, nvf_resync_kept_phase, Shadow, ShadowBatchSetKey, ShadowBinKey};
        use bevy::render::batching::gpu_preprocessing::GpuPreprocessingMode;
        use bevy::render::mesh::allocator::MeshSlabs;
        use bevy::render::render_phase::{BinnedRenderPhaseType, DrawFunctionId, InputUniformIndex, ViewBinnedRenderPhases};
        use bevy::render::render_resource::CachedRenderPipelineId;
        use bevy::render::sync_world::MainEntity;
        use bevy::render::view::{RenderVisibleEntitiesClass, RetainedViewEntity};

        let mut world = World::new();
        let [a, b, c, d] = std::array::from_fn(|_| MainEntity::from(world.spawn_empty().id()));
        let lamp = MainEntity::from(world.spawn_empty().id());
        let view = RetainedViewEntity::new(lamp, None, 0);
        let mut phases = ViewBinnedRenderPhases::<Shadow>::default();
        phases.prepare_for_new_frame(view, GpuPreprocessingMode::Culling);
        let phase = phases.get_mut(&view).unwrap();
        let key = || ShadowBatchSetKey {
            pipeline: CachedRenderPipelineId::INVALID,
            draw_function: DrawFunctionId(0),
            material_bind_group_index: None,
            slabs: MeshSlabs::default(),
        };
        let bin = || ShadowBinKey { asset_id: AssetId::<Mesh>::default().untyped() };
        let add = |phase: &mut bevy::render::render_phase::BinnedRenderPhase<Shadow>, e: MainEntity, i: u32| {
            nvf_add_once(true, phase, key(), bin(), (Entity::PLACEHOLDER, e), InputUniformIndex(i), BinnedRenderPhaseType::MultidrawableMesh)
        };
        for (i, e) in [a, b, c].into_iter().enumerate() {
            assert!(!add(phase, e, i as u32), "binned once");
        }

        // Back in view: b and c still visible, a gone, d new; the list was made anew.
        let mut visible = RenderVisibleEntitiesClass::default();
        let mut now: Vec<(Entity, MainEntity)> = [b, c, d].iter().map(|e| (Entity::PLACEHOLDER, *e)).collect();
        now.sort_unstable_by_key(|(_, e)| *e);
        visible.update_cpu_culled_entities(&now);
        assert_eq!(visible.added_entities().len(), 3, "every visible entity reads as added");

        // The mesh instances as they are now: b and c keep the index they were binned with.
        let index_now = |at: [(MainEntity, u32); 3]| move |e: MainEntity| at.iter().find(|(x, _)| *x == e).map(|(_, i)| InputUniformIndex(*i));
        let mut to_queue = Vec::new();
        assert_eq!(nvf_resync_kept_phase(phase, &visible, index_now([(b, 1), (c, 2), (d, 3)]), &mut to_queue), (1, 0), "a left its bin");
        assert_eq!(to_queue, vec![(Entity::PLACEHOLDER, d)], "only d is queued");
        assert!(!add(phase, d, 3));

        // Away again and back: nothing saw c meanwhile, so its mesh instance went and came
        // back at another index (7; index 2 is another mesh's now), and b's is not there yet.
        // Both leave their bins and are queued again; d stays (the Primm casino's slabs of
        // shadow, 2026-11-25: c was drawn with index 2's transform).
        to_queue.clear();
        assert_eq!(nvf_resync_kept_phase(phase, &visible, index_now([(c, 7), (d, 3), (a, 2)]), &mut to_queue), (0, 2));
        assert_eq!(to_queue.len(), 2);
        assert!([b, c].iter().all(|e| to_queue.contains(&(Entity::PLACEHOLDER, *e))), "{to_queue:?}");
        assert!(phase.nvf_is_binned(d) && !phase.nvf_is_binned(b) && !phase.nvf_is_binned(c));
        assert!(!add(phase, b, 1) && !add(phase, c, 7));
        assert!(!phase.nvf_is_binned(a));
        assert!([b, c, d].iter().all(|e| phase.nvf_is_binned(*e)));

        // The guard: what the panic was (b queued while binned) is a re-bin.
        assert!(add(phase, b, 4), "b was binned already");
        assert!(phase.nvf_is_binned(b));
        assert_eq!(phase.nvf_retain_binned(|_, _| true), 0);
        assert_eq!(phase.nvf_retain_binned(|e, i| e != c || i != InputUniformIndex(7)), 1);
        assert!([b, d].iter().all(|e| phase.nvf_is_binned(*e)) && !phase.nvf_is_binned(c));
    }

    #[test]
    fn lamps_out_of_view_are_shown_nearest_first_once_those_in_view_are_done() {
        let mut unseen = vec![(1, 900.0), (2, 0.0), (3, 300.0), (4, 2500.0)];
        assert!(shown_unseen(&mut unseen, true, 8).is_empty(), "lamps in view still drawing go first");
        assert_eq!(shown_unseen(&mut unseen, false, 8), vec![2, 3, 1, 4]);
        assert_eq!(shown_unseen(&mut unseen, false, 2), vec![2, 3], "a few a frame");
        assert!(shown_unseen::<u32>(&mut [], false, 8).is_empty());
        // The cap spreads the most lamps a mode casts over a few frames, well within the
        // screen's wait (at 60 frames a second).
        let frames = tuning(2).unwrap().lights.div_ceil(PREDRAW_UNSEEN_PER_FRAME);
        assert!(frames <= 4 && (frames as f32 / 60.0) < PREDRAW_CAP_SECS);
    }

    #[test]
    fn the_cache_costs_as_much_again_as_the_cubes() {
        // 512 squared, 24 lamps: about 150 MB; 1024: four times that.
        assert!((cube_texture_mb(512, 24) - 150.99).abs() < 0.01);
        assert!((cube_texture_mb(1024, 32) - 805.31).abs() < 0.01);
        assert_eq!(cube_texture_mb(512, 0), 0.0);
    }

    #[test]
    fn the_largest_scale_is_read_off_the_axes() {
        let t = GlobalTransform::from(Transform::from_scale(Vec3::new(1.0, 3.0, 2.0)).with_rotation(Quat::from_rotation_y(0.7)));
        assert!((max_scale(&t) - 3.0).abs() < 1e-5);
        assert!((max_scale(&GlobalTransform::IDENTITY) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn lamp_models_are_fixtures_by_their_file_name() {
        for m in [
            r"meshes\dungeons\office\lights\offrmlighthanging01.nif",
            r"meshes\architecture\megaton\interior\shackinteriors\shackhanginglight03.nif",
            r"meshes\clutter\lights\LampLightLamp.nif",
            r"meshes\architecture\strip\neoclassicalsconce.nif",
            "meshes/clutter/lantern01.nif",
        ] {
            assert!(is_fixture_model(m), "{m}");
        }
        for m in [r"meshes\dungeons\office\lights\..\offrmwall01.nif", r"meshes\clutter\office\rubberfloormatlrg01.nif", r"meshes\light\shelf01.nif"] {
            assert!(!is_fixture_model(m), "{m}: only the file name counts");
        }
    }

    #[test]
    fn a_small_shape_the_light_sits_in_or_against_is_its_fixture_a_wall_never() {
        use bevy::math::{Affine3A, Quat};
        let is_fixture = |light: Vec3, to_world: &Affine3A, aabb: &Aabb| fixture_gap(light, to_world, aabb).is_some_and(|g| g <= FIXTURE_MARGIN);
        // A hanging box lamp 100 x 40 x 20 units, its middle 30 units over the floor point.
        let lamp = Aabb::from_min_max(Vec3::new(-50.0, -20.0, -10.0), Vec3::new(50.0, 20.0, 10.0));
        let at = Affine3A::from_translation(Vec3::new(0.0, 0.0, 30.0));
        assert!(is_fixture(Vec3::new(0.0, 0.0, 30.0), &at, &lamp), "inside");
        assert!(is_fixture(Vec3::new(0.0, 0.0, 8.0), &at, &lamp), "12 units under it");
        assert!(!is_fixture(Vec3::new(0.0, 0.0, -10.0), &at, &lamp), "30 units under it");
        assert!(!is_fixture(Vec3::new(80.0, 0.0, 30.0), &at, &lamp), "30 units beside it");
        // Turned a quarter about z and scaled 2: its long side runs along y, 200 units.
        let turned = Affine3A::from_scale_rotation_translation(Vec3::splat(2.0), Quat::from_rotation_z(std::f32::consts::FRAC_PI_2), Vec3::ZERO);
        let small = Aabb::from_min_max(Vec3::new(-25.0, -10.0, -5.0), Vec3::new(25.0, 10.0, 5.0));
        assert!(is_fixture(Vec3::new(0.0, 60.0, 0.0), &turned, &small), "along its long side");
        assert!(!is_fixture(Vec3::new(60.0, 0.0, 0.0), &turned, &small), "beyond its short side");
        // A wall piece 256 x 256 x 16 with the light 4 units off it: too large to be a fixture.
        let wall = Aabb::from_min_max(Vec3::new(-128.0, -8.0, -128.0), Vec3::new(128.0, 8.0, 128.0));
        assert_eq!(fixture_gap(Vec3::new(0.0, 12.0, 0.0), &Affine3A::IDENTITY, &wall), None);
        assert!(!is_fixture(Vec3::new(0.0, 12.0, 0.0), &Affine3A::IDENTITY, &wall));
    }
}
