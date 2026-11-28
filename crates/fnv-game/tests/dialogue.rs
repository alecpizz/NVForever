//! Conversations (docs/spec-notes/dialogue.md §4–§6) on a synthetic plugin built in
//! memory, one small test per rule; no game data. Real-data checks: `dialogue_data.rs`.

use crate::common;

use common::*;
use fnv_esm::raw::Subrecord;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::data::{info_flag, topic_type};
use fnv_game::functions::Ctx;
use fnv_game::game::{Game, GameConfig, GameSnapshot};
use fnv_game::trace::EventKind;
use fnv_game::GameData;
use fnv_script::vm::{ArgValue, Value};
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::{json, Value as Json};

const G_OUT: u32 = 0x800;
const G_GATE: u32 = 0x801;
const G_DAYS: u32 = 0x803;
const G_TICKS: u32 = 0x804;
const G_MENU_TICKS: u32 = 0x805;
const G_COUNT: u32 = 0x809;
const QTICK: u32 = 0x904;
const QTICK_SCRIPT: u32 = 0x905;
const T_POOL: u32 = 0x939;
const P1: u32 = 0xF31;
const P2: u32 = 0xF32;
const P_PLAIN: u32 = 0xF33;
const P3: u32 = 0xF34;
const P4: u32 = 0xF35;
const QA: u32 = 0x900;
const QHI: u32 = 0x901;
const QOFF: u32 = 0x902;
const QEQ: u32 = 0x903;
const NPC1: u32 = 0x920;
const NPC2: u32 = 0x921;
const AVIF: u32 = 0x940;
const GREETING: u32 = 0x930;
const T_ASK: u32 = 0x931;
const T_BYE: u32 = 0x932;
const T_HIDDEN: u32 = 0x933;
const T_TOP: u32 = 0x934;
const T_TOP2: u32 = 0x935;
const T_ADDED: u32 = 0x936;
const T_SKILL: u32 = 0x937;
const GOODBYE: u32 = 0x938;
const T_ARGUE: u32 = 0x93A;
const T_REPLY: u32 = 0x93B;
const T_REPLY_NOT: u32 = 0x93C;
const T_LAST: u32 = 0x93D;
const K_OPEN: u32 = 0xF41;
const K_REPLY: u32 = 0xF42;
const K_NOT: u32 = 0xF43;
const K_LAST: u32 = 0xF44;
const HELLO: u32 = 0x93E;
const K_HELLO_LINKED: u32 = 0xF45;
const K_HELLO: u32 = 0xF46;
const K_HELLO_PLAYER: u32 = 0xF48;
const K_REPLY_TOPIC: u32 = 0xF49;
const CONV: u8 = topic_type::CONVERSATION;
/// #2548: a topic whose lines name Chatter as fixed speaker; one also tests the
/// TestCrew faction on the subject (Talker is in it), the other nothing.
const T_GROUP: u32 = 0x9F1;
const GR_CREW: u32 = 0xF6E;
const GR_FIXED: u32 = 0xF6F;
const CREW: u32 = 0x9F0;
const GET_IN_FACTION: u16 = 0x47;
const T_SOURCE: u32 = 0x950;
const T_SHARED: u32 = 0x951;
const T_LINKS: u32 = 0x952;
const T_DEAD_END: u32 = 0x953;
const S_LINE: u32 = 0xF51;
const S_LINKS: u32 = 0xF52;
const S_DEAD_END: u32 = 0xF53;
const T_LF_FROM: u32 = 0x954;
const T_LF_PLAIN: u32 = 0x955;
const T_LF_TO: u32 = 0x956;
const S_LF_FROM: u32 = 0xF54;
const S_LF_PLAIN: u32 = 0xF55;
const S_LF_TO: u32 = 0xF56;
const T_CHAT: u32 = 0x957;
const T_FAIL: u32 = 0x9E0;
const S_FAIL: u32 = 0xFE0;
const T_ANSWER: u32 = 0x958;
const C_CHAT: u32 = 0xF57;
const C_OPENER: u32 = 0xF58;
const C_REPLY: u32 = 0xF59;
// Ping-pong between actors: in QA (no repeated topics) and in QREP (repeats allowed).
const QREP: u32 = 0x90A;
const T_PING: u32 = 0x959;
const T_PONG: u32 = 0x95A;
const T_RPING: u32 = 0x95B;
const T_RPONG: u32 = 0x95C;
const P_PING: u32 = 0xF5A;
const P_PONG: u32 = 0xF5B;
const P_RPING: u32 = 0xF5C;
const P_RPONG: u32 = 0xF5D;
// Between actors: a line links to a topic whose line has a follow-up stored under the first
// topic, and the follow-up links back. In QA (no repeated topics) and in QREP (repeats allowed).
const T_HIST_A: u32 = 0x960;
const T_HIST_B: u32 = 0x961;
const T_RHIST_A: u32 = 0x962;
const T_RHIST_B: u32 = 0x963;
const H_A: u32 = 0xF62;
const H_FOLLOW: u32 = 0xF63;
const H_B: u32 = 0xF64;
const H_RA: u32 = 0xF65;
const H_RFOLLOW: u32 = 0xF66;
const H_RB: u32 = 0xF67;
// Follow-up lines stored under a topic of their own (dialogue.md §3.4).
const T_FU_LEAD: u32 = 0x95D;
const T_FU: u32 = 0x95E;
const T_FU_ONLY: u32 = 0x95F;
const U_LEAD: u32 = 0xF5E;
const U_TARGET: u32 = 0xF5F;
const U_PLAIN: u32 = 0xF60;
const U_ONLY: u32 = 0xF61;
// A SayTo topic whose first Conversation line has the next speaker Self: the speaker goes
// on with the next line.
const T_SELF: u32 = 0x964;
const V_SELF: u32 = 0xF70;
const V_BYE: u32 = 0xF71;
const V_AFTER: u32 = 0xF72;
const A_OFF: u32 = 0xA01;
const A_FIRST: u32 = 0xA02;
const A_HIGH: u32 = 0xA03;
const A_LATER: u32 = 0xA04;
const A_EQUAL: u32 = 0xA05;
const R1: u32 = 0xA06;
const R2: u32 = 0xA07;
const R_AFTER: u32 = 0xA08;
const A_BARK: u32 = 0xA09;
const B_ASK: u32 = 0xB01;
const B_FOLLOW: u32 = 0xB02;
const C_BYE: u32 = 0xC01;
const D_HIDDEN: u32 = 0xD01;
const E_DAY: u32 = 0xE01;
const E_NOW: u32 = 0xE02;
const F_ADDED: u32 = 0xF01;
const H_PASS: u32 = 0xF11;
const H_FAIL: u32 = 0xF12;
const I_BYE: u32 = 0xF21;
const CELL: u32 = 0x200;
/// A Voice note (holotape) whose topic's one line, fixed to NPC1, has begin and end
/// result scripts.
const NOTE_TAPE: u32 = 0xA60;
const T_TAPE: u32 = 0xA61;
const S_TAPE: u32 = 0xA62;
// Issue #1107: lines whose end script has ChatterREF force-greet the player.
const T_HANDOFF: u32 = 0xA63;
const T_HANDOFF_STAY: u32 = 0xA64;
const H_HANDOFF: u32 = 0xA65;
const H_HANDOFF_STAY: u32 = 0xA66;
// Issue #964: lines with both follow-ups and choices.
const T_BOTH: u32 = 0xA67;
const T_BOTH_FU: u32 = 0xA68;
const W_BOTH: u32 = 0xA69;
const W_BOTH_FU: u32 = 0xA6A;
const W_BOTH_BYE: u32 = 0xA6B;
const T_KEEP: u32 = 0xA6C;
const W_KEEP: u32 = 0xA6D;
// Issue #1606: a Goodbye line whose end script hands off to ChatterREF and makes it say
// TestByeB (ChatterREF's Goodbye line, which counts its end in TestCount); a Goodbye line
// whose begin script makes its speaker say another line.
const T_RELAY: u32 = 0xA6E;
const H_RELAY: u32 = 0xA6F;
const T_BYE_B: u32 = 0xA70;
const B_BYE: u32 = 0xA71;
const T_BEGIN_SAY: u32 = 0xA72;
const H_BEGIN_SAY: u32 = 0xA73;
// ChatterREF's Goodbye line whose end script has TalkerREF force-greet the player, with
// TopicSource in its Add Topics.
const T_B_HAND: u32 = 0xA74;
const B_HAND: u32 = 0xA75;
const PATROL_REF: u32 = 0x322;
const NPC1_REF: u32 = 0x320;
const NPC2_REF: u32 = 0x321;

const GET_GLOBAL_VALUE: u16 = 0x4A;
const GET_ACTOR_VALUE: u16 = 0x0E;
const SPEECH: u32 = 43;
const EQ: u8 = 0x00;
const NE: u8 = 0x20;
const GE: u8 = 0x60;
const LT: u8 = 0x80;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn u32b(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

/// A CTDA: comparison, value, function index, first parameter, run on (0 subject,
/// 1 target).
fn ctda(cmp: u8, value: f32, function: u16, p1: u32, run_on: u32) -> Subrecord {
    let mut d = vec![cmp, 0, 0, 0];
    d.extend(value.to_le_bytes());
    d.extend(function.to_le_bytes());
    d.extend([0, 0]);
    d.extend(p1.to_le_bytes());
    d.extend(0u32.to_le_bytes());
    d.extend(run_on.to_le_bytes());
    d.extend(0u32.to_le_bytes());
    sub(b"CTDA", d)
}

fn gate(cmp: u8, value: f32) -> Subrecord {
    ctda(cmp, value, GET_GLOBAL_VALUE, G_GATE, 0)
}

/// A dialogue line as the editor writes it.
#[derive(Default)]
struct L {
    id: u32,
    quest: u32,
    flags: u16,
    /// DATA type (`topic_type`; 0 Topic, the default).
    ty: u8,
    /// DATA next speaker (0 Target, 1 Self, 2 Either).
    next: u8,
    speaker: Option<u32>,
    conds: Vec<Subrecord>,
    /// (response number, text, emotion).
    responses: Vec<(u8, &'static str, u32)>,
    prompt: Option<&'static str>,
    choices: Vec<u32>,
    /// TCLF ("Link From").
    link_from: Vec<u32>,
    follow: Vec<u32>,
    add: Vec<u32>,
    /// Result scripts: the value TestOut is set to.
    begin: Option<&'static str>,
    end: Option<&'static str>,
    /// The end script is `ChatterREF.StartConversation player` instead.
    end_talk: bool,
    /// Result scripts as given (instead of `begin` / `end`).
    begin_raw: Option<Vec<Subrecord>>,
    end_raw: Option<Vec<Subrecord>>,
    knam: Option<u32>,
}

/// `ChatterREF.StartConversation player` (SCRO 1 ChatterREF, 2 the player).
fn talk_result() -> Vec<Subrecord> {
    talk_result_by(NPC2_REF)
}

/// `<speaker>.StartConversation player` (SCRO 1 the speaker, 2 the player).
fn talk_result_by(speaker: u32) -> Vec<Subrecord> {
    let call = [le16(0x1C), le16(1), st(0x1056, &[le16(1), vec![b'r'], le16(2)].concat())].concat();
    unit(0, &call, &[], &[speaker, FormId::PLAYER_REF.0])
}

/// `<speaker>.Say <topic>` (SCRO 1 the speaker, 2 the topic).
fn say_result(speaker: u32, topic: u32) -> Vec<Subrecord> {
    let call = [le16(0x1C), le16(1), st(0x1033, &[le16(1), vec![b'r'], le16(2)].concat())].concat();
    unit(0, &call, &[], &[speaker, topic])
}

/// `ChatterREF.StartConversation player`, `ChatterREF.Say TestByeB`, `set TestOut to 5`:
/// the hand-off first, then a line that ends ChatterREF's farewell, then one more
/// statement (SCRO 1 ChatterREF, 2 the player, 3 TestByeB, 4 TestOut).
fn relay_result() -> Vec<Subrecord> {
    let talk = [le16(0x1C), le16(1), st(0x1056, &[le16(1), vec![b'r'], le16(2)].concat())].concat();
    let say = [le16(0x1C), le16(1), st(0x1033, &[le16(1), vec![b'r'], le16(3)].concat())].concat();
    let out = set(var(b'G', 4), ex(&[a("5")]));
    unit(0, &[talk, say, out].concat(), &[], &[NPC2_REF, FormId::PLAYER_REF.0, T_BYE_B, G_OUT])
}

/// `set TestCount to TestCount + 1`.
fn count_result() -> Vec<Subrecord> {
    unit(0, &set(var(b'G', 1), ex(&[var(b'G', 1), a("1"), a("+")])), &[], &[G_COUNT])
}

fn result(value: Option<&str>) -> Vec<Subrecord> {
    match value {
        Some(v) => unit(0, &set(var(b'G', 1), ex(&[a(v)])), &[], &[G_OUT]),
        None => unit(0, &[], &[], &[]),
    }
}

fn info(l: L) -> Vec<u8> {
    // `LineB01` and so on, for scripts (the dialogue edits' tests).
    let mut s = vec![sub(b"EDID", z(&format!("Line{:X}", l.id))), sub(b"DATA", vec![l.ty, l.next, l.flags as u8, (l.flags >> 8) as u8]), sub(b"QSTI", u32b(l.quest))];
    s.extend(l.add.iter().map(|t| sub(b"NAME", u32b(*t))));
    for (number, text, emotion) in &l.responses {
        let trdt = [u32b(*emotion), u32b(50), vec![0; 4], vec![*number, 0, 0, 0], u32b(0), vec![0; 4]].concat();
        s.extend([sub(b"TRDT", trdt), sub(b"NAM1", z(text)), sub(b"NAM2", z("")), sub(b"NAM3", z(""))]);
    }
    s.extend(l.conds);
    s.extend(l.choices.iter().map(|t| sub(b"TCLT", u32b(*t))));
    s.extend(l.link_from.iter().map(|t| sub(b"TCLF", u32b(*t))));
    s.extend(l.follow.iter().map(|t| sub(b"TCFU", u32b(*t))));
    s.extend(l.begin_raw.unwrap_or_else(|| result(l.begin)));
    s.push(sub(b"NEXT", vec![]));
    s.extend(match l.end_raw {
        Some(raw) => raw,
        None if l.end_talk => talk_result(),
        None => result(l.end),
    });
    s.extend(l.prompt.map(|p| sub(b"RNAM", z(p))));
    s.extend(l.speaker.map(|a| sub(b"ANAM", u32b(a))));
    s.extend(l.knam.map(|k| sub(b"KNAM", u32b(k))));
    record(b"INFO", 0, l.id, &s)
}

fn topic(id: u32, edid: &str, name: &str, ty: u8, flags: u8, priority: f32, lines: Vec<L>) -> Vec<u8> {
    topic_sharing(id, edid, name, ty, flags, priority, &[], lines)
}

/// A topic that also lists lines stored under other topics: (quest, line, Info Index)
/// as QSTI, INFC, INFX ("Shared Infos").
#[allow(clippy::too_many_arguments)]
fn topic_sharing(id: u32, edid: &str, name: &str, ty: u8, flags: u8, priority: f32, shared: &[(u32, u32, i32)], lines: Vec<L>) -> Vec<u8> {
    let mut s = vec![sub(b"EDID", z(edid))];
    let mut previous_quest = None;
    for &(quest, line, index) in shared {
        if previous_quest != Some(quest) {
            s.push(sub(b"QSTI", u32b(quest)));
            previous_quest = Some(quest);
        }
        s.extend([sub(b"INFC", u32b(line)), sub(b"INFX", index.to_le_bytes().to_vec())]);
    }
    s.extend([sub(b"FULL", z(name)), sub(b"PNAM", f32b(priority)), sub(b"DATA", vec![ty, flags])]);
    let dial = record(b"DIAL", 0, id, &s);
    let children: Vec<u8> = lines.into_iter().flat_map(info).collect();
    [dial, group(id.to_le_bytes(), 7, &children)].concat()
}

fn quest(id: u32, edid: &str, start: bool, priority: u8) -> Vec<u8> {
    record(b"QUST", 0, id, &[sub(b"EDID", z(edid)), sub(b"DATA", [vec![start as u8, priority, 0, 0], f32b(0.0)].concat())])
}

fn glob(id: u32, edid: &str, v: f32) -> Vec<u8> {
    record(b"GLOB", 0, id, &[sub(b"EDID", z(edid)), sub(b"FNAM", vec![b'f']), sub(b"FLTV", f32b(v))])
}

fn plugin_bytes() -> Vec<u8> {
    let globs = [
        glob(G_OUT, "TestOut", 0.0),
        glob(G_GATE, "TestGate", 0.0),
        glob(0x802, "GameHour", 8.0),
        glob(G_DAYS, "GameDaysPassed", 0.0),
        glob(0x806, "GameYear", 2281.0),
        glob(0x807, "GameMonth", 9.0),
        glob(0x808, "GameDay", 18.0),
        glob(G_TICKS, "TestTicks", 0.0),
        glob(G_MENU_TICKS, "TestMenuTicks", 0.0),
        glob(G_COUNT, "TestCount", 0.0),
    ]
    .concat();
    // A quest whose script counts its GameMode runs in TestTicks and its MenuMode runs in
    // TestMenuTicks, every frame.
    let count = |g: u16| set(var(b'G', g), ex(&[var(b'G', g), a("1"), a("+")]));
    let tick_script = record(
        b"SCPT",
        0,
        QTICK_SCRIPT,
        &[vec![sub(b"EDID", z("TickScript"))], unit(1, &script(&[(0, None, count(1)), (1, None, count(2))]), &[], &[G_TICKS, G_MENU_TICKS])].concat(),
    );
    let tick_quest = record(
        b"QUST",
        0,
        QTICK,
        &[sub(b"EDID", z("QTick")), sub(b"SCRI", u32b(QTICK_SCRIPT)), sub(b"DATA", [vec![1u8, 0, 0, 0], f32b(0.01)].concat())],
    );
    // Start Game Enabled and Allow Repeated Conversation Topics.
    let repeat_quest = record(b"QUST", 0, QREP, &[sub(b"EDID", z("QRepeat")), sub(b"DATA", [vec![0x05, 50, 0, 0], f32b(0.0)].concat())]);
    let quests =
        [quest(QA, "QA", true, 50), quest(QHI, "QHi", true, 80), quest(QOFF, "QOff", false, 90), quest(QEQ, "QEq", true, 50), repeat_quest, tick_quest].concat();
    let npcs = [
        record(b"NPC_", 0, NPC1, &[sub(b"EDID", z("Talker")), sub(b"SNAM", [&CREW.to_le_bytes()[..], &[0, 0, 0, 0]].concat())]),
        record(b"NPC_", 0, NPC2, &[sub(b"EDID", z("Chatter"))]),
    ]
    .concat();
    let avif = record(b"AVIF", 0, AVIF, &[sub(b"EDID", z("AVSpeech"))]);
    let one = |text: &'static str| vec![(1u8, text, 0u32)];
    let me = Some(NPC1);
    let top_level = 2u8;
    let dial = [
        topic(
            GREETING,
            "GREETING",
            "GREETING",
            topic_type::TOPIC,
            0,
            90.0,
            vec![
                // A stopped quest's line, of the highest priority.
                L { id: A_OFF, quest: QOFF, speaker: me, responses: one("Never"), ..L::default() },
                // The first greeting: Say Once, responses stored out of number order,
                // choices, both result scripts.
                L {
                    id: A_FIRST,
                    quest: QA,
                    flags: info_flag::SAY_ONCE,
                    speaker: me,
                    conds: vec![gate(NE, 9.0)],
                    responses: vec![(2, "Second.", 5), (1, "First", 3)],
                    choices: vec![T_ASK, T_BYE, T_HIDDEN],
                    begin: Some("1"),
                    end: Some("2"),
                    ..L::default()
                },
                // A higher-priority quest's line, stored later.
                L { id: A_HIGH, quest: QHI, speaker: me, conds: vec![gate(EQ, 5.0)], responses: one("Urgent"), ..L::default() },
                L { id: A_LATER, quest: QA, speaker: me, responses: one("Again"), ..L::default() },
                // A quest of the same priority as QA, stored after it.
                L { id: A_EQUAL, quest: QEQ, speaker: me, responses: one("Equal"), ..L::default() },
                // The second actor: two Random lines, the stack closed by Random End.
                L { id: R1, quest: QA, flags: info_flag::RANDOM, speaker: Some(NPC2), responses: one("One"), ..L::default() },
                L { id: R2, quest: QA, flags: info_flag::RANDOM | info_flag::RANDOM_END, speaker: Some(NPC2), responses: one("Two"), ..L::default() },
                L { id: R_AFTER, quest: QA, speaker: Some(NPC2), responses: one("After"), ..L::default() },
                // A greeting with the Goodbye flag: said at the player, no conversation.
                L {
                    id: A_BARK,
                    quest: QHI,
                    flags: info_flag::GOODBYE,
                    speaker: me,
                    conds: vec![gate(EQ, 7.0)],
                    responses: one("Move along"),
                    add: vec![T_ADDED],
                    begin: Some("3"),
                    end: Some("4"),
                    ..L::default()
                },
            ],
        ),
        topic(
            T_ASK,
            "TopicAsk",
            "Ask",
            topic_type::TOPIC,
            0,
            50.0,
            vec![
                L { id: B_ASK, quest: QA, speaker: me, responses: one("Asked"), prompt: Some("Tell me"), follow: vec![B_FOLLOW], add: vec![T_ADDED], ..L::default() },
                L { id: B_FOLLOW, quest: QA, speaker: me, responses: one("And more"), end: Some("7"), ..L::default() },
            ],
        ),
        // A line whose follow-ups are stored under other topics: T_FU's first line is
        // one of them, its second is not; T_FU_ONLY has only a follow-up line.
        topic(
            T_FU_LEAD,
            "TopicFollowLead",
            "Lead",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: U_LEAD, quest: QA, speaker: me, responses: one("Listen"), follow: vec![U_TARGET, U_ONLY], ..L::default() }],
        ),
        topic(
            T_FU,
            "TopicFollowed",
            "Followed",
            topic_type::TOPIC,
            0,
            50.0,
            vec![
                L { id: U_TARGET, quest: QA, speaker: me, responses: one("As I was saying"), ..L::default() },
                L { id: U_PLAIN, quest: QA, speaker: me, responses: one("Fine, then"), ..L::default() },
            ],
        ),
        topic(T_FU_ONLY, "TopicFollowOnly", "Only", topic_type::TOPIC, 0, 50.0, vec![L { id: U_ONLY, quest: QA, speaker: me, responses: one("Only this"), ..L::default() }]),
        topic(
            T_BYE,
            "TopicBye",
            "Bye",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: C_BYE, quest: QA, flags: info_flag::GOODBYE, speaker: me, responses: one("Farewell"), end: Some("9"), ..L::default() }],
        ),
        topic(
            T_HIDDEN,
            "TopicHidden",
            "Hidden",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: D_HIDDEN, quest: QA, speaker: me, conds: vec![gate(EQ, 99.0)], responses: one("Secret"), ..L::default() }],
        ),
        topic(
            T_TOP,
            "TopicDaily",
            "Daily",
            topic_type::TOPIC,
            top_level,
            70.0,
            vec![L { id: E_DAY, quest: QA, flags: info_flag::SAY_ONCE_A_DAY, speaker: me, responses: one("Today"), ..L::default() }],
        ),
        topic(
            T_TOP2,
            "TopicNow",
            "Now",
            topic_type::TOPIC,
            top_level,
            90.0,
            vec![L { id: E_NOW, quest: QA, flags: info_flag::RUN_IMMEDIATELY, speaker: me, responses: one("Now"), begin: Some("20"), end: Some("21"), ..L::default() }],
        ),
        topic(T_ADDED, "TopicAdded", "Added", topic_type::TOPIC, 0, 60.0, vec![L { id: F_ADDED, quest: QA, speaker: me, responses: one("Added"), ..L::default() }]),
        topic(
            T_SKILL,
            "TopicSkill",
            "Skill",
            topic_type::TOPIC,
            top_level,
            10.0,
            vec![
                L {
                    id: H_PASS,
                    quest: QA,
                    speaker: me,
                    conds: vec![ctda(GE, 35.0, GET_ACTOR_VALUE, SPEECH, 1)],
                    responses: one("Convinced"),
                    prompt: Some("Trust me"),
                    knam: Some(AVIF),
                    ..L::default()
                },
                L {
                    id: H_FAIL,
                    quest: QA,
                    speaker: me,
                    conds: vec![ctda(LT, 35.0, GET_ACTOR_VALUE, SPEECH, 1)],
                    responses: one("No"),
                    prompt: Some("Trust me?"),
                    knam: Some(AVIF),
                    ..L::default()
                },
            ],
        ),
        // Two groups of Random lines with a plain line between them: two pools. The first
        // group is said unless TestGate is 31; the plain line only when it is 77.
        topic(
            T_POOL,
            "TopicPool",
            "Pool",
            topic_type::TOPIC,
            0,
            50.0,
            vec![
                L { id: P1, quest: QA, flags: info_flag::RANDOM, speaker: me, conds: vec![gate(NE, 31.0)], responses: one("P1"), ..L::default() },
                L { id: P2, quest: QA, flags: info_flag::RANDOM, speaker: me, conds: vec![gate(NE, 31.0)], responses: one("P2"), ..L::default() },
                L { id: P_PLAIN, quest: QA, speaker: me, conds: vec![gate(EQ, 77.0)], responses: one("Plain"), ..L::default() },
                L { id: P3, quest: QA, flags: info_flag::RANDOM, speaker: me, responses: one("P3"), ..L::default() },
                L { id: P4, quest: QA, flags: info_flag::RANDOM, speaker: me, responses: one("P4"), ..L::default() },
            ],
        ),
        topic(
            GOODBYE,
            "GOODBYE",
            "GOODBYE",
            topic_type::CONVERSATION,
            0,
            50.0,
            vec![L { id: I_BYE, quest: QA, flags: info_flag::GOODBYE, responses: one("See you"), ..L::default() }],
        ),
        // A conversation between the two actors: the first opens with two responses (only
        // the first is said) and links to two topics, of which the second actor can say
        // only one; its reply goes on with itself (next speaker Self) and its last line
        // says goodbye, its Link To ignored.
        topic(
            T_ARGUE,
            "TestArgue",
            "TestArgue",
            topic_type::CONVERSATION,
            0,
            50.0,
            vec![L {
                id: K_OPEN,
                quest: QA,
                ty: CONV,
                speaker: me,
                responses: vec![(1, "Open", 0), (2, "Never said", 0)],
                choices: vec![T_REPLY_NOT, T_REPLY],
                begin: Some("30"),
                end: Some("31"),
                ..L::default()
            }],
        ),
        topic(
            T_REPLY,
            "TestReply",
            "TestReply",
            topic_type::CONVERSATION,
            0,
            50.0,
            vec![
                // A Topic line the second actor could say, stored first: never said
                // between actors.
                L { id: K_REPLY_TOPIC, quest: QA, speaker: Some(NPC2), responses: one("For the player"), ..L::default() },
                L { id: K_REPLY, quest: QA, ty: CONV, next: 1, speaker: Some(NPC2), responses: one("Reply"), choices: vec![T_LAST], end: Some("32"), ..L::default() },
            ],
        ),
        topic(
            T_REPLY_NOT,
            "TestReplyNot",
            "TestReplyNot",
            topic_type::CONVERSATION,
            0,
            50.0,
            vec![L { id: K_NOT, quest: QA, ty: CONV, speaker: Some(NPC2), conds: vec![gate(EQ, 99.0)], responses: one("Not said"), ..L::default() }],
        ),
        topic(
            T_LAST,
            "TestLast",
            "TestLast",
            topic_type::CONVERSATION,
            0,
            50.0,
            // A Goodbye line that links nowhere ends the conversation (one that links on
            // goes on: `an_actor_talks_with_a_talking_activator_through_goodbye_lines_that_link_on`).
            vec![L { id: K_LAST, quest: QA, ty: CONV, flags: info_flag::GOODBYE, speaker: Some(NPC2), responses: one("Last"), ..L::default() }],
        ),
        // HELLO: a line with a Link From (cannot start anything); a Topic line for the
        // player, said only when TestGate is 5; then a Conversation line that starts the
        // same exchange as TestArgue's reply.
        topic(
            HELLO,
            "HELLO",
            "HELLO",
            topic_type::CONVERSATION,
            0,
            50.0,
            vec![
                L { id: K_HELLO_LINKED, quest: QA, ty: CONV, speaker: me, responses: one("Linked"), link_from: vec![T_ARGUE], ..L::default() },
                L { id: K_HELLO_PLAYER, quest: QA, speaker: me, conds: vec![gate(EQ, 5.0)], responses: one("Hello, stranger"), ..L::default() },
                L { id: K_HELLO, quest: QA, ty: CONV, speaker: me, responses: one("Hi"), choices: vec![T_REPLY], ..L::default() },
            ],
        ),
        // Shared lines (INFC / INFX): TopicShared has no line of its own and shares
        // TopicSource's. TopicLinks' line offers it and the hidden topic as its choices;
        // TopicDeadEnd's only choice cannot be said.
        topic(T_SOURCE, "TopicSource", "Where?", topic_type::TOPIC, 0, 50.0, vec![L { id: S_LINE, quest: QA, speaker: me, responses: one("Here"), ..L::default() }]),
        topic_sharing(T_SHARED, "TopicShared", "Where?", topic_type::TOPIC, 0, 50.0, &[(QA, S_LINE, 0)], vec![]),
        topic(
            T_LINKS,
            "TopicLinks",
            "Links",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: S_LINKS, quest: QA, speaker: me, responses: one("Ask away"), choices: vec![T_HIDDEN, T_SHARED], ..L::default() }],
        ),
        topic(
            T_DEAD_END,
            "TopicDeadEnd",
            "Dead end",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: S_DEAD_END, quest: QA, speaker: me, responses: one("Hmm"), choices: vec![T_HIDDEN], ..L::default() }],
        ),
        // Link From: TopicLinkTo's line names TopicLinkFrom (whose own only choice cannot
        // be said) and TopicLinkPlain (no choices of its own).
        topic(
            T_LF_FROM,
            "TopicLinkFrom",
            "Link from",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: S_LF_FROM, quest: QA, speaker: me, responses: one("From"), choices: vec![T_HIDDEN], ..L::default() }],
        ),
        topic(T_LF_PLAIN, "TopicLinkPlain", "Link plain", topic_type::TOPIC, 0, 50.0, vec![L { id: S_LF_PLAIN, quest: QA, speaker: me, responses: one("Plain"), ..L::default() }]),
        topic(
            T_LF_TO,
            "TopicLinkTo",
            "Link to",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: S_LF_TO, quest: QA, speaker: me, responses: one("To"), link_from: vec![T_LF_FROM, T_LF_PLAIN], ..L::default() }],
        ),
        // Fallout 3's speech challenge failure topic: a line whose Link From names the
        // challenge's topic (Lucas Simms's MegTownFatherChoice answer, #3024).
        // SayTo with the next speaker Self: V_SELF, then V_BYE (Goodbye, TestOut 4); the
        // Topic line after it is never reached.
        topic(
            T_SELF,
            "TestSelf",
            "TestSelf",
            topic_type::CONVERSATION,
            0,
            50.0,
            vec![
                L { id: V_SELF, quest: QA, ty: CONV, next: 1, flags: info_flag::SAY_ONCE, speaker: me, responses: one("First"), ..L::default() },
                L { id: V_BYE, quest: QA, ty: CONV, flags: info_flag::GOODBYE | info_flag::SAY_ONCE, speaker: me, responses: one("Second"), end: Some("4"), ..L::default() },
                L { id: V_AFTER, quest: QA, ty: CONV, speaker: me, responses: one("Third"), ..L::default() },
            ],
        ),
        topic(T_FAIL, "SpeechChallengeFailure", "", topic_type::TOPIC, 0, 50.0, vec![L { id: S_FAIL, quest: QA, speaker: me, responses: one("Sorry friend."), link_from: vec![T_LF_PLAIN], ..L::default() }]),
        // Between actors: TestChat's line links nowhere; TestAnswer's second line names
        // TestChat in its Link From, its first (an opener) does not.
        topic(T_CHAT, "TestChat", "TestChat", topic_type::CONVERSATION, 0, 50.0, vec![L { id: C_CHAT, quest: QA, ty: CONV, speaker: me, responses: one("Chat"), ..L::default() }]),
        topic(
            T_GROUP,
            "TestGroup",
            "Group",
            topic_type::TOPIC,
            0,
            50.0,
            vec![
                L { id: GR_CREW, quest: QA, speaker: Some(NPC2), conds: vec![ctda(EQ, 1.0, GET_IN_FACTION, CREW, 0)], responses: one("Halt"), ..L::default() },
                L { id: GR_FIXED, quest: QA, speaker: Some(NPC2), responses: one("Mine alone"), ..L::default() },
            ],
        ),
        topic(
            T_ANSWER,
            "TestAnswer",
            "TestAnswer",
            topic_type::CONVERSATION,
            0,
            50.0,
            vec![
                L { id: C_OPENER, quest: QA, ty: CONV, flags: info_flag::GOODBYE, speaker: Some(NPC2), responses: one("Opener"), ..L::default() },
                L { id: C_REPLY, quest: QA, ty: CONV, flags: info_flag::GOODBYE, speaker: Some(NPC2), responses: one("Reply"), link_from: vec![T_CHAT], ..L::default() },
            ],
        ),
        topic(T_PING, "TestPing", "TestPing", topic_type::CONVERSATION, 0, 50.0, vec![L { id: P_PING, quest: QA, ty: CONV, responses: one("Ping"), choices: vec![T_PONG], ..L::default() }]),
        topic(T_PONG, "TestPong", "TestPong", topic_type::CONVERSATION, 0, 50.0, vec![L { id: P_PONG, quest: QA, ty: CONV, responses: one("Pong"), choices: vec![T_PING], ..L::default() }]),
        topic(T_RPING, "TestRPing", "TestRPing", topic_type::CONVERSATION, 0, 50.0, vec![L { id: P_RPING, quest: QREP, ty: CONV, responses: one("Ping"), choices: vec![T_RPONG], ..L::default() }]),
        topic(T_RPONG, "TestRPong", "TestRPong", topic_type::CONVERSATION, 0, 50.0, vec![L { id: P_RPONG, quest: QREP, ty: CONV, responses: one("Pong"), choices: vec![T_RPING], ..L::default() }]),
        topic(
            T_HIST_A,
            "TestHistA",
            "TestHistA",
            topic_type::CONVERSATION,
            0,
            50.0,
            vec![
                L { id: H_A, quest: QA, ty: CONV, responses: one("Start"), choices: vec![T_HIST_B], ..L::default() },
                L { id: H_FOLLOW, quest: QA, ty: CONV, responses: one("Follow-up"), choices: vec![T_HIST_B], ..L::default() },
            ],
        ),
        topic(T_HIST_B, "TestHistB", "TestHistB", topic_type::CONVERSATION, 0, 50.0, vec![L { id: H_B, quest: QA, ty: CONV, responses: one("Linked"), follow: vec![H_FOLLOW], ..L::default() }]),
        topic(
            T_RHIST_A,
            "TestRHistA",
            "TestRHistA",
            topic_type::CONVERSATION,
            0,
            50.0,
            vec![
                L { id: H_RA, quest: QREP, ty: CONV, responses: one("Start"), choices: vec![T_RHIST_B], ..L::default() },
                L { id: H_RFOLLOW, quest: QREP, ty: CONV, responses: one("Follow-up"), choices: vec![T_RHIST_B], ..L::default() },
            ],
        ),
        topic(T_RHIST_B, "TestRHistB", "TestRHistB", topic_type::CONVERSATION, 0, 50.0, vec![L { id: H_RB, quest: QREP, ty: CONV, responses: one("Linked"), follow: vec![H_RFOLLOW], ..L::default() }]),
        topic(
            T_HANDOFF,
            "TestHandoff",
            "Handoff",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: H_HANDOFF, quest: QA, flags: info_flag::GOODBYE, speaker: me, responses: one("Go see him"), end_talk: true, ..L::default() }],
        ),
        topic(
            T_HANDOFF_STAY,
            "TestHandoffStay",
            "HandoffStay",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: H_HANDOFF_STAY, quest: QA, speaker: me, responses: one("He can wait"), choices: vec![T_BYE], end_talk: true, ..L::default() }],
        ),
        // Lines with follow-ups and choices: W_BOTH's follow-up is a Goodbye line, W_KEEP's
        // has choices of its own.
        topic(
            T_BOTH,
            "TestBoth",
            "Both",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: W_BOTH, quest: QA, speaker: me, responses: one("Hear me out"), follow: vec![W_BOTH_BYE], choices: vec![T_ASK, T_BYE], ..L::default() }],
        ),
        topic(
            T_KEEP,
            "TestKeep",
            "Keep",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: W_KEEP, quest: QA, speaker: me, responses: one("Listen"), follow: vec![W_BOTH_FU], choices: vec![T_ASK, T_BYE], ..L::default() }],
        ),
        topic(
            T_BOTH_FU,
            "TestBothFollow",
            "BothFollow",
            topic_type::TOPIC,
            0,
            50.0,
            vec![
                L { id: W_BOTH_FU, quest: QA, speaker: me, responses: one("And another thing"), choices: vec![T_BYE], ..L::default() },
                L { id: W_BOTH_BYE, quest: QA, flags: info_flag::GOODBYE, speaker: me, responses: one("That's all"), ..L::default() },
            ],
        ),
        topic(T_TAPE, "TestTape", "TestTape", topic_type::TOPIC, 0, 50.0, vec![L { id: S_TAPE, quest: QA, speaker: me, responses: one("Tape"), begin: Some("6"), end: Some("7"), ..L::default() }]),
        topic(
            T_RELAY,
            "TestRelay",
            "Relay",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: H_RELAY, quest: QA, flags: info_flag::GOODBYE, speaker: me, responses: one("Off you go"), add: vec![T_ADDED], end_raw: Some(relay_result()), ..L::default() }],
        ),
        topic(
            T_BYE_B,
            "TestByeB",
            "ByeB",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: B_BYE, quest: QA, flags: info_flag::GOODBYE, speaker: Some(NPC2), responses: one("So long"), end_raw: Some(count_result()), ..L::default() }],
        ),
        topic(
            T_B_HAND,
            "TestByeHandoff",
            "ByeHandoff",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: B_HAND, quest: QA, flags: info_flag::GOODBYE, speaker: Some(NPC2), responses: one("See him"), add: vec![T_SOURCE], end_raw: Some(talk_result_by(NPC1_REF)), ..L::default() }],
        ),
        topic(
            T_BEGIN_SAY,
            "TestBeginSay",
            "BeginSay",
            topic_type::TOPIC,
            0,
            50.0,
            vec![L { id: H_BEGIN_SAY, quest: QA, flags: info_flag::GOODBYE, speaker: me, responses: one("Hold on"), begin_raw: Some(say_result(NPC1_REF, T_ASK)), end: Some("9"), ..L::default() }],
        ),
    ]
    .concat();
    // DATA 3 Voice; SNAM the speaker, TNAM the topic (the NOTE unions, holotapes.md §1).
    let note = record(b"NOTE", 0, NOTE_TAPE, &[sub(b"EDID", z("TestTapeNote")), sub(b"DATA", vec![3]), sub(b"SNAM", u32b(NPC1)), sub(b"TNAM", u32b(T_TAPE)), sub(b"ONAM", u32b(QA))]);
    let placed = |id: u32, base: u32, edid: &str| record(b"ACHR", 0, id, &[sub(b"EDID", z(edid)), sub(b"NAME", u32b(base)), sub(b"DATA", vec![0; 24])]);
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TalkCell"))]);
    // A patrol point whose result script (after XPPA) sets TestOut to 9.
    let patrol = record(b"REFR", 0, PATROL_REF, &[vec![sub(b"EDID", z("PatrolREF")), sub(b"NAME", u32b(NPC2)), sub(b"XPRD", f32b(0.0)), sub(b"XPPA", vec![])], result(Some("9")), vec![sub(b"DATA", vec![0; 24])]].concat());
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &[placed(NPC1_REF, NPC1, "TalkerREF"), placed(NPC2_REF, NPC2, "ChatterREF"), patrol].concat()));
    [
        tes4(),
        top(b"GLOB", &globs),
        top(b"SCPT", &tick_script),
        top(b"AVIF", &avif),
        top(b"FACT", &record(b"FACT", 0, CREW, &[sub(b"EDID", z("TestCrew"))])),
        top(b"NPC_", &npcs),
        top(b"QUST", &quests),
        top(b"NOTE", &note),
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
    fixture_bytes(&plugin_bytes())
}

