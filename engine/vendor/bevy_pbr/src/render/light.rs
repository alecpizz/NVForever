use crate::*;
use alloc::sync::Arc;
use bevy_asset::UntypedAssetId;
use bevy_camera::primitives::{
    face_index_to_name, CascadesFrusta, CubeMapFace, CubemapFrusta, Frustum, CUBE_MAP_FACES,
};
use bevy_camera::visibility::{
    CascadesVisibleEntities, CubemapVisibleEntities, RenderLayers, ViewVisibility,
    VisibleMeshEntities,
};
use bevy_camera::{Camera, Camera3d, RenderTarget, ShadowLodOrigin};
use bevy_color::ColorToComponents;
use bevy_core_pipeline::schedule::{NvfHostedView, RootNonCameraView};
use bevy_derive::{Deref, DerefMut};
use bevy_ecs::schedule::ScheduleLabel;
use bevy_ecs::system::SystemChangeTick;
use bevy_ecs::{
    entity::{EntityHashMap, EntityHashSet},
    prelude::*,
    system::{SystemParam, SystemState},
};
use bevy_light::cascade::Cascade;
use bevy_light::cluster::assign::{calculate_cluster_factors, ClusterableObjectType};
use bevy_light::SunDisk;
use bevy_light::{
    spot_light_clip_from_view, spot_light_world_from_view, AmbientLight, CascadeShadowConfig,
    Cascades, DirectionalLight, DirectionalLightShadowMap, GlobalAmbientLight, PointLight,
    PointLightShadowMap, RectLight, ShadowFilteringMethod, SpotLight, VolumetricLight,
};
use bevy_log::warn_once;
use bevy_material::{
    key::{ErasedMaterialPipelineKey, ErasedMeshPipelineKey},
    MaterialProperties,
};
use bevy_math::{ops, proj, Mat4, UVec4, Vec3, Vec3Swizzles, Vec4, Vec4Swizzles};
use bevy_mesh::{Mesh3d, MeshVertexBufferLayoutRef};
use bevy_platform::collections::{HashMap, HashSet};
use bevy_platform::hash::FixedHasher;
use bevy_render::batching::gpu_preprocessing::{
    BuildIndirectParametersMetadata, IndirectParametersBuffers,
};
use bevy_render::camera::{DirtySpecializations, PendingQueues};
use bevy_render::diagnostic::RecordDiagnostics;
use bevy_render::erased_render_asset::ErasedRenderAssets;
use bevy_render::mesh::allocator::MeshSlabs;
use bevy_render::occlusion_culling::{
    OcclusionCulling, OcclusionCullingSubview, OcclusionCullingSubviewEntities,
};
use bevy_render::sync_world::{MainEntity, MainEntityHashMap, MainEntityHashSet, RenderEntity};
use bevy_render::view::{
    RenderExtractedShadowMapVisibleEntities, RenderShadowLodOrigin, RenderShadowMapVisibleEntities,
    RenderVisibleEntities, RenderVisibleEntitiesClass, VisibilityExtractionSystemParam,
};
use bevy_render::{
    batching::gpu_preprocessing::{GpuPreprocessingMode, GpuPreprocessingSupport},
    camera::{ExtractedCamera, SortedCameras},
    mesh::allocator::MeshAllocator,
    view::{NoIndirectDrawing, RetainedViewEntity},
};
use bevy_render::{
    mesh::RenderMesh,
    render_asset::RenderAssets,
    render_phase::*,
    render_resource::*,
    renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
    texture::*,
    view::ExtractedView,
    Extract,
};
use bevy_shape::{HalfSpace, ViewFrustum};
use bevy_transform::{components::GlobalTransform, prelude::Transform};
use bevy_utils::default;
use core::{any::TypeId, hash::Hash, mem, ops::Range};
use decal::clustered::RenderClusteredDecals;
#[cfg(feature = "trace")]
use tracing::info_span;
use tracing::{error, warn};

#[derive(Component)]
#[require(PointAndSpotLightViewEntities)]
pub struct ExtractedPointLight {
    pub color: LinearRgba,
    /// luminous intensity in lumens per steradian
    pub intensity: f32,
    pub range: f32,
    pub radius: f32,
    pub transform: GlobalTransform,
    pub shadow_maps_enabled: bool,
    pub contact_shadows_enabled: bool,
    pub shadow_depth_bias: f32,
    pub shadow_normal_bias: f32,
    pub shadow_map_near_z: f32,
    pub spot_light_angles: Option<(f32, f32)>,
    pub volumetric: bool,
    pub soft_shadows_enabled: bool,
    /// whether this point light contributes diffuse light to lightmapped meshes
    pub affects_lightmapped_mesh_diffuse: bool,
}

#[derive(Component, Debug)]
pub struct ExtractedRectLight {
    pub color: LinearRgba,
    pub intensity: f32,
    pub range: f32,
    pub width: f32,
    pub height: f32,
    pub transform: GlobalTransform,
}

#[derive(Component, Debug)]
#[require(DirectionalLightViewEntities)]
pub struct ExtractedDirectionalLight {
    pub color: LinearRgba,
    pub illuminance: f32,
    pub transform: GlobalTransform,
    pub shadow_maps_enabled: bool,
    pub contact_shadows_enabled: bool,
    pub volumetric: bool,
    /// whether this directional light contributes diffuse light to lightmapped
    /// meshes
    pub affects_lightmapped_mesh_diffuse: bool,
    pub shadow_depth_bias: f32,
    pub shadow_normal_bias: f32,
    pub cascade_shadow_config: CascadeShadowConfig,
    pub cascades: EntityHashMap<Vec<Cascade>>,
    pub frusta: EntityHashMap<Vec<Frustum>>,
    pub soft_shadow_size: Option<f32>,
    /// True if this light is using two-phase occlusion culling.
    pub occlusion_culling: bool,
    pub sun_disk_angular_size: f32,
    pub sun_disk_intensity: f32,
}

// NOTE: These must match the bit flags in bevy_pbr/src/render/mesh_view_types.wesl!
bitflags::bitflags! {
    #[repr(transparent)]
    struct PointLightFlags: u32 {
        const SHADOW_MAPS_ENABLED               = 1 << 0;
        const SPOT_LIGHT_Y_NEGATIVE             = 1 << 1;
        const VOLUMETRIC                        = 1 << 2;
        const AFFECTS_LIGHTMAPPED_MESH_DIFFUSE  = 1 << 3;
        const CONTACT_SHADOWS_ENABLED           = 1 << 4;
        const SPOT_LIGHT                        = 1 << 5;
        const RECT_LIGHT                        = 1 << 6;
        const NONE                              = 0;
        const UNINITIALIZED                     = 0xFFFF;
    }
}

#[derive(Copy, Clone, ShaderType, Default, Debug)]
pub struct GpuDirectionalCascade {
    clip_from_world: Mat4,
    texel_size: f32,
    far_bound: f32,
}

#[derive(Copy, Clone, ShaderType, Default, Debug)]
pub struct GpuDirectionalLight {
    cascades: [GpuDirectionalCascade; MAX_CASCADES_PER_LIGHT],
    color: Vec4,
    dir_to_light: Vec3,
    flags: u32,
    soft_shadow_size: f32,
    shadow_depth_bias: f32,
    shadow_normal_bias: f32,
    num_cascades: u32,
    cascades_overlap_proportion: f32,
    depth_texture_base_index: u32,
    decal_index: u32,
    sun_disk_angular_size: f32,
    sun_disk_intensity: f32,
}

// NOTE: These must match the bit flags in bevy_pbr/src/render/mesh_view_types.wesl!
bitflags::bitflags! {
    #[repr(transparent)]
    struct DirectionalLightFlags: u32 {
        const SHADOW_MAPS_ENABLED               = 1 << 0;
        const VOLUMETRIC                        = 1 << 1;
        const AFFECTS_LIGHTMAPPED_MESH_DIFFUSE  = 1 << 2;
        const CONTACT_SHADOWS_ENABLED           = 1 << 3;
        // NVForever sun visibility: the light is the outdoor sun ([`NvfOutdoorSun`]).
        const NVF_OUTDOOR_SUN                   = 1 << 4;
        const NONE                              = 0;
        const UNINITIALIZED                     = 0xFFFF;
    }
}

/// NVForever sun visibility (2026-11-17, engine/vendor/README.md). On the render-world
/// entity of a directional light, it marks that light as the outdoor sun:
/// `prepare_lights` sets `DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT` in its GPU flags,
/// so a shader can tell it from every other directional light without assuming an index.
/// Bevy inserts none; without it the flags are as upstream's.
#[derive(Component, Clone, Copy, Default, Debug)]
pub struct NvfOutdoorSun;

#[derive(Copy, Clone, ShaderType, Default, Debug)]
pub struct GpuRectLight {
    color: Vec4,
    position: Vec3,
    width: f32,
    right: Vec3,
    height: f32,
    up: Vec3,
    range: f32,
}

// NOTE: These must match the bit flags in bevy_pbr/src/render/mesh_view_types.wesl!
bitflags::bitflags! {
    #[repr(transparent)]
    struct AmbientLightFlags: u32 {
        const AFFECTS_LIGHTMAPPED_MESHES        = 1 << 0;
        const NONE                              = 0;
        const UNINITIALIZED                     = 0xFFFF;
    }
}

#[derive(Copy, Clone, Debug, ShaderType)]
pub struct GpuLights {
    directional_lights: [GpuDirectionalLight; MAX_DIRECTIONAL_LIGHTS],
    ambient_color: Vec4,
    // xyz are x/y/z cluster dimensions and w is the number of clusters
    cluster_dimensions: UVec4,
    // xy are vec2<f32>(cluster_dimensions.xy) / vec2<f32>(view.width, view.height)
    // z is cluster_dimensions.z / log(far / near)
    // w is cluster_dimensions.z * log(near) / log(far / near)
    cluster_factors: Vec4,
    n_directional_lights: u32,
    // offset from spot light's light index to spot light's shadow map index
    spot_light_shadowmap_offset: i32,
    ambient_light_flags: u32,
    // this is unused if we have access to storage buffers, in which case rect lights are clustered
    n_rect_lights: u32,
    rect_lights: [GpuRectLight; MAX_RECT_LIGHTS],
}

// NOTE: When running bevy on Adreno GPU chipsets in WebGL, any value above 1 will result in a crash
// when loading "pbr_functions.wesl" in the function apply_fog.
#[cfg(all(feature = "webgl", target_arch = "wasm32", not(feature = "webgpu")))]
pub const MAX_DIRECTIONAL_LIGHTS: usize = 1;
#[cfg(any(
    not(feature = "webgl"),
    not(target_arch = "wasm32"),
    feature = "webgpu"
))]
pub const MAX_DIRECTIONAL_LIGHTS: usize = 10;
#[cfg(any(
    not(feature = "webgl"),
    not(target_arch = "wasm32"),
    feature = "webgpu"
))]
pub const MAX_CASCADES_PER_LIGHT: usize = 4;
#[cfg(all(feature = "webgl", target_arch = "wasm32", not(feature = "webgpu")))]
pub const MAX_CASCADES_PER_LIGHT: usize = 1;

pub const MAX_RECT_LIGHTS: usize = 8;

#[derive(Resource, Clone)]
pub struct ShadowSamplers {
    pub point_light_comparison_sampler: Sampler,
    #[cfg(feature = "experimental_pbr_pcss")]
    pub point_light_linear_sampler: Sampler,
    pub directional_light_comparison_sampler: Sampler,
    #[cfg(feature = "experimental_pbr_pcss")]
    pub directional_light_linear_sampler: Sampler,
}

pub fn init_shadow_samplers(mut commands: Commands, render_device: Res<RenderDevice>) {
    let base_sampler_descriptor = SamplerDescriptor {
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        mipmap_filter: MipmapFilterMode::Nearest,
        ..default()
    };

    commands.insert_resource(ShadowSamplers {
        point_light_comparison_sampler: render_device.create_sampler(&SamplerDescriptor {
            compare: Some(CompareFunction::GreaterEqual),
            ..base_sampler_descriptor
        }),
        #[cfg(feature = "experimental_pbr_pcss")]
        point_light_linear_sampler: render_device.create_sampler(&base_sampler_descriptor),
        directional_light_comparison_sampler: render_device.create_sampler(&SamplerDescriptor {
            compare: Some(CompareFunction::GreaterEqual),
            ..base_sampler_descriptor
        }),
        #[cfg(feature = "experimental_pbr_pcss")]
        directional_light_linear_sampler: render_device.create_sampler(&base_sampler_descriptor),
    });
}

// This is needed because of the orphan rule not allowing implementing
// foreign trait ExtractComponent on foreign type ShadowFilteringMethod
pub fn extract_shadow_filtering_method(
    mut commands: Commands,
    mut previous_len: Local<usize>,
    query: Extract<Query<(RenderEntity, &ShadowFilteringMethod)>>,
) {
    let mut values = Vec::with_capacity(*previous_len);
    for (entity, query_item) in &query {
        values.push((entity, *query_item));
    }
    *previous_len = values.len();
    commands.try_insert_batch(values);
}

// This is needed because of the orphan rule not allowing implementing
// foreign trait ExtractResource on foreign type AmbientLight
pub fn extract_ambient_light_resource(
    mut commands: Commands,
    main_resource: Extract<Option<Res<GlobalAmbientLight>>>,
    target_resource: Option<ResMut<GlobalAmbientLight>>,
) {
    if let Some(main_resource) = main_resource.as_ref() {
        if let Some(mut target_resource) = target_resource {
            if main_resource.is_changed() {
                *target_resource = (*main_resource).clone();
            }
        } else {
            commands.insert_resource((*main_resource).clone());
        }
    }
}

// This is needed because of the orphan rule not allowing implementing
// foreign trait ExtractComponent on foreign type AmbientLight
pub fn extract_ambient_light(
    mut commands: Commands,
    mut previous_len: Local<usize>,
    query: Extract<Query<(RenderEntity, &AmbientLight)>>,
) {
    let mut values = Vec::with_capacity(*previous_len);
    for (entity, query_item) in &query {
        values.push((entity, query_item.clone()));
    }
    *previous_len = values.len();
    commands.try_insert_batch(values);
}

