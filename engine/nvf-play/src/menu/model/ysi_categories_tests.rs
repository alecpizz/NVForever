use super::*;
use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::ui_ysi::YsiItemDisplay;
use fnv_game::yui_ini::{YsiCategoryDef, YsiCategoryIndex};
use fnv_sim::SimConfig;

fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
    Subrecord { sig: *sig, data, extended: false }
}

fn z(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn record(sig: &[u8; 4], flags: u32, fid: u32, subs: &[Subrecord]) -> Vec<u8> {
    let payload = write_subrecords(subs);
    cat(&[
        sig,
        &(payload.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &fid.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        &payload,
    ])
}

fn group(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
    cat(&[b"GRUP", &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body])
}

fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
    group(*sig, 0, body)
}

fn named(edid: &str, full: &str) -> [Subrecord; 2] {
    [sub(b"EDID", z(edid)), sub(b"FULL", z(full))]
}

fn weap(fid: u32, edid: &str, full: &str, value: i32, weight: f32) -> Vec<u8> {
    let [e, f] = named(edid, full);
    let data = cat(&[&value.to_le_bytes(), &100i32.to_le_bytes(), &weight.to_le_bytes(), &10i16.to_le_bytes(), &[12u8]]);
    record(b"WEAP", 0, fid, &[e, f, sub(b"DATA", data)])
}

fn misc(fid: u32, edid: &str, full: &str) -> Vec<u8> {
    let [e, f] = named(edid, full);
    record(b"MISC", 0, fid, &[e, f, sub(b"DATA", cat(&[&5i32.to_le_bytes(), &0.5f32.to_le_bytes()]))])
}

fn key(fid: u32, edid: &str, full: &str) -> Vec<u8> {
    let [e, f] = named(edid, full);
    record(b"KEYM", 0, fid, &[e, f, sub(b"DATA", cat(&[&0i32.to_le_bytes(), &0f32.to_le_bytes()]))])
}

fn cnto(item: u32, count: i32) -> Subrecord {
    sub(b"CNTO", cat(&[&item.to_le_bytes(), &count.to_le_bytes()]))
}

const PISTOL: u32 = 0x200;
const RIFLE: u32 = 0x201;
const KNIFE: u32 = 0x202;
const NOTE_A: u32 = 0x210;
const NOTE_B: u32 = 0x211;
const KEY_A: u32 = 0x220;
const CARD_A: u32 = 0x230;
const JUNK: u32 = 0x240;

fn build_fixture() -> (FormStore, RefTable) {
    let tes4 = record(b"TES4", 1, 0, &[sub(b"HEDR", cat(&[&1.34f32.to_le_bytes(), &1u32.to_le_bytes(), &0x800u32.to_le_bytes()]))]);
    let weaps = cat(&[
        &weap(PISTOL, "TestPistol", "9mm Pistol", 100, 1.5),
        &weap(RIFLE, "TestRifle", "Varmint Rifle", 150, 5.5),
        &weap(KNIFE, "TestKnife", "Combat Knife", 50, 1.0),
    ]);
    let miscs = cat(&[
        &misc(NOTE_A, "TestNoteA", "Alpha Note"),
        &misc(NOTE_B, "TestNoteB", "Beta Note"),
        &misc(JUNK, "TestJunk", "Tin Can"),
    ]);
    let keys = key(KEY_A, "TestKeyA", "Shack Key");
    let [ce, cf] = named("TestCardA", "Ace of Spades");
    let card = record(b"CCRD", 0, CARD_A, &[ce, cf]);
    let npc_data = cat(&[&100i32.to_le_bytes(), &[5u8, 6, 7, 4, 3, 2, 1]]);
    let npc = record(
        b"NPC_",
        0,
        0x7,
        &[
            sub(b"EDID", z("Player")),
            cnto(PISTOL, 1),
            cnto(RIFLE, 1),
            cnto(KNIFE, 1),
            cnto(NOTE_A, 1),
            cnto(KEY_A, 1),
            cnto(JUNK, 1),
            sub(b"DATA", npc_data),
        ],
    );
    let bytes = cat(&[
        &tes4,
        &top(b"WEAP", &weaps),
        &top(b"MISC", &miscs),
        &top(b"KEYM", &keys),
        &top(b"CCRD", &card),
        &top(b"NPC_", &npc),
    ]);
    let schema = Schema::load_dir(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv"))).unwrap();
    let plugins = [Plugin::parse(&bytes, "ysi_cats_test.esm".into()).unwrap()];
    let order = LoadOrder::new(&plugins).unwrap();
    (FormStore::build(&order, &schema), RefTable::build(&order, &schema))
}

fn tag_for(id: FormId) -> Option<YsiItemDisplay> {
    let (tag, icon) = match id.0 {
        PISTOL => ("(fa11)", "interface\\icons\\pistol.dds"),
        RIFLE => ("(fa20)", "interface\\icons\\rifle.dds"),
        KNIFE => ("(fb00)", "interface\\icons\\knife.dds"),
        NOTE_A | NOTE_B => ("(jdc1)", "interface\\icons\\note.dds"),
        KEY_A => ("(jdd1)", "interface\\icons\\key.dds"),
        CARD_A => ("(jde1)", "interface\\icons\\card.dds"),
        JUNK => ("(j000)", "interface\\icons\\junk.dds"),
        _ => return None,
    };
    Some(YsiItemDisplay {
        tag: Some(tag.to_string()),
        icon: Some(icon.to_string()),
        template: "ySIDefault".to_string(),
    })
}

fn sample_categories() -> YsiCategoryIndex {
    let mut tag_to_category = BTreeMap::new();
    // 0: Guns (tab 0 root)
    // 1: Pistols (tab 0 child of Guns)
    // 2: Melee (tab 0 root)
    // 3: Notes (tab 3 root, min_count = 2)
    // 4: Keyring (tab 3 root, min_count = 1)
    // 5: Caravan Deck (tab 3 root, min_count = 1)
    tag_to_category.insert("(fa11)".into(), 1);
    tag_to_category.insert("(fa20)".into(), 0);
    tag_to_category.insert("(fb00)".into(), 2);
    tag_to_category.insert("(jdc1)".into(), 3);
    tag_to_category.insert("(jdd1)".into(), 4);
    tag_to_category.insert("(jde1)".into(), 5);
    YsiCategoryIndex {
        categories: vec![
            YsiCategoryDef {
                tag: "(fa00)".into(),
                name: "Guns".into(),
                tab: 0,
                parent: None,
                min_count: 1,
                row_icon: Some("interface\\icons\\cat_guns.dds".into()),
                template: "ySIDefault".into(),
                preview_icon: Some("interface\\icons\\preview_guns.dds".into()),
            },
            YsiCategoryDef {
                tag: "(fa10)".into(),
                name: "Pistols".into(),
                tab: 0,
                parent: Some(0),
                min_count: 1,
                row_icon: Some("interface\\icons\\cat_pistols.dds".into()),
                template: "ySIDefault".into(),
                preview_icon: Some("interface\\icons\\preview_pistols.dds".into()),
            },
            YsiCategoryDef {
                tag: "(fb00c)".into(),
                name: "Melee".into(),
                tab: 0,
                parent: None,
                min_count: 1,
                row_icon: Some("interface\\icons\\cat_melee.dds".into()),
                template: "ySIDefault".into(),
                preview_icon: None,
            },
            YsiCategoryDef {
                tag: "(jdc0)".into(),
                name: "Notes".into(),
                tab: 3,
                parent: None,
                min_count: 2,
                row_icon: Some("interface\\icons\\cat_notes.dds".into()),
                template: "ySIDefault".into(),
                preview_icon: None,
            },
            YsiCategoryDef {
                tag: "(jdd0)".into(),
                name: "Keyring".into(),
                tab: 3,
                parent: None,
                min_count: 1,
                row_icon: Some("interface\\icons\\cat_keys.dds".into()),
                template: "ySIDefault".into(),
                preview_icon: None,
            },
            YsiCategoryDef {
                tag: "(jde0)".into(),
                name: "Caravan Deck".into(),
                tab: 3,
                parent: None,
                min_count: 1,
                row_icon: Some("interface\\icons\\cat_cards.dds".into()),
                template: "ySIDefault".into(),
                preview_icon: None,
            },
        ],
        tag_to_category,
    }
}

#[test]
fn nested_weapon_categories_open_equip_and_back_restore_selection() {
    let (forms, refs) = build_fixture();
    let mut world = World::new(&forms, &refs, SimConfig::default());
    let mut m = MenuModel::open_inventory(&mut world).unwrap();
    m.set_ysi_categories(Some(sample_categories()));
    m.apply(&mut world, Command::SetCategory(Side::Player, Category::Weapons));
    m.apply_ysi_options(true, true, tag_for);

    // Tab root folds Pistol + Rifle under Guns (2) and Knife under Melee (1).
    let root = &m.list(Side::Player).rows;
    let names: Vec<(&str, i32)> = root.iter().map(|r| (r.name.as_str(), r.count)).collect();
    assert_eq!(names, vec![("Guns", 2), ("Melee", 1)]);
    assert_eq!(root[0].icon.as_deref(), Some("interface\\icons\\preview_guns.dds"));
    assert_eq!(
        root[0].ysi.as_ref().and_then(|y| y.icon.as_deref()),
        Some("interface\\icons\\cat_guns.dds")
    );
    // RowKey::at produces unique stable keys for distinct ySI category folders.
    assert_ne!(RowKey::at(root, 0), RowKey::at(root, 1));

    // Open Guns -> Back + Pistols (1) subfolder + direct Varmint Rifle.
    m.apply(&mut world, Command::One);
    m.apply_ysi_options(true, true, tag_for);
    assert_eq!(m.folder, Some(Folder::Ysi(0)));
    let inside_guns: Vec<(&str, i32)> = m.list(Side::Player).rows.iter().map(|r| (r.name.as_str(), r.count)).collect();
    assert_eq!(inside_guns, vec![("Back", 0), ("Pistols", 1), ("Varmint Rifle", 1)]);

    // Open Pistols subfolder -> Back + 9mm Pistol.
    m.apply(&mut world, Command::Down);
    m.apply(&mut world, Command::One);
    m.apply_ysi_options(true, true, tag_for);
    assert_eq!(m.folder, Some(Folder::Ysi(1)));
    let inside_pistols: Vec<&str> = m.list(Side::Player).rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(inside_pistols, vec!["Back", "9mm Pistol"]);

    // Equip 9mm Pistol inside the nested subfolder.
    m.apply(&mut world, Command::Down);
    let applied = m.apply(&mut world, Command::One);
    assert!(applied.changed);
    m.apply_ysi_options(true, true, tag_for);
    assert!(m.list(Side::Player).rows[1].equipped);

    // Back from Pistols -> Guns, restoring selection on Pistols (index 1).
    m.apply(&mut world, Command::Back);
    m.apply_ysi_options(true, true, tag_for);
    assert_eq!(m.folder, Some(Folder::Ysi(0)));
    assert_eq!(m.selected_row(Side::Player).map(|r| r.name.as_str()), Some("Pistols"));

    // Back from Guns -> Weapons root, restoring selection on Guns (index 0).
    m.apply(&mut world, Command::Back);
    m.apply_ysi_options(true, true, tag_for);
    assert_eq!(m.folder, None);
    assert_eq!(m.selected_row(Side::Player).map(|r| r.name.as_str()), Some("Guns"));
}

#[test]
fn misc_threshold_and_caravan_cards_use_ysi_categories_without_vanilla_duplication() {
    let (forms, refs) = build_fixture();
    let mut world = World::new(&forms, &refs, SimConfig::default());
    let mut m = MenuModel::open_inventory(&mut world).unwrap();
    m.set_ysi_categories(Some(sample_categories()));
    m.set_cards(&mut world, vec![FormId(CARD_A)]).unwrap();
    m.apply(&mut world, Command::SetCategory(Side::Player, Category::Misc));
    m.apply_ysi_options(true, true, tag_for);

    // Only 1 note (min_count = 2), so Alpha Note stays flat; Keyring & Caravan Deck collapse.
    let names: Vec<&str> = m.list(Side::Player).rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["Tin Can", "Alpha Note", "Keyring", "Caravan Deck"]);
    assert!(m.list(Side::Player).rows.iter().all(|r| !matches!(r.folder, Some(FolderRow::Open(Folder::Keys | Folder::Cards)))));

    // Adding a second distinct note crosses count: 2 and collapses both into Notes (2).
    world.add_item(PLAYER, FormId(NOTE_B), 1).unwrap();
    m.refresh(&mut world).unwrap();
    m.apply_ysi_options(true, true, tag_for);
    let names_after: Vec<(&str, i32)> = m.list(Side::Player).rows.iter().map(|r| (r.name.as_str(), r.count)).collect();
    assert_eq!(names_after, vec![("Tin Can", 1), ("Notes", 2), ("Keyring", 1), ("Caravan Deck", 1)]);
}

#[test]
fn tab_switch_search_icons_off_and_vanilla_disabled_behave_cleanly() {
    let (forms, refs) = build_fixture();
    let mut world = World::new(&forms, &refs, SimConfig::default());
    let mut m = MenuModel::open_inventory(&mut world).unwrap();
    m.set_ysi_categories(Some(sample_categories()));
    m.apply(&mut world, Command::SetCategory(Side::Player, Category::Weapons));
    m.apply_ysi_options(true, true, tag_for);
    // Enter Guns -> Pistols, then switch tab to Misc: folder stack resets.
    m.apply(&mut world, Command::One);
    m.apply_ysi_options(true, true, tag_for);
    m.apply(&mut world, Command::Down);
    m.apply(&mut world, Command::One);
    m.apply_ysi_options(true, true, tag_for);
    assert_eq!(m.folder, Some(Folder::Ysi(1)));
    m.apply(&mut world, Command::SetCategory(Side::Player, Category::Weapons));
    // Same tab is a no-op; switching to Misc clears folder & stack.
    m.apply(&mut world, Command::SetCategory(Side::Player, Category::Misc));
    assert_eq!(m.folder, None);
    assert!(m.ysi_folder_stack.is_empty());

    // Search at Weapons root keeps matching items flat across collapsed categories.
    m.apply(&mut world, Command::SetCategory(Side::Player, Category::Weapons));
    m.set_search(&mut world, "pistol").unwrap();
    m.apply_ysi_options(true, true, tag_for);
    let search_names: Vec<&str> = m.list(Side::Player).rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(search_names, vec!["9mm Pistol"]);

    // Clearing search with bAddInventoryIcons = 0 still folds categories while stripping row icons.
    m.set_search(&mut world, "").unwrap();
    m.apply_ysi_options(true, false, tag_for);
    let cat_names: Vec<&str> = m.list(Side::Player).rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(cat_names, vec!["Guns", "Melee"]);
    assert!(m.list(Side::Player).rows.iter().all(|r| r.ysi.is_none()));

    // Disabling ySI categories restores vanilla flat Weapons and vanilla Misc Keyring.
    m.set_ysi_categories(None);
    m.refresh(&mut world).unwrap();
    let vanilla_weapons: Vec<&str> = m.list(Side::Player).rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(vanilla_weapons, vec!["9mm Pistol", "Combat Knife", "Varmint Rifle"]);
    m.apply(&mut world, Command::SetCategory(Side::Player, Category::Misc));
    assert_eq!(m.list(Side::Player).rows[0].folder, Some(FolderRow::Open(Folder::Keys)));
}

#[test]
fn repair_mod_or_stats_refresh_inside_ysi_folder_preserves_back_subfolders_and_icons() {
    let (forms, refs) = build_fixture();
    let mut world = World::new(&forms, &refs, SimConfig::default());
    let mut m = MenuModel::open_inventory(&mut world).unwrap();
    m.set_ysi_categories(Some(sample_categories()));
    m.apply(&mut world, Command::SetCategory(Side::Player, Category::Weapons));
    m.apply_ysi_options(true, true, tag_for);

    // Enter Guns (Folder::Ysi(0)), which contains Back, Pistols subfolder, and Varmint Rifle.
    m.apply(&mut world, Command::One);
    m.apply_ysi_options(true, true, tag_for);
    assert_eq!(m.folder, Some(Folder::Ysi(0)));
    assert_eq!(m.list(Side::Player).rows[0].folder, Some(FolderRow::Back));

    // Simulate a Pip-Boy Repair/Mod/Stats inventory mutation followed by `m.refresh` + `sync_ysi_model`.
    world.add_item(PLAYER, FormId(PISTOL), 1).unwrap();
    m.refresh(&mut world).unwrap();
    // Raw `m.refresh` rebuilds flat rows without ySI metadata; re-running `apply_ysi_options`
    // (as `crate::menu::sync_ysi_model` now does after repair/mod/stats refreshes) restores
    // subfolder folding, the Back row, and all `r.ysi` icons while staying in `Folder::Ysi(0)`.
    m.apply_ysi_options(true, true, tag_for);

    assert_eq!(m.folder, Some(Folder::Ysi(0)));
    let rows = &m.list(Side::Player).rows;
    let names: Vec<(&str, Option<FolderRow>)> = rows.iter().map(|r| (r.name.as_str(), r.folder)).collect();
    assert_eq!(
        names,
        vec![
            ("Back", Some(FolderRow::Back)),
            ("Pistols", Some(FolderRow::Open(Folder::Ysi(1)))),
            ("Varmint Rifle", None),
        ]
    );
    assert_eq!(
        rows[1].ysi.as_ref().and_then(|y| y.icon.as_deref()),
        Some("interface\\icons\\cat_pistols.dds")
    );
    assert_eq!(
        rows[2].ysi.as_ref().and_then(|y| y.icon.as_deref()),
        Some("interface\\icons\\rifle.dds")
    );
}

