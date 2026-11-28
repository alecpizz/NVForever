//! Authored PBR material sets as materials use them (#2981, docs/spec-notes/pbr-materials.md).
//!
//! - The run's switch ([`enabled`]): the settings' Authored materials row, read once at the
//!   start ([`set_enabled`], nvforever `main.rs`) and fixed for the run, so cached models
//!   never mix the two states. Off, nothing here asks for the index (`GameData::pbr_index`)
//!   or reads a file under `textures\pbr\`.
//! - [`find`]: a shape's set by its resolved diffuse key ([`PbrSet`]: authored, or height
//!   only), the members loaded by the exact paths the index names, the scalar file read once
//!   per mount ([`scalars_cached`], `GameData::pbr_scalars`).
//! - [`apply`] / [`restore`]: a set on a material and back, from the legacy look every
//!   eligible shape's material keeps ([`LegacyLook`]); [`retexture`] is the one rule for a
//!   lit copy whose diffuse is replaced later.
//!
//! [design] The rules are NVForever's own (the format is ours); none describes the original.

use crate::data::GameData;
use crate::texture::TextureUsage;
use bevy::pbr::ParallaxMappingMethod;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use fnv_pbr::{Found, Member, PbrIndex, Scalars, SetState};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};

/// The run's state of the Authored materials setting (module notes).
static RUN: OnceLock<bool> = OnceLock::new();

/// Fix the run's state of the setting, before the first material is made. Called once at the
/// start; a later call that disagrees is refused with a warning (the state is the run's).
pub fn set_enabled(on: bool) {
    if RUN.set(on).is_err() && enabled() != on {
        warn!("authored materials: already {} for this run; the setting takes effect at the next start", if enabled() { "on" } else { "off" });
    }
}

/// Whether authored sets shape materials in this run: as [`set_enabled`] fixed it, else on
/// (the default; the viewer never sets it).
pub fn enabled() -> bool {
    *RUN.get_or_init(|| true)
}

/// An authored set found for a shape's diffuse, its maps loaded.
#[derive(Clone, Debug, PartialEq)]
pub struct AuthoredSet {
    /// The set's stem (`textures\pbr\...`, without suffix).
    pub stem: String,
    /// `_orm`: occlusion red, roughness green, metallic blue (linear).
    pub orm: Handle<Image>,
    /// `_n` in the two-channel role, when the set has one and it loaded.
    pub normal: Option<Handle<Image>>,
    /// `_p` as a depth map with its settings ([`Parallax::height`]), when the set has one, it
    /// loaded and its depth is above 0.
    pub height: Option<Parallax>,
    /// The scalar file's values (all `None` without one).
    pub scalars: Scalars,
    /// The base colour gain of the set's metals ([`set_metal_gain`]: the run's override, the
    /// scalar file's `metal_gain`, or its default), carried to the shader by
    /// `NifSurface::metal_gain` ([`apply`]).
    pub metal_gain: f32,
}

/// A height-only set (`_p` without `_orm`) found for a shape's diffuse, its `_p` loaded: the
/// game's own material with the height map, nothing else changed [design,
/// docs/spec-notes/pbr-materials.md "Layout"].
#[derive(Clone, Debug, PartialEq)]
pub struct HeightSet {
    /// The set's stem (`textures\pbr\...`, without suffix).
    pub stem: String,
    /// `_p` as a depth map with its settings ([`Parallax::height`]).
    pub height: Parallax,
}

/// What [`find`] found for a shape's diffuse: the one active set the index's lookup names
/// (`PbrIndex::lookup`, with its first-person rule), by its state.
#[derive(Clone, Debug, PartialEq)]
pub enum PbrSet {
    /// A set with `_orm` (and its `_n`, `_p` and scalars where it has them).
    Authored(AuthoredSet),
    /// A set with `_p` and no `_orm`.
    HeightOnly(HeightSet),
}

impl PbrSet {
    pub fn stem(&self) -> &str {
        match self {
            PbrSet::Authored(s) => &s.stem,
            PbrSet::HeightOnly(s) => &s.stem,
        }
    }

    /// The authored part (`None` for a height-only set).
    pub fn authored(&self) -> Option<&AuthoredSet> {
        match self {
            PbrSet::Authored(s) => Some(s),
            PbrSet::HeightOnly(_) => None,
        }
    }

    /// The height map, from either kind of set.
    pub fn height(&self) -> Option<&Parallax> {
        match self {
            PbrSet::Authored(s) => s.height.as_ref(),
            PbrSet::HeightOnly(s) => Some(&s.height),
        }
    }
}

impl From<AuthoredSet> for PbrSet {
    fn from(set: AuthoredSet) -> PbrSet {
        PbrSet::Authored(set)
    }
}

/// The parallax method a set's height map is drawn with: Bevy's relief method, the march
/// through layers ([`MAX_PARALLAX_LAYERS`]) followed by a binary search of 4 halvings between
/// the last two layers, one read each [design: the main session's pictures of 2026-11-20 on
/// a tiled floor (set `offrmfloor01`, a 512 BC4 `_p`): the occlusion method at 8 layers
/// showed stair-step bands on the patch edges from depth 0.03 on, more layers only made them
/// finer (16, 32), and `relief:4` at 8 layers removed them at 0.03; its frame cost is not
/// measured]. Relief is its own pipeline variant (`RELIEF_MAPPING`, Bevy's
/// `StandardMaterialKey`), so height-mapped materials get their own pipelines. The method
/// switch (`NVF_PBR_PARALLAX_METHOD=occlusion`, [`parallax_method`]) brings back the
/// occlusion method for a comparison.
pub const PARALLAX_METHOD: ParallaxMappingMethod = ParallaxMappingMethod::Relief { max_steps: 4 };

/// The most layers the march takes before the relief search (Bevy's default is 16; at a
/// head-on view it takes one) [design, the review's starting point; no frame-time budget is
/// measured for it].
pub const MAX_PARALLAX_LAYERS: f32 = 8.0;

/// The environment variable of the layer switch ([`parallax_layers`]).
pub const PARALLAX_LAYERS_ENV: &str = "NVF_PBR_PARALLAX_LAYERS";

/// The environment variable of the method switch ([`parallax_method`]).
pub const PARALLAX_METHOD_ENV: &str = "NVF_PBR_PARALLAX_METHOD";

/// The environment variable of the alpha-tested switch ([`parallax_mask`]).
pub const PARALLAX_MASK_ENV: &str = "NVF_PBR_PARALLAX_MASK";

/// The depth a NIF's own height map (texture-set slot 3 with shader flag bit 11, upstream
/// issue #2979; `nif_spawn::game_height_slot`) is drawn at, in UV units: 0.015, half the
/// authored default ([`fnv_pbr::DEFAULT_PARALLAX_DEPTH`]) [hypothesis: how deep the original
/// draws these maps is not observed (docs/handoff/observer-checks.md #265). The GECK wiki
/// calls bit 11's effect "pretty weak" without bit 28 "Parallax_Occlusion" [documented:
/// GECK wiki "NIF Shader Properties"], which no shipped shape sets, and 0.015 looked
/// convincing on a 512 floor map in the authored tests (docs/spec-notes/pbr-materials.md
/// "Height maps: guidance for pack authors"). The shader property's stored `parallax_scale`
/// ("a multiplier for the strength", the same wiki page) and `parallax_max_passes` are not
/// read]. On by default; the value is provisional until that check and a frame-cost
/// measurement on a parallax-heavy view. The depth switch ([`game_parallax_depth`]) changes
/// it for a run.
pub const GAME_PARALLAX_DEPTH: f32 = 0.015;

/// The least view steepness (|Vt.z|, the cosine of the view's angle to the surface normal)
/// the parallax march divides by: the vendored `parallax_mapping.wesl` "NVForever grazing
/// clamp" (engine/vendor/README.md). A shallower view shifts no further than at about 81
/// degrees, and a view along the surface stays finite [design, #2979].
pub const PARALLAX_MIN_STEEPNESS: f32 = 0.15;

/// The environment variable of the game depth switch ([`game_parallax_depth`]).
pub const GAME_PARALLAX_DEPTH_ENV: &str = "NVF_GAME_PARALLAX_DEPTH";

/// The layer counts the layer switch takes.
pub const PARALLAX_LAYERS_RANGE: std::ops::RangeInclusive<u32> = 2..=64;

/// The relief steps the method switch takes (`relief:<steps>`).
pub const RELIEF_STEPS_RANGE: std::ops::RangeInclusive<u32> = 1..=8;

/// The layer switch's text (`NVF_PBR_PARALLAX_LAYERS`) as a layer count: a whole number in
/// [`PARALLAX_LAYERS_RANGE`]; why not otherwise.
pub fn parse_parallax_layers(text: &str) -> Result<f32, String> {
    let t = text.trim();
    let n: u32 = t.parse().map_err(|_| format!("is no whole number of layers ({} to {})", PARALLAX_LAYERS_RANGE.start(), PARALLAX_LAYERS_RANGE.end()))?;
    if !PARALLAX_LAYERS_RANGE.contains(&n) {
        return Err(format!("is outside {} to {} layers", PARALLAX_LAYERS_RANGE.start(), PARALLAX_LAYERS_RANGE.end()));
    }
    Ok(n as f32)
}

