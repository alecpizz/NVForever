//! Environment maps (docs/spec-notes/nif.md §6 "Window glass and environment maps",
//! "Environment maps on other shapes"): what the material build needs beyond the shape's
//! own properties (`nif_spawn::shape_env`) and the debug switches for the unknowns (a
//! distance fade among them, off by default).
//!
//! A lit shape (BSShaderPPLightingProperty) with BSShaderFlags bit 7 Environment_Mapping
//! [documented: nif.xml flag names], a texture-set slot-4 cube that loads as a cube map
//! and an `environment_map_scale` above 0 reflects that cube, whether or not bit 21
//! Window_Environment_Mapping is set. Whether it is set is kept as its own property
//! ([`ShapeEnv::window`]): the GECK wiki says the window flag "simply flips the cubemap
//! orientation in Fallout 3 and New Vegas" [documented: GECK wiki "NIF Shader
//! Properties"], without saying which way, so the two sets can be given different cube
//! orientations ([`EnvAids::axes`], [`EnvAids::axes_window`]).
//!
//! Light: by default the reflection of a shape without the window flag is scaled by the
//! scene's light ([`scene_light`]: the ambient colour plus the sun's or moon's, or an
//! interior's ambient plus directional colour, as display values, held to 1 per channel)
//! [hypothesis, from captures of the original (jobs envmap2/envmap3, Primm, 2026-11-16):
//! a streak on a static bottle kept its screen place under sun, fire and moon, took the
//! light's colour and fell to about a third from day to night; a fit (a hypothesis)
//! against NVForever's unscaled term asked for 3.75-7.75 times the term on that bottle by
//! day and about none at night; observer check 275]. By day L is at most 1, so the
//! default can only dim the strength-1 term, while the fit asks for up to 8.5 times the
//! term on some objects (the other Sarsaparilla bottle): the day strength is open and
//! deliberately left at 1. L follows the sun
//! light's fade near the horizon, so around sunrise and sunset the reflection dips
//! towards the ambient part alone. Window shapes keep #2462's unscaled term by default.
//!
//! Debug switches (development aids for comparing frames with the original's, not part
//! of a release; read once at start; the run stays attended, as with every
//! `NVF_PLAY_DEBUG_*` switch, nvf-play `test_aid.rs`). Each defaults to the renderer's own
//! choice. Unlike the repo's other debug switches, `NON_WINDOW` is on by default and is
//! turned off by `0`. A set but empty value counts as not set
//! for every switch, as it does elsewhere (`dialogue::debug_switch`), so it keeps the
//! default:
//!
//! - `NVF_PLAY_DEBUG_ENV_NON_WINDOW=0`: shapes without the window flag do not reflect
//!   (the pictures of #2669, from the same build). Default on.
//! - `NVF_PLAY_DEBUG_ENV_STRENGTH=<factor>`: every reflection × the factor (0 or more;
//!   window shapes too). Default 1.
//! - `NVF_PLAY_DEBUG_ENV_NO_MASK=full|none|normal-alpha`: a reflecting shape whose set
//!   names no slot-5 mask reflects at full strength, not at all, or masked by its normal
//!   map's alpha (full when the normal map has no alpha channel). Window shapes too.
//!   Default `full`.
//! - `NVF_PLAY_DEBUG_ENV_AXES=<orientation>` (shapes without the window flag) and
//!   `NVF_PLAY_DEBUG_ENV_AXES_WINDOW=<orientation>` (window shapes): `base` (the lookup of
//!   #2669: the Bevy-space reflection (x, y, z) read as (x, y, −z)), `flip-x`, `flip-y`,
//!   `flip-z` (that direction with one axis negated) or `swap-yz` (its y and z swapped).
//!   Defaults: `swap-yz` without the window flag (the game-space reflection read as it is,
//!   #4392), `base` for windows.
//! - `NVF_PLAY_DEBUG_ENV_NORMAL=vertex|mapped` (shapes without the window flag) and
//!   `NVF_PLAY_DEBUG_ENV_NORMAL_WINDOW=…` (window shapes): the reflection is taken at the
//!   shape's interpolated normal or at the normal-mapped one. Defaults `vertex` (#4392) and
//!   `mapped` (#2669's lookup).
//! - `NVF_PLAY_DEBUG_ENV_BLEND=weighted|premultiplied` (shapes without the window flag)
//!   and `NVF_PLAY_DEBUG_ENV_BLEND_WINDOW=weighted|premultiplied` (window shapes): a
//!   blended reflecting shape keeps its blend, so the reflection is weighted by its alpha
//!   like the rest of its colour, or is drawn premultiplied, the reflection added over the
//!   blend. Defaults `weighted` and `premultiplied` (#2669's rule for windows).
//! - `NVF_PLAY_DEBUG_ENV_LIGHT=scene|off|all|fade-flag` (shapes without the window flag)
//!   and `NVF_PLAY_DEBUG_ENV_LIGHT_WINDOW=…` (window shapes, same values): `scene` scales
//!   the reflection by the scene light ([`scene_light`]); `off` leaves it unscaled; `all`
//!   scales it by the light the fragment receives (its lit colour's luminance over its
//!   base colour's, at most 1), and `fade-flag` does that only on shapes whose
//!   BSShaderFlags2 bit 15 is set ("Env Map Fade" [documented: GECK wiki "NIF Shader
//!   Properties"]; the captures showed no difference with the flag). `all` and
//!   `fade-flag` are rough stand-ins, **not** models of the original. Defaults `scene`
//!   and `off`.
//! - `NVF_PLAY_DEBUG_ENV_DISTANCE_FADE=1`: reflections fade with distance
//!   ([`ENV_MAP_LOD1`], [`ENV_MAP_LOD2`]; by the distance from the camera to the shape
//!   node's origin). Default off [design, #2841]: the fade would change window shapes too
//!   (#2462's pictures) and is not observed (observer check 276), so it stays out of the
//!   default until a change of its own takes it up.

