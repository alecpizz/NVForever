//! ShowOff NVSE `ShowOff:OnAdd` calling-reference (`GetSelfAlt` / `ThisAlt`) and
//! `GetAddedItemRefShowOff` semantics (docs/spec-notes/ext-showoff.md §1) on a synthetic
//! plugin.

use crate::common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{event, ActMode, Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::{Value, XVal};
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CELL_A: u32 = 0x200;
const CASING: u32 = 0x810;
const STIMPAK: u32 = 0x811;
const WORLD_STIMPAK_REF: FormId = FormId(0x300);

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn misc_item(id: u32, edid: &str) -> Vec<u8> {
    let data = [1i32.to_le_bytes().to_vec(), 0.1f32.to_le_bytes().to_vec()].concat();
    record(b"MISC", 0, id, &[sub(b"EDID", z(edid)), sub(b"FULL", z(edid)), sub(b"DATA", data)])
}

fn placed_item(id: FormId, edid: &str, base: u32) -> Vec<u8> {
    let data = [0.0f32; 6].iter().flat_map(|v| v.to_le_bytes()).collect();
    record(
        b"REFR",
        0,
        id.0,
        &[sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)],
    )
}

fn fixture() -> Fixture {
    let schemas = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(schemas.join("fnv")).unwrap();
    let functions = schemas.join("script/functions.json");
    let table = FunctionTable::load(&functions).unwrap();
    let catalogue: Json = serde_json::from_slice(&std::fs::read(&functions).unwrap()).unwrap();

    let player = record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))]);
    let miscs = [misc_item(CASING, "OwnedCase556mm"), misc_item(STIMPAK, "OwnedWorldMisc")].concat();
    let refs = placed_item(WORLD_STIMPAK_REF, "OwnedWorldMiscRef", STIMPAK);
    let cell = [
        record(b"CELL", 0, CELL_A, &[sub(b"EDID", z("OwnedOnAddRoom")), sub(b"DATA", vec![1])]),
        group(CELL_A.to_le_bytes(), 6, &group(CELL_A.to_le_bytes(), 9, &refs)),
    ]
    .concat();
    let bytes = [
        tes4(),
        top(b"NPC_", &player),
        top(b"MISC", &miscs),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell))),
    ]
    .concat();
    let plugins = [Plugin::parse(&bytes, "OwnedShowOffOnAdd.esm".into()).unwrap()];
    let order = LoadOrder::new(&plugins).unwrap();
    let forms = FormStore::build(&order, &schema);
    let refs = RefTable::build(&order, &schema);
    let data = GameData::build(&order, &schema, table, &catalogue, &refs);
    Fixture { data, forms, refs }
}

fn new_game(f: &Fixture) -> Game<'_> {
    let mut g = Game::new(
        &f.data,
        World::new(&f.forms, &f.refs, SimConfig::default()),
        GameConfig { strict: true, ..Default::default() },
    );
    g.player_entered(Some(FormId(CELL_A)));
    g
}

fn query(g: &mut Game<'_>, body: &str) -> XVal {
    let source = format!("Begin Function {{}}\n{body}\nEnd\n");
    g.run_query(&source, RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() })
        .unwrap_or_else(|error| panic!("{source}: {error}"))
}

