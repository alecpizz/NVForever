//! The matchmaking service's I/O (docs/design/coop-networking.md §6,
//! docs/design/coop-signal-wire.md), on a thread of its own so the co-op network thread never
//! waits on HTTPS or a WebSocket. The thread does the REST call first (a host creates its
//! lobby, a guest looks the code up), then opens the lobby's WebSocket and passes text frames
//! both ways over channels. What the frames mean is `nvf_signal::client`'s and
//! [`crate::online`]'s business.
//!
//! HTTPS and WSS go through the system's TLS (`ureq` and `tungstenite` with native-tls, the
//! same as player reports: Windows' SChannel, the system's certificates). An `http://`
//! service (a stand-in on this PC) is plain HTTP and WS.

use std::io::ErrorKind;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use nvf_signal::code::Code;
use nvf_signal::wire::{CreateLobby, ErrorBody, LobbyCreated, LobbyInfo};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// Overrides the service ([`nvf_signal::DEFAULT_SERVICE`]); `off` turns online codes off.
pub const SERVICE_ENV: &str = "NVF_COOP_SIGNAL";
/// The STUN servers a guest asks (the service gives a host its list).
pub const DEFAULT_STUN: [&str; 2] = ["stun.cloudflare.com:3478", "stun.l.google.com:19302"];
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a read on the socket waits before the thread looks at what it has to send.
const READ_WAIT: Duration = Duration::from_millis(30);
/// A socket that sent nothing for this long sends a WebSocket ping. Through a VPN the
/// service's socket was closed after 60 s with nothing sent (close 1006, the VPN's idle
/// timeout) and kept open by a ping every 20 s [observed 2026-11-25]: a host between guests,
/// or a guest waiting for the host to let it in (up to 130 s), would lose it otherwise.
const KEEPALIVE: Duration = Duration::from_secs(20);

/// The service to use: [`SERVICE_ENV`], else the default; None when turned off.
pub fn service() -> Option<String> {
    if crate::prefs::connection() == crate::prefs::Connection::LanOnly {
        return None;
    }
    match std::env::var(SERVICE_ENV) {
        Ok(v) if v.trim().eq_ignore_ascii_case("off") => None,
        Ok(v) if !v.trim().is_empty() => Some(v.trim().trim_end_matches('/').to_string()),
        _ => Some(nvf_signal::DEFAULT_SERVICE.to_string()),
    }
}

/// What the thread tells the game.
#[derive(Debug)]
pub enum FromIo {
    /// The host's lobby was made.
    Created { code: String, host_secret: String, stun: Vec<SocketAddr> },
    /// The guest's code is a lobby.
    LookedUp(LobbyInfo),
    /// The lobby's WebSocket is open: frames can go.
    Open,
    Text(String),
    /// Something failed: what to tell the player.
    Failed(String),
    /// The socket closed.
    Closed,
}

enum ToIo {
    Text(String),
    Close,
}

/// The thread, seen from the network thread. Dropping it closes the socket (the thread ends
/// on its own; nothing waits for it).
pub struct SignalIo {
    tx: Sender<ToIo>,
    rx: Receiver<FromIo>,
}

/// What the thread starts with.
enum Start {
    Create(CreateLobby),
    Resume { code: String },
    Lookup(Code),
}

impl SignalIo {
    /// A host's lobby: created, then its socket opened.
    pub fn create(service: &str, body: CreateLobby) -> Self {
        Self::spawn(service, Start::Create(body))
    }

    /// A host's lobby again after a restart (the service keeps it 120 s): its socket only.
    pub fn resume(service: &str, code: &str) -> Self {
        Self::spawn(service, Start::Resume { code: code.to_string() })
    }

    /// A guest's lobby: the code looked up, then its socket opened.
    pub fn lookup(service: &str, code: Code) -> Self {
        Self::spawn(service, Start::Lookup(code))
    }

    fn spawn(service: &str, start: Start) -> Self {
        let (tx_in, rx_in) = mpsc::channel();
        let (tx_out, rx_out) = mpsc::channel();
        let service = service.to_string();
        let fail = tx_out.clone();
        if std::thread::Builder::new().name("coop-signal".into()).spawn(move || run(&service, start, &tx_out, &rx_in)).is_err() {
            // A thread that cannot start: the game hears why on its next look.
            let _ = fail.send(FromIo::Failed("Cannot start the matchmaking connection.".into()));
        }
        Self { tx: tx_in, rx: rx_out }
    }

