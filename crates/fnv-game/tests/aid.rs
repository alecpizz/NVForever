//! Aid items on a synthetic plugin (no game data): value modifiers held while an effect
//! lasts, restoration at once and by the second, Medicine and Food potency, rads, the
//! usage monitor, addiction and its cure, abilities in the permanent pool.
//! docs/spec-notes/aid-items.md.

use crate::common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::functions::{actor_value, add_actor_value_damage, set_actor_value_current, Ctx};
use fnv_game::game::{Game, GameConfig};
use fnv_game::trace::EventKind;
use fnv_game::GameData;
use fnv_script::vm::{ArgValue, Value};
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use serde_json::Value as Json;

const CELL: u32 = 0x200;
const MOD_STR: u32 = 0x850;
const RESTORE_HEALTH: u32 = 0x851;
const PARTS: u32 = 0x852;
const RESTORE_RADS: u32 = 0x853;
const REDUCE_STR: u32 = 0x854;
const CURE: u32 = 0x855;
const STIM: u32 = 0x860;
const STIM_HC: u32 = 0x861;
const BUFF: u32 = 0x862;
const RADAWAY: u32 = 0x863;
const FIXER: u32 = 0x864;
const FOOD: u32 = 0x865;
const WITHDRAWAL: u32 = 0x870;
const ABILITY: u32 = 0x871;
const UMON: u32 = 0x14F;
const CHAMELEON_FX: u32 = 0x856;
const DAMAGE_RADS: u32 = 0x857;
const RESTORE_HUNGER: u32 = 0x858;
const STEALTH: u32 = 0x866;
const NUKA: u32 = 0x867;
const BREAD: u32 = 0x868;
const THIRST1: u32 = 0x872;
const WATER1: u32 = 0x880;
const CHEMIST: u32 = 0x890;
const RESISTANT: u32 = 0x891;
const LEAD_BELLY: u32 = 0x892;
const MAGAZINE: u32 = 0x869;
const BLOOD: u32 = 0x86A;
const STIM_GATED: u32 = 0x86B;
const REDUCE_END: u32 = 0x859;
const CHEM_HELD: u32 = 0x86C;
const POISON_FX: u32 = 0x85A;
const POISON: u32 = 0x86D;
const WITHDRAWAL_HELD: u32 = 0x873;
const NAMED_FX: u32 = 0x85B;
const SCRIPT_FX: u32 = 0x85C;
const LIMB_COND_FX: u32 = 0x85D;
const PSYCHO: u32 = 0x86E;
const REGEN: u32 = 0x85F;
const DAMAGE_HEALTH: u32 = 0x85E;
const RADS200: u32 = 0x879;
const RADS400: u32 = 0x875;
const REGEN_CAST: u32 = 0x876;
const HEAL_ABILITY: u32 = 0x877;
const HURT_ABILITY: u32 = 0x878;
const RAD1: u32 = 0x881;
const RAD2: u32 = 0x882;
const RAD_CHILD: u32 = 0x893;
const DRBAG: u32 = 0x86F;
const HYDRA: u32 = 0x874;

const P: FormId = FormId::PLAYER_REF;
const STRENGTH: u16 = 5;
const ENDURANCE: u16 = 7;
const LUCK: u16 = 11;
const HEALTH: u16 = 16;
const PERCEPTION_CONDITION: u16 = 25;
const MEDICINE: u16 = 37;
const SURVIVAL: u16 = 44;
const RAD_LEVEL: u16 = 54;
const CHAMELEON: u16 = 49;
const DEHYDRATION: u16 = 73;
const HUNGER: u16 = 74;
const SLEEP_DEPRIVATION: u16 = 75;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

/// MGEF DATA (72 bytes): flags, archetype and actor value; everything else empty.
fn mgef(fid: u32, edid: &str, flags: u32, archetype: u32, av: i32) -> Vec<u8> {
    let data = [u32s(&[flags]), f32b(0.0), u32s(&[0]), vec![0xFF; 8], vec![0; 4], vec![0; 4], f32b(1.0), vec![0; 24], vec![0; 8], u32s(&[archetype]), av.to_le_bytes().to_vec()].concat();
    assert_eq!(data.len(), 72);
    record(b"MGEF", 0, fid, &[sub(b"EDID", z(edid)), sub(b"DATA", data)])
}

/// EFID + EFIT (magnitude, area, duration, type Self, actor value).
fn effect(base: u32, magnitude: u32, duration: u32, av: i32) -> Vec<fnv_esm::raw::Subrecord> {
    vec![sub(b"EFID", base.to_le_bytes().to_vec()), sub(b"EFIT", [u32s(&[magnitude, 0, duration, 0]), av.to_le_bytes().to_vec()].concat())]
}

/// A condition `HasPerk perk == value` on the subject (function 449).
fn has_perk(perk: u32, value: f32) -> fnv_esm::raw::Subrecord {
    let mut d = vec![0x00, 0, 0, 0];
    d.extend(value.to_le_bytes());
    d.extend(449u16.to_le_bytes());
    d.extend([0, 0]);
    d.extend(perk.to_le_bytes());
    d.extend([0; 12]);
    sub(b"CTDA", d)
}

/// A condition `IsHardcore == value` (function 586).
fn is_hardcore(value: f32) -> fnv_esm::raw::Subrecord {
    let mut d = vec![0x00, 0, 0, 0];
    d.extend(value.to_le_bytes());
    d.extend(586u16.to_le_bytes());
    d.extend([0; 18]);
    sub(b"CTDA", d)
}

/// A condition `GetActorValue Luck >= 5` on the subject (function 14, comparison byte 0x60).
fn lucky() -> fnv_esm::raw::Subrecord {
    let mut d = vec![0x60, 0, 0, 0];
    d.extend(5f32.to_le_bytes());
    d.extend(14u16.to_le_bytes());
    d.extend([0, 0]);
    d.extend(u32::from(LUCK).to_le_bytes());
    d.extend([0; 12]);
    sub(b"CTDA", d)
}

/// An effect with conditions after its EFIT.
fn effect_if(base: u32, magnitude: u32, duration: u32, av: i32, conds: Vec<fnv_esm::raw::Subrecord>) -> Vec<fnv_esm::raw::Subrecord> {
    let mut e = effect(base, magnitude, duration, av);
    e.extend(conds);
    e
}

/// ALCH with ETYP, DATA, ENIT (value, flags, withdrawal, addiction chance, sound) and effects.
fn alch(fid: u32, edid: &str, flags: u8, withdrawal: u32, chance: f32, effects: Vec<Vec<fnv_esm::raw::Subrecord>>) -> Vec<u8> {
    let enit = [u32s(&[1]), vec![flags, 0, 0, 0], u32s(&[withdrawal]), f32b(chance), u32s(&[0])].concat();
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"ETYP", 10i32.to_le_bytes().to_vec()), sub(b"DATA", f32b(0.0)), sub(b"ENIT", enit)];
    subs.extend(effects.into_iter().flatten());
    record(b"ALCH", 0, fid, &subs)
}

/// SPEL with SPIT (type, cost, level, flags) and effects.
fn spel(fid: u32, edid: &str, kind: u32, effects: Vec<Vec<fnv_esm::raw::Subrecord>>) -> Vec<u8> {
    let mut subs = vec![sub(b"EDID", z(edid)), sub(b"SPIT", [u32s(&[kind, 0, 0]), vec![0; 4]].concat())];
    subs.extend(effects.into_iter().flatten());
    record(b"SPEL", 0, fid, &subs)
}

/// A float game setting.
fn gmst(fid: u32, name: &str, v: f32) -> Vec<u8> {
    record(b"GMST", 0, fid, &[sub(b"EDID", z(name)), sub(b"DATA", f32b(v))])
}

/// A perk with one entry point effect: entry point, function (3 Multiply), number.
fn perk(fid: u32, edid: &str, point: u8, function: u8, value: f32) -> Vec<u8> {
    record(
        b"PERK",
        0,
        fid,
        &[
            sub(b"EDID", z(edid)),
            sub(b"DATA", vec![0, 1, 1, 1, 0]),
            sub(b"PRKE", vec![2, 0, 0]),
            sub(b"DATA", vec![point, function, 1]),
            sub(b"EPFT", vec![1]),
            sub(b"EPFD", f32b(value)),
            sub(b"PRKF", Vec::new()),
        ],
    )
}

