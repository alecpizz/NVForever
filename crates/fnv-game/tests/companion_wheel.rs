//! The companion wheel (docs/spec-notes/companion-wheel.md) on a synthetic plugin built in
//! memory: a companion whose command lines set its variables as the shipped
//! VNPCFollowers lines do. No game data.

use crate::common;

use common::*;
use fnv_esm::raw::Subrecord;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::companion_wheel::{Command, Outcome, MENU_CODE};
use fnv_game::functions::add_actor_value_damage;
use fnv_game::functions::senses::Motion;
use fnv_game::game::{Game, GameConfig};
use fnv_game::GameData;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const QUEST: u32 = 0x900;
const NPC: u32 = 0x920;
const ROBOT_CREA: u32 = 0x921;
const SCRIPT: u32 = 0x930;
const STIMPAK: u32 = 0x940;
const SCRAP_METAL: u32 = 0x941;
const FOLLOW: u32 = 0x950;
const CELL: u32 = 0x200;
const REF: u32 = 0x320;
const OTHER_REF: u32 = 0x321;
const ROBOT_REF: u32 = 0x322;

/// The companion script's variables, as the shipped companions name them.
const WAITING: u16 = 1;
const AGGRESSIVE: u16 = 2;
const LONG: u16 = 3;
const RANGED: u16 = 4;

const HEALTH: u16 = 16;
const OPEN_TEAMMATE_CONTAINER: u16 = 0x11C8;

/// Topic FormIDs and their lines.
const TOPICS: [(u32, &str); 11] = [
    (0xA00, "GREETING"),
    (0xA01, "FollowersWait"),
    (0xA02, "FollowersLetsGo"),
    (0xA03, "FollowersTacticsCombatAggressive"),
    (0xA04, "FollowersTacticsCombatPassive"),
    (0xA05, "FollowersTacticsDistanceLong"),
    (0xA06, "FollowersTacticsDistanceDefault"),
    (0xA07, "FollowersTacticsCombatRanged"),
    (0xA08, "FollowersTacticsCombatMelee"),
    (0xA09, "FollowersTrade"),
    (0xA0A, "Regenerating"),
];
const STEALTHING: u32 = 0xA0B;

