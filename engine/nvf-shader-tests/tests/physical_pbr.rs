//! Physical PBR lighting (the Physical lighting row): `pbr_lighting.wesl` with OpenPBR
//! `Fd_OrenNayar` EON diffuse, reciprocal dielectric coupling, desktop 0.045 roughness
//! floor, physical inverse-square attenuation, `pbr_functions.wesl` with procedural
//! `ENVIRONMENT_MAP` IBL and Frostbite specular AO, and `fog.wesl` with linear
//! Beer-Lambert extinction/inscattering, validated by Naga across all world shaders.

mod support;
use support::{Compiled, Compiler};

/// Whether the composed module declares group 0, binding `binding`.
fn binds(compiled: &Compiled, binding: u32) -> bool {
    compiled
        .module
        .global_variables
        .iter()
        .any(|(_, g)| g.binding.as_ref().is_some_and(|b| b.group == 0 && b.binding == binding))
}

fn has_fn(compiled: &Compiled, needle: &str) -> bool {
    compiled
        .module
        .functions
        .iter()
        .any(|(_, f)| f.name.as_deref().is_some_and(|n| n.contains(needle)))
}

#[test]
fn every_world_material_compiles_with_physical_pbr() {
    let mut compiler = Compiler::with_physical_pbr();
    for (shader, extra) in [
        ("nvf_assets/nif_material.wesl", &["NVF_PHYSICAL_PBR"][..]),
        ("nvf_assets/nif_material.wesl", &["NVF_PHYSICAL_PBR", "MAY_DISCARD"][..]),
        ("nvf_assets/nif_material.wesl", &["NVF_PHYSICAL_PBR", "FIRST_PERSON_DEPTH"][..]),
        (
            "nvf_assets/nif_material.wesl",
            &["NVF_PHYSICAL_PBR", "FIRST_PERSON_DEPTH", "SHADE_LIGHT"][..],
        ),
        ("nvforever/terrain.wesl", &["NVF_PHYSICAL_PBR"][..]),
        ("nvforever/grass.wesl", &[][..]),
        // Canopy shading (part 3b of #3828's split).
        ("nvforever/grass.wesl", &["NVF_PHYSICAL_PBR", "NVF_CANOPY", "VERTEX_UVS_B", "VERTEX_COLORS"][..]),
        ("nvforever/tree.wesl", &["TREE_LEAVES"][..]),
        ("nvforever/tree.wesl", &["TREE_LEAVES", "NVF_PHYSICAL_PBR", "NVF_CANOPY", "VERTEX_UVS_B", "VERTEX_COLORS"][..]),
        ("nvforever/tree.wesl", &["TREE_LEAVES", "NVF_CROWN_SHADOW"][..]),
        ("bevy_pbr/render/pbr.wesl", &[][..]),
    ] {
        for env_map in [false, true] {
            // The ambient occlusion's hook into the physical light loop comes with #3828's
            // split part 2 (#4287), which makes this a loop over `[false, true]` again.
            #[allow(clippy::single_element_loop)]
            for on in [false] {
                let mut defs = extra.to_vec();
                defs.push("DISTANCE_FOG");
                if env_map {
                    defs.push("ENVIRONMENT_MAP");
                }
                if on {
                    defs.push("NVF_AMBIENT_OCCLUSION");
                }
                let compiled = compiler.compile(shader, &defs);
                assert_eq!(
                    binds(&compiled, 43),
                    on,
                    "{shader} {defs:?}: the uniform at binding 43"
                );
                let first_person = defs.contains(&"FIRST_PERSON_DEPTH");
                assert_eq!(
                    binds(&compiled, 44),
                    on && !first_person,
                    "{shader} {defs:?}: the texture at binding 44"
                );
                assert!(
                    has_fn(&compiled, "Fd_OrenNayar"),
                    "{shader} {defs:?}: Fd_OrenNayar present in Naga module"
                );
                assert!(
                    has_fn(&compiled, "nvf_dielectric_E"),
                    "{shader} {defs:?}: nvf_dielectric_E present in Naga module"
                );
                assert_eq!(
                    has_fn(&compiled, "nvf_specular_ao"),
                    env_map && on,
                    "{shader} {defs:?}: nvf_specular_ao present iff ENVIRONMENT_MAP && NVF_AMBIENT_OCCLUSION"
                );
                assert_eq!(
                    has_fn(&compiled, "nvf_physical_occlusion"),
                    on && !first_person,
                    "{shader} {defs:?}: nvf_physical_occlusion present iff NVF_AMBIENT_OCCLUSION && !FIRST_PERSON_DEPTH"
                );
                assert!(
                    compiled
                        .module
                        .entry_points
                        .iter()
                        .any(|e| e.stage == naga::ShaderStage::Fragment),
                    "{shader} {defs:?}: a fragment entry point"
                );
            }
        }
    }
}

