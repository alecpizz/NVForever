//! Joint progression in co-op (`fnv_game::coop_joint`; shared-progression coop-shared-progress.md §3): the
//! host's stage hook and presence, the guest's per-stage check, the claim ledger and its
//! replay, on a synthetic plugin built in memory (no game data).

use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::coop_joint::{check, ClaimLedger, JointGuest, StageOffer, TurnedDown};
use fnv_game::game::{Game, GameConfig};
use fnv_game::GameData;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};

fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
    Subrecord { sig: *sig, data, extended: false }
}

fn z(s: &str) -> Vec<u8> {
    [s.as_bytes(), &[0]].concat()
}

fn record(sig: &[u8; 4], flags: u32, fid: u32, subs: &[Subrecord]) -> Vec<u8> {
    let payload = write_subrecords(subs);
    [&sig[..], &(payload.len() as u32).to_le_bytes(), &flags.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload]
        .concat()
}

fn group(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
    [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body].concat()
}

fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
    group(*sig, 0, body)
}

fn schema() -> Schema {
    Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap()
}

fn catalogue() -> (FunctionTable, serde_json::Value) {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    (FunctionTable::load(&p).unwrap(), j)
}

const SIDE: u32 = 0x900;
const MAIN: u32 = 0x904;
/// Old World Blues' implant quest (the Auto-Doc's Implant GRX): each player's own.
const IMPLANT: u32 = 0x905;
const GUEST: u32 = 0xFF00_0801;

fn quest(fid: u32, edid: &str, stages: &[i16]) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"DATA", [vec![0u8, 0, 0, 0], 5.0f32.to_le_bytes().to_vec()].concat())];
    for s in stages {
        subs.push(sub(b"INDX", s.to_le_bytes().to_vec()));
        subs.push(sub(b"QSDT", vec![0]));
    }
    record(b"QUST", 0, fid, &subs)
}

