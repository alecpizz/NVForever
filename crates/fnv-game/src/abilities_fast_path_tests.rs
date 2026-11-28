//! Regression tests for the `sync_loaded_actor_effects` / `object_effect` fast paths:
//! ARMO with and without EITM, `SetObjectEffect` overrides (and `TempCloneForm` clones),
//! WEAP EITM fallback, and incremental `effects_actors` tracking equivalence across cell
//! relocations, created actors, dispelled abilities, and `World` replacement.

use crate::common::*;
use crate::game::{Game, GameConfig, RunCtx};
use crate::GameData;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use std::collections::BTreeSet;

const CELL_A: FormId = FormId(0x200);
const CELL_B: FormId = FormId(0x201);
const MOD_PER: u32 = 0x850;
const MOD_DT: u32 = 0x851;
const ENCH_APPAREL: FormId = FormId(0x880);
const ENCH_APPAREL_ALT: FormId = FormId(0x881);
const ENCH_WEAPON: FormId = FormId(0x882);
const ENCH_WEAPON_ALT: FormId = FormId(0x883);
const ABIL_DT: FormId = FormId(0x870);
const HAT_ENCH: FormId = FormId(0x890);
const HAT_PLAIN: FormId = FormId(0x891);
const HAT_PLAIN_DEST: FormId = FormId(0x892);
const PISTOL_ENCH: FormId = FormId(0x893);
const PISTOL_PLAIN: FormId = FormId(0x894);
const NPC_BASE: FormId = FormId(0x400);
const ACTOR_A: FormId = FormId(0x500);
const ACTOR_B: FormId = FormId(0x501);
const PKG: FormId = FormId(0x600);

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn mgef(fid: u32, edid: &str, av: i32) -> Vec<u8> {
    let data = [
        u32s(&[0]),
        0.0f32.to_le_bytes().to_vec(),
        u32s(&[0]),
        vec![0xFF; 8],
        vec![0; 4],
        vec![0; 4],
        1.0f32.to_le_bytes().to_vec(),
        vec![0; 24],
        vec![0; 8],
        u32s(&[0]),
        av.to_le_bytes().to_vec(),
    ]
    .concat();
    record(b"MGEF", 0, fid, &[sub(b"EDID", z(edid)), sub(b"DATA", data)])
}

fn effect_subs(base: u32, mag: u32, av: i32) -> Vec<fnv_esm::raw::Subrecord> {
    vec![
        sub(b"EFID", base.to_le_bytes().to_vec()),
        sub(b"EFIT", [u32s(&[mag, 0, 0, 0]), av.to_le_bytes().to_vec()].concat()),
    ]
}

