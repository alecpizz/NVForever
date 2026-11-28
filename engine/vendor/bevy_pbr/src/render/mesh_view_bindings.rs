use crate::{
    AreaLightLuts, DfgLut, ScreenSpaceTransmission, ViewFogUniformOffset,
    ViewLightProbesUniformOffset, ViewLightsUniformOffset, ViewScreenSpaceReflectionsUniformOffset,
};
use arrayvec::ArrayVec;
use bevy_core_pipeline::{
    oit::{
        OitBuffers, OrderIndependentTransparencySettings,
        OrderIndependentTransparencySettingsOffset,
    },
    prepass::ViewPrepassTextures,
    tonemapping::{
        get_lut_bind_group_layout_entries, get_lut_bindings, Tonemapping, TonemappingLuts,
    },
};
use bevy_ecs::{
    component::Component,
    entity::Entity,
    query::Has,
    resource::Resource,
    system::{Commands, Query, Res},
};
use bevy_light::{EnvironmentMapLight, IrradianceVolume};
use bevy_math::Vec4;
use bevy_platform::sync::Arc;
use bevy_render::{
    camera::ExtractedCamera,
    globals::{GlobalsBuffer, GlobalsUniform},
    render_asset::RenderAssets,
    render_resource::{binding_types::*, *},
    renderer::{RenderAdapter, RenderDevice},
    texture::{FallbackImage, FallbackImageZero, GpuImage},
    view::{
        Msaa, RenderVisibilityRanges, ViewUniform, ViewUniformOffset, ViewUniforms,
        VISIBILITY_RANGES_STORAGE_BUFFER_COUNT,
    },
};
use core::fmt::Write;
use core::num::NonZero;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::{
    contact_shadows::{
        ContactShadowsBuffer, ContactShadowsUniform, ViewContactShadowsUniformOffset,
    },
    decal::{
        self,
        clustered::{
            DecalsBuffer, RenderClusteredDecals, RenderViewClusteredDecalBindGroupEntries,
        },
    },
    environment_map::{self, RenderViewEnvironmentMapBindGroupEntries},
    irradiance_volume::{
        self, RenderViewIrradianceVolumeBindGroupEntries, IRRADIANCE_VOLUMES_ARE_USABLE,
    },
    prepass,
    resources::{AtmosphereBuffer, AtmosphereSampler, AtmosphereTextures, GpuAtmosphere},
    Bluenoise, ExtractedAtmosphere, FogMeta, GlobalClusterableObjectMeta, GpuClusteredLights,
    GpuFog, GpuLights, LightMeta, LightProbesBuffer, LightProbesUniform, MeshPipeline,
    MeshPipelineKey, RenderViewLightProbes, ScreenSpaceAmbientOcclusionResources,
    ScreenSpaceReflectionsBuffer, ScreenSpaceReflectionsUniform, ShadowSamplers,
    ViewClusterBindings, ViewShadowBindings, ViewTransmissionTexture,
    CLUSTERED_FORWARD_STORAGE_BUFFER_COUNT,
};

#[cfg(all(feature = "webgl", target_arch = "wasm32", not(feature = "webgpu")))]
use bevy_render::render_resource::binding_types::texture_cube;

#[cfg(debug_assertions)]
use {crate::MESH_PIPELINE_VIEW_LAYOUT_SAFE_MAX_TEXTURES, bevy_utils::once, tracing::warn};

pub const TONEMAPPING_LUT_TEXTURE_BINDING_INDEX: u32 = 18;
pub const TONEMAPPING_LUT_SAMPLER_BINDING_INDEX: u32 = 19;

#[derive(Clone)]
pub struct MeshPipelineViewLayout {
    pub main_layout: BindGroupLayoutDescriptor,
    pub binding_array_layout: BindGroupLayoutDescriptor,
    pub empty_layout: BindGroupLayoutDescriptor,
}

bitflags::bitflags! {
    /// A key that uniquely identifies a [`MeshPipelineViewLayout`].
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    #[repr(transparent)]
    pub struct MeshPipelineViewLayoutKey: u32 {
        const MULTISAMPLED                     = 1 << 0;
        const DEPTH_PREPASS                    = 1 << 1;
        const NORMAL_PREPASS                   = 1 << 2;
        const MOTION_VECTOR_PREPASS            = 1 << 3;
        const DEFERRED_PREPASS                 = 1 << 4;
        const OIT_ENABLED                      = 1 << 5;
        const ATMOSPHERE                       = 1 << 6;
        const STBN                             = 1 << 7;
        const TONEMAP_IN_SHADER                = 1 << 8;
        const ENVIRONMENT_MAP                  = 1 << 9;
        const SCREEN_SPACE_AMBIENT_OCCLUSION   = 1 << 10;
        const IRRADIANCE_VOLUME                = 1 << 11;
        const SCREEN_SPACE_REFLECTIONS         = 1 << 12;
        const CONTACT_SHADOWS                  = 1 << 13;
        const DISTANCE_FOG                     = 1 << 14;
        const AREA_LIGHT_LUTS                  = 1 << 15;
        const VIEW_TRANSMISSION_TEXTURE        = 1 << 16;
        // NVForever sun visibility: bindings 39-42 ([`NvfSunVisibilityBindings`]).
        const NVF_SUN_VISIBILITY               = 1 << 17;
        // NVForever ambient occlusion: bindings 43-44 ([`NvfAmbientOcclusionBindings`]).
        const NVF_AMBIENT_OCCLUSION            = 1 << 18;
        // NVForever sun visibility's light volume: bindings 45-46, with
        // `NVF_SUN_VISIBILITY` only ([`NvfSunVisibilityBindings::cloud_volume`]).
        const NVF_CLOUD_VOLUME                 = 1 << 19;
        // NVForever RTGI volume: bindings 47-49 ([`NvfRtgiVolumeBindings`]).
        const NVF_RTGI_VOLUME                  = 1 << 20;
    }
}