fn fixture_bytes(bytes: &[u8]) -> Fixture {
    let plugins = vec![Plugin::parse(bytes, "Talk.esm".into()).unwrap()];
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

fn game_seeded(f: &Fixture, seed: u64) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, seed, ..GameConfig::default() });
    g.new_game();
    g
}

fn game(f: &Fixture) -> Game<'_> {
    game_seeded(f, 0)
}

/// A game with `GameConfig::goodbye_at_once`: a chosen Goodbye line closes the
/// conversation at once and goes on in the world (`Game::say_farewell`, #1606). Off by
/// default since #2157; the farewell tests run with it.
fn game_at_once(f: &Fixture) -> Game<'_> {
    let mut g = game(f);
    g.cfg.goodbye_at_once = true;
    g
}

fn actor() -> FormId {
    FormId(NPC1_REF)
}

fn out(g: &Game) -> f32 {
    g.world.global(FormId(G_OUT)).unwrap()
}

fn set_global(g: &mut Game, id: u32, v: f32) {
    g.world.set_global(FormId(id), v).unwrap();
}

/// Frames until the actor has finished speaking (choices offered or conversation over).
fn listen(g: &mut Game) {
    for _ in 0..20_000 {
        if g.dialogue.as_ref().is_none_or(|c| !c.choices.is_empty()) {
            return;
        }
        g.tick();
    }
    panic!("the actor never stopped speaking");
}

/// Frames until the actor has finished the Goodbye line the player chose, said in the
/// world once its conversation closed (`Speech::farewell`).
fn hear_out(g: &mut Game) {
    for _ in 0..20_000 {
        if !g.speech.iter().any(|s| s.farewell) {
            return;
        }
        g.tick();
    }
    panic!("the farewell never ended");
}

/// Where the first trace event that matches comes in the trace.
fn event_at(g: &Game, m: impl Fn(&EventKind) -> bool) -> usize {
    g.events.iter().position(|e| m(&e.kind)).expect("no such event")
}

fn offered(g: &Game) -> Vec<u32> {
    g.dialogue_choices().iter().map(|c| c.topic.0).collect()
}

fn pick(g: &mut Game, topic: u32) -> FormId {
    let i = g.dialogue_choices().iter().position(|c| c.topic.0 == topic).unwrap_or_else(|| panic!("0x{topic:X} is not offered: {:X?}", offered(g)));
    g.choose(i).unwrap()
}

/// A conversation past the first greeting, at the topic list.
fn at_topic_list<'a>(f: &'a Fixture) -> Game<'a> {
    let mut g = game(f);
    g.said.insert((actor(), FormId(A_FIRST)));
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_LATER)));
    listen(&mut g);
    g
}

fn call(g: &mut Game, opcode: u16, on: u32, args: &[ArgValue]) -> f64 {
    let ctx = Ctx { unit: "test".into(), opcode, on: Some(FormId(on)), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, args).num()
}

#[test]
fn the_records_are_read_in_full() {
    let f = fixture();
    let d = &f.data;
    let t = &d.topic_defs[&FormId(T_TOP2)];
    assert_eq!((t.edid.as_deref(), t.name.as_deref(), t.topic_type, t.top_level(), t.priority), (Some("TopicNow"), Some("Now"), topic_type::TOPIC, true, 90.0));
    assert_eq!(d.topic_defs[&FormId(GOODBYE)].topic_type, topic_type::CONVERSATION);
    assert_eq!((d.greeting, d.goodbye), (Some(FormId(GREETING)), Some(FormId(GOODBYE))));
    // Offered order of the top-level topics: priority, highest first.
    assert_eq!(d.top_level, vec![FormId(T_TOP2), FormId(T_TOP), FormId(T_SKILL)]);
    let i = &d.infos[&FormId(A_FIRST)];
    assert!(i.has(info_flag::SAY_ONCE) && i.say_once && !i.goodbye);
    assert_eq!((i.quest, i.speaker, i.topic), (Some(FormId(QA)), Some(FormId(NPC1)), Some(FormId(GREETING))));
    assert_eq!(i.choices, vec![FormId(T_ASK), FormId(T_BYE), FormId(T_HIDDEN)]);
    assert_eq!(i.responses.iter().map(|r| (r.number, r.text.as_str(), r.emotion, r.emotion_value)).collect::<Vec<_>>(), vec![(2, "Second.", 5, 50), (1, "First", 3, 50)]);
    assert_eq!(i.response_chars, vec![7, 5]);
    assert_eq!(i.response_order(), vec![1, 0]);
    assert!(i.begin.is_some() && i.end.is_some());
    let b = &d.infos[&FormId(B_ASK)];
    assert_eq!((b.prompt.as_deref(), &b.follow_ups, &b.add_topics), (Some("Tell me"), &vec![FormId(B_FOLLOW)], &vec![FormId(T_ADDED)]));
    assert_eq!(d.infos[&FormId(H_PASS)].challenge_form, Some(FormId(AVIF)));
    assert!(d.infos[&FormId(E_NOW)].has(info_flag::RUN_IMMEDIATELY) && d.infos[&FormId(E_DAY)].has(info_flag::SAY_ONCE_A_DAY));
    assert!(d.infos[&FormId(R2)].has(info_flag::RANDOM | info_flag::RANDOM_END));
    assert_eq!(d.choice_text(FormId(T_ASK), FormId(B_ASK)), "Tell me");
    assert_eq!(d.choice_text(FormId(T_BYE), FormId(C_BYE)), "Bye");
}

#[test]
fn the_greeting_is_the_first_line_the_actor_can_say() {
    let f = fixture();
    let mut g = game(&f);
    // QOff's line comes first by priority, but its quest is stopped.
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_FIRST)));
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::DialogueStarted { actor: a, topic } if a == actor() && topic == FormId(GREETING))));
    // An actor with nothing to say cannot be talked to.
    let mut h = game(&f);
    h.stop_quest(FormId(QA));
    h.stop_quest(FormId(QHI));
    h.stop_quest(FormId(QEQ));
    assert!(h.start_dialogue(actor(), None).is_err());
    assert!(h.dialogue.is_none());
    assert!(h.events.iter().any(|e| matches!(e.kind, EventKind::NoLine { .. })));
}

#[test]
fn a_topic_chosen_passes_over_its_follow_up_lines_while_another_line_can_be_said() {
    let f = fixture();
    assert!(f.data.follow_up_targets.contains(&FormId(U_TARGET)) && !f.data.follow_up_targets.contains(&FormId(U_PLAIN)));
    let mut g = game(&f);
    g.player.topics.extend([FormId(T_FU), FormId(T_FU_ONLY)]);
    g.said.insert((actor(), FormId(A_FIRST)));
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_LATER)));
    listen(&mut g);
    let info = |g: &Game, t: u32| g.dialogue_choices().iter().find(|c| c.topic.0 == t).map(|c| c.info);
    // T_FU's first line is a follow-up of U_LEAD: the topic offered says its other line.
    assert_eq!(info(&g, T_FU), Some(FormId(U_PLAIN)));
    // A topic with nothing but a follow-up line still says it.
    assert_eq!(info(&g, T_FU_ONLY), Some(FormId(U_ONLY)));
    assert_eq!(pick(&mut g, T_FU), FormId(U_PLAIN));
    // Said other than as a choice (a greeting, SayTo), the stored first line is taken.
    assert_eq!(g.select_line(actor(), Some(FormId::PLAYER_REF), FormId(T_FU)), Some(FormId(U_TARGET)));
}

#[test]
fn the_higher_priority_quest_wins_and_equal_priorities_keep_the_stored_order() {
    let f = fixture();
    let mut g = game(&f);
    // Both A_FIRST (QA, 50) and A_HIGH (QHi, 80, stored later) qualify.
    set_global(&mut g, G_GATE, 5.0);
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_HIGH)));
    // QA and QEq have the same priority: QA's line is stored first.
    let mut h = game(&f);
    h.said.insert((actor(), FormId(A_FIRST)));
    assert_eq!(h.start_dialogue(actor(), None), Ok(FormId(A_LATER)));
    assert_eq!(
        f.data.lines_by_priority(FormId(GREETING)).iter().map(|i| i.0).collect::<Vec<_>>(),
        vec![A_OFF, A_HIGH, A_BARK, A_FIRST, A_LATER, A_EQUAL, R1, R2, R_AFTER]
    );
}

#[test]
fn a_line_runs_its_begin_script_says_its_responses_in_order_and_runs_its_end_script() {
    let f = fixture();
    let mut g = game(&f);
    g.start_dialogue(actor(), None).unwrap();
    // The begin result ran inside the start; the actor talks; the dialogue menu is up.
    assert_eq!(out(&g), 1.0);
    assert!(g.is_talking(actor()));
    assert_eq!(call(&mut g, 0x1024, NPC1_REF, &[ArgValue::Value(Value::Num(1009.0))]), 1.0);
    // Response 1 ("First", emotion 3) before response 2 ("Second.", emotion 5).
    assert_eq!(g.dialogue_emotion(actor()), 3);
    let f0 = g.frame;
    let first = fnv_game::game::fitted_seconds(5);
    g.frames((first * 60.0) as u64 + 2);
    assert_eq!(g.dialogue_emotion(actor()), 5);
    assert_eq!(out(&g), 1.0);
    listen(&mut g);
    // The line lasted as long as its two responses (1.1 s + 1/16 s per character each).
    let want = (fnv_game::game::fitted_seconds(5) + fnv_game::game::fitted_seconds(7)) * 60.0;
    assert_eq!(g.line_seconds(actor(), FormId(A_FIRST)) * 60.0, want);
    assert!(((g.frame - f0) as f64 - want).abs() <= 2.0, "{} frames for {want}", g.frame - f0);
    assert_eq!(out(&g), 2.0);
    assert!(!g.is_talking(actor()));
    let said: Vec<(u8, usize)> = g.events.iter().filter_map(|e| if let EventKind::ResponseSaid { number, chars, .. } = e.kind { Some((number, chars)) } else { None }).collect();
    assert_eq!(said, vec![(1, 5), (2, 7)]);
    // A menu (a message box) holds the conversation.
    let mut h = game(&f);
    h.start_dialogue(actor(), None).unwrap();
    h.menu = Some(1001);
    h.frames(2000);
    assert!(h.is_talking(actor()));
}

#[test]
fn a_conversation_pauses_the_game_behind_it() {
    let f = fixture();
    let mut g = game(&f);
    let ticks = |g: &Game| (g.world.global(FormId(G_TICKS)).unwrap(), g.world.global(FormId(G_MENU_TICKS)).unwrap());
    g.frames(10);
    assert_eq!(ticks(&g), (10.0, 0.0));
    // A line said outside a conversation pauses nothing.
    g.say(actor(), None, FormId(T_BYE), false).unwrap();
    g.frames(5);
    assert_eq!(ticks(&g), (15.0, 0.0));
    assert!(!g.paused() && !g.in_menu());
    // In the conversation: menu mode. GameMode does not run, game time stands still,
    // MenuMode blocks run, MenuMode reads 1 with and without the code. `Game::menu` is
    // for the other menus.
    let t0 = g.time;
    g.start_dialogue(actor(), None).unwrap();
    assert!(g.paused() && g.in_menu() && g.menu_open(1009) && !g.menu_open(1001));
    assert_eq!((g.menu, g.open_menu_code()), (None, Some(1009)));
    assert_eq!(call(&mut g, 0x1024, NPC1_REF, &[]), 1.0);
    assert_eq!(call(&mut g, 0x1024, NPC1_REF, &[ArgValue::Value(Value::Num(1009.0))]), 1.0);
    assert_eq!(call(&mut g, 0x1024, NPC1_REF, &[ArgValue::Value(Value::Num(1001.0))]), 0.0);
    let f0 = g.frame;
    listen(&mut g);
    let spoken = (g.frame - f0) as f32;
    assert!(spoken > 60.0);
    assert_eq!(ticks(&g), (15.0, spoken));
    assert_eq!(g.time, t0);
    // Choosing an answer does not unpause, a Goodbye line neither: it holds the pause
    // until it is said, then ends the conversation (#2157).
    pick(&mut g, T_BYE);
    assert!(g.dialogue.is_some() && g.paused() && g.in_menu());
    listen(&mut g);
    assert!(g.dialogue.is_none() && !g.paused());
    assert_eq!(g.time, t0);
    let menu_ticks = ticks(&g).1;
    g.frames(4);
    assert_eq!(ticks(&g), (19.0, menu_ticks));
    assert!((g.time - t0 - 4.0 / 60.0).abs() < 1e-9);
    assert_eq!(call(&mut g, 0x1024, NPC1_REF, &[]), 0.0);
    // A message box over the conversation holds it; both menus read as open.
    let mut h = game(&f);
    h.start_dialogue(actor(), None).unwrap();
    h.menu = Some(1001);
    assert!(h.menu_open(1001) && h.menu_open(1009));
    assert_eq!(h.open_menu_code(), Some(1001));
    h.frames(2000);
    assert!(h.is_talking(actor()));
    h.menu = None;
    listen(&mut h);
    assert!(!h.dialogue_choices().is_empty());
}

