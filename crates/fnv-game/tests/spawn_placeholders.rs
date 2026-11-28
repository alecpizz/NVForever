//! Leveled spawns' placeholder creatures (docs/spec-notes/dead-on-spawn.md §4.1, #3228,
//! #3139): a placed creature whose base takes its script, stats and base data from a
//! leveled list (template flags "Script", "Stats", "Base Data") runs the script of the
//! list's pick. Actors scripted only through their template get OnLoad and OnReset, not
//! the per-frame blocks (scripts.md §7.1). A placeholder pick (Health 0, Respawn, Is
//! Ghost, a script whose OnLoad kills it and sets it destroyed) starts dead, comes back
//! alive at a cell reset (its Respawn flag) and its OnLoad kills it again at once; a
//! loaded save that holds one alive kills it at the first frame. On a synthetic plugin
//! (no game data); the last test reads the master when `FNVLAB_DATA_DIR` is set.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{event, Game, GameConfig, InstKey};
use fnv_game::GameData;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;
use std::path::{Path, PathBuf};

/// The placeholder: Health 0, Respawn, Is Ghost, the script below.
const PLACEHOLDER: u32 = 0x840;
/// The leveled list whose only entry is the placeholder (the pick is never in doubt).
const LIST: u32 = 0x841;
/// The placed base: Script, Stats and Base Data from the list.
const SPAWN: u32 = 0x842;
/// A creature with its own script and no template, for comparison.
const OWN_SCRIPTED: u32 = 0x843;
/// A spawn base whose list holds only an unscripted creature.
const PLAIN: u32 = 0x844;
const PLAIN_LIST: u32 = 0x845;
const PLAIN_SPAWN: u32 = 0x846;
/// A list whose pick moves with the player's level: CREA_A at level 1, CREA_B at 10.
const MIXED_LIST: u32 = 0x847;
const CREA_A: u32 = 0x848;
const CREA_B: u32 = 0x849;
const MIXED_SPAWN: u32 = 0x84A;
/// An actor with its own script whose template list has only a level-50 entry: at a
/// low level the list yields none and the actor itself is the source of the script.
const FALLBACK: u32 = 0x84B;
const HIGH_LIST: u32 = 0x84C;
const FALLBACK_SPAWN: u32 = 0x84D;
const PLACEHOLDER_SCRIPT: u32 = 0x850;
const SCRIPT_A: u32 = 0x851;
const SCRIPT_B: u32 = 0x852;
const FALLBACK_SCRIPT: u32 = 0x853;
const SPAWN_REF: u32 = 0x300;
const OWN_SCRIPTED_REF: u32 = 0x301;
const PLAIN_SPAWN_REF: u32 = 0x302;
const MIXED_REF: u32 = 0x303;
/// The placeholder list placed directly (a root list as the base).
const ROOT_LIST_REF: u32 = 0x304;
const FALLBACK_REF: u32 = 0x305;
const CELL_A: u32 = 0x200;
const CELL_B: u32 = 0x201;
const KILL_ACTOR: u16 = 0x108B;
const SET_DESTROYED: u16 = 0x10CC;
/// ACBS flags: Respawn (bit 3), Is Ghost (bit 29) [documented: xEdit; schema gameplay.json].
const RESPAWN: u32 = 0x8;
const IS_GHOST: u32 = 0x2000_0000;
/// ACBS template flags: Stats, Base Data, Script.
const STATS: u16 = 0x2;
const BASE_DATA: u16 = 0x80;
const SCRIPT: u16 = 0x200;

fn acbs(flags: u32, template_flags: u16) -> fnv_esm::raw::Subrecord {
    let mut d = flags.to_le_bytes().to_vec();
    d.extend([0u8; 16]);
    d.extend(0i16.to_le_bytes());
    d.extend(template_flags.to_le_bytes());
    sub(b"ACBS", d)
}

