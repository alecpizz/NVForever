//! JohnnyGuitar NVSE batch 9 (#1007; docs/spec-notes/ext-jg9.md) on a synthetic plugin:
//! sounds from files, holotapes, terminal menu items, audio markers, the player's moves and
//! the values the engine tells.

mod common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::nvse_jg9::{data_path, HolotapeRequest, MarkerValues, HOLOTAPE_STOP, PATH_SOUND_BASE};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const TERM: u32 = 0x900;
const SOUN: u32 = 0x901;
const ALOC: u32 = 0x902;

fn glob(fid: u32, name: &str, v: f32) -> Vec<u8> {
    record(b"GLOB", 0, fid, &[sub(b"EDID", z(name)), sub(b"FNAM", vec![b'f']), sub(b"FLTV", v.to_le_bytes().to_vec())])
}

fn plugin_bytes() -> Vec<u8> {
    let term = record(b"TERM", 0, TERM, &[sub(b"EDID", z("TestTerminal")), sub(b"ITXT", z("One")), sub(b"ITXT", z("Two"))]);
    let globs = [glob(0x910, "KarmaVeryEvil", -750.0), glob(0x911, "KarmaEvil", -250.0), glob(0x912, "KarmaGood", 250.0), glob(0x913, "KarmaVeryGood", 750.0)].concat();
    [
        tes4(),
        top(b"NPC_", &record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))])),
        top(b"GLOB", &globs),
        top(b"SOUN", &record(b"SOUN", 0, SOUN, &[sub(b"EDID", z("TestSound"))])),
        top(b"TERM", &term),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Jg9.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() })
}

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(g: &mut Game, expr: &str) -> f64 {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Num(x) => x,
        XVal::Ref(r) => f64::from(r),
        v => panic!("{expr}: {v:?}"),
    }
}

fn s(g: &mut Game, expr: &str) -> String {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Str(s) => s,
        v => panic!("{expr}: {v:?}"),
    }
}

