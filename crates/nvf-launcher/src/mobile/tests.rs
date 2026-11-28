use super::*;
use std::{fs, sync::atomic::{AtomicUsize, Ordering}};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!("nvf-mobile-launcher-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn home(&self) -> PathBuf { self.0.join("home") }
    fn call(&self, request_json: Value) -> Value {
        serde_json::from_str(&request(&self.home(), &request_json.to_string(), |url| panic!("unexpected request to {url}"))).unwrap()
    }
    fn copy(&self) -> PathBuf {
        let root = self.0.join("imported copy");
        let data = root.join("Data");
        fs::create_dir_all(&data).unwrap();
        for name in fnv_install::REQUIRED_DATA_FILES {
            fs::write(data.join(name.to_lowercase()), [1]).unwrap();
        }
        root
    }
    fn ini(&self, text: &str) {
        fs::create_dir_all(self.home()).unwrap();
        fs::write(self.home().join(fnv_install::CONFIG_FILE), text).unwrap();
    }
    fn select(&self, root: &Path) { self.ini(&format!("[Game]\nPath={}\n", root.display())); }
}
impl Drop for Fixture {
    fn drop(&mut self) { fs::remove_dir_all(&self.0).unwrap(); }
}

#[test]
fn play_and_continue_recheck_the_import_and_continue_selects_the_newest_save() {
    let fixture = Fixture::new();
    let start = |resume| fixture.call(json!({"action": "start", "resume": resume}));
    fixture.ini("[Settings]\nhardcore=1\n");
    assert_eq!(start(false)["error"], IMPORT_FIRST);
    assert_eq!(start(true)["error"], IMPORT_FIRST);
    assert!(fixture.call(json!({"action":"state"}))["data"]["folder"].is_null());

    let root = fixture.copy();
    fixture.select(&root);
    assert_eq!(start(false)["data"], json!(["--play", "--menu", "--android-player-start"]));
    assert_eq!(start(true)["error"], "No NVForever save is available to continue.");
    let saves = EngineHome::new(fixture.home()).saves();
    fs::create_dir_all(&saves).unwrap();
    for (name, time) in [("Earlier", 2), ("Latest", 8)] {
        let mut header = fnv_save::SaveHeader::new(fnv_save::SaveKind::Manual);
        header.player_name = name.into();
        header.saved_unix = time;
        fnv_save::write(&saves.join(format!("{name}.{}", fnv_save::EXTENSION)), &header, &[], b"{}", false).unwrap();
    }
    let latest = saves.join(format!("Latest.{}", fnv_save::EXTENSION));
    assert_eq!(start(true)["data"], json!(["--play", "--load", latest.to_str().unwrap(), "--android-player-start"]));
    assert_eq!(fixture.call(json!({"action":"state"}))["data"]["continue"], "Latest");
    fs::remove_file(root.join("Data/update.bsa")).unwrap();
    assert_eq!(start(false)["error"], IMPORT_FIRST);
    assert_eq!(start(true)["error"], IMPORT_FIRST);
}

#[test]
fn saf_copy_validation_requires_nonempty_archives_before_path_commit() {
    let fixture = Fixture::new();
    let root = fixture.copy();
    let check = |path: &Path| fixture.call(json!({"action": "validate_import", "path": path}));
    fixture.ini("[Game]\nPath=/previous\n[Settings]\nhardcore=1\n");
    assert_eq!(check(&root)["data"], Value::Null);
    assert!(check(&root.join("Data"))["error"].as_str().unwrap().contains("own Data"));
    fs::write(root.join("Data/update.bsa"), []).unwrap();
    assert!(check(&root)["error"].as_str().unwrap().contains("Update.bsa"));
    let ini = fs::read_to_string(fixture.home().join(fnv_install::CONFIG_FILE)).unwrap();
    assert_eq!(ini_get(&ini, "Game", "Path").as_deref(), Some("/previous"));
    assert_eq!(ini_get(&ini, "Settings", "hardcore").as_deref(), Some("1"));
}

