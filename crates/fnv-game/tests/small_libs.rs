//! The small extender libraries (#1013, #1015-#1020): kNVSE's second slice, JIP PP LN,
//! vheyes-nvse, Anh's and CacheUI's last functions, NX_GetQVEVFL and the small plugins
//! (docs/spec-notes/ext-knvse.md, ext-jippp.md, ext-vheyes.md, ext-small.md), run through
//! scripts on a synthetic plugin and made-up files (no game data).

mod common;

use common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::game::{ActiveEffect, Game, GameConfig};
use fnv_game::GameData;
use fnv_script::vm::XVal;
use fnv_script::FunctionTable;
use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};
use fnv_ui::MemSource;
use serde_json::Value as Json;
use std::sync::Arc;

const WEAPON: u32 = 0x800;
const AMMO: u32 = 0x801;
const AMMO_EFFECT: u32 = 0x802;
const LIGHT: u32 = 0x803;
const WORLD: u32 = 0x804;
const REGION: u32 = 0x805;
const OUTSIDE: u32 = 0x806;
const INSIDE: u32 = 0x807;
const BOX: u32 = 0x808;
const BOX_REF: u32 = 0x809;
const QUEST: u32 = 0x80A;
const SPELL: u32 = 0x80B;
const EFFECT: u32 = 0x80C;

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn plugin_bytes() -> Vec<u8> {
    // WEAP DNAM (204 bytes): flags2 at 56 with bit 7 (Override - Damage to Weapon Mult),
    // the override at 84, the skill at 104 (schemas/fnv WeaponDNAM).
    let mut dnam = vec![0u8; 204];
    dnam[56..60].copy_from_slice(&(1u32 << 7).to_le_bytes());
    dnam[84..88].copy_from_slice(&0.5f32.to_le_bytes());
    dnam[104..108].copy_from_slice(&38u32.to_le_bytes());
    let weap_data = [&10i32.to_le_bytes()[..], &100i32.to_le_bytes(), &0.5f32.to_le_bytes(), &10i16.to_le_bytes(), &[12u8]].concat();
    let weap = record(b"WEAP", 0, WEAPON, &[sub(b"EDID", z("TestGun")), sub(b"DATA", weap_data), sub(b"DNAM", dnam)]);
    // AMEF DATA: Weapon Condition Mod (4), Multiply (1), 3.
    let amef = record(b"AMEF", 0, AMMO_EFFECT, &[sub(b"EDID", z("TestWear")), sub(b"DATA", [4u32.to_le_bytes().to_vec(), 1u32.to_le_bytes().to_vec(), f32b(3.0)].concat())]);
    let ammo = record(b"AMMO", 0, AMMO, &[sub(b"EDID", z("TestAmmo")), sub(b"RCIL", AMMO_EFFECT.to_le_bytes().to_vec())]);
    let ligh_data = [0u32.to_le_bytes().to_vec(), 200u32.to_le_bytes().to_vec(), vec![10, 20, 30, 0], 0u32.to_le_bytes().to_vec(), f32b(1.0), f32b(90.0), 0u32.to_le_bytes().to_vec(), f32b(0.0)].concat();
    let ligh = record(b"LIGH", 0, LIGHT, &[sub(b"EDID", z("TestLight")), sub(b"DATA", ligh_data)]);
    let wrld = record(b"WRLD", 0, WORLD, &[sub(b"EDID", z("TestWorld"))]);
    let regn = record(b"REGN", 0, REGION, &[sub(b"EDID", z("TestRegion"))]);
    let acti = record(b"ACTI", 0, BOX, &[sub(b"EDID", z("TestBox")), sub(b"MODL", z("Test\\Box.nif"))]);
    let qust = record(b"QUST", 0, QUEST, &[sub(b"EDID", z("TestQuest"))]);
    let mgef = record(b"MGEF", 0, EFFECT, &[sub(b"EDID", z("TestEffect"))]);
    let spel = record(b"SPEL", 0, SPELL, &[sub(b"EDID", z("TestSpell"))]);
    let outside = record(b"CELL", 0, OUTSIDE, &[sub(b"EDID", z("TestOutside")), sub(b"DATA", vec![0]), sub(b"XCLR", REGION.to_le_bytes().to_vec())]);
    let inside = record(b"CELL", 0, INSIDE, &[sub(b"EDID", z("TestInside")), sub(b"DATA", vec![1]), sub(b"XCLR", REGION.to_le_bytes().to_vec())]);
    let box_ref = record(b"REFR", 0, BOX_REF, &[sub(b"EDID", z("TestBoxRef")), sub(b"NAME", BOX.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
    let children = group(INSIDE.to_le_bytes(), 6, &group(INSIDE.to_le_bytes(), 9, &box_ref));
    [
        tes4(),
        top(b"MGEF", &mgef),
        top(b"ACTI", &acti),
        top(b"LIGH", &ligh),
        top(b"WEAP", &weap),
        top(b"AMMO", &ammo),
        top(b"AMEF", &amef),
        top(b"REGN", &regn),
        top(b"SPEL", &spel),
        top(b"QUST", &qust),
        top(b"NPC_", &record(b"NPC_", 0, FormId::PLAYER_BASE.0, &[sub(b"EDID", z("Player"))])),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[outside, inside, children].concat()))),
        top(b"WRLD", &wrld),
    ]
    .concat()
}

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Small.esm".into()).unwrap()];
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
    let mut g = Game::new(&f.data, world, GameConfig { strict: true, ..GameConfig::default() });
    g.player_entered(Some(FormId(INSIDE)));
    let src = MemSource::default().with(
        "menus\\type.xml",
        r#"<menu name="TypeMenu"> <rect name="Line"> <_TS0> </_TS0> <_TS1> </_TS1> <_TS2> </_TS2>
             <_TA0> 0 </_TA0> <_TA1> 0 </_TA1> <_TA2> 0 </_TA2> <_flag> 0 </_flag> </rect> </menu>"#,
    );
    g.ui_tree.load_menu(&src, "menus\\type.xml").unwrap();
    g.ui_tree.update();
    g
}

#[test]
fn source_get_is_form_type_uses_the_reference_base_signature() {
    let f = fixture();
    let mut g = game(&f);
    for (expression, expected) in [
        ("TestBoxRef.GetIsFormType \"ACTI\"", 1.0),
        ("TestBoxRef.GetIsFormType \"acti\"", 0.0),
        ("TestBoxRef.GetIsFormType \"Acti\"", 0.0),
        ("TestBoxRef.GetIsFormType \"aCtI\"", 0.0),
        ("TestBoxRef.GetIsFormType \"REFR\"", 0.0),
        ("TestBoxRef.GetIsFormType \"MISC\"", 0.0),
        ("TestBoxRef.GetIsFormType \"ACT\"", 0.0),
        ("TestBoxRef.GetIsFormType \"ACTIS\"", 0.0),
        ("TestBoxRef.GetIsFormType \"\"", 0.0),
        // Direct base forms return 0 and continue (w4g_01_getisformtype_case).
        ("TestGun.GetIsFormType \"WEAP\"", 0.0),
        ("TestGun.GetIsFormType \"weap\"", 0.0),
        ("TestGun.GetIsFormType \"Weap\"", 0.0),
        ("TestGun.GetIsFormType \"Weapon\"", 0.0),
        ("TestBox.GetIsFormType \"ACTI\"", 0.0),
        ("GetIsFormType \"ACTI\"", 0.0),
    ] {
        assert_eq!(g.run_query(&format!("Begin Function {{}}\nSetFunctionValue ({expression})\nEnd"), Default::default()).unwrap(), XVal::Num(expected), "{expression}");
    }
    // Base-form call does not abort the script; subsequent statements still run.
    assert_eq!(
        g.run_query(
            "int a\nBegin Function {}\nlet a := TestGun.GetIsFormType \"WEAP\"\nSetFunctionValue (a + 7)\nEnd",
            Default::default(),
        )
        .unwrap(),
        XVal::Num(7.0),
    );
    // Placed/spawned reference of the same weapon base matches strictly uppercase "WEAP".
    assert_eq!(
        g.run_query(
            "ref w\nint u\nint l\nint m\nint d\nBegin Function {}\nlet w := player.PlaceAtMe TestGun 1\nlet u := w.GetIsFormType \"WEAP\"\nlet l := w.GetIsFormType \"weap\"\nlet m := w.GetIsFormType \"Weap\"\nlet d := w.GetIsFormType \"DOOR\"\nSetFunctionValue (u * 1000 + l * 100 + m * 10 + d)\nEnd",
            Default::default(),
        )
        .unwrap(),
        XVal::Num(1000.0),
    );
    // Temporary inventory reference preserves reference resolution and strict case-sensitivity.
    let XVal::Ref(temporary) = g.run_query("Begin Function {}\nplayer.AddItem TestGun 1\nSetFunctionValue (player.GetFirstRefForItem TestGun)\nEnd", Default::default()).unwrap() else { panic!("expected an inventory reference") };
    assert_ne!(temporary, 0);
    assert_eq!(fnv_game::nvse::call(&mut g, &Default::default(), "getisformtype", Some(FormId(temporary)), &[XVal::Str("WEAP".into())]), Some(Ok(XVal::Num(1.0))));
    assert_eq!(fnv_game::nvse::call(&mut g, &Default::default(), "getisformtype", Some(FormId(temporary)), &[XVal::Str("weap".into())]), Some(Ok(XVal::Num(0.0))));
}

/// A data folder made for the run: the KF files the kNVSE functions look for, Stewie's
/// Tweaks' INI and vheyes-nvse's queued command, and the plugin bytes TTW_CRC32 reads.
fn files() {
    use fnv_game::mod_files::{set_roots, Roots};
    let dir = std::env::temp_dir().join(format!("nvf-small-libs-{}", std::process::id()));
    // Written once per test process: the tests run in parallel, and writing a file again
    // empties it for a moment while another test may be reading it.
    static WRITE: std::sync::Once = std::sync::Once::new();
    WRITE.call_once(|| {
        std::fs::create_dir_all(dir.join("NVSE/Plugins")).unwrap();
        std::fs::create_dir_all(dir.join("NVSE/user_defined_functions")).unwrap();
        std::fs::create_dir_all(dir.join("Config/vheyes-nvse")).unwrap();
        std::fs::write(
            dir.join("NVSE/user_defined_functions/TileProbe.txt"),
            "Begin Function { }\nSetUIFloat \"HUDMainMenu/Probe/_calls\" ((GetUIFloat \"HUDMainMenu/Probe/_calls\") + 1)\nEnd\n",
        ).unwrap();
        std::fs::write(dir.join("NVSE/Plugins/nvse_stewie_tweaks.ini"), "[Tweaks]\r\nbJumpWhileAiming=1\r\n[Cookable Grenades]\r\nsSoundName=\"WPNGrenadeBeep\"\r\n").unwrap();
        std::fs::write(dir.join("Config/vheyes-nvse/QueuedCommand.txt"), "SetGlobalTimeMultiplier 2\r\n").unwrap();
        std::fs::write(dir.join("NVSE/Plugins/OwnedSibling.ini"), "[Main]\r\nValue=17\r\n").unwrap();
        std::fs::write(dir.join("Config/OwnedLocal.ini"), "[Main]\r\nValue=41\r\n").unwrap();
    });
    set_roots(Roots {
        read: vec![dir],
        lookup: Some(Arc::new(|p: &str| u8::from(p.to_ascii_lowercase().starts_with("meshes\\test\\")))),
        reader: Some(Arc::new(|p: &str| p.eq_ignore_ascii_case("Small.esm").then(|| b"123456789".to_vec()))),
        ..Roots::default()
    });
}

fn q(g: &mut Game, body: &str) -> XVal {
    g.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default()).unwrap_or_else(|e| panic!("{body}: {e}"))
}

fn n(v: f64) -> XVal {
    XVal::Num(v)
}

