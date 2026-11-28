//! The menu model on a synthetic plugin built in memory and decoded through the
//! repository schemas (no game data). The builders follow
//! crates/fnv-sim/tests/synthetic.rs.

#[path = "search_tests.rs"]
mod search_tests;

use super::*;
use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_esm::{LoadOrder, Plugin, Schema};
use bevy::app::{App, Update};
use bevy::state::app::AppExtStates;
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
    cat(&[sig, &(payload.len() as u32).to_le_bytes(), &flags.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload])
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

fn misc(fid: u32, edid: &str, full: &str, value: i32, weight: f32) -> Vec<u8> {
    let [e, f] = named(edid, full);
    record(b"MISC", 0, fid, &[e, f, sub(b"DATA", cat(&[&value.to_le_bytes(), &weight.to_le_bytes()]))])
}

fn quest_misc(fid: u32, edid: &str, full: &str) -> Vec<u8> {
    let [e, f] = named(edid, full);
    record(b"MISC", 0x400, fid, &[e, f, sub(b"DATA", cat(&[&0i32.to_le_bytes(), &0f32.to_le_bytes()]))])
}

fn weap(fid: u32, edid: &str, full: &str, value: i32, weight: f32) -> Vec<u8> {
    let [e, f] = named(edid, full);
    let data = cat(&[&value.to_le_bytes(), &100i32.to_le_bytes(), &weight.to_le_bytes(), &10i16.to_le_bytes(), &[12u8]]);
    record(b"WEAP", 0, fid, &[e, f, sub(b"DATA", data)])
}

fn cnto(item: u32, count: i32) -> Subrecord {
    sub(b"CNTO", cat(&[&item.to_le_bytes(), &count.to_le_bytes()]))
}

fn refr(fid: u32, base: u32) -> Vec<u8> {
    let data: Vec<u8> = [0f32; 6].iter().flat_map(|f| f.to_le_bytes()).collect();
    record(b"REFR", 0, fid, &[sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

const CAN: u32 = 0x100; // MISC "tin can", value 2, weight 1
const CAPS: u32 = 0x101; // MISC "Bottle Cap", value 1, weight 0
const ANVIL: u32 = 0x102; // MISC "anvil", value 1, weight 250
const PISTOL: u32 = 0x170; // WEAP "Pistol", value 100, weight 3
const RIFLE: u32 = 0x171; // WEAP "Varmint Rifle", value 300, weight 8
const SCOPE: u32 = 0x178; // IMOD "Rifle Scope", value 125: RIFLE's mod slot 2
const AID: u32 = 0x175; // ALCH "Aid", weight 0.5
const BULLETS: u32 = 0x176; // AMMO "Ammo", weight 0.1 (Hardcore only)
const CHIP: u32 = 0x177; // MISC "Platinum Chip" with the "Quest Item" header flag (bit 10)
const BOX: u32 = 0x120; // CONT "Cash Box": CAPS 200, CAN 2, RIFLE 1, ANVIL 1
const KEY_A: u32 = 0x180; // KEYM "Shack Key"
const KEY_B: u32 = 0x181; // KEYM "Vault Key"
const SKILL_BOOK: u32 = 0x182; // BOOK "Big Book of Science", teaches Science
const PLAIN_BOOK: u32 = 0x183; // BOOK "Paradise Lost", teaches nothing
const CARD: u32 = 0x184; // CCRD "Ace of Hearts" (no DATA here)
const VEST: u32 = 0x185; // ARMO "Leather Vest", upper body
const KEY_VALUED: u32 = 0x186; // KEYM "Valued Key", non-quest, value 50
const CELL: u32 = 0x200;
const BOX_REF: u32 = 0x300;
const CAN_REF: u32 = 0x301;
const CHIP_LOOSE_REF: u32 = 0x302; // CHIP lying loose, persistent, owned by CAN (#3350)
const BOOK_LOOSE_REF: u32 = 0x303; // PLAIN_BOOK lying loose, not persistent, owned by CAN
const BOOK_LOOSE_REF2: u32 = 0x304; // another PLAIN_BOOK, owned by CAN
const CAN_LOOSE_REF: u32 = 0x305; // CAN lying loose, owned by no one

fn owned_refr(fid: u32, base: u32, owner: u32) -> Vec<u8> {
    let data: Vec<u8> = [0f32; 6].iter().flat_map(|f| f.to_le_bytes()).collect();
    record(b"REFR", 0, fid, &[sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"XOWN", owner.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn plugin_bytes() -> Vec<u8> {
    let tes4 = record(b"TES4", 1, 0, &[sub(b"HEDR", cat(&[&1.34f32.to_le_bytes(), &0i32.to_le_bytes(), &0x800u32.to_le_bytes()]))]);
    let miscs = cat(&[
        &misc(CAN, "TestCan", "tin can", 2, 1.0),
        &misc(CAPS, "TestCaps", "Bottle Cap", 1, 0.0),
        &misc(ANVIL, "TestAnvil", "anvil", 1, 250.0),
        &quest_misc(CHIP, "TestChip", "Platinum Chip"),
    ]);
    // The rifle's mod slot 2 holds SCOPE (WMI2).
    let rifle = {
        let [e, f] = named("TestRifle", "Varmint Rifle");
        let data = cat(&[&300i32.to_le_bytes(), &100i32.to_le_bytes(), &8f32.to_le_bytes(), &10i16.to_le_bytes(), &[12u8]]);
        record(b"WEAP", 0, RIFLE, &[e, f, sub(b"WMI2", SCOPE.to_le_bytes().to_vec()), sub(b"DATA", data)])
    };
    let weaps = cat(&[&weap(PISTOL, "TestPistol", "Pistol", 100, 3.0), &rifle]);
    let [e, f] = named("TestScope", "Rifle Scope");
    let imod = record(b"IMOD", 0, SCOPE, &[e, f, sub(b"DATA", cat(&[&125i32.to_le_bytes(), &0f32.to_le_bytes()]))]);
    let enit = cat(&[&20i32.to_le_bytes(), &[0u8, 0, 0, 0], &0u32.to_le_bytes(), &0f32.to_le_bytes(), &0u32.to_le_bytes()]);
    let [e, f] = named("TestAid", "Aid");
    let alch = record(b"ALCH", 0, AID, &[e, f, sub(b"DATA", 0.5f32.to_le_bytes().to_vec()), sub(b"ENIT", enit)]);
    let ammo_data = cat(&[&1f32.to_le_bytes(), &[0u8, 0, 0, 0], &2i32.to_le_bytes(), &[10u8]]);
    let ammo_dat2 = cat(&[&1u32.to_le_bytes(), &0u32.to_le_bytes(), &0.1f32.to_le_bytes()]);
    let [e, f] = named("TestBullets", "Ammo");
    let ammo = record(b"AMMO", 0, BULLETS, &[e, f, sub(b"DATA", ammo_data), sub(b"DAT2", ammo_dat2)]);
    let [e, f] = named("TestBox", "Cash Box");
    let cont = record(
        b"CONT",
        0,
        BOX,
        &[e, f, cnto(CAPS, 200), cnto(CAN, 2), cnto(RIFLE, 1), cnto(ANVIL, 1), sub(b"DATA", cat(&[&[0u8], &10f32.to_le_bytes()]))],
    );
    // S.P.E.C.I.A.L. after base health: Strength 5.
    let npc_data = cat(&[&100i32.to_le_bytes(), &[5u8, 6, 7, 4, 3, 2, 1]]);
    let npc = record(
        b"NPC_",
        0,
        0x7,
        &[sub(b"EDID", z("Player")), cnto(CAPS, 5), cnto(PISTOL, 1), cnto(AID, 2), cnto(BULLETS, 10), sub(b"DATA", npc_data)],
    );
    let gmst = cat(&[
        &record(b"GMST", 0, 0x160, &[sub(b"EDID", z("fAVDCarryWeightsBase")), sub(b"DATA", 150f32.to_le_bytes().to_vec())]),
        &record(b"GMST", 0, 0x162, &[sub(b"EDID", z("fAVDCarryWeightMult")), sub(b"DATA", 10f32.to_le_bytes().to_vec())]),
    ]);
    let key = |fid: u32, edid: &str, full: &str, value: i32| {
        let [e, f] = named(edid, full);
        record(b"KEYM", 0, fid, &[e, f, sub(b"DATA", cat(&[&value.to_le_bytes(), &0f32.to_le_bytes()]))])
    };
    let keys = cat(&[&key(KEY_A, "TestKeyA", "Shack Key", 0), &key(KEY_B, "TestKeyB", "Vault Key", 0), &key(KEY_VALUED, "TestKeyValued", "Valued Key", 50)]);
    // BOOK DATA: flags, skill (-1 none, 10 Science), value, weight.
    let book = |fid: u32, edid: &str, full: &str, skill: i8| {
        let [e, f] = named(edid, full);
        record(b"BOOK", 0, fid, &[e, f, sub(b"DATA", cat(&[&[0u8, skill as u8], &10i32.to_le_bytes(), &1f32.to_le_bytes()]))])
    };
    let books = cat(&[&book(SKILL_BOOK, "TestSkillBook", "Big Book of Science", 10), &book(PLAIN_BOOK, "TestPlainBook", "Paradise Lost", -1)]);
    let [e, f] = named("TestCard", "Ace of Hearts");
    let card = record(b"CCRD", 0, CARD, &[e, f]);
    // ARMO BMDT: biped flags (upper body), general flags; DATA: value, health, weight.
    let [e, f] = named("TestVest", "Leather Vest");
    let vest = record(b"ARMO", 0, VEST, &[e, f, sub(b"BMDT", cat(&[&0x4u32.to_le_bytes(), &[0u8, 0, 0, 0]])), sub(b"DATA", cat(&[&10i32.to_le_bytes(), &50i32.to_le_bytes(), &3f32.to_le_bytes()]))]);
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
    let label = CELL.to_le_bytes();
    let persistent = group(label, 8, &owned_refr(CHIP_LOOSE_REF, CHIP, CAN));
    let temporary = group(label, 9, &cat(&[&refr(BOX_REF, BOX), &refr(CAN_REF, CAN), &owned_refr(BOOK_LOOSE_REF, PLAIN_BOOK, CAN), &owned_refr(BOOK_LOOSE_REF2, PLAIN_BOOK, CAN), &refr(CAN_LOOSE_REF, CAN)]));
    let children = group(label, 6, &cat(&[&persistent, &temporary]));
    let cells = top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cat(&[&cell, &children]))));
    cat(&[
        &tes4,
        &top(b"GMST", &gmst),
        &top(b"MISC", &miscs),
        &top(b"CONT", &cont),
        &top(b"WEAP", &weaps),
        &top(b"IMOD", &imod),
        &top(b"ALCH", &alch),
        &top(b"AMMO", &ammo),
        &top(b"KEYM", &keys),
        &top(b"BOOK", &books),
        &top(b"CCRD", &card),
        &top(b"ARMO", &vest),
        &top(b"NPC_", &npc),
        &cells,
    ])
}

struct Fixture {
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Test.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let s = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    Fixture { forms: FormStore::build(&lo, &s), refs: RefTable::build(&lo, &s) }
}

fn names(l: &Listing) -> Vec<(&str, i32)> {
    l.rows.iter().map(|r| (r.name.as_str(), r.count)).collect()
}

fn count(w: &mut World<'_>, of: u32, item: u32) -> i32 {
    w.item_count(FormId(of), FormId(item)).unwrap()
}

fn select(m: &mut MenuModel, side: Side, name: &str) {
    let i = m.list(side).rows.iter().position(|r| r.name == name).unwrap_or_else(|| panic!("no row {name}"));
    m.focus = side;
    m.selected[side as usize] = i;
}

#[test]
fn rows_are_sorted_by_name_with_totals() {
    let f = fixture();
    assert!(f.forms.issues.is_empty(), "{:?}", f.forms.issues.to_json());
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    assert_eq!(m.title, "Cash Box");
    assert_eq!(m.focus, Side::Container);
    // Byte order, as the original lists them: capitals before small letters.
    assert_eq!(names(m.list(Side::Container)), vec![("Bottle Cap", 200), ("Varmint Rifle", 1), ("anvil", 1), ("tin can", 2)]);
    let c = m.list(Side::Container);
    assert_eq!((c.weight, c.value), (260.0, 200 + 4 + 300 + 1));
    let rifle = &c.rows[1];
    assert_eq!((rifle.weight, rifle.value, rifle.kind, rifle.equipped), (Some(8.0), Some(300), Some(ItemKind::Weapon), false));
    assert_eq!(names(m.list(Side::Player)), vec![("Aid", 2), ("Ammo", 10), ("Bottle Cap", 5), ("Pistol", 1)]);
    // Ammunition weighs nothing outside Hardcore mode (the simulation's rule).
    let p = m.list(Side::Player);
    assert_eq!(p.weight, 3.0 + 1.0);
    let e = m.encumbrance.as_ref().unwrap();
    assert_eq!((e.weight, e.capacity, e.over), (4.0, 200.0, false));
}

#[test]
fn take_and_store_through_the_sim() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    select(&mut m, Side::Container, "tin can");
    let a = m.apply(&mut w, Command::One);
    assert!(a.changed && !a.close);
    assert_eq!((count(&mut w, BOX_REF, CAN), count(&mut w, 0x14, CAN)), (1, 1));
    assert_eq!(m.status.as_deref(), Some("Took tin can x1"));
    // The selection stays on the item; the player's column shows it.
    assert_eq!(m.selected_row(Side::Container).unwrap().name, "tin can");
    assert!(names(m.list(Side::Player)).contains(&("tin can", 1)));
    // The rest of the stack: the row goes, the selection moves to the row before (it was
    // the last).
    m.apply(&mut w, Command::Stack);
    assert_eq!(count(&mut w, BOX_REF, CAN), 0);
    assert_eq!(names(m.list(Side::Container)), vec![("Bottle Cap", 200), ("Varmint Rifle", 1), ("anvil", 1)]);
    assert_eq!(m.selected_row(Side::Container).unwrap().name, "anvil");
    // Store a stack from the player.
    m.apply(&mut w, Command::Focus(Side::Player));
    select(&mut m, Side::Player, "Bottle Cap");
    m.apply(&mut w, Command::Stack);
    assert_eq!((count(&mut w, BOX_REF, CAPS), count(&mut w, 0x14, CAPS)), (205, 0));
    assert_eq!(m.status.as_deref(), Some("Stored Bottle Cap x5"));
}

#[test]
fn take_all_and_encumbrance() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    let a = m.apply(&mut w, Command::TakeAll);
    assert!(a.changed);
    // Taking everything closes the menu (owner, list 5 item 60).
    assert!(a.close);
    assert_eq!(m.status.as_deref(), Some("Took everything (204 items)"));
    assert!(m.list(Side::Container).rows.is_empty());
    assert_eq!(count(&mut w, 0x14, CAPS), 205);
    let e = m.encumbrance.as_ref().unwrap();
    assert_eq!((e.weight, e.over), (264.0, true));
    // Empty column: commands on it do nothing harmful.
    let a = m.apply(&mut w, Command::TakeAll);
    assert!(!a.changed && !a.close);
    assert_eq!(m.status.as_deref(), Some("Nothing to take"));
    m.apply(&mut w, Command::Focus(Side::Container));
    m.apply(&mut w, Command::One);
    assert_eq!(m.status.as_deref(), Some("Nothing selected"));
    m.apply(&mut w, Command::Down);
    assert_eq!(m.selected, [0, m.selected[1]]);
    assert!(m.apply(&mut w, Command::Close).close);
}

#[test]
fn a_live_explosive_is_a_row_of_its_own_that_stays() {
    // #1640: a planted explosive's unit (`Extra::live`) is listed apart, marked, and no
    // command takes it; the ordinary units of the same item are taken as usual.
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let inv = w.inventory_mut(FormId(BOX_REF)).unwrap();
    inv.add(FormId(RIFLE), 1);
    assert!(inv.mark_live(FormId(RIFLE)));
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    let armed = super::live_name("Varmint Rifle".into());
    assert_eq!(armed, format!("Varmint Rifle ({})", super::LIVE_NOTE));
    assert_eq!(names(m.list(Side::Container)), vec![("Bottle Cap", 200), ("Varmint Rifle", 1), (armed.as_str(), 1), ("anvil", 1), ("tin can", 2)]);
    assert!(m.list(Side::Container).rows[2].live && !m.list(Side::Container).rows[1].live);
    // Refused, nothing moves; the selection stays on it.
    select(&mut m, Side::Container, &armed);
    let a = m.apply(&mut w, Command::Stack);
    assert!(!a.changed && a.taken.is_empty());
    assert_eq!(m.status.as_deref(), Some(super::live_refusal(&armed).as_str()));
    assert_eq!((count(&mut w, BOX_REF, RIFLE), count(&mut w, 0x14, RIFLE)), (2, 0));
    assert_eq!(m.selected_row(Side::Container).unwrap().name, armed);
    // The ordinary rifle is taken; Take All leaves the live one.
    select(&mut m, Side::Container, "Varmint Rifle");
    m.apply(&mut w, Command::One);
    assert_eq!((count(&mut w, BOX_REF, RIFLE), count(&mut w, 0x14, RIFLE)), (1, 1));
    m.apply(&mut w, Command::TakeAll);
    assert_eq!(names(m.list(Side::Container)), vec![(armed.as_str(), 1)]);
    assert_eq!(count(&mut w, BOX_REF, RIFLE), 1);
    // Storing the player's rifle back adds an ordinary row beside it.
    m.apply(&mut w, Command::Focus(Side::Player));
    select(&mut m, Side::Player, "Varmint Rifle");
    m.apply(&mut w, Command::One);
    assert_eq!(names(m.list(Side::Container)), vec![("Varmint Rifle", 1), (armed.as_str(), 1)]);
}

/// A pocket menu on the box (steal mode) whose rolls always go unnoticed (`sure`) or are
/// always caught.
fn pocket(world: &mut World<'_>, sure: bool) -> MenuModel {
    let mut m = MenuModel::open_container(world, FormId(BOX_REF)).unwrap();
    m.steal = Some(Steal::new(FormId(BOX_REF), rolls(sure), 0.0, 0.0, 7));
    m
}

fn rolls(sure: bool) -> fnv_game::pickpocket::Rules {
    let c = if sure { 100.0 } else { 0.0 };
    fnv_game::pickpocket::Rules { min_chance: c, max_chance: c, ..Default::default() }
}

#[test]
fn an_explosive_armed_in_the_open_pocket_is_listed_and_refused_at_once() {
    // #1640 (review): the menu moves the item as an ordinary one; arming it
    // (`pickpocket::arm_given`, as `menu::apply_input` runs it right after the command)
    // lists the pocket again, so the next command sees the "(Armed)" row and a take of it
    // is refused before any roll.
    let f: &'static Fixture = Box::leak(Box::new(fixture()));
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Test.esm".into()).unwrap()];
    let mut script = crate::script::ScriptState::over_plugins(&plugins);
    let rt = script.rt.as_mut().unwrap();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut sim = crate::sim::SimState { forms: &f.forms, refs: &f.refs, world };
    let mut m = pocket(&mut sim.world, true);
    m.apply(&mut sim.world, Command::Focus(Side::Player));
    select(&mut m, Side::Player, "Pistol");
    let a = m.apply(&mut sim.world, Command::One);
    assert_eq!(a.given, vec![(FormId(PISTOL), 1)]);
    assert!(names(m.list(Side::Container)).contains(&("Pistol", 1)), "listed as an ordinary item first");
    let blast = fnv_game::pickpocket::Explosive { damage: 50.0, ..Default::default() };
    let explosion = FormId(0x999);
    let placed = crate::pickpocket::arm_given(rt, &mut sim, Some(&mut m), |item| (item == FormId(PISTOL)).then_some((explosion, blast)), FormId(BOX_REF), &a.given);
    assert_eq!(placed, vec![crate::pickpocket::Placed::Live(explosion)]);
    assert_eq!(rt.game.pickpocket.planted.len(), 1);
    let armed = super::live_name("Pistol".into());
    let listed = names(m.list(Side::Container));
    assert!(listed.contains(&(armed.as_str(), 1)) && !listed.contains(&("Pistol", 1)), "{listed:?}");
    // Taking it back: refused with no roll (every roll would be caught now).
    m.steal.as_mut().unwrap().rules = rolls(false);
    m.apply(&mut sim.world, Command::Focus(Side::Container));
    select(&mut m, Side::Container, &armed);
    let a = m.apply(&mut sim.world, Command::One);
    assert!(a.caught.is_none() && !a.close && a.taken.is_empty(), "{a:?}");
    assert_eq!(m.status.as_deref(), Some(super::live_refusal(&armed).as_str()));
    assert_eq!(count(&mut sim.world, BOX_REF, PISTOL), 1);
    // A placement that is no explosive is listed as before.
    let placed = crate::pickpocket::arm_given(rt, &mut sim, Some(&mut m), |_| None, FormId(BOX_REF), &[(FormId(CAN), 1)]);
    assert_eq!(placed, vec![crate::pickpocket::Placed::Plain]);
}

#[test]
fn a_pocket_row_listed_before_its_arming_is_not_rolled_for() {
    // A row the menu listed as ordinary, its unit since reserved: nothing to move, so no
    // roll and no catch.
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = pocket(&mut w, false);
    assert!(w.inventory_mut(FormId(BOX_REF)).unwrap().mark_live(FormId(RIFLE)));
    select(&mut m, Side::Container, "Varmint Rifle");
    let a = m.apply(&mut w, Command::One);
    assert!(a.caught.is_none() && !a.close && a.taken.is_empty(), "{a:?}");
    assert_eq!(m.status.as_deref(), Some(super::live_refusal(&super::live_name("Varmint Rifle".into())).as_str()));
    assert_eq!(count(&mut w, BOX_REF, RIFLE), 1);
}

#[test]
fn stored_equipped_items_are_unequipped() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.equip(FormId::PLAYER_REF, FormId(PISTOL)).unwrap();
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    assert!(m.list(Side::Player).rows.iter().any(|r| r.name == "Pistol" && r.equipped));
    select(&mut m, Side::Player, "Pistol");
    m.apply(&mut w, Command::One);
    let row = m.list(Side::Container).rows.iter().find(|r| r.name == "Pistol").unwrap();
    assert!(!row.equipped);
}

