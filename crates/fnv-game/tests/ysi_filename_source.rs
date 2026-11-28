//! Synthetic ySI configuration/source/inventory/UI consumers, no original files.
//! Own process and lock: the existing file-reader roots are process-wide.
mod common;
use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{
    game::{Game, GameConfig, GameSnapshot, RunCtx},
    mod_files::{self, Roots},
    trace::EventKind,
    GameData,
};
use fnv_script::{vm::XVal, FunctionTable};
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use fnv_ui::MemSource;
use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};
static LOCK: Mutex<()> = Mutex::new(());
const ITEM: FormId = FormId(0x820);
const PLACED: FormId = FormId(0x900);
struct Files {
    _lock: MutexGuard<'static, ()>,
    root: PathBuf,
    temp: PathBuf,
    previous: Option<Roots>,
}
impl Files {
    fn new() -> Self {
        let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let root = temp.join(format!(
            "nvf-owned-ysi-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let mut out = Self {
            _lock: lock,
            root,
            temp,
            previous: None,
        };
        std::fs::create_dir_all(out.root.join("Data/menus/ySI")).unwrap();
        mod_files::update_roots(|r| {
            out.previous = Some(std::mem::replace(
                r,
                Roots {
                    read: vec![out.root.join("Data")],
                    ..Default::default()
                },
            ))
        });
        out
    }
    fn write(&self, name: &str, text: &str) {
        assert!(!name.contains(['\\', '/', ':']));
        std::fs::write(self.root.join("Data/menus/ySI").join(name), text).unwrap();
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        if let Some(old) = self.previous.take() {
            mod_files::set_roots(old);
        }
        if self.root.parent() == Some(self.temp.as_path())
            && self
                .root
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("nvf-owned-ysi-"))
        {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}
struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}
fn fixture() -> Fixture {
    fixture_with_item_flags(0)
}
fn fixture_with_item_flags(flags: u32) -> Fixture {
    let item = record(
        b"MISC",
        flags,
        ITEM.0,
        &[
            sub(b"EDID", z("OwnYsiItem")),
            sub(b"DATA", [1i32.to_le_bytes(), 1f32.to_le_bytes()].concat()),
        ],
    );
    let placed = record(
        b"REFR",
        0,
        PLACED.0,
        &[
            sub(b"EDID", z("OwnYsiPlaced")),
            sub(b"NAME", ITEM.0.to_le_bytes().to_vec()),
            sub(b"DATA", vec![0; 24]),
        ],
    );
    let room = record(b"CELL", 0, 0x200, &[sub(b"EDID", z("OwnYsiRoom"))]);
    let children = group(
        0x200u32.to_le_bytes(),
        6,
        &group(0x200u32.to_le_bytes(), 9, &placed),
    );
    let bytes = [
        tes4(),
        top(b"MISC", &item),
        top(
            b"NPC_",
            &record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))]),
        ),
        top(
            b"CELL",
            &group(
                0i32.to_le_bytes(),
                2,
                &group(0i32.to_le_bytes(), 3, &[room, children].concat()),
            ),
        ),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "OwnedYsi.esm".into()).unwrap()];
    let order = LoadOrder::new(&plugins).unwrap();
    let schema =
        Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/script/functions.json");
    let catalogue = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let forms = FormStore::build(&order, &schema);
    let refs = RefTable::build(&order, &schema);
    let data = GameData::build(
        &order,
        &schema,
        FunctionTable::load(&path).unwrap(),
        &catalogue,
        &refs,
    );
    Fixture { data, forms, refs }
}
fn game(f: &Fixture) -> Game<'_> {
    let mut g = Game::new(
        &f.data,
        World::new(&f.forms, &f.refs, SimConfig::default()),
        GameConfig {
            strict: true,
            ..Default::default()
        },
    );
    let source=MemSource::default().with("menus/owned.xml",r#"<menu name="HUDMainMenu"><image name="OwnedIcon"><filename>Owned/before.dds</filename></image><image name="ForeignIcon"><filename>Owned/foreign.dds</filename></image></menu>"#);
    g.ui_tree.load_menu(&source, "menus/owned.xml").unwrap();
    g.ui_tree.update();
    g.ui_files = Some(Box::new(source));
    g
}
fn query(g: &mut Game<'_>, body: &str) -> Result<XVal, String> {
    g.run_query(&format!("ref item\nref empty\nstring_var icon\narray_var pair\nbegin Function {{}}\n{body}\nend"),RunCtx {self_ref:Some(FormId::PLAYER_REF),..Default::default()})
}
fn success(g: &mut Game<'_>, body: &str) -> XVal {
    let before = g.events.len();
    let result = query(g, body).unwrap_or_else(|e| panic!("{body}: {e}"));
    assert!(
        !g.events[before..].iter().any(|e| matches!(
            e.kind,
            EventKind::Stub { .. }
                | EventKind::Unhandled { .. }
                | EventKind::Skipped { .. }
                | EventKind::CallFailed { .. }
                | EventKind::ScriptError { .. }
                | EventKind::ArgUnread { .. }
        )),
        "{:?}",
        &g.events[before..]
    );
    result
}
fn config(tags: serde_json::Value, icons: serde_json::Value) -> String {
    serde_json::json!({"tags":tags,"icons":icons}).to_string()
}
fn certain(filename: &str) -> String {
    config(
        serde_json::json!([{"priority":20,"mod":"OwnedYsi.esm","form":"00000820","tag":"OwnedTag"}]),
        serde_json::json!([{"priority":10,"tag":"OwnedTag","filename":filename}]),
    )
}

#[test]
fn required_filename_calls_reach_real_inventory_and_the_existing_ui_image_trait() {
    let files = Files::new();
    files.write("ySI.json", &certain("Owned/matched.dds"));
    let f = fixture();
    let mut g = game(&f);
    g.world.add_item(FormId::PLAYER_REF, ITEM, 2).unwrap();
    let before = g.world.snapshot();
    assert_eq!(success(&mut g,"let item := OwnYsiItem\nlet icon := ySIGetTrait \"filename\" item\nSetUIString \"HUDMainMenu/OwnedIcon/filename\" (icon)\nSetFunctionValue (GetUIString \"HUDMainMenu/OwnedIcon/filename\")"),XVal::Str("Owned/matched.dds".into()));
    // Plain legacy string arguments keep their literal-token convention; grouped
    // values and explicit conversion pass the variable contents instead.
    assert_eq!(
        success(&mut g, "let icon := ySIGetTrait \"filename\" OwnYsiItem\nSetUIString \"HUDMainMenu/OwnedIcon/filename\" icon\nSetFunctionValue (GetUIString \"HUDMainMenu/OwnedIcon/filename\")"),
        XVal::Str("icon".into())
    );
    assert_eq!(
        success(&mut g, "let icon := ySIGetTrait \"filename\" OwnYsiItem\nSetUIStringAlt (\"HUDMainMenu/OwnedIcon/filename\") $icon\nSetFunctionValue (GetUIString \"HUDMainMenu/OwnedIcon/filename\")"),
        XVal::Str("Owned/matched.dds".into())
    );
    assert_eq!(success(&mut g,"let pair := ar_List OwnYsiItem\nSetFunctionValue (pair[0].ySIGetTrait \"filename\")"),XVal::Str("Owned/matched.dds".into()));
    assert_eq!(
        success(
            &mut g,
            "SetFunctionValue (OwnYsiPlaced.ySIGetTrait \"filename\")"
        ),
        XVal::Str("Owned/matched.dds".into())
    );
    assert_eq!(
        success(
            &mut g,
            "SetFunctionValue (GetUIString \"HUDMainMenu/ForeignIcon/filename\")"
        ),
        XVal::Str("Owned/foreign.dds".into())
    );
    assert_eq!(
        g.world.snapshot(),
        before,
        "getter and explicit UI assignment do not alter inventory/world"
    );
    assert_eq!(g.world.item_count(FormId::PLAYER_REF, ITEM).unwrap(), 2);
}

#[test]
fn multiple_files_use_certain_highest_priority_without_guessing_tie_order() {
    let files = Files::new();
    files.write("ySI.json", &certain("Owned/low.dds"));
    files.write(
        "extra.json",
        &config(
            serde_json::json!([{"priority":30,"formType":31,"tag":"Other"}]),
            serde_json::json!([{"priority":10,"tag":"Other","filename":"Owned/high.dds"}]),
        ),
    );
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(
        success(
            &mut g,
            "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"
        ),
        XVal::Str("Owned/high.dds".into())
    );
    files.write(
        "tie.json",
        &config(
            serde_json::json!([{"priority":30,"formType":31,"tag":"Conflict"}]),
            serde_json::json!([{"priority":10,"tag":"Conflict","filename":"Owned/conflict.dds"}]),
        ),
    );
    let mut fresh = game(&f);
    assert_eq!(
        success(
            &mut fresh,
            "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiItem)"
        ),
        XVal::Str(String::new())
    );
    assert_eq!(
        success(
            &mut fresh,
            "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"
        ),
        XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
    );
}

#[test]
fn unknown_possible_winners_refuse_but_decisively_false_or_lower_rules_do_not_win() {
    let files = Files::new();
    let f = fixture();
    for (priority, extra, expected) in [
        (30, serde_json::json!({"unknownPredicate":1}), false),
        (10, serde_json::json!({"unknownPredicate":1}), true),
        (
            30,
            serde_json::json!({"formType":40,"unknownWeaponPredicate":1}),
            true,
        ),
    ] {
        let mut rule = extra.as_object().unwrap().clone();
        rule.insert("priority".into(), priority.into());
        rule.insert("tag".into(), "Unknown".into());
        let mut value: serde_json::Value =
            serde_json::from_str(&certain("Owned/certain.dds")).unwrap();
        value["tags"].as_array_mut().unwrap().push(rule.into());
        files.write("ySI.json", &value.to_string());
        let mut g = game(&f);
        let result = success(
            &mut g,
            "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)",
        );
        if expected {
            assert_eq!(result, XVal::Str("Owned/certain.dds".into()));
        } else {
            assert_eq!(
                result,
                XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
            );
        }
    }
}

