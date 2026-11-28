//! The temporal upscaler choice shared by FSR 3.1 (`fsr.rs`) and DLSS (`dlss/`), #2198
//! (docs/design/dlss-upscaling.md, "Internal interfaces").
//!
//! - [`UpscalerChoice`] is what the player asks for: off, FSR at a quality or DLSS at a quality.
//!   It is the one requested setting, `Graphics::upscaler`; FSR's call sites read it through
//!   `Graphics::fsr` and write it through `Graphics::set_fsr`.
//! - [`UpscaleQuality`] is the quality list both upscalers use (`fsr::FsrQuality` is an alias;
//!   FSR's ratios stay in `fsr.rs`, DLSS's sizes come from DLSS).
//! - [`UpscalerStatus`] is what this session can do and what it draws: the **effective** choice,
//!   whether a **restart is pending**, and why a request cannot run ([`Unavailable`]).
//!   [`resolve`] is its rule [design]: Streamline is loaded at start whenever NVIDIA's files are
//!   in the DLSS folder (or DLSS is chosen), so with it loaded off, FSR and DLSS all switch at
//!   once (FSR then draws through the interposer). A restart is pending only for DLSS asked for
//!   in a session that started without Streamline while the files are there now (they were
//!   added since, or the test aid `NVF_DLSS_PRELOAD=0` kept it out); until then what drew stays.
//!
//! - [`prep`] is the view preparation every temporal upscaler shares (step 4): camera history
//!   and cuts, the render-size override, jitter and mip bias, the corrected and expanded depth,
//!   the spatial fallback and the alpha copy. FSR adds its own state after it; DLSS gets it
//!   without any of FSR's.
//! - [`temporal_active`] is the question the shared parts ask (HDR, prepasses, cut-outs drawn
//!   alpha-tested, the anti-aliasing lock, the ambient occlusion at the render size), where
//!   `graphics::fsr_active` stays FSR's own (its sharpening, frame generation). Both go by the
//!   effective choice ([`effective`]).
//!
//! **Start** (step 8) [design]: the requested choice is the command line's
//! ([`command_line_choice`]: `--upscaler`, `--upscaler-quality`, the older `--fsr`), then the
//! settings' INI by the settings' own precedence (`settings::upscaler_after_ini`), read in
//! `main` before the renderer exists, because Streamline must be loaded before the Vulkan
//! instance is made. [`UpscalerStartup`] decides whether the DLSS loader runs: a DLSS choice,
//! or the four files in the DLSS folder (a cheap existence check, `dlss::bundle_present`; the
//! loader's own checks still decide), on Windows with Vulkan among the renderer's backends.
//! The test aid `NVF_DLSS_PRELOAD=1` always loads it, `=0` only for a DLSS choice. The same INI
//! lines are applied again with the other rows at `PostStartup`; both read them by the rows'
//! own rules, so they agree. [`follow_request`] then keeps the status.

pub(crate) mod prep;

use bevy::camera::Hdr;
use bevy::core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass};
use bevy::prelude::*;
use bevy::render::camera::{MipBias, TemporalJitter};

/// The backend kind (`--upscaler off|fsr|dlss`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum Upscaler {
    #[default]
    Off,
    Fsr,
    Dlss,
}

impl Upscaler {
    // The tests' list of every kind.
    #[cfg_attr(not(test), allow(dead_code))]
    pub const ALL: [Upscaler; 3] = [Upscaler::Off, Upscaler::Fsr, Upscaler::Dlss];

    /// The name for log lines (not a settings label: those come from the language files).
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Fsr => "FSR 3.1",
            Self::Dlss => "DLSS",
        }
    }
}

/// The qualities both upscalers offer, in the settings' order. Native AA reconstructs at 1:1
/// (DLSS: DLAA). The `clap` names (`native-aa`, `quality`, `balanced`, `performance`,
/// `ultra-performance`) are the ones `FsrQuality` had.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum UpscaleQuality {
    NativeAa,
    #[default]
    Quality,
    Balanced,
    Performance,
    UltraPerformance,
}

impl UpscaleQuality {
    // The tests' list of every quality.
    #[cfg_attr(not(test), allow(dead_code))]
    pub const ALL: [UpscaleQuality; 5] =
        [UpscaleQuality::NativeAa, UpscaleQuality::Quality, UpscaleQuality::Balanced, UpscaleQuality::Performance, UpscaleQuality::UltraPerformance];

    /// The name the Upscaling mode row saves, and the older `fsr-upscaling` row saved for FSR at
    /// this quality (`graphics::FSR_CHOICES`).
    pub fn name(self) -> &'static str {
        match self {
            Self::NativeAa => "Native AA",
            Self::Quality => "Quality",
            Self::Balanced => "Balanced",
            Self::Performance => "Performance",
            Self::UltraPerformance => "Ultra Performance",
        }
    }
}

/// Which DLSS Super Resolution model DLSS uses (the DLSS model row, `--dlss-model`,
/// `Graphics::dlss_model`): NVIDIA's own pick for each mode, or one of the four presets
/// offered, K, L, M and J, by the letters Streamline's header names them with (`ePresetJ` …)
/// [design: docs/design/dlss-upscaling.md, "The DLSS model"]. The others are left out: A to D
/// are removed, E and F deprecated, G, H, I, N and O "revert to default, not recommended"
/// [documented: Streamline 2.14.16 `include/sl_dlss.h:46-65`]. The number each sends is
/// `dlss::sizing::preset`'s. The `clap` names are `default`, `k`, `l`, `m`, `j`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum DlssModel {
    #[default]
    Default,
    K,
    L,
    M,
    J,
}

impl DlssModel {
    /// Every model, in the row's order: Default, K, L, M, J [design: the user's order,
    /// 2026-11-12]. K, L and M first as the defaults of the modes [documented: `sl_dlss.h:60-63`].
    pub const ALL: [DlssModel; 5] = [DlssModel::Default, DlssModel::K, DlssModel::L, DlssModel::M, DlssModel::J];

    /// The name the DLSS model row saves, shows and the log says: `Default` or the preset's
    /// letter (the row's label says they are DLSS models, so no "Preset" before it).
    pub fn name(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::K => "K",
            Self::L => "L",
            Self::M => "M",
            Self::J => "J",
        }
    }

    /// The model of this name (any case, spaces around it ignored), also by the names the row
    /// saved before (`Preset K` …, kept INIs load); `None`: not one of the five.
    pub fn named(name: &str) -> Option<Self> {
        let name = name.trim();
        let by_name = |n: &str| Self::ALL.into_iter().find(|m| m.name().eq_ignore_ascii_case(n));
        match name.get(..7).filter(|p| p.eq_ignore_ascii_case("preset ")) {
            // `Preset ` and a preset's letter; never `Preset Default`.
            Some(_) => by_name(name[7..].trim()).filter(|m| *m != Self::Default),
            None => by_name(name),
        }
    }
}

/// What the player asks for: the one requested upscaler setting (`Graphics::upscaler`). One
/// value, so FSR and DLSS can never both be asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UpscalerChoice {
    #[default]
    Off,
    Fsr(UpscaleQuality),
    Dlss(UpscaleQuality),
}

impl UpscalerChoice {
    pub fn new(upscaler: Upscaler, quality: UpscaleQuality) -> Self {
        match upscaler {
            Upscaler::Off => Self::Off,
            Upscaler::Fsr => Self::Fsr(quality),
            Upscaler::Dlss => Self::Dlss(quality),
        }
    }

    /// FSR at `quality`, or off: what the old `Graphics.fsr: Option<FsrQuality>` held.
    pub fn from_fsr(quality: Option<UpscaleQuality>) -> Self {
        quality.map_or(Self::Off, Self::Fsr)
    }