use bevy::prelude::*;
use std::sync::OnceLock;

/// Camera distance (game units, which are Bevy's world units here) at which reflections
/// start to fade: `fEnvMapLOD1` = 1500, "Distance at which Environment mapping effects
/// (fake environment reflections) start to fade on objects" [documented: GECK wiki "INI
/// Settings", Display section]; the running original reported 1500 [observed: capture
/// job envmap, GetNumericINISetting]. NVForever reads no INI value for it [design]. The
/// fade itself is not observed: setting both values to 1 in the running original changed
/// no picture at 51-1,200 units [observed: capture job envmap5], so either the setting
/// does not apply live or the fade does not act there; observer check 276. Used only
/// under `NVF_PLAY_DEBUG_ENV_DISTANCE_FADE=1` (module docs).
pub const ENV_MAP_LOD1: f32 = 1500.0;

/// Camera distance past which there is no reflection: `fEnvMapLOD2` = 1800, "Max distance
/// to render Environment mapping effects (fake environment reflections) on objects"
/// [documented: GECK wiki "INI Settings", Display section; the running original reported
/// 1800, observed: capture job envmap]. Between the two the reflection fades linearly,
/// by the distance from the camera to the shape node's origin (its mesh instance's
/// translation, usually the reference's), not to each fragment; a model whose parts sit
/// under nodes of their own (the HELIOS One reflector row) fades part by part
/// [hypothesis: the wiki gives the two distances and says "on objects", not the curve;
/// observer check 276].
pub const ENV_MAP_LOD2: f32 = 1800.0;

/// The environment-map inputs of a lit shader property with Environment_Mapping
/// (`nif_spawn::env_flags`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EnvFlags {
    /// `environment_map_scale` (1 when the field is absent).
    pub scale: f32,
    /// BSShaderFlags bit 21 Window_Environment_Mapping is set too
    /// (`nif_spawn::window_env_flags`).
    pub window: bool,
    /// BSShaderFlags2 bit 15 [documented: nif.xml `envmap_light_fade`; GECK wiki "NIF
    /// Shader Properties": "Env Map Fade: Fades the cubemap's brightness based on the
    /// distance to the nearest light"].
    pub light_fade: bool,
}

/// A shape's environment map (`nif_spawn::shape_env`): the cube (slot 4), the mask
/// (slot 5) if the set names one, and the shader's flags.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShapeEnv {
    pub cube: String,
    pub mask: Option<String>,
    pub scale: f32,
    pub window: bool,
    pub light_fade: bool,
}

/// What a shape without a slot-5 mask reflects with (`NVF_PLAY_DEBUG_ENV_NO_MASK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoMask {
    /// Full strength [hypothesis: the default; nothing documents the case].
    Full,
    /// No reflection.
    None,
    /// The normal map's alpha (documented as gloss/specular, not as the environment
    /// mask; a variant to compare).
    NormalAlpha,
}

/// How the mask is read in the shader (`NifSurface::env` y).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MaskMode {
    /// No mask: 1.
    Full = 0,
    /// The slot-5 mask's red channel (#2669).
    Slot5 = 1,
    /// The normal map's alpha (the debug switch's variant): the game's own normal map is
    /// bound as the mask and its alpha read, so an authored set's `_n`, which replaces the
    /// drawn normal map (`crate::authored::apply`), does not change the comparison.
    NormalAlpha = 2,
}

/// The cube lookup's orientation (`NVF_PLAY_DEBUG_ENV_AXES`, `_AXES_WINDOW`); the value
/// is the shader's code (`NifSurface::env_how` x).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axes {
    /// #2669's lookup: the Bevy-space reflection (x, y, z) read as (x, y, −z), i.e. the
    /// game-space direction (x, y, z) as (x, z, y) [hypothesis in #2669].
    Base = 0,
    FlipX = 1,
    FlipY = 2,
    FlipZ = 3,
    /// The game-space direction (x, y, z) as it is: Bevy's (x, y, z) read as (x, −z, y)
    /// [hypothesis for shapes without the window flag, #4392; nif.md §6].
    SwapYZ = 4,
}

/// The normal the view is reflected at (`NVF_PLAY_DEBUG_ENV_NORMAL`, `_NORMAL_WINDOW`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvNormal {
    /// The interpolated vertex normal [hypothesis for shapes without the window flag, #4392].
    Vertex,
    /// The normal-mapped one (#2669).
    Mapped,
}

/// How a blended reflecting shape is drawn (`NVF_PLAY_DEBUG_ENV_BLEND`, `_BLEND_WINDOW`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvBlend {
    /// It keeps its blend: the reflection is weighted by the alpha.
    Weighted,
    /// Premultiplied: lit colour × alpha + reflection (#2669's rule for windows).
    Premultiplied,
}

