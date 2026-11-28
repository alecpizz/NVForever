//! NifAssetPlugin: spawn a NIF's scene graph as Bevy entities.
//!
//! Conventions [observed, docs/spec-notes/nif.md §7]: rotation matrices are stored row by
//! row and rotate column vectors; a child's world transform is parent ∘ child; triangles
//! are counter-clockwise when seen from the side their normals face; a bone's skin
//! transform is inverse(bone bind world) ∘ shape bind world, which is exactly the
//! inverse bind pose Bevy expects. Gamebryo is Z-up, Bevy Y-up: the root entity rotates
//! by −90° about X.

use crate::data::GameData;
use crate::nif_material::{display_emissive, emissive_in_display, nif_material, Falloff, NifMaterial};
use crate::tangents::{StoredArrays, Tangents};
use crate::texture::TextureCache;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::DynamicSkinnedMeshBounds;
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::math::Affine3A;
use bevy::prelude::*;
use fnv_nif::scene::{mesh_data, scene_objects, Decoded, SceneObject, Transform as NifTransform};
use fnv_nif::{NifFile, Struct, Value};
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

/// Marks the root of a spawned NIF.
#[derive(Component)]
pub struct NifRoot;

/// Set on objects whose NiAVObject flags have bit 0 set [hypothesis: "hidden"; editor
/// markers and collision helpers carry it].
#[derive(Component)]
pub struct NifHidden;

/// Summary of what was spawned, for the overlay and for framing the camera.
#[derive(Debug, Clone, Default)]
pub struct SpawnReport {
    pub blocks: usize,
    pub shapes: usize,
    pub skinned: usize,
    pub triangles: usize,
    pub textures_missing: Vec<String>,
    /// Bounds in Bevy world space (after the Z-up → Y-up rotation).
    pub bounds: Option<(Vec3, Vec3)>,
    /// Node name → entity, for animation targets.
    pub nodes: HashMap<String, Entity>,
    /// Local transform of each named node as spawned (its bind pose), so a node an
    /// animation took over can be put back.
    pub bind: HashMap<Entity, Transform>,
    pub root: Option<Entity>,
    /// Skeleton file the skinned shapes were bound to.
    pub skeleton: Option<String>,
}

#[derive(SystemParam)]
pub struct SpawnCtx<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub materials: ResMut<'w, Assets<NifMaterial>>,
    pub images: ResMut<'w, Assets<Image>>,
    pub bindposes: ResMut<'w, Assets<SkinnedMeshInverseBindposes>>,
    pub textures: ResMut<'w, TextureCache>,
    pub data: Res<'w, GameData>,
    pub formats: Option<Res<'w, CompressedImageFormatSupport>>,
}

pub fn to_bevy(t: &NifTransform) -> Transform {
    let m = Mat3::from_cols_array(&t.rotation).transpose(); // stored row-major
    Transform { translation: Vec3::from_array(t.translation), rotation: Quat::from_mat3(&m).normalize(), scale: Vec3::splat(t.scale) }
}

pub(crate) fn affine(t: &NifTransform) -> Affine3A {
    let m = Mat3::from_cols_array(&t.rotation).transpose();
    Affine3A::from_mat3_translation(m * t.scale, Vec3::from_array(t.translation))
}

impl Props {
    /// Name the first-person form of each texture that has one
    /// (`crate::texture::first_person_texture`); the number named anew.
    pub(crate) fn use_first_person_textures(&mut self, exists: impl Fn(&str) -> bool) -> usize {
        let mut n = 0;
        for t in self.textures.iter_mut().chain(self.glow.as_mut()) {
            if let Some(first) = crate::texture::first_person_texture(t, &exists) {
                *t = first;
                n += 1;
            }
        }
        n
    }

    /// How many of the shape's textures (glow map included) are in their first-person form.
    pub(crate) fn first_person_texture_count(&self) -> usize {
        self.textures.iter().chain(self.glow.as_ref()).filter(|t| crate::texture::is_first_person_texture(t)).count()
    }

    /// Put in a texture set's files (a record's alternate textures, `fnv_world::alt_textures`;
    /// docs/spec-notes/texture-swaps.md §3), given in NIF texture-set slot order as the
    /// TXST stores them: each given slot replaces the shape's own, an empty one keeps it
    /// [hypothesis]. The glow map follows slot 2 unless the shape uses slot 2 for skin or
    /// hair, as when the file names it (`glow_map`). The number of slots replaced.
    pub(crate) fn use_texture_set(&mut self, set: &[Option<String>; 6]) -> usize {
        let mut n = 0;
        for (slot, t) in set.iter().enumerate() {
            let Some(t) = t.as_deref().filter(|t| !t.is_empty()) else { continue };
            if self.textures.len() <= slot {
                self.textures.resize(slot + 1, String::new());
            }
            self.textures[slot] = fnv_vfs::texture_path(t);
            if slot == 2 && !self.hair && !self.skin && !self.unlit {
                self.glow = Some(self.textures[2].clone());
            }
            n += 1;
        }
        n
    }
}

/// Rendering-relevant properties accumulated from an object and its ancestors.
#[derive(Debug, Clone, Default)]
pub(crate) struct Props {
    pub(crate) textures: Vec<String>,
    alpha: Option<(i64, i64)>,
    pub(crate) material_alpha: Option<f32>,
    emissive: Option<[f32; 3]>,
    emissive_mult: Option<f32>,
    /// Glow map: texture-set slot 2 of a BSShaderPPLightingProperty without the face-gen
    /// or hair flag (docs/spec-notes/nif.md §6, "Emissive light").
    glow: Option<String>,
    /// BSShaderFlags bit 11 "Parallax_Shader_Index_15" of a BSShaderPPLightingProperty
    /// [documented: nif.xml]: texture-set slot 3 is then drawn as the shape's height map
    /// ([`game_height_slot`], #2979).
    parallax: bool,
    glossiness: Option<f32>,
    /// BSShaderFlags bit 0 "Specular" of the shader property [documented: nif.xml].
    specular: bool,
    /// BSShaderFlags bit 3 "Vertex_Alpha" [documented: nif.xml]: vertex colour alpha is
    /// used only with it (`fix_vertex_colors`).
    vertex_alpha: bool,
    /// NiStencilProperty draw mode DRAW_BOTH; also set on dismemberment caps (actor.rs).
    pub(crate) double_sided: bool,
    /// NiStencilProperty draw mode (`0..=3`: `DRAW_CCW_OR_BOTH` / `DRAW_CCW`, `DRAW_CW`,
    /// `DRAW_BOTH`) [documented: nif.xml StencilDrawMode], mapped through
    /// [`stencil_draw_mode_cull`] so load-time and live `SetStencilPropertyValue` culling match.
    stencil_draw_mode: Option<i64>,
    vertex_colors_tint: bool,
    /// BSShaderNoLightingProperty: drawn without lighting (effects, glows).
    unlit: bool,
    /// The texture came from a BSShaderNoLightingProperty's file name: an
    /// NiTexturingProperty after it keeps it.
    shader_texture: bool,
    /// BSShaderFlags bit 19 "Dynamic_Alpha" [documented: nif.xml] on a
    /// BSShaderNoLightingProperty ([`shape_alpha_mode`]).
    dynamic_alpha: bool,
    /// BSShaderFlags bit 18 "Hair" [documented: nif.xml].
    pub(crate) hair: bool,
    /// The shader property's type is [`SHADER_SKIN`]: the shape is an actor's skin.
    pub(crate) skin: bool,
    /// The view-angle falloff of a BSShaderNoLightingProperty that switches it on
    /// ([`property_falloff`]).
    falloff: Option<Falloff>,
    /// BSShaderFlags bit 29 "External_Emittance" [documented: nif.xml].
    external_emittance: bool,
    /// A decal shader flag is set ([`is_decal`]).
    decal: bool,
    /// The Refraction shader flag is set ([`refracts`]): the stored refraction strength.
    refraction: Option<f32>,
    /// The Fire Refraction shader flag is set too ([`fire_period`]): seconds for the
    /// normal map to scroll once, else 0.
    fire_period: f32,
    /// BSShaderFlags2 bit 0 "ZBuffer_Write" [documented: nif.xml]: the shape asks to write
    /// depth although it is blended. Read for the solid cores only ([`solid_core_rules`]);
    /// depth writes otherwise follow the alpha mode, as before.
    z_write: bool,
    /// BSShaderFlags2 bit 2 "LOD_Building" [documented: nif.xml]: object LOD block
    /// geometry, cut out by its atlas's alpha (`shape_alpha_mode`).
    lod_building: bool,
    /// A sequence of the model drives the shape's emissive colour or multiplier
    /// (`crate::material_anim`): its glow map is loaded even when the stored colour is
    /// black, as the radios' dials store black and light up by their "Open" sequence
    /// (docs/spec-notes/activators.md §2).
    pub(crate) animated_emissive: bool,
    /// A clip of the model drives the shape's material alpha ([`shape_alpha_mode`]).
    pub(crate) animated_alpha: bool,
    /// The environment-map inputs of a lit shader property that sets Environment_Mapping
    /// ([`env_flags`]): the shape reflects its texture set's environment cube
    /// ([`shape_env`]).
    env: Option<crate::env_map::EnvFlags>,
    /// An actor's part whose material carries an NiAlphaController for the actor's
    /// clips to drive ([`shape_alpha_mode`], [`material_alpha_controlled`]).
    pub(crate) alpha_gated: bool,
    /// BSShaderFlags bit 3 "Vertex_Alpha" itself, without Dynamic_Alpha's part in
    /// [`uses_vertex_alpha`], on an unlit shape without a NiAlphaProperty, some stored
    /// vertex alpha of which is below 1 ([`fades_by_vertex_alpha`]).
    vertex_faded: bool,
    /// The caller keeps the shape off authored sets (#2981): particle systems, object LOD
    /// (`crate::authored::shape_eligible`).
    pub(crate) no_authored: bool,
    /// Some property is attached (to the shape or an ancestor).
    has_properties: bool,
    /// One of them derives from BSShaderProperty.
    shader: bool,
    /// The shape is a shadow patch baked into its model ([`baked_shadow_patch`]).
    pub(crate) shadow_patch: bool,
}

impl Props {
    pub(crate) fn external_emittance(&self) -> bool {
        self.external_emittance
    }

    /// The stored emissive colour (black when none) and multiplier (1 when none), held
    /// for unlit pictures by [`shape_emissive_mult`] [hypothesis, nif.md §6
    /// "Unlit shapes that are pictures, and cut-out signs"].
    pub(crate) fn emissive_parts(&self) -> ([f32; 3], f32) {
        (self.emissive.unwrap_or([0.0; 3]), self.emissive_mult.unwrap_or(1.0).max(0.0))
    }

    /// The stored refraction strength (0 when the shape does not refract).
    pub(crate) fn refraction_part(&self) -> f32 {
        self.refraction.unwrap_or(0.0)
    }

    /// The stored material alpha (1 when none).
    pub(crate) fn alpha_part(&self) -> f32 {
        self.material_alpha.unwrap_or(1.0)
    }

    /// Helper geometry the game does not draw: neither a shader property nor a texture,
    /// and either some other property attached or none at all in a model that shades
    /// other shapes (`shaded_model`, from [`shaded_model`]).
    ///
    /// With properties: bone display boxes (`TTail1:0`, `Bone01:0`), collision proxies
    /// (`c_Minigun`, `DummyCol:3`) and invisible markers (`##PLRInvis:0`): 434 of 48,996
    /// visible shapes in `Fallout - Meshes.bsa`, all with only a NiMaterialProperty
    /// [observed: `crates/fnv-nif/examples/shape_survey.rs`; hypothesis: the game skips
    /// them, consistent with the in-game look of the Goodsprings flag].
    ///
    /// Without any property: the furniture-marker figures, capsules and boundaries
    /// copied into the Tops lounge chair (`furniture\loungechair_tops.nif`, the only
    /// such model outside landscape LOD in the base and DLC archives), and the untextured
    /// water-height shape of the landscape LOD chunks, which `lod.rs` never drew
    /// [observed: `crates/fnv-nif/examples/marker_shape_survey.rs`; documented: GECK
    /// wiki "Bethsoft Tutorial NPC population": a furniture marker's "white markers and
    /// blue characters will not show up in-game"; hypothesis: the game draws none of
    /// them; docs/spec-notes/nif.md §6 "Shapes with no property at all"]. A model with
    /// no shader property anywhere keeps its bare shapes [design: no model in the
    /// archives has bare shapes and no shader property; a rule that could blank a whole
    /// model is not wanted].
    pub(crate) fn is_helper(&self, shaded_model: bool) -> bool {
        !self.shader && self.textures.is_empty() && (self.has_properties || shaded_model)
    }

    /// No property at all on the shape or an ancestor.
    pub(crate) fn is_bare(&self) -> bool {
        !self.has_properties
    }

    /// Drawn without lighting (a BSShaderNoLightingProperty): effects, glows.
    pub(crate) fn unlit_part(&self) -> bool {
        self.unlit
    }

    /// The glow map, when the shape has one.
    pub(crate) fn glow_part(&self) -> Option<&str> {
        self.glow.as_deref()
    }

    /// The shape takes its vertex alpha ([`uses_vertex_alpha`]).
    pub(crate) fn vertex_alpha_part(&self) -> bool {
        self.vertex_alpha
    }
}

/// Some block of the model derives from BSShaderProperty, so its shapes without any
/// property are editor geometry ([`Props::is_helper`]).
pub(crate) fn shaded_model(file: &NifFile, dec: &Decoded) -> bool {
    (0..file.blocks.len()).any(|i| dec.is_a(i, "BSShaderProperty"))
}

/// BSShaderType 14 "SHADER_SKIN" [documented: nif.xml BSShaderType]. In
/// `Fallout - Meshes.bsa` the 255 visible shapes of this type are exactly those with the
/// "FaceGen" shader flag (bit 10), and all but 2 (which name no texture) name a head,
/// upper body or hand texture under `textures\characters\` [observed:
/// `crates/fnv-nif/examples/skin_survey.rs`, docs/spec-notes/actors.md §3.1].
pub(crate) const SHADER_SKIN: i64 = 14;

pub(crate) fn text_list(s: &Struct, name: &str) -> Vec<String> {
    s.list(name)
        .unwrap_or_default()
        .iter()
        .map(|v| match v {
            Value::Str { text: Some(t), .. } => t.clone(),
            _ => String::new(),
        })
        .collect()
}

/// The glow map of a BSShaderPPLightingProperty: texture-set slot 2, unless the face-gen
/// (bit 10) or hair (bit 18) shader flag is set [documented: nif.xml BSShaderFlags,
/// BSShaderTextureSet], in which case slot 2 holds skin or hair maps.
fn glow_map(shader_flags: i64, textures: &[String]) -> Option<String> {
    if shader_flags & (1 << 10 | 1 << 18) != 0 {
        return None;
    }
    textures.get(2).filter(|t| !t.is_empty()).cloned()
}

/// Whether a BSShaderPPLightingProperty's flags ask for parallax: BSShaderFlags bit 11
/// "Parallax_Shader_Index_15" [documented: nif.xml BSShaderFlags]. Bit 28
/// "Parallax_Occlusion" is not asked: no shape of the base game or its DLC sets it, and all
/// 510 shapes with a slot-3 texture set bit 11 [observed: the per-shape survey of upstream
/// issue #2979, counts only; docs/spec-notes/nif.md §6 "Parallax height maps"].
pub(crate) fn parallax_flag(shader_flags: i64) -> bool {
    shader_flags & (1 << 11) != 0
}

/// The game's own height map of a shape (#2979): texture-set slot 3 ("height"
/// [documented: nif.xml BSShaderTextureSet]) of a BSShaderPPLightingProperty with the
/// parallax flag ([`parallax_flag`]); `None` without the flag or with an empty slot. Read
/// from the slots as they stand, so a record's texture set (`Props::use_texture_set`, TXST
/// TX04 into slot 3) and the first-person forms replace it as they replace the others.
pub(crate) fn game_height_slot(props: &Props) -> Option<&str> {
    props.textures.get(3).filter(|t| props.parallax && !props.unlit && !t.is_empty()).map(String::as_str)
}

/// The shape is a decal: BSShaderFlags bit 26 "Decal_Single_Pass" or bit 27
/// "Dynamic_Decal_Single_Pass" [documented: nif.xml BSShaderFlags]. 2,130 visible shapes
/// in `Fallout - Meshes.bsa` set one (41 only bit 26, none only bit 27): dirt, stains,
/// cracks, markings and insignia laid 0.05 to 1 unit in front of another shape of their
/// file, a few exactly in its plane [observed: `crates/fnv-nif/examples/decal_survey.rs`,
/// docs/spec-notes/nif.md §6 "Decal flags"]. They are drawn with a depth offset towards
/// the camera (`nif_material::decal_bias`) [hypothesis].
pub(crate) fn is_decal(shader_flags: i64) -> bool {
    shader_flags & (1 << 26 | 1 << 27) != 0
}

/// The shape bends what lies behind it instead of being drawn: BSShaderFlags bit 15
/// "Refraction" [documented: nif.xml BSShaderFlags; docs/spec-notes/explosion-effects.md
/// §7]. 31 of the 257 models the weapon effect records name set it, all on a
/// BSShaderPPLightingProperty (the heat-haze shells of explosions), as do Mister Handy's
/// jet clouds, whose diffuse texture is plain white (`FXWHITE.dds`) [observed]; drawn as
/// ordinary shapes they show as solid grey or white domes [observed: the Incinerator's
/// `dlc03flamerexplosion.nif`, owner 2026-10-20].
pub(crate) fn refracts(shader_flags: i64) -> bool {
    shader_flags & (1 << 15) != 0
}

/// Seconds for a refracting shape's normal map to scroll once up its texture: with
/// BSShaderFlags bit 16 "Fire_Refraction" [documented: nif.xml BSShaderFlags] the
/// BSShaderPPLightingProperty's "Refraction Fire Period" [documented: nif.xml; the GECK
/// wiki's NIF controller page names `BSRefractionFirePeriodController` as animating it],
/// read as tenths of a second, a negative period scrolling down instead of up
/// [hypothesis, docs/spec-notes/refraction.md §3]; 0 without the flag or with a period
/// of 0.
pub(crate) fn fire_period(shader_flags: i64, period: Option<i64>) -> f32 {
    match period {
        Some(p) if p != 0 && shader_flags & (1 << 16) != 0 => p as f32 * FIRE_PERIOD_UNIT,
        _ => 0.0,
    }
}

/// Seconds per unit of the stored fire period [hypothesis, docs/spec-notes/refraction.md §3].
pub const FIRE_PERIOD_UNIT: f32 = 0.1;

/// BSShaderFlags bit 6 switches the view-angle falloff of a BSShaderNoLightingProperty on
/// [hypothesis, docs/spec-notes/nif.md §6 "View-angle falloff". nif.xml names the bit
/// "Empty" for this game and "Use_Falloff" ("Use Falloff value in EffectShaderProperty")
/// in the Skyrim and Fallout 4 flag sets, which keep this game's names at bits 0, 1, 3, 7,
/// 15 to 17, 29 and 31 [documented: nif.xml SkyrimShaderPropertyFlags1,
/// Fallout4ShaderPropertyFlags1]. In `Fallout - Meshes.bsa` 1,013 of the 1,193 visible
/// shapes with falloff values of their own set it and 95 of the 4,495 with the nif.xml
/// defaults; the Prospector Saloon lamp beam sets it [observed:
/// `crates/fnv-nif/examples/falloff_survey.rs`]. No game frame of a shape without the
/// bit was compared].
pub(crate) fn uses_falloff(shader_flags: i64) -> bool {
    shader_flags & (1 << 6) != 0
}

/// Whether shader flags ask for a window's environment map: BSShaderFlags bit 7
/// "Environment_Mapping" and bit 21 "Window_Environment_Mapping" [documented: nif.xml flag
/// names] (docs/spec-notes/nif.md §6 "Window glass and environment maps"). The glass of
/// windows, bottles and lamps (490 files in `Fallout - Meshes.bsa`, gap audit §2.1). Kept
/// apart from [`env_flags`]: the GECK wiki says the window flag only flips the cube's
/// orientation in this game [documented: GECK wiki "NIF Shader Properties"], so the two
/// sets can be drawn with different orientations (`env_map::EnvAids::axes_for`), and
/// window shapes keep #2669's blend rule.
pub(crate) fn window_env_flags(shader_flags: i64) -> bool {
    shader_flags & (1 << 7) != 0 && shader_flags & (1 << 21) != 0
}

/// Whether shader flags ask for an environment map: BSShaderFlags bit 7
/// "Environment_Mapping" [documented: nif.xml flag names; GECK wiki "NIF Shader
/// Properties": "Enables environment mapping on the object"], with or without the window
/// flag ([`window_env_flags`]); docs/spec-notes/nif.md §6 "Environment maps on other
/// shapes" (#2841).
pub(crate) fn env_flags(shader_flags: i64) -> bool {
    shader_flags & (1 << 7) != 0
}

/// A shape's environment map ([`env_flags`]): the environment cube (texture set slot 4),
/// the environment mask (slot 5) if the set has one, the scale (`environment_map_scale`)
/// and the window and Env Map Fade flags [documented: nif.xml BSShaderTextureSet slot
/// order, BSShaderProperty field; GECK wiki "NIF Shader Properties": "Texture 5:
/// Environment Map (Cubemap)", "Texture 6: Environment Mask"]. `None` without the flag,
/// without a cube, on an unlit shape, at a scale of 0 or less, and, for a shape without
/// the window flag, on object LOD block geometry (LOD_Building: drawn only beyond the
/// loaded cells, farther than the game's end of reflections, `env_map::ENV_MAP_LOD2`
/// [design]) and while `NVF_PLAY_DEBUG_ENV_NON_WINDOW=0` (env_map.rs). Window shapes are
/// gated as #2462 gated them, LOD block geometry included. A cube that does not load as a
/// cube map leaves the shape as it was (`shape_material_inner`).
pub(crate) fn shape_env(props: &Props, aids: &crate::env_map::EnvAids) -> Option<crate::env_map::ShapeEnv> {
    let f = props.env.filter(|f| f.scale > 0.0 && !props.unlit)?;
    if !f.window && (!aids.non_window || props.lod_building) {
        return None;
    }
    let slot = |i: usize| props.textures.get(i).filter(|t| !t.is_empty()).cloned();
    Some(crate::env_map::ShapeEnv { cube: slot(4)?, mask: slot(5), scale: f.scale, window: f.window, light_fade: f.light_fade })
}

/// Whether a shader property's shapes take their vertex alpha: with BSShaderFlags bit 3
/// "Vertex_Alpha", and on an unlit shape (BSShaderNoLightingProperty) also with bit 19
/// "Dynamic_Alpha" [documented: nif.xml flag names; hypothesis for the second: the
/// dust vortex sheets of `effects\nv\sanddust\sanddust02.nif` set bit 19, not bit 3,
/// and their vertex alpha is 0 along the sheets' edges and 1 inside; without it they drew
/// as a dense sheet with hard vertical edges where the game shows a soft column of dust
/// (owner, playtest list 5 item 1; docs/spec-notes/nif.md §6 "Vertex colours")].
pub(crate) fn uses_vertex_alpha(property_type: &str, shader_flags: i64) -> bool {
    shader_flags & (1 << 3) != 0 || (property_type == "BSShaderNoLightingProperty" && shader_flags & (1 << 19) != 0)
}

/// The view-angle falloff of a BSShaderNoLightingProperty with these shader flags and
/// falloff fields (start angle, stop angle, start opacity, stop opacity; absent in files
/// of stream version 26 or lower): none without the flag ([`uses_falloff`]), whatever
/// the fields hold. Without the flag the fields are mostly the defaults, opacity 1 → 0
/// over cosine 1 → 0, which applied would fade every such shape by the cosine of the
/// view angle: neon tubes, lit sign letters, the shadows under playing cards.
pub(crate) fn property_falloff(shader_flags: i64, fields: [Option<f32>; 4]) -> Option<Falloff> {
    let [Some(a), Some(b), Some(c), Some(d)] = fields else { return None };
    uses_falloff(shader_flags).then(|| Falloff::from_fields(a, b, c, d)).flatten()
}