#[test]
fn random_lines_that_sit_together_are_a_pool_and_a_plain_line_splits_pools() {
    let f = fixture();
    let listener = Some(FormId::PLAYER_REF);
    let picks = |gate: f32| {
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..32 {
            let mut g = game_seeded(&f, seed);
            set_global(&mut g, G_GATE, gate);
            seen.insert(g.select_line(actor(), listener, FormId(T_POOL)).unwrap().0);
        }
        seen.into_iter().collect::<Vec<u32>>()
    };
    // R R N R R with N not sayable: the first pair is one pool, never mixed with the
    // second pair.
    assert_eq!(picks(0.0), vec![P1, P2]);
    // Nothing sayable in the first pool, the plain line not sayable either: the second
    // pool.
    assert_eq!(picks(31.0), vec![P3, P4]);
    // The plain line sayable: the first pool still comes first.
    assert_eq!(picks(77.0), vec![P1, P2]);
    assert_eq!(f.data.lines_by_priority(FormId(T_POOL)).iter().map(|i| i.0).collect::<Vec<_>>(), vec![P1, P2, P_PLAIN, P3, P4]);
}

#[test]
fn a_lines_choices_are_offered_and_nothing_else_and_a_topic_without_a_line_is_hidden() {
    let f = fixture();
    let mut g = game(&f);
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    // T_HIDDEN's only line fails its condition; the top-level topics are not listed.
    assert_eq!(offered(&g), vec![T_ASK, T_BYE]);
    let c = g.dialogue_choices().to_vec();
    assert_eq!((c[0].info, c[0].prompt, c[0].chars, &*g.choice_text(&c[0])), (FormId(B_ASK), true, 7, "Tell me"));
    assert_eq!((c[1].info, c[1].prompt, c[1].goodbye, &*g.choice_text(&c[1])), (FormId(C_BYE), false, true, "Bye"));
    assert!(g.events.iter().any(|e| matches!(&e.kind, EventKind::ChoicesOffered { choices, .. } if choices.len() == 2)));
    // Choosing while the actor speaks, or an entry that is not there, is refused.
    assert!(g.choose(5).is_err());
    pick(&mut g, T_ASK);
    assert!(g.choose(0).is_err());
}

/// The engine lets Tab leave a topic list even without a Goodbye (#1595); a line's own
/// choices are no topic list.
#[test]
fn the_conversation_says_whether_its_choices_are_the_topic_list() {
    let f = fixture();
    let mut g = game(&f);
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    assert_eq!(offered(&g), vec![T_ASK, T_BYE], "the first greeting's own choices");
    assert!(!g.dialogue.as_ref().unwrap().topic_list);
    let g = at_topic_list(&f);
    assert!(!g.dialogue_choices().is_empty());
    assert!(g.dialogue.as_ref().unwrap().topic_list, "a line without choices of its own offers the topic list");
}

/// A choice listed for a dim player shows its topic's Dumb Response (`Game::player_dim`,
/// checked against the master in dialogue_data.rs); the fixture's player has no record,
/// so it is not dim.
#[test]
fn a_dim_players_choice_shows_the_topics_dumb_response() {
    let mut f = fixture();
    f.data.topic_defs.get_mut(&FormId(T_ASK)).unwrap().dumb_response = Some("Me ask".into());
    let mut g = game(&f);
    assert!(!g.player_dim(), "no Intelligence to read: not dim");
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    let c = g.dialogue_choices().to_vec();
    assert_eq!((c[0].topic, c[0].dumb, &*g.choice_text(&c[0])), (FormId(T_ASK), false, "Tell me"));
    let dim = fnv_game::dialogue::Choice { dumb: true, ..c[0].clone() };
    assert_eq!(g.choice_text(&dim), "Me ask");
    // A topic without one keeps its text whatever the flag.
    assert_eq!(g.choice_text(&fnv_game::dialogue::Choice { dumb: true, ..c[1].clone() }), "Bye");
}

/// A choice shows the player string codes of its text as they read now ("The name's
/// &PCName;." in FalloutNV.esm, issue #1720); a code that names nothing stays as written.
#[test]
fn a_choice_shows_the_players_name_for_its_code() {
    let mut f = fixture();
    f.data.infos.get_mut(&FormId(B_ASK)).unwrap().prompt = Some("The name's &PCName;. &NoSuchCode; &pcsexpronoun;".into());
    let mut g = game(&f);
    g.player.name = Some("Courier Six".into());
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    let c = g.dialogue_choices().to_vec();
    assert_eq!(g.choice_text(&c[0]), "The name's &PCName;. &NoSuchCode; &pcsexpronoun;", "the data's text is kept");
    assert_eq!(g.shown_choice_text(&c[0]), "The name's Courier Six. &NoSuchCode; he");
    assert_eq!(g.shown_choice_text(&c[1]), "Bye");
}

#[test]
fn a_choice_with_only_a_shared_line_is_offered_and_says_that_line() {
    let f = fixture();
    // The shared line counts as a line of the topic that lists it, not of its own.
    assert!(f.data.lines_by_priority(FormId(T_SHARED)).is_empty());
    assert_eq!(f.data.lines_tried(FormId(T_SHARED)), vec![FormId(S_LINE)]);
    assert_eq!(f.data.topics[&FormId(T_SOURCE)], vec![FormId(S_LINE)]);
    let mut g = game(&f);
    assert_eq!(g.start_dialogue(actor(), Some(FormId(T_LINKS))), Ok(FormId(S_LINKS)));
    listen(&mut g);
    // Only the line's choices: the hidden one has nothing to say, the shared one is
    // offered with the topic's own name; no top-level topics, no GOODBYE.
    assert_eq!(offered(&g), vec![T_SHARED]);
    assert_eq!(g.choice_text(&g.dialogue_choices()[0]), "Where?");
    assert_eq!(pick(&mut g, T_SHARED), FormId(S_LINE));
}

fn shared_order_fixture(listing: Vec<u8>, source: Vec<L>) -> Fixture {
    let source = topic(0xB81, "SharedOrderSource", "Source", topic_type::TOPIC, 0, 50.0, source);
    fixture_bytes(&[plugin_bytes(), top(b"DIAL", &[listing, source].concat())].concat())
}

/// The own lines of a topic in `lines`, in the order `lines` has them.
fn own_lines(f: &Fixture, topic: FormId, lines: &[FormId]) -> Vec<FormId> {
    lines.iter().copied().filter(|i| f.data.topics[&topic].contains(i)).collect()
}

/// Info Index belongs to the listing quest, not the entire topic
/// (see docs/spec-notes/dialogue.md §1.2). [design] Each shared line goes in among its
/// listing quest's own lines; the own lines keep their stored order, so equal priorities
/// try them as `lines_by_priority` does, and the Random pools follow that order as in a
/// topic without shared lines (`GameData::lines_tried`).
#[test]
fn shared_indices_place_lines_among_their_quests_own_lines_in_stored_order() {
    let line = |id, quest, flags| L {
        id, quest, flags, responses: vec![(1, "A synthetic line", 0)],
        conds: if quest == QA { vec![gate(EQ, 0.0)] } else { vec![] }, ..L::default()
    };
    let listing = topic_sharing(
        0xB80, "SharedOrder", "Listing", topic_type::TOPIC, 0, 50.0,
        &[(QEQ, 0xB95, 1), (QEQ, 0xB97, 2), (QA, 0xB94, 1), (QA, 0xB96, 2)],
        vec![line(0xB90, QA, info_flag::RANDOM), line(0xB91, QEQ, info_flag::RANDOM), line(0xB92, QA, 0), line(0xB93, QEQ, 0)],
    );
    let f = shared_order_fixture(listing, vec![
        line(0xB94, QA, info_flag::RANDOM | info_flag::RANDOM_END), line(0xB95, QEQ, info_flag::RANDOM | info_flag::RANDOM_END),
        line(0xB96, QA, info_flag::RANDOM), line(0xB97, QEQ, info_flag::RANDOM),
    ]);
    let topic = FormId(0xB80);
    // QA's list is B90, B94, B96, B92 and QEQ's B91, B95, B97, B93: each quest's shared
    // lines go just before its second own line.
    let expected = [0xB90, 0xB91, 0xB94, 0xB96, 0xB92, 0xB95, 0xB97, 0xB93].map(FormId);
    assert_eq!(f.data.shared_lines[&topic], expected);
    assert_eq!(f.data.lines_tried(topic), expected);
    assert_eq!(own_lines(&f, topic, &f.data.lines_tried(topic)), f.data.lines_by_priority(topic));
    assert_eq!(f.data.topic_defs[&topic].quests, [FormId(QEQ), FormId(QA)]);
    // The first pool runs B90, B91, B94 (Random End); QA's lines need the gate at 0.
    for (gate_value, pool) in [(0.0, &[0xB90, 0xB91, 0xB94][..]), (1.0, &[0xB91][..])] {
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..64 {
            let mut g = game_seeded(&f, seed);
            set_global(&mut g, G_GATE, gate_value);
            for _ in 0..8 {
                let pick = g.select_line(actor(), Some(FormId::PLAYER_REF), topic).unwrap();
                assert!(pool.contains(&pick.0), "{seed}: {pick}");
                seen.insert(pick);
            }
        }
        assert_eq!(seen, pool.iter().copied().map(FormId).collect());
    }
}

/// [design] Own lines of two equal-priority quests stored interleaved (A1, B1, A2) keep
/// that order when one of the quests also lists a shared line: the shared line goes in
/// before B1, the quest's first own line, and A2 stays after B1 (dialogue.md §1.2, §4).
#[test]
fn a_shared_line_leaves_interleaved_own_lines_in_stored_order() {
    let listing = topic_sharing(
        0xB80, "SharedOrder", "Listing", topic_type::TOPIC, 0, 50.0,
        &[(QEQ, 0xB94, 0)],
        vec![L { id: 0xB90, quest: QA, ..L::default() }, L { id: 0xB91, quest: QEQ, ..L::default() }, L { id: 0xB92, quest: QA, ..L::default() }],
    );
    let f = shared_order_fixture(listing, vec![L { id: 0xB94, quest: QEQ, ..L::default() }]);
    let topic = FormId(0xB80);
    let tried = f.data.lines_tried(topic);
    assert_eq!(own_lines(&f, topic, &tried), f.data.lines_by_priority(topic));
    assert_eq!(tried, [0xB90, 0xB94, 0xB91, 0xB92].map(FormId));
}

#[test]
fn shared_only_quests_follow_qsti_order_before_index_order() {
    let listing = topic_sharing(
        0xB80, "SharedOrder", "Listing", topic_type::TOPIC, 0, 50.0,
        &[(QEQ, 0xB91, 1), (QEQ, 0xB90, 0), (QA, 0xB93, 1), (QA, 0xB92, 0)], vec![],
    );
    let source = [(0xB90, QEQ), (0xB91, QEQ), (0xB92, QA), (0xB93, QA)].into_iter().map(|(id, quest)| L { id, quest, ..L::default() }).collect();
    let f = shared_order_fixture(listing, source);
    assert_eq!(f.data.lines_tried(FormId(0xB80)), [0xB90, 0xB91, 0xB92, 0xB93].map(FormId));
}

#[test]
fn an_empty_first_qsti_group_still_sets_shared_only_quest_order() {
    let listing = record(b"DIAL", 0, 0xB80, &[
        sub(b"EDID", z("SharedOrder")), sub(b"QSTI", u32b(QA)),
        sub(b"QSTI", u32b(QEQ)), sub(b"INFC", u32b(0xB90)), sub(b"INFX", 0i32.to_le_bytes().to_vec()),
        sub(b"QSTI", u32b(QA)), sub(b"INFC", u32b(0xB91)), sub(b"INFX", 0i32.to_le_bytes().to_vec()),
    ]);
    let f = shared_order_fixture(listing, vec![L { id: 0xB90, quest: QEQ, ..L::default() }, L { id: 0xB91, quest: QA, ..L::default() }]);
    assert_eq!(f.data.lines_tried(FormId(0xB80)), [0xB91, 0xB90].map(FormId));
}

/// [design] A listing/source-quest mismatch changes placement only: owning quest,
/// conditions and priority remain those of the INFO (`data.rs`, `dialogue.rs`).
#[test]
fn a_shared_line_is_placed_by_listing_quest_but_keeps_its_own_priority_and_gate() {
    let listing = topic_sharing(
        0xB80, "SharedOrder", "Listing", topic_type::TOPIC, 0, 50.0,
        &[(QA, 0xB94, 1), (QA, 0xB95, 2)],
        vec![L { id: 0xB90, quest: QA, ..L::default() }, L { id: 0xB91, quest: QEQ, ..L::default() }, L { id: 0xB92, quest: QA, ..L::default() }],
    );
    let f = shared_order_fixture(listing, vec![L { id: 0xB94, quest: QHI, ..L::default() }, L { id: 0xB95, quest: QOFF, ..L::default() }]);
    let topic = FormId(0xB80);
    // QA's list is B90, B94, B95, B92: both shared lines go just before B92.
    assert_eq!(f.data.shared_lines[&topic], [0xB90, 0xB91, 0xB94, 0xB95, 0xB92].map(FormId));
    assert_eq!(f.data.lines_tried(topic), [0xB95, 0xB94, 0xB90, 0xB91, 0xB92].map(FormId));
    assert_eq!(own_lines(&f, topic, &f.data.lines_tried(topic)), f.data.lines_by_priority(topic));
    assert_eq!(f.data.infos[&FormId(0xB94)].quest, Some(FormId(QHI)));
    assert_eq!(f.data.infos[&FormId(0xB94)].topic, Some(FormId(0xB81)));
    let mut g = game(&f);
    assert!(!g.can_say(actor(), Some(FormId::PLAYER_REF), FormId(0xB95)), "the source quest is stopped");
    assert_eq!(g.select_line(actor(), Some(FormId::PLAYER_REF), topic), Some(FormId(0xB94)));
}

/// [design] Malformed pairs keep their previous fallbacks (`data.rs::shared_lines`):
/// missing INFOs are skipped, duplicates are not inserted, bad indices append, and an
/// absent QSTI retains topic-wide insertion instead of inferring INFO ownership.
#[test]
fn malformed_shared_entries_keep_their_fallbacks() {
    let fields = vec![
        sub(b"EDID", z("SharedOrder")),
        sub(b"INFC", u32b(0xB98)), sub(b"INFX", 1i32.to_le_bytes().to_vec()), // No QSTI.
        sub(b"QSTI", u32b(QA)),
        sub(b"INFC", u32b(0xB94)), sub(b"INFX", 99i32.to_le_bytes().to_vec()),
        sub(b"INFC", u32b(0xB95)), sub(b"INFX", (-2i32).to_le_bytes().to_vec()),
        sub(b"INFC", u32b(0xB96)), // No index.
        sub(b"INFC", u32b(0xB97)), sub(b"INFX", (-1i32).to_le_bytes().to_vec()),
        sub(b"INFC", u32b(0xB94)), sub(b"INFX", 100i32.to_le_bytes().to_vec()), // Duplicate.
        sub(b"INFC", u32b(0xB90)), sub(b"INFX", 0i32.to_le_bytes().to_vec()), // Already authored.
        sub(b"INFC", u32b(0xBFF)), sub(b"INFX", 0i32.to_le_bytes().to_vec()), // Missing INFO.
        sub(b"INFC", u32b(0)), sub(b"INFX", 0i32.to_le_bytes().to_vec()), // Absent FormID.
        sub(b"INFX", 0i32.to_le_bytes().to_vec()), // No INFC.
        sub(b"QSTI", u32b(QEQ)),
        sub(b"INFC", u32b(0xB99)), sub(b"INFX", 1i32.to_le_bytes().to_vec()), // Same index as the unscoped pair.
        sub(b"INFC", u32b(0xB94)), sub(b"INFX", 101i32.to_le_bytes().to_vec()), // Duplicate across quests.
    ];
    for unscoped in [false, true] {
        let mut fields = fields.clone();
        if !unscoped {
            fields.drain(1..3);
        }
        let authored = [info(L { id: 0xB90, quest: QA, ..L::default() }), info(L { id: 0xB91, quest: QEQ, ..L::default() })].concat();
        let listing = [record(b"DIAL", 0, 0xB80, &fields), group(0xB80u32.to_le_bytes(), 7, &authored)].concat();
        let source = (0xB94..=0xB99).map(|id| L { id, quest: QA, ..L::default() }).collect();
        let f = shared_order_fixture(listing, source);
        let defs = &f.data.topic_defs[&FormId(0xB80)].shared_infos;
        let expected = if unscoped {
            assert_eq!((defs[0].quest, defs[3].index), (None, None));
            // The whole malformed topic retains its previous topic-wide merge.
            vec![0xB90, 0xB99, 0xB98, 0xB91, 0xB94, 0xB95, 0xB96, 0xB97]
        } else {
            assert_eq!(defs[2].index, None);
            vec![0xB90, 0xB94, 0xB95, 0xB96, 0xB97, 0xB91, 0xB99]
        };
        assert_eq!(f.data.lines_tried(FormId(0xB80)), expected.into_iter().map(FormId).collect::<Vec<_>>());
    }
}

#[test]
fn a_topic_is_offered_after_the_topics_its_lines_link_from() {
    let f = fixture();
    assert_eq!(f.data.linked_from[&FormId(T_LF_FROM)], vec![FormId(T_LF_TO)]);
    // A line with choices: Link From joins them (its own choice cannot be said).
    let mut g = game(&f);
    g.start_dialogue(actor(), Some(FormId(T_LF_FROM))).unwrap();
    listen(&mut g);
    assert_eq!(offered(&g), vec![T_LF_TO]);
    assert_eq!(pick(&mut g, T_LF_TO), FormId(S_LF_TO));
    // A line without choices: the topic list, with the linked topic in it by priority.
    let mut h = game(&f);
    h.start_dialogue(actor(), Some(FormId(T_LF_PLAIN))).unwrap();
    listen(&mut h);
    assert_eq!(offered(&h), vec![T_TOP2, T_TOP, T_LF_TO, T_SKILL, GOODBYE]);
}

/// #3024: SpeechChallengeFailure is not offered as a player choice through Link From
/// (its line links from the topic just said); other Link From topics still are.
#[test]
fn the_speech_challenge_failure_topic_is_not_a_player_choice() {
    let f = fixture();
    assert!(f.data.linked_from[&FormId(T_LF_PLAIN)].contains(&FormId(T_FAIL)));
    let mut g = game(&f);
    g.start_dialogue(actor(), Some(FormId(T_LF_PLAIN))).unwrap();
    listen(&mut g);
    let list = offered(&g);
    assert!(!list.contains(&T_FAIL), "{list:?}");
    assert!(list.contains(&T_LF_TO), "{list:?}");
}

#[test]
fn a_line_whose_choices_cannot_be_said_offers_nothing_else() {
    let f = fixture();
    let mut g = game(&f);
    g.start_dialogue(actor(), Some(FormId(T_DEAD_END))).unwrap();
    listen(&mut g);
    // Not the topic list: the conversation ends.
    assert!(g.dialogue.is_none());
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::DialogueEnded { reason: "no_choices", .. })));
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::ChoicesOffered { .. })));
}

#[test]
fn say_once_lines_are_said_once_per_actor() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_FIRST)));
    assert!(g.end_dialogue());
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_LATER)));
}

/// A Say Once line another actor said is not said again (observer check 31: the Yes Man
/// mainframe did not repeat the Tops Yes Man's first-meeting greeting; #1113).
#[test]
fn say_once_lines_said_by_another_actor_are_not_said_again() {
    let f = fixture();
    let mut g = game(&f);
    assert!(g.can_say(actor(), Some(FormId(0x14)), FormId(A_FIRST)));
    g.said.insert((FormId(0x00FF_0001), FormId(A_FIRST)));
    assert!(!g.can_say(actor(), Some(FormId(0x14)), FormId(A_FIRST)));
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_LATER)));
}

#[test]
fn added_topics_follow_ups_and_the_topic_list() {
    let f = fixture();
    let mut g = game(&f);
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    assert_eq!(pick(&mut g, T_ASK), FormId(B_ASK));
    assert!(g.player.topics.is_empty());
    listen(&mut g);
    // The line's Add Topics list was added when it ended, and its follow-up line was
    // said next without a choice; the follow-up's end script ran.
    assert!(g.player.topics.contains(&FormId(T_ADDED)));
    let lines: Vec<u32> = g.events.iter().filter_map(|e| if let EventKind::LineStarted { info, .. } = e.kind { Some(info.0) } else { None }).collect();
    assert_eq!(lines, vec![A_FIRST, B_ASK, B_FOLLOW]);
    assert_eq!(out(&g), 7.0);
    // The follow-up has no choices: the top-level topics and the added topic, by
    // priority (90, 70, 60, 10), then the actor's GOODBYE since none of them says
    // goodbye.
    assert_eq!(offered(&g), vec![T_TOP2, T_TOP, T_ADDED, T_SKILL, GOODBYE]);
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::TopicAdded { topic, by: Some(by) } if topic == FormId(T_ADDED) && by == FormId(B_ASK))));
}

/// Issue #2308: a line without the Goodbye flag whose end script is `ShowRepairMenu` (Mick's,
/// Old Lady Gibson's, Major Knight's in FalloutNV.esm) leaves the conversation open under
/// the repair services menu: the menu is set for the engine to open, the conversation waits
/// under it, and once the engine closes the menu it goes on to the topic list.
#[test]
fn a_repair_line_without_goodbye_holds_the_conversation_until_the_menu_closes() {
    use fnv_game::repair::{RepairRequest, REPAIR_SERVICES_MENU};
    // `ShowRepairMenu` (0x117A, no parameters) as B_FOLLOW's end script; its subject is the
    // speaker.
    let extra = topic(0x982, "RepairTopic", "Repair", topic_type::TOPIC, 0, 50.0, vec![L {
        id: 0xF82, quest: QA, end_raw: Some(unit(0, &st(0x117A, &le16(0)), &[], &[])), ..L::default()
    }]);
    let mut f = fixture_bytes(&[plugin_bytes(), top(b"DIAL", &extra)].concat());
    let end = f.data.infos[&FormId(0xF82)].end;
    let follow = f.data.infos.get_mut(&FormId(B_FOLLOW)).unwrap();
    assert!(!follow.goodbye);
    follow.end = end;
    let mut g = game(&f);
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    pick(&mut g, T_ASK);
    for _ in 0..20_000 {
        if g.menu.is_some() {
            break;
        }
        g.tick();
    }
    assert_eq!(g.menu, Some(REPAIR_SERVICES_MENU));
    assert_eq!(g.repair_requests, vec![RepairRequest { vendor: actor() }]);
    // Held: the conversation stays open as long as the menu is.
    g.frames(2000);
    assert_eq!((g.dialogue_actor(), g.menu), (Some(actor()), Some(REPAIR_SERVICES_MENU)));
    // The engine closes the menu: the conversation goes on to the topic list.
    assert_eq!(g.close_menu(), Some(REPAIR_SERVICES_MENU));
    listen(&mut g);
    assert_eq!(g.dialogue_actor(), Some(actor()));
    assert_eq!(offered(&g), vec![T_TOP2, T_TOP, T_ADDED, T_SKILL, GOODBYE]);
}

#[test]
fn a_goodbye_line_ends_the_conversation() {
    let f = fixture();
    let mut g = game(&f);
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    // Chosen, the Goodbye line is said in the conversation; when it is over its end
    // script runs and the conversation ends.
    assert_eq!(pick(&mut g, T_BYE), FormId(C_BYE));
    assert!(g.dialogue.is_some() && g.speech.is_empty());
    assert_eq!(call(&mut g, 0x1024, NPC1_REF, &[ArgValue::Value(Value::Num(1009.0))]), 1.0);
    assert_eq!(out(&g), 2.0);
    listen(&mut g);
    assert!(g.dialogue.is_none());
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::DialogueEnded { reason: "goodbye", lines: 2, .. })));
    assert_eq!(call(&mut g, 0x1024, NPC1_REF, &[ArgValue::Value(Value::Num(1009.0))]), 0.0);
    assert_eq!(out(&g), 9.0);
    // The generic GOODBYE of the topic list is said in the conversation too.
    let mut h = at_topic_list(&f);
    assert_eq!(pick(&mut h, GOODBYE), FormId(I_BYE));
    assert!(h.dialogue.is_some() && h.speech.is_empty());
    listen(&mut h);
    assert!(h.dialogue.is_none() && h.speech.is_empty());
    // With the switch it ends the conversation at once and is finished in the world.
    let mut h = at_topic_list(&f);
    h.cfg.goodbye_at_once = true;
    assert_eq!(pick(&mut h, GOODBYE), FormId(I_BYE));
    assert!(h.dialogue.is_none());
    assert!(h.speech.iter().any(|s| s.farewell && s.info == FormId(I_BYE)));
}

/// The fixture with the GOODBYE topic's line as Pronto's in Fallout 3 (#3477): no Goodbye
/// flag.
fn unflagged_goodbye() -> Fixture {
    let mut f = fixture();
    let bye = f.data.infos.get_mut(&FormId(I_BYE)).unwrap();
    bye.goodbye = false;
    bye.flags &= !info_flag::GOODBYE;
    f
}

/// Issue #3477: the GOODBYE topic's line ends the conversation when it is over without
/// the Goodbye flag too, held until then like a flagged one (#2157); its entry counts as a
/// Goodbye (Tab).
#[test]
fn a_goodbye_topics_line_without_the_flag_ends_the_conversation() {
    let f = unflagged_goodbye();
    let mut g = at_topic_list(&f);
    assert!(g.dialogue_choices().iter().all(|c| c.goodbye == (c.topic == FormId(GOODBYE))));
    assert_eq!(pick(&mut g, GOODBYE), FormId(I_BYE));
    g.tick();
    assert!(g.dialogue.as_ref().is_some_and(|c| c.line.as_ref().is_some_and(|l| l.info == FormId(I_BYE))));
    listen(&mut g);
    assert!(g.dialogue.is_none() && g.speech.is_empty());
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::DialogueEnded { reason: "goodbye", lines: 2, .. })));
    // Skipped to its end.
    let mut g = at_topic_list(&f);
    pick(&mut g, GOODBYE);
    assert!(g.skip_response() && g.dialogue.is_none());
    // With the switch it is not moved into the world: it ends as said to the end.
    let mut g = at_topic_list(&f);
    g.cfg.goodbye_at_once = true;
    pick(&mut g, GOODBYE);
    assert!(g.dialogue.is_some() && g.speech.is_empty());
    listen(&mut g);
    assert!(g.dialogue.is_none() && g.speech.is_empty());
}

