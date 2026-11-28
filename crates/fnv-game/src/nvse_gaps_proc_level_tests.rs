//! Regression tests for `GetActorsByProcessingLevel`: level 0 returns only enabled actors
//! currently in loaded cells (excluding unloaded actors that previously ran AI packages),
//! level 3 returns enabled unloaded placed actors (including unloaded package actors), and
//! levels 1, 2, and unsupported values return `ar_Null`.

use crate::common::*;
use crate::game::{Game, GameConfig, RunCtx};
use crate::GameData;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};

const CELL_LOADED: FormId = FormId(0x200);
const CELL_FAR: FormId = FormId(0x201);
const NPC_BASE: FormId = FormId(0x400);
const ACTOR_LOADED: FormId = FormId(0x500);
const ACTOR_DISABLED: FormId = FormId(0x501);
const ACTOR_FAR_PKG: FormId = FormId(0x502);
const ACTOR_FAR_IDLE: FormId = FormId(0x503);
const PKG: FormId = FormId(0x600);

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let achr = |id: FormId, edid: &str, flags: u32| {
        record(b"ACHR", flags, id.0, &[sub(b"EDID", z(edid)), sub(b"NAME", NPC_BASE.0.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])])
    };
    let loaded_refs = [achr(ACTOR_LOADED, "ActorLoaded", 0), achr(ACTOR_DISABLED, "ActorDisabled", 0x0800)].concat();
    let far_refs = [achr(ACTOR_FAR_PKG, "ActorFarPkg", 0), achr(ACTOR_FAR_IDLE, "ActorFarIdle", 0)].concat();
    let cell_loaded = [
        record(b"CELL", 0, CELL_LOADED.0, &[sub(b"DATA", vec![1])]),
        group(CELL_LOADED.0.to_le_bytes(), 6, &group(CELL_LOADED.0.to_le_bytes(), 8, &loaded_refs)),
    ]
    .concat();
    let cell_far = [
        record(b"CELL", 0, CELL_FAR.0, &[sub(b"DATA", vec![1])]),
        group(CELL_FAR.0.to_le_bytes(), 6, &group(CELL_FAR.0.to_le_bytes(), 8, &far_refs)),
    ]
    .concat();
    let bytes = [
        tes4(),
        top(
            b"NPC_",
            &[
                record(b"NPC_", 0, FormId::PLAYER_BASE.0, &[sub(b"EDID", z("Player"))]),
                record(b"NPC_", 0, NPC_BASE.0, &[sub(b"EDID", z("Guard"))]),
            ]
            .concat(),
        ),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell_loaded, cell_far].concat()))),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "ProcLevel.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let path = root.join("script/functions.json");
    let catalogue = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&path).unwrap(), &catalogue, &refs);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() })
}

fn query_num(g: &mut Game<'_>, expr: &str) -> f64 {
    let v = g
        .run_query(&format!("Begin Function {{}}\nSetFunctionValue ({expr})\nEnd\n"), RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() })
        .unwrap_or_else(|e| panic!("{expr}: {e}"));
    match v {
        XVal::Num(n) => n,
        XVal::Ref(r) => f64::from(r),
        other => panic!("{expr}: unexpected {other:?}"),
    }
}

fn proc_level_refs(g: &mut Game<'_>, level: i32) -> Option<Vec<FormId>> {
    let size = query_num(g, &format!("ar_Size (GetActorsByProcessingLevel {level})")) as i32;
    if size < 0 {
        return None;
    }
    let mut out = Vec::with_capacity(size as usize);
    for i in 0..size {
        let id = query_num(g, &format!("(GetActorsByProcessingLevel {level})[{i}]")) as u32;
        out.push(FormId(id));
    }
    Some(out)
}

#[test]
fn level_zero_excludes_unloaded_package_actors_and_tracks_loaded_relocations_and_created_actors() {
    let f = fixture();
    let mut g = game(&f);
    g.loaded_cells.insert(CELL_LOADED);
    // Simulate a long playthrough where an unloaded actor previously chose an AI package.
    g.extra.entry(ACTOR_FAR_PKG).or_default().current_package = Some(PKG);

    // Level 0 must contain only enabled actors currently in loaded cells (not ACTOR_FAR_PKG or disabled ACTOR_DISABLED).
    assert_eq!(proc_level_refs(&mut g, 0), Some(vec![ACTOR_LOADED]));
    // Unloaded package actor belongs in level 3 alongside other enabled unloaded placed actors.
    assert_eq!(proc_level_refs(&mut g, 3), Some(vec![ACTOR_FAR_PKG, ACTOR_FAR_IDLE]));
    // Levels 1, 2, and unsupported -1 return ar_Null.
    assert_eq!(proc_level_refs(&mut g, 1), None);
    assert_eq!(proc_level_refs(&mut g, 2), None);
    assert_eq!(proc_level_refs(&mut g, -1), None);

    // Walking ACTOR_FAR_PKG into CELL_LOADED moves it from level 3 to level 0.
    g.set_actor_place(ACTOR_FAR_PKG, Some(CELL_LOADED), [10.0, 20.0, 0.0], 0.0).unwrap();
    assert_eq!(proc_level_refs(&mut g, 0), Some(vec![ACTOR_LOADED, ACTOR_FAR_PKG]));
    assert_eq!(proc_level_refs(&mut g, 3), Some(vec![ACTOR_FAR_IDLE]));

    // Moving ACTOR_LOADED out to unloaded CELL_FAR moves it out of level 0 and into level 3.
    g.set_actor_place(ACTOR_LOADED, Some(CELL_FAR), [0.0; 3], 0.0).unwrap();
    assert_eq!(proc_level_refs(&mut g, 0), Some(vec![ACTOR_FAR_PKG]));
    assert_eq!(proc_level_refs(&mut g, 3), Some(vec![ACTOR_LOADED, ACTOR_FAR_IDLE]));

    // Created actors in loaded cells appear in level 0; created actors in unloaded cells or deleted do not.
    let spawned_loaded = g.world.place(NPC_BASE, Some(CELL_LOADED), [0.0; 3], [0.0; 3], 1).unwrap();
    let spawned_far = g.world.place(NPC_BASE, Some(CELL_FAR), [0.0; 3], [0.0; 3], 1).unwrap();
    assert_eq!(proc_level_refs(&mut g, 0), Some(vec![ACTOR_FAR_PKG, spawned_loaded]));
    assert!(!proc_level_refs(&mut g, 0).unwrap().contains(&spawned_far));

    g.world.set_deleted(spawned_loaded, true).unwrap();
    assert_eq!(proc_level_refs(&mut g, 0), Some(vec![ACTOR_FAR_PKG]));
}