#[test]
fn invalid_receiver_non_item_and_missing_icon_never_abort_and_share_icon_filename_store() {
    let files = Files::new();
    files.write("ySI.json", &certain("Owned/icon.dds"));
    let f = fixture();
    let mut g = game(&f);
    let before = g.world.snapshot();
    // Empty dot-receiver still hits the VM's NoReference check before the host, whereas an
    // explicit null ref returns "" and non-item forms (Player) return Misc_Unknown.dds
    // without aborting [observed: w4e_03].
    assert!(query(&mut g, "SetFunctionValue (empty.ySIGetTrait \"filename\")").is_err());
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" empty)"),
        XVal::Str(String::new())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" Player)"),
        XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
    );
    assert_eq!(g.world.snapshot(), before);
    // ySISetTrait "icon" and ySIGetTrait "icon"/"filename" share the same tag-scoped store [observed: w4e_c01].
    success(
        &mut g,
        "ySISetTrait \"tag\" \"(owned)\" OwnYsiItem\nySISetTrait \"icon\" \"Owned/set.dds\" OwnYsiItem",
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiItem)"),
        XVal::Str("(owned)".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiItem)"),
        XVal::Str("Owned/set.dds".into())
    );
    assert_eq!(
        success(
            &mut g,
            "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"
        ),
        XVal::Str("Owned/set.dds".into())
    );
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([{"priority":1,"formType":31,"tag":"NoIcon"}]),
            serde_json::json!([]),
        ),
    );
    let mut fresh = game(&f);
    assert_eq!(
        success(
            &mut fresh,
            "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"
        ),
        XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
    );
}

#[test]
fn game_restore_reloads_the_config_snapshot_without_saving_provider_cache() {
    let files = Files::new();
    files.write("ySI.json", &certain("Owned/first.dds"));
    let f = fixture();
    let mut g = game(&f);
    let expression = "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)";
    assert_eq!(
        success(&mut g, expression),
        XVal::Str("Owned/first.dds".into())
    );
    g.world.add_item(FormId::PLAYER_REF, ITEM, 3).unwrap();
    let snapshot: GameSnapshot =
        serde_json::from_value(serde_json::to_value(g.snapshot()).unwrap()).unwrap();
    files.write("ySI.json", &certain("Owned/next.dds"));
    assert_eq!(
        success(&mut g, expression),
        XVal::Str("Owned/first.dds".into()),
        "mapped session snapshot stays stable"
    );
    let world = World::restore(&f.forms, &f.refs, &snapshot.sim).unwrap();
    let mut restored = Game::restore(
        &f.data,
        world,
        GameConfig {
            strict: true,
            ..Default::default()
        },
        &snapshot,
    )
    .unwrap();
    assert_eq!(
        success(&mut restored, expression),
        XVal::Str("Owned/next.dds".into())
    );
    assert_eq!(
        restored.world.item_count(FormId::PLAYER_REF, ITEM).unwrap(),
        3
    );
}

#[test]
fn local_comments_preserve_quoted_content_and_generic_json_stays_strict() {
    let files = Files::new();
    let value = certain("https://owned.invalid/a/*literal*/icon.dds");
    files.write(
        "ySI.json",
        &format!("\u{feff}/* own header */\n// own line\n{value}"),
    );
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(
        success(
            &mut g,
            "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"
        ),
        XVal::Str("https://owned.invalid/a/*literal*/icon.dds".into())
    );
    // Existing ReadFromJSON is a separate strict reader; this provider does not change it.
    assert!(query(
        &mut g,
        "SetFunctionValue (ReadFromJSON \"Data/menus/ySI/ySI.json\")"
    )
    .unwrap_err()
    .contains("ReadFromJSON: no JSON file Data/menus/ySI/ySI.json"));
    files.write("ySI.json", &value);
    assert!(
        matches!(
            success(
                &mut g,
                "SetFunctionValue (ReadFromJSON \"Data/menus/ySI/ySI.json\")"
            ),
            XVal::Array(_)
        ),
        "same existing JSON reader/path must successfully read plain JSON"
    );
}

#[test]
fn mapped_file_bound_and_malformed_configuration_refuse_explicitly() {
    let files = Files::new();
    files.write("ySI.json", &certain("Owned/base.dds"));
    let f = fixture();
    for n in 0..127 {
        files.write(&format!("owned-{n}.json"), "{}");
    }
    let mut at_bound = game(&f);
    assert_eq!(
        success(
            &mut at_bound,
            "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"
        ),
        XVal::Str("Owned/base.dds".into())
    );
    files.write("owned-127.json", "{}");
    let mut g = game(&f);
    assert_eq!(
        success(
            &mut g,
            "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"
        ),
        XVal::Str(String::new())
    );
    for n in 0..128 {
        std::fs::remove_file(
            files
                .root
                .join("Data/menus/ySI")
                .join(format!("owned-{n}.json")),
        )
        .unwrap();
    }
    files.write("ySI.json", "/* unfinished");
    let mut fresh = game(&f);
    assert_eq!(
        success(
            &mut fresh,
            "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"
        ),
        XVal::Str(String::new())
    );
}

#[test]
fn quest_item_filename_uses_authored_and_saved_world_flags_through_real_source_and_ui() {
    let files = Files::new();
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                {"priority":20,"formType":31,"questItem":0,"tag":"Ordinary"},
                {"priority":20,"formType":31,"questItem":1,"tag":"Quest"}
            ]),
            serde_json::json!([
                {"priority":1,"tag":"Ordinary","filename":"Owned/ordinary.dds"},
                {"priority":1,"tag":"Quest","filename":"Owned/quest.dds"}
            ]),
        ),
    );
    let assign = "let item := OwnYsiItem\nlet icon := ySIGetTrait \"filename\" item\nSetUIString \"HUDMainMenu/OwnedIcon/filename\" (icon)\nSetFunctionValue (GetUIString \"HUDMainMenu/OwnedIcon/filename\")";
    for flags in [0, 0x400] {
        let f = fixture_with_item_flags(flags);
        let mut g = game(&f);
        g.world.add_item(FormId::PLAYER_REF, ITEM, 2).unwrap();
        let initial = if flags == 0 {
            "Owned/ordinary.dds"
        } else {
            "Owned/quest.dds"
        };
        assert_eq!(success(&mut g, assign), XVal::Str(initial.into()));
        assert_eq!(g.world.is_quest_item(ITEM), flags != 0);
        success(&mut g, "SetQuestObject OwnYsiItem 1");
        assert!(g.world.is_quest_item(ITEM));
        assert_eq!(success(&mut g, assign), XVal::Str("Owned/quest.dds".into()));
        let saved: GameSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&g.checked_snapshot().unwrap()).unwrap())
                .unwrap();
        let world = World::restore(&f.forms, &f.refs, &saved.sim).unwrap();
        let mut back = Game::restore(&f.data, world, g.cfg, &saved).unwrap();
        assert_eq!(
            success(
                &mut back,
                "SetFunctionValue (OwnYsiPlaced.ySIGetTrait \"filename\")"
            ),
            XVal::Str("Owned/quest.dds".into())
        );
        assert_eq!(back.world.item_count(FormId::PLAYER_REF, ITEM).unwrap(), 2);
        success(&mut back, "SetQuestObject OwnYsiItem 0");
        assert!(
            !back.world.is_quest_item(ITEM),
            "an explicit clear overrides an authored flag too"
        );
        assert_eq!(
            success(
                &mut back,
                "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"
            ),
            XVal::Str("Owned/ordinary.dds".into())
        );
        assert_eq!(back.world.item_count(FormId::PLAYER_REF, ITEM).unwrap(), 2);
    }
}

#[test]
fn quest_item_false_rules_disqualify_unknowns_but_possible_or_malformed_winners_refuse() {
    let files = Files::new();
    let mut cfg: serde_json::Value = serde_json::from_str(&certain("Owned/certain.dds")).unwrap();
    cfg["tags"].as_array_mut().unwrap().push(
        serde_json::json!({"priority":30,"questItem":1,"unknownPredicate":1,"tag":"Unknown"}),
    );
    files.write("ySI.json", &cfg.to_string());
    let f = fixture();
    let mut g = game(&f);
    let ask = "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)";
    assert_eq!(success(&mut g, ask), XVal::Str("Owned/certain.dds".into()));
    success(&mut g, "SetQuestObject OwnYsiItem 1");
    assert_eq!(
        success(&mut g, ask),
        XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
    );
    for value in [
        serde_json::json!(2),
        serde_json::json!("true"),
        serde_json::json!([1]),
    ] {
        let mut cfg: serde_json::Value =
            serde_json::from_str(&certain("Owned/certain.dds")).unwrap();
        cfg["tags"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"priority":30,"questItem":value,"tag":"Unknown"}));
        files.write("ySI.json", &cfg.to_string());
        let mut fresh = game(&f);
        assert_eq!(
            success(&mut fresh, ask),
            XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
        );
    }
}

