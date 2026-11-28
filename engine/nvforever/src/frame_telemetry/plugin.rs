//! Main-world collection and lifecycle (docs/design/frame-telemetry.md §§1–4).

use super::capture::{Capture, Shutdown};
use super::records::{Diagnostic, DiagnosticKind, DiagnosticReason, DisplaySync, Header, HeaderMetadata, Marker, MarkerKind, Phase, StopReason};
use super::{parse_aids, Config};
use crate::frame_pace::{self, FramePace, Refresh};
use bevy::diagnostic::{DiagnosticsStore, FrameCount};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::renderer::{RenderAdapterInfo, RenderDevice};
use bevy::time::TimeUpdateStrategy;
use bevy::window::{PresentMode, PrimaryWindow};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CURSOR_PATHS: usize = 4_096;

pub struct FrameTelemetryPlugin(pub PathBuf);

/// Marker producers may join this set before the final Update collector.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum TelemetrySystems {
    Producers,
    Collect,
}

impl Plugin for FrameTelemetryPlugin {
    fn build(&self, app: &mut App) {
        let config = match read_aids() {
            Ok(config) if !config.duration.is_zero() => config,
            Ok(_) => return,
            Err(error) => {
                warn!("frame telemetry: disabled: {error}");
                return;
            }
        };
        // The commit, read before the clocks start: running Git is not part of what is
        // measured (git_paths.rs).
        let built = std::env::current_exe().and_then(std::fs::metadata).and_then(|m| m.modified()).ok();
        let revision = super::git_paths::engine_revision(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), built);
        // Pair the clocks before the first update; startup is part of the duration.
        let epoch = Instant::now();
        let utc = SystemTime::now().duration_since(UNIX_EPOCH).ok();
        let pending = (|| {
            let game = fnv_install::GameInstall::open(&self.0).map_err(|e| e.to_string())?;
            let dir = crate::crash::logs_dir(&self.0)?;
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let id = capture_id(utc.unwrap_or_default(), std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
            let path = dir.join(format!("frame-telemetry-{id}.jsonl"));
            fnv_install::guard_write(&path, Some(&game)).map_err(|e| e.to_string())?;
            remove_old_captures(&dir, &game);
            Ok::<_, String>(Pending { config, epoch, utc, id, path, revision })
        })();
        let pending = match pending {
            Ok(pending) => pending,
            Err(error) => {
                warn!("frame telemetry: disabled: {error}");
                return;
            }
        };
        if pending.config.gpu {
            crate::graphics::ensure_render_diagnostics(app);
        }
        app.insert_resource(Telemetry {
            pending: Some(pending),
            session: None,
        })
        .configure_sets(
            Update,
            (
                // Observe only: order telemetry after the engine's systems, never reorder theirs.
                TelemetrySystems::Producers
                    .after(nvf_play::PlaySet::Sync)
                    .after(crate::frame_pace::ApplyFramePace)
                    .after(crate::window_mode::ApplyWindowMode)
                    .after(crate::world::loading_gate)
                    .after(crate::handoff::reveal),
                TelemetrySystems::Collect.after(TelemetrySystems::Producers),
            ),
        )
        .add_systems(Update, collect.in_set(TelemetrySystems::Collect))
        .add_systems(Last, on_exit);
    }
}

/// Captures kept in the logs folder, this run's included: each may reach MAX_BYTES (64 MB),
/// and smoke runs start one every time (#1312) [design].
pub const KEEP_CAPTURES: usize = 8;

/// Remove the oldest `frame-telemetry-*.jsonl` files so that, with the one this run is
/// about to write, KEEP_CAPTURES remain. Capture IDs start with the UTC seconds, so the
/// names sort oldest first (`crash::old_files`). A file that cannot go is left.
fn remove_old_captures(dir: &std::path::Path, game: &fnv_install::GameInstall) {
    let names: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
    let old = crate::crash::old_files(&names, "frame-telemetry-", ".jsonl", KEEP_CAPTURES - 1);
    let removed = old.iter().map(|n| dir.join(n)).filter(|p| fnv_install::guard_write(p, Some(game)).is_ok() && std::fs::remove_file(p).is_ok()).count();
    if removed > 0 {
        info!("frame telemetry: removed {removed} older captures (keeping {KEEP_CAPTURES})");
    }
}