#[test]
fn sky_shader_validates_with_and_without_premultiply_alpha() {
    let mut compiler = Compiler::with_physical_pbr();
    for defs in [&[][..], &["PREMULTIPLY_ALPHA", "BLEND_PREMULTIPLIED_ALPHA"][..]] {
        let compiled = compiler.compile("nvforever/sky.wesl", defs);
        assert!(
            has_fn(&compiled, "tony_inverse_rgb"),
            "nvforever/sky.wesl {defs:?}: tony_inverse_rgb present in Naga module"
        );
        assert!(
            has_fn(&compiled, "eval_physical_sky"),
            "nvforever/sky.wesl {defs:?}: eval_physical_sky present in Naga module"
        );
        assert!(
            compiled
                .module
                .entry_points
                .iter()
                .any(|e| e.stage == naga::ShaderStage::Fragment),
            "nvforever/sky.wesl {defs:?}: a fragment entry point"
        );
    }
}

#[test]
fn imagespace_shader_validates_under_physical_pbr() {
    let mut compiler = Compiler::with_physical_pbr();
    compiler.register(
        "bevy_core_pipeline/fullscreen_vertex_shader.wesl",
        "struct FullscreenVertexOutput {\n    @builtin(position) position: vec4<f32>,\n    @location(0) uv: vec2<f32>,\n};\n".into(),
    );
    let compiled = compiler.compile("nvforever/imagespace.wesl", &[]);
    assert!(binds(&compiled, 0) && binds(&compiled, 2), "screen and grade bindings present");
    assert!(
        compiled
            .module
            .entry_points
            .iter()
            .any(|e| e.stage == naga::ShaderStage::Fragment),
        "nvforever/imagespace.wesl: a fragment entry point"
    );
}

#[test]
fn blend_multiply_decals_attenuate_with_fog_transmittance() {
    for physical_pbr in [false, true] {
        let mut compiler = if physical_pbr {
            Compiler::with_physical_pbr()
        } else {
            Compiler::with_ambient_occlusion_hook()
        };
        for defs in [
            &["BLEND_MULTIPLY"][..],
            &["BLEND_MULTIPLY", "DISTANCE_FOG"][..],
        ] {
            let mut full_defs = defs.to_vec();
            if physical_pbr {
                full_defs.push("NVF_PHYSICAL_PBR");
            }
            let compiled = compiler.compile("nvf_assets/nif_material.wesl", &full_defs);
            assert!(
                compiled
                    .module
                    .entry_points
                    .iter()
                    .any(|e| e.stage == naga::ShaderStage::Fragment),
                "physical={physical_pbr} {full_defs:?}: fragment validates"
            );
        }
    }
}

/// Physical lighting with authored reflections: the gated light loop compiles for an authored
/// material (NVF_PBR_REFLECT, the cube) and any other (the flat ambient), with and without
/// an environment map in the view.
#[test]
fn the_gated_physical_light_loop_compiles_with_and_without_the_opt_in() {
    let mut compiler = support::Compiler::with_physical_pbr_and_gate();
    for reflect in [false, true] {
        for env_map in [false, true] {
            let mut defs = vec!["NVF_PHYSICAL_PBR", "DISTANCE_FOG"];
            if env_map {
                defs.push("ENVIRONMENT_MAP");
            }
            if reflect {
                defs.push("NVF_PBR_REFLECT");
            }
            let compiled = compiler.compile("nvf_assets/nif_material.wesl", &defs);
            assert!(
                compiled.module.entry_points.iter().any(|e| e.stage == naga::ShaderStage::Fragment),
                "{defs:?}: a fragment entry point"
            );
        }
    }
}
