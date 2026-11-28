//! The live bounce-light kernel on room dumps (#3009). Each folder holds scene.json and scene.bin.
//!
//!     rtgi-live-run [--reach] [--adapter text] [--validation] [--json file]
//!                   [--threads n] [--rays n] [--points n] [--near distance] [--lamp i] <folder>...
//!
//! By default gate D gathers the CPU solve's reached points on the GPU under the dump's light, with
//! one active lamp removed (highest display luminance times range by default) and with half the ambient. Each of the twelve
//! coefficients has a p95 limit of 1 % and a maximum of 5 % of that state's mean CPU C0 luminance.
//! Lamp and shadow-ray counts must agree at every point. Hit and back count differences are diagnostic.
//! `--points` limits the number of points in one dispatch.
//!
//! With `--reach`, gate R runs the CPU flood with GPU classifications and fits the first solve of
//! the places it reached. The capture-to-publication estimate starts with the captured scene inputs
//! and ends with the fitted constants. Adapter open (including kernel/pipeline creation), dump I/O and decoding, CPU reference checks, in-game
//! collection and renderer publication are outside the estimate. All times are wall milliseconds.
//! The point budget for play is reported separately from parity.
//!
//! Exit 0 for a pass, 1 for a gate or runner failure, and 2 if no matching Vulkan adapter has ray queries.

mod compare;
mod dump;
#[expect(dead_code, reason = "the standalone check also uses fixture-only GPU helpers")]
#[path = "../rtgi-live-check/gpu.rs"]
mod gpu;
mod reach;
mod stage;
#[path = "../rtgi-live-check/validation.rs"]
mod validation;

use nvf_rtgi::{GatherConfig, GatherLimits, SolveConfig, MOST_RAYS, MOST_THREADS};
use nvf_rtgi_live::{Config, Target};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const USAGE: &str = "rtgi-live-run [--reach] [--adapter text] [--validation] [--json file] [--threads n] [--rays n] [--points n] [--near distance] [--lamp i] <dump folder>...";

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn word(passed: bool) -> &'static str {
    if passed {
        "PASS"
    } else {
        "FAIL"
    }
}

struct Args {
    folders: Vec<PathBuf>,
    reach: bool,
    validation: bool,
    adapter: String,
    json: Option<PathBuf>,
    lamp: Option<usize>,
    config: SolveConfig,
    live: Config,
}

fn arguments(mut rest: impl Iterator<Item = OsString>) -> Result<Args> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(MOST_THREADS);
    // The cave probes 49,194 places. Allow 131,072 for reference work and keep the play seed limit.
    let limits = GatherLimits { points: 1 << 17, ..GatherLimits::default() };
    let gather = GatherConfig { threads, limits, ..GatherConfig::default() };
    // A sparse room can need a much larger atlas than its reached set. Cap the packed atlas at 256 MiB.
    let config = SolveConfig { gather, atlas_bytes: 256 << 20, ..SolveConfig::default() };
    let live = Config { emission: gather.emission, ..Config::default() };
    let mut args = Args {
        folders: Vec::new(),
        reach: false,
        validation: false,
        adapter: String::new(),
        json: None,
        lamp: None,
        config,
        live,
    };
    while let Some(arg) = rest.next() {
        let flag = arg.to_str().unwrap_or("");
        let mut value = || rest.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag {
            "--reach" => args.reach = true,
            "--validation" => args.validation = true,
            "--adapter" => args.adapter = value()?.into_string().map_err(|_| "invalid adapter name")?,
            "--json" => args.json = Some(value()?.into()),
            "--threads" => args.config.gather.threads = value()?.to_string_lossy().parse()?,
            "--rays" => args.config.gather.rays = value()?.to_string_lossy().parse()?,
            "--points" => args.live.points = value()?.to_string_lossy().parse()?,
            "--near" => args.live.near = value()?.to_string_lossy().parse()?,
            "--lamp" => args.lamp = Some(value()?.to_string_lossy().parse()?),
            "--" => {
                args.folders.extend(rest.map(PathBuf::from));
                break;
            }
            _ if flag.starts_with('-') => return Err(format!("unknown option {flag}\n{USAGE}").into()),
            _ => args.folders.push(arg.into()),
        }
    }
    if args.folders.is_empty() {
        return Err(USAGE.into());
    }
    if !(1..=MOST_THREADS).contains(&args.config.gather.threads) || !(1..=MOST_RAYS).contains(&args.config.gather.rays) {
        return Err(format!("threads must be in 1..={MOST_THREADS}, rays in 1..={MOST_RAYS}").into());
    }
    if args.reach && args.lamp.is_some() {
        return Err("--lamp selects the lamp-off state of gate D".into());
    }
    if args.live.points == 0 || !(args.live.near > 0.0 && args.live.near < 1.0) {
        return Err("points per dispatch must be positive and near must be between 0 and 1".into());
    }
    Ok(args)
}