/// NVForever sun visibility (2026-11-17, engine/vendor/README.md): what a view's mesh
/// view bind group holds for the game's sun visibility factor (nvforever
/// `sun_visibility.rs`), at bindings 39 (uniform), 40 (cloud transmittance, a filterable
/// float 2D texture), 41 (terrain shadow-height map, an unfilterable float 2D texture,
/// `Rgba32Float`, read with `textureLoad` only) and 42 (a filtering sampler, used with 40
/// only). A view with it gets [`MeshPipelineViewLayoutKey::NVF_SUN_VISIBILITY`] and
/// those bindings, and its pipelines `MeshPipelineKey::NVF_SUN_VISIBILITY` with the
/// shader def of the same name (`cloud` and `terrain` add `NVF_CLOUD_VISIBILITY` and
/// `NVF_TERRAIN_VISIBILITY`). Both sides read this one component in the same frame, so
/// it may be inserted or removed, and `cloud`/`terrain` changed, only during extraction;
/// the texture views and the buffer's contents may change in any render set before
/// `RenderSystems::PrepareBindGroups`. Bevy inserts none; without it views are as
/// upstream's. Bindings 45 and 46 (2026-11-20, nvforever #3032 stage 2i M2): the
/// volumetric clouds' light volume, near and far front sets (filterable float 2D arrays,
/// `R16Float`, sampled with 42), for the cloud shadows from the volume; a stand-in when
/// there is none. They are in the layout only with `cloud_volume` (layout key
/// [`MeshPipelineViewLayoutKey::NVF_CLOUD_VOLUME`], pipeline key
/// `MeshPipelineKey::NVF_CLOUD_VOLUME_VISIBILITY`, shader def `NVF_CLOUD_VOLUME`), which the
/// game leaves off while its Volumetric clouds row is off and on adapters whose fragment stage has
/// too few sampled textures for them (nvforever `sun_visibility.rs` `volume_layout`,
/// `CloudVolumeRoom`): the weather-only layout.
#[derive(Component, Clone)]
pub struct NvfSunVisibilityBindings {
    pub uniform: Buffer,
    pub cloud_texture: TextureView,
    pub terrain_texture: TextureView,
    pub sampler: Sampler,
    pub light_near: TextureView,
    pub light_far: TextureView,
    pub cloud: bool,
    pub terrain: bool,
    /// Bindings 45 and 46 are in the layout and the bind group (and the pipelines get
    /// `NVF_CLOUD_VOLUME`); off, `light_near` and `light_far` are not bound.
    pub cloud_volume: bool,
}

/// NVForever ambient occlusion (2026-11-18, engine/vendor/README.md): what a view's mesh
/// view bind group holds for the game's ambient occlusion on the ambient light (nvforever
/// `ssao_lighting.rs`), at bindings 43 (uniform) and 44 (the half-resolution occlusion, an
/// unfilterable float 2D texture read with `textureLoad`). A view with it gets
/// [`MeshPipelineViewLayoutKey::NVF_AMBIENT_OCCLUSION`] and those bindings, and its
/// pipelines `MeshPipelineKey::NVF_AMBIENT_OCCLUSION`, with the shader def of the same name
/// for the pipelines that write depth (Bevy's own rule for its SSAO: what is not in the
/// depth prepass has no occlusion there). Both sides read this one component in the same
/// frame, so it may be inserted or removed only during extraction; the texture view and
/// the buffer's contents may change in any render set before
/// `RenderSystems::PrepareBindGroups`. Bevy inserts none; without it views are as
/// upstream's.
#[derive(Component, Clone)]
pub struct NvfAmbientOcclusionBindings {
    pub uniform: Buffer,
    pub texture: TextureView,
}

/// NVForever RTGI volume (2026-11-25, engine/vendor/README.md): what a view's mesh view bind
/// group holds for the bounce light of a probe volume (nvforever `rtgi_volume.rs`,
/// #3009), at bindings 47 (uniform), 48 (the volume, a filterable float 3D texture) and 49
/// (a filtering sampler, used with 48 only). A view with it gets
/// [`MeshPipelineViewLayoutKey::NVF_RTGI_VOLUME`] and those bindings, and its pipelines
/// `MeshPipelineKey::NVF_RTGI_VOLUME`; the mesh pipeline pushes no shader def for them: the
/// materials that read the bindings push their own. Both sides read this one component in
/// the same frame, so it may be inserted or removed only during extraction; the texture
/// view, the sampler and the buffer's contents may change in any render set before
/// `RenderSystems::PrepareBindGroups`. Bevy inserts none; without it views are as
/// upstream's.
#[derive(Component, Clone)]
pub struct NvfRtgiVolumeBindings {
    pub uniform: Buffer,
    pub texture: TextureView,
    pub sampler: Sampler,
}

/// NVForever RTGI volume: how many mesh view bind groups [`prepare_mesh_view_bind_groups`] has
/// made in this process, and how many of them with [`NvfRtgiVolumeBindings`]: what a run says at
/// its end (nvforever `rtgi_volume.rs`), counted where it is bound.
pub static NVF_VIEW_BIND_GROUPS: AtomicU64 = AtomicU64::new(0);
pub static NVF_RTGI_VIEW_BIND_GROUPS: AtomicU64 = AtomicU64::new(0);

impl MeshPipelineViewLayoutKey {
    /// Builds a unique label for each layout based on the flags
    pub fn label(&self) -> String {
        let iter = self
            .iter_names()
            .filter(|(_, key)| self.contains(*key))
            .map(|(name, _)| name.to_lowercase());
        let (lower, _) = iter.size_hint();
        let sep = ",";
        let mut result = String::with_capacity(sep.len() * lower);

        for (i, name) in iter.enumerate() {
            if i != 0 {
                result.push_str(sep);
            }
            write!(&mut result, "{}", name).unwrap();
        }

        format!("mesh_view_layout:{}", result)
    }
}

