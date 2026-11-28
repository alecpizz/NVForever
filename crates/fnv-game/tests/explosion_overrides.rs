//! Live session explosion trait/form/flag overrides (`SetExplosionTraitForm`,
//! `SetExplosionFlag`, `SetExplosionTraitNumeric`, `SetExplosionSound`), `TempCloneForm`
//! snapshot isolation, and `fnv-game` explosion consumer wiring (`Game::explosion_traits`,
//! `Game::apply_explosion_effect`, `Game::explosive_from_explosion`,
//! `Game::add_explosion_radiation_zone`, `Game::explosion_placed`,
//! `Game::explosion_hit_damage`, and `PlaceAtMe` of an `EXPL`).

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::actor_values;
use fnv_game::combat::CombatRequest;
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::nvse_jip7::{
    EXPL_FLAG_ALWAYS_WORLD_ORIENTATION, EXPL_FLAG_IGNORE_IMAGE_SPACE_SWAP, EXPL_FLAG_IGNORE_LOS_CHECK, EXPL_FLAG_KNOCK_DOWN_ALWAYS, EXPL_FLAG_KNOCK_DOWN_BY_FORMULA,
    EXPL_FLAG_PUSH_SOURCE_REF_ONLY, EXPL_FLAG_RADIUS_BS_UNITS,
};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const LIGH_A: u32 = 0x801;
const LIGH_B: u32 = 0x802;
const SOUN_A: u32 = 0x803;
const SOUN_B: u32 = 0x804;
const IPDS_A: u32 = 0x805;
const IPDS_B: u32 = 0x806;
const STAT_A: u32 = 0x807;
const STAT_B: u32 = 0x808;
const MGEF_HURT: u32 = 0x809;
const MGEF_SLOW: u32 = 0x80A;
const ENCH_A: u32 = 0x80B;
const ENCH_B: u32 = 0x80C;
const IMAD_A: u32 = 0x80D;
const IMAD_B: u32 = 0x80E;
const EXPL_A: u32 = 0x80F;
const WEAP_A: u32 = 0x810;
const NPC_BASE: u32 = 0x811;
const ROOM: u32 = 0x900;
const TARGET_REF: u32 = 0x910;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

#[allow(clippy::too_many_arguments)]
fn expl_data(force: f32, damage: f32, radius: f32, light: u32, sound_1: u32, flags: u32, is_radius: f32, ipds: u32, sound_2: u32, rad_level: f32, rad_time: f32, rad_radius: f32, sound_level: u32) -> Vec<u8> {
    [
        force.to_le_bytes().as_slice(),
        &damage.to_le_bytes(),
        &radius.to_le_bytes(),
        &light.to_le_bytes(),
        &sound_1.to_le_bytes(),
        &flags.to_le_bytes(),
        &is_radius.to_le_bytes(),
        &ipds.to_le_bytes(),
        &sound_2.to_le_bytes(),
        &rad_level.to_le_bytes(),
        &rad_time.to_le_bytes(),
        &rad_radius.to_le_bytes(),
        &sound_level.to_le_bytes(),
    ]
    .concat()
}

/// MGEF DATA: 36-byte header + flags(u32) + base_cost(f32) + assoc(u32) + school(i32) +
/// resist(i32) + counter_count(u16) + pad(u16) + light(u32) + proj_speed(f32) +
/// effect_shader(u32) + display_shader(u32) + sound_effect(u32) + sound_bolt(u32) +
/// sound_hit(u32) + sound_area(u32) + ce_enchant_factor(f32) + ce_barter_factor(f32) +
/// archetype(u32) + actor_value(i32).
fn mgef_record(id: u32, edid: &str, flags: u32, actor_value: i32) -> Vec<u8> {
    let mut data = vec![0u8; 72];
    data[0..4].copy_from_slice(&flags.to_le_bytes());
    // Archetype 0 = Value Modifier at offset 64, actor_value at offset 68.
    data[64..68].copy_from_slice(&0u32.to_le_bytes());
    data[68..72].copy_from_slice(&actor_value.to_le_bytes());
    record(b"MGEF", 0, id, &[sub(b"EDID", z(edid)), sub(b"FULL", z(edid)), sub(b"DATA", data)])
}