/// The method switch's text (`NVF_PBR_PARALLAX_METHOD`, any case): `occlusion` (Bevy's
/// occlusion method: one more read and an interpolation after the march) or
/// `relief:<steps>` (Bevy's relief method: after the march, a binary search between the last
/// two layers of `steps` halvings, one read each; vendored `parallax_mapping.wesl`
/// `parallaxed_uv`, `RELIEF_MAPPING`), the steps in [`RELIEF_STEPS_RANGE`]; why not otherwise.
pub fn parse_parallax_method(text: &str) -> Result<ParallaxMappingMethod, String> {
    let t = text.trim().to_ascii_lowercase();
    let range = format!("relief:{} to relief:{}", RELIEF_STEPS_RANGE.start(), RELIEF_STEPS_RANGE.end());
    if t == "occlusion" {
        return Ok(ParallaxMappingMethod::Occlusion);
    }
    let Some(steps) = t.strip_prefix("relief") else {
        return Err(format!("is neither occlusion nor {range}"));
    };
    let Some(steps) = steps.strip_prefix(':') else {
        return Err(format!("names no relief steps ({range})"));
    };
    let n: u32 = steps.trim().parse().map_err(|_| format!("names no whole number of relief steps ({range})"))?;
    if !RELIEF_STEPS_RANGE.contains(&n) {
        return Err(format!("is outside {range}"));
    }
    Ok(ParallaxMappingMethod::Relief { max_steps: n })
}

/// The alpha-tested switch's text (`NVF_PBR_PARALLAX_MASK`): `1` on, `0` off; why not
/// otherwise.
pub fn parse_parallax_mask(text: &str) -> Result<bool, String> {
    match text.trim() {
        "1" => Ok(true),
        "0" => Ok(false),
        _ => Err("is neither 1 nor 0".to_string()),
    }
}

/// The game depth switch's text (`NVF_GAME_PARALLAX_DEPTH`): a number in the scalar file's
/// range for `parallax_depth` ([`fnv_pbr::PARALLAX_DEPTH_RANGE`], 0 to 0.08), 0 drawing no
/// game height map at all; why not otherwise.
pub fn parse_game_parallax_depth(text: &str) -> Result<f32, String> {
    let range = fnv_pbr::PARALLAX_DEPTH_RANGE;
    let d: f32 = text.trim().parse().map_err(|_| format!("is no number ({} to {})", range.start(), range.end()))?;
    if !d.is_finite() || !range.contains(&d) {
        return Err(format!("is outside {} to {}", range.start(), range.end()));
    }
    Ok(d)
}

/// A method as the switch spells it (`occlusion`, `relief:5`).
pub fn parallax_method_name(method: ParallaxMappingMethod) -> String {
    match method {
        ParallaxMappingMethod::Occlusion => "occlusion".to_string(),
        ParallaxMappingMethod::Relief { max_steps } => format!("relief:{max_steps}"),
    }
}

/// What a development switch's text makes of its value ([`switch_value`]).
#[derive(Clone, Debug, PartialEq)]
pub enum SwitchLog {
    /// Unset or empty: the default, nothing logged.
    Quiet,
    /// Set and taken: the line to log as information.
    Taken(String),
    /// Refused (garbage or out of range): the warning; the default is kept.
    Refused(String),
}

/// A development switch's value from its variable `name`'s text (`None`: unset), parsed by
/// `parse`, and what to log: `said` describes a taken value, `quiet` says a taken value that
/// needs no line (the mask switch's `0`). Refused text keeps `default`.
pub fn switch_value<T: Copy>(
    name: &str,
    text: Option<&str>,
    default: T,
    parse: impl Fn(&str) -> Result<T, String>,
    said: impl Fn(T) -> String,
    quiet: impl Fn(T) -> bool,
) -> (T, SwitchLog) {
    let Some(text) = text.filter(|t| !t.trim().is_empty()) else { return (default, SwitchLog::Quiet) };
    match parse(text) {
        Ok(v) if quiet(v) => (v, SwitchLog::Quiet),
        Ok(v) => (v, SwitchLog::Taken(format!("authored materials: {name}={}: {}", text.trim(), said(v)))),
        Err(why) => (default, SwitchLog::Refused(format!("authored materials: {name}={text} {why}; the default is kept"))),
    }
}

/// Read a development switch from the environment ([`switch_value`]) and log its line.
fn read_switch<T: Copy>(name: &str, default: T, parse: impl Fn(&str) -> Result<T, String>, said: impl Fn(T) -> String, quiet: impl Fn(T) -> bool) -> T {
    let text = std::env::var_os(name).map(|t| t.to_string_lossy().into_owned());
    let (value, log) = switch_value(name, text.as_deref(), default, parse, said, quiet);
    match log {
        SwitchLog::Quiet => {}
        SwitchLog::Taken(line) => info!("{line}"),
        SwitchLog::Refused(line) => warn!("{line}"),
    }
    value
}

/// The most layers a set's height map is marched with this run: [`MAX_PARALLAX_LAYERS`], or
/// the layer switch's count (`NVF_PBR_PARALLAX_LAYERS=<2..64>`, read once at the start; a
/// development aid for comparing pictures and frame times without a rebuild) [design].
pub fn parallax_layers() -> f32 {
    static V: OnceLock<f32> = OnceLock::new();
    *V.get_or_init(|| {
        read_switch(PARALLAX_LAYERS_ENV, MAX_PARALLAX_LAYERS, parse_parallax_layers, |n| format!("height maps are marched with at most {n} layers (default {MAX_PARALLAX_LAYERS})"), |_| false)
    })
}

/// The method a set's height map is drawn with this run: [`PARALLAX_METHOD`], or the method
/// switch's (`NVF_PBR_PARALLAX_METHOD=occlusion|relief:<1..8>`, read once at the start; a
/// development aid like [`parallax_layers`]) [design].
pub fn parallax_method() -> ParallaxMappingMethod {
    static V: OnceLock<ParallaxMappingMethod> = OnceLock::new();
    *V.get_or_init(|| {
        read_switch(
            PARALLAX_METHOD_ENV,
            PARALLAX_METHOD,
            parse_parallax_method,
            |m| format!("height maps are drawn with the {} method (default {})", parallax_method_name(m), parallax_method_name(PARALLAX_METHOD)),
            |_| false,
        )
    })
}

/// Whether alpha-tested shapes may take a height map this run (the alpha-tested switch,
/// `NVF_PBR_PARALLAX_MASK=1`, read once at the start; off by default, a development aid):
/// [`height_alpha`]. Their cut is tested at the unshifted coordinates in every pass, the main
/// pass included (nif_material.wesl `NVF_PBR_PARALLAX_MASK`), so it agrees with the depth-only
/// passes; the colour inside it is the shifted texel's [design].
pub fn parallax_mask() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| {
        read_switch(
            PARALLAX_MASK_ENV,
            false,
            parse_parallax_mask,
            |_| "alpha-tested shapes may take a height map; their cut stays at the unshifted coordinates in every pass".to_string(),
            |on| !on,
        )
    })
}

/// The depth the game's own height maps are drawn at this run: [`GAME_PARALLAX_DEPTH`], or
/// the game depth switch's (`NVF_GAME_PARALLAX_DEPTH=<0..0.08>`, read once at the start; a
/// development aid for fitting the depth to the original by picture; 0 draws them flat and
/// loads none). The method and the layers are the authored height maps' switches
/// ([`parallax_method`], [`parallax_layers`]) [design].
pub fn game_parallax_depth() -> f32 {
    static V: OnceLock<f32> = OnceLock::new();
    *V.get_or_init(|| {
        read_switch(
            GAME_PARALLAX_DEPTH_ENV,
            GAME_PARALLAX_DEPTH,
            parse_game_parallax_depth,
            |d| {
                if d > 0.0 {
                    format!("the game's own height maps (slot 3, shader flag 11) are drawn at depth {d} (default {GAME_PARALLAX_DEPTH})")
                } else {
                    "the game's own height maps (slot 3, shader flag 11) are not drawn".to_string()
                }
            },
            |_| false,
        )
    })
}

/// The parallax fields of a material, as Bevy's standard material has them: the depth map,
/// its depth, the method and the most layers. What a set's `_p` puts on a material
/// ([`Parallax::height`]) and what the legacy look keeps to put back ([`LegacyLook::parallax`]).
#[derive(Clone, Debug, PartialEq, Reflect)]
pub struct Parallax {
    pub depth_map: Option<Handle<Image>>,
    pub depth_scale: f32,
    pub method: ParallaxMappingMethod,
    pub max_layers: f32,
}

impl Default for Parallax {
    /// The fields of a material without a depth map (Bevy's defaults).
    fn default() -> Parallax {
        Parallax::of(&StandardMaterial::default())
    }
}

impl Parallax {
    /// A set's height map: `image` (the `_p`, linear) as the depth map at `depth_scale` UV
    /// units, the run's method and layers ([`parallax_method`], [`parallax_layers`]:
    /// [`PARALLAX_METHOD`], [`MAX_PARALLAX_LAYERS`] unless a development switch is set).
    pub fn height(image: Handle<Image>, depth_scale: f32) -> Parallax {
        Parallax { depth_map: Some(image), depth_scale, method: parallax_method(), max_layers: parallax_layers() }
    }