fn fixture() -> Fixture {
    let ench = |id: FormId, edid: &str, kind: u32, ef: u32, mag: u32, av: i32| {
        let mut subs = vec![sub(b"EDID", z(edid)), sub(b"ENIT", u32s(&[kind, 0, 0, 0]))];
        subs.extend(effect_subs(ef, mag, av));
        record(b"ENCH", 0, id.0, &subs)
    };
    let mut spel_subs = vec![sub(b"EDID", z("AbilDT")), sub(b"SPIT", u32s(&[4, 0, 0, 0]))];
    spel_subs.extend(effect_subs(MOD_DT, 15, 76));
    let spel = record(b"SPEL", 0, ABIL_DT.0, &spel_subs);

    let armo = |id: FormId, edid: &str, eitm: Option<FormId>, with_dest: bool| {
        let mut subs = vec![sub(b"EDID", z(edid)), sub(b"DATA", [0u32.to_le_bytes().to_vec(), 10u32.to_le_bytes().to_vec(), 1.0f32.to_le_bytes().to_vec()].concat())];
        if let Some(e) = eitm {
            subs.push(sub(b"EITM", e.0.to_le_bytes().to_vec()));
        }
        if with_dest {
            subs.push(sub(b"DEST", vec![0; 8]));
        }
        record(b"ARMO", 0, id.0, &subs)
    };
    let weap = |id: FormId, edid: &str, eitm: Option<FormId>| {
        let mut subs = vec![sub(b"EDID", z(edid))];
        if let Some(e) = eitm {
            subs.push(sub(b"EITM", e.0.to_le_bytes().to_vec()));
        }
        record(b"WEAP", 0, id.0, &subs)
    };
    let placed_actor = |id: FormId, edid: &str| {
        record(b"ACHR", 0, id.0, &[sub(b"EDID", z(edid)), sub(b"NAME", NPC_BASE.0.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])])
    };
    let cell_a = [
        record(b"CELL", 0, CELL_A.0, &[sub(b"DATA", vec![1])]),
        group(CELL_A.0.to_le_bytes(), 6, &group(CELL_A.0.to_le_bytes(), 8, &placed_actor(ACTOR_A, "ActorA"))),
    ]
    .concat();
    let cell_b = [
        record(b"CELL", 0, CELL_B.0, &[sub(b"DATA", vec![1])]),
        group(CELL_B.0.to_le_bytes(), 6, &group(CELL_B.0.to_le_bytes(), 8, &placed_actor(ACTOR_B, "ActorB"))),
    ]
    .concat();

    let bytes = [
        tes4(),
        top(b"MGEF", &[mgef(MOD_PER, "ModPer", 6), mgef(MOD_DT, "ModDT", 76)].concat()),
        top(b"SPEL", &spel),
        top(
            b"ENCH",
            &[
                ench(ENCH_APPAREL, "EnchApparel", 3, MOD_PER, 2, 6),
                ench(ENCH_APPAREL_ALT, "EnchApparelAlt", 3, MOD_PER, 4, 6),
                ench(ENCH_WEAPON, "EnchWeapon", 2, MOD_DT, 5, 76),
                ench(ENCH_WEAPON_ALT, "EnchWeaponAlt", 2, MOD_DT, 9, 76),
            ]
            .concat(),
        ),
        top(
            b"ARMO",
            &[
                armo(HAT_ENCH, "HatEnch", Some(ENCH_APPAREL), false),
                armo(HAT_PLAIN, "HatPlain", None, false),
                armo(HAT_PLAIN_DEST, "HatPlainDest", None, true),
            ]
            .concat(),
        ),
        top(b"WEAP", &[weap(PISTOL_ENCH, "PistolEnch", Some(ENCH_WEAPON)), weap(PISTOL_PLAIN, "PistolPlain", None)].concat()),
        top(
            b"NPC_",
            &[
                record(b"NPC_", 0, FormId::PLAYER_BASE.0, &[sub(b"EDID", z("Player"))]),
                record(b"NPC_", 0, NPC_BASE.0, &[sub(b"EDID", z("Guard")), sub(b"SPLO", ABIL_DT.0.to_le_bytes().to_vec())]),
            ]
            .concat(),
        ),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell_a, cell_b].concat()))),
    ]
    .concat();

    let plugins = vec![Plugin::parse(&bytes, "AbilitiesFastPath.esm".into()).unwrap()];
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

/// Reference oracle matching the pre-cache `effects_actors` definition verbatim.
fn reference_effects_actors(g: &Game<'_>) -> BTreeSet<FormId> {
    let placed = g.world.placed;
    let is_actor = |r: &FormId| placed.get(*r).is_some_and(|p| matches!(&p.sig, b"ACHR" | b"ACRE"));
    let mut actors: BTreeSet<FormId> = g.loaded_cells.iter().filter_map(|c| placed.by_cell.get(c)).flatten().copied().filter(is_actor).collect();
    actors.extend(g.moved.iter().filter(|r| is_actor(r)));
    actors.extend(g.created_actors());
    actors.extend(
        g.extra
            .iter()
            .filter(|(r, e)| e.current_package.is_some() && is_actor(r))
            .map(|(r, _)| *r)
            .filter(|r| g.cell_of(*r).is_some_and(|c| g.loaded_cells.contains(&c))),
    );
    actors.retain(|r| g.effects_loaded(*r));
    actors.insert(FormId::PLAYER_REF);
    actors.extend(g.effects.iter().filter(|e| e.worn.is_some() || g.is_ability_effect(e)).map(|e| e.target));
    actors.extend(g.extra.iter().filter(|(_, x)| !x.dispelled.is_empty()).map(|(r, _)| *r));
    actors
}

