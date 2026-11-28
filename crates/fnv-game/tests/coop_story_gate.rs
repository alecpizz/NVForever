//! The co-op story on the host and its guests (`fnv_game::coop`): the host waits for every
//! player before it talks to a story NPC, and a guest wears what the host's story put on
//! the host's player. A synthetic plugin built in memory; no game data.

use crate::common;

use common::*;
use fnv_esm::raw::Subrecord;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::data::{info_flag, topic_type};
use fnv_game::game::{Game, GameConfig};
use fnv_game::GameData;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CASH: u32 = 0x700;
const CHIP: u32 = 0x701;
/// A body suit, as a DLC's story puts on the Courier (the Sierra Madre jumpsuit).
const SUIT: u32 = 0x702;
const QC: u32 = 0x900;
const CASHIER: u32 = 0x920;
const CASHIER_REF: u32 = 0x320;
const CELL: u32 = 0x200;
const GREETING: u32 = 0x930;
const GOODBYE: u32 = 0x931;
const T_BUY: u32 = 0x932;
const T_CASH_IN: u32 = 0x933;
const T_STORY: u32 = 0x934;
const T_FREE: u32 = 0x935;
const T_MENU: u32 = 0x936;
const I_HELLO: u32 = 0xA01;
const I_BYE: u32 = 0xA02;
const I_BUY: u32 = 0xA03;
const I_CASH_IN: u32 = 0xA04;
const I_STORY: u32 = 0xA05;
const I_FREE: u32 = 0xA06;
const I_MENU: u32 = 0xA07;

const ADD_ITEM: u16 = 0x1002;
const REMOVE_ITEM: u16 = 0x1052;

