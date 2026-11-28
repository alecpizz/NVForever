//! Gate R. The ordinary flood asks the GPU about each wave, then the GPU gathers what that flood reached.

use crate::stage::{Bench, Times};
use crate::{ms, word, Report, Result};
use nvf_rtgi::{classified, fit, reach_by, Coverage, Failed, FitConfig, FitOutcome, GatherLimits, Place, Probe, ReachedBy, SolvedVolume, V3};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

const TOLD: usize = 10;

struct Wave {
    reached: ReachedBy,
    asked: Vec<(Place, [u32; 2])>,
    times: Times,
    classify_ms: f64,
    flood_ms: f64,
}

fn wave(bench: &Bench) -> Result<Wave> {
    let (mut asked, mut times, mut classify_ms, mut fault) = (Vec::new(), Times::default(), 0.0, None);
    let mut callback_ms = 0.0;
    let mut classify = |places: &[Place], positions: &[V3]| {
        let start = Instant::now();
        let counts = bench.classify(positions, &mut times).map_err(|e| {
            fault = Some(e);
            Failed::Internal
        })?;
        classify_ms += ms(start);
        // Save these for the independent CPU validity check after the timed flood.
        asked.extend(places.iter().copied().zip(counts.iter().copied()));
        callback_ms += ms(start);
        Ok(counts)
    };
    let start = Instant::now();
    let reached = reach_by(&bench.capture.scene, &bench.capture.seeds.points, &bench.config.gather, &mut classify, &|| false);
    let flood_ms = ms(start) - callback_ms;
    let reached = reached.map_err(|e| fault.unwrap_or_else(|| e.into()))?;
    if asked.len() != reached.classified {
        return Err(format!("recorded {} classifications, flood counted {}", asked.len(), reached.classified).into());
    }
    Ok(Wave { reached, asked, times, classify_ms, flood_ms })
}

fn position(place: Place, spacing: f32) -> V3 {
    V3(place.map(|c| c as f32 * spacing))
}

fn support(volume: &SolvedVolume) -> Result<BTreeSet<Place>> {
    Ok(volume.points()?.into_iter().map(|p| p.place).collect())
}

fn coverage(c: Coverage) -> Value {
    json!({ "seeds": c.seeds, "failed": c.failed, "far": c.far, "reached": c.reached, "parts": c.parts, "largest": c.largest, "in_bounds": c.in_bounds })
}

/// Count differences can leave a candidate on the same side of the validity threshold.
fn validity(bench: &Bench, wave: &Wave) -> Result<(bool, Value)> {
    let positions: Vec<_> = wave.asked.iter().map(|&(p, _)| position(p, wave.reached.spacing)).collect();
    let start = Instant::now();
    let cpu = classified(&bench.capture.scene, &positions, &bench.directions, bench.config.gather.threads, &|| false)?;
    let check_ms = ms(start);
    if cpu.len() != wave.asked.len() {
        return Err("wrong number of CPU classifications".into());
    }
    let valid = |[hits, backs]: [u32; 2]| Probe { hits, backs, ..Default::default() }.valid();
    let (mut counts_differ, mut validity_differs) = (0usize, 0usize);
    let mut first = Vec::new();
    for ((place, g), c) in wave.asked.iter().zip(&cpu) {
        counts_differ += usize::from(c != g);
        if valid(*c) != valid(*g) {
            validity_differs += 1;
            if first.len() < TOLD {
                first.push(json!({ "place": place, "cpu": c, "gpu": g }));
            }
        }
    }
    println!("  candidates: {counts_differ} hit/back count differences, {validity_differs} validity differences {first:?}");
    Ok((
        validity_differs == 0,
        json!({ "counts_differ": counts_differ, "validity_differs": validity_differs, "first": first, "cpu_check_ms": check_ms }),
    ))
}

fn fitted(bench: &Bench, volume: &SolvedVolume) -> Result<(FitOutcome, f64)> {
    let start = Instant::now();
    let config =
        FitConfig { emission: volume.emission, offset: volume.offset, threads: bench.config.gather.threads, ..FitConfig::default() };
    let cap = &bench.capture;
    let outcome = fit(&cap.scene, &cap.light, volume, &cap.seeds, &config, &|| false)?;
    Ok((outcome, ms(start)))
}

