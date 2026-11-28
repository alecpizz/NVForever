//! Authored callbacks on the actual compiler, Game::hit and Game::tick paths.
//! No installed scripts, game data, native calls or fabricated callback clock.
use super::*;
use fnv_game::game::RunCtx;
use fnv_script::vm::XVal;

const RESTORE: u32 = 0xA00;
const HIT: u32 = 0xA01;
const OBSERVE: u32 = 0xA02;
const NESTED: u32 = 0xA03;

fn clock_fixture() -> Fixture {
    struct Names;
    impl fnv_script::compile::Env for Names {
        fn form(&self, name: &str) -> Option<fnv_script::compile::FormInfo> {
            let form = match name.to_ascii_lowercase().as_str() {
                "ownedrestore" => RESTORE,
                "ownedhit" => HIT,
                "ownedobserve" => OBSERVE,
                "ownednested" => NESTED,
                _ => return None,
            };
            Some(fnv_script::compile::FormInfo { form, sig: *b"SCPT" })
        }
        fn info(&self, _: u32) -> Option<fnv_script::compile::FormInfo> { None }
        fn editor_id(&self, _: u32) -> Option<String> { None }
        fn local_form(&self, _: u32) -> Option<u32> { None }
        fn script_vars(&self, _: u32) -> Option<Vec<fnv_script::VarDecl>> { None }
    }
    let scripts = [
        (RESTORE, "OwnedRestore", r#"scn OwnedRestore
ref actor
float left
Begin Function {}
    let actor := GetSelfAlt
    let left := actor.AuxVarGetFlt "*_owned_remaining"
    if left <= 0
        actor.ModAV SpeedMult (actor.AuxVarGetFlt "*_owned_restore")
        actor.AuxVarErase "*_owned_restore"
        actor.AuxVarErase "*_owned_remaining"
        actor.AuxVarSetFlt "*_owned_finishes" ((actor.AuxVarGetFlt "*_owned_finishes") + 1)
        actor.SGMLC OwnedRestore 0
    else
        let left -= GetSecondsPassed
        actor.AuxVarSetFlt "*_owned_remaining" left
    endif
End
"#),
        (HIT, "OwnedHit", r#"scn OwnedHit
ref actor
float delta
Begin Function {}
    let actor := GetSelfAlt
    if eval (actor.AuxVarSize "*_owned_restore") == 0
        let delta := (actor.GetAV SpeedMult) * 0.75
        actor.AuxVarSetFlt "*_owned_restore" delta
        actor.ModAV SpeedMult (delta * -1)
    endif
    actor.AuxVarSetFlt "*_owned_remaining" 0.2
    actor.SGMLC OwnedRestore 1 1 9
End
"#),
        (OBSERVE, "OwnedObserve", r#"scn OwnedObserve
ref actor
float elapsed
Begin Function {}
    let actor := GetSelfAlt
    let elapsed := GetSecondsPassed
    actor.AuxVarSetFlt "*_owned_elapsed" elapsed
    actor.AuxVarSetFlt "*_owned_same" (GetSecondsPassed - elapsed)
    actor.AuxVarSetFlt "*_owned_nested" (Call OwnedNested)
    actor.AuxVarSetFlt "*_owned_sum" ((actor.AuxVarGetFlt "*_owned_sum") + elapsed)
    actor.AuxVarSetFlt "*_owned_count" ((actor.AuxVarGetFlt "*_owned_count") + 1)
End
"#),
        (NESTED, "OwnedNested", "scn OwnedNested\nBegin Function {}\nSetFunctionValue GetSecondsPassed\nEnd\n"),
    ];
    let (table, _) = catalogue();
    let mut records = Vec::new();
    for (id, name, src) in scripts {
        let c = fnv_script::compile::compile(src, &table, &Names, None);
        assert!(c.errors().next().is_none(), "{name}: {:?}", c.diags);
        let vars: Vec<(u32, &str, u8)> = c.variables.iter().map(|v| (v.index, v.name.as_str(), u8::from(v.ty == fnv_script::VarType::Int))).collect();
        let mut subs = unit(0, &c.bytes, &vars, &[]);
        subs[0].data[4..8].copy_from_slice(&(c.refs.len() as u32).to_le_bytes());
        for r in &c.refs {
            subs.push(match r {
                fnv_script::unit::RefSlot::Form(f) | fnv_script::unit::RefSlot::Unmapped(f) => sub(b"SCRO", f.to_le_bytes().to_vec()),
                fnv_script::unit::RefSlot::Local(v) => sub(b"SCRV", v.to_le_bytes().to_vec()),
            });
        }
        subs.push(sub(b"EDID", z(name)));
        subs.push(sub(b"SCTX", src.as_bytes().to_vec()));
        records.push(record(b"SCPT", 0, id, &subs));
    }
    let bytes = [plugin_bytes(), top(b"SCPT", &records.concat())].concat();
    let plugins = [Plugin::parse(&bytes, "OwnedClock.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = schema();
    let (table, j) = catalogue();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let data = GameData::build(&lo, &schema, table, &j, &refs);
    Fixture { data, forms, refs }
}

fn ctx(actor: u32) -> RunCtx { RunCtx { self_ref: Some(FormId(actor)), ..Default::default() } }
fn query(g: &mut Game, actor: u32, expr: &str) -> f64 {
    let value = g.run_query(&format!("Begin Function {{}}\nSetFunctionValue ({expr})\nEnd\n"), ctx(actor)).unwrap();
    let XVal::Num(value) = value else { panic!("expected numeric query, got {value:?}") };
    value
}
fn command(g: &mut Game, actor: u32, body: &str) {
    g.run_query(&format!("Begin Function {{}}\n{body}\nEnd\n"), ctx(actor)).unwrap();
}
fn read(g: &mut Game, actor: u32, key: &str) -> f64 { query(g, actor, &format!("AuxVarGetFlt \"*_owned_{key}\"")) }
fn ticks(g: &mut Game, n: usize) { for _ in 0..n { g.tick(); } }
fn observation_callback(g: &mut Game, actor: u32, add: bool, delay: u64, flags: u32) {
    // SGMLC's receiver is explicit. A bare command registers on the player even
    // when the containing query has another RunCtx.self_ref (vm/jip.rs).
    let add = u8::from(add);
    g.run_query(&format!("ref subject\nBegin Function {{}}\nlet subject := GetSelfAlt\nsubject.SGMLC OwnedObserve {add} {delay} {flags}\nEnd\n"), ctx(actor)).unwrap();
}
fn register(g: &mut Game, actor: u32, delay: u64, flags: u32) {
    observation_callback(g, actor, true, delay, flags);
}
fn configure(g: &mut Game) {
    g.cfg.frame_seconds = 1.0 / 64.0;
    for actor in [0x14, NPC_REF, NPC_AUTO_REF] {
        command(g, actor, "SetAV SpeedMult 100");
        assert_eq!(query(g, actor, "GetAV SpeedMult"), 100.0);
    }
}

#[test]
fn on_hit_callback_restores_each_subject_and_erases_its_timer() {
    let f = clock_fixture();
    let mut g = game(&f);
    configure(&mut g);
    command(&mut g, NPC_REF, "SetOnHitEventHandler OwnedHit 1");
    g.hit(FormId(NPC_REF), 1.0, Default::default());
    assert_eq!(query(&mut g, NPC_REF, "GetAV SpeedMult"), 25.0);
    assert!((read(&mut g, NPC_REF, "remaining") - 0.2).abs() < 1e-7);
    ticks(&mut g, 8);
    assert!(read(&mut g, NPC_REF, "remaining") > 0.0);
    assert!(read(&mut g, NPC_REF, "remaining") < 0.2, "the real callback must advance time");
    g.hit(FormId::PLAYER_REF, 1.0, Default::default());
    assert_eq!(query(&mut g, 0x14, "GetAV SpeedMult"), 25.0);
    ticks(&mut g, 8);
    assert_eq!(query(&mut g, NPC_REF, "GetAV SpeedMult"), 100.0, "the older timer expires independently");
    assert_eq!(query(&mut g, 0x14, "GetAV SpeedMult"), 25.0, "the later subject still owns its hold");
    ticks(&mut g, 8);
    for actor in [NPC_REF, 0x14] {
        assert_eq!(query(&mut g, actor, "GetAV SpeedMult"), 100.0);
        assert_eq!(query(&mut g, actor, "AuxVarSize \"*_owned_restore\""), 0.0);
        assert_eq!(query(&mut g, actor, "AuxVarSize \"*_owned_remaining\""), 0.0);
        assert_eq!(read(&mut g, actor, "finishes"), 1.0);
    }
    ticks(&mut g, 20);
    assert_eq!(read(&mut g, NPC_REF, "finishes"), 1.0, "actual unregistration prevents a second restore");
    assert_eq!(query(&mut g, NPC_AUTO_REF, "GetAV SpeedMult"), 100.0, "untouched neighbor");
}

#[test]
fn registration_clocks_are_independent_and_nested_calls_share_one_interval() {
    let f = clock_fixture();
    let mut g = game(&f);
    configure(&mut g);
    register(&mut g, NPC_REF, 1, 1);
    register(&mut g, 0x14, 5, 1);
    ticks(&mut g, 10);
    assert_eq!(read(&mut g, NPC_REF, "count"), 10.0);
    assert_eq!(read(&mut g, 0x14, "count"), 2.0);
    assert_eq!(read(&mut g, NPC_AUTO_REF, "count"), 0.0, "unregistered subject remains untouched");
    // Both delay=1 and delay=5 callbacks receive the single-frame delta, not accumulated time
    // across skipped frames (w4g_03_jip_callback_getsecondspassed).
    assert_eq!(read(&mut g, NPC_REF, "elapsed"), 1.0 / 64.0);
    assert_eq!(read(&mut g, 0x14, "elapsed"), 1.0 / 64.0);
    assert_eq!(read(&mut g, NPC_REF, "sum"), 10.0 / 64.0);
    assert_eq!(read(&mut g, 0x14, "sum"), 2.0 / 64.0);
    for actor in [NPC_REF, 0x14] {
        assert_eq!(read(&mut g, actor, "same"), 0.0);
        assert_eq!(read(&mut g, actor, "nested"), read(&mut g, actor, "elapsed"));
    }
}

#[test]
fn menu_frames_do_not_age_game_mode_timers_or_supply_wall_time() {
    let f = clock_fixture();
    let mut g = game(&f);
    configure(&mut g);
    register(&mut g, NPC_REF, 1, 1);
    register(&mut g, 0x14, 1, 3);
    ticks(&mut g, 4);
    let count = read(&mut g, NPC_REF, "count");
    g.menu = Some(1008);
    let before = g.time;
    ticks(&mut g, 100);
    assert_eq!(g.time, before);
    assert_eq!(read(&mut g, NPC_REF, "count"), count);
    assert_eq!(read(&mut g, 0x14, "elapsed"), 0.0);
    g.menu = None;
    ticks(&mut g, 1);
    assert_eq!(read(&mut g, NPC_REF, "elapsed"), 1.0 / 64.0, "the prior game frame, not the paused interval");
    ticks(&mut g, 1);
    assert_eq!(read(&mut g, 0x14, "elapsed"), 1.0 / 64.0);
}

#[test]
fn explicit_removal_and_reregistration_start_a_fresh_interval() {
    let f = clock_fixture();
    let mut g = game(&f);
    configure(&mut g);
    register(&mut g, NPC_REF, 3, 1);
    ticks(&mut g, 6);
    observation_callback(&mut g, NPC_REF, false, 1, 1);
    let count = read(&mut g, NPC_REF, "count");
    ticks(&mut g, 40);
    assert_eq!(read(&mut g, NPC_REF, "count"), count);
    register(&mut g, NPC_REF, 2, 1);
    ticks(&mut g, 2);
    assert_eq!(read(&mut g, NPC_REF, "elapsed"), 1.0 / 64.0);
    assert_eq!(read(&mut g, NPC_REF, "count"), count + 1.0);
}

#[test]
fn load_removes_flag_eight_and_rebases_only_retained_registration_clocks() {
    let f = clock_fixture();
    let mut g = game(&f);
    configure(&mut g);
    register(&mut g, NPC_REF, 1, 9);
    register(&mut g, 0x14, 1, 1);
    ticks(&mut g, 12);
    // Both actual receiver-bound callbacks must have run before testing transfer.
    // Otherwise an overwritten player-only registration could falsely pass cleanup.
    assert_eq!(read(&mut g, NPC_REF, "count"), 12.0);
    assert_eq!(read(&mut g, 0x14, "count"), 12.0);
    let session = g.take_session();
    let mut loaded = game(&f);
    configure(&mut loaded);
    loaded.time = 100.0;
    loaded.put_session(session);
    ticks(&mut loaded, 1);
    assert_eq!(read(&mut loaded, NPC_REF, "count"), 0.0);
    assert_eq!(read(&mut loaded, 0x14, "count"), 1.0);
    assert_eq!(read(&mut loaded, 0x14, "elapsed"), 1.0 / 64.0);
    ticks(&mut loaded, 1);
    assert_eq!(read(&mut loaded, 0x14, "elapsed"), 1.0 / 64.0);
}

#[test]
fn ordinary_udfs_results_and_persistent_instance_clocks_keep_their_contracts() {
    let f = clock_fixture();
    let mut g = game(&f);
    let mut neighbor = game(&f);
    configure(&mut g);
    configure(&mut neighbor);
    register(&mut g, NPC_REF, 1, 1);
    g.run_source("SetStage Q 10", Default::default()).unwrap();
    neighbor.run_source("SetStage Q 10", Default::default()).unwrap();
    for _ in 0..80 {
        g.tick();
        neighbor.tick();
        assert_eq!(query(&mut g, 0x14, "Q.fTimer"), query(&mut neighbor, 0x14, "Q.fTimer"));
    }
    // Ordinary stateless UDFs (direct Call, query UDF, and CallAfterSeconds) observe the
    // single-frame delta, shared with repeated reads and nested Calls (w4g_03).
    assert_eq!(query(&mut g, NPC_REF, "Call OwnedNested"), 1.0 / 64.0);
    assert_eq!(query(&mut g, NPC_REF, "GetSecondsPassed"), 1.0 / 64.0);
    command(&mut g, NPC_AUTO_REF, "CallAfterSeconds 0.05 OwnedObserve 1");
    ticks(&mut g, 5);
    assert_eq!(read(&mut g, NPC_AUTO_REF, "count"), 1.0);
    assert_eq!(read(&mut g, NPC_AUTO_REF, "elapsed"), 1.0 / 64.0);
    assert_eq!(read(&mut g, NPC_AUTO_REF, "same"), 0.0);
    assert_eq!(read(&mut g, NPC_AUTO_REF, "nested"), 1.0 / 64.0);
    // Result scripts (not UDFs, no persistent instance) still read zero.
    g.run_source("set TestGlobal to GetSecondsPassed", Default::default()).unwrap();
    assert_eq!(g.world.global(FormId(GLOBAL)).unwrap(), 0.0);
    assert!(read(&mut g, NPC_REF, "sum") > 1.0);
}