struct Report {
    passed: bool,
    line: String,
    detail: Value,
}

impl Report {
    fn failed(e: Box<dyn std::error::Error>) -> Self {
        let line = e.to_string();
        Report { passed: false, detail: json!({ "error": line }), line }
    }
}

fn room(gpu: &gpu::Gpu, args: &Args, folder: &Path) -> Report {
    let mut scene_warning = None;
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<Report> {
        let capture = dump::loaded(folder, args.config.gather.threads)?;
        let stats = capture.scene.stats();
        println!("{} lamps, {} seeds", capture.light.lamps.len(), capture.seeds.points.len());
        if stats.sound() {
            println!("{stats:?}");
        } else {
            let warning = format!("Scene preparation omitted or replaced input: {stats:?}");
            eprintln!("warning: {warning}");
            scene_warning = Some(warning);
        }
        let bench = stage::Bench::open(gpu, capture, args.config, args.live)?;
        let mut report = if args.reach { reach::run(&bench) } else { compare::run(&bench, args.lamp) }.unwrap_or_else(Report::failed);
        report.detail["scene"] = json!(bench.times);
        Ok(report)
    }));
    let mut report = match result {
        Ok(report) => report.unwrap_or_else(Report::failed),
        Err(payload) => {
            let why = payload.downcast_ref::<String>().map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied()).unwrap_or("unknown panic payload");
            Report::failed(format!("room panicked: {why}").into())
        }
    };
    if let Some(warning) = scene_warning {
        report.detail["scene_warning"] = json!(warning);
    }
    report
}

fn save(path: Option<&Path>, summary: &Value) -> Result<()> {
    if let Some(path) = path {
        std::fs::write(path, serde_json::to_vec_pretty(summary)?).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

fn run(args: Args) -> Result<ExitCode> {
    let gate = if args.reach { "R" } else { "D" };
    let targets: &[Target] = if args.reach { &[Target::Moments, Target::Reach] } else { &[Target::Moments] };
    validation::listen();
    let gpu = match gpu::Gpu::open(&args.adapter, args.validation, targets) {
        Ok(gpu) => gpu,
        Err(why) => {
            let (code, reason) = match why {
                gpu::Refused::Unsupported(s) => (2, s),
                gpu::Refused::Failed(s) => (1, s),
            };
            eprintln!("{reason}");
            let summary = json!({ "gate": gate, "passed": false, "exit_code": code, "error": reason, "rooms": [] });
            if let Err(e) = save(args.json.as_deref(), &summary) {
                eprintln!("{e}");
            }
            return Ok(ExitCode::from(code));
        }
    };
    let adapter = gpu.describe();
    println!("{adapter}");
    let mut passed = true;
    let mut lines = Vec::new();
    let mut rooms = Vec::new();
    for folder in &args.folders {
        println!("\n{}", folder.display());
        let name = folder.file_name().unwrap_or(folder.as_os_str()).to_string_lossy();
        let report = room(&gpu, &args, folder);
        passed &= report.passed;
        lines.push(format!("{name:24} {gate}   {}   {}", word(report.passed), report.line));
        rooms.push(json!({ "room": name, "folder": folder.to_string_lossy(), "passed": report.passed, "detail": report.detail }));
    }
    let device_error = gpu.sound().err();
    if let Some(e) = &device_error {
        eprintln!("GPU: {e}");
    }
    drop(gpu);
    let quiet = validation::quiet(args.validation);
    passed &= quiet && device_error.is_none();
    let code = u8::from(!passed);
    let estimate = "Prepared scene and seeds + tables + device buffers + BLAS/TLAS + classification and flood + first solve and readback + packing + full-sphere fit. The read ms column is the first solve's readback. Excludes adapter open (including kernel/pipeline creation), dump I/O, decoding, reference checks, in-game collection and renderer publication.";
    if args.reach {
        println!("\nCapture-to-publication estimate (wall ms): {estimate}");
    }
    let header = if args.reach { reach::HEADER } else { compare::HEADER };
    println!("\n{:<24} gate pass   {header}", "room");
    for line in lines {
        println!("{line}");
    }
    let summary = json!({
        "gate": gate, "passed": passed, "exit_code": code, "adapter": adapter, "device_error": device_error,
        "validation_requested": args.validation, "validation_and_log_passed": quiet, "rays": args.config.gather.rays, "threads": args.config.gather.threads,
        "points_per_dispatch_limit": args.live.points, "near": args.live.near,
        "capture_to_publication_estimate": args.reach.then_some(estimate), "rooms": rooms
    });
    save(args.json.as_deref(), &summary)?;
    println!("{}", word(passed));
    Ok(ExitCode::from(code))
}

fn main() -> ExitCode {
    arguments(std::env::args_os().skip(1)).and_then(run).unwrap_or_else(|e| {
        eprintln!("{e}");
        ExitCode::FAILURE
    })
}
