//! #3615: base-game functions players' games call that were not registered (SameFaction,
//! SameSex, HasFriendDisposition, GetFatiguePercentage, GetTimeDead, GetLastHitCritical,
//! GetIsObjectType, GetVampire), through compiled script source on a synthetic plugin.
use crate::common::*;
use crate::game::{GameConfig, LastHit, RunCtx};
use crate::{Game, GameData};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_script::{vm::XVal, FunctionTable};
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};

const A: FormId = FormId(0x830);
const B: FormId = FormId(0x831);
/// MemberA (faction A), MemberB (faction B), OtherA (faction A), a box.
const MEMBER_A: FormId = FormId(0x301);
const MEMBER_B: FormId = FormId(0x302);
const OTHER_A: FormId = FormId(0x303);
const DAYS: FormId = FormId(0x860);

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let actor = |id: u32, edid: &str, faction: FormId| {
        record(b"NPC_", 0, id, &[sub(b"EDID", z(edid)), sub(b"SNAM", [faction.0.to_le_bytes().to_vec(), vec![0; 4]].concat())])
    };
    let placed = |sig: &[u8; 4], id: u32, base: u32, edid: &str| {
        record(sig, 0, id, &[sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])])
    };
    let refs = [
        placed(b"ACHR", MEMBER_A.0, 0x801, "MemberARef"),
        placed(b"ACHR", MEMBER_B.0, 0x802, "MemberBRef"),
        placed(b"ACHR", OTHER_A.0, 0x803, "OtherARef"),
        placed(b"REFR", 0x304, 0x850, "BoxRef"),
    ]
    .concat();
    let cell = [record(b"CELL", 0, 0x200, &[sub(b"DATA", vec![1])]), group(0x200u32.to_le_bytes(), 6, &group(0x200u32.to_le_bytes(), 8, &refs))].concat();
    let bytes = [
        tes4(),
        top(b"GLOB", &record(b"GLOB", 0, DAYS.0, &[sub(b"EDID", z("GameDaysPassed")), sub(b"FNAM", vec![b'f']), sub(b"FLTV", 2.0f32.to_le_bytes().to_vec())])),
        top(b"FACT", &[record(b"FACT", 0, A.0, &[sub(b"EDID", z("FirstFaction"))]), record(b"FACT", 0, B.0, &[sub(b"EDID", z("SecondFaction"))])].concat()),
        top(
            b"NPC_",
            &[
                record(b"NPC_", 0, FormId::PLAYER_BASE.0, &[sub(b"EDID", z("Player"))]),
                actor(0x801, "MemberA", A),
                actor(0x802, "MemberB", B),
                actor(0x803, "OtherA", A),
            ]
            .concat(),
        ),
        top(b"CONT", &record(b"CONT", 0, 0x850, &[sub(b"EDID", z("Box"))])),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell))),
    ]
    .concat();
    let plugins = vec![Plugin::parse(&bytes, "BaseFunctions.esm".into()).unwrap()];
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

fn run(g: &mut Game<'_>, body: &str) {
    g.run_query(&format!("Begin Function {{}}\n{body}\nEnd\n"), RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() })
        .unwrap_or_else(|e| panic!("{body}: {e}"));
}

/// What `expr` reads, from a script.
fn n(g: &mut Game<'_>, expr: &str) -> f64 {
    let v = g
        .run_query(&format!("Begin Function {{}}\nSetFunctionValue ({expr})\nEnd\n"), RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() })
        .unwrap_or_else(|e| panic!("{expr}: {e}"));
    match v {
        XVal::Num(x) => x,
        other => panic!("{expr}: {other:?}"),
    }
}

#[test]
fn the_eight_are_registered() {
    for (op, name) in [
        (0x1028, "GetVampire"),
        (0x102A, "SameFaction"),
        (0x102C, "SameSex"),
        (0x1080, "GetFatiguePercentage"),
        (0x1169, "GetTimeDead"),
        (0x1193, "HasFriendDisposition"),
        (0x11B1, "GetIsObjectType"),
        (0x11FE, "GetLastHitCritical"),
    ] {
        assert!(crate::functions::registry().iter().any(|(o, nm, _)| *o == op && *nm == name), "{name} 0x{op:04X} not registered");
    }
}

#[test]
fn get_vampire_is_always_zero() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "MemberARef.GetVampire"), 0.0);
    assert_eq!(n(&mut g, "PlayerRef.GetVampire"), 0.0);
}

#[test]
fn same_faction_needs_one_faction_in_common() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "MemberARef.SameFaction OtherARef"), 1.0);
    assert_eq!(n(&mut g, "MemberARef.SameFaction MemberBRef"), 0.0);
    run(&mut g, "MemberBRef.AddToFaction FirstFaction 0");
    assert_eq!(n(&mut g, "MemberARef.SameFaction MemberBRef"), 1.0);
    // Rank -1 is no membership.
    run(&mut g, "MemberBRef.SetFactionRank FirstFaction -1");
    assert_eq!(n(&mut g, "MemberARef.SameFaction MemberBRef"), 0.0);
}