fn plugin_bytes() -> Vec<u8> {
    // Flags: 1 Hostile, 2 Recover, 4 Detrimental. Archetypes: 0 Value Modifier, 31 Cure
    // Addiction, 34 Value And Parts.
    let mgefs = [
        mgef(MOD_STR, "IncStrength", 2, 0, 5),
        mgef(RESTORE_HEALTH, "RestoreHealth", 0, 0, 16),
        mgef(PARTS, "RestoreHealthParts", 0, 34, 16),
        mgef(RESTORE_RADS, "RestoreRads", 0, 0, 54),
        mgef(REDUCE_STR, "ReduceStrength", 2 | 4, 0, 5),
        // Hostile and Detrimental: a poison's effect [documented: GECK wiki Ingestible].
        mgef(POISON_FX, "PoisonDamage", 1 | 4, 0, 16),
        mgef(CURE, "CureAddiction", 0, 31, -1),
        mgef(CHAMELEON_FX, "StealthField", 2, 12, 49),
        mgef(DAMAGE_RADS, "DamageRads", 4, 0, 54),
        mgef(RESTORE_HUNGER, "RestoreHunger", 0, 0, 74),
        mgef(REDUCE_END, "ReduceEndurance", 2 | 4, 0, 7),
        // Script effects (archetype 1): one flagged Display Name Only (bit 13), as
        // Psycho's "Damage +25%", and one not, as a Nuka-Cola's cap.
        mgef(NAMED_FX, "PsychoMagicEffect", 1 << 13, 1, -1),
        mgef(SCRIPT_FX, "PlainScriptEffect", 0, 1, -1),
        // Self and No Duration, as the masters' Regeneration.
        mgef(REGEN, "Regen", 0x10 | 0x80, 0, 16),
        mgef(DAMAGE_HEALTH, "DamageHealth", 1 | 4, 0, 16),
        // Limb Condition (archetype 35): NVRestoreLimbs for Doctor's Bag (instant) and Hydra (over time).
        mgef(LIMB_COND_FX, "RestoreLimbs", 0, 35, -1),
    ]
    .concat();
    let alchs = [
        alch(STIM, "Stim", 4, 0, 0.0, vec![effect(PARTS, 30, 0, 16)]),
        alch(STIM_HC, "StimHC", 4, 0, 0.0, vec![effect(RESTORE_HEALTH, 5, 6, 16)]),
        alch(BUFF, "Buff", 0, WITHDRAWAL, 0.1, vec![effect(MOD_STR, 2, 4, 5), effect(UMON, 30, 100, -1)]),
        alch(RADAWAY, "Rad", 4, 0, 0.0, vec![effect(RESTORE_RADS, 50, 0, 54)]),
        alch(FIXER, "Fix", 0, 0, 0.0, vec![effect(CURE, 0, 1, -1)]),
        alch(FOOD, "Meal", 2, 0, 0.0, vec![effect(RESTORE_HEALTH, 2, 5, 16)]),
        alch(STEALTH, "StealthBoy", 0, 0, 0.0, vec![effect(CHAMELEON_FX, 75, 10, 49)]),
        alch(NUKA, "Nuka", 0, 0, 0.0, vec![effect(DAMAGE_RADS, 10, 0, 54)]),
        alch(BREAD, "Bread", 0, 0, 0.0, vec![effect(RESTORE_HUNGER, 30, 0, 74)]),
        // A skill magazine: +1 Strength, +2 with the perk (as Programmer's Digest and
        // Comprehension); a Blood Pack: 1 Health at once, 4 a second for 5 s more with the
        // perk (as Hematophage).
        alch(MAGAZINE, "Magazine", 0, 0, 0.0, vec![effect_if(MOD_STR, 1, 60, 5, vec![has_perk(RESISTANT, 0.0)]), effect_if(MOD_STR, 2, 60, 5, vec![has_perk(RESISTANT, 1.0)])]),
        // A Stimpak as the masters' (aid-items.md §14): 30 at once outside Hardcore, 5 a
        // second for 6 s in it, each behind IsHardcore.
        alch(STIM_GATED, "StimGated", 4, 0, 0.0, vec![effect_if(PARTS, 30, 0, 16, vec![is_hardcore(0.0)]), effect_if(RESTORE_HEALTH, 5, 6, 16, vec![is_hardcore(1.0)])]),
        alch(BLOOD, "Blood", 0, 0, 0.0, vec![effect_if(RESTORE_HEALTH, 4, 5, 16, vec![has_perk(LEAD_BELLY, 1.0)]), effect(RESTORE_HEALTH, 1, 0, 16)]),
        alch(CHEM_HELD, "ChemHeld", 0, WITHDRAWAL_HELD, 0.0, vec![effect(MOD_STR, 2, 4, 5)]),
        alch(POISON, "TestVenom", 0, 0, 0.0, vec![effect(POISON_FX, 5, 0, 16)]),
        alch(PSYCHO, "Psycho", 0, 0, 0.0, vec![effect(NAMED_FX, 0, 240, -1), effect(SCRIPT_FX, 0, 240, -1), effect(MOD_STR, 1, 240, 5)]),
        alch(DRBAG, "DrBag", 4, 0, 0.0, vec![effect(LIMB_COND_FX, 36, 0, -1)]),
        alch(HYDRA, "Hydra", 0, 0, 0.0, vec![effect(LIMB_COND_FX, 10, 6, -1)]),
    ]
    .concat();
    let spels = [
        spel(WITHDRAWAL, "BuffWithdrawal", 10, vec![effect(REDUCE_STR, 1, 0, 5)]),
        spel(ABILITY, "StrongAbility", 4, vec![effect(MOD_STR, 1, 0, 5)]),
        spel(THIRST1, "Dehydration200", 1, vec![effect(REDUCE_STR, 1, 0, 5)]),
        // Stages of radiation sickness that regenerate Health with the perk (Disease).
        spel(RADS200, "Radiation200", 1, vec![effect(REDUCE_STR, 1, 0, 5), effect_if(REGEN, 2, 0, 16, vec![has_perk(RAD_CHILD, 1.0)])]),
        spel(RADS400, "Radiation400", 1, vec![effect(REDUCE_STR, 2, 0, 5), effect_if(REGEN, 4, 0, 16, vec![has_perk(RAD_CHILD, 1.0)])]),
        // The same base effect cast once, and abilities that are no regeneration.
        spel(REGEN_CAST, "RegenCast", 0, vec![effect(REGEN, 5, 0, 16)]),
        spel(HEAL_ABILITY, "HealAbility", 4, vec![effect(RESTORE_HEALTH, 10, 0, 16)]),
        spel(HURT_ABILITY, "HurtAbility", 4, vec![effect(DAMAGE_HEALTH, 10, 0, 16)]),
        spel(WITHDRAWAL_HELD, "HeldWithdrawal", 10, vec![effect_if(REDUCE_STR, 1, 0, 5, vec![lucky()]), effect_if(REDUCE_END, 1, 0, 7, vec![lucky()])]),
    ]
    .concat();
    let stages = record(b"DEHY", 0, WATER1, &[sub(b"EDID", z("Water1")), sub(b"DATA", u32s(&[200, THIRST1]))]);
    let rads =
        [(RAD1, "Rad1", 200, RADS200), (RAD2, "Rad2", 400, RADS400)].map(|(id, edid, at, spell)| record(b"RADS", 0, id, &[sub(b"EDID", z(edid)), sub(b"DATA", u32s(&[at, spell]))])).concat();
    let settings = [
        gmst(0x8A0, "fHCStarvationRate", 25.0),
        gmst(0x8A1, "fHCSleepDeprivationRate", 50.0),
        gmst(0x8A2, "fAVDCarryWeightsBase", 150.0),
        gmst(0x8A3, "fAVDCarryWeightMult", 10.0),
    ]
    .concat();
    let perks = [perk(CHEMIST, "Chemist", 25, 3, 2.0), perk(RESISTANT, "ChemResistant", 23, 3, 0.5), perk(LEAD_BELLY, "LeadBelly", 44, 3, 0.5), perk(RAD_CHILD, "RadChild", 24, 3, 1.0)].concat();
    let player = record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player")), sub(b"DATA", [100i32.to_le_bytes().to_vec(), vec![5; 7]].concat())]);
    let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
    [
        tes4(),
        top(b"GMST", &settings),
        top(b"NPC_", &player),
        top(b"PERK", &perks),
        top(b"DEHY", &stages),
        top(b"RADS", &rads),
        top(b"MGEF", &mgefs),
        top(b"SPEL", &spels),
        top(b"ALCH", &alchs),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell))),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Aid.esm".into()).unwrap()];
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

/// The player with Health 100, Strength 5, Medicine 15 and Survival 55.
fn game(f: &Fixture) -> Game<'_> {
    let world = World::new(&f.forms, &f.refs, SimConfig::default());
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() });
    g.player_entered(Some(FormId(CELL)));
    for (code, v) in [(HEALTH, 100.0), (STRENGTH, 5.0), (MEDICINE, 15.0), (SURVIVAL, 55.0), (RAD_LEVEL, 0.0)] {
        set_actor_value_current(&mut g, P, code, v).unwrap();
    }
    g
}