/// What scales the reflection (`NVF_PLAY_DEBUG_ENV_LIGHT`, `_LIGHT_WINDOW`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvLight {
    /// The scene light ([`scene_light`]); the default for shapes without the window flag.
    Scene,
    /// Nothing; the default for window shapes (#2462).
    Off,
    /// The fragment's received light (a stand-in).
    All,
    /// Only shapes with BSShaderFlags2 bit 15 ([`EnvFlags::light_fade`]).
    FadeFlag,
}

/// The debug switches of the module docs, with their defaults.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvAids {
    pub non_window: bool,
    pub strength: f32,
    pub no_mask: NoMask,
    pub axes: Axes,
    pub axes_window: Axes,
    pub normal: EnvNormal,
    pub normal_window: EnvNormal,
    pub blend: EnvBlend,
    pub blend_window: EnvBlend,
    pub light: EnvLight,
    pub light_window: EnvLight,
    pub distance_fade: bool,
}

impl Default for EnvAids {
    fn default() -> Self {
        EnvAids {
            non_window: true,
            strength: 1.0,
            no_mask: NoMask::Full,
            axes: Axes::SwapYZ,
            axes_window: Axes::Base,
            normal: EnvNormal::Vertex,
            normal_window: EnvNormal::Mapped,
            blend: EnvBlend::Weighted,
            blend_window: EnvBlend::Premultiplied,
            light: EnvLight::Scene,
            light_window: EnvLight::Off,
            distance_fade: false,
        }
    }
}

/// The names of the debug switches, in the order the module docs give them.
pub const ENV_SWITCHES: [&str; 12] = [
    "NVF_PLAY_DEBUG_ENV_NON_WINDOW",
    "NVF_PLAY_DEBUG_ENV_STRENGTH",
    "NVF_PLAY_DEBUG_ENV_NO_MASK",
    "NVF_PLAY_DEBUG_ENV_AXES",
    "NVF_PLAY_DEBUG_ENV_AXES_WINDOW",
    "NVF_PLAY_DEBUG_ENV_NORMAL",
    "NVF_PLAY_DEBUG_ENV_NORMAL_WINDOW",
    "NVF_PLAY_DEBUG_ENV_BLEND",
    "NVF_PLAY_DEBUG_ENV_BLEND_WINDOW",
    "NVF_PLAY_DEBUG_ENV_LIGHT",
    "NVF_PLAY_DEBUG_ENV_LIGHT_WINDOW",
    "NVF_PLAY_DEBUG_ENV_DISTANCE_FADE",
];

fn on_off(v: &str) -> Option<bool> {
    match v {
        "1" | "on" | "true" | "yes" => Some(true),
        "0" | "off" | "false" | "no" => Some(false),
        _ => None,
    }
}

fn normal(v: &str) -> Option<EnvNormal> {
    match v {
        "vertex" => Some(EnvNormal::Vertex),
        "mapped" => Some(EnvNormal::Mapped),
        _ => None,
    }
}

fn axes(v: &str) -> Option<Axes> {
    match v {
        "base" => Some(Axes::Base),
        "flip-x" => Some(Axes::FlipX),
        "flip-y" => Some(Axes::FlipY),
        "flip-z" => Some(Axes::FlipZ),
        "swap-yz" => Some(Axes::SwapYZ),
        _ => None,
    }
}

fn blend(v: &str) -> Option<EnvBlend> {
    match v {
        "weighted" => Some(EnvBlend::Weighted),
        "premultiplied" => Some(EnvBlend::Premultiplied),
        _ => None,
    }
}

fn no_mask(v: &str) -> Option<NoMask> {
    match v {
        "full" => Some(NoMask::Full),
        "none" => Some(NoMask::None),
        "normal-alpha" => Some(NoMask::NormalAlpha),
        _ => None,
    }
}

fn light(v: &str) -> Option<EnvLight> {
    match v {
        "scene" => Some(EnvLight::Scene),
        "off" => Some(EnvLight::Off),
        "all" => Some(EnvLight::All),
        "fade-flag" => Some(EnvLight::FadeFlag),
        _ => None,
    }
}

fn strength(v: &str) -> Option<f32> {
    v.parse::<f32>().ok().filter(|f| f.is_finite() && *f >= 0.0)
}

/// Put `value` in `slot` if there is one; whether there was.
fn put<T>(slot: &mut T, value: Option<T>) -> bool {
    match value {
        Some(v) => {
            *slot = v;
            true
        }
        None => false,
    }
}

