//! Sending reports (player-reports.md §8): at a start, after the main menu shows, a thread
//! of its own sends what is waiting in `<home>\reports\pending\`, oldest first, at most
//! [`MAX_PER_START`], and moves what was sent to `reports\sent\` (the last [`KEEP_SENT`]
//! kept for the player to see). Nothing here blocks a frame, a load or the exit: the
//! thread is never joined, every request has a [`TIMEOUT`], nothing is retried in the same
//! run, and being offline is normal and silent (one `info!` line).
//!
//! What the server says decides what happens to a file (the server's wire format,
//! LootJunkieAzza/NVForever-Data `docs/wire-format.md`): accepted (`202`) moves it to
//! `sent`; refused as malformed or too large (`400`, `413`) or from a deleted install or a
//! turned-away build (`403`, `410`) deletes it (no retry loop, §5.1); a rate limit or a
//! server error (`429`, `5xx`) or no answer keeps it for the next start; the kill switch
//! (`503`, or `kill` in `/v1/config`) stops sending for this run.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Files sent at one start, at most (§8).
pub const MAX_PER_START: usize = 5;
/// Sent files kept in `reports\sent\` for the player to look at (§8).
pub const KEEP_SENT: usize = 20;
/// Pending files older than this are dropped unsent (§8).
pub const MAX_AGE: Duration = Duration::from_secs(14 * 86_400);
/// How long after the start sending begins.
pub const SEND_AFTER: Duration = Duration::from_secs(6);
/// The largest minidump sent (the server's cap, §5.1).
pub const MAX_DUMP_BYTES: usize = 4 * 1024 * 1024;
/// One request's limit (§8).
#[cfg_attr(target_os = "android", allow(dead_code))]
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// The project's server, used when `[Reports] Endpoint` is empty.
pub const DEFAULT_ENDPOINT: &str = "https://reports-api.nv-forever.com";

/// What to do with a pending file after one attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Accepted: move it to `sent`.
    Sent,
    /// The server will never take it: delete it.
    Drop,
    /// Try again at a later start.
    Keep,
    /// Reports are switched off on the server: keep it and send nothing more this run.
    Stop,
}

/// The outcome of an HTTP status (module notes). `None`: no answer (offline, timeout).
pub fn outcome(status: Option<u16>) -> Outcome {
    match status {
        Some(200..=299) => Outcome::Sent,
        Some(400 | 403 | 410 | 413) => Outcome::Drop,
        Some(503) => Outcome::Stop,
        Some(_) | None => Outcome::Keep,
    }
}

/// A pending upload: a session summary (`session-<time>-<pid>.json`) and the parts that go
/// with it, which share its stem (`.log.gz` the run log, `.panic.txt` a crash report).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub summary: PathBuf,
    pub log: Option<PathBuf>,
    pub panic: Option<PathBuf>,
    /// The flight recorder a panic wrote (anomaly.rs), gzip JSONL.
    pub recorder: Option<PathBuf>,
    /// The minidump a native crash left (native_crash.rs).
    pub dump: Option<PathBuf>,
    /// The world snapshot a panic wrote (snapshot.rs).
    pub snapshot: Option<PathBuf>,
}

impl Pending {
    /// Every file of this upload.
    pub fn files(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.summary.as_path()).chain(self.log.as_deref()).chain(self.panic.as_deref()).chain(self.recorder.as_deref()).chain(self.dump.as_deref()).chain(self.snapshot.as_deref())
    }
}

/// The pending uploads in `names` (the folder's file names), oldest first: summaries sort
/// by the time stamp in their names.
pub fn pending_in(dir: &Path, names: &[String]) -> Vec<Pending> {
    let mut summaries: Vec<&String> = names.iter().filter(|n| n.starts_with("session-") && n.ends_with(".json")).collect();
    summaries.sort();
    summaries
        .into_iter()
        .map(|n| {
            let stem = &n[..n.len() - ".json".len()];
            let part = |ext: &str| {
                let name = format!("{stem}{ext}");
                names.contains(&name).then(|| dir.join(name))
            };
            Pending { summary: dir.join(n), log: part(".log.gz"), panic: part(".panic.txt"), recorder: part(".recorder.jsonl.gz"), dump: part(".dmp"), snapshot: part(".snapshot.json") }
        })
        .collect()
}

