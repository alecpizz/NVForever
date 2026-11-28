//! `nvf-signal-server`: co-op matchmaking anyone can run (#3795 section 6, "Self-hosting", and
//! section 12's offline tests). It is `nvf_signal::standin::StandIn`, the same rules the
//! Cloudflare Worker follows (`docs/design/coop-signal-wire.md`), behind one TCP port:
//!
//! - `POST /v1/lobbies`, `GET /v1/lobbies/{code}`, and the lobby browser's `GET /v1/browse` and
//!   `POST /v1/lobbies/{code}/report`: plain HTTP/1.1, one request a connection.
//! - `GET /v1/lobbies/{code}/ws`: the lobby WebSocket (`ws://`).
//!
//! No TLS: on loopback or a LAN the game takes `NVF_COOP_SIGNAL=http://host:port`; on the
//! internet, put a TLS proxy in front. No rate limit, no logs of addresses, nothing on disk.
//! One thread per connection, which is plenty for a handful of lobbies.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nvf_signal::standin::{Config, ConnId, Out, StandIn};
use tungstenite::{Message, WebSocket};

/// The largest request head read (request line and headers).
const MAX_HEAD: usize = 8 * 1024;
/// The largest request body read (a create call is a few hundred bytes).
const MAX_BODY: usize = 16 * 1024;

/// A random u64 from the operating system, for codes and secrets.
pub fn os_random() -> u64 {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).expect("the operating system's random source");
    u64::from_le_bytes(b)
}

/// A config for a real deployment: codes and secrets from [`os_random`].
pub fn deployment_config(min_protocol: u32, stun: Vec<String>) -> Config {
    Config { min_protocol, stun, random: Some(os_random), ..Config::default() }
}

struct Shared {
    standin: Mutex<StandIn>,
    sockets: Mutex<HashMap<ConnId, Sender<(String, bool)>>>,
    stop: AtomicBool,
}

impl Shared {
    fn dispatch(&self, outs: Vec<Out>) {
        let socks = self.sockets.lock().expect("sockets lock");
        for o in outs {
            if let Some(tx) = socks.get(&o.conn) {
                let _ = tx.send((o.text, o.close));
            }
        }
    }
}

/// A running server.
pub struct Server {
    pub addr: SocketAddr,
    shared: Arc<Shared>,
}

impl Server {
    /// Listens on `listen` and serves until [`Server::stop`].
    pub fn start(listen: SocketAddr, cfg: Config) -> io::Result<Server> {
        let listener = TcpListener::bind(listen)?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let shared = Arc::new(Shared { standin: Mutex::new(StandIn::new(cfg)), sockets: Mutex::new(HashMap::new()), stop: AtomicBool::new(false) });
        let s = shared.clone();
        thread::spawn(move || {
            let mut last_tick = Instant::now();
            while !s.stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let s = s.clone();
                        thread::spawn(move || {
                            if let Err(e) = handle(stream, &s) {
                                eprintln!("nvf-signal-server: a request failed: {e}");
                            }
                        });
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(20)),
                    Err(_) => thread::sleep(Duration::from_millis(20)),
                }
                if last_tick.elapsed() >= Duration::from_secs(1) {
                    last_tick = Instant::now();
                    let outs = s.standin.lock().expect("stand-in lock").tick(Instant::now());
                    s.dispatch(outs);
                }
            }
        });
        Ok(Server { addr, shared })
    }

    /// Lobbies alive.
    pub fn lobbies(&self) -> usize {
        self.shared.standin.lock().expect("stand-in lock").lobbies()
    }

    /// Stops accepting; open sockets end on their next read.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The request line and headers, peeked (not consumed) so a WebSocket upgrade can still read
/// them. Returns (method, path, head length, content length).
fn peek_head(stream: &TcpStream) -> io::Result<(String, String, usize, usize)> {
    let mut buf = vec![0u8; MAX_HEAD];
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let n = stream.peek(&mut buf)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        if let Some(end) = buf[..n].windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).into_owned();
            let mut lines = head.split("\r\n");
            let mut first = lines.next().unwrap_or("").split(' ');
            let method = first.next().unwrap_or("").to_string();
            let path = first.next().unwrap_or("").to_string();
            let length = lines
                .filter_map(|l| l.split_once(':'))
                .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.trim().parse().ok())
                .unwrap_or(0);
            return Ok((method, path, end + 4, length));
        }
        if n == MAX_HEAD || Instant::now() >= deadline {
            return Err(io::ErrorKind::InvalidData.into());
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "Error",
    }
}

fn respond(mut stream: TcpStream, status: u16, body: &str) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        reason(status),
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

fn not_found() -> String {
    r#"{"error":"not_found","message":"no such endpoint"}"#.to_string()
}