#[test]
fn inventory_equip_use_drop_and_errors() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_inventory(&mut w).unwrap();
    assert_eq!(m.mode, Mode::Inventory);
    assert!(m.list(Side::Container).rows.is_empty());
    // Focus cannot move to a column that is not there.
    m.apply(&mut w, Command::Focus(Side::Container));
    assert_eq!(m.focus, Side::Player);
    select(&mut m, Side::Player, "Pistol");
    let a = m.apply(&mut w, Command::One);
    assert_eq!(m.status.as_deref(), Some("Equipped Pistol"));
    assert_eq!(a.used, None);
    assert!(m.selected_row(Side::Player).unwrap().equipped);
    m.apply(&mut w, Command::One);
    assert_eq!(m.status.as_deref(), Some("Unequipped Pistol"));
    assert!(!m.selected_row(Side::Player).unwrap().equipped);
    // Not equippable: the error is shown, nothing changes.
    select(&mut m, Side::Player, "Bottle Cap");
    m.apply(&mut w, Command::One);
    assert_eq!(m.status.as_deref(), Some("Cannot equip or use Bottle Cap"));
    assert_eq!(count(&mut w, 0x14, CAPS), 5);
    // Aid is used up.
    select(&mut m, Side::Player, "Aid");
    let a = m.apply(&mut w, Command::One);
    assert_eq!(m.status.as_deref(), Some("Used Aid"));
    // Its effects (and effect scripts) go to the script runtime.
    assert_eq!(a.used, Some(FormId(AID)));
    assert_eq!(count(&mut w, 0x14, AID), 1);
    // Drops create references at the player. Ten rounds ask how many first (the prompt
    // starts at the whole stack); one is chosen.
    select(&mut m, Side::Player, "Ammo");
    let a = m.apply(&mut w, Command::Drop);
    assert!(a.created.is_empty());
    assert_eq!(m.quantity.map(|q| (q.amount, q.max)), Some((10, 10)));
    m.apply(&mut w, Command::Amount(1));
    let a = m.apply(&mut w, Command::AmountOk);
    assert_eq!(m.quantity, None);
    assert_eq!(a.created.len(), 1);
    assert!(a.created[0].is_dynamic());
    assert_eq!(count(&mut w, 0x14, BULLETS), 9);
    let a = m.apply(&mut w, Command::DropStack);
    assert_eq!(count(&mut w, 0x14, BULLETS), 0);
    assert_eq!(m.dropped.len(), 2);
    assert_eq!(w.peek(a.created[0]).unwrap().count, 9);
    // Container commands in the inventory menu say so.
    m.apply(&mut w, Command::TakeAll);
    assert_eq!(m.status.as_deref(), Some("No container open"));
}

#[test]
fn opening_something_without_an_inventory_fails() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    assert_eq!(MenuModel::open_container(&mut w, FormId(CAN_REF)).unwrap_err(), SimError::NoInventory(FormId(CAN_REF)));
    assert_eq!(MenuModel::open_container(&mut w, FormId(0x999)).unwrap_err(), SimError::UnknownRef(FormId(0x999)));
}

#[test]
fn mouse_select_then_click_again_moves_one() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    let a = m.apply(&mut w, Command::Select(Side::Player, 3));
    assert!(!a.changed);
    assert_eq!((m.focus, m.selected[1]), (Side::Player, 3));
    let a = m.apply(&mut w, Command::Select(Side::Player, 3));
    assert!(a.changed);
    assert_eq!(count(&mut w, BOX_REF, PISTOL), 1);
    // Out of range: ignored.
    assert_eq!(m.apply(&mut w, Command::Select(Side::Container, 99)), Applied::default());
}

#[test]
fn selection_scrolls_into_view() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_inventory(&mut w).unwrap();
    let row = m.list(Side::Player).rows[0].clone();
    m.lists[1].rows = (0..40).map(|i| Row { name: format!("{i:02}"), ..row.clone() }).collect();
    for _ in 0..20 {
        m.apply(&mut w, Command::Down);
    }
    assert_eq!((m.selected[1], m.scroll[1]), (20, 20 + 1 - VISIBLE_ROWS));
    assert_eq!(m.visible(Side::Player), 21 - VISIBLE_ROWS..21);
    // A page down moves by a page less one row, and stops at the last row.
    m.apply(&mut w, Command::PageDown);
    assert_eq!(m.selected[1], 20 + VISIBLE_ROWS - 1);
    while m.selected[1] < 39 {
        m.apply(&mut w, Command::PageDown);
    }
    m.apply(&mut w, Command::PageDown);
    assert_eq!((m.selected[1], m.scroll[1]), (39, 40 - VISIBLE_ROWS));
    m.apply(&mut w, Command::Scroll(-30));
    assert_eq!((m.selected[1], m.scroll[1]), (9, 9));
    m.apply(&mut w, Command::PageUp);
    m.apply(&mut w, Command::PageUp);
    assert_eq!((m.selected[1], m.scroll[1]), (0, 0));
}

#[test]
fn nearest_container_in_an_interior() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let near = nearest_container(&w, &f.refs, Some(FormId(CELL)), FormId(0), [3.0, 4.0, 0.0]);
    assert_eq!(near, Some((FormId(BOX_REF), 5.0)));
    w.set_enabled(FormId(BOX_REF), false).unwrap();
    assert_eq!(nearest_container(&w, &f.refs, Some(FormId(CELL)), FormId(0), [0.0; 3]), None);
    assert_eq!(nearest_container(&w, &f.refs, None, FormId(0x400), [0.0; 3]), None);
}

#[test]
fn non_playable_items_are_not_listed_and_stay_in_containers() {
    let mut f = fixture();
    // The anvil stands in for a non-playable item (like the Pip-Boy Glove).
    match f.forms.bases.get_mut(&FormId(ANVIL)).map(|b| &mut b.data) {
        Some(BaseData::Item(i)) => i.non_playable = true,
        other => panic!("anvil: {other:?}"),
    }
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    assert!(m.list(Side::Container).rows.iter().all(|r| r.item != FormId(ANVIL)));
    m.apply(&mut w, Command::TakeAll);
    assert_eq!(m.status.as_deref(), Some("Took everything (203 items)"));
    assert!(m.list(Side::Container).rows.is_empty(), "nothing visible is left");
    assert_eq!(count(&mut w, BOX_REF, ANVIL), 1, "the hidden item stays");
    assert_eq!(count(&mut w, 0x14, ANVIL), 0);
    // Carried, it is still not listed, so it cannot be dropped from the inventory.
    w.add_item(FormId::PLAYER_REF, FormId(ANVIL), 1).unwrap();
    let mut inv = MenuModel::open_inventory(&mut w).unwrap();
    assert!(inv.list(Side::Player).rows.iter().all(|r| r.item != FormId(ANVIL)));
    inv.apply(&mut w, Command::DropStack);
    assert_eq!(count(&mut w, 0x14, ANVIL), 1);
}

