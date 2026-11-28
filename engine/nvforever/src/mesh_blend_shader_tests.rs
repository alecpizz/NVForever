//! [design] CPU-only WESL composition and naga validation of the production binding/
//! blend module. Minimal mesh/PBR declarations stand in for the unrelated renderer.
//! Further tests compose the complete forward and NIF depth-prepass shaders, the folded
//! terrain bake and the single-texture capture using repository-vendored modules, and
//! count the textures each declares per bind group. None executes fragments or creates
//! GPU pipelines.

use bevy::prelude::*;
use bevy::shader::{ShaderCache, ShaderCacheSource, ShaderDefVal};
use wgpu::naga;

/// A CPU shader cache whose module-creation callback parses and validates with naga.
fn validating_cache() -> ShaderCache<String, ()> {
    ShaderCache::new((), |_: &(), source, _| {
        let ShaderCacheSource::Wgsl(source) = source else { panic!("expected WGSL") };
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&module).unwrap_or_else(|e| panic!("{e:?}\n{source}"));
        Ok(source)
    })
}

pub(crate) fn sources(directory: &std::path::Path, package: &str, root: &std::path::Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() { sources(&path, package, root, out); }
        else if path.extension().is_some_and(|e| e == "wesl") {
            let relative = path.strip_prefix(root).unwrap().with_extension("").to_string_lossy().replace('\\', "/");
            out.push((format!("embedded://{package}/{relative}.wesl"), std::fs::read_to_string(path).unwrap()));
        }
    }
}

/// The vendored Bevy PBR/render modules, plus the fullscreen vertex IO type imported by
/// Bevy's SSR library (bevy_core_pipeline is not vendored; no tested entry point uses it).
pub(crate) fn vendored_modules() -> Vec<(String, String)> {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut modules = Vec::new();
    for package in ["bevy_pbr", "bevy_render"] {
        let root = manifest.join("../vendor").join(package).join("src");
        sources(&root, package, &root, &mut modules);
    }
    modules.push(("embedded://bevy_core_pipeline/fullscreen_vertex_shader.wesl".into(),
        "struct FullscreenVertexOutput { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }".into()));
    modules
}

/// Composes `entry` from `modules` with `defs` (naga-validated), and returns its source.
pub(crate) fn compose(modules: &[(String, String)], entry: &str, defs: &[ShaderDefVal]) -> String {
    let mut cache = validating_cache();
    let mut assets = Assets::<Shader>::default();
    let mut root = None;
    for (path, source) in modules {
        let shader = Shader::from_wesl(source.clone(), path.clone());
        let handle = assets.add(shader.clone());
        cache.set_shader(handle.id(), shader);
        if path == entry { root = Some(handle); }
    }
    cache.get(0, root.unwrap_or_else(|| panic!("no module {entry}")).id(), defs)
        .map(std::sync::Arc::unwrap_or_clone).unwrap_or_else(|e| panic!("{entry} {defs:?}: {e:?}"))
}

/// The (group, binding) of every sampled texture (plain or binding array) the composed
/// module declares. Composition keeps only what the entry point reaches, so this is what
/// the shader uses, a subset of its pipeline layout (which Rust's AsBindGroup makes).
pub(crate) fn texture_bindings(source: &str) -> std::collections::BTreeSet<(u32, u32)> {
    let module = naga::front::wgsl::parse_str(source).unwrap();
    let mut bindings = std::collections::BTreeSet::new();
    for (_, variable) in module.global_variables.iter() {
        let Some(binding) = &variable.binding else { continue };
        let inner = match &module.types[variable.ty].inner {
            naga::TypeInner::BindingArray { base, .. } => &module.types[*base].inner,
            inner => inner,
        };
        if matches!(inner, naga::TypeInner::Image { class: naga::ImageClass::Sampled { .. } | naga::ImageClass::Depth { .. }, .. }) {
            bindings.insert((binding.group, binding.binding));
        }
    }
    bindings
}