    pub fn send(&self, text: String) {
        let _ = self.tx.send(ToIo::Text(text));
    }

    /// What came since the last look.
    pub fn take(&self) -> Vec<FromIo> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(m) => out.push(m),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }
}

impl Drop for SignalIo {
    fn drop(&mut self) {
        let _ = self.tx.send(ToIo::Close);
    }
}

fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    ureq::Agent::config_builder().tls_config(tls).timeout_global(Some(HTTP_TIMEOUT)).http_status_as_error(false).user_agent(concat!("NVForever/", env!("CARGO_PKG_VERSION"))).build().into()
}

/// The body of an answer, as `T`, or what to tell the player.
fn answer<T: serde::de::DeserializeOwned>(resp: Result<ureq::http::Response<ureq::Body>, ureq::Error>, not_found: &str) -> Result<T, String> {
    let mut resp = resp.map_err(|e| format!("Cannot reach the matchmaking service ({e})."))?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().map_err(|e| format!("The matchmaking service's answer was cut off ({e})."))?;
    match status {
        200..=299 => serde_json::from_str(&text).map_err(|e| format!("The matchmaking service's answer was not understood ({e}).")),
        404 => Err(not_found.into()),
        429 => Err("Too many tries: wait a minute and try again.".into()),
        _ => {
            let why = serde_json::from_str::<ErrorBody>(&text).map(|b| b.error).unwrap_or_default();
            Err(match why.as_str() {
                "protocol" => "The matchmaking service has turned this game version off: update NVForever.".into(),
                "wire_version" => "This game's matchmaking is too old for the service: update NVForever.".into(),
                _ => format!("The matchmaking service said no ({status} {why})."),
            })
        }
    }
}

/// The public lobbies (`GET /v1/browse`, the lobby browser), blocking: call it from a
/// thread of its own.
pub fn browse(service: &str, query: &nvf_signal::wire::BrowseQuery) -> Result<nvf_signal::wire::BrowseAnswer, String> {
    let agent = agent();
    let url = format!("{service}/v1/browse{}", query.to_query());
    answer(tries(|| agent.get(url.as_str()).call()), "The matchmaking service has no lobby browser: update the service.")
}

/// Report a public lobby (`POST /v1/lobbies/{code}/report`), blocking.
pub fn report(service: &str, code: &str) -> Result<(), String> {
    let agent = agent();
    let url = format!("{service}/v1/lobbies/{}/report", code.replace('-', ""));
    answer::<serde_json::Value>(tries(|| agent.post(url.as_str()).header("content-type", "application/json").send("{}")), "That lobby has closed.").map(|_| ())
}

fn ws_url(service: &str, code: &str) -> String {
    let base = service.replacen("https://", "wss://", 1).replacen("http://", "ws://", 1);
    format!("{base}/v1/lobbies/{code}/ws")
}

/// The first IPv4 address of each `host:port` (IPv4: the game's STUN asks go from its IPv4
/// socket), in order.
pub fn resolve_stun(servers: &[String]) -> Vec<SocketAddr> {
    servers.iter().filter_map(|s| s.to_socket_addrs().ok()?.find(SocketAddr::is_ipv4)).collect()
}

fn run(service: &str, start: Start, out: &Sender<FromIo>, cmds: &Receiver<ToIo>) {
    let fail = |why: String| {
        let _ = out.send(FromIo::Failed(why));
    };
    let agent = agent();
    let code = match start {
        Start::Create(body) => {
            let json = serde_json::to_string(&body).expect("a lobby body serialises");
            let resp = tries(|| agent.post(format!("{service}/v1/lobbies")).header("content-type", "application/json").send(json.as_str()));
            match answer::<LobbyCreated>(resp, "The matchmaking service is not there.") {
                Ok(made) => {
                    let stun = resolve_stun(&made.stun);
                    let code = made.code.clone();
                    let _ = out.send(FromIo::Created { code: made.code, host_secret: made.host_secret, stun });
                    code
                }
                Err(why) => return fail(why),
            }
        }
        Start::Resume { code } => code,
        Start::Lookup(code) => {
            let compact = code.compact();
            match answer::<LobbyInfo>(tries(|| agent.get(format!("{service}/v1/lobbies/{compact}")).call()), "Code not found, or the lobby has closed: check it with the host.") {
                Ok(info) => {
                    let _ = out.send(FromIo::LookedUp(info));
                    compact
                }
                Err(why) => return fail(why),
            }
        }
    };
    let mut ws = match connect(&ws_url(service, &code)) {
        Ok(ws) => ws,
        Err(e) => return fail(format!("Cannot open the lobby's connection ({e}).")),
    };
    set_read_wait(&ws);
    let _ = out.send(FromIo::Open);
    pump(&mut ws, out, cmds, KEEPALIVE);
}