#[test]
fn nameless_items_are_listed_nowhere() {
    // #1603: DONOTUSEBookGeneric07 has no FULL; its row read as the Editor ID.
    let mut f = fixture();
    let anvil = f.forms.bases.get_mut(&FormId(ANVIL)).unwrap();
    assert!(anvil.edid.is_some(), "the fixture's anvil has an Editor ID to fall back on");
    anvil.name = None;
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    assert!(m.list(Side::Container).rows.iter().all(|r| r.item != FormId(ANVIL)));
    m.apply(&mut w, Command::TakeAll);
    assert_eq!(count(&mut w, BOX_REF, ANVIL), 1, "the nameless item stays");
    // Carried, it is not listed either, the player's inventory included.
    w.add_item(PLAYER, FormId(ANVIL), 1).unwrap();
    let inv = MenuModel::open_inventory(&mut w).unwrap();
    assert!(inv.list(Side::Player).rows.iter().all(|r| r.item != FormId(ANVIL)));
    // A blank name is no name.
    f.forms.bases.get_mut(&FormId(ANVIL)).unwrap().name = Some("  ".into());
    let w = World::new(&f.forms, &f.refs, SimConfig::default());
    assert!(!crate::menu::model::shown(w.forms, FormId(ANVIL), true));
    assert!(crate::menu::model::shown(w.forms, FormId(RIFLE), false), "a named item is shown");
}

#[test]
fn non_playable_weapons_show_only_in_the_players_inventory() {
    let mut f = fixture();
    // The rifle stands in for a companion's non-playable weapon (Boone's rifle).
    match f.forms.bases.get_mut(&FormId(RIFLE)).map(|b| &mut b.data) {
        Some(BaseData::Item(i)) => i.non_playable = true,
        other => panic!("rifle: {other:?}"),
    }
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    assert!(m.list(Side::Container).rows.iter().all(|r| r.item != FormId(RIFLE)));
    m.apply(&mut w, Command::TakeAll);
    assert_eq!(count(&mut w, BOX_REF, RIFLE), 1, "the hidden rifle stays");
    // The player's own non-playable weapon is listed (GECK wiki "Unplayable Items").
    w.add_item(PLAYER, FormId(RIFLE), 1).unwrap();
    let inv = MenuModel::open_inventory(&mut w).unwrap();
    assert!(inv.list(Side::Player).rows.iter().any(|r| r.item == FormId(RIFLE)));
}

#[test]
fn quest_items_cannot_be_dropped_but_can_be_stored() {
    let f = fixture();
    assert!(f.forms.issues.is_empty(), "{:?}", f.forms.issues.to_json());
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.add_item(PLAYER, FormId(CHIP), 1).unwrap();
    let mut m = MenuModel::open_inventory(&mut w).unwrap();
    select(&mut m, Side::Player, "Platinum Chip");
    assert!(m.selected_row(Side::Player).unwrap().quest);
    // Both drop commands are refused with our status line; nothing moves.
    for cmd in [Command::Drop, Command::DropStack] {
        let a = m.apply(&mut w, cmd);
        assert_eq!(m.status.as_deref(), Some("Platinum Chip is a quest item and can't be dropped"));
        assert!(a.created.is_empty() && !a.changed);
        assert_eq!(count(&mut w, 0x14, CHIP), 1);
    }
    assert!(m.dropped.is_empty());
    // A script's SetQuestObject counts the same way, both ways.
    w.set_quest_object(FormId(CAPS), true);
    w.set_quest_object(FormId(CHIP), false);
    m.refresh(&mut w).unwrap();
    select(&mut m, Side::Player, "Bottle Cap");
    assert!(m.selected_row(Side::Player).unwrap().quest);
    m.apply(&mut w, Command::Drop);
    assert_eq!(m.status.as_deref(), Some("Bottle Cap is a quest item and can't be dropped"));
    select(&mut m, Side::Player, "Platinum Chip");
    assert!(!m.selected_row(Side::Player).unwrap().quest);
    m.apply(&mut w, Command::Drop);
    assert_eq!(m.status.as_deref(), Some("Dropped Platinum Chip x1"));
    assert_eq!(count(&mut w, 0x14, CHIP), 0);
    // Storing a quest item in a container is allowed [hypothesis].
    let mut c = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    select(&mut c, Side::Player, "Bottle Cap");
    c.apply(&mut w, Command::One);
    assert_eq!(c.status.as_deref(), Some("Stored Bottle Cap x1"));
    assert_eq!(count(&mut w, 0x14, CAPS), 4);
}

fn barter(w: &mut World<'_>) -> MenuModel {
    // The cash box stands in for both the merchant and its stock; Barter 100.
    MenuModel::open_barter(w, FormId(BOX_REF), FormId(BOX_REF), FormId(CAPS), 100.0, 0).unwrap()
}

fn prices(l: &Listing) -> Vec<(&str, i32, String, bool)> {
    l.rows.iter().map(|r| (r.name.as_str(), r.count, r.price.map_or("?".into(), barter::price_text), r.bartered)).collect()
}

#[test]
fn barter_lists_prices_without_caps_or_quest_items() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.add_item(PLAYER, FormId(CHIP), 1).unwrap();
    let m = barter(&mut w);
    assert_eq!((m.mode, m.focus, m.title.as_str()), (Mode::Barter(FormId(BOX_REF)), Side::Player, "Cash Box"));
    // Selling at 90 %, buying at 110 %, rounded; the caps are in the headings only.
    let s = |n: &str, c: i32, p: &str| (n.to_string(), c, p.to_string(), false);
    let own = |l: &Listing| prices(l).into_iter().map(|(n, c, p, b)| (n.to_string(), c, p, b)).collect::<Vec<_>>();
    assert_eq!(own(m.list(Side::Player)), vec![s("Aid", 2, "18"), s("Ammo", 10, "2"), s("Pistol", 1, "90")]);
    assert_eq!(own(m.list(Side::Container)), vec![s("Varmint Rifle", 1, "330"), s("anvil", 1, "1"), s("tin can", 2, "2")]);
    let t = m.trade.as_ref().unwrap();
    assert_eq!((t.purse, t.balance), ([200, 5], 0));
}

/// A merchant's barter menu offers the items it owns lying loose in its cell, persistent
/// or not, quest items too, one row per item; buying takes the references out of the
/// world into the player's inventory [observed: observer check 256 and its follow-up,
/// Blake's two books and his eight beers, #3350]. A loose item it does not own is not
/// offered.
#[test]
fn barter_offers_the_merchants_owned_loose_items() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.add_item(PLAYER, FormId(CAPS), 100).unwrap();
    // The tin can stands in for a merchant owning them (the cash box its stock).
    let merchant = |w: &mut World<'_>| MenuModel::open_barter(w, FormId(CAN_REF), FormId(BOX_REF), FormId(CAPS), 100.0, 0).unwrap();
    assert_eq!(owned_loose(&w, FormId(CAN_REF)), vec![FormId(CHIP_LOOSE_REF), FormId(BOOK_LOOSE_REF), FormId(BOOK_LOOSE_REF2)]);
    assert!(owned_loose(&w, FormId(BOX_REF)).is_empty());
    let mut m = merchant(&mut w);
    let rows = names(m.list(Side::Container));
    assert!(rows.contains(&("Platinum Chip", 1)), "a quest item too: {rows:?}");
    assert!(rows.contains(&("Paradise Lost", 2)), "two references, one row: {rows:?}");
    // The stock's two cans; the loose can is no one's.
    assert!(rows.contains(&("tin can", 2)), "{rows:?}");
    select(&mut m, Side::Container, "Platinum Chip");
    m.apply(&mut w, Command::One);
    select(&mut m, Side::Container, "Paradise Lost");
    m.apply(&mut w, Command::One);
    m.apply(&mut w, Command::One);
    m.apply(&mut w, Command::Accept);
    assert!(m.status.as_deref().is_some_and(|s| s.starts_with("Traded")), "{:?}", m.status);
    assert_eq!((count(&mut w, 0x14, CHIP), count(&mut w, 0x14, PLAIN_BOOK)), (1, 2));
    assert_eq!((count(&mut w, BOX_REF, CHIP), count(&mut w, BOX_REF, PLAIN_BOOK)), (0, 0));
    for r in [CHIP_LOOSE_REF, BOOK_LOOSE_REF, BOOK_LOOSE_REF2] {
        assert!(w.peek(FormId(r)).is_some_and(|s| s.deleted()), "{r:#x} left the world");
    }
    assert!(w.peek(FormId(CAN_LOOSE_REF)).is_none_or(|s| !s.deleted()));
    let m = merchant(&mut w);
    let rows = names(m.list(Side::Container));
    assert!(!rows.iter().any(|(n, _)| *n == "Platinum Chip" || *n == "Paradise Lost"), "{rows:?}");
}

#[test]
fn barter_put_up_take_back_and_accept() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.equip(PLAYER, FormId(PISTOL)).unwrap();
    let mut m = barter(&mut w);
    // Nothing put up: nothing to accept.
    m.apply(&mut w, Command::Accept);
    assert_eq!(m.status.as_deref(), Some("Nothing to trade"));
    // Sell the (equipped) pistol: it moves to the top of the merchant's list, marked.
    select(&mut m, Side::Player, "Pistol");
    m.apply(&mut w, Command::One);
    assert!(m.list(Side::Player).rows.iter().all(|r| r.item != FormId(PISTOL)));
    assert_eq!(prices(m.list(Side::Container))[0], ("Pistol", 1, "90".into(), true));
    assert_eq!(m.trade.as_ref().unwrap().balance, 90);
    // Nothing has moved yet.
    assert_eq!((count(&mut w, 0x14, PISTOL), count(&mut w, 0x14, CAPS)), (1, 5));
    m.apply(&mut w, Command::Accept);
    assert_eq!(m.status.as_deref(), Some("Traded: received 90 caps"));
    assert_eq!((count(&mut w, 0x14, PISTOL), count(&mut w, BOX_REF, PISTOL)), (0, 1));
    assert_eq!((count(&mut w, 0x14, CAPS), count(&mut w, BOX_REF, CAPS)), (95, 110));
    let t = m.trade.as_ref().unwrap();
    assert!(t.is_empty());
    assert_eq!((t.purse, t.balance), ([110, 95], 0));
    assert!(t.affordable());
    // Buy the rifle: 330 caps, more than the player has.
    select(&mut m, Side::Container, "Varmint Rifle");
    m.apply(&mut w, Command::One);
    assert_eq!(prices(m.list(Side::Player))[0], ("Varmint Rifle", 1, "330".into(), true));
    assert_eq!(m.trade.as_ref().unwrap().balance, -330);
    assert!(!m.trade.as_ref().unwrap().affordable());
    m.apply(&mut w, Command::Accept);
    assert_eq!(m.status.as_deref(), Some("You don't have enough caps."));
    assert_eq!(count(&mut w, 0x14, RIFLE), 0);
    // Take it back from the player's side; sell the aid and buy the cans instead.
    select(&mut m, Side::Player, "Varmint Rifle");
    m.apply(&mut w, Command::One);
    assert!(m.trade.as_ref().unwrap().is_empty());
    assert!(m.list(Side::Container).rows.iter().any(|r| r.name == "Varmint Rifle" && !r.bartered));
    select(&mut m, Side::Player, "Aid");
    m.apply(&mut w, Command::Stack);
    select(&mut m, Side::Container, "tin can");
    m.apply(&mut w, Command::Stack);
    assert_eq!(m.trade.as_ref().unwrap().balance, 36 - 4);
    assert!(m.trade.as_ref().unwrap().affordable());
    m.apply(&mut w, Command::Accept);
    assert_eq!(m.status.as_deref(), Some("Traded: received 32 caps"));
    assert_eq!((count(&mut w, 0x14, AID), count(&mut w, 0x14, CAN), count(&mut w, 0x14, CAPS)), (0, 2, 127));
}

#[test]
fn barter_tiles_show_the_models_sale_first_and_use_their_own_headings() {
    use crate::tiles::barter_menu::tests::of_model;
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = barter(&mut w);
    let before = of_model(&m);
    let root = "BarterMenu/NOGLOW_BRANCH";
    assert_eq!(before.get_string(&format!("{root}/BM_ItemsRect/BM_ItemsTitle/string")).as_deref(), Some("Personal items"));
    assert_eq!(before.get_string(&format!("{root}/BM_ItemsRect/BM_Items_CapsLabel/string")).as_deref(), Some("Coins   5"));
    assert_eq!(before.get_float(&format!("{root}/BM_ButtonRect/BM_ButtonX/target")), Some(0.0));
    select(&mut m, Side::Player, "Pistol");
    m.apply(&mut w, Command::One);
    let sold = of_model(&m);
    let offered = format!("{root}/BM_ContainerRect/BM_Container_InventoryList/Pistol");
    assert_eq!(sold.get_float(&format!("{offered}/listindex")), Some(0.0));
    assert_eq!(sold.get_string(&format!("{offered}/BM_list_template_ItemValue/string")).as_deref(), Some("90"));
    assert_eq!(sold.get_float(&format!("{offered}/BM_list_template_ItemMarker/visible")), Some(1.0));
    assert!(sold.find_tile(&format!("{root}/BM_ItemsRect/BM_Items_InventoryList/Pistol")).is_none());
    assert_eq!(sold.get_string(&format!("{root}/BM_CapsFlow/BM_TransactionAmount/string")).as_deref(), Some("90"));
    assert_eq!(sold.get_float(&format!("{root}/BM_ButtonRect/BM_ButtonX/target")), Some(1.0));
    m.apply(&mut w, Command::Accept);
    let accepted = of_model(&m);
    assert_eq!(accepted.get_string(&format!("{root}/BM_ItemsRect/BM_Items_CapsLabel/string")).as_deref(), Some("Coins   95"));
    assert_eq!(accepted.get_float(&format!("{root}/BM_ButtonRect/BM_ButtonX/target")), Some(0.0));
}

#[test]
fn a_merchant_without_enough_caps_pays_what_it_has() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.add_item(PLAYER, FormId(RIFLE), 1).unwrap();
    w.transfer(FormId(BOX_REF), PLAYER, FormId(CAPS), 150).unwrap();
    let mut m = barter(&mut w);
    select(&mut m, Side::Player, "Varmint Rifle");
    m.apply(&mut w, Command::One);
    let t = m.trade.as_ref().unwrap();
    let held = i64::from(t.purse[Side::Container as usize]);
    assert!(!t.affordable(), "the figure dims");
    assert!(t.balance > held, "the sale asks more than the merchant holds");
    assert_eq!(t.paid(), held, "the figure is held at the merchant's purse");
    let before = count(&mut w, 0x14, CAPS);
    m.apply(&mut w, Command::Accept);
    assert_eq!(m.status.as_deref(), Some(format!("Traded: received {held} caps").as_str()));
    assert_eq!(count(&mut w, 0x14, RIFLE), 0);
    assert_eq!(i64::from(count(&mut w, 0x14, CAPS)), i64::from(before) + held);
    assert_eq!(m.trade.as_ref().unwrap().purse[Side::Container as usize], 0);
}

