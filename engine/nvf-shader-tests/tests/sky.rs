//! The sky parts' shader (nvforever sky.wesl, with the night sky detail of night_sky.rs,
//! #2995): composed through Bevy's shader cache with Bevy's own imports and validated by
//! Naga for each alpha mode the sky parts use, with the definitions Bevy's mesh pipeline
//! sets for it (bevy_pbr render/mesh.rs `specialize`): none for the opaque dome and the
//! blended clouds and moon (`AlphaMode::Blend`), `PREMULTIPLY_ALPHA` with
//! `BLEND_PREMULTIPLIED_ALPHA` for the additive stars (`AlphaMode::Add`). No render device is
//! created; only a run of the game proves the pipeline.

mod support;
use support::{Compiled, Compiler};

/// The material's uniform (group 3, binding 100: sky.rs `SkyDisplay::light`).
fn sky_light_members(compiled: &Compiled) -> (Vec<String>, u32) {
    let (_, var) = compiled
        .module
        .global_variables
        .iter()
        .find(|(_, g)| g.binding.as_ref().is_some_and(|b| b.group == 3 && b.binding == 100))
        .expect("the SkyLight uniform at group 3, binding 100");
    let naga::TypeInner::Struct { members, span } = &compiled.module.types[var.ty].inner else {
        panic!("SkyLight is a struct");
    };
    (members.iter().map(|m| m.name.clone().unwrap_or_default()).collect(), *span)
}

#[test]
fn the_sky_shader_compiles_with_the_night_sky_detail() {
    let mut compiler = Compiler::new(&[]);
    for extra in [&[][..], &["PREMULTIPLY_ALPHA", "BLEND_PREMULTIPLIED_ALPHA"][..]] {
        let compiled = compiler.compile("nvforever/sky.wesl", extra);
        assert!(
            compiled.module.entry_points.iter().any(|e| e.stage == naga::ShaderStage::Fragment),
            "{extra:?}: a fragment entry point"
        );
        // sky.rs `SkyLight`: eight vec4s in the Rust struct's order (ShaderType, std140).
        let (names, span) = sky_light_members(&compiled);
        assert_eq!(names, ["params", "glow", "sun_dir", "sun", "glare", "moon", "night", "disc"], "{extra:?}");
        assert_eq!(span, 8 * 16, "{extra:?}");
        // The twinkle's clock: the view's globals (group 0, binding 11).
        assert!(
            compiled
                .module
                .global_variables
                .iter()
                .any(|(_, g)| g.binding.as_ref().is_some_and(|b| b.group == 0 && b.binding == 11)),
            "{extra:?}: the globals uniform"
        );
    }
}
