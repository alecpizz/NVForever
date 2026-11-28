//! A co-op guest buys a doctor's services and they act on the guest (`fnv_game::coop_services`), over FalloutNV.esm's own doctors:
//! - the doctors, their quests and their service lines are found in the data;
//! - a guest is offered the heal and the rad removal;
//! - in a replication guest's game, the rad removal line takes the guest's caps and
//!   clears the guest's rads; the heal line takes its caps, and the doctor's own script,
//!   run in the guest's game, heals the guest some seconds later;
//! - what the guest paid is its own, not the host's story: nothing of it is undone in
//!   its save.
//!
//! Needs the game data, never in the repository: set `FNVLAB_DATA_DIR` to the game's
//! `Data` folder. Without it the test prints why it was skipped and passes.

use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::functions::console::call_words;
use fnv_game::game::{Game, GameConfig};
use fnv_game::GameData;
use fnv_script::vm::Value;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;
use std::path::{Path, PathBuf};

/// VDoctors, the doctors' quest: stage 10 ResetHealth, 20 the addiction cure, 30 the
/// rads [observed: FalloutNV.esm QUST 0x000CE959].
const VDOCTORS: FormId = FormId(0x000C_E959);
/// A doctor's heal for 75 caps (`SetActorValue Variable01 1`) and rad removal for 100
/// (`SetStage VDoctors 30`) [observed: FalloutNV.esm INFO 0x000CE882, 0x000CE87F].
const HEAL_LINE: FormId = FormId(0x000C_E882);
const RADS_LINE: FormId = FormId(0x000C_E87F);
const CAPS: FormId = FormId(0xF);

fn data_dir() -> Option<PathBuf> {
    let Some(dir) = std::env::var_os("FNVLAB_DATA_DIR").map(PathBuf::from) else {
        eprintln!("skipped: FNVLAB_DATA_DIR is not set (no game data)");
        return None;
    };
    if !dir.join("FalloutNV.esm").is_file() {
        eprintln!("skipped: FalloutNV.esm is not in {}", dir.display());
        return None;
    }
    Some(dir)
}

struct Loaded {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn load(dir: &Path) -> Loaded {
    let plugins = [Plugin::open(dir.join("FalloutNV.esm")).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema = Schema::load_dir(root.join("schemas/fnv")).unwrap();
    let fp = root.join("schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&fp).unwrap()).unwrap();
    let table = FunctionTable::load(&fp).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Loaded { data, forms, refs }
}

fn guest(l: &Loaded) -> Game<'_> {
    let world = World::new(&l.forms, &l.refs, SimConfig::default());
    let mut g = Game::new(&l.data, world, GameConfig::default());
    g.new_game();
    g.coop_guest = true;
    g.world_logic_off = true;
    g.story_plugins = vec![0];
    g
}

fn console(g: &mut Game, func: &str, args: &[&str]) -> f64 {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    match call_words(g, Some(FormId::PLAYER_REF), func, &args, "test").unwrap_or_else(|e| panic!("{func}: {e}")).value {
        Value::Num(n) => n,
        _ => 0.0,
    }
}

fn caps(g: &mut Game) -> i32 {
    g.world.item_count(FormId::PLAYER_REF, CAPS).unwrap_or(0)
}

/// The line's begin and end scripts, as the conversation runs them, said by `doctor`.
fn say(g: &mut Game, doctor: FormId, line: FormId) {
    let d = g.data.infos[&line].clone();
    for u in [d.begin, d.end].into_iter().flatten() {
        g.run_info_result(doctor, u, d.quest);
    }
}

#[test]
fn a_guest_buys_a_doctors_heal_and_rad_removal_for_itself() {
    let Some(dir) = data_dir() else { return };
    let l = load(&dir);
    let mut g = guest(&l);
    let (quests, scripts, doctors, lines) = {
        let s = g.coop_services();
        (s.quests.clone(), s.scripts.clone(), s.doctors.clone(), s.lines.clone())
    };
    eprintln!("doctor quests {quests:X?}; {} doctor scripts; {} doctors; {} service lines", scripts.len(), doctors.len(), lines.len());
    assert!(quests.contains(&VDOCTORS), "VDoctors is a doctor quest: {quests:X?}");
    assert!(lines.contains(&HEAL_LINE) && lines.contains(&RADS_LINE), "the heal and rad lines are services");
    assert!(lines.len() < 200, "only doctors' lines: {}", lines.len());
    assert!(g.guest_lines().contains(&HEAL_LINE) && g.guest_lines().contains(&RADS_LINE), "a guest is offered them");
    // VDoctors' stages run again each time a player pays.
    assert!(l.data.quests[&VDOCTORS].repeated_stages(), "VDoctors allows repeated stages");

    // A doctor of the heal line, placed in the world.
    let speaker = fnv_game::coop_services::line_speaker(&l.data, &l.data.infos[&HEAL_LINE]).expect("the heal line names its doctor");
    let doctor = l.refs.refs.iter().find(|(_, p)| p.base == speaker).map(|(r, _)| *r).expect("the doctor is placed");
    g.world.set_enabled(doctor, true).ok();
    eprintln!("the heal line's doctor: {} ({doctor})", l.data.name(speaker));

    // The guest: hurt, irradiated, with caps.
    console(&mut g, "AddItem", &["Caps001", "500"]);
    let full = console(&mut g, "GetAV", &["Health"]);
    console(&mut g, "DamageAV", &["Health", "40"]);
    console(&mut g, "DamageAV", &["RadiationRads", "300"]);
    let hurt = console(&mut g, "GetAV", &["Health"]);
    let rads = console(&mut g, "GetAV", &["RadiationRads"]);
    assert!(hurt < full && rads >= 300.0, "the guest is hurt ({hurt}/{full}) and irradiated ({rads})");
    let caps0 = caps(&mut g);
    // What the setup gave counts as nothing either way (the new game's own scripts and the
    // test's AddItem run as a guest's would).
    g.coop_story = Default::default();

    // Rad removal: the stage acts on the guest at once.
    say(&mut g, doctor, RADS_LINE);
    assert_eq!(caps(&mut g), caps0 - 100, "the guest paid 100 caps");
    assert_eq!(console(&mut g, "GetAV", &["RadiationRads"]), 0.0, "the guest's rads are gone");

    // The heal: the doctor's script heals the guest when its procedure ends.
    say(&mut g, doctor, HEAL_LINE);
    assert_eq!(caps(&mut g), caps0 - 175, "the guest paid 75 more");
    assert!(g.guest_own_refs.contains_key(&doctor), "the doctor's script runs in the guest's game");
    let mut healed_at = None;
    for frame in 0..60 * 20 {
        g.tick();
        if healed_at.is_none() && console(&mut g, "GetAV", &["Health"]) >= full {
            healed_at = Some(frame);
        }
    }
    let healed_at = healed_at.expect("the guest is healed");
    eprintln!("healed after {healed_at} frames");
    assert!(healed_at > 60 * 5, "after the doctor's procedure, not at once: frame {healed_at}");

    // Paid and cured for good: nothing of it is the host's story.
    assert!(g.coop_story.is_empty(), "the guest's purchase is its own: {:?}", g.coop_story);
    // The doctor's script runs out.
    for _ in 0..60 * 15 {
        g.tick();
    }
    assert!(!g.guest_own_refs.contains_key(&doctor), "the doctor's script stops running in the guest's game");
}