#[test]
fn categories_filter_each_list() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    m.apply(&mut w, Command::Category(Side::Container, 1));
    assert_eq!(m.category[0], Category::Weapons);
    assert_eq!(names(m.list(Side::Container)), vec![("Varmint Rifle", 1)]);
    assert_eq!(m.filtered[Side::Container as usize], 3, "what the category leaves out");
    // The player's list keeps its own.
    assert_eq!(m.list(Side::Player).rows.len(), 4);
    m.apply(&mut w, Command::Category(Side::Container, -2));
    assert_eq!((m.category[0], m.category[0].title()), (Category::Ammo, "AMMO"));
    assert!(m.list(Side::Container).rows.is_empty());
    m.apply(&mut w, Command::Category(Side::Container, -1));
    assert_eq!(names(m.list(Side::Container)), vec![("Bottle Cap", 200), ("anvil", 1), ("tin can", 2)]);
    // Take All takes what is listed.
    m.apply(&mut w, Command::TakeAll);
    assert_eq!(count(&mut w, BOX_REF, RIFLE), 1);
    // The inventory menu has no categories.
    let mut inv = MenuModel::open_inventory(&mut w).unwrap();
    inv.apply(&mut w, Command::Category(Side::Player, 1));
    assert_eq!(inv.category[1], Category::All);
}

#[test]
fn the_wheel_over_a_heading_steps_its_category() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    m.apply(&mut w, Command::Hover(Side::Container, 2));
    // Down over the container's heading: the next category, the selection back at the top.
    m.apply(&mut w, Command::wheel(Some(Side::Container), -1));
    assert_eq!((m.category[0], m.selected[0], m.scroll[0]), (Category::Weapons, 0, 0));
    assert_eq!(m.category[1], Category::All, "the player's list keeps its own");
    // Up twice wraps round to the last, as the left arrow does.
    m.apply(&mut w, Command::wheel(Some(Side::Container), 2));
    assert_eq!(m.category[0], Category::Weapons.step(-2));
    // The player's heading.
    m.apply(&mut w, Command::wheel(Some(Side::Player), -1));
    assert_eq!(m.category[1], Category::Weapons);
    // Off the headings the wheel moves the selection as before.
    assert_eq!(Command::wheel(None, 1), Command::Scroll(-1));
    assert_eq!(Command::wheel(None, -3), Command::Scroll(3));
}

#[test]
fn hovering_selects_without_acting() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    let a = m.apply(&mut w, Command::Hover(Side::Player, 3));
    assert_eq!((a, m.focus, m.selected[1]), (Applied::default(), Side::Player, 3));
    // A click on the row under the pointer then acts at once.
    assert!(m.apply(&mut w, Command::Select(Side::Player, 3)).changed);
    assert_eq!(count(&mut w, BOX_REF, PISTOL), 1);
}

#[test]
fn the_pointer_never_scrolls_the_list() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    assert!(m.list(Side::Player).rows.len() >= 3);
    m.scroll[1] = 2;
    // A row out of the list's view under the pointer is selected; the list stays.
    m.apply(&mut w, Command::Hover(Side::Player, 0));
    assert_eq!((m.selected[1], m.scroll[1], m.pointer_pick), (0, 2, true));
    // The keys bring the selection back into view.
    m.apply(&mut w, Command::Down);
    assert_eq!((m.selected[1], m.pointer_pick), (1, false));
    assert!(m.visible(Side::Player).contains(&1));
}

#[test]
fn stacks_of_more_than_five_ask_how_many() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    // 200 caps: taking "one" asks first, starting at the whole stack.
    select(&mut m, Side::Container, "Bottle Cap");
    let a = m.apply(&mut w, Command::One);
    assert!(!a.changed);
    let q = m.quantity.unwrap();
    assert_eq!((q.cmd, q.side, q.max, q.amount), (Command::One, Side::Container, 200, 200));
    // While it is open nothing else acts: no move, no other row, no close.
    m.apply(&mut w, Command::Down);
    m.apply(&mut w, Command::TakeAll);
    assert_eq!(count(&mut w, BOX_REF, CAPS), 200);
    assert_eq!(m.selected_row(Side::Container).unwrap().name, "Bottle Cap");
    // The amount stays between 1 and the stack.
    m.apply(&mut w, Command::AmountStep(5));
    assert_eq!(m.quantity.unwrap().amount, 200);
    m.apply(&mut w, Command::Amount(0));
    assert_eq!(m.quantity.unwrap().amount, 1);
    m.apply(&mut w, Command::AmountStep(QUANTITY_JUMP));
    m.apply(&mut w, Command::AmountStep(1));
    assert_eq!(m.quantity.unwrap().amount, 12);
    let a = m.apply(&mut w, Command::AmountOk);
    assert!(a.changed && !a.close);
    assert_eq!(a.taken, vec![(FormId(CAPS), 12)]);
    assert_eq!((count(&mut w, BOX_REF, CAPS), count(&mut w, 0x14, CAPS)), (188, 17));
    assert_eq!(m.status.as_deref(), Some("Took Bottle Cap x12"));
    // Cancel (or Close) moves nothing and leaves the menu open.
    m.apply(&mut w, Command::One);
    assert!(!m.apply(&mut w, Command::Close).close);
    assert_eq!(m.quantity, None);
    m.apply(&mut w, Command::One);
    m.apply(&mut w, Command::AmountCancel);
    assert_eq!(count(&mut w, BOX_REF, CAPS), 188);
    // Storing a stack of 17 back asks too; Shift's whole stack never asks.
    select(&mut m, Side::Player, "Bottle Cap");
    m.apply(&mut w, Command::One);
    assert_eq!(m.quantity.map(|q| q.max), Some(17));
    m.apply(&mut w, Command::Amount(7));
    m.apply(&mut w, Command::AmountOk);
    assert_eq!(count(&mut w, BOX_REF, CAPS), 195);
    m.apply(&mut w, Command::Stack);
    assert_eq!((m.quantity, count(&mut w, BOX_REF, CAPS)), (None, 205));
    // Five or fewer move one at a time.
    select(&mut m, Side::Container, "tin can");
    m.apply(&mut w, Command::One);
    assert_eq!((m.quantity, count(&mut w, 0x14, CAN)), (None, 1));
}

#[test]
fn barter_and_drops_ask_how_many_too() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = barter(&mut w);
    // Sell 4 of the 10 rounds, then take 1 back from the 4 put up (5 or fewer: no prompt).
    select(&mut m, Side::Player, "Ammo");
    m.apply(&mut w, Command::One);
    m.apply(&mut w, Command::Amount(4));
    m.apply(&mut w, Command::AmountOk);
    assert_eq!(m.trade.as_ref().unwrap().offered[Side::Player as usize].get(&(FormId(BULLETS), None, 0)), Some(&4));
    assert_eq!(m.status.as_deref(), Some("Selling Ammo x4"));
    select(&mut m, Side::Container, "Ammo");
    assert!(m.selected_row(Side::Container).unwrap().bartered);
    m.apply(&mut w, Command::One);
    assert_eq!(m.quantity, None);
    assert_eq!(m.trade.as_ref().unwrap().offered[Side::Player as usize].get(&(FormId(BULLETS), None, 0)), Some(&3));
    // Inventory: drop 7 of the 10 rounds as one reference holding 7.
    let mut m = MenuModel::open_inventory(&mut w).unwrap();
    select(&mut m, Side::Player, "Ammo");
    m.apply(&mut w, Command::Drop);
    m.apply(&mut w, Command::Amount(7));
    let a = m.apply(&mut w, Command::AmountOk);
    assert_eq!(a.created.len(), 1);
    assert_eq!(w.peek(a.created[0]).unwrap().count, 7);
    assert_eq!(count(&mut w, 0x14, BULLETS), 3);
}

#[test]
fn the_meter_gives_a_part_of_the_stack() {
    assert_eq!(amount_at(1.0, 6), 6);
    assert_eq!(amount_at(0.5, 6), 3);
    assert_eq!(amount_at(0.0, 6), 1);
    assert_eq!(amount_at(1.4, 6), 6);
    assert_eq!(amount_at(0.66, 200), 132);
}

/// Six Varmint Rifles, one at 95 %: a row of five whole ones and below it a row of one,
/// and each row moves, equips and sells only its own units (owner, 2026-10-16).
#[test]
fn worn_units_list_apart_from_whole_ones() {
    use fnv_sim::inventory::Extra;
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.add_item(PLAYER, FormId(RIFLE), 5).unwrap();
    w.add_item_extra(PLAYER, FormId(RIFLE), 1, Extra { condition: Some(0.95), ..Extra::default() }).unwrap();
    let rifles = |m: &MenuModel, side: Side| -> Vec<(i32, Option<f32>, bool)> {
        m.list(side).rows.iter().filter(|r| r.item == FormId(RIFLE)).map(|r| (r.count, r.condition, r.bartered)).collect()
    };
    let worn = |m: &mut MenuModel, side: Side| {
        let i = m.list(side).rows.iter().position(|r| r.item == FormId(RIFLE) && r.condition == Some(0.95)).expect("a worn row");
        m.focus = side;
        m.selected[side as usize] = i;
    };
    let mut inv = MenuModel::open_inventory(&mut w).unwrap();
    assert_eq!(rifles(&inv, Side::Player), vec![(5, Some(1.0), false), (1, Some(0.95), false)]);
    // Equipping the worn row equips the worn rifle; the rows stay apart.
    worn(&mut inv, Side::Player);
    inv.apply(&mut w, Command::One);
    assert_eq!(inv.status.as_deref(), Some("Equipped Varmint Rifle"));
    let player = w.get(PLAYER).unwrap().inventory.clone().unwrap();
    assert_eq!(player.condition_in_use(FormId(RIFLE)), Some(0.95));
    assert!(inv.list(Side::Player).rows.iter().any(|r| r.item == FormId(RIFLE) && r.equipped && r.count == 1));
    // Storing it moves that one and no whole one; the box's own whole rifle stays apart.
    let mut c = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    worn(&mut c, Side::Player);
    c.apply(&mut w, Command::One);
    assert_eq!(rifles(&c, Side::Player), vec![(5, Some(1.0), false)]);
    assert_eq!(rifles(&c, Side::Container), vec![(1, Some(1.0), false), (1, Some(0.95), false)]);
    // Buying the worn one back costs less than a whole one and brings that one.
    let mut m = barter(&mut w);
    worn(&mut m, Side::Container);
    m.apply(&mut w, Command::One);
    assert_eq!(rifles(&m, Side::Player)[0], (1, Some(0.95), true));
    assert!(m.trade.as_ref().unwrap().balance > -330);
    w.add_item(PLAYER, FormId(CAPS), 400).unwrap();
    m.refresh(&mut w).unwrap();
    m.apply(&mut w, Command::Accept);
    assert!(m.status.as_deref().unwrap_or("").starts_with("Traded"), "{:?}", m.status);
    assert_eq!(rifles(&m, Side::Player), vec![(5, Some(1.0), false), (1, Some(0.95), false)]);
    assert_eq!(rifles(&m, Side::Container), vec![(1, Some(1.0), false)]);
}

/// A modded rifle beside whole plain ones at the same condition: its row (" +") equips,
/// stores, sells back and drops that rifle, not a plain one (weapon-mods.md §3).
#[test]
fn a_modded_row_takes_the_modded_unit() {
    use fnv_sim::inventory::Extra;
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.add_item(PLAYER, FormId(RIFLE), 5).unwrap();
    w.add_item_extra(PLAYER, FormId(RIFLE), 1, Extra { mods: 2, ..Extra::default() }).unwrap();
    let rifles = |m: &MenuModel, side: Side| -> Vec<(i32, u8, bool)> {
        m.list(side).rows.iter().filter(|r| r.item == FormId(RIFLE)).map(|r| (r.count, r.mods, r.bartered)).collect()
    };
    let modded = |m: &mut MenuModel, side: Side| {
        let i = m.list(side).rows.iter().position(|r| r.item == FormId(RIFLE) && r.mods == 2).expect("a modded row");
        m.focus = side;
        m.selected[side as usize] = i;
    };
    let mods_held = |w: &World<'_>| w.peek(PLAYER).unwrap().inventory.as_ref().unwrap().equipped_mods(FormId(RIFLE));
    let mut inv = MenuModel::open_inventory(&mut w).unwrap();
    assert_eq!(rifles(&inv, Side::Player), vec![(5, 0, false), (1, 2, false)]);
    modded(&mut inv, Side::Player);
    inv.apply(&mut w, Command::One);
    assert_eq!(inv.status.as_deref(), Some("Equipped Varmint Rifle +"));
    assert_eq!(mods_held(&w), 2);
    // Stored: the modded one leaves, the plain ones stay.
    let mut c = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    modded(&mut c, Side::Player);
    c.apply(&mut w, Command::One);
    assert_eq!(rifles(&c, Side::Player), vec![(5, 0, false)]);
    assert_eq!(rifles(&c, Side::Container), vec![(1, 0, false), (1, 2, false)]);
    // Bought back: that one comes, the box's plain one stays.
    let mut m = barter(&mut w);
    modded(&mut m, Side::Container);
    m.apply(&mut w, Command::One);
    assert_eq!(rifles(&m, Side::Player)[0], (1, 2, true));
    w.add_item(PLAYER, FormId(CAPS), 1000).unwrap(); // the scope (slot 2, 125) adds to its price
    m.refresh(&mut w).unwrap();
    m.apply(&mut w, Command::Accept);
    assert!(m.status.as_deref().unwrap_or("").starts_with("Traded"), "{:?}", m.status);
    assert_eq!(rifles(&m, Side::Player), vec![(5, 0, false), (1, 2, false)]);
    assert_eq!(rifles(&m, Side::Container), vec![(1, 0, false)]);
    // Dropped: the reference keeps the mods.
    let mut inv = MenuModel::open_inventory(&mut w).unwrap();
    modded(&mut inv, Side::Player);
    inv.apply(&mut w, Command::Drop);
    let dropped = inv.dropped.last().copied().expect("a dropped reference");
    assert_eq!(w.peek(dropped).unwrap().extra.mods, 2);
    assert_eq!(rifles(&inv, Side::Player), vec![(5, 0, false)]);
}