#[test]
fn compile_script_from_loose_root_runs_as_a_tile_callback() {
    files();
    let f = fixture();
    let mut g = game(&f);
    // [design: script_input.rs, mod_files.rs] The home mods layer is a read root;
    // the host VFS reader does not contain this loose callback.
    assert!(fnv_game::mod_files::read_data("nvse\\user_defined_functions\\tileprobe.txt").is_none());
    let src = MemSource::default().with(
        "menus\\probe.xml",
        r#"<menu name="HUDMainMenu"><rect name="Probe"><_probe>1</_probe><_calls>0</_calls></rect></menu>"#,
    );
    g.ui_tree.load_menu(&src, "menus\\probe.xml").unwrap();
    g.ui_tree.update();
    let callback = q(&mut g, "SetFunctionValue (CompileScript \"tileprobe.txt\")");
    assert!(matches!(callback, XVal::Ref(id) if id != 0), "{callback:?}");
    assert_eq!(q(&mut g, "SetFunctionValue (GetUDFFromFile \"TileProbe.txt\")"), callback);
    assert_eq!(q(&mut g, "SetFunctionValue (CompileScript \"missing.txt\")"), XVal::Ref(0));
    q(&mut g, "SetTileScript \"hudmainmenu/PROBE/_PROBE\" (CompileScript \"TileProbe.txt\")");
    g.tick();
    assert_eq!(g.ui_tree.get_float("HUDMainMenu/Probe/_calls"), Some(1.0));
    g.ui_tree.set_float("HUDMainMenu\\Probe\\_probe", 0.0);
    g.tick();
    assert_eq!(g.ui_tree.get_float("HUDMainMenu/Probe/_calls"), Some(1.0));
    g.ui_tree.set_float("HUDMainMenu/Probe/_probe", 1.0);
    g.tick();
    assert_eq!(g.ui_tree.get_float("HUDMainMenu/Probe/_calls"), Some(2.0));
}

fn s(v: &str) -> XVal {
    XVal::Str(v.into())
}

#[test]
fn knvse_clock_weights_text_keys_and_attributes() {
    files();
    let f = fixture();
    let mut g = game(&f);
    // The clock starts with the first frame [observed: knvse-anim, Ceil 1 soon after].
    assert_eq!(q(&mut g, "SetFunctionValue (Ceil (PlayerREF.GetGlobalAnimTime 0))"), n(1.0));
    q(&mut g, "PlayerREF.SetGlobalAnimTime 5");
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.GetGlobalAnimTime)"), n(5.0));
    g.tick();
    let after = q(&mut g, "SetFunctionValue (PlayerREF.GetGlobalAnimTime)");
    assert_eq!(after, n(5.0 + g.cfg.frame_seconds));
    // The current time is the offset plus the clock; a change shows from the next frame.
    q(&mut g, "PlayerREF.SetAnimCurrentTime \"test\\a.kf\" 1.5");
    assert_ne!(q(&mut g, "SetFunctionValue (PlayerREF.GetAnimCurrentTime \"test\\a.kf\")"), n(1.5));
    g.tick();
    let XVal::Num(now) = q(&mut g, "SetFunctionValue (PlayerREF.GetAnimCurrentTime \"test\\a.kf\")") else { panic!("a number") };
    assert!((now - (1.5 + g.cfg.frame_seconds)).abs() < 1e-9, "{now}");
    // Text keys: added for every actor, only for a file that exists.
    assert_eq!(q(&mut g, "SetFunctionValue (AddAnimTextKey \"test\\a.kf\" 0.5 \"Hit\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (AddAnimTextKey \"other\\none.kf\" 0.5 \"Hit\")"), n(0.0));
    assert_eq!(q(&mut g, "array_var a\nlet a := GetAnimationAttributes \"test\\a.kf\"\nSetFunctionValue (ar_Size a)"), n(13.0));
    assert_eq!(q(&mut g, "array_var a\nlet a := GetAnimationAttributes \"test\\a.kf\"\nlet a := a[\"textKeyValues\"]\nSetFunctionValue a[0]"), s("Hit"));
    assert_eq!(q(&mut g, "array_var a\nlet a := GetAnimationAttributes \"test\\a.kf\"\nSetFunctionValue a[\"sequenceName\"]"), s("a"));
    // Played on the actor: 22 keys, the weight set.
    q(&mut g, "PlayerREF.ActivateAnim \"test\\a.kf\"");
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.GetAnimCurrentTime \"test\\a.kf\")"), n(0.0));
    q(&mut g, "PlayerREF.SetAnimWeight \"test\\a.kf\" 0.25");
    assert_eq!(q(&mut g, "array_var a\nlet a := PlayerREF.GetAnimationAttributes \"test\\a.kf\"\nSetFunctionValue (ar_Size a)"), n(22.0));
    assert_eq!(q(&mut g, "array_var a\nlet a := PlayerREF.GetAnimationAttributes \"test\\a.kf\"\nSetFunctionValue a[\"seqWeight\"]"), n(0.25));
    // SetAnimTextKeys does nothing while the actor plays the sequence.
    q(&mut g, "PlayerREF.SetAnimTextKeys \"test\\a.kf\" (ar_List 0, 1) (ar_List \"S\", \"E\")");
    assert_eq!(q(&mut g, "array_var a\nlet a := PlayerREF.GetAnimationAttributes \"test\\a.kf\"\nlet a := a[\"textKeyValues\"]\nSetFunctionValue (ar_Size a)"), n(1.0));
    // CrossFadeAnims: the source must play and the destination not.
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.CrossFadeAnims \"test\\b.kf\" \"test\\a.kf\")"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.CrossFadeAnims \"test\\a.kf\" \"test\\b.kf\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.IsAnimSequencePlaying \"test\\a.kf\")"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.IsAnimSequencePlaying \"test\\b.kf\")"), n(1.0));
    // BlendToAnim: 1 for a destination that exists, the source playing or not; a playing
    // source stops at weight 0. A sequence the actor does not play has no attributes.
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.BlendToAnim \"test\\a.kf\" \"other\\c.kf\")"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.GetAnimationAttributes \"test\\c.kf\")"), XVal::Array(0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.BlendToAnim \"test\\b.kf\" \"test\\c.kf\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.IsAnimSequencePlaying \"test\\b.kf\")"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.BlendToAnimFromPose \"test\\d.kf\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.SetAnimAdditive \"test\\d.kf\" 0 \"test\\a.kf\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.SetAnimAdditive \"other\\d.kf\")"), n(0.0));
    // No body clip told: no sequence of a type plays, ar_Null.
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.GetPlayingAnimBySequenceType 0)"), XVal::Array(0));
    // Overrides copied between forms.
    q(&mut g, "SetWeaponAnimationPath TestGun 0 1 \"test\\gun.kf\"\nCopyAnimationsToForm TestGun TestLight");
    assert_eq!(g.anim_paths.weapon.get(&(FormId(LIGHT), false)).cloned(), Some(vec!["test\\gun.kf".to_string()]));
}

