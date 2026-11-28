# Dependency plan: using the Rust ecosystem fully

Owner decision (2026-10-06): use Rust "to the full extent, external libraries and
everything". The first code was std-only only because its build sandbox could not reach
crates.io; that restriction was lifted the same day.

Migration happened crate by crate, each step behind the existing tests and an old-vs-new
comparison of `fnvtool` output on synthetic plugins and archives (strict: key order,
int-vs-float type, values, exit codes). The parity lab needs the owner's game data, so it
still has to be re-run there (84/84 Layer 1 scenarios expected).

## Layer 0/1: done

| Was | Now | Notes |
|---|---|---|
| `fnv-json` | `serde` + `serde_json` (`preserve_order`) | documents built with `json!`; `nvforever.json` was a derived struct (now `NVForever.ini`, hand-written, the old JSON still read); schema files keep duplicate-key rejection via a strict deserializer. Floats >= 1e16 are now written as floats (fnv-json printed them as integer tokens). |
| `fnv-inflate` decoder | `flate2`, **zlib-rs** backend | our zlib header + Adler-32 trailer diagnostics kept (`adler2`). miniz_oxide was rejected: in streaming mode it accepts back-references before the output start. |
| `fnv-util::sha256` | `sha2` (+ `hex`) | |
| `fnv-util::cp1252` | `encoding_rs` (WINDOWS_1252) | `fnv-util` also uses the existing crate's `GB18030` codec for code page 936, including GBK double-byte and GB18030 four-byte text. |
| hand-rolled CLI parsing | `clap` (derive) | `--help` everywhere; exits 0 for help, 2 for usage errors |
| ad-hoc error enums | `thiserror` (libs) + `anyhow` (fnvtool) | messages unchanged (they appear in JSON) |
| `std::fs::read` of masters | `memmap2` + `bytes::Bytes` | records are zero-copy slices; about half the peak memory |
| sequential scans | `rayon` | per-record/per-file work in parallel, folded in file order so output is identical |
| manual home-dir logic | `directories` | Windows known folders; a redirected Documents folder is followed |
| — | `tracing` + `tracing-subscriber` | debug events for data accepted without being reported; `NVFOREVER_LOG=debug` |
| — | `proptest` | malformed-input properties for every parser; `cargo-fuzz` (nightly) not set up |
| — | `cargo-deny` | `deny.toml`: licences, RustSec advisories, sources; runs in CI |
| — | `fancy-regex` 0.14 (MIT) | JIP's Sv_RegexMatch/Search/Replace and ValidateRegex in fnv-script (jip-strings.md §1): ECMAScript-style patterns need back-references, which the `regex` crate lacks |
| — | `toml` 1 (MIT/Apache-2.0), `parse` + `serde` only, the version the engine already has through Bevy | fnv-util `words` (#1443): the language files of NVForever's own words (`lang/<id>.toml`); fnv-pbr (#2981): the scalar files of authored material sets (`textures\pbr\<stem>.toml`, docs/spec-notes/pbr-materials.md), same features; nvforever tone_presets.rs (#4690): the tonemapping presets' files (`<home>\Tonemapping\<name>.toml`, docs/design/tonemapping-presets.md), same features, engine workspace |
| — | `jiff` 1 (Unlicense OR MIT) | xNVSE GetUserTime, GetTimeStamp, GetFileTime and GetFileTimeSTR in fnv-game (the computer's local time); the standard library has no time zones; photo mode's file names in nvf-play (the same version) |
| — | `getrandom` 0.3 (MIT/Apache-2.0), the version the engine already has through Bevy | `nvf-coop` (docs/design/coop-net-core.md): each co-op lobby's secret and a guest's token, from the operating system's random source |
| — | `png` 0.18 (MIT/Apache-2.0), the version the engine already builds, and `qrcodegen` 1.8 (MIT) | fnv-game `image_files.rs` (#1008, SUP batch 5): CaptureScreenshot's PNG files and TextToQRCode's code (ext-sup5.md) |
| — | `wgpu` 30 (`vulkan`) + `ash` 0.38, MIT/Apache-2.0, the versions Bevy already uses | nvforever `memory_budget.rs` (#1170): the GPU's `VK_EXT_memory_budget` through wgpu's Vulkan device (`Device::as_hal`); no wgpu patch. Tests only: wgpu's feature `noop`, a device without a GPU (`rtgi_volume/tests.rs`, #3009) |
| — | `tracy-client` 0.19 (MIT/Apache-2.0; its `tracy-client-sys` 0.30.1 carries the Tracy 0.14.1 C++ client, BSD-3-Clause) with `tracing-tracy` 0.12 (MIT/Apache-2.0), through Bevy's `trace_tracy`, the versions Bevy 0.20.1 pins | optional, only in the engine's `tracy`/`tracy-memory` build features (#1329): Bevy's spans sent live to the Tracy profiler; built `ondemand` + `only-localhost`; never in release or CI builds (frame-rate-audit.md §1) |
| — | `eframe`/`egui` 0.37 (glow, no accesskit), `egui_extras`, `egui_commonmark` 0.26, `image` 0.25, `ureq` 3 (native TLS), `rfd` 0.15 (Windows), all MIT/Apache-2.0; egui's fonts OFL-1.1 / Ubuntu Font Licence (deny.toml exception for `epaint_default_fonts` only) | `nvf-launcher` (#2708, docs/design/launcher.md): the launcher window, its news and the newest save's picture |
| — | `sha2` 0.10 (MIT/Apache-2.0), the version fnv-util already builds; `getrandom` 0.3 (MIT/Apache-2.0), already in the lock; `tungstenite` 0.30 (MIT/Apache-2.0), `handshake` only, no TLS | co-op self-hosting (#3880, #3795 sections 6 and 9): `nvf-nat::hosted`'s relay session id (`sha2`); `nvf-signal-server`'s codes and lobby secrets from the system's random source (`getrandom`) and its lobby WebSocket, server side, blocking (`tungstenite`); docs/coop-self-hosting.md |

## Engine layers (candidates, to evaluate when each layer starts)

| Layer | Need | Candidates |
|---|---|---|
| 2 | NIF parsing | **done**: own schema-as-data interpreter (`schemas/nif`, re-expressed from nif.xml as documentation); `quick-xml` codegen rejected to keep the licence open |
| 2 | DDS / BCn textures | **done**: own header/layout parser (`fnv-dds`); GPU upload as BCn directly; CPU BCn decoders deferred (ADR-0001) |
| — | player reports (#2055) | **chosen (2026-11-10)**: `ureq` 3 (MIT/Apache-2.0), engine workspace, `native-tls` only: HTTPS through SChannel and the system's certificates (`native-tls` MIT/Apache-2.0, `schannel` MIT). Windows only until #3144 (2026-11-21); since then on Linux too, through the system's OpenSSL (`openssl` Apache-2.0, `openssl-sys` MIT), which the engine workspace already built there for `nvf-dlss-files`' download, so no new crate. Not rustls: its `ring`/`rustls-webpki` (ISC) and `webpki-roots` (CDLA-Permissive-2.0) are outside deny.toml. `uuid` 1 with `v4`, `flate2` 1 (`zlib-rs`) and `sysinfo` 0.39 (`system` only), each the version the engine already builds through Bevy or fnv-inflate: install and session ids, the summary's gzip, the CPU and memory lines (engine/nvforever/src/reports/) |
| — | first-start dialogs (game folder) | **chosen**: `rfd` 0.15 (MIT), engine workspace, Windows only, no default features: the system's message boxes and folder picker before the game window opens (install-layout.md, "First start") |
| 3 | CPU BCn encode/decode (generated LOD) | **chosen**: `texpresso` 2.0 (pure Rust, MIT), engine workspace only: decodes terrain textures and encodes the generated LOD's BC1 textures (docs/design/generated-lod.md); in `nvf-assets` too, for the texture levels the bounce-light solver reads (#3009, [rtgi-collect](rtgi-collect.md)) |
| 2 | math, viewer | **done**: Bevy 0.20.1 (its `glam`; 0.19.1 until #934), viewer in its own workspace (ADR-0001) |
| 7 | interface CJK glyph fallback | **chosen**: `swash` 0.2.10 (MIT/Apache-2.0), engine workspace only and the version Bevy already builds: reads user-supplied or installed fonts and rasterizes missing glyphs ([fonts](../spec-notes/fonts.md#23-missing-cjk-characters)) |
| 3 | rendering | `wgpu` (Vulkan/Metal/DX12), `winit` 0.30.13 patched (Apache-2.0, `engine/vendor/winit`, [vendor notes](../../engine/vendor/README.md#winit-03013-apache-20-rust-windowing-contributors)) for windows/input (Android-only controller admission hook in `src/platform/android.rs` and `src/platform_impl/android/mod.rs`; desktop backends unchanged) |
| 3 | Windows script cursor acknowledgement (#2883) & Android cursor-grab no-op (#2436) | `bevy_winit` 0.20.1 patched (MIT/Apache-2.0, [vendor notes](../../engine/vendor/README.md#bevy_winit-0201-mit-or-apache-20-bevy)): only acknowledged script warps update both Window and the backend cache. The crate-level `unsafe_code` lint is `deny` rather than `forbid` for the function-local Win32 ownership query; pre-existing modules retain `forbid`. On `target_os = "android"` only, `winit_windows.rs::attempt_grab` is a no-op returning `Ok(())` because Android `NativeActivity` has no desktop cursor-grab API; non-Android platforms are unchanged. |
| 3 | temporal upscaling (FSR 3.1) | **chosen (2026-11-05)**: `wgpu-ffx` 0.1.0 and `wgpu-ffx-shaders-spv` 0.1.0 (MIT; the FidelityFX SDK 1.1.4 FSR 3.1.4 shaders as SPIR-V). Not on crates.io, so vendored at commit `6fe7064` with the FSR 3.1 shader sources and the compile tool (the SDK's other effects left out), and moved to wgpu 30 (engine/vendor/README.md). It turns on wgpu's `spirv` and `wgsl` and brings `pollster` 0.4 (MIT/Apache-2.0) and `termcolor` 1.4 (Unlicense/MIT, through naga's `codespan-reporting`). No AMD DLL; Vulkan only ([fsr-upscaling](fsr-upscaling.md)) |
| 3 | scaled main passes (FSR) | `bevy_pbr` 0.20.1 patched (MIT/Apache-2.0, engine/vendor/README.md): the transmission pass and the clustered lights follow `MainPassResolutionOverride` |
| 3 | frame generation (FSR 3.1) | **chosen (2026-11-07)**: no new crate. The vendored `wgpu-ffx` gains optical flow and frame interpolation, ported from FidelityFX SDK 1.1.4's MIT host code, with the SDK's MIT GLSL compiled to SPIR-V in `wgpu-ffx-shaders-spv` (engine/vendor/README.md). Not used: AMD's FSR SDK 2.x DLLs (binary-only licence) and its frame-generation swapchain; the frames are presented through Bevy's own window surface ([frame-generation](frame-generation.md)) |
| 3 | temporal upscaling (NVIDIA DLSS, #2198) | **chosen (2026-11-12)**: no new crate. NVIDIA Streamline 2.14.16 is not linked: the player supplies its DLLs and NVIDIA's DLSS (`<program folder>\dlss\`), loaded at run time on Windows and only when DLSS is chosen at start; NVForever ships no NVIDIA file. The few declarations called are translated from Streamline's MIT headers (`engine/nvforever/src/dlss/ffi.rs`, `security.rs`; notice in `dlss/LICENSE-MIT`, carried into the releases' `THIRD-PARTY-LICENSES.txt` by `tools/release/third_party_licenses.py`). `windows-sys` 0.61 (already in the build) gains its `Win32_System_LibraryLoader`, `Win32_Security_WinTrust` and `Win32_Security_Cryptography` features; `ash` 0.38 (as above) reads the GPUs on a probe instance; the vendored `bevy_render` makes the Vulkan instance from the interposer (`RawVulkanInitSettings::set_vulkan_library`, engine/vendor/README.md). Not used: Bevy's own DLSS (`dlss_wgpu` links NVIDIA's NGX SDK statically, which NVIDIA's licence does not allow in an open-source program) ([dlss-upscaling](dlss-upscaling.md)) |
| 3 | latency reduction (AMD Anti-Lag) | **chosen (2026-11-06)**: no new crate. Bevy's `raw_vulkan_init` feature (bevy_render 0.20.1) adds `VK_AMD_anti_lag` at device creation; the calls are written against the Vulkan registry, as ash 0.38 predates the extension ([latency-reduction](latency-reduction.md)) |
| 3 | latency reduction (NVIDIA low latency) | **chosen (2026-11-12)**: no new crate. `VK_NV_low_latency2` from the public Vulkan registry through `ash` 0.38, which has its structures and calls; NVIDIA's Streamline, Reflex SDK and NvAPI are not used (proprietary). `wgpu-hal` 30.0.1 patched (MIT/Apache-2.0, engine/vendor/README.md): the swapchain is created with `VkSwapchainLatencyCreateInfoNV` on a device that has the extension ([latency-reduction](latency-reduction.md)) |
| 3 | ALC shadows (#3074) | **chosen (2026-11-18)**: no new crate. `bevy_light` 0.20.1 patched (MIT/Apache-2.0, [vendor notes](../../engine/vendor/README.md#bevy_light-0201-mit-or-apache-20-bevy-contributors)): shadow caster candidates and shown shadow lights, both resources empty with the row Off; `bevy_pbr` 0.20.1 patched further (held light shadow maps, static shadow cache, kept shadow phases, cached shadow views, hidden light views, light view counts, the contact shadows' main-pass viewport): all gated by `HoldLightShadowMaps::active`, upstream's paths with the row Off. Bevy's `experimental_pbr_pcss` feature (engine/Cargo.toml) for the soft edges on Quality: a compile-time feature, so it is on in every build; it adds two filtering samplers to the mesh view bindings and the `PCSS_SAMPLERS_AVAILABLE` shader def, and a light takes the PCSS path only with soft shadows enabled, which only the ALC shadows' lights get (light_shadow_detail.rs; the sun's has none) [documented: engine/vendor/bevy_pbr render/light.rs, mesh_view_bindings.rs, shadows.wesl] |
| 3 | graphics pass (#3781) | **chosen (2026-11-24)**: no new crate. `bevy_core_pipeline` 0.20.1 patched further (vendored with HDR output, #3348, below; MIT/Apache-2.0, [vendor notes](../../engine/vendor/README.md#bevy_core_pipeline-0201-mit-or-apache-20-bevy-contributors), archive SHA-256 there; `bytemuck`, already in the build, as its new dependency): bevy#26239's depth pyramid over the main pass's region. Further patches, all MIT/Apache-2.0 and listed in the vendor notes: `bevy_pbr` 0.20.1 (shadow early-outs, never occlusion culling a `NoFrustumCulling` mesh, bevy#26418's kept preprocessing bind groups, bevy#25869 and bevy#26588's clustering buffers, bevy#26239's light and meshlet parts, the merged scene unpacking's bind groups, dispatches and two shaders), `bevy_render` 0.20.1 (the merged scene unpacking's packed buffers, bevy#26418's stale-job clearing, bevy#26239's main-pass viewport), `bevy_light` 0.20.1 (small casters left out of far cascades, `NVF_SMALL_CASTER_TEXELS`) and `bevy_post_process` 0.20.1 (the auto-exposure histogram counting each bin once, upstream Bevy's fix). Visible: the metered exposure with bright pixels in view, and sub-texel casters missing from the far cascades (both accepted by the maintainer, 2026-11-24) ([frame-rate-audit](frame-rate-audit.md) §13) |
| 3 | first Pip-Boy readiness (#1803) | `bevy_pbr` 0.20.1 patched (engine/vendor/README.md): the optional `MaterialQueueProbe` records watched main-pass entries for the rig's readiness check; nothing changes without it |
| 3 | HDR output (#3348) | **chosen**: no new crate. `bevy_core_pipeline` 0.20.1 patched (MIT/Apache-2.0, [vendor notes](../../engine/vendor/README.md#bevy_core_pipeline-0201-mit-or-apache-20-bevy-contributors), archive SHA-256 `2463cf477b683bfe8567c43142295d00ed980f8e2a293c0a69e2f9e10e9c0aaf`): the window blit's key gains `hdr` and `hdr_gamma22` (shader defs `NVF_HDR_PAPER_WHITE`, `NVF_HDR_GAMMA22`), a third binding holds the HUD's white in nits / 80, and the upscaling pipeline's key takes the HDR statics for a window whose surface is HDR; with HDR off both are upstream's paths [documented: engine/vendor/bevy_core_pipeline src/blit/mod.rs, blit.wesl, upscaling/mod.rs] |
| 3 | HDR output: the screen's HDR peak (#3348) | `windows` 0.62 (MIT/Apache-2.0), engine workspace, Windows only, direct dependency of `nvforever`, features `Win32_Foundation`, `Win32_Graphics_Dxgi`, `Win32_Graphics_Dxgi_Common`, `Win32_Graphics_Gdi`; the version already in the build: `IDXGIOutput6::GetDesc1` gives the monitor's colour space and peak luminance (hdr.rs) |
| 3 | half floats (FaceGen offset textures; reflection cube texels, #3207) | **chosen**: `half` 2 (MIT/Apache-2.0), engine workspace, the version Bevy already builds (2.7.1 in the lock, no second copy): `nvforever` writes the FaceGen offset textures with it (facegen.rs) and `nvf-assets` the authored materials' reflection cube's `Rgba16Float` texels (reflection_cube.rs) |
| 3 | bounce-light solver (#3009) | **chosen**: no new external dependency. `nvf-rtgi`, a member of the engine workspace (GPL-3.0-or-later, not published), is the standard library alone ([rtgi-solver](rtgi-solver.md)); `nvforever` depends on it for the volume and its read ([rtgi-volume](rtgi-volume.md)); `nvf-assets` takes it for its plain data and its choice of texture levels ([rtgi-collect](rtgi-collect.md)) |
| 4 | physics / character controller | **chosen**: `rapier3d` 0.36 with `enhanced-determinism` (pure Rust, Apache-2.0, no Bevy coupling; Jolt bindings rejected for the C++ build and FFI, `avian3d` for its Bevy coupling; layer4.md §2.8). Used by `fnv-phys`, which runs rapier in metres internally and exposes game units, and directly by `nvforever` for the Caravan pot (the caps tossed into the pot fall and settle on the table, `caravan_pot.rs`, its own small rapier world, the same pinned version); terrain as triangle meshes (rapier's heightfield missed rays along grid lines). Pinned to `=0.36.1` (0.36.0 until the physics rate work, #2358; traces are compared bit for bit; licence Apache-2.0, its new dependencies `glamx`/`glam`/`parry3d` MIT/Apache-2.0, within deny.toml). Vendored at `vendor/rapier3d/` (patched in both `Cargo.toml` and `engine/Cargo.toml` via `[patch.crates-io]`, crates.io archive SHA-256 `033e2edc815d61ab97bd74ed5d1cbea5ebe575218774a7cfedbd3551f0033317`, Apache-2.0 `LICENSE` retained; [vendor notes](../../vendor/rapier3d/README.nvforever.md)) with two bounded zero-dt query-refresh fixes: joining a deferred broad-phase BVH optimization before the `dt <= 0.0` early return (`src/pipeline/physics_pipeline/substep.rs`, matching upstream Rapier PR #1067) and committing sleeping chunks after persistent sleep-component bootstrap (`src/dynamics/island_manager/manager.rs`). docs/spec-notes/physics.md, docs/spec-notes/query-refresh-island-lifecycle.md |
| 4 | entity model | **decided**: plain ordered maps in `fnv-sim`, no ECS in the core (layer4.md §2.2); `rand_chacha` for the seeded, reproducible RNG |
| 5 | script runtime | in-house bytecode interpreter (no off-the-shelf fit) |
| 7 | audio decode / playback | `symphonia` (WAV/MP3/OGG), `kira` or `cpal`. **Dialogue voice (2026-10-10)**: no new dependency. Playback uses Bevy's own audio (`bevy_audio`, already built by Bevy's default features: `rodio` with `lewton` for Ogg Vorbis; WAV, MP3 and FLAC decoding are not enabled and not needed, every voice line has an `.ogg`). File lengths come from `crates/fnv-voice`'s own header readers (Ogg pages and the Vorbis identification header, RIFF chunks), so the headless tools need no audio crate (docs/spec-notes/voice.md §5). **Music and ambient sound (2026-10-11)**: `nvf-play` depends on `rodio` 0.22 directly, the version Bevy's audio is built on (no second copy), with its `mp3`, `vorbis` and `wav` features; these bring in `symphonia` (MPL-2.0, already allowed in `deny.toml`). Music files are MP3 and many ambient sounds Ogg Vorbis; they are decoded while they play by a source of our own that mixes a media set's three layers (docs/spec-notes/audio.md §7) |
| 7 | save thumbnails | **chosen (2026-10-15)**: `png` 0.18 (MIT/Apache-2.0) in `nvf-play`, the version Bevy's image loading already builds (no second copy): encodes the 512×288 thumbnail and decodes it for the load page (docs/design/save-load.md §6) |
| — | code mods (WebAssembly) | **chosen (owner, 2026-11-02)**: `wasmtime` (Apache-2.0 WITH LLVM-exception) with the Component Model and `wasmtime-wasi` (granting nothing), pinned exactly for determinism; not added yet. Phase M1 adds it and allows `Apache-2.0 WITH LLVM-exception` in `deny.toml`. `wasmi` (an interpreter) was rejected: too slow for the per-frame visual tier. docs/design/code-mods.md §6, §17 item 5 |
| 7 | UI (Bethesda XML menus) | own forgiving reader (`crates/fnv-ui`; the files are not strict XML, ui-tile-tree.md §9, so `quick-xml` was not used) + own layout engine; `egui` for dev tools |
| video | Bink intro | **chosen (2026-10-15)**: `infinitier_bik_decoder` 0.0.14 (GPL-3.0-or-later, as NVForever; pure Rust, a port of FFmpeg's open-source Bink video 1 and Bink audio decoders, not of any leaked code) in `nvf-play`, pinned `=0.0.14`; with it `infinitier_datasource` (GPL-3.0-or-later), `hound` (Apache-2.0), `bitflags`. Plays `Video\FNVIntro.bik` before a new game (`engine/nvf-play/src/intro.rs`): about 4.3 ms a 1280×720 frame on the owner's machine, decoded on its own thread. No FFmpeg. Behind the cargo feature `intro-video`, **on by default** since the owner settled the licence as GPL-3.0-or-later (2026-10-15); `--no-default-features` leaves it out |

## Licence check

**NVForever is GPL-3.0-or-later** (owner decision 2026-10-15, settling the earlier
placeholder): dependencies may be permissive or GPL-3.0-compatible copyleft. Most are
MIT/Apache-2.0 or similar, one MPL-2.0 (`option-ext`, via `directories`), and the intro
film's decoder (`infinitier_bik_decoder`, `infinitier_datasource`) GPL-3.0-or-later.
`cargo deny check` enforces the allow-list in `deny.toml` and RustSec advisories, for the
root workspace and, with `--manifest-path engine/Cargo.toml`, the engine workspace (the
game binary, its own lock) against the same `deny.toml`; CI runs both (#1998). The engine
adds BSD-2-Clause (`arrayref`), MIT-0 (`encase`) and ISC (`libloading`, `inotify`) to
the allow-list. One advisory is ignored with its reason: RUSTSEC-2026-0192, `ttf-parser`
unmaintained, which comes only through winit's Linux window decorations in Bevy 0.20
(no upgrade available; look again at the next Bevy upgrade).

## CI

`.github/workflows/ci.yml` (read 2026-11-12): one Linux job, so branch protection has a
single check to require. It runs on every pull request, on pushes to main that change more
than docs (`docs/` and `*.md`), and by hand. In order: `cargo deny` (licences, advisories,
bans, sources) for the root workspace; the root workspace's tests and clippy (`--release
--locked`, `-D warnings`); the Python self-tests of the parity lab, the capture tools, the
smoke runner, the stress tool and the profile campaign; the engine workspace's tests and
clippy. A pull request that changes only docs skips those steps and passes at once; a newer
push to the same branch or pull request cancels the run still in progress. The repository
is public, so runner minutes are free. Since #3143 (2026-11-21) one Windows job,
`windows-clippy`, lints both workspaces, so the `cfg(windows)` code is compiled and
checked on every pull request; the tests on Windows and all of macOS are not run in CI:
the local checks in CLAUDE.md cover them before each push. (The "one Linux job" above
has been several parallel jobs with one `ci` result since #1956.) The parity job that needs game data
stays in `parity-lab/ci/github-actions.yml`, a template for a self-hosted runner only.


## Optional Linux GPU usage (#2249)

[design, #2249: `engine/nvforever/Cargo.toml`, `perf_overlay/nvml.rs`]
`nvforever` directly uses `libloading` 0.8.9 on Linux only, reusing the exact version
already in `engine/Cargo.lock` through the graphics stack. This avoids another crate
and hand-written loader bindings. NVML remains optional at run time; no NVIDIA SDK
or link-time NVIDIA library is required. The library stays loaded through all calls
and a matching `nvmlShutdown`, including initialization paths with no readable devices.

## Android platform boundary (#2436)

[design] The Android-only `nvforever` dependency on `jni` 0.21.1 (MIT/Apache-2.0)
reuses the version already in engine/Cargo.lock. It attaches the engine thread to the
VM owned by Bevy's AndroidApp and calls the private app activity for startup arguments,
restart and exit. The `nvf-android` entry package uses the existing pinned Bevy and
its `android-native-activity` feature. Java uses Android's platform APIs without a
third-party runtime library. Gradle 8.11.1 and Android Gradle Plugin 8.9.2 are build tools.

### Vendored forks touched or added by the Android port (#2436)

All four vendored crates touched or added by `#2436` are wired through `[patch.crates-io]`
(preserving upstream licence files and passing `cargo deny check` in both the root and
`engine/` workspaces):

- **`rapier3d` 0.36.1 (Apache-2.0)** — `vendor/rapier3d/` (introduced by `#2436`), patched
  in both `Cargo.toml` and `engine/Cargo.toml` (crates.io archive SHA-256
  `033e2edc815d61ab97bd74ed5d1cbea5ebe575218774a7cfedbd3551f0033317`; details in
  [`vendor/rapier3d/README.nvforever.md`](../../vendor/rapier3d/README.nvforever.md)).
  Only two Rust files differ from upstream `0.36.1`: joining a deferred broad-phase BVH
  optimization before `dt <= 0.0` returns (`src/pipeline/physics_pipeline/substep.rs`,
  same fix as Rapier PR #1067) and invoking the already-upstream
  `IslandManager::commit_sleeping_chunks` (present unchanged in upstream `0.36.1`'s
  `src/dynamics/island_manager/sleep.rs`) for sleeping bodies remaining in the awake
  island after persistent sleep bootstrap (`src/dynamics/island_manager/manager.rs`).
  Both files are active on all targets (desktop and Android).
- **`winit` 0.30.13 (Apache-2.0)** — `engine/vendor/winit/` (introduced by `#2436`),
  patched in `engine/Cargo.toml` (details in
  [`engine/vendor/README.md`](../../engine/vendor/README.md#winit-03013-apache-20-rust-windowing-contributors)).
  Carries an Android-only `NVForever controller admission` hook in
  `src/platform/android.rs` and `src/platform_impl/android/mod.rs` so NVForever can
  inspect native Android controller events before `NativeActivity`'s single queue
  consumer translates them into window events; desktop backends are unchanged.
- **`bevy_winit` 0.20.1 (MIT OR Apache-2.0)** — `engine/vendor/bevy_winit/`, already
  vendored on `main` for Windows script cursor acknowledgement (#2883; archive SHA-256
  `397c685e6ab6d4d87b89fb4ce11993c223d0e3c7b34c415cb4b9f69f213d85b6`; details in
  [`engine/vendor/README.md`](../../engine/vendor/README.md#bevy_winit-0201-mit-or-apache-20-bevy)).
  Adds a `#[cfg(target_os = "android")]` no-op in `src/winit_windows.rs::attempt_grab`
  returning `Ok(())` because Android `NativeActivity` has no cursor-grab API; desktop
  `attempt_grab` is unchanged.
- **`bevy_pbr` 0.20.1 (MIT OR Apache-2.0)** — `engine/vendor/bevy_pbr/`, already vendored
  on `main` for FSR, ALC shadows, and Pip-Boy readiness (details in
  [`engine/vendor/README.md`](../../engine/vendor/README.md#bevy_pbr-0201-mit-or-apache-20-bevy-contributors)).
  Adds a `+36/-19` patch in `src/render/skinning.wesl` wrapping storage `joint_matrices` /
  `prev_joint_matrices` in `struct StorageJointMatrices` and adding zero-determinant /
  zero-normal guards in `inverse_transpose_3x3m` and `skin_normals`. Compiles and runs on
  **both desktop and Android** (covered by `tools/android/skinning-check`, 9 tests).

## Owned Android touch SVG rasterization (#2436)

[design] `nvforever` requests tiny-skia =0.11.4 directly (BSD-3-Clause), already present
in engine/Cargo.lock via Bevy. Default features are disabled; `std`/`simd` provide the
bounded antialiased path stroker, without enabling a PNG encoder. Owned SVGs under
`engine/nvforever/assets/touch/` are validated and compiled into the committed
`engine/nvforever/src/touch/icons_generated.rs` by running `tools/android/svg_icons.py`
by hand when SVGs change (`tools/android/tests/test_svg_icons.py` verifies parity).
`engine/nvforever/build.rs` does not invoke Python, so desktop Cargo builds need no
Python runtime, and no runtime XML/SVG crate (`resvg`/`usvg`) is added.
See [android-svg-icons.md](android-svg-icons.md) for bounds and pending GPU validation.