/// The `end` field of a summary's JSON, if it can be read.
pub fn end_of(summary: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(summary).ok()?;
    v.get("end")?.as_str().map(str::to_string)
}

/// Whether a session that ended this way goes to `/v1/crash` rather than `/v1/session`.
pub fn is_crash(end: &str) -> bool {
    matches!(end, "panic" | "native")
}

/// A `multipart/form-data` body: `(field name, file name, content type, bytes)` parts; a
/// part with no file name is a text field (the hand report's description, form.rs).
/// Returns the body and its content type (with the boundary).
pub fn multipart(parts: &[(&str, &str, &str, &[u8])]) -> (Vec<u8>, String) {
    // A boundary that cannot be in the parts: long, and checked.
    let mut boundary = String::from("nvf-reports-boundary-7d1c4f0e9a2b");
    while parts.iter().any(|(_, _, _, b)| b.windows(boundary.len()).any(|w| w == boundary.as_bytes())) {
        boundary.push('x');
    }
    let mut body = Vec::new();
    for (name, file, ctype, bytes) in parts {
        if file.is_empty() {
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes());
        } else {
            body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{file}\"\r\nContent-Type: {ctype}\r\n\r\n").as_bytes());
        }
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (body, format!("multipart/form-data; boundary={boundary}"))
}

/// gzip of `bytes` (summaries are sent compressed, §5.1).
pub fn gzip(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = e.write_all(bytes);
    e.finish().unwrap_or_default()
}

/// Files in `names` (in `dir`) to delete so the last `keep` sent summaries (and their
/// parts) remain.
pub fn old_sent(dir: &Path, names: &[String], keep: usize) -> Vec<PathBuf> {
    let all = pending_in(dir, names);
    let n = all.len().saturating_sub(keep);
    all.into_iter().take(n).flat_map(|p| p.files().map(Path::to_path_buf).collect::<Vec<_>>()).collect()
}

/// The server's `/v1/config` answer, the parts sending needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServerConfig {
    pub kill: bool,
    pub sampling: f64,
}

pub fn parse_config(body: &[u8]) -> Option<ServerConfig> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    Some(ServerConfig { kill: v["kill"].as_bool().unwrap_or(false), sampling: v["sampling"].as_f64().unwrap_or(1.0).clamp(0.0, 1.0) })
}

/// Whether a clean session is sent under `sampling` (0..1): decided by its session id, so
/// the same session gets the same answer at every start.
pub fn sampled_in(summary: &[u8], sampling: f64) -> bool {
    if sampling >= 1.0 {
        return true;
    }
    let id = serde_json::from_slice::<serde_json::Value>(summary).ok().and_then(|v| v["session_id"].as_str().map(str::to_string)).unwrap_or_default();
    let h = id.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    f64::from(h % 10_000) / 10_000.0 < sampling
}

/// Start sending earlier sessions on a thread of its own (module notes). `pid`: this run's
/// process, whose own summary is left alone.
pub fn spawn(active: super::Active, pending: PathBuf, sent: PathBuf, pid: u32) {
    let _ = std::thread::Builder::new().name("nvf-reports".into()).spawn(move || {
        // After the start's heavy work, so sending never competes with it; short, as a run
        // that only shows the main menu before a load is handed over lives a few seconds.
        std::thread::sleep(SEND_AFTER);
        send_all(&active, &pending, &sent, pid);
    });
}

/// The process id in a summary's name (`session-<time>-<pid>.json`).
pub fn pid_of(summary: &Path) -> Option<u32> {
    summary.file_name()?.to_str()?.strip_suffix(".json")?.rsplit('-').next()?.parse().ok()
}

/// Whether a pending upload is sent at this start (run `own`): not this run's own summary,
/// and not one of a run still going (`running`). After a load handoff the old run waits for
/// the new one to be ready, often longer than [`SEND_AFTER`]; its summary still says
/// `unknown` then, and sending it would report that run as a native crash (#2260). It is
/// sent at a later start, once it has ended.
pub fn sendable(p: &Pending, own: u32, running: impl Fn(u32) -> bool) -> bool {
    pid_of(&p.summary).is_none_or(|pid| pid != own && !running(pid))
}