fn apply_property(file: &NifFile, dec: &Decoded, r: i32, p: &mut Props) {
    let Some(s) = dec.get(r) else { return };
    let ty = file.blocks[r as usize].type_name.as_str();
    p.has_properties = true;
    let shader = dec.is_a(r as usize, "BSShaderProperty");
    p.shader |= shader;
    if let Some(ts) = s.block_ref("texture_set").and_then(|t| dec.get(t)) {
        p.textures = text_list(ts, "textures");
    }
    if shader || ty == "NiTexturingProperty" {
        p.glow = None;
    }
    if shader {
        let flags = s.int("shader_flags").unwrap_or(0);
        p.specular = flags & 1 != 0;
        p.vertex_alpha = uses_vertex_alpha(ty, flags);
        // The flag's part; `shape_props` asks the rest of the shape.
        p.vertex_faded = flags & (1 << 3) != 0;
        p.dynamic_alpha = ty == "BSShaderNoLightingProperty" && flags & (1 << 19) != 0;
        p.hair = flags & (1 << 18) != 0;
        p.skin = s.int("shader_type") == Some(SHADER_SKIN);
        p.external_emittance = flags & (1 << 29) != 0;
        p.decal = is_decal(flags);
        p.refraction = refracts(flags).then(|| s.float("refraction_strength").unwrap_or(0.0));
        p.fire_period = fire_period(flags, s.int("refraction_fire_period"));
        p.z_write = s.int("shader_flags_2").unwrap_or(0) & 1 != 0;
        p.lod_building = s.int("shader_flags_2").unwrap_or(0) & (1 << 2) != 0;
        p.falloff = None;
        p.parallax = false;
        let lit = dec.is_a(r as usize, "BSShaderPPLightingProperty");
        p.env = (lit && env_flags(flags)).then(|| crate::env_map::EnvFlags {
            scale: s.float("environment_map_scale").unwrap_or(1.0),
            window: window_env_flags(flags),
            light_fade: s.int("shader_flags_2").unwrap_or(0) & (1 << 15) != 0,
        });
    }
    match ty {
        "BSShaderPPLightingProperty" => {
            let flags = s.int("shader_flags").unwrap_or(0);
            p.glow = glow_map(flags, &p.textures);
            p.parallax = parallax_flag(flags);
        }
        "BSShaderNoLightingProperty" => {
            p.unlit = true;
            if let Some(f) = s.text("file_name") {
                p.textures = vec![f.to_string()];
                p.shader_texture = !f.is_empty();
            }
            let fields = ["falloff_start_angle", "falloff_stop_angle", "falloff_start_opacity", "falloff_stop_opacity"];
            p.falloff = property_falloff(s.int("shader_flags").unwrap_or(0), fields.map(|n| s.float(n)));
        }
        // The unlit shader's own texture wins over the base texture of an NiTexturingProperty
        // listed after it, which then only adds its texture transform [hypothesis: in
        // 3,783 shapes of Fallout - Meshes.bsa and Update.bsa holding both, two differ:
        // the jukebox's light tubes, whose shader names `JBLightTubesClean.dds` (bright
        // cyan in the owner's frames of 2026-10-27) and texturing `jblighttubesdirty.dds`,
        // and a REPCONN mist layer].
        "NiTexturingProperty" if p.shader_texture => {}
        "NiTexturingProperty" => {
            let src = s.strukt("base_texture").and_then(|b| b.block_ref("source")).and_then(|r| dec.get(r));
            if let Some(f) = src.and_then(|t| t.text("file_name")) {
                p.textures = vec![f.to_string()];
            }
        }
        "NiAlphaProperty" => p.alpha = Some((s.int("flags").unwrap_or(0), s.int("threshold").unwrap_or(128))),
        "NiMaterialProperty" => {
            p.material_alpha = s.float("alpha");
            p.emissive = s.floats("emissive_color").and_then(|e| e.try_into().ok());
            p.emissive_mult = s.float("emissive_mult");
            p.glossiness = s.float("glossiness");
        }
        "NiStencilProperty" => {
            // 20.1.0.3+: draw mode in flags bits 10-11; older files: draw_mode field.
            let mode = s.int("draw_mode").or_else(|| s.int("flags").map(|f| (f >> 10) & 3)).unwrap_or(0);
            let (double_sided, _) = stencil_draw_mode_cull(mode);
            p.double_sided = double_sided;
            p.stencil_draw_mode = Some(mode);
        }
        "NiVertexColorProperty" => p.vertex_colors_tint = true,
        _ => {}
    }
}

/// Editor-only geometry: nodes named `EditorMarker...` [documented: nif.xml BSXFlags bit
/// 5 "EditorMarkers present"; observed: 327 meshes in Fallout - Meshes.bsa, 155 of them
/// effects, mostly without the hidden flag; hypothesis: the game never draws them].
pub fn is_editor_marker(name: Option<&str>) -> bool {
    name.is_some_and(|n| n.len() >= 12 && n[..12].eq_ignore_ascii_case("editormarker"))
}

/// Blend mode for NiAlphaProperty flags [documented: nif.xml AlphaFlags: bit 0 blend,
/// bits 1-4 source and 5-8 destination factor (AlphaFunction: 0 ONE, 1 ZERO, 2 SRC_COLOR,
/// 4 DEST_COLOR, 6 SRC_ALPHA, 7 INV_SRC_ALPHA, ...), bit 9 alpha test]. Bevy offers fixed
/// modes, so factors map to the nearest: destination ONE is additive, a source of ZERO
/// or DEST_COLOR multiplies.
///
/// When both test and blend are set, a threshold of [`BLEND_TEST_CUT`] or more draws
/// alpha-tested (the test cuts the shape: weapon parts), a lower one blended (the test only
/// trims nearly transparent texels: dirty glass) [hypothesis, from
/// `crates/fnv-nif/examples/alpha_survey.rs`, docs/spec-notes/nif.md §6 "Alpha"].
/// Additive stays additive. Tested first, the dirty glass of the NV poster marquees
/// (`clutter/signnv/nvposter01.nif`, flags 0x12ED, threshold 0) hid the poster behind
/// an opaque grey panel [observed].
///
/// The test compares alpha with the threshold by bits 10-12 (TestFunction: 4 GREATER, 6
/// GREATER_EQUAL, ...) [documented: nif.xml AlphaFlags]. Bevy's mask keeps alpha ≥ cutoff,
/// so GREATER moves the cutoff half a step above the threshold; other functions keep ≥.
pub(crate) const BLEND_TEST_CUT: i64 = 64;

pub fn alpha_mode(flags: i64, threshold: i64) -> AlphaMode {
    let (src, dst) = ((flags >> 1) & 0xF, (flags >> 5) & 0xF);
    let (blend, test) = (flags & 1 != 0, flags & 0x200 != 0);
    let strict = (flags >> 10) & 7 == 4;
    let cutoff = (threshold as f32 + if strict { 0.5 } else { 0.0 }) / 255.0;
    match (blend, test, src, dst) {
        (true, _, _, 0) => AlphaMode::Add,
        (false, true, _, _) => AlphaMode::Mask(cutoff),
        (true, true, _, _) if threshold >= BLEND_TEST_CUT => AlphaMode::Mask(cutoff),
        (true, _, 1 | 4, _) => AlphaMode::Multiply,
        (true, ..) => AlphaMode::Blend,
        _ => AlphaMode::Opaque,
    }
}

/// Double-sided flag and face culling from a `NiStencilProperty` `StencilDrawMode`
/// (`0/1` `DRAW_CCW_OR_BOTH`/`DRAW_CCW` -> back-face cull, `2` `DRAW_CW` -> front-face cull,
/// `3` `DRAW_BOTH` -> double-sided / no cull) [documented: nif.xml StencilDrawMode;
/// design: shared by load-time `NiStencilProperty` material creation and live
/// `SetStencilPropertyValue` updates so both paths produce identical culling].
pub fn stencil_draw_mode_cull(draw_mode: i64) -> (bool, Option<bevy::render::render_resource::Face>) {
    match draw_mode & 3 {
        3 => (true, None),
        2 => (false, Some(bevy::render::render_resource::Face::Front)),
        _ => (false, Some(bevy::render::render_resource::Face::Back)),
    }
}

/// Double-sided flag and face culling from `NiStencilProperty` flags bits 10-11
/// (`draw_mode`: 0/1 `DRAW_CCW`, 2 `DRAW_CW`, 3 `DRAW_BOTH`) [documented: nif.xml
/// StencilDrawMode; design: Bevy's `StandardMaterial` does not expose per-material
/// hardware stencil reference/mask state, so only the face-cull mode applies live].
pub fn stencil_cull(flags: i64) -> (bool, Option<bevy::render::render_resource::Face>) {
    stencil_draw_mode_cull((flags >> 10) & 3)
}

fn props_cull(props: &Props) -> (bool, Option<bevy::render::render_resource::Face>) {
    if props.double_sided {
        stencil_draw_mode_cull(3)
    } else {
        stencil_draw_mode_cull(props.stencil_draw_mode.unwrap_or(0))
    }
}

/// Whether a shape is additive with source factor ONE (NiAlphaProperty blend on, source
/// and destination blend modes 0, ONE [documented: nif.xml AlphaFlags]): 182 shapes in
/// `Fallout - Meshes.bsa` (docs/spec-notes/nif.md §6 "Additive blend"), among them the
/// Securitron's screen noise (effects-animation-audit.md §2).
pub(crate) fn adds_whole(props: &Props) -> bool {
    props.alpha.is_some_and(|(flags, _)| flags & 1 != 0 && (flags >> 1) & 0xF == 0 && (flags >> 5) & 0xF == 0)
}

/// Alpha cutoff for object LOD geometry: alpha above 128 is drawn [hypothesis: the game's
/// value is not known; 128 with TEST_GREATER is the most common setting of the
/// alpha-tested `*_lod.nif` meshes, 45 of 128 shapes, and `watertower01_lod.nif` has 127;
/// observed: `crates/fnv-nif/examples/lod_alpha_survey.rs`].
pub(crate) const LOD_BUILDING_CUT: f32 = 128.5 / 255.0;

/// How a shape's alpha is drawn: by its NiAlphaProperty when it has one. Without one,
/// shapes whose shader sets "LOD_Building" are alpha-tested at [`LOD_BUILDING_CUT`]
/// [hypothesis, docs/spec-notes/world.md §7 "Objects": all 355 object LOD block shapes
/// in `Fallout - Meshes.bsa` set the flag (no other shape does) and none has a
/// NiAlphaProperty, yet their atlases carry cut-outs in alpha (trees, pylons, the water
/// tower's legs: 14% of `WastelandNV.Buildings.dds` is alpha 0, near black in colour);
/// drawn opaque those were black boxes on the hills around Goodsprings]. An unlit shape
/// whose shader sets "Dynamic_Alpha" (bit 19) and whose alpha a clip of its model drives
/// is blended by its alpha [hypothesis,
/// docs/spec-notes/explosion-effects.md §9: of the 257 weapon effect models only two
/// shapes have no NiAlphaProperty and an unlit shader with the flag, the Fat Man
/// explosion's mushroom-cloud card `SmokeGeo:0` (`explosionlarge01.nif`) and `WhiteBG:0`
/// of `explosionincendiarygrenade.nif`, and both are faded in and out by alpha tracks
/// from a stored alpha of 0; drawn opaque the mushroom stood whole for 5 s (owner,
/// 2026-10-20). In the base mesh archives 29 more shapes have such a shader and no
/// NiAlphaProperty (Lucky 38's light glows, terminal screens), whose alpha no clip fades;
/// they stay as they were checked, opaque]. Such a shape of an actor's part whose
/// material carries an NiAlphaController, the target of the actor's own `.kf` clips
/// ([`material_alpha_controlled`]), is drawn opaque where its material alpha is 1/2 or
/// more and not at all below [observed: in the actor folders of all the archives, 9
/// such unlit shapes, all Mr. House's three stacked screens in the three
/// `nvpenthouemaincomputer` screen files, each with an alpha controller and an opaque
/// texture; `crates/fnv-nif/examples/actor_alpha_survey.rs`. They lie a few units apart,
/// Yes Man's face in front, House's behind it, Connection Lost at the back, and the
/// clips set their alpha to 0 or 1: `mtidle.kf` shows Connection Lost alone,
/// `DynamicIdle_MrHouse.kf` House alone, `DynamicIdle_YesMan.kf` Yes Man over Connection
/// Lost. Drawn opaque whatever the alpha, Yes Man's face covered House's before Yes Man
/// was uploaded (#1542). Hypothesis: the original skips geometry at material alpha 0 and
/// draws the rest by depth, as the clips' alphas of 0 and 1 suggest; blending is not
/// needed for them]. An unlit shape whose only picture is its vertex alpha ([`fades_by_vertex_alpha`]:
/// the cone of light at the end of Lonesome Road's silo lift, #3443) is blended by it.
/// Everything else without the property is opaque.
pub(crate) fn shape_alpha_mode(props: &Props) -> AlphaMode {
    match props.alpha {
        Some((flags, threshold)) => alpha_mode(flags, threshold),
        None if props.lod_building => AlphaMode::Mask(LOD_BUILDING_CUT),
        None if props.unlit && props.dynamic_alpha && props.animated_alpha => AlphaMode::Blend,
        None if props.unlit && props.dynamic_alpha && props.alpha_gated => AlphaMode::Mask(0.5),
        None if fades_by_vertex_alpha(props) => AlphaMode::Blend,
        None => AlphaMode::Opaque,
    }
}

/// Whether a shape is an unlit one whose only picture is its vertex alpha: a
/// BSShaderNoLightingProperty that names no texture and sets bit 3 "Vertex_Alpha"
/// (Dynamic_Alpha alone, which also takes the vertex alpha, [`uses_vertex_alpha`], is not
/// asked for: no shape without texture and NiAlphaProperty has it alone), no
/// NiAlphaProperty, and some stored vertex alpha below 1. It is blended SRC_ALPHA /
/// INV_SRC_ALPHA ([`shape_alpha_mode`]) [hypothesis: no frame of the original was compared,
/// observer check 286; docs/spec-notes/nif.md §6 "Unlit shapes faded by vertex alpha
/// alone"]. 21 shapes of the base game's and the add-ons' archives are such:
/// dark patches under gurneys and at tunnel mouths, fog shells, two small orange shapes of
/// static collections, the silo lift's cone. 1,507 others have the same shader, no texture
/// and an alpha below 1 and do store the property, 1,458 of them with this blend. The cone,
/// `Object03:0` of `nvdlc04_silo-elevator-shaft06.nif`, is stored a second time in the
/// unsplit `nvdlc04_silo-elevator-shaft.nif`: the same vertices, colours, strips, material
/// and shader, and a NiAlphaProperty 0x10ED [observed, decoded from the archives]. Drawn
/// opaque it was a flat white cone (player report R-F8KPPE34, #3443).
pub(crate) fn fades_by_vertex_alpha(props: &Props) -> bool {
    props.alpha.is_none() && props.unlit && props.vertex_faded && props.textures.iter().all(|t| t.is_empty())
}

/// Whether a shape's stored vertex colours hold an alpha below 1 (cleaned as they are
/// drawn, [`clean_vertex_color`]).
fn vertex_alpha_below_one(dec: &Decoded, shape: usize) -> bool {
    let data = dec.blocks.get(shape).and_then(|b| b.as_ref().ok()).and_then(|s| s.block_ref("data")).and_then(|r| dec.get(r));
    let colours = data.and_then(|d| d.floats("vertex_colors"));
    colours.is_some_and(|f| f.as_chunks::<4>().0.iter().any(|c| clean_vertex_color(*c, true)[3] < 1.0))
}

/// Whether the NiMaterialProperty that shape block `shape` lists carries an
/// NiAlphaController in its controller chain: the material alpha a `.kf` clip can drive
/// (a sequence's controlled block names the shape and the property type, and drives the
/// controller of that type on it [documented: nif.xml ControlledBlock "Node Name",
/// "Property Type", "Controller Type"]).
pub(crate) fn material_alpha_controlled(file: &NifFile, dec: &Decoded, shape: usize) -> bool {
    let Some(s) = dec.blocks.get(shape).and_then(|b| b.as_ref().ok()) else { return false };
    s.refs("properties").unwrap_or_default().iter().any(|&r| {
        if !file.blocks.get(r as usize).is_some_and(|p| p.type_name == "NiMaterialProperty") {
            return false;
        }
        let mut c = dec.get(r).and_then(|m| m.block_ref("controller"));
        for _ in 0..64 {
            let Some(cr) = c.filter(|&x| x >= 0) else { return false };
            if file.blocks.get(cr as usize).is_some_and(|b| b.type_name == "NiAlphaController") {
                return true;
            }
            c = dec.get(cr).and_then(|b| b.block_ref("next_controller"));
        }
        false
    })
}

/// The cut of a shape that is drawn blended although its alpha test would make it an
/// alpha-tested one ([`alpha_mode`]: blend and test both set, threshold of
/// [`BLEND_TEST_CUT`] or more), or `None` for every other shape: its cutoff, when its
/// shader uses vertex alpha and some vertex's alpha is below 1. Such a shape's alpha is a
/// gradient across its triangles, not the outline of a texture: drawn alpha-tested, the
/// part above the cutoff would be opaque where the shape asks for a part of its colour.
/// So it blends, and the fragments below the cutoff are dropped, which is the reading of
/// "blend and test" itself [documented: nif.xml AlphaFlags; hypothesis that the game
/// tests the vertex alpha: docs/spec-notes/nif.md §6 "Alpha", "Vertex alpha and the
/// test"]. `mesh` must have been through [`fix_vertex_colors`].
pub(crate) fn blend_cut(props: &Props, mesh: &Mesh) -> Option<f32> {
    let (flags, threshold) = props.alpha?;
    if flags & 1 == 0 || !props.vertex_alpha {
        return None;
    }
    let AlphaMode::Mask(cutoff) = alpha_mode(flags, threshold) else { return None };
    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) else { return None };
    colors.iter().any(|c| c[3] < 1.0).then_some(cutoff)
}

/// Properties of a shape: its ancestors' first, then its own.
pub(crate) fn shape_props(file: &NifFile, dec: &Decoded, o: &SceneObject, by_block: &HashMap<usize, &SceneObject>) -> Props {
    let mut chain = vec![o.block];
    let mut cur = o.parent;
    while let Some(p) = cur {
        if chain.len() > 512 {
            break;
        }
        chain.push(p);
        cur = by_block.get(&p).and_then(|x| x.parent);
    }
    let mut props = Props::default();
    for b in chain.iter().rev() {
        if let Some(refs) = dec.blocks[*b].as_ref().ok().and_then(|s| s.refs("properties")) {
            for &r in refs {
                apply_property(file, dec, r, &mut props);
            }
        }
    }
    props.vertex_faded = props.vertex_faded && props.unlit && props.alpha.is_none() && vertex_alpha_below_one(dec, o.block);
    props.shadow_patch = baked_shadow_patch(&props, dec, o.block);
    // A shadow patch takes its vertex alpha whatever the flag ([`baked_shadow_patch`]).
    props.vertex_alpha |= props.shadow_patch;
    props
}

/// NiAlphaProperty flags that blend SRC_ALPHA / INV_SRC_ALPHA [documented: nif.xml
/// AlphaFlags: bit 0 blending, bits 1-4 the source factor, bits 5-8 the destination's;
/// 6 SRC_ALPHA, 7 INV_SRC_ALPHA].
pub(crate) fn blends_over(flags: i64) -> bool {
    flags & 1 != 0 && (flags >> 1) & 15 == 6 && (flags >> 5) & 15 == 7
}

/// Whether these vertex colours (as stored) are a shadow's: every one black (each channel
/// at most [`SHADOW_BLACK`] once cleaned), some alpha below 1 and some above 0.
pub(crate) fn shadow_colours(colours: &[[f32; 4]]) -> bool {
    let clean: Vec<[f32; 4]> = colours.iter().map(|c| clean_vertex_color(*c, true)).collect();
    !clean.is_empty()
        && clean.iter().all(|c| c[0] <= SHADOW_BLACK && c[1] <= SHADOW_BLACK && c[2] <= SHADOW_BLACK)
        && clean.iter().any(|c| c[3] < 1.0)
        && clean.iter().any(|c| c[3] > 0.0)
}

/// The largest colour channel a shadow patch's vertex may hold [engineering choice: the
/// patches store exact black].
pub(crate) const SHADOW_BLACK: f32 = 0.02;

/// Whether a shape is a shadow patch baked into its model: the dark polygon the game's
/// furniture carries on the floor under it. An unlit shape (BSShaderNoLightingProperty)
/// that names no texture, blends SRC_ALPHA / INV_SRC_ALPHA ([`blends_over`]) and whose
/// vertex colours are black with an alpha that varies ([`shadow_colours`]) [observed:
/// `CardTable01:1` of `meshes\clutter\casino\cardtable01.nif`, decoded: 20 vertices on
/// the floor plane under the table, colour 0 0 0, alpha 0 along the outline and 0.7
/// inside, NiAlphaProperty 0x10ED, shader flags 0x82000000].
///
/// Such a shape takes its vertex alpha whether or not its shader sets "Vertex_Alpha"
/// [hypothesis, playtest 2026-11-25: the card table's does not set it, and with the alpha
/// taken as 1 ([`clean_vertex_color`]) the patch was an opaque black octagon with a hard
/// outline; its stored alpha is the picture of a shadow, nothing at its edge and 70% in
/// the middle, which only shows if the alpha is drawn. No frame of the original was
/// compared; docs/spec-notes/nif.md §6 "Vertex colours"]. It does not fade where it meets
/// the surface behind it either (`nif_material::soft_intersections`): it lies on that
/// surface, and the fade's test of that (the two facings from screen derivatives) failed
/// from a few metres away, so the patch went from half gone to whole as the camera moved
/// (the player's video of that table).
///
/// 1,314 shapes of the base game's and the add-ons' archives are such [observed 2026-11-25,
/// `crates/fnv-nif/examples/baked_shadow_survey.rs`]: the patches under furniture (alpha
/// 0.4 to 0.8), and 196 that are black in full somewhere, the dark of recessed ceiling
/// lights, tunnel mouths, doorways and wall holes.
pub(crate) fn baked_shadow_patch(props: &Props, dec: &Decoded, shape: usize) -> bool {
    if !props.unlit || !props.textures.iter().all(|t| t.is_empty()) || !props.alpha.is_some_and(|(flags, _)| blends_over(flags)) {
        return false;
    }
    let data = dec.blocks.get(shape).and_then(|b| b.as_ref().ok()).and_then(|s| s.block_ref("data")).and_then(|r| dec.get(r));
    data.and_then(|d| d.floats("vertex_colors")).is_some_and(|c| shadow_colours(c.as_chunks::<4>().0))
}

/// Perceptual roughness for a NiMaterialProperty glossiness, read as a Blinn-Phong
/// exponent n: Beckmann slope m = sqrt(2 / (n + 2)) [documented: Walter et al. 2007,
/// "Microfacet Models for Refraction through Rough Surfaces"], used as the GGX alpha,
/// and Bevy's perceptual roughness is sqrt(alpha) [hypothesis: the game's highlight
/// size follows glossiness as a Phong exponent].
pub fn gloss_roughness(gloss: f32) -> f32 {
    (2.0 / (gloss.max(0.0) + 2.0)).sqrt().sqrt()
}

/// Clean a shape's vertex colours: non-finite values become 1 and the rest are clamped
/// to 0..1, and the alpha is 1 unless the shader property sets the vertex-alpha flag
/// [hypothesis, docs/spec-notes/nif.md §6 "Vertex colours": of 3,032 visible shapes with
/// some vertex alpha below 1, 2,445 set the flag, while 331 of the 343 shapes with
/// non-finite or out-of-range values do not (Victor's shack trim: NaN, −3e38 and denormal
/// alphas on 709 of 1,173 vertices)].
pub(crate) fn fix_vertex_colors(mesh: &mut Mesh, vertex_alpha: bool) {
    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) else { return };
    for c in colors.iter_mut() {
        *c = clean_vertex_color(*c, vertex_alpha);
    }
}

/// One vertex colour cleaned as [`fix_vertex_colors`] cleans them.
pub(crate) fn clean_vertex_color(c: [f32; 4], vertex_alpha: bool) -> [f32; 4] {
    let clean = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
    [clean(c[0]), clean(c[1]), clean(c[2]), if vertex_alpha { clean(c[3]) } else { 1.0 }]
}

/// The vertex colours of an unlit shape (BSShaderNoLightingProperty) taken as display
/// values: the mesh keeps them as stored, and the material marks them so
/// (`NifSurface::emissive_rule` z = 1); the shader blends them across the triangles as
/// display values and makes the colour linear per fragment (sRGB transfer,
/// [`display_to_linear`]), the alpha kept.
///
/// Per fragment, not at the vertices (#2435): the game blends the stored values across
/// a triangle [hypothesis: a DirectX 9 frame without sRGB conversion, as in nif.md §6
/// "Additive blend"]. Made linear at the vertices, a colour falling to black across a
/// triangle stayed bright almost to its black edge (display 0.096 at nine tenths of the
/// way from 0.34 to 0, where the stored values give 0.034): the Gomorrah Suites' wall
/// sconce glow (`neoclassicalsconce.nif`, `NeoclassicalSconce:9`, ONE / ONE, no texture,
/// 0.34 at the bowl to black on its outline) was a pale board with a hard outline.
///
/// An unlit shape's colour is texture × vertex colour, which the game forms in display
/// values [hypothesis, as for the landscape's vertex colours (terrain.rs) and the grass
/// (grass.rs)]; this renderer multiplies in linear values, so a stored 0.57 grey drew as
/// a display 0.78. The smoke meshes are tinted so: the ground smoke at the camp fires
/// (`dlcpitt/effects/dlcpittfxgroundsmoke01.nif`, vertex colour (0.57, 0.54, 0.51), white
/// texture) is a grey near the desert floor's in the game, where it can barely be seen
/// (owner's screenshots, 2026-10-12), and drew as a white plume here
/// (docs/spec-notes/nif.md §6 "Vertex colours").
pub(crate) fn unlit_vertex_colors(material: &mut NifMaterial) {
    material.extension.emissive_rule.z = 1.0;
}