    pub fn upscaler(self) -> Upscaler {
        match self {
            Self::Off => Upscaler::Off,
            Self::Fsr(_) => Upscaler::Fsr,
            Self::Dlss(_) => Upscaler::Dlss,
        }
    }

    pub fn quality(self) -> Option<UpscaleQuality> {
        match self {
            Self::Off => None,
            Self::Fsr(q) | Self::Dlss(q) => Some(q),
        }
    }

    /// The quality when this is FSR; `None` for off and for DLSS.
    pub fn fsr(self) -> Option<UpscaleQuality> {
        match self {
            Self::Fsr(q) => Some(q),
            _ => None,
        }
    }

    /// The quality when this is DLSS.
    pub fn dlss(self) -> Option<UpscaleQuality> {
        match self {
            Self::Dlss(q) => Some(q),
            _ => None,
        }
    }

    pub fn is_dlss(self) -> bool {
        matches!(self, Self::Dlss(_))
    }

    /// For log lines: `Off`, `FSR 3.1 Quality`, `DLSS Native AA`.
    pub fn describe(self) -> String {
        match self.quality() {
            None => self.upscaler().name().to_string(),
            Some(q) => format!("{} {}", self.upscaler().name(), q.name()),
        }
    }
}

/// Why an upscaler cannot run. The keys are stable (logs, tests, the settings' reasons).
/// Most reasons are made only by the Windows loader (`dlss/win.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Unavailable {
    /// DLSS through Streamline is Windows only.
    NotWindows,
    /// The renderer's backends leave Vulkan out (`WGPU_BACKEND`), or the device is not Vulkan.
    NotVulkan,
    /// The DLSS folder lacks one of its four files (the log names them).
    FilesMissing,
    /// The DLSS folder holds an `nvngx_update.exe`, which Streamline would start unchecked
    /// (docs/spec-notes/streamline.md, "NVIDIA's updater"), or could not be inspected.
    UpdaterInFolder,
    /// The interposer's signature did not verify, or it is not a tested build (its SHA-256,
    /// `nvf-dlss-files` `security.rs` `TESTED_INTERPOSERS`), or a DLL in the folder is not a file of the
    /// tested release with its SHA-256 nor an `nvngx_*.dll` (`dlss/preflight.rs` `RELEASE_DLLS`).
    SignatureRejected,
    /// Streamline would not load or initialise (`LoadLibrary`, exports, `slInit`, the instance).
    StreamlineRefused,
    /// A GPU Bevy might pick lacks what Streamline's device proxy needs, or the pick cannot be
    /// told (the start-up device check).
    DeviceUnsuitable,
    /// DLSS is not supported on this GPU and driver (`slIsFeatureSupported`, no NVIDIA RTX GPU).
    GpuNotSupported,
    /// DLSS stopped producing output in this session (persistent evaluate failure,
    /// `dlss::session_state`).
    EvaluateFailed,
    /// FSR 3.1 cannot run here (not Vulkan, or the GPU lacks its formats and limits).
    FsrUnsupported,
}

/// A signature check's refusal (`nvf-dlss-files` `security`) as the reason DLSS is unavailable.
#[cfg(windows)]
impl From<nvf_dlss_files::security::Reason> for Unavailable {
    fn from(r: nvf_dlss_files::security::Reason) -> Self {
        match r {
            nvf_dlss_files::security::Reason::StreamlineRefused => Unavailable::StreamlineRefused,
            nvf_dlss_files::security::Reason::SignatureRejected => Unavailable::SignatureRejected,
        }
    }
}

impl Unavailable {
    pub fn key(self) -> &'static str {
        match self {
            Self::NotWindows => "not-windows",
            Self::NotVulkan => "not-vulkan",
            Self::FilesMissing => "files-missing",
            Self::UpdaterInFolder => "updater-in-folder",
            Self::SignatureRejected => "signature-rejected",
            Self::StreamlineRefused => "streamline-refused",
            Self::DeviceUnsuitable => "device-unsuitable",
            Self::GpuNotSupported => "gpu-not-supported",
            Self::EvaluateFailed => "evaluate-failed",
            Self::FsrUnsupported => "fsr-unsupported",
        }
    }

    /// The reason in the settings' words (English; the page translates it as it draws it,
    /// `lang/*.toml`). NVForever's own words [design].
    pub fn words(self) -> &'static str {
        match self {
            Self::NotWindows => "DLSS needs Windows",
            Self::NotVulkan => "DLSS needs the Vulkan renderer",
            Self::FilesMissing => "DLSS files missing: choose Download DLSS… in this list, or put them in the dlss folder beside nvforever.exe, then restart",
            Self::UpdaterInFolder => "The dlss folder holds NVIDIA's updater (nvngx_update.exe): remove it, then restart",
            Self::SignatureRejected => "A file in the dlss folder is not from the tested NVIDIA release (Streamline 2.14.16)",
            Self::StreamlineRefused => "DLSS did not start (the log says why)",
            Self::DeviceUnsuitable => "A graphics card the game may use cannot run DLSS",
            Self::GpuNotSupported => "Not on this GPU or driver (DLSS needs an NVIDIA RTX card)",
            Self::EvaluateFailed => "DLSS stopped working this session; it is tried again at the next start",
            Self::FsrUnsupported => "FSR 3.1 cannot run on this GPU",
        }
    }
}

/// Whether DLSS can run in this session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
// `Ready` is made only by the Windows loader (`dlss/win.rs`).
#[cfg_attr(not(windows), allow(dead_code))]
pub enum DlssState {
    /// Streamline was not loaded at start, and the files are in the DLSS folder now: a restart
    /// brings it ([`dlss_state_now`]).
    #[default]
    NotLoaded,
    /// Streamline is loaded, DLSS is supported and its functions were fetched.
    Ready,
    /// DLSS cannot run, for this reason.
    Unavailable(Unavailable),
}

/// What this session can do, filled at start and as things change ([`session_now`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Session {
    /// Streamline was loaded and initialised at start (the interposer made the device).
    pub streamline_loaded: bool,
    pub dlss: DlssState,
    /// Why FSR cannot run, or `None` when it can (`fsr::FsrSupport`).
    pub fsr: Option<Unavailable>,
}

/// What a requested choice comes to in a session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Resolution {
    /// What draws: the request, the choice kept until a restart, or `Off` (native) when the
    /// request cannot run.
    pub effective: UpscalerChoice,
    /// The request differs from what draws only until the next start.
    pub restart_pending: bool,
    /// Why the request (or the DLSS still drawing) cannot run.
    pub reason: Option<Unavailable>,
}

/// The rule from the requested choice to what draws [design: docs/design/dlss-upscaling.md,
/// "Switching"]. `previous` is the last effective choice (`None` at start).
///
/// - DLSS asked for: DLSS draws when it is ready; where it cannot run (also once it stopped
///   working this session, `EvaluateFailed`), native (`Off`) draws with the reason; where
///   Streamline was not loaded at start but the files are there now, what drew stays and a
///   restart is pending.
/// - FSR asked for: FSR draws at once where it can run (with Streamline loaded or not), else
///   native with the reason.
/// - Off: native at once.
pub fn resolve(requested: UpscalerChoice, previous: Option<UpscalerChoice>, session: Session) -> Resolution {
    let now = |effective: UpscalerChoice, reason: Option<Unavailable>| Resolution { effective, restart_pending: false, reason };
    match requested {
        UpscalerChoice::Dlss(_) => match session.dlss {
            DlssState::Ready => now(requested, None),
            DlssState::Unavailable(why) => now(UpscalerChoice::Off, Some(why)),
            DlssState::NotLoaded => {
                let drawing = previous.filter(|p| !p.is_dlss()).unwrap_or(UpscalerChoice::Off);
                Resolution { effective: drawing, restart_pending: true, reason: None }
            }
        },
        UpscalerChoice::Fsr(_) => match session.fsr {
            None => now(requested, None),
            Some(why) => now(UpscalerChoice::Off, Some(why)),
        },
        UpscalerChoice::Off => now(UpscalerChoice::Off, None),
    }
}

