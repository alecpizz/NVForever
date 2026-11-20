//! Frame generation's presenter (2026-11-08): a thread of its own that takes the window's
//! images, copies the generated and the real frame into them and presents them, the second
//! present half a real frame after the first. The render thread hands each frame over and goes
//! on with the next (frame_gen.rs `hand_to_presenter`).
//!
//! Why: presented one after the other from the render thread, the real frame's present waited
//! inside the driver until the generated frame was shown, about one refresh, and the render
//! thread recorded nothing meanwhile. A frame's own work plus that wait came to more than two
//! refreshes once the work passed about 7 ms, and the real frame rate fell from 80 to 50-58 at
//! 160 Hz [observed 2026-11-08: the generated frame's present 0.13-0.21 ms, the real one's
//! 4.4-6.2 ms; issued 6 ms after the first, it returned in 0.4-0.75 ms; h9x000/NVForever#1,
//! docs/design/frame-generation.md "Presentation and pacing"].
//!
//! Pacing follows the design of AMD's `FrameInterpolationSwapchainVK` (FidelityFX SDK 1.1.4,
//! MIT; read as reference, no code taken): wait for the frame's GPU work, then space the
//! presents by half the mean real frame time over the last 10 frames, less 0.1 x its standard
//! deviation and a 0.1 ms margin, the average started again after a gap of more than 100 ms
//! [documented: `FrameInterpolationSwapchainVK.cpp` 1251-1268, `varianceFactor` 0.1 and
//! `safetyMarginInSec` 0.0001 in `FrameInterpolationSwapchainVK.h` 216-217; its `getVariance`
//! returns the square root of the variance, `FrameInterpolationSwapchainVK_Helpers.h` 373-391].
//! Two choices are NVForever's own: AMD presents back to back until 10 frames are in (its
//! average is 0 until then), where this one averages the frames it has; and AMD's wait spins on
//! the performance counter throughout (`FrameInterpolationSwapchainVK_Helpers.cpp` 29-36),
//! where this one sleeps, then spins its last millisecond. The spacing is never less than one
//! refresh, and the real frame rate is capped slightly below half the refresh rate
//! (`frame_gen::real_frame_cap`), as AMD advises, so two presents never fall within one refresh.
//! A frame that reaches the presenter when the next is already waiting is shown without its
//! generated frame, so the presenter catches up at once instead of keeping a frame of delay.
//!
//! wgpu 30 lets one acquired image exist per surface and holds a device-wide lock across each
//! present (`command_indices`, written), which stops submits from every thread while a present
//! blocks. So only this thread takes and presents the window's images while it runs
//! (bevy_render `DeferredSwapchain`), and its presents are timed to fall away from the render
//! thread's submit: the real frame's goes out about 1 ms after the next frame is handed over
//! [observed 2026-11-08, 4K at 160 Hz, 78 real fps: each present held 1-1.6 ms in the driver,
//! the render thread kept the cap].
//!
//! This thread never configures the surface: wgpu-core 30's `configure_surface` waits for the
//! GPU and fails with a validation error when another thread submits meanwhile
//! [documented: `wgpu-core-30.0.1/src/device/resource.rs` 5338-5351], and the render thread
//! keeps submitting. An outdated surface's images are skipped and the surface is marked for
//! the render thread, which reconfigures it before its next frame (bevy_render
//! `SurfaceGuard::acquire`, `create_surfaces`). A presenter whose thread could not start or has
//! ended (a panic) is noticed when the next frame is handed over: frame_gen drops it and the
//! window is presented from the render thread again (`frame_gen::hand_to_presenter`).
//!
//! Measured against presenting from the render thread (h9x000/NVForever#1, 4K, the owner's
//! settings): 77.7-78.0 real fps at the cap of 78 with a frame of about 8 ms and with 4 ms more
//! (55.6 before), the 95th percentile between presents 7.0 ms (13.5 before); the generated
//! frame is presented when the GPU has done the frame (4.4-4.8 ms after it is handed over,
//! polled about every 0.5 ms, and every 50 us once it is nearly due: `wait_for_gpu`), as the
//! driver showed it before.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bevy::render::view::{SharedSurface, STAND_INS};

