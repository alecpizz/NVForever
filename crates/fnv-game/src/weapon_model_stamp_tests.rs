use super::*;
use crate::data::GameData;
use crate::game::{Game, GameConfig};
use fnv_esm::{LoadOrder, Schema};
use fnv_script::FunctionTable;
use fnv_sim::{FormStore, RefTable, SimConfig, World};

fn empty_game() -> (GameData, FormStore, RefTable) {
    let plugins: &[fnv_esm::Plugin] = &[];
    let lo = LoadOrder::new(plugins).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let path = root.join("script/functions.json");
    let catalog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&path).unwrap(), &catalog, &refs);
    (data, forms, refs)
}

#[test]
fn field_overrides_indexes_weapon_models_and_invalidates_stamp() {
    let (mut data, forms, refs) = empty_game();
    let weap = FormId(0x0000_4321);
    let armor = FormId(0x0000_4322);
    data.sigs.insert(weap, *b"WEAP");
    data.sigs.insert(armor, *b"ARMO");
    let world = World::new(&forms, &refs, SimConfig::default());
    let mut g = Game::new(&data, world, GameConfig::default());

    assert_eq!(g.weapon_model_stamp(), 0);
    assert_eq!(g.field_overrides.cached_weapon_model_stamp(), Some(0));

    // Unrelated field override must not dirty or change the cached stamp.
    g.field_overrides.insert((weap, "DATA/weight".into()), XVal::Num(3.5));
    assert_eq!(g.field_overrides.cached_weapon_model_stamp(), Some(0));
    assert_eq!(g.weapon_model_stamp(), 0);

    // Weapon model override invalidates cache, updates stamp, and re-caches.
    g.field_overrides.insert((weap, "MODL/".into()), XVal::Str("weapons\\pistol.nif".into()));
    assert_eq!(g.field_overrides.cached_weapon_model_stamp(), None);
    let s1 = g.weapon_model_stamp();
    assert_ne!(s1, 0);
    assert_eq!(g.field_overrides.cached_weapon_model_stamp(), Some(s1));

    // Mod-slot override (`MWD3/`, `WNM2/`) also updates the stamp.
    g.field_overrides.insert((weap, "MWD3/".into()), XVal::Str("weapons\\pistol_mod.nif".into()));
    let s2 = g.weapon_model_stamp();
    assert_ne!(s2, s1);

    // Non-WEAP MODL/ override invalidates dirty flag cleanly without altering WEAP stamp.
    g.field_overrides.insert((armor, "MODL/".into()), XVal::Str("armor\\suit.nif".into()));
    assert_eq!(g.weapon_model_stamp(), s2);

    // Removing the weapon-model overrides restores stamp back to 0.
    g.field_overrides.remove(&(weap, "MODL/".into()));
    g.field_overrides.remove(&(weap, "MWD3/".into()));
    assert_eq!(g.field_overrides.cached_weapon_model_stamp(), None);
    assert_eq!(g.weapon_model_stamp(), 0);
}