fn av(g: &mut Game, code: u16) -> f64 {
    actor_value(g, P, code).unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn call(g: &mut Game, op: u16, args: &[ArgValue]) -> f64 {
    let ctx = Ctx { unit: "test".into(), opcode: op, on: Some(P), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
    let before = g.events.len();
    let v = g.call_function(&ctx, args);
    for e in &g.events[before..] {
        if let EventKind::CallFailed { name, error, .. } | EventKind::Unhandled { name, unit: error, .. } = &e.kind {
            panic!("0x{op:04X} {name}: {error}");
        }
    }
    match v {
        Value::Num(x) => x,
        Value::Ref(f) => f as f64,
    }
}

fn form(x: u32) -> ArgValue {
    ArgValue::Value(Value::Ref(x))
}

#[test]
fn records_are_read() {
    let f = fixture();
    let d = &f.data;
    let i = &d.ingestibles[&FormId(BUFF)];
    assert_eq!((i.withdrawal, i.equip_type, i.flags), (Some(FormId(WITHDRAWAL)), 10, 0));
    assert!((i.addiction_chance - 0.1).abs() < 1e-6);
    assert_eq!(d.ingestibles[&FormId(STIM)].flags, 4);
    assert_eq!(d.spell_types[&FormId(WITHDRAWAL)], 10);
    assert_eq!(d.magic_effects[&FormId(MOD_STR)].actor_value, Some(STRENGTH));
    assert_eq!(d.effect_items[&FormId(BUFF)][0].actor_value, Some(STRENGTH));
    assert_eq!(d.effect_items[&FormId(BUFF)][1].actor_value, None);
}

/// A Value And Parts effect of duration 0 restores health and the limbs at once, with
/// the Medicine potency 1 + 2 × 15 / 100 = 1.3 (aid-items.md §3, §4).
#[test]
fn a_stimpak_restores_at_once_with_medicine_potency() {
    let f = fixture();
    let mut g = game(&f);
    add_actor_value_damage(&mut g, P, HEALTH, 60.0);
    add_actor_value_damage(&mut g, P, PERCEPTION_CONDITION, 20.0);
    let r = g.ingest(P, FormId(STIM));
    assert!(near(r.potency, 1.3));
    assert!(near(av(&mut g, HEALTH), 79.0));
    // On the body each limb gets a share of the 39 (fnv_game::aid::PARTS_LIMB_SHARE).
    let share = 39.0 * fnv_game::aid::PARTS_LIMB_SHARE;
    assert!(near(av(&mut g, PERCEPTION_CONDITION), 80.0 + share));
    // Never past the permanent value.
    g.ingest(P, FormId(STIM));
    assert!(near(av(&mut g, HEALTH), 100.0));
}

/// A Stimpak on one limb (the Pip-Boy's CND page) gives that limb the whole amount and
/// the other limbs nothing.
#[test]
fn a_stimpak_on_a_limb_gives_it_the_whole_amount() {
    let f = fixture();
    let mut g = game(&f);
    add_actor_value_damage(&mut g, P, HEALTH, 60.0);
    add_actor_value_damage(&mut g, P, PERCEPTION_CONDITION, 50.0);
    add_actor_value_damage(&mut g, P, PERCEPTION_CONDITION + 1, 50.0);
    g.ingest_on(P, FormId(STIM), Some(PERCEPTION_CONDITION));
    assert!(near(av(&mut g, HEALTH), 79.0));
    assert!(near(av(&mut g, PERCEPTION_CONDITION), 89.0));
    assert!(near(av(&mut g, PERCEPTION_CONDITION + 1), 50.0));
}

/// A Doctor's Bag aimed at one limb in Hardcore heals only that limb to full; un-aimed it
/// restores every limb by `36 * potency`, and a duration Limb Condition item (Hydra) still
/// restores all limbs over time (#3431).
#[test]
fn a_doctors_bag_aimed_at_a_limb_heals_only_that_limb() {
    let f = fixture();
    let mut g = game(&f);
    let head = PERCEPTION_CONDITION;
    let left_arm = PERCEPTION_CONDITION + 2;
    let left_leg = PERCEPTION_CONDITION + 4;
    // Aimed at left arm: only left arm heals to 100; head and left leg stay crippled.
    for limb in [head, left_arm, left_leg] {
        add_actor_value_damage(&mut g, P, limb, 100.0);
    }
    g.ingest_on(P, FormId(DRBAG), Some(left_arm));
    assert!(near(av(&mut g, head), 0.0), "head must stay untouched when left arm is targeted");
    assert!(near(av(&mut g, left_arm), 100.0), "targeted left arm heals to full");
    assert!(near(av(&mut g, left_leg), 0.0), "left leg must stay untouched");
    // Aimed at head: only head heals to 100; left leg stays at 0.
    g.ingest_on(P, FormId(DRBAG), Some(head));
    assert!(near(av(&mut g, head), 100.0));
    assert!(near(av(&mut g, left_leg), 0.0));
    // Un-aimed (body): restores every damaged limb by floor(36 * 1.3) = 46 at Medicine 15.
    add_actor_value_damage(&mut g, P, head, 100.0);
    g.ingest_on(P, FormId(DRBAG), None);
    assert!(near(av(&mut g, head), 46.0));
    assert!(near(av(&mut g, left_leg), 46.0));
    // Duration Limb Condition (Hydra): restores all limbs over time even if a limb was passed.
    g.ingest_on(P, FormId(HYDRA), Some(head));
    g.frames(120);
    assert!(av(&mut g, head) > 65.0 && av(&mut g, left_leg) > 65.0);
}

/// Without Recover and with a duration the magnitude is restored every second while
/// the effect lasts, and no more after it.
#[test]
fn restoration_over_time_follows_the_seconds() {
    let f = fixture();
    let mut g = game(&f);
    add_actor_value_damage(&mut g, P, HEALTH, 60.0);
    g.ingest(P, FormId(STIM_HC));
    assert!(near(av(&mut g, HEALTH), 40.0));
    // 5 a second at Medicine 15's potency 1.3 is 6.5, rounded down to 6 (#1167).
    g.frames(180);
    assert!((av(&mut g, HEALTH) - 58.0).abs() < 1e-3, "{}", av(&mut g, HEALTH));
    g.frames(240);
    assert!((av(&mut g, HEALTH) - 76.0).abs() < 1e-3);
    assert!(g.effects.is_empty());
}

/// Food's potency follows Survival (55 → 2.1, the GECK wiki's example); the scaled
/// magnitude is rounded down.
#[test]
fn food_follows_survival() {
    let f = fixture();
    let mut g = game(&f);
    add_actor_value_damage(&mut g, P, HEALTH, 50.0);
    let r = g.ingest(P, FormId(FOOD));
    assert!(near(r.potency, 2.1));
    // 2 a second × 2.1 = 4.2, rounded down to 4, for 5 s.
    g.frames(300);
    assert!((av(&mut g, HEALTH) - 70.0).abs() < 1e-3);
}

/// Recover: a temporary modifier while the effect lasts; the base and the permanent
/// value do not change.
#[test]
fn a_chem_raises_a_value_while_it_lasts() {
    let f = fixture();
    let mut g = game(&f);
    g.ingest(P, FormId(BUFF));
    assert_eq!(av(&mut g, STRENGTH), 7.0);
    assert_eq!(call(&mut g, 0x1115, &[ArgValue::Enum(STRENGTH)]), 5.0);
    assert_eq!(call(&mut g, 0x11EF, &[ArgValue::Enum(STRENGTH)]), 5.0);
    g.frames(4 * 60 + 1);
    assert_eq!(av(&mut g, STRENGTH), 5.0);
    // The usage monitor lasts on.
    assert_eq!(call(&mut g, 0x122B, &[form(BUFF)]), 30.0);
}

/// RadAway lowers rads (Medicine potency), never below 0.
#[test]
fn radaway_lowers_rads() {
    let f = fixture();
    let mut g = game(&f);
    set_actor_value_current(&mut g, P, RAD_LEVEL, 100.0).unwrap();
    g.ingest(P, FormId(RADAWAY));
    assert!(near(av(&mut g, RAD_LEVEL), 35.0));
    g.ingest(P, FormId(RADAWAY));
    assert_eq!(av(&mut g, RAD_LEVEL), 0.0);
}

/// The usage monitor adds 30 a use; from 70 the chance is (usage − 70) / 100, so the
/// sixth use (180) addicts for sure. The withdrawal waits while the chem lasts, then
/// takes a point of Strength; Fixer cures it (aid-items.md §5).
#[test]
fn addiction_follows_usage_and_is_cured() {
    let f = fixture();
    let mut g = game(&f);
    let first = g.ingest(P, FormId(BUFF));
    assert_eq!((first.usage, first.addiction), (Some(30.0), Some((0.0, false))));
    let second = g.ingest(P, FormId(BUFF));
    assert_eq!(second.usage, Some(60.0));
    let mut addicted = false;
    for _ in 0..4 {
        let r = g.ingest(P, FormId(BUFF));
        addicted |= r.addiction.is_some_and(|a| a.1);
    }
    assert!(addicted);
    assert_eq!(call(&mut g, 0x122B, &[form(BUFF)]), 180.0);
    assert_eq!(g.addictions(P), vec![FormId(WITHDRAWAL)]);
    // Sated: Strength is 5 + 2 while the (merged, 24 s) Buffout effect lasts.
    assert_eq!(av(&mut g, STRENGTH), 7.0);
    g.frames(24 * 60 + 2);
    assert_eq!(av(&mut g, STRENGTH), 4.0);
    // Another dose sates it again.
    g.ingest(P, FormId(BUFF));
    g.frames(1);
    assert_eq!(av(&mut g, STRENGTH), 7.0);
    g.frames(4 * 60 + 2);
    assert_eq!(av(&mut g, STRENGTH), 4.0);
    g.ingest(P, FormId(FIXER));
    assert!(g.addictions(P).is_empty());
    assert_eq!(av(&mut g, STRENGTH), 5.0);
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::AddictionCured { .. })));
}