#[test]
fn object_effect_fast_path_preserves_armo_weap_overrides_and_clones() {
    let f = fixture();
    let mut g = game(&f);

    // Authored ARMO (enchanted, plain, plain+DEST) and WEAP (enchanted, plain).
    assert_eq!(g.object_effect(HAT_ENCH), Some(ENCH_APPAREL));
    assert_eq!(g.object_effect(HAT_PLAIN), None);
    assert_eq!(g.object_effect(HAT_PLAIN_DEST), None);
    assert_eq!(g.object_effect(PISTOL_ENCH), Some(ENCH_WEAPON));
    assert_eq!(g.object_effect(PISTOL_PLAIN), None);

    // Equip both plain and enchanted armor on the player; only the enchanted piece produces a worn effect.
    g.world.add_item(FormId::PLAYER_REF, HAT_PLAIN, 1).unwrap();
    g.world.add_item(FormId::PLAYER_REF, HAT_PLAIN_DEST, 1).unwrap();
    g.world.add_item(FormId::PLAYER_REF, HAT_ENCH, 1).unwrap();
    g.world.equip(FormId::PLAYER_REF, HAT_PLAIN).unwrap();
    g.world.equip(FormId::PLAYER_REF, HAT_PLAIN_DEST).unwrap();
    g.world.equip(FormId::PLAYER_REF, HAT_ENCH).unwrap();
    assert_eq!(g.worn_enchantments(FormId::PLAYER_REF), vec![(HAT_ENCH, ENCH_APPAREL)]);
    g.sync_loaded_actor_effects();
    assert!(g.effects.iter().any(|e| e.target == FormId::PLAYER_REF && e.worn == Some(HAT_ENCH) && e.source == ENCH_APPAREL));

    // SetObjectEffect override on plain ARMO adds an apparel enchantment; clearing removes it.
    assert!(g.set_object_effect(HAT_PLAIN, Some(ENCH_APPAREL_ALT)));
    assert_eq!(g.object_effect(HAT_PLAIN), Some(ENCH_APPAREL_ALT));
    assert_eq!(g.worn_enchantments(FormId::PLAYER_REF), vec![(HAT_ENCH, ENCH_APPAREL), (HAT_PLAIN, ENCH_APPAREL_ALT)]);
    assert!(g.effects.iter().any(|e| e.target == FormId::PLAYER_REF && e.worn == Some(HAT_PLAIN) && e.source == ENCH_APPAREL_ALT));

    assert!(g.set_object_effect(HAT_ENCH, None));
    assert_eq!(g.object_effect(HAT_ENCH), None);
    assert!(!g.effects.iter().any(|e| e.target == FormId::PLAYER_REF && e.worn == Some(HAT_ENCH)));

    // SetObjectEffect on WEAP overrides and clears the weapon enchantment.
    assert!(g.set_object_effect(PISTOL_ENCH, Some(ENCH_WEAPON_ALT)));
    assert_eq!(g.object_effect(PISTOL_ENCH), Some(ENCH_WEAPON_ALT));
    assert!(g.set_object_effect(PISTOL_ENCH, None));
    assert_eq!(g.object_effect(PISTOL_ENCH), None);

    // TempCloneForm snapshot isolates clone from post-clone edits on original.
    let clone = FormId(0xFF00_9001);
    g.clones.insert(clone, HAT_PLAIN);
    g.snapshot_object_effect(HAT_PLAIN, clone);
    assert_eq!(g.object_effect(clone), Some(ENCH_APPAREL_ALT));
    assert!(g.set_object_effect(HAT_PLAIN, None));
    assert_eq!(g.object_effect(HAT_PLAIN), None);
    assert_eq!(g.object_effect(clone), Some(ENCH_APPAREL_ALT));
}