/// A display value made linear by the sRGB transfer (CPU mirror of nif_material.wesl
/// `display_to_linear`, which [`unlit_vertex_colors`] has the shader apply per fragment).
pub fn display_to_linear(v: f32) -> f32 {
    Color::srgb(v, 0.0, 0.0).to_linear().red
}

/// Reflectance for a shape, and whether its normal map's alpha (the specular mask,
/// docs/spec-notes/nif.md §6 "Specular") scales it. Bevy maps mask alpha a to
/// reflectance × a × 0.5, so 2.0 lets the mask span the full [0, 1] range: a mask of
/// 0.5 gives the usual dielectric F0 of 0.04 [hypothesis]. Shaders without the specular
/// flag get none; shapes without a normal map keep the earlier constant. Hair-shader
/// shapes (hair, brows, beards) get none either [hypothesis, nif.md §6 "Hair": read as a
/// plain mask their normal-map alpha gave glinting white strands the game does not show].
///
/// A normal map without an alpha channel (DXT1: `unmasked`) holds no mask; read as one,
/// its alpha of 1 is the strongest mask there is, seven times the median authored one,
/// and made Doc Mitchell's bed, the Goodsprings gas station's vending machine and door
/// shine like wet paint (owner's playtest, 2026-10-11). Such a shape gets
/// [`UNMASKED_SPECULAR`] instead [hypothesis, nif.md §6 "Specular"].
///
/// Under Physical lighting (graphics-enhanced.md §3.25) a normal-mapped shape without the
/// flag takes a faint reflectance of 0.32 (F0 about 0.016) in place of none, with its
/// roughness held at 0.86 or more (`shape_material_inner`) [design].
pub(crate) fn specular(props: &Props, has_normal_map: bool, unmasked: bool) -> (f32, bool) {
    specular_with(props, has_normal_map, unmasked, crate::authored::physical_pbr())
}

/// [`specular`] for a run with or without Physical lighting.
fn specular_with(props: &Props, has_normal_map: bool, unmasked: bool, physical: bool) -> (f32, bool) {
    if props.hair {
        return (0.0, false);
    }
    match (has_normal_map, props.specular) {
        (true, true) if unmasked => (UNMASKED_SPECULAR, false),
        (true, true) => (2.0, true),
        (true, false) if physical => (0.32, false),
        (true, false) => (0.0, false),
        (false, _) => (0.3, false),
    }
}

/// The reflectance of a shape whose normal map has no alpha channel: the median mask of
/// the normal maps that have one, 35 of 255, as the masked path reads it (reflectance
/// 2.0 × 0.5 × mask) [observed: `crates/fnv-nif/examples/normal_mask_survey.rs` over
/// `Fallout - Textures.bsa` and `Textures2.bsa`, 2,998 masked normal maps, quartiles of
/// their mean alpha 21 / 35 / 58; the choice of the median is a hypothesis].
pub const UNMASKED_SPECULAR: f32 = 35.0 / 255.0;

/// Whether an unexpanded loaded format has no alpha channel to hold a specular mask.
pub fn lacks_alpha(image: &Image) -> bool {
    format_lacks_alpha(image.texture_descriptor.format)
}

/// [`lacks_alpha`] by the texture's format: the one- and two-channel formats (BC4, BC5,
/// and their CPU fallbacks R8 and RG8) and BC1.
pub fn format_lacks_alpha(format: bevy::render::render_resource::TextureFormat) -> bool {
    use bevy::render::render_resource::TextureFormat;
    matches!(
        format,
        TextureFormat::Bc1RgbaUnorm
            | TextureFormat::Bc1RgbaUnormSrgb
            | TextureFormat::Bc4RUnorm
            | TextureFormat::Bc4RSnorm
            | TextureFormat::Bc5RgUnorm
            | TextureFormat::Bc5RgSnorm
            | TextureFormat::R8Unorm
            | TextureFormat::R8Snorm
            | TextureFormat::Rg8Unorm
            | TextureFormat::Rg8Snorm
    )
}

/// The falloff to draw a shape's material with. Falloff scales alpha, so only blended
/// materials use it [hypothesis: on alpha-tested or opaque shapes it would only cut holes;
/// 15 of the 1,108 visible shapes in `Fallout - Meshes.bsa` that switch the falloff on
/// with a range to fade over are not blended; observed:
/// `crates/fnv-nif/examples/falloff_survey.rs`, docs/spec-notes/nif.md §6].
pub(crate) fn shape_falloff(props: &Props, alpha_mode: AlphaMode) -> Option<Falloff> {
    props.falloff.filter(|_| matches!(alpha_mode, AlphaMode::Blend | AlphaMode::Add | AlphaMode::Multiply | AlphaMode::Premultiplied))
}

/// Whether a shape's emitted light is over white: emissive colour × multiplier over 1 in
/// its brightest channel (neon, glowing signs; [`crate::nif_material::soft_intersections`]).
pub(crate) fn burns(props: &Props) -> bool {
    let (e, m) = props.emissive_parts();
    e.iter().fold(0.0f32, |a, &c| a.max(c)) * m > 1.0
}

/// An opaque unlit surface without External Emittance is a picture, including House's
/// authored opaque screen drawn with an actor alpha gate (`shape_alpha_mode`): its colour
/// times its multiplier is held to 1, so a Securitron's face (grey 0.05 x 30) and House's
/// screen show their texture and no more. Blended and additive effects and authored
/// cut-outs keep their authored multipliers, and a material track's keys multiply the
/// stored multiplier, as authored (`Props::emissive_parts`) [hypothesis, fitted to the
/// original's frames of 2026-11-06; nif.md section 6 "Unlit shapes that are pictures, and
/// cut-out signs"].
pub(crate) fn shape_emissive_mult(props: &Props, alpha_mode: AlphaMode) -> f32 {
    let mult = props.emissive_mult.unwrap_or(1.0).max(0.0);
    let opaque = matches!(alpha_mode, AlphaMode::Opaque)
        || (props.alpha.is_none() && props.alpha_gated && !props.lod_building && matches!(alpha_mode, AlphaMode::Mask(_)));
    if props.unlit && !props.external_emittance && opaque {
        let top = props.emissive.map_or(1.0, |e| e.iter().copied().fold(0.0f32, f32::max));
        mult.min(1.0 / top.max(f32::EPSILON))
    } else {
        mult
    }
}

/// Unlit shapes without External Emittance skip the image-space scale (x). An authored
/// cut-out sign skips it too, and takes the plain path of blended shapes rather than
/// `hot_light` (y), so its texture keeps its letters and it stays under the bright pass's
/// threshold [hypothesis, fitted to the same frames and nif.md's section].
pub(crate) fn shape_emissive_rule(props: &Props, alpha_mode: AlphaMode) -> Vec4 {
    if !props.unlit {
        return Vec4::ZERO;
    }
    let cutout = props.alpha.is_some() && matches!(alpha_mode, AlphaMode::Mask(_));
    Vec4::new(if !props.external_emittance || cutout { 1.0 } else { 0.0 }, if cutout { 1.0 } else { 0.0 }, 0.0, 0.0)
}

/// Emitted light of a shape: emissive colour × emissive multiplier (× the glow map and the
/// surface colour in the shader) [hypothesis, docs/spec-notes/nif.md §6 "Emissive light"],
/// `None` when the colour is black. An unlit shape's value is [`unlit_emissive`]. A
/// multiply-blended or additive shape's light is part of what it does to the frame behind
/// it, which is formed in display values, so its brightness is raised to the display gamma
/// ([`display_emissive`], nif.md §6 "Multiply blend", "Additive blend").
pub(crate) fn shape_emissive(props: &Props, alpha_mode: AlphaMode) -> Option<LinearRgba> {
    let e = props.emissive.filter(|e| e.iter().any(|&c| c > 0.0))?;
    let m = shape_emissive_mult(props, alpha_mode);
    if props.unlit && !matches!(alpha_mode, AlphaMode::Multiply) {
        let mut tint = unlit_emissive(e, m);
        if emissive_in_display(alpha_mode) {
            // The hue is already a display colour raised to the gamma; the brightness
            // (max(1, m x brightest channel)) is raised too.
            let top = e.iter().copied().fold(0.0f32, f32::max);
            let scale = (m * top).max(1.0);
            tint = tint.map(|c| c * scale.powf(crate::nif_material::MULTIPLY_GAMMA - 1.0));
        }
        let [r, g, b] = tint;
        return Some(LinearRgba::rgb(r, g, b));
    }
    let value = |c: f32| if emissive_in_display(alpha_mode) { display_emissive(c * m) } else { c * m };
    Some(LinearRgba::rgb(value(e[0]), value(e[1]), value(e[2])))
}

/// The factor on an unlit shape's surface colour from its emissive colour `e` and
/// multiplier `m` (nif.md §6 "Emissive colour of unlit shapes"): the colour's hue (the
/// colour over its brightest channel, a display colour, so raised to the display gamma)
/// times max(1, m × brightest channel). A warm or cyan colour tints a white glow texture;
/// a grey colour changes nothing below 1 (Lucky 38's base lights store grey 0.1 × 1.25
/// and are bright in the original); neon at white × 25 burns 25 times its texture
/// [hypothesis from the original's frames].
pub fn unlit_emissive(e: [f32; 3], m: f32) -> [f32; 3] {
    let top = e.iter().copied().fold(0.0f32, f32::max);
    if top <= 0.0 {
        return [1.0; 3];
    }
    let scale = (m * top).max(1.0);
    e.map(|c| (c.max(0.0) / top).powf(crate::nif_material::MULTIPLY_GAMMA) * scale)
}

/// The largest side of a mesh's bounding box in its own units (0 without positions).
fn mesh_extent(mesh: &Mesh) -> f32 {
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(ps)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return 0.0 };
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in ps {
        let p = Vec3::from_array(*p);
        lo = lo.min(p);
        hi = hi.max(p);
    }
    if ps.is_empty() { 0.0 } else { (hi - lo).max_element() }
}

/// Where a shape's material takes its textures from: the texture cache and image store on
/// the main thread ([`StoreTextures`]), or a worker thread's handles reserved for images it
/// decodes (`model::Deferred`).
pub(crate) trait TextureSource {
    /// The texture at a NIF texture path, loaded once.
    fn texture(&mut self, raw_path: &str, srgb: bool) -> Option<Handle<Image>>;
    /// A normal map and its source alpha classification (BC5 expansion must not
    /// turn its synthetic storage alpha into a specular mask).
    fn normal(&mut self, raw_path: &str) -> Option<(Handle<Image>, bool)> {
        let h = self.texture(raw_path, false)?;
        let unmasked = self.lacks_alpha(&h);
        Some((h, unmasked))
    }
    /// Whether the texture of `h` has no alpha channel ([`lacks_alpha`]).
    fn lacks_alpha(&self, h: &Handle<Image>) -> bool;
    /// Whether the texture at a NIF texture path has a cut-out mask in its alpha
    /// (`texture::AlphaShares::is_mask`), read from the archives once per file
    /// (`texture::WorkerTextures::alpha_is_mask`): asked only for the shapes
    /// [`blended_cutout`] concerns.
    fn alpha_is_mask(&self, raw_path: &str) -> bool;
    /// Whether every texel of the texture at a NIF texture path is opaque
    /// (`texture::alpha_all_opaque`), read from the archives once per file: asked only
    /// for the shapes [`blend_is_opaque`] concerns.
    fn alpha_is_opaque(&self, raw_path: &str) -> bool;
    /// The key a NIF texture path loads under (`texture::resolved_path` over this source's
    /// VFS): the diffuse's key is what an authored set is looked up by.
    fn resolve(&self, raw_path: &str) -> String;
    /// The set (#2981: authored, or height only) for a resolved diffuse key, its maps loaded by
    /// the exact paths the index names (`crate::authored::find`); `None` when the run's setting
    /// is off (then nothing is asked of the index), with no active set, when an authored set's
    /// `_orm` does not load, or when a height-only set gives no height map. Every production
    /// source implements it (no default, so none can leave the feature off by omission); test
    /// doubles answer for themselves.
    fn pbr_set(&mut self, resolved_diffuse: &str) -> Option<crate::authored::PbrSet>;
    /// Whether authored sets are in use at all (the run's setting, `crate::authored::enabled`,
    /// for every production source): off, no shape is looked up and no legacy look is kept,
    /// so an off run's materials carry nothing new. No default, as for [`Self::pbr_set`].
    fn authored_on(&self) -> bool;
}

/// The main thread's textures: the cache and the image store.
pub(crate) struct StoreTextures<'a> {
    pub textures: &'a mut TextureCache,
    pub data: &'a GameData,
    pub images: &'a mut Assets<Image>,
    pub formats: Option<&'a CompressedImageFormatSupport>,
}

impl TextureSource for StoreTextures<'_> {
    fn texture(&mut self, raw_path: &str, srgb: bool) -> Option<Handle<Image>> {
        self.textures.get(raw_path, srgb, self.data, self.images, self.formats)
    }

    fn normal(&mut self, raw_path: &str) -> Option<(Handle<Image>, bool)> {
        self.textures.normal(raw_path, self.data, self.images, self.formats)
    }

    fn lacks_alpha(&self, h: &Handle<Image>) -> bool {
        self.images.get(h).is_some_and(lacks_alpha)
    }

    fn alpha_is_mask(&self, raw_path: &str) -> bool {
        self.textures.workers.alpha_is_mask(raw_path, self.data)
    }
    fn alpha_is_opaque(&self, raw_path: &str) -> bool {
        self.textures.workers.alpha_is_opaque(raw_path, self.data)
    }

    fn resolve(&self, raw_path: &str) -> String {
        crate::texture::resolved_path(raw_path, |p| self.data.vfs.exists(p))
    }

    fn authored_on(&self) -> bool {
        crate::authored::enabled()
    }

    fn pbr_set(&mut self, resolved_diffuse: &str) -> Option<crate::authored::PbrSet> {
        let workers = self.textures.workers.clone();
        let (textures, data, images, formats) = (&mut *self.textures, self.data, &mut *self.images, self.formats);
        crate::authored::find(data, crate::authored::enabled(), resolved_diffuse, &workers.authored, |path, usage| {
            let (h, _) = textures.exact(path, usage, data, images, formats)?;
            let format = images.get(&h).map(|i| i.texture_descriptor.format);
            Some((h, format))
        })
    }
}

/// The alpha cut of a blended cut-out (`NifSurface::blended_cutout`) [engineering choice:
/// half way; the masks it applies to have under 13% of their texels between clear and
/// opaque, and the game's single corn plants cut the same texture at 124 of 255].
pub(crate) const BLENDED_CUTOUT_CUT: f32 = 0.5;

/// Whether a shape drawn blended draws the same as a cut-out, and its cut if so: it blends
/// SRC_ALPHA / INV_SRC_ALPHA and tests alpha (below [`BLEND_TEST_CUT`], or it would be
/// drawn alpha-tested already), and nothing but its base texture sets its alpha, which is
/// a cut-out mask (`mask`, asked last): no vertex-alpha cut, material alpha 1, no alpha
/// animation, view-angle falloff, light, decal, refraction or hair shading. Where alpha is
/// 0 or 1, blending and testing give the same pixels; they differ only on the mask's edge
/// texels. The game's corn rows (`scol\scolmaizerow1b.nif`, flags 0x12ED, threshold 0,
/// `maize_d.dds`) are such shapes [observed]; dirty glass (`nvposter01.nif`, the same
/// flags, `marquee.dds` 40% in between) is not. Survey: docs/design/fsr-upscaling.md
/// "Blended cut-outs". The test function (bits 10-12) is not asked: such a shape is drawn blended
/// whatever its function ([`alpha_mode`] below the cut), so the cut-out draws the pixels the
/// blend does, as FSR's switch needs.
pub(crate) fn blended_cutout(props: &Props, cut: Option<f32>, alpha_mode: AlphaMode, mask: impl FnOnce() -> bool) -> Option<f32> {
    let (flags, threshold) = props.alpha?;
    let source_over = (flags >> 1) & 0xF == 6 && (flags >> 5) & 0xF == 7;
    let tested = flags & 0x200 != 0 && threshold < BLEND_TEST_CUT;
    let alpha_from_texture_only = cut.is_none()
        && props.material_alpha.unwrap_or(1.0) >= 1.0
        && !props.animated_alpha
        && props.falloff.is_none()
        && !props.unlit
        && !props.decal
        && props.refraction.is_none()
        && !props.hair;
    (matches!(alpha_mode, AlphaMode::Blend) && source_over && tested && alpha_from_texture_only && mask()).then_some(BLENDED_CUTOUT_CUT)
}

/// Whether a shape that asks to be blended draws the same as an opaque one: it blends
/// SRC_ALPHA / INV_SRC_ALPHA and nothing gives it an alpha below 1. Its base texture is
/// opaque in every texel (`opaque_texture`, asked last), its material alpha is 1, it has no
/// vertex alpha, no vertex-alpha cut, no alpha animation, view-angle falloff, light,
/// decal, refraction, hair shading or window environment map. With alpha 1 the blend is `colour x 1 + behind x 0`:
/// the shape's own colour, as if opaque.
///
/// Drawn opaque it is in the depth and motion prepasses, which a blended shape is not
/// (docs/design/fsr-upscaling.md "Blended cut-outs"). Drawn blended, the placed T-45d
/// Power Armor (`armor\powerarmor\powerarmorgo.nif`: one NiTriStrips, NiAlphaProperty
/// flags 4333 = blend on, test off, threshold 0, material alpha 1, no vertex colours,
/// `PowerArmorBody.dds` in DXT1) showed as a faint see-through outline on a table with
/// DLSS, frame generation and the room's haze on, which all take a blended shape for
/// something to see through [observed: player report R-C66YZC4D, #3230; fnvtool nif
/// decode]. In a plain scene it drew solid either way [observed: a placed ArmorPowerT45d
/// on the Searchlight Airport runway, 2026-11-21].
pub(crate) fn blend_is_opaque(props: &Props, cut: Option<f32>, alpha_mode: AlphaMode, opaque_texture: impl FnOnce() -> bool) -> bool {
    let Some((flags, _)) = props.alpha else { return false };
    let source_over = (flags >> 1) & 0xF == 6 && (flags >> 5) & 0xF == 7;
    let nothing_thins_it = cut.is_none()
        && !props.vertex_alpha
        && props.material_alpha.unwrap_or(1.0) >= 1.0
        && !props.animated_alpha
        && props.falloff.is_none()
        && !props.unlit
        && !props.decal
        && props.refraction.is_none()
        && !props.hair
        // A shape with a window environment map (`shape_env` with the window flag) keeps
        // its blend: blended, its reflection is added over it (`premultiply` below), and
        // that path is left as it is [design, #3230]. (The drinking glass, the glass pitcher and the NCR
        // glasses are not such shapes: they have the plain environment-map flag only,
        // material alpha 1 and an opaque texture, so the rule takes them too; they drew
        // at alpha 1 before as well [observed: fnvtool nif decode of
        // `clutter\cafeteria\drinkingglass01.nif`; the smoke runs' summary lines].)
        && !props.env.is_some_and(|e| e.window);
    matches!(alpha_mode, AlphaMode::Blend) && source_over && nothing_thins_it && opaque_texture()
}

/// One shape of a survey of [`blend_is_opaque`] over whole archives
/// ([`blend_opaque_survey`]; the example `blend_opaque_survey`, #3230).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlendSurveyShape {
    /// The shape's name in its file (`"?"` when it has none).
    pub shape: String,
    /// Its base texture path as the file stores it.
    pub texture: String,
    /// The rule draws it opaque.
    pub drawn_opaque: bool,
    /// It tests alpha as well (NiAlphaProperty bit 9).
    pub tested: bool,
    /// Everything holds but the window environment map, which keeps the blend.
    pub only_window_env: bool,
}

/// The shapes of one model that are drawn blended by their NiAlphaProperty, with whether
/// [`blend_is_opaque`] draws each opaque, `opaque` telling whether a texture path is opaque
/// in every texel. The rule's own test, not a copy: a survey that counts what the engine
/// does (#3230 asked for every world model).
pub fn blend_opaque_survey(bytes: &[u8], schema: &fnv_nif::Schema, opaque: impl Fn(&str) -> bool) -> Vec<BlendSurveyShape> {
    let Ok(file) = NifFile::parse(bytes, Some(schema)) else { return Vec::new() };
    let dec = Decoded::new(&file, bytes, schema);
    let objects = fnv_nif::scene::scene_objects(&file, &dec);
    let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
    let mut out = Vec::new();
    for o in objects.iter().filter(|o| !o.is_node) {
        let props = shape_props(&file, &dec, o, &by_block);
        let Some((flags, _)) = props.alpha else { continue };
        let mode = shape_alpha_mode(&props);
        if !matches!(mode, AlphaMode::Blend) {
            continue;
        }
        let texture = props.textures.first().cloned().unwrap_or_default();
        let tex_opaque = || !texture.is_empty() && opaque(&texture);
        // A vertex-alpha cut needs the mesh; the rule refuses vertex alpha before it.
        let drawn_opaque = blend_is_opaque(&props, None, mode, tex_opaque);
        let only_window_env = !drawn_opaque && props.env.is_some_and(|e| e.window) && {
            let mut p = props.clone();
            p.env = None;
            blend_is_opaque(&p, None, mode, tex_opaque)
        };
        out.push(BlendSurveyShape { shape: o.name.clone().unwrap_or_else(|| "?".into()), texture, drawn_opaque, tested: flags & 0x200 != 0, only_window_env });
    }
    out
}

/// The alpha from which a blended shape's texels belong to its solid core ([`solid_core`])
/// [engineering choice: at 0.85 and above the blend shows at most 15% of what lies behind, so
/// the texel is a surface to everything that reads depth. At 0.98, the first choice, the
/// border texels of `NV_WaterTank.dds` (226 to 244 of 255 [hypothesis: read from the texture on 2026-11-24;
/// the steps were not kept]) were left out, and the light of what is behind drew a bright line along
/// the tank's rim, judged by eye in a play test, 2026-11-24].
pub const SOLID_CORE_CUT: f32 = 0.85;
/// The cut where the texture has thin parts in that range too, or its alpha cannot be read
/// (`texture::SolidTexels`): what is solid beyond doubt.
pub const SOLID_CORE_CUT_THIN: f32 = 0.98;

/// Whether a blended shape may have a solid core ([`solid_core`]) by what its file says, the
/// cheap part of the question (the base texture's alpha is asked by nvforever `solid_cores.rs`,
/// and only while cores are wanted): it blends SRC_ALPHA / INV_SRC_ALPHA, is lit, its shader
/// writes depth ("ZBuffer_Write"), it has a base texture, and nothing but that texture sets
/// its alpha: material alpha 1, no vertex alpha, no vertex-alpha cut, no alpha animation, no
/// view-angle falloff, and it is no decal, hair, skin, refracting shape, window reflection or
/// object LOD block [engineering choice of the enhanced look, docs/design/volumetric-medium.md
/// "Solid cores"; the flags' names documented: nif.xml]. A shape with a test below
/// [`BLEND_TEST_CUT`] (the corn rows, [`blended_cutout`]) is included: it is drawn blended
/// where no temporal upscaler runs. The Goodsprings water tank
/// (`architecture\goodsprings\nv_watertank.nif`, flags 4333: blend on, no test) is such a
/// shape (painted with the light of what is behind it: that document says how it was seen).
pub(crate) fn solid_core_rules(props: &Props, cut: Option<f32>, alpha_mode: AlphaMode) -> bool {
    let Some((flags, _)) = props.alpha else { return false };
    let source_over = (flags >> 1) & 0xF == 6 && (flags >> 5) & 0xF == 7;
    matches!(alpha_mode, AlphaMode::Blend)
        && flags & 1 != 0
        && source_over
        && props.z_write
        && props.textures.first().is_some_and(|t| !t.is_empty())
        && cut.is_none()
        && !props.vertex_alpha
        && props.material_alpha.unwrap_or(1.0) >= 1.0
        && !props.animated_alpha
        && !props.alpha_gated
        && props.falloff.is_none()
        && !props.unlit
        && !props.decal
        && props.refraction.is_none()
        && !props.hair
        && !props.skin
        && !props.lod_building
        && !props.env.is_some_and(|e| e.window)
}