impl From<MeshPipelineKey> for MeshPipelineViewLayoutKey {
    fn from(value: MeshPipelineKey) -> Self {
        let mut result = MeshPipelineViewLayoutKey::empty();

        if value.msaa_samples() > 1 {
            result |= MeshPipelineViewLayoutKey::MULTISAMPLED;
        }
        if value.contains(MeshPipelineKey::DEPTH_PREPASS) {
            result |= MeshPipelineViewLayoutKey::DEPTH_PREPASS;
        }
        if value.contains(MeshPipelineKey::NORMAL_PREPASS) {
            result |= MeshPipelineViewLayoutKey::NORMAL_PREPASS;
        }
        if value.contains(MeshPipelineKey::MOTION_VECTOR_PREPASS) {
            result |= MeshPipelineViewLayoutKey::MOTION_VECTOR_PREPASS;
        }
        if value.contains(MeshPipelineKey::DEFERRED_PREPASS) {
            result |= MeshPipelineViewLayoutKey::DEFERRED_PREPASS;
        }
        if value.contains(MeshPipelineKey::OIT_ENABLED) {
            result |= MeshPipelineViewLayoutKey::OIT_ENABLED;
        }
        if value.contains(MeshPipelineKey::ATMOSPHERE) {
            result |= MeshPipelineViewLayoutKey::ATMOSPHERE;
        }

        if cfg!(feature = "bluenoise_texture") {
            result |= MeshPipelineViewLayoutKey::STBN;
        }

        if cfg!(feature = "area_light_luts") {
            result |= MeshPipelineViewLayoutKey::AREA_LIGHT_LUTS;
        }

        if value.contains(MeshPipelineKey::TONEMAP_IN_SHADER) {
            result |= MeshPipelineViewLayoutKey::TONEMAP_IN_SHADER;
        }
        if value.contains(MeshPipelineKey::ENVIRONMENT_MAP) {
            result |= MeshPipelineViewLayoutKey::ENVIRONMENT_MAP;
        }
        if value.contains(MeshPipelineKey::SCREEN_SPACE_AMBIENT_OCCLUSION) {
            result |= MeshPipelineViewLayoutKey::SCREEN_SPACE_AMBIENT_OCCLUSION;
        }
        if value.contains(MeshPipelineKey::IRRADIANCE_VOLUME) {
            result |= MeshPipelineViewLayoutKey::IRRADIANCE_VOLUME;
        }
        if value.contains(MeshPipelineKey::SCREEN_SPACE_REFLECTIONS) {
            result |= MeshPipelineViewLayoutKey::SCREEN_SPACE_REFLECTIONS;
        }
        if value.contains(MeshPipelineKey::CONTACT_SHADOWS) {
            result |= MeshPipelineViewLayoutKey::CONTACT_SHADOWS;
        }
        if value.contains(MeshPipelineKey::DISTANCE_FOG) {
            result |= MeshPipelineViewLayoutKey::DISTANCE_FOG;
        }
        if value.contains(MeshPipelineKey::VIEW_TRANSMISSION_TEXTURE) {
            result |= MeshPipelineViewLayoutKey::VIEW_TRANSMISSION_TEXTURE;
        }
        // NVForever sun visibility.
        if value.contains(MeshPipelineKey::NVF_SUN_VISIBILITY) {
            result |= MeshPipelineViewLayoutKey::NVF_SUN_VISIBILITY;
            if value.contains(MeshPipelineKey::NVF_CLOUD_VOLUME_VISIBILITY) {
                result |= MeshPipelineViewLayoutKey::NVF_CLOUD_VOLUME;
            }
        }
        // NVForever ambient occlusion.
        if value.contains(MeshPipelineKey::NVF_AMBIENT_OCCLUSION) {
            result |= MeshPipelineViewLayoutKey::NVF_AMBIENT_OCCLUSION;
        }
        // NVForever RTGI volume.
        if value.contains(MeshPipelineKey::NVF_RTGI_VOLUME) {
            result |= MeshPipelineViewLayoutKey::NVF_RTGI_VOLUME;
        }

        result
    }
}

impl From<Msaa> for MeshPipelineViewLayoutKey {
    fn from(value: Msaa) -> Self {
        let mut result = MeshPipelineViewLayoutKey::empty();

        if value.samples() > 1 {
            result |= MeshPipelineViewLayoutKey::MULTISAMPLED;
        }

        result
    }
}

impl From<Option<&ViewPrepassTextures>> for MeshPipelineViewLayoutKey {
    fn from(value: Option<&ViewPrepassTextures>) -> Self {
        let mut result = MeshPipelineViewLayoutKey::empty();

        if let Some(prepass_textures) = value {
            if prepass_textures.depth.is_some() {
                result |= MeshPipelineViewLayoutKey::DEPTH_PREPASS;
            }
            if prepass_textures.normal.is_some() {
                result |= MeshPipelineViewLayoutKey::NORMAL_PREPASS;
            }
            if prepass_textures.motion_vectors.is_some() {
                result |= MeshPipelineViewLayoutKey::MOTION_VECTOR_PREPASS;
            }
            if prepass_textures.deferred.is_some() {
                result |= MeshPipelineViewLayoutKey::DEFERRED_PREPASS;
            }
        }

        result
    }
}

pub(crate) fn buffer_layout(
    buffer_binding_type: BufferBindingType,
    has_dynamic_offset: bool,
    min_binding_size: Option<NonZero<u64>>,
) -> BindGroupLayoutEntryBuilder {
    match buffer_binding_type {
        BufferBindingType::Uniform => uniform_buffer_sized(has_dynamic_offset, min_binding_size),
        BufferBindingType::Storage { read_only } => {
            if read_only {
                storage_buffer_read_only_sized(has_dynamic_offset, min_binding_size)
            } else {
                storage_buffer_sized(has_dynamic_offset, min_binding_size)
            }
        }
    }
}

/// NVForever sun visibility: the mesh view layout's entries for
/// [`NvfSunVisibilityBindings`] under `layout_key` (binding, entry): none without
/// [`MeshPipelineViewLayoutKey::NVF_SUN_VISIBILITY`]; 39-42 with it; 45-46 as well with
/// [`MeshPipelineViewLayoutKey::NVF_CLOUD_VOLUME`].
pub fn nvf_sun_visibility_layout_entries(
    layout_key: MeshPipelineViewLayoutKey,
) -> Vec<(u32, BindGroupLayoutEntryBuilder)> {
    let mut out = Vec::new();
    if !layout_key.contains(MeshPipelineViewLayoutKey::NVF_SUN_VISIBILITY) {
        return out;
    }
    out.push((39, uniform_buffer_sized(false, None)));
    out.push((
        40,
        texture_2d(TextureSampleType::Float { filterable: true }),
    ));
    // The terrain shadow-height map is Rgba32Float, which is not filterable without an
    // optional feature: unfilterable, read with loads only.
    out.push((
        41,
        texture_2d(TextureSampleType::Float { filterable: false }),
    ));
    out.push((42, sampler(SamplerBindingType::Filtering)));
    if layout_key.contains(MeshPipelineViewLayoutKey::NVF_CLOUD_VOLUME) {
        // The light volume's near and far front sets (R16Float arrays, filterable).
        out.push((
            45,
            texture_2d_array(TextureSampleType::Float { filterable: true }),
        ));
        out.push((
            46,
            texture_2d_array(TextureSampleType::Float { filterable: true }),
        ));
    }
    out
}

/// NVForever RTGI volume: the mesh view layout's entries for [`NvfRtgiVolumeBindings`]
/// under `layout_key` (binding, entry): 47-49 with
/// [`MeshPipelineViewLayoutKey::NVF_RTGI_VOLUME`], none without.
pub fn nvf_rtgi_layout_entries(
    layout_key: MeshPipelineViewLayoutKey,
) -> Vec<(u32, BindGroupLayoutEntryBuilder)> {
    if !layout_key.contains(MeshPipelineViewLayoutKey::NVF_RTGI_VOLUME) {
        return Vec::new();
    }
    vec![
        (47, uniform_buffer_sized(false, None)),
        (
            48,
            texture_3d(TextureSampleType::Float { filterable: true }),
        ),
        (49, sampler(SamplerBindingType::Filtering)),
    ]
}