#[test]
fn aggregate_byte_limit_accepts_exact_valid_input_and_rejects_one_more_byte_without_side_effects() {
    let files = Files::new();
    let primary = certain("Owned/byte-limit.dds");
    // Both files are valid JSON. Only their aggregate size crosses the bound.
    let extra = format!("{{}}{}", " ".repeat(4 * 1024 * 1024 - primary.len() - 2));
    assert_eq!(primary.len() + extra.len(), 4 * 1024 * 1024);
    files.write("ySI.json", &primary);
    files.write("zz-extra.json", &extra);
    let f = fixture();
    let mut allowed = game(&f);
    allowed.world.add_item(FormId::PLAYER_REF, ITEM, 2).unwrap();
    let before = allowed.world.snapshot();
    let assign = "let icon := ySIGetTrait \"filename\" OwnYsiItem\nSetUIString \"HUDMainMenu/OwnedIcon/filename\" (icon)\nSetFunctionValue (GetUIString \"HUDMainMenu/OwnedIcon/filename\")";
    let read_only = "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)";
    assert_eq!(
        success(&mut allowed, assign),
        XVal::Str("Owned/byte-limit.dds".into())
    );
    assert_eq!(allowed.world.snapshot(), before);

    files.write("zz-extra.json", &format!("{extra} "));
    assert_eq!(primary.len() + extra.len() + 1, 4 * 1024 * 1024 + 1);
    let mut refused = game(&f);
    refused.world.add_item(FormId::PLAYER_REF, ITEM, 2).unwrap();
    let before = refused.world.snapshot();
    assert_eq!(success(&mut refused, read_only), XVal::Str(String::new()));
    assert_eq!(refused.world.snapshot(), before);
    assert_eq!(
        success(
            &mut refused,
            "SetFunctionValue (GetUIString \"HUDMainMenu/OwnedIcon/filename\")"
        ),
        XVal::Str("Owned/before.dds".into())
    );
    assert_eq!(
        success(
            &mut refused,
            "SetFunctionValue (GetUIString \"HUDMainMenu/ForeignIcon/filename\")"
        ),
        XVal::Str("Owned/foreign.dds".into())
    );
    // An already accepted snapshot is not invalidated by later file growth.
    assert_eq!(
        success(&mut allowed, assign),
        XVal::Str("Owned/byte-limit.dds".into())
    );
    // A rejected snapshot stays unloaded; only a fresh Game rereads corrected files.
    files.write("zz-extra.json", &extra);
    assert_eq!(success(&mut refused, read_only), XVal::Str(String::new()));
    let mut fresh = game(&f);
    assert_eq!(
        success(&mut fresh, assign),
        XVal::Str("Owned/byte-limit.dds".into())
    );
}

#[test]
fn aggregate_valid_rule_limit_reaches_the_final_winner_and_rejects_the_next_rule() {
    let files = Files::new();
    let irrelevant = serde_json::json!({"priority":1,"formType":40,"tag":"NoMatch"});
    let main_tags = vec![irrelevant.clone(); 9_999];
    let primary = config(
        main_tags.into(),
        serde_json::json!([{"priority":1,"tag":"FinalWinner","filename":"Owned/rule-limit.dds"}]),
    );
    let mut extra_tags = vec![irrelevant; 9_999];
    extra_tags.push(serde_json::json!({"priority":20,"mod":"OwnedYsi.esm","form":"00000820","tag":"FinalWinner"}));
    assert_eq!(9_999 + 1 + extra_tags.len(), 20_000);
    let extra = config(extra_tags.clone().into(), serde_json::json!([]));
    assert!(primary.len() + extra.len() < 4 * 1024 * 1024);
    files.write("ySI.json", &primary);
    files.write("zz-extra.json", &extra);
    let f = fixture();
    let mut allowed = game(&f);
    allowed.world.add_item(FormId::PLAYER_REF, ITEM, 3).unwrap();
    let before = allowed.world.snapshot();
    let assign = "let icon := ySIGetTrait \"filename\" OwnYsiItem\nSetUIStringAlt (\"HUDMainMenu/OwnedIcon/filename\") $icon\nSetFunctionValue (GetUIString \"HUDMainMenu/OwnedIcon/filename\")";
    let read_only = "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)";
    assert_eq!(
        success(&mut allowed, assign),
        XVal::Str("Owned/rule-limit.dds".into())
    );
    assert_eq!(allowed.world.snapshot(), before);
    // This extra entry is valid and matches the existing icon. Without the rule
    // cap it would succeed, so the negative control cannot fail on malformed input.
    extra_tags.push(serde_json::json!({"priority":21,"formType":31,"tag":"FinalWinner"}));
    assert_eq!(9_999 + 1 + extra_tags.len(), 20_001);
    let over = config(extra_tags.into(), serde_json::json!([]));
    assert!(primary.len() + over.len() < 4 * 1024 * 1024);
    files.write("zz-extra.json", &over);
    let mut refused = game(&f);
    refused.world.add_item(FormId::PLAYER_REF, ITEM, 3).unwrap();
    let before = refused.world.snapshot();
    assert_eq!(success(&mut refused, read_only), XVal::Str(String::new()));
    assert_eq!(refused.world.snapshot(), before);
    assert_eq!(
        success(
            &mut refused,
            "SetFunctionValue (GetUIString \"HUDMainMenu/OwnedIcon/filename\")"
        ),
        XVal::Str("Owned/before.dds".into())
    );
    assert_eq!(
        success(
            &mut refused,
            "SetFunctionValue (GetUIString \"HUDMainMenu/ForeignIcon/filename\")"
        ),
        XVal::Str("Owned/foreign.dds".into())
    );
    assert_eq!(
        success(&mut allowed, assign),
        XVal::Str("Owned/rule-limit.dds".into())
    );
    files.write("zz-extra.json", &extra);
    assert_eq!(success(&mut refused, read_only), XVal::Str(String::new()));
    let mut fresh = game(&f);
    assert_eq!(
        success(&mut fresh, assign),
        XVal::Str("Owned/rule-limit.dds".into())
    );
}

const ITEM_B: FormId = FormId(0x821);
const MOD_ITEM: FormId = FormId(0x822);
const ACTI_BASE: FormId = FormId(0x830);
const ACTI_PLACED: FormId = FormId(0x901);
const LIST_A: FormId = FormId(0x840);
const LIST_B: FormId = FormId(0x841);

fn extended_fixture() -> Fixture {
    let item_a = record(
        b"MISC",
        0,
        ITEM.0,
        &[
            sub(b"EDID", z("OwnYsiItem")),
            sub(b"DATA", [1i32.to_le_bytes(), 1f32.to_le_bytes()].concat()),
        ],
    );
    let item_b = record(
        b"MISC",
        0,
        ITEM_B.0,
        &[
            sub(b"EDID", z("OwnYsiItemB")),
            sub(b"DATA", [2i32.to_le_bytes(), 1f32.to_le_bytes()].concat()),
        ],
    );
    let mod_item = record(
        b"IMOD",
        0,
        MOD_ITEM.0,
        &[
            sub(b"EDID", z("OwnYsiMod")),
            sub(b"DATA", [5i32.to_le_bytes(), 1f32.to_le_bytes()].concat()),
        ],
    );
    let acti = record(b"ACTI", 0, ACTI_BASE.0, &[sub(b"EDID", z("OwnYsiActi"))]);
    let list_a = record(
        b"FLST",
        0,
        LIST_A.0,
        &[
            sub(b"EDID", z("OwnYsiListA")),
            sub(b"LNAM", ITEM.0.to_le_bytes().to_vec()),
        ],
    );
    let list_b = record(b"FLST", 0, LIST_B.0, &[sub(b"EDID", z("OwnYsiListB"))]);
    let placed_item = record(
        b"REFR",
        0,
        PLACED.0,
        &[
            sub(b"EDID", z("OwnYsiPlaced")),
            sub(b"NAME", ITEM.0.to_le_bytes().to_vec()),
            sub(b"DATA", vec![0; 24]),
        ],
    );
    let placed_acti = record(
        b"REFR",
        0,
        ACTI_PLACED.0,
        &[
            sub(b"EDID", z("OwnYsiActiPlaced")),
            sub(b"NAME", ACTI_BASE.0.to_le_bytes().to_vec()),
            sub(b"DATA", vec![0; 24]),
        ],
    );
    let room = record(b"CELL", 0, 0x200, &[sub(b"EDID", z("OwnYsiRoom"))]);
    let children = group(
        0x200u32.to_le_bytes(),
        6,
        &group(0x200u32.to_le_bytes(), 9, &[placed_item, placed_acti].concat()),
    );
    let bytes = [
        tes4(),
        top(b"MISC", &[item_a, item_b].concat()),
        top(b"IMOD", &mod_item),
        top(b"ACTI", &acti),
        top(b"FLST", &[list_a, list_b].concat()),
        top(
            b"NPC_",
            &record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))]),
        ),
        top(
            b"CELL",
            &group(
                0i32.to_le_bytes(),
                2,
                &group(0i32.to_le_bytes(), 3, &[room, children].concat()),
            ),
        ),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "OwnedYsi.esm".into()).unwrap()];
    let order = LoadOrder::new(&plugins).unwrap();
    let schema =
        Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/script/functions.json");
    let catalogue = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let forms = FormStore::build(&order, &schema);
    let refs = RefTable::build(&order, &schema);
    let data = GameData::build(
        &order,
        &schema,
        FunctionTable::load(&path).unwrap(),
        &catalogue,
        &refs,
    );
    Fixture { data, forms, refs }
}