use crate::frame_gen::Blit;

/// The presenter of one window.
pub(crate) struct Presenter {
    jobs: Option<mpsc::Sender<Job>>,
    thread: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
    sent: u64,
    /// A frame was left out because the presenter was behind (said once).
    said_behind: bool,
}

/// What became of a frame given to [`Presenter::send`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handed {
    /// The presenter has it.
    Over,
    /// The presenter is alive but did not catch up within the wait (its thread is held in the
    /// driver: a present or an image take that does not return); the frame is not shown.
    Behind,
    /// The thread has ended; the frame is not shown.
    Ended,
}

/// One real frame's images: the generated frame then the real one, or the only one.
struct Job {
    index: u64,
    frames: Vec<wgpu::TextureView>,
    /// When the render thread handed it over, and how long it waited for the presenter first
    /// (with what the limiter held the frame back for the presenter).
    sent: Instant,
    held: Duration,
    /// One refresh of the window's monitor (zero when not known): the least spacing.
    refresh: Duration,
    /// NVIDIA low latency: the markers that map this frame's presents to it (nv_low_latency.rs).
    out_of_band: Option<crate::nv_low_latency::OutOfBandPresents>,
}

pub(crate) struct Shared {
    /// The last frame whose copies are all submitted.
    copied: Mutex<u64>,
    copied_changed: Condvar,
    /// Spent waiting for the presents' times, and frames that could not be presented (an
    /// occluded or outdated window), since last read.
    pub(crate) paced_nanos: AtomicU64,
    pub(crate) skipped: AtomicU64,
    /// Generated frames left out to catch up (the presenter a whole frame behind), since last
    /// read.
    pub(crate) dropped: AtomicU64,
    /// From each frame's handing over to its first present (the GPU wait, the pacing and any
    /// frames ahead of it: what the presenter adds to the latency), since last read.
    pub(crate) queued_nanos: AtomicU64,
    /// Of that, waiting for the GPU, and the polls it took (each up to about 0.2 ms after the
    /// GPU was done), since last read.
    pub(crate) gpu_wait_nanos: AtomicU64,
    pub(crate) gpu_polls: AtomicU64,
    /// When each present was issued (frame generation's log line).
    presents: Arc<Mutex<Vec<Instant>>>,
    /// `frame_gen::GeneratedFrames`: counted when generated, taken back when left out.
    generated_shown: Arc<AtomicU64>,
}

impl Shared {
    fn new(presents: Arc<Mutex<Vec<Instant>>>, generated_shown: Arc<AtomicU64>) -> Self {
        Self {
            copied: Mutex::new(0),
            copied_changed: Condvar::new(),
            paced_nanos: AtomicU64::new(0),
            skipped: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            queued_nanos: AtomicU64::new(0),
            gpu_wait_nanos: AtomicU64::new(0),
            gpu_polls: AtomicU64::new(0),
            presents,
            generated_shown,
        }
    }

    fn copied(&self, index: u64) {
        let mut copied = self.copied.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if *copied < index {
            *copied = index;
        }
        self.copied_changed.notify_all();
    }
}

impl Presenter {
    /// The window's presenter, its thread started; the error when the thread could not be.
    pub(crate) fn start(surface: SharedSurface, device: wgpu::Device, queue: wgpu::Queue, presents: Arc<Mutex<Vec<Instant>>>, generated_shown: Arc<AtomicU64>) -> std::io::Result<Self> {
        let shared = Arc::new(Shared::new(presents, generated_shown));
        let (jobs, received) = mpsc::channel();
        let thread_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("nvf-frame-gen-present".into())
            .spawn(move || run(&received, &surface, &device, &queue, &thread_shared))?;
        bevy::log::info!("frame generation: presenting on a thread of its own, paced half a real frame apart");
        Ok(Self { jobs: Some(jobs), thread: Some(thread), shared, sent: 0, said_behind: false })
    }

    pub(crate) fn shared(&self) -> &Shared {
        &self.shared
    }

