//! A co-op guest's world reads the host's story (`fnv_game::coop::HostStory`; design): a gate whose OnActivate asks a quest's
//! variable and stage, on a synthetic plugin built in memory (no game data). The scripts
//! are compiled from source by NVForever's compiler.

use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::coop::HostStory;
use fnv_game::game::{ActMode, Game, GameConfig, InstKey};
use fnv_game::GameData;
use fnv_script::compile::{compile, Env, FormInfo};
use fnv_script::unit::{RefSlot, VarType};
use fnv_script::vm::Value;
use fnv_script::{FunctionTable, VarDecl};
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};

const QUEST: u32 = 0x900;
const QUEST_SCRIPT: u32 = 0x901;
const GATE_SCRIPT: u32 = 0x902;
const GATE: u32 = 0x903;
const CELL: u32 = 0x200;
const GATE_REF: u32 = 0x300;

const QUEST_SRC: &str = "scn TQScript\nshort iPapers\n";
/// As Long 15's gate asks it: the quest's variable, its stage and whether it runs.
const GATE_SRC: &str = "scn TGateScript\nshort nOpen\nshort nStage\nshort nRunning\nbegin OnActivate player\n  set nOpen to 0\n  if TQ.iPapers == 1\n    set nOpen to 1\n  endif\n  set nStage to GetStage TQ\n  set nRunning to GetQuestRunning TQ\nend\n";

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

/// The names the two scripts use.
struct Names;

impl Env for Names {
    fn form(&self, editor_id: &str) -> Option<FormInfo> {
        match editor_id.to_ascii_lowercase().as_str() {
            "tq" => Some(FormInfo { form: QUEST, sig: *b"QUST" }),
            "player" => Some(FormInfo { form: 0x14, sig: *b"REFR" }),
            _ => None,
        }
    }
    fn info(&self, form: u32) -> Option<FormInfo> {
        match form {
            QUEST => Some(FormInfo { form, sig: *b"QUST" }),
            0x14 => Some(FormInfo { form, sig: *b"REFR" }),
            _ => None,
        }
    }
    fn editor_id(&self, form: u32) -> Option<String> {
        (form == QUEST).then(|| "TQ".to_string())
    }
    fn local_form(&self, local: u32) -> Option<u32> {
        Some(local)
    }
    fn script_vars(&self, form: u32) -> Option<Vec<VarDecl>> {
        (form == QUEST).then(|| vec![VarDecl { index: 1, name: "iPapers".into(), ty: VarType::Int }])
    }
}

/// SCHR, SCDA, the variables and references, and SCTX of a unit compiled from `src`.
fn unit(table: &FunctionTable, kind: u16, src: &str) -> Vec<Subrecord> {
    let c = compile(src, table, &Names, None);
    assert!(c.errors().next().is_none(), "{src}: {:?}", c.diags);
    let mut schr = vec![0u8; 4];
    schr.extend((c.refs.len() as u32).to_le_bytes());
    schr.extend((c.bytes.len() as u32).to_le_bytes());
    schr.extend((c.variables.len() as u32).to_le_bytes());
    schr.extend(kind.to_le_bytes());
    schr.extend(1u16.to_le_bytes());
    let mut out = vec![sub(b"SCHR", schr), sub(b"SCDA", c.bytes.clone()), sub(b"SCTX", src.as_bytes().to_vec())];
    for v in &c.variables {
        let mut slsd = v.index.to_le_bytes().to_vec();
        slsd.extend([0u8; 12]);
        slsd.push(u8::from(matches!(v.ty, VarType::Int)));
        slsd.extend([0u8; 7]);
        out.push(sub(b"SLSD", slsd));
        out.push(sub(b"SCVR", z(&v.name)));
    }
    for r in &c.refs {
        if let RefSlot::Form(f) = r {
            out.push(sub(b"SCRO", f.to_le_bytes().to_vec()));
        }
    }
    out
}

