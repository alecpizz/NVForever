//! Gate D. Each coefficient has its own distribution over the reached points of a room.

use crate::stage::{Bench, Times};
use crate::{word, Report, Result};
use nvf_rtgi::{reach, Lamp, LightState, Place, Probe, VolumePoint, V3};
use nvf_rtgi_live::Points;
use serde::Serialize;
use serde_json::json;

const CHANNELS: [&str; 12] = ["C0.r", "C0.g", "C0.b", "Cx.r", "Cx.g", "Cx.b", "Cy.r", "Cy.g", "Cy.b", "Cz.r", "Cz.g", "Cz.b"];
const TOLD: usize = 10;

#[derive(Serialize)]
struct Channel {
    name: &'static str,
    p95_pct: f64,
    max_pct: f64,
}

#[derive(Serialize)]
struct Difference {
    place: Place,
    channel: &'static str,
    cpu: f32,
    gpu: f32,
    error_pct: f64,
}

#[derive(Serialize)]
struct Counts {
    place: Place,
    /// Lamps, shadow rays.
    cpu: [u32; 2],
    gpu: [u32; 2],
}

#[derive(Serialize)]
struct State {
    name: String,
    mean_c0_luminance: f64,
    channels: Vec<Channel>,
    worst: Vec<Difference>,
    hit_mismatches: usize,
    back_mismatches: usize,
    lamp_mismatches: usize,
    shadow_mismatches: usize,
    first_count_mismatches: Vec<Counts>,
    passed: bool,
}