    /// Whether its thread is still there to present: it has not ended (a panic, as nothing else
    /// ends it while it is handed frames).
    pub(crate) fn running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Hand over this frame's images (each a view of a texture it may read until the frame
    /// after next), with one refresh of the monitor. Waits while the presenter is more than
    /// `STAND_INS - 1` frames behind, so nothing it has still to copy is drawn over.
    /// [`Handed::Ended`]: the thread has ended and the frame was not handed over (an ended thread
    /// never signals, so the wait looks for that every 10 ms rather than running to its 250 ms
    /// limit). [`Handed::Behind`]: the wait ran to its limit; the frame is not handed over
    /// either. Handed over, it joined a queue with no end whose stand-ins the render thread
    /// draws over in turn before they are copied, and every frame of it was shown wrong.
    /// `out_of_band`: NVIDIA low latency's markers for the frame's presents, when it is on.
    pub(crate) fn send(&mut self, frames: Vec<wgpu::TextureView>, refresh: Duration, out_of_band: Option<crate::nv_low_latency::OutOfBandPresents>) -> Handed {
        let Some(jobs) = &self.jobs else { return Handed::Ended };
        let index = self.sent + 1;
        let needed = index.saturating_sub(STAND_INS as u64 - 1);
        let started = Instant::now();
        let deadline = started + Duration::from_millis(250);
        let mut copied = self.shared.copied.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        while *copied < needed {
            if !self.running() {
                return Handed::Ended;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                drop(copied);
                if !std::mem::replace(&mut self.said_behind, true) {
                    bevy::log::warn!("frame generation: the presenter thread did not take a frame within 250 ms (held in the driver); frames are left out until it does");
                }
                return Handed::Behind;
            };
            copied = self.shared.copied_changed.wait_timeout(copied, left.min(Duration::from_millis(10))).unwrap_or_else(std::sync::PoisonError::into_inner).0;
        }
        drop(copied);
        let sent = Instant::now();
        // With what the limiter held frames back for this thread to catch up
        // (frame_pace.rs `catch_up`): not the frames' own time either.
        let held = sent - started + Duration::from_nanos(crate::frame_pace::HELD_FOR_PRESENTER_NANOS.swap(0, Ordering::Relaxed));
        if jobs.send(Job { index, frames, sent, held, refresh, out_of_band }).is_err() {
            return Handed::Ended;
        }
        self.sent = index;
        Handed::Over
    }
}