/// Strength and Endurance, the two values the conditional withdrawal takes.
fn str_end(g: &mut Game) -> (f64, f64) {
    (av(g, STRENGTH), av(g, ENDURANCE))
}

/// The conditional withdrawal's conditions pass or fail, and the once-a-second check sees it.
fn set_lucky(g: &mut Game, pass: bool) {
    set_actor_value_current(g, P, LUCK, if pass { 5.0 } else { 1.0 }).unwrap();
    g.frames(90);
}

/// The chem's 4 s run out, and the frame after sees it.
fn chem_runs_out(g: &mut Game) {
    g.frames(4 * 60 + 2);
}

/// A withdrawal held by its conditions takes nothing when its chem ends; when they pass it
/// applies once, and the cure gives back that once (#1580).
#[test]
fn a_held_addiction_applies_once_after_its_chem() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(str_end(&mut g), (5.0, 5.0));
    call(&mut g, 0x101C, &[form(WITHDRAWAL_HELD)]);
    assert_eq!(str_end(&mut g), (4.0, 4.0));
    set_lucky(&mut g, false);
    assert_eq!(str_end(&mut g), (5.0, 5.0));
    g.ingest(P, FormId(CHEM_HELD));
    g.frames(1);
    assert_eq!(str_end(&mut g), (7.0, 5.0));
    chem_runs_out(&mut g);
    assert_eq!(str_end(&mut g), (5.0, 5.0));
    g.frames(10 * 60);
    assert_eq!(str_end(&mut g), (5.0, 5.0));
    set_lucky(&mut g, true);
    assert_eq!(str_end(&mut g), (4.0, 4.0));
    g.frames(10 * 60);
    assert_eq!(str_end(&mut g), (4.0, 4.0));
    g.ingest(P, FormId(FIXER));
    assert!(g.addictions(P).is_empty());
    assert_eq!(str_end(&mut g), (5.0, 5.0));
}

/// The same with the conditions failing already when the addiction is gained, so its effects
/// never started.
#[test]
fn an_addiction_gained_held_applies_once_after_its_chem() {
    let f = fixture();
    let mut g = game(&f);
    set_lucky(&mut g, false);
    call(&mut g, 0x101C, &[form(WITHDRAWAL_HELD)]);
    assert_eq!(str_end(&mut g), (5.0, 5.0));
    g.ingest(P, FormId(CHEM_HELD));
    g.frames(1);
    assert_eq!(str_end(&mut g), (7.0, 5.0));
    chem_runs_out(&mut g);
    assert_eq!(str_end(&mut g), (5.0, 5.0));
    set_lucky(&mut g, true);
    assert_eq!(str_end(&mut g), (4.0, 4.0));
    g.ingest(P, FormId(FIXER));
    assert_eq!(str_end(&mut g), (5.0, 5.0));
}

/// Conditions failing while the chem sates the addiction: nothing to give back, and nothing
/// taken when the chem ends; it applies when they pass, and is sated again by another dose.
#[test]
fn a_sated_addiction_held_by_its_conditions_stays_out_when_the_chem_ends() {
    let f = fixture();
    let mut g = game(&f);
    call(&mut g, 0x101C, &[form(WITHDRAWAL_HELD)]);
    g.ingest(P, FormId(CHEM_HELD));
    g.frames(1);
    assert_eq!(str_end(&mut g), (7.0, 5.0));
    set_lucky(&mut g, false);
    assert_eq!(str_end(&mut g), (7.0, 5.0));
    chem_runs_out(&mut g);
    assert_eq!(str_end(&mut g), (5.0, 5.0));
    set_lucky(&mut g, true);
    assert_eq!(str_end(&mut g), (4.0, 4.0));
    g.ingest(P, FormId(CHEM_HELD));
    g.frames(1);
    assert_eq!(str_end(&mut g), (7.0, 5.0));
    chem_runs_out(&mut g);
    assert_eq!(str_end(&mut g), (4.0, 4.0));
}

/// A cure gives back only what the addiction holds: nothing while its conditions hold it,
/// and nothing while the chem sates it, conditions passing or not.
#[test]
fn a_cure_of_a_held_or_sated_addiction_changes_nothing() {
    let f = fixture();
    // Held, the chem run out.
    let mut g = game(&f);
    call(&mut g, 0x101C, &[form(WITHDRAWAL_HELD)]);
    set_lucky(&mut g, false);
    g.ingest(P, FormId(CHEM_HELD));
    chem_runs_out(&mut g);
    assert_eq!(str_end(&mut g), (5.0, 5.0));
    g.ingest(P, FormId(FIXER));
    assert!(g.addictions(P).is_empty());
    assert_eq!(str_end(&mut g), (5.0, 5.0));
    set_lucky(&mut g, true);
    assert_eq!(str_end(&mut g), (5.0, 5.0));
    // Sated, then sated and held.
    for held in [false, true] {
        let mut g = game(&f);
        call(&mut g, 0x101C, &[form(WITHDRAWAL_HELD)]);
        g.ingest(P, FormId(CHEM_HELD));
        g.frames(1);
        if held {
            set_lucky(&mut g, false);
        }
        assert_eq!(str_end(&mut g), (7.0, 5.0));
        g.ingest(P, FormId(FIXER));
        assert!(g.addictions(P).is_empty());
        assert_eq!(str_end(&mut g), (7.0, 5.0));
        chem_runs_out(&mut g);
        assert_eq!(str_end(&mut g), (5.0, 5.0));
        set_lucky(&mut g, true);
        assert_eq!(str_end(&mut g), (5.0, 5.0));
    }
}

/// A player's ability without conditions changes the permanent pool; RemoveSpell takes
/// it back [documented: GECK wiki ActorValue].
#[test]
fn an_unconditional_ability_is_permanent_on_the_player() {
    let f = fixture();
    let mut g = game(&f);
    call(&mut g, 0x101C, &[form(ABILITY)]);
    assert_eq!(av(&mut g, STRENGTH), 6.0);
    assert_eq!(call(&mut g, 0x11EF, &[ArgValue::Enum(STRENGTH)]), 6.0);
    call(&mut g, 0x101D, &[form(ABILITY)]);
    assert_eq!(av(&mut g, STRENGTH), 5.0);
    assert_eq!(call(&mut g, 0x11EF, &[ArgValue::Enum(STRENGTH)]), 5.0);
}

/// A negative modifier does not take an attribute below 0 [documented: GECK wiki
/// ActorValue], and gives back only what it took.
#[test]
fn a_negative_modifier_stops_at_zero() {
    let f = fixture();
    let mut g = game(&f);
    set_actor_value_current(&mut g, P, STRENGTH, 0.0).unwrap();
    call(&mut g, 0x101C, &[form(WITHDRAWAL)]);
    assert_eq!(av(&mut g, STRENGTH), 0.0);
    call(&mut g, 0x101D, &[form(WITHDRAWAL)]);
    assert_eq!(av(&mut g, STRENGTH), 0.0);
}