#[test]
fn effects_actors_incremental_cache_matches_oracle_across_relocation_creation_and_dispel() {
    let f = fixture();
    let mut g = game(&f);
    g.loaded_cells.insert(CELL_A);
    g.extra.entry(ACTOR_B).or_default().current_package = Some(PKG);

    // Initial cold build: only PLAYER_REF and ACTOR_A (placed in CELL_A).
    let got = g.effects_actors();
    assert_eq!(got, reference_effects_actors(&g));
    assert!(got.contains(&ACTOR_A) && !got.contains(&ACTOR_B));
    g.sync_loaded_actor_effects();
    assert!(g.effects.iter().any(|e| e.target == ACTOR_A && e.source == ABIL_DT));

    // ACTOR_B walks from unloaded CELL_B into loaded CELL_A (via set_actor_place, no ref script).
    g.set_actor_place(ACTOR_B, Some(CELL_A), [10.0, 20.0, 0.0], 0.0).unwrap();
    let got = g.effects_actors();
    assert_eq!(got, reference_effects_actors(&g));
    assert!(got.contains(&ACTOR_B));
    g.sync_loaded_actor_effects();
    assert!(g.effects.iter().any(|e| e.target == ACTOR_B && e.source == ABIL_DT));

    // Direct world.get(r).cell mutation moving ACTOR_B back to CELL_B removes it after sync ends its ability.
    g.world.get(ACTOR_B).unwrap().cell = Some(CELL_B);
    let got = g.effects_actors();
    assert_eq!(got, reference_effects_actors(&g));
    assert!(got.contains(&ACTOR_B), "still listed while its ability effect is active so sync can end it");
    g.sync_loaded_actor_effects();
    assert!(!g.effects.iter().any(|e| e.target == ACTOR_B && e.source == ABIL_DT));
    let got = g.effects_actors();
    assert_eq!(got, reference_effects_actors(&g));
    assert!(!got.contains(&ACTOR_B));

    // Created actor spawned into CELL_A is tracked, then pruned when deleted.
    let spawned = g.world.place(NPC_BASE, Some(CELL_A), [0.0; 3], [0.0; 3], 1).unwrap();
    let got = g.effects_actors();
    assert_eq!(got, reference_effects_actors(&g));
    assert!(got.contains(&spawned));
    g.sync_loaded_actor_effects();
    assert!(g.effects.iter().any(|e| e.target == spawned && e.source == ABIL_DT));
    g.world.set_deleted(spawned, true).unwrap();
    g.sync_loaded_actor_effects();
    let got = g.effects_actors();
    assert_eq!(got, reference_effects_actors(&g));
    assert!(!got.contains(&spawned));

    // Dispel ACTOR_A's ability while loaded, then unload CELL_A: ACTOR_A must still be
    // picked up once by effects_actors so sync_abilities clears its dispelled list.
    g.run_query(
        "Begin Function {}\nActorA.Dispel AbilDT\nEnd\n",
        RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() },
    )
    .unwrap();
    assert!(g.extra.get(&ACTOR_A).is_some_and(|x| x.dispelled.contains(&ABIL_DT)));
    g.loaded_cells.clear();
    g.loaded_cells.insert(CELL_B);
    let got = g.effects_actors();
    assert_eq!(got, reference_effects_actors(&g));
    assert!(got.contains(&ACTOR_A), "dispelled actor must stay in effects_actors until unload clears dispelled");
    assert!(got.contains(&ACTOR_B));
    g.sync_loaded_actor_effects();
    assert!(g.extra.get(&ACTOR_A).is_some_and(|x| x.dispelled.is_empty()));
    let got = g.effects_actors();
    assert_eq!(got, reference_effects_actors(&g));
    assert!(!got.contains(&ACTOR_A));

    // Replacing Game.world with a fresh World sharing the same RefTable invalidates the cache:
    // relocate ACTOR_B into CELL_A first, then swap World so ACTOR_B returns to CELL_B.
    g.loaded_cells.clear();
    g.loaded_cells.insert(CELL_A);
    g.sync_loaded_actor_effects();
    g.set_actor_place(ACTOR_B, Some(CELL_A), [1.0, 2.0, 0.0], 0.0).unwrap();
    assert!(g.effects_actors().contains(&ACTOR_B));
    g.world = World::new(&f.forms, &f.refs, SimConfig::default());
    let got = g.effects_actors();
    assert_eq!(got, reference_effects_actors(&g));
    assert!(got.contains(&ACTOR_A) && !got.contains(&ACTOR_B));
}