/// Whether process `pid` is an NVForever run still going (its image name starts with
/// `nvforever`, so a dead run's reused id holds nothing back for long).
#[cfg(windows)]
pub fn still_running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: the handle is checked before use and closed once; the buffers outlive the calls.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let alive = GetExitCodeProcess(h, &mut code) != 0 && code == STILL_ACTIVE as u32;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let named = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0;
        CloseHandle(h);
        let name = String::from_utf16_lossy(&buf[..len as usize]);
        alive && named && is_our_image(Path::new(&name))
    }
}

/// Linux: `/proc/<pid>/exe` is a link to the image of a process that is alive
/// [documented: proc(5), `/proc/pid/exe`]; it cannot be read for one that has ended.
/// Needed once reports are sent from Linux (#3144), or a load handoff's old run would be
/// reported as a native crash there (#2260).
#[cfg(target_os = "linux")]
pub fn still_running(pid: u32) -> bool {
    std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|exe| is_our_image(&exe))
}

/// Elsewhere there is no check: every earlier session counts as ended.
#[cfg(not(any(windows, target_os = "linux")))]
pub fn still_running(_pid: u32) -> bool {
    false
}

/// Whether a process image is an NVForever game (`nvforever`, `nvforever.exe`, a test
/// binary `nvforever-<hash>`).
#[cfg(any(windows, target_os = "linux", test))]
fn is_our_image(image: &Path) -> bool {
    image.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_ascii_lowercase().starts_with("nvforever"))
}

fn names_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir).map(|r| r.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()).unwrap_or_default()
}

fn send_all(active: &super::Active, pending: &Path, sent: &Path, pid: u32) {
    let logs = fnv_install::EngineHome::user().map(|h| h.logs());
    let mut uploads: Vec<Pending> = pending_in(pending, &names_in(pending)).into_iter().filter(|p| sendable(p, pid, still_running)).collect();
    // Too old to send: dropped (§8).
    uploads.retain(|p| {
        let old = std::fs::metadata(&p.summary).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > MAX_AGE);
        if old {
            p.files().for_each(|f| _ = std::fs::remove_file(f));
        }
        !old
    });
    if uploads.is_empty() {
        return;
    }
    let config = http::get(&format!("{}/v1/config", active.endpoint)).and_then(|(s, b)| (s == 200).then(|| parse_config(&b)).flatten());
    let Some(config) = config else {
        bevy::log::info!("reports: the server could not be reached; {} report(s) wait for a later start", uploads.len());
        return;
    };
    if config.kill {
        bevy::log::info!("reports: the server is not taking reports now; {} wait", uploads.len());
        return;
    }
    let mut sent_n = 0;
    for p in uploads.into_iter().take(MAX_PER_START) {
        let p = match &logs {
            Some(l) => super::finish_earlier(&p.summary, l).unwrap_or(p),
            None => p,
        };
        let Ok(summary) = std::fs::read(&p.summary) else { continue };
        let end = end_of(&summary).unwrap_or_default();
        if !is_crash(&end) && !sampled_in(&summary, config.sampling) {
            p.files().for_each(|f| _ = std::fs::remove_file(f));
            continue;
        }
        let log = p.log.as_ref().and_then(|l| std::fs::read(l).ok());
        let panic = p.panic.as_ref().and_then(|l| std::fs::read(l).ok());
        let recorder = p.recorder.as_ref().filter(|_| is_crash(&end)).and_then(|l| std::fs::read(l).ok());
        // The server takes dumps up to 4 MiB (§5.1); a larger one stays on the player's PC.
        let dump = p.dump.as_ref().filter(|_| end == "native").and_then(|d| std::fs::read(d).ok()).filter(|d| d.len() <= MAX_DUMP_BYTES);
        let gz = gzip(&summary);
        let mut parts: Vec<(&str, &str, &str, &[u8])> = vec![("summary", "summary.json.gz", "application/gzip", &gz)];
        if let Some(l) = &log {
            parts.push(("log", "log.txt.gz", "application/gzip", l));
        }
        if let Some(t) = &panic {
            parts.push(("panic", "panic.txt", "text/plain", t));
        }
        if let Some(r) = &recorder {
            parts.push(("recorder", "recorder.jsonl.gz", "application/gzip", r));
        }
        if let Some(d) = &dump {
            parts.push(("dump", "crash.dmp", "application/octet-stream", d));
        }
        let snapshot = p.snapshot.as_ref().filter(|_| end == "panic").and_then(|s| std::fs::read(s).ok());
        if let Some(s) = &snapshot {
            parts.push(("snapshot", "snapshot.json", "application/json", s));
        }
        let (body, ctype) = multipart(&parts);
        let path = if is_crash(&end) { "/v1/crash" } else { "/v1/session" };
        let (_, token) = super::identity(active);
        let status = http::post(&format!("{}{path}", active.endpoint), &body, &ctype, &token);
        let name = p.summary.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match outcome(status) {
            Outcome::Sent => {
                for f in p.files() {
                    if let Some(n) = f.file_name() {
                        let _ = std::fs::rename(f, sent.join(n));
                    }
                }
                sent_n += 1;
                bevy::log::info!("reports: sent {name} ({end})");
            }
            Outcome::Drop => {
                p.files().for_each(|f| _ = std::fs::remove_file(f));
                bevy::log::warn!("reports: the server refused {name} ({}); it is deleted", status.unwrap_or(0));
            }
            Outcome::Keep => {
                bevy::log::info!("reports: {name} not sent ({}); kept for a later start", status.map_or("no answer".to_string(), |s| s.to_string()));
                break;
            }
            Outcome::Stop => {
                bevy::log::info!("reports: the server is not taking reports now");
                break;
            }
        }
    }
    for f in old_sent(sent, &names_in(sent), KEEP_SENT) {
        let _ = std::fs::remove_file(f);
    }
    if sent_n > 0 {
        bevy::log::info!("reports: {sent_n} report(s) sent");
    }
}

