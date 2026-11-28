//! The bounce light's bindings, keys, hook and readers without a GPU: wgpu's no-op device for
//! the layouts and the view keys, Bevy's shader cache and Naga for the composed shaders. No
//! fragment is drawn and no bind group or pipeline made.

use super::hook::*;
use super::*;
use crate::lights::unix_lines;
use crate::mesh_blend_shader_tests::{compose, declaration_difference, declarations, nif_defs, sources, terrain_defs, vendored_modules};
use crate::sun_visibility::tests::{balanced, block_after, code, code_lines, resolve_known};
use bevy::core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass, NormalPrepass};
use bevy::mesh::BaseMeshPipelineKey;
use bevy::pbr::{
    alpha_mode_pipeline_key, check_prepass_views_need_specialization, check_views_lights_need_specialization, check_views_need_specialization, init_mesh_pipeline_view_layouts, LightEntity, LightKeyCache,
    MeshPipelineKey, MeshPipelineViewLayoutKey, MeshPipelineViewLayouts, NvfRtgiVolumeBindings, Shadow, ViewKeyCache, ViewKeyPrepassCache,
};
use bevy::render::batching::gpu_preprocessing::GpuPreprocessingMode;
use bevy::render::camera::DirtySpecializations;
use bevy::render::render_phase::ViewBinnedRenderPhases;
use bevy::render::render_resource::{
    BufferDescriptor, BufferUsages, Extent3d, SamplerDescriptor, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor,
};
use bevy::render::renderer::{RenderAdapter, RenderDevice, RenderQueue};
use bevy::render::sync_world::MainEntity;
use bevy::render::view::{ExtractedView, RetainedViewEntity};
use bevy::shader::ShaderDefVal;
use nvf_assets::nif_material::{reads_rtgi_volume, BlendedCutouts, NifMaterial, NifSurface, NifSurfaceKey, RTGI_VOLUME_DEF};
use nvf_assets::nif_surface_blend::HeightLayout;
use std::collections::BTreeSet;
use wgpu::naga;

fn vendored(file: &str) -> String {
    let path = format!("{}/../vendor/bevy_pbr/src/render/{file}", env!("CARGO_MANIFEST_DIR"));
    unix_lines(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")))
}

/// wgpu's no-op device, adapter and queue: real resource creation and validation, no GPU.
pub(super) fn noop() -> (RenderDevice, RenderAdapter, RenderQueue) {
    use bevy::tasks::block_on;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        flags: wgpu::InstanceFlags::default(),
        memory_budget_thresholds: default(),
        display: None,
        backend_options: wgpu::BackendOptions { noop: wgpu::NoopBackendOptions { enable: true, ..default() }, ..default() },
    });
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("the no-op adapter");
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: adapter.limits(), ..default() })).expect("the no-op device");
    (RenderDevice::from(device), RenderAdapter::new(adapter), RenderQueue::new(queue))
}

// ---------------------------------------------------------------------------------------
// The keys.

const BIT: MeshPipelineKey = MeshPipelineKey::NVF_RTGI_VOLUME;
const LAYOUT_BIT: MeshPipelineViewLayoutKey = MeshPipelineViewLayoutKey::NVF_RTGI_VOLUME;