#[test]
fn json_backed_tag_and_icon_traits_honor_overrides_tag_sharing_and_clearing() {
    let files = Files::new();
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                {"priority": 10, "formType": 31, "tag": "(shared)"},
                {"priority": 20, "mod": "OwnedYsi.esm", "form": "00000822", "tag": "(modtag)"}
            ]),
            serde_json::json!([
                {"priority": 10, "tag": "(shared)", "filename": "Owned/shared.dds"},
                {"priority": 10, "tag": "(alt)", "filename": "Owned/alt.dds"},
                {"priority": 10, "tag": "(modtag)", "filename": "Owned/mod.dds"}
            ]),
        ),
    );
    let f = extended_fixture();
    let mut g = game(&f);

    // JSON-backed "tag" and "icon" work for both explicit forms and placed references.
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiItem)"),
        XVal::Str("(shared)".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (OwnYsiPlaced.ySIGetTrait \"icon\")"),
        XVal::Str("Owned/shared.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiItemB)"),
        XVal::Str("Owned/shared.dds".into())
    );

    // Setting "icon" on one JSON-tagged form updates every form sharing that tag
    // on both "icon" and "filename" queries [observed: w4e_c01].
    success(&mut g, "ySISetTrait \"icon\" \"Owned/overridden.dds\" OwnYsiItem");
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiItem)"),
        XVal::Str("Owned/overridden.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiItemB)"),
        XVal::Str("Owned/overridden.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItemB)"),
        XVal::Str("Owned/overridden.dds".into())
    );

    // Reassigning a form's tag to another JSON tag immediately switches "icon" to that tag's JSON icon.
    success(&mut g, "ySISetTrait \"tag\" \"(alt)\" OwnYsiItem");
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiItem)"),
        XVal::Str("(alt)".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiItem)"),
        XVal::Str("Owned/alt.dds".into())
    );

    // Explicitly clearing a tag to "" suppresses the JSON tag without affecting other forms;
    // with ySI.json loaded, untagged "icon" falls back to Misc_Unknown.dds (see crates/fnv-game/src/nvse_small.rs and docs/spec-notes/ysi-filename.md).
    success(&mut g, "ySISetTrait \"tag\" \"\" OwnYsiItem");
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiItem)"),
        XVal::Str(String::new())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiItem)"),
        XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
    );
    success(&mut g, "ySISetTrait \"icon\" \"Owned/solo.dds\" OwnYsiItem");
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiItem)"),
        XVal::Str("Owned/solo.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiItemB)"),
        XVal::Str("Owned/overridden.dds".into())
    );

    // Missing ySI.json returns "" for "tag" and "icon" without errors.
    std::fs::remove_file(files.root.join("Data/menus/ySI/ySI.json")).unwrap();
    let mut no_cfg = game(&f);
    assert_eq!(
        success(&mut no_cfg, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiItem)"),
        XVal::Str(String::new())
    );
    assert_eq!(
        success(&mut no_cfg, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiItem)"),
        XVal::Str(String::new())
    );
}

#[test]
fn full_form_type_codes_and_non_item_tag_and_icon_queries_resolve() {
    let files = Files::new();
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                {"priority": 10, "formType": 103, "tag": "(imod)"},
                {"priority": 10, "formType": 21, "tag": "(acti)"}
            ]),
            serde_json::json!([
                {"priority": 10, "tag": "(imod)", "filename": "Owned/imod.dds"},
                {"priority": 10, "tag": "(acti)", "filename": "Owned/acti.dds"}
            ]),
        ),
    );
    let f = extended_fixture();
    let mut g = game(&f);

    // Full form-type codes cover IMOD (103) and ACTI (21) beyond the 10-item inventory subset.
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiMod)"),
        XVal::Str("(imod)".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiMod)"),
        XVal::Str("Owned/imod.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiMod)"),
        XVal::Str("Owned/imod.dds".into())
    );

    // Non-item forms (base or placed reference) resolve "tag", "icon", and "filename"
    // identically without aborting scripts [observed: w4e_03].
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiActi)"),
        XVal::Str("(acti)".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (OwnYsiActiPlaced.ySIGetTrait \"icon\")"),
        XVal::Str("Owned/acti.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiActi)"),
        XVal::Str("Owned/acti.dds".into())
    );
}

#[test]
fn formlist_predicate_supports_authored_and_live_flst_membership_and_refuses_unknown_modes() {
    let files = Files::new();
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                {"priority": 10, "formType": 31, "tag": "(base)"},
                {"priority": 20, "mod": "OwnedYsi.esm", "form": "00000840", "formlist": 1, "tag": "(listA)"},
                {"priority": 25, "mod": "OwnedYsi.esm", "form": ["00000841"], "formlist": 1, "tag": "(listB)"},
                {"priority": 15, "mod": "OwnedYsi.esm", "form": "00000821", "formlist": 0, "tag": "(directB)"}
            ]),
            serde_json::json!([
                {"priority": 10, "tag": "(base)", "filename": "Owned/base.dds"},
                {"priority": 10, "tag": "(listA)", "filename": "Owned/list-a.dds"},
                {"priority": 10, "tag": "(listB)", "filename": "Owned/list-b.dds"},
                {"priority": 10, "tag": "(directB)", "filename": "Owned/direct-b.dds"}
            ]),
        ),
    );
    let f = extended_fixture();
    let mut g = game(&f);

    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"),
        XVal::Str("Owned/list-a.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiItemB)"),
        XVal::Str("(directB)".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItemB)"),
        XVal::Str("Owned/direct-b.dds".into())
    );

    // Live FLST mutation takes effect on the next query without reloading files.
    success(&mut g, "AddFormToFormList OwnYsiListB OwnYsiItemB");
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiItemB)"),
        XVal::Str("(listB)".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiItemB)"),
        XVal::Str("Owned/list-b.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItemB)"),
        XVal::Str("Owned/list-b.dds".into())
    );

    // Unverified formlist mode (3) and non-FLST targets on mode 1 remain conservative Match::Unknown.
    for extra in [
        serde_json::json!({"priority": 30, "mod": "OwnedYsi.esm", "form": "00000840", "formlist": 3, "tag": "(mode3)"}),
        serde_json::json!({"priority": 30, "mod": "OwnedYsi.esm", "form": "00000820", "formlist": 1, "tag": "(notFlst)"}),
    ] {
        files.write(
            "ySI.json",
            &config(
                serde_json::json!([
                    {"priority": 10, "formType": 31, "tag": "(base)"},
                    extra
                ]),
                serde_json::json!([
                    {"priority": 10, "tag": "(base)", "filename": "Owned/base.dds"}
                ]),
            ),
        );
        let mut fresh = game(&f);
        assert_eq!(
            success(&mut fresh, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"),
            XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
        );
        assert_eq!(
            success(&mut fresh, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiItem)"),
            XVal::Str(String::new())
        );
    }
}