/// The solid core of a blended material as it stands now: a copy drawn as a cut-out at
/// [`SOLID_CORE_CUT`], so its solid texels are in the depth prepass, or `None` where it has
/// none. The blended shape goes on being drawn over it: at the core's own depth the main
/// pass's test (greater or equal) lets it through (as a hair model's layer over its core,
/// [`hair_model_layer`]), so the soft edges stay the blend's. The colour is not exactly what
/// it was: the part of the background the blend let through (up to 1 - `cut`) is
/// now the core's colour, which may also take ambient occlusion the blend does not.
///
/// Everything that can change a material while the game runs is asked of the material
/// itself, not of who changed it: a fade, a script's alpha, a stealth field, an effect
/// shader's holes, a swapped texture or the FSR cut-outs' alpha test all leave a state this
/// refuses. The core's cut is hard (`NifSurface::hard_edges`): a soft one would widen the
/// solid part past the cut. The caller keeps the core from casting a shadow.
pub fn solid_core(m: &NifMaterial, cut: f32) -> Option<NifMaterial> {
    let (b, s) = (&m.base, &m.extension);
    let source = s.solid_core.as_ref()?;
    let plain = matches!(b.alpha_mode, AlphaMode::Blend)
        && b.base_color.alpha() >= 1.0
        && b.base_color_texture.as_ref().is_some_and(|t| t.id() == source.image)
        && !b.unlit
        && b.depth_bias == 0.0
        && b.depth_map.is_none()
        && b.specular_transmission == 0.0
        && b.diffuse_transmission == 0.0
        && s.alpha_cut <= 0.0
        && s.falloff == Falloff::NONE.params
        && s.holes == crate::nif_material::NO_HOLES
        && s.holes_texture.is_none()
        && s.facegen_offset.is_none()
        && s.soft == Vec4::ZERO
        && s.terrain_repeat == 0.0
        && s.env_how.z == 0.0
        && !(s.refract || s.decal || s.shade_light || s.depth_write || s.first_person || s.add_linear || s.pipboy_screen);
    if !plain {
        return None;
    }
    let mut core = m.clone();
    core.base.alpha_mode = AlphaMode::Mask(cut);
    core.extension.hard_edges = true;
    // The core is no source of anything: no FSR switch, no core of its own, no texture swap.
    core.extension.blended_cutout = None;
    core.extension.solid_core = None;
    core.extension.legacy = None;
    Some(core)
}

/// How many materials [`blend_is_opaque`] turned opaque this run, and the first few
/// textures they were for (the exit's summary line).
static BLEND_DRAWN_OPAQUE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static BLEND_DRAWN_OPAQUE_FIRST: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn note_blend_drawn_opaque(texture: &str) {
    BLEND_DRAWN_OPAQUE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut first) = BLEND_DRAWN_OPAQUE_FIRST.lock() {
        let t = texture.to_ascii_lowercase();
        if first.len() < 8 && !first.contains(&t) {
            first.push(t);
        }
    }
}

/// The exit's line: how many materials that asked to be blended were made opaque because
/// nothing could thin them ([`blend_is_opaque`]), and the first few textures.
pub fn blend_opaque_summary() -> String {
    let n = BLEND_DRAWN_OPAQUE.load(std::sync::atomic::Ordering::Relaxed);
    let first = BLEND_DRAWN_OPAQUE_FIRST.lock().map(|f| f.join(", ")).unwrap_or_default();
    format!("blended shapes drawn opaque: {n} material builds (alpha 1 everywhere: opaque texture, material alpha 1, no vertex alpha){}", if first.is_empty() { String::new() } else { format!("; first textures: {first}") })
}

/// How many materials were blended by their vertex alpha alone this run
/// ([`fades_by_vertex_alpha`]; the exit's summary line).
static VERTEX_ALPHA_BLENDED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The exit's line: how many materials [`fades_by_vertex_alpha`] had blended.
pub fn vertex_alpha_blended_summary() -> String {
    let n = VERTEX_ALPHA_BLENDED.load(std::sync::atomic::Ordering::Relaxed);
    format!("unlit shapes blended by their vertex alpha alone: {n} material builds (no NiAlphaProperty, no texture)")
}

/// The material for a shape's properties; also returns the base texture path when it
/// could not be loaded. Adds tangents to `mesh` when a normal map is used.
pub(crate) fn shape_material(
    props: &Props,
    mesh: &mut Mesh,
    textures: &mut TextureCache,
    data: &GameData,
    images: &mut Assets<Image>,
    formats: Option<&CompressedImageFormatSupport>,
) -> (NifMaterial, Option<String>) {
    shape_material_with(props, mesh, &mut StoreTextures { textures, data, images, formats }, Tangents::Generate)
}

/// [`shape_material`] for a copy of a mesh that went through it before (an actor part's
/// kept CPU mesh, `actor::PartShape::cpu`): tangents it already has are the ones
/// generating them again would give, so they are kept. The copy only serves to make the
/// material; generating them again cost an actor 5 ms in the frame it was drawn
/// [observed 2026-11-07, #1429, Sierra Madre lobby].
pub(crate) fn shape_material_reusing_tangents(
    props: &Props,
    mesh: &mut Mesh,
    textures: &mut TextureCache,
    data: &GameData,
    images: &mut Assets<Image>,
    formats: Option<&CompressedImageFormatSupport>,
) -> (NifMaterial, Option<String>) {
    shape_material_with(props, mesh, &mut StoreTextures { textures, data, images, formats }, Tangents::Keep)
}

/// [`shape_material`] with its textures from `textures` and the normal map's tangents from
/// `tangents`: the static models (`crate::model`) give the ones stored in the shape's data
/// block ([`crate::tangents`]); every other caller generates them (or keeps them,
/// [`shape_material_reusing_tangents`]).
pub(crate) fn shape_material_with(props: &Props, mesh: &mut Mesh, textures: &mut impl TextureSource, tangents: Tangents) -> (NifMaterial, Option<String>) {
    shape_material_inner(props, mesh, textures, tangents, crate::env_map::aids())
}

/// The material build itself; `aids`: the environment maps' debug switches (this run's,
/// `env_map::aids`, except in tests, which pass their own so that a developer's
/// environment cannot change them).
fn shape_material_inner(props: &Props, mesh: &mut Mesh, textures: &mut impl TextureSource, tangents: Tangents, aids: &crate::env_map::EnvAids) -> (NifMaterial, Option<String>) {
    fix_vertex_colors(mesh, props.vertex_alpha);
    let diffuse = props.textures.first().filter(|t| !t.is_empty());
    let base = diffuse.and_then(|t| textures.texture(t, true));
    let normal = props.textures.get(1).filter(|t| !t.is_empty()).and_then(|t| textures.normal(t));
    let missing = if base.is_none() { diffuse.cloned() } else { None };
    let has_uv = mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some();
    let had_tangents = mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_some();
    // A copy of a drawn mesh (`Tangents::Keep`: an actor part's kept CPU mesh, for a texture
    // swap or a record's texture set) only serves to make the material; the actor goes on
    // drawing the original mesh, so tangents made on the copy never reach the draw. A height
    // map (a set's or the game's own) is put on such a material only when the mesh already
    // has them: Bevy marches no parallax on a mesh without tangents (#2979 review).
    let height_mesh_ok = !matches!(tangents, Tangents::Keep) || had_tangents;
    let normal = normal.filter(|_| has_uv && tangents.add_to(mesh));
    let (normal, unmasked) = normal.map_or((None, false), |(h, unmasked)| (Some(h), unmasked));
    let cut = blend_cut(props, mesh);
    let alpha_mode = if cut.is_some() { AlphaMode::Blend } else { shape_alpha_mode(props) };
    if matches!(alpha_mode, AlphaMode::Blend) && fades_by_vertex_alpha(props) {
        VERTEX_ALPHA_BLENDED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    // A blended shape that nothing can thin is drawn opaque (#3230).
    let alpha_mode = if blend_is_opaque(props, cut, alpha_mode, || props.textures.first().is_some_and(|t| !t.is_empty() && textures.alpha_is_opaque(t))) {
        note_blend_drawn_opaque(props.textures.first().map_or("", String::as_str));
        AlphaMode::Opaque
    } else {
        alpha_mode
    };
    // The environment map: the cube, the mask and how the mask is read
    // (docs/spec-notes/nif.md §6 "Window glass and environment maps", "Environment maps
    // on other shapes"; the debug switches in env_map.rs). A blended window shape that
    // reflects is drawn premultiplied, so its reflection is added over the blend whatever
    // its alpha; a blended shape without the window flag keeps its blend, so the
    // reflection is weighted by its alpha (`env_map::env_alpha_mode`).
    let env = shape_env(props, aids).and_then(|e| {
        let cube = textures.texture(&crate::texture::cube_path(&e.cube), true)?;
        let mask = e.mask.as_deref().filter(|_| has_uv).and_then(|m| textures.texture(m, false));
        let mode = crate::env_map::mask_mode(mask.is_some(), aids.no_mask, has_uv && normal.is_some() && !unmasked)?;
        // The normal-alpha variant reads the game's own normal map, bound in the mask's
        // slot: an authored set may replace the drawn normal map later (`crate::authored`).
        let mask = if mode == crate::env_map::MaskMode::NormalAlpha { normal.clone() } else { mask };
        Some((e, cube, mask, mode))
    });
    // Premultiplied only where the set's blend rule asks for it (`env_map::env_alpha_mode`:
    // window shapes by default), and never on a blended cut-out (a tested shape whose
    // texture is a mask): it stays Blend, the mode `blended_cutout` and the FSR cut-out
    // switch (nif_material.rs) know, and its reflection is added inside the blend.
    let premultiply = matches!(crate::env_map::env_alpha_mode(alpha_mode, env.as_ref().map(|(e, ..)| e.window), aids), AlphaMode::Premultiplied)
        && blended_cutout(props, cut, alpha_mode, || props.textures.first().is_some_and(|t| !t.is_empty() && textures.alpha_is_mask(t))).is_none();
    let alpha_mode = if premultiply { AlphaMode::Premultiplied } else { alpha_mode };
    // An authored set (#2981, crate::authored): looked up by the resolved diffuse when the
    // shape may take one; `None` whenever the run's setting is off.
    let on = textures.authored_on();
    let eligible = on && crate::authored::shape_eligible(props.unlit, props.refraction.is_some(), props.hair, props.skin, alpha_mode, has_uv, props.no_authored);
    if on && !eligible {
        crate::authored::note_skipped(false);
    }
    let key = diffuse.filter(|_| eligible).map(|t| textures.resolve(t));
    let key_ok = key.as_deref().is_some_and(crate::authored::key_eligible);
    if key.is_some() && !key_ok {
        crate::authored::note_skipped(true);
    }
    let found = key.filter(|_| key_ok).and_then(|k| textures.pbr_set(&k));
    let authored = found.as_ref().and_then(crate::authored::PbrSet::authored);
    let height = found.as_ref().and_then(crate::authored::PbrSet::height);
    // A height map is drawn only on an opaque (or, under the alpha-tested switch, an
    // alpha-tested), single-sided, non-decal shape (`crate::authored::height_gate`, asked
    // again of the built material): only such a shape gains tangents for one.
    let height_wanted = height.is_some()
        && height_mesh_ok
        && crate::authored::height_alpha(alpha_mode, crate::authored::parallax_mask()).is_ok()
        && !props.double_sided
        && !props.decal;
    // The game's own height map (#2979: texture-set slot 3 with the parallax flag), whatever
    // the Authored materials setting; the same shapes as a set's height map
    // (`crate::authored::height_gate`), and none at a game depth of 0 (the development
    // switch, `crate::authored::game_parallax_depth`). Loaded only for a shape that may draw it.
    let game_slot = game_height_slot(props).filter(|_| crate::authored::game_parallax_depth() > 0.0);
    let game_height_wanted = game_slot.is_some()
        && has_uv
        && height_mesh_ok
        && props.refraction.is_none()
        && crate::authored::height_alpha(alpha_mode, crate::authored::parallax_mask()).is_ok()
        && !props.double_sided
        && !props.decal;
    // The final normal is chosen before the tangents are settled: a set's `_n` or `_p`, or the
    // game's height map, on a shape without a normal map needs tangents on the drawn mesh,
    // given here from `tangents` (kept, stored or generated, as for the game's normal map).
    let tangents = if normal.is_some() {
        true
    } else if (authored.is_some_and(|s| s.normal.is_some()) || height_wanted || game_height_wanted) && !had_tangents {
        has_uv && tangents.add_to(mesh)
    } else {
        had_tangents
    };
    let cutout = blended_cutout(props, cut, alpha_mode, || props.textures.first().is_some_and(|t| !t.is_empty() && textures.alpha_is_mask(t)));
    let gloss = props.glossiness.unwrap_or(10.0);
    // Emitted light × glow map, or uniform without a glow map. Not scaled by the camera
    // exposure (Bevy's default emissive_exposure_weight 0).
    let emissive = shape_emissive(props, alpha_mode);
    let glow = (emissive.is_some() || props.animated_emissive)
        .then_some(props.glow.as_deref())
        .flatten()
        .and_then(|t| textures.texture(t, true));
    let (reflectance, masked) = specular(props, normal.is_some(), unmasked);
    let (double_sided, cull_mode) = props_cull(props);
    let material = StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, props.material_alpha.unwrap_or(1.0)),
        base_color_texture: base,
        specular_texture: normal.clone().filter(|_| masked),
        normal_map_texture: normal,
        // Green points down the image (DirectX style), Bevy expects up [observed,
        // docs/spec-notes/nif.md §6 "Normal maps"].
        flip_normal_map_y: true,
        // Physical lighting: shapes without the specular flag (or hair) are rough, at least
        // 0.86, so the sky's image-based light does not gloss them [design, tuned by eye on
        // #3828's branch].
        perceptual_roughness: if crate::authored::physical_pbr() && !props.specular && !props.hair { gloss_roughness(gloss).max(0.86) } else { gloss_roughness(gloss) },
        metallic: 0.0,
        reflectance,
        emissive: emissive.unwrap_or(LinearRgba::BLACK),
        emissive_texture: glow,
        alpha_mode,
        double_sided,
        cull_mode,
        unlit: props.unlit,
        ..default()
    };
    let mut material = nif_material(material);
    if let Some(f) = shape_falloff(props, alpha_mode) {
        material.extension.falloff = f.params;
    }
    material.extension.alpha_cut = cut.unwrap_or(0.0);
    material.extension.blended_cutout = cutout;
    if solid_core_rules(props, cut, alpha_mode) {
        material.extension.solid_core = diffuse
            .zip(material.base.base_color_texture.as_ref())
            .map(|(t, image)| crate::nif_material::SolidCoreSource { texture: t.clone(), image: image.id(), animated: false });
    }
    if props.hair && cut.is_none() && matches!(alpha_mode, AlphaMode::Blend) {
        material.extension.alpha_cut = HAIR_DEPTH_CUT;
        material.extension.depth_write = true;
    }
    material.extension.add_whole = if adds_whole(props) { 1.0 } else { 0.0 };
    if let Some((e, cube, mask, mode)) = env {
        (material.extension.env, material.extension.env_how) = crate::env_map::env_uniforms(&e, mode, aids);
        material.extension.env_cube = Some(cube);
        material.extension.env_mask = mask;
    }
    material.extension.emissive_rule = shape_emissive_rule(props, alpha_mode);
    if props.unlit {
        unlit_vertex_colors(&mut material);
    }
    material.extension.decal = props.decal;
    // A baked shadow patch lies on the surface behind it: no fade where they meet
    // ([`baked_shadow_patch`]).
    if !props.shadow_patch && crate::nif_material::soft_intersections(alpha_mode, props.unlit, props.refraction.is_some(), burns(props)) {
        material.extension.soft.x = crate::nif_material::soft_distance(mesh_extent(mesh));
    }
    if let Some(strength) = props.refraction {
        refraction_material(&mut material, strength, props.fire_period);
        // The models' refracting shells bend all of their area, as before the cut was
        // read for refraction (docs/spec-notes/refraction.md §4).
        material.extension.alpha_cut = 0.0;
    }
    // Object LOD atlases hold near-black colour where their alpha is 0 (see
    // `shape_alpha_mode`); a soft edge would draw a little of it around every cut-out.
    material.extension.hard_edges = props.lod_building;
    // The game's own height map, before the legacy look is taken (it is part of the game's
    // material, so a set taken off later puts it back) and before any set (a set's `_p`
    // replaces it: `crate::authored::apply_height`).
    // Asked for only once the drawn mesh's tangents are settled: a shape whose tangents could
    // not be made (or do not reach the drawn mesh) stays flat without loading the file.
    let drawn_tangents = tangents && height_mesh_ok;
    let game_drawable = game_height_wanted && drawn_tangents;
    let game_height = game_slot.map(|slot| {
        let image = if game_drawable { textures.texture(slot, false) } else { None };
        crate::authored::apply_game_height(&mut material, image, game_drawable, drawn_tangents)
    });
    // The authored set last, over the legacy values chosen above and after the extension's
    // `emissive_rule` is assigned (its w lane carries the mark). Shapes that may take a set
    // keep their legacy look for later swaps (`crate::authored::LegacyLook`).
    if eligible {
        let legacy = crate::authored::LegacyLook {
            normal: material.base.normal_map_texture.clone(),
            specular: material.base.specular_texture.clone(),
            roughness: material.base.perceptual_roughness,
            reflectance: material.base.reflectance,
            metallic: material.base.metallic,
            // The drawn mesh's tangents: not those made on a copy of a drawn mesh, so a later
            // swap (`crate::authored::retexture`) puts no `_n` or `_p` on a mesh that cannot
            // draw it (#2979 review).
            tangents: drawn_tangents,
            specular_tint: material.base.specular_tint,
            parallax: crate::authored::Parallax::of(&material.base),
            scripted_tint: None,
        };
        if let Some(set) = authored {
            crate::authored::apply(&mut material, set, &legacy);
            crate::authored::note_built(set);
        }
        // The height map over either kind of set (a height-only set changes nothing else and
        // leaves the mark clear).
        if let Some(height) = height {
            if crate::authored::apply_height(&mut material, height, &legacy).is_ok() {
                crate::authored::note_height_built(authored.is_none());
                if game_height == Some(crate::authored::GameHeight::Drawn) {
                    crate::authored::note_game_height_replaced();
                }
            }
        }
        material.extension.legacy = Some(legacy);
    }
    (material, missing)
}

/// The alpha from which a blended hair-shader shape (hair, beards, brows) writes depth;
/// fainter fragments are dropped (`nif_material` module notes, "Depth of blended hair")
/// [engineering choice: high enough that the faint fringe of a strand leaves no outline
/// in the smoke or flames behind it, low enough that the strands keep their soft ends].
pub const HAIR_DEPTH_CUT: f32 = 0.25;

/// The alpha from which a hair model's shapes (the HAIR record's model) are drawn solid:
/// a cut-out at it ([`hair_model_cut_out`]) holds the hair's depth and shadow, and the
/// fainter texels are drawn blended over it ([`hair_model_layer`]) (docs/spec-notes/nif.md
/// §6 "Hair models: a solid core and a blended layer") [engineering choice, from the
/// hair textures' alpha: the Mohawk's shaved sides (`hairraider01.dds`, which 11 of the
/// 67 HAIR models use) are a plateau at 0.65–0.75 that has to stay see-through, while
/// `hairbase.dds` (Buzz Saw, Balding, Combover) is a plateau at 0.85–0.95 with no texel
/// above it, so a cut above 0.85 would leave those hairs no solid part at all].
pub const HAIR_MODEL_CUT: f32 = 0.8;

/// Make a hair model's shape the solid core of the hair: a cut-out at [`HAIR_MODEL_CUT`]
/// in place of the blend its NiAlphaProperty asks for (docs/spec-notes/nif.md §6). The
/// fainter texels are drawn by a second, blended draw of the shape
/// ([`hair_model_layer`]). Blended alone, the outer layer of `hairwavy.nif` (Chet), mapped
/// onto the soft edge of its texture's alpha, showed as a see-through shell around his
/// head against a bright window where the original draws solid hair [observed: owner's
/// frame of the original, 2026-11-03, #1454]: a blended layer that writes depth hides the
/// hair drawn after it behind it. Head parts that use the hair shader (beards, brows)
/// stay blended: as cut-outs `beardthin.nif` became a full beard, where the original's is
/// thin. A cut-out draws in the opaque passes, so it hides what lies behind it whatever
/// the order of the blended shapes, and it casts a shadow.
pub fn hair_model_cut_out(material: &mut NifMaterial) {
    if !matches!(material.base.alpha_mode, AlphaMode::Blend) || material.extension.refract {
        return;
    }
    material.base.alpha_mode = AlphaMode::Mask(HAIR_MODEL_CUT);
    material.extension.alpha_cut = 0.0;
    material.extension.depth_write = false;
}

/// The blended layer drawn over a hair model's solid core ([`hair_model_cut_out`]), made
/// from the shape's blended material: the texels below [`HAIR_MODEL_CUT`] (shaved and
/// stubbled scalp, the soft ends and edges of the strands) blend over what lies behind
/// them, the hair's own core included, as the NiAlphaProperty asks (SRC_ALPHA /
/// INV_SRC_ALPHA). It writes no depth, so its order among the hair's layers does not
/// matter, and it casts no shadow. Over the core it draws the core's own colour again at
/// the core's depth, which the main passes' depth test (greater or equal) lets through
/// and which leaves the colour as it is. `None` for a shape that is not blended or that
/// refracts.
pub fn hair_model_layer(material: &NifMaterial) -> Option<NifMaterial> {
    if !matches!(material.base.alpha_mode, AlphaMode::Blend) || material.extension.refract {
        return None;
    }
    let mut layer = material.clone();
    layer.extension.alpha_cut = 0.0;
    layer.extension.depth_write = false;
    Some(layer)
}

/// Make `material` a refracting one (`nif_material`, module notes "Refraction"): drawn
/// after the opaque pass from Bevy's copy of it (a specular transmission above 0 puts
/// the material there and keeps it out of the prepass), opaque, without depth writes.
pub fn refraction_material(material: &mut NifMaterial, strength: f32, fire_period: f32) {
    material.base.specular_transmission = 1.0;
    material.base.alpha_mode = AlphaMode::Opaque;
    material.extension.refraction = strength;
    material.extension.fire_period = fire_period;
    material.extension.refract = true;
    material.extension.depth_write = false;
    // #2981: a refracting copy (a cloak's: nvforever `stealth_field.rs`, player and actors
    // alike) draws no authored height map: the shader samples the standard material, with
    // the march, before its refraction return.
    crate::authored::recheck_height(material);
}

/// Numeric mesh admission is an Android-only engine policy; see nif-numeric-admission.md.
#[cfg(any(target_os = "android", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumericStream {
    Position,
    Normal,
    Uv,
    Tangent,
    Color,
    Weight,
    NormalizedWeight,
    InverseBind,
}

#[cfg(any(target_os = "android", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NumericError {
    stream: NumericStream,
    element: usize,
}

#[cfg(any(target_os = "android", test))]
fn finite_stream<const N: usize>(values: &[[f32; N]], stream: NumericStream) -> Result<(), NumericError> {
    match values.iter().position(|v| v.iter().any(|x| !x.is_finite())) {
        Some(element) => Err(NumericError { stream, element }),
        None => Ok(()),
    }
}

/// Check the actual streams after material preparation, including repaired colours
/// and generated tangents. Does not rewrite values or discard individual vertices.
#[cfg(any(target_os = "android", test))]
fn finite_mesh(mesh: &Mesh) -> Result<(), NumericError> {
    for (attribute, stream) in [
        (Mesh::ATTRIBUTE_POSITION, NumericStream::Position),
        (Mesh::ATTRIBUTE_NORMAL, NumericStream::Normal),
        (Mesh::ATTRIBUTE_UV_0, NumericStream::Uv),
        (Mesh::ATTRIBUTE_TANGENT, NumericStream::Tangent),
        (Mesh::ATTRIBUTE_COLOR, NumericStream::Color),
        (Mesh::ATTRIBUTE_JOINT_WEIGHT, NumericStream::NormalizedWeight),
    ] {
        match mesh.attribute(attribute) {
            Some(VertexAttributeValues::Float32x2(v)) => finite_stream(v, stream)?,
            Some(VertexAttributeValues::Float32x3(v)) => finite_stream(v, stream)?,
            Some(VertexAttributeValues::Float32x4(v)) => finite_stream(v, stream)?,
            _ => {}
        }
    }
    Ok(())
}

pub(crate) struct AdmittedShape {
    pub mesh: Mesh,
    pub triangles: usize,
    pub skin: Option<Skin>,
    pub stored: StoredArrays,
}

/// Numeric errors are separate from absent/unusable optional skin data. A numeric
/// failure must not become an ordinary unskinned shape in any of the three loaders.
#[cfg(any(target_os = "android", test))]
fn admit_shape(dec: &Decoded, shape: &Struct, data: &Struct) -> Result<Option<AdmittedShape>, NumericError> {
    let Some((mesh, triangles, stored)) = build_mesh(data)? else { return Ok(None) };
    let skin = shape_skin(dec, shape, mesh.count_vertices())?;
    Ok(Some(AdmittedShape { mesh, triangles, skin, stored }))
}