/// The Misc tab's keyring and Caravan cards (owner's playtest list 7 item 6): one row
/// each, opening on what they hold with a "Back" row first; skill books are listed with
/// the aid items (item 33).
#[test]
fn misc_folders_and_skill_books() {
    let f = fixture();
    assert!(f.forms.issues.is_empty(), "{:?}", f.forms.issues.to_json());
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    for item in [KEY_A, KEY_B, SKILL_BOOK, PLAIN_BOOK] {
        w.add_item(FormId::PLAYER_REF, FormId(item), 1).unwrap();
    }
    let mut m = MenuModel::open_inventory(&mut w).unwrap();
    m.apply(&mut w, Command::SetCategory(Side::Player, Category::Aid));
    assert_eq!(names(m.list(Side::Player)), [("Aid", 2), ("Big Book of Science", 1)]);
    m.apply(&mut w, Command::SetCategory(Side::Player, Category::Misc));
    assert_eq!(names(m.list(Side::Player)), [("Keyring", 2), ("Bottle Cap", 5), ("Paradise Lost", 1)]);
    // The cards AddCardToPlayer gave: two of one card.
    m.set_cards(&mut w, vec![FormId(CARD), FormId(CARD)]).unwrap();
    assert_eq!(names(m.list(Side::Player))[..2], [("Keyring", 2), ("Caravan Cards", 2)]);
    // Into the keyring and back.
    select(&mut m, Side::Player, "Keyring");
    m.apply(&mut w, Command::One);
    assert_eq!(m.folder, Some(Folder::Keys));
    assert_eq!(names(m.list(Side::Player)), [("Back", 0), ("Shack Key", 1), ("Vault Key", 1)]);
    // A folder row drops nothing.
    select(&mut m, Side::Player, "Back");
    m.apply(&mut w, Command::Drop);
    assert_eq!(m.status.as_deref(), Some("Nothing to drop"));
    m.apply(&mut w, Command::One);
    assert_eq!(m.folder, None);
    assert_eq!(m.selected_row(Side::Player).map(|r| r.name.as_str()), Some("Keyring"));
    // The cards; Backspace's Back leaves; another tab closes the folder.
    select(&mut m, Side::Player, "Caravan Cards");
    m.apply(&mut w, Command::One);
    assert_eq!(names(m.list(Side::Player)), [("Back", 0), ("Ace of Hearts", 2)]);
    select(&mut m, Side::Player, "Ace of Hearts");
    m.apply(&mut w, Command::Drop);
    assert_eq!(m.status.as_deref(), Some("Ace of Hearts can't be dropped"));
    m.apply(&mut w, Command::Back);
    assert_eq!(m.selected_row(Side::Player).map(|r| r.name.as_str()), Some("Caravan Cards"));
    m.apply(&mut w, Command::One);
    m.apply(&mut w, Command::SetCategory(Side::Player, Category::Aid));
    m.apply(&mut w, Command::SetCategory(Side::Player, Category::Misc));
    assert_eq!(m.folder, None);
    assert_eq!(count(&mut w, 0x14, KEY_A), 1);
}

/// The Pip-Boy's equip hint is bright exactly when Enter does something (#1896): it reads
/// the game's own checks (`enter_acts`: `World::equip_check`, which `World::equip_at` runs),
/// not what a hotkey can hold. A book that teaches nothing and a broken weapon are dim and
/// Enter leaves them be; a weapon, armour and an aid item are bright and Enter takes them.
#[test]
fn the_equip_hint_is_bright_when_enter_does_something() {
    use fnv_sim::inventory::Extra;
    let f = fixture();
    assert!(f.forms.issues.is_empty(), "{:?}", f.forms.issues.to_json());
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.add_item(PLAYER, FormId(PLAIN_BOOK), 1).unwrap();
    w.add_item(PLAYER, FormId(VEST), 1).unwrap();
    w.add_item_extra(PLAYER, FormId(RIFLE), 1, Extra { condition: Some(0.0), ..Extra::default() }).unwrap();
    let mut m = MenuModel::open_inventory(&mut w).unwrap();
    m.apply(&mut w, Command::SetCategory(Side::Player, Category::All));
    // The equip hint (shown with a controller, the first as the original's order puts it):
    // its word and whether dim.
    let equip_hint = |m: &MenuModel, w: &World<'_>, name: &str| -> (&'static str, bool) {
        let r = m.list(Side::Player).rows.iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no row {name}"));
        crate::pad::set_labels(crate::pad::Labels::Xbox);
        let h = crate::pipboy::items::page_hints(Category::All, Some(r), ("Repair", false), false, enter_acts(w, r));
        crate::pad::set_labels(crate::pad::Labels::Keys);
        (h[0].word, h[0].dim)
    };
    let equip = crate::pipboy::labels::built_in_label("sInventoryEquip");
    assert_eq!(equip_hint(&m, &w, "Paradise Lost"), ("Use", true));
    assert_eq!(equip_hint(&m, &w, "Varmint Rifle"), (equip, true));
    assert_eq!(equip_hint(&m, &w, "Pistol"), (equip, false));
    assert_eq!(equip_hint(&m, &w, "Leather Vest"), (equip, false));
    assert_eq!(equip_hint(&m, &w, "Aid"), ("Use", false));
    // Enter agrees: the dim ones are refused, the bright ones taken.
    select(&mut m, Side::Player, "Paradise Lost");
    m.apply(&mut w, Command::One);
    assert_eq!(count(&mut w, 0x14, PLAIN_BOOK), 1);
    select(&mut m, Side::Player, "Varmint Rifle");
    m.apply(&mut w, Command::One);
    assert!(!w.get(PLAYER).unwrap().inventory.as_ref().unwrap().is_equipped(FormId(RIFLE)));
    for name in ["Pistol", "Leather Vest"] {
        select(&mut m, Side::Player, name);
        m.apply(&mut w, Command::One);
        assert_eq!(m.status.as_deref(), Some(format!("Equipped {name}").as_str()));
    }
    // Worn, the hint takes it off; Enter does.
    assert_eq!(equip_hint(&m, &w, "Pistol"), ("Unequip", false));
    select(&mut m, Side::Player, "Aid");
    m.apply(&mut w, Command::One);
    assert_eq!(count(&mut w, 0x14, AID), 1);
}

#[test]
fn a_list_kept_in_view_moves_only_for_a_choice_out_of_it() {
    // 20 rows, 7 shown from the 6th: any row in view (one the pointer can be over) keeps it.
    for sel in 5..12 {
        assert_eq!(keep_in_view(5, sel, 20, 7), 5);
    }
    // The keys past either end move it by as little as shows the choice.
    assert_eq!(keep_in_view(5, 12, 20, 7), 6);
    assert_eq!(keep_in_view(5, 4, 20, 7), 4);
    // A shorter list (a filter, a deleted row) clamps it.
    assert_eq!(keep_in_view(5, 2, 4, 7), 0);
}

#[test]
fn a_weapon_dropped_beside_its_holder_is_one_item_in_the_menu_and_the_world() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    // The box stands in for a corpse that dropped its rifle as it died (#1032).
    let drop = |w: &mut World<'_>| {
        let r = w.drop_item(FormId(BOX_REF), FormId(RIFLE), 1).unwrap()[0];
        w.get(r).unwrap().dropped_by = Some(FormId(BOX_REF));
        r
    };
    let r = drop(&mut w);
    assert_eq!(count(&mut w, BOX_REF, RIFLE), 0);
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    assert!(names(m.list(Side::Container)).contains(&("Varmint Rifle", 1)));
    // Taking the row picks the reference up: the player has it, the world does not.
    select(&mut m, Side::Container, "Varmint Rifle");
    let a = m.apply(&mut w, Command::One);
    assert_eq!(a.picked, vec![r]);
    assert!(w.peek(r).unwrap().deleted());
    assert_eq!((count(&mut w, BOX_REF, RIFLE), count(&mut w, 0x14, RIFLE)), (0, 1));
    assert!(!names(m.list(Side::Container)).contains(&("Varmint Rifle", 1)));
    // The other way: picked up from the ground, it is no longer listed.
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let r = drop(&mut w);
    w.activate(FormId(0x14), r).unwrap();
    let m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    assert!(!names(m.list(Side::Container)).contains(&("Varmint Rifle", 1)));
    // Take All takes it too, once.
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let r = drop(&mut w);
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    let a = m.apply(&mut w, Command::TakeAll);
    assert_eq!(a.picked, vec![r]);
    assert_eq!(count(&mut w, 0x14, RIFLE), 1);
}

/// `crate::menu::apply_input` over the box, as the game runs it.
fn app_on_the_box() -> App {
    let f: &'static Fixture = Box::leak(Box::new(fixture()));
    let mut world = World::new(&f.forms, &f.refs, SimConfig::default());
    let m = MenuModel::open_container(&mut world, FormId(BOX_REF)).unwrap();
    let mut app = App::new();
    app.add_plugins(bevy::state::app::StatesPlugin)
        .insert_state(crate::PlayState::Menu)
        .insert_resource(crate::sim::SimState { forms: &f.forms, refs: &f.refs, world })
        .insert_resource(crate::menu::ActiveMenu(Some(m)))
        .init_resource::<crate::menu::PromptKeys>()
        .add_message::<crate::menu::MenuInput>()
        .add_message::<crate::RefChanged>()
        .add_message::<crate::ItemUsed>()
        .add_message::<crate::ui_sounds::UiSound>()
        .add_message::<crate::crime::ItemsTaken>()
        .add_message::<crate::pickpocket::Pickpocketed>()
        .add_systems(Update, crate::menu::apply_input);
    app
}

fn send(app: &mut App, commands: &[Command]) {
    for &c in commands {
        app.world_mut().write_message(crate::menu::MenuInput(c));
    }
    app.update();
}

fn in_box(app: &mut App, item: u32) -> i32 {
    count(&mut app.world_mut().resource_mut::<crate::sim::SimState>().world, BOX_REF, item)
}

/// The guard the tile input sends with a click on the container's row `index`.
fn guard_of(app: &App, index: usize, selected: bool, then: u8) -> RowGuard {
    let m = app.world().resource::<crate::menu::ActiveMenu>().0.as_ref().unwrap();
    RowGuard { mode: m.mode, side: Side::Container, index, key: RowKey::at(&m.list(Side::Container).rows, index).unwrap(), selected, then }
}

fn row_of(app: &App, name: &str) -> usize {
    let m = app.world().resource::<crate::menu::ActiveMenu>().0.as_ref().unwrap();
    m.list(Side::Container).rows.iter().position(|r| r.name == name).unwrap()
}

#[test]
fn a_queued_category_leaves_a_clicked_row_to_nothing() {
    // The hazard: unguarded, the category's refresh puts the rifle at the index clicked.
    let mut app = app_on_the_box();
    assert_ne!(row_of(&app, "Varmint Rifle"), 0);
    send(&mut app, &[Command::Category(Side::Container, 1), Command::Hover(Side::Container, 0), Command::One]);
    assert_eq!(in_box(&mut app, RIFLE), 0, "the other row went");
    // Guarded, by what the click saw: nothing moves, the category does change, and the next
    // command is the menu's own again.
    let mut app = app_on_the_box();
    let guard = guard_of(&app, 0, false, 2);
    send(&mut app, &[Command::Category(Side::Container, 1), Command::Expect(guard), Command::Hover(Side::Container, 0), Command::One]);
    assert_eq!(in_box(&mut app, RIFLE), 1);
    assert_eq!(app.world().resource::<crate::menu::ActiveMenu>().0.as_ref().unwrap().category[0], Category::Weapons);
    send(&mut app, &[Command::Hover(Side::Container, 0), Command::One]);
    assert_eq!(in_box(&mut app, RIFLE), 0);
}

#[test]
fn a_guarded_click_on_a_row_still_there_acts() {
    let mut app = app_on_the_box();
    let can = row_of(&app, "tin can");
    let guard = guard_of(&app, can, false, 2);
    send(&mut app, &[Command::Expect(guard), Command::Hover(Side::Container, can), Command::One]);
    assert_eq!(in_box(&mut app, CAN), 1, "one of the two");
    // A click on the selected row, the selection moved before it applies: nothing.
    let can = row_of(&app, "tin can");
    let guard = guard_of(&app, can, true, 1);
    send(&mut app, &[Command::Up, Command::Expect(guard), Command::Select(Side::Container, can)]);
    assert_eq!(in_box(&mut app, CAN), 1);
    assert_eq!(app.world().resource::<crate::menu::ActiveMenu>().0.as_ref().unwrap().selected[Side::Container as usize], can - 1, "not selected again");
    // Another menu's row.
    let guard = RowGuard { mode: Mode::Inventory, ..guard_of(&app, 0, false, 2) };
    let first = app.world().resource::<crate::menu::ActiveMenu>().0.as_ref().unwrap().list(Side::Container).rows[0].item.0;
    let held = in_box(&mut app, first);
    send(&mut app, &[Command::Expect(guard), Command::Hover(Side::Container, 0), Command::Stack]);
    assert_eq!(in_box(&mut app, first), held);
}

/// A modded weapon's value is its own at its condition plus its mods' values, which the
/// condition does not lower [observed: o26_value 2026-11-06, a Varmint Rifle 75 with all
/// three mods (300) read 326 at 50 % condition; issue #982].
#[test]
fn a_modded_weapon_is_worth_its_mods_on_top() {
    use fnv_sim::inventory::Extra;
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.add_item_extra(PLAYER, FormId(RIFLE), 1, Extra { mods: 2, condition: Some(0.5), ..Extra::default() }).unwrap();
    let inv = MenuModel::open_inventory(&mut w).unwrap();
    let r = inv.list(Side::Player).rows.iter().find(|r| r.item == FormId(RIFLE) && r.mods == 2).expect("a modded row");
    assert_eq!(r.mods_value, 125);
    let p = Prices::default();
    let worth = r.worth(&p).unwrap();
    assert!((worth - (p.value_at(300, Some(0.5)) + 125.0)).abs() < 1e-3, "{worth}");
    // Slot 1 is empty: its flag adds nothing.
    assert_eq!(f.forms.mods_value(FormId(RIFLE), 1), 0);
    assert_eq!(f.forms.mods_value(FormId(RIFLE), 3), 125);
}

#[test]
fn barter_kind_mults_pick_the_side_and_kind() {
    let k = BarterKindMults { sell: [0.5, 0.6, 0.7, 0.8, 0.9, 1.1], buy: [2.0, 2.1, 2.2, 2.3, 2.4, 2.5], water_unscaled: true };
    assert_eq!(k.of(false, Some(ItemKind::Weapon), false), 0.6);
    assert_eq!(k.of(true, Some(ItemKind::ItemMod), false), 2.5);
    assert_eq!(k.of(true, Some(ItemKind::Book), false), 2.4);
    assert_eq!(k.of(true, Some(ItemKind::Aid), true), 1.0);
    assert_eq!(BarterKindMults { water_unscaled: false, ..k }.of(true, Some(ItemKind::Aid), true), 2.2);
}

/// Take All Confirmation: the first Take All asks, the next takes.
#[test]
fn take_all_asks_first_under_the_tweak() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    m.take_all_confirm = Some(1);
    let a = m.apply(&mut w, Command::TakeAll);
    assert!(!a.changed && m.confirming_take_all(), "{:?}", m.status);
    let a = m.apply(&mut w, Command::TakeAll);
    assert!(a.changed);
    assert!(m.list(Side::Container).rows.is_empty());
}