/// The session's facts and what the requested choice comes to (docs/design/dlss-upscaling.md,
/// "Internal interfaces"). The requested choice itself stays in `Graphics::upscaler`; the
/// status is kept by [`follow_request`].
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UpscalerStatus {
    pub session: Session,
    /// The last [`resolve`], `None` before the first.
    resolved: Option<Resolution>,
}

impl UpscalerStatus {
    pub fn new(session: Session) -> Self {
        Self { session, resolved: None }
    }

    /// Applies [`resolve`] to `requested`, the previous effective choice carried over. Returns
    /// whether anything changed.
    pub fn update(&mut self, requested: UpscalerChoice) -> bool {
        let next = resolve(requested, self.resolved.map(|r| r.effective), self.session);
        let changed = self.resolved != Some(next);
        self.resolved = Some(next);
        changed
    }

    /// What draws (`Off` before the first [`Self::update`]).
    pub fn effective(&self) -> UpscalerChoice {
        self.resolved.map_or(UpscalerChoice::Off, |r| r.effective)
    }

    pub fn restart_pending(&self) -> bool {
        self.resolved.is_some_and(|r| r.restart_pending)
    }

    pub fn reason(&self) -> Option<Unavailable> {
        self.resolved.and_then(|r| r.reason)
    }

    /// Why DLSS cannot be asked for in this session (the settings hold it out of reach), by the
    /// rule [`resolve`] uses; `None` when it can, or when a restart brings it.
    pub fn dlss_unavailable(&self) -> Option<Unavailable> {
        match self.session.dlss {
            DlssState::Unavailable(why) => Some(why),
            DlssState::Ready | DlssState::NotLoaded => None,
        }
    }

    /// For the log: what is asked for, what draws, a restart pending and the reason.
    pub fn describe(&self, requested: UpscalerChoice) -> String {
        let mut line = format!("{} asked for, {} draws", requested.describe(), self.effective().describe());
        if self.restart_pending() {
            line.push_str(" until the next start");
        }
        if let Some(why) = self.reason() {
            line.push_str(&format!(" ({})", why.key()));
        }
        line
    }
}

/// Why the DLSS loader runs at start or not ([`UpscalerStartup`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartLoad {
    /// DLSS is the start's choice: the loader runs whatever the folder holds (and says why DLSS
    /// cannot run, when it cannot).
    DlssChosen,
    /// The four files are in the DLSS folder: the loader runs, so every switch is live.
    FilesPresent,
    /// `NVF_DLSS_PRELOAD=1`: the loader runs whatever the folder holds.
    PreloadOn,
    /// `NVF_DLSS_PRELOAD=0`: not loaded without a DLSS choice (DLSS then takes a restart).
    PreloadOff,
    /// The DLSS folder lacks one of the four files (or there is none): not loaded.
    NoFiles,
    /// Not Windows, or Vulkan is not among the renderer's backends: not loaded.
    Unsupported,
}

/// What the start-up chose, before the renderer was made (`main`; module notes): the
/// requested choice the start resolved and whether, and why, the DLSS loader runs.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpscalerStartup {
    pub requested: UpscalerChoice,
    pub load: StartLoad,
}

/// The test aid for the start's decision: `1` always runs the DLSS loader (FSR then draws
/// through the interposer), `0` runs it only for a DLSS choice; unset (or anything else), the
/// files decide.
pub const PRELOAD_ENV: &str = "NVF_DLSS_PRELOAD";

impl UpscalerStartup {
    /// `preload_var`: `NVF_DLSS_PRELOAD`'s value; `supported`: Windows with Vulkan among the
    /// renderer's backends ([`platform_supports_dlss`]); `files_present`: the cheap check of
    /// the DLSS folder (`dlss::bundle_present`), asked only when it decides.
    pub fn new(requested: UpscalerChoice, preload_var: Option<&std::ffi::OsStr>, supported: bool, files_present: impl FnOnce() -> bool) -> Self {
        let load = if requested.is_dlss() {
            StartLoad::DlssChosen
        } else if preload_var.is_some_and(|v| v == "1") {
            StartLoad::PreloadOn
        } else if preload_var.is_some_and(|v| v == "0") {
            StartLoad::PreloadOff
        } else if !supported {
            StartLoad::Unsupported
        } else if files_present() {
            StartLoad::FilesPresent
        } else {
            StartLoad::NoFiles
        };
        Self { requested, load }
    }

    /// The DLSS loader runs before the renderer (`main` adds `dlss::DlssInitPlugin`).
    pub fn loads_streamline(&self) -> bool {
        matches!(self.load, StartLoad::DlssChosen | StartLoad::FilesPresent | StartLoad::PreloadOn)
    }

    /// For the start's log line.
    pub fn describe(&self) -> String {
        let chosen = self.requested.describe();
        match self.load {
            StartLoad::DlssChosen => format!("{chosen} chosen at start: Streamline loaded before the renderer"),
            StartLoad::FilesPresent => format!("{chosen} chosen at start; the DLSS files are in the dlss folder: Streamline loaded before the renderer, so DLSS switches without a restart"),
            StartLoad::PreloadOn => format!("{chosen} chosen at start; {PRELOAD_ENV}=1: Streamline loaded before the renderer all the same"),
            StartLoad::PreloadOff => format!("{chosen} chosen at start; {PRELOAD_ENV}=0: Streamline not loaded (DLSS takes a restart)"),
            StartLoad::NoFiles => format!("{chosen} chosen at start; no DLSS files in the dlss folder: Streamline not loaded"),
            StartLoad::Unsupported => format!("{chosen} chosen at start: Streamline not loaded (DLSS needs Windows and the Vulkan renderer)"),
        }
    }
}

/// Whether DLSS could be loaded on this platform at all: Windows, with Vulkan among the
/// renderer's backends.
pub fn platform_supports_dlss() -> bool {
    cfg!(windows) && vulkan_in_backends()
}

/// The requested choice from the command line: `--upscaler off|fsr|dlss` with
/// `--upscaler-quality <q>` (default `quality`), or the older `--fsr <mode>` (`None`: not
/// given, unlike `--fsr off`). Both given must say the same: `--fsr quality --upscaler fsr`
/// agree, `--fsr quality --upscaler dlss` or `--fsr off --upscaler dlss` do not.
pub fn command_line_choice(fsr: Option<crate::graphics::FsrMode>, upscaler: Option<Upscaler>, quality: Option<UpscaleQuality>) -> Result<UpscalerChoice, String> {
    let from_fsr = fsr.map(|m| UpscalerChoice::from_fsr(m.quality()));
    let Some(upscaler) = upscaler else {
        if quality.is_some() {
            return Err("--upscaler-quality needs --upscaler fsr or --upscaler dlss".into());
        }
        return Ok(from_fsr.unwrap_or_default());
    };
    if upscaler == Upscaler::Off && quality.is_some() {
        return Err("--upscaler off takes no --upscaler-quality".into());
    }
    let chosen = UpscalerChoice::new(upscaler, quality.or_else(|| from_fsr.and_then(UpscalerChoice::quality).filter(|_| upscaler == Upscaler::Fsr)).unwrap_or_default());
    match from_fsr {
        Some(legacy) if legacy != chosen => Err(format!(
            "--fsr asks for {} but --upscaler for {}: give one of them, or both alike",
            legacy.describe(),
            chosen.describe()
        )),
        _ => Ok(chosen),
    }
}