/// CREA DATA: type, combat, magic and stealth skill, health, unused, damage, attributes.
fn crea_data(health: i16) -> fnv_esm::raw::Subrecord {
    sub(b"DATA", [vec![0u8, 50, 50, 50], health.to_le_bytes().to_vec(), vec![0; 2], 0i16.to_le_bytes().to_vec(), vec![1; 7]].concat())
}

fn list(fid: u32, edid: &str, entries: &[(i16, u32)]) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"LVLD", vec![0]), sub(b"LVLF", vec![0])];
    for (level, entry) in entries {
        subs.push(sub(b"LVLO", [level.to_le_bytes().to_vec(), vec![0; 2], entry.to_le_bytes().to_vec(), 1i16.to_le_bytes().to_vec(), vec![0; 2]].concat()));
    }
    record(b"LVLC", 0, fid, &subs)
}

fn glob(fid: u32, edid: &str, v: f32) -> Vec<u8> {
    record(b"GLOB", 0, fid, &[sub(b"EDID", z(edid)), sub(b"FNAM", vec![b'f']), sub(b"FLTV", v.to_le_bytes().to_vec())])
}

fn placed(fid: u32, base: u32) -> Vec<u8> {
    record(b"ACRE", 0, fid, &[sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])])
}

fn cell(fid: u32, edid: &str, refs: &[u8]) -> Vec<u8> {
    let c = record(b"CELL", 0, fid, &[sub(b"EDID", z(edid))]);
    [c, group(fid.to_le_bytes(), 6, &group(fid.to_le_bytes(), 9, refs))].concat()
}

/// An object script whose OnLoad sets its variable `index` (Integer) to `value`; both
/// variables 1 and 2 are declared.
fn marking_script(fid: u32, edid: &str, index: u16, value: &str) -> Vec<u8> {
    let body = set(var(b's', index), ex(&[a(value)]));
    record(b"SCPT", 0, fid, &[vec![sub(b"EDID", z(edid))], unit(0, &script(&[(event::ON_LOAD, None, body), (event::GAME_MODE, None, set(var(b's', 3), ex(&[a("1")])))]), &[(1, "nA", 1), (2, "nB", 1), (3, "nFrame", 1)], &[])].concat())
}