/// Returns the appropriate bind group layout vec based on the parameters
fn layout_entries(
    layout_key: MeshPipelineViewLayoutKey,
    &MeshPipelineViewLayoutParams {
        clustered_forward_buffer_binding_type,
        visibility_ranges_buffer_binding_type,
        environment_map_entries,
        irradiance_volume_entries,
        clustered_decal_entries,
        is_oit_supported,
    }: &MeshPipelineViewLayoutParams,
) -> [Vec<BindGroupLayoutEntry>; 2] {
    let mut entries = DynamicBindGroupLayoutEntries::new_with_indices(
        ShaderStages::FRAGMENT,
        (
            // View
            (
                0,
                uniform_buffer::<ViewUniform>(true).visibility(ShaderStages::VERTEX_FRAGMENT),
            ),
            // Lights
            (1, uniform_buffer::<GpuLights>(true)),
            // Point Shadow Texture Cube Array
            (
                2,
                #[cfg(all(
                    not(target_abi = "sim"),
                    any(
                        not(feature = "webgl"),
                        not(target_arch = "wasm32"),
                        feature = "webgpu"
                    )
                ))]
                texture_cube_array(TextureSampleType::Depth),
                #[cfg(any(
                    target_abi = "sim",
                    all(feature = "webgl", target_arch = "wasm32", not(feature = "webgpu"))
                ))]
                texture_cube(TextureSampleType::Depth),
            ),
            // Point Shadow Texture Array Comparison Sampler
            (3, sampler(SamplerBindingType::Comparison)),
            // Point Shadow Texture Array Linear Sampler
            #[cfg(feature = "experimental_pbr_pcss")]
            (4, sampler(SamplerBindingType::Filtering)),
            // NVForever ALCS far maps: the far point shadow texture array (light.rs
            // `HoldLightShadowMaps::far_slots`), with binding 2's samplers.
            #[cfg(all(
                not(target_abi = "sim"),
                any(
                    not(feature = "webgl"),
                    not(target_arch = "wasm32"),
                    feature = "webgpu"
                )
            ))]
            (50, texture_cube_array(TextureSampleType::Depth)),
            // Directional Shadow Texture Array
            (
                5,
                #[cfg(any(
                    not(feature = "webgl"),
                    not(target_arch = "wasm32"),
                    feature = "webgpu"
                ))]
                texture_2d_array(TextureSampleType::Depth),
                #[cfg(all(feature = "webgl", target_arch = "wasm32", not(feature = "webgpu")))]
                texture_2d(TextureSampleType::Depth),
            ),
            // Directional Shadow Texture Array Comparison Sampler
            (6, sampler(SamplerBindingType::Comparison)),
            // Directional Shadow Texture Array Linear Sampler
            #[cfg(feature = "experimental_pbr_pcss")]
            (7, sampler(SamplerBindingType::Filtering)),
            // PointLights
            (
                8,
                buffer_layout(
                    clustered_forward_buffer_binding_type,
                    false,
                    Some(GpuClusteredLights::min_size(
                        clustered_forward_buffer_binding_type,
                    )),
                ),
            ),
            // ClusteredLightIndexLists
            (
                9,
                buffer_layout(
                    clustered_forward_buffer_binding_type,
                    false,
                    Some(
                        ViewClusterBindings::min_size_clusterable_object_index_lists(
                            clustered_forward_buffer_binding_type,
                        ),
                    ),
                ),
            ),
            // ClusterOffsetsAndCounts
            (
                10,
                buffer_layout(
                    clustered_forward_buffer_binding_type,
                    false,
                    Some(ViewClusterBindings::min_size_cluster_offsets_and_counts(
                        clustered_forward_buffer_binding_type,
                    )),
                ),
            ),
            // Globals
            (
                11,
                uniform_buffer::<GlobalsUniform>(false).visibility(ShaderStages::VERTEX_FRAGMENT),
            ),
            // Light probes
            (12, uniform_buffer::<LightProbesUniform>(true)),
            // Visibility ranges
            (
                14,
                buffer_layout(
                    visibility_ranges_buffer_binding_type,
                    false,
                    Some(Vec4::min_size()),
                )
                .visibility(ShaderStages::VERTEX),
            ),
        ),
    );

    if layout_key.contains(MeshPipelineViewLayoutKey::DISTANCE_FOG) {
        entries = entries.extend_with_indices((
            // Fog
            (13, uniform_buffer::<GpuFog>(true)),
        ));
    }
    if layout_key.contains(MeshPipelineViewLayoutKey::SCREEN_SPACE_REFLECTIONS) {
        entries = entries.extend_with_indices((
            // Screen space reflection settings
            (15, uniform_buffer::<ScreenSpaceReflectionsUniform>(true)),
        ));
    }
    if layout_key.contains(MeshPipelineViewLayoutKey::CONTACT_SHADOWS) {
        entries = entries.extend_with_indices((
            // Contact shadows settings
            (16, uniform_buffer::<ContactShadowsUniform>(true)),
        ));
    }
    if layout_key.contains(MeshPipelineViewLayoutKey::SCREEN_SPACE_AMBIENT_OCCLUSION) {
        entries = entries.extend_with_indices((
            // Screen space ambient occlusion texture
            (
                17,
                texture_2d(TextureSampleType::Float { filterable: false }),
            ),
        ));
    }

    if layout_key.contains(MeshPipelineViewLayoutKey::TONEMAP_IN_SHADER) {
        // Tonemapping
        let tonemapping_lut_entries = get_lut_bind_group_layout_entries();
        entries = entries.extend_with_indices((
            (
                TONEMAPPING_LUT_TEXTURE_BINDING_INDEX,
                tonemapping_lut_entries[0],
            ),
            (
                TONEMAPPING_LUT_SAMPLER_BINDING_INDEX,
                tonemapping_lut_entries[1],
            ),
        ));
    }

    // Prepass
    if cfg!(any(feature = "webgpu", not(target_arch = "wasm32")))
        || !layout_key.contains(MeshPipelineViewLayoutKey::MULTISAMPLED)
    {
        for (entry, binding) in prepass::get_bind_group_layout_entries(layout_key)
            .iter()
            .zip([20, 21, 22, 23])
        {
            if let Some(entry) = entry {
                entries = entries.extend_with_indices(((binding as u32, *entry),));
            }
        }
    }

    // View Transmission Texture
    if layout_key.contains(MeshPipelineViewLayoutKey::VIEW_TRANSMISSION_TEXTURE) {
        entries = entries.extend_with_indices((
            (
                24,
                texture_2d(TextureSampleType::Float { filterable: true }),
            ),
            (25, sampler(SamplerBindingType::Filtering)),
        ));
    }

    // OIT
    if layout_key.contains(MeshPipelineViewLayoutKey::OIT_ENABLED) {
        // Check if we can use OIT. This is a hack to avoid errors on webgl --
        // the OIT plugin will warn the user that OIT is not supported on their
        // platform, so we don't need to do it here.
        if is_oit_supported {
            entries = entries.extend_with_indices((
                (
                    26,
                    uniform_buffer::<OrderIndependentTransparencySettings>(true),
                ),
                // oit_nodes_capacity
                (27, uniform_buffer::<u32>(false)),
                // oit_nodes
                (28, storage_buffer_sized(false, None)),
                // oit_heads,
                (29, storage_buffer_sized(false, None)),
                // oit_atomic_counter
                (
                    30,
                    storage_buffer_sized(false, NonZero::<u64>::new(size_of::<u32>() as u64)),
                ),
            ));
        }
    }

    // Atmosphere
    if layout_key.contains(MeshPipelineViewLayoutKey::ATMOSPHERE) {
        entries = entries.extend_with_indices((
            // transmittance LUT
            (
                31,
                texture_2d(TextureSampleType::Float { filterable: true }),
            ),
            (32, sampler(SamplerBindingType::Filtering)),
            // atmosphere data buffer
            (33, storage_buffer_read_only::<GpuAtmosphere>(false)),
        ));
    }

    // Blue noise
    if layout_key.contains(MeshPipelineViewLayoutKey::STBN) {
        entries = entries.extend_with_indices(((
            34,
            texture_2d_array(TextureSampleType::Float { filterable: false }),
        ),));
    }
    // LTC LUTs for area lights
    if cfg!(feature = "area_light_luts") {
        entries = entries.extend_with_indices((
            (
                35,
                texture_2d_array(TextureSampleType::Float { filterable: true }),
            ),
            (36, sampler(SamplerBindingType::Filtering)),
        ));
    }
    // DFG LUT
    if cfg!(feature = "dfg_lut") {
        entries = entries.extend_with_indices((
            (
                37,
                texture_2d(TextureSampleType::Float { filterable: true }),
            ),
            (38, sampler(SamplerBindingType::Filtering)),
        ));
    }
    // NVForever sun visibility ([`NvfSunVisibilityBindings`]).
    for (binding, entry) in nvf_sun_visibility_layout_entries(layout_key) {
        entries = entries.extend_with_indices(((binding, entry),));
    }
    // NVForever ambient occlusion ([`NvfAmbientOcclusionBindings`]).
    if layout_key.contains(MeshPipelineViewLayoutKey::NVF_AMBIENT_OCCLUSION) {
        entries = entries.extend_with_indices((
            (43, uniform_buffer_sized(false, None)),
            (
                44,
                texture_2d(TextureSampleType::Float { filterable: false }),
            ),
        ));
    }
    // NVForever RTGI volume ([`NvfRtgiVolumeBindings`]).
    for (binding, entry) in nvf_rtgi_layout_entries(layout_key) {
        entries = entries.extend_with_indices(((binding, entry),));
    }

    let mut binding_array_entries = DynamicBindGroupLayoutEntries::new(ShaderStages::FRAGMENT);
    if layout_key.contains(MeshPipelineViewLayoutKey::ENVIRONMENT_MAP) {
        binding_array_entries = binding_array_entries.extend_with_indices((
            (0, environment_map_entries[0]),
            (1, environment_map_entries[1]),
            (2, environment_map_entries[2]),
        ));
    }

    if layout_key.contains(MeshPipelineViewLayoutKey::IRRADIANCE_VOLUME) {
        // Irradiance volumes
        if IRRADIANCE_VOLUMES_ARE_USABLE {
            binding_array_entries = binding_array_entries.extend_with_indices((
                (3, irradiance_volume_entries[0]),
                (4, irradiance_volume_entries[1]),
            ));
        }
    }

    // Clustered decals
    if let Some(clustered_decal_entries) = clustered_decal_entries {
        binding_array_entries = binding_array_entries.extend_with_indices((
            (5, clustered_decal_entries[0]),
            (6, clustered_decal_entries[1]),
            (7, clustered_decal_entries[2]),
        ));
    }

    [entries.to_vec(), binding_array_entries.to_vec()]
}

