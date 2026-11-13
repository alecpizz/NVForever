//! Sky, sun, ambient light and fog from the worldspace's climate and weather at a time of
//! day (fnv_world::sky; hypotheses listed there, in docs/spec-notes/world.md §6 and in
//! docs/design/layer3.md).
//!
//! The sky is the game's own sky meshes around the camera [observed: `meshes\sky\`]:
//! - `atmosphere.nif`, a dome coloured by the weather's sky-upper, horizon and sky-lower
//!   colours: sky-upper weighted by the vertex colours' blue channel, horizon by a fitted
//!   function of elevation, mixed as display (sRGB) values (`dome_color`) [hypothesis,
//!   fitted to screenshots of the original game, docs/spec-notes/world.md §6 "Sky
//!   colours"; reading the green channel as the horizon weight gave the horizon colour far
//!   too much of the sky].
//! - `clouds.nif`, four cloud shapes whose vertex alpha fades them towards the horizon.
//!   The weather's cloud layers 0-3 (WTHR DNAM, CNAM, ANAM, BNAM textures and PNAM
//!   colours) go on the four shapes in file order [hypothesis: layer 0 is the top dome,
//!   which the night weather fills with a starfield; layer 3 the low dome, which the
//!   clear day weather fills with `NVCloudlight.dds`]. By day they take on part of the
//!   sunlight's hue (`SkyState::cloud_tint`) [hypothesis, fitted].
//! - The sun: the climate's sun texture and glare texture spread over angular squares
//!   around the sun, coloured by the weather's "sun" colour and drawn into the dome (the
//!   disc, behind the clouds) and the clouds (the glare) by sky.wesl [hypothesis, fitted].
//! - The climate's night-sky model (CLMT MODL, `Sky\Stars.nif`, additive at
//!   `STAR_STRENGTH`, weighted by the star texture's alpha where its shader flags set
//!   Alpha_Texture),
//!   coloured by the weather's "stars" colour and faded in as the sun sinks from 0° to
//!   −12° (sunset to full dark) [hypothesis: the stars colour is white by day, so the
//!   weather alone does not hide them].
//! - A moon disc opposite the sun (where the night light comes from), textured
//!   `sky\masser_<phase>.dds` by the climate's phase length and the days passed, faded in
//!   as the sun goes from +5° to −6° [hypothesis].
//!
//! Every sky part uses `SkyMaterial` (sky.wesl): its colour is a display value, mapped
//! through the inverse of the camera's tonemapper so the image-space grade sees the
//! weather's colours as the game's grade does; by day the sky is dimmed and warmed
//! (`SkyLight`) [hypothesis, fitted].
//!
//! Without those meshes the sky falls back to a sphere. The sun lights the world as a
//! directional light (colour "sunlight"), optionally with cascaded shadow maps. Ambient
//! light uses "ambient", fog "fog" with the weather's day/night distances; an unset range
//! (far not beyond near) takes the other one, and a weather with neither has no fog
//! [hypothesis, world.md §6].

use crate::fly::FlyCam;
use crate::world::{game_to_bevy, WorldData};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::light::{CascadeShadowConfig, CascadeShadowConfigBuilder, NotShadowCaster, NotShadowReceiver};
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::pbr::{DistanceFog, ExtendedMaterial, FogFalloff, MaterialExtension};
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::prelude::*;
use fnv_nif::scene::{mesh_data, scene_objects, Decoded};
use fnv_nif::NifFile;
use fnv_world::imagespace::{adapter, cinematic_at, image_space, weather_adapters, Adapter, Cinematic};
use std::collections::HashMap;
use fnv_world::sky::{
    climate, default_weather, eligible, global_value, moon_phase, roll, roll_due, smoothstep, sun_direction, weather, Climate, ColorType,
    RegionWeather, Weather, WeatherChoice, MOON_PHASES,
};
use nvf_assets::{GameData, TextureCache};
use std::f32::consts::PI;

/// Dome radius in Bevy units (inside the camera's far plane).
const DOME_RADIUS: f32 = 350_000.0;
/// Radius of the game's sky meshes [observed: atmosphere.nif and clouds.nif span about
/// ±500 units horizontally].
const NIF_SKY_RADIUS: f32 = 500.0;
/// The atmosphere dome is flat: about 500 units across but only 250 high [observed], so
/// sky objects stay within half the dome radius to remain in front of it overhead.
const SKY_INNER: f32 = 0.5 * DOME_RADIUS;
/// Depth beyond this distance from the camera (game units) is the sky dome's (it is opaque
/// and in the depth prepass); nothing of the world is drawn this far (sun shafts,
/// `shafts.rs`).
pub const SKY_DEPTH_FROM: f32 = 0.9 * SKY_INNER;
/// Distance of the sun and moon discs and the sun's size [hypothesis: about 4 degrees
/// across].
const SUN_DISTANCE: f32 = 0.8 * SKY_INNER;
const SUN_SIZE: f32 = 0.07 * SUN_DISTANCE;
/// Angular size (full width, degrees) over which the sun texture is spread around the
/// sun and the factor on its colour, and the same for the glare texture [hypothesis,
/// fitted to the sun and its glow in screenshots of the original game,
/// docs/spec-notes/world.md §6 "Sun and glare"].
const SUN_SIZE_DEG: f32 = 20.0;
const SUN_STRENGTH: f32 = 1.8;
const GLARE_SIZE_DEG: f32 = 130.0;
const GLARE_STRENGTH: f32 = 0.12;
/// Moon quad size at SUN_DISTANCE; the disc fills about 30% of the masser textures
/// [observed], so this draws it about 3.6 degrees across [tuned by eye].
const MOON_SIZE: f32 = 2.4 * SUN_SIZE;
/// Sun elevations (degrees) over which the stars and the moon fade in [hypothesis].
const STARS_FADE: (f32, f32) = (0.0, -12.0);
const MOON_FADE: (f32, f32) = (5.0, -6.0);
/// How far south of the east-west line the sun's path runs, in radians (9°) [hypothesis,
/// fitted to the original game's sun positions; fnv_world::sky::sun_direction].
const SUN_SOUTH: f32 = 9.0 * PI / 180.0;
/// Elevation (radians) below which the directional light fades out towards the horizon.
const HORIZON_FADE: f32 = 0.1;
/// Game hours per real second at the default TimeScale of 30 [documented: GECK wiki
/// TimeScale]: a weather's transition, given in real seconds
/// (`Weather::transition_seconds`), fades over that many game hours.
const GAME_HOURS_PER_SECOND: f32 = 30.0 / 3600.0;
/// Transparent sky parts are drawn before everything else. Bevy sorts transparent meshes
/// by view-space z plus depth bias, ascending, and that value grows towards the camera
/// [documented: bevy_core_pipeline 0.19 Transparent3d], so a large negative bias draws the
/// sky (centred on the camera) first; blended world meshes write no depth and would
/// otherwise be painted over. Within the sky, lower biases are drawn first.
const SKY_SORT_BIAS: f32 = -1.0e6;

/// Whether a transparent item's sort key (view distance plus depth bias, as Bevy sorts
/// them) is one of the sky's blended or additive parts ([`SKY_SORT_BIAS`]: the cloud
/// layers, the night's starfield layer, the stars, the moon): above half again the bias and
/// up to the terrain layers' band (`nif_material::sorts_first`), which leaves room for
/// every view distance in the world (the stars sit at the world's origin, the moon
/// [`SUN_DISTANCE`] out). FSR draws these into its reactive reference (fsr.rs).
pub(crate) fn sorts_with_sky(sort_key: f32) -> bool {
    sort_key > SKY_SORT_BIAS * 1.5 && sort_key <= nvf_assets::nif_material::SORT_FIRST_BIAS * 1.5
}
/// How far the sun's shadows reach when they are switched on in a run started without
/// them (game units): the default of `--shadow-distance`. With [`DEFAULT_SHADOW_CASCADES`]
/// [engineering choice, 2026-10-12, owner's playtest "shadow rendering seriously affects
/// performance"]: every cascade is a pass over its casters, and at Goodsprings by day the
/// earlier 16,000 in 4 cascades took the frame from about 7.6 ms without sun shadows to
/// 19.7 ms; 8,000 in 3 takes it to about 14.4 ms (docs/design/graphics-enhanced.md §2).
/// The original casts no sun shadows from the world, so shadows ending nearer are no
/// further from its look.
pub const DEFAULT_SHADOW_DISTANCE: f32 = 8_000.0;
/// The sun's shadow cascades by default (see [`DEFAULT_SHADOW_DISTANCE`]).
pub const DEFAULT_SHADOW_CASCADES: usize = 3;

/// Shares of the weather's and the interior's light colours (display values) in the
/// display-value lighting sum (lights.rs `DISPLAY_HELPERS`; world.md §8 "Light on
/// surfaces in display values") [fitted to the original's frames: the lab's 15 L3 frames,
/// the effects audit's night frames and Dead Money's lantern]. Outdoors: the weather's
/// ambient colour, its sunlight by day (before the Sunlight Dimmer's scale) and its
/// night "sunlight" as moonlight. Interiors: XCLL's ambient and directional colours as
/// stored.
pub const AMBIENT_SHARE: f32 = 0.9;
pub const SUN_SHARE: f32 = 0.75;
pub const MOON_SHARE: f32 = 0.5;
pub const INTERIOR_AMBIENT_SHARE: f32 = 1.0;
pub const INTERIOR_DIRECTIONAL_SHARE: f32 = 1.0;

/// Light level that makes a colour of 1.0 light a white surface to 1.0 on screen: the
/// inverse of the camera's exposure (Bevy's default, EV100 9.7). The game's lighting is
/// treated as texture × (ambient + sunlight · N·L) in display units [hypothesis], so the
/// sun gets π times this (Lambert's 1/π) and ambient light this.
pub(crate) fn light_unit() -> f32 {
    1.0 / bevy::camera::Exposure::default().exposure()
}

/// Material of every sky part: unlit, with the colour mapped through the inverse of the
/// camera's tonemapper so the display value reaches the image-space grade (sky.wesl).
pub type SkyMaterial = ExtendedMaterial<StandardMaterial, SkyDisplay>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct SkyDisplay {
    #[uniform(100)]
    pub light: SkyLight,
    /// The climate's sun texture (drawn on the dome only, so clouds pass in front).
    #[texture(101)]
    #[sampler(102)]
    pub sun_texture: Option<Handle<Image>>,
    /// The climate's sun glare texture (dome and clouds).
    #[texture(103)]
    #[sampler(104)]
    pub glare_texture: Option<Handle<Image>>,
}

/// Daylight on the sky (sky.wesl). By day the original game's sky and clouds are darker
/// and warmer than the weather colours through the image-space grade would give, while at
/// dusk and at night they match [observed: screenshots of the original game,
/// docs/spec-notes/world.md §6 "Sky colours"]. Modelled as: linear colour × `scale` plus
/// a share of the sunlight colour, both growing as the sun rises [hypothesis, fitted to
/// those screenshots; the mechanism in the game is not known (eye adaptation and bloom
/// are candidates)].
///
/// The sun is drawn into the same colours, before the tonemapper inverse: the sun texture
/// and the glare texture spread over angular squares centred on the sun (SUN_SIZE_DEG,
/// GLARE_SIZE_DEG), added as colour × alpha (the textures' blend in the sky meshes) times
/// the weather's sun colour and a strength [hypothesis, fitted]. Adding them as separate
/// additive quads after the inverse could not reproduce the game's glow over a bright sky.
#[derive(Clone, Copy, Debug, Default, ShaderType, Reflect)]
pub struct SkyLight {
    /// x: scale; y: 1 for additive parts weighted by texture alpha (Alpha_Texture), 0 for
    /// those adding their colour alone; z: 1 for parts the light applies to (the dome,
    /// clouds and moon), 0 for additive ones (stars); w: 2 to draw the sun disc and glare (the dome), 1 the glare
    /// only (clouds), 0 neither.
    pub params: Vec4,
    /// Linear RGB added; w 1 when the view's tonemapper is the clip (lights.rs
    /// `display_clip`), 0 for Tony McMapface.
    pub glow: Vec4,
    /// xyz: direction to the sun (Bevy world space).
    pub sun_dir: Vec4,
    /// rgb: sun colour × strength (linear); w: tan of half the disc's angular size.
    pub sun: Vec4,
    /// rgb: sun colour × glare strength (linear); w: tan of half the glare's size.
    pub glare: Vec4,
}

impl MaterialExtension for SkyDisplay {
    fn fragment_shader() -> ShaderRef {
        "embedded://nvforever/sky.wesl".into()
    }
}

/// Daylight (`SkyLight`) applies to every part but the additive ones; `sun` is the
/// `SkyLight::params.w` mode.
fn sky_material(base: StandardMaterial, sun: f32) -> SkyMaterial {
    let applies = if matches!(base.alpha_mode, AlphaMode::Add) { 0.0 } else { 1.0 };
    let light = SkyLight { params: Vec4::new(1.0, 0.0, applies, sun), ..default() };
    ExtendedMaterial {
        // Forward even where the default is deferred (Solari, solari.rs): the sky is a picture, not a lit surface.
        base: StandardMaterial { unlit: true, fog_enabled: false, cull_mode: None, opaque_render_method: bevy::material::OpaqueRendererMethod::Forward, ..base },
        extension: SkyDisplay { light, ..default() },
    }
}

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "sky.wesl");
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default());
    }
}