fn compare_fits(cpu: FitOutcome, gpu: FitOutcome) -> (bool, Value) {
    let outcome = |f| match f {
        FitOutcome::Fitted(f) => json!({ "kind": "fitted", "b": f.b, "g": f.g, "pay": f.pay, "capped": f.capped }),
        FitOutcome::Stock(s) => json!({ "kind": "stock", "reason": format!("{s:?}") }),
    };
    let passed = match (cpu, gpu) {
        (FitOutcome::Fitted(a), FitOutcome::Fitted(b)) => {
            let close = |a: f32, b: f32| {
                let (a, b) = (f64::from(a), f64::from(b));
                a.is_finite() && b.is_finite() && (b - a).abs() <= 0.005 * a.abs()
            };
            a.capped == b.capped && close(a.b, b.b) && close(a.g, b.g) && close(a.pay, b.pay)
        }
        (FitOutcome::Stock(a), FitOutcome::Stock(b)) => std::mem::discriminant(&a) == std::mem::discriminant(&b),
        _ => false,
    };
    (passed, json!({ "passed": passed, "cpu": outcome(cpu), "gpu": outcome(gpu) }))
}

type Layout = (Place, [u32; 3], [u32; 3]);

#[derive(Serialize)]
struct FirstSolve {
    points: usize,
    invalid: usize,
    timing: Times,
    solve_ms: f64,
    pack_ms: f64,
    fit_ms: f64,
    cpu_fit_ms: f64,
    packed_places: usize,
    packed_matches_reach: bool,
    packed_matches_cpu: bool,
    cpu_box: Layout,
    gpu_box: Layout,
    same_box: bool,
    fits: Value,
    fit_matches: bool,
}

fn first_solve(bench: &Bench, reached: &ReachedBy, cpu_places: &BTreeSet<Place>) -> Result<FirstSolve> {
    let places = &reached.reached;
    let mut times = Times::default();
    let start = Instant::now();
    let points = bench.points(places.iter().map(|&p| position(p, reached.spacing)).collect(), &mut times)?;
    let probes = bench.probes(&points, &bench.capture.light, &mut times)?;
    let solve_ms = ms(start) - times.read_ms;
    let invalid = probes.iter().filter(|p| !p.valid()).count();
    if probes.len() != places.len() {
        return Err(format!("{} probes for {} reached places", probes.len(), places.len()).into());
    }
    let start = Instant::now();
    let lit: BTreeMap<_, _> = places.iter().copied().zip(probes.iter().map(|p| p.moments)).collect();
    let packed = SolvedVolume::of_points(&lit, reached.coverage, &bench.config).map_err(|e| format!("packing the GPU's volume: {e}"))?;
    let pack_ms = ms(start);
    // Read the support mask back from the packed atlas. The extent alone misses interior holes.
    let packed_places = support(&packed)?;
    let packed_matches_reach = &packed_places == places;
    let packed_matches_cpu = &packed_places == cpu_places;
    let layout = |v: &SolvedVolume| (v.low, v.dimensions, v.atlas);
    let (cpu_box, gpu_box) = (layout(&bench.reference), layout(&packed));
    let same_box = cpu_box == gpu_box;
    println!("  packed support: own reach {packed_matches_reach}, CPU {packed_matches_cpu}. CPU box {cpu_box:?}, GPU {gpu_box:?}, {invalid} invalid lit probes");
    let (gpu_fit, fit_ms) = fitted(bench, &packed).map_err(|e| format!("fitting the GPU's volume: {e}"))?;
    let (cpu_fit, cpu_fit_ms) = fitted(bench, &bench.reference).map_err(|e| format!("fitting the CPU's volume: {e}"))?;
    let (fit_matches, fits) = compare_fits(cpu_fit, gpu_fit);
    println!("  full-sphere fit: {fits}");
    Ok(FirstSolve {
        points: probes.len(),
        invalid,
        timing: times,
        solve_ms,
        pack_ms,
        fit_ms,
        cpu_fit_ms,
        packed_places: packed_places.len(),
        packed_matches_reach,
        packed_matches_cpu,
        cpu_box,
        gpu_box,
        same_box,
        fits,
        fit_matches,
    })
}

