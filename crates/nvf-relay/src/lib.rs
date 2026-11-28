//! `nvf-relay`: a co-op relay a player hosts (#3795 section 9, option 2). The state machine and
//! wire format are `nvf_nat::hosted`; this is the sockets around it, plus OS randomness for
//! return-check cookie secrets (#3908).
//!
//! One socket per `--listen` address (an IPv4 one and an IPv6 one, say), sharing one table of
//! sessions, so a guest on IPv6 and a host on IPv4 still meet. A packet goes out on the socket
//! of the destination's family.

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub use nvf_nat::hosted::{ServerConfig, Stats};
use nvf_nat::hosted::Server;

/// How often a status line is printed.
pub const STATUS_EVERY: Duration = Duration::from_secs(60);

/// A running relay: its sockets' addresses, and a switch to stop it.
pub struct Relay {
    pub addrs: Vec<SocketAddr>,
    stop: Arc<AtomicBool>,
    server: Arc<Mutex<Server>>,
    threads: Vec<thread::JoinHandle<()>>,
}

/// A 256-bit secret from the operating system's random source, for return-check cookies (#3908).
pub fn os_secret() -> [u8; 32] {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("the operating system's random source");
    b
}

impl Relay {
    /// Binds every address and starts one thread per socket.
    pub fn start(listen: &[SocketAddr], mut cfg: ServerConfig) -> io::Result<Relay> {
        if cfg.random_secret.is_none() {
            cfg.random_secret = Some(os_secret);
        }
        let mut sockets = Vec::new();
        for a in listen {
            let s = UdpSocket::bind(a)?;
            s.set_read_timeout(Some(Duration::from_millis(250)))?;
            sockets.push(s);
        }
        let addrs = sockets.iter().map(UdpSocket::local_addr).collect::<io::Result<Vec<_>>>()?;
        let sockets = Arc::new(sockets);
        let server = Arc::new(Mutex::new(Server::new(cfg)));
        let stop = Arc::new(AtomicBool::new(false));
        let threads = (0..sockets.len())
            .map(|i| {
                let (sockets, server, stop) = (sockets.clone(), server.clone(), stop.clone());
                thread::spawn(move || serve(i, &sockets, &server, &stop))
            })
            .collect();
        Ok(Relay { addrs, stop, server, threads })
    }

    pub fn stats(&self) -> Stats {
        self.server.lock().expect("relay lock").stats()
    }

    pub fn sessions(&self) -> usize {
        self.server.lock().expect("relay lock").sessions()
    }

    /// Lets go of silent sides. The binary calls this each second.
    pub fn expire(&self) {
        self.server.lock().expect("relay lock").expire(Instant::now());
    }

    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads {
            let _ = t.join();
        }
    }
}

fn serve(me: usize, sockets: &[UdpSocket], server: &Mutex<Server>, stop: &AtomicBool) {
    let mut buf = [0u8; 2048];
    while !stop.load(Ordering::Relaxed) {
        let (n, from) = match sockets[me].recv_from(&mut buf) {
            Ok(r) => r,
            // Timeouts, and on Windows the ICMP "port unreachable" a send earlier caused.
            Err(_) => continue,
        };
        let out = server.lock().expect("relay lock").on_datagram(from, &buf[..n], Instant::now());
        for (to, bytes) in out {
            let sock = sockets.iter().find(|s| s.local_addr().is_ok_and(|l| l.is_ipv4() == to.is_ipv4())).unwrap_or(&sockets[me]);
            let _ = sock.send_to(&bytes, to);
        }
    }
}

/// The command line: `--listen ADDR` (repeatable, default `0.0.0.0:47961`), `--max-sessions N`,
/// `--rate BYTES_PER_SECOND`. Returns the addresses and limits, or a message for the user.
pub fn parse_args(args: &[String]) -> Result<(Vec<SocketAddr>, ServerConfig), String> {
    let mut listen = Vec::new();
    let mut cfg = ServerConfig::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--listen" => {
                let v = value("--listen")?;
                listen.push(v.parse().map_err(|_| format!("--listen {v}: not an address such as 0.0.0.0:47961 or [::]:47961"))?);
            }
            "--max-sessions" => {
                let v = value("--max-sessions")?;
                cfg.max_sessions = v.parse().map_err(|_| format!("--max-sessions {v}: not a number"))?;
            }
            "--rate" => {
                let v = value("--rate")?;
                cfg.rate = v.parse().map_err(|_| format!("--rate {v}: not a number of bytes a second"))?;
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument {other}\n\n{USAGE}")),
        }
    }
    if listen.is_empty() {
        listen.push(SocketAddr::from(([0, 0, 0, 0], 47961)));
    }
    Ok((listen, cfg))
}

pub const USAGE: &str = "nvf-relay: a co-op relay for friends whose networks cannot reach each other.

Players put this machine's public address in their game's [Coop] Relay=host:port setting.
Game traffic stays end-to-end encrypted: the relay cannot read or change it.

  --listen ADDR          a UDP address to listen on (repeatable; default 0.0.0.0:47961,
                         add [::]:47961 for IPv6)
  --max-sessions N       pairs of players held at once (default 256)
  --rate BYTES           bytes a second each player may send (default 262144)

Open the UDP port on your firewall and router. The relay keeps nothing on disk.";

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses() {
        let (l, c) = parse_args(&[]).unwrap();
        assert_eq!(l, vec![SocketAddr::from(([0, 0, 0, 0], 47961))]);
        assert_eq!(c.max_sessions, 256);
        let (l, c) = parse_args(&args("--listen 127.0.0.1:5000 --listen [::1]:5000 --max-sessions 4 --rate 1000")).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!((c.max_sessions, c.rate), (4, 1000));
        assert!(parse_args(&args("--listen nope")).is_err());
        assert!(parse_args(&args("--rate")).is_err());
        assert!(parse_args(&args("--bogus")).is_err());
    }
}