impl EnvAids {
    /// The switches from `get` (a variable's value by name), and a complaint for each
    /// value not understood (that switch keeps its default).
    pub fn parse(get: impl Fn(&str) -> Option<String>) -> (EnvAids, Vec<String>) {
        let mut aids = EnvAids::default();
        let mut bad = Vec::new();
        for name in ENV_SWITCHES {
            let Some(raw) = get(name) else { continue };
            let v = raw.trim().to_ascii_lowercase();
            let v = v.as_str();
            if v.is_empty() {
                continue;
            }
            let ok = match name {
                "NVF_PLAY_DEBUG_ENV_NON_WINDOW" => put(&mut aids.non_window, on_off(v)),
                "NVF_PLAY_DEBUG_ENV_STRENGTH" => put(&mut aids.strength, strength(v)),
                "NVF_PLAY_DEBUG_ENV_NO_MASK" => put(&mut aids.no_mask, no_mask(v)),
                "NVF_PLAY_DEBUG_ENV_AXES" => put(&mut aids.axes, axes(v)),
                "NVF_PLAY_DEBUG_ENV_AXES_WINDOW" => put(&mut aids.axes_window, axes(v)),
                "NVF_PLAY_DEBUG_ENV_NORMAL" => put(&mut aids.normal, normal(v)),
                "NVF_PLAY_DEBUG_ENV_NORMAL_WINDOW" => put(&mut aids.normal_window, normal(v)),
                "NVF_PLAY_DEBUG_ENV_BLEND" => put(&mut aids.blend, blend(v)),
                "NVF_PLAY_DEBUG_ENV_BLEND_WINDOW" => put(&mut aids.blend_window, blend(v)),
                "NVF_PLAY_DEBUG_ENV_LIGHT" => put(&mut aids.light, light(v)),
                "NVF_PLAY_DEBUG_ENV_LIGHT_WINDOW" => put(&mut aids.light_window, light(v)),
                "NVF_PLAY_DEBUG_ENV_DISTANCE_FADE" => put(&mut aids.distance_fade, on_off(v)),
                _ => true,
            };
            if !ok {
                bad.push(format!("{name}={raw}"));
            }
        }
        (aids, bad)
    }

    /// The cube orientation for a window shape or another.
    pub fn axes_for(&self, window: bool) -> Axes {
        if window { self.axes_window } else { self.axes }
    }

    /// The normal a window shape's reflection, or another's, is taken at.
    pub fn normal_for(&self, window: bool) -> EnvNormal {
        if window { self.normal_window } else { self.normal }
    }

    /// What scales the reflection of a window shape or another.
    pub fn light_for(&self, window: bool) -> EnvLight {
        if window { self.light_window } else { self.light }
    }

    /// The blend rule for a window shape or another.
    pub fn blend_for(&self, window: bool) -> EnvBlend {
        if window { self.blend_window } else { self.blend }
    }
}

/// The debug switches of this run, read from the environment once; a line in the log
/// when any is set.
pub fn aids() -> &'static EnvAids {
    static AIDS: OnceLock<EnvAids> = OnceLock::new();
    AIDS.get_or_init(|| {
        let (aids, bad) = EnvAids::parse(|name| std::env::var(name).ok());
        for b in &bad {
            warn!("debug switch {b}: value not understood, the default is kept (env_map.rs)");
        }
        if aids != EnvAids::default() {
            info!("debug switch NVF_PLAY_DEBUG_ENV_*: environment maps drawn with {aids:?}");
        }
        aids
    })
}

/// Which mask a reflecting shape is drawn with: its slot-5 mask when it loaded, else as
/// `no_mask` says; `None`: no reflection (`NoMask::None`). `normal_alpha` tells whether
/// the shape has a normal map with an alpha channel to read.
pub(crate) fn mask_mode(has_mask: bool, no_mask: NoMask, normal_alpha: bool) -> Option<MaskMode> {
    if has_mask {
        return Some(MaskMode::Slot5);
    }
    match no_mask {
        NoMask::Full => Some(MaskMode::Full),
        NoMask::None => None,
        NoMask::NormalAlpha => Some(if normal_alpha { MaskMode::NormalAlpha } else { MaskMode::Full }),
    }
}

/// The alpha mode of a shape that reflects (`window`: whether it has the window flag;
/// `None`: it does not reflect): a blended one is drawn premultiplied where the blend
/// rule for its set says so ([`EnvAids::blend_for`]), otherwise it keeps its mode.
/// Window shapes are premultiplied by default (#2669; observer check 249); other shapes
/// keep their blend [hypothesis, the conservative choice: hair, cell fades, blended
/// cut-outs and material animation key on `AlphaMode::Blend`; observer check 274].
pub(crate) fn env_alpha_mode(alpha_mode: AlphaMode, window: Option<bool>, aids: &EnvAids) -> AlphaMode {
    match window {
        Some(w) if aids.blend_for(w) == EnvBlend::Premultiplied && matches!(alpha_mode, AlphaMode::Blend) => AlphaMode::Premultiplied,
        _ => alpha_mode,
    }
}

/// The two values a reflecting shape's material hands the shaders (`NifSurface::env`,
/// `NifSurface::env_how`):
/// - env: x the scale × the strength switch, y the [`MaskMode`], z and w the distances
///   of the fade ([`ENV_MAP_LOD1`], [`ENV_MAP_LOD2`]; both 0: no fade);
/// - env_how: x the [`Axes`] code, y what scales the reflection (0 nothing, 1 the
///   fragment's received light, 2 the scene light), z 1 for a window shape, w 1 on a
///   reflecting shape read at the shading normal, 2 on one read at the vertex normal ([`EnvNormal`];
///   the shader premultiplies a Premultiplied one by it, even where the
///   reflection has faded out or its strength is 0).
pub(crate) fn env_uniforms(env: &ShapeEnv, mask: MaskMode, aids: &EnvAids) -> (Vec4, Vec4) {
    let (start, end) = if aids.distance_fade { (ENV_MAP_LOD1, ENV_MAP_LOD2) } else { (0.0, 0.0) };
    let light = match aids.light_for(env.window) {
        EnvLight::Scene => 2.0,
        EnvLight::Off => 0.0,
        EnvLight::All => 1.0,
        EnvLight::FadeFlag if env.light_fade => 1.0,
        EnvLight::FadeFlag => 0.0,
    };
    (
        Vec4::new(env.scale * aids.strength, mask as u8 as f32, start, end),
        Vec4::new(aids.axes_for(env.window) as u8 as f32, light, if env.window { 1.0 } else { 0.0 }, if aids.normal_for(env.window) == EnvNormal::Vertex { 2.0 } else { 1.0 }),
    )
}

