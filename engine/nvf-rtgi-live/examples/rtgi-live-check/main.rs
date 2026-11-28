//! The live kernel held against nvf-rtgi on scenes made in code (#3009), on a GPU with ray queries; no game data.
//!
//!     cargo run --release -p nvf-rtgi-live --example rtgi-live-check -- [--validation] [--adapter <part of a name>]
//!
//! Each fixture's rays are held against the CPU's (`nvf_rtgi::bounces`) one by one: what they meet, the light they
//! bring and their lamps and shadow rays. Each point's hits, backs, lamps and shadow rays from the solve are held
//! against the sums of those answers, its reach (hits and backs) against `nvf_rtgi::classified`, and the triangle
//! test against `nvf_rtgi::meet`, all bit for bit but the light (within 5e-4). A difference that is known is
//! reported as KNOWN with its reason and decides nothing. Exit 0 when all the rest pass, 1 on a FAIL or an error,
//! 2 when no Vulkan adapter has ray queries. `--validation` turns the Vulkan validation layer on; any error it or
//! wgpu reports is a FAIL, and so is a layer that is not installed.

mod corpus;
mod fixtures;
mod gpu;
mod validation;

use std::process::ExitCode;

fn main() -> ExitCode {
    validation::listen();
    let (mut validate, mut name, mut args) = (false, String::new(), std::env::args().skip(1));
    while let Some(a) = args.next() {
        match (a.as_str(), args.len()) {
            ("--validation", _) => validate = true,
            ("--adapter", 1..) => name = args.next().unwrap_or_default(),
            _ => {
                eprintln!("rtgi-live-check [--validation] [--adapter <part of a name>]");
                return ExitCode::FAILURE;
            }
        }
    }
    let gpu = match gpu::Gpu::open(&name, validate, &gpu::FIXTURE_TARGETS) {
        Ok(gpu) => gpu,
        Err(gpu::Refused::Unsupported(why)) => {
            println!("{why}");
            return ExitCode::from(2);
        }
        Err(gpu::Refused::Failed(why)) => {
            println!("FAIL: {why}");
            return ExitCode::FAILURE;
        }
    };
    println!("{}", gpu.describe());
    let passed = fixtures::run(&gpu).unwrap_or_else(|e| {
        println!("FAIL: {e}");
        false
    });
    let sound = gpu.sound().map_err(|e| println!("FAIL: {e}")).is_ok();
    drop(gpu);
    let quiet = validation::quiet(validate);
    let all = passed && sound && quiet;
    println!("check: {}", if all { "PASS" } else { "FAIL" });
    if all {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
