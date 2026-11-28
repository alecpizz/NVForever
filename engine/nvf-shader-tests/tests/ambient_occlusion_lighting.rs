//! The ambient occlusion in the lighting (nvforever ssao_lighting.rs, #2897 stage two):
//! `pbr_functions` with the display-value patch and the occlusion's hook, composed through
//! Bevy's shader cache and validated by Naga for each material shader that will read it,
//! with the shader def `NVF_AMBIENT_OCCLUSION` and without. No render device is created.
//!
//! With the def the composed module holds the occlusion's two bindings (group 0, bindings 43
//! and 44 of the mesh view bind group); without it neither, so the text compiled without the
//! def is the one compiled before the hook.

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

#[test]
fn every_material_that_reads_the_occlusion_compiles_with_and_without_the_def() {
    let mut compiler = Compiler::with_ambient_occlusion_hook();
    // The shaders whose main pass calls the patched lighting and that the def reaches
    // (bevy_pbr's rule: they write depth; the landscape's blended layers by opt-in, the same
    // NIF shader). Water opts out, so it is not here.
    for (shader, extra) in [
        ("nvf_assets/nif_material.wesl", &[][..]),
        ("nvf_assets/nif_material.wesl", &["MAY_DISCARD"][..]),
        // The first-person arms and weapon (their depth squeezed, their lighting with the
        // shade light's algebra).
        ("nvf_assets/nif_material.wesl", &["FIRST_PERSON_DEPTH"][..]),
        ("nvf_assets/nif_material.wesl", &["FIRST_PERSON_DEPTH", "SHADE_LIGHT"][..]),
        ("nvforever/terrain.wesl", &[][..]),
        ("nvforever/grass.wesl", &[][..]),
        ("nvforever/tree.wesl", &["TREE_LEAVES"][..]),
        // The leaf cards' main pass takes the sun's shadow outside their crown (tree.rs).
        ("nvforever/tree.wesl", &["TREE_LEAVES", "NVF_CROWN_SHADOW"][..]),
        // Bevy's own fragment: the bark (tree.rs `BarkWind` has a vertex shader only) and
        // every StandardMaterial that writes depth.
        ("bevy_pbr/render/pbr.wesl", &[][..]),
    ] {
        for on in [false, true] {
            let mut defs = extra.to_vec();
            if on {
                defs.push("NVF_AMBIENT_OCCLUSION");
            }
            let compiled = compiler.compile(shader, &defs);
            assert_eq!(binds(&compiled, 43), on, "{shader} {defs:?}: the uniform at binding 43");
            // First-person shapes take no occlusion (ssao_lighting/hook.rs `LAMP_TEXT`): the
            // texture is not read, the uniform still is (the multi-bounce and lamps switches).
            let first_person = defs.contains(&"FIRST_PERSON_DEPTH");
            assert_eq!(binds(&compiled, 44), on && !first_person, "{shader} {defs:?}: the texture at binding 44");
            assert!(
                compiled.module.entry_points.iter().any(|e| e.stage == naga::ShaderStage::Fragment),
                "{shader} {defs:?}: a fragment entry point"
            );
        }
    }
}
