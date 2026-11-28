//! Authored public file queries in owned temporary directories, no installed data.
//! A dedicated target because Roots is process-wide; each case is also serialized.
mod common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{GameData, game::{Game, GameConfig}, mod_files::{self, Roots}};
use fnv_script::{FunctionTable, vm::XVal};
use fnv_sim::{FormStore, RefTable, SimConfig, World};
use std::{collections::BTreeMap, path::{Path, PathBuf}, sync::{Mutex, MutexGuard}, time::{SystemTime, UNIX_EPOCH}};

static ROOTS_LOCK: Mutex<()> = Mutex::new(());

struct Files {
    _lock: MutexGuard<'static, ()>,
    root: PathBuf,
    temp: PathBuf,
    previous: Option<Roots>,
    inputs: BTreeMap<PathBuf, Vec<u8>>,
}

impl Files {
    fn new() -> Self {
        let lock = ROOTS_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = temp.join(format!("nvf-owned-getfiles-{}-{nonce}", std::process::id()));
        // Exclusive creation establishes cleanup ownership, even if setup later fails.
        std::fs::create_dir(&root).unwrap();
        let mut out = Self { _lock: lock, root, temp, previous: None, inputs: BTreeMap::new() };
        for dir in ["game/Data/Allowed/Child", "game/Data/Allowed/Nested", "game/Data/Allowed/.settings", "game/Allowed/ParentFolder", "game/Outside/OutsideFolder", "layer/Allowed"] {
            std::fs::create_dir_all(out.root.join(dir)).unwrap();
        }
        for (path, body) in [
            ("game/Data/root.ini", "17\t29\n"),
            ("game/Data/Allowed/valid.ini", "17\t29\n"),
            ("game/Data/Allowed/extensionless_valid", "$Owned\t$Value\n"),
            ("game/Data/Allowed/__folder_managed_by_vortex", "$Owned metadata\n"),
            ("game/Data/Allowed/shared.ini", "31\t43\n"),
            ("game/Data/Allowed/Nested/deep.ini", "37\t41\n"),
            ("game/Data/Allowed/.settings/inside.ini", "43\t47\n"),
            ("game/Allowed/parent_only.ini", "47\t59\n"),
            ("game/Outside/outside.ini", "61\t73\n"),
            ("layer/Allowed/SHARED.INI", "79\t83\n"),
            ("layer/Allowed/layer.ini", "89\t97\n"),
        ] {
            let path = out.root.join(path);
            std::fs::write(&path, body).unwrap();
            out.inputs.insert(path, body.as_bytes().to_vec());
        }
        let read = vec![out.root.join("layer"), out.root.join("game/Data")];
        mod_files::update_roots(|roots| out.previous = Some(std::mem::replace(roots, Roots { read, ..Default::default() })));
        out
    }