    /// The march's shift at full depth (layers x step) for a tangent-space view vector `vt`, as
    /// the vendored `parallax_mapping.wesl` `parallaxed_uv` computes it with its "NVForever
    /// grazing clamp": `depth_scale x vt.xy x (1, -1) / max(|vt.z|, PARALLAX_MIN_STEEPNESS)`.
    /// A CPU mirror for tests; the GPU does not call it [design, #2979].
    pub fn full_depth_shift(depth_scale: f32, vt: Vec3) -> Vec2 {
        depth_scale * vt.truncate() * Vec2::new(1.0, -1.0) / vt.z.abs().max(PARALLAX_MIN_STEEPNESS)
    }

    /// A NIF's own height map (#2979): `image` (texture-set slot 3, linear) at the run's game
    /// depth ([`game_parallax_depth`]), with the method and layers a set's height map takes.
    /// The file is height as a set's `_p` is (brighter is higher [observed:
    /// docs/spec-notes/nif.md §6 "Normal maps"]), read from its red channel by the same march
    /// [hypothesis: the shipped maps are grey].
    pub fn game(image: Handle<Image>) -> Parallax {
        Parallax::height(image, game_parallax_depth())
    }

    /// The fields as `material` has them.
    pub fn of(material: &StandardMaterial) -> Parallax {
        Parallax {
            depth_map: material.depth_map.clone(),
            depth_scale: material.parallax_depth_scale,
            method: material.parallax_mapping_method,
            max_layers: material.max_parallax_layer_count,
        }
    }

    /// Put the fields on `material`.
    pub fn put(&self, material: &mut StandardMaterial) {
        material.depth_map = self.depth_map.clone();
        material.parallax_depth_scale = self.depth_scale;
        material.parallax_mapping_method = self.method;
        material.max_parallax_layer_count = self.max_layers;
    }
}

/// The files [`find`] and the decoders already warned about (one warning per file), and the
/// own formats and sizes of the members decoded under `textures\pbr\` ([`note_source`]).
#[derive(Default)]
pub struct Memo {
    warned: Mutex<HashSet<String>>,
    sources: Mutex<HashMap<String, Source>>,
}

/// A decoded member file's own format and its top level's size, before any decode on the
/// CPU ([`note_source`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Source {
    pub format: fnv_dds::Format,
    pub width: u32,
    pub height: u32,
}

/// The scalar files of one mount by path, each read once ([`scalars_cached`]); kept beside the
/// mount's index (`GameData`), so a new mount starts empty.
pub type ScalarCache = Mutex<HashMap<String, Arc<OnceLock<Scalars>>>>;

/// The scalars of the file at `path`, from `cache`: the first caller reads (`read`), parses
/// and logs its warnings while any other caller for the same path waits on that path's cell,
/// so a file is read and warned about once; other paths are not held up.
pub(crate) fn scalars_cached(cache: &ScalarCache, path: &str, read: impl FnOnce() -> Option<Vec<u8>>) -> Scalars {
    let cell = cache.lock().expect("authored scalars").entry(path.to_string()).or_default().clone();
    *cell.get_or_init(|| match read() {
        Some(bytes) => {
            let file = fnv_pbr::parse_scalars(&bytes);
            for w in &file.warnings {
                warn!("authored materials: {path}: {w}");
            }
            file.scalars
        }
        None => {
            warn!("authored materials: {path} could not be read; the defaults apply");
            Scalars::default()
        }
    })
}

impl Memo {
    /// Log `message` the first time `path` is warned about.
    pub(crate) fn warn_once(&self, path: &str, message: impl FnOnce() -> String) {
        if self.warned.lock().expect("authored warnings").insert(path.to_string()) {
            warn!("{}", message());
        }
    }
}

/// The authored set (state [`SetState::Authored`]) for the resolved diffuse `key`: the one
/// active set the index finds (`PbrIndex::lookup`, with its first-person rule), if it is
/// authored. A height-only set means "no authored set" for the shape's ORM part, also when
/// it is a first-person twin's own set: the twin's set wins the lookup and the plain
/// texture's set is not used behind it ([`find`] gives the twin's height only) [design: the
/// first-person rule of docs/spec-notes/pbr-materials.md "Layout"].
pub fn authored_found(index: &PbrIndex, key: &str) -> Option<Found> {
    index.lookup(key).filter(|f| f.state() == SetState::Authored)
}

/// A member loaded by [`find`]'s caller: its handle and, where the caller knows it, the
/// format it went up in.
pub(crate) type Loaded = (Handle<Image>, Option<TextureFormat>);

/// The set for the resolved diffuse `key` with its maps loaded by `load` (path as the index
/// names it, exactly: never resolved again, so a `\xbox\` member stays the one listed; role),
/// or `None`: the setting off (`enabled`; nothing is asked of the index then), no active
/// set, an authored set whose `_orm` does not load (one warning per file; the shape keeps the
/// game's material), or a height-only set without a height map to draw ([`height_map`]). An
/// `_n` that does not load leaves the game's normal map in use; an authored set's `_p` that
/// gives no height map leaves the rest of the set in use.
pub(crate) fn find(data: &GameData, enabled: bool, key: &str, memo: &Memo, mut load: impl FnMut(&str, TextureUsage) -> Option<Loaded>) -> Option<PbrSet> {
    if !enabled {
        return None;
    }
    let found = data.pbr_index().lookup(key)?;
    let scalars = |found: &Found| found.path(Member::Scalars).map(|path| data.pbr_scalars(&path)).unwrap_or_default();
    match found.state() {
        SetState::Authored => {
            let orm_path = found.path(Member::Orm)?;
            let Some((orm, _)) = load(&orm_path, TextureUsage::Linear) else {
                Stats::add(&stats().orm_failed);
                memo.warn_once(&orm_path, || format!("authored materials: {orm_path} could not be loaded; surfaces drawn with {key} keep the game's material"));
                return None;
            };
            let normal = found.path(Member::Normal).and_then(|path| {
                let h = load(&path, TextureUsage::NormalTwoChannel).map(|(h, _)| h);
                if h.is_none() {
                    memo.warn_once(&path, || format!("authored materials: {path} could not be loaded; the game's normal map is used"));
                }
                h
            });
            let scalars = scalars(&found);
            let height = height_map(&found, &scalars, memo, &mut load);
            let metal_gain = set_metal_gain(&scalars, metal_gain_override());
            Some(PbrSet::Authored(AuthoredSet { stem: found.stem, orm, normal, height, scalars, metal_gain }))
        }
        SetState::HeightOnly => {
            // Of the scalars only the depth applies to a height-only set.
            let height = height_map(&found, &scalars(&found), memo, &mut load)?;
            Some(PbrSet::HeightOnly(HeightSet { stem: found.stem, height }))
        }
        SetState::Incomplete => None,
    }
}

/// The height map alone of the set for the resolved diffuse `key`: an authored set's `_p` or a
/// height-only set's, at its scalars' depth, the set's other members not loaded; for a surface
/// that takes nothing else of a set (the landscape's texture layers, nvforever terrain.rs
/// "Parallax", #3322) [design]. `None` as [`find`] gives no height map: the setting off (the
/// index not asked), no active set, no `_p`, a depth of 0, or a `_p` that does not load or is
/// refused ([`height_map`], one warning per file). Unlike [`find`], an authored set whose
/// `_orm` does not load keeps its height map here, as nothing of the `_orm` is used.
pub(crate) fn find_height(data: &GameData, enabled: bool, key: &str, memo: &Memo, mut load: impl FnMut(&str, TextureUsage) -> Option<Loaded>) -> Option<Parallax> {
    if !enabled {
        return None;
    }
    let found = data.pbr_index().lookup(key)?;
    if found.state() == SetState::Incomplete {
        return None;
    }
    let scalars = found.path(Member::Scalars).map(|path| data.pbr_scalars(&path)).unwrap_or_default();
    height_map(&found, &scalars, memo, &mut load)
}

/// The set's `_p` as a height map at the scalars' depth ([`Scalars::parallax_depth_or_default`]),
/// or `None`: no `_p`, a depth of 0 (the file is then not loaded), a `_p` that does not
/// load, or a signed one ([`signed_format`]; refused, as `fnvtool pbr check` refuses it), one
/// warning per file either way.
fn height_map(found: &Found, scalars: &Scalars, memo: &Memo, load: &mut impl FnMut(&str, TextureUsage) -> Option<Loaded>) -> Option<Parallax> {
    let path = found.path(Member::Height)?;
    let depth = scalars.parallax_depth_or_default();
    if depth <= 0.0 {
        return None;
    }
    let Some((image, format)) = load(&path, TextureUsage::Linear) else {
        Stats::add(&stats().height_failed);
        memo.warn_once(&path, || format!("authored materials: {path} could not be loaded; no height map is drawn from it"));
        return None;
    };
    let source = memo.source(&path);
    if !height_source_ok(source.map(|s| s.format), format) {
        Stats::add(&stats().height_refused);
        let why = height_refusal(source.map(|s| s.format), format);
        memo.warn_once(&path, || format!("authored materials: {path} {why}, so it is not drawn (export it as BC4_UNORM)"));
        return None;
    }
    if let Some(why) = source.and_then(|s| height_size_refusal(s.width, s.height)) {
        Stats::add(&stats().height_refused);
        memo.warn_once(&path, || format!("authored materials: {path} {why}, so it is not drawn (`fnvtool pbr check` refuses it too)"));
        return None;
    }
    Some(Parallax::height(image, depth))
}

