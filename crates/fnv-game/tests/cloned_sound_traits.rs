//! `TempCloneForm` SOUN aliases and `SetSoundSourceFile` / `SetSoundTraitNumeric`
//! inheritance and isolation in `fnv-game`.

use crate::common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{Game, GameConfig, RunCtx};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use std::path::Path;

const SOUN_ORIG: u32 = 0x0000_0D50;
const STAT_OTHER: u32 = 0x0000_0D51;

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let mut sndd = vec![0u8; 36];
    // SNDD layout (schemas/fnv/objects.json):
    // 0: min_attenuation_distance (u8, *5 -> 200)
    // 1: max_attenuation_distance (u8, *100 -> 2800)
    // 2: frequency_adjustment_percent (i8 -> 5)
    // 8..10: static_attenuation_cdb (i16 LE -> 1200 => -12.0 dB)
    // 10: stop_time (u8 -> 128)
    // 11: start_time (u8 -> 64)
    // 12..22: attenuation_curve ([i16; 5] LE -> 100, 60, 30, 10, 0)
    // 22..24: reverb_attenuation_control (i16 LE -> 40)
    // 24..28: priority (i32 LE -> 7)
    sndd[0] = 40;
    sndd[1] = 28;
    sndd[2] = 5;
    sndd[8..10].copy_from_slice(&1200i16.to_le_bytes());
    sndd[10] = 128;
    sndd[11] = 64;
    for (i, v) in [100i16, 60, 30, 10, 0].into_iter().enumerate() {
        sndd[12 + i * 2..14 + i * 2].copy_from_slice(&v.to_le_bytes());
    }
    sndd[22..24].copy_from_slice(&40i16.to_le_bytes());
    sndd[24..28].copy_from_slice(&7i32.to_le_bytes());

    let mut bytes = tes4();
    bytes.extend(top(
        b"SOUN",
        &record(
            b"SOUN",
            0,
            SOUN_ORIG,
            &[sub(b"EDID", z("OwnedMenuFocus")), sub(b"FNAM", z("fx\\ui\\owned_focus.wav")), sub(b"SNDD", sndd)],
        ),
    ));
    bytes.extend(top(b"STAT", &record(b"STAT", 0, STAT_OTHER, &[sub(b"EDID", z("OwnedStatic"))])));
    let plugins = vec![Plugin::parse(&bytes, "OwnedSoundClones.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let functions = root.join("script/functions.json");
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&functions).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, FunctionTable::load(&functions).unwrap(), &json, &refs);
    Fixture { data, forms, refs }
}

fn call(g: &mut Game, name: &str, args: &[XVal]) -> XVal {
    fnv_game::nvse::call(g, &RunCtx::default(), name, None, args).unwrap().unwrap()
}

