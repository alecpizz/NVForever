//! The Home page's news posts. Read from the site's `news.json` (a list of posts written
//! for the launcher) and, when the site has none, from this repository's GitHub releases
//! (each release's title, date and notes). The last list fetched is kept in the home's
//! cache folder, so the page shows it at once and offline. Nothing is sent but the
//! requests themselves.
//!
//! `news.json`, as the site serves it:
//!
//! ```json
//! { "posts": [ { "title": "NVForever 0.7.0", "date": "2026-11-11", "author": "LootJunkieAzza",
//!                "image": "https://nv-forever.com/assets/img/news/070.jpg",
//!                "body": "Markdown…", "url": "https://…" } ] }
//! ```

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[cfg(feature = "desktop")]
use std::time::Duration;

pub const SITE_NEWS: &str = "https://nv-forever.com/news.json";
pub const RELEASES: &str = "https://api.github.com/repos/LootJunkieAzza/NVForever/releases?per_page=10";
pub const SITE: &str = "https://nv-forever.com";
const CACHE_FILE: &str = "launcher-news.json";
/// The longest a news image may be; a bigger one is left out.
#[cfg(feature = "desktop")]
const MAX_IMAGE: u64 = 8 << 20;
/// The most posts kept from a fetch (and so the most images fetched at once).
const MAX_POSTS: usize = 20;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Post {
    pub title: String,
    /// `YYYY-MM-DD` (or a full ISO time; only the date is shown).
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub url: Option<String>,
    /// The image's bytes, once fetched; never cached.
    #[serde(skip)]
    pub image_bytes: Option<Vec<u8>>,
}

impl Post {
    /// The date as the page shows it: `13 November 2026`, or the text as given when it is
    /// not a date.
    pub fn shown_date(&self) -> String {
        let d = self.date.get(..10).unwrap_or(&self.date);
        let mut it = d.split('-').map(|x| x.parse::<u32>().ok());
        match (it.next().flatten(), it.next().flatten(), it.next().flatten()) {
            (Some(y), Some(m @ 1..=12), Some(day)) => {
                const MONTHS: [&str; 12] =
                    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
                format!("{day} {} {y}", MONTHS[m as usize - 1])
            }
            _ => self.date.clone(),
        }
    }
}

#[derive(Deserialize)]
struct SiteNews {
    posts: Vec<Post>,
}

#[derive(Deserialize)]
struct Release {
    #[serde(default)]
    name: Option<String>,
    tag_name: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    author: Option<Author>,
}

#[derive(Deserialize)]
struct Author {
    login: String,
}

/// The site's posts in `text`.
pub fn parse_site(text: &str) -> Result<Vec<Post>, String> {
    serde_json::from_str::<SiteNews>(text).map(|n| n.posts).map_err(|e| format!("news.json: {e}"))
}

/// GitHub's release list in `text` as posts, drafts left out.
pub fn parse_releases(text: &str) -> Result<Vec<Post>, String> {
    let v: Vec<Release> = serde_json::from_str(text).map_err(|e| format!("releases: {e}"))?;
    Ok(v.into_iter()
        .filter(|r| !r.draft)
        .map(|r| Post {
            title: r.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| r.tag_name.clone()),
            date: r.published_at.unwrap_or_default(),
            author: r.author.map(|a| a.login).unwrap_or_default(),
            image: None,
            body: r.body.unwrap_or_default().replace("\r\n", "\n"),
            url: r.html_url,
            image_bytes: None,
        })
        .collect())
}

#[cfg(feature = "desktop")]
fn tls() -> ureq::tls::TlsConfig {
    ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).root_certs(ureq::tls::RootCerts::PlatformVerifier).build()
}

