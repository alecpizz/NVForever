//! "Delete them now" for the reports this PC sent (Settings, Gameplay, Reports), as the
//! game's own row does it (engine/nvforever/src/reports/delete.rs, player-reports.md §6):
//! one `DELETE <endpoint>/v1/install/<id>` with the install's `X-NVF-Delete-Token` for the
//! current install ID and each earlier one kept, each forgotten once the server took it; then
//! a new ID when reports are on, none when off, and the summaries not sent yet under a
//! deleted ID go. Nothing else is ever sent from the launcher. The tests read the game's file
//! so the two stay the same.

use fnv_install::reports::Reports;
use fnv_install::EngineHome;
use std::path::Path;
use std::time::Duration;

/// The project's server, as the game's `upload::DEFAULT_ENDPOINT`.
pub const DEFAULT_ENDPOINT: &str = "https://reports-api.nv-forever.com";

/// Where deletions go: `NVF_REPORTS_ENDPOINT`, `[Reports] Endpoint`, else the project's.
pub fn endpoint(r: &Reports) -> String {
    std::env::var("NVF_REPORTS_ENDPOINT")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| r.endpoint.clone())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// The installs to delete: the current one (if any) and the past ones.
pub fn installs(r: &Reports) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = r.install_id.clone().zip(r.delete_token.clone()).into_iter().collect();
    v.extend(r.past.iter().cloned());
    v
}

/// Whether this PC has reports on the server to delete.
pub fn any_sent(home: &EngineHome) -> bool {
    !installs(&Reports::load(home)).is_empty()
}

/// The server took a deletion: 2xx, or 404/410 (nothing (left) under that ID).
fn done(status: Option<u16>) -> bool {
    matches!(status, Some(200..=299 | 404 | 410))
}

