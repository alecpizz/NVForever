//! Unique quest rewards in co-op (`fnv_game::coop_uniques`;
//! coop-shared-progress.md §4.1): the list computed from the data, the hand-out filter for
//! guests that claimed no stage of the quest, and the undo on leaving keeping a claimer's uniques, on a
//! synthetic plugin built in memory (no game data). Scripts are compiled from source by
//! NVForever's compiler.

use crate::common::{group, record, sub, tes4, top, z};
use fnv_esm::raw::Subrecord;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::coop_joint::ClaimedStage;
use fnv_game::coop_rewards::Gift;
use fnv_game::coop_uniques::{compute, Kind};
use fnv_game::game::{Game, GameConfig};
use fnv_game::GameData;
use fnv_script::compile::{compile, Env, FormInfo};
use fnv_script::unit::RefSlot;
use fnv_script::{FunctionTable, VarDecl};
use fnv_sim::inventory::Extra;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};

const QA: u32 = 0x900;
const QB: u32 = 0x901;
const QB_SCRIPT: u32 = 0x910;
const LEVER_SCRIPT: u32 = 0x911;
const LEVER: u32 = 0x920;
/// Given by QA's stage 10 only.
const GUN: u32 = 0xA00;
/// Given by both quests.
const SHARED: u32 = 0xA01;
/// Given by QB, and in a vendor's container.
const VENDOR: u32 = 0xA02;
/// Given by QB, and in a leveled list.
const LEVELED: u32 = 0xA03;
/// Given by QB, and placed in a cell.
const PLACED: u32 = 0xA04;
/// Given by QB, and by a lever's script.
const SCRIPTED: u32 = 0xA05;
/// Given by QB's quest script only.
const QB_ITEM: u32 = 0xA06;
/// Given by a dialogue line of QB only.
const LINE_ITEM: u32 = 0xA07;
/// Sold for caps by a dialogue line (the clinic's implants).
const SOLD: u32 = 0xA08;
/// Handed out by a background quest's script (a preorder pack).
const PACK: u32 = 0xA09;
/// Given by QA's stage 30, and refilled by its stage 31 (an implant's charge: GRX).
const CHARGE: u32 = 0xA0A;
const CAPS: u32 = 0xF;
const QC: u32 = 0x902;
const QC_SCRIPT: u32 = 0x912;
const LINE2: u32 = 0xE02;
const QPERK: u32 = 0xB00;
const PLAYABLE_PERK: u32 = 0xB01;
/// A companion's dialogue quest, its perk, the companion (its script makes it the
/// player's teammate) and a line it says.
const QD: u32 = 0x903;
const COMPANION: u32 = 0x701;
const COMPANION_SCRIPT: u32 = 0x913;
const LINE3: u32 = 0xE03;
const COMPANION_PERK: u32 = 0xB02;
/// A perk a companion lends while it follows (`coop_perks::COMPANION_PERKS`), given by
/// the companion's quest only here, but lent to every player: not unique.
const FOLLOW_PERK: u32 = 0xB03;
const QSPELL: u32 = 0xC00;
const CONT: u32 = 0xD00;
const LIST: u32 = 0xD01;
const TOPIC: u32 = 0xE00;
const LINE: u32 = 0xE01;
const CELL: u32 = 0x200;
const PLACED_REF: u32 = 0x300;

const NAMES: &[(&str, u32, &[u8; 4])] = &[
    ("player", 0x14, b"REFR"),
    ("TQuestA", QA, b"QUST"),
    ("TQuestB", QB, b"QUST"),
    ("TUniqueGun", GUN, b"MISC"),
    ("TShared", SHARED, b"MISC"),
    ("TVendor", VENDOR, b"MISC"),
    ("TLeveled", LEVELED, b"MISC"),
    ("TPlaced", PLACED, b"MISC"),
    ("TScripted", SCRIPTED, b"MISC"),
    ("TQuestBItem", QB_ITEM, b"MISC"),
    ("TLineItem", LINE_ITEM, b"MISC"),
    ("TSold", SOLD, b"MISC"),
    ("TPack", PACK, b"MISC"),
    ("TCharge", CHARGE, b"MISC"),
    ("Caps001", CAPS, b"MISC"),
    ("TPackQuest", QC, b"QUST"),
    ("TQuestPerk", QPERK, b"PERK"),
    ("TPlayablePerk", PLAYABLE_PERK, b"PERK"),
    ("TCompanionQuest", QD, b"QUST"),
    ("TCompanion", COMPANION, b"NPC_"),
    ("TCompanionPerk", COMPANION_PERK, b"PERK"),
    ("Spotting", FOLLOW_PERK, b"PERK"),
    ("TQuestSpell", QSPELL, b"SPEL"),
];