fn ench_record(id: u32, edid: &str, mgef: u32, magnitude: u32, duration: u32) -> Vec<u8> {
    // ENIT: type(u32)=2 Weapon, charge(u32)=0, cost(u32)=0, flags(u32)=0.
    let enit = [2u32.to_le_bytes(), 0u32.to_le_bytes(), 0u32.to_le_bytes(), 0u32.to_le_bytes()].concat();
    // EFIT: magnitude(u32), area(u32), duration(u32), type(u32)=2 Target, actor_value(i32)=-1.
    let efit = [magnitude.to_le_bytes(), 0u32.to_le_bytes(), duration.to_le_bytes(), 2u32.to_le_bytes(), (-1i32).to_le_bytes()].concat();
    record(b"ENCH", 0, id, &[sub(b"EDID", z(edid)), sub(b"FULL", z(edid)), sub(b"ENIT", enit), sub(b"EFID", mgef.to_le_bytes().to_vec()), sub(b"EFIT", efit)])
}

fn fixture() -> Fixture {
    let lights = [record(b"LIGH", 0, LIGH_A, &[sub(b"EDID", z("BlastLightA"))]), record(b"LIGH", 0, LIGH_B, &[sub(b"EDID", z("BlastLightB"))])].concat();
    let sounds = [record(b"SOUN", 0, SOUN_A, &[sub(b"EDID", z("BlastSoundA"))]), record(b"SOUN", 0, SOUN_B, &[sub(b"EDID", z("BlastSoundB"))])].concat();
    let ipds = [record(b"IPDS", 0, IPDS_A, &[sub(b"EDID", z("BlastImpactA"))]), record(b"IPDS", 0, IPDS_B, &[sub(b"EDID", z("BlastImpactB"))])].concat();
    let statics = [record(b"STAT", 0, STAT_A, &[sub(b"EDID", z("BlastPropA"))]), record(b"STAT", 0, STAT_B, &[sub(b"EDID", z("BlastPropB"))])].concat();
    // MGEF flags: 0x01 Hostile, 0x04 Detrimental.
    let mgefs = [
        mgef_record(MGEF_HURT, "BlastDamageHealth", 0x05, i32::from(actor_values::HEALTH)),
        mgef_record(MGEF_SLOW, "BlastDrainAgility", 0x05, i32::from(actor_values::AGILITY)),
    ]
    .concat();
    let enchs = [ench_record(ENCH_A, "BlastEnchA", MGEF_HURT, 25, 10), ench_record(ENCH_B, "BlastEnchB", MGEF_SLOW, 3, 15)].concat();
    let imads = [record(b"IMAD", 0, IMAD_A, &[sub(b"EDID", z("BlastImadA"))]), record(b"IMAD", 0, IMAD_B, &[sub(b"EDID", z("BlastImadB"))])].concat();
    let expl = record(
        b"EXPL",
        0,
        EXPL_A,
        &[
            sub(b"EDID", z("OwnedBlastExpl")),
            sub(b"MODL", z("Effects\\OwnedBlastA.NIF")),
            sub(b"EITM", ENCH_A.to_le_bytes().to_vec()),
            sub(b"MNAM", IMAD_A.to_le_bytes().to_vec()),
            sub(b"DATA", expl_data(120.0, 80.0, 400.0, LIGH_A, SOUN_A, EXPL_FLAG_RADIUS_BS_UNITS | EXPL_FLAG_KNOCK_DOWN_BY_FORMULA, 600.0, IPDS_A, SOUN_B, 0.0, 0.0, 0.0, 1)),
            sub(b"INAM", STAT_A.to_le_bytes().to_vec()),
        ],
    );
    let weap = record(b"WEAP", 0, WEAP_A, &[sub(b"EDID", z("OwnedGuardWeap")), sub(b"EITM", ENCH_A.to_le_bytes().to_vec())]);
    let npc_data = [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat();
    let npcs = [
        record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]),
        record(b"NPC_", 0, NPC_BASE, &[sub(b"EDID", z("BlastTargetBase")), sub(b"FULL", z("Target")), sub(b"DATA", npc_data)]),
    ]
    .concat();
    let room = record(b"CELL", 0, ROOM, &[sub(b"EDID", z("BlastRoom")), sub(b"DATA", vec![1])]);
    let target_ref = record(b"ACHR", 0, TARGET_REF, &[sub(b"EDID", z("BlastTargetRef")), sub(b"NAME", NPC_BASE.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let children = group(ROOM.to_le_bytes(), 6, &group(ROOM.to_le_bytes(), 8, &target_ref));
    let bytes = [
        tes4(),
        top(b"LIGH", &lights),
        top(b"SOUN", &sounds),
        top(b"IPDS", &ipds),
        top(b"STAT", &statics),
        top(b"MGEF", &mgefs),
        top(b"ENCH", &enchs),
        top(b"IMAD", &imads),
        top(b"EXPL", &expl),
        top(b"WEAP", &weap),
        top(b"NPC_", &npcs),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[room, children].concat()))),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "ExplosionOverridesTest.esm".into()).unwrap()];
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
    let mut world = World::new(&f.forms, &f.refs, SimConfig::default());
    let _ = world.get(FormId::PLAYER_REF).map(|p| p.cell = Some(FormId(ROOM)));
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

