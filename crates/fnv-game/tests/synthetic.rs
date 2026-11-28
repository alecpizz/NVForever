//! Layer 5 behaviour on a synthetic plugin built in memory (no game data): a start-game
//! quest with a timer script and stage results (one with conditions), an activator
//! whose OnActivate block sets a stage and shows a message, an actor with two AI
//! packages, the runner contract, menus and loading screens, and snapshots. Bytecode is written by hand in the format of docs/spec-notes/scripts.md §2.

use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::functions::Ctx;
use fnv_game::game::{ActMode, FurnitureUse, Game, GameConfig, InstKey};
use fnv_game::trace::EventKind;
use fnv_game::GameData;
use fnv_script::vm::{ArgValue, Value};
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::{json, Value as Json};

#[path = "common/inline_call_source_bridge.rs"]
mod inline_call_source_bridge_tests;

// ---- plugin builders ------------------------------------------------------------

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

fn tes4() -> Vec<u8> {
    let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat();
    record(b"TES4", 1, 0, &[sub(b"HEDR", hedr)])
}

/// SCHR + SCDA + variables (`(index, name, 1 = Integer / 0 = FloatOrRef)`) + SCRO list.
fn unit(kind: u16, scda: &[u8], vars: &[(u32, &str, u8)], refs: &[u32]) -> Vec<Subrecord> {
    let mut schr = vec![0u8; 4];
    schr.extend((refs.len() as u32).to_le_bytes());
    schr.extend((scda.len() as u32).to_le_bytes());
    schr.extend((vars.len() as u32).to_le_bytes());
    schr.extend(kind.to_le_bytes());
    schr.extend(1u16.to_le_bytes());
    let mut out = vec![sub(b"SCHR", schr)];
    if !scda.is_empty() {
        out.push(sub(b"SCDA", scda.to_vec()));
    }
    for (i, n, t) in vars {
        let mut slsd = i.to_le_bytes().to_vec();
        slsd.extend([0u8; 12]);
        slsd.push(*t);
        slsd.extend([0u8; 7]);
        out.push(sub(b"SLSD", slsd));
        out.push(sub(b"SCVR", z(n)));
    }
    for r in refs {
        out.push(sub(b"SCRO", r.to_le_bytes().to_vec()));
    }
    out
}

/// A CTDA: comparison type byte, value, function index, two parameters, run on Subject.
fn ctda(ty: u8, value: f32, function: u16, p1: u32, p2: u32) -> Subrecord {
    let mut d = vec![ty, 0, 0, 0];
    d.extend(value.to_le_bytes());
    d.extend(function.to_le_bytes());
    d.extend([0, 0]);
    d.extend(p1.to_le_bytes());
    d.extend(p2.to_le_bytes());
    d.extend(0u32.to_le_bytes());
    d.extend(0u32.to_le_bytes());
    sub(b"CTDA", d)
}

// ---- bytecode builders (scripts.md §2) -----------------------------------------

fn le16(v: u16) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}
fn st(op: u16, body: &[u8]) -> Vec<u8> {
    [le16(op), le16(body.len() as u16), body.to_vec()].concat()
}
fn ex(tokens: &[Vec<u8>]) -> Vec<u8> {
    tokens.iter().flat_map(|t| [vec![b' '], t.clone()].concat()).collect()
}
fn a(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}
fn var(letter: u8, i: u16) -> Vec<u8> {
    [vec![letter], le16(i)].concat()
}
fn set(target: Vec<u8>, e: Vec<u8>) -> Vec<u8> {
    st(0x15, &[target, le16(e.len() as u16), e].concat())
}
fn if_(skip: u16, e: Vec<u8>) -> Vec<u8> {
    st(0x16, &[le16(skip), le16(e.len() as u16), e].concat())
}
fn endif() -> Vec<u8> {
    st(0x19, &[])
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
fn xcall(op: u16, blob: &[u8]) -> Vec<u8> {
    [vec![b'X'], le16(op), le16(blob.len() as u16), blob.to_vec()].concat()
}
/// ScriptName, then `Begin <event> [params]` … `End` around `body`.
/// One block: event code, parameter block, body.
type BlockBytes = (u16, Option<Vec<u8>>, Vec<u8>);

fn script(blocks: &[BlockBytes]) -> Vec<u8> {
    let mut out = st(0x1D, &[]);
    for (event, params, body) in blocks {
        let end = st(0x11, &[]);
        let len = (body.len() + end.len()) as u32;
        let mut b = [le16(*event), len.to_le_bytes().to_vec()].concat();
        if let Some(p) = params {
            b.extend(p);
        }
        out.extend(st(0x10, &b));
        out.extend(body);
        out.extend(end);
    }
    out
}

const GLOBAL: u32 = 0x800;
const MESSAGE: u32 = 0x810;
const QUEST: u32 = 0x900;
const QUEST_SCRIPT: u32 = 0x901;
const ACT_SCRIPT: u32 = 0x902;
const ACTIVATOR: u32 = 0x903;
const TRIG_SCRIPT: u32 = 0x904;
const TRIGGER: u32 = 0x905;
const MARKER: u32 = 0x906;
const WEAPON: u32 = 0x907;
const WEAPON_SCRIPT: u32 = 0x908;
const PACK_GATED: u32 = 0x910;
const PACK_IDLE: u32 = 0x911;
const PACK_SCRIPTED: u32 = 0x912;
const PACK_EVENTS: u32 = 0x913;
const NPC: u32 = 0x920;
const NPC_SCRIPT: u32 = 0x921;
const NPC_AUTO: u32 = 0x922;
const TOPIC: u32 = 0x930;
const INFO_A: u32 = 0x931;
const INFO_B: u32 = 0x932;
const CELL: u32 = 0x200;
const OTHER_CELL: u32 = 0x201;
const ACT_REF: u32 = 0x300;
const TRIG_REF: u32 = 0x301;
const NPC_REF: u32 = 0x320;
const NPC_AUTO_REF: u32 = 0x321;
const PATROL_REF: u32 = 0x330;
const OTHER_REF: u32 = 0x331;
const TELEPORT_DOOR: u32 = 0x940;
const DOOR_REF: u32 = 0x340;
const DEST_DOOR_REF: u32 = 0x341;
const DOOR_POS: [f32; 3] = [11.0, 22.0, 33.0];

const SET_STAGE: u16 = 0x1039;
const GET_SECONDS_PASSED: u16 = 0x100C;
const SET_OBJECTIVE_DISPLAYED: u16 = 0x11A3;
const SHOW_MESSAGE: u16 = 0x1059;
const GET_STAGE_DONE_INDEX: u16 = 0x3B;
const GET_DISABLED_INDEX: u16 = 0x23;

fn plugin_bytes() -> Vec<u8> {
    let glob = record(b"GLOB", 0, GLOBAL, &[sub(b"EDID", z("TestGlobal")), sub(b"FNAM", vec![b's']), sub(b"FLTV", 5f32.to_le_bytes().to_vec())]);
    let mesg = record(
        b"MESG",
        0,
        MESSAGE,
        &[sub(b"EDID", z("TestMsg")), sub(b"FULL", z("Renamed One")), sub(b"DESC", z("Hello")), sub(b"ITXT", z("Yes")), sub(b"ITXT", z("No"))],
    );
    // Quest script: fTimer (1, float), bRun (2, short).
    //   begin GameMode
    //     if bRun == 1
    //       set fTimer to fTimer - GetSecondsPassed
    //       if fTimer < 0
    //         SetStage Q 20
    //         set bRun to 0
    //       endif
    //     endif
    //   end
    let body = [
        if_(5, ex(&[var(b's', 2), a("1"), a("==")])),
        set(var(b'f', 1), ex(&[var(b'f', 1), xcall(GET_SECONDS_PASSED, &[]), a("-")])),
        if_(2, ex(&[var(b'f', 1), a("0"), a("<")])),
        call(SET_STAGE, &[r(1), n(20)]),
        set(var(b's', 2), ex(&[a("0")])),
        endif(),
        endif(),
    ]
    .concat();
    let qscript = [vec![sub(b"EDID", z("QScript"))], unit(1, &script(&[(0, None, body)]), &[(1, "fTimer", 0), (2, "bRun", 1)], &[QUEST])].concat();
    // Activator script: nUsed (1, short), nHit (2, short).
    //   begin OnActivate player
    //     set nUsed to nUsed + 1
    //     SetStage Q 10
    //     ShowMessage TestMsg
    //   end
    //   begin OnHit player
    //     set nHit to nHit + 10
    //   end
    //   begin OnHitWith
    //     set nHit to nHit + 1
    //   end
    //   begin OnHitWith Q (the quest's form id standing in for a form list in the test)
    //     set nHit to nHit + 100
    //   end
    //   begin GameMode
    //     set nFrames to nFrames + 1
    //   end
    //   begin MenuMode 1 / 1007 / 1056
    //     set nPip / nLoad / nVats to … + 1
    //   end
    //   begin OnLoad
    //     set nOnLoad to nOnLoad + 1
    //   end
    let body = [
        set(var(b's', 1), ex(&[var(b's', 1), a("1"), a("+")])),
        call(SET_STAGE, &[r(2), n(10)]),
        call(SHOW_MESSAGE, &[[r(3), le16(0), 0i32.to_le_bytes().to_vec()].concat()]),
    ]
    .concat();
    let actscript = [
        vec![sub(b"EDID", z("ActScript"))],
        unit(
            0,
            &script(&[
                (2, Some([le16(1), r(1)].concat()), body),
                (0x08, Some([le16(1), r(1)].concat()), set(var(b's', 2), ex(&[var(b's', 2), a("10"), a("+")]))),
                (0x09, None, set(var(b's', 2), ex(&[var(b's', 2), a("1"), a("+")]))),
                (0x09, Some([le16(1), r(2)].concat()), set(var(b's', 2), ex(&[var(b's', 2), a("100"), a("+")]))),
                (0, None, set(var(b's', 3), ex(&[var(b's', 3), a("1"), a("+")]))),
                (1, Some([le16(1), n(1)].concat()), set(var(b's', 4), ex(&[var(b's', 4), a("1"), a("+")]))),
                (1, Some([le16(1), n(1007)].concat()), set(var(b's', 5), ex(&[var(b's', 5), a("1"), a("+")]))),
                (1, Some([le16(1), n(1056)].concat()), set(var(b's', 6), ex(&[var(b's', 6), a("1"), a("+")]))),
                (0x15, None, set(var(b's', 7), ex(&[var(b's', 7), a("1"), a("+")]))),
                (1, Some([le16(1), n(1013)].concat()), set(var(b's', 8), ex(&[var(b's', 8), a("1"), a("+")]))),
            ]),
            &[(1, "nUsed", 1), (2, "nHit", 1), (3, "nFrames", 1), (4, "nPip", 1), (5, "nLoad", 1), (6, "nVats", 1), (7, "nOnLoad", 1), (8, "nStart", 1)],
            &[0x14, QUEST, MESSAGE],
        ),
    ]
    .concat();
    // NPC script: nDone (1), nFrames (3), nOnLoad (7), all short counters.
    //   begin SayToDone
    //     set nDone to nDone + 1
    //   end
    let body = set(var(b's', 1), ex(&[var(b's', 1), a("1"), a("+")]));
    let npcscript = [
        vec![sub(b"EDID", z("NpcScript"))],
        unit(
            0,
            &script(&[
                (0x07, None, body),
                (0, None, set(var(b's', 3), ex(&[var(b's', 3), a("1"), a("+")]))),
                (0x15, None, set(var(b's', 7), ex(&[var(b's', 7), a("1"), a("+")]))),
            ]),
            &[(1, "nDone", 1), (3, "nFrames", 1), (7, "nOnLoad", 1)],
            &[],
        ),
    ]
    .concat();
    // Trigger script: fSum (1, float); no GameMode block.
    //   begin OnTrigger
    //     set fSum to fSum + GetSecondsPassed
    //   end
    let body = set(var(b'f', 1), ex(&[var(b'f', 1), xcall(GET_SECONDS_PASSED, &[]), a("+")]));
    let trigscript = [vec![sub(b"EDID", z("TrigScript"))], unit(0, &script(&[(0x18, None, body)]), &[(1, "fSum", 0)], &[])].concat();
    let scpt = [
        record(b"SCPT", 0, QUEST_SCRIPT, &qscript),
        record(b"SCPT", 0, TRIG_SCRIPT, &trigscript),
        record(b"SCPT", 0, ACT_SCRIPT, &actscript),
        record(b"SCPT", 0, NPC_SCRIPT, &npcscript),
        // Weapon script: begin OnFire / set TestGlobal to 9 / end.
        record(b"SCPT", 0, WEAPON_SCRIPT, &[vec![sub(b"EDID", z("WeaponScript"))], unit(0, &script(&[(0x24, None, set(var(b'G', 1), ex(&[a("9")])))]), &[], &[GLOBAL])].concat()),
    ]
    .concat();
    // The quest: start game enabled, delay 0.5 s.
    let mut q = vec![
        sub(b"EDID", z("Q")),
        sub(b"SCRI", QUEST_SCRIPT.to_le_bytes().to_vec()),
        sub(b"DATA", [vec![1u8, 0, 0, 0], 0.5f32.to_le_bytes().to_vec()].concat()),
    ];
    // Stage 10: set Q.bRun to 1 ; set Q.fTimer to 1
    q.push(sub(b"INDX", 10i16.to_le_bytes().to_vec()));
    q.push(sub(b"QSDT", vec![0]));
    let s10 = [set([r(1), var(b's', 2)].concat(), ex(&[a("1")])), set([r(1), var(b'f', 1)].concat(), ex(&[a("1.0")]))].concat();
    q.extend(unit(0, &s10, &[], &[QUEST]));
    // Stage 20 (Complete Quest): SetObjectiveDisplayed Q 10 1
    q.push(sub(b"INDX", 20i16.to_le_bytes().to_vec()));
    q.push(sub(b"QSDT", vec![1]));
    q.extend(unit(0, &call(SET_OBJECTIVE_DISPLAYED, &[r(1), n(10), n(1)]), &[], &[QUEST]));
    // Stage 30: two entries; the first if GetStageDone Q 20 == 1, the second if
    // GetStageDone Q 99 == 1 (never). Each sets TestGlobal.
    q.push(sub(b"INDX", 30i16.to_le_bytes().to_vec()));
    q.push(sub(b"QSDT", vec![0]));
    q.push(ctda(0, 1.0, GET_STAGE_DONE_INDEX, QUEST, 20));
    q.extend(unit(0, &set(var(b'G', 1), ex(&[a("7")])), &[], &[GLOBAL]));
    q.push(sub(b"QSDT", vec![0]));
    q.push(ctda(0, 1.0, GET_STAGE_DONE_INDEX, QUEST, 99));
    q.extend(unit(0, &set(var(b'G', 1), ex(&[a("99")])), &[], &[GLOBAL]));
    q.push(sub(b"QOBJ", 10i32.to_le_bytes().to_vec()));
    q.push(sub(b"NNAM", z("Do the test")));
    // Three targets: the activator once stage 30 is done, the NPC (ignoring locks) until
    // then, and the activator again while it is enabled (GetDisabled run on the subject,
    // which is the target).
    let qsta = |target: u32, flags: u8| sub(b"QSTA", [target.to_le_bytes().to_vec(), vec![flags, 0, 0, 0]].concat());
    q.push(qsta(ACT_REF, 0));
    q.push(ctda(0, 1.0, GET_STAGE_DONE_INDEX, QUEST, 30));
    q.push(qsta(NPC_REF, 1));
    q.push(ctda(0, 0.0, GET_STAGE_DONE_INDEX, QUEST, 30));
    q.push(qsta(ACT_REF, 0));
    q.push(ctda(0, 0.0, GET_DISABLED_INDEX, 0, 0));
    let qust = record(b"QUST", 0, QUEST, &q);
    let acti = record(b"ACTI", 0, ACTIVATOR, &[sub(b"EDID", z("TestActivator")), sub(b"SCRI", ACT_SCRIPT.to_le_bytes().to_vec())]);
    let trig = record(b"ACTI", 0, TRIGGER, &[sub(b"EDID", z("TestTrigger")), sub(b"SCRI", TRIG_SCRIPT.to_le_bytes().to_vec())]);
    let trig_ref = record(
        b"REFR",
        0,
        TRIG_REF,
        &[sub(b"EDID", z("TestTriggerREF")), sub(b"NAME", TRIGGER.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])],
    );
    let refr = record(
        b"REFR",
        0,
        ACT_REF,
        &[sub(b"EDID", z("TestActivatorREF")), sub(b"NAME", ACTIVATOR.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])],
    );
    // Two AI packages: the first only once stage 20 is done; an actor runs both, in
    // that order. A third one for AddScriptPackage.
    let pack = [
        record(b"PACK", 0, PACK_GATED, &[sub(b"EDID", z("GatedPackage")), ctda(0, 1.0, GET_STAGE_DONE_INDEX, QUEST, 20)]),
        record(b"PACK", 0, PACK_IDLE, &[sub(b"EDID", z("IdlePackage"))]),
        record(b"PACK", 0, PACK_SCRIPTED, &[sub(b"EDID", z("ScriptedPackage"))]),
        // Begin, end and change result scripts that set TestGlobal to 21, 22 and 23.
        record(b"PACK", 0, PACK_EVENTS, &{
            let mut subs = vec![sub(b"EDID", z("EventsPackage"))];
            for (marker, value) in [(b"POBA", "21"), (b"POEA", "22"), (b"POCA", "23")] {
                subs.push(sub(marker, vec![]));
                subs.extend(unit(0, &set(var(b'G', 1), ex(&[a(value)])), &[], &[GLOBAL]));
            }
            subs
        }),
    ]
    .concat();
    let npc = record(
        b"NPC_",
        0,
        NPC,
        &[
            sub(b"EDID", z("TestNpc")),
            // ACBS: flags, fatigue, barter gold, level 4, calc min/max, speed, karma,
            // disposition, template flags.
            sub(b"ACBS", [&0u32.to_le_bytes()[..], &[0; 4], &4u16.to_le_bytes(), &[0; 6], &0f32.to_le_bytes(), &[0; 4]].concat()),
            // DATA: Base Health 30, S.P.E.C.I.A.L. 5 6 6 5 5 5 5 (Endurance 6).
            sub(b"DATA", [&30i32.to_le_bytes()[..], &[5, 6, 6, 5, 5, 5, 5]].concat()),
            sub(b"SCRI", NPC_SCRIPT.to_le_bytes().to_vec()),
            sub(b"PKID", PACK_GATED.to_le_bytes().to_vec()),
            sub(b"PKID", PACK_IDLE.to_le_bytes().to_vec()),
        ],
    );
    let achr = record(
        b"ACHR",
        0,
        NPC_REF,
        &[sub(b"EDID", z("TestNpcREF")), sub(b"NAME", NPC.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])],
    );
    // An NPC with "Auto-calc stats" (ACBS flag bit 4), level 5, Base Health 10, Endurance
    // 5: the NCR troopers' template (NCRTrooperAAMTEMPLATE).
    let npc_auto = record(
        b"NPC_",
        0,
        NPC_AUTO,
        &[
            sub(b"EDID", z("TestAutoNpc")),
            sub(b"ACBS", [&0x10u32.to_le_bytes()[..], &[0; 4], &5u16.to_le_bytes(), &[0; 6], &0f32.to_le_bytes(), &[0; 4]].concat()),
            sub(b"DATA", [&10i32.to_le_bytes()[..], &[5, 5, 5, 5, 5, 5, 5]].concat()),
        ],
    );
    let achr_auto = record(
        b"ACHR",
        0,
        NPC_AUTO_REF,
        &[sub(b"EDID", z("TestAutoNpcREF")), sub(b"NAME", NPC_AUTO.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])],
    );
    // A topic with two Say Once lines of the quest; their end scripts set TestGlobal to
    // 3 and 4.
    let line = |id: u32, text: &str, value: &str| {
        let mut subs = vec![sub(b"DATA", vec![0, 0, 4, 0]), sub(b"QSTI", QUEST.to_le_bytes().to_vec()), sub(b"NAM1", z(text)), sub(b"NEXT", vec![])];
        subs.extend(unit(0, &set(var(b'G', 1), ex(&[a(value)])), &[], &[GLOBAL]));
        record(b"INFO", 0, id, &subs)
    };
    let dial = [
        record(b"DIAL", 0, TOPIC, &[sub(b"EDID", z("TestTopic"))]),
        group(TOPIC.to_le_bytes(), 7, &[line(INFO_A, "Hello there", "3"), line(INFO_B, "Bye", "4")].concat()),
    ]
    .concat();
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
    // A patrol point with a patrol script (after XPPA) that sets TestGlobal to 8, on an
    // unscripted marker base.
    let marker = record(b"ACTI", 0, MARKER, &[sub(b"EDID", z("TestMarker"))]);
    let patrol_ref = record(
        b"REFR",
        0,
        PATROL_REF,
        &[
            &[sub(b"EDID", z("TestPatrolPointREF")), sub(b"NAME", MARKER.to_le_bytes().to_vec()), sub(b"XPPA", vec![])][..],
            &unit(0, &set(var(b'G', 1), ex(&[a("8")])), &[], &[GLOBAL]),
            &[sub(b"DATA", vec![0; 24])],
        ]
        .concat(),
    );
    let door = record(b"DOOR", 0, TELEPORT_DOOR, &[sub(b"EDID", z("TestTeleportDoor"))]);
    let destination: Vec<u8> = DOOR_POS.into_iter().chain([0.0, 0.0, 0.75]).flat_map(f32::to_le_bytes).collect();
    let door_ref = record(b"REFR", 0, DOOR_REF, &[
        sub(b"EDID", z("TestDoorREF")), sub(b"NAME", TELEPORT_DOOR.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24]),
        // XTEL: destination door, position and rotation, No Alarm (u32) [xEdit REFR XTEL].
        sub(b"XTEL", [DEST_DOOR_REF.to_le_bytes().to_vec(), destination, 0u32.to_le_bytes().to_vec()].concat()),
    ]);
    let dest_door = record(b"REFR", 0, DEST_DOOR_REF, &[
        sub(b"NAME", TELEPORT_DOOR.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24]),
    ]);
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &[refr, trig_ref, achr, achr_auto, patrol_ref, door_ref].concat()));
    // A second cell, with one unscripted reference to move things to.
    let other_cell = record(b"CELL", 0, OTHER_CELL, &[sub(b"EDID", z("TestOtherCell"))]);
    let other_ref = record(b"REFR", 0, OTHER_REF, &[sub(b"EDID", z("TestOtherREF")), sub(b"NAME", MARKER.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let other_children = group(OTHER_CELL.to_le_bytes(), 6, &group(OTHER_CELL.to_le_bytes(), 9, &[other_ref, dest_door].concat()));
    [
        tes4(),
        top(b"GLOB", &glob),
        top(b"MESG", &mesg),
        top(b"SCPT", &scpt),
        top(b"ACTI", &[acti, trig, marker].concat()),
        top(b"DOOR", &door),
        top(b"NPC_", &[npc, npc_auto].concat()),
        top(b"WEAP", &record(b"WEAP", 0, WEAPON, &[sub(b"EDID", z("TestWeapon")), sub(b"SCRI", WEAPON_SCRIPT.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 15])])),
        top(b"PACK", &pack),
        top(b"QUST", &qust),
        top(b"DIAL", &dial),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children, other_cell, other_children].concat()))),
    ]
    .concat()
}

fn schema() -> Schema {
    Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap()
}

fn catalogue() -> (FunctionTable, Json) {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    (FunctionTable::load(&p).unwrap(), j)
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Test.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = schema();
    let (table, j) = catalogue();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() })
}

fn q() -> FormId {
    FormId(QUEST)
}

#[test]
fn data_reads_quest_stages_conditions_and_units() {
    let f = fixture();
    let d = &f.data;
    let qd = &d.quests[&q()];
    assert!(qd.start_enabled());
    assert_eq!(qd.script, Some(FormId(QUEST_SCRIPT)));
    assert_eq!(qd.script_delay(), 0.5);
    assert_eq!(qd.stages.keys().copied().collect::<Vec<_>>(), vec![10, 20, 30]);
    assert_eq!(qd.stages[&20][0].flags, 1);
    assert_eq!(qd.stages[&30].len(), 2);
    assert_eq!(qd.stages[&30][0].conditions.len(), 1);
    assert_eq!(qd.stages[&30][0].conditions[0].opcode(), 0x103B);
    assert!(qd.stages[&10][0].unit.is_some());
    assert_eq!(qd.objectives[&10].text.as_deref(), Some("Do the test"));
    let s = d.script(FormId(QUEST_SCRIPT)).unwrap();
    assert_eq!(s.raw, 0);
    assert_eq!(s.var_index("bRun"), Some(2));
    assert_eq!(d.script_of_base(FormId(ACTIVATOR)), Some(FormId(ACT_SCRIPT)));
    assert_eq!(d.scripted_by_cell[&FormId(CELL)], vec![FormId(ACT_REF), FormId(TRIG_REF), FormId(NPC_REF)]);
    assert_eq!(d.messages[&FormId(MESSAGE)].buttons, vec!["Yes", "No"]);
}

/// An answered box its script never reads stays in `message_box` but no longer waits
/// (#1649: saving was refused for a whole session after a mod's start-up warning box).
#[test]
fn an_answered_box_no_longer_waits_though_its_button_is_unread() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    assert!(g.message_box.as_ref().is_some_and(|b| b.waiting()));
    g.press_button(0).unwrap();
    let b = g.message_box.as_ref().expect("kept for its script's GetButtonPressed");
    assert_eq!(b.pressed, Some(0));
    assert!(!b.waiting());
}

#[test]
fn hand_activation_runs_on_activate_instead_of_the_default() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    assert!(g.quest_state(q()).running);
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    assert_eq!(g.var(InstKey::Ref { reference: FormId(ACT_REF) }, 1), Value::Num(1.0));
    let s = g.quest_state(q());
    assert!(s.stages_done.contains(&10));
    assert_eq!(s.stage(), 10);
    // The stage result wrote the quest's variables through `Q.var`.
    assert_eq!(g.var(InstKey::Quest { quest: q() }, 2), Value::Num(1.0));
    assert_eq!(g.var(InstKey::Quest { quest: q() }, 1), Value::Num(1.0));
    assert!(g.message_box.is_some());
    assert!(g.events.iter().any(|e| matches!(&e.kind, EventKind::MessageShown { text: Some(t), .. } if t == "Hello")));
    // The default action did not run.
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::Activated { .. })));
    // Activation by another actor is not accepted by `Begin OnActivate player`.
    g.activate(FormId(ACT_REF), FormId(ACT_REF), ActMode::Hand).unwrap();
    assert_eq!(g.var(InstKey::Ref { reference: FormId(ACT_REF) }, 1), Value::Num(1.0));
}

#[test]
fn the_quest_timer_runs_at_the_quest_delay_and_sets_the_complete_stage() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.set_stage(q(), 10);
    // The script runs every 0.5 s; GetSecondsPassed gives 0 on the first call, then
    // the time since the previous run: after about 1.5 s fTimer is below 0.
    g.frames(60);
    assert!(!g.quest_state(q()).stages_done.contains(&20));
    g.frames(60);
    let s = g.quest_state(q());
    assert!(s.stages_done.contains(&20), "{:?}", g.var(InstKey::Quest { quest: q() }, 1));
    assert!(s.completed && s.running);
    assert!(s.objectives[&10].displayed);
    assert_eq!(g.var(InstKey::Quest { quest: q() }, 2), Value::Num(0.0));
    // A stopped quest's script does not run.
    g.stop_quest(q());
    g.store(InstKey::Quest { quest: q() }, Some(FormId(QUEST_SCRIPT)), 2, Value::Num(1.0), "test", true);
    let before = g.var(InstKey::Quest { quest: q() }, 1);
    g.frames(120);
    assert_eq!(g.var(InstKey::Quest { quest: q() }, 1), before);
}

/// #3465 (VMS49's helper quest): a quest that ran, stopped and is started again with a
/// timer set runs the timer its full time: the first run's GetSecondsPassed is not the
/// time the quest stood stopped.
#[test]
fn a_quest_started_again_runs_its_timer_from_the_start() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.set_stage(q(), 10);
    g.frames(120);
    assert!(g.quest_state(q()).stages_done.contains(&20), "the first timer ran out");
    g.stop_quest(q());
    // Ten seconds stopped.
    g.frames(600);
    // A timer of 1 s set, then the quest started (as the trigger script does).
    let key = InstKey::Quest { quest: q() };
    g.store(key, Some(FormId(QUEST_SCRIPT)), 1, Value::Num(1.0), "test", true);
    g.store(key, Some(FormId(QUEST_SCRIPT)), 2, Value::Num(1.0), "test", true);
    g.start_quest(q());
    // Half a second on: the timer has not run out.
    g.frames(30);
    assert_eq!(g.var(key, 2), Value::Num(1.0), "still running: fTimer {:?}", g.var(key, 1));
    let Value::Num(left) = g.var(key, 1) else { panic!() };
    assert!(left > 0.3 && left <= 1.0, "{left}");
    // And it runs out about a second later.
    g.frames(90);
    assert_eq!(g.var(key, 2), Value::Num(0.0), "ran out: fTimer {:?}", g.var(key, 1));
}

/// #4851 (Lucky 38's pod timer): the first run after a start counts the time since the
/// start, as VMS40Timer's first drop did (layer5-captures.md §5.4), so a timer started
/// again runs out at its time plus at most one quest delay, not one run later.
#[test]
fn a_quest_started_again_counts_its_first_run_from_the_start() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.set_stage(q(), 10);
    g.frames(120);
    g.stop_quest(q());
    g.frames(600);
    let key = InstKey::Quest { quest: q() };
    g.store(key, Some(FormId(QUEST_SCRIPT)), 1, Value::Num(1.0), "test", true);
    g.store(key, Some(FormId(QUEST_SCRIPT)), 2, Value::Num(1.0), "test", true);
    g.start_quest(q());
    // Just past the first run (the quest delay is 0.5 s): it took off the half second
    // since the start.
    g.frames(32);
    let Value::Num(left) = g.var(key, 1) else { panic!() };
    assert!((0.4..=0.6).contains(&left), "first run after the start: fTimer {left}");
    // Out by 1.0 s and one delay more (0.5 s), not a run later.
    g.frames(64);
    assert_eq!(g.var(key, 2), Value::Num(0.0), "ran out: fTimer {:?}", g.var(key, 1));
}

#[test]
fn get_seconds_passed_counts_frames_in_a_trigger_script_without_game_mode() {
    // OnTrigger runs every frame the player is inside; with `Game::trigger_frame` before
    // it, each run's GetSecondsPassed is the time since the previous frame's run, so the
    // sum is the time from the first run to the last (VMS15's detonator spot beeps
    // every 0.5 s by such a timer).
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let t = FormId(TRIG_REF);
    let inside = |g: &mut Game| {
        g.trigger_frame(t);
        assert!(g.ref_event(t, fnv_game::game::event::ON_TRIGGER, Some(Value::Ref(FormId::PLAYER_REF.0)), Some(FormId::PLAYER_REF)));
    };
    inside(&mut g);
    let first = g.time;
    for _ in 0..60 {
        g.frames(1);
        inside(&mut g);
    }
    let elapsed = g.time - first;
    let Value::Num(sum) = g.var(InstKey::Ref { reference: t }, 1) else { panic!("fSum is not a number") };
    assert!(elapsed > 0.5, "{elapsed}");
    // The clock is read in 1/64 s steps.
    assert!((sum - elapsed).abs() <= 2.0 / 64.0, "sum {sum}, elapsed {elapsed}");
}

#[test]
fn get_seconds_passed_counts_a_trigger_frame_once_however_many_runs_it_has() {
    // Two references inside a trigger: its OnTrigger runs twice a frame, and only the
    // first run gets the frame's time (`Game::trigger_again`; Hidden Valley's virus
    // purge, issue #2455, ran out several times too fast).
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let t = FormId(TRIG_REF);
    let run = |g: &mut Game| assert!(g.ref_event(t, fnv_game::game::event::ON_TRIGGER, Some(Value::Ref(FormId::PLAYER_REF.0)), Some(FormId::PLAYER_REF)));
    let inside_twice = |g: &mut Game| {
        g.trigger_frame(t);
        run(g);
        g.trigger_again(t);
        run(g);
    };
    inside_twice(&mut g);
    let first = g.time;
    for _ in 0..60 {
        g.frames(1);
        inside_twice(&mut g);
    }
    let elapsed = g.time - first;
    let Value::Num(sum) = g.var(InstKey::Ref { reference: t }, 1) else { panic!("fSum is not a number") };
    assert!(elapsed > 0.5, "{elapsed}");
    assert!((sum - elapsed).abs() <= 2.0 / 64.0, "sum {sum}, elapsed {elapsed}");
}

#[test]
fn stage_entries_run_when_their_conditions_pass_and_done_stages_do_not_rerun() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.set_stage(q(), 20);
    g.set_stage(q(), 30);
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 7.0);
    // The second entry's condition (stage 99 done) fails.
    assert!(g.events.iter().any(|e| matches!(&e.kind, EventKind::StageSet { stage: 30, entries, .. } if entries == &vec![0])));
    // Again: the quest does not allow repeated stages.
    g.world.set_global(FormId(GLOBAL), 0.0).unwrap();
    g.set_stage(q(), 30);
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 0.0);
    // An undefined stage does nothing; GetStage stays the highest.
    assert!(!g.set_stage(q(), 15));
    g.set_stage(q(), 10);
    assert_eq!(g.quest_state(q()).stage(), 30);
}

#[test]
fn quest_targets_are_marked_while_their_conditions_pass() {
    let f = fixture();
    let t = &f.data.quests[&q()].objectives[&10].targets;
    assert_eq!(t.iter().map(|t| (t.target, t.ignores_locks, t.conditions.len())).collect::<Vec<_>>(), vec![
        (Some(FormId(ACT_REF)), false, 1),
        (Some(FormId(NPC_REF)), true, 1),
        (Some(FormId(ACT_REF)), false, 1)
    ]);
    let mut g = game(&f);
    g.new_game();
    // Not shown yet: no target.
    assert!(g.check_quest_targets(q()).is_empty());
    g.set_stage(q(), 20);
    let shown = |g: &mut Game| g.check_quest_targets(q()).iter().map(|c| (c.index, c.shown)).collect::<Vec<_>>();
    assert_eq!(shown(&mut g), vec![(0, false), (1, true), (2, true)]);
    // Stage 30 swaps the alternatives.
    g.set_stage(q(), 30);
    assert_eq!(shown(&mut g), vec![(0, true), (1, false), (2, true)]);
    // The subject is the target: disabling it fails its own GetDisabled == 0.
    g.world.set_enabled(FormId(ACT_REF), false).unwrap();
    assert_eq!(shown(&mut g), vec![(0, true), (1, false), (2, false)]);
    // A completed objective has no targets. (Stage 20 completed the quest, and a
    // completed quest's objectives no longer change, #2958: complete it as
    // CompleteAllObjectives does.)
    assert!(!g.set_objective(q(), 10, None, Some(true)));
    assert_eq!(shown(&mut g), vec![(0, true), (1, false), (2, false)]);
    g.set_objective_whatever(q(), 10, None, Some(true));
    assert!(g.check_quest_targets(q()).is_empty());
}

/// #2958 (layer5-captures.md §5.8): once a quest is completed or failed, SetStage still marks the
/// stage and runs its entries but does not start the quest again, and
/// SetObjectiveDisplayed / SetObjectiveCompleted do nothing, running or not. A quest
/// stopped without being completed starts again on both.
#[test]
fn a_completed_quest_stays_stopped_and_its_objectives_no_longer_change() {
    let f = fixture();
    let objective = |g: &mut Game| g.quest(q()).objectives.get(&10).map_or((false, false), |o| (o.displayed, o.completed));
    // Stopped, not completed: SetStage starts it, and so does displaying an objective.
    let mut g = game(&f);
    g.new_game();
    g.set_stage(q(), 10);
    g.stop_quest(q());
    assert!(!g.quest_state(q()).running);
    g.set_stage(q(), 30);
    assert!(g.quest_state(q()).running);
    g.stop_quest(q());
    assert!(g.set_objective(q(), 10, Some(true), None));
    assert!(g.quest_state(q()).running);
    assert_eq!(objective(&mut g), (true, false));
    // Completed (stage 20 has the Complete Quest flag) and stopped.
    let mut g = game(&f);
    g.new_game();
    g.set_stage(q(), 20);
    assert!(g.quest_state(q()).completed);
    g.stop_quest(q());
    let before = objective(&mut g);
    g.world.set_global(FormId(GLOBAL), 0.0).unwrap();
    assert!(g.set_stage(q(), 30));
    assert!(g.quest_state(q()).stages_done.contains(&30), "the stage is marked done");
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 7.0, "and its entry ran");
    assert!(!g.quest_state(q()).running, "but the quest is not started again");
    // Its objectives no longer change, stopped or started again.
    for running in [false, true] {
        if running {
            g.start_quest(q());
            assert!(g.quest_state(q()).running);
        }
        assert!(!g.set_objective(q(), 10, Some(!before.0), None));
        assert!(!g.set_objective(q(), 10, None, Some(!before.1)));
        assert_eq!(objective(&mut g), before);
    }
    // CompleteAllObjectives' way still completes one.
    assert!(g.set_objective_whatever(q(), 10, None, Some(true)));
    assert!(objective(&mut g).1);
}

/// Displaying an objective already shown keeps its completed flag; showing a hidden one
/// clears it (observer check 40, issue #1108; scripts.md §5).
#[test]
fn redisplaying_a_completed_objective_keeps_it_completed() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let state = |g: &mut Game| {
        let o = &g.quest(q()).objectives[&10];
        (o.displayed, o.completed)
    };
    g.set_objective(q(), 10, Some(true), None);
    g.set_objective(q(), 10, None, Some(true));
    g.set_objective(q(), 10, Some(true), None);
    assert_eq!(state(&mut g), (true, true));
    g.set_objective(q(), 10, Some(false), None);
    assert_eq!(state(&mut g), (false, true));
    g.set_objective(q(), 10, Some(true), None);
    assert_eq!(state(&mut g), (true, false));
}

/// ResetQuest keeps a SetQuestDelay override and clears the rest (observer check 41,
/// issue #1109; layer5-captures.md §5.7).
#[test]
fn reset_quest_keeps_the_set_quest_delay() {
    const RESET_QUEST: u16 = 0x11DB;
    const SET_QUEST_DELAY: u16 = 0x11DC;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let quest_call = |g: &mut Game, op: u16, args: &[ArgValue]| {
        let ctx = Ctx { unit: "test".into(), opcode: op, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        g.call_function(&ctx, args);
    };
    let quest_arg = ArgValue::Value(Value::Ref(QUEST));
    quest_call(&mut g, SET_QUEST_DELAY, &[quest_arg.clone(), ArgValue::Value(Value::Num(0.1))]);
    g.set_stage(q(), 20);
    quest_call(&mut g, RESET_QUEST, &[quest_arg]);
    let s = g.quest(q());
    assert!(s.delay.is_some_and(|d| (d - 0.1).abs() < 1e-6), "{:?}", s.delay);
    assert!(s.stages_done.is_empty());
}

/// ResetQuest from another script stops the quest (o41, issue #1232); from the quest's
/// own script it keeps running (the Emergency Radio's self-reset, not observed).
#[test]
fn reset_quest_stops_the_quest_unless_its_own_script() {
    const RESET_QUEST: u16 = 0x11DB;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let reset = |g: &mut Game, from: Option<FormId>| {
        let ctx = Ctx { unit: "test".into(), opcode: RESET_QUEST, on: None, self_ref: None, action_ref: None, quest: from, inst: None, in_condition: false };
        g.call_function(&ctx, &[ArgValue::Value(Value::Ref(QUEST))]);
    };
    g.quest(q()).running = true;
    reset(&mut g, Some(q()));
    assert!(g.quest(q()).running, "a self-reset keeps the quest running");
    reset(&mut g, None);
    assert!(!g.quest(q()).running, "a reset from another script stops it");
}

#[test]
fn snapshots_round_trip() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    g.frames(10);
    // A lock broken by forcing it stays broken, and the lockpicking help stays shown.
    g.locks.broken.insert(FormId(ACT_REF));
    g.locks.help_shown = true;
    // What the save adds (docs/design/save-load.md §3.2): terminals, radio, the weather
    // shown, long-lived provocations.
    g.terminals.access.insert(FormId(ACT_REF), fnv_game::terminal::Access::Hacked);
    g.radio.pipboy.on = true;
    g.radio.pipboy.station = Some(FormId(ACT_REF));
    g.radio.npc_radios.insert(FormId::PLAYER_REF, FormId(ACT_REF));
    g.weather.shown = Some(FormId(ACT_REF));
    g.weather.raining = true;
    g.provocations.grudges.insert((FormId(ACT_REF), FormId::PLAYER_REF), fnv_game::provocation::Grudge { why: fnv_game::provocation::Why::Assault, since: 3.0 });
    g.provocations.forgiven.insert(FormId(ACT_REF), 7.5);
    // A gate a script opened (PlayGroup Forward) shows and collides open after a load
    // (issue #1396).
    g.ask_anim(fnv_game::anims::AnimRequest { on: FormId(ACT_REF), group: 1, name: Some("Forward".into()), start: fnv_game::anims::Start::Normal });
    let s = g.snapshot();
    let text = serde_json::to_string(&s).unwrap();
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&text).unwrap();
    assert_eq!(back, s);
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert_eq!(h.locks, g.locks);
    assert!(text.contains("\"locks\""), "{text}");
    assert_eq!(h.terminals.access, g.terminals.access);
    assert_eq!((h.radio.pipboy.on, h.radio.pipboy.station), (true, Some(FormId(ACT_REF))));
    assert_eq!(h.radio.npc_radios, g.radio.npc_radios);
    assert_eq!((h.weather.shown, h.weather.raining), (g.weather.shown, true));
    assert_eq!(h.provocations.grudges, g.provocations.grudges);
    assert_eq!(h.provocations.forgiven, g.provocations.forgiven);
    assert_eq!(h.anims.asked(FormId(ACT_REF)), Some((1, "Forward")));
    // A snapshot written before these fields existed still loads.
    let mut old: serde_json::Value = serde_json::from_str(&text).unwrap();
    for k in ["terminals", "pipboy_radio_on", "pipboy_station", "npc_radios", "weather_shown", "raining", "grudges", "forgiven", "object_groups"] {
        old.as_object_mut().unwrap().remove(k);
    }
    let older: fnv_game::game::GameSnapshot = serde_json::from_value(old).unwrap();
    assert!(older.terminals.is_empty() && older.grudges.is_empty() && older.object_groups.is_empty());
    g.frames(90);
    h.frames(90);
    assert_eq!(g.quests, h.quests);
    assert_eq!(g.instances, h.instances);
}

/// Players 2–4 of a co-op game (docs/design/coop-save.md): each keeps its own actor
/// (inventory, place), values (health damage), character and effects through a save,
/// apart from the game's own player and from each other; the quests stay shared.
#[test]
fn coop_players_round_trip_apart_from_the_player() {
    use fnv_game::coop::{CoopError, CoopPlayer};
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.world.get(FormId::PLAYER_REF).unwrap();
    // A one-player snapshot writes no `coop`.
    assert!(!serde_json::to_string(&g.snapshot()).unwrap().contains("\"coop\""));
    let health = 16u16;
    let mut p2 = CoopPlayer::copy_of_player(&g, 2).unwrap();
    p2.character.name = Some("Second".into());
    p2.character.xp = 250.0;
    p2.character.perks.insert(FormId(0xABC), 1);
    p2.actor.pos = [10.0, 20.0, 30.0];
    p2.actor.inventory.get_or_insert_with(fnv_sim::Inventory::new).add(FormId(0x4242), 3);
    p2.extra.av_damage.insert(health, 40.0);
    p2.heading_deg = 90.0;
    let mut p4 = CoopPlayer::copy_of_player(&g, 4).unwrap();
    p4.character.name = Some("Fourth".into());
    p4.extra.dead = true;
    g.set_coop_player(p4).unwrap();
    g.set_coop_player(p2.clone()).unwrap();
    assert_eq!(g.coop.iter().map(|p| p.slot).collect::<Vec<_>>(), vec![2, 4]);
    assert_eq!(g.set_coop_player(CoopPlayer { slot: 1, ..p2.clone() }), Err(CoopError::BadSlot(1)));
    assert_eq!(g.set_coop_player(CoopPlayer { slot: 5, ..p2.clone() }), Err(CoopError::BadSlot(5)));
    g.set_stage(FormId(QUEST), 10);
    let s = g.snapshot();
    let text = serde_json::to_string(&s).unwrap();
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&text).unwrap();
    assert_eq!(back, s);
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert_eq!(h.coop, g.coop);
    assert_eq!(h.coop_player(2), Some(&p2));
    assert!(h.coop_player(3).is_none());
    assert!(h.coop_player(4).unwrap().extra.dead);
    // The game's own player kept none of it.
    assert_eq!(h.player.name, g.player.name);
    assert!(h.player.perks.is_empty());
    assert!(!h.extra.get(&FormId::PLAYER_REF).is_some_and(|e| e.dead || e.av_damage.contains_key(&health)));
    assert_ne!(h.world.peek(FormId::PLAYER_REF).unwrap().pos, [10.0, 20.0, 30.0]);
    // The quests are saved once, not per player.
    assert_eq!(h.quests, g.quests);
    assert_eq!(text.matches("\"quests\"").count(), 1);
    // A snapshot naming one slot twice is refused.
    let mut twice = back.clone();
    twice.coop.push(twice.coop[0].clone());
    let world = World::restore(&f.forms, &f.refs, &twice.sim).unwrap();
    assert!(Game::restore(&f.data, world, g.cfg, &twice).is_err());
}

/// Network co-op's parts of a save (docs/design/coop-save.md §5) are optional: a solo game
/// writes none of them, a load gives it none, and set they come back as they went (a
/// character's ID and claim ledger, the host's presence record).
#[test]
fn network_co_ops_save_fields_are_absent_solo_and_round_trip_when_set() {
    use fnv_game::coop_joint::ClaimedStage;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.world.get(FormId::PLAYER_REF).unwrap();
    let fields = ["\"character_id\"", "\"claims\"", "\"coop_presence\""];
    let solo = g.snapshot();
    let text = serde_json::to_string(&solo).unwrap();
    assert!(fields.iter().all(|f| !text.contains(f)), "a solo save");
    let world = World::restore(&f.forms, &f.refs, &solo.sim).unwrap();
    let h = Game::restore(&f.data, world, g.cfg, &solo).unwrap();
    let again = serde_json::to_string(&h.snapshot()).unwrap();
    assert!(fields.iter().all(|f| !again.contains(f)), "a solo save after a load");
    // Set: a guest's character, and what a host keeps.
    let id = g.ensure_character_id();
    assert_ne!(id, 0);
    g.player.claims.stages.entry(FormId(QUEST)).or_default().push(ClaimedStage { stage: 10, line: None });
    g.player.claims.markers.insert(FormId(0x0000_1234));
    g.coop_presence.stages.entry(FormId(QUEST)).or_default().entry(10).or_default().insert(id);
    let s = g.snapshot();
    let text = serde_json::to_string(&s).unwrap();
    assert!(fields.iter().all(|f| text.contains(f)));
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&text).unwrap();
    assert_eq!(back, s);
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert_eq!(h.player.character_id, id);
    assert_eq!(h.player.claims, g.player.claims);
    assert!(h.coop_presence.was_there(FormId(QUEST), 10, id));
}

/// A co-op guest's save: its own game as it was when it joined, with the
/// character as it is in the host's world; the place, quests and time stay its own.
#[test]
fn a_guests_save_keeps_its_own_place_and_world_with_the_character_now() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.world.get(FormId::PLAYER_REF).unwrap();
    let solo = g.snapshot();
    let p = FormId::PLAYER_REF;
    let at_home = solo.sim.refs[&p].clone();
    // In the host's world: elsewhere, a quest stage the host's story set, and progress.
    g.set_stage(FormId(QUEST), 10);
    let mut now = g.snapshot();
    let actor = now.sim.refs.get_mut(&p).unwrap();
    actor.pos = [5000.0, -3000.0, 120.0];
    actor.rot = [0.0, 0.0, 1.5];
    actor.inventory.get_or_insert_with(fnv_sim::Inventory::new).add(FormId(0x4242), 3);
    now.player.xp = 750.0;
    now.player.level = Some(4);
    now.player.perks.insert(FormId(0xABC), 1);
    now.extra.entry(p).or_default().av_damage.insert(16, 25.0);
    now.time = solo.time + 600.0;
    let merged = solo.clone().with_character_of(&now);
    let me = &merged.sim.refs[&p];
    assert_eq!((me.cell, me.pos, me.rot), (at_home.cell, at_home.pos, at_home.rot));
    assert_eq!(me.inventory.as_ref().unwrap().count(FormId(0x4242)), 3);
    assert_eq!(merged.player.xp, 750.0);
    assert_eq!(merged.player.level, Some(4));
    assert!(merged.player.perks.contains_key(&FormId(0xABC)));
    assert_eq!(merged.extra[&p].av_damage.get(&16), Some(&25.0));
    assert_eq!(merged.time, now.time);
    assert_eq!(merged.quests, solo.quests);
    assert_ne!(merged.quests, now.quests);
    // It loads.
    let world = World::restore(&f.forms, &f.refs, &merged.sim).unwrap();
    let h = Game::restore(&f.data, world, g.cfg, &merged).unwrap();
    assert_eq!(h.world.peek(p).unwrap().pos, at_home.pos);
    assert_eq!(h.player.xp, 750.0);
}

/// A change that spoils a snapshot.
type SnapshotChange<'a> = Box<dyn Fn(&mut fnv_game::game::GameSnapshot) + 'a>;

/// What a load asks before it starts anything from a save says what the restores would
/// say (#1590): a good snapshot passes, and each refusal is the restores' own.
#[test]
fn a_snapshot_says_whether_the_restores_take_it() {
    use fnv_game::coop::CoopPlayer;
    use fnv_game::game::GameSnapshot;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.world.get(FormId::PLAYER_REF).unwrap();
    let p2 = CoopPlayer::copy_of_player(&g, 2).unwrap();
    let good = g.snapshot();
    let restores = |s: &GameSnapshot| World::restore(&f.forms, &f.refs, &s.sim).and_then(|w| Game::restore(&f.data, w, g.cfg, s).map(|_| ()));
    assert_eq!(good.check(), Ok(()));
    assert_eq!(restores(&good), Ok(()));
    let bad: [(&str, SnapshotChange<'_>); 6] = [
        ("unsupported snapshot format \"fnv.game.state/2\"", Box::new(|s| s.format = "fnv.game.state/2".into())),
        ("unsupported snapshot format \"fnv.sim.state/2\"", Box::new(|s| s.sim.format = "fnv.sim.state/2".into())),
        ("bad rng_word_pos \"x\"", Box::new(|s| s.rng_word_pos = "x".into())),
        ("bad rng_word_pos \"y\"", Box::new(|s| s.sim.rng_word_pos = "y".into())),
        ("given twice", Box::new(|s| s.coop = vec![p2.clone(), p2.clone()])),
        ("slots are", Box::new(|s| s.coop = vec![CoopPlayer { slot: 1, ..p2.clone() }])),
    ];
    for (why, change) in bad {
        let mut s = good.clone();
        change(&mut s);
        let e = s.check().unwrap_err();
        assert!(e.contains(why), "{why}: {e}");
        assert_eq!(restores(&s), Err(e));
    }
}

#[test]
fn a_save_made_with_another_load_order_drops_the_missing_plugins_forms() {
    // docs/design/save-load.md §4.1. The save was made with [Test.esm, Extra.esm] and is
    // loaded with [Test.esm]: everything of Extra.esm (index 0x01) in it goes, the rest
    // comes back as it was.
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    g.frames(10);
    g.radio.npc_radios.insert(FormId::PLAYER_REF, FormId(ACT_REF));
    g.world.set_enabled(FormId(NPC_REF), false).unwrap();
    let s = g.snapshot();
    let mut v = serde_json::to_value(&s).unwrap();
    // Extra.esm's part of the save: a changed reference of its own, a created reference of
    // one of its bases, a global, its quest with a script instance, a form list entry, a
    // radio, and the weather shown.
    let some_ref = v["sim"]["refs"].as_object().unwrap().values().next().cloned().expect("a changed reference");
    let mut own = some_ref.clone();
    own["base"] = json!("0x01000ABC");
    v["sim"]["refs"]["0x01000123"] = some_ref.clone();
    v["sim"]["refs"]["0xFF000099"] = own;
    v["sim"]["globals"]["0x01000800"] = json!(3.0);
    let quest = v["quests"].as_object().unwrap().values().next().cloned().unwrap();
    v["quests"]["0x01000900"] = quest;
    let inst = v["instances"][0][1].clone();
    v["instances"].as_array_mut().unwrap().push(json!([{"kind": "quest", "quest": "0x01000900"}, inst]));
    v["form_lists"]["0x00000100"] = json!(["0x01000001", "0x00000300"]);
    v["npc_radios"]["0x01000321"] = json!("0x00000300");
    v["weather_shown"] = json!("0x01000044");
    let e = |n: &str| fnv_save::PluginEntry { name: n.into(), size: 0 };
    let remap = fnv_save::PluginRemap::new(&[e("Test.esm"), e("Extra.esm")], &[e("Test.esm")]);
    assert_eq!(remap.missing(), vec!["Extra.esm".to_string()]);
    let report = fnv_save::remap_json(&mut v, &remap);
    // Dropped as keys and list entries: the reference, the global, the quest, the form list
    // entry, the radio; left for drop_gone: the created reference's base, the instance's
    // quest, the weather.
    assert_eq!((report.dropped_total(), report.gone_total()), (5, 3), "{report:?}");
    let mut back: fnv_game::game::GameSnapshot = serde_json::from_value(v).unwrap();
    assert_eq!(back.drop_gone(|id| fnv_save::is_gone(id.0)), 3, "the created reference, the quest instance, the weather");
    // The form list keeps its entry of Test.esm.
    assert_eq!(back.form_lists.remove(&FormId(0x100)), Some(vec![FormId(0x300)]));
    assert_eq!(back, s);
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    g.frames(30);
    h.frames(30);
    assert_eq!(g.quests, h.quests);
    assert_eq!(g.instances, h.instances);
}

#[test]
fn the_runner_follows_the_actions_contract() {
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "given": [{"kind": "quest", "quest": "Q", "running": false, "vars": {"fTimer": 0}}],
        "actions": [
            {"op": "get_quest", "quest": "Q", "fields": ["running", "stage", "done.10"], "label": "before"},
            {"op": "player_activate", "target": "TestActivatorREF"},
            {"op": "set_stage_read", "quest": "Q", "stage": 20, "field": "completed", "label": "read in the same script"},
            {"op": "get_quest", "quest": "Q", "fields": ["running", "stage", "completed", "failed", "objective.10.displayed", "var.bRun", "var.nope"], "label": "after"},
            {"op": "set_local", "type": "short", "value": 3000000000.5, "label": "short"},
            {"op": "set_local", "type": "float", "value": -3000000000.0, "label": "float"},
            {"op": "press_button", "button": 1},
            {"op": "get_global", "name": "TestGlobal", "label": "global"}
        ]
    }))
    .unwrap();
    let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    assert_eq!(out["format"], "fnv.script.trace/1");
    let obs = out["observations"].as_array().unwrap();
    let by = |l: &str| obs.iter().find(|o| o["label"] == l).unwrap().clone();
    assert_eq!(by("before")["running"], false);
    assert_eq!(by("before")["done.10"], false);
    assert_eq!(by("read in the same script")["completed"], true);
    let after = by("after");
    assert_eq!(after["running"], true);
    assert_eq!(after["stage"], 20);
    assert_eq!(after["completed"], true);
    assert_eq!(after["failed"], false);
    assert_eq!(after["objective.10.displayed"], true);
    assert!(after["errors"]["var.nope"].is_string());
    assert_eq!(by("short")["value"], -2147483648.0);
    assert_eq!(by("float")["value"], -2147483647.0);
    assert_eq!(by("global")["value"], 5.0);
    assert_eq!(by("before")["step"], 0);
}

#[test]
fn registered_functions_match_the_catalogue() {
    let (table, _) = catalogue();
    for (op, name, _) in fnv_game::functions::registry() {
        let f = table.get(*op).unwrap_or_else(|| panic!("0x{op:04X} {name} is not in functions.json"));
        assert!(f.name.eq_ignore_ascii_case(name), "0x{op:04X}: registered as {name}, catalogue says {}", f.name);
    }
    let mut ops: Vec<u16> = fnv_game::functions::registry().iter().map(|r| r.0).collect();
    let n = ops.len();
    ops.sort();
    ops.dedup();
    assert_eq!(ops.len(), n, "an opcode is registered twice");
}

#[test]
fn console_functions_are_opcodeless_console_functions_of_the_catalogue() {
    let (table, _) = catalogue();
    for (name, _) in fnv_game::functions::console_registry() {
        let op = table.by_name(name).unwrap_or_else(|| panic!("{name} is not in functions.json"));
        let f = table.get(op).unwrap();
        assert!(op >= fnv_script::table::CONSOLE_OPCODE_BASE, "{name}: has the game's opcode 0x{op:04X}; register it in registry()");
        assert_eq!(&f.name, name, "{name}: the catalogue spells it {}", f.name);
    }
    // The console's words reach them by alias.
    assert_eq!(table.by_name("tcl"), table.by_name("ToggleCollision"));
    assert!(fnv_game::functions::lookup_console("togglegrass").is_some());
}

#[test]
fn extender_functions_are_extender_rows_of_the_catalogue_and_names_are_unique() {
    let (_, j) = catalogue();
    let rows = j["functions"].as_array().unwrap();
    let family = |name: &str| {
        rows.iter().find(|f| f["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))).map(|f| f["family"].as_str().unwrap_or_default())
    };
    for (name, _) in fnv_game::functions::ext_registry() {
        assert_eq!(family(name), Some("extender"), "{name}: not an extender function of functions.json");
    }
    let f = fnv_game::functions::registry().iter().map(|r| r.1);
    let named = fnv_game::functions::console_registry().iter().chain(fnv_game::functions::ext_registry()).map(|r| r.0);
    let mut names: Vec<String> = f.chain(named).map(str::to_ascii_lowercase).collect();
    let n = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), n, "a name is registered twice");
    // By name, any case, reaches the same entry as by opcode.
    let by_name = fnv_game::functions::lookup_name("setstage").map(|k| k as *const _);
    assert_eq!(by_name, fnv_game::functions::lookup(0x1039).map(|k| k as *const _));
    assert!(fnv_game::functions::lookup_name("ToggleGrass").is_some());
    assert!(fnv_game::functions::lookup_name("NoSuchFunction").is_none());
}

#[test]
fn aliases_reach_the_function_they_name() {
    let (table, _) = catalogue();
    let ptr = |k: Option<&'static fnv_game::functions::Kind>| k.map(|k| k as *const _);
    // GetAV is GetActorValue's alias; TCL is ToggleCollision's (a console function).
    assert!(fnv_game::functions::lookup_name("GetAV").is_none());
    assert_eq!(ptr(fnv_game::functions::lookup_alias(&table, "getav")), ptr(fnv_game::functions::lookup(0x100E)));
    assert_eq!(ptr(fnv_game::functions::lookup_alias(&table, "TCL")), ptr(fnv_game::functions::lookup_name("ToggleCollision")));
    assert!(fnv_game::functions::lookup_alias(&table, "NoSuchFunction").is_none());
    assert_eq!(table.get(0x100E).and_then(|f| f.origin.as_deref()), Some("GECK1"));
}

#[test]
fn menus_and_loading_screens_stop_game_time_for_scripts() {
    // The quest timer of the_quest_timer_runs_at_the_quest_delay_and_sets_the_complete_stage,
    // with a long menu and a loading screen in the middle: neither counts.
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.set_stage(q(), 10);
    g.frames(30);
    let t0 = g.time;
    g.menu = Some(1008);
    g.frames(600);
    g.menu = None;
    g.loading = true;
    g.frames(600);
    g.loading = false;
    assert_eq!(g.time, t0);
    assert!(!g.paused());
    g.frames(60);
    assert!(!g.quest_state(q()).stages_done.contains(&20));
    g.frames(60);
    assert!(g.quest_state(q()).stages_done.contains(&20));
}

#[test]
fn only_the_script_that_showed_a_message_box_gets_its_button() {
    use fnv_game::functions::Ctx;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    g.message_box.as_mut().unwrap().pressed = Some(1);
    let ctx = |inst| Ctx { unit: "test".into(), opcode: 0x101F, on: None, self_ref: None, action_ref: None, quest: None, inst, in_condition: false };
    // Another script (the quest's) does not see the box, which stays.
    assert_eq!(g.call_function(&ctx(Some(InstKey::Quest { quest: q() })), &[]), Value::Num(-1.0));
    assert!(g.message_box.is_some());
    // The activator's script gets the button once.
    let owner = ctx(Some(InstKey::Ref { reference: FormId(ACT_REF) }));
    assert_eq!(g.call_function(&owner, &[]), Value::Num(1.0));
    assert_eq!(g.call_function(&owner, &[]), Value::Num(-1.0));
}

#[test]
fn a_hidden_button_cannot_be_pressed() {
    // A button whose conditions failed is not on screen (`Game::hidden_buttons`); the
    // runner once pressed it anyway (the Lucky 38 mainframe's bug button,
    // walkthrough moon-over-the-tower).
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    g.message_box.as_mut().unwrap().hidden = vec![1];
    assert!(g.press_button(1).unwrap_err().contains("not shown"));
    assert_eq!(g.message_box.as_ref().unwrap().pressed, None, "the box still waits");
    g.press_button(0).unwrap();
    assert_eq!(g.message_box.as_ref().unwrap().pressed, Some(0));
    // The runner's press_button checks the same (`MessageBox::can_press`).
    let m = g.message_box.as_ref().unwrap();
    assert!(m.can_press(1).is_err() && m.can_press(0).is_ok() && m.can_press(-1).is_ok());
}

#[test]
fn actors_choose_the_first_package_whose_conditions_pass() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let npc = FormId(NPC_REF);
    assert_eq!(g.package_list(npc), vec![FormId(PACK_GATED), FormId(PACK_IDLE)]);
    assert_eq!(g.current_package(npc), Some(FormId(PACK_IDLE)));
    // The current package changes only when the actor evaluates its packages.
    g.set_stage(q(), 20);
    assert_eq!(g.current_package(npc), Some(FormId(PACK_IDLE)));
    assert_eq!(g.evaluate_package(npc), Some(FormId(PACK_GATED)));
    // A script package overrides the list.
    g.extra.entry(npc).or_default().script_package = Some(FormId(PACK_SCRIPTED));
    assert_eq!(g.evaluate_package(npc), Some(FormId(PACK_SCRIPTED)));
}

/// Call a function by opcode on `on`, as a script line would.
fn call_on(g: &mut Game, op: u16, on: u32, args: &[ArgValue]) {
    let ctx = Ctx {
        unit: "test".into(),
        opcode: op,
        on: Some(FormId(on)),
        self_ref: None,
        action_ref: None,
        quest: None,
        inst: None,
        in_condition: false,
    };
    g.call_function(&ctx, args);
}

/// AddScriptPackage A then B, then one RemoveScriptPackage: the actor is back on its own
/// list, not on A; EvaluatePackage with a script package on keeps it [observed:
/// `od_08_script_packages`, docs/spec-notes/owner-decision-captures.md].
#[test]
fn a_second_script_package_replaces_the_first() {
    const ADD: u16 = 0x1097;
    const REMOVE: u16 = 0x1098;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let npc = FormId(NPC_REF);
    let pkg = |p: u32| [ArgValue::Value(Value::Ref(p))];
    assert_eq!(g.current_package(npc), Some(FormId(PACK_IDLE)));
    call_on(&mut g, ADD, NPC_REF, &pkg(PACK_SCRIPTED));
    call_on(&mut g, ADD, NPC_REF, &pkg(PACK_EVENTS));
    assert_eq!(g.current_package(npc), Some(FormId(PACK_EVENTS)));
    assert_eq!(g.extra[&npc].script_package, Some(FormId(PACK_EVENTS)));
    // EvaluatePackage keeps the script package.
    assert_eq!(g.evaluate_package(npc), Some(FormId(PACK_EVENTS)));
    call_on(&mut g, REMOVE, NPC_REF, &[]);
    assert_eq!(g.extra[&npc].script_package, None);
    assert_eq!(g.evaluate_package(npc), Some(FormId(PACK_IDLE)));
    // A second RemoveScriptPackage changes nothing.
    call_on(&mut g, REMOVE, NPC_REF, &[]);
    assert_eq!(g.evaluate_package(npc), Some(FormId(PACK_IDLE)));
}

/// Saves written while the slot was a list still load: the list's last package is the
/// script package.
#[test]
fn a_saved_script_package_list_loads_as_its_last_package() {
    use fnv_game::game::RefExtra;
    let old: RefExtra = serde_json::from_value(json!({"packages": [FormId(PACK_SCRIPTED), FormId(PACK_EVENTS)]})).unwrap();
    assert_eq!(old.script_package, Some(FormId(PACK_EVENTS)));
    let empty: RefExtra = serde_json::from_value(json!({"packages": []})).unwrap();
    assert_eq!(empty.script_package, None);
    let new = RefExtra { script_package: Some(FormId(PACK_IDLE)), ..RefExtra::default() };
    let back: RefExtra = serde_json::from_value(serde_json::to_value(&new).unwrap()).unwrap();
    assert_eq!(back, new);
    assert_eq!(serde_json::from_value::<RefExtra>(json!({})).unwrap().script_package, None);
}

/// A disabled reference's GameMode block runs every frame, as while it is enabled
/// [observed: `od_09a_disabled_ref_script`, the skull's timer fell at the same rate while
/// disabled, docs/spec-notes/owner-decision-captures.md].
#[test]
fn a_disabled_reference_runs_its_game_mode_block() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.set_loaded([FormId(CELL)].into_iter().collect());
    let key = InstKey::Ref { reference: FormId(ACT_REF) };
    let frames = |g: &Game| g.var(key, 3);
    let start = match frames(&g) {
        Value::Num(x) => x,
        v => panic!("{v:?}"),
    };
    g.tick();
    assert_eq!(frames(&g), Value::Num(start + 1.0));
    g.world.set_enabled(FormId(ACT_REF), false).unwrap();
    g.tick();
    g.tick();
    assert_eq!(frames(&g), Value::Num(start + 3.0));
    g.world.set_enabled(FormId(ACT_REF), true).unwrap();
    g.tick();
    assert_eq!(frames(&g), Value::Num(start + 4.0));
}

/// A reference disabled when its cell loads runs its OnLoad block at that load, and
/// again at the start of the frame after its Enable [observed: VMS15's rising plane read
/// `bLoaded` 1 on the line after its Enable, layer5-captures.md §8.3 d; the jukebox's
/// OnLoad one frame after Enable, same section].
#[test]
fn a_disabled_reference_runs_on_load_when_its_cell_loads() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.world.set_enabled(FormId(ACT_REF), false).unwrap();
    let key = InstKey::Ref { reference: FormId(ACT_REF) };
    let loads = |g: &Game| match g.var(key, 7) {
        Value::Num(x) => x,
        v => panic!("{v:?}"),
    };
    g.set_loaded([FormId(CELL)].into_iter().collect());
    assert_eq!(loads(&g), 1.0);
    g.tick();
    assert_eq!(loads(&g), 1.0);
    g.world.set_enabled(FormId(ACT_REF), true).unwrap();
    g.note_enabled(FormId(ACT_REF));
    assert_eq!(loads(&g), 1.0, "not inside Enable");
    g.tick();
    assert_eq!(loads(&g), 2.0);
    g.tick();
    assert_eq!(loads(&g), 2.0);
}

/// How many times `r`'s OnLoad block has run (the fixture scripts' nOnLoad).
fn on_loads(g: &Game, r: FormId) -> f64 {
    match g.var(InstKey::Ref { reference: r }, 7) {
        Value::Num(x) => x,
        v => panic!("{v:?}"),
    }
}

fn load_only(g: &mut Game, cell: u32) {
    g.set_loaded([FormId(cell)].into_iter().collect());
}

/// [design: game.rs scripted_refs_in] A scripted door move registers an NPC for
/// destination GameMode and OnLoad selection, including after a save is restored.
#[test]
fn a_scripted_npc_sent_through_a_door_runs_in_its_destination() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let npc = FormId(NPC_REF);
    let frames = |g: &Game| g.var(InstKey::Ref { reference: npc }, 3);
    load_only(&mut g, CELL);
    assert_eq!(on_loads(&g, npc), 1.0);
    g.tick();
    assert_eq!(frames(&g), Value::Num(1.0));
    g.events.clear();
    g.run_source("TestDoorREF.Activate TestNpcREF", Default::default()).unwrap();
    let s = g.world.peek(npc).unwrap();
    assert_eq!((s.cell, s.pos, s.rot), (Some(FormId(OTHER_CELL)), DOOR_POS, [0.0, 0.0, 0.75]));
    assert!(g.moved.contains(&npc));
    assert_eq!(g.take_teleported(), [npc].into_iter().collect());
    assert!(g.brought_along.is_empty());
    assert!(!g.turned.contains(&npc));
    assert_eq!(g.events.iter().filter(|e| matches!(e.kind, EventKind::Moved { reference, cell: Some(cell), pos }
        if reference == npc && cell == FormId(OTHER_CELL) && pos == DOOR_POS)).count(), 1);
    g.tick();
    assert_eq!(frames(&g), Value::Num(1.0), "the source no longer runs the NPC");
    load_only(&mut g, OTHER_CELL);
    assert_eq!(on_loads(&g, npc), 2.0);
    g.tick();
    assert_eq!(frames(&g), Value::Num(2.0));
    load_only(&mut g, OTHER_CELL);
    assert_eq!(on_loads(&g, npc), 2.0, "no second load while the cell stays loaded");
    load_only(&mut g, CELL);
    assert_eq!(on_loads(&g, npc), 2.0, "the authored cell loads without it");
    load_only(&mut g, OTHER_CELL);
    assert_eq!(on_loads(&g, npc), 3.0, "once on destination reload");

    let saved: fnv_game::game::GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.snapshot()).unwrap()).unwrap();
    let world = World::restore(&f.forms, &f.refs, &saved.sim).unwrap();
    let mut restored = Game::restore(&f.data, world, g.cfg, &saved).unwrap();
    assert_eq!(restored.cell_of(npc), Some(FormId(OTHER_CELL)));
    assert!(restored.moved.contains(&npc));
    restored.tick();
    assert_eq!(frames(&restored), Value::Num(3.0));
    load_only(&mut restored, CELL);
    restored.tick();
    assert_eq!(frames(&restored), Value::Num(3.0));
    assert_eq!(on_loads(&restored, npc), 3.0);
    load_only(&mut restored, OTHER_CELL);
    assert_eq!(on_loads(&restored, npc), 4.0);
    restored.tick();
    assert_eq!(frames(&restored), Value::Num(4.0));
}

#[test]
fn an_unscripted_npc_door_move_is_reported_only_after_success() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    load_only(&mut g, CELL);
    let npc = FormId(NPC_AUTO_REF);
    g.events.clear();
    g.world.lock(FormId(DOOR_REF), Some(50)).unwrap();
    g.run_source("TestDoorREF.Activate TestAutoNpcREF", Default::default()).unwrap();
    assert_eq!(g.cell_of(npc), Some(FormId(CELL)));
    assert!(g.take_teleported().is_empty());
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::Moved { .. })));
    g.world.unlock(FormId(DOOR_REF)).unwrap();
    g.run_source("TestDoorREF.Activate TestAutoNpcREF", Default::default()).unwrap();
    assert_eq!(g.cell_of(npc), Some(FormId(OTHER_CELL)));
    assert_eq!(g.take_teleported(), [npc].into_iter().collect());
    assert!(!g.moved.contains(&npc));
    assert!(!g.turned.contains(&npc));
    assert_eq!(g.loaded_cells, [FormId(CELL)].into_iter().collect());
    assert_eq!(g.events.iter().filter(|e| matches!(e.kind, EventKind::Moved { reference, .. } if reference == npc)).count(), 1);
}

#[test]
fn a_player_door_move_keeps_cell_entry_and_followers() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    load_only(&mut g, CELL);
    let npc = FormId(NPC_REF);
    g.extra.entry(npc).or_default().teammate = true;
    g.events.clear();
    g.run_source("TestDoorREF.Activate player", Default::default()).unwrap();
    assert_eq!(g.cell_of(FormId::PLAYER_REF), Some(FormId(OTHER_CELL)));
    assert_eq!(g.loaded_cells, [FormId(OTHER_CELL)].into_iter().collect());
    assert_eq!(g.cell_of(npc), Some(FormId(OTHER_CELL)));
    assert_eq!(g.brought_along, [npc].into_iter().collect());
    assert_eq!(g.take_teleported(), [npc].into_iter().collect());
    assert_eq!(g.events.iter().filter(|e| matches!(e.kind, EventKind::Moved { reference, .. } if reference == npc)).count(), 1);
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::Moved { reference: FormId::PLAYER_REF, .. })));
}

/// OnLoad at a cell load goes by the cell a reference is in now: a moved one runs it when
/// its new cell loads, not when its old one does (issue #1577; the same reading as
/// `Game::active_refs`, which picks the references whose GameMode blocks run).
#[test]
fn a_moved_reference_runs_on_load_with_its_new_cell() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let act = FormId(ACT_REF);
    load_only(&mut g, CELL);
    assert_eq!(on_loads(&g, act), 1.0);
    load_only(&mut g, OTHER_CELL);
    assert_eq!(on_loads(&g, act), 1.0, "not in the other cell yet");
    g.run_source("TestActivatorREF.MoveTo TestOtherREF", Default::default()).unwrap();
    assert_eq!(g.cell_of(act), Some(FormId(OTHER_CELL)));
    assert_eq!(on_loads(&g, act), 1.0, "moving into a loaded cell is no cell load");
    load_only(&mut g, CELL);
    assert_eq!(on_loads(&g, act), 1.0, "its old cell loads without it");
    load_only(&mut g, OTHER_CELL);
    assert_eq!(on_loads(&g, act), 2.0, "its new cell loads with it, once");
    // Moved back, it goes by its first cell again.
    g.run_source("TestActivatorREF.MoveTo TestPatrolPointREF", Default::default()).unwrap();
    load_only(&mut g, CELL);
    assert_eq!(on_loads(&g, act), 3.0);
}

/// Disabled references run OnLoad in the cell they moved to, as in their own
/// (`a_disabled_reference_runs_on_load_when_its_cell_loads`); deleted ones run none.
#[test]
fn a_moved_reference_keeps_the_disabled_and_deleted_rules() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let act = FormId(ACT_REF);
    g.run_source("TestActivatorREF.MoveTo TestOtherREF", Default::default()).unwrap();
    g.world.set_enabled(act, false).unwrap();
    load_only(&mut g, OTHER_CELL);
    assert_eq!(on_loads(&g, act), 1.0);
    load_only(&mut g, CELL);
    g.world.set_deleted(act, true).unwrap();
    load_only(&mut g, OTHER_CELL);
    assert_eq!(on_loads(&g, act), 1.0);
}

/// A reference made at run time is in no cell's placed list: it runs OnLoad when its cell
/// loads later (issue #1577).
#[test]
fn a_created_reference_runs_on_load_when_its_cell_loads_later() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    load_only(&mut g, CELL);
    let made = g.create_at(FormId(ACTIVATOR), Some(FormId(OTHER_CELL)), [0.0; 3], [0.0; 3]).unwrap();
    g.tick();
    assert_eq!(on_loads(&g, made), 0.0, "its cell is not loaded");
    load_only(&mut g, OTHER_CELL);
    assert_eq!(on_loads(&g, made), 1.0);
    load_only(&mut g, CELL);
    assert_eq!(on_loads(&g, made), 1.0);
    load_only(&mut g, OTHER_CELL);
    assert_eq!(on_loads(&g, made), 2.0);
}

/// A scripted player runs no OnLoad at a cell load, MoveTo or not.
#[test]
fn a_moved_player_runs_no_on_load_at_a_cell_load() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    load_only(&mut g, CELL);
    g.other.scripts.insert(FormId::PLAYER_BASE, Some(FormId(ACT_SCRIPT)));
    g.run_source("player.MoveTo TestOtherREF", Default::default()).unwrap();
    assert!(g.loaded_cells.contains(&FormId(OTHER_CELL)));
    assert_eq!(on_loads(&g, FormId::PLAYER_REF), 0.0);
}

/// A player moved to a marker turns to the marker's heading and takes its pitch (the X
/// angle); moved to itself it keeps both (issue #2226: the ending's XMarkerHeading faces
/// the slide; #2710: the slide markers are level, and the view held there was tilted).
#[test]
fn a_moved_player_takes_the_markers_heading_and_pitch() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.world.get(FormId(OTHER_REF)).unwrap().rot = [0.4, 0.0, 1.5];
    g.world.get(FormId::PLAYER_REF).unwrap().rot = [0.3, 0.0, 2.5];
    g.run_source("player.MoveTo TestOtherREF", Default::default()).unwrap();
    assert_eq!(g.world.get(FormId::PLAYER_REF).unwrap().rot, [0.4, 0.0, 1.5]);
    g.run_source("player.MoveTo player 0 0 10", Default::default()).unwrap();
    assert_eq!(g.world.get(FormId::PLAYER_REF).unwrap().rot, [0.4, 0.0, 1.5]);
}

/// An OnLoad event handler that counts in TestGlobal: 1 for the activator, 10 for the
/// trigger, 100 for the unscripted reference of the other cell.
const COUNT_ON_LOADS: &str = "SetEventHandler \"OnLoad\" (begin function {ref rOn, ref rSame}\nif rOn == TestActivatorREF\nlet TestGlobal := TestGlobal + 1\nelseif rOn == TestTriggerREF\nlet TestGlobal := TestGlobal + 10\nelseif rOn == TestOtherREF\nlet TestGlobal := TestGlobal + 100\nendif\nend)";

/// The extender's handlers hear each reference of a loaded cell once, by the cell it is in
/// now, scripted or not.
#[test]
fn on_load_handlers_hear_a_moved_reference_once_with_its_new_cell() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    load_only(&mut g, OTHER_CELL);
    g.run_source("TestActivatorREF.MoveTo TestOtherREF", Default::default()).unwrap();
    g.run_source(COUNT_ON_LOADS, Default::default()).unwrap();
    g.world.set_global(FormId(GLOBAL), 0.0).unwrap();
    load_only(&mut g, CELL);
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 10.0, "the trigger stayed; the activator left");
    g.world.set_global(FormId(GLOBAL), 0.0).unwrap();
    load_only(&mut g, OTHER_CELL);
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 101.0, "the activator and the unscripted reference, once each");
    assert_eq!(on_loads(&g, FormId(ACT_REF)), 1.0);
}

/// A block moving a reference during the load gives it one OnLoad, not one per cell: the
/// references are chosen before any block runs.
#[test]
fn a_reference_moved_during_the_load_runs_on_load_once() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.run_source(COUNT_ON_LOADS, Default::default()).unwrap();
    g.run_source("SetEventHandler \"OnLoad\" (begin function {ref rOn, ref rSame}\nif rOn == TestActivatorREF\nTestTriggerREF.MoveTo TestOtherREF\nendif\nend)", Default::default()).unwrap();
    g.world.set_global(FormId(GLOBAL), 0.0).unwrap();
    g.set_loaded([FormId(CELL), FormId(OTHER_CELL)].into_iter().collect());
    assert_eq!(g.cell_of(FormId(TRIG_REF)), Some(FormId(OTHER_CELL)));
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 111.0, "the activator, the trigger and the other cell's reference, once each");
}

#[test]
fn an_ai_that_runs_packages_runs_their_begin_end_and_change_scripts() {
    let f = fixture();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, package_interval: Some(10.0), ..GameConfig::default() });
    let npc = FormId(NPC_REF);
    let global = |g: &Game| g.world.global(FormId(GLOBAL)).unwrap();
    assert_eq!(g.evaluate_package(npc), Some(FormId(PACK_IDLE)));
    assert_eq!(global(&g), 5.0);
    // A script package starts: its begin script runs.
    let add = |g: &mut Game| g.extra.entry(npc).or_default().script_package = Some(FormId(PACK_EVENTS));
    let remove = |g: &mut Game| g.extra.entry(npc).or_default().script_package = None;
    add(&mut g);
    assert_eq!(g.evaluate_package(npc), Some(FormId(PACK_EVENTS)));
    assert_eq!(global(&g), 21.0);
    // It reaches its goal: the end script, once; being replaced afterwards is a change
    // too (its change script runs as the actor leaves it, issue #2221).
    g.package_goal_met(npc);
    assert_eq!(global(&g), 22.0);
    g.package_goal_met(npc);
    assert_eq!(global(&g), 22.0);
    remove(&mut g);
    g.world.set_global(FormId(GLOBAL), 0.0).unwrap();
    assert_eq!(g.evaluate_package(npc), Some(FormId(PACK_IDLE)));
    assert_eq!(global(&g), 23.0, "the change script ran as the finished package was left");
    g.world.set_global(FormId(GLOBAL), 0.0).unwrap();
    // Replaced before its goal: the change script.
    add(&mut g);
    g.evaluate_package(npc);
    assert_eq!(global(&g), 21.0);
    remove(&mut g);
    assert_eq!(g.evaluate_package(npc), Some(FormId(PACK_IDLE)));
    assert_eq!(global(&g), 23.0);
    assert_eq!(g.current_package(npc), Some(FormId(PACK_IDLE)));
    // Without the interval a choice runs no package script (Layer 5 behaviour).
    let mut quiet = game(&f);
    quiet.extra.entry(npc).or_default().script_package = Some(FormId(PACK_EVENTS));
    assert_eq!(quiet.evaluate_package(npc), Some(FormId(PACK_EVENTS)));
    assert_eq!(global(&quiet), 5.0);
}

#[test]
fn the_runner_reaches_a_patrol_point_and_runs_its_script() {
    let f = fixture();
    assert!(f.data.patrol_scripts.contains_key(&FormId(PATROL_REF)));
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "actions": [
            {"op": "patrol_point", "actor": "TestNpcREF", "point": "TestPatrolPointREF"},
            {"op": "get_global", "name": "TestGlobal", "label": "after"},
            {"op": "patrol_point", "actor": "TestNpcREF", "point": "TestActivatorREF"}
        ]
    }))
    .unwrap();
    let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    let steps = out["steps"].as_array().unwrap();
    assert!(steps[0].get("error").is_none(), "{}", out["steps"]);
    // A reference with no patrol script and no topic is refused.
    assert!(steps[2].get("error").is_some(), "{}", out["steps"]);
    let obs = out["observations"].as_array().unwrap();
    assert_eq!(obs.iter().find(|o| o["label"] == "after").unwrap()["value"], 8.0);
}

/// `go_to_cell` (the console's coc) is a cell change: the teammate comes along
/// ("Changing cells will teleport the teammate to the player" [documented: GECK wiki
/// SetPlayerTeammate notes]).
#[test]
fn the_runners_go_to_cell_brings_the_teammate() {
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "actions": [
            {"op": "call", "line": "TestNpcREF.SetPlayerTeammate 1"},
            {"op": "get_ref", "of": "TestNpcREF", "label": "before"},
            {"op": "go_to_cell", "cell": "TestCell"},
            {"op": "get_ref", "of": "TestNpcREF", "label": "after"},
            {"op": "get_ref", "of": "player", "label": "player"}
        ]
    }))
    .unwrap();
    let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    let steps = out["steps"].as_array().unwrap();
    assert!(steps.iter().all(|s| s.get("error").is_none()), "{}", out["steps"]);
    let obs = out["observations"].as_array().unwrap();
    let pos = |l: &str| -> Vec<f64> { obs.iter().find(|o| o["label"] == l).unwrap()["pos"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect() };
    let dist = |a: &[f64], b: &[f64]| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
    assert!(dist(&pos("after"), &pos("player")) < 200.0, "{obs:?}");
    assert_ne!(pos("before"), pos("after"), "{obs:?}");
}

#[test]
fn the_runner_fires_a_carried_weapon_and_runs_its_on_fire_block() {
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "actions": [
            {"op": "fire", "weapon": "TestWeapon", "actor": "TestNpcREF"},
            {"op": "add_item", "to": "TestNpcREF", "item": "TestWeapon", "count": 1},
            {"op": "fire", "weapon": "TestWeapon", "actor": "TestNpcREF"},
            {"op": "get_global", "name": "TestGlobal", "label": "after"}
        ]
    }))
    .unwrap();
    let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    let steps = out["steps"].as_array().unwrap();
    // Not carried: refused; carried: OnFire runs.
    assert!(steps[0].get("error").is_some(), "{}", out["steps"]);
    assert!(steps[2].get("error").is_none(), "{} {}", out["steps"], out["events"]);
    let obs = out["observations"].as_array().unwrap();
    assert_eq!(obs.iter().find(|o| o["label"] == "after").unwrap()["value"], 9.0);
}

/// #1848: a labelled step that fails answers with its error, whatever the action; the
/// menu steps open and close the container menu as the game does; `#0` is no index.
#[test]
fn the_runners_failed_steps_answer_and_its_menus_open_and_close() {
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "actions": [
            {"op": "consume", "item": "NoSuchItem", "label": "consume"},
            {"op": "get_quest", "quest": "NoSuchQuest", "fields": ["stage"], "label": "quest"},
            {"op": "sample_ref_var", "ref": "TestActivatorREF", "var": "nNoSuchVar", "frames": 2, "label": "sample"},
            {"op": "get_ref", "of": "scripted:ActScript#0", "label": "zero"},
            {"op": "get_ref", "of": "scripted:ActScript#1", "label": "one"},
            {"op": "call", "line": "TestNpcREF.KillActor"},
            {"op": "hold_menu", "via": "TestNpcREF", "frames": 3}
        ]
    }))
    .unwrap();
    let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    let steps = out["steps"].as_array().unwrap();
    let obs = |l: &str| out["observations"].as_array().unwrap().iter().find(|o| o["label"] == l).cloned();
    for (i, l) in ["consume", "quest", "sample", "zero"].into_iter().enumerate() {
        assert!(steps[i].get("error").is_some(), "{l}: {}", out["steps"]);
        assert!(obs(l).is_some_and(|o| o["errors"]["action"].is_string()), "{l}: {}", out["observations"]);
    }
    assert!(steps[4].get("error").is_none(), "{}", out["steps"]);
    assert!(steps[6].get("error").is_none(), "{}", out["steps"]);
    let events = out["events"].to_string();
    assert!(events.contains(r#""kind":"menu_opened","menu":1008"#) && events.contains(r#""kind":"menu_closed","menu":1008"#), "{events}");
}

/// `save_load`: the game is written into a save's bytes and restored from them; what a
/// save keeps (globals, reference variables, an item given) is there after it, the
/// frame does not move, and the trace goes on in one piece.
#[test]
fn the_runner_saves_and_loads_in_place() {
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "actions": [
            {"op": "set_global", "name": "TestGlobal", "value": 4},
            {"op": "set_ref_var", "ref": "TestActivatorREF", "var": "nUsed", "value": 3},
            {"op": "add_item", "to": "TestNpcREF", "item": "TestWeapon", "count": 1},
            {"op": "save_load", "label": "saved"},
            {"op": "get_global", "name": "TestGlobal", "label": "global"},
            {"op": "get_ref", "ref": "TestActivatorREF", "fields": ["var.nUsed"], "label": "ref"},
            {"op": "fire", "weapon": "TestWeapon", "actor": "TestNpcREF"},
            {"op": "get_global", "name": "TestGlobal", "label": "fired"}
        ]
    }))
    .unwrap();
    let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    let steps = out["steps"].as_array().unwrap();
    assert!(steps.iter().all(|s| s.get("error").is_none()), "{}", out["steps"]);
    let obs = |l: &str| out["observations"].as_array().unwrap().iter().find(|o| o["label"] == l).unwrap().clone();
    assert!(obs("saved")["bytes"].as_u64().unwrap() > 0);
    assert_eq!(obs("global")["value"], 4.0);
    assert_eq!(obs("ref")["var.nUsed"], 3.0);
    // The weapon given before the save is still carried: its OnFire runs.
    assert_eq!(obs("fired")["value"], 9.0);
    // No frame passes for the save: the step after it starts in the same frame.
    assert_eq!(steps[3]["frame"], steps[4]["frame"]);
    // The trace kept the events from before the save.
    assert!(out["events"].as_array().unwrap().iter().any(|e| e["kind"] == "global_set"), "{}", out["events"]);
}

#[test]
fn a_given_reference_starts_locked_or_unlocked() {
    let f = fixture();
    let run = |locked: bool| {
        let input: fnv_game::runner::Input = serde_json::from_value(json!({
            "format": "fnv.script.actions/1",
            "given": [{"kind": "ref", "ref": "TestActivatorREF", "locked": locked}],
            "actions": [{"op": "get_ref", "ref": "TestActivatorREF", "fields": ["locked"], "label": "given"}]
        }))
        .unwrap();
        let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
        assert_eq!(out["given_errors"], json!([]));
        out["observations"].as_array().unwrap().iter().find(|o| o["label"] == "given").unwrap()["locked"].clone()
    };
    assert_eq!(run(true), json!(true));
    assert_eq!(run(false), json!(false));
}

#[test]
fn the_runner_reads_and_writes_reference_variables_and_activates_with_the_flag() {
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "given": [{"kind": "ref", "ref": "TestActivatorREF", "vars": {"nUsed": 5}, "disabled": false}],
        "actions": [
            {"op": "get_ref", "ref": "TestActivatorREF", "fields": ["var.nUsed", "var.nope", "disabled", "dead"], "label": "given"},
            {"op": "set_ref_var", "ref": "TestActivatorREF", "var": "nUsed", "value": 1},
            {"op": "activate_read", "target": "TestActivatorREF", "read": {"ref": "TestActivatorREF", "field": "var.nUsed"}, "label": "plain"},
            {"op": "activate_read", "target": "TestActivatorREF", "run_on_activate": true,
             "read": {"ref": "TestActivatorREF", "field": "var.nUsed"}, "label": "flagged"},
            {"op": "sample_ref_var", "ref": "TestActivatorREF", "var": "nUsed", "frames": 3, "values": true, "label": "sampled"},
            {"op": "evaluate_package", "of": "TestNpcREF"},
            {"op": "is_current_package", "of": "TestNpcREF", "package": "IdlePackage", "label": "package"}
        ]
    }))
    .unwrap();
    let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    assert_eq!(out["given_errors"], json!([]));
    assert_eq!(out["steps"].as_array().unwrap().iter().filter(|s| s.get("error").is_some()).count(), 0, "{}", out["steps"]);
    let obs = out["observations"].as_array().unwrap();
    let by = |l: &str| obs.iter().find(|o| o["label"] == l).unwrap().clone();
    assert_eq!(by("given")["var.nUsed"], 5.0);
    assert_eq!(by("given")["var.nope"], 0.0);
    assert_eq!(by("given")["disabled"], false);
    assert_eq!(by("given")["dead"], false);
    // A scripted Activate without the flag does not run OnActivate; with it, the block
    // has run when Activate returns.
    assert_eq!(by("plain")["var.nUsed"], 1.0);
    assert_eq!(by("flagged")["var.nUsed"], 2.0);
    assert_eq!(by("sampled")["values"], json!([2.0]));
    assert_eq!(by("package")["current"], true);
}

#[test]
fn the_runner_starts_from_a_saves_place_skills_and_items() {
    // What `fnvtool ext queries --save` gives (issue #1023). The fixture's player has no
    // base record, so its skills read 0: the tags are read instead.
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "given": [
            {"kind": "tag_skills", "skills": ["32", "Speech"]},
            {"kind": "skill_points", "skill": "Barter", "value": 2.0},
            {"kind": "item_change", "of": "TestNpcREF", "item": "TestWeapon", "change": 3},
            {"kind": "item_change", "of": "TestNpcREF", "item": "TestWeapon", "change": -1},
            {"kind": "place", "ref": "player", "cell": "TestCell", "pos": [100.0, 200.0, 300.0], "rot": [0.0, 0.0, 1.309]}
        ],
        "actions": [
            {"op": "get_tag_skills", "skills": ["Barter", "Speech", "Guns"], "label": "tags"},
            {"op": "get_item_count", "of": "TestNpcREF", "item": "TestWeapon", "label": "items"},
            {"op": "query", "script": "Begin Function { }
SetFunctionValue (player.GetPos X)
End", "label": "x"},
            {"op": "query", "script": "Begin Function { }
SetFunctionValue (player.GetAngle Z)
End", "label": "z"}
        ]
    }))
    .unwrap();
    let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    assert_eq!(out["given_errors"], json!([]));
    let obs = out["observations"].as_array().unwrap();
    let by = |l: &str| obs.iter().find(|o| o["label"] == l).unwrap().clone();
    assert_eq!((by("tags")["tag.Barter"].clone(), by("tags")["tag.Speech"].clone(), by("tags")["tag.Guns"].clone()), (json!(true), json!(true), json!(false)));
    assert_eq!(by("items")["count"], 2);
    assert_eq!(by("x")["value"], 100.0, "{}", by("x"));
    assert!((by("z")["value"].as_f64().unwrap() - 75.0).abs() < 0.01, "{}", by("z"));
}

/// A save's place must be a cell, or a worldspace with a cell at the position: the player
/// is never put in another form (#1325 review).
#[test]
fn a_place_that_is_no_cell_is_refused() {
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "given": [{"kind": "place", "ref": "player", "cell": "TestWeapon", "pos": [1.0, 2.0, 3.0]}],
        "actions": []
    }))
    .unwrap();
    let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    let errors = out["given_errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1);
    assert!(errors[0]["error"].as_str().unwrap().contains("neither a cell nor a worldspace"), "{errors:?}");
}

#[test]
fn say_to_says_the_next_unsaid_line_and_runs_its_end_script_when_it_ends() {
    let f = fixture();
    assert_eq!(f.data.topics[&FormId(TOPIC)], vec![FormId(INFO_A), FormId(INFO_B)]);
    assert_eq!(f.data.infos[&FormId(INFO_A)].topic, Some(FormId(TOPIC)));
    assert!(f.data.infos[&FormId(INFO_A)].say_once);
    let mut g = game(&f);
    g.new_game();
    let speaker = FormId(NPC_REF);
    assert_eq!(g.say(speaker, Some(FormId::PLAYER_REF), FormId(TOPIC), true), Some(FormId(INFO_A)));
    assert!(g.is_talking(speaker));
    // "Hello there": 1.1 s + 11 / 16 s = 1.7875 s of game time; a menu holds it.
    g.menu = Some(1001);
    g.frames(300);
    assert!(g.is_talking(speaker));
    g.menu = None;
    g.frames(106);
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 5.0);
    g.frames(2);
    assert!(!g.is_talking(speaker));
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 3.0);
    // Say Once: the next SayTo takes the second line, then there is none left.
    assert_eq!(g.say(speaker, Some(FormId::PLAYER_REF), FormId(TOPIC), true), Some(FormId(INFO_B)));
    // Nothing left to say: the line being said goes on.
    assert_eq!(g.say(speaker, None, FormId(TOPIC), false), None);
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::NoLine { .. })));
    g.frames(120);
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 4.0);
    // A stopped quest's lines are not said.
    let mut h = game(&f);
    assert_eq!(h.say(speaker, None, FormId(TOPIC), false), None);
}

#[test]
fn say_to_done_follows_say_as_well_as_say_to() {
    // docs/spec-notes/level-design.md §11.2 D1: SayToDone runs on the speaker after a
    // line said with Say as well as with SayTo.
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let speaker = FormId(NPC_REF);
    let done = |g: &Game| g.var(InstKey::Ref { reference: speaker }, 1).num();
    assert_eq!(g.say(speaker, None, FormId(TOPIC), false), Some(FormId(INFO_A)));
    g.frames(120);
    assert!(!g.is_talking(speaker));
    assert_eq!(done(&g), 1.0);
    assert_eq!(g.say(speaker, Some(FormId::PLAYER_REF), FormId(TOPIC), true), Some(FormId(INFO_B)));
    g.frames(120);
    assert_eq!(done(&g), 2.0);
    // A line cut by another does not end: no SayToDone for it.
    let mut h = game(&f);
    h.new_game();
    h.say(speaker, None, FormId(TOPIC), false);
    h.say(speaker, None, FormId(TOPIC), false);
    h.frames(120);
    assert_eq!(done(&h), 1.0);
}

// [documented: #2714, spooknik's 2026-11-15 comment] Roy's plain Say uses a
// Conversation/Goodbye line without a target or continuation. [design, #2714]
// Exercise that shape with this file's authored fixture; check engine pause/audio in
// the roy-say-pause-resume smoke scenario, not by this headless menu-mode test.
#[test]
fn plain_say_resumes_after_mid_line_pauses_and_finishes_once() {
    let mut f = fixture();
    let line = f.data.infos.get_mut(&FormId(INFO_A)).unwrap();
    line.info_type = fnv_game::data::topic_type::CONVERSATION;
    line.flags |= fnv_game::data::info_flag::GOODBYE;
    line.goodbye = true;
    assert!(line.choices.is_empty() && line.follow_ups.is_empty());
    let end = line.end.unwrap();
    let opcode = f.data.table.by_name("Say").unwrap();
    let mut g = game(&f);
    g.new_game();
    let speaker = FormId(NPC_REF);
    let ctx = Ctx { unit: "test".into(), opcode, on: Some(speaker), self_ref: Some(speaker), action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[ArgValue::Value(Value::Ref(TOPIC))]);
    let done = |g: &Game| g.var(InstKey::Ref { reference: speaker }, 1).num();
    let ends = |g: &Game| g.events.iter().filter(|e| matches!(e.kind, EventKind::LineEnded { info, .. } if info == FormId(INFO_A))).count();
    let total = g.speech[0].remaining;
    g.frames(40);
    assert!(g.speech[0].remaining > 0.0 && g.speech[0].remaining < total);
    for _ in 0..2 {
        let held = g.speech[0].clone();
        assert_eq!((held.speaker, held.info, held.target, held.conversation), (speaker, FormId(INFO_A), None, None));
        assert_eq!(held.response, 0);
        assert!(!held.say_to && !held.leads_on && !held.greeting && !held.farewell);
        g.open_menu(fnv_game::game::menus::START);
        g.frames(600);
        assert!(g.is_talking(speaker));
        // [design: nvf-play/script_hud.rs::subtitles, voice.rs::speech_response]
        // Retained INFO/response and remaining time let the HUD keep this subtitle.
        assert_eq!(g.speech.len(), 1);
        assert_eq!((g.speech[0].info, g.speech[0].response, g.speech[0].remaining), (held.info, held.response, held.remaining));
        assert_eq!((ends(&g), done(&g)), (0, 0.0));
        assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 5.0);
        assert_eq!(g.close_menu(), Some(fnv_game::game::menus::START));
        g.frames(10);
        assert!(g.is_talking(speaker));
        assert!(g.speech[0].remaining < held.remaining);
    }
    let left = g.speech[0].remaining;
    let frames_left = (left / g.cfg.frame_seconds).ceil() as u64;
    g.frames(frames_left - 1);
    assert!(g.is_talking(speaker));
    assert_eq!((ends(&g), done(&g)), (0, 0.0));
    g.tick();
    assert!(!g.is_talking(speaker) && g.speech.is_empty());
    assert_eq!((ends(&g), done(&g)), (1, 1.0));
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 3.0);
    g.frames(600);
    assert_eq!((ends(&g), done(&g)), (1, 1.0));
    assert_eq!(g.events.iter().filter(|e| matches!(&e.kind, EventKind::ResultScript { unit, on: Some(on) } if *on == speaker && *unit == f.data.unit(end).key())).count(), 1);
    assert_eq!(g.events.iter().filter(|e| matches!(e.kind, EventKind::LineStarted { .. })).count(), 1);
    assert!(!g.events.iter().any(|e| matches!(e.kind, EventKind::LineEnded { cut: true, .. } | EventKind::ActorConversationStarted { .. })));
}

#[test]
fn a_line_lasts_as_long_as_its_voice_file_when_the_game_knows_it() {
    // docs/spec-notes/voice.md §8: the hook gives a response's length; a response it
    // does not know keeps the length fitted to its text.
    struct Sounds;
    impl fnv_game::voice::VoiceLengths for Sounds {
        fn response_seconds(&self, info: FormId, response: usize, speaker_base: Option<FormId>) -> Option<f64> {
            (info == FormId(INFO_A) && response == 0 && speaker_base == Some(FormId(NPC))).then_some(0.5)
        }
    }
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let speaker = FormId(NPC_REF);
    let started = |g: &Game, info: u32| {
        g.events.iter().rev().find_map(|e| match e.kind {
            EventKind::LineStarted { info: i, seconds, .. } if i == FormId(info) => Some(seconds),
            _ => None,
        })
    };
    // Without the hook: "Hello there" is 1.1 s + 11 / 16 s.
    assert_eq!(g.response_lengths(speaker, FormId(INFO_A)), vec![1.7875]);
    g.voice = Some(std::sync::Arc::new(Sounds));
    assert_eq!(g.response_lengths(speaker, FormId(INFO_A)), vec![0.5]);
    assert_eq!(g.response_lengths_as(Some(FormId(NPC)), FormId(INFO_A)), vec![0.5]);
    // Another speaker's base: the hook has no sound for it.
    assert_eq!(g.response_lengths_as(None, FormId(INFO_A)), vec![1.7875]);
    assert_eq!(g.say(speaker, None, FormId(TOPIC), false), Some(FormId(INFO_A)));
    assert_eq!(started(&g, INFO_A), Some(0.5));
    g.frames(29);
    assert!(g.is_talking(speaker));
    g.frames(2);
    assert!(!g.is_talking(speaker));
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 3.0);
    // "Bye" has no sound: 1.1 s + 3 / 16 s.
    assert_eq!(g.say(speaker, None, FormId(TOPIC), false), Some(FormId(INFO_B)));
    assert_eq!(started(&g, INFO_B), Some(1.2875));
}

#[test]
fn a_line_said_for_its_fixed_speaker_is_timed_by_the_fixed_speakers_file() {
    // Old World Blues' Think Tank and the end slides: a line whose fixed speaker (ANAM)
    // is not the actor saying it has its voice file in the fixed speaker's folder only.
    struct Sounds;
    impl fnv_game::voice::VoiceLengths for Sounds {
        fn response_seconds(&self, info: FormId, response: usize, speaker_base: Option<FormId>) -> Option<f64> {
            (info == FormId(INFO_A) && response == 0 && speaker_base == Some(FormId(NPC))).then_some(0.5)
        }
    }
    let mut f = fixture();
    f.data.infos.get_mut(&FormId(INFO_A)).unwrap().speaker = Some(FormId(NPC));
    let mut g = game(&f);
    g.voice = Some(std::sync::Arc::new(Sounds));
    // Said by another base (or none): the fixed speaker's file times it.
    assert_eq!(g.response_lengths_as(Some(FormId(NPC_AUTO)), FormId(INFO_A)), vec![0.5]);
    assert_eq!(g.response_lengths_as(None, FormId(INFO_A)), vec![0.5]);
    // A line without a fixed speaker and no file keeps the fitted length.
    assert_eq!(g.response_lengths_as(Some(FormId(NPC_AUTO)), FormId(INFO_B)), vec![1.2875]);
}

#[test]
fn a_scripts_say_falls_back_to_a_fixed_speakers_line_when_no_line_passes_the_check() {
    // Issue #965: Dead Money's narrator is told to SayTo topics whose only line names
    // Elijah (or Vera) as fixed speaker, with no GetIsID condition. A script's Say or
    // SayTo then says that line in the fixed speaker's stead; other Say callers (combat
    // barks, package topics) keep the check (dialogue.md §4).
    let speaker = FormId(NPC_REF);
    let mut f = fixture();
    for i in [INFO_A, INFO_B] {
        f.data.infos.get_mut(&FormId(i)).unwrap().speaker = Some(FormId(NPC_AUTO));
    }
    let mut g = game(&f);
    g.new_game();
    assert!(!g.can_say(speaker, Some(FormId::PLAYER_REF), FormId(INFO_A)));
    assert_eq!(g.say(speaker, None, FormId(TOPIC), false), None);
    assert_eq!(g.say_by_script(speaker, Some(FormId::PLAYER_REF), FormId(TOPIC), true), Some(FormId(INFO_A)));
    // A topic with a line the speaker passes the check for picks as before: the
    // fixed speaker's line stored first is passed over.
    let mut f = fixture();
    f.data.infos.get_mut(&FormId(INFO_A)).unwrap().speaker = Some(FormId(NPC_AUTO));
    let mut g = game(&f);
    g.new_game();
    assert_eq!(g.say_by_script(speaker, None, FormId(TOPIC), false), Some(FormId(INFO_B)));
}

#[test]
fn a_scripts_say_does_not_fall_back_when_the_speakers_own_line_is_spent() {
    // Review of #1767: the fallback is decided from the data, not from the run-time
    // result. The speaker's own Say Once line, once said, leaves the topic with no line
    // it can say; the other speaker's fixed line beside it is still not said in its
    // mouth, and the SayTo goes unsaid as before (dialogue.md §4).
    let speaker = FormId(NPC_REF);
    let mut f = fixture();
    f.data.infos.get_mut(&FormId(INFO_A)).unwrap().speaker = Some(FormId(NPC_AUTO));
    f.data.infos.get_mut(&FormId(INFO_B)).unwrap().say_once = true;
    let mut g = game(&f);
    g.new_game();
    assert_eq!(g.say_by_script(speaker, Some(FormId::PLAYER_REF), FormId(TOPIC), true), Some(FormId(INFO_B)));
    // Once the line is over (a repeat while it plays leaves it playing, #2582).
    g.frames(120);
    assert!(!g.is_talking(speaker));
    assert_eq!(g.say_by_script(speaker, Some(FormId::PLAYER_REF), FormId(TOPIC), true), None);
}

#[test]
fn a_scripts_say_of_the_topic_being_said_leaves_the_line_playing() {
    // #2582: the Strip gate's kill zone SayTos its topic every frame; the original renews
    // the talking state without a queue (observer batch o30), and here the line being
    // said goes on instead of restarting every frame.
    let f = fixture();
    let speaker = FormId(NPC_REF);
    let mut g = game(&f);
    g.new_game();
    let started = |g: &Game| g.events.iter().filter(|e| matches!(e.kind, EventKind::LineStarted { .. })).count();
    assert_eq!(g.say_by_script(speaker, Some(FormId::PLAYER_REF), FormId(TOPIC), true), Some(FormId(INFO_A)));
    // A SayTo or a Say of the same topic every frame while it plays: the same line.
    for k in 0..60 {
        g.frames(1);
        assert_eq!(g.say_by_script(speaker, Some(FormId::PLAYER_REF), FormId(TOPIC), k % 2 == 0), Some(FormId(INFO_A)));
    }
    assert_eq!(started(&g), 1);
    // Said to its end: its end script ran (GLOBAL 3, as in the SayTo test above), and
    // the next call says the topic's next line.
    g.frames(120);
    assert!(!g.is_talking(speaker));
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 3.0);
    assert_eq!(g.say_by_script(speaker, Some(FormId::PLAYER_REF), FormId(TOPIC), true), Some(FormId(INFO_B)));
    assert_eq!(started(&g), 2);
    // A line started otherwise (here a Say by the engine, `Game::say`) is no different:
    // a script's repeat of its topic leaves it playing too, while `Game::say` still cuts.
    let mut h = game(&f);
    h.new_game();
    assert_eq!(h.say(speaker, None, FormId(TOPIC), false), Some(FormId(INFO_A)));
    assert_eq!(h.say_by_script(speaker, None, FormId(TOPIC), false), Some(FormId(INFO_A)));
    assert_eq!(h.say(speaker, None, FormId(TOPIC), false), Some(FormId(INFO_B)));
}

#[test]
fn an_actor_whose_traits_come_from_a_leveled_list_speaks_with_the_picked_entrys_voice() {
    // Issue #1051: generic actors take race, sex and voice type from a leveled list of
    // male and female entries (template flag "Traits"). The voice is the entry's, the
    // one `GetIsVoiceType` reads, not the placed base record's own.
    const LIST: u32 = 0x9F0;
    struct Sounds;
    impl fnv_game::voice::VoiceLengths for Sounds {
        fn response_seconds(&self, info: FormId, response: usize, speaker_base: Option<FormId>) -> Option<f64> {
            (info == FormId(INFO_A) && response == 0 && speaker_base == Some(FormId(NPC_AUTO))).then_some(0.5)
        }
    }
    let mut f = fixture();
    f.data.leveled_actors.insert(FormId(LIST), fnv_game::data::LeveledActorDef { entries: vec![(1, FormId(NPC_AUTO))], counts: vec![1], ..Default::default() });
    let npc = f.data.actors.get_mut(&FormId(NPC)).unwrap();
    npc.template = Some(FormId(LIST));
    npc.template_traits = true;
    let mut g = game(&f);
    g.new_game();
    g.voice = Some(std::sync::Arc::new(Sounds));
    let speaker = FormId(NPC_REF);
    assert_eq!(g.voice_base(FormId(NPC), speaker), FormId(NPC_AUTO));
    assert_eq!(g.speaker_voice_base(speaker), Some(FormId(NPC_AUTO)));
    assert_eq!(g.response_lengths(speaker, FormId(INFO_A)), vec![0.5]);
    // A fixed speaker (ANAM) that is the placed base itself is no other actor: the
    // record's own voice does not stand in when the picked voice has no file.
    struct BaseOnly;
    impl fnv_game::voice::VoiceLengths for BaseOnly {
        fn response_seconds(&self, info: FormId, response: usize, speaker_base: Option<FormId>) -> Option<f64> {
            (info == FormId(INFO_A) && response == 0 && speaker_base == Some(FormId(NPC))).then_some(0.5)
        }
    }
    f.data.infos.get_mut(&FormId(INFO_A)).unwrap().speaker = Some(FormId(NPC));
    let mut g = game(&f);
    g.new_game();
    g.voice = Some(std::sync::Arc::new(BaseOnly));
    assert_eq!(g.response_lengths(speaker, FormId(INFO_A)), vec![1.7875]);
    assert_eq!(g.response_seconds(speaker, FormId(INFO_A), 0), 1.7875);
    // Said by another actor, the fixed speaker's file still stands in.
    assert_eq!(g.response_lengths_voiced(Some(FormId(NPC_AUTO)), Some(FormId(NPC_AUTO)), FormId(INFO_A)), vec![0.5]);
    // Without the flag the base keeps its own voice.
    let mut f = fixture();
    f.data.leveled_actors.insert(FormId(LIST), fnv_game::data::LeveledActorDef { entries: vec![(1, FormId(NPC_AUTO))], counts: vec![1], ..Default::default() });
    f.data.actors.get_mut(&FormId(NPC)).unwrap().template = Some(FormId(LIST));
    let g = game(&f);
    assert_eq!(g.voice_base(FormId(NPC), speaker), FormId(NPC));
}

#[test]
fn a_templated_actors_combat_style_follows_the_traits_flag() {
    // Issue #2138: the combat style (ZNAM) is on the Traits tab [documented: GECK wiki
    // Traits Tab - NPC, Traits Tab - Creatures], so a templated actor takes it from its
    // template under "Use Traits", as the death item, not under "Use AI Data".
    const OWN: u32 = 0xA20;
    const PARENT: u32 = 0xA21;
    const TOP: u32 = 0xA22;
    const PACKAGE: u32 = 0xA23;
    const SCRIPTED: u32 = 0xA24;
    const MID: u32 = 0xA25;
    const LIST: u32 = 0xA26;
    let me = FormId(NPC_REF);
    // NPC (own style OWN) templated on NPC_AUTO (style PARENT), with the given flags.
    let with = |traits: bool, ai: bool| {
        let mut f = fixture();
        for s in [OWN, PARENT, TOP, PACKAGE, SCRIPTED] {
            f.data.combat_styles.insert(FormId(s), fnv_game::data::CombatStyleDef::default());
        }
        f.data.actors.get_mut(&FormId(NPC_AUTO)).unwrap().combat_style = Some(FormId(PARENT));
        let npc = f.data.actors.get_mut(&FormId(NPC)).unwrap();
        npc.combat_style = Some(FormId(OWN));
        npc.template = Some(FormId(NPC_AUTO));
        npc.template_traits = traits;
        npc.template_ai = ai;
        f
    };
    // Traits only: the template's; AI Data only: its own; neither: its own; both: the
    // template's.
    for (traits, ai, style) in [(true, false, PARENT), (false, true, OWN), (false, false, OWN), (true, true, PARENT)] {
        let f = with(traits, ai);
        let g = game(&f);
        assert_eq!(g.combat_style_id(me), Some(FormId(style)), "traits {traits}, AI data {ai}");
    }
    // A chain: NPC → NPC_AUTO → MID, each with Traits, takes MID's; NPC_AUTO without the
    // flag stops at its own.
    for (mid_traits, style) in [(true, TOP), (false, PARENT)] {
        let mut f = with(true, false);
        let mut mid = f.data.actors[&FormId(NPC_AUTO)].clone();
        mid.combat_style = Some(FormId(TOP));
        f.data.actors.insert(FormId(MID), mid);
        let parent = f.data.actors.get_mut(&FormId(NPC_AUTO)).unwrap();
        parent.template = Some(FormId(MID));
        parent.template_traits = mid_traits;
        let g = game(&f);
        assert_eq!(g.combat_style_id(me), Some(FormId(style)), "parent's Traits flag {mid_traits}");
    }
    // A leveled list template stands for its picked entry.
    let mut f = with(true, false);
    f.data.leveled_actors.insert(FormId(LIST), fnv_game::data::LeveledActorDef { entries: vec![(1, FormId(NPC_AUTO))], counts: vec![1], ..Default::default() });
    f.data.actors.get_mut(&FormId(NPC)).unwrap().template = Some(FormId(LIST));
    let g = game(&f);
    assert_eq!(g.combat_style_id(me), Some(FormId(PARENT)));
    // The precedence is unchanged: the running package's style, then SetCombatStyle's,
    // then the record's from its Traits source.
    let mut f = with(true, false);
    f.data.package_defs.get_mut(&FormId(PACK_IDLE)).unwrap().combat_style = Some(FormId(PACKAGE));
    let mut g = game(&f);
    g.extra.entry(me).or_default().combat_style = Some(FormId(SCRIPTED));
    assert_eq!(g.combat_style_id(me), Some(FormId(SCRIPTED)));
    g.extra.entry(me).or_default().current_package = Some(FormId(PACK_IDLE));
    assert_eq!(g.combat_style_id(me), Some(FormId(PACKAGE)));
    g.extra.entry(me).or_default().current_package = None;
    g.extra.entry(me).or_default().combat_style = None;
    assert_eq!(g.combat_style_id(me), Some(FormId(PARENT)));
}

#[test]
fn the_player_speaks_with_the_default_objects_voice_of_her_or_his_sex() {
    // Issue #2088: the player record names PlayerVoiceMale; a female player's Hit,
    // PowerAttack and Death lines are in PlayerVoiceFemale's folder. The DefaultObjectManager
    // names both (places 14 and 16); the player speaks with the one of her or his sex.
    const VOICE_MALE: u32 = 0x9E1;
    const VOICE_FEMALE: u32 = 0x9E2;
    struct Sounds;
    impl fnv_game::voice::VoiceLengths for Sounds {
        fn response_seconds(&self, _info: FormId, _response: usize, _speaker_base: Option<FormId>) -> Option<f64> {
            Some(0.9)
        }
        fn response_seconds_in_voice(&self, info: FormId, response: usize, voice_type: FormId) -> Option<f64> {
            if info != FormId(INFO_A) || response != 0 {
                return None;
            }
            match voice_type.0 {
                VOICE_MALE => Some(0.3),
                VOICE_FEMALE => Some(0.5),
                _ => None,
            }
        }
    }
    let mut f = fixture();
    let mut list = vec![None; 34];
    list[14] = Some(FormId(VOICE_MALE));
    list[16] = Some(FormId(VOICE_FEMALE));
    f.data.default_objects = list;
    let player = Some(FormId::PLAYER_BASE);
    let mut g = game(&f);
    g.voice = Some(std::sync::Arc::new(Sounds));
    g.player.female = Some(true);
    assert_eq!(g.voice_type_of(FormId::PLAYER_REF), Some(FormId(VOICE_FEMALE)));
    assert_eq!(g.voice_override(player), Some(FormId(VOICE_FEMALE)));
    assert_eq!(g.response_lengths_as(player, FormId(INFO_A)), vec![0.5]);
    g.player.female = Some(false);
    assert_eq!(g.voice_type_of(FormId::PLAYER_REF), Some(FormId(VOICE_MALE)));
    assert_eq!(g.response_lengths_as(player, FormId(INFO_A)), vec![0.3]);
    // A child race (Tale of Two Wastelands' Vault 101 childhood): the child places,
    // the adult one of the same sex when the list names no child voice.
    const CHILD_RACE: u32 = 0x9E5;
    const VOICE_MALE_CHILD: u32 = 0x9E3;
    const VOICE_FEMALE_CHILD: u32 = 0x9E4;
    let mut f = fixture();
    let mut list = vec![None; 34];
    list[14] = Some(FormId(VOICE_MALE));
    list[15] = Some(FormId(VOICE_MALE_CHILD));
    list[16] = Some(FormId(VOICE_FEMALE));
    f.data.default_objects = list.clone();
    f.data.child_races.insert(FormId(CHILD_RACE));
    let mut c = game(&f);
    c.player.race = Some(FormId(CHILD_RACE));
    c.player.female = Some(false);
    assert_eq!(c.voice_type_of(FormId::PLAYER_REF), Some(FormId(VOICE_MALE_CHILD)));
    c.player.female = Some(true);
    assert_eq!(c.voice_type_of(FormId::PLAYER_REF), Some(FormId(VOICE_FEMALE)));
    list[17] = Some(FormId(VOICE_FEMALE_CHILD));
    f.data.default_objects = list;
    let mut c = game(&f);
    c.player.race = Some(FormId(CHILD_RACE));
    c.player.female = Some(true);
    assert_eq!(c.voice_type_of(FormId::PLAYER_REF), Some(FormId(VOICE_FEMALE_CHILD)));
    // Grown up again: the adult voice.
    c.player.race = None;
    assert_eq!(c.voice_type_of(FormId::PLAYER_REF), Some(FormId(VOICE_FEMALE)));
    // Other speakers keep their record's voice.
    assert_eq!(g.voice_override(Some(FormId(NPC))), None);
    assert_eq!(g.response_lengths_as(Some(FormId(NPC)), FormId(INFO_A)), vec![0.9]);
    // A list without the places: the player record's own voice.
    let f = fixture();
    let mut g = game(&f);
    g.voice = Some(std::sync::Arc::new(Sounds));
    g.player.female = Some(true);
    assert_eq!(g.voice_override(player), None);
    assert_eq!(g.response_lengths_as(player, FormId(INFO_A)), vec![0.9]);
}

#[test]
fn a_message_box_is_a_menu_until_closed_and_holds_the_rest_of_the_frame() {
    use fnv_game::game::menus;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.set_stage(q(), 10);
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    assert_eq!(g.menu, Some(menus::MESSAGE));
    // The quest timer (1 s, set by stage 10) does not run while the box is open.
    let t0 = g.time;
    g.frames(300);
    assert_eq!(g.time, t0);
    assert!(!g.quest_state(q()).stages_done.contains(&20));
    // A second box waits for the first.
    g.show_box(FormId(MESSAGE));
    assert_eq!(g.close_menu(), Some(menus::MESSAGE));
    assert_eq!(g.menu, Some(menus::MESSAGE));
    // Closing a box that waits for a button presses OK (button 0).
    assert_eq!(g.message_box.as_ref().unwrap().pressed, Some(0));
    g.close_menu();
    assert_eq!(g.menu, None);
    g.frames(120);
    assert!(g.quest_state(q()).stages_done.contains(&20));
}

#[test]
fn the_runner_runs_the_round_4_operations() {
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "given": [{"kind": "menu", "menu": 1001, "open": true}],
        "actions": [
            {"op": "set_stage", "quest": "Q", "stage": 10},
            {"op": "get_menus", "menus": [1001, 1051], "label": "menus at the start"},
            {"op": "dismiss_message_boxes", "max": 2, "wait_frames": 5, "label": "boxes"},
            {"op": "set_quest_var", "quest": "Q", "var": "fTimer", "value": 1, "stopwatch": true},
            {"op": "wait_quest", "quest": "Q", "field": "done.20", "value": true, "max_frames": 600, "label": "Q 20", "stopwatch": true},
            {"op": "wait_menu", "menu": 1051, "max_frames": 5, "label": "no name menu"},
            {"op": "player_activate", "target": "TestActivatorREF"},
            {"op": "close_menus"},
            {"op": "start_stopwatch"},
            {"op": "get_tag_skills", "skills": ["Guns"], "label": "tags"},
            {"op": "get_ref", "ref": "TestActivatorREF", "fields": ["destroyed"], "label": "ref"}
        ]
    }))
    .unwrap();
    let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    let obs = out["observations"].as_array().unwrap();
    let by = |l: &str| obs.iter().find(|o| o["label"] == l).unwrap_or_else(|| panic!("no {l}")).clone();
    assert_eq!(by("menus at the start")["menu.1001"], true);
    assert_eq!(by("menus at the start")["menu.1051"], false);
    assert_eq!(by("boxes")["closed"], 1);
    let w = by("Q 20");
    assert_eq!(w["found"], true);
    // The quest runs every 0.5 s and its countdown ends below 0: on its third or fourth
    // run after the countdown was set (the phase of the 0.5 s cycle varies).
    let secs = w["seconds"].as_f64().unwrap();
    assert!((1.0..=2.1).contains(&secs), "{secs}");
    assert_eq!(by("no name menu")["found"], false);
    assert_eq!(by("tags")["tag.Guns"], false);
    assert_eq!(by("ref")["destroyed"], false);
    assert!(out["steps"].as_array().unwrap().iter().all(|s| s.get("error").is_none()), "{}", out["steps"]);
}

#[test]
fn saving_functions_ask_the_engine() {
    use fnv_game::functions::is_stub;
    // Autosave, SystemSave and ForceSave ask the engine for a save (nvf_play::save); the
    // game layer itself writes nothing.
    let f = fixture();
    let mut g = game(&f);
    for op in [0x115E, 0x127B, 0x1217] {
        assert!(!is_stub(op), "{op:#X}");
        let ctx = fnv_game::functions::Ctx { unit: "test".into(), opcode: op, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        g.call_function(&ctx, &[]);
        g.call_function(&ctx, &[]);
    }
    assert_eq!((g.ui.autosaves, g.ui.system_saves, g.ui.force_saves), (2, 2, 2));
}

#[test]
fn combat_hits_damage_health_kill_and_reward() {
    use fnv_game::combat::HitInfo;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let npc = FormId(NPC_REF);
    // A hand-set NPC: Base Health 30 + (Endurance 6 − 1) × 5; its level adds nothing.
    assert_eq!(g.actor_level(npc), 4.0);
    assert_eq!(g.max_health(npc), 55.0);
    // An auto-calculated one: Base Health 10 + (Endurance 5 − 1) × 5 + (level 5 − 1) × 5,
    // the original's NCR troopers' 50 (npc-behaviour-compare.md §9).
    assert_eq!(g.max_health(FormId(NPC_AUTO_REF)), 50.0);
    let by_player = HitInfo { attacker: Some(FormId::PLAYER_REF), location: 1, critical: true, ..HitInfo::default() };
    let r = g.hit(npc, 30.0, by_player);
    assert_eq!((r.dealt, r.health, r.died), (30.0, 25.0, false));
    assert_eq!(g.extra[&npc].last_hit.map(|h| (h.location, h.critical)), Some((1, true)));
    // The killing hit takes what is left; level 4 is an average NPC: 20 XP.
    let r = g.hit(npc, 60.0, by_player);
    assert_eq!((r.dealt, r.health, r.died, r.xp), (25.0, 0.0, true, 20.0));
    // The damage beyond the 25 left is overkill (docs/spec-notes/dismemberment.md §4).
    assert_eq!(r.overkill, 35.0);
    assert!(g.is_dead(npc) && g.extra[&npc].killer == Some(FormId::PLAYER_REF));
    assert_eq!(g.player.xp, 20.0);
    let r = g.hit(npc, 10.0, by_player);
    assert!(r.immune && r.corpse && r.overkill == 10.0);
    // An essential actor falls unconscious at 1 health instead.
    let mut g = game(&f);
    g.new_game();
    g.extra.entry(npc).or_default().essential = Some(true);
    let r = g.hit(npc, 500.0, by_player);
    assert!(r.knocked_out && !r.died && r.health == 1.0 && !g.is_dead(npc));
    // Others did most of the damage: no XP.
    let mut g = game(&f);
    g.new_game();
    g.hit(npc, 45.0, HitInfo { attacker: None, ..by_player });
    let r = g.hit(npc, 10.0, by_player);
    assert!(r.died && r.xp == 0.0);
}

/// A hired companion's kill earns the player the same XP as the player's own (owner,
/// 2026-10-27); anyone else's earns none.
#[test]
fn a_hired_companions_kill_earns_the_player_xp() {
    use fnv_game::combat::HitInfo;
    let f = fixture();
    let (npc, mate) = (FormId(NPC_REF), FormId(NPC_AUTO_REF));
    let by_mate = HitInfo { attacker: Some(mate), location: 1, ..HitInfo::default() };
    // Not hired: no XP.
    let mut g = game(&f);
    g.new_game();
    let r = g.hit(npc, 60.0, by_mate);
    assert!(r.died && r.xp == 0.0 && g.player.xp == 0.0);
    // Hired: the player's 20 XP for a level 4 NPC.
    let mut g = game(&f);
    g.new_game();
    g.extra.entry(mate).or_default().teammate = true;
    let r = g.hit(npc, 60.0, by_mate);
    assert!(r.died && g.extra[&npc].killer == Some(mate));
    assert_eq!((r.xp, g.player.xp), (20.0, 20.0));
    // The player's and the companion's damage count together towards the share.
    let mut g = game(&f);
    g.new_game();
    g.extra.entry(mate).or_default().teammate = true;
    g.hit(npc, 20.0, HitInfo { attacker: Some(FormId::PLAYER_REF), ..by_mate });
    g.hit(npc, 20.0, HitInfo { attacker: None, ..by_mate });
    let r = g.hit(npc, 20.0, by_mate);
    assert!(r.died && r.xp == 20.0, "{r:?}");
}

/// GetKillingBlowLimb and GetCauseofDeath answer from the hit that killed, not from an
/// earlier wound; GetHitLocation still reads the last hit (#1588).
#[test]
fn only_the_hit_that_killed_is_the_killing_blow() {
    use fnv_game::combat::HitInfo;
    const HIT_LOCATION: u16 = 0x1187;
    const CAUSE: u16 = 0x118D;
    const KILLING_BLOW_LIMB: u16 = 0x11F0;
    const KILL_ACTOR: u16 = 0x108B;
    let f = fixture();
    let npc = FormId(NPC_REF);
    let call = |g: &mut Game, op: u16, args: &[ArgValue]| {
        let ctx = Ctx { unit: "test".into(), opcode: op, on: Some(npc), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        g.call_function(&ctx, args).num()
    };
    let wound = HitInfo { attacker: Some(FormId::PLAYER_REF), location: 3, critical: true, ..HitInfo::default() };

    // A wound, then a KillActor with no cause: no hit killed.
    let mut g = game(&f);
    g.new_game();
    assert!(!g.hit(npc, 10.0, wound).died);
    assert_eq!((call(&mut g, KILLING_BLOW_LIMB, &[]), call(&mut g, CAUSE, &[])), (-1.0, -1.0), "alive");
    call(&mut g, KILL_ACTOR, &[]);
    assert!(g.is_dead(npc));
    assert_eq!((call(&mut g, KILLING_BLOW_LIMB, &[]), call(&mut g, CAUSE, &[])), (-1.0, -1.0), "the wound did not kill");
    assert_eq!(call(&mut g, HIT_LOCATION, &[]), 3.0, "still its last hit");
    assert!(!g.kill_outcome(npc).critical, "the wound's critical is not the kill's");

    // KillActor's own cause still counts, and it is no killing blow.
    let mut g = game(&f);
    g.new_game();
    g.hit(npc, 10.0, wound);
    call(&mut g, KILL_ACTOR, &[ArgValue::Value(Value::Ref(FormId::PLAYER_REF.0)), ArgValue::Value(Value::Num(-1.0)), ArgValue::Value(Value::Num(1.0))]);
    assert_eq!((call(&mut g, KILLING_BLOW_LIMB, &[]), call(&mut g, CAUSE, &[])), (-1.0, 1.0));

    // A wound, then a fatal hit: the fatal one, which a hit on the corpse does not replace.
    let mut g = game(&f);
    g.new_game();
    g.hit(npc, 10.0, wound);
    assert!(g.hit(npc, 500.0, HitInfo { location: 1, ..wound }).died);
    g.hit(npc, 5.0, HitInfo { location: 7, critical: false, ..wound });
    assert_eq!((call(&mut g, KILLING_BLOW_LIMB, &[]), call(&mut g, CAUSE, &[])), (1.0, 4.0));
    assert!(g.kill_outcome(npc).critical);

    // A save keeps the record.
    let snap = serde_json::to_value(g.snapshot()).unwrap();
    let back: fnv_game::game::GameSnapshot = serde_json::from_value(snap.clone()).unwrap();
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert_eq!((call(&mut h, KILLING_BLOW_LIMB, &[]), call(&mut h, CAUSE, &[])), (1.0, 4.0));

    // A save from before the record: what killed is unknown, not taken from the last hit.
    let mut old = snap;
    for e in old["extra"].as_object_mut().unwrap().values_mut() {
        e.as_object_mut().unwrap().remove("killing_hit");
    }
    let old: fnv_game::game::GameSnapshot = serde_json::from_value(old).unwrap();
    let world = World::restore(&f.forms, &f.refs, &old.sim).unwrap();
    let mut o = Game::restore(&f.data, world, g.cfg, &old).unwrap();
    assert!(o.is_dead(npc));
    assert_eq!((call(&mut o, KILLING_BLOW_LIMB, &[]), call(&mut o, CAUSE, &[])), (-1.0, -1.0));
    assert_eq!(call(&mut o, HIT_LOCATION, &[]), 1.0);

    // Back from the dead the record is gone: a later death without a hit reads none.
    g.resurrect(npc, false);
    assert_eq!(g.extra[&npc].killing_hit, None);
    assert_eq!(call(&mut g, KILLING_BLOW_LIMB, &[]), -1.0);
    call(&mut g, KILL_ACTOR, &[]);
    assert_eq!((call(&mut g, KILLING_BLOW_LIMB, &[]), call(&mut g, CAUSE, &[])), (-1.0, -1.0));
}

/// The player's death is no kill: not by their own hit, a companion's or their own
/// KillActor; the same kills of an NPC still count (#1614) [design].
#[test]
fn a_players_death_earns_no_kill_credit() {
    use fnv_game::challenges::{kind, ChallengeDef};
    use fnv_game::combat::HitInfo;
    use fnv_game::misc_stats::{PEOPLE_KILLED, TOTAL_THINGS_KILLED};
    const KILL_ACTOR: u16 = 0x108B;
    const HEALTH: u16 = 16;
    let p = FormId::PLAYER_REF;
    let (npc, mate, chal) = (FormId(NPC_REF), FormId(NPC_AUTO_REF), FormId(0x9990));
    let mut f = fixture();
    let def = ChallengeDef { form: chal, kind: kind::KILL_CATEGORY, threshold: 100, flags: 0, interval: 0, value1: 0, value2: 0, value3: 0, form1: None, form2: None, name: None, description: None };
    f.data.challenge_defs.insert(chal, def);
    let by_player = HitInfo { attacker: Some(p), location: 0, ..HitInfo::default() };
    let credit = |g: &Game| (g.player.xp, g.misc_stat(PEOPLE_KILLED), g.misc_stat(TOTAL_THINGS_KILLED), g.player.challenges.get(&chal).map_or(0, |c| c.progress));
    let start = || {
        let mut g = game(&f);
        g.new_game();
        g.extra.entry(mate).or_default().teammate = true;
        fnv_game::functions::set_actor_value_current(&mut g, p, HEALTH, 100.0).unwrap();
        g
    };

    // The controls: the player's kill and a hired companion's.
    let mut g = start();
    assert!(g.hit(npc, 500.0, by_player).died);
    assert_eq!(credit(&g), (20.0, 1.0, 1.0, 1));
    let mut g = start();
    assert!(g.hit(npc, 500.0, HitInfo { attacker: Some(mate), ..by_player }).died);
    assert_eq!(credit(&g), (20.0, 0.0, 0.0, 0));

    // By the player's own hit (their own blast's, in play).
    let mut g = start();
    let r = g.hit(p, 500.0, by_player);
    assert!(r.died && r.xp == 0.0, "{r:?}");
    assert_eq!(g.extra[&p].killer, Some(p), "the killer stays");
    assert_eq!(credit(&g), (0.0, 0.0, 0.0, 0));

    // By a hired companion's.
    let mut g = start();
    let r = g.hit(p, 500.0, HitInfo { attacker: Some(mate), ..by_player });
    assert!(r.died && r.xp == 0.0, "{r:?}");
    assert_eq!(g.extra[&p].killer, Some(mate));
    assert_eq!(credit(&g), (0.0, 0.0, 0.0, 0));

    // By their own KillActor.
    let mut g = start();
    let ctx = Ctx { unit: "test".into(), opcode: KILL_ACTOR, on: Some(p), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[ArgValue::Value(Value::Ref(p.0))]);
    assert!(g.is_dead(p));
    assert_eq!(credit(&g), (0.0, 0.0, 0.0, 0));
}

/// Gone parts: IsLimbGone, GetKillingBlowLimb, KillActor's limb, the Dismembered Limbs
/// statistic (docs/spec-notes/dismemberment.md §5).
#[test]
fn dismembered_parts_are_kept_and_read_by_scripts() {
    use fnv_game::combat::{CombatRequest, HitInfo};
    const IS_LIMB_GONE: u16 = 0x118E;
    const KILLING_BLOW_LIMB: u16 = 0x11F0;
    const KILL_ACTOR: u16 = 0x108B;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let npc = FormId(NPC_REF);
    let call = |g: &mut Game, op: u16, args: &[ArgValue]| {
        let ctx = Ctx { unit: "test".into(), opcode: op, on: Some(npc), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
        g.call_function(&ctx, args).num()
    };
    let nums = |v: &[f64]| v.iter().map(|&v| ArgValue::Value(Value::Num(v))).collect::<Vec<_>>();
    assert_eq!(call(&mut g, KILLING_BLOW_LIMB, &[]), -1.0);
    let head = HitInfo { attacker: Some(FormId::PLAYER_REF), location: 1, ..HitInfo::default() };
    assert!(g.hit(npc, 500.0, head).died);
    assert_eq!(call(&mut g, KILLING_BLOW_LIMB, &[]), 1.0);
    // A hit on the corpse is not the killing blow.
    g.hit(npc, 5.0, HitInfo { location: 7, ..head });
    assert_eq!(call(&mut g, KILLING_BLOW_LIMB, &[]), 1.0);
    assert_eq!(call(&mut g, IS_LIMB_GONE, &nums(&[1.0])), 0.0);
    // The head explodes and the brain (sharing its actor value) is severed: two parts.
    assert_eq!(g.dismember(npc, &[(1, true), (13, false)], Some(FormId::PLAYER_REF)).len(), 2);
    assert_eq!(g.misc_stat(fnv_game::misc_stats::DISMEMBERED_LIMBS), 2.0);
    // Gone once only.
    assert!(g.dismember(npc, &[(1, true)], Some(FormId::PLAYER_REF)).is_empty());
    assert_eq!(g.misc_stat(fnv_game::misc_stats::DISMEMBERED_LIMBS), 2.0);
    assert_eq!(call(&mut g, IS_LIMB_GONE, &nums(&[1.0])), 1.0);
    assert_eq!(call(&mut g, IS_LIMB_GONE, &nums(&[3.0])), 0.0);
    assert_eq!(call(&mut g, IS_LIMB_GONE, &nums(&[0.0, 10.0])), 1.0);
    assert_eq!(g.limbs_gone(npc), vec![(1, true), (13, false)]);
    // KillActor's limb on a dead actor asks the engine, which knows its body parts.
    g.combat_requests.clear();
    call(&mut g, KILL_ACTOR, &[]);
    assert!(g.combat_requests.is_empty());
    call(&mut g, KILL_ACTOR, &[ArgValue::Value(Value::Ref(FormId::PLAYER_REF.0)), ArgValue::Value(Value::Num(3.0))]);
    assert_eq!(g.combat_requests, vec![CombatRequest::Dismember { actor: npc, location: 3, by: Some(FormId::PLAYER_REF), explode: None }]);
    // #1846: ResurrectActor 1 does not update the body; ResurrectActor 0 brings it back whole.
    g.resurrect(npc, true);
    assert_eq!(g.limbs_gone(npc), vec![(1, true), (13, false)], "animate 1 keeps them gone");
    assert!(g.hit(npc, 500.0, head).died);
    g.resurrect(npc, false);
    assert!(g.limbs_gone(npc).is_empty(), "animate 0: whole again");
    assert_eq!(call(&mut g, IS_LIMB_GONE, &nums(&[0.0, 10.0])), 0.0);
}

#[test]
fn a_hit_on_an_object_runs_its_hit_blocks() {
    use fnv_game::combat::HitInfo;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let target = FormId(ACT_REF);
    let hits = |g: &Game| g.var(InstKey::Ref { reference: target }, 2).num();
    let by_player = HitInfo { attacker: Some(FormId::PLAYER_REF), location: -1, ..HitInfo::default() };
    // Without a weapon: `OnHit player` only.
    assert!(g.hit_object(target, by_player));
    assert_eq!(hits(&g), 10.0);
    // With one (any form stands in for it): OnHit, then OnHitWith.
    assert!(g.hit_object(target, HitInfo { weapon: Some(FormId(MESSAGE)), ..by_player }));
    assert_eq!(hits(&g), 21.0);
    // Someone else's shot: `OnHit player` does not accept it, OnHitWith does.
    assert!(g.hit_object(target, HitInfo { attacker: Some(FormId(NPC_REF)), weapon: Some(FormId(MESSAGE)), ..by_player }));
    assert_eq!(hits(&g), 22.0);
    // The object takes no damage and remembers no hit.
    assert!(g.extra.get(&target).is_none_or(|e| e.last_hit.is_none() && e.damage.is_none()));
    // A block whose parameter is a form list runs for a weapon in the list (Old World
    // Blues' force fields: `Begin OnHitWith NVDLC03SonicWeaponList`), not for another.
    g.form_lists.insert(FormId(QUEST), vec![FormId(NPC_REF)]);
    assert!(g.hit_object(target, HitInfo { weapon: Some(FormId(NPC_REF)), ..by_player }));
    assert_eq!(hits(&g), 133.0);
    assert!(g.hit_object(target, HitInfo { weapon: Some(FormId(MESSAGE)), ..by_player }));
    assert_eq!(hits(&g), 144.0);
    // A reference whose script has no hit block.
    assert!(!g.hit_object(FormId(NPC_REF), by_player));
}

#[test]
fn the_runners_hit_runs_an_objects_hit_blocks() {
    // A stand-in for a scripted shot (Lonesome Road's ED-E zapping the silo mainframe).
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "actions": [
            {"op": "hit", "target": "TestActivatorREF", "label": "by the player"},
            {"op": "hit", "target": "TestActivatorREF", "attacker": "TestNpcREF", "weapon": "TestMsg", "label": "by another with a weapon"},
            {"op": "get_ref", "ref": "TestActivatorREF", "fields": ["var.nHit"], "label": "count"}
        ]
    }))
    .unwrap();
    let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    let obs = out["observations"].as_array().unwrap();
    let by = |l: &str| obs.iter().find(|o| o["label"] == l).unwrap().clone();
    assert_eq!(by("by the player")["ran"], true);
    assert_eq!(by("by another with a weapon")["ran"], true);
    // OnHit player (10), then only OnHitWith for the other attacker (1).
    assert_eq!(by("count")["var.nHit"], 11.0);
}

/// The runner's `hit` on an actor is a hit as in play: the actor turns on the player and
/// fights (issue #1114: a hit started no AI combat headless, the chains called
/// StartCombat instead); and with no detection measured the awareness update still ends
/// a fight whose target died (Simon after the bomber).
#[test]
fn the_runners_hit_on_an_actor_starts_a_fight_and_a_fight_ends_with_its_target_dead() {
    let f = fixture();
    let input: fnv_game::runner::Input = serde_json::from_value(json!({
        "format": "fnv.script.actions/1",
        "actions": [
            {"op": "call", "line": "TestNpcREF.IsInCombat", "label": "calm"},
            {"op": "hit", "target": "TestNpcREF", "label": "struck"},
            {"op": "advance_seconds", "seconds": 0.1},
            {"op": "call", "line": "TestNpcREF.IsInCombat", "label": "fights"},
            {"op": "call", "line": "TestNpcREF.GetCombatTarget", "label": "target"},
            {"op": "call", "line": "TestAutoNpcREF.StartCombat TestNpcREF"},
            {"op": "call", "line": "TestAutoNpcREF.IsInCombat", "label": "other fights"},
            {"op": "kill", "target": "TestNpcREF"},
            {"op": "advance_seconds", "seconds": 0.1},
            {"op": "call", "line": "TestAutoNpcREF.IsInCombat", "label": "target dead"}
        ]
    }))
    .unwrap();
    let out = fnv_game::runner::run(&f.data, &f.forms, &f.refs, &input).unwrap();
    let obs = out["observations"].as_array().unwrap();
    let by = |l: &str| obs.iter().find(|o| o["label"] == l).unwrap().clone();
    assert_eq!(by("calm")["value"], 0.0);
    assert_eq!((by("struck")["died"].clone(), by("struck")["health"].as_f64().is_some()), (json!(false), true));
    assert_eq!(by("fights")["value"], 1.0);
    assert_eq!(by("target")["value"], FormId::PLAYER_REF.to_string());
    assert_eq!(by("other fights")["value"], 1.0);
    assert_eq!(by("target dead")["value"], 0.0);
}

#[test]
fn weapon_out_and_weapon_animation_type() {
    use fnv_game::functions::weapon_anim_code;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let ask = |g: &mut Game, opcode: u16| {
        let ctx = Ctx { unit: "test".into(), opcode, on: Some(FormId::PLAYER_REF), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: true };
        g.call_function(&ctx, &[]).num()
    };
    const IS_WEAPON_OUT: u16 = 0x1065;
    const GET_WEAPON_ANIM_TYPE: u16 = 0x106C;
    assert_eq!(ask(&mut g, IS_WEAPON_OUT), 0.0);
    g.set_weapon_out(FormId::PLAYER_REF, true);
    assert_eq!(ask(&mut g, IS_WEAPON_OUT), 1.0);
    g.set_weapon_out(FormId::PLAYER_REF, false);
    assert_eq!(ask(&mut g, IS_WEAPON_OUT), 0.0);
    // Nothing equipped: Hand to Hand.
    assert_eq!(ask(&mut g, GET_WEAPON_ANIM_TYPE), 1.0);
    // The record's 14 types (0 Hand to Hand … 13 Thrown) as the function numbers them:
    // both pistols 4, the ballistic and energy rifles 5, Thrown the catch-all.
    let codes: Vec<i64> = (0..=13).map(weapon_anim_code).collect();
    assert_eq!(codes, [1, 2, 3, 4, 4, 5, 6, 5, 7, 8, 9, 10, 11, 0]);
    assert_eq!(weapon_anim_code(99), 0);
}

#[test]
fn player_limb_losses_fit_the_10mm_capture_at_200_maximum_health() {
    use fnv_game::actor_values::HEALTH;
    use fnv_game::combat::{HitInfo, LimbHit, HEAD, LEFT_ARM, LEFT_MOBILITY, RIGHT_ARM, RIGHT_MOBILITY, TORSO};
    use fnv_game::functions::set_actor_value_current;
    let f = fixture();
    let p = FormId::PLAYER_REF;
    // [observed: o17_air limbs_10mm, check 3] The losses below; maximum Health 200
    // remains open (see docs/spec-notes/combat-rules.md), since that run did not record its base Health.
    for (condition, share, health_damage, limb_damage, loss) in [
        (LEFT_ARM, 25.0, 16.06, 16.06, 16.06),
        (RIGHT_ARM, 25.0, 16.06, 16.06, 16.06),
        (LEFT_MOBILITY, 25.0, 16.06, 16.06, 16.06),
        (RIGHT_MOBILITY, 25.0, 16.06, 16.06, 16.06),
        (TORSO, 60.0, 16.06, 16.06, 6.69),
        (HEAD, 20.0, 32.12, 16.06, 20.075),
        (HEAD, 20.0, 76.12, 38.06, 47.575),
    ] {
        let mut g = game(&f);
        g.new_game();
        set_actor_value_current(&mut g, p, HEALTH, 200.0).unwrap();
        let info = HitInfo { attacker: Some(FormId(NPC_REF)), limb: Some(LimbHit { condition, health_percent: share, damage: limb_damage, points: false }), ..Default::default() };
        let r = g.hit(p, health_damage, info);
        assert_eq!(r.max_health, 200.0);
        assert!((r.dealt - health_damage).abs() < 1e-9);
        let (code, after) = r.limb.unwrap();
        assert_eq!(code, condition);
        assert!((100.0 - after - loss).abs() < 0.005, "condition {condition}: loss {} against {loss}", 100.0 - after);
        assert!(!r.crippled);
    }
}

#[test]
fn limb_hits_lower_conditions_and_cripple() {
    use fnv_game::combat::{HitInfo, LimbHit};
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let npc = FormId(NPC_REF);
    // Maximum health 55; a leg at 25 %: a pool of 13.75. Left Mobility Condition is code 29.
    let leg = LimbHit { condition: 29, health_percent: 25.0, damage: 0.0, points: false };
    let hit = |damage: f64| HitInfo { attacker: Some(FormId::PLAYER_REF), location: 7, limb: Some(LimbHit { damage, ..leg }), ..HitInfo::default() };
    let r = g.hit(npc, 5.0, hit(2.75));
    assert_eq!((r.limb, r.crippled), (Some((29, 80.0)), false));
    assert!(!g.is_crippled(npc, 29));
    // 11 more limb damage (the weapon's limb multiplier is the caller's): crippled.
    let r = g.hit(npc, 5.0, hit(11.0));
    assert_eq!((r.limb, r.crippled), (Some((29, 0.0)), true));
    assert!(g.is_crippled(npc, 29));
    // It stays at 0 and cripples once.
    let r = g.hit(npc, 5.0, hit(10.0));
    assert_eq!((r.limb, r.crippled), (Some((29, 0.0)), false));
    // Health took only the hits' damage.
    assert_eq!(r.health, 40.0);
}

/// An explosion's limb damage follows the original's shares (docs/spec-notes/crippled-
/// limbs.md §5): for the five measured blasts (o25_limbs, Ringo; Health lost, then the
/// torso's and the legs' condition points lost) the torso loses 1/30 of the Health
/// damage and the legs 0.24 together, the head, arms and brain nothing [observed];
/// nothing on a dead actor. The points come straight off the condition, whatever the
/// actor's maximum Health (see docs/spec-notes/combat-rules.md; measured at 5000 Health, on an NPC).
#[test]
fn blasts_take_the_measured_limb_shares() {
    use fnv_game::combat::{blast_limb_shares, HEAD, LEFT_ARM, LEFT_MOBILITY, RIGHT_ARM, RIGHT_MOBILITY, TORSO};
    const BRAIN: u16 = 31;
    let f = fixture();
    let npc = FormId(NPC_REF);
    let human = [LEFT_MOBILITY, RIGHT_MOBILITY, TORSO, RIGHT_ARM, LEFT_ARM, HEAD, BRAIN];
    // (Health lost, torso, left leg + right leg) [observed 2026-11-06].
    let measured = [(99.95, 3.33, 23.99), (96.99, 3.27, 23.52), (98.97, 3.33, 24.00), (98.00, 3.27, 23.52), (123.0, 4.10, 29.52)];
    for (health, torso, legs) in measured {
        let mut g = game(&f);
        g.new_game();
        let hurt = g.blast_limbs(npc, None, &blast_limb_shares(&human, health));
        let lost = |c: u16| 100.0 - hurt.iter().find(|h| h.0 == c).map_or(100.0, |h| h.1);
        let close = |ours: f64, theirs: f64| (ours - theirs).abs() <= theirs * 0.015;
        assert!(close(lost(TORSO), torso), "torso {} against {torso} for {health}", lost(TORSO));
        assert!(close(lost(LEFT_MOBILITY) + lost(RIGHT_MOBILITY), legs), "legs against {legs} for {health}: {hurt:?}");
        for c in [HEAD, LEFT_ARM, RIGHT_ARM, BRAIN] {
            assert_eq!(lost(c), 0.0, "{c} for {health}");
            assert!(!g.is_crippled(npc, c));
        }
        // The left leg takes two thirds of the legs' share (see docs/spec-notes/combat-rules.md).
        assert!((lost(LEFT_MOBILITY) - 2.0 * lost(RIGHT_MOBILITY)).abs() < 1e-9);
    }
    // A blast big enough cripples the left leg first; never an arm or the head.
    let mut g = game(&f);
    g.new_game();
    let hurt = g.blast_limbs(npc, None, &blast_limb_shares(&human, 700.0));
    assert_eq!(hurt.iter().filter(|h| h.2).map(|h| h.0).collect::<Vec<_>>(), vec![LEFT_MOBILITY]);
    // One leg in the data takes the whole 0.24; no torso, no limb damage there.
    let one = blast_limb_shares(&[RIGHT_MOBILITY], 100.0);
    assert_eq!(one.len(), 1);
    assert!((one[0].damage - 24.0).abs() < 1e-9 && one[0].points);
    assert!(blast_limb_shares(&[HEAD, LEFT_ARM], 100.0).is_empty());
    assert!(blast_limb_shares(&human, 0.0).is_empty());
    g.kill(npc, None);
    assert!(g.blast_limbs(npc, None, &blast_limb_shares(&human, 100.0)).is_empty());
}

/// A fall's leg (docs/spec-notes/physics.md §6.12): limb damage in condition points comes
/// straight off the condition, without the part's pool or the player's limb multiplier.
#[test]
fn limb_damage_in_points() {
    use fnv_game::combat::{HitInfo, LimbHit, RIGHT_MOBILITY};
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let leg = |damage: f64| HitInfo { location: -1, limb: Some(LimbHit { condition: RIGHT_MOBILITY, health_percent: 0.0, damage, points: true }), ..HitInfo::default() };
    for target in [FormId(NPC_REF), FormId::PLAYER_REF] {
        let r = g.hit(target, 49.88, leg(24.94));
        assert_eq!(r.limb.map(|(c, v)| (c, (v * 100.0).round() / 100.0)), Some((RIGHT_MOBILITY, 75.06)));
        assert!(!r.crippled);
        // More than is left: down to 0, crippled.
        let r = g.hit(target, 1.0, leg(500.0));
        assert_eq!((r.limb, r.crippled), (Some((RIGHT_MOBILITY, 0.0)), true));
    }
}

/// Crippled legs slow every actor by the game settings, the player's limbs take half the
/// limb damage, and a sleep outside Hardcore heals them (docs/spec-notes/crippled-limbs.md).
#[test]
fn crippled_limbs_slow_and_heal() {
    use fnv_game::combat::{HitInfo, LimbHit, LEFT_MOBILITY, RIGHT_MOBILITY};
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let npc = FormId(NPC_REF);
    // fCombatPlayerLimbDamageMult's default: half for the player, all for others.
    assert_eq!(g.limb_damage(FormId::PLAYER_REF, 10.0), 5.0);
    assert_eq!(g.limb_damage(npc, 10.0), 10.0);
    let hit = |condition: u16| HitInfo { attacker: Some(FormId::PLAYER_REF), location: 7, limb: Some(LimbHit { condition, health_percent: 25.0, damage: 1000.0, points: false }), ..HitInfo::default() };
    assert_eq!(g.leg_speed_mult(npc), 1.0);
    assert!(g.hit(npc, 1.0, hit(LEFT_MOBILITY)).crippled);
    assert_eq!(g.leg_speed_mult(npc), 0.85);
    assert!(g.hit(npc, 1.0, hit(RIGHT_MOBILITY)).crippled);
    assert_eq!(g.leg_speed_mult(npc), 0.75);
    // The player's leg; an hour waited does not heal it, an hour slept does.
    assert!(g.hit(FormId::PLAYER_REF, 1.0, hit(LEFT_MOBILITY)).crippled);
    assert!(g.pass_hours(1.0, false).limbs_healed.is_empty());
    assert!(g.is_crippled(FormId::PLAYER_REF, LEFT_MOBILITY));
    assert_eq!(g.pass_hours(1.0, true).limbs_healed, vec![LEFT_MOBILITY]);
    assert!(!g.is_crippled(FormId::PLAYER_REF, LEFT_MOBILITY));
    assert_eq!(g.leg_speed_mult(FormId::PLAYER_REF), 1.0);
}

#[test]
fn furniture_functions_answer_from_the_use_set_on_the_actor() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let npc = FormId(NPC_REF);
    // The activator reference stands in for a chair: the functions only compare forms.
    let (chair, chair_base) = (FormId(ACT_REF), FormId(ACTIVATOR));
    let ask = |g: &mut Game, opcode: u16, on: FormId, arg: Option<FormId>| {
        let ctx = Ctx { unit: "test".into(), opcode, on: Some(on), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: true };
        let args: Vec<ArgValue> = arg.map(|a| ArgValue::Value(Value::Ref(a.0))).into_iter().collect();
        g.call_function(&ctx, &args).num()
    };
    const SITTING: u16 = 0x109F;
    const SLEEPING: u16 = 0x1031;
    const MARKER: u16 = 0x10A0;
    const IS_REF: u16 = 0x10A2;
    const IS_OBJ: u16 = 0x10A3;
    // Not on furniture.
    for op in [SITTING, SLEEPING, MARKER] {
        assert_eq!(ask(&mut g, op, npc, None), 0.0);
    }
    assert_eq!(ask(&mut g, IS_REF, npc, Some(chair)), 0.0);
    // Getting onto a chair by its left marker, then seated.
    g.set_furniture_use(npc, Some(FurnitureUse { reference: chair, marker_id: 11, state: 2 }));
    assert_eq!((ask(&mut g, SITTING, npc, None), ask(&mut g, SLEEPING, npc, None), ask(&mut g, MARKER, npc, None)), (2.0, 0.0, 11.0));
    g.set_furniture_use(npc, Some(FurnitureUse { reference: chair, marker_id: 11, state: 3 }));
    assert_eq!(ask(&mut g, SITTING, npc, None), 3.0);
    assert_eq!(ask(&mut g, IS_REF, npc, Some(chair)), 1.0);
    assert_eq!(ask(&mut g, IS_REF, npc, Some(npc)), 0.0);
    assert_eq!(ask(&mut g, IS_OBJ, npc, Some(chair_base)), 1.0);
    assert_eq!(ask(&mut g, IS_OBJ, npc, Some(FormId(NPC))), 0.0);
    // Someone else is not seated.
    assert_eq!(ask(&mut g, SITTING, FormId::PLAYER_REF, None), 0.0);
    // A bed marker answers GetSleeping instead.
    g.set_furniture_use(npc, Some(FurnitureUse { reference: chair, marker_id: 1, state: 3 }));
    assert_eq!((ask(&mut g, SITTING, npc, None), ask(&mut g, SLEEPING, npc, None)), (0.0, 3.0));
    // The use survives a snapshot and ends when cleared.
    let snap = serde_json::to_value(g.snapshot()).unwrap();
    assert_eq!(snap["extra"][&npc.to_string()]["furniture"]["marker_id"], json!(1));
    g.set_furniture_use(npc, None);
    assert_eq!(ask(&mut g, SLEEPING, npc, None), 0.0);
    assert_eq!(g.furniture_use(npc), None);
}

/// #2948: a teammate brought into another cell forgets the foes left in the one it came
/// from and, with none left, leaves the fight; a foe in its new cell is kept.
#[test]
fn a_teammate_brought_along_leaves_the_foes_of_the_other_cell_behind() {
    use fnv_game::awareness::{Awareness, State, Target};
    use fnv_game::detection::Band;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let (npc, foe, near) = (FormId(NPC_REF), FormId(PATROL_REF), FormId(OTHER_REF));
    let target = Target { band: Band::Hidden, value: 10.0, at: [0.0; 3], detected: Some(0.0), hidden_since: Some(0.0) };
    let mut searching = Awareness { state: State::Alert, ..Default::default() };
    searching.targets.insert(foe, target);
    let put = |g: &mut fnv_game::game::Game, r: FormId, cell: u32| g.world.get(r).unwrap().cell = Some(FormId(cell));
    put(&mut g, npc, OTHER_CELL);
    put(&mut g, foe, CELL);
    put(&mut g, near, OTHER_CELL);
    // The foe is in the same cell: kept.
    put(&mut g, foe, OTHER_CELL);
    g.awareness.actors.insert(npc, searching.clone());
    g.leave_foes_behind(npc);
    assert_eq!(g.awareness.state(npc), State::Alert);
    // Left behind in the other cell: forgotten, and the search is over.
    put(&mut g, foe, CELL);
    g.leave_foes_behind(npc);
    assert_eq!(g.awareness.state(npc), State::Normal);
    // One left behind, one in the new cell: only the first is forgotten.
    searching.targets.insert(near, target);
    g.awareness.actors.insert(npc, searching);
    g.leave_foes_behind(npc);
    let a = g.awareness.get(npc).expect("still searching for the near one");
    assert!(a.targets.contains_key(&near) && !a.targets.contains_key(&foe), "{:?}", a.targets.keys());
}

#[test]
fn a_level_is_earned_by_experience_and_waits_for_no_one_hunting_the_player() {
    use fnv_game::awareness::{Awareness, State, Target};
    use fnv_game::detection::Band;
    use fnv_game::experience;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    // The test plugin has no story add-on: the master's cap, or its default 30.
    assert_eq!(experience::max_level(&g), 30);
    assert_eq!(g.player_level(), 1.0);
    g.player.xp = 199.0;
    assert!(!experience::level_up_due(&g));
    g.player.xp = 204.0;
    assert!(experience::level_up_due(&g));
    // Not in character generation (SetInCharGen 1): the XP stays, the level waits.
    g.player.in_chargen = true;
    assert!(!experience::level_up_due(&g));
    g.player.in_chargen = false;
    // An actor searching for the player (Alert, the [CAUTION] state) keeps the menu shut.
    let npc = FormId(NPC_REF);
    let target = Target { band: Band::Detected, value: 10.0, at: [0.0; 3], detected: Some(0.0), hidden_since: None };
    let mut searching = Awareness { state: State::Alert, ..Default::default() };
    searching.targets.insert(FormId::PLAYER_REF, target);
    g.awareness.actors.insert(npc, searching.clone());
    assert_eq!(experience::hunters(&g), 1);
    // Not when it searches for someone else, nor when it is back to Normal.
    let mut other = searching.clone();
    other.targets.clear();
    g.awareness.actors.insert(npc, other);
    assert_eq!(experience::hunters(&g), 0);
    g.awareness.actors.insert(npc, Awareness { state: State::Normal, ..searching.clone() });
    assert_eq!(experience::hunters(&g), 0);
    // Nor once it is dead.
    g.awareness.actors.insert(npc, Awareness { state: State::Combat, ..searching });
    assert_eq!(experience::hunters(&g), 1);
    g.kill(npc, Some(FormId::PLAYER_REF));
    assert_eq!(experience::hunters(&g), 0);
    // Taking the level: level 2, the level-up menu open; no more is due.
    assert_eq!(experience::level_up(&mut g), 2);
    assert!(g.menu_open(fnv_game::game::menus::LEVEL_UP));
    assert!(!experience::level_up_due(&g));
}

/// `Begin MenuMode 1` runs for any of the Pip-Boy's menus; `Begin MenuMode 1007` under a
/// loading screen and `1056` while VATS marks, with nothing else running then
/// (`game::menus`).
#[test]
fn coded_menu_mode_blocks_run_for_their_menus() {
    use fnv_game::game::menus;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.set_loaded([FormId(CELL)].into_iter().collect());
    let key = InstKey::Ref { reference: FormId(ACT_REF) };
    let v = |g: &Game, i: u16| g.var(key, i).num();
    g.frames(2);
    let frames = v(&g, 3);
    assert!(frames > 0.0, "GameMode runs");
    assert_eq!((v(&g, 4), v(&g, 5), v(&g, 6)), (0.0, 0.0, 0.0));
    // The Pip-Boy by each of its pages: MenuMode 1.
    for code in [menus::INVENTORY, menus::STATS, menus::MAP, menus::REPAIR] {
        g.menu = Some(code);
        g.tick();
    }
    assert_eq!(v(&g, 4), 4.0);
    // Another menu is not the Pip-Boy.
    g.menu = Some(menus::MESSAGE);
    g.tick();
    assert_eq!(v(&g, 4), 4.0);
    g.menu = None;
    // A loading screen: only the 1007 block runs; GameMode does not, nor does time pass.
    let (t, frames) = (g.time, v(&g, 3));
    g.loading = true;
    g.frames(2);
    g.loading = false;
    assert_eq!((v(&g, 5), v(&g, 3), g.time), (2.0, frames, t));
    // VATS marking: GameMode goes on, and the 1056 block runs beside it.
    g.vats.mode = fnv_game::vats::MODE_TARGET_SELECT;
    g.tick();
    assert_eq!((v(&g, 6), v(&g, 3)), (1.0, frames + 1.0));
    g.vats.mode = fnv_game::vats::MODE_OFF;
    g.tick();
    assert_eq!(v(&g, 6), 1.0);
}

/// The pause menu's Mods page (#976 T11): `Game::page_tick` runs the start menu's
/// `MenuMode 1013` blocks and nothing else of a frame, and leaves the open menu (a box a
/// script showed) as it is.
#[test]
fn a_page_tick_runs_only_the_start_menus_blocks_and_keeps_the_open_menu() {
    use fnv_game::game::menus;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.set_loaded([FormId(CELL)].into_iter().collect());
    let key = InstKey::Ref { reference: FormId(ACT_REF) };
    let v = |g: &Game, i: u16| g.var(key, i).num();
    g.frames(2);
    assert_eq!(v(&g, 8), 0.0, "no 1013 block in play");
    let (t, frames) = (g.time, v(&g, 3));
    g.page_tick(menus::START);
    g.page_tick(menus::START);
    assert_eq!((v(&g, 8), v(&g, 3), v(&g, 4), g.time), (2.0, frames, 0.0, t), "only the 1013 block, no game time");
    assert_eq!(g.menu, None);
    g.menu = Some(menus::MESSAGE);
    g.page_tick(menus::START);
    assert_eq!((g.menu, v(&g, 8)), (Some(menus::MESSAGE), 3.0), "a box stays the open menu");
    g.menu = None;
    g.tick();
    assert_eq!(v(&g, 8), 3.0, "the page's menu ends with its tick");
}

fn ui_call(g: &mut Game, op: u16, args: &[ArgValue]) {
    let ctx = Ctx { unit: "test".into(), opcode: op, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    let before = g.events.len();
    g.call_function(&ctx, args);
    assert!(!g.events[before..].iter().any(|e| matches!(e.kind, EventKind::CallFailed { .. } | EventKind::Unhandled { .. })), "0x{op:04X}: {:?}", &g.events[before..]);
}

fn menu_mode(g: &mut Game, code: f64) -> f64 {
    let ctx = Ctx { unit: "test".into(), opcode: 0x1024, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[ArgValue::Value(Value::Num(code))]).num()
}

/// ShowTutorialMenu opens the tutorial menu (1059) over the Pip-Boy and gives the
/// Pip-Boy back when it closes; MenuMode answers for both meanwhile; a second message is
/// a second page; a message box waits for it.
#[test]
fn the_tutorial_menu_opens_over_the_pipboy_and_gives_it_back() {
    use fnv_game::game::menus;
    let f = fixture();
    let mut g = game(&f);
    g.open_menu(menus::STATS);
    assert_eq!((menu_mode(&mut g, 1.0), menu_mode(&mut g, 1003.0), menu_mode(&mut g, 1002.0)), (1.0, 1.0, 0.0));
    let msg = ArgValue::Value(Value::Ref(MESSAGE));
    ui_call(&mut g, 0x11E3, std::slice::from_ref(&msg));
    assert_eq!((g.menu, g.menu_under, g.tutorial.clone()), (Some(menus::TUTORIAL), Some(menus::STATS), vec![FormId(MESSAGE)]));
    assert_eq!((menu_mode(&mut g, 1059.0), menu_mode(&mut g, 1.0)), (1.0, 1.0));
    // Shown again while open: no second page of the same message.
    ui_call(&mut g, 0x11E3, &[msg]);
    assert_eq!(g.tutorial.len(), 1);
    // A message box waits until the tutorial and the Pip-Boy are closed.
    g.show_box(FormId(MESSAGE));
    assert_eq!(g.menu, Some(menus::TUTORIAL));
    assert_eq!(g.close_menu(), Some(menus::TUTORIAL));
    assert_eq!((g.menu, g.menu_under, g.tutorial.len()), (Some(menus::STATS), None, 0));
    // The Pip-Boy turns its page; the engine closes it by any code; the box opens.
    assert!(g.set_pipboy_menu(menus::MAP));
    assert_eq!(menu_mode(&mut g, 1023.0), 1.0);
    assert!(g.engine_menu_closed(menus::INVENTORY));
    assert_eq!(g.menu, Some(menus::MESSAGE));
    assert!(!g.set_pipboy_menu(menus::STATS), "a message box is no Pip-Boy page");
    g.close_menu();
    // The Pip-Boy put away under the tutorial: closing the tutorial leaves no menu.
    g.open_menu(menus::INVENTORY);
    g.show_tutorial(FormId(MESSAGE));
    assert!(g.engine_menu_closed(menus::INVENTORY));
    assert_eq!((g.menu, g.menu_under), (Some(menus::TUTORIAL), None));
    g.close_menu();
    assert_eq!(g.menu, None);
    // CloseAllMenus closes the tutorial and the menu under it [observed: mh1_tutorial].
    g.open_menu(menus::MESSAGE);
    g.show_tutorial(FormId(MESSAGE));
    assert_eq!(g.close_all_menus(), vec![menus::TUTORIAL, menus::MESSAGE]);
    assert_eq!(g.menu, None);
    // Not a message: refused.
    let ctx = Ctx { unit: "test".into(), opcode: 0x11E3, on: None, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    let before = g.events.len();
    g.call_function(&ctx, &[ArgValue::Value(Value::Ref(GLOBAL))]);
    assert!(g.events[before..].iter().any(|e| matches!(e.kind, EventKind::CallFailed { .. })));
    assert_eq!(g.menu, None);
    // A loading screen is MenuMode 1007, and a menu for MenuMode without a code.
    let ctx0 = Ctx { opcode: 0x1024, ..ctx };
    g.loading = true;
    assert_eq!((menu_mode(&mut g, 1007.0), g.call_function(&ctx0, &[]).num()), (1.0, 1.0));
    g.loading = false;
    assert_eq!(menu_mode(&mut g, 1007.0), 0.0);
}

/// ResetPipboyManager, ExitGame and AutoDisplayObjectives leave their requests for the
/// engine; the objective switch is saved with the player.
#[test]
fn pipboy_reset_exit_and_objective_display_requests() {
    let f = fixture();
    let mut g = game(&f);
    ui_call(&mut g, 0x1201, &[]);
    ui_call(&mut g, 0x1201, &[]);
    assert_eq!(g.ui.pipboy_resets, 2);
    ui_call(&mut g, 0x11B5, &[]);
    assert_eq!(g.ui.exit_game, 1);
    assert!(!g.player.auto_objectives);
    ui_call(&mut g, 0x1225, &[ArgValue::Value(Value::Num(1.0))]);
    assert!(g.player.auto_objectives);
    let saved = serde_json::to_value(&g.player).unwrap();
    assert_eq!(saved["auto_objectives"], true);
    ui_call(&mut g, 0x1225, &[ArgValue::Value(Value::Num(0.0))]);
    assert!(!g.player.auto_objectives);
    assert!(serde_json::to_value(&g.player).unwrap().get("auto_objectives").is_none());
}

#[test]
fn set_actor_full_name_renames_the_base_and_is_saved() {
    // GECK wiki SetActorFullName: the base's name becomes the message's title (FULL, not
    // the text), for every reference of the base; kept in the save with `Game::forms`.
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    assert_eq!(g.script_name(FormId(NPC)), None);
    let ctx = Ctx { unit: "test".into(), opcode: 0x111C, on: Some(FormId(NPC_REF)), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    g.call_function(&ctx, &[ArgValue::Value(Value::Ref(MESSAGE))]);
    assert_eq!(g.script_name(FormId(NPC)), Some("Renamed One"));
    let s = g.snapshot();
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert_eq!(h.script_name(FormId(NPC)), Some("Renamed One"));
}

#[test]
fn a_unit_whose_bytes_call_an_unknown_high_opcode_runs_from_its_source() {
    // Bytes as a mod compiled with the script extender loaded might hold them: a call to
    // an opcode above every opcode of the function table (fnv_game::source_compile). The
    // source says something else; the source is what runs.
    let bytes = script(&[(0, None, call(0x7FFF, &[]))]);
    let mut subs = unit(0, &bytes, &[(1, "a", 1)], &[]);
    subs.push(sub(b"SCTX", b"scn TestExtScript\nshort a\nbegin GameMode\n  set a to 5\nend\n".to_vec()));
    let scpt = record(b"SCPT", 0, 0x0000_0900, &[vec![sub(b"EDID", z("TestExtScript"))], subs].concat());
    let plugin = [tes4(), top(b"SCPT", &scpt)].concat();
    let plugins = vec![Plugin::parse(&plugin, "Ext.esp".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = schema();
    let (table, j) = catalogue();
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    let s = data.script(FormId(0x900)).expect("the script loads");
    let body = &s.program.blocks[0].body;
    assert!(matches!(&body[..], [fnv_script::Stmt::Set { .. }]), "the compiled source runs: {body:?}");
    assert_eq!(data.issues.count("script_compiled_from_source"), 1);
    assert!(fnv_game::source_compile::highest_opcode(&data.table) < 0x7FFF);
}

// ---- user-defined functions and lambdas (extender tasks B7, B8) -------------------------

/// No names: the function scripts below name only their own variables.
struct NoNames;

impl fnv_script::compile::Env for NoNames {
    fn form(&self, _: &str) -> Option<fnv_script::compile::FormInfo> {
        None
    }
    fn info(&self, _: u32) -> Option<fnv_script::compile::FormInfo> {
        None
    }
    fn editor_id(&self, _: u32) -> Option<String> {
        None
    }
    fn local_form(&self, _: u32) -> Option<u32> {
        None
    }
    fn script_vars(&self, _: u32) -> Option<Vec<fnv_script::VarDecl>> {
        None
    }
}

/// A plugin with the global TestGlobal and function scripts compiled by NVForever's
/// compiler: TestAdd {a, b} returns a + b; TestFact {n} returns n! by calling itself.
fn udf_fixture() -> Fixture {
    let (table, _) = catalogue();
    let compiled = |src: &str| {
        let c = fnv_script::compile::compile(src, &table, &NoNames, None);
        assert!(c.errors().next().is_none(), "{:?}", c.diags);
        c
    };
    let glob = record(b"GLOB", 0, 0x0000_0800, &[sub(b"EDID", z("TestGlobal")), sub(b"FNAM", vec![b'f']), sub(b"FLTV", 0f32.to_le_bytes().to_vec())]);
    let mut scripts = Vec::new();
    for (fid, edid, src) in [
        (0x0000_0901u32, "TestAdd", "scn TestAdd\nint a\nint b\nBegin Function {a, b}\n  SetFunctionValue a + b\nEnd\n".to_string()),
        (
            0x0000_0902u32,
            "TestFact",
            "scn TestFact\nint n\nint r\nBegin Function {n}\n  if eval n <= 1\n    SetFunctionValue 1\n  else\n    let r := Call TestFact (n - 1)\n    SetFunctionValue n * r\n  endif\nEnd\n".to_string(),
        ),
        (0x0000_0903u32, "TestSet", "scn TestSet\nfloat n\nBegin Function {n}\n  let TestGlobal := n\nEnd\n".to_string()),
        (
            0x0000_0904u32,
            "TestMap",
            "scn TestMap\narray_var m\nBegin Function {m}\n  let TestGlobal := Sv_Length (m[\"eventName\"]) * 10 + Sv_Length (m[\"eventSender\"])\nEnd\n".to_string(),
        ),
        // A call on an empty reference behind a classic `&&` (EDEScript's shape, #1112).
        (0x0000_0905u32, "TestEmpty", "scn TestEmpty\nref r\nint n\nBegin Function {n}\n  if n == 1 && r.GetDisabled == 0\n  endif\nEnd\n".to_string()),
        // Functions that return a lambda (#1088): two in one unit, one in another.
        (0x0000_0906u32, "TestMaker", "scn TestMaker\nint k\nint w\nBegin Function {w}\n  if w\n    SetFunctionValue ({k} => k + 100)\n  else\n    SetFunctionValue ({k} => k * 2)\n  endif\nEnd\n".to_string()),
        (0x0000_0907u32, "TestMaker2", "scn TestMaker2\nint k\nBegin Function {}\n  SetFunctionValue ({k} => k - 1)\nEnd\n".to_string()),
    ] {
        // TestFact calls itself: its own name resolves through a seed reference list.
        let c = if edid != "TestAdd" {
            struct Me;
            impl fnv_script::compile::Env for Me {
                fn form(&self, e: &str) -> Option<fnv_script::compile::FormInfo> {
                    let (form, sig) = match e.to_ascii_lowercase().as_str() {
                        "testfact" => (0x0000_0902, *b"SCPT"),
                        "testglobal" => (0x0000_0800, *b"GLOB"),
                        _ => return None,
                    };
                    Some(fnv_script::compile::FormInfo { form, sig })
                }
                fn info(&self, _: u32) -> Option<fnv_script::compile::FormInfo> {
                    None
                }
                fn editor_id(&self, _: u32) -> Option<String> {
                    None
                }
                fn local_form(&self, _: u32) -> Option<u32> {
                    None
                }
                fn script_vars(&self, _: u32) -> Option<Vec<fnv_script::VarDecl>> {
                    None
                }
            }
            let c = fnv_script::compile::compile(&src, &table, &Me, None);
            assert!(c.errors().next().is_none(), "{:?}", c.diags);
            c
        } else {
            compiled(&src)
        };
        let vars: Vec<(u32, String, u8)> = c.variables.iter().map(|v| (v.index, v.name.clone(), u8::from(v.ty == fnv_script::VarType::Int))).collect();
        let vars: Vec<(u32, &str, u8)> = vars.iter().map(|(i, n, t)| (*i, n.as_str(), *t)).collect();
        let refs: Vec<u32> = c.refs.iter().filter_map(|r| if let fnv_script::unit::RefSlot::Form(f) = r { Some(*f) } else { None }).collect();
        let mut subs = unit(0, &c.bytes, &vars, &refs);
        // A local `ref` variable's slot is an SCRV (TestEmpty); the list keeps its order.
        if refs.len() < c.refs.len() {
            subs.retain(|s| &s.sig != b"SCRO");
            subs[0].data[4..8].copy_from_slice(&(c.refs.len() as u32).to_le_bytes());
            for r in &c.refs {
                subs.push(match r {
                    fnv_script::unit::RefSlot::Form(f) => sub(b"SCRO", f.to_le_bytes().to_vec()),
                    fnv_script::unit::RefSlot::Local(v) => sub(b"SCRV", v.to_le_bytes().to_vec()),
                    fnv_script::unit::RefSlot::Unmapped(f) => sub(b"SCRO", f.to_le_bytes().to_vec()),
                });
            }
        }
        subs.push(sub(b"SCTX", src.as_bytes().to_vec()));
        scripts.push(record(b"SCPT", 0, fid, &[vec![sub(b"EDID", z(edid))], subs].concat()));
    }
    let plugin = [tes4(), top(b"GLOB", &glob), top(b"SCPT", &scripts.concat())].concat();
    let plugins = vec![Plugin::parse(&plugin, "Udf.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = schema();
    let (table, j) = catalogue();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Fixture { data, forms, refs }
}

fn global_after(f: &Fixture, src: &str) -> f32 {
    let mut g = game(f);
    g.run_source(src, Default::default()).expect("compiles");
    let errors: Vec<String> = g.events.iter().filter_map(|e| match &e.kind {
        EventKind::ScriptError { error, .. } => Some(error.clone()),
        _ => None,
    }).collect();
    assert!(errors.is_empty(), "{errors:?}");
    g.world.global(FormId(0x0000_0800)).unwrap()
}

#[test]
fn call_runs_a_function_script_with_its_parameters() {
    let f = udf_fixture();
    assert_eq!(global_after(&f, "let TestGlobal := Call TestAdd 2, 3"), 5.0);
    // Recursion: each call has its own variables [documented: User Defined Function].
    assert_eq!(global_after(&f, "let TestGlobal := Call TestFact 5"), 120.0);
}

#[test]
fn a_scripts_call_on_an_empty_reference_is_traced_once() {
    // Classic `&&` evaluates both sides, so the call is made on every run, in the
    // original too [observed: o5_01, observer-checks.md check 51]; a script record traces
    // the failure once per function, not every frame (#1112). Console lines are traced
    // each time.
    let f = udf_fixture();
    let mut g = game(&f);
    g.cfg.strict = false;
    let failed = |g: &Game<'_>| g.events.iter().filter(|e| matches!(&e.kind, EventKind::CallFailed { error, .. } if error.contains("empty reference"))).count();
    for _ in 0..3 {
        g.run_source("let TestGlobal := Call TestEmpty 0", Default::default()).expect("compiles");
    }
    assert_eq!(failed(&g), 1);
    for _ in 0..2 {
        g.run_source("ref r\nr.Disable", Default::default()).expect("compiles");
    }
    assert_eq!(failed(&g), 3);
}

#[test]
fn a_call_on_an_empty_reference_gives_0_and_the_script_goes_on() {
    // [observed: o5_00, a function script went on to its next line; the Nellis artillery
    // (issue #1600) fires in the original although its script calls GetDead on an unset
    // reference every frame]. Until 2026-11-05 the call stopped the script.
    let f = udf_fixture();
    assert_eq!(global_after(&f, "ref r
r.Disable
let TestGlobal := 7"), 7.0);
    assert_eq!(global_after(&f, "ref r
if r.GetDisabled == 0
  let TestGlobal := 8
endif"), 8.0);
    // Behind a classic `&&` (EDEScript's shape), the line after it runs too.
    assert_eq!(global_after(&f, "let TestGlobal := Call TestEmpty 1
let TestGlobal := 9"), 9.0);
}

#[test]
fn deeper_than_thirty_calls_is_refused() {
    let f = udf_fixture();
    let mut g = game(&f);
    g.cfg.strict = false;
    g.run_source("let TestGlobal := Call TestFact 40", Default::default()).expect("compiles");
    let refused = g.events.iter().any(|e| matches!(&e.kind, EventKind::Skipped { what, .. } | EventKind::ScriptError { error: what, .. } if what.contains("nested")));
    assert!(refused, "a 31st nested call is refused [documented: User Defined Function]");
}

#[test]
fn lambdas_and_the_array_functions_that_take_them() {
    let f = udf_fixture();
    assert_eq!(global_after(&f, "int x\nlet TestGlobal := Call ({x} => x * 3) 4"), 12.0);
    let src = "array_var a\narray_var b\narray_var it\nlet a := Ar_List 1, 2, 3, 4\nlet b := Ar_Filter a, ({it} => *it > 2)\nlet TestGlobal := Ar_Size b";
    assert_eq!(global_after(&f, src), 2.0);
    let src = "array_var a\narray_var b\narray_var it\nlet a := Ar_List 1, 2, 3\nlet b := Ar_MapTo a, ({it} => *it * 10)\nlet TestGlobal := b[2]";
    assert_eq!(global_after(&f, src), 30.0);
}

/// A lambda is the same one each time its function makes it, whatever ran in between
/// and wherever its copy of the body was: one name, and the list of lambdas stays as
/// long as the lambdas written (#1088). Each lambda of a unit is its own.
#[test]
fn a_function_makes_the_same_lambda_every_call() {
    let f = udf_fixture();
    let mut g = game(&f);
    let src = "ref a\nref b\nref c\nref d\nint x\nint n\n\
        let a := Call TestMaker 0\nlet x := Call TestAdd 1 2\nlet c := Call TestMaker 1\nlet n := Call TestFact 4\n\
        let d := Call TestMaker2\nlet b := Call TestMaker 0\n\
        let TestGlobal := 0\n\
        if eval a == b && a != c && a != d && c != d\n  let TestGlobal := (Call a 5) + (Call c 5) * 1000 + (Call d 5) * 1000000\nendif";
    for round in 0..50 {
        g.run_source(src, Default::default()).expect("compiles");
        assert_eq!(g.world.global(FormId(0x0000_0800)).unwrap(), 4_105_010.0, "round {round}");
        assert_eq!(g.snapshot().lambdas.len(), 3, "round {round}: one per lambda written");
    }
    // After a save and a load the lambdas are made again under their saved names.
    let snap = g.snapshot();
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut r = Game::restore(&f.data, world, GameConfig::default(), &snap).unwrap();
    r.run_source(src, Default::default()).expect("compiles");
    assert_eq!(r.world.global(FormId(0x0000_0800)).unwrap(), 4_105_010.0);
    assert_eq!(r.snapshot().lambdas, snap.lambdas);
}

/// A saved lambda is made again under its saved name only while the unit's lambda of
/// that number is the same code: a save from before the numbering reads every lambda as
/// number 0, and an edited plugin can give a number other code (#1310 review).
#[test]
fn a_loaded_lambda_keeps_its_name_only_for_the_same_code() {
    let f = udf_fixture();
    let src = "ref a
ref b
ref c
ref d
int x
int n
        let a := Call TestMaker 0
let x := Call TestAdd 1 2
let c := Call TestMaker 1
let n := Call TestFact 4
        let d := Call TestMaker2
let b := Call TestMaker 0
        let TestGlobal := 0
        if eval a == b && a != c && a != d && c != d
  let TestGlobal := (Call a 5) + (Call c 5) * 1000 + (Call d 5) * 1000000
endif";
    let number = |l: &mut fnv_script::ir::XExpr, n: u32| {
        if let fnv_script::ir::XExpr::Lambda { index, .. } = l {
            *index = n;
        }
    };
    let load_and_run = |snap: &fnv_game::game::GameSnapshot| {
        let world = World::new(&f.forms, &f.refs, SimConfig::default());
        let mut r = Game::restore(&f.data, world, GameConfig::default(), snap).unwrap();
        r.run_source(src, Default::default()).expect("compiles");
        (r.world.global(FormId(0x0000_0800)).unwrap(), r.snapshot().lambdas.len())
    };

    // A save from before the numbering: TestMaker's second lambda (k * 2) reads as
    // number 0, which is k + 100 now.
    let mut g = game(&f);
    g.run_source("ref a
let a := Call TestMaker 0", Default::default()).expect("compiles");
    let mut old = g.snapshot();
    assert_eq!(old.lambdas.len(), 1);
    number(&mut old.lambdas[0].lambda, 0);
    assert_eq!(load_and_run(&old), (4_105_010.0, 4), "k + 100 runs its own code under a new name");

    // A plugin edited since the save: lambda 0 of TestMaker was k * 2 then.
    let mut g = game(&f);
    g.run_source(src, Default::default()).expect("compiles");
    let mut edited = g.snapshot();
    // Only TestMaker has a lambda 1.
    let j = edited.lambdas.iter().position(|l| matches!(l.lambda, fnv_script::ir::XExpr::Lambda { index: 1, .. })).unwrap();
    let i = edited.lambdas.iter().position(|l| matches!(l.lambda, fnv_script::ir::XExpr::Lambda { index: 0, .. }) && l.unit == edited.lambdas[j].unit).unwrap();
    edited.lambdas[i].lambda = edited.lambdas[j].lambda.clone();
    number(&mut edited.lambdas[i].lambda, 0);
    assert_eq!(load_and_run(&edited), (4_105_010.0, 4), "the current k + 100 runs, not the saved code");

    // The same code keeps its name (a_function_makes_the_same_lambda_every_call).
    assert_eq!(load_and_run(&g.snapshot()), (4_105_010.0, 3));
}

#[test]
fn the_extenders_strings_arrays_and_lambdas_are_in_the_save() {
    // Task F1: a snapshot keeps the ExtStore and the lambdas; a restore gives them back.
    let f = udf_fixture();
    let mut g = game(&f);
    g.run_source("int x\nlet TestGlobal := Call ({x} => x * 3) 4", Default::default()).expect("compiles");
    let id = g.ext.new_string("kept".into());
    let snap = g.snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(back, snap);
    assert_eq!(snap.lambdas.len(), 1);
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let r = Game::restore(&f.data, world, GameConfig::default(), &back).unwrap();
    assert_eq!(r.ext.text(id), Some("kept"));
    assert_eq!(r.snapshot().lambdas, snap.lambdas);
}

#[test]
fn event_handlers_are_set_dispatched_filtered_and_removed() {
    // Tasks E1–E3 [documented: SetEventHandler; SetEventHandlerAlt; RemoveEventHandler;
    // DispatchEvent; DispatchEventAlt].
    let f = udf_fixture();
    // The handler names are scripts; the console line resolves them by Editor ID.
    let mut g = game(&f);
    let run = |g: &mut Game<'_>, src: &str| g.run_source(src, Default::default()).expect("compiles");
    run(&mut g, "SetEventHandler \"NVFTestEvent\" TestSet\nDispatchEventAlt \"NVFTestEvent\" 7");
    assert_eq!(g.world.global(FormId(0x0000_0800)).unwrap(), 7.0);
    // A filter on the first argument: only 5 passes.
    run(&mut g, "RemoveEventHandler \"NVFTestEvent\" TestSet\nSetEventHandlerAlt \"NVFAlt\" TestSet 1::5\nDispatchEventAlt \"NVFAlt\" 4\nDispatchEventAlt \"NVFTestEvent\" 9");
    assert_eq!(g.world.global(FormId(0x0000_0800)).unwrap(), 7.0, "4 is filtered out and the removed handler does not run");
    run(&mut g, "DispatchEventAlt \"NVFAlt\" 5");
    assert_eq!(g.world.global(FormId(0x0000_0800)).unwrap(), 5.0);
    // DispatchEvent: one string map with eventName and eventSender (the console's
    // "plugin" is `console`).
    run(&mut g, "SetEventHandler \"Abc\" TestMap\nDispatchEvent \"Abc\"");    assert_eq!(g.world.global(FormId(0x0000_0800)).unwrap(), 37.0);
    // Priority 0 is refused [documented: SetEventHandler].
    run(&mut g, "let TestGlobal := SetEventHandler \"X\" TestSet \"priority\"::0");
    assert_eq!(g.world.global(FormId(0x0000_0800)).unwrap(), 0.0);
}

#[test]
fn is_event_handler_first_and_last_match_execution_priority_order() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| {
        g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default()).unwrap()
    };
    q(
        &mut g,
        "SetEventHandler \"W6Ev\" TestSet \"priority\"::10
         SetEventHandler \"W6Ev\" TestMap \"priority\"::5
         SetEventHandler \"W6Ev\" TestAdd \"priority\"::5
         SetEventHandler \"W6Ev\" TestSet \"priority\"::1",
    );
    // Highest priority (10) is first (1) and not last (0).
    assert_eq!(q(&mut g, "SetFunctionValue (IsEventHandlerFirst \"W6Ev\" TestSet 10)"), XVal::Num(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (IsEventHandlerLast \"W6Ev\" TestSet 10)"), XVal::Num(0.0));
    // Tied middle priority (5) is neither first nor last.
    assert_eq!(q(&mut g, "SetFunctionValue (IsEventHandlerFirst \"W6Ev\" TestMap 5)"), XVal::Num(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (IsEventHandlerLast \"W6Ev\" TestMap 5)"), XVal::Num(0.0));
    // Lowest priority (1) is not first (0) and is last (1).
    assert_eq!(q(&mut g, "SetFunctionValue (IsEventHandlerFirst \"W6Ev\" TestSet 1)"), XVal::Num(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (IsEventHandlerLast \"W6Ev\" TestSet 1)"), XVal::Num(1.0));
    // Unregistered priority (99) is 0 for both.
    assert_eq!(q(&mut g, "SetFunctionValue (IsEventHandlerFirst \"W6Ev\" TestSet 99)"), XVal::Num(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (IsEventHandlerLast \"W6Ev\" TestSet 99)"), XVal::Num(0.0));
}

#[test]
fn on_disable_and_on_enable_fire_pre_update_with_null_self() {
    use fnv_script::vm::XVal;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let q = |g: &mut fnv_game::Game, body: &str| {
        g.run_query(
            &format!("int afterDis\nint afterEn\nBegin Function {{ }}\n{body}\nEnd\n"),
            Default::default(),
        )
        .unwrap()
    };
    let res = q(
        &mut g,
        "set TestGlobal to 0
         SetEventHandler \"OnDisable\" (Begin Function {ref r, int z}
           if eval (GetSelfAlt == 0) && (r == TestActivatorREF) && (z == 0) && (r.GetDisabled == 0)
             let TestGlobal := TestGlobal + 1
           endif
         End)
         SetEventHandler \"OnEnable\" (Begin Function {ref r, int z}
           if eval (GetSelfAlt == 0) && (r == TestActivatorREF) && (z == 0) && (r.GetDisabled == 1)
             let TestGlobal := TestGlobal + 10
           endif
         End)
         TestActivatorREF.Disable 0
         let afterDis := TestActivatorREF.GetDisabled
         TestActivatorREF.Enable 0
         let afterEn := TestActivatorREF.GetDisabled
         SetFunctionValue (afterDis * 10 + afterEn)",
    );
    assert!(
        !g.events.iter().any(|e| matches!(e.kind, fnv_game::trace::EventKind::ScriptError { .. })),
        "unexpected script error: {:?}",
        g.events
    );
    assert_eq!(res, XVal::Num(10.0));
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 11.0);
}

#[test]
fn on_disable_and_on_enable_reentrancy_does_not_recurse_or_duplicate_events() {
    use fnv_game::trace::EventKind;
    use fnv_script::vm::XVal;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let res = g
        .run_query(
            "int afterDis
             int afterEn
             Begin Function { }
               set TestGlobal to 0
               SetEventHandler \"OnDisable\" (Begin Function {ref r, int z}
                 let TestGlobal := TestGlobal + 1
                 r.Disable 0
               End)
               SetEventHandler \"OnEnable\" (Begin Function {ref r, int z}
                 let TestGlobal := TestGlobal + 10
                 r.Enable 0
               End)
               TestActivatorREF.Disable 0
               let afterDis := TestActivatorREF.GetDisabled
               TestActivatorREF.Enable 0
               let afterEn := TestActivatorREF.GetDisabled
               SetFunctionValue (afterDis * 10 + afterEn)
             End",
            Default::default(),
        )
        .unwrap();
    assert!(
        !g.events.iter().any(|e| matches!(e.kind, EventKind::ScriptError { .. })),
        "unexpected script error on reentrant Disable/Enable: {:?}",
        g.events
    );
    assert_eq!(res, XVal::Num(10.0));
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 11.0);
    let dis_count = g.events.iter().filter(|e| matches!(e.kind, EventKind::RefDisabled { .. })).count();
    let en_count = g.events.iter().filter(|e| matches!(e.kind, EventKind::RefEnabled { .. })).count();
    assert_eq!(dis_count, 1, "expected exactly 1 RefDisabled event");
    assert_eq!(en_count, 1, "expected exactly 1 RefEnabled event");
}

#[test]
fn get_explosion_hit_damage_returns_minus_one_outside_handler() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| {
        g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default()).unwrap()
    };
    assert_eq!(q(&mut g, "SetFunctionValue (GetExplosionHitDamage)"), XVal::Num(-1.0));
    // SetExplosionHitDamage outside a handler is a no-op and does not change the -1.0 sentinel.
    q(&mut g, "SetExplosionHitDamage 42");
    assert_eq!(q(&mut g, "SetFunctionValue (GetExplosionHitDamage)"), XVal::Num(-1.0));
    // Inside a ShowOff:OnExplosionHit handler, GetExplosionHitDamage reads the live pre-armor
    // damage (100.0) and SetExplosionHitDamage 30 returns 30.0; after exit it reverts to -1.0.
    q(
        &mut g,
        "set TestGlobal to 0
         SetEventHandler \"ShowOff:OnExplosionHit\" (Begin Function {ref target, ref source}
           let TestGlobal := GetExplosionHitDamage
           SetExplosionHitDamage 30
         End)",
    );
    let out = g.explosion_hit(FormId::PLAYER_REF, 100.0);
    assert_eq!(g.world.global(FormId(0x0000_0800)).unwrap(), 100.0);
    assert_eq!(out, 30.0);
    assert_eq!(q(&mut g, "SetFunctionValue (GetExplosionHitDamage)"), XVal::Num(-1.0));
}

#[test]
fn on_actor_equip_and_unequip_aliases_self_and_pre_unequip() {
    use fnv_script::vm::XVal;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let q = |g: &mut fnv_game::Game, body: &str| {
        g.run_query(
            &format!("ref cbEq\nref cbUn\nint g1\nint g2\nint r1\nint r2\nint ok\nBegin Function {{ }}\n{body}\nEnd\n"),
            Default::default(),
        )
        .unwrap()
    };
    let res = q(
        &mut g,
        "set TestGlobal to 0
         TestNpcREF.AddItem TestWeapon 1 1
         let cbEq := (Begin Function {ref a, ref it}
           let TestGlobal := TestGlobal + 1000
           if eval (GetSelfAlt == 0)
             let TestGlobal := TestGlobal + 1
           endif
           if eval (GetCurrentEventName == \"onactorequip\")
             let TestGlobal := TestGlobal + 2
           endif
         End)
         let cbUn := (Begin Function {ref a, ref it}
           let TestGlobal := TestGlobal + 2000
           if eval (GetSelfAlt == 0)
             let TestGlobal := TestGlobal + 10
           endif
           if eval (a.GetEquipped it)
             let TestGlobal := TestGlobal + 100
           endif
           if eval (GetCurrentEventName == \"onactorunequip\")
             let TestGlobal := TestGlobal + 200
           endif
         End)
         SetEventHandler \"OnActorEquip\" cbEq
         SetEventHandler \"OnActorUnequip\" cbUn
         let g1 := Ar_Size (GetEventHandlers \"OnEquip\")
         let g2 := Ar_Size (GetEventHandlers \"OnUnequip\")
         TestNpcREF.EquipItem TestWeapon 0 1
         let ok := (TestNpcREF.GetEquipped TestWeapon)
         TestNpcREF.UnequipItem TestWeapon 0 1
         let r1 := RemoveEventHandler \"OnEquip\" cbEq
         let r2 := RemoveEventHandler \"OnUnequip\" cbUn
         SetFunctionValue (ok * 10000 + (g1 == 1) * 1000 + (g2 == 1) * 100 + (r1 == 1) * 10 + (r2 == 1))",
    );
    assert!(
        !g.events.iter().any(|e| matches!(e.kind, fnv_game::trace::EventKind::ScriptError { .. })),
        "unexpected script error: {:?}",
        g.events
    );
    assert_eq!(res, XVal::Num(11111.0));
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 3313.0);
}

#[test]
fn remove_event_handler_respects_priority_and_zero_wildcard() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| {
        g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default()).unwrap()
    };
    q(
        &mut g,
        "SetEventHandler \"W6Rem\" TestSet \"priority\"::10
         SetEventHandler \"W6Rem\" TestSet \"priority\"::5
         SetEventHandler \"W6Rem\" TestSet \"priority\"::1",
    );
    // Removing priority 5 removes only priority 5; repeat returns 0.
    assert_eq!(q(&mut g, "SetFunctionValue (RemoveEventHandler \"W6Rem\" TestSet \"priority\"::5)"), XVal::Num(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (RemoveEventHandler \"W6Rem\" TestSet \"priority\"::5)"), XVal::Num(0.0));
    assert_eq!(q(&mut g, "array_var a\nlet a := GetEventHandlers \"W6Rem\"\nSetFunctionValue ((Ar_HasKey a 10) * 100 + (Ar_HasKey a 5) * 10 + (Ar_HasKey a 1))"), XVal::Num(101.0));
    // Priority 10 and 1 remain (2 priority buckets); priority 0 removes both.
    assert_eq!(q(&mut g, "SetFunctionValue (Ar_Size (GetEventHandlers \"W6Rem\"))"), XVal::Num(2.0));
    assert_eq!(q(&mut g, "SetFunctionValue (RemoveEventHandler \"W6Rem\" TestSet \"priority\"::0)"), XVal::Num(1.0));
    assert_eq!(q(&mut g, "DispatchEventAlt \"W6Rem\" 77\nSetFunctionValue TestGlobal"), XVal::Num(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (RemoveEventHandler \"W6Rem\" TestSet)"), XVal::Num(0.0));
}

#[test]
fn jip_quest_stage_handler_exact_stage_zero_and_duplicate_return() {
    use fnv_script::vm::XVal;
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let q = |g: &mut fnv_game::Game, body: &str| {
        g.run_query(
            &format!("ref cb10\nref cb0\nint r1\nint r2\nint r3\nint r4\nBegin Function {{ }}\n{body}\nEnd\n"),
            Default::default(),
        )
        .unwrap()
    };
    let res = q(
        &mut g,
        "set TestGlobal to 0
         let cb10 := (Begin Function {ref qst, int stg}
           if eval (GetSelfAlt == 0) && (qst == Q) && (stg == 10) && (GetStage Q == 10)
             let TestGlobal := TestGlobal + 1
           endif
         End)
         let cb0 := (Begin Function {ref qst, int stg}
           let TestGlobal := TestGlobal + 100
         End)
         let r1 := SetOnQuestStageEventHandler cb10 1 Q 10
         let r2 := SetOnQuestStageEventHandler cb10 1 Q 10
         SetOnQuestStageEventHandler cb0 1 Q 0
         SetStage Q 10
         let r3 := SetOnQuestStageEventHandler cb10 0 Q 10
         let r4 := SetOnQuestStageEventHandler cb10 0 Q 10
         SetFunctionValue (r1 * 1000 + r2 * 100 + r3 * 10 + r4)",
    );
    assert!(
        !g.events.iter().any(|e| matches!(e.kind, fnv_game::trace::EventKind::ScriptError { .. })),
        "unexpected script error: {:?}",
        g.events
    );
    // ret=1,0,1,0 (add=1, dup=0, rem=1, miss=0)
    assert_eq!(res, XVal::Num(1010.0));
    // cb10 fired once with GetSelfAlt==0 & POST-update GetStage==10 (+1); cb0 (stage 0) did NOT fire on stage 10.
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 1.0);
}

#[test]
fn dispatch_event_returns_one_and_overwrites_info_map_keys() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| {
        g.run_query(
            &format!("array_var m\nint r1\nint r2\nint ok\nBegin Function {{ }}\n{body}\nEnd\n"),
            Default::default(),
        )
        .unwrap()
    };
    let res = q(
        &mut g,
        "SetEventHandler \"W6DEv\" (Begin Function {array_var a}
           let a[\"mut\"] := 1
         End)
         let r1 := DispatchEvent \"W6DEv\"
         let m := Ar_Construct \"stringmap\"
         let m[\"eventName\"] := \"Wrong\"
         let m[\"eventSender\"] := \"OldSnd\"
         let r2 := DispatchEvent \"W6DEv\" m \"NewSnd\"
         let ok := r1 * 1000 + r2 * 100
         if eval (m[\"mut\"] == 1)
           let ok := ok + 10
         endif
         if eval (m[\"eventName\"] == \"W6DEv\") && (m[\"eventSender\"] == \"NewSnd\")
           let ok := ok + 1
         endif
         SetFunctionValue ok",
    );
    assert_eq!(res, XVal::Num(1111.0));
    // Case-variant caller keys ("EventName", "eventsender") must overwrite in place rather than
    // pushing duplicate entries into the stringmap.
    let res_case = q(
        &mut g,
        "let m := Ar_Construct \"stringmap\"
         let m[\"EventName\"] := \"Wrong\"
         let m[\"eventsender\"] := \"OldSnd\"
         let r1 := DispatchEvent \"W6DEv\" m \"CaseSnd\"
         let ok := r1 * 1000
         if eval (Ar_Size m == 3)
           let ok := ok + 100
         endif
         if eval (m[\"eventName\"] == \"W6DEv\") && (m[\"eventSender\"] == \"CaseSnd\")
           let ok := ok + 10
         endif
         if eval (m[\"EventName\"] == \"W6DEv\") && (m[\"eventsender\"] == \"CaseSnd\")
           let ok := ok + 1
         endif
         SetFunctionValue ok",
    );
    assert_eq!(res_case, XVal::Num(1111.0));
}

#[test]
fn set_event_handler_and_alt_deduplicate_identical_registrations() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| {
        g.run_query(&format!("ref cb\nint r1\nint r2\nint r3\nBegin Function {{ }}\n{body}\nEnd\n"), Default::default()).unwrap()
    };
    // SetEventHandler: duplicate returns 0 and dispatches once.
    assert_eq!(
        q(
            &mut g,
            "let cb := (Begin Function {int x}
               let TestGlobal := TestGlobal + x
             End)
             let r1 := SetEventHandler \"W6Dup\" cb
             let r2 := SetEventHandler \"W6Dup\" cb
             DispatchEventAlt \"W6Dup\" 1
             SetFunctionValue (r1 * 10 + r2)",
        ),
        XVal::Num(10.0)
    );
    assert_eq!(g.world.global(FormId(0x0000_0800)).unwrap(), 1.0);
    // SetEventHandlerAlt: duplicate with identical filter returns 0 and dispatches once;
    // distinct priority is accepted.
    assert_eq!(
        q(
            &mut g,
            "let cb := (Begin Function {int x}
               let TestGlobal := TestGlobal + x
             End)
             let r1 := SetEventHandlerAlt \"W6DupAlt\" cb 1::5
             let r2 := SetEventHandlerAlt \"W6DupAlt\" cb 1::5
             DispatchEventAlt \"W6DupAlt\" 5
             let r3 := SetEventHandlerAlt \"W6DupAlt\" cb 1::5 \"priority\"::2
             SetFunctionValue (r1 * 100 + r2 * 10 + r3)",
        ),
        XVal::Num(101.0)
    );
    assert_eq!(g.world.global(FormId(0x0000_0800)).unwrap(), 6.0);
    // Multiset filter equality: repeated filter pairs (1::5, 1::5) must not match distinct
    // pairs of equal length (1::5, 2::6), while reordered identical pairs (2::6, 1::5) deduplicate.
    assert_eq!(
        q(
            &mut g,
            "let cb := (Begin Function {int x, int y}
               let TestGlobal := TestGlobal + x + y
             End)
             let r1 := SetEventHandlerAlt \"W6DupMulti\" cb 1::5 1::5
             let r2 := SetEventHandlerAlt \"W6DupMulti\" cb 1::5 2::6
             let r3 := SetEventHandlerAlt \"W6DupMulti\" cb 2::6 1::5
             SetFunctionValue (r1 * 100 + r2 * 10 + r3)",
        ),
        XVal::Num(110.0)
    );
}

#[test]
fn get_game_loaded_and_restarted_are_true_once_per_script() {
    // [documented: GetGameLoaded; GetGameRestarted] once per script after a load / per
    // session; a session's SetEventHandler registrations carry over a load.
    let f = udf_fixture();
    let mut g = game(&f);
    g.run_source("SetEventHandler \"NVFTestEvent\" TestSet", Default::default()).unwrap();
    let session = g.take_session();
    let mut g2 = game(&f);
    g2.put_session(session);
    g2.run_source("DispatchEventAlt \"NVFTestEvent\" 3", Default::default()).unwrap();
    assert_eq!(g2.world.global(FormId(0x0000_0800)).unwrap(), 3.0, "the handler lasted for the session");
}

#[test]
fn nx_variables_are_kept_per_reference_and_saved() {
    // #1017 [documented: Category:Functions (NX)]: keys without regard to case, one set
    // per kind, prefix maps and clears, kept in the save.
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("array_var aA
string_var sS
Begin Function {{ }}
{body}
End
"), Default::default());
    q(&mut g, "PlayerREF.NX_SetEVFl \"MyMod:Mood\" 67
PlayerREF.NX_SetEVFl \"MyMod:Age\" 30
PlayerREF.NX_SetEVFl \"Other\" 5
PlayerREF.NX_SetEVSt \"MyMod:Mood\" \"glad\"").unwrap();
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.NX_GetEVFl \"mymod:MOOD\")"), Ok(XVal::Num(67.0)));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.NX_GetEVFl \"MyMod:None\")"), Ok(XVal::Num(0.0)));
    assert_eq!(q(&mut g, "let sS := PlayerREF.NX_GetEVSt \"MyMod:Mood\"
SetFunctionValue sS"), Ok(XVal::Str("glad".into())));
    assert_eq!(q(&mut g, "let aA := PlayerREF.NX_GetEVFlAr \"mymod:\"
SetFunctionValue (Ar_Size aA) * 100 + aA[\"MyMod:Age\"]"), Ok(XVal::Num(230.0)));
    // A save keeps them.
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&serde_json::to_string(&g.snapshot()).unwrap()).unwrap();
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let mut h = fnv_game::Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert_eq!(q(&mut h, "SetFunctionValue (PlayerREF.NX_GetEVFl \"MyMod:Age\")"), Ok(XVal::Num(30.0)));
    // Match 2 clears every key with the prefix; the string of the same key stays.
    q(&mut h, "PlayerREF.NX_ClrEVFl \"MYMOD\" 2").unwrap();
    assert_eq!(q(&mut h, "SetFunctionValue (PlayerREF.NX_GetEVFl \"MyMod:Mood\") + (PlayerREF.NX_GetEVFl \"Other\")"), Ok(XVal::Num(5.0)));
    assert_eq!(q(&mut h, "let sS := PlayerREF.NX_GetEVSt \"MyMod:Mood\"
SetFunctionValue sS"), Ok(XVal::Str("glad".into())));
    for _ in 0..20 {
        let Ok(XVal::Num(r)) = q(&mut h, "SetFunctionValue (NX_GetRandom 1 3)") else { panic!() };
        assert!((1.0..=3.0).contains(&r) && r.fract() == 0.0, "{r}");
    }
    assert_eq!(q(&mut h, "SetFunctionValue (PlayerREF.NX_GetConversationPartner)"), Ok(XVal::Ref(0)));
}

/// NX form variables in a save loaded without a plugin (#1017 review): one naming a gone
/// plugin's form is dropped and the save still reads; the others stay. A created
/// reference deleted before the save keeps no variables.
#[test]
fn nx_variables_survive_a_gone_plugin_and_leave_with_deleted_references() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("Begin Function {{ }}
{body}
End
"), Default::default());
    q(&mut g, "PlayerREF.NX_SetEVFo \"MyMod:Pet\" PlayerREF
PlayerREF.NX_SetEVFo \"Mine\" PlayerREF
PlayerREF.NX_SetEVFl \"Count\" 2").unwrap();
    let mut v = serde_json::to_value(g.snapshot()).unwrap();
    let pet = &mut v["nx"]["0x00000014"]["fo"]["mymod:pet"];
    assert_eq!(pet["key"], json!("MyMod:Pet"), "{pet}");
    pet["value"] = json!("0x01000ABC");
    // A created reference deleted before the save, with a variable.
    v["sim"]["deleted_created"]["0xFF000099"] = json!("0x00000014");
    v["nx"]["0xFF000099"] = json!({"fl": {"n": {"key": "N", "value": 1.0}}});
    let e = |n: &str| fnv_save::PluginEntry { name: n.into(), size: 0 };
    let remap = fnv_save::PluginRemap::new(&[e("Test.esm"), e("Extra.esm")], &[e("Test.esm")]);
    fnv_save::remap_json(&mut v, &remap);
    let mut back: fnv_game::game::GameSnapshot = serde_json::from_value(v).unwrap();
    assert_eq!(back.drop_gone(|id| fnv_save::is_gone(id.0)), 1, "the form variable of Extra.esm");
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let mut h = fnv_game::Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert_eq!(q(&mut h, "SetFunctionValue (PlayerREF.NX_GetEVFo \"MyMod:Pet\")"), Ok(XVal::Ref(0)));
    assert_eq!(q(&mut h, "SetFunctionValue (PlayerREF.NX_GetEVFo \"mine\")"), Ok(XVal::Ref(FormId::PLAYER_REF.0)));
    assert_eq!(q(&mut h, "SetFunctionValue (PlayerREF.NX_GetEVFl \"Count\")"), Ok(XVal::Num(2.0)));
    let again = serde_json::to_value(h.snapshot()).unwrap();
    assert!(again["nx"].get("0xFF000099").is_none(), "{}", again["nx"]);
}

#[test]
fn menu_getters_read_what_the_engine_reports() {
    // #983: GetMenuTargetRef and GetSelectedItemRef answer the picks the engine reports
    // for an open menu (Game::set_menu_picks), an item as an inventory reference.
    use fnv_game::game::{menus, MenuPick, MenuPicks, TEMP_REF_BASE};
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("Begin Function {{ }}
{body}
End
"), Default::default());
    let chest = FormId(0x0000_0A00);
    let item = FormId(0x0000_0A01);
    g.set_menu_picks(1008, MenuPicks { target: Some(MenuPick::Form(chest)), selected: Some(MenuPick::Item { owner: chest, item }) });
    // Closed: nothing.
    assert_eq!(q(&mut g, "SetFunctionValue (GetMenuTargetRef 1008)"), Ok(XVal::Ref(0)));
    assert_eq!(q(&mut g, "SetFunctionValue (GetSelectedItemRef)"), Ok(XVal::Ref(0)));
    g.menu = Some(1008);
    assert_eq!(q(&mut g, "SetFunctionValue (GetMenuTargetRef 1008)"), Ok(XVal::Ref(chest.0)));
    assert_eq!(q(&mut g, "SetFunctionValue (GetMenuTargetRef 1053)"), Ok(XVal::Ref(0)));
    match q(&mut g, "SetFunctionValue (GetSelectedItemRef)") {
        Ok(XVal::Ref(r)) => assert!(r >= TEMP_REF_BASE, "an inventory reference, not {r:08X}"),
        other => panic!("{other:?}"),
    }
    // The menu on top answers GetSelectedItemRef.
    g.set_menu_picks(menus::ITEM_MOD, MenuPicks { target: None, selected: Some(MenuPick::Form(item)) });
    g.menu_under = g.menu;
    g.menu = Some(menus::ITEM_MOD);
    assert_eq!(q(&mut g, "SetFunctionValue (GetSelectedItemRef)"), Ok(XVal::Ref(item.0)));
}

#[test]
fn refresh_items_list_sets_one_shot_transient_request() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| {
        g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default())
    };
    assert!(!g.refresh_items_list_pending());
    assert!(!g.take_refresh_items_list());

    // Calling RefreshItemsList returns 0 and latches a pending request; repeated calls coalesce.
    assert_eq!(q(&mut g, "SetFunctionValue (RefreshItemsList)"), Ok(XVal::Num(0.0)));
    assert!(g.refresh_items_list_pending());
    assert_eq!(q(&mut g, "RefreshItemsList"), Ok(XVal::Num(0.0)));
    assert!(g.refresh_items_list_pending());

    // The request is transient (not carried across snapshot/restore).
    let snap = g.snapshot();
    let restored = Game::restore(&f.data, World::restore(&f.forms, &f.refs, &snap.sim).unwrap(), g.cfg, &snap).unwrap();
    assert!(!restored.refresh_items_list_pending());

    // Engine consumption is one-shot.
    assert!(g.take_refresh_items_list());
    assert!(!g.refresh_items_list_pending());
    assert!(!g.take_refresh_items_list());
}

#[test]
fn set_delay_elapsed_returns_zero_and_updates_remaining_delay() {
    // [observed 2026-11-24: w6t2_31_xnvse_items_ammo_rounds_and_ingrid -> del=0.00]
    use fnv_script::vm::XVal;
    let q_data = [vec![1u8, 0, 0, 0], 5.0f32.to_le_bytes().to_vec()].concat();
    let quest = record(b"QUST", 0, 0x0000_0820, &[sub(b"EDID", z("TestDelayQuest")), sub(b"DATA", q_data)]);
    let bytes = [tes4(), top(b"QUST", &quest)].concat();
    let plugins = vec![Plugin::parse(&bytes, "DelayQuest.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = schema();
    let (table, j) = catalogue();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    let f = Fixture { data, forms, refs };
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| {
        g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default()).unwrap()
    };

    // SetDelayElapsed returns 0.0 (not secs) and adjusts remaining delay.
    assert_eq!(q(&mut g, "SetFunctionValue (SetDelayElapsed TestDelayQuest 2.5)"), XVal::Num(0.0), "SetDelayElapsed returns 0.0");
    assert_eq!(q(&mut g, "SetFunctionValue (GetDelayElapsed TestDelayQuest)"), XVal::Num(2.5), "remaining delay updated to 2.5s");
    assert_eq!(q(&mut g, "SetFunctionValue (SetDelayElapsed TestDelayQuest 0)"), XVal::Num(0.0), "SetDelayElapsed 0 returns 0.0");
    assert_eq!(q(&mut g, "SetFunctionValue (GetDelayElapsed TestDelayQuest)"), XVal::Num(0.0), "remaining delay updated to 0.0s");
    // Missing/null form returns 0.0 safely.
    assert_eq!(q(&mut g, "SetFunctionValue (SetDelayElapsed 0 2.5)"), XVal::Num(0.0), "null quest returns 0.0");
}

#[test]
fn get_debug_mode_state_reports_combat_ai_default_and_toggles() {
    // [observed 2026-11-24: w6t2_24_jip_fall_time_and_is_in_air_edges -> dbg(0,1,1,0,0,0,tgm=1)]
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| {
        g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default()).unwrap()
    };
    assert_eq!(q(&mut g, "SetFunctionValue (GetDebugModeState 0)"), XVal::Num(0.0), "god mode off by default");
    assert_eq!(q(&mut g, "SetFunctionValue (GetDebugModeState 1)"), XVal::Num(1.0), "AI enabled by default");
    assert_eq!(q(&mut g, "SetFunctionValue (GetDebugModeState 2)"), XVal::Num(1.0), "Combat AI enabled by default");
    assert_eq!(q(&mut g, "SetFunctionValue (GetDebugModeState 3)"), XVal::Num(0.0), "collision debug off by default");
    assert_eq!(q(&mut g, "SetFunctionValue (GetDebugModeState 4)"), XVal::Num(0.0), "borders off by default");
    assert_eq!(q(&mut g, "SetFunctionValue (GetDebugModeState 5)"), XVal::Num(0.0), "out-of-range mode reads 0");

    g.toggles.god_mode = true;
    assert_eq!(q(&mut g, "SetFunctionValue (GetDebugModeState 0)"), XVal::Num(1.0), "tgm sets mode 0 to 1");
    g.toggles.god_mode = false;
    assert_eq!(q(&mut g, "SetFunctionValue (GetDebugModeState 0)"), XVal::Num(0.0));

    g.toggles.actors_ai_off = true;
    assert_eq!(q(&mut g, "SetFunctionValue (GetDebugModeState 1)"), XVal::Num(0.0), "tai sets mode 1 to 0");
    assert_eq!(q(&mut g, "SetFunctionValue (GetDebugModeState 2)"), XVal::Num(1.0), "mode 2 stays 1 independently");
}

#[test]
fn get_event_handlers_showoff_known_and_filter_keys() {
    // [observed 2026-11-24: w6t2_16_xnvse_get_event_handlers_filter_keys ->
    // empty(cross=-1,soAdd=0)|std(first,second)|alt(0,1)]
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| {
        g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default()).unwrap()
    };

    // Known ShowOff:* events return an empty Map (size 0), while OnCrosshairOn and unknown
    // event names return ar_Null (size -1).
    assert_eq!(q(&mut g, "SetFunctionValue (ar_Size (GetEventHandlers \"ShowOff:OnAdd\"))"), XVal::Num(0.0), "ShowOff:OnAdd empty Map");
    assert_eq!(q(&mut g, "SetFunctionValue (ar_Size (GetEventHandlers \"ShowOff:OnFireWeapon\"))"), XVal::Num(0.0), "ShowOff:OnFireWeapon empty Map");
    assert_eq!(q(&mut g, "SetFunctionValue (ar_Size (GetEventHandlers \"OnCrosshairOn\"))"), XVal::Num(-1.0), "OnCrosshairOn returns ar_Null when unregistered");
    assert_eq!(q(&mut g, "SetFunctionValue (ar_Size (GetEventHandlers \"UnknownUserEvent\"))"), XVal::Num(-1.0), "unknown event returns ar_Null");
    assert_eq!(q(&mut g, "SetFunctionValue (ar_Size (GetEventHandlers \"\"))"), XVal::Num(0.0), "unfiltered GetEventHandlers remains empty");

    // Standard SetEventHandler stores filter StringMap keys as "first"/"second".
    assert_eq!(
        q(
            &mut g,
            "SetEventHandler \"OnActivate\" TestAdd \"first\"::PlayerREF \"second\"::TestGlobal\n\
             array_var aAct = GetEventHandlers \"OnActivate\" TestAdd 1\n\
             array_var aF = aAct[0][1]\n\
             array_var aK = ar_Keys aF\n\
             string_var sOut = sv_Construct \"%z,%z\" (aK[0]) (aK[1])\n\
             RemoveEventHandler \"OnActivate\" TestAdd\n\
             SetFunctionValue sOut",
        ),
        XVal::Str("first,second".into()),
        "SetEventHandler filter keys are first,second"
    );

    // SetEventHandlerAlt stores filter StringMap keys as numeric index strings "0"/"1".
    assert_eq!(
        q(
            &mut g,
            "SetEventHandlerAlt \"OnActivate\" TestAdd 0::PlayerREF 1::TestGlobal\n\
             array_var aAlt = GetEventHandlers \"OnActivate\" TestAdd 1\n\
             array_var aF = aAlt[0][1]\n\
             array_var aK = ar_Keys aF\n\
             string_var sOut = sv_Construct \"%z,%z\" (aK[0]) (aK[1])\n\
             RemoveEventHandler \"OnActivate\" TestAdd\n\
             SetFunctionValue sOut",
        ),
        XVal::Str("0,1".into()),
        "SetEventHandlerAlt filter keys are 0,1"
    );
}

#[test]
fn queries_run_function_scripts_and_keep_what_they_change() {
    // The runner's `query` action (Game::run_query): JIP's auxiliary variables set by one
    // query are read by the next, and the saved ones are in the snapshot.
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let ctx = || fnv_game::game::RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() };
    let set = "Begin Function { }\nAuxVarSetFlt \"nvfQ\" 2.5\nAuxVarSetFlt \"*nvfT\" 1\nSetFunctionValue 7\nEnd\n";
    assert_eq!(g.run_query(set, ctx()), Ok(XVal::Num(7.0)));
    let get = "float x\nBegin Function { }\nlet x := AuxVarGetFlt \"NVFQ\"\nSetFunctionValue x * 2\nEnd\n";
    assert_eq!(g.run_query(get, ctx()), Ok(XVal::Num(5.0)));
    assert_eq!(g.run_query("Begin Function { }\nSetFunctionValue Sv_Construct \"a%gb\", 1\nEnd\n", ctx()), Ok(XVal::Str("a1b".into())));
    assert_eq!(g.run_query("Begin Function { }\nlet x := 1\nEnd\n", ctx()), Err("script did not compile".into()));
    let snap = g.snapshot();
    assert!(snap.ext.jip.aux[&FormId::PLAYER_REF.0].contains_key("nvfq"));
    assert!(serde_json::to_string(&snap).unwrap().contains("nvfq") && !serde_json::to_string(&snap).unwrap().contains("*nvft"), "a * name is not saved");
}

#[test]
fn jip_hit_handlers_run_before_the_damage_and_read_the_hit() {
    // Tasks J6, J7 [documented: SetOnHitEventHandler, GetHitHealthDamage; observed:
    // jip-hits, Health in the handler is the value before the hit].
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let ctx = || fnv_game::game::RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() };
    let reg = "Begin Function { }\nSetOnHitEventHandler (Begin Function { }\nAuxVarSetFlt \"*hD\" (GetHitHealthDamage) -1\nAuxVarSetFlt \"*hH\" (GetAV Health) -1\nAuxVarSetFlt \"*hC\" (GetHitExtendedFlag 1) -1\nEnd) 1 PlayerREF\nEnd\n";
    assert_eq!(g.run_query(reg, ctx()), Ok(XVal::Num(0.0)));
    let before = g.run_query("Begin Function { }\nSetFunctionValue (GetAV Health)\nEnd\n", ctx()).unwrap();
    let info = fnv_game::combat::HitInfo { critical: true, location: -1, ..Default::default() };
    g.hit(FormId::PLAYER_REF, 5.0, info);
    let read = |g: &mut Game, s: &str| g.run_query(&format!("Begin Function {{ }}\nSetFunctionValue (AuxVarGetFlt \"{s}\" 0)\nEnd\n"), ctx()).unwrap();
    assert_eq!(read(&mut g, "*hD"), XVal::Num(5.0));
    assert_eq!(read(&mut g, "*hH"), before, "the handler runs before the damage");
    assert_eq!(read(&mut g, "*hC"), XVal::Num(1.0), "a critical hit");
}

#[test]
fn player_value_changes_cripples_and_ragdolls_reach_their_handlers() {
    // #2134 [documented: SetOnActorValueChangeEventHandler, the player's changes, whole
    // numbers without flag bit 0; SetOnRagdollEventHandler, death; observed: jip_events2,
    // DamageAV LeftAttackCondition 200 fired the crippled-limb handler with limb 2].
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let ctx = || fnv_game::game::RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() };
    let reg = "Begin Function { }\nSetOnActorValueChangeEventHandler 1 (Begin Function {int iAV, float fOld, float fNew}\nPlayerREF.AuxVarSetFlt \"*av\" iAV -1\nPlayerREF.AuxVarSetFlt \"*avd\" (fNew - fOld) -1\nEnd) 0\nSetOnCrippledLimbEventHandler (Begin Function {int iLimb}\nPlayerREF.AuxVarSetFlt \"*limb\" iLimb -1\nEnd) 1 PlayerREF\nSetOnRagdollEventHandler (Begin Function { }\nPlayerREF.AuxVarSetFlt \"*rag\" (GetSelf == PlayerREF) -1\nEnd) 1\nEnd\n";
    assert_eq!(g.run_query(reg, ctx()), Ok(XVal::Num(0.0)));
    let size = |g: &mut Game, s: &str| g.run_query(&format!("Begin Function {{ }}\nSetFunctionValue (PlayerREF.AuxVarSize \"{s}\")\nEnd\n"), ctx()).unwrap();
    let read = |g: &mut Game, s: &str, i: u32| g.run_query(&format!("Begin Function {{ }}\nSetFunctionValue (PlayerREF.AuxVarGetFlt \"{s}\" {i})\nEnd\n"), ctx()).unwrap();
    // A hit is no script function, and the handler still hears of it, once, truncated
    // (100.0 -> 94.6 truncates to 100.0 -> 94.0, delta -6.0; observed 2026-11-24:
    // w6t2p2_10_jg_on_av_change_timed / w6t2p3_10_av_m1_exact).
    g.hit(FormId::PLAYER_REF, 5.4, fnv_game::combat::HitInfo { location: -1, ..Default::default() });
    assert_eq!(size(&mut g, "*av"), XVal::Num(1.0));
    assert_eq!((read(&mut g, "*av", 0), read(&mut g, "*avd", 0)), (XVal::Num(16.0), XVal::Num(-6.0)));
    // DamageAV takes the left arm to 0: crippled, limb 2; the change is told once more.
    g.run_query("Begin Function { }\nPlayerREF.DamageAV LeftAttackCondition 200\nEnd\n", ctx()).unwrap();
    assert_eq!((size(&mut g, "*limb"), read(&mut g, "*limb", 0)), (XVal::Num(1.0), XVal::Num(2.0)));
    assert_eq!((size(&mut g, "*av"), read(&mut g, "*av", 1)), (XVal::Num(2.0), XVal::Num(27.0)));
    // Already crippled: no second firing.
    g.run_query("Begin Function { }\nPlayerREF.DamageAV LeftAttackCondition 10\nEnd\n", ctx()).unwrap();
    assert_eq!(size(&mut g, "*limb"), XVal::Num(1.0));
    // Death ragdolls.
    assert!(g.kill(FormId::PLAYER_REF, None));
    assert_eq!((size(&mut g, "*rag"), read(&mut g, "*rag", 0)), (XVal::Num(1.0), XVal::Num(1.0)));
}

#[test]
fn message_ex_box_ex_and_debug_print() {
    // Task D4 [documented: MessageEx; MessageBoxEx; DebugPrint; SetDebugMode].
    let f = udf_fixture();
    let mut g = game(&f);
    g.run_source("MessageEx \"n %g\", 3\nMessageBoxEx \"Pick|Ok|Cancel\"\nDebugPrint \"hidden\"\nSetDebugMode 1\nDebugPrint \"shown\"", Default::default()).expect("compiles");
    let shown: Vec<(Option<String>, Vec<String>)> = g.events.iter().filter_map(|e| match &e.kind {
        EventKind::MessageShown { text, buttons, .. } => Some((text.clone(), buttons.clone())),
        _ => None,
    }).collect();
    assert_eq!(shown, vec![(Some("n 3".into()), vec![]), (Some("Pick".into()), vec!["Ok".into(), "Cancel".into()])]);
    assert_eq!(g.message_box.as_ref().map(|m| m.buttons), Some(2));
    let printed: Vec<&str> = g.events.iter().filter_map(|e| match &e.kind {
        EventKind::ScriptPrint { text, .. } => Some(text.as_str()),
        _ => None,
    }).collect();
    assert_eq!(printed, ["shown"]);
}

#[test]
fn each_queued_box_keeps_its_own_button_count() {
    // `--no-messages` (nvf-play) closes the front box when it declared one button or none,
    // and leaves it when its count is not known.
    let f = udf_fixture();
    let mut g = game(&f);
    let buttons = |g: &Game| g.front_box().and_then(|b| b.buttons);
    // A form-less Yes/No box A: two buttons, though no Message form says so.
    g.run_source("MessageBoxEx \"A|Yes|No\"", Default::default()).expect("compiles");
    assert_eq!((g.boxes.clone(), buttons(&g)), (vec![FormId(0)], Some(2)));
    // A form box B of two buttons shown behind it overwrites the last-shown record, as
    // ShowMessage does; A on screen still counts its own two.
    g.message_box = Some(fnv_game::game::MessageBox { message: FormId(0x810), buttons: 2, pressed: None, owner: None, hidden: Vec::new() });
    g.show_box_buttons(FormId(0x810), 2);
    // Two more form-less boxes, the same ID 0 and the same words, of other counts: each
    // its own count and its own showing.
    g.run_source("MessageBoxEx \"C|Ok\"", Default::default()).expect("compiles");
    g.run_source("MessageBoxEx \"C|Ok\"", Default::default()).expect("compiles");
    assert_eq!((g.boxes.len(), buttons(&g)), (4, Some(2)));
    let serials: Vec<u64> = g.queued.iter().map(|q| q.serial).collect();
    assert!(serials.windows(2).all(|w| w[0] < w[1]), "every box its own showing: {serials:?}");
    // Through a snapshot and back: the counts and showings kept.
    let text = serde_json::to_string(&g.snapshot()).unwrap();
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&text).unwrap();
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let mut g = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert_eq!((g.boxes.len(), buttons(&g)), (4, Some(2)));
    assert_eq!(g.queued.iter().map(|q| q.serial).collect::<Vec<_>>(), serials);
    // A scenario's box put in front: its count not known; the rest keep theirs.
    g.put_box_in_front(FormId(0));
    assert_eq!(g.front_box().map(|b| b.buttons), Some(None));
    g.menu = Some(fnv_game::game::menus::MESSAGE);
    g.close_menu();
    assert_eq!(buttons(&g), Some(2), "A again, after the scenario's box closed");
    g.close_menu();
    assert_eq!((g.boxes.clone(), buttons(&g)), (vec![FormId(0x810), FormId(0), FormId(0)], Some(2)));
    g.close_menu();
    assert_eq!(buttons(&g), Some(1));
    // Code that changes the queue alone puts it out of step: nothing read, not a 0;
    // the next box shown puts it back in step, the others' counts not known.
    g.boxes.push(FormId(0));
    assert_eq!(g.front_box(), None);
    g.show_box_buttons(FormId(0), 2);
    assert_eq!(g.queued.len(), g.boxes.len());
    // The two boxes left, the one pushed out of step, then the box shown.
    assert_eq!(g.queued.iter().map(|q| q.buttons).collect::<Vec<_>>(), vec![None, None, None, Some(2)]);
}

#[test]
fn answer_ok_boxes_answers_a_scripts_ok_box_without_opening_it() {
    // `--no-messages` (GameConfig::answer_ok_boxes): the add-on packs' start-up boxes
    // never open, so game time does not stop for them.
    use fnv_game::game::menus;
    let mut f = fixture();
    f.data.messages.get_mut(&FormId(MESSAGE)).unwrap().buttons = vec!["Ok".into()];
    let mut g = game(&f);
    g.cfg.answer_ok_boxes = true;
    g.new_game();
    // The activator's ShowMessage (and SetStage Q 10, whose 1 s quest timer then runs).
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    assert_eq!((g.menu, g.boxes.len()), (None, 0), "the box never opened");
    assert_eq!(g.message_box.as_ref().and_then(|m| m.pressed), Some(0), "answered with OK");
    assert_eq!(g.take_answered_boxes(), vec![FormId(MESSAGE)]);
    assert!(g.take_answered_boxes().is_empty(), "taken once");
    let t0 = g.time;
    g.frames(120);
    assert!(g.time > t0, "game time runs on");
    assert!(g.quest_state(q()).stages_done.contains(&20));
    // Off (the default): the same box opens and holds the game.
    let mut g = game(&f);
    g.new_game();
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    assert_eq!((g.menu, g.boxes.clone()), (Some(menus::MESSAGE), vec![FormId(MESSAGE)]));
    assert!(g.take_answered_boxes().is_empty());
}

#[test]
fn answer_ok_boxes_leaves_a_box_with_a_choice_to_the_player() {
    use fnv_game::game::menus;
    use fnv_script::vm::XVal;
    // The fixture's message asks Yes / No.
    let f = fixture();
    let mut g = game(&f);
    g.cfg.answer_ok_boxes = true;
    g.new_game();
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    assert_eq!((g.menu, g.boxes.clone()), (Some(menus::MESSAGE), vec![FormId(MESSAGE)]));
    assert_eq!(g.message_box.as_ref().and_then(|m| m.pressed), None);
    assert!(g.take_answered_boxes().is_empty());
    // A form-less MessageBoxEx box: OK alone is answered (GetButtonPressed of the script
    // that showed it reads 0), Yes / No waits.
    let f = udf_fixture();
    let mut g = game(&f);
    g.cfg.answer_ok_boxes = true;
    let ok = "Begin Function { }\nMessageBoxEx \"C|Ok\"\nSetFunctionValue (GetButtonPressed)\nEnd\n";
    assert_eq!(g.run_query(ok, Default::default()), Ok(XVal::Num(0.0)));
    assert_eq!((g.menu, g.boxes.len(), g.take_answered_boxes()), (None, 0, vec![FormId(0)]));
    g.run_source("MessageBoxEx \"A|Yes|No\"", Default::default()).expect("compiles");
    assert_eq!((g.menu, g.boxes.len()), (Some(menus::MESSAGE), 1));
    assert!(g.take_answered_boxes().is_empty());
}

/// The fixture with its message made a box without buttons (Message Box flag, no ITXT),
/// as BMRhondaFixMsg (MESG 0x000E9551) is (#2925).
fn ok_box_fixture(message_box: bool) -> Fixture {
    let mut f = fixture();
    let msg = f.data.messages.get_mut(&FormId(MESSAGE)).unwrap();
    msg.buttons.clear();
    msg.button_conditions.clear();
    msg.message_box = message_box;
    f
}

/// GetButtonPressed as the activator's script (the one that shows the message) calls it.
fn activator_reads_button(g: &mut Game) -> Value {
    let ctx = Ctx { unit: "test".into(), opcode: 0x101F, on: None, self_ref: None, action_ref: None, quest: None, inst: Some(InstKey::Ref { reference: FormId(ACT_REF) }), in_condition: false };
    g.call_function(&ctx, &[])
}

/// #2925: the Broken Mr. Handy in Black Mountain waits for button 0 after a box with no
/// buttons. The player's OK on such a box gives the script that showed it 0, once, then
/// −1 (inferred from SCPT 0x000E9483; see docs/spec-notes/scripts.md); before, the box had no `message_box`
/// and GetButtonPressed read −1 for good.
#[test]
fn ok_on_a_message_box_without_buttons_gives_its_script_button_0_once() {
    use fnv_game::game::menus;
    let f = ok_box_fixture(true);
    let mut g = game(&f);
    g.new_game();
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    assert_eq!((g.menu, g.boxes.clone()), (Some(menus::MESSAGE), vec![FormId(MESSAGE)]));
    assert_eq!(g.message_box.as_ref().map(|m| (m.buttons, m.pressed)), Some((0, None)));
    assert_eq!(activator_reads_button(&mut g), Value::Num(-1.0), "not answered yet");
    // The player's OK (`Game::close_menu`, as nvf-play closes an answered box).
    assert_eq!(g.close_menu(), Some(menus::MESSAGE));
    assert_eq!(g.menu, None);
    assert_eq!(activator_reads_button(&mut g), Value::Num(0.0));
    assert_eq!(activator_reads_button(&mut g), Value::Num(-1.0), "once");
}

/// A message without the Message Box flag and without buttons is a corner notification:
/// no box waits, GetButtonPressed stays −1 (#2925).
#[test]
fn a_corner_message_gives_get_button_pressed_nothing() {
    let f = ok_box_fixture(false);
    let mut g = game(&f);
    g.new_game();
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    assert_eq!((g.menu, g.boxes.len()), (None, 0));
    assert!(g.message_box.is_none());
    assert_eq!(activator_reads_button(&mut g), Value::Num(-1.0));
}

/// `--no-messages` (GameConfig::answer_ok_boxes) answers the same box with OK as it is
/// shown: its script reads 0 once (#2925; before, −1).
#[test]
fn answer_ok_boxes_gives_a_box_without_buttons_button_0() {
    let f = ok_box_fixture(true);
    let mut g = game(&f);
    g.cfg.answer_ok_boxes = true;
    g.new_game();
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    assert_eq!((g.menu, g.boxes.len()), (None, 0), "the box never opened");
    assert_eq!(g.take_answered_boxes(), vec![FormId(MESSAGE)]);
    assert_eq!(activator_reads_button(&mut g), Value::Num(0.0));
    assert_eq!(activator_reads_button(&mut g), Value::Num(-1.0));
}

/// A box without buttons shown behind a box with buttons that still waits does not take
/// its place: the choice's button reaches its script [design, #2925].
#[test]
fn a_box_without_buttons_leaves_a_waiting_choice_its_button() {
    use fnv_game::game::menus;
    const INFO: u32 = 0x811;
    let mut f = fixture();
    let info = fnv_game::data::MessageDef { text: Some("Info".into()), message_box: true, ..Default::default() };
    f.data.messages.insert(FormId(INFO), info);
    let mut g = game(&f);
    g.new_game();
    // The activator's Yes / No box, then another script's box with OK alone behind it.
    g.activate(FormId::PLAYER_REF, FormId(ACT_REF), ActMode::Hand).unwrap();
    let quest = Ctx { unit: "test".into(), opcode: SHOW_MESSAGE, on: None, self_ref: None, action_ref: None, quest: None, inst: Some(InstKey::Quest { quest: q() }), in_condition: false };
    g.call_function(&quest, &[ArgValue::Value(Value::Ref(INFO))]);
    assert_eq!(g.boxes, vec![FormId(MESSAGE), FormId(INFO)]);
    assert_eq!(g.message_box.as_ref().map(|m| m.message), Some(FormId(MESSAGE)), "the choice keeps the slot");
    g.press_button(1).unwrap();
    assert_eq!(activator_reads_button(&mut g), Value::Num(1.0));
    // The box with OK alone is on screen now; its OK has nothing to answer.
    assert_eq!((g.menu, g.boxes.clone()), (Some(menus::MESSAGE), vec![FormId(INFO)]));
    g.close_menu();
    assert_eq!(g.menu, None);
}

#[test]
fn an_older_snapshots_boxes_restore_as_showings_of_unknown_count() {
    let f = udf_fixture();
    let mut g = game(&f);
    g.run_source("MessageBoxEx \"A|Yes|No\"", Default::default()).expect("compiles");
    g.run_source("MessageBoxEx \"A|Yes|No\"", Default::default()).expect("compiles");
    // A snapshot from before `queued`: the boxes alone.
    let mut s = g.snapshot();
    s.queued.clear();
    let text = serde_json::to_string(&s).unwrap();
    assert!(!text.contains("\"queued\""));
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&text).unwrap();
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let mut g = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    // In step, each box its own showing, its buttons not known (left open by
    // --no-messages, never counted 0).
    assert_eq!(g.queued.len(), 2);
    assert!(g.queued.iter().all(|q| q.buttons.is_none()));
    let (first, second) = (g.queued[0].serial, g.queued[1].serial);
    assert_ne!(first, second, "two boxes alike are two showings");
    // An identical box shown after them is another showing again.
    g.run_source("MessageBoxEx \"A|Yes|No\"", Default::default()).expect("compiles");
    assert_eq!(g.queued.len(), 3);
    assert!(g.queued[2].serial > second && g.queued[2].buttons == Some(2));
    // Closing the front brings the second, still its own showing.
    g.menu = Some(fnv_game::game::menus::MESSAGE);
    g.close_menu();
    assert_eq!(g.front_box().map(|b| b.serial), Some(second));
}

#[test]
fn delayed_calls_run_when_due() {
    // Task D3 [documented: CallAfterFrames (catalogue), CallWhile, CallWhen].
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let ctx = || fnv_game::game::RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() };
    let q = |g: &mut Game, body: &str| g.run_query(&format!("Begin Function {{ }}
{body}
End
"), ctx()).unwrap();
    q(&mut g, "CallAfterFrames 2 ({} => AuxVarSetFlt \"*a\" 1 -1)
CallWhile ({} => AuxVarSetFlt \"*w\" 1 -1) ({} => (AuxVarSize \"*w\") < 3)
CallWhen ({} => AuxVarSetFlt \"*o\" 1 -1) ({} => (AuxVarSize \"*w\") >= 2)");
    g.frames(1);
    assert_eq!(q(&mut g, "SetFunctionValue (AuxVarSize \"*a\")"), XVal::Num(0.0), "not yet");
    g.frames(5);
    assert_eq!(q(&mut g, "SetFunctionValue (AuxVarSize \"*a\")"), XVal::Num(1.0), "once");
    assert_eq!(q(&mut g, "SetFunctionValue (AuxVarSize \"*w\")"), XVal::Num(3.0), "while the condition holds");
    assert_eq!(q(&mut g, "SetFunctionValue (AuxVarSize \"*o\")"), XVal::Num(1.0), "when, exactly once");
}

/// A queued call whose function cannot run (a ref variable still 0, as the player
/// reports' mods queued: "0x00000000 is not a function script") is reported once, not on
/// every frame it comes due (#2280).
#[test]
fn a_call_that_cannot_run_is_reported_once() {
    let f = udf_fixture();
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() };
    g.run_query("ref r
Begin Function { }
CallWhile r ({} => (AuxVarSize \"*never\") < 1)
End
", ctx).unwrap();
    g.frames(10);
    let errors: Vec<String> = g.events.iter().filter_map(|e| match &e.kind {
        EventKind::ScriptError { unit, error } if unit == "delayed call" => Some(error.clone()),
        _ => None,
    }).collect();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].ends_with("is not a function script"), "{errors:?}");
}

#[test]
fn message_box_ex_alt_runs_its_callback_with_the_button() {
    // [documented: MessageBoxExAlt] the UDF gets the pressed button's index.
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx::default;
    let q = "Begin Function { }\nMessageBoxExAlt (Begin Function {int b}\nPlayerREF.AuxVarSetFlt \"*btn\" b\nEnd) \"^Title^Pick|A|B\"\nEnd\n";
    g.run_query(q, ctx()).unwrap();
    g.press_button(1).unwrap();
    g.tick();
    let read = g.run_query("Begin Function { }\nSetFunctionValue (PlayerREF.AuxVarGetFlt \"*btn\")\nEnd\n", ctx()).unwrap();
    assert_eq!(read, XVal::Num(1.0));
}

#[test]
fn player_control_locks_are_kept_per_plugin() {
    // [documented: DisablePlayerControlsAlt] "stored on a per-mod basis": a group stays
    // locked until every plugin that locked it gives it back.
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx::default;
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), ctx()).unwrap();
    q(&mut g, "DisablePlayerControlsAltEx (1 << 3)");
    assert_eq!(g.controls_locked(), [false, false, true, false, false, false, false], "bit 3 is fighting");
    g.other.alt_locks.insert("other.esp".into(), [true, false, false, false, false, false, false]);
    assert_eq!(q(&mut g, "SetFunctionValue (GetPlayerControlsDisabledAlt 0 1)"), XVal::Num(0.0), "movement is the other plugin's");
    assert_eq!(q(&mut g, "SetFunctionValue (GetPlayerControlsDisabledAlt 1 1)"), XVal::Num(1.0), "any plugin's");
    // [observed: nvse-fifth, the vanilla getter sees an Alt lock]
    assert_eq!(q(&mut g, "SetFunctionValue (GetPlayerControlsDisabled)"), XVal::Num(1.0), "the vanilla getter sees them");
    q(&mut g, "EnablePlayerControlsAlt");
    assert_eq!(g.controls_locked(), [true, false, false, false, false, false, false], "the other plugin's lock stays");
}

/// Only a story script's locks hold a co-op host's guests [design: only story
/// scenes; coop-replication.md §7.1]: a quest's lock is a story lock, an object
/// script's on something the player uses is not, and EnablePlayerControls clears both.
#[test]
fn only_a_story_scripts_lock_is_a_story_lock() {
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str, ctx: fnv_game::game::RunCtx| {
        g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), ctx).unwrap();
    };
    let own = fnv_game::game::RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() };
    q(&mut g, "DisablePlayerControls 1 0 0 0", own);
    assert!(g.controls_locked()[0], "the host is held");
    assert_eq!(g.story_locks(), [false; 7], "an object script's lock is the host's own");
    let story = fnv_game::game::RunCtx { quest: Some(FormId(0x901)), ..Default::default() };
    q(&mut g, "DisablePlayerControls 0 1 0 0", story);
    assert_eq!(g.story_locks(), [false, true, false, false, false, false, false], "a quest's lock goes to the guests");
    q(&mut g, "EnablePlayerControls", fnv_game::game::RunCtx::default());
    assert_eq!(g.story_locks(), [false; 7]);
    assert_eq!(g.controls_locked(), [false; 7]);
    // An NVForever mini-game's own lock (the drinking contest's) holds this game only.
    g.other.alt_locks.insert(format!("{}drink_contest", fnv_game::nvse_other::ENGINE_GAME_LOCK_PREFIX), [true; 7]);
    assert_eq!(g.controls_locked(), [true; 7], "the player at the table is held");
    assert_eq!(g.story_locks(), [false; 7], "a mini-game's lock is no scene for the guests");
    assert_eq!(g.scene_locks(), [false; 7], "nor a scene that hides the other players here");
}

/// The runner's `created:<base>` names the newest reference made at run time from a
/// base form (PlaceAtMe, a drop), as the sim runner's actions do: a chain can then
/// activate a container a quest script placed (Emergency Radio's supply cache).
#[test]
fn the_runner_names_a_reference_made_at_run_time_by_its_base() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    assert!(fnv_game::runner::resolve(&g, "created:TestActivator").is_err(), "nothing made yet");
    let first = g.world.place(FormId(ACTIVATOR), None, [0.0; 3], [0.0; 3], 1).unwrap();
    assert_eq!(fnv_game::runner::resolve(&g, "created:TestActivator"), Ok(first));
    let second = g.world.place(FormId(ACTIVATOR), None, [0.0; 3], [0.0; 3], 1).unwrap();
    assert_eq!(fnv_game::runner::resolve(&g, "created:TestActivator"), Ok(second), "the newest");
    // A placed reference of the same base is not one made at run time.
    assert_eq!(fnv_game::runner::resolve(&g, "placed:TestActivator"), Ok(FormId(ACT_REF)));
}

/// Script coverage counts a quest completed when a run marks it completed, by a Complete
/// Quest stage or by the script function CompleteQuest (many quests end by the function
/// with no flagged stage reached: VMS39, VVault21Tour; High Times and Debt Collector have
/// only a fail stage); a chain's given state does not. A quest completed with no stage
/// reached counts as touched.
#[test]
fn coverage_counts_a_quest_completed_by_complete_quest() {
    let f = fixture();
    let report = |cov: &fnv_game::coverage::Coverage| {
        let log = cov.log(&f.data, "t");
        fnv_game::coverage::report(&f.data, &[log], 10).unwrap()
    };
    let ctx = fnv_game::game::RunCtx::default;
    // Stage 10 only (not flagged): touched, not completed.
    let mut g = game(&f);
    g.new_game();
    g.coverage = Some(Default::default());
    g.set_stage(q(), 10);
    let r = report(g.coverage.as_ref().unwrap());
    assert_eq!(r["summary"]["quests"]["touched"], json!(1));
    assert_eq!(r["summary"]["quests"]["completed"], json!(0));
    // A given completed state (as a chain's `given`) is not the run's: still not counted.
    g.quest(q()).completed = true;
    assert!(g.coverage.as_ref().unwrap().completed.is_empty());
    g.quest(q()).completed = false;
    // CompleteQuest with stage 20 (the flagged one) never reached: completed.
    g.run_query("Begin Function { }
CompleteQuest Q
End
", ctx()).unwrap();
    let r = report(g.coverage.as_ref().unwrap());
    assert_eq!(r["summary"]["quests"]["completed"], json!(1));
    assert!(!g.coverage.as_ref().unwrap().stages.contains(&(q(), 20)));
    // A log without the field (older runs) still reads, by the stage flags alone.
    let mut log = g.coverage.as_ref().unwrap().log(&f.data, "old");
    log.as_object_mut().unwrap().remove("completed");
    let r = fnv_game::coverage::report(&f.data, &[log], 10).unwrap();
    assert_eq!(r["summary"]["quests"]["completed"], json!(0));

    // CompleteQuest alone, no stage reached: touched and completed, not untouched.
    let mut g = game(&f);
    g.new_game();
    g.coverage = Some(Default::default());
    g.run_query("Begin Function { }
CompleteQuest Q
End
", ctx()).unwrap();
    let cov = g.coverage.as_ref().unwrap();
    assert!(!cov.stages.iter().any(|(quest, _)| *quest == q()), "no stage of Q reached");
    let r = report(cov);
    assert_eq!(r["summary"]["quests"]["touched"], json!(1));
    assert_eq!(r["summary"]["quests"]["completed"], json!(1));
    assert!(r["untouched"].as_array().unwrap().iter().all(|u| u["quest"] != "Q"));
}

/// Script coverage reports which missing stages the data can set (`fnvtool script
/// cover`): a SetStage call with a literal stage in any unit (a script, a stage result)
/// makes that stage settable; a stage set only from its own result script is not; a
/// stage given by a variable makes the quest's other stages settable "by variable".
#[test]
fn coverage_reports_which_missing_stages_the_data_can_set() {
    const Q2: u32 = 0xA00;
    const Q3: u32 = 0xA01;
    const SCPT: u32 = 0xA02;
    const GLOB: u32 = 0xA03;
    let stage = |s: i16, unit_: Vec<Subrecord>| [vec![sub(b"INDX", s.to_le_bytes().to_vec()), sub(b"QSDT", vec![0])], unit_].concat();
    let data = sub(b"DATA", [vec![0u8, 0, 0, 0], 0f32.to_le_bytes().to_vec()].concat());
    // Q2 stage 10: SetStage Q2 10 (itself) ; SetStage Q2 20. Stage 20: SetStage Q3
    // TestGlobal. Stage 30: nothing.
    let q2 = [
        vec![sub(b"EDID", z("Q2")), data.clone()],
        stage(10, unit(0, &[call(SET_STAGE, &[r(1), n(10)]), call(SET_STAGE, &[r(1), n(20)])].concat(), &[], &[Q2])),
        stage(20, unit(0, &call(SET_STAGE, &[r(1), [vec![b'G'], le16(2)].concat()]), &[], &[Q3, GLOB])),
        stage(30, vec![]),
    ]
    .concat();
    let q3 = [vec![sub(b"EDID", z("Q3")), data], stage(5, vec![]), stage(6, vec![])].concat();
    // A script: begin GameMode / SetStage Q2 30 / end.
    let scpt = [vec![sub(b"EDID", z("SetterScript"))], unit(0, &script(&[(0, None, call(SET_STAGE, &[r(1), n(30)]))]), &[], &[Q2])].concat();
    let glob = record(b"GLOB", 0, GLOB, &[sub(b"EDID", z("TestGlobal")), sub(b"FNAM", vec![b's']), sub(b"FLTV", 5f32.to_le_bytes().to_vec())]);
    let bytes = [
        tes4(),
        top(b"GLOB", &glob),
        top(b"SCPT", &record(b"SCPT", 0, SCPT, &scpt)),
        top(b"QUST", &[record(b"QUST", 0, Q2, &q2), record(b"QUST", 0, Q3, &q3)].concat()),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "Test.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = schema();
    let (table, j) = catalogue();
    let refs = RefTable::build(&lo, &schema);
    let d = GameData::build(&lo, &schema, table, &j, &refs);
    assert_eq!(d.quests[&FormId(Q2)].stages.len(), 3);

    let mut cov = fnv_game::coverage::Coverage::default();
    cov.stages.insert((FormId(Q2), 20));
    let r = fnv_game::coverage::report(&d, &[cov.log(&d, "t")], 10).unwrap();
    let s = &r["summary"]["stages"];
    assert_eq!(s["total"], json!(5));
    assert_eq!(s["reached"], json!(1));
    assert_eq!(s["settable"], json!(2), "Q2 20 and 30; not 10, set only by its own result");
    assert_eq!(s["by_variable"], json!(2), "Q3 5 and 6");
    assert_eq!(s["missing_settable"], json!(1));
    assert_eq!(s["missing_by_variable"], json!(2));
    let quests = r["detail"]["quests"].as_array().unwrap();
    let q2 = quests.iter().find(|q| q["quest"] == "Q2").unwrap();
    assert_eq!(q2["missing_settable"], json!([30]));
    assert_eq!(q2["missing_unsettable"], json!([10]));
    assert_eq!(q2["setters"]["30"], json!([{"kind": "script", "form": "Test.esm:000A02", "edid": "SetterScript"}]));
    assert!(q2.get("missing_by_variable").is_none());
    let q3 = quests.iter().find(|q| q["quest"] == "Q3").unwrap();
    assert_eq!(q3["missing_settable"], json!([]));
    assert_eq!(q3["missing_by_variable"], json!([5, 6]));
    assert_eq!(q3["by_variable_setters"], json!([{"kind": "stage", "form": "Test.esm:000A00", "edid": "Q2", "stage": 20}]));
    // The setter scan: stage 20 is set by Q2's stage 10 result.
    let set = fnv_game::coverage::stage_setters(&d);
    assert_eq!(set.literal[&(FormId(Q2), 20)][0].stage, Some(10));
    assert!(!set.literal.contains_key(&(FormId(Q2), 10)));
}

/// A condition's null form (FormID 0) reads 0 for GetIsID and GetQuestVariable, and for
/// any other function (#966, hypothesis), without a failed call [observed 2026-10-30, capture o4_00_probe: Papa Khan's goodbye with
/// `GetQuestVariable <null> 1 == 0` was chosen and Cass's greeting with only `GetIsID
/// <null> == 1` passed over; observer-checks.md check 35].
#[test]
fn a_null_form_in_a_condition_reads_zero_without_a_failed_call() {
    use fnv_game::cond::{Cmp, Condition, Operand, RunOn};
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let index = |name: &str| f.data.table.by_name(name).unwrap() - 0x1000;
    let null = ArgValue::Value(Value::NONE);
    let cond = |function: u16, params: Vec<ArgValue>, cmp: Cmp, rhs: f64| Condition { cmp, or: false, run_on: RunOn::Subject, function, params, rhs: Operand::Value(rhs) };
    let is_id = cond(index("GetIsID"), vec![null.clone()], Cmp::Eq, 1.0);
    let qvar_0 = cond(index("GetQuestVariable"), vec![null.clone(), ArgValue::Value(Value::Num(1.0))], Cmp::Eq, 0.0);
    let qvar_1 = cond(index("GetQuestVariable"), vec![null, ArgValue::Value(Value::Num(1.0))], Cmp::Eq, 1.0);
    let player = Some(FormId::PLAYER_REF);
    assert!(!g.conditions_pass(&[is_id], player, None, "null GetIsID"));
    assert!(g.conditions_pass(&[qvar_0], player, None, "null GetQuestVariable == 0"));
    assert!(!g.conditions_pass(&[qvar_1], player, None, "null GetQuestVariable == 1"));
    // Any other function with a null form parameter reads 0 the same way (#966).
    for name in ["GetInFaction", "GetItemCount", "GetDistance", "GetDead"] {
        let zero = cond(index(name), vec![ArgValue::Value(Value::NONE)], Cmp::Eq, 0.0);
        assert!(g.conditions_pass(&[zero], player, None, name), "{name} <null> == 0");
    }
    assert!(!g.events.iter().any(|e| matches!(&e.kind, EventKind::CallFailed { .. })), "{:?}", g.events.iter().filter(|e| matches!(&e.kind, EventKind::CallFailed { .. })).collect::<Vec<_>>());
}

#[test]
fn a_string_variable_named_in_a_plain_string_parameter_is_its_name() {
    // [observed: nvse-sixth, `SetStringIniSetting "sLanguage:General" sText` set the text
    // "sText"; documented: SetNameEx, string variables "must be converted" with `$`].
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx::default;
    let q = "string_var s\nref r\nBegin Function { }\nlet s := \"nvfKey\"\nlet r := PlayerREF\nSetEditorID r s\nSetFunctionValue (EditorIDToFormID \"s\")\nEnd\n";
    assert_eq!(g.run_query(q, ctx()).unwrap(), XVal::Ref(0x14), "the editor ID is the text \"s\"");
}

#[test]
fn main_loop_callbacks_of_function_scripts_outlive_a_load_unless_flag_8() {
    // [documented: SetGameMainLoopCallback, 8 "auto-remove on load game"] TestSet sets
    // TestGlobal to its parameter, 0 when called with none.
    for (flags, ran) in [(1, true), (9, false)] {
        let f = udf_fixture();
        let mut g = game(&f);
        g.run_source(&format!("SetGameMainLoopCallback TestSet 1 1 {flags}"), Default::default()).unwrap();
        let session = g.take_session();
        let mut g2 = game(&f);
        g2.put_session(session);
        g2.run_source("set TestGlobal to 5", Default::default()).unwrap();
        g2.tick();
        let v = g2.world.global(FormId(0x0000_0800)).unwrap();
        assert_eq!(v == 0.0, ran, "flags {flags}: the callback {} after the load", if ran { "runs" } else { "is gone" });
    }
}

#[test]
fn console_functions_run_their_commands() {
    // #913 [documented: the Con_ pages, "Runs the console command … via a script"].
    use fnv_game::functions::console::ConsoleRequest;
    use fnv_game::functions::toggles::Toggle;
    use fnv_script::vm::XVal;
    let mut f = udf_fixture();
    f.data.ini_texts = vec!["[Display]\nfDefaultFOV=75.0000\n".into()];
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx::default;
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), ctx());
    assert_eq!(q(&mut g, "Con_TFC\nSetFunctionValue GetTFC"), Ok(XVal::Num(1.0)), "Con_TFC is ToggleFlyCam");
    q(&mut g, "con_SetCameraFOV 90 60").unwrap();
    assert_eq!(g.ui.camera.fov, Some((Some(90.0), Some(60.0))));
    q(&mut g, "Con_TFIK\nCon_TCL").unwrap();
    assert_eq!(g.toggles.asked, [Toggle::FootIk, Toggle::Collision]);
    q(&mut g, "con_QuitGame\ncon_Save \"nvfq\" 1").unwrap();
    assert_eq!(g.console_requests, [ConsoleRequest::Quit, ConsoleRequest::Save("nvfq".into())]);
    // The INI the extender reads: con_SetINISetting changes it, con_RefreshINI reads the
    // files again.
    let num = |g: &mut fnv_game::Game| q(g, "SetFunctionValue (GetNumericINISetting \"fDefaultFOV:Display\")");
    q(&mut g, "con_SetINISetting \"fDefaultFOV:Display\" \"80\"").unwrap();
    assert_eq!(num(&mut g), Ok(XVal::Num(80.0)));
    // Lines only with the echo on [documented: Con_SQV, Con_Inv "Requires Console Echo"].
    q(&mut g, "con_GetINISetting \"fDefaultFOV:Display\"").unwrap();
    assert!(g.console_out.is_empty(), "no echo");
    q(&mut g, "SetConsoleEcho 1\ncon_GetINISetting \"fDefaultFOV:Display\"").unwrap();
    assert_eq!(g.console_out, ["fDefaultFOV:Display = 80"]);
    q(&mut g, "con_RefreshINI\ncon_SaveINI").unwrap();
    assert_eq!(num(&mut g), Ok(XVal::Num(75.0)), "the files' value again");
}

#[test]
fn script_console_assignments_update_quest_variables_instead_of_calling_their_names() {
    use fnv_script::vm::XVal;
    let f = fixture();
    let mut g = game(&f);
    let timer = g.run_query("Begin Function {}\nConsole \"Q.fTimer = 2\"\nConsole \"Q.fTimer += 0.5\"\nConsole \"set Q.bRun to 7.8\"\nSetFunctionValue Q.fTimer\nEnd", Default::default()).unwrap();
    assert_eq!(timer, XVal::Num(2.5));
    let run_flag = g.run_query("Begin Function {}\nSetFunctionValue Q.bRun\nEnd", Default::default()).unwrap();
    assert_eq!(run_flag, XVal::Num(7.0));
    assert!(g.console_out.iter().all(|line| !line.starts_with("Script error:")), "{:?}", g.console_out);
    assert!(g.events.iter().all(|e| !matches!(&e.kind, EventKind::ScriptError { .. })));
}

#[test]
fn script_console_assignment_expressions_preserve_the_callers_next_statement() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let result = g.run_query("float result\nBegin Function {}\nConsole \"set TestGlobal to 5\"\nConsole \"TestGlobal += 3 * 2\"\nlet result := TestGlobal + 1\nSetFunctionValue result\nEnd", Default::default()).unwrap();
    assert_eq!(result, XVal::Num(12.0));
    assert_eq!(g.world.global(FormId(0x800)).unwrap(), 11.0);
    assert!(g.console_out.is_empty(), "success is silent with echo off: {:?}", g.console_out);
}

#[test]
fn script_console_classic_set_accepts_single_token_expressions() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    for (line, expected) in [("set TestGlobal to (5)", 5.0), ("set TestGlobal to 3*2", 6.0), ("set TestGlobal to -7", -7.0)] {
        fnv_game::nvse::call(&mut g, &Default::default(), "console", None, &[XVal::Str(line.into())]).unwrap().unwrap();
        assert_eq!(g.world.global(FormId(0x800)).unwrap(), expected);
    }
    assert!(g.console_out.is_empty(), "{:?}", g.console_out);
}

#[test]
fn script_console_cache_limits_distinct_sources_but_reuses_existing_entries() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let run = |g: &mut fnv_game::Game, line: String| {
        fnv_game::nvse::call(g, &Default::default(), "console", None, &[XVal::Str(line)]).unwrap().unwrap();
    };
    for i in 0..256 { run(&mut g, format!("TestGlobal = {i}")); }
    assert!(g.console_out.is_empty());
    run(&mut g, "TestGlobal = 999".into());
    assert_eq!(g.console_out.len(), 1);
    assert!(g.console_out[0].contains("256 distinct"));
    assert_eq!(g.world.global(FormId(0x800)).unwrap(), 255.0);
    run(&mut g, "TestGlobal = 3".into());
    assert_eq!(g.console_out.len(), 1, "cache hits remain executable at the limit");
    assert_eq!(g.world.global(FormId(0x800)).unwrap(), 3.0);
}

#[test]
fn script_console_source_length_boundary_does_not_disable_shorter_lines() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let run = |g: &mut fnv_game::Game, line: String| {
        fnv_game::nvse::call(g, &Default::default(), "console", None, &[XVal::Str(line)]).unwrap().unwrap();
    };
    let mut exact = "TestGlobal = 7".to_string();
    exact.extend(std::iter::repeat_n(' ', 16 * 1024 - exact.len()));
    run(&mut g, exact.clone());
    assert!(g.console_out.is_empty());
    assert_eq!(g.world.global(FormId(0x800)).unwrap(), 7.0);
    exact.push(' ');
    run(&mut g, exact);
    assert_eq!(g.console_out.len(), 1);
    assert!(g.console_out[0].contains("16 KiB"));
    run(&mut g, "TestGlobal = 8".into());
    assert_eq!(g.world.global(FormId(0x800)).unwrap(), 8.0);
    assert_eq!(g.console_out.len(), 1);
}

#[test]
fn script_console_unknown_plain_commands_still_report_errors_and_continue() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let result = g.run_query("Begin Function {}\nConsole \"OwnMissingCommand\"\nConsole \"set TestGlobal to 9\"\nSetFunctionValue TestGlobal\nEnd", Default::default()).unwrap();
    assert_eq!(result, XVal::Num(9.0));
    assert_eq!(g.console_out.len(), 1);
    assert!(g.console_out[0].contains("OwnMissingCommand"));
}

#[test]
fn script_console_repeated_expressions_reuse_their_compilation_and_read_live_state() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    g.world.set_global(FormId(0x800), 0.0).unwrap();
    for _ in 0..300 {
        fnv_game::nvse::call(&mut g, &Default::default(), "console", None, &[XVal::Str("TestGlobal += 1".into())]).unwrap().unwrap();
    }
    assert_eq!(g.world.global(FormId(0x800)).unwrap(), 300.0);
    assert!(g.console_out.is_empty(), "the repeated expression must not exhaust its cache: {:?}", g.console_out);
}

#[test]
fn script_console_runtime_and_syntax_failures_are_diagnostic_and_do_not_end_the_caller() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let result = g.run_query("Begin Function {}\nConsole \"TestGlobal = 1 / 0\"\nConsole \"TestGlobal +=\"\nConsole \"set TestGlobal to 13\"\nSetFunctionValue TestGlobal\nEnd", Default::default()).unwrap();
    assert_eq!(result, XVal::Num(13.0));
    assert_eq!(g.console_out.len(), 2, "{:?}", g.console_out);
    assert!(g.console_out[0].contains("division by zero"));
    assert!(g.console_out[1].starts_with("Script error: console:"));
}

#[test]
fn script_console_changing_signed_number_words_preserve_word_dispatch() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let run = |g: &mut fnv_game::Game, line: String| {
        fnv_game::nvse::call(g, &Default::default(), "console", None, &[XVal::Str(line)]).unwrap().unwrap();
    };
    for i in 1..=300 {
        run(&mut g, format!("player.SetPos z -{i}"));
        run(&mut g, format!("player.SetAngle z -{}", f64::from(i) / 10.0));
        let player = g.world.get(FormId::PLAYER_REF).unwrap();
        assert_eq!(player.pos[2], -(i as f32), "negative SetPos word {i}");
        assert!((player.rot[2].to_degrees() + i as f32 / 10.0).abs() < 0.001, "negative SetAngle word {i}");
    }
    assert!(g.console_out.is_empty(), "ordinary numeric words: {:?}", g.console_out);
}

#[test]
fn script_console_numeric_words_leave_the_expression_budget_and_post_budget_dispatch_available() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let run = |g: &mut fnv_game::Game, line: String| {
        fnv_game::nvse::call(g, &Default::default(), "console", None, &[XVal::Str(line)]).unwrap().unwrap();
    };
    for i in 1..=300 {
        run(&mut g, format!("player.SetPos z -{i}"));
        run(&mut g, format!("player.SetAngle z -{}", f64::from(i) / 10.0));
    }
    assert!(g.console_out.is_empty(), "ordinary numeric words: {:?}", g.console_out);
    // All expression slots remain available after more than 256 changing commands.
    for i in 0..256 { run(&mut g, format!("TestGlobal = {i}")); }
    assert!(g.console_out.is_empty(), "numeric words must not occupy expression slots: {:?}", g.console_out);
    run(&mut g, "TestGlobal = 999".into());
    assert_eq!(g.console_out.len(), 1);
    assert!(g.console_out[0].contains("256 distinct"));
    run(&mut g, "player.SetPos z -999".into());
    assert_eq!(g.world.get(FormId::PLAYER_REF).unwrap().pos[2], -999.0);
    assert_eq!(g.console_out.len(), 1, "word dispatch remains available after the expression budget");
}

#[test]
fn script_console_compact_decimal_assignments_still_use_the_expression_compiler() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    g.world.set_global(FormId(0x800), 0.0).unwrap();
    for _ in 0..300 {
        fnv_game::nvse::call(&mut g, &Default::default(), "console", None, &[XVal::Str("TestGlobal+=0.5".into())]).unwrap().unwrap();
    }
    assert_eq!(g.world.global(FormId(0x800)).unwrap(), 150.0);
    assert!(g.console_out.is_empty(), "{:?}", g.console_out);
}

#[test]
fn script_console_unquoted_string_paths_reach_the_word_handler_without_compilation() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let run = |g: &mut fnv_game::Game, line: String| {
        fnv_game::nvse::call(g, &Default::default(), "console", None, &[XVal::Str(line)]).unwrap().unwrap();
    };
    for i in 0..256 { run(&mut g, format!("TestGlobal = {i}")); }
    assert!(g.console_out.is_empty());
    // PlayIdle's declared string argument must reach its existing host lookup as one
    // unchanged word. The authored missing name gives a discriminating host error.
    run(&mut g, "player.PlayIdle Owned-Thing/path.kf".into());
    assert_eq!(g.console_out.len(), 1);
    assert!(g.console_out[0].contains("no form named Owned-Thing/path.kf"), "{:?}", g.console_out);
    assert!(!g.console_out[0].contains("256 distinct"));
    let failures = g.events.iter().filter(|e| matches!(&e.kind, EventKind::CallFailed { name, .. } if name.eq_ignore_ascii_case("PlayIdle"))).count();
    assert_eq!(failures, 1, "a failed word handler is not retried through the compiler");
}

#[test]
fn generic_form_lists_and_animation_lists_change_for_the_session() {
    // #914 [documented: GenericAddForm "-2 will add to the end", GenericDeleteForm and
    // GenericReplaceForm return the form taken out; AddAnimation, DelAnimation,
    // DelAnimations, GetNthAnimation].
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx::default;
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), ctx()).unwrap();
    let glob = XVal::Ref(0x0000_0901);
    let player = XVal::Ref(FormId::PLAYER_REF.0);
    // On an empty list, -2 appends and returns 0 [observed: w4d_04_generic_form_neg_idx,
    // #3802 / PR #3803]; a subsequent index 0 inserts at front and shifts it to index 1.
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 7, PlayerREF, TestAdd, -2)"), XVal::Num(0.0), "empty list -2 appends returning 0");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 7, PlayerREF, PlayerREF, 0)"), XVal::Num(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericGetForm 7, PlayerREF, 0)"), player, "index 0 put first");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericGetForm 7, PlayerREF, 1)"), glob);
    assert_eq!(q(&mut g, "SetFunctionValue (GenericGetForm 7, PlayerREF, 2)"), XVal::Ref(0), "outside the list");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericReplaceForm 7, PlayerREF, TestAdd, 0)"), player);
    assert_eq!(q(&mut g, "SetFunctionValue (GenericDeleteForm 7, PlayerREF, 1)"), glob);
    assert_eq!(q(&mut g, "SetFunctionValue (ar_Size (GetBaseSpellListLevSpells PlayerREF))"), XVal::Num(1.0), "the getter sees it");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericGetForm 99, PlayerREF, 0)"), XVal::Ref(0), "no such list");
    // Valid target past-end index 999 returns -1 without appending [observed: 2026-11-23,
    // GSDocMitchellHouse (0x00103DF9), `core_13_generic_add_form_split`].
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 7, PlayerREF, TestAdd, 999)"), XVal::Num(-1.0), "valid target past-end index returns -1");
    assert_eq!(q(&mut g, "SetFunctionValue (ar_Size (GetBaseSpellListLevSpells PlayerREF))"), XVal::Num(1.0), "past-end index does not append");
    // Mismatched target record types and unknown list codes return -1 for Add and 0 for
    // Get/Delete/Replace [observed: `core_13_generic_add_form_split`].
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 1, PlayerREF, TestAdd, 0)"), XVal::Num(-1.0), "mismatched target at index 0 returns -1");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 1, PlayerREF, TestAdd, -2)"), XVal::Num(-1.0), "mismatched target at index -2 returns -1");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 1, PlayerREF, TestAdd, 999)"), XVal::Num(-1.0), "mismatched target at index 999 returns -1");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericGetForm 1, PlayerREF, 0)"), XVal::Ref(0), "mismatched Get returns 0");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericDeleteForm 1, PlayerREF, 0)"), XVal::Ref(0), "mismatched Delete returns 0");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericReplaceForm 1, PlayerREF, TestAdd, 0)"), XVal::Ref(0), "mismatched Replace returns 0");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 99, PlayerREF, TestAdd, 0)"), XVal::Num(-1.0), "unknown list code returns -1");

    q(&mut g, "AddAnimation \"a.kf\" PlayerREF\nAddAnimation \"B.kf\" PlayerREF\nAddAnimation \"A.KF\" PlayerREF");
    let nth = |g: &mut fnv_game::Game, i: u32| g.run_query(&format!("Begin Function {{ }}\nSetFunctionValue (PlayerREF.GetNthAnimation {i})\nEnd\n"), ctx()).unwrap();
    assert_eq!((nth(&mut g, 0), nth(&mut g, 1), nth(&mut g, 2)), (XVal::Str("a.kf".into()), XVal::Str("B.kf".into()), XVal::Str(String::new())));
    q(&mut g, "PlayerREF.DelAnimation \"b.kf\"");
    assert_eq!(nth(&mut g, 1), XVal::Str(String::new()));
    q(&mut g, "PlayerREF.DelAnimations");
    assert_eq!(nth(&mut g, 0), XVal::Str(String::new()));
}

#[test]
fn generic_form_lists_validate_target_type_and_past_end_index() {
    // [observed: 2026-11-23, GSDocMitchellHouse (0x00103DF9), Fallout: New Vegas
    // 1.4.0.525, xNVSE 6.4.9 with the 10 official base masters, clean-room script probe
    // `core_13_generic_add_form_split`]:
    // (a) Valid targets (RACE 0/1, NPC_ 5, FLST 13) accept index 0 (returns 0) and -2
    //     (appends, returns n0 - 1), and reject past-end index 999 with -1 without mutating.
    // (b) Mismatched target record types (NPC_+1, RACE+5, FLST+1) reject GenericAddForm
    //     with -1 even at valid index 0/-2, and return 00000000 for Get/Delete/Replace.
    use fnv_script::vm::XVal;
    let race = record(b"RACE", 0, 0x0000_0810, &[sub(b"EDID", z("TestRace")), sub(b"HNAM", 0x0000_0811u32.to_le_bytes().to_vec()), sub(b"ENAM", 0x0000_0812u32.to_le_bytes().to_vec())]);
    let hair = record(b"HAIR", 0, 0x0000_0811, &[sub(b"EDID", z("TestHair"))]);
    let eyes = record(b"EYES", 0, 0x0000_0812, &[sub(b"EDID", z("TestEye"))]);
    let pkg = record(b"PACK", 0, 0x0000_0813, &[sub(b"EDID", z("TestPkg"))]);
    let npc = record(b"NPC_", 0, 0x0000_0814, &[sub(b"EDID", z("TestNpc")), sub(b"PKID", 0x0000_0813u32.to_le_bytes().to_vec())]);
    let flst = record(b"FLST", 0, 0x0000_0815, &[sub(b"EDID", z("TestFlst")), sub(b"LNAM", 0x0000_0813u32.to_le_bytes().to_vec())]);
    let flst_empty = record(b"FLST", 0, 0x0000_0816, &[sub(b"EDID", z("TestEmptyFlst"))]);
    let flst_multi = record(
        b"FLST",
        0,
        0x0000_0817,
        &[
            sub(b"EDID", z("TestMultiFlst")),
            sub(b"LNAM", 0x0000_0811u32.to_le_bytes().to_vec()),
            sub(b"LNAM", 0x0000_0812u32.to_le_bytes().to_vec()),
            sub(b"LNAM", 0x0000_0813u32.to_le_bytes().to_vec()),
        ],
    );
    let bytes = [
        tes4(),
        top(b"HAIR", &hair),
        top(b"EYES", &eyes),
        top(b"RACE", &race),
        top(b"PACK", &pkg),
        top(b"NPC_", &npc),
        top(b"FLST", &[flst, flst_empty, flst_multi].concat()),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "GenericLists.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = schema();
    let (table, j) = catalogue();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    let f = Fixture { data, forms, refs };
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx::default;
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), ctx()).unwrap();

    // (a) Valid targets: RACE code 0 (Hair), RACE code 1 (Eyes), NPC_ code 5 (Packages), FLST code 13 (FormList).
    for (code, target, item) in [(0, "TestRace", "TestHair"), (1, "TestRace", "TestEye"), (5, "TestNpc", "TestPkg"), (13, "TestFlst", "TestPkg")] {
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericAddForm {code}, {target}, {item}, 999)")), XVal::Num(-1.0), "code {code} on {target} rejects 999");
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericGetForm {code}, {target}, 1)")), XVal::Ref(0), "code {code} on {target} did not append on 999");
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericAddForm {code}, {target}, {item}, 0)")), XVal::Num(0.0), "code {code} on {target} inserts at 0");
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericAddForm {code}, {target}, {item}, -2)")), XVal::Num(1.0), "code {code} on {target} appends with -2 returning n0 - 1");
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericAddForm {code}, {target}, {item}, 999)")), XVal::Num(-1.0), "code {code} on {target} still rejects 999 at len 3");
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericGetForm {code}, {target}, 3)")), XVal::Ref(0), "code {code} on {target} len stayed 3");
    }

    // (b) Mismatched target record types: NPC_ + code 1, RACE + code 5, FLST + code 1.
    for (code, target, item) in [(1, "TestNpc", "TestEye"), (5, "TestRace", "TestPkg"), (1, "TestFlst", "TestEye")] {
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericAddForm {code}, {target}, {item}, 0)")), XVal::Num(-1.0), "mismatched {target} + code {code} Add(0)");
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericAddForm {code}, {target}, {item}, -2)")), XVal::Num(-1.0), "mismatched {target} + code {code} Add(-2)");
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericAddForm {code}, {target}, {item}, 999)")), XVal::Num(-1.0), "mismatched {target} + code {code} Add(999)");
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericGetForm {code}, {target}, 0)")), XVal::Ref(0), "mismatched {target} + code {code} Get");
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericDeleteForm {code}, {target}, 0)")), XVal::Ref(0), "mismatched {target} + code {code} Delete");
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericReplaceForm {code}, {target}, {item}, 0)")), XVal::Ref(0), "mismatched {target} + code {code} Replace");
    }

    // (c) Negative indices on empty and non-empty FLST [observed: w4d_04_generic_form_neg_idx,
    // #3802 / PR #3803: `emp(n0=0,d=0/0/0/0:0,r=0:0/0:1/0/0:1,a=-1/0:1/-1/-1/-1:1)`]:
    // Empty FLST (n0=0): Delete(-1/-2/-3/oob) all return 0 leaving n=0;
    // Replace(-1/-3/oob) return 0 leaving n=0, while Replace(-2) returns 0 AND appends (n: 0->1);
    // Add(-1/-3/-10/oob) return -1 leaving n=0, while Add(-2) returns 0 AND appends (n: 0->1).
    for idx in [-1, -2, -3, 99] {
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericDeleteForm 13, TestEmptyFlst, {idx})")), XVal::Ref(0), "empty FLST Delete({idx})");
        assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestEmptyFlst)"), XVal::Num(0.0), "empty FLST count stayed 0 after Delete({idx})");
    }
    for idx in [-1, -3, 99] {
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericReplaceForm 13, TestEmptyFlst, TestPkg, {idx})")), XVal::Ref(0), "empty FLST Replace({idx})");
        assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestEmptyFlst)"), XVal::Num(0.0), "empty FLST count stayed 0 after Replace({idx})");
    }
    assert_eq!(q(&mut g, "SetFunctionValue (GenericReplaceForm 13, TestEmptyFlst, TestPkg, -2)"), XVal::Ref(0), "empty FLST Replace(-2) returns 0");
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestEmptyFlst)"), XVal::Num(1.0), "empty FLST Replace(-2) appends (n: 0->1)");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericGetForm 13, TestEmptyFlst, 0)"), XVal::Ref(0x0000_0813));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericDeleteForm 13, TestEmptyFlst, 0)"), XVal::Ref(0x0000_0813));
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestEmptyFlst)"), XVal::Num(0.0));
    for idx in [-1, -3, -10, 5] {
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericAddForm 13, TestEmptyFlst, TestPkg, {idx})")), XVal::Num(-1.0), "empty FLST Add({idx})");
        assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestEmptyFlst)"), XVal::Num(0.0), "empty FLST count stayed 0 after Add({idx})");
    }
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 13, TestEmptyFlst, TestPkg, -2)"), XVal::Num(0.0), "empty FLST Add(-2) returns 0");
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestEmptyFlst)"), XVal::Num(1.0), "empty FLST Add(-2) appends (n: 0->1)");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericGetForm 13, TestEmptyFlst, 0)"), XVal::Ref(0x0000_0813));
    // Exact boundary `i == n` when `n > 0` returns -1 without appending, whereas `i == 0`
    // succeeds on an empty list (`n == 0`) [observed: w6t2p3_25_generic_form_real_flst].
    assert_eq!(q(&mut g, "SetFunctionValue (GenericDeleteForm 13, TestEmptyFlst, 0)"), XVal::Ref(0x0000_0813));
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestEmptyFlst)"), XVal::Num(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 13, TestEmptyFlst, TestHair, 0)"), XVal::Num(0.0), "empty FLST Add(0) inserts at 0");
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestEmptyFlst)"), XVal::Num(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 13, TestEmptyFlst, TestEye, 1)"), XVal::Num(-1.0), "non-empty FLST (n=1) Add(1 == n) returns -1");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 13, TestEmptyFlst, TestPkg, 2)"), XVal::Num(-1.0), "non-empty FLST (n=1) Add(2 > n) returns -1");
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestEmptyFlst)"), XVal::Num(1.0), "Add(n) did not append");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericDeleteForm 13, TestEmptyFlst, -1)"), XVal::Ref(0x0000_0811), "Delete(-1) removes sole entry");
    assert_eq!(q(&mut g, "SetFunctionValue (GenericDeleteForm 13, TestEmptyFlst, -2)"), XVal::Ref(0), "Delete(-2) on now-empty list returns 0");
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestEmptyFlst)"), XVal::Num(0.0));

    // Non-empty FLST (TestMultiFlst = [TestHair 0x811, TestEye 0x812, TestPkg 0x813], n0=3):
    // Add: `i == n` (3) returns -1 without mutating; `0..n` inserts in-range; ONLY -2 appends (ret = n0 - 1 = 2, n = 4); -1/-3/-10/n+5 return -1.
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 13, TestMultiFlst, TestRace, 3)"), XVal::Num(-1.0), "non-empty FLST (n=3) Add(3 == n) returns -1");
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestMultiFlst)"), XVal::Num(3.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 13, TestMultiFlst, TestRace, -1)"), XVal::Num(-1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestMultiFlst)"), XVal::Num(3.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericAddForm 13, TestMultiFlst, TestRace, -2)"), XVal::Num(2.0));
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestMultiFlst)"), XVal::Num(4.0));
    for idx in [-3, -10, 9] {
        assert_eq!(q(&mut g, &format!("SetFunctionValue (GenericAddForm 13, TestMultiFlst, TestRace, {idx})")), XVal::Num(-1.0), "non-empty FLST Add({idx})");
        assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestMultiFlst)"), XVal::Num(4.0));
    }
    // Replace: ONLY -2 replaces last entry (TestRace 0x810 -> TestNpc 0x814, returning 0x810);
    // -1/-3/999 return 0 without mutating.
    assert_eq!(q(&mut g, "SetFunctionValue (GenericReplaceForm 13, TestMultiFlst, TestNpc, -1)"), XVal::Ref(0));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericReplaceForm 13, TestMultiFlst, TestNpc, -2)"), XVal::Ref(0x0000_0810));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericGetForm 13, TestMultiFlst, 3)"), XVal::Ref(0x0000_0814));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericReplaceForm 13, TestMultiFlst, TestNpc, -3)"), XVal::Ref(0));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericReplaceForm 13, TestMultiFlst, TestNpc, 999)"), XVal::Ref(0));
    // Delete: -1, -2, -3 each remove an entry on a non-empty list (returning non-zero and
    // decrementing count 4->3->2->1; [observed: w4d_04_generic_form_neg_idx]); removing
    // index 0 (head of the list: 0x811, 0x812, 0x813; see docs/spec-notes/ext-console-generic.md §3 and crates/fnv-game/src/nvse_lists.rs). OOB 999 returns 0.
    assert_eq!(q(&mut g, "SetFunctionValue (GenericDeleteForm 13, TestMultiFlst, -1)"), XVal::Ref(0x0000_0811));
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestMultiFlst)"), XVal::Num(3.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericDeleteForm 13, TestMultiFlst, -2)"), XVal::Ref(0x0000_0812));
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestMultiFlst)"), XVal::Num(2.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericDeleteForm 13, TestMultiFlst, -3)"), XVal::Ref(0x0000_0813));
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestMultiFlst)"), XVal::Num(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericGetForm 13, TestMultiFlst, 0)"), XVal::Ref(0x0000_0814));
    assert_eq!(q(&mut g, "SetFunctionValue (GenericDeleteForm 13, TestMultiFlst, 999)"), XVal::Ref(0));
    assert_eq!(q(&mut g, "SetFunctionValue (ListGetCount TestMultiFlst)"), XVal::Num(1.0));
}

#[test]
fn compile_script_reads_a_udf_file_and_print_var_names_variables() {
    // [documented: CompileScript, a text file under data/nvse/user_defined_functions, used
    // with Call, cached; PrintVar, "fMyVariable: 2.5", "aMyVariable: [2, 4]"].
    use fnv_script::vm::XVal;
    let reader: fnv_game::mod_files::Reader = std::sync::Arc::new(|p: &str| {
        (p.eq_ignore_ascii_case("nvse\\user_defined_functions\\sub\\add.txt")).then(|| b"begin Function { int iA, int iB }\nSetFunctionValue iA + iB\nend\n".to_vec())
    });
    fnv_game::mod_files::set_roots(fnv_game::mod_files::Roots { hosted_from: None, read: Vec::new(), config: None, write: None, lookup: None, reader: Some(reader), archives: Vec::new(), list: None });
    let f = udf_fixture();
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx::default;
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("ref rF\nref rG\nfloat fRes\narray_var aA\nBegin Function {{ }}\n{body}\nEnd\n"), ctx());
    assert_eq!(q(&mut g, "let rF := CompileScript \"Sub/Add.txt\"\nSetFunctionValue (Call rF 2 3)"), Ok(XVal::Num(5.0)));
    // A compiled script is a valid form while it lasts (#4491).
    assert_eq!(q(&mut g, "let rF := CompileScript \"Sub/Add.txt\"\nSetFunctionValue (IsFormValid rF)"), Ok(XVal::Num(1.0)));
    assert_eq!(q(&mut g, "let rF := CompileScript \"sub\\add.txt\"\nlet rG := GetUDFFromFile \"Sub/Add.txt\"\nSetFunctionValue (rF == rG)"), Ok(XVal::Num(1.0)), "cached by path");
    assert_eq!(q(&mut g, "SetFunctionValue (CompileScript \"Sub/None.txt\")"), Ok(XVal::Ref(0)), "no file");
    fnv_game::mod_files::set_roots(fnv_game::mod_files::Roots { hosted_from: None, read: Vec::new(), config: None, write: None, lookup: None, reader: None, archives: Vec::new(), list: None });

    g.console_out.clear();
    q(&mut g, "let fRes := 2.5\nlet aA := ar_List 2, 4\nPrintVar fRes, aA").unwrap();
    assert_eq!(g.console_out, ["fRes: 2.5", "aA: [2, 4]"]);
    g.console_out.clear();
    q(&mut g, "DebugPrintVar fRes").unwrap();
    assert!(g.console_out.is_empty(), "debug mode off");
}

#[test]
fn last_core_setters_change_what_their_getters_read() {
    // [documented: SetHairLength; SetNPCWeight; SetActorBaseFlagsHigh; SetEyes;
    // SetCurrentQuest, as ForceActiveQuest].
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx::default;
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), ctx()).unwrap();
    q(&mut g, "SetActorBaseFlagsHigh 5 PlayerREF");
    assert_eq!(q(&mut g, "SetFunctionValue (GetActorBaseFlagsHigh PlayerREF)"), XVal::Num(5.0));
    q(&mut g, "SetNPCWeight 42 PlayerREF");
    assert_eq!(q(&mut g, "SetFunctionValue (GetNPCWeight PlayerREF)"), XVal::Num(42.0));
    q(&mut g, "SetHairLength 0.5 PlayerREF");
    assert_eq!(q(&mut g, "SetFunctionValue (GetHairLength PlayerREF)"), XVal::Num(0.5));
}

#[test]
fn arrays_that_hold_themselves_do_not_crash() {
    // #1089: Ar_DeepCopy, Ar_DeepEquals and PrintVar on an array that holds itself, and on
    // two arrays that hold each other, end instead of overflowing the stack.
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx::default;
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("array_var aA
array_var aB
array_var aC
array_var aD
float fRes
Begin Function {{ }}
{body}
End
"), ctx());
    let make = "let aA := ar_List 1, 2
let aA[2] := aA
let aB := ar_List 3
let aC := ar_List aB
let aB[1] := aC
";
    // The copy keeps the shape: its third element is the copy itself, not the original, so
    // writing through it changes the copy and leaves the original alone.
    assert_eq!(q(&mut g, &format!("{make}let aC := Ar_DeepCopy aA
let aD := aC[2]
let aD[0] := 9
SetFunctionValue (aC[0] * 100 + aA[0])")), Ok(XVal::Num(901.0)));
    // An array held twice is copied twice: the two copies stay apart.
    assert_eq!(q(&mut g, "let aB := ar_List 5
let aA := ar_List aB, aB
let aC := Ar_DeepCopy aA
let aD := aC[0]
let aD[0] := 7
SetFunctionValue ((aC[1])[0] * 100 + aB[0])"), Ok(XVal::Num(505.0)));
    assert_eq!(q(&mut g, &format!("{make}let aC := Ar_DeepCopy aA
SetFunctionValue (Ar_DeepEquals aC aA)")), Ok(XVal::Num(1.0)));
    assert_eq!(q(&mut g, &format!("{make}SetFunctionValue (Ar_DeepEquals aA aA)")), Ok(XVal::Num(1.0)));
    assert_eq!(q(&mut g, &format!("{make}let aC := ar_List 1, 2, 3
SetFunctionValue (Ar_DeepEquals aC aA)")), Ok(XVal::Num(0.0)));
    g.console_out.clear();
    q(&mut g, &format!("{make}PrintVar aA, aB")).unwrap();
    assert_eq!(g.console_out.len(), 2, "{:?}", g.console_out);
    assert!(g.console_out[0].starts_with("aA: [1, 2, array #"), "{:?}", g.console_out);
    assert!(g.console_out[1].starts_with("aB: [3, [array #"), "{:?}", g.console_out);
}

/// The extender functions a script call still finds unbuilt ("not built yet"), by plugin,
/// from the catalogue: `cargo test --release -p fnv-game --test synthetic -- --ignored
/// extender_census --nocapture`; the list goes to `target/tmp/extender-census.txt`. Each
/// is called once (a name two plugins share, once) with placeholder arguments (0,
/// PlayerREF, "a"), then with fewer when the compiler takes fewer than the catalogue
/// lists; the statements (`let`, `while`, …) are syntax and skipped. A name the compiler
/// does not know at all is listed apart, with what the compiler said.
#[test]
#[ignore]
fn extender_census() {
    const STATEMENTS: [&str; 8] = ["let", "while", "loop", "foreach", "break", "continue", "eval", "search"];
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{root}/schemas/script/functions.json")).unwrap()).unwrap();
    let fs = doc.as_array().or_else(|| doc["functions"].as_array()).unwrap();
    let f = udf_fixture();
    let mut g = game(&f);
    let mut unbuilt: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let mut unknown = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // The catalogue's names the installed extenders do not register, read as text by
    // the compiler (`fnv-script` encodings.json `compiler.not_commands`) [observed:
    // nvse-fourth fose.*, `SetFunctionValue (GetFOSEVersion)` gives the text; lutana1
    // misc.messagedisplaytime]: nothing to build, left out of the count (#1924).
    let enc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{root}/crates/fnv-script/data/encodings.json")).unwrap()).unwrap();
    let not_commands: Vec<String> = enc["compiler"]["not_commands"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).map(str::to_ascii_lowercase).collect();
    let mut not_registered = Vec::new();
    // `NVF_CENSUS_FAMILY=vanilla`: the base game's functions the catalogue has no opcode
    // for (no shipped script calls them). A mod's source reaches them by name, as it
    // reaches an extender's, so an unbuilt one is skipped the same way; the extender
    // count alone did not show them (the owner's run of 2026-11-12: SetDefaultOpen and
    // IsActorDetected "not built yet" in Just Assorted Mods and CASM). The list then goes
    // to `target/tmp/vanilla-census.txt`.
    let family = std::env::var("NVF_CENSUS_FAMILY").unwrap_or_else(|_| "extender".to_string());
    for x in fs.iter().filter(|x| x["family"] == family.as_str() && (family == "extender" || x["opcodes"].is_null())) {
        let n = x["name"].as_str().unwrap_or_default();
        if not_commands.contains(&n.to_ascii_lowercase()) {
            not_registered.push(n.to_string());
            continue;
        }
        if n.is_empty() || !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || STATEMENTS.contains(&n.to_ascii_lowercase().as_str()) || !seen.insert(n.to_ascii_lowercase()) {
            continue;
        }
        let args: Vec<&str> = x["params"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| match p["kind"].as_str() {
                Some("form" | "reference") => "PlayerREF",
                Some("string") => "\"a\"",
                Some("actor_value") => "Luck",
                Some("axis") => "X",
                _ => "0",
            })
            .collect();
        let mut verdict = None;
        for k in (0..=args.len()).rev() {
            let before = g.events.len();
            let r = g.run_query(&format!("Begin Function {{ }}\n{n} {}\nEnd\n", args[..k].join(" ")), fnv_game::game::RunCtx::default());
            let said: Vec<String> = g.events[before..].iter().map(|e| format!("{e:?}")).collect();
            if r.as_ref().is_err_and(|e| e.contains("did not compile")) {
                verdict = Some(Err(said.join(" | ")));
                continue;
            }
            verdict = Some(Ok(said.iter().any(|e| e.contains("not built")) || r.as_ref().is_err_and(|e| e.contains("not built"))));
            break;
        }
        match verdict {
            Some(Ok(true)) => unbuilt.entry(x["origin"].as_str().unwrap_or("?").to_string()).or_default().push(n.to_string()),
            Some(Err(why)) => unknown.push(format!("{n} ({why})")),
            _ => {}
        }
    }
    let mut out = String::new();
    for (o, v) in &unbuilt {
        out += &format!("{o} {}: {}\n", v.len(), v.join(", "));
    }
    out += &format!("not compiled with any argument count {}: {}\n", unknown.len(), unknown.join(", "));
    out += &format!("not registered by the original, left out {}: {}\n", not_registered.len(), not_registered.join(", "));
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/tmp");
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(format!("{dir}/{family}-census.txt"), &out).unwrap();
    println!("unbuilt: {}", unbuilt.values().map(Vec::len).sum::<usize>());
}

/// A plugin with one class, light, climate, grass, texture set and global for the fourth
/// JIP batch (`nvse_jip4`).
fn jip4_fixture() -> Fixture {
    let le32 = |v: u32| v.to_le_bytes().to_vec();
    let f32b = |v: f32| v.to_le_bytes().to_vec();
    // CLAS DATA: four tag skills (i32), flags, services, teaches, max training, 2 unused.
    let clas_data = [le32(42), le32(41), le32(33), le32(44), le32(0b01), le32((1 << 2) | (1 << 17)), vec![0, 50, 0, 0]].concat();
    let clas = record(b"CLAS", 0, 0x0000_0A01, &[sub(b"EDID", z("TestClass")), sub(b"DATA", clas_data), sub(b"ATTR", vec![5, 6, 7, 4, 3, 8, 9])]);
    // LIGH DATA: time, radius, colour, flags (Flicker Slow, bit 6), falloff, FOV, value,
    // weight; FNAM fade.
    let ligh_data = [le32(0), le32(200), vec![177, 120, 60, 0], le32(1 << 6), f32b(1.0), f32b(90.0), le32(0), f32b(0.0)].concat();
    let ligh = record(b"LIGH", 0, 0x0000_0A02, &[sub(b"EDID", z("TestLight")), sub(b"DATA", ligh_data), sub(b"FNAM", f32b(1.25))]);
    // CLMT TNAM: sunrise 30/42, sunset 96/108 (10-minute units), volatility 20, moons
    // byte 0x83 (bit 7, phase length 3).
    let clmt = record(b"CLMT", 0, 0x0000_0A03, &[sub(b"EDID", z("TestClimate")), sub(b"TNAM", vec![30, 42, 96, 108, 20, 0x83])]);
    let gras_data = [vec![30, 0, 90, 0], 50u16.to_le_bytes().to_vec(), vec![0, 0], le32(4), f32b(22.0), f32b(16.0), f32b(0.5), f32b(4.0), vec![0b101, 0, 0, 0]].concat();
    let gras = record(b"GRAS", 0, 0x0000_0A04, &[sub(b"EDID", z("TestGrass")), sub(b"MODL", z("Landscape\\Grass\\Test.nif")), sub(b"DATA", gras_data)]);
    let txst = record(b"TXST", 0, 0x0000_0A05, &[sub(b"EDID", z("TestTextures")), sub(b"TX00", z("a_d.dds")), sub(b"TX02", z("a_m.dds"))]);
    let glob = record(b"GLOB", 0, 0x0000_0A06, &[sub(b"EDID", z("TestRefGlobal")), sub(b"FNAM", vec![b'f']), sub(b"FLTV", 0f32.to_le_bytes().to_vec())]);
    let plugin = [tes4(), top(b"GLOB", &glob), top(b"CLAS", &clas), top(b"LIGH", &ligh), top(b"CLMT", &clmt), top(b"GRAS", &gras), top(b"TXST", &txst)].concat();
    let plugins = vec![Plugin::parse(&plugin, "Jip4.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = schema();
    let (table, j) = catalogue();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Fixture { data, forms, refs }
}

#[test]
fn jip_batch_four_reads_and_sets_record_traits() {
    use fnv_script::vm::XVal;
    let f = jip4_fixture();
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx::default;
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), ctx()).unwrap();
    let n = |g: &mut fnv_game::Game, e: &str| q(g, &format!("SetFunctionValue ({e})"));
    let num = XVal::Num;
    // Classes: the tag skills, then the attributes; flags 0-1 and the services from 2.
    assert_eq!(n(&mut g, "GetClassTraitNumeric TestClass 2"), num(33.0));
    assert_eq!(n(&mut g, "GetClassTraitNumeric TestClass 4"), num(5.0), "strength");
    assert_eq!(n(&mut g, "GetClassTraitNumeric TestClass 10"), num(9.0), "luck");
    assert_eq!(n(&mut g, "GetClassTraitNumeric TestClass 11"), num(0.0), "out of range");
    assert_eq!(n(&mut g, "GetClassFlag TestClass 0"), num(1.0), "playable");
    assert_eq!(n(&mut g, "GetClassFlag TestClass 1"), num(0.0));
    assert_eq!(n(&mut g, "GetClassFlag TestClass 4"), num(1.0), "service bit 2, alcohol");
    assert_eq!(n(&mut g, "GetClassFlag TestClass 19"), num(1.0), "service bit 17, repair");
    assert_eq!(n(&mut g, "SetClassTraitNumeric TestClass 2 34"), num(34.0));
    assert_eq!(n(&mut g, "GetClassTraitNumeric TestClass 2"), num(34.0));
    q(&mut g, "SetClassFlag TestClass 19 0\nSetClassFlag TestClass 1 1");
    assert_eq!((n(&mut g, "GetClassFlag TestClass 19"), n(&mut g, "GetClassFlag TestClass 1"), n(&mut g, "GetClassFlag TestClass 4")), (num(0.0), num(1.0), num(1.0)));
    // Lights.
    assert_eq!(n(&mut g, "GetLightTraitNumeric TestLight 0"), num(200.0));
    assert_eq!(n(&mut g, "GetLightTraitNumeric TestLight 2"), num(120.0), "green");
    assert_eq!(n(&mut g, "GetLightTraitNumeric TestLight 5"), num(90.0));
    assert_eq!(n(&mut g, "GetLightTraitNumeric TestLight 6"), num(1.25), "fade");
    assert_eq!((n(&mut g, "GetLightFlag TestLight 6"), n(&mut g, "GetLightFlag TestLight 3")), (num(1.0), num(0.0)));
    q(&mut g, "SetLightTraitNumeric TestLight 0 512\nSetLightFlag TestLight 3 1");
    assert_eq!((n(&mut g, "GetLightTraitNumeric TestLight 0"), n(&mut g, "GetLightFlag TestLight 3")), (num(512.0), num(1.0)));
    // Climates: minutes from the 10-minute units, the moons byte split.
    assert_eq!(n(&mut g, "GetClimateTraitNumeric TestClimate 0"), num(300.0));
    assert_eq!(n(&mut g, "GetClimateTraitNumeric TestClimate 2"), num(960.0));
    assert_eq!(n(&mut g, "GetClimateTraitNumeric TestClimate 4"), num(20.0));
    assert_eq!((n(&mut g, "GetClimateTraitNumeric TestClimate 5"), n(&mut g, "GetClimateTraitNumeric TestClimate 6")), (num(2.0), num(3.0)));
    q(&mut g, "SetClimateTraitNumeric TestClimate 2 1000\nSetClimateTraitNumeric TestClimate 6 7");
    assert_eq!(n(&mut g, "GetClimateTraitNumeric TestClimate 2"), num(1000.0));
    assert_eq!((n(&mut g, "GetClimateTraitNumeric TestClimate 5"), n(&mut g, "GetClimateTraitNumeric TestClimate 6")), (num(2.0), num(7.0)), "the moons kept");
    // Grass and texture sets.
    assert_eq!(n(&mut g, "GetGrassTraitNumeric TestGrass 0"), num(30.0));
    assert_eq!(n(&mut g, "GetGrassTraitNumeric TestGrass 3"), num(50.0));
    assert_eq!(n(&mut g, "GetGrassTraitNumeric TestGrass 5"), num(22.0));
    assert_eq!(n(&mut g, "GetGrassTraitNumeric TestGrass 9"), num(5.0), "flags bitmask");
    assert_eq!(n(&mut g, "GetGrassModel TestGrass"), XVal::Str("Landscape\\Grass\\Test.nif".into()));
    q(&mut g, "SetGrassTraitNumeric TestGrass 5 30\nSetGrassModel TestGrass \"x.nif\"");
    assert_eq!((n(&mut g, "GetGrassTraitNumeric TestGrass 5"), n(&mut g, "GetGrassModel TestGrass")), (num(30.0), XVal::Str("x.nif".into())));
    assert_eq!(n(&mut g, "GetTextureSetTexture TestTextures 2"), XVal::Str("a_m.dds".into()));
    assert_eq!(n(&mut g, "GetTextureSetTexture TestTextures 1"), XVal::Str(String::new()), "no TX01");
    q(&mut g, "SetTextureSetTexture TestTextures 1 \"n.dds\"");
    assert_eq!(n(&mut g, "GetTextureSetTexture TestTextures 1"), XVal::Str("n.dds".into()));
}

#[test]
fn jip_batch_four_reads_the_game_state() {
    use fnv_script::vm::XVal;
    let f = jip4_fixture();
    let mut g = game(&f);
    let ctx = fnv_game::game::RunCtx::default;
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), ctx()).unwrap();
    let n = |g: &mut fnv_game::Game, e: &str| q(g, &format!("SetFunctionValue ({e})"));
    assert_eq!(n(&mut g, "GetGameDifficulty"), XVal::Num(2.0), "normal");
    q(&mut g, "SetGameDifficulty 4");
    assert_eq!(n(&mut g, "GetGameDifficulty"), XVal::Num(4.0));
    q(&mut g, "SetGameDifficulty 9");
    assert_eq!(n(&mut g, "GetGameDifficulty"), XVal::Num(4.0), "out of range: unchanged");
    assert_eq!(n(&mut g, "GetGlobalRef TestRefGlobal"), XVal::Ref(0));
    q(&mut g, "SetGlobalRef TestRefGlobal TestClass");
    assert_eq!(n(&mut g, "GetGlobalRef TestRefGlobal"), XVal::Ref(0x0000_0A01));
    // The form is the global's own value: its float's bits are the form ID [observed: jip4].
    assert_eq!(n(&mut g, "GetGlobalValue TestRefGlobal"), XVal::Num(f64::from(f32::from_bits(0x0000_0A01))));
    q(&mut g, "PlayerREF.SetGlobalRef TestRefGlobal");
    assert_eq!(n(&mut g, "GetGlobalRef TestRefGlobal"), XVal::Ref(FormId::PLAYER_REF.0), "the calling reference");
    let achr = f64::from(fnv_script::vm::form_type_code(b"ACHR").unwrap());
    assert_eq!(n(&mut g, "PlayerREF.GetRefType"), XVal::Num(achr), "the reference's own type");
    assert_eq!(n(&mut g, "GetPCDetectionState"), XVal::Num(0.0), "no one measured");
    assert_eq!(n(&mut g, "PlayerREF.GetIsPoisoned"), XVal::Num(0.0));
    assert_eq!(n(&mut g, "PlayerREF.GetTeleportDoor"), XVal::Ref(0));
    let before = n(&mut g, "GetSessionTime");
    g.tick();
    let after = n(&mut g, "GetSessionTime");
    assert!(matches!((before, after), (XVal::Num(a), XVal::Num(b)) if b > a), "time goes on");
}

/// A save with the tutorial menu open over the Pip-Boy, a SayToDone and a reference
/// event waiting for the next frame: all come back, and closing the tutorial after the
/// load gives the Pip-Boy back (#1090). A save from before these fields still loads.
#[test]
fn the_tutorial_stack_and_next_frame_work_survive_a_save() {
    use fnv_game::game::menus;
    let f = fixture();
    let mut g = game(&f);
    g.open_menu(menus::STATS);
    g.show_tutorial(FormId(MESSAGE));
    g.say_to_unsaid.push((FormId(ACT_REF), FormId(MESSAGE)));
    g.queue_ref_event(FormId(ACT_REF), 7, Some(FormId::PLAYER_REF));
    let s = g.snapshot();
    let text = serde_json::to_string(&s).unwrap();
    let back: fnv_game::game::GameSnapshot = serde_json::from_str(&text).unwrap();
    assert_eq!(back, s);
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let mut h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert_eq!((h.menu, h.menu_under, h.tutorial.clone()), (Some(menus::TUTORIAL), Some(menus::STATS), vec![FormId(MESSAGE)]));
    assert_eq!(h.say_to_unsaid, g.say_to_unsaid);
    assert_eq!(h.queued_ref_events, g.queued_ref_events);
    assert_eq!(h.close_menu(), Some(menus::TUTORIAL));
    assert_eq!((h.menu, h.menu_under), (Some(menus::STATS), None), "the Pip-Boy is back");
    let mut old: serde_json::Value = serde_json::from_str(&text).unwrap();
    for k in ["menu_under", "tutorial", "say_to_unsaid", "queued_ref_events"] {
        assert!(old.as_object_mut().unwrap().remove(k).is_some(), "{k} is written");
    }
    let old: fnv_game::game::GameSnapshot = serde_json::from_value(old).unwrap();
    let world = World::restore(&f.forms, &f.refs, &old.sim).unwrap();
    let h = Game::restore(&f.data, world, g.cfg, &old).unwrap();
    assert!(h.tutorial.is_empty() && h.menu_under.is_none());
}

/// A topic of three lines in Base.esm (A, B, C) and a patch that adds two lines chained
/// by Previous INFO behind A (X names A, Y names X) and one with PNAM 0 (Z): the added
/// run goes behind A, Z stays at the end (issue #968, `data::follow_previous`).
#[test]
fn lines_a_later_plugin_adds_follow_their_previous_info() {
    const TOPIC: u32 = 0x900;
    let info = |id: u32, pnam: Option<u32>| {
        let mut subs = vec![sub(b"DATA", vec![0, 0, 0, 0]), sub(b"NAM1", z("line"))];
        if let Some(p) = pnam {
            subs.push(sub(b"PNAM", p.to_le_bytes().to_vec()));
        }
        record(b"INFO", 0, id, &subs)
    };
    let base = [
        tes4(),
        top(
            b"DIAL",
            &[record(b"DIAL", 0, TOPIC, &[sub(b"EDID", z("TestTopic"))]), group(TOPIC.to_le_bytes(), 7, &[info(0xA01, None), info(0xA02, None), info(0xA03, None)].concat())].concat(),
        ),
    ]
    .concat();
    let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat();
    let patch_tes4 = record(b"TES4", 0, 0, &[sub(b"HEDR", hedr), sub(b"MAST", z("Base.esm")), sub(b"DATA", vec![0; 8])]);
    // The patch's own forms carry its index (1) in the top byte; the topic is the base's.
    let (x, y, zl) = (0x0100_0B01, 0x0100_0B02, 0x0100_0B03);
    let patch = [
        patch_tes4,
        top(
            b"DIAL",
            &[record(b"DIAL", 0, TOPIC, &[sub(b"EDID", z("TestTopic"))]), group(TOPIC.to_le_bytes(), 7, &[info(x, Some(0xA01)), info(y, Some(x)), info(zl, Some(0))].concat())].concat(),
        ),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&base, "Base.esm".into()).unwrap(), Plugin::parse(&patch, "Patch.esp".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = schema();
    let (table, j) = catalogue();
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    let ids: Vec<u32> = data.topics[&FormId(TOPIC)].iter().map(|f| f.0).collect();
    assert_eq!(ids, vec![0xA01, x, y, 0xA02, 0xA03, zl]);
    assert_eq!(data.lines_moved_by_previous, 2);
    // In one plugin a PNAM changes nothing: the base order is its file order.
    let alone = vec![Plugin::parse(&base, "Base.esm".into()).unwrap()];
    let lo = LoadOrder::new(&alone).unwrap();
    let (table, j) = catalogue();
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    assert_eq!(data.topics[&FormId(TOPIC)], [0xA01, 0xA02, 0xA03].map(FormId));
}

/// `killall` (KillAllActors, #1046): the console word reaches it by alias; every actor of
/// the loaded cell dies, the player does not, and the killer given takes the kills.
#[test]
fn kill_all_actors_kills_the_loaded_cells_actors_but_never_the_player() {
    use fnv_game::functions::console::{lookup_word, ConsoleCtx};
    let f = fixture();
    let mut g = game(&f);
    g.set_loaded([FormId(CELL)].into_iter().collect());
    let (name, run) = lookup_word(&g.data.table, "killall").expect("killall");
    assert_eq!(name, "KillAllActors");
    let lines = run(&mut g, &ConsoleCtx::default(), &["player".to_string()]).unwrap();
    assert_eq!(lines, ["KillAllActors >> 2 killed, 0 essential knocked out"]);
    assert!(g.is_dead(FormId(NPC_REF)) && g.is_dead(FormId(NPC_AUTO_REF)));
    assert!(!g.is_dead(FormId::PLAYER_REF));
    assert_eq!(g.extra[&FormId(NPC_REF)].killer, Some(FormId::PLAYER_REF));
    // Run again: nobody is left to kill.
    assert_eq!(run(&mut g, &ConsoleCtx::default(), &[]).unwrap(), ["KillAllActors >> 0 killed, 0 essential knocked out"]);
}

/// `saq`, `rdf`, `rat` and `zap` (#1046 step 2): reached by their aliases; StartAllQuests
/// starts the quests not running, ResetDialogueFlags forgets the lines said once,
/// ResetAllTerminals forgets the hacking, DeleteReference removes the selection and never
/// the player.
#[test]
fn console_saq_rdf_rat_and_zap() {
    use fnv_game::functions::console::{lookup_word, ConsoleCtx};
    let f = fixture();
    let mut g = game(&f);
    let run = |g: &mut Game, word: &str, ctx: &ConsoleCtx| {
        let (_, run) = lookup_word(&g.data.table, word).unwrap_or_else(|| panic!("{word}"));
        run(g, ctx, &[])
    };
    let none = ConsoleCtx::default();
    g.stop_quest(FormId(QUEST));
    assert!(!g.quest_state(FormId(QUEST)).running);
    let lines = run(&mut g, "saq", &none).unwrap();
    assert!(lines[0].ends_with(&format!("({} in all)", g.data.quests.len())), "{lines:?}");
    assert!(g.quest_state(FormId(QUEST)).running);
    g.said.insert((FormId(NPC_REF), FormId(INFO_A)));
    g.said_day.entry(FormId(NPC_REF)).or_default().insert(FormId(INFO_B), 1.0);
    assert_eq!(run(&mut g, "rdf", &none).unwrap(), ["Dialogue flags reset: 1 lines said once, 1 once a day"]);
    assert!(g.said.is_empty() && g.said_day.is_empty());
    g.terminals.access.insert(FormId(0x7777), fnv_game::terminal::Access::Hacked);
    assert_eq!(run(&mut g, "rat", &none).unwrap(), ["1 terminals reset"]);
    assert!(g.terminals.access.is_empty());
    assert!(run(&mut g, "zap", &none).is_err(), "nothing selected");
    assert!(run(&mut g, "zap", &ConsoleCtx { on: Some(FormId::PLAYER_REF), ..Default::default() }).is_err(), "never the player");
    run(&mut g, "zap", &ConsoleCtx { on: Some(FormId(NPC_REF)), ..Default::default() }).unwrap();
    assert!(g.extra[&FormId(NPC_REF)].marked_for_delete);
}

/// `tlik`, `temo`, `tfow`, `slpm` and `MFG` (#1046 step 3): the toggles are asked of the
/// engine, ToggleFogOfWar says why it does nothing, ShowLockPickMenu asks for a practice
/// lock (Average without a difficulty), MFG keeps one expression at a time, takes a
/// morph off at 0 and forgets the face on reset.
#[test]
fn console_tlik_temo_tfow_slpm_and_mfg() {
    use fnv_game::functions::console::{lookup_word, ConsoleCtx, ConsoleRequest, FaceMfg};
    use fnv_game::functions::toggles::Toggle;
    use fnv_game::lockpick::Level;
    let f = fixture();
    let mut g = game(&f);
    for word in ["tlik", "temo", "ToggleLookIK"] {
        fnv_game::functions::console::call_words(&mut g, None, word, &[], "console").unwrap();
    }
    assert_eq!(g.toggles.asked, [Toggle::LookIk, Toggle::Emotions, Toggle::LookIk]);
    let run = |g: &mut Game, word: &str, ctx: &ConsoleCtx, a: &[&str]| {
        let (_, run) = lookup_word(&g.data.table, word).unwrap_or_else(|| panic!("{word}"));
        run(g, ctx, &a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    };
    let none = ConsoleCtx::default();
    assert!(run(&mut g, "tfow", &none, &[]).unwrap()[0].contains("no fog of war"));
    run(&mut g, "slpm", &none, &[]).unwrap();
    run(&mut g, "slpm", &none, &["5"]).unwrap();
    assert!(run(&mut g, "slpm", &none, &["6"]).is_err());
    assert_eq!(g.console_requests, [ConsoleRequest::Lockpick(Level::Average), ConsoleRequest::Lockpick(Level::VeryHard)]);
    assert!(run(&mut g, "mfg", &none, &["expression", "3", "100"]).is_err(), "nothing selected");
    let on = ConsoleCtx { on: Some(FormId(NPC_REF)), ..Default::default() };
    assert!(run(&mut g, "mfg", &on, &["expression", "3", "100"]).unwrap()[0].ends_with("MFG expression Sad at 100"));
    run(&mut g, "mfg", &on, &["expression", "2", "250"]).unwrap();
    run(&mut g, "mfg", &on, &["phoneme", "0", "25"]).unwrap();
    run(&mut g, "mfg", &on, &["modifier", "16", "-50"]).unwrap();
    assert!(run(&mut g, "mfg", &on, &["modifier", "17", "50"]).is_err());
    let face = &g.mfg[&FormId(NPC_REF)];
    assert_eq!(face.expression, Some((2, 100.0)), "one expression at a time, at most 100");
    assert_eq!((face.phonemes[&0], face.modifiers[&16]), (25.0, -50.0));
    run(&mut g, "mfg", &on, &["phoneme", "0", "0"]).unwrap();
    assert!(g.mfg[&FormId(NPC_REF)].phonemes.is_empty());
    run(&mut g, "mfg", &on, &["reset"]).unwrap();
    assert!(!g.mfg.contains_key(&FormId(NPC_REF)));
    run(&mut g, "mfg", &on, &["modifier", "9", "100"]).unwrap();
    run(&mut g, "mfg", &on, &["modifier", "9", "0"]).unwrap();
    assert!(!g.mfg.contains_key(&FormId(NPC_REF)), "nothing left set: no face kept");
    assert_eq!(FaceMfg::default().expression, None);
}

#[test]
fn console_plm_clm_s1st_setvel_and_the_small_ones() {
    use fnv_game::functions::console::{lookup_word, ConsoleCtx, ConsoleRequest};
    let f = fixture();
    let mut g = game(&f);
    let run = |g: &mut Game, word: &str, ctx: &ConsoleCtx, a: &[&str]| {
        let (_, run) = lookup_word(&g.data.table, word).unwrap_or_else(|| panic!("{word}"));
        run(g, ctx, &a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    };
    let none = ConsoleCtx::default();
    run(&mut g, "plm", &none, &["10", "25", "0", "TestLocation"]).unwrap();
    run(&mut g, "plm", &none, &["1.5", "-2", "3"]).unwrap();
    assert!(run(&mut g, "plm", &none, &["1", "2", "3", "x", "10"]).is_err(), "colours 0 to 9");
    assert!(run(&mut g, "plm", &none, &["1", "2"]).is_err());
    run(&mut g, "clm", &none, &[]).unwrap();
    run(&mut g, "s1st", &none, &[]).unwrap();
    assert!(run(&mut g, "SetVel", &none, &["z", "300"]).is_err(), "nothing selected");
    let on = ConsoleCtx { on: Some(FormId::PLAYER_REF), ..Default::default() };
    assert!(run(&mut g, "SetVel", &on, &["w", "300"]).is_err());
    run(&mut g, "SetVel", &on, &["z", "300"]).unwrap();
    assert_eq!(
        g.console_requests,
        [
            ConsoleRequest::PlaceMarker { pos: [10.0, 25.0, 0.0], name: "TestLocation".into(), color: 0 },
            ConsoleRequest::PlaceMarker { pos: [1.5, -2.0, 3.0], name: "2 -2 3".into(), color: 0 },
            ConsoleRequest::ClearMarkers,
            ConsoleRequest::Show1stPerson,
            ConsoleRequest::SetVel { on: FormId::PLAYER_REF, axis: 'Z', speed: 300.0 },
        ]
    );
    assert_eq!(run(&mut g, "tst", &none, &["TestGlobal"]).unwrap(), ["TestGlobal = 5"]);
    assert!(run(&mut g, "Show", &none, &["NoSuchGlobal"]).is_err());
    assert!(run(&mut g, "prc", &none, &[]).unwrap()[0].starts_with("Persistent references: "));
    run(&mut g, "ClearScreenBlood", &none, &[]).unwrap();
    assert_eq!(g.ui.splatter.clears, 1);
    run(&mut g, "TestRaceSexTerimnal", &none, &[]).unwrap();
    assert_eq!(g.race_menu, fnv_game::game::RaceMenu::Creator);
}

/// A co-op guest takes a quest's reward the host completed: the XP as
/// RewardXP gives it. On a host, caps that reach the player count towards the reward of
/// a quest completed around then (`coop_rewards`). (The fixture has no caps form, so the
/// caps go in as the event AddItem emits.)
#[test]
fn a_guest_takes_the_hosts_quest_reward() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let caps = fnv_game::coop_rewards::CAPS;
    let xp0 = g.player.xp;
    assert_eq!(g.take_coop_reward(250.0, 0).unwrap(), 250.0);
    assert_eq!(g.player.xp, xp0 + 250.0);
    let paid = |g: &mut Game| g.emit(fnv_game::trace::EventKind::ItemAdded { to: FormId::PLAYER_REF, item: caps, count: 40, silent: false });
    // Off (a guest, a game alone): nothing kept.
    paid(&mut g);
    assert!(g.coop_rewards.due(u64::MAX).is_empty());
    // A host: the caps a script gives count.
    g.coop_rewards.on = true;
    paid(&mut g);
    let frame = g.frame;
    g.coop_rewards.completed(frame, FormId(QUEST));
    let due = g.coop_rewards.due(frame + fnv_game::coop_rewards::AFTER);
    assert_eq!(due, [fnv_game::coop_rewards::QuestReward { quest: FormId(QUEST), xp: 0.0, caps: 40 }]);
}

// ---- TTW's extender functions (issue #1408, crate::nvse_ttw) -------------------------

/// A cell with one reference of a scripted activator whose GameMode block waits
/// (ScriptWait 3) halfway, and the global TestGlobal.
fn wait_fixture() -> Fixture {
    const GLOB: u32 = 0x800;
    const SCPT: u32 = 0x801;
    const ACTI: u32 = 0x802;
    const CELL_: u32 = 0x803;
    const REF: u32 = 0x804;
    let (table, _) = catalogue();
    struct Names;
    impl fnv_script::compile::Env for Names {
        fn form(&self, e: &str) -> Option<fnv_script::compile::FormInfo> {
            (e.eq_ignore_ascii_case("testglobal")).then_some(fnv_script::compile::FormInfo { form: 0x800, sig: *b"GLOB" })
        }
        fn info(&self, _: u32) -> Option<fnv_script::compile::FormInfo> {
            None
        }
        fn editor_id(&self, _: u32) -> Option<String> {
            None
        }
        fn local_form(&self, _: u32) -> Option<u32> {
            None
        }
        fn script_vars(&self, _: u32) -> Option<Vec<fnv_script::VarDecl>> {
            None
        }
    }
    let src = "scn TestWaitScript\nint n\nBegin GameMode\n  set n to n + 1\n  if n == 1\n    set TestGlobal to 10\n    ScriptWait 3\n    set TestGlobal to 20\n  endif\nEnd\n";
    let c = fnv_script::compile::compile(src, &table, &Names, None);
    assert!(c.errors().next().is_none(), "{:?}", c.diags);
    let vars: Vec<(u32, String, u8)> = c.variables.iter().map(|v| (v.index, v.name.clone(), u8::from(v.ty == fnv_script::VarType::Int))).collect();
    let vars: Vec<(u32, &str, u8)> = vars.iter().map(|(i, n, t)| (*i, n.as_str(), *t)).collect();
    let refs: Vec<u32> = c.refs.iter().filter_map(|r| if let fnv_script::unit::RefSlot::Form(f) = r { Some(*f) } else { None }).collect();
    let mut subs = unit(0, &c.bytes, &vars, &refs);
    subs.push(sub(b"SCTX", src.as_bytes().to_vec()));
    let scpt = record(b"SCPT", 0, SCPT, &[vec![sub(b"EDID", z("TestWaitScript"))], subs].concat());
    let glob = record(b"GLOB", 0, GLOB, &[sub(b"EDID", z("TestGlobal")), sub(b"FNAM", vec![b'f']), sub(b"FLTV", 0f32.to_le_bytes().to_vec())]);
    let acti = record(b"ACTI", 0, ACTI, &[sub(b"EDID", z("TestWaitActivator")), sub(b"SCRI", SCPT.to_le_bytes().to_vec())]);
    let cell = record(b"CELL", 0, CELL_, &[sub(b"EDID", z("TestWaitCell"))]);
    let refr = record(b"REFR", 0, REF, &[sub(b"EDID", z("TestWaitREF")), sub(b"NAME", ACTI.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let children = group(CELL_.to_le_bytes(), 6, &group(CELL_.to_le_bytes(), 9, &refr));
    let bytes = [
        tes4(),
        top(b"GLOB", &glob),
        top(b"SCPT", &scpt),
        top(b"ACTI", &acti),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat()))),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "Wait.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = schema();
    let (table, j) = catalogue();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Fixture { data, forms, refs }
}

#[test]
fn script_wait_holds_the_rest_of_a_block_for_its_cycles() {
    // [documented: ScriptWait; IsScriptWaiting]: the block stops at the call, is skipped
    // while it waits (its first line does not run either), and goes on after the call in
    // the third cycle after it.
    let f = wait_fixture();
    let mut g = game(&f);
    g.new_game();
    g.set_loaded([FormId(0x803)].into_iter().collect());
    let global = |g: &Game| g.world.global(FormId(0x800)).unwrap();
    let key = InstKey::Ref { reference: FormId(0x804) };
    g.tick();
    assert_eq!(global(&g), 10.0);
    assert_eq!(g.var(key, 1), Value::Num(1.0));
    let waiting = |g: &mut Game| {
        g.run_source("let TestGlobal := TestWaitREF.IsScriptWaiting", Default::default()).unwrap();
        global(g)
    };
    assert_eq!(waiting(&mut g), 1.0);
    g.run_source("set TestGlobal to 10", Default::default()).unwrap();
    g.tick();
    g.tick();
    assert_eq!(global(&g), 10.0, "still waiting");
    assert_eq!(g.var(key, 1), Value::Num(1.0), "the waiting block is skipped whole");
    g.tick();
    assert_eq!(global(&g), 20.0, "the rest of the block ran");
    assert_eq!(g.var(key, 1), Value::Num(1.0));
    assert_eq!(waiting(&mut g), 0.0);
    // The block runs from its start again.
    g.tick();
    assert_eq!(g.var(key, 1), Value::Num(2.0));
    // A result script does not wait [documented: not in result scripts].
    g.run_source("set TestGlobal to 1\nScriptWait 5\nset TestGlobal to 2", Default::default()).unwrap();
    assert_eq!(global(&g), 2.0);
}

#[test]
fn ash_pile_activators_must_be_marked_before_attach_ash_pile_ex() {
    // [documented: MarkActivatorAshPile; AttachAshPileEx]
    let f = wait_fixture();
    let mut g = game(&f);
    g.new_game();
    let pile = |g: &Game| g.extra.get(&FormId(0x804)).and_then(|e| e.ash_pile);
    g.run_source("TestWaitREF.AttachAshPileEx TestWaitActivator", Default::default()).unwrap();
    assert_eq!(pile(&g), None, "not marked: no pile");
    g.run_source("MarkActivatorAshPile TestWaitActivator\nTestWaitREF.AttachAshPileEx TestWaitActivator", Default::default()).unwrap();
    let p = pile(&g).expect("a pile");
    assert_eq!(g.ash_pile_source(p), Some(FormId(0x804)));
    // The mark lasts the session, across a load.
    let s = g.take_session();
    let mut g2 = game(&f);
    g2.new_game();
    g2.put_session(s);
    g2.run_source("TestWaitREF.AttachAshPileEx TestWaitActivator", Default::default()).unwrap();
    assert!(g2.extra.get(&FormId(0x804)).and_then(|e| e.ash_pile).is_some());
}

#[test]
fn jip_menu_key_crafting_and_discovery_events_reach_their_handlers() {
    // [documented: SetOnMenuOpenEventHandler, SetOnMenuCloseEventHandler (a menu ID, or
    // any menu), SetOnKeyUpEventHandler, SetOnCraftingEventHandler,
    // SetOnLocationDiscoverEventHandler]
    let f = udf_fixture();
    let mut g = game(&f);
    let run = |g: &mut Game<'_>, src: &str| g.run_source(src, Default::default()).expect("compiles");
    let global = |g: &Game| g.world.global(FormId(0x0000_0800)).unwrap();
    run(&mut g, "SetOnMenuOpenEventHandler (begin function {int i}\nlet TestGlobal := i\nend) 1 1060");
    g.open_menu(1002);
    assert_eq!(global(&g), 0.0, "another menu");
    g.close_menu();
    g.open_menu(1060);
    assert_eq!(global(&g), 1060.0);
    run(&mut g, "SetOnMenuCloseEventHandler (begin function {int i}\nlet TestGlobal := i + 1\nend) 1");
    g.close_menu();
    assert_eq!(global(&g), 1061.0, "any menu that closes");
    run(&mut g, "SetOnKeyUpEventHandler (begin function {int k}\nlet TestGlobal := k * 100\nend) 1 3");
    g.input.held.insert(3);
    g.tick();
    assert_eq!(global(&g), 1061.0, "pressed, not let go");
    g.input.held.remove(&3);
    g.tick();
    assert_eq!(global(&g), 300.0);
    run(&mut g, "SetOnCraftingEventHandler (begin function {ref r, int q}\nlet TestGlobal := q\nend) 1");
    g.item_crafted(FormId(0x0000_0800), 25);
    assert_eq!(global(&g), 25.0);
    run(&mut g, "SetOnLocationDiscoverEventHandler (begin function {ref m}\nlet TestGlobal := 77\nend) 1");
    g.challenge_marker_found(FormId(0x0000_0800));
    assert_eq!(global(&g), 77.0);
}

#[test]
fn the_mods_page_sends_the_start_menus_open_and_close() {
    // MCM Extender adds its menus from a handler for the start menu, 1013 [observed:
    // its OnPauseMenuOpen.gek; Begin Again list 2026-11-13].
    let f = udf_fixture();
    let mut g = game(&f);
    let run = |g: &mut Game<'_>, src: &str| g.run_source(src, Default::default()).expect("compiles");
    let global = |g: &Game| g.world.global(FormId(0x0000_0800)).unwrap();
    run(&mut g, "SetOnMenuOpenEventHandler (begin function {int i}\nlet TestGlobal := i + MenuMode 1013\nend) 1 1013");
    run(&mut g, "SetOnMenuCloseEventHandler (begin function {int i}\nlet TestGlobal := 0 - i\nend) 1 1013");
    g.page_menu_event(1013, true);
    assert_eq!(global(&g), 1014.0, "the handler runs with MenuMode 1013 true");
    assert_eq!(g.menu, None, "no menu is opened");
    g.page_menu_event(1013, false);
    assert_eq!(global(&g), -1013.0);
}

#[test]
fn extra_misc_stats_music_states_eaten_corpses_and_terminal_art() {
    let f = udf_fixture();
    let mut g = game(&f);
    let run = |g: &mut Game<'_>, src: &str| g.run_source(src, Default::default()).expect("compiles");
    let global = |g: &Game| g.world.global(FormId(0x0000_0800)).unwrap();
    // A stat must be initialised first [documented: InitExtraMiscStat; observed: -1 after
    // ModExtraMiscStat alone].
    run(&mut g, "ModExtraMiscStat \"Bobbleheads Found\" 1\nlet TestGlobal := GetExtraMiscStat \"Bobbleheads Found\"");
    assert_eq!(global(&g), -1.0);
    run(&mut g, "InitExtraMiscStat \"Bobbleheads Found\"\nlet TestGlobal := GetExtraMiscStat \"Bobbleheads Found\"");
    assert_eq!(global(&g), 0.0);
    run(&mut g, "ModExtraMiscStat \"Bobbleheads Found\" 2\nInitExtraMiscStat \"Bobbleheads Found\"\nlet TestGlobal := GetExtraMiscStat \"Bobbleheads Found\"");
    assert_eq!(global(&g), 2.0);
    // Kept in the save [documented: ModExtraMiscStat].
    assert_eq!(g.snapshot().extra_misc_stats.get("bobbleheads found"), Some(&2.0));
    // SetMusicState's requests reach the engine; other numbers do nothing.
    run(&mut g, "SetMusicState 1\nSetMusicState 7\nSetMusicState 0");
    assert_eq!(g.take_music_states(), vec![1, 0]);
    // DonnerReedKuruParty marks the calling actor's corpse eaten and back.
    let on = fnv_game::game::RunCtx { self_ref: Some(FormId(0x0000_0901)), ..Default::default() };
    g.run_source("DonnerReedKuruParty 1", on).unwrap();
    assert!(g.extra[&FormId(0x0000_0901)].eaten);
    g.run_source("DonnerReedKuruParty 0", on).unwrap();
    assert!(!g.extra[&FormId(0x0000_0901)].eaten);
    // SetTerminalUIModel: a path, and back to the default without one.
    run(&mut g, "SetTerminalUIModel TestAdd \"Terminals\\TerminalInterface02.NIF\"");
    assert_eq!(g.terminal_ui_model(FormId(0x0000_0901)), Some("Terminals\\TerminalInterface02.NIF"));
    run(&mut g, "SetTerminalUIModel TestAdd");
    assert_eq!(g.terminal_ui_model(FormId(0x0000_0901)), None);
}

#[test]
fn planted_forms_remap_and_missing_dependencies_are_dropped() {
    use fnv_game::game::GameSnapshot;
    use fnv_game::pickpocket::{Explosive, Planted};
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let victim = FormId(NPC_REF);
    g.world.get(victim).unwrap();
    let mut snap = g.snapshot();
    let extra = FormId(0x01000800);
    let base_gone = FormId(0xFF000090);
    let cell_gone = FormId(0xFF000091);
    let mut reference = snap.sim.refs[&victim].clone();
    reference.base = extra;
    snap.sim.refs.insert(base_gone, reference);
    let mut reference = snap.sim.refs[&victim].clone();
    reference.cell = Some(extra);
    snap.sim.refs.insert(cell_gone, reference);
    let plant = Planted { victim, item: FormId(ACTIVATOR), at: 12.5, blast: Explosive { damage: 3.0, resist: Some(19), enchantment: Some(FormId(ACTIVATOR)) }, seen_by: vec![victim, extra, base_gone, cell_gone] };
    snap.planted = vec![
        plant.clone(),
        Planted { victim: extra, ..plant.clone() },
        Planted { item: extra, ..plant.clone() },
        Planted { blast: Explosive { enchantment: Some(extra), ..plant.blast }, ..plant.clone() },
        Planted { victim: base_gone, ..plant.clone() },
        Planted { victim: cell_gone, ..plant.clone() },
    ];
    let entry = |name: &str| fnv_save::PluginEntry { name: name.into(), size: 0 };
    let before = [entry("Test.esm"), entry("Extra.esm")];
    let after = [entry("Extra.esm"), entry("Test.esm")];
    let mut value = serde_json::to_value(&snap).unwrap();
    fnv_save::remap_json(&mut value, &fnv_save::PluginRemap::new(&before, &after));
    let mut moved: GameSnapshot = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(moved.drop_gone(|id| fnv_save::is_gone(id.0)), 0);
    assert_eq!(moved.planted[0].victim, FormId(NPC_REF | 0x01000000));
    assert_eq!(moved.planted[0].item, FormId(ACTIVATOR | 0x01000000));
    assert_eq!(moved.planted[0].blast.enchantment, Some(FormId(ACTIVATOR | 0x01000000)));
    assert_eq!(moved.planted[0].seen_by, vec![FormId(NPC_REF | 0x01000000), FormId(0x800), base_gone, cell_gone]);
    assert_eq!(moved.planted[0].at, 12.5);
    // Moving the plugin back restores every serialized field exactly.
    fnv_save::remap_json(&mut value, &fnv_save::PluginRemap::new(&after, &before));
    let back: GameSnapshot = serde_json::from_value(value).unwrap();
    assert_eq!(back, snap);
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert_eq!(h.pickpocket.planted, snap.planted);
    assert!(h.pickpocket.blast_witnesses.is_empty());

    let mut value = serde_json::to_value(&snap).unwrap();
    fnv_save::remap_json(&mut value, &fnv_save::PluginRemap::new(&before, &[entry("Test.esm")]));
    let mut back: GameSnapshot = serde_json::from_value(value).unwrap();
    assert_eq!(back.drop_gone(|id| fnv_save::is_gone(id.0)), 7, "two removed references and five plants");
    assert_eq!(back.planted, vec![Planted { seen_by: vec![victim], ..plant }]);
    assert!(!back.sim.refs.contains_key(&base_gone) && !back.sim.refs.contains_key(&cell_gone));
    let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
    let h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
    assert_eq!(h.pickpocket.planted, back.planted);
    // Cleanup also handles a gone witness supplied directly in a record field.
    back.planted[0].seen_by.push(extra);
    assert_eq!(back.drop_gone(|id| id == extra), 0);
    assert_eq!(back.planted[0].seen_by, vec![victim]);
}

/// An activator opens and closes like a door: SetOpenState and its default activation
/// change its open state (GetOpenState) and ask for its "Open" or "Close" sequence, as
/// PlayGroup asks one (TTW's Overseer desk and vault door, issues #2231, #2232). Setting
/// the state it has asks for nothing.
#[test]
fn an_activator_plays_open_and_close_with_its_open_state() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let r = FormId(ACT_REF);
    assert!(!g.world.get(r).unwrap().open);
    g.run_source("TestActivatorREF.SetOpenState 1", Default::default()).unwrap();
    assert!(g.world.get(r).unwrap().open);
    assert_eq!(g.anims.asked(r).map(|(_, n)| n), Some("Open"));
    let asked = g.anims.requests.len();
    g.run_source("TestActivatorREF.SetOpenState 1", Default::default()).unwrap();
    assert_eq!(g.anims.requests.len(), asked, "already open: nothing asked");
    // Its default activation (a script's Activate, the vault door's pod) closes it again.
    g.default_activation(FormId::PLAYER_REF, r, false).unwrap();
    assert!(!g.world.get(r).unwrap().open);
    assert_eq!(g.anims.asked(r).map(|(_, n)| n), Some("Close"));
}

/// The plugins whose functions the engine builds answer as installed, the others do not
/// (docs/spec-notes/ext-version.md §5): a mod's start-up check passes for what is here.
#[test]
fn built_in_plugins_report_installed() {
    use fnv_script::vm::XVal;
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("Begin Function {{ }}\nSetFunctionValue ({body})\nEnd\n"), Default::default());
    for name in ["JIP NVSE Plugin", "JohnnyGuitarNVSE", "ShowOffNVSE Plugin", "SUP NVSE Plugin", "JIP PP LN", "CUI", "AnhNVSE", "Extensions for NVSE", "UI Organizer Plugin", "hNVSE", "Improved Console", "sup nvse plugin"] {
        assert_eq!(q(&mut g, &format!("IsPluginInstalled \"{name}\"")), Ok(XVal::Num(1.0)), "{name}");
        let Ok(XVal::Num(v)) = q(&mut g, &format!("GetPluginVersion \"{name}\"")) else { panic!("{name}") };
        assert!(v > 0.0, "{name}: {v}");
    }
    // A version a mod asks for "at least" is met where none was read in the original.
    assert_eq!(q(&mut g, "GetPluginVersion \"SUP NVSE Plugin\" >= 250"), Ok(XVal::Num(1.0)));
    assert_eq!(q(&mut g, "GetPluginVersion \"CUI\""), Ok(XVal::Num(120.0)));
    assert_eq!(q(&mut g, "GetPluginVersion \"Improved Console\""), Ok(XVal::Num(3.0)));
    // No script functions; the engine does their work itself (#4491).
    assert_eq!(q(&mut g, "GetPluginVersion \"Engine Optimizations\""), Ok(XVal::Num(100.0)));
    assert_eq!(q(&mut g, "GetPluginVersion \"HighResLocalMaps\""), Ok(XVal::Num(12.0)));
    for name in ["yUI", "NVTF", "No Such Plugin"] {
        assert_eq!(q(&mut g, &format!("IsPluginInstalled \"{name}\"")), Ok(XVal::Num(0.0)), "{name}");
        assert_eq!(q(&mut g, &format!("GetPluginVersion \"{name}\"")), Ok(XVal::Num(-1.0)), "{name}");
    }
}

/// The test aid's list of what scripts ask about plugins (`fnv_game::plugin_questions`):
/// nothing is kept by default; switched on, each question comes once with its answer.
#[test]
fn plugin_questions_are_kept_only_when_asked_for() {
    let f = udf_fixture();
    let mut g = game(&f);
    let q = |g: &mut fnv_game::Game, body: &str| g.run_query(&format!("Begin Function {{ }}\nSetFunctionValue ({body})\nEnd\n"), Default::default()).unwrap();
    q(&mut g, "IsPluginInstalled \"yUI\"");
    assert!(g.plugin_questions.is_none());
    g.plugin_questions = Some(Default::default());
    for body in ["IsPluginInstalled \"yUI\"", "IsPluginInstalled \"yUI\"", "GetPluginVersion \"JIP NVSE Plugin\"", "IsDLLLoaded \"no_such\"", "IsModLoaded \"Nothing.esp\"", "GetModIndex \"Nothing.esp\""] {
        q(&mut g, body);
    }
    let got = g.plugin_questions.as_mut().unwrap().take();
    let read: Vec<(&str, &str, f64, bool)> = got.iter().map(|x| (x.function.as_str(), x.name.as_str(), x.answer, x.absent())).collect();
    assert_eq!(
        read,
        [("isplugininstalled", "yUI", 0.0, true), ("getpluginversion", "JIP NVSE Plugin", 57.3, false), ("isdllloaded", "no_such", 0.0, true), ("ismodloaded", "Nothing.esp", 0.0, true), ("getmodindex", "Nothing.esp", 255.0, true)]
    );
}

#[test]
fn a_clone_of_an_item_goes_into_a_container_by_its_own_id() {
    // MCM marks each mod it listed by adding it to a container; MCM Extender's menus are
    // temporary clones of an item [observed: MCM's script source, MCM Extender's
    // OnPauseMenuOpen.gek; Begin Again list 2026-11-13].
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    let src = "ref r
let r := TempCloneForm TestWeapon
TestNpcREF.AddItem r 1
set TestGlobal to TestNpcREF.GetItemCount r * 10 + TestNpcREF.GetItemCount TestWeapon";
    g.run_source(src, Default::default()).expect("runs");
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 10.0, "the clone counted, the weapon not");
}

#[path = "common/jip_callback_clock.rs"]
mod jip_callback_clock_tests;