#[cfg(not(target_os = "android"))]
fn admit_shape_desktop(dec: &Decoded, shape: &Struct, data: &Struct) -> Option<AdmittedShape> {
    let (mesh, triangles, stored) = build_mesh_desktop(data)?;
    let skin = shape_skin_desktop(dec, shape, mesh.count_vertices());
    Some(AdmittedShape { mesh, triangles, skin, stored })
}

#[derive(Default)]
pub(crate) struct NumericRejections {
    #[cfg(any(target_os = "android", test))]
    total: usize,
    #[cfg(any(target_os = "android", test))]
    first: Vec<(usize, NumericError)>,
}

impl NumericRejections {
    #[cfg(any(target_os = "android", test))]
    fn record(&mut self, block: usize, error: NumericError) {
        self.total = self.total.saturating_add(1);
        if self.first.len() < 8 {
            self.first.push((block, error));
        }
    }

    #[cfg(any(target_os = "android", test))]
    pub(crate) fn admit_android(&mut self, block: usize, dec: &Decoded, shape: &Struct, data: &Struct) -> Option<AdmittedShape> {
        match admit_shape(dec, shape, data) {
            Ok(shape) => shape,
            Err(error) => {
                self.record(block, error);
                None
            }
        }
    }

    pub fn admit(&mut self, block: usize, dec: &Decoded, shape: &Struct, data: &Struct) -> Option<AdmittedShape> {
        #[cfg(target_os = "android")]
        {
            self.admit_android(block, dec, shape, data)
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = block;
            admit_shape_desktop(dec, shape, data)
        }
    }

    #[cfg(any(target_os = "android", test))]
    pub(crate) fn accept_mesh_android(&mut self, block: usize, mesh: &Mesh) -> bool {
        match finite_mesh(mesh) {
            Ok(()) => true,
            Err(error) => {
                self.record(block, error);
                false
            }
        }
    }

    pub fn accept_mesh(&mut self, block: usize, mesh: &Mesh) -> bool {
        #[cfg(target_os = "android")]
        {
            self.accept_mesh_android(block, mesh)
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (block, mesh);
            true
        }
    }

    pub fn report(&self, source: &str) {
        #[cfg(any(target_os = "android", test))]
        if self.total > 0 {
            let source: String = source.chars().take(240).collect();
            warn!("NIF numeric admission: {source}: {} shape(s) rejected; first block/reasons: {:?}", self.total, self.first);
        }
        #[cfg(not(any(target_os = "android", test)))]
        let _ = source;
    }
}

/// The mesh of a shape-data block, its triangle count, and the tangent arrays it stores
/// (for [`Tangents::Stored`]; empty when it stores none) — desktop path matching `main`.
#[cfg(not(target_os = "android"))]
fn build_mesh_desktop(data: &Struct) -> Option<(Mesh, usize, StoredArrays)> {
    let md = mesh_data(data);
    if md.positions.is_empty() || md.triangles.is_empty() {
        return None;
    }
    let n = md.positions.len();
    let tris = md.triangles.len();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, md.positions);
    let has_normals = md.normals.len() == n;
    if has_normals {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, md.normals);
    }
    if let Some(uv) = md.uv_sets.first().filter(|u| u.len() == n) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv.clone());
    }
    if md.colors.len() == n {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, md.colors);
    }
    let idx: Vec<u32> = md.triangles.into_iter().filter(|&[a, b, c]| (a as usize) < n && (b as usize) < n && (c as usize) < n).flatten().collect();
    mesh.insert_indices(Indices::U32(idx));
    if !has_normals {
        mesh.compute_normals();
    }
    Some((mesh, tris, StoredArrays { tangents: md.tangents, bitangents: md.bitangents }))
}

/// The mesh of a shape-data block, its triangle count, and the tangent arrays it stores
/// (for [`Tangents::Stored`]; empty when it stores none) — Android finite-admission path.
#[cfg(any(target_os = "android", test))]
pub(crate) fn build_mesh(data: &Struct) -> Result<Option<(Mesh, usize, StoredArrays)>, NumericError> {
    let md = mesh_data(data);
    if md.positions.is_empty() || md.triangles.is_empty() {
        return Ok(None);
    }
    finite_stream(&md.positions, NumericStream::Position)?;
    let n = md.positions.len();
    let tris = md.triangles.len();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, md.positions);
    let has_normals = md.normals.len() == n;
    if has_normals {
        finite_stream(&md.normals, NumericStream::Normal)?;
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, md.normals);
    }
    if let Some(uv) = md.uv_sets.first().filter(|u| u.len() == n) {
        finite_stream(uv, NumericStream::Uv)?;
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv.clone());
    }
    if md.colors.len() == n {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, md.colors);
    }
    let max = n as u32;
    let idx: Vec<u32> = md.triangles.iter().filter(|t| t.iter().all(|&i| i < max)).flatten().copied().collect();
    mesh.insert_indices(Indices::U32(idx));
    if !has_normals {
        mesh.compute_normals();
    }
    Ok(Some((mesh, tris, StoredArrays { tangents: md.tangents, bitangents: md.bitangents })))
}

/// Per-vertex joint indices and normalised weights (four influences per vertex).
type SkinWeights = (Vec<[u16; 4]>, Vec<[f32; 4]>);

/// Joint indices and weights per vertex from NiSkinData (top four influences) — desktop path.
#[cfg(not(target_os = "android"))]
fn skin_weights_desktop(skin_data: &Struct, vertices: usize) -> Option<SkinWeights> {
    let bones = skin_data.list("bone_list")?;
    let mut infl: Vec<Vec<(u16, f32)>> = vec![Vec::new(); vertices];
    for (b, bone) in bones.iter().enumerate() {
        let Value::Struct(bone) = bone else { continue };
        for w in bone.list("vertex_weights").unwrap_or_default() {
            let Value::Struct(w) = w else { continue };
            let (Some(i), Some(wt)) = (w.int("index"), w.float("weight")) else { continue };
            if let Some(v) = infl.get_mut(i as usize) {
                v.push((b as u16, wt));
            }
        }
    }
    if infl.iter().all(|v| v.is_empty()) {
        return None;
    }
    let mut joints = Vec::with_capacity(vertices);
    let mut weights = Vec::with_capacity(vertices);
    for mut v in infl {
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v.truncate(4);
        let sum: f32 = v.iter().map(|x| x.1).sum();
        let mut j = [0u16; 4];
        let mut w = [0f32; 4];
        for (k, (b, wt)) in v.iter().enumerate() {
            j[k] = *b;
            w[k] = if sum > 0.0 { wt / sum } else { 0.0 };
        }
        if sum <= 0.0 {
            w[0] = 1.0;
        }
        joints.push(j);
        weights.push(w);
    }
    Some((joints, weights))
}

/// Joint indices and weights per vertex from NiSkinData (top four influences) — Android path.
#[cfg(any(target_os = "android", test))]
fn skin_weights(skin_data: &Struct, vertices: usize) -> Result<Option<SkinWeights>, NumericError> {
    let Some(bones) = skin_data.list("bone_list") else { return Ok(None) };
    let mut infl: Vec<Vec<(u16, f32)>> = vec![Vec::new(); vertices];
    for (b, bone) in bones.iter().enumerate() {
        let Value::Struct(bone) = bone else { continue };
        for w in bone.list("vertex_weights").unwrap_or_default() {
            let Value::Struct(w) = w else { continue };
            let (Some(i), Some(wt)) = (w.int("index"), w.float("weight")) else { continue };
            if let Some(v) = infl.get_mut(i as usize) {
                if !wt.is_finite() {
                    return Err(NumericError { stream: NumericStream::Weight, element: i as usize });
                }
                v.push((b as u16, wt));
            }
        }
    }
    if infl.iter().all(|v| v.is_empty()) {
        return Ok(None);
    }
    let mut joints = Vec::with_capacity(vertices);
    let mut weights = Vec::with_capacity(vertices);
    for (vertex, mut v) in infl.into_iter().enumerate() {
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v.truncate(4);
        // Four finite f32 values cannot overflow f64. Compensation also preserves
        // a small residual when finite signed influences nearly cancel.
        let (mut sum, mut correction) = (0.0_f64, 0.0_f64);
        for &(_, weight) in &v {
            let weight = f64::from(weight);
            let next = sum + weight;
            correction += if sum.abs() >= weight.abs() { (sum - next) + weight } else { (weight - next) + sum };
            sum = next;
        }
        let sum = sum + correction;
        let mut j = [0u16; 4];
        let mut w = [0f32; 4];
        for (k, (b, wt)) in v.iter().enumerate() {
            j[k] = *b;
            w[k] = if sum > 0.0 { (f64::from(*wt) / sum) as f32 } else { 0.0 };
            if !w[k].is_finite() {
                return Err(NumericError { stream: NumericStream::NormalizedWeight, element: vertex });
            }
        }
        if sum <= 0.0 {
            w[0] = 1.0;
        }
        joints.push(j);
        weights.push(w);
    }
    Ok(Some((joints, weights)))
}

/// A shape's skin: the bone blocks it binds to, per bone the inverse bind matrix in
/// shape space (the bone's stored skin transform, docs/spec-notes/nif.md §6), and per
/// vertex the joint indices and weights.
pub(crate) struct Skin {
    pub bones: Vec<usize>,
    pub inverse_binds: Vec<Mat4>,
    weights: SkinWeights,
}

impl Skin {
    /// Add the joint attributes to `mesh`.
    pub(crate) fn insert_into(self, mesh: &mut Mesh) -> Vec<Mat4> {
        let (ji, jw) = self.weights;
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(ji));
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, jw);
        self.inverse_binds
    }
}

/// The skin of `shape` (with `vertices` vertices), if it has a usable one — desktop path.
#[cfg(not(target_os = "android"))]
fn shape_skin_desktop(dec: &Decoded, shape: &Struct, vertices: usize) -> Option<Skin> {
    let si = shape.block_ref("skin_instance").and_then(|r| dec.get(r))?;
    let bones: Vec<usize> = si.refs("bones").unwrap_or_default().iter().map(|&b| usize::try_from(b).ok()).collect::<Option<_>>()?;
    let sd = si.block_ref("data").and_then(|r| dec.get(r))?;
    let inverse_binds: Vec<Mat4> = sd
        .list("bone_list")
        .unwrap_or_default()
        .iter()
        .map(|b| match b {
            Value::Struct(b) => Some(Mat4::from(affine(&NifTransform::from_struct(b.strukt("skin_transform")?)?))),
            _ => None,
        })
        .collect::<Option<_>>()?;
    if inverse_binds.len() != bones.len() {
        return None;
    }
    Some(Skin { bones, inverse_binds, weights: skin_weights_desktop(sd, vertices)? })
}

/// The skin of `shape` (with `vertices` vertices), if it has a usable one — Android path.
#[cfg(any(target_os = "android", test))]
fn shape_skin(dec: &Decoded, shape: &Struct, vertices: usize) -> Result<Option<Skin>, NumericError> {
    let Some(si) = shape.block_ref("skin_instance").and_then(|r| dec.get(r)) else { return Ok(None) };
    let Some(sd) = si.block_ref("data").and_then(|r| dec.get(r)) else { return Ok(None) };
    let weights = skin_weights(sd, vertices)?;
    let Some(bones) = si.refs("bones").unwrap_or_default().iter().map(|&b| usize::try_from(b).ok()).collect::<Option<Vec<usize>>>() else { return Ok(None) };
    let mut inverse_binds = Vec::new();
    for (element, bone) in sd.list("bone_list").unwrap_or_default().iter().enumerate() {
        let Value::Struct(bone) = bone else { return Ok(None) };
        let Some(transform) = bone.strukt("skin_transform").and_then(NifTransform::from_struct) else { return Ok(None) };
        let bind = Mat4::from(affine(&transform));
        if !bind.is_finite() {
            return Err(NumericError { stream: NumericStream::InverseBind, element });
        }
        inverse_binds.push(bind);
    }
    if inverse_binds.len() != bones.len() {
        return Ok(None);
    }
    Ok(weights.map(|weights| Skin { bones, inverse_binds, weights }))
}

impl SpawnCtx<'_, '_> {
    /// Spawn the NIF at `path` under a new root entity. When `skeleton` is given, that
    /// file's node hierarchy is spawned first and skinned shapes bind to its bones by
    /// name: creature and character body meshes store their bones flat, as direct
    /// children of the root with bind transforms baked in, while animations are relative
    /// to the skeleton's hierarchy [observed, docs/spec-notes/nif.md §6].
    pub fn spawn_nif(&mut self, path: &str, skeleton: Option<&str>) -> Result<SpawnReport, String> {
        let root = self
            .commands
            .spawn((
                Name::new(path.to_string()),
                NifRoot,
                Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
                Visibility::default(),
            ))
            .id();
        // A panic while building is this file's failure, not the game's (guard.rs).
        let skel = match skeleton {
            Some(sp) => Some(crate::guard::guarded(|| self.spawn_into(sp, root, None))?),
            None => None,
        };
        let mut report = crate::guard::guarded(|| self.spawn_into(path, root, skel.as_ref().map(|s| &s.nodes)))?;
        if let Some(sk) = skel {
            // Animation targets: skeleton bones take precedence over the mesh's flat copies.
            for (k, v) in sk.nodes {
                report.nodes.insert(k, v);
            }
            report.bind.extend(sk.bind);
            report.skeleton = skeleton.map(str::to_string);
        }
        report.root = Some(root);
        Ok(report)
    }

    fn spawn_into(
        &mut self,
        path: &str,
        root: Entity,
        skeleton: Option<&HashMap<String, Entity>>,
    ) -> Result<SpawnReport, String> {
        let bytes = self.data.read(path).ok_or_else(|| format!("{path} not found"))?;
        let schema = self.data.schema.clone();
        let file = NifFile::parse(&bytes, Some(&schema)).map_err(|e| e.to_string())?;
        let dec = Decoded::new(&file, &bytes, &schema);
        let objects = scene_objects(&file, &dec);
        let shaded = shaded_model(&file, &dec);
        let mut report = SpawnReport { blocks: file.blocks.len(), ..Default::default() };

        // Pass 1: one entity per node or shape, with its local transform.
        let mut entity_of: HashMap<usize, Entity> = HashMap::new();
        let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
        for o in &objects {
            let flags = dec.blocks[o.block].as_ref().ok().and_then(|s| s.int("flags")).unwrap_or(0);
            let mut e = self.commands.spawn((
                Name::new(o.name.clone().unwrap_or_else(|| o.type_name.clone())),
                to_bevy(&o.transform),
                Visibility::default(),
            ));
            if flags & 1 != 0 || is_editor_marker(o.name.as_deref()) {
                e.insert((NifHidden, Visibility::Hidden));
            }
            let id = e.id();
            entity_of.insert(o.block, id);
            if let Some(n) = &o.name {
                report.nodes.entry(n.clone()).or_insert(id);
                report.bind.insert(id, to_bevy(&o.transform));
            }
        }
        for o in &objects {
            let parent = o.parent.and_then(|p| entity_of.get(&p)).copied().unwrap_or(root);
            self.commands.entity(parent).add_child(entity_of[&o.block]);
        }

        // World transforms in NIF space (for bounds), parents first.
        let mut world: HashMap<usize, Affine3A> = HashMap::new();
        fn world_of(b: usize, by_block: &HashMap<usize, &SceneObject>, cache: &mut HashMap<usize, Affine3A>, depth: usize) -> Affine3A {
            if let Some(w) = cache.get(&b) {
                return *w;
            }
            let o = by_block[&b];
            let local = affine(&o.transform);
            let w = match o.parent {
                Some(p) if depth < 512 && by_block.contains_key(&p) => world_of(p, by_block, cache, depth + 1) * local,
                _ => local,
            };
            cache.insert(b, w);
            w
        }
        let to_y_up = Affine3A::from_rotation_x(-FRAC_PI_2);

        // Pass 2: geometry and materials.
        let mut rejected = NumericRejections::default();
        for o in objects.iter().filter(|o| !o.is_node) {
            let Ok(shape) = &dec.blocks[o.block] else { continue };
            let Some(data_ref) = shape.block_ref("data") else { continue };
            let Some(data) = dec.get(data_ref) else { continue };
            let Some(AdmittedShape { mut mesh, triangles: tris, skin, .. }) = rejected.admit(o.block, &dec, shape, data) else { continue };

            let mut props = shape_props(&file, &dec, o, &by_block);
            // Landscape LOD takes no authored set (#2981), as in the model loader.
            props.no_authored = crate::model::is_lod_model(path);
            let (material, missing) = shape_material(
                &props,
                &mut mesh,
                &mut self.textures,
                &self.data,
                &mut self.images,
                self.formats.as_deref(),
            );
            if !rejected.accept_mesh(o.block, &mesh) {
                continue;
            }
            report.textures_missing.extend(missing);
            if props.is_helper(shaded) {
                self.commands.entity(entity_of[&o.block]).insert((NifHidden, Visibility::Hidden));
            }

            // Skinning: bones bind to the skeleton's nodes by name when one is given.
            let joint = |b: &usize| {
                let by_name = by_block.get(b).and_then(|o| o.name.as_ref()).and_then(|n| skeleton?.get(n));
                by_name.or_else(|| entity_of.get(b)).copied()
            };
            let mut skinned = None;
            if let Some(skin) = skin {
                if let Some(joints) = skin.bones.iter().map(joint).collect::<Option<Vec<Entity>>>() {
                    let binds = skin.insert_into(&mut mesh);
                    skinned = Some(SkinnedMesh { inverse_bindposes: self.bindposes.add(binds), joints });
                }
            }

            // Bounds in Bevy world space.
            let w = to_y_up * world_of(o.block, &by_block, &mut world, 0);
            if let Some(VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
                for v in p {
                    let q = w.transform_point3(Vec3::from_array(*v));
                    report.bounds = Some(match report.bounds {
                        None => (q, q),
                        Some((lo, hi)) => (lo.min(q), hi.max(q)),
                    });
                }
            }

            let e = entity_of[&o.block];
            let mut cmd = self.commands.entity(e);
            match skinned {
                Some(s) => {
                    let mesh = mesh.with_generated_skinned_mesh_bounds().unwrap_or_else(|_| Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD));
                    cmd.insert((Mesh3d(self.meshes.add(mesh)), MeshMaterial3d(self.materials.add(material)), s, DynamicSkinnedMeshBounds));
                    report.skinned += 1;
                }
                None => {
                    cmd.insert((Mesh3d(self.meshes.add(mesh)), MeshMaterial3d(self.materials.add(material))));
                }
            }
            report.shapes += 1;
            report.triangles += tris;
        }
        rejected.report(path);
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoTextures;
    impl TextureSource for NoTextures {
        fn texture(&mut self, _: &str, _: bool) -> Option<Handle<Image>> {
            None
        }
        fn lacks_alpha(&self, _: &Handle<Image>) -> bool {
            false
        }
        fn alpha_is_mask(&self, _: &str) -> bool {
            false
        }
        fn alpha_is_opaque(&self, _: &str) -> bool {
            false
        }
        fn resolve(&self, raw_path: &str) -> String {
            fnv_vfs::texture_path(raw_path)
        }
        fn pbr_set(&mut self, _: &str) -> Option<crate::authored::PbrSet> {
            None
        }
        fn authored_on(&self) -> bool {
            false
        }
    }

    /// Textures whose alpha is a cut-out mask when their path says "mask".
    struct MaskTextures;
    impl TextureSource for MaskTextures {
        fn texture(&mut self, _: &str, _: bool) -> Option<Handle<Image>> {
            None
        }
        fn lacks_alpha(&self, _: &Handle<Image>) -> bool {
            false
        }
        fn alpha_is_mask(&self, raw_path: &str) -> bool {
            raw_path.contains("mask")
        }
        fn alpha_is_opaque(&self, raw_path: &str) -> bool {
            raw_path.contains("solid")
        }
        fn resolve(&self, raw_path: &str) -> String {
            fnv_vfs::texture_path(raw_path)
        }
        fn pbr_set(&mut self, _: &str) -> Option<crate::authored::PbrSet> {
            None
        }
        fn authored_on(&self) -> bool {
            false
        }
    }

    #[test]
    fn a_dismemberment_cap_is_alpha_tested_and_drawn_from_both_sides() {
        let mut tri = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        tri.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        // `upperbody.nif` `bodycaps`: NiAlphaProperty 0x12EC (test GREATER 127), no stencil;
        // actor.rs sets `double_sided` on shapes with cap partitions.
        let cap = Props { alpha: Some((0x12EC, 127)), textures: vec![r"gore\meatcapgore01.dds".into()], double_sided: true, ..Default::default() };
        let (m, _) = shape_material_with(&cap, &mut tri.clone(), &mut NoTextures, Tangents::Generate);
        assert!(matches!(m.base.alpha_mode, AlphaMode::Mask(_)));
        assert!(m.base.double_sided && m.base.cull_mode.is_none());
        let (m, _) = shape_material_with(&Props { double_sided: false, ..cap }, &mut tri, &mut NoTextures, Tangents::Generate);
        assert!(!m.base.double_sided && m.base.cull_mode.is_some());
    }