/// Why a `_p` whose top level is `width` x `height` is refused by its size, or `None`: each
/// side must be a multiple of 4, the rule `fnvtool pbr check` applies to every member
/// (`not-block-aligned`), so the engine and the validator agree on `_p`. The engine could
/// decode such a file on the CPU and draw it; it refuses it so that a `_p` the game draws
/// also passes the check [design, review of 2026-11-20; docs/spec-notes/pbr-materials.md
/// "Compression and sizes"]. Only `_p` is judged so here: `_orm` and `_n` keep the
/// decoder's own rules. A `_p` whose source is not known here (not decoded through this
/// memo) is not judged by size.
pub fn height_size_refusal(width: u32, height: u32) -> Option<String> {
    (!width.is_multiple_of(4) || !height.is_multiple_of(4)).then(|| format!("is {width}x{height}, a side not a multiple of 4"))
}

/// Whether a loaded `_p` may be drawn, by the file's own format (`source`, as decoded:
/// [`note_source`]) and the format it went up in (`format`, [`height_format_ok`]). The file's
/// format must be one `fnvtool pbr check` allows for `_p` (its `allowed_formats`: BC4 unsigned,
/// DXT1/BC1, DXT5/BC3), so a DXT3 file is refused whether it went up as BC2 blocks or, off the
/// 4-texel grid, decoded to `Rgba8Unorm` [design, review of 2026-11-20]. A `_p` whose own
/// format is not known here (not decoded through this memo) is taken only in a format that
/// names its source (`Rgba8Unorm` comes from a block file off the grid or an uncompressed one).
pub fn height_source_ok(source: Option<fnv_dds::Format>, format: Option<TextureFormat>) -> bool {
    use fnv_dds::Format;
    height_format_ok(format)
        && match source {
            Some(f) => matches!(f, Format::Bc4U | Format::Dxt1 | Format::Dxt5),
            None => format != Some(TextureFormat::Rgba8Unorm),
        }
}

/// Why a `_p` is refused ([`height_source_ok`]), naming the format it is in: the file's own
/// where known (`BC5_SNORM`, `DXT3`), else the one it went up in.
pub fn height_refusal(source: Option<fnv_dds::Format>, format: Option<TextureFormat>) -> String {
    use fnv_dds::Format;
    let shown = match (source, format) {
        (Some(f), _) => f.name().to_string(),
        (None, Some(f)) => format!("{f:?}"),
        (None, None) => "an unknown format".to_string(),
    };
    if matches!(source, Some(Format::Bc4S | Format::Bc5S)) || format.is_some_and(signed_format) {
        format!("is signed ({shown}); a height map is unsigned (0 low, 1 high)")
    } else {
        format!("is in {shown}, not in a height map's format (BC4_UNORM, DXT1 or DXT5)")
    }
}

/// Note the own format and top-level size (`size`: width, height) of a file decoded under
/// `textures\pbr\` (`path` as the decoder keys it, normalised), for [`height_source_ok`] and
/// [`height_size_refusal`]: the main thread and the workers decode with the same [`Memo`],
/// so a later lookup of a cached or adopted texture finds it.
pub(crate) fn note_source(memo: &Memo, path: &str, source: Option<fnv_dds::Format>, size: (u32, u32)) {
    if let Some(format) = source.filter(|_| path.starts_with(fnv_pbr::PBR_DIR)) {
        memo.sources.lock().expect("authored sources").insert(path.to_string(), Source { format, width: size.0, height: size.1 });
    }
}

impl Memo {
    /// The own format and size of the file at `path` as last decoded ([`note_source`]).
    fn source(&self, path: &str) -> Option<Source> {
        self.sources.lock().expect("authored sources").get(&fnv_vfs::normalize(path)).copied()
    }
}

/// Whether a member went up in a signed format (`_SNORM`: values -1..1). A height map holds
/// 0 (lowest) to 1 (highest); the parallax march reads `1 - height` as depth, which a negative
/// sample would push below the deepest layer, so a signed `_p` is refused [design, the
/// format's rule: docs/spec-notes/pbr-materials.md "Compression and sizes"].
pub fn signed_format(format: TextureFormat) -> bool {
    matches!(format, TextureFormat::Bc4RSnorm | TextureFormat::R8Snorm | TextureFormat::Bc5RgSnorm | TextureFormat::Rg8Snorm)
}

/// Whether a loaded `_p` went up in a format a height map takes: the unsigned linear forms
/// the format allows (BC4 unsigned, BC1, BC3) and their decoded fallbacks (`R8Unorm`,
/// `Rgba8Unorm`). Anything else, a signed format or a format the caller could not tell
/// (`None`), is refused rather than accepted by default [design, review of 2026-11-20]. The
/// file's own format is judged too ([`height_source_ok`]).
pub fn height_format_ok(format: Option<TextureFormat>) -> bool {
    matches!(
        format,
        Some(TextureFormat::Bc4RUnorm | TextureFormat::R8Unorm | TextureFormat::Bc1RgbaUnorm | TextureFormat::Bc3RgbaUnorm | TextureFormat::Rgba8Unorm)
    )
}

/// The value of `NifSurface::emissive_rule.w` that marks a material shaded from an authored
/// set (0 on every other material; the lane is otherwise unused, and survives the mesh
/// terrain blend's conversion).
pub const AUTHORED_MARK: f32 = 1.0;

/// What a lit shape's material is without a set (its "legacy look"), kept on the material
/// (`NifSurface::legacy`) for every shape that may take a set, so a later change of its
/// diffuse (a script's texture swap, the blackjack cards, the Vigor Tester's digits) can put
/// the new diffuse's set on it or go back to the game's values ([`retexture`]). Changed
/// after the material is made only by a script's glossiness or specular colour edit
/// ([`set_legacy_roughness`], [`set_legacy_specular_tint`]);
/// copies carry it along.
#[derive(Clone, Debug, PartialEq, Reflect)]
pub struct LegacyLook {
    /// The game's normal map in use (after the tangent check).
    pub normal: Option<Handle<Image>>,
    /// The specular mask (the normal map, when its alpha is one: `nif_spawn::specular`).
    pub specular: Option<Handle<Image>>,
    pub roughness: f32,
    pub reflectance: f32,
    pub metallic: f32,
    /// The drawn mesh has tangents, so a set's `_n` or a height map can be applied to it even
    /// where the game gives no normal map. Those of the mesh the shape is drawn with: a
    /// material made on a copy of a drawn mesh (`nif_spawn::Tangents::Keep`, an actor's texture
    /// swap or texture set) records the drawn mesh's own, not any made on the copy (#2979).
    pub tangents: bool,
    /// The specular tint (Bevy multiplies the reflectance by it): white as built, a script's
    /// specular colour once one is set ([`set_legacy_specular_tint`]).
    pub specular_tint: Color,
    /// The parallax fields as built: the game's own height map where the shape draws one
    /// ([`apply_game_height`], #2979), else Bevy's no-depth-map defaults.
    pub parallax: Parallax,
    /// A specular colour a script set itself (the colour was the value it changed), which a
    /// set put on later keeps drawing in place of white ([`apply`]); `None` as built, and when
    /// only the stored colour was written back with another value [design].
    pub scripted_tint: Option<Color>,
}

/// What became of a shape's own height map (texture-set slot 3 with the parallax flag,
/// #2979) when its material was built ([`apply_game_height`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameHeight {
    /// On the material: the game's material draws it, whatever the Authored materials setting.
    Drawn,
    /// Not on: the shape may draw no height map ([`height_gate`]: alpha-tested, blended,
    /// double-sided, a decal, unlit, refracting or without tangents or texture coordinates).
    Flat,
    /// The shape may draw it, but the file did not load.
    NotLoaded,
}

/// Put a shape's own height map on its freshly built `material` (#2979): `image` (the slot-3
/// texture, `None` when it was not asked for or did not load) at the game depth
/// ([`Parallax::game`]) when the shape may draw one (`wanted`, the build's early test that
/// decides whether the file is loaded at all and tangents made for it) and [`height_gate`]
/// passes with the drawn mesh's `tangents`. It is the game's material, not an overlay
/// (`NifSurface::height_overlay` stays clear): taken before the legacy look, it is that look's
/// baseline, which a set's `_p` replaces and a set taken off puts back [design,
/// docs/spec-notes/pbr-materials.md "Height maps"]. Counted for the exit summary.
pub fn apply_game_height(material: &mut crate::NifMaterial, image: Option<Handle<Image>>, wanted: bool, tangents: bool) -> GameHeight {
    let out = match image {
        Some(image) if wanted && height_gate(material, tangents).is_ok() => {
            Parallax::game(image).put(&mut material.base);
            GameHeight::Drawn
        }
        None if wanted => GameHeight::NotLoaded,
        _ => GameHeight::Flat,
    };
    note_game_height(out);
    out
}