/// The Clicking the Tab Sorts tweak's orders: value, then weight, the most first, name
/// order kept between equals; name brings the usual order back.
#[test]
fn the_player_list_sorts_by_value_or_weight() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let m = MenuModel::open_inventory(&mut w).unwrap();
    let row = m.list(Side::Player).rows[0].clone();
    let mk = |name: &str, value, weight| Row { name: name.into(), value: Some(value), weight: Some(weight), ..row.clone() };
    let base = vec![mk("a", 5, 1.0), mk("b", 20, 0.5), mk("c", 5, 9.0)];
    let names = |rows: &[Row]| rows.iter().map(|r| r.name.clone()).collect::<Vec<_>>();
    let mut rows = base.clone();
    SortBy::Value.apply(&mut rows);
    assert_eq!(names(&rows), ["b", "a", "c"]);
    let mut rows = base.clone();
    SortBy::Weight.apply(&mut rows);
    assert_eq!(names(&rows), ["c", "a", "b"]);
    let mut rows = base.clone();
    SortBy::Name.apply(&mut rows);
    assert_eq!(names(&rows), ["a", "b", "c"]);
    assert_eq!(SortBy::Name.next().next().next(), SortBy::Name);
}

#[test]
fn ysi_apply_sorts_by_tag_keeps_folders_and_preserves_selection() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut m = MenuModel::open_inventory(&mut w).unwrap();
    let base_rows = m.list(Side::Player).rows.clone();
    assert!(base_rows.len() >= 3, "fixture has at least three player items");

    // Pick the last item before sorting so we can verify selection follows it.
    let target_item = base_rows[2].item;
    let first_item = base_rows[0].item;
    let second_item = base_rows[1].item;
    m.selected[Side::Player as usize] = 2;

    // Assign ySI tags in reverse alphabetical order vs item names, leaving first_item untagged.
    m.apply_ysi(|id| {
        if id == target_item {
            Some(fnv_game::ui_ysi::YsiItemDisplay {
                tag: Some("(a01)".into()),
                icon: Some("icons\\target.dds".into()),
                template: fnv_game::ui_ysi::YSI_DEFAULT_TEMPLATE.into(),
            })
        } else if id == second_item {
            Some(fnv_game::ui_ysi::YsiItemDisplay {
                tag: Some("(b02)".into()),
                icon: Some("icons\\second.dds".into()),
                template: fnv_game::ui_ysi::YSI_DEFAULT_TEMPLATE.into(),
            })
        } else {
            None
        }
    });

    let rows = &m.list(Side::Player).rows;
    assert_eq!(rows[0].item, target_item, "(a01) sorts first");
    assert_eq!(rows[0].ysi.as_ref().and_then(|y| y.icon.as_deref()), Some("icons\\target.dds"));
    assert_eq!(rows[1].item, second_item, "(b02) sorts second");
    let untagged_pos = rows.iter().position(|r| r.item == first_item).unwrap();
    assert!(untagged_pos >= 2, "untagged items follow tagged items");
    assert_eq!(m.selected_row(Side::Player).map(|r| r.item), Some(target_item), "selection tracks moved row");
}

#[test]
fn ysi_apply_options_honors_sort_inventory_and_add_inventory_icons_independently() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let base_order: Vec<FormId> = MenuModel::open_inventory(&mut w).unwrap().list(Side::Player).rows.iter().map(|r| r.item).collect();
    assert!(base_order.len() >= 3);
    let target_item = base_order[2];
    let second_item = base_order[1];
    let resolve = |id: FormId| {
        if id == target_item {
            Some(fnv_game::ui_ysi::YsiItemDisplay {
                tag: Some("(a01)".into()),
                icon: Some("icons\\target.dds".into()),
                template: fnv_game::ui_ysi::YSI_DEFAULT_TEMPLATE.into(),
            })
        } else if id == second_item {
            Some(fnv_game::ui_ysi::YsiItemDisplay {
                tag: Some("(b02)".into()),
                icon: Some("icons\\second.dds".into()),
                template: fnv_game::ui_ysi::YSI_DEFAULT_TEMPLATE.into(),
            })
        } else {
            None
        }
    };

    // 1. bSortInventory = 1, bAddInventoryIcons = 0: sorted by ySI tag, no row icon metadata retained.
    let mut m_sort_only = MenuModel::open_inventory(&mut w).unwrap();
    m_sort_only.apply_ysi_options(true, false, resolve);
    assert!(!m_sort_only.ysi_icons, "bAddInventoryIcons=0 must propagate ysi_icons=false to tile menus");
    let rows_sort = &m_sort_only.list(Side::Player).rows;
    assert_eq!(rows_sort[0].item, target_item);
    assert_eq!(rows_sort[1].item, second_item);
    assert!(rows_sort.iter().all(|r| r.ysi.is_none()), "bAddInventoryIcons=0 must strip row icons");

    // 2. bSortInventory = 0, bAddInventoryIcons = 1: vanilla alphabetical order kept, row icons attached.
    let mut m_icons_only = MenuModel::open_inventory(&mut w).unwrap();
    m_icons_only.apply_ysi_options(false, true, resolve);
    assert!(m_icons_only.ysi_icons);
    let rows_icons = &m_icons_only.list(Side::Player).rows;
    let icon_order: Vec<FormId> = rows_icons.iter().map(|r| r.item).collect();
    assert_eq!(icon_order, base_order, "bSortInventory=0 must preserve vanilla alphabetical order");
    assert_eq!(rows_icons[2].ysi.as_ref().and_then(|y| y.icon.as_deref()), Some("icons\\target.dds"));

    // 3. Both off: vanilla order and no row icons.
    let mut m_off = MenuModel::open_inventory(&mut w).unwrap();
    m_off.apply_ysi_options(false, false, resolve);
    let rows_off = &m_off.list(Side::Player).rows;
    let off_order: Vec<FormId> = rows_off.iter().map(|r| r.item).collect();
    assert_eq!(off_order, base_order);
    assert!(rows_off.iter().all(|r| r.ysi.is_none()));
}

/// The Quantity Menu Respects Companion Carry Cap tweak: what fits, at least one.
#[test]
fn a_companion_takes_no_more_than_it_can_carry() {
    assert_eq!(carry_cap(10, 2.0, 7.0), 3);
    assert_eq!(carry_cap(10, 2.0, 100.0), 10);
    assert_eq!(carry_cap(10, 2.0, 1.0), 1);
    assert_eq!(carry_cap(10, 2.0, -5.0), 1);
    assert_eq!(carry_cap(10, 0.0, 0.0), 10);
}


#[test]
fn switching_cancels_quantity_meter_drag_and_preserves_the_mouse_release() {
    use bevy::prelude::*;
    use bevy::input::{gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent}, keyboard::{Key, KeyboardInput}, mouse::MouseButtonInput, ButtonState};
    use crate::{bindings::{Bindings, InputDevice}, menu::{ActiveMenu, AmountMeter, MenuInput, quantity_mouse}, ui_pointer::{PointerState, UiPointerPlugin}};
    let mut model = MenuModel::empty(Mode::Inventory, "Synthetic inventory".into());
    model.quantity = Some(Quantity { cmd: crate::menu::model::Command::One, side: Side::Player, row: 0, max: 10, amount: 1 });
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, crate::pad::InputSwitchPlugin, UiPointerPlugin))
        .init_resource::<Bindings>().init_resource::<bevy::ui::UiStack>()
        .insert_resource(ActiveMenu(Some(model)))
        .add_message::<MenuInput>().add_systems(Update, quantity_mouse);
    let meter = app.world_mut().spawn((AmountMeter, PointerState::Pressed,
        bevy::ui::RelativeCursorPosition { normalized: Some(Vec2::ZERO), ..default() })).id();
    app.world_mut().write_message(MouseButtonInput { button: MouseButton::Left, state: ButtonState::Pressed, window: Entity::PLACEHOLDER });
    app.update();
    assert_eq!(app.world().resource::<Messages<MenuInput>>().len(), 1, "the real quantity reader sees the initial drag");
    app.world_mut().resource_mut::<Messages<MenuInput>>().clear();
    let pad = app.world_mut().spawn(Gamepad::default()).id();
    app.world_mut().write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(pad, GamepadButton::South, 1.0)));
    app.update();
    assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
    crate::bindings::set_device(app.world_mut(), InputDevice::Controller);
    assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
    assert_ne!(app.world().get::<PointerState>(meter), Some(&PointerState::Pressed));
    app.world_mut().write_message(KeyboardInput { key_code: KeyCode::KeyX, logical_key: Key::Character("x".into()), state: ButtonState::Pressed, text: None, repeat: false, window: Entity::PLACEHOLDER });
    app.update();
    assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
    crate::bindings::set_device(app.world_mut(), InputDevice::KeyboardMouse);
    assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
    app.world_mut().write_message(MouseButtonInput { button: MouseButton::Left, state: ButtonState::Released, window: Entity::PLACEHOLDER });
    app.update();
    assert!(app.world().resource::<ButtonInput<MouseButton>>().just_released(MouseButton::Left));
    assert_ne!(app.world().get::<PointerState>(meter), Some(&PointerState::Pressed));
    // Isolate post-release travel from buffered commands from earlier drag frames
    // [design, #2200; menu::quantity_mouse, bevy::ecs::message::Messages].
    app.world_mut().resource_mut::<Messages<MenuInput>>().clear();
    app.world_mut().get_mut::<bevy::ui::RelativeCursorPosition>(meter).unwrap().normalized = Some(Vec2::splat(0.4));
    app.update();
    assert!(app.world().resource::<Messages<MenuInput>>().is_empty(), "pointer travel after release cannot change the quantity");
}

#[test]
fn touch_drag_assigns_actual_inventory_row_without_consuming_and_stale_rows_are_refused() {
    use crate::pipboy::touch_items::TouchItems;
    use bevy::input::touch::{TouchInput,TouchPhase};
    use bevy::prelude::{Res,ResMut,Entity,Vec2};
    fn apply(active:Res<crate::menu::ActiveMenu>,mut touch:ResMut<TouchItems>,mut keys:ResMut<crate::hotkeys::Hotkeys>,mut sim:ResMut<crate::sim::SimState>){touch.assign_pending(&active,&mut keys,&mut sim.world);}
    let f=fixture();let mut w=World::new(&f.forms,&f.refs,SimConfig::default());
    let m=MenuModel::open_inventory(&mut w).unwrap();let index=m.list(Side::Player).rows.iter().position(|r|r.item==FormId(AID)).unwrap();
    let initial=count(&mut w,0x14,AID);
    for stale in [false,true]{
        let mut touch=TouchItems::default();
        let mut event=TouchInput{phase:TouchPhase::Started,position:Vec2::new(100.,100.),window:Entity::PLACEHOLDER,force:None,id:8};
        assert!(touch.event(&event,0));touch.project(Some);touch.take_projected();touch.capture_row(&m,index);
        event.phase=TouchPhase::Moved;event.position=Vec2::new(500.,500.);assert!(touch.event(&event,100));
        event.phase=TouchPhase::Ended;assert!(touch.event(&event,150));touch.release_over(Some(2));
        assert!(touch.suppress_click(),"drag release must not trigger the row's equip/use click");
        let mut model=m.clone();if stale{model.lists[Side::Player as usize].rows.rotate_left(1);}
        let mut sim=crate::sim::test_world::sim(vec![crate::sim::test_world::item_base(AID)],vec![]);
        sim.world.add_item(FormId::PLAYER_REF,FormId(AID),initial).unwrap();
        assert_eq!(sim.world.item_count(FormId::PLAYER_REF,FormId(AID)).unwrap(),initial);
        let mut app=App::new();app.insert_resource(crate::menu::ActiveMenu(Some(model))).insert_resource(touch).init_resource::<crate::hotkeys::Hotkeys>().insert_resource(sim).add_systems(Update,apply);app.update();
        let slot=app.world().resource::<crate::hotkeys::Hotkeys>().0[2];
        if stale{assert_eq!(slot,None);}else{assert_eq!(slot,Some(crate::hotkeys::Slot{item:AID,mods:0}));}
        assert_eq!(count(&mut w,0x14,AID),initial,"assignment never consumes inventory");
        assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(AID)).unwrap(),initial,"actual assignment world is not consumed");
    }
}

#[test]
fn cancelled_touch_assignment_cannot_cross_a_menu_session_or_replace_ammo_swap(){
    use crate::pipboy::touch_items::TouchItems;
    use bevy::input::touch::{TouchInput,TouchPhase};
    use bevy::prelude::{Entity,Vec2};
    let f=fixture();let mut w=World::new(&f.forms,&f.refs,SimConfig::default());let m=MenuModel::open_inventory(&mut w).unwrap();
    let index=m.list(Side::Player).rows.iter().position(|r|r.item==FormId(AID)).unwrap();
    for cancelled in [false,true]{
        let mut touch=TouchItems::default();let mut event=TouchInput{phase:TouchPhase::Started,position:Vec2::ONE,window:Entity::PLACEHOLDER,force:None,id:5};
        touch.event(&event,0);touch.project(Some);touch.take_projected();touch.capture_row(&m,index);
        event.phase=TouchPhase::Ended;touch.event(&event,500);touch.release_over(Some(if cancelled{2}else{crate::hotkeys::AMMO_SWAP}));
        if cancelled{touch.cancel();}
        let mut keys=crate::hotkeys::Hotkeys::default();assert!(!touch.assign_pending(&crate::menu::ActiveMenu(Some(m.clone())),&mut keys,&mut w));assert!(keys.0.iter().all(Option::is_none));
    }
}