#[test]
fn settings_read_and_save_use_player_catalogue_without_overwriting_the_export_or_other_keys() {
    let fixture = Fixture::new();
    fixture.ini("[Game]\nPath=/keep\n[Settings]\nframe-cap=0.20\n");
    let row = |key: &str| menu::Row { key: key.into(), label: key.into(), note: "engine export".into(), kind: menu::Kind::Switch, default: "0".into(), shown_with: None, needs: vec![] };
    let menu = menu::Menu {
        version: env!("CARGO_PKG_VERSION").into(), layout: nvf_settings_layout::mobile_layout_stamp(),
        tabs: vec![
            menu::Tab { name: "Mobile".into(), label: "Mobile".into(), categories: vec![menu::Category { group: None, heading: Some("Movement".into()), rows: vec![row("touch-gyro-v1"), row("touch-layout-editor-v1")] }] },
            menu::Tab { name: "Display".into(), label: "Display".into(), categories: vec![menu::Category { group: None, heading: None, rows: vec![row("borderless"), row("resolution"), row("dlss-model")] }] },
        ],
    }.to_text();
    let export = fixture.home().join(menu::MENU_FILE);
    fs::write(&export, &menu).unwrap();
    let rows = fixture.call(json!({"action": "settings", "edits": {"touch-gyro-v1": "1"}}))["data"]["rows"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["tab"], "Mobile");
    assert_eq!(rows[0]["value"], "1");
    assert_eq!(rows[0]["control"]["type"], "switch");
    let config = fixture.home().join(fnv_install::CONFIG_FILE);
    assert!(ini_get(&fs::read_to_string(&config).unwrap(), "Settings", "touch-gyro-v1").is_none());
    fixture.ini("; Changed after viewing\n[Game]\nPath=/keep\n[Settings]\nframe-cap=0.60\n");
    let reply = fixture.call(json!({"action":"save_settings", "edits":{"touch-gyro-v1":"1"}}));
    assert_eq!(reply, json!({"data":null}));
    let ini = fs::read_to_string(&config).unwrap();
    assert!(ini.starts_with("; Changed after viewing\n"));
    assert_eq!(ini_get(&ini, "Game", "Path").as_deref(), Some("/keep"));
    assert_eq!(ini_get(&ini, "Settings", "frame-cap").as_deref(), Some("0.60"));
    assert_eq!(ini_get(&ini, "Settings", "touch-gyro-v1").as_deref(), Some("1"));
    assert_eq!(fs::read_to_string(&export).unwrap(), menu);
    assert!(fixture.call(json!({"action":"save_settings", "edits":{"borderless":"1"}}))["error"].is_string());
    assert_eq!(fs::read_to_string(&config).unwrap(), ini);
    let desktop_export = menu.replace(&nvf_settings_layout::mobile_layout_stamp(), &nvf_settings_layout::layout_stamp());
    fs::write(&export, desktop_export).unwrap();
    assert!(fixture.call(json!({"action":"settings", "edits":{}}))["data"]["rows"]
        .as_array().unwrap().iter().all(|row| row["key"] != "touch-gyro-v1"));
}

#[test]
fn mobile_catalog_disables_and_restores_the_same_saved_dialogue_choice() {
    let fixture = Fixture::new();
    fixture.ini("[Settings]\ntouch-dialogue=0\ndialogue-theme-v1=Classic\n");
    let row = |key: &str, kind| menu::Row {
        key: key.into(), label: key.into(), kind,
        note: String::new(), default: String::new(), shown_with: None, needs: vec![],
    };
    let menu = menu::Menu {
        version: env!("CARGO_PKG_VERSION").into(),
        layout: nvf_settings_layout::mobile_layout_stamp(),
        tabs: vec![menu::Tab {
            name: MOBILE_TAB.into(), label: MOBILE_TAB.into(),
            categories: vec![menu::Category {
                group: None, heading: Some("Dialogue".into()),
                rows: vec![row("touch-dialogue", menu::Kind::Switch), row("dialogue-theme-v1", menu::Kind::List {
                    choices: ["Normal", "Classic"].map(|name| menu::Choice {
                        name: name.into(), label: name.into(),
                    }).to_vec(),
                })],
            }],
        }],
    };
    fs::write(fixture.home().join(menu::MENU_FILE), menu.to_text()).unwrap();
    let theme = |enabled| fixture.call(json!({"action":"settings", "edits":{"touch-dialogue":enabled}}))
        ["data"]["rows"].as_array().unwrap().iter().find(|r| r["key"] == "dialogue-theme-v1").unwrap().clone();
    let disabled = theme("0");
    assert_eq!(disabled["value"], "Classic");
    assert_eq!(disabled["reason"], "Needs Touch dialogue.");
    let enabled = theme("1");
    assert_eq!(enabled["value"], "Classic");
    assert!(enabled["reason"].is_null());
}