/// #3477: a player topic whose line names GOODBYE in its Link From is a link of the
/// unflagged GOODBYE line: while the actor can say that line the conversation goes on with
/// it offered. Said by another actor only, of a closed condition, or no player line, it
/// does not hold the conversation open.
#[test]
fn a_goodbye_topics_line_without_the_flag_goes_on_to_a_link_from_the_actor_can_say() {
    const T_LATE: u32 = 0x9E8;
    const S_LATE: u32 = 0xFE8;
    let linked = |ty: u8, speaker: u32, conds: Vec<Subrecord>| {
        let late = topic(T_LATE, "TopicLateThought", "One more thing", ty, 0, 50.0, vec![L { id: S_LATE, quest: QA, ty, speaker: Some(speaker), conds, responses: vec![(1, "Yes?", 0)], link_from: vec![GOODBYE], ..L::default() }]);
        let mut f = fixture_bytes(&[plugin_bytes(), top(b"DIAL", &late)].concat());
        let bye = f.data.infos.get_mut(&FormId(I_BYE)).unwrap();
        bye.goodbye = false;
        bye.flags &= !info_flag::GOODBYE;
        assert_eq!(f.data.linked_from[&FormId(GOODBYE)], vec![FormId(T_LATE)]);
        f
    };
    // The actor's own reply.
    let f = linked(topic_type::TOPIC, NPC1, Vec::new());
    let mut g = at_topic_list(&f);
    assert!(g.dialogue_choices().iter().all(|c| !c.goodbye));
    pick(&mut g, GOODBYE);
    listen(&mut g);
    assert!(offered(&g).contains(&T_LATE), "{:X?}", offered(&g));
    assert_eq!(pick(&mut g, T_LATE), FormId(S_LATE));
    // Nothing the actor can say: another's reply, a condition that fails, a line for
    // conversations between actors.
    for f in [linked(topic_type::TOPIC, NPC2, Vec::new()), linked(topic_type::TOPIC, NPC1, vec![gate(EQ, 77.0)]), linked(topic_type::CONVERSATION, NPC1, Vec::new())] {
        let mut g = at_topic_list(&f);
        assert!(g.dialogue_choices().iter().all(|c| c.goodbye == (c.topic == FormId(GOODBYE))));
        pick(&mut g, GOODBYE);
        listen(&mut g);
        assert!(g.dialogue.is_none());
        assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::DialogueEnded { reason: "goodbye", lines: 2, .. })));
    }
}

/// #3477: an unflagged GOODBYE line with choices or follow-ups of its own leads on, if
/// one can be said.
#[test]
fn a_goodbye_topics_line_without_the_flag_that_leads_on_goes_on() {
    let mut f = unflagged_goodbye();
    f.data.infos.get_mut(&FormId(I_BYE)).unwrap().choices = vec![FormId(T_ASK)];
    let mut g = at_topic_list(&f);
    assert!(g.dialogue_choices().iter().all(|c| !c.goodbye));
    pick(&mut g, GOODBYE);
    listen(&mut g);
    assert_eq!(offered(&g), vec![T_ASK]);
    let mut f = unflagged_goodbye();
    f.data.infos.get_mut(&FormId(I_BYE)).unwrap().follow_ups = vec![FormId(B_FOLLOW)];
    let mut g = at_topic_list(&f);
    pick(&mut g, GOODBYE);
    listen(&mut g);
    assert!(g.dialogue.is_some());
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::LineStarted { info, .. } if info == FormId(B_FOLLOW))));
    // Choices that cannot be said and a Goodbye follow-up, which a line with choices
    // passes over (#964): nothing is left to lead on to.
    let bye = f.data.infos.get_mut(&FormId(I_BYE)).unwrap();
    bye.choices = vec![FormId(T_HIDDEN)];
    bye.follow_ups = vec![FormId(C_BYE)];
    let mut g = at_topic_list(&f);
    assert!(g.dialogue_choices().iter().any(|c| c.goodbye && c.topic == FormId(GOODBYE)));
    pick(&mut g, GOODBYE);
    listen(&mut g);
    assert!(g.dialogue.is_none());
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::DialogueEnded { reason: "goodbye", lines: 2, .. })));
    // A follow-up the actor cannot say: the topic list does not come back.
    f.data.infos.get_mut(&FormId(I_BYE)).unwrap().choices.clear();
    f.data.infos.get_mut(&FormId(I_BYE)).unwrap().follow_ups = vec![FormId(R1)];
    let mut g = at_topic_list(&f);
    pick(&mut g, GOODBYE);
    listen(&mut g);
    assert!(g.dialogue.is_none());
}

/// #3477: the end script of an unflagged GOODBYE line runs before the conversation ends,
/// and the conversation it starts comes after, as for a flagged line (#1107).
#[test]
fn a_goodbye_topics_line_without_the_flag_hands_off_like_a_flagged_one() {
    let mut f = unflagged_goodbye();
    let end = f.data.infos[&FormId(H_HANDOFF)].end;
    f.data.infos.get_mut(&FormId(I_BYE)).unwrap().end = end;
    let mut g = at_topic_list(&f);
    pick(&mut g, GOODBYE);
    listen(&mut g);
    assert_eq!(g.dialogue_actor(), Some(FormId(NPC2_REF)));
    let ended = event_at(&g, |k| matches!(k, EventKind::DialogueEnded { actor, reason: "goodbye", .. } if actor.0 == NPC1_REF));
    let started = event_at(&g, |k| matches!(k, EventKind::DialogueStarted { actor, .. } if actor.0 == NPC2_REF));
    assert!(ended < started, "{ended} {started}");
}

/// Owner's playtest 2026-11-11 (#2157): the last line of a conversation holds the player
/// like the lines before it. A chosen line with the Goodbye flag stays in the
/// conversation, the game paused, and ends it when it is over: end script, Add Topics,
/// then `dialogue_ended`. Nothing of it is said in the world.
#[test]
fn a_chosen_goodbye_line_holds_the_player_until_it_is_said() {
    let mut f = fixture();
    f.data.infos.get_mut(&FormId(C_BYE)).unwrap().add_topics = vec![FormId(T_ADDED)];
    let mut g = game(&f);
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    assert_eq!(pick(&mut g, T_BYE), FormId(C_BYE));
    let t0 = g.time;
    for _ in 0..30 {
        g.tick();
        assert!(g.dialogue.as_ref().is_some_and(|c| c.line.as_ref().is_some_and(|l| l.info == FormId(C_BYE))));
        assert!(g.in_menu() && g.paused() && g.speech.is_empty());
    }
    // Neither the end script nor the Add Topics before the line is over.
    assert_eq!(out(&g), 2.0);
    assert!(!g.player.topics.contains(&FormId(T_ADDED)));
    listen(&mut g);
    assert!(g.dialogue.is_none() && g.speech.is_empty() && g.farewell_said.is_none());
    assert_eq!((out(&g), g.time), (9.0, t0));
    assert!(g.player.topics.contains(&FormId(T_ADDED)));
    let line_end = event_at(&g, |k| matches!(k, EventKind::LineEnded { info, cut: false, .. } if *info == FormId(C_BYE)));
    let added = event_at(&g, |k| matches!(k, EventKind::TopicAdded { by: Some(by), .. } if *by == FormId(C_BYE)));
    let ended = event_at(&g, |k| matches!(k, EventKind::DialogueEnded { reason: "goodbye", lines: 2, .. }));
    assert!(line_end < added && added < ended, "{line_end} {added} {ended}");
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { cut: true, .. })));
}

/// Owner's playtest 2026-11-05 against the original: choosing a Goodbye closes the menu
/// at once and the actor finishes the line as the player walks away (`Game::choose`,
/// `Speech::farewell`). Only with `GameConfig::goodbye_at_once` since #2157.
#[test]
fn a_chosen_goodbye_closes_the_conversation_at_once_and_the_line_goes_on_in_the_world() {
    let mut f = fixture();
    // The line gets Add Topics, and a begin script that counts its runs in TestCount
    // (TestByeB's end script).
    let count = f.data.infos[&FormId(B_BYE)].end;
    let bye = f.data.infos.get_mut(&FormId(C_BYE)).unwrap();
    (bye.add_topics, bye.begin) = (vec![FormId(T_ADDED)], count);
    let mut g = game_at_once(&f);
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    assert_eq!(pick(&mut g, T_BYE), FormId(C_BYE));
    // No conversation, no menu mode: the game goes on while the line is heard.
    assert!(g.dialogue.is_none() && !g.in_menu() && !g.paused());
    let s = g.speech.iter().find(|s| s.speaker == actor()).expect("the line goes on in the world");
    assert!(s.farewell && !s.greeting && !s.say_to && s.info == FormId(C_BYE) && s.target == Some(FormId::PLAYER_REF) && s.conversation.is_none());
    assert_eq!(s.remaining, fnv_game::game::fitted_seconds(8));
    assert!(g.is_talking(actor()));
    // A farewell does not hold its speaker in place, as a Goodbye greeting does.
    assert!(!g.greets_player(actor()));
    // Nothing of the line's start comes again: one line_started.
    let started = g.events.iter().filter(|e| matches!(e.kind, EventKind::LineStarted { info, .. } if info == FormId(C_BYE))).count();
    assert_eq!(started, 1);
    // The end script and the Add Topics wait for the line's end.
    assert_eq!(out(&g), 2.0);
    assert!(!g.player.topics.contains(&FormId(T_ADDED)));
    hear_out(&mut g);
    assert_eq!(out(&g), 9.0);
    assert!(g.player.topics.contains(&FormId(T_ADDED)));
    // The conversation ended before the line did, and the line was not cut.
    let ended = event_at(&g, |k| matches!(k, EventKind::DialogueEnded { reason: "goodbye", lines: 2, .. }));
    let line_end = event_at(&g, |k| matches!(k, EventKind::LineEnded { info, cut: false, .. } if *info == FormId(C_BYE)));
    let added = event_at(&g, |k| matches!(k, EventKind::TopicAdded { by: Some(by), .. } if *by == FormId(C_BYE)));
    assert!(ended < line_end && line_end < added, "{ended} {line_end} {added}");
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { cut: true, .. })));
    assert!(g.dialogue.is_none() && g.speech.is_empty());
    // The line started once and its response was said once: nothing of its start came
    // again with the transfer, and its begin script ran once.
    let starts = g.events.iter().filter(|e| matches!(e.kind, EventKind::LineStarted { info, .. } if info == FormId(C_BYE))).count();
    let said = g.events.iter().filter(|e| matches!(e.kind, EventKind::ResponseSaid { info, .. } if info == FormId(C_BYE))).count();
    assert_eq!((starts, said), (1, 1));
    assert_eq!(g.world.global(FormId(G_COUNT)).unwrap(), 1.0);
    // The engine's log line takes the transfer as the conversation closed.
    assert_eq!(g.farewell_said, Some((actor(), FormId(C_BYE))));
}

/// Issue #1606 review: a farewell that ends inside another farewell's end script keeps
/// to its own state. A's end script asks for ChatterREF's force greet, then makes
/// ChatterREF say a line, which ends ChatterREF's own farewell (B) as said; A's force
/// greet still starts, after the rest of A's end script and A's Add Topics, and B's end
/// script runs once.
#[test]
fn a_farewell_ending_inside_another_farewells_end_script_keeps_the_outer_hand_off() {
    let mut f = fixture();
    f.data.infos.get_mut(&FormId(S_DEAD_END)).unwrap().choices = vec![FormId(T_RELAY)];
    let mut g = game_at_once(&f);
    g.start_dialogue(actor(), Some(FormId(T_DEAD_END))).unwrap();
    listen(&mut g);
    assert_eq!(pick(&mut g, T_RELAY), FormId(H_RELAY));
    assert!(g.dialogue.is_none());
    // ChatterREF is saying its own farewell meanwhile, ending well after A's.
    let b = FormId(NPC2_REF);
    g.speech.push(fnv_game::game::Speech {
        speaker: b,
        target: Some(FormId::PLAYER_REF),
        topic: FormId(T_BYE_B),
        info: FormId(B_BYE),
        remaining: 100.0,
        say_to: false,
        greeting: false,
        remote: false,
        farewell: true,
        conversation: None,
        earlier_topics: Vec::new(),
        hello: false,
        look: fnv_game::game::LineLook::Both,
        response: 0,
        force_subtitles: false,
        two_d: false,
        end_ran: false,
        leads_on: false,
        through: None,
    });
    for _ in 0..20_000 {
        if !g.speech.iter().any(|s| s.farewell && s.speaker == actor()) {
            break;
        }
        g.tick();
    }
    // B ended as said inside A's end script, once.
    assert_eq!(g.world.global(FormId(G_COUNT)).unwrap(), 1.0);
    let b_ends = g.events.iter().filter(|e| matches!(e.kind, EventKind::LineEnded { info, cut: false, .. } if info == FormId(B_BYE))).count();
    assert_eq!(b_ends, 1);
    // A's hand-off started after A's whole end script and A's Add Topics.
    assert_eq!(g.dialogue_actor(), Some(b));
    assert_eq!(out(&g), 5.0);
    let a_end = event_at(&g, |k| matches!(k, EventKind::LineEnded { info, cut: false, .. } if *info == FormId(H_RELAY)));
    let b_end = event_at(&g, |k| matches!(k, EventKind::LineEnded { info, cut: false, .. } if *info == FormId(B_BYE)));
    let added = event_at(&g, |k| matches!(k, EventKind::TopicAdded { by: Some(by), .. } if *by == FormId(H_RELAY)));
    let next = event_at(&g, |k| matches!(k, EventKind::DialogueStarted { actor, .. } if *actor == b));
    assert!(a_end < b_end && b_end < added && added < next, "{a_end} {b_end} {added} {next}");
    let starts = g.events.iter().filter(|e| matches!(e.kind, EventKind::DialogueStarted { .. })).count();
    assert_eq!(starts, 2, "the first conversation and A's hand-off, nothing more");
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::CallFailed { .. })));
}

/// Issue #1606 review: a farewell that ends inside a conversation line's end script
/// keeps its force greet in its own slot. TalkerREF's Goodbye line, opening a
/// conversation by its topic, asks in its end script for ChatterREF's force greet (X),
/// then makes ChatterREF say a line, which ends ChatterREF's farewell; that farewell's
/// end script asks for TalkerREF's (Y). Y is tried after the farewell's Add Topics and
/// refused, the conversation still being there; X starts once the conversation closes.
#[test]
fn a_farewell_ending_inside_a_conversation_lines_end_script_leaves_the_conversations_hand_off() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(g.start_dialogue(actor(), Some(FormId(T_RELAY))), Ok(FormId(H_RELAY)));
    let b = FormId(NPC2_REF);
    g.speech.push(fnv_game::game::Speech {
        speaker: b,
        target: Some(FormId::PLAYER_REF),
        topic: FormId(T_B_HAND),
        info: FormId(B_HAND),
        remaining: 100.0,
        say_to: false,
        greeting: false,
        remote: false,
        farewell: true,
        conversation: None,
        earlier_topics: Vec::new(),
        hello: false,
        look: fnv_game::game::LineLook::Both,
        response: 0,
        force_subtitles: false,
        two_d: false,
        end_ran: false,
        leads_on: false,
        through: None,
    });
    listen(&mut g);
    // ChatterREF's farewell ended as said inside the conversation's end script, its Add
    // Topics before the conversation closed.
    let b_end = event_at(&g, |k| matches!(k, EventKind::LineEnded { info, cut: false, .. } if *info == FormId(B_HAND)));
    let b_added = event_at(&g, |k| matches!(k, EventKind::TopicAdded { by: Some(by), .. } if *by == FormId(B_HAND)));
    let closed = event_at(&g, |k| matches!(k, EventKind::DialogueEnded { reason: "goodbye", .. }));
    assert!(b_end < b_added && b_added < closed, "{b_end} {b_added} {closed}");
    // The rest of the conversation's end script ran; X, not Y, started after it closed.
    assert_eq!(out(&g), 5.0);
    assert_eq!(g.dialogue_actor(), Some(b));
    let started: Vec<u32> = g.events.iter().filter_map(|e| if let EventKind::DialogueStarted { actor, .. } = e.kind { Some(actor.0) } else { None }).collect();
    assert_eq!(started, vec![NPC1_REF, NPC2_REF]);
    let c = g.dialogue.as_ref().unwrap();
    assert!(c.then.is_none() && !c.ending);
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::CallFailed { .. })));
}

/// Issue #1606 review: a chosen Goodbye line whose begin script makes its speaker say
/// another line stays in the conversation until it is over, as before, and the other
/// line is not cut for it.
#[test]
fn a_goodbye_whose_begin_script_makes_its_speaker_say_a_line_stays_in_the_conversation() {
    let mut f = fixture();
    f.data.infos.get_mut(&FormId(S_DEAD_END)).unwrap().choices = vec![FormId(T_BEGIN_SAY)];
    let mut g = game(&f);
    g.start_dialogue(actor(), Some(FormId(T_DEAD_END))).unwrap();
    listen(&mut g);
    assert_eq!(pick(&mut g, T_BEGIN_SAY), FormId(H_BEGIN_SAY));
    assert!(g.dialogue.as_ref().is_some_and(|c| c.line.as_ref().is_some_and(|l| l.info == FormId(H_BEGIN_SAY))));
    assert_eq!(g.speech.iter().map(|s| (s.info.0, s.farewell)).collect::<Vec<_>>(), vec![(B_ASK, false)]);
    assert!(g.farewell_said.is_none());
    listen(&mut g);
    assert!(g.dialogue.is_none() && out(&g) == 9.0);
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::DialogueEnded { reason: "goodbye", .. })));
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { info, cut: true, .. } if info == FormId(B_ASK))));
}

/// A farewell of two responses lasts as long as both together, as the line would have
/// in the conversation.
#[test]
fn a_two_response_farewell_lasts_as_long_as_both_responses() {
    let mut f = fixture();
    let first = f.data.infos[&FormId(A_FIRST)].clone();
    let bye = f.data.infos.get_mut(&FormId(C_BYE)).unwrap();
    (bye.responses, bye.response_chars) = (first.responses, first.response_chars);
    let mut g = game_at_once(&f);
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    pick(&mut g, T_BYE);
    let s = g.speech.iter().find(|s| s.farewell).unwrap();
    let whole = fnv_game::game::fitted_seconds(5) + fnv_game::game::fitted_seconds(7);
    assert!((s.remaining - whole).abs() < 1e-9, "{} for {whole}", s.remaining);
    assert_eq!(s.response, 0);
    let f0 = g.frame;
    hear_out(&mut g);
    assert!(((g.frame - f0) as f64 - whole * 60.0).abs() <= 2.0, "{} frames for {}", g.frame - f0, whole * 60.0);
    assert_eq!(out(&g), 9.0);
}

/// With Run Immediately the Goodbye line's end script ran when it was picked; it does
/// not run again when the farewell ends.
#[test]
fn a_run_immediately_farewell_runs_its_end_script_once() {
    let mut f = fixture();
    f.data.infos.get_mut(&FormId(C_BYE)).unwrap().flags |= info_flag::RUN_IMMEDIATELY;
    let mut g = game_at_once(&f);
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    pick(&mut g, T_BYE);
    assert_eq!(out(&g), 9.0);
    assert!(g.speech.iter().any(|s| s.farewell && s.end_ran));
    set_global(&mut g, G_OUT, 0.0);
    hear_out(&mut g);
    assert_eq!(out(&g), 0.0);
    let ends = g.events.iter().filter(|e| matches!(e.kind, EventKind::LineEnded { info, cut: false, .. } if info == FormId(C_BYE))).count();
    assert_eq!(ends, 1);
}

/// Issue #1107 with a chosen Goodbye line that closes at once: its end script's `StartConversation player`
/// starts the next conversation when the farewell ends, after its end script and its
/// Add Topics (`Game::hold_force_greet`).
#[test]
fn a_chosen_goodbyes_end_script_starts_the_next_conversation_after_its_add_topics() {
    let mut f = fixture();
    f.data.infos.get_mut(&FormId(S_DEAD_END)).unwrap().choices = vec![FormId(T_HANDOFF)];
    f.data.infos.get_mut(&FormId(H_HANDOFF)).unwrap().add_topics = vec![FormId(T_ADDED)];
    let mut g = game_at_once(&f);
    g.start_dialogue(actor(), Some(FormId(T_DEAD_END))).unwrap();
    listen(&mut g);
    assert_eq!(pick(&mut g, T_HANDOFF), FormId(H_HANDOFF));
    assert!(g.dialogue.is_none());
    hear_out(&mut g);
    assert_eq!(g.dialogue_actor(), Some(FormId(NPC2_REF)));
    let ended = event_at(&g, |k| matches!(k, EventKind::DialogueEnded { actor: a, reason: "goodbye", .. } if *a == actor()));
    let line_end = event_at(&g, |k| matches!(k, EventKind::LineEnded { info, cut: false, .. } if *info == FormId(H_HANDOFF)));
    let added = event_at(&g, |k| matches!(k, EventKind::TopicAdded { by: Some(by), .. } if *by == FormId(H_HANDOFF)));
    let next = event_at(&g, |k| matches!(k, EventKind::DialogueStarted { actor, .. } if *actor == FormId(NPC2_REF)));
    assert!(ended < line_end && line_end < added && added < next, "{ended} {line_end} {added} {next}");
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::CallFailed { .. })));
    let c = g.dialogue.as_ref().unwrap();
    assert!(c.then.is_none() && !c.ending);
}

/// A farewell is never cut: another line of its speaker ends it as said, end script
/// included; so does its speaker going away.
#[test]
fn a_farewell_cut_by_another_line_of_its_speaker_still_runs_its_end_script() {
    let f = fixture();
    let mut g = game_at_once(&f);
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    pick(&mut g, T_BYE);
    g.frames(3);
    assert_eq!(out(&g), 2.0);
    assert_eq!(g.say(actor(), Some(FormId::PLAYER_REF), FormId(T_ASK), true), Some(FormId(B_ASK)));
    assert_eq!(out(&g), 9.0);
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { info, cut: false, .. } if info == FormId(C_BYE))));
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { cut: true, .. })));
    assert_eq!(g.speech.iter().map(|s| (s.info.0, s.farewell)).collect::<Vec<_>>(), vec![(B_ASK, false)]);
    // Its speaker disabled mid-line: the same.
    let mut h = game_at_once(&f);
    h.start_dialogue(actor(), None).unwrap();
    listen(&mut h);
    pick(&mut h, T_BYE);
    h.frames(3);
    h.world.set_enabled(actor(), false).unwrap();
    h.frames(1);
    assert!(h.speech.is_empty() && out(&h) == 9.0);
    assert!(!h.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { cut: true, .. })));
}

/// A save made while the farewell is heard keeps it; its end script runs after the load.
#[test]
fn a_farewell_survives_a_snapshot_and_runs_its_end_script_after_the_load() {
    let f = fixture();
    let mut g = game_at_once(&f);
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    pick(&mut g, T_BYE);
    g.frames(5);
    let snap = g.snapshot();
    let text = serde_json::to_string(&snap).unwrap();
    assert!(text.contains(r#""farewell":true"#), "{text}");
    let back: GameSnapshot = serde_json::from_str(&text).unwrap();
    assert_eq!(back, snap);
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert!(h.dialogue.is_none());
    assert!(h.speech.iter().any(|s| s.farewell && s.info == FormId(C_BYE)));
    assert_eq!(out(&h), 2.0);
    hear_out(&mut h);
    assert_eq!(out(&h), 9.0);
    hear_out(&mut g);
    assert_eq!(h.frame, g.frame);
}

/// A conversation through a talking activator keeps a chosen Goodbye line in the
/// conversation until it is over, as before (see `Game::choose`). The fixture
/// has no talking activator: the conversation is given a present reference to go
/// through.
#[test]
fn a_goodbye_chosen_through_a_talking_activator_stays_in_the_conversation() {
    let f = fixture();
    let mut g = game(&f);
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    g.dialogue.as_mut().unwrap().through = Some(FormId(PATROL_REF));
    pick(&mut g, T_BYE);
    assert!(g.dialogue.as_ref().is_some_and(|c| c.line.as_ref().is_some_and(|l| l.info == FormId(C_BYE))));
    assert!(g.speech.is_empty());
    listen(&mut g);
    assert!(g.dialogue.is_none() && out(&g) == 9.0);
    let line_end = event_at(&g, |k| matches!(k, EventKind::LineEnded { info, cut: false, .. } if *info == FormId(C_BYE)));
    let ended = event_at(&g, |k| matches!(k, EventKind::DialogueEnded { reason: "goodbye", .. }));
    assert!(line_end < ended);
}

/// Issue #1107: `StartConversation player` in the end script of a Goodbye line starts
/// the new conversation once the old one has closed (`Game::hold_force_greet`).
#[test]
fn a_goodbye_lines_end_script_starts_the_next_conversation_after_it_closes() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(g.start_dialogue(actor(), Some(FormId(T_HANDOFF))), Ok(FormId(H_HANDOFF)));
    listen(&mut g);
    assert_eq!(g.dialogue_actor(), Some(FormId(NPC2_REF)));
    // The old conversation ended (Goodbye) before the new one started, with the second
    // actor's greeting.
    let order: Vec<(&str, u32)> = g
        .events
        .iter()
        .filter_map(|e| match e.kind {
            EventKind::DialogueEnded { actor, .. } => Some(("ended", actor.0)),
            EventKind::DialogueStarted { actor, .. } => Some(("started", actor.0)),
            _ => None,
        })
        .collect();
    assert_eq!(order, vec![("started", NPC1_REF), ("ended", NPC1_REF), ("started", NPC2_REF)]);
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::DialogueEnded { reason: "goodbye", .. })));
    let c = g.dialogue.as_ref().unwrap();
    assert!(c.then.is_none() && !c.ending);
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::CallFailed { .. })));
}

/// When the line does not close the conversation the force greet is refused, as any
/// StartConversation while a conversation runs, and does not come back later.
#[test]
fn a_force_greet_from_a_line_that_goes_on_is_refused() {
    let f = fixture();
    let mut g = game(&f);
    g.start_dialogue(actor(), Some(FormId(T_HANDOFF_STAY))).unwrap();
    listen(&mut g);
    assert_eq!(g.dialogue_actor(), Some(actor()));
    assert_eq!(offered(&g), vec![T_BYE]);
    assert!(g.dialogue.as_ref().unwrap().then.is_none());
    pick(&mut g, T_BYE);
    listen(&mut g);
    assert!(g.dialogue.is_none());
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::DialogueStarted { actor, .. } if actor == FormId(NPC2_REF))));
}

/// Issue #964: a line with both choices and a Goodbye follow-up offers its choices (the
/// follow-up would leave them unreachable); a follow-up that is no Goodbye line is said
/// first, as before, and offers its own choices.
#[test]
fn a_lines_choices_pass_over_a_goodbye_follow_up() {
    let f = fixture();
    let started = |g: &Game| -> Vec<u32> { g.events.iter().filter_map(|e| if let EventKind::LineStarted { info, .. } = e.kind { Some(info.0) } else { None }).collect() };
    let mut g = game(&f);
    g.start_dialogue(actor(), Some(FormId(T_BOTH))).unwrap();
    listen(&mut g);
    assert_eq!(started(&g), vec![W_BOTH]);
    assert_eq!(offered(&g), vec![T_ASK, T_BYE]);
    let mut h = game(&f);
    h.start_dialogue(actor(), Some(FormId(T_KEEP))).unwrap();
    listen(&mut h);
    assert_eq!(started(&h), vec![W_KEEP, W_BOTH_FU]);
    assert_eq!(offered(&h), vec![T_BYE]);
}