fn read_aids() -> Result<Config, String> {
    let mut raw = Vec::with_capacity(4);
    for name in [
        "NVF_PLAY_TEST_FRAME_TELEMETRY",
        "NVF_PLAY_TEST_FRAME_TELEMETRY_GPU",
        "NVF_PLAY_TEST_FRAME_TELEMETRY_RUN_ID",
        "NVF_PLAY_TEST_FRAME_TELEMETRY_SCENARIO_ID",
    ] {
        raw.push(match std::env::var(name) {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => return Err(format!("{name}: expected Unicode text")),
        });
    }
    parse_aids(raw[0].as_deref(), raw[1].as_deref(), raw[2].as_deref(), raw[3].as_deref()).map_err(|e| e.to_string())
}

struct Pending {
    config: Config,
    epoch: Instant,
    utc: Option<Duration>,
    id: String,
    path: PathBuf,
    /// The engine's commit, or why it is not known.
    revision: Result<String, &'static str>,
}

#[derive(Resource)]
struct Telemetry {
    pending: Option<Pending>,
    session: Option<Session>,
}

struct Session {
    capture: Option<Capture>,
    config: Config,
    epoch: Instant,
    path: PathBuf,
    settings: SettingsSnapshot,
    phases: PhaseTracker,
    phase: Option<Phase>,
    cursors: Cursors,
    gpu_seen: bool,
    timestamp_queries: Option<bool>,
    finishing: bool,
    last_main_frame: u32,
}

impl Session {
    fn sentinel(&self, t_ns: u64) -> Option<Diagnostic> {
        if !self.config.gpu || self.gpu_seen {
            return None;
        }
        let reason = terminal_reason(self.timestamp_queries);
        Diagnostic::gpu_sentinel(reason, t_ns, self.last_main_frame).ok()
    }

    fn begin_finish(&mut self, reason: StopReason) {
        if self.finishing {
            return;
        }
        self.finishing = true;
        let t_ns = elapsed_ns(self.epoch);
        let sentinel = self.sentinel(t_ns);
        if let Some(capture) = &mut self.capture {
            capture.begin_finish_at(t_ns, reason, sentinel);
        }
    }

    fn poll(&mut self) {
        let result = self.capture.as_mut().and_then(Capture::poll_finish);
        if let Some(result) = result {
            self.capture.take();
            log_summary(&self.path, result, self.config.gpu);
        }
    }
}

/// Finish the capture now, as the app's teardown would (`Drop for Telemetry`, the bounded
/// wait): a run that ends its process without the teardown (nvforever `handoff.rs`,
/// `end_now`) calls this first.
pub fn finish_for_exit(world: &mut World) {
    drop(world.remove_resource::<Telemetry>());
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        // App/world teardown is outside the Update collector. This is the only bounded
        // wait; it also handles exits whose AppExit message arrived after our Last system.
        if let Some(session) = &mut self.session {
            if let Some(capture) = session.capture.take() {
                let t_ns = elapsed_ns(session.epoch);
                let sentinel = session.sentinel(t_ns);
                log_summary(&session.path, capture.finish_at(t_ns, StopReason::NormalExit, sentinel), session.config.gpu);
            }
        }
    }
}

#[derive(SystemParam)]
struct Inputs<'w, 's> {
    real: Res<'w, Time<Real>>,
    frame: Res<'w, FrameCount>,
    strategy: Res<'w, TimeUpdateStrategy>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    pace: Option<Res<'w, FramePace>>,
    refresh: Option<Res<'w, Refresh>>,
    adapter: Option<Res<'w, RenderAdapterInfo>>,
    device: Option<Res<'w, RenderDevice>>,
    streamer: Option<Res<'w, crate::world::Streamer>>,
    loading: Option<Res<'w, nvf_play::loading::Loading>>,
    diagnostics: Option<Res<'w, DiagnosticsStore>>,
}

#[derive(Clone, Debug, PartialEq)]
struct SettingsSnapshot {
    size: Option<(u32, u32)>,
    pace: Option<FramePace>,
    cap: Option<u32>,
    refresh: Option<u32>,
    present: Option<PresentMode>,
    window_mode: Option<bevy::window::WindowMode>,
}