#[test]
fn android_lists_omit_dlss_and_news_off_never_requests_a_url() {
    let fixture = Fixture::new();
    fixture.ini("[Launcher]\nNews=0\n");
    let model = fixture.call(json!({"action":"settings", "edits":{}}));
    let rows = model["data"]["rows"].as_array().unwrap();
    for key in ["upscaler", "frame-generation"] {
        let row = rows.iter().find(|r| r["key"] == key).unwrap();
        let choices = row["control"]["choices"].as_array().unwrap();
        assert_eq!(choices.len(), 2);
        assert!(choices.iter().all(|choice| !choice["name"].as_str().unwrap().to_ascii_lowercase().contains("dlss")));
        assert!(fixture.call(json!({"action":"save_settings", "edits": {(key): "DLSS"}}))["error"].is_string());
    }
    assert_eq!(fixture.call(json!({"action":"fetch_news"}))["data"]["posts"], json!([]));
}

#[test]
fn mods_edit_the_real_plugin_list_and_shared_new_game_fields() {
    let fixture = Fixture::new();
    let root = fixture.copy();
    fixture.select(&root);
    fs::write(root.join("Data/TaleOfTwoWastelands.esm"), [1]).unwrap();
    fs::write(root.join("Data/Mod.esp"), [1]).unwrap();
    let mods = || fixture.call(json!({"action":"mods"}))["data"].clone();
    let model = mods();
    assert_eq!(model["ttw"], false);
    assert_eq!(model["entries"][0]["name"], "falloutnv.esm");
    assert_eq!(fixture.call(json!({"action":"plugin", "name":"TaleOfTwoWastelands.esm", "change":"toggle"})), json!({"data":null}));
    assert_eq!(mods()["ttw"], true);
    assert_eq!(fixture.call(json!({"action":"save_start", "fields":model["ttwDefaults"]})), json!({"data":null}));
    let fields: start::Fields = serde_json::from_value(mods()["fields"].clone()).unwrap();
    assert_eq!(fields, start::Fields::ttw());
}