pub fn extract_lights(
    mut commands: Commands,
    point_light_shadow_map: Extract<Res<PointLightShadowMap>>,
    directional_light_shadow_map: Extract<Res<DirectionalLightShadowMap>>,
    point_lights: Extract<
        Query<
            (
                Entity,
                RenderEntity,
                &PointLight,
                &CubemapVisibleEntities,
                &GlobalTransform,
                &ViewVisibility,
                &CubemapFrusta,
                Option<&RenderLayers>,
                Option<&VolumetricLight>,
            ),
            Or<(
                Changed<PointLight>,
                Changed<CubemapVisibleEntities>,
                Changed<GlobalTransform>,
                Changed<ViewVisibility>,
                Changed<RenderLayers>,
                Changed<CubemapFrusta>,
                Changed<VolumetricLight>,
            )>,
        >,
    >,
    spot_lights: Extract<
        Query<
            (
                Entity,
                RenderEntity,
                &SpotLight,
                &VisibleMeshEntities,
                &GlobalTransform,
                &ViewVisibility,
                &Frustum,
                Option<&RenderLayers>,
                Option<&VolumetricLight>,
            ),
            Or<(
                Changed<SpotLight>,
                Changed<VisibleMeshEntities>,
                Changed<GlobalTransform>,
                Changed<ViewVisibility>,
                Changed<RenderLayers>,
                Changed<Frustum>,
                Changed<VolumetricLight>,
            )>,
        >,
    >,
    directional_lights: Extract<
        Query<
            (
                Entity,
                RenderEntity,
                &DirectionalLight,
                &CascadesVisibleEntities,
                &Cascades,
                &CascadeShadowConfig,
                &CascadesFrusta,
                &GlobalTransform,
                &ViewVisibility,
                Option<&RenderLayers>,
                Option<&VolumetricLight>,
                Has<OcclusionCulling>,
                Option<&SunDisk>,
            ),
            (
                Without<SpotLight>,
                Or<(
                    Changed<DirectionalLight>,
                    Changed<CascadesVisibleEntities>,
                    Changed<Cascades>,
                    Changed<CascadeShadowConfig>,
                    Changed<CascadesFrusta>,
                    Changed<GlobalTransform>,
                    Changed<ViewVisibility>,
                    Changed<RenderLayers>,
                    Changed<VolumetricLight>,
                    Changed<OcclusionCulling>,
                    Changed<SunDisk>,
                )>,
            ),
        >,
    >,
    rect_lights: Extract<
        Query<
            (
                Entity,
                RenderEntity,
                &RectLight,
                &GlobalTransform,
                &ViewVisibility,
                Option<&RenderLayers>,
            ),
            Or<(
                Changed<RectLight>,
                Changed<GlobalTransform>,
                Changed<ViewVisibility>,
                Changed<RenderLayers>,
            )>,
        >,
    >,
    visibility_extraction_system_param: VisibilityExtractionSystemParam,
    mut existing_render_shadow_map_visible_entities: Query<(
        Entity,
        &mut RenderExtractedShadowMapVisibleEntities,
        &mut RenderShadowMapVisibleEntities,
    )>,
    mut all_lights_found: Local<EntityHashSet>,
    mut rect_light_missing_luts_warning_emitted: Local<bool>,
    system_change_tick: SystemChangeTick,
    // NVForever hidden light views: the views a hidden light takes with it, counted.
    nvf_light_views: Query<&PointAndSpotLightViewEntities>,
) {
    // NVForever light view counts: the system's time.
    let nvf_start = nvf_counting().then(std::time::Instant::now);
    let mapper = &visibility_extraction_system_param.mapper;

    // NOTE: These shadow map resources are extracted here as they are used here too so this avoids
    // races between scheduling of ExtractResourceSystems and this system.
    if point_light_shadow_map.is_changed() {
        commands.insert_resource(point_light_shadow_map.clone());
    }
    if directional_light_shadow_map.is_changed() {
        commands.insert_resource(directional_light_shadow_map.clone());
    }

    // This is the point light shadow map texel size for one face of the cube as a distance of 1.0
    // world unit from the light.
    // point_light_texel_size = 2.0 * 1.0 * tan(PI / 4.0) / cube face width in texels
    // PI / 4.0 is half the cube face fov, tan(PI / 4.0) = 1.0, so this simplifies to:
    // point_light_texel_size = 2.0 / cube face width in texels
    // NOTE: When using various PCF kernel sizes, this will need to be adjusted, according to:
    // https://catlikecoding.com/unity/tutorials/custom-srp/point-and-spot-shadows/
    let point_light_texel_size = 2.0 / point_light_shadow_map.size as f32;

    all_lights_found.clear();

    for (
        main_entity,
        render_entity,
        point_light,
        cubemap_visible_entities,
        transform,
        view_visibility,
        frusta,
        maybe_render_layers,
        volumetric_light,
    ) in point_lights.iter()
    {
        if !view_visibility.get() {
            if let Ok(mut entity_commands) = commands.get_entity(render_entity) {
                // NVForever hidden light views (bevyengine/bevy#26059, the fix of the open
                // bevyengine/bevy#26480): its shadow views go with the light, through the
                // observer `remove_point_and_spot_light_view_entities`. Upstream removed only
                // `ExtractedPointLight`, so `prepare_lights`, which despawns the views of a
                // light no longer shadowed, never saw the light again and its views kept
                // running the Core3d schedule every frame while it was hidden. Only while
                // the game's hold is active (`HoldLightShadowMaps::active`, as last frame's
                // `prepare_lights` saw it); otherwise upstream's.
                if nvf_counting() {
                    nvf_count_hidden_views(&nvf_light_views, render_entity);
                    entity_commands.remove::<(ExtractedPointLight, PointAndSpotLightViewEntities)>();
                } else {
                    entity_commands.remove::<ExtractedPointLight>();
                }
            }
            continue;
        }

        if point_light.shadow_maps_enabled {
            // Fetch or create the visible entities for each cubemap face.
            let (
                mut render_extracted_shadow_map_visible_entities,
                mut render_shadow_map_visible_entities,
            ) = match existing_render_shadow_map_visible_entities.get_mut(render_entity) {
                Ok((
                    _,
                    ref mut existing_extracted_shadow_map_visible_entities,
                    ref mut existing_shadow_map_visible_entities,
                )) => (
                    mem::take(&mut **existing_extracted_shadow_map_visible_entities),
                    mem::take(&mut **existing_shadow_map_visible_entities),
                ),
                Err(_) => (
                    RenderExtractedShadowMapVisibleEntities::default(),
                    RenderShadowMapVisibleEntities::default(),
                ),
            };

            all_lights_found.insert(render_entity);

            for face_index in 0..6 {
                let retained_view_entity = RetainedViewEntity {
                    main_entity: MainEntity::from(main_entity),
                    auxiliary_entity: None,
                    subview_index: face_index,
                };
                render_shadow_map_visible_entities
                    .subviews
                    .entry(retained_view_entity)
                    .or_insert_with(|| (system_change_tick.this_run(), Default::default()));

                // Extract the visible entities to the list for this face.
                let extracted_entities = &mut render_extracted_shadow_map_visible_entities
                    .subviews
                    .entry(retained_view_entity)
                    .or_default()
                    .classes
                    .entry(TypeId::of::<Mesh3d>())
                    .or_default()
                    .entities;
                extracted_entities.clear();
                let visible_mesh_entities = cubemap_visible_entities.get(face_index as usize);
                extracted_entities.extend(visible_mesh_entities.entities.iter().map(
                    |main_entity| {
                        let render_entity = match mapper.get(*main_entity) {
                            Ok(render_entity) => **render_entity,
                            Err(_) => Entity::PLACEHOLDER,
                        };
                        (render_entity, MainEntity::from(*main_entity))
                    },
                ));
            }

            // Clear out visible entity lists corresponding to subviews that no
            // longer exist.
            render_extracted_shadow_map_visible_entities
                .subviews
                .retain(|view_entity, _| {
                    view_entity.main_entity.entity() == main_entity
                        && view_entity.auxiliary_entity.is_none()
                        && view_entity.subview_index < 6
                });
            render_shadow_map_visible_entities
                .subviews
                .retain(|view_entity, _| {
                    view_entity.main_entity.entity() == main_entity
                        && view_entity.auxiliary_entity.is_none()
                        && view_entity.subview_index < 6
                });

            let mut entity_commands = commands.entity(render_entity);
            entity_commands.insert((
                render_extracted_shadow_map_visible_entities,
                render_shadow_map_visible_entities,
            ));
        }

        let mut entity_commands = commands.entity(render_entity);
        let extracted_point_light = ExtractedPointLight {
            color: point_light.color.into(),
            // NOTE: Map from luminous power in lumens to luminous intensity in lumens per steradian
            // for a point light. See https://google.github.io/filament/Filament.md.html#mjx-eqn-pointLightLuminousPower
            // for details.
            intensity: point_light.intensity / (4.0 * core::f32::consts::PI),
            range: point_light.range,
            radius: point_light.radius,
            transform: *transform,
            shadow_maps_enabled: point_light.shadow_maps_enabled,
            contact_shadows_enabled: point_light.contact_shadows_enabled,
            shadow_depth_bias: point_light.shadow_depth_bias,
            // The factor of SQRT_2 is for the worst-case diagonal offset
            shadow_normal_bias: point_light.shadow_normal_bias
                * point_light_texel_size
                * core::f32::consts::SQRT_2,
            shadow_map_near_z: point_light.shadow_map_near_z,
            spot_light_angles: None,
            volumetric: volumetric_light.is_some(),
            affects_lightmapped_mesh_diffuse: point_light.affects_lightmapped_mesh_diffuse,
            #[cfg(feature = "experimental_pbr_pcss")]
            soft_shadows_enabled: point_light.soft_shadows_enabled,
            #[cfg(not(feature = "experimental_pbr_pcss"))]
            soft_shadows_enabled: false,
        };
        entity_commands.insert((
            extracted_point_light,
            (*frusta).clone(),
            MainEntity::from(main_entity),
            maybe_render_layers.unwrap_or_default().clone(),
        ));
    }

    for (
        main_entity,
        render_entity,
        spot_light,
        visible_entities,
        transform,
        view_visibility,
        frustum,
        maybe_render_layers,
        volumetric_light,
    ) in spot_lights.iter()
    {
        if !view_visibility.get() {
            if let Ok(mut entity_commands) = commands.get_entity(render_entity) {
                // NVForever hidden light views: as for the point lights above.
                if nvf_counting() {
                    nvf_count_hidden_views(&nvf_light_views, render_entity);
                    entity_commands.remove::<(ExtractedPointLight, PointAndSpotLightViewEntities)>();
                } else {
                    entity_commands.remove::<ExtractedPointLight>();
                }
            }
            continue;
        }

        if spot_light.shadow_maps_enabled {
            // Fetch or create the visible entities.
            let (
                mut render_extracted_shadow_map_visible_entities,
                mut render_shadow_map_visible_entities,
            ) = match existing_render_shadow_map_visible_entities.get_mut(render_entity) {
                Ok((
                    _,
                    ref mut existing_extracted_shadow_map_visible_entities,
                    ref mut existing_shadow_map_visible_entities,
                )) => (
                    mem::take(&mut **existing_extracted_shadow_map_visible_entities),
                    mem::take(&mut **existing_shadow_map_visible_entities),
                ),
                Err(_) => (
                    RenderExtractedShadowMapVisibleEntities::default(),
                    RenderShadowMapVisibleEntities::default(),
                ),
            };

            all_lights_found.insert(render_entity);

            let retained_view_entity = RetainedViewEntity {
                main_entity: MainEntity::from(main_entity),
                auxiliary_entity: None,
                subview_index: 0,
            };
            render_shadow_map_visible_entities
                .subviews
                .entry(retained_view_entity)
                .or_insert_with(|| (system_change_tick.this_run(), Default::default()));

            // Extract the visible CPU culled entities to the list.
            let entities_cpu_culling = &mut render_extracted_shadow_map_visible_entities
                .subviews
                .entry(retained_view_entity)
                .or_default()
                .classes
                .entry(TypeId::of::<Mesh3d>())
                .or_default()
                .entities;
            entities_cpu_culling.clear();
            entities_cpu_culling.extend(visible_entities.entities.iter().map(|main_entity| {
                let render_entity = match mapper.get(*main_entity) {
                    Ok(render_entity) => **render_entity,
                    Err(_) => Entity::PLACEHOLDER,
                };
                (render_entity, MainEntity::from(*main_entity))
            }));

            // Clear out visible entity lists corresponding to subviews that no
            // longer exist.
            render_extracted_shadow_map_visible_entities
                .subviews
                .retain(|view_entity, _| retained_view_entity == *view_entity);
            render_shadow_map_visible_entities
                .subviews
                .retain(|view_entity, _| retained_view_entity == *view_entity);

            let mut entity_commands = commands.entity(render_entity);
            entity_commands.insert((
                render_extracted_shadow_map_visible_entities,
                render_shadow_map_visible_entities,
            ));
        }

        // `outer_angle` must be <π/2.
        let (inner_angle, outer_angle) = {
            let mut outer_angle = spot_light.outer_angle;
            if outer_angle >= core::f32::consts::FRAC_PI_2 {
                warn_once!(
                    "A `SpotLight` has `outer_angle` {} >= π/2 (~1.57079), clamping it to just below π/2. \
                     Spot lights require `outer_angle < π/2`",
                    outer_angle
                );
                outer_angle = core::f32::consts::FRAC_PI_2 - 1e-4;
            }
            // Keep inner_angle <= outer_angle after clamping.
            (spot_light.inner_angle.min(outer_angle), outer_angle)
        };

        let texel_size = 2.0 * ops::tan(outer_angle) / directional_light_shadow_map.size as f32;

        let mut entity_commands = commands.entity(render_entity);
        let extracted_spot_light = ExtractedPointLight {
            color: spot_light.color.into(),
            // NOTE: Map from luminous power in lumens to luminous intensity in lumens per steradian
            // for a point light. See https://google.github.io/filament/Filament.md.html#mjx-eqn-pointLightLuminousPower
            // for details.
            // Note: Filament uses a divisor of PI for spot lights. We choose to use the same 4*PI divisor
            // in both cases so that toggling between point light and spot light keeps lit areas lit equally,
            // which seems least surprising for users
            intensity: spot_light.intensity / (4.0 * core::f32::consts::PI),
            range: spot_light.range,
            radius: spot_light.radius,
            transform: *transform,
            shadow_maps_enabled: spot_light.shadow_maps_enabled,
            contact_shadows_enabled: spot_light.contact_shadows_enabled,
            shadow_depth_bias: spot_light.shadow_depth_bias,
            // The factor of SQRT_2 is for the worst-case diagonal offset
            shadow_normal_bias: spot_light.shadow_normal_bias
                * texel_size
                * core::f32::consts::SQRT_2,
            shadow_map_near_z: spot_light.shadow_map_near_z,
            spot_light_angles: Some((inner_angle, outer_angle)),
            volumetric: volumetric_light.is_some(),
            affects_lightmapped_mesh_diffuse: spot_light.affects_lightmapped_mesh_diffuse,
            #[cfg(feature = "experimental_pbr_pcss")]
            soft_shadows_enabled: spot_light.soft_shadows_enabled,
            #[cfg(not(feature = "experimental_pbr_pcss"))]
            soft_shadows_enabled: false,
        };
        entity_commands.insert((
            extracted_spot_light,
            *frustum,
            MainEntity::from(main_entity),
            maybe_render_layers.unwrap_or_default().clone(),
        ));
    }

    for (
        main_entity,
        render_entity,
        directional_light,
        visible_entities,
        cascades,
        cascade_config,
        frusta,
        transform,
        view_visibility,
        maybe_render_layers,
        volumetric_light,
        occlusion_culling,
        sun_disk,
    ) in &directional_lights
    {
        if !view_visibility.get() {
            commands
                .get_entity(render_entity)
                .expect("Light entity wasn't synced.")
                .remove::<(
                    ExtractedDirectionalLight,
                    RenderExtractedShadowMapVisibleEntities,
                )>();
            continue;
        }

        // TODO: update in place instead of reinserting.
        let mut extracted_cascades = EntityHashMap::default();
        let mut extracted_frusta = EntityHashMap::default();

        if directional_light.shadow_maps_enabled {
            // Fetch or create the visible entities set for each cascade.
            let (
                mut existing_extracted_shadow_map_visible_entities,
                mut existing_shadow_map_visible_entities,
            ) = match existing_render_shadow_map_visible_entities.get_mut(render_entity) {
                Ok((
                    _,
                    ref mut existing_extracted_shadow_map_visible_entities,
                    ref mut existing_shadow_map_visible_entities,
                )) => (
                    mem::take(&mut **existing_extracted_shadow_map_visible_entities),
                    mem::take(&mut **existing_shadow_map_visible_entities),
                ),
                Err(_) => (
                    RenderExtractedShadowMapVisibleEntities::default(),
                    RenderShadowMapVisibleEntities::default(),
                ),
            };

            all_lights_found.insert(render_entity);

            for (e, v) in cascades.cascades.iter() {
                if let Ok(entity) = mapper.get(*e) {
                    extracted_cascades.insert(**entity, v.clone());
                }
            }
            for (e, v) in frusta.frusta.iter() {
                if let Ok(entity) = mapper.get(*e) {
                    extracted_frusta.insert(**entity, v.clone());
                }
            }
            // Calculate the added and removed entities for each cascade.
            let mut all_cascades_seen = HashSet::new();
            for (main_auxiliary_entity, visible_mesh_entities_list) in
                visible_entities.entities.iter()
            {
                for subview_index in 0..(cascade_config.bounds.len() as u32) {
                    let retained_view_entity = RetainedViewEntity {
                        main_entity: MainEntity::from(main_entity),
                        auxiliary_entity: (*main_auxiliary_entity != Entity::PLACEHOLDER)
                            .then_some(MainEntity::from(*main_auxiliary_entity)),
                        subview_index,
                    };
                    all_cascades_seen.insert(retained_view_entity);

                    existing_shadow_map_visible_entities
                        .subviews
                        .entry(retained_view_entity)
                        .or_insert_with(|| (system_change_tick.this_run(), Default::default()));

                    // Extract the visible CPU culled entities to the list.
                    let extracted_entities = &mut existing_extracted_shadow_map_visible_entities
                        .subviews
                        .entry(retained_view_entity)
                        .or_default()
                        .classes
                        .entry(TypeId::of::<Mesh3d>())
                        .or_default()
                        .entities;
                    extracted_entities.clear();
                    let Some(visible_mesh_entities) =
                        visible_mesh_entities_list.get(subview_index as usize)
                    else {
                        continue;
                    };
                    extracted_entities.extend(visible_mesh_entities.entities.iter().map(
                        |main_entity| {
                            let render_entity = match mapper.get(*main_entity) {
                                Ok(render_entity) => **render_entity,
                                Err(_) => Entity::PLACEHOLDER,
                            };
                            (render_entity, MainEntity::from(*main_entity))
                        },
                    ));
                }
            }

            // Clear out visible entity lists corresponding to cascades that no
            // longer exist.
            existing_extracted_shadow_map_visible_entities
                .subviews
                .retain(|cascade_entity, _| all_cascades_seen.contains(cascade_entity));
            existing_shadow_map_visible_entities
                .subviews
                .retain(|cascade_entity, _| all_cascades_seen.contains(cascade_entity));

            let mut entity_commands = commands.entity(render_entity);
            entity_commands.insert((
                existing_extracted_shadow_map_visible_entities,
                existing_shadow_map_visible_entities,
            ));
        }

        let extracted_directional_light = ExtractedDirectionalLight {
            color: directional_light.color.into(),
            illuminance: directional_light.illuminance,
            transform: *transform,
            volumetric: volumetric_light.is_some(),
            affects_lightmapped_mesh_diffuse: directional_light.affects_lightmapped_mesh_diffuse,
            #[cfg(feature = "experimental_pbr_pcss")]
            soft_shadow_size: directional_light.soft_shadow_size,
            #[cfg(not(feature = "experimental_pbr_pcss"))]
            soft_shadow_size: None,
            shadow_maps_enabled: directional_light.shadow_maps_enabled,
            contact_shadows_enabled: directional_light.contact_shadows_enabled,
            shadow_depth_bias: directional_light.shadow_depth_bias,
            // The factor of SQRT_2 is for the worst-case diagonal offset
            shadow_normal_bias: directional_light.shadow_normal_bias * core::f32::consts::SQRT_2,
            cascade_shadow_config: cascade_config.clone(),
            cascades: extracted_cascades,
            frusta: extracted_frusta,
            occlusion_culling,
            sun_disk_angular_size: sun_disk.unwrap_or_default().angular_size,
            sun_disk_intensity: sun_disk.unwrap_or_default().intensity,
        };

        let mut entity_commands = commands
            .get_entity(render_entity)
            .expect("Light entity wasn't synced.");
        entity_commands.insert((
            extracted_directional_light,
            MainEntity::from(main_entity),
            maybe_render_layers.unwrap_or_default().clone(),
        ));
    }

    for (main_entity, render_entity, rect_light, transform, view_visibility, maybe_render_layers) in
        &rect_lights
    {
        if !cfg!(feature = "area_light_luts") && !*rect_light_missing_luts_warning_emitted {
            warn!(
                "RectLight will not work properly because the `area_light_luts` cargo feature is not enabled."
            );
            *rect_light_missing_luts_warning_emitted = true;
        }
        if !view_visibility.get() {
            if let Ok(mut entity_commands) = commands.get_entity(render_entity) {
                entity_commands.remove::<ExtractedRectLight>();
            }
            continue;
        }

        all_lights_found.insert(render_entity);

        let affine = transform.affine();
        let effective_width = rect_light.width * affine.matrix3.x_axis.length();
        let effective_height = rect_light.height * affine.matrix3.y_axis.length();
        let mut entity_commands = commands
            .get_entity(render_entity)
            .expect("RectLight entity wasn't synced.");
        entity_commands.insert((
            ExtractedRectLight {
                color: rect_light.color.into(),
                intensity: rect_light.intensity
                    / (effective_width * effective_height * core::f32::consts::PI),
                width: effective_width,
                height: effective_height,
                range: rect_light.range,
                transform: *transform,
            },
            MainEntity::from(main_entity),
            maybe_render_layers.unwrap_or_default().clone(),
        ));
    }

    // Clear out any shadow maps that correspond to lights that have become
    // invisible or have their shadow mapping turned off.
    for (render_entity, _, _) in existing_render_shadow_map_visible_entities
        .iter()
        .filter(|(render_entity, _, _)| !all_lights_found.contains(render_entity))
    {
        let Ok(mut entity_commands) = commands.get_entity(render_entity) else {
            return;
        };
        entity_commands.remove::<(
            RenderExtractedShadowMapVisibleEntities,
            RenderShadowMapVisibleEntities,
        )>();
    }
    // NVForever light view counts.
    if let Some(start) = nvf_start {
        nvf_time(NVF_EXTRACT, start.elapsed());
    }
}

#[derive(Component, Default, Deref, DerefMut)]
/// Component automatically attached to a light entity to track light-view entities
/// for each view.
pub struct DirectionalLightViewEntities(EntityHashMap<Vec<Entity>>);

pub(crate) fn remove_light_view_entities(
    remove: On<Remove<DirectionalLightViewEntities>>,
    query: Query<&DirectionalLightViewEntities>,
    mut commands: Commands,
) {
    if let Ok(entities) = query.get(remove.entity) {
        for v in entities.0.values() {
            for e in v.iter().copied() {
                if let Ok(mut v) = commands.get_entity(e) {
                    v.despawn();
                }
            }
        }
    }
}

pub(crate) fn remove_point_and_spot_light_view_entities(
    remove: On<Remove<PointAndSpotLightViewEntities>>,
    query: Query<&PointAndSpotLightViewEntities>,
    mut commands: Commands,
) {
    if let Ok(entities) = query.get(remove.entity) {
        for e in entities.0.iter().copied() {
            if let Ok(mut v) = commands.get_entity(e) {
                v.despawn();
            }
        }
    }
}

/// A component that stores the shadow maps associated with a point or spot
/// light.
///
/// This component is placed on the light, because these types of shadow maps
/// aren't associated with views.
#[derive(Component, Default, Deref, DerefMut)]
pub struct PointAndSpotLightViewEntities(Vec<Entity>);

#[derive(Component)]
pub struct ShadowView {
    pub depth_attachment: DepthStencilViewAttachment,
    pub pass_name: String,
}

#[derive(Component)]
pub struct ViewShadowBindings {
    pub point_light_depth_texture: Texture,
    pub point_light_depth_texture_view: TextureView,
    /// NVForever ALCS far maps: the far cube texture's array view (the first texture's
    /// while there is none).
    pub nvf_point_light_far_depth_texture_view: TextureView,
    pub directional_light_depth_texture: Texture,
    pub directional_light_depth_texture_view: TextureView,
}

/// A component that holds the shadow cascade views for all shadow cascades
/// associated with a camera.
///
/// Note: Despite the name, this component actually holds the shadow cascade
/// views, not the lights themselves.
#[derive(Component)]
pub struct ViewLightEntities {
    /// The shadow cascade views for all shadow cascades associated with a
    /// camera.
    ///
    /// Note: Despite the name, this component actually holds the shadow cascade
    /// views, not the lights themselves.
    pub lights: Vec<Entity>,
}

#[derive(Component)]
pub struct ViewLightsUniformOffset {
    pub offset: u32,
}

#[derive(Resource, Default)]
pub struct LightMeta {
    pub view_gpu_lights: DynamicUniformBuffer<GpuLights>,
}

#[derive(Component)]
pub enum LightEntity {
    Directional {
        light_entity: Entity,
        cascade_index: usize,
    },
    Point {
        light_entity: Entity,
        face_index: usize,
    },
    Spot {
        light_entity: Entity,
    },
}