impl Inputs<'_, '_> {
    fn settings(&self) -> SettingsSnapshot {
        let window = self.windows.single().ok();
        SettingsSnapshot {
            size: window.map(|w| (w.physical_width(), w.physical_height())),
            pace: self.pace.as_deref().copied(),
            cap: self
                .pace
                .as_deref()
                .map(|p| frame_pace::effective_cap(p.cap, self.refresh.as_deref().map_or(0, |r| r.vrr_cap))),
            refresh: self.refresh.as_deref().and_then(|r| r.millihertz),
            present: window.map(|w| w.present_mode),
            window_mode: window.map(|w| w.mode),
        }
    }
}

fn start(pending: Pending, inputs: &Inputs<'_, '_>) -> Result<Session, String> {
    if !matches!(*inputs.strategy, TimeUpdateStrategy::Automatic) {
        return Err("automatic Time<Real> is required".to_owned());
    }
    let settings = inputs.settings();
    let mut metadata = HeaderMetadata {
        started_at_utc: pending.utc.and_then(utc_stamp),
        engine_revision: pending.revision.clone().ok(),
        build_features: build_features(),
        width_px: settings.size.map(|s| s.0),
        height_px: settings.size.map(|s| s.1),
        display_sync: settings.pace.map(|p| match p.sync {
            frame_pace::DisplaySync::Off => DisplaySync::Off,
            frame_pace::DisplaySync::Vsync => DisplaySync::Vsync,
            frame_pace::DisplaySync::Variable => DisplaySync::Variable,
        }),
        cap_fps: settings.cap,
        refresh_millihz: settings.refresh,
        window_present_mode: settings.present.map(|mode| format!("{mode:?}")),
        renderer_available: inputs.device.is_some(),
        ..Default::default()
    };
    if let Err(reason) = &pending.revision {
        metadata.unavailable.insert("engine_revision".to_owned(), reason.to_string());
    }
    if metadata.started_at_utc.is_none() {
        metadata.unavailable.insert("started_at_utc".to_owned(), "utc_clock_unavailable".to_owned());
    }
    if let Some(adapter) = inputs.adapter.as_deref() {
        metadata.adapter = Some(adapter.name.clone());
        metadata.backend = Some(format!("{:?}", adapter.backend));
        metadata.driver = Some(adapter.driver.clone());
        metadata.driver_info = Some(adapter.driver_info.clone());
    }
    let header = Header::new(&pending.id, pending.config.clone(), metadata).map_err(|e| e.to_string())?;
    let capture = Capture::create(&pending.path, header, pending.epoch).map_err(|e| e.to_string())?;
    info!(
        "frame telemetry: capturing {} s to {} (frames{})",
        pending.config.duration.as_secs_f64(),
        pending.path.display(),
        if pending.config.gpu { " + render timings" } else { "" }
    );
    let timestamp_queries = inputs.device.as_deref().map(|device| {
        device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS)
    });
    Ok(Session {
        capture: Some(capture),
        config: pending.config,
        epoch: pending.epoch,
        path: pending.path,
        settings,
        phases: PhaseTracker::default(),
        phase: None,
        cursors: Cursors::default(),
        gpu_seen: false,
        timestamp_queries,
        finishing: false,
        last_main_frame: inputs.frame.0,
    })
}