#[test]
fn vertical_touch_browsing_cannot_assign_a_captured_row_even_when_released_over_a_hotkey(){
    use crate::pipboy::touch_items::TouchItems;
    use bevy::{prelude::{Res,ResMut,Entity,Vec2},input::touch::{TouchInput,TouchPhase}};
    fn assign(active:Res<crate::menu::ActiveMenu>,mut touch:ResMut<TouchItems>,mut keys:ResMut<crate::hotkeys::Hotkeys>,mut sim:ResMut<crate::sim::SimState>){touch.assign_pending(&active,&mut keys,&mut sim.world);}
    let f=fixture();let mut world=World::new(&f.forms,&f.refs,SimConfig::default());
    let model=MenuModel::open_inventory(&mut world).unwrap();let index=model.list(Side::Player).rows.iter().position(|r|r.item==FormId(AID)).unwrap();
    let before=count(&mut world,0x14,AID);
    for deliberate in [false,true] {
        let mut touch=TouchItems::default();let mut t=TouchInput{window:Entity::PLACEHOLDER,id:44,position:Vec2::new(100.,100.),phase:TouchPhase::Started,force:None};
        touch.event(&t,0);touch.project(Some);touch.take_projected();touch.capture_row(&model,index);
        if deliberate {touch.update_hold(500);}
        t.phase=TouchPhase::Moved;t.position.y+=100.;touch.event(&t,if deliberate{550}else{50});
        t.phase=TouchPhase::Ended;touch.event(&t,700);touch.release_over(Some(2));
        let mut sim=crate::sim::test_world::sim(vec![crate::sim::test_world::item_base(AID)],vec![]);
        sim.world.add_item(FormId::PLAYER_REF,FormId(AID),before).unwrap();
        assert_eq!(sim.world.item_count(FormId::PLAYER_REF,FormId(AID)).unwrap(),before);
        let mut app=App::new();app.insert_resource(touch).insert_resource(crate::menu::ActiveMenu(Some(model.clone())))
            .init_resource::<crate::hotkeys::Hotkeys>().insert_resource(sim).add_systems(Update,assign);app.update();
        assert_eq!(app.world().resource::<crate::hotkeys::Hotkeys>().0[2],deliberate.then_some(crate::hotkeys::Slot{item:AID,mods:0}));
        assert_eq!(count(&mut world,0x14,AID),before);
        assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(AID)).unwrap(),before,"actual assignment world is not consumed");
    }
}

/// #2751: two openings of one victim's pocket do not roll the same; with the test aid's
/// number they do; and over many openings a first take at 85% goes unnoticed about 85%
/// of the time (seeded by the victim alone, the Legion assassin's first roll was 90.79
/// at every opening: caught every time).
#[test]
fn every_opening_of_a_pocket_rolls_afresh() {
    let victim = FormId(0x0013_4FF6);
    let rolls_of = |seed: u64| {
        let mut st = Steal::new(victim, rolls(true), 100.0, 15.0, seed);
        (0..4).map(|_| st.rng.next_f32()).collect::<Vec<f32>>()
    };
    let clock = 1_763_000_000_000_000_000u64;
    // The same instant, the next opening: other rolls.
    assert_ne!(rolls_of(steal_seed(victim, None, clock, 0)), rolls_of(steal_seed(victim, None, clock, 1)));
    // A later instant, the same count (another session after a reload): other rolls.
    assert_ne!(rolls_of(steal_seed(victim, None, clock, 0)), rolls_of(steal_seed(victim, None, clock + 1_000_000, 0)));
    // The test aid: the same at every opening, and what the fixed seed gave.
    assert_eq!(steal_seed(victim, Some(0x5EED), clock, 0), steal_seed(victim, Some(0x5EED), clock + 77, 9));
    assert_eq!(steal_seed(victim, Some(0x5EED), clock, 0), u64::from(victim.0) ^ 0x5EED);
    let fixed = rolls_of(steal_seed(victim, Some(0x5EED), 0, 0));
    assert!(fixed[0] * 100.0 > 85.0, "the old seed's first roll on the assassin was over the cap: {}", fixed[0] * 100.0);
    // 10,000 openings, a few milliseconds apart: the first take at 85%.
    let mut unseen = 0;
    for i in 0..10_000u64 {
        let mut st = Steal::new(victim, rolls(true), 100.0, 15.0, steal_seed(victim, None, clock + i * 3_141_593, i));
        unseen += u32::from(st.unnoticed(85));
    }
    assert!((8_300..=8_700).contains(&unseen), "{unseen} of 10,000 first takes at 85% went unnoticed");
}

#[test]
fn refresh_items_list_rebuilds_open_inventory_container_barter_preserves_selection_and_is_one_shot() {
    let f: &'static Fixture = Box::leak(Box::new(fixture()));
    let mut world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut inv = MenuModel::open_inventory(&mut world).unwrap();
    // Switch to Weapons (player holds Pistol) and select it.
    inv.category[Side::Player as usize] = Category::Weapons;
    inv.refresh(&mut world).unwrap();
    assert_eq!(inv.selected_row(Side::Player).map(|r| r.item), Some(FormId(PISTOL)));
    assert_eq!(inv.list(Side::Player).rows.len(), 1);

    let script = crate::script::ScriptState::over_no_plugins();
    let mut app = App::new();
    app.insert_resource(crate::sim::SimState { forms: &f.forms, refs: &f.refs, world })
        .insert_resource(crate::menu::ActiveMenu(Some(inv)))
        .insert_resource(script)
        .add_systems(Update, crate::menu::sync_refresh_items_list);

    // Script adds RIFLE to player inventory and calls RefreshItemsList.
    app.world_mut().resource_mut::<crate::sim::SimState>().world.add_item(FormId(0x14), FormId(RIFLE), 1).unwrap();
    app.world_mut()
        .resource_mut::<crate::script::ScriptState>()
        .rt
        .as_mut()
        .unwrap()
        .game
        .run_query("Begin Function { }\nRefreshItemsList\nEnd\n", Default::default())
        .unwrap();
    assert!(app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.refresh_items_list_pending());

    app.update();
    // One-shot flag consumed; menu stays open with refreshed rows and preserved selection on Pistol.
    assert!(!app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.refresh_items_list_pending());
    let active = app.world().resource::<crate::menu::ActiveMenu>();
    let m = active.0.as_ref().expect("inventory menu stays open");
    assert_eq!(m.list(Side::Player).rows.len(), 2, "new RIFLE row appears immediately");
    assert_eq!(m.selected_row(Side::Player).map(|r| r.item), Some(FormId(PISTOL)), "selection stays on Pistol");

    // Second frame with no RefreshItemsList does not mutate ActiveMenu.
    app.world_mut().resource_mut::<crate::sim::SimState>().world.add_item(FormId(0x14), FormId(CAN), 1).unwrap();
    app.update();
    assert!(!app.world().is_resource_changed::<crate::menu::ActiveMenu>(), "no spurious change when no refresh was requested");

    // Container mode: mutating container stock + RefreshItemsList rebuilds container rows & keeps selection.
    {
        let mut sim = app.world_mut().resource_mut::<crate::sim::SimState>();
        let mut cont = MenuModel::open_container(&mut sim.world, FormId(BOX_REF)).unwrap();
        select(&mut cont, Side::Container, "Varmint Rifle");
        app.world_mut().resource_mut::<crate::menu::ActiveMenu>().0 = Some(cont);
    }
    app.world_mut().resource_mut::<crate::sim::SimState>().world.add_item(FormId(BOX_REF), FormId(AID), 2).unwrap();
    app.world_mut()
        .resource_mut::<crate::script::ScriptState>()
        .rt
        .as_mut()
        .unwrap()
        .game
        .run_query("Begin Function { }\nRefreshItemsList\nEnd\n", Default::default())
        .unwrap();
    app.update();
    {
        let m = app.world().resource::<crate::menu::ActiveMenu>().0.as_ref().expect("container menu stays open");
        assert!(m.list(Side::Container).rows.iter().any(|r| r.item == FormId(AID) && r.count == 2));
        assert_eq!(m.selected_row(Side::Container).map(|r| r.item), Some(FormId(RIFLE)));
    }

    // Barter mode: mutating vendor stock + RefreshItemsList rebuilds barter rows & keeps selection.
    {
        let mut sim = app.world_mut().resource_mut::<crate::sim::SimState>();
        let mut barter = MenuModel::open_barter(&mut sim.world, FormId(BOX_REF), FormId(BOX_REF), FormId(CAPS), 50.0, 0).unwrap();
        select(&mut barter, Side::Container, "Varmint Rifle");
        app.world_mut().resource_mut::<crate::menu::ActiveMenu>().0 = Some(barter);
    }
    app.world_mut().resource_mut::<crate::sim::SimState>().world.add_item(FormId(BOX_REF), FormId(PISTOL), 1).unwrap();
    app.world_mut()
        .resource_mut::<crate::script::ScriptState>()
        .rt
        .as_mut()
        .unwrap()
        .game
        .run_query("Begin Function { }\nRefreshItemsList\nEnd\n", Default::default())
        .unwrap();
    app.update();
    {
        let m = app.world().resource::<crate::menu::ActiveMenu>().0.as_ref().expect("barter menu stays open");
        assert!(m.list(Side::Container).rows.iter().any(|r| r.item == FormId(PISTOL)));
        assert_eq!(m.selected_row(Side::Container).map(|r| r.item), Some(FormId(RIFLE)));
    }

    // Closed menu: RefreshItemsList is still consumed cleanly as a one-shot no-op.
    app.world_mut().resource_mut::<crate::menu::ActiveMenu>().0 = None;
    app.world_mut()
        .resource_mut::<crate::script::ScriptState>()
        .rt
        .as_mut()
        .unwrap()
        .game
        .run_query("Begin Function { }\nRefreshItemsList\nEnd\n", Default::default())
        .unwrap();
    app.update();
    assert!(!app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.refresh_items_list_pending());
}

#[test]
fn hide_item_barter_ex_filters_vendor_player_and_both_sides() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    // Give both sides CAN, PISTOL, and RIFLE so each side flag is exercised distinctly.
    w.inventory_mut(FormId(0x14)).unwrap().add(FormId(CAN), 1);
    w.inventory_mut(FormId(0x14)).unwrap().add(FormId(RIFLE), 1);
    w.inventory_mut(FormId(BOX_REF)).unwrap().add(FormId(PISTOL), 1);

    let mut m = MenuModel::open_barter(&mut w, FormId(BOX_REF), FormId(BOX_REF), FormId(CAPS), 50.0, 0).unwrap();
    let has = |m: &MenuModel, side: Side, item: u32| m.list(side).rows.iter().any(|r| r.item == FormId(item));
    assert!(has(&m, Side::Container, CAN) && has(&m, Side::Player, CAN));
    assert!(has(&m, Side::Container, PISTOL) && has(&m, Side::Player, PISTOL));
    assert!(has(&m, Side::Container, RIFLE) && has(&m, Side::Player, RIFLE));

    // Flag 1: shown on player's side only (hidden on vendor's side).
    // Flag 2: shown on vendor's side only (hidden on player's side).
    // Flag 3: hidden on both sides.
    let hidden = BTreeMap::from([(FormId(CAN), 1), (FormId(PISTOL), 2), (FormId(RIFLE), 3)]);
    m.set_barter_hidden(&mut w, hidden).unwrap();
    assert!(!has(&m, Side::Container, CAN) && has(&m, Side::Player, CAN), "flag 1 hides vendor side only");
    assert!(has(&m, Side::Container, PISTOL) && !has(&m, Side::Player, PISTOL), "flag 2 hides player side only");
    assert!(!has(&m, Side::Container, RIFLE) && !has(&m, Side::Player, RIFLE), "flag 3 hides both sides");

    // Clearing the script overrides restores all items on both sides.
    m.set_barter_hidden(&mut w, BTreeMap::new()).unwrap();
    assert!(has(&m, Side::Container, CAN) && has(&m, Side::Player, CAN));
    assert!(has(&m, Side::Container, PISTOL) && has(&m, Side::Player, PISTOL));
    assert!(has(&m, Side::Container, RIFLE) && has(&m, Side::Player, RIFLE));
}

#[test]
fn barter_hides_keys_on_both_sides_in_all_and_misc_while_inventory_and_containers_keep_them() {
    use crate::tiles::barter_menu::tests::of_model;
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let cloned_key = FormId(0xFF00_0180);
    assert!(w.add_clone_item(cloned_key, FormId(KEY_A)));
    // Give both sides zero-value, valued, and TempCloneForm keys alongside ordinary MISC and a quest item.
    for holder in [PLAYER, FormId(BOX_REF)] {
        w.add_item(holder, FormId(KEY_A), 1).unwrap();
        w.add_item(holder, FormId(KEY_VALUED), 2).unwrap();
        w.add_item(holder, cloned_key, 1).unwrap();
    }
    w.add_item(PLAYER, FormId(CAN), 1).unwrap();
    w.add_item(PLAYER, FormId(CHIP), 1).unwrap();

    let mut m = barter(&mut w);
    let has = |m: &MenuModel, side: Side, item: FormId| m.list(side).rows.iter().any(|r| r.item == item);
    for category in [Category::All, Category::Misc] {
        m.apply(&mut w, Command::SetCategory(Side::Player, category));
        m.apply(&mut w, Command::SetCategory(Side::Container, category));
        for side in [Side::Player, Side::Container] {
            for key in [FormId(KEY_A), FormId(KEY_VALUED), cloned_key] {
                assert!(!has(&m, side, key), "key {key} must not appear in barter ({side:?}, {category:?})");
            }
            assert!(!has(&m, side, FormId(CHIP)), "quest item stays hidden");
            assert!(!has(&m, side, FormId(CAPS)), "caps stay hidden");
            assert!(has(&m, side, FormId(CAN)), "ordinary MISC stays listed in barter ({side:?}, {category:?})");
        }
        // Tiled barter view reflects the same model rows: no key tiles on either side.
        let ui = of_model(&m);
        let root = "BarterMenu/NOGLOW_BRANCH";
        for list in ["BM_ItemsRect/BM_Items_InventoryList", "BM_ContainerRect/BM_Container_InventoryList"] {
            assert!(ui.find_tile(&format!("{root}/{list}/Shack Key")).is_none());
            assert!(ui.find_tile(&format!("{root}/{list}/Valued Key")).is_none());
            assert!(ui.find_tile(&format!("{root}/{list}/tin can")).is_some());
        }
    }

    // Even with a vendor service filter (whether allowing or refusing Miscellaneous) and cleared
    // HideItemBarterEx flags, keys stay hidden and are not reported in `withheld`.
    m.set_barter_hidden(&mut w, BTreeMap::new()).unwrap();
    m.set_sells(&mut w, Sells(std::sync::Arc::new(|_| Some("Miscellaneous")))).unwrap();
    assert!(!has(&m, Side::Container, FormId(KEY_A)) && !has(&m, Side::Container, FormId(KEY_VALUED)));
    assert!(m.trade.as_ref().unwrap().withheld.iter().all(|(id, _)| !matches!(id.0, KEY_A | KEY_VALUED) && *id != cloned_key));

    // A stale offered key cannot survive refresh or trade acceptance.
    m.trade.as_mut().unwrap().offered[Side::Player as usize].insert((FormId(KEY_VALUED), None, 0), 1);
    m.refresh(&mut w).unwrap();
    assert!(m.trade.as_ref().unwrap().is_empty());
    assert_eq!((count(&mut w, 0x14, KEY_VALUED), count(&mut w, BOX_REF, KEY_VALUED)), (2, 2));

    // Controls: containers and the Pip-Boy inventory still list keys as before.
    let mut cont = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    cont.apply(&mut w, Command::SetCategory(Side::Container, Category::Misc));
    cont.apply(&mut w, Command::SetCategory(Side::Player, Category::Misc));
    assert!(has(&cont, Side::Container, FormId(KEY_A)) && has(&cont, Side::Container, FormId(KEY_VALUED)));
    assert!(has(&cont, Side::Player, FormId(KEY_A)) && has(&cont, Side::Player, FormId(KEY_VALUED)));

    let mut inv = MenuModel::open_inventory(&mut w).unwrap();
    assert!(has(&inv, Side::Player, FormId(KEY_A)) && has(&inv, Side::Player, FormId(KEY_VALUED)));
    inv.apply(&mut w, Command::SetCategory(Side::Player, Category::Misc));
    assert!(names(inv.list(Side::Player)).contains(&("Keyring", 3)));
}

