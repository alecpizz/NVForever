//! [design] NVForever's mesh/landscape blend (#2661, docs/design/graphics-enhanced.md
//! §3.14): rigid exterior STAT and SCOL shapes, no noise, immutable source materials.
//!
//! On with the Graphics settings row `mesh-terrain-blend` ("Blend meshes into ground";
//! off by default, off in both looks, not part of `enhanced`), which draws
//! [`setting_config`]: an 8-unit band of the `height` shape, 512 colour texels per cell
//! (round 10b; 1024 before), full shading. The test aid `NVF_PLAY_TEST_MESH_BLEND=[band:]view[:shape[:resolution
//! [:shading[:flat,steep]]]]` overrides the row for comparison runs (it is not stored);
//! omitted parts take the same defaults, and hybrid v2's widths are `band / 16` and
//! `6 band`. These are NVForever's own choices.
//!
//! [design] Live switching ([`follow_setting`]): turning the blend on opens its material
//! gate ([`BlendGate`]), records the census and the landscape captures of what is already
//! drawn (world.rs::blend_backfill, terrain_surface.rs::backfill_cells) and lets the cache
//! bake; turning it off puts every shape's ordinary material back ([`stop`], in PostUpdate
//! after the material producers), drops the census, the captures, the atlas and the
//! enabled clones, and closes the gate once their materials are gone ([`close_gate`]).
//! While it is off no bake is scheduled and nothing is allocated; what still runs is
//! listed in docs/design/graphics-enhanced.md §3.14.
//!
//! [design] Census (round 6): every spawned STAT or SCOL shape carries a [`BlendCensus`]
//! with the reasons its reference and shape cannot blend, recorded before any filtering
//! (world.rs::spawn_model); the conversion adds the material and ownership reasons in each
//! full pass ([`pass_needed`]) and reports counts on the ready line and bounded `census-shape` lines. The census
//! is separate from [`BlendShape`]: only shapes without reference or shape reasons get a
//! `BlendShape`, so tracking a shape never makes it convertible.

use crate::terrain_surface::{SurfaceCache, update_cache};
use bevy::light::NotShadowCaster;
use bevy::pbr::MaterialGate;
use bevy::prelude::*;
use nvf_assets::nif_material::{Falloff, MaterialOverride, NifMaterial, SeeThroughNoShadow};
use nvf_assets::nif_surface_blend::{ContactShape, ContactWidths, NifBlendMaterial, NifSurfaceBlend, Shading, DEFAULT_RESOLUTION, RESOLUTIONS};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant { Surface, Weight, Normal, Selection, Status }

impl Variant {
    fn name(self) -> &'static str {
        match self { Self::Surface => "surface", Self::Weight => "weight", Self::Normal => "normal", Self::Selection => "selection", Self::Status => "status" }
    }
    fn number(self) -> f32 {
        match self { Self::Surface => 0.0, Self::Weight => 1.0, Self::Normal => 2.0, Self::Selection => 3.0, Self::Status => 4.0 }
    }
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "surface" => Self::Surface, "weight" => Self::Weight, "normal" => Self::Normal,
            "selection" => Self::Selection, "status" => Self::Status, _ => return None,
        })
    }
}

/// [design] Defaults of the parts the aid's value leaves out, and the settings row's look
/// ([`setting_config`]). Round 7 made `height` the default shape: at a road shoulder it
/// turned the broken asphalt edge into ground-lit dirt and left the road top alone, where
/// the hybrid shapes lost the effect or striped the road's lip (the user's comparison
/// pictures of round 7 [observed], #2661). `hybrid`, `hybrid-v1` and `penetration` stay
/// for the aid.
pub const DEFAULT_BAND: f32 = 8.0;
pub const DEFAULT_SHAPE: ContactShape = ContactShape::Height;
pub const DEFAULT_SHADING: Shading = Shading::Full;

/// `flat` and `steep` are hybrid v2's widths (resolved: `band / 16` and `6 band` unless
/// the value gives them); the other shapes do not read them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlendConfig { pub band: f32, pub variant: Variant, pub shape: ContactShape, pub resolution: u32, pub shading: Shading, pub flat: f32, pub steep: f32 }

impl BlendConfig {
    /// The enabled material's controls (nif_surface_bindings.wesl `mesh_terrain`): band,
    /// view, contact shape, shading.
    pub fn controls(&self) -> Vec4 {
        Vec4::new(self.band, self.variant.number(), self.shape.number(), self.shading.number())
    }
    /// The enabled material's `mesh_terrain_widths`: flat and steep widths, z and w unused.
    pub fn width_controls(&self) -> Vec4 {
        Vec4::new(self.flat, self.steep, 0.0, 0.0)
    }
    #[cfg(test)]
    pub fn widths(&self) -> ContactWidths {
        ContactWidths { flat: self.flat, band: self.band, steep: self.steep }
    }
}

const SYNTAX: &str = "expected [positive band:]surface|weight|normal|selection|status[:height|penetration|hybrid|hybrid-v1[:512|1024|2048[:albedo|full[:flat,steep]]]]";
const WIDTHS: &str = "widths must be flat,steep: finite positive numbers with flat <= band <= steep, for the hybrid shape only";

/// [design] `[band:]view[:shape[:resolution[:shading[:flat,steep]]]]`. The first part is
/// the view when it names one, otherwise the band (empty: the default). An empty later
/// part takes its default, except the widths: a sixth part must be a `flat,steep` pair,
/// and only with the hybrid (v2) shape. Old short forms (`surface`, `:selection`,
/// `24:status`) keep their meaning, with the new defaults for what they leave out.
pub fn parse(value: &str) -> Result<BlendConfig, &'static str> {
    let mut parts: Vec<&str> = value.split(':').collect();
    let band = if Variant::parse(parts[0]).is_some() { DEFAULT_BAND } else {
        let band = parts.remove(0);
        if band.is_empty() { DEFAULT_BAND } else { band.parse::<f32>().map_err(|_| "band must be a finite positive number")? }
    };
    if !band.is_finite() || band <= 0.0 { return Err("band must be a finite positive number"); }
    let mut parts = parts.into_iter();
    let variant = parts.next().and_then(Variant::parse).ok_or(SYNTAX)?;
    let shape = match parts.next().unwrap_or("") {
        "" => DEFAULT_SHAPE, "height" => ContactShape::Height, "penetration" => ContactShape::Penetration,
        "hybrid" => ContactShape::Hybrid, "hybrid-v1" => ContactShape::HybridV1, _ => return Err(SYNTAX),
    };
    let resolution = match parts.next().unwrap_or("") {
        "" => DEFAULT_RESOLUTION,
        r => r.parse::<u32>().ok().filter(|r| RESOLUTIONS.contains(r)).ok_or("resolution must be 512, 1024 or 2048")?,
    };
    let shading = match parts.next().unwrap_or("") {
        "" => DEFAULT_SHADING, "albedo" => Shading::Albedo, "full" => Shading::Full, _ => return Err(SYNTAX),
    };
    let widths = match parts.next() {
        None => ContactWidths::from_band(band),
        Some(pair) => {
            if shape != ContactShape::Hybrid { return Err(WIDTHS); }
            let (flat, steep) = pair.split_once(',').ok_or(WIDTHS)?;
            let number = |w: &str| w.parse::<f32>().map_err(|_| WIDTHS);
            ContactWidths { flat: number(flat)?, band, steep: number(steep)? }
        }
    };
    if shape == ContactShape::Hybrid && !widths.valid() { return Err(WIDTHS); }
    if parts.next().is_some() { return Err(SYNTAX); }
    Ok(BlendConfig { band, variant, shape, resolution, shading, flat: widths.flat, steep: widths.steep })
}

/// The test aid's value, read once; `None` when it is not set or rejected (said once in the
/// log), and the settings row decides then.
pub fn aid() -> Option<BlendConfig> {
    static CONFIG: OnceLock<Option<BlendConfig>> = OnceLock::new();
    *CONFIG.get_or_init(|| {
        let value = match std::env::var("NVF_PLAY_TEST_MESH_BLEND") {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent) => return None,
            Err(_) => { warn!("mesh terrain blend: rejected NVF_PLAY_TEST_MESH_BLEND: value is not Unicode; the settings row decides"); return None; }
        };
        parse(&value).map_err(|e| warn!("mesh terrain blend: rejected NVF_PLAY_TEST_MESH_BLEND={value:?}: {e}; the settings row decides")).ok()
    })
}

/// [design] What the settings row draws: the aid's defaults, `8:surface:height:512:full`
/// (round 10b: 512 colour texels per cell, the colour cache being the fallback beyond the
/// landscape's own layers).
pub fn setting_config() -> BlendConfig {
    parse("surface").expect("the default value parses")
}

/// [design] The blend wanted now: the aid's value when given (comparison runs), else the
/// row's look while the row is on, else off.
pub fn desired(aid: Option<BlendConfig>, row: bool) -> Option<BlendConfig> {
    aid.or_else(|| row.then(setting_config))
}

/// Whether the blend is on, for the spawns that record its census and captures
/// (world.rs::blend_census, terrain.rs::spawn_terrain); set by [`follow_setting`] only.
static RECORDING: AtomicBool = AtomicBool::new(false);

/// [design] The blend is on: a spawn records its census and landscape captures now.
pub fn recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

/// [design] The blend's state while the game runs.
#[derive(Resource, Default)]
pub struct BlendState {
    /// The look drawn; `None` while off.
    pub active: Option<BlendConfig>,
    /// The census of the shapes drawn while it was off is still to be taken
    /// (world.rs::blend_backfill), set as it turns on.
    pub shapes_pending: bool,
    /// The cells whose capture was tried since it turned on
    /// (terrain_surface.rs::backfill_cells).
    pub tried: HashSet<Entity>,
    /// Switched off this frame: [`stop`] puts the ordinary materials back in PostUpdate.
    stopping: bool,
    /// Off, the material gate still open: frames in a row with no blend or capture
    /// material left ([`close_gate`]).
    draining: Option<u32>,
}

/// Run condition: the blend is on.
pub fn active(state: Res<BlendState>) -> bool {
    state.active.is_some()
}

fn stopping(state: Res<BlendState>) -> bool {
    state.stopping
}

fn draining(state: Res<BlendState>) -> bool {
    state.draining.is_some()
}

/// [design] The gate of the blend's two material types, the enabled clones'
/// [`NifBlendMaterial`] and the bake's `SurfaceCaptureMaterial`, and of the bake view's
/// extraction (vendored bevy_pbr `MaterialGate`, engine/vendor/README.md). Open from the
/// moment the blend turns on until [`close_gate`] finds nothing of either type left after
/// it turned off; closed in a run that never turns it on.
#[derive(Resource, Clone)]
pub struct BlendGate(pub MaterialGate);

/// Hybrid v2's resolved widths and the weight formula's version, appended to the start
/// and ready lines: ` flat=... steep=... weight-version=2`, or ` weight-version=1` for the
/// shapes round 6 left unchanged.
fn weight_fields(c: &BlendConfig) -> String {
    if c.shape == ContactShape::Hybrid {
        format!(" flat={} steep={} weight-version={}", c.flat, c.steep, c.shape.weight_version())
    } else {
        format!(" weight-version={}", c.shape.weight_version())
    }
}

/// The resolved configuration; `start band=... variant=...` stays the line's prefix and
/// round 5's fields keep their order before the appended weight fields; round 10a's
/// resolved landscape-detail mode ends it ([`DetailMode::fields`]).
fn start_line(c: &BlendConfig, detail: DetailMode) -> String {
    format!("mesh terrain blend: start band={} variant={} resolution={} noise=0 shape={} shading={}{}{}", c.band, c.variant.name(), c.resolution, c.shape.name(), c.shading.name(), weight_fields(c), detail.fields())
}

/// [design] Round 10a: whether publications sample the landscape's own layer textures
/// near the camera (`Layers`) or keep the colour cache alone, and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailMode { Layers, Colour(&'static str) }
impl DetailMode {
    /// ` detail=layers`, or ` detail=colour detail-reason=<aid|device|no-device>`.
    fn fields(self) -> String {
        match self { Self::Layers => " detail=layers".into(), Self::Colour(reason) => format!(" detail=colour detail-reason={reason}") }
    }
    pub fn layers(self) -> bool { self == Self::Layers }
}

/// [design] The test aid `NVF_PLAY_TEST_MESH_BLEND_DETAIL=colour` keeps the colour cache
/// alone for comparison runs (read once; any other value is refused in the log).
fn detail_aid() -> bool {
    static FORCED: OnceLock<bool> = OnceLock::new();
    *FORCED.get_or_init(|| match std::env::var("NVF_PLAY_TEST_MESH_BLEND_DETAIL") {
        Ok(v) if v == "colour" => true,
        Ok(v) => { warn!("mesh terrain blend: rejected NVF_PLAY_TEST_MESH_BLEND_DETAIL={v:?}: expected colour"); false }
        Err(_) => false,
    })
}

/// [design] Round 10a's capability: the blend material is bindless on this device (its
/// landscape palette lives in the bindless index table, nif_surface_blend.rs) and the
/// device indexes sampled-texture arrays non-uniformly (a fragment's quadrant picks the
/// layers). Otherwise, or with the aid, the colour cache alone.
pub fn detail_capability(device: Option<&bevy::render::renderer::RenderDevice>, forced_colour: bool) -> DetailMode {
    use bevy::render::settings::WgpuFeatures;
    if forced_colour { return DetailMode::Colour("aid"); }
    let Some(device) = device else { return DetailMode::Colour("no-device") };
    let bindless = bevy::render::material_bind_groups::material_uses_bindless_resources::<NifBlendMaterial>(device);
    if bindless && device.features().contains(WgpuFeatures::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING) { DetailMode::Layers } else { DetailMode::Colour("device") }
}

/// [design] Why a tracked shape cannot blend, one bit each; the order is the first-reason
/// precedence of the `census-shape` lines and the order of the ready line's counts.
/// Reference reasons (world.rs::spawn_model), shape reasons, the material's state and
/// ownership, then each material predicate (from `world.rs`, fnv-world `placed.rs`,
/// nvf-assets `model.rs::PreparedModel` and `nif_material.rs::NifSurface`).
pub mod reason {
    pub const WORLD: u32 = 1 << 0;
    pub const RIG: u32 = 1 << 1;
    pub const PARTICLES: u32 = 1 << 2;
    pub const POSED: u32 = 1 << 3;
    pub const DAMAGE_STAGES: u32 = 1 << 4;
    pub const DEST: u32 = 1 << 5;
    pub const DISTANT: u32 = 1 << 6;
    pub const EMITTANCE: u32 = 1 << 7;
    pub const MORPH: u32 = 1 << 8;
    pub const MISSING_SOURCE: u32 = 1 << 9;
    pub const TEMPORARY: u32 = 1 << 10;
    pub const ALPHA_MODE: u32 = 1 << 11;
    pub const UNLIT: u32 = 1 << 12;
    pub const REFRACTION: u32 = 1 << 13;
    pub const FIRST_PERSON: u32 = 1 << 14;
    pub const DEPTH_WRITE: u32 = 1 << 15;
    pub const SPECULAR_TRANSMISSION: u32 = 1 << 16;
    pub const DIFFUSE_TRANSMISSION: u32 = 1 << 17;
    pub const FALLOFF: u32 = 1 << 18;
    pub const HOLES_VALUE: u32 = 1 << 19;
    pub const HOLES_TEXTURE: u32 = 1 << 20;
    pub const FACEGEN: u32 = 1 << 21;
    pub const PIPBOY: u32 = 1 << 22;
    pub const ADD_LINEAR: u32 = 1 << 23;
    pub const SOFT: u32 = 1 << 24;
    pub const DEPTH_BIAS: u32 = 1 << 25;
    /// Drawn under a billboard holder that turns to the camera (#2549, nvf-assets
    /// model.rs `BakedBillboards`); last, so earlier precedence is unchanged.
    pub const BILLBOARD: u32 = 1 << 26;
    /// Reflects a window environment map (#2462, nvf-assets `NifSurface::env`): the blend
    /// type has no environment bindings (its table has the landscape palette at 54 on).
    pub const ENVIRONMENT_MAP: u32 = 1 << 27;
    /// Bits and the names the logs use (`excluded-<name>=`).
    pub const NAMES: [(u32, &str); 28] = [
        (WORLD, "world"), (RIG, "rig"), (PARTICLES, "particles"), (POSED, "posed"),
        (DAMAGE_STAGES, "damage-stages"), (DEST, "dest"), (DISTANT, "distant"),
        (EMITTANCE, "emittance"), (MORPH, "morph"), (MISSING_SOURCE, "missing-source"),
        (TEMPORARY, "temporary"), (ALPHA_MODE, "alpha-mode"), (UNLIT, "unlit"),
        (REFRACTION, "refraction"), (FIRST_PERSON, "first-person"), (DEPTH_WRITE, "depth-write"),
        (SPECULAR_TRANSMISSION, "specular-transmission"), (DIFFUSE_TRANSMISSION, "diffuse-transmission"),
        (FALLOFF, "falloff"), (HOLES_VALUE, "holes-value"), (HOLES_TEXTURE, "holes-texture"),
        (FACEGEN, "facegen"), (PIPBOY, "pipboy"), (ADD_LINEAR, "add-linear"), (SOFT, "soft"),
        (DEPTH_BIAS, "depth-bias-nonfinite"), (BILLBOARD, "billboard"),
        (ENVIRONMENT_MAP, "environment-map"),
    ];
    /// The first reason in precedence order, `none` for an empty mask.
    pub fn first(mask: u32) -> &'static str {
        NAMES.iter().find(|(bit, _)| mask & bit != 0).map_or("none", |(_, name)| name)
    }
    /// Every reason, `+`-joined, `none` for an empty mask.
    pub fn list(mask: u32) -> String {
        let names: Vec<&str> = NAMES.iter().filter(|(bit, _)| mask & bit != 0).map(|(_, name)| *name).collect();
        if names.is_empty() { "none".into() } else { names.join("+") }
    }
}

/// [design] Code facts of a placed reference (world.rs::spawn_model, fnv-world
/// placed.rs/index.rs, model.rs::PreparedModel); no asset-name selection.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReferenceFacts {
    /// In an exterior cell of the world being drawn.
    pub exterior: bool,
    /// A rig or skinned shapes.
    pub rigged: bool,
    pub particles: bool,
    /// Jointed, loose bodies, or drawn in a stored pose.
    pub posed: bool,
    pub damage_stages: bool,
    /// The base record has DEST (fnv-game/src/data.rs).
    pub destructible: bool,
    /// Drawn as a distant imposter.
    pub distant: bool,
}

/// Bits whose condition holds, combined.
fn mask_of<const N: usize>(conditions: [(bool, u32); N]) -> u32 {
    conditions.into_iter().filter(|c| c.0).fold(0, |mask, (_, bit)| mask | bit)
}

pub fn reference_reasons(f: ReferenceFacts) -> u32 {
    use reason::*;
    mask_of([(!f.exterior, WORLD), (f.rigged, RIG), (f.particles, PARTICLES), (f.posed, POSED),
        (f.damage_stages, DAMAGE_STAGES), (f.destructible, DEST), (f.distant, DISTANT)])
}

/// [design] A shape driven by the reference's emittance, or a registered morph target.
pub fn shape_reasons(external_emittance: bool, morph: bool) -> u32 {
    mask_of([(external_emittance, reason::EMITTANCE), (morph, reason::MORPH)])
}

/// [design] A shape spawn_model put under a billboard holder (#2549): it turns to the
/// camera, so it is counted and never blends (the backfill leaves such shapes out too).
pub fn billboard_reason(billboard: bool) -> u32 {
    mask_of([(billboard, reason::BILLBOARD)])
}

/// [design] The base record types whose rigid shapes can blend: STAT, and SCOL, a static
/// collection drawn as the one model the editor generated from its statics
/// [documented: GECK wiki "Static Collection", revision 40901, snapshot 2026-10-28:
/// "composed of Static forms", "do not react to Havok physics and cannot be destroyed",
/// its Model "the NIF file used for the object", usually generated on creation; GECK wiki
/// "Bethsoft Tutorial Static Collections", revision 38886: grouping "creates a new .NIF"].
/// world.rs draws a SCOL reference as that one model, as it draws a STAT (`spawn_model`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BaseType { Stat, Scol }
impl BaseType {
    /// The census's type of a base record signature; `None` for every other record.
    pub fn of(sig: &[u8; 4]) -> Option<Self> {
        match sig { b"STAT" => Some(Self::Stat), b"SCOL" => Some(Self::Scol), _ => None }
    }
    /// The record signature, as the `census-shape` lines name it.
    pub fn name(self) -> &'static str {
        match self { Self::Stat => "STAT", Self::Scol => "SCOL" }
    }
}

