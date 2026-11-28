//! A room on both devices and the wall times of its first dispatch and readback.

use crate::dump::Capture;
use crate::gpu::Gpu;
use crate::{ms, Result};
use nvf_rtgi::{directions, solve, LightState, Probe, SolveConfig, SolvedVolume, V3};
use nvf_rtgi_live::{probes_of, reach_of, read_back, Config, Points, Room, Tables, Target};
use serde::Serialize;
use std::sync::Arc;
use std::time::Instant;

#[derive(Serialize)]
pub struct SceneTimes {
    pub load_ms: f64,
    pub prepare_ms: f64,
    pub tables_ms: f64,
    pub upload_ms: f64,
    /// Through the build fence, including any pending buffer uploads.
    pub blas_tlas_ms: f64,
    pub cpu_solve_ms: f64,
}

impl SceneTimes {
    pub fn setup_ms(&self) -> f64 {
        self.prepare_ms + self.tables_ms + self.upload_ms + self.blas_tlas_ms
    }
}

#[derive(Default, Serialize)]
pub struct Times {
    pub points_ms: f64,
    pub buffers_ms: f64,
    pub dispatch_ms: f64,
    pub read_ms: f64,
    /// Kernel submits only. Each has a separate copy for its readback.
    pub submits: usize,
    pub dispatches: usize,
}

pub struct Bench<'a> {
    gpu: &'a Gpu,
    room: Arc<Room>,
    tables: Tables,
    live: Config,
    pub capture: Capture,
    pub reference: SolvedVolume,
    pub directions: Vec<V3>,
    pub config: SolveConfig,
    pub times: SceneTimes,
}

fn wait(gpu: &Gpu) -> Result<()> {
    gpu.device.poll(wgpu::PollType::wait_indefinitely())?;
    Ok(gpu.sound()?)
}

impl<'a> Bench<'a> {
    pub fn open(gpu: &'a Gpu, capture: Capture, config: SolveConfig, live: Config) -> Result<Self> {
        let start = Instant::now();
        let reference = solve(&capture.scene, &capture.light, &capture.seeds, &config, &|| false)?;
        let cpu_solve_ms = ms(start);
        let start = Instant::now();
        let directions = directions(config.gather.rays);
        let tables = Tables::new(&capture.scene, &directions)?;
        let tables_ms = ms(start);
        let start = Instant::now();
        let room = Arc::new(Room::new(&gpu.device, &tables)?);
        let upload_ms = ms(start);
        let start = Instant::now();
        room.build(&gpu.device, &gpu.queue)?;
        wait(gpu)?;
        let times = SceneTimes {
            load_ms: capture.load_ms,
            prepare_ms: capture.prepare_ms,
            tables_ms,
            upload_ms,
            blas_tlas_ms: ms(start),
            cpu_solve_ms,
        };
        println!(
            "dump {:.1} ms, scene {:.1}, tables {:.1}, device buffers {:.1}, BLAS/TLAS {:.1}, reference solve {:.1}",
            times.load_ms, times.prepare_ms, times.tables_ms, times.upload_ms, times.blas_tlas_ms, times.cpu_solve_ms
        );
        Ok(Self { gpu, room, tables, live, capture, reference, directions, config, times })
    }

    pub fn points(&self, positions: Vec<V3>, times: &mut Times) -> Result<Points> {
        let start = Instant::now();
        let points = Points::at(positions, &self.capture.scene, &self.tables)?;
        times.points_ms += ms(start);
        Ok(points)
    }

    /// Wait for the kernel before timing the copy. The library decoders discard partial rows.
    fn round(&self, target: Target, points: &Points, light: &LightState, times: &mut Times) -> Result<Vec<u8>> {
        let stride = match target {
            Target::Moments => 64,
            Target::Reach => 8,
            _ => return Err("unexpected room kernel".into()),
        };
        let start = Instant::now();
        let live = self.gpu.live(target, &self.room, points, light, &self.live)?;
        times.buffers_ms += ms(start);
        let start = Instant::now();
        self.gpu.submit(&live)?;
        wait(self.gpu)?;
        times.dispatch_ms += ms(start);
        times.submits += 1;
        times.dispatches += live.batches() as usize;
        let start = Instant::now();
        let bytes = read_back(&self.gpu.device, &self.gpu.queue, live.out())?;
        times.read_ms += ms(start);
        check_rows(bytes.len(), points.positions().len(), stride)?;
        Ok(bytes)
    }

    pub fn probes(&self, points: &Points, light: &LightState, times: &mut Times) -> Result<Vec<Probe>> {
        let bytes = self.round(Target::Moments, points, light, times)?;
        let probes = probes_of(&bytes);
        if probes.len() != points.positions().len() {
            return Err("wrong number of moment rows".into());
        }
        // The four counters are floats on the wire. A cast alone can turn a NaN into a zero count.
        for (i, (row, p)) in bytes.as_chunks::<64>().0.iter().zip(&probes).enumerate() {
            if p.hits as usize > self.config.gather.rays || p.backs > p.hits || p.shadow_rays > p.lamps {
                return Err(format!("point {i}: inconsistent hit, back, lamp or shadow-ray counts").into());
            }
            for (v, count) in row.as_chunks::<16>().0.iter().zip([p.hits, p.backs, p.lamps, p.shadow_rays]) {
                let raw = f32::from_le_bytes([v[12], v[13], v[14], v[15]]);
                if f64::from(raw) != f64::from(count) {
                    return Err(format!("point {i}: invalid count {raw}").into());
                }
            }
        }
        Ok(probes)
    }

    /// A large wave can take more than one submit.
    pub fn classify(&self, positions: &[V3], times: &mut Times) -> Result<Vec<[u32; 2]>> {
        let mut counts = Vec::with_capacity(positions.len());
        for some in positions.chunks(1 << 16) {
            let points = self.points(some.to_vec(), times)?;
            counts.extend(reach_of(&self.round(Target::Reach, &points, &LightState::default(), times)?));
        }
        if counts.len() != positions.len() {
            return Err("wrong number of classification rows".into());
        }
        Ok(counts)
    }
}

fn check_rows(bytes: usize, rows: usize, stride: usize) -> Result<()> {
    let expected = rows.checked_mul(stride).ok_or("readback too large")?;
    if bytes != expected {
        return Err(format!("read {bytes} bytes, expected {rows} rows of {stride} bytes ({expected})").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readbacks_must_end_at_the_last_requested_row() {
        for stride in [8, 64] {
            assert!(check_rows(2 * stride, 2, stride).is_ok());
            for bytes in [stride, 2 * stride - 1, 2 * stride + 1, 3 * stride] {
                assert!(check_rows(bytes, 2, stride).is_err());
            }
        }
    }
}