#[test]
fn empty_auxiliary_json_stubs_are_ignored_while_main_file_and_bad_comments_still_reject_book() {
    let files = Files::new();
    files.write("ySI.json", &certain("Owned/with-stubs.dds"));
    files.write("stub-empty.json", "");
    files.write("stub-comment-only.json", "// placeholder stub\n/* empty */\n");
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"),
        XVal::Str("Owned/with-stubs.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiItem)"),
        XVal::Str("OwnedTag".into())
    );

    // An unterminated comment in an auxiliary file rejects loading the book without aborting scripts.
    files.write("stub-bad.json", "/* unclosed");
    let mut bad = game(&f);
    assert_eq!(
        success(&mut bad, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"),
        XVal::Str(String::new())
    );
    std::fs::remove_file(files.root.join("Data/menus/ySI/stub-bad.json")).unwrap();

    // An empty main ySI.json rejects loading the book without aborting scripts.
    files.write("ySI.json", "");
    let mut empty_main = game(&f);
    assert_eq!(
        success(&mut empty_main, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"),
        XVal::Str(String::new())
    );
}

fn predicate_fixture() -> Fixture {
    // MGEF 0x860: restore Dehydration (AV 73, non-detrimental); MGEF 0x861: hostile damage Health (AV 16, detrimental+hostile).
    let mut mgef_water_data = vec![0u8; 72];
    mgef_water_data[64..68].copy_from_slice(&0i32.to_le_bytes()); // archetype 0 = Value Modifier
    mgef_water_data[68..72].copy_from_slice(&73i32.to_le_bytes()); // AV 73 = Dehydration
    let mgef_water = record(
        b"MGEF",
        0,
        0x860,
        &[sub(b"EDID", z("OwnYsiMgefWater")), sub(b"DATA", mgef_water_data)],
    );

    let mut mgef_poison_data = vec![0u8; 72];
    mgef_poison_data[0..4].copy_from_slice(&((0x0000_0001u32 | 0x0000_0004u32).to_le_bytes())); // Hostile | Detrimental
    mgef_poison_data[64..68].copy_from_slice(&0i32.to_le_bytes());
    mgef_poison_data[68..72].copy_from_slice(&16i32.to_le_bytes()); // AV 16 = Health
    let mgef_poison = record(
        b"MGEF",
        0,
        0x861,
        &[sub(b"EDID", z("OwnYsiMgefPoison")), sub(b"DATA", mgef_poison_data)],
    );

    // AMMO 0x862: DATA flags bit 0 = Ignores Normal Weapon Resistance.
    let mut ammo_data = vec![0u8; 13];
    ammo_data[0..4].copy_from_slice(&1.0f32.to_le_bytes());
    ammo_data[4] = 1; // ignores DT/DR
    ammo_data[8..12].copy_from_slice(&2i32.to_le_bytes());
    ammo_data[12] = 1;
    let ammo = record(
        b"AMMO",
        0,
        0x862,
        &[
            sub(b"EDID", z("OwnYsiAmmoAP")),
            sub(b"FULL", z("10mm AP Round")),
            sub(b"DATA", ammo_data),
        ],
    );

    // WEAP 0x863: clip_size=12, projectile_count=3, attack_animation=26, grip_animation=230, VNAM=2 (Silent), NAM0=0x862.
    let mut weap_data = vec![0u8; 15];
    weap_data[0..4].copy_from_slice(&100i32.to_le_bytes()); // value
    weap_data[4..8].copy_from_slice(&100i32.to_le_bytes()); // health
    weap_data[8..12].copy_from_slice(&2.5f32.to_le_bytes()); // weight
    weap_data[12..14].copy_from_slice(&20i16.to_le_bytes()); // base_damage
    weap_data[14] = 12; // clip_size
    let mut weap_dnam = vec![0u8; 207];
    weap_dnam[0..4].copy_from_slice(&3i32.to_le_bytes()); // animation_type
    weap_dnam[13] = 230; // grip_animation
    weap_dnam[41] = 26; // attack_animation
    weap_dnam[42] = 3; // projectile_count
    let weap = record(
        b"WEAP",
        0,
        0x863,
        &[
            sub(b"EDID", z("OwnYsiSilencedGun")),
            sub(b"FULL", z("Nuka-Pistol Custom")),
            sub(b"NAM0", 0x862u32.to_le_bytes().to_vec()),
            sub(b"VNAM", 2u32.to_le_bytes().to_vec()),
            sub(b"DATA", weap_data),
            sub(b"DNAM", weap_dnam),
        ],
    );

    // ALCH 0x864: water restoring Dehydration (73).
    let mut efit_water = vec![0u8; 20];
    efit_water[0..4].copy_from_slice(&25u32.to_le_bytes()); // magnitude
    efit_water[16..20].copy_from_slice(&73i32.to_le_bytes()); // actor_value
    let mut enit_food = vec![0u8; 20];
    enit_food[0..4].copy_from_slice(&5i32.to_le_bytes());
    enit_food[4] = 0x02; // Food
    let water = record(
        b"ALCH",
        0,
        0x864,
        &[
            sub(b"EDID", z("OwnYsiWater")),
            sub(b"FULL", z("Purified Spring Water")),
            sub(b"DATA", 1.0f32.to_le_bytes().to_vec()),
            sub(b"ENIT", enit_food),
            sub(b"EFID", 0x860u32.to_le_bytes().to_vec()),
            sub(b"EFIT", efit_water),
        ],
    );

    // ALCH 0x865: weapon poison (hostile+detrimental Health, neither Food nor Medicine).
    let mut efit_poison = vec![0u8; 20];
    efit_poison[0..4].copy_from_slice(&15u32.to_le_bytes());
    efit_poison[8..12].copy_from_slice(&5u32.to_le_bytes());
    efit_poison[16..20].copy_from_slice(&16i32.to_le_bytes());
    let mut enit_poison = vec![0u8; 20];
    enit_poison[0..4].copy_from_slice(&20i32.to_le_bytes());
    let poison = record(
        b"ALCH",
        0,
        0x865,
        &[
            sub(b"EDID", z("OwnYsiPoison")),
            sub(b"FULL", z("Viper Venom")),
            sub(b"DATA", 0.5f32.to_le_bytes().to_vec()),
            sub(b"ENIT", enit_poison),
            sub(b"EFID", 0x861u32.to_le_bytes().to_vec()),
            sub(b"EFIT", efit_poison),
        ],
    );

    // MISC 0x868 (component) and 0x869 (product), linked by RCPE 0x86A.
    let comp = record(
        b"MISC",
        0,
        0x868,
        &[
            sub(b"EDID", z("OwnYsiScrapComp")),
            sub(b"FULL", z("Scrap Coil")),
            sub(b"DATA", [5i32.to_le_bytes(), 1.0f32.to_le_bytes()].concat()),
        ],
    );
    let prod = record(
        b"MISC",
        0,
        0x869,
        &[
            sub(b"EDID", z("OwnYsiCraftedPart")),
            sub(b"FULL", z("Wound Inductor")),
            sub(b"DATA", [15i32.to_le_bytes(), 1.0f32.to_le_bytes()].concat()),
        ],
    );
    let mut rcpe_data = vec![0u8; 16];
    rcpe_data[0..4].copy_from_slice(&(-1i32).to_le_bytes());
    let rcpe = record(
        b"RCPE",
        0,
        0x86A,
        &[
            sub(b"EDID", z("OwnYsiRecipe")),
            sub(b"FULL", z("Make Inductor")),
            sub(b"DATA", rcpe_data),
            sub(b"RCIL", 0x868u32.to_le_bytes().to_vec()),
            sub(b"RCQY", 1u32.to_le_bytes().to_vec()),
            sub(b"RCOD", 0x869u32.to_le_bytes().to_vec()),
            sub(b"RCQY", 1u32.to_le_bytes().to_vec()),
        ],
    );

    let bytes = [
        tes4(),
        top(b"MGEF", &[mgef_water, mgef_poison].concat()),
        top(b"AMMO", &ammo),
        top(b"WEAP", &weap),
        top(b"ALCH", &[water, poison].concat()),
        top(b"MISC", &[comp, prod].concat()),
        top(b"RCPE", &rcpe),
        top(b"NPC_", &record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))])),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "OwnedYsi.esm".into()).unwrap()];
    let order = LoadOrder::new(&plugins).unwrap();
    let schema =
        Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/script/functions.json");
    let catalogue = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let forms = FormStore::build(&order, &schema);
    let refs = RefTable::build(&order, &schema);
    let data = GameData::build(
        &order,
        &schema,
        FunctionTable::load(&path).unwrap(),
        &catalogue,
        &refs,
    );
    Fixture { data, forms, refs }
}