    #[test]
    fn unlit_vertex_colours_are_blended_as_display_values() {
        // The Gomorrah Suites sconce glow (neoclassicalsconce.nif, NeoclassicalSconce:9):
        // unlit, additive ONE / ONE (0x1001), no texture, vertex colour 0.34 at the bowl
        // and black on the outline (#2435).
        let glow = Props { unlit: true, alpha: Some((0x1001, 0)), textures: vec![String::new()], ..Default::default() };
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        let stored = vec![[0.34f32, 0.32, 0.27, 1.0], [0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]];
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, stored.clone());
        let (m, _) = shape_material_with(&glow, &mut mesh, &mut NoTextures, Tangents::Generate);
        assert_eq!(m.base.alpha_mode, AlphaMode::Add);
        assert!(m.base.unlit);
        // The mesh keeps the stored display values; the shader makes them linear.
        assert_eq!(m.extension.emissive_rule.z, 1.0);
        let Some(VertexAttributeValues::Float32x4(kept)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) else { panic!("colours") };
        assert_eq!(kept, &stored);
        // Nine tenths of the way to the black outline: blended as display values and made
        // linear there, the colour is a tenth of the bowl's display value; blended after
        // being made linear (before #2435), 0.096, almost three times as bright.
        let t = 0.9;
        let per_fragment = display_to_linear(0.34 * (1.0 - t));
        let per_vertex = display_to_linear(0.34) * (1.0 - t);
        assert!((Color::linear_rgb(per_fragment, 0.0, 0.0).to_srgba().red - 0.034).abs() < 1e-3);
        assert!(Color::linear_rgb(per_vertex, 0.0, 0.0).to_srgba().red > 0.09);
        // At the vertices both readings agree.
        assert!((display_to_linear(0.34) - Color::srgb(0.34, 0.0, 0.0).to_linear().red).abs() < 1e-6);
        // A lit shape's vertex colours stay linear factors.
        let lit = Props { textures: vec![String::new()], ..Default::default() };
        let (m, _) = shape_material_with(&lit, &mut Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()), &mut NoTextures, Tangents::Generate);
        assert_eq!(m.extension.emissive_rule.z, 0.0);
    }

    #[test]
    fn blended_shapes_with_a_cut_out_mask_are_blended_cutouts() {
        let quad = || {
            let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
            m
        };
        // The corn rows: blend SRC_ALPHA / INV_SRC_ALPHA, test GREATER 0 (0x12ED), lit.
        let corn = Props { alpha: Some((0x12ED, 0)), textures: vec![r"landscape\plants\maize_mask.dds".into()], ..Default::default() };
        let (m, _) = shape_material_with(&corn, &mut quad(), &mut MaskTextures, Tangents::Generate);
        assert!(matches!(m.base.alpha_mode, AlphaMode::Blend), "drawn blended until FSR asks");
        assert_eq!(m.extension.blended_cutout, Some(BLENDED_CUTOUT_CUT));
        // Translucent texture alpha (dirty glass), or no texture: stays blended only.
        assert_eq!(shape_material_with(&corn, &mut quad(), &mut NoTextures, Tangents::Generate).0.extension.blended_cutout, None);
        // Anything else that sets alpha keeps it a real blend.
        for props in [
            Props { material_alpha: Some(0.5), ..corn.clone() },
            Props { animated_alpha: true, ..corn.clone() },
            Props { unlit: true, ..corn.clone() },
            Props { decal: true, ..corn.clone() },
            Props { hair: true, ..corn.clone() },
            Props { refraction: Some(1.0), ..corn.clone() },
            // Not tested, additive, or tested at a cut that already makes it a cut-out.
            Props { alpha: Some((0x12ED & !0x200, 0)), ..corn.clone() },
            Props { alpha: Some((0x12ED & !(0xF << 5), 0)), ..corn.clone() },
            Props { alpha: Some((0x12ED, 128)), ..corn.clone() },
        ] {
            assert_eq!(shape_material_with(&props, &mut quad(), &mut MaskTextures, Tangents::Generate).0.extension.blended_cutout, None, "{props:?}");
        }
    }

    /// The water tank's shape (flags 4333, z-write, a base texture) may have a solid core;
    /// what can thin it, or sets its alpha otherwise, keeps it from one.
    #[test]
    fn solid_core_rules_take_only_lit_depth_writing_blends_whose_texture_alone_sets_alpha() {
        let tank = Props { alpha: Some((4333, 0)), z_write: true, textures: vec!["tank.dds".into()], ..Default::default() };
        let rules = |p: &Props| solid_core_rules(p, None, shape_alpha_mode(p));
        assert!(rules(&tank));
        assert!(rules(&Props { material_alpha: Some(1.0), ..tank.clone() }));
        // The corn rows: blend and test below the cut.
        assert!(rules(&Props { alpha: Some((0x12ED, 0)), ..tank.clone() }));
        assert!(!solid_core_rules(&tank, Some(0.5), AlphaMode::Blend), "a vertex-alpha cut");
        assert!(!solid_core_rules(&tank, None, AlphaMode::Opaque), "drawn opaque");
        assert!(!solid_core_rules(&tank, None, AlphaMode::Premultiplied), "a window's reflection");
        for p in [
            Props { z_write: false, ..tank.clone() },
            Props { alpha: None, ..tank.clone() },
            // Blend off, additive, alpha-tested at a cut of its own.
            Props { alpha: Some((4332, 0)), ..tank.clone() },
            Props { alpha: Some((4333 & !(0xF << 5), 0)), ..tank.clone() },
            Props { alpha: Some((0x12ED, 128)), ..tank.clone() },
            Props { textures: Vec::new(), ..tank.clone() },
            Props { textures: vec![String::new()], ..tank.clone() },
            Props { vertex_alpha: true, ..tank.clone() },
            Props { material_alpha: Some(0.99), ..tank.clone() },
            Props { animated_alpha: true, ..tank.clone() },
            Props { alpha_gated: true, ..tank.clone() },
            Props { falloff: Some(Falloff { params: Vec4::new(1.0, 0.0, 1.0, 0.0) }), ..tank.clone() },
            Props { unlit: true, ..tank.clone() },
            Props { decal: true, ..tank.clone() },
            Props { refraction: Some(0.1), ..tank.clone() },
            Props { hair: true, ..tank.clone() },
            Props { skin: true, ..tank.clone() },
            Props { lod_building: true, ..tank.clone() },
            Props { env: Some(crate::env_map::EnvFlags { scale: 1.0, window: true, light_fade: false }), ..tank.clone() },
        ] {
            assert!(!rules(&p), "{p:?}");
        }
    }

    /// The built material names its core's texture only for such a shape, and the core is a
    /// hard cut-out copy of it; every run-time change of the blend takes the core away.
    #[test]
    fn a_solid_core_is_a_hard_cut_out_copy_and_goes_with_any_change_of_the_blend() {
        let mut tri = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        tri.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        tri.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        let tank = Props { alpha: Some((4333, 0)), z_write: true, textures: vec!["tank.dds".into()], ..Default::default() };
        let build = |p: &Props| shape_material_with(p, &mut tri.clone(), &mut AllTextures, Tangents::Generate).0;
        let m = build(&tank);
        let source = m.extension.solid_core.clone().expect("a candidate");
        assert_eq!(source.texture, "tank.dds");
        assert_eq!(Some(source.image), m.base.base_color_texture.as_ref().map(Handle::id));
        assert!(build(&Props { z_write: false, ..tank.clone() }).extension.solid_core.is_none());
        // Without its texture there is nothing to cut.
        assert!(shape_material_with(&tank, &mut tri.clone(), &mut NoTextures, Tangents::Generate).0.extension.solid_core.is_none());

        let core = solid_core(&m, SOLID_CORE_CUT).expect("a core");
        assert_eq!(core.base.alpha_mode, AlphaMode::Mask(SOLID_CORE_CUT));
        assert!(core.extension.hard_edges);
        assert!(core.extension.solid_core.is_none() && core.extension.blended_cutout.is_none() && core.extension.legacy.is_none());
        assert_eq!(core.base.base_color_texture, m.base.base_color_texture);
        assert!(solid_core(&core, SOLID_CORE_CUT).is_none(), "no core of a core");
        assert!(!crate::nif_material::see_through(&core));

        type Change = fn(&mut NifMaterial);
        let changes: [(&str, Change); 12] = [
            ("a fade's or a script's alpha", |m| m.base.base_color.set_alpha(0.99)),
            ("drawn opaque", |m| m.base.alpha_mode = AlphaMode::Opaque),
            ("FSR's cut-out", |m| m.base.alpha_mode = AlphaMode::Mask(0.5)),
            ("a stealth field", |m| refraction_material(m, 0.1, 0.0)),
            ("an effect shader's holes", |m| m.extension.holes = Vec4::new(128.0, 0.0, 0.0, 0.0)),
            ("a texture swap", |m| m.base.base_color_texture = Some(bevy::asset::uuid_handle!("6d0f3f0e-6f5b-4a1c-9a64-2f8f5c7d1e90"))),
            ("no texture", |m| m.base.base_color_texture = None),
            ("an alpha cut", |m| m.extension.alpha_cut = 0.5),
            ("a height map", |m| m.base.depth_map = Some(Handle::default())),
            ("a first-person copy", |m| m.extension.first_person = true),
            ("depth written by the blend", |m| m.extension.depth_write = true),
            ("a decal copy", |m| m.extension.decal = true),
        ];
        for (what, change) in changes {
            let mut c = m.clone();
            change(&mut c);
            assert!(solid_core(&c, SOLID_CORE_CUT).is_none(), "{what}");
        }
        // Followed, not refused: a changed emissive colour is the core's too.
        let mut lit = m.clone();
        lit.base.emissive = LinearRgba::rgb(0.5, 0.2, 0.1);
        assert_eq!(solid_core(&lit, SOLID_CORE_CUT).expect("still a core").base.emissive, lit.base.emissive);
    }

    /// #3230: a shape that asks to be blended and that nothing can thin is drawn opaque.
    /// The placed T-45d Power Armor: blend SRC_ALPHA / INV_SRC_ALPHA, test off, threshold 0
    /// (flags 4333), material alpha 1, no vertex colours, an opaque texture.
    #[test]
    fn a_blended_shape_that_nothing_thins_is_opaque() {
        let quad = || {
            let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
            m
        };
        let mode = |p: &Props| shape_material_with(p, &mut quad(), &mut MaskTextures, Tangents::Generate).0.base.alpha_mode;
        let armour = Props { alpha: Some((4333, 0)), material_alpha: Some(1.0), textures: vec![r"armor\powerarmor\powerarmorbody_solid.dds".into()], ..Default::default() };
        assert!(matches!(mode(&armour), AlphaMode::Opaque), "{:?}", mode(&armour));
        // No stated material alpha is 1 too; so is one tested below the cut (0x12ED).
        assert!(matches!(mode(&Props { material_alpha: None, ..armour.clone() }), AlphaMode::Opaque));
        assert!(matches!(mode(&Props { alpha: Some((0x12ED, 0)), ..armour.clone() }), AlphaMode::Opaque));
        // A texture with any alpha below 1 (or none loaded), and everything else that can
        // thin a shape, keeps the blend.
        for props in [
            Props { textures: vec![r"clutter\glass.dds".into()], ..armour.clone() },
            Props { textures: vec![String::new()], ..armour.clone() },
            Props { textures: Vec::new(), ..armour.clone() },
            Props { material_alpha: Some(0.99), ..armour.clone() },
            Props { vertex_alpha: true, ..armour.clone() },
            Props { animated_alpha: true, ..armour.clone() },
            Props { falloff: Some(crate::nif_material::Falloff { params: Vec4::new(1.0, 0.5, 1.0, 0.0) }), ..armour.clone() },
            Props { unlit: true, ..armour.clone() },
            Props { decal: true, ..armour.clone() },
            Props { hair: true, ..armour.clone() },
            Props { env: Some(crate::env_map::EnvFlags { scale: 1.0, window: true, light_fade: false }), ..armour.clone() },
        ] {
            assert!(matches!(mode(&props), AlphaMode::Blend), "{props:?} drawn {:?}", mode(&props));
            assert!(!blend_is_opaque(&props, None, AlphaMode::Blend, || props.textures.first().is_some_and(|t| t.contains("solid"))), "{props:?}");
        }
        // Other blends are not this one: additive stays additive, multiply multiply.
        assert!(matches!(mode(&Props { alpha: Some((4333 & !(0xF << 5), 0)), ..armour.clone() }), AlphaMode::Add));
        assert!(matches!(mode(&Props { alpha: Some(((4333 & !(0xF << 1)) | (1 << 1), 0)), ..armour.clone() }), AlphaMode::Multiply));
        // A shape that is not blended at all is as it was.
        assert!(matches!(mode(&Props { alpha: None, ..armour.clone() }), AlphaMode::Opaque));
        assert!(matches!(mode(&Props { alpha: Some((0x12ED, 128)), ..armour }), AlphaMode::Mask(_)));
    }

    fn triangle() -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3])
    }

    /// `hairbase.nif`'s NiAlphaProperty: blended, no test (flags 4333) [observed].
    #[test]
    fn blended_hair_writes_depth_above_its_cut() {
        let hair = Props { hair: true, alpha: Some((4333, 0)), ..Default::default() };
        let (m, _) = shape_material_with(&hair, &mut triangle(), &mut NoTextures, Tangents::Generate);
        assert!(matches!(m.base.alpha_mode, AlphaMode::Blend));
        assert!(m.extension.depth_write);
        assert_eq!(m.extension.alpha_cut, HAIR_DEPTH_CUT);
        assert!(crate::nif_material::see_through(&m), "hair still casts no shadow");
        // Other blended shapes (smoke, glass) write none and drop nothing.
        let (m, _) = shape_material_with(&Props { hair: false, ..hair }, &mut triangle(), &mut NoTextures, Tangents::Generate);
        assert!(!m.extension.depth_write);
        assert_eq!(m.extension.alpha_cut, 0.0);
    }

    /// A hair model's blended shape becomes a solid cut-out (#1454) with a blended layer
    /// drawn over it (#1576); one that is already a cut-out or refracts is left alone.
    #[test]
    fn hair_models_are_a_core_and_a_layer() {
        let hair = Props { hair: true, alpha: Some((4333, 0)), ..Default::default() };
        let (blended, _) = shape_material_with(&hair, &mut triangle(), &mut NoTextures, Tangents::Generate);
        let mut m = blended.clone();
        hair_model_cut_out(&mut m);
        assert_eq!(m.base.alpha_mode, AlphaMode::Mask(HAIR_MODEL_CUT));
        assert!(!m.extension.depth_write);
        assert_eq!(m.extension.alpha_cut, 0.0);
        assert!(!crate::nif_material::see_through(&m), "a cut-out casts a shadow");
        let layer = hair_model_layer(&blended).expect("a blended hair shape has a layer");
        assert_eq!(layer.base.alpha_mode, AlphaMode::Blend);
        assert!(!layer.extension.depth_write, "the layer writes no depth");
        assert_eq!(layer.extension.alpha_cut, 0.0, "the layer drops nothing");
        assert!(crate::nif_material::see_through(&layer), "the layer casts no shadow");
        let mut masked = m.clone();
        masked.base.alpha_mode = AlphaMode::Mask(0.7);
        hair_model_cut_out(&mut masked);
        assert_eq!(masked.base.alpha_mode, AlphaMode::Mask(0.7));
        assert!(hair_model_layer(&masked).is_none());
    }

    /// Texture-set lists for the environment-map tests.
    fn texture_set(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| s.to_string()).collect()
    }

    /// The flags of a window's environment map: both bits; scale 1, no Env Map Fade.
    const WINDOW_FLAGS: crate::env_map::EnvFlags = crate::env_map::EnvFlags { scale: 1.0, window: true, light_fade: false };

    /// A window environment map needs both flags (bits 7 and 21), a lit shader, a cube in
    /// slot 4 and a scale above 0; the mask in slot 5 is optional (`vwindow01.nif`: flags
    /// 0x82200180, ShinyBright_e.dds, no mask; nif.md §6 "Window glass and environment
    /// maps").
    #[test]
    fn window_env_needs_both_flags_a_lit_shader_and_a_cube() {
        assert!(window_env_flags(0x8220_0180));
        assert!(!window_env_flags(1 << 7));
        assert!(!window_env_flags(1 << 21));
        let aids = crate::env_map::EnvAids::default();
        let env = |p: &Props| shape_env(p, &aids).map(|e| (e.cube, e.mask, e.scale, e.window));
        let glass = Props { env: Some(WINDOW_FLAGS), textures: texture_set(&["vglass01.dds", "vglass01_n.dds", "", "", "shinybright_e.dds", ""]), ..default() };
        assert_eq!(env(&glass), Some(("shinybright_e.dds".to_string(), None, 1.0, true)));
        let masked = Props { textures: texture_set(&["a.dds", "a_n.dds", "", "", "b_e.dds", "a_m.dds"]), ..glass.clone() };
        assert_eq!(env(&masked), Some(("b_e.dds".to_string(), Some("a_m.dds".to_string()), 1.0, true)));
        assert_eq!(env(&Props { textures: texture_set(&["a.dds", "a_n.dds"]), ..glass.clone() }), None);
        assert_eq!(env(&Props { env: Some(crate::env_map::EnvFlags { scale: 0.0, ..WINDOW_FLAGS }), ..glass.clone() }), None);
        assert_eq!(env(&Props { unlit: true, ..glass.clone() }), None);
        assert_eq!(env(&Props { env: None, ..glass }), None);
    }

    /// Environment_Mapping alone (bit 7) is enough since #2841; the window flag stays a
    /// property of its own, and the debug switch NVF_PLAY_DEBUG_ENV_NON_WINDOW=0 brings
    /// back #2669's set. A shape with the flag but no cube in slot 4 (`winebottle01.nif`
    /// in the data survey), an unlit one, one at scale 0 and object LOD block geometry do
    /// not reflect. Fixtures written for the test (a Sunset Sarsaparilla-like bottle:
    /// scale 1.2, a slot-5 mask, Env Map Fade).
    #[test]
    fn env_needs_bit_7_a_lit_shader_and_a_cube_window_or_not() {
        assert!(env_flags(1 << 7));
        assert!(env_flags(0x8220_0180));
        assert!(!env_flags(1 << 21));
        assert!(!env_flags(0x8000_0001));
        let aids = crate::env_map::EnvAids::default();
        let flags = crate::env_map::EnvFlags { scale: 1.2, window: false, light_fade: true };
        let bottle = Props { env: Some(flags), textures: texture_set(&["bottle.dds", "bottle_n.dds", "", "", "textures\\effects\\shinybright_e.dds", "bottle_m.dds"]), ..default() };
        assert_eq!(
            shape_env(&bottle, &aids),
            Some(crate::env_map::ShapeEnv { cube: "textures\\effects\\shinybright_e.dds".into(), mask: Some("bottle_m.dds".into()), scale: 1.2, window: false, light_fade: true })
        );
        let no_cube = Props { textures: texture_set(&["wine.dds", "wine_n.dds", "", "", "", "wine_m.dds"]), ..bottle.clone() };
        assert_eq!(shape_env(&no_cube, &aids), None);
        assert_eq!(shape_env(&Props { textures: texture_set(&["a.dds"]), ..bottle.clone() }, &aids), None);
        assert_eq!(shape_env(&Props { unlit: true, ..bottle.clone() }, &aids), None);
        assert_eq!(shape_env(&Props { lod_building: true, ..bottle.clone() }, &aids), None);
        let lod_window = Props { lod_building: true, env: Some(crate::env_map::EnvFlags { window: true, ..flags }), ..bottle.clone() };
        assert!(shape_env(&lod_window, &aids).is_some(), "window shapes keep #2462's gate on LOD block geometry");
        assert_eq!(shape_env(&Props { env: Some(crate::env_map::EnvFlags { scale: 0.0, ..flags }), ..bottle.clone() }, &aids), None);
        let before = crate::env_map::EnvAids { non_window: false, ..aids };
        assert_eq!(shape_env(&bottle, &before), None);
        let window = Props { env: Some(crate::env_map::EnvFlags { window: true, ..flags }), ..bottle };
        assert!(shape_env(&window, &before).is_some_and(|e| e.window));
    }

    /// Every texture loads (a placeholder handle), with an alpha channel.
    struct AllTextures;
    impl TextureSource for AllTextures {
        fn texture(&mut self, _: &str, _: bool) -> Option<Handle<Image>> {
            Some(Handle::default())
        }
        fn lacks_alpha(&self, _: &Handle<Image>) -> bool {
            false
        }
        fn alpha_is_mask(&self, _: &str) -> bool {
            false
        }
        fn alpha_is_opaque(&self, _: &str) -> bool {
            false
        }
        fn resolve(&self, raw_path: &str) -> String {
            fnv_vfs::texture_path(raw_path)
        }
        fn pbr_set(&mut self, _: &str) -> Option<crate::authored::PbrSet> {
            None
        }
        fn authored_on(&self) -> bool {
            false
        }
    }

    /// [`AllTextures`] whose base textures' alpha is a mask (a blended cut-out's).
    struct AllMaskTextures;
    impl TextureSource for AllMaskTextures {
        fn texture(&mut self, _: &str, _: bool) -> Option<Handle<Image>> {
            Some(Handle::default())
        }
        fn lacks_alpha(&self, _: &Handle<Image>) -> bool {
            false
        }
        fn alpha_is_mask(&self, _: &str) -> bool {
            true
        }
        fn alpha_is_opaque(&self, _: &str) -> bool {
            false
        }
        fn resolve(&self, raw_path: &str) -> String {
            fnv_vfs::texture_path(raw_path)
        }
        fn pbr_set(&mut self, _: &str) -> Option<crate::authored::PbrSet> {
            None
        }
        fn authored_on(&self) -> bool {
            false
        }
    }

    /// The material of a blended reflecting shape: a window one is drawn premultiplied
    /// (#2669), one without the window flag keeps its blend (the reflection weighted by its
    /// alpha); a shape with the flag but no cube is drawn as before. Blend SRC_ALPHA /
    /// INV_SRC_ALPHA without a test (0x10ED, as `vwindow01.nif`); fixtures written for the
    /// test. The default switches are passed in, so `NVF_PLAY_DEBUG_ENV_*` set on the
    /// machine does not change the result.
    #[test]
    fn blended_env_shapes_premultiply_only_windows() {
        let mut tri = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        tri.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        tri.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        let flags = crate::env_map::EnvFlags { scale: 1.0, window: false, light_fade: false };
        let glass = Props {
            alpha: Some((0x10ED, 0)),
            shader: true,
            has_properties: true,
            env: Some(flags),
            textures: texture_set(&["glass.dds", "", "", "", "textures\\effects\\shinybright_e.dds", "glass_m.dds"]),
            ..default()
        };
        let d = crate::env_map::EnvAids::default();
        let (plain, _) = shape_material_inner(&Props { env: None, ..glass.clone() }, &mut tri.clone(), &mut AllTextures, Tangents::Generate, &d);
        assert!(matches!(plain.base.alpha_mode, AlphaMode::Blend), "the fixture is blended");
        let (m, _) = shape_material_inner(&glass, &mut tri.clone(), &mut AllTextures, Tangents::Generate, &d);
        assert!(matches!(m.base.alpha_mode, AlphaMode::Blend));
        assert_eq!(m.extension.env, Vec4::new(1.0, 1.0, 0.0, 0.0), "scale 1, the slot-5 mask, no distance fade");
        assert_eq!(m.extension.env_how.z, 0.0, "not a window");
        assert!(m.extension.env_cube.is_some() && m.extension.env_mask.is_some());
        let window = Props { env: Some(crate::env_map::EnvFlags { window: true, ..flags }), ..glass.clone() };
        let (w, _) = shape_material_inner(&window, &mut tri.clone(), &mut AllTextures, Tangents::Generate, &d);
        assert!(matches!(w.base.alpha_mode, AlphaMode::Premultiplied));
        assert_eq!(w.extension.env_how.z, 1.0, "a window");
        let no_cube = Props { textures: texture_set(&["glass.dds", "", "", "", "", "glass_m.dds"]), ..glass.clone() };
        let (n, _) = shape_material_inner(&no_cube, &mut tri.clone(), &mut AllTextures, Tangents::Generate, &d);
        assert!(matches!(n.base.alpha_mode, AlphaMode::Blend));
        assert_eq!((n.extension.env, n.extension.env_how), (Vec4::ZERO, Vec4::ZERO));
        assert!(n.extension.env_cube.is_none() && n.extension.env_mask.is_none());
        let (none, _) = shape_material_inner(&glass, &mut tri.clone(), &mut NoTextures, Tangents::Generate, &d);
        assert_eq!(none.extension.env, Vec4::ZERO, "a cube that does not load: no reflection");
        // A blended cut-out (blend with a test below BLEND_TEST_CUT, 0x12ED, and a base
        // texture whose alpha is a mask) stays blended even with the window flag (main's
        // rule since #2669), and still reflects.
        let cutout_window = Props { alpha: Some((0x12ED, 0)), ..window.clone() };
        let (c, _) = shape_material_inner(&cutout_window, &mut tri.clone(), &mut AllMaskTextures, Tangents::Generate, &d);
        assert!(matches!(c.base.alpha_mode, AlphaMode::Blend), "a blended cut-out is not premultiplied");
        assert!(c.extension.blended_cutout.is_some());
        assert_eq!(c.extension.env_how.z, 1.0);
        let (wc, _) = shape_material_inner(&cutout_window, &mut tri, &mut AllTextures, Tangents::Generate, &d);
        assert!(matches!(wc.base.alpha_mode, AlphaMode::Premultiplied), "the same tested blend over a soft alpha is premultiplied");
    }

    #[test]
    fn fire_period_needs_its_flag() {
        // The Incinerator's shell: flags 0x82018101, period 10.
        assert!((fire_period(0x8201_8101, Some(10)) - 1.0).abs() < 1e-6);
        // Refraction without Fire Refraction, or no period: still.
        assert_eq!(fire_period(1 << 15, Some(10)), 0.0);
        assert_eq!(fire_period(0x8201_8101, Some(0)), 0.0);
        assert_eq!(fire_period(0x8201_8101, None), 0.0);
        // A negative period scrolls the other way.
        assert!((fire_period(0x8201_8101, Some(-5)) + 0.5).abs() < 1e-6);
    }

    #[test]
    fn blend_modes() {
        let flags = |blend: bool, src: i64, dst: i64, test: bool| (blend as i64) | (src << 1) | (dst << 5) | ((test as i64) << 9);
        assert!(matches!(alpha_mode(flags(true, 6, 7, false), 0), AlphaMode::Blend));
        assert!(matches!(alpha_mode(flags(true, 6, 0, false), 0), AlphaMode::Add));
        assert!(matches!(alpha_mode(flags(true, 4, 1, false), 0), AlphaMode::Multiply));
        assert!(matches!(alpha_mode(flags(false, 6, 7, true), 128), AlphaMode::Mask(t) if (t - 128.0 / 255.0).abs() < 1e-6));
        assert!(matches!(alpha_mode(flags(false, 6, 7, false), 0), AlphaMode::Opaque));
        assert!(matches!(alpha_mode(flags(true, 6, 7, true), 128), AlphaMode::Mask(_)));
        // Blend + test with a low threshold blends (dirty glass).
        assert!(matches!(alpha_mode(flags(true, 6, 7, true), 0), AlphaMode::Blend));
        assert!(matches!(alpha_mode(flags(true, 6, 7, true), 63), AlphaMode::Blend));
        assert!(matches!(alpha_mode(flags(true, 6, 0, true), 128), AlphaMode::Add));
        assert!(is_editor_marker(Some("EditorMarker01")) && is_editor_marker(Some("editormarker")));
        assert!(!is_editor_marker(Some("C_EditorMarker01")) && !is_editor_marker(None));
        // nif.xml default flags 4844 = 0x12EC: blend off, test on, TEST_GREATER.
        assert!(matches!(alpha_mode(4844, 128), AlphaMode::Mask(t) if (t - 128.5 / 255.0).abs() < 1e-6));
        // The poster marquee glass (0x12ED, threshold 0) blends.
        assert!(matches!(alpha_mode(0x12ED, 0), AlphaMode::Blend));
        // Test only, TEST_GREATER with threshold 0: alpha 0 fails.
        assert!(matches!(alpha_mode(0x12EC, 0), AlphaMode::Mask(t) if t > 0.0 && t < 1.0 / 255.0));
        // GREATER_EQUAL keeps the threshold itself.
        assert!(matches!(alpha_mode(0x200 | (6 << 10), 128), AlphaMode::Mask(t) if (t - 128.0 / 255.0).abs() < 1e-6));
    }

    #[test]
    fn lod_building_is_alpha_tested() {
        // Object LOD blocks: the flag and no NiAlphaProperty.
        let block = Props { shader: true, has_properties: true, lod_building: true, ..Default::default() };
        assert!(matches!(shape_alpha_mode(&block), AlphaMode::Mask(t) if (t - 128.5 / 255.0).abs() < 1e-6));
        // A NiAlphaProperty wins; without the flag nothing is cut.
        assert!(matches!(shape_alpha_mode(&Props { alpha: Some((0x12ED, 0)), ..block.clone() }), AlphaMode::Blend));
        assert!(matches!(shape_alpha_mode(&Props { lod_building: false, ..block }), AlphaMode::Opaque));
        assert!(matches!(shape_alpha_mode(&Props { alpha: Some((4844, 64)), ..Default::default() }), AlphaMode::Mask(_)));
        // An unlit shape with "Dynamic_Alpha", faded by a clip and without a NiAlphaProperty
        // blends (the Fat Man's mushroom card); otherwise it stays opaque.
        let card = Props { shader: true, has_properties: true, unlit: true, dynamic_alpha: true, animated_alpha: true, ..Default::default() };
        assert!(matches!(shape_alpha_mode(&card), AlphaMode::Blend));
        assert!(matches!(shape_alpha_mode(&Props { animated_alpha: false, ..card.clone() }), AlphaMode::Opaque));
        assert!(matches!(shape_alpha_mode(&Props { dynamic_alpha: false, ..card.clone() }), AlphaMode::Opaque));
        assert!(matches!(shape_alpha_mode(&Props { unlit: false, ..card.clone() }), AlphaMode::Opaque));
        // An actor's screen whose material its clips fade is shown or not by its alpha
        // (Mr. House's screens, #1542).
        let screen = Props { animated_alpha: false, alpha_gated: true, ..card };
        assert!(matches!(shape_alpha_mode(&screen), AlphaMode::Mask(t) if t == 0.5));
        assert!(matches!(shape_alpha_mode(&Props { dynamic_alpha: false, ..screen }), AlphaMode::Opaque));
    }

    #[test]
    fn a_baked_shadow_patch_is_black_vertices_with_a_varying_alpha_that_blend() {
        // The card table's patch: alpha 0 along the outline, 0.7 inside.
        let patch = [[0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.7], [0.0, 0.0, 0.0, 0.0]];
        assert!(shadow_colours(&patch));
        assert!(!shadow_colours(&[]), "no colours");
        assert!(!shadow_colours(&[[0.0, 0.0, 0.0, 1.0]; 3]), "alpha 1 everywhere: nothing to show");
        assert!(!shadow_colours(&[[0.0, 0.0, 0.0, 0.0]; 3]), "alpha 0 everywhere: nothing drawn");
        assert!(!shadow_colours(&[[0.0, 0.0, 0.0, 0.0], [0.9, 0.5, 0.1, 0.7]]), "a coloured glow, not a shadow");
        assert!(shadow_colours(&[[0.01, 0.0, 0.02, 0.0], [0.0, 0.0, 0.0, 0.5]]), "within SHADOW_BLACK");
        // Garbage alphas are cleaned to 1 first (`clean_vertex_color`).
        assert!(!shadow_colours(&[[0.0, 0.0, 0.0, f32::NAN], [0.0, 0.0, 0.0, 1.0]]));
        // 0x10ED: blending, SRC_ALPHA / INV_SRC_ALPHA, no test.
        assert!(blends_over(0x10ED));
        assert!(!blends_over(0x10EC), "blending off");
        assert!(!blends_over(0x100D), "SRC_ALPHA / ONE: additive");
        // The shape takes its vertex alpha though its shader does not set the flag.
        let props = Props { unlit: true, alpha: Some((0x10ED, 0)), shadow_patch: true, vertex_alpha: true, textures: vec![String::new()], ..Default::default() };
        assert!(matches!(shape_alpha_mode(&props), AlphaMode::Blend));
        assert_eq!(clean_vertex_color([0.0, 0.0, 0.0, 0.7], props.vertex_alpha)[3], 0.7);
        assert_eq!(clean_vertex_color([0.0, 0.0, 0.0, 0.7], false)[3], 1.0, "without it: the opaque black patch");
    }

    #[test]
    fn falloff_only_when_blended() {
        let props = Props { falloff: Falloff::from_fields(0.985, 0.174, 1.0, 0.0), ..Default::default() };
        assert!(shape_falloff(&props, AlphaMode::Add).is_some());
        assert!(shape_falloff(&props, AlphaMode::Blend).is_some());
        assert!(shape_falloff(&props, AlphaMode::Opaque).is_none());
        assert!(shape_falloff(&props, AlphaMode::Mask(0.5)).is_none());
        assert!(shape_falloff(&Props::default(), AlphaMode::Add).is_none());
    }

    #[test]
    fn falloff_only_with_the_flag() {
        // Shader flags as stored: the Saloon lamp beam (bit 6 set), the neon tubes of the
        // Saloon's sign and the Old Town shadow strips (bit 6 clear).
        let (beam, neon, strip) = (0xa200_0148, 0x8200_0100, 0x8e00_0008);
        assert!(uses_falloff(beam) && !uses_falloff(neon) && !uses_falloff(strip));
        let authored = [Some(0.985), Some(0.174), Some(1.0), Some(0.0)];
        let defaults = [Some(1.0), Some(0.0), Some(1.0), Some(0.0)];
        assert_eq!(property_falloff(beam, authored), Falloff::from_fields(0.985, 0.174, 1.0, 0.0));
        // With the flag the default values are a falloff too: opacity is the cosine.
        assert!((property_falloff(beam, defaults).unwrap().opacity(0.3) - 0.3).abs() < 1e-6);
        // Without it nothing fades, whether the fields hold the defaults or other values.
        assert_eq!(property_falloff(neon, defaults), None);
        assert_eq!(property_falloff(strip, defaults), None);
        assert_eq!(property_falloff(neon, authored), None);
        // The flag does not rescue an empty range, constant full opacity or absent fields.
        assert_eq!(property_falloff(beam, [Some(0.0); 4]), None);
        assert_eq!(property_falloff(beam, [Some(1.0), Some(0.0), Some(1.0), Some(1.0)]), None);
        assert_eq!(property_falloff(beam, [None; 4]), None);
        assert_eq!(property_falloff(beam, [Some(1.0), None, Some(1.0), Some(0.0)]), None);
    }

    #[test]
    fn unlit_emissive_tints_by_hue() {
        // Dead Money's lantern glow card: warm (0.85, 0.65, 0.47) x 1 tints a white texture.
        let t = unlit_emissive([0.847, 0.651, 0.475], 1.0);
        assert!((t[0] - 1.0).abs() < 1e-6 && t[1] < 0.6 && t[2] < 0.3, "{t:?}");
        // Grey below 1 changes nothing; white x 25 is 25 times the texture.
        assert_eq!(unlit_emissive([0.1; 3], 1.25), [1.0; 3]);
        assert_eq!(unlit_emissive([1.0; 3], 25.0), [25.0; 3]);
        // A multiplier of 0 keeps the surface colour.
        assert_eq!(unlit_emissive([0.5; 3], 0.0), [1.0; 3]);
        // An additive unlit shape takes the brightness in display values (nif.md §6
        // "Additive blend"): the Big MT's sky beams, white × 10, are 10^2.2 in linear, so
        // the shader's display value is 10 × the texture; alpha-blended ones keep 10.
        let beam = Props { unlit: true, emissive: Some([1.0; 3]), emissive_mult: Some(10.0), ..Default::default() };
        assert!((shape_emissive(&beam, AlphaMode::Add).unwrap().red - 10f32.powf(2.2)).abs() < 1e-2);
        assert!((shape_emissive(&beam, AlphaMode::Blend).unwrap().red - 10.0).abs() < 1e-5);
        // The hue stays a display colour raised to the gamma.
        let warm = Props { unlit: true, emissive: Some([1.0, 0.5, 0.0]), emissive_mult: Some(2.0), ..Default::default() };
        let w = shape_emissive(&warm, AlphaMode::Add).unwrap();
        assert!((w.red - 2f32.powf(2.2)).abs() < 1e-4 && (w.green - (0.5f32 * 2.0).powf(2.2)).abs() < 1e-4 && w.blue == 0.0);
    }

    #[test]
    fn an_unlit_pictures_tracks_multiply_the_stored_multiplier_not_the_held_one() {
        let picture = Props { unlit: true, emissive: Some([1.0; 3]), emissive_mult: Some(30.0), ..Default::default() };
        assert_eq!(shape_emissive_mult(&picture, AlphaMode::Opaque), 1.0);
        assert_eq!(shape_emissive(&picture, AlphaMode::Opaque).unwrap(), LinearRgba::WHITE);
        assert_eq!(picture.emissive_parts(), ([1.0; 3], 30.0));
        let gated = Props { dynamic_alpha: true, alpha_gated: true, ..picture.clone() };
        assert_eq!(gated.emissive_parts(), ([1.0; 3], 30.0));
        assert_eq!(shape_emissive(&gated, shape_alpha_mode(&gated)).unwrap(), LinearRgba::WHITE);

        let blended = Props { alpha: Some((0x12ed, 0)), emissive_mult: Some(40.0), ..picture.clone() };
        assert_eq!(shape_emissive_mult(&blended, AlphaMode::Blend), 40.0);
        assert_eq!(blended.emissive_parts().1, 40.0);
        assert_eq!(shape_emissive(&blended, AlphaMode::Blend).unwrap().red, 40.0);
        let additive = Props { alpha: Some((1, 0)), ..blended };
        assert_eq!(shape_emissive_mult(&additive, AlphaMode::Add), 40.0);
        assert_eq!(additive.emissive_parts().1, 40.0);
        assert!((shape_emissive(&additive, AlphaMode::Add).unwrap().red - 40f32.powf(2.2)).abs() < 1e-2);

        let neon = Props { external_emittance: true, emissive_mult: Some(25.0), ..picture };
        assert_eq!(shape_emissive_mult(&neon, AlphaMode::Opaque), 25.0);
        assert_eq!(neon.emissive_parts().1, 25.0);
        assert_eq!(shape_emissive(&neon, AlphaMode::Opaque).unwrap().red, 25.0);
    }

    #[test]
    fn a_picture_holds_its_colour_times_its_multiplier() {
        // A Securitron's screen: grey 0.05 x 30 shows its texture, not a twentieth of it.
        let face = Props { unlit: true, emissive: Some([0.05; 3]), emissive_mult: Some(30.0), ..Default::default() };
        // Its idle track keys 0.032 x 30, about 1, on the stored multiplier.
        assert_eq!(face.emissive_parts().1, 30.0);
        assert!((shape_emissive(&face, AlphaMode::Opaque).unwrap().red - 1.0).abs() < 1e-5);
        // Under 1 it is left alone; with the flag it is no picture.
        let dim = Props { emissive_mult: Some(4.0), ..face.clone() };
        assert_eq!(shape_emissive_mult(&dim, AlphaMode::Opaque), 4.0);
        let neon = Props { external_emittance: true, ..face };
        assert_eq!(shape_emissive_mult(&neon, AlphaMode::Opaque), 30.0);
    }

    #[test]
    fn unlit_cutouts_take_the_plain_path_and_no_scale() {
        let no_scale = Vec4::new(1.0, 0.0, 0.0, 0.0);
        let cutout_rule = Vec4::new(1.0, 1.0, 0.0, 0.0);
        let picture = Props { unlit: true, emissive_mult: Some(30.0), ..Default::default() };
        for mode in [AlphaMode::Opaque, AlphaMode::Blend, AlphaMode::Add] {
            assert_eq!(shape_emissive_rule(&picture, mode), no_scale);
        }
        // A gated screen with no authored alpha is a picture, not a cut-out.
        assert_eq!(shape_emissive_rule(&picture, AlphaMode::Mask(0.5)), no_scale);
        let sign = Props { external_emittance: true, emissive_mult: Some(4.0), alpha: Some((0x12EC, 128)), ..picture };
        for mult in [4.0, 7.5, 14.0] {
            let sign = Props { emissive_mult: Some(mult), ..sign.clone() };
            assert_eq!(shape_emissive_rule(&sign, AlphaMode::Mask(0.5)), cutout_rule);
            assert_eq!(shape_emissive_mult(&sign, AlphaMode::Mask(0.5)), mult);
            let no_flag = Props { external_emittance: false, ..sign };
            assert_eq!(shape_emissive_rule(&no_flag, AlphaMode::Mask(0.5)), cutout_rule);
        }
        for mode in [AlphaMode::Opaque, AlphaMode::Blend, AlphaMode::Add] {
            assert_eq!(shape_emissive_rule(&sign, mode), Vec4::ZERO);
        }
    }

    #[test]
    fn lit_shapes_keep_their_emissive_rules() {
        for external_emittance in [false, true] {
            let lit = Props { external_emittance, emissive: Some([1.0; 3]), emissive_mult: Some(30.0), ..Default::default() };
            for mode in [AlphaMode::Opaque, AlphaMode::Mask(0.5), AlphaMode::Blend, AlphaMode::Add] {
                assert_eq!(shape_emissive_mult(&lit, mode), 30.0);
                assert_eq!(shape_emissive_rule(&lit, mode), Vec4::ZERO);
            }
            assert_eq!(lit.emissive_parts().1, 30.0);
            assert_eq!(shape_emissive(&lit, AlphaMode::Opaque).unwrap().red, 30.0);
            assert_eq!(shape_emissive_rule(&Props { emissive_mult: Some(4.0), ..lit }, AlphaMode::Mask(0.5)), Vec4::ZERO);
        }
    }

    #[test]
    fn multiply_blended_emissive() {
        // Rex's skull cap (rex_skullcap.nif `##SkullCap:0`): NiAlphaProperty flags 4163
        // (blend, source ZERO, destination SRC_COLOR), emissive 0.19 × 12.
        let cap = Props { alpha: Some((4163, 0)), emissive: Some([0.190_283_18; 3]), emissive_mult: Some(12.0), ..Default::default() };
        let mode = alpha_mode(4163, 0);
        assert!(matches!(mode, AlphaMode::Multiply));
        // Its light is a display-value factor of 2.28 on the texture, 2.28^2.2 in linear.
        let e = shape_emissive(&cap, mode).unwrap();
        assert!((e.red - 2.283_398_f32.powf(2.2)).abs() < 1e-3 && e.red == e.green && e.green == e.blue);
        // An additive shape forms its light in display values too (nif.md §6 "Additive
        // blend"); one that is alpha-blended or opaque keeps the plain product.
        assert!((shape_emissive(&cap, AlphaMode::Add).unwrap().red - e.red).abs() < 1e-6);
        assert!((shape_emissive(&cap, AlphaMode::Blend).unwrap().red - 2.283_398).abs() < 1e-5);
        assert!((shape_emissive(&cap, AlphaMode::Opaque).unwrap().red - 2.283_398).abs() < 1e-5);
        // A black emissive colour emits nothing, whatever the multiplier.
        let dark = Props { emissive: Some([0.0; 3]), emissive_mult: Some(10.0), ..Default::default() };
        assert!(shape_emissive(&dark, mode).is_none());
        assert!(shape_emissive(&Props::default(), mode).is_none());
    }

    #[test]
    fn helper_shapes() {
        // No properties at all: skipped in a model that shades other shapes (the lounge
        // chair's marker figures), drawn in one that shades none.
        assert!(Props::default().is_helper(true) && Props::default().is_bare());
        assert!(!Props::default().is_helper(false));
        // Material only (bone boxes, collision proxies): skipped.
        let material_only = Props { has_properties: true, ..Default::default() };
        assert!(material_only.is_helper(false) && material_only.is_helper(true) && !material_only.is_bare());
        // A shader property, or a texture from NiTexturingProperty: drawn.
        assert!(!Props { shader: true, ..material_only.clone() }.is_helper(true));
        assert!(!Props { textures: vec!["textures\\a.dds".into()], ..material_only }.is_helper(true));
    }

    // A synthetic NIF (20.2.0.7, user version 11, BS version 34) in the layout of
    // `crates/fnv-nif/tests/synthetic.rs` and `schemas/nif`; no game data.
    use super::made_nif::{floats, nif, words};

    /// NiAVObject fields: name (string index), no extra data or controller, flags 14,
    /// identity transform, these properties, no collision.
    fn av_object(name: u32, properties: &[u32]) -> Vec<u8> {
        let mut out = vec![];
        words(&mut out, &[name, 0, u32::MAX, 14]);
        floats(&mut out, &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0]);
        words(&mut out, &[properties.len() as u32]);
        words(&mut out, properties);
        words(&mut out, &[u32::MAX]);
        out
    }

    fn node(name: u32, children: &[u32]) -> Vec<u8> {
        let mut out = av_object(name, &[]);
        words(&mut out, &[children.len() as u32]);
        words(&mut out, children);
        words(&mut out, &[0]); // effects
        out
    }

    fn strips(name: u32, properties: &[u32], data: u32) -> Vec<u8> {
        let mut out = av_object(name, properties);
        words(&mut out, &[data, u32::MAX, 0, u32::MAX]); // data, skin, materials, active
        out.push(0); // material needs update
        out
    }

    /// A 100 × 100 square at height `z`, one strip of two triangles.
    fn square(z: f32) -> Vec<u8> {
        let mut out = vec![];
        words(&mut out, &[0]); // group id
        out.extend(4u16.to_le_bytes());
        out.extend([0, 0, 1]); // keep, compress, has vertices
        floats(&mut out, &[0.0, 0.0, z, 100.0, 0.0, z, 0.0, 100.0, z, 100.0, 100.0, z]);
        out.extend(1u16.to_le_bytes()); // one UV set
        out.push(0); // no normals
        floats(&mut out, &[50.0, 50.0, z, 71.0]); // bounding sphere
        out.push(0); // no vertex colours
        floats(&mut out, &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        out.extend(0u16.to_le_bytes()); // consistency
        words(&mut out, &[u32::MAX]); // additional data
        for v in [2u16, 1, 4] {
            out.extend(v.to_le_bytes()); // triangles, strips, strip length
        }
        out.push(1); // has points
        for v in [0u16, 1, 2, 3] {
            out.extend(v.to_le_bytes());
        }
        out
    }

    /// BSShaderPPLightingProperty: no name, smooth shading, SHADER_DEFAULT, no flags,
    /// wrap clamp mode, no texture set.
    fn lit_shader() -> Vec<u8> {
        let mut out = vec![];
        words(&mut out, &[u32::MAX, 0, u32::MAX]);
        out.extend(1u16.to_le_bytes());
        words(&mut out, &[1, 0, 0]);
        floats(&mut out, &[1.0]);
        words(&mut out, &[3, u32::MAX]);
        floats(&mut out, &[0.0]);
        words(&mut out, &[0]);
        floats(&mut out, &[4.0, 1.0]);
        out
    }

    /// BSShaderPPLightingProperty as [`lit_shader`], with these shader flags,
    /// `environment_map_scale` and its texture set at block `set`.
    fn lit_shader_env(flags: u32, scale: f32, set: u32) -> Vec<u8> {
        let mut out = vec![];
        words(&mut out, &[u32::MAX, 0, u32::MAX]);
        out.extend(1u16.to_le_bytes());
        words(&mut out, &[1, flags, 0]);
        floats(&mut out, &[scale]);
        words(&mut out, &[3, set]);
        floats(&mut out, &[0.0]);
        words(&mut out, &[0]);
        floats(&mut out, &[4.0, 1.0]);
        out
    }

    /// BSShaderTextureSet: these paths, slot by slot.
    fn texture_set_block(paths: &[&str]) -> Vec<u8> {
        let mut out = vec![];
        words(&mut out, &[paths.len() as u32]);
        for p in paths {
            words(&mut out, &[p.len() as u32]);
            out.extend(p.as_bytes());
        }
        out
    }

    /// The properties of the one shape of a synthetic model whose lit shader has `flags`
    /// and scale 1.5, with a full texture set (diffuse, normal, cube in slot 4, mask in
    /// slot 5), read through `shape_props` / `apply_property`.
    fn env_model_props(flags: u32) -> Props {
        let schema = fnv_nif::Schema::load_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/nif")).unwrap();
        let bytes = nif(
            &[
                ("BSFadeNode", node(0, &[1])),
                ("NiTriStrips", strips(1, &[2], 4)),
                ("BSShaderPPLightingProperty", lit_shader_env(flags, 1.5, 3)),
                ("BSShaderTextureSet", texture_set_block(&["textures\\b.dds", "textures\\b_n.dds", "", "", "textures\\effects\\chrome_e.dds", "textures\\b_m.dds"])),
                ("NiTriStripsData", square(0.0)),
            ],
            &["Bottle", "Bottle:0"],
        );
        let file = NifFile::parse(&bytes, Some(&schema)).unwrap();
        let dec = Decoded::new(&file, &bytes, &schema);
        let objects = scene_objects(&file, &dec);
        let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
        let shape = objects.iter().find(|o| !o.is_node).expect("the shape");
        shape_props(&file, &dec, shape, &by_block)
    }

    /// #2841's gate end to end: a lit shader property with Environment_Mapping (bit 7)
    /// alone, decoded from a file, gives a material that binds the slot-4 cube and the
    /// slot-5 mask at the property's scale; with the window flag too it is a window shape;
    /// without bit 7 nothing is bound. Restoring the both-bits gate fails the first case.
    /// Fixtures written for the test.
    #[test]
    fn decoded_bit_7_shader_binds_cube_mask_and_scale() {
        let props = env_model_props(1 << 7);
        assert_eq!(props.env, Some(crate::env_map::EnvFlags { scale: 1.5, window: false, light_fade: false }));
        let mut tri = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        tri.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        tri.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        let d = crate::env_map::EnvAids::default();
        let (m, _) = shape_material_inner(&props, &mut tri.clone(), &mut AllTextures, Tangents::Generate, &d);
        assert!(m.extension.env_cube.is_some() && m.extension.env_mask.is_some(), "cube and mask bound");
        assert_eq!((m.extension.env.x, m.extension.env.y), (1.5, 1.0), "the scale and the slot-5 mask");
        assert_eq!((m.extension.env_how.z, m.extension.env_how.w), (0.0, 2.0), "a reflecting shape, not a window: the vertex normal");
        let window = env_model_props((1 << 7) | (1 << 21));
        let (w, _) = shape_material_inner(&window, &mut tri.clone(), &mut AllTextures, Tangents::Generate, &d);
        assert_eq!(w.extension.env_how.z, 1.0, "both flags: a window shape");
        let plain = env_model_props(1 << 21);
        assert_eq!(plain.env, None);
        let (n, _) = shape_material_inner(&plain, &mut tri, &mut AllTextures, Tangents::Generate, &d);
        assert!(n.extension.env_cube.is_none() && n.extension.env_mask.is_none());
        assert_eq!(n.extension.env, Vec4::ZERO);
    }

    /// The normal-alpha debug variant binds the game's own normal map as the mask (read by
    /// its alpha), so an authored set that replaces the drawn normal map later
    /// (`crate::authored::apply`) cannot change what the comparison reads. Fixtures written
    /// for the test; the mesh keeps its own tangents.
    #[test]
    fn normal_alpha_variant_binds_the_legacy_normal_as_mask() {
        let mut tri = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        tri.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        tri.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        tri.insert_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0f32, 0.0, 0.0, 1.0]; 3]);
        let whiskey = Props {
            shader: true,
            has_properties: true,
            env: Some(crate::env_map::EnvFlags { scale: 1.0, window: false, light_fade: false }),
            textures: texture_set(&["w.dds", "w_n.dds", "", "", "textures\\effects\\shinydull_e.dds", ""]),
            ..default()
        };
        let variant = crate::env_map::EnvAids { no_mask: crate::env_map::NoMask::NormalAlpha, ..default() };
        let (m, _) = shape_material_inner(&whiskey, &mut tri.clone(), &mut AllTextures, Tangents::Keep, &variant);
        assert_eq!(m.extension.env.y, 2.0, "read by alpha");
        assert!(m.base.normal_map_texture.is_some());
        assert_eq!(m.extension.env_mask, m.base.normal_map_texture, "the legacy normal map is the mask");
        let (full, _) = shape_material_inner(&whiskey, &mut tri, &mut AllTextures, Tangents::Keep, &crate::env_map::EnvAids::default());
        assert_eq!((full.extension.env.y, full.extension.env_mask.is_none()), (0.0, true), "by default: no mask, full strength");
    }

    /// The lounge chair's set-up (`furniture\loungechair_tops.nif`): a seat shape with a
    /// shader property, and a marker capsule under its own node with no property at all.
    /// Without `shader` the seat has no property either and block 2 is an empty node.
    fn chair(shader: bool) -> Vec<u8> {
        let (seat_props, block2): (&[u32], (&str, Vec<u8>)) =
            if shader { (&[2], ("BSShaderPPLightingProperty", lit_shader())) } else { (&[], ("NiNode", node(u32::MAX, &[]))) };
        nif(
            &[
                ("BSFadeNode", node(0, &[1, 3])),
                ("NiTriStrips", strips(1, seat_props, 5)),
                block2,
                ("NiNode", node(2, &[4])),
                ("NiTriStrips", strips(3, &[], 6)),
                ("NiTriStripsData", square(5.0)),
                ("NiTriStripsData", square(50.0)),
            ],
            &["Shape01", "Shape01:0", "Capsule01", "Capsule01:0"],
        )
    }

    /// Whether the model counts as shaded, and (is_helper, is_bare) of each shape by name.
    fn chair_helpers(shader: bool) -> (bool, HashMap<String, (bool, bool)>) {
        let schema = fnv_nif::Schema::load_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/nif")).unwrap();
        let bytes = chair(shader);
        let file = NifFile::parse(&bytes, Some(&schema)).unwrap();
        let dec = Decoded::new(&file, &bytes, &schema);
        let objects = scene_objects(&file, &dec);
        let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
        let shaded = shaded_model(&file, &dec);
        let shapes = objects
            .iter()
            .filter(|o| !o.is_node)
            .map(|o| {
                let p = shape_props(&file, &dec, o, &by_block);
                (o.name.clone().unwrap(), (p.is_helper(shaded), p.is_bare()))
            })
            .collect();
        (shaded, shapes)
    }

    #[test]
    fn bare_shapes_beside_shaded_ones_are_editor_geometry() {
        // Issue #2469: the lounge chair's marker capsule is left out, its seat drawn.
        let (shaded, shapes) = chair_helpers(true);
        assert!(shaded);
        assert_eq!(shapes.len(), 2);
        assert_eq!(shapes["Shape01:0"], (false, false), "the seat is drawn");
        assert_eq!(shapes["Capsule01:0"], (true, true), "the bare capsule is editor geometry");
        // A model with no shader property anywhere keeps its bare shapes.
        let (shaded, shapes) = chair_helpers(false);
        assert!(!shaded);
        assert_eq!(shapes["Shape01:0"], (false, true));
        assert_eq!(shapes["Capsule01:0"], (false, true));
    }

    /// BSShaderNoLightingProperty: SHADER_NOLIGHTING, these shader flags, this file name,
    /// the default falloff.
    fn unlit_shader(flags: u32, texture: &str) -> Vec<u8> {
        let mut out = vec![];
        words(&mut out, &[u32::MAX, 0, u32::MAX]);
        out.extend(1u16.to_le_bytes());
        words(&mut out, &[33, flags, 1]);
        floats(&mut out, &[1.0]);
        words(&mut out, &[3, texture.len() as u32]);
        out.extend(texture.as_bytes());
        floats(&mut out, &[1.0, 0.0, 1.0, 0.0]);
        out
    }

    /// NiMaterialProperty: black specular, emissive white × 3, alpha 1.
    fn white_material() -> Vec<u8> {
        let mut out = vec![];
        words(&mut out, &[u32::MAX, 0, u32::MAX]);
        floats(&mut out, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 10.0, 1.0, 3.0]);
        out
    }

    /// NiAlphaProperty with these flags, threshold 0.
    fn alpha_block(flags: u16) -> Vec<u8> {
        let mut out = vec![];
        words(&mut out, &[u32::MAX, 0, u32::MAX]);
        out.extend(flags.to_le_bytes());
        out.push(0);
        out
    }

    /// [`square`] at height 0 with white vertex colours of these alphas.
    fn faded_square(alphas: [f32; 4]) -> Vec<u8> {
        let mut out = square(0.0);
        // The vertex colour switch follows the group id, vertex count, three bytes, four
        // positions, the UV count, the normals switch and the bounding sphere.
        let at = 4 + 2 + 3 + 48 + 2 + 1 + 16;
        assert_eq!(out[at], 0);
        out[at] = 1;
        let mut colours = vec![];
        for a in alphas {
            floats(&mut colours, &[1.0, 1.0, 1.0, a]);
        }
        out.splice(at + 1..at + 1, colours);
        out
    }

    /// The properties and material of the one shape of a synthetic model: an unlit shader
    /// with these flags and file name, emissive white × 3, these vertex alphas, and a
    /// NiAlphaProperty with these flags if any.
    fn faded_model(flags: u32, texture: &str, alphas: [f32; 4], alpha: Option<u16>) -> (Props, NifMaterial) {
        let schema = fnv_nif::Schema::load_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/nif")).unwrap();
        let properties: &[u32] = if alpha.is_some() { &[2, 3, 5] } else { &[2, 3] };
        let mut blocks = vec![
            ("BSFadeNode", node(0, &[1])),
            ("NiTriStrips", strips(1, properties, 4)),
            ("NiMaterialProperty", white_material()),
            ("BSShaderNoLightingProperty", unlit_shader(flags, texture)),
            ("NiTriStripsData", faded_square(alphas)),
        ];
        blocks.extend(alpha.map(|a| ("NiAlphaProperty", alpha_block(a))));
        let bytes = nif(&blocks, &["Shaft", "Shaft:0"]);
        let file = NifFile::parse(&bytes, Some(&schema)).unwrap();
        let dec = Decoded::new(&file, &bytes, &schema);
        assert!(dec.blocks.iter().all(|b| b.is_ok()), "the fixture decodes");
        let objects = scene_objects(&file, &dec);
        let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
        let shape = objects.iter().find(|o| !o.is_node).expect("the shape");
        let props = shape_props(&file, &dec, shape, &by_block);
        let (mut mesh, ..) = build_mesh(dec.get(4).unwrap()).unwrap().unwrap();
        let material = shape_material_with(&props, &mut mesh, &mut AllTextures, Tangents::Generate).0;
        (props, material)
    }

    /// #3443: the cone of light at the end of Lonesome Road's silo lift. An unlit shape
    /// with Vertex_Alpha and the falloff bit, no file name, no NiAlphaProperty and an alpha
    /// that falls across its vertices is blended, as the same shape with the property is;
    /// anything else without the property stays opaque. Fixtures written for the test.
    #[test]
    fn an_unlit_shape_whose_only_picture_is_its_vertex_alpha_is_blended() {
        const CONE: u32 = 0x8200_0048;
        let fading = [1.0, 0.1, 0.0, 0.15];
        let (props, cone) = faded_model(CONE, "", fading, None);
        assert!(props.alpha.is_none());
        assert_eq!(shape_alpha_mode(&props), AlphaMode::Blend);
        assert_eq!(cone.base.alpha_mode, AlphaMode::Blend);
        // Blended it is no picture: its light is the stored multiplier's, and its edges
        // fall off by the view angle, as the same shape's with the property.
        let (_, twin) = faded_model(CONE, "", fading, Some(0x10ED));
        assert_eq!(twin.base.alpha_mode, AlphaMode::Blend);
        assert_eq!(cone.base.emissive, LinearRgba::rgb(3.0, 3.0, 3.0));
        assert_eq!(cone.base.emissive, twin.base.emissive);
        assert_eq!(cone.extension.falloff, twin.extension.falloff);
        assert!(cone.extension.falloff != Vec4::ZERO);
        // The falloff bit is not asked for: some of the dark patches set Vertex_Alpha
        // without it, with and without the decal bits.
        for flags in [0x8200_0008, 0x8E00_0008, 0x8E00_0048] {
            assert_eq!(faded_model(flags, "", fading, None).1.base.alpha_mode, AlphaMode::Blend, "{flags:#x}");
        }
        // No Vertex_Alpha, no alpha below 1, or a texture: a picture, opaque as before.
        assert_eq!(faded_model(0x8200_0040, "", fading, None).1.base.alpha_mode, AlphaMode::Opaque);
        // Dynamic_Alpha alone takes the vertex alpha, and is not the flag this rule asks for;
        // with Vertex_Alpha beside it (a fog shell) it does not stand in the way.
        let (dynamic, opaque) = faded_model(0x8208_0000, "", [0.5; 4], None);
        assert!(dynamic.vertex_alpha && dynamic.dynamic_alpha);
        assert_eq!(opaque.base.alpha_mode, AlphaMode::Opaque);
        assert_eq!(faded_model(0x8208_0008, "", [0.5; 4], None).1.base.alpha_mode, AlphaMode::Blend);
        assert_eq!(faded_model(CONE, "", [1.0; 4], None).1.base.alpha_mode, AlphaMode::Opaque);
        assert_eq!(faded_model(CONE, "map.dds", fading, None).1.base.alpha_mode, AlphaMode::Opaque);
        // Where there is a property its own flags decide.
        assert_eq!(faded_model(CONE, "", fading, Some(0x100D)).1.base.alpha_mode, AlphaMode::Add);
        assert_eq!(faded_model(CONE, "", fading, Some(0)).1.base.alpha_mode, AlphaMode::Opaque);
        // A lit shape has no such rule.
        assert_eq!(shape_alpha_mode(&Props { unlit: false, ..props }), AlphaMode::Opaque);
    }

    #[test]
    fn decal_flags() {
        // utlhallsm1waypipe01.nif: the rust stain `:8` (bits 26, 27 and 31) and the
        // alpha-tested decal `:24` are decals, the wall `:0` and the pipe cards `:1` not.
        assert!(is_decal(0x8c00_0000));
        assert!(is_decal(0x8e00_0101));
        assert!(!is_decal(0x8200_0001));
        assert!(!is_decal(0x8200_0101));
        // Either bit alone (creatures\protectron\decalflame.nif sets only bit 26).
        assert!(is_decal(1 << 26));
        assert!(is_decal(1 << 27));
        assert!(!is_decal(0xffff_ffff & !(1 << 26 | 1 << 27)));
        // The flags reach the material's properties from the shader property.
        assert!(!Props::default().decal);
    }

    /// A BSShaderPPLightingProperty like [`lit_shader`] with `flags` and the texture set at
    /// block `set`.
    fn lit_shader_with(flags: u32, set: u32) -> Vec<u8> {
        let mut out = vec![];
        words(&mut out, &[u32::MAX, 0, u32::MAX]);
        out.extend(1u16.to_le_bytes());
        words(&mut out, &[1, flags, 0]);
        floats(&mut out, &[1.0]);
        words(&mut out, &[3, set]);
        floats(&mut out, &[0.0]);
        words(&mut out, &[0]);
        floats(&mut out, &[4.0, 1.0]);
        out
    }

    /// A BSShaderTextureSet of these slots.
    fn texture_set_bytes(slots: &[&str]) -> Vec<u8> {
        let mut out = vec![];
        words(&mut out, &[slots.len() as u32]);
        for s in slots {
            words(&mut out, &[s.len() as u32]);
            out.extend(s.as_bytes());
        }
        out
    }

    /// Every texture asked for, with its sRGB flag, as handles made on demand.
    #[derive(Default)]
    struct RecordTextures {
        images: Assets<Image>,
        asked: Vec<(String, bool)>,
    }
    impl TextureSource for RecordTextures {
        fn texture(&mut self, raw_path: &str, srgb: bool) -> Option<Handle<Image>> {
            self.asked.push((raw_path.to_string(), srgb));
            Some(self.images.add(Image::default()))
        }
        fn lacks_alpha(&self, _: &Handle<Image>) -> bool {
            true
        }
        fn alpha_is_mask(&self, _: &str) -> bool {
            false
        }
        fn alpha_is_opaque(&self, _: &str) -> bool {
            false
        }
        fn resolve(&self, raw_path: &str) -> String {
            fnv_vfs::texture_path(raw_path)
        }
        fn pbr_set(&mut self, _: &str) -> Option<crate::authored::PbrSet> {
            None
        }
        fn authored_on(&self) -> bool {
            false
        }
    }

    /// #2979: bit 11 decoded from a synthetic BSShaderPPLightingProperty with a texture set
    /// (no game data) marks the shape; its slot 3 is loaded as linear data (sRGB off) and put
    /// on as the depth map. Without the bit the slot is neither loaded nor drawn.
    #[test]
    fn a_decoded_parallax_flag_loads_slot_3_as_linear_height() {
        let schema = fnv_nif::Schema::load_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/nif")).unwrap();
        let slots = [r"textures\test\floor.dds", r"textures\test\floor_n.dds", "", r"textures\test\floor_p.dds", "", ""];
        let build = |flags: u32| {
            let bytes = nif(
                &[
                    ("BSFadeNode", node(0, &[1])),
                    ("NiTriStrips", strips(1, &[2], 4)),
                    ("BSShaderPPLightingProperty", lit_shader_with(flags, 3)),
                    ("BSShaderTextureSet", texture_set_bytes(&slots)),
                    ("NiTriStripsData", square(0.0)),
                ],
                &["Floor", "Floor:0"],
            );
            let file = NifFile::parse(&bytes, Some(&schema)).unwrap();
            let dec = Decoded::new(&file, &bytes, &schema);
            let objects = scene_objects(&file, &dec);
            let by_block: HashMap<usize, &SceneObject> = objects.iter().map(|o| (o.block, o)).collect();
            let shape = objects.iter().find(|o| !o.is_node).unwrap();
            shape_props(&file, &dec, shape, &by_block)
        };
        let flagged = build(1 << 11);
        assert!(flagged.parallax, "bit 11 decoded");
        assert_eq!(game_height_slot(&flagged), Some(slots[3]));
        let plain = build(0);
        assert!(!plain.parallax && game_height_slot(&plain).is_none());
        if crate::authored::game_parallax_depth() <= 0.0 {
            return;
        }
        let mut source = RecordTextures::default();
        let (m, _) = shape_material_with(&flagged, &mut crate::texture::bc5_tests::triangle(), &mut source, Tangents::Generate);
        assert!(source.asked.contains(&(slots[3].to_string(), false)), "slot 3 asked for as linear: {:?}", source.asked);
        assert!(!source.asked.contains(&(slots[3].to_string(), true)), "never as sRGB");
        assert!(m.base.depth_map.is_some());
        let mut source = RecordTextures::default();
        let (m, _) = shape_material_with(&plain, &mut crate::texture::bc5_tests::triangle(), &mut source, Tangents::Generate);
        assert!(!source.asked.iter().any(|(p, _)| p == slots[3]) && m.base.depth_map.is_none(), "unflagged: slot 3 not loaded");
    }

    /// #2979 review: a flagged shape whose tangents cannot be made (no vertex normals) stays
    /// flat and never asks for its slot-3 file.
    #[test]
    fn a_failed_tangent_build_does_not_load_the_height_map() {
        if crate::authored::game_parallax_depth() <= 0.0 {
            return;
        }
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0], [1.0, 0.0], [0.0, 1.0]]);
        let props = Props { textures: vec![r"textures\test\floor.dds".into(), String::new(), String::new(), r"textures\test\floor_p.dds".into()], parallax: true, ..Default::default() };
        let mut source = RecordTextures::default();
        let (m, _) = shape_material_with(&props, &mut mesh, &mut source, Tangents::Generate);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_TANGENT).is_none(), "no tangents could be made");
        assert!(m.base.depth_map.is_none());
        assert!(!source.asked.iter().any(|(p, _)| p.ends_with("floor_p.dds")), "slot 3 not asked for: {:?}", source.asked);
    }

    #[test]
    fn parallax_takes_bit_11_and_slot_3() {
        assert!(parallax_flag(1 << 11) && parallax_flag(0x8200_0901));
        assert!(!parallax_flag(1 << 28), "Parallax_Occlusion alone is not asked");
        assert!(!parallax_flag(0x8200_0101));
        let set = |t: &[&str], parallax: bool| Props { textures: t.iter().map(|s| s.to_string()).collect(), parallax, ..Default::default() };
        assert_eq!(game_height_slot(&set(&["d.dds", "d_n.dds", "", "d_p.dds"], true)), Some("d_p.dds"));
        assert_eq!(game_height_slot(&set(&["d.dds", "d_n.dds", "", "d_p.dds"], false)), None);
        assert_eq!(game_height_slot(&set(&["d.dds", "d_n.dds", "", ""], true)), None);
        assert_eq!(game_height_slot(&set(&["d.dds", "d_n.dds"], true)), None);
        // A record's texture set replaces slot 3 like any other slot.
        let mut p = set(&["d.dds", "d_n.dds", "", "d_p.dds"], true);
        p.use_texture_set(&[None, None, None, Some("other_p.dds".into()), None, None]);
        assert_eq!(game_height_slot(&p), Some(r"textures\other_p.dds"));
    }

    #[test]
    fn glow_map_slot() {
        let set: Vec<String> = ["d.dds", "d_n.dds", "d_g.dds", "", "", ""].iter().map(|s| s.to_string()).collect();
        assert_eq!(glow_map(0x8200_0101, &set).as_deref(), Some("d_g.dds"));
        assert_eq!(glow_map(1 << 10, &set), None); // face gen: slot 2 is a skin map
        assert_eq!(glow_map(1 << 18, &set), None); // hair
        assert_eq!(glow_map(0, &set[..2]), None);
        let empty: Vec<String> = ["d.dds", "d_n.dds", ""].iter().map(|s| s.to_string()).collect();
        assert_eq!(glow_map(0, &empty), None);
    }

    #[test]
    fn vertex_colors_cleaned() {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.5, f32::NAN, 2.0, -3e38], [0.2, 0.3, 0.4, 0.25]]);
        let mut flagged = mesh.clone();
        fix_vertex_colors(&mut mesh, false);
        fix_vertex_colors(&mut flagged, true);
        let get = |m: &Mesh| match m.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Float32x4(c)) => c.clone(),
            _ => panic!(),
        };
        assert_eq!(get(&mesh), vec![[0.5, 1.0, 1.0, 1.0], [0.2, 0.3, 0.4, 1.0]]);
        assert_eq!(get(&flagged), vec![[0.5, 1.0, 1.0, 0.0], [0.2, 0.3, 0.4, 0.25]]);
    }

    #[test]
    fn unlit_dynamic_alpha_shapes_take_their_vertex_alpha() {
        // The dust vortex's unlit sheets: 0xA2080140 (bits 6, 8, 19, 25, 29, 31).
        assert!(uses_vertex_alpha("BSShaderNoLightingProperty", 0xA208_0140));
        assert!(!uses_vertex_alpha("BSShaderPPLightingProperty", 0xA208_0140));
        assert!(uses_vertex_alpha("BSShaderPPLightingProperty", 1 << 3));
        assert!(!uses_vertex_alpha("BSShaderNoLightingProperty", 0x8000_0000));
    }

    #[test]
    fn vertex_alpha_gradient_blends_with_a_cut() {
        let mesh = |alphas: &[f32]| {
            let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, alphas.iter().map(|&a| [0.0, 0.0, 0.0, a]).collect::<Vec<_>>());
            m
        };
        // otbldgdestroy01nv.nif `OTBldgDestroy01NV:5`: untextured black strips along the
        // wall edges, flags 0x12ED (blend, test, GREATER), threshold 128, vertex alpha
        // 0 to 0.75 with the vertex-alpha flag.
        let strip = Props { alpha: Some((0x12ED, 128)), vertex_alpha: true, unlit: true, ..Default::default() };
        let gradient = mesh(&[0.55, 0.0, 0.3, 0.75]);
        assert!(matches!(shape_alpha_mode(&strip), AlphaMode::Mask(_)));
        assert!(matches!(blend_cut(&strip, &gradient), Some(c) if (c - 128.5 / 255.0).abs() < 1e-6));
        // Not without the flag (the alpha is forced to 1 then), with vertex alpha 1
        // throughout, or without vertex colours: those stay alpha-tested.
        let mut unflagged = gradient.clone();
        fix_vertex_colors(&mut unflagged, false);
        assert_eq!(blend_cut(&Props { vertex_alpha: false, ..strip.clone() }, &unflagged), None);
        assert_eq!(blend_cut(&strip, &mesh(&[1.0, 1.0, 1.0])), None);
        assert_eq!(blend_cut(&strip, &Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)), None);
        // Nor shapes that are only tested, blend below the threshold rule already, or add.
        assert_eq!(blend_cut(&Props { alpha: Some((0x12EC, 128)), ..strip.clone() }, &gradient), None);
        assert_eq!(blend_cut(&Props { alpha: Some((0x12ED, 0)), ..strip.clone() }, &gradient), None);
        assert_eq!(blend_cut(&Props { alpha: Some((0x120D, 128)), ..strip.clone() }, &gradient), None);
        assert_eq!(blend_cut(&Props { alpha: None, ..strip }, &gradient), None);
    }

    #[test]
    fn specular_from_mask() {
        let flagged = Props { specular: true, ..Default::default() };
        assert_eq!(specular(&flagged, true, false), (2.0, true));
        assert_eq!(specular(&flagged, true, true), (UNMASKED_SPECULAR, false));
        assert_eq!(specular(&Props::default(), true, false), (0.0, false));
        assert_eq!(specular(&flagged, false, false), (0.3, false));
        let hair = Props { specular: true, hair: true, ..Default::default() };
        assert_eq!((specular(&hair, true, false), specular(&hair, false, false)), ((0.0, false), (0.0, false)));
        // Physical lighting: a faint reflectance without the flag; the flag and hair as before.
        assert_eq!(specular_with(&Props::default(), true, false, true), (0.32, false));
        assert_eq!(specular_with(&flagged, true, false, true), (2.0, true));
        assert_eq!(specular_with(&hair, true, false, true), (0.0, false));
        // Phong exponent → roughness: 0 is fully rough, higher is smoother.
        assert!((gloss_roughness(0.0) - 1.0).abs() < 1e-6);
        assert!((gloss_roughness(67.0) - 0.41).abs() < 0.01);
        assert!(gloss_roughness(10.0) > gloss_roughness(67.0) && gloss_roughness(-5.0) == 1.0);
    }
}