fn u32b(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn r(i: u16) -> Vec<u8> {
    [vec![b'r'], le16(i)].concat()
}

/// `set CompanionREF.<var> to <v>` as a result script.
fn set_ref(var_index: u16, v: &str) -> Vec<Subrecord> {
    unit(0, &set([r(1), var(b's', var_index)].concat(), ex(&[a(v)])), &[], &[REF])
}

/// A result script that calls a function without parameters on the speaker.
fn call0(op: u16) -> Vec<Subrecord> {
    unit(0, &st(op, &le16(0)), &[], &[])
}

/// A line of the companion's: begin and end result scripts.
fn line(id: u32, text: &str, begin: Vec<Subrecord>, end: Vec<Subrecord>) -> Vec<u8> {
    let trdt = [u32b(0), u32b(50), vec![0; 4], vec![1, 0, 0, 0], u32b(0), vec![0; 4]].concat();
    let mut s = vec![sub(b"DATA", vec![0, 0, 0, 0]), sub(b"QSTI", u32b(QUEST))];
    s.extend([sub(b"TRDT", trdt), sub(b"NAM1", z(text)), sub(b"NAM2", z("")), sub(b"NAM3", z(""))]);
    s.extend(if begin.is_empty() { unit(0, &[], &[], &[]) } else { begin });
    s.push(sub(b"NEXT", vec![]));
    s.extend(if end.is_empty() { unit(0, &[], &[], &[]) } else { end });
    s.push(sub(b"ANAM", u32b(NPC)));
    record(b"INFO", 0, id, &s)
}

fn topic(id: u32, edid: &str, lines: &[u8]) -> Vec<u8> {
    let dial = record(b"DIAL", 0, id, &[sub(b"EDID", z(edid)), sub(b"FULL", z(edid)), sub(b"PNAM", f32b(50.0)), sub(b"DATA", vec![0, 0])]);
    [dial, group(id.to_le_bytes(), 7, lines)].concat()
}

fn plugin_bytes() -> Vec<u8> {
    let quest = record(b"QUST", 0, QUEST, &[sub(b"EDID", z("VNPCFollowers")), sub(b"DATA", [vec![1u8, 50, 0, 0], f32b(0.0)].concat())]);
    let vars = [(WAITING as u32, "Waiting", 1u8), (AGGRESSIVE as u32, "FollowerSwitchAggressive", 1), (LONG as u32, "IsFollowingLong", 1), (RANGED as u32, "CombatStyleRanged", 1)];
    let scpt = record(b"SCPT", 0, SCRIPT, &[vec![sub(b"EDID", z("CompanionScript"))], unit(0, &script(&[(0, None, vec![])]), &vars, &[])].concat());
    let npc = record(b"NPC_", 0, NPC, &[sub(b"EDID", z("Companion")), sub(b"SCRI", u32b(SCRIPT)), sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat())]);
    // The carry weight's settings (the masters' values).
    let gmst = [
        record(b"GMST", 0, 0x960, &[sub(b"EDID", z("fAVDCarryWeightsBase")), sub(b"DATA", f32b(150.0))]),
        record(b"GMST", 0, 0x961, &[sub(b"EDID", z("fAVDCarryWeightMult")), sub(b"DATA", f32b(10.0))]),
    ]
    .concat();
    // The player, so that the player has an inventory.
    let player = record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player")), sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat())]);
    let enit = [u32s(&[1]), vec![4, 0, 0, 0], u32s(&[0]), f32b(0.0), u32s(&[0])].concat();
    let stim = record(b"ALCH", 0, STIMPAK, &[sub(b"EDID", z("Stimpak")), sub(b"ETYP", 11i32.to_le_bytes().to_vec()), sub(b"DATA", f32b(0.0)), sub(b"ENIT", enit)]);
    let scrap = record(b"MISC", 0, SCRAP_METAL, &[sub(b"EDID", z("SpareParts")), sub(b"DATA", [1i32.to_le_bytes().to_vec(), f32b(1.0)].concat())]);
    // Robot creature (creature_type = 6, Robot) with 100 base health (DATA bytes 4..8 = health: u32 in FNV CREA).
    let crea_data = [vec![6u8, 0, 0, 0], 100u32.to_le_bytes().to_vec(), vec![0; 9]].concat();
    let robot = record(b"CREA", 0, ROBOT_CREA, &[sub(b"EDID", z("EDE")), sub(b"DATA", crea_data)]);
    // A Follow package on the player (PTDT: a specific reference, the player).
    let pack = record(
        b"PACK",
        0,
        FOLLOW,
        &[
            sub(b"EDID", z("CompanionFollow")),
            sub(b"PKDT", [&0u32.to_le_bytes()[..], &[1, 0], &0u16.to_le_bytes(), &0u16.to_le_bytes(), &[0, 0]].concat()),
            sub(b"PSDT", [&[0xFFu8, 0xFF, 0, 0xFF][..], &0u32.to_le_bytes()].concat()),
            sub(b"PTDT", [0i32.to_le_bytes(), 0x14u32.to_le_bytes(), 0i32.to_le_bytes(), 0f32.to_le_bytes()].concat()),
            sub(b"PKFD", 330f32.to_le_bytes().to_vec()),
        ],
    );
    let mut dial = Vec::new();
    for (i, (id, edid)) in TOPICS.iter().enumerate() {
        let info = 0xB00 + i as u32;
        let (begin, end) = match *edid {
            "FollowersWait" => (set_ref(WAITING, "1"), vec![]),
            // As the shipped FollowersLetsGo lines: Waiting is cleared by the end script.
            "FollowersLetsGo" => (vec![], set_ref(WAITING, "0")),
            "FollowersTacticsCombatAggressive" => (set_ref(AGGRESSIVE, "1"), vec![]),
            "FollowersTacticsCombatPassive" => (set_ref(AGGRESSIVE, "0"), vec![]),
            "FollowersTacticsDistanceLong" => (set_ref(LONG, "1"), vec![]),
            "FollowersTacticsDistanceDefault" => (set_ref(LONG, "0"), vec![]),
            "FollowersTacticsCombatRanged" => (set_ref(RANGED, "1"), vec![]),
            "FollowersTacticsCombatMelee" => (set_ref(RANGED, "0"), vec![]),
            "FollowersTrade" => (call0(OPEN_TEAMMATE_CONTAINER), vec![]),
            _ => (vec![], vec![]),
        };
        dial.extend(topic(*id, edid, &line(info, edid, begin, end)));
    }
    dial.extend(topic(STEALTHING, "FollowersStealthing", &line(0xB20, "Going quiet", vec![], vec![])));
    let placed = |id: u32, edid: &str| record(b"ACHR", 0, id, &[sub(b"EDID", z(edid)), sub(b"NAME", u32b(NPC)), sub(b"DATA", vec![0; 24])]);
    let placed_robot = record(b"ACRE", 0, ROBOT_REF, &[sub(b"EDID", z("EDERef")), sub(b"NAME", u32b(ROBOT_CREA)), sub(b"DATA", vec![0; 24])]);
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("WheelCell"))]);
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &[placed(REF, "CompanionREF"), placed(OTHER_REF, "StrangerREF"), placed_robot].concat()));
    [
        tes4(),
        top(b"GMST", &gmst),
        top(b"SCPT", &scpt),
        top(b"NPC_", &[player, npc].concat()),
        top(b"CREA", &robot),
        top(b"ALCH", &stim),
        top(b"MISC", &scrap),
        top(b"PACK", &pack),
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
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Wheel.esm".into()).unwrap()];
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

fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() });
    g.new_game();
    g
}