#[test]
fn weapon_armor_and_aid_record_predicates_match_and_disqualify_cleanly() {
    const GUN: FormId = FormId(0x850);
    const HELMET: FormId = FormId(0x851);
    const POWER_SUIT: FormId = FormId(0x852);
    const FOOD: FormId = FormId(0x853);
    const MED: FormId = FormId(0x854);
    const WITHDRAWAL: FormId = FormId(0x855);

    let mut dnam = vec![0u8; 204];
    // WeaponDNAM (204 B): animation_type at 0..4 = 6 (Rifle - Automatic), flags1 at 12 = 0x07 (ignores DT/DR | auto | scope), skill at 104..108 = 41 (Guns).
    dnam[0..4].copy_from_slice(&6u32.to_le_bytes());
    dnam[12] = 0x07;
    dnam[104..108].copy_from_slice(&41u32.to_le_bytes());
    let weap_data = [100i32.to_le_bytes().as_slice(), &100i32.to_le_bytes(), &2.0f32.to_le_bytes(), &10i16.to_le_bytes(), &[12u8]].concat();
    let gun = record(b"WEAP", 0, GUN.0, &[sub(b"EDID", z("OwnYsiGun")), sub(b"DATA", weap_data), sub(b"DNAM", dnam)]);

    let make_armo = |id: FormId, edid: &str, slots: u32, general: u8| {
        let bmdt = [slots.to_le_bytes().as_slice(), &[general, 0, 0, 0]].concat();
        let data = [100i32.to_le_bytes().as_slice(), &100i32.to_le_bytes(), &5.0f32.to_le_bytes()].concat();
        let dnam = [0i16.to_le_bytes().as_slice(), &[0, 0], &10.0f32.to_le_bytes(), &[0, 0, 0, 0]].concat();
        record(b"ARMO", 0, id.0, &[sub(b"EDID", z(edid)), sub(b"BMDT", bmdt), sub(b"DATA", data), sub(b"DNAM", dnam)])
    };
    let helmet = make_armo(HELMET, "OwnYsiHelmet", 1 << 0, 0x08);
    let power_suit = make_armo(POWER_SUIT, "OwnYsiPowerSuit", 1 << 2, 0xA0);

    let make_alch = |id: FormId, edid: &str, flags: u8, withdrawal: u32| {
        let enit = [1u32.to_le_bytes().as_slice(), &[flags, 0, 0, 0], &withdrawal.to_le_bytes(), &0.5f32.to_le_bytes(), &0u32.to_le_bytes()].concat();
        record(b"ALCH", 0, id.0, &[sub(b"EDID", z(edid)), sub(b"ETYP", 10i32.to_le_bytes().to_vec()), sub(b"DATA", 0.5f32.to_le_bytes().to_vec()), sub(b"ENIT", enit)])
    };
    let food = make_alch(FOOD, "OwnYsiFood", 0x02, 0);
    let med = make_alch(MED, "OwnYsiMed", 0x04, WITHDRAWAL.0);
    let spel = record(b"SPEL", 0, WITHDRAWAL.0, &[sub(b"EDID", z("OwnYsiWithdrawal"))]);
    let misc = record(b"MISC", 0, ITEM.0, &[sub(b"EDID", z("OwnYsiItem")), sub(b"DATA", [1i32.to_le_bytes(), 1f32.to_le_bytes()].concat())]);
    let room = record(b"CELL", 0, 0x200, &[sub(b"EDID", z("OwnYsiRoom"))]);
    let bytes = [
        tes4(),
        top(b"MISC", &misc),
        top(b"WEAP", &gun),
        top(b"ARMO", &[helmet, power_suit].concat()),
        top(b"ALCH", &[food, med].concat()),
        top(b"SPEL", &spel),
        top(b"NPC_", &record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))])),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &room))),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "OwnedYsi.esm".into()).unwrap()];
    let order = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let catalogue = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let forms = FormStore::build(&order, &schema);
    let refs = RefTable::build(&order, &schema);
    let data = GameData::build(&order, &schema, FunctionTable::load(&path).unwrap(), &catalogue, &refs);
    let f = Fixture { data, forms, refs };

    let files = Files::new();
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                {"priority": 10, "formType": 31, "tag": "MiscBase"},
                {"priority": 20, "weaponSkill": 41, "weaponType": [5, 6], "weaponIsAutomatic": 1, "weaponHasScope": 1, "weaponIgnoresDTDR": 1, "tag": "AutoScopedGun"},
                {"priority": 20, "armorClass": 2, "armorHead": 1, "armorHat": -1, "armorMask": -1, "armorPower": 0, "tag": "MedHelmet"},
                {"priority": 20, "armorClass": 3, "armorUpperBody": 1, "armorPower": 1, "tag": "PowerArmor"},
                {"priority": 20, "aidIsFood": 1, "aidIsMedicine": 0, "aidIsAddictive": 0, "tag": "FoodItem"},
                {"priority": 20, "aidIsMedicine": 1, "aidIsAddictive": 1, "tag": "AddictiveMed"},
                // High-priority rule with unknown predicate is decisively disqualified on non-WEAP forms via weaponSkill.
                {"priority": 50, "weaponSkill": 34, "unknownPredicate": 1, "tag": "EnergyUnknown"}
            ]),
            serde_json::json!([
                {"priority": 10, "tag": "MiscBase", "filename": "Owned/misc.dds"},
                {"priority": 10, "tag": "AutoScopedGun", "filename": "Owned/auto-gun.dds"},
                {"priority": 10, "tag": "MedHelmet", "filename": "Owned/helmet.dds"},
                {"priority": 10, "tag": "PowerArmor", "filename": "Owned/power.dds"},
                {"priority": 10, "tag": "FoodItem", "filename": "Owned/food.dds"},
                {"priority": 10, "tag": "AddictiveMed", "filename": "Owned/med.dds"}
            ]),
        ),
    );
    let mut g = game(&f);

    // Non-WEAP forms disqualify the priority-50 EnergyUnknown rule instead of refusing on unknownPredicate.
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"), XVal::Str("Owned/misc.dds".into()));
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiGun)"), XVal::Str("Owned/auto-gun.dds".into()));
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiHelmet)"), XVal::Str("Owned/helmet.dds".into()));
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiPowerSuit)"), XVal::Str("Owned/power.dds".into()));
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiFood)"), XVal::Str("Owned/food.dds".into()));
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiMed)"), XVal::Str("Owned/med.dds".into()));

    // Malformed boolean flag values, -1 on non-slot flags, and array weaponSkill stay conservative Match::Unknown and refuse.
    for (target, bad_rule) in [
        ("OwnYsiHelmet", serde_json::json!({"priority": 30, "armorHead": 2, "tag": "BadFlag"})),
        ("OwnYsiHelmet", serde_json::json!({"priority": 30, "armorPower": -1, "tag": "BadNegPower"})),
        ("OwnYsiGun", serde_json::json!({"priority": 30, "weaponSkill": [41, 35], "tag": "BadSkillArr"})),
        ("OwnYsiGun", serde_json::json!({"priority": 30, "weaponHasScope": -1, "tag": "BadNegScope"})),
        ("OwnYsiFood", serde_json::json!({"priority": 30, "aidIsFood": -1, "tag": "BadNegFood"})),
    ] {
        files.write(
            "ySI.json",
            &config(
                serde_json::json!([
                    {"priority": 10, "armorClass": 2, "tag": "MedHelmet"},
                    {"priority": 10, "weaponSkill": 41, "tag": "GunsBase"},
                    {"priority": 10, "aidIsFood": 1, "tag": "FoodItem"},
                    bad_rule
                ]),
                serde_json::json!([
                    {"priority": 10, "tag": "MedHelmet", "filename": "Owned/helmet.dds"},
                    {"priority": 10, "tag": "GunsBase", "filename": "Owned/auto-gun.dds"},
                    {"priority": 10, "tag": "FoodItem", "filename": "Owned/food.dds"}
                ]),
            ),
        );
        let mut bad = game(&f);
        assert_eq!(
            success(&mut bad, &format!("SetFunctionValue (ySIGetTrait \"filename\" {target})")),
            XVal::Str("interface\\icons\\Misc_Unknown.dds".into()),
            "{target} must decline uncertain winner without aborting"
        );
    }

    // Non-existent shorthand key names (`weaponAutomatic` / `weaponScope`) stay Match::Unknown
    // rather than aliasing `weaponIsAutomatic` / `weaponHasScope`.
    for unknown_key in ["weaponAutomatic", "weaponScope"] {
        files.write(
            "ySI.json",
            &config(
                serde_json::json!([
                    {"priority": 10, "weaponSkill": 41, "tag": "GunsBase"},
                    {"priority": 30, unknown_key: 1, "tag": "UnknownShorthand"}
                ]),
                serde_json::json!([{"priority": 10, "tag": "GunsBase", "filename": "Owned/auto-gun.dds"}]),
            ),
        );
        let mut g_unknown = game(&f);
        assert_eq!(
            success(&mut g_unknown, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiGun)"),
            XVal::Str("interface\\icons\\Misc_Unknown.dds".into()),
            "{unknown_key} must stay Match::Unknown"
        );
    }

    // Explicit `form` selector (`formlist: 0`) ignores non-applicable record-type predicates
    // on mismatched record types (e.g. weaponSkill on ALCH, aidIsFood on MISC; see crates/fnv-game/src/ysi_filename.rs),
    // while non-explicit-form rules still disqualify mismatched record types.
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                {"priority": 10, "formType": 31, "tag": "MiscBase"},
                {"priority": 30, "mod": "OwnedYsi.esm", "form": "00000853", "weaponSkill": 41, "tag": "(zbadw)"},
                {"priority": 30, "mod": "OwnedYsi.esm", "form": "00000820", "aidIsFood": 0, "tag": "(zbadf)"}
            ]),
            serde_json::json!([
                {"priority": 10, "tag": "(zbadw)", "filename": "Owned/zbadw.dds"},
                {"priority": 10, "tag": "(zbadf)", "filename": "Owned/zbadf.dds"}
            ]),
        ),
    );
    let mut g_mismatch = game(&f);
    assert_eq!(
        success(&mut g_mismatch, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiFood)"),
        XVal::Str("(zbadw)".into())
    );
    assert_eq!(
        success(&mut g_mismatch, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiFood)"),
        XVal::Str("Owned/zbadw.dds".into())
    );
    assert_eq!(
        success(&mut g_mismatch, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiItem)"),
        XVal::Str("(zbadf)".into())
    );
    assert_eq!(
        success(&mut g_mismatch, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"),
        XVal::Str("Owned/zbadf.dds".into())
    );
}

#[test]
fn orthogonal_record_recipe_and_template_predicates_resolve_and_disqualify() {
    let files = Files::new();
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                // weaponClipRounds (7 <= 12) and weaponNumProjectiles (2 <= 3) evaluate as >= N thresholds [observed: yUI 1.4d].
                {"priority": 10, "weaponClipRounds": 7, "weaponNumProjectiles": 2, "weaponAttackAnim": 26, "weaponHandgrip": 230, "weaponSoundLevel": 2, "ammoMod": "OwnedYsi.esm", "ammoForm": "00000862", "tag": "(w10)"},
                // Higher threshold rules disqualify cleanly (13 > 12, 4 > 3).
                {"priority": 40, "weaponClipRounds": 13, "tag": "(tooBigClip)"},
                {"priority": 40, "weaponNumProjectiles": 4, "tag": "(tooManyProj)"},
                {"priority": 20, "aidIsWater": 1, "aidRestoresAV": 73, "tag": "(h2o)"},
                {"priority": 20, "aidIsPoisonous": 1, "aidDamagesAV": 16, "tag": "(psn)"},
                {"priority": 25, "miscComponent": 1, "miscProduct": 0, "tag": "(mc1)"},
                {"priority": 25, "miscComponent": 0, "miscProduct": 1, "tag": "(mp1)"}
            ]),
            serde_json::json!([
                {"priority": 10, "tag": "(w10)", "filename": "Owned/gun.dds", "template": "ySICustomGun"},
                {"priority": 10, "tag": "(h2o)", "filename": "Owned/water.dds"},
                {"priority": 10, "tag": "(psn)", "filename": "Owned/poison.dds"},
                {"priority": 10, "tag": "(mc1)", "filename": "Owned/comp.dds"},
                {"priority": 10, "tag": "(mp1)", "filename": "Owned/prod.dds"}
            ]),
        ),
    );
    let f = predicate_fixture();
    let mut g = game(&f);

    // Every documented orthogonal predicate family resolves its item and disqualifies mismatched types cleanly.
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiSilencedGun)"), XVal::Str("Owned/gun.dds".into()));
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiWater)"), XVal::Str("Owned/water.dds".into()));
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiPoison)"), XVal::Str("Owned/poison.dds".into()));
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiScrapComp)"), XVal::Str("Owned/comp.dds".into()));
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiCraftedPart)"), XVal::Str("Owned/prod.dds".into()));

    // Game::ysi_item_display surfaces custom template + icon + tag and honors ySISetTrait overrides.
    assert!(g.ysi_active());
    let disp = g.ysi_item_display(FormId(0x863)).unwrap();
    assert_eq!(disp.tag.as_deref(), Some("(w10)"));
    assert_eq!(disp.icon.as_deref(), Some("Owned/gun.dds"));
    assert_eq!(disp.template, "ySICustomGun");

    let water_disp = g.ysi_item_display(FormId(0x864)).unwrap();
    assert_eq!(water_disp.template, fnv_game::ui_ysi::YSI_DEFAULT_TEMPLATE);

    // Dangling ammoMod without ammoForm refuses conservatively.
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                {"priority": 10, "formType": 40, "tag": "(w10)"},
                {"priority": 50, "ammoMod": "OwnedYsi.esm", "tag": "(bad)"}
            ]),
            serde_json::json!([{"priority": 10, "tag": "(w10)", "filename": "Owned/gun.dds"}]),
        ),
    );
    let mut fresh = game(&f);
    assert_eq!(
        success(&mut fresh, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiSilencedGun)"),
        XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
    );

    // Uncited keys absent from Sorting Icons 1.4b (and observed ignored by yUI 1.4d) stay Match::Unknown.
    for (unknown_key, val) in [
        ("bookSkill", serde_json::json!(35)),
        ("isSkillBook", serde_json::json!(1)),
        ("noteType", serde_json::json!(3)),
        ("ammoIgnoresDTDR", serde_json::json!(1)),
        ("prefix", serde_json::json!("Nuka-")),
        ("suffix", serde_json::json!("Custom")),
        ("edid", serde_json::json!("OwnYsiSilencedGun")),
        ("edidPrefix", serde_json::json!("OwnYsi")),
    ] {
        files.write(
            "ySI.json",
            &config(
                serde_json::json!([
                    {"priority": 10, "formType": 40, "tag": "(w10)"},
                    {"priority": 50, unknown_key: val, "tag": "(unknown)"}
                ]),
                serde_json::json!([{"priority": 10, "tag": "(w10)", "filename": "Owned/gun.dds"}]),
            ),
        );
        let mut g_unknown = game(&f);
        assert_eq!(
            success(&mut g_unknown, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiSilencedGun)"),
            XVal::Str("interface\\icons\\Misc_Unknown.dds".into()),
            "{unknown_key} must stay Match::Unknown"
        );
    }
}