#[test]
fn explosion_trait_form_and_flag_overrides_readback_and_validation() {
    let f = fixture();
    let mut g = game(&f);

    // Authored baseline through getters and Game::explosion_traits.
    let base = g.explosion_traits(FormId(EXPL_A)).expect("EXPL traits");
    assert_eq!(base.original, FormId(EXPL_A));
    assert_eq!((base.force, base.damage, base.radius, base.is_radius), (120.0, 80.0, 400.0, 600.0));
    assert_eq!(base.sound_level, 1, "authored Normal");
    assert_eq!(base.light, Some(FormId(LIGH_A)));
    assert_eq!(base.sound_1, Some(FormId(SOUN_A)));
    assert_eq!(base.sound_2, Some(FormId(SOUN_B)));
    assert_eq!(base.impact_dataset, Some(FormId(IPDS_A)));
    assert_eq!(base.placed_impact, Some(FormId(STAT_A)));
    assert_eq!(base.enchantment, Some(FormId(ENCH_A)));
    assert_eq!(base.image_space_modifier, Some(FormId(IMAD_A)));
    assert_eq!(base.model.as_deref(), Some("Effects\\OwnedBlastA.NIF"));
    assert!(base.radius_in_bs_units());
    assert!(base.knock_down_by_formula());
    assert!(!base.ignore_los());
    assert_eq!(g.explosion_flag(FormId(EXPL_A), 0), None, "flag 0 unreachable");
    assert_eq!(g.explosion_flag(FormId(EXPL_A), 3), Some(true));
    assert_eq!(g.explosion_flag(FormId(EXPL_A), 4), Some(false));
    assert_eq!(g.explosion_traits(FormId(WEAP_A)), None, "non-EXPL rejected");

    // Override all 7 form traits (0..=6) and verify getter + GetObjectEffect + typed readback.
    q(
        &mut g,
        r#"
        SetExplosionTraitForm OwnedBlastExpl 0 BlastLightB
        SetExplosionTraitForm OwnedBlastExpl 1 BlastSoundB
        SetExplosionTraitForm OwnedBlastExpl 2 BlastSoundA
        SetExplosionTraitForm OwnedBlastExpl 3 BlastImpactB
        SetExplosionTraitForm OwnedBlastExpl 4 BlastPropB
        SetExplosionTraitForm OwnedBlastExpl 5 BlastEnchB
        SetExplosionTraitForm OwnedBlastExpl 6 BlastImadB
        "#,
    );
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 0"), f64::from(LIGH_B));
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 1"), f64::from(SOUN_B));
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 2"), f64::from(SOUN_A));
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 3"), f64::from(IPDS_B));
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 4"), f64::from(STAT_B));
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 5"), f64::from(ENCH_B));
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 6"), f64::from(IMAD_B));
    assert_eq!(n(&mut g, "GetObjectEffect OwnedBlastExpl"), f64::from(ENCH_B));

    let after = g.explosion_traits(FormId(EXPL_A)).unwrap();
    assert_eq!(after.light, Some(FormId(LIGH_B)));
    assert_eq!(after.sound_1, Some(FormId(SOUN_B)));
    assert_eq!(after.sound_2, Some(FormId(SOUN_A)));
    assert_eq!(after.impact_dataset, Some(FormId(IPDS_B)));
    assert_eq!(after.placed_impact, Some(FormId(STAT_B)));
    assert_eq!(after.enchantment, Some(FormId(ENCH_B)));
    assert_eq!(after.image_space_modifier, Some(FormId(IMAD_B)));

    // Wrong-type target forms on typed EXPL slots clear those slots to 0/None
    // [observed: jip_24_expl_trait_form_type_check], while non-EXPL forms remain untouched.
    q(
        &mut g,
        r#"
        SetExplosionTraitForm OwnedBlastExpl 0 BlastSoundA
        SetExplosionTraitForm OwnedBlastExpl 1 BlastLightA
        SetExplosionTraitForm OwnedBlastExpl 3 BlastPropA
        SetExplosionTraitForm OwnedBlastExpl 5 BlastImadA
        SetExplosionTraitForm OwnedBlastExpl 6 BlastEnchA
        SetExplosionTraitForm OwnedGuardWeap 5 BlastEnchB
        SetExplosionFlag OwnedGuardWeap 2 1
        "#,
    );
    assert_eq!(g.explosion_traits(FormId(EXPL_A)).unwrap().light, None, "wrong-type clears slot 0");
    assert_eq!(g.explosion_traits(FormId(EXPL_A)).unwrap().sound_1, None, "wrong-type clears slot 1");
    assert_eq!(g.explosion_traits(FormId(EXPL_A)).unwrap().impact_dataset, None, "wrong-type clears slot 3");
    assert_eq!(g.explosion_traits(FormId(EXPL_A)).unwrap().enchantment, None, "wrong-type clears slot 5");
    assert_eq!(g.explosion_traits(FormId(EXPL_A)).unwrap().image_space_modifier, None, "wrong-type clears slot 6");
    assert_eq!(n(&mut g, "GetObjectEffect OwnedGuardWeap"), f64::from(ENCH_A), "unrelated WEAP EITM untouched");

    // Slot 4 (INAM) accepts any non-null form without type checking
    // [observed: jip_24_expl_trait_form_type_check, w4d_27_expl_inam_placeable].
    q(
        &mut g,
        r#"
        SetExplosionTraitForm OwnedBlastExpl 0 BlastLightB
        SetExplosionTraitForm OwnedBlastExpl 4 BlastEnchA
        SetExplosionTraitForm OwnedBlastExpl 5 BlastEnchB
        SetExplosionTraitForm OwnedBlastExpl 6 BlastImadB
        "#,
    );
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 4"), f64::from(ENCH_A));
    assert_eq!(g.explosion_traits(FormId(EXPL_A)).unwrap().placed_impact, Some(FormId(ENCH_A)));

    // Passing an explicit null ref variable or 0 is a no-op across all slots (0, 4, 5, 6)
    // [observed: w4h_02_explosion_slot4_reconcile, w4d_27_expl_inam_placeable].
    q(
        &mut g,
        r#"
        ref rNull
        let rNull := 0
        SetExplosionTraitForm OwnedBlastExpl 0 rNull
        SetExplosionTraitForm OwnedBlastExpl 4 rNull
        SetExplosionTraitForm OwnedBlastExpl 5 rNull
        SetExplosionTraitForm OwnedBlastExpl 6 rNull
        SetExplosionTraitForm OwnedBlastExpl 0 0
        SetExplosionTraitForm OwnedBlastExpl 4 0
        SetExplosionTraitForm OwnedBlastExpl 5 0
        SetExplosionTraitForm OwnedBlastExpl 6 0
        "#,
    );
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 0"), f64::from(LIGH_B), "explicit null/0 must not clear slot 0");
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 4"), f64::from(ENCH_A), "explicit null/0 must not clear slot 4");
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 5"), f64::from(ENCH_B), "explicit null/0 must not clear slot 5");
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 6"), f64::from(IMAD_B), "explicit null/0 must not clear slot 6");

    // Omitting the 3rd argument clears every slot (including slot 4) to 0/None
    // [observed: gaps7, w4h_02_explosion_slot4_reconcile].
    q(
        &mut g,
        r#"
        SetExplosionTraitForm OwnedBlastExpl 0
        SetExplosionTraitForm OwnedBlastExpl 4
        SetExplosionTraitForm OwnedBlastExpl 5
        SetExplosionTraitForm OwnedBlastExpl 6
        "#,
    );
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 0"), 0.0);
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 4"), 0.0);
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 5"), 0.0);
    assert_eq!(n(&mut g, "GetExplosionTraitForm OwnedBlastExpl 6"), 0.0);
    assert_eq!(n(&mut g, "GetObjectEffect OwnedBlastExpl"), 0.0);
    let cleared = g.explosion_traits(FormId(EXPL_A)).unwrap();
    assert_eq!((cleared.light, cleared.placed_impact, cleared.enchantment, cleared.image_space_modifier), (None, None, None, None));

    // SetExplosionFlag across documented flags 1..=6, preserving bit 0 (Radius in BS Units).
    q(
        &mut g,
        r#"
        SetExplosionFlag OwnedBlastExpl 0 0
        SetExplosionFlag OwnedBlastExpl 1 1
        SetExplosionFlag OwnedBlastExpl 2 1
        SetExplosionFlag OwnedBlastExpl 3 0
        SetExplosionFlag OwnedBlastExpl 4 1
        SetExplosionFlag OwnedBlastExpl 5 1
        SetExplosionFlag OwnedBlastExpl 6 1
        "#,
    );
    let flagged = g.explosion_traits(FormId(EXPL_A)).unwrap();
    assert!(flagged.radius_in_bs_units(), "bit 0 cannot be cleared via SetExplosionFlag 0");
    assert!(flagged.always_world_orientation());
    assert!(flagged.knock_down_always());
    assert!(!flagged.knock_down_by_formula());
    assert!(flagged.ignore_los());
    assert!(flagged.push_source_only());
    assert!(flagged.ignore_image_space_swap());
    assert_eq!(
        flagged.flags,
        EXPL_FLAG_RADIUS_BS_UNITS | EXPL_FLAG_ALWAYS_WORLD_ORIENTATION | EXPL_FLAG_KNOCK_DOWN_ALWAYS | EXPL_FLAG_IGNORE_LOS_CHECK | EXPL_FLAG_PUSH_SOURCE_REF_ONLY | EXPL_FLAG_IGNORE_IMAGE_SPACE_SWAP
    );
    for (bit, want) in [(1, 1.0), (2, 1.0), (3, 0.0), (4, 1.0), (5, 1.0), (6, 1.0)] {
        assert_eq!(n(&mut g, &format!("GetExplosionFlag OwnedBlastExpl {bit}")), want, "flag {bit}");
        assert_eq!(g.explosion_flag(FormId(EXPL_A), bit), Some(want != 0.0));
    }
}