fn plugin_bytes() -> Vec<u8> {
    // Begin OnLoad: Kill, SetDestroyed 1, as the masters' placeholder script does.
    let on_load = [st(KILL_ACTOR, &le16(0)), st(SET_DESTROYED, &[le16(1), vec![b'n'], 1i32.to_le_bytes().to_vec()].concat())].concat();
    let scpts = [
        record(b"SCPT", 0, PLACEHOLDER_SCRIPT, &[vec![sub(b"EDID", z("PlaceholderScript"))], unit(0, &script(&[(event::ON_LOAD, None, on_load)]), &[], &[])].concat()),
        marking_script(SCRIPT_A, "TestScriptA", 1, "5"),
        marking_script(SCRIPT_B, "TestScriptB", 2, "7"),
        marking_script(FALLBACK_SCRIPT, "TestFallbackScript", 2, "9"),
    ]
    .concat();
    let scri = |s: u32| sub(b"SCRI", s.to_le_bytes().to_vec());
    let tplt = |t: u32| sub(b"TPLT", t.to_le_bytes().to_vec());
    let spawn_flags = STATS | BASE_DATA | SCRIPT;
    let creatures = [
        record(b"CREA", 0, PLACEHOLDER, &[sub(b"EDID", z("TestVoidPlaceholder")), acbs(RESPAWN | IS_GHOST, 0), scri(PLACEHOLDER_SCRIPT), crea_data(0)]),
        record(b"CREA", 0, SPAWN, &[sub(b"EDID", z("TestSpawnTier1Small75")), acbs(0, spawn_flags), tplt(LIST), crea_data(50)]),
        record(b"CREA", 0, OWN_SCRIPTED, &[sub(b"EDID", z("TestScriptedCritter")), acbs(0, 0), scri(PLACEHOLDER_SCRIPT), crea_data(0)]),
        record(b"CREA", 0, PLAIN, &[sub(b"EDID", z("TestCalf")), acbs(RESPAWN, 0), crea_data(20)]),
        record(b"CREA", 0, PLAIN_SPAWN, &[sub(b"EDID", z("TestSpawnCalf")), acbs(0, spawn_flags), tplt(PLAIN_LIST), crea_data(20)]),
        record(b"CREA", 0, CREA_A, &[sub(b"EDID", z("TestCritterA")), acbs(RESPAWN, 0), scri(SCRIPT_A), crea_data(10)]),
        record(b"CREA", 0, CREA_B, &[sub(b"EDID", z("TestCritterB")), acbs(RESPAWN, 0), scri(SCRIPT_B), crea_data(10)]),
        record(b"CREA", 0, MIXED_SPAWN, &[sub(b"EDID", z("TestSpawnMixed")), acbs(0, spawn_flags), tplt(MIXED_LIST), crea_data(10)]),
        record(b"CREA", 0, FALLBACK, &[sub(b"EDID", z("TestFallback")), acbs(0, SCRIPT), tplt(HIGH_LIST), scri(FALLBACK_SCRIPT), crea_data(10)]),
        record(b"CREA", 0, FALLBACK_SPAWN, &[sub(b"EDID", z("TestSpawnFallback")), acbs(0, SCRIPT), tplt(FALLBACK), crea_data(10)]),
    ]
    .concat();
    let lists = [
        list(LIST, "TestEncSmall75", &[(1, PLACEHOLDER)]),
        list(PLAIN_LIST, "TestEncCalf", &[(1, PLAIN)]),
        list(MIXED_LIST, "TestEncMixed", &[(1, CREA_A), (10, CREA_B)]),
        list(HIGH_LIST, "TestEncHigh", &[(50, PLAIN)]),
    ]
    .concat();
    let globs = [
        glob(0x900, "GameYear", 2281.0),
        glob(0x901, "GameMonth", 9.0),
        glob(0x902, "GameDay", 19.0),
        glob(0x903, "GameHour", 9.0),
        glob(0x904, "GameDaysPassed", 1.0),
    ]
    .concat();
    let refs = [
        placed(SPAWN_REF, SPAWN),
        placed(OWN_SCRIPTED_REF, OWN_SCRIPTED),
        placed(PLAIN_SPAWN_REF, PLAIN_SPAWN),
        placed(MIXED_REF, MIXED_SPAWN),
        placed(ROOT_LIST_REF, LIST),
        placed(FALLBACK_REF, FALLBACK_SPAWN),
    ]
    .concat();
    let cells = [cell(CELL_A, "TestHerdCell", &refs), cell(CELL_B, "TestAwayCell", &[])].concat();
    [
        tes4(),
        top(b"GLOB", &globs),
        top(b"SCPT", &scpts),
        top(b"CREA", &creatures),
        top(b"LVLC", &lists),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cells))),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn build(plugins: &[Plugin]) -> Fixture {
    let lo = LoadOrder::new(plugins).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema = Schema::load_dir(root.join("schemas/fnv")).unwrap();
    let p = root.join("schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Fixture { data, forms, refs }
}

fn fixture() -> Fixture {
    build(&[Plugin::parse(&plugin_bytes(), "Placeholders.esm".into()).unwrap()])
}

fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    Game::new(&f.data, world, GameConfig::default())
}

fn destroyed(g: &Game, r: u32) -> bool {
    g.extra.get(&FormId(r)).is_some_and(|e| e.destroyed)
}

fn local(g: &Game, r: u32, i: u16) -> f64 {
    g.var(InstKey::Ref { reference: FormId(r) }, i).num()
}