fn armor_and_mode2_fixture() -> Fixture {
    // FLST 0x870: repair list (empty LNAM, referenced via REPL by WEAP 0x873).
    let repair_flst = record(b"FLST", 0, 0x870, &[sub(b"EDID", z("OwnYsiRepairList"))]);
    // FLST 0x871 (outer) -> FLST 0x872 (inner) -> WEAP 0x874 (nested member); also adds cyclic back-edge 0x871.
    let outer_flst = record(
        b"FLST",
        0,
        0x871,
        &[
            sub(b"EDID", z("OwnYsiOuterList")),
            sub(b"LNAM", 0x872u32.to_le_bytes().to_vec()),
        ],
    );
    let inner_flst = record(
        b"FLST",
        0,
        0x872,
        &[
            sub(b"EDID", z("OwnYsiInnerList")),
            sub(b"LNAM", 0x871u32.to_le_bytes().to_vec()),
            sub(b"LNAM", 0x874u32.to_le_bytes().to_vec()),
        ],
    );

    let weap_data = vec![0u8; 15];
    let weap_dnam = vec![0u8; 207];
    let weap_repl = record(
        b"WEAP",
        0,
        0x873,
        &[
            sub(b"EDID", z("OwnYsiReplGun")),
            sub(b"FULL", z("Scoped Carbine")),
            sub(b"REPL", 0x870u32.to_le_bytes().to_vec()),
            sub(b"DATA", weap_data.clone()),
            sub(b"DNAM", weap_dnam.clone()),
        ],
    );
    let weap_nested = record(
        b"WEAP",
        0,
        0x874,
        &[
            sub(b"EDID", z("OwnYsiNestedGun")),
            sub(b"FULL", z("Nested Rifle")),
            sub(b"DATA", weap_data.clone()),
            sub(b"DNAM", weap_dnam.clone()),
        ],
    );
    let weap_direct = record(
        b"WEAP",
        0,
        0x875,
        &[
            sub(b"EDID", z("OwnYsiDirectGun")),
            sub(b"FULL", z("Direct Rifle")),
            sub(b"DATA", weap_data),
            sub(b"DNAM", weap_dnam),
        ],
    );

    // ARMO 0x876: DT = 6.0 (passes armorDT: 5); ARMO 0x877: DT = 1.0 (fails armorDT: 5).
    let mut armo_data = vec![0u8; 12];
    armo_data[0..4].copy_from_slice(&50i32.to_le_bytes()); // value
    armo_data[4..8].copy_from_slice(&100i32.to_le_bytes()); // health
    armo_data[8..12].copy_from_slice(&10.0f32.to_le_bytes()); // weight
    let mut dnam_hi = vec![0u8; 12];
    dnam_hi[4..8].copy_from_slice(&6.0f32.to_le_bytes()); // DT = 6.0
    let mut dnam_lo = vec![0u8; 12];
    dnam_lo[4..8].copy_from_slice(&1.0f32.to_le_bytes()); // DT = 1.0
    let armo_hi = record(
        b"ARMO",
        0,
        0x876,
        &[
            sub(b"EDID", z("OwnYsiLightArmor")),
            sub(b"FULL", z("Reinforced Jacket")),
            sub(b"DATA", armo_data.clone()),
            sub(b"DNAM", dnam_hi),
        ],
    );
    let armo_lo = record(
        b"ARMO",
        0,
        0x877,
        &[
            sub(b"EDID", z("OwnYsiCloth")),
            sub(b"FULL", z("Plain Shirt")),
            sub(b"DATA", armo_data),
            sub(b"DNAM", dnam_lo),
        ],
    );

    let bytes = [
        tes4(),
        top(b"FLST", &[repair_flst, outer_flst, inner_flst].concat()),
        top(b"WEAP", &[weap_repl, weap_nested, weap_direct].concat()),
        top(b"ARMO", &[armo_hi, armo_lo].concat()),
        top(b"NPC_", &record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))])),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "OwnedYsi.esm".into()).unwrap()];
    let order = LoadOrder::new(&plugins).unwrap();
    let schema =
        Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/script/functions.json");
    let catalogue = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let forms = FormStore::build(&order, &schema);
    let refs = RefTable::build(&order, &schema);
    let data = GameData::build(
        &order,
        &schema,
        FunctionTable::load(&path).unwrap(),
        &catalogue,
        &refs,
    );
    Fixture { data, forms, refs }
}

#[test]
fn formlist_mode2_and_armordt_predicates_evaluate_repair_lists_nested_flsts_and_live_overrides() {
    let files = Files::new();
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                {"priority": 10, "formType": 40, "tag": "(w00)"},
                {"priority": 20, "mod": "OwnedYsi.esm", "form": "00000870", "formlist": 2, "tag": "(wrepl)"},
                {"priority": 20, "mod": "OwnedYsi.esm", "form": ["00000871", "00000875"], "formlist": 2, "tag": "(wnest)"},
                {"priority": 10, "formType": 24, "tag": "(cloth)"},
                {"priority": 20, "formType": 24, "armorDT": 5, "tag": "(armor5)"}
            ]),
            serde_json::json!([
                {"priority": 10, "tag": "(w00)", "filename": "Owned/w00.dds"},
                {"priority": 10, "tag": "(wrepl)", "filename": "Owned/wrepl.dds"},
                {"priority": 10, "tag": "(wnest)", "filename": "Owned/wnest.dds"},
                {"priority": 10, "tag": "(cloth)", "filename": "Owned/cloth.dds"},
                {"priority": 10, "tag": "(armor5)", "filename": "Owned/armor5.dds"}
            ]),
        ),
    );
    let f = armor_and_mode2_fixture();
    let mut g = game(&f);

    // formlist: 2 matches REPL back-reference, cyclic nested FLST membership, and mixed direct item.
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiReplGun)"), XVal::Str("Owned/wrepl.dds".into()));
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiNestedGun)"), XVal::Str("Owned/wnest.dds".into()));
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiDirectGun)"), XVal::Str("Owned/wnest.dds".into()));

    // Live SetRepairList override (`SetRepairList list item`) promotes a weapon into the formlist: 2 match.
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                {"priority": 10, "formType": 40, "tag": "(w00)"},
                {"priority": 30, "mod": "OwnedYsi.esm", "form": "00000870", "formlist": 2, "tag": "(wrepl)"}
            ]),
            serde_json::json!([
                {"priority": 10, "tag": "(w00)", "filename": "Owned/w00.dds"},
                {"priority": 10, "tag": "(wrepl)", "filename": "Owned/wrepl.dds"}
            ]),
        ),
    );
    let mut g2 = game(&f);
    assert_eq!(success(&mut g2, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiDirectGun)"), XVal::Str("Owned/w00.dds".into()));
    success(&mut g2, "SetRepairList OwnYsiRepairList OwnYsiDirectGun");
    assert_eq!(success(&mut g2, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiDirectGun)"), XVal::Str("Owned/wrepl.dds".into()));

    // armorDT threshold classifies ARMO >= 5 vs < 5, disqualifies WEAP cleanly, and honors SetArmorDT.
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiLightArmor)"), XVal::Str("Owned/armor5.dds".into()));
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiCloth)"), XVal::Str("Owned/cloth.dds".into()));
    success(&mut g, "SetArmorDT 7 OwnYsiCloth");
    assert_eq!(success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiCloth)"), XVal::Str("Owned/armor5.dds".into()));

    // Negative or non-numeric armorDT stays Match::Unknown [design], returning Misc_Unknown.dds
    // without aborting scripts.
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                {"priority": 10, "formType": 24, "tag": "(cloth)"},
                {"priority": 30, "formType": 24, "armorDT": -2, "tag": "(neg)"}
            ]),
            serde_json::json!([{"priority": 10, "tag": "(cloth)", "filename": "Owned/cloth.dds"}]),
        ),
    );
    let mut neg = game(&f);
    assert_eq!(
        success(&mut neg, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiLightArmor)"),
        XVal::Str(String::new())
    );
    assert_eq!(
        success(&mut neg, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiLightArmor)"),
        XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
    );

    // armorDT=0 on a WEAP with an explicit `form` selector ignores the non-applicable
    // ARMO predicate and matches cleanly, whereas armorDT=0 without an explicit `form`
    // selector still disqualifies WEAP.
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                {"priority": 10, "formType": 40, "tag": "(w00)"},
                {"priority": 25, "armorDT": 0, "tag": "(armoOnly)"},
                {"priority": 30, "mod": "OwnedYsi.esm", "form": "00000875", "armorDT": 0, "tag": "(zbada)"}
            ]),
            serde_json::json!([
                {"priority": 10, "tag": "(w00)", "filename": "Owned/w00.dds"},
                {"priority": 10, "tag": "(armoOnly)", "filename": "Owned/armo.dds"},
                {"priority": 10, "tag": "(zbada)", "filename": "Owned/zbada.dds"}
            ]),
        ),
    );
    let mut g_weap_dt = game(&f);
    assert_eq!(
        success(&mut g_weap_dt, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiDirectGun)"),
        XVal::Str("(zbada)".into())
    );
    assert_eq!(
        success(&mut g_weap_dt, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiDirectGun)"),
        XVal::Str("Owned/zbada.dds".into())
    );
    // Sibling WEAP without explicit form match is NOT caught by the non-explicit armorDT=0 rule.
    assert_eq!(
        success(&mut g_weap_dt, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiNestedGun)"),
        XVal::Str("(w00)".into())
    );
}