/// Parameters needed by [`layout_entries`].
#[derive(Clone, Copy)]
struct MeshPipelineViewLayoutParams {
    clustered_forward_buffer_binding_type: BufferBindingType,
    visibility_ranges_buffer_binding_type: BufferBindingType,
    environment_map_entries: [BindGroupLayoutEntryBuilder; 3],
    irradiance_volume_entries: [BindGroupLayoutEntryBuilder; 2],
    clustered_decal_entries: Option<[BindGroupLayoutEntryBuilder; 3]>,
    is_oit_supported: bool,
}

/// Stores the view layouts entries for creating bind group layouts of pipeline keys.
#[derive(Resource, Clone)]
pub struct MeshPipelineViewLayouts {
    params: Arc<MeshPipelineViewLayoutParams>,
}

pub fn init_mesh_pipeline_view_layouts(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_adapter: Res<RenderAdapter>,
) {
    let clustered_forward_buffer_binding_type =
        render_device.get_supported_read_only_binding_type(CLUSTERED_FORWARD_STORAGE_BUFFER_COUNT);
    let visibility_ranges_buffer_binding_type =
        render_device.get_supported_read_only_binding_type(VISIBILITY_RANGES_STORAGE_BUFFER_COUNT);

    let res = MeshPipelineViewLayouts {
        params: Arc::new(MeshPipelineViewLayoutParams {
            clustered_forward_buffer_binding_type,
            visibility_ranges_buffer_binding_type,
            environment_map_entries: environment_map::get_bind_group_layout_entries(
                &render_device,
                &render_adapter,
            ),
            irradiance_volume_entries: irradiance_volume::get_bind_group_layout_entries(
                &render_device,
                &render_adapter,
            ),
            clustered_decal_entries: decal::clustered::get_bind_group_layout_entries(
                &render_device,
                &render_adapter,
            ),
            is_oit_supported: bevy_core_pipeline::oit::resolve::is_oit_supported(
                &render_adapter,
                &render_device,
                false,
            ),
        }),
    };

    commands.insert_resource(res);
}