#[test]
fn jippp_runtime_nodes_effects_and_wear() {
    files();
    let f = fixture();
    let mut g = game(&f);
    // InsertNode / AttachModel calls fill the lists HasRuntimeNode reads.
    q(&mut g, "InsertNode TestBox 1 \"Weapon\\BarrelDevice|Second_Barrel\"\nAttachModel TestBox 1 \"Weapon|test\\scope.nif\"");
    assert_eq!(q(&mut g, "SetFunctionValue (HasRuntimeNode TestBox \"Weapon\\BarrelDevice|Second_Barrel\" 1)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (HasRuntimeNode TestBox \"Weapon\\BarrelDevice|Second_Barrel\" 2)"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (HasRuntimeNode TestBox \"Weapon|test\\scope.nif\" 2)"), n(1.0));
    // Mode 0 reads the target path only, not the new node's name [observed: jipppln].
    assert_eq!(q(&mut g, "SetFunctionValue (HasRuntimeNode TestBox \"BarrelDevice\" 0)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (HasRuntimeNode TestBox \"Second_Barrel\" 0)"), n(0.0));
    // A query belongs to no plugin: it copies and clears nothing [observed: jipppln].
    assert_eq!(q(&mut g, "SetFunctionValue (CopyRuntimeNodes TestGun TestBox 0)"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (ClearRuntimeNodes TestBox)"), n(0.0));
    // A plugin's script: copied (overwrite, then concatenate), attached as a whole form,
    // and only its own entries cleared.
    let (gun, box_, light) = (Some(FormId(WEAPON)), XVal::Ref(BOX), XVal::Ref(LIGHT));
    let as_mod = |g: &mut Game, name: &str, args: &[XVal]| fnv_game::nvse_jippp::call_as(g, "small.esp", name, None, args).unwrap().unwrap();
    assert_eq!(as_mod(&mut g, "CopyRuntimeNodes", &[XVal::Ref(WEAPON), box_.clone(), n(0.0)]), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (HasRuntimeNode TestGun \"BarrelDevice\")"), n(1.0));
    assert_eq!(as_mod(&mut g, "InsertNode", &[XVal::Ref(WEAPON), n(1.0), s("Bip01|Mine")]), n(0.0));
    assert_eq!(as_mod(&mut g, "AttachFormModel", &[light.clone(), box_.clone(), n(1.0), s("Root|*_X*")]), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (HasRuntimeNode TestLight \"Root|Test\\Box.nif\" 2)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (HasRuntimeNode TestLight \"Weapon\\BarrelDevice|Second_Barrel*_X*\" 1)"), n(1.0));
    // A remove succeeds again with nothing left to remove [observed: jipppln].
    assert_eq!(as_mod(&mut g, "AttachFormModel", &[light.clone(), box_.clone(), n(0.0), s("Root")]), n(1.0));
    assert_eq!(as_mod(&mut g, "AttachFormModel", &[light, box_, n(0.0), s("Root")]), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (HasRuntimeNode TestLight \"Root|Test\\Box.nif\" 2)"), n(0.0));
    // The copied entries are the query's (no plugin), so only the mod's own one goes.
    assert_eq!(fnv_game::nvse_jippp::call_as(&mut g, "small.esp", "ClearRuntimeNodes", gun, &[XVal::Ref(WEAPON)]).unwrap().unwrap(), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (HasRuntimeNode TestGun \"Bip01\")"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (HasRuntimeNode TestGun \"BarrelDevice\")"), n(1.0));
    // Wear per shot: the record's override 0.5, times the ammunition's 3.
    assert_eq!(q(&mut g, "SetFunctionValue (GetCalculatedWeaponDegradation TestGun TestAmmo)"), n(1.5));
    // Active effects as maps, filtered and written back.
    g.effects.push(ActiveEffect {
        id: 77,
        target: FormId::PLAYER_REF,
        source: FormId(SPELL),
        effect: FormId(EFFECT),
        index: 0,
        magnitude: 10,
        duration: 30.0,
        elapsed: 10.0,
        permanent: false,
        caster: None,
        script: None,
        change: None,
        usage: 0.0,
        worn: None,
        inactive: false,
    });
    let info = "array_var a\narray_var e\nlet a := PlayerREF.GetActiveEffectsInfo";
    assert_eq!(q(&mut g, &format!("{info}\nSetFunctionValue (ar_Size a)")), n(1.0));
    assert_eq!(q(&mut g, &format!("{info} 2\nlet e := a[0]\nSetFunctionValue e[\"TimeLeft\"]")), n(20.0));
    // None to list: no array [observed: jipppln].
    assert_eq!(q(&mut g, &format!("{info} 3\nSetFunctionValue (ar_Size a)")), n(-1.0));
    assert_eq!(q(&mut g, &format!("{info} 0 0\nSetFunctionValue (ar_Size a)")), n(-1.0));
    assert_eq!(q(&mut g, &format!("{info} TestEffect\nlet e := a[0]\nSetFunctionValue e[\"MetaData\"]")), n(77.0));
    q(&mut g, &format!("{info}\nlet e := a[0]\nlet e[\"Magnitude\"] := 20\nlet e[\"TimeLeft\"] := 99\nPlayerREF.SetActiveEffectsInfo a"));
    let e = g.effects.iter().find(|e| e.id == 77).unwrap();
    assert_eq!((e.magnitude, e.elapsed), (20, 0.0));
    // A control's key and mouse button; a pad's is unbound.
    assert_eq!(q(&mut g, "SetFunctionValue (GetControlCodeByID 7 0)"), q(&mut g, "SetFunctionValue (GetControl 7 0)"));
    assert_eq!(q(&mut g, "SetFunctionValue (GetControlCodeByID 7 2)"), n(-1.0));
    // Asked every frame while nothing takes the queue: in it once.
    for _ in 0..3 {
        q(&mut g, "TestBoxRef.ClampToGround");
    }
    assert_eq!(g.take_clamp_requests(), vec![FormId(BOX_REF)]);
    assert_eq!(g.take_clamp_requests(), Vec::<FormId>::new());
    // On a base form (a weapon is its own base) the call stops the script, in a
    // non-strict run too [observed: jipppln clamp.base, "script call failed"].
    let base = "Begin Function { }\nref r\nlet r := TestGun\nSetFunctionValue (r.ClampToGround)\nEnd\n";
    let mut loose = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig::default());
    for g in [&mut g, &mut loose] {
        assert_eq!(g.run_query(base, Default::default()).unwrap_err(), "clamptoground called on no reference");
    }
    assert_eq!(g.take_clamp_requests(), Vec::<FormId>::new());
    // Mode 2 removes [documented: InsertNode's addOrRemove table]: off the runtime list,
    // and nothing added to the list or the scene; a spec without `|` is not kept.
    q(&mut g, "InsertNode TestBox 2 \"Weapon\\BarrelDevice|Second_Barrel\"");
    assert_eq!(q(&mut g, "SetFunctionValue (HasRuntimeNode TestBox \"Weapon\\BarrelDevice|Second_Barrel\" 1)"), n(0.0));
    q(&mut g, "InsertNode TestBox 1 \"NoBar\"");
    assert_eq!(q(&mut g, "SetFunctionValue (HasRuntimeNode TestBox \"NoBar\" 1)"), n(0.0));
}

/// GetSessionTime adds up each frame's own length too: frames of changing length move it
/// on by exactly their sum, never back (#1297).
#[test]
fn session_time_adds_up_frames_of_changing_length() {
    files();
    let f = fixture();
    let mut g = game(&f);
    let secs = |g: &mut Game| match q(g, "SetFunctionValue (GetSessionTime)") {
        XVal::Num(x) => x,
        v => panic!("{v:?}"),
    };
    let start = secs(&mut g);
    let mut want = start;
    let mut last = start;
    for (i, dt) in [1.0 / 60.0, 1.0 / 30.0, 0.0166, 0.017, 0.1, 0.008].iter().cycle().take(600).enumerate() {
        g.cfg.frame_seconds = *dt;
        g.tick();
        want += dt;
        let now = secs(&mut g);
        assert!(now >= last, "frame {i}: {last} then {now}");
        assert!((now - want).abs() < 1e-9, "frame {i}: {now}, frames add up to {want}");
        last = now;
    }
    // A long session with a frame-time change: the clock does not jump with it.
    g.cfg.frame_seconds = 0.0166;
    for _ in 0..36_000 {
        g.tick();
    }
    let before = secs(&mut g);
    g.cfg.frame_seconds = 0.017;
    g.tick();
    assert!((secs(&mut g) - before - 0.017).abs() < 1e-6);
}

/// The CacheUI and vheyes clocks add up each frame's own length: the engine gives every
/// frame its own `frame_seconds`, so a change of it does not move the clock back or on.
#[test]
fn clocks_add_up_frames_of_changing_length() {
    files();
    let f = fixture();
    let mut g = game(&f);
    // 0.25 s of 1/60 frames, then the frame length doubles: the typewriter (1 s, no
    // fade) is half way at 0.5 s, not at 0.25 s + 15 × 2/60.
    g.cfg.frame_seconds = 1.0 / 60.0;
    for _ in 0..15 {
        g.tick();
    }
    q(&mut g, "SetUITypewriter \"TypeMenu\\Line\" \"abcd\" 1 0");
    g.cfg.frame_seconds = 2.0 / 60.0;
    for _ in 0..8 {
        g.tick();
    }
    // 8 × 2/60 = 0.267 s since the start: letters 0 and 1 (0 and 0.25 s) are in.
    assert_eq!(g.ui_tree.get_float("TypeMenu\\Line\\_TA1"), Some(255.0));
    assert_eq!(g.ui_tree.get_float("TypeMenu\\Line\\_TA2"), Some(0.0));
    // The moon's fade: 1 s, just past half way (alpha 1/30, the looks changed) after 31
    // frames of 1/60.
    q(&mut g, "SetMoonFade 1 0 1 0");
    g.cfg.frame_seconds = 1.0 / 60.0;
    for _ in 0..31 {
        g.tick();
    }
    let (look, _, alpha) = g.moon_look(1.0);
    assert!((alpha - 1.0 / 30.0).abs() < 1e-6, "{alpha}");
    assert!(look.doom);
    // A zero fade changes the looks at once.
    for _ in 0..40 {
        g.tick();
    }
    q(&mut g, "SetMoonFade 0 0 0 1");
    let (look, _, alpha) = g.moon_look(1.0);
    assert!(look.full && !look.doom && alpha == 1.0);
    // A reference queued for its 3D again and again is in the queue once.
    for _ in 0..3 {
        q(&mut g, "TestBoxRef.QueueRef3D");
    }
    assert_eq!(g.take_rebuild_3d(), vec![FormId(BOX_REF)]);
}

#[test]
fn vheyes_moon_stamps_and_queued_command() {
    files();
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(q(&mut g, "SetFunctionValue (GetMoonDoomState)"), n(0.0));
    q(&mut g, "SetMoonDoomSet 1");
    assert_eq!(q(&mut g, "SetFunctionValue (GetMoonDoomState)"), n(1.0));
    q(&mut g, "SetMoonDoom 1");
    assert_eq!(q(&mut g, "SetFunctionValue (GetMoonDoomState)"), n(2.0));
    // The fade changes the look at its midpoint; a second call meanwhile is ignored.
    q(&mut g, "SetMoonFade 0.1 0 0 1\nSetMoonFade 9 1 1 1");
    for _ in 0..4 {
        g.tick();
    }
    assert_eq!(q(&mut g, "SetFunctionValue (GetMoonDoomState)"), n(0.0));
    q(&mut g, "SetMoonGrowth 1 2 3 0");
    assert_eq!(g.moon_look(1.0).1, Some(2.0));
    assert_eq!(g.moon_look(5.0).1, Some(3.0));
    q(&mut g, "SetMoonScale 0.5");
    assert_eq!(g.moon_look(1.0).1, Some(0.5));
    // Stamps: first touch stamps and says 0, a changed model path says 1.
    assert_eq!(q(&mut g, "SetFunctionValue (TestBoxRef.QueueRefCloneHard)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (TestBoxRef.QueueRefClone)"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (TestBoxRef.QueueRefClone)"), n(0.0));
    q(&mut g, "SetModelPath \"Test\\Other.nif\" TestBox");
    assert_eq!(q(&mut g, "SetFunctionValue (TestBoxRef.QueueRefClone)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (TestBoxRef.DeferRefUpdate3D 1)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.DeferRefUpdate3D 1)"), n(0.0));
    g.tick();
    assert_eq!(g.take_rebuild_3d(), vec![FormId(BOX_REF)]);
    assert_eq!(q(&mut g, "SetFunctionValue (TestBoxRef.QueueRefClone)"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (ClearRefStamps)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (TestBoxRef.QueueRefCloneHard)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GetRefCloneQueueCount)"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (TestBoxRef.DetachRef3D)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.QueueRef3D)"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (TestBoxRef.GetStoredAmmoRounds)"), n(-1.0));
    // The queued console line runs two frames later, once.
    assert_eq!(q(&mut g, "SetFunctionValue (ArmQueuedConsoleCommand)"), n(1.0));
    g.tick();
    assert_eq!(q(&mut g, "SetFunctionValue (GetGlobalTimeMultiplier)"), n(1.0));
    g.tick();
    assert_eq!(q(&mut g, "SetFunctionValue (GetGlobalTimeMultiplier)"), n(2.0));
}

#[test]
fn interactive_console_evaluation_prints_compiled_results_before_cleanup() {
    let f = fixture();
    let mut g = game(&f);
    let eval = |g: &mut fnv_game::Game<'_>, expression| g.eval_console_expression(expression, Default::default()).unwrap();
    assert_eq!(eval(&mut g, "5 + 5"), "10");
    assert_eq!(eval(&mut g, "GetModelPath TestBox"), r#""Test\\Box.nif""#);
    assert_eq!(eval(&mut g, "ar_List 1, 9"), "[0: 1, 1: 9]");
    assert!(g.eval_console_expression("5 +", Default::default()).is_err());
}

#[test]
fn cacheui_anh_and_the_small_plugins() {
    files();
    let f = fixture();
    let mut g = game(&f);
    // Anh.
    assert_eq!(q(&mut g, "SetFunctionValue (ArrayToString (ar_List 1, 2))"), s("[1, 2]"));
    assert_eq!(q(&mut g, "SetFunctionValue (GetCameraFOV 1)"), n(55.0));
    q(&mut g, "SetCameraFOV 90 70");
    assert_eq!(q(&mut g, "SetFunctionValue (GetCameraFOV 0)"), n(90.0));
    // CacheUI.
    assert_eq!(q(&mut g, "SetFunctionValue (GetFPS)"), n(60.0));
    let refs = q(&mut g, "array_var a\nlet a := SplitStringToRef \"TestGun, TestAmmo,Small.esm:803,Nothing\" \",\"\nSetFunctionValue (ar_Size a)");
    assert_eq!(refs, n(3.0));
    assert_eq!(q(&mut g, "array_var a\nlet a := SplitStringToRef \"TestGun,Small.esm:803\" \",\"\nSetFunctionValue a[1]"), XVal::Ref(LIGHT));
    // The typewriter: letters at once, the alphas over the duration.
    q(&mut g, "SetUITypewriter \"TypeMenu\\Line\" \"abc\" 0.5 0.1");
    assert_eq!(g.ui_tree.get_string("TypeMenu\\Line\\_TS1"), Some("b".to_string()));
    assert_eq!(g.ui_tree.get_float("TypeMenu\\Line\\_TA2"), Some(0.0));
    for _ in 0..60 {
        g.tick();
    }
    assert_eq!(g.ui_tree.get_float("TypeMenu\\Line\\_TA2"), Some(255.0));
    q(&mut g, "SetUITypewriter \"TypeMenu\\Line\" \"abc\" 0");
    assert_eq!(g.ui_tree.get_float("TypeMenu\\Line\\_TA0"), Some(0.0));
    // The frame loop: every 0.05 s, two frames after.
    q(&mut g, "SetFrameControllerLoop ({} => SetUIFloat \"TypeMenu\\Line\\_flag\" ((GetUIFloat \"TypeMenu\\Line\\_flag\") + 1)) 1 0.05 2");
    for _ in 0..12 {
        g.tick();
    }
    let calls = g.ui_tree.get_float("TypeMenu\\Line\\_flag").unwrap();
    assert!((3.0..=4.0).contains(&calls), "{calls}");
    // The NX, Sandbox and plugin-less functions.
    assert_eq!(q(&mut g, "SetFunctionValue (NX_GetQVEVFL)"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GetButcherPeteVersion)"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GetQuestFailedAlt TestQuest)"), n(0.0));
    q(&mut g, "FailQuest TestQuest");
    assert_eq!(q(&mut g, "SetFunctionValue (GetQuestFailedAlt TestQuest)"), n(1.0));
    // Overcharge, yUI, hNVSE, Stewie's Tweaks, TTW.
    q(&mut g, "SetOCSettings 8 0.35\nPlayerREF.SetOCWeaponState TestGun 16 73");
    assert_eq!(q(&mut g, "SetFunctionValue (GetOCSettings 8)"), n(0.35));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.GetOCWeaponState TestGun 16)"), n(73.0));
    assert!(matches!(q(&mut g, "SetFunctionValue (CreateOCLightClone TestLight PlayerREF)"), XVal::Ref(r) if r != 0 && r != LIGHT));
    q(&mut g, "ySISetTrait \"tag\" \"(fba1)\" TestGun\nySISetTrait \"tag\" \"(fba1)\" TestAmmo\nySISetTrait \"icon\" \"Interface\\Icons\\X.dds\" TestGun");
    assert_eq!(q(&mut g, "SetFunctionValue (ySIGetTrait \"icon\" TestAmmo)"), s("Interface\\Icons\\X.dds"));
    assert_eq!(q(&mut g, "array_var a\nlet a := GetCellRegions TestOutside\nSetFunctionValue a[0]"), XVal::Ref(REGION));
    assert_eq!(q(&mut g, "array_var a\nlet a := GetCellRegions TestInside\nSetFunctionValue (ar_Size a)"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewieGetValue \"bJumpWhileAiming:Tweaks\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewieGetValue \"sSoundName:Cookable Grenades\")"), s("WPNGrenadeBeep"));
    assert_eq!(q(&mut g, "SetFunctionValue (TTW_CRC32 \"Small.esm\")"), n(f64::from(0xCBF4_3926u32)));
    assert_eq!(q(&mut g, "array_var a\narray_var r\nlet a := TTW_GetPlugins\nlet r := a[0]\nSetFunctionValue r[0]"), s("Small.esm"));
    // The plugin's header version (1.34 here): TTW calls a plugin below 1.3 an
    // unconverted Fallout 3 mod.
    assert_eq!(q(&mut g, "array_var a\narray_var r\nlet a := TTW_GetPlugins\nlet r := a[0]\nSetFunctionValue r[1]"), n(f64::from(1.34f32)));
    // TTW's radio fix: a switch the engine's radio reads.
    assert!(!g.small.ttw_radio_fix);
    q(&mut g, "TTW_EnableRadioFix 1");
    assert!(g.small.ttw_radio_fix);
    q(&mut g, "TTW_EnableRadioFix 0");
    assert!(!g.small.ttw_radio_fix);
    // TTW's gene projector is the race menu, NVForever's character creator.
    q(&mut g, "TTW_ShowGeneProjector");
    assert_eq!(g.open_menu_code(), Some(fnv_game::game::menus::RACE));
    assert!(g.race_menu_studio, "drawn in the creator's studio");
    g.close_menu();
    q(&mut g, "ShowRaceMenu");
    assert!(!g.race_menu_studio, "New Vegas's race menu stays in the room");
    g.close_menu();
    q(&mut g, "SetWorldspaceMapBounds TestWorld -10 20 30 -40");
    assert_eq!(g.world_map_bounds(FormId(WORLD)), Some([-10, 20, 30, -40]));
}

#[test]
fn a_lambda_writes_its_makers_variables() {
    // [documented: Lambda, its variables are its maker's; the High and Dry session's repro,
    // #2275] Called in the run that made it, then again from the run's variables.
    let f = fixture();
    let mut g = game(&f);
    let src = "int c\nref f\nint x\nBegin Function { }\nlet c := 1\nlet f := ({x} => c += x)\nCall f 10\nCall f 5\nSetFunctionValue c\nEnd\n";
    assert_eq!(g.run_query(src, Default::default()).unwrap(), n(16.0));
    let src = "int c\nref f\nint x\nBegin Function { }\nlet f := ({x} => x * 2)\nlet c := Call f 10\nSetFunctionValue c\nEnd\n";
    assert_eq!(g.run_query(src, Default::default()).unwrap(), n(20.0));
}

#[test]
fn format_k_prints_the_keys_name() {
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(q(&mut g, "SetFunctionValue (sv_Construct \"%k\" 18)"), s("E"));
}

#[test]
fn cached_ini_file_quotes_are_removed_without_changing_default_or_cached_values() {
    let f = fixture();
    let mut g = game(&f);
    q(&mut g, "SetINIString \"Template:Message\" (Sv_Construct \"%qAir %%.0f%q\") \"OwnedQuotes.ini\"");
    assert_eq!(q(&mut g, "SetFunctionValue (GetINIString_Cached \"Template:Message\" \"OwnedQuotes.ini\")"), XVal::Str("Air %.0f".into()));
    assert_eq!(q(&mut g, "SetFunctionValue (GetINIStringOrDefault_Cached \"Template:Missing\" \"OwnedQuotes.ini\" \"%qFallback%q\")"), XVal::Str("\"Fallback\"".into()));
    q(&mut g, "SetINIString \"Quotes:Empty\" \"%q%q\" \"OwnedEmptyQuotes.ini\"");
    q(&mut g, "SetINIString \"Quotes:Unmatched\" \"%qOpen\" \"OwnedEmptyQuotes.ini\"");
    q(&mut g, "SetINIString \"Quotes:Internal\" \"%qA %qB%q C%q\" \"OwnedEmptyQuotes.ini\"");
    assert_eq!(q(&mut g, "SetFunctionValue (GetINIString_Cached \"Quotes:Empty\" \"OwnedEmptyQuotes.ini\")"), XVal::Str(String::new()));
    assert_eq!(q(&mut g, "SetFunctionValue (GetINIString_Cached \"Quotes:Unmatched\" \"OwnedEmptyQuotes.ini\")"), XVal::Str("\"Open".into()));
    assert_eq!(q(&mut g, "SetFunctionValue (GetINIString_Cached \"Quotes:Internal\" \"OwnedEmptyQuotes.ini\")"), XVal::Str("A \"B\" C".into()));
    assert_eq!(q(&mut g, "SetFunctionValue (GetINIStringOrCreate_Cached \"Template:Created\" \"OwnedQuotes.ini\" \"%qCached%q\")"), XVal::Str("\"Cached\"".into()));
    assert_eq!(q(&mut g, "SetFunctionValue (GetINIString_Cached \"Template:Created\" \"OwnedQuotes.ini\")"), XVal::Str("\"Cached\"".into()));
    q(&mut g, "SetINIString \"Template:Warm\" \"%qWarm%q\" \"OwnedWarmQuotes.ini\"");
    assert_eq!(q(&mut g, "SetFunctionValue (HasINISetting_Cached \"Template:Warm\" \"OwnedWarmQuotes.ini\")"), XVal::Num(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (GetINIString_Cached \"Template:Warm\" \"OwnedWarmQuotes.ini\")"), XVal::Str("Warm".into()));
    q(&mut g, "SetINIString \"Template:Warm\" \"%qReloaded%q\" \"OwnedWarmQuotes.ini\"");
    q(&mut g, "ReloadIniCache \"OwnedWarmQuotes.ini\"");
    assert_eq!(q(&mut g, "SetFunctionValue (GetINIString_Cached \"Template:Warm\" \"OwnedWarmQuotes.ini\")"), XVal::Str("Reloaded".into()));
}

#[test]
fn ini_sibling_paths_read_owned_files_and_share_session_cache_values() {
    files();
    let f = fixture();
    let mut g = game(&f);
    assert_eq!(q(&mut g, r#"SetFunctionValue (GetINIFloat "Main:Value" "../NVSE/Plugins/OwnedSibling.ini")"#), n(17.0));
    q(&mut g, r#"SetINIFloat "Main:Value" 23 "../NVSE/Plugins/OwnedSibling.ini""#);
    assert_eq!(q(&mut g, r#"SetFunctionValue (GetINIFloat_Cached "Main:Value" "../NVSE/Plugins/Group/../OwnedSibling.ini" -7)"#), n(23.0));
    assert_eq!(q(&mut g, r#"SetFunctionValue (GetINIFloat "Main:Value" "Group/../OwnedLocal.ini")"#), n(41.0));
    assert_eq!(q(&mut g, r#"SetFunctionValue (GetINIFloat "Main:Value" "/OwnedLocal.ini")"#), n(0.0), "a rooted name must not alias a valid Config file");
    q(&mut g, r#"SetINIFloat "Main:Value" 99 "../../OwnedEscape.ini""#);
    assert_eq!(q(&mut g, r#"SetFunctionValue (GetINIFloat "Main:Value" "../../OwnedEscape.ini")"#), n(0.0));
    let original = std::env::temp_dir().join(format!("nvf-small-libs-{}", std::process::id())).join("NVSE/Plugins/OwnedSibling.ini");
    assert_eq!(std::fs::read_to_string(original).unwrap(), "[Main]\r\nValue=17\r\n", "without a write root, the session write leaves the owned original unchanged");
}

#[test]
fn objective_links_are_nested_paths_while_alt_is_a_flat_marker_list() {
    use fnv_game::quest::ObjectiveState;
    use fnv_sim::refs::Teleport;
    let mut f = fixture();
    // Typed, owned graph metadata uses only baseline public structures, so this
    // identical fixture also compiles before the new cache field exists.
    let template = f.refs.get(FormId(BOX_REF)).unwrap().clone();
    for (id, cell, destination) in [(0x900, INSIDE, 0x902), (0x902, 0xFFFF, 0x900), (0x901, 0xFFFF, 0x903), (0x903, OUTSIDE, 0x901)] {
        let mut door = template.clone();
        door.id = FormId(id);
        door.edid = None;
        door.cell = Some(FormId(cell));
        door.stored_in = Some(FormId(cell));
        door.teleport = Some(Teleport { door: FormId(destination), pos: [0.0; 3], rot: [0.0; 3], no_alarm: false });
        f.refs.refs.insert(door.id, door);
        f.refs.by_cell.entry(FormId(cell)).or_default().push(FormId(id));
    }
    let mut g = game(&f);
    let query = |g: &mut Game, source: &str| g.run_query(source, Default::default()).unwrap_or_else(|e| panic!("{source}: {e}"));
    assert_eq!(q(&mut g, "SetFunctionValue (Ar_Size (GetCurrentQuestObjectiveTeleportLinks))"), n(0.0));
    g.ui.active_quest = Some(FormId(QUEST));
    g.quests.entry(FormId(QUEST)).or_default().objectives.insert(10, ObjectiveState { displayed:true, completed:false });
    g.other.jip10.objective_targets.insert((FormId(QUEST),10), vec![FormId(BOX_REF)]);
    g.world.get(FormId::PLAYER_REF).unwrap().cell = Some(FormId(INSIDE));
    assert_eq!(g.check_quest_targets(FormId(QUEST)).len(), 1);
    assert_eq!(g.cell_of(FormId(BOX_REF)), Some(FormId(INSIDE)));
    assert_eq!(query(&mut g, "array_var paths\nBegin Function {}\nlet paths := GetCurrentQuestObjectiveTeleportLinks\nSetFunctionValue paths[0][0]\nEnd"), XVal::Ref(BOX_REF));
    assert_eq!(query(&mut g, "array_var markers\nBegin Function {}\nlet markers := GetCurrentQuestObjectiveTeleportLinksAlt\nSetFunctionValue markers[0]\nEnd"), XVal::Ref(BOX_REF));
    // Two rooms away: the first marker is the local door, and the nested row
    // keeps both doors followed by the actual objective reference.
    g.world.get(FormId(BOX_REF)).unwrap().cell = Some(FormId(OUTSIDE));
    assert_eq!(query(&mut g, "array_var paths\nBegin Function {}\nlet paths := GetCurrentQuestObjectiveTeleportLinks\nSetFunctionValue (Ar_Size paths[0])\nEnd"), n(3.0));
    assert_eq!(query(&mut g, "array_var paths\nBegin Function {}\nlet paths := GetCurrentQuestObjectiveTeleportLinks\nSetFunctionValue paths[0][2]\nEnd"), XVal::Ref(BOX_REF));
    assert_eq!(query(&mut g, "array_var markers\nBegin Function {}\nlet markers := GetCurrentQuestObjectiveTeleportLinksAlt\nSetFunctionValue markers[0]\nEnd"), XVal::Ref(0x900));
    g.quests.get_mut(&FormId(QUEST)).unwrap().objectives.get_mut(&10).unwrap().completed = true;
    assert_eq!(q(&mut g, "SetFunctionValue (Ar_Size (GetCurrentQuestObjectiveTeleportLinks))"), n(0.0));
}

#[test]
fn explicit_player_sequences_keep_view_clocks_targets_and_release_state() {
    files();
    let f = fixture();
    let mut g = game(&f);
    g.cfg.frame_seconds = 0.1;
    g.senses.first_person = true;
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.ActivateAnim \"test\\a.kf\" 1 3 1 0.4 0.3)"),n(1.0));
    let key = (FormId::PLAYER_REF,true,"test\\a.kf".into());
    let requested = g.anim_paths.sequences.get(&key).unwrap();
    assert_eq!(requested.priority,3);
    assert_eq!(requested.weight,0.4);
    assert_eq!(requested.ease_in,Some(0.3));
    assert!(!requested.auto_finish);
    g.tick();
    let first_time = g.script_sequence_time(FormId::PLAYER_REF,"test\\a.kf");
    assert!(first_time>0.09 && first_time<0.11);
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.ActivateAnim \"test\\a.kf\" 1)"),n(0.0));
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.ActivateAnim \"test\\a.kf\" 0 0 1 0.8)"),n(1.0));
    assert_eq!(g.script_sequence_time(FormId::PLAYER_REF,"test\\a.kf"),first_time,"starting the other view must not restart this one");
    assert_eq!(q(&mut g,"SetFunctionValue (Ar_Size (PlayerREF.GetActiveAnims 1))"),n(1.0));
    assert_eq!(q(&mut g,"SetFunctionValue (Ar_Size (PlayerREF.GetActiveAnims 0))"),n(1.0));
    q(&mut g,"PlayerREF.SetAnimCurrentTime \"test\\a.kf\" 0.6");
    assert_eq!(g.script_sequence_time(FormId::PLAYER_REF,"test\\a.kf"),first_time);
    g.tick();
    // The deferred seek is applied before this tick advances the actor clock.
    assert!((g.script_sequence_time(FormId::PLAYER_REF,"test\\a.kf")-0.7).abs()<1e-8);
    q(&mut g,"PlayerREF.SetAnimWeight \"test\\a.kf\" 0.25");
    assert_eq!(g.anim_paths.sequences[&key].weight,0.25);
    assert_eq!(g.anim_paths.sequences[&(FormId::PLAYER_REF,false,"test\\a.kf".into())].weight,0.8);
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.DeactivateAnim \"test\\a.kf\" 0.4)"),n(1.0));
    assert!(g.anim_paths.sequences[&key].stopped.is_some(),"the renderer retains the fade request");
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.IsAnimSequencePlaying \"test\\a.kf\")"),n(0.0));
    for _ in 0..6 { g.tick(); }
    assert!(!g.anim_paths.sequences.contains_key(&key));
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.ActivateAnim \"test\\a.kf\" 1 1 0)"),n(1.0));
    assert!((g.script_sequence_time(FormId::PLAYER_REF,"test\\a.kf")-0.7).abs()<1e-8,"resuming uses the paused time, not time spent inactive");
}

#[test]
fn transition_helpers_preserve_explicit_weight_and_offset_readbacks() {
    files();
    let f = fixture();
    let mut g = game(&f);
    g.senses.first_person = false;
    q(&mut g,"PlayerREF.ActivateAnim \"test\\a.kf\" 0");
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.CrossFadeAnims \"test\\a.kf\" \"test\\b.kf\" 0 0.3 1 1 0.4)"),n(1.0));
    assert_eq!(q(&mut g,"array_var a\nlet a := PlayerREF.GetAnimationAttributes \"test\\b.kf\"\nSetFunctionValue a[\"seqWeight\"]"),n(f64::from(0.4f32)));
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.BlendToAnim \"test\\b.kf\" \"test\\c.kf\" 0 0.3 0.75 0 0.6)"),n(1.0));
    assert_eq!(q(&mut g,"array_var a\nlet a := PlayerREF.GetAnimationAttributes \"test\\c.kf\"\nSetFunctionValue a[\"seqWeight\"]"),n(f64::from(0.6f32)));
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.GetAnimCurrentTime \"test\\c.kf\")"),n(0.75));
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.BlendToAnimFromPose \"test\\d.kf\" 0 0.3 0.5)"),n(1.0));
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.GetAnimCurrentTime \"test\\d.kf\")"),n(0.5));
}