pub fn prepare_lights(
    mut commands: Commands,
    mut texture_cache: ResMut<TextureCache>,
    (render_device, render_queue): (Res<RenderDevice>, Res<RenderQueue>),
    mut global_clusterable_object_meta: ResMut<GlobalClusterableObjectMeta>,
    mut light_meta: ResMut<LightMeta>,
    views: Query<
        (
            Entity,
            MainEntity,
            &ExtractedView,
            &ExtractedClusterConfig,
            Option<&RenderLayers>,
            Has<NoIndirectDrawing>,
            Option<&AmbientLight>,
        ),
        With<Camera3d>,
    >,
    ambient_light: Res<GlobalAmbientLight>,
    point_light_shadow_map: Res<PointLightShadowMap>,
    directional_light_shadow_map: Res<DirectionalLightShadowMap>,
    mut shadow_render_phases: ResMut<ViewBinnedRenderPhases<Shadow>>,
    (
        mut max_directional_lights_warning_emitted,
        mut max_rect_lights_warning_emitted,
        mut max_cascades_per_light_warning_emitted,
        mut live_shadow_mapping_lights,
    ): (
        Local<bool>,
        Local<bool>,
        Local<bool>,
        Local<HashSet<RetainedViewEntity>>,
    ),
    (
        mut point_lights,
        changed_point_lights,
        directional_lights,
        rect_lights,
        mut directional_light_view_entities,
    ): (
        Query<(
            Entity,
            &MainEntity,
            &ExtractedPointLight,
            &mut PointAndSpotLightViewEntities,
            &RenderLayers,
            AnyOf<(&CubemapFrusta, &Frustum)>,
        )>,
        Query<
            (),
            Or<(
                Changed<ExtractedPointLight>,
                Changed<CubemapFrusta>,
                Changed<Frustum>,
                Changed<RenderLayers>,
            )>,
        >,
        Query<(
            Entity,
            &MainEntity,
            &ExtractedDirectionalLight,
            &RenderLayers,
        )>,
        Query<(Entity, &MainEntity, &ExtractedRectLight, &RenderLayers)>,
        Query<&mut DirectionalLightViewEntities>,
    ),
    sorted_cameras: Res<SortedCameras>,
    (gpu_preprocessing_support, decals): (
        Res<GpuPreprocessingSupport>,
        Option<Res<RenderClusteredDecals>>,
    ),
    (existing_shadow_views, mut light_key_cache, mut specialized_shadow_material_pipeline_cache): (
        Query<&ShadowView>,
        ResMut<LightKeyCache>,
        ResMut<SpecializedShadowMaterialPipelineCache>,
    ),
    // NVForever held shadow maps: the hold, and the directional shadow texture of the last
    // frame (see where the texture is taken below).
    (mut hold_directional, mut last_directional_texture, mut nvf_directional_high_water, mut nvf_last_directional_cascades, nvf_outdoor_sun, hold_lights, mut nvf_light_record, mut nvf_last_point_texture, mut nvf_view_cache, mut nvf_parked, mut nvf_was_on): (
        Option<ResMut<HoldDirectionalShadowMaps>>,
        Local<Option<TextureId>>,
        // NVForever held shadow maps (#3779): high-water layer count for the current side.
        Local<NvfDirectionalShadowLayerHighWater>,
        // NVForever held shadow maps (#3779): last frame's cascade count on the texture.
        Local<usize>,
        // NVForever sun visibility: the light marked as the outdoor sun.
        Query<(), With<NvfOutdoorSun>>,
        // NVForever held light shadow maps: the hold and the slots, and what each layer was
        // last drawn with.
        Option<Res<HoldLightShadowMaps>>,
        Local<NvfLightShadowRecord>,
        // NVForever light view counts: the point shadow texture of the last frame.
        Local<Option<TextureId>>,
        // NVForever cached shadow views.
        Local<NvfShadowViewCache>,
        // NVForever kept shadow phases: the views of the lights the game holds.
        Option<ResMut<NvfParkedShadowViews>>,
        // NVForever held light shadow maps: whether the hold was active last frame.
        Local<bool>,
    ),
) {
    // NVForever held light shadow maps: the hold counts only while the game marks it
    // `active` (its ALC shadows row on). Without it every NVForever path of the point and
    // spot lights below is upstream's: no slots, no held faces, no static cache, no kept
    // phases, no cached shadow views and no counts. When it switches either way, every
    // light's shadow views are made again and what the layers held is forgotten.
    let hold_lights = hold_lights.filter(|hold| hold.active);
    let nvf_on = hold_lights.is_some();
    let nvf_switched = *nvf_was_on != nvf_on;
    *nvf_was_on = nvf_on;
    NVF_COUNTING.store(nvf_on, core::sync::atomic::Ordering::Relaxed);
    if nvf_switched {
        *nvf_light_record = NvfLightShadowRecord::default();
        *nvf_view_cache = NvfShadowViewCache::default();
    }
    if let Some(parked) = nvf_parked.as_deref_mut() {
        parked.active = nvf_on && nvf_keep_shadow_phases_on();
    }
    // NVForever cached shadow views: only with the hold (and not under the measuring aid).
    let nvf_view_cache_on = nvf_on && nvf_shadow_view_cache_on();
    // NVForever light view counts: the system's time and its stretches.
    let nvf_start = nvf_counting().then(std::time::Instant::now);
    let mut nvf_lap = NvfLap::start();
    let views_iter = views.iter();
    let views_count = views_iter.len();
    let Some(mut view_gpu_lights_writer) =
        light_meta
            .view_gpu_lights
            .get_writer(views_count, &render_device, &render_queue)
    else {
        return;
    };
    // NVForever light view counts: the light uniforms' writer (a staging buffer).
    nvf_lap.lap(NVF_WRITER);

    // Pre-calculate for PointLights
    let cube_face_rotations = CUBE_MAP_FACES
        .iter()
        .map(|CubeMapFace { target, up }| Transform::IDENTITY.looking_at(*target, *up))
        .collect::<Vec<_>>();

    global_clusterable_object_meta.entity_to_index.clear();

    let mut point_light_entities: Vec<_> = point_lights
        .iter()
        .map(|(entity, _, _, _, _, _)| entity)
        .collect::<Vec<_>>();
    let mut directional_light_entities: Vec<_> = directional_lights
        .iter()
        .map(|(entity, _, _, _)| entity)
        .collect::<Vec<_>>();
    let rect_light_entities: Vec<_> = rect_lights
        .iter()
        .map(|(entity, _, _, _)| entity)
        .collect::<Vec<_>>();

    #[cfg(any(
        not(feature = "webgl"),
        not(target_arch = "wasm32"),
        feature = "webgpu"
    ))]
    let max_texture_array_layers = render_device.limits().max_texture_array_layers as usize;
    #[cfg(any(
        not(feature = "webgl"),
        not(target_arch = "wasm32"),
        feature = "webgpu"
    ))]
    let max_texture_cubes = max_texture_array_layers / 6;
    #[cfg(all(feature = "webgl", target_arch = "wasm32", not(feature = "webgpu")))]
    let max_texture_array_layers = 1;
    #[cfg(all(feature = "webgl", target_arch = "wasm32", not(feature = "webgpu")))]
    let max_texture_cubes = 1;

    // When storage buffers are available, rect lights are clustered.
    // Otherwise they fall back to a non-clustered uniform capped at `MAX_RECT_LIGHTS`.
    let rect_lights_are_clustered = global_clusterable_object_meta
        .gpu_clustered_lights
        .is_storage_buffer();

    if !*max_directional_lights_warning_emitted
        && directional_light_entities.len() > MAX_DIRECTIONAL_LIGHTS
    {
        warn!(
            "The amount of directional lights of {} is exceeding the supported limit of {}.",
            directional_light_entities.len(),
            MAX_DIRECTIONAL_LIGHTS
        );
        *max_directional_lights_warning_emitted = true;
    }

    // The `MAX_RECT_LIGHTS` cap only applies on the non-clustered path.
    if !rect_lights_are_clustered
        && !*max_rect_lights_warning_emitted
        && rect_light_entities.len() > MAX_RECT_LIGHTS
    {
        warn!(
            "The amount of rectangle area lights of {} is exceeding the supported limit of {}.",
            rect_light_entities.len(),
            MAX_RECT_LIGHTS
        );
        *max_rect_lights_warning_emitted = true;
    }

    if !*max_cascades_per_light_warning_emitted
        && directional_lights.iter().any(|(_, _, light, _)| {
            light.cascade_shadow_config.bounds.len() > MAX_CASCADES_PER_LIGHT
        })
    {
        warn!(
            "The number of cascades configured for a directional light exceeds the supported limit of {}.",
            MAX_CASCADES_PER_LIGHT
        );
        *max_cascades_per_light_warning_emitted = true;
    }

    let point_light_count = point_lights
        .iter()
        .filter(|light| light.2.spot_light_angles.is_none())
        .count();

    let point_light_volumetric_enabled_count = point_lights
        .iter()
        .filter(|(_, _, light, _, _, _)| light.volumetric && light.spot_light_angles.is_none())
        .count()
        .min(max_texture_cubes);

    let point_light_shadow_maps_count = point_lights
        .iter()
        .filter(|light| light.2.shadow_maps_enabled && light.2.spot_light_angles.is_none())
        .count()
        .min(max_texture_cubes);

    let directional_volumetric_enabled_count = directional_lights
        .iter()
        .take(MAX_DIRECTIONAL_LIGHTS)
        .filter(|(_, _, light, _)| light.volumetric)
        .count()
        .min(max_texture_array_layers / MAX_CASCADES_PER_LIGHT);

    let directional_shadow_enabled_count = directional_lights
        .iter()
        .take(MAX_DIRECTIONAL_LIGHTS)
        .filter(|(_, _, light, _)| light.shadow_maps_enabled)
        .count()
        .min(max_texture_array_layers / MAX_CASCADES_PER_LIGHT);

    let spot_light_count = point_lights
        .iter()
        .filter(|(_, _, light, _, _, _)| light.spot_light_angles.is_some())
        .count()
        .min(max_texture_array_layers - directional_shadow_enabled_count * MAX_CASCADES_PER_LIGHT);

    let spot_light_volumetric_enabled_count = point_lights
        .iter()
        .filter(|(_, _, light, _, _, _)| light.volumetric && light.spot_light_angles.is_some())
        .count()
        .min(max_texture_array_layers - directional_shadow_enabled_count * MAX_CASCADES_PER_LIGHT);

    let spot_light_shadow_maps_count = point_lights
        .iter()
        .filter(|(_, _, light, _, _, _)| {
            light.shadow_maps_enabled && light.spot_light_angles.is_some()
        })
        .count()
        .min(max_texture_array_layers - directional_shadow_enabled_count * MAX_CASCADES_PER_LIGHT);

    // Sort lights by
    // - point-light vs spot-light, so that we can iterate point lights and spot lights in contiguous blocks in the fragment shader,
    // - then those with shadows enabled first, so that the index can be used to render at most `point_light_shadow_maps_count`
    //   point light shadows and `spot_light_shadow_maps_count` spot light shadow maps,
    // - then by entity as a stable key to ensure that a consistent set of lights are chosen if the light count limit is exceeded.
    point_light_entities.sort_by_cached_key(|entity| {
        (
            point_or_spot_light_to_clusterable(point_lights.get(*entity).unwrap().2).ordering(),
            *entity,
        )
    });

    // Sort lights by
    // - those with volumetric (and shadows) enabled first, so that the
    //   volumetric lighting pass can quickly find the volumetric lights;
    // - then those with shadows enabled second, so that the index can be used
    //   to render at most `directional_light_shadow_maps_count` directional light
    //   shadows
    // - then by entity as a stable key to ensure that a consistent set of
    //   lights are chosen if the light count limit is exceeded.
    directional_light_entities.sort_by_cached_key(|entity| {
        let light = directional_lights.get(*entity).unwrap().2;
        (light.volumetric, light.shadow_maps_enabled, *entity)
    });

    if global_clusterable_object_meta.entity_to_index.capacity() < point_light_entities.len() {
        global_clusterable_object_meta
            .entity_to_index
            .reserve(point_light_entities.len());
    }

    global_clusterable_object_meta.gpu_clustered_lights.clear();

    // NVForever held light shadow maps: the cube each shadowed point light draws into. The
    // game's slots first (a light keeps its slot while it casts, seen or not); any other
    // shadowed point light takes the lowest slot left this frame.
    let mut nvf_cube_of = EntityHashMap::<usize>::default();
    let mut nvf_cubes = point_light_shadow_maps_count;
    // NVForever ALCS far maps: the far texture's cubes and side.
    let mut nvf_far_cubes = 0usize;
    let nvf_far_size = hold_lights.as_deref().map_or(0, |hold| hold.far_size);
    // The shadowed point lights that draw into the first texture this frame: Bevy's count
    // less the far texture's (it sized the first texture by all of them, which made it anew
    // whenever far lights drew).
    let mut nvf_near_shadow_maps_count = point_light_shadow_maps_count;
    if let Some(hold) = hold_lights.as_deref() {
        let mut used: HashSet<u32> = hold.slots.values().copied().collect();
        let mut next_free = 0u32;
        for entity in point_light_entities.iter() {
            let (_, main_entity, light, ..) = point_lights.get(*entity).unwrap();
            if light.spot_light_angles.is_some() {
                continue;
            }
            // NVForever ALCS far maps: a light of the far texture.
            if let Some(&slot) = hold.far_slots.get(&main_entity.id()).filter(|_| nvf_far_size > 0) {
                nvf_cube_of.insert(*entity, NVF_FAR_CUBE + slot as usize);
                nvf_far_cubes = nvf_far_cubes.max(slot as usize + 1);
                if light.shadow_maps_enabled {
                    nvf_near_shadow_maps_count = nvf_near_shadow_maps_count.saturating_sub(1);
                }
                continue;
            }
            let slot = match hold.slots.get(&main_entity.id()) {
                Some(&slot) => slot,
                None if light.shadow_maps_enabled => {
                    while used.contains(&next_free) {
                        next_free += 1;
                    }
                    used.insert(next_free);
                    next_free
                }
                None => continue,
            };
            nvf_cube_of.insert(*entity, slot as usize);
        }
        if nvf_far_size > 0 {
            nvf_far_cubes = nvf_far_cubes.max(hold.far_capacity as usize).min(NVF_FAR_CUBE - 1).min(max_texture_cubes);
        }
        nvf_cubes = nvf_cube_of
            .values()
            .filter(|cube| !nvf_far(**cube))
            .map(|slot| slot + 1)
            .max()
            .unwrap_or(0)
            .max(hold.point_capacity as usize)
            .max(nvf_near_shadow_maps_count)
            .min(max_texture_cubes);
    }

    // NVForever held light shadow maps: the same for the spot lights. The slot each shadowed
    // spot light draws into, past the sun's cascades in the directional shadow texture: the
    // game's slots first (kept while it casts, with or without views), then the lowest slot
    // left this frame for any other shadowed spot light (the flashlight). Without the hold
    // the shadowed spot lights keep Bevy's layers (their index among the spot lights).
    let mut nvf_spot_of = EntityHashMap::<usize>::default();
    let mut nvf_spot_layers = spot_light_shadow_maps_count;
    if let Some(hold) = hold_lights.as_deref() {
        let mut used: HashSet<u32> = hold.spot_slots.values().copied().collect();
        let mut next_free = 0u32;
        for entity in point_light_entities.iter() {
            let (_, main_entity, light, ..) = point_lights.get(*entity).unwrap();
            if light.spot_light_angles.is_none() {
                continue;
            }
            let slot = match hold.spot_slots.get(&main_entity.id()) {
                Some(&slot) => slot,
                None if light.shadow_maps_enabled => {
                    while used.contains(&next_free) {
                        next_free += 1;
                    }
                    used.insert(next_free);
                    next_free
                }
                None => continue,
            };
            nvf_spot_of.insert(*entity, slot as usize);
        }
        nvf_spot_layers = nvf_spot_of
            .values()
            .map(|slot| slot + 1)
            .max()
            .unwrap_or(0)
            .max(hold.spot_capacity as usize)
            .max(spot_light_shadow_maps_count);
    }
    // NVForever kept shadow phases (engine/vendor/README.md): the shadow views (by retained
    // view entity) of every light the game holds a slot for, seen or not, drawing or held.
    // Their phases, pipeline keys and specialized pipelines outlive their views, so a held
    // light switched on again queues and specializes only what changed.
    if let Some(parked) = nvf_parked.as_deref_mut() {
        parked.views.clear();
        if let Some(hold) = hold_lights.as_deref().filter(|_| nvf_keep_shadow_phases_on()) {
            // (NVForever ALCS far maps: the lamps of the far texture too; left out until
            // 2026-11-25, so each of their draws specialized and queued every caster anew.)
            for main in hold.slots.keys().chain(hold.far_slots.keys()) {
                parked.views.extend((0..6).map(|face| RetainedViewEntity::new(MainEntity::from(*main), None, face)));
            }
            for main in hold.spot_slots.keys() {
                parked.views.insert(RetainedViewEntity::new(MainEntity::from(*main), None, 0));
            }
        }
    }
    // NVForever light view counts.
    nvf_lap.lap(NVF_SLOTS);

    // NVForever held light shadow maps: the cascade count and the directional shadow texture
    // are taken before the lights' GPU data (moved up from below), as a spot light's layer
    // (past the cascades) and the texture's id decide whether a held spot light's layer
    // holds its map.
    // iterate the views once to find the maximum number of cascade shadowmaps we will need
    let mut num_directional_cascades_enabled = 0usize;
    for (
        _entity,
        _camera_main_entity,
        _extracted_view,
        _clusters,
        maybe_layers,
        _no_indirect_drawing,
        _maybe_ambient_override,
    ) in sorted_cameras
        .0
        .iter()
        .filter_map(|sorted_camera| views.get(sorted_camera.entity).ok())
    {
        let mut num_directional_cascades_for_this_view = 0usize;
        let render_layers = maybe_layers.unwrap_or_default();

        for light_entity in directional_light_entities.iter() {
            let (_, _, light, light_render_layers) = directional_lights.get(*light_entity).unwrap();
            if light.shadow_maps_enabled && light_render_layers.intersects(render_layers) {
                num_directional_cascades_for_this_view += light
                    .cascade_shadow_config
                    .bounds
                    .len()
                    .min(MAX_CASCADES_PER_LIGHT);
            }
        }

        num_directional_cascades_enabled = num_directional_cascades_enabled
            .max(num_directional_cascades_for_this_view)
            .min(max_texture_array_layers);
    }

    // NVForever held light shadow maps: room past the cascades for every spot slot.
    // NVForever held shadow maps (#3779, [design]): while the directional shadow texture's
    // side stays the same, its layer count only grows (high-water mark) so switching
    // between third person, first person (`ArmsShade`) and the flashlight reuses the
    // existing texture instead of allocating a new one on every transition.
    let nvf_directional_side = (directional_light_shadow_map.size as u32)
        .min(render_device.limits().max_texture_dimension_2d);
    let nvf_needed_directional_layers = (num_directional_cascades_enabled + nvf_spot_layers)
        .min(max_texture_array_layers)
        .max(1);
    let nvf_directional_layers = if hold_directional.is_some() || hold_lights.is_some() {
        nvf_directional_high_water.layers(
            nvf_directional_side,
            nvf_needed_directional_layers,
            max_texture_array_layers,
        )
    } else {
        nvf_directional_high_water.reset();
        nvf_needed_directional_layers
    };
    let directional_light_depth_texture = texture_cache.get(
        &render_device,
        TextureDescriptor {
            size: Extent3d {
                width: nvf_directional_side,
                height: nvf_directional_side,
                depth_or_array_layers: nvf_directional_layers as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: CORE_3D_SHADOW_MAP_FORMAT,
            label: Some("directional_light_shadow_map_texture"),
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
    );

    // NVForever held shadow maps: a held cascade samples what an earlier frame drew into
    // this texture, so it can only be held when the texture is the one the last frame drew
    // into. The cache hands out another one whenever the descriptor changes (the layer count
    // grew, or the map size changed) or the old one was dropped; that frame draws every
    // cascade, with the matrices the main world gave the held ones, which are then what the
    // following frames hold.
    let directional_texture_id = directional_light_depth_texture.texture.id();
    if *last_directional_texture != Some(directional_texture_id) {
        // NVForever light view counts.
        nvf_event(NVF_NEW_DIRECTIONAL_TEXTURE, u64::from(last_directional_texture.is_some()));
        if let Some(hold) = hold_directional.as_deref_mut() {
            if last_directional_texture.is_some() {
                bevy_log::info!(
                    "held shadow maps: a new directional shadow texture ({} layers): every cascade drawn this frame",
                    nvf_directional_layers
                );
            }
            if hold.0 != 0 {
                hold.0 = 0;
            }
        }
        *last_directional_texture = Some(directional_texture_id);
    }
    // NVForever held shadow maps (#3779): when the high-water texture is reused across a
    // change in cascade count (such as `ArmsShade` toggling off or on), draw every cascade
    // once so no held cascade samples a layer whose directional light assignment shifted.
    if *nvf_last_directional_cascades != num_directional_cascades_enabled {
        if let Some(hold) = hold_directional.as_deref_mut()
            && hold.0 != 0
        {
            hold.0 = 0;
        }
        *nvf_last_directional_cascades = num_directional_cascades_enabled;
    }

    // NVForever held light shadow maps: the sun's cascades draw into the layers below
    // `num_directional_cascades_enabled`, so no spot light's map is held there any more (the
    // cascade count changes at run time: sun shadows on or off, the cascade count).
    nvf_light_record
        .spot
        .retain(|layer, _| *layer as usize >= num_directional_cascades_enabled);

    // NVForever static shadow cache: on while the game asks for it (and holds the lamps).
    let nvf_static_cache = hold_lights.as_deref().is_some_and(|hold| hold.static_cache);

    // NVForever held light shadow maps: the point texture is taken before the lights' GPU
    // data (moved up from below), as its id decides whether a held light's cube holds its maps.
    let point_light_depth_texture = texture_cache.get(
        &render_device,
        TextureDescriptor {
            size: Extent3d {
                width: point_light_shadow_map.size as u32,
                height: point_light_shadow_map.size as u32,
                // NVForever held light shadow maps: room for every slot.
                depth_or_array_layers: nvf_cubes.max(nvf_near_shadow_maps_count).max(1) as u32 * 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: CORE_3D_SHADOW_MAP_FORMAT,
            label: Some("point_light_shadow_map_texture"),
            // NVForever static shadow cache: the cache's layers are copied into it.
            usage: if nvf_static_cache {
                TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST
            } else {
                TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING
            },
            view_formats: &[],
        },
    );

    // NVForever static shadow cache: a second cube texture of the same layers, each point
    // light's faces drawn with the static casters alone, copied into the sampled texture
    // (above) before the dynamic casters are drawn over them. Taken every frame while the
    // cache is on, so the texture cache keeps it.
    let nvf_static_texture = nvf_static_cache.then(|| {
        texture_cache.get(
            &render_device,
            TextureDescriptor {
                size: Extent3d {
                    width: point_light_shadow_map.size as u32,
                    height: point_light_shadow_map.size as u32,
                    depth_or_array_layers: nvf_cubes.max(nvf_near_shadow_maps_count).max(1) as u32 * 6,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: CORE_3D_SHADOW_MAP_FORMAT,
                label: Some("nvf_point_light_static_shadow_cache"),
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
                view_formats: &[],
            },
        )
    });
    let nvf_static_texture_id = nvf_static_texture.as_ref().map(|t| t.texture.id());

    // NVForever ALCS far maps: the second cube texture and its static cache, while the game
    // keeps lights in it.
    let nvf_far_descriptor = |label: &'static str, usage: TextureUsages| TextureDescriptor {
        size: Extent3d {
            width: nvf_far_size,
            height: nvf_far_size,
            depth_or_array_layers: nvf_far_cubes.max(1) as u32 * 6,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: CORE_3D_SHADOW_MAP_FORMAT,
        label: Some(label),
        usage,
        view_formats: &[],
    };
    let nvf_far_texture = (nvf_far_cubes > 0).then(|| {
        texture_cache.get(
            &render_device,
            nvf_far_descriptor(
                "nvf_point_light_far_shadow_map_texture",
                if nvf_static_cache {
                    TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST
                } else {
                    TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING
                },
            ),
        )
    });
    let nvf_far_static_texture = (nvf_far_cubes > 0 && nvf_static_cache).then(|| {
        texture_cache.get(
            &render_device,
            nvf_far_descriptor(
                "nvf_point_light_far_static_shadow_cache",
                TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            ),
        )
    });
    let nvf_far_texture_id = nvf_far_texture.as_ref().map(|t| t.texture.id());
    let nvf_far_static_texture_id = nvf_far_static_texture.as_ref().map(|t| t.texture.id());

    let nvf_point_texture_id = point_light_depth_texture.texture.id();
    // NVForever ALCS far maps: the sampled texture a cube number is in.
    let nvf_texture_id_of = |cube: usize| match nvf_far_texture_id {
        Some(far) if nvf_far(cube) => far,
        _ => nvf_point_texture_id,
    };
    // NVForever light view counts.
    if *nvf_last_point_texture != Some(nvf_point_texture_id) {
        nvf_event(NVF_NEW_POINT_TEXTURE, u64::from(nvf_last_point_texture.is_some()));
        *nvf_last_point_texture = Some(nvf_point_texture_id);
    }
    nvf_lap.lap(NVF_TEXTURES);

    for (index, entity) in point_light_entities.iter().enumerate() {
        let light = point_lights.get(*entity).unwrap().2;

        let mut flags = PointLightFlags::NONE;

        // Lights are sorted, shadow enabled lights are first
        if light.shadow_maps_enabled
            && (index < point_light_shadow_maps_count
                || (light.spot_light_angles.is_some()
                    && index - point_light_count < spot_light_shadow_maps_count))
        {
            flags |= PointLightFlags::SHADOW_MAPS_ENABLED;
        }

        if light.contact_shadows_enabled {
            flags |= PointLightFlags::CONTACT_SHADOWS_ENABLED;
        }

        let cube_face_projection = proj::perspective_infinite_reverse(
            core::f32::consts::FRAC_PI_2,
            1.0,
            light.shadow_map_near_z,
        );
        if light.shadow_maps_enabled
            && light.volumetric
            && (index < point_light_volumetric_enabled_count
                || (light.spot_light_angles.is_some()
                    && index - point_light_count < spot_light_volumetric_enabled_count))
        {
            flags |= PointLightFlags::VOLUMETRIC;
        }

        if light.affects_lightmapped_mesh_diffuse {
            flags |= PointLightFlags::AFFECTS_LIGHTMAPPED_MESH_DIFFUSE;
        }

        let (light_custom_data, spot_light_tan_angle) = match light.spot_light_angles {
            Some((inner, outer)) => {
                flags |= PointLightFlags::SPOT_LIGHT;

                let light_direction = light.transform.forward();
                if light_direction.y.is_sign_negative() {
                    flags |= PointLightFlags::SPOT_LIGHT_Y_NEGATIVE;
                }

                let cos_outer = ops::cos(outer);
                let spot_scale = 1.0 / f32::max(ops::cos(inner) - cos_outer, 1e-4);
                let spot_offset = -cos_outer * spot_scale;

                (
                    // For spot lights: the direction (x,z), spot_scale and spot_offset
                    light_direction.xz().extend(spot_scale).extend(spot_offset),
                    ops::tan(outer),
                )
            }
            None => {
                (
                    // For point lights: the lower-right 2x2 values of the projection matrix [2][2] [2][3] [3][2] [3][3]
                    Vec4::new(
                        cube_face_projection.z_axis.z,
                        cube_face_projection.z_axis.w,
                        cube_face_projection.w_axis.z,
                        cube_face_projection.w_axis.w,
                    ),
                    // unused
                    0.0,
                )
            }
        };

        global_clusterable_object_meta
            .gpu_clustered_lights
            .add(GpuClusteredLight {
                light_custom_data,
                // premultiply color by intensity
                // we don't use the alpha at all, so no reason to multiply only [0..3]
                color_inverse_square_range: (light.color.to_vec4() * light.intensity)
                    .xyz()
                    .extend(1.0 / (light.range * light.range)),
                position_radius: light.transform.translation().extend(light.radius),
                // NVForever held light shadow maps: a point light's slot + 1 in bits 16..23; a
                // point light the game holds without views casts once all six faces hold its
                // maps. A spot light's layer of the directional texture + 1 in the same bits;
                // one the game holds without views casts once its layer holds its map. The
                // game's fade in bits 24..31 ([`nvf_fade_bits`]). A light whose slot or layer
                // does not fit in the bits casts nothing.
                flags: {
                    let nvf_main = *point_lights.get(*entity).unwrap().1;
                    let nvf_fade = hold_lights
                        .as_deref()
                        .and_then(|hold| hold.fade.get(&nvf_main.id()))
                        .map_or(0, |fade| nvf_fade_bits(*fade));
                    let nvf_spot_layer = nvf_spot_of
                        .get(entity)
                        .map(|slot| num_directional_cascades_enabled + slot);
                    match (nvf_cube_of.get(entity), nvf_spot_layer) {
                        (Some(&cube), _) if cube + 1 > NVF_LAYER_BITS_MAX => {
                            (flags - PointLightFlags::SHADOW_MAPS_ENABLED).bits()
                        }
                        (Some(&cube), _) if flags.contains(PointLightFlags::SHADOW_MAPS_ENABLED) => {
                            flags.bits() | ((cube as u32 + 1) << 16) | nvf_fade
                        }
                        (Some(&cube), _)
                            if light.spot_light_angles.is_none()
                                && (0..6).all(|face| {
                                    nvf_light_record.point.get(&((cube * 6 + face) as u32))
                                        == Some(&(nvf_main, nvf_texture_id_of(cube)))
                                }) =>
                        {
                            (flags | PointLightFlags::SHADOW_MAPS_ENABLED).bits()
                                | ((cube as u32 + 1) << 16)
                                | nvf_fade
                        }
                        (_, Some(layer))
                            if layer + 1 > NVF_LAYER_BITS_MAX || layer >= nvf_directional_layers =>
                        {
                            (flags - PointLightFlags::SHADOW_MAPS_ENABLED).bits()
                        }
                        (_, Some(layer)) if flags.contains(PointLightFlags::SHADOW_MAPS_ENABLED) => {
                            flags.bits() | ((layer as u32 + 1) << 16) | nvf_fade
                        }
                        (_, Some(layer))
                            if nvf_light_record.spot.get(&(layer as u32))
                                == Some(&(nvf_main, directional_texture_id)) =>
                        {
                            (flags | PointLightFlags::SHADOW_MAPS_ENABLED).bits()
                                | ((layer as u32 + 1) << 16)
                                | nvf_fade
                        }
                        _ => flags.bits(),
                    }
                },
                shadow_depth_bias: light.shadow_depth_bias,
                shadow_normal_bias: light.shadow_normal_bias,
                shadow_map_near_z: light.shadow_map_near_z,
                spot_light_tan_angle,
                decal_index: decals
                    .as_ref()
                    .and_then(|decals| decals.get(*entity))
                    .and_then(|index| index.try_into().ok())
                    .unwrap_or(u32::MAX),
                range: light.range,
                // NVForever ALCS soft sizes: the game's size for the light's soft shadows,
                // apart from the light's `radius` (which the lighting also reads).
                soft_shadow_size: match hold_lights
                    .as_deref()
                    .filter(|hold| hold.active)
                    .and_then(|hold| hold.soft_size.get(&point_lights.get(*entity).unwrap().1.id()))
                {
                    Some(size) => *size,
                    None if light.soft_shadows_enabled => light.radius,
                    None => 0.0,
                },
            });
        global_clusterable_object_meta
            .entity_to_index
            .insert(*entity, index);
        debug_assert_eq!(
            global_clusterable_object_meta.entity_to_index.len(),
            global_clusterable_object_meta.gpu_clustered_lights.len()
        );
    }

    if rect_lights_are_clustered {
        for entity in &rect_light_entities {
            let light = rect_lights.get(*entity).unwrap().2;

            let index = global_clusterable_object_meta.gpu_clustered_lights.len();
            global_clusterable_object_meta
                .gpu_clustered_lights
                .add(GpuClusteredLight {
                    light_custom_data: Vec4::from(light.transform.rotation()),
                    color_inverse_square_range: (light.color.to_vec4() * light.intensity)
                        .xyz()
                        .extend(light.height),
                    position_radius: light.transform.translation().extend(light.width),
                    flags: PointLightFlags::RECT_LIGHT.bits(),
                    shadow_depth_bias: 0.0,
                    shadow_normal_bias: 0.0,
                    shadow_map_near_z: 0.0,
                    spot_light_tan_angle: 0.0,
                    decal_index: u32::MAX,
                    range: light.range,
                    soft_shadow_size: 0.0,
                });
            global_clusterable_object_meta
                .entity_to_index
                .insert(*entity, index);
            debug_assert_eq!(
                global_clusterable_object_meta.entity_to_index.len(),
                global_clusterable_object_meta.gpu_clustered_lights.len()
            );
        }
    }

    // NVForever held light shadow maps: the cascade count is found above.

    global_clusterable_object_meta
        .gpu_clustered_lights
        .write_buffer(&render_device, &render_queue);
    // NVForever light view counts.
    nvf_lap.lap(NVF_GPU_LIGHTS);

    live_shadow_mapping_lights.clear();

    // NVForever cached shadow views (engine/vendor/README.md): the two textures' array views,
    // the point and spot lights' layer attachments and the cascades' layer views are kept
    // while the texture is the same, instead of made anew every frame (and every frame a
    // light's views are made again); an attachment kept is set to clear again each frame.
    let nvf_cache = &mut *nvf_view_cache;
    if !nvf_view_cache_on {
        // Without the hold, or with the measuring aid: every view made anew each frame, as
        // upstream.
        *nvf_cache = NvfShadowViewCache::default();
    }
    if nvf_cache.point_texture != Some((nvf_point_texture_id, nvf_far_texture_id)) {
        nvf_cache.point_texture = Some((nvf_point_texture_id, nvf_far_texture_id));
        nvf_cache.point_array = None;
        nvf_cache.far_array = None;
        nvf_cache.point_layers.clear();
        // NVForever static shadow cache: the sampled layers' loading attachments.
        nvf_cache.load_layers.clear();
    }
    // NVForever static shadow cache: the cache's layer attachments, kept while it is the same
    // texture and set to clear again each frame.
    if nvf_cache.static_texture != (nvf_static_texture_id, nvf_far_static_texture_id) {
        nvf_cache.static_texture = (nvf_static_texture_id, nvf_far_static_texture_id);
        nvf_cache.static_layers.clear();
    }
    for attachment in nvf_cache.static_layers.values() {
        attachment.prepare_for_new_frame();
    }
    if nvf_static_texture_id.is_none() {
        nvf_light_record.static_point.clear();
    }
    if nvf_cache.directional_texture != Some(directional_texture_id) {
        nvf_cache.directional_texture = Some(directional_texture_id);
        nvf_cache.directional_array = None;
        nvf_cache.spot_layers.clear();
        nvf_cache.cascade_views.clear();
    }
    for attachment in nvf_cache.point_layers.values().chain(nvf_cache.spot_layers.values()) {
        attachment.prepare_for_new_frame();
    }
    let mut point_light_depth_attachments = mem::take(&mut nvf_cache.point_layers);
    let mut directional_light_depth_attachments = mem::take(&mut nvf_cache.spot_layers);
    let mut nvf_cascade_views = mem::take(&mut nvf_cache.cascade_views);
    let nvf_new_point_array = nvf_cache.point_array.is_none();
    let nvf_new_directional_array = nvf_cache.directional_array.is_none();

    let point_light_depth_texture_view = nvf_cache.point_array.get_or_insert_with(|| {
        point_light_depth_texture
            .texture
            .create_view(&TextureViewDescriptor {
                label: Some("point_light_shadow_map_array_texture_view"),
                format: None,
                // NOTE: iOS Simulator is missing CubeArray support so we use Cube instead.
                // See https://github.com/bevyengine/bevy/pull/12052 - remove if support is added.
                #[cfg(all(
                    not(target_abi = "sim"),
                    any(
                        not(feature = "webgl"),
                        not(target_arch = "wasm32"),
                        feature = "webgpu"
                    )
                ))]
                dimension: Some(TextureViewDimension::CubeArray),
                #[cfg(any(
                    target_abi = "sim",
                    all(feature = "webgl", target_arch = "wasm32", not(feature = "webgpu"))
                ))]
                dimension: Some(TextureViewDimension::Cube),
                usage: None,
                aspect: TextureAspect::DepthOnly,
                base_mip_level: 0,
                mip_level_count: None,
                base_array_layer: 0,
                array_layer_count: None,
            })
    }).clone();

    // NVForever ALCS far maps: the far texture's cube array view for the mesh views' binding
    // (the first texture's while there is none: the binding is always there).
    let nvf_far_depth_texture_view = match nvf_far_texture.as_ref() {
        Some(far) => nvf_cache
            .far_array
            .get_or_insert_with(|| {
                far.texture.create_view(&TextureViewDescriptor {
                    label: Some("nvf_point_light_far_shadow_map_array_texture_view"),
                    format: None,
                    dimension: Some(TextureViewDimension::CubeArray),
                    usage: None,
                    aspect: TextureAspect::DepthOnly,
                    base_mip_level: 0,
                    mip_level_count: None,
                    base_array_layer: 0,
                    array_layer_count: None,
                })
            })
            .clone(),
        None => point_light_depth_texture_view.clone(),
    };

    // NVForever held light shadow maps: the directional shadow texture is taken above.

    // NVForever cached shadow views: kept while the texture is the same.
    let directional_light_depth_texture_view = nvf_cache.directional_array.get_or_insert_with(|| {
        directional_light_depth_texture
            .texture
            .create_view(&TextureViewDescriptor {
                label: Some("directional_light_shadow_map_array_texture_view"),
                format: None,
                #[cfg(any(
                    not(feature = "webgl"),
                    not(target_arch = "wasm32"),
                    feature = "webgpu"
                ))]
                dimension: Some(TextureViewDimension::D2Array),
                #[cfg(all(feature = "webgl", target_arch = "wasm32", not(feature = "webgpu")))]
                dimension: Some(TextureViewDimension::D2),
                usage: None,
                aspect: TextureAspect::DepthOnly,
                base_mip_level: 0,
                mip_level_count: None,
                base_array_layer: 0,
                array_layer_count: None,
            })
    }).clone();
    nvf_event(NVF_TEXTURE_VIEWS, u64::from(nvf_new_point_array) + u64::from(nvf_new_directional_array));

    // NVForever light view counts: the two texture views made above.
    nvf_lap.lap(NVF_TEXTURES);
    let mut live_views = EntityHashSet::with_capacity(views_count);

    // TODO: this should select lights based on relevance to the view instead of the first ones that show up in a query
    for light_entity in point_light_entities
        .iter()
        // Lights are sorted, shadow enabled lights are first
        .take(point_light_count.min(max_texture_cubes))
    {
        let (
            _,
            light_main_entity,
            light,
            mut point_and_spot_light_view_entities,
            light_render_layers,
            (point_light_frusta, _),
        ) = point_lights.get_mut(*light_entity).unwrap();

        if !light.shadow_maps_enabled {
            // NVForever light view counts.
            nvf_event(NVF_DESPAWNED, point_and_spot_light_view_entities.0.len() as u64);
            despawn_entities(
                &mut commands,
                mem::take(&mut point_and_spot_light_view_entities.0),
            );
            // NVForever held light shadow maps.
            nvf_light_record.views.remove(light_entity);
            // NVForever static shadow cache: its views (and their components) are gone.
            nvf_light_record.static_faces.remove(light_entity);
            // NVForever kept shadow views.
            nvf_light_record.made_from.remove(light_entity);
            continue;
        }

        // NVForever held light shadow maps: the views' cube and texture.
        // NVForever ALCS far maps: the texture and side of the light's cube.
        let nvf_light_far = nvf_cube_of.get(light_entity).is_some_and(|cube| nvf_far(*cube)) && nvf_far_texture.is_some();
        let nvf_light_texture = match nvf_far_texture.as_ref() {
            Some(far) if nvf_light_far => far,
            _ => &point_light_depth_texture,
        };
        let nvf_light_size = if nvf_light_far { nvf_far_size } else { point_light_shadow_map.size as u32 };
        let nvf_view_key = (
            nvf_cube_of.get(light_entity).copied().unwrap_or(usize::MAX),
            nvf_light_texture.texture.id(),
        );
        let nvf_views_stale = hold_lights.is_some()
            && nvf_light_record.views.insert(*light_entity, nvf_view_key) != Some(nvf_view_key);
        // NVForever kept shadow views: what the views are made from; a light re-extracted (its
        // maps switched on, its casters' lists changed) with the same keeps its views as they
        // are instead of having them made again.
        let nvf_source = nvf_view_source(light, light_render_layers);
        let nvf_remake = point_and_spot_light_view_entities.0.is_empty()
            || nvf_views_stale
            || nvf_switched
            || (changed_point_lights.get(*light_entity).is_ok()
                && (!nvf_view_cache_on
                    || nvf_light_record.made_from.get(light_entity) != Some(&nvf_source)));
        if !point_and_spot_light_view_entities.0.is_empty()
            && !nvf_remake
            && changed_point_lights.get(*light_entity).is_ok()
        {
            nvf_event(NVF_VIEWS_KEPT, 6);
        }

        // NVForever light view counts.
        if point_and_spot_light_view_entities.0.is_empty() {
            nvf_event(NVF_SPAWNED, 6);
            // NVForever kept shadow phases: the phases the new views find kept.
            nvf_event(
                NVF_PHASES_REUSED,
                (0..6)
                    .filter(|face| {
                        shadow_render_phases
                            .contains_key(&RetainedViewEntity::new(*light_main_entity, None, *face))
                    })
                    .count() as u64,
            );
        } else if nvf_remake {
            nvf_event(NVF_REMADE, 6);
        }
        nvf_event(
            NVF_PHASES_NEW,
            (0..6)
                .filter(|face| {
                    !shadow_render_phases
                        .contains_key(&RetainedViewEntity::new(*light_main_entity, None, *face))
                })
                .count() as u64,
        );
        if nvf_remake {
            nvf_light_record.made_from.insert(*light_entity, nvf_source);
        }

        if point_and_spot_light_view_entities.0.is_empty() {
            // For each face of a cube we spawn a light entity
            let light_view_entities: Vec<_> = (0..6).map(|_| commands.spawn_empty().id()).collect();

            create_point_shadow_maps(
                &mut commands,
                &mut point_light_depth_attachments,
                &global_clusterable_object_meta,
                (
                    &cube_face_rotations,
                    point_light_frusta,
                    &light_view_entities,
                ),
                nvf_light_texture,
                (light_entity, light_main_entity, light, light_render_layers),
                nvf_light_size,
                gpu_preprocessing_support.max_supported_mode,
                nvf_cube_of.get(light_entity).copied(),
            );

            point_and_spot_light_view_entities.0 = light_view_entities;
        } else if nvf_remake {
            // Update the point shadow maps with the changes (NVForever held light shadow
            // maps: also when the light's slot or the texture changed; NVForever kept shadow
            // views: not when what they are made from is the same).
            create_point_shadow_maps(
                &mut commands,
                &mut point_light_depth_attachments,
                &global_clusterable_object_meta,
                (
                    &cube_face_rotations,
                    point_light_frusta,
                    &point_and_spot_light_view_entities.0,
                ),
                nvf_light_texture,
                (light_entity, light_main_entity, light, light_render_layers),
                nvf_light_size,
                gpu_preprocessing_support.max_supported_mode,
                nvf_cube_of.get(light_entity).copied(),
            );
        }

        // NVForever static shadow cache: when the hold switches, no face keeps the cache's
        // attachment (the hold below gives it back to the faces it still composites).
        if nvf_switched {
            for &view in point_and_spot_light_view_entities.0.iter() {
                commands.entity(view).remove::<NvfStaticShadowFace>();
            }
        }

        // NVForever held light shadow maps: a face the game holds, whose layer still holds
        // what this light drew into this texture, is left out of the frame's root views.
        if let Some(hold) = hold_lights.as_deref() {
            let held = hold.held.get(&light_main_entity.id()).copied().unwrap_or(0);
            let cube = nvf_cube_of.get(light_entity).copied().unwrap_or_else(|| {
                *global_clusterable_object_meta
                    .entity_to_index
                    .get(light_entity)
                    .unwrap()
            });
            let drawn_with = (*light_main_entity, nvf_light_texture.texture.id());
            // NVForever ALCS far maps: the static cache of the light's texture.
            let (nvf_light_static, nvf_light_static_id) = if nvf_light_far {
                (nvf_far_static_texture.as_ref(), nvf_far_static_texture_id)
            } else {
                (nvf_static_texture.as_ref(), nvf_static_texture_id)
            };
            let nvf_has_slot = nvf_cube_of.contains_key(light_entity);
            // NVForever static shadow cache: the faces drawn into the cache, and those
            // composited from it, this frame; the faces that carry the component.
            let main_id = light_main_entity.id();
            let to_static = hold.to_static.get(&main_id).copied().unwrap_or(0);
            let on_static = hold.on_static.get(&main_id).copied().unwrap_or(0);
            let mut with_face = nvf_light_record.static_faces.get(light_entity).copied().unwrap_or(0);
            for (face, &view) in point_and_spot_light_view_entities.0.iter().enumerate() {
                let layer = (cube * 6 + face) as u32;
                if held & (1 << face) != 0 && nvf_light_record.point.get(&layer) == Some(&drawn_with) {
                    commands.entity(view).remove::<RootNonCameraView>();
                } else {
                    // NVForever kept shadow views: views not made again keep a held face's
                    // missing root; a face drawn runs the schedule again.
                    if !nvf_remake {
                        commands.entity(view).insert(RootNonCameraView(Core3d.intern()));
                    }
                    // NVForever ALCS quiet static draws: only a layer that already holds
                    // this light's map may be left as it is.
                    let nvf_had_map = nvf_light_record.point.get(&layer) == Some(&drawn_with);
                    nvf_light_record.point.insert(layer, drawn_with);
                    // NVForever static shadow cache: how this face is drawn.
                    let bit = 1u8 << face;
                    // NVForever ALCS far maps: `layer` is the cube number's (the records' and the
                    // attachments' key); the layer in the light's texture is its slot's.
                    let nvf_layer = (if nvf_has_slot { nvf_slot(cube) } else { cube } * 6 + face) as u32;
                    let face_copy = match (nvf_light_static, nvf_light_static_id) {
                        (Some(cache), Some(cache_id)) if (to_static | on_static) & bit != 0 => {
                            let copy = NvfLayerCopy {
                                cache: cache.texture.clone(),
                                sampled: nvf_light_texture.texture.clone(),
                                layer: nvf_layer,
                                size: nvf_light_size,
                            };
                            if to_static & bit != 0 {
                                // Drawn into the cache (cleared), then copied out after.
                                nvf_light_record.static_point.insert(layer, (*light_main_entity, cache_id));
                                nvf_event(NVF_STATIC_DRAWS, 1);
                                let attachment = nvf_static_layer_attachment(&mut nvf_cache.static_layers, &cache.texture, layer, nvf_layer, true);
                                // NVForever ALCS quiet static draws: a face in
                                // `static_quiet` is not copied out; the game composites
                                // it next, so its moving casters never leave the picture.
                                let quiet = nvf_had_map && hold.static_quiet.get(&main_id).copied().unwrap_or(0) & bit != 0;
                                Some(NvfStaticShadowFace { attachment, copy, after: true, quiet })
                            } else if nvf_light_record.static_point.get(&layer) == Some(&(*light_main_entity, cache_id)) {
                                // Copied in from the cache first, then drawn over (loaded).
                                nvf_event(NVF_STATIC_COMPOSITES, 1);
                                let attachment = nvf_static_layer_attachment(&mut nvf_cache.load_layers, &nvf_light_texture.texture, layer, nvf_layer, false);
                                Some(NvfStaticShadowFace { attachment, copy, after: false, quiet: false })
                            } else {
                                // The cache's layer holds no static map of this light: drawn
                                // as ever (with only the casters the game left in its list).
                                nvf_event(NVF_STATIC_MISSES, 1);
                                None
                            }
                        }
                        _ => None,
                    };
                    match face_copy {
                        Some(face_copy) => {
                            commands.entity(view).insert(face_copy);
                            with_face |= bit;
                        }
                        None if with_face & bit != 0 => {
                            commands.entity(view).remove::<NvfStaticShadowFace>();
                            with_face &= !bit;
                        }
                        None => {}
                    }
                }
            }
            if with_face != 0 {
                nvf_light_record.static_faces.insert(*light_entity, with_face);
            } else {
                nvf_light_record.static_faces.remove(light_entity);
            }
        }

        // Initialize the shadow render phases. We have to do this even if we've
        // already created the views in order to clear out old data.
        for face_index in 0..6 {
            let retained_view_entity =
                RetainedViewEntity::new(*light_main_entity, None, face_index);
            shadow_render_phases.prepare_for_new_frame(
                retained_view_entity,
                gpu_preprocessing_support.max_supported_mode,
            );
            live_shadow_mapping_lights.insert(retained_view_entity);
        }
    }
    // NVForever light view counts.
    nvf_lap.lap(NVF_POINT_VIEWS);

    // spot lights
    for (light_index, light_entity) in point_light_entities
        .iter()
        .skip(point_light_count)
        .take(spot_light_count)
        .enumerate()
    {
        let (
            _,
            light_main_entity,
            light,
            mut point_and_spot_light_view_entities,
            light_render_layers,
            (_, spot_light_frustum),
        ) = point_lights.get_mut(*light_entity).unwrap();

        if !light.shadow_maps_enabled {
            // NVForever light view counts.
            nvf_event(NVF_DESPAWNED, point_and_spot_light_view_entities.0.len() as u64);
            despawn_entities(
                &mut commands,
                mem::take(&mut point_and_spot_light_view_entities.0),
            );
            // NVForever held light shadow maps.
            nvf_light_record.views.remove(light_entity);
            // NVForever kept shadow views.
            nvf_light_record.made_from.remove(light_entity);
            continue;
        }

        // NVForever held light shadow maps: the light's slot (else its index, as in Bevy)
        // past the cascades; a layer the texture has no room for draws nothing.
        let light_index = nvf_spot_of.get(light_entity).copied().unwrap_or(light_index);
        if num_directional_cascades_enabled + light_index >= nvf_directional_layers {
            // NVForever light view counts.
            nvf_event(NVF_DESPAWNED, point_and_spot_light_view_entities.0.len() as u64);
            despawn_entities(
                &mut commands,
                mem::take(&mut point_and_spot_light_view_entities.0),
            );
            nvf_light_record.views.remove(light_entity);
            nvf_light_record.made_from.remove(light_entity);
            continue;
        }

        // Spot light shadow maps are shared across all cameras,
        // so the retained view entity must not include the camera.
        let retained_view_entity = RetainedViewEntity::new(*light_main_entity, None, 0);

        // NVForever held light shadow maps: the view's layer and texture.
        let nvf_view_key = (num_directional_cascades_enabled + light_index, directional_texture_id);
        let nvf_views_stale = hold_lights.is_some()
            && nvf_light_record.views.insert(*light_entity, nvf_view_key) != Some(nvf_view_key);
        // NVForever kept shadow views: as for the point lights.
        let nvf_source = nvf_view_source(light, light_render_layers);
        let nvf_remake = point_and_spot_light_view_entities.0.is_empty()
            || nvf_views_stale
            || nvf_switched
            || (changed_point_lights.get(*light_entity).is_ok()
                && (!nvf_view_cache_on
                    || nvf_light_record.made_from.get(light_entity) != Some(&nvf_source)));
        if !point_and_spot_light_view_entities.0.is_empty()
            && !nvf_remake
            && changed_point_lights.get(*light_entity).is_ok()
        {
            nvf_event(NVF_VIEWS_KEPT, 1);
        }

        // NVForever light view counts.
        if point_and_spot_light_view_entities.0.is_empty() {
            nvf_event(NVF_SPAWNED, 1);
            nvf_event(NVF_PHASES_REUSED, u64::from(shadow_render_phases.contains_key(&retained_view_entity)));
        } else if nvf_remake {
            nvf_event(NVF_REMADE, 1);
        }
        nvf_event(NVF_PHASES_NEW, u64::from(!shadow_render_phases.contains_key(&retained_view_entity)));
        if nvf_remake {
            nvf_light_record.made_from.insert(*light_entity, nvf_source);
        }

        if point_and_spot_light_view_entities.0.is_empty() {
            let view_light_entity = commands.spawn_empty().id();

            create_spot_shadow_map(
                &mut commands,
                &mut directional_light_depth_attachments,
                (num_directional_cascades_enabled, light_index),
                &directional_light_depth_texture,
                view_light_entity,
                (light_entity, light_main_entity, light, light_render_layers),
                spot_light_frustum,
                directional_light_shadow_map.size as u32,
                gpu_preprocessing_support.max_supported_mode,
            );

            point_and_spot_light_view_entities.0 = vec![view_light_entity];
        } else if nvf_remake {
            create_spot_shadow_map(
                &mut commands,
                &mut directional_light_depth_attachments,
                (num_directional_cascades_enabled, light_index),
                &directional_light_depth_texture,
                // There should only be one view light entity for spotlights
                point_and_spot_light_view_entities.0[0],
                (light_entity, light_main_entity, light, light_render_layers),
                spot_light_frustum,
                directional_light_shadow_map.size as u32,
                gpu_preprocessing_support.max_supported_mode,
            );
        }

        // NVForever held light shadow maps: as for the point lights' faces, bit 0.
        if let Some(hold) = hold_lights.as_deref() {
            let layer = (num_directional_cascades_enabled + light_index) as u32;
            let drawn_with = (*light_main_entity, directional_texture_id);
            let view = point_and_spot_light_view_entities.0[0];
            if hold.held.get(&light_main_entity.id()).is_some_and(|held| held & 1 != 0)
                && nvf_light_record.spot.get(&layer) == Some(&drawn_with)
            {
                commands.entity(view).remove::<RootNonCameraView>();
            } else {
                // NVForever kept shadow views: as for the point lights' faces.
                if !nvf_remake {
                    commands.entity(view).insert(RootNonCameraView(Core3d.intern()));
                }
                nvf_light_record.spot.insert(layer, drawn_with);
            }
        }

        shadow_render_phases.prepare_for_new_frame(
            retained_view_entity,
            gpu_preprocessing_support.max_supported_mode,
        );
        live_shadow_mapping_lights.insert(retained_view_entity);
    }
    // NVForever light view counts.
    nvf_lap.lap(NVF_SPOT_VIEWS);

    // set up light data for each view
    for (
        entity,
        camera_main_entity,
        extracted_view,
        clusters,
        maybe_layers,
        no_indirect_drawing,
        maybe_ambient_override,
    ) in sorted_cameras
        .0
        .iter()
        .filter_map(|sorted_camera| views.get(sorted_camera.entity).ok())
    {
        live_views.insert(entity);

        let view_layers = maybe_layers.unwrap_or_default();
        let mut view_lights = Vec::new();
        let mut view_occlusion_culling_lights = Vec::new();

        let gpu_preprocessing_mode = gpu_preprocessing_support.min(if !no_indirect_drawing {
            GpuPreprocessingMode::Culling
        } else {
            GpuPreprocessingMode::PreprocessingOnly
        });

        let is_orthographic = extracted_view.clip_from_view.w_axis.w == 1.0;
        let cluster_factors_zw = calculate_cluster_factors(
            clusters.near,
            clusters.far,
            clusters.dimensions.z as f32,
            is_orthographic,
        );

        let n_clusters = clusters.dimensions.x * clusters.dimensions.y * clusters.dimensions.z;
        let ambient_light = AmbientLight {
            color: ambient_light.color,
            brightness: ambient_light.brightness,
            affects_lightmapped_meshes: ambient_light.affects_lightmapped_meshes,
        };
        let ambient_light = maybe_ambient_override.unwrap_or(&ambient_light);

        let mut gpu_directional_lights = [GpuDirectionalLight::default(); MAX_DIRECTIONAL_LIGHTS];
        let mut num_directional_cascades_enabled_for_this_view = 0usize;
        let mut num_directional_lights_for_this_view = 0usize;
        for (index, light_entity) in directional_light_entities
            .iter()
            .filter(|light_entity| {
                directional_lights
                    .get(**light_entity)
                    .unwrap()
                    .3
                    .intersects(view_layers)
            })
            .enumerate()
            .take(MAX_DIRECTIONAL_LIGHTS)
        {
            let light = directional_lights.get(*light_entity).unwrap().2;

            num_directional_lights_for_this_view += 1;

            let mut flags = DirectionalLightFlags::NONE;

            // Lights are sorted, volumetric and shadow enabled lights are first
            if light.volumetric
                && light.shadow_maps_enabled
                && (index < directional_volumetric_enabled_count)
            {
                flags |= DirectionalLightFlags::VOLUMETRIC;
            }

            // Shadow enabled lights are second
            let mut num_cascades = 0;
            if light.shadow_maps_enabled {
                let cascades = light
                    .cascade_shadow_config
                    .bounds
                    .len()
                    .min(MAX_CASCADES_PER_LIGHT);

                if num_directional_cascades_enabled_for_this_view + cascades
                    <= max_texture_array_layers
                {
                    flags |= DirectionalLightFlags::SHADOW_MAPS_ENABLED;
                    num_cascades += cascades;
                }
            }

            if light.contact_shadows_enabled {
                flags |= DirectionalLightFlags::CONTACT_SHADOWS_ENABLED;
            }

            if light.affects_lightmapped_mesh_diffuse {
                flags |= DirectionalLightFlags::AFFECTS_LIGHTMAPPED_MESH_DIFFUSE;
            }

            // NVForever sun visibility.
            if nvf_outdoor_sun.contains(*light_entity) {
                flags |= DirectionalLightFlags::NVF_OUTDOOR_SUN;
            }

            gpu_directional_lights[index] = GpuDirectionalLight {
                // Filled in later.
                cascades: [GpuDirectionalCascade::default(); MAX_CASCADES_PER_LIGHT],
                // premultiply color by illuminance
                // we don't use the alpha at all, so no reason to multiply only [0..3]
                color: Vec4::from_slice(&light.color.to_f32_array()) * light.illuminance,
                // direction is negated to be ready for N.L
                dir_to_light: light.transform.back().into(),
                flags: flags.bits(),
                soft_shadow_size: light.soft_shadow_size.unwrap_or_default(),
                shadow_depth_bias: light.shadow_depth_bias,
                shadow_normal_bias: light.shadow_normal_bias,
                num_cascades: num_cascades as u32,
                cascades_overlap_proportion: light.cascade_shadow_config.overlap_proportion,
                depth_texture_base_index: num_directional_cascades_enabled_for_this_view as u32,
                sun_disk_angular_size: light.sun_disk_angular_size,
                sun_disk_intensity: light.sun_disk_intensity,
                decal_index: decals
                    .as_ref()
                    .and_then(|decals| decals.get(*light_entity))
                    .and_then(|index| index.try_into().ok())
                    .unwrap_or(u32::MAX),
            };
            num_directional_cascades_enabled_for_this_view += num_cascades;
        }

        let mut ambient_light_flags = AmbientLightFlags::NONE;

        if ambient_light.affects_lightmapped_meshes {
            ambient_light_flags |= AmbientLightFlags::AFFECTS_LIGHTMAPPED_MESHES;
        }

        let mut gpu_lights = GpuLights {
            directional_lights: gpu_directional_lights,
            ambient_color: Vec4::from_slice(&LinearRgba::from(ambient_light.color).to_f32_array())
                * ambient_light.brightness,
            cluster_factors: Vec4::new(
                clusters.dimensions.x as f32 / extracted_view.viewport.z as f32,
                clusters.dimensions.y as f32 / extracted_view.viewport.w as f32,
                cluster_factors_zw.x,
                cluster_factors_zw.y,
            ),
            cluster_dimensions: clusters.dimensions.extend(n_clusters),
            n_directional_lights: num_directional_lights_for_this_view as u32,
            // spotlight shadow maps are stored in the directional light array, starting at num_directional_cascades_enabled.
            // the spot lights themselves start in the light array at point_light_count. so to go from light
            // index to shadow map index, we need to subtract point light count and add directional shadowmap count.
            spot_light_shadowmap_offset: num_directional_cascades_enabled as i32
                - point_light_count as i32,
            ambient_light_flags: ambient_light_flags.bits(),
            n_rect_lights: 0,
            rect_lights: [GpuRectLight::default(); MAX_RECT_LIGHTS],
        };

        // directional lights
        // clear entities for lights that don't intersect the layer
        for light_entity in directional_light_entities.iter().filter(|light_entity| {
            !directional_lights
                .get(**light_entity)
                .unwrap()
                .3
                .intersects(view_layers)
        }) {
            let Ok(mut light_view_entities) =
                directional_light_view_entities.get_mut(*light_entity)
            else {
                continue;
            };
            if let Some(entities) = light_view_entities.remove(&entity) {
                despawn_entities(&mut commands, entities);
            }
        }

        let mut directional_depth_texture_array_index = 0u32;
        for (light_index, light_entity) in directional_light_entities
            .iter()
            .filter(|light_entity| {
                directional_lights
                    .get(**light_entity)
                    .unwrap()
                    .3
                    .intersects(view_layers)
            })
            .enumerate()
            .take(MAX_DIRECTIONAL_LIGHTS)
        {
            let (_, light_main_entity, light, light_render_layers) =
                directional_lights.get(*light_entity).unwrap();

            let Ok(mut light_view_entities) =
                directional_light_view_entities.get_mut(*light_entity)
            else {
                continue;
            };

            let gpu_light = &mut gpu_lights.directional_lights[light_index];

            // Only deal with cascades when shadows are enabled.
            if (gpu_light.flags & DirectionalLightFlags::SHADOW_MAPS_ENABLED.bits()) == 0u32 {
                if let Some(entities) = light_view_entities.remove(&entity) {
                    despawn_entities(&mut commands, entities);
                }
                continue;
            }

            let cascades = light
                .cascades
                .get(&entity)
                .unwrap()
                .iter()
                .take(MAX_CASCADES_PER_LIGHT);
            let frusta = light
                .frusta
                .get(&entity)
                .unwrap()
                .iter()
                .take(MAX_CASCADES_PER_LIGHT);

            let iter = cascades
                .zip(frusta)
                .zip(&light.cascade_shadow_config.bounds);

            let light_view_entities = light_view_entities.entry(entity).or_insert_with(|| {
                (0..iter.len())
                    .map(|_| commands.spawn_empty().id())
                    .collect()
            });
            if light_view_entities.len() != iter.len() {
                let entities = mem::take(light_view_entities);
                despawn_entities(&mut commands, entities);
                light_view_entities.extend((0..iter.len()).map(|_| commands.spawn_empty().id()));
            }

            for (cascade_index, (((cascade, frustum), bound), view_light_entity)) in
                iter.zip(light_view_entities.iter().copied()).enumerate()
            {
                gpu_lights.directional_lights[light_index].cascades[cascade_index] =
                    GpuDirectionalCascade {
                        clip_from_world: cascade.clip_from_world,
                        texel_size: cascade.texel_size,
                        far_bound: *bound,
                    };

                // NVForever cached shadow views: the layer's view is kept while the texture
                // is the same; the attachment below is still new for each view.
                let depth_texture_view = nvf_cascade_views
                    .entry(directional_depth_texture_array_index)
                    .or_insert_with(|| {
                        nvf_event(NVF_TEXTURE_VIEWS, 1);
                        directional_light_depth_texture
                            .texture
                            .create_view(&TextureViewDescriptor {
                                label: Some("directional_light_shadow_map_array_texture_view"),
                                format: None,
                                dimension: Some(TextureViewDimension::D2),
                                usage: None,
                                aspect: TextureAspect::All,
                                base_mip_level: 0,
                                mip_level_count: None,
                                base_array_layer: directional_depth_texture_array_index,
                                array_layer_count: Some(1u32),
                            })
                    })
                    .clone();

                // NOTE: For point and spotlights, we reuse the same depth attachment for all views.
                // However, for directional lights, we want a new depth attachment for each view,
                // so that the view is cleared for each view.
                let depth_attachment = DepthStencilViewAttachment::new(
                    DepthStencilViews::DepthOnly {
                        depth_view: depth_texture_view.clone(),
                    },
                    Some(0.0),
                    None,
                );

                directional_depth_texture_array_index += 1;

                let mut frustum = *frustum;
                // Push the near clip plane out to infinity for directional lights
                frustum.half_spaces[ViewFrustum::NEAR_PLANE_IDX] = HalfSpace::new(
                    frustum.half_spaces[ViewFrustum::NEAR_PLANE_IDX]
                        .normal()
                        .extend(f32::INFINITY),
                );

                let retained_view_entity = RetainedViewEntity::new(
                    *light_main_entity,
                    Some(camera_main_entity.into()),
                    cascade_index as u32,
                );

                commands.entity(view_light_entity).insert((
                    ShadowView {
                        depth_attachment,
                        pass_name: format!(
                            "shadow_directional_light_{light_index}_cascade_{cascade_index}"
                        ),
                    },
                    ExtractedView {
                        retained_view_entity,
                        viewport: UVec4::new(
                            0,
                            0,
                            directional_light_shadow_map.size as u32,
                            directional_light_shadow_map.size as u32,
                        ),
                        world_from_view: GlobalTransform::from(cascade.world_from_cascade),
                        clip_from_view: cascade.clip_from_cascade,
                        clip_from_world: Some(cascade.clip_from_world),
                        target_format: CORE_3D_SHADOW_MAP_FORMAT,
                        color_grading: Default::default(),
                        invert_culling: false,
                    },
                    frustum,
                    LightEntity::Directional {
                        light_entity: *light_entity,
                        cascade_index,
                    },
                    (*light_render_layers).clone(),
                ));

                if !matches!(gpu_preprocessing_mode, GpuPreprocessingMode::Culling) {
                    commands.entity(view_light_entity).insert(NoIndirectDrawing);
                }

                view_lights.push(view_light_entity);

                // If this light is using occlusion culling, add the appropriate components.
                if light.occlusion_culling {
                    commands.entity(view_light_entity).insert((
                        OcclusionCulling,
                        OcclusionCullingSubview { depth_texture_view },
                    ));
                    view_occlusion_culling_lights.push(view_light_entity);
                }

                // Subsequent views with the same light entity will **NOT** reuse the same shadow map
                // (Because the cascades are unique to each view)
                // TODO: Implement GPU culling for shadow passes.
                shadow_render_phases
                    .prepare_for_new_frame(retained_view_entity, gpu_preprocessing_mode);
                live_shadow_mapping_lights.insert(retained_view_entity);
            }
        }

        // Make a link from the camera to all shadow cascades with occlusion
        // culling enabled.
        if !view_occlusion_culling_lights.is_empty() {
            commands
                .entity(entity)
                .insert(OcclusionCullingSubviewEntities(
                    view_occlusion_culling_lights,
                ));
        }

        // Set up rect lights.
        //
        // These are per-view because we have no mechanism for "non-clustered but non-view-specific" lights.
        // We could introduce such a thing, but it may not be worth it for this fallback path that's only used
        // when we have too few storage buffers to cluster area lights.
        gpu_lights.n_rect_lights = 0;
        if !rect_lights_are_clustered {
            // TODO use light_render_layers whenever that is supported for rect_lights
            for (index, (_, _, rect_light, _)) in
                rect_lights.iter().enumerate().take(MAX_RECT_LIGHTS)
            {
                let right = rect_light.transform.right().into();
                let up = rect_light.transform.up().into();
                gpu_lights.rect_lights[index] = GpuRectLight {
                    color: rect_light.color.to_vec4() * rect_light.intensity,
                    position: rect_light.transform.translation(),
                    right,
                    up,
                    width: rect_light.width,
                    height: rect_light.height,
                    range: rect_light.range,
                };
                gpu_lights.n_rect_lights += 1;
            }
        }

        commands.entity(entity).insert((
            ViewShadowBindings {
                point_light_depth_texture: point_light_depth_texture.texture.clone(),
                point_light_depth_texture_view: point_light_depth_texture_view.clone(),
                nvf_point_light_far_depth_texture_view: nvf_far_depth_texture_view.clone(),
                directional_light_depth_texture: directional_light_depth_texture.texture.clone(),
                directional_light_depth_texture_view: directional_light_depth_texture_view.clone(),
            },
            ViewLightEntities {
                lights: view_lights,
            },
            ViewLightsUniformOffset {
                offset: view_gpu_lights_writer.write(&gpu_lights),
            },
        ));
    }

    // NVForever cached shadow views: kept for the next frame.
    nvf_view_cache.point_layers = point_light_depth_attachments;
    nvf_view_cache.spot_layers = directional_light_depth_attachments;
    nvf_view_cache.cascade_views = nvf_cascade_views;

    // NVForever light view counts.
    nvf_lap.lap(NVF_CAMERAS);

    // Mark the existing shadow maps as unused this frame so that the first
    // drawcall to them will clear them.
    for existing_shadow_view in &existing_shadow_views {
        existing_shadow_view
            .depth_attachment
            .prepare_for_new_frame();
    }

    // Despawn light-view entities for views that no longer exist
    for mut entities in &mut directional_light_view_entities {
        for (_, light_view_entities) in
            entities.extract_if(|entity, _| !live_views.contains(entity))
        {
            despawn_entities(&mut commands, light_view_entities);
        }
    }

    // NVForever light view counts: the phases dropped.
    let nvf_phases = shadow_render_phases.len();
    // NVForever kept shadow phases: the views of the lights the game holds keep theirs.
    let nvf_kept = |entity: &RetainedViewEntity| {
        live_shadow_mapping_lights.contains(entity)
            || nvf_parked.as_deref().is_some_and(|parked| parked.views.contains(entity))
    };
    shadow_render_phases.retain(|entity, _| nvf_kept(entity));
    // NVForever kept shadow views: a light gone from the render world (hidden, despawned)
    // has its views made anew when it is back.
    nvf_light_record.made_from.retain(|entity, _| point_lights.contains(*entity));
    nvf_event(NVF_PHASES_DROPPED, (nvf_phases - shadow_render_phases.len()) as u64);
    light_key_cache.retain(|entity, _| nvf_kept(entity));
    specialized_shadow_material_pipeline_cache.retain(|entity, _| nvf_kept(entity));
    // NVForever light view counts.
    nvf_lap.lap(NVF_SWEEP);
    if let Some(start) = nvf_start {
        nvf_time(NVF_PREPARE, start.elapsed());
    }
}

