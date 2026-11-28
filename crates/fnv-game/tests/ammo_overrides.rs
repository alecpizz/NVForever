//! Live session `AMMO` and `AMEF` trait/flag/projectile/casing/name/effect overrides,
//! `TempCloneForm` snapshot isolation, and `fnv-game` ammo consumers
//! (`Game::ammo_traits`, `Game::ammo_flag`, `Game::ammo_effect_traits`,
//! `Game::apply_ammo_effects_value`, `ApplyAmmoEffects`, `GetCalculatedWeaponDegradation`,
//! and `Game::projectile_for_launch`).

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::nvse_jip7::{AMMO_FLAG_IGNORES_NORMAL_WEAPON_RESISTANCE, AMMO_FLAG_NON_PLAYABLE};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const PROJ_A: u32 = 0x801;
const PROJ_B: u32 = 0x802;
const CASING_A: u32 = 0x803;
const CASING_B: u32 = 0x804;
const AMEF_DMG: u32 = 0x805;
const AMEF_COND: u32 = 0x806;
const AMEF_DT: u32 = 0x807;
const AMMO_A: u32 = 0x808;
const WEAP_A: u32 = 0x809;
const ROOM: u32 = 0x900;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn amef_record(id: u32, edid: &str, ty: u32, op: u32, val: f32) -> Vec<u8> {
    let data = [ty.to_le_bytes().as_slice(), &op.to_le_bytes(), &val.to_le_bytes()].concat();
    record(b"AMEF", 0, id, &[sub(b"EDID", z(edid)), sub(b"FULL", z(edid)), sub(b"DATA", data)])
}

fn fixture() -> Fixture {
    let projs = [record(b"PROJ", 0, PROJ_A, &[sub(b"EDID", z("OwnedProjA"))]), record(b"PROJ", 0, PROJ_B, &[sub(b"EDID", z("OwnedProjB"))])].concat();
    let casings = [
        record(b"MISC", 0, CASING_A, &[sub(b"EDID", z("OwnedCasingA")), sub(b"DATA", [1i32.to_le_bytes().as_slice(), &0.01f32.to_le_bytes()].concat())]),
        record(b"MISC", 0, CASING_B, &[sub(b"EDID", z("OwnedCasingB")), sub(b"DATA", [1i32.to_le_bytes().as_slice(), &0.01f32.to_le_bytes()].concat())]),
    ]
    .concat();
    let amefs = [
        // Damage Mod (0), Multiply (1), 1.5
        amef_record(AMEF_DMG, "OwnedAmefDmg", 0, 1, 1.5),
        // Weapon Condition Mod (4), Multiply (1), 2.0
        amef_record(AMEF_COND, "OwnedAmefCond", 4, 1, 2.0),
        // DT Mod (2), Subtract (2), 5.0
        amef_record(AMEF_DT, "OwnedAmefDt", 2, 2, 5.0),
    ]
    .concat();
    // AMMO DATA: speed(f32), flags(u8), unused([u8;3]), value(i32), clip_rounds(u8)
    let ammo_data = [
        1.25f32.to_le_bytes().as_slice(),
        &[AMMO_FLAG_IGNORES_NORMAL_WEAPON_RESISTANCE, 0, 0, 0],
        &10i32.to_le_bytes(),
        &[8],
    ]
    .concat();
    // AMMO DAT2: projectiles_per_shot(u32), projectile(u32), weight(f32), consumed_ammo(u32), consumed_percentage(f32)
    let ammo_dat2 = [
        2u32.to_le_bytes().as_slice(),
        &PROJ_A.to_le_bytes(),
        &0.05f32.to_le_bytes(),
        &CASING_A.to_le_bytes(),
        &0.5f32.to_le_bytes(),
    ]
    .concat();
    let ammo = record(
        b"AMMO",
        0,
        AMMO_A,
        &[
            sub(b"EDID", z("OwnedAmmoA")),
            sub(b"FULL", z("Owned Round")),
            sub(b"ONAM", z("OwnRnd")),
            sub(b"QNAM", z("OWN")),
            sub(b"DATA", ammo_data),
            sub(b"DAT2", ammo_dat2),
            sub(b"RCIL", AMEF_DMG.to_le_bytes().to_vec()),
        ],
    );
    // WEAP with override_damage_to_weapon_mult (DNAM flags2 bit 7 = 0x80) = 1.0, NAM0 = AMMO_A.
    let mut dnam = vec![0u8; 204];
    // flags2 is at offset 100 in WeaponDNAM (see jippp / small_libs): set override_damage_to_weapon_mult via schema?
    // Instead use fDamageToWeaponValue GMST = 1.0 so we don't depend on DNAM raw offset!
    dnam.clear();
    let weap = record(b"WEAP", 0, WEAP_A, &[sub(b"EDID", z("OwnedAmmoGun")), sub(b"NAM0", AMMO_A.to_le_bytes().to_vec())]);
    let gmst = record(b"GMST", 0, 0x820, &[sub(b"EDID", z("fDamageToWeaponValue")), sub(b"DATA", 1.0f32.to_le_bytes().to_vec())]);
    let room = record(b"CELL", 0, ROOM, &[sub(b"EDID", z("AmmoRoom")), sub(b"DATA", vec![1])]);
    let bytes = [
        tes4(),
        top(b"GMST", &gmst),
        top(b"PROJ", &projs),
        top(b"MISC", &casings),
        top(b"AMEF", &amefs),
        top(b"AMMO", &ammo),
        top(b"WEAP", &weap),
        top(b"NPC_", &record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))])),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &room))),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "AmmoOverridesTest.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() })
}

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(g: &mut Game, expr: &str) -> f64 {
    let body = if expr.contains('\n') { expr.to_string() } else { format!("SetFunctionValue ({expr})") };
    match q(g, &body) {
        XVal::Num(x) => x,
        XVal::Ref(r) => f64::from(r),
        v => panic!("{expr}: {v:?}"),
    }
}