#[test]
fn play_animation_path_preserves_loop_and_reverse_until_deactivated() {
    use fnv_game::nvse_knvse::SequenceInfo;
    files();
    let f = fixture();
    for cycle in [0, 1] {
        let mut g = game(&f);
        g.cfg.frame_seconds = 0.1;
        g.set_sequence_info("test\\b.kf", SequenceInfo {
            name:"Owned cycle".into(), accum_root:String::new(), frequency:2.0,
            begin:0.0, end:0.25, cycle, text_keys:vec![],
        });
        assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.PlayAnimationPath \"test\\b.kf\" 1)"),n(1.0));
        for _ in 0..20 { g.tick(); }
        assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.IsAnimSequencePlaying \"test\\b.kf\")"),n(1.0),"cycle {cycle} must survive several full periods");
        assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.DeactivateAnim \"test\\b.kf\" 0)"),n(1.0));
        g.tick();
        assert!(g.anim_paths.sequences.is_empty());
    }
}

#[test]
fn play_animation_path_stops_at_its_reported_end() {
    use fnv_game::nvse_knvse::SequenceInfo;
    files();
    let f = fixture();
    let mut g = game(&f);
    g.cfg.frame_seconds = 0.1;
    g.set_sequence_info("test\\b.kf",SequenceInfo {name:"Owned".into(),accum_root:String::new(),frequency:1.0,begin:0.0,end:0.25,cycle:2,text_keys:vec![]});
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.PlayAnimationPath \"test\\b.kf\" 1)"),n(1.0));
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.IsAnimSequencePlaying \"test\\b.kf\")"),n(1.0));
    for _ in 0..7 { g.tick(); }
    assert_eq!(q(&mut g,"SetFunctionValue (PlayerREF.IsAnimSequencePlaying \"test\\b.kf\")"),n(0.0));
    assert!(g.anim_paths.sequences.is_empty());
}