/// Creates six point shadow maps for a `RetainedViewEntity` identified by the `light_main_entity`
/// and the six cube face rotation indices. These shadow maps are shared across
/// all cameras.
fn create_point_shadow_maps(
    commands: &mut Commands,
    point_light_depth_attachments: &mut HashMap<u32, DepthStencilViewAttachment>,
    global_clusterable_object_meta: &ResMut<GlobalClusterableObjectMeta>,
    (cube_face_rotations, point_light_frusta, light_view_entities): (
        &Vec<Transform>,
        Option<&CubemapFrusta>,
        &Vec<Entity>,
    ),
    point_light_depth_texture: &CachedTexture,
    (light_entity, light_main_entity, light, light_render_layers): (
        &Entity,
        &MainEntity,
        &ExtractedPointLight,
        &RenderLayers,
    ),
    point_light_shadow_map_size: u32,
    gpu_preprocessing_support_max_supported_mode: GpuPreprocessingMode,
    // NVForever held light shadow maps: the light's slot, else its index.
    nvf_cube: Option<usize>,
) {
    let light_index = nvf_cube.unwrap_or_else(|| {
        *global_clusterable_object_meta
            .entity_to_index
            .get(light_entity)
            .unwrap()
    });
    // ignore scale because we don't want to effectively scale light radius and range
    // by applying those as a view transform to shadow map rendering of objects
    // and ignore rotation because we want the shadow map projections to align with the axes
    let view_translation = GlobalTransform::from_translation(light.transform.translation());

    let cube_face_projection = proj::perspective_infinite_reverse(
        core::f32::consts::FRAC_PI_2,
        1.0,
        light.shadow_map_near_z,
    );

    for (face_index, ((view_rotation, frustum), view_light_entity)) in cube_face_rotations
        .iter()
        .zip(&point_light_frusta.unwrap().frusta)
        .zip(light_view_entities.iter().copied())
        .enumerate()
    {
        // NVForever ALCS far maps: the attachment's key is the cube number's, the layer the
        // slot's in its texture.
        let nvf_key = (light_index * 6 + face_index) as u32;
        let base_array_layer = (if nvf_cube.is_some() { nvf_slot(light_index) } else { light_index } * 6 + face_index) as u32;

        let depth_attachment = point_light_depth_attachments
            .entry(nvf_key)
            .or_insert_with(|| {
                // NVForever light view counts (the attachments are kept: cached shadow views).
                nvf_event(NVF_TEXTURE_VIEWS, 1);
                let depth_texture_view =
                    point_light_depth_texture
                        .texture
                        .create_view(&TextureViewDescriptor {
                            label: Some("point_light_shadow_map_texture_view"),
                            format: None,
                            dimension: Some(TextureViewDimension::D2),
                            usage: None,
                            aspect: TextureAspect::All,
                            base_mip_level: 0,
                            mip_level_count: None,
                            base_array_layer,
                            array_layer_count: Some(1u32),
                        });

                DepthStencilViewAttachment::new(
                    DepthStencilViews::DepthOnly {
                        depth_view: depth_texture_view,
                    },
                    Some(0.0),
                    None,
                )
            })
            .clone();

        // Point light shadow maps are shared across all cameras,
        // so the retained view entity must not include the camera.
        let retained_view_entity =
            RetainedViewEntity::new(*light_main_entity, None, face_index as u32);

        commands.entity(view_light_entity).insert((
            ShadowView {
                depth_attachment,
                pass_name: format!(
                    "shadow_point_light_{}_{}",
                    light_index,
                    face_index_to_name(face_index)
                ),
            },
            ExtractedView {
                retained_view_entity,
                viewport: UVec4::new(
                    0,
                    0,
                    point_light_shadow_map_size,
                    point_light_shadow_map_size,
                ),
                world_from_view: view_translation * *view_rotation,
                clip_from_world: None,
                clip_from_view: cube_face_projection,
                target_format: CORE_3D_SHADOW_MAP_FORMAT,
                color_grading: Default::default(),
                invert_culling: false,
            },
            *frustum,
            LightEntity::Point {
                light_entity: *light_entity,
                face_index,
            },
            (*light_render_layers).clone(),
            RootNonCameraView(Core3d.intern()),
        ));

        if !matches!(
            gpu_preprocessing_support_max_supported_mode,
            GpuPreprocessingMode::Culling
        ) {
            commands.entity(view_light_entity).insert(NoIndirectDrawing);
        }
    }
}