/// Whether a shape may take an authored set, by what is known when its material is made:
/// lit, not refracting, not hair or skin (FaceGen), drawn neither additive nor multiplied
/// (the final alpha mode), with texture coordinates, and not excluded by its caller
/// (`no_authored`: particles, object LOD) [design, docs/spec-notes/pbr-materials.md "What a
/// set changes"]. The key is checked by [`key_eligible`].
pub fn shape_eligible(unlit: bool, refracts: bool, hair: bool, skin: bool, alpha_mode: AlphaMode, has_uv: bool, no_authored: bool) -> bool {
    !unlit && !refracts && !hair && !skin && has_uv && !no_authored && !matches!(alpha_mode, AlphaMode::Add | AlphaMode::Multiply)
}

/// Whether a resolved diffuse key may take a set: not the Pip-Boy's or the interface's
/// textures [design].
pub fn key_eligible(key: &str) -> bool {
    !key.starts_with("textures\\pipboy3000\\") && !key.starts_with("textures\\interface\\")
}

/// The metallic value an authored material is built with: 0 (the ORM's blue channel then has
/// no effect: no reflection source, docs/spec-notes/pbr-materials.md "Metallic"), or 1 in the
/// metallic channel view ([`PbrView::Metallic`]), so the material's sample of the blue
/// channel is the per-texel value the view shows, under the metallic debug switch
/// ([`metallic_debug`]), so the blue channel takes effect, and with reflections on
/// (`reflection_cube::enabled`), where nif_material.wesl holds the sampled value at 0 in every
/// view without a reflection cube, so only a view with one shades a metal as a metal; and
/// under Physical lighting ([`physical_pbr`]), whose sky image-based light every view has.
pub fn authored_metallic() -> f32 {
    if view() == PbrView::Metallic || metallic_debug() || crate::reflection_cube::enabled() || physical_pbr() { 1.0 } else { 0.0 }
}

/// The environment variable of the metallic debug switch ([`metallic_debug`]).
pub const METALLIC_ENV: &str = "NVF_PBR_METALLIC";

/// The metallic debug switch (a calibration aid, off by default; `NVF_PBR_METALLIC=1`, read
/// once at the start): authored materials are built with metallic 1, so the ORM's blue
/// channel takes effect, and nif_material.wesl's removal of Bevy's ambient highlight keeps
/// the metal's share of it on authored materials ((1 - m) of it is removed, m the sampled
/// metallic), so a metal shows the ambient light in its colour instead of drawing black where
/// no lamp's highlight falls (`NVF_PBR_METALLIC` shader def) [design, DESIGN option (b)].
/// Every other material is drawn as without it. With reflections on
/// (`reflection_cube::enabled`) the "keep the metal share" rule is retired: the shader def is
/// not set and the full ambient highlight is removed, also at metallic 1, since the cube
/// replaces it (nif_material.rs, reflections slice 1).
pub fn metallic_debug() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var(METALLIC_ENV).is_ok_and(|v| v.trim() == "1");
        if on {
            info!("authored materials: metallic debug switch ({METALLIC_ENV}=1): authored materials are metallic by their ORM's blue channel and keep a metal's ambient highlight");
        }
        on
    })
}

/// The environment variable of the metal gain override ([`metal_gain_override`]).
pub const METAL_GAIN_ENV: &str = "NVF_PBR_METAL_GAIN";

/// The value of [`METAL_GAIN_ENV`] as a gain: `Ok(None)` for no value (or an empty one),
/// `Ok(Some(g))` for a number in the scalar file's range (`fnv_pbr::METAL_GAIN_RANGE`, 1 to
/// 40), `Err` with the reason otherwise.
pub fn parse_metal_gain(text: Option<&str>) -> Result<Option<f32>, String> {
    let range = fnv_pbr::METAL_GAIN_RANGE;
    let Some(t) = text.map(str::trim).filter(|t| !t.is_empty()) else { return Ok(None) };
    match t.parse::<f32>() {
        Ok(g) if range.contains(&g) => Ok(Some(g)),
        Ok(g) => Err(format!("{METAL_GAIN_ENV}={g} is outside {} to {}", range.start(), range.end())),
        Err(_) => Err(format!("{METAL_GAIN_ENV}={t} is not a number")),
    }
}

/// The metal gain override (a tuning aid, off by default; `NVF_PBR_METAL_GAIN=<gain>`, 1 to
/// 40, read once at the start, NifMaterialPlugin): every set's metal gain
/// ([`AuthoredSet::metal_gain`]) is this value for the run, whatever its scalar file says,
/// so a gain can be judged by picture [design]. A value that is no number or
/// out of range is warned about once and the sets keep their own gains. Not a
/// `NVF_PLAY_TEST_*` aid: it does not make a run unattended.
pub fn metal_gain_override() -> Option<f32> {
    static GAIN: OnceLock<Option<f32>> = OnceLock::new();
    *GAIN.get_or_init(|| match parse_metal_gain(std::env::var(METAL_GAIN_ENV).ok().as_deref()) {
        Ok(Some(g)) => {
            info!("authored materials: metal gain override ({METAL_GAIN_ENV}={g}): every authored set's metal gain is {g} for this run");
            Some(g)
        }
        Ok(None) => None,
        Err(reason) => {
            warn!("authored materials: {reason}; each set keeps its own metal gain");
            None
        }
    })
}

/// A set's metal gain: `over` (the run's override, [`metal_gain_override`]) when given, else
/// the scalar file's `metal_gain`, else 12 (`fnv_pbr::DEFAULT_METAL_GAIN`) [design, owner
/// 2026-11-21, #3237].
pub fn set_metal_gain(scalars: &Scalars, over: Option<f32>) -> f32 {
    over.unwrap_or_else(|| scalars.metal_gain_or_default())
}

/// The base colour an authored metal is lit with in a view with a reflection cube: the CPU
/// mirror of nif_material.wesl's `nvf_metal_base` (tests compare the two by their rules):
/// mix(base, lifted, metallic), lifted = base x min(gain, 1 / brightest channel), never below
/// base. One gain on all three channels, limited so the brightest channel reaches 1, keeps
/// the hue (the experiment before it clipped each channel at 1, which shifted bright texels
/// towards white) [design].
pub fn lifted_base(base: Vec3, gain: f32, metallic: f32) -> Vec3 {
    let brightest = base.x.max(base.y).max(base.z.max(1e-4));
    let lifted = base * gain.min(1.0 / brightest).max(1.0);
    base.lerp(lifted, metallic)
}

/// The environment variable that picks a channel view ([`PbrView`]).
pub const VIEW_ENV: &str = "NVF_PBR_VIEW";

/// A debug view of the material inputs of NIF surfaces (a calibration aid for authored sets,
/// off by default): the lit colour of forward-drawn lit NIF surfaces is replaced by one
/// input, read just after the standard material's sampling (nif_material.wesl,
/// "NVF_PBR_VIEW"). Refracting shapes return before it; additive draws add it by their alpha
/// and multiplied ones stay neutral, so effects fade out of the view. Picked once at
/// the start by `NVF_PBR_VIEW` = `roughness` | `metallic` | `occlusion` | `normal` |
/// `authored` | `reflection` | `height` [design]. Not a `NVF_PLAY_TEST_*` aid: it does not
/// make a run unattended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PbrView {
    Off,
    /// Perceptual roughness after the scale (the material's value × the ORM's green).
    Roughness,
    /// The ORM's blue channel (authored materials are built with metallic 1 in this view).
    Metallic,
    /// The ORM's red channel, sampled on its own (no screen-space occlusion); 1 without one.
    Occlusion,
    /// The world-space normal as 0.5 + 0.5 N.
    Normal,
    /// Authored shapes tinted orange, every other shape grey, by its base colour's brightness.
    Authored,
    /// What an authored surface reflects: the view's reflection cube at level 0 along the
    /// surface's mirror direction, as a white perfect mirror writes it (texel x intensity x
    /// exposure, tone-mapped as the rest of the picture), so its scale can be judged against
    /// the sky in the same frame. Every other shape grey as in [`PbrView::Authored`];
    /// authored shapes orange where the view has no cube or reflections are off. Only the
    /// opted-in NIF fragment (`reflection_cube::REFLECT_DEF`) reads the cube.
    Reflection,
    /// The height map as stored (0 black = low, 1 white = high), read at the unshifted
    /// texture coordinates: the original height, not the depth the march reads; dark blue on
    /// a shape without a depth map (#2981 stage 4).
    Height,
}

impl PbrView {
    pub const ALL: [PbrView; 7] = [PbrView::Roughness, PbrView::Metallic, PbrView::Occlusion, PbrView::Normal, PbrView::Authored, PbrView::Reflection, PbrView::Height];