#[test]
fn stewie_inventory_queries_keep_unbound_fallback_and_follow_current_choices_after_handoff() {
    files();
    let f = fixture();
    let mut g = game(&f);
    let ini = b"[Tweaks]\nbHardcoreTweaks=1\nbIndividialItemStats=1\nbWeightlessItems=1\nbUnmapped=7\nsUnmapped=\"retained\"\n[Hardcore Tweaks]\nbAmmoWeight=1\n[Other]\nbAmmoWeight=9\n";
    assert!(g.mod_files.write_bytes("NVSE/Plugins/nvse_stewie_tweaks.ini", ini));
    let keys = ["bHardcoreTweaks:Tweaks", "bAmmoWeight:Hardcore Tweaks", "bIndividialItemStats:Tweaks"];
    for key in keys {
        assert_eq!(q(&mut g, &format!("SetFunctionValue (StewVal \"{key}\")")), n(1.0), "unbound {key} retains file fallback");
    }
    // The default retains an enabled ammo child under its disabled parent.
    // This control instead explicitly selects all three choices off.
    let chosen = fnv_game::tweaks::Tweaks {
        hardcore_tweaks: false,
        hardcore_ammo_weight: false,
        single_item_figures: false,
        ..Default::default()
    };
    g.bind_stewie_stat_settings(&chosen);
    let version = q(&mut g, "SetFunctionValue (GetPluginVersion \"lStewieAl's Tweaks\")");
    for name in ["StewVal", "StewieGetValue"] {
        for key in keys {
            assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"{key}\")")), n(0.0), "loaded explicit off for {name} {key}");
        }
        for (key, expected) in [("bUnmapped:Tweaks", n(7.0)), ("sUnmapped:Tweaks", s("retained")), ("bWeightlessItems:Tweaks", n(1.0)), ("bAmmoWeight:Other", n(9.0)), ("malformed", n(0.0))] {
            assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"{key}\")")), expected, "unchanged fallback {name} {key}");
        }
    }
    g.tweaks.hardcore_tweaks = true;
    g.tweaks.hardcore_ammo_weight = true;
    g.tweaks.single_item_figures = true;
    let before = g.tweaks.clone();
    // Surrounding whitespace is NOT trimmed [observed: w4h_08_stewie_get_value_whitespace];
    // ASCII case-insensitive matching stays (see docs/spec-notes/ext-small.md).
    assert_eq!(q(&mut g, "SetFunctionValue (StewVal \"bHARDCOREtweaks:tWEAKs\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewieGetValue \"bAMMOweight:hardcore tweaks\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewVal \"bindividialitemstats:tweaks\")"), n(1.0));
    for padded in [
        " bHardcoreTweaks:Tweaks",
        "bHardcoreTweaks:Tweaks ",
        "bHardcoreTweaks : Tweaks",
        " bHARDCOREtweaks : tWEAKs ",
        " bAMMOweight : hardcore tweaks ",
        " bUnmapped:Tweaks",
        "bUnmapped:Tweaks ",
        "bUnmapped : Tweaks",
    ] {
        assert_eq!(q(&mut g, &format!("SetFunctionValue (StewVal \"{padded}\")")), n(0.0), "whitespace not trimmed: {padded:?}");
    }
    assert_eq!(g.tweaks, before, "queries do not mutate any settings");
    assert_eq!(q(&mut g, "SetFunctionValue (GetPluginVersion \"lStewieAl's Tweaks\")"), version);
    let saved = g.snapshot();
    let mut restored = Game::restore(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() }, &saved).unwrap();
    assert!(restored.mod_files.write_bytes("NVSE/Plugins/nvse_stewie_tweaks.ini", ini));
    assert_eq!(q(&mut restored, "SetFunctionValue (StewVal \"bHardcoreTweaks:Tweaks\")"), n(1.0), "saved game does not retain application authority");
    restored.bind_stewie_stat_settings(&chosen);
    for key in keys {
        assert_eq!(q(&mut restored, &format!("SetFunctionValue (StewVal \"{key}\")")), n(0.0), "application choices rebound after restore {key}");
    }
}

#[test]
fn stewie_ammo_child_choice_and_actual_inventory_totals_follow_the_parent_separately() {
    let mut f = fixture();
    let fnv_sim::forms::BaseData::Item(ammo) = &mut f.forms.bases.get_mut(&FormId(AMMO)).unwrap().data else { panic!("fixture ammunition is an item") };
    ammo.weight = Some(0.5);
    let mut g = game(&f);
    q(&mut g, "PlayerREF.AddItem TestAmmo 3");
    for parent in [false, true] {
        for child in [false, true] {
            let settings = fnv_game::tweaks::Tweaks { hardcore_tweaks: parent, hardcore_ammo_weight: child, ..Default::default() };
            g.bind_stewie_stat_settings(&settings);
            assert_eq!(q(&mut g, "SetFunctionValue (StewVal \"bHardcoreTweaks:Tweaks\")"), n(f64::from(u8::from(parent))));
            assert_eq!(q(&mut g, "SetFunctionValue (StewieGetValue \"bAmmoWeight:Hardcore Tweaks\")"), n(f64::from(u8::from(child))), "child remains selected while parent is off");
            g.sync_perk_weights();
            let rules = g.world.weight_rules(FormId::PLAYER_REF);
            for hardcore in [false, true] {
                let total = g.world.inventory_mut(FormId::PLAYER_REF).unwrap().totals_with(&f.forms, hardcore, &rules);
                let expected = if if parent { child } else { hardcore } { 1.5 } else { 0.0 };
                assert_eq!(total.weight, expected, "parent={parent} child={child} mode={hardcore}");
                assert!(total.unknown_weight.is_empty(), "weighted fixture must be admitted");
            }
        }
    }
}

#[test]
fn stewie_pipboy_queries_keep_unbound_fallback_and_follow_current_choices_after_handoff() {
    files();
    let f = fixture();
    let mut g = game(&f);
    let ini = b"[Tweaks]\nbDisableStealthEffectInPipboy=1\nbKeepPipboyLightOnCellChange=0\n[Other]\nbDisableStealthEffectInPipboy=7\n";
    assert!(g.mod_files.write_bytes("NVSE/Plugins/nvse_stewie_tweaks.ini", ini));
    // Unbound headless games retain the layered INI fallback.
    assert_eq!(q(&mut g, "SetFunctionValue (StewVal \"bDisableStealthEffectInPipboy:Tweaks\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewieGetValue \"bKeepPipboyLightOnCellChange:Tweaks\")"), n(0.0));
    let chosen = fnv_game::tweaks::Tweaks {
        no_stealth_shimmer_in_pipboy: false,
        ..Default::default()
    };
    g.bind_stewie_stat_settings(&chosen);
    let version = q(&mut g, "SetFunctionValue (GetPluginVersion \"lStewieAl's Tweaks\")");
    assert_eq!(version, n(960.0));
    for name in ["StewVal", "StewieGetValue"] {
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bDisableStealthEffectInPipboy:Tweaks\")")), n(0.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bKeepPipboyLightOnCellChange:Tweaks\")")), n(1.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \" bKeepPipboyLightOnCellChange : Tweaks \")")), n(0.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bDisableStealthEffectInPipboy:Other\")")), n(7.0));
    }
    g.tweaks.no_stealth_shimmer_in_pipboy = true;
    let before = g.tweaks.clone();
    assert_eq!(q(&mut g, "SetFunctionValue (StewVal \"bdisablestealtheffectinpipboy:tweaks\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewieGetValue \"bkeeppipboylightoncellchange:tweaks\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewVal \" bdisablestealtheffectinpipboy : tweaks \")"), n(0.0));
    assert_eq!(g.tweaks, before, "queries do not mutate settings");
    // Restore clears application authority until rebound.
    let saved = g.snapshot();
    let mut restored = Game::restore(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() }, &saved).unwrap();
    assert!(restored.mod_files.write_bytes("NVSE/Plugins/nvse_stewie_tweaks.ini", ini));
    assert_eq!(q(&mut restored, "SetFunctionValue (StewVal \"bDisableStealthEffectInPipboy:Tweaks\")"), n(1.0));
    assert_eq!(q(&mut restored, "SetFunctionValue (StewieGetValue \"bKeepPipboyLightOnCellChange:Tweaks\")"), n(0.0));
    restored.bind_stewie_stat_settings(&chosen);
    assert_eq!(q(&mut restored, "SetFunctionValue (StewVal \"bDisableStealthEffectInPipboy:Tweaks\")"), n(0.0));
    assert_eq!(q(&mut restored, "SetFunctionValue (StewieGetValue \"bKeepPipboyLightOnCellChange:Tweaks\")"), n(1.0));
}