/// Creates the spot shadow map for a `RetainedViewEntity` identified by the `light_main_entity`.
/// This shadow map is shared across all cameras.
fn create_spot_shadow_map(
    commands: &mut Commands,
    directional_light_depth_attachments: &mut HashMap<u32, DepthStencilViewAttachment>,
    (num_directional_cascades_enabled, light_index): (usize, usize),
    directional_light_depth_texture: &CachedTexture,
    view_light_entity: Entity,
    (light_entity, light_main_entity, light, light_render_layers): (
        &Entity,
        &MainEntity,
        &ExtractedPointLight,
        &RenderLayers,
    ),
    spot_light_frustum: Option<&Frustum>,
    directional_light_shadow_map_size: u32,
    gpu_preprocessing_support_max_supported_mode: GpuPreprocessingMode,
) {
    let spot_world_from_view = spot_light_world_from_view(&light.transform);
    let spot_world_from_view = spot_world_from_view.into();

    let angle = light.spot_light_angles.expect("lights should be sorted so that \
                [point_light_count..point_light_count + spot_light_shadow_maps_count] are spot lights").1;
    let spot_projection = spot_light_clip_from_view(angle, light.shadow_map_near_z);

    let base_array_layer = (num_directional_cascades_enabled + light_index) as u32;

    let depth_attachment = directional_light_depth_attachments
        .entry(base_array_layer)
        .or_insert_with(|| {
            // NVForever light view counts (the attachments are kept: cached shadow views).
            nvf_event(NVF_TEXTURE_VIEWS, 1);
            let depth_texture_view =
                directional_light_depth_texture
                    .texture
                    .create_view(&TextureViewDescriptor {
                        label: Some("spot_light_shadow_map_texture_view"),
                        format: None,
                        dimension: Some(TextureViewDimension::D2),
                        usage: None,
                        aspect: TextureAspect::All,
                        base_mip_level: 0,
                        mip_level_count: None,
                        base_array_layer,
                        array_layer_count: Some(1u32),
                    });

            DepthStencilViewAttachment::new(
                DepthStencilViews::DepthOnly {
                    depth_view: depth_texture_view,
                },
                Some(0.0),
                None,
            )
        })
        .clone();

    let retained_view_entity = RetainedViewEntity::new(*light_main_entity, None, 0);
    commands.entity(view_light_entity).insert((
        ShadowView {
            depth_attachment,
            pass_name: format!("shadow_spot_light_{light_index}"),
        },
        ExtractedView {
            retained_view_entity,
            viewport: UVec4::new(
                0,
                0,
                directional_light_shadow_map_size,
                directional_light_shadow_map_size,
            ),
            world_from_view: spot_world_from_view,
            clip_from_view: spot_projection,
            clip_from_world: None,
            target_format: CORE_3D_SHADOW_MAP_FORMAT,
            color_grading: Default::default(),
            invert_culling: false,
        },
        *spot_light_frustum.unwrap(),
        LightEntity::Spot {
            light_entity: *light_entity,
        },
        (*light_render_layers).clone(),
        RootNonCameraView(Core3d.intern()),
    ));

    if !matches!(
        gpu_preprocessing_support_max_supported_mode,
        GpuPreprocessingMode::Culling
    ) {
        commands.entity(view_light_entity).insert(NoIndirectDrawing);
    }
}

fn despawn_entities(commands: &mut Commands, entities: Vec<Entity>) {
    if entities.is_empty() {
        return;
    }
    commands.queue(move |world: &mut World| {
        for entity in entities {
            world.despawn(entity);
        }
    });
}

#[derive(Resource, Deref, DerefMut, Default, Debug, Clone)]
pub struct LightKeyCache(HashMap<RetainedViewEntity, MeshPipelineKey>);

#[derive(Resource, Deref, DerefMut, Default)]
pub struct SpecializedShadowMaterialPipelineCache {
    // view light entity -> view pipeline cache
    #[deref]
    map: HashMap<RetainedViewEntity, SpecializedShadowMaterialViewPipelineCache>,
}

#[derive(Deref, DerefMut, Default)]
pub struct SpecializedShadowMaterialViewPipelineCache {
    #[deref]
    map: MainEntityHashMap<(CachedRenderPipelineId, DrawFunctionId, bool)>,
}

pub fn check_views_lights_need_specialization(
    view_light_entities: Query<(&LightEntity, &ExtractedView)>,
    shadow_render_phases: Res<ViewBinnedRenderPhases<Shadow>>,
    mut light_key_cache: ResMut<LightKeyCache>,
    mut dirty_specializations: ResMut<DirtySpecializations>,
) {
    for (light_entity, extracted_view_light) in &view_light_entities {
        if !shadow_render_phases.contains_key(&extracted_view_light.retained_view_entity) {
            continue;
        }

        let is_directional_light = matches!(light_entity, LightEntity::Directional { .. });
        let mut light_key = MeshPipelineKey::DEPTH_PREPASS;
        light_key.set(
            MeshPipelineKey::VIEW_PROJECTION_ORTHOGRAPHIC | MeshPipelineKey::UNCLIPPED_DEPTH_ORTHO,
            is_directional_light,
        );
        light_key.set(
            MeshPipelineKey::VIEW_PROJECTION_PERSPECTIVE,
            !is_directional_light,
        );
        if let Some(current_key) =
            light_key_cache.get_mut(&extracted_view_light.retained_view_entity)
        {
            if *current_key != light_key {
                light_key_cache.insert(extracted_view_light.retained_view_entity, light_key);
                dirty_specializations
                    .views
                    .insert(extracted_view_light.retained_view_entity);
            }
        } else {
            light_key_cache.insert(extracted_view_light.retained_view_entity, light_key);
            dirty_specializations
                .views
                .insert(extracted_view_light.retained_view_entity);
        }
    }
}

pub(crate) struct ShadowSpecializationWorkItem {
    visible_entity: MainEntity,
    retained_view_entity: RetainedViewEntity,
    mesh_key: MeshPipelineKey,
    layout: MeshVertexBufferLayoutRef,
    properties: Arc<MaterialProperties>,
    material_type_id: TypeId,
}

/// Holds all entities with mesh materials for which the shadow pass couldn't be
/// specialized and/or queued because their materials hadn't loaded yet.
///
/// See the [`PendingQueues`] documentation for more information.
#[derive(Default, Deref, DerefMut, Resource)]
pub struct PendingShadowQueues(pub PendingQueues);

#[derive(SystemParam)]
pub(crate) struct SpecializeShadowsSystemParam<'w, 's> {
    render_meshes: Res<'w, RenderAssets<RenderMesh>>,
    render_mesh_instances: Res<'w, RenderMeshInstances>,
    render_materials: Res<'w, ErasedRenderAssets<PreparedMaterial>>,
    render_material_instances: Res<'w, RenderMaterialInstances>,
    // NVForever kept shadow phases: written for the kept views' removed entities.
    shadow_render_phases: ResMut<'w, ViewBinnedRenderPhases<Shadow>>,
    render_lightmaps: Res<'w, RenderLightmaps>,
    view_light_entities: Query<'w, 's, (&'static LightEntity, &'static ExtractedView)>,
    shadow_map_visible_entities_query: Query<'w, 's, &'static RenderShadowMapVisibleEntities>,
    light_key_cache: Res<'w, LightKeyCache>,
    specialized_shadow_material_pipeline_cache: ResMut<'w, SpecializedShadowMaterialPipelineCache>,
    pending_shadow_queues: ResMut<'w, PendingShadowQueues>,
    dirty_specializations: Res<'w, DirtySpecializations>,
    // NVForever kept shadow phases.
    nvf_parked: Option<ResMut<'w, NvfParkedShadowViews>>,
}