#[test]
fn composed_production_blend_module_validates_with_key_on_and_off() {
    use nvf_assets::nif_surface_blend::HeightLayout;
    // #2149/#2661: the blend at every height atlas layout (smoothing off, 2 x 2, 4 x 4).
    let layouts = [HeightLayout::STORED, HeightLayout::new(2), HeightLayout::new(4)];
    for (bindless, layout) in [false, true].into_iter().flat_map(|b| layouts.map(|l| (b, l))) {
        for blend in [false, true] {
            let mut modules: Vec<(String, String)> = vec![
                ("embedded://bevy_pbr/render/mesh_bindings.wesl".into(), "struct Mesh { material_and_lightmap_bind_group_slot: u32 } @group(2) @binding(0) var<storage> mesh: array<Mesh>;".into()),
                ("embedded://bevy_pbr/render/pbr_types.wesl".into(), "struct StandardMaterial { base_color: vec4<f32> }".into()),
                ("embedded://bevy_pbr/render/pbr_bindings.wesl".into(), r#"
                    import bevy_pbr::render::pbr_types::StandardMaterial;
                    struct Indices { material: u32, base_color_texture: u32, base_color_sampler: u32, normal_map_texture: u32, normal_map_sampler: u32 }
                    @if(BINDLESS) @group(3) @binding(0) var<storage> material_indices: array<Indices>;
                    @if(BINDLESS) @group(3) @binding(1) var<storage> material_array: array<StandardMaterial>;
                    @if(!BINDLESS) @group(3) @binding(0) var<uniform> material: StandardMaterial;
                    @if(!BINDLESS) @group(3) @binding(1) var base_color_texture: texture_2d<f32>;
                    @if(!BINDLESS) @group(3) @binding(2) var base_color_sampler: sampler;
                    @if(!BINDLESS) @group(3) @binding(3) var normal_map_texture: texture_2d<f32>;
                    @if(!BINDLESS) @group(3) @binding(4) var normal_map_sampler: sampler;
                "#.into()),
                // As engine/vendor/bevy_render/src/bindless.wesl: binding arrays need the
                // enable directive.
                ("embedded://bevy_render/bindless.wesl".into(), r#"
                    @if(BINDLESS)
                    enable wgpu_binding_array;
                    @if(BINDLESS) @group(3) @binding(10) var bindless_textures_2d: binding_array<texture_2d<f32>>;
                    @if(BINDLESS) @group(3) @binding(11) var bindless_samplers_filtering: binding_array<sampler>;
                "#.into()),
                ("embedded://nvf_assets/nif_surface_bindings.wesl".into(), include_str!("../../nvf-assets/src/nif_surface_bindings.wesl").into()),
                // #2550/#2551: the shared plain-WGSL modules the detail composes with.
                ("embedded://nvf_assets/terrain_sampling.wesl".into(), nvf_assets::terrain_sampling::SAMPLING_SOURCE.into()),
                ("embedded://nvforever/terrain_weights.wesl".into(), include_str!("terrain_weights.wesl").into()),
                // Stand-ins with the signatures of Bevy's TBN and normal-map helpers (the
                // complete composition below uses the vendored ones).
                ("embedded://bevy_pbr/render/pbr_functions.wesl".into(), r#"
                    fn calculate_tbn_mikktspace(world_normal: vec3<f32>, world_tangent: vec4<f32>) -> mat3x3<f32> {
                        return mat3x3(world_tangent.xyz, world_tangent.w * cross(world_normal, world_tangent.xyz), world_normal);
                    }
                    fn apply_normal_mapping(flags: u32, tbn: mat3x3<f32>, double_sided: bool, is_front: bool, nt: vec3<f32>) -> vec3<f32> {
                        return normalize(tbn * (nt * 2.0 - 1.0) * f32(flags + 1u));
                    }
                "#.into()),
            ];
            modules.push(("embedded://nvforever/blend_validation.wesl".into(), r#"
                import nvf_assets::nif_surface_bindings::{nif_surface, holes_alpha, facegen_size, facegen_texel};
                @if(MESH_TERRAIN_BLEND) import nvf_assets::nif_surface_bindings::{mesh_terrain_lookup, mesh_terrain_diagnostic, mesh_terrain_view, mesh_terrain_shading, mesh_terrain_blend_normal, mesh_terrain_target_colour, mesh_terrain_target_normal, mesh_terrain_face_up, MeshTerrainInputs, LAND_ROUGHNESS, LAND_REFLECTANCE};
                @fragment fn fragment(@location(0) world: vec3<f32>, @location(1) normal: vec3<f32>) -> @location(0) vec4<f32> {
                    var colour = nif_surface(0u).falloff.xyz * holes_alpha(0u, world.xy) + facegen_texel(0u, vec2<i32>(facegen_size(0u)));
                    @if(MESH_TERRAIN_BLEND) {
                        let wdx = dpdx(world);
                        let wdy = dpdy(world);
                        let inputs = MeshTerrainInputs(length(world), vec2(wdx.x, -wdx.z), vec2(wdy.x, -wdy.z), 0.0, mesh_terrain_shading(0u), mesh_terrain_face_up(wdx, wdy, normal));
                        let t = mesh_terrain_lookup(0u, world, normal, inputs);
                        colour = mix(colour, mesh_terrain_target_colour(t), t.weight) + mesh_terrain_blend_normal(normal, mesh_terrain_target_normal(t), t.weight) * LAND_ROUGHNESS * LAND_REFLECTANCE;
                        if mesh_terrain_shading(0u) == 0u { colour = mesh_terrain_diagnostic(mesh_terrain_view(0u), t, colour); }
                    }
                    return vec4(colour, 1.0);
                }
            "#.into()));
            let mut defs = vec![
                ShaderDefVal::Bool("BINDLESS".into(), bindless),
                ShaderDefVal::Bool("NIF_SURFACE_BLEND".into(), blend),
                ShaderDefVal::Bool("MESH_TERRAIN_BLEND".into(), blend),
                ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 3),
            ];
            if blend { defs.extend(nvf_assets::nif_surface_blend::blend_shader_defs(layout)); }
            let source = compose(&modules, "embedded://nvforever/blend_validation.wesl", &defs);
            let module = naga::front::wgsl::parse_str(&source).unwrap();
            let mut surface_sizes = Vec::new();
            let mut index_sizes = Vec::new();
            for (_, ty) in module.types.iter() {
                if let naga::TypeInner::Struct { ref members, span } = ty.inner {
                    if members.first().is_some_and(|m| m.name.as_deref() == Some("falloff")) { surface_sizes.push(span); }
                    if members.first().is_some_and(|m| m.name.as_deref() == Some("surface")) { index_sizes.push(span); }
                }
            }
            if blend {
                assert!(source.contains("terrain_covered"), "enabled coverage survives bindless and ordinary composition");
                assert!(source.contains("mesh_terrain_origin.z"), "coverage reuses the enabled uniform without another binding");
                // Round 10a: the layer detail only where the palette exists (bindless).
                assert_eq!(source.contains("textureSampleGrad"), bindless, "bindless={bindless}: detail reads with explicit gradients, none without bindless");
                // #2149/#2661: only a subdivided surface's lookup finds the ground's own
                // square; the stored layout's lookup is the one before it.
                assert_eq!(source.contains("terrain_square"), layout.sub() > 1, "{layout:?}");
            }
            // Round 9 reuses origin.z; round 6's enabled width and ordinary layouts stay fixed.
            // #2551 added the ordinary `terrain` row (144 bytes); the blend's three rows follow.
            assert_eq!(surface_sizes, vec![if blend { 192 } else { 176 }], "the ordinary type alone has #2462's env row and #2841's env_how row after the 144 bytes");
            // The blend type reuses the ordinary table and slots (nif_surface_blend.rs notes);
            // round 10a's bindless blend row is longer by the palette's 400 entries.
            if bindless { assert_eq!(index_sizes, vec![if blend { 16 + 4 * nvf_assets::nif_surface_blend::PALETTE_ENTRIES as u32 } else { 28 }], "the ordinary row: four entries, then the window environment map's three (#2462)"); }
            assert_eq!(nvf_assets::nif_surface_blend::PALETTE_ENTRIES, 400, "nif_surface_bindings.wesl spells out array<u32, 400>");
            if !bindless {
                let bound = |binding| module.global_variables.iter().any(|(_, v)| v.binding.as_ref().is_some_and(|b| b.group == 3 && b.binding == binding));
                // Round 10b review: the blend reads its colour atlas with textureLoad (weighted
                // in the shader), so it need not reach its sampler (52, still in the layout
                // AsBindGroup makes); the ordinary type's holes sampler is reached.
                for binding in [50, 51, 53] { assert!(bound(binding), "bindless={bindless} blend={blend} binding {binding}"); }
                if !blend { assert!(bound(52), "bindless={bindless} blend={blend} binding 52"); }
                for binding in [54, 55, 56] { assert!(!bound(binding), "no binding past main's 53"); }
                let extension: Vec<_> = texture_bindings(&source).into_iter().filter(|&(g, b)| g == 3 && b >= 50).collect();
                assert_eq!(extension, vec![(3, 51), (3, 53)], "bindless={bindless} blend={blend}: textures only in slots 51 and 53");
            }
        }
    }
    let material = include_str!("../../nvf-assets/src/nif_material.wesl");
    assert!(material.contains("let terrain = mesh_terrain_lookup(in.instance_index, in.world_position.xyz, pbr_input.world_normal, terrain_inputs);"), "the weight uses the unperturbed normal");
    assert!(material.find("let terrain = mesh_terrain_lookup(").unwrap() > material.find("alpha_discard(pbr_input.material.flags").unwrap(), "after the coverage tests");
    assert!(material.find("let terrain = mesh_terrain_lookup(").unwrap() < material.find("out.color = apply_pbr_lighting(pbr_input);").unwrap(), "before lighting");
    // [design] Round 10a: the game-xy derivatives are taken before the lookup, at the
    // function's top level (uniform control flow), never inside its branches. Merge review
    // (#2661): they are captured at the very start of nif_fragment, before the visibility
    // dither and before every discard (alpha cut, holes, alpha_discard), as terrain.wesl
    // captures its own; only the blend variant has the capture. #2661 review: the world
    // position's derivatives are captured there (the steepness fade's geometric normal),
    // and the game-xy derivatives are picked out of them.
    let fragment = material.find("fn nif_fragment(").expect("nif_fragment");
    // The function's body, after its own opening brace.
    let body = &material[fragment + material[fragment..].find('{').unwrap() + 1..];
    let at = |needle: &str| body.find(needle).unwrap_or_else(|| panic!("nif_fragment: {needle}"));
    let capture = [
        "let terrain_wdx = dpdx(in.world_position.xyz);",
        "let terrain_wdy = dpdy(in.world_position.xyz);",
        "let terrain_dx = vec2(terrain_wdx.x, -terrain_wdx.z);",
        "let terrain_dy = vec2(terrain_wdy.x, -terrain_wdy.z);",
    ];
    for statement in capture {
        let pos = at(statement);
        let line_start = body[..pos].rfind('\n').unwrap();
        assert_eq!(body[line_start + 1..pos].trim(), "@if(MESH_TERRAIN_BLEND && !PREPASS_PIPELINE)", "{statement}: a top-level statement of the blend variant only");
        assert!(pos < at("discard;"), "{statement}: before the first discard");
        assert!(pos < at("visibility_range_dither(in.position"), "{statement}: before the visibility dither");
        assert!(pos < at("alpha_discard(pbr_input.material.flags"), "{statement}: before alpha_discard");
        assert!(pos < at("{"), "{statement}: before the first block (no branch)");
    }
    assert_eq!(material.matches("dpdx(in.world_position.xyz)").count(), 1, "the world-position derivatives are taken once");
    assert_eq!(material.matches("dpdy(in.world_position.xyz)").count(), 1, "the world-position derivatives are taken once");
    assert!(at("mesh_terrain_face_up(terrain_wdx, terrain_wdy, pbr_input.world_normal)") > at("let terrain_inputs = MeshTerrainInputs("), "the face's steepness is one of the lookup's inputs");
    let inputs = at("let terrain_inputs = MeshTerrainInputs(distance(view.world_position, in.world_position.xyz), terrain_dx, terrain_dy, view.mip_bias,");
    assert!(inputs < at("let terrain = mesh_terrain_lookup("));
    let line_start = body[..inputs].rfind('\n').unwrap();
    assert_eq!(body[line_start + 1..inputs].trim(), "@if(MESH_TERRAIN_BLEND && !PREPASS_PIPELINE)", "a top-level statement, not inside a branch");
    // [design] Zero weight leaves every shading input as it was: the surface blend sits
    // inside the positive-weight branch, which hybrid v2 keeps.
    let branch = material.find("} else if terrain.status == LOOKUP_SUCCESS && terrain.weight > 0.0 {").expect("the positive-weight branch");
    for input in ["pbr_input.material.base_color = vec4(mix(pbr_input.material.base_color.rgb, mesh_terrain_target_colour(terrain), w)", "pbr_input.N = mesh_terrain_blend_normal(pbr_input.N, mesh_terrain_target_normal(terrain), w)", "pbr_input.material.perceptual_roughness = mix(", "pbr_input.material.emissive = vec4(pbr_input.material.emissive.rgb * (1.0 - w)"] {
        assert!(material.find(input).unwrap() > branch, "{input}");
    }
    // [design] The layer detail is read only inside the lookup's positive-weight, positive-
    // share branch, with explicit gradients from the inputs, and normal maps only for full
    // shading; never from the screen-space prepass normal.
    let bindings = include_str!("../../nvf-assets/src/nif_surface_bindings.wesl").replace("\r\n", "\n");
    let positive = bindings.find("    if r.weight > 0.0 {\n").expect("the positive-weight branch");
    // #2661 review: the colour cache is read only with a positive weight and only where it
    // has a share of the target (the layers' share below 1).
    let cache_read = bindings.find("if colour_valid && r.detail < 1.0 { r.colour = terrain_colour_mips(instance_index, slot, uv, resolution, levels); }").expect("the cache read");
    assert!(cache_read > positive && cache_read < bindings.find("@if(BINDLESS) if r.detail > 0.0 {").unwrap());
    assert!(bindings.find("@if(BINDLESS) if r.detail > 0.0 {").unwrap() > positive);
    assert!(bindings.contains("let ddx = axes * inputs.ddx * scale;") && bindings.contains("let scale = exp2(inputs.bias) / tile;"));
    assert!(bindings.contains("if full && (flags & 8u) != 0u {") && bindings.contains("        if full {\n            var layer_n = plain;"));
    assert!(!bindings.contains("prepass_normal") && !bindings.contains("prepass_utils"), "the target never comes from the prepass normal");
    let detail = bindings.split("fn mesh_terrain_detail(").nth(1).unwrap().split("\n}\n").next().unwrap();
    let sample = bindings.split("fn detail_sample(").nth(1).unwrap().split("\n}\n").next().unwrap();
    for body in [detail, sample] {
        assert!(!body.contains("textureSample(") && !body.contains("textureSampleBias(") && !body.contains("dpdx") && !body.contains("fwidth"), "no implicit derivatives in the divergent detail branch");
    }
    assert!(sample.contains("textureSampleGrad("));
    // [design] Round 10b: the colour cache is read at explicit levels chosen from the same
    // derivatives (no implicit derivatives inside the lookup's branches) and both
    // contributing levels' footprints validated. Round 10b review: each level is the
    // bilinear footprint at the fragment's position across valid neighbouring cells, its
    // taps loaded and weighted in the shader, unreadable ones given no weight and the rest
    // renormalized (terrain_surface.rs `colour_read_taps`).
    for name in ["terrain_colour_tap_valid", "terrain_colour_taps", "terrain_colour_level", "terrain_colour_mips", "terrain_colour_lod", "terrain_colour_level_valid", "terrain_colour_footprint"] {
        let body = bindings.split(&format!("fn {name}(")).nth(1).unwrap_or_else(|| panic!("{name}")).split("\n}\n").next().unwrap();
        assert!(!body.contains("textureSample") && !body.contains("dpdx") && !body.contains("fwidth"), "{name}: no implicit derivatives, no sampler");
    }
    assert!(bindings.contains("levels = terrain_colour_levels(resolution, terrain_colour_lod(resolution, inputs));"));
    // #2661 review: the footprint is checked wherever the colour cache can matter, i.e. not
    // where valid detail nearer than DETAIL_NEAR serves the fragment alone (the distance
    // mix is exactly 1 there).
    assert!(bindings.contains("let detail_alone = detail_valid && inputs.distance < DETAIL_NEAR;"));
    assert!(bindings.contains("if colour_quadrant && !detail_alone {"));
    assert!(bindings.contains("colour_valid = terrain_colour_footprint(instance_index, slot, uv, resolution, levels);"));
    assert!(bindings.contains("let near = 1.0 - smoothstep(DETAIL_NEAR, DETAIL_FAR, inputs.distance);"));
    assert!(bindings.contains("if detail_valid && colour_valid { r.detail = near; } else if detail_valid { r.detail = 1.0; weight *= near; }"));
    assert!(bindings.contains("let footprint = max(length(inputs.ddx), length(inputs.ddy)) * f32(resolution) / CELL_SIZE;")
        && bindings.contains("return clamp(log2(max(footprint, 1e-6)) + inputs.bias, 0.0, f32(terrain_last_level(resolution)));"));
    let taps = bindings.split("fn terrain_colour_taps(").nth(1).unwrap().split("\n}\n").next().unwrap();
    for line in [
        "let p = uv * f32(3 * size) - vec2(0.5);",
        "var weights = vec4((1.0 - f.x) * (1.0 - f.y), f.x * (1.0 - f.y), (1.0 - f.x) * f.y, f.x * f.y);",
        "if !valid { weights[i] = 0.0; }",
        "return TerrainColourTaps(first, weights / max(total, 1e-6), has_own && own_valid && total > 0.0);",
    ] { assert!(taps.contains(line), "{line}"); }
    assert!(!taps.contains("clamp("), "the footprint is not clamped into the cell");
    assert!(bindings.contains("if t.weights[i] > 0.0 { colour += t.weights[i] * terrain_colour_load(instance_index, t.first + vec2(i % 2, i / 2), level); }"));
    assert!(bindings.contains("return l.share <= 0.0 || l.high == l.low || terrain_colour_level_valid(instance_index, slot, uv, resolution, l.high);"));
}

/// [design] Round 10b: the colour atlas's mip filter (terrain_surface.rs `MipPipeline`)
/// parses and validates with naga, has the bindings the pipeline's layout gives it (a
/// sampled level, a write-only RGBA16F storage level, the cells' uniform) and averages the
/// 2 by 2 texels under each texel of the cells it is given, nothing else.
#[test]
fn colour_mip_filter_validates_and_reads_only_its_cell() {
    let source = crate::terrain_surface::MIP_SHADER;
    let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
    let entry = module.entry_points.iter().find(|e| e.name == "downsample").expect("the entry point");
    assert_eq!(entry.stage, naga::ShaderStage::Compute);
    assert_eq!(entry.workgroup_size, [crate::terrain_surface::MIP_WORKGROUP, crate::terrain_surface::MIP_WORKGROUP, 1]);
    let mut bindings: Vec<_> = module.global_variables.iter().filter_map(|(_, v)| v.binding.as_ref().map(|b| (b.group, b.binding, module.types[v.ty].inner.clone()))).collect();
    bindings.sort_by_key(|b| b.1);
    assert_eq!(bindings.iter().map(|b| (b.0, b.1)).collect::<Vec<_>>(), vec![(0, 0), (0, 1), (0, 2)]);
    assert!(matches!(bindings[0].2, naga::TypeInner::Image { class: naga::ImageClass::Sampled { .. }, .. }));
    assert!(matches!(bindings[1].2, naga::TypeInner::Image { class: naga::ImageClass::Storage { format: naga::StorageFormat::Rgba16Float, access }, .. } if access == naga::StorageAccess::STORE));
    for line in [
        "let side = textureDimensions(destination).x / 3u;",
        "let texel = cells.slots[id.z].xy * side + id.xy;",
        "let below = 2u * texel;",
        "textureStore(destination, texel, sum * 0.25);",
    ] {
        assert!(source.contains(line), "{line}");
    }
}

/// [design] Round 10a: the shader's barycentric interpolation and record layout transcribe
/// terrain.rs's mesh triangles and nif_surface_blend.rs's packing.
#[test]
fn shader_detail_transcribes_the_mesh_triangles_and_the_record_layout() {
    let bindings = include_str!("../../nvf-assets/src/nif_surface_bindings.wesl");
    for line in [
        // Below the diagonal (SW, SE, NE), above it (SW, NE, NW), as quadrant_mesh's indices.
        "var vertices = vec3(sw, sw + 1u, sw + side + 1u);",
        "var weights = vec3(1.0 - f.x, f.x - f.y, f.y);",
        "vertices = vec3(sw, sw + side + 1u, sw + side);",
        "weights = vec3(1.0 - f.y, f.x, f.y - f.x);",
        // Records: colour (unorm), normal and tangent (snorm), then opacities from value 9.
        "m.tint += t.weights[i] * vec3(detail_unorm(w0), detail_unorm(w0 >> 16u), detail_unorm(w1));",
        "m.normal += t.weights[i] * vec3(detail_snorm(w1 >> 16u), detail_snorm(w2), detail_snorm(w2 >> 16u));",
        "m.tangent += t.weights[i] * vec3(detail_snorm(w3), detail_snorm(w3 >> 16u), detail_snorm(w4));",
        "let h = 9u + k;",
        // Ordered opacities (the diffuse's alpha has no part, #3789), folded overlays tinted by
        // the base material's colour, overflow ones by white.
        "let o = saturate(detail_opacity(instance_index, t, k));",
        "let share = o;",
        "colour = mix(colour, texel.rgb * a.tint * select(vec3(1.0), base_rgb, k < folded), share);",
        "normal = mix(normal, layer_n, share);",
        "return TerrainDetail(colour, normalize(normal));",
        // Table words.
        "let total = min((flags >> 12u) & 15u, DETAIL_MAX_OVERLAYS);",
        "let folded = min((flags >> 8u) & 15u, total);",
        "let pair = terrain_detail_word(instance_index, q + 15u + k);",
        "let handedness = bitcast<f32>(terrain_detail_word(instance_index, q + 27u));",
        "let local = select(place, place - vec2<f32>(cell) * CELL_SIZE, (flags & 2u) != 0u);",
        // Bevy's helpers, the interpolated (not renormalized) normal and tangent.
        "let tbn = calculate_tbn_mikktspace(world_n, vec4(game_to_world_normal(a.tangent), handedness));",
        "@if(NIF_SURFACE_BLEND) palette: array<u32, 400>, // 54-453",
    ] {
        assert!(bindings.contains(line), "{line}");
    }
    use nvf_assets::nif_surface_blend::*;
    // #2149/#2661: the detail rows follow the heights of every layout.
    for layout in [HeightLayout::STORED, HeightLayout::new(2), HeightLayout::new(4)] {
        let defs = blend_shader_defs(layout);
        let uint = |name: &str| defs.iter().find_map(|d| match d { ShaderDefVal::UInt(n, v) if n == name => Some(*v), _ => None }).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(uint("MESH_TERRAIN_DETAIL_ROW") as usize, layout.rows());
        assert_eq!(uint("MESH_TERRAIN_HEIGHT_WIDTH") as usize, layout.width());
    }
    let defs = blend_shader_defs(HeightLayout::STORED);
    let uint = |name: &str| defs.iter().find_map(|d| match d { ShaderDefVal::UInt(n, v) if n == name => Some(*v), _ => None }).unwrap_or_else(|| panic!("{name}"));
    assert_eq!(uint("MESH_TERRAIN_DETAIL_VERTEX_WORDS") as usize, DETAIL_VERTEX_WORDS);
    assert_eq!(uint("MESH_TERRAIN_DETAIL_QUADRANT_WORDS") as usize, DETAIL_QUADRANT_WORDS);
    assert_eq!(uint("MESH_TERRAIN_PALETTE_IMAGES") as usize, PALETTE_IMAGES);
    assert_eq!((uint("MESH_TERRAIN_DETAIL_NEAR"), uint("MESH_TERRAIN_DETAIL_FAR")), (2048, 4096));
    assert_eq!(uint("MESH_TERRAIN_FLAG_TWO_COMPONENT"), bevy::pbr::StandardMaterialFlags::TWO_COMPONENT_NORMAL_MAP.bits());
    assert_eq!(uint("MESH_TERRAIN_FLAG_FLIP_Y"), bevy::pbr::StandardMaterialFlags::FLIP_NORMAL_MAP_Y.bits());
}

/// [design] The WESL weight transcribes nif_surface_blend.rs::contact_weight and v2_band:
/// stable shape numbers with hybrid v1 appended as 3, the widths read from the enabled
/// uniform as (flat, band, steep), and the shared cosine definitions.
#[test]
fn shader_weight_matches_the_cpu_statement_of_hybrid_v2() {
    let bindings = include_str!("../../nvf-assets/src/nif_surface_bindings.wesl").replace("\r\n", "\n");
    for line in [
        "let c = clamp(abs(cos_angle), 0.0, 1.0);",
        "let shallow = 1.0 - smoothstep(V2_COS_PARALLEL, 1.0, c);",
        "let steep = 1.0 - smoothstep(V2_COS_STEEP, COS_FULL, c);",
        "return widths.x + (widths.y - widths.x) * shallow + (widths.z - widths.y) * steep;",
        "if shape == 0u { return height; }",
        "if shape == 1u { return penetration; }",
        "if shape == 3u {",
        "return max(1.0 - smoothstep(0.0, band, max(d, 0.0)), penetration);",
        "let widths = vec3(s.mesh_terrain_widths.x, s.mesh_terrain.x, s.mesh_terrain_widths.y);",
        "@if(NIF_SURFACE_BLEND) mesh_terrain_widths: vec4<f32>,",
        // #2661 review: the steepness fade (nif_surface_blend.rs `face_up`, `steepness_fade`)
        // multiplies every shape's weight.
        "let face = cross(dx, dy);",
        "if face_length > 1e-6 * length(dx) * length(dy) { return min(abs(face.y) / face_length, 1.0); }",
        "if n_length > 1e-6 { return min(abs(mesh_normal.y) / n_length, 1.0); }",
        "return smoothstep(STEEP_COS_NONE, STEEP_COS_FULL, clamp(up, 0.0, 1.0));",
    ] {
        assert!(bindings.contains(line), "{line}");
    }
    use nvf_assets::nif_surface_blend::ContactShape;
    assert_eq!([ContactShape::Height, ContactShape::Penetration, ContactShape::Hybrid, ContactShape::HybridV1].map(|s| s.number()), [0.0, 1.0, 2.0, 3.0]);
    let defs = nvf_assets::nif_surface_blend::blend_shader_defs(nvf_assets::nif_surface_blend::HeightLayout::STORED);
    for name in ["MESH_TERRAIN_V2_COS_PARALLEL_E6", "MESH_TERRAIN_V2_COS_STEEP_E6", "MESH_TERRAIN_COS_FULL_E6", "MESH_TERRAIN_STEEP_COS_FULL_E6", "MESH_TERRAIN_STEEP_COS_NONE_E6"] {
        assert!(defs.iter().any(|d| matches!(d, ShaderDefVal::UInt(n, _) if n == name)), "{name}");
        assert!(bindings.contains(&format!("constants::{name}")), "{name} reaches the shader");
    }
}

/// [design] #2661 review: the lookup computes the contact weight from the three height loads
/// first and, in the surface view only, stops where it is exactly 0, before every colour,
/// detail, residency and footprint read; the diagnostic views run the whole lookup.
#[test]
fn shader_lookup_stops_at_zero_contact_outside_the_diagnostic_views() {
    let bindings = include_str!("../../nvf-assets/src/nif_surface_bindings.wesl").replace("\r\n", "\n");
    let lookup = bindings.split("fn mesh_terrain_lookup(").nth(1).unwrap().split("\n}\n").next().unwrap();
    let at = |needle: &str| lookup.find(needle).unwrap_or_else(|| panic!("mesh_terrain_lookup: {needle}"));
    let stop = at("if contact == 0.0 && u32(s.mesh_terrain.y) == 0u { r.status = LOOKUP_NO_CONTACT; return r; }");
    assert!(at("let contact = mesh_terrain_weight(u32(s.mesh_terrain.z), widths, game.z - height, cos_angle) * mesh_terrain_steepness(inputs.up);") < stop);
    assert!(at("let dy = terrain_height_texel(instance_index, base + vec2(2, 0));") < stop, "after the three height loads");
    assert!(at("r.status = LOOKUP_INVALID_HEIGHT; return r;") < stop, "after the height check");
    for later in ["terrain_colour_valid(", "terrain_detail_valid(", "terrain_cell_resident(", "terrain_resolution(", "terrain_colour_lod(", "terrain_colour_footprint(", "terrain_colour_mips(", "mesh_terrain_detail("] {
        assert!(at(later) > stop, "{later} after the early stop");
    }
    assert!(at("var weight = contact * smoothstep(0.0, SPACING, border);") > stop, "the final weight is the same contact weight, faded");
    assert_eq!(lookup.matches("mesh_terrain_weight(").count(), 1, "the contact weight is computed once");
    // The stop's status draws nothing: the surface view blends only on success.
    assert!(bindings.contains("@if(MESH_TERRAIN_BLEND) const LOOKUP_SUCCESS: u32 = 4u;") && bindings.contains("@if(MESH_TERRAIN_BLEND) const LOOKUP_NO_CONTACT: u32 = 5u;"));
    let material = include_str!("../../nvf-assets/src/nif_material.wesl");
    assert!(material.contains("} else if terrain.status == LOOKUP_SUCCESS && terrain.weight > 0.0 {"));
    assert!(bindings.contains("return u32(nif_surface(instance_index).mesh_terrain.y);"), "the view the stop tests is the aid's view");
}

/// [design] #2149/#2661: the lookup reads the ground from the drawn surface's squares, as
/// terrain_surface.rs's CPU twins (`ground_square`, `probe_height`, `probe_normal`) do, and
/// keeps the LAND square for the quadrant and the detail; the metadata row follows the
/// surface's squares; the definitions name the layout's dimensions.
#[test]
fn shader_ground_lookup_reads_the_surface_squares() {
    let bindings = include_str!("../../nvf-assets/src/nif_surface_bindings.wesl").replace("\r\n", "\n");
    let lookup = bindings.split("fn mesh_terrain_lookup(").nth(1).unwrap().split("\n}\n").next().unwrap();
    let at = |needle: &str| lookup.find(needle).unwrap_or_else(|| panic!("mesh_terrain_lookup: {needle}"));
    let ground = at("@if(MESH_TERRAIN_SUBDIVIDED) let ground = terrain_square(local * f32(SUB), HEIGHT_GRID);");
    assert!(at("@if(!MESH_TERRAIN_SUBDIVIDED) let ground = TerrainSquare(square, f, triangle);") > ground);
    assert!(at("let triangle = select(0, 1, f.y > f.x);") < ground, "the LAND square first");
    assert!(at("let base = slot * HEIGHT_TILE + vec2(ground.square.x * 6 + ground.triangle * 3, ground.square.y);") > ground);
    assert!(at("let height = a + ground.f.x * dx + ground.f.y * dy;") > ground);
    // The quadrant and the detail stay on the LAND grid.
    assert!(lookup.contains("let quadrant = select(0u, 1u, local.x >= f32(GRID) * 0.5) + select(0u, 2u, local.y >= f32(GRID) * 0.5);"));
    assert!(lookup.contains("terrain_triangle(entry, quadrant, square, f, triangle)"));
    let square = bindings.split("fn terrain_square(").nth(1).unwrap().split("\n}\n").next().unwrap();
    for line in [
        "let square = min(vec2<i32>(floor(local)), vec2(grid - 1));",
        "let f = local - vec2<f32>(square);",
        "return TerrainSquare(square, f, select(0, 1, f.y > f.x));",
    ] { assert!(square.contains(line), "{line}"); }
    for line in [
        "@if(MESH_TERRAIN_BLEND) const HEIGHT_GRID: i32 = GRID * SUB;",
        "@if(MESH_TERRAIN_BLEND) const HEIGHT_SPACING: f32 = SPACING / f32(SUB);",
        "return normalize(vec3(-dx / HEIGHT_SPACING, -dy / HEIGHT_SPACING, 1.0));",
        "slot * HEIGHT_TILE + vec2(1, HEIGHT_GRID)",
        "slot * HEIGHT_TILE + vec2(2, HEIGHT_GRID)",
    ] { assert!(bindings.contains(line), "{line}"); }
    assert!(!bindings.contains("vec2(1, GRID)") && !bindings.contains("vec2(2, GRID)"), "the metadata row is after the surface's squares");
    // The CPU twin at points of a square: the same square, fraction and triangle.
    use nvf_assets::nif_surface_blend::HeightLayout;
    for (sub, local, square, triangle) in [(1, Vec2::new(3.25, 7.5), UVec2::new(3, 7), 1), (4, Vec2::new(3.25, 7.5), UVec2::new(13, 30), 0), (2, Vec2::new(31.9, 0.1), UVec2::new(63, 0), 0), (4, Vec2::splat(32.0), UVec2::splat(127), 0)] {
        let (s, f, t) = crate::terrain_surface::ground_square(HeightLayout::new(sub), local);
        assert_eq!((s, t), (square, triangle), "{sub} {local:?}");
        assert!((f - (local * sub as f32 - s.as_vec2())).abs().max_element() < 1e-6);
    }
}

pub(crate) fn nif_defs(bindless: bool, blend: bool, prepass: bool, soft_edges: bool, layout: nvf_assets::nif_surface_blend::HeightLayout) -> Vec<ShaderDefVal> {
    let mut defs = vec![
        ShaderDefVal::Bool("BINDLESS".into(), bindless),
        ShaderDefVal::Bool("NIF_SURFACE_BLEND".into(), blend),
        ShaderDefVal::Bool("MESH_TERRAIN_BLEND".into(), blend && !prepass),
        ShaderDefVal::Bool("PREPASS_PIPELINE".into(), prepass),
        ShaderDefVal::Bool("SOFT_EDGES".into(), soft_edges),
        "VERTEX_POSITIONS".into(), "VERTEX_NORMALS".into(), "VERTEX_UVS_A".into(),
        "VERTEX_TANGENTS".into(), "VERTEX_COLORS".into(), "VERTEX_OUTPUT_INSTANCE_INDEX".into(), "MAY_DISCARD".into(),
        "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3".into(), "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_6".into(),
        ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 3),
        ShaderDefVal::UInt("MAX_DIRECTIONAL_LIGHTS".into(), 10),
        ShaderDefVal::UInt("MAX_CASCADES_PER_LIGHT".into(), 4),
        ShaderDefVal::UInt("MAX_RECT_LIGHTS".into(), 1),
    ];
    if blend && !prepass { defs.extend(nvf_assets::nif_surface_blend::blend_shader_defs(layout)); }
    defs
}

#[test]
fn complete_nif_forward_and_prepass_compose_with_both_interfaces() {
    // [design] Use the real WESL compiler through Bevy's CPU ShaderCache, replacing
    // only its GPU-module creation callback with naga parsing and validation.
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut modules = vendored_modules();
    let root = manifest.join("../nvf-assets/src");
    sources(&root, "nvf_assets", &root, &mut modules);
    // The detail's height blending functions (#2550) live in this crate.
    modules.push(("embedded://nvforever/terrain_weights.wesl".into(), include_str!("terrain_weights.wesl").into()));
    // #2149/#2661: the blend's forward pass at the stored layout and at the 4 x 4 surface's.
    use nvf_assets::nif_surface_blend::HeightLayout;
    for bindless in [false, true] { for blend in [false, true] { for prepass in [false, true] {
        let entry = if prepass { "embedded://nvf_assets/nif_prepass.wesl" } else { "embedded://nvf_assets/nif_material.wesl" };
        // SOFT_EDGES is a main-pass variant; the unlit diagnostics write its coverage.
        let soft: &[bool] = if prepass { &[false] } else { &[false, true] };
        let layouts: &[HeightLayout] = if blend && !prepass { &[HeightLayout::STORED, HeightLayout::new(4)] } else { &[HeightLayout::STORED] };
        for (&soft_edges, &layout) in soft.iter().flat_map(|s| layouts.iter().map(move |l| (s, l))) {
            let blended = compose(&modules, entry, &nif_defs(bindless, blend, prepass, soft_edges, layout));
            if blend && !bindless {
                // [design] Every texture the blend pipeline's shader uses is a binding of the
                // ordinary NIF pipeline's layout for the same key: the same view and mesh
                // bindings, and in the material group only StandardMaterial's and the
                // ordinary extension's slots 51 and 53 (which the ordinary shader may leave
                // unused, its layout still has them: nif_material.rs NifSurface).
                let mut ordinary = texture_bindings(&compose(&modules, entry, &nif_defs(bindless, false, prepass, soft_edges, layout)));
                ordinary.extend([(3, 51), (3, 53)]);
                let blended = texture_bindings(&blended);
                assert!(blended.is_subset(&ordinary), "prepass={prepass} soft_edges={soft_edges}: {:?}", blended.difference(&ordinary).collect::<Vec<_>>());
            }
        }
    }}}
}

/// `nif_material.wesl` without the authored materials' debug code (#2981, nvf-assets
/// authored.rs `PbrView`, `metallic_debug`): every line (or block) under a positive
/// `@if(NVF_PBR_...)` condition removed, and the `@if(!NVF_PBR_METALLIC)` guard taken off
/// the line it guards. That is the shader as it was before those switches.
fn without_pbr_debug(source: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    let braces = |l: &str| l.matches('{').count() as i32 - l.matches('}').count() as i32;
    for line in source.lines() {
        let t = line.trim_start();
        if depth > 0 {
            depth += braces(line);
            continue;
        }
        if t.starts_with("@if(NVF_PBR_") {
            depth = braces(line).max(0);
            continue;
        }
        match t.strip_prefix("@if(!NVF_PBR_METALLIC) ") {
            Some(rest) => out.push_str(&format!("{}{rest}", &line[..line.len() - t.len()])),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// A composed module's module-scope declarations, each kept whole and verbatim (its
/// attribute lines, its body with every statement in place), then sorted: equal for two
/// modules that declare the same things in any order. A declaration runs from its first line
/// at brace depth 0 (attribute lines included) to the line that brings the depth back to 0
/// ending in `}` or `;`; blank lines at depth 0 separate nothing and are dropped, blank lines
/// inside a body stay in it. The composed WGSL has no comments or strings.
pub(crate) fn declarations(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut depth = 0i32;
    for line in source.lines() {
        let t = line.trim();
        if depth == 0 && current.is_empty() && t.is_empty() {
            continue;
        }
        current.push(line);
        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
        assert!(depth >= 0, "unbalanced braces at {line:?}");
        if depth == 0 && (t.ends_with('}') || t.ends_with(';')) {
            out.push(current.join("\n"));
            current.clear();
        }
    }
    assert!(current.iter().all(|l| l.trim().is_empty()), "an unterminated declaration: {current:?}");
    out.sort_unstable();
    out
}

/// The first of two sorted declaration lists' differences, as a message (`None`: equal).
pub(crate) fn declaration_difference(ours: &[String], theirs: &[String]) -> Option<String> {
    let i = (0..ours.len().max(theirs.len())).find(|&i| ours.get(i) != theirs.get(i))?;
    let (a, b) = (ours.get(i).map_or("", String::as_str), theirs.get(i).map_or("", String::as_str));
    let at = first_difference(a, b).unwrap_or(0);
    Some(format!("sorted declaration {i} of {}/{} differs at byte {at}:\nours:   {:?}\ntheirs: {:?}", ours.len(), theirs.len(), window(a, at), window(b, at)))
}

/// The byte where two texts first differ (`None`: equal).
fn first_difference(a: &str, b: &str) -> Option<usize> {
    if a == b {
        return None;
    }
    Some(a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len())))
}

/// About 200 bytes of `s` around byte `at`, on character boundaries.
fn window(s: &str, at: usize) -> &str {
    let mut start = at.saturating_sub(120).min(s.len());
    while !s.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (at + 120).min(s.len());
    while !s.is_char_boundary(end) {
        end += 1;
    }
    &s[start..end]
}

/// #2981: the NIF forward shader with the authored materials' debug defs.
///
/// What it proves: the authored debug views and parallax variants compose and validate,
/// bindless or not, ordinary or blend, without UVs and with UVs and tangents (with or
/// without a normal map). The mask switch changes the shader only when UVs are present.
/// With none of the debug defs, the composed shader declares exactly what the same
/// source declares with its debug lines stripped (`without_pbr_debug`), declaration by
/// declaration, each verbatim, in any order.
///
/// What it does not prove: that the off shader equals the shader before stage 3. The stripped
/// reference is made from the same file, so an unconditional edit to nif_material.wesl (or to
/// a module it imports, such as the vendored two-channel clamp) is in both and passes. It
/// executes no fragment.
#[test]
fn nif_forward_composes_with_the_authored_debug_defs_and_is_unchanged_without_them() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut modules = vendored_modules();
    let root = manifest.join("../nvf-assets/src");
    sources(&root, "nvf_assets", &root, &mut modules);
    modules.push(("embedded://nvforever/terrain_weights.wesl".into(), include_str!("terrain_weights.wesl").into()));
    let entry = "embedded://nvf_assets/nif_material.wesl";
    let original = modules.iter().find(|(p, _)| p == entry).map(|(_, s)| s.clone()).expect("nif_material.wesl");
    let stripped = without_pbr_debug(&original);
    assert_ne!(stripped, original, "the debug code is there to strip");
    assert!(!stripped.lines().any(|l| l.trim_start().starts_with("@if(NVF_PBR_") || l.contains("@if(!NVF_PBR_")), "every debug line stripped");
    let mut before = modules.clone();
    before.iter_mut().find(|(p, _)| p == entry).unwrap().1 = stripped;
    use nvf_assets::nif_surface_blend::HeightLayout;
    for bindless in [false, true] {
        for blend in [false, true] {
            for (uvs, normal_map) in [(false, false), (true, false), (true, true)] {
                let mut base = nif_defs(bindless, blend, false, false, HeightLayout::STORED);
                if uvs {
                    base.push("VERTEX_UVS".into());
                } else {
                    base.retain(|d| *d != ShaderDefVal::from("VERTEX_UVS_A"));
                }
                if normal_map {
                    base.push("STANDARD_MATERIAL_NORMAL_MAP".into());
                }
                let key = format!("bindless={bindless} blend={blend} uvs={uvs} normal_map={normal_map}");
                let composed = compose(&modules, entry, &base);
                let off = declarations(&composed);
                if let Some(d) = declaration_difference(&off, &declarations(&compose(&before, entry, &base))) {
                    panic!("{key}: off is not the shader without the debug code; {d}");
                }
                let mut sets: Vec<&[&str]> = nvf_assets::authored::PbrView::ALL.iter().map(|v| v.shader_defs()).collect();
                sets.push(&["NVF_PBR_METALLIC"]);
                sets.push(&["NVF_PBR_VIEW", "NVF_PBR_VIEW_OCCLUSION", "NVF_PBR_METALLIC"]);
                for set in sets {
                    let mut defs = base.clone();
                    defs.extend(set.iter().map(|d| ShaderDefVal::from(*d)));
                    assert!(declaration_difference(&declarations(&compose(&modules, entry, &defs)), &off).is_some(), "{key} {set:?}: the defs change no declaration");
                }
                // The march needs coordinates and tangents, independently of a normal map.
                assert_eq!(composed.contains("1.0 - clamp(height, 0.0, 1.0)"), uvs, "{key}: the march inverts the stored height, held to 0..1");
                if !bindless {
                    assert_eq!(texture_bindings(&composed).contains(&(3, 11)), uvs, "{key}: the march reads the depth map");
                }
                for extra in [&[][..], &["NVF_PBR_VIEW", "NVF_PBR_VIEW_HEIGHT"], &["RELIEF_MAPPING"], &["RELIEF_MAPPING", "NVF_PBR_VIEW", "NVF_PBR_VIEW_HEIGHT"]] {
                    let mut defs = base.clone();
                    defs.extend(extra.iter().map(|d| ShaderDefVal::from(*d)));
                    let without_mask = declarations(&compose(&modules, entry, &defs));
                    if extra.len() == 1 && extra.contains(&"RELIEF_MAPPING") {
                        assert_eq!(declaration_difference(&without_mask, &off).is_some(), uvs, "{key}: the relief key changes the march");
                    }
                    // Isolate the mask's contribution from the height view or relief key.
                    defs.push(nvf_assets::nif_material::PARALLAX_MASK_DEF.into());
                    let masked = compose(&modules, entry, &defs);
                    assert_eq!(masked.contains("unshifted_alpha"), uvs, "{key} {extra:?}: the mask reads unshifted alpha where UVs exist");
                    assert_eq!(declaration_difference(&declarations(&masked), &without_mask).is_some(), uvs, "{key} {extra:?}: the mask changes the shader only with UVs");
                }
            }
        }
    }
}

/// The splitter keeps a function body whole (its blank lines and statement order included)
/// and attribute lines with what they qualify.
#[test]
fn declarations_are_split_whole_and_compared_in_any_order() {
    let a = "@group(0) @binding(0)\nvar<uniform> v: f32;\n\nfn f() -> f32 {\n    let x = 1.0;\n\n    let y = 2.0;\n    return x + y;\n}\n\nstruct S {\n    a: f32,\n}\n";
    let b = "struct S {\n    a: f32,\n}\n\nfn f() -> f32 {\n    let x = 1.0;\n\n    let y = 2.0;\n    return x + y;\n}\n@group(0) @binding(0)\nvar<uniform> v: f32;\n";
    let (da, db) = (declarations(a), declarations(b));
    assert_eq!(da.len(), 3);
    assert!(declaration_difference(&da, &db).is_none(), "the same declarations in another order");
    assert!(da.iter().any(|d| d.starts_with("@group(0) @binding(0)") && d.ends_with("v: f32;")));
    // Two statements of one body swapped: a different declaration.
    let swapped = a.replace("    let x = 1.0;\n\n    let y = 2.0;", "    let y = 2.0;\n\n    let x = 1.0;");
    assert!(declaration_difference(&declarations(&swapped), &da).is_some());
    // Another binding: different.
    assert!(declaration_difference(&declarations(&a.replace("@binding(0)", "@binding(1)")), &da).is_some());
}

#[test]
fn terrain_surface_bake_composes_for_the_folded_base() {
    // [design] terrain.wesl with the ordinary and folded-bake variants (terrain.rs
    // TerrainLayersKey), through the same CPU ShaderCache and naga validation.
    // #2550/#2551: the bake copy of a folded base keeps the near draw's height blending and
    // anti-tiling (terrain.rs `TerrainLayersKey`), so terrain.wesl composes with them too.
    let modules = terrain_modules();
    for bindless in [false, true] { for bake in [false, true] { for (height, anti) in [(false, false), (true, false), (false, true), (true, true)] {
        compose(&modules, "embedded://nvforever/terrain.wesl", &terrain_defs(bindless, bake, height, anti, false));
    }}}
    assert!(!include_str!("terrain.wesl").contains("SURFACE_BAKE_LAYER"), "layer copies use the single-texture capture");
    // The bake's return comes after every composition path, so it stores what they compose.
    let terrain = include_str!("terrain.wesl").replace("\r\n", "\n");
    let bake = terrain.find("@if(SURFACE_BAKE) return surface_bake(pbr_input.material.base_color.rgb);").expect("the bake's return");
    for path in ["@if(TERRAIN_HEIGHT_BLEND && !TERRAIN_ANTI_TILING) {", "@if(TERRAIN_ANTI_TILING) {\n            // One set of taps"] {
        assert!(terrain.find(path).expect(path) < bake, "{path}");
    }
}

/// The folded TerrainMaterial's main-pass defs (terrain.rs `TerrainLayersKey`) on the
/// landscape's mesh layout.
pub(crate) fn terrain_defs(bindless: bool, bake: bool, height: bool, anti: bool, parallax: bool) -> Vec<ShaderDefVal> {
    vec![
        ShaderDefVal::Bool("BINDLESS".into(), bindless),
        ShaderDefVal::Bool("SURFACE_BAKE".into(), bake),
        ShaderDefVal::Bool("TERRAIN_HEIGHT_BLEND".into(), height),
        ShaderDefVal::Bool("TERRAIN_ANTI_TILING".into(), anti),
        ShaderDefVal::Bool("TERRAIN_PARALLAX".into(), parallax),
        "VERTEX_POSITIONS".into(), "VERTEX_NORMALS".into(), "VERTEX_UVS_A".into(), "VERTEX_UVS_B".into(),
        "VERTEX_TANGENTS".into(), "VERTEX_COLORS".into(), "VERTEX_OUTPUT_INSTANCE_INDEX".into(),
        "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3".into(), "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_6".into(),
        ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 3),
        ShaderDefVal::UInt("MAX_DIRECTIONAL_LIGHTS".into(), 10),
        ShaderDefVal::UInt("MAX_CASCADES_PER_LIGHT".into(), 4),
        ShaderDefVal::UInt("MAX_RECT_LIGHTS".into(), 1),
    ]
}

fn terrain_modules() -> Vec<(String, String)> {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut modules = vendored_modules();
    for name in ["terrain", "terrain_surface", "terrain_weights"] {
        modules.push((format!("embedded://nvforever/{name}.wesl"), std::fs::read_to_string(manifest.join("src").join(format!("{name}.wesl"))).unwrap()));
    }
    let root = manifest.join("../nvf-assets/src");
    sources(&root, "nvf_assets", &root, &mut modules);
    modules
}

#[test]
fn terrain_parallax_composes_with_every_folded_variant_and_is_absent_without_its_def() {
    // #3322: terrain.wesl with TERRAIN_PARALLAX beside height blending and anti-tiling, both
    // bindings, through the same CPU ShaderCache and naga validation. Without the def (the
    // switch off, or a material without a height map: terrain.rs `parallax_def`) the composed
    // shader reads no height map and holds no march: the pipeline there always was.
    let modules = terrain_modules();
    let entry = "embedded://nvforever/terrain.wesl";
    let heights: std::collections::BTreeSet<(u32, u32)> = (64..71).map(|b| (3, b)).collect();
    for bindless in [false, true] { for (height, anti) in [(false, false), (true, false), (false, true), (true, true)] {
        let off = compose(&modules, entry, &terrain_defs(bindless, false, height, anti, false));
        let on = compose(&modules, entry, &terrain_defs(bindless, false, height, anti, true));
        assert!(!off.contains("parallax_shift") && !off.contains("layer_height"), "bindless={bindless} height={height} anti={anti}: no march without the def");
        assert!(on.contains("parallax_shift") && on.contains("layer_height"), "bindless={bindless} height={height} anti={anti}: the march with it");
        assert!(declaration_difference(&declarations(&on), &declarations(&off)).is_some());
        if !bindless {
            // Without bindless every texture is its own binding: the height maps are read
            // only with the def (the folded type is used only with bindless; this checks
            // the reads, terrain.rs `TerrainMaterials::folds`).
            let (t_off, t_on) = (texture_bindings(&off), texture_bindings(&on));
            assert!(t_off.is_disjoint(&heights), "{t_off:?}: no height map without the def");
            assert!(heights.is_subset(&t_on), "{t_on:?}: every layer's height map, and the base's, with it");
        }
        // A quadrant of its base alone (#3846): the base mesh has no second coordinate.
        for (bake, parallax) in [(false, false), (false, true), (true, false)] {
            let defs: Vec<ShaderDefVal> = terrain_defs(bindless, bake, height, anti, parallax).into_iter().filter(|d| *d != ShaderDefVal::from("VERTEX_UVS_B")).collect();
            compose(&modules, entry, &defs);
        }
    }}
    // A bake copy never takes the def (terrain.rs `parallax_def`), but the variant composes.
    compose(&modules, entry, &terrain_defs(true, true, false, false, true));
}

fn capture_source(colours: bool, uvs: bool) -> String {
    capture_source_with(colours, uvs, false)
}

fn capture_source_with(colours: bool, uvs: bool, anti_tiling: bool) -> String {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut modules = vendored_modules();
    modules.push(("embedded://nvforever/terrain_capture.wesl".into(), std::fs::read_to_string(manifest.join("src/terrain_capture.wesl")).unwrap()));
    modules.push(("embedded://nvf_assets/terrain_sampling.wesl".into(), nvf_assets::terrain_sampling::SAMPLING_SOURCE.into()));
    let defs = vec![
        ShaderDefVal::Bool("TERRAIN_ANTI_TILING".into(), anti_tiling),
        ShaderDefVal::Bool("VERTEX_COLORS".into(), colours), ShaderDefVal::Bool("VERTEX_UVS_A".into(), uvs),
        "VERTEX_POSITIONS".into(), "VERTEX_NORMALS".into(), "VERTEX_TANGENTS".into(),
        ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 3),
    ];
    compose(&modules, "embedded://nvforever/terrain_capture.wesl", &defs)
}

#[test]
fn single_texture_capture_composes_for_base_and_layer_meshes() {
    // [design] The landscape's base and layer meshes carry colours and coordinates
    // (terrain.rs quadrant_mesh); the variants without them must still compose.
    for colours in [false, true] { for uvs in [false, true] {
        let source = capture_source(colours, uvs);
        let textures = texture_bindings(&source);
        assert!(textures.iter().all(|&(group, binding)| group == 3 && binding == 1), "{textures:?}: only the diffuse texture");
        assert_eq!(textures.len(), usize::from(uvs), "sampled where the mesh has coordinates");
    }}
    let capture = include_str!("terrain_capture.wesl");
    // Base: alpha 1; layer: material x vertex alpha (the diffuse's has no part, #3789).
    // Missing texture: white.
    assert!(capture.contains("out.color = vec4(colour.rgb, select(1.0, colour.a, surface_capture.uv_offset_flags.w > 0.5));"));
    assert!(capture.contains("colour *= vec4(select(vec3(1.0), texel.rgb, surface_capture.uv_offset_flags.z > 0.5), 1.0);"));
    assert!(capture.find("colour *= in.color;").unwrap() < capture.find("out.color =").unwrap());
    // #2551: an anti-tiled source's copy reads its one texture at the taps, at the world
    // place over the repeat (as terrain_pbr.wesl `terrain_footprint`), with or without the
    // mesh's coordinates; still only the diffuse texture.
    for colours in [false, true] { for uvs in [false, true] {
        let source = capture_source_with(colours, uvs, true);
        assert_eq!(texture_bindings(&source).into_iter().collect::<Vec<_>>(), vec![(3, 1)], "colours={colours} uvs={uvs}");
        assert!(source.contains("textureSampleGrad") && !source.contains("textureSample("), "the anti-tiled read only");
    }}
    assert!(capture.contains("let at_uv = mat2x2<f32>(surface_capture.uv_axes.xy, surface_capture.uv_axes.zw) * terrain_world_uv(in.world_position.xyz, surface_capture.anti_tiling.x) + surface_capture.uv_offset_flags.xy;"));
    assert!(capture.contains("colour *= vec4(select(vec3(1.0), at_texel.rgb, surface_capture.uv_offset_flags.z > 0.5), 1.0);"));
}

/// [design] #2661 with #2550/#2551: the detail's weighted composition (nif_surface_bindings.wesl
/// `mesh_terrain_detail`) is the one terrain.wesl's TERRAIN_HEIGHT_BLEND and
/// TERRAIN_ANTI_TILING paths make, through the same shared functions, its lines those of the
/// CPU mirror in terrain.rs (`detail_mirror`), and its words those nif_surface_blend.rs packs.
#[test]
fn shader_detail_composes_height_blending_and_anti_tiling_as_the_near_draw() {
    let bindings = include_str!("../../nvf-assets/src/nif_surface_bindings.wesl").replace("\r\n", "\n");
    let terrain = include_str!("terrain.wesl").replace("\r\n", "\n");
    assert!(bindings.contains("@if(MESH_TERRAIN_BLEND && BINDLESS) import nvforever::terrain_weights::{terrain_height, terrain_ordered_shares, terrain_height_weights};"));
    assert!(bindings.contains("@if(MESH_TERRAIN_BLEND && BINDLESS) import nvf_assets::terrain_sampling::{TerrainFootprint, TerrainTaps, terrain_hex3, terrain_normal_tap, terrain_unit_normal, terrain_world_normal};"));
    let detail = bindings.split("fn mesh_terrain_detail(").nth(1).unwrap().split("\n}\n").next().unwrap();
    for line in [
        // The switches from the table words (DetailQuadrant::words).
        "let height_blend = (flags & 16u) != 0u;",
        "let anti = (flags & 32u) != 0u;",
        "let overflow_anti = terrain_detail_word(instance_index, q + 28u);",
        "let local_reference = (flags & 64u) != 0u && !anti;",
        "let depth = bitcast<f32>(terrain_detail_word(instance_index, q + 29u));",
        "let gain = bitcast<f32>(terrain_detail_word(instance_index, q + 30u));",
        "let repeat = bitcast<f32>(terrain_detail_word(instance_index, q + 31u));",
        // The anti-tiled footprint: the world place over the repeat, the base's transform.
        "fp = TerrainFootprint(axes * (place / repeat) + offset, axes * inputs.ddx * at_scale, axes * inputs.ddy * at_scale);",
        "taps = terrain_hex3(fp.uv);",
        // terrain.wesl's walk and weights, line for line (terrain.rs `detail_mirror`).
        "if remaining == 0.0 {",
        "if o == 0.0 {",
        "w[k + 1u] = o * remaining;",
        "remaining *= 1.0 - o;",
        "paint[k] = o;",
        "w = terrain_ordered_shares(paint, n);",
        "f = terrain_height_weights(w, height, depth);",
        "normal = select(plain, normalize(normal_sum), dot(normal_sum, normal_sum) > 1e-12);",
        // The overlays left after the base draw: all without the switches, else overflow.
        "for (var k = select(0u, folded, weighted); k < total; k++) {",
        "let anti_k = ((overflow_anti >> k) & 1u) != 0u;",
    ] {
        assert!(detail.contains(line), "{line}");
    }
    for line in ["if remaining == 0.0 {", "if o == 0.0 {", "w[k + 1u] = o * remaining;", "remaining *= 1.0 - o;", "f = terrain_height_weights(w, height, anti_params.x);", "pbr_input.N = select(plain, normalize(normal_sum), dot(normal_sum, normal_sum) > 1e-12);"] {
        assert!(terrain.contains(line), "terrain.wesl still: {line}");
    }
    // The height reference is placed as terrain.wesl's `height_reference_at` places it.
    let reference = bindings.split("fn detail_height_reference(").nth(1).unwrap().split("\n}\n").next().unwrap();
    let near = terrain.split("fn height_reference_at(").nth(1).unwrap().split("\n}\n").next().unwrap();
    for line in [
        "var lod = last_mip;",
        "var coord = vec2(0.5);",
        "let gx = dx * fsize;",
        "let gy = dy * fsize;",
        "lod = min(last_mip, max(4.0, current_lod + 2.0));",
        "coord = uv;",
    ] {
        assert!(reference.contains(line) && near.contains(line), "{line}");
    }
    assert!(reference.contains("let current_lod = 0.5 * log2(max(max(dot(gx, gx), dot(gy, gy)), 1e-12)) + bias;")
        && near.contains("let current_lod = 0.5 * log2(max(max(dot(gx, gx), dot(gy, gy)), 1e-12)) + view.mip_bias;"));
    // The anti-tiled reads follow terrain_sampling.wesl's tap rule.
    for name in ["detail_sample_taps", "detail_normal_taps"] {
        let body = bindings.split(&format!("fn {name}(")).nth(1).unwrap().split("\n}\n").next().unwrap();
        assert!(body.contains("if tap.weight > 0.0 {") && body.contains("tap.uv, tap.rotation * fp.dx, tap.rotation * fp.dy)"), "{name}");
        assert!(!body.contains("textureSample(") && !body.contains("dpdx"), "{name}: explicit gradients only");
    }
    assert!(bindings.contains("sum += tap.weight * terrain_normal_tap(encoded, two_component, flip_y, tap.rotation);"));
    assert!(bindings.contains("return terrain_world_normal(tbn, terrain_unit_normal(sum), false);"));
}


/// [design] The 16-texture stage limit of a GPU without bindless textures, against the
/// material groups the aid creates (from the production types' resource tables, where
/// every texture becomes a binding without bindless, and the composed capture shader).
/// The view and mesh groups are the mesh pipeline's, as for every ordinary material:
/// the blend type has the ordinary NIF type's count less #2462's two (also per composed group in
/// `complete_nif_forward_and_prepass_compose_with_both_interfaces`); the capture binds
/// one texture; the folded TerrainMaterial's group alone exceeds the limit, so it is used
/// only where TerrainMaterials::folds found bindless textures, and every other draw is
/// captured with the single-texture material (terrain_surface.rs tests).
#[test]
fn material_groups_fit_a_16_texture_stage_without_bindless() {
    use bevy::render::render_resource::{AsBindGroup, BindlessDescriptor, BindlessResourceType as R};
    const STAGE_LIMIT: usize = 16;
    let textures = |d: Option<BindlessDescriptor>| d.unwrap().resources.iter().filter(|r| matches!(r, R::Texture1d | R::Texture2d | R::Texture2dArray | R::Texture3d | R::TextureCube | R::TextureCubeArray)).count();
    let standard = textures(StandardMaterial::bindless_descriptor());
    let ordinary = standard + textures(nvf_assets::nif_material::NifSurface::bindless_descriptor());
    // [design] Without bindless the blend type's layout is its colour interface's
    // (nif_surface_blend.rs: round 10a's palette exists only in the bindless table).
    let blend = standard + textures(nvf_assets::nif_surface_blend::NifSurfaceBlendColour::bindless_descriptor());
    let folded = standard + textures(crate::terrain::TerrainLayers::bindless_descriptor());
    assert_eq!(blend + 2, ordinary, "the blend adds no texture to the ordinary NIF material group; it lacks the two of the window environment map (#2462: the cube and the mask)");
    assert!(ordinary <= STAGE_LIMIT);
    assert!(folded > STAGE_LIMIT, "{folded}: the folded material needs bindless textures");
    let capture = texture_bindings(&capture_source(true, true)).len();
    assert_eq!(capture, 1);
    assert!(capture < ordinary, "the capture's group is smaller than any ordinary NIF shape's");
}