fn u32b(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

/// `player.<op> <ref slot> <count>`: the player is SCRO 1.
fn item_call(op: u16, item_slot: u16, count: i32) -> Vec<u8> {
    let params = [le16(2), vec![b'r'], le16(item_slot), vec![b'n'], count.to_le_bytes().to_vec()].concat();
    [le16(0x1C), le16(1), st(op, &params)].concat()
}

/// A result script of item calls: (opcode, item, count); SCRO 1 the player, then the items.
fn result(calls: &[(u16, u32, i32)]) -> Vec<Subrecord> {
    result_naming(calls, &[])
}

/// As [`result`], with more forms in the reference list after the items (a quest the
/// script names, as a StartQuest or SetStage call would).
fn result_naming(calls: &[(u16, u32, i32)], named: &[u32]) -> Vec<Subrecord> {
    let mut refs = vec![FormId::PLAYER_REF.0];
    let mut body = Vec::new();
    for &(op, item, count) in calls {
        let slot = match refs.iter().position(|r| *r == item) {
            Some(i) => i + 1,
            None => {
                refs.push(item);
                refs.len()
            }
        };
        body.extend(item_call(op, slot as u16, count));
    }
    refs.extend(named.iter().filter(|n| !refs.contains(n)).copied().collect::<Vec<_>>());
    unit(0, &body, &[], &refs)
}

fn info(id: u32, flags: u16, text: &str, choices: &[u32], end: &[(u16, u32, i32)]) -> Vec<u8> {
    info_naming(id, flags, text, choices, end, &[])
}

/// As [`info`], its end script naming the forms `named` too.
fn info_naming(id: u32, flags: u16, text: &str, choices: &[u32], end: &[(u16, u32, i32)], named: &[u32]) -> Vec<u8> {
    let trdt = [u32b(0), u32b(50), vec![0; 4], vec![1, 0, 0, 0], u32b(0), vec![0; 4]].concat();
    let mut s = vec![sub(b"DATA", vec![0, 0, flags as u8, (flags >> 8) as u8]), sub(b"QSTI", u32b(QC))];
    s.extend([sub(b"TRDT", trdt), sub(b"NAM1", z(text)), sub(b"NAM2", z("")), sub(b"NAM3", z(""))]);
    s.extend(choices.iter().map(|t| sub(b"TCLT", u32b(*t))));
    s.extend(result(&[]));
    s.push(sub(b"NEXT", vec![]));
    s.extend(result_naming(end, named));
    s.push(sub(b"ANAM", u32b(CASHIER)));
    record(b"INFO", 0, id, &s)
}

fn topic(id: u32, edid: &str, ty: u8, flags: u8, lines: Vec<u8>) -> Vec<u8> {
    let s = [sub(b"EDID", z(edid)), sub(b"FULL", z(edid)), sub(b"PNAM", 50f32.to_le_bytes().to_vec()), sub(b"DATA", vec![ty, flags])];
    [record(b"DIAL", 0, id, &s), group(id.to_le_bytes(), 7, &lines)].concat()
}

fn plugin_bytes() -> Vec<u8> {
    plugin_bytes_as(false)
}

/// `vendor`: the greeting offers the exchange (`T_BUY`) as its choice, and no exchange
/// topic is top-level, as Genaro's barter is reached only from his greeting [observed:
/// FalloutNV.esm INFO 0012936A].
fn plugin_bytes_as(vendor: bool) -> Vec<u8> {
    plugin_bytes_quest(vendor, "QCashier")
}

/// As [`plugin_bytes_as`], the lines' quest named `quest` (a story quest's Editor ID makes
/// the cashier a story NPC, `Game::story_npc`).
fn plugin_bytes_quest(vendor: bool, quest: &str) -> Vec<u8> {
    plugin_bytes_starting(vendor, quest, None)
}

/// A second quest (Editor ID `starts`) that the story line's end script names, as Jed's
/// greeting at Zion starts NVDLC02MQ01 from a line of NVDLC02DialogueCaravanMaster
/// [observed: HonestHearts.esm INFO 0x0100F195].
const QS: u32 = 0x901;

fn plugin_bytes_starting(vendor: bool, quest: &str, starts: Option<&str>) -> Vec<u8> {
    plugin_bytes_full(vendor, false, quest, starts)
}

/// `both`, with `vendor`: the greeting offers the exchange as its choice, and the cash-in
/// and menu topics are top-level too, as a cashier in the game's cashier faction gets the
/// vanilla cashiers' top-level exchange topics [hypothesis].
fn plugin_bytes_full(vendor: bool, both: bool, quest: &str, starts: Option<&str>) -> Vec<u8> {
    let misc = record(b"MISC", 0, CASH, &[sub(b"EDID", z("TestCash")), sub(b"DATA", [1i32.to_le_bytes(), 0f32.to_le_bytes()].concat())]);
    let chip = record(b"CHIP", 0, CHIP, &[sub(b"EDID", z("TestChip"))]);
    let suit = record(b"ARMO", 0, SUIT, &[sub(b"EDID", z("TestSuit")), sub(b"BMDT", [u32s(&[1 << 2]), vec![0; 4]].concat()), sub(b"DATA", [u32s(&[1, 100]), 1f32.to_le_bytes().to_vec()].concat())]);
    let qust = |id: u32, edid: &str| record(b"QUST", 0, id, &[sub(b"EDID", z(edid)), sub(b"DATA", [vec![1u8, 50, 0, 0], 0f32.to_le_bytes().to_vec()].concat())]);
    let mut quest = qust(QC, quest);
    if let Some(e) = starts {
        quest.extend(qust(QS, e));
    }
    let story_named: &[u32] = if starts.is_some() { &[QS] } else { &[] };
    // The player's base form (0x7), whose reference carries the player's inventory.
    let npc = [record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]), record(b"NPC_", 0, CASHIER, &[sub(b"EDID", z("Cashier"))])].concat();
    let top_level = 2u8;
    let exchange_level = if vendor && !both { 0 } else { top_level };
    let hello_choices: &[u32] = if vendor { &[T_BUY] } else { &[] };
    let dial = [
        // A greeting without choices: the conversation lists the top-level topics, as at
        // most casinos' cashiers [observed: FalloutNV.esm vDialogueCasinoCashier].
        topic(GREETING, "GREETING", topic_type::TOPIC, 0, info(I_HELLO, 0, "Welcome", hello_choices, &[])),
        topic(GOODBYE, "GOODBYE", topic_type::CONVERSATION, 0, info(I_BYE, info_flag::GOODBYE, "Enjoy your stay", &[], &[])),
        // Chips for cash, and cash for chips: the exchange.
        topic(T_BUY, "TestBuy", topic_type::TOPIC, 0, info(I_BUY, 0, "Here are your chips", &[], &[(REMOVE_ITEM, CASH, 10), (ADD_ITEM, CHIP, 10)])),
        topic(T_CASH_IN, "TestCashIn", topic_type::TOPIC, exchange_level, info(I_CASH_IN, 0, "Here is your money", &[], &[(REMOVE_ITEM, CHIP, 4), (ADD_ITEM, CASH, 4)])),
        // A line that leads to the exchange: the guest gets it too.
        topic(T_MENU, "TestMenu", topic_type::TOPIC, exchange_level, info(I_MENU, 0, "How many chips?", &[T_BUY], &[])),
        // A quest line, and a line that only hands chips out (a comp, not an exchange).
        topic(T_STORY, "TestStory", topic_type::TOPIC, top_level, info_naming(I_STORY, 0, "About the boss", &[], &[(ADD_ITEM, CHIP, 1)], story_named)),
        topic(T_FREE, "TestFree", topic_type::TOPIC, top_level, info(I_FREE, 0, "On the house", &[], &[(ADD_ITEM, CHIP, 50)])),
    ]
    .concat();
    let placed = record(b"ACHR", 0, CASHIER_REF, &[sub(b"EDID", z("CashierREF")), sub(b"NAME", u32b(CASHIER)), sub(b"DATA", vec![0; 24])]);
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("CasinoCell"))]);
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &placed));
    [
        tes4(),
        top(b"MISC", &misc),
        top(b"CHIP", &chip),
        top(b"ARMO", &suit),
        top(b"NPC_", &npc),
        top(b"QUST", &quest),
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
    fixture_of(plugin_bytes())
}