fn companion() -> FormId {
    FormId(REF)
}

/// A hired companion: a teammate, its wheel open.
fn hired(f: &Fixture) -> Game<'_> {
    let mut g = game(f);
    g.extra.entry(companion()).or_default().teammate = true;
    assert!(g.open_companion_wheel(companion()));
    g
}

fn var_of(g: &Game, name: &str) -> f64 {
    g.ref_var_named(companion(), name).expect("declared")
}

fn button(g: &mut Game, c: Command) -> fnv_game::companion_wheel::State {
    g.wheel_buttons(companion()).into_iter().find(|b| b.command == c).unwrap()
}

/// Frames of menu mode: the companion's reply goes on behind the wheel.
fn frames(g: &mut Game, n: u64) {
    for _ in 0..n {
        g.tick();
    }
}

#[test]
fn the_wheel_opens_only_on_a_teammate_and_is_menu_1075() {
    let f = fixture();
    let mut g = game(&f);
    assert!(!g.has_companion_wheel(companion()));
    assert!(!g.open_companion_wheel(companion()));
    g.extra.entry(companion()).or_default().teammate = true;
    assert!(g.open_companion_wheel(companion()));
    assert_eq!(g.menu, Some(MENU_CODE));
    assert_eq!(g.wheel_companion(), Some(companion()));
    assert!(g.in_menu());
    assert!(g.close_companion_wheel());
    assert_eq!(g.menu, None);
    assert_eq!(g.wheel_companion(), None);
}

#[test]
fn wait_and_follow_say_the_followers_topics_and_their_results_set_waiting() {
    let f = fixture();
    let mut g = hired(&f);
    let b = button(&mut g, Command::StayFollow);
    assert!(!b.alt && b.enabled && b.label == "Wait");
    let out = g.wheel_command(Command::StayFollow).unwrap();
    assert!(matches!(out, Outcome::Said { topic, .. } if topic == FormId(0xA01)), "{out:?}");
    // The begin script ran at once.
    assert_eq!(var_of(&g, "Waiting"), 1.0);
    let b = button(&mut g, Command::StayFollow);
    assert!(b.alt && b.label == "Follow");
    frames(&mut g, 600);
    // Let's Go clears Waiting in its end script, which a wheel command runs as the line
    // starts (owner, 2026-10-27), and not again when the line is over.
    let out = g.wheel_command(Command::StayFollow).unwrap();
    assert!(matches!(out, Outcome::Said { topic, .. } if topic == FormId(0xA02)), "{out:?}");
    assert_eq!(var_of(&g, "Waiting"), 0.0);
    let b = button(&mut g, Command::StayFollow);
    assert!(!b.alt && b.label == "Wait");
    assert!(g.line_goes_on_in_menu(companion()));
    assert!(g.speech[0].end_ran, "kept from running again at the line's end");
    frames(&mut g, 600);
    assert!(g.speech.is_empty(), "the line is over");
    assert_eq!(var_of(&g, "Waiting"), 0.0);
    assert_eq!(g.menu, Some(MENU_CODE), "the wheel stays open");
    let time = g.time;
    frames(&mut g, 10);
    assert_eq!(g.time, time, "game time stands still behind the wheel");
}

