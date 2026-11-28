//! The `nvf-relay` binary: see the library's notes and `nvf-relay --help`.

use std::time::{Duration, Instant};

use nvf_relay::{parse_args, Relay, STATUS_EVERY};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (listen, cfg) = match parse_args(&args) {
        Ok(r) => r,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    let relay = match Relay::start(&listen, cfg) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nvf-relay: cannot listen on {listen:?}: {e}");
            std::process::exit(1);
        }
    };
    for a in &relay.addrs {
        println!("nvf-relay: listening on udp {a}");
    }
    let mut last_status = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(1));
        relay.expire();
        if last_status.elapsed() >= STATUS_EVERY {
            last_status = Instant::now();
            let s = relay.stats();
            println!(
                "nvf-relay: {} sessions, {} packets ({} KB) forwarded, {} dropped, {} refused",
                relay.sessions(),
                s.forwarded,
                s.forwarded_bytes / 1024,
                s.dropped,
                s.refused
            );
        }
    }
}