#[derive(Resource)]
#[cfg_attr(test, derive(Default))]
pub struct SkyState {
    pub climate: Climate,
    pub weather: Weather,
    pub hour: f32,
    /// Fog distances from the weather are used as stored unless this is set.
    pub fog_override: Option<(f32, f32)>,
    /// Interior mode: the cell's own lighting replaces sky, sun and weather.
    pub interior: Option<fnv_world::lighting::CellLighting>,
    /// Interior mode: the cell's own image space (CELL XCIM) grades the picture.
    pub interior_grade: Option<Cinematic>,
    /// The sun casts shadows (`--sun-shadows` with `--shadow-distance` above 0; the settings page:
    /// [`set_shadows`]).
    pub shadows: bool,
    /// Sun shadows reach this far from the camera (game units).
    pub shadow_distance: f32,
    /// The cascades reach this many times `shadow_distance` (1; more with the finer
    /// shadows of `graphics.rs`), the first cascade as near as without.
    pub shadow_reach: f32,
    /// Number of the sun's shadow cascades.
    pub shadow_cascades: usize,
    /// The picture is graded by the image space (`--no-grade` and the settings page turn
    /// it off).
    pub grade: bool,
    /// Test aid: game hours per real second (0: time stands still).
    pub time_scale: f32,
    /// The time-lapse clock runs (set once the cells around the start are loaded).
    pub clock_running: bool,
    /// Game days passed at midnight of the current day (moon phase).
    pub days_passed: f32,
    /// The weather being faded out, while a change is in progress.
    pub prev: Option<Weather>,
    /// Progress of the fade from `prev` to `weather` (1 = done).
    pub fade: f32,
    /// The climate's weathers with their gates, when the weather follows the climate
    /// (no --weather).
    choices: Vec<Choice>,
    /// The worldspace's base image space (WRLD INAM); without one there is no grade.
    pub image_space: Option<Cinematic>,
    /// Each weather's image space adapters per time of day, by weather form ID.
    adapters: HashMap<u32, [Option<Adapter>; 6]>,
    changed: bool,
    /// A script's weather (SetWeather, ForceWeather), while it lasts.
    script: Option<ScriptWeather>,
    /// The weather was given (`--weather`): it never changes by itself (the parity lab's
    /// frames and the reference views pass it).
    pinned: bool,
    /// The worldspace's overriding weather regions with their weathers.
    regions: Vec<RegionList>,
    /// The region whose list the weather follows (index into `regions`); `None`: the
    /// climate's.
    source: Option<usize>,
    /// The camera's place has been looked at once ([`weather_region`]); until then the
    /// weather is the one the start chose and no roll is made.
    placed: bool,
    /// The game clock has moved the sky (fades take game time; without a clock a change
    /// is at once).
    pub(crate) clock_seen: bool,
    /// The list (region FormID, `None` for the climate) and its eligible weathers the
    /// last roll was made for.
    rolled_for: Option<(Option<u32>, Vec<u32>)>,
    /// The weather the last roll chose: shown unless a script's weather is.
    pick: u32,
    /// Rolls made so far.
    rolls: u64,
    /// The weather rolls' random state (splitmix64; `--weather-seed`, kept in saves).
    pub rng: u64,
    /// The whole game hour of the last volatility check.
    hour_index: Option<i64>,
    /// Gating globals' values as the simulation has them (`--play`).
    live_globals: HashMap<u32, f32>,
    /// A loaded game's weather: kept, without a roll, by the first list it is eligible in.
    resume: Option<u32>,
    /// A lightning flash's light now (linear RGB, added to the ambient light and the
    /// sky's glow; `crate::weather_fx`).
    flash: [f32; 3],
    /// The scripts' wind (SetWindDirection, `fnv_game::nvse_jip12`): degrees from the
    /// game's +x toward +y; `None` leaves `crate::tree`'s fixed direction.
    pub script_wind: Option<f32>,
    /// The scripts' moon textures by phase (SetMoonTexture), under `textures\`; `None`
    /// leaves the phase's own.
    pub moon_textures: [Option<String>; 8],
    /// A script asked for a lightning flash (TriggerLightningFX); `crate::weather_fx`
    /// draws it.
    pub lightning_request: bool,
}

/// An overriding weather region of the worldspace and its weathers.
struct RegionList {
    region: RegionWeather,
    choices: Vec<Choice>,
}

/// The weather rolls' seed when `--weather-seed` is not given [engineering choice: a
/// fixed seed keeps every run the same, the smoke tests' included; a save keeps the state].
pub const DEFAULT_WEATHER_SEED: u64 = 0x4E56_5745_4154_4852;

/// The sky's part of a save (nvf_play::save): the weather shown (the start takes it from
/// the save's header), whether a script holds it against the climate and the rolls' random
/// state. All of it is taken as the sky is made ([`SkyState::restore_saved`]), so loading
/// the part later does nothing: by then the sky has rolled from that state and a load
/// script may have released or changed the weather.
pub const SAVE_HOOK: nvf_play::save::SaveHook = nvf_play::save::SaveHook {
    key: "sky",
    save: |w| {
        let s = w.get_resource::<SkyState>()?;
        Some(serde_json::json!({"weather": s.weather.edid, "held": s.script.is_some_and(|sw| sw.hold), "rng": s.rng.to_string()}))
    },
    load: |_, _| {},
    remap: None,
};

/// A weather a script set: held until ReleaseWeatherOverride, or kept until the
/// weather system's next roll after the one it was set under [documented: GECK wiki
/// SetWeather, "the weather will change back in the normal fashion according to the
/// weather system"; hypothesis: "in the normal fashion" is the next roll].
#[derive(Debug, Clone, Copy)]
struct ScriptWeather {
    hold: bool,
    /// [`SkyState::rolls`] when the script set it.
    roll: u64,
}

/// A weather list entry (climate or region) with its weather and gating global.
struct Choice {
    weather: Weather,
    chance: i32,
    global: Option<Gate>,
}

/// A gating global: FormID, Editor ID and stored value.
struct Gate {
    id: u32,
    edid: Option<String>,
    stored: Option<f32>,
}

impl Choice {
    fn entry(&self) -> WeatherChoice {
        WeatherChoice { weather: self.weather.form_id, chance: self.chance, global: self.global.as_ref().map(|g| g.id) }
    }
}

/// Value of a gating global at `hour`: the simulation's value when it runs, else the
/// stored one; VNight always by the hour, standing in for the one script that drives the
/// Mojave's weather gate: NightQuestScript (FalloutNV.esm) sets VNight to 1 when
/// `GetCurrentTime >= 21 || GetCurrentTime < 4.30`, else to 0 [observed: its source text,
/// SCTX] (the script runs every few seconds, the stand-in keeps runs without `--play` and
/// the hour's edge exact).
fn gate_value(edid: Option<&str>, value: Option<f32>, hour: f32) -> Option<f32> {
    match edid {
        Some(e) if e.eq_ignore_ascii_case("VNight") => Some(if !(4.3..21.0).contains(&hour) { 1.0 } else { 0.0 }),
        _ => value,
    }
}

/// The next number in 0..1 of a splitmix64 stream.
fn next_unit(state: &mut u64) -> f32 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

impl SkyState {
    /// The worldspace's climate (or its parent's) and the weather: `pinned` (`--weather`)
    /// for good, else `start` (a loaded game's) or the climate's most likely, followed by
    /// the rolls from the first frame on ([`weather_region`]).
    pub fn new(world: &WorldData, pinned: Option<&str>, start: Option<&str>, hour: f32, seed: u64) -> Result<SkyState, String> {
        let ws = world.index.worldspaces.get(&world.world).ok_or("worldspace not in the world index")?;
        let clim_id = ws
            .climate
            .or_else(|| ws.parent.and_then(|p| world.index.worldspaces.get(&p)).and_then(|p| p.climate))
            .ok_or("worldspace has no climate")?;
        SkyState::with_climate(world, clim_id, pinned, start, hour, seed)
    }

    /// The sky of the start, when the start worldspace's own cannot be made (no climate,
    /// or one that is unreadable or lists no weather: a broken mod). Travel keeps the
    /// previous worldspace's sky then (worldspace.rs); at the start there is none, so the
    /// climate of another worldspace stands in: WastelandNV's, else the first (by FormID)
    /// that works [hypothesis: our fallback, not the original's behaviour].
    pub fn fallback(world: &WorldData, pinned: Option<&str>, start: Option<&str>, hour: f32, seed: u64) -> Result<SkyState, String> {
        let mut spaces: Vec<_> = world.index.worldspaces.values().filter_map(|w| Some((w.form_id, w.climate?, w.edid.as_deref()))).collect();
        spaces.sort_by_key(|&(id, _, edid)| (!edid.is_some_and(|e| e.eq_ignore_ascii_case("WastelandNV")), id));
        for (id, clim, _) in spaces {
            if let Ok(s) = SkyState::with_climate(world, clim, pinned, start, hour, seed) {
                info!("sky: the start uses worldspace {id:08X}'s climate");
                return Ok(s);
            }
        }
        Err("no worldspace in the load order has a usable climate".into())
    }

    /// [`SkyState::new`] with climate `clim_id`.
    pub fn with_climate(world: &WorldData, clim_id: u32, pinned: Option<&str>, start: Option<&str>, hour: f32, seed: u64) -> Result<SkyState, String> {
        let clim = climate(world.lo, world.schema, clim_id).ok_or("climate unreadable")?;
        let most_likely = || default_weather(&clim, |g| global_value(world.lo, world.schema, g)).ok_or("climate lists no weather");
        let wid = match (pinned, start) {
            (Some(n), _) => find_weather(world, n).ok_or_else(|| format!("no weather {n}"))?,
            (None, Some(n)) => match find_weather(world, n) {
                Some(id) => id,
                None => {
                    warn!("weather: the saved weather {n} is not in the load order; the climate's instead");
                    most_likely()?
                }
            },
            (None, None) => most_likely()?,
        };
        let load = |list: &[WeatherChoice]| -> Vec<Choice> {
            list.iter()
                .filter_map(|c| {
                    let global = c.global.map(|g| Gate { id: g, edid: global_edid(world, g), stored: global_value(world.lo, world.schema, g) });
                    Some(Choice { weather: weather(world.lo, world.schema, c.weather)?, chance: c.chance, global })
                })
                .collect()
        };
        let (choices, regions) = if pinned.is_some() {
            (Vec::new(), Vec::new())
        } else {
            let regions: Vec<RegionList> = world
                .weather_regions
                .iter()
                .filter(|r| r.overrides() && r.world == Some(world.world))
                .map(|r| RegionList { choices: load(&r.weathers), region: r.clone() })
                .filter(|r| !r.choices.is_empty())
                .collect();
            (load(&clim.weathers), regions)
        };
        let w = weather(world.lo, world.schema, wid).ok_or("weather unreadable")?;
        let image_space = worldspace_image_space(world);
        let adapters = choices
            .iter()
            .chain(regions.iter().flat_map(|r| &r.choices))
            .map(|c| &c.weather)
            .chain(std::iter::once(&w))
            .map(|w| {
                let ids = weather_adapters(world.lo, world.schema, w);
                (w.form_id, ids.map(|id| id.and_then(|id| adapter(world.lo, world.schema, id))))
            })
            .collect();
        Ok(SkyState {
            climate: clim,
            weather: w,
            hour,
            fog_override: None,
            interior: None,
            interior_grade: None,
            shadows: false,
            shadow_distance: DEFAULT_SHADOW_DISTANCE,
            shadow_reach: 1.0,
            shadow_cascades: DEFAULT_SHADOW_CASCADES,
            grade: true,
            time_scale: 0.0,
            clock_running: false,
            days_passed: 0.0,
            prev: None,
            fade: 1.0,
            choices,
            image_space,
            adapters,
            changed: true,
            script: None,
            pinned: pinned.is_some(),
            regions,
            source: None,
            placed: false,
            clock_seen: false,
            rolled_for: None,
            pick: wid,
            rolls: 0,
            rng: seed,
            hour_index: None,
            live_globals: HashMap::new(),
            resume: start.map(|_| wid),
            flash: [0.0; 3],
            script_wind: None,
            moon_textures: Default::default(),
            lightning_request: false,
        })
    }