pub(crate) fn specialize_shadows(
    world: &mut World,
    state: &mut SystemState<SpecializeShadowsSystemParam>,
    mut work_items: Local<Vec<ShadowSpecializationWorkItem>>,
    mut all_shadow_views: Local<HashSet<RetainedViewEntity, FixedHasher>>,
    // NVForever kept shadow phases: kept views to drop this frame, the views live last frame,
    // and the entities of a view to specialize.
    mut nvf_wiped: Local<Vec<RetainedViewEntity>>,
    mut nvf_live_before: Local<HashSet<RetainedViewEntity, FixedHasher>>,
    mut nvf_items: Local<Vec<(Entity, MainEntity)>>,
) {
    work_items.clear();
    all_shadow_views.clear();

    {
        let SpecializeShadowsSystemParam {
            render_meshes,
            render_mesh_instances,
            render_materials,
            render_material_instances,
            mut shadow_render_phases,
            render_lightmaps,
            view_light_entities,
            shadow_map_visible_entities_query,
            light_key_cache,
            mut specialized_shadow_material_pipeline_cache,
            mut pending_shadow_queues,
            dirty_specializations,
            mut nvf_parked,
        } = state.get_mut(world).unwrap();
        if let Some(parked) = nvf_parked.as_deref_mut() {
            parked.resumed.clear();
        }

        for (light_entity, extracted_view_light) in &view_light_entities {
            all_shadow_views.insert(extracted_view_light.retained_view_entity);

            if !shadow_render_phases.contains_key(&extracted_view_light.retained_view_entity) {
                continue;
            }
            let Some(light_key) = light_key_cache.get(&extracted_view_light.retained_view_entity)
            else {
                continue;
            };

            let visible_entities = get_shadow_map_visible_entities(
                &shadow_map_visible_entities_query,
                light_entity,
                extracted_view_light,
            );

            let mut maybe_specialized_shadow_material_pipeline_cache =
                specialized_shadow_material_pipeline_cache
                    .get_mut(&extracted_view_light.retained_view_entity);

            // Remove cached pipeline IDs corresponding to entities that
            // either have been removed or need to be respecialized.
            if let Some(ref mut specialized_shadow_material_pipeline_cache) =
                maybe_specialized_shadow_material_pipeline_cache
            {
                if dirty_specializations
                    .must_wipe_specializations_for_view(extracted_view_light.retained_view_entity)
                {
                    specialized_shadow_material_pipeline_cache.clear();
                } else {
                    for &renderable_entity in dirty_specializations.iter_to_despecialize() {
                        specialized_shadow_material_pipeline_cache.remove(&renderable_entity);
                    }
                }
            }

            // Initialize the pending queues.
            let view_pending_shadow_queues = pending_shadow_queues
                .prepare_for_new_frame(extracted_view_light.retained_view_entity);

            // NOTE: Lights with shadow mapping disabled will have no visible entities
            // so no meshes will be queued

            let Some(visible_entities) = visible_entities.get::<Mesh3d>() else {
                continue;
            };

            // NVForever kept shadow phases: a kept view back after a frame without a view
            // missed its light's visible-list changes of those frames, so every visible entity
            // is looked at (the specialized ones are skipped below), as when the view is new.
            let view = extracted_view_light.retained_view_entity;
            let resumed = nvf_parked.as_deref().is_some_and(|parked| parked.active)
                && !nvf_live_before.contains(&view)
                && !dirty_specializations.must_wipe_specializations_for_view(view);
            nvf_items.clear();
            if resumed {
                if let Some(parked) = nvf_parked.as_deref_mut() {
                    parked.resumed.insert(view);
                }
                nvf_items.extend(
                    visible_entities
                        .iter_visible()
                        .chain(view_pending_shadow_queues.prev_frame.iter().filter_map(
                            |(entity, main_entity)| {
                                visible_entities
                                    .entity_pair_is_visible(*entity, *main_entity)
                                    .then_some((entity, main_entity))
                            },
                        ))
                        .map(|(entity, main_entity)| (*entity, *main_entity)),
                );
            } else {
                nvf_items.extend(
                    dirty_specializations
                        .iter_to_specialize(view, visible_entities, &view_pending_shadow_queues.prev_frame)
                        .map(|(entity, main_entity)| (*entity, *main_entity)),
                );
            }

            // Now process all shadow meshes that need to be re-specialized.
            for (render_entity, visible_entity) in nvf_items.iter().map(|(entity, main_entity)| (entity, main_entity)) {
                if maybe_specialized_shadow_material_pipeline_cache
                    .as_ref()
                    .is_some_and(|specialized_shadow_material_pipeline_cache| {
                        specialized_shadow_material_pipeline_cache.contains_key(visible_entity)
                    })
                {
                    continue;
                }

                // Check for material instance, mesh, and material. If any of
                // these fail, it's probably because the relevant asset hasn't
                // loaded yet. In that case, add the entity to the list of
                // pending mesh materials and bail.
                let Some(material_instance) =
                    render_material_instances.instances.get(visible_entity)
                else {
                    view_pending_shadow_queues
                        .current_frame
                        .insert((*render_entity, *visible_entity));
                    continue;
                };
                let Some(mesh_instance) =
                    render_mesh_instances.render_mesh_queue_data(*visible_entity)
                else {
                    view_pending_shadow_queues
                        .current_frame
                        .insert((*render_entity, *visible_entity));
                    continue;
                };
                let Some(material) = render_materials.get(material_instance.asset_id) else {
                    view_pending_shadow_queues
                        .current_frame
                        .insert((*render_entity, *visible_entity));
                    continue;
                };

                if !material.properties.shadows_enabled {
                    // If the material is not a shadow caster, we don't need to specialize it.
                    continue;
                }
                if !mesh_instance
                    .flags()
                    .contains(RenderMeshInstanceFlags::SHADOW_CASTER)
                {
                    continue;
                }
                let Some(mesh) = render_meshes.get(mesh_instance.mesh_asset_id()) else {
                    continue;
                };

                let mut mesh_key =
                    *light_key | MeshPipelineKey::from_bits_retain(mesh.key_bits.bits());

                // Even though we don't use the lightmap in the shadow map, the
                // `SetMeshBindGroup` render command will bind the data for it. So
                // we need to include the appropriate flag in the mesh pipeline key
                // to ensure that the necessary bind group layout entries are
                // present.
                if render_lightmaps
                    .render_lightmaps
                    .contains_key(visible_entity)
                {
                    mesh_key |= MeshPipelineKey::LIGHTMAPPED;
                }

                mesh_key |= match material.properties.alpha_mode {
                    AlphaMode::Mask(_)
                    | AlphaMode::Blend
                    | AlphaMode::Premultiplied
                    | AlphaMode::Add
                    | AlphaMode::AlphaToCoverage => MeshPipelineKey::MAY_DISCARD,
                    _ => MeshPipelineKey::NONE,
                };

                if material.properties.prepass_reads_material() {
                    mesh_key |= MeshPipelineKey::PREPASS_READS_MATERIAL;
                }

                work_items.push(ShadowSpecializationWorkItem {
                    visible_entity: *visible_entity,
                    retained_view_entity: extracted_view_light.retained_view_entity,
                    mesh_key,
                    layout: mesh.layout.clone(),
                    properties: material.properties.clone(),
                    material_type_id: material_instance.asset_id.type_id(),
                });
            }
        }

        // NVForever kept shadow phases (engine/vendor/README.md): a kept view with no view
        // entity this frame missed this frame's changes. An entity removed is dropped from its
        // pipelines and from its bin (from the bin too since 2026-11-25: one removed and then
        // given another mesh or material in a later frame was no longer among the pipelines,
        // so the change went unseen, and back in view it was drawn from its old bin, with
        // the old pipeline, material and mesh); an entity that changed (its mesh, material or
        // flags) leaves the kept view's bin and pipeline stale, so that view is dropped whole
        // and drawn again as a new one. Then the kept views keep their pending queues and
        // pipelines like the live ones.
        nvf_live_before.clear();
        nvf_live_before.extend(view_light_entities.iter().map(|(_, view)| view.retained_view_entity));
        if let Some(parked) = nvf_parked.as_deref().filter(|parked| parked.active) {
            let (changed, removed) = (
                &dirty_specializations.changed_renderables,
                &dirty_specializations.removed_renderables,
            );
            for view in parked.views.iter().filter(|view| !all_shadow_views.contains(*view)) {
                if !removed.is_empty() {
                    if let Some(phase) = shadow_render_phases.get_mut(view) {
                        if phase.nvf_binned_len() < removed.len() {
                            phase.nvf_retain_binned(|entity, _| !removed.contains(&entity));
                        } else {
                            for entity in removed {
                                phase.remove(*entity);
                            }
                        }
                    }
                }
                let Some(cache) = specialized_shadow_material_pipeline_cache.get_mut(view) else {
                    continue;
                };
                let stale = if cache.len() < changed.len() {
                    cache.keys().any(|entity| changed.contains(entity))
                } else {
                    changed.iter().any(|entity| cache.contains_key(entity))
                };
                if stale {
                    nvf_wiped.push(*view);
                } else if cache.len() < removed.len() {
                    cache.retain(|entity, _| !removed.contains(entity));
                } else {
                    for entity in removed {
                        cache.remove(entity);
                    }
                }
            }
            all_shadow_views.extend(parked.views.iter().copied().filter(|view| !nvf_wiped.contains(view)));
        }

        pending_shadow_queues.expire_stale_views(&all_shadow_views);
    }

    // NVForever kept shadow phases: the stale kept views go (their phase, key, pipelines and
    // pending queue), as a light's did with its views.
    if !nvf_wiped.is_empty() {
        nvf_event(NVF_PARKED_WIPED, nvf_wiped.len() as u64);
        for view in nvf_wiped.drain(..) {
            world.resource_mut::<ViewBinnedRenderPhases<Shadow>>().remove(&view);
            world.resource_mut::<LightKeyCache>().remove(&view);
            world.resource_mut::<SpecializedShadowMaterialPipelineCache>().remove(&view);
            world.resource_mut::<PendingShadowQueues>().remove(&view);
        }
    }

    let depth_clip_control_supported = world
        .resource::<PrepassPipeline>()
        .depth_clip_control_supported;

    for item in work_items.drain(..) {
        let Some(prepass_specialize) = item.properties.prepass_specialize else {
            continue;
        };

        let key = ErasedMaterialPipelineKey {
            type_id: item.material_type_id,
            mesh_key: ErasedMeshPipelineKey::new(item.mesh_key),
            material_key: item.properties.material_key.clone(),
        };

        let emulate_unclipped_depth = item
            .mesh_key
            .contains(MeshPipelineKey::UNCLIPPED_DEPTH_ORTHO)
            && !depth_clip_control_supported;
        let is_depth_only_opaque = !item
            .mesh_key
            .intersects(MeshPipelineKey::MAY_DISCARD | MeshPipelineKey::PREPASS_READS_MATERIAL)
            && !emulate_unclipped_depth;
        let draw_function = if is_depth_only_opaque {
            item.properties
                .get_draw_function(ShadowsDepthOnlyDrawFunction)
        } else {
            item.properties.get_draw_function(ShadowsDrawFunction)
        };

        let Some(draw_function) = draw_function else {
            continue;
        };

        match prepass_specialize(world, &key, &item.layout, &item.properties) {
            Ok(pipeline_id) => {
                world
                    .resource_mut::<SpecializedShadowMaterialPipelineCache>()
                    .entry(item.retained_view_entity)
                    .or_default()
                    .insert(
                        item.visible_entity,
                        (pipeline_id, draw_function, is_depth_only_opaque),
                    );
            }
            Err(err) => error!("{}", err),
        }
    }

    // Delete specialized pipelines belonging to views that have expired.
    world
        .resource_mut::<SpecializedShadowMaterialPipelineCache>()
        .retain(|view, _| all_shadow_views.contains(view));
}

/// For each shadow cascade, iterates over all the meshes "visible" from it and
/// adds them to [`BinnedRenderPhase`]s or [`SortedRenderPhase`]s as
/// appropriate.
pub fn queue_shadows(
    render_mesh_instances: Res<RenderMeshInstances>,
    render_materials: Res<ErasedRenderAssets<PreparedMaterial>>,
    render_material_instances: Res<RenderMaterialInstances>,
    mut shadow_render_phases: ResMut<ViewBinnedRenderPhases<Shadow>>,
    gpu_preprocessing_support: Res<GpuPreprocessingSupport>,
    mesh_allocator: Res<MeshAllocator>,
    view_light_entities: Query<(&LightEntity, &ExtractedView, &RenderLayers, Has<RootNonCameraView>)>,
    shadow_map_visible_entities_query: Query<&RenderShadowMapVisibleEntities>,
    specialized_material_pipeline_cache: Res<SpecializedShadowMaterialPipelineCache>,
    mut pending_shadow_queues: ResMut<PendingShadowQueues>,
    dirty_specializations: Res<DirtySpecializations>,
    mut mesh_instances_queued_this_iteration_scratch_space: Local<MainEntityHashSet>,
    // NVForever kept shadow phases: the views resumed this frame, and the entities to queue.
    nvf_parked: Option<Res<NvfParkedShadowViews>>,
    mut nvf_items: Local<Vec<(Entity, MainEntity)>>,
    // NVForever ALCS unready casters: whether pipelines are still being built.
    nvf_pipeline_cache: Res<PipelineCache>,
) {
    let (mut nvf_resynced, mut nvf_rebinned, mut nvf_stale) = (0u64, 0u64, 0u64);
    // NVForever ALCS unready casters: a pipeline still being built anywhere may be one a
    // caster already binned waits for (it is skipped when drawn, and nothing tells).
    let nvf_building = nvf_pipeline_cache.waiting_pipelines().next().is_some();
    // NVForever kept shadow phases: only while the game's hold keeps them.
    let nvf_parked = nvf_parked.filter(|parked| parked.active);
    let nvf_guard = nvf_parked.is_some();
    for (light_entity, extracted_view_light, view_light_render_layers, nvf_root) in &view_light_entities {
        let Some(shadow_phase) =
            shadow_render_phases.get_mut(&extracted_view_light.retained_view_entity)
        else {
            continue;
        };
        // NVForever ALCS unready casters: casters of this view that cannot be drawn yet.
        let mut nvf_unready = 0u32;

        let Some(view_specialized_material_pipeline_cache) =
            specialized_material_pipeline_cache.get(&extracted_view_light.retained_view_entity)
        else {
            continue;
        };

        // Fetch the pending mesh material queues for this view.
        let view_pending_shadow_queues = pending_shadow_queues
            .get_mut(&extracted_view_light.retained_view_entity)
            .expect("View pending shadow queues should have been created in `specialize_shadows`");

        let visible_entities = get_shadow_map_visible_entities(
            &shadow_map_visible_entities_query,
            light_entity,
            extracted_view_light,
        );

        let Some(visible_entities) = visible_entities.get::<Mesh3d>() else {
            continue;
        };

        // First, remove meshes that need to be respecialized, and those that were removed, from the bins.
        for &main_entity in dirty_specializations
            .iter_to_dequeue(extracted_view_light.retained_view_entity, visible_entities)
        {
            shadow_phase.remove(main_entity);
        }

        // NVForever kept shadow phases (engine/vendor/README.md): a kept phase whose view is
        // back missed the visible-list changes of the frames it had none (its light's list kept
        // changing, or was made anew, every visible entity "added"), so it is resynced with the
        // whole list: entities no longer visible leave, visible ones not binned are queued.
        nvf_items.clear();
        if nvf_parked
            .as_deref()
            .is_some_and(|parked| parked.resumed.contains(&extracted_view_light.retained_view_entity))
        {
            nvf_resynced += 1;
            let (_, stale) = nvf_resync_kept_phase(
                shadow_phase,
                visible_entities,
                |main_entity| {
                    render_mesh_instances
                        .render_mesh_queue_data(main_entity)
                        .map(|mesh_instance| mesh_instance.current_uniform_index)
                },
                &mut nvf_items,
            );
            nvf_stale += stale as u64;
            mesh_instances_queued_this_iteration_scratch_space.clear();
            mesh_instances_queued_this_iteration_scratch_space
                .extend(nvf_items.iter().map(|(_, main_entity)| *main_entity));
            // (Only the pending pairs still visible, as `specialize_shadows` and upstream's
            // `iter_to_queue` take them: one gone since read as a caster not ready and had
            // the light drawn once more for nothing.)
            nvf_items.extend(
                view_pending_shadow_queues
                    .prev_frame
                    .iter()
                    .filter(|(entity, main_entity)| {
                        visible_entities.entity_pair_is_visible(*entity, *main_entity)
                            && mesh_instances_queued_this_iteration_scratch_space.insert(*main_entity)
                    })
                    .copied(),
            );
        } else {
            nvf_items.extend(
                dirty_specializations
                    .iter_to_queue(
                        extracted_view_light.retained_view_entity,
                        visible_entities,
                        &view_pending_shadow_queues.prev_frame,
                        &mut mesh_instances_queued_this_iteration_scratch_space,
                    )
                    .map(|(entity, main_entity)| (*entity, *main_entity)),
            );
        }

        // Now iterate through all newly-visible entities and those needing respecialization.
        for (render_entity, main_entity) in nvf_items.iter().map(|(entity, main_entity)| (entity, main_entity)) {
            let Some(&(pipeline_id, draw_function, is_depth_only_opaque)) =
                view_specialized_material_pipeline_cache.get(main_entity)
            else {
                // NVForever ALCS unready casters: not specialized, for its mesh or its
                // material is not there yet (one that casts no shadows never is).
                nvf_unready += u32::from(nvf_guard && nvf_caster_to_be(
                    *main_entity,
                    &render_mesh_instances,
                    &render_material_instances,
                    &render_materials,
                ));
                continue;
            };

            let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(*main_entity)
            else {
                // We couldn't fetch the mesh, probably because it hasn't
                // loaded yet. Add the entity to the list of pending shadows
                // and bail.
                view_pending_shadow_queues
                    .current_frame
                    .insert((*render_entity, *main_entity));
                nvf_unready += 1;
                continue;
            };
            if !mesh_instance
                .flags()
                .contains(RenderMeshInstanceFlags::SHADOW_CASTER)
            {
                continue;
            }

            let mesh_layers = mesh_instance.render_layers.as_ref().unwrap_or_default();
            if !view_light_render_layers.intersects(mesh_layers) {
                continue;
            }

            let material_bind_group_index = if is_depth_only_opaque {
                None
            } else {
                let Some(material_instance) = render_material_instances.instances.get(main_entity)
                else {
                    continue;
                };
                let Some(material) = render_materials.get(material_instance.asset_id) else {
                    // We couldn't fetch the material, probably because the
                    // material hasn't been loaded yet. Add the entity to the
                    // list of pending shadows and bail.
                    view_pending_shadow_queues
                        .current_frame
                        .insert((*render_entity, *main_entity));
                    nvf_unready += 1;
                    continue;
                };
                Some(material.binding.group.0)
            };

            let Some(mesh_slabs) = mesh_allocator.mesh_slabs(&mesh_instance.mesh_asset_id()) else {
                nvf_unready += 1;
                continue;
            };
            // NVForever ALCS unready casters: binned, but skipped when drawn until its
            // pipeline is built.
            nvf_unready += u32::from(nvf_guard && nvf_pipeline_cache.get_render_pipeline(pipeline_id).is_none());

            let batch_set_key = ShadowBatchSetKey {
                pipeline: pipeline_id,
                draw_function,
                material_bind_group_index,
                slabs: mesh_slabs,
            };

            // NVForever kept shadow phases: never binned twice (a panic upstream); without
            // kept phases upstream's `add`.
            nvf_rebinned += u64::from(nvf_add_once(
                nvf_guard,
                shadow_phase,
                batch_set_key,
                ShadowBinKey {
                    asset_id: mesh_instance.mesh_asset_id().into(),
                },
                (*render_entity, *main_entity),
                mesh_instance.current_uniform_index,
                BinnedRenderPhaseType::mesh(
                    mesh_instance.should_batch(),
                    &gpu_preprocessing_support,
                ),
            ));
        }
        // NVForever ALCS unready casters: a point or spot light's view drawn this frame
        // without all of its casters is told to the game.
        if nvf_guard && nvf_root && (nvf_unready > 0 || nvf_building) && !matches!(light_entity, LightEntity::Directional { .. }) {
            nvf_report_unready(extracted_view_light.retained_view_entity.main_entity.id(), nvf_unready, nvf_building);
        }
    }
    nvf_event(NVF_RESYNCED, nvf_resynced);
    nvf_event(NVF_REBINNED, nvf_rebinned);
    nvf_event(NVF_STALE_BINNED, nvf_stale);
}

/// NVForever ALCS unready casters (2026-11-24): whether an entity a light sees, not yet
/// specialized for its shadow views, is a caster to be: its mesh or its material is not in
/// the render world yet, or both are and it casts (its mesh is then still being uploaded).
fn nvf_caster_to_be(
    main_entity: MainEntity,
    render_mesh_instances: &RenderMeshInstances,
    render_material_instances: &RenderMaterialInstances,
    render_materials: &ErasedRenderAssets<PreparedMaterial>,
) -> bool {
    let Some(mesh_instance) = render_mesh_instances.render_mesh_queue_data(main_entity) else {
        return true;
    };
    if !mesh_instance.flags().contains(RenderMeshInstanceFlags::SHADOW_CASTER) {
        return false;
    }
    match render_material_instances.instances.get(&main_entity) {
        None => true,
        Some(instance) => render_materials
            .get(instance.asset_id)
            .is_none_or(|material| material.properties.shadows_enabled),
    }
}

/// NVForever ALCS unready casters: the lights (main world entities) whose shadow views were
/// drawn while casters of theirs could not be (a mesh or a material not loaded, a pipeline
/// still being built), with how many and whether any pipeline was being built then, since
/// the game last took them. Bevy draws such a view
/// again next frame and the caster appears; a light the game holds is not drawn again, so
/// the game asks and marks it ([`nvf_take_unready_shadow_lights`]).
static NVF_UNREADY: std::sync::Mutex<Vec<(Entity, u32, bool)>> = std::sync::Mutex::new(Vec::new());

fn nvf_report_unready(light: Entity, casters: u32, building: bool) {
    if let Ok(mut unready) = NVF_UNREADY.lock() {
        // The game takes them every frame while its lamps cast; never without bound.
        if unready.len() >= 4096 {
            unready.clear();
        }
        unready.push((light, casters, building));
    }
}

/// NVForever ALCS unready casters: take what [`NVF_UNREADY`] holds.
pub fn nvf_take_unready_shadow_lights() -> Vec<(Entity, u32, bool)> {
    NVF_UNREADY.lock().map(|mut unready| mem::take(&mut *unready)).unwrap_or_default()
}

pub struct Shadow {
    /// Determines which objects can be placed into a *batch set*.
    ///
    /// Objects in a single batch set can potentially be multi-drawn together,
    /// if it's enabled and the current platform supports it.
    pub batch_set_key: ShadowBatchSetKey,
    /// Information that separates items into bins.
    pub bin_key: ShadowBinKey,
    pub representative_entity: (Entity, MainEntity),
    pub batch_range: Range<u32>,
    pub extra_index: PhaseItemExtraIndex,
}

/// Information that must be identical in order to place opaque meshes in the
/// same *batch set*.
///
/// A batch set is a set of batches that can be multi-drawn together, if
/// multi-draw is in use.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShadowBatchSetKey {
    /// The identifier of the render pipeline.
    pub pipeline: CachedRenderPipelineId,

    /// The function used to draw.
    pub draw_function: DrawFunctionId,

    /// The ID of a bind group specific to the material.
    ///
    /// In the case of PBR, this is the `MaterialBindGroupIndex`.
    pub material_bind_group_index: Option<u32>,

    /// The IDs of the slabs of GPU memory in the mesh allocator that contain
    /// the mesh data.
    ///
    /// For non-mesh items, you can fill the [`MeshSlabs::vertex_slab_id`] with
    /// 0 if your items can be multi-drawn, or with a unique value if they
    /// can't.
    pub slabs: MeshSlabs,
}

impl PhaseItemBatchSetKey for ShadowBatchSetKey {
    fn indexed(&self) -> bool {
        self.slabs.index_slab_id.is_some()
    }
}

/// Data used to bin each object in the shadow map phase.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShadowBinKey {
    /// The object.
    pub asset_id: UntypedAssetId,
}

impl PhaseItem for Shadow {
    #[inline]
    fn entity(&self) -> Entity {
        self.representative_entity.0
    }

    fn main_entity(&self) -> MainEntity {
        self.representative_entity.1
    }

    #[inline]
    fn draw_function(&self) -> DrawFunctionId {
        self.batch_set_key.draw_function
    }

