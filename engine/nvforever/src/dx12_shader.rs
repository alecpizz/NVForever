//! The DirectX 12 renderer's shader compiler: DXC beside the program.
//!
//! [observed 2026-11-15, RTX 3070, Goodsprings at 2560x1440] With `WGPU_BACKEND=dx12` and no
//! DXC, wgpu compiles the HLSL it translates from the WGSL with FXC. FXC took so long over
//! Bevy's PBR pipelines that the loading screen gave up waiting after 15 s with 66 of them
//! (the blit and bloom ones too) not compiled, and the world, the sky and the loading screen
//! itself stayed undrawn. With DXC (the Windows SDK 10.0.22621 `dxcompiler.dll` and
//! `dxil.dll`) the same start waited 8.2 s for its pipelines, and the picture matched the
//! Vulkan renderer's.
//!
//! Bevy picks DXC only when `dxcompiler.dll` is in the *working directory*
//! (vendor/bevy_render/src/settings.rs), which a shortcut or the launcher need not set to
//! the program's folder. So the choice is made here: `WGPU_DX12_COMPILER` when it is set
//! (wgpu's own variable: `fxc`, `dxc`, `staticdxc`), otherwise DXC by its full path when
//! `dxcompiler.dll` lies beside the program (the Windows release ships it with `dxil.dll`,
//! `.github/workflows/release.yml`), otherwise FXC, with a warning when the DirectX 12
//! renderer is the one in use. Linking DXC in (Bevy's `statically-linked-dxc`) was tried and
//! does not link with MSVC 14.42: its prebuilt library needs a newer C++ runtime library.
//! The Vulkan renderer compiles no HLSL and is untouched.

use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::settings::Dx12Compiler;
use std::path::{Path, PathBuf};

/// DXC's library, looked for beside the program.
pub const DXC_DLL: &str = "dxcompiler.dll";

/// The compiler for the DirectX 12 renderer (the module comment).
pub fn compiler() -> Dx12Compiler {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    choose(Dx12Compiler::from_env(), exe_dir.as_deref(), |p| p.is_file())
}

fn choose(from_env: Option<Dx12Compiler>, exe_dir: Option<&Path>, exists: impl Fn(&Path) -> bool) -> Dx12Compiler {
    if let Some(c) = from_env {
        return c;
    }
    match exe_dir.map(|d| d.join(DXC_DLL)) {
        Some(dll) if exists(&dll) => Dx12Compiler::DynamicDxc { dxc_path: path_string(dll) },
        _ => Dx12Compiler::Fxc,
    }
}

fn path_string(p: PathBuf) -> String {
    p.to_string_lossy().into_owned()
}

/// Logs the compiler once the renderer is up, and warns when DirectX 12 runs on FXC.
pub struct Dx12ShaderPlugin;

impl Plugin for Dx12ShaderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, report);
    }
}

fn report(info: Option<Res<RenderAdapterInfo>>) {
    if info.is_none_or(|i| i.backend != wgpu::Backend::Dx12) {
        return;
    }
    match compiler() {
        Dx12Compiler::Fxc => warn!(
            "dx12: shaders compile with FXC: {DXC_DLL} is not beside the program; pipelines may take minutes and the world may not draw (use the Vulkan renderer, or put {DXC_DLL} and dxil.dll beside nvforever.exe)"
        ),
        Dx12Compiler::DynamicDxc { dxc_path } => info!("dx12: shaders compile with DXC ({dxc_path})"),
        other => info!("dx12: shaders compile with {other:?} (WGPU_DX12_COMPILER)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variable_wins() {
        let dir = Path::new("C:\\games\\nvf");
        assert!(matches!(choose(Some(Dx12Compiler::Fxc), Some(dir), |_| true), Dx12Compiler::Fxc));
    }

    #[test]
    fn dxc_beside_the_program_by_its_full_path() {
        let dir = Path::new("C:\\games\\nvf");
        match choose(None, Some(dir), |p| p == dir.join(DXC_DLL)) {
            Dx12Compiler::DynamicDxc { dxc_path } => assert_eq!(PathBuf::from(dxc_path), dir.join(DXC_DLL)),
            other => panic!("expected DXC, got {other:?}"),
        }
    }

    #[test]
    fn fxc_without_dxc() {
        assert!(matches!(choose(None, Some(Path::new("C:\\games\\nvf")), |_| false), Dx12Compiler::Fxc));
        assert!(matches!(choose(None, None, |_| true), Dx12Compiler::Fxc));
    }
}