fn delete_request(url: &str, token: &str) -> Option<u16> {
    let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .user_agent(concat!("NVForever-Launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    agent.delete(url).header("X-NVF-Build", env!("CARGO_PKG_VERSION")).header("X-NVF-Delete-Token", token).call().ok().map(|r| r.status().as_u16())
}

/// What "Delete them" did: how many install IDs the server was asked about, and how many of
/// this PC's reports not sent yet could not be removed (#4591).
#[derive(Debug, PartialEq, Eq)]
pub struct Deleted {
    pub ids: usize,
    pub left: usize,
}

/// Ask the server to delete every report this PC sent, and remove the ones not sent yet. On a
/// failure the IDs the server did take are forgotten already, so trying again asks only for
/// the rest.
pub fn delete_all(home: &EngineHome) -> Result<Deleted, String> {
    let r = Reports::load(home);
    let url = endpoint(&r);
    let all = installs(&r);
    if all.is_empty() {
        return Ok(Deleted { ids: 0, left: 0 });
    }
    let mut failed = 0;
    for (id, token) in &all {
        if done(delete_request(&format!("{url}/v1/install/{id}"), token)) {
            let mut r = Reports::load(home);
            r.past.retain(|(i, _)| i != id);
            r.save(home).map_err(|e| format!("deleted on the server, but could not be noted here: {e}"))?;
        } else {
            failed += 1;
        }
    }
    if failed > 0 {
        return Err(format!("the server could not be reached or refused for {failed} of {} install ID(s); try again later", all.len()));
    }
    let mut r = Reports::load(home);
    r.past.clear();
    if r.send == Some(true) {
        r.install_id = Some(uuid::Uuid::new_v4().to_string());
        r.delete_token = Some(format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple()));
    } else {
        r.install_id = None;
        r.delete_token = None;
    }
    let ids: Vec<&str> = all.iter().map(|(i, _)| i.as_str()).collect();
    let left = drop_pending(&home.reports().join("pending"), &ids);
    r.save(home).map_err(|e| format!("deleted on the server, but the new ID could not be stored: {e}"))?;
    Ok(Deleted { ids: all.len(), left })
}

/// Removes the summaries not sent yet that were written under one of `ids`; how many of those
/// could not be removed (a file another program holds open, one in a read-only folder), which
/// the player is told about. A summary that cannot be read is counted too: its ID is unknown,
/// so it may be one of `ids`, and it is still on disk.
fn drop_pending(pending: &Path, ids: &[&str]) -> usize {
    let Ok(dir) = std::fs::read_dir(pending) else { return 0 };
    let mut left = 0;
    for e in dir.flatten() {
        let path = e.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if !name.starts_with("session-") || !name.ends_with(".json") {
            continue;
        }
        let Some(summary) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else {
            // Unreadable or not JSON (#4591): it is not removed, so the player hears of it.
            if path.exists() {
                left += 1;
            }
            continue;
        };
        let id = summary.get("install_id").and_then(|i| i.as_str());
        if id.is_some_and(|id| ids.contains(&id)) && std::fs::remove_file(&path).is_err() && path.exists() {
            left += 1;
        }
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    #[test]
    fn the_same_requests_as_the_game() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../engine/nvforever/src/reports/delete.rs")).unwrap();
        assert!(src.contains("format!(\"{url}/v1/install/{id}\")"), "the game's delete path changed");
        assert!(src.contains("Some(200..=299 | 404 | 410)"), "the game's done statuses changed");
        let up = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../engine/nvforever/src/reports/upload.rs")).unwrap();
        assert!(up.contains(&format!("pub const DEFAULT_ENDPOINT: &str = \"{DEFAULT_ENDPOINT}\";")), "the game's endpoint changed");
        assert!(up.contains(".header(\"X-NVF-Delete-Token\", delete_token)"), "the game's token header changed");
        let m = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../engine/nvforever/src/reports/mod.rs")).unwrap();
        assert!(m.contains("format!(\"{}{}\", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())"), "the game's token form changed");
    }

    /// The (path, token) of each request a test server took.
    type Seen = Arc<Mutex<Vec<(String, String)>>>;

    /// A local server answering each DELETE with `status`, noting (path, token).
    fn serve(status: u16) -> (String, Seen) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}", l.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        std::thread::spawn(move || {
            for mut c in l.incoming().flatten() {
                let mut req = Vec::new();
                let mut b = [0u8; 1024];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match c.read(&mut b) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => req.extend_from_slice(&b[..n]),
                    }
                }
                let text = String::from_utf8_lossy(&req).to_string();
                let path = text.lines().next().unwrap_or_default().split(' ').nth(1).unwrap_or_default().to_string();
                let token = text.lines().find_map(|l| l.to_ascii_lowercase().starts_with("x-nvf-delete-token:").then(|| l[19..].trim().to_string())).unwrap_or_default();
                s.lock().unwrap().push((path, token));
                let _ = write!(c, "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        });
        (addr, seen)
    }

    fn home(name: &str, endpoint: &str, send: bool) -> EngineHome {
        let d = std::env::temp_dir().join(format!("nvf-launcher-reports-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let h = EngineHome::new(&d);
        let r = Reports {
            send: Some(send),
            install_id: Some("now".into()),
            delete_token: Some("t-now".into()),
            endpoint: Some(endpoint.into()),
            past: vec![("old".into(), "t-old".into())],
            ..Default::default()
        };
        r.save(&h).unwrap();
        let pending = h.reports().join("pending");
        std::fs::create_dir_all(&pending).unwrap();
        std::fs::write(pending.join("session-1.json"), r#"{"install_id":"old"}"#).unwrap();
        std::fs::write(pending.join("session-2.json"), r#"{"install_id":"other"}"#).unwrap();
        h
    }

    /// #4591: a report not sent yet that cannot be removed is counted, not passed over.
    #[cfg(unix)]
    #[test]
    fn a_pending_report_that_cannot_be_removed_is_counted() {
        use std::os::unix::fs::PermissionsExt;
        let (url, _) = serve(204);
        let h = home("left", &url, false);
        let pending = h.reports().join("pending");
        // A read-only folder: its files cannot be removed (Unix; Windows has no such folder).
        std::fs::set_permissions(&pending, std::fs::Permissions::from_mode(0o555)).unwrap();
        let r = delete_all(&h);
        std::fs::set_permissions(&pending, std::fs::Permissions::from_mode(0o755)).unwrap();
        // Run as root, the folder's mode does not stop the removal: nothing to count then.
        let expected = if pending.join("session-1.json").exists() { 1 } else { 0 };
        assert_eq!(r, Ok(Deleted { ids: 2, left: expected }));
        let _ = std::fs::remove_dir_all(&h.root);
    }

    /// #4591 on Windows: a pending report another program holds open without sharing delete
    /// (the game writing it) is counted. A read-only file is no test there: `remove_file`
    /// deletes read-only files on Windows (its documentation, "Platform-specific behavior").
    #[cfg(windows)]
    #[test]
    fn a_pending_report_held_open_is_counted() {
        use std::os::windows::fs::OpenOptionsExt;
        let (url, _) = serve(204);
        let h = home("held", &url, false);
        let file = h.reports().join("pending").join("session-1.json");
        // FILE_SHARE_READ only: others may read it, not delete it.
        let held = std::fs::OpenOptions::new().read(true).share_mode(0x1).open(&file).unwrap();
        let r = delete_all(&h);
        drop(held);
        assert_eq!(r, Ok(Deleted { ids: 2, left: 1 }));
        assert!(file.exists());
        let _ = std::fs::remove_dir_all(&h.root);
    }

    /// #4591: a pending report that cannot be read (its ID unknown) is counted and kept.
    #[test]
    fn an_unreadable_pending_report_is_counted() {
        let (url, _) = serve(204);
        let h = home("unreadable", &url, false);
        let file = h.reports().join("pending").join("session-3.json");
        std::fs::write(&file, b"{\"install_id\":\"ol").unwrap();
        assert_eq!(delete_all(&h), Ok(Deleted { ids: 2, left: 1 }));
        assert!(file.exists());
        assert!(!h.reports().join("pending").join("session-1.json").exists());
        let _ = std::fs::remove_dir_all(&h.root);
    }

    #[test]
    fn deletes_every_install_and_takes_a_new_id() {
        let (url, seen) = serve(204);
        let h = home("ok", &url, true);
        assert_eq!(delete_all(&h), Ok(Deleted { ids: 2, left: 0 }));
        assert_eq!(*seen.lock().unwrap(), [("/v1/install/now".to_string(), "t-now".to_string()), ("/v1/install/old".to_string(), "t-old".to_string())]);
        let r = Reports::load(&h);
        assert!(r.past.is_empty());
        let id = r.install_id.unwrap();
        assert!(id != "now" && id.len() == 36, "a new ID: {id}");
        assert_eq!(r.delete_token.unwrap().len(), 64);
        let pending = h.reports().join("pending");
        assert!(!pending.join("session-1.json").exists(), "an unsent summary of a deleted ID goes");
        assert!(pending.join("session-2.json").exists());
        assert!(!any_sent(&EngineHome::new(h.root.join("none"))));
        let _ = std::fs::remove_dir_all(&h.root);
    }

    #[test]
    fn reports_off_leave_no_id_and_a_refusal_keeps_them() {
        let (url, _) = serve(404);
        let h = home("off", &url, false);
        assert_eq!(delete_all(&h), Ok(Deleted { ids: 2, left: 0 }), "404: nothing left under that ID, which is what was asked");
        let r = Reports::load(&h);
        assert_eq!((r.install_id, r.delete_token), (None, None));
        let _ = std::fs::remove_dir_all(&h.root);

        let (url, _) = serve(500);
        let h = home("refused", &url, true);
        assert!(delete_all(&h).is_err());
        let r = Reports::load(&h);
        assert_eq!(r.install_id.as_deref(), Some("now"), "kept, to try again");
        assert_eq!(r.past.len(), 1);
        let _ = std::fs::remove_dir_all(&h.root);
    }
}