fn plugin() -> Vec<u8> {
    let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x1000u32.to_le_bytes()].concat();
    let tes4 = record(b"TES4", 1, 0, &[sub(b"HEDR", hedr)]);
    // A side quest and a base-game main-story quest (its editor ID is on the list, and
    // this plugin is load-order index 0, as FalloutNV.esm is).
    [tes4, top(b"QUST", &[quest(SIDE, "VMS24", &[1, 10, 20, 50, 100]), quest(MAIN, "VMQ01", &[10, 20]), quest(IMPLANT, "NVDLC03GenericPerkQUEST", &[10, 11])].concat())].concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let (table, j) = catalogue();
    let bytes = plugin();
    let plugins = vec![Plugin::parse(&bytes, "Story.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = schema();
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

fn offer(stage: i32, prior: &[i32]) -> StageOffer {
    StageOffer { quest: FormId(SIDE), stage, prior: prior.to_vec(), line: None, to: vec![FormId(GUEST)] }
}

#[test]
fn the_host_offers_side_stages_to_guests_with_joint_on_and_keeps_presence() {
    let f = fixture();
    let mut host = game(&f);
    // Nobody joined: nothing offered.
    host.set_stage(FormId(SIDE), 1);
    assert!(host.take_joint_offers().is_empty());
    host.coop_joint_guests.insert(FormId(GUEST), JointGuest { character: 77, on: true, ..JointGuest::default() });
    host.set_stage(FormId(SIDE), 10);
    host.set_stage(FormId(MAIN), 10);
    host.set_stage(FormId(IMPLANT), 10);
    let offers = host.take_joint_offers();
    // The base-game main story is offered (docs/design/coop-quest-sync.md §2); a
    // player's own implant never is.
    assert_eq!(offers.len(), 2, "the side quest and the main story: {offers:?}");
    assert!(!host.coop_presence.stages.contains_key(&FormId(IMPLANT)));
    assert_eq!((offers[0].quest, offers[0].stage, offers[0].prior.clone(), offers[0].to.clone()), (FormId(SIDE), 10, vec![1], vec![FormId(GUEST)]));
    assert_eq!((offers[1].quest, offers[1].stage), (FormId(MAIN), 10));
    assert!(host.coop_presence.was_there(FormId(SIDE), 10, 77));
    assert!(!host.coop_presence.was_there(FormId(SIDE), 1, 77));
    assert!(host.coop_presence.was_there(FormId(MAIN), 10, 77));
    // A guest that sits out the main story's trunk: no offer and no presence for it; its
    // other quests still come (per-line filtering, the host's side).
    let main_line = fnv_game::coop_questlines::bit(fnv_game::coop_questlines::index("main").unwrap());
    assert_eq!(host.coop_questlines().lines_of(FormId(MAIN)), main_line, "VMQ01 is the trunk's root");
    host.coop_joint_guests.insert(FormId(GUEST), JointGuest { character: 77, on: true, sat_out: main_line });
    host.set_stage(FormId(MAIN), 20);
    host.set_stage(FormId(SIDE), 20);
    let offers = host.take_joint_offers();
    assert_eq!(offers.iter().map(|o| (o.quest, o.stage)).collect::<Vec<_>>(), vec![(FormId(SIDE), 20)], "{offers:?}");
    assert!(!host.coop_presence.was_there(FormId(MAIN), 20, 77));
    // Joint off: no offer, no presence.
    host.coop_joint_guests.insert(FormId(GUEST), JointGuest { character: 77, on: false, ..JointGuest::default() });
    host.set_stage(FormId(SIDE), 100);
    assert!(host.take_joint_offers().is_empty());
    assert!(!host.coop_presence.was_there(FormId(SIDE), 100, 77));
    // A sharing guest dropped: its stages are still offered, to
    // nobody here, so the engine keeps them for its return; nobody is marked present.
    host.coop_joint_guests.clear();
    host.coop_joint_away = true;
    host.set_stage(FormId(SIDE), 50);
    let offers = host.take_joint_offers();
    assert_eq!((offers.len(), offers[0].stage, offers[0].to.is_empty()), (1, 50, true), "{offers:?}");
    assert!(host.coop_presence.stages.get(&FormId(SIDE)).and_then(|s| s.get(&50)).is_none_or(|w| w.is_empty()));
    host.coop_joint_away = false;
    // Presence survives a save and load.
    let json = serde_json::to_string(&host.snapshot()).unwrap();
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&json).unwrap();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let again = Game::restore(&f.data, world, GameConfig { strict: true, ..GameConfig::default() }, &back).unwrap();
    assert!(again.coop_presence.was_there(FormId(SIDE), 10, 77));
    // The debug console's view (`coop.presence`).
    let lines = again.coop_presence_lines(FormId(SIDE));
    // Stages 10 and 20 had the guest there (20 in the sat-out check above).
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[1].contains("stage 10") && lines[1].contains("000000000000004d"), "{lines:?}");
    assert_eq!(again.coop_presence_lines(FormId(MAIN)).len(), 2, "one stage of the main story with the guest there");
}

#[test]
fn continuity_prerequisites_and_own_progress_decide_each_claim() {
    let own = std::collections::BTreeMap::new();
    let mut ledger = ClaimLedger::default();
    // First stage of a quest its own game never started: prerequisites decide.
    assert_eq!(check(&own, &ledger, &offer(1, &[]), || false), Err(TurnedDown::Prerequisite));
    assert_eq!(check(&own, &ledger, &offer(1, &[]), || true), Ok(()));
    ledger.stages.entry(FormId(SIDE)).or_default().push(fnv_game::coop_joint::ClaimedStage { stage: 1, line: None });
    // Later stages: continuity only (prerequisites are not asked again).
    assert_eq!(check(&own, &ledger, &offer(10, &[1]), || false), Ok(()));
    // A stage missed (joined late, dropped) stops the later ones.
    assert_eq!(check(&own, &ledger, &offer(50, &[1, 10, 20]), || true), Err(TurnedDown::Missed(10)));
    // Its own game did the missed stage: continuity holds through it.
    let mut own2 = std::collections::BTreeMap::new();
    let mut q = fnv_game::quest::QuestState { running: true, ..Default::default() };
    q.stages_done.extend([1, 10]);
    own2.insert(FormId(SIDE), q.clone());
    assert_eq!(check(&own2, &ClaimLedger::default(), &offer(20, &[1, 10]), || false), Ok(()));
    assert_eq!(check(&own2, &ClaimLedger::default(), &offer(10, &[1]), || true), Err(TurnedDown::AlreadyDone));
    // Its own game ended the quest: nothing more.
    q.completed = true;
    own2.insert(FormId(SIDE), q);
    assert_eq!(check(&own2, &ClaimLedger::default(), &offer(20, &[1, 10]), || true), Err(TurnedDown::QuestEnded));
}