fn luminance(color: V3) -> f64 {
    let [r, g, b] = color.0.map(f64::from);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn compare(name: &str, places: &[VolumePoint], cpu: &[Probe], gpu: &[Probe]) -> Result<State> {
    if places.is_empty() || places.len() != cpu.len() || cpu.len() != gpu.len() {
        return Err(format!("{} places, {} CPU probes, {} GPU probes", places.len(), cpu.len(), gpu.len()).into());
    }
    let mean = cpu.iter().map(|p| luminance(p.moments.c[0])).sum::<f64>() / cpu.len() as f64;
    let error = |a: f32, b: f32| {
        if !a.is_finite() || !b.is_finite() || !mean.is_finite() || mean < 0.0 {
            f64::INFINITY
        } else if a == b {
            0.0
        } else if mean == 0.0 {
            // A nonzero difference in a black room fails at any tolerance.
            f64::INFINITY
        } else {
            100.0 * (f64::from(a) - f64::from(b)).abs() / mean
        }
    };
    let mut channels = Vec::new();
    let mut worst = vec![(0, 0.0); places.len()];
    for (k, channel) in CHANNELS.iter().enumerate() {
        let mut errors = Vec::with_capacity(cpu.len());
        for ((a, b), most) in cpu.iter().zip(gpu).zip(&mut worst) {
            let pct = error(a.moments.c[k / 3][k % 3], b.moments.c[k / 3][k % 3]);
            if pct > most.1 {
                *most = (k, pct);
            }
            errors.push(pct);
        }
        errors.sort_unstable_by(f64::total_cmp);
        // Nearest rank, ceil(0.95 n), as a zero-based index.
        let last = errors.len() - 1;
        channels.push(Channel { name: channel, p95_pct: errors[last - errors.len() / 20], max_pct: errors[last] });
    }
    let mut indices: Vec<_> = (0..worst.len()).collect();
    indices.sort_by(|&a, &b| worst[b].1.total_cmp(&worst[a].1));
    let worst = indices
        .into_iter()
        .take(TOLD)
        .map(|i| {
            let (k, error_pct) = worst[i];
            Difference {
                place: places[i].place,
                channel: CHANNELS[k],
                cpu: cpu[i].moments.c[k / 3][k % 3],
                gpu: gpu[i].moments.c[k / 3][k % 3],
                error_pct,
            }
        })
        .collect();
    let (mut hit_mismatches, mut back_mismatches) = (0, 0);
    let (mut lamp_mismatches, mut shadow_mismatches) = (0, 0);
    let mut first_count_mismatches = Vec::new();
    for ((at, a), b) in places.iter().zip(cpu).zip(gpu) {
        hit_mismatches += usize::from(a.hits != b.hits);
        back_mismatches += usize::from(a.backs != b.backs);
        let (cpu, gpu) = ([a.lamps, a.shadow_rays], [b.lamps, b.shadow_rays]);
        lamp_mismatches += usize::from(cpu[0] != gpu[0]);
        shadow_mismatches += usize::from(cpu[1] != gpu[1]);
        if cpu != gpu && first_count_mismatches.len() < TOLD {
            first_count_mismatches.push(Counts { place: at.place, cpu, gpu });
        }
    }
    let passed = lamp_mismatches == 0 && shadow_mismatches == 0 && channels.iter().all(|c| c.p95_pct <= 1.0 && c.max_pct <= 5.0);
    Ok(State {
        name: name.into(),
        mean_c0_luminance: mean,
        channels,
        worst,
        hit_mismatches,
        back_mismatches,
        lamp_mismatches,
        shadow_mismatches,
        first_count_mismatches,
        passed,
    })
}

fn state(bench: &Bench, places: &[VolumePoint], points: &Points, light: &LightState, name: &str) -> Result<State> {
    let cap = &bench.capture;
    // The volume stores half floats. reach gives the CPU's original moments and counts, keyed by place.
    let reached = reach(&cap.scene, light, &cap.seeds.points, &bench.config.gather, &|| false)?;
    if reached.points.len() != places.len() || reached.coverage != bench.reference.coverage {
        return Err(format!("{name}: CPU reach differs from the reference solve").into());
    }
    let cpu = places
        .iter()
        .map(|p| reached.points.get(&p.place).copied().ok_or("CPU reach missed a reference place"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let gpu = bench.probes(points, light, &mut Times::default())?;
    let s = compare(name, places, &cpu, &gpu)?;
    println!(
        "  {} D, {name}, mean C0 luminance {:.6}, lamp/shadow count mismatches {} / {}, hit/back mismatches (diagnostic) {} / {}",
        word(s.passed),
        s.mean_c0_luminance,
        s.lamp_mismatches,
        s.shadow_mismatches,
        s.hit_mismatches,
        s.back_mismatches
    );
    for rgb in s.channels.chunks(3) {
        let line: Vec<_> = rgb.iter().map(|c| format!("{} {:.4}/{:.4}", c.name, c.p95_pct, c.max_pct)).collect();
        println!("    p95/max (%): {}", line.join(", "));
    }
    for d in &s.worst {
        println!("    {:?} {}: CPU {:.6}, GPU {:.6}, {:.4} %", d.place, d.channel, d.cpu, d.gpu, d.error_pct);
    }
    for c in &s.first_count_mismatches {
        println!("    {:?} lamps/shadows: CPU {:?}, GPU {:?}", c.place, c.cpu, c.gpu);
    }
    Ok(s)
}

pub const HEADER: &str = "points   worst p95 %   max %   lamp errors   shadow errors   hit diffs   back diffs";

pub fn run(bench: &Bench, lamp: Option<usize>) -> Result<Report> {
    let places = bench.reference.points()?;
    let points = bench.points(places.iter().map(|p| p.position).collect(), &mut Times::default())?;
    let light = &bench.capture.light;
    let mut states = vec![state(bench, &places, &points, light, "dump light")?];
    let score = |l: &Lamp| luminance(l.display) * f64::from(l.range.max(0.0));
    let off = lamp.or_else(|| {
        light.lamps.iter().enumerate().max_by(|(_, a), (_, b)| score(a).total_cmp(&score(b))).map(|(i, _)| i)
    });
    if let Some(i) = off {
        let selected = light.lamps.get(i).ok_or_else(|| format!("lamp {i} requested, {} active lamps", light.lamps.len()))?;
        let why = if lamp.is_some() { "--lamp" } else { "highest display luminance times range" };
        println!("  switching off active lamp {i} ({why}, score {:.4})", score(selected));
        let mut changed = light.clone();
        changed.lamps.remove(i);
        states.push(state(bench, &places, &points, &changed, &format!("lamp {i} off"))?);
    } else {
        println!("  lamp-off state skipped: no active lamp");
    }
    let dim = LightState { ambient: light.ambient * 0.5, ..light.clone() };
    states.push(state(bench, &places, &points, &dim, "half ambient")?);
    let passed = states.iter().all(|s| s.passed);
    let channels = || states.iter().flat_map(|s| &s.channels);
    let p95 = channels().map(|c| c.p95_pct).fold(0.0, f64::max);
    let max = channels().map(|c| c.max_pct).fold(0.0, f64::max);
    let lamps: usize = states.iter().map(|s| s.lamp_mismatches).sum();
    let shadows: usize = states.iter().map(|s| s.shadow_mismatches).sum();
    let hits: usize = states.iter().map(|s| s.hit_mismatches).sum();
    let backs: usize = states.iter().map(|s| s.back_mismatches).sum();
    let line = format!("{:6}   {p95:11.4}   {max:5.2}   {lamps:11}   {shadows:13}   {hits:9}   {backs:10}", places.len());
    let detail = json!({ "points": places.len(), "active_lamps": light.lamps.len(), "lamp_off": off,
        "hit_mismatches": hits, "back_mismatches": backs, "states": states });
    Ok(Report { passed, line, detail })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_dim_coefficient_decides_its_own_percentile() {
        let at = VolumePoint { place: [0; 3], position: V3::ZERO, texel: [1; 3] };
        let mut lit = Probe::default();
        lit.moments.c[0] = V3([1.0; 3]);
        let cpu = [lit; 20];
        let mut gpu = cpu;
        let check = |gpu: &[Probe]| compare("test", &[at; 20], &cpu, gpu).unwrap().passed;
        // p95 is the 19th point. The largest error is below the 5 % limit in both cases.
        gpu[0].moments.c[3].0[2] = 0.03125;
        assert!(check(&gpu));
        gpu[1].moments.c[3].0[2] = 0.03125;
        assert!(!check(&gpu));
        gpu = cpu;
        gpu[0].lamps += 1;
        assert!(!check(&gpu));
        gpu = cpu;
        gpu[0].shadow_rays += 1;
        assert!(!check(&gpu));
        gpu = cpu;
        gpu[0].hits += 1;
        gpu[0].backs += 1;
        let state = compare("test", &[at; 20], &cpu, &gpu).unwrap();
        assert_eq!((state.hit_mismatches, state.back_mismatches), (1, 1));
        assert!(state.passed);
    }
}