/// Selection of the reference: a STAT or SCOL without reference reasons (the census
/// records only those; world.rs::blend_census).
#[cfg(test)]
pub fn eligible_reference(sig: &[u8; 4], facts: ReferenceFacts) -> bool {
    BaseType::of(sig).is_some() && reference_reasons(facts) == 0
}

/// [design] Each material predicate the blend cannot take, one reason each. Admitted since
/// round 6: ordinary `Blend` (its transparent phase, blend state, sorting and depth-write
/// state stay the shared specialization's), a finite depth bias (copied exactly), the lit
/// mesh decal flag (the shared specialization keeps its bias) and `alpha_cut` (the discard
/// runs before terrain shading; copied unchanged); `Mask` was already admitted. Add,
/// Multiply, Premultiplied and AlphaToCoverage stay out; holes and FaceGen always, because
/// the blend type repurposes their slots (nif_surface_blend.rs module notes).
pub fn material_reasons(m: &NifMaterial) -> u32 {
    use reason::*;
    let (b, s) = (&m.base, &m.extension);
    mask_of([
        (!matches!(b.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_) | AlphaMode::Blend), ALPHA_MODE),
        (b.unlit, UNLIT), (s.refract, REFRACTION), (s.first_person, FIRST_PERSON), (s.depth_write, DEPTH_WRITE),
        (b.specular_transmission != 0.0, SPECULAR_TRANSMISSION), (b.diffuse_transmission != 0.0, DIFFUSE_TRANSMISSION),
        (s.falloff != Falloff::NONE.params, FALLOFF),
        (s.holes != nvf_assets::nif_material::NO_HOLES, HOLES_VALUE), (s.holes_texture.is_some(), HOLES_TEXTURE),
        (s.facegen_offset.is_some(), FACEGEN), (s.pipboy_screen, PIPBOY), (s.add_linear, ADD_LINEAR),
        (s.soft != Vec4::ZERO, SOFT), (!b.depth_bias.is_finite(), DEPTH_BIAS),
        (s.env.x > 0.0 || s.env_cube.is_some(), ENVIRONMENT_MAP),
    ])
}

/// The conversion reads the full mask ([`shape_mask`]); this is its material part alone.
#[cfg(test)]
pub fn eligible_material(m: &NifMaterial) -> bool {
    material_reasons(m) == 0
}

pub use nvf_assets::nif_surface_blend::BlendShape;

/// [design] Metadata of one spawned STAT or SCOL shape while the blend is on
/// (world.rs::spawn_model, or blend_backfill for what was drawn while it was off),
/// recorded before any filtering: its reference, base, base type and model, the shape's
/// index in the model (in a rigged model, its index among the drawn shapes), and its
/// reference and shape reasons. Diagnostics only: conversion follows [`BlendShape`] alone.
#[derive(Component, Clone, Debug)]
pub struct BlendCensus {
    pub reference: u32,
    pub base: u32,
    pub base_type: BaseType,
    pub model: Arc<str>,
    pub shape: usize,
    pub reasons: u32,
}

/// What an enabled clone was made from, compared in each full pass so that a producer editing
/// the source under the same handle (blended cut-outs flipping Blend and Mask, NIF block
/// edits) rebuilds the clone without inserting a new ordinary material component.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SourceStamp { alpha_mode: AlphaMode, alpha_cut: u32, alpha: u32, depth_bias: u32, decal: bool, roughness: u32, blended_cutout: Option<u32> }
impl SourceStamp {
    fn of(m: &NifMaterial) -> Self {
        Self {
            alpha_mode: m.base.alpha_mode, alpha_cut: m.extension.alpha_cut.to_bits(), alpha: m.base.base_color.alpha().to_bits(),
            depth_bias: m.base.depth_bias.to_bits(), decal: m.extension.decal, roughness: m.base.perceptual_roughness.to_bits(),
            blended_cutout: m.extension.blended_cutout.map(f32::to_bits),
        }
    }
}

/// [design] The census of the currently resident tracked shapes (not unique materials or
/// cumulative spawns): `stat_shapes` and `scol_shapes` by base type, `excluded`,
/// `pending` and `reasons` over both; `reasons` overlap, one count per [`reason::NAMES`]
/// entry; `admitted` counts the enabled shapes whose source is Blend, has a depth bias,
/// the decal flag, is Mask, or has an alpha cut.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Census {
    /// The prefix's `eligible-shapes` (compared for refreshes, printed by `ready_line`).
    eligible: usize,
    stat_shapes: usize,
    scol_shapes: usize,
    excluded: usize,
    pending: usize,
    reasons: [usize; reason::NAMES.len()],
    admitted: [usize; 5],
}
impl Census {
    fn count(&mut self, base_type: BaseType, mask: u32) {
        match base_type {
            BaseType::Stat => self.stat_shapes += 1,
            BaseType::Scol => self.scol_shapes += 1,
        }
        self.excluded += usize::from(mask != 0);
        self.pending += usize::from(mask & reason::MISSING_SOURCE != 0);
        for (n, (bit, _)) in self.reasons.iter_mut().zip(reason::NAMES) {
            *n += usize::from(mask & bit != 0);
        }
    }
    fn admit(&mut self, m: &NifMaterial) {
        let flags = [
            m.base.alpha_mode == AlphaMode::Blend, m.base.depth_bias != 0.0, m.extension.decal,
            matches!(m.base.alpha_mode, AlphaMode::Mask(_)), m.extension.alpha_cut != 0.0,
        ];
        for (n, f) in self.admitted.iter_mut().zip(flags) { *n += usize::from(f); }
    }
    /// Appended to the ready line after round 5's fields and the weight fields; round 7's
    /// `scol-shapes` comes last, after the admitted counts, so the earlier fields keep
    /// their order and neighbours.
    fn fields(&self) -> String {
        use std::fmt::Write;
        let mut out = format!(" stat-shapes={} excluded-shapes={} pending-material-shapes={}", self.stat_shapes, self.excluded, self.pending);
        for ((_, name), n) in reason::NAMES.iter().zip(self.reasons) {
            let _ = write!(out, " excluded-{name}={n}");
        }
        for (name, n) in ["blend", "bias", "decal", "mask", "alpha-cut"].iter().zip(self.admitted) {
            let _ = write!(out, " admitted-{name}={n}");
        }
        let _ = write!(out, " scol-shapes={}", self.scol_shapes);
        out
    }
}

/// [design] Census-only changes refresh the ready line at most once per this many frames
/// (a publication reports at once), so cell streaming does not log every frame.
const REFRESH_FRAMES: u32 = 60;
/// [design] #2956: at most this many blend clones made or restricted to a new coverage a
/// frame (each one the renderer prepares again). A publication's whole set (over a
/// thousand around Primm) then switches over some frames instead of in one 50-120 ms
/// frame [observed 2026-11-19, stress tour Goodsprings-Primm, `profile hitch` lines].
const CLONE_WORK_PER_FRAME: usize = 64;
/// [design] At most this many entity material-component swaps (`remove::<NifMaterial>` +
/// `insert::<NifBlendMaterial>`) per frame, so a ready publication with thousands of
/// eligible shapes amortizes ECS archetype table moves across frames instead of stalling
/// the main thread in one frame.
const SWAP_WORK_PER_FRAME: usize = 128;
/// [design] At most this many `census-shape` lines per run; each line is a distinct base
/// form, shape index and reason mask (the first reference seen of it).
const SHAPE_LINES: usize = 2048;

#[derive(Resource, Default)]
struct Variants {
    generation: u64,
    readiness: Option<BlendReadiness>,
    publications: HashMap<AssetId<NifBlendMaterial>, crate::terrain_surface::PublishedSurface>,
    ordinary_seen: HashMap<Entity, u32>,
    materials: HashMap<AssetId<NifMaterial>, (Handle<NifBlendMaterial>, SourceStamp)>,
    reported: Option<u64>,
    reported_census: Option<Census>,
    frames_since_report: u32,
    logged: HashSet<(u32, usize, u32)>,
    shape_lines: usize,
    /// What the last full pass saw of its frame-level inputs; `None` before the first
    /// ([`pass_needed`]).
    last: Option<LastPass>,
    /// Each source material the last full pass read, with what it read of it
    /// ([`fingerprint`]), compared when the material store changed.
    watched: HashMap<AssetId<NifMaterial>, Fingerprint>,
    /// The ordinary material each census-only shape drew in the last full pass
    /// ([`CensusDrawn`]).
    census_drawn: HashMap<Entity, AssetId<NifMaterial>>,
    /// One publication of each key in [`Self::publications`], and the coverage last put on
    /// that key's clones (`None`: not yet, or a clone was added since) ([`follow_coverage`]).
    surfaces: HashMap<PublicationKey, (crate::terrain_surface::PublishedSurface, Option<u32>)>,
    /// Full passes the gate called for since the blend turned on (tests and the verify
    /// line; the verify aid's passes in skipped frames are `verify.skipped`).
    passes: u64,
    verify: Verify,
}

/// A publication's identity: its revision and colour atlas (the revision alone already
/// differs between publications of one cache; the atlas makes it hold across caches).
type PublicationKey = (u64, AssetId<Image>);
fn publication_key(p: &crate::terrain_surface::PublishedSurface) -> PublicationKey {
    (p.revision, p.colour.id())
}

/// What the full pass reads of a source material: whether it is loaded and its reasons
/// ([`material_reasons`]) for every shape; its [`SourceStamp`] (which also holds every
/// field the admitted counts and `see_through` read; `depth_write` is a reason) only for
/// a shape that may blend (mask 0). [design] So a source only shapes kept out read (a
/// fade's or an override's copy whose alpha moves every frame, a census-only shape's
/// material) is watched without its stamp: such a shape becomes allowed only through a
/// change of its reasons or its source loading (this fingerprint), a temporary marker's
/// removal, its ordinary material component, its `BlendShape` or its `BlendCensus` added,
/// replaced or removed, each a trigger of its own.
type Fingerprint = Option<(u32, Option<SourceStamp>)>;
fn fingerprint(source: Option<&NifMaterial>, stamped: bool) -> Fingerprint {
    source.map(|m| (material_reasons(m), stamped.then(|| SourceStamp::of(m))))
}
/// Whether a recorded fingerprint holds the stamp.
fn stamped(seen: &Fingerprint) -> bool {
    seen.is_some_and(|(_, stamp)| stamp.is_some())
}
/// Records what a shape read of its source: with the stamp when the shape may blend; a
/// source read both ways keeps the stamp.
fn watch(watched: &mut HashMap<AssetId<NifMaterial>, Fingerprint>, id: AssetId<NifMaterial>, source: Option<&NifMaterial>, allowed: bool) {
    match watched.entry(id) {
        std::collections::hash_map::Entry::Occupied(mut e) => {
            if allowed && !stamped(e.get()) { e.insert(fingerprint(source, true)); }
        }
        std::collections::hash_map::Entry::Vacant(e) => { e.insert(fingerprint(source, allowed)); }
    }
}

/// [design] The frame-level inputs of the last full pass, and what it ended with.
#[derive(Clone)]
struct LastPass {
    config: BlendConfig,
    published: Option<PublicationKey>,
    pending: bool,
    census: Census,
}

/// [design] The test aid `NVF_PLAY_TEST_MESH_BLEND_VERIFY=1`: in every frame the gate
/// would skip, the full pass runs anyway and any change it makes is logged
/// (`verify diverged`, bounded), with a count line (`verify skipped-frames=`). Read once;
/// any other value is refused in the log.
fn verify_aid() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| match std::env::var("NVF_PLAY_TEST_MESH_BLEND_VERIFY") {
        Ok(v) if v == "1" => true,
        Ok(v) => { warn!("mesh terrain blend: rejected NVF_PLAY_TEST_MESH_BLEND_VERIFY={v:?}: expected 1"); false }
        Err(_) => false,
    })
}

/// At most this many `verify diverged` lines per run of the blend.
const VERIFY_LINES: usize = 16;
/// A `verify skipped-frames=` line at least once per this many skipped frames.
const VERIFY_PERIOD: u64 = 600;

/// Why a full pass ran ([`pass_needed`]), the names of the verify line's `pass-<name>=` counts.
const PASS_REASONS: [&str; 6] = ["first", "config", "publication", "pending", "shapes", "materials"];

/// Which shape signal ran a pass whose first reason was `shapes` ([`count_shape_signals`],
/// the verify aid only), the names of the verify line's `shapes-<name>=` counts: a
/// `BlendShape` or `BlendCensus` added or replaced, an ordinary material put on or written
/// on a convertible shape or (another handle) on a census-only one, a temporary marker
/// added, then each removal kind (the ordinary and marker removals on a shape still
/// tracked). Several can fire in one frame.
const SHAPE_SIGNALS: [&str; 10] = [
    "blend-shape", "census", "ordinary-convertible", "ordinary-census-only", "marker-added",
    "removed-blend-shape", "removed-census", "removed-ordinary", "removed-fading", "removed-override",
];
/// At most this many `verify shape-trigger` lines per signal per run of the blend (only
/// the five signals with a shape to name log one: at most 10 lines).
const TRIGGER_LINES: usize = 2;

/// The verify aid's state; `reasons` counts every pass by its first reason, aid or not.
struct Verify { on: bool, skipped: u64, diverged: u64, lines: usize, since_line: u64, reasons: [u64; PASS_REASONS.len()], signals: [u64; SHAPE_SIGNALS.len()], trigger_lines: [usize; SHAPE_SIGNALS.len()] }
impl Default for Verify {
    fn default() -> Self { Self { on: verify_aid(), skipped: 0, diverged: 0, lines: 0, since_line: 0, reasons: [0; PASS_REASONS.len()], signals: [0; SHAPE_SIGNALS.len()], trigger_lines: [0; SHAPE_SIGNALS.len()] } }
}
impl Verify {
    /// `verify skipped-frames=N diverged=M passes=P pass-first=.. pass-config=..
    /// pass-publication=.. pass-pending=.. pass-shapes=.. pass-materials=..`, then the
    /// shape signals' `shapes-<name>=..` counts.
    fn line(&mut self, passes: u64) -> String {
        use std::fmt::Write;
        self.since_line = 0;
        let mut out = format!("mesh terrain blend: verify skipped-frames={} diverged={} passes={passes}", self.skipped, self.diverged);
        for (name, n) in PASS_REASONS.iter().zip(self.reasons) { let _ = write!(out, " pass-{name}={n}"); }
        for (name, n) in SHAPE_SIGNALS.iter().zip(self.signals) { let _ = write!(out, " shapes-{name}={n}"); }
        out
    }
    /// One divergence: counted, and logged while under [`VERIFY_LINES`].
    fn diverged(&mut self, what: &str) {
        self.diverged += 1;
        self.lines += 1;
        if self.lines < VERIFY_LINES {
            warn!("mesh terrain blend: verify diverged skipped-frame={} {what}", self.skipped);
        } else if self.lines == VERIFY_LINES {
            warn!("mesh terrain blend: verify diverged lines stopped at {VERIFY_LINES}; the count goes on");
        }
    }
}

/// [design] Asset IDs alone cross this acknowledgement boundary; main-world handles
/// in candidates and BlendShape keep both image generations alive until the switch.
#[derive(Resource, Clone, Default)]
pub(crate) struct BlendReadiness {
    ready: Arc<std::sync::Mutex<HashSet<AssetId<NifBlendMaterial>>>>,
    settled: Arc<std::sync::atomic::AtomicU64>,
    /// Test aid only (the crossing driver, terrain_surface.rs `Crossing`): the step whose
    /// new materials are held unacknowledged, 0 for none ([`acknowledgeable`]).
    hold: Arc<std::sync::atomic::AtomicU64>,
}
impl BlendReadiness {
    pub(crate) fn usable(&self, id: AssetId<NifBlendMaterial>) -> bool { self.ready.lock().unwrap().contains(&id) }
    pub(crate) fn settled(&self, revision: u64) -> bool { self.settled.load(Ordering::Acquire) == revision }
    /// Test aid only: hold the materials first seen from now until the next call (0: hold
    /// nothing). See [`acknowledgeable`].
    pub(crate) fn hold(&self, step: u64) { self.hold.store(step, Ordering::Release); }
}
#[derive(Resource, Default)]
struct ReadinessInput { ids: HashSet<AssetId<NifBlendMaterial>>, ages: HashMap<AssetId<NifBlendMaterial>, u32>, steps: HashMap<AssetId<NifBlendMaterial>, u64>, delay: u32 }

/// [design] Round 10a review (P3): whether a prepared, bound material may be acknowledged:
/// older than the readiness delay aid's frames and not held by the crossing aid. The
/// crossing driver holds the materials first seen after its step `n` (`first_seen == n`,
/// `hold == n`) until it issues the next step, so the replacement its step started stays
/// pending until the step after it is issued; a delayed step can then never find the work
/// already settled (it would wait for pending work that cannot come, up to its timeout).
/// Materials seen before a step, the retained clones among them, are never held. Without
/// the crossing aid `hold` stays 0.
pub(crate) fn acknowledgeable(age: u32, delay: u32, first_seen: u64, hold: u64) -> bool {
    age > delay && (hold == 0 || first_seen != hold)
}

fn install_readiness(app: &mut App, gate: MaterialGate) {
    use bevy::render::{ExtractSchedule, Render, RenderApp, RenderSystems};
    let shared = BlendReadiness::default();
    app.insert_resource(shared.clone()).insert_resource(Variants { readiness: Some(shared.clone()), ..default() });
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        let delay = std::env::var("NVF_PLAY_TEST_MESH_BLEND_READY_DELAY").ok().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0).min(120);
        let extract_gate = gate.clone();
        render.insert_resource(shared).insert_resource(ReadinessInput { delay, ..default() })
            .add_systems(ExtractSchedule, extract_readiness.run_if(move || extract_gate.is_open()))
            .add_systems(Render, acknowledge_materials.run_if(move || gate.is_open()).in_set(RenderSystems::Cleanup));
    }
}
fn extract_readiness(mut input: ResMut<ReadinessInput>, variants: bevy::render::Extract<Res<Variants>>) {
    input.ids = variants.materials.values().map(|(h, _)| h.id()).chain(variants.publications.keys().copied()).collect();
}
fn acknowledge_materials(
    mut input: ResMut<ReadinessInput>, shared: Res<BlendReadiness>,
    materials: Res<bevy::render::erased_render_asset::ErasedRenderAssets<bevy::pbr::PreparedMaterial>>,
    allocators: Res<bevy::render::material_bind_groups::MaterialBindGroupAllocators>,
) {
    let ids = input.ids.clone();
    input.ages.retain(|id, _| ids.contains(id));
    input.steps.retain(|id, _| ids.contains(id));
    let hold = shared.hold.load(Ordering::Acquire);
    // Built outside the lock and swapped in: the main world's conversion asks `usable`
    // once per shape while this runs beside it (pipelined rendering).
    let mut ready = HashSet::with_capacity(ids.len());
    for id in ids {
        let age = input.ages.entry(id).or_default();
        *age = age.saturating_add(1);
        let age = *age;
        let first_seen = *input.steps.entry(id).or_insert(hold);
        if !acknowledgeable(age, input.delay, first_seen, hold) { continue; }
        let Some(material) = materials.get(id) else { continue };
        let bound = allocators.get(&std::any::TypeId::of::<NifBlendMaterial>()).and_then(|a| a.get(material.binding.group)).is_some_and(|b| b.bind_group().is_some());
        if bound { ready.insert(id); }
    }
    *shared.ready.lock().unwrap() = ready;
}

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct BlendConversion;

/// [design] Registered in every run, so the settings row can switch the blend while the
/// game runs: the materials' plugins behind the closed [`BlendGate`] and empty resources;
/// every system that captures, bakes or converts runs only while the blend is on
/// ([`active`]), the restoration only in the frame it turns off, the gate's closing only
/// after that. What still runs while it is off: graphics-enhanced.md §3.14 "Off".
pub struct MeshTerrainBlendPlugin;