#[test]
fn temp_cloned_sounds_inherit_isolate_and_chain_source_and_numeric_traits() {
    let f = fixture();
    let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() });
    let orig = FormId(SOUN_ORIG);

    // Baseline authored SOUN values.
    assert_eq!(call(&mut g, "getsoundsourcefile", &[XVal::Ref(orig.0)]), XVal::Str("fx\\ui\\owned_focus.wav".into()));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(orig.0), XVal::Num(0.0)]), XVal::Num(200.0));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(orig.0), XVal::Num(1.0)]), XVal::Num(2800.0));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(orig.0), XVal::Num(3.0)]), XVal::Num(-12.0));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(orig.0), XVal::Num(11.0)]), XVal::Num(40.0));

    // Direct TempCloneForm inherits authored values.
    let XVal::Ref(c_unmodified_raw) = call(&mut g, "tempcloneform", &[XVal::Ref(orig.0), XVal::Num(1.0)]) else {
        panic!("expected unmodified clone");
    };
    let c_unmodified = FormId(c_unmodified_raw);
    assert_ne!(c_unmodified, orig);
    assert_eq!(call(&mut g, "getsoundsourcefile", &[XVal::Ref(c_unmodified.0)]), XVal::Str("fx\\ui\\owned_focus.wav".into()));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(c_unmodified.0), XVal::Num(3.0)]), XVal::Num(-12.0));

    // Mutating orig AFTER cloning does NOT retroactively leak into already-created clones,
    // while a clone created AFTER the edit snapshots orig's active session overrides.
    call(&mut g, "setsoundsourcefile", &[XVal::Ref(orig.0), XVal::Str("fx\\owned\\silence.wav".into())]);
    call(&mut g, "setsoundtraitnumeric", &[XVal::Ref(orig.0), XVal::Num(3.0), XVal::Num(-99.0)]);
    assert_eq!(call(&mut g, "getsoundsourcefile", &[XVal::Ref(orig.0)]), XVal::Str("fx\\owned\\silence.wav".into()));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(orig.0), XVal::Num(3.0)]), XVal::Num(-99.0));
    assert_eq!(call(&mut g, "getsoundsourcefile", &[XVal::Ref(c_unmodified.0)]), XVal::Str("fx\\ui\\owned_focus.wav".into()));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(c_unmodified.0), XVal::Num(3.0)]), XVal::Num(-12.0));
    assert_eq!(g.sound_source_override(c_unmodified), None);
    assert_eq!(g.sound_trait_override(c_unmodified, 3), None);

    let XVal::Ref(c1_raw) = call(&mut g, "tempcloneform", &[XVal::Ref(orig.0), XVal::Num(1.0)]) else {
        panic!("expected clone 1");
    };
    let c1 = FormId(c1_raw);
    assert_eq!(call(&mut g, "getsoundsourcefile", &[XVal::Ref(c1.0)]), XVal::Str("fx\\owned\\silence.wav".into()));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(c1.0), XVal::Num(3.0)]), XVal::Num(-99.0));

    // Overriding clone 1 leaves original untouched.
    call(&mut g, "setsoundsourcefile", &[XVal::Ref(c1.0), XVal::Str("fx\\owned\\scope_click.wav".into())]);
    call(&mut g, "setsoundtraitnumeric", &[XVal::Ref(c1.0), XVal::Num(3.0), XVal::Num(-25.0)]);
    assert_eq!(g.sound_source_override(c1), Some("fx\\owned\\scope_click.wav"));
    assert_eq!(g.sound_trait_override(c1, 3), Some(-25.0));
    assert_eq!(call(&mut g, "getsoundsourcefile", &[XVal::Ref(c1.0)]), XVal::Str("fx\\owned\\scope_click.wav".into()));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(c1.0), XVal::Num(3.0)]), XVal::Num(-25.0));
    assert_eq!(call(&mut g, "getsoundsourcefile", &[XVal::Ref(orig.0)]), XVal::Str("fx\\owned\\silence.wav".into()));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(orig.0), XVal::Num(3.0)]), XVal::Num(-99.0));

    // Clone-of-clone snapshots clone 1's active overrides; subsequent edits on clone 1 do
    // NOT retroactively mutate clone 2, and clone 2 can independently set an empty path.
    let XVal::Ref(c2_raw) = call(&mut g, "tempcloneform", &[XVal::Ref(c1.0), XVal::Num(1.0)]) else {
        panic!("expected clone 2");
    };
    let c2 = FormId(c2_raw);
    assert_eq!(call(&mut g, "getsoundsourcefile", &[XVal::Ref(c2.0)]), XVal::Str("fx\\owned\\scope_click.wav".into()));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(c2.0), XVal::Num(3.0)]), XVal::Num(-25.0));
    call(&mut g, "setsoundsourcefile", &[XVal::Ref(c1.0), XVal::Str("fx\\owned\\later_c1.wav".into())]);
    call(&mut g, "setsoundtraitnumeric", &[XVal::Ref(c1.0), XVal::Num(3.0), XVal::Num(-40.0)]);
    assert_eq!(call(&mut g, "getsoundsourcefile", &[XVal::Ref(c2.0)]), XVal::Str("fx\\owned\\scope_click.wav".into()));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(c2.0), XVal::Num(3.0)]), XVal::Num(-25.0));
    call(&mut g, "setsoundsourcefile", &[XVal::Ref(c2.0), XVal::Str(String::new())]);
    call(&mut g, "setsoundtraitnumeric", &[XVal::Ref(c2.0), XVal::Num(3.0), XVal::Num(-80.0)]);
    assert_eq!(call(&mut g, "getsoundsourcefile", &[XVal::Ref(c2.0)]), XVal::Str(String::new()));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(c2.0), XVal::Num(3.0)]), XVal::Num(-80.0));
    assert_eq!(call(&mut g, "getsoundsourcefile", &[XVal::Ref(c1.0)]), XVal::Str("fx\\owned\\later_c1.wav".into()));
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(c1.0), XVal::Num(3.0)]), XVal::Num(-40.0));

    // PlaySound on a cloned SOUN preserves the exact cloned FormID in sound_requests.
    let res = g
        .run_query(
            "ref c\nBegin Function { }\nlet c := TempCloneForm OwnedMenuFocus 1\nSetSoundSourceFile c \"fx\\owned\\inject.wav\"\nPlaySound c 1\nSetFunctionValue c\nEnd\n",
            RunCtx::default(),
        )
        .unwrap();
    let XVal::Ref(played_clone) = res else {
        panic!("expected played clone FormId");
    };
    assert!(g.sound_requests.iter().any(|r| r.sound == FormId(played_clone) && r.system));
    assert_eq!(g.sound_source_override(FormId(played_clone)), Some("fx\\owned\\inject.wav"));

    // Trait 12 is ignored/unwritable by SetSoundTraitNumeric [observed: w4c_b1_21_sound_and_diff_bounds].
    let p0 = call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(orig.0), XVal::Num(12.0)]);
    call(&mut g, "setsoundtraitnumeric", &[XVal::Ref(orig.0), XVal::Num(12.0), XVal::Num(77.0)]);
    assert_eq!(call(&mut g, "getsoundtraitnumeric", &[XVal::Ref(orig.0), XVal::Num(12.0)]), p0);
    assert_eq!(g.sound_trait_override(orig, 12), None);
}