#[test]
fn sounds_from_files_and_holotapes() {
    // [documented: paths "relative to the New Vegas folder"; engineering choice: Data's only].
    assert_eq!(data_path("Data\\Sound\\FX\\Test.wav").as_deref(), Some("sound\\fx\\test.wav"));
    assert_eq!(data_path("data/music/x.mp3").as_deref(), Some("music\\x.mp3"));
    assert_eq!(data_path("FalloutNV.exe"), None);
    assert_eq!(data_path("Data\\..\\FalloutNV.exe"), None);
    assert_eq!(data_path("Data\\C:\\x.wav"), None);

    let dir = std::env::temp_dir().join(format!("nvf-jg9-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("sound").join("fx")).unwrap();
    std::fs::write(dir.join("sound").join("fx").join("test.wav"), b"RIFF").unwrap();
    fnv_game::mod_files::set_roots(fnv_game::mod_files::Roots { read: vec![dir.clone()], ..Default::default() });

    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, r#"PlaySoundFromPath "Data\Sound\fx\test.wav" 2.5"#), 1.0);
    assert_eq!(n(&mut g, r#"PlaySoundFromPath "Data\Sound\fx\nosuch.wav""#), 1.0, "no such file: 1 all the same, as the original");
    assert_eq!(n(&mut g, r#"PlaySoundFromPath "Data\Sound\fx\test.wav""#), 1.0);
    // One sound for the file, played twice; the first fades in.
    let st = &g.other.jg9;
    assert_eq!(st.path_sounds.len(), 1);
    assert_eq!(st.new_paths, vec![(FormId(PATH_SOUND_BASE), "sound\\fx\\test.wav".to_string(), false)]);
    assert_eq!(st.fades_in.len(), 1);
    assert_eq!(st.fades_in[0].1, 2.5);
    let asked: Vec<FormId> = g.sound_requests.iter().map(|r| r.sound).collect();
    assert_eq!(asked, vec![FormId(PATH_SOUND_BASE); 2]);
    // Looping, or at a reference, is a sound of its own.
    assert_eq!(n(&mut g, r#"PlaySoundFromPath "Data\Sound\fx\test.wav" 0 0 0 1"#), 1.0);
    assert_eq!(n(&mut g, r#"player.PlaySound3DFromPath "Data\Sound\fx\test.wav""#), 1.0);
    assert_eq!(g.other.jg9.path_sounds.len(), 3);
    assert!(g.other.jg9.path_sounds[1].looping);
    assert_eq!(g.other.jg9.path_sounds[2].at, Some(FormId::PLAYER_REF));
    // The stops: those of the file, with the fade; StopSoundFile those of PlaySoundFile.
    assert_eq!(n(&mut g, r#"StopSoundFromPath "Data\Sound\fx\test.wav" 1.5"#), 0.0, "0 though it stops them, as the original");
    assert_eq!(g.other.jg9.fades_out.len(), 3);
    assert!(g.other.jg9.fades_out.iter().all(|f| f.1 == 1.5));
    assert_eq!(n(&mut g, r#"PlaySoundFile "Data\Sound\fx\test.wav" 0 1"#), 1.0);
    assert!(g.other.jg9.path_sounds[3].file);
    // Nonexistent WAV path still returns 1 without adding a path sound
    // [observed: b3_jg9_01_sound_runspeed_compass_pipboy]; escaping Data\ also plays
    // nothing and returns 1 [design].
    let before_paths = g.other.jg9.path_sounds.len();
    assert_eq!(n(&mut g, r#"PlaySoundFile "sound\fx\nvseprobe_no_such.wav""#), 1.0);
    assert_eq!(n(&mut g, r#"PlaySoundFile "Data\Sound\fx\nosuch.wav""#), 1.0);
    assert_eq!(n(&mut g, r#"PlaySoundFile "..\escape.wav""#), 1.0);
    assert_eq!(g.other.jg9.path_sounds.len(), before_paths);
    // PlaySoundFade: a SOUN at the player, faded in.
    assert_eq!(n(&mut g, "player.PlaySoundFade TestSound 2"), 1.0);
    assert_eq!(g.sound_requests.last().map(|r| (r.sound, r.at)), Some((FormId(SOUN), Some(FormId::PLAYER_REF))));
    // Holotapes: a form that is no note plays nothing; StopHolotape plays its sound.
    assert_eq!(n(&mut g, "PlayHolotape TestSound"), 0.0);
    assert_eq!(n(&mut g, "StopHolotape"), 1.0);
    assert_eq!(g.take_holotape_requests(), vec![HolotapeRequest::Stop]);
    assert_eq!(g.sound_requests.last().map(|r| r.sound), Some(HOLOTAPE_STOP));
    assert_eq!(n(&mut g, "StopHolotape 0"), 1.0);
    assert_eq!(g.sound_requests.last().map(|r| r.sound), Some(HOLOTAPE_STOP), "no second sound");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn terminal_items_come_and_go() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "GetTerminalMenuItemCount TestTerminal"), 2.0);
    assert_eq!(n(&mut g, "AddTerminalMenuItem TestTerminal"), 1.0);
    assert_eq!(n(&mut g, "GetTerminalMenuItemCount TestTerminal"), 3.0);
    assert_eq!(s(&mut g, "GetTerminalMenuItemText TestTerminal 1 2"), "", "an added item is empty");
    let _ = q(&mut g, r#"SetTerminalMenuItemText TestTerminal 1 2 "Three""#);
    assert_eq!(n(&mut g, "RemoveTerminalMenuItem TestTerminal 0"), 1.0);
    assert_eq!(n(&mut g, "RemoveTerminalMenuItem TestTerminal 5"), 0.0, "no such item");
    assert_eq!(n(&mut g, "GetTerminalMenuItemCount TestTerminal"), 2.0);
    // The items after the removed one move up, with a script's text.
    assert_eq!(s(&mut g, "GetTerminalMenuItemText TestTerminal 1 0"), "Two");
    assert_eq!(s(&mut g, "GetTerminalMenuItemText TestTerminal 1 1"), "Three");
    // A removal redraws an open menu, whose positions moved (#2018).
    assert!(g.other.jg9.refresh_terminal);
    g.other.jg9.refresh_terminal = false;
    assert_eq!(n(&mut g, "RemoveTerminalMenuItem TestTerminal 7"), 0.0);
    assert!(!g.other.jg9.refresh_terminal, "nothing removed, nothing redrawn");
    let _ = q(&mut g, "RefreshTerminalMenu");
    assert!(g.other.jg9.refresh_terminal);
}

#[test]
fn markers_moves_and_told_values() {
    let f = fixture();
    let mut g = game(&f);
    // Audio markers: the values the engine told, and a script's changes.
    let marker = FormId::PLAYER_REF;
    g.other.jg9.markers.insert(marker, MarkerValues { controller: Some(FormId(ALOC)), radius: 1500.0, layer2: 0.33, layer3: 0.67 });
    g.other.jg9.current_marker = Some(marker);
    assert_eq!(n(&mut g, "player.AudioMarkerGetController"), f64::from(ALOC));
    assert_eq!(n(&mut g, "AudioMarkerGetCurrent"), f64::from(FormId::PLAYER_REF.0));
    assert_eq!(n(&mut g, "player.AudioMarkerGetProperty 0"), 1500.0);
    assert!((n(&mut g, "player.AMKGetProp 2") - 0.67).abs() < 1e-6);
    assert_eq!(n(&mut g, "player.AudioMarkerSetProperty 0 900"), 1.0);
    assert_eq!(n(&mut g, "player.AudioMarkerGetProperty 0"), 900.0);
    assert!(g.other.jg9.markers_changed);
    assert_eq!(n(&mut g, "player.AudioMarkerSetProperty 3 1"), 0.0, "no property 3");
    // The moves, for the engine.
    let _ = q(&mut g, "player.Jump");
    let _ = q(&mut g, "SetAutoMove 1");
    let _ = q(&mut g, "SetPlayerMovementFlags 513");
    let _ = q(&mut g, "player.SetVelEx 10 20 300");
    let st = &g.other.jg9;
    assert_eq!(st.jumps, vec![FormId::PLAYER_REF]);
    assert_eq!(st.auto_move, Some(true));
    assert_eq!(st.move_flags, Some(513));
    assert_eq!(st.velocities, vec![(FormId::PLAYER_REF, [10.0, 20.0, 300.0])]);
    // What the engine tells.
    g.other.jg9.run_speeds.insert(FormId::PLAYER_REF, 344.0);
    g.other.jg9.spreads.insert(FormId::PLAYER_REF, 1.25);
    g.other.jg9.grenade_hold = 0.75;
    g.other.jg9.compass_hostile = [Some(2), None];
    let mut sky = [[0.0; 3]; 10];
    sky[4] = [255.0, 204.0, 51.0];
    g.other.jg9.sky_colors = Some(sky);
    assert_eq!(n(&mut g, "player.GetRunSpeed"), 344.0);
    assert_eq!(n(&mut g, "player.GetCalculatedSpread"), 1.25);
    assert_eq!(n(&mut g, "GetGrenadeHoldTime"), 0.75);
    assert_eq!(n(&mut g, "GetNearestCompassHostileDirection"), 2.0);
    assert_eq!(n(&mut g, "GetNearestCompassHostileDirection 1"), -1.0, "none that is not invisible");
    // Normalized 0..1 float channels; out-of-range type returns 0 and leaves variables
    // unchanged [observed: w6_t1_09_jg_sky_color_interior].
    assert_eq!(
        q(&mut g, "float r = 111\nfloat gr = 222\nfloat b = 333\nint ok = GetCurrentSkyColor 4 r gr b\nSetFunctionValue (sv_Construct \"%g %.1f %.1f %.1f\" ok r gr b)"),
        XVal::Str("1 1.0 0.8 0.2".into())
    );
    assert_eq!(
        q(&mut g, "float r = 111\nfloat gr = 222\nfloat b = 333\nint ok = GetCurrentSkyColor 10 r gr b\nSetFunctionValue (sv_Construct \"%g %.0f %.0f %.0f\" ok r gr b)"),
        XVal::Str("0 111 222 333".into())
    );
    g.other.jg9.sky_colors = None;
    assert_eq!(
        q(&mut g, "float r = 111\nfloat gr = 222\nfloat b = 333\nint ok = GetCurrentSkyColor 4 r gr b\nSetFunctionValue (sv_Construct \"%g %.0f %.0f %.0f\" ok r gr b)"),
        XVal::Str("0 111 222 333".into()),
        "headless with no sky preserves variables"
    );
    // FOV: the INI's defaults without a SetCameraFOV (75 and 55; see docs/spec-notes/ext-jg9.md).
    assert_eq!(n(&mut g, "player.GetPlayerCamFOV 0"), 75.0);
    assert_eq!(n(&mut g, "player.GetPlayerCamFOV 1"), 55.0);
    assert_eq!(n(&mut g, "player.GetPlayerCamFOV 2"), 55.0, "first person until the engine tells");
    g.other.jg9.third_person = true;
    assert_eq!(n(&mut g, "player.GetPlayerCamFOV 2"), 75.0);
    // The settings page's values once the engine tells them (at start, then after a
    // change), over the INI's; a SetCameraFOV value over both, until it is set back.
    g.other.jg9.third_person = false;
    g.ui.camera.settings_fov = (Some(90.0), Some(70.0));
    assert_eq!(n(&mut g, "GetCameraFOV 0"), 90.0);
    assert_eq!(n(&mut g, "GetCameraFOV 1"), 70.0);
    assert_eq!(n(&mut g, "player.GetPlayerCamFOV 0"), 90.0);
    assert_eq!(n(&mut g, "player.GetPlayerCamFOV 1"), 70.0);
    assert_eq!(n(&mut g, "player.GetPlayerCamFOV 2"), 70.0, "the viewmodel's in first person");
    g.ui.camera.settings_fov = (Some(80.0), Some(45.0));
    assert_eq!(n(&mut g, "player.GetPlayerCamFOV 0"), 80.0);
    assert_eq!(n(&mut g, "player.GetPlayerCamFOV 1"), 45.0);
    q(&mut g, "SetCameraFOV 100 60");
    assert_eq!(n(&mut g, "player.GetPlayerCamFOV 0"), 100.0, "the console's fov wins");
    assert_eq!(n(&mut g, "player.GetPlayerCamFOV 1"), 60.0);
    q(&mut g, "SetCameraFOV 0 0");
    assert_eq!(n(&mut g, "player.GetPlayerCamFOV 0"), 80.0, "back to the setting");
    assert_eq!(n(&mut g, "GetCameraFOV 1"), 45.0);
    g.ui.camera.settings_fov = (None, None);
    // Karma: neutral at 0 between the globals; with no title setting in the fixture,
    // the title is the alignment word.
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 1"), "Neutral");
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 0"), "Neutral");
}

#[test]
fn ttw_ini_karma_titles_override_levels_1_through_50() {
    let f = fixture();
    let mut g = game(&f);

    // Without TTW.ini: the plugins' settings, none in the fixture, so the alignment word.
    g.player.level = Some(1);
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 0"), "Neutral");
    g.player.level = Some(45);
    fnv_game::functions::set_actor_value_current(&mut g, FormId::PLAYER_REF, fnv_game::actor_values::KARMA, -500.0).unwrap();
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 1"), "Evil");
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 0"), "Evil");
    g.player.level = Some(50);
    fnv_game::functions::set_actor_value_current(&mut g, FormId::PLAYER_REF, fnv_game::actor_values::KARMA, 800.0).unwrap();
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 1"), "Very Good");
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 0"), "Very Good");

    // Synthetic TTW.ini providing custom level 1..=50 karma titles.
    let mut ini = String::from("[Karma Titles]\n");
    for lvl in 1..=50 {
        ini.push_str(&format!("sKarmicTitleGood{lvl}=TestGood{lvl}\n"));
        ini.push_str(&format!("sKarmicTitleNeutral{lvl}=TestNeutral{lvl}\n"));
        ini.push_str(&format!("sKarmicTitleEvil{lvl}=TestEvil{lvl}\n"));
    }
    assert!(g.mod_files.write_bytes("NVSE/Plugins/TTW.ini", ini.as_bytes()));

    // Good & Very Good
    g.player.level = Some(50);
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 1"), "Very Good");
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 0"), "TestGood50");
    g.player.level = Some(1);
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 0"), "TestGood1");

    // Neutral
    fnv_game::functions::set_actor_value_current(&mut g, FormId::PLAYER_REF, fnv_game::actor_values::KARMA, 0.0).unwrap();
    g.player.level = Some(35);
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 1"), "Neutral");
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 0"), "TestNeutral35");

    // Evil & Very Evil (and clamping above 50 to top defined tier 50)
    fnv_game::functions::set_actor_value_current(&mut g, FormId::PLAYER_REF, fnv_game::actor_values::KARMA, -900.0).unwrap();
    g.player.level = Some(31);
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 1"), "Very Evil");
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 0"), "TestEvil31");
    g.player.level = Some(55);
    assert_eq!(s(&mut g, "GetPlayerKarmaTitle 0"), "TestEvil50");
}