/// #3442: ammo the weapon cannot fire is dim; the weapon's other kinds and every
/// row that is not ammo stay full; with the fists out every ammo row is dim; not
/// knowing the weapon dims nothing [observed: observer check 288].
#[test]
fn ammo_the_weapon_cannot_fire_is_dim() {
    let row = |item: u32, kind: ItemKind| Row {
        item: FormId(item), at: None, name: format!("Item {item}"), count: 1, weight: None, value: None,
        kind: Some(kind), equipped: false, quest: false, condition: None, icon: None, price: None,
        bartered: false, loaded: false, dim: true, mods: 0, mods_value: 0, folder: None, reference: None, live: false, ysi: None,
    };
    let mut rows = vec![row(1, ItemKind::Ammo), row(2, ItemKind::Ammo), row(3, ItemKind::Ammo), row(4, ItemKind::Weapon)];
    let dims = |rows: &[Row]| rows.iter().map(|r| r.dim).collect::<Vec<_>>();
    // A .357 revolver: .357 and .38 full, the rest dim.
    mark_dim(&mut rows, Some(&[FormId(1), FormId(2)]));
    assert_eq!(dims(&rows), [false, false, true, false]);
    // Fists: every ammo row dim, the weapon row not.
    mark_dim(&mut rows, Some(&[]));
    assert_eq!(dims(&rows), [true, true, true, false]);
    // Unknown: nothing dim.
    mark_dim(&mut rows, None);
    assert_eq!(dims(&rows), [false, false, false, false]);
}

/// #3799: `quantity-select-short-keys` (off by default) leaves vanilla Inventory,
/// Container, and Barter transfers/drops unchanged when off; when enabled, Shift skips
/// `QuantityMenu` for the whole stack, Ctrl skips `QuantityMenu` for a single item, and
/// weightless container stacks (`count > 1`) open `QuantityMenu`.
#[test]
fn quantity_select_short_keys_container_barter_inventory_and_weightless_pickup() {
    use crate::tiles::container_menu::row_commands_with;

    // 1. Default OFF: vanilla container transfer & weightless pickup unchanged.
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let start_bullets = count(&mut w, 0x14, BULLETS);
    if start_bullets > 0 {
        w.remove_item(PLAYER, FormId(BULLETS), start_bullets).unwrap();
    }
    w.add_item(FormId(BOX_REF), FormId(BULLETS), 3).unwrap();
    w.add_item(FormId(BOX_REF), FormId(CAN), 10).unwrap();
    w.add_item(PLAYER, FormId(CAN), 12).unwrap();

    let mut m = MenuModel::open_container(&mut w, FormId(BOX_REF)).unwrap();
    assert!(!m.qtt_select_short_keys, "option must default to false");
    m.focus = Side::Container;
    let ammo_idx = m.list(Side::Container).rows.iter().position(|r| r.item == FormId(BULLETS)).unwrap();
    m.selected[Side::Container as usize] = ammo_idx;
    // Off: 3 non-hardcore weightless ammo (<= ASK_QUANTITY_ABOVE) takes 1 without opening QuantityMenu.
    let a = m.apply(&mut w, Command::One);
    assert!(a.changed);
    assert!(m.quantity.is_none(), "vanilla off must not open quantity prompt for 3 weightless ammo");
    assert_eq!(count(&mut w, 0x14, BULLETS), 1);
    assert_eq!(count(&mut w, BOX_REF, BULLETS), 2);

    // 2. Enabled ON: weightless container pickup (`count > 1`) opens QuantityMenu.
    m.qtt_select_short_keys = true;
    let ammo_idx = m.list(Side::Container).rows.iter().position(|r| r.item == FormId(BULLETS)).unwrap();
    m.selected[Side::Container as usize] = ammo_idx;
    let a = m.apply(&mut w, Command::One);
    assert!(!a.changed);
    let q = m.quantity.expect("enabled option opens quantity prompt for 2 weightless ammo in container");
    assert_eq!((q.max, q.amount, q.side), (2, 2, Side::Container));
    // Cancel the prompt, then test Ctrl (OneSingle) and Shift (Stack) bypass QuantityMenu.
    m.apply(&mut w, Command::AmountCancel);
    assert!(m.quantity.is_none());
    let a = m.apply(&mut w, Command::OneSingle);
    assert!(a.changed);
    assert!(m.quantity.is_none(), "Ctrl single-item shortcut skips quantity prompt");
    assert_eq!(count(&mut w, 0x14, BULLETS), 2);
    assert_eq!(count(&mut w, BOX_REF, BULLETS), 1);
    // Single remaining weightless ammo (count == 1) does NOT open quantity prompt.
    let ammo_idx = m.list(Side::Container).rows.iter().position(|r| r.item == FormId(BULLETS)).unwrap();
    m.selected[Side::Container as usize] = ammo_idx;
    let a = m.apply(&mut w, Command::One);
    assert!(a.changed);
    assert!(m.quantity.is_none(), "count == 1 weightless item takes directly without prompt");
    assert_eq!(count(&mut w, 0x14, BULLETS), 3);

    // Storing a weightless stack from player -> container does NOT trigger container-pickup rule.
    m.focus = Side::Player;
    let p_ammo_idx = m.list(Side::Player).rows.iter().position(|r| r.item == FormId(BULLETS)).unwrap();
    m.selected[Side::Player as usize] = p_ammo_idx;
    let a = m.apply(&mut w, Command::One);
    assert!(a.changed);
    assert!(m.quantity.is_none(), "storing 3 weightless ammo into container keeps ASK_QUANTITY_ABOVE");
    assert_eq!(count(&mut w, BOX_REF, BULLETS), 1);

    // Hardcore mode gives ammo weight: 3 hardcore ammo in container does NOT open quantity prompt.
    w.set_hardcore(true);
    w.add_item(FormId(BOX_REF), FormId(BULLETS), 2).unwrap();
    m.refresh(&mut w).unwrap();
    m.focus = Side::Container;
    let ammo_idx = m.list(Side::Container).rows.iter().position(|r| r.item == FormId(BULLETS)).unwrap();
    m.selected[Side::Container as usize] = ammo_idx;
    let a = m.apply(&mut w, Command::One);
    assert!(a.changed);
    assert!(m.quantity.is_none(), "hardcore ammo has weight (0.1) so stack of 3 keeps ASK_QUANTITY_ABOVE");

    // Weighted large stack (> ASK_QUANTITY_ABOVE): One asks, OneSingle takes 1, Stack takes all.
    w.set_hardcore(false);
    m.refresh(&mut w).unwrap();
    let can_idx = m.list(Side::Container).rows.iter().position(|r| r.item == FormId(CAN)).unwrap();
    m.selected[Side::Container as usize] = can_idx;
    let before_box_cans = count(&mut w, BOX_REF, CAN);
    assert!(before_box_cans > ASK_QUANTITY_ABOVE);
    m.apply(&mut w, Command::One);
    assert!(m.quantity.is_some(), "plain One opens QuantityMenu on large stack");
    m.apply(&mut w, Command::AmountCancel);
    let a = m.apply(&mut w, Command::OneSingle);
    assert!(a.changed && m.quantity.is_none(), "Ctrl OneSingle transfers 1 without prompt");
    assert_eq!(count(&mut w, BOX_REF, CAN), before_box_cans - 1);
    let a = m.apply(&mut w, Command::Stack);
    assert!(a.changed && m.quantity.is_none(), "Shift Stack transfers remaining stack without prompt");
    assert_eq!(count(&mut w, BOX_REF, CAN), 0);

    // 3. Barter: One asks on large stack, OneSingle offers 1, Stack offers whole stack.
    let mut wb = World::new(&f.forms, &f.refs, SimConfig::default());
    wb.add_item(PLAYER, FormId(CAN), 12).unwrap();
    let mut b = barter(&mut wb);
    b.qtt_select_short_keys = true;
    b.focus = Side::Player;
    let b_can = b.list(Side::Player).rows.iter().position(|r| r.item == FormId(CAN)).unwrap();
    b.selected[Side::Player as usize] = b_can;
    b.apply(&mut wb, Command::One);
    assert!(b.quantity.is_some(), "plain barter One opens QuantityMenu on stack of 12");
    b.apply(&mut wb, Command::AmountCancel);
    let a = b.apply(&mut wb, Command::OneSingle);
    assert!(a.changed && b.quantity.is_none(), "barter Ctrl OneSingle offers 1 without prompt");
    assert_eq!(b.trade.as_ref().unwrap().offered[Side::Player as usize].get(&(FormId(CAN), None, 0)).copied(), Some(1));
    let b_can = b.list(Side::Player).rows.iter().position(|r| r.item == FormId(CAN) && !r.bartered).unwrap();
    b.selected[Side::Player as usize] = b_can;
    let a = b.apply(&mut wb, Command::Stack);
    assert!(a.changed && b.quantity.is_none(), "barter Shift Stack offers remaining 11 without prompt");
    assert_eq!(b.trade.as_ref().unwrap().offered[Side::Player as usize].get(&(FormId(CAN), None, 0)).copied(), Some(12));

    // 4. Inventory: Drop asks on large stack, DropSingle drops 1, DropStack drops rest.
    let mut wi = World::new(&f.forms, &f.refs, SimConfig::default());
    wi.add_item(PLAYER, FormId(CAN), 10).unwrap();
    let mut inv = MenuModel::open_inventory(&mut wi).unwrap();
    inv.qtt_select_short_keys = true;
    inv.apply(&mut wi, Command::SetCategory(Side::Player, Category::Misc));
    let i_can = inv.list(Side::Player).rows.iter().position(|r| r.item == FormId(CAN)).unwrap();
    inv.selected[Side::Player as usize] = i_can;
    inv.apply(&mut wi, Command::Drop);
    assert!(inv.quantity.is_some(), "plain Drop opens QuantityMenu on stack of 10");
    inv.apply(&mut wi, Command::AmountCancel);
    let a = inv.apply(&mut wi, Command::DropSingle);
    assert!(a.changed && inv.quantity.is_none(), "Ctrl DropSingle drops 1 without prompt");
    assert_eq!(count(&mut wi, 0x14, CAN), 9);
    let a = inv.apply(&mut wi, Command::DropStack);
    assert!(a.changed && inv.quantity.is_none(), "Shift DropStack drops remaining 9 without prompt");
    assert_eq!(count(&mut wi, 0x14, CAN), 0);

    // 5. Tiled row_commands_with: Shift takes precedence over Ctrl; off keeps vanilla Select/One.
    assert_eq!(row_commands_with(Side::Container, 2, true, false, false), vec![Command::Select(Side::Container, 2)]);
    assert_eq!(row_commands_with(Side::Container, 2, true, false, true), vec![Command::OneSingle]);
    assert_eq!(row_commands_with(Side::Container, 2, true, true, false), vec![Command::Stack]);
    assert_eq!(row_commands_with(Side::Container, 2, true, true, true), vec![Command::Stack]);
    assert_eq!(row_commands_with(Side::Container, 2, false, false, false), vec![Command::Hover(Side::Container, 2), Command::One]);
    assert_eq!(row_commands_with(Side::Container, 2, false, false, true), vec![Command::Hover(Side::Container, 2), Command::OneSingle]);
    assert_eq!(row_commands_with(Side::Container, 2, false, true, true), vec![Command::Hover(Side::Container, 2), Command::Stack]);
}

#[test]
fn quantity_select_short_keys_setting_defaults_off_syncs_and_round_trips_ini() {
    use crate::menu::{sync_quantity_select_short_keys, ActiveMenu, MenuPlugin, QuantitySelectShortKeys};
    use crate::PlayState;
    use bevy::app::PostStartup;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::MinimalPlugins;

    let dir = std::env::temp_dir().join(format!("nvf-qtt-short-keys-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("NVForever.ini");

    let make_app = |ini_path: &std::path::Path| {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_state::<PlayState>()
            .insert_resource(crate::settings::SettingsFile(Some(ini_path.to_path_buf())))
            .init_resource::<crate::settings::GivenSwitches>()
            .add_plugins(MenuPlugin)
            .add_systems(PostStartup, crate::settings::apply_stored);
        app.world_mut().run_schedule(PostStartup);
        app
    };

    // 1. Fresh start with no INI: defaults to off, within note budget.
    let mut app = make_app(&path);
    assert!(!app.world().resource::<QuantitySelectShortKeys>().0, "must default to off");
    let idx = app.world().resource::<crate::settings::Settings>().index("quantity-select-short-keys").expect("setting registered");
    let row = &app.world().resource::<crate::settings::Settings>().list()[idx];
    assert_eq!(crate::settings::note_over_budget(row.note), None, "note must fit within 200-char budget");

    // Live sync propagates to an open MenuModel when flipped on.
    app.world_mut().resource_mut::<ActiveMenu>().0 = Some(MenuModel::empty(Mode::Inventory, "Inv".into()));
    assert_eq!(crate::settings::flip(app.world_mut(), idx), Some(true));
    app.world_mut().run_system_once(sync_quantity_select_short_keys).unwrap();
    assert!(app.world().resource::<QuantitySelectShortKeys>().0);
    assert!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().qtt_select_short_keys);
    let ini = std::fs::read_to_string(&path).unwrap();
    assert!(ini.contains("quantity-select-short-keys=1"), "persisted to NVForever.ini: {ini}");

    // 2. Next start loads the persisted ON value from NVForever.ini.
    let mut reloaded = make_app(&path);
    assert!(reloaded.world().resource::<QuantitySelectShortKeys>().0, "restored from NVForever.ini");

    // 3. Flipping back off writes 0 and restores off on next start.
    let idx2 = reloaded.world().resource::<crate::settings::Settings>().index("quantity-select-short-keys").unwrap();
    assert_eq!(crate::settings::flip(reloaded.world_mut(), idx2), Some(false));
    let ini_off = std::fs::read_to_string(&path).unwrap();
    assert!(ini_off.contains("quantity-select-short-keys=0"), "off persisted: {ini_off}");
    let reloaded_off = make_app(&path);
    assert!(!reloaded_off.world().resource::<QuantitySelectShortKeys>().0, "off restored from NVForever.ini");

    let _ = std::fs::remove_dir_all(&dir);
}