/// Leave the herd's cell for 72 h and come back: the cell resets.
fn reset_herd_cell(g: &mut Game) {
    g.player_entered(Some(FormId(CELL_B)));
    g.world.advance_hours(72.0).unwrap();
    g.player_entered(Some(FormId(CELL_A)));
}

/// A reference whose base gets its script only through its template chain is indexed
/// apart from those with their own script: through a list's pick, a list placed
/// directly, or an actor on the chain kept when its list yields no entry. One whose
/// chain can give no script is not indexed.
#[test]
fn a_script_from_the_template_chain_is_indexed() {
    let f = fixture();
    let d = &f.data;
    assert!(d.script_through_template(FormId(SPAWN)), "the list's placeholder has a script");
    assert!(d.script_through_template(FormId(LIST)), "a root list placed directly");
    assert!(d.script_through_template(FormId(FALLBACK_SPAWN)), "the fallback actor on the chain has one");
    assert!(d.script_through_template(FormId(MIXED_SPAWN)));
    assert!(!d.script_through_template(FormId(PLAIN_SPAWN)), "the list's calf has none");
    assert!(!d.script_through_template(FormId(OWN_SCRIPTED)), "its own script, no template");
    assert_eq!(d.scripted_by_cell.get(&FormId(CELL_A)), Some(&vec![FormId(OWN_SCRIPTED_REF)]));
    assert_eq!(d.template_scripted_by_cell.get(&FormId(CELL_A)), Some(&vec![FormId(SPAWN_REF), FormId(MIXED_REF), FormId(ROOT_LIST_REF), FormId(FALLBACK_REF)]));
    let g = game(&f);
    assert_eq!(g.ref_script(FormId(SPAWN_REF)), Some(FormId(PLACEHOLDER_SCRIPT)));
    assert_eq!(g.ref_script(FormId(ROOT_LIST_REF)), Some(FormId(PLACEHOLDER_SCRIPT)));
    assert_eq!(g.ref_script(FormId(FALLBACK_REF)), Some(FormId(FALLBACK_SCRIPT)), "the level-50 entry is out of reach: the fallback's own");
    assert_eq!(g.ref_script(FormId(PLAIN_SPAWN_REF)), None);
}

/// The template index schedules OnLoad for a placed actor before it moves.
#[test]
fn template_scripted_actors_get_on_load_but_not_game_mode() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.player_entered(Some(FormId(CELL_A)));
    assert_eq!(local(&g, MIXED_REF, 1), 5.0, "OnLoad of the level-1 pick's script ran");
    assert_eq!(local(&g, FALLBACK_REF, 2), 9.0, "OnLoad of the fallback's script ran");
    g.frames(3);
    assert_eq!(local(&g, MIXED_REF, 3), 0.0, "its GameMode block does not run");
}

/// The placeholder is laid out dead by a new game (Health 0 from the pick), comes back
/// alive at a cell reset (Respawn from the pick, "Base Data") and is killed and set
/// destroyed by its OnLoad in the same load (#3228: before, its OnLoad never ran and it
/// stayed alive at health 1, fought and hit the player).
#[test]
fn a_placeholder_brought_back_by_a_cell_reset_is_killed_by_its_on_load() {
    let f = fixture();
    let mut g = game(&f);
    let spawn = FormId(SPAWN_REF);
    assert!(g.starts_dead(spawn), "Health 0 through the pick");
    assert!(g.respawns(spawn), "Respawn through the pick (Base Data)");
    assert!(g.is_ghost(spawn), "Is Ghost through the pick (Base Data)");
    g.new_game();
    g.player_entered(Some(FormId(CELL_A)));
    assert!(g.is_dead(spawn));
    assert!(destroyed(&g, SPAWN_REF), "its OnLoad ran at the first load");
    // The calf of the plain spawn is killed, to come back alive with it.
    assert!(g.kill(FormId(PLAIN_SPAWN_REF), Some(FormId::PLAYER_REF)));

    reset_herd_cell(&mut g);
    let resets = g.take_cell_resets();
    assert_eq!(resets.len(), 1);
    assert!(resets[0].respawned.contains(&spawn) && resets[0].respawned.contains(&FormId(PLAIN_SPAWN_REF)), "both were recreated: {:?}", resets[0].respawned);
    assert!(g.is_dead(spawn), "the recreated placeholder's OnLoad killed it again");
    assert!(destroyed(&g, SPAWN_REF), "and set it destroyed (the reset had cleared that)");
    assert!(g.extra[&spawn].killer.is_none());
    assert!(!g.is_dead(FormId(PLAIN_SPAWN_REF)), "a real creature of a spawn stays alive");
}