#[test]
fn same_sex_follows_sex_change() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "MemberARef.SameSex MemberBRef"), 1.0);
    run(&mut g, "MemberBRef.SexChange 1");
    assert_eq!(n(&mut g, "MemberARef.SameSex MemberBRef"), 0.0);
    run(&mut g, "MemberARef.SexChange 1");
    assert_eq!(n(&mut g, "MemberARef.SameSex MemberBRef"), 1.0);
}

/// #3787: SameSexAsPC is SameSex with the player as the other actor.
#[test]
fn same_sex_as_pc_compares_with_the_player() {
    assert!(crate::functions::registry().iter().any(|(o, nm, _)| *o == 0x1087 && *nm == "SameSexAsPC"));
    let f = fixture();
    let mut g = game(&f);
    run(&mut g, "MemberARef.SexChange 1");
    run(&mut g, "PlayerRef.SexChange 1");
    assert_eq!(n(&mut g, "MemberARef.SameSexAsPC"), 1.0);
    run(&mut g, "PlayerRef.SexChange 0");
    assert_eq!(n(&mut g, "MemberARef.SameSexAsPC"), 0.0);
    assert_eq!(n(&mut g, "MemberARef.SameSexAsPC"), n(&mut g, "MemberARef.SameSex PlayerRef"));
    assert_eq!(n(&mut g, "PlayerRef.SameSexAsPC"), 1.0);
}

#[test]
fn friend_disposition_is_towards_the_player_at_the_friend_level() {
    let f = fixture();
    let mut g = game(&f);
    let at = crate::functions::actor::FRIEND_DISPOSITION;
    run(&mut g, &format!("MemberARef.SetDisposition PlayerRef {at}"));
    assert_eq!(n(&mut g, "MemberARef.HasFriendDisposition"), 1.0);
    run(&mut g, &format!("MemberARef.SetDisposition PlayerRef {}", at - 1.0));
    assert_eq!(n(&mut g, "MemberARef.HasFriendDisposition"), 0.0);
    // Towards someone else does not count.
    run(&mut g, "MemberARef.SetDisposition OtherARef 100");
    assert_eq!(n(&mut g, "MemberARef.HasFriendDisposition"), 0.0);
}

#[test]
fn fatigue_percentage_is_current_over_base() {
    let f = fixture();
    let mut g = game(&f);
    // The synthetic NPC has no ACBS: a base of 0 reads 1, then a base is given.
    assert_eq!(n(&mut g, "MemberARef.GetFatiguePercentage"), 1.0);
    run(&mut g, "MemberARef.SetAV Fatigue 200");
    assert_eq!(n(&mut g, "MemberARef.GetFatiguePercentage"), 1.0);
    let base = n(&mut g, "MemberARef.GetBaseAV Fatigue");
    assert!(base > 0.0, "a base fatigue to divide by: {base}");
    run(&mut g, &format!("MemberARef.DamageAV Fatigue {}", base / 4.0));
    let now = n(&mut g, "MemberARef.GetAV Fatigue");
    let pct = n(&mut g, "MemberARef.GetFatiguePercentage");
    assert!((pct - 0.75).abs() < 1e-6, "base {base}, now {now}, percentage {pct}");
}

#[test]
fn time_dead_counts_hours_from_the_death_and_forgets_on_disable() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "MemberARef.GetTimeDead"), 0.0, "alive");
    run(&mut g, "MemberARef.KillActor");
    assert_eq!(n(&mut g, "MemberARef.GetTimeDead"), 0.0, "just died");
    run(&mut g, "set GameDaysPassed to 2.5");
    assert!((n(&mut g, "MemberARef.GetTimeDead") - 12.0).abs() < 1e-3, "half a day later");
    run(&mut g, "MemberARef.Disable");
    assert_eq!(n(&mut g, "MemberARef.GetTimeDead"), 0.0, "disabled");
    run(&mut g, "MemberARef.Enable");
    assert_eq!(n(&mut g, "MemberARef.GetTimeDead"), 0.0, "re-enabled: the time is lost");
}

#[test]
fn last_hit_critical_reads_the_last_hit() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(n(&mut g, "MemberARef.GetLastHitCritical"), 0.0, "never hit");
    let hit = |critical| LastHit { attacker: Some(FormId::PLAYER_REF), weapon: None, location: 0, critical };
    g.extra.entry(MEMBER_A).or_default().last_hit = Some(hit(true));
    assert_eq!(n(&mut g, "MemberARef.GetLastHitCritical"), 1.0);
    g.extra.entry(MEMBER_A).or_default().last_hit = Some(hit(false));
    assert_eq!(n(&mut g, "MemberARef.GetLastHitCritical"), 0.0);
}

#[test]
fn is_object_type_reads_the_references_base() {
    let f = fixture();
    let mut g = game(&f);
    // `Container` is code 27, the one form type name the compiler spells.
    assert_eq!(n(&mut g, "BoxRef.GetIsObjectType Container"), 1.0);
    assert_eq!(n(&mut g, "MemberARef.GetIsObjectType Container"), 0.0);
}