struct Names;

impl Env for Names {
    fn form(&self, editor_id: &str) -> Option<FormInfo> {
        NAMES.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(editor_id)).map(|(_, f, s)| FormInfo { form: *f, sig: **s })
    }
    fn info(&self, form: u32) -> Option<FormInfo> {
        NAMES.iter().find(|(_, f, _)| *f == form).map(|(_, f, s)| FormInfo { form: *f, sig: **s })
    }
    fn editor_id(&self, form: u32) -> Option<String> {
        NAMES.iter().find(|(_, f, _)| *f == form).map(|(n, _, _)| n.to_string())
    }
    fn local_form(&self, local: u32) -> Option<u32> {
        Some(local)
    }
    fn script_vars(&self, _form: u32) -> Option<Vec<VarDecl>> {
        None
    }
}

/// SCHR, SCDA, SCRO and SCTX of a unit compiled from `src` (`kind`: 0 object, 1 quest).
fn unit(table: &FunctionTable, kind: u16, src: &str) -> Vec<Subrecord> {
    let c = compile(src, table, &Names, None);
    assert!(c.errors().next().is_none(), "{src}: {:?}", c.diags);
    let refs: Vec<u32> = c.refs.iter().filter_map(|r| if let RefSlot::Form(f) = r { Some(*f) } else { None }).collect();
    let mut schr = vec![0u8; 4];
    schr.extend((refs.len() as u32).to_le_bytes());
    schr.extend((c.bytes.len() as u32).to_le_bytes());
    schr.extend(0u32.to_le_bytes());
    schr.extend(kind.to_le_bytes());
    schr.extend(1u16.to_le_bytes());
    let mut out = vec![sub(b"SCHR", schr), sub(b"SCDA", c.bytes.clone())];
    out.extend(refs.iter().map(|r| sub(b"SCRO", r.to_le_bytes().to_vec())));
    out.push(sub(b"SCTX", src.as_bytes().to_vec()));
    out
}

fn misc(fid: u32, edid: &str) -> Vec<u8> {
    record(b"MISC", 0, fid, &[sub(b"EDID", z(edid)), sub(b"DATA", [1i32.to_le_bytes(), 1f32.to_le_bytes()].concat())])
}

fn quest(table: &FunctionTable, fid: u32, edid: &str, script: Option<u32>, stages: &[(i16, &str)]) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid))];
    if let Some(s) = script {
        subs.push(sub(b"SCRI", s.to_le_bytes().to_vec()));
    }
    subs.push(sub(b"DATA", [vec![0u8, 0, 0, 0], 5.0f32.to_le_bytes().to_vec()].concat()));
    for (s, src) in stages {
        subs.push(sub(b"INDX", s.to_le_bytes().to_vec()));
        subs.push(sub(b"QSDT", vec![0]));
        subs.extend(unit(table, 0, src));
    }
    record(b"QUST", 0, fid, &subs)
}