/// Pass frames both ways until either side closes, pinging after `keepalive` with nothing
/// sent (tungstenite answers the service's pings itself).
fn pump<S: std::io::Read + std::io::Write>(ws: &mut WebSocket<S>, out: &Sender<FromIo>, cmds: &Receiver<ToIo>, keepalive: Duration) {
    let mut sent_at = Instant::now();
    loop {
        if sent_at.elapsed() >= keepalive {
            sent_at = Instant::now();
            if ws.send(Message::Ping(Default::default())).is_err() {
                let _ = out.send(FromIo::Closed);
                return;
            }
        }
        // What the game has to send first.
        loop {
            match cmds.try_recv() {
                Ok(ToIo::Text(t)) => {
                    sent_at = Instant::now();
                    if ws.send(Message::text(t)).is_err() {
                        let _ = out.send(FromIo::Closed);
                        return;
                    }
                }
                Ok(ToIo::Close) | Err(TryRecvError::Disconnected) => {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    return;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        match ws.read() {
            Ok(Message::Text(t)) => {
                let _ = out.send(FromIo::Text(t.to_string()));
            }
            Ok(Message::Close(_)) => {
                let _ = out.send(FromIo::Closed);
                return;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => {
                let _ = out.send(FromIo::Closed);
                return;
            }
        }
    }
}

/// How many times a request or the lobby's socket is tried.
const TRIES: usize = 5;
/// The wait before each try after the first, when no Retry-After says otherwise (seconds).
const BACKOFF: [u64; TRIES - 1] = [1, 2, 3, 4];
/// The longest Retry-After waited for (seconds): the zone's block lasts 10; the Worker's own
/// lookup limit asks 60, longer than a player should stare at "joining", so that one ends
/// the tries at once [documented: docs/design/coop-signal-wire.md, rate limit].
const RETRY_AFTER_MAX: u64 = 30;
/// The wait after a 429 that carried no readable Retry-After (seconds): the block's length.
const RETRY_AFTER_DEFAULT: u64 = 10;

/// The wait a 429's Retry-After asks for (seconds, as a number; an HTTP date or nothing
/// gives [`RETRY_AFTER_DEFAULT`]); `None` past [`RETRY_AFTER_MAX`]: no more tries.
fn retry_after(header: Option<&str>) -> Option<u64> {
    Some(header.and_then(|h| h.trim().parse::<u64>().ok()).unwrap_or(RETRY_AFTER_DEFAULT)).filter(|w| *w <= RETRY_AFTER_MAX)
}

/// A request, tried again after a connection error (as [`connect`]) and after a 429 (too
/// many requests: the service's rate limit), waiting the answer's Retry-After first so a
/// burst of tries does not keep the block going. Any other HTTP answer is not tried again;
/// the last 429 goes back to the caller.
fn tries(mut request: impl FnMut() -> Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
    let mut wait = 0;
    for n in 0..TRIES {
        std::thread::sleep(Duration::from_secs(wait));
        let last = n + 1 == TRIES;
        match request() {
            Ok(r) if r.status().as_u16() == 429 && !last => {
                let Some(w) = retry_after(r.headers().get("retry-after").and_then(|v| v.to_str().ok())) else { return Ok(r) };
                wait = w;
                log::info!("co-op: the matchmaking service asks to wait (429); trying again in {wait} s");
            }
            Ok(r) => return Ok(r),
            Err(e) if !last => {
                log::info!("co-op: the matchmaking service's request failed ({e}); trying again");
                wait = BACKOFF.get(n).copied().unwrap_or_default();
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("the last try returns")
}

/// The lobby's socket, tried again after a connection error: in the game, a TLS handshake
/// was sometimes reset (os error 10054) where the same request from a test went through
/// [observed 2026-11-24, 2 of 3 game runs; 15 of 15 test opens]. An HTTP answer (a lobby
/// that is not there) is not tried again, except a 429 (the rate limit), which waits its
/// Retry-After as [`tries`] does.
fn connect(url: &str) -> tungstenite::Result<WebSocket<MaybeTlsStream<TcpStream>>> {
    let mut wait = 0;
    for n in 0..TRIES {
        std::thread::sleep(Duration::from_secs(wait));
        let last = n + 1 == TRIES;
        match tungstenite::connect(url) {
            Ok((ws, _)) => return Ok(ws),
            Err(tungstenite::Error::Http(resp)) if resp.status().as_u16() == 429 && !last => {
                let Some(w) = retry_after(resp.headers().get("retry-after").and_then(|v| v.to_str().ok())) else {
                    return Err(tungstenite::Error::Http(resp));
                };
                wait = w;
                log::info!("co-op: the matchmaking service asks to wait (429); opening the lobby's connection again in {wait} s");
            }
            Err(e @ tungstenite::Error::Http(_)) => return Err(e),
            Err(e) if !last => {
                log::info!("co-op: the lobby's connection failed ({e}); trying again");
                wait = BACKOFF.get(n).copied().unwrap_or_default();
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("the last try returns")
}

/// Reads give up after [`READ_WAIT`], so the thread can send between them.
fn set_read_wait(ws: &WebSocket<MaybeTlsStream<TcpStream>>) {
    let tcp: Option<&TcpStream> = match ws.get_ref() {
        MaybeTlsStream::Plain(s) => Some(s),
        MaybeTlsStream::NativeTls(s) => Some(s.get_ref()),
        _ => None,
    };
    if let Some(s) = tcp {
        let _ = s.set_read_timeout(Some(READ_WAIT));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A socket with nothing to send pings the service after the keepalive, and frames still
    /// pass both ways.
    #[test]
    fn an_idle_socket_pings_the_service() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let at = listener.local_addr().unwrap();
        let (out_tx, out_rx) = mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let client = std::thread::spawn(move || {
            let tcp = TcpStream::connect(at).unwrap();
            tcp.set_read_timeout(Some(READ_WAIT)).unwrap();
            let (mut ws, _) = tungstenite::client(format!("ws://{at}/"), tcp).unwrap();
            pump(&mut ws, &out_tx, &cmd_rx, Duration::from_millis(200));
        });
        let (tcp, _) = listener.accept().unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut server = tungstenite::accept(tcp).unwrap();
        let t = Instant::now();
        assert!(matches!(server.read().unwrap(), Message::Ping(_)), "a ping first");
        assert!(t.elapsed() >= Duration::from_millis(150), "not before the keepalive: {:?}", t.elapsed());
        cmd_tx.send(ToIo::Text("hello".into())).unwrap();
        // The pong to the client's ping comes back to it on its own; the text follows.
        let next = loop {
            match server.read().unwrap() {
                Message::Ping(_) => continue,
                m => break m,
            }
        };
        assert_eq!(next, Message::text("hello"));
        server.send(Message::text("back")).unwrap();
        assert!(matches!(out_rx.recv_timeout(Duration::from_secs(5)), Ok(FromIo::Text(t)) if t == "back"));
        drop(cmd_tx);
        client.join().unwrap();
    }

    /// Against the live service (run by hand: `--ignored --nocapture`): fourteen lookups of a
    /// code that does not exist, past the rate limit (10 in 10 s): the ones past it wait the
    /// 429's Retry-After inside [`tries`] and then get their 404.
    #[test]
    #[ignore]
    fn live_rate_limit_waits() {
        let agent = agent();
        let service = nvf_signal::DEFAULT_SERVICE;
        for i in 0..14 {
            let t = std::time::Instant::now();
            let what = match tries(|| agent.get(format!("{service}/v1/lobbies/ZZZZZZ")).call()) {
                Ok(r) => format!("http {}", r.status()),
                Err(e) => format!("error {e}"),
            };
            println!("lookup {i}: {what} after {:.1} s", t.elapsed().as_secs_f32());
        }
    }

    /// A 429's Retry-After: seconds as given; past the cap, no wait; a date, junk or none waits
    /// the block.
    #[test]
    fn retry_after_reads_seconds() {
        assert_eq!(retry_after(Some("7")), Some(7));
        assert_eq!(retry_after(Some(" 3 ")), Some(3));
        assert_eq!(retry_after(Some("60")), None, "the Worker's minute is not waited out");
        assert_eq!(retry_after(Some("Wed, 21 Oct 2026 07:28:00 GMT")), Some(RETRY_AFTER_DEFAULT));
        assert_eq!(retry_after(None), Some(RETRY_AFTER_DEFAULT));
    }

    /// Against the live service (run by hand: `--ignored --nocapture`): opens the lobby
    /// socket of a code that does not exist ten times and prints how each attempt ended.
    #[test]
    #[ignore]
    fn live_socket_attempts() {
        for i in 0..10 {
            let r = tungstenite::connect(ws_url(nvf_signal::DEFAULT_SERVICE, "AAAAAA"));
            let what = match r {
                Ok(_) => "open".to_string(),
                Err(tungstenite::Error::Http(resp)) => format!("http {}", resp.status()),
                Err(e) => format!("error: {e}"),
            };
            println!("attempt {i}: {what}");
        }
    }

    /// Against the live service (run by hand): makes a lobby, then opens its socket as the
    /// host and as a guest, five times, printing how each ended.
    #[test]
    #[ignore]
    fn live_lobby_attempts() {
        let svc = nvf_signal::DEFAULT_SERVICE;
        for i in 0..5 {
            let body = CreateLobby { wire: 1, protocol: crate::net::VERSION as u32, build: "test".into(), load_order_hash: String::new(), max_players: 6, host_key: "ab".repeat(32), name: "probe".into(), listing: None };
            let resp = agent().post(format!("{svc}/v1/lobbies")).header("content-type", "application/json").send(serde_json::to_string(&body).unwrap().as_str());
            let made: LobbyCreated = match answer(resp, "404") {
                Ok(m) => m,
                Err(e) => {
                    println!("attempt {i}: create failed: {e}");
                    continue;
                }
            };
            let mut ok = Vec::new();
            for who in ["host", "guest"] {
                let r = tungstenite::connect(ws_url(svc, &made.code));
                ok.push(match r {
                    Ok((mut ws, _)) => {
                        let first = if who == "host" { format!(r#"{{"type":"host","wire":1,"host_secret":"{}"}}"#, made.host_secret) } else { format!(r#"{{"type":"join","wire":1,"name":"g","key":"{}","candidates":[]}}"#, "cd".repeat(32)) };
                        let _ = ws.send(Message::text(first));
                        let got = ws.read().map(|m| m.to_string()).unwrap_or_else(|e| format!("read error {e}"));
                        format!("{who}: open, then {}", &got[..got.len().min(60)])
                    }
                    Err(tungstenite::Error::Http(resp)) => format!("{who}: http {}", resp.status()),
                    Err(e) => format!("{who}: error {e}"),
                });
            }
            println!("attempt {i} ({}): {}", made.code, ok.join(" | "));
        }
    }

    #[test]
    fn socket_urls_follow_the_service() {
        assert_eq!(ws_url("https://coop.nv-forever.com", "K7QM2X"), "wss://coop.nv-forever.com/v1/lobbies/K7QM2X/ws");
        assert_eq!(ws_url("http://127.0.0.1:8787", "K7QM2X"), "ws://127.0.0.1:8787/v1/lobbies/K7QM2X/ws");
    }

    #[test]
    fn the_service_can_be_named_or_turned_off() {
        // One test, so the variable is not raced by another.
        std::env::set_var(SERVICE_ENV, "off");
        assert_eq!(service(), None);
        std::env::set_var(SERVICE_ENV, "http://127.0.0.1:8787/");
        assert_eq!(service().as_deref(), Some("http://127.0.0.1:8787"));
        std::env::remove_var(SERVICE_ENV);
        assert_eq!(service().as_deref(), Some(nvf_signal::DEFAULT_SERVICE));
    }
}