/// A reset that picks another entry of a mixed list, whose script differs, starts the
/// reference's script instance afresh: the old script's variables do not carry over.
#[test]
fn a_reset_that_changes_the_pick_starts_a_fresh_script_instance() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.player_entered(Some(FormId(CELL_A)));
    let key = InstKey::Ref { reference: FormId(MIXED_REF) };
    assert_eq!(g.ref_script(FormId(MIXED_REF)), Some(FormId(SCRIPT_A)));
    assert_eq!(local(&g, MIXED_REF, 1), 5.0);
    assert!(g.kill(FormId(MIXED_REF), Some(FormId::PLAYER_REF)));
    // The player has grown: the reset's pick reads level 20, the level-10 entry.
    g.player.level = Some(20);
    reset_herd_cell(&mut g);
    assert!(!g.is_dead(FormId(MIXED_REF)), "back alive");
    assert_eq!(g.ref_script(FormId(MIXED_REF)), Some(FormId(SCRIPT_B)));
    assert_eq!(g.instances.get(&key).and_then(|i| i.script), Some(FormId(SCRIPT_B)));
    assert_eq!(local(&g, MIXED_REF, 1), 0.0, "script A's variable is gone");
    assert_eq!(local(&g, MIXED_REF, 2), 7.0, "script B's OnLoad ran on the fresh instance");
}

/// The game saved as a build from before #3228 wrote it: no mark that the template-scripted
/// actors' OnLoad ran at their cell load.
fn older_save(g: &Game) -> fnv_game::game::GameSnapshot {
    let mut s = g.snapshot();
    s.template_onloads_run = false;
    s.restored_onload.clear();
    s
}

/// A save from this build: the template-scripted actors ran OnLoad at their cell load, so
/// a load runs none of them again (#4820: a script drawing GetRandomPercent in its OnLoad
/// drew again and moved the game's generator).
#[test]
fn a_restored_new_save_runs_no_template_on_load_again() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.player_entered(Some(FormId(CELL_A)));
    // Alive (as an older build left it) but saved by this one: not queued.
    g.extra.remove(&FormId(SPAWN_REF));
    let s = g.snapshot();
    assert!(s.template_onloads_run);
    let world = World::restore(&f.forms, &f.refs, &s.sim).unwrap();
    let mut back = Game::restore(&f.data, world, GameConfig::default(), &s).unwrap();
    assert!(back.restored_onload.is_empty());
    back.frames(1);
    assert!(!back.is_dead(FormId(SPAWN_REF)), "no second OnLoad");
}

/// A save made before the queue's first frame keeps the queue: the load runs it.
#[test]
fn a_queue_saved_before_its_frame_runs_after_the_next_load() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.player_entered(Some(FormId(CELL_A)));
    g.extra.remove(&FormId(SPAWN_REF));
    let s = older_save(&g);
    let world = World::restore(&f.forms, &f.refs, &s.sim).unwrap();
    let back = Game::restore(&f.data, world, GameConfig::default(), &s).unwrap();
    assert!(back.restored_onload.contains(&FormId(SPAWN_REF)));
    // Saved again at once, by this build.
    let s = back.snapshot();
    assert!(s.template_onloads_run);
    let world = World::restore(&f.forms, &f.refs, &s.sim).unwrap();
    let mut again = Game::restore(&f.data, world, GameConfig::default(), &s).unwrap();
    assert!(again.restored_onload.contains(&FormId(SPAWN_REF)));
    again.frames(1);
    assert!(again.is_dead(FormId(SPAWN_REF)), "its OnLoad killed it");
}