#[test]
fn the_toggles_say_their_topics_by_the_companions_state() {
    let f = fixture();
    let mut g = hired(&f);
    for (c, name) in [(Command::AggressivePassive, "FollowerSwitchAggressive"), (Command::NearFar, "IsFollowingLong"), (Command::RangedMelee, "CombatStyleRanged")] {
        assert_eq!(var_of(&g, name), 0.0);
        g.wheel_command(c).unwrap();
        assert_eq!(var_of(&g, name), 1.0, "{c:?} on");
        assert!(button(&mut g, c).alt);
        frames(&mut g, 600);
        g.wheel_command(c).unwrap();
        assert_eq!(var_of(&g, name), 0.0, "{c:?} off");
        frames(&mut g, 600);
    }
}

#[test]
fn talk_closes_the_wheel_and_inventory_asks_for_the_teammates_container() {
    let f = fixture();
    let mut g = hired(&f);
    let b = button(&mut g, Command::OpenInventory);
    assert!(b.context.as_deref().is_some_and(|c| c == "Weight 0/200"), "{b:?}");
    assert!(matches!(g.wheel_command(Command::OpenInventory).unwrap(), Outcome::Said { .. }));
    assert_eq!(g.teammate_containers, vec![companion()]);
    let out = g.wheel_command(Command::Talk).unwrap();
    assert!(matches!(out, Outcome::Talk { info } if info == FormId(0xB00)), "{out:?}");
    assert_eq!(g.wheel_companion(), None);
    assert!(g.dialogue.is_some());
}

#[test]
fn the_inventory_line_goes_on_while_the_inventory_is_open() {
    let f = fixture();
    let mut g = hired(&f);
    assert!(matches!(g.wheel_command(Command::OpenInventory).unwrap(), Outcome::Said { .. }));
    // The engine closes the wheel and opens the container menu (`nvf-play` menu.rs).
    g.close_companion_wheel();
    g.open_menu(fnv_game::runner::CONTAINER_MENU);
    assert!(g.line_goes_on_in_menu(companion()), "said as the inventory comes up");
    let left = g.speech[0].remaining;
    frames(&mut g, 10);
    assert!(g.speech[0].remaining < left, "its time runs behind the inventory");
    frames(&mut g, 600);
    assert!(g.speech.is_empty());
    assert!(!g.line_goes_on_in_menu(companion()));
}

#[test]
fn a_stimpak_is_used_on_a_hurt_companion() {
    let f = fixture();
    let mut g = hired(&f);
    let stim = FormId(STIMPAK);
    // Not hurt, no stimpak: the button cannot be used.
    assert!(!button(&mut g, Command::UseStimpak).enabled);
    assert_eq!(g.wheel_unavailable_reason(Command::UseStimpak).as_deref(), Some("You have no stimpaks."));
    assert_eq!(g.wheel_command(Command::UseStimpak).unwrap(), Outcome::Refused);
    g.world.add_item(FormId::PLAYER_REF, stim, 2).unwrap();
    assert!(!button(&mut g, Command::UseStimpak).enabled, "at full health");
    assert_eq!(g.wheel_unavailable_reason(Command::UseStimpak).as_deref(), Some("The companion is already at full health."));
    add_actor_value_damage(&mut g, companion(), HEALTH, 40.0);
    let b = button(&mut g, Command::UseStimpak);
    assert!(b.enabled, "{b:?}");
    assert!(g.wheel_unavailable_reason(Command::UseStimpak).is_none());
    assert!(b.context.as_deref().is_some_and(|c| c.ends_with("Stimpak 2")), "{b:?}");
    let out = g.wheel_command(Command::UseStimpak).unwrap();
    assert!(matches!(out, Outcome::Stimpak { item, line: Some(l) } if item == stim && l == FormId(0xB0A)), "{out:?}");
    assert_eq!(g.world.item_count(FormId::PLAYER_REF, stim).unwrap(), 1);
}