#[cfg(test)]
#[path = "../tests/made_nif/mod.rs"]
mod made_nif;

#[cfg(test)]
#[path = "nif_numeric_admission_tests.rs"]
mod numeric_admission_tests;

#[cfg(test)]
mod authored_tests;

#[cfg(test)]
mod bc5_material_tests {
    use super::*;
    use crate::texture::bc5_tests::{OwnedData, triangle};
    use bevy::render::render_resource::TextureFormat;

    #[test]
    fn actual_nif_normal_slot_keeps_bc5_orientation_and_original_alpha_semantics() {
        let owned = OwnedData::new();
        let mut cache = TextureCache::default();
        let mut images = Assets::default();
        // Preload the same path as generic linear data. It cannot answer Normal.
        let linear = cache.get("signed.dds", false, &owned.data, &mut images, None).unwrap();
        let props = Props { textures: vec!["old.dds".into(), "signed.dds".into()], specular: true, ..Default::default() };
        let (material, missing) = shape_material(&props, &mut triangle(), &mut cache, &owned.data, &mut images, None);
        assert!(missing.is_none());
        let normal = material.base.normal_map_texture.as_ref().expect("real normal slot");
        assert_ne!(normal, &linear);
        assert_eq!(images.get(normal).unwrap().texture_descriptor.format, TextureFormat::Rgba8Unorm);
        assert_eq!(images.get(normal).unwrap().data.as_deref(), Some([0, 255, 128, 255].repeat(16).as_slice()));
        assert!(material.base.flip_normal_map_y, "the existing DirectX green flip still applies exactly once");
        assert!(material.base.specular_texture.is_none(), "generated alpha 255 must not become a full mask");
        assert_eq!(material.base.reflectance, UNMASKED_SPECULAR);
        let base = material.base.base_color_texture.as_ref().unwrap();
        assert_eq!(images.get(base).unwrap().texture_descriptor.format, TextureFormat::Bc3RgbaUnormSrgb);
        // The same ordinary DXT5 retains its mask, native bytes and normal convention.
        let old = Props { textures: vec!["old.dds".into(), "old.dds".into()], specular: true, ..Default::default() };
        let (old_material, missing) = shape_material(&old, &mut triangle(), &mut cache, &owned.data, &mut images, None);
        assert!(missing.is_none());
        let old_normal = old_material.base.normal_map_texture.as_ref().unwrap();
        assert_eq!(old_material.base.specular_texture.as_ref(), Some(old_normal));
        assert_eq!(old_material.base.reflectance, 2.0);
        assert_eq!(images.get(old_normal).unwrap().texture_descriptor.format, TextureFormat::Bc3RgbaUnorm);
        assert_eq!(images.get(old_normal).unwrap().data.as_deref(), Some([255; 16].as_slice()));
        // Tangent/UV admission remains in force even though decoding succeeded.
        let mut no_uv = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::MAIN_WORLD);
        no_uv.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3]);
        let refused = shape_material(&props, &mut no_uv, &mut cache, &owned.data, &mut images, None).0;
        assert!(refused.base.normal_map_texture.is_none());
        assert!(refused.base.specular_texture.is_none());
        assert_eq!(refused.base.reflectance, 0.3);
    }

    #[test]
    fn stencil_cull_maps_draw_mode_bits() {
        assert_eq!(stencil_cull(0), (false, Some(bevy::render::render_resource::Face::Back)));
        assert_eq!(stencil_cull(1 << 10), (false, Some(bevy::render::render_resource::Face::Back)));
        assert_eq!(stencil_cull(2 << 10), (false, Some(bevy::render::render_resource::Face::Front)));
        assert_eq!(stencil_cull(3 << 10), (true, None));
        for mode in 0..=3i64 {
            let flags = (mode << 10) | 0x0015;
            let expected = stencil_cull(flags);
            let props = Props {
                double_sided: mode == 3,
                stencil_draw_mode: Some(mode),
                ..Default::default()
            };
            assert_eq!(props_cull(&props), expected, "load-time props_cull must match stencil_cull for mode {mode}");
        }
    }
}