fn fixture_of(bytes: Vec<u8>) -> Fixture {
    let plugins = vec![Plugin::parse(&bytes, "Casino.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    assert!(data.issues.is_empty(), "{:?}", data.issues);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture, guest: bool) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() });
    g.new_game();
    g.coop_guest = guest;
    g
}

#[test]
fn a_guest_wears_what_the_hosts_story_put_on_and_gives_it_back() {
    // The host's scripts dress the host (Dead Money's collar
    // and jumpsuit, Old World Blues' gown); each guest wears them too, and they go when
    // the host's player no longer wears them or the guest leaves.
    let f = fixture();
    let suit = FormId(SUIT);
    let mut host = game(&f, false);
    host.world.add_item(FormId::PLAYER_REF, suit, 1).unwrap();
    host.world.equip(FormId::PLAYER_REF, suit).unwrap();
    host.script_worn.insert(suit, true);
    assert_eq!(host.story_gear(), vec![(suit, true)]);
    host.world.unequip(FormId::PLAYER_REF, suit).unwrap();
    assert!(host.story_gear().is_empty(), "taken off: no longer sent");
    let mut g = game(&f, true);
    let count = |g: &mut Game| g.world.item_count(FormId::PLAYER_REF, suit).unwrap();
    let worn = |g: &Game| g.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).is_some_and(|i| i.is_equipped(suit));
    assert_eq!(g.follow_story_gear(&[(suit, true)]), (vec![suit], vec![]));
    assert_eq!(count(&mut g), 1);
    assert!(worn(&g));
    assert_eq!(g.world.equip_lock(FormId::PLAYER_REF, suit), Some(fnv_sim::world::EquipLock::NoUnequip));
    // Heard again: nothing more.
    assert_eq!(g.follow_story_gear(&[(suit, true)]), (vec![], vec![]));
    assert_eq!(count(&mut g), 1);
    // Gone from the host: taken off and away.
    assert_eq!(g.follow_story_gear(&[]), (vec![], vec![suit]));
    assert_eq!(count(&mut g), 0);
    // A guest that had one keeps it, taken off.
    g.world.add_item(FormId::PLAYER_REF, suit, 1).unwrap();
    g.follow_story_gear(&[(suit, false)]);
    assert!(worn(&g));
    g.follow_story_gear(&[]);
    assert_eq!(count(&mut g), 1);
    assert!(!worn(&g));
}

#[test]
fn a_host_keeps_its_story_gear_across_a_save_and_load() {
    // Observed (buckallobill, in game, FNV 1.4.0.525): Game::script_worn was not saved, so a host that loaded sent its
    // guests an empty story-gear list and they took the gear off.
    let f = fixture();
    let suit = FormId(SUIT);
    let mut host = game(&f, false);
    host.world.add_item(FormId::PLAYER_REF, suit, 1).unwrap();
    host.world.equip(FormId::PLAYER_REF, suit).unwrap();
    host.script_worn.insert(suit, true);
    let s = host.snapshot();
    assert_eq!(s.script_worn.get(&suit), Some(&true));
    // Through the save file's own form, as a load reads it.
    let s: fnv_game::game::GameSnapshot = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    let world = World::restore(&f.forms, &f.refs, &s.sim).unwrap();
    let loaded = Game::restore(&f.data, world, GameConfig { strict: true, ..GameConfig::default() }, &s).unwrap();
    assert_eq!(loaded.story_gear(), vec![(suit, true)], "sent to the guests as before the load");
}

#[test]
fn a_guest_keeps_on_what_it_already_wore_when_the_hosts_story_lets_it_go() {
    // [design] The host's Pip-Boy is story gear (Doc
    // Mitchell's script equips it); the guest wore its own. When the host's list no
    // longer had it, the guest's was taken off and its Pip-Boy fell back to the
    // screen-only frame.
    let f = fixture();
    let suit = FormId(SUIT);
    let mut g = game(&f, true);
    let count = |g: &mut Game| g.world.item_count(FormId::PLAYER_REF, suit).unwrap();
    let worn = |g: &Game| g.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).is_some_and(|i| i.is_equipped(suit));
    g.world.add_item(FormId::PLAYER_REF, suit, 1).unwrap();
    g.world.equip(FormId::PLAYER_REF, suit).unwrap();
    assert_eq!(g.follow_story_gear(&[(suit, false)]), (vec![], vec![]), "worn already: nothing put on");
    assert_eq!(g.follow_story_gear(&[(suit, false)]), (vec![], vec![]));
    assert_eq!(g.follow_story_gear(&[]), (vec![], vec![]), "the host's goes: nothing taken off");
    assert!(worn(&g), "the guest's own stays on");
    assert_eq!(count(&mut g), 1);
    // Heard again after that, and the guest leaving: still its own, still on.
    g.follow_story_gear(&[(suit, true)]);
    assert_eq!(g.world.equip_lock(FormId::PLAYER_REF, suit), None, "its own is not locked on");
    g.follow_story_gear(&[]);
    assert!(worn(&g));
}