/// A fight breaking out while an actor says its Goodbye greeting at the player ends the
/// line at once as if said to the end: its end script runs, its Add Topics apply, and the
/// actor is no longer held for it (`Game::greets_player`).
#[test]
fn a_goodbye_greeting_ends_as_said_when_its_speaker_fights() {
    let f = fixture();
    let mut g = game(&f);
    set_global(&mut g, G_GATE, 7.0);
    g.frames(3);
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_BARK)));
    g.frames(2);
    assert!(g.greets_player(actor()) && out(&g) == 3.0);
    g.start_combat(actor(), FormId::PLAYER_REF);
    assert!(!g.greets_player(actor()));
    g.frames(1);
    assert!(g.speech.is_empty());
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { info, cut: false, .. } if info == FormId(A_BARK))));
    assert_eq!(out(&g), 4.0);
    assert!(g.player.topics.contains(&FormId(T_ADDED)));
    // Its speaker disabled mid-line: the same, the line ends as said.
    let mut h = game(&f);
    set_global(&mut h, G_GATE, 7.0);
    h.frames(3);
    assert_eq!(h.start_dialogue(actor(), None), Ok(FormId(A_BARK)));
    h.frames(2);
    h.world.set_enabled(actor(), false).unwrap();
    h.frames(1);
    assert!(h.speech.is_empty() && out(&h) == 4.0 && h.player.topics.contains(&FormId(T_ADDED)));
    assert!(!h.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { cut: true, .. })));
}

/// Issue #3663: a Goodbye greeting started through the player's activation while its
/// speaker is already searching (`State::Alert` or `State::Lost`) must play out rather
/// than ending on the same frame, while entering a fight (`State::Combat`) mid-line still
/// ends it at once with its end script and Add Topics.
#[test]
fn a_goodbye_greeting_while_searching_plays_out_and_still_ends_on_combat() {
    use fnv_game::awareness::{Awareness, State};
    use fnv_sim::world::Outcome;

    let f = fixture();
    // 1. Already searching (Alert, then Lost): player activation starts the Goodbye
    //    greeting and it plays out across frames before running its end script.
    for state in [State::Alert, State::Lost] {
        let mut g = game(&f);
        set_global(&mut g, G_GATE, 7.0);
        g.frames(3);
        g.awareness.actors.insert(actor(), Awareness { state, ..Default::default() });
        assert_eq!(g.awareness.state(actor()), state);

        let hand = g.default_activation_by_hand(actor()).unwrap().expect("player activates searching actor");
        assert_eq!(hand.activation.outcome, Outcome::Talk { actor: actor() });
        assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_BARK)));
        // Must NOT end in the same frame or after a few ticks while still searching.
        g.frames(5);
        assert_eq!(g.speech.len(), 1, "{state:?}: Goodbye greeting must stay active while searching");
        assert!(g.greets_player(actor()), "{state:?}: searching speaker pauses for its Goodbye greeting");
        assert_eq!(out(&g), 3.0, "{state:?}: begin script ran, end script must wait for line end");
        assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { info, .. } if info == FormId(A_BARK))));

        // Plays out to completion and then runs its end script and Add Topics.
        let f0 = g.frame;
        for _ in 0..20_000 {
            if g.speech.is_empty() {
                break;
            }
            g.tick();
        }
        assert!(g.speech.is_empty() && g.frame - f0 > 30, "{state:?}: line must play out over time");
        assert_eq!(out(&g), 4.0, "{state:?}: end script runs when line finishes");
        assert!(g.player.topics.contains(&FormId(T_ADDED)));
        assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { info, cut: false, .. } if info == FormId(A_BARK))));
    }

    // 2. Starting while searching (Alert), then drawn into a fight (Combat) mid-line:
    //    ends at once as if said to the end, running its end script and Add Topics.
    let mut g = game(&f);
    set_global(&mut g, G_GATE, 7.0);
    g.frames(3);
    g.awareness.actors.insert(actor(), Awareness { state: State::Alert, ..Default::default() });
    let hand = g.default_activation_by_hand(actor()).unwrap().expect("player activates Alert actor");
    assert_eq!(hand.activation.outcome, Outcome::Talk { actor: actor() });
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_BARK)));
    g.frames(2);
    assert_eq!(g.speech.len(), 1);
    assert!(g.greets_player(actor()) && out(&g) == 3.0);

    g.start_combat(actor(), FormId::PLAYER_REF);
    assert_eq!(g.awareness.state(actor()), State::Combat);
    assert!(!g.greets_player(actor()));
    g.frames(1);
    assert!(g.speech.is_empty());
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { info, cut: false, .. } if info == FormId(A_BARK))));
    assert_eq!(out(&g), 4.0);
    assert!(g.player.topics.contains(&FormId(T_ADDED)));
}

#[test]
fn a_goodbye_greeting_is_said_at_the_player_without_a_conversation() {
    let f = fixture();
    let mut g = game(&f);
    set_global(&mut g, G_GATE, 7.0);
    let ticks = |g: &Game| (g.world.global(FormId(G_TICKS)).unwrap(), g.world.global(FormId(G_MENU_TICKS)).unwrap());
    g.frames(3);
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_BARK)));
    // No conversation: no menu mode, no pause, nothing to choose or to leave.
    assert!(g.dialogue.is_none() && !g.paused() && !g.in_menu() && !g.menu_open(1009));
    assert!(g.dialogue_choices().is_empty() && !g.end_dialogue());
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::DialogueStarted { .. })));
    assert_eq!(call(&mut g, 0x1024, NPC1_REF, &[ArgValue::Value(Value::Num(1009.0))]), 0.0);
    // The actor says the line at the player; its begin script ran.
    assert!(matches!(g.speech.as_slice(), [s] if s.speaker == actor() && s.target == Some(FormId::PLAYER_REF) && s.info == FormId(A_BARK) && s.greeting));
    // The engine holds it, turned to the player, while it says it.
    assert!(g.greets_player(actor()));
    assert_eq!((call(&mut g, 0x108D, NPC1_REF, &[]), call(&mut g, 0x1032, NPC1_REF, &[])), (1.0, 1.0));
    assert_eq!(out(&g), 3.0);
    // The game goes on while it is heard: GameMode blocks run, game time passes.
    let (t0, f0) = (g.time, g.frame);
    for _ in 0..20_000 {
        if g.speech.is_empty() {
            break;
        }
        g.tick();
    }
    let heard = (g.frame - f0) as f32;
    assert!(g.speech.is_empty() && heard > 30.0 && !g.greets_player(actor()));
    assert_eq!(ticks(&g), (3.0 + heard, 0.0));
    assert!(g.time > t0);
    // Its end: the end script and the Add Topics list, and no conversation event.
    assert_eq!(out(&g), 4.0);
    assert!(g.player.topics.contains(&FormId(T_ADDED)));
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { info, cut: false, .. } if info == FormId(A_BARK))));
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::DialogueEnded { .. })));
    // Talked to again while it speaks: the line ends as if said (a Goodbye greeting at
    // the player is never cut, `Game::cut_line`) and the greeting starts afresh.
    g.start_dialogue(actor(), None).unwrap();
    g.frames(2);
    let said = |g: &Game| g.events.iter().filter(|e| matches!(e.kind, EventKind::LineEnded { info, cut: false, .. } if info == FormId(A_BARK))).count();
    let before = said(&g);
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_BARK)));
    assert_eq!(g.speech.len(), 1);
    assert_eq!(said(&g), before + 1);
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { info, cut: true, .. } if info == FormId(A_BARK))));
    // StartConversation without a topic is the same greeting, but the actor starts it:
    // a force greet holds the player in a conversation until the line is over, and the
    // Goodbye flag ends it then (`Game::force_greet`).
    let mut h = game(&f);
    set_global(&mut h, G_GATE, 7.0);
    call(&mut h, 0x1056, NPC1_REF, &[ArgValue::Value(Value::Ref(FormId::PLAYER_REF.0))]);
    assert!(h.dialogue.as_ref().is_some_and(|c| c.line.as_ref().is_some_and(|l| l.info == FormId(A_BARK))) && h.speech.is_empty());
    assert!(h.in_menu() && h.menu_open(1009));
    for _ in 0..20_000 {
        if h.dialogue.is_none() {
            break;
        }
        h.tick();
    }
    assert!(h.dialogue.is_none());
    assert!(h.events.iter().any(|e| matches!(e.kind, EventKind::DialogueEnded { reason: "goodbye", lines: 1, .. })));
    assert_eq!(out(&h), 4.0);
    // So does a force greet by a Dialogue package (`fnv_ai`).
    let mut m = game(&f);
    set_global(&mut m, G_GATE, 7.0);
    assert_eq!(m.force_greet(actor(), None), Ok(FormId(A_BARK)));
    assert!(m.dialogue.is_some() && m.speech.is_empty());
    // A greeting without the flag is a conversation as before.
    let mut k = game(&f);
    k.start_dialogue(actor(), None).unwrap();
    assert!(k.dialogue.is_some() && k.speech.is_empty());
}

#[test]
fn leaving_cuts_the_line_without_its_end_script() {
    let f = fixture();
    let mut g = game(&f);
    g.start_dialogue(actor(), None).unwrap();
    g.frames(3);
    assert!(g.end_dialogue());
    assert!(!g.end_dialogue());
    g.frames(600);
    assert_eq!(out(&g), 1.0);
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { cut: true, .. })));
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::DialogueEnded { reason: "left", .. })));
    // An actor that is gone ends it as well.
    let mut h = game(&f);
    h.start_dialogue(actor(), None).unwrap();
    h.extra.entry(actor()).or_default().dead = true;
    h.tick();
    assert!(h.dialogue.is_none());
    assert!(h.start_dialogue(actor(), None).is_err());
}

#[test]
fn skipping_a_response_ends_it_early_and_the_line_still_runs_its_end_script() {
    let f = fixture();
    let mut g = game(&f);
    // The first greeting has two responses ("First" then "Second.") and both scripts.
    g.start_dialogue(actor(), None).unwrap();
    g.frames(3);
    assert_eq!((out(&g), g.dialogue_emotion(actor())), (1.0, 3));
    // Skipping the first response starts the second with its full length.
    assert!(g.skip_response());
    let line = g.dialogue.as_ref().and_then(|c| c.line.clone()).unwrap();
    assert_eq!((line.response, line.remaining), (1, fnv_game::game::fitted_seconds(7)));
    assert_eq!((out(&g), g.dialogue_emotion(actor())), (1.0, 5));
    // Skipping the last one ends the line as if it had been said: the end script ran,
    // the line is not cut and its choices are offered, with no frame in between.
    let frame = g.frame;
    assert!(g.skip_response());
    assert_eq!((g.frame, out(&g)), (frame, 2.0));
    assert_eq!(offered(&g), vec![T_ASK, T_BYE]);
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { cut: true, .. })));
    let said: Vec<u8> = g.events.iter().filter_map(|e| if let EventKind::ResponseSaid { number, .. } = e.kind { Some(number) } else { None }).collect();
    assert_eq!(said, vec![1, 2]);
    // Nothing to skip while the player chooses.
    assert!(!g.skip_response());
    // A chosen Goodbye line is said in the conversation and skipped like any other:
    // its end script runs and the conversation ends, the line not cut.
    pick(&mut g, T_BYE);
    assert!(g.dialogue.is_some());
    assert_eq!(out(&g), 2.0);
    assert!(g.skip_response());
    assert!(g.dialogue.is_none());
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::DialogueEnded { reason: "goodbye", lines: 2, .. })));
    assert_eq!(out(&g), 9.0);
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { cut: true, .. })));
    assert!(!g.skip_response());
    // A menu holds the conversation: nothing is skipped under it.
    let mut h = game(&f);
    h.start_dialogue(actor(), None).unwrap();
    h.menu = Some(1001);
    assert!(!h.skip_response());
    assert_eq!(h.dialogue.as_ref().and_then(|c| c.line.as_ref()).map(|l| l.response), Some(0));
}

#[test]
fn random_lines_form_a_stack_from_which_one_is_picked() {
    let f = fixture();
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..24 {
        let mut g = game_seeded(&f, seed);
        let line = g.start_dialogue(FormId(NPC2_REF), None).unwrap();
        // The line after the Random End is never reached.
        assert!(line == FormId(R1) || line == FormId(R2), "{line}");
        seen.insert(line);
    }
    assert_eq!(seen.len(), 2);
}

#[test]
fn run_immediately_runs_both_results_when_the_line_is_picked() {
    let f = ordered_fixture(E_NOW, None);
    let mut g = at_topic_list(&f);
    set_global(&mut g, G_OUT, 0.0);
    assert_eq!(pick(&mut g, T_TOP2), FormId(E_NOW));
    assert_eq!(out(&g), 12.0);
    set_global(&mut g, G_OUT, 0.0);
    listen(&mut g);
    assert_eq!(out(&g), 0.0);
    // The entry is now shaded: the actor has said that line.
    let c = g.dialogue_choices().iter().find(|c| c.topic == FormId(T_TOP2)).unwrap();
    assert!(c.darkened);
    assert!(!g.dialogue_choices().iter().find(|c| c.topic == FormId(T_SKILL)).unwrap().darkened);
}

#[test]
fn say_once_a_day_lines_come_back_after_the_timer() {
    let f = fixture();
    let mut g = at_topic_list(&f);
    assert_eq!(pick(&mut g, T_TOP), FormId(E_DAY));
    listen(&mut g);
    assert!(!offered(&g).contains(&T_TOP));
    // 11 game hours later it is still held back, 12 hours later it is offered again.
    set_global(&mut g, G_DAYS, 11.0 / 24.0);
    pick(&mut g, T_TOP2);
    listen(&mut g);
    assert!(!offered(&g).contains(&T_TOP));
    set_global(&mut g, G_DAYS, 0.5);
    pick(&mut g, T_TOP2);
    listen(&mut g);
    assert!(offered(&g).contains(&T_TOP));
    // Another actor's count is its own.
    assert!(g.said_day[&actor()].contains_key(&FormId(E_DAY)));
}

/// At the topic list, `hours` game hours after the fixture's 08:00, the actor says the Say
/// Once a Day line.
fn daily_line_said_after(f: &Fixture, hours: f64) -> Game<'_> {
    let mut g = at_topic_list(f);
    g.world.advance_hours(hours).unwrap();
    assert!(daily_line_free(&mut g));
    assert_eq!(pick(&mut g, T_TOP), FormId(E_DAY));
    listen(&mut g);
    g
}

fn daily_line_free(g: &mut Game) -> bool {
    g.can_say(actor(), Some(FormId::PLAYER_REF), FormId(E_DAY))
}

fn game_hour(g: &Game) -> f32 {
    g.world.global(FormId(0x802)).unwrap()
}

/// Issue #1579: every hour through the clock counts once, whatever the time of day. On a
/// clock adding GameHour to GameDaysPassed × 24, the line came back at 16:00.
#[test]
fn a_daily_line_said_in_the_morning_is_held_twelve_hours_of_the_clock() {
    let f = fixture();
    let mut g = daily_line_said_after(&f, 2.0);
    assert!((game_hour(&g) - 10.0).abs() < 0.01);
    g.world.advance_hours(6.0).unwrap();
    assert!((game_hour(&g) - 16.0).abs() < 0.01);
    assert!(!daily_line_free(&mut g), "six hours on");
    g.world.advance_hours(5.99).unwrap();
    assert!(!daily_line_free(&mut g), "just under twelve hours on");
    g.world.advance_hours(0.02).unwrap();
    assert!((game_hour(&g) - 22.01).abs() < 0.01);
    assert!(daily_line_free(&mut g), "just over twelve hours on");
}

/// Issue #1579: midnight takes nothing off the clock. On a clock adding GameHour, the line
/// said at 23:00 was held until 17:00.
#[test]
fn a_daily_line_said_before_midnight_is_held_twelve_hours_of_the_clock() {
    let f = fixture();
    let mut g = daily_line_said_after(&f, 15.0);
    assert!((game_hour(&g) - 23.0).abs() < 0.01);
    g.world.advance_hours(2.0).unwrap();
    assert!((game_hour(&g) - 1.0).abs() < 0.01);
    assert!(!daily_line_free(&mut g), "two hours on, past midnight");
    g.world.advance_hours(9.99).unwrap();
    assert!(!daily_line_free(&mut g), "just under twelve hours on");
    g.world.advance_hours(0.02).unwrap();
    assert!((game_hour(&g) - 11.01).abs() < 0.01);
    assert!(daily_line_free(&mut g), "just over twelve hours on");
    // The topic list says the same.
    pick(&mut g, T_TOP2);
    listen(&mut g);
    assert!(offered(&g).contains(&T_TOP));
}

#[test]
fn a_save_keeps_a_daily_line_held_on_the_same_clock() {
    let f = fixture();
    let mut g = daily_line_said_after(&f, 2.0);
    g.world.advance_hours(6.0).unwrap();
    let snap = g.snapshot();
    assert!(snap.said_day_days_clock);
    let back: GameSnapshot = serde_json::from_str(&serde_json::to_string(&snap).unwrap()).unwrap();
    assert_eq!(back, snap);
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert_eq!(h.said_day, g.said_day);
    assert!(!daily_line_free(&mut h));
    h.world.advance_hours(5.99).unwrap();
    assert!(!daily_line_free(&mut h), "just under twelve hours from when it was said");
    h.world.advance_hours(0.02).unwrap();
    assert!(daily_line_free(&mut h));
}

/// A save from before #1579 holds hours of the old clock, which cannot be converted: its
/// Say Once a Day holds end at the load, once, and its Say Once history stays.
#[test]
fn a_save_of_the_old_clock_drops_daily_holds_and_keeps_say_once() {
    let f = fixture();
    let g = daily_line_said_after(&f, 2.0);
    let text = serde_json::to_string(&g.snapshot()).unwrap();
    let older = text.replacen("\"said_day_days_clock\":true,", "", 1);
    assert_ne!(older, text);
    let back: GameSnapshot = serde_json::from_str(&older).unwrap();
    assert!(!back.said_day_days_clock && !back.said_day.is_empty());
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert!(h.said_day.is_empty());
    assert!(!h.said.is_empty());
    assert_eq!(h.said, g.said);
    assert!(daily_line_free(&mut h));
    // Said again after the load, it is held on the new clock, and a save says so.
    pick(&mut h, T_TOP2);
    listen(&mut h);
    assert_eq!(pick(&mut h, T_TOP), FormId(E_DAY));
    listen(&mut h);
    assert!(!daily_line_free(&mut h));
    let next = h.snapshot();
    assert!(next.said_day_days_clock && !next.said_day.is_empty());
}

#[test]
fn a_skill_line_is_tagged_and_routed_by_its_conditions() {
    let f = fixture();
    let mut g = at_topic_list(&f);
    let c = g.dialogue_choices().iter().find(|c| c.topic == FormId(T_SKILL)).cloned().unwrap();
    let ch = c.challenge.clone().unwrap();
    let have = ch.have.unwrap();
    assert_eq!((ch.form, ch.needed, ch.difficulty), (FormId(AVIF), Some(35.0), None));
    assert_eq!(c.info, if have >= 35.0 { FormId(H_PASS) } else { FormId(H_FAIL) });
    // The other side of the threshold gives the other line.
    let other = if have >= 35.0 { 10.0 } else { 60.0 };
    fnv_game::functions::set_actor_value_current(&mut g, FormId::PLAYER_REF, SPEECH as u16, other).unwrap();
    pick(&mut g, T_TOP2);
    listen(&mut g);
    let c = g.dialogue_choices().iter().find(|c| c.topic == FormId(T_SKILL)).cloned().unwrap();
    assert_eq!(c.info, if other >= 35.0 { FormId(H_PASS) } else { FormId(H_FAIL) });
    assert_eq!(c.challenge.unwrap().have, Some(other));
}

/// A New Vegas skill line gives no XP of its own (its result script would); a Fallout 3
/// style speech challenge (the flag and a difficulty) gives `iXPRewardSpeechChallenge
/// <class>` when it passes, nothing for a class no setting gives, and nothing when it
/// fails (docs/spec-notes/experience.md §2, dialogue.md §3.3).
#[test]
fn a_speech_challenge_gives_its_xp_and_a_skill_line_none() {
    let mut f = fixture();
    let skill_xp = {
        let mut g = at_topic_list(&f);
        fnv_game::functions::set_actor_value_current(&mut g, FormId::PLAYER_REF, SPEECH as u16, 60.0).unwrap();
        pick(&mut g, T_TOP2);
        listen(&mut g);
        assert_eq!(pick(&mut g, T_SKILL), FormId(H_PASS));
        g.player.xp
    };
    assert_eq!(skill_xp, 0.0);
    let mut challenge = |difficulty: u32| {
        let line = f.data.infos.get_mut(&FormId(H_PASS)).unwrap();
        line.flags |= info_flag::SPEECH_CHALLENGE;
        line.challenge_difficulty = Some(difficulty);
        let mut g = at_topic_list(&f);
        // Speech 100 and disposition 85 exceed fSpeechAutoSuccessThreshold (0.9) for both
        // Very Easy and Hard so the pass path is deterministic.
        fnv_game::functions::set_actor_value_current(&mut g, FormId::PLAYER_REF, SPEECH as u16, 100.0).unwrap();
        g.extra.entry(actor()).or_default().dispositions.insert(FormId::PLAYER_REF, 85.0);
        pick(&mut g, T_TOP2);
        listen(&mut g);
        assert_eq!(pick(&mut g, T_SKILL), FormId(H_PASS));
        g.player.xp
    };
    // Very Easy: the master's 5 (the default here, the test plugin sets none).
    assert_eq!(challenge(1), 5.0);
    // Hard (the masters' three lines): no setting, nothing.
    assert_eq!(challenge(4), 0.0);
}

/// A New Vegas Speech check counts a Speech Success when its pass line is chosen and a
/// Speech Failure for its fail line (stats 12 and 31), with no XP of its own; a check
/// of another skill counts neither (#967; misc-stats.md §3).
#[test]
fn a_speech_check_counts_a_success_or_a_failure() {
    use fnv_game::misc_stats::{SPEECH_FAILURES, SPEECH_SUCCESSES};
    let f = fixture();
    let run = |speech: f64| {
        let mut g = at_topic_list(&f);
        fnv_game::functions::set_actor_value_current(&mut g, FormId::PLAYER_REF, SPEECH as u16, speech).unwrap();
        pick(&mut g, T_TOP2);
        listen(&mut g);
        assert_eq!(g.dialogue_choices().iter().find(|c| c.topic == FormId(T_SKILL)).unwrap().challenge.as_ref().unwrap().skill, Some(SPEECH as u16));
        let line = pick(&mut g, T_SKILL);
        (line, g.misc_stat(SPEECH_SUCCESSES), g.misc_stat(SPEECH_FAILURES), g.player.xp)
    };
    assert_eq!(run(60.0), (FormId(H_PASS), 1.0, 0.0, 0.0));
    assert_eq!(run(35.0), (FormId(H_PASS), 1.0, 0.0, 0.0));
    assert_eq!(run(10.0), (FormId(H_FAIL), 0.0, 1.0, 0.0));
    // The same pair testing another skill (Barter, 32): nothing counted.
    let mut f = f;
    for id in [H_PASS, H_FAIL] {
        let line = f.data.infos.get_mut(&FormId(id)).unwrap();
        line.conditions[0].params = vec![ArgValue::Enum(32)];
    }
    let mut g = at_topic_list(&f);
    pick(&mut g, T_TOP2);
    listen(&mut g);
    pick(&mut g, T_SKILL);
    assert_eq!((g.misc_stat(SPEECH_SUCCESSES), g.misc_stat(SPEECH_FAILURES)), (0.0, 0.0));
}

#[test]
fn start_conversation_and_the_dialogue_functions() {
    let f = fixture();
    let mut g = game(&f);
    let player = ArgValue::Value(Value::Ref(FormId::PLAYER_REF.0));
    assert_eq!(call(&mut g, 0x1032, NPC1_REF, &[]), 0.0);
    // StartConversation player: the greeting at once.
    call(&mut g, 0x1056, NPC1_REF, std::slice::from_ref(&player));
    assert_eq!(g.dialogue_actor(), Some(actor()));
    assert_eq!(call(&mut g, 0x1032, NPC1_REF, &[]), 1.0);
    assert_eq!(call(&mut g, 0x108D, NPC1_REF, &[]), 1.0);
    assert_eq!(call(&mut g, 0x11B3, NPC1_REF, &[]), 3.0);
    // A second call while it runs does nothing.
    let lines = g.dialogue.as_ref().unwrap().lines;
    call(&mut g, 0x1056, NPC2_REF, std::slice::from_ref(&player));
    assert_eq!((g.dialogue_actor(), g.dialogue.as_ref().unwrap().lines), (Some(actor()), lines));
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::CallFailed { .. })));
    listen(&mut g);
    assert_eq!(call(&mut g, 0x108D, NPC1_REF, &[]), 0.0);
    // With a topic: that topic instead of the greeting.
    let mut h = game(&f);
    call(&mut h, 0x1056, NPC1_REF, &[player.clone(), ArgValue::Value(Value::Ref(T_BYE))]);
    assert_eq!(h.dialogue.as_ref().unwrap().line.as_ref().unwrap().info, FormId(C_BYE));
    // Two NPCs: no conversation with the player (`two_actors_talk_to_each_other`); with no
    // topic the speaker starts with HELLO, the first line whose Link From is blank.
    let mut k = game(&f);
    call(&mut k, 0x1056, NPC1_REF, &[ArgValue::Value(Value::Ref(NPC2_REF))]);
    assert!(k.dialogue.is_none());
    assert_eq!(k.speech.iter().map(|s| (s.info.0, s.conversation)).collect::<Vec<_>>(), vec![(K_HELLO, Some(1))]);
    assert!(!k.events.iter().any(|e| matches!(&e.kind, EventKind::Stub { .. } | EventKind::CallFailed { .. })));
    // AddTopic: the topic is offered in the topic list.
    let mut a = game(&f);
    call(&mut a, 0x1058, NPC1_REF, &[ArgValue::Value(Value::Ref(T_ADDED))]);
    a.said.insert((actor(), FormId(A_FIRST)));
    a.start_dialogue(actor(), None).unwrap();
    listen(&mut a);
    assert!(offered(&a).contains(&T_ADDED));
}