impl Plugin for MeshTerrainBlendPlugin {
    fn build(&self, app: &mut App) {
        let gate = MaterialGate::new();
        install_readiness(app, gate.clone());
        crate::terrain_surface::install_continuity_probe(app);
        app.add_plugins((MaterialPlugin::<NifBlendMaterial>::gated(gate.clone()), crate::terrain_surface::SurfaceBakePlugin { gate: gate.clone() }))
            .insert_resource(BlendGate(gate))
            .init_resource::<SurfaceCache>().init_resource::<Variants>().init_resource::<BlendState>()
            .add_systems(Update, (
                follow_setting.before(crate::world::stream),
                close_gate.after(follow_setting).run_if(draining),
                (crate::world::blend_backfill, crate::terrain_surface::backfill_cells).after(follow_setting).run_if(active),
            ))
            .add_systems(PostUpdate, conversion_order((crate::terrain_surface::invalidate_context.run_if(active), stop.run_if(stopping), apply_variants.run_if(active)).chain().in_set(BlendConversion)))
            // Source edits (including this frame's producers) must be versioned before
            // accepting a bake receipt. Conversion consumes this publication next frame.
            .add_systems(PostUpdate, (crate::terrain_surface::refresh_captures, update_cache).chain()
                .after(bevy::asset::AssetEventSystems).after(bevy::transform::TransformSystems::Propagate).run_if(active));
    }
}

/// The shapes the blend tracks, for switching it off, and whether a producer has put an
/// ordinary material on them.
type Switched<'w, 's> = Query<'w, 's, (Entity, Option<&'static BlendShape>, Has<MeshMaterial3d<NifMaterial>>), Or<(With<BlendShape>, With<BlendCensus>)>>;

/// [design] Each frame, before the world streams: the blend [`desired`] from the aid and
/// the settings row (`Graphics::mesh_terrain_blend`), switched on or off when it differs
/// from what is drawn. On: the gate opened, the start line, the atlas checked against the
/// device, the census of what is already drawn queued. Off: no more spawns record, and
/// [`stop`] puts the materials back in PostUpdate, after this frame's material producers;
/// a different look (not reachable today: the aid is read once, the row has one look)
/// starts the frame after. No restart either way.
fn follow_setting(
    graphics: Option<Res<crate::graphics::Graphics>>,
    device: Option<Res<bevy::render::renderer::RenderDevice>>,
    gate: Res<BlendGate>,
    mut state: ResMut<BlendState>,
    mut cache: ResMut<SurfaceCache>,
    mut variants: ResMut<Variants>,
) {
    let want = desired(aid(), graphics.is_some_and(|g| g.mesh_terrain_blend));
    if want == state.active || state.stopping { return; }
    if state.active.is_some() {
        RECORDING.store(false, Ordering::Relaxed);
        state.active = None;
        state.stopping = true;
        return;
    }
    let Some(c) = want else { return };
    gate.0.set_open(true);
    RECORDING.store(true, Ordering::Relaxed);
    // [design] Round 10a: the detail mode is resolved only when the blend turns on (no
    // detail work while off).
    let detail = detail_capability(device.as_deref(), detail_aid());
    info!("{}", start_line(&c, detail));
    // [design] #2149/#2661: the height atlas holds the run's landscape surface (fixed before
    // the first cell, at PostStartup; this runs in Update).
    let layout = nvf_assets::nif_surface_blend::HeightLayout::new(nvf_play::smooth_terrain::spec().subdiv());
    cache.start(c.resolution, device.map(|d| d.limits().max_texture_dimension_2d), detail.layers(), layout);
    *variants = Variants { readiness: variants.readiness.take(), ..default() };
    state.active = Some(c);
    state.shapes_pending = true;
    state.draining = None;
}

/// [design] The blend off, in PostUpdate where the conversion runs ([`conversion_order`]:
/// after the Update producers, texture swaps, fades, effect shaders and overrides, and the
/// PostUpdate ones, the NIF mirror and the blended cut-outs, with their commands applied).
/// Each converted shape leaves the blend material: an ordinary material a producer put on
/// it this frame stays (a texture swap's copy, whose generation the swap has consumed, or
/// a fade's or override's copy); only a shape with none gets [`BlendShape::original`] back
/// (the shared see-through shadow bookkeeping follows either change). Then the census and
/// the landscape captures are removed, the bake cancelled and the atlas and clones dropped
/// ([`SurfaceCache::release`]), one `stop` line, and [`close_gate`] waits for the
/// materials to go.
fn stop(mut commands: Commands, mut state: ResMut<BlendState>, mut cache: ResMut<SurfaceCache>, mut variants: ResMut<Variants>, shapes: Switched, cells: Query<Entity, With<crate::terrain_surface::SurfaceCell>>) {
    let (mut restored, mut kept) = (0, 0);
    for (entity, shape, ordinary) in shapes.iter() {
        let mut e = commands.entity(entity);
        if let Some(enabled) = shape.filter(|s| s.enabled.is_some()) {
            e.try_remove::<MeshMaterial3d<NifBlendMaterial>>();
            if ordinary {
                kept += 1;
            } else {
                e.try_insert(MeshMaterial3d(enabled.original.clone()));
            }
            restored += 1;
        }
        e.try_remove::<(BlendShape, BlendCensus)>();
    }
    for cell in cells.iter() {
        commands.entity(cell).try_remove::<crate::terrain_surface::SurfaceCell>();
    }
    let released = cells.iter().count();
    cache.release(&mut commands);
    if variants.verify.on {
        let passes = variants.passes;
        info!("{}", variants.verify.line(passes));
    }
    *variants = Variants { readiness: variants.readiness.take(), ..default() };
    *state = BlendState { draining: Some(0), ..default() };
    info!("mesh terrain blend: stop restored-shapes={restored} tracked-shapes={} released-cells={released} kept-producer-materials={kept}", shapes.iter().count());
}

/// [design] Frames in a row with no blend or capture material, asset or component, before
/// the gate closes: the removals made by [`stop`] are extracted and unloaded in the render
/// world in the first of them (vendored bevy_pbr `MaterialGate`).
const IDLE_FRAMES: u32 = 3;

type BlendDrawn<'w, 's> = Query<'w, 's, (), Or<(With<MeshMaterial3d<NifBlendMaterial>>, With<MeshMaterial3d<crate::terrain_surface::SurfaceCaptureMaterial>>)>>;

/// [design] While off with the gate open: once nothing of the blend's two material types
/// has been left for [`IDLE_FRAMES`] frames, the gate closes; the render world then drops
/// their allocators and shader handles and puts Bevy's draw functions back
/// (terrain_surface.rs `uninstall_bake_receipts`), and an `idle` line says so. A switch
/// back on meanwhile keeps the gate open ([`follow_setting`]).
fn close_gate(
    mut state: ResMut<BlendState>,
    gate: Res<BlendGate>,
    blends: Res<Assets<NifBlendMaterial>>,
    captures: Res<Assets<crate::terrain_surface::SurfaceCaptureMaterial>>,
    drawn: BlendDrawn,
) {
    let Some(frames) = state.draining.as_mut() else { return };
    *frames = if blends.is_empty() && captures.is_empty() && drawn.is_empty() { *frames + 1 } else { 0 };
    if *frames >= IDLE_FRAMES {
        gate.0.set_open(false);
        state.draining = None;
        info!("mesh terrain blend: idle materials-released=true");
    }
}

/// [design] Where the conversion runs: ordinary producers (swaps, alpha, scripted fades)
/// finish in Update; in PostUpdate it follows the scripted NIF edits (`nif_mirror`, their
/// commands applied, so an edited source is never left beside the blend material), the
/// blended cut-outs' same-handle alpha-mode edits, and the see-through shadow bookkeeping
/// (which must see a restored ordinary material before it is swapped away, or a finished
/// fade keeps its shadow markers), and precedes asset events and mesh/material change
/// marking.
fn conversion_order<M>(system: impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, M>) -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    system
        .after(crate::nif_mirror::MirrorSystems)
        .after(nvf_assets::nif_material::BlendedCutoutSystems)
        .after(nvf_assets::nif_material::SeeThroughShadowSystems)
        .before(bevy::asset::AssetEventSystems)
        .before(bevy::mesh::mark_3d_meshes_as_changed_if_their_assets_changed)
}

fn enabled_material(source: &NifMaterial, c: BlendConfig, surface: &crate::terrain_surface::PublishedSurface) -> NifBlendMaterial {
    NifBlendMaterial {
        base: source.base.clone(),
        // [design] Round 10a: every clone of a publication binds that publication's palette
        // (empty without landscape detail), so its table numbers match its height atlas.
        extension: NifSurfaceBlend::from_source(&source.extension, c.controls(), c.width_controls(),
            Vec4::new(surface.origin.0 as f32, surface.origin.1 as f32, surface.coverage as f32, 0.0), surface.colour.clone(), (surface.height.clone(), surface.layout), surface.palette.clone()),
    }
}