    /// A loaded game's part of the sky ([`SAVE_HOOK`]), before the first look at the weather
    /// lists ([`SkyState::place`]): a hold set after that look would hold what its roll
    /// chose. Only the weather the save holds is held: the sky starts on another when that
    /// one is gone from the load order or `--weather` names one, and nothing asked to hold
    /// those.
    pub fn restore_saved(&mut self, saved: &serde_json::Value) {
        if let Some(r) = saved.get("rng").and_then(serde_json::Value::as_str).and_then(|r| r.parse().ok()) {
            self.rng = r;
        }
        let held = saved.get("held").and_then(serde_json::Value::as_bool) == Some(true);
        let showing = saved.get("weather").and_then(serde_json::Value::as_str).is_some_and(|w| self.weather.edid.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(w)));
        if held && showing {
            self.script = Some(ScriptWeather { hold: true, roll: self.rolls });
        }
    }

    /// The camera stands in the overriding weather region `region` (FormID; `None`: in
    /// none), or indoors with `at` = `None`, where the list followed stays as it was:
    /// follow that region's list, or the climate's.
    pub fn place(&mut self, at: Option<Option<u32>>) {
        let first = !self.placed;
        self.placed = true;
        // Said here, not in `new`, which runs before the log is set up.
        if first {
            info!("weather: the sky starts on day {:.0} at {:02}:{:02}", self.days_passed, self.hour as u32 % 24, (self.hour.fract() * 60.0) as u32);
        }
        if first && !self.regions.is_empty() {
            let names: Vec<String> = self
                .regions
                .iter()
                .map(|r| format!("{} (priority {}, {} weathers)", r.region.edid.as_deref().unwrap_or("?"), r.region.priority, r.choices.len()))
                .collect();
            info!("weather: {} region(s) override the climate here: {}", self.regions.len(), names.join(", "));
        }
        if first && self.script.is_some_and(|sw| sw.hold) {
            info!("load: the weather stays held as a script left it");
        }
        if let Some(region) = at {
            self.source = region.and_then(|f| self.regions.iter().position(|r| r.region.form_id == f));
        }
        self.follow_climate(first || !self.clock_seen);
        // A release waits for the roll after this one, not for the start's own.
        if let Some(sw) = self.script.as_mut().filter(|_| first) {
            sw.roll = self.rolls;
        }
    }

    /// The camera's region, if the place has been looked at.
    pub fn region(&self) -> Option<&str> {
        self.source.map(|i| self.regions[i].region.edid.as_deref().unwrap_or("?"))
    }

    /// The gating globals of the lists, to read from the simulation.
    pub fn gate_globals(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.choices.iter().chain(self.regions.iter().flat_map(|r| &r.choices)).filter_map(|c| c.global.as_ref().map(|g| g.id)).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// A gating global's value as the simulation has it; a change looks at the lists again.
    pub fn set_live_global(&mut self, id: u32, v: f32) {
        if self.live_globals.insert(id, v) != Some(v) {
            self.follow_climate(!self.clock_seen);
        }
    }

    /// The list followed now: the region's or the climate's, and its name.
    fn list(&self) -> (&[Choice], String) {
        match self.source {
            Some(i) => {
                let r = &self.regions[i].region;
                (&self.regions[i].choices, format!("region {} (priority {})", r.edid.as_deref().unwrap_or("?"), r.priority))
            }
            None => (&self.choices, format!("climate {}", self.climate.edid.as_deref().unwrap_or("?"))),
        }
    }

    /// The entries of the list followed that a roll draws from now.
    fn eligible_now(&self) -> Vec<WeatherChoice> {
        let (list, _) = self.list();
        let h = self.hour;
        let entries: Vec<WeatherChoice> = list.iter().map(Choice::entry).collect();
        eligible(&entries, |g| {
            let gate = list.iter().find_map(|c| c.global.as_ref().filter(|x| x.id == g))?;
            gate_value(gate.edid.as_deref(), self.live_globals.get(&g).copied().or(gate.stored), h)
        })
    }

    /// Follow the weather system (docs/spec-notes/world.md §6 "Choosing a weather"): roll
    /// again when the list followed or its eligible weathers change, or when the
    /// climate's volatility calls for it at a whole game hour; show the pick unless a
    /// script's weather holds. A change fades, or is at once with `instant`.
    fn follow_climate(&mut self, instant: bool) {
        if self.pinned || !self.placed {
            return;
        }
        let src = self.source.map(|i| self.regions[i].region.form_id);
        let el = self.eligible_now();
        let key = (src, el.iter().map(|w| w.weather).collect::<Vec<u32>>());
        let mut why = None;
        if self.rolled_for.as_ref() != Some(&key) {
            why = Some(match &self.rolled_for {
                None => "at the start",
                Some((s, _)) if *s != src && src.is_some() => "on entering the region",
                Some((s, _)) if *s != src => "on leaving a weather region",
                _ => "as the weathers open to it changed",
            });
            if let Some(p) = self.resume.take().filter(|p| key.1.contains(p)) {
                self.pick = p;
                self.rolled_for = Some(key.clone());
                why = None;
                info!("weather: {} kept from the saved game ({})", self.name_of(p), self.list().1);
            }
        }
        let idx = (self.days_passed as f64 * 24.0 + self.hour as f64).floor() as i64;
        if why.is_none() && self.climate.volatility > 0 {
            if let Some(prev) = self.hour_index.filter(|&p| idx > p) {
                for _ in prev..idx.min(prev + 24) {
                    if roll_due(self.climate.volatility, next_unit(&mut self.rng)) {
                        why = Some("by the climate's volatility");
                        break;
                    }
                }
            }
        }
        self.hour_index = Some(idx);
        if let Some(why) = why {
            let u = next_unit(&mut self.rng);
            if let Some(p) = roll(&el, u) {
                self.pick = p;
                self.rolls += 1;
                let total: i32 = el.iter().map(|w| w.chance.max(0)).sum();
                let chance = el.iter().find(|w| w.weather == p).map_or(0, |w| w.chance);
                info!(
                    "weather roll: {} ({chance} of {total} among {} eligible) from {} {why}, at {:02}:{:02}",
                    self.name_of(p),
                    el.len(),
                    self.list().1,
                    self.hour as u32 % 24,
                    (self.hour.fract() * 60.0) as u32
                );
            }
            self.rolled_for = Some(key);
        }
        if let Some(sw) = self.script {
            if sw.hold || sw.roll == self.rolls {
                return;
            }
            self.script = None;
            info!("weather: the climate takes over from the script's weather");
        }
        let pick = self.pick;
        if pick == self.weather.form_id {
            if instant {
                self.prev = None;
                self.fade = 1.0;
            }
            return;
        }
        if let Some(next) = self.weather_by_id(pick) {
            self.switch_to(next, instant);
        }
    }

    /// A weather of the lists by FormID.
    fn weather_by_id(&self, id: u32) -> Option<Weather> {
        self.choices.iter().chain(self.regions.iter().flat_map(|r| &r.choices)).find(|c| c.weather.form_id == id).map(|c| c.weather.clone())
    }

    fn name_of(&self, id: u32) -> String {
        match self.choices.iter().chain(self.regions.iter().flat_map(|r| &r.choices)).find(|c| c.weather.form_id == id) {
            Some(c) => c.weather.edid.clone().unwrap_or_else(|| format!("{id:08X}")),
            None => format!("{id:08X}"),
        }
    }

    /// Show `w`: a fade from the weather shown, or at once.
    fn switch_to(&mut self, w: Weather, instant: bool) {
        let h = self.hour;
        let old = std::mem::replace(&mut self.weather, w);
        if instant {
            self.prev = None;
            self.fade = 1.0;
        } else {
            self.prev = Some(old);
            self.fade = 0.0;
        }
        info!("weather: {} at {:02}:{:02}", self.weather.edid.as_deref().unwrap_or("?"), h as u32, (h.fract() * 60.0) as u32);
        self.changed = true;
    }

    /// A script's weather (SetWeather fades to it, ForceWeather is `instant`), held
    /// against the weather system with `hold` until [`SkyState::release_script_weather`].
    pub fn script_weather(&mut self, w: Weather, adapters: [Option<Adapter>; 6], instant: bool, hold: bool) {
        self.adapters.entry(w.form_id).or_insert(adapters);
        self.script = Some(ScriptWeather { hold, roll: self.rolls });
        info!(
            "weather: {} by a script ({}{})",
            w.edid.as_deref().unwrap_or("?"),
            if instant { "at once" } else { "fading in" },
            if hold { ", held until released" } else { "" }
        );
        if w.form_id == self.weather.form_id {
            return;
        }
        let old = std::mem::replace(&mut self.weather, w);
        if instant {
            self.prev = None;
            self.fade = 1.0;
        } else {
            self.prev = Some(old);
            self.fade = 0.0;
        }
        self.changed = true;
    }

    /// ReleaseWeatherOverride: the weather system chooses again (at its next roll).
    pub fn release_script_weather(&mut self) {
        if let Some(sw) = self.script.as_mut() {
            sw.hold = false;
            info!("weather: a script releases its weather");
        }
        self.follow_climate(false);
    }

    /// Light a lightning flash adds now (linear RGB; zero when none).
    pub fn set_flash(&mut self, flash: [f32; 3]) {
        if flash != self.flash {
            self.flash = flash;
            self.changed = true;
        }
    }

    /// The weather shown is rainy (WTHR DATA) and its change is over (IsRaining).
    pub fn raining(&self) -> bool {
        self.weather.rainy && self.prev.is_none()
    }

    /// Set the hour; `instant` skips weather fades (a jump rather than time passing).
    pub fn set_hour(&mut self, hour: f32, instant: bool) {
        self.hour = hour.rem_euclid(24.0);
        self.follow_climate(instant);
        self.changed = true;
    }

    /// `--play`: the sky shows the game's time of day (GameHour, which the simulation
    /// advances at TimeScale). Steps of half a game minute, so the weather is not
    /// re-evaluated every frame.
    pub fn follow_game_clock(&mut self, hour: f32, days_passed: f32) {
        self.clock_seen = true;
        let ahead = (hour - self.hour).rem_euclid(24.0);
        if (1.0 / 120.0..12.0).contains(&ahead) {
            self.days_passed = days_passed.floor();
            self.advance_fade(ahead);
            self.set_hour(hour, false);
        } else if ahead >= 12.0 {
            // The game's clock was set back (or far on): a jump, not time passing.
            self.days_passed = days_passed.floor();
            self.set_hour(hour, true);
        }
    }

    /// Advance a weather fade by `hours` of game time.
    fn advance_fade(&mut self, hours: f32) {
        if self.prev.is_some() {
            let length = self.weather.transition_seconds() * GAME_HOURS_PER_SECOND;
            self.fade = if length > 0.0 { (self.fade + hours / length).min(1.0) } else { 1.0 };
            if self.fade >= 1.0 {
                self.prev = None;
            }
            self.changed = true;
        }
    }

    /// Every colour type now as stored (0..255), for GetCurrentSkyColor
    /// (`fnv_game::nvse_jg9`).
    pub fn all_color_bytes(&self) -> [[f32; 3]; 10] {
        use fnv_world::sky::ColorType::*;
        [SkyUpper, Fog, Unused, Ambient, Sunlight, Sun, Stars, SkyLower, Horizon, EffectLighting].map(|t| self.color_bytes(t))
    }

    /// A weather colour at the current hour, blended across a weather fade.
    pub fn color(&self, ty: ColorType) -> [f32; 3] {
        let cur = self.weather.color(&self.climate, ty, self.hour);
        match &self.prev {
            Some(p) => {
                let old = p.color(&self.climate, ty, self.hour);
                std::array::from_fn(|i| old[i] + (cur[i] - old[i]) * self.fade)
            }
            None => cur,
        }
    }

    /// A colour type now as stored (0..255 per channel, blended on the bytes;
    /// `fnv_world::sky::Weather::color_bytes`), blended across a weather fade.
    pub fn color_bytes(&self, ty: ColorType) -> [f32; 3] {
        let cur = self.weather.color_bytes(&self.climate, ty, self.hour);
        match &self.prev {
            Some(p) => {
                let old = p.color_bytes(&self.climate, ty, self.hour);
                std::array::from_fn(|i| old[i] + (cur[i] - old[i]) * self.fade)
            }
            None => cur,
        }
    }

    /// The image-space grade now: the base image space with the weather's adapters for
    /// the time of day, blended across a weather fade; none indoors yet [interim].
    pub fn cinematic(&self) -> Cinematic {
        if !self.grade {
            return Cinematic::default();
        }
        if self.interior.is_some() {
            return self.interior_grade.unwrap_or_default();
        }
        let Some(base) = self.image_space else { return Cinematic::default() };
        let none = [None; 6];
        let at = |w: &Weather| cinematic_at(&base, self.adapters.get(&w.form_id).unwrap_or(&none), &self.climate, self.hour);
        let cur = at(&self.weather);
        match &self.prev {
            Some(p) => at(p).lerp(&cur, self.fade),
            None => cur,
        }
    }

    /// How strong the outdoor sun and moon are against the Mojave's at this hour: the
    /// image space's Sunlight Dimmer with the weather's adapters, blended like the grade
    /// and across a weather fade, over the dimmer the Mojave's lighting was fitted under
    /// (`fnv_world::imagespace::sun_scale`; world.md §6). 1 without an image space. Kept
    /// when the grade is switched off: it is light, not a colour grade.
    pub fn sun_scale(&self) -> f32 {
        let Some(base) = self.image_space else { return 1.0 };
        let none = [None; 6];
        let at = |w: &Weather| cinematic_at(&base, self.adapters.get(&w.form_id).unwrap_or(&none), &self.climate, self.hour).hdr.v[fnv_world::imagespace::HDR_SUNLIGHT_DIMMER];
        let cur = at(&self.weather);
        let dimmer = match &self.prev {
            Some(p) => at(p) + (cur - at(p)) * self.fade,
            None => cur,
        };
        fnv_world::imagespace::sun_scale(dimmer, &self.climate, self.hour)
    }

    /// Fog near, far and strength (0..1), blended across a weather fade; `None` when
    /// neither weather has fog. Fading to or from a weather without fog fades the
    /// strength and keeps the other weather's distances.
    pub fn fog(&self) -> Option<(f32, f32, f32)> {
        let cur = self.weather.fog_at(&self.climate, self.hour);
        let prev = match &self.prev {
            Some(p) => p.fog_at(&self.climate, self.hour),
            None => return cur.map(|(n, f)| (n, f, 1.0)),
        };
        let t = self.fade;
        match (prev, cur) {
            (Some((pn, pf)), Some((n, f))) => Some((pn + (n - pn) * t, pf + (f - pf) * t, 1.0)),
            (Some((n, f)), None) => Some((n, f, 1.0 - t)),
            (None, Some((n, f))) => Some((n, f, t)),
            (None, None) => None,
        }
    }

    /// Fog power now (the weather's FNAM day/night power, blended across a weather fade;
    /// see `fnv_world::sky::Weather::fog_power_at`). An overridden fog (`--fog`) is linear.
    pub fn fog_power(&self) -> f32 {
        if self.fog_override.is_some() {
            return 1.0;
        }
        let cur = self.weather.fog_power_at(&self.climate, self.hour);
        match &self.prev {
            Some(p) if p.fog_at(&self.climate, self.hour).is_some() && self.weather.fog_at(&self.climate, self.hour).is_some() => {
                let prev = p.fog_power_at(&self.climate, self.hour);
                prev + (cur - prev) * self.fade
            }
            Some(p) if self.weather.fog_at(&self.climate, self.hour).is_none() => p.fog_power_at(&self.climate, self.hour),
            _ => cur,
        }
    }

    /// How much brighter emitted light is drawn under the image space now: its adapted
    /// Emissive Mult ("multiplier applied to the emittance of an object" [documented:
    /// GECK wiki ImageSpace, HDR tab]) over the worldspace base image space's own
    /// [hypothesis: the renderer's emissive light was judged under the Mojave's base
    /// value, 1.5, which its adapters leave alone; Dead Money's Villa adapter multiplies
    /// it by 3.5 and Puesta del Sol's by 2, and there the lanterns' glass burns near
    /// white in the original. world.md §6 "Emissive mult"]. 1 indoors and without a base.
    pub fn emissive_scale(&self) -> f32 {
        use fnv_world::imagespace::HDR_EMISSIVE_MULT;
        if self.interior.is_some() {
            return 1.0;
        }
        let Some(base) = self.image_space.as_ref().map(|c| c.hdr.v[HDR_EMISSIVE_MULT]).filter(|&v| v > 0.0) else { return 1.0 };
        let now = self.cinematic().hdr.v[HDR_EMISSIVE_MULT];
        if now > 0.0 { now / base } else { 1.0 }
    }

    /// Recompute sky, light and fog next frame (e.g. after something else changed the fog).
    pub fn refresh(&mut self) {
        self.changed = true;
    }

    /// The image-space grade of an interior cell (CELL XCIM), if it names one.
    pub fn cell_grade(lo: &fnv_esm::LoadOrder, schema: &fnv_esm::Schema, cell: u32) -> Option<Cinematic> {
        fnv_world::lighting::cell_image_space(lo, schema, cell).and_then(|f| fnv_world::imagespace::image_space(lo, schema, f))
    }

    /// Switch between interior lighting (`Some`) and the worldspace's sky (`None`).
    pub fn set_interior(&mut self, lighting: Option<fnv_world::lighting::CellLighting>) {
        self.interior = lighting;
        self.changed = true;
    }

    /// How daylight changes the sky's display colours now (see [`SkyLight`]).
    pub fn light(&self) -> SkyLight {
        let day = self.daylight();
        let sun = self.color(ColorType::Sunlight);
        let s = Vec3::from_array(sun);
        // The disc and glare fade out as the sun sets [hypothesis: the game shows no glare
        // once the sun is down].
        let fade = if self.interior.is_some() { 0.0 } else { smoothstep(-2.0, 3.0, self.sun_elevation()) };
        let sun_col = Vec3::from_array(self.color(ColorType::Sun)) * fade;
        let dir = game_to_bevy().rotation * Vec3::from_array(sun_direction(&self.climate, self.hour, SUN_SOUTH));
        let half_tan = |deg: f32| (deg / 2.0).to_radians().tan();
        // The weather's Sun Glare, blended across a weather fade.
        let glare = self.weather.glare_scale();
        let glare = self.prev.as_ref().map_or(glare, |p| p.glare_scale() + (glare - p.glare_scale()) * self.fade);
        SkyLight {
            params: Vec4::new(1.0 - SKY_DIM * day, 0.0, 1.0, 0.0),
            // w: 1 when the main view shows display values clipped (lights.rs
            // `display_clip`), so the sky leaves out its inverse of Tony McMapface.
            glow: (s * SKY_GLOW * day).extend(if crate::lights::display_clip() { 1.0 } else { 0.0 }),
            sun_dir: dir.extend(0.0),
            sun: (sun_col * SUN_STRENGTH).extend(half_tan(SUN_SIZE_DEG)),
            glare: (sun_col * GLARE_STRENGTH * glare).extend(half_tan(GLARE_SIZE_DEG)),
        }
    }

    /// Daylight strength (0..1): grows as the sun rises to `DAYLIGHT_DEG`.
    pub fn daylight(&self) -> f32 {
        if self.interior.is_some() {
            return 0.0;
        }
        smoothstep(0.0, DAYLIGHT_DEG, self.sun_elevation())
    }

    /// A cloud colour lit by the sun: by day the game's clouds are warmer than their
    /// weather colour, by about the sunlight colour's hue [hypothesis fitted to the noon
    /// reference frames, world.md §6 "Sky colours"].
    pub fn cloud_tint(&self, c: [f32; 3]) -> [f32; 3] {
        self.sun_hue(c, CLOUD_SUN)
    }

    /// The fog colour as distant land fades into it: by day the game's haze is warm where
    /// the weather's fog colour is a light blue (NVWastelandClear day (150, 168, 190)), so
    /// the fog takes on the sunlight colour's hue in full daylight [hypothesis, fitted to
    /// the exterior reference frames, world.md §6 "Fog colour by day"].
    pub fn fog_color(&self) -> [f32; 3] {
        self.sun_hue(self.color(ColorType::Fog), FOG_SUN)
    }

    /// `c` moved towards the sunlight colour's hue (each channel × sunlight / its largest
    /// channel) by `share` × daylight.
    fn sun_hue(&self, c: [f32; 3], share: f32) -> [f32; 3] {
        let s = self.color(ColorType::Sunlight);
        let m = s.iter().copied().fold(1e-3f32, f32::max);
        let k = (self.daylight() * share).min(1.0);
        std::array::from_fn(|i| c[i] * (1.0 + (s[i] / m - 1.0) * k))
    }

    /// Direction towards the sun, in Bevy's axes (also below the horizon).
    pub fn sun_dir(&self) -> Vec3 {
        game_to_bevy().rotation * Vec3::from_array(sun_direction(&self.climate, self.hour, SUN_SOUTH))
    }

    /// Direction towards the sun, in game axes (also below the horizon).
    pub fn sun_dir_game(&self) -> [f32; 3] {
        sun_direction(&self.climate, self.hour, SUN_SOUTH)
    }

    /// Compass heading of the sun in degrees (0 north, 90 east).
    pub fn sun_heading(&self) -> f32 {
        let d = sun_direction(&self.climate, self.hour, SUN_SOUTH);
        d[0].atan2(d[1]).to_degrees().rem_euclid(360.0)
    }

    /// Sun elevation above the horizon in degrees.
    pub fn sun_elevation(&self) -> f32 {
        sun_direction(&self.climate, self.hour, SUN_SOUTH)[2].clamp(-1.0, 1.0).asin().to_degrees()
    }

    /// Twilight (0..1): 1 while the sun is above the horizon, falling to 0 as it sinks to
    /// `TWILIGHT_END_DEG` (the end of astronomical twilight) [hypothesis fitted 2026-10-23:
    /// the share of the horizon colour's wide floor that fits the original's sky is about
    /// 1.1 with the sun at +11° (19:06), 0.6 at −9° (20:30) and 0 at −54° (23:00); world.md
    /// §6 "Sky colours"]. 1 indoors (no sky).
    pub fn twilight(&self) -> f32 {
        if self.interior.is_some() {
            return 1.0;
        }
        smoothstep(TWILIGHT_END_DEG, 0.0, self.sun_elevation())
    }

    /// Star and moon visibility (0..1) at this hour.
    pub fn night_visibility(&self) -> (f32, f32) {
        let e = self.sun_elevation();
        (smoothstep(STARS_FADE.0, STARS_FADE.1, e), smoothstep(MOON_FADE.0, MOON_FADE.1, e))
    }

    /// Index into MOON_PHASES for the current day.
    pub fn moon_phase(&self) -> usize {
        moon_phase(self.days_passed + self.hour / 24.0, self.climate.moon_phase_length())
    }

    /// The time, sun elevation and the luminance of the light and sky colours at this
    /// hour (for the time-lapse log).
    pub fn sample(&self) -> String {
        let h = self.hour;
        let lum = |ty| {
            let v = self.color(ty);
            0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]
        };
        let elevation = self.sun_elevation();
        let (stars, moon) = self.night_visibility();
        format!(
            "day {:.0} {:02}:{:02}  {}  sun elevation {elevation:6.1}°  stars {stars:.2} moon {moon:.2} ({})  luminance: sunlight {:.3} ambient {:.3} sky-upper {:.3} horizon {:.3} fog {:.3}",
            self.days_passed,
            h as u32 % 24,
            (h.fract() * 60.0) as u32,
            self.weather.edid.as_deref().unwrap_or("?"),
            MOON_PHASES[self.moon_phase()],
            lum(ColorType::Sunlight),
            lum(ColorType::Ambient),
            lum(ColorType::SkyUpper),
            lum(ColorType::Horizon),
            lum(ColorType::Fog)
        )
    }

    pub fn describe(&self) -> String {
        if self.interior.is_some() {
            return "interior lighting (XCLL / template)".into();
        }
        format!(
            "{:02}:{:02}  climate {}  weather {}{}",
            self.hour as u32 % 24,
            ((self.hour.fract()) * 60.0) as u32,
            self.climate.edid.as_deref().unwrap_or("?"),
            self.weather.edid.as_deref().unwrap_or("?"),
            self.region().map_or(String::new(), |r| format!("  region {r}"))
        )
    }
}