    #[inline]
    fn batch_range(&self) -> &Range<u32> {
        &self.batch_range
    }

    #[inline]
    fn batch_range_mut(&mut self) -> &mut Range<u32> {
        &mut self.batch_range
    }

    #[inline]
    fn extra_index(&self) -> PhaseItemExtraIndex {
        self.extra_index.clone()
    }

    #[inline]
    fn batch_range_and_extra_index_mut(&mut self) -> (&mut Range<u32>, &mut PhaseItemExtraIndex) {
        (&mut self.batch_range, &mut self.extra_index)
    }
}

impl BinnedPhaseItem for Shadow {
    type BatchSetKey = ShadowBatchSetKey;
    type BinKey = ShadowBinKey;

    #[inline]
    fn new(
        batch_set_key: Self::BatchSetKey,
        bin_key: Self::BinKey,
        representative_entity: (Entity, MainEntity),
        batch_range: Range<u32>,
        extra_index: PhaseItemExtraIndex,
    ) -> Self {
        Shadow {
            batch_set_key,
            bin_key,
            representative_entity,
            batch_range,
            extra_index,
        }
    }
}

impl CachedRenderPipelinePhaseItem for Shadow {
    #[inline]
    fn cached_pipeline(&self) -> CachedRenderPipelineId {
        self.batch_set_key.pipeline
    }
}

pub const EARLY_SHADOW_PASS: bool = false;
pub const LATE_SHADOW_PASS: bool = true;

/// Renders the shadow maps that aren't associated with a specific view.
///
/// At present, these consist of the point and spot light shadow maps.
pub fn shared_shadow_pass<const IS_LATE: bool>(
    world: &World,
    // NVForever static shadow cache: a face drawn into the cache or composited from it.
    view_light_query: ViewQuery<(Entity, &ShadowView, &ExtractedView, Has<OcclusionCulling>, Option<&NvfStaticShadowFace>)>,
    shadow_render_phases: Res<ViewBinnedRenderPhases<Shadow>>,
    mut ctx: RenderContext,
) {
    let (view_light_entity, view_light, extracted_light_view, occlusion_culling, nvf_static_face) =
        view_light_query.into_inner();
    view_shadow_pass::<IS_LATE>(
        view_light_entity,
        view_light,
        extracted_light_view,
        occlusion_culling,
        world,
        &shadow_render_phases,
        &mut ctx,
        nvf_static_face,
    );
}

/// Renders the shadow maps that are associated with a specific view.
///
/// At present, these consist of the directional light shadows.
/// NVForever held shadow maps (2026-11-10, engine/vendor/README.md). The cascades of
/// directional lights whose bit is set in this render-world resource (bit `i` for cascade
/// index `i`) are not drawn this frame: the shadow pass skips their views
/// ([`per_view_shadow_pass`]), and so do the compute passes that prepare those views' draws
/// (gpu_preprocess.rs `gather_shadow_cascades_for_view`). Their maps keep what the last
/// frame that drew them left in them, so whoever sets it must also give the lights those
/// cascades as that frame had them (the game: nvforever shadow_rate.rs). Point and spot
/// lights' maps are drawn as ever. Bevy inserts none; without it nothing changes.
#[derive(Resource, Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct HoldDirectionalShadowMaps(pub u32);

impl HoldDirectionalShadowMaps {
    /// Every cascade held.
    pub const ALL: Self = Self(u32::MAX);

    /// Whether cascade `index`'s map is kept from an earlier frame.
    pub fn holds(&self, index: usize) -> bool {
        index < 32 && self.0 & (1 << index) != 0
    }
}

impl bevy_render::extract_resource::ExtractResource<RenderApp> for HoldDirectionalShadowMaps {
    type Source = HoldDirectionalShadowMaps;
    fn extract_resource(source: &Self::Source) -> Self {
        *source
    }
}

/// NVForever held shadow maps (#3779, [design]): high-water layer count for the directional
/// shadow map array texture. While the shadow map's side (resolution) stays the same, the
/// requested layer count only grows up to `max_layers` so switching between third person,
/// first person (`ArmsShade`), and the flashlight reuses the existing texture instead of
/// triggering a new GPU allocation on every transition. Changing the side resets the
/// high-water mark to the newly needed layer count.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct NvfDirectionalShadowLayerHighWater {
    side: u32,
    layers: usize,
}

impl NvfDirectionalShadowLayerHighWater {
    /// Returns the layer count to request for a directional shadow texture of dimension
    /// `side x side` needing `needed` layers, capped by `max_layers` (at least 1).
    pub fn layers(&mut self, side: u32, needed: usize, max_layers: usize) -> usize {
        let cap = max_layers.max(1);
        let needed = needed.min(cap).max(1);
        if self.side != side {
            self.side = side;
            self.layers = needed;
        } else {
            self.layers = self.layers.max(needed).min(cap).max(1);
        }
        self.layers
    }

    /// Clears the remembered side and layer count.
    pub fn reset(&mut self) {
        self.side = 0;
        self.layers = 0;
    }
}

/// NVForever held light shadow maps (2026-11-18, engine/vendor/README.md): point and spot
/// lights' shadow maps drawn less often, as [`HoldDirectionalShadowMaps`] does for the
/// sun's cascades. With this resource:
/// - each shadowed point light named in `slots` draws its cube into the layers of that slot
///   (slot × 6 + face) instead of those of its index among the shadowed lights, so it keeps
///   its layers while other lights start or stop casting, and while no camera sees it; the
///   shader finds the cube through bits 16..23 of the light's flags (slot + 1;
///   shadow_sampling.wesl `nvf_point_shadow_cube`). Any other shadowed point light takes the
///   lowest slot left that frame. The cube texture has room for at least `point_capacity`
///   lights.
/// - the faces named in `held` (bit `i` for cube face `i`; a spot light's map is bit 0) are
///   not drawn this frame when their layer still holds what that light drew into the same
///   texture: their views are left out of the frame's root views (no culling, preprocessing
///   or pass). A face whose layer last held another light, or another texture, is drawn
///   whatever `held` says.
///
/// - a point light with a slot whose shadow maps are switched off (`shadow_maps_enabled`
///   false: no views, no culling, nothing queued) still casts from its cube once all six
///   faces hold what it drew into this texture: the game switches a light's maps on only
///   on the frames it draws them, and holds them, view-less, in between.
/// - spot lights the same way: each shadowed spot light named in `spot_slots` draws its map
///   into layer `cascades + slot` of the directional shadow texture (past the sun's
///   cascades) instead of `cascades + its index among the spot lights`; any other shadowed
///   spot light takes the lowest slot left that frame; the texture has room past the
///   cascades for at least `spot_capacity` maps. The shader finds the layer through bits
///   16..23 of the light's flags (layer + 1; shadow_sampling.wesl `nvf_spot_shadow_layer`).
///   A spot light with a slot whose maps are switched off still casts from its layer once
///   that layer holds what it drew into this texture.
/// - `fade` (absent: 1) is the weight of a light's shadow, the game's fade in and out
///   (0..1) times its strength, up to [`NVF_SHADOW_WEIGHT_MAX`]: the shader takes
///   `1 - weight × (1 - shadow)` (shadows.wesl `nvf_weigh_shadow`), which past a weight of 1
///   is under 0 where the map shades in full (the game's display sum then takes the rest
///   from the other light there; Bevy's own sum holds it to 0). It is carried in bits
///   24..31 of the light's flags ([`nvf_fade_bits`]); a light without one casts as
///   upstream's.
///
/// A shadowed light's views are brought up to date when its slot or the texture changed,
/// not only when the light changed. Bevy inserts none; without it nothing changes.
///
/// NVForever static shadow cache (2026-11-18, engine/vendor/README.md "static shadow
/// cache"): with `static_cache`, a second cube texture of the same layers is kept (the cache;
/// the sampled cube texture gets `COPY_DST`). Of a point light's faces drawn this frame (not
/// held), those in `to_static` are drawn into the cache's layer (cleared), and that layer is
/// then copied into the sampled layer; those in `on_static` get the cache's layer copied into
/// the sampled layer first and are then drawn over it, the depth loaded, not cleared. Which
/// casters each face's list holds is the game's to choose (the static ones for `to_static`,
/// the moving ones for `on_static`). A face in `on_static` whose cache layer does not hold a
/// static map this light drew into this cache texture is drawn as any other (counted as a
/// miss). Faces in neither are drawn as before.
///
/// All of it only while `active` (the game sets it while its ALC shadows row is on): an
/// inactive hold, as no hold, leaves every point and spot light path upstream's (slots,
/// held faces, the static cache, kept phases, cached shadow views, the hidden lights' views,
/// the counts); the shaders' slot and fade bits are then 0, upstream's index and full
/// strength.
#[derive(Resource, Clone, Default, Debug)]
pub struct HoldLightShadowMaps {
    pub active: bool,
    pub held: EntityHashMap<u8>,
    pub slots: EntityHashMap<u32>,
    pub point_capacity: u32,
    pub spot_slots: EntityHashMap<u32>,
    pub spot_capacity: u32,
    pub fade: EntityHashMap<f32>,
    pub static_cache: bool,
    pub to_static: EntityHashMap<u8>,
    pub on_static: EntityHashMap<u8>,
    /// NVForever ALCS quiet static draws (2026-11-24): of the faces in `to_static`, those
    /// drawn into the cache without the copy into the sampled layer after. The game asks
    /// for it where moving casters stand in the face: the copy would show the face without
    /// them for a frame; it composites the face in the next frame instead.
    pub static_quiet: EntityHashMap<u8>,
    /// NVForever ALCS soft sizes (2026-11-24): the size of a light's soft shadows (world
    /// units), in place of `soft_shadows_enabled` and the light's `radius`. Bevy takes the
    /// soft size from the radius, which its lighting also reads (the highlight of a sphere
    /// light): a lamp given a soft size of 24 units got a wider, dimmer highlight, wrong
    /// within 24 units of it, and popped when the size was switched.
    pub soft_size: EntityHashMap<f32>,
    /// NVForever ALCS far maps (2026-11-24): stable slots of point lights whose cubes are in
    /// a second cube texture, `far_size` texels a side with room for `far_capacity` lights,
    /// kept beside the first (whose side is `PointLightShadowMap::size`) with a static cache
    /// of its own. A light is in `slots` or in `far_slots`, never both. The game keeps the
    /// lights near the camera in the first and the rest in the second, smaller one: a cube
    /// costs its side squared, and a far light's shadow covers few pixels. In the shaders a
    /// far light's slot is `NVF_FAR_CUBE` + its slot (bits 16..23 of its flags hold that
    /// + 1; `shadow_sampling.wesl` `nvf_point_far`). Empty, nothing changes.
    pub far_slots: EntityHashMap<u32>,
    pub far_capacity: u32,
    pub far_size: u32,
}

/// NVForever ALCS far maps: a point light's cube number is its slot in the first cube
/// texture, or this plus its slot in the far one ([`HoldLightShadowMaps::far_slots`]).
pub const NVF_FAR_CUBE: usize = 128;

/// NVForever ALCS far maps: whether a cube number is of the far texture.
fn nvf_far(cube: usize) -> bool {
    cube >= NVF_FAR_CUBE
}

/// NVForever ALCS far maps: a cube number's slot in its texture.
fn nvf_slot(cube: usize) -> usize {
    cube & (NVF_FAR_CUBE - 1)
}

/// NVForever static shadow cache: one layer copied between the cache texture and the
/// sampled cube texture (the same layer of each), a whole face.
#[derive(Clone)]
pub struct NvfLayerCopy {
    pub cache: Texture,
    pub sampled: Texture,
    pub layer: u32,
    pub size: u32,
}

impl NvfLayerCopy {
    /// Record the copy, from the cache into the sampled texture. Depth copies take whole
    /// subresources (the face's full size, one array layer) of one format [documented:
    /// WebGPU, `copyTextureToTexture` validation for depth-or-stencil formats].
    fn record(&self, encoder: &mut CommandEncoder) {
        fn at(texture: &Texture, layer: u32) -> TexelCopyTextureInfo<'_> {
            TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: Origin3d { x: 0, y: 0, z: layer },
                aspect: TextureAspect::All,
            }
        }
        encoder.copy_texture_to_texture(
            at(&self.cache, self.layer),
            at(&self.sampled, self.layer),
            Extent3d { width: self.size, height: self.size, depth_or_array_layers: 1 },
        );
    }
}

/// NVForever static shadow cache: on a point light face's shadow view drawn into the static
/// cache (`after`: its layer is then copied into the sampled texture) or composited from it
/// (the layer copied in first, then drawn over with `attachment` loading the depth). The
/// pass draws into `attachment` in place of the [`ShadowView`]'s.
#[derive(Component, Clone)]
pub struct NvfStaticShadowFace {
    pub attachment: DepthStencilViewAttachment,
    pub copy: NvfLayerCopy,
    pub after: bool,
    /// NVForever ALCS quiet static draws: drawn into the cache and not copied out; the
    /// sampled layer keeps its picture until the game composites the face.
    pub quiet: bool,
}

/// NVForever static shadow cache: the attachment of one layer of `texture`, kept in
/// `layers` while the texture is the same; `clear` sets it to clear on its first use each
/// frame, else it always loads.
fn nvf_static_layer_attachment(
    layers: &mut HashMap<u32, DepthStencilViewAttachment>,
    texture: &Texture,
    // NVForever ALCS far maps: kept by the cube number's layer, made of the texture's.
    key: u32,
    layer: u32,
    clear: bool,
) -> DepthStencilViewAttachment {
    layers
        .entry(key)
        .or_insert_with(|| {
            nvf_event(NVF_TEXTURE_VIEWS, 1);
            let view = texture.create_view(&TextureViewDescriptor {
                label: Some("nvf_point_light_static_shadow_view"),
                format: None,
                dimension: Some(TextureViewDimension::D2),
                usage: None,
                aspect: TextureAspect::All,
                base_mip_level: 0,
                mip_level_count: None,
                base_array_layer: layer,
                array_layer_count: Some(1u32),
            });
            DepthStencilViewAttachment::new(DepthStencilViews::DepthOnly { depth_view: view }, clear.then_some(0.0), None)
        })
        .clone()
}

/// NVForever held light shadow maps: the largest slot + 1 (point) or layer + 1 (spot) the
/// light flags' bits 16..23 carry.
const NVF_LAYER_BITS_MAX: usize = 0xff;

/// NVForever held light shadow maps: a light's shadow weight (its fade, 0..1, times its
/// strength; held to 0..[`NVF_SHADOW_WEIGHT_MAX`]) in bits 24..31 of its flags, as
/// `255 - weight × 200`: steps of half a hundredth, and never 0, which is a light without a
/// weight (1, Bevy's default; shadows.wesl `nvf_shadow_weight`).
pub fn nvf_fade_bits(fade: f32) -> u32 {
    (255 - (fade.clamp(0.0, NVF_SHADOW_WEIGHT_MAX) * 200.0).round() as u32) << 24
}

/// The largest shadow weight the light's flags carry ([`nvf_fade_bits`]).
pub const NVF_SHADOW_WEIGHT_MAX: f32 = 1.27;

impl bevy_render::extract_resource::ExtractResource<RenderApp> for HoldLightShadowMaps {
    type Source = HoldLightShadowMaps;
    fn extract_resource(source: &Self::Source) -> Self {
        source.clone()
    }
}

/// NVForever light view counts (2026-11-18, engine/vendor/README.md): what the point and
/// spot lights' shadow views and `extract_lights` / `prepare_lights` cost, summed over the
/// render frames since the last [`nvf_take_light_view_counts`], for the game's lamp shadow
/// summary (nvforever light_shadows.rs). Bevy reads none of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NvfLightViewCounts {
    /// Render frames counted.
    pub frames: u64,
    /// Point and spot light shadow views alive (a point light's six faces, a spot light's
    /// one), summed over the frames.
    pub views: u64,
    /// Those of them that ran the Core3d schedule (a `RootNonCameraView`; a held face has
    /// none), summed over the frames.
    pub run: u64,
    /// Views despawned with their light because it was hidden (bevyengine/bevy#26059: before
    /// the fix they stayed alive and ran every frame).
    pub hidden: u64,
    /// Wall time of each of [`NVF_SECTIONS`], nanoseconds: summed over the frames, and the
    /// longest in one frame.
    pub ns: [u64; NVF_SECTION_COUNT],
    pub max_ns: [u64; NVF_SECTION_COUNT],
    /// Each of [`NVF_EVENTS`], counted over the frames.
    pub events: [u64; NVF_EVENT_COUNT],
}

/// NVForever light view counts: the parts of `extract_lights` and `prepare_lights` timed, in
/// the order of [`NvfLightViewCounts::ns`]. `prepare_lights` is the whole system; the parts
/// after it are its consecutive stretches (they add up to it).
pub const NVF_SECTIONS: [&str; NVF_SECTION_COUNT] = [
    "extract_lights",
    "prepare_lights",
    "uniform writer",
    "counts and slots",
    "textures",
    "gpu lights",
    "point views",
    "spot views",
    "cameras",
    "sweep",
];
pub const NVF_SECTION_COUNT: usize = 10;
const NVF_EXTRACT: usize = 0;
const NVF_PREPARE: usize = 1;
const NVF_WRITER: usize = 2;
const NVF_SLOTS: usize = 3;
const NVF_TEXTURES: usize = 4;
const NVF_GPU_LIGHTS: usize = 5;
const NVF_POINT_VIEWS: usize = 6;
const NVF_SPOT_VIEWS: usize = 7;
const NVF_CAMERAS: usize = 8;
const NVF_SWEEP: usize = 9;

/// NVForever light view counts: what `prepare_lights` did, in the order of
/// [`NvfLightViewCounts::events`]: point and spot shadow views spawned, views despawned
/// because their light's maps were switched off (or had no room), a light's existing views
/// made again (its light, slot or texture changed), shadow phases made anew and dropped, and
/// the point and directional shadow textures that were not last frame's.
pub const NVF_EVENTS: [&str; NVF_EVENT_COUNT] = [
    "views spawned",
    "views despawned",
    "views remade",
    "phases new",
    "phases dropped",
    "new point textures",
    "new directional textures",
    "texture views made",
    "static cache draws",
    "static cache composites",
    "static cache misses",
    "phases reused",
    "kept phases wiped",
    "views kept",
    "kept phases resynced",
    "entities rebinned",
    "stale entries requeued",
];
pub const NVF_EVENT_COUNT: usize = 17;
const NVF_SPAWNED: usize = 0;
const NVF_DESPAWNED: usize = 1;
const NVF_REMADE: usize = 2;
const NVF_PHASES_NEW: usize = 3;
const NVF_PHASES_DROPPED: usize = 4;
const NVF_NEW_POINT_TEXTURE: usize = 5;
const NVF_NEW_DIRECTIONAL_TEXTURE: usize = 6;
const NVF_TEXTURE_VIEWS: usize = 7;
// NVForever static shadow cache: faces drawn into the cache, composited from it, and
// composites whose cache layer held no map of the light.
const NVF_STATIC_DRAWS: usize = 8;
const NVF_STATIC_COMPOSITES: usize = 9;
const NVF_STATIC_MISSES: usize = 10;
// NVForever kept shadow phases: the phases a light's new views found kept (each saved its
// view a full specialization and queueing), the kept phases dropped because something in them
// changed while they had no view, and views not made again because what they are made from
// is the same (NVForever kept shadow views).
const NVF_PHASES_REUSED: usize = 11;
const NVF_PARKED_WIPED: usize = 12;
const NVF_VIEWS_KEPT: usize = 13;
// NVForever kept shadow phases: kept views back after a frame without a view, their phases
// resynced with their light's visible list (`queue_shadows`), and entities queued into a bin
// they were already in, taken out first (none expected; each was a panic before).
const NVF_RESYNCED: usize = 14;
const NVF_REBINNED: usize = 15;
// NVForever kept shadow phases: entities a resumed view's phase still held with an input
// uniform index that is no longer theirs (their mesh instance was dropped and made again while
// the view was away), taken out and queued again.
const NVF_STALE_BINNED: usize = 16;

/// NVForever kept shadow phases: off with `NVF_LIGHT_SHADOW_KEEP_PHASES=0` (a measuring aid
/// of the game's: a light's phases, pipeline keys and specialized pipelines go with its views,
/// as upstream).
pub fn nvf_keep_shadow_phases_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let off = std::env::var("NVF_LIGHT_SHADOW_KEEP_PHASES").is_ok_and(|v| v.trim() == "0");
        if off {
            bevy_log::info!("held light shadow maps: NVF_LIGHT_SHADOW_KEEP_PHASES=0, a held light's shadow phases go with its views");
        }
        !off
    })
}

/// NVForever kept shadow phases (render world, engine/vendor/README.md): the shadow views
/// (retained view entities) of the lights the game holds a slot for (`HoldLightShadowMaps`
/// `slots`, `spot_slots`), filled by `prepare_lights` each frame. Their shadow phases,
/// pipeline keys ([`LightKeyCache`]), specialized pipelines and pending queues are kept while
/// the light has no views (its maps held, or the light out of view); one whose specialized
/// pipelines name an entity that changed while it had no view is dropped (`specialize_shadows`),
/// so that light is specialized and queued in full when it draws again, as upstream.
///
/// A kept view that has a view again this frame after none last frame is `resumed`
/// (`specialize_shadows` fills it each frame): its light's visible list kept changing while it
/// had no view, and the added and removed entities of those frames were never queued nor taken
/// out, so its phase is resynced with the whole visible list instead ([`nvf_resync_kept_phase`]).
#[derive(Resource, Default, Debug)]
pub struct NvfParkedShadowViews {
    pub views: HashSet<RetainedViewEntity>,
    pub resumed: HashSet<RetainedViewEntity>,
    /// Set by `prepare_lights` each frame: the game's hold is active and phases are kept
    /// (`nvf_keep_shadow_phases_on`). Off, `specialize_shadows` and `queue_shadows` do as
    /// upstream (no resumed views, no kept views, binning as upstream's `add`).
    pub active: bool,
}

/// NVForever kept shadow phases: the stale-entry check of [`nvf_resync_kept_phase`], off with
/// `NVF_LIGHT_SHADOW_STALE_CHECK=0` (a measuring aid of the game's: the slabs come back).
pub fn nvf_stale_check_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("NVF_LIGHT_SHADOW_STALE_CHECK").is_ok_and(|v| v.trim() == "0"))
}

/// NVForever kept shadow phases: whether the main entity is in the visible list (by main
/// entity alone; the CPU-culled list is sorted by it).
pub fn nvf_entity_visible(visible: &RenderVisibleEntitiesClass, main_entity: MainEntity) -> bool {
    visible
        .entities_cpu_culling
        .binary_search_by_key(&main_entity, |(_, main)| *main)
        .is_ok()
        || visible.entities_gpu_culling.contains_key(&main_entity)
}

/// NVForever kept shadow phases: resyncs a kept phase whose view is back with its light's
/// visible list. The binned entities no longer visible leave their bins, and so do the stale
/// ones: a mesh nothing saw while the view was away lost its mesh instance, and seen again it
/// has another input uniform index (`current_index`) than the one its bin still draws with,
/// which by then is some other mesh's transform (the slabs and shards of shadow in the Primm
/// casino, 2026-11-25). The visible ones not binned (the stale ones among them) are pushed to
/// `to_queue`. Returns how many left for not being visible, and how many for being stale.
pub fn nvf_resync_kept_phase<BPI: BinnedPhaseItem>(
    phase: &mut BinnedRenderPhase<BPI>,
    visible: &RenderVisibleEntitiesClass,
    current_index: impl Fn(MainEntity) -> Option<InputUniformIndex>,
    to_queue: &mut Vec<(Entity, MainEntity)>,
) -> (usize, usize) {
    let mut stale = 0;
    let check = nvf_stale_check_on();
    let gone = phase.nvf_retain_binned(|main_entity, binned_with| {
        if !nvf_entity_visible(visible, main_entity) {
            return false;
        }
        let good = !check || current_index(main_entity) == Some(binned_with);
        stale += usize::from(!good);
        good
    });
    let dropped = gone - stale;
    to_queue.extend(
        visible
            .iter_visible()
            .filter(|(_, main_entity)| !phase.nvf_is_binned(**main_entity))
            .map(|(entity, main_entity)| (*entity, *main_entity)),
    );
    (dropped, stale)
}

