use crate::common::*;
use crate::{game::GameConfig, Game, GameData};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_script::{vm::XVal, FunctionTable};
use fnv_sim::{FormStore, RefTable, SimConfig, World};

const MGEF_HARM: u32 = 0x1801;
const MGEF_HEAL: u32 = 0x1802;
const SPEL_POIS: u32 = 0x1803;
const SPEL_FIRE: u32 = 0x1804;
const ALCH_POIS: u32 = 0x1805;
const ALCH_HEAL: u32 = 0x1806;

struct Fixture { data: GameData, forms: FormStore, refs: RefTable }

fn mgef_rec(fid: u32, name: &str, flags: u32) -> Vec<u8> {
    let mut d = vec![0u8; 72];
    d[0..4].copy_from_slice(&flags.to_le_bytes());
    d[68..72].copy_from_slice(&16i32.to_le_bytes());
    record(b"MGEF", 0, fid, &[sub(b"EDID", z(name)), sub(b"DATA", d)])
}

fn fixture() -> Fixture {
    let efit = [u32s(&[1, 0, 10, 0]), 16i32.to_le_bytes().to_vec()].concat();
    let spit_pois = [u32s(&[5, 0, 0]), vec![0u8; 4]].concat(); // Type 5 Poison
    let spit_fire = [u32s(&[0, 0, 0]), vec![0u8; 4]].concat(); // Type 0 ActorEffect
    let enit = [u32s(&[0]), vec![0u8; 16]].concat();
    let bytes = [
        tes4(),
        top(b"MGEF", &[mgef_rec(MGEF_HARM, "PsHarm", 0x5), mgef_rec(MGEF_HEAL, "PsHeal", 0x0)].concat()),
        top(b"SPEL", &[
            record(b"SPEL", 0, SPEL_POIS, &[sub(b"EDID", z("PsVenom")), sub(b"SPIT", spit_pois), sub(b"EFID", u32s(&[MGEF_HARM])), sub(b"EFIT", efit.clone())]),
            record(b"SPEL", 0, SPEL_FIRE, &[sub(b"EDID", z("PsFlamer")), sub(b"SPIT", spit_fire), sub(b"EFID", u32s(&[MGEF_HARM])), sub(b"EFIT", efit.clone())]),
        ].concat()),
        top(b"ALCH", &[
            record(b"ALCH", 0, ALCH_POIS, &[sub(b"EDID", z("PsAlchPois")), sub(b"ENIT", enit.clone()), sub(b"EFID", u32s(&[MGEF_HARM])), sub(b"EFIT", efit.clone())]),
            record(b"ALCH", 0, ALCH_HEAL, &[sub(b"EDID", z("PsAlchHeal")), sub(b"ENIT", enit), sub(b"EFID", u32s(&[MGEF_HEAL])), sub(b"EFIT", efit)]),
        ].concat()),
    ].concat();
    let plugins = [Plugin::parse(&bytes, "W6D12Poisoned.esm".into()).unwrap()];
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

fn qn(g: &mut Game<'_>, expr: &str) -> f64 {
    let src = format!("Begin Function {{}}
SetFunctionValue ({expr})
End
");
    match g.run_query(&src, Default::default()).unwrap() {
        XVal::Num(n) => n,
        other => panic!("expected Num for {expr}, got {other:?}"),
    }
}

#[test]
fn getispoisoned_detects_spel_poison_alch_poison_and_hostile_detrimental_spel() {
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() });
    assert_eq!(qn(&mut g, "PlayerREF.GetIsPoisoned"), 0.0);
    // SPEL type 5 Poison
    g.run_query("Begin Function {}
PlayerREF.CastImmediateOnSelf PsVenom
End
", Default::default()).unwrap();
    assert_eq!(qn(&mut g, "PlayerREF.GetIsPoisoned"), 1.0);
    g.run_query("Begin Function {}
PlayerREF.Dispel PsVenom
End
", Default::default()).unwrap();
    assert_eq!(qn(&mut g, "PlayerREF.GetIsPoisoned"), 0.0);
    // ALCH Poison
    g.run_query("Begin Function {}
PlayerREF.CastImmediateOnSelf PsAlchPois
End
", Default::default()).unwrap();
    assert_eq!(qn(&mut g, "PlayerREF.GetIsPoisoned"), 1.0);
    g.run_query("Begin Function {}
PlayerREF.Dispel PsAlchPois
End
", Default::default()).unwrap();
    // SPEL type 0 with Hostile+Detrimental effect (SpellFlamerEffect)
    g.run_query("Begin Function {}
PlayerREF.CastImmediateOnSelf PsFlamer
End
", Default::default()).unwrap();
    assert_eq!(qn(&mut g, "PlayerREF.GetIsPoisoned"), 1.0);
    g.run_query("Begin Function {}
PlayerREF.Dispel PsFlamer
End
", Default::default()).unwrap();
    // Beneficial ALCH stays 0
    g.run_query("Begin Function {}
PlayerREF.CastImmediateOnSelf PsAlchHeal
End
", Default::default()).unwrap();
    assert_eq!(qn(&mut g, "PlayerREF.GetIsPoisoned"), 0.0);
}