#[test]
fn check_update_selects_signed_arm64_apk_verifies_url_and_extracts_checksum() {
    let fixture = Fixture::new();
    let expected_sha = "a".repeat(64);
    let releases = json!([
        {
            "tag_name": "v0.9.0",
            "draft": true,
            "html_url": "https://github.com/LootJunkieAzza/NVForever/releases/tag/v0.9.0",
            "body": "Draft release should be ignored",
            "assets": []
        },
        {
            "tag_name": "v0.8.2",
            "draft": false,
            "html_url": "https://github.com/LootJunkieAzza/NVForever/releases/tag/v0.8.2",
            "body": "## Highlights\n- Better Android launcher",
            "assets": [
                {
                    "name": "NVForever-v0.8.2-x86_64.apk",
                    "browser_download_url": "https://github.com/LootJunkieAzza/NVForever/releases/download/v0.8.2/NVForever-v0.8.2-x86_64.apk",
                    "size": 100
                },
                {
                    "name": "NVForever-v0.8.2-android-arm64-unsigned.apk",
                    "browser_download_url": "https://github.com/LootJunkieAzza/NVForever/releases/download/v0.8.2/NVForever-v0.8.2-android-arm64-unsigned.apk",
                    "size": 200
                },
                {
                    "name": "NVForever-v0.8.2-android-arm64.apk",
                    "browser_download_url": "https://github.com/LootJunkieAzza/NVForever/releases/download/v0.8.2/NVForever-v0.8.2-android-arm64.apk",
                    "size": 45_000_000
                },
                {
                    "name": "SHA256SUMS.txt",
                    "browser_download_url": "https://github.com/LootJunkieAzza/NVForever/releases/download/v0.8.2/SHA256SUMS.txt",
                    "size": 120
                }
            ]
        }
    ]).to_string();
    let sums = format!("{expected_sha}  NVForever-v0.8.2-android-arm64.apk\n");
    let reply: Value = serde_json::from_str(&request(
        &fixture.home(),
        &json!({"action": "check_update", "current": "0.8.1"}).to_string(),
        |url| match url {
            u if u == news::RELEASES => Ok(releases.clone()),
            "https://github.com/LootJunkieAzza/NVForever/releases/download/v0.8.2/SHA256SUMS.txt" => Ok(sums.clone()),
            other => Err(format!("unexpected url {other}")),
        },
    )).unwrap();
    let data = &reply["data"];
    assert_eq!(data["available"], true);
    assert_eq!(data["version"], "0.8.2");
    assert_eq!(data["tag"], "v0.8.2");
    assert_eq!(data["asset"]["name"], "NVForever-v0.8.2-android-arm64.apk");
    assert_eq!(data["asset"]["size"], 45_000_000);
    assert_eq!(data["asset"]["sha256"], expected_sha);

    // Same or newer current version reports available=false.
    let up_to_date: Value = serde_json::from_str(&request(
        &fixture.home(),
        &json!({"action": "check_update", "current": "0.8.2"}).to_string(),
        |_| Ok(releases.clone()),
    )).unwrap();
    assert_eq!(up_to_date["data"]["available"], false);

    // Off-repository APK download URL is refused (asset is null, release page URL stays available).
    let off_repo = json!([{
        "tag_name": "v0.8.3",
        "draft": false,
        "html_url": "https://github.com/LootJunkieAzza/NVForever/releases/tag/v0.8.3",
        "body": "Notes",
        "assets": [{
            "name": "NVForever-v0.8.3-android-arm64.apk",
            "browser_download_url": "https://evil.example/NVForever-v0.8.3-android-arm64.apk",
            "size": 100
        }]
    }]).to_string();
    let refused: Value = serde_json::from_str(&request(
        &fixture.home(),
        &json!({"action": "check_update", "current": "0.8.1"}).to_string(),
        |_| Ok(off_repo.clone()),
    )).unwrap();
    assert_eq!(refused["data"]["available"], true);
    assert!(refused["data"]["asset"].is_null());

    // Fail closed when SHA256SUMS.txt is published on the release but fetching it fails.
    let sums_fetch_failed: Value = serde_json::from_str(&request(
        &fixture.home(),
        &json!({"action": "check_update", "current": "0.8.1"}).to_string(),
        |url| match url {
            u if u == news::RELEASES => Ok(releases.clone()),
            _ => Err("simulated 302/network failure".into()),
        },
    )).unwrap();
    assert_eq!(sums_fetch_failed["data"]["available"], true);
    assert!(sums_fetch_failed["data"]["asset"].is_null());

    // Fail closed when SHA256SUMS.txt is fetched but omits a valid 64-hex hash for the APK.
    let sums_missing_entry: Value = serde_json::from_str(&request(
        &fixture.home(),
        &json!({"action": "check_update", "current": "0.8.1"}).to_string(),
        |url| match url {
            u if u == news::RELEASES => Ok(releases.clone()),
            _ => Ok(format!("{}  NVForever-v0.8.2-x86_64-pc-windows-msvc.zip\n", "b".repeat(64))),
        },
    )).unwrap();
    assert_eq!(sums_missing_entry["data"]["available"], true);
    assert!(sums_missing_entry["data"]["asset"].is_null());

    // Fail closed when a release publishes no SHA256SUMS.txt asset at all.
    let no_sums_release = json!([{
        "tag_name": "v0.8.5",
        "draft": false,
        "html_url": "https://github.com/LootJunkieAzza/NVForever/releases/tag/v0.8.5",
        "body": "Release missing checksums",
        "assets": [{
            "name": "NVForever-v0.8.5-android-arm64.apk",
            "browser_download_url": "https://github.com/LootJunkieAzza/NVForever/releases/download/v0.8.5/NVForever-v0.8.5-android-arm64.apk",
            "size": 45_000_000
        }]
    }]).to_string();
    let refused_no_sums: Value = serde_json::from_str(&request(
        &fixture.home(),
        &json!({"action": "check_update", "current": "0.8.1"}).to_string(),
        |_| Ok(no_sums_release.clone()),
    )).unwrap();
    assert_eq!(refused_no_sums["data"]["available"], true);
    assert!(refused_no_sums["data"]["asset"].is_null());

    // Unsigned-only ARM64 release is refused (Android package installer rejects unsigned APKs).
    let unsigned_only = json!([{
        "tag_name": "v0.8.4",
        "draft": false,
        "html_url": "https://github.com/LootJunkieAzza/NVForever/releases/tag/v0.8.4",
        "body": "Unsigned build only",
        "assets": [{
            "name": "NVForever-v0.8.4-android-arm64-unsigned.apk",
            "browser_download_url": "https://github.com/LootJunkieAzza/NVForever/releases/download/v0.8.4/NVForever-v0.8.4-android-arm64-unsigned.apk",
            "size": 45_000_000
        }]
    }]).to_string();
    let refused_unsigned: Value = serde_json::from_str(&request(
        &fixture.home(),
        &json!({"action": "check_update", "current": "0.8.1"}).to_string(),
        |_| Ok(unsigned_only.clone()),
    )).unwrap();
    assert_eq!(refused_unsigned["data"]["available"], true);
    assert!(refused_unsigned["data"]["asset"].is_null());
}