fn plugin(table: &FunctionTable) -> Vec<u8> {
    let scpt = [
        record(b"SCPT", 0, QB_SCRIPT, &[vec![sub(b"EDID", z("TQuestBScript"))], unit(table, 1, "scn TQuestBScript\nbegin GameMode\n  player.AddItem TQuestBItem 1\nend\n")].concat()),
        record(b"SCPT", 0, QC_SCRIPT, &[vec![sub(b"EDID", z("TPackScript"))], unit(table, 1, "scn TPackScript\nbegin GameMode\n  player.AddItem TPack 1\nend\n")].concat()),
        record(b"SCPT", 0, COMPANION_SCRIPT, &[vec![sub(b"EDID", z("TCompanionScript"))], unit(table, 0, "scn TCompanionScript
begin OnActivate
  SetPlayerTeammate 1
end
")].concat()),
        record(b"SCPT", 0, LEVER_SCRIPT, &[vec![sub(b"EDID", z("TLeverScript"))], unit(table, 0, "scn TLeverScript\nbegin OnActivate\n  player.AddItem TScripted 1\nend\n")].concat()),
    ]
    .concat();
    let items = [
        misc(GUN, "TUniqueGun"),
        misc(SHARED, "TShared"),
        misc(VENDOR, "TVendor"),
        misc(LEVELED, "TLeveled"),
        misc(PLACED, "TPlaced"),
        misc(SCRIPTED, "TScripted"),
        misc(QB_ITEM, "TQuestBItem"),
        misc(LINE_ITEM, "TLineItem"),
        misc(SOLD, "TSold"),
        misc(PACK, "TPack"),
        misc(CHARGE, "TCharge"),
        misc(CAPS, "Caps001"),
    ]
    .concat();
    // DATA: trait, minimum level, ranks, playable, hidden.
    let perks = [
        record(b"PERK", 0, QPERK, &[sub(b"EDID", z("TQuestPerk")), sub(b"DATA", vec![0, 1, 1, 0, 0])]),
        record(b"PERK", 0, PLAYABLE_PERK, &[sub(b"EDID", z("TPlayablePerk")), sub(b"DATA", vec![0, 1, 1, 1, 0])]),
        record(b"PERK", 0, COMPANION_PERK, &[sub(b"EDID", z("TCompanionPerk")), sub(b"DATA", vec![0, 1, 1, 0, 0])]),
        record(b"PERK", 0, FOLLOW_PERK, &[sub(b"EDID", z("Spotting")), sub(b"DATA", vec![0, 1, 1, 0, 0])]),
    ]
    .concat();
    let spel = record(b"SPEL", 0, QSPELL, &[sub(b"EDID", z("TQuestSpell")), sub(b"SPIT", [4u32.to_le_bytes(), 0u32.to_le_bytes(), 0u32.to_le_bytes(), 0u32.to_le_bytes()].concat())]);
    let cont = record(
        b"CONT",
        0,
        CONT,
        &[sub(b"EDID", z("TVendorBox")), sub(b"CNTO", [VENDOR.to_le_bytes(), 1i32.to_le_bytes()].concat()), sub(b"DATA", [vec![0], 0f32.to_le_bytes().to_vec()].concat())],
    );
    let lvlo = sub(b"LVLO", [1i16.to_le_bytes().to_vec(), vec![0; 2], LEVELED.to_le_bytes().to_vec(), 1i16.to_le_bytes().to_vec(), vec![0; 2]].concat());
    let lvli = record(b"LVLI", 0, LIST, &[sub(b"EDID", z("TList")), sub(b"LVLD", vec![0]), sub(b"LVLF", vec![0]), lvlo]);
    let acti = record(b"ACTI", 0, LEVER, &[sub(b"EDID", z("TLever")), sub(b"SCRI", LEVER_SCRIPT.to_le_bytes().to_vec())]);
    let qust = [
        quest(
            table,
            QA,
            "TQuestA",
            None,
            &[(10, "player.AddItem TUniqueGun 1\nplayer.AddPerk TQuestPerk\nplayer.AddPerk TPlayablePerk\nplayer.AddSpell TQuestSpell\n"), (20, "player.AddItem TShared 1\n"), (30, "player.AddItem TCharge 5\n"), (31, "player.RemoveItem TCharge 10 1\nplayer.AddItem TCharge 10 1\n")],
        ),
        quest(
            table,
            QB,
            "TQuestB",
            Some(QB_SCRIPT),
            &[(10, "player.AddItem TShared 1\nplayer.AddItem TVendor 1\nplayer.AddItem TLeveled 1\nplayer.AddItemHealthPercent TPlaced 1 0.5\nplayer.AddItem TScripted 1\n")],
        ),
        // A companion's dialogue quest: its stage gives the companion's perk.
        quest(table, QD, "TCompanionQuest", None, &[(20, "player.AddPerk TCompanionPerk
"), (30, "player.AddPerk Spotting
")]),
        // A background quest running from the game's start (DATA flags 0x01), no name,
        // stages or objectives, whose script hands out TPack.
        record(b"QUST", 0, QC, &[sub(b"EDID", z("TPackQuest")), sub(b"SCRI", QC_SCRIPT.to_le_bytes().to_vec()), sub(b"DATA", [vec![1u8, 0, 0, 0], 5.0f32.to_le_bytes().to_vec()].concat())]),
    ]
    .concat();
    // A line of TQuestB whose end script gives TLineItem.
    let trdt = [0u32.to_le_bytes().to_vec(), 50u32.to_le_bytes().to_vec(), vec![0; 4], vec![1, 0, 0, 0], 0u32.to_le_bytes().to_vec(), vec![0; 4]].concat();
    let mut info = vec![sub(b"DATA", vec![0, 0, 0, 0]), sub(b"QSTI", QB.to_le_bytes().to_vec()), sub(b"TRDT", trdt.clone()), sub(b"NAM1", z("Take this.")), sub(b"NAM2", z("")), sub(b"NAM3", z(""))];
    info.push(sub(b"NEXT", vec![]));
    info.extend(unit(table, 0, "player.AddItem TLineItem 1\n"));
    let dial_subs = [sub(b"EDID", z("TTopic")), sub(b"FULL", z("TTopic")), sub(b"QSTI", QB.to_le_bytes().to_vec()), sub(b"PNAM", 50f32.to_le_bytes().to_vec()), sub(b"DATA", vec![0, 0])];
    // A second line of TQuestB that sells TSold for caps.
    let mut sale = vec![sub(b"DATA", vec![0, 0, 0, 0]), sub(b"QSTI", QB.to_le_bytes().to_vec()), sub(b"TRDT", trdt.clone()), sub(b"NAM1", z("Sold.")), sub(b"NAM2", z("")), sub(b"NAM3", z(""))];
    sale.push(sub(b"NEXT", vec![]));
    sale.extend(unit(table, 0, "player.RemoveItem Caps001 100\nplayer.AddItem TSold 1\n"));
    // A line of TCompanionQuest said by TCompanion (ANAM) that gives its perk.
    let mut said = vec![sub(b"DATA", vec![0, 0, 0, 0]), sub(b"QSTI", QD.to_le_bytes().to_vec()), sub(b"TRDT", trdt.clone()), sub(b"NAM1", z("For you.")), sub(b"NAM2", z("")), sub(b"NAM3", z(""))];
    said.push(sub(b"ANAM", COMPANION.to_le_bytes().to_vec()));
    said.push(sub(b"NEXT", vec![]));
    said.extend(unit(table, 0, "player.AddPerk TCompanionPerk
"));
    let lines = [record(b"INFO", 0, LINE, &info), record(b"INFO", 0, LINE2, &sale), record(b"INFO", 0, LINE3, &said)].concat();
    let dial = [record(b"DIAL", 0, TOPIC, &dial_subs), group(TOPIC.to_le_bytes(), 7, &lines)].concat();
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TCell"))]);
    let placed = record(b"REFR", 0, PLACED_REF, &[sub(b"NAME", PLACED.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &placed));
    // The player's base, so the player has an inventory.
    let player = [
        record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]),
        record(b"NPC_", 0, COMPANION, &[sub(b"EDID", z("TCompanion")), sub(b"SCRI", COMPANION_SCRIPT.to_le_bytes().to_vec())]),
    ]
    .concat();
    [
        tes4(),
        top(b"NPC_", &player),
        top(b"SCPT", &scpt),
        top(b"MISC", &items),
        top(b"PERK", &perks),
        top(b"SPEL", &spel),
        top(b"CONT", &cont),
        top(b"LVLI", &lvli),
        top(b"ACTI", &acti),
        top(b"QUST", &qust),
        top(b"DIAL", &dial),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat()))),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let bytes = plugin(&table);
    let plugins = vec![Plugin::parse(&bytes, "Uniques.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() });
    g.new_game();
    g
}

fn claim(g: &mut Game, q: u32, stage: i32) {
    g.player.claims.stages.entry(FormId(q)).or_default().push(ClaimedStage { stage, line: None });
}

fn carried(g: &Game, item: u32) -> i32 {
    g.world.peek(FormId::PLAYER_REF).and_then(|r| r.inventory.as_ref()).map_or(0, |i| i.count(FormId(item)))
}

#[test]
fn only_what_one_quests_scripts_alone_give_is_unique() {
    let f = fixture();
    let u = compute(&f.data, &f.forms, &f.refs);
    let got: Vec<(u32, Kind, u32)> = u.by_form.iter().map(|(f, (k, q))| (f.0, *k, q.0)).collect();
    assert_eq!(
        got,
        vec![
            // A stage script's item, perk and spell.
            (GUN, Kind::Item, QA),
            // The quest script's item, and a dialogue line's of the quest.
            (QB_ITEM, Kind::Item, QB),
            (LINE_ITEM, Kind::Item, QB),
            (QPERK, Kind::Perk, QA),
            (COMPANION_PERK, Kind::Perk, QD),
            (QSPELL, Kind::Spell, QA),
        ],
        "not unique: given by two quests, a vendor's container, a leveled list, a placed reference, an object's script, a playable perk, \
         sold for caps, a background quest's hand-out, a refilled charge, a companion's follow perk"
    );
    assert!(f.data.infos.contains_key(&FormId(LINE2)) && f.data.quests.contains_key(&FormId(QC)), "the sale line and the pack quest are read");
    // The same list the game keeps, computed once.
    let g = game(&f);
    assert_eq!(g.coop_uniques(), &u);
    assert!(std::ptr::eq(g.coop_uniques(), g.coop_uniques()));
}

#[test]
fn a_unique_goes_only_to_a_guest_that_claimed_a_stage_of_its_quest() {
    let f = fixture();
    let mut g = game(&f);
    let gift = |item: u32, batch: u32| Gift { batch, item: FormId(item), count: 1, quest: Some(FormId(QA)), stage: Some(10) };
    let mut last = None;
    // Not claimed: the unique stays the host's; a shared hand-out still comes.
    assert!(!g.coop_unique_allowed(FormId(GUN)));
    assert_eq!(g.take_coop_gift(&gift(GUN, 1), &mut last), Ok(0));
    assert_eq!(carried(&g, GUN), 0);
    assert_eq!(g.take_coop_gift(&gift(SHARED, 1), &mut last), Ok(1));
    assert_eq!(carried(&g, SHARED), 1);
    // A claim of another quest does not count.
    claim(&mut g, QB, 10);
    assert_eq!(g.take_coop_gift(&gift(GUN, 2), &mut last), Ok(0));
    // A claimed stage of its quest: it comes.
    claim(&mut g, QA, 10);
    assert!(g.coop_unique_allowed(FormId(GUN)));
    assert_eq!(g.take_coop_gift(&gift(GUN, 3), &mut last), Ok(1));
    assert_eq!(carried(&g, GUN), 1);
}

#[test]
fn the_undo_keeps_a_claimers_uniques_and_takes_the_rest() {
    let f = fixture();
    let mut g = game(&f);
    // The host's story gave the guest the gun, the shared item and the quest's perk.
    let none = Extra::default();
    g.world.add_item(FormId::PLAYER_REF, FormId(GUN), 1).unwrap();
    g.world.add_item(FormId::PLAYER_REF, FormId(SHARED), 1).unwrap();
    g.coop_story.record_items(&[], &[(FormId(GUN), none, 1), (FormId(SHARED), none, 1)]);
    g.coop_story.record_perk(FormId(QPERK), 0);
    g.player.perks.insert(FormId(QPERK), 1);
    // Not a claimer: everything is undone in its own save.
    let mut s = g.snapshot();
    g.coop_story_to_undo().undo_on(&mut s);
    let inv = |s: &fnv_game::game::GameSnapshot, item: u32| s.sim.refs.get(&FormId::PLAYER_REF).and_then(|r| r.inventory.as_ref()).map_or(0, |i| i.count(FormId(item)));
    assert_eq!((inv(&s, GUN), inv(&s, SHARED)), (0, 0));
    assert!(!s.player.perks.contains_key(&FormId(QPERK)));
    // A claimer of the quest keeps its uniques; the shared item still goes.
    claim(&mut g, QA, 10);
    let mut s = g.snapshot();
    let undone = g.coop_story_to_undo().undo_on(&mut s);
    assert_eq!((inv(&s, GUN), inv(&s, SHARED)), (1, 0), "{undone:?}");
    assert_eq!(s.player.perks.get(&FormId(QPERK)), Some(&1));
    // The running game is not touched.
    assert_eq!(g.coop_story.items.len(), 2);
}

/// Companion perks are the host's alone [design: only the host]: a
/// guest's game never gives one, claimed quest or not, and its own save never keeps one.
#[test]
fn companion_perks_are_the_hosts_alone() {
    let f = fixture();
    let u = compute(&f.data, &f.forms, &f.refs);
    assert_eq!(u.companion.iter().map(|p| p.0).collect::<Vec<_>>(), vec![COMPANION_PERK], "the companion quest's perk, not the follow perk");
    assert_eq!(u.quest_of(FormId(FOLLOW_PERK)), None, "a follow perk is no unique reward");
    assert!(!u.is_companion_perk(FormId(QPERK)), "another quest's unique perk is no companion perk");
    assert!(f.data.infos.get(&FormId(LINE3)).is_some_and(|i| i.speaker == Some(FormId(COMPANION))), "the companion's line is read");
    // A guest that claimed the companion's quest: the perk is still not given.
    let mut g = game(&f);
    g.coop_guest = true;
    claim(&mut g, QD, 20);
    assert!(!g.coop_unique_allowed(FormId(COMPANION_PERK)));
    g.set_stage(FormId(QD), 20);
    assert!(!g.player.perks.contains_key(&FormId(COMPANION_PERK)), "a guest's AddPerk of a companion perk gives nothing");
    // Had the story recorded one, the guest's own save would not keep it.
    g.coop_story.record_perk(FormId(COMPANION_PERK), 0);
    g.player.perks.insert(FormId(COMPANION_PERK), 1);
    let mut s = g.snapshot();
    g.coop_story_to_undo().undo_on(&mut s);
    assert!(!s.player.perks.contains_key(&FormId(COMPANION_PERK)));
    // Its own world replaying the claim (solo, after the session): still not given.
    let mut r = game(&f);
    r.coop_replaying = true;
    r.set_stage(FormId(QD), 20);
    assert!(!r.player.perks.contains_key(&FormId(COMPANION_PERK)), "a replayed claim gives no companion perk");
    // The host (not a guest) gets it as the original gives it.
    let mut h = game(&f);
    h.set_stage(FormId(QD), 20);
    assert_eq!(h.player.perks.get(&FormId(COMPANION_PERK)), Some(&1));
}

/// One round of the companion-perk exchange: each player sends the follow perks its own
/// companions give, and each takes the others' (`nvf-play` coop_companions).
fn party_round(players: &mut [(Game, Vec<FormId>)]) {
    let sent: Vec<Vec<FormId>> = players.iter().map(|(g, m)| fnv_game::coop_perks::own_companion_perks(g, m)).collect();
    for (i, (g, mirrored)) in players.iter_mut().enumerate() {
        let want = fnv_game::coop_perks::party_perks(sent.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, l)| l.as_slice()));
        fnv_game::coop_perks::sync_party_perks(g, mirrored, &want);
    }
}

#[test]
fn a_companions_follow_perk_reaches_every_player_whoever_hired_it() {
    let f = fixture();
    let spot = FormId(FOLLOW_PERK);
    let host = game(&f);
    let mut g1 = game(&f);
    g1.coop_guest = true;
    let mut g2 = game(&f);
    g2.coop_guest = true;
    // A guest (not the host) has the companion: its script gave it Spotting.
    g1.player.perks.insert(spot, 1);
    let mut party = vec![(host, Vec::new()), (g1, Vec::new()), (g2, Vec::new())];
    party_round(&mut party);
    party_round(&mut party);
    for (i, (g, m)) in party.iter().enumerate() {
        assert_eq!(g.player.perks.get(&spot), Some(&1), "player {i} has the follow perk");
        assert_eq!(m.contains(&spot), i != 1, "player {i}: lent, except to the one whose companion gives it");
    }
    assert!(fnv_game::coop_perks::own_companion_perks(&party[0].0, &party[0].1).is_empty(), "a lent perk is not sent on");
    // The host's save does not keep a perk another player's companion lends it.
    let mut s = party[0].0.snapshot();
    party[0].0.coop_story_to_undo().undo_on(&mut s);
    assert!(!s.player.perks.contains_key(&spot), "the host's save is without the lent perk");
    // The companion leaves the guest (its script takes the perk back): every player loses it.
    party[1].0.player.perks.remove(&spot);
    party_round(&mut party);
    party_round(&mut party);
    for (i, (g, m)) in party.iter().enumerate() {
        assert!(!g.player.perks.contains_key(&spot), "player {i} lost the follow perk");
        assert!(m.is_empty());
    }
    // A player's own perk is never taken: the host hires its own, then another's leaves.
    party[0].0.player.perks.insert(spot, 1);
    party[2].0.player.perks.insert(spot, 1);
    party_round(&mut party);
    assert!(party[0].1.is_empty() && party[2].1.is_empty() && party[1].1 == vec![spot]);
    party[2].0.player.perks.remove(&spot);
    party_round(&mut party);
    assert_eq!(party[0].0.player.perks.get(&spot), Some(&1), "the host's own companion's perk stays");
    assert_eq!(party[1].0.player.perks.get(&spot), Some(&1), "still lent by the host's companion");
}