pub const HEADER: &str =
    "CPU/GPU reached   probed/budget  admission   fit+support+box   reach ms   flood ms   solve ms   read ms   pack ms   fit ms   estimate ms";

pub fn run(bench: &Bench) -> Result<Report> {
    let cpu = &bench.reference;
    let cpu_places = support(cpu)?;
    let wave = wave(bench)?;
    let reached = &wave.reached;
    let places = &reached.reached;
    let only_cpu: Vec<_> = cpu_places.difference(places).take(TOLD).collect();
    let only_gpu: Vec<_> = places.difference(&cpu_places).take(TOLD).collect();
    let same_places = cpu_places == *places;
    let same_coverage = cpu.coverage == reached.coverage;
    println!("  {} reached sets, first CPU-only {only_cpu:?}, GPU-only {only_gpu:?}", word(same_places));
    println!("  {} coverage, CPU {:?}, GPU {:?}", word(same_coverage), cpu.coverage, reached.coverage);
    let limit = GatherLimits::default().points;
    let admitted = reached.classified <= limit;
    let admission = if admitted { "within" } else { "over" };
    println!("  CPU probed {}, GPU probed {}, {admission} the {limit}-point budget on the GPU flood", cpu.stats.points, reached.classified);
    println!(
        "  reach: {} waves, {} kernel submits, {:.2} ms, including {:.2} ms readback. Flood {:.2} ms",
        reached.calls, wave.times.submits, wave.classify_ms, wave.times.read_ms, wave.flood_ms
    );
    let mut detail = json!({
        "reach": {
            "cpu": cpu_places.len(), "gpu": places.len(), "same_places": same_places, "first_only_cpu": only_cpu, "first_only_gpu": only_gpu,
            "cpu_coverage": coverage(cpu.coverage), "gpu_coverage": coverage(reached.coverage), "same_coverage": same_coverage,
            "waves": reached.calls, "timing": wave.times,
            "classify_ms": wave.classify_ms, "flood_ms": wave.flood_ms
        },
        "point_budget": { "limit": limit, "cpu_probed": cpu.stats.points, "cpu_within_budget": cpu.stats.points <= limit, "gpu_probed": reached.classified, "gpu_within_budget": admitted }
    });
    let solved = first_solve(bench, reached, &cpu_places);
    let (same_validity, classification) = validity(bench, &wave)?;
    detail["reach"]["classification"] = classification;
    let solved = match solved {
        Ok(s) => s,
        Err(e) => {
            let line = format!("{} probed ({admission} {limit}), {e}", reached.classified);
            detail["error"] = json!(e.to_string());
            return Ok(Report { passed: false, line, detail });
        }
    };
    let fit_passed = solved.fit_matches && solved.packed_matches_reach && solved.packed_matches_cpu && solved.same_box;
    let passed = same_places && same_coverage && same_validity && solved.invalid == 0 && fit_passed;
    let estimate_ms = bench.times.setup_ms()
        + wave.classify_ms
        + wave.flood_ms
        + solved.solve_ms
        + solved.timing.read_ms
        + solved.pack_ms
        + solved.fit_ms;
    detail["capture_to_publication_estimate_ms"] = json!(estimate_ms);
    detail["solve"] = json!(solved);
    let line = format!(
        "{:6}/{:<8} {:6}/{:<7} {admission:9}   {:15}   {:8.1}   {:8.1}   {:8.1}   {:7.1}   {:7.1}   {:6.1}   {estimate_ms:11.1}",
        cpu_places.len(),
        places.len(),
        reached.classified,
        limit,
        word(fit_passed),
        wave.classify_ms,
        wave.flood_ms,
        solved.solve_ms,
        solved.timing.read_ms,
        solved.pack_ms,
        solved.fit_ms
    );
    Ok(Report { passed, line, detail })
}