/// HTTP through the system's TLS and certificates (Windows: SChannel; elsewhere the
/// system's OpenSSL), on every desktop platform the game is built for (#3144: off Windows
/// this was four stubs that never answered, so Report a bug, Send reports and Delete my
/// reports told a Linux player their network was at fault). Android suppresses player
/// reports (`NVF_REPORTS=0`) and does not link system OpenSSL.
#[cfg(not(target_os = "android"))]
pub(crate) mod http {
    use super::TIMEOUT;

    fn agent() -> ureq::Agent {
        let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
        ureq::Agent::config_builder().tls_config(tls).timeout_global(Some(TIMEOUT)).http_status_as_error(false).user_agent(concat!("NVForever/", env!("CARGO_PKG_VERSION"))).build().into()
    }

    /// (status, body), or `None` without an answer.
    pub fn get(url: &str) -> Option<(u16, Vec<u8>)> {
        let mut r = agent().get(url).header("X-NVF-Build", env!("CARGO_PKG_VERSION")).call().ok()?;
        let status = r.status().as_u16();
        Some((status, r.body_mut().with_config().limit(64 * 1024).read_to_vec().unwrap_or_default()))
    }

    /// `DELETE` with the install's token (§5.1); the status, or `None` without an answer.
    pub fn delete(url: &str, delete_token: &str) -> Option<u16> {
        agent().delete(url).header("X-NVF-Build", env!("CARGO_PKG_VERSION")).header("X-NVF-Delete-Token", delete_token).call().ok().map(|r| r.status().as_u16())
    }

    /// `POST` returning the answer's body too (a hand report's reference, form.rs); the
    /// delete token only with an install ID.
    pub fn post_for(url: &str, body: &[u8], content_type: &str, delete_token: Option<&str>) -> Option<(u16, Vec<u8>)> {
        let mut req = agent().post(url).header("X-NVF-Build", env!("CARGO_PKG_VERSION"));
        if let Some(t) = delete_token {
            req = req.header("X-NVF-Delete-Token", t);
        }
        let mut r = req.content_type(content_type).send(body).ok()?;
        let status = r.status().as_u16();
        Some((status, r.body_mut().with_config().limit(64 * 1024).read_to_vec().unwrap_or_default()))
    }

    pub fn post(url: &str, body: &[u8], content_type: &str, delete_token: &str) -> Option<u16> {
        agent()
            .post(url)
            .header("X-NVF-Build", env!("CARGO_PKG_VERSION"))
            .header("X-NVF-Delete-Token", delete_token)
            .content_type(content_type)
            .send(body)
            .ok()
            .map(|r| r.status().as_u16())
    }
}