fn s(g: &mut Game, expr: &str) -> String {
    match q(g, &format!("SetFunctionValue ({expr})")) {
        XVal::Str(v) => v,
        v => panic!("{expr}: {v:?}"),
    }
}

#[test]
fn ammo_traits_flags_projectile_casing_and_validation() {
    let f = fixture();
    let mut g = game(&f);

    let base = g.ammo_traits(FormId(AMMO_A)).expect("AMMO traits");
    assert_eq!(base.original, FormId(AMMO_A));
    assert!((base.speed - 1.25).abs() < 1e-6);
    assert_eq!(base.flags, AMMO_FLAG_IGNORES_NORMAL_WEAPON_RESISTANCE);
    assert!(base.ignores_normal_weapon_resistance());
    assert!(base.is_playable());
    assert!(!base.non_playable());
    assert_eq!(base.value, 10);
    assert_eq!(base.clip_rounds, 8);
    assert_eq!(base.projectiles_per_shot, 2);
    assert_eq!(base.projectile, Some(FormId(PROJ_A)));
    assert!((base.weight - 0.05).abs() < 1e-6);
    assert_eq!(base.casing, Some(FormId(CASING_A)));
    assert!((base.consumed_percentage - 0.5).abs() < 1e-6);
    assert_eq!(base.effects, vec![FormId(AMEF_DMG)]);
    assert_eq!(base.full_name.as_deref(), Some("Owned Round"));
    assert_eq!(base.short_name.as_deref(), Some("OwnRnd"));
    assert_eq!(base.abbrev.as_deref(), Some("OWN"));
    assert_eq!(g.ammo_flag(FormId(AMMO_A), 0), Some(true));
    assert_eq!(g.ammo_flag(FormId(AMMO_A), 1), Some(false));
    assert_eq!(g.ammo_flag(FormId(AMMO_A), 8), None);
    assert_eq!(g.ammo_traits(FormId(WEAP_A)), None, "non-AMMO rejected");
    assert_eq!(g.ammo_effect_traits(FormId(AMMO_A)), None, "non-AMEF rejected");

    // Mutate numeric traits, consumed percent, projectile, casing, weight, value, names, and playability flag.
    q(
        &mut g,
        r#"
        SetAmmoTraitNumeric OwnedAmmoA 0 2.5
        SetAmmoTraitNumeric OwnedAmmoA 1 7
        SetAmmoTraitNumeric OwnedAmmoA 3 24
        SetAmmoConsumedPercent 0.75 OwnedAmmoA
        SetAmmoProjectile OwnedAmmoA OwnedProjB
        SetAmmoCasing OwnedAmmoA OwnedCasingB
        SetWeight 0.125 OwnedAmmoA
        SetBaseItemValue OwnedAmmoA 42
        SetAmmoName "Match Round" 0 OwnedAmmoA
        SetAmmoName "Match" 1 OwnedAmmoA
        SetAmmoName "MTC" 2 OwnedAmmoA
        LNSetIsPlayable 0 OwnedAmmoA
        "#,
    );
    // Invalid SetAmmoProjectile target (non-PROJ) and SetAmmoConsumedPercent on non-AMMO are ignored.
    q(
        &mut g,
        r#"
        SetAmmoProjectile OwnedAmmoA OwnedCasingA
        SetAmmoConsumedPercent 0.9 OwnedAmmoGun
        "#,
    );

    let after = g.ammo_traits(FormId(AMMO_A)).unwrap();
    assert!((after.speed - 2.5).abs() < 1e-6);
    assert_eq!(after.projectiles_per_shot, 7);
    assert_eq!(after.clip_rounds, 24);
    assert!((after.consumed_percentage - 0.75).abs() < 1e-6);
    assert_eq!(after.projectile, Some(FormId(PROJ_B)), "wrong-type projectile rejected");
    assert_eq!(after.casing, Some(FormId(CASING_B)));
    assert!((after.weight - 0.125).abs() < 1e-6);
    assert_eq!(after.value, 42);
    assert_eq!(after.full_name.as_deref(), Some("Match Round"));
    assert_eq!(after.short_name.as_deref(), Some("Match"));
    assert_eq!(after.abbrev.as_deref(), Some("MTC"));
    assert!(after.ignores_normal_weapon_resistance());
    assert!(after.non_playable());
    assert_eq!(after.flags, AMMO_FLAG_IGNORES_NORMAL_WEAPON_RESISTANCE | AMMO_FLAG_NON_PLAYABLE);
    assert_eq!(g.ammo_flag(FormId(AMMO_A), 1), Some(true));
    assert_eq!(n(&mut g, "LNIsPlayable OwnedAmmoA"), 0.0);

    // Clearing projectile falls back to the weapon's projectile in Game::projectile_for_launch.
    assert_eq!(g.projectile_for_launch(FormId(WEAP_A), Some(FormId(AMMO_A)), Some(FormId(PROJ_A)), Some(FormId(PROJ_A))), (Some(FormId(PROJ_B)), Some("ammo")));
    q(&mut g, "SetAmmoProjectile OwnedAmmoA");
    assert_eq!(g.ammo_traits(FormId(AMMO_A)).unwrap().projectile, None);
    assert_eq!(g.projectile_for_launch(FormId(WEAP_A), Some(FormId(AMMO_A)), Some(FormId(PROJ_A)), Some(FormId(PROJ_A))), (Some(FormId(PROJ_A)), Some("weapon")));
}