/// A save from before the fix holds a placeholder alive in a loaded cell: the cells stay
/// loaded through the restore, and its OnLoad runs at the first frame after it.
#[test]
fn a_restored_live_placeholder_is_killed_at_the_first_frame() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.player_entered(Some(FormId(CELL_A)));
    // As an older build left it: alive, never destroyed.
    g.extra.remove(&FormId(SPAWN_REF));
    assert!(!g.is_dead(FormId(SPAWN_REF)));
    let s = older_save(&g);
    let world = World::restore(&f.forms, &f.refs, &s.sim).unwrap();
    let mut back = Game::restore(&f.data, world, GameConfig::default(), &s).unwrap();
    assert!(!back.is_dead(FormId(SPAWN_REF)), "nothing runs inside the restore");
    back.frames(1);
    assert!(back.is_dead(FormId(SPAWN_REF)), "its OnLoad killed it");
    assert!(destroyed(&back, SPAWN_REF));
    assert_eq!(local(&back, MIXED_REF, 1), 5.0, "the living mixed spawn ran its OnLoad again, unchanged");
}

/// The per-frame scheduler still runs the template script for a moved actor.
#[test]
fn a_template_scripted_actor_that_moved_keeps_game_mode() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.player_entered(Some(FormId(CELL_A)));
    g.set_actor_place(FormId(MIXED_REF), Some(FormId(CELL_A)), [64.0, 0.0, 0.0], 0.0).unwrap();
    assert!(g.moved.contains(&FormId(MIXED_REF)), "a step lists it as moved");
    assert!(g.template_only(FormId(MIXED_REF)));
    assert!(!g.template_only(FormId(OWN_SCRIPTED_REF)));
    g.frames(3);
    assert_eq!(local(&g, MIXED_REF, 3), 1.0, "its GameMode block still runs after moving");
}

/// A placeholder an older build left alive and walking, saved in another loaded cell than
/// the one it was placed in (the report's ran at the player), is found by where it is now.
#[test]
fn a_restored_live_placeholder_away_from_its_cell_is_killed() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.player_entered(Some(FormId(CELL_A)));
    g.set_loaded([FormId(CELL_A), FormId(CELL_B)].into_iter().collect());
    g.extra.remove(&FormId(SPAWN_REF));
    g.set_actor_place(FormId(SPAWN_REF), Some(FormId(CELL_B)), [500.0, 0.0, 0.0], 0.0).unwrap();
    let s = older_save(&g);
    let world = World::restore(&f.forms, &f.refs, &s.sim).unwrap();
    let mut back = Game::restore(&f.data, world, GameConfig::default(), &s).unwrap();
    assert_eq!(back.cell_of(FormId(SPAWN_REF)), Some(FormId(CELL_B)));
    back.frames(1);
    assert!(back.is_dead(FormId(SPAWN_REF)), "its OnLoad killed it where it stood");
    assert!(destroyed(&back, SPAWN_REF));
}

/// One queued by the restore that dies before its turn does not run its OnLoad.
#[test]
fn a_restored_placeholder_killed_before_its_turn_runs_no_on_load() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.player_entered(Some(FormId(CELL_A)));
    g.extra.remove(&FormId(SPAWN_REF));
    let s = older_save(&g);
    let world = World::restore(&f.forms, &f.refs, &s.sim).unwrap();
    let mut back = Game::restore(&f.data, world, GameConfig::default(), &s).unwrap();
    assert!(back.restored_onload.contains(&FormId(SPAWN_REF)));
    assert!(back.kill(FormId(SPAWN_REF), None));
    back.frames(1);
    assert!(!destroyed(&back, SPAWN_REF), "checked again at its turn: dead, so no OnLoad");
    assert!(back.restored_onload.is_empty());
}