#[test]
fn temp_clone_form_explosion_overrides_stay_isolated_in_both_directions() {
    let f = fixture();
    let mut g = game(&f);

    // Pre-clone edit on original must carry onto clone1.
    q(
        &mut g,
        r#"
        SetExplosionTraitForm OwnedBlastExpl 0 BlastLightB
        SetExplosionFlag OwnedBlastExpl 4 1
        SetExplosionTraitNumeric OwnedBlastExpl 2 900
        "#,
    );
    let XVal::Ref(clone1) = q(&mut g, "ref c\nlet c := TempCloneForm OwnedBlastExpl 1\nSetFunctionValue c") else {
        panic!("expected clone1 ref");
    };
    let XVal::Ref(sound_clone) = q(&mut g, "ref s\nlet s := TempCloneForm BlastSoundA 1\nSetFunctionValue s") else {
        panic!("expected cloned sound ref");
    };

    // Post-clone edits on original (including previously untouched fields!) must NOT leak into clone1.
    q(
        &mut g,
        r#"
        SetExplosionTraitForm OwnedBlastExpl 0 BlastLightA
        SetExplosionTraitForm OwnedBlastExpl 5 BlastEnchB
        SetExplosionFlag OwnedBlastExpl 4 0
        SetExplosionFlag OwnedBlastExpl 2 1
        SetExplosionTraitNumeric OwnedBlastExpl 1 999
        SetExplosionTraitNumeric OwnedBlastExpl 2 1200
        "#,
    );
    let c1 = g.explosion_traits(FormId(clone1)).expect("clone1 traits");
    assert_eq!(c1.original, FormId(EXPL_A));
    assert_eq!(c1.light, Some(FormId(LIGH_B)), "inherited pre-clone light edit");
    assert_eq!(c1.enchantment, Some(FormId(ENCH_A)), "isolated from post-clone original EITM edit");
    assert!(c1.ignore_los(), "inherited pre-clone flag 4");
    assert!(!c1.knock_down_always(), "isolated from post-clone original flag 2");
    assert_eq!(c1.damage, 80.0, "isolated from post-clone original damage edit");
    assert_eq!(c1.radius, 900.0, "inherited pre-clone radius edit");
    assert_eq!(n(&mut g, &format!("GetExplosionTraitForm {clone1:#010X} 0")), f64::from(LIGH_B));
    assert_eq!(n(&mut g, &format!("GetExplosionTraitForm {clone1:#010X} 5")), f64::from(ENCH_A));
    assert_eq!(n(&mut g, &format!("GetExplosionFlag {clone1:#010X} 4")), 1.0);
    assert_eq!(n(&mut g, &format!("GetExplosionFlag {clone1:#010X} 2")), 0.0);

    // Post-clone edits on clone1 (including SetExplosionSound with a cloned SOUN & SetExplosionTraitNumeric)
    // must NOT leak back into original, and a child clone2 snapshots clone1's state cleanly.
    q(
        &mut g,
        &format!(
            r#"
            ref c1
            ref sc
            let c1 := {clone1:#010X}
            let sc := {sound_clone:#010X}
            SetExplosionSound c1 1 sc
            SetExplosionTraitNumeric c1 1 222
            SetExplosionFlag c1 6 1
            "#
        ),
    );
    let XVal::Ref(clone2) = q(&mut g, &format!("ref c1\nref c2\nlet c1 := {clone1:#010X}\nlet c2 := TempCloneForm c1 1\nSetFunctionValue c2")) else {
        panic!("expected clone2 ref");
    };
    q(&mut g, &format!("ref c1\nlet c1 := {clone1:#010X}\nSetExplosionTraitNumeric c1 1 333\nSetExplosionFlag c1 6 0"));

    assert_eq!(g.explosion_traits(FormId(EXPL_A)).unwrap().sound_1, Some(FormId(SOUN_A)));
    assert_eq!(g.explosion_traits(FormId(EXPL_A)).unwrap().damage, 999.0);
    assert!(!g.explosion_traits(FormId(EXPL_A)).unwrap().ignore_image_space_swap());

    let c2 = g.explosion_traits(FormId(clone2)).expect("clone2 traits");
    assert_eq!(c2.original, FormId(EXPL_A));
    assert_eq!(c2.sound_1, Some(FormId(sound_clone)));
    assert_eq!(c2.damage, 222.0, "clone2 keeps snapshot from before clone1 mutated to 333");
    assert!(c2.ignore_image_space_swap(), "clone2 keeps flag 6 snapshot");
}

#[test]
fn live_explosion_overrides_reach_effect_pickpocket_radiation_placement_and_hit_consumers() {
    let f = fixture();
    let mut g = game(&f);

    // 1. Live EITM override reaches Game::apply_explosion_effect.
    let ids_a = g.apply_explosion_effect(FormId(EXPL_A), FormId(TARGET_REF), Some(FormId::PLAYER_REF));
    assert_eq!(ids_a.len(), 1);
    assert_eq!(n(&mut g, "BlastTargetRef.HasMagicEffect BlastDamageHealth"), 1.0);
    assert_eq!(n(&mut g, "BlastTargetRef.HasMagicEffect BlastDrainAgility"), 0.0);

    q(&mut g, "SetExplosionTraitForm OwnedBlastExpl 5 BlastEnchB");
    let ids_b = g.apply_explosion_effect(FormId(EXPL_A), FormId(TARGET_REF), Some(FormId::PLAYER_REF));
    assert_eq!(ids_b.len(), 1);
    assert_eq!(n(&mut g, "BlastTargetRef.HasMagicEffect BlastDrainAgility"), 1.0, "live EITM swap applied BlastEnchB");

    q(&mut g, "SetExplosionTraitForm OwnedBlastExpl 5 0");
    assert_eq!(g.explosion_traits(FormId(EXPL_A)).unwrap().enchantment, Some(FormId(ENCH_B)), "explicit 0 does not clear EITM");

    q(&mut g, "SetExplosionTraitForm OwnedBlastExpl 5");
    assert!(g.apply_explosion_effect(FormId(EXPL_A), FormId(TARGET_REF), Some(FormId::PLAYER_REF)).is_empty(), "omitting 3rd arg clears EITM and applies no effect");

    // 2. Live damage + EITM overrides reach Game::explosive_from_explosion -> explosive_harms.
    q(&mut g, "SetExplosionTraitNumeric OwnedBlastExpl 1 0");
    let harmless = g.explosive_from_explosion(FormId(EXPL_A), None).unwrap();
    assert!(!g.explosive_harms(FormId(TARGET_REF), &harmless), "0 damage + cleared EITM does not harm");
    q(&mut g, "SetExplosionTraitForm OwnedBlastExpl 5 BlastEnchA");
    let lethal_by_ench = g.explosive_from_explosion(FormId(EXPL_A), None).unwrap();
    assert!(g.explosive_harms(FormId(TARGET_REF), &lethal_by_ench), "live Health-damaging EITM makes planted explosive lethal");

    // 3. Live radiation trait overrides reach Game::add_explosion_radiation_zone.
    assert!(g.add_explosion_radiation_zone(FormId(EXPL_A), [0.0; 3]).is_none(), "0 authored radiation leaves no zone");
    q(
        &mut g,
        r#"
        SetExplosionTraitNumeric OwnedBlastExpl 4 15
        SetExplosionTraitNumeric OwnedBlastExpl 5 20
        SetExplosionTraitNumeric OwnedBlastExpl 6 512
        "#,
    );
    let zone = g.add_explosion_radiation_zone(FormId(EXPL_A), [10.0, 20.0, 0.0]).expect("live radiation zone");
    assert_eq!((zone.level, zone.duration, zone.radius), (15.0, 20.0, 512.0));

    // 4. Live radius & damage overrides reach Game::explosion_placed (including clones) and ShowOff:OnExplosionHit.
    let XVal::Ref(clone) = q(&mut g, "ref c\nlet c := TempCloneForm OwnedBlastExpl 1\nSetFunctionValue c") else {
        panic!("expected clone ref");
    };
    q(
        &mut g,
        &format!(
            r#"
            ref c
            let c := {clone:#010X}
            SetExplosionTraitNumeric c 1 135
            SetExplosionTraitNumeric c 2 777
            "#
        ),
    );
    let live_ref = g.explosion_placed(FormId(clone), Some(FormId::PLAYER_REF), [0.0; 3], 400.0).expect("placed live explosion from clone");
    assert_eq!(g.explosion_radius(live_ref), Some(777.0), "live DATA/radius override wins over stale caller radius");
    let got_radius = g
        .run_query("Begin Function { }\nSetFunctionValue (GetExplosionRefRadius)\nEnd\n", RunCtx { self_ref: Some(live_ref), ..RunCtx::default() })
        .unwrap();
    assert_eq!(got_radius, XVal::Num(777.0));
    assert_eq!(g.explosion_hit_damage(FormId(clone), FormId(TARGET_REF)), Some(135.0));

    // 5. PlaceAtMe of an authored EXPL emits CombatRequest::Explode resolving live overrides and caller source.
    g.combat_requests.clear();
    q(&mut g, "player.PlaceAtMe OwnedBlastExpl 1");
    assert!(matches!(
        g.combat_requests.as_slice(),
        [CombatRequest::Explode { explosion, source, .. }]
            if *explosion == FormId(EXPL_A) && *source == Some(FormId::PLAYER_REF)
    ));
}

#[test]
fn unhandled_explosion_hit_damage_does_not_spawn_explosion_ref() {
    let f = fixture();
    let mut g = game(&f);

    let before = g.explosion_placed(FormId(EXPL_A), None, [0.0; 3], 400.0).expect("baseline ref");
    assert_eq!(g.explosion_hit_damage(FormId(EXPL_A), FormId(TARGET_REF)), Some(80.0));
    let after = g.explosion_placed(FormId(EXPL_A), None, [0.0; 3], 400.0).expect("next ref");
    assert_eq!(after.0, before.0 + 1, "unhandled explosion_hit_damage must not allocate a live explosion ref");
}

#[test]
fn showoff_on_explosion_hit_passes_explosion_ref_as_self_and_source_actor() {
    let f = fixture();
    let mut g = game(&f);

    let expl_ref = g
        .explosion_placed(FormId(EXPL_A), Some(FormId::PLAYER_REF), [0.0; 3], 400.0)
        .expect("placed live explosion reference");

    q(
        &mut g,
        r#"
        SetEventHandler "ShowOff:OnExplosionHit" (Begin Function { ref rHit, ref rSrc }
            ref rSelf
            ref rBase
            let rSelf := GetSelfAlt
            let rBase := rSelf.GetBaseObject
            Player.AuxVarSetRef "*eh_self" rSelf
            Player.AuxVarSetRef "*eh_base" rBase
            Player.AuxVarSetRef "*eh_hit" rHit
            Player.AuxVarSetRef "*eh_src" rSrc
            Player.AuxVarSetFlt "*eh_src_zero" (rSrc == 0)
            Player.AuxVarSetFlt "*eh_base_dmg" (GetExplosionTraitNumeric rBase 1)
            Player.AuxVarSetFlt "*eh_in_dmg" (GetExplosionHitDamage)
            SetExplosionHitDamage 42.5
        End)
        "#,
    );

    let out = g.explosion_hit_with_context(Some(expl_ref), FormId(TARGET_REF), Some(FormId::PLAYER_REF), 100.0);
    assert_eq!(out, 42.5);
    assert_eq!(q(&mut g, "SetFunctionValue (Player.AuxVarGetRef \"*eh_self\")"), XVal::Ref(expl_ref.0));
    assert_eq!(q(&mut g, "SetFunctionValue (Player.AuxVarGetRef \"*eh_base\")"), XVal::Ref(EXPL_A));
    assert_eq!(q(&mut g, "SetFunctionValue (Player.AuxVarGetRef \"*eh_hit\")"), XVal::Ref(TARGET_REF));
    assert_eq!(q(&mut g, "SetFunctionValue (Player.AuxVarGetRef \"*eh_src\")"), XVal::Ref(FormId::PLAYER_REF.0));
    assert_eq!(q(&mut g, "SetFunctionValue (Player.AuxVarGetFlt \"*eh_src_zero\")"), XVal::Num(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (Player.AuxVarGetFlt \"*eh_base_dmg\")"), XVal::Num(80.0));
    assert_eq!(q(&mut g, "SetFunctionValue (Player.AuxVarGetFlt \"*eh_in_dmg\")"), XVal::Num(100.0));

    // When no source actor is provided, arg2 is null ref (0) and GetSelfAlt stays the explosion ref.
    let out2 = g.explosion_hit_with_context(Some(expl_ref), FormId(TARGET_REF), None, 60.0);
    assert_eq!(out2, 42.5);
    assert_eq!(q(&mut g, "SetFunctionValue (Player.AuxVarGetRef \"*eh_self\")"), XVal::Ref(expl_ref.0));
    assert_eq!(q(&mut g, "SetFunctionValue (Player.AuxVarGetFlt \"*eh_src_zero\")"), XVal::Num(1.0));
}