#[test]
fn temp_clone_form_ammo_and_amef_overrides_stay_isolated_and_reach_consumers() {
    let f = fixture();
    let mut g = game(&f);

    // Pre-clone edits on AMMO_A and AMEF_DMG carry onto their clones.
    q(
        &mut g,
        r#"
        SetAmmoTraitNumeric OwnedAmmoA 0 1.75
        SetAmmoTraitNumeric OwnedAmmoA 1 4
        SetAmmoName "PreClone Abbrev" 2 OwnedAmmoA
        AddAmmoEffect OwnedAmmoA OwnedAmefCond
        SetAmmoEffectTraitNumeric OwnedAmefDmg 2 2.0
        "#,
    );
    let XVal::Ref(ammo_clone) = q(&mut g, "ref c\nlet c := TempCloneForm OwnedAmmoA 1\nSetFunctionValue c") else {
        panic!("expected cloned ammo");
    };
    let XVal::Ref(amef_clone) = q(&mut g, "ref e\nlet e := TempCloneForm OwnedAmefCond 1\nSetFunctionValue e") else {
        panic!("expected cloned amef");
    };
    let XVal::Ref(proj_clone) = q(&mut g, "ref p\nlet p := TempCloneForm OwnedProjB 1\nSetFunctionValue p") else {
        panic!("expected cloned proj");
    };

    // Post-clone edits on originals must NOT leak into ammo_clone or amef_clone.
    q(
        &mut g,
        r#"
        SetAmmoTraitNumeric OwnedAmmoA 0 9.0
        SetAmmoTraitNumeric OwnedAmmoA 3 50
        SetAmmoName "PostClone Abbrev" 2 OwnedAmmoA
        SetAmmoName "PostClone Short" 1 OwnedAmmoA
        SetAmmoProjectile OwnedAmmoA OwnedProjB
        RemoveAmmoEffect OwnedAmmoA OwnedAmefCond
        LNSetIsPlayable 0 OwnedAmmoA
        SetAmmoEffectTraitNumeric OwnedAmefCond 2 5.0
        "#,
    );
    let ac = g.ammo_traits(FormId(ammo_clone)).expect("cloned ammo traits");
    assert_eq!(ac.original, FormId(AMMO_A));
    assert!((ac.speed - 1.75).abs() < 1e-6, "inherited pre-clone speed");
    assert_eq!(ac.projectiles_per_shot, 4, "inherited pre-clone projectiles_per_shot");
    assert_eq!(ac.clip_rounds, 8, "isolated from post-clone original clip_rounds");
    assert_eq!(ac.projectile, Some(FormId(PROJ_A)), "isolated from post-clone original projectile");
    assert_eq!(ac.abbrev.as_deref(), Some("PreClone Abbrev"), "inherited pre-clone abbrev");
    assert_eq!(ac.short_name.as_deref(), Some("OwnRnd"), "isolated from post-clone original short name");
    assert!(ac.is_playable(), "isolated from post-clone original non-playable");
    assert!(!g.world.effective_non_playable(FormId(ammo_clone)), "World::effective_non_playable isolated from post-clone LNSetIsPlayable 0 on original");
    assert!(g.world.effective_non_playable(FormId(AMMO_A)), "original World::effective_non_playable reflects LNSetIsPlayable 0");
    assert_eq!(ac.effects, vec![FormId(AMEF_DMG), FormId(AMEF_COND)], "inherited pre-clone RCIL list");
    assert_eq!(s(&mut g, &format!("GetAmmoName 2 {ammo_clone:#010X}")), "PreClone Abbrev");
    assert_eq!(s(&mut g, &format!("GetAmmoName 1 {ammo_clone:#010X}")), "OwnRnd");
    assert_eq!(n(&mut g, &format!("LNIsPlayable {ammo_clone:#010X}")), 1.0);
    assert_eq!(g.ammo_effect_traits(FormId(amef_clone)).unwrap().value, 2.0, "cloned AMEF isolated from post-clone original edit");

    // Mutate ammo_clone & amef_clone directly (including SetAmmoProjectile to a cloned PROJ,
    // AddAmmoEffect with a cloned AMEF, SetAmmoEffectTraitNumeric on a cloned AMEF, and SetWeight).
    q(
        &mut g,
        &format!(
            r#"
            SetAmmoTraitNumeric {ammo_clone:#010X} 1 6
            SetAmmoProjectile {ammo_clone:#010X} {proj_clone:#010X}
            SetWeight 0.25 {ammo_clone:#010X}
            SetAmmoEffectTraitNumeric {amef_clone:#010X} 2 3.5
            RemoveAmmoEffect {ammo_clone:#010X} OwnedAmefCond
            AddAmmoEffect {ammo_clone:#010X} {amef_clone:#010X}
            AddAmmoEffect {ammo_clone:#010X} OwnedAmefDt
            "#
        ),
    );
    let ac2 = g.ammo_traits(FormId(ammo_clone)).unwrap();
    assert_eq!(ac2.projectiles_per_shot, 6);
    assert_eq!(ac2.projectile, Some(FormId(proj_clone)));
    assert!((ac2.weight - 0.25).abs() < 1e-6);
    assert_eq!(ac2.effects, vec![FormId(AMEF_DMG), FormId(amef_clone), FormId(AMEF_DT)]);
    assert_eq!(n(&mut g, &format!("GetWeight {ammo_clone:#010X}")), 0.25);
    assert_eq!(n(&mut g, "GetWeight OwnedAmmoA"), 0.05, "original weight untouched");
    assert_eq!(g.ammo_traits(FormId(AMMO_A)).unwrap().effects, vec![FormId(AMEF_DMG)], "original effects untouched");
    assert_eq!(g.projectile_for_launch(FormId(WEAP_A), Some(FormId(ammo_clone)), Some(FormId(PROJ_A)), Some(FormId(PROJ_A))), (Some(FormId(proj_clone)), Some("ammo")));

    // Live consumers: ApplyAmmoEffects & GetCalculatedWeaponDegradation see cloned AMMO and cloned AMEF live!
    assert_eq!(n(&mut g, &format!("ApplyAmmoEffects {ammo_clone:#010X} 0 10")), 20.0, "Damage Mod x2.0");
    assert_eq!(n(&mut g, &format!("ApplyAmmoEffects {ammo_clone:#010X} 2 12")), 7.0, "DT Mod -5.0 on clone only");
    assert_eq!(n(&mut g, "ApplyAmmoEffects OwnedAmmoA 2 12"), 12.0, "original has no DT Mod");
    assert_eq!(n(&mut g, &format!("GetCalculatedWeaponDegradation OwnedAmmoGun {ammo_clone:#010X}")), 3.5, "cloned AMEF Weapon Condition Mod x3.5");
    assert_eq!(n(&mut g, "GetCalculatedWeaponDegradation OwnedAmmoGun OwnedAmmoA"), 1.0, "original had OwnedAmefCond removed");

    // Clone-of-clone snapshots ammo_clone and stays isolated from subsequent ammo_clone mutations.
    let XVal::Ref(ammo_clone2) = q(&mut g, &format!("ref a1\nref a2\nlet a1 := {ammo_clone:#010X}\nlet a2 := TempCloneForm a1 1\nSetFunctionValue a2")) else {
        panic!("expected ammo_clone2");
    };
    q(&mut g, &format!("SetAmmoTraitNumeric {ammo_clone:#010X} 1 9\nRemoveAmmoEffect {ammo_clone:#010X} OwnedAmefDt"));
    let child = g.ammo_traits(FormId(ammo_clone2)).unwrap();
    assert_eq!(child.original, FormId(AMMO_A));
    assert_eq!(child.projectiles_per_shot, 6);
    assert_eq!(child.effects, vec![FormId(AMEF_DMG), FormId(amef_clone), FormId(AMEF_DT)]);
    assert_eq!(n(&mut g, &format!("ApplyAmmoEffects {ammo_clone2:#010X} 2 12")), 7.0);
}

#[test]
fn set_ammo_projectile_explicit_null_is_noop_and_omitted_arg_clears() {
    let f = fixture();
    let mut g = game(&f);

    // Baseline: OwnedAmmoA has authored projectile OwnedProjA.
    assert_eq!(q(&mut g, "SetFunctionValue (GetAmmoProjectile OwnedAmmoA)"), XVal::Ref(PROJ_A));
    assert_eq!(g.ammo_traits(FormId(AMMO_A)).unwrap().projectile, Some(FormId(PROJ_A)));

    // Explicit null ref (rNull = 0) via script MUST NOT clear the authored projectile.
    q(&mut g, "ref rNull\nlet rNull := 0\nSetAmmoProjectile OwnedAmmoA rNull");
    assert_eq!(q(&mut g, "SetFunctionValue (GetAmmoProjectile OwnedAmmoA)"), XVal::Ref(PROJ_A));
    assert_eq!(g.ammo_traits(FormId(AMMO_A)).unwrap().projectile, Some(FormId(PROJ_A)));
    assert_eq!(
        g.projectile_for_launch(FormId(WEAP_A), Some(FormId(AMMO_A)), Some(FormId(PROJ_B)), Some(FormId(PROJ_A))),
        (Some(FormId(PROJ_A)), Some("ammo"))
    );

    // Direct host calls with explicit XVal::Ref(0) or XVal::Num(0.0) are also no-ops.
    fnv_game::nvse::call(&mut g, &RunCtx::default(), "setammoprojectile", None, &[XVal::Ref(AMMO_A), XVal::Ref(0)]).unwrap().unwrap();
    fnv_game::nvse::call(&mut g, &RunCtx::default(), "setammoprojectile", None, &[XVal::Ref(AMMO_A), XVal::Num(0.0)]).unwrap().unwrap();
    assert_eq!(g.ammo_traits(FormId(AMMO_A)).unwrap().projectile, Some(FormId(PROJ_A)));

    // Omitting the optional 2nd argument clears the ammo projectile to 0 and falls back to weapon projectile.
    q(&mut g, "SetAmmoProjectile OwnedAmmoA");
    assert_eq!(q(&mut g, "SetFunctionValue (GetAmmoProjectile OwnedAmmoA)"), XVal::Ref(0));
    assert_eq!(g.ammo_traits(FormId(AMMO_A)).unwrap().projectile, None);
    assert_eq!(
        g.projectile_for_launch(FormId(WEAP_A), Some(FormId(AMMO_A)), Some(FormId(PROJ_B)), Some(FormId(PROJ_A))),
        (Some(FormId(PROJ_B)), Some("weapon"))
    );

    // After setting a session projectile (OwnedProjB), explicit null ref again leaves it unchanged,
    // while omitting the 2nd argument clears it back to 0.
    q(&mut g, "SetAmmoProjectile OwnedAmmoA OwnedProjB\nref rNull\nlet rNull := 0\nSetAmmoProjectile OwnedAmmoA rNull");
    assert_eq!(q(&mut g, "SetFunctionValue (GetAmmoProjectile OwnedAmmoA)"), XVal::Ref(PROJ_B));
    assert_eq!(g.ammo_traits(FormId(AMMO_A)).unwrap().projectile, Some(FormId(PROJ_B)));
    q(&mut g, "SetAmmoProjectile OwnedAmmoA");
    assert_eq!(q(&mut g, "SetFunctionValue (GetAmmoProjectile OwnedAmmoA)"), XVal::Ref(0));
    assert_eq!(g.ammo_traits(FormId(AMMO_A)).unwrap().projectile, None);
}