/// The weather follows the region the camera stands in (the player's eye in play mode, as
/// for imposters): an overriding weather region's list, else the climate's; indoors the
/// list stays as it was [hypothesis].
pub fn weather_region(world: Res<WorldData>, cam: Query<&Transform, With<FlyCam>>, mut sky: ResMut<SkyState>) {
    let Ok(t) = cam.single() else { return };
    if sky.interior.is_some() {
        if !sky.placed {
            sky.place(None);
        }
        return;
    }
    let g = crate::world::bevy_to_game(t.translation);
    let inside = fnv_world::sky::region_at(world.weather_regions, world.world, g.x, g.y)
        .map(|r| r.form_id)
        .filter(|f| sky.regions.iter().any(|r| r.region.form_id == *f));
    let now = sky.source.map(|i| sky.regions[i].region.form_id);
    if !sky.placed || inside != now {
        sky.place(Some(inside));
    }
}

/// The worldspace's image space (WRLD INAM), or its parent's.
fn worldspace_image_space(world: &WorldData) -> Option<Cinematic> {
    let inam = |id: u32| {
        let (pi, r) = fnv_world::fields::winner(world.lo, id)?;
        let f = fnv_world::fields::Fields::new(r, pi, world.schema).ok()?;
        fnv_world::fields::global(world.lo, pi, fnv_world::fields::formid_of(&f.get(b"INAM")?)?)
    };
    let ws = &world.index.worldspaces[&world.world];
    let id = inam(world.world).or_else(|| ws.parent.and_then(inam))?;
    let c = image_space(world.lo, world.schema, id);
    info!("image space {id:#010x}: {c:?}");
    c
}

/// A global variable's Editor ID.
fn global_edid(world: &WorldData, fid: u32) -> Option<String> {
    let (pi, r) = fnv_world::fields::winner(world.lo, fid)?;
    fnv_world::fields::Fields::new(r, pi, world.schema).ok()?.string(b"EDID")
}

/// A global variable's value by Editor ID (e.g. `GameDaysPassed`).
pub fn find_global(world: &WorldData, edid: &str) -> Option<f32> {
    world.lo.records.iter().enumerate().find_map(|(pi, recs)| {
        recs.iter().filter(|r| &r.header.sig == b"GLOB").find_map(|r| {
            let f = fnv_world::fields::Fields::new(r, pi, world.schema).ok()?;
            f.string(b"EDID")?.eq_ignore_ascii_case(edid).then(|| f.get(b"FLTV")?.as_f64().map(|v| v as f32)).flatten()
        })
    })
}

fn find_weather(world: &WorldData, name: &str) -> Option<u32> {
    if let Some(hex) = name.strip_prefix("0x") {
        return u32::from_str_radix(hex, 16).ok();
    }
    world.lo.records.iter().enumerate().find_map(|(pi, recs)| {
        recs.iter().filter(|r| &r.header.sig == b"WTHR").find_map(|r| {
            let f = fnv_world::fields::Fields::new(r, pi, world.schema).ok()?;
            let e = f.string(b"EDID")?;
            e.eq_ignore_ascii_case(name).then(|| world.lo.to_global(pi, r.header.form_id)).flatten()
        })
    })
}

/// The atmosphere dome, with each vertex's elevation (degrees) and sky-upper weight kept
/// from creation: the colours are recomputed from these.
#[derive(Component)]
pub struct SkyDome(Vec<[f32; 2]>);

/// One of the four cloud layers (WTHR layer index) in one of two sets: the set showing
/// the current weather and the one fading out the previous weather.
#[derive(Component)]
pub struct CloudLayer {
    layer: usize,
    set: usize,
    /// Form ID of the weather whose texture this layer shows.
    shows: Option<u32>,
}

#[derive(Component)]
pub struct Sun;

/// Render layer of what casts a sun shadow without being seen: the sun casts from it and
/// from the world's layer 0, and no camera draws it (the player's body in first person,
/// `player_body.rs`). Layer 1 is the water's (`reflection::WATER_LAYER`).
pub const SHADOW_LAYER: usize = 2;

/// Render layer of the player's first-person arms and their weapon: seen by the cameras,
/// and the sun does not cast from it (`player_body.rs`).
pub const ARMS_LAYER: usize = 3;

/// Render layer of what the cameras see of the player's third-person body and its weapon:
/// the sun casts from it, the flashlight does not (`flashlight.rs`; owner 2026-10-22: the
/// player casts no flashlight shadow, as the arms did aiming in third person).
pub const BODY_LAYER: usize = 4;

/// Alpha_Texture in a BSShaderProperty's shader flags (bit 8) [documented: nif.xml BSShaderFlags].
const ALPHA_TEXTURE: i64 = 1 << 8;

/// What a star dome adds, as a share of its texture's display value × the weather's
/// stars colour [hypothesis, fitted 2026-10-23 to the original's clear Mojave night at
/// 23:00 (star peaks p50 39, p90 69, max 143 grey in a 1280 × 720 frame at FOV 75; this
/// value gives 40, 76, 130); the same value leaves the Big MT's stars about 1.7 × the
/// original's, as much as our grade lifts that sky's background (world.md §6 "Star
/// domes")]. The material's emissive multiplier (× 1 Mojave, × 10 Big MT) is not used.
const STAR_STRENGTH: f32 = 0.25;

/// The climate's star model.
#[derive(Component)]
pub struct Stars;