#[test]
fn stewie_worn_armor_weightless_queries_follow_handoff_and_keep_item_mask_fallback() {
    files();
    let f = fixture();
    let mut g = game(&f);
    let ini = b"[Tweaks]\nbWeightlessWornArmor=1\nbWeightlessWornPowerArmor=1\nbWeightlessItems=1\n[Other]\nbWeightlessWornArmor=5\n";
    assert!(g.mod_files.write_bytes("NVSE/Plugins/nvse_stewie_tweaks.ini", ini));

    // Unbound headless games retain the layered INI fallback.
    assert_eq!(q(&mut g, "SetFunctionValue (StewVal \"bWeightlessWornArmor:Tweaks\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewieGetValue \"bWeightlessWornPowerArmor:Tweaks\")"), n(1.0));

    // After handoff, both worn-armor switches read NVForever choices while bWeightlessItems stays on INI fallback.
    let chosen = fnv_game::tweaks::Tweaks {
        weightless_worn_armor: false,
        weightless_worn_power_armor: true,
        ..Default::default()
    };
    g.bind_stewie_stat_settings(&chosen);
    assert_eq!(q(&mut g, "SetFunctionValue (GetPluginVersion \"lStewieAl's Tweaks\")"), n(960.0));
    for name in ["StewVal", "StewieGetValue"] {
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bWeightlessWornArmor:Tweaks\")")), n(0.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bWeightlessWornPowerArmor:Tweaks\")")), n(1.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \" bWeightlessWornPowerArmor : Tweaks \")")), n(0.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bWeightlessItems:Tweaks\")")), n(1.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bWeightlessWornArmor:Other\")")), n(5.0));
    }

    g.tweaks.weightless_worn_armor = true;
    g.tweaks.weightless_worn_power_armor = false;
    let before = g.tweaks.clone();
    assert_eq!(q(&mut g, "SetFunctionValue (StewVal \"bweightlesswornarmor:tweaks\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewieGetValue \"bweightlesswornpowerarmor:tweaks\")"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewVal \" bweightlesswornarmor : tweaks \")"), n(0.0));
    assert_eq!(g.tweaks, before, "queries do not mutate settings");

    // Snapshot/restore clears application authority until rebound.
    let saved = g.snapshot();
    let mut restored = Game::restore(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() }, &saved).unwrap();
    assert!(restored.mod_files.write_bytes("NVSE/Plugins/nvse_stewie_tweaks.ini", ini));
    assert_eq!(q(&mut restored, "SetFunctionValue (StewVal \"bWeightlessWornArmor:Tweaks\")"), n(1.0));
    assert_eq!(q(&mut restored, "SetFunctionValue (StewieGetValue \"bWeightlessWornPowerArmor:Tweaks\")"), n(1.0));
    restored.bind_stewie_stat_settings(&chosen);
    assert_eq!(q(&mut restored, "SetFunctionValue (StewVal \"bWeightlessWornArmor:Tweaks\")"), n(0.0));
    assert_eq!(q(&mut restored, "SetFunctionValue (StewieGetValue \"bWeightlessWornPowerArmor:Tweaks\")"), n(1.0));
}

#[test]
fn stewie_965_970_queries_follow_handoff_and_retain_sync_category_child_choice() {
    files();
    let f = fixture();
    let mut g = game(&f);
    let ini = b"[Tweaks]\n\
bSynchronizeContainerCategories=1\n\
bBarterCheckActorBuySellFlags=1\n\
bDontOpenDoorsAfterLockpick=1\n\
bNoSkipDialog=1\n\
bRestoreLockedDoorSound=0\n\
[Synchronized Container Categories]\n\
bPreventSwitchWhenEmptyingCategory=0\n\
[Other]\n\
bPreventSwitchWhenEmptyingCategory=9\n";
    assert!(g.mod_files.write_bytes("NVSE/Plugins/nvse_stewie_tweaks.ini", ini));

    // Unbound headless games retain the layered INI fallback.
    assert_eq!(q(&mut g, "SetFunctionValue (StewVal \"bSynchronizeContainerCategories:Tweaks\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewieGetValue \"bPreventSwitchWhenEmptyingCategory:Synchronized Container Categories\")"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewVal \"bBarterCheckActorBuySellFlags:Tweaks\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewieGetValue \"bRestoreLockedDoorSound:Tweaks\")"), n(0.0));

    // Default bound settings: parent switches off, child bPreventSwitchWhenEmptyingCategory retained true,
    // bRestoreLockedDoorSound always true (NVForever already plays DRSLocked on refused locks).
    let defaults = fnv_game::tweaks::Tweaks::default();
    g.bind_stewie_stat_settings(&defaults);
    assert_eq!(q(&mut g, "SetFunctionValue (GetPluginVersion \"lStewieAl's Tweaks\")"), n(960.0));
    for name in ["StewVal", "StewieGetValue"] {
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bSynchronizeContainerCategories:Tweaks\")")), n(0.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bPreventSwitchWhenEmptyingCategory:Synchronized Container Categories\")")), n(1.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \" bPreventSwitchWhenEmptyingCategory : Synchronized Container Categories \")")), n(0.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bBarterCheckActorBuySellFlags:Tweaks\")")), n(0.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bDontOpenDoorsAfterLockpick:Tweaks\")")), n(0.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bNoSkipDialog:Tweaks\")")), n(0.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bRestoreLockedDoorSound:Tweaks\")")), n(1.0));
        assert_eq!(q(&mut g, &format!("SetFunctionValue ({name} \"bPreventSwitchWhenEmptyingCategory:Other\")")), n(9.0));
    }

    // Live mutations reflect immediately without mutating unrelated fields.
    g.tweaks.sync_categories = true;
    g.tweaks.prevent_switch_when_emptying_category = false;
    g.tweaks.vendor_lists_both = true;
    g.tweaks.no_door_after_pick = true;
    g.tweaks.no_skip_dialogue = true;
    let before = g.tweaks.clone();
    assert_eq!(q(&mut g, "SetFunctionValue (StewVal \"bSynchronizeContainerCategories:Tweaks\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewieGetValue \"bPreventSwitchWhenEmptyingCategory:Synchronized Container Categories\")"), n(0.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewVal \"bBarterCheckActorBuySellFlags:Tweaks\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewVal \"bDontOpenDoorsAfterLockpick:Tweaks\")"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (StewieGetValue \"bNoSkipDialog:Tweaks\")"), n(1.0));
    assert_eq!(g.tweaks, before, "queries do not mutate settings");

    // Snapshot/restore clears application authority until rebound.
    let saved = g.snapshot();
    let mut restored = Game::restore(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() }, &saved).unwrap();
    assert!(restored.mod_files.write_bytes("NVSE/Plugins/nvse_stewie_tweaks.ini", ini));
    assert_eq!(q(&mut restored, "SetFunctionValue (StewVal \"bRestoreLockedDoorSound:Tweaks\")"), n(0.0));
    restored.bind_stewie_stat_settings(&defaults);
    assert_eq!(q(&mut restored, "SetFunctionValue (StewVal \"bRestoreLockedDoorSound:Tweaks\")"), n(1.0));
    assert_eq!(q(&mut restored, "SetFunctionValue (StewieGetValue \"bPreventSwitchWhenEmptyingCategory:Synchronized Container Categories\")"), n(1.0));
}

#[test]
fn add_item_and_add_item_health_percent_request_refresh_only_while_item_menu_open() {
    let f = fixture();
    let mut g = game(&f);

    // Closed menus: AddItem and AddItemHealthPercent leave refresh_items_list_pending false.
    q(&mut g, "PlayerREF.AddItem TestAmmo 2");
    assert!(!g.refresh_items_list_pending());
    q(&mut g, "PlayerREF.AddItemHealthPercent TestAmmo 1 0.5");
    assert!(!g.refresh_items_list_pending());

    // Open Inventory, Container, or Barter menu: both functions request a one-shot refresh.
    for code in [fnv_game::game::menus::INVENTORY, fnv_game::game::menus::CONTAINER, fnv_game::game::menus::BARTER] {
        g.open_menu(code);
        q(&mut g, "PlayerREF.AddItem TestAmmo 1");
        assert!(g.take_refresh_items_list(), "AddItem refreshes open menu {code}");
        assert!(!g.refresh_items_list_pending());

        q(&mut g, "PlayerREF.AddItemHealthPercent TestAmmo 1 0.75");
        assert!(g.take_refresh_items_list(), "AddItemHealthPercent refreshes open menu {code}");
        assert!(!g.refresh_items_list_pending());
        g.close_menu();
    }
}

