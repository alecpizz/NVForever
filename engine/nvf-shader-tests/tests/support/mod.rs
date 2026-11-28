//! Shared by the shader compile checks: production WESL composed through Bevy's shader
//! cache, then parsed and validated with the Naga version the renderer uses. No render
//! device is created.

// Each test crate uses its own part of this module.
#![allow(dead_code)]

use bevy_asset::{AssetId, Assets};
use bevy_shader::{Shader, ShaderCache, ShaderCacheSource, ShaderDefVal};
use std::{collections::HashMap, fs, path::Path, sync::Arc};

#[path = "../../../nvforever/src/lights/display_shader.rs"]
pub mod production_display_shader;

#[path = "../../../nvforever/src/ssao_lighting/hook.rs"]
pub mod production_ambient_occlusion_hook;

#[path = "../../../nvforever/src/lights/reflection_gate.rs"]
pub mod production_reflection_gate;

pub struct Compiled {
    pub module: naga::Module,
    pub info: naga::valid::ModuleInfo,
}

pub struct Compiler {
    cache: ShaderCache<Compiled, ()>,
    shaders: HashMap<String, AssetId<Shader>>,
    assets: Assets<Shader>,
    /// `pbr_functions` also takes the ambient occlusion's hook (nvforever ssao_lighting.rs),
    /// as lights.rs makes it in the game after the display-value patch.
    ambient_occlusion_hook: bool,
    /// `pbr_functions` also takes the reflection gate (nvforever lights/reflection_gate.rs)
    /// after the ambient occlusion's hook, as lights.rs makes it in the game.
    reflection_gate: bool,
    /// `pbr_lighting` and `pbr_functions` take the physical PBR patches (the Physical lighting row).
    physical_pbr: bool,
}

impl Compiler {
    /// `remove_invariance` restores the unannotated producer in memory. This runs
    /// through WESL and Naga too; it is not a source-text assertion.
    pub fn new(remove_invariance: &[&str]) -> Self {
        Self::build(remove_invariance, false, false, false)
    }

    /// The production sources with the ambient occlusion's hook in `pbr_functions` as well,
    /// and nvf_assets' shaders (the NIF material) registered.
    pub fn with_ambient_occlusion_hook() -> Self {
        Self::build(&[], true, false, false)
    }

    /// As [`Self::with_ambient_occlusion_hook`], with the reflection gate after the hook.
    pub fn with_reflection_gate() -> Self {
        Self::build(&[], true, true, false)
    }

    /// The production sources with the physical PBR patches (`Fd_OrenNayar`, full
    /// Cook-Torrance Fresnel, linear diffuse/ambient; the ambient occlusion's hook for them
    /// comes with #3828's split part 2).
    pub fn with_physical_pbr() -> Self {
        Self::build(&[], false, false, true)
    }

    /// The physical PBR patches with the reflection gate after them (Physical lighting in a
    /// run with authored reflections, nvforever lights.rs `gating`).
    pub fn with_physical_pbr_and_gate() -> Self {
        Self::build(&[], false, true, true)
    }