#[test]
fn ysi_get_and_set_trait_match_observed_yui140_string_metadata_fallback_and_worldref() {
    let files = Files::new();
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                {"priority": 20, "mod": "OwnedYsi.esm", "form": "00000873", "tag": "(w01)"},
                {"priority": 20, "mod": "OwnedYsi.esm", "form": "00000874", "tag": "(w01)"},
                {"priority": 25, "formType": 40, "weaponIsAutomatic": false, "tag": "(nonauto)"}
            ]),
            serde_json::json!([
                {
                    "priority": 10,
                    "tag": "(w01)",
                    "filename": "Owned/carbine.dds",
                    "template": "ySICustom",
                    "systemcolor": -1,
                    "category": "(catW)",
                    "name": "Weapons",
                    "tab": 0,
                    "count": 2
                }
            ]),
        ),
    );
    let f = armor_and_mode2_fixture();
    let mut g = game(&f);

    // "string" returns only the raw sorting tag (not "{tag} {name}"; see crates/fnv-game/src/nvse_small.rs and docs/spec-notes/ysi-filename.md).
    // Priority-25 weaponIsAutomatic=false (JSON bool; see crates/fnv-game/src/ysi_filename.rs `bool_pred` and docs/spec-notes/ysi-filename.md) beats priority-20 (w01)
    // for non-auto weapons; verify JSON bool matching first, then restore clean (w01) config.
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiDirectGun)"),
        XVal::Str("(nonauto)".into())
    );
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([
                {"priority": 20, "mod": "OwnedYsi.esm", "form": "00000873", "tag": "(w01)"},
                {"priority": 20, "mod": "OwnedYsi.esm", "form": "00000874", "tag": "(w01)"}
            ]),
            serde_json::json!([
                {
                    "priority": 10,
                    "tag": "(w01)",
                    "filename": "Owned/carbine.dds",
                    "template": "ySICustom",
                    "systemcolor": -1,
                    "category": "(catW)",
                    "name": "Weapons",
                    "tab": 0,
                    "count": 2
                }
            ]),
        ),
    );
    let mut g = game(&f);

    // "string" is an exact alias for "tag" on both get and set (leaving siblings unchanged; see crates/fnv-game/src/nvse_small.rs and docs/spec-notes/ysi-filename.md).
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiReplGun)"),
        XVal::Str("(w01)".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"string\" OwnYsiReplGun)"),
        XVal::Str("(w01)".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"string\" OwnYsiDirectGun)"),
        XVal::Str(String::new())
    );

    // All 6 metadata names return "" on get and 0 (no-op) on set (see crates/fnv-game/src/nvse_small.rs and docs/spec-notes/ysi-filename.md).
    for meta in ["template", "systemcolor", "category", "name", "tab", "count"] {
        assert_eq!(
            success(&mut g, &format!("SetFunctionValue (ySIGetTrait \"{meta}\" OwnYsiReplGun)")),
            XVal::Str(String::new()),
            "ySIGetTrait \"{meta}\" must return empty string"
        );
        assert_eq!(
            success(&mut g, &format!("SetFunctionValue (ySISetTrait \"{meta}\" \"ignored\" OwnYsiReplGun)")),
            XVal::Num(0.0),
            "ySISetTrait \"{meta}\" must return 0 (rejected no-op)"
        );
        assert_eq!(
            success(&mut g, &format!("SetFunctionValue (ySIGetTrait \"{meta}\" OwnYsiReplGun)")),
            XVal::Str(String::new())
        );
    }
    // JSON icon rule's template still flows to Game::ysi_item_display (untouched by rejected set).
    assert_eq!(g.ysi_item_display(FormId(0x874)).unwrap().template, "ySICustom");

    // Unmatched/untagged and non-item forms return Misc_Unknown.dds on BOTH "icon" and
    // "filename" when ySI.json is loaded without aborting [observed: w4e_03].
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiDirectGun)"),
        XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiDirectGun)"),
        XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" Player)"),
        XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" Player)"),
        XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
    );

    // ySISetTrait "icon" and "filename" share the tag-scoped store and return 1, visible
    // through BOTH ySIGetTrait "icon" and ySIGetTrait "filename" [observed: w4e_c01].
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySISetTrait \"icon\" \"Owned/sib_icon.dds\" OwnYsiReplGun)"),
        XVal::Num(1.0)
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiNestedGun)"),
        XVal::Str("Owned/sib_icon.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiNestedGun)"),
        XVal::Str("Owned/sib_icon.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySISetTrait \"filename\" \"Owned/sib_file.dds\" OwnYsiReplGun)"),
        XVal::Num(1.0)
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiNestedGun)"),
        XVal::Str("Owned/sib_file.dds".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiNestedGun)"),
        XVal::Str("Owned/sib_file.dds".into())
    );

    // ySISetTrait "string" mutates both "tag" and "string" per-form (leaving siblings),
    // and ySISetTrait "tag" likewise updates "string" [observed: w4e_01].
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySISetTrait \"string\" \"(zstr)\" OwnYsiReplGun)"),
        XVal::Num(1.0)
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiReplGun)"),
        XVal::Str("(zstr)".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"string\" OwnYsiReplGun)"),
        XVal::Str("(zstr)".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiNestedGun)"),
        XVal::Str("(w01)".into())
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySISetTrait \"tag\" \"(w4et)\" OwnYsiReplGun)"),
        XVal::Num(1.0)
    );
    assert_eq!(
        success(&mut g, "SetFunctionValue (ySIGetTrait \"string\" OwnYsiReplGun)"),
        XVal::Str("(w4et)".into())
    );

    // Dot-receiver unwraps placed world ref to base form, whereas explicit 2nd-arg world
    // ref does NOT unwrap on tag, icon, OR filename [observed: w4e_04].
    let ext = extended_fixture();
    files.write(
        "ySI.json",
        &config(
            serde_json::json!([{"priority": 20, "mod": "OwnedYsi.esm", "form": "00000820", "tag": "(fba1)"}]),
            serde_json::json!([{"priority": 10, "tag": "(fba1)", "filename": "Owned/pistol.dds"}]),
        ),
    );
    let mut g_ref = game(&ext);
    assert_eq!(
        success(&mut g_ref, "SetFunctionValue (OwnYsiPlaced.ySIGetTrait \"tag\")"),
        XVal::Str("(fba1)".into())
    );
    assert_eq!(
        success(&mut g_ref, "SetFunctionValue (OwnYsiPlaced.ySIGetTrait \"icon\")"),
        XVal::Str("Owned/pistol.dds".into())
    );
    assert_eq!(
        success(&mut g_ref, "SetFunctionValue (OwnYsiPlaced.ySIGetTrait \"filename\")"),
        XVal::Str("Owned/pistol.dds".into())
    );
    assert_eq!(
        success(&mut g_ref, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiPlaced)"),
        XVal::Str(String::new())
    );
    assert_eq!(
        success(&mut g_ref, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiPlaced)"),
        XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
    );
    assert_eq!(
        success(&mut g_ref, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiPlaced)"),
        XVal::Str("interface\\icons\\Misc_Unknown.dds".into())
    );

    // w4c_b4_15b_ysi_no_json_fallback: when NO ySI JSON packs exist, unmatched "icon" and "filename" stay "".
    std::fs::remove_file(files.root.join("Data/menus/ySI/ySI.json")).unwrap();
    let mut g_no_json = game(&ext);
    assert_eq!(
        success(&mut g_no_json, "SetFunctionValue (ySIGetTrait \"tag\" OwnYsiItem)"),
        XVal::Str(String::new())
    );
    assert_eq!(
        success(&mut g_no_json, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiItem)"),
        XVal::Str(String::new())
    );
    assert_eq!(
        success(&mut g_no_json, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiItem)"),
        XVal::Str(String::new())
    );
    assert_eq!(
        success(&mut g_no_json, "SetFunctionValue (ySIGetTrait \"icon\" OwnYsiPlaced)"),
        XVal::Str(String::new())
    );
    assert_eq!(
        success(&mut g_no_json, "SetFunctionValue (ySIGetTrait \"filename\" OwnYsiPlaced)"),
        XVal::Str(String::new())
    );

    // Honest yUI version gate remains unadvertised until full yUI 140 parity.
    assert_eq!(success(&mut g, "SetFunctionValue (IsPluginInstalled \"yUI\")"), XVal::Num(0.0));
    assert_eq!(success(&mut g, "SetFunctionValue (GetPluginVersion \"yUI\")"), XVal::Num(-1.0));
}