    fn unchanged(&self) {
        for (path, expected) in &self.inputs {
            assert_eq!(std::fs::read(path).unwrap(), *expected, "{}", path.display());
        }
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            mod_files::set_roots(previous);
        }
        if self.root.parent() == Some(self.temp.as_path())
            && self.root.file_name().is_some_and(|n| n.to_string_lossy().starts_with("nvf-owned-getfiles-")) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn with_game(f: impl FnOnce(&mut Game<'_>)) {
    let bytes = tes4();
    let plugins = [Plugin::parse(&bytes, "OwnedFileQuery.esm".into()).unwrap()];
    let order = LoadOrder::new(&plugins).unwrap();
    let schemas = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(schemas.join("fnv")).unwrap();
    let functions = schemas.join("script/functions.json");
    let catalogue = serde_json::from_slice(&std::fs::read(&functions).unwrap()).unwrap();
    let forms = FormStore::build(&order, &schema);
    let refs = RefTable::build(&order, &schema);
    let data = GameData::build(&order, &schema, FunctionTable::load(&functions).unwrap(), &catalogue, &refs);
    let mut game = Game::new(&data, World::new(&forms, &refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() });
    f(&mut game);
}

fn query(game: &mut Game<'_>, call: &str, result: &str) -> XVal {
    let source = format!("array_var a\nBegin Function {{}}\nlet a := {call}\nSetFunctionValue ({result})\nEnd\n");
    game.run_query(&source, Default::default()).unwrap_or_else(|error| panic!("{source}: {error}"))
}

#[test]
fn files_only_come_from_data_roots_not_parent_namesakes() {
    let files = Files::new();
    with_game(|game| {
        assert_eq!(query(game, "GetFiles \"Allowed\"", "(ar_Find \"valid.ini\" a) >= 0"), XVal::Num(1.0));
        assert_eq!(query(game, "GetFiles \"Allowed\"", "(ar_Find \"parent_only.ini\" a) >= 0"), XVal::Num(0.0));
        assert_eq!(query(game, "GetFilesInFolder \"Allowed\"", "ar_Size a"), XVal::Num(5.0));
        // This separate API is documented game-relative; its parent lookup survives.
        assert_eq!(query(game, "ReadArrayFromFile \"Allowed/parent_only.ini\"", "a[0] * 100 + a[1]"), XVal::Num(4759.0));
    });
    files.unchanged();
}

#[test]
fn folders_only_come_from_data_roots_not_parent_namesakes() {
    let files = Files::new();
    with_game(|game| {
        assert_eq!(query(game, "GetFolders \"Allowed\"", "(ar_Find \"Child\" a) >= 0"), XVal::Num(1.0));
        assert_eq!(query(game, "GetFolders \"Allowed\"", "(ar_Find \"ParentFolder\" a) >= 0"), XVal::Num(0.0));
        assert_eq!(query(game, "GetFoldersInFolder \"Allowed\" \"Ch*\"", "ar_Size a"), XVal::Num(1.0));
    });
    files.unchanged();
}

#[test]
fn enumeration_refuses_paths_outside_the_lexical_data_root() {
    let files = Files::new();
    // Keep every old parent-fallback join inside this owned fixture too. Never
    // enumerate a drive root while comparing the unfixed implementation.
    mod_files::update_roots(|roots| roots.read = vec![files.root.join("game/Data")]);
    with_game(|game| {
        assert_eq!(query(game, "GetFiles \"Allowed\"", "(ar_Find \"valid.ini\" a) >= 0"), XVal::Num(1.0));
        let absolute = files.root.join("game/Outside").to_string_lossy().replace('\\', "/");
        for path in ["../Outside", "..\\Outside", "Allowed/../../Outside", absolute.as_str()] {
            for command in ["GetFiles", "GetFolders"] {
                assert_eq!(query(game, &format!("{command} \"{path}\""), "ar_Size a"), XVal::Num(0.0), "{command} {path}");
            }
        }
    });
    files.unchanged();
}

#[test]
fn ordinary_filters_layers_root_and_marker_neighbors_remain_visible() {
    let files = Files::new();
    with_game(|game| {
        let all = "GetFiles \"aLLoWeD\"";
        assert_eq!(query(game, all, "ar_Size a"), XVal::Num(5.0));
        for name in ["__folder_managed_by_vortex", "extensionless_valid", "layer.ini", "SHARED.INI", "valid.ini"] {
            assert_eq!(query(game, all, &format!("(ar_Find \"{name}\" a) >= 0")), XVal::Num(1.0), "{name}");
        }
        assert_eq!(query(game, all, "a[0]"), XVal::Str("__folder_managed_by_vortex".into()));
        assert_eq!(query(game, all, "a[3]"), XVal::Str("SHARED.INI".into()));
        assert_eq!(query(game, "GetFiles \"Allowed\" \"*.ini\"", "ar_Size a"), XVal::Num(3.0));
        assert_eq!(query(game, "GetFiles \"Allowed\" \"__folder_managed_by_vortex\"", "ar_Size a"), XVal::Num(1.0));
        assert_eq!(query(game, "GetFiles \"aLLoWeD/nEsTeD\"", "a[0]"), XVal::Str("deep.ini".into()));
        assert_eq!(query(game, "GetFiles \"Allowed/.settings\"", "a[0]"), XVal::Str("inside.ini".into()));
        assert_eq!(query(game, "GetFiles \"\"", "ar_Size a"), XVal::Num(1.0));
        assert_eq!(query(game, "GetFiles \"\"", "a[0]"), XVal::Str("root.ini".into()));
        assert_eq!(query(game, "GetFolders \"\"", "ar_Size a"), XVal::Num(1.0));
        assert_eq!(query(game, "GetFiles \"Absent\"", "ar_Size a"), XVal::Num(0.0));
    });
    files.unchanged();
}

#[test]
fn model_queries_keep_file_fallback_until_a_reference_tree_is_published() {
    use common::model_has_block::{with_game as with_model_game, has, tree, FIRST};
    let files = Files::new();
    let reads = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = reads.clone();
    // The existing file query consumes length-prefixed strings; these authored
    // bytes characterize that fallback without claiming complete NIF parsing.
    let name = b"FileBlock";
    let bytes = [(name.len() as u32).to_le_bytes().as_slice(), name].concat();
    assert!(!mod_files::nif_has_name(&bytes, ""), "the established file helper rejects an empty name");
    mod_files::update_roots(|roots| roots.reader = Some(std::sync::Arc::new(move |path| {
        seen.lock().unwrap().push(path.to_string());
        (path == "meshes\\Owned/base.nif").then(|| bytes.clone())
    })));
    with_model_game(|game| {
        assert_eq!(has(game, "OwnedModel", "FileBlock"), XVal::Num(1.0));
        assert_eq!(has(game, "OwnedRefA", "FileBlock"), XVal::Num(1.0));
        assert_eq!(has(game, "OwnedRefA", "fileblock"), XVal::Num(0.0));
        assert_eq!(has(game, "OwnedRefA", "Absent"), XVal::Num(0.0));
        let before_empty = reads.lock().unwrap().len();
        assert_eq!(has(game, "OwnedModel", ""), XVal::Num(0.0));
        assert_eq!(has(game, "OwnedRefA", ""), XVal::Num(0.0));
        assert_eq!(reads.lock().unwrap().len(), before_empty + 2, "base and cold empty queries retain their existing file consumer");
        let before = reads.lock().unwrap().len();
        game.scene.set_tree(FIRST, tree("SceneBlock"));
        assert_eq!(has(game, "OwnedRefA", "SceneBlock"), XVal::Num(1.0));
        assert_eq!(has(game, "OwnedRefA", "FileBlock"), XVal::Num(0.0));
        game.scene.set_tree(FIRST, Vec::new());
        assert_eq!(has(game, "OwnedRefA", "FileBlock"), XVal::Num(0.0), "an empty published tree is authoritative");
        assert_eq!(reads.lock().unwrap().len(), before, "published queries do not read the base file");
        game.scene.forget(FIRST);
        assert_eq!(has(game, "OwnedRefA", "FileBlock"), XVal::Num(1.0), "retain cold fallback after unload");
        game.world.set_deleted(FIRST, true).unwrap();
        assert_eq!(has(game, "OwnedRefA", "FileBlock"), XVal::Num(1.0), "retain the previous deleted-reference file lookup");
        assert_eq!(has(game, "57005", "FileBlock"), XVal::Num(0.0));
    });
    files.unchanged();
}

#[test]
fn knvse_ini_loads_from_disk_layer_and_vfs_and_reloads_when_roots_update() {
    let files = Files::new();
    // Start with roots containing no kNVSE.ini -> vanilla empty list.
    with_game(|game| {
        assert!(game.knvse_ini.legacy_anim_time_paths.is_empty());

        // Populate VFS reader fallback first.
        mod_files::update_roots(|r| {
            r.reader = Some(std::sync::Arc::new(|p: &str| {
                if p.replace('/', "\\").eq_ignore_ascii_case("NVSE\\Plugins\\kNVSE.ini") {
                    Some(b"[Anim Fixes]\nsLegacyAnimTimePaths = B42Inject\n".to_vec())
                } else {
                    None
                }
            }));
        });
        game.tick();
        assert_eq!(game.knvse_ini.legacy_anim_time_paths, vec!["b42inject"]);

        // Loose file in Data overrides archive/VFS reader; mods layer overrides Data.
        let data_plugins = files.root.join("game/Data/nvse/plugins");
        let layer_plugins = files.root.join("layer/NVSE/Plugins");
        std::fs::create_dir_all(&data_plugins).unwrap();
        std::fs::create_dir_all(&layer_plugins).unwrap();
        std::fs::write(
            data_plugins.join("knvse.ini"),
            b"[Anim Fixes]\nsLegacyAnimTimePaths = B42Interact\n",
        )
        .unwrap();
        mod_files::update_roots(|_| {});
        game.tick();
        assert_eq!(game.knvse_ini.legacy_anim_time_paths, vec!["b42interact"]);

        std::fs::write(
            layer_plugins.join("kNVSE.ini"),
            b"\xEF\xBB\xBF[Anim Fixes]\nsLegacyAnimTimePaths = B42Inject,B42Interact,B42Loot\n",
        )
        .unwrap();
        mod_files::update_roots(|_| {});
        game.tick();
        assert_eq!(game.knvse_ini.legacy_anim_time_paths, vec!["b42inject", "b42interact", "b42loot"]);

        // Manual override is preserved across subsequent ticks and root updates.
        game.knvse_ini.legacy_anim_time_paths.clear();
        mod_files::update_roots(|_| {});
        game.tick();
        assert!(game.knvse_ini.legacy_anim_time_paths.is_empty());
    });
    files.unchanged();
}