fn collect(mut telemetry: ResMut<Telemetry>, inputs: Inputs) {
    if let Some(pending) = telemetry.pending.take() {
        match start(pending, &inputs) {
            Ok(session) => telemetry.session = Some(session),
            Err(error) => {
                warn!("frame telemetry: disabled: {error}");
                return;
            }
        }
    }
    let Some(session) = &mut telemetry.session else {
        return;
    };
    if session.capture.is_none() {
        return;
    }
    if session.finishing {
        session.poll();
        return;
    }
    session.last_main_frame = inputs.frame.0;
    let t_ns = elapsed_ns(session.epoch);
    let reason = session.capture.as_ref().and_then(Capture::stop_reason);
    if let Some(reason) = reason {
        session.begin_finish(reason);
        session.poll();
        return;
    }
    if t_ns >= session.config.duration.as_nanos() as u64 {
        session.begin_finish(StopReason::DurationLimit);
        session.poll();
        return;
    }
    if session.settings != inputs.settings() {
        session.begin_finish(StopReason::SettingsChanged);
        session.poll();
        return;
    }
    if !matches!(*inputs.strategy, TimeUpdateStrategy::Automatic) {
        warn!("frame telemetry: automatic Time<Real> changed during capture");
        session.begin_finish(StopReason::SettingsChanged);
        session.poll();
        return;
    }
    let duration_ns = inputs.real.delta().as_nanos().min(u128::from(u64::MAX)) as u64;
    let phase = session.phases.observe(
        duration_ns,
        inputs.streamer.as_deref().map(|s| s.settled),
        inputs.loading.as_deref().is_some_and(nvf_play::loading::Loading::unseen_frames),
    );
    let capture = session.capture.as_mut().expect("active capture");
    if session.phase != Some(phase) {
        if let Ok(marker) = Marker::new(MarkerKind::PhaseEnter, phase.name(), t_ns) {
            capture.marker(marker);
        }
        session.phase = Some(phase);
    }
    capture.frame_at(t_ns, inputs.frame.0, duration_ns, phase);
    if session.config.gpu && capture.stop_reason().is_none() {
        if let Some(store) = inputs.diagnostics.as_deref() {
            let mut watermark = session.cursors.watermark;
            for diagnostic in store.iter() {
                let path = diagnostic.path().as_str();
                let Some(kind) = render_kind(path) else {
                    continue;
                };
                let Some(measurement) = diagnostic.measurement() else {
                    continue;
                };
                if measurement.time < session.epoch {
                    continue;
                }
                watermark = Some(watermark.map_or(measurement.time, |t| t.max(measurement.time)));
                let decision = session.cursors.observe(path, measurement.time, session.epoch);
                if decision == Observation::Old {
                    continue;
                }
                session.gpu_seen |= kind == DiagnosticKind::GpuElapsed && diagnostic.suffix == "ms" && measurement.value.is_finite();
                if decision == Observation::Drop {
                    capture.drop_diagnostic_at(t_ns);
                    continue;
                }
                let observation_ns = measurement.time.duration_since(session.epoch).as_nanos().min(u128::from(u64::MAX)) as u64;
                capture.diagnostic_at(
                    t_ns,
                    Diagnostic::measurement(path, &diagnostic.suffix, kind, measurement.value, observation_ns, inputs.frame.0),
                );
            }
            session.cursors.watermark = watermark;
        }
    }
    if let Some(reason) = capture.stop_reason() {
        session.begin_finish(reason);
    }
    session.poll();
}

fn on_exit(mut exits: MessageReader<AppExit>, mut telemetry: ResMut<Telemetry>) {
    if exits.read().next().is_some() {
        if let Some(session) = &mut telemetry.session {
            session.begin_finish(StopReason::NormalExit);
            session.poll();
        }
    }
}

#[derive(Default)]
struct PhaseTracker {
    started: bool,
    startup_ns: u64,
}

impl PhaseTracker {
    fn observe(&mut self, duration_ns: u64, settled: Option<bool>, unseen: bool) -> Phase {
        if !self.started {
            self.startup_ns = self.startup_ns.saturating_add(duration_ns);
            self.started = settled.unwrap_or(true) || self.startup_ns > 60_000_000_000;
            Phase::Startup
        } else if unseen {
            Phase::Loading
        } else {
            Phase::Gameplay
        }
    }
}