impl Drop for Presenter {
    /// The frames handed over are presented, then the thread ends, before the window is
    /// Bevy's again.
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(received: &mpsc::Receiver<Job>, surface: &SharedSurface, device: &wgpu::Device, queue: &wgpu::Queue, shared: &Shared) {
    let mut blit = Blit::default();
    // HDR output: the frames drawn into the window's images encoded (hdr.rs).
    let mut hdr = crate::hdr::Encoder::default();
    let mut pacer = Pacer::default();
    let mut gpu_time = Duration::ZERO;
    let mut last_present: Option<Instant> = None;
    let mut next: Option<Job> = None;
    loop {
        let job = match next.take() {
            Some(job) => job,
            None => match received.recv() {
                Ok(job) => job,
                Err(_) => break,
            },
        };
        // A whole frame behind (the next frame is already here): show this frame's real image
        // only, a refresh after the last present, and leave its generated frame out. At the cap
        // two paced presents take nearly a whole real frame, so a backlog drained by about
        // 0.2 ms a frame and the frames waited here for up to another frame [observed
        // 2026-11-09, 4K at 160 Hz, 78 real fps: the first present 13-21 ms after the frame
        // was handed over in some 10 s periods, 7.4 in others].
        let behind = match received.try_recv() {
            Ok(newer) => {
                next = Some(newer);
                true
            }
            Err(_) => false,
        };
        pacer.add(job.sent, job.held);
        let frames = if behind && job.frames.len() == 2 {
            shared.dropped.fetch_add(1, Ordering::Relaxed);
            // Not shown after all: the frame-rate counter counts generated frames shown.
            shared.generated_shown.fetch_sub(1, Ordering::Relaxed);
            &job.frames[1..]
        } else {
            &job.frames[..]
        };
        // Never closer than a refresh: two presents within one queue up in the driver, and the
        // second waits there for the first to be shown [observed 2026-11-08: spaced 5.4-6.15 ms
        // at 160 Hz, each present took about 2 ms].
        let paired = frames.len() == 2;
        let spacing = if paired { pacer.spacing().max(job.refresh) } else { job.refresh };
        let mut gpu_waited = Duration::ZERO;
        if paired {
            let (waited, polls) = wait_for_gpu(device, queue, gpu_time);
            gpu_waited = waited;
            // A running mean over about eight frames.
            gpu_time = (gpu_time * 7 + waited) / 8;
            shared.gpu_wait_nanos.fetch_add(waited.as_nanos() as u64, Ordering::Relaxed);
            shared.gpu_polls.fetch_add(polls, Ordering::Relaxed);
        }
        let guard = surface.lock();
        let (_, view_format) = guard.formats();
        let count = frames.len();
        for (i, source) in frames.iter().enumerate() {
            let Some(image) = guard.acquire() else {
                shared.skipped.fetch_add(1, Ordering::Relaxed);
                continue;
            };
            let target = image.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(view_format), ..Default::default() });
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame_gen_present_copy") });
            if view_format == crate::hdr::SURFACE_FORMAT && crate::hdr::active() {
                hdr.draw(device, queue, &mut encoder, source, &target);
            } else {
                blit.draw(device, &mut encoder, source, &target, view_format, "frame_gen_present_copy");
            }
            queue.submit([encoder.finish()]);
            crate::hdr::probe(device, queue, &image.texture, if count == 2 && i == 0 { "fg-generated" } else { "fg" });
            if i + 1 == count {
                shared.copied(job.index);
            }
            // Paired frames are spaced; a frame shown alone while catching up is held a refresh
            // after the last present; frames without a generated one (menus, cuts) go at once.
            if let (true, Some(last)) = (paired || behind, last_present) {
                let waited = wait_until(last + spacing);
                shared.paced_nanos.fetch_add(waited.as_nanos() as u64, Ordering::Relaxed);
            }
            // Spaced from when the last present was issued, not from when it returned: from its
            // return, the time each present takes in the driver was added to every spacing, and
            // the presenter fell behind the real frame cap [observed 2026-11-08: 0.26-0.47 ms
            // per present, presents 6.5-6.7 ms apart where 6.3 was due, 74-77 real fps of 78].
            let issued = Instant::now();
            last_present = Some(issued);
            if i == 0 {
                let queued = issued.saturating_duration_since(job.sent);
                shared.queued_nanos.fetch_add(queued.as_nanos() as u64, Ordering::Relaxed);
                // What the frame waited here beyond the GPU's work on it, for the limiter to
                // make up before the next frame's input (frame_pace.rs `catch_up`). A frame
                // shown alone while catching up was not waited for: the GPU's usual time.
                // Menus' and cuts' frames go at once and wait for nothing.
                let lag = if paired {
                    queued.saturating_sub(gpu_waited)
                } else if behind {
                    queued.saturating_sub(gpu_time)
                } else {
                    Duration::ZERO
                };
                crate::frame_pace::PRESENTER_LAG_NANOS.store(lag.as_nanos() as u64, Ordering::Relaxed);
            }
            // NVIDIA low latency: each present names the rendered frame it shows.
            crate::nv_low_latency::OutOfBandPresents::around(job.out_of_band.as_ref(), || guard.present(queue, image));
            shared.presents.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(issued);
        }
        shared.copied(job.index);
    }
}