#[test]
fn a_guest_claims_into_its_character_and_its_own_world_replays_once() {
    let f = fixture();
    let mut guest = game(&f);
    let id = guest.ensure_character_id();
    assert_ne!(id, 0);
    assert_eq!(guest.ensure_character_id(), id, "the ID is stable");
    let own = guest.quests.clone();
    // No claim open yet: a guest's infamy in the host's world stays there.
    const REP: FormId = FormId(0x0000_0900);
    guest.coop_note_reputation(&[(REP, 30.0)]);
    assert!(guest.player.claims.reputation.is_empty());
    for (s, prior) in [(1, vec![]), (10, vec![1]), (20, vec![1, 10])] {
        guest.take_joint_offer(&own, &offer(s, &prior)).unwrap();
    }
    assert_eq!(guest.take_joint_offer(&own, &offer(100, &[1, 10, 20, 50])), Err(TurnedDown::Missed(50)));
    // With a claim open, its own acts' infamy goes into the ledger (coop-shared-progress.md §3.6).
    guest.coop_note_reputation(&[(REP, 30.0)]);
    assert_eq!(guest.player.claims.reputation.get(&REP), Some(&[30.0, 0.0]));
    // The debug console's view (`coop.ledger`).
    let ledger = guest.coop_ledger_lines();
    assert!(ledger[0].contains("1 quest(s) waiting") && ledger[1].contains("1, 10, 20"), "{ledger:?}");
    // A map marker found in the host's world .
    guest.player.claims.markers.insert(FormId(0x0000_0A00));
    // The ledger travels with the character into its own save.
    let c = fnv_game::coop::Character::of(&guest.player);
    assert_eq!(c.character_id, id);
    let mut own_world = game(&f);
    c.put_on(&mut own_world.player);
    let xp = own_world.player.xp;
    assert!(own_world.player.claims.markers.contains(&FormId(0x0000_0A00)), "markers carry over for the engine's load");
    let lines = own_world.replay_claims();
    assert_eq!(lines.len(), 1, "{lines:?}");
    let q = own_world.quests.get(&FormId(SIDE)).unwrap();
    assert!([1, 10, 20].iter().all(|s| q.stages_done.contains(s)), "{:?}", q.stages_done);
    assert!(!q.stages_done.contains(&50));
    assert_eq!(own_world.player.xp, xp);
    assert_eq!(own_world.player.reputation.get(&REP).map(|v| v[0]), Some(30.0), "the infamy is replayed");
    assert!(own_world.player.claims.reputation.is_empty());
    assert!(!own_world.coop_replaying);
    // Replayed once: a second load replays nothing, and the claims stay known.
    assert!(own_world.replay_claims().is_empty());
    assert!(own_world.player.claims.claimed(FormId(SIDE), 20));
    // A replay never offers stages on (it is not the host's world).
    assert!(own_world.take_joint_offers().is_empty());
}