/// Hours waited advance active effects by the seconds of play an hour stands for at the
/// TimeScale (120 s at 30): Buffout's 4 s modifier and its 100 s usage monitor end
/// (aid-items.md §6).
#[test]
fn waiting_advances_effect_timers() {
    let f = fixture();
    let mut g = game(&f);
    g.ingest(P, FormId(BUFF));
    assert_eq!(av(&mut g, STRENGTH), 7.0);
    let passed = g.pass_hours(1.0, false);
    assert!(near(passed.seconds, 120.0));
    assert_eq!(passed.effects_ended, 2);
    assert_eq!(av(&mut g, STRENGTH), 5.0);
    assert_eq!(call(&mut g, 0x122B, &[form(BUFF)]), 0.0);
    // A restoration by the second gives what is left of it, no more.
    add_actor_value_damage(&mut g, P, HEALTH, 60.0);
    g.ingest(P, FormId(STIM_HC));
    g.pass_hours(1.0, false);
    assert!((av(&mut g, HEALTH) - 76.0).abs() < 1e-6);
    // Outside Hardcore the needs do not move.
    assert_eq!(av(&mut g, HUNGER), 0.0);
}

/// In Hardcore the needs rise one point every `rate` seconds of play (hunger 25, sleep
/// 50, dehydration 10 from the owner's table) in play, and by whole points an hour while
/// waiting (12, 4, 2); sleep lowers sleep deprivation (aid-items.md §11).
#[test]
fn hardcore_needs_rise_with_time() {
    let f = fixture();
    let mut g = game(&f);
    g.frames(25 * 60);
    assert_eq!(av(&mut g, HUNGER), 0.0);
    g.player.hardcore = Some(true);
    g.frames(25 * 60);
    assert!((av(&mut g, HUNGER) - 1.0).abs() < 1e-6, "{}", av(&mut g, HUNGER));
    assert!((av(&mut g, DEHYDRATION) - 2.5).abs() < 1e-6);
    assert!((av(&mut g, SLEEP_DEPRIVATION) - 0.5).abs() < 1e-6);
    let passed = g.pass_hours(10.0, false);
    assert_eq!(passed.needs.len(), 3);
    assert!((av(&mut g, HUNGER) - 41.0).abs() < 1e-6);
    assert!((av(&mut g, SLEEP_DEPRIVATION) - 20.5).abs() < 1e-6);
    g.pass_hours(1.0, true);
    assert_eq!(av(&mut g, SLEEP_DEPRIVATION), 2.0);
    assert!((av(&mut g, HUNGER) - 45.0).abs() < 1e-6);
    // Food lowers hunger ("Restore" on a need), never below 0.
    g.ingest(P, FormId(BREAD));
    assert!((av(&mut g, HUNGER) - 15.0).abs() < 1e-6);
    g.ingest(P, FormId(BREAD));
    assert_eq!(av(&mut g, HUNGER), 0.0);
}

/// The owner's check in the original (2026-10-31, Hardcore, all needs at 0, #1177): 3
/// hours waited give H2O 36, FOD 12, SLP 6; 3 hours slept give 36, 12 and 2. In
/// Hardcore the sleep heals no Health.
#[test]
fn waits_and_sleeps_give_the_originals_totals() {
    let f = fixture();
    for (sleeping, slp) in [(false, 6.0), (true, 2.0)] {
        let mut g = game(&f);
        g.player.hardcore = Some(true);
        add_actor_value_damage(&mut g, P, HEALTH, 40.0);
        for _ in 0..3 {
            g.pass_hours(1.0, sleeping);
        }
        assert_eq!((av(&mut g, DEHYDRATION), av(&mut g, HUNGER), av(&mut g, SLEEP_DEPRIVATION)), (36.0, 12.0, slp), "sleeping {sleeping}");
        // Hours passed at once give the same.
        let mut h = game(&f);
        h.player.hardcore = Some(true);
        h.pass_hours(3.0, sleeping);
        assert_eq!((av(&mut h, DEHYDRATION), av(&mut h, HUNGER), av(&mut h, SLEEP_DEPRIVATION)), (36.0, 12.0, slp));
        assert_eq!(av(&mut g, HEALTH), 60.0);
    }
}

/// Outside Hardcore a sleep restores all Health (and limbs); a wait restores none.
#[test]
fn a_sleep_outside_hardcore_restores_health() {
    let f = fixture();
    let mut g = game(&f);
    add_actor_value_damage(&mut g, P, HEALTH, 70.0);
    assert_eq!(g.pass_hours(1.0, false).health_restored, 0.0);
    assert_eq!(av(&mut g, HEALTH), 30.0);
    assert_eq!(g.pass_hours(1.0, true).health_restored, 70.0);
    assert_eq!(av(&mut g, HEALTH), 100.0);
}

/// Hardcore Tweaks: sleep heals as its option says, in Hardcore or out of it.
#[test]
fn the_hardcore_tweaks_set_sleep_healing_apart_from_the_mode() {
    let f = fixture();
    let mut g = game(&f);
    g.tweaks.hardcore_tweaks = true;
    g.tweaks.hardcore_sleep_heals = false;
    add_actor_value_damage(&mut g, P, HEALTH, 70.0);
    assert_eq!(g.pass_hours(1.0, true).health_restored, 0.0);
    g.player.hardcore = Some(true);
    g.tweaks.hardcore_sleep_heals = true;
    assert_eq!(g.pass_hours(1.0, true).health_restored, 70.0);
}

/// SetHardcore turns on the simulation's switch too (ammunition weighs), and turning it
/// off marks the game as not always Hardcore.
#[test]
fn set_hardcore_reaches_the_ammunition_weight() {
    let f = fixture();
    let mut g = game(&f);
    assert!(!g.world.config().hardcore);
    g.set_hardcore(true);
    assert!(g.hardcore() && g.world.config().hardcore && !g.player.hardcore_was_off);
    g.set_hardcore(false);
    assert!(!g.hardcore() && !g.world.config().hardcore && g.player.hardcore_was_off);
}

/// A need past a stage's threshold gives the stage's effect; below it, or with
/// Hardcore off, the effect goes. Restore- and DamageActorValue move a need.
#[test]
fn need_stages_give_their_effects() {
    let f = fixture();
    let mut g = game(&f);
    g.player.hardcore = Some(true);
    set_actor_value_current(&mut g, P, DEHYDRATION, 199.0).unwrap();
    g.frames(1);
    assert_eq!(av(&mut g, STRENGTH), 5.0);
    g.pass_hours(1.0, false);
    assert!(av(&mut g, DEHYDRATION) >= 200.0);
    assert_eq!(av(&mut g, STRENGTH), 4.0);
    assert!(g.events.iter().any(|e| matches!(e.kind, EventKind::NeedStage { stage: Some(FormId(WATER1)), .. })));
    call(&mut g, 0x1182, &[ArgValue::Enum(DEHYDRATION), ArgValue::Value(Value::Num(50.0))]);
    g.frames(1);
    assert!(av(&mut g, DEHYDRATION) < 200.0);
    assert_eq!(av(&mut g, STRENGTH), 5.0);
    // Back over it, then Hardcore off.
    call(&mut g, 0x1181, &[ArgValue::Enum(DEHYDRATION), ArgValue::Value(Value::Num(50.0))]);
    g.frames(1);
    assert_eq!(av(&mut g, STRENGTH), 4.0);
    g.player.hardcore = Some(false);
    g.frames(1);
    assert_eq!(av(&mut g, STRENGTH), 5.0);
}

/// Turning Hardcore off (SetHardcore 0, the settings page's switch) sets every need back
/// to 0 and takes their stages' effects away at once; on again, they start from 0.
#[test]
fn hardcore_off_clears_the_needs_and_their_effects() {
    let f = fixture();
    let mut g = game(&f);
    g.set_hardcore(true);
    set_actor_value_current(&mut g, P, DEHYDRATION, 250.0).unwrap();
    set_actor_value_current(&mut g, P, HUNGER, 120.0).unwrap();
    set_actor_value_current(&mut g, P, SLEEP_DEPRIVATION, 80.0).unwrap();
    g.frames(1);
    assert_eq!(av(&mut g, STRENGTH), 4.0);
    assert!(!g.player.need_stages.is_empty());
    g.set_hardcore(false);
    for code in [DEHYDRATION, HUNGER, SLEEP_DEPRIVATION] {
        assert_eq!(av(&mut g, code), 0.0, "need {code}");
    }
    assert_eq!(av(&mut g, STRENGTH), 5.0);
    assert!(g.player.need_stages.is_empty());
    g.set_hardcore(true);
    g.frames(1);
    assert_eq!(av(&mut g, STRENGTH), 5.0);
    assert!(av(&mut g, DEHYDRATION) < 1.0);
}