/// Temporary material owners (fades/actor alpha under `Fading`, overrides), whose copies never blend.
type Temporary = (Has<nvf_play::fade::Fading>, Has<MaterialOverride>);
/// The see-through shadow markers (nif_material.rs `see_through_casts_no_shadow`).
type ShadowMarks = (Has<NotShadowCaster>, Has<SeeThroughNoShadow>);
/// Every shape the blend tracks: convertible ones ([`BlendShape`]) and census-only ones.
type Tracked<'w, 's> = Query<'w, 's, (Entity, Option<&'static mut BlendShape>, Option<&'static BlendCensus>, Option<Ref<'static, MeshMaterial3d<NifMaterial>>>, Temporary, ShadowMarks), Or<(With<BlendShape>, With<BlendCensus>)>>;

/// [design] Tracked shapes whose conversion inputs changed since the conversion last ran:
/// a `BlendShape` added or replaced (only the conversion edits one in place, and a system
/// never sees its own edits as changes), a `BlendCensus` added or replaced, an ordinary
/// material put on or written on a convertible shape (any write: the pass's eviction of an
/// owned source republished under the same handle reads its change tick), a temporary
/// owner's marker added. Census-only shapes' ordinary materials: [`CensusDrawn`].
/// Removals: [`Removals`]. The shadow marks are left out on purpose: the pass reads them
/// only where it swaps a shape's material, which in a frame the gate skips no shape needs
/// (see [`pass_needed`]).
type ShapeChanges<'w, 's> = Query<'w, 's, (), (Or<(With<BlendShape>, With<BlendCensus>)>, Or<(
    Changed<BlendShape>, Changed<BlendCensus>, (Changed<MeshMaterial3d<NifMaterial>>, With<BlendShape>),
    Added<nvf_play::fade::Fading>, Added<MaterialOverride>,
)>)>;

/// [design] Census-only shapes (no `BlendShape`) whose ordinary material component was put
/// on or written since the conversion last ran. Of it the full pass reads only the handle
/// (which source's reasons join the mask; the source's own changes are the fingerprint's),
/// so such a write runs the pass only when the handle differs from the one the last pass
/// recorded ([`Variants::census_drawn`]): a rig's animated look published again under its
/// own handle (nvf-assets material_anim.rs, most frames for an animated STAT) runs none.
type CensusDrawn<'w, 's> = Query<'w, 's, (Entity, &'static MeshMaterial3d<NifMaterial>), (With<BlendCensus>, Without<BlendShape>, Changed<MeshMaterial3d<NifMaterial>>)>;

/// The verify aid only: each tracked shape's signals ([`count_shape_signals`]).
type Signals<'w, 's> = Query<'w, 's, (
    Entity, Option<Ref<'static, BlendShape>>, Option<Ref<'static, BlendCensus>>, Option<Ref<'static, MeshMaterial3d<NifMaterial>>>,
    Option<Ref<'static, nvf_play::fade::Fading>>, Option<Ref<'static, MaterialOverride>>,
), Or<(With<BlendShape>, With<BlendCensus>)>>;

/// The conversion's shape queries; one at a time.
type ShapeQueries<'w, 's> = ParamSet<'w, 's, (Tracked<'w, 's>, ShapeChanges<'w, 's>, CensusDrawn<'w, 's>, Signals<'w, 's>)>;

/// [design] Removals the gate reads: the tracking components (spawned shapes streaming out,
/// a despawn sends these too) count always, the others only on a shape still tracked.
#[derive(bevy::ecs::system::SystemParam)]
struct Removals<'w, 's> {
    shape: RemovedComponents<'w, 's, BlendShape>,
    census: RemovedComponents<'w, 's, BlendCensus>,
    ordinary: RemovedComponents<'w, 's, MeshMaterial3d<NifMaterial>>,
    fading: RemovedComponents<'w, 's, nvf_play::fade::Fading>,
    overridden: RemovedComponents<'w, 's, MaterialOverride>,
}

/// Everything the conversion reads and writes ([`apply_configured_variants`]).
#[derive(bevy::ecs::system::SystemParam)]
struct Conversion<'w, 's> {
    commands: Commands<'w, 's>,
    cache: Res<'w, SurfaceCache>,
    variants: ResMut<'w, Variants>,
    materials: Res<'w, Assets<NifMaterial>>,
    blends: ResMut<'w, Assets<NifBlendMaterial>>,
    shapes: ShapeQueries<'w, 's>,
    removals: Removals<'w, 's>,
    /// Above 60 fps a frame gets the same share of [`CLONE_WORK_PER_FRAME`]
    /// (stream_pace.rs); `None` in tests that build no pace.
    pace: Option<Res<'w, crate::stream_pace::StreamPace>>,
}

fn apply_variants(state: Res<BlendState>, mut conversion: Conversion) {
    let Some(c) = state.active else { return };
    apply_configured_variants(&mut conversion, c);
}

/// The full reason mask of a tracked shape this frame: its recorded reference and shape
/// reasons, temporary ownership, and its source material's reasons (or that the source
/// is not loaded).
fn shape_mask(recorded: u32, temporary: bool, source: Option<&NifMaterial>) -> u32 {
    let owned = if temporary { reason::TEMPORARY } else { 0 };
    recorded | owned | source.map_or(reason::MISSING_SOURCE, material_reasons)
}

/// [design] Whether any tracked shape's inputs changed since the conversion last ran
/// ([`ShapeChanges`], [`CensusDrawn`], [`Removals`]), and which removal kinds fired (for
/// the verify aid's counts). Every removal reader is drained each run.
fn shapes_changed(shapes: &mut ShapeQueries, removals: &mut Removals, variants: &Variants) -> (bool, [bool; 5]) {
    let mut removed = [!removals.shape.is_empty(), !removals.census.is_empty(), false, false, false];
    removals.shape.clear();
    removals.census.clear();
    {
        let tracked = shapes.p0();
        removed[2] = removals.ordinary.read().fold(false, |hit, e| hit | tracked.contains(e));
        removed[3] = removals.fading.read().fold(false, |hit, e| hit | tracked.contains(e));
        removed[4] = removals.overridden.read().fold(false, |hit, e| hit | tracked.contains(e));
    }
    let changed = removed.contains(&true) || !shapes.p1().is_empty()
        || shapes.p2().iter().any(|(e, drawn)| variants.census_drawn.get(&e) != Some(&drawn.0.id()));
    (changed, removed)
}

/// The verify aid only, in a frame whose pass ran for `shapes`: which shape signals fired
/// ([`SHAPE_SIGNALS`]), counted once each, and up to [`TRIGGER_LINES`] lines per signal naming a
/// shape that fired one (its model, shape index and recorded reasons).
fn count_shape_signals(shapes: &mut ShapeQueries, variants: &mut Variants, removed: [bool; 5]) {
    let mut fired = [false; SHAPE_SIGNALS.len()];
    fired[5..].copy_from_slice(&removed);
    for (e, shape, census, ordinary, fading, overridden) in shapes.p3().iter() {
        let ordinary_changed = ordinary.as_ref().is_some_and(|o| o.is_changed());
        let hits = [
            shape.as_ref().is_some_and(|s| s.is_changed()),
            census.as_ref().is_some_and(|c| c.is_changed()),
            ordinary_changed && shape.is_some(),
            ordinary_changed && shape.is_none() && ordinary.as_ref().is_some_and(|o| variants.census_drawn.get(&e) != Some(&o.0.id())),
            fading.is_some_and(|m| m.is_added()) || overridden.is_some_and(|m| m.is_added()),
        ];
        for (i, hit) in hits.iter().enumerate() {
            if !hit { continue; }
            fired[i] = true;
            if variants.verify.trigger_lines[i] < TRIGGER_LINES {
                variants.verify.trigger_lines[i] += 1;
                let (model, index, reasons) = census.as_ref().map_or(("-".to_string(), 0, 0), |c| (c.model.to_string(), c.shape, c.reasons));
                info!("mesh terrain blend: verify shape-trigger signal={} model={model} shape={index} reasons={}", SHAPE_SIGNALS[i], reason::list(reasons));
            }
        }
    }
    for (n, f) in variants.verify.signals.iter_mut().zip(fired) { *n += u64::from(f); }
}

/// [design] Whether the full pass must run this frame. It reads, besides its own state:
/// the configuration; the publication (`None` or its key); the render world's readiness
/// of the clones (only while the last pass ended pending: once it settled, every clone on
/// a shape was usable and a clone becoming unusable later changes nothing the pass does,
/// `settled` already holding this revision); each tracked shape's components (changes and
/// removals, [`shapes_changed`]); and each source material ([`Variants::watched`], compared
/// only when the material store was changed since the last run). In a frame it skips,
/// every convertible shape that can blend already draws its clone and has no ordinary
/// material, so the material swap (and the shadow marks it reads) is not reached.
/// Returns the first reason ([`PASS_REASONS`]), `None` to skip.
fn pass_needed(variants: &Variants, c: BlendConfig, published: Option<PublicationKey>, shapes: bool, materials: &Res<Assets<NifMaterial>>) -> Option<usize> {
    let Some(last) = &variants.last else { return Some(0) };
    [
        last.config != c,
        last.published != published,
        last.pending,
        shapes,
    ].iter().position(|&b| b).map(|i| i + 1).or_else(|| {
        let edited = materials.is_changed() && variants.watched.iter().any(|(id, seen)| fingerprint(materials.get(*id), stamped(seen)) != *seen);
        edited.then_some(5)
    })
}

/// [design] Restrict old drawable clones too, while their replacement prepares. Only
/// uniform POD changes; the atlas handles remain fixed (no colour upload). Round 10a review:
/// and the palette follows the coverage, keeping only what covered quadrants read
/// (`PublishedSurface::live_palette`), so the clone's rebuild never waits for a masked
/// cell's image that is gone; the bound numbers keep their meaning. Each frame, the
/// coverage of one publication per key is compared with the coverage last put on that
/// key's clones; only a key whose coverage changed, or that gained a clone, walks its
/// clones. Returns the clones changed.
fn follow_coverage(cache: &SurfaceCache, variants: &mut Variants, blends: &mut Assets<NifBlendMaterial>, budget: usize) -> usize {
    let mut changed = 0;
    for (key, (publication, applied)) in &mut variants.surfaces {
        let coverage = cache.coverage_for(publication);
        if *applied == Some(coverage) { continue; }
        // [design] #2956: each changed clone is prepared again by the renderer, so at most
        // `budget` change a frame; a key not walked to its end is walked again next frame
        // (an already restricted clone is skipped by `restrict_clone`).
        let mut done = true;
        for (id, publication) in variants.publications.iter().filter(|(_, p)| publication_key(p) == *key) {
            if changed >= budget { done = false; break; }
            changed += restrict_clone(cache, blends, *id, publication);
        }
        if done { *applied = Some(coverage); }
    }
    changed
}

/// One clone brought to its publication's coverage; 1 when it changed.
fn restrict_clone(cache: &SurfaceCache, blends: &mut Assets<NifBlendMaterial>, id: AssetId<NifBlendMaterial>, publication: &crate::terrain_surface::PublishedSurface) -> usize {
    let coverage = cache.coverage_for(publication);
    if !blends.get(id).is_some_and(|m| m.extension.mesh_terrain_origin.z != coverage as f32) { return 0; }
    let extension = &mut blends.get_mut(id).unwrap().extension;
    extension.mesh_terrain_origin.z = coverage as f32;
    extension.palette = publication.live_palette(coverage);
    1
}

/// What one full pass did, for the gate's next frame and the verify aid.
#[derive(Default)]
struct Outcome { pending: bool, census: Census, commands: usize, created: usize, evicted: usize, dropped: usize, edits: usize }

// [design] Explicit config boundary lets ECS tests publish without process-wide env state.
// The conversion is change-driven (graphics-enhanced.md §3.14 "Per frame"): the full pass
// ([`full_pass`]) runs only in frames [`pass_needed`] says one of its inputs changed; a
// quiet frame follows the coverage, stores `settled` again and refreshes a deferred ready
// line from the last pass's census.
fn apply_configured_variants(p: &mut Conversion, c: BlendConfig) {
    let Conversion { commands, cache, variants, materials, blends, shapes, removals, pace } = p;
    let variants: &mut Variants = variants;
    let published = cache.published.as_ref().map(publication_key);
    let revision = published.map_or(0, |k| k.0);
    let clone_work = pace.as_deref().map_or(CLONE_WORK_PER_FRAME, |p| p.count(CLONE_WORK_PER_FRAME));
    let swap_budget = pace.as_deref().map_or(SWAP_WORK_PER_FRAME, |p| p.count(SWAP_WORK_PER_FRAME));
    let _spending = pace.as_deref().map(|p| p.spending());
    let restricted = follow_coverage(cache, variants, blends, clone_work);
    let create_budget = clone_work.saturating_sub(restricted);
    // A follow that used its whole budget may have left clones for the next frame: not a
    // divergence, so the verify aid checks only after a follow that finished.
    if variants.verify.on && restricted < clone_work {
        // The verify aid: the per-clone check of every recorded publication, as before.
        let restricted: usize = variants.publications.iter().map(|(id, publication)| restrict_clone(cache, blends, *id, publication)).sum();
        if restricted > 0 { variants.verify.diverged(&format!("coverage-clones={restricted}")); }
    }
    let (changed, removed) = shapes_changed(shapes, removals, variants);
    let needed = pass_needed(variants, c, published, changed, materials);
    if let Some(reason) = needed { variants.verify.reasons[reason] += 1; }
    if needed == Some(4) && variants.verify.on { count_shape_signals(shapes, variants, removed); }
    if needed.is_none() && !variants.verify.on {
        quiet(variants, cache, revision, c);
        return;
    }
    let skipped = needed.is_none();
    let settled_before = variants.readiness.as_ref().map(|r| r.settled.load(Ordering::Acquire));
    let lines_before = variants.shape_lines;
    let outcome = full_pass(commands, cache, variants, materials, blends, &mut shapes.p0(), c, revision, create_budget, swap_budget);
    variants.passes += u64::from(!skipped);
    if let Some(readiness) = variants.readiness.as_ref().filter(|_| !outcome.pending) { readiness.settled.store(revision, Ordering::Release); }
    if skipped {
        variants.verify.skipped += 1;
        variants.verify.since_line += 1;
        let last = variants.last.as_ref().expect("a skipped frame follows a pass");
        let settled_after = variants.readiness.as_ref().map(|r| r.settled.load(Ordering::Acquire));
        let mut what = Vec::new();
        for (name, n) in [("commands", outcome.commands), ("created", outcome.created), ("evicted", outcome.evicted), ("dropped-publications", outcome.dropped), ("shape-edits", outcome.edits), ("census-shape-lines", variants.shape_lines - lines_before)] {
            if n > 0 { what.push(format!("{name}={n}")); }
        }
        if outcome.census != last.census { what.push("census=changed".into()); }
        if settled_after != settled_before { what.push(format!("settled={settled_before:?}->{settled_after:?}")); }
        // [design] Pending turning true alone is not a divergence: it stores nothing
        // (`settled` already holds this revision) and changes no shape; it is named with
        // the other changes when there are some.
        if !what.is_empty() {
            if outcome.pending != last.pending { what.push(format!("pending={}->{}", last.pending, outcome.pending)); }
            variants.verify.diverged(&what.join(" "));
        }
        if variants.verify.since_line >= VERIFY_PERIOD {
            let passes = variants.passes;
            info!("{}", variants.verify.line(passes));
        }
    }
    variants.last = Some(LastPass { config: c, published, pending: outcome.pending, census: outcome.census.clone() });
    report(variants, cache, revision, &c, &outcome.census);
}

/// [design] A frame where nothing the full pass reads changed: the last pass ended settled
/// (not pending), so `settled` is stored again as it would have been, and the ready line
/// is refreshed from the last pass's census once [`REFRESH_FRAMES`] have passed.
fn quiet(variants: &mut Variants, cache: &SurfaceCache, revision: u64, c: BlendConfig) {
    let census = variants.last.as_ref().expect("a quiet frame follows a pass").census.clone();
    if let Some(readiness) = variants.readiness.as_ref() { readiness.settled.store(revision, Ordering::Release); }
    report(variants, cache, revision, &c, &census);
}

/// The ready line: at once for a new publication, otherwise for a changed census at most
/// once per [`REFRESH_FRAMES`] frames.
fn report(variants: &mut Variants, cache: &SurfaceCache, revision: u64, c: &BlendConfig, census: &Census) {
    variants.frames_since_report = variants.frames_since_report.saturating_add(1);
    if let Some(surface) = &cache.published {
        let published = variants.reported != Some(revision);
        let refreshed = variants.reported_census.as_ref() != Some(census) && variants.frames_since_report >= REFRESH_FRAMES;
        if published || refreshed {
            info!("{}", ready_line(surface, revision, c, census));
            variants.reported = Some(revision);
            variants.reported_census = Some(census.clone());
            variants.frames_since_report = 0;
            if variants.verify.on {
                let passes = variants.passes;
                info!("{}", variants.verify.line(passes));
            }
        }
    }
}

/// [design] The whole conversion of every tracked shape, from scratch: the decisions of
/// rounds 1-10 (unchanged by the gate), and the record of what it read
/// ([`Variants::watched`], [`Variants::surfaces`]).
#[allow(clippy::too_many_arguments)]
fn full_pass(
    commands: &mut Commands,
    cache: &SurfaceCache,
    variants: &mut Variants,
    materials: &Assets<NifMaterial>,
    blends: &mut Assets<NifBlendMaterial>,
    shapes: &mut Tracked,
    c: BlendConfig,
    revision: u64,
    create_budget: usize,
    swap_budget: usize,
) -> Outcome {
    let mut o = Outcome::default();
    let mut swapped = 0usize;
    if variants.generation != revision {
        variants.generation = revision;
        o.evicted += variants.materials.len();
        variants.materials.clear();
    }
    variants.watched.clear();
    variants.census_drawn.clear();
    // A producer can edit an owned source in place and republish the same handle
    // (NIF block material edits). Evict once per source before creating shared clones.
    let mut ordinary_live = HashSet::new();
    for (entity, shape, _, drawn, _, _) in shapes.iter() {
        if shape.is_none() { continue; }
        if let Some(drawn) = drawn {
            ordinary_live.insert(entity);
            if variants.ordinary_seen.insert(entity, drawn.last_changed().get()) != Some(drawn.last_changed().get()) {
                o.evicted += usize::from(variants.materials.remove(&drawn.0.id()).is_some());
            }
        }
    }
    variants.ordinary_seen.retain(|e, _| ordinary_live.contains(e));
    for (entity, shape, tracked, drawn, (fading, overridden), (no_shadow, see_through_mark)) in shapes.iter_mut() {
        let temporary = fading || overridden;
        let recorded = tracked.map_or(0, |t| t.reasons);
        let Some(mut shape) = shape else {
            // Census only: never converted.
            let source = drawn.as_ref().and_then(|d| materials.get(&d.0));
            if let Some(d) = &drawn {
                watch(&mut variants.watched, d.0.id(), source, false);
                variants.census_drawn.insert(entity, d.0.id());
            }
            let mask = shape_mask(recorded, temporary, source);
            if let Some(t) = tracked {
                o.census.count(t.base_type, mask);
                log_shape(variants, t, mask);
            }
            continue;
        };
        // [design] Respect a material override, retaining its actual source handle too.
        if let Some(drawn) = drawn.as_ref().filter(|d| d.0 != shape.original) {
            shape.original = drawn.0.clone();
            o.edits += 1;
        }
        let source = materials.get(&shape.original);
        let mask = shape_mask(recorded, temporary, source);
        watch(&mut variants.watched, shape.original.id(), source, mask == 0);
        if let Some(t) = tracked {
            o.census.count(t.base_type, mask);
            log_shape(variants, t, mask);
        }
        let allowed = mask == 0;
        o.census.eligible += usize::from(allowed);
        let (Some(surface), Some(source), true) = (cache.published.as_ref(), source, allowed) else {
            if shape.enabled.is_some() {
                commands.entity(entity).remove::<MeshMaterial3d<NifBlendMaterial>>().insert(MeshMaterial3d(shape.original.clone()));
                o.commands += 1;
                shape.enabled = None;
                o.edits += 1;
            }
            continue;
        };
        o.census.admit(source);
        let id = shape.original.id();
        let stamp = SourceStamp::of(source);
        let handle = match variants.materials.get(&id) {
            Some((handle, made)) if *made == stamp => Some(handle.clone()),
            // [design] #2956: every new clone is prepared by the renderer in the frame
            // after; a publication replaces the clone of every blended source at once (a
            // thousand or more around Primm: 50-120 ms frames at each cell crossing).
            // Past the frame's budget the shape keeps what it draws and the pass stays
            // pending, so the rest are made in the frames after.
            _ if o.created >= create_budget => None,
            _ => {
                let handle = blends.add(enabled_material(source, c, surface));
                variants.publications.insert(handle.id(), surface.clone());
                // A new clone of this publication: its coverage is checked next frame.
                variants.surfaces.entry(publication_key(surface)).or_insert_with(|| (surface.clone(), None)).1 = None;
                variants.materials.insert(id, (handle.clone(), stamp));
                o.created += 1;
                Some(handle)
            }
        };
        // Keep the old enabled handle until the render world acknowledges a prepared,
        // bound replacement (or until the replacement is made, past the budget). Source/
        // temporary owners still take precedence above.
        let handle = handle.filter(|h| variants.readiness.as_ref().is_none_or(|r| r.usable(h.id())));
        let Some(handle) = handle else {
            o.pending = true;
            // A non-temporary producer may have republished an ordinary source. Its
            // handle is recorded above; keep only the old drawable blend while the
            // replacement prepares, so two material types never compete for the entity.
            if shape.enabled.is_some() && drawn.is_some() {
                commands.entity(entity).remove::<MeshMaterial3d<NifMaterial>>();
                o.commands += 1;
            }
            continue;
        };
        if shape.enabled.as_ref() != Some(&handle) || drawn.is_some() {
            if swapped >= swap_budget {
                o.pending = true;
                continue;
            }
            swapped += 1;
            commands.entity(entity).remove::<MeshMaterial3d<NifMaterial>>().insert(MeshMaterial3d(handle.clone()));
            o.commands += 1;
            // [design] The shared bookkeeping runs on ordinary material changes only; a
            // source edited under the same handle is checked here, by the same rule.
            match (nvf_assets::nif_material::see_through(source), no_shadow, see_through_mark) {
                (true, false, _) => { commands.entity(entity).try_insert((NotShadowCaster, SeeThroughNoShadow)); }
                (false, true, true) => { commands.entity(entity).try_remove::<(NotShadowCaster, SeeThroughNoShadow)>(); }
                _ => {}
            }
        }
        if shape.enabled.as_ref() != Some(&handle) {
            shape.enabled = Some(handle);
            o.edits += 1;
        }
    }
    let sources: HashSet<_> = shapes.iter().filter_map(|(_, s, _, _, _, _)| s.map(|s| s.original.id())).collect();
    let before = variants.materials.len();
    variants.materials.retain(|id, _| sources.contains(id));
    o.evicted += before - variants.materials.len();
    let live: HashSet<_> = shapes.iter().filter_map(|(_, s, _, _, _, _)| s.and_then(|s| s.enabled.as_ref().map(Handle::id))).chain(variants.materials.values().map(|(h, _)| h.id())).collect();
    let before = variants.publications.len();
    variants.publications.retain(|id, _| live.contains(id));
    o.dropped += before - variants.publications.len();
    let kept: HashSet<PublicationKey> = variants.publications.values().map(publication_key).collect();
    variants.surfaces.retain(|key, _| kept.contains(key));
    o
}

/// One bounded `census-shape` line per distinct base form, shape index and reason mask;
/// shapes whose source material is not loaded yet are left for later.
fn log_shape(variants: &mut Variants, t: &BlendCensus, mask: u32) {
    if mask & reason::MISSING_SOURCE != 0 || variants.shape_lines > SHAPE_LINES { return; }
    if !variants.logged.insert((t.base, t.shape, mask)) { return; }
    variants.shape_lines += 1;
    if variants.shape_lines > SHAPE_LINES {
        info!("mesh terrain blend: census-shape lines stopped at {SHAPE_LINES}; the ready line still counts every shape");
        return;
    }
    info!("{}", shape_line(t, mask));
}

/// `census-shape ref=0x... base=0x... model=... shape=N status=eligible|excluded
/// first-reason=... reasons=a+b base-type=STAT|SCOL` (reference: the first seen with this
/// base, shape and mask; round 7's `base-type` last, so the earlier fields keep their
/// order and neighbours).
fn shape_line(t: &BlendCensus, mask: u32) -> String {
    format!("mesh terrain blend: census-shape ref=0x{:08X} base=0x{:08X} model={} shape={} status={} first-reason={} reasons={} base-type={}",
        t.reference, t.base, t.model, t.shape, if mask == 0 { "eligible" } else { "excluded" }, reason::first(mask), reason::list(mask), t.base_type.name())
}

/// `ready eligible-shapes=... resident-cells=... generation=...` stays the line's prefix and
/// round 5's `variant= shape= resolution= shading=` follow unchanged; then the weight fields
/// and the census. The resolution is the published atlas's. Round 10a ends it with
/// ` detail=layers` when the publication supplies layer detail for at least one quadrant
/// (` detail-quadrants=N` follows), else ` detail=colour`.
fn ready_line(surface: &crate::terrain_surface::PublishedSurface, generation: u64, c: &BlendConfig, census: &Census) -> String {
    let detail = if surface.detail.is_empty() { " detail=colour".to_string() } else { format!(" detail=layers detail-quadrants={}", surface.detail.len()) };
    format!("mesh terrain blend: ready eligible-shapes={} resident-cells={} generation={generation} variant={} shape={} resolution={} shading={}{}{}{detail}", census.eligible, surface.cells, c.variant.name(), c.shape.name(), surface.resolution, c.shading.name(), weight_fields(c), census.fields())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvf_assets::nif_material::NifSurface;

    fn configured(mut conversion: Conversion) {
        apply_configured_variants(&mut conversion, parse("24:status").unwrap());
    }
    fn publish(app: &mut App) {
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let surface = crate::terrain_surface::PublishedSurface { colour: images.add(Image::default()), height: images.add(Image::default()), origin: (-18, -1), cells: 9, resolution: 1024, revision: 7, coverage: 511, ..default() };
        app.init_resource::<SurfaceCache>();
        let mut cache = app.world_mut().resource_mut::<SurfaceCache>();
        cache.generation = 7;
        cache.published = Some(surface);
    }
    fn simulation(app: &mut App) {
        simulation_with(app, fnv_sim::RefTable::default());
    }
    fn simulation_with(app: &mut App, refs: fnv_sim::RefTable) {
        let forms = Box::leak(Box::<fnv_sim::FormStore>::default());
        let refs = Box::leak(Box::new(refs));
        app.insert_resource(nvf_play::sim::SimState { forms, refs, world: fnv_sim::World::new(forms, refs, default()) });
    }
    /// A placed reference 42 that starts disabled: a Disable fade (nvf_play fade.rs) runs
    /// only for a reference on its way out.
    fn disabled_reference() -> fnv_sim::RefTable {
        let mut placed = crate::outfits::tests::placed_ref(42, *b"REFR");
        placed.flags |= fnv_sim::refs::FLAG_INITIALLY_DISABLED;
        let mut refs = fnv_sim::RefTable::default();
        refs.refs.insert(fnv_sim::FormId(42), placed);
        refs
    }
    #[test]
    fn publication_followed_by_scripted_swap_recreates_enabled_clone_and_restores_swap() {
        use fnv_game::functions::misc::TextureSwaps;
        #[derive(Resource, Default)] struct Swaps(TextureSwaps);
        let (mut app, _files) = crate::outfits::tests::asset_fixture();
        simulation(&mut app);
        app.insert_resource(crate::outfits::tests::empty_world_data())
            .init_resource::<Variants>().init_resource::<Assets<NifBlendMaterial>>()
            .init_resource::<crate::texture_swap::Swapped>().init_resource::<Swaps>()
            .add_systems(Update, |swaps: Res<Swaps>, sim: Res<nvf_play::sim::SimState>, world: Res<crate::world::WorldData>, mut drawing: crate::texture_swap::Swapping| drawing.sync(&swaps.0, &sim, &world))
            .add_systems(PostUpdate, conversion_order(configured));
        publish(&mut app);
        let original = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
        let range = crate::fade::drawn(8);
        let e = app.world_mut().spawn((BlendShape::new(original.clone()), MeshMaterial3d(original.clone()), nvf_play::RefVisual(42), nvf_assets::actor::ShapeName("road".into()), range.clone())).id();
        app.update();
        let before = app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap().0.clone();
        assert!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).is_none());
        // Synthetic BC1 DDS through the production texture loader; no game data.
        let texture = "textures\\synthetic-blend-swap.dds";
        let mut dds = vec![0u8; 136]; dds[..4].copy_from_slice(b"DDS ");
        for (at, value) in [(4, 124u32), (8, 0x1007), (12, 4), (16, 4), (28, 1), (76, 32), (80, 4), (108, 0x1000)] { dds[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
        dds[84..88].copy_from_slice(b"DXT1");
        app.world_mut().resource_mut::<nvf_assets::TextureCache>().offer(texture.into(), dds, 0.0);
        {
            let mut swaps = app.world_mut().resource_mut::<Swaps>();
            swaps.0.by_ref.insert(fnv_sim::FormId(42), vec![("road".into(), texture.into())]);
            swaps.0.generation = 1;
        }
        app.update();
        let shape = app.world().get::<BlendShape>(e).unwrap();
        assert_ne!(shape.original, original);
        let swapped = shape.original.clone();
        let after = app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap();
        assert_ne!(after.0, before);
        let image = app.world().resource::<Assets<NifMaterial>>().get(&swapped).unwrap().base.base_color_texture.clone().unwrap();
        assert_eq!(app.world().resource::<Assets<NifBlendMaterial>>().get(&after.0).unwrap().base.base_color_texture.as_ref(), Some(&image));
        assert!(app.world().resource::<Assets<NifMaterial>>().get(&original).unwrap().base.base_color_texture.is_none());
        assert!(app.world().get::<bevy::camera::visibility::VisibilityRange>(e) == Some(&range));
        app.update(); // The processed generation must keep the swap on enabled shapes.
        assert_eq!(app.world().get::<BlendShape>(e).unwrap().original, swapped);
        // NIF block material edits republish an owned source under the same handle.
        let before_edit = app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap().0.clone();
        app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&swapped).unwrap().base.perceptual_roughness = 0.31;
        app.world_mut().entity_mut(e).insert(MeshMaterial3d(swapped.clone()));
        app.update();
        let refreshed = &app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap().0;
        assert_ne!(refreshed, &before_edit);
        assert_eq!(app.world().resource::<Assets<NifBlendMaterial>>().get(refreshed).unwrap().base.perceptual_roughness, 0.31);
        app.world_mut().resource_mut::<SurfaceCache>().published = None;
        app.update();
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, swapped);
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_none());
    }

    #[test]
    fn published_shape_joins_script_fade_and_returns_to_blending_afterwards() {
        // [design] nvf_play fade.rs `start` skips a fade out of a reference that is not
        // going out (present, not held by a script's Disable); an unknown reference is
        // present, so the reference here is a placed one that starts disabled.
        let mut app = App::new();
        simulation_with(&mut app, disabled_reference());
        assert!(!app.world().resource::<nvf_play::sim::SimState>().present(42));
        app.init_resource::<Assets<NifMaterial>>().init_resource::<Assets<NifBlendMaterial>>()
            .init_resource::<Assets<Image>>().init_resource::<Variants>().init_resource::<Time<Virtual>>()
            .add_plugins(nvf_play::fade::FadePlugin).add_systems(PostUpdate, conversion_order(configured));
        publish(&mut app);
        let original = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
        let e = app.world_mut().spawn((BlendShape::new(original.clone()), MeshMaterial3d(original), nvf_play::RefVisual(42), Visibility::Inherited)).id();
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_some());
        app.world_mut().write_message(nvf_play::fade::RefFade { form_id: 42, fade_in: false });
        app.update();
        assert!(app.world().get::<nvf_play::fade::Fading>(e).is_some());
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_none());
        app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs(1));
        app.update();
        let handle = &app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0;
        assert_eq!(app.world().resource::<Assets<NifMaterial>>().get(handle).unwrap().base.base_color.alpha(), 0.5);
        app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs(2));
        app.update();
        assert!(app.world().get::<nvf_play::fade::Fading>(e).is_none());
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_some());
        assert_eq!(app.world().get::<Visibility>(e), Some(&Visibility::Hidden));
    }

    #[test]
    fn finished_fade_in_restores_shadows_before_blending_again() {
        use bevy::light::NotShadowCaster;
        use nvf_assets::nif_material::SeeThroughNoShadow;
        let mut app = App::new();
        simulation(&mut app); // reference 42 unknown, so present: a fade in runs
        app.init_resource::<Assets<NifMaterial>>().init_resource::<Assets<NifBlendMaterial>>()
            .init_resource::<Assets<Image>>().init_resource::<Variants>().init_resource::<Time<Virtual>>()
            .add_plugins(nvf_play::fade::FadePlugin)
            .add_systems(PostUpdate, (nvf_assets::nif_material::see_through_shadow_system(), conversion_order(configured)));
        publish(&mut app);
        let original = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
        let e = app.world_mut().spawn((BlendShape::new(original.clone()), MeshMaterial3d(original.clone()), nvf_play::RefVisual(42), Visibility::Inherited)).id();
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_some());
        assert!(app.world().get::<NotShadowCaster>(e).is_none());
        app.world_mut().write_message(nvf_play::fade::RefFade { form_id: 42, fade_in: true });
        app.update();
        // The see-through copy drawn while fading casts no shadow.
        assert!(app.world().get::<nvf_play::fade::Fading>(e).is_some());
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_none());
        assert!(app.world().get::<SeeThroughNoShadow>(e).is_some() && app.world().get::<NotShadowCaster>(e).is_some());
        app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs(3));
        app.update();
        assert!(app.world().get::<nvf_play::fade::Fading>(e).is_none());
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_some(), "blending again after the fade");
        assert!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).is_none());
        assert_eq!(app.world().get::<BlendShape>(e).unwrap().original, original);
        assert!(app.world().get::<SeeThroughNoShadow>(e).is_none(), "the fade's shadow marker is gone");
        assert!(app.world().get::<NotShadowCaster>(e).is_none(), "the restored mesh casts its shadow again");
        assert_eq!(app.world().get::<Visibility>(e), Some(&Visibility::Inherited));
        app.update(); // and it stays so
        assert!(app.world().get::<NotShadowCaster>(e).is_none());
    }

    #[test]
    fn scripted_nif_edit_in_postupdate_is_converted_in_the_same_frame() {
        // [design] nif_mirror's material edits run in PostUpdate (MirrorSystems) and
        // republish the edited source by command; the conversion follows them, so the
        // frame never ends with the ordinary material beside the blend one.
        #[derive(Resource)] struct Edit(Option<(Entity, Handle<NifMaterial>)>);
        fn edit(mut commands: Commands, mut pending: ResMut<Edit>, mut materials: ResMut<Assets<NifMaterial>>) {
            let Some((e, source)) = pending.0.take() else { return };
            materials.get_mut(&source).unwrap().base.perceptual_roughness = 0.27;
            commands.entity(e).insert(MeshMaterial3d(source));
        }
        let mut app = App::new();
        app.init_resource::<Assets<NifMaterial>>().init_resource::<Assets<NifBlendMaterial>>()
            .init_resource::<Assets<Image>>().init_resource::<Variants>().insert_resource(Edit(None))
            .add_systems(PostUpdate, (edit.in_set(crate::nif_mirror::MirrorSystems), conversion_order(configured)));
        publish(&mut app);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
        let e = app.world_mut().spawn((BlendShape::new(source.clone()), MeshMaterial3d(source.clone()))).id();
        app.update();
        let before = app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap().0.clone();
        app.world_mut().resource_mut::<Edit>().0 = Some((e, source.clone()));
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).is_none(), "no ordinary material left beside the blend one");
        let after = app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap().0.clone();
        assert_ne!(after, before);
        assert_eq!(app.world().resource::<Assets<NifBlendMaterial>>().get(&after).unwrap().base.perceptual_roughness, 0.27);
    }

    #[test]
    fn aid_parsing() {
        let config = |band: f32, variant, shape, resolution, shading| BlendConfig { band, variant, shape, resolution, shading, flat: band / 16.0, steep: band * 6.0 };
        use ContactShape::*;
        // Old short forms keep working, with the new defaults for what they leave out
        // (round 7: the height shape).
        assert_eq!(parse("surface").unwrap(), config(8.0, Variant::Surface, Height, 512, Shading::Full));
        assert_eq!(parse(":selection").unwrap(), config(8.0, Variant::Selection, Height, 512, Shading::Full));
        assert_eq!(parse("12:weight").unwrap().variant, Variant::Weight);
        assert_eq!(parse("24:normal").unwrap().variant, Variant::Normal);
        assert_eq!(parse("24:status").unwrap(), config(24.0, Variant::Status, Height, 512, Shading::Full));
        // The settings row's look is the defaults, written out (round 10b: 512 colour texels
        // per cell; an explicit 1024 stays 1024).
        assert_eq!(setting_config(), parse("8:surface:height:512:full").unwrap());
        assert_eq!(DEFAULT_RESOLUTION, 512);
        assert_eq!(parse("8:surface:height:1024:full").unwrap().resolution, 1024);
        assert_eq!(DEFAULT_SHAPE, Height);
        // The comparison sheet's values (handoff, round 5).
        assert_eq!(parse("24:surface:height:512:albedo").unwrap(), config(24.0, Variant::Surface, Height, 512, Shading::Albedo));
        assert_eq!(parse("8:surface:height:512:albedo").unwrap(), config(8.0, Variant::Surface, Height, 512, Shading::Albedo));
        assert_eq!(parse("8:surface:hybrid:512:albedo").unwrap(), config(8.0, Variant::Surface, Hybrid, 512, Shading::Albedo));
        assert_eq!(parse("8:surface:hybrid:512:full").unwrap(), config(8.0, Variant::Surface, Hybrid, 512, Shading::Full));
        assert_eq!(parse("8:surface:hybrid:1024:full").unwrap(), config(8.0, Variant::Surface, Hybrid, 1024, Shading::Full));
        assert_eq!(parse("8:surface:hybrid:2048:full").unwrap().resolution, 2048);
        assert_eq!(parse("8:surface:penetration:1024:full").unwrap().shape, Penetration);
        for view in ["weight", "status", "selection", "normal"] {
            assert_eq!(parse(&format!("8:{view}:hybrid:1024:full")).unwrap(), config(8.0, Variant::parse(view).unwrap(), Hybrid, 1024, Shading::Full));
        }
        // Without a band; empty parts take their defaults.
        assert_eq!(parse("weight:penetration").unwrap(), config(8.0, Variant::Weight, Penetration, 512, Shading::Full));
        assert_eq!(parse("surface::2048").unwrap(), config(8.0, Variant::Surface, Height, 2048, Shading::Full));
        assert_eq!(parse("6.5:surface:::albedo").unwrap(), config(6.5, Variant::Surface, Height, 512, Shading::Albedo));
        for value in ["", "24", "0:surface", "-1:weight", "NaN:surface", "inf:surface", "a:surface", "24:bad", "24:surface:weight",
            "surface:hybrid:768", "surface:hybrid:1024:shiny", "surface:hybrid:1024:full:extra", "8:surface:hybrid:1024:full:", "8:8:surface", "surface:24"] {
            assert!(parse(value).is_err(), "{value}");
        }
        // Hybrid v1 keeps round 5's hybrid; its widths are the unused defaults.
        assert_eq!(parse("8:surface:hybrid-v1:1024:full").unwrap(), config(8.0, Variant::Surface, HybridV1, 1024, Shading::Full));
    }

    #[test]
    fn aid_widths_extend_the_value_only_at_its_end() {
        let widths = |v: &str| parse(v).map(|c| (c.flat, c.band, c.steep));
        // Omitted: band / 16 and 6 band; the default band gives 0.5 and 48.
        assert_eq!(widths("surface"), Ok((0.5, 8.0, 48.0)));
        assert_eq!(widths("24:surface"), Ok((1.5, 24.0, 144.0)));
        // The comparison set (plan round 6, step 4).
        for (value, steep) in [("8:surface:hybrid:1024:full:0.5,32", 32.0), ("8:surface:hybrid:1024:full:0.5,48", 48.0),
            ("8:surface:hybrid:1024:full:0.5,64", 64.0), ("8:weight:hybrid:1024:full:0.5,48", 48.0)] {
            assert_eq!(widths(value), Ok((0.5, 8.0, steep)), "{value}");
        }
        assert_eq!(widths("8:surface:hybrid:::0.25,40"), Ok((0.25, 8.0, 40.0)), "empty parts before the widths take their defaults");
        assert!(parse("8:surface::::0.25,40").is_err(), "round 7: an empty shape part is height, which takes no widths");
        assert_eq!(widths("8:surface:hybrid:1024:full:8,8"), Ok((8.0, 8.0, 8.0)), "equal widths are ordered");
        assert_eq!(parse("8:surface:hybrid:1024:full:0.5,32").unwrap().width_controls(), Vec4::new(0.5, 32.0, 0.0, 0.0));
        assert_eq!(parse("8:surface:hybrid:1024:full:0.5,32").unwrap().widths(), ContactWidths { flat: 0.5, band: 8.0, steep: 32.0 });
        for value in [
            "8:surface:hybrid:1024:full:", "8:surface:hybrid:1024:full:0.5", "8:surface:hybrid:1024:full:0.5,", "8:surface:hybrid:1024:full:,48",
            "8:surface:hybrid:1024:full:0.5,48,64", "8:surface:hybrid:1024:full:0.5;48", "8:surface:hybrid:1024:full:a,48",
            "8:surface:hybrid:1024:full:0,48", "8:surface:hybrid:1024:full:-0.5,48", "8:surface:hybrid:1024:full:9,48",
            "8:surface:hybrid:1024:full:0.5,4", "8:surface:hybrid:1024:full:NaN,48", "8:surface:hybrid:1024:full:0.5,inf",
            "8:surface:hybrid:1024:full:0.5,48:extra",
            // Only hybrid v2 takes widths.
            "8:surface:height:1024:full:0.5,48", "8:surface:penetration:1024:full:0.5,48", "8:surface:hybrid-v1:1024:full:0.5,48",
            // A derived hybrid width that is not finite (round 7's default `height` derives
            // none, so the shape is named).
            "3e38:surface:hybrid",
        ] {
            assert!(parse(value).is_err(), "{value}");
        }
        // The old shapes keep accepting what they accepted.
        assert!(parse("3e38:surface:height").is_ok());
        assert!(parse("3e38:surface").is_ok(), "the default height shape derives no widths");
    }

    #[test]
    fn start_and_ready_lines_keep_their_prefixes_and_name_every_setting() {
        let c = parse("24:surface").unwrap();
        assert_eq!(start_line(&c, DetailMode::Layers), "mesh terrain blend: start band=24 variant=surface resolution=512 noise=0 shape=height shading=full weight-version=1 detail=layers");
        // Round 10a: the resolved detail mode ends the line, with the reason for the colour cache alone.
        assert!(start_line(&c, DetailMode::Colour("aid")).ends_with(" weight-version=1 detail=colour detail-reason=aid"));
        assert!(start_line(&c, DetailMode::Colour("device")).ends_with(" detail=colour detail-reason=device"));
        assert_eq!(detail_capability(None, false), DetailMode::Colour("no-device"));
        assert_eq!(detail_capability(None, true), DetailMode::Colour("aid"), "the aid wins before the device is asked");
        assert!(start_line(&c, DetailMode::Layers).starts_with("mesh terrain blend: start band=24 variant=surface resolution=512 noise=0 shape=height shading=full"), "round 5's prefix");
        assert_eq!(start_line(&parse("24:surface:hybrid").unwrap(), DetailMode::Layers), "mesh terrain blend: start band=24 variant=surface resolution=512 noise=0 shape=hybrid shading=full flat=1.5 steep=144 weight-version=2 detail=layers");
        assert_eq!(start_line(&setting_config(), DetailMode::Layers), "mesh terrain blend: start band=8 variant=surface resolution=512 noise=0 shape=height shading=full weight-version=1 detail=layers", "the settings row's look");
        assert_eq!(start_line(&parse("8:status:penetration:2048:albedo").unwrap(), DetailMode::Layers), "mesh terrain blend: start band=8 variant=status resolution=2048 noise=0 shape=penetration shading=albedo weight-version=1 detail=layers");
        assert_eq!(start_line(&parse("8:surface:hybrid:1024:full:0.5,32").unwrap(), DetailMode::Layers), "mesh terrain blend: start band=8 variant=surface resolution=1024 noise=0 shape=hybrid shading=full flat=0.5 steep=32 weight-version=2 detail=layers");
        let mut images = Assets::<Image>::default();
        let surface = crate::terrain_surface::PublishedSurface { colour: images.add(Image::default()), height: images.add(Image::default()), origin: (0, 0), cells: 9, resolution: 512, revision: 3, coverage: 511, ..default() };
        let mut census = Census::default();
        census.count(BaseType::Stat, 0);
        census.count(BaseType::Stat, reason::RIG | reason::PARTICLES);
        census.count(BaseType::Scol, reason::MISSING_SOURCE);
        census.admit(&NifMaterial { base: StandardMaterial { alpha_mode: AlphaMode::Blend, depth_bias: 2.0, ..default() }, extension: NifSurface { decal: true, alpha_cut: 0.1, ..default() } });
        census.eligible = 12;
        let line = ready_line(&surface, 3, &parse("8:status:height:512:albedo").unwrap(), &census);
        let prefix = "mesh terrain blend: ready eligible-shapes=12 resident-cells=9 generation=3 variant=status shape=height resolution=512 shading=albedo";
        assert!(line.starts_with(&format!("{prefix} weight-version=1 stat-shapes=2 excluded-shapes=2 pending-material-shapes=1 excluded-world=0 excluded-rig=1 excluded-particles=1 excluded-posed=0")), "{line}");
        assert!(line.contains(" excluded-missing-source=1 excluded-temporary=0 excluded-alpha-mode=0"), "{line}");
        assert!(line.ends_with(" excluded-depth-bias-nonfinite=0 excluded-billboard=0 excluded-environment-map=0 admitted-blend=1 admitted-bias=1 admitted-decal=1 admitted-mask=0 admitted-alpha-cut=1 scol-shapes=1 detail=colour"), "round 7: SCOL shapes after the census; round 10a: the detail mode last: {line}");
        let layered = crate::terrain_surface::PublishedSurface { detail: vec![((0, 0), 1), ((1, 0), 3)], ..surface.clone() };
        assert!(ready_line(&layered, 3, &parse("8:status:height:512:albedo").unwrap(), &census).ends_with(" scol-shapes=1 detail=layers detail-quadrants=2"));
        for (_, name) in reason::NAMES { assert!(line.contains(&format!(" excluded-{name}=")), "every reason is printed, zero included: {name}"); }
        let v2 = ready_line(&surface, 3, &parse("8:surface:hybrid").unwrap(), &Census::default());
        assert!(v2.starts_with("mesh terrain blend: ready eligible-shapes=0 resident-cells=9 generation=3 variant=surface shape=hybrid resolution=512 shading=full flat=0.5 steep=48 weight-version=2 stat-shapes=0 "), "{v2}");
        assert_eq!(parse("6.5:normal:penetration").unwrap().controls(), Vec4::new(6.5, 2.0, 1.0, 1.0));
        assert_eq!(parse("6.5:normal:hybrid-v1").unwrap().controls().z, 3.0);
    }

    #[test]
    fn shape_lines_name_reference_base_model_shape_and_first_reason() {
        let t = BlendCensus { reference: 0x0010_2A3B, base: 0x0001_F00D, base_type: BaseType::Stat, model: "landscape/roads/road01.nif".into(), shape: 2, reasons: 0 };
        assert_eq!(shape_line(&t, 0), "mesh terrain blend: census-shape ref=0x00102A3B base=0x0001F00D model=landscape/roads/road01.nif shape=2 status=eligible first-reason=none reasons=none base-type=STAT");
        assert_eq!(shape_line(&t, reason::SOFT | reason::UNLIT | reason::DEST), "mesh terrain blend: census-shape ref=0x00102A3B base=0x0001F00D model=landscape/roads/road01.nif shape=2 status=excluded first-reason=dest reasons=dest+unlit+soft base-type=STAT");
        let scol = BlendCensus { base_type: BaseType::Scol, model: "scol/synthetic_road.nif".into(), ..t.clone() };
        assert!(shape_line(&scol, 0).ends_with(" model=scol/synthetic_road.nif shape=2 status=eligible first-reason=none reasons=none base-type=SCOL"));
        // Bounded: one line per base, shape and mask; pending shapes wait; a hard cap.
        let mut variants = Variants::default();
        log_shape(&mut variants, &t, 0);
        log_shape(&mut variants, &BlendCensus { reference: 7, ..t.clone() }, 0);
        log_shape(&mut variants, &t, reason::MISSING_SOURCE);
        assert_eq!(variants.shape_lines, 1);
        log_shape(&mut variants, &t, reason::TEMPORARY);
        assert_eq!(variants.shape_lines, 2);
        for shape in 0..SHAPE_LINES + 10 { log_shape(&mut variants, &BlendCensus { shape: shape + 10, ..t.clone() }, 0); }
        assert_eq!(variants.shape_lines, SHAPE_LINES + 1, "stops after the cap and its one notice");
        // Every name is distinct and the precedence is the bit order.
        let names: HashSet<&str> = reason::NAMES.iter().map(|(_, n)| *n).collect();
        assert_eq!(names.len(), reason::NAMES.len());
        for (i, (bit, _)) in reason::NAMES.iter().enumerate() { assert_eq!(*bit, 1 << i); }
        assert!(!reason::NAMES.iter().any(|(_, n)| n.contains("error") || n.contains("shader")), "census lines never match the smoke error filter");
    }

    #[test]
    fn selection_excludes_other_records_interiors_animation_destruction_and_lod() {
        let plain = ReferenceFacts { exterior: true, ..default() };
        assert!(eligible_reference(b"STAT", plain));
        // Round 7: a static collection is one prebuilt model of statics, drawn as a STAT is.
        assert!(eligible_reference(b"SCOL", plain));
        assert_eq!((BaseType::of(b"STAT"), BaseType::of(b"SCOL")), (Some(BaseType::Stat), Some(BaseType::Scol)));
        for sig in [b"MSTT", b"DOOR", b"TREE", b"MISC", b"NPC_", b"PWAT", b"ACTI", b"FURN"] {
            assert!(!eligible_reference(sig, plain));
            assert_eq!(BaseType::of(sig), None);
        }
        // Every reference reason on its own, then all at once (independent bits).
        let cases = [
            (ReferenceFacts { exterior: false, ..plain }, reason::WORLD),
            (ReferenceFacts { rigged: true, ..plain }, reason::RIG),
            (ReferenceFacts { particles: true, ..plain }, reason::PARTICLES),
            (ReferenceFacts { posed: true, ..plain }, reason::POSED),
            (ReferenceFacts { damage_stages: true, ..plain }, reason::DAMAGE_STAGES),
            (ReferenceFacts { destructible: true, ..plain }, reason::DEST),
            (ReferenceFacts { distant: true, ..plain }, reason::DISTANT),
        ];
        for (facts, bit) in cases {
            assert_eq!(reference_reasons(facts), bit, "{facts:?}");
            assert!(!eligible_reference(b"STAT", facts));
            assert!(!eligible_reference(b"SCOL", facts), "the same reference rules for a SCOL");
        }
        let all = ReferenceFacts { exterior: false, rigged: true, particles: true, posed: true, damage_stages: true, destructible: true, distant: true };
        assert_eq!(reference_reasons(all), cases.iter().fold(0, |m, (_, b)| m | b));
        assert_eq!(reason::first(reference_reasons(all)), "world");
        assert_eq!(shape_reasons(false, false), 0);
        assert_eq!(shape_reasons(true, false), reason::EMITTANCE);
        assert_eq!(shape_reasons(false, true), reason::MORPH);
        assert_eq!(shape_reasons(true, true), reason::EMITTANCE | reason::MORPH);
        assert_eq!((billboard_reason(false), billboard_reason(true)), (0, reason::BILLBOARD));
        assert_eq!(reason::first(reason::BILLBOARD | reason::DEPTH_BIAS), "depth-bias-nonfinite", "billboard is last in precedence");
    }

    #[test]
    fn every_material_predicate_has_its_own_reason() {
        let m = |base: StandardMaterial, extension: NifSurface| NifMaterial { base, extension };
        let image = Assets::<Image>::default().reserve_handle();
        let cases: Vec<(NifMaterial, u32)> = vec![
            (m(StandardMaterial { alpha_mode: AlphaMode::Add, ..default() }, default()), reason::ALPHA_MODE),
            (m(StandardMaterial { alpha_mode: AlphaMode::Multiply, ..default() }, default()), reason::ALPHA_MODE),
            (m(StandardMaterial { alpha_mode: AlphaMode::Premultiplied, ..default() }, default()), reason::ALPHA_MODE),
            (m(StandardMaterial { alpha_mode: AlphaMode::AlphaToCoverage, ..default() }, default()), reason::ALPHA_MODE),
            (m(StandardMaterial { unlit: true, ..default() }, default()), reason::UNLIT),
            (m(default(), NifSurface { refract: true, ..default() }), reason::REFRACTION),
            (m(default(), NifSurface { first_person: true, ..default() }), reason::FIRST_PERSON),
            (m(default(), NifSurface { depth_write: true, ..default() }), reason::DEPTH_WRITE),
            (m(StandardMaterial { specular_transmission: 0.5, ..default() }, default()), reason::SPECULAR_TRANSMISSION),
            (m(StandardMaterial { diffuse_transmission: 0.5, ..default() }, default()), reason::DIFFUSE_TRANSMISSION),
            (m(default(), NifSurface { falloff: Vec4::new(1.0, 0.0, 1.0, 0.0), ..default() }), reason::FALLOFF),
            (m(default(), NifSurface { holes: Vec4::ZERO, ..default() }), reason::HOLES_VALUE),
            (m(default(), NifSurface { holes_texture: Some(image.clone()), ..default() }), reason::HOLES_TEXTURE),
            (m(default(), NifSurface { facegen_offset: Some(image.clone()), ..default() }), reason::FACEGEN),
            (m(default(), NifSurface { pipboy_screen: true, ..default() }), reason::PIPBOY),
            (m(default(), NifSurface { add_linear: true, ..default() }), reason::ADD_LINEAR),
            (m(default(), NifSurface { soft: Vec4::X, ..default() }), reason::SOFT),
            (m(StandardMaterial { depth_bias: f32::NAN, ..default() }, default()), reason::DEPTH_BIAS),
            // [design] #2841: a reflecting shape without the window flag (env_how z 0) is
            // left out like a window one (z 1): the reflection wins over the blend
            // (docs/design/graphics-enhanced.md "Selection").
            (m(default(), NifSurface { env: Vec4::new(1.0, 1.0, 0.0, 0.0), env_how: Vec4::new(0.0, 2.0, 0.0, 1.0), env_cube: Some(image.clone()), ..default() }), reason::ENVIRONMENT_MAP),
            (m(default(), NifSurface { env: Vec4::new(1.0, 0.0, 0.0, 0.0), env_how: Vec4::new(0.0, 0.0, 1.0, 1.0), env_cube: Some(image.clone()), ..default() }), reason::ENVIRONMENT_MAP),
            // A strength-0 run (NVF_PLAY_DEBUG_ENV_STRENGTH=0) still binds the cube: still out.
            (m(default(), NifSurface { env_how: Vec4::new(0.0, 2.0, 0.0, 1.0), env_cube: Some(image.clone()), ..default() }), reason::ENVIRONMENT_MAP),
        ];
        for (material, bit) in &cases {
            assert_eq!(material_reasons(material), *bit, "{}", reason::list(*bit));
            assert!(!eligible_material(material));
        }
        // Overlapping reasons stay separate bits.
        let both = m(StandardMaterial { unlit: true, alpha_mode: AlphaMode::Add, ..default() }, NifSurface { soft: Vec4::X, holes_texture: Some(image), ..default() });
        assert_eq!(material_reasons(&both), reason::UNLIT | reason::ALPHA_MODE | reason::SOFT | reason::HOLES_TEXTURE);
        // The ownership and missing-source states join the mask.
        assert_eq!(shape_mask(reason::DEST, true, None), reason::DEST | reason::TEMPORARY | reason::MISSING_SOURCE);
        assert_eq!(shape_mask(0, false, Some(&m(default(), default()))), 0);
    }

    #[test]
    fn enabled_clone_preserves_source_and_alpha_coverage_descriptors() {
        let mut images = Assets::<Image>::default();
        let surface = crate::terrain_surface::PublishedSurface { colour: images.add(Image::default()), height: images.add(Image::default()), origin: (-2, -1), cells: 1, resolution: 512, revision: 1, coverage: 511, ..default() };
        // Round 6 admissions: Blend, a finite depth bias, the lit decal flag and alpha_cut,
        // alone and combined, beside Opaque and Mask with hard and soft edges.
        let mut sources = Vec::new();
        for alpha_mode in [AlphaMode::Opaque, AlphaMode::Mask(0.4), AlphaMode::Blend] {
            for (depth_bias, decal, alpha_cut, hard_edges, blended_cutout) in [(0.0, false, 0.0, false, None), (3.5, true, 0.2, true, Some(0.5)), (-1.25, false, 0.1, false, None), (0.0, true, 0.0, true, None)] {
                sources.push(NifMaterial {
                    base: StandardMaterial { alpha_mode, double_sided: true, cull_mode: None, depth_bias, base_color: Color::srgba(0.5, 0.4, 0.3, 0.7), ..default() },
                    extension: NifSurface { decal, alpha_cut, hard_edges, blended_cutout, ..default() },
                });
            }
        }
        for source in &sources {
            assert!(eligible_material(source), "{:?} {:?}", source.base.alpha_mode, source.extension.decal);
            let c = parse("24:surface:hybrid:1024:full:1,100").unwrap();
            let enabled = enabled_material(source, c, &surface);
            assert_eq!(source.base.alpha_mode, enabled.base.alpha_mode);
            assert_eq!(source.base.cull_mode, enabled.base.cull_mode);
            assert_eq!(source.base.double_sided, enabled.base.double_sided);
            assert_eq!(source.base.depth_bias.to_bits(), enabled.base.depth_bias.to_bits(), "the exact bias");
            assert_eq!(source.base.base_color, enabled.base.base_color);
            assert_eq!(source.extension.alpha_cut.to_bits(), enabled.extension.alpha_cut.to_bits());
            assert_eq!(source.extension.decal, enabled.extension.decal);
            assert_eq!(source.extension.hard_edges, enabled.extension.hard_edges);
            assert_eq!(source.extension.depth_write, enabled.extension.depth_write);
            assert_eq!(source.extension.blended_cutout, enabled.extension.blended_cutout);
            assert_eq!(source.extension.holes, enabled.extension.holes);
            assert!(enabled.extension.mesh_terrain_colour.is_some());
            assert_eq!(enabled.extension.mesh_terrain, c.controls());
            assert_eq!(enabled.extension.mesh_terrain_widths, Vec4::new(1.0, 100.0, 0.0, 0.0));
            assert_eq!(nvf_assets::nif_material::NifSurfaceKey::from(&source.extension), nvf_assets::nif_surface_blend::NifSurfaceBlendKey::from(&enabled.extension).0);
        }
        for alpha_mode in [AlphaMode::Add, AlphaMode::Multiply, AlphaMode::Premultiplied, AlphaMode::AlphaToCoverage] {
            assert!(!eligible_material(&NifMaterial { base: StandardMaterial { alpha_mode, ..default() }, extension: default() }));
        }
        for extension in [NifSurface { refract: true, ..default() }, NifSurface { first_person: true, ..default() }, NifSurface { holes: Vec4::ZERO, ..default() }, NifSurface { depth_write: true, ..default() }] {
            assert!(!eligible_material(&NifMaterial { base: default(), extension }));
        }
        assert!(!eligible_material(&NifMaterial { base: StandardMaterial { unlit: true, ..default() }, extension: NifSurface { decal: true, ..default() } }), "an unlit decal stays out");
    }

    /// An app with the conversion and nothing else; the cache exists but is not published.
    fn conversion_app() -> App {
        let mut app = App::new();
        app.init_resource::<Assets<NifMaterial>>().init_resource::<Assets<NifBlendMaterial>>()
            .init_resource::<Assets<Image>>().init_resource::<Variants>().init_resource::<SurfaceCache>()
            .add_systems(PostUpdate, conversion_order(configured));
        app
    }
    fn tracked(shape: usize, reasons: u32) -> BlendCensus {
        BlendCensus { reference: 0x42, base: 0x99, base_type: BaseType::Stat, model: "synthetic.nif".into(), shape, reasons }
    }
    fn reported(app: &App) -> Census {
        app.world().resource::<Variants>().reported_census.clone().expect("a ready line")
    }
    fn reason_count(census: &Census, bit: u32) -> usize {
        census.reasons[reason::NAMES.iter().position(|(b, _)| *b == bit).unwrap()]
    }
    fn settle(app: &mut App) {
        for _ in 0..REFRESH_FRAMES { app.update(); }
    }

    #[test]
    fn a_publication_makes_at_most_the_frame_budget_of_clones_a_frame() {
        // #2956: each clone is prepared again by the renderer; a whole set at once was a
        // 50-120 ms frame at every cell crossing.
        let mut app = conversion_app();
        let n = CLONE_WORK_PER_FRAME * 2 + 5;
        let shapes: Vec<Entity> = (0..n).map(|i| {
            let m = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
            app.world_mut().spawn((tracked(i, 0), BlendShape::new(m.clone()), MeshMaterial3d(m))).id()
        }).collect();
        publish(&mut app);
        let converted = |app: &App| shapes.iter().filter(|e| app.world().get::<MeshMaterial3d<NifBlendMaterial>>(**e).is_some()).count();
        app.update();
        assert_eq!(app.world().resource::<Assets<NifBlendMaterial>>().len(), CLONE_WORK_PER_FRAME);
        assert_eq!(converted(&app), CLONE_WORK_PER_FRAME, "the rest keep their ordinary material");
        assert!(app.world().resource::<Variants>().last.as_ref().unwrap().pending, "the pass stays pending");
        app.update();
        assert_eq!(converted(&app), CLONE_WORK_PER_FRAME * 2);
        app.update();
        assert_eq!(converted(&app), n);
        assert!(!app.world().resource::<Variants>().last.as_ref().unwrap().pending);
        assert_eq!(app.world().resource::<Assets<NifBlendMaterial>>().len(), n);
    }

    #[test]
    fn census_counts_tracked_shapes_through_publication_arrivals_changes_and_removal() {
        let mut app = conversion_app();
        let (add, opaque, missing) = {
            let mut m = app.world_mut().resource_mut::<Assets<NifMaterial>>();
            let add = m.add(NifMaterial { base: StandardMaterial { alpha_mode: AlphaMode::Add, ..default() }, extension: default() });
            let opaque = m.add(NifMaterial { base: default(), extension: default() });
            (add, opaque, m.reserve_handle())
        };
        // A rigged STAT shape (census only), an Add material, a material not loaded yet.
        let rigged = app.world_mut().spawn((tracked(0, reason::RIG), MeshMaterial3d(opaque.clone()))).id();
        let added = app.world_mut().spawn((tracked(1, 0), BlendShape::new(add.clone()), MeshMaterial3d(add))).id();
        let pending = app.world_mut().spawn((tracked(2, 0), BlendShape::new(missing.clone()), MeshMaterial3d(missing.clone()))).id();
        app.update();
        assert!(app.world().resource::<Variants>().reported.is_none(), "nothing is reported before a publication");
        // A publication with no eligible shape is still reported.
        publish(&mut app);
        app.update();
        assert_eq!(app.world().resource::<Variants>().reported, Some(7));
        let c = reported(&app);
        assert_eq!((c.stat_shapes, c.excluded, c.pending), (3, 3, 1));
        assert_eq!((reason_count(&c, reason::RIG), reason_count(&c, reason::ALPHA_MODE), reason_count(&c, reason::MISSING_SOURCE)), (1, 1, 1));
        assert_eq!(c.admitted, [0; 5]);
        for e in [rigged, added, pending] { assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_none()); }
        assert!(app.world().get::<BlendShape>(rigged).is_none(), "tracking never adds a BlendShape");
        // A late arrival converts at once; the census line refreshes after the interval.
        let late = app.world_mut().spawn((tracked(3, 0), BlendShape::new(opaque.clone()), MeshMaterial3d(opaque.clone()))).id();
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(late).is_some());
        assert_eq!(reported(&app).stat_shapes, 3, "rate-limited");
        settle(&mut app);
        assert_eq!((reported(&app).stat_shapes, reported(&app).excluded), (4, 3));
        // The pending material loads: it converts and leaves the pending count.
        app.world_mut().resource_mut::<Assets<NifMaterial>>().insert(missing.id(), NifMaterial { base: StandardMaterial { alpha_mode: AlphaMode::Blend, ..default() }, extension: default() }).unwrap();
        settle(&mut app);
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(pending).is_some(), "an authored Blend material is admitted");
        let c = reported(&app);
        assert_eq!((c.stat_shapes, c.excluded, c.pending, c.admitted[0]), (4, 2, 0, 1));
        // A source edited under the same handle: the converted shape goes back to it.
        app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&opaque).unwrap().base.unlit = true;
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(late).is_none());
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(late).unwrap().0, opaque);
        settle(&mut app);
        assert_eq!(reason_count(&reported(&app), reason::UNLIT), 2, "the rigged shape shares the source");
        // Removal leaves the census.
        app.world_mut().despawn(rigged);
        settle(&mut app);
        let c = reported(&app);
        assert_eq!((c.stat_shapes, reason_count(&c, reason::RIG)), (3, 0));
    }

    #[test]
    fn authored_blend_is_admitted_while_temporary_owners_are_not() {
        let mut app = conversion_app();
        publish(&mut app);
        let authored = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: StandardMaterial { alpha_mode: AlphaMode::Blend, ..default() }, extension: default() });
        let e = app.world_mut().spawn((tracked(0, 0), BlendShape::new(authored.clone()), MeshMaterial3d(authored.clone()))).id();
        app.update();
        let enabled = app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap().0.clone();
        assert_eq!(app.world().resource::<Assets<NifBlendMaterial>>().get(&enabled).unwrap().base.alpha_mode, AlphaMode::Blend);
        // An override's copy is a Blend material too; ownership, not alpha, keeps it out.
        let cloak = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: StandardMaterial { alpha_mode: AlphaMode::Blend, base_color: Color::srgba(1.0, 1.0, 1.0, 0.3), ..default() }, extension: default() });
        assert!(eligible_material(app.world().resource::<Assets<NifMaterial>>().get(&cloak).unwrap()));
        app.world_mut().entity_mut(e).insert((MeshMaterial3d(cloak.clone()), MaterialOverride { source: authored.clone(), copy: cloak.clone() }));
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_none());
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, cloak);
        assert_eq!(reason_count(&reported(&app), reason::TEMPORARY), 0, "not refreshed yet");
        settle(&mut app);
        assert_eq!(reason_count(&reported(&app), reason::TEMPORARY), 1);
        // The owner restores the authored material: it blends again.
        app.world_mut().entity_mut(e).remove::<MaterialOverride>().insert(MeshMaterial3d(authored.clone()));
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_some());
        assert_eq!(app.world().get::<BlendShape>(e).unwrap().original, authored);
    }

    #[test]
    fn authored_blend_leaves_for_a_script_fade_and_returns_after_it() {
        // As published_shape_joins_script_fade_and_returns_to_blending_afterwards, with an
        // authored Blend source: the fade's copy is material-eligible, Fading keeps it out.
        let mut app = App::new();
        simulation_with(&mut app, disabled_reference());
        app.init_resource::<Assets<NifMaterial>>().init_resource::<Assets<NifBlendMaterial>>()
            .init_resource::<Assets<Image>>().init_resource::<Variants>().init_resource::<Time<Virtual>>()
            .add_plugins(nvf_play::fade::FadePlugin).add_systems(PostUpdate, conversion_order(configured));
        publish(&mut app);
        let authored = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: StandardMaterial { alpha_mode: AlphaMode::Blend, ..default() }, extension: default() });
        let e = app.world_mut().spawn((BlendShape::new(authored.clone()), MeshMaterial3d(authored.clone()), nvf_play::RefVisual(42), Visibility::Inherited)).id();
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_some());
        app.world_mut().write_message(nvf_play::fade::RefFade { form_id: 42, fade_in: false });
        app.update();
        let copy = app.world().get::<nvf_play::fade::Fading>(e).unwrap().copy().clone();
        assert!(eligible_material(app.world().resource::<Assets<NifMaterial>>().get(&copy).unwrap()), "alpha mode alone would admit the copy");
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_none());
        app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs(3));
        app.update();
        assert!(app.world().get::<nvf_play::fade::Fading>(e).is_none());
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_some());
        assert_eq!(app.world().get::<BlendShape>(e).unwrap().original, authored, "the fade restored the authored source");
    }

    #[test]
    fn blended_cutout_flips_rebuild_the_clone_in_the_same_frame() {
        use nvf_assets::nif_material::BlendedCutouts;
        let mut app = conversion_app();
        app.init_resource::<BlendedCutouts>().add_message::<AssetEvent<NifMaterial>>()
            .add_systems(PostUpdate, nvf_assets::nif_material::blended_cutout_system());
        publish(&mut app);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial {
            base: StandardMaterial { alpha_mode: AlphaMode::Blend, ..default() },
            extension: NifSurface { blended_cutout: Some(0.5), ..default() },
        });
        let e = app.world_mut().spawn((tracked(0, 0), BlendShape::new(source.clone()), MeshMaterial3d(source.clone()))).id();
        app.update();
        let drawn = |app: &App| {
            let handle = app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap().0.clone();
            (app.world().resource::<Assets<NifBlendMaterial>>().get(&handle).unwrap().base.alpha_mode, handle)
        };
        let (mode, blended) = drawn(&app);
        assert_eq!(mode, AlphaMode::Blend);
        for (on, want) in [(true, AlphaMode::Mask(0.5)), (false, AlphaMode::Blend)] {
            app.world_mut().insert_resource(BlendedCutouts(on));
            app.update();
            assert_eq!(app.world().resource::<Assets<NifMaterial>>().get(&source).unwrap().base.alpha_mode, want, "edited under the same handle");
            let (mode, handle) = drawn(&app);
            assert_eq!(mode, want, "the clone follows in the same frame");
            assert_ne!(handle, blended);
            assert!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).is_none());
            assert_eq!(app.world().get::<BlendShape>(e).unwrap().original, source, "no clone nesting: the source stays the original");
        }
    }

    #[test]
    fn same_handle_see_through_edits_refresh_the_shadow_markers() {
        let mut app = conversion_app();
        app.add_systems(PostUpdate, nvf_assets::nif_material::see_through_shadow_system());
        publish(&mut app);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
        let e = app.world_mut().spawn((tracked(0, 0), BlendShape::new(source.clone()), MeshMaterial3d(source.clone()))).id();
        app.update();
        assert!(app.world().get::<NotShadowCaster>(e).is_none());
        app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&source).unwrap().base.alpha_mode = AlphaMode::Blend;
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_some());
        assert!(app.world().get::<SeeThroughNoShadow>(e).is_some() && app.world().get::<NotShadowCaster>(e).is_some(), "a see-through source casts no shadow");
        app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&source).unwrap().base.alpha_mode = AlphaMode::Opaque;
        app.update();
        assert!(app.world().get::<SeeThroughNoShadow>(e).is_none() && app.world().get::<NotShadowCaster>(e).is_none(), "and casts again when opaque");
    }

    #[test]
    fn request_restarts_keep_enabled_material_on_every_intermediate_frame() {
        let mut app = conversion_app();
        publish(&mut app);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
        let entity = app.world_mut().spawn((BlendShape::new(source.clone()), MeshMaterial3d(source))).id();
        app.update();
        let old = app.world().get::<BlendShape>(entity).unwrap().enabled.clone().unwrap();
        for _ in 0..8 {
            app.world_mut().resource_mut::<SurfaceCache>().generation += 1;
            app.update();
            assert_eq!(app.world().get::<BlendShape>(entity).unwrap().enabled.as_ref(), Some(&old));
            assert_eq!(&app.world().get::<MeshMaterial3d<NifBlendMaterial>>(entity).unwrap().0, &old);
            assert!(app.world().get::<MeshMaterial3d<NifMaterial>>(entity).is_none());
            assert_eq!(app.world().resource::<Assets<NifBlendMaterial>>().len(), 1);
        }
    }
    #[test]
    fn replacement_keeps_old_clone_and_images_until_prepared_acknowledgement() {
        let mut app = conversion_app();
        publish(&mut app);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
        let entity = app.world_mut().spawn((BlendShape::new(source.clone()), MeshMaterial3d(source))).id();
        app.update();
        let old = app.world().get::<BlendShape>(entity).unwrap().enabled.clone().unwrap();
        let old_colour = app.world().resource::<Assets<NifBlendMaterial>>().get(&old).unwrap().extension.mesh_terrain_colour.clone().unwrap();
        let ready = BlendReadiness::default();
        ready.ready.lock().unwrap().insert(old.id());
        app.world_mut().resource_mut::<Variants>().readiness = Some(ready.clone());
        publish(&mut app);
        app.world_mut().resource_mut::<SurfaceCache>().published.as_mut().unwrap().revision = 8;
        let mut replacement = None;
        for _ in 0..6 {
            app.update();
            assert_eq!(app.world().get::<BlendShape>(entity).unwrap().enabled.as_ref(), Some(&old));
            let variants = app.world().resource::<Variants>();
            let next = variants.materials.values().next().unwrap().0.clone();
            assert_ne!(next, old);
            assert_eq!(*replacement.get_or_insert(next.clone()), next, "the prepared candidate is stable while waiting");
            assert_eq!(variants.publications.len(), 2, "old images retained through preparation");
            assert_eq!(app.world().resource::<Assets<NifBlendMaterial>>().get(&old).unwrap().extension.mesh_terrain_colour.as_ref(), Some(&old_colour));
            assert!(!ready.settled(8));
        }
        let next = replacement.unwrap();
        ready.ready.lock().unwrap().insert(next.id());
        app.update();
        assert_eq!(app.world().get::<BlendShape>(entity).unwrap().enabled.as_ref(), Some(&next));
        assert_eq!(app.world().resource::<Variants>().publications.len(), 1);
        assert!(ready.settled(8));
    }
    /// [design] Round 10a review (P2): one cell's normal map is removed from the image
    /// assets while another cell stays covered. The removal masks only that cell; the
    /// retained clone keeps drawing, its rebuilt palette (for the coverage update) and the
    /// new candidates' bind only images that are still there, so neither can wait on the
    /// removed one (`build_bind_group`'s retry); the palette numbers keep their entries;
    /// once the front settles the replacement starts and publishes without the cell.
    #[test]
    fn removed_palette_image_keeps_retained_clones_drawable_and_the_replacement_publishes() {
        let (mut app, _files, [a_base, a_normal, b_base, b_normal]) = crate::terrain_surface::tests::detail_fixture();
        app.init_resource::<Assets<NifBlendMaterial>>().init_resource::<Variants>().add_systems(PostUpdate, conversion_order(configured));
        let published = app.world().resource::<SurfaceCache>().published.clone().unwrap();
        assert_eq!(published.origin, (-1, -1));
        assert_eq!(published.detail, vec![((0, 0), 0), ((1, 0), 0)]);
        assert_eq!(published.palette.images, vec![Some(a_base.clone()), Some(a_normal.clone()), Some(b_base.clone()), Some(b_normal.clone())]);
        assert_eq!(published.palette.samplers, vec![Some(a_base.clone())]);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
        let entity = app.world_mut().spawn((BlendShape::new(source.clone()), MeshMaterial3d(source))).id();
        app.update();
        let clone = app.world().get::<BlendShape>(entity).unwrap().enabled.clone().unwrap();
        assert_eq!(app.world().resource::<Assets<NifBlendMaterial>>().get(&clone).unwrap().extension.palette, published.palette);
        // From here the render world acknowledges only what the test says.
        let ready = BlendReadiness::default();
        ready.ready.lock().unwrap().insert(clone.id());
        app.world_mut().resource_mut::<Variants>().readiness = Some(ready.clone());
        app.insert_resource(ready.clone());
        app.update();
        assert!(ready.settled(published.revision));
        // The removal (Assets::remove ignores strong handles) and its event.
        app.world_mut().resource_mut::<Assets<Image>>().remove(&a_normal);
        app.world_mut().resource_mut::<Messages<AssetEvent<Image>>>().write(AssetEvent::Removed { id: a_normal.id() });
        crate::terrain_surface::tests::tick(&mut app);
        let front = app.world().resource::<SurfaceCache>().published.clone().unwrap();
        assert_eq!(((front.coverage >> 4) & 1, (front.coverage >> 5) & 1), (0, 1), "(0, 0) masked, (1, 0) still covered");
        assert_eq!(front.palette.images, vec![None, None, Some(b_base.clone()), Some(b_normal.clone())], "same numbers, the masked cell's entries on the fallback");
        assert_eq!(front.palette.samplers, vec![Some(b_base.clone())], "the sampler represented by an image a covered quadrant reads");
        assert!(crate::terrain_surface::tests::job_colour(&app).is_none(), "the replacement waits for the front's materials to settle");
        let bindable = |app: &App, h: &Handle<NifBlendMaterial>| {
            let palette = &app.world().resource::<Assets<NifBlendMaterial>>().get(h).unwrap().extension.palette;
            let images = app.world().resource::<Assets<Image>>();
            palette.images.iter().chain(&palette.samplers).flatten().all(|i| images.contains(i))
        };
        for _ in 0..3 {
            app.update();
            assert_eq!(app.world().get::<BlendShape>(entity).unwrap().enabled.as_ref(), Some(&clone), "the retained clone keeps drawing");
            let material = app.world().resource::<Assets<NifBlendMaterial>>().get(&clone).unwrap();
            assert_eq!(material.extension.mesh_terrain_origin.z as u32, front.coverage);
            assert_eq!(material.extension.palette, front.palette);
            assert!(bindable(&app, &clone), "its rebuild needs no removed image");
            let candidate = app.world().resource::<Variants>().materials.values().next().unwrap().0.clone();
            assert_ne!(candidate, clone);
            assert!(bindable(&app, &candidate), "nor does the new candidate's");
        }
        // The candidate prepared: the front settles and the replacement starts, then
        // publishes, without the cell whose image is gone.
        let candidate = app.world().resource::<Variants>().materials.values().next().unwrap().0.clone();
        ready.ready.lock().unwrap().insert(candidate.id());
        app.update();
        assert_eq!(app.world().get::<BlendShape>(entity).unwrap().enabled.as_ref(), Some(&candidate));
        assert!(ready.settled(front.revision));
        crate::terrain_surface::tests::tick(&mut app);
        let colour = crate::terrain_surface::tests::job_colour(&app).expect("the replacement started");
        assert_ne!(colour, front.colour);
        crate::terrain_surface::tests::finish(&mut app);
        let next = app.world().resource::<SurfaceCache>().published.clone().unwrap();
        assert_eq!(next.colour, colour, "the replacement published");
        assert_eq!(next.detail, vec![((1, 0), 0)]);
        assert_eq!(next.palette.images, vec![Some(b_base), Some(b_normal)]);
    }
    #[test]
    fn first_clone_same_handle_emissive_edit_rejects_superseded_acknowledgement() {
        let mut app = conversion_app();
        publish(&mut app);
        let ready = BlendReadiness::default();
        app.world_mut().resource_mut::<Variants>().readiness = Some(ready.clone());
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
        let entity = app.world_mut().spawn((BlendShape::new(source.clone()), MeshMaterial3d(source.clone()))).id();
        app.update();
        let stale = app.world().resource::<Variants>().materials[&source.id()].0.clone();
        assert!(app.world().get::<BlendShape>(entity).unwrap().enabled.is_none());
        let emission = LinearRgba::rgb(0.7, 0.2, 0.1);
        let stamp = SourceStamp::of(app.world().resource::<Assets<NifMaterial>>().get(&source).unwrap());
        app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&source).unwrap().base.emissive = emission;
        assert_eq!(SourceStamp::of(app.world().resource::<Assets<NifMaterial>>().get(&source).unwrap()), stamp, "the component tick must catch this edit");
        // The mirror republishes its owned source under the same handle.
        app.world_mut().entity_mut(entity).insert(MeshMaterial3d(source.clone()));
        ready.ready.lock().unwrap().insert(stale.id());
        app.update();
        let next = app.world().resource::<Variants>().materials[&source.id()].0.clone();
        assert_ne!(next, stale);
        for _ in 0..4 {
            assert!(app.world().get::<BlendShape>(entity).unwrap().enabled.is_none());
            assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(entity).is_none());
            assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(entity).unwrap().0, source);
            assert_eq!(app.world().resource::<Assets<NifBlendMaterial>>().get(&next).unwrap().base.emissive, emission);
            app.update();
            assert_eq!(app.world().resource::<Variants>().materials[&source.id()].0, next);
        }
        ready.ready.lock().unwrap().insert(next.id());
        app.update();
        assert_eq!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(entity).unwrap().0, next);
        assert!(app.world().get::<MeshMaterial3d<NifMaterial>>(entity).is_none());
    }
    #[test]
    fn producer_replacement_while_preparing_keeps_one_drawable_material_and_new_source() {
        let mut app = conversion_app();
        publish(&mut app);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
        let entity = app.world_mut().spawn((BlendShape::new(source.clone()), MeshMaterial3d(source))).id();
        app.update();
        let old = app.world().get::<BlendShape>(entity).unwrap().enabled.clone().unwrap();
        let ready = BlendReadiness::default();
        app.world_mut().resource_mut::<Variants>().readiness = Some(ready.clone());
        let new_source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: StandardMaterial { perceptual_roughness: 0.3, ..default() }, extension: default() });
        app.world_mut().entity_mut(entity).insert(MeshMaterial3d(new_source.clone()));
        app.update();
        let next = app.world().resource::<Variants>().materials[&new_source.id()].0.clone();
        for _ in 0..4 {
            assert!(app.world().get::<MeshMaterial3d<NifMaterial>>(entity).is_none());
            assert_eq!(app.world().get::<BlendShape>(entity).unwrap().original, new_source);
            assert_eq!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(entity).unwrap().0, old);
            app.update();
            assert_eq!(app.world().resource::<Variants>().materials[&new_source.id()].0, next);
        }
        ready.ready.lock().unwrap().insert(next.id());
        app.update();
        assert_eq!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(entity).unwrap().0, next);
    }

    #[test]
    fn off_drops_preparing_candidates_and_on_requires_fresh_acknowledgement() {
        let _switching = switching();
        let mut app = App::new();
        switch_systems(&mut app, true);
        let ready = BlendReadiness::default();
        app.world_mut().resource_mut::<Variants>().readiness = Some(ready.clone());
        app.update();
        publish(&mut app);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
        let entity = app.world_mut().spawn((BlendShape::new(source.clone()), MeshMaterial3d(source.clone()))).id();
        app.update();
        let stale = app.world().resource::<Variants>().materials.values().next().unwrap().0.clone();
        assert!(app.world().get::<BlendShape>(entity).unwrap().enabled.is_none());
        set_row(&mut app, false); app.update();
        assert!(app.world().resource::<Variants>().materials.is_empty());
        assert!(app.world().resource::<Variants>().publications.is_empty());
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(entity).unwrap().0, source);
        ready.ready.lock().unwrap().insert(stale.id());
        set_row(&mut app, true); app.update();
        publish(&mut app);
        // The normal census backfill would add BlendShape again in the live plugin.
        app.world_mut().entity_mut(entity).insert(BlendShape::new(source));
        app.update();
        let candidate = app.world().resource::<Variants>().materials.values().next().unwrap().0.clone();
        assert_ne!(candidate, stale);
        assert!(app.world().get::<BlendShape>(entity).unwrap().enabled.is_none());
    }

    #[test]
    fn the_aid_overrides_the_row_and_the_row_draws_the_default_look() {
        let aid = parse("24:weight:hybrid").unwrap();
        assert_eq!(desired(None, false), None, "off by default");
        assert_eq!(desired(None, true), Some(setting_config()));
        assert_eq!(desired(Some(aid), false), Some(aid), "the aid turns it on for comparison runs");
        assert_eq!(desired(Some(aid), true), Some(aid), "and wins over the row");
    }

    /// Tests that switch the blend serialise on this: [`RECORDING`] is process-wide.
    static SWITCHING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn switching() -> std::sync::MutexGuard<'static, ()> {
        SWITCHING.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The plugin's switching schedule ([`MeshTerrainBlendPlugin`] without the world's
    /// capture and bake systems), with the row as given.
    fn switch_systems(app: &mut App, row: bool) {
        use crate::graphics::{Graphics, Preset};
        let mut graphics = Graphics::preset(Preset::Vanilla);
        graphics.mesh_terrain_blend = row;
        app.insert_resource(graphics).insert_resource(BlendGate(MaterialGate::new()))
            .init_resource::<Assets<NifMaterial>>().init_resource::<Assets<NifBlendMaterial>>().init_resource::<Assets<Image>>()
            .init_resource::<Assets<crate::terrain_surface::SurfaceCaptureMaterial>>()
            .init_resource::<Variants>().init_resource::<SurfaceCache>().init_resource::<BlendState>()
            .add_systems(Update, (follow_setting, close_gate.after(follow_setting).run_if(draining)))
            .add_systems(PostUpdate, conversion_order((stop.run_if(stopping), apply_variants.run_if(active)).chain().in_set(BlendConversion)));
    }
    fn set_row(app: &mut App, on: bool) {
        app.world_mut().resource_mut::<crate::graphics::Graphics>().mesh_terrain_blend = on;
    }
    fn gate_open(app: &App) -> bool {
        app.world().resource::<BlendGate>().0.is_open()
    }
    /// What the asset plugin's tracking does with the dropped clones (not in these apps).
    fn drop_unused_blends(app: &mut App) {
        let mut blends = app.world_mut().resource_mut::<Assets<NifBlendMaterial>>();
        let ids: Vec<_> = blends.ids().collect();
        for id in ids { blends.remove(id); }
    }

    /// #2661 round 7: the settings row switches the blend while the game runs. Off, no
    /// system converts, nothing is allocated and the material gate stays closed; on, the
    /// gate opens, the row's look starts and the conversion runs; off again, every
    /// converted shape gets its ordinary material back and the census, the landscape
    /// captures, the published atlas and the clones go; round 8: the gate closes once no
    /// blend material has been left for [`IDLE_FRAMES`] frames. (The test aid is not set in
    /// unit tests: `aid()` is `None`.)
    #[test]
    fn the_row_switches_the_blend_on_and_off_while_the_game_runs() {
        use crate::terrain_surface::{CapturePath, SurfaceCell};
        let _switching = switching();
        let mut app = App::new();
        switch_systems(&mut app, false);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
        // Off: a published surface and a tracked shape left over would still not convert.
        publish(&mut app);
        let e = app.world_mut().spawn((tracked(0, 0), BlendShape::new(source.clone()), MeshMaterial3d(source.clone()))).id();
        app.update();
        assert_eq!(app.world().resource::<BlendState>().active, None);
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_none(), "nothing converts while off");
        assert!(app.world().resource::<Assets<NifBlendMaterial>>().is_empty(), "no clone is made while off");
        assert!(!gate_open(&app), "a run that never turns the blend on keeps its materials' gate closed");
        // On.
        set_row(&mut app, true);
        app.update();
        let state = app.world().resource::<BlendState>();
        assert_eq!(state.active, Some(setting_config()));
        assert!(state.shapes_pending, "the census of what is drawn is queued");
        assert!(recording(), "spawns record the census and captures now");
        assert!(gate_open(&app), "the gate opens before the first clone is made");
        assert_eq!(app.world().resource::<SurfaceCache>().resolution, 512, "round 10b: the row's 512 colour texels per cell");
        publish(&mut app);
        let cell = app.world_mut().spawn(SurfaceCell { world: 0, grid: (0, 0), heights: vec![], quadrants: vec![], paths: [CapturePath::HeightOnly("missing-base-mesh"); 4], content_revision: 0, sources: vec![], dependencies: default(), detail: default() }).id();
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_some(), "the shape blends");
        assert!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).is_none());
        let generation = app.world().resource::<SurfaceCache>().generation;
        // Off again.
        set_row(&mut app, false);
        app.update();
        assert!(!recording());
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).map(|m| m.0.clone()), Some(source.clone()), "its ordinary material is back");
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_none());
        assert!(app.world().get::<BlendShape>(e).is_none() && app.world().get::<BlendCensus>(e).is_none(), "the census goes");
        assert!(app.world().get::<SurfaceCell>(cell).is_none(), "the landscape captures go");
        let cache = app.world().resource::<SurfaceCache>();
        assert!(cache.published.is_none() && cache.generation > generation, "the atlas is dropped; an old receipt cannot publish");
        assert!(app.world().resource::<Variants>().materials.is_empty(), "the clones are dropped");
        assert_eq!(app.world().resource::<BlendState>().active, None);
        // The gate waits while a blend material is still in the store, then for IDLE_FRAMES.
        for _ in 0..IDLE_FRAMES + 2 { app.update(); }
        assert!(gate_open(&app), "a clone not yet dropped keeps the gate open");
        drop_unused_blends(&mut app);
        for _ in 0..IDLE_FRAMES - 1 { app.update(); }
        assert!(gate_open(&app));
        app.update();
        assert!(!gate_open(&app), "nothing of the blend left: the gate closes");
        assert_eq!(app.world().resource::<BlendState>().draining, None);
        // A stale publication while off converts nothing.
        publish(&mut app);
        app.world_mut().entity_mut(e).insert((tracked(0, 0), BlendShape::new(source.clone())));
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_none());
        assert!(app.world().resource::<Assets<NifBlendMaterial>>().is_empty());
        // Back on while draining keeps the gate open.
        app.world_mut().entity_mut(e).remove::<(BlendShape, BlendCensus)>();
        set_row(&mut app, true);
        app.update();
        set_row(&mut app, false);
        app.update();
        assert_eq!(app.world().resource::<BlendState>().draining, Some(0));
        set_row(&mut app, true);
        app.update();
        assert!(gate_open(&app) && app.world().resource::<BlendState>().draining.is_none(), "back on before it closed");
    }

    /// #2661 round 8: switching off in the frame a texture swap publishes its copy keeps the
    /// copy. The swap consumed its generation, so a restoration of the old source would
    /// lose it for good (no `BlendShape` is left to bring it back).
    #[test]
    fn switching_off_during_a_texture_swap_keeps_the_swap() {
        use fnv_game::functions::misc::TextureSwaps;
        #[derive(Resource, Default)] struct Swaps(TextureSwaps);
        let _switching = switching();
        let (mut app, _files) = crate::outfits::tests::asset_fixture();
        simulation(&mut app);
        switch_systems(&mut app, true);
        app.insert_resource(crate::outfits::tests::empty_world_data())
            .init_resource::<crate::texture_swap::Swapped>().init_resource::<Swaps>()
            .add_systems(Update, |swaps: Res<Swaps>, sim: Res<nvf_play::sim::SimState>, world: Res<crate::world::WorldData>, mut drawing: crate::texture_swap::Swapping| drawing.sync(&swaps.0, &sim, &world));
        publish(&mut app);
        let original = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
        let e = app.world_mut().spawn((tracked(0, 0), BlendShape::new(original.clone()), MeshMaterial3d(original.clone()), nvf_play::RefVisual(42), nvf_assets::actor::ShapeName("road".into()))).id();
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_some(), "converted");
        // Synthetic BC1 DDS through the production texture loader; no game data.
        let texture = "textures\\synthetic-blend-swap-off.dds";
        let mut dds = vec![0u8; 136]; dds[..4].copy_from_slice(b"DDS ");
        for (at, value) in [(4, 124u32), (8, 0x1007), (12, 4), (16, 4), (28, 1), (76, 32), (80, 4), (108, 0x1000)] { dds[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
        dds[84..88].copy_from_slice(b"DXT1");
        app.world_mut().resource_mut::<nvf_assets::TextureCache>().offer(texture.into(), dds, 0.0);
        {
            let mut swaps = app.world_mut().resource_mut::<Swaps>();
            swaps.0.by_ref.insert(fnv_sim::FormId(42), vec![("road".into(), texture.into())]);
            swaps.0.generation = 1;
        }
        // The swap and the switch in the same frame.
        set_row(&mut app, false);
        app.update();
        let drawn = app.world().get::<MeshMaterial3d<NifMaterial>>(e).expect("an ordinary material").0.clone();
        assert_ne!(drawn, original, "the swap's copy stays, not the old source");
        assert!(app.world().resource::<Assets<NifMaterial>>().get(&drawn).unwrap().base.base_color_texture.is_some(), "the swapped texture is drawn");
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_none());
        assert!(app.world().get::<BlendShape>(e).is_none());
        // The swap consumed generation 1: nothing applies it again unless the copy stays.
        app.update();
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, drawn, "and stays");
    }

    /// #2661 round 8: switching off in the frame a temporary owner, a script fade or a
    /// material override, puts its copy on keeps the copy; the owner restores the source
    /// itself.
    #[test]
    fn switching_off_during_a_fade_or_an_override_keeps_the_owners_copy() {
        let _switching = switching();
        // A fade out starting in the same frame.
        let mut app = App::new();
        simulation_with(&mut app, disabled_reference());
        switch_systems(&mut app, true);
        app.init_resource::<Time<Virtual>>().add_plugins(nvf_play::fade::FadePlugin);
        publish(&mut app);
        let original = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
        let e = app.world_mut().spawn((tracked(0, 0), BlendShape::new(original.clone()), MeshMaterial3d(original.clone()), nvf_play::RefVisual(42), Visibility::Inherited)).id();
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_some(), "converted");
        app.world_mut().write_message(nvf_play::fade::RefFade { form_id: 42, fade_in: false });
        set_row(&mut app, false);
        app.update();
        let copy = app.world().get::<nvf_play::fade::Fading>(e).expect("the fade runs").copy().clone();
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).map(|m| m.0.clone()), Some(copy), "the fade's copy is drawn");
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_none() && app.world().get::<BlendShape>(e).is_none());
        app.world_mut().resource_mut::<Time<Virtual>>().advance_by(std::time::Duration::from_secs(3));
        app.update();
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).map(|m| m.0.clone()), Some(original.clone()), "the fade restores the source");

        // An override published in the same frame (before the switch's PostUpdate).
        let mut app = App::new();
        switch_systems(&mut app, true);
        publish(&mut app);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: default(), extension: default() });
        let e = app.world_mut().spawn((tracked(0, 0), BlendShape::new(source.clone()), MeshMaterial3d(source.clone()))).id();
        app.update();
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_some(), "converted");
        let cloak = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial { base: StandardMaterial { alpha_mode: AlphaMode::Blend, ..default() }, extension: default() });
        app.world_mut().entity_mut(e).insert((MeshMaterial3d(cloak.clone()), MaterialOverride { source: source.clone(), copy: cloak.clone() }));
        set_row(&mut app, false);
        app.update();
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).map(|m| m.0.clone()), Some(cloak), "the override's copy stays");
        assert!(app.world().get::<MaterialOverride>(e).is_some());
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_none() && app.world().get::<BlendShape>(e).is_none());
    }

    fn passes(app: &App) -> u64 {
        app.world().resource::<Variants>().passes
    }
    /// Frames until one runs no full pass: the pass's own commands may call for one
    /// settling pass, never a pass every frame.
    fn go_quiet(app: &mut App) {
        for _ in 0..4 {
            let before = passes(app);
            app.update();
            if passes(app) == before { return; }
        }
        panic!("the conversion keeps running its full pass");
    }
    fn assert_quiet(app: &mut App, frames: usize, what: &str) {
        let before = passes(app);
        for _ in 0..frames { app.update(); }
        assert_eq!(passes(app), before, "{what}: no full pass");
    }
    fn runs(app: &mut App, what: &str) {
        let before = passes(app);
        app.update();
        assert!(passes(app) > before, "{what} runs the full pass");
    }
    fn blends_on(app: &App, e: Entity) -> bool {
        app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_some() && app.world().get::<MeshMaterial3d<NifMaterial>>(e).is_none()
    }

    /// The change-driven conversion (graphics-enhanced.md §3.14 "Per frame"): in a steady
    /// state no full pass runs; each input change runs it in the frame it happens, and the
    /// pass's own commands settle within a frame.
    #[test]
    fn the_full_pass_runs_only_in_frames_where_an_input_changed() {
        let mut app = conversion_app();
        publish(&mut app);
        let (a, b, late) = {
            let mut m = app.world_mut().resource_mut::<Assets<NifMaterial>>();
            (m.add(NifMaterial::default()), m.add(NifMaterial { base: StandardMaterial { perceptual_roughness: 0.4, ..default() }, extension: default() }), m.reserve_handle())
        };
        let e = app.world_mut().spawn((tracked(0, 0), BlendShape::new(a.clone()), MeshMaterial3d(a.clone()))).id();
        app.world_mut().spawn((tracked(1, reason::RIG), MeshMaterial3d(a.clone())));
        app.update();
        assert!(blends_on(&app, e));
        go_quiet(&mut app);
        assert_quiet(&mut app, 5, "a steady state");
        // A new publication revision.
        app.world_mut().resource_mut::<SurfaceCache>().published.as_mut().unwrap().revision = 8;
        runs(&mut app, "a new publication revision");
        assert_eq!(app.world().resource::<Variants>().generation, 8);
        go_quiet(&mut app);
        assert_quiet(&mut app, 3, "after the publication");
        // A shape spawned, then despawned: its source and clone are collected.
        let f = app.world_mut().spawn((tracked(2, 0), BlendShape::new(b.clone()), MeshMaterial3d(b.clone()))).id();
        runs(&mut app, "a shape spawned");
        assert!(blends_on(&app, f));
        go_quiet(&mut app);
        let clone_b = app.world().resource::<Variants>().materials[&b.id()].0.id();
        assert!(app.world().resource::<Variants>().publications.contains_key(&clone_b));
        app.world_mut().despawn(f);
        runs(&mut app, "a shape despawned");
        let variants = app.world().resource::<Variants>();
        assert!(!variants.materials.contains_key(&b.id()), "the despawned shape's source leaves the clones");
        assert!(!variants.publications.contains_key(&clone_b), "and its clone the publications");
        assert!(!variants.watched.contains_key(&b.id()), "and the watched sources");
        go_quiet(&mut app);
        // An ordinary material put on a converted shape.
        app.world_mut().entity_mut(e).insert(MeshMaterial3d(a.clone()));
        runs(&mut app, "an ordinary material put on a converted shape");
        assert!(blends_on(&app, e));
        go_quiet(&mut app);
        // A source edited in place under the same handle, no component touched.
        app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&a).unwrap().base.unlit = true;
        runs(&mut app, "a source edited in place");
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, a, "an unlit source cannot blend");
        go_quiet(&mut app);
        app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&a).unwrap().base.unlit = false;
        runs(&mut app, "the edit undone");
        assert!(blends_on(&app, e));
        go_quiet(&mut app);
        // A store change no tracked shape reads does not run it.
        app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
        assert_quiet(&mut app, 2, "a material no tracked shape draws");
        // A source material that loads later.
        let g = app.world_mut().spawn((tracked(3, 0), BlendShape::new(late.clone()), MeshMaterial3d(late.clone()))).id();
        app.update();
        go_quiet(&mut app);
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(g).is_none(), "not loaded yet");
        app.world_mut().resource_mut::<Assets<NifMaterial>>().insert(late.id(), NifMaterial::default()).unwrap();
        runs(&mut app, "a source that loads later");
        assert!(blends_on(&app, g));
        go_quiet(&mut app);
        // A temporary owner's marker added, then removed.
        app.world_mut().entity_mut(e).insert(MaterialOverride { source: a.clone(), copy: a.clone() });
        runs(&mut app, "a temporary marker added");
        assert!(app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).is_none(), "a temporary owner keeps it out");
        go_quiet(&mut app);
        app.world_mut().entity_mut(e).remove::<MaterialOverride>();
        runs(&mut app, "a temporary marker removed");
        assert!(blends_on(&app, e));
        go_quiet(&mut app);
        assert_quiet(&mut app, 5, "the steady state again");
    }

    #[test]
    fn a_pending_replacement_keeps_the_pass_running_until_it_is_usable() {
        let mut app = conversion_app();
        publish(&mut app);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
        let entity = app.world_mut().spawn((BlendShape::new(source.clone()), MeshMaterial3d(source))).id();
        app.update();
        let old = app.world().get::<BlendShape>(entity).unwrap().enabled.clone().unwrap();
        let ready = BlendReadiness::default();
        ready.ready.lock().unwrap().insert(old.id());
        app.world_mut().resource_mut::<Variants>().readiness = Some(ready.clone());
        go_quiet(&mut app);
        assert!(ready.settled(7), "quiet frames store settled as before");
        app.world_mut().resource_mut::<SurfaceCache>().published.as_mut().unwrap().revision = 8;
        for _ in 0..4 {
            runs(&mut app, "a pending replacement");
            assert_eq!(app.world().get::<BlendShape>(entity).unwrap().enabled.as_ref(), Some(&old));
            assert!(!ready.settled(8));
        }
        let next = app.world().resource::<Variants>().materials.values().next().unwrap().0.clone();
        ready.ready.lock().unwrap().insert(next.id());
        runs(&mut app, "the replacement usable");
        assert_eq!(app.world().get::<BlendShape>(entity).unwrap().enabled.as_ref(), Some(&next));
        assert!(ready.settled(8));
        go_quiet(&mut app);
        assert_quiet(&mut app, 3, "settled");
        assert!(ready.settled(8));
    }

    #[test]
    fn a_deferred_census_report_comes_from_quiet_frames() {
        let mut app = conversion_app();
        publish(&mut app);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
        app.world_mut().spawn((tracked(0, 0), BlendShape::new(source.clone()), MeshMaterial3d(source.clone())));
        app.update();
        assert_eq!(reported(&app).stat_shapes, 1);
        go_quiet(&mut app);
        app.world_mut().spawn((tracked(1, reason::RIG), MeshMaterial3d(source)));
        runs(&mut app, "a census-only shape spawned");
        assert_eq!(reported(&app).stat_shapes, 1, "rate-limited");
        go_quiet(&mut app);
        let before = passes(&app);
        settle(&mut app);
        assert_eq!(passes(&app), before, "no full pass while waiting");
        let c = reported(&app);
        assert_eq!((c.stat_shapes, reason_count(&c, reason::RIG)), (2, 1), "the deferred report came");
    }

    /// The coverage of a retained publication's clones follows the cache in the frame it
    /// changes ([`follow_coverage`]); a front coverage change is a new revision, so the
    /// pass runs too.
    #[test]
    fn a_coverage_change_restricts_retained_clones_and_runs_the_pass() {
        let (mut app, _files, [_, a_normal, _, _]) = crate::terrain_surface::tests::detail_fixture();
        app.init_resource::<Assets<NifBlendMaterial>>().init_resource::<Variants>().add_systems(PostUpdate, conversion_order(configured));
        let published = app.world().resource::<SurfaceCache>().published.clone().unwrap();
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
        let entity = app.world_mut().spawn((BlendShape::new(source.clone()), MeshMaterial3d(source))).id();
        app.update();
        let clone = app.world().get::<BlendShape>(entity).unwrap().enabled.clone().unwrap();
        go_quiet(&mut app);
        assert_quiet(&mut app, 3, "a steady state");
        app.world_mut().resource_mut::<Assets<Image>>().remove(&a_normal);
        app.world_mut().resource_mut::<Messages<AssetEvent<Image>>>().write(AssetEvent::Removed { id: a_normal.id() });
        crate::terrain_surface::tests::tick(&mut app);
        let front = app.world().resource::<SurfaceCache>().published.clone().unwrap();
        assert_ne!(front.coverage, published.coverage);
        runs(&mut app, "a coverage change");
        let material = app.world().resource::<Assets<NifBlendMaterial>>().get(&clone).unwrap();
        assert_eq!(material.extension.mesh_terrain_origin.z as u32, front.coverage, "the retained clone follows the coverage");
        assert_eq!(material.extension.palette, front.palette);
        go_quiet(&mut app);
    }

    /// A temporary owner's copy whose alpha moves every frame (a fade's, an override's) is
    /// read only by a shape kept out, so its edits run no full pass; the marker's removal
    /// does. [design] Only a shape that may blend records its source's stamp.
    #[test]
    fn a_kept_out_shapes_animated_copy_runs_no_pass_until_its_marker_goes() {
        let mut app = conversion_app();
        publish(&mut app);
        let (a, copy) = {
            let mut m = app.world_mut().resource_mut::<Assets<NifMaterial>>();
            (m.add(NifMaterial::default()), m.add(NifMaterial { base: StandardMaterial { alpha_mode: AlphaMode::Blend, ..default() }, extension: default() }))
        };
        let e = app.world_mut().spawn((tracked(0, 0), BlendShape::new(a.clone()), MeshMaterial3d(a.clone()))).id();
        app.update();
        go_quiet(&mut app);
        app.world_mut().entity_mut(e).insert((MeshMaterial3d(copy.clone()), MaterialOverride { source: a.clone(), copy: copy.clone() }));
        runs(&mut app, "a temporary owner's copy put on");
        go_quiet(&mut app);
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, copy);
        let before = passes(&app);
        for frame in 0..8 {
            let alpha = 0.9 - frame as f32 * 0.1;
            app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&copy).unwrap().base.base_color.set_alpha(alpha);
            app.update();
        }
        assert_eq!(passes(&app), before, "the copy's alpha edits run no full pass while the marker stays");
        app.world_mut().entity_mut(e).remove::<MaterialOverride>().insert(MeshMaterial3d(a.clone()));
        runs(&mut app, "the marker removed");
        assert!(blends_on(&app, e));
    }

    /// A census-only shape (an animated rig's, kept out by its recorded reason) whose own
    /// material is posed and published again under the same handle every frame (nvf-assets
    /// material_anim.rs) runs no full pass; another handle on it does.
    #[test]
    fn a_census_only_shapes_same_handle_republish_runs_no_pass() {
        let mut app = conversion_app();
        publish(&mut app);
        let (own, other) = {
            let mut m = app.world_mut().resource_mut::<Assets<NifMaterial>>();
            (m.add(NifMaterial::default()), m.add(NifMaterial { base: StandardMaterial { unlit: true, ..default() }, extension: default() }))
        };
        let rig = app.world_mut().spawn((tracked(0, reason::RIG), MeshMaterial3d(own.clone()))).id();
        app.update();
        go_quiet(&mut app);
        let before = passes(&app);
        for frame in 0..8 {
            app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&own).unwrap().base.base_color.set_alpha(1.0 - frame as f32 * 0.1);
            app.world_mut().entity_mut(rig).insert(MeshMaterial3d(own.clone()));
            app.update();
        }
        assert_eq!(passes(&app), before, "the same handle published again every frame runs no full pass");
        app.world_mut().entity_mut(rig).insert(MeshMaterial3d(other.clone()));
        runs(&mut app, "another handle on a census-only shape");
        assert_eq!(app.world().resource::<Variants>().census_drawn.get(&rig), Some(&other.id()));
        go_quiet(&mut app);
        settle(&mut app);
        assert_eq!(reason_count(&reported(&app), reason::UNLIT), 1, "the census follows the new handle's reasons");
    }

    /// A convertible shape's owned source edited in place in a field the stamp does not
    /// hold (emissive) and republished under the same handle still runs the pass, which
    /// evicts the clone and rebuilds it (the `ordinary_seen` eviction); the edit alone does not.
    #[test]
    fn a_convertible_shapes_same_handle_republish_still_runs_the_pass_and_evicts() {
        let mut app = conversion_app();
        publish(&mut app);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
        let e = app.world_mut().spawn((tracked(0, 0), BlendShape::new(source.clone()), MeshMaterial3d(source.clone()))).id();
        app.update();
        go_quiet(&mut app);
        let before = app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap().0.clone();
        let emission = LinearRgba::rgb(0.6, 0.3, 0.1);
        app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&source).unwrap().base.emissive = emission;
        assert_quiet(&mut app, 1, "an edit the stamp does not hold, not yet republished");
        app.world_mut().entity_mut(e).insert(MeshMaterial3d(source.clone()));
        runs(&mut app, "the same handle republished on a convertible shape");
        let after = app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap().0.clone();
        assert_ne!(after, before, "evicted and rebuilt");
        assert_eq!(app.world().resource::<Assets<NifBlendMaterial>>().get(&after).unwrap().base.emissive, emission);
        assert!(blends_on(&app, e));
    }

    /// A source read both ways keeps its stamp whichever reader is recorded first.
    #[test]
    fn watch_keeps_the_stamp_of_a_source_read_both_ways_in_either_order() {
        let mut store = Assets::<NifMaterial>::default();
        let id = store.add(NifMaterial::default()).id();
        let source = store.get(id);
        for order in [[false, true], [true, false]] {
            let mut watched = HashMap::new();
            for allowed in order { watch(&mut watched, id, source, allowed); }
            assert!(stamped(&watched[&id]), "{order:?}");
            assert_eq!(watched[&id], fingerprint(source, true));
        }
        let mut watched = HashMap::new();
        for _ in 0..2 { watch(&mut watched, id, source, false); }
        assert!(!stamped(&watched[&id]), "kept-out readers alone record no stamp");
    }

    /// A kept-out shape (an override's marker) and an allowed shape read one source, in
    /// both spawn orders: an alpha edit of the source runs the pass and rebuilds the
    /// allowed shape's clone.
    #[test]
    fn a_source_read_both_ways_keeps_its_stamp_watched_in_either_spawn_order() {
        for kept_out_first in [true, false] {
            let mut app = conversion_app();
            publish(&mut app);
            let s = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
            let kept_out = (tracked(0, 0), BlendShape::new(s.clone()), MeshMaterial3d(s.clone()), MaterialOverride { source: s.clone(), copy: s.clone() });
            let allowed = (tracked(1, 0), BlendShape::new(s.clone()), MeshMaterial3d(s.clone()));
            let e = if kept_out_first {
                app.world_mut().spawn(kept_out);
                app.world_mut().spawn(allowed).id()
            } else {
                let e = app.world_mut().spawn(allowed).id();
                app.world_mut().spawn(kept_out);
                e
            };
            app.update();
            go_quiet(&mut app);
            assert!(blends_on(&app, e), "kept_out_first={kept_out_first}");
            assert!(stamped(&app.world().resource::<Variants>().watched[&s.id()]), "kept_out_first={kept_out_first}");
            let before = app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap().0.clone();
            app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&s).unwrap().base.base_color.set_alpha(0.5);
            runs(&mut app, "an alpha edit of a source read both ways");
            let after = app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap().0.clone();
            assert_ne!(after, before, "the allowed shape's clone is rebuilt, kept_out_first={kept_out_first}");
            assert_eq!(app.world().resource::<Assets<NifBlendMaterial>>().get(&after).unwrap().base.base_color.alpha(), 0.5);
        }
    }

    /// An allowed shape's source edited under its handle in a stamped field (alpha) still
    /// runs the pass, and the clone is rebuilt.
    #[test]
    fn an_allowed_shapes_source_alpha_edit_runs_the_pass() {
        let mut app = conversion_app();
        publish(&mut app);
        let a = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
        let e = app.world_mut().spawn((tracked(0, 0), BlendShape::new(a.clone()), MeshMaterial3d(a.clone()))).id();
        app.update();
        go_quiet(&mut app);
        let before = app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap().0.clone();
        app.world_mut().resource_mut::<Assets<NifMaterial>>().get_mut(&a).unwrap().base.base_color.set_alpha(0.5);
        runs(&mut app, "an allowed shape's source alpha edit");
        let after = app.world().get::<MeshMaterial3d<NifBlendMaterial>>(e).unwrap().0.clone();
        assert_ne!(after, before, "the clone is rebuilt");
        assert_eq!(app.world().resource::<Assets<NifBlendMaterial>>().get(&after).unwrap().base.base_color.alpha(), 0.5);
    }

    /// The verify aid runs the full pass in frames the gate skips and reports any change;
    /// an edit the gate cannot see (the store's change mark bypassed) is found.
    #[test]
    fn the_verify_aid_runs_skipped_frames_and_reports_a_missed_change() {
        let mut app = conversion_app();
        publish(&mut app);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
        let e = app.world_mut().spawn((tracked(0, 0), BlendShape::new(source.clone()), MeshMaterial3d(source.clone()))).id();
        app.update();
        go_quiet(&mut app);
        app.world_mut().resource_mut::<Variants>().verify.on = true;
        for _ in 0..5 { app.update(); }
        let verify = &app.world().resource::<Variants>().verify;
        assert_eq!((verify.skipped, verify.diverged), (5, 0), "every steady frame verified, nothing differs");
        app.world_mut().resource_mut::<Assets<NifMaterial>>().bypass_change_detection().get_mut_untracked(&source).unwrap().base.unlit = true;
        app.update();
        let verify = &app.world().resource::<Variants>().verify;
        assert_eq!((verify.skipped, verify.diverged), (6, 1), "the unseen edit is a divergence");
        assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(e).unwrap().0, source, "and the verify pass acts on it");
    }

    #[test]
    fn swaps_above_per_frame_budget_stay_pending_and_finish_on_next_frame() {
        let mut app = conversion_app();
        publish(&mut app);
        let source = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
        let total = SWAP_WORK_PER_FRAME + 20;
        let entities: Vec<Entity> = (0..total)
            .map(|i| app.world_mut().spawn((tracked(i, 0), BlendShape::new(source.clone()), MeshMaterial3d(source.clone()))).id())
            .collect();
        app.update();
        let after_first = entities.iter().filter(|&&e| blends_on(&app, e)).count();
        assert_eq!(after_first, SWAP_WORK_PER_FRAME, "first frame swaps exactly SWAP_WORK_PER_FRAME shapes");
        assert!(app.world().resource::<Variants>().last.as_ref().unwrap().pending, "remaining swaps keep the pass pending");
        app.update();
        let after_second = entities.iter().filter(|&&e| blends_on(&app, e)).count();
        assert_eq!(after_second, total, "second frame finishes the remaining swaps");
        assert!(!app.world().resource::<Variants>().last.as_ref().unwrap().pending, "pass settles once every shape is swapped");
    }
}