#[test]
fn robot_companion_scrap_heal_unavailable_reason_tracks_scrap_metal_not_stimpaks() {
    let f = fixture();
    let mut g = game(&f);
    let stim = FormId(STIMPAK);
    let scrap = FormId(SCRAP_METAL);
    let ede = FormId(ROBOT_REF);
    g.extra.entry(ede).or_default().teammate = true;
    g.tweaks.robots_heal_with_scrap = true;
    assert!(g.open_companion_wheel(ede));

    // Case 1: Full health with Scrap Metal -> disabled with full-health reason.
    g.world.add_item(FormId::PLAYER_REF, scrap, 2).unwrap();
    let buttons = g.wheel_buttons(ede);
    let stim_btn = buttons.iter().find(|b| b.command == Command::UseStimpak).unwrap();
    assert!(!stim_btn.enabled, "full-health robot must have disabled heal button");
    assert_eq!(g.wheel_unavailable_reason_for(ede, stim_btn).as_deref(), Some("The companion is already at full health."));
    assert_eq!(g.wheel_unavailable_reason(Command::UseStimpak).as_deref(), Some("The companion is already at full health."));

    // Case 2: Injured ED-E with Scrap Metal & NO Stimpaks -> enabled, NO unavailable reason.
    add_actor_value_damage(&mut g, ede, HEALTH, 40.0);
    let buttons = g.wheel_buttons(ede);
    let stim_btn = buttons.iter().find(|b| b.command == Command::UseStimpak).unwrap();
    assert!(stim_btn.enabled, "injured robot with scrap must have enabled heal button");
    assert_eq!(g.wheel_unavailable_reason_for(ede, stim_btn), None);
    assert_eq!(g.wheel_unavailable_reason(Command::UseStimpak), None);

    // Case 3: Remove Scrap Metal and add Stimpaks -> disabled, reason names Scrap Metal (not Stimpaks).
    g.world.remove_item(FormId::PLAYER_REF, scrap, 2).unwrap();
    g.world.add_item(FormId::PLAYER_REF, stim, 3).unwrap();
    let buttons = g.wheel_buttons(ede);
    let stim_btn = buttons.iter().find(|b| b.command == Command::UseStimpak).unwrap();
    assert!(!stim_btn.enabled, "robot without scrap must have disabled heal button even with stimpaks");
    assert_eq!(g.wheel_unavailable_reason_for(ede, stim_btn).as_deref(), Some("You have no scrap metal."));
    assert_eq!(g.wheel_unavailable_reason(Command::UseStimpak).as_deref(), Some("You have no scrap metal."));
}

#[test]
fn back_up_leaves_an_order_for_the_ai() {
    let f = fixture();
    let mut g = hired(&f);
    assert_eq!(g.wheel_command(Command::BackUp).unwrap(), Outcome::BackUp);
    assert!(g.take_back_up(companion()).is_some());
    assert!(g.take_back_up(companion()).is_none(), "taken once");
}

#[test]
fn a_passive_companion_fights_only_who_fights_the_players_side() {
    let f = fixture();
    let mut g = game(&f);
    let (c, other) = (companion(), FormId(OTHER_REF));
    g.extra.entry(c).or_default().teammate = true;
    // Kin by base form: never enemies on their own. A grudge against the player makes the
    // stranger the companion's enemy while it is aggressive.
    assert!(g.attacked_by(other, FormId::PLAYER_REF));
    assert!(g.would_attack(other, FormId::PLAYER_REF));
    g.store(fnv_game::game::InstKey::Ref { reference: c }, Some(FormId(SCRIPT)), AGGRESSIVE, fnv_script::vm::Value::Num(1.0), "test", false);
    assert!(!g.is_passive_teammate(c));
    assert!(g.would_attack(c, other));
    // Passive: not until the stranger fights the player.
    g.store(fnv_game::game::InstKey::Ref { reference: c }, Some(FormId(SCRIPT)), AGGRESSIVE, fnv_script::vm::Value::Num(0.0), "test", false);
    assert!(g.is_passive_teammate(c));
    assert!(!g.would_attack(c, other));
    g.start_combat(other, FormId::PLAYER_REF);
    assert!(g.fights_the_players_side(other));
    assert!(g.would_attack(c, other));
}

#[test]
fn a_following_teammate_sneaks_with_the_player_and_says_so() {
    let f = fixture();
    let mut g = game(&f);
    let c = companion();
    g.extra.entry(c).or_default().teammate = true;
    g.extra.entry(c).or_default().current_package = Some(FormId(FOLLOW));
    assert!(g.package_follows_player(c));
    assert!(!g.sneaks(c));
    g.senses.motion.insert(FormId::PLAYER_REF, Motion { sneaking: true, ..Motion::default() });
    assert!(g.sneaks(c));
    g.tick();
    assert!(g.speech.iter().any(|s| s.speaker == c && s.info == FormId(0xB20)), "FollowersStealthing said");
    // Told to wait (no Follow package): it does not.
    g.extra.entry(c).or_default().current_package = None;
    assert!(!g.sneaks(c));
}