/// The share of the reflection left on a shape whose node's origin (its mesh instance's
/// world position, usually the reference's) is `origin`, seen from `camera`, with the
/// fade distances `start` and `end` (env z and w): 1 up to `start`, 0 from `end`, linear
/// between; 1 when `end` is not past `start` (no fade). Every fragment of the shape gets
/// the same share; the parts of a model under other nodes fade by their own. The CPU
/// mirror of nif_material.wesl's fade.
pub fn distance_fade(camera: Vec3, origin: Vec3, start: f32, end: f32) -> f32 {
    if end <= start {
        return 1.0;
    }
    1.0 - ((camera.distance(origin) - start) / (end - start)).clamp(0.0, 1.0)
}

/// The scene light that scales a reflection by default ([`EnvLight::Scene`]); the CPU
/// mirror of nif_material.wesl's `env_scene_light`. From the view's light uniforms: the
/// ambient colour (`ambient`, its alpha `ambient_alpha`) and each directional light's
/// colour, as Bevy hands them to the shader, and the view's exposure. In a world view
/// (negative ambient alpha, nvforever lights.rs `display_ambient`) those hold display
/// values × the light unit (1 / exposure; × π for a directional light, sky.rs
/// `light_unit`), so the sum of ambient + every directional light, held to 0..1 per
/// channel, is the display-value light of the scene without any N·L: outdoors the
/// weather's ambient and sun or moon colours as NVForever uses them (× the shares of
/// sky.rs `AMBIENT_SHARE`, `SUN_SHARE`, `MOON_SHARE`, with the lightning flash and the
/// sunlight's fade, not the records' bytes as stored), indoors the cell's ambient and
/// directional colours (× their fade). Point lights are left out [hypothesis: no evidence
/// either way]. Other views (minigames, the Pip-Boy's own views) have no display-value
/// lights: 1, the term unscaled.
pub fn scene_light(ambient: Vec3, ambient_alpha: f32, directional: &[Vec3], exposure: f32) -> Vec3 {
    if ambient_alpha >= 0.0 {
        return Vec3::ONE;
    }
    let sum = directional.iter().fold(ambient * exposure, |s, &d| s + d * (exposure / std::f32::consts::PI));
    sum.clamp(Vec3::ZERO, Vec3::ONE)
}

/// The direction the cube is read in, in Bevy's axes as sent to the sampler; the CPU mirror of
/// nif_material.wesl's `env_lookup`: `v` towards the viewer, `shading` the normal-mapped normal,
/// `interpolated` the vertex one, `axes` and `mark` as in `NifSurface::env_how` x and w.
pub fn lookup(v: Vec3, shading: Vec3, interpolated: Vec3, axes: f32, mark: f32) -> Vec3 {
    let n = match mark > 1.5 {
        true if interpolated.length_squared() > 1e-12 => interpolated / interpolated.length(),
        true => v,
        false => shading,
    };
    let r = -v - 2.0 * n.dot(-v) * n;
    match axes {
        a if a > 3.5 => Vec3::new(r.x, -r.z, r.y),
        a if a > 2.5 => r,
        a if a > 1.5 => Vec3::new(r.x, -r.y, -r.z),
        a if a > 0.5 => Vec3::new(-r.x, r.y, -r.z),
        _ => Vec3::new(r.x, r.y, -r.z),
    }
}