/// The moon disc with one texture per phase (MOON_PHASES order).
#[derive(Component)]
pub struct MoonDisc {
    phases: Vec<Option<Handle<Image>>>,
    shown: Option<usize>,
}

/// Geometry of one shape of a sky mesh, in game space (z up).
struct SkyShape {
    /// The SkyShaderProperty's texture (file_name), if any.
    texture: Option<String>,
    /// The SkyShaderProperty's shader flags set Alpha_Texture (bit 8).
    alpha_texture: bool,
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uv: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

/// The triangle shapes of a sky NIF in file order (their transforms are the identity in
/// the game's sky meshes [observed]).
fn sky_shapes(data: &GameData, path: &str) -> Vec<SkyShape> {
    let Some(bytes) = data.read(path) else {
        warn!("{path}: not found; using the fallback sky");
        return Vec::new();
    };
    // A panic reading a broken file counts as a file with no shapes (the fallback sky).
    let read = nvf_assets::guard::guarded(|| Ok(read_sky_shapes(data, &bytes)));
    read.unwrap_or_else(|e| {
        warn!("{path}: not read ({e}); using the fallback sky");
        Vec::new()
    })
}

fn read_sky_shapes(data: &GameData, bytes: &[u8]) -> Vec<SkyShape> {
    let Ok(file) = NifFile::parse(bytes, Some(&data.schema)) else { return Vec::new() };
    let dec = Decoded::new(&file, bytes, &data.schema);
    scene_objects(&file, &dec)
        .iter()
        .filter(|o| !o.is_node)
        .filter_map(|o| {
            let shape = dec.blocks[o.block].as_ref().ok()?;
            let md = mesh_data(dec.get(shape.block_ref("data")?)?);
            let n = md.positions.len();
            if n == 0 || md.triangles.is_empty() {
                return None;
            }
            let colors = if md.colors.len() == n { md.colors } else { vec![[1.0; 4]; n] };
            let uv = md.uv_sets.into_iter().next().filter(|u| u.len() == n).unwrap_or_else(|| vec![[0.0; 2]; n]);
            let indices = md.triangles.iter().filter(|t| t.iter().all(|&i| (i as usize) < n)).flatten().copied().collect();
            let props: Vec<_> = shape.refs("properties").unwrap_or_default().iter().filter_map(|&r| dec.get(r)).collect();
            let texture = props.iter().find_map(|p| p.text("file_name").filter(|t| !t.is_empty()).map(str::to_string));
            let alpha_texture = props.iter().filter_map(|p| p.int("shader_flags")).any(|f| f & ALPHA_TEXTURE != 0);
            Some(SkyShape { texture, alpha_texture, positions: md.positions, colors, uv, indices })
        })
        .collect()
}

fn sky_mesh(s: &SkyShape, colors: Vec<[f32; 4]>) -> Mesh {
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, s.positions.clone());
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 0.0, 1.0]; s.positions.len()]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, s.uv.clone());
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    m.insert_indices(Indices::U32(s.indices.clone()));
    m
}

/// Fallback dome: a sphere (game space, z up) with weights from height [hypothesis,
/// the earlier interim gradient]: horizon to sky-upper over the lower half of the upper
/// hemisphere, horizon to sky-lower just below the horizon.
fn fallback_dome() -> SkyShape {
    let mut sphere = Sphere::new(NIF_SKY_RADIUS).mesh().uv(48, 24);
    let pos: Vec<[f32; 3]> = match sphere.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(p)) => p.iter().map(|v| [v[0], -v[2], v[1]]).collect(),
        _ => Vec::new(),
    };
    let colors = pos
        .iter()
        .map(|p| {
            let y = p[2] / NIF_SKY_RADIUS;
            if y >= 0.0 {
                let t = (y / 0.5).clamp(0.0, 1.0).sqrt();
                [0.0, 1.0 - t, t, 1.0]
            } else {
                let t = (-y / 0.2).clamp(0.0, 1.0);
                [t, 1.0 - t, 0.0, 1.0]
            }
        })
        .collect();
    let n = pos.len();
    let indices = match sphere.indices_mut() {
        Some(Indices::U32(i)) => i.clone(),
        Some(Indices::U16(i)) => i.iter().map(|&x| x as u32).collect(),
        None => Vec::new(),
    };
    SkyShape { texture: None, alpha_texture: false, positions: pos, colors, uv: vec![[0.0; 2]; n], indices }
}

/// Transform of a sky mesh around the camera: game space to Bevy, scaled to the dome.
fn sky_transform(at: Vec3) -> Transform {
    Transform { translation: at, rotation: game_to_bevy().rotation, scale: Vec3::splat(DOME_RADIUS / NIF_SKY_RADIUS) }
}

