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
fn dispatch_cache_hits_and_preserves_precedence_and_unknown_miss() {
    let (data, forms, refs) = empty_game();
    let world = World::new(&forms, &refs, SimConfig::default());
    let mut g = Game::new(&data, world, GameConfig::default());
    let ctx = RunCtx::default();

    // Early slot (disablehardcoretracking) resolves cold and warm.
    assert!(!g.player.hardcore_was_off);
    let first = call(&mut g, &ctx, "disablehardcoretracking", None, &[]);
    assert!(matches!(first, Some(Ok(XVal::Num(n))) if n == 0.0));
    assert!(g.player.hardcore_was_off);

    let cached_idx = NVSE_DISPATCH_CACHE.with(|c| c.borrow().get("disablehardcoretracking").copied());
    assert!(cached_idx.is_some(), "resolved function must populate dispatch cache");
    let second = call(&mut g, &ctx, "disablehardcoretracking", None, &[]);
    assert!(matches!(second, Some(Ok(XVal::Num(n))) if n == 0.0));

    // Tail module (ui_functions::call -> getuifloat) resolves identically cold and cached.
    let ui_args = [XVal::Str("HUDMainMenu\\missing\\x".into())];
    let r1 = call(&mut g, &ctx, "getuifloat", None, &ui_args);
    let r2 = call(&mut g, &ctx, "getuifloat", None, &ui_args);
    assert_eq!(r1, r2);
    assert!(NVSE_DISPATCH_CACHE.with(|c| c.borrow().contains_key("getuifloat")));

    // enablekey clears ITR key_masks both cold and warm.
    g.other.itr.key_masks.insert(42, std::collections::BTreeSet::from([1]));
    let _ = call(&mut g, &ctx, "enablekey", None, &[XVal::Num(42.0)]);
    assert!(!g.other.itr.key_masks.contains_key(&42));
    g.other.itr.key_masks.insert(42, std::collections::BTreeSet::from([1]));
    let _ = call(&mut g, &ctx, "enablekey", None, &[XVal::Num(42.0)]);
    assert!(!g.other.itr.key_masks.contains_key(&42));

    // Unknown function returns None and is never cached.
    assert!(call(&mut g, &ctx, "not_a_real_nvse_fn_xyz", None, &[]).is_none());
    assert!(!NVSE_DISPATCH_CACHE.with(|c| c.borrow().contains_key("not_a_real_nvse_fn_xyz")));
}