/// The Stealth Boy's Chameleon effect raises the Chameleon actor value while it lasts,
/// which detection reads (aid-items.md §2, detection.md).
#[test]
fn a_stealth_boy_gives_chameleon() {
    let f = fixture();
    let mut g = game(&f);
    let r = g.ingest(P, FormId(STEALTH));
    assert_eq!(r.applied.len(), 1);
    assert_eq!(av(&mut g, CHAMELEON), 75.0);
    g.frames(10 * 60 + 1);
    assert_eq!(av(&mut g, CHAMELEON), 0.0);
}

/// Perk entry points: Chemist doubles a chem's beneficial durations (not the usage
/// monitor's), Chem Resistant halves the addiction chance, Lead Belly halves the rads
/// an item gives (aid-items.md §10).
#[test]
fn perks_change_chems_addiction_and_rads() {
    let f = fixture();
    let mut g = game(&f);
    g.player.perks.insert(FormId(CHEMIST), 1);
    g.player.perks.insert(FormId(RESISTANT), 1);
    g.player.perks.insert(FormId(LEAD_BELLY), 1);
    let r = g.ingest(P, FormId(BUFF));
    assert_eq!(r.applied[0].duration, 8.0);
    assert_eq!(r.applied[1].duration, 100.0);
    assert!(r.perks.contains(&(FormId(CHEMIST), 25)));
    g.frames(5 * 60);
    assert_eq!(av(&mut g, STRENGTH), 7.0);
    g.frames(3 * 60 + 1);
    assert_eq!(av(&mut g, STRENGTH), 5.0);
    g.ingest(P, FormId(BUFF));
    let third = g.ingest(P, FormId(BUFF));
    // Usage 90: (90 - 70) / 100 = 0.2, halved.
    assert!(near(third.addiction.unwrap().0, 0.1));
    g.ingest(P, FormId(NUKA));
    assert!(near(av(&mut g, RAD_LEVEL), 5.0));
    // Without the perks.
    g.player.perks.clear();
    g.ingest(P, FormId(NUKA));
    assert!(near(av(&mut g, RAD_LEVEL), 15.0));
}

/// In god mode (tgm) a damaging effect leaves the player alone: the original's Beer and
/// Nuka-Cola, drunk with tgm on, raised no Dehydration (o7_00, #1167). Without it the
/// soda's damage holds.
#[test]
fn god_mode_keeps_damaging_effects_off_the_player() {
    let f = fixture();
    let mut g = game(&f);
    g.toggles.god_mode = true;
    g.ingest(P, FormId(NUKA));
    assert_eq!(av(&mut g, RAD_LEVEL), 0.0);
    g.toggles.god_mode = false;
    g.ingest(P, FormId(NUKA));
    assert!(near(av(&mut g, RAD_LEVEL), 10.0));
}

/// In god mode a script does not hurt the player (#2792, the maintainer's request on
/// #2743: "no trap may damage the player in god mode"): the bear trap's DamageActorValue
/// on Health and a leg, and a ModActorValue or ForceActorValue that would lower Health
/// or a limb, change nothing; restoring still heals what was taken before. Without god
/// mode they all apply.
#[test]
fn god_mode_keeps_script_damage_off_the_players_health_and_limbs() {
    const LEFT_MOBILITY_CONDITION: u16 = 29;
    const DAMAGE: u16 = 0x1181;
    const RESTORE: u16 = 0x1182;
    const MOD: u16 = 0x1010;
    const FORCE: u16 = 0x110E;
    let arg = |code: u16, n: f64| [ArgValue::Enum(code), ArgValue::Value(Value::Num(n))];
    let f = fixture();
    let mut g = game(&f);
    let health = av(&mut g, HEALTH);
    let leg = av(&mut g, LEFT_MOBILITY_CONDITION);
    let head = av(&mut g, PERCEPTION_CONDITION);
    assert!(health > 50.0 && leg == 100.0 && head == 100.0);
    // Something taken before god mode came on.
    call(&mut g, DAMAGE, &arg(HEALTH, 10.0));
    g.toggles.god_mode = true;
    call(&mut g, DAMAGE, &arg(HEALTH, 45.0));
    call(&mut g, DAMAGE, &arg(LEFT_MOBILITY_CONDITION, 50.0));
    call(&mut g, MOD, &arg(HEALTH, -20.0));
    call(&mut g, FORCE, &arg(PERCEPTION_CONDITION, 10.0));
    assert!(near(av(&mut g, HEALTH), health - 10.0));
    assert_eq!(av(&mut g, LEFT_MOBILITY_CONDITION), leg);
    assert_eq!(av(&mut g, PERCEPTION_CONDITION), head);
    // Healing and raising go on in god mode.
    call(&mut g, RESTORE, &arg(HEALTH, 10.0));
    assert!(near(av(&mut g, HEALTH), health));
    call(&mut g, MOD, &arg(HEALTH, 5.0));
    assert!(near(av(&mut g, HEALTH), health + 5.0));
    call(&mut g, MOD, &arg(HEALTH, -5.0));
    assert!(near(av(&mut g, HEALTH), health + 5.0));
    g.toggles.god_mode = false;
    call(&mut g, MOD, &arg(HEALTH, -5.0));
    call(&mut g, DAMAGE, &arg(HEALTH, 45.0));
    call(&mut g, DAMAGE, &arg(LEFT_MOBILITY_CONDITION, 50.0));
    call(&mut g, MOD, &arg(HEALTH, -20.0));
    call(&mut g, FORCE, &arg(PERCEPTION_CONDITION, 10.0));
    assert!(near(av(&mut g, HEALTH), health - 65.0));
    assert!(near(av(&mut g, LEFT_MOBILITY_CONDITION), leg - 50.0));
    assert!(near(av(&mut g, PERCEPTION_CONDITION), 10.0));
}

/// In god mode the player's Action Points are never spent, by the engine (a VATS attack,
/// a tweak's cost: `add_actor_value_damage`) or by a script's DamageActorValue (owner
/// decision 2026-11-15, #2743). Without it both take them.
#[test]
fn god_mode_keeps_the_players_action_points() {
    const ACTION_POINTS: u16 = 12;
    let f = fixture();
    let mut g = game(&f);
    let full = av(&mut g, ACTION_POINTS);
    g.toggles.god_mode = true;
    add_actor_value_damage(&mut g, P, ACTION_POINTS, 26.0);
    call(&mut g, 0x1181, &[ArgValue::Enum(ACTION_POINTS), ArgValue::Value(Value::Num(10.0))]);
    assert_eq!(av(&mut g, ACTION_POINTS), full);
    g.toggles.god_mode = false;
    add_actor_value_damage(&mut g, P, ACTION_POINTS, 26.0);
    call(&mut g, 0x1181, &[ArgValue::Enum(ACTION_POINTS), ArgValue::Value(Value::Num(10.0))]);
    assert!(near(av(&mut g, ACTION_POINTS), full - 36.0));
    // What was spent before god mode came on can still come back in it.
    g.toggles.god_mode = true;
    call(&mut g, 0x1182, &[ArgValue::Enum(ACTION_POINTS), ArgValue::Value(Value::Num(36.0))]);
    assert_eq!(av(&mut g, ACTION_POINTS), full);
}

/// In god mode the player's rads do not rise from a script's DamageActorValue or
/// ModActorValue (#2743); they still fall. Without it both raise them.
#[test]
fn god_mode_keeps_rads_off_the_player() {
    let f = fixture();
    let mut g = game(&f);
    set_actor_value_current(&mut g, P, RAD_LEVEL, 100.0).unwrap();
    g.toggles.god_mode = true;
    call(&mut g, 0x1181, &[ArgValue::Enum(RAD_LEVEL), ArgValue::Value(Value::Num(50.0))]);
    call(&mut g, 0x1010, &[ArgValue::Enum(RAD_LEVEL), ArgValue::Value(Value::Num(50.0))]);
    assert_eq!(av(&mut g, RAD_LEVEL), 100.0);
    call(&mut g, 0x1182, &[ArgValue::Enum(RAD_LEVEL), ArgValue::Value(Value::Num(40.0))]);
    assert!(near(av(&mut g, RAD_LEVEL), 60.0));
    g.toggles.god_mode = false;
    call(&mut g, 0x1181, &[ArgValue::Enum(RAD_LEVEL), ArgValue::Value(Value::Num(50.0))]);
    call(&mut g, 0x1010, &[ArgValue::Enum(RAD_LEVEL), ArgValue::Value(Value::Num(50.0))]);
    assert!(near(av(&mut g, RAD_LEVEL), 160.0));
}