/// NVForever kept shadow phases: bins an entity as [`BinnedRenderPhase::add`] does, but with
/// `guard` one already binned is taken out of its bin first instead of the add's panic
/// ("Binning main entity … when it was already binned"). Returns whether it was binned
/// already (always false without `guard`: upstream's add).
pub fn nvf_add_once<BPI: BinnedPhaseItem>(
    guard: bool,
    phase: &mut BinnedRenderPhase<BPI>,
    batch_set_key: BPI::BatchSetKey,
    bin_key: BPI::BinKey,
    entities: (Entity, MainEntity),
    input_uniform_index: InputUniformIndex,
    phase_type: BinnedRenderPhaseType,
) -> bool {
    let binned = guard && phase.nvf_is_binned(entities.1);
    if binned {
        phase.remove(entities.1);
    }
    phase.add(batch_set_key, bin_key, entities, input_uniform_index, phase_type);
    binned
}

/// NVForever kept shadow views: what a point or spot light's shadow views are made from (its
/// place, for a spot light its turn and cone, the near plane, the range its views' frusta end
/// at, the layers), as float bits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NvfViewSource([u32; 15], RenderLayers);

fn nvf_view_source(light: &ExtractedPointLight, layers: &RenderLayers) -> NvfViewSource {
    let mut bits = [0u32; 15];
    let near = light.shadow_map_near_z.to_bits();
    bits[14] = light.range.to_bits();
    match light.spot_light_angles {
        // A spot light's view follows its whole transform and its outer angle.
        Some((_, outer)) => {
            let a = light.transform.affine();
            for (i, v) in a.matrix3.to_cols_array().iter().chain(a.translation.to_array().iter()).enumerate() {
                bits[i] = v.to_bits();
            }
            bits[12] = outer.to_bits();
            bits[13] = near;
        }
        // A point light's faces follow its place only (the cube is axis-aligned).
        None => {
            for (i, v) in light.transform.translation().to_array().iter().enumerate() {
                bits[i] = v.to_bits();
            }
            bits[3] = near;
        }
    }
    NvfViewSource(bits, layers.clone())
}

/// NVForever cached shadow views: off with `NVF_SHADOW_VIEW_CACHE=0` (a measuring aid of
/// the game's: every view made anew each frame, as upstream).
fn nvf_shadow_view_cache_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let off = std::env::var("NVF_SHADOW_VIEW_CACHE").is_ok_and(|v| v.trim() == "0");
        if off {
            bevy_log::info!("held light shadow maps: NVF_SHADOW_VIEW_CACHE=0, shadow texture views made anew every frame");
        }
        !off
    })
}

/// NVForever cached shadow views (engine/vendor/README.md): the shadow textures' views kept
/// across frames by `prepare_lights` while the texture is the same.
#[derive(Default)]
pub struct NvfShadowViewCache {
    /// The point texture and (NVForever ALCS far maps) the far one.
    point_texture: Option<(TextureId, Option<TextureId>)>,
    directional_texture: Option<TextureId>,
    point_array: Option<TextureView>,
    /// NVForever ALCS far maps: the far texture's cube array view.
    far_array: Option<TextureView>,
    directional_array: Option<TextureView>,
    /// A point light face's attachment, by layer of the cube texture.
    point_layers: HashMap<u32, DepthStencilViewAttachment>,
    /// A spot light's attachment, by layer of the directional texture.
    spot_layers: HashMap<u32, DepthStencilViewAttachment>,
    /// A cascade's layer view, by layer of the directional texture.
    cascade_views: HashMap<u32, TextureView>,
    /// NVForever static shadow cache: the cache texture, its layers' attachments (clearing),
    /// and the sampled point texture's layers' loading attachments.
    static_texture: (Option<TextureId>, Option<TextureId>),
    static_layers: HashMap<u32, DepthStencilViewAttachment>,
    load_layers: HashMap<u32, DepthStencilViewAttachment>,
}

type NvfAtomic = core::sync::atomic::AtomicU64;
static NVF_LIGHT_VIEW_FRAMES: NvfAtomic = NvfAtomic::new(0);
static NVF_LIGHT_VIEWS: NvfAtomic = NvfAtomic::new(0);
static NVF_LIGHT_VIEWS_RUN: NvfAtomic = NvfAtomic::new(0);
static NVF_LIGHT_VIEWS_HIDDEN: NvfAtomic = NvfAtomic::new(0);
static NVF_NS: [NvfAtomic; NVF_SECTION_COUNT] = [const { NvfAtomic::new(0) }; NVF_SECTION_COUNT];
static NVF_MAX_NS: [NvfAtomic; NVF_SECTION_COUNT] = [const { NvfAtomic::new(0) }; NVF_SECTION_COUNT];
static NVF_EVENT: [NvfAtomic; NVF_EVENT_COUNT] = [const { NvfAtomic::new(0) }; NVF_EVENT_COUNT];

/// NVForever light view counts: the counts since the last call, and start counting anew.
pub fn nvf_take_light_view_counts() -> NvfLightViewCounts {
    use core::sync::atomic::Ordering::Relaxed;
    NvfLightViewCounts {
        frames: NVF_LIGHT_VIEW_FRAMES.swap(0, Relaxed),
        views: NVF_LIGHT_VIEWS.swap(0, Relaxed),
        run: NVF_LIGHT_VIEWS_RUN.swap(0, Relaxed),
        hidden: NVF_LIGHT_VIEWS_HIDDEN.swap(0, Relaxed),
        ns: core::array::from_fn(|i| NVF_NS[i].swap(0, Relaxed)),
        max_ns: core::array::from_fn(|i| NVF_MAX_NS[i].swap(0, Relaxed)),
        events: core::array::from_fn(|i| NVF_EVENT[i].swap(0, Relaxed)),
    }
}

/// NVForever light view counts: on while the game's hold is active (set by `prepare_lights`
/// each frame); off, nothing is counted or timed.
static NVF_COUNTING: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn nvf_counting() -> bool {
    NVF_COUNTING.load(core::sync::atomic::Ordering::Relaxed)
}

/// NVForever light view counts: `n` more of event `event`.
fn nvf_event(event: usize, n: u64) {
    if n > 0 && nvf_counting() {
        NVF_EVENT[event].fetch_add(n, core::sync::atomic::Ordering::Relaxed);
    }
}

/// NVForever light view counts: a stopwatch over consecutive sections (none while nothing
/// is counted).
struct NvfLap(Option<std::time::Instant>);

impl NvfLap {
    fn start() -> Self {
        Self(nvf_counting().then(std::time::Instant::now))
    }

    /// Add the time since the last lap (or the start) to `section`.
    fn lap(&mut self, section: usize) {
        if let Some(last) = self.0 {
            let now = std::time::Instant::now();
            nvf_time(section, now - last);
            self.0 = Some(now);
        }
    }
}

/// NVForever light view counts: `elapsed` more of `section`.
fn nvf_time(section: usize, elapsed: core::time::Duration) {
    use core::sync::atomic::Ordering::Relaxed;
    let ns = elapsed.as_nanos().min(u64::MAX as u128) as u64;
    NVF_NS[section].fetch_add(ns, Relaxed);
    NVF_MAX_NS[section].fetch_max(ns, Relaxed);
}

/// NVForever light view counts: the views of a light being hidden (extraction).
fn nvf_count_hidden_views(views: &Query<&PointAndSpotLightViewEntities>, light: Entity) {
    if let Ok(v) = views.get(light) {
        NVF_LIGHT_VIEWS_HIDDEN.fetch_add(v.len() as u64, core::sync::atomic::Ordering::Relaxed);
    }
}

/// NVForever light view counts: count this frame's point and spot light shadow views, and
/// those that run (render world, once the views of the frame are made: `RenderSystems::Prepare`).
pub fn nvf_count_light_views(views: Query<(&LightEntity, Has<RootNonCameraView>)>) {
    use core::sync::atomic::Ordering::Relaxed;
    if !nvf_counting() {
        return;
    }
    let (mut alive, mut run) = (0u64, 0u64);
    for (light, root) in &views {
        if matches!(light, LightEntity::Point { .. } | LightEntity::Spot { .. }) {
            alive += 1;
            run += u64::from(root);
        }
    }
    NVF_LIGHT_VIEW_FRAMES.fetch_add(1, Relaxed);
    NVF_LIGHT_VIEWS.fetch_add(alive, Relaxed);
    NVF_LIGHT_VIEWS_RUN.fetch_add(run, Relaxed);
}

/// NVForever held light shadow maps: what each layer was last drawn with (the light and the
/// texture), by layer of the point cube texture and of the directional texture (spot lights).
#[derive(Default)]
pub struct NvfLightShadowRecord {
    point: HashMap<u32, (MainEntity, TextureId)>,
    spot: HashMap<u32, (MainEntity, TextureId)>,
    /// The layer (cube or spot layer) and texture each light's views were made for.
    views: EntityHashMap<(usize, TextureId)>,
    /// NVForever static shadow cache: what each layer of the cache texture was last drawn
    /// with, and the faces of each light (render entity) whose views carry an
    /// [`NvfStaticShadowFace`].
    static_point: HashMap<u32, (MainEntity, TextureId)>,
    static_faces: EntityHashMap<u8>,
    /// NVForever kept shadow views: what each light's (render entity) views were last made
    /// from.
    made_from: EntityHashMap<NvfViewSource>,
}

/// NVForever hosted shadow views (2026-11-25, engine/vendor/README.md "hosted shadow
/// views"). Upstream makes every point and spot light shadow view a root view: the camera
/// driver runs the whole `Core3d` schedule for each (six for a point light's cube), and each
/// run records four compute passes (uniform allocation, bin unpacking, mesh preprocessing,
/// the indirect parameters) and the shadow pass, in five command buffers. With the game's
/// lamps drawing 6 to 8 faces a frame that was most of what their shadows cost the
/// processor: the render thread took 8.6 ms a frame against 4.6 ms without them, with
/// 0.4 ms of GPU time in those views (a casino hall of 25 lamps, measured).
///
/// While the game's hold is active, those views are drawn the way a camera's cascades
/// are: the first camera that runs `Core3d` with light views (`host`) takes them into its
/// own uniform allocation, bin unpacking and mesh preprocessing passes (gpu_preprocess.rs
/// `gather_shadow_cascades_for_view`), and draws them after its cascades
/// ([`per_view_shadow_pass`], which builds each one's indirect parameters first, as it
/// does a cascade's). They carry `NvfHostedView`, so the camera driver runs nothing for
/// them. Every camera drawn after the host samples this frame's maps, as before; with no
/// such camera (none, or its window minimized) the views stay root views. Off with `NVF_LIGHT_SHADOW_HOSTED=0` (a
/// measuring aid).
#[derive(Resource, Default, Debug)]
pub struct NvfSharedShadowViews {
    pub host: Option<Entity>,
    pub views: Vec<Entity>,
}

/// NVForever hosted shadow views: how many of them share a command encoder.
const NVF_HOSTED_VIEWS_PER_ENCODER: usize = 2;

fn nvf_hosted_shadow_views_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        let off = std::env::var("NVF_LIGHT_SHADOW_HOSTED").is_ok_and(|v| v.trim() == "0");
        if off {
            bevy_log::info!("held light shadow maps: NVF_LIGHT_SHADOW_HOSTED=0, every light's shadow view runs the camera's schedule by itself");
        }
        !off
    })
}

/// NVForever hosted shadow views: this frame's host and the views it draws
/// ([`NvfSharedShadowViews`]); render world, once the frame's views are made.
pub fn nvf_host_shadow_views(
    mut commands: Commands,
    hold: Option<Res<HoldLightShadowMaps>>,
    sorted_cameras: Res<SortedCameras>,
    cameras: Query<(&ExtractedCamera, Has<ViewLightEntities>, Has<SkipGpuPreprocess>)>,
    windows: Query<(MainEntity, &bevy_render::view::ExtractedWindow)>,
    views: Query<(Entity, &LightEntity, Has<NvfHostedView>), With<RootNonCameraView>>,
    stale: Query<Entity, (With<NvfHostedView>, Without<RootNonCameraView>)>,
    mut shared: ResMut<NvfSharedShadowViews>,
) {
    shared.views.clear();
    let on = hold.is_some_and(|hold| hold.active) && nvf_hosted_shadow_views_on();
    let core_3d = Core3d.intern();
    let host = sorted_cameras
        .0
        .iter()
        .map(|camera| camera.entity)
        .filter(|_| on)
        .find(|entity| {
            cameras.get(*entity).is_ok_and(|(camera, lights, skip)| {
                // A camera whose window has no size (minimized) is not drawn: the driver
                // skips its schedule, and with it what it hosts.
                let drawn = match &camera.target {
                    Some(bevy_camera::NormalizedRenderTarget::Window(window)) => windows
                        .iter()
                        .find(|(main_entity, _)| *main_entity == window.entity())
                        .is_some_and(|(_, w)| w.physical_width > 0 && w.physical_height > 0),
                    _ => true,
                };
                lights && !skip && drawn && camera.schedule == core_3d
            })
        });
    for (entity, light, hosted) in &views {
        let take = host.is_some() && matches!(light, LightEntity::Point { .. } | LightEntity::Spot { .. });
        if take {
            shared.views.push(entity);
        }
        if take && !hosted {
            commands.entity(entity).insert(NvfHostedView);
        } else if !take && hosted {
            commands.entity(entity).remove::<NvfHostedView>();
        }
    }
    for entity in &stale {
        commands.entity(entity).remove::<NvfHostedView>();
    }
    shared.host = host.filter(|_| !shared.views.is_empty());
}

pub fn per_view_shadow_pass<const IS_LATE: bool>(
    world: &World,
    view: ViewQuery<&ViewLightEntities>,
    // NVForever held shadow maps.
    hold_directional: Option<Res<HoldDirectionalShadowMaps>>,
    // NVForever hosted shadow views.
    nvf_shared: Option<Res<NvfSharedShadowViews>>,
    light_entities: Query<&LightEntity>,
    view_light_query: Query<(
        &ShadowView,
        &ExtractedView,
        Has<OcclusionCulling>,
        Has<PreprocessBindGroups>,
        Has<SkipGpuPreprocess>,
        Has<NoIndirectDrawing>,
        // NVForever static shadow cache (the hosted views').
        Option<&NvfStaticShadowFace>,
    )>,
    shadow_render_phases: Res<ViewBinnedRenderPhases<Shadow>>,
    preprocess_pipelines: Option<Res<PreprocessPipelines>>,
    build_indirect_params_bind_groups: Option<Res<BuildIndirectParametersBindGroups>>,
    pipeline_cache: Res<PipelineCache>,
    indirect_parameters_buffers: Option<Res<IndirectParametersBuffers>>,
    build_indirect_parameters_metadata: Option<Res<BuildIndirectParametersMetadata>>,
    mut ctx: RenderContext,
) {
    let nvf_view_entity = view.entity();
    let view_lights = view.into_inner();

    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "per_view_shadow_pass");

    let hold_directional = hold_directional.map_or(HoldDirectionalShadowMaps(0), |hold| *hold);
    for view_light_entity in view_lights.lights.iter().copied() {
        // NVForever held shadow maps: a held cascade's map keeps an earlier frame's picture.
        if let Ok(LightEntity::Directional { cascade_index, .. }) = light_entities.get(view_light_entity)
            && hold_directional.holds(*cascade_index)
        {
            continue;
        }
        let Ok((
            view_light,
            extracted_light_view,
            occlusion_culling,
            has_preprocess_bind_groups,
            has_skip_gpu_preprocess,
            has_no_indirect_drawing,
            _,
        )) = view_light_query.get(view_light_entity)
        else {
            continue;
        };

        // Make sure to build indirect parameters here, as the normal
        // `*_prepass_build_indirect_parameters` functions only run for root
        // views, and this shadow view isn't a root view.
        if has_preprocess_bind_groups
            && !has_skip_gpu_preprocess
            && !has_no_indirect_drawing
            && let (Some(preprocess_pipelines), Some(build_indirect_parameters_metadata)) =
                (&preprocess_pipelines, &build_indirect_parameters_metadata)
        {
            run_build_indirect_parameters(
                &mut ctx,
                extracted_light_view.retained_view_entity,
                build_indirect_params_bind_groups.as_deref(),
                &pipeline_cache,
                indirect_parameters_buffers.as_deref(),
                build_indirect_parameters_metadata,
                if IS_LATE {
                    &preprocess_pipelines.late_phase
                } else {
                    &preprocess_pipelines.early_phase
                },
                if IS_LATE {
                    "late_view_shadow_indirect_parameters_building"
                } else {
                    "early_view_shadow_indirect_parameters_building"
                },
            );
        }

        // Draw the shadow map.
        view_shadow_pass::<IS_LATE>(
            view_light_entity,
            view_light,
            extracted_light_view,
            occlusion_culling,
            world,
            &shadow_render_phases,
            &mut ctx,
            // NVForever static shadow cache: the cascades have none.
            None,
        );
    }

    // NVForever hosted shadow views: the point and spot lights' views this camera hosts,
    // each as a cascade above (its indirect parameters, then its map). Not in the late
    // pass: they have no occlusion culling.
    if !IS_LATE
        && let Some(shared) = nvf_shared
            .as_deref()
            .filter(|shared| shared.host == Some(nvf_view_entity))
    {
        let hosted_span = diagnostics.time_span(ctx.command_encoder(), "hosted_shadow_views");
        for (nvf_index, view_light_entity) in shared.views.iter().copied().enumerate() {
            // Two views to a command encoder: finishing one is most of what a view costs
            // the processor, and a system's encoders are finished side by side
            // (bevy_render render_context.rs `nvf_split_encoder`).
            if nvf_index % NVF_HOSTED_VIEWS_PER_ENCODER == 0 {
                ctx.nvf_split_encoder();
            }
            let Ok((
                view_light,
                extracted_light_view,
                occlusion_culling,
                has_preprocess_bind_groups,
                has_skip_gpu_preprocess,
                has_no_indirect_drawing,
                nvf_static_face,
            )) = view_light_query.get(view_light_entity)
            else {
                continue;
            };
            if has_preprocess_bind_groups
                && !has_skip_gpu_preprocess
                && !has_no_indirect_drawing
                && let (Some(preprocess_pipelines), Some(build_indirect_parameters_metadata)) =
                    (&preprocess_pipelines, &build_indirect_parameters_metadata)
            {
                run_build_indirect_parameters(
                    &mut ctx,
                    extracted_light_view.retained_view_entity,
                    build_indirect_params_bind_groups.as_deref(),
                    &pipeline_cache,
                    indirect_parameters_buffers.as_deref(),
                    build_indirect_parameters_metadata,
                    &preprocess_pipelines.early_phase,
                    "early_hosted_shadow_indirect_parameters_building",
                );
            }
            view_shadow_pass::<IS_LATE>(
                view_light_entity,
                view_light,
                extracted_light_view,
                occlusion_culling,
                world,
                &shadow_render_phases,
                &mut ctx,
                nvf_static_face,
            );
        }
        hosted_span.end(ctx.command_encoder());
    }

    time_span.end(ctx.command_encoder());
}

/// A common helper function to render a shadow map.
#[allow(clippy::too_many_arguments)]
fn view_shadow_pass<const IS_LATE: bool>(
    view_light_entity: Entity,
    view_light: &ShadowView,
    extracted_light_view: &ExtractedView,
    occlusion_culling: bool,
    world: &World,
    shadow_render_phases: &ViewBinnedRenderPhases<Shadow>,
    ctx: &mut RenderContext,
    // NVForever static shadow cache.
    nvf_static_face: Option<&NvfStaticShadowFace>,
) {
    if IS_LATE && !occlusion_culling {
        return;
    }

    let Some(shadow_phase) = shadow_render_phases.get(&extracted_light_view.retained_view_entity)
    else {
        return;
    };

    #[cfg(feature = "trace")]
    let _shadow_pass_span = info_span!("", "{}", view_light.pass_name).entered();

    // NVForever static shadow cache: a composited face gets the cache's layer copied in before
    // its first pass (the early one), and is then drawn over it (its attachment loads).
    if let Some(face) = nvf_static_face.filter(|f| !IS_LATE && !f.after) {
        face.copy.record(ctx.command_encoder());
    }
    let depth_attachment = nvf_static_face.map_or(&view_light.depth_attachment, |f| &f.attachment);
    let depth_stencil_attachment = Some(depth_attachment.get_attachment(StoreOp::Store));

    let mut render_pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some(&view_light.pass_name),
        color_attachments: &[],
        depth_stencil_attachment,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });

    if let Err(err) = shadow_phase.render(&mut render_pass, world, view_light_entity) {
        error!("Error encountered while rendering the shadow phase {err:?}");
    }

    // NVForever static shadow cache: a face drawn into the cache has its layer copied into
    // the sampled texture once drawn, after its last pass (the late one only with occlusion
    // culling, which the light views do not have).
    if let Some(face) = nvf_static_face.filter(|f| f.after && !f.quiet && (IS_LATE || !occlusion_culling)) {
        drop(render_pass);
        face.copy.record(ctx.command_encoder());
    }
}

/// Creates the [`ClusterableObjectType`] data for a point or spot light.
fn point_or_spot_light_to_clusterable(point_light: &ExtractedPointLight) -> ClusterableObjectType {
    match point_light.spot_light_angles {
        Some((_, outer_angle)) => ClusterableObjectType::SpotLight {
            outer_angle,
            shadow_maps_enabled: point_light.shadow_maps_enabled,
            volumetric: point_light.volumetric,
        },
        None => ClusterableObjectType::PointLight {
            shadow_maps_enabled: point_light.shadow_maps_enabled,
            volumetric: point_light.volumetric,
        },
    }
}

/// Returns the [`RenderShadowMapVisibleEntities`] table corresponding to the
/// given [`LightEntity`].
fn get_shadow_map_visible_entities<'w, 's: 'w>(
    shadow_map_visible_entities_query: &'w Query<'w, 's, &'_ RenderShadowMapVisibleEntities>,
    light_entity: &'_ LightEntity,
    extracted_view_light: &'_ ExtractedView,
) -> &'w RenderVisibleEntities {
    match light_entity {
        LightEntity::Directional { light_entity, .. } => {
            let retained_view_entity = extracted_view_light.retained_view_entity;
            &shadow_map_visible_entities_query
                .get(*light_entity)
                .expect("Failed to get directional light visible entities")
                .subviews
                .get(&retained_view_entity)
                .expect("Failed to get directional light visible entities for cascade")
                .1
        }
        LightEntity::Point {
            light_entity,
            face_index,
        } => {
            // We replace the auxiliary entity with `None`
            // because all cubemap views for a single point light
            // currently share the same set of visible entities.
            let retained_view_entity = RetainedViewEntity {
                main_entity: extracted_view_light.retained_view_entity.main_entity,
                auxiliary_entity: None,
                subview_index: *face_index as u32,
            };
            &shadow_map_visible_entities_query
                .get(*light_entity)
                .expect("Failed to get point light visible entities")
                .subviews
                .get(&retained_view_entity)
                .expect("Failed to get point light visible entity for face")
                .1
        }
        LightEntity::Spot { light_entity } => {
            // We replace the auxiliary entity with `None`
            // because all shadow maps for a single spot light
            // currently share the same set of visible entities.
            let retained_view_entity = RetainedViewEntity {
                main_entity: extracted_view_light.retained_view_entity.main_entity,
                auxiliary_entity: None,
                subview_index: 0,
            };
            &shadow_map_visible_entities_query
                .get(*light_entity)
                .expect("Failed to get spot light visible entities")
                .subviews
                .get(&retained_view_entity)
                .expect("Failed to get spot light visible entity for view")
                .1
        }
    }
}

/// An extraction system that determines the origin for LOD computation for
/// point and spot light shadow maps and updates the [`RenderShadowLodOrigin`]
/// with the result.
///
/// See [`ShadowLodOrigin`] for more details on the algorithm that this system
/// uses.
pub fn extract_shadow_lod_origin(
    global_transform_query: Extract<Query<&GlobalTransform>>,
    mut camera_query: Extract<Query<(Entity, &RenderTarget), With<Camera>>>,
    mut shadow_lod_origin_query: Extract<Query<Entity, With<ShadowLodOrigin>>>,
    mut lights_query: Extract<Query<Entity, Or<(With<PointLight>, With<SpotLight>)>>>,
    mut render_shadow_lod_origin: ResMut<RenderShadowLodOrigin>,
) {
    match bevy_light::get_shadow_lod_origin(
        camera_query.transmute_lens_filtered(),
        shadow_lod_origin_query.transmute_lens_filtered(),
        lights_query.transmute_lens_filtered(),
    )
    .and_then(|shadow_lod_origin_entity| global_transform_query.get(shadow_lod_origin_entity).ok())
    {
        Some(global_transform) => {
            render_shadow_lod_origin.0 = global_transform.translation();
        }
        None => render_shadow_lod_origin.0 = Default::default(),
    }
}