/// The agent of the small requests: 15 s for the whole of each.
#[cfg(feature = "desktop")]
pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .tls_config(tls())
        .timeout_global(Some(Duration::from_secs(15)))
        // A release's launcher fetches over HTTPS only; plain http to a local test server
        // only with the test aids (crate::update::test_aids).
        .https_only(!crate::update::test_aids())
        .user_agent(concat!("NVForever-Launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// The agent of a release download (100 MB and more): no limit on the whole, but 15 s to
/// connect and 30 s for any one read, so a dead connection ends in an error.
#[cfg(feature = "desktop")]
pub(crate) fn download_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .tls_config(tls())
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .https_only(!crate::update::test_aids())
        .user_agent(concat!("NVForever-Launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

#[cfg(feature = "desktop")]
pub(crate) fn get_text(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    let mut r = agent.get(url).header("Accept", "application/json").call().map_err(|e| format!("{url}: {e}"))?;
    r.body_mut().read_to_string().map_err(|e| format!("{url}: {e}"))
}

/// The release list, fetched at most once a minute: the news (when the site has none) and
/// the update check both read it, and GitHub allows 60 requests an hour without a sign-in.
#[cfg(feature = "desktop")]
fn release_list(agent: &ureq::Agent) -> Result<String, String> {
    use std::sync::Mutex;
    use std::time::Instant;
    static LAST: Mutex<Option<(Instant, String)>> = Mutex::new(None);
    // Held across the request, so a second caller waits for the first one's answer.
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, text)) = last.as_ref() {
        if at.elapsed() < Duration::from_secs(60) {
            return Ok(text.clone());
        }
    }
    // Test aid: a release list of a local server (docs/design/launcher.md), for trying the
    // updater against a fixture release.
    let url = std::env::var("NVF_LAUNCHER_RELEASES").ok().filter(|u| !u.is_empty() && crate::update::test_aids());
    let text = get_text(agent, url.as_deref().unwrap_or(RELEASES))?;
    *last = Some((Instant::now(), text.clone()));
    Ok(text)
}

#[cfg(feature = "desktop")]
fn get_bytes(agent: &ureq::Agent, url: &str) -> Option<Vec<u8>> {
    let mut r = agent.get(url).call().ok()?;
    r.body_mut().with_config().limit(MAX_IMAGE).read_to_vec().ok()
}

/// Where the news came from, for the page's small print.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Site,
    Releases,
    Cache,
}

/// The site's posts, else the releases, each with its image when it has one.
#[cfg(feature = "desktop")]
pub fn fetch() -> Result<(Vec<Post>, Source), String> {
    let agent = agent();
    let (mut posts, source) = select_posts(get_text(&agent, SITE_NEWS), || release_list(&agent))?;
    std::thread::scope(|s| {
        for p in &mut posts {
            if let Some(url) = p.image.clone().filter(|u| u.starts_with("https://")) {
                let agent = &agent;
                s.spawn(move || p.image_bytes = get_bytes(agent, &url));
            }
        }
    });
    Ok((posts, source))
}

/// Choose the news feed independently of the platform's HTTPS client.
pub fn select_posts(site: Result<String, String>, releases: impl FnOnce() -> Result<String, String>) -> Result<(Vec<Post>, Source), String> {
    let (mut posts, source) = match site.and_then(|t| parse_site(&t)) {
        Ok(p) if !p.is_empty() => (p, Source::Site),
        site => {
            let site_err = site.err().unwrap_or_else(|| "news.json: no posts".into());
            let posts = releases().and_then(|t| parse_releases(&t)).map_err(|e| format!("{site_err}; {e}"))?;
            (posts, Source::Releases)
        }
    };
    posts.truncate(MAX_POSTS);
    Ok((posts, source))
}

pub fn cache_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(CACHE_FILE)
}