/// The bit values, and that the packed fields still hold what is put in them, with the bit and
/// without.
#[test]
fn the_bit_is_the_last_flag_and_the_packed_fields_hold() {
    assert_eq!((BIT.bits(), LAYOUT_BIT.bits()), (1 << 30, 1 << 20));
    assert_eq!(MeshPipelineKey::LAST_FLAG, BIT);
    assert_eq!(((BIT.bits() << 1) - 1) & MeshPipelineKey::ALL_RESERVED_BITS.bits(), 0, "flags below the fields");
    assert_eq!((BIT | MeshPipelineKey::ALL_RESERVED_BITS).bits() & BaseMeshPipelineKey::all().bits(), 0, "the mesh's own bits");
    assert_eq!(MeshPipelineKey::all().bits() & BaseMeshPipelineKey::all().bits(), MeshPipelineKey::MORPH_TARGETS.bits());
    let blends = [
        MeshPipelineKey::BLEND_OPAQUE,
        MeshPipelineKey::BLEND_PREMULTIPLIED_ALPHA,
        MeshPipelineKey::BLEND_MULTIPLY,
        MeshPipelineKey::BLEND_ALPHA,
        MeshPipelineKey::BLEND_ALPHA_TO_COVERAGE,
    ];
    let tonemaps = [MeshPipelineKey::TONEMAP_METHOD_LINEAR, MeshPipelineKey::TONEMAP_METHOD_TONY_MC_MAPFACE, MeshPipelineKey::TONEMAP_METHOD_PBR_NEUTRAL];
    let filters = [MeshPipelineKey::SHADOW_FILTER_METHOD_HARDWARE_2X2, MeshPipelineKey::SHADOW_FILTER_METHOD_GAUSSIAN, MeshPipelineKey::SHADOW_FILTER_METHOD_TEMPORAL];
    let projections = [MeshPipelineKey::VIEW_PROJECTION_NONSTANDARD, MeshPipelineKey::VIEW_PROJECTION_PERSPECTIVE, MeshPipelineKey::VIEW_PROJECTION_ORTHOGRAPHIC];
    let transmission = MeshPipelineKey::SCREEN_SPACE_SPECULAR_TRANSMISSION_ULTRA;
    for samples in [1, 2, 4, 8] {
        for format in [TextureFormat::R8Unorm, TextureFormat::Rgba8UnormSrgb, TextureFormat::Bgra8UnormSrgb, TextureFormat::Rgba16Float, TextureFormat::Rg11b10Ufloat] {
            for (blend, (tonemap, (filter, projection))) in blends.into_iter().flat_map(|b| tonemaps.map(|t| (b, t))).flat_map(|(b, t)| filters.into_iter().zip(projections).map(move |p| (b, (t, p)))) {
                let without = MeshPipelineKey::from_msaa_samples(samples) | MeshPipelineKey::from_target_format(format) | blend | tonemap | filter | projection | transmission;
                for key in [without, without | BIT] {
                    assert_eq!((key.msaa_samples(), key.target_format()), (samples, format));
                    assert_eq!(key.intersection(MeshPipelineKey::BLEND_RESERVED_BITS), blend);
                    assert_eq!(key.intersection(MeshPipelineKey::TONEMAP_METHOD_RESERVED_BITS), tonemap);
                    assert_eq!(key.intersection(MeshPipelineKey::SHADOW_FILTER_METHOD_RESERVED_BITS), filter);
                    assert_eq!(key.intersection(MeshPipelineKey::VIEW_PROJECTION_RESERVED_BITS), projection);
                    assert_eq!(key.intersection(MeshPipelineKey::SCREEN_SPACE_SPECULAR_TRANSMISSION_RESERVED_BITS), transmission);
                }
                assert!(!without.contains(BIT) && (without | BIT).difference(BIT) == without);
            }
        }
    }
}

/// The flags that choose what a view's bind group holds, as the mesh pipeline's key has them.
const VIEW_FLAGS: [MeshPipelineKey; 9] = [
    MeshPipelineKey::NVF_SUN_VISIBILITY,
    MeshPipelineKey::NVF_CLOUD_VOLUME_VISIBILITY,
    MeshPipelineKey::NVF_AMBIENT_OCCLUSION,
    MeshPipelineKey::DEPTH_PREPASS,
    MeshPipelineKey::NORMAL_PREPASS,
    MeshPipelineKey::MOTION_VECTOR_PREPASS,
    MeshPipelineKey::DISTANCE_FOG,
    MeshPipelineKey::TONEMAP_IN_SHADER,
    MeshPipelineKey::ENVIRONMENT_MAP,
];

/// Every combination of [`VIEW_FLAGS`], with and without multisampling.
fn view_keys() -> impl Iterator<Item = MeshPipelineKey> {
    (0..1u32 << VIEW_FLAGS.len()).flat_map(|set| {
        let flags = VIEW_FLAGS.into_iter().enumerate().filter(|(i, _)| set >> i & 1 == 1).fold(MeshPipelineKey::NONE, |key, (_, f)| key | f);
        [1, 4].map(|samples| flags | MeshPipelineKey::from_msaa_samples(samples))
    })
}

#[test]
fn the_mesh_key_gives_the_layout_key() {
    for key in view_keys() {
        let without = MeshPipelineViewLayoutKey::from(key);
        assert!(!without.contains(LAYOUT_BIT), "{key:?}");
        assert_eq!(MeshPipelineViewLayoutKey::from(key | BIT), without | LAYOUT_BIT, "{key:?}");
    }
}

// ---------------------------------------------------------------------------------------
// The layouts.