// A companion handed over by a claimed stage (module notes of `fnv_game::coop_joint`): the
// shape of Nothin' But a Hound Dog's hand-over, on synthetic records. The quest giver's
// line sets the stage and SayTos the dog; the giver's hand-over line marks the dog
// available and has the dog SayTo the player the followers' hire topic; the hire line,
// conditioned on the dog, hires it.
mod hand_over {
    use super::{group, record, schema, sub, top, z, catalogue};
    use crate::common::{a, ex, le16, set, st, tes4, unit, var};
    use fnv_esm::raw::Subrecord;
    use fnv_esm::{LoadOrder, Plugin};
    use fnv_game::coop_joint::{ClaimedStage, REPLAY_SAY_DEPTH};
    use fnv_game::game::{Game, GameConfig};
    use fnv_game::GameData;
    use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};

    const G_AVAIL: u32 = 0x810;
    const G_HIRED: u32 = 0x811;
    const G_COUNT: u32 = 0x812;
    const KING: u32 = 0x820;
    const DOG: u32 = 0x821;
    const KING_REF: u32 = 0x830;
    const DOG_REF: u32 = 0x831;
    const CELL: u32 = 0x832;
    const DOGQ: u32 = 0x840;
    const FOLLOWERS: u32 = 0x841;
    const T_KING: u32 = 0x850;
    const T_HAND: u32 = 0x851;
    const T_HIRED: u32 = 0x852;
    const T_LOOP: u32 = 0x853;
    const K_HAND: u32 = 0x860;
    const K_LOOP: u32 = 0x861;
    const HAND: u32 = 0x862;
    const HIRE_KING: u32 = 0x863;
    const HIRE_DOG: u32 = 0x864;
    const LOOP: u32 = 0x865;
    const SET_STAGE: u16 = 0x1039;
    const SAY_TO: u16 = 0x1034;
    const SET_PLAYER_TEAMMATE: u16 = 0x11C5;
    /// Condition function indices (opcode - 0x1000).
    const GET_IS_ID: u16 = 0x48;
    const GET_GLOBAL_VALUE: u16 = 0x4A;

    fn u32b(v: u32) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }
    fn r(i: u16) -> Vec<u8> {
        [vec![b'r'], le16(i)].concat()
    }
    fn n(v: i32) -> Vec<u8> {
        [vec![b'n'], v.to_le_bytes().to_vec()].concat()
    }
    fn call(op: u16, params: &[Vec<u8>]) -> Vec<u8> {
        st(op, &[le16(params.len() as u16), params.concat()].concat())
    }
    /// `<SCRO i>.` before a call.
    fn on(i: u16) -> Vec<u8> {
        [le16(0x1C), le16(i)].concat()
    }
    /// A CTDA: `function(p1) == value`, run on the subject.
    fn ctda(function: u16, p1: u32, value: f32) -> Subrecord {
        let mut d = vec![0u8, 0, 0, 0];
        d.extend(value.to_le_bytes());
        d.extend(function.to_le_bytes());
        d.extend([0, 0]);
        d.extend(p1.to_le_bytes());
        d.extend([0u8; 12]);
        sub(b"CTDA", d)
    }
    fn info(id: u32, quest: u32, speaker: Option<u32>, conds: Vec<Subrecord>, end: Vec<Subrecord>) -> Vec<u8> {
        let trdt = [u32b(0), u32b(50), vec![0; 4], vec![1, 0, 0, 0], u32b(0), vec![0; 4]].concat();
        let mut s = vec![sub(b"EDID", z(&format!("Line{id:X}"))), sub(b"DATA", vec![0, 0, 0, 0]), sub(b"QSTI", u32b(quest))];
        s.extend([sub(b"TRDT", trdt), sub(b"NAM1", z("...")), sub(b"NAM2", z("")), sub(b"NAM3", z(""))]);
        s.extend(conds);
        s.extend(unit(0, &[], &[], &[]));
        s.push(sub(b"NEXT", vec![]));
        s.extend(end);
        s.extend(speaker.map(|a| sub(b"ANAM", u32b(a))));
        record(b"INFO", 0, id, &s)
    }
    fn topic(id: u32, edid: &str, lines: &[Vec<u8>]) -> Vec<u8> {
        let dial = record(b"DIAL", 0, id, &[sub(b"EDID", z(edid)), sub(b"FULL", z(edid)), sub(b"PNAM", 50.0f32.to_le_bytes().to_vec()), sub(b"DATA", vec![0, 0])]);
        [dial, group(id.to_le_bytes(), 7, &lines.concat())].concat()
    }
    fn quest(id: u32, edid: &str, start: bool, stages: &[i16]) -> Vec<u8> {
        let mut subs = vec![sub(b"EDID", z(edid)), sub(b"DATA", [vec![start as u8, 50, 0, 0], 0.0f32.to_le_bytes().to_vec()].concat())];
        for s in stages {
            subs.push(sub(b"INDX", s.to_le_bytes().to_vec()));
            subs.push(sub(b"QSDT", vec![0]));
        }
        record(b"QUST", 0, id, &subs)
    }
    fn glob(id: u32, edid: &str) -> Vec<u8> {
        record(b"GLOB", 0, id, &[sub(b"EDID", z(edid)), sub(b"FNAM", vec![b'f']), sub(b"FLTV", 0.0f32.to_le_bytes().to_vec())])
    }

    fn plugin() -> Vec<u8> {
        let player = FormId::PLAYER_REF.0;
        // The giver: `SetStage DogQuest 10`, `KingREF.SayTo DogREF TestHandOver`.
        let k_hand = unit(0, &[call(SET_STAGE, &[r(1), n(10)]), on(2), call(SAY_TO, &[r(3), r(4)])].concat(), &[], &[DOGQ, KING_REF, DOG_REF, T_HAND]);
        // The giver's hand-over: `set TestDogAvailable to 1`, `DogREF.SayTo player TestHired`.
        let hand = unit(0, &[set(var(b'G', 1), ex(&[a("1")])), on(2), call(SAY_TO, &[r(3), r(4)])].concat(), &[], &[G_AVAIL, DOG_REF, player, T_HIRED]);
        // The hire: `set TestDogHired to 1`, `DogREF.SetPlayerTeammate 1`.
        let hire = unit(0, &[set(var(b'G', 1), ex(&[a("1")])), on(2), call(SET_PLAYER_TEAMMATE, &[n(1)])].concat(), &[], &[G_HIRED, DOG_REF]);
        // Another follower's hire line in the same topic: not the dog's.
        let hire_king = unit(0, &set(var(b'G', 1), ex(&[a("5")])), &[], &[G_HIRED]);
        // A line that SayTos its own topic again: `SetStage DogQuest 20`, `KingREF.SayTo
        // player TestLoop`; and TestLoop's `set TestCount to TestCount + 1`, the same SayTo.
        let k_loop = unit(0, &[call(SET_STAGE, &[r(1), n(20)]), on(2), call(SAY_TO, &[r(3), r(4)])].concat(), &[], &[DOGQ, KING_REF, player, T_LOOP]);
        let lp = unit(0, &[set(var(b'G', 1), ex(&[var(b'G', 1), a("1"), a("+")])), on(2), call(SAY_TO, &[r(3), r(4)])].concat(), &[], &[G_COUNT, KING_REF, player, T_LOOP]);
        let available = || vec![ctda(GET_GLOBAL_VALUE, G_AVAIL, 1.0), ctda(GET_GLOBAL_VALUE, G_HIRED, 0.0)];
        let dial = [
            topic(T_KING, "TestKingTalk", &[info(K_HAND, DOGQ, Some(KING), vec![], k_hand), info(K_LOOP, DOGQ, Some(KING), vec![], k_loop)]),
            topic(T_HAND, "TestHandOver", &[info(HAND, DOGQ, Some(KING), vec![], hand)]),
            topic(
                T_HIRED,
                "TestHired",
                &[
                    info(HIRE_KING, FOLLOWERS, None, [vec![ctda(GET_IS_ID, KING, 1.0)], available()].concat(), hire_king),
                    info(HIRE_DOG, FOLLOWERS, None, [vec![ctda(GET_IS_ID, DOG, 1.0)], available()].concat(), hire),
                ],
            ),
            topic(T_LOOP, "TestLoop", &[info(LOOP, FOLLOWERS, Some(KING), vec![], lp)]),
        ]
        .concat();
        let npcs = [record(b"NPC_", 0, KING, &[sub(b"EDID", z("TestKing"))]), record(b"NPC_", 0, DOG, &[sub(b"EDID", z("TestDog"))])].concat();
        let placed = |id: u32, base: u32, edid: &str| record(b"ACHR", 0, id, &[sub(b"EDID", z(edid)), sub(b"NAME", u32b(base)), sub(b"DATA", vec![0; 24])]);
        let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("KingCell"))]);
        let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &[placed(KING_REF, KING, "KingREF"), placed(DOG_REF, DOG, "DogREF")].concat()));
        [
            tes4(),
            top(b"GLOB", &[glob(G_AVAIL, "TestDogAvailable"), glob(G_HIRED, "TestDogHired"), glob(G_COUNT, "TestCount")].concat()),
            top(b"NPC_", &npcs),
            top(b"QUST", &[quest(DOGQ, "TestDogQuest", false, &[1, 10, 20]), quest(FOLLOWERS, "TestFollowers", true, &[])].concat()),
            top(b"DIAL", &dial),
            top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat()))),
        ]
        .concat()
    }

    /// The guest claimed stages 10 and 20 through the giver's lines; its own world's
    /// replay follows the SayTos: the dog is hired (its line, not the other follower's),
    /// and the line that SayTos itself stops at the bound.
    #[test]
    fn a_replayed_stage_hires_the_companion_its_lines_hand_over() {
        let (table, j) = catalogue();
        let bytes = plugin();
        let plugins = vec![Plugin::parse(&bytes, "HandOver.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let schema = schema();
        let forms = FormStore::build(&lo, &schema);
        let refs = RefTable::build(&lo, &schema);
        let data = GameData::build(&lo, &schema, table, &j, &refs);
        let world = World::new(&forms, &refs, SimConfig::default());
        let mut own = Game::new(&data, world, GameConfig { strict: true, ..GameConfig::default() });
        own.new_game();
        assert!(!own.is_teammate(FormId(DOG_REF)));
        let stages = own.player.claims.stages.entry(FormId(DOGQ)).or_default();
        stages.push(ClaimedStage { stage: 10, line: Some((FormId(K_HAND), true)) });
        stages.push(ClaimedStage { stage: 20, line: Some((FormId(K_LOOP), true)) });
        let lines = own.replay_claims();
        assert_eq!(lines.len(), 1, "{lines:?}");
        let q = own.quests.get(&FormId(DOGQ)).unwrap();
        assert!(q.stages_done.contains(&10) && q.stages_done.contains(&20), "{:?}", q.stages_done);
        assert_eq!(own.world.global(FormId(G_AVAIL)).unwrap(), 1.0f32, "the hand-over line ran");
        assert_eq!(own.world.global(FormId(G_HIRED)).unwrap(), 1.0, "the dog's hire line, not the other follower's");
        assert!(own.is_teammate(FormId(DOG_REF)), "hired");
        assert_eq!(own.world.global(FormId(G_COUNT)).unwrap(), f32::from(REPLAY_SAY_DEPTH), "the self-repeating line stops at the bound: once per level");
        assert!(!own.coop_replaying && own.coop_replay_depth == 0);
    }
}

/// A guest's quest progression sync (docs/design/coop-quest-sync.md): the guest's filter
/// follows its choice per line (toggle off takes nothing), a claimed quest part way through
/// is what the warning names, and the choice goes with the character into its own save,
/// back through a load, and on to another host.
#[test]
fn a_guests_quest_sync_choice_filters_per_line_and_saves_with_its_character() {
    use fnv_game::coop_questlines::{bit, index};
    let f = fixture();
    let own_world = game(&f);
    let own_snap = own_world.snapshot();
    let own = own_world.quests.clone();
    // The guest's game in the host's world.
    let mut guest = game(&f);
    let main = index("main").unwrap();
    // Never chosen: what came before the toggle (rewards) still comes.
    assert!(guest.coop_takes_quest(Some(FormId(MAIN))));
    assert!(guest.coop_takes_quest(None));
    // On, the trunk sat out: its quests are filtered, another line's are not.
    guest.player.quest_sync.on = Some(true);
    guest.player.quest_sync.set_sat_out(main, true);
    assert!(!guest.coop_takes_quest(Some(FormId(MAIN))), "VMQ01 is the trunk's");
    assert!(guest.coop_takes_quest(Some(FormId(SIDE))), "VMS24 is another line's");
    assert!(guest.coop_takes_quest(None), "script XP names no quest");
    // Off: nothing at all.
    guest.player.quest_sync.on = Some(false);
    assert!(!guest.coop_takes_quest(Some(FormId(SIDE))));
    assert!(!guest.coop_takes_quest(None));
    // The warning: nothing claimed, nothing part way through; a claimed stage of the side
    // quest, ended nowhere, is; not within the trunk's line; ended by the host, it is not.
    assert!(guest.coop_sync_in_progress(&own, 0).is_empty());
    guest.player.claims.stages.entry(FormId(SIDE)).or_default().push(fnv_game::coop_joint::ClaimedStage { stage: 10, line: None });
    let busy = guest.coop_sync_in_progress(&own, 0);
    assert_eq!(busy.len(), 1, "{busy:?}");
    assert!(!busy[0].trim().is_empty());
    assert!(guest.coop_sync_in_progress(&own, bit(main)).is_empty());
    guest.coop_host_ended.push((FormId(SIDE), false));
    assert!(guest.coop_sync_in_progress(&own, 0).is_empty());
    // The guest's save: its own game with this character, through JSON and a load.
    let saved = own_snap.with_character_of(&guest.snapshot());
    let json = serde_json::to_string(&saved).unwrap();
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&json).unwrap();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let again = Game::restore(&f.data, world, GameConfig { strict: true, ..GameConfig::default() }, &back).unwrap();
    assert_eq!(again.player.quest_sync, guest.player.quest_sync);
    assert_eq!((again.player.quest_sync.on, again.player.quest_sync.mask()), (Some(false), bit(main)));
    // On to another host: the character carries it.
    let mut other = game(&f);
    fnv_game::coop::Character::of(&again.player).put_on(&mut other.player);
    assert_eq!(other.player.quest_sync, guest.player.quest_sync);
}