fn plugin(table: &FunctionTable) -> Vec<u8> {
    let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x1000u32.to_le_bytes()].concat();
    let tes4 = record(b"TES4", 1, 0, &[sub(b"HEDR", hedr)]);
    let scpt = [
        record(b"SCPT", 0, QUEST_SCRIPT, &[vec![sub(b"EDID", z("TQScript"))], unit(table, 1, QUEST_SRC)].concat()),
        record(b"SCPT", 0, GATE_SCRIPT, &[vec![sub(b"EDID", z("TGateScript"))], unit(table, 0, GATE_SRC)].concat()),
    ]
    .concat();
    // The quest: start game enabled; stage 20 (no result).
    let qust = record(
        b"QUST",
        0,
        QUEST,
        &[
            sub(b"EDID", z("TQ")),
            sub(b"SCRI", QUEST_SCRIPT.to_le_bytes().to_vec()),
            sub(b"DATA", [vec![1u8, 0, 0, 0], 5.0f32.to_le_bytes().to_vec()].concat()),
            sub(b"INDX", 20i16.to_le_bytes().to_vec()),
            sub(b"QSDT", vec![0]),
        ],
    );
    let acti = record(b"ACTI", 0, GATE, &[sub(b"EDID", z("TGate")), sub(b"SCRI", GATE_SCRIPT.to_le_bytes().to_vec())]);
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TCell"))]);
    let gate = record(b"REFR", 0, GATE_REF, &[sub(b"EDID", z("TGateREF")), sub(b"NAME", GATE.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &gate));
    [
        tes4,
        top(b"SCPT", &scpt),
        top(b"ACTI", &acti),
        top(b"QUST", &qust),
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
    let (table, j) = catalogue();
    let bytes = plugin(&table);
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

fn quest() -> FormId {
    FormId(QUEST)
}

fn gate() -> FormId {
    FormId(GATE_REF)
}

/// The quest variable's index, and the gate's three (nOpen, nStage, nRunning).
fn ix(f: &Fixture, script: u32, name: &str) -> u16 {
    f.data.script(FormId(script)).and_then(|s| s.var_index(name)).unwrap_or_else(|| panic!("{name}"))
}

/// The host's story with the papers shown (the guard talked to) and stage 20 done.
fn hosts(f: &Fixture) -> HostStory {
    let mut host = game(f);
    host.store(InstKey::Quest { quest: quest() }, Some(FormId(QUEST_SCRIPT)), ix(f, QUEST_SCRIPT, "iPapers"), Value::Num(1.0), "test", true);
    host.set_stage(quest(), 20);
    host.story_view()
}

/// Activate the gate by hand; its (nOpen, nStage, nRunning).
fn use_gate(f: &Fixture, g: &mut Game) -> (f64, f64, f64) {
    g.activate(FormId::PLAYER_REF, gate(), ActMode::Hand).unwrap();
    let v = |name| g.var(InstKey::Ref { reference: gate() }, ix(f, GATE_SCRIPT, name)).num();
    (v("nOpen"), v("nStage"), v("nRunning"))
}

#[test]
fn the_hosts_story_has_its_quest_variables_and_stages() {
    let f = fixture();
    let s = hosts(&f);
    assert_eq!(s.vars.get(&(quest(), ix(&f, QUEST_SCRIPT, "iPapers"))), Some(&Value::Num(1.0)));
    let q = &s.quests[&quest()];
    assert_eq!((q.stage, q.running, q.ended), (20, true, false));
    assert_eq!(q.done, vec![20]);
}

#[test]
fn a_guests_gate_opens_on_the_hosts_story() {
    let f = fixture();
    let story = hosts(&f);
    let mut g = game(&f);
    g.coop_guest = true;
    // Before the host's story comes: the guest's own quest (papers never shown).
    assert_eq!(use_gate(&f, &mut g), (0.0, 0.0, 1.0));
    g.coop_host_story = story;
    assert_eq!(use_gate(&f, &mut g), (1.0, 20.0, 1.0));
    // Read, not written: the guest's own quest is as it was.
    assert_eq!(g.var(InstKey::Quest { quest: quest() }, ix(&f, QUEST_SCRIPT, "iPapers")), Value::Num(0.0));
    assert_eq!(g.quest_state(quest()).stage(), 0);
}

#[test]
fn a_game_that_is_no_guest_reads_its_own_story() {
    let f = fixture();
    let story = hosts(&f);
    let mut g = game(&f);
    g.coop_host_story = story;
    assert_eq!(use_gate(&f, &mut g), (0.0, 0.0, 1.0));
}

#[test]
fn a_quest_the_host_did_not_send_is_the_guests_own() {
    let f = fixture();
    let mut g = game(&f);
    g.coop_guest = true;
    g.coop_host_story = HostStory::default();
    g.store(InstKey::Quest { quest: quest() }, Some(FormId(QUEST_SCRIPT)), ix(&f, QUEST_SCRIPT, "iPapers"), Value::Num(1.0), "test", true);
    assert_eq!(use_gate(&f, &mut g), (1.0, 0.0, 1.0));
}