/// What DLSS can do now: the session's state once the loader ran (`dlss::session_state`: the
/// loader's, or `EvaluateFailed` once DLSS's own pass gave up on it), else what is known
/// without loading anything (not Windows, Vulkan left out of the renderer's backends, the
/// files not in the DLSS folder now).
pub fn dlss_state_now() -> DlssState {
    state_now(crate::dlss::session_state(), cfg!(windows), vulkan_in_backends(), crate::dlss::loader_ran(), files_now)
}

/// [`dlss_state_now`]'s rule over its inputs. `loader_ran`: the DLSS loader ran at start;
/// `files`: the four files are in the DLSS folder now (asked only when it decides).
///
/// A loader that stopped at missing files (`FilesMissing`) stopped before it loaded the
/// interposer (`dlss/win.rs` `start` checks the folder first), so nothing of Streamline is in
/// the process and the files are looked at again as for a loader that never ran: once all four
/// are there, a restart brings DLSS. Every other reason the loader stopped at (the updater, the
/// signature, the device check, Streamline refusing) and DLSS given up on later stay for the
/// session.
fn state_now(session: DlssState, windows: bool, vulkan: bool, loader_ran: bool, files: impl FnOnce() -> bool) -> DlssState {
    match session {
        DlssState::NotLoaded => match before_loading(windows, vulkan) {
            // The loader ran and left nothing else: something refused it before it said why (a
            // restart would do the same).
            DlssState::NotLoaded if loader_ran => DlssState::Unavailable(Unavailable::StreamlineRefused),
            // Not loaded at start: a restart brings DLSS once the files are there, not before.
            DlssState::NotLoaded if !files() => DlssState::Unavailable(Unavailable::FilesMissing),
            state => state,
        },
        // The files put there since the start: a restart brings DLSS.
        DlssState::Unavailable(Unavailable::FilesMissing) if files() => DlssState::NotLoaded,
        state => state,
    }
}

/// Whether the four files are in the DLSS folder, looked at most once a second (the status asks
/// every frame while Streamline is not loaded; `dlss::bundle_present`).
fn files_now() -> bool {
    use std::sync::{Mutex, PoisonError};
    use std::time::{Duration, Instant};
    static SEEN: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let mut seen = SEEN.lock().unwrap_or_else(PoisonError::into_inner);
    match *seen {
        Some((at, present)) if at.elapsed() < Duration::from_secs(1) => present,
        _ => {
            let present = crate::dlss::bundle_present();
            *seen = Some((Instant::now(), present));
            present
        }
    }
}

/// The renderer's backends include Vulkan, read as Bevy reads them (`WGPU_BACKEND`, as
/// `dlss/win.rs` `start` does).
fn vulkan_in_backends() -> bool {
    static VULKAN: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| wgpu::Backends::from_env().unwrap_or(wgpu::Backends::all()).contains(wgpu::Backends::VULKAN));
    *VULKAN
}

/// DLSS's state in a session that did not load Streamline, by the platform alone: unavailable
/// where no restart could bring it, else not loaded.
fn before_loading(windows: bool, vulkan: bool) -> DlssState {
    if !windows {
        DlssState::Unavailable(Unavailable::NotWindows)
    } else if !vulkan {
        DlssState::Unavailable(Unavailable::NotVulkan)
    } else {
        DlssState::NotLoaded
    }
}

/// The session's facts now: DLSS as [`dlss_state_now`] says, FSR as `fsr::FsrSupport` says,
/// and whether the device came through Streamline (`dlss::streamline_loaded`, which stays true
/// when DLSS turned out unsupported or stopped working).
pub fn session_now(fsr_supported: bool) -> Session {
    Session { streamline_loaded: crate::dlss::streamline_loaded(), dlss: dlss_state_now(), fsr: (!fsr_supported).then_some(Unavailable::FsrUnsupported) }
}

/// Keeps [`UpscalerStatus`] with the request (`Graphics::upscaler`) and the session's facts,
/// before `graphics::apply` puts the effective choice on the camera. Made on the first frame
/// (after the INI is applied at `PostStartup`).
pub fn follow_request(mut commands: Commands, graphics: Res<crate::graphics::Graphics>, fsr: Option<Res<crate::fsr::FsrSupport>>, status: Option<ResMut<UpscalerStatus>>, thermal: Option<Res<crate::mobile_performance::ThermalRuntime>>) {
    let session = session_now(fsr.as_deref().is_some_and(|s| s.0));
    let requested = thermal.as_deref().map_or(graphics.upscaler, |t| t.render_request(graphics.upscaler, fsr.as_deref().is_some_and(|s| s.0)));
    match status {
        None => {
            let mut made = UpscalerStatus::new(session);
            made.update(requested);
            info!("upscaler: {}", made.describe(requested));
            commands.insert_resource(made);
        }
        Some(mut status) => {
            let mut next = *status;
            next.session = session;
            let changed = next.update(requested);
            if changed {
                info!("upscaler: {}", next.describe(requested));
            }
            if next != *status {
                *status = next;
            }
        }
    }
}

/// DLSS on a perspective HDR camera, the counterpart of `fsr::FsrUpscaling`: the common
/// preparation ([`prep`]) takes the camera, and DLSS draws it in its own pass
/// (`dlss::dlss_pass`). The graphics settings put it on the main camera while DLSS is the
/// effective choice ([`UpscalerStatus::effective`]) and enforce `Msaa::Off` and no other
/// anti-aliasing, as for FSR. DLSS sharpens nothing itself: the separate sharpening pass runs
/// for it as it does natively (`graphics::apply`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(Hdr, DepthPrepass, MotionVectorPrepass, TemporalJitter, MipBias)]
pub struct DlssUpscaling {
    pub quality: UpscaleQuality,
}

/// What the render world last drew the main camera with (the first view with an upscaler
/// camera, `prep::prepare`), for the performance overlay's Upscaling row (#2242). The sizes
/// exist only in the render world, which prepares the view a frame after the main world asked
/// for it, so the row reads this and the main world's effective choice side by side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawnUpscale {
    /// The upscaler drew it: the scene at `render`, rebuilt to `output` (physical pixels).
    Upscaled { choice: UpscalerChoice, render: UVec2, output: UVec2 },
    /// The camera asked for `choice` but the view rendered natively at `output`: a view the
    /// upscaler cannot use (multisampled, not HDR, offset), or a backend that gave no render
    /// size (FSR below its smallest size, DLSS's optimal settings failing).
    Native { choice: UpscalerChoice, output: UVec2 },
}

/// [`DrawnUpscale`] shared by the render world, which writes it each frame (`None`: no camera
/// asked for an upscaler), and the main world's performance overlay, which reads it: one
/// `Arc` on each side, as `frame_gen::GeneratedFrames` is.
#[derive(Resource, Clone, Default)]
pub struct UpscaleDrawn(pub std::sync::Arc<std::sync::Mutex<Option<DrawnUpscale>>>);

