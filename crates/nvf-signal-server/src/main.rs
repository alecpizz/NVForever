//! The `nvf-signal-server` binary: see the library's notes and `--help`.

use std::time::Duration;

use nvf_signal_server::{parse_args, Server};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (listen, cfg) = match parse_args(&args) {
        Ok(r) => r,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    let server = match Server::start(listen, cfg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("nvf-signal-server: cannot listen on {listen}: {e}");
            std::process::exit(1);
        }
    };
    println!("nvf-signal-server: serving on http://{} (NVF_COOP_SIGNAL=http://{})", server.addr, server.addr);
    let mut last = usize::MAX;
    loop {
        std::thread::sleep(Duration::from_secs(10));
        let n = server.lobbies();
        if n != last {
            println!("nvf-signal-server: {n} lobbies");
            last = n;
        }
    }
}