#[derive(Default)]
struct Cursors {
    paths: HashMap<String, Instant>,
    watermark: Option<Instant>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Observation {
    Old,
    Export,
    Drop,
}

impl Cursors {
    fn observe(&mut self, path: &str, time: Instant, epoch: Instant) -> Observation {
        if time < epoch {
            return Observation::Old;
        }
        if let Some(previous) = self.paths.get_mut(path) {
            if time <= *previous {
                return Observation::Old;
            }
            *previous = time;
            return Observation::Export;
        }
        if path.len() > 1_024 || self.paths.len() >= CURSOR_PATHS {
            return if self.watermark.is_some_and(|watermark| time <= watermark) {
                Observation::Old
            } else {
                Observation::Drop
            };
        }
        self.paths.insert(path.to_owned(), time);
        Observation::Export
    }
}

fn render_kind(path: &str) -> Option<DiagnosticKind> {
    if !path.starts_with("render/") {
        return None;
    }
    if path.ends_with("/elapsed_cpu") {
        Some(DiagnosticKind::RenderCpuElapsed)
    } else if path.ends_with("/elapsed_gpu") {
        Some(DiagnosticKind::GpuElapsed)
    } else {
        None
    }
}

fn elapsed_ns(epoch: Instant) -> u64 {
    epoch.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
}

fn terminal_reason(timestamp_queries: Option<bool>) -> DiagnosticReason {
    if timestamp_queries == Some(false) {
        DiagnosticReason::TimestampQueriesUnsupported
    } else {
        DiagnosticReason::NoSampleObserved
    }
}

fn build_features() -> Vec<String> {
    [
        ("default", cfg!(feature = "default")),
        ("dev-aids", cfg!(feature = "dev-aids")),
        ("intro-video", cfg!(feature = "intro-video")),
        ("profile", cfg!(feature = "profile")),
        ("tracy", cfg!(feature = "tracy")),
        ("tracy-memory", cfg!(feature = "tracy-memory")),
    ]
    .into_iter()
    .filter(|(_, enabled)| *enabled)
    .map(|(name, _)| name.to_owned())
    .collect()
}

fn capture_id(utc: Duration, pid: u32, counter: u64) -> String {
    format!("{}-{:09}-{pid}-{counter}", utc.as_secs(), utc.subsec_nanos())
}

fn utc_stamp(utc: Duration) -> Option<String> {
    let seconds = utc.as_secs();
    let (year, month, day) = crate::crash::civil_from_days((seconds / 86_400) as i64);
    if !(0..=9999).contains(&year) {
        return None;
    }
    let seconds = seconds % 86_400;
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:09}Z",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        utc.subsec_nanos()
    ))
}