pub fn load_cache(cache_dir: &Path) -> Vec<Post> {
    std::fs::read_to_string(cache_path(cache_dir)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_cache(cache_dir: &Path, posts: &[Post]) {
    if std::fs::create_dir_all(cache_dir).is_ok() {
        if let Ok(t) = serde_json::to_string(posts) {
            let _ = std::fs::write(cache_path(cache_dir), t);
        }
    }
}

/// The site's settings script, whose `nextRelease` drives its countdown bar.
pub const SITE_CONFIG: &str = "https://nv-forever.com/assets/js/config.js";

/// The site's next release: its version and, when the date is set, the moment (seconds since
/// the Unix epoch).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextRelease {
    pub version: String,
    pub at: Option<i64>,
}

/// The quoted value of `name: "…"` (or `'…'`) in `block`.
fn js_field(block: &str, name: &str) -> Option<String> {
    let mut rest = block;
    loop {
        let i = rest.find(name)?;
        rest = &rest[i + name.len()..];
        if let Some(v) = rest.trim_start().strip_prefix(':') {
            let v = v.trim_start();
            let q = v.chars().next().filter(|c| matches!(c, '"' | '\''))?;
            let v = &v[1..];
            return Some(v[..v.find(q)?].to_string());
        }
    }
}

/// `nextRelease: { version: "0.8.0", date: "2026-11-18T20:00:00Z" }` of the site's
/// `config.js`; `None` when there is no such block or its version is empty (the site hides
/// its bar then too).
pub fn parse_next_release(js: &str) -> Option<NextRelease> {
    let block = &js[js.find("nextRelease")?..];
    let block = &block[..block.find('}')?];
    let version = js_field(block, "version").filter(|v| !v.trim().is_empty())?;
    let at = js_field(block, "date").and_then(|d| parse_iso(&d));
    Some(NextRelease { version, at })
}

/// `YYYY-MM-DDTHH:MM[:SS][Z|±HH:MM]` as seconds since the Unix epoch (no zone: UTC).
pub fn parse_iso(t: &str) -> Option<i64> {
    let t = t.trim();
    let num = |a: usize, b: usize| t.get(a..b).and_then(|x| x.parse::<i64>().ok());
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi) = (num(11, 13).unwrap_or(0), num(14, 16).unwrap_or(0));
    let sec = if t.get(16..17) == Some(":") { num(17, 19).unwrap_or(0) } else { 0 };
    let zone = t.get(16..).unwrap_or("");
    let offset = match zone.rfind(['+', '-']) {
        Some(i) if zone.len() - i == 6 => {
            let z = &zone[i..];
            // `get`, not an index: a byte range that splits a character is no date, not a panic.
            let m = z.get(1..3)?.parse::<i64>().ok()? * 60 + z.get(4..6)?.parse::<i64>().ok()?;
            if z.starts_with('-') {
                -m
            } else {
                m
            }
        }
        _ => 0,
    };
    // Days from the civil date (Howard Hinnant's days_from_civil).
    let yy = if mo <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * ((mo + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + sec - offset * 60)
}

/// The countdown's words for `left` seconds: `2d 14h 03m 12s`.
pub fn countdown_text(left: i64) -> String {
    let l = left.max(0);
    let (d, h, m, s) = (l / 86_400, l % 86_400 / 3600, l % 3600 / 60, l % 60);
    if d > 0 {
        format!("{d}d {h:02}h {m:02}m {s:02}s")
    } else {
        format!("{h:02}h {m:02}m {s:02}s")
    }
}


/// A release's file, as GitHub's release list gives it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

/// A published release on GitHub: its version (`0.8.0`, the tag without its `v`), tag, page,
/// notes and files (the archives and `SHA256SUMS.txt`, for the updater).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Latest {
    pub version: String,
    pub tag: String,
    pub url: String,
    pub notes: String,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct ListedRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

impl From<ListedRelease> for Latest {
    fn from(r: ListedRelease) -> Latest {
        Latest {
            version: r.tag_name.trim_start_matches('v').to_string(),
            tag: r.tag_name,
            url: r.html_url,
            notes: r.body.unwrap_or_default().replace("\r\n", "\n"),
            assets: r.assets,
        }
    }
}

/// The release list's published releases (drafts left out).
pub fn parse_release_list(text: &str) -> Result<Vec<Latest>, String> {
    let v: Vec<ListedRelease> = serde_json::from_str(text).map_err(|e| format!("releases: {e}"))?;
    Ok(v.into_iter().filter(|r| !r.draft).map(Latest::from).collect())
}

/// The release with the highest version (not the list's first: a fix of an older line
/// published later would come first).
pub fn newest(list: Vec<Latest>) -> Option<Latest> {
    list.into_iter().max_by(|a, b| cmp_versions(&a.version, &b.version))
}

/// Versions (`0.8.0`, `v0.10.1`, `0.9.0-rc.1`) compared as semantic versions: the numbers
/// part by part, then a pre-release (`-rc.1`) before the release it leads to, pre-releases
/// by their parts (numbers as numbers); build data (`+…`) does not count.
pub fn cmp_versions(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let split = |v: &str| -> (Vec<u64>, Option<String>) {
        let v = v.trim().trim_start_matches('v');
        let v = v.split('+').next().unwrap_or(v);
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p.to_string())),
            None => (v, None),
        };
        (core.split('.').map(|p| p.parse().unwrap_or(0)).collect(), pre)
    };
    let ((ac, ap), (bc, bp)) = (split(a), split(b));
    let n = ac.len().max(bc.len());
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    if let Some(o) = (0..n).map(|i| at(&ac, i).cmp(&at(&bc, i))).find(|o| o.is_ne()) {
        return o;
    }
    match (ap, bp) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            let (xs, ys): (Vec<&str>, Vec<&str>) = (x.split('.').collect(), y.split('.').collect());
            for i in 0..xs.len().max(ys.len()) {
                let o = match (xs.get(i), ys.get(i)) {
                    (Some(p), Some(q)) => match (p.parse::<u64>(), q.parse::<u64>()) {
                        (Ok(m), Ok(n)) => m.cmp(&n),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        _ => p.cmp(q),
                    },
                    (None, Some(_)) => Ordering::Less,
                    (Some(_), None) => Ordering::Greater,
                    (None, None) => Ordering::Equal,
                };
                if o.is_ne() {
                    return o;
                }
            }
            Ordering::Equal
        }
    }
}