#[test]
fn showoff_on_add_passes_null_this_on_add_item_and_world_ref_on_pickup() {
    let f = fixture();
    let mut g = new_game(&f);

    // Register a B42-Loot-style ShowOff:OnAdd handler filtered on 2::Player.
    query(
        &mut g,
        r#"
SetEventHandlerAlt "ShowOff:OnAdd" (Begin Function {ref rBase, ref rOwner}
    PlayerREF.AuxVarSetFlt "*onadd_calls" ((PlayerREF.AuxVarGetFlt "*onadd_calls") + 1)
    PlayerREF.AuxVarSetRef "*onadd_base" rBase
    PlayerREF.AuxVarSetRef "*onadd_owner" rOwner
    ref rSelf = GetSelfAlt
    PlayerREF.AuxVarSetRef "*onadd_self" rSelf
    PlayerREF.AuxVarSetFlt "*onadd_self_valid" (IsFormValid rSelf)
    ref rAdded = GetAddedItemRefShowOff
    PlayerREF.AuxVarSetRef "*onadd_added" rAdded
    PlayerREF.AuxVarSetFlt "*onadd_is_inv" (rAdded.IsInventoryRef)
    PlayerREF.AuxVarSetRef "*onadd_added_base" (rAdded.GetBaseObject)
    if eval IsFormValid rSelf
        PlayerREF.AuxVarSetFlt "*world_pickups" ((PlayerREF.AuxVarGetFlt "*world_pickups") + 1)
    endif
End) 2::PlayerREF
"#,
    );

    // 1) Scripted AddItem: calling reference (GetSelfAlt / ThisAlt) must be null (0),
    //    GetAddedItemRefShowOff must be an inventory reference whose base is the item,
    //    and a world-pickup guard (`IsFormValid ThisAlt`) must NOT fire.
    query(&mut g, "PlayerREF.AddItem OwnedCase556mm 1 1");
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetFlt \"*onadd_calls\")"), XVal::Num(1.0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetRef \"*onadd_base\")"), XVal::Ref(CASING));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetRef \"*onadd_owner\")"), XVal::Ref(FormId::PLAYER_REF.0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetRef \"*onadd_self\")"), XVal::Ref(0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetFlt \"*onadd_self_valid\")"), XVal::Num(0.0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetFlt \"*onadd_is_inv\")"), XVal::Num(1.0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetRef \"*onadd_added_base\")"), XVal::Ref(CASING));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetFlt \"*world_pickups\")"), XVal::Num(0.0));

    // 2) Engine-observed inventory addition (e.g. spent shell casing added by combat):
    //    same non-world semantics — GetSelfAlt is null (0) and IsInventoryRef is 1.
    g.world.add_item(FormId::PLAYER_REF, FormId(CASING), 1).unwrap();
    g.item_event(FormId::PLAYER_REF, FormId(CASING), event::ON_ADD, Some(Value::Ref(FormId::PLAYER_REF.0)));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetFlt \"*onadd_calls\")"), XVal::Num(2.0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetRef \"*onadd_self\")"), XVal::Ref(0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetFlt \"*onadd_self_valid\")"), XVal::Num(0.0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetFlt \"*onadd_is_inv\")"), XVal::Num(1.0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetFlt \"*world_pickups\")"), XVal::Num(0.0));

    // 3) Direct world item pickup (`activate(..., ActMode::Hand)` -> `Outcome::PickedUp`):
    //    GetSelfAlt / ThisAlt and GetAddedItemRefShowOff both equal the world item ref.
    g.activate(FormId::PLAYER_REF, WORLD_STIMPAK_REF, ActMode::Hand).unwrap();
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetFlt \"*onadd_calls\")"), XVal::Num(3.0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetRef \"*onadd_base\")"), XVal::Ref(STIMPAK));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetRef \"*onadd_owner\")"), XVal::Ref(FormId::PLAYER_REF.0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetRef \"*onadd_self\")"), XVal::Ref(WORLD_STIMPAK_REF.0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetFlt \"*onadd_self_valid\")"), XVal::Num(1.0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetRef \"*onadd_added\")"), XVal::Ref(WORLD_STIMPAK_REF.0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetFlt \"*onadd_is_inv\")"), XVal::Num(0.0));
    assert_eq!(query(&mut g, "SetFunctionValue (PlayerREF.AuxVarGetFlt \"*world_pickups\")"), XVal::Num(1.0));

    // Outside a handler, GetAddedItemRefShowOff returns null (0).
    assert_eq!(query(&mut g, "SetFunctionValue (GetAddedItemRefShowOff)"), XVal::Ref(0));
}