fn handle(mut stream: TcpStream, s: &Shared) -> io::Result<()> {
    // Accepted from a non-blocking listener, the stream is non-blocking too on some systems.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let (method, path, head_len, length) = peek_head(&stream)?;
    let (path, query) = path.split_once('?').map(|(p, q)| (p.to_string(), q.to_string())).unwrap_or((path, String::new()));
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    // The lobby browser (wire 2).
    if method == "GET" && parts == ["v1", "browse"] {
        let mut buf = vec![0u8; head_len];
        stream.read_exact(&mut buf)?;
        let (status, out) = s.standin.lock().expect("stand-in lock").browse(&query, Instant::now());
        return respond(stream, status, &out);
    }
    if parts.len() < 2 || parts[0] != "v1" || parts[1] != "lobbies" {
        // Read what was sent first: closing on unread bytes resets the connection.
        let mut buf = vec![0u8; head_len];
        let _ = stream.read_exact(&mut buf);
        return respond(stream, 404, &not_found());
    }
    match (method.as_str(), parts.len()) {
        ("POST", 2) => {
            if length > MAX_BODY {
                return respond(stream, 413, r#"{"error":"bad_request","message":"too large"}"#);
            }
            let mut buf = vec![0u8; head_len + length];
            stream.read_exact(&mut buf)?;
            let body = String::from_utf8_lossy(&buf[head_len..]).into_owned();
            let unix = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let (status, out) = s.standin.lock().expect("stand-in lock").create(&body, Instant::now(), unix);
            respond(stream, status, &out)
        }
        ("GET", 3) => {
            let mut buf = vec![0u8; head_len];
            stream.read_exact(&mut buf)?;
            let (status, out) = s.standin.lock().expect("stand-in lock").info(parts[2]);
            respond(stream, status, &out)
        }
        ("POST", 4) if parts[3] == "report" => {
            if length > MAX_BODY {
                return respond(stream, 413, r#"{"error":"bad_request","message":"too large"}"#);
            }
            let mut buf = vec![0u8; head_len + length];
            stream.read_exact(&mut buf)?;
            let body = String::from_utf8_lossy(&buf[head_len..]).into_owned();
            let reporter = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
            let (status, out) = s.standin.lock().expect("stand-in lock").report(parts[2], &reporter, &body);
            respond(stream, status, &out)
        }
        ("GET", 4) if parts[3] == "ws" => {
            let opened = s.standin.lock().expect("stand-in lock").open(parts[2]);
            match opened {
                Err((status, body)) => {
                    let mut buf = vec![0u8; head_len];
                    stream.read_exact(&mut buf)?;
                    respond(stream, status, &body)
                }
                Ok(conn) => socket(stream, conn, s),
            }
        }
        _ => {
            let mut buf = vec![0u8; head_len];
            let _ = stream.read_exact(&mut buf);
            respond(stream, 404, &not_found())
        }
    }
}

/// One lobby socket: frames in go to the stand-in, what it sends this socket goes out.
fn socket(stream: TcpStream, conn: ConnId, s: &Shared) -> io::Result<()> {
    let (tx, rx) = mpsc::channel();
    s.sockets.lock().expect("sockets lock").insert(conn, tx);
    let result = (|| -> io::Result<()> {
        let mut ws: WebSocket<TcpStream> = tungstenite::accept(stream).map_err(|e| io::Error::other(e.to_string()))?;
        ws.get_ref().set_read_timeout(Some(Duration::from_millis(20)))?;
        loop {
            if s.stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            while let Ok((text, close)) = rx.try_recv() {
                let _ = ws.send(Message::text(text));
                if close {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    return Ok(());
                }
            }
            match ws.read() {
                Ok(Message::Text(t)) => {
                    let outs = s.standin.lock().expect("stand-in lock").message(conn, t.as_str(), Instant::now());
                    s.dispatch(outs);
                }
                Ok(Message::Close(_)) => return Ok(()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
                Err(_) => return Ok(()),
            }
        }
    })();
    s.sockets.lock().expect("sockets lock").remove(&conn);
    let outs = s.standin.lock().expect("stand-in lock").close(conn, Instant::now());
    s.dispatch(outs);
    result
}

/// The command line: `--listen ADDR` (default `127.0.0.1:8787`), `--stun HOST:PORT` (repeat;
/// default Cloudflare's and Google's), `--min-protocol N`.
pub fn parse_args(args: &[String]) -> Result<(SocketAddr, Config), String> {
    let mut listen: SocketAddr = SocketAddr::from(([127, 0, 0, 1], 8787));
    let mut stun = Vec::new();
    let mut min_protocol = 0;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
        match a.as_str() {
            "--listen" => {
                let v = value("--listen")?;
                listen = v.parse().map_err(|_| format!("--listen {v}: not an address such as 0.0.0.0:8787"))?;
            }
            "--stun" => stun.push(value("--stun")?),
            "--min-protocol" => {
                let v = value("--min-protocol")?;
                min_protocol = v.parse().map_err(|_| format!("--min-protocol {v}: not a number"))?;
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument {other}\n\n{USAGE}")),
        }
    }
    if stun.is_empty() {
        stun = Config::default().stun;
    }
    Ok((listen, deployment_config(min_protocol, stun)))
}

pub const USAGE: &str = "nvf-signal-server: co-op matchmaking anyone can run, in place of coop.nv-forever.com.

Players set NVF_COOP_SIGNAL=http://this-machine:port (or [Coop] SignalUrl). It only introduces
players: game traffic goes between the games. No TLS: for the internet, put a TLS proxy in front.

  --listen ADDR          TCP address to serve on (default 127.0.0.1:8787; 0.0.0.0:8787 for a LAN)
  --stun HOST:PORT       a STUN server handed to hosts (repeatable; default Cloudflare's and Google's)
  --min-protocol N       the oldest game protocol let in (default: any)";