#[test]
fn state_exposes_last_save_preview_with_metadata_and_png_thumbnail() {
    let fixture = Fixture::new();
    let root = fixture.copy();
    fixture.select(&root);
    // Without a save, savePreview is null.
    let empty: Value = serde_json::from_str(&request(&fixture.home(), r#"{"action":"state"}"#, |_| Err("offline".into()))).unwrap();
    assert!(empty["data"]["savePreview"].is_null());

    // Write a real NVForever save containing header metadata and a PNG thumbnail.
    let saves = EngineHome::new(fixture.home()).saves();
    std::fs::create_dir_all(&saves).unwrap();
    let png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x01, 0x02, 0x03];
    let mut header = fnv_save::SaveHeader::new(fnv_save::SaveKind::Manual);
    header.player_name = "Courier Six".into();
    header.level = 12;
    header.location = "Goodsprings".into();
    header.play_seconds = 8100.0; // 2h 15m
    header.saved_unix = 1_700_000_000;
    fnv_save::write(&saves.join(format!("0001_CourierSix.{}", fnv_save::EXTENSION)), &header, &png, b"{}", false).unwrap();

    let with_save: Value = serde_json::from_str(&request(&fixture.home(), r#"{"action":"state"}"#, |_| Err("offline".into()))).unwrap();
    let preview = &with_save["data"]["savePreview"];
    assert_eq!(preview["name"], "Courier Six");
    assert_eq!(preview["level"], 12);
    assert_eq!(preview["location"], "Goodsprings");
    assert_eq!(preview["playtime"], "2h 15m");
    assert_eq!(preview["detail"], "Level 12 · Goodsprings · 2h 15m");
    assert_eq!(preview["shotBase64"], base64_encode(&png));
    assert_eq!(with_save["data"]["continue"], "Courier Six");
}

#[test]
fn android_revision_version_ordering_and_update_check() {
    use std::cmp::Ordering;
    assert_eq!(cmp_versions("0.7.0-android-r10", "v0.7.0"), Ordering::Greater);
    assert_eq!(cmp_versions("v0.7.0", "0.7.0-android-r10"), Ordering::Less);
    assert_eq!(cmp_versions("0.7.0-android-r10", "0.7.0-android-r9"), Ordering::Greater);
    assert_eq!(cmp_versions("0.7.0-android-r10", "v0.7.0-android-r10"), Ordering::Equal);
    assert_eq!(cmp_versions("v0.7.1", "0.7.0-android-r10"), Ordering::Greater);
    assert_eq!(cmp_versions("v0.8.0", "0.7.0-android-r10"), Ordering::Greater);
    // Ordinary SemVer pre-releases keep their standard ordering below final releases.
    assert_eq!(cmp_versions("0.9.0-rc.1", "0.9.0"), Ordering::Less);
    assert_eq!(cmp_versions("0.9.0-rc.10", "0.9.0-rc.9"), Ordering::Greater);

    // Installed 0.7.0-android-r10 must NOT treat v0.7.0 as an available upgrade.
    let releases_json = r#"[{"tag_name":"v0.7.0","name":"v0.7.0","draft":false,"prerelease":false,"html_url":"https://github.com/LootJunkieAzza/NVForever/releases/tag/v0.7.0","body":"","assets":[]}]"#;
    let res = check_update("0.7.0-android-r10", &mut |_| Ok(releases_json.into())).unwrap();
    assert_eq!(res["available"], false);
}