/// Until the GPU has done the work submitted so far (the frame and its generated frame), so the
/// presents are timed from when the frame can be shown (at most 100 ms); how long it waited and
/// how often it polled. Polled, not waited on: wgpu's blocking wait holds a device lock
/// (`snatchable_lock`, read) for as long as it waits, and a thread that asks for it to write
/// (a resource destroyed) would stop every other thread's submits behind it until then.
///
/// `expected` is what the last frames' GPU work took: until half a millisecond before that it
/// sleeps between polls (0.2 ms asked, about 0.5 ms on Windows), then it polls every 50 us,
/// spinning, so the present goes out within that of the GPU finishing, not up to a sleep
/// later (below the real-frame cap that sleep was the generated frame's timing error). A frame
/// that takes more than 1.5 ms longer than expected goes back to sleeping.
fn wait_for_gpu(device: &wgpu::Device, queue: &wgpu::Queue, expected: Duration) -> (Duration, u64) {
    let done = Arc::new(AtomicBool::new(false));
    let flag = done.clone();
    queue.on_submitted_work_done(move || flag.store(true, Ordering::Release));
    let started = Instant::now();
    let close = expected.saturating_sub(Duration::from_micros(500))..expected + Duration::from_micros(1500);
    let mut polls = 0;
    while !done.load(Ordering::Acquire) && started.elapsed() < Duration::from_millis(100) {
        let _ = device.poll(wgpu::PollType::Poll);
        polls += 1;
        if done.load(Ordering::Acquire) {
            break;
        }
        if close.contains(&started.elapsed()) {
            let until = Instant::now() + Duration::from_micros(50);
            while Instant::now() < until {
                std::hint::spin_loop();
            }
        } else {
            std::thread::sleep(Duration::from_micros(200));
        }
    }
    (started.elapsed(), polls)
}

/// Sleep until a millisecond before `due`, then spin; how long it waited.
fn wait_until(due: Instant) -> Duration {
    let started = Instant::now();
    if let Some(sleep) = due.checked_duration_since(started).and_then(|d| d.checked_sub(Duration::from_millis(1))) {
        std::thread::sleep(sleep);
    }
    while Instant::now() < due {
        std::hint::spin_loop();
    }
    started.elapsed()
}

/// The real frames' intervals, for the presents' spacing: between the times the render thread
/// handed them over, less what it waited for the presenter before each (so a presenter that
/// falls behind does not slow its own pacing to match, which locked the frame rate low: at a
/// start, slow frames gave a long spacing, the long spacing held the render thread back, and
/// the held frames kept the spacing long [observed 2026-11-08: 19, 36, 43 real fps in the first
/// 30 s at 1280x720, where 80 was due]).
#[derive(Default)]
struct Pacer {
    last: Option<Instant>,
    intervals: VecDeque<f64>,
}

/// Intervals the mean is taken over.
const PACER_FRAMES: usize = 10;

impl Pacer {
    fn add(&mut self, now: Instant, held: Duration) {
        if let Some(last) = self.last {
            let interval = now.duration_since(last).saturating_sub(held).as_secs_f64();
            if interval > 0.1 {
                self.intervals.clear();
            } else {
                if self.intervals.len() == PACER_FRAMES {
                    self.intervals.pop_front();
                }
                self.intervals.push_back(interval);
            }
        }
        self.last = Some(now);
    }

    /// Half the mean interval, less 0.1 x its standard deviation and 0.1 ms, never below 0.
    fn spacing(&self) -> Duration {
        spacing(self.intervals.iter().copied())
    }
}