/// Buffout's +2 Strength raises the carry capacity the menus show by 20 while it lasts
/// (aid-items.md §12).
#[test]
fn buffout_raises_carry_weight() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(g.world.encumbrance(P).unwrap().capacity, 200.0);
    g.ingest(P, FormId(BUFF));
    assert_eq!(g.world.encumbrance(P).unwrap().capacity, 220.0);
    assert_eq!(av(&mut g, 13), 220.0);
    g.frames(4 * 60 + 1);
    assert_eq!(g.world.encumbrance(P).unwrap().capacity, 200.0);
}

/// The EFFECTS preview: only the effects whose conditions pass, one entry per value, an
/// effect over time counted whole, the item's potency (owner's playtest list 7 item 5).
#[test]
fn the_preview_keeps_the_effects_that_apply() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(g.aid_preview(P, FormId(MAGAZINE)), vec![(STRENGTH, 1.0)]);
    assert_eq!(g.aid_preview(P, FormId(BLOOD)), vec![(HEALTH, 1.0)]);
    g.player.perks.insert(FormId(RESISTANT), 1);
    g.player.perks.insert(FormId(LEAD_BELLY), 1);
    assert_eq!(g.aid_preview(P, FormId(MAGAZINE)), vec![(STRENGTH, 2.0)]);
    assert_eq!(g.aid_preview(P, FormId(BLOOD)), vec![(HEALTH, 21.0)]);
    // A Medicine item at Medicine 15: potency 1.3.
    let stim = g.aid_preview(P, FormId(STIM));
    assert_eq!(stim.len(), 1);
    assert!(near(stim[0].1, 39.0), "{stim:?}");
    // Chemist lengthens a chem's time, so its effect over time counts longer.
    g.player.perks.insert(FormId(CHEMIST), 1);
    assert_eq!(g.aid_preview(P, FormId(BLOOD)), vec![(HEALTH, 41.0)]);
    // Nothing was used.
    assert!(g.effects.is_empty());
    assert_eq!(av(&mut g, STRENGTH), 5.0);
}

/// A Stimpak's effects split by IsHardcore (#1438): outside Hardcore it heals at once and
/// leaves no active effect (its Hardcore heal is held inactive for its 6 s, then gone, and
/// never takes hold if Hardcore comes on later); in Hardcore one heal over time runs.
#[test]
fn a_hardcore_gated_stimpak_applies_only_its_mode() {
    let f = fixture();
    let active = |g: &Game| g.effects.iter().filter(|e| e.target == P && e.source == FormId(STIM_GATED) && !e.inactive).count();
    // Outside Hardcore: 30 × 1.3 at once.
    let mut g = game(&f);
    add_actor_value_damage(&mut g, P, HEALTH, 60.0);
    assert_eq!(g.aid_preview(P, FormId(STIM_GATED)), vec![(HEALTH, 39.0)]);
    let r = g.ingest(P, FormId(STIM_GATED));
    assert_eq!(r.applied.len(), 1);
    assert!(near(av(&mut g, HEALTH), 79.0));
    assert_eq!(active(&g), 0);
    g.frames(6 * 60 + 2);
    assert!(!g.effects.iter().any(|e| e.source == FormId(STIM_GATED)), "{:?}", g.effects);
    g.player.hardcore = Some(true);
    g.frames(3 * 60);
    assert!(near(av(&mut g, HEALTH), 79.0));
    // In Hardcore: one heal over time, 5 × 1.3 floored to 6 a second for 6 s.
    let mut g = game(&f);
    g.player.hardcore = Some(true);
    add_actor_value_damage(&mut g, P, HEALTH, 60.0);
    assert_eq!(g.aid_preview(P, FormId(STIM_GATED)), vec![(HEALTH, 36.0)]);
    let r = g.ingest(P, FormId(STIM_GATED));
    assert_eq!(r.applied.len(), 1);
    assert!(near(av(&mut g, HEALTH), 40.0));
    assert_eq!(active(&g), 1);
    g.frames(6 * 60 + 2);
    assert!(near(av(&mut g, HEALTH), 76.0), "{}", av(&mut g, HEALTH));
    assert!(!g.effects.iter().any(|e| e.source == FormId(STIM_GATED)));
}

/// A Stimpak used outside Hardcore, then Hardcore turned on while its held heal's 6 s still
/// run: the heal takes hold within a condition check and heals only for the time left; turned
/// on after the 6 s, nothing [observed: o26_stim, 2026-11-06, nvfairport: Hardcore on 2.8 s
/// after the use, Health rose from 0.9 s later at the Hardcore rate until 5.9 s after the
/// use, 12 of the 36; on 9.3 s after, it stayed; issue #1743].
#[test]
fn a_held_hardcore_heal_takes_hold_for_the_time_left() {
    let f = fixture();
    for (on_after, low, high) in [(3 * 60, 11.9, 18.1), (10 * 60, 0.0, 0.01)] {
        let mut g = game(&f);
        add_actor_value_damage(&mut g, P, HEALTH, 60.0);
        g.ingest(P, FormId(STIM_GATED));
        let at_use = av(&mut g, HEALTH);
        g.frames(on_after);
        assert!(near(av(&mut g, HEALTH), at_use));
        g.player.hardcore = Some(true);
        g.frames(10 * 60);
        let gain = av(&mut g, HEALTH) - at_use;
        assert!(gain >= low && gain <= high, "Hardcore on after {on_after} frames: gained {gain}");
    }
}

/// Weapon poisons: an ingestible whose effects are all Hostile and Detrimental is a poison;
/// with no weapon in hand that takes one it is refused (the caller gives it back).
#[test]
fn a_poison_needs_a_weapon_that_takes_it() {
    use fnv_game::weapon_poison::PoisonUse;
    let f = fixture();
    let mut g = game(&f);
    assert!(g.is_poison(FormId(POISON)));
    assert!(!g.is_poison(FormId(STIM)) && !g.is_poison(FormId(BUFF)));
    assert_eq!(g.use_poison(FormId(POISON)), PoisonUse::NoWeapon);
    assert!(g.player.weapon_poisons.is_empty() && g.player.poison_asked.is_none());
}

/// #3119: the preview names a Script effect flagged Display Name Only (Psycho's), not one
/// without the flag, beside the values; an item with none names nothing.
#[test]
fn the_preview_names_script_effects_shown_by_name() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(g.aid_preview_names(P, FormId(PSYCHO)), vec![FormId(NAMED_FX)]);
    assert_eq!(g.aid_preview(P, FormId(PSYCHO)), vec![(STRENGTH, 1.0)]);
    assert!(g.aid_preview_names(P, FormId(STIM)).is_empty());
    let m = &g.data.magic_effects[&FormId(NAMED_FX)];
    assert!(m.shows_name());
    assert!(!g.data.magic_effects[&FormId(SCRIPT_FX)].shows_name());
}

/// What a regeneration of `magnitude` gives in `seconds` (aid-items.md §3).
fn regen(magnitude: f64, seconds: f64) -> f64 {
    magnitude * seconds * fnv_game::aid::REGENERATION_RATE
}

/// The player at Health 100 hurt by `hurt`, with `rads`, in their stage (and the perk).
fn irradiated(f: &Fixture, hurt: f64, rads: f64, rad_child: bool) -> Game<'_> {
    let mut g = game(f);
    if rad_child {
        g.player.perks.insert(FormId(RAD_CHILD), 1);
    }
    set_actor_value_current(&mut g, P, RAD_LEVEL, rads).unwrap();
    g.frames(1);
    add_actor_value_damage(&mut g, P, HEALTH, hurt);
    g
}