    pub fn name(self) -> &'static str {
        match self {
            PbrView::Off => "off",
            PbrView::Roughness => "roughness",
            PbrView::Metallic => "metallic",
            PbrView::Occlusion => "occlusion",
            PbrView::Normal => "normal",
            PbrView::Authored => "authored",
            PbrView::Reflection => "reflection",
            PbrView::Height => "height",
        }
    }

    /// The view named `text` (any case; empty or `off`: none); `None` for another word.
    pub fn parse(text: &str) -> Option<PbrView> {
        let t = text.trim().to_ascii_lowercase();
        if t.is_empty() || t == "off" {
            return Some(PbrView::Off);
        }
        PbrView::ALL.into_iter().find(|v| v.name() == t)
    }

    /// The fragment shader's defs for the view: `NVF_PBR_VIEW` and the mode's own.
    pub fn shader_defs(self) -> &'static [&'static str] {
        match self {
            PbrView::Off => &[],
            PbrView::Roughness => &["NVF_PBR_VIEW", "NVF_PBR_VIEW_ROUGHNESS"],
            PbrView::Metallic => &["NVF_PBR_VIEW", "NVF_PBR_VIEW_METALLIC"],
            PbrView::Occlusion => &["NVF_PBR_VIEW", "NVF_PBR_VIEW_OCCLUSION"],
            PbrView::Normal => &["NVF_PBR_VIEW", "NVF_PBR_VIEW_NORMAL"],
            PbrView::Authored => &["NVF_PBR_VIEW", "NVF_PBR_VIEW_AUTHORED"],
            PbrView::Reflection => &["NVF_PBR_VIEW", "NVF_PBR_VIEW_REFLECTION"],
            PbrView::Height => &["NVF_PBR_VIEW", "NVF_PBR_VIEW_HEIGHT"],
        }
    }
}

/// The run's channel view, read from [`VIEW_ENV`] the first time (the NIF material plugin asks
/// at the start) and fixed for the run.
pub fn view() -> PbrView {
    static VIEW: OnceLock<PbrView> = OnceLock::new();
    *VIEW.get_or_init(|| {
        let Some(text) = std::env::var_os(VIEW_ENV) else { return PbrView::Off };
        let text = text.to_string_lossy();
        match PbrView::parse(&text) {
            Some(PbrView::Off) => PbrView::Off,
            Some(v) => {
                info!("authored materials: channel view {} ({VIEW_ENV}): NIF surfaces show that input instead of their lit colour", v.name());
                v
            }
            None => {
                warn!("authored materials: {VIEW_ENV}={text} is none of roughness, metallic, occlusion, normal, authored, reflection, height; no channel view");
                PbrView::Off
            }
        }
    })
}

/// Put `set` on `material` from its legacy look: the ORM as metallic-roughness and occlusion
/// map (one image in both slots), no specular mask, the scalars' roughness scale and
/// reflectance (their defaults without a file), [`authored_metallic`], the set's `_n` when it
/// has one and the drawn mesh has tangents (else the game's normal map, its alpha no longer a
/// mask), the mark ([`AUTHORED_MARK`]) and the set's metal gain (`NifSurface::metal_gain`).
/// Everything else (base colour, alpha, glow, decal, the extension's other values) stays. Whether the set's `_n` was applied.
pub fn apply(material: &mut crate::NifMaterial, set: &AuthoredSet, legacy: &LegacyLook) -> bool {
    let b = &mut material.base;
    b.metallic_roughness_texture = Some(set.orm.clone());
    b.occlusion_texture = Some(set.orm.clone());
    b.specular_texture = None;
    b.perceptual_roughness = set.scalars.roughness_scale_or_default();
    b.reflectance = set.scalars.reflectance_or_default();
    b.metallic = authored_metallic();
    // A set's highlight strength is its reflectance alone, never scaled by the game's stored
    // specular colour (#2981); a colour a script set itself stays drawn, as it is on the
    // material the script edited, across a later set (a texture swap's).
    b.specular_tint = legacy.scripted_tint.unwrap_or(Color::WHITE);
    let normal = set.normal.clone().filter(|_| legacy.tangents);
    if normal.is_none() && set.normal.is_some() {
        Stats::add(&stats().normal_without_tangents);
        no_tangents_once(&set.stem);
    }
    let applied = normal.is_some();
    b.normal_map_texture = normal.or_else(|| legacy.normal.clone());
    material.extension.emissive_rule.w = AUTHORED_MARK;
    material.extension.metal_gain = set.metal_gain;
    applied
}

/// Put the legacy look back on `material` (the inverse of [`apply`], and of
/// [`apply_height`]: the parallax fields go back too where a height overlay is on).
pub fn restore(material: &mut crate::NifMaterial, legacy: &LegacyLook) {
    let b = &mut material.base;
    b.metallic_roughness_texture = None;
    b.occlusion_texture = None;
    b.specular_texture = legacy.specular.clone();
    b.normal_map_texture = legacy.normal.clone();
    b.perceptual_roughness = legacy.roughness;
    b.reflectance = legacy.reflectance;
    b.metallic = legacy.metallic;
    b.specular_tint = legacy.specular_tint;
    material.extension.emissive_rule.w = 0.0;
    material.extension.metal_gain = 0.0;
    remove_height(material, legacy);
}

/// Why a set's height map is not drawn on a material ([`height_gate`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeightSkip {
    /// Alpha-tested (`AlphaMode::Mask`: cut-outs), without the alpha-tested switch
    /// ([`parallax_mask`]).
    AlphaTested,
    /// Any other alpha mode but opaque (blends, fades, blended cut-outs, hair).
    Blended,
    DoubleSided,
    Decal,
    /// The drawn mesh has no tangents (a copy of a drawn mesh, a swap onto one).
    NoTangents,
    /// Unlit, refracting, or eaten by an effect shader's holes.
    Other,
}

/// Whether a shape drawn with `alpha_mode` may take a height map, `mask` being the run's
/// alpha-tested switch ([`parallax_mask`]): opaque always, alpha-tested (`Mask`) only with
/// the switch, any other mode never ([`height_gate`]).
pub fn height_alpha(alpha_mode: AlphaMode, mask: bool) -> Result<(), HeightSkip> {
    match alpha_mode {
        AlphaMode::Opaque => Ok(()),
        AlphaMode::Mask(_) if mask => Ok(()),
        AlphaMode::Mask(_) => Err(HeightSkip::AlphaTested),
        _ => Err(HeightSkip::Blended),
    }
}

/// Whether `material` may draw a height map now, its drawn mesh having tangents (`tangents`):
/// opaque (or alpha-tested under the alpha-tested switch: [`height_alpha`]), single-sided,
/// not a decal, lit, not refracting, without holes [design, the stage 4 review A3]. A copy
/// with any holes set-up (a holes value at or above 0, 255 included, or a holes texture) is
/// refused for its whole life: an effect shader's holes start at 255 (none eaten yet) and
/// fall later by animation alone (nvforever `effect_shaders.rs` `animate_material`), which
/// asks this gate no more (#2979 review). Why: the
/// depth prepass, the shadow passes and the holes test read alpha at the unshifted texture
/// coordinates, so a cut-out's or a blend's coverage would not follow the shifted texels (the
/// switch keeps an alpha-tested shape's cut at the unshifted coordinates in the main pass too,
/// nif_material.wesl `NVF_PBR_PARALLAX_MASK`; a blend has no such test to move); the march
/// takes the incoming tangent frame without turning it for back faces; decals already lean
/// on a depth bias. Asked when a material is built, when a swap puts a set on it
/// ([`retexture`]) and when a copy changes how it draws ([`recheck_height`]).
pub fn height_gate(material: &crate::NifMaterial, tangents: bool) -> Result<(), HeightSkip> {
    let (b, e) = (&material.base, &material.extension);
    if b.unlit || e.refract || e.holes.x >= 0.0 || e.holes_texture.is_some() {
        Err(HeightSkip::Other)
    } else if let Err(why) = height_alpha(b.alpha_mode, parallax_mask()) {
        Err(why)
    } else if b.double_sided {
        Err(HeightSkip::DoubleSided)
    } else if e.decal {
        Err(HeightSkip::Decal)
    } else if !tangents {
        Err(HeightSkip::NoTangents)
    } else {
        Ok(())
    }
}

/// Put a set's height map on `material` (the depth map, its depth, method and layers) when
/// [`height_gate`] passes with the legacy look's tangents; nothing else changes (a height-only
/// set leaves the material the game's own, the authored mark clear). Counts a skip by its
/// reason. Whether it was put on.
pub fn apply_height(material: &mut crate::NifMaterial, height: &Parallax, legacy: &LegacyLook) -> Result<(), HeightSkip> {
    match height_gate(material, legacy.tangents) {
        Ok(()) => {
            height.put(&mut material.base);
            material.extension.height_overlay = true;
            Ok(())
        }
        Err(why) => {
            Stats::add(match why {
                HeightSkip::AlphaTested => &stats().height_alpha_tested,
                HeightSkip::Blended => &stats().height_blended,
                HeightSkip::DoubleSided => &stats().height_double_sided,
                HeightSkip::Decal => &stats().height_decal,
                HeightSkip::NoTangents => &stats().height_no_tangents,
                HeightSkip::Other => &stats().height_other,
            });
            Err(why)
        }
    }
}

/// Whether a set's height map is on `material` (`NifSurface::height_overlay`, set by
/// [`apply_height`]; tracked apart from [`is_authored`]: a height-only set leaves the mark
/// clear).
pub fn height_on(material: &crate::NifMaterial) -> bool {
    material.extension.height_overlay
}

/// Take a height overlay off `material`: the legacy look's parallax fields back, the overlay
/// mark cleared. Nothing changes on a material without one.
fn remove_height(material: &mut crate::NifMaterial, legacy: &LegacyLook) {
    if material.extension.height_overlay {
        legacy.parallax.put(&mut material.base);
        material.extension.height_overlay = false;
    }
}