#[test]
fn two_actors_talk_to_each_other() {
    let f = fixture();
    let mut g = game(&f);
    let (a, b) = (FormId(NPC1_REF), FormId(NPC2_REF));
    let r = |id: u32| ArgValue::Value(Value::Ref(id));
    call(&mut g, 0x1056, NPC1_REF, &[r(NPC2_REF), r(T_ARGUE)]);
    // No menu: the player only overhears; the first line's begin script ran.
    assert!(g.dialogue.is_none() && !g.in_menu());
    assert!(g.in_actor_conversation(a) && g.in_actor_conversation(b));
    assert_eq!(out(&g), 30.0);
    let s = g.speech[0].clone();
    assert_eq!((s.speaker, s.target, s.info, s.conversation), (a, Some(b), FormId(K_OPEN), Some(1)));
    // Only the first response is said.
    assert_eq!(s.remaining, g.response_seconds(a, FormId(K_OPEN), 0));
    assert!(s.remaining < g.line_seconds(a, FormId(K_OPEN)));
    // Either side starting another conversation meanwhile is refused.
    call(&mut g, 0x1056, NPC2_REF, &[r(NPC1_REF), r(T_ARGUE)]);
    let started = |g: &Game| g.events.iter().filter(|e| matches!(e.kind, EventKind::ActorConversationStarted { .. })).count();
    assert_eq!(started(&g), 1);
    for _ in 0..20_000 {
        if g.speech.is_empty() {
            break;
        }
        g.tick();
    }
    // The reply comes from the second actor (next speaker Target), only from the topic it
    // can say; then the same actor again (Self) with the Goodbye line, which ends it.
    let lines: Vec<(FormId, Option<FormId>, u32)> =
        g.events.iter().filter_map(|e| if let EventKind::LineStarted { speaker, target, info, .. } = e.kind { Some((speaker, target, info.0)) } else { None }).collect();
    assert_eq!(lines, vec![(a, Some(b), K_OPEN), (b, Some(a), K_REPLY), (b, Some(a), K_LAST)]);
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::ActorConversationEnded { speaker, target, reason: "goodbye", lines: 3 } if speaker == b && target == a)));
    assert_eq!(out(&g), 32.0);
    assert!(!g.in_actor_conversation(a) && g.dialogue.is_none());
}

/// A Dialogue package's Say To line to another actor goes on as their conversation once
/// its SayToDone has run (TTW's Mary and Tom Holden, issue #2229); a scripted SayTo of
/// the same topic stays one line.
#[test]
fn a_package_say_to_line_leads_on_to_the_reply() {
    let f = fixture();
    let (a, b) = (FormId(NPC1_REF), FormId(NPC2_REF));
    let said = |g: &Game| -> Vec<(FormId, u32)> { g.events.iter().filter_map(|e| if let EventKind::LineStarted { speaker, info, .. } = e.kind { Some((speaker, info.0)) } else { None }).collect() };
    let run = |g: &mut Game| {
        for _ in 0..20_000 {
            if g.speech.is_empty() {
                break;
            }
            g.tick();
        }
    };
    let mut g = game(&f);
    assert_eq!(g.say_package_line(a, b, FormId(T_ARGUE)), Some(FormId(K_OPEN)));
    run(&mut g);
    assert_eq!(said(&g), vec![(a, K_OPEN), (b, K_REPLY), (b, K_LAST)]);
    let mut h = game(&f);
    assert_eq!(h.say(a, Some(b), FormId(T_ARGUE), true), Some(FormId(K_OPEN)));
    run(&mut h);
    assert_eq!(said(&h), vec![(a, K_OPEN)], "a scripted SayTo is one line");
}

#[test]
fn actors_talk_to_each_other_only_with_conversation_lines() {
    let f = fixture();
    let mut g = game(&f);
    let (a, b) = (FormId(NPC1_REF), FormId(NPC2_REF));
    // The Topic line for the player can be said now, and is stored before the
    // Conversation line.
    set_global(&mut g, G_GATE, 5.0);
    assert_eq!(g.conversation_opener(a, b), Some(FormId(K_HELLO)));
    call(&mut g, 0x1056, NPC1_REF, &[ArgValue::Value(Value::Ref(NPC2_REF))]);
    assert_eq!(g.speech.iter().map(|s| s.info.0).collect::<Vec<_>>(), vec![K_HELLO]);
    for _ in 0..20_000 {
        if g.speech.is_empty() {
            break;
        }
        g.tick();
    }
    // The reply skips the Topic line its topic holds for the player.
    let said: Vec<u32> = g.events.iter().filter_map(|e| if let EventKind::LineStarted { info, .. } = e.kind { Some(info.0) } else { None }).collect();
    assert_eq!(said, vec![K_HELLO, K_REPLY, K_LAST]);
    // A hello to the player takes the Topic line.
    assert_eq!(g.say_hello(a), Some(FormId(K_HELLO_PLAYER)));
}

/// A SayTo line to an actor that is a Conversation line with the next speaker Self: the
/// speaker goes on with its next line of the topic, and stops at the Goodbye line.
#[test]
fn a_say_to_line_with_the_next_speaker_self_goes_on_with_the_next_line() {
    let f = fixture();
    let mut g = game(&f);
    // A co-op game: the speaker's own next line is co-op only (`Game::coop_game`).
    g.coop_game = true;
    let (a, b) = (FormId(NPC1_REF), FormId(NPC2_REF));
    assert_eq!(g.say(a, Some(b), FormId(T_SELF), true), Some(FormId(V_SELF)));
    for _ in 0..20_000 {
        if g.speech.is_empty() {
            break;
        }
        g.tick();
    }
    let lines: Vec<(FormId, u32)> = g.events.iter().filter_map(|e| if let EventKind::LineStarted { speaker, info, .. } = e.kind { Some((speaker, info.0)) } else { None }).collect();
    assert_eq!(lines, vec![(a, V_SELF), (a, V_BYE)]);
    assert_eq!(out(&g), 4.0);
}

/// Single player: the line with the next speaker Self is said alone, as on main.
#[test]
fn single_player_says_no_own_next_line() {
    let f = fixture();
    let mut g = game(&f);
    let (a, b) = (FormId(NPC1_REF), FormId(NPC2_REF));
    assert_eq!(g.say(a, Some(b), FormId(T_SELF), true), Some(FormId(V_SELF)));
    for _ in 0..20_000 {
        if g.speech.is_empty() {
            break;
        }
        g.tick();
    }
    let lines: Vec<(FormId, u32)> = g.events.iter().filter_map(|e| if let EventKind::LineStarted { speaker, info, .. } = e.kind { Some((speaker, info.0)) } else { None }).collect();
    assert_eq!(lines, vec![(a, V_SELF)]);
}

#[test]
fn between_actors_a_link_from_reply_answers_a_line_that_links_nowhere() {
    let f = fixture();
    let mut g = game(&f);
    let (a, b) = (FormId(NPC1_REF), FormId(NPC2_REF));
    let r = |id: u32| ArgValue::Value(Value::Ref(id));
    call(&mut g, 0x1056, NPC1_REF, &[r(NPC2_REF), r(T_CHAT)]);
    for _ in 0..20_000 {
        if g.speech.is_empty() {
            break;
        }
        g.tick();
    }
    // The answer is the line that names TestChat, not the topic's first line.
    let lines: Vec<(FormId, u32)> = g.events.iter().filter_map(|e| if let EventKind::LineStarted { speaker, info, .. } = e.kind { Some((speaker, info.0)) } else { None }).collect();
    assert_eq!(lines, vec![(a, C_CHAT), (b, C_REPLY)]);
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::ActorConversationEnded { reason: "goodbye", lines: 2, .. })));
}

/// #2156 (owner's playtest 2026-11-11: everyone in the Legion's Fort said the same line,
/// the slaves' stories came "one after another without variations"): a hello or idle
/// chatter line said of late is not said again by anyone. A Random pool picks among its
/// other lines; once they are all said the actor says nothing, and never a line stored
/// after the pool in its place.
#[test]
fn an_idle_line_said_of_late_is_not_said_again() {
    use fnv_game::dialogue::IDLE_LINE_MEMORY;
    let f = fixture();
    let a = FormId(NPC1_REF);
    // A plain hello: once, then not until the memory is over.
    let mut g = game(&f);
    assert_eq!(g.say_hello_fresh(a), Some(FormId(K_HELLO)));
    g.speech.clear();
    assert_eq!(g.say_hello_fresh(a), None);
    assert!(g.speech.is_empty(), "nothing is said");
    g.time += IDLE_LINE_MEMORY;
    assert_eq!(g.say_hello_fresh(a), Some(FormId(K_HELLO)));
    // A pool of two (P1, P2), then a second pool (P3, P4): whatever the seed, two calls
    // say both lines of the first pool, and a third says nothing.
    for seed in 0..16 {
        let mut g = game_seeded(&f, seed);
        let mut said = Vec::new();
        for _ in 0..2 {
            said.push(g.say_chatter(a, FormId(T_POOL)).expect("a fresh line of the pool").0);
            g.speech.clear();
        }
        said.sort();
        assert_eq!(said, vec![P1, P2], "seed {seed}");
        assert_eq!(g.say_chatter(a, FormId(T_POOL)), None, "seed {seed}: the second pool is not said in its place");
        g.time += IDLE_LINE_MEMORY;
        assert!(g.say_chatter(a, FormId(T_POOL)).is_some_and(|l| l.0 == P1 || l.0 == P2), "seed {seed}");
    }
}

#[test]
fn a_hello_to_the_player_is_one_line() {
    let f = fixture();
    let mut g = game(&f);
    let a = FormId(NPC1_REF);
    // The line with a Link From is passed over; the hello is no conversation and no menu.
    assert_eq!(g.say_hello(a), Some(FormId(K_HELLO)));
    assert!(g.dialogue.is_none() && !g.in_menu() && !g.in_actor_conversation(a));
    let s = g.speech[0].clone();
    assert_eq!((s.speaker, s.target, s.hello, s.conversation), (a, Some(FormId::PLAYER_REF), true, None));
    for _ in 0..20_000 {
        if g.speech.is_empty() {
            break;
        }
        g.tick();
    }
    // Its links are not followed: nobody answers.
    let lines = g.events.iter().filter(|e| matches!(e.kind, EventKind::LineStarted { .. })).count();
    assert_eq!(lines, 1);
    // An actor with no HELLO line says nothing.
    assert_eq!(g.say_hello(FormId(NPC2_REF)), None);
}

#[test]
fn say_to_uses_the_same_selection() {
    let f = fixture();
    let mut g = game(&f);
    set_global(&mut g, G_GATE, 5.0);
    assert_eq!(g.say(actor(), Some(FormId::PLAYER_REF), FormId(GREETING), true), Some(FormId(A_HIGH)));
    // A SayTo line is no conversation.
    assert!(g.dialogue.is_none());
    assert_eq!(call(&mut g, 0x1032, NPC1_REF, &[]), 0.0);
}

#[test]
fn lines_said_to_someone_turn_the_two_to_each_other() {
    let f = fixture();
    let (a, b, player) = (FormId(NPC1_REF), FormId(NPC2_REF), FormId::PLAYER_REF);
    let r = |id: u32| ArgValue::Value(Value::Ref(id));
    let n = |x: f64| ArgValue::Value(Value::Num(x));
    let turns = |g: &Game| g.facings().iter().map(|f| (f.who, f.to, f.body, f.speaking)).collect::<Vec<_>>();
    // SayTo: the speaker turns head and body, the target looks back (head only).
    let mut g = game(&f);
    set_global(&mut g, G_GATE, 5.0);
    call(&mut g, 0x1034, NPC1_REF, &[r(player.0), r(GREETING)]);
    assert_eq!(turns(&g), vec![(a, player, true, true), (player, a, false, false)]);
    assert!(g.facings().iter().all(|f| f.info == FormId(A_HIGH)));
    // noTargetLookFlag 0 given: the same turns.
    let mut g = game(&f);
    set_global(&mut g, G_GATE, 5.0);
    call(&mut g, 0x1034, NPC1_REF, &[r(player.0), r(GREETING), n(0.0), n(0.0)]);
    assert_eq!(turns(&g), vec![(a, player, true, true), (player, a, false, false)]);
    // Say has no listener: nobody turns.
    let mut g = game(&f);
    set_global(&mut g, G_GATE, 5.0);
    call(&mut g, 0x1033, NPC1_REF, &[r(GREETING)]);
    assert!(g.is_talking(a) && turns(&g).is_empty());
    // A conversation between actors: both turn head and body, line after line.
    let mut g = game(&f);
    call(&mut g, 0x1056, NPC1_REF, &[r(NPC2_REF), r(T_ARGUE)]);
    assert_eq!(turns(&g), vec![(a, b, true, true), (b, a, true, false)]);
    // headTrack 0: nobody turns, in the replies too.
    let mut g = game(&f);
    call(&mut g, 0x1056, NPC1_REF, &[r(NPC2_REF), r(T_ARGUE), r(0), r(0), n(0.0)]);
    assert!(g.in_actor_conversation(a) && turns(&g).is_empty());
    let first = g.speech[0].info;
    for _ in 0..20_000 {
        if g.speech.first().is_none_or(|s| s.info != first) {
            break;
        }
        g.tick();
    }
    assert_eq!(g.speech.first().map(|s| s.speaker), Some(b), "the reply is said");
    assert!(turns(&g).is_empty());
}

/// SayTo with `noTargetLookFlag` 1 turns the speaker to its target, and the target does
/// not look back, whoever the target is (observer check 218, #3989,
/// docs/design/dialogue-play.md §9.2).
#[test]
fn say_to_with_no_target_look_turns_the_speaker_alone() {
    let f = fixture();
    let r = |id: u32| ArgValue::Value(Value::Ref(id));
    let n = |x: f64| ArgValue::Value(Value::Num(x));
    for target in [FormId::PLAYER_REF.0, NPC2_REF] {
        let mut g = game(&f);
        set_global(&mut g, G_GATE, 5.0);
        call(&mut g, 0x1034, NPC1_REF, &[r(target), r(GREETING), n(0.0), n(1.0)]);
        assert!(g.is_talking(FormId(NPC1_REF)), "the line is said to {target:08X}");
        assert_eq!(g.speech[0].target, Some(FormId(target)));
        let f = g.facings();
        assert_eq!(f.len(), 1, "the speaker alone turns for the line to {target:08X}");
        assert_eq!((f[0].who, f[0].to, f[0].body), (FormId(NPC1_REF), FormId(target), true));
    }
}

#[test]
fn a_conversation_survives_a_snapshot() {
    let f = fixture();
    let mut g = at_topic_list(&f);
    pick(&mut g, T_TOP);
    g.frames(5);
    let snap = g.snapshot();
    let text = serde_json::to_string(&snap).unwrap();
    let back: GameSnapshot = serde_json::from_str(&text).unwrap();
    assert_eq!(back, snap);
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert_eq!(h.dialogue, g.dialogue);
    assert_eq!(h.said_day, g.said_day);
    listen(&mut g);
    listen(&mut h);
    assert_eq!(offered(&h), offered(&g));
    assert_eq!(h.frame, g.frame);
}

#[test]
fn the_runner_talks_and_prints_a_dialogue_trace() {
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.dialogue.actions/1",
        "actions": [
            {"op": "talk", "actor": "TalkerREF", "label": "greeting"},
            {"op": "wait_talk", "label": "first list"},
            {"op": "choose", "prompt": "tell me", "label": "asked"},
            {"op": "choose", "topic": "TopicNow"},
            {"op": "choose", "index": 4, "label": "bye"},
            {"op": "wait_talk", "label": "over"},
            {"op": "choose", "index": 0, "label": "too late"}
        ]
    }))
    .unwrap();
    let t = fnv_game::runner::run_dialogue(&f.data, &f.forms, &f.refs, &input, false).unwrap();
    assert_eq!(t["format"], "fnv.dialogue.trace/1");
    let obs = |label: &str| t["observations"].as_array().unwrap().iter().find(|o| o["label"] == label).cloned().unwrap();
    assert_eq!(obs("greeting")["info"], FormId(A_FIRST).to_string());
    assert_eq!((obs("first list")["state"].as_str(), obs("first list")["choices"].as_u64()), (Some("choosing"), Some(2)));
    assert_eq!(obs("asked")["info"], FormId(B_ASK).to_string());
    assert_eq!(obs("bye")["info"], FormId(I_BYE).to_string());
    assert_eq!(obs("over")["state"], "ended");
    assert!(obs("too late")["errors"]["action"].as_str().unwrap().contains("no choices"));
    let kinds: Vec<&str> = t["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    for k in ["dialogue_started", "line_started", "response_said", "result_script", "line_ended", "choices_offered", "choice_made", "topic_added", "dialogue_ended"] {
        assert!(kinds.contains(&k), "{k} missing in {kinds:?}");
    }
    // Lengths, not texts, unless asked.
    let line = t["events"].as_array().unwrap().iter().find(|e| e["kind"] == "line_started").unwrap();
    assert_eq!((line["chars"].as_u64(), line["topic_edid"].as_str(), line["responses"].as_u64()), (Some(12), Some("GREETING"), Some(2)));
    assert!(!t.to_string().contains("Second."));
    let with_text = fnv_game::runner::run_dialogue(&f.data, &f.forms, &f.refs, &input, true).unwrap();
    assert!(with_text.to_string().contains("Second.") && with_text.to_string().contains("Tell me"));
    assert_eq!(t["final"]["dialogue"], Json::Null);
    // The same document is not a script actions document.
    assert!(fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).is_err());
}

/// The runner's `wait_talk` after a Goodbye greeting (no conversation opens) waits for
/// the line to end, so its end script has run before the next step (issue #1114: a
/// following conversation used to cut Carlitos's and Joana's lines); `end_talk` with no
/// conversation running does nothing.
#[test]
fn the_runners_wait_talk_hears_a_goodbye_greeting_out_and_end_talk_after_the_end_does_nothing() {
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "actions": [
            {"op": "set_global", "name": "TestGate", "value": 7},
            {"op": "talk", "actor": "TalkerREF", "label": "bark"},
            {"op": "get_global", "name": "TestOut", "label": "begun"},
            {"op": "wait_talk", "label": "heard"},
            {"op": "get_global", "name": "TestOut", "label": "after"},
            {"op": "end_talk"},
            {"op": "get_global", "name": "TestOut", "label": "still"}
        ]
    }))
    .unwrap();
    let t = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    let all = t["observations"].as_array().unwrap();
    let obs = |label: &str| all.iter().find(|o| o["label"] == label).cloned().unwrap();
    assert_eq!(obs("bark")["info"], FormId(A_BARK).to_string());
    assert_eq!(obs("begun")["value"], 3.0);
    assert_eq!(obs("heard")["state"], "ended");
    // The end script ran during the wait.
    assert_eq!(obs("after")["value"], 4.0);
    assert_eq!(obs("still")["value"], 4.0);
    assert!(all.iter().all(|o| o.get("errors").is_none()), "{all:?}");
}

/// The runner's `player_activate` of an actor ends in the default action of talking
/// (`Outcome::Talk`), and the conversation opens with the greeting, as the play loop's
/// activation does (nvf-play `talk_after`).
#[test]
fn the_runners_player_activation_of_an_actor_opens_the_conversation() {
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.dialogue.actions/1",
        "actions": [
            {"op": "player_activate", "target": "TalkerREF", "label": "activated"},
            {"op": "wait_talk", "label": "list"}
        ]
    }))
    .unwrap();
    let t = fnv_game::runner::run_dialogue(&f.data, &f.forms, &f.refs, &input, false).unwrap();
    let obs = |label: &str| t["observations"].as_array().unwrap().iter().find(|o| o["label"] == label).cloned().unwrap();
    assert_eq!(obs("activated")["info"], FormId(A_FIRST).to_string());
    assert_eq!((obs("list")["state"].as_str(), obs("list")["choices"].as_u64()), (Some("choosing"), Some(2)));
}

/// "If this box is unchecked, conversations will never use the same topic twice (in the
/// same conversation)" [documented: GECK wiki Quest Data Tab]: ping-pong in a quest
/// without Allow Repeated Conversation Topics stops after the pong; in one with it, it
/// goes on.
#[test]
fn a_conversation_uses_a_topic_once_unless_its_quest_allows_repeats() {
    let f = fixture();
    let r = |id: u32| ArgValue::Value(Value::Ref(id));
    let lines = |g: &Game| g.events.iter().filter(|e| matches!(e.kind, EventKind::LineStarted { .. })).count();
    let mut g = game(&f);
    call(&mut g, 0x1056, NPC1_REF, &[r(NPC2_REF), r(T_PING)]);
    for _ in 0..20_000 {
        if g.speech.is_empty() {
            break;
        }
        g.tick();
    }
    assert_eq!(lines(&g), 2, "ping, pong, and no second ping");
    let mut g = game(&f);
    call(&mut g, 0x1056, NPC1_REF, &[r(NPC2_REF), r(T_RPING)]);
    for _ in 0..2_000 {
        g.tick();
    }
    assert!(lines(&g) >= 4, "the repeating quest's ping-pong goes on: {} lines", lines(&g));
}

/// Issue #1592: a follow-up keeps the conversation's topics, so a link after it back to a
/// topic already used is passed over unless the quest allows repeats. The follow-up itself
/// is stored under a topic already used and is said all the same (dialogue.md §5.1).
#[test]
fn a_follow_up_between_actors_keeps_the_topics_of_the_conversation() {
    let f = fixture();
    let r = |id: u32| ArgValue::Value(Value::Ref(id));
    let lines = |g: &Game| g.events.iter().filter_map(|e| if let EventKind::LineStarted { info, .. } = e.kind { Some(info.0) } else { None }).collect::<Vec<_>>();
    let mut g = game(&f);
    call(&mut g, 0x1056, NPC1_REF, &[r(NPC2_REF), r(T_HIST_A)]);
    for _ in 0..20_000 {
        if g.speech.is_empty() {
            break;
        }
        g.tick();
    }
    assert_eq!(lines(&g), vec![H_A, H_B, H_FOLLOW], "the link back to the linked line's topic is passed over");
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::ActorConversationEnded { reason: "no_links", lines: 3, .. })));
    let mut g = game(&f);
    call(&mut g, 0x1056, NPC1_REF, &[r(NPC2_REF), r(T_RHIST_A)]);
    for _ in 0..2_000 {
        g.tick();
    }
    assert_eq!(lines(&g).get(..4), Some(&[H_RA, H_RB, H_RFOLLOW, H_RB][..]), "the repeating quest's chain goes back: {:X?}", lines(&g));
}

/// Playing a Voice note runs its line's result scripts: the begin script as it starts,
/// the end script as it ends (`Game::holotape_script`; Dead Money's Dog command tape is
/// how its quest reaches NVDLC01MQ01a 40, docs/spec-notes/holotapes.md §3). The runner's
/// `play_note` runs both.
#[test]
fn a_voice_note_runs_its_lines_result_scripts() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(g.holotape_script(FormId(NOTE_TAPE), 0), Some(FormId(S_TAPE)));
    assert_eq!(out(&g), 6.0, "the begin script");
    assert_eq!(g.holotape_script(FormId(NOTE_TAPE), 1), Some(FormId(S_TAPE)));
    assert_eq!(out(&g), 7.0, "the end script");
    // A topic changed while the tape plays: it ends with the line it began.
    assert_eq!(g.holotape_script(FormId(NOTE_TAPE), 0), Some(FormId(S_TAPE)));
    g.run_query("Begin Function { }
SetNoteTopic TestTapeNote TestPing
End
", Default::default()).unwrap();
    assert_eq!(g.holotape_script(FormId(NOTE_TAPE), 1), Some(FormId(S_TAPE)));
    // Not a voice note: nothing runs.
    assert_eq!(g.holotape_script(FormId(G_OUT), 0), None);
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "actions": [{"op": "play_note", "note": "TestTapeNote", "label": "tape"}, {"op": "get_global", "name": "TestOut", "label": "out"}]
    }))
    .unwrap();
    let t = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    let obs = t["observations"].as_array().unwrap();
    assert_eq!(obs[0]["info"], json!(FormId(S_TAPE).to_string()));
    assert_eq!(obs[1]["value"], json!(7.0));
}

/// The runner's `patrol_point` stands in for an actor walking to a patrol point: the
/// point's result script runs (`Game::patrol_point_reached`; Dead Money's vault path
/// marker sets bElijahArrivesVault this way).
#[test]
fn the_runner_reaches_a_patrol_point() {
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "actions": [{"op": "patrol_point", "actor": "TalkerREF", "point": "PatrolREF"}, {"op": "get_global", "name": "TestOut", "label": "out"}]
    }))
    .unwrap();
    let t = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    assert_eq!(t["steps"][0].get("error"), None, "{}", t["steps"][0]);
    assert_eq!(t["observations"][0]["value"], json!(9.0));
}