/// bevy_pbr's own view layouts on the no-op device: 47-49 with the bit and not without, nothing
/// else changed; the labels aside.
#[test]
fn the_bit_adds_three_bindings_and_nothing_else() {
    let (device, adapter, _) = noop();
    let mut world = World::new();
    world.insert_resource(device);
    world.insert_resource(adapter);
    world.run_system_cached(init_mesh_pipeline_view_layouts).expect("the layouts");
    let layouts = world.resource::<MeshPipelineViewLayouts>();
    for key in view_keys().map(MeshPipelineViewLayoutKey::from) {
        let (off, on) = (layouts.get_view_layout(key), layouts.get_view_layout(key | LAYOUT_BIT));
        assert!(off.main_layout.entries.iter().all(|e| !(47..=49).contains(&e.binding)), "{key:?}");
        let (ours, rest): (Vec<_>, Vec<_>) = on.main_layout.entries.iter().copied().partition(|e| (47..=49).contains(&e.binding));
        assert_eq!(rest, off.main_layout.entries, "{key:?}");
        assert_eq!(ours.iter().map(|e| e.binding).collect::<Vec<_>>(), [47, 48, 49], "{key:?}");
        assert!(ours.iter().all(|e| e.visibility == wgpu::ShaderStages::FRAGMENT && e.count.is_none()));
        assert_eq!(ours[0].ty, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None });
        assert_eq!(ours[1].ty, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D3, multisampled: false });
        assert_eq!(ours[2].ty, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering));
        assert_eq!(on.binding_array_layout.entries, off.binding_array_layout.entries, "{key:?}");
        assert_eq!(on.empty_layout.entries, off.empty_layout.entries, "{key:?}");
    }
}

/// The light as it was approved, statement by statement: the stand of k without support, the
/// debit from the occluded ambient, the occlusion once on the bounce, the colour's way back,
/// the share over all of it. The module is valid by itself, with four reads.
#[test]
fn the_light_is_the_approved_one() {
    let wesl = unix_lines(include_str!("../rtgi_volume.wesl"));
    let module = naga::front::wgsl::parse_str(&wesl).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&wesl)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(wesl.matches("textureSampleLevel(").count(), 4);
    let mut last = 0;
    for line in [
        "let q = (p + nvf_rtgi.dimensions.w * g - nvf_rtgi.origin.xyz) * nvf_rtgi.origin.w;",
        "if !(all(q > vec3(-1.0)) && all(q < nvf_rtgi.dimensions.xyz)) {",
        "let t = q + 1.5;",
        "let slab = vec3(0.0, 0.0, nvf_rtgi.atlas.w);",
        "let w = 1.0 / max(c0.a, nvf_rtgi.fit.z);",
        "let e = max(w * (c0.rgb + cx.rgb * n.x + cy.rgb * n.y + cz.rgb * n.z), vec3(0.0));",
        "return NvfRtgiRead(e, w * cx.a);",
        "return pow(max(x, vec3(0.0)), vec3(2.2));",
        "dot(x, vec3(0.2126, 0.7152, 0.0722));",
        "let g = select(face, -face, dot(face, to_view) < 0.0);",
        "let k = nvf_rtgi.fit.y + (1.0 - nvf_rtgi.fit.y) * min(nvf_rtgi.fit.x * read.v, 1.0);",
        "let bounce = nvf_rtgi.light.y * read.e * occlusion;",
        "var light = nvf_rtgi_linear(sum - (1.0 - nvf_rtgi.light.x) * k * ambient) + bounce;",
        "let was = nvf_rtgi_linear(sum);",
        "light += nvf_rtgi.fit.w * (nvf_rtgi_luminance(light) * was / before - light);",
        "if nvf_rtgi.light.z < 1.0 {\n        light = was + nvf_rtgi.light.z * (light - was);\n    }\n    return light;\n",
    ] {
        assert_eq!(wesl.matches(line).count(), 1, "{line}");
        let at = wesl.find(line).unwrap();
        assert!(at > last, "in this order: {line}");
        last = at;
    }
}

// ---------------------------------------------------------------------------------------
// The view keys.

pub(super) fn extracted(view: RetainedViewEntity) -> ExtractedView {
    ExtractedView {
        retained_view_entity: view,
        clip_from_view: Mat4::IDENTITY,
        world_from_view: GlobalTransform::default(),
        clip_from_world: None,
        target_format: TextureFormat::Rgba16Float,
        viewport: UVec4::ZERO,
        color_grading: default(),
        invert_culling: false,
    }
}