#[allow(clippy::too_many_arguments)]
pub fn setup_sky(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut textures: ResMut<TextureCache>,
    data: Res<GameData>,
    sky: Res<SkyState>,
) {
    let sun_tex = sky.climate.sun_texture.as_deref().and_then(|t| textures.get(t, true, &data, &mut images, None));
    let glare_tex = sky.climate.sun_glare_texture.as_deref().and_then(|t| textures.get(t, true, &data, &mut images, None));
    let atmosphere = sky_shapes(&data, "meshes\\sky\\atmosphere.nif").into_iter().next().unwrap_or_else(fallback_dome);
    let weights = atmosphere.colors.clone();
    // Per vertex: elevation (degrees) and the sky-upper weight (blue).
    let dome: Vec<[f32; 2]> = atmosphere
        .positions
        .iter()
        .zip(&weights)
        .map(|(p, w)| [p[2].atan2(p[0].hypot(p[1])).to_degrees(), w[2]])
        .collect();
    commands.spawn((
        Name::new("Sky"),
        SkyDome(dome),
        Mesh3d(meshes.add(sky_mesh(&atmosphere, weights))),
        // Opaque, so it is drawn before every transparent part of the world.
        MeshMaterial3d(materials.add({
            let mut m = sky_material(StandardMaterial::default(), 2.0);
            m.extension.sun_texture = sun_tex;
            m.extension.glare_texture = glare_tex.clone();
            m
        })),
        sky_transform(Vec3::ZERO),
        NotShadowCaster,
        NotShadowReceiver,
    ));
    for (layer, shape) in sky_shapes(&data, "meshes\\sky\\clouds.nif").iter().take(4).enumerate() {
        // Vertex alpha fades the layer out; the colour comes from the weather.
        let colors: Vec<[f32; 4]> = shape.colors.iter().map(|c| [1.0, 1.0, 1.0, c[3]]).collect();
        let mesh = meshes.add(sky_mesh(shape, colors));
        for set in 0..2 {
            commands.spawn((
                Name::new(format!("Cloud layer {layer}.{set}")),
                CloudLayer { layer, set, shows: None },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(materials.add({
                    let mut m = sky_material(
                        StandardMaterial { alpha_mode: AlphaMode::Blend, depth_bias: SKY_SORT_BIAS + (2 * layer + set) as f32, ..default() },
                        1.0,
                    );
                    m.extension.glare_texture = glare_tex.clone();
                    m
                })),
                sky_transform(Vec3::ZERO),
                Visibility::Hidden,
                NotShadowCaster,
                NotShadowReceiver,
            ));
        }
    }
    // Stars: inside the atmosphere dome, drawn before the moon, sun and clouds.
    if let Some(model) = sky.climate.model.as_deref() {
        let shapes = sky_shapes(&data, &format!("meshes\\{model}"));
        // The star sphere is about 16 units in radius [observed: Sky\Stars.nif].
        let radius = shapes
            .iter()
            .flat_map(|s| s.positions.iter())
            .map(|p| Vec3::from_array(*p).length())
            .fold(0.0f32, f32::max)
            .max(1.0);
        for shape in &shapes {
            let tex = shape.texture.as_deref().and_then(|t| textures.get(t, true, &data, &mut images, None));
            commands.spawn((
                Name::new("Stars"),
                Stars,
                Mesh3d(meshes.add(sky_mesh(shape, vec![[1.0; 4]; shape.positions.len()]))),
                MeshMaterial3d(materials.add({
                    let mut m = sky_material(StandardMaterial {
                        base_color: Color::LinearRgba(LinearRgba::new(0.0, 0.0, 0.0, 1.0)),
                        base_color_texture: tex,
                        alpha_mode: AlphaMode::Add,
                        depth_bias: SKY_SORT_BIAS - 30.0,
                        ..default()
                    }, 0.0);
                    // Additive (NiAlphaProperty ONE/ONE): the texture's alpha weighs the
                    // colour only where the shader flags set Alpha_Texture (sky.wesl).
                    m.extension.light.params.y = if shape.alpha_texture { 1.0 } else { 0.0 };
                    m
                })),
                Transform { rotation: game_to_bevy().rotation, scale: Vec3::splat(0.9 * SKY_INNER / radius), ..default() },
                NotShadowCaster,
                NotShadowReceiver,
            ));
        }
    }
    if sky.climate.has_moon() {
        // A script's texture for a phase in place of the phase's own (SetMoonTexture).
        let phases = MOON_PHASES
            .iter()
            .zip(&sky.moon_textures)
            .map(|(p, set)| match set {
                Some(t) => textures.get(t, true, &data, &mut images, None),
                None => textures.get(&format!("sky\\masser_{p}.dds"), true, &data, &mut images, None),
            })
            .collect();
        commands.spawn((
            Name::new("Moon disc"),
            MoonDisc { phases, shown: None },
            Mesh3d(meshes.add(Mesh::from(Rectangle::new(MOON_SIZE, MOON_SIZE)))),
            MeshMaterial3d(materials.add(sky_material(StandardMaterial {
                alpha_mode: AlphaMode::Blend,
                depth_bias: SKY_SORT_BIAS - 20.0,
                ..default()
            }, 0.0))),
            Transform::from_scale(Vec3::ZERO),
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
    let mut sun = commands.spawn((
        Name::new("Sun"),
        Sun,
        DirectionalLight { illuminance: PI * light_unit(), shadow_maps_enabled: sky.shadows, ..default() },
        Transform::IDENTITY,
        // A light's shadow casters are the entities on its layers; it lights the views
        // whose layers meet its own, so the main and the reflection camera as before.
        RenderLayers::from_layers(&[0, SHADOW_LAYER, BODY_LAYER]),
    ));
    if sky.shadows {
        sun.insert(shadow_cascades(sky.shadow_distance, sky.shadow_reach, sky.shadow_cascades));
    }
}

/// How far toward the sun each of the sun's shadow cascades reaches before its casters
/// are squashed onto its near plane, past the nearest of the view's slice (game units).
/// Bevy puts a cascade's near plane at the slice's corner nearest the sun, which with the
/// sun behind the camera lies just in front of the eye: the player's unseen body and a
/// wall behind it would both land on that plane, and the arms' shadows from beyond the
/// body (nif_material.wesl `shadow_past_body`) could not tell them apart. Pushed back by
/// this much the body keeps its true depth [engineering choice: well past the body's
/// reach behind the eye, about 100 units].
pub const SUN_REACH_BACK: f32 = 256.0;

/// Move the near plane of a directional light's cascade `extra` units further toward the
/// light, keeping its far plane: the depth range (reverse Z, near 1, far 0) is stretched
/// over the longer box. Its projection and view-projection change alike.
pub fn reach_toward_light(cascade: &mut bevy::light::cascade::Cascade, extra: f32) {
    // Bevy's projection maps the light-space depth z (0 at the near plane, -range at the
    // far) to r z + 1 with r = 1 / range; wanted is r' (z - extra) + 1 with
    // r' = 1 / (range + extra), so NDC depth d becomes s d + 1 - s - r' extra, s = r' / r.
    let r = cascade.clip_from_cascade.z_axis.z;
    if r <= 0.0 || extra <= 0.0 {
        return;
    }
    let r_new = 1.0 / (1.0 / r + extra);
    let s = r_new / r;
    let remap = Mat4::from_cols(Vec4::X, Vec4::Y, Vec4::new(0.0, 0.0, s, 0.0), Vec4::new(0.0, 0.0, 1.0 - s - r_new * extra, 1.0));
    cascade.clip_from_cascade = remap * cascade.clip_from_cascade;
    cascade.clip_from_world = remap * cascade.clip_from_world;
}

/// Stretch the sun's shadow cascades [`SUN_REACH_BACK`] toward the sun (PostUpdate, after
/// Bevy builds them each frame and before their frusta and shadow views are made).
pub fn reach_toward_sun(mut suns: Query<&mut bevy::light::Cascades, With<Sun>>) {
    for mut cascades in &mut suns {
        for view in cascades.cascades.values_mut() {
            for cascade in view.iter_mut() {
                reach_toward_light(cascade, SUN_REACH_BACK);
            }
        }
    }
}

/// Drop the sun's casters that cannot shade anything a cascade covers. Seen from the sun,
/// a caster shades a receiver only if it covers it, and a cascade's receivers all lie in
/// the slice of the camera's view it was fitted to; Bevy draws everything in the square
/// around that slice's outline (`bevy_light` `calculate_cascade`), so the square's corners
/// hold casters for nothing. Kept: whatever comes within [`SLICE_MARGIN_TEXELS`] of the
/// outline, for the shadow filter's reach.
#[allow(clippy::type_complexity)]
pub fn sun_casters_over_slice(
    mut suns: Query<(&mut bevy::camera::visibility::CascadesVisibleEntities, &bevy::light::Cascades, &CascadeShadowConfig, &GlobalTransform), With<Sun>>,
    views: Query<(&GlobalTransform, &Projection)>,
    casters: Query<(&bevy::camera::primitives::Aabb, &GlobalTransform)>,
) {
    for (mut visible, cascades, config, light) in &mut suns {
        let light_from_world = Mat3::from_quat(light.rotation()).transpose();
        let near_bounds = std::iter::once(config.minimum_distance).chain(config.bounds.iter().map(|b| b * (1.0 - config.overlap_proportion)));
        let bounds: Vec<(f32, f32)> = near_bounds.zip(config.bounds.iter().copied()).collect();
        for (view, lists) in visible.entities.iter_mut() {
            let (Ok((at, projection)), Some(cs)) = (views.get(*view), cascades.cascades.get(view)) else { continue };
            let world_from_view = at.affine();
            for ((list, c), (near, far)) in lists.iter_mut().zip(cs).zip(&bounds) {
                let outline = hull(projection.get_frustum_corners(-near, -far).map(|p| (light_from_world * Vec3::from(world_from_view.transform_point3a(p))).truncate()));
                let margin = SLICE_MARGIN_TEXELS * c.texel_size;
                list.entities.retain(|e| {
                    let Ok((aabb, at)) = casters.get(*e) else { return true };
                    let place = at.affine();
                    let centre = (light_from_world * Vec3::from(place.transform_point3a(aabb.center))).truncate();
                    let m = light_from_world * Mat3::from(place.matrix3);
                    let half = Vec3::from(aabb.half_extents);
                    let reach = Vec2::new(m.row(0).abs().dot(half), m.row(1).abs().dot(half));
                    overlaps(&outline, centre, reach + margin)
                });
            }
        }
    }
}

/// How far past a slice's outline a caster still counts, in the cascade's shadow-map
/// texels [engineering choice: wider than the shadow filter's reach, a few texels].
const SLICE_MARGIN_TEXELS: f32 = 8.0;

/// The convex hull of `points`, counter-clockwise (monotone chain).
fn hull<const N: usize>(mut points: [Vec2; N]) -> Vec<Vec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let mut out: Vec<Vec2> = Vec::with_capacity(2 * N);
    for pass in 0..2 {
        let start = out.len();
        for i in 0..N {
            let p = if pass == 0 { points[i] } else { points[N - 1 - i] };
            while out.len() >= start + 2 && (out[out.len() - 1] - out[out.len() - 2]).perp_dot(p - out[out.len() - 2]) <= 0.0 {
                out.pop();
            }
            out.push(p);
        }
        out.pop();
    }
    out
}

/// Whether the rectangle `centre` ± `reach` overlaps the convex polygon `outline`
/// (counter-clockwise): separated only along one of the outline's edges (the rectangle's
/// own axes are the cascade's square, which Bevy has tested already).
fn overlaps(outline: &[Vec2], centre: Vec2, reach: Vec2) -> bool {
    (0..outline.len()).all(|i| {
        let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
        let out = -(b - a).perp();
        let n = out.normalize_or_zero();
        n.dot(centre - a) - n.abs().dot(reach) <= 0.0
    })
}

/// The sun's shadow cascades, in game units: the first covers the area right around the
/// camera (a twelfth of `distance`), the last ends at `distance` × `reach`.
pub(crate) fn shadow_cascades(distance: f32, reach: f32, count: usize) -> CascadeShadowConfig {
    CascadeShadowConfigBuilder {
        num_cascades: count.max(1),
        minimum_distance: 5.0,
        first_cascade_far_bound: (distance / 12.0).max(200.0),
        maximum_distance: distance * reach.max(1.0),
        overlap_proportion: 0.2,
    }
    .build()
}

/// Height of the sun's direction above the horizon (the sine of its elevation; below 0
/// the directional light is the moon, from the opposite side) and the share of its
/// strength the light has at that height (it fades out within [`HORIZON_FADE`]; [`update_sky`]).
pub fn sun_height_and_fade(sky: &SkyState) -> (f32, f32) {
    let y = (game_to_bevy().rotation * Vec3::from_array(sun_direction(&sky.climate, sky.hour, SUN_SOUTH))).y;
    (y, (y.abs() / HORIZON_FADE.sin()).min(1.0))
}

/// Switch the sun's shadows on or off now (the settings page); they reach
/// [`SkyState::shadow_distance`].
pub fn set_shadows(world: &mut World, on: bool) {
    let distance = {
        let mut sky = world.resource_mut::<SkyState>();
        sky.shadows = on;
        (sky.shadow_distance, sky.shadow_reach, sky.shadow_cascades)
    };
    let mut suns = world.query_filtered::<(Entity, &mut DirectionalLight), With<Sun>>();
    let lit: Vec<Entity> = suns
        .iter_mut(world)
        .map(|(e, mut light)| {
            light.shadow_maps_enabled = on;
            e
        })
        .collect();
    if on {
        for e in lit {
            world.entity_mut(e).insert(shadow_cascades(distance.0, distance.1, distance.2));
        }
    }
}

/// Cloud layers follow the camera and drift with their ONAM speeds; the current set shows
/// the current weather's textures and colours at the fade's alpha, the other set the
/// previous weather's at the remaining alpha.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_clouds(
    sky: Res<SkyState>,
    time: Res<Time>,
    data: Res<GameData>,
    mut textures: ResMut<TextureCache>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    cam: Query<&Transform, (With<FlyCam>, Without<CloudLayer>)>,
    mut clouds: Query<(&mut CloudLayer, &MeshMaterial3d<SkyMaterial>, &mut Transform, &mut Visibility), Without<FlyCam>>,
    mut logged: Local<Option<u32>>,
) {
    let Ok(cam_t) = cam.single() else { return };
    let secs = time.elapsed_secs_f64();
    // Set 0 shows the current weather while no fade runs; during a fade the set whose
    // texture already shows the new weather keeps it.
    for (mut cl, m, mut t, mut vis) in &mut clouds {
        t.translation = cam_t.translation;
        let (w, alpha) = match (&sky.prev, cl.set) {
            _ if sky.interior.is_some() => (None, 0.0),
            (None, 0) => (Some(&sky.weather), 1.0),
            (None, _) => (None, 0.0),
            (Some(_), 0) => (Some(&sky.weather), sky.fade),
            (Some(p), _) => (Some(p), 1.0 - sky.fade),
        };
        let Some(w) = w else {
            *vis = Visibility::Hidden;
            continue;
        };
        let new_texture = cl.shows != Some(w.form_id);
        let c = sky.cloud_tint(w.cloud_color(&sky.climate, cl.layer, sky.hour));
        let color = LinearRgba::new(c[0], c[1], c[2], alpha);
        // Each set scrolls at its own weather's ONAM speeds, so a fade blends the two
        // weathers' layers each moving at its own rate (fnv_world::sky::Weather::cloud_scroll).
        let offset = w.cloud_offset(cl.layer, secs);
        // The material is written only when it shows differently: a new texture, a colour
        // moved by more than a thousandth, or a scroll of more than 1/2048 of the texture
        // (under a texel; the scroll is a few thousandths of a repeat a second). Written
        // every frame, each cloud layer's material was made again on the GPU every frame,
        // GPU memory made and freed behind the night stalls (#897).
        let Some(was) = materials.get(&m.0) else { continue };
        let old = was.base.base_color.to_linear();
        let moved = (was.base.uv_transform.translation.x - offset).rem_euclid(1.0);
        let unchanged = !new_texture
            && [old.red - color.red, old.green - color.green, old.blue - color.blue, old.alpha - color.alpha].iter().all(|d| d.abs() <= 1e-3)
            && moved.min(1.0 - moved) <= 1.0 / 2048.0;
        let mut textured = was.base.base_color_texture.is_some();
        if !unchanged {
            let Some(mut mat) = materials.get_mut(&m.0) else { continue };
            if new_texture {
                mat.base.base_color_texture =
                    w.cloud_textures[cl.layer].as_deref().and_then(|p| textures.get(p, true, &data, &mut images, None));
                cl.shows = Some(w.form_id);
            }
            mat.base.base_color = Color::LinearRgba(color);
            mat.base.uv_transform = Affine2::from_translation(Vec2::new(offset, 0.0));
            textured = mat.base.base_color_texture.is_some();
        }
        *vis = if textured && alpha > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
        if cl.set == 0 && cl.layer == 0 && *logged != Some(w.form_id) {
            *logged = Some(w.form_id);
            let rates = [0, 1, 2, 3].map(|i| format!("{:.5}", w.cloud_scroll(i)));
            info!(
                "clouds: {} layer speeds {:?} (ONAM), scrolling {} texture repeats/s",
                w.edid.as_deref().unwrap_or("?"),
                w.cloud_speeds,
                rates.join(", ")
            );
        }
    }
}

/// Stars and moon follow the camera; their visibility, colour and the moon's phase and
/// position follow the hour.
#[allow(clippy::type_complexity)]
pub fn update_night_sky(
    sky: Res<SkyState>,
    cam: Query<&Transform, (With<FlyCam>, Without<Stars>, Without<MoonDisc>)>,
    mut stars: Query<(&mut Transform, &MeshMaterial3d<SkyMaterial>, &mut Visibility), (With<Stars>, Without<FlyCam>, Without<MoonDisc>)>,
    mut moon: Query<(&mut Transform, &MeshMaterial3d<SkyMaterial>, &mut MoonDisc), (Without<FlyCam>, Without<Stars>)>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut last: Local<Option<(f32, f32)>>,
) {
    let Ok(cam_t) = cam.single() else { return };
    let cam_pos = cam_t.translation;
    let indoors = sky.interior.is_some();
    let sun_dir = game_to_bevy().rotation * Vec3::from_array(sun_direction(&sky.climate, sky.hour, SUN_SOUTH));
    let (star_vis, moon_vis) = if indoors { (0.0, 0.0) } else { sky.night_visibility() };
    let refresh = *last != Some((sky.hour, sky.days_passed));
    *last = Some((sky.hour, sky.days_passed));
    for (mut t, m, mut vis) in &mut stars {
        t.translation = cam_pos;
        *vis = if star_vis > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
        if refresh {
            let c = sky.color(ColorType::Stars);
            let v = star_vis * STAR_STRENGTH;
            let color = Color::LinearRgba(LinearRgba::new(c[0] * v, c[1] * v, c[2] * v, 1.0));
            // Read first: taking the material mutably marks it changed, and by day (the
            // stars at 0) every half game minute wrote the same black again.
            if materials.get(&m.0).is_some_and(|mat| mat.base.base_color != color) {
                if let Some(mut mat) = materials.get_mut(&m.0) {
                    mat.base.base_color = color;
                }
            }
        }
    }
    for (mut t, m, mut disc) in &mut moon {
        let moon_dir = -sun_dir;
        if indoors || moon_vis <= 0.0 || moon_dir.y < -0.1 {
            t.scale = Vec3::ZERO;
            continue;
        }
        t.scale = Vec3::ONE;
        t.translation = cam_pos + moon_dir * SUN_DISTANCE;
        t.look_at(cam_pos, Vec3::Y);
        let phase = sky.moon_phase();
        if refresh || disc.shown != Some(phase) {
            let color = Color::LinearRgba(LinearRgba::new(1.0, 1.0, 1.0, moon_vis));
            // Read first, as the stars.
            if materials.get(&m.0).is_some_and(|mat| mat.base.base_color_texture != disc.phases[phase] || mat.base.base_color != color) {
                if let Some(mut mat) = materials.get_mut(&m.0) {
                    mat.base.base_color_texture = disc.phases[phase].clone();
                    mat.base.base_color = color;
                }
            }
            disc.shown = Some(phase);
        }
    }
}

/// The dome mesh last coloured and the sky colours it was coloured with.
type DomeShown = (AssetId<Mesh>, [f32; 10]);

/// Whether the dome mesh `mesh` needs colouring for the sky colours `now`: a new dome (a
/// worldspace change spawns one whose vertex colours are still the atmosphere's weights,
/// drawn as a green-to-red sky, #1395) or colours that moved noticeably since `shown`.
fn dome_needs_colour(shown: Option<DomeShown>, mesh: AssetId<Mesh>, now: [f32; 10]) -> bool {
    shown.is_none_or(|(was_mesh, was)| was_mesh != mesh || was.iter().zip(now).any(|(a, b)| (a - b).abs() > 1e-3))
}

/// Colour of an atmosphere vertex at `elevation` (degrees) with sky-upper weight `up_w`
/// (the vertex colour's blue), linear RGB. Mixed as display (sRGB) values: sky-upper ×
/// `up_w` plus horizon × `horizon_weight(elevation, twilight)`; below the horizon the
/// colour turns to sky-lower by 10° down [hypothesis fitted to screenshots of the original
/// game, docs/spec-notes/world.md §6 "Sky colours": the vertex colours' green channel, read
/// as the horizon weight, peaks 10-15° up and gives the horizon colour far too much sky].
pub(crate) fn dome_color(elevation: f32, up_w: f32, twilight: f32, lower: [f32; 3], horizon: [f32; 3], upper: [f32; 3]) -> [f32; 3] {
    use crate::lodgen::{linear_to_srgb as d, srgb_to_linear as l};
    let a = horizon_weight(elevation, twilight);
    let below = smoothstep(0.0, -10.0, elevation);
    std::array::from_fn(|i| {
        let above = up_w * d(upper[i]) + a * d(horizon[i]);
        l(above + (d(lower[i]) - above) * below)
    })
}

/// Weight of the horizon colour at `elevation` (degrees): 1 at the horizon, about 0.5 at
/// 15°, 0.3 at 25° at dusk [hypothesis fitted to the dusk reference frame, where the sky
/// colours are graded without further change; world.md §6]. The wide part (the floor,
/// `HORIZON_FLOOR`) is scaled by `twilight` (`SkyState::twilight`, 1 while the sun is up,
/// 0 in full night): the original's night sky keeps under a third of it above 25°
/// [hypothesis fitted 2026-10-23 to clear Mojave frames at 19:06, 20:30 and 23:00, world.md
/// §6 "Sky colours"].
pub(crate) fn horizon_weight(elevation: f32, twilight: f32) -> f32 {
    let e = elevation.max(0.0);
    let (near, far) = (HORIZON_NEAR_DEG, HORIZON_FAR_DEG);
    let fl = HORIZON_FLOOR;
    (1.0 - fl) * (-(e / near).powi(2)).exp() + fl * twilight.clamp(0.0, 1.0) * (-(e / far).powi(2)).exp()
}

const HORIZON_NEAR_DEG: f32 = 14.0;
const HORIZON_FAR_DEG: f32 = 60.0;
const HORIZON_FLOOR: f32 = 0.28;
/// Sun elevation (degrees) at which the horizon colour's floor is gone (see `twilight`).
const TWILIGHT_END_DEG: f32 = -18.0;

/// Fitted daylight factors (see `SkyLight`).
const SKY_DIM: f32 = 0.6;
const SKY_GLOW: f32 = 0.05;
/// Share of the sunlight hue in the clouds' colour in full daylight (see `cloud_tint`).
const CLOUD_SUN: f32 = 0.5;
/// Share of the sunlight hue in the fog's colour in full daylight (see `fog_color`)
/// [hypothesis, fitted 2026-10-21]: 1.0 took the mean colour error of the
/// spring_mountains_east reference frame from 0.077 to 0.013 and of hoover_ridge_lake from
/// 0.048 to 0.011, but lowered black_mountain_north's SSIM from 0.602 to 0.5996, under the
/// 0.6 threshold; 0.5 leaves 0.046 and 0.030 and keeps it at 0.60 (world.md §6).
const FOG_SUN: f32 = 0.5;
/// Sun elevation (degrees) at which daylight reaches full strength.
const DAYLIGHT_DEG: f32 = 40.0;

fn to_color(c: [f32; 3]) -> Color {
    Color::LinearRgba(LinearRgba::rgb(c[0], c[1], c[2]))
}

/// The explorer's `[` and `]`: the hour back or on (a quarter with left Shift). Not run
/// with `--play`: there they are the radio's stations, and the hour moved here became the
/// game's hour, schedules and timers shifted with it [#1988].
pub fn sky_keys(input: Res<ButtonInput<KeyCode>>, mut sky: ResMut<SkyState>) {
    let step = if input.pressed(KeyCode::ShiftLeft) { 0.25 } else { 1.0 };
    if input.just_pressed(KeyCode::BracketRight) {
        let h = sky.hour + step;
        sky.set_hour(h, true);
    }
    if input.just_pressed(KeyCode::BracketLeft) {
        let h = sky.hour - step;
        sky.set_hour(h, true);
    }
}

/// The camera's far plane outdoors (and indoors without a clip distance), game units.
pub const OUTDOOR_FAR: f32 = 400_000.0;

/// Indoors the camera sees no farther than the cell's Fog Clip Distance (CELL XCLL, or
/// its lighting template's) [documented: GECK wiki SetClipDist, "overriding far clip
/// distance in interior cells"; GetInteriorLightingTraitNumeric "Fog clip distance";
/// docs/spec-notes/world.md §6 "Interior clip distance"]; 0 is no clip [hypothesis: 43
/// interiors store 0]. Outdoors, [`OUTDOOR_FAR`].
pub fn interior_clip(sky: Res<SkyState>, mut cam: Query<&mut Projection, With<FlyCam>>, mut last: Local<Option<f32>>) {
    let far = sky.interior.as_ref().map(|l| l.fog_clip_dist).filter(|&c| c > 0.0).unwrap_or(OUTDOOR_FAR);
    if *last == Some(far) {
        return;
    }
    for mut p in &mut cam {
        if let Projection::Perspective(pp) = &mut *p {
            pp.far = far;
            *last = Some(far);
            info!("sky: the camera's far plane at {far:.0} units{}", if sky.interior.is_some() && far < OUTDOOR_FAR { " (the cell's fog clip distance)" } else { "" });
        }
    }
}

/// The sky's colours now, for GetCurrentSkyColor (`fnv_game::nvse_jg9`); none in an
/// interior, which shows no sky.
pub fn tell_sky_colors(sky: Res<SkyState>, mut script: Option<ResMut<nvf_play::script::ScriptState>>) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    rt.game.other.jg9.sky_colors = sky.interior.is_none().then(|| sky.all_color_bytes());
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_sky(
    mut sky: ResMut<SkyState>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    cam: Query<(&Transform, &mut DistanceFog), (With<FlyCam>, Without<SkyDome>, Without<Sun>, Without<CloudLayer>)>,
    mut dome: Query<(&Mesh3d, &SkyDome, &mut Transform), (Without<FlyCam>, Without<Sun>, Without<CloudLayer>)>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), (With<Sun>, Without<FlyCam>, Without<SkyDome>, Without<CloudLayer>)>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut flashed: Local<bool>,
    mut dome_shown: Local<Option<DomeShown>>,
    screen: Option<Res<crate::imagespace::ScreenModifiers>>,
) {
    let mut cam = cam;
    // What the image space modifiers on the screen do to the Sunlight Dimmer and the
    // Emissive Mult (image-modifiers.md §11), on top of the image space's.
    let ratio = |i: usize| screen.as_deref().map_or(1.0, |s| s.hdr_ratio(i));
    let (sun_mod, emissive_mod) = (ratio(fnv_world::imagespace::HDR_SUNLIGHT_DIMMER), ratio(fnv_world::imagespace::HDR_EMISSIVE_MULT));
    let Ok((cam_t, mut fog)) = cam.single_mut() else { return };
    let cam_pos = cam_t.translation;
    let to_bevy = game_to_bevy();
    let unit = light_unit();
    if sky.time_scale > 0.0 && sky.clock_running && sky.interior.is_none() {
        sky.clock_seen = true;
        let dt = time.delta_secs() * sky.time_scale;
        let h = sky.hour + dt;
        if h >= 24.0 {
            sky.days_passed += (h / 24.0).floor();
        }
        sky.advance_fade(dt);
        sky.set_hour(h, false);
    }
    let changed = std::mem::take(&mut sky.changed);
    if let Some(l) = sky.interior {
        if !changed {
            return;
        }
        let c = |v: [u8; 3]| to_color(v.map(fnv_world::sky::srgb_to_linear));
        if let Ok((_, _, mut t)) = dome.single_mut() {
            t.scale = Vec3::ZERO;
        }
        clear.0 = c(l.fog_color);
        let shown = |v: [u8; 3]| v.map(|b| b as f32 / 255.0);
        ambient.color = crate::lights::display_ambient(shown(l.ambient));
        ambient.brightness = unit * INTERIOR_AMBIENT_SHARE;
        if let Ok((mut light, mut t)) = sun.single_mut() {
            let from = game_to_bevy().rotation * interior_light_from(l.directional_rotation_xy, l.directional_rotation_z);
            *t = Transform::IDENTITY.looking_to(-from, Vec3::Y);
            light.color = crate::lights::display_color(shown(l.directional));
            light.illuminance = PI * unit * l.directional_fade.clamp(0.0, 1.0) * INTERIOR_DIRECTIONAL_SHARE;
        }
        fog.color = c(l.fog_color);
        fog.falloff = crate::fog::falloff(l.fog_near.max(0.0), l.fog_far, l.fog_power, false, 1.0);
        info!("interior lighting: fog {:.0}..{:.0} power {:.2}", l.fog_near, l.fog_far, l.fog_power);
        return;
    }
    let (h, c) = (sky.hour, &sky.climate);
    let sun_dir_game = Vec3::from_array(sun_direction(c, h, SUN_SOUTH));
    let sun_dir = to_bevy.rotation * sun_dir_game;

    // Follow the camera every frame; clouds drift with their ONAM speeds. The dome is
    // shrunk to nothing in interiors: give it its size back (the sun disc and glare are
    // drawn on it, so after the first interior, e.g. a new game's Doc Mitchell's house,
    // neither showed again).
    if let Ok((_, _, mut t)) = dome.single_mut() {
        t.translation = cam_pos;
        if t.scale == Vec3::ZERO {
            info!("sky: outdoors again, the sky dome (with the sun disc and glare) shown");
        }
        t.scale = sky_transform(cam_pos).scale;
    }
    if !changed {
        return;
    }

    // Daylight on every sky part that takes it (see SkyLight).
    let mut light = sky.light();
    let flash = Vec3::from_array(sky.flash);
    light.glow += flash.extend(0.0);
    // Written only when the light moved noticeably: each write remakes the material's
    // uniform buffer on the GPU, and writing every frame (the sun creeps a little each
    // frame) made and freed GPU memory every frame, part of the night stalls (#897). A
    // thousandth is far below what shows; a lightning flash passes it at once.
    let ids: Vec<_> = materials.ids().collect();
    for id in ids {
        let Some(m) = materials.get(id) else { continue };
        let old = m.extension.light;
        let new = SkyLight { params: light.params.with_y(old.params.y).with_z(old.params.z).with_w(old.params.w), ..light };
        let same = |a: Vec4, b: Vec4| a.abs_diff_eq(b, 1e-3);
        if same(old.params, new.params) && same(old.glow, new.glow) && same(old.sun_dir, new.sun_dir) && same(old.sun, new.sun) && same(old.glare, new.glare) {
            continue;
        }
        if let Some(mut m) = materials.get_mut(id) {
            m.extension.light = new;
        }
    }
    let upper = sky.color(ColorType::SkyUpper);
    let lower = sky.color(ColorType::SkyLower);
    let horizon = sky.color(ColorType::Horizon);
    let twilight = sky.twilight();
    // The dome is coloured again only when its colours moved noticeably (as the sky
    // materials above): a changed mesh is sent to the GPU again, every frame before (#897).
    let shown = [upper[0], upper[1], upper[2], lower[0], lower[1], lower[2], horizon[0], horizon[1], horizon[2], twilight];
    if let Some((m, weights, _)) = dome.single().ok().filter(|(m, _, _)| dome_needs_colour(*dome_shown, m.0.id(), shown)) {
        if dome_shown.is_none_or(|(was, _)| was != m.0.id()) {
            info!("sky: a new dome coloured from {}'s sky colours", sky.weather.edid.as_deref().unwrap_or("the weather"));
        }
        *dome_shown = Some((m.0.id(), shown));
        if let Some(mut mesh) = meshes.get_mut(&m.0) {
            let cols: Vec<[f32; 4]> = weights
                .0
                .iter()
                .map(|wt| {
                    let c = dome_color(wt[0], wt[1], twilight, lower, horizon, upper);
                    [c[0], c[1], c[2], 1.0]
                })
                .collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, cols);
        }
    }
    clear.0 = to_color(lower);

    let sunlight = sky.color(ColorType::Sunlight);
    if let Ok((mut light, mut t)) = sun.single_mut() {
        // Below the horizon the light comes from the opposite side (moonlight colour is
        // the weather's night "sunlight") [hypothesis].
        let from = if sun_dir.y >= 0.0 { sun_dir } else { -sun_dir };
        *t = Transform::IDENTITY.looking_to(-from, Vec3::Y);
        light.color = crate::lights::display_color(sunlight.map(crate::lights::linear_to_display));
        // Fade out while the sun (or moon) is within HORIZON_FADE of the horizon, so the
        // switch between the two does not flip shadows at full strength [interim].
        // Scaled by the image space's Sunlight Dimmer against the Mojave's (the Big MT's
        // sun is about twice it, Dead Money's West Town's under half) [hypothesis].
        light.illuminance = PI * unit * (sun_dir.y.abs() / HORIZON_FADE.sin()).min(1.0) * sky.sun_scale() * sun_mod * if sun_dir.y >= 0.0 { SUN_SHARE } else { MOON_SHARE };
    }
    let amb = sky.color(ColorType::Ambient);
    ambient.color = crate::lights::display_ambient([amb[0] + flash.x, amb[1] + flash.y, amb[2] + flash.z].map(crate::lights::linear_to_display));
    ambient.brightness = unit * AMBIENT_SHARE;

    // No fog: the fog colour's alpha scales the whole effect, so zero turns it off.
    let range = sky.fog_override.map(|(n, f)| (n, f, 1.0)).or_else(|| sky.fog());
    let (near, far, strength) = range.unwrap_or((0.0, 0.0, 0.0));
    fog.color = to_color(sky.fog_color()).with_alpha(strength);
    let power = sky.fog_power();
    fog.falloff = crate::fog::falloff(near.max(0.0), far, power, true, sky.emissive_scale() * emissive_mod);
    // A lightning flash changes the light each frame it lasts: not worth a line each.
    let flashing = std::mem::replace(&mut *flashed, flash != Vec3::ZERO);
    if sky.time_scale <= 0.0 && flash == Vec3::ZERO && !flashing {
        let desc = match range {
            Some(_) if strength < 1.0 => format!("{near:.0}..{far:.0} power {power:.2} x{strength:.2}"),
            Some(_) => format!("{near:.0}..{far:.0} power {power:.2} ({:.0} % at 1,000 units)", 100.0 * crate::fog::share(near.max(0.0), far, power, 1_000.0)),
            None => "none".into(),
        };
        let g = sky.cinematic();
        info!(
            "sky: {}  fog {desc}  sun x{:.2}  grade: saturation {:.2} contrast {:.2} around {:.2} brightness {:.2} tint ({:.2}, {:.2}, {:.2}) x{:.2}",
            sky.describe(),
            sky.sun_scale(),
            g.saturation,
            g.contrast,
            g.contrast_avg_lum,
            g.brightness,
            g.tint[0],
            g.tint[1],
            g.tint[2],
            g.tint_amount
        );
    }
}