impl MeshPipelineViewLayouts {
    /// Get view bind group layout for the given key.
    pub fn get_view_layout(&self, layout_key: MeshPipelineViewLayoutKey) -> MeshPipelineViewLayout {
        let mut entries = layout_entries(layout_key, &self.params);

        #[cfg(debug_assertions)]
        let texture_count: usize = entries
            .iter()
            .flat_map(|e| {
                e.iter()
                    .filter(|entry| matches!(entry.ty, BindingType::Texture { .. }))
            })
            .count();

        #[cfg(debug_assertions)]
        if texture_count > MESH_PIPELINE_VIEW_LAYOUT_SAFE_MAX_TEXTURES {
            // Issue our own warning here because Naga's error message is a bit cryptic in this situation
            once!(warn!(
                "Too many textures in mesh pipeline view layout, this might cause us \
            to hit `wgpu::Limits::max_sampled_textures_per_shader_stage` in some environments."
            ));
        }

        MeshPipelineViewLayout {
            main_layout: BindGroupLayoutDescriptor {
                label: layout_key.label().into(),
                entries: core::mem::take(&mut entries[0]),
            },
            binding_array_layout: BindGroupLayoutDescriptor {
                label: layout_key
                    .label()
                    .replace("mesh_view_layout:", "mesh_view_layout_binding_array:")
                    .into(),
                entries: core::mem::take(&mut entries[1]),
            },
            empty_layout: BindGroupLayoutDescriptor {
                label: "mesh_view_layout_empty".into(),
                entries: vec![],
            },
        }
    }
}

#[derive(Component)]
pub struct MeshViewBindGroup {
    pub main: BindGroup,
    pub main_offsets: ArrayVec<u32, 7>,
    pub binding_array: BindGroup,
    pub empty: BindGroup,
}