/// A reset whose pick runs the same script keeps the instance: its variables stay.
#[test]
fn a_reset_that_keeps_the_pick_keeps_the_script_instance() {
    let f = fixture();
    let mut g = game(&f);
    g.new_game();
    g.player_entered(Some(FormId(CELL_A)));
    let key = InstKey::Ref { reference: FormId(MIXED_REF) };
    g.store(key, Some(FormId(SCRIPT_A)), 2, fnv_script::vm::Value::Num(4.0), "test", true);
    assert!(g.kill(FormId(MIXED_REF), Some(FormId::PLAYER_REF)));
    reset_herd_cell(&mut g);
    assert!(!g.is_dead(FormId(MIXED_REF)));
    assert_eq!(g.ref_script(FormId(MIXED_REF)), Some(FormId(SCRIPT_A)));
    assert_eq!(local(&g, MIXED_REF, 2), 4.0, "the same script: its instance kept");
}

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

/// The masters: the Camp Golf herd of #3228 (0x00168D44–0x00168D51, bighorner and
/// bloatfly spawns whose lists hold `VCrILikeOblivionChancedSpawns`) is indexed through
/// its picks. Prints the herd's references whose pick is the placeholder (the picks are
/// seeded by the reference, and every entry of these lists is level 1, so a new game
/// and a reset pick the same) and how many references are indexed through a template,
/// by script.
#[test]
fn the_camp_golf_herd_runs_its_placeholder_script() {
    let Some(dir) = data_dir() else { return };
    let f = build(&[Plugin::open(dir.join("FalloutNV.esm")).unwrap()]);
    let placeholder = f.data.by_edid("VCrILikeOblivionChancedSpawns").expect("the placeholder");
    let herd: Vec<FormId> = (0x0016_8D44..=0x0016_8D51)
        .map(FormId)
        .filter(|&r| f.refs.get(r).is_some_and(|p| f.data.template_sources_any_pick(p.base, |a| a.template_script).contains(&placeholder)))
        .collect();
    assert!(herd.len() >= 4, "the herd's spawns that may be the placeholder: {herd:?}");
    let indexed: std::collections::BTreeSet<FormId> = f.data.template_scripted_by_cell.values().flatten().copied().collect();
    for r in &herd {
        assert!(indexed.contains(r), "{r} may be the placeholder: its OnLoad must run");
    }
    let picked: Vec<String> = herd
        .iter()
        .filter(|&&r| f.data.template_source(f.refs.get(r).unwrap().base, r, |a| a.template_script).is_some_and(|(id, _)| id == placeholder))
        .map(|r| r.to_string())
        .collect();
    eprintln!("herd references whose pick is the placeholder: {}", picked.join(" "));
    let direct: usize = f.data.scripted_by_cell.values().map(Vec::len).sum();
    eprintln!("{direct} references scripted by their own base, {} through a template only", indexed.len());
    // By the scripts their picks may run (a reference may count under several).
    let mut by_script: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for r in &indexed {
        let base = f.refs.get(*r).unwrap().base;
        let scripts: std::collections::BTreeSet<FormId> = f.data.template_chain_any_pick(base, |a| a.template_script).into_iter().filter_map(|s| f.data.script_of_base(s)).collect();
        for s in scripts {
            let game_mode = f.data.script(s).is_some_and(|u| u.has_block(event::GAME_MODE));
            let on_load = f.data.script(s).is_some_and(|u| u.has_block(event::ON_LOAD));
            *by_script.entry(format!("{}{}{}", f.data.name(s), if on_load { " (OnLoad)" } else { "" }, if game_mode { " (GameMode)" } else { "" })).or_default() += 1;
        }
    }
    for (s, n) in &by_script {
        eprintln!("  {n:5} {s}");
    }
}