/// Take whatever set is on `material` off: the authored fields and the height map
/// ([`restore`]) when it is authored, else the height map alone (a height-only set), back to
/// the legacy look; a material wearing neither is left as it is.
pub fn take_off(material: &mut crate::NifMaterial, legacy: &LegacyLook) {
    if is_authored(material) {
        restore(material, legacy);
    } else {
        remove_height(material, legacy);
    }
}

/// A copy of a material whose drawing changed after it was made (an effect shader's holes
/// making an opaque copy `Mask(0)`, a fade making it `Blend`: nvforever `effect_shaders.rs`,
/// nvf-play `fade.rs`; a cloak's refracting copy: `nif_spawn::refraction_material`): if it
/// carries a height map that [`height_gate`] no longer allows, a set's overlay or the game's
/// own (#2979; no other source puts a depth map on a NIF material), it comes off (the depth
/// map cleared, the legacy look's other parallax fields back). A material without a depth map
/// is left as it is. Whether it took one off.
pub fn recheck_height(material: &mut crate::NifMaterial) -> bool {
    if material.base.depth_map.is_none() || height_gate(material, true).is_ok() {
        return false;
    }
    let baseline = material.extension.legacy.as_ref().map(|l| l.parallax.clone()).unwrap_or_default();
    Parallax { depth_map: None, ..baseline }.put(&mut material.base);
    Stats::add(if material.extension.height_overlay { &stats().height_dropped } else { &stats().game_height_dropped });
    material.extension.height_overlay = false;
    true
}

/// A script's change of a shape's glossiness (nvforever `nif_mirror.rs`, the material
/// property mirror) as perceptual roughness: the legacy look takes it, so the game's value is
/// the one restored later, and the drawn roughness takes it only while no set is on the
/// material; a set's roughness (its scale on the ORM's green) stays in force meanwhile
/// [design].
pub fn set_legacy_roughness(material: &mut crate::NifMaterial, roughness: f32) {
    if let Some(legacy) = material.extension.legacy.as_mut() {
        legacy.roughness = roughness;
    }
    if !is_authored(material) {
        material.base.perceptual_roughness = roughness;
    }
}

/// A script's specular colour (nvforever `nif_mirror.rs`): the legacy look takes it, so it is
/// the one restored later; the drawn tint takes it when no set is on the material, or when
/// the colour is what the script changed (`changed`). The mirror writes every value back
/// after any one change, so an edit of another property leaves a set's highlight (Bevy draws
/// `specular_tint x reflectance`) as it is [design]. A colour the script changed is kept as
/// its override (`LegacyLook::scripted_tint`), so a set applied later ([`apply`], through a
/// swap's [`retexture`]) keeps drawing it instead of going back to white.
pub fn set_legacy_specular_tint(material: &mut crate::NifMaterial, tint: Color, changed: bool) {
    if let Some(legacy) = material.extension.legacy.as_mut() {
        legacy.specular_tint = tint;
        if changed {
            legacy.scripted_tint = Some(tint);
        }
    }
    if changed || !is_authored(material) {
        material.base.specular_tint = tint;
    }
}

/// Whether any shape of `data`'s mount can take a set: the run's setting on (`enabled`) and
/// at least one active set (authored or height only) in the mount's index (built here if it
/// was not).
/// Callers that make shapes legacy for good once their role is known (nvforever `actors.rs`)
/// skip that work when it is false: no set can reach such a shape in this mount, through a
/// material build or a later swap alike. A new mount (`GameData::set_vfs`) would need those
/// materials made again; no production path mounts again (only tests call `set_vfs`).
pub fn mount_has_sets(data: &GameData) -> bool {
    mount_has_sets_when(data, enabled())
}

fn mount_has_sets_when(data: &GameData, on: bool) -> bool {
    on && data.pbr_index().counts().active() > 0
}

/// Whether `material` is shaded from an authored set.
pub fn is_authored(material: &crate::NifMaterial) -> bool {
    material.extension.emissive_rule.w == AUTHORED_MARK
}

/// Make `material` the game's own for good: its legacy look put back and its baseline
/// dropped, so no later swap gives it a set. For shapes found to be hair or a FaceGen tint
/// target only after their material was made (nvforever `actors.rs`, `facegen.rs`). Whether
/// anything changed.
pub fn make_legacy(material: &mut crate::NifMaterial) -> bool {
    let Some(legacy) = material.extension.legacy.take() else { return false };
    take_off(material, &legacy);
    Stats::add(&stats().made_legacy);
    true
}

/// Clear every authored map from an unlit copy (the Pip-Boy's glare and menus copies,
/// nvforever `pipboy_arm.rs`, the terminal's glass and glare): the ORM in both slots, the
/// depth map (Bevy applies parallax before its unlit test; a height overlay's other parallax
/// fields go back to the legacy look's), the mark, the metal gain and the legacy look, so nothing of a set reaches it whatever its source wore [design, a second
/// guard beside the `textures\pipboy3000\` exclusion].
pub fn clear_for_unlit_copy(material: &mut crate::NifMaterial) {
    material.base.metallic_roughness_texture = None;
    material.base.occlusion_texture = None;
    // A height overlay of this module's: its settings back to the legacy look's (or Bevy's
    // defaults without one); any other parallax settings are the copy's own and stay.
    let legacy = material.extension.legacy.take();
    if material.extension.height_overlay {
        let baseline = legacy.map(|l| l.parallax).unwrap_or_default();
        baseline.put(&mut material.base);
        material.extension.height_overlay = false;
    }
    material.base.depth_map = None;
    material.extension.emissive_rule.w = 0.0;
    material.extension.metal_gain = 0.0;
}

/// What [`retexture`] did to a material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retextured {
    /// The material takes no set (no legacy look kept: not eligible, or made legacy).
    Untouched,
    /// The new diffuse's set is on it.
    Authored,
    /// The new diffuse's set is height only: the legacy look with its height map, where
    /// [`height_gate`] allows one.
    HeightOnly,
    /// No set for the new diffuse: the legacy look is on it (as it was, or put back).
    Legacy,
}

/// The one rule for a lit copy of a shape's material whose diffuse was just replaced by
/// `texture` (a NIF texture path; script `SwapTexture`, the blackjack cards, the Vigor
/// Tester's buttons and digits): whatever set the copy wore comes off first ([`take_off`]:
/// authored fields and height map alike), then, from the material's legacy look
/// ([`LegacyLook`], carried by the copy), the new diffuse's set is applied (its scalars'
/// defaults, the game's normal map where it has no `_n` or the drawn mesh has no tangents;
/// its height map where [`height_gate`] allows one with the drawn mesh's recorded tangents),
/// or a height-only set's height map alone, or nothing. Always from the legacy look, never
/// from what the copy wore: a copy of a material that wore another diffuse's set takes none
/// of it. Only the set's fields change; tint, animation, fade and every other value stay.
pub fn retexture(
    material: &mut crate::NifMaterial,
    texture: &str,
    textures: &mut crate::TextureCache,
    data: &GameData,
    images: &mut Assets<Image>,
    formats: Option<&bevy::image::CompressedImageFormatSupport>,
) -> Retextured {
    use crate::nif_spawn::TextureSource;
    let Some(legacy) = material.extension.legacy.clone() else { return Retextured::Untouched };
    let mut source = crate::nif_spawn::StoreTextures { textures, data, images, formats };
    let key = source.resolve(texture);
    let set = key_eligible(&key).then(|| source.pbr_set(&key)).flatten();
    take_off(material, &legacy);
    match set {
        Some(PbrSet::Authored(set)) => {
            apply(material, &set, &legacy);
            if let Some(height) = &set.height {
                let _ = apply_height(material, height, &legacy);
            }
            Stats::add(&stats().swaps_authored);
            Retextured::Authored
        }
        Some(PbrSet::HeightOnly(set)) => {
            Stats::add(&stats().swaps_height);
            if apply_height(material, &set.height, &legacy).is_ok() {
                Stats::add(&stats().swaps_height_applied);
            }
            Retextured::HeightOnly
        }
        None => {
            Stats::add(&stats().swaps_legacy);
            Retextured::Legacy
        }
    }
}