fn log_summary(path: &std::path::Path, result: Shutdown, gpu: bool) {
    if let Shutdown::Finished(report) = result {
        if let Some(trailer) = report.trailer {
            let gameplay = &trailer.phases[Phase::Gameplay.index()];
            let number = |value: Option<f64>| value.map_or_else(|| "none".to_owned(), |v| format!("{v:.3}"));
            let observations = if gpu {
                format!(", gpu observations {}", trailer.gpu_observations_written)
            } else {
                String::new()
            };
            let error = report.error.as_deref().map_or(String::new(), |error| format!(", writer error: {error}"));
            let flush = if report.flush_confirmed { "" } else { ", flush unconfirmed" };
            let summary = format!(
                "frame telemetry: {} {} ({}), {} frames, gameplay p50 {} ms p95 {} ms p99 {} ms, dropped {}, {} bytes{observations}{flush}{error}",
                path.display(),
                if trailer.complete() { "complete" } else { "truncated" },
                trailer.stop_reason.name(),
                trailer.frames_written,
                number(gameplay.p50_ms),
                number(gameplay.p95_ms),
                number(gameplay.p99_ms),
                trailer.dropped_frames + trailer.dropped_diagnostics + trailer.dropped_markers,
                report.bytes_written
            );
            if report.error.is_some() {
                warn!("{summary}");
            } else {
                info!("{summary}");
            }
        } else {
            warn!(
                "frame telemetry: {} truncated ({}), trailer unavailable: {}",
                path.display(),
                report.stop_reason.name(),
                report.error.as_deref().unwrap_or("shutdown grace expired")
            );
        }
    } else if matches!(result, Shutdown::TimedOut) {
        warn!("frame telemetry: {} truncated (shutdown_timeout), trailer unavailable", path.display());
    } else {
        warn!("frame telemetry: {} trailer in progress, final status unavailable", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_priority_and_settled_boundary() {
        let mut phases = PhaseTracker::default();
        assert_eq!(phases.observe(0, Some(false), true), Phase::Startup);
        assert_eq!(phases.observe(1, Some(true), true), Phase::Startup);
        assert_eq!(phases.observe(1, Some(false), true), Phase::Loading);
        assert_eq!(phases.observe(1, None, false), Phase::Gameplay);
        let mut absent = PhaseTracker::default();
        assert_eq!(absent.observe(0, None, false), Phase::Startup);
        assert_eq!(absent.observe(0, None, false), Phase::Gameplay);
    }

    #[test]
    fn startup_timeout_is_strict_and_saturates() {
        let mut phases = PhaseTracker::default();
        assert_eq!(phases.observe(60_000_000_000, Some(false), false), Phase::Startup);
        assert_eq!(phases.observe(u64::MAX, Some(false), false), Phase::Startup);
        assert_eq!(phases.observe(0, Some(false), false), Phase::Gameplay);
    }

    #[test]
    fn capture_ids_fit_schema_and_distinguish_runs() {
        let id = capture_id(Duration::new(u64::MAX, 999_999_999), u32::MAX, u64::MAX);
        assert!(id.len() <= 64);
        assert!(super::super::parse_id(Some(&id), "capture_id").is_ok());
        assert_ne!(capture_id(Duration::ZERO, 1, 0), capture_id(Duration::ZERO, 1, 1));
        assert_ne!(capture_id(Duration::ZERO, 1, 0), capture_id(Duration::ZERO, 2, 0));
    }

    #[test]
    fn utc_is_rfc3339_and_checks_range() {
        assert_eq!(utc_stamp(Duration::ZERO).as_deref(), Some("1970-01-01T00:00:00.000000000Z"));
        assert_eq!(utc_stamp(Duration::new(951_782_400, 123)).as_deref(), Some("2000-02-29T00:00:00.000000123Z"));
        assert!(utc_stamp(Duration::from_secs(u64::MAX)).is_none());
    }

    #[test]
    fn cursors_deduplicate_and_exclude_pre_capture_measurements() {
        let epoch = Instant::now();
        let mut cursors = Cursors::default();
        assert_eq!(cursors.observe("render/elapsed_gpu", epoch - Duration::from_nanos(1), epoch), Observation::Old);
        assert_eq!(cursors.observe("render/elapsed_gpu", epoch, epoch), Observation::Export);
        assert_eq!(cursors.observe("render/elapsed_gpu", epoch, epoch), Observation::Old);
        assert_eq!(
            cursors.observe("render/elapsed_gpu", epoch + Duration::from_nanos(1), epoch),
            Observation::Export
        );
    }

    #[test]
    fn cursor_overflow_uses_import_watermark() {
        let epoch = Instant::now();
        let mut cursors = Cursors::default();
        for n in 0..CURSOR_PATHS {
            assert_eq!(cursors.observe(&n.to_string(), epoch, epoch), Observation::Export);
        }
        for path in ["overflow-a", "overflow-b"] {
            assert_eq!(cursors.observe(path, epoch, epoch), Observation::Drop);
        }
        cursors.watermark = Some(epoch);
        assert_eq!(cursors.observe("overflow-a", epoch, epoch), Observation::Old);
        assert_eq!(cursors.observe("overflow-a", epoch + Duration::from_nanos(1), epoch), Observation::Drop);
        assert_eq!(cursors.paths.len(), CURSOR_PATHS);
        assert_eq!(cursors.observe(&"x".repeat(1_025), epoch + Duration::from_nanos(1), epoch), Observation::Drop);
    }

    #[test]
    fn only_render_elapsed_paths_are_exported() {
        assert_eq!(render_kind("render/elapsed_gpu"), Some(DiagnosticKind::GpuElapsed));
        assert_eq!(render_kind("render/a/elapsed_cpu"), Some(DiagnosticKind::RenderCpuElapsed));
        assert_eq!(render_kind("else/elapsed_gpu"), None);
        assert_eq!(render_kind("render/gpu_memory"), None);
    }

    #[test]
    fn missing_gpu_samples_distinguish_unsupported_from_unknown() {
        assert_eq!(terminal_reason(Some(false)), DiagnosticReason::TimestampQueriesUnsupported);
        assert_eq!(terminal_reason(Some(true)), DiagnosticReason::NoSampleObserved);
        assert_eq!(terminal_reason(None), DiagnosticReason::NoSampleObserved);
    }
}