/// Android suppresses player reports (`NVF_REPORTS=0`) and does not link system OpenSSL.
#[cfg(target_os = "android")]
pub(crate) mod http {
    pub fn get(_url: &str) -> Option<(u16, Vec<u8>)> {
        None
    }

    pub fn delete(_url: &str, _delete_token: &str) -> Option<u16> {
        None
    }

    pub fn post_for(_url: &str, _body: &[u8], _content_type: &str, _delete_token: Option<&str>) -> Option<(u16, Vec<u8>)> {
        None
    }

    pub fn post(_url: &str, _body: &[u8], _content_type: &str, _delete_token: &str) -> Option<u16> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #2260: the session of a run still going (the old run of a load handoff) waits.
    #[test]
    fn sessions_of_runs_still_going_wait() {
        let p = |name: &str| Pending { summary: PathBuf::from(format!("pending/{name}")), log: None, panic: None, recorder: None, dump: None, snapshot: None };
        assert_eq!(pid_of(Path::new("pending/session-20261112-172311-600.json")), Some(600));
        let running = |pid: u32| pid == 600;
        assert!(!sendable(&p("session-20261112-172311-600.json"), 700, running), "the old run, still waiting");
        assert!(!sendable(&p("session-20261112-173713-700.json"), 700, running), "this run's own");
        assert!(sendable(&p("session-20261112-160000-500.json"), 700, running), "an ended run");
        assert!(sendable(&p("session-odd.json"), 700, running), "no pid in the name");
    }

    /// The test binary (`nvforever-<hash>.exe`) is an NVForever run still going.
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn this_process_is_still_running() {
        assert!(still_running(std::process::id()));
        assert!(!still_running(u32::MAX - 3));
    }

    #[test]
    fn a_process_image_is_ours_by_its_name() {
        assert!(is_our_image(Path::new("/opt/nvf/nvforever")));
        assert!(is_our_image(Path::new("C:/Games/NVForever/NVForever.exe")));
        assert!(is_our_image(Path::new("target/release/deps/nvforever-0a1b2c3d")));
        assert!(!is_our_image(Path::new("/usr/bin/bash")));
    }

