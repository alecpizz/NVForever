//! Synthetic VFS integration tests for Stewie Tweaks modular override folders
//! (`Data\NVSE\Plugins\Tweaks\INIs\*.ini` and
//! `Data\NVSE\Plugins\Tweaks\GameSettings\*.ini`).
//!
//! Independently authored synthetic records and temporary directories only (no game
//! data, no private paths, and no provider-version uplift).

mod common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{
    game::{Game, GameConfig, RunCtx},
    mod_files::{self, Roots},
    tweaks::Tweaks,
    ui_search_bar, GameData,
};
use fnv_script::{vm::XVal, FunctionTable};
use fnv_sim::{settings::Setting, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static ROOTS_LOCK: Mutex<()> = Mutex::new(());

struct TempDataRoots {
    _lock: MutexGuard<'static, ()>,
    previous: Roots,
    root: PathBuf,
    data: PathBuf,
}

impl TempDataRoots {
    fn new(tag: &str) -> Self {
        let lock = ROOTS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("nvf-stewie-modular-{tag}-{}-{stamp}", std::process::id()));
        let data = root.join("Data");
        std::fs::create_dir_all(&data).unwrap();
        let mut previous = Roots::default();
        mod_files::update_roots(|r| {
            previous = std::mem::replace(
                r,
                Roots {
                    read: vec![data.clone()],
                    ..Default::default()
                },
            );
        });
        Self {
            _lock: lock,
            previous,
            root,
            data,
        }
    }

    fn write(&self, rel: &str, contents: &str) {
        let path = self.data.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
}

impl Drop for TempDataRoots {
    fn drop(&mut self) {
        mod_files::set_roots(self.previous.clone());
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn gmst_float(id: u32, edid: &str, value: f32) -> Vec<u8> {
    record(b"GMST", 0, id, &[sub(b"EDID", z(edid)), sub(b"DATA", value.to_le_bytes().to_vec())])
}

fn gmst_int(id: u32, edid: &str, value: i32) -> Vec<u8> {
    record(b"GMST", 0, id, &[sub(b"EDID", z(edid)), sub(b"DATA", value.to_le_bytes().to_vec())])
}

fn gmst_str(id: u32, edid: &str, value: &str) -> Vec<u8> {
    record(b"GMST", 0, id, &[sub(b"EDID", z(edid)), sub(b"DATA", z(value))])
}

fn fixture() -> Fixture {
    let gmsts = [
        gmst_float(0x101, "fOwnedFloatSetting", 10.0),
        gmst_float(0x102, "fOwnedUnchangedFloat", 2.5),
        gmst_int(0x103, "iOwnedIntSetting", 5),
        gmst_str(0x104, "sOwnedStringSetting", "Original"),
        gmst_str(0x105, "sOwnedQuotedSetting", "Plain"),
    ]
    .concat();
    let npc = record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]);
    let bytes = [tes4(), top(b"GMST", &gmsts), top(b"NPC_", &npc)].concat();
    let plugins = [Plugin::parse(&bytes, "StewieModular.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let funcs = root.join("script/functions.json");
    let json: Json = serde_json::from_str(&std::fs::read_to_string(&funcs).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&funcs).unwrap(), &json, &refs);
    Fixture { data, forms, refs }
}

fn make_game(f: &Fixture) -> Game<'_> {
    Game::new(
        &f.data,
        World::new(&f.forms, &f.refs, SimConfig::default()),
        GameConfig {
            strict: true,
            ..Default::default()
        },
    )
}

fn eval_expr(g: &mut Game<'_>, expr: &str) -> XVal {
    g.run_query(&format!("Begin Function {{}}\nSetFunctionValue ({expr})\nEnd\n"), RunCtx::default())
        .unwrap_or_else(|e| panic!("{expr}: {e}"))
}

#[test]
fn absent_modular_folders_preserve_vanilla_and_single_ini_behavior() {
    let fs = TempDataRoots::new("absent");
    fs.write(
        "NVSE/Plugins/nvse_stewie_tweaks.ini",
        "[Tweaks]\r\nbJumpWhileAiming=1\r\nbCritChanceIgnoresFireRate=1\r\n[Critical Hits]\r\nfCriticalDamageMult=2.5\r\n",
    );

    let f = fixture();
    let mut g = make_game(&f);

    // Base nvse_stewie_tweaks.ini never imports into Tweaks defaults.
    assert_eq!(g.tweaks, Tweaks::default());
    assert!(!ui_search_bar::menu_search_enabled(&g));

    // Vanilla GMSTs remain untouched.
    assert_eq!(g.world.setting_f32("fOwnedFloatSetting"), Some(10.0));
    assert_eq!(g.world.setting_f32("iOwnedIntSetting"), Some(5.0));
    assert_eq!(g.world.setting_value("sOwnedStringSetting"), Some(&Setting::Str("Original".into())));

    // Single-INI StewVal / StewieGetValue lookup works identically.
    assert_eq!(eval_expr(&mut g, r#"StewVal "bJumpWhileAiming:Tweaks""#), XVal::Num(1.0));
    assert_eq!(eval_expr(&mut g, r#"StewieGetValue "fCriticalDamageMult:Critical Hits""#), XVal::Num(2.5));
    assert_eq!(eval_expr(&mut g, r#"GetPluginVersion "lStewieAl's Tweaks""#), XVal::Num(960.0));
}

#[test]
fn sorted_modular_tweak_inis_override_base_ini_and_reach_tweaks_and_stewval() {
    let fs = TempDataRoots::new("inis");
    fs.write(
        "NVSE/Plugins/nvse_stewie_tweaks.ini",
        "[Tweaks]\n\
         bBaseOnly=1\n\
         bOverriddenByMod=0\n\
         bNoCasinoBans=0\n\
         bMenuSearch=0\n\
         [Custom Section]\n\
         sLabel=\"FromBase\"\n\
         fRate=1.0\n",
    );
    fs.write(
        "NVSE/Plugins/Tweaks/INIs/10_alpha.ini",
        "; first modular override\n\
         [Tweaks]\n\
         bOverriddenByMod = 1 ; enabled in alpha\n\
         bNoCasinoBans = 1\n\
         bMenuSearch = 1\n\
         bNoLockEncounterZoneLevels = 1\n\
         bFasterSleepWait = 1\n\
         bUseWASDAsArrowKeys = 1\n\
         bCritChanceIgnoresFireRate = 1\n\
         bHardcoreTweaks = 1\n\
         iPerksPerLevel = 3\n\
         fCraftedItemHealthPct = 80.0\n\
         [Critical Hits]\n\
         fCriticalDamageMult = 1.5\n\
         [Hardcore Tweaks]\n\
         bAmmoWeight = 0\n\
         [Custom Section]\n\
         sLabel = \"FromAlpha\"\n\
         fRate = 2.0\n",
    );
    fs.write(
        "NVSE/Plugins/Tweaks/INIs/20_beta.INI",
        "[tweaks]\n\
         bNoCasinoBans = 0 ; beta turns it back off\n\
         bPowerArmorNeedsNoTraining = 1\n\
         [Critical Hits]\n\
         fCriticalDamageMult = 1.75\n\
         [Custom Section]\n\
         fRate = 3.5\n",
    );
    // Non-INI sibling must be ignored.
    fs.write(
        "NVSE/Plugins/Tweaks/INIs/99_ignored.txt",
        "[Tweaks]\nbPowerArmorNeedsNoTraining=0\nbBaseOnly=0\n",
    );

    let f = fixture();
    let mut g = make_game(&f);

    // Merged modular values apply to Game::tweaks and ui_search_bar.
    assert!(!g.tweaks.no_casino_bans, "20_beta.INI overrides 10_alpha.ini");
    assert!(g.tweaks.power_armor_needs_no_training);
    assert!(g.tweaks.menu_search);
    assert!(g.tweaks.no_lock_encounter_zone_levels);
    assert!(g.tweaks.faster_sleep_wait);
    assert!(g.tweaks.wasd_as_arrow_keys);
    assert!(g.tweaks.crit_ignores_fire_rate);
    assert!(g.tweaks.hardcore_tweaks);
    assert!(!g.tweaks.hardcore_ammo_weight);
    assert_eq!(g.tweaks.perks_per_level, 3);
    assert_eq!(g.tweaks.crafted_item_health_pct, 80.0);
    assert_eq!(g.tweaks.crit_damage_scale, 1.75);
    assert!(ui_search_bar::menu_search_enabled(&g));

    // Unbound StewVal / StewieGetValue queries see base + sorted modular merges.
    for fn_name in ["StewVal", "StewieGetValue"] {
        assert_eq!(eval_expr(&mut g, &format!(r#"{fn_name} "bBaseOnly:Tweaks""#)), XVal::Num(1.0));
        assert_eq!(eval_expr(&mut g, &format!(r#"{fn_name} "bOverriddenByMod:Tweaks""#)), XVal::Num(1.0));
        assert_eq!(eval_expr(&mut g, &format!(r#"{fn_name} "bNoCasinoBans:Tweaks""#)), XVal::Num(0.0));
        assert_eq!(eval_expr(&mut g, &format!(r#"{fn_name} "bPowerArmorNeedsNoTraining:Tweaks""#)), XVal::Num(1.0));
        assert_eq!(eval_expr(&mut g, &format!(r#"{fn_name} "fCriticalDamageMult:Critical Hits""#)), XVal::Num(1.75));
        assert_eq!(eval_expr(&mut g, &format!(r#"{fn_name} "bAmmoWeight:Hardcore Tweaks""#)), XVal::Num(0.0));
        assert_eq!(eval_expr(&mut g, &format!(r#"{fn_name} "sLabel:Custom Section""#)), XVal::Str("FromAlpha".into()));
        assert_eq!(eval_expr(&mut g, &format!(r#"{fn_name} "fRate:Custom Section""#)), XVal::Num(3.5));
    }

    // Binding live engine tweaks (as nvf-play does) keeps bound settings authoritative
    // while unmapped keys still resolve through base + modular INIs.
    let bound = g.tweaks.clone();
    g.bind_stewie_stat_settings(&bound);
    assert_eq!(eval_expr(&mut g, r#"StewVal "bCritChanceIgnoresFireRate:Tweaks""#), XVal::Num(1.0));
    assert_eq!(eval_expr(&mut g, r#"StewVal "bNoLockEncounterZoneLevels:Tweaks""#), XVal::Num(1.0));
    assert_eq!(eval_expr(&mut g, r#"StewieGetValue "bNoLockEncounterZoneLevels:Tweaks""#), XVal::Num(1.0));
    assert_eq!(eval_expr(&mut g, r#"StewVal "bFasterSleepWait:Tweaks""#), XVal::Num(1.0));
    assert_eq!(eval_expr(&mut g, r#"StewieGetValue "bFasterSleepWait:Tweaks""#), XVal::Num(1.0));
    assert_eq!(eval_expr(&mut g, r#"StewVal "bUseWASDAsArrowKeys:Tweaks""#), XVal::Num(1.0));
    assert_eq!(eval_expr(&mut g, r#"StewieGetValue "bUseWASDAsArrowKeys:Tweaks""#), XVal::Num(1.0));
    assert_eq!(eval_expr(&mut g, r#"StewVal "fCraftedItemHealthPct:Tweaks""#), XVal::Num(80.0));
    assert_eq!(eval_expr(&mut g, r#"StewieGetValue "fCraftedItemHealthPct:Tweaks""#), XVal::Num(80.0));
    assert_eq!(eval_expr(&mut g, r#"StewieGetValue "fCriticalDamageMult:Critical Hits""#), XVal::Num(1.75));
    assert_eq!(eval_expr(&mut g, r#"StewVal "bAmmoWeight:Hardcore Tweaks""#), XVal::Num(0.0));
    assert_eq!(eval_expr(&mut g, r#"StewVal "fRate:Custom Section""#), XVal::Num(3.5));
}

#[test]
fn sorted_modular_game_settings_override_float_int_and_string_gmsts() {
    let fs = TempDataRoots::new("gmst");
    // Case-insensitive directory path + headerless file + sectioned file + comments + BOM.
    fs.write(
        "nvse/plugins/tweaks/gamesettings/01_first.ini",
        "\u{feff}; headerless GMST overrides\r\n\
         fOwnedFloatSetting = 25.5 ; inline comment\r\n\
         iOwnedIntSetting = 12\r\n\
         sOwnedStringSetting = FirstValue ; stripped comment\r\n\
         sOwnedQuotedSetting = \"Keep;Semicolon\" ; outside quotes\r\n",
    );
    fs.write(
        "nvse/plugins/tweaks/gamesettings/02_second.InI",
        "[GameSettings]\n\
         iOwnedIntSetting = 99\n\
         sOwnedStringSetting = \"Second Value\"\n\
         [IgnoredSection]\n\
         fOwnedFloatSetting = 999.0\n",
    );
    fs.write(
        "nvse/plugins/tweaks/gamesettings/03_empty_and_malformed.ini",
        "; only comments and malformed lines\n\
         \n\
         not an assignment\n\
         = missing_name\n\
         fOwnedFloatSetting = not_a_float\n\
         iOwnedIntSetting = bad_int\n\
         fUnknownSetting = 42.0\n\
         uUnsupportedPrefix = 7\n",
    );

    let f = fixture();
    let mut g = make_game(&f);

    assert_eq!(g.world.setting_f32("fOwnedFloatSetting"), Some(25.5));
    assert_eq!(g.world.setting_f32("fOwnedUnchangedFloat"), Some(2.5));
    assert_eq!(g.world.setting_f32("iOwnedIntSetting"), Some(99.0));
    assert_eq!(g.world.setting_value("sOwnedStringSetting"), Some(&Setting::Str("Second Value".into())));
    assert_eq!(g.world.setting_value("sOwnedQuotedSetting"), Some(&Setting::Str("Keep;Semicolon".into())));
    assert_eq!(g.world.setting_value("fUnknownSetting"), None);

    // Script getters see every modular GMST override.
    assert_eq!(eval_expr(&mut g, r#"GetGameSetting "fOwnedFloatSetting""#), XVal::Num(25.5));
    assert_eq!(eval_expr(&mut g, r#"GetNumericGameSetting "fOwnedFloatSetting""#), XVal::Num(25.5));
    assert_eq!(eval_expr(&mut g, r#"GetNumericGameSetting "iOwnedIntSetting""#), XVal::Num(99.0));
    assert_eq!(eval_expr(&mut g, r#"GetStringGameSetting "sOwnedStringSetting""#), XVal::Str("Second Value".into()));
    assert_eq!(eval_expr(&mut g, r#"GetStringGameSetting "sOwnedQuotedSetting""#), XVal::Str("Keep;Semicolon".into()));

    // Runtime script SetNumericGameSetting still overrides startup GameSettings INI.
    assert_eq!(eval_expr(&mut g, r#"SetNumericGameSetting "fOwnedFloatSetting" 77.25"#), XVal::Num(1.0));
    assert_eq!(eval_expr(&mut g, r#"GetNumericGameSetting "fOwnedFloatSetting""#), XVal::Num(77.25));
}

#[test]
fn in_memory_mod_files_session_writes_apply_modular_inis_and_game_settings() {
    let _fs = TempDataRoots::new("memory");
    let f = fixture();
    let mut g = make_game(&f);

    assert!(g.mod_files.write_bytes(
        "NVSE/Plugins/Tweaks/INIs/a_mod.ini",
        b"[Tweaks]\nbPowerArmorNeedsNoTraining=1\nbMenuSearch=1\n",
    ));
    assert!(g.mod_files.write_bytes(
        "NVSE/Plugins/Tweaks/GameSettings/a_gmst.ini",
        b"[GameSettings]\nfOwnedFloatSetting=64.0\nsOwnedStringSetting=\"FromMemory\"\n",
    ));

    let (tweaks_applied, gmsts_applied) = g.apply_stewie_modular_inis();
    assert_eq!(tweaks_applied, 2);
    assert_eq!(gmsts_applied, 2);
    assert!(g.tweaks.power_armor_needs_no_training);
    assert!(g.tweaks.menu_search);
    assert_eq!(g.world.setting_f32("fOwnedFloatSetting"), Some(64.0));
    assert_eq!(g.world.setting_value("sOwnedStringSetting"), Some(&Setting::Str("FromMemory".into())));
    assert_eq!(eval_expr(&mut g, r#"StewVal "bPowerArmorNeedsNoTraining:Tweaks""#), XVal::Num(1.0));
}