    fn build(
        remove_invariance: &[&str],
        ambient_occlusion_hook: bool,
        reflection_gate: bool,
        physical_pbr: bool,
    ) -> Self {
        let mut compiler = Self {
            cache: ShaderCache::new((), |_, source, _| {
                let ShaderCacheSource::Wgsl(wgsl) = source else {
                    panic!("WESL must compose to WGSL");
                };
                let module = naga::front::wgsl::parse_str(&wgsl)
                    .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&wgsl)));
                let info = naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::default(),
                )
                .validate(&module)
                .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&wgsl)));
                Ok(Compiled { module, info })
            }),
            shaders: HashMap::new(),
            assets: Assets::default(),
            ambient_occlusion_hook,
            reflection_gate,
            physical_pbr,
        };
        let engine = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut packages = vec![
            ("bevy_pbr", "vendor/bevy_pbr/src"),
            ("bevy_render", "vendor/bevy_render/src"),
            ("nvforever", "nvforever/src"),
        ];
        if ambient_occlusion_hook || physical_pbr {
            packages.push(("nvf_assets", "nvf-assets/src"));
        }
        let all_assets = packages.iter().any(|(package, _)| *package == "nvf_assets");
        for (package, directory) in packages {
            let root = engine.join(directory);
            compiler.register_directory(package, &root, &root, remove_invariance);
        }
        // The shared tonemapper input, which nvforever's sky and clouds import whatever else
        // of nvf_assets a test leaves out.
        if !all_assets {
            let path = engine.join("nvf-assets/src/tone_input.wesl");
            compiler.register("nvf_assets/tone_input.wesl", fs::read_to_string(path).expect("tone_input.wesl"));
        }
        compiler
    }

    fn register_directory(
        &mut self,
        package: &str,
        root: &Path,
        directory: &Path,
        remove: &[&str],
    ) {
        let mut paths: Vec<_> = fs::read_dir(directory)
            .expect("production shader directory")
            .map(|entry| entry.expect("shader directory entry").path())
            .collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                self.register_directory(package, root, &path, remove);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "wesl")
            {
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/");
                let name = format!("{package}/{relative}");
                let mut source = fs::read_to_string(&path).expect("production WESL source");
                if self.physical_pbr && name == "bevy_pbr/render/pbr_lighting.wesl" {
                    source = production_display_shader::lighting_patched(&source, true)
                        .expect("production physical lighting patch anchors match the selected shader");
                }
                if name == "bevy_pbr/render/pbr_functions.wesl" {
                    if self.physical_pbr {
                        source = production_display_shader::physical_patched(
                            &production_display_shader::unix_lines(&source),
                        )
                        .expect("production physical PBR patch anchors match the selected shader");
                    } else {
                        // LightsPlugin prepares this actual module before grass uses it.
                        source = production_display_shader::display_patched(
                            &production_display_shader::unix_lines(&source),
                        )
                        .expect("production display-lighting patch anchors match the selected shader");
                        if self.ambient_occlusion_hook {
                            source = production_ambient_occlusion_hook::hook_patched(&source)
                                .expect("the ambient occlusion hook's anchors match the selected shader");
                        }
                    }
                    if self.reflection_gate {
                        source = production_reflection_gate::gate_patched(&source)
                            .expect("the reflection gate's anchors match the selected shader");
                    }
                }
                if remove.contains(&name.as_str()) {
                    source = source.replace("@invariant ", "");
                }
                self.register(&name, source);
            }
        }
    }

    pub fn register(&mut self, name: &str, source: String) {
        let shader = Shader::from_wesl(source, format!("embedded://{name}"));
        let id = self.assets.add(shader.clone()).id();
        self.cache.set_shader(id, shader);
        self.shaders.insert(name.to_owned(), id);
    }

    pub fn compile(&mut self, name: &str, extra: &[&'static str]) -> Arc<Compiled> {
        // Representative storage-buffer mesh view; constants are the renderer's
        // specialization inputs, not substitute shader declarations.
        let prepass =
            name == "bevy_pbr/prepass/prepass.wesl" || extra.contains(&"PREPASS_PIPELINE");
        // PrepassPipeline::specialize only supplies normal/tangent attributes for
        // normal/deferred prepasses; the depth-only morph vertex has neither field.
        let normal_attributes = !prepass || extra.contains(&"NORMAL_PREPASS_OR_DEFERRED_PREPASS");
        let mut definitions: Vec<ShaderDefVal> = [
            "VERTEX_POSITIONS",
            "VERTEX_UVS",
            "VERTEX_UVS_A",
            "VERTEX_UVS_B",
            "VERTEX_COLORS",
            "VERTEX_OUTPUT_INSTANCE_INDEX",
            "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3",
            "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_6",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        if prepass && !extra.contains(&"PREPASS_PIPELINE") {
            definitions.push("PREPASS_PIPELINE".into());
        }
        if normal_attributes {
            definitions.extend(
                ["VERTEX_NORMALS", "VERTEX_TANGENTS"]
                    .into_iter()
                    .map(ShaderDefVal::from),
            );
        }
        definitions.extend(
            extra
                .iter()
                .copied()
                .filter(|feature| {
                    normal_attributes
                        || !matches!(
                            *feature,
                            "VERTEX_NORMALS_COMPRESSED" | "VERTEX_TANGENTS_COMPRESSED"
                        )
                })
                .map(ShaderDefVal::from),
        );
        definitions.extend([
            ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 3),
            ShaderDefVal::UInt("MAX_CASCADES_PER_LIGHT".into(), 4),
            ShaderDefVal::UInt("MAX_DIRECTIONAL_LIGHTS".into(), 10),
            ShaderDefVal::UInt("MAX_RECT_LIGHTS".into(), 1),
        ]);
        self.cache
            .get(0, *self.shaders.get(name).unwrap_or_else(|| panic!("{name}: no such shader registered")), &definitions)
            .unwrap_or_else(|error| panic!("{name}, {extra:?}: {error}"))
    }
}