pub fn initial_fog() -> DistanceFog {
    DistanceFog { color: Color::WHITE, falloff: FogFalloff::Linear { start: 8_000.0, end: 20_000.0 }, ..default() }
}

/// The direction towards an interior's directional light, in game axes, from its XCLL
/// (or lighting template) rotation in degrees: `xy` an azimuth about z, `z` a pitch,
/// together the direction the light shines in (270 straight down) [hypothesis, world.md
/// §8 "Interior cells"; observer-checks 101].
pub fn interior_light_from(xy: i32, z: i32) -> Vec3 {
    let (az, el) = ((xy as f32).to_radians(), (z as f32).to_radians());
    -Vec3::new(az.sin() * el.cos(), az.cos() * el.cos(), el.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The common template pitch (z = 270) puts the light straight overhead, and the Gas
    /// Station's (xy 180, z 250, #1645) puts it high above, not below the floor.
    #[test]
    fn interior_light_comes_from_above() {
        let overhead = interior_light_from(0, 270);
        assert!((overhead - Vec3::Z).length() < 1e-5, "{overhead:?}");
        let gas_station = interior_light_from(180, 250);
        assert!(gas_station.z > 0.9, "{gas_station:?}");
        assert!((gas_station.length() - 1.0).abs() < 1e-5);
        // In Bevy's axes (y up) the light shines downwards.
        let from = game_to_bevy().rotation * gas_station;
        let shines = Transform::IDENTITY.looking_to(-from, Vec3::Y).forward();
        assert!(shines.y < -0.9, "{shines:?}");
    }

    /// The rolls made once a sky that started on `sky_day` follows a restored game on day
    /// 25, half a game minute on from the save's noon, with a saved weather its list holds
    /// and a volatility that rolls at every whole hour passed.
    fn rolls_following_the_restored_game(sky_day: f32) -> u64 {
        const SAVED: u32 = 1;
        let weather = |id| Weather { form_id: id, ..Default::default() };
        let choice = |id| Choice { weather: weather(id), chance: 100, global: None };
        let mut sky = SkyState {
            climate: Climate { volatility: 255, ..Default::default() },
            weather: weather(SAVED),
            hour: 12.0,
            fog_override: None,
            interior: None,
            interior_grade: None,
            shadows: false,
            shadow_distance: DEFAULT_SHADOW_DISTANCE,
            shadow_reach: 1.0,
            shadow_cascades: DEFAULT_SHADOW_CASCADES,
            grade: true,
            time_scale: 0.0,
            clock_running: false,
            days_passed: sky_day,
            prev: None,
            fade: 1.0,
            choices: vec![choice(SAVED), choice(2)],
            image_space: None,
            adapters: HashMap::new(),
            changed: true,
            script: None,
            pinned: false,
            regions: Vec::new(),
            source: None,
            placed: false,
            clock_seen: false,
            rolled_for: None,
            pick: SAVED,
            rolls: 0,
            rng: DEFAULT_WEATHER_SEED,
            hour_index: None,
            live_globals: HashMap::new(),
            resume: Some(SAVED),
            flash: [0.0; 3],
            script_wind: None,
            moon_textures: Default::default(),
            lightning_request: false,
        };
        sky.place(Some(None));
        assert_eq!((sky.rolls, sky.pick), (0, SAVED), "the first look keeps the saved weather");
        sky.follow_game_clock(12.0 + 1.0 / 60.0, 25.375);
        assert_eq!(sky.days_passed, 25.0);
        sky.rolls
    }

    #[test]
    fn a_sky_on_the_games_day_rolls_nothing_following_it() {
        assert_eq!(rolls_following_the_restored_game(25.0), 0);
        // On the plugins' day it counts the days between as hours passed (#1607).
        assert_eq!(rolls_following_the_restored_game(5.0), 1);
        // Ahead of the game: a step back, no roll.
        assert_eq!(rolls_following_the_restored_game(30.0), 0);
    }

    const HELD: u32 = 9;
    const LISTED: u32 = 1;
    const GATED: u32 = 2;
    const GATE: u32 = 77;

    /// A sky as the start makes it for a save showing `HELD`, which no list holds; the
    /// list's other entry waits for global `GATE`.
    fn sky_started_on_held() -> SkyState {
        let weather = |id| Weather { form_id: id, edid: Some(format!("W{id}")), ..Default::default() };
        let gate = Gate { id: GATE, edid: None, stored: Some(0.0) };
        SkyState {
            climate: Climate::default(),
            weather: weather(HELD),
            hour: 12.0,
            fog_override: None,
            interior: None,
            interior_grade: None,
            shadows: false,
            shadow_distance: DEFAULT_SHADOW_DISTANCE,
            shadow_reach: 1.0,
            shadow_cascades: DEFAULT_SHADOW_CASCADES,
            grade: true,
            time_scale: 0.0,
            clock_running: false,
            days_passed: 25.0,
            prev: None,
            fade: 1.0,
            choices: vec![Choice { weather: weather(LISTED), chance: 100, global: None }, Choice { weather: weather(GATED), chance: 100, global: Some(gate) }],
            image_space: None,
            adapters: HashMap::new(),
            changed: true,
            script: None,
            pinned: false,
            regions: Vec::new(),
            source: None,
            placed: false,
            clock_seen: false,
            rolled_for: None,
            pick: HELD,
            rolls: 0,
            rng: DEFAULT_WEATHER_SEED,
            hour_index: None,
            live_globals: HashMap::new(),
            resume: Some(HELD),
            flash: [0.0; 3],
            script_wind: None,
            moon_textures: Default::default(),
            lightning_request: false,
        }
    }

    fn saved_held() -> serde_json::Value {
        serde_json::json!({"weather": format!("W{HELD}"), "held": true, "rng": "123"})
    }

    #[test]
    fn a_saved_hold_outlasts_the_first_look_and_what_follows() {
        let mut sky = sky_started_on_held();
        sky.restore_saved(&saved_held());
        sky.place(Some(None));
        // The climate still rolls under the hold; what it picks waits for a release.
        assert_eq!((sky.weather.form_id, sky.rolls, sky.pick), (HELD, 1, LISTED));
        let mut rng = 123;
        next_unit(&mut rng);
        assert_eq!(sky.rng, rng, "the roll comes from the saved random state");
        sky.follow_game_clock(12.0 + 1.0 / 60.0, 25.0);
        assert_eq!(sky.weather.form_id, HELD);
        sky.set_live_global(GATE, 1.0);
        assert_eq!((sky.weather.form_id, sky.rolls, sky.pick), (HELD, 2, GATED));
        assert!(sky.prev.is_none());
    }

    #[test]
    fn a_release_after_the_load_stands_and_the_next_roll_takes_over() {
        let mut sky = sky_started_on_held();
        sky.restore_saved(&saved_held());
        sky.place(Some(None));
        // A load script releases it before the engine's part of the save is put back.
        sky.release_script_weather();
        assert_eq!(sky.weather.form_id, HELD, "until the next roll, as any release");
        let rng = sky.rng;
        let mut world = World::new();
        world.insert_resource(sky);
        (SAVE_HOOK.load)(&mut world, &saved_held());
        let mut sky = world.remove_resource::<SkyState>().unwrap();
        assert_eq!(sky.rng, rng, "the hook does not put the random state back");
        sky.set_live_global(GATE, 1.0);
        assert_eq!(sky.weather.form_id, GATED, "the hook did not hold it again");
        assert!(sky.script.is_none());
    }

    #[test]
    fn only_the_saved_held_weather_is_held() {
        // Not held in the save, and held in a save whose weather the sky could not start
        // on: the first look's roll shows.
        for saved in [serde_json::json!({"weather": format!("W{HELD}"), "held": false}), serde_json::json!({"weather": "Gone", "held": true})] {
            let mut sky = sky_started_on_held();
            sky.restore_saved(&saved);
            sky.place(Some(None));
            assert_eq!(sky.weather.form_id, LISTED, "{saved}");
            assert!(sky.script.is_none());
        }
    }

    #[test]
    fn a_new_dome_is_coloured_even_when_the_sky_colours_stay() {
        // #1395: on a worldspace change the colours were recorded against the old dome in
        // the frame before the new one spawned; the new one kept its raw weights.
        let colours = [0.2, 0.4, 0.5, 0.5, 0.6, 0.6, 0.6, 0.7, 0.7, 1.0];
        let (old, new) = (AssetId::<Mesh>::from(bevy::asset::uuid::Uuid::from_u128(1)), AssetId::<Mesh>::from(bevy::asset::uuid::Uuid::from_u128(2)));
        assert!(dome_needs_colour(None, old, colours));
        assert!(!dome_needs_colour(Some((old, colours)), old, colours));
        assert!(dome_needs_colour(Some((old, colours)), new, colours));
        let mut moved = colours;
        moved[0] += 0.01;
        assert!(dome_needs_colour(Some((old, colours)), old, moved));
    }

    #[test]
    fn a_caster_counts_only_near_the_slice_outline() {
        // A diamond with a point inside it; the hull keeps the four corners.
        let outline = hull([Vec2::new(0.0, -10.0), Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0), Vec2::new(-10.0, 0.0), Vec2::new(1.0, 1.0)]);
        assert_eq!(outline.len(), 4);
        // Inside, and touching an edge.
        assert!(overlaps(&outline, Vec2::ZERO, Vec2::splat(1.0)));
        assert!(overlaps(&outline, Vec2::new(6.0, 6.0), Vec2::splat(1.0)));
        // In the diamond's bounding square but past its edge: Bevy would draw it, nothing
        // in the slice is under it.
        assert!(!overlaps(&outline, Vec2::new(8.0, 8.0), Vec2::splat(1.0)));
        // The same with the margin added to its reach.
        assert!(overlaps(&outline, Vec2::new(8.0, 8.0), Vec2::splat(1.0 + 5.0)));
    }

    #[test]
    fn cascade_reaches_further_toward_the_light() {
        // A light shining straight down; a box 100 units deep whose near plane is at y = 50.
        let range = 100.0;
        let near_y = 50.0;
        let clip_from_cascade = Mat4::from_cols(Vec4::new(0.01, 0.0, 0.0, 0.0), Vec4::new(0.0, 0.01, 0.0, 0.0), Vec4::new(0.0, 0.0, 1.0 / range, 0.0), Vec4::new(0.0, 0.0, 1.0, 1.0));
        // Light space: z points up toward the light, origin on the near plane.
        let cascade_from_world = Mat4::from_cols(Vec4::X, Vec4::new(0.0, 0.0, 1.0, 0.0), Vec4::new(0.0, -1.0, 0.0, 0.0), Vec4::new(0.0, 0.0, -near_y, 1.0));
        let mut cascade = bevy::light::cascade::Cascade {
            world_from_cascade: cascade_from_world.inverse(),
            clip_from_cascade,
            clip_from_world: clip_from_cascade * cascade_from_world,
            texel_size: 1.0,
        };
        let depth = |c: &bevy::light::cascade::Cascade, y: f32| c.clip_from_world.project_point3(Vec3::new(3.0, y, 4.0)).z;
        assert!((depth(&cascade, near_y) - 1.0).abs() < 1e-6);
        assert!(depth(&cascade, near_y - range).abs() < 1e-6);
        reach_toward_light(&mut cascade, 200.0);
        // The far plane stays, the near plane moves 200 units toward the light.
        assert!(depth(&cascade, near_y - range).abs() < 1e-6);
        assert!((depth(&cascade, near_y + 200.0) - 1.0).abs() < 1e-6);
        assert!((depth(&cascade, near_y) - 1.0 / 3.0).abs() < 1e-6);
        // Projection and view-projection agree.
        let both = cascade.clip_from_cascade * cascade_from_world;
        assert!(both.abs_diff_eq(cascade.clip_from_world, 1e-6));
        // Nothing to stretch.
        let before = cascade.clip_from_world;
        reach_toward_light(&mut cascade, 0.0);
        assert_eq!(before, cascade.clip_from_world);
    }

    #[test]
    fn horizon_weight_falls_with_elevation() {
        assert!((horizon_weight(0.0, 1.0) - 1.0).abs() < 1e-6);
        assert!((horizon_weight(-5.0, 1.0) - 1.0).abs() < 1e-6);
        // The dusk fit (world.md §6 "Sky colours"): about 0.86 at 7°, 0.55 at 15°, 0.3 at 25°.
        assert!((horizon_weight(6.8, 1.0) - 0.86).abs() < 0.03);
        assert!((horizon_weight(14.7, 1.0) - 0.55).abs() < 0.05);
        assert!((horizon_weight(26.6, 1.0) - 0.28).abs() < 0.04);
        let mut last = 2.0;
        for e in 0..=90 {
            let w = horizon_weight(e as f32, 1.0);
            assert!(w <= last && w >= 0.0);
            last = w;
        }
    }

    #[test]
    fn night_sky_loses_the_horizon_floor() {
        // Near the horizon the weight is the same day and night; high up only the floor
        // differs, and in full night it is gone.
        assert!((horizon_weight(5.0, 1.0) - horizon_weight(5.0, 0.0)).abs() < 0.3);
        assert!(horizon_weight(30.0, 0.0) < 0.02);
        assert!(horizon_weight(30.0, 1.0) > 0.2);
        assert!(horizon_weight(30.0, 0.5) > horizon_weight(30.0, 0.0) && horizon_weight(30.0, 0.5) < horizon_weight(30.0, 1.0));
    }

    #[test]
    fn dome_mixes_display_values() {
        let (lower, horizon, upper) = ([0.1, 0.2, 0.3], [0.7, 0.8, 0.9], [0.03, 0.06, 0.25]);
        // Well below the horizon: sky-lower.
        let c = dome_color(-20.0, 0.0, 1.0, lower, horizon, upper);
        assert!(c.iter().zip(lower).all(|(a, b)| (a - b).abs() < 1e-4));
        // At the horizon with no upper weight: the horizon colour.
        let c = dome_color(0.0, 0.0, 1.0, lower, horizon, upper);
        assert!(c.iter().zip(horizon).all(|(a, b)| (a - b).abs() < 1e-4));
        // Half of a colour as display values is darker than half in linear light.
        let half = dome_color(90.0, 0.5, 1.0, lower, horizon, upper)[2];
        assert!(half < 0.5 * upper[2] + 0.5 * horizon_weight(90.0, 1.0) * horizon[2]);
    }
}