pub fn prepare_mesh_view_bind_groups(
    mut commands: Commands,
    (render_device, pipeline_cache, render_adapter): (
        Res<RenderDevice>,
        Res<PipelineCache>,
        Res<RenderAdapter>,
    ),
    mesh_pipeline: Res<MeshPipeline>,
    shadow_samplers: Res<ShadowSamplers>,
    (light_meta, global_clusterable_object_meta, fog_meta, view_uniforms): (
        Res<LightMeta>,
        Res<GlobalClusterableObjectMeta>,
        Res<FogMeta>,
        Res<ViewUniforms>,
    ),
    views: Query<(
        Entity,
        Option<&ExtractedCamera>,
        &ViewShadowBindings,
        &ViewClusterBindings,
        &Msaa,
        Option<&ScreenSpaceAmbientOcclusionResources>,
        Option<&ViewPrepassTextures>,
        Option<&ViewTransmissionTexture>,
        Option<&AtmosphereTextures>,
        Option<&AtmosphereBuffer>,
        &Tonemapping,
        (
            Option<&RenderViewLightProbes<EnvironmentMapLight>>,
            Option<&RenderViewLightProbes<IrradianceVolume>>,
            // NVForever sun visibility.
            Option<&NvfSunVisibilityBindings>,
            // NVForever ambient occlusion.
            Option<&NvfAmbientOcclusionBindings>,
            // NVForever RTGI volume.
            Option<&NvfRtgiVolumeBindings>,
        ),
        Has<ExtractedAtmosphere>,
        (
            &ViewUniformOffset,
            &ViewLightsUniformOffset,
            &ViewLightProbesUniformOffset,
            Option<&ViewFogUniformOffset>,
            Option<&ViewScreenSpaceReflectionsUniformOffset>,
            Option<&ViewContactShadowsUniformOffset>,
            Option<&OrderIndependentTransparencySettingsOffset>,
            Has<ScreenSpaceTransmission>,
        ),
    )>,
    (images, fallback_image, fallback_image_zero): (
        Res<RenderAssets<GpuImage>>,
        Res<FallbackImage>,
        Res<FallbackImageZero>,
    ),
    globals_buffer: Res<GlobalsBuffer>,
    tonemapping_luts: Res<TonemappingLuts>,
    light_probes_buffer: Res<LightProbesBuffer>,
    visibility_ranges: Res<RenderVisibilityRanges>,
    (ssr_buffer, contact_shadows_buffer, oit_buffers): (
        Res<ScreenSpaceReflectionsBuffer>,
        Res<ContactShadowsBuffer>,
        Res<OitBuffers>,
    ),
    (decals_buffer, render_decals, atmosphere_sampler, blue_noise, area_light_luts, dfg_lut): (
        Res<DecalsBuffer>,
        Res<RenderClusteredDecals>,
        Option<Res<AtmosphereSampler>>,
        Res<Bluenoise>,
        Res<AreaLightLuts>,
        Res<DfgLut>,
    ),
    // TODO: Figure out how to reuse the memory. `BindGroupEntry` is non-send on wasm with atomics.
    #[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
    mut entries_cache: bevy_ecs::system::Local<Vec<BindGroupEntry>>,
    #[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
    mut entries_binding_array_cache: bevy_ecs::system::Local<Vec<BindGroupEntry>>,
) {
    if let (
        Some(view_binding),
        Some(light_binding),
        Some(clusterable_objects_binding),
        Some(globals),
        Some(light_probes_binding),
        Some(visibility_ranges_buffer),
    ) = (
        view_uniforms.uniforms.binding(),
        light_meta.view_gpu_lights.binding(),
        global_clusterable_object_meta
            .gpu_clustered_lights
            .binding(),
        globals_buffer.buffer.binding(),
        light_probes_buffer.binding(),
        visibility_ranges.buffer().buffer(),
    ) {
        for (
            entity,
            camera,
            shadow_bindings,
            cluster_bindings,
            msaa,
            ssao_resources,
            prepass_textures,
            transmission_texture,
            atmosphere_textures,
            atmosphere_buffer,
            tonemapping,
            (
                render_view_environment_maps,
                render_view_irradiance_volumes,
                nvf_sun_visibility,
                nvf_ambient_occlusion,
                nvf_rtgi_volume,
            ),
            has_atmosphere,
            (
                view_uniform_offset,
                view_lights_offset,
                view_light_probes_offset,
                view_fog_offset,
                view_ssr_offset,
                view_contact_shadows_offset,
                view_oit_settings_offset,
                has_transmission,
            ),
        ) in &views
        {
            let mut entries = DynamicBindGroupEntries::new();
            let mut entries_binding_array = DynamicBindGroupEntries::new();
            #[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
            {
                // Take cache that has static lifetime for `DynamicBindGroupEntries`.
                // See <https://users.rust-lang.org/t/how-to-cache-a-vectors-capacity/94478/10>.
                entries.entries = core::mem::take(&mut *entries_cache)
                    .into_iter()
                    .map(|_| -> BindGroupEntry { unreachable!() })
                    .collect();
                entries_binding_array.entries = core::mem::take(&mut *entries_binding_array_cache)
                    .into_iter()
                    .map(|_| -> BindGroupEntry { unreachable!() })
                    .collect();
            }

            let tonemap_in_shader =
                camera.is_none_or(|camera| !camera.hdr) && tonemapping.is_enabled();
            let mut layout_key = MeshPipelineViewLayoutKey::from(*msaa)
                | MeshPipelineViewLayoutKey::from(prepass_textures);
            let mut offsets = ArrayVec::from_iter([
                view_uniform_offset.offset,
                view_lights_offset.offset,
                **view_light_probes_offset,
            ]);

            entries = entries.extend_with_indices((
                (0, view_binding.clone()),
                (1, light_binding.clone()),
                (2, &shadow_bindings.point_light_depth_texture_view),
                (3, &shadow_samplers.point_light_comparison_sampler),
                #[cfg(feature = "experimental_pbr_pcss")]
                (4, &shadow_samplers.point_light_linear_sampler),
                // NVForever ALCS far maps.
                #[cfg(all(
                    not(target_abi = "sim"),
                    any(
                        not(feature = "webgl"),
                        not(target_arch = "wasm32"),
                        feature = "webgpu"
                    )
                ))]
                (50, &shadow_bindings.nvf_point_light_far_depth_texture_view),
                (5, &shadow_bindings.directional_light_depth_texture_view),
                (6, &shadow_samplers.directional_light_comparison_sampler),
                #[cfg(feature = "experimental_pbr_pcss")]
                (7, &shadow_samplers.directional_light_linear_sampler),
                (8, clusterable_objects_binding.clone()),
                (
                    9,
                    cluster_bindings
                        .clusterable_object_index_lists_binding()
                        .unwrap(),
                ),
                (10, cluster_bindings.offsets_and_counts_binding().unwrap()),
                (11, globals.clone()),
                (12, light_probes_binding.clone()),
                (14, visibility_ranges_buffer.as_entire_binding()),
            ));

            if let Some(view_fog_offset) = view_fog_offset {
                layout_key |= MeshPipelineViewLayoutKey::DISTANCE_FOG;
                offsets.push(view_fog_offset.offset);
                entries =
                    entries.extend_with_indices(((13, fog_meta.gpu_fogs.binding().unwrap()),));
            }

            if let Some(view_ssr_offset) = view_ssr_offset {
                layout_key |= MeshPipelineViewLayoutKey::SCREEN_SPACE_REFLECTIONS;
                offsets.push(**view_ssr_offset);
                entries = entries.extend_with_indices(((15, ssr_buffer.binding().unwrap()),));
            }

            if let Some(view_contact_shadows_offset) = view_contact_shadows_offset {
                layout_key |= MeshPipelineViewLayoutKey::CONTACT_SHADOWS;
                offsets.push(**view_contact_shadows_offset);
                entries = entries
                    .extend_with_indices(((16, contact_shadows_buffer.0.binding().unwrap()),));
            }

            if let Some(view_oit_settings_offset) = view_oit_settings_offset {
                layout_key |= MeshPipelineViewLayoutKey::OIT_ENABLED;
                offsets.push(view_oit_settings_offset.offset);
                entries = entries.extend_with_indices((
                    (26, oit_buffers.settings.binding().unwrap()),
                    (27, oit_buffers.nodes_capacity.binding().unwrap()),
                    (28, oit_buffers.nodes.binding().unwrap()),
                    (29, oit_buffers.heads.binding().unwrap()),
                    (30, oit_buffers.atomic_counter.binding().unwrap()),
                ));
            }

            if has_atmosphere
                && let Some(atmosphere_textures) = atmosphere_textures
                && let Some(atmosphere_buffer) = atmosphere_buffer
                && let Some(atmosphere_sampler) = atmosphere_sampler.as_ref()
                && let Some(atmosphere_buffer_binding) = atmosphere_buffer.buffer.binding()
            {
                layout_key |= MeshPipelineViewLayoutKey::ATMOSPHERE;
                entries = entries.extend_with_indices((
                    (31, &atmosphere_textures.transmittance_lut.default_view),
                    (32, &***atmosphere_sampler),
                    (33, atmosphere_buffer_binding),
                ));
            }

            if cfg!(feature = "bluenoise_texture") {
                layout_key |= MeshPipelineViewLayoutKey::STBN;
                let stbn_view = &images
                    .get(&blue_noise.texture)
                    .expect("STBN texture is added unconditionally with at least a placeholder")
                    .texture_view;
                entries = entries.extend_with_indices(((34, stbn_view),));
            }

            if tonemap_in_shader {
                layout_key |= MeshPipelineViewLayoutKey::TONEMAP_IN_SHADER;
                let lut_bindings =
                    get_lut_bindings(&images, &tonemapping_luts, tonemapping, &fallback_image);
                entries = entries.extend_with_indices((
                    (TONEMAPPING_LUT_TEXTURE_BINDING_INDEX, lut_bindings.0),
                    (TONEMAPPING_LUT_SAMPLER_BINDING_INDEX, lut_bindings.1),
                ));
            }

            if let Some(ssao_resources) = ssao_resources {
                layout_key |= MeshPipelineViewLayoutKey::SCREEN_SPACE_AMBIENT_OCCLUSION;
                let ssao_view = &ssao_resources
                    .screen_space_ambient_occlusion_texture
                    .default_view;
                entries = entries.extend_with_indices(((17, ssao_view),));
            }

            if has_transmission {
                layout_key |= MeshPipelineViewLayoutKey::VIEW_TRANSMISSION_TEXTURE;
                let transmission_view = transmission_texture
                    .map(|transmission| &transmission.view)
                    .unwrap_or(&fallback_image_zero.texture_view);
                let transmission_sampler = transmission_texture
                    .map(|transmission| &transmission.sampler)
                    .unwrap_or(&fallback_image_zero.sampler);
                entries = entries
                    .extend_with_indices(((24, transmission_view), (25, transmission_sampler)));
            }

            // When using WebGL, we can't have a multisampled texture with `TEXTURE_BINDING`
            // See https://github.com/gfx-rs/wgpu/issues/5263
            let prepass_bindings;
            if cfg!(any(feature = "webgpu", not(target_arch = "wasm32"))) || msaa.samples() == 1 {
                prepass_bindings = prepass::get_bindings(prepass_textures);
                for (binding, index) in prepass_bindings
                    .iter()
                    .map(Option::as_ref)
                    .zip([20, 21, 22, 23])
                    .flat_map(|(b, i)| b.map(|b| (b, i)))
                {
                    entries = entries.extend_with_indices(((index, binding),));
                }
            };

            // LTC LUTs for area lights
            if cfg!(feature = "area_light_luts") {
                let (ltc_view, ltc_sampler) = images
                    .get(&area_light_luts.image)
                    .map(|img| (&img.texture_view, &img.sampler))
                    .unwrap_or((
                        &fallback_image.d2_array.texture_view,
                        &fallback_image.d2_array.sampler,
                    ));
                entries = entries.extend_with_indices(((35, ltc_view), (36, ltc_sampler)));
            }

            // DFG LUT
            if cfg!(feature = "dfg_lut") {
                let (dfg_view, dfg_sampler) = images
                    .get(&dfg_lut.texture)
                    .map(|img| (&img.texture_view, &img.sampler))
                    .unwrap_or((&fallback_image.d2.texture_view, &fallback_image.d2.sampler));
                entries = entries.extend_with_indices(((37, dfg_view), (38, dfg_sampler)));
            }

            // NVForever sun visibility: present exactly when `check_views_need_specialization`
            // gave the view's pipelines `MeshPipelineKey::NVF_SUN_VISIBILITY`.
            if let Some(sun) = nvf_sun_visibility {
                layout_key |= MeshPipelineViewLayoutKey::NVF_SUN_VISIBILITY;
                entries = entries.extend_with_indices((
                    (39, sun.uniform.as_entire_binding()),
                    (40, &sun.cloud_texture),
                    (41, &sun.terrain_texture),
                    (42, &sun.sampler),
                ));
                // Bindings 45-46 only in the layout that has them (the weather-only one
                // leaves them out).
                if sun.cloud_volume {
                    layout_key |= MeshPipelineViewLayoutKey::NVF_CLOUD_VOLUME;
                    entries = entries
                        .extend_with_indices(((45, &sun.light_near), (46, &sun.light_far)));
                }
            }

            // NVForever ambient occlusion: present exactly when
            // `check_views_need_specialization` gave the view's pipelines
            // `MeshPipelineKey::NVF_AMBIENT_OCCLUSION`.
            if let Some(ao) = nvf_ambient_occlusion {
                layout_key |= MeshPipelineViewLayoutKey::NVF_AMBIENT_OCCLUSION;
                entries = entries
                    .extend_with_indices(((43, ao.uniform.as_entire_binding()), (44, &ao.texture)));
            }

            // NVForever RTGI volume: present exactly when `check_views_need_specialization`
            // gave the view's pipelines `MeshPipelineKey::NVF_RTGI_VOLUME`.
            NVF_VIEW_BIND_GROUPS.fetch_add(1, Ordering::Relaxed);
            if let Some(volume) = nvf_rtgi_volume {
                NVF_RTGI_VIEW_BIND_GROUPS.fetch_add(1, Ordering::Relaxed);
                layout_key |= MeshPipelineViewLayoutKey::NVF_RTGI_VOLUME;
                entries = entries.extend_with_indices((
                    (47, volume.uniform.as_entire_binding()),
                    (48, &volume.texture),
                    (49, &volume.sampler),
                ));
            }

            let environment_map_bind_group_entries =
                render_view_environment_maps.map(|render_view_environment_maps| {
                    layout_key |= MeshPipelineViewLayoutKey::ENVIRONMENT_MAP;

                    RenderViewEnvironmentMapBindGroupEntries::get(
                        Some(render_view_environment_maps),
                        &images,
                        &fallback_image,
                        &render_device,
                        &render_adapter,
                    )
                });
            match environment_map_bind_group_entries {
                Some(RenderViewEnvironmentMapBindGroupEntries::Single {
                    diffuse_texture_view,
                    specular_texture_view,
                    sampler,
                }) => {
                    entries_binding_array = entries_binding_array.extend_with_indices((
                        (0, diffuse_texture_view),
                        (1, specular_texture_view),
                        (2, sampler),
                    ));
                }
                Some(RenderViewEnvironmentMapBindGroupEntries::Multiple {
                    ref diffuse_texture_views,
                    ref specular_texture_views,
                    sampler,
                }) => {
                    entries_binding_array = entries_binding_array.extend_with_indices((
                        (0, diffuse_texture_views.as_slice()),
                        (1, specular_texture_views.as_slice()),
                        (2, sampler),
                    ));
                }
                None => {}
            }

            let irradiance_volume_bind_group_entries =
                if render_view_irradiance_volumes.is_some() && IRRADIANCE_VOLUMES_ARE_USABLE {
                    layout_key |= MeshPipelineViewLayoutKey::IRRADIANCE_VOLUME;

                    Some(RenderViewIrradianceVolumeBindGroupEntries::get(
                        render_view_irradiance_volumes,
                        &images,
                        &fallback_image,
                        &render_device,
                        &render_adapter,
                    ))
                } else {
                    None
                };

            match irradiance_volume_bind_group_entries {
                Some(RenderViewIrradianceVolumeBindGroupEntries::Single {
                    texture_view,
                    sampler,
                }) => {
                    entries_binding_array = entries_binding_array
                        .extend_with_indices(((3, texture_view), (4, sampler)));
                }
                Some(RenderViewIrradianceVolumeBindGroupEntries::Multiple {
                    ref texture_views,
                    sampler,
                }) => {
                    entries_binding_array = entries_binding_array
                        .extend_with_indices(((3, texture_views.as_slice()), (4, sampler)));
                }
                None => {}
            }

            let decal_bind_group_entries = RenderViewClusteredDecalBindGroupEntries::get(
                &render_decals,
                &decals_buffer,
                &images,
                &fallback_image,
                &render_device,
                &render_adapter,
            );

            // Add the decal bind group entries.
            if let Some(ref render_view_decal_bind_group_entries) = decal_bind_group_entries {
                entries_binding_array = entries_binding_array.extend_with_indices((
                    // `clustered_decals`
                    (
                        5,
                        render_view_decal_bind_group_entries
                            .decals
                            .as_entire_binding(),
                    ),
                    // `clustered_decal_textures`
                    (
                        6,
                        render_view_decal_bind_group_entries
                            .texture_views
                            .as_slice(),
                    ),
                    // `clustered_decal_sampler`
                    (7, render_view_decal_bind_group_entries.sampler),
                ));
            }

            let layout = mesh_pipeline.get_view_layout(layout_key);
            commands.entity(entity).insert((MeshViewBindGroup {
                main_offsets: offsets,
                main: render_device.create_bind_group(
                    "mesh_view_bind_group",
                    &pipeline_cache.get_bind_group_layout(&layout.main_layout),
                    &entries,
                ),
                binding_array: render_device.create_bind_group(
                    "mesh_view_bind_group_binding_array",
                    &pipeline_cache.get_bind_group_layout(&layout.binding_array_layout),
                    &entries_binding_array,
                ),
                empty: render_device.create_bind_group(
                    "mesh_view_bind_group_empty",
                    &pipeline_cache.get_bind_group_layout(&layout.empty_layout),
                    &[],
                ),
            },));

            #[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
            {
                entries.entries.clear();
                entries_binding_array.entries.clear();
                *entries_cache = entries
                    .entries
                    .into_iter()
                    .map(|_| -> BindGroupEntry<'static> { unreachable!() })
                    .collect();
                *entries_binding_array_cache = entries_binding_array
                    .entries
                    .into_iter()
                    .map(|_| -> BindGroupEntry<'static> { unreachable!() })
                    .collect();
            }
        }
    }
}