#[test]
fn hnvse_merge_region_lists_unions_overridden_cell_regions_when_enabled() {
    files();
    fn tes4_with_masters(masters: &[&str]) -> Vec<u8> {
        let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat();
        let mut subs = vec![sub(b"HEDR", hedr)];
        for m in masters {
            subs.push(sub(b"MAST", z(m)));
            subs.push(sub(b"DATA", 0u64.to_le_bytes().to_vec()));
        }
        record(b"TES4", 0, 0, &subs)
    }
    fn u32s(v: &[u32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }
    fn map_regn(id: u32, edid: &str, map_name: &str) -> Vec<u8> {
        // RDAT: type 4 (Map), flags 0, priority 50, pad.
        let rdat = [4u32.to_le_bytes().as_slice(), &[0, 50, 0, 0]].concat();
        record(b"REGN", 0, id, &[sub(b"EDID", z(edid)), sub(b"RDAT", rdat), sub(b"RDMP", z(map_name))])
    }

    const EXT_WORLD: u32 = 0x0000_0810;
    const EXT_CELL: u32 = 0x0000_0820;
    const INT_CELL: u32 = 0x0000_0821;
    const REG_MASTER_MAP: u32 = 0x0000_0830;
    const REG_MASTER_AUDIO: u32 = 0x0000_0831;
    const REG_P1_LOCAL: u32 = 0x0100_0840; // global 0x0100_0840
    const REG_P2_LOCAL: u32 = 0x0100_0850; // global 0x0200_0850 (1 master: Master.esm)
    const PLAYER_X: [u8; 24] = [0; 24];

    let player_ref = record(
        b"ACHR",
        0,
        FormId::PLAYER_REF.0,
        &[sub(b"NAME", FormId::PLAYER_BASE.0.to_le_bytes().to_vec()), sub(b"DATA", PLAYER_X.to_vec())],
    );
    let cell_p0 = record(
        b"CELL",
        0,
        EXT_CELL,
        &[sub(b"EDID", z("ExtCell")), sub(b"DATA", vec![0]), sub(b"XCLC", u32s(&[0, 0])), sub(b"XCLR", u32s(&[REG_MASTER_MAP, REG_MASTER_AUDIO]))],
    );
    let cell_int = record(
        b"CELL",
        0,
        INT_CELL,
        &[sub(b"EDID", z("IntCell")), sub(b"DATA", vec![1]), sub(b"XCLR", u32s(&[REG_MASTER_MAP]))],
    );
    let cell_children = group(EXT_CELL.to_le_bytes(), 6, &group(EXT_CELL.to_le_bytes(), 9, &player_ref));
    let wrld_p0 = [
        record(b"WRLD", 0, EXT_WORLD, &[sub(b"EDID", z("MergeWorld")), sub(b"FULL", z("Merge World"))]),
        group(EXT_WORLD.to_le_bytes(), 1, &group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &[cell_p0, cell_children].concat()))),
    ]
    .concat();
    let p0 = [
        tes4_with_masters(&[]),
        top(b"REGN", &[map_regn(REG_MASTER_MAP, "RegMasterMap", "Master Valley"), record(b"REGN", 0, REG_MASTER_AUDIO, &[sub(b"EDID", z("RegMasterAudio"))])].concat()),
        top(b"NPC_", &record(b"NPC_", 0, FormId::PLAYER_BASE.0, &[sub(b"EDID", z("Player"))])),
        top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell_int))),
        top(b"WRLD", &wrld_p0),
    ]
    .concat();

    // Plugin 1 overrides EXT_CELL, adding its own region and duplicating REG_MASTER_AUDIO.
    let cell_p1 = record(
        b"CELL",
        0,
        EXT_CELL,
        &[sub(b"EDID", z("ExtCell")), sub(b"DATA", vec![0]), sub(b"XCLC", u32s(&[0, 0])), sub(b"XCLR", u32s(&[REG_P1_LOCAL, REG_MASTER_AUDIO]))],
    );
    let p1 = [
        tes4_with_masters(&["Master.esm"]),
        top(b"REGN", &record(b"REGN", 0, REG_P1_LOCAL, &[sub(b"EDID", z("RegWeather"))])),
        top(b"WRLD", &group(EXT_WORLD.to_le_bytes(), 1, &group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &cell_p1)))),
    ]
    .concat();

    // Plugin 2 overrides EXT_CELL with only its own non-map region (wiping master regions in vanilla).
    let cell_p2 = record(
        b"CELL",
        0,
        EXT_CELL,
        &[sub(b"EDID", z("ExtCell")), sub(b"DATA", vec![0]), sub(b"XCLC", u32s(&[0, 0])), sub(b"XCLR", u32s(&[REG_P2_LOCAL]))],
    );
    let p2 = [
        tes4_with_masters(&["Master.esm"]),
        top(b"REGN", &record(b"REGN", 0, REG_P2_LOCAL, &[sub(b"EDID", z("RegLighting"))])),
        top(b"WRLD", &group(EXT_WORLD.to_le_bytes(), 1, &group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &cell_p2)))),
    ]
    .concat();

    let plugins = vec![
        Plugin::parse(&p0, "Master.esm".into()).unwrap(),
        Plugin::parse(&p1, "Weather.esp".into()).unwrap(),
        Plugin::parse(&p2, "Lighting.esp".into()).unwrap(),
    ];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    let j: Json = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let table = FunctionTable::load(&p).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let mut data = GameData::build(&lo, &schema, table, &j, &refs);
    // Absent hNVSE.ini: GameData::build must NOT walk overridden CELLs at startup.
    assert!(!data.hnvse_merge_region_lists);
    assert!(data.merged_cell_regions.is_empty());

    // [General] bMergeRegionLists=1 and [Main] bMergeRegionLists=0 are both ignored / disabled
    // (observed in hNVSE 120: only [Main] is read) and leave merged_cell_regions empty.
    assert!(!data.load_hnvse_region_merge(&lo, &schema, Some(b"[General]\nbMergeRegionLists=1\n")));
    assert!(!data.load_hnvse_region_merge(&lo, &schema, Some(b"[Main]\nbMergeRegionLists=0\n")));
    assert!(!data.hnvse_merge_region_lists);
    assert!(data.merged_cell_regions.is_empty());

    let regions_of = |g: &mut Game, edid: &str| -> Vec<u32> {
        let len = match q(g, &format!("array_var a\nlet a := GetCellRegions {edid}\nSetFunctionValue (ar_Size a)")) {
            XVal::Num(n) => n as usize,
            other => panic!("expected number, got {other:?}"),
        };
        (0..len)
            .map(|i| match q(g, &format!("array_var a\nlet a := GetCellRegions {edid}\nSetFunctionValue a[{i}]")) {
                XVal::Ref(r) => r,
                other => panic!("expected ref at {i}, got {other:?}"),
            })
            .collect()
    };

    {
        let mut g = Game::new(&data, World::new(&forms, &refs, SimConfig::default()), GameConfig { strict: true, ..GameConfig::default() });
        g.new_game();

        // Disabled at load: vanilla last-writer-wins (Lighting.esp only; no master map region).
        let vanilla = regions_of(&mut g, "ExtCell");
        assert_eq!(vanilla.len(), 1);
        assert!(!vanilla.contains(&REG_MASTER_MAP));
        assert_eq!(fnv_game::nvse_jg5::player_region_name(&g), None);
        assert_eq!(q(&mut g, "SetFunctionValue (player.GetLocationName)"), XVal::Str("Merge World".into()));

        // Writing hNVSE.ini from a script mid-session does NOT re-read the switch, so
        // the script side and the world side always agree on the load-time setting.
        assert!(g.mod_files.write_bytes("NVSE/Plugins/hNVSE.ini", b"[Main]\nbMergeRegionLists=1\n"));
        assert_eq!(regions_of(&mut g, "ExtCell"), vanilla);
        assert_eq!(fnv_game::nvse_jg5::player_region_name(&g), None);
    }

    // [Main] bMergeRegionLists=1 at load: computes merged_cell_regions in
    // overriding-plugin-first deduplicated order (observed in hNVSE 120).
    assert!(data.load_hnvse_region_merge(&lo, &schema, Some(b"[Main]\nbMergeRegionLists = 1\n")));
    assert!(data.hnvse_merge_region_lists);
    assert!(!data.merged_cell_regions.is_empty());

    let mut g_merged = Game::new(&data, World::new(&forms, &refs, SimConfig::default()), GameConfig { strict: true, ..GameConfig::default() });
    g_merged.new_game();
    assert_eq!(
        regions_of(&mut g_merged, "ExtCell"),
        vec![0x0200_0850, 0x0100_0840, REG_MASTER_AUDIO, REG_MASTER_MAP]
    );
    assert_eq!(fnv_game::nvse_jg5::player_region_name(&g_merged).as_deref(), Some("Master Valley"));
    assert_eq!(q(&mut g_merged, "SetFunctionValue (player.GetLocationName)"), XVal::Str("Master Valley".into()));
    // Interior cells still return empty.
    assert!(regions_of(&mut g_merged, "IntCell").is_empty());
    // Writing bMergeRegionLists=0 from a script mid-session does NOT toggle the load-time switch.
    assert!(g_merged.mod_files.write_bytes("NVSE/Plugins/hNVSE.ini", b"[Main]\nbMergeRegionLists=0\n"));
    assert_eq!(
        regions_of(&mut g_merged, "ExtCell"),
        vec![0x0200_0850, 0x0100_0840, REG_MASTER_AUDIO, REG_MASTER_MAP]
    );
}

#[test]
fn knvse_transition_duration_view_weights_additive_and_non_finite_dest_frame() {
    files();
    let f = fixture();
    let mut g = game(&f);
    g.senses.first_person = false;

    // CrossFadeAnims forwards explicit first-person view, ease time, priority and weight.
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.ActivateAnim \"test\\a.kf\" 1 2 1 0.8 0.1)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.CrossFadeAnims \"test\\a.kf\" \"test\\b.kf\" 1 0.45 5 1 0.65)"), n(1.0));
    let src = g.anim_paths.sequences.get(&(FormId::PLAYER_REF, true, "test\\a.kf".into())).unwrap();
    assert!(src.stopped.is_some());
    assert!((src.ease_out - 0.45).abs() < 1e-6);
    let dst = g.anim_paths.sequences.get(&(FormId::PLAYER_REF, true, "test\\b.kf".into())).unwrap();
    assert!(dst.stopped.is_none());
    assert_eq!(dst.priority, 5);
    assert!((dst.weight - 0.65).abs() < 1e-6);
    assert_eq!(dst.ease_in, Some(0.45));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.GetAnimWeight \"test\\b.kf\")"), n(f64::from(0.65f32)));

    // BlendToAnim forwards duration, priority, source weight and dest frame on the requested view.
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.BlendToAnim \"test\\b.kf\" \"test\\c.kf\" 1 0.35 0.5 0.2 0.75 7)"), n(1.0));
    let b = g.anim_paths.sequences.get(&(FormId::PLAYER_REF, true, "test\\b.kf".into())).unwrap();
    assert!(b.stopped.is_none(), "positive source weight keeps source active");
    assert!((b.weight - 0.2).abs() < 1e-6);
    let c = g.anim_paths.sequences.get(&(FormId::PLAYER_REF, true, "test\\c.kf".into())).unwrap();
    assert_eq!(c.priority, 7);
    assert_eq!(c.ease_in, Some(0.35));
    assert!((c.weight - 0.75).abs() < 1e-6);
    assert!((g.script_sequence_time_for(FormId::PLAYER_REF, "test\\c.kf", true) - 0.5).abs() < 1e-6);

    // BlendToAnimFromPose forwards view, duration, dest frame and priority.
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.BlendToAnimFromPose \"test\\d.kf\" 1 0.25 0.4 9)"), n(1.0));
    let d = g.anim_paths.sequences.get(&(FormId::PLAYER_REF, true, "test\\d.kf".into())).unwrap();
    assert_eq!(d.priority, 9);
    assert_eq!(d.ease_in, Some(0.25));
    assert!((g.script_sequence_time_for(FormId::PLAYER_REF, "test\\d.kf", true) - 0.4).abs() < 1e-6);

    // SetAnimAdditive keeps first- and third-person additive configs isolated per view.
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.SetAnimAdditive \"test\\d.kf\" 1 \"test\\a.kf\" 0.2)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.SetAnimAdditive \"test\\d.kf\" 0 \"test\\b.kf\" 0.8)"), n(1.0));
    assert_eq!(g.sequence_additive(FormId::PLAYER_REF, true, "test\\d.kf"), Some(("test\\a.kf", 0.2)));
    assert_eq!(g.sequence_additive(FormId::PLAYER_REF, false, "test\\d.kf"), Some(("test\\b.kf", 0.8)));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.IsAnimAdditive \"test\\d.kf\")"), n(1.0));

    // Non-finite destFrame in BlendToAnim / BlendToAnimFromPose is rejected without activating or poisoning clock_offset.
    const INF: &str = "(Pow 10 400)";
    const NEG_INF: &str = "(-1 * (Pow 10 400))";
    const NAN: &str = "((Pow 10 400) - (Pow 10 400))";
    for bad in [NAN, INF, NEG_INF] {
        assert_eq!(
            q(&mut g, &format!("SetFunctionValue (PlayerREF.BlendToAnim \"test\\d.kf\" \"test\\a.kf\" 0 0.2 {bad} 0 1 2)")),
            n(0.0),
            "BlendToAnim must reject non-finite destFrame {bad}"
        );
        assert!(!g.anim_paths.sequences.contains_key(&(FormId::PLAYER_REF, false, "test\\a.kf".into())));
        assert_eq!(
            q(&mut g, &format!("SetFunctionValue (PlayerREF.BlendToAnimFromPose \"test\\b.kf\" 0 0.2 {bad} 2)")),
            n(0.0),
            "BlendToAnimFromPose must reject non-finite destFrame {bad}"
        );
        assert!(!g.anim_paths.sequences.contains_key(&(FormId::PLAYER_REF, false, "test\\b.kf".into())));
    }
    assert!(g.script_sequence_time_for(FormId::PLAYER_REF, "test\\d.kf", true).is_finite());

    // timeSync in CrossFadeAnims, BlendToAnim, and BlendToAnimFromPose:
    // - aligns destination clock by normalized phase when both SequenceInfo spans are known;
    // - copies sync clock directly when SequenceInfo spans are absent;
    // - never refuses and still runs the transition when timeSync is not currently playing.
    let mut g_sync = game(&f);
    g_sync.senses.first_person = true;
    g_sync.set_sequence_info(
        "test\\a.kf",
        fnv_game::nvse_knvse::SequenceInfo {
            name: "a".into(),
            accum_root: "Bip01".into(),
            frequency: 1.0,
            begin: 0.0,
            end: 2.0,
            cycle: 0,
            text_keys: Vec::new(),
        },
    );
    g_sync.set_sequence_info(
        "test\\b.kf",
        fnv_game::nvse_knvse::SequenceInfo {
            name: "b".into(),
            accum_root: "Bip01".into(),
            frequency: 1.0,
            begin: 0.0,
            end: 4.0,
            cycle: 0,
            text_keys: Vec::new(),
        },
    );
    assert_eq!(q(&mut g_sync, "SetFunctionValue (PlayerREF.ActivateAnim \"test\\a.kf\" 1 1 1 1 0.1)"), n(1.0));
    q(&mut g_sync, "PlayerREF.SetAnimCurrentTime \"test\\a.kf\" 0.5");
    g_sync.tick();
    let a_time = g_sync.script_sequence_time_for(FormId::PLAYER_REF, "test\\a.kf", true);
    assert!((a_time - (0.5 + g_sync.cfg.frame_seconds)).abs() < 1e-6);
    // CrossFadeAnims with timeSync = source ("test\\a.kf", span 2.0) scales "test\\b.kf" (span 4.0) by normalized phase.
    let expected_b = (a_time / 2.0).rem_euclid(1.0) * 4.0;
    assert_eq!(
        q(&mut g_sync, "SetFunctionValue (PlayerREF.CrossFadeAnims \"test\\a.kf\" \"test\\b.kf\" 1 0.2 3 1 0.9 \"test\\a.kf\")"),
        n(1.0)
    );
    assert!((g_sync.script_sequence_time_for(FormId::PLAYER_REF, "test\\b.kf", true) - expected_b).abs() < 1e-6);

    // BlendToAnim with playing timeSync without SequenceInfo on destination copies sync clock directly.
    assert_eq!(
        q(&mut g_sync, "SetFunctionValue (PlayerREF.BlendToAnim \"test\\b.kf\" \"test\\c.kf\" 1 0.25 0 0 0.8 4 \"test\\b.kf\")"),
        n(1.0)
    );
    assert!((g_sync.script_sequence_time_for(FormId::PLAYER_REF, "test\\c.kf", true) - expected_b).abs() < 1e-6);

    // BlendToAnimFromPose with a non-playing timeSync still succeeds (returns 1) and honors destFrame (0.35).
    assert_eq!(
        q(&mut g_sync, "SetFunctionValue (PlayerREF.BlendToAnimFromPose \"test\\d.kf\" 1 0.2 0.35 5 \"test\\a.kf\")"),
        n(1.0)
    );
    assert!((g_sync.script_sequence_time_for(FormId::PLAYER_REF, "test\\d.kf", true) - 0.35).abs() < 1e-6);
}