/// How a shape without the window flag is drawn with its environment map, for the
/// model's log line (model.rs): the cube, the scale, the mask and the blend.
pub(crate) fn describe(cube: &str, material: &crate::nif_material::NifMaterial) -> String {
    let (env, how) = (material.extension.env, material.extension.env_how);
    let mask = match env.y {
        y if y > 1.5 => "normal-map alpha as mask",
        y if y > 0.5 => "masked",
        _ => "no mask",
    };
    let blend = match material.base.alpha_mode {
        AlphaMode::Premultiplied => "over the blend",
        AlphaMode::Blend => "weighted by alpha",
        AlphaMode::Mask(_) => "alpha-tested",
        AlphaMode::Opaque => "opaque",
        _ => "other blend",
    };
    let light = match how.y {
        y if y > 1.5 => ", by scene light",
        y if y > 0.5 => ", by received light",
        _ => "",
    };
    format!("{cube}, scale {:.2}, {mask}, {blend}{light}", env.x)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A variable lookup over fixed pairs.
    fn set(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |n: &str| pairs.iter().find(|(k, _)| *k == n).map(|(_, v)| v.to_string())
    }

    fn env(window: bool, light_fade: bool) -> ShapeEnv {
        ShapeEnv { cube: "textures\\effects\\shinybright_e.dds".into(), mask: None, scale: 1.2, window, light_fade }
    }

    /// The switches' defaults are the renderer's own choices; values not understood keep
    /// the default and are reported.
    #[test]
    fn switches_parse_with_defaults() {
        let (none, bad) = EnvAids::parse(|_| None);
        assert_eq!(none, EnvAids::default());
        assert!(bad.is_empty());
        let (a, bad) = EnvAids::parse(set(&[
            ("NVF_PLAY_DEBUG_ENV_NON_WINDOW", "0"),
            ("NVF_PLAY_DEBUG_ENV_STRENGTH", "0.5"),
            ("NVF_PLAY_DEBUG_ENV_NO_MASK", "normal-alpha"),
            ("NVF_PLAY_DEBUG_ENV_AXES", "flip-y"),
            ("NVF_PLAY_DEBUG_ENV_AXES_WINDOW", "flip-z"),
            ("NVF_PLAY_DEBUG_ENV_NORMAL", "mapped"),
            ("NVF_PLAY_DEBUG_ENV_NORMAL_WINDOW", "vertex"),
            ("NVF_PLAY_DEBUG_ENV_BLEND", "premultiplied"),
            ("NVF_PLAY_DEBUG_ENV_BLEND_WINDOW", "weighted"),
            ("NVF_PLAY_DEBUG_ENV_LIGHT", "fade-flag"),
            ("NVF_PLAY_DEBUG_ENV_LIGHT_WINDOW", "scene"),
            ("NVF_PLAY_DEBUG_ENV_DISTANCE_FADE", "on"),
        ]));
        assert!(bad.is_empty());
        assert_eq!(
            a,
            EnvAids {
                non_window: false,
                strength: 0.5,
                no_mask: NoMask::NormalAlpha,
                axes: Axes::FlipY,
                axes_window: Axes::FlipZ,
                normal: EnvNormal::Mapped,
                normal_window: EnvNormal::Vertex,
                blend: EnvBlend::Premultiplied,
                blend_window: EnvBlend::Weighted,
                light: EnvLight::FadeFlag,
                light_window: EnvLight::Scene,
                distance_fade: true
            }
        );
        let (b, bad) = EnvAids::parse(set(&[("NVF_PLAY_DEBUG_ENV_STRENGTH", "-1"), ("NVF_PLAY_DEBUG_ENV_AXES", "sideways")]));
        assert_eq!(b, EnvAids::default());
        assert_eq!(bad.len(), 2);
        // A set but empty value counts as not set, for every switch (as
        // `dialogue::debug_switch` reads one), without a complaint.
        let (c, bad) = EnvAids::parse(set(&[("NVF_PLAY_DEBUG_ENV_NON_WINDOW", ""), ("NVF_PLAY_DEBUG_ENV_LIGHT", " "), ("NVF_PLAY_DEBUG_ENV_DISTANCE_FADE", "")]));
        assert!(bad.is_empty());
        assert_eq!(c, EnvAids::default());
        assert_eq!((EnvAids::default().light, EnvAids::default().light_window), (EnvLight::Scene, EnvLight::Off));
        assert!(!EnvAids::default().distance_fade, "the distance fade is off by default: it would change window shapes too");
    }

    /// A slot-5 mask always wins; without one the switch decides, and the normal-alpha
    /// variant falls back to full strength when the normal map has no alpha.
    #[test]
    fn mask_mode_follows_the_mask_then_the_switch() {
        assert_eq!(mask_mode(true, NoMask::None, false), Some(MaskMode::Slot5));
        assert_eq!(mask_mode(false, NoMask::Full, true), Some(MaskMode::Full));
        assert_eq!(mask_mode(false, NoMask::None, true), None);
        assert_eq!(mask_mode(false, NoMask::NormalAlpha, true), Some(MaskMode::NormalAlpha));
        assert_eq!(mask_mode(false, NoMask::NormalAlpha, false), Some(MaskMode::Full));
    }

    /// By default a blended window shape is drawn premultiplied (#2669) and a blended
    /// shape without the window flag keeps its blend; opaque and alpha-tested shapes and
    /// shapes that do not reflect are never changed.
    #[test]
    fn blend_rule_by_set() {
        let d = EnvAids::default();
        assert_eq!(env_alpha_mode(AlphaMode::Blend, Some(true), &d), AlphaMode::Premultiplied);
        assert_eq!(env_alpha_mode(AlphaMode::Blend, Some(false), &d), AlphaMode::Blend);
        assert_eq!(env_alpha_mode(AlphaMode::Blend, None, &d), AlphaMode::Blend);
        assert_eq!(env_alpha_mode(AlphaMode::Opaque, Some(true), &d), AlphaMode::Opaque);
        assert_eq!(env_alpha_mode(AlphaMode::Mask(0.5), Some(true), &d), AlphaMode::Mask(0.5));
        let swapped = EnvAids { blend: EnvBlend::Premultiplied, blend_window: EnvBlend::Weighted, ..d };
        assert_eq!(env_alpha_mode(AlphaMode::Blend, Some(true), &swapped), AlphaMode::Blend);
        assert_eq!(env_alpha_mode(AlphaMode::Blend, Some(false), &swapped), AlphaMode::Premultiplied);
    }

    /// The fade: whole to fEnvMapLOD1, none from fEnvMapLOD2, linear between, by the
    /// shape node's origin; no fade without the distances.
    #[test]
    fn distance_fade_is_linear_between_the_two_distances() {
        // (A shape's node origin, as in the shader.)
        let fade = |d: f32| distance_fade(Vec3::ZERO, Vec3::new(0.0, 0.0, -d), ENV_MAP_LOD1, ENV_MAP_LOD2);
        assert_eq!(fade(0.0), 1.0);
        assert_eq!(fade(1500.0), 1.0);
        assert!((fade(1650.0) - 0.5).abs() < 1e-4);
        assert_eq!(fade(1800.0), 0.0);
        assert_eq!(fade(40_000.0), 0.0);
        assert_eq!(distance_fade(Vec3::ZERO, Vec3::splat(40_000.0), 0.0, 0.0), 1.0);
    }

    /// Per shape node, not per fragment: a 1,000-unit counter whose node origin stands
    /// 1,000 units away fades as a whole by it, so its far end (2,000 units off) still
    /// reflects fully and its whole length is faded alike once the origin passes 1,500
    /// (the shader takes the mesh instance's translation, `get_world_from_local(..)[3]`).
    #[test]
    fn distance_fade_is_per_object() {
        let camera = Vec3::new(10.0, 120.0, -30.0);
        let origin = camera + Vec3::new(0.0, 0.0, -1000.0);
        assert_eq!(distance_fade(camera, origin, ENV_MAP_LOD1, ENV_MAP_LOD2), 1.0, "the far end at 2,000 units is not faded on its own");
        let farther = camera + Vec3::new(1650.0, 0.0, 0.0);
        assert!((distance_fade(camera, farther, ENV_MAP_LOD1, ENV_MAP_LOD2) - 0.5).abs() < 1e-4);
        assert!(include_str!("nif_material.wesl").contains("get_world_from_local(in.instance_index)[3].xyz"), "the shader fades by the instance's origin");
    }

    /// A premultiplied environment-mapped shape (a blended window pane) is multiplied by
    /// its alpha whether or not it still reflects: the premultiply sits in the branch of
    /// the env-shape mark (env_how.w), before and outside the distance fade's gate, so a
    /// pane beyond fEnvMapLOD2 is not drawn at its full texture colour (review B1 of
    /// #2841: it once sat inside the gate).
    #[test]
    fn premultiply_is_outside_the_fade_gate() {
        let shader = include_str!("nif_material.wesl").replace("\r\n", "\n");
        let premultiply = "out.color.rgb * out.color.a, premultiplied)";
        assert_eq!(shader.matches(premultiply).count(), 1, "one premultiply of env shapes");
        let at = shader.find(premultiply).unwrap();
        let gate = shader.find("if surface.env.x > 0.0 && env_fade > 0.0 {").expect("the fade's gate");
        assert!(at < gate, "the premultiply comes before the fade's gate");
        let mark = shader[..at].rfind("if surface.env_how.w > 0.5 {").expect("the env-shape mark's branch");
        assert!(!shader[mark..at].contains('}'), "the premultiply is inside the mark's branch, nothing closed between");
        let gated = shader[gate..].split("\n    } else {").next().unwrap();
        assert!(!gated.contains("premultiplied"), "nothing premultiplies inside the gate");
    }

    /// The scene light: ambient + every directional light as display values (the uniforms
    /// hold display × light unit, × π for a directional light), held to 0..1 per channel;
    /// 1 in a view without display-value lights. Values written for the test.
    #[test]
    fn scene_light_sums_ambient_and_directional_and_clamps() {
        let exposure = 0.25;
        let unit = 1.0 / exposure;
        let pi = std::f32::consts::PI;
        // A day: ambient (0.20, 0.20, 0.25), sun (0.60, 0.55, 0.40) as display values.
        let day = scene_light(Vec3::new(0.20, 0.20, 0.25) * unit, -1.0, &[Vec3::new(0.60, 0.55, 0.40) * pi * unit], exposure);
        assert!((day - Vec3::new(0.80, 0.75, 0.65)).abs().max_element() < 1e-5);
        // A night: ambient (0.05, 0.06, 0.10), moon (0.08, 0.10, 0.15), and the arms'
        // shade light, a 1/1024 share of the moon.
        let moon = Vec3::new(0.08, 0.10, 0.15) * pi * unit;
        let night = scene_light(Vec3::new(0.05, 0.06, 0.10) * unit, -1.0, &[moon, moon / 1024.0], exposure);
        assert!((night - Vec3::new(0.13, 0.16, 0.25)).abs().max_element() < 1e-3);
        // Held to 1 per channel.
        let bright = scene_light(Vec3::new(0.7, 0.2, 0.0) * unit, -1.0, &[Vec3::new(0.6, 0.3, 0.0) * pi * unit], exposure);
        assert!((bright - Vec3::new(1.0, 0.5, 0.0)).abs().max_element() < 1e-5);
        // No lights at all: none; a view without the world mark: 1.
        assert_eq!(scene_light(Vec3::ZERO, -1.0, &[], exposure), Vec3::ZERO);
        assert_eq!(scene_light(Vec3::splat(0.1), 1.0, &[Vec3::ONE], exposure), Vec3::ONE);
    }

    /// The default uniforms of a window shape are #2669's (scale, mask flag, lookup, no
    /// fade); the switches change only what they name.
    #[test]
    fn uniforms_carry_the_set_and_the_switches() {
        let d = EnvAids::default();
        let (e, how) = env_uniforms(&env(true, false), MaskMode::Slot5, &d);
        assert_eq!(e, Vec4::new(1.2, 1.0, 0.0, 0.0), "no distance fade by default");
        assert_eq!(how, Vec4::new(0.0, 0.0, 1.0, 1.0));
        let (e, how) = env_uniforms(&env(false, true), MaskMode::Full, &d);
        assert_eq!(e, Vec4::new(1.2, 0.0, 0.0, 0.0));
        assert_eq!(how, Vec4::new(4.0, 2.0, 0.0, 2.0), "game axes, the vertex normal, scaled by the scene light by default");
        let a = EnvAids { strength: 2.0, axes: Axes::FlipX, normal: EnvNormal::Mapped, light: EnvLight::FadeFlag, distance_fade: true, ..d };
        let (e, how) = env_uniforms(&env(false, true), MaskMode::NormalAlpha, &a);
        assert_eq!(e, Vec4::new(2.4, 2.0, ENV_MAP_LOD1, ENV_MAP_LOD2), "the switch turns the fade on");
        assert_eq!(how, Vec4::new(1.0, 1.0, 0.0, 1.0));
        let (_, how) = env_uniforms(&env(true, true), MaskMode::Full, &a);
        assert_eq!(how, Vec4::new(0.0, 0.0, 1.0, 1.0), "the window orientation and no light: LIGHT names the other set");
        let (e, how) = env_uniforms(&env(true, false), MaskMode::Full, &EnvAids { strength: 0.0, ..d });
        assert_eq!((e.x, how.w), (0.0, 1.0), "strength 0 still marks the shape, so a premultiplied pane is premultiplied");
        let (_, how) = env_uniforms(&env(false, false), MaskMode::Full, &a);
        assert_eq!(how.y, 0.0, "fade-flag: no light on a shape without bit 15");
        let lit_windows = EnvAids { light_window: EnvLight::Scene, light: EnvLight::Off, ..d };
        assert_eq!(env_uniforms(&env(true, false), MaskMode::Full, &lit_windows).1.y, 2.0);
        assert_eq!(env_uniforms(&env(false, false), MaskMode::Full, &lit_windows).1.y, 0.0);
    }

    /// The default lookups (#4392): another shape reads the cube at the interpolated normal in
    /// the game's axes, a window at the shading normal in #2669's. Each half alone gives a third
    /// direction, so undoing either fails. The shader's function is the mirror's, line by line.
    #[test]
    fn the_lookup_takes_the_vertex_normal_and_the_game_axes() {
        let d = EnvAids::default();
        // Seen from above a floor whose normal map tilts it by 45 degrees.
        let (v, vertex, shading) = (Vec3::Y, Vec3::Y, Vec3::new(0.0, 1.0, 1.0).normalize());
        let at = |window: bool, aids: &EnvAids| {
            let (_, how) = env_uniforms(&env(window, false), MaskMode::Full, aids);
            lookup(v, shading, vertex, how.x, how.w)
        };
        let near = |a: Vec3, b: Vec3| assert!((a - b).length() < 1e-5, "{a} is not {b}");
        near(at(false, &d), Vec3::Z);
        near(at(true, &d), Vec3::NEG_Z);
        near(at(false, &EnvAids { axes: Axes::Base, ..d }), Vec3::Y);
        near(at(false, &EnvAids { normal: EnvNormal::Mapped, ..d }), Vec3::NEG_Y);
        // The game-space direction as it is: Bevy's (x, y, z) is the game's (x, -z, y).
        let r = Vec3::new(0.6, 0.0, 0.8);
        near(lookup(r, r, r, 4.0, 2.0), Vec3::new(0.6, -0.8, 0.0));
        // Either side of the normal; a normal of no length is not sampled as NaN.
        near(lookup(v, shading, -vertex, 4.0, 2.0), lookup(v, shading, vertex, 4.0, 2.0));
        assert!(lookup(v, shading, Vec3::ZERO, 4.0, 2.0).is_finite());
        let shader = include_str!("nif_material.wesl").replace("\r\n", "\n");
        for line in [
            "let dir = env_lookup(pbr_input.V, pbr_input.N, pbr_input.world_normal, surface.env_how.x, surface.env_how.w);",
            "    var n = shading;\n    if mark > 1.5 {",
            "n = select(v, interpolated * inverseSqrt(max(squared, 1e-12)), squared > 1e-12);",
            "let r = reflect(-v, n);",
            "if axes > 3.5 {\n        return vec3(r.x, -r.z, r.y);",
            "    return vec3(r.x, r.y, -r.z);\n}",
        ] {
            assert_eq!(shader.matches(line).count(), 1, "the shader's lookup: {line}");
        }
    }

    /// The log line's words for a shape without the window flag.
    #[test]
    fn describe_names_mask_and_blend() {
        let mut m = crate::nif_material::nif_material(StandardMaterial { alpha_mode: AlphaMode::Blend, ..default() });
        (m.extension.env, m.extension.env_how) = env_uniforms(&env(false, false), MaskMode::Slot5, &EnvAids::default());
        assert_eq!(describe("textures\\effects\\shinybright_e.dds", &m), "textures\\effects\\shinybright_e.dds, scale 1.20, masked, weighted by alpha, by scene light");
        m.base.alpha_mode = AlphaMode::Opaque;
        m.extension.env.y = 0.0;
        m.extension.env_how.y = 1.0;
        assert_eq!(describe("c_e.dds", &m), "c_e.dds, scale 1.20, no mask, opaque, by received light");
    }
}