/// Whether version `a` is later than `b` ([`cmp_versions`]).
pub fn is_newer(a: &str, b: &str) -> bool {
    cmp_versions(a, b) == std::cmp::Ordering::Greater
}

/// The newest release, when GitHub answers.
/// From the release list, not `releases/latest`: that one leaves pre-releases out, and every
/// NVForever release so far is one (it answers 404).
#[cfg(feature = "desktop")]
pub fn fetch_latest() -> Result<Option<Latest>, String> {
    let text = release_list(&agent())?;
    Ok(newest(parse_release_list(&text)?))
}

/// The site's next release, if it announces one.
#[cfg(feature = "desktop")]
pub fn fetch_next_release() -> Result<Option<NextRelease>, String> {
    let js = get_text(&agent(), SITE_CONFIG)?;
    Ok(parse_next_release(&js))
}

#[cfg(all(test, feature = "desktop"))]
mod tests {
    use super::*;

    #[test]
    fn next_release_from_the_sites_config() {
        let js = "window.NVF_CONFIG = {\n  version: \"0.7.0\",\n  nextRelease: {\n    version: \"0.8.0\",\n    date: \"2026-11-18T20:00:00Z\"\n  },\n  links: {}\n};";
        let n = parse_next_release(js).unwrap();
        assert_eq!(n.version, "0.8.0");
        assert_eq!(n.at, parse_iso("2026-11-18T20:00:00Z"));
        assert_eq!(parse_next_release("nextRelease: { version: \"\", date: \"\" }"), None);
        assert_eq!(parse_next_release("nextRelease: { version: '0.9.0', date: '' }"), Some(NextRelease { version: "0.9.0".into(), at: None }));
    }