/// AMD's `getAverage() * 0.5 - getVariance() * varianceFactor`, less `safetyMarginInSec`, where
/// `getVariance` is the standard deviation (the square root of the variance), in seconds here;
/// over the intervals there are, where AMD's is 0 until it has 10.
fn spacing(intervals: impl Iterator<Item = f64> + Clone) -> Duration {
    let n = intervals.clone().count();
    if n == 0 {
        return Duration::ZERO;
    }
    let mean = intervals.clone().sum::<f64>() / n as f64;
    let deviation = (intervals.map(|i| (i - mean) * (i - mean)).sum::<f64>() / n as f64).sqrt();
    Duration::from_secs_f64((mean * 0.5 - deviation * 0.1 - 0.0001).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presents_are_spaced_half_a_real_frame_less_a_margin() {
        let frames = [0.0125; 10];
        let s = spacing(frames.iter().copied()).as_secs_f64();
        assert!((s - (0.00625 - 0.0001)).abs() < 1e-9, "{s}");
        assert_eq!(spacing([].iter().copied()), Duration::ZERO);
        // Uneven frames: a tenth of the standard deviation (2.5 ms here) comes off as well, as
        // AMD's `getVariance` (a square root) gives it: 6.25 - 0.25 - 0.1 ms.
        let uneven = spacing([0.010, 0.015].iter().copied()).as_secs_f64();
        assert!((uneven - 0.0059).abs() < 1e-9, "{uneven}");
        // Never below zero.
        assert_eq!(spacing([0.0001].iter().copied()), Duration::ZERO);
    }

    #[test]
    fn a_presenter_whose_thread_ended_takes_no_frames_and_does_not_hold_the_render_thread() {
        let (jobs, received) = mpsc::channel::<Job>();
        let thread = std::thread::spawn(move || {
            let _ = received.recv();
            // Unwinds as a panic does, without the panic hook's message.
            std::panic::resume_unwind(Box::new("the presenter's panic"));
        });
        let mut presenter = Presenter { jobs: Some(jobs), thread: Some(thread), shared: Arc::new(Shared::new(Default::default(), Default::default())), sent: 0, said_behind: false };
        assert!(presenter.running());
        assert_eq!(presenter.send(Vec::new(), Duration::ZERO, None), Handed::Over, "handed over while the thread runs");
        let deadline = Instant::now() + Duration::from_secs(10);
        while presenter.running() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!presenter.running(), "a thread that panicked is not running");
        // Behind (two frames handed over and not copied): the wait for the copies ends at once.
        presenter.sent = 5;
        let started = Instant::now();
        assert_eq!(presenter.send(Vec::new(), Duration::ZERO, None), Handed::Ended, "nothing handed over");
        assert!(started.elapsed() < Duration::from_millis(100), "{:?}", started.elapsed());
        // Not behind: the closed channel says so.
        presenter.sent = 0;
        assert_eq!(presenter.send(Vec::new(), Duration::ZERO, None), Handed::Ended);
    }

    #[test]
    fn a_presenter_that_stays_behind_is_not_handed_the_frame() {
        let (jobs, received) = mpsc::channel::<Job>();
        let (stop, stopped) = mpsc::channel::<()>();
        // Alive, and copies nothing: a thread held in the driver.
        let thread = std::thread::spawn(move || {
            let _ = stopped.recv();
            drop(received);
        });
        let mut presenter = Presenter { jobs: Some(jobs), thread: Some(thread), shared: Arc::new(Shared::new(Default::default(), Default::default())), sent: 0, said_behind: false };
        // The first frames fit the stand-ins.
        for _ in 0..STAND_INS - 1 {
            assert_eq!(presenter.send(Vec::new(), Duration::ZERO, None), Handed::Over);
        }
        let started = Instant::now();
        assert_eq!(presenter.send(Vec::new(), Duration::ZERO, None), Handed::Behind);
        assert!(started.elapsed() >= Duration::from_millis(240), "{:?}", started.elapsed());
        assert_eq!(presenter.sent, STAND_INS as u64 - 1, "the frame left out has no number");
        // Caught up: frames go over again.
        presenter.shared.copied(presenter.sent);
        assert_eq!(presenter.send(Vec::new(), Duration::ZERO, None), Handed::Over);
        let _ = stop.send(());
    }

    #[test]
    fn a_long_gap_starts_the_average_again() {
        let mut p = Pacer::default();
        let t = Instant::now();
        p.add(t, Duration::ZERO);
        p.add(t + Duration::from_millis(12), Duration::ZERO);
        p.add(t + Duration::from_millis(500), Duration::ZERO);
        assert!(p.intervals.is_empty());
        p.add(t + Duration::from_millis(512), Duration::ZERO);
        assert_eq!(p.intervals.len(), 1);
    }

    #[test]
    fn time_held_back_by_the_presenter_does_not_count() {
        let mut p = Pacer::default();
        let t = Instant::now();
        p.add(t, Duration::ZERO);
        // 25 ms apart, 12 of them waiting for the presenter: the frame took 13.
        p.add(t + Duration::from_millis(25), Duration::from_millis(12));
        assert!((p.intervals[0] - 0.013).abs() < 1e-9, "{:?}", p.intervals);
    }
}
