//! The game's display size settings (`iSize W`/`iSize H` under `[Display]`) as the
//! extender's INI functions read them: the live window's size once the engine tells it,
//! else the INI text's value, else missing (`nvse_settings::live_display_size`).
use crate::common::*;
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_game::{
    game::{Game, GameConfig, RunCtx},
    GameData,
};
use fnv_script::{vm::XVal, FunctionTable};
use fnv_sim::{FormStore, RefTable, World};

struct Fixture {
    data: GameData,
    forms: FormStore,
    refs: RefTable,
}

fn fixture(ini: Option<&str>) -> Fixture {
    let bytes = [tes4(), top(b"NPC_", &record(b"NPC_", 0, 7, &[sub(b"EDID", z("Player"))]))].concat();
    let plugins = [Plugin::parse(&bytes, "IniDisplaySize.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let schema = Schema::load_dir(root.join("fnv")).unwrap();
    let path = root.join("script/functions.json");
    let catalogue = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let forms = FormStore::build(&lo, &schema);
    let refs = RefTable::build(&lo, &schema);
    let mut data = GameData::build(&lo, &schema, FunctionTable::load(&path).unwrap(), &catalogue, &refs);
    data.ini_texts = ini.map(|t| vec![t.to_string()]).unwrap_or_default();
    Fixture { data, forms, refs }
}

fn game(f: &Fixture) -> Game<'_> {
    Game::new(&f.data, World::new(&f.forms, &f.refs, Default::default()), GameConfig { strict: true, ..Default::default() })
}

fn value(g: &mut Game, expression: &str) -> Result<XVal, String> {
    g.run_query(&format!("Begin Function {{}}\nSetFunctionValue ({expression})\nEnd\n"), RunCtx::default())
}

fn number(g: &mut Game, setting: &str) -> f64 {
    match value(g, &format!("GetNumericINISetting \"{setting}\"")) {
        Ok(XVal::Num(x)) => x,
        v => panic!("{setting}: {v:?}"),
    }
}

#[test]
fn no_window_and_no_ini_keeps_the_missing_value() {
    let f = fixture(None);
    let mut g = game(&f);
    assert_eq!(number(&mut g, "iSize W:Display"), -1.0, "missing, as before");
    assert_eq!(number(&mut g, "iSize H:Display"), -1.0);
    assert!(value(&mut g, "GetStringIniSetting \"iSize W:Display\"").is_err(), "the string form still fails");
}

#[test]
fn a_headless_run_keeps_the_ini_text_value() {
    let f = fixture(Some("[Display]\niSize W=1920\niSize H=1080\n"));
    let mut g = game(&f);
    assert_eq!(number(&mut g, "iSize W:Display"), 1920.0);
    assert_eq!(number(&mut g, "iSize H:Display"), 1080.0);
}

#[test]
fn the_live_window_answers_without_ini_text() {
    // The case the Pip-Boy mods hit: no `--mo2`, so no INI text, and a script that
    // recentres the cursor whenever it is past `iSize W` + 20 pixels.
    let f = fixture(None);
    let mut g = game(&f);
    g.other.jip9.window = Some((3440.0, 1369.0));
    assert_eq!(number(&mut g, "iSize W:Display"), 3440.0);
    assert_eq!(number(&mut g, "ISIZE H:DISPLAY"), 1369.0, "names ignore case");
    assert_eq!(value(&mut g, "GetStringIniSetting \"iSize W:Display\""), Ok(XVal::Str("3440".into())));
    assert_eq!(number(&mut g, "iSize W:General"), -1.0, "only the [Display] keys");
    assert_eq!(number(&mut g, "fDefaultFOV:Display"), -1.0, "other settings are still missing");
}

#[test]
fn the_live_window_answers_above_an_ini_with_another_size() {
    let f = fixture(Some("[Display]\niSize W=1280\niSize H=768\nfDefaultFOV=75.0000\n"));
    let mut g = game(&f);
    g.other.jip9.window = Some((2560.0, 1440.0));
    assert_eq!(number(&mut g, "iSize W:Display"), 2560.0);
    assert_eq!(number(&mut g, "iSize H:Display"), 1440.0);
    assert_eq!(number(&mut g, "fDefaultFOV:Display"), 75.0, "other INI values are untouched");
    // A window the engine reports as empty (minimised) is not a size: the INI answers.
    g.other.jip9.window = Some((0.0, 0.0));
    assert_eq!(number(&mut g, "iSize W:Display"), 1280.0);
}