pub(super) fn bindings_on(device: &RenderDevice) -> NvfRtgiVolumeBindings {
    let texture = device.create_texture(&TextureDescriptor {
        label: None,
        size: Extent3d { width: 3, height: 3, depth_or_array_layers: 12 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D3,
        format: TextureFormat::Rgba16Float,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    NvfRtgiVolumeBindings {
        uniform: device.create_buffer(&BufferDescriptor { label: None, size: 80, usage: BufferUsages::UNIFORM, mapped_at_creation: false }),
        texture: texture.create_view(&TextureViewDescriptor::default()),
        sampler: device.create_sampler(&SamplerDescriptor::default()),
    }
}

/// Two cameras through bevy_pbr's own systems: the bit is in the key of the one with the
/// component, and nowhere else (the other camera, the prepass, a light's shadow view that was
/// given the component too); without it the key is the old one again.
#[test]
fn only_the_bound_view_has_the_bit() {
    let (device, ..) = noop();
    let mut world = World::new();
    world.init_resource::<ViewKeyCache>();
    world.init_resource::<ViewKeyPrepassCache>();
    world.init_resource::<LightKeyCache>();
    world.init_resource::<DirtySpecializations>();
    let [a, b, lamp] = std::array::from_fn(|_| RetainedViewEntity::new(MainEntity::from(world.spawn_empty().id()), None, 0));
    let mut phases = ViewBinnedRenderPhases::<Shadow>::default();
    phases.prepare_for_new_frame(lamp, GpuPreprocessingMode::None);
    world.insert_resource(phases);
    let first = world.spawn((extracted(a), Msaa::Off, DepthPrepass, NormalPrepass, MotionVectorPrepass)).id();
    world.spawn((extracted(b), Msaa::Sample4));
    let light_entity = world.spawn_empty().id();
    let shadow = world.spawn((extracted(lamp), LightEntity::Spot { light_entity })).id();
    let keys = |world: &mut World| {
        world.resource_mut::<DirtySpecializations>().views.clear();
        world.run_system_cached(check_views_need_specialization).unwrap();
        let dirty = world.resource::<DirtySpecializations>().views.clone();
        world.run_system_cached(check_prepass_views_need_specialization).unwrap();
        world.run_system_cached(check_views_lights_need_specialization).unwrap();
        let (view, prepass, light) = (world.resource::<ViewKeyCache>(), world.resource::<ViewKeyPrepassCache>(), world.resource::<LightKeyCache>());
        assert!(prepass.len() == 2 && prepass.values().all(|k| !k.contains(BIT)), "the prepass");
        assert!(light.len() == 1 && light.values().all(|k| !k.contains(BIT)), "the shadow view");
        ([view[&a], view[&b]], [prepass[&a], prepass[&b], light[&lamp]], [dirty.contains(&a), dirty.contains(&b)])
    };
    let (views, others, _) = keys(&mut world);
    assert!(views.iter().all(|k| !k.contains(BIT)));
    assert_eq!(keys(&mut world), (views, others, [false, false]), "nothing new, nothing specialised again");
    let bound = bindings_on(&device);
    world.entity_mut(first).insert(bound.clone());
    world.entity_mut(shadow).insert(bound);
    assert_eq!(keys(&mut world), ([views[0] | BIT, views[1]], others, [true, false]), "the one view alone");
    world.entity_mut(first).remove::<NvfRtgiVolumeBindings>();
    assert_eq!(keys(&mut world), (views, others, [true, false]), "the old key again");
}

// ---------------------------------------------------------------------------------------
// The readers.

/// Over every flag of a NIF surface, every value of the blend bits and both states of the
/// view: opaque or alpha to coverage, in a view with the bit, and none of the six flags.
#[test]
fn which_surfaces_read() {
    for set in 0..1u32 << 13 {
        let on = |i: u32| set >> i & 1 == 1;
        let surface = NifSurfaceKey {
            refract: on(0),
            first_person: on(1),
            pipboy_screen: on(2),
            add_linear: on(3),
            depth_write: on(4),
            terrain_layer: on(5),
            decal: on(6),
            hard_edges: on(7),
            shade_light: on(8),
            facegen: on(9),
            anti_tiling: on(10),
            soft: on(11),
            hdr: on(12),
        };
        let may = !(0..6).any(on);
        for n in 0..8 {
            let blend = MeshPipelineKey::from_bits_retain(n * MeshPipelineKey::BLEND_PREMULTIPLIED_ALPHA.bits());
            let solid = blend == MeshPipelineKey::BLEND_OPAQUE || blend == MeshPipelineKey::BLEND_ALPHA_TO_COVERAGE;
            let others = MeshPipelineKey::MAY_DISCARD | MeshPipelineKey::DEPTH_PREPASS | MeshPipelineKey::NVF_AMBIENT_OCCLUSION | MeshPipelineKey::NVF_SUN_VISIBILITY | MeshPipelineKey::from_msaa_samples(4);
            for more in [MeshPipelineKey::NONE, others] {
                assert!(!reads_rtgi_volume(&surface, blend | more), "a view without the bindings");
                assert_eq!(reads_rtgi_volume(&surface, blend | more | BIT), may && solid, "{surface:?} {blend:?}");
            }
        }
    }
}

/// The same from materials: what the alpha mode makes of the blend bits, the landscape's
/// repeat, a face.
#[test]
fn which_materials_read() {
    let surface = |change: &dyn Fn(&mut NifSurface)| {
        let mut s = NifSurface::default();
        change(&mut s);
        NifSurfaceKey::from(&s)
    };
    let plain = surface(&|_| {});
    for msaa in [Msaa::Off, Msaa::Sample4] {
        let reads = |mode| reads_rtgi_volume(&plain, BIT | alpha_mode_pipeline_key(mode, &msaa));
        for mode in [AlphaMode::Opaque, AlphaMode::Mask(0.5), AlphaMode::AlphaToCoverage] {
            assert!(reads(mode), "{mode:?} {msaa:?}");
        }
        for mode in [AlphaMode::Blend, AlphaMode::Premultiplied, AlphaMode::Add, AlphaMode::Multiply] {
            assert!(!reads(mode), "{mode:?} {msaa:?}");
        }
    }
    assert!(reads_rtgi_volume(&surface(&|s| s.decal = true), BIT) && reads_rtgi_volume(&surface(&|s| s.facegen_offset = Some(Handle::default())), BIT));
    let others: [&dyn Fn(&mut NifSurface); 6] = [&|s| s.refract = true, &|s| s.first_person = true, &|s| s.pipboy_screen = true, &|s| s.add_linear = true, &|s| s.depth_write = true, &|s| s.terrain_repeat = 6.0];
    for change in others {
        assert!(!reads_rtgi_volume(&surface(change), BIT));
    }
    // The mesh terrain blend's pipelines give the def up again (the pipeline's own state needs a
    // device's layouts), and the fragment keeps its face before anything is discarded.
    assert_eq!(unix_lines(include_str!("../../../nvf-assets/src/nif_surface_blend.rs")).matches("rtgi_volume_opt_out(descriptor);").count(), 1);
    let material = unix_lines(include_str!("../../../nvf-assets/src/nif_material.wesl"));
    let body = block_after(&material, "fn nif_fragment(");
    let face = body.find("@if(NVF_RTGI_VOLUME && !PREPASS_PIPELINE) nvf_rtgi_face(in.world_position.xyz);").expect("the face");
    assert!(face < body.find("visibility_range_dither(").unwrap() && face < body.find("discard;").unwrap());
    assert_eq!(RTGI_VOLUME_DEF, "NVF_RTGI_VOLUME");
}

/// A blended cut-out reads while a temporal upscaler (FSR or DLSS) has it drawn as a mask
/// (graphics.rs `apply`), and is stock while it is drawn blended.
#[test]
fn blended_cutouts_read_while_they_are_masks() {
    let mut app = App::new();
    app.init_resource::<Assets<NifMaterial>>().init_resource::<BlendedCutouts>().add_message::<AssetEvent<NifMaterial>>().add_systems(PostUpdate, nvf_assets::nif_material::blended_cutout_system());
    let material = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial {
        base: StandardMaterial { alpha_mode: AlphaMode::Blend, ..default() },
        extension: NifSurface { blended_cutout: Some(0.5), ..default() },
    });
    for on in [true, false, true] {
        app.world_mut().insert_resource(BlendedCutouts(on));
        app.update();
        let m = app.world().resource::<Assets<NifMaterial>>().get(&material).unwrap();
        // A temporal upscaler takes the place of multisampling.
        let msaa = if on { Msaa::Off } else { Msaa::Sample4 };
        assert_eq!(reads_rtgi_volume(&NifSurfaceKey::from(&m.extension), BIT | alpha_mode_pipeline_key(m.base.alpha_mode, &msaa)), on);
    }
}

// ---------------------------------------------------------------------------------------
// The hook.

/// `pbr_functions` as lights.rs has it before this hook, and after it.
fn hooked() -> (String, String) {
    let display = crate::lights::display_patched(&vendored("pbr_functions.wesl")).expect("display patch applies");
    let sun = crate::sun_visibility::hook_patched(&display).expect("sun hook applies");
    let ao = crate::ssao_lighting::hook_patched(&sun).expect("ambient occlusion hook applies");
    let hooked = hook_patched(&ao).expect("RTGI volume hook applies");
    (ao, hooked)
}

#[test]
fn the_hook_undone_is_the_input_and_each_variant_resolves() {
    let (input, hooked) = hooked();
    assert_ne!(input, hooked);
    let back = HOOK_EDITS.iter().fold(hooked.clone(), |text, (line, hooked)| text.replacen(hooked, line, 1));
    assert_eq!(back, input);
    assert!(DIFFUSE_LINE_HOOKED.starts_with(&format!("        @if(!NVF_RTGI_VOLUME)\n{DIFFUSE_LINE}")), "the line kept word for word");
    for ao in [false, true] {
        for sun in [false, true] {
            let known = [("NVF_AMBIENT_OCCLUSION", ao), ("NVF_SUN_VISIBILITY", sun)];
            let with = |rtgi: bool| {
                let mut all = known.to_vec();
                all.push((RTGI_VOLUME_DEF, rtgi));
                resolve_known(&hooked, &all)
            };
            let off = with(false);
            assert_eq!(code_lines(&off), code_lines(&resolve_known(&input, &known)), "ao {ao} sun {sun}");
            assert!(!code(&off).contains("nvf_rtgi"));
            let on = with(true);
            assert!(balanced(&on));
            let lighting = block_after(&on, "fn nvf_apply_pbr_lighting(");
            assert_eq!(lighting.matches("direct_light += nvf_display_to_linear(min(nvf_linear_to_display(diffuse_color) * ").count(), 1, "ao {ao} sun {sun}: one diffuse line");
            assert_eq!(lighting.matches("nvf_rtgi_light(nvf_weights.x * nvf_ambient, nvf_sum, nvf_rtgi_occlusion, in.world_position.xyz, in.N, in.V)").count(), 1);
            assert_eq!(lighting.matches("let nvf_rtgi_occlusion = diffuse_occlusion;").count(), usize::from(!ao));
            assert_eq!(lighting.matches("let nvf_rtgi_occlusion = diffuse_occlusion * nvf_ao_multibounce(nvf_ao_ambient, diffuse_color);").count(), usize::from(ao));
            assert_eq!(lighting.matches("nvf_ambient_visibility(").count(), usize::from(ao), "the texels are still read once");
            let at = |s: &str| lighting.find(s).unwrap_or_else(|| panic!("{s}"));
            assert!(at("let nvf_sum = ") < at("let nvf_rtgi_occlusion = ") && at("let nvf_rtgi_occlusion = ") < at("nvf_rtgi_light("));
        }
    }
}

#[test]
fn a_missing_or_doubled_line_leaves_the_source_alone() {
    let (input, _) = hooked();
    for (line, _) in HOOK_EDITS {
        assert_eq!(input.matches(line).count(), 1, "{line:?}");
        assert!(hook_patched(&input.replacen(line, "", 1)).is_none(), "missing {line:?}");
        assert!(hook_patched(&format!("{input}{line}")).is_none(), "twice {line:?}");
    }
    assert!(hook_patched(&vendored("pbr_functions.wesl")).is_none(), "not without the display-value sum");
    let display = crate::lights::display_patched(&vendored("pbr_functions.wesl")).unwrap();
    assert!(hook_patched(&crate::sun_visibility::hook_patched(&display).unwrap()).is_none(), "nor without the ambient occlusion's hook");
}

// ---------------------------------------------------------------------------------------
// The composed shaders, the whole chain of lights.rs.

const NIF: &str = "embedded://nvf_assets/nif_material.wesl";
const PBR_FUNCTIONS: &str = "embedded://bevy_pbr/render/pbr_functions.wesl";

/// Every module the world's shaders import, `pbr_functions` as lights.rs patches it. Without
/// `hook` the bounce light is taken out of everything it touches: no hook, no shader of its own,
/// and not the two statements in nif_material.wesl.
pub(super) fn modules(hook: bool) -> Vec<(String, String)> {
    let (without, with) = hooked();
    let functions = crate::lights::reflection_gate::gate_patched(if hook { &with } else { &without }).expect("the reflection gate applies");
    let mut modules = vendored_modules();
    modules.iter_mut().find(|(p, _)| p == PBR_FUNCTIONS).expect("pbr_functions").1 = functions;
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for (package, root) in [("nvf_assets", src.join("../nvf-assets/src")), ("nvforever", src.join("src"))] {
        sources(&root, package, &root, &mut modules);
    }
    if !hook {
        modules.retain(|(p, _)| p != "embedded://nvforever/rtgi_volume.wesl");
        let nif = &mut modules.iter_mut().find(|(p, _)| p == NIF).expect("the NIF shader").1;
        let text = unix_lines(nif);
        let (ours, rest): (Vec<&str>, Vec<&str>) = text.lines().partition(|l| l.contains(RTGI_VOLUME_DEF));
        assert!(ours.len() == 2 && ours.iter().all(|l| l.trim_start().starts_with("@if(")), "{ours:?}");
        *nif = rest.join("\n");
    }
    modules
}

/// The NIF fragment's defs and `extra`, bindless where that says so.
fn defs(extra: &[&'static str]) -> Vec<ShaderDefVal> {
    let mut out: Vec<ShaderDefVal> = ["BINDLESS", "NIF_SURFACE_BLEND", "MESH_TERRAIN_BLEND", "PREPASS_PIPELINE"].into_iter().filter(|d| !extra.contains(d)).map(|d| ShaderDefVal::Bool(d.into(), false)).collect();
    out.extend(["VERTEX_POSITIONS", "VERTEX_NORMALS", "VERTEX_UVS", "VERTEX_UVS_A", "VERTEX_TANGENTS", "VERTEX_COLORS", "VERTEX_OUTPUT_INSTANCE_INDEX", "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3", "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_6"].map(ShaderDefVal::from));
    out.extend([("MATERIAL_BIND_GROUP", 3), ("MAX_DIRECTIONAL_LIGHTS", 10), ("MAX_CASCADES_PER_LIGHT", 4), ("MAX_RECT_LIGHTS", 1)].map(|(d, n)| ShaderDefVal::UInt(d.into(), n)));
    out.extend(extra.iter().map(|d| ShaderDefVal::from(*d)));
    out
}

const SUN: [&str; 4] = ["NVF_SUN_VISIBILITY", "NVF_CLOUD_VISIBILITY", "NVF_TERRAIN_VISIBILITY", "NVF_CLOUD_VOLUME"];

pub(super) fn bindings(source: &str) -> BTreeSet<(u32, u32)> {
    let module = naga::front::wgsl::parse_str(source).unwrap();
    module.global_variables.iter().filter_map(|(_, g)| g.binding.as_ref().map(|b| (b.group, b.binding))).collect()
}

/// Two composed shaders declare the same: whole declarations, statement by statement, in any
/// order of the declarations (which composition does not hold to), and the same bindings.
fn same(case: &str, before: &str, after: &str) {
    if let Some(d) = declaration_difference(&declarations(after), &declarations(before)) {
        panic!("{case}: {d}");
    }
    assert_eq!(bindings(after), bindings(before), "{case}");
    assert!(!after.contains("rtgi"), "{case}");
}

/// Without the def every shader of the world's that the patched lighting or the NIF source
/// reaches declares exactly what it declares with the bounce light taken out.
#[test]
fn without_the_def_the_composed_shaders_are_what_they_were() {
    let (before, after) = (modules(false), modules(true));
    let check = |entry: &str, defs: &[ShaderDefVal], case: &str| same(&format!("{entry} {case}"), &compose(&before, entry, defs), &compose(&after, entry, defs));
    let nif: [&[&'static str]; 17] = [
        &[],
        &["BINDLESS"],
        &["ALPHA_TO_COVERAGE", "MULTISAMPLED"],
        &["BINDLESS", "ALPHA_TO_COVERAGE", "MULTISAMPLED"],
        &["MAY_DISCARD"],
        &["MAY_DISCARD", "SOFT_EDGES", "MULTISAMPLED"],
        &["MAY_DISCARD", "PREPASS_COVERAGE", "DEPTH_PREPASS"],
        &["DEPTH_PREPASS", "NORMAL_PREPASS", "LOAD_PREPASS_NORMALS", "MOTION_VECTOR_PREPASS"],
        &["NVF_AMBIENT_OCCLUSION", "DEPTH_PREPASS"],
        &SUN[..3],
        &SUN,
        &["FIRST_PERSON_DEPTH", "SHADE_LIGHT"],
        &["STANDARD_MATERIAL_NORMAL_MAP", "FACEGEN_TINT"],
        &["BLEND_MULTIPLY", "PREMULTIPLY_ALPHA"],
        &["REFRACT", "VIEW_TRANSMISSION_TEXTURE"],
        &["NVF_PBR_REFLECT", "ENVIRONMENT_MAP"],
        &["DISTANCE_FOG", "NVF_AMBIENT_OCCLUSION"],
    ];
    for extra in nif {
        check(NIF, &defs(extra), &format!("{extra:?}"));
    }
    // The depth prepass and shadow passes of the NIF shapes, and the mesh terrain blend, whose
    // pipelines never have the def.
    check("embedded://nvf_assets/nif_prepass.wesl", &nif_defs(false, false, true, false, HeightLayout::STORED), "prepass");
    for bindless in [false, true] {
        check(NIF, &nif_defs(bindless, true, false, true, HeightLayout::STORED), "mesh terrain blend");
    }
    check("embedded://bevy_pbr/render/pbr.wesl", &defs(&[]), "bark, impact decals");
    for ao in [false, true] {
        let more: &[&'static str] = if ao { &["NVF_AMBIENT_OCCLUSION"] } else { &[] };
        let mut terrain = terrain_defs(false, false, false, false, false);
        terrain.extend(more.iter().map(|d| ShaderDefVal::from(*d)));
        check("embedded://nvforever/terrain.wesl", &terrain, &format!("{more:?}"));
        for (entry, extra) in [("grass", &["VERTEX_UVS_B"][..]), ("tree", &["VERTEX_UVS_B", "TREE_LEAVES"]), ("tree", &["VERTEX_UVS_B", "TREE_LEAVES", "NVF_CROWN_SHADOW"])] {
            let all: Vec<&'static str> = extra.iter().chain(more).copied().collect();
            check(&format!("embedded://nvforever/{entry}.wesl"), &defs(&all), &format!("{all:?}"));
        }
    }
    for extra in [&["VIEW_TRANSMISSION_TEXTURE"][..], &["VIEW_TRANSMISSION_TEXTURE", "DEPTH_PREPASS"]] {
        check("embedded://nvforever/water.wesl", &defs(extra), &format!("{extra:?}"));
    }
}

const OURS: [(u32, u32); 3] = [(0, 47), (0, 48), (0, 49)];

/// With the def the NIF fragment composes and validates in its variants and reads the three
/// bindings more, all the others too; the uniform is the one the notes describe.
#[test]
fn with_the_def_the_nif_fragment_reads_the_volume() {
    let modules = modules(true);
    let reads = |extra: &[&'static str]| {
        let off = bindings(&compose(&modules, NIF, &defs(extra)));
        assert!(OURS.iter().all(|b| !off.contains(b)), "{extra:?}");
        let on = compose(&modules, NIF, &defs(&[extra, &[RTGI_VOLUME_DEF]].concat()));
        assert_eq!(bindings(&on), off.into_iter().chain(OURS).collect(), "{extra:?}");
        let module = naga::front::wgsl::parse_str(&on).unwrap();
        let uniform = module.global_variables.iter().find(|(_, g)| g.binding.as_ref().is_some_and(|b| (b.group, b.binding) == OURS[0])).expect("the uniform").1;
        let naga::TypeInner::Struct { members, span } = &module.types[uniform.ty].inner else { panic!("a struct") };
        assert_eq!(*span, 80);
        assert_eq!(members.iter().map(|m| (m.name.as_deref().unwrap_or(""), m.offset)).collect::<Vec<_>>(), [("origin", 0), ("dimensions", 16), ("atlas", 32), ("fit", 48), ("light", 64)]);
    };
    let coverage: [&[&'static str]; 4] = [&[], &["MAY_DISCARD"], &["MAY_DISCARD", "SOFT_EDGES", "DEPTH_PREPASS"], &["MAY_DISCARD", "PREPASS_COVERAGE", "DEPTH_PREPASS"]];
    for i in 0..8 {
        let mut extra: Vec<&'static str> = Vec::new();
        for (bit, set) in [&["MULTISAMPLED"][..], &["NVF_AMBIENT_OCCLUSION"], &SUN].into_iter().enumerate() {
            if (i >> bit) & 1 == 1 {
                extra.extend(set);
            }
        }
        for (j, cover) in coverage.iter().enumerate() {
            let mut all = extra.clone();
            all.extend(*cover);
            // The prepass with normals and motion vectors (TAA, the upscalers), single-sampled.
            if i & 1 == 0 && j == 1 {
                all.extend(["DEPTH_PREPASS", "NORMAL_PREPASS", "LOAD_PREPASS_NORMALS", "MOTION_VECTOR_PREPASS"]);
            }
            reads(&all);
        }
    }
    let others: [&[&'static str]; 6] = [
        &["BINDLESS"],
        &["BINDLESS", "ALPHA_TO_COVERAGE", "MULTISAMPLED"],
        &["ALPHA_TO_COVERAGE", "MULTISAMPLED"],
        &["NVF_PBR_REFLECT", "ENVIRONMENT_MAP"],
        &["DISTANCE_FOG", "NVF_AMBIENT_OCCLUSION"],
        &["STANDARD_MATERIAL_NORMAL_MAP", "FACEGEN_TINT"],
    ];
    others.iter().for_each(|extra| reads(extra));
}