/// Fallout 3's Wait, StopWaiting and IsWaiting (`functions::fo3`): only the actor's
/// current Follow package can be waited in.
#[test]
fn wait_and_stop_waiting_hold_a_follow_package() {
    let f = fixture();
    let mut g = game(&f);
    let c = companion();
    let call = |g: &mut Game, op: u16, args: &[fnv_script::vm::ArgValue]| {
        let ctx = fnv_game::functions::Ctx { unit: "test".into(), opcode: op, on: Some(c), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        g.call_function(&ctx, args).num()
    };
    let follow = [fnv_script::vm::ArgValue::Value(fnv_script::vm::Value::Ref(FOLLOW))];
    let (wait, stop, is_waiting) = (0x10B7, 0x10B8, 0x106F);
    // Not its current package: nothing happens.
    g.extra.entry(c).or_default().current_package = Some(FormId(QUEST));
    call(&mut g, wait, &follow);
    assert!(!g.waits(c));
    g.extra.entry(c).or_default().current_package = Some(FormId(FOLLOW));
    call(&mut g, wait, &follow);
    assert!(g.waits(c));
    assert_eq!(call(&mut g, is_waiting, &[]), 1.0);
    call(&mut g, stop, &follow);
    assert!(!g.waits(c));
    assert_eq!(call(&mut g, is_waiting, &[]), 0.0);
    // A wait ends with its package.
    call(&mut g, wait, &follow);
    g.extra.entry(c).or_default().current_package = None;
    assert!(!g.waits(c));
}

/// Map Selectable Companions: a teammate told to wait far away is summoned beside the
/// player; someone who is not a teammate is not.
#[test]
fn a_summoned_teammate_comes_to_the_players_side() {
    let f = fixture();
    let mut g = game(&f);
    // The companion far off in the player's cell.
    let cell = g.world.get(companion()).unwrap().cell;
    let player = g.world.get(FormId::PLAYER_REF).unwrap();
    player.cell = cell;
    let p = player.pos;
    g.world.get(companion()).unwrap().pos = [p[0] + 20000.0, p[1], p[2]];
    assert!(g.summon_teammates(&[companion()]).is_empty(), "not a teammate yet");
    g.extra.entry(companion()).or_default().teammate = true;
    let got = g.summon_teammates(&[companion()]);
    assert_eq!(got, vec![companion()]);
    let at = g.world.peek(companion()).unwrap().pos;
    assert!((at[0] - p[0]).hypot(at[1] - p[1]) < 400.0, "{at:?} beside {p:?}");
}

/// #3335: why an actor goes for the companion is named. A stranger with a grudge against
/// the player attacks the player's teammate as it would the player, and the reason says so
/// (the `alert:` log's `fights` line); with no grudge neither has a reason.
#[test]
fn the_reason_a_stranger_attacks_the_companion_names_the_player() {
    use fnv_game::awareness::Hostile;
    let f = fixture();
    let mut g = game(&f);
    let (c, other) = (companion(), FormId(OTHER_REF));
    g.extra.entry(c).or_default().teammate = true;
    assert_eq!(g.hostility_reason(other, c), None);
    assert_eq!(g.hostility_reason(c, other), None);
    assert!(g.hostility_text(other, c).is_none());
    assert!(g.attacked_by(other, FormId::PLAYER_REF));
    assert_eq!(g.hostility_reason(other, FormId::PLAYER_REF), Some(Hostile::Grudge));
    assert_eq!(g.hostility_reason(other, c), Some(Hostile::AsAgainstThePlayer));
    let text = g.hostility_text(other, c).unwrap();
    assert!(text.contains("is the player's teammate, and it would attack the player: it holds a grudge against"), "{text}");
    // The companion, aggressive, defends the player against it.
    g.store(fnv_game::game::InstKey::Ref { reference: c }, Some(FormId(SCRIPT)), AGGRESSIVE, fnv_script::vm::Value::Num(1.0), "test", false);
    assert_eq!(g.hostility_reason(c, other), Some(Hostile::DefendsThePlayer));
    assert_eq!(g.would_attack(c, other), g.hostility_reason(c, other).is_some());
}