    #[test]
    fn newer_versions() {
        assert!(is_newer("0.8.0", "0.7.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("0.7.1", "0.7"));
        assert!(!is_newer("0.7.0", "0.7.0"));
        assert!(!is_newer("0.6.9", "0.7.0"));
        let l = parse_release_list(r#"[{"tag_name":"v0.8.0","html_url":"https://x/v0.8.0","assets":[]}]"#).unwrap().remove(0);
        assert_eq!(l, Latest { version: "0.8.0".into(), tag: "v0.8.0".into(), url: "https://x/v0.8.0".into(), ..Latest::default() });
    }

    #[test]
    fn the_newest_release_by_version() {
        let list = parse_release_list(
            r#"[{"tag_name":"v0.7.1","html_url":"a","body":"fix\r\nes"},
                {"tag_name":"v0.9.0","draft":true},
                {"tag_name":"v0.8.0","html_url":"b","assets":[{"name":"SHA256SUMS.txt","browser_download_url":"https://x/s","size":225}]},
                {"tag_name":"v0.7.0"}]"#,
        )
        .unwrap();
        assert_eq!(list.len(), 3, "the draft is left out");
        assert_eq!(list[0].notes, "fix\nes");
        let n = newest(list).unwrap();
        assert_eq!((n.version.as_str(), n.tag.as_str(), n.assets.len()), ("0.8.0", "v0.8.0", 1));
        assert_eq!(n.assets[0].size, 225);
        assert_eq!(newest(Vec::new()), None);
        assert_eq!(cmp_versions("0.7", "0.7.0"), std::cmp::Ordering::Equal);
        // A pre-release comes before its release, and after the one before.
        assert!(is_newer("0.9.0", "0.9.0-rc.1"));
        assert!(!is_newer("0.9.0-rc.1", "0.9.0"));
        assert!(is_newer("0.9.0-rc.2", "0.9.0-rc.1"));
        assert!(is_newer("0.9.0-rc.10", "0.9.0-rc.9"));
        assert!(is_newer("0.9.0-rc.1", "0.8.5"));
        assert!(is_newer("0.9.0-rc.1", "0.9.0-beta.3"));
        assert_eq!(cmp_versions("0.9.0+build.5", "0.9.0"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn iso_times() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("2000-03-01T00:00Z"), Some(951_868_800));
        assert_eq!(parse_iso("2026-11-18T21:00:00+01:00"), parse_iso("2026-11-18T20:00:00Z"));
        assert_eq!(parse_iso("nope"), None);
        // A zone of the right length whose bytes split a character (#4590).
        assert_eq!(parse_iso("2026-11-18T21:00:00+0\u{e9}00"), None);
        assert_eq!(countdown_text(2 * 86_400 + 14 * 3600 + 3 * 60 + 12), "2d 14h 03m 12s");
        assert_eq!(countdown_text(-5), "00h 00m 00s");
    }

    #[test]
    fn site_posts() {
        let p = parse_site(r#"{"posts":[{"title":"A","date":"2026-11-13","body":"*hi*","image":"https://x/y.jpg"},{"title":"B"}]}"#).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].shown_date(), "13 November 2026");
        assert_eq!(p[1].shown_date(), "");
        assert!(parse_site("[]").is_err());
    }

    #[test]
    fn releases_as_posts() {
        let p = parse_releases(
            r#"[{"name":"","tag_name":"v0.7.0","published_at":"2026-11-11T10:00:00Z","body":"a\r\nb","html_url":"u","author":{"login":"L"}},
                {"name":"x","tag_name":"v9","draft":true}]"#,
        )
        .unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].title.as_str(), p[0].body.as_str(), p[0].author.as_str()), ("v0.7.0", "a\nb", "L"));
        assert_eq!(p[0].shown_date(), "11 November 2026");
    }

    #[test]
    fn cache_round_trip() {
        let dir = std::env::temp_dir().join(format!("nvf-launcher-news-{}", std::process::id()));
        let posts = vec![Post { title: "T".into(), body: "B".into(), image_bytes: Some(vec![1]), ..Post::default() }];
        save_cache(&dir, &posts);
        let back = load_cache(&dir);
        assert_eq!(back[0].title, "T");
        assert_eq!(back[0].image_bytes, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