/// The extender's note functions (#984): the getters read the record, the setters change
/// the note for the session, and the read flag is saved (docs/spec-notes/holotapes.md §5).
#[test]
fn note_functions_read_and_change_a_note() {
    use fnv_script::vm::XVal;
    let f = fixture();
    let mut g = game(&f);
    let q = |g: &mut Game, body: &str| g.run_query(&format!("array_var aQ
Begin Function {{ }}
{body}
End
"), fnv_game::game::RunCtx::default());
    let n = |v: f64| Ok(XVal::Num(v));
    assert_eq!(q(&mut g, "SetFunctionValue (GetNoteType TestTapeNote)"), n(3.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GetNoteSpeaker TestTapeNote)"), Ok(XVal::Ref(NPC1)));
    assert_eq!(q(&mut g, "SetFunctionValue (GetNoteTopic TestTapeNote)"), Ok(XVal::Ref(T_TAPE)));
    assert_eq!(q(&mut g, "let aQ := GetNoteQuestList TestTapeNote
SetFunctionValue (Ar_Size aQ) * 10 + (aQ[0] == QA)"), n(11.0));
    // A setter changes the note for the session; the tape it plays follows.
    assert_eq!(q(&mut g, "SetFunctionValue (SetNoteTopic TestTapeNote TestPing)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GetNoteTopic TestTapeNote)"), Ok(XVal::Ref(T_PING)));
    assert_eq!(
        g.note_tape(FormId(NOTE_TAPE)),
        Some(fnv_game::holotapes::Tape::Voice { speaker: FormId(NPC1), info: FormId(P_PING) }),
        "the new topic's line that names no speaker"
    );
    assert_eq!(q(&mut g, "SetFunctionValue (AddNoteQuest TestTapeNote QHi) + (AddNoteQuest TestTapeNote QHi) + (RemoveNoteQuest TestTapeNote QA)"), n(3.0));
    assert_eq!(q(&mut g, "let aQ := GetNoteQuestList TestTapeNote
SetFunctionValue (Ar_Size aQ) * 10 + (aQ[0] == QHi)"), n(11.0));
    assert_eq!(q(&mut g, "SetFunctionValue (SetNoteType TestTapeNote 9) * 10 + (SetNoteType Talker 1)"), n(0.0));
    // The read flag: the Pip-Boy marks it (saved); SetNoteRead without serialize lasts the
    // session, with it the save.
    assert_eq!(q(&mut g, "SetFunctionValue (GetNoteRead TestTapeNote)"), n(0.0));
    g.mark_note_read(FormId(NOTE_TAPE));
    assert_eq!(q(&mut g, "SetFunctionValue (GetNoteRead TestTapeNote)"), n(1.0));
    q(&mut g, "SetNoteRead TestTapeNote 0").unwrap();
    assert_eq!(q(&mut g, "SetFunctionValue (GetNoteRead TestTapeNote)"), n(0.0));
    let snap: GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.snapshot()).unwrap()).unwrap();
    assert!(snap.player.notes_read.contains(&FormId(NOTE_TAPE)), "the session change is not in the save");
    q(&mut g, "SetNoteRead TestTapeNote 0 1").unwrap();
    assert!(!g.snapshot().player.notes_read.contains(&FormId(NOTE_TAPE)));
    // The note's one SNAM is read by its type: made a Sound note, the speaker is its
    // sound, for the getter and the tape alike.
    assert_eq!(q(&mut g, "SetFunctionValue (SetNoteType TestTapeNote 0)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GetNoteSound TestTapeNote)"), Ok(XVal::Ref(NPC1)));
    assert_eq!(g.note_tape(FormId(NOTE_TAPE)), Some(fnv_game::holotapes::Tape::Sound(FormId(NPC1))));
}

/// Known topics are kept newest first: one line's Add Topics go to the front together in
/// record order, a line's topic learned again moves to the front, and `AddTopic` on a
/// known topic leaves it in place (owner's choice 2026-10-31, dialogue.md §3.5). Saves
/// holding the old sorted list still load.
#[test]
fn known_topics_newest_first() {
    use fnv_game::game::KnownTopics;
    let ids = |k: &KnownTopics| k.iter().map(|t| t.0).collect::<Vec<_>>();
    let mut k = KnownTopics::default();
    assert_eq!(k.learn(&[FormId(0xC8), FormId(0xC7)]), vec![FormId(0xC8), FormId(0xC7)]);
    assert!(k.insert(FormId(0xCA)));
    assert_eq!(k.learn(&[FormId(0xCC), FormId(0xCB)]).len(), 2);
    assert_eq!(ids(&k), vec![0xCC, 0xCB, 0xCA, 0xC8, 0xC7]);
    // AddTopic again: stays where it is, and not reported as new.
    assert!(!k.insert(FormId(0xC8)));
    assert_eq!(ids(&k), vec![0xCC, 0xCB, 0xCA, 0xC8, 0xC7]);
    // A line's Add Topics again: to the front.
    assert!(k.learn(&[FormId(0xC8)]).is_empty());
    assert_eq!(ids(&k), vec![0xC8, 0xCC, 0xCB, 0xCA, 0xC7]);
    let old: KnownTopics = serde_json::from_value(json!([FormId(1).to_string(), FormId(2).to_string(), FormId(3).to_string()])).unwrap();
    assert_eq!(ids(&old), vec![1, 2, 3]);
}

/// A co-op guest says the host's lines (`Game::set_remote_speech`): the host times them,
/// so they never end here on their own and their end scripts do not run here; the next
/// list replaces them. A line of the guest's own by an actor the host runs stops; its own
/// Goodbye greeting at the player does not.
#[test]
fn the_hosts_lines_are_said_without_their_scripts() {
    use fnv_game::game::{LineLook, Speech};
    let f = fixture();
    let mut g = game(&f);
    set_global(&mut g, G_GATE, 7.0);
    g.frames(3);
    let line = |speaker: FormId, remaining: f64| Speech {
        speaker,
        target: None,
        leads_on: false,
        through: None,
        topic: FormId(GREETING),
        info: FormId(A_BARK),
        remaining,
        say_to: false,
        greeting: false,
        farewell: false,
        conversation: None,
        earlier_topics: Vec::new(),
        hello: false,
        look: LineLook::Both,
        response: 0,
        force_subtitles: false,
        two_d: false,
        end_ran: false,
        remote: false,
    };
    let before = out(&g);
    g.set_remote_speech(vec![line(actor(), 0.05)], |_| false);
    g.frames(200);
    assert!(matches!(g.speech.as_slice(), [s] if s.remote && s.speaker == actor()), "{:?}", g.speech);
    assert_eq!(out(&g), before, "no end script here");
    g.set_remote_speech(Vec::new(), |_| false);
    assert!(g.speech.is_empty());
    assert_eq!(out(&g), before);
    // The guest's own line by an actor the host runs stops; its greeting at the player stays.
    g.speech.push(line(FormId(NPC2_REF), 30.0));
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_BARK)));
    assert_eq!(g.speech.len(), 2);
    g.set_remote_speech(Vec::new(), |_| true);
    assert!(matches!(g.speech.as_slice(), [s] if s.speaker == actor() && s.greeting && !s.remote), "{:?}", g.speech);
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { speaker, cut: true, .. } if speaker == FormId(NPC2_REF))));
}

/// A co-op guest: an update of the host's line that comes a little late does not take its
/// time left back up (that starts the sound over), a line said again does.
#[test]
fn a_hosts_line_come_late_is_not_said_again() {
    use fnv_game::game::{LineLook, Speech, REMOTE_SAID_AGAIN};
    let f = fixture();
    let mut g = game(&f);
    let line = |remaining: f64| Speech {
        speaker: actor(),
        target: None,
        leads_on: false,
        through: None,
        topic: FormId(GREETING),
        info: FormId(A_BARK),
        remaining,
        say_to: false,
        greeting: false,
        farewell: false,
        conversation: None,
        earlier_topics: Vec::new(),
        hello: false,
        look: LineLook::Both,
        response: 0,
        force_subtitles: false,
        two_d: false,
        end_ran: false,
        remote: false,
    };
    let left = |g: &fnv_game::Game| g.speech.iter().find(|s| s.remote).map(|s| s.remaining);
    g.set_remote_speech(vec![line(4.0)], |_| false);
    g.set_remote_speech(vec![line(3.5)], |_| false);
    // Late by 0.1 s: stays where it was.
    g.set_remote_speech(vec![line(3.6)], |_| false);
    assert_eq!(left(&g), Some(3.5));
    // Down: taken.
    g.set_remote_speech(vec![line(3.2)], |_| false);
    assert_eq!(left(&g), Some(3.2));
    // Said again from the start: taken.
    g.set_remote_speech(vec![line(3.2 + REMOTE_SAID_AGAIN + 1.0)], |_| false);
    assert_eq!(left(&g), Some(3.2 + REMOTE_SAID_AGAIN + 1.0));
}

#[test]
fn a_view_of_the_conversation_hears_before_its_result_scripts_run() {
    use std::sync::{Arc, Mutex};
    type Heard = Arc<Mutex<Vec<(f32, Option<u32>, usize)>>>;
    fn listen_in(g: &mut Game) -> Heard {
        let heard: Heard = Default::default();
        let ears = heard.clone();
        g.ui_dialogue_hook = Some(Box::new(move |g| {
            let c = g.dialogue.as_ref().unwrap();
            ears.lock().unwrap().push((out(g), c.line.as_ref().map(|l| l.info.0), c.choices.len()));
        }));
        heard
    }
    let f = fixture();
    // At the conversation's start: the first line's begin script sets TestOut to 1, and the
    // conversation and its line are up before it runs.
    let mut g = game(&f);
    let heard = listen_in(&mut g);
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_FIRST)));
    assert_eq!(out(&g), 1.0);
    assert_eq!(heard.lock().unwrap()[0], (0.0, Some(A_FIRST), 0));
    // The next response; before the end script (it sets 2); the choices offered after it.
    listen(&mut g);
    let list = heard.lock().unwrap().clone();
    assert_eq!(list[1], (1.0, Some(A_FIRST), 0), "{list:?}");
    assert!(list.iter().any(|h| h.0 == 1.0 && h.2 == 0), "{list:?}");
    assert_eq!(*list.last().unwrap(), (2.0, None, offered(&g).len()), "{list:?}");
    // At a choice: E_NOW runs its result script as it starts; the runtime's choose reaches the
    // hook with the new line up and the choices gone, before the script's value is there.
    let mut g = at_topic_list(&f);
    let before = out(&g);
    let heard = listen_in(&mut g);
    let said = pick(&mut g, T_TOP2);
    assert_eq!(said, FormId(E_NOW));
    assert_ne!(out(&g), before);
    assert_eq!(heard.lock().unwrap()[0], (before, Some(E_NOW), 0));
    // A line with no result script: told all the same, before the call that started it returns.
    let mut g = game(&f);
    g.said.insert((actor(), FormId(A_FIRST)));
    let heard = listen_in(&mut g);
    assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_LATER)));
    assert_eq!(heard.lock().unwrap().first().copied(), Some((0.0, Some(A_LATER), 0)));
    // No conversation: nothing to tell.
    let mut g = game(&f);
    let heard = listen_in(&mut g);
    g.dialogue_transition();
    assert!(heard.lock().unwrap().is_empty());
}

/// Authored results append digits: 0 -> 1 -> 12 proves order and counts executions.
fn ordered_fixture(line: u32, begin: Option<Vec<Subrecord>>) -> Fixture {
    let digit = |n: &str| unit(0, &set(var(b'G', 1), ex(&[var(b'G', 1), a("10"), a("*"), a(n), a("+")])), &[], &[G_OUT]);
    let extra = topic(0x980, "OrderTopic", "Order", topic_type::TOPIC, 0, 50.0, vec![L {
        id: 0xF80, quest: QA, begin_raw: Some(begin.unwrap_or_else(|| digit("1"))), end_raw: Some(digit("2")), ..L::default()
    }]);
    let mut f = fixture_bytes(&[plugin_bytes(), top(b"DIAL", &extra)].concat());
    let scripts = f.data.infos[&FormId(0xF80)].clone();
    let def = f.data.infos.get_mut(&FormId(line)).unwrap();
    def.flags |= info_flag::RUN_IMMEDIATELY;
    def.begin = scripts.begin;
    def.end = scripts.end;
    f
}

fn finish_speech(g: &mut Game) {
    for _ in 0..20_000 {
        if g.speech.is_empty() {
            return;
        }
        g.tick();
    }
    panic!("speech never ended");
}

#[test]
fn immediate_actor_conversation_results_run_in_order_once() {
    let mut f = ordered_fixture(K_OPEN, None);
    // End this authored exchange after its opener: a Goodbye line that links nowhere.
    let open = f.data.infos.get_mut(&FormId(K_OPEN)).unwrap();
    open.goodbye = true;
    open.choices.clear();
    open.follow_ups.clear();
    for interrupt in [false, true] {
        let mut g = game(&f);
        call(&mut g, 0x1056, NPC1_REF, &[ArgValue::Value(Value::Ref(NPC2_REF)), ArgValue::Value(Value::Ref(T_ARGUE))]);
        assert_eq!(out(&g), 12.0);
        assert!(g.speech[0].end_ran && g.speech[0].conversation.is_some());
        if interrupt {
            g.world.set_enabled(actor(), false).unwrap();
        }
        finish_speech(&mut g);
        assert_eq!(out(&g), 12.0);
        assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { info, cut, .. } if info == FormId(K_OPEN) && cut == interrupt)));
    }
}

#[test]
fn immediate_say_to_and_hello_results_survive_completion_interruption_and_load() {
    for hello in [false, true] {
        let line = if hello { K_HELLO } else { A_FIRST };
        let f = ordered_fixture(line, None);
        for interrupt in [false, true] {
            let mut g = game(&f);
            let said = if hello { g.say_hello(actor()) } else { g.say(actor(), Some(FormId::PLAYER_REF), FormId(GREETING), true) };
            assert_eq!(said, Some(FormId(line)));
            assert_eq!(out(&g), 12.0);
            assert!(g.speech[0].end_ran);
            let snap: GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.snapshot()).unwrap()).unwrap();
            let world = World::restore(&f.forms, &f.refs, &snap.sim).unwrap();
            let mut h = Game::restore(&f.data, world, g.cfg, &snap).unwrap();
            assert_eq!(h.speech, g.speech);
            for game in [&mut g, &mut h] {
                if interrupt {
                    game.world.set_enabled(actor(), false).unwrap();
                }
                finish_speech(game);
                assert_eq!(out(game), 12.0);
                assert!(game.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { info, cut, .. } if info == FormId(line) && cut == interrupt)));
                game.frames(3);
                assert_eq!(out(game), 12.0);
            }
        }
    }
}

#[test]
fn immediate_results_do_not_mark_a_replacement_speech() {
    // The begin script replaces its own line. The original end still runs now, while
    // the replacement's ordinary end must remain due at completion.
    let f = ordered_fixture(A_FIRST, Some(say_result(NPC1_REF, T_BYE)));
    let mut g = game(&f);
    assert_eq!(g.say(actor(), Some(FormId::PLAYER_REF), FormId(GREETING), true), Some(FormId(A_FIRST)));
    assert_eq!(out(&g), 2.0);
    assert_eq!(g.speech.len(), 1);
    assert_eq!(g.speech[0].info, FormId(C_BYE));
    assert!(!g.speech[0].end_ran);
    finish_speech(&mut g);
    assert_eq!(out(&g), 9.0);
}

#[test]
fn immediate_radio_results_run_in_order_at_start_even_if_interrupted() {
    let mut f = ordered_fixture(E_NOW, None);
    f.data.infos.get_mut(&FormId(E_NOW)).unwrap().info_type = topic_type::RADIO;
    for complete in [false, true] {
        let mut g = game(&f);
        g.radio_line_begins(actor(), FormId(E_NOW));
        assert_eq!(out(&g), 12.0);
        // An interrupted engine broadcast discards on_air without an end call.
        if complete {
            g.radio_line_ends(actor(), FormId(E_NOW));
        }
        assert_eq!(out(&g), 12.0);
    }
    f.data.infos.get_mut(&FormId(E_NOW)).unwrap().flags &= !info_flag::RUN_IMMEDIATELY;
    let mut g = game(&f);
    g.radio_line_begins(actor(), FormId(E_NOW));
    assert_eq!(out(&g), 1.0);
    g.radio_line_ends(actor(), FormId(E_NOW));
    assert_eq!(out(&g), 12.0);
    let mut interrupted = game(&f);
    interrupted.radio_line_begins(actor(), FormId(E_NOW));
    assert_eq!(out(&interrupted), 1.0);
}

#[test]
fn immediate_goodbye_greeting_marks_its_end_before_a_begin_callback_finishes_it() {
    let f = ordered_fixture(A_BARK, Some(say_result(NPC1_REF, T_BYE)));
    let mut g = game(&f);
    set_global(&mut g, G_GATE, 7.0);
    assert_eq!(g.start_dialogue(actor(), None).unwrap(), FormId(A_BARK));
    assert!(g.dialogue.is_none());
    assert_eq!(out(&g), 2.0, "the greeting ended inside begin; its immediate end runs once");
    assert_eq!(g.speech[0].info, FormId(C_BYE));
    assert!(!g.speech[0].end_ran);
    finish_speech(&mut g);
    assert_eq!(out(&g), 9.0);
}

/// Scripts' edits to topics and lines, read by the conversation (SUP batch 2, #1008;
/// docs/spec-notes/ext-sup2.md): the setters of SUP, JohnnyGuitar, ShowOff and ITR.
mod edits {
    use super::*;
    use fnv_script::vm::XVal;

    fn q(g: &mut Game, body: &str) -> XVal {
        g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
    }

    fn n(g: &mut Game, expr: &str) -> f64 {
        match q(g, &format!("SetFunctionValue ({expr})")) {
            XVal::Num(x) => x,
            XVal::Ref(r) => f64::from(r),
            v => panic!("{expr}: {v:?}"),
        }
    }

    fn s(g: &mut Game, expr: &str) -> String {
        match q(g, &format!("string_var sOut\nlet sOut := {expr}\nSetFunctionValue sOut")) {
            XVal::Str(s) => s,
            v => panic!("{expr}: {v:?}"),
        }
    }

    /// An array's entries, forms as their IDs in hex and strings as they are.
    fn entries(g: &mut Game, expr: &str) -> Vec<String> {
        let at = |i: &str| format!("array_var a\nlet a := {expr}\nSetFunctionValue {i}");
        let len = match q(g, &at("(ar_Size a)")) {
            XVal::Num(x) => x as usize,
            v => panic!("{expr}: {v:?}"),
        };
        (0..len)
            .map(|i| match q(g, &at(&format!("(a[{i}])"))) {
                XVal::Ref(r) => format!("{r:X}"),
                XVal::Num(x) => format!("{:X}", x as u32),
                XVal::Str(s) => s,
                v => panic!("{expr}[{i}]: {v:?}"),
            })
            .collect()
    }

    fn hex(ids: &[u32]) -> Vec<String> {
        ids.iter().map(|i| format!("{i:X}")).collect()
    }

    #[test]
    fn topic_flags_and_priority_change_the_topic_list() {
        let f = fixture();
        let mut g = game(&f);
        // [documented: TopicSetFlags, TopicSetPriority: set, or read with no value;
        // observed: setting returns 0].
        assert_eq!(n(&mut g, "TopicSetFlags TopicDaily"), 2.0);
        assert_eq!(n(&mut g, "TopicSetPriority TopicSkill"), 10.0);
        assert_eq!(n(&mut g, "TopicSetPriority TopicSkill 100"), 0.0);
        assert_eq!(n(&mut g, "TopicSetPriority TopicSkill"), 100.0);
        assert_eq!(n(&mut g, "TopicSetFlags TopicDaily 0"), 0.0);
        assert_eq!(n(&mut g, "TopicSetFlags TopicDaily"), 0.0);
        g.start_dialogue(actor(), Some(FormId(T_LF_PLAIN))).unwrap();
        listen(&mut g);
        // Was T_TOP2, T_TOP, T_LF_TO, T_SKILL, GOODBYE: Skill now leads, Daily is no
        // longer top-level.
        assert_eq!(offered(&g), vec![T_SKILL, T_TOP2, T_LF_TO, GOODBYE]);
        // The records are as they were.
        assert_eq!(f.data.topic_defs[&FormId(T_SKILL)].priority, 10.0);
    }

    #[test]
    fn a_lines_choices_and_prompt() {
        let f = fixture();
        let mut g = game(&f);
        g.said.insert((actor(), FormId(A_FIRST)));
        // [documented: TopicInfoAddChoice, -1 the end and 0 the beginning].
        q(&mut g, "TopicInfoAddChoice LineA04 TopicAsk -1");
        q(&mut g, "TopicInfoAddChoice LineA04 TopicNow 0");
        assert_eq!(entries(&mut g, "TopicInfoGetAllChoices LineA04"), hex(&[T_TOP2, T_ASK]));
        // Setting returns "" [observed].
        assert_eq!(s(&mut g, "TopicInfoSetPrompt LineB01 \"Ask away\""), "");
        assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_LATER)));
        listen(&mut g);
        // Only the line's own choices, by priority.
        assert_eq!(offered(&g), vec![T_TOP2, T_ASK]);
        assert_eq!(g.choice_text(&g.dialogue_choices()[1]), "Ask away");
        // [documented: TopicInfoReplaceChoice; DialogResponseRelatedGetAll type 1].
        q(&mut g, "TopicInfoReplaceChoice LineA04 TopicDaily 0");
        q(&mut g, "TopicInfoReplaceChoice LineA04 TopicDaily 7");
        assert_eq!(entries(&mut g, "DialogResponseRelatedGetAll LineA04 1"), hex(&[T_TOP, T_ASK]));
        // A topic already there goes in again; past the end, nowhere [observed].
        q(&mut g, "TopicInfoAddChoice LineA04 TopicAsk 0");
        q(&mut g, "TopicInfoAddChoice LineA04 TopicSkill 9");
        assert_eq!(entries(&mut g, "TopicInfoGetAllChoices LineA04"), hex(&[T_ASK, T_TOP, T_ASK]));
    }

    #[test]
    fn a_choices_edited_flags_and_prompt_reach_the_menu() {
        let f = fixture();
        // Unedited: LineB01 (TopicAsk) is a plain choice and the generic GOODBYE is offered.
        let mut g = game(&f);
        g.said.insert((actor(), FormId(A_FIRST)));
        q(&mut g, "TopicInfoAddChoice LineA04 TopicAsk -1");
        assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_LATER)));
        listen(&mut g);
        let ask = g.dialogue_choices().iter().find(|c| c.topic == FormId(T_ASK)).cloned().expect("TopicAsk offered");
        assert!(!ask.goodbye && !ask.darkened);
        // Goodbye (high byte 1), Always Darken (low byte 2) and a prompt set by script
        // [documented: the TopicInfoSetFlagsHigh and …Low tables; TopicInfoSetPrompt].
        let mut h = game(&f);
        h.said.insert((actor(), FormId(A_FIRST)));
        q(&mut h, "TopicInfoAddChoice LineA04 TopicAsk -1");
        q(&mut h, "TopicInfoSetFlagsHigh LineB01 1");
        q(&mut h, "TopicInfoSetFlagsLow LineB01 2");
        q(&mut h, "TopicInfoSetPrompt LineB01 \"Ask away\"");
        assert_eq!(h.start_dialogue(actor(), None), Ok(FormId(A_LATER)));
        listen(&mut h);
        let ask = h.dialogue_choices().iter().find(|c| c.topic == FormId(T_ASK)).cloned().expect("TopicAsk offered");
        assert!(ask.goodbye, "the edited Goodbye flag");
        assert!(ask.darkened, "the edited Always Darken flag");
        assert!(ask.prompt, "the prompt set by script");
    }

    #[test]
    fn related_topics_link_from_and_follow_ups() {
        let f = fixture();
        let mut g = game(&f);
        // Link From (0): the Added topic is offered after the plain line's topic.
        q(&mut g, "DialogResponseAddRelatedTopic LineF01 TopicLinkPlain 0 0");
        assert_eq!(entries(&mut g, "DialogResponseRelatedGetAll LineF01 0"), hex(&[T_LF_PLAIN]));
        // Passing position == len on a non-empty list does not append, while -1 appends
        // [observed: Wave 4 jg_38_dialog_response_related_add_pos, JohnnyGuitarNVSE 528].
        q(&mut g, "DialogResponseAddRelatedTopic LineF01 TopicAsk 0 1");
        assert_eq!(entries(&mut g, "DialogResponseRelatedGetAll LineF01 0"), hex(&[T_LF_PLAIN]));
        g.start_dialogue(actor(), Some(FormId(T_LF_PLAIN))).unwrap();
        listen(&mut g);
        assert_eq!(offered(&g), vec![T_TOP2, T_TOP, T_ADDED, T_LF_TO, T_SKILL, GOODBYE]);
        // Follow Ups (2) take neither a topic nor a line [observed]; the record's own
        // follow-ups are read.
        let mut h = game(&f);
        q(&mut h, "DialogResponseAddRelatedTopic LineA04 TopicAsk 2 -1");
        q(&mut h, "DialogResponseAddRelatedTopic LineA04 LineB02 2 -1");
        assert!(entries(&mut h, "DialogResponseRelatedGetAll LineA04 2").is_empty());
        assert_eq!(entries(&mut h, "DialogResponseRelatedGetAll LineB01 2"), hex(&[B_FOLLOW]));
    }

    #[test]
    fn flags_reach_the_say_once_rule() {
        let f = fixture();
        let mut g = game(&f);
        // [documented: the TopicInfoSetFlagsHigh and …Low tables: Say Once is 4 in the
        // first byte, Say Once a Day 1 in the second].
        assert_eq!(n(&mut g, "TopicInfoSetFlagsHigh LineA02"), 4.0);
        assert_eq!(n(&mut g, "TopicInfoSetFlagsLow LineE01"), 1.0);
        assert_eq!(n(&mut g, "TopicInfoSetFlagsHigh LineA02 0"), 0.0);
        assert_eq!(n(&mut g, "GetDialogueInfoFlags LineA02"), 0.0);
        // No longer Say Once: said before, it greets again.
        g.said.insert((actor(), FormId(A_FIRST)));
        assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_FIRST)));
        // ITR's setter is the same edit.
        let mut h = game(&f);
        assert_eq!(n(&mut h, "SetDialogueInfoFlags LineA04 1"), 1.0);
        assert_eq!(n(&mut h, "TopicInfoSetFlagsHigh LineA04"), 1.0);
        assert!(h.dialogue_view().info(&FormId(A_LATER)).unwrap().goodbye);
    }

    #[test]
    fn said_once_for_the_session_or_the_save() {
        let f = fixture();
        let mut g = game(&f);
        assert_eq!(n(&mut g, "GetSaidOnce LineA02"), 0.0);
        // Not saved [documented: TopicInfoSetSaidOnce]: the save's said lines stay as
        // they are, and the first greeting is held back.
        assert_eq!(n(&mut g, "TopicInfoSetSaidOnce LineA02 1"), 0.0);
        assert!(!g.said.iter().any(|&(_, i)| i == FormId(A_FIRST)));
        assert_eq!(n(&mut g, "GetSaidOnce LineA02"), 1.0);
        assert_eq!(g.start_dialogue(actor(), None), Ok(FormId(A_LATER)));
        g.end_dialogue();
        // A load drops the session value.
        let session = g.take_session();
        let mut h = game(&f);
        h.put_session(session);
        assert_eq!(n(&mut h, "GetSaidOnce LineA02"), 0.0);
        // saveBake writes the said lines the save keeps [documented: SetSaidOnce].
        assert_eq!(n(&mut h, "SetSaidOnce LineA02 1 1"), 0.0);
        assert!(h.said.contains(&(FormId(0), FormId(A_FIRST))));
        assert_eq!(n(&mut h, "SetSaidOnce LineA02 0 1"), 0.0);
        assert_eq!(n(&mut h, "GetSaidOnce LineA02"), 0.0);
        assert_eq!(n(&mut h, "GetParentTopic LineA02"), f64::from(GREETING));
        assert_eq!(entries(&mut h, "GetTopicInfo TopicAsk"), hex(&[B_ASK, B_FOLLOW]));
    }

    #[test]
    fn response_texts_and_the_session() {
        let f = fixture();
        let mut g = game(&f);
        // [documented: TopicInfoSetNthResponseString, TopicInfoSetResponseStrings].
        assert_eq!(n(&mut g, "TopicInfoSetNthResponseString LineA04 0 \"Back again\""), 1.0);
        assert_eq!(n(&mut g, "TopicInfoSetNthResponseString LineA04 1 \"None\""), 0.0);
        assert_eq!(entries(&mut g, "TopicInfoGetResponseStrings LineA04"), vec!["Back again"]);
        // Returns 1 and leaves the texts as they are [observed: the installed ShowOff].
        assert_eq!(n(&mut g, "TopicInfoSetResponseStrings LineA02 (ar_List \"One\", \"Two.\")"), 1.0);
        assert_eq!(entries(&mut g, "TopicInfoGetResponseStrings LineA02"), vec!["Second.", "First"]);
        q(&mut g, "TopicSetPriority TopicSkill 100");
        // The edits ride across a load [documented: "persistent throughout the game
        // session"].
        let session = g.take_session();
        let mut h = game(&f);
        h.put_session(session);
        assert_eq!(n(&mut h, "TopicSetPriority TopicSkill"), 100.0);
        assert_eq!(entries(&mut h, "TopicInfoGetResponseStrings LineA04"), vec!["Back again"]);
    }
}

