//! Co-op kill credit (`fnv_game::coop_kills`, coop-shared-progress.md §4.2): the killer
//! 100 %, each other player in combat with the target half (at least 1), nobody else;
//! karma and the kill count the killer's only, a guest's queued for its own character;
//! the 40 % damage threshold counts every player's damage together.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::combat::HitInfo;
use fnv_game::coop_kills::{share_of, SHARE_WINDOW};
use fnv_game::crime::Note;
use fnv_game::game::{Game, GameConfig};
use fnv_game::GameData;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const TARGET: u32 = 0x800;
const MATE: u32 = 0x801;
const TARGET_REF: FormId = FormId(0x300);
/// The guests' stand-in actors and a stranger (no player).
const GUEST_A: FormId = FormId(0x301);
const GUEST_B: FormId = FormId(0x302);
const STRANGER: FormId = FormId(0x303);
const CELL: u32 = 0x200;

fn acbs() -> fnv_esm::raw::Subrecord {
    let mut d = 0u32.to_le_bytes().to_vec();
    d.extend([0u8; 16]);
    d.extend(0i16.to_le_bytes());
    d.extend(0u16.to_le_bytes());
    sub(b"ACBS", d)
}

fn npc(fid: u32, edid: &str, health: i32) -> Vec<u8> {
    record(b"NPC_", 0, fid, &[sub(b"EDID", z(edid)), acbs(), sub(b"DATA", [health.to_le_bytes().to_vec(), vec![5; 7]].concat())])
}

fn placed(fid: FormId, base: u32, x: f32) -> Vec<u8> {
    let data = [x, 0.0, 0.0, 0.0, 0.0, 1.0].iter().flat_map(|v| v.to_le_bytes()).collect();
    record(b"ACHR", 0, fid.0, &[sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
}

fn plugin_bytes() -> Vec<u8> {
    let npcs = [npc(0x7, "Player", 100), npc(TARGET, "TestTarget", 100), npc(MATE, "TestMate", 100)].concat();
    let refs = [placed(TARGET_REF, TARGET, 0.0), placed(GUEST_A, MATE, 300.0), placed(GUEST_B, MATE, 600.0), placed(STRANGER, MATE, 900.0)].concat();
    let c = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
    let cells = [c, group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &refs))].concat();
    [tes4(), top(b"NPC_", &npcs), top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cells)))].concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "CoopKills.esm".into()).unwrap()];
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

/// A co-op host with two guests.
fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig::default());
    g.new_game();
    g.player_entered(Some(FormId(CELL)));
    g.coop_actors.insert(GUEST_A, "A".into());
    g.coop_actors.insert(GUEST_B, "B".into());
    g
}

fn by(attacker: FormId) -> HitInfo {
    HitInfo { attacker: Some(attacker), location: -1, ..HitInfo::default() }
}

/// The host's XP and karma change for killing the target alone.
fn host_alone(f: &Fixture) -> (f64, f64) {
    let mut g = game(f);
    let out = g.hit(TARGET_REF, 10_000.0, by(FormId::PLAYER_REF));
    assert!(out.died);
    (out.xp, karma(&g))
}

fn karma(g: &Game) -> f64 {
    g.crimes.notes.iter().map(|n| if let Note::Karma { change, .. } = n { *change } else { 0.0 }).sum()
}

fn max_health(f: &Fixture) -> f64 {
    game(f).hit(TARGET_REF, 0.0, by(STRANGER)).max_health
}

#[test]
fn the_killer_gets_all_an_in_combat_player_half_the_others_none() {
    let f = fixture();
    let (full, _) = host_alone(&f);
    assert!(full > 0.0);
    let mut g = game(&f);
    // B hit it long ago (out of combat); the host hit it just now; A kills.
    g.hit(TARGET_REF, 1.0, by(GUEST_B));
    g.time += SHARE_WINDOW + 1.0;
    g.hit(TARGET_REF, 1.0, by(FormId::PLAYER_REF));
    let out = g.hit(TARGET_REF, 10_000.0, by(GUEST_A));
    assert!(out.died);
    assert_eq!(out.xp, share_of(full), "the host earns half");
    let credits = g.take_coop_kill_credits();
    assert_eq!(credits.len(), 1, "B was not in combat: {credits:?}");
    let c = &credits[0];
    assert!(c.player == GUEST_A && c.killer && c.target == TARGET_REF);
    assert_eq!(c.xp, full);
    assert!(g.take_coop_kill_credits().is_empty(), "drained");
}

#[test]
fn the_host_kill_shares_with_a_guest_in_combat() {
    let f = fixture();
    let (full, _) = host_alone(&f);
    let mut g = game(&f);
    g.hit(TARGET_REF, 1.0, by(GUEST_B));
    let out = g.hit(TARGET_REF, 10_000.0, by(FormId::PLAYER_REF));
    assert_eq!(out.xp, full);
    let credits = g.take_coop_kill_credits();
    assert_eq!(credits.len(), 1);
    assert!(credits[0].player == GUEST_B && !credits[0].killer && credits[0].karma == 0.0);
    assert_eq!(credits[0].xp, share_of(full));
    // At least 1 of a little XP, nothing of none.
    assert_eq!(share_of(1.0), 1.0);
    assert_eq!(share_of(0.0), 0.0);
}

#[test]
fn a_guest_kill_karma_is_the_guests_not_the_hosts() {
    let f = fixture();
    let (_, host_karma) = host_alone(&f);
    assert!(host_karma != 0.0, "the fixture's kill is a murder");
    let mut g = game(&f);
    g.hit(TARGET_REF, 10_000.0, by(GUEST_A));
    assert_eq!(karma(&g), 0.0, "the host's karma stays");
    let credits = g.take_coop_kill_credits();
    assert_eq!(credits.len(), 1);
    assert_eq!(credits[0].karma, host_karma);
    // The guest's game takes the XP and the karma.
    let mut guest = game(&f);
    let gained = guest.take_coop_kill_credit(&credits[0]);
    assert_eq!(gained, credits[0].xp);
    assert_eq!(karma(&guest), host_karma);
}

#[test]
fn the_threshold_counts_every_players_damage() {
    let f = fixture();
    let (full, _) = host_alone(&f);
    let max = max_health(&f);
    // A stranger 55 %, the guest 30 %, the host the last 15 %: 45 % by the players.
    let mut g = game(&f);
    g.hit(TARGET_REF, max * 0.55, by(STRANGER));
    g.hit(TARGET_REF, max * 0.30, by(GUEST_A));
    let out = g.hit(TARGET_REF, 10_000.0, by(FormId::PLAYER_REF));
    assert!(out.died);
    assert_eq!(out.xp, full);
    // Without the guest's part the host's 15 % is too little.
    let mut g = game(&f);
    g.hit(TARGET_REF, max * 0.85, by(STRANGER));
    assert_eq!(g.hit(TARGET_REF, 10_000.0, by(FormId::PLAYER_REF)).xp, 0.0);
}

#[test]
fn a_host_kill_after_a_guests_damage_still_earns() {
    let f = fixture();
    let (full, _) = host_alone(&f);
    let max = max_health(&f);
    let mut g = game(&f);
    g.hit(TARGET_REF, max * 0.9, by(GUEST_A));
    let out = g.hit(TARGET_REF, 10_000.0, by(FormId::PLAYER_REF));
    assert_eq!(out.xp, full);
    assert_eq!(g.take_coop_kill_credits()[0].xp, share_of(full));
}