impl UpscaleDrawn {
    /// What was drawn last (a poisoned lock still reads: the value is a plain copy).
    pub fn get(&self) -> Option<DrawnUpscale> {
        *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set(&self, drawn: Option<DrawnUpscale>) {
        *self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = drawn;
    }
}

/// The temporal upscaler the main camera draws with: the status's effective choice, for every
/// backend, once there is a status; without one (before [`follow_request`] makes it, and in
/// tests) FSR's rule as it always was: FSR while it is asked for and `fsr_supported`, else off.
pub fn effective_temporal(requested_fsr: Option<UpscaleQuality>, fsr_supported: bool, status: Option<&UpscalerStatus>) -> UpscalerChoice {
    match status {
        Some(status) => status.effective(),
        None => UpscalerChoice::from_fsr(requested_fsr.filter(|_| fsr_supported)),
    }
}

/// [`effective_temporal`] for this world.
pub fn effective(world: &World) -> UpscalerChoice {
    let fsr_supported = world.get_resource::<crate::fsr::FsrSupport>().is_some_and(|s| s.0);
    effective_temporal(world.resource::<crate::graphics::Graphics>().fsr(), fsr_supported, world.get_resource::<UpscalerStatus>())
}

/// Whether a temporal upscaler (FSR or DLSS) draws the main camera on this device: the
/// question for what both need (the HDR view, the prepasses, cut-outs drawn alpha-tested, the
/// native anti-aliasing replaced). `graphics::fsr_active` is FSR's own.
pub fn temporal_active(world: &World) -> bool {
    effective(world) != UpscalerChoice::Off
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::ValueEnum;
    use UpscaleQuality::*;

    const READY: Session = Session { streamline_loaded: true, dlss: DlssState::Ready, fsr: None };
    const STOCK: Session = Session { streamline_loaded: false, dlss: DlssState::NotLoaded, fsr: None };

    fn ok(effective: UpscalerChoice) -> Resolution {
        Resolution { effective, restart_pending: false, reason: None }
    }

    #[test]
    fn off_is_the_default_everywhere() {
        assert_eq!(Upscaler::default(), Upscaler::Off);
        assert_eq!(UpscalerChoice::default(), UpscalerChoice::Off);
        assert_eq!(UpscaleQuality::default(), Quality, "FSR's old default quality");
        assert_eq!(UpscalerStatus::default().effective(), UpscalerChoice::Off);
        assert!(!UpscalerStatus::default().restart_pending());
        assert_eq!(Session::default().dlss, DlssState::NotLoaded);
    }

    #[test]
    fn command_line_names_parse() {
        for (name, upscaler) in [("off", Upscaler::Off), ("fsr", Upscaler::Fsr), ("dlss", Upscaler::Dlss)] {
            assert_eq!(Upscaler::from_str(name, true), Ok(upscaler));
        }
        assert!(Upscaler::from_str("xess", true).is_err());
        let names: Vec<String> = UpscaleQuality::value_variants().iter().map(|q| q.to_possible_value().unwrap().get_name().to_string()).collect();
        assert_eq!(names, ["native-aa", "quality", "balanced", "performance", "ultra-performance"], "the names FsrQuality had");
        assert_eq!(UpscaleQuality::value_variants(), UpscaleQuality::ALL);
    }

    #[test]
    fn quality_names_are_the_fsr_rows_saved_names() {
        let fsr: Vec<&str> = crate::graphics::FSR_CHOICES.iter().filter(|(_, m)| m.quality().is_some()).map(|(n, _)| *n).collect();
        let ours: Vec<&str> = UpscaleQuality::ALL.iter().map(|q| q.name()).collect();
        assert_eq!(fsr, ours);
        for (name, mode) in crate::graphics::FSR_CHOICES {
            if let Some(q) = mode.quality() {
                assert_eq!(q.name(), name);
            }
        }
    }

    #[test]
    fn choice_round_trips_its_parts() {
        for upscaler in Upscaler::ALL {
            for quality in UpscaleQuality::ALL {
                let choice = UpscalerChoice::new(upscaler, quality);
                assert_eq!(choice.upscaler(), upscaler);
                assert_eq!(choice.quality(), (upscaler != Upscaler::Off).then_some(quality));
                assert_eq!(choice.fsr(), (upscaler == Upscaler::Fsr).then_some(quality));
                assert_eq!(choice.dlss(), (upscaler == Upscaler::Dlss).then_some(quality));
                assert_eq!(choice.is_dlss(), upscaler == Upscaler::Dlss);
            }
        }
        assert_eq!(UpscalerChoice::from_fsr(None), UpscalerChoice::Off);
        assert_eq!(UpscalerChoice::from_fsr(Some(Balanced)), UpscalerChoice::Fsr(Balanced));
        assert_eq!(UpscalerChoice::from_fsr(Some(Performance)).fsr(), Some(Performance));
        assert_eq!(UpscalerChoice::Off.describe(), "Off");
        assert_eq!(UpscalerChoice::Fsr(Quality).describe(), "FSR 3.1 Quality");
        assert_eq!(UpscalerChoice::Dlss(NativeAa).describe(), "DLSS Native AA");
    }

    #[test]
    fn reason_keys_are_distinct() {
        let all = [
            Unavailable::NotWindows,
            Unavailable::NotVulkan,
            Unavailable::FilesMissing,
            Unavailable::UpdaterInFolder,
            Unavailable::SignatureRejected,
            Unavailable::StreamlineRefused,
            Unavailable::DeviceUnsuitable,
            Unavailable::GpuNotSupported,
            Unavailable::EvaluateFailed,
            Unavailable::FsrUnsupported,
        ];
        let keys: std::collections::HashSet<&str> = all.iter().map(|r| r.key()).collect();
        assert_eq!(keys.len(), all.len());
    }

    /// Streamline loaded at start whatever was chosen (the DLSS files in the folder, #2198's
    /// play-test): off, FSR and DLSS all follow at once.
    #[test]
    fn with_streamline_loaded_every_switch_is_live() {
        let mut status = UpscalerStatus::new(READY);
        for requested in [
            UpscalerChoice::Fsr(Quality),
            UpscalerChoice::Dlss(Quality),
            UpscalerChoice::Dlss(Performance),
            UpscalerChoice::Off,
            UpscalerChoice::Dlss(NativeAa),
            UpscalerChoice::Fsr(Balanced),
            UpscalerChoice::Off,
            UpscalerChoice::Fsr(UltraPerformance),
        ] {
            status.update(requested);
            assert_eq!((status.effective(), status.restart_pending(), status.reason()), (requested, false, None), "{requested:?} draws at once");
        }
        assert_eq!(status.dlss_unavailable(), None);
    }

    #[test]
    fn at_start_the_request_runs_when_its_backend_can() {
        let at_start = |requested: UpscalerChoice, session: Session| resolve(requested, None, session);
        assert_eq!(at_start(UpscalerChoice::Off, STOCK), ok(UpscalerChoice::Off));
        assert_eq!(at_start(UpscalerChoice::Fsr(Quality), STOCK), ok(UpscalerChoice::Fsr(Quality)));
        assert_eq!(at_start(UpscalerChoice::Dlss(Balanced), READY), ok(UpscalerChoice::Dlss(Balanced)));
        let no_fsr = Session { fsr: Some(Unavailable::FsrUnsupported), ..STOCK };
        assert_eq!(at_start(UpscalerChoice::Fsr(Quality), no_fsr), Resolution { effective: UpscalerChoice::Off, restart_pending: false, reason: Some(Unavailable::FsrUnsupported) });
        let missing = Session { dlss: DlssState::Unavailable(Unavailable::FilesMissing), ..STOCK };
        assert_eq!(at_start(UpscalerChoice::Dlss(Quality), missing), Resolution { effective: UpscalerChoice::Off, restart_pending: false, reason: Some(Unavailable::FilesMissing) }, "native, not FSR");
        // FSR with Streamline loaded at start (the files in the folder, or the preload aid).
        assert_eq!(at_start(UpscalerChoice::Fsr(Performance), READY), ok(UpscalerChoice::Fsr(Performance)));
        // Streamline loaded but DLSS refused once the device existed: FSR and off still run.
        let refused = Session { dlss: DlssState::Unavailable(Unavailable::GpuNotSupported), ..READY };
        assert_eq!(at_start(UpscalerChoice::Fsr(Quality), refused), ok(UpscalerChoice::Fsr(Quality)));
        assert_eq!(at_start(UpscalerChoice::Dlss(Quality), refused).reason, Some(Unavailable::GpuNotSupported));
    }

    /// The restart that remains: DLSS asked for in a session that started without Streamline
    /// while the files are in the folder now (added since, or `NVF_DLSS_PRELOAD=0`).
    #[test]
    fn dlss_without_streamline_loaded_keeps_what_draws_until_a_restart() {
        for previous in [UpscalerChoice::Off, UpscalerChoice::Fsr(Balanced)] {
            assert_eq!(resolve(UpscalerChoice::Dlss(Quality), Some(previous), STOCK), Resolution { effective: previous, restart_pending: true, reason: None });
            let refused = Session { dlss: DlssState::Unavailable(Unavailable::NotVulkan), ..STOCK };
            assert_eq!(resolve(UpscalerChoice::Dlss(Quality), Some(previous), refused), Resolution { effective: UpscalerChoice::Off, restart_pending: false, reason: Some(Unavailable::NotVulkan) }, "a restart would not help: native with the reason");
        }
        // Off and FSR in the same session: at once, nothing pending.
        assert_eq!(resolve(UpscalerChoice::Off, Some(UpscalerChoice::Fsr(Quality)), STOCK), ok(UpscalerChoice::Off));
        assert_eq!(resolve(UpscalerChoice::Fsr(UltraPerformance), Some(UpscalerChoice::Off), STOCK), ok(UpscalerChoice::Fsr(UltraPerformance)));
        let mut status = UpscalerStatus::new(STOCK);
        status.update(UpscalerChoice::Fsr(Quality));
        status.update(UpscalerChoice::Dlss(Balanced));
        assert!(status.restart_pending());
        assert!(status.describe(UpscalerChoice::Dlss(Balanced)).ends_with("DLSS Balanced asked for, FSR 3.1 Quality draws until the next start"));
        status.update(UpscalerChoice::Fsr(Quality));
        assert!(!status.restart_pending(), "back to FSR: nothing pending");
    }

    #[test]
    fn dlss_that_stops_working_falls_back_to_native_and_fsr_still_follows() {
        let failed = Session { dlss: DlssState::Unavailable(Unavailable::EvaluateFailed), ..READY };
        let running = Some(UpscalerChoice::Dlss(Quality));
        assert_eq!(resolve(UpscalerChoice::Dlss(Quality), running, failed), Resolution { effective: UpscalerChoice::Off, restart_pending: false, reason: Some(Unavailable::EvaluateFailed) });
        assert_eq!(resolve(UpscalerChoice::Fsr(Quality), running, failed), ok(UpscalerChoice::Fsr(Quality)), "FSR runs through the interposer");
        assert_eq!(resolve(UpscalerChoice::Off, running, failed), ok(UpscalerChoice::Off));
    }

    /// Review P2 (2026-11-12), with live switches: after DLSS fails, DLSS stays out for the rest
    /// of the session with its reason, however often it is asked for again and whatever runs in
    /// between; nothing waits for a restart.
    #[test]
    fn repeated_updates_after_a_dlss_failure_keep_the_reason() {
        let mut status = UpscalerStatus::new(READY);
        assert!(status.update(UpscalerChoice::Dlss(Quality)));
        assert_eq!(status.effective(), UpscalerChoice::Dlss(Quality));
        status.session.dlss = DlssState::Unavailable(Unavailable::EvaluateFailed);
        assert!(status.update(UpscalerChoice::Dlss(Quality)));
        assert_eq!((status.effective(), status.restart_pending(), status.reason()), (UpscalerChoice::Off, false, Some(Unavailable::EvaluateFailed)));
        for _ in 0..3 {
            for requested in [UpscalerChoice::Off, UpscalerChoice::Fsr(Quality), UpscalerChoice::Fsr(Balanced)] {
                status.update(requested);
                assert_eq!((status.effective(), status.restart_pending(), status.reason()), (requested, false, None), "{requested:?}");
                assert_eq!(status.dlss_unavailable(), Some(Unavailable::EvaluateFailed), "{requested:?}: DLSS still held out with its reason");
            }
            status.update(UpscalerChoice::Dlss(Performance));
            assert_eq!((status.effective(), status.restart_pending(), status.reason()), (UpscalerChoice::Off, false, Some(Unavailable::EvaluateFailed)), "back to DLSS: native, the reason again");
        }
    }

    #[test]
    fn dlss_is_unavailable_to_the_settings_by_the_same_rule() {
        assert_eq!(UpscalerStatus::new(STOCK).dlss_unavailable(), None, "a restart brings it");
        assert_eq!(UpscalerStatus::new(READY).dlss_unavailable(), None);
        let missing = Session { dlss: DlssState::Unavailable(Unavailable::FilesMissing), ..STOCK };
        assert_eq!(UpscalerStatus::new(missing).dlss_unavailable(), Some(Unavailable::FilesMissing));
        let refused = Session { dlss: DlssState::Unavailable(Unavailable::StreamlineRefused), ..STOCK };
        assert_eq!(UpscalerStatus::new(refused).dlss_unavailable(), Some(Unavailable::StreamlineRefused));
    }

    /// Review P2 (2026-11-12): the status's effective choice, not the request, draws the camera.
    #[test]
    fn the_camera_draws_the_status_effective_choice_for_every_backend() {
        // No status (before follow_request makes it, and tests without one): FSR's rule.
        assert_eq!(effective_temporal(Some(Quality), true, None), UpscalerChoice::Fsr(Quality));
        assert_eq!(effective_temporal(Some(Quality), false, None), UpscalerChoice::Off, "unsupported FSR draws natively");
        assert_eq!(effective_temporal(None, true, None), UpscalerChoice::Off);
        // DLSS drawing, then FSR asked for: FSR at once, never both.
        let mut status = UpscalerStatus::new(READY);
        status.update(UpscalerChoice::Dlss(Balanced));
        assert_eq!(effective_temporal(None, true, Some(&status)), UpscalerChoice::Dlss(Balanced));
        status.update(UpscalerChoice::Fsr(Quality));
        assert_eq!(effective_temporal(Some(Quality), true, Some(&status)), UpscalerChoice::Fsr(Quality));
        // FSR drawing, DLSS asked for without Streamline loaded: the request's FSR quality is gone
        // (Graphics::fsr is None) but FSR keeps drawing until the restart, as the status says.
        let mut stock = UpscalerStatus::new(STOCK);
        stock.update(UpscalerChoice::Fsr(Balanced));
        assert_eq!(effective_temporal(Some(Balanced), true, Some(&stock)), UpscalerChoice::Fsr(Balanced));
        stock.update(UpscalerChoice::Dlss(Quality));
        assert!(stock.restart_pending());
        assert_eq!(effective_temporal(None, true, Some(&stock)), UpscalerChoice::Fsr(Balanced), "restart pending: FSR keeps drawing");
        stock.update(UpscalerChoice::Off);
        assert_eq!(effective_temporal(None, true, Some(&stock)), UpscalerChoice::Off);
    }

    #[test]
    fn status_carries_the_effective_choice_over() {
        let mut status = UpscalerStatus::new(READY);
        assert!(status.update(UpscalerChoice::Dlss(Quality)));
        assert!(!status.update(UpscalerChoice::Dlss(Quality)), "nothing changed");
        assert!(status.update(UpscalerChoice::Off));
        assert_eq!(status.effective(), UpscalerChoice::Off);
        assert!(!status.restart_pending());
        assert!(status.update(UpscalerChoice::Dlss(Balanced)));
        assert_eq!(status.effective(), UpscalerChoice::Dlss(Balanced));
        assert_eq!(status.reason(), None);
        assert!(status.describe(UpscalerChoice::Dlss(Balanced)).contains("DLSS Balanced draws"));
    }

    #[test]
    fn dlss_unavailable_before_loading_only_where_no_restart_brings_it() {
        assert_eq!(before_loading(false, true), DlssState::Unavailable(Unavailable::NotWindows));
        assert_eq!(before_loading(true, false), DlssState::Unavailable(Unavailable::NotVulkan));
        assert_eq!(before_loading(true, true), DlssState::NotLoaded);
        let session = Session { streamline_loaded: false, dlss: before_loading(true, false), fsr: None };
        assert_eq!(resolve(UpscalerChoice::Dlss(Quality), Some(UpscalerChoice::Off), session).reason, Some(Unavailable::NotVulkan));
        assert_eq!(session_now(false).fsr, Some(Unavailable::FsrUnsupported));
        assert_eq!(session_now(true).fsr, None);
        assert!(!session_now(true).streamline_loaded, "no loader ran in the tests");
    }

    /// Without Streamline loaded at start, the files decide: there now, a restart brings DLSS
    /// (`NotLoaded`); missing, DLSS is held out with that reason (a restart alone would not
    /// help). A loader that ran and left nothing else says Streamline refused.
    #[test]
    fn without_streamline_the_files_now_decide_between_a_restart_and_files_missing() {
        let never = || -> bool { panic!("the files are not asked about here") };
        assert_eq!(state_now(DlssState::NotLoaded, true, true, false, || true), DlssState::NotLoaded);
        assert_eq!(state_now(DlssState::NotLoaded, true, true, false, || false), DlssState::Unavailable(Unavailable::FilesMissing));
        assert_eq!(state_now(DlssState::NotLoaded, true, true, true, never), DlssState::Unavailable(Unavailable::StreamlineRefused));
        assert_eq!(state_now(DlssState::NotLoaded, false, true, false, never), DlssState::Unavailable(Unavailable::NotWindows));
        assert_eq!(state_now(DlssState::NotLoaded, true, false, false, never), DlssState::Unavailable(Unavailable::NotVulkan));
        assert_eq!(state_now(DlssState::Ready, true, true, true, never), DlssState::Ready);
    }

    /// The loader ran with DLSS chosen and stopped at missing files (before the interposer):
    /// files put there since are found by the same once-a-second look, and DLSS becomes a
    /// restart away (`NotLoaded`), in reach again. The loader's other reasons and DLSS given up
    /// on stay for the session, whatever the folder holds.
    #[test]
    fn files_added_after_a_start_that_missed_them_bring_dlss_at_the_next_start() {
        let missing = DlssState::Unavailable(Unavailable::FilesMissing);
        assert_eq!(state_now(missing, true, true, true, || false), missing, "still missing");
        assert_eq!(state_now(missing, true, true, true, || true), DlssState::NotLoaded, "there now: a restart brings DLSS");
        let never = || -> bool { panic!("the files are not asked about here") };
        for why in [
            Unavailable::UpdaterInFolder,
            Unavailable::SignatureRejected,
            Unavailable::StreamlineRefused,
            Unavailable::DeviceUnsuitable,
            Unavailable::GpuNotSupported,
            Unavailable::EvaluateFailed,
            Unavailable::NotVulkan,
        ] {
            assert_eq!(state_now(DlssState::Unavailable(why), true, true, true, never), DlssState::Unavailable(why), "{why:?} stays");
        }
        // What the status makes of it: missing, DLSS asked for draws natively with the reason;
        // the files there, what drew (FSR) keeps drawing until the restart, no reason left.
        let mut status = UpscalerStatus::new(Session { streamline_loaded: false, dlss: missing, fsr: None });
        status.update(UpscalerChoice::Fsr(Quality));
        status.update(UpscalerChoice::Dlss(Quality));
        assert_eq!((status.effective(), status.reason()), (UpscalerChoice::Off, Some(Unavailable::FilesMissing)), "missing: native with the reason");
        assert_eq!(status.dlss_unavailable(), Some(Unavailable::FilesMissing));
        status.session.dlss = state_now(missing, true, true, true, || true);
        status.update(UpscalerChoice::Fsr(Quality));
        status.update(UpscalerChoice::Dlss(Quality));
        assert_eq!((status.effective(), status.restart_pending(), status.reason()), (UpscalerChoice::Fsr(Quality), true, None));
        assert_eq!(status.dlss_unavailable(), None, "in reach again");
    }

    /// DLSS's own pass giving up (`dlss::session_state` says `EvaluateFailed`) reaches the
    /// status whatever the platform checks say: the camera draws natively, not with FSR, and the
    /// reason is kept.
    #[test]
    fn evaluate_failed_reaches_the_status_and_the_camera_draws_natively() {
        let failed = DlssState::Unavailable(Unavailable::EvaluateFailed);
        for (windows, vulkan) in [(true, true), (true, false), (false, true)] {
            assert_eq!(state_now(failed, windows, vulkan, true, || true), failed);
            assert_eq!(state_now(DlssState::Ready, windows, vulkan, true, || true), DlssState::Ready);
        }
        // What follow_request does: the session refreshed every frame.
        let mut status = UpscalerStatus::new(READY);
        status.update(UpscalerChoice::Dlss(Balanced));
        assert_eq!(effective_temporal(None, true, Some(&status)), UpscalerChoice::Dlss(Balanced));
        status.session = Session { streamline_loaded: true, dlss: state_now(failed, true, true, true, || true), fsr: None };
        assert!(status.update(UpscalerChoice::Dlss(Balanced)));
        assert_eq!(effective_temporal(None, true, Some(&status)), UpscalerChoice::Off, "native, not FSR");
        assert_eq!((status.restart_pending(), status.reason()), (false, Some(Unavailable::EvaluateFailed)));
        assert!(status.describe(UpscalerChoice::Dlss(Balanced)).ends_with("Off draws (evaluate-failed)"));
    }

    #[test]
    fn the_command_line_choice_and_its_conflicts() {
        use crate::graphics::FsrMode;
        let cli = command_line_choice;
        assert_eq!(cli(None, None, None), Ok(UpscalerChoice::Off), "nothing given: off");
        assert_eq!(cli(Some(FsrMode::Balanced), None, None), Ok(UpscalerChoice::Fsr(Balanced)), "--fsr as before");
        assert_eq!(cli(Some(FsrMode::Off), None, None), Ok(UpscalerChoice::Off));
        assert_eq!(cli(None, Some(Upscaler::Dlss), None), Ok(UpscalerChoice::Dlss(Quality)), "quality by default");
        assert_eq!(cli(None, Some(Upscaler::Dlss), Some(UltraPerformance)), Ok(UpscalerChoice::Dlss(UltraPerformance)));
        assert_eq!(cli(None, Some(Upscaler::Fsr), Some(NativeAa)), Ok(UpscalerChoice::Fsr(NativeAa)));
        assert_eq!(cli(None, Some(Upscaler::Off), None), Ok(UpscalerChoice::Off));
        // Both selectors given and agreeing.
        assert_eq!(cli(Some(FsrMode::Performance), Some(Upscaler::Fsr), None), Ok(UpscalerChoice::Fsr(Performance)), "--upscaler fsr takes --fsr's quality");
        assert_eq!(cli(Some(FsrMode::Performance), Some(Upscaler::Fsr), Some(Performance)), Ok(UpscalerChoice::Fsr(Performance)));
        assert_eq!(cli(Some(FsrMode::Off), Some(Upscaler::Off), None), Ok(UpscalerChoice::Off));
        // Conflicts: an explicit --fsr against --upscaler, --fsr off included.
        for (fsr, upscaler, quality) in [
            (Some(FsrMode::Quality), Some(Upscaler::Dlss), None),
            (Some(FsrMode::Off), Some(Upscaler::Dlss), None),
            (Some(FsrMode::Off), Some(Upscaler::Fsr), None),
            (Some(FsrMode::Quality), Some(Upscaler::Off), None),
            (Some(FsrMode::Quality), Some(Upscaler::Fsr), Some(Balanced)),
        ] {
            assert!(cli(fsr, upscaler, quality).is_err(), "{fsr:?} {upscaler:?} {quality:?}");
        }
        assert!(cli(None, None, Some(Quality)).unwrap_err().contains("--upscaler-quality"));
        assert!(cli(Some(FsrMode::Quality), None, Some(Quality)).is_err(), "--upscaler-quality goes with --upscaler only");
        assert!(cli(None, Some(Upscaler::Off), Some(Quality)).is_err());
    }

    /// The start's decision (#2198's play-test): DLSS chosen, or the files in the folder, runs
    /// the loader; `NVF_DLSS_PRELOAD=1` always, `=0` only for DLSS; without Windows and Vulkan
    /// the files are not looked at.
    #[test]
    fn streamline_is_loaded_at_start_for_dlss_or_the_files() {
        use std::ffi::OsStr;
        let never = || -> bool { panic!("the files are not asked about here") };
        let (on, off) = (Some(OsStr::new("1")), Some(OsStr::new("0")));
        let fsr = UpscalerChoice::Fsr(Quality);
        let start = |requested, preload, supported, files: bool| UpscalerStartup::new(requested, preload, supported, || files);
        // DLSS chosen: the loader runs whatever else is so (it says why DLSS cannot run).
        for (preload, supported) in [(None, true), (on, true), (off, true), (None, false), (off, false)] {
            let s = UpscalerStartup::new(UpscalerChoice::Dlss(Quality), preload, supported, never);
            assert_eq!(s.load, StartLoad::DlssChosen, "{preload:?} {supported}");
            assert!(s.loads_streamline());
        }
        // The files decide when nothing else does.
        for requested in [fsr, UpscalerChoice::Off] {
            assert_eq!(start(requested, None, true, true).load, StartLoad::FilesPresent);
            assert!(start(requested, None, true, true).loads_streamline());
            assert_eq!(start(requested, None, true, false).load, StartLoad::NoFiles);
            assert!(!start(requested, None, true, false).loads_streamline());
            assert_eq!(UpscalerStartup::new(requested, None, false, never).load, StartLoad::Unsupported, "not Windows, or no Vulkan: unchanged");
            assert!(!UpscalerStartup::new(requested, None, false, never).loads_streamline());
            // The test aid: 1 always, 0 never without DLSS, files or not.
            assert_eq!(UpscalerStartup::new(requested, on, true, never).load, StartLoad::PreloadOn);
            assert!(UpscalerStartup::new(requested, on, true, never).loads_streamline());
            assert_eq!(UpscalerStartup::new(requested, off, true, never).load, StartLoad::PreloadOff);
            assert!(!UpscalerStartup::new(requested, off, true, never).loads_streamline());
            // Another value is no aid: the files decide.
            assert_eq!(start(requested, Some(OsStr::new("yes")), true, true).load, StartLoad::FilesPresent);
        }
        assert_eq!(UpscalerStartup::new(UpscalerChoice::Dlss(Quality), None, true, never).describe(), "DLSS Quality chosen at start: Streamline loaded before the renderer");
        assert_eq!(start(fsr, None, true, true).describe(), "FSR 3.1 Quality chosen at start; the DLSS files are in the dlss folder: Streamline loaded before the renderer, so DLSS switches without a restart");
        assert_eq!(UpscalerStartup::new(fsr, on, true, never).describe(), "FSR 3.1 Quality chosen at start; NVF_DLSS_PRELOAD=1: Streamline loaded before the renderer all the same");
        assert_eq!(UpscalerStartup::new(fsr, off, true, never).describe(), "FSR 3.1 Quality chosen at start; NVF_DLSS_PRELOAD=0: Streamline not loaded (DLSS takes a restart)");
    }

    #[test]
    fn dlss_models_have_their_names_and_command_line_names() {
        assert_eq!(DlssModel::default(), DlssModel::Default);
        let names: Vec<&str> = DlssModel::ALL.iter().map(|m| m.name()).collect();
        assert_eq!(names, ["Default", "K", "L", "M", "J"], "the presets by their letters, in the row's order");
        for model in DlssModel::ALL {
            assert_eq!(DlssModel::named(model.name()), Some(model));
            assert_eq!(DlssModel::named(&format!(" {} ", model.name().to_lowercase())), Some(model), "any case");
        }
        // The names the row saved before: `Preset K` and so on, any case.
        for (older, model) in [("Preset J", DlssModel::J), ("Preset K", DlssModel::K), ("preset l", DlssModel::L), (" PRESET  M ", DlssModel::M)] {
            assert_eq!(DlssModel::named(older), Some(model), "{older:?}");
        }
        assert_eq!(DlssModel::named("Preset Default"), None, "Default never had the word");
        assert_eq!(DlssModel::named("A"), None, "removed presets are not offered");
        assert_eq!(DlssModel::named("Preset A"), None);
        assert_eq!(DlssModel::named("N"), None, "nor the ones that revert to the default");
        assert_eq!(DlssModel::named("Preset N"), None);
        assert_eq!(DlssModel::named(""), None);
        assert_eq!(DlssModel::named("Preset"), None);
        let cli: Vec<String> = DlssModel::value_variants().iter().map(|m| m.to_possible_value().unwrap().get_name().to_string()).collect();
        assert_eq!(cli, ["default", "k", "l", "m", "j"], "--dlss-model default|k|l|m|j");
        assert_eq!(DlssModel::value_variants(), DlssModel::ALL);
        assert_eq!(DlssModel::from_str("L", true), Ok(DlssModel::L));
        assert!(DlssModel::from_str("a", true).is_err());
    }

    #[test]
    fn what_was_drawn_is_shared_between_the_worlds() {
        let main = UpscaleDrawn::default();
        let render = main.clone();
        assert_eq!(main.get(), None, "nothing drawn yet");
        let drawn = DrawnUpscale::Upscaled { choice: UpscalerChoice::Dlss(Quality), render: UVec2::new(853, 480), output: UVec2::new(1280, 720) };
        render.set(Some(drawn));
        assert_eq!(main.get(), Some(drawn), "the render world's write is the main world's read");
        render.set(None);
        assert_eq!(main.get(), None, "no upscaler camera: none");
    }

    #[test]
    fn every_reason_has_words() {
        for why in [
            Unavailable::NotWindows,
            Unavailable::NotVulkan,
            Unavailable::FilesMissing,
            Unavailable::UpdaterInFolder,
            Unavailable::SignatureRejected,
            Unavailable::StreamlineRefused,
            Unavailable::DeviceUnsuitable,
            Unavailable::GpuNotSupported,
            Unavailable::EvaluateFailed,
            Unavailable::FsrUnsupported,
        ] {
            assert!(!why.words().is_empty() && !why.words().ends_with('.'), "{why:?}");
        }
    }
}