#[test]
fn knvse_legacy_anim_time_paths_loaded_matched_and_kept_on_existing_clock() {
    use fnv_game::nvse_knvse::SequenceInfo;
    files();
    let f = fixture();
    let mut g = game(&f);
    g.cfg.frame_seconds = 0.25;
    g.senses.first_person = true;
    g.knvse_ini = fnv_game::knvse_ini::KnvseIni::parse(
        b"[Anim Fixes]\nsLegacyAnimTimePaths = B42Inject, B42Interact, B42Loot\n",
    );

    let legacy_loop = "B42Inject\\loop.kf";
    let legacy_clamp = "Characters\\_1stPerson\\B42Interact\\clamp.kf";
    let modern = "test\\a.kf";
    assert!(g.knvse_ini.uses_legacy_anim_time(legacy_loop));
    assert!(g.knvse_ini.uses_legacy_anim_time(legacy_clamp));
    assert!(!g.knvse_ini.uses_legacy_anim_time(modern));

    g.set_sequence_info(
        legacy_loop,
        SequenceInfo { name: "Loop".into(), accum_root: String::new(), frequency: 2.0, begin: 0.0, end: 1.0, cycle: 0, text_keys: vec![] },
    );
    g.set_sequence_info(
        legacy_clamp,
        SequenceInfo { name: "Clamp".into(), accum_root: String::new(), frequency: 1.0, begin: 0.0, end: 0.5, cycle: 2, text_keys: vec![] },
    );
    g.set_sequence_info(
        modern,
        SequenceInfo { name: "Modern".into(), accum_root: String::new(), frequency: 2.0, begin: 0.0, end: 1.0, cycle: 0, text_keys: vec![] },
    );

    // Establish the actor's animation clock origin and seed all three sequences at time 0.
    let key_loop = legacy_loop.to_ascii_lowercase();
    let key_clamp = legacy_clamp.to_ascii_lowercase();
    let key_modern = modern.to_ascii_lowercase();
    let XVal::Num(now_clock) = q(&mut g, "SetFunctionValue (PlayerREF.GetAnimCurrentTime \"test\\a.kf\")") else {
        panic!("expected numeric anim time");
    };
    g.knvse.offsets.insert((FormId::PLAYER_REF, key_loop), -now_clock);
    g.knvse.offsets.insert((FormId::PLAYER_REF, key_clamp), -now_clock);
    g.knvse.offsets.insert((FormId::PLAYER_REF, key_modern), -now_clock);
    g.anim_paths.played.entry(FormId::PLAYER_REF).or_default().push(legacy_loop.into());
    g.anim_paths.played.entry(FormId::PLAYER_REF).or_default().push(modern.into());

    for _ in 0..3 {
        g.tick();
    }
    // Listed and unlisted paths stay on the existing sequence clock (3 * 0.25 s = 0.75 s)
    // until a committed original-game probe shows where kNVSE v37 diverges.
    let t_loop = g.script_sequence_time_for(FormId::PLAYER_REF, legacy_loop, false);
    let t_clamp = g.script_sequence_time_for(FormId::PLAYER_REF, legacy_clamp, false);
    let t_modern = g.script_sequence_time_for(FormId::PLAYER_REF, modern, false);
    assert!((t_loop - 0.75).abs() < 1e-6, "listed looping path stays on existing clock: got {t_loop}");
    assert!((t_clamp - 0.75).abs() < 1e-6, "listed clamped path stays on existing clock: got {t_clamp}");
    assert!((t_modern - 0.75).abs() < 1e-6, "unlisted path stays on existing clock: got {t_modern}");

    // Pause/resume and GetAnimationAttributes keep existing clock contract on listed paths too.
    g.knvse_ini = fnv_game::knvse_ini::KnvseIni::parse(b"[Anim Fixes]\nsLegacyAnimTimePaths = test\n");
    assert!(g.knvse_ini.uses_legacy_anim_time(modern));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.ActivateAnim \"test\\a.kf\" 1 1 1 1 0)"), n(1.0));
    g.tick(); // 0.25 s on existing clock
    assert!((g.script_sequence_time_for(FormId::PLAYER_REF, modern, true) - 0.25).abs() < 1e-6);
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.DeactivateAnim \"test\\a.kf\" 0)"), n(1.0));
    g.tick(); // inactive while paused
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.ActivateAnim \"test\\a.kf\" 1 1 0 1 0)"), n(1.0));
    let resumed = g.script_sequence_time_for(FormId::PLAYER_REF, modern, true);
    assert!((resumed - 0.25).abs() < 1e-6, "resuming listed sequence preserves existing clock 0.25: got {resumed}");

    let XVal::Num(last_time) = q(&mut g, "array_var a\nlet a := PlayerREF.GetAnimationAttributes \"test\\a.kf\"\nSetFunctionValue (a[\"lastTime\"])") else {
        panic!("expected lastTime");
    };
    let XVal::Num(last_scaled) = q(&mut g, "array_var a\nlet a := PlayerREF.GetAnimationAttributes \"test\\a.kf\"\nSetFunctionValue (a[\"lastScaledTime\"])") else {
        panic!("expected lastScaledTime");
    };
    let XVal::Num(calc_time) = q(&mut g, "array_var a\nlet a := PlayerREF.GetAnimationAttributes \"test\\a.kf\"\nSetFunctionValue (a[\"calculatedTime\"])") else {
        panic!("expected calculatedTime");
    };
    assert!((last_time - 0.25).abs() < 1e-6, "lastTime stays on existing clock 0.25: got {last_time}");
    assert!((last_scaled - 0.5).abs() < 1e-6, "lastScaledTime scales once by frequency to 0.5: got {last_scaled}");
    assert!((calc_time - 0.25).abs() < 1e-6, "calculatedTime stays on existing clock 0.25: got {calc_time}");

    // Clearing sLegacyAnimTimePaths leaves matching off while keeping clock behavior unchanged.
    g.knvse_ini.legacy_anim_time_paths.clear();
    assert!(!g.knvse_ini.uses_legacy_anim_time(legacy_loop));
    let t_loop_vanilla = g.script_sequence_time_for(FormId::PLAYER_REF, legacy_loop, false);
    assert!((t_loop_vanilla - 1.25).abs() < 1e-6, "vanilla default keeps existing clock 1.25: got {t_loop_vanilla}");
}

#[test]
fn knvse_play_animation_path_pre_tick_neg_flt_max_and_frequency_scaled_delta() {
    files();
    let f = fixture();
    let mut g = game(&f);
    g.cfg.frame_seconds = 0.055;
    g.senses.first_person = true;
    g.knvse_ini = fnv_game::knvse_ini::KnvseIni::parse(
        b"[Anim Fixes]\nsLegacyAnimTimePaths = W4ELegacy,B42Inject\n",
    );
    let legacy = "test\\W4ELegacy\\test.kf";
    let normal = "test\\W4ENormal\\test.kf";
    assert!(g.knvse_ini.uses_legacy_anim_time(legacy));
    assert!(!g.knvse_ini.uses_legacy_anim_time(normal));

    // Advance a few frames so GetGlobalAnimTime is nonzero before PlayAnimationPath.
    for _ in 0..8 {
        g.tick();
    }
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.PlayAnimationPath \"test\\W4ELegacy\\test.kf\" 1)"), n(1.0));
    assert_eq!(q(&mut g, "SetFunctionValue (PlayerREF.PlayAnimationPath \"test\\W4ENormal\\test.kf\" 1)"), n(1.0));

    // Immediately after PlayAnimationPath before the first tick, GetAnimCurrentTime reads
    // -FLT_MAX (t0L=-3.40282e+38) [observed: w4e_08_knvse_legacy_anim_time], while
    // pre-first-tick offset and lastTime returning -f32::MAX follows
    // nvse_knvse.rs and docs/spec-notes/ext-knvse.md §5.
    let neg_flt_max = f64::from(-f32::MAX);
    for path in [legacy, normal] {
        let XVal::Num(t0) = q(&mut g, &format!("SetFunctionValue (PlayerREF.GetAnimCurrentTime \"{path}\")")) else {
            panic!("expected GetAnimCurrentTime number");
        };
        let XVal::Num(off0) = q(&mut g, &format!("array_var a\nlet a := PlayerREF.GetAnimationAttributes \"{path}\"\nSetFunctionValue (a[\"offset\"])")) else {
            panic!("expected offset");
        };
        let XVal::Num(last0) = q(&mut g, &format!("array_var a\nlet a := PlayerREF.GetAnimationAttributes \"{path}\"\nSetFunctionValue (a[\"lastTime\"])")) else {
            panic!("expected lastTime");
        };
        assert_eq!(t0, neg_flt_max, "{path} pre-tick GetAnimCurrentTime must be -FLT_MAX");
        assert_eq!(off0, neg_flt_max, "{path} pre-tick offset must be -FLT_MAX");
        assert_eq!(last0, neg_flt_max, "{path} pre-tick lastTime must be -FLT_MAX");
    }

    // After ticking at frequency 1.0 (3 ticks -> 0.165 s), both legacy and normal paths match.
    for _ in 0..3 {
        g.tick();
    }
    let XVal::Num(g1) = q(&mut g, "SetFunctionValue (PlayerREF.GetGlobalAnimTime 1)") else { panic!("g1") };
    let XVal::Num(t1_l) = q(&mut g, "SetFunctionValue (PlayerREF.GetAnimCurrentTime \"test\\W4ELegacy\\test.kf\")") else { panic!("t1_l") };
    let XVal::Num(t1_n) = q(&mut g, "SetFunctionValue (PlayerREF.GetAnimCurrentTime \"test\\W4ENormal\\test.kf\")") else { panic!("t1_n") };
    let XVal::Num(off1_l) = q(&mut g, "array_var a\nlet a := PlayerREF.GetAnimationAttributes \"test\\W4ELegacy\\test.kf\"\nSetFunctionValue (a[\"offset\"])") else { panic!("off1_l") };
    let XVal::Num(last1_l) = q(&mut g, "array_var a\nlet a := PlayerREF.GetAnimationAttributes \"test\\W4ELegacy\\test.kf\"\nSetFunctionValue (a[\"lastTime\"])") else { panic!("last1_l") };
    assert!((t1_l - 0.165).abs() < 1e-6, "got {t1_l}");
    assert!((t1_n - t1_l).abs() < 1e-9);
    assert!((last1_l - t1_l).abs() < 1e-6);

    // After SetAnimSequenceFrequency 2.5 and 3 more ticks (dg = 0.165 s):
    // GetAnimCurrentTime = t1 + dg * 2.5, offset stays fixed, lastTime = t1 + dg (unscaled)
    // identically on BOTH listed (W4ELegacy) and unlisted (W4ENormal) paths [observed: w4e_08].
    q(&mut g, "PlayerREF.SetAnimSequenceFrequency \"test\\W4ELegacy\\test.kf\" 2.5");
    q(&mut g, "PlayerREF.SetAnimSequenceFrequency \"test\\W4ENormal\\test.kf\" 2.5");
    for _ in 0..3 {
        g.tick();
    }
    let XVal::Num(g2) = q(&mut g, "SetFunctionValue (PlayerREF.GetGlobalAnimTime 1)") else { panic!("g2") };
    let dg = g2 - g1;
    assert!((dg - 0.165).abs() < 1e-6);
    for path in [legacy, normal] {
        let XVal::Num(t2) = q(&mut g, &format!("SetFunctionValue (PlayerREF.GetAnimCurrentTime \"{path}\")")) else { panic!("t2") };
        let XVal::Num(off2) = q(&mut g, &format!("array_var a\nlet a := PlayerREF.GetAnimationAttributes \"{path}\"\nSetFunctionValue (a[\"offset\"])")) else { panic!("off2") };
        let XVal::Num(last2) = q(&mut g, &format!("array_var a\nlet a := PlayerREF.GetAnimationAttributes \"{path}\"\nSetFunctionValue (a[\"lastTime\"])")) else { panic!("last2") };
        assert!((t2 - (t1_l + dg * 2.5)).abs() < 1e-6, "{path} GetAnimCurrentTime expected {}, got {t2}", t1_l + dg * 2.5);
        assert!((off2 - off1_l).abs() < 1e-9, "{path} offset must remain fixed ({off1_l}), got {off2}");
        assert!((last2 - (t1_l + dg)).abs() < 1e-6, "{path} lastTime must remain unscaled ({}), got {last2}", t1_l + dg);
    }
}