    /// A server on this machine that answers each of `n` requests with `status` and
    /// `body`, and gives back what each request sent.
    fn local_server(n: usize, status: u16, body: &'static str) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let served = std::thread::spawn(move || {
            (0..n)
                .map(|_| {
                    let (mut s, _) = listener.accept().unwrap();
                    let mut got = Vec::new();
                    let mut buf = [0u8; 4096];
                    // The head, then the body its Content-Length names.
                    let (head, length) = loop {
                        let k = s.read(&mut buf).unwrap();
                        got.extend_from_slice(&buf[..k]);
                        if let Some(end) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&got[..end]).to_ascii_lowercase();
                            let length = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                            break (end + 4, length);
                        }
                        assert!(k > 0, "the request ended before its head");
                    };
                    while got.len() < head + length {
                        let k = s.read(&mut buf).unwrap();
                        assert!(k > 0, "the request ended before its body");
                        got.extend_from_slice(&buf[..k]);
                    }
                    write!(s, "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                    String::from_utf8_lossy(&got).into_owned()
                })
                .collect()
        });
        (url, served)
    }

    /// #3144: the four requests get their answer on every platform the game is built for
    /// (off Windows they were stubs that never answered). Plain HTTP to this machine: the
    /// TLS side needs a server with a certificate and is not covered here.
    #[test]
    fn requests_get_an_answer_on_this_platform() {
        let (url, served) = local_server(4, 202, r#"{"kill":false}"#);
        assert_eq!(http::get(&format!("{url}/v1/config")), Some((202, br#"{"kill":false}"#.to_vec())));
        assert_eq!(http::post(&format!("{url}/v1/session"), b"the body", "text/plain", "tok-1"), Some(202));
        assert_eq!(http::post_for(&format!("{url}/v1/report"), b"by hand", "text/plain", None).map(|(s, _)| s), Some(202));
        assert_eq!(http::delete(&format!("{url}/v1/install/abc"), "tok-2"), Some(202));
        let got: Vec<String> = served.join().unwrap().into_iter().map(|r| r.to_ascii_lowercase()).collect();
        assert!(got[0].starts_with("get /v1/config ") && got[0].contains("x-nvf-build:"), "{}", got[0]);
        assert!(got[1].starts_with("post /v1/session ") && got[1].contains("x-nvf-delete-token: tok-1") && got[1].ends_with("the body"), "{}", got[1]);
        assert!(got[2].starts_with("post /v1/report ") && !got[2].contains("x-nvf-delete-token") && got[2].ends_with("by hand"), "{}", got[2]);
        assert!(got[3].starts_with("delete /v1/install/abc ") && got[3].contains("x-nvf-delete-token: tok-2"), "{}", got[3]);
        // No server: no answer, which the callers report as "kept for a later start".
        let closed = { std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap() };
        assert_eq!(http::get(&format!("http://{closed}/v1/config")), None);
    }

    #[test]
    fn the_server_config_and_sampling() {
        assert_eq!(parse_config(br#"{"kill":true,"sampling":0.5}"#), Some(ServerConfig { kill: true, sampling: 0.5 }));
        assert_eq!(parse_config(br#"{}"#), Some(ServerConfig { kill: false, sampling: 1.0 }));
        let s = br#"{"session_id":"9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d"}"#;
        assert!(sampled_in(s, 1.0));
        assert!(!sampled_in(s, 0.0));
        assert_eq!(sampled_in(s, 0.5), sampled_in(s, 0.5), "the same answer every time");
    }

    #[test]
    fn the_server_answer_decides_the_file() {
        assert_eq!(outcome(Some(202)), Outcome::Sent);
        for s in [400, 403, 410, 413] {
            assert_eq!(outcome(Some(s)), Outcome::Drop, "{s}");
        }
        assert_eq!(outcome(Some(503)), Outcome::Stop);
        for s in [Some(429), Some(500), Some(502), None] {
            assert_eq!(outcome(s), Outcome::Keep, "{s:?}");
        }
    }

    #[test]
    fn pending_uploads_are_oldest_first_with_their_parts() {
        let names: Vec<String> = [
            "session-20261110-120000-7.json",
            "session-20261109-080000-3.json",
            "session-20261109-080000-3.log.gz",
            "session-20261109-080000-3.panic.txt",
            "session-20261110-120000-7.log.gz",
            "notes.txt",
        ]
        .map(String::from)
        .to_vec();
        let dir = Path::new("p");
        let p = pending_in(dir, &names);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].summary, dir.join("session-20261109-080000-3.json"));
        assert_eq!(p[0].panic, Some(dir.join("session-20261109-080000-3.panic.txt")));
        assert_eq!(p[1].panic, None);
        assert_eq!(p[1].log, Some(dir.join("session-20261110-120000-7.log.gz")));
        assert_eq!(old_sent(dir, &names, 1).len(), 3, "the older upload's three files");
    }

    #[test]
    fn crashes_go_to_their_own_endpoint() {
        assert_eq!(end_of(br#"{"end":"native","x":1}"#).as_deref(), Some("native"));
        assert!(is_crash("panic") && is_crash("native") && !is_crash("clean") && !is_crash("unknown"));
        assert_eq!(end_of(b"not json"), None);
    }

    #[test]
    fn a_part_without_a_file_name_is_a_text_field() {
        let (body, _) = multipart(&[("description", "", "", b"the door")]);
        let text = String::from_utf8(body).unwrap();
        assert!(text.contains("name=\"description\"\r\n\r\nthe door\r\n"), "{text}");
        assert!(!text.contains("filename"));
    }

    #[test]
    fn a_multipart_body_has_each_part_and_a_safe_boundary() {
        let (body, ctype) = multipart(&[("summary", "s.json.gz", "application/gzip", b"abc"), ("log", "l.gz", "application/gzip", b"nvf-reports-boundary-7d1c4f0e9a2b")]);
        let boundary = ctype.strip_prefix("multipart/form-data; boundary=").unwrap();
        assert_ne!(boundary, "nvf-reports-boundary-7d1c4f0e9a2b", "a part held the first boundary");
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("name=\"summary\"") && text.contains("name=\"log\"") && text.ends_with(&format!("--{boundary}--\r\n")));
    }

    #[test]
    fn gzip_round_trips() {
        use std::io::Read;
        let mut out = String::new();
        flate2::read::GzDecoder::new(&gzip(b"hello")[..]).read_to_string(&mut out).unwrap();
        assert_eq!(out, "hello");
    }
}