/// What authored sets did this run, for the exit summary ([`report`]): counts of events, not
/// a census of visible shapes (a material rebuilt or discarded counts again). Relaxed
/// atomics; a set of stems only where a set is applied.
#[derive(Default)]
pub struct Stats {
    /// Material builds that applied an ORM set (`nif_spawn::shape_material`; height-only sets
    /// are counted in `height_only_built`).
    pub built: AtomicU64,
    /// Shapes the run's setting would serve that may take no set (unlit, refracting, hair,
    /// skin, additive or multiplied, no UVs, particles, landscape LOD).
    pub not_eligible: AtomicU64,
    /// Shapes whose diffuse is under `textures\pipboy3000\` or `textures\interface\`.
    pub excluded_texture: AtomicU64,
    /// Sets whose `_orm` did not load, asked for by a material build or by a swap's
    /// [`retexture`] alike ([`find`]; the shape stays the game's).
    pub orm_failed: AtomicU64,
    /// Sets put on a material without their `_n`, the drawn mesh having no tangents.
    pub normal_without_tangents: AtomicU64,
    /// Copies made the game's own after their material was built (a role found later:
    /// [`make_legacy`] calls that dropped a legacy look).
    pub made_legacy: AtomicU64,
    /// [`retexture`] calls on materials with a legacy look that put a set on, and that left or
    /// put back the legacy look (cached swaps do not call it again).
    pub swaps_authored: AtomicU64,
    pub swaps_legacy: AtomicU64,
    /// [`retexture`] calls whose new diffuse's height-only set loaded (a match with a depth of
    /// 0 or a `_p` that did not load or was refused counts in `swaps_legacy`), and of them
    /// those that put its height map on ([`height_gate`] passed).
    pub swaps_height: AtomicU64,
    pub swaps_height_applied: AtomicU64,
    /// Material builds that put a height map on (`nif_spawn::shape_material`), and of them
    /// those from a height-only set.
    pub height_built: AtomicU64,
    pub height_only_built: AtomicU64,
    /// Height maps a build or a swap did not put on, by [`HeightSkip`] reason.
    pub height_alpha_tested: AtomicU64,
    pub height_blended: AtomicU64,
    pub height_double_sided: AtomicU64,
    pub height_decal: AtomicU64,
    pub height_no_tangents: AtomicU64,
    pub height_other: AtomicU64,
    /// Sets whose `_p` did not load (the rest of an authored set still applies).
    pub height_failed: AtomicU64,
    /// Sets whose `_p` is refused: signed or in no height map's format ([`height_source_ok`]),
    /// or a side not a multiple of 4 ([`height_size_refusal`]).
    pub height_refused: AtomicU64,
    /// Later copies that dropped a height map ([`recheck_height`]).
    pub height_dropped: AtomicU64,
    /// The game's own height maps (#2979, [`apply_game_height`]), whatever the setting:
    /// material builds that drew one, that left it flat, whose file did not load, that drew
    /// a set's `_p` in its place, and later copies that dropped one ([`recheck_height`]).
    pub game_height_drawn: AtomicU64,
    pub game_height_flat: AtomicU64,
    pub game_height_not_loaded: AtomicU64,
    pub game_height_replaced: AtomicU64,
    pub game_height_dropped: AtomicU64,
    stems: Mutex<HashSet<String>>,
}

impl Stats {
    fn add(counter: &AtomicU64) {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn get(counter: &AtomicU64) -> u64 {
        counter.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The summary line (without its prefix).
    pub fn line(&self) -> String {
        format!(
            "{} material builds applied an ORM set ({} distinct ORM set stems at build), {} a height-only set's height map; builds without one: {} not eligible, {} on excluded textures; {} ORM load failures at build or swap; {} applied without their _n (no tangents); {} role copies made legacy; {} retexture calls ({} to an ORM set, {} to a loaded height-only set ({} with its height map applied), {} to the legacy look); height maps: {} material builds applied one; not put on at build or swap: {} alpha-tested, {} blended, {} double-sided, {} decals, {} without tangents, {} unlit, refracting or holed; {} _p not loaded, {} _p refused (signed, in no height map's format or a side not a multiple of 4); {} later copies dropped one",
            Self::get(&self.built),
            self.stems.lock().expect("authored stats").len(),
            Self::get(&self.height_only_built),
            Self::get(&self.not_eligible),
            Self::get(&self.excluded_texture),
            Self::get(&self.orm_failed),
            Self::get(&self.normal_without_tangents),
            Self::get(&self.made_legacy),
            Self::get(&self.swaps_authored) + Self::get(&self.swaps_height) + Self::get(&self.swaps_legacy),
            Self::get(&self.swaps_authored),
            Self::get(&self.swaps_height),
            Self::get(&self.swaps_height_applied),
            Self::get(&self.swaps_legacy),
            Self::get(&self.height_built),
            Self::get(&self.height_alpha_tested),
            Self::get(&self.height_blended),
            Self::get(&self.height_double_sided),
            Self::get(&self.height_decal),
            Self::get(&self.height_no_tangents),
            Self::get(&self.height_other),
            Self::get(&self.height_failed),
            Self::get(&self.height_refused),
            Self::get(&self.height_dropped),
        )
    }
}

impl Stats {
    /// The game height maps' summary line (without its prefix), `depth` the run's game depth.
    /// The first count is of the builds that still draw the game's map after a set's `_p`
    /// replaced it on some (drawn minus replaced); a depth of 0 says "off" and counts nothing.
    pub fn game_line(&self, depth: f32) -> String {
        let state = if depth > 0.0 { format!("depth {depth}") } else { format!("off ({GAME_PARALLAX_DEPTH_ENV}=0)") };
        let replaced = Self::get(&self.game_height_replaced);
        format!(
            "{state} ({}, {} layers); {} material builds draw a NIF's own height map (texture-set slot 3, shader flag 11), {} more had it replaced by an authored _p, {} left it flat (alpha-tested, blended, double-sided, decal, unlit, refracting, a holes effect or without tangents on the drawn mesh), {} could not load it; {} later copies dropped one",
            parallax_method_name(parallax_method()),
            parallax_layers(),
            Self::get(&self.game_height_drawn).saturating_sub(replaced),
            replaced,
            Self::get(&self.game_height_flat),
            Self::get(&self.game_height_not_loaded),
            Self::get(&self.game_height_dropped),
        )
    }
}

/// A build's [`GameHeight`], counted.
pub(crate) fn note_game_height(what: GameHeight) {
    Stats::add(match what {
        GameHeight::Drawn => &stats().game_height_drawn,
        GameHeight::Flat => &stats().game_height_flat,
        GameHeight::NotLoaded => &stats().game_height_not_loaded,
    });
}

/// A build whose own height map a set's `_p` replaced.
pub(crate) fn note_game_height_replaced() {
    Stats::add(&stats().game_height_replaced);
}

/// The run's counts ([`Stats`]).
pub fn stats() -> &'static Stats {
    static STATS: OnceLock<Stats> = OnceLock::new();
    STATS.get_or_init(Stats::default)
}

/// A shape the setting would serve skipped by [`shape_eligible`] (`texture`: by
/// [`key_eligible`] instead).
pub(crate) fn note_skipped(texture: bool) {
    Stats::add(if texture { &stats().excluded_texture } else { &stats().not_eligible });
}

/// A material built with a height map on (`height_only`: from a height-only set).
pub(crate) fn note_height_built(height_only: bool) {
    Stats::add(&stats().height_built);
    if height_only {
        Stats::add(&stats().height_only_built);
    }
}

/// A material built with `set`.
pub(crate) fn note_built(set: &AuthoredSet) {
    Stats::add(&stats().built);
    let mut stems = stats().stems.lock().expect("authored stats");
    if !stems.contains(&set.stem) {
        stems.insert(set.stem.clone());
    }
}

/// The exit summary (Last, on the app's exit, as the other systems' summary lines): one line,
/// `authored materials summary: ...`, or that the setting was off; and one for the game's own
/// height maps, `game height maps summary: ...`, whatever the setting.
pub fn report(mut exit: MessageReader<AppExit>) {
    if exit.read().last().is_none() {
        return;
    }
    info!("game height maps summary: {}", stats().game_line(game_parallax_depth()));
    if enabled() {
        info!("authored materials summary: {}", stats().line());
    } else {
        info!("authored materials summary: off (setting)");
    }
}

/// Log once per set that its `_n` was not applied to a shape without tangents.
fn no_tangents_once(stem: &str) {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    if SEEN.get_or_init(Default::default).lock().expect("authored tangents").insert(stem.to_string()) {
        info!("authored materials: {stem}_n.dds not applied where the drawn mesh has no tangents (the game's normal state is kept)");
    }
}

/// The warning for a linear member whose DX10 header names an `_SRGB` format (once per file:
/// the decode of a (path, role) happens once).
pub(crate) fn srgb_warning(memo: &Memo, path: &str, usage: TextureUsage, srgb_code: bool) {
    if srgb_code && usage != TextureUsage::Color && path.starts_with(fnv_pbr::PBR_DIR) {
        memo.warn_once(path, || format!("authored materials: {path} names an sRGB format in its DX10 header; it is read as linear data, as its role is"));
    }
}


/// The run's Physical PBR lighting state (the settings row `physical-pbr`, nvforever
/// graphics.rs; docs/design/graphics-enhanced.md §3.25), fixed before the first shader hook
/// and material: off unless the start set it ([`set_physical_pbr`]).
static PHYSICAL_PBR: OnceLock<bool> = OnceLock::new();

/// Fix the run's Physical PBR state before the first shader hook and material are made. A
/// later call with another value is logged and ignored: the row takes effect at the next
/// start.
pub fn set_physical_pbr(on: bool) {
    if PHYSICAL_PBR.set(on).is_err() && physical_pbr() != on {
        warn!("physical PBR: already {} for this run; the setting takes effect at the next start", if physical_pbr() { "on" } else { "off" });
    }
}

/// Whether this run lights the world physically (Oren-Nayar diffuse and Cook-Torrance GGX
/// specular with Schlick Fresnel, image-based light from the sky, linear fog and exposure)
/// in place of the display-value lighting sum. Off unless [`set_physical_pbr`] said so.
pub fn physical_pbr() -> bool {
    *PHYSICAL_PBR.get_or_init(|| false)
}

#[cfg(test)]
pub(crate) mod tests;