#[test]
fn the_no_auto_continue_tweak_holds_each_response_for_a_click() {
    // bNoAutoContinueDialog (docs/design/tweaks.md): time alone never moves the line on.
    let f = fixture();
    let mut g = game(&f);
    g.tweaks.no_auto_continue_dialogue = true;
    g.start_dialogue(actor(), None).unwrap();
    g.frames(600);
    let line = g.dialogue.as_ref().and_then(|c| c.line.clone()).unwrap();
    assert_eq!(line.response, 0, "still the first response after ten seconds");
    // A click goes on to the second response, which waits in its turn.
    assert!(g.skip_response());
    g.frames(600);
    assert_eq!(g.dialogue.as_ref().and_then(|c| c.line.as_ref()).map(|l| l.response), Some(1));
    assert!(g.skip_response());
    assert_eq!(offered(&g), vec![T_ASK, T_BYE]);
}

/// The editor's dialogue debugger (docs/design/editor.md §14): its winner is the line
/// `select_line` takes, on every topic of the fixture, for each gate value.
#[test]
fn the_debugger_agrees_with_the_line_the_game_takes() {
    let f = fixture();
    let listener = Some(FormId::PLAYER_REF);
    let mut topics: Vec<FormId> = f.data.topics.keys().copied().collect();
    topics.sort();
    for gate in [0.0, 31.0, 77.0] {
        for &t in &topics {
            let mut g = game(&f);
            set_global(&mut g, G_GATE, gate);
            let e = g.explain_topic(actor(), listener, t);
            let picked = g.select_line(actor(), listener, t);
            match picked {
                Some(p) => assert!(e.winners.contains(&p), "topic {t} gate {gate}: game took {p}, debugger says {:?}", e.winners),
                None => assert!(e.winners.is_empty(), "topic {t} gate {gate}: nothing said, debugger says {:?}", e.winners),
            }
            assert_eq!(e.lines.len(), f.data.lines_tried(t).len());
        }
    }
}

#[test]
fn the_debugger_shows_pools_and_why_a_line_is_held_back() {
    use fnv_game::dialogue::Blocked;
    let f = fixture();
    let listener = Some(FormId::PLAYER_REF);
    let mut g = game(&f);
    set_global(&mut g, G_GATE, 0.0);
    let e = g.explain_topic(actor(), listener, FormId(T_POOL));
    assert_eq!(e.lines.iter().map(|l| l.info.0).collect::<Vec<_>>(), vec![P1, P2, P_PLAIN, P3, P4]);
    assert_eq!(e.lines.iter().map(|l| l.group).collect::<Vec<_>>(), vec![0, 0, 1, 2, 2]);
    assert_eq!(e.winners, vec![FormId(P1), FormId(P2)]);
    assert!(matches!(&e.lines[2].verdict, Err(Blocked::Conditions(r)) if r.iter().any(|c| !c.passed)));
    let text = g.explain_topic_text(actor(), listener, FormId(T_POOL), true);
    assert!(text.last().unwrap().contains("at random"), "{text:?}");
    // A Say Once greeting once said: held back for that reason, the next line wins.
    let greeting = f.data.greeting.unwrap();
    g.said.insert((actor(), FormId(A_FIRST)));
    let e = g.explain_topic(actor(), listener, greeting);
    let first = e.lines.iter().find(|l| l.info == FormId(A_FIRST)).unwrap();
    assert_eq!(first.verdict, Err(Blocked::SaidOnce));
    assert_eq!(e.winners, vec![FormId(A_LATER)]);
    let text = g.explain_topic_text(actor(), listener, greeting, true);
    assert!(text.iter().any(|l| l.contains("Say Once, and it has been said")), "{text:?}");
}

#[test]
fn a_group_condition_on_the_speaker_lets_others_than_the_fixed_speaker_say_a_line() {
    // #2548: Cottonwood Cove's "Halt!" greeting names the AAM explorer as fixed speaker
    // (ANAM) and tests the explorers' faction; all four explorers say it. A line with
    // a fixed speaker and no condition naming who says it stays the fixed speaker's.
    let f = fixture();
    let mut g = game(&f);
    let (talker, chatter, player) = (actor(), FormId(NPC2_REF), Some(FormId::PLAYER_REF));
    assert!(g.can_say(talker, player, FormId(GR_CREW)), "in the faction the line tests");
    assert!(!g.can_say(talker, player, FormId(GR_FIXED)), "not the fixed speaker");
    assert!(!g.can_say(chatter, player, FormId(GR_CREW)), "the fixed speaker, but not in the faction");
    assert!(g.can_say(chatter, player, FormId(GR_FIXED)));
}

#[test]
fn set_on_dialog_topic_event_handler_fires_for_info_topic_and_wildcard_filters() {
    let f = fixture();
    let mut g = game(&f);
    let run = |g: &mut Game, body: &str| {
        let src = format!("Begin Function {{ }}\n{body}\nEnd\n");
        g.run_query(&src, fnv_game::game::RunCtx::default()).unwrap();
    };
    // Register an INFO-filtered handler (using bare hex FormID like Casino Rewards Fix),
    // a DIAL topic-filtered handler, an unrelated INFO handler (must not fire), and an
    // unfiltered wildcard handler.
    let info_hex = format!("{C_BYE:08X}");
    let other_hex = format!("{I_BYE:08X}");
    let topic_hex = format!("{T_BYE:08X}");
    run(
        &mut g,
        &format!(
            "SetOnDialogTopicEventHandler (begin function {{ref rForm}}\n\
             PlayerREF.AuxVarSetRef \"*dt_info_self\" (GetSelf)\n\
             PlayerREF.AuxVarSetRef \"*dt_info_form\" rForm\n\
             end) 1 {info_hex}"
        ),
    );
    run(
        &mut g,
        &format!(
            "SetOnDialogTopicEventHandler (begin function {{ref rForm}}\n\
             PlayerREF.AuxVarSetRef \"*dt_topic_self\" (GetSelf)\n\
             PlayerREF.AuxVarSetRef \"*dt_topic_form\" rForm\n\
             end) 1 {topic_hex}"
        ),
    );
    run(
        &mut g,
        &format!(
            "SetOnDialogTopicEventHandler (begin function {{ref rForm}}\n\
             PlayerREF.AuxVarSetFlt \"*dt_other\" 1\n\
             end) 1 {other_hex}"
        ),
    );
    run(
        &mut g,
        "SetOnDialogTopicEventHandler (begin function {ref rForm}\n\
         PlayerREF.AuxVarSetRef \"*dt_wild_form\" rForm\n\
         PlayerREF.AuxVarSetFlt \"*dt_wild\" ((PlayerREF.AuxVarGetFlt \"*dt_wild\") + 1)\n\
         end) 1",
    );
    g.start_dialogue(actor(), None).unwrap();
    listen(&mut g);
    assert_eq!(pick(&mut g, T_BYE), FormId(C_BYE));
    let aux_flt = |g: &mut Game, key: &str| -> f64 {
        let src = format!("Begin Function {{ }}\nSetFunctionValue (PlayerREF.AuxVarGetFlt \"{key}\")\nEnd\n");
        g.run_query(&src, fnv_game::game::RunCtx::default()).unwrap().num().unwrap()
    };
    let aux_ref = |g: &mut Game, key: &str| -> u32 {
        let src = format!("Begin Function {{ }}\nSetFunctionValue (PlayerREF.AuxVarGetRef \"{key}\")\nEnd\n");
        g.run_query(&src, fnv_game::game::RunCtx::default()).unwrap().value().unwrap().as_form().unwrap_or(0)
    };
    assert_eq!(aux_ref(&mut g, "*dt_info_self"), NPC1_REF, "INFO-filtered handler gets speaker as GetSelf");
    assert_eq!(aux_ref(&mut g, "*dt_info_form"), C_BYE, "INFO-filtered handler receives the spoken INFO form");
    assert_eq!(aux_ref(&mut g, "*dt_topic_self"), NPC1_REF, "topic-filtered handler gets speaker as GetSelf");
    assert_eq!(aux_ref(&mut g, "*dt_topic_form"), T_BYE, "topic-filtered handler receives the spoken DIAL topic form");
    assert_eq!(aux_flt(&mut g, "*dt_other"), 0.0, "non-matching INFO filter must not fire");
    assert_eq!(aux_ref(&mut g, "*dt_wild_form"), T_BYE, "unfiltered wildcard handler receives the spoken topic");
    // Greeting + T_BYE = 2 spoken lines for the unfiltered wildcard handler.
    assert_eq!(aux_flt(&mut g, "*dt_wild"), 2.0, "unfiltered wildcard handler fired for both greeting and topic");
}

/// #3406: Fallout 3 speech challenge pass-chance formula (`Game::speech_challenge_chance`)
/// follows GECK wiki Speech Formula (Fallout 3): Speech, Difficulty 1..=5, Charisma,
/// base disposition vs SetDisposition override, Responsibility x Karma, weapon-out penalty,
/// auto-success threshold, non-negative factor floor, and GMST overrides.
#[test]
fn fo3_speech_challenge_formula_matches_documented_terms() {
    use fnv_game::actor_values::{CHARISMA, KARMA, RESPONSIBILITY, SPEECH};
    let mut f = fixture();
    // Give Talker a typical Megaton NPC disposition_base (35) and neutral responsibility (50).
    let npc = f.data.actors.get_mut(&FormId(NPC1)).unwrap();
    npc.disposition_base = 35;
    npc.responsibility = Some(50);

    let mut g = game(&f);
    let set_av = |g: &mut Game, target: FormId, code: u16, v: f64| {
        if code == CHARISMA {
            g.extra.entry(target).or_default().actor_values.insert(code, v);
        } else {
            fnv_game::functions::set_actor_value_current(g, target, code, v).unwrap();
        }
    };
    let approx = |a: f64, b: f64| assert!((a - b).abs() < 1e-6, "got {a}, expected {b}");

    // Out-of-range difficulties produce 0.0.
    set_av(&mut g, FormId::PLAYER_REF, SPEECH, 50.0);
    set_av(&mut g, FormId::PLAYER_REF, CHARISMA, 5.0);
    set_av(&mut g, FormId::PLAYER_REF, KARMA, 0.0);
    assert_eq!(g.speech_challenge_chance(actor(), 0), 0.0);
    assert_eq!(g.speech_challenge_chance(actor(), 6), 0.0);

    // Baseline: disp 35 -> disp_val 1.05; Speech 50 -> speech_val 1.0; all 5 difficulties.
    // Very Easy: 1.05 * 1.0 * 0.90 = 0.945 > fSpeechAutoSuccessThreshold (0.9) -> 1.0.
    approx(g.speech_challenge_chance(actor(), 1), 1.0);
    // Easy: 1.05 * 1.0 * 0.60 = 0.63.
    approx(g.speech_challenge_chance(actor(), 2), 0.63);
    // Average: 1.05 * 1.0 * 0.45 = 0.4725.
    approx(g.speech_challenge_chance(actor(), 3), 0.4725);
    // Hard: 1.05 * 1.0 * 0.30 = 0.315.
    approx(g.speech_challenge_chance(actor(), 4), 0.315);
    // Very Hard: 1.05 * 1.0 * 0.20 = 0.21.
    approx(g.speech_challenge_chance(actor(), 5), 0.21);

    // Speech 0 -> speech_val 0.0 -> 0% on every difficulty.
    set_av(&mut g, FormId::PLAYER_REF, SPEECH, 0.0);
    assert_eq!(g.speech_challenge_chance(actor(), 1), 0.0);

    // Low Speech 15 -> speech_val 0.30; Average -> 1.05 * 0.30 * 0.45 = 0.14175.
    set_av(&mut g, FormId::PLAYER_REF, SPEECH, 15.0);
    approx(g.speech_challenge_chance(actor(), 3), 0.14175);

    // Charisma scaling: CHA 10 (+50 -> disp 85 -> disp_val 1.55) vs CHA 1 (-40 -> disp -5 -> disp_val 0.65).
    set_av(&mut g, FormId::PLAYER_REF, SPEECH, 50.0);
    set_av(&mut g, FormId::PLAYER_REF, CHARISMA, 10.0);
    approx(g.speech_challenge_chance(actor(), 4), 1.55 * 1.0 * 0.30);
    set_av(&mut g, FormId::PLAYER_REF, CHARISMA, 1.0);
    approx(g.speech_challenge_chance(actor(), 4), 0.65 * 1.0 * 0.30);

    // SetDisposition override replaces base + Charisma without double-counting Charisma.
    g.extra.entry(actor()).or_default().dispositions.insert(FormId::PLAYER_REF, 50.0);
    // disp_val = ((50 - 30) * 1.0) / 100 + 1 = 1.20; Hard -> 1.20 * 1.0 * 0.30 = 0.36.
    approx(g.speech_challenge_chance(actor(), 4), 0.36);
    g.extra.get_mut(&actor()).unwrap().dispositions.clear();

    // Weapon drawn penalty: fDispTargetWeaponBase = -10.0.
    set_av(&mut g, FormId::PLAYER_REF, CHARISMA, 5.0);
    g.extra.entry(FormId::PLAYER_REF).or_default().weapon_out = true;
    // disp = 35 - 10 = 25 -> disp_val = 0.95; Hard -> 0.95 * 1.0 * 0.30 = 0.285.
    approx(g.speech_challenge_chance(actor(), 4), 0.285);
    g.extra.entry(FormId::PLAYER_REF).or_default().weapon_out = false;

    // Responsibility x Karma: lawful (resp 75) + good karma (+500) -> karma_val = (75 - 50) * (500 * 0.002) = +25.
    // disp = 35 + 25 = 60 -> disp_val = 1.30; Hard -> 1.30 * 1.0 * 0.30 = 0.39.
    set_av(&mut g, actor(), RESPONSIBILITY, 75.0);
    set_av(&mut g, FormId::PLAYER_REF, KARMA, 500.0);
    approx(g.speech_challenge_chance(actor(), 4), 0.39);

    // Evil NPC (resp 25) + evil karma (-500) -> karma_val = (25 - 50) * (-500 * 0.002) = +25 -> 0.39 too.
    set_av(&mut g, actor(), RESPONSIBILITY, 25.0);
    set_av(&mut g, FormId::PLAYER_REF, KARMA, -500.0);
    approx(g.speech_challenge_chance(actor(), 4), 0.39);

    // Negative factor guard: extreme negative disposition x negative speech must clamp to 0.0, never positive.
    // GMST overrides take effect: insert settings into the form store and override via World::change_setting.
    drop(g);
    f.forms.settings.insert("fspeechchallengespeechbase".into(), fnv_sim::settings::Setting::Float(60.0));
    f.forms.settings.insert("fspeechautosuccessthreshold".into(), fnv_sim::settings::Setting::Float(0.9));
    let mut g = game(&f);
    set_av(&mut g, actor(), RESPONSIBILITY, 100.0);
    set_av(&mut g, FormId::PLAYER_REF, KARMA, -2000.0);
    set_av(&mut g, FormId::PLAYER_REF, SPEECH, 0.0);
    assert_eq!(g.speech_challenge_chance(actor(), 1), 0.0);

    // GMST overrides take effect (fSpeechAutoSuccessThreshold raised above 1.0 disables auto-success snap).
    g.world.change_setting("fSpeechChallengeSpeechBase", fnv_sim::settings::Setting::Float(50.0)).unwrap();
    g.world.change_setting("fSpeechAutoSuccessThreshold", fnv_sim::settings::Setting::Float(1.5)).unwrap();
    set_av(&mut g, actor(), RESPONSIBILITY, 50.0);
    set_av(&mut g, FormId::PLAYER_REF, KARMA, 0.0);
    set_av(&mut g, FormId::PLAYER_REF, SPEECH, 50.0);
    approx(g.speech_challenge_chance(actor(), 1), 0.945);
}

/// #3406: A Fallout 3 speech challenge (`SPEECH_CHALLENGE` flag + DNAM, even with no KNAM)
/// attaches `Challenge::chance`, rolls on choose, speaks the topic-linked
/// `SpeechChallengeFailure` line on failure without running the pass line's result scripts
/// or awarding XP, marks a Say Once challenge spent, and speaks the pass line on success.
#[test]
fn fo3_speech_challenge_without_knam_shows_chance_and_rolls_to_failure_topic() {
    use fnv_game::actor_values::SPEECH;
    use fnv_game::misc_stats::{SPEECH_FAILURES, SPEECH_SUCCESSES};
    let mut f = fixture();
    // Turn S_LF_PLAIN into a Fallout 3 style Say Once speech challenge (Average, NO KNAM!),
    // with a begin result script that sets G_OUT = 1 (from A_FIRST's begin unit).
    let pass_begin = f.data.infos[&FormId(A_FIRST)].begin;
    let fail_begin = f.data.infos[&FormId(A_BARK)].begin;
    assert!(pass_begin.is_some() && fail_begin.is_some());
    {
        let pass = f.data.infos.get_mut(&FormId(S_LF_PLAIN)).unwrap();
        pass.flags |= info_flag::SPEECH_CHALLENGE | info_flag::SAY_ONCE;
        pass.say_once = true;
        pass.challenge_form = None;
        pass.challenge_difficulty = Some(3); // Average (55)
        pass.begin = pass_begin;
        // Offer T_LF_PLAIN from T_TOP2 alongside T_BYE so we can test both failure and retry prevention.
        f.data.infos.get_mut(&FormId(E_NOW)).unwrap().choices = vec![FormId(T_LF_PLAIN), FormId(T_BYE)];
        // Give S_FAIL (already in SpeechChallengeFailure with link_from = [T_LF_PLAIN]) a distinct begin script.
        f.data.infos.get_mut(&FormId(S_FAIL)).unwrap().begin = fail_begin;
    }

    // 1) Low Speech (0) -> 0% chance -> guaranteed failure -> SpeechChallengeFailure line S_FAIL.
    {
        let mut g = at_topic_list(&f);
        fnv_game::functions::set_actor_value_current(&mut g, FormId::PLAYER_REF, SPEECH, 0.0).unwrap();
        pick(&mut g, T_TOP2);
        listen(&mut g);
        let offered_ch = g.dialogue_choices().iter().find(|c| c.topic == FormId(T_LF_PLAIN)).unwrap();
        let ch = offered_ch.challenge.as_ref().expect("FO3 speech challenge without KNAM must still attach Challenge");
        assert_eq!(ch.form, FormId(AVIF), "falls back to AVSpeech");
        assert_eq!(ch.difficulty, Some(3));
        assert_eq!(ch.chance, Some(0.0));
        assert_eq!((ch.needed, ch.have), (None, None));

        // Choosing it fails: speaks S_FAIL instead of S_LF_PLAIN.
        let spoken = pick(&mut g, T_LF_PLAIN);
        assert_eq!(spoken, FormId(S_FAIL));
        assert_eq!(g.misc_stat(SPEECH_FAILURES), 1.0);
        assert_eq!(g.misc_stat(SPEECH_SUCCESSES), 0.0);
        assert_eq!(g.player.xp, 0.0);
        // S_FAIL's begin script ran (sets G_OUT = 3), NOT S_LF_PLAIN's pass script (G_OUT = 1).
        assert_eq!(g.world.global(FormId(G_OUT)).unwrap(), 3.0);

        // After S_FAIL finishes, S_LF_PLAIN (Say Once) is marked said and cannot be retried.
        listen(&mut g);
        assert!(!g.can_say(actor(), Some(FormId::PLAYER_REF), FormId(S_LF_PLAIN)));
        assert!(!offered(&g).contains(&T_LF_PLAIN));
    }

    // 2) High Speech (100) + high disposition (85) -> auto-success (1.0) -> speaks pass line S_LF_PLAIN.
    {
        let mut g = at_topic_list(&f);
        fnv_game::functions::set_actor_value_current(&mut g, FormId::PLAYER_REF, SPEECH, 100.0).unwrap();
        g.extra.entry(actor()).or_default().dispositions.insert(FormId::PLAYER_REF, 85.0);
        // Very Easy so XP = 5 is also verified on pass.
        // (Keep Average here; check XP on Very Easy below, and verify pass line + script + stat.)
        pick(&mut g, T_TOP2);
        listen(&mut g);
        let offered_ch = g.dialogue_choices().iter().find(|c| c.topic == FormId(T_LF_PLAIN)).unwrap();
        assert_eq!(offered_ch.challenge.as_ref().unwrap().chance, Some(1.0));

        let spoken = pick(&mut g, T_LF_PLAIN);
        assert_eq!(spoken, FormId(S_LF_PLAIN));
        assert_eq!(g.misc_stat(SPEECH_SUCCESSES), 1.0);
        assert_eq!(g.misc_stat(SPEECH_FAILURES), 0.0);
        // S_LF_PLAIN's begin script ran (sets G_OUT = 1).
        assert_eq!(g.world.global(FormId(G_OUT)).unwrap(), 1.0);
    }
}

/// #3406: When a failed challenge topic has no `Link From` line in `SpeechChallengeFailure`,
/// an unlinked failure line is spoken instead of another topic's linked line; when no
/// failure line is sayable at all, the pass line is still not spoken and a Say Once
/// challenge is removed from the offered choices.
#[test]
fn fo3_speech_challenge_failure_fallback_and_missing_failure_line() {
    use fnv_game::actor_values::SPEECH;
    use fnv_game::misc_stats::{SPEECH_FAILURES, SPEECH_SUCCESSES};
    let mut f = fixture();
    // Make S_LF_TO a Say Once Very Hard challenge (no KNAM). Note S_FAIL links from T_LF_PLAIN, NOT T_LF_TO!
    {
        let pass = f.data.infos.get_mut(&FormId(S_LF_TO)).unwrap();
        pass.flags |= info_flag::SPEECH_CHALLENGE | info_flag::SAY_ONCE;
        pass.say_once = true;
        pass.challenge_form = None;
        pass.challenge_difficulty = Some(5);
        pass.link_from.clear();
        f.data.infos.get_mut(&FormId(E_NOW)).unwrap().choices = vec![FormId(T_LF_TO), FormId(T_BYE)];
    }

    // Case A: S_FAIL links from T_LF_PLAIN (a different topic), and no unlinked failure line exists.
    // Failing T_LF_TO must NOT speak S_FAIL (wrong topic) and must NOT speak S_LF_TO (pass line).
    {
        let mut g = at_topic_list(&f);
        fnv_game::functions::set_actor_value_current(&mut g, FormId::PLAYER_REF, SPEECH, 0.0).unwrap();
        pick(&mut g, T_TOP2);
        listen(&mut g);
        let spoken = pick(&mut g, T_LF_TO);
        assert_eq!(spoken, FormId(0), "must not speak pass line or wrong-topic failure line");
        assert_eq!((g.misc_stat(SPEECH_SUCCESSES), g.misc_stat(SPEECH_FAILURES)), (0.0, 1.0));
        assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::NoLine { topic, .. } if topic == FormId(T_FAIL))));
        // Failed Say Once choice was removed; T_BYE remains offered.
        assert_eq!(offered(&g), vec![T_BYE]);
    }

    // Case B: Clear S_FAIL's link_from so it becomes an unlinked generic failure line (the
    // 113 unlinked lines in Fallout3.esm). Failing T_LF_TO now falls back to S_FAIL.
    f.data.infos.get_mut(&FormId(S_FAIL)).unwrap().link_from.clear();
    {
        let mut g = at_topic_list(&f);
        fnv_game::functions::set_actor_value_current(&mut g, FormId::PLAYER_REF, SPEECH, 0.0).unwrap();
        pick(&mut g, T_TOP2);
        listen(&mut g);
        let spoken = pick(&mut g, T_LF_TO);
        assert_eq!(spoken, FormId(S_FAIL), "falls back to unlinked SpeechChallengeFailure line");
        assert_eq!((g.misc_stat(SPEECH_SUCCESSES), g.misc_stat(SPEECH_FAILURES)), (0.0, 1.0));
    }
}

/// #3406: An intermediate pass chance (~47.25%) produces both passes and failures across
/// seeded runs rather than always passing.
#[test]
fn fo3_speech_challenge_intermediate_chance_rolls_probabilistically() {
    use fnv_game::actor_values::{CHARISMA, SPEECH};
    use fnv_game::misc_stats::{SPEECH_FAILURES, SPEECH_SUCCESSES};
    let mut f = fixture();
    f.data.actors.get_mut(&FormId(NPC1)).unwrap().disposition_base = 35;
    {
        let pass = f.data.infos.get_mut(&FormId(S_LF_PLAIN)).unwrap();
        pass.flags |= info_flag::SPEECH_CHALLENGE | info_flag::SAY_ONCE;
        pass.say_once = true;
        pass.challenge_form = None;
        pass.challenge_difficulty = Some(3); // Average: 1.05 * 1.0 * 0.45 = 0.4725
        f.data.infos.get_mut(&FormId(E_NOW)).unwrap().choices = vec![FormId(T_LF_PLAIN), FormId(T_BYE)];
    }

    let mut passes = 0u32;
    let mut failures = 0u32;
    for seed in 1u64..=64 {
        let mut g = game_seeded(&f, seed);
        g.start_dialogue(actor(), None).unwrap();
        listen(&mut g);
        pick(&mut g, T_BYE);
        listen(&mut g);
        g.start_dialogue(actor(), None).unwrap();
        listen(&mut g);
        fnv_game::functions::set_actor_value_current(&mut g, FormId::PLAYER_REF, SPEECH, 50.0).unwrap();
        g.extra.entry(FormId::PLAYER_REF).or_default().actor_values.insert(CHARISMA, 5.0);
        pick(&mut g, T_TOP2);
        listen(&mut g);
        let spoken = pick(&mut g, T_LF_PLAIN);
        if spoken == FormId(S_LF_PLAIN) {
            assert_eq!((g.misc_stat(SPEECH_SUCCESSES), g.misc_stat(SPEECH_FAILURES)), (1.0, 0.0));
            passes += 1;
        } else {
            assert_eq!(spoken, FormId(S_FAIL));
            assert_eq!((g.misc_stat(SPEECH_SUCCESSES), g.misc_stat(SPEECH_FAILURES)), (0.0, 1.0));
            failures += 1;
        }
    }
    assert_eq!(passes + failures, 64);
    assert!(passes >= 15 && failures >= 15, "expected ~47% passes across 64 seeds, got {passes} passes / {failures} failures");
}