/// #3471: with Rad Child a stage of radiation sickness gives Health back by the second,
/// up to the maximum; the next stage's rate takes over, and it stops with the perk or
/// below the first stage (aid-items.md §3).
#[test]
fn rad_child_regenerates_by_the_second_in_a_stage() {
    let f = fixture();
    // Without the perk, nothing.
    let mut g = irradiated(&f, 80.0, 250.0, false);
    g.frames(600);
    assert!(near(av(&mut g, HEALTH), 20.0));
    // The perk is noticed within a second (the conditions' check).
    g.player.perks.insert(FormId(RAD_CHILD), 1);
    g.frames(62);
    let h = av(&mut g, HEALTH);
    g.frames(600);
    let now = av(&mut g, HEALTH);
    assert!((now - h - regen(2.0, 10.0)).abs() < 1e-3, "{h} -> {now}");
    // The second stage: its rate alone.
    set_actor_value_current(&mut g, P, RAD_LEVEL, 450.0).unwrap();
    g.frames(2);
    let h = av(&mut g, HEALTH);
    g.frames(300);
    let now = av(&mut g, HEALTH);
    assert!((now - h - regen(4.0, 5.0)).abs() < 1e-3, "{h} -> {now}");
    // Back to the first.
    set_actor_value_current(&mut g, P, RAD_LEVEL, 399.0).unwrap();
    g.frames(2);
    let h = av(&mut g, HEALTH);
    g.frames(60);
    let now = av(&mut g, HEALTH);
    assert!((now - h - regen(2.0, 1.0)).abs() < 1e-3, "{h} -> {now}");
    // Not past the maximum.
    g.pass_hours(10.0, false);
    assert_eq!(av(&mut g, HEALTH), 100.0);
    g.frames(60);
    assert_eq!(av(&mut g, HEALTH), 100.0);
    // The perk gone: it stops.
    add_actor_value_damage(&mut g, P, HEALTH, 50.0);
    g.player.perks.remove(&FormId(RAD_CHILD));
    g.frames(62);
    let h = av(&mut g, HEALTH);
    g.frames(300);
    assert_eq!(av(&mut g, HEALTH), h);
    // Rads under the first stage: it stops at once.
    let mut g = irradiated(&f, 50.0, 250.0, true);
    g.frames(60);
    assert!(av(&mut g, HEALTH) > 50.0);
    set_actor_value_current(&mut g, P, RAD_LEVEL, 199.0).unwrap();
    g.frames(2);
    let h = av(&mut g, HEALTH);
    g.frames(300);
    assert_eq!(av(&mut g, HEALTH), h);
    // Nor does a body regenerate.
    let mut g = irradiated(&f, 50.0, 250.0, true);
    g.extra.entry(P).or_default().dead = true;
    let h = av(&mut g, HEALTH);
    g.frames(60);
    assert_eq!(av(&mut g, HEALTH), h);
}

/// #3471: saved while active, the regeneration goes on at its rate, neither lost nor
/// twice; one saved as a single restoration (before the fix) goes on by the second too.
#[test]
fn a_regeneration_is_saved() {
    use fnv_game::aid::ChangeKind;
    let f = fixture();
    let mut g = irradiated(&f, 80.0, 250.0, true);
    g.frames(120);
    let before = av(&mut g, HEALTH);
    for old in [false, true] {
        let mut s = g.snapshot();
        for e in s.effects.iter_mut().filter(|e| old && e.effect == FormId(REGEN)) {
            e.change.as_mut().unwrap().kind = ChangeKind::Once;
        }
        let text = serde_json::to_string(&s).unwrap();
        let back: fnv_game::game::GameSnapshot = serde_json::from_str(&text).unwrap();
        let world = World::restore(&f.forms, &f.refs, &back.sim).unwrap();
        let mut h = Game::restore(&f.data, world, g.cfg, &back).unwrap();
        assert!(near(av(&mut h, HEALTH), before), "old {old}");
        h.frames(300);
        assert_eq!(h.effects.iter().filter(|e| e.target == P && e.effect == FormId(REGEN)).count(), 1);
        let now = av(&mut h, HEALTH);
        assert!((now - before - regen(2.0, 5.0)).abs() < 1e-3, "old {old}: {before} -> {now}");
    }
}

/// #3471: a wait gives the regeneration of the seconds of play its hours stand for, up
/// to the maximum, and checks the conditions first.
#[test]
fn a_wait_regenerates_for_its_seconds() {
    let f = fixture();
    let mut g = irradiated(&f, 80.0, 250.0, true);
    let h = av(&mut g, HEALTH);
    let passed = g.pass_hours(0.1, false);
    assert!(near(passed.seconds, 12.0));
    let now = av(&mut g, HEALTH);
    assert!((now - (h + regen(2.0, 12.0)).min(100.0)).abs() < 1e-6, "{h} -> {now}");
    g.pass_hours(1.0, false);
    let next = av(&mut g, HEALTH);
    assert!((next - (now + regen(2.0, 120.0)).min(100.0)).abs() < 1e-6, "{now} -> {next}");
    // The perk just gone: the wait does not heal.
    let mut g = irradiated(&f, 80.0, 250.0, true);
    g.frames(1);
    g.player.perks.remove(&FormId(RAD_CHILD));
    let h = av(&mut g, HEALTH);
    g.pass_hours(1.0, false);
    assert_eq!(av(&mut g, HEALTH), h);
}

/// #3471: what is no permanent regeneration acts once, as before.
#[test]
fn other_restorations_still_act_once() {
    let f = fixture();
    let mut g = game(&f);
    add_actor_value_damage(&mut g, P, HEALTH, 60.0);
    g.apply_magic_item(P, FormId(REGEN_CAST), None, false);
    assert!(near(av(&mut g, HEALTH), 45.0));
    call(&mut g, 0x101C, &[form(HEAL_ABILITY)]);
    assert!(near(av(&mut g, HEALTH), 55.0));
    call(&mut g, 0x101C, &[form(HURT_ABILITY)]);
    g.frames(120);
    assert!(near(av(&mut g, HEALTH), 45.0));
}

/// `[Sleep Healing] bMiscStatRequiresSleepDuration`: with a minimum sleep duration set,
/// Times Slept increments only when the sleep meets that duration unless the option is off.
#[test]
fn sleep_misc_stat_requires_duration_gates_times_slept() {
    use fnv_game::misc_stats::TIMES_SLEPT;
    let f = fixture();

    // Default (sleep_heal_min_hours = 0): any sleep counts.
    let mut g = game(&f);
    assert_eq!(g.tweaks.sleep_heal_min_hours, 0);
    assert!(g.tweaks.sleep_misc_stat_requires_duration);
    g.begin_rest(true, 1);
    assert_eq!(g.misc_stat(TIMES_SLEPT), 1.0);

    // Min duration 6h, bMiscStatRequiresSleepDuration = true: 3h does not count, 6h does.
    let mut g = game(&f);
    g.tweaks.sleep_heal_min_hours = 6;
    g.tweaks.sleep_misc_stat_requires_duration = true;
    g.begin_rest(true, 3);
    assert_eq!(g.misc_stat(TIMES_SLEPT), 0.0, "short sleep skipped");
    g.rest.end();
    g.begin_rest(true, 6);
    assert_eq!(g.misc_stat(TIMES_SLEPT), 1.0, "full duration sleep counts");

    // Min duration 6h, bMiscStatRequiresSleepDuration = false: 3h still counts.
    let mut g = game(&f);
    g.tweaks.sleep_heal_min_hours = 6;
    g.tweaks.sleep_misc_stat_requires_duration = false;
    g.begin_rest(true, 3);
    assert_eq!(g.misc_stat(TIMES_SLEPT), 1.0);
}

/// `[Grabbing Items Is Crime] fMinValue`: grabbing an owned item counts as stealing only
/// when the grabbed stack's base value (unit value × count) meets `fMinValue`.
#[test]
fn grab_triggers_theft_respects_min_value_and_stack_count() {
    const VAL0: u32 = 0x901;
    const VAL5: u32 = 0x902;
    const VAL15: u32 = 0x903;
    let misc = |id: u32, edid: &str, value: i32| {
        record(b"MISC", 0, id, &[sub(b"EDID", z(edid)), sub(b"DATA", [value.to_le_bytes(), 1.0f32.to_le_bytes()].concat())])
    };
    let bytes = [
        tes4(),
        top(b"MISC", &[misc(VAL0, "Worthless", 0), misc(VAL5, "Cheap", 5), misc(VAL15, "Costly", 15)].concat()),
        top(b"NPC_", &record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))])),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &record(b"CELL", 0, CELL, &[sub(b"EDID", z("Room"))])))),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "GrabVal.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    let world = World::new(&forms, &refs, SimConfig::default());
    let mut g = Game::new(&data, world, GameConfig::default());

    // Tweak off: never triggers.
    assert!(!g.grab_triggers_theft(FormId(VAL15), 1));

    // fMinValue = 0.0: even worthless items trigger.
    g.tweaks.grabbing_is_theft = true;
    g.tweaks.grabbing_is_theft_min_value = 0.0;
    assert!(g.grab_triggers_theft(FormId(VAL0), 1));
    assert!(g.grab_triggers_theft(FormId(VAL5), 1));

    // fMinValue = 10.0: single 5-cap item skipped; stack of 2x5 = 10 triggers; single 15 triggers.
    g.tweaks.grabbing_is_theft_min_value = 10.0;
    assert!(!g.grab_triggers_theft(FormId(VAL0), 1));
    assert!(!g.grab_triggers_theft(FormId(VAL5), 1));
    assert!(g.grab_triggers_theft(FormId(VAL5), 2));
    assert!(g.grab_triggers_theft(FormId(VAL15), 1));
}