/// [design] A co-op host cannot start a conversation with a story NPC (one with
/// lines of the main quest or a DLC's main quest) while another player is not within
/// sight of it; it can once every player is near, and an NPC of no story quest is never
/// held.
#[test]
fn a_host_waits_for_every_player_to_talk_to_a_story_npc() {
    let near_story = |quest: &str, at: f32| {
        let f = fixture_of(plugin_bytes_quest(false, quest));
        let mut g = game(&f, false);
        let cashier = FormId(CASHIER_REF);
        // The fixture's reference has no cell of its own: put it in the casino.
        let cell = Some(FormId(CELL));
        g.world.get(cashier).unwrap().cell = cell;
        let other = g.world.place(FormId::PLAYER_BASE, cell, [at, 0.0, 0.0], [0.0; 3], 1).unwrap();
        g.coop_actors.insert(other, "P2".into());
        let talked = g.start_dialogue(cashier, None).is_ok();
        (talked, g.coop_story_wait.as_ref().map(|w| w.far.clone()))
    };
    // A DLC's main quest, the other player 3000 units away: held, with who is far.
    let (talked, wait) = near_story("NVDLC02MQ99", 3000.0);
    assert!(!talked);
    assert_eq!(wait.map(|w| w.len()), Some(1));
    // Near (within sight): the conversation starts.
    let (talked, wait) = near_story("NVDLC02MQ99", 500.0);
    assert!(talked && wait.is_none());
    // Not a story quest: never held.
    let (talked, wait) = near_story("QCashier", 3000.0);
    assert!(talked && wait.is_none());
}

/// A script's `X.Activate Player` on a story NPC is the script's scene: the co-op story
/// gate does not hold it, as it does not hold a script's StartConversation. Dead Money's
/// fountain script activates the hologram NVDLC01ElijahTalkingActivatorREF with the
/// player [observed: DeadMoney.esm NVDLC01FountainStartSequenceSCRIPT; buckallobill, in game, FNV 1.4.0.525
/// where the host's talk waited for a guest and never started]. The player's
/// own activation is still held.
#[test]
fn a_scripts_activate_player_on_a_story_npc_is_not_held() {
    let f = fixture_of(plugin_bytes_quest(false, "NVDLC02MQ99"));
    let mut g = game(&f, false);
    let cashier = FormId(CASHIER_REF);
    let cell = Some(FormId(CELL));
    g.world.get(cashier).unwrap().cell = cell;
    let other = g.world.place(FormId::PLAYER_BASE, cell, [3000.0, 0.0, 0.0], [0.0; 3], 1).unwrap();
    g.coop_actors.insert(other, "P2".into());
    // By hand: no scripted talk is noted, and the talk waits.
    g.activate(FormId::PLAYER_REF, cashier, fnv_game::game::ActMode::Hand).unwrap();
    assert!(g.script_talks.is_empty(), "the player's hand is not a script's");
    assert!(g.start_dialogue(cashier, None).is_err());
    // A script's Activate with the player: noted, and its conversation opens.
    g.activate(FormId::PLAYER_REF, cashier, fnv_game::game::ActMode::Default).unwrap();
    assert_eq!(g.script_talks, vec![cashier]);
    assert!(g.start_script_dialogue(cashier, None).is_ok());
    assert!(g.dialogue.is_some());
}

/// A line of an ordinary quest whose compiled end script names a story quest makes its
/// speaker a story NPC: Jed's greeting at Zion (NVDLC02DialogueCaravanMaster) starts
/// NVDLC02MQ01 [observed: HonestHearts.esm INFO 0x0100F195; buckallobill, in game, FNV 1.4.0.525,
/// where the host was greeted with the guest in Goodsprings].
#[test]
fn a_line_starting_a_story_quest_makes_a_story_npc() {
    let f = fixture_of(plugin_bytes_starting(false, "QCashier", Some("NVDLC02MQ01")));
    let mut g = game(&f, false);
    let cashier = FormId(CASHIER_REF);
    let cell = Some(FormId(CELL));
    g.world.get(cashier).unwrap().cell = cell;
    let other = g.world.place(FormId::PLAYER_BASE, cell, [3000.0, 0.0, 0.0], [0.0; 3], 1).unwrap();
    g.coop_actors.insert(other, "P2".into());
    assert!(g.story_npc(cashier));
    assert!(g.start_dialogue(cashier, None).is_err());
    assert_eq!(g.coop_story_wait.as_ref().map(|w| w.far.len()), Some(1));
    // Every player near: the talk starts; far again: held again.
    g.world.get(other).unwrap().pos = [100.0, 0.0, 0.0];
    assert!(!g.story_gate(cashier));
    g.world.get(other).unwrap().pos = [3000.0, 0.0, 0.0];
    assert!(g.story_gate(cashier));
}

/// An NPC's own greet held for a far guest shows on the screens only at first, though it
/// stays held; the host's own try to talk shows it again [design: "WAITING FOR PLAYERS..." stayed up on the Strip while a Securitron's greet waited].
#[test]
fn an_npcs_held_greet_is_shown_only_at_first() {
    let f = fixture_of(plugin_bytes_starting(false, "QCashier", Some("NVDLC02MQ01")));
    let mut g = game(&f, false);
    let cashier = FormId(CASHIER_REF);
    let cell = Some(FormId(CELL));
    g.world.get(cashier).unwrap().cell = cell;
    let other = g.world.place(FormId::PLAYER_BASE, cell, [3000.0, 0.0, 0.0], [0.0; 3], 1).unwrap();
    g.coop_actors.insert(other, "P2".into());
    let shown = |g: &fnv_game::Game| g.coop_story_wait.as_ref().is_some_and(|w| w.shown(g.time));
    assert!(g.story_gate_npc(cashier));
    assert!(shown(&g));
    // Its package asks every update: still held, no longer shown.
    for _ in 0..20 {
        g.time += 0.5;
        assert!(g.story_gate_npc(cashier));
    }
    assert!(!shown(&g), "an NPC's greet held for 10 s is not on the screens");
    // The host walks up and talks to it: shown again, and stays shown while asked.
    assert!(g.story_gate(cashier));
    assert!(shown(&g));
    g.time += 0.5;
    assert!(g.story_gate_npc(cashier));
    assert!(shown(&g), "the host's own try keeps it shown");
    // After a break, the NPC's greet alone is a new hold: shown at first only.
    g.time += 10.0;
    assert!(g.story_gate_npc(cashier));
    assert!(shown(&g));
    for _ in 0..20 {
        g.time += 0.5;
        assert!(g.story_gate_npc(cashier));
    }
    assert!(!shown(&g));
    g.time += 0.5;
    assert!(g.story_gate(cashier));
    assert!(shown(&g));
}

/// A player alone, and a guest's own game, are never held.
#[test]
fn no_story_wait_alone_or_for_a_guest() {
    let f = fixture_of(plugin_bytes_quest(false, "NVDLC02MQ99"));
    let mut solo = game(&f, false);
    assert!(!solo.story_gate(FormId(CASHIER_REF)));
    let mut g = game(&f, true);
    g.coop_actors.insert(FormId(0xFF00_0001), "Host".into());
    assert!(!g.story_gate(FormId(CASHIER_REF)));
}
