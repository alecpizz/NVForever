//! `fnvtool ext …`: script-extender use in a load order (task N2 of
//! docs/handoff/extender-core-tasks.md). Only names and counts are printed, never script
//! text for scans. Runtime diagnostic detail and played-log messages can contain private
//! text or paths and are emitted only with an explicit detail option.
//!
//! - **Functions**: every identifier of a unit's source (`SCTX`) that names a catalogue
//!   function of family `extender` (name or alias, any case; `schemas/script/functions.json`),
//!   with `;` comments and quoted text left out, and the unit's own variable names skipped
//!   (a local called `Rand` is not a call).
//! - **Syntax**: lines using `let`, `:=`, the compound assignments, `if eval`, `while`,
//!   `foreach`, `string_var`, `array_var`, a `Begin _Block` (the compiler override), `[`
//!   and `->` (docs/spec-notes/ext-syntax.md).
//! - **Units without source**: when a unit has no `SCTX`, its compiled bytes are decoded
//!   and any function opcode at or above 0x1400 counts as extender use: the base game's
//!   functions end at 0x13FF [documented: GECK wiki "NVSE Opcode Base"]. Such units could
//!   only run through the extender's bytecode (extender-compat.md, route 1).

use crate::esm_cmd::open_load_order;
use crate::LoadOrderArgs;
use anyhow::Context;
use clap::Subcommand;
use fnv_esm::{LoadOrder, Schema};
use fnv_script::unit::read_load_order;
use fnv_script::{decode, Expr, FunctionTable, ScriptUnit, Stmt, Token};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// First opcode past the base game's range [documented: GECK wiki "NVSE Opcode Base"].
const EXTENDER_OPCODE_BASE: u16 = 0x1400;

/// Syntax features counted, in output order.
const SYNTAX: [&str; 11] =
    ["let", "assign", "compound", "if_eval", "while", "foreach", "string_var", "array_var", "begin_override", "subscript", "member"];

const COMPOUND: [&str; 8] = ["+=", "-=", "*=", "/=", "^=", "%=", "|=", "&="];

#[derive(Subcommand)]
pub enum ExtCmd {
    /// Count the extender functions and syntax each plugin's scripts use (fnv.ext.scan/1).
    Scan {
        #[command(flatten)]
        lo: LoadOrderArgs,
        /// Layer 1 schema directory (schemas/fnv).
        #[arg(long)]
        schema: PathBuf,
        /// Function catalogue (default: ../script/functions.json next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
    },
    /// What the game makes of the load order's extender scripts when it starts
    /// (fnv.ext.report/1): per plugin, how many units run from their source text, which
    /// kept their bytes because the source does not compile (unit, line and kind of the
    /// first error), and the functions they call that NVForever has no entry for. The
    /// build is the game's own (`fnv_game::source_compile`). No script text is printed
    /// unless `--detail` is given.
    Report {
        #[command(flatten)]
        lo: LoadOrderArgs,
        /// Layer 1 schema directory (schemas/fnv).
        #[arg(long)]
        schema: PathBuf,
        /// Function catalogue (default: ../script/functions.json next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// Also print each error's detail, which may quote a line of the mod's script:
        /// keep such output private.
        #[arg(long)]
        detail: bool,
    },
    /// Static call coverage over the game's compiled units (fnv.ext.connections/1).
    /// Does not run scripts or placeholder probes. Named calls remain unchecked; a
    /// registered handler does not prove its effect or native/mod compatibility.
    Connections {
        #[command(flatten)]
        lo: LoadOrderArgs,
        #[arg(long)]
        schema: PathBuf,
        #[arg(long)]
        functions: Option<PathBuf>,
        /// Only these plugin file names (case-insensitive); an unknown name is refused.
        #[arg(id = "connections_selected_plugins", long = "plugin")]
        selected_plugins: Vec<String>,
    },
    /// Read only an explicit played log; no game/profile scan. Bounded input and
    /// retention; default output contains counts, never message text or file paths.
    ModLog {
        log: PathBuf,
        #[arg(long)]
        previous: Option<PathBuf>,
        /// Include private log messages/paths in entries and the two-run difference.
        #[arg(long)]
        detail: bool,
        /// Write the same JSON into a NEW file; existing files are never overwritten.
        #[arg(long)]
        summary: Option<PathBuf>,
    },
    /// Run one probe set's quests headless (tools/capture/ext-probes/<set>) and collect
    /// the EXTPROBE lines they print, as the in-game collector does (fnv.ext.probes/1).
    Probes {
        #[command(flatten)]
        lo: LoadOrderArgs,
        /// Layer 1 schema directory (schemas/fnv).
        #[arg(long)]
        schema: PathBuf,
        /// Function catalogue (default: ../script/functions.json next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// The probe set: strings, expressions, arrays, loops or udf.
        #[arg(long)]
        probes: String,
        /// The probe sources (default: tools/capture/ext-probes).
        #[arg(long, default_value = "tools/capture/ext-probes")]
        probes_dir: PathBuf,
        /// Frames to run after starting each quest.
        #[arg(long, default_value_t = 600)]
        frames: u64,
        /// Also run the runaway loop and the deep recursion.
        #[arg(long)]
        risky: bool,
        /// Start every quest's run from the original game's save (`.fos`, only read), as
        /// `ext queries --save`. What it could not carry over is listed under `save`.
        #[arg(long)]
        save: Option<PathBuf>,
    },
    /// Run a runtime-query job (fnv.capture.steps/1, as tools/capture/jip_probes.py writes
    /// it) headless, one runner query per step in order, and write fnv.ext.probes/1 as
    /// jip_probes.py's collector does.
    Queries {
        #[command(flatten)]
        lo: LoadOrderArgs,
        /// Layer 1 schema directory (schemas/fnv).
        #[arg(long)]
        schema: PathBuf,
        /// Function catalogue (default: ../script/functions.json next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// The job file (tools/capture/jip-probes/<job>.steps.json).
        #[arg(long)]
        steps: PathBuf,
        /// Move the player to this cell (FormID or Editor ID) before the first step, to
        /// start where the oracle's save stands.
        #[arg(long)]
        start_cell: Option<String>,
        /// Start from the original game's save (`.fos`, only read): its globals, quests,
        /// the player's SPECIAL, skills, inventory and place (fnv_save::fos). Its place
        /// replaces `--start-cell`. What it could not carry over is listed under `save`.
        #[arg(long)]
        save: Option<PathBuf>,
        /// Also write the run's whole trace (fnv.script.trace) to this file, for debugging
        /// a row.
        #[arg(long)]
        trace: Option<PathBuf>,
        /// The game's INI files the extender's INI functions read, in order (only read).
        #[arg(long)]
        ini: Vec<PathBuf>,
        /// A folder that stands for Data\Config (the INI files mods read with LU, ShowOff
        /// and MCM functions), as `script run --config`. Writes stay in memory.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Compile the JIP Script Runner files (`Data\NVSE\plugins\scripts\*.txt`,
    /// fnv_game::script_runner) against the load order, and with `--run` run the game
    /// restart, new-game, and game-exit files in a headless new game
    /// (fnv.ext.script_runner/1): each file's name and whether it compiled (ran), or the
    /// error. No script text is printed.
    ScriptRunner {
        #[command(flatten)]
        lo: LoadOrderArgs,
        /// Layer 1 schema directory (schemas/fnv).
        #[arg(long)]
        schema: PathBuf,
        /// Function catalogue (default: ../script/functions.json next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// More folders that stand for Data (a mod manager's mod folders), read before it,
        /// in order. Only read.
        #[arg(long = "root")]
        roots: Vec<PathBuf>,
        /// Also run the files of the game restart, new game, and game exit.
        #[arg(long)]
        run: bool,
        /// Include private unit names and printed/error text in the new diagnostics.
        /// Counts alone are included by default when --run is given.
        #[arg(long, requires = "run")]
        detail: bool,
        /// Leave the Data folder's loose files out (only the `--root` folders' files are
        /// read; the load order and its archives still are): a check whose result must not
        /// depend on what the player installed.
        #[arg(long)]
        no_data_files: bool,
    },
    /// Write the compiled script units that have no source text (route 1 input) to a
    /// directory: each unit's bytes as `<n>.scda`, and `units.json` with its plugin, owner,
    /// header, variables, references and whether its bytes call extender opcodes. Keep the
    /// directory private: it holds a plugin's compiled scripts.
    Units {
        #[command(flatten)]
        lo: LoadOrderArgs,
        /// Layer 1 schema directory (schemas/fnv).
        #[arg(long)]
        schema: PathBuf,
        /// Function catalogue (default: ../script/functions.json next to --schema).
        #[arg(long)]
        functions: Option<PathBuf>,
        /// Output directory (created).
        #[arg(long)]
        dump: PathBuf,
        /// Only units of this plugin (file name, any case).
        #[arg(long)]
        plugin: Option<String>,
        /// Also write units that do have source text.
        #[arg(long)]
        all: bool,
    },
}

pub fn run(cmd: ExtCmd) -> anyhow::Result<Value> {
    match cmd {
        ExtCmd::Scan { lo, schema, functions } => {
            let fp = functions.unwrap_or_else(|| schema.join("..").join("script").join("functions.json"));
            let plugins = open_load_order(&lo)?;
            let order = LoadOrder::new(&plugins)?;
            let schema = Schema::load_dir(&schema)?;
            scan(&order, &schema, &fp)
        }
        ExtCmd::Report { lo, schema, functions, detail } => {
            let fp = functions.unwrap_or_else(|| schema.join("..").join("script").join("functions.json"));
            let ftext = std::fs::read_to_string(&fp).with_context(|| fp.display().to_string())?;
            let fjson: Value = serde_json::from_str(&ftext).with_context(|| fp.display().to_string())?;
            let table = FunctionTable::load(&fp).with_context(|| fp.display().to_string())?;
            let plugins = open_load_order(&lo)?;
            let order = LoadOrder::new(&plugins)?;
            let schema = Schema::load_dir(&schema)?;
            let refs = fnv_sim::RefTable::build(&order, &schema);
            let data = fnv_game::GameData::build(&order, &schema, table, &fjson, &refs);
            Ok(source_report(&data.source_units, order.plugins.iter().map(|p| p.name.clone()).collect(), detail))
        }
        ExtCmd::Connections { lo, schema, functions, selected_plugins: selected } => {
            let fp = functions.unwrap_or_else(|| schema.join("..").join("script").join("functions.json"));
            let fjson: Value = serde_json::from_str(&std::fs::read_to_string(&fp)?)?;
            let table = FunctionTable::load(&fp)?;
            let plugins = open_load_order(&lo)?;
            let order = LoadOrder::new(&plugins)?;
            let schema = Schema::load_dir(&schema)?;
            let only: std::collections::BTreeSet<String> = selected.iter().map(|p| p.to_ascii_lowercase()).collect();
            for name in &only {
                anyhow::ensure!(order.plugins.iter().any(|p| p.name.eq_ignore_ascii_case(name)), "Selected plugin is not in the load order");
            }
            let refs = fnv_sim::RefTable::build(&order, &schema);
            let data = fnv_game::GameData::build(&order, &schema, table, &fjson, &refs);
            let report = fnv_game::connections::check(&data, &only).map_err(anyhow::Error::msg)?;
            Ok(json!({"schema": "fnv.ext.connections/1", "plugins": report, "executed": false,
                "scope": "Compiled calls and static registrations only. Unchecked means no verdict; registration is not effect/native/mod acceptance."}))
        }
        ExtCmd::ModLog { log, previous, detail, summary } => {
            let read = |p: &Path| -> anyhow::Result<crate::mod_log_digest::Digest> {
                let f = std::fs::File::open(p).context("Cannot open explicit mod-log input")?;
                crate::mod_log_digest::Digest::read(std::io::BufReader::new(f)).map_err(anyhow::Error::msg)
            };
            let now = read(&log)?;
            // An explicitly supplied missing previous log is an error, not a clean run.
            let before = previous.as_deref().map(read).transpose()?;
            let mut doc = json!({"schema": "fnv.ext.mod_log/1", "detail": detail, "run": now.json(detail)});
            if let Some(before) = &before { doc["changes"] = now.changes(before, detail); }
            if let Some(path) = summary {
                use std::io::Write;
                let path = std::path::absolute(path)?;
                fnv_install::guard_write(&path, None)?;
                // A log-only command has no Data argument to remember at startup.
                // Check only ancestors of its explicit output, not installed profiles.
                for ancestor in path.ancestors().skip(1) {
                    if let Ok(game) = fnv_install::GameInstall::open(ancestor) {
                        fnv_install::guard_write(&path, Some(&game))?;
                    }
                }
                let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).context("Summary requires a new output file")?;
                f.write_all(serde_json::to_string_pretty(&doc)?.as_bytes())?;
            }
            Ok(doc)
        }
        ExtCmd::Probes { lo, schema, functions, probes, probes_dir, frames, risky, save } => {
            let fp = functions.unwrap_or_else(|| schema.join("..").join("script").join("functions.json"));
            let plans = probe_plans(&probes_dir.join(&probes), risky)?;
            let ftext = std::fs::read_to_string(&fp).with_context(|| fp.display().to_string())?;
            let fjson: Value = serde_json::from_str(&ftext).with_context(|| fp.display().to_string())?;
            let table = FunctionTable::load(&fp).with_context(|| fp.display().to_string())?;
            let plugins = open_load_order(&lo)?;
            let order = LoadOrder::new(&plugins)?;
            let schema = Schema::load_dir(&schema)?;
            let forms = fnv_sim::FormStore::build(&order, &schema);
            let refs = fnv_sim::RefTable::build(&order, &schema);
            let data = fnv_game::GameData::build(&order, &schema, table, &fjson, &refs);
            let start = match &save {
                Some(p) => {
                    let fos = fnv_save::fos::Fos::parse(&std::fs::read(p).with_context(|| p.display().to_string())?).with_context(|| p.display().to_string())?;
                    Some(fnv_game::ext_save::save_given(&fos, &fnv_game::ext_save::Loaded { data: &data, forms: &forms, refs: &refs }, &lo.plugins).map_err(anyhow::Error::msg)?)
                }
                None => None,
            };
            let mut lines = Vec::new();
            let mut runs = Vec::new();
            let mut given_errors = Vec::new();
            for plan in &plans {
                let mut input = plan.input(frames);
                if let Some((given, _)) = &start {
                    input["given"] = json!(given);
                    input["new_game"] = json!(true);
                }
                let input: fnv_game::runner::Input = serde_json::from_value(input).context("probe actions")?;
                match fnv_game::runner::run(&data, &forms, &refs, &input) {
                    Ok(trace) => {
                        let before = lines.len();
                        probe_lines(&trace, &mut lines);
                        runs.push(json!({"quest": plan.quest, "which": plan.which, "lines": lines.len() - before}));
                        if given_errors.is_empty() {
                            given_errors = trace["given_errors"].as_array().cloned().unwrap_or_default();
                        }
                    }
                    Err(e) => runs.push(json!({"quest": plan.quest, "which": plan.which, "error": e})),
                }
            }
            let mut doc = probes_doc(&probes, &lines);
            doc["runs"] = json!(runs);
            if let Some((_, mut notes)) = start {
                notes["given_errors"] = json!(given_errors);
                doc["save"] = notes;
            }
            Ok(doc)
        }
        ExtCmd::ScriptRunner { lo, schema, functions, roots, run, detail, no_data_files } => script_runner(&lo, &schema, functions, roots, run, detail, no_data_files),
        ExtCmd::Queries { lo, schema, functions, steps, start_cell, save, trace, ini, config } => {
            // Mods' files as `script run` sees them: the Data folder (and its archives)
            // read-only, `--config` for Data\Config; nothing is written to disk.
            let vfs = std::sync::Arc::new(fnv_vfs::Vfs::open_for_plugins(&lo.data_dir, &lo.plugins).ok());
            let v2 = vfs.clone();
            let archives: Vec<String> = vfs.as_ref().as_ref().map(|v| v.archives().map(str::to_string).collect()).unwrap_or_default();
            let lookup: fnv_game::mod_files::Lookup = std::sync::Arc::new(move |p: &str| match vfs.as_ref() {
                Some(v) if v.loose_path(p).is_some() => 1,
                Some(v) if v.exists(p) => 2,
                _ => 0,
            });
            let reader: fnv_game::mod_files::Reader = std::sync::Arc::new(move |p: &str| v2.as_ref().as_ref().and_then(|v| v.read(p)).and_then(Result::ok));
            fnv_game::mod_files::set_roots(fnv_game::mod_files::Roots { hosted_from: None, read: vec![lo.data_dir.clone()], config, write: None, lookup: Some(lookup), reader: Some(reader), archives, list: None });
            // The menus' tile tree, as the engine loads it at its start, so the UI
            // functions read the HUD's traits (ui_loading::load).
            if let Ok(v) = fnv_vfs::Vfs::open_for_plugins(&lo.data_dir, &lo.plugins) {
                let v = std::sync::Arc::new(v);
                fnv_game::ui_loading::set_headless_menus(std::sync::Arc::new(move |w: &fnv_sim::World<'_>| {
                    let files = fnv_game::ui_loading::UiFiles::new(v.clone(), w);
                    let paths = files.menu_paths();
                    Some((Box::new(files) as Box<dyn fnv_ui::Source + Send + Sync>, paths))
                }));
            }
            let fp = functions.unwrap_or_else(|| schema.join("..").join("script").join("functions.json"));
            let jtext = std::fs::read_to_string(&steps).with_context(|| steps.display().to_string())?;
            let job: Value = serde_json::from_str(&jtext).with_context(|| steps.display().to_string())?;
            let fos = match &save {
                Some(p) => Some(fnv_save::fos::Fos::parse(&std::fs::read(p).with_context(|| p.display().to_string())?).with_context(|| p.display().to_string())?),
                None => None,
            };
            let placed = fos.as_ref().and_then(|f| f.player().ok()).is_some_and(|p| p.place.is_some());
            let (set, mut input) = query_input(&job, if placed { None } else { start_cell.as_deref() })?;
            let ftext = std::fs::read_to_string(&fp).with_context(|| fp.display().to_string())?;
            let fjson: Value = serde_json::from_str(&ftext).with_context(|| fp.display().to_string())?;
            let table = FunctionTable::load(&fp).with_context(|| fp.display().to_string())?;
            let plugins = open_load_order(&lo)?;
            let order = LoadOrder::new(&plugins)?;
            let schema = Schema::load_dir(&schema)?;
            let forms = fnv_sim::FormStore::build(&order, &schema);
            let refs = fnv_sim::RefTable::build(&order, &schema);
            let mut data = fnv_game::GameData::build(&order, &schema, table, &fjson, &refs);
            for p in &ini {
                data.ini_texts.push(std::fs::read_to_string(p).with_context(|| p.display().to_string())?);
            }
            let mut save_notes = None;
            if let Some(fos) = &fos {
                let (given, notes) = fnv_game::ext_save::save_given(fos, &fnv_game::ext_save::Loaded { data: &data, forms: &forms, refs: &refs }, &lo.plugins).map_err(anyhow::Error::msg)?;
                input["given"] = json!(given);
                input["new_game"] = json!(true);
                save_notes = Some(notes);
            }
            let input: fnv_game::runner::Input = serde_json::from_value(input).context("query actions")?;
            let run = fnv_game::runner::run(&data, &forms, &refs, &input).map_err(anyhow::Error::msg)?;
            if let Some(path) = trace {
                fnv_install::guard_write(&path, None)?;
                std::fs::write(&path, serde_json::to_string(&run)?).with_context(|| path.display().to_string())?;
            }
            let mut doc = queries_doc(&set, &run);
            if let Some(mut notes) = save_notes {
                notes["given_errors"] = run["given_errors"].clone();
                doc["save"] = notes;
            }
            Ok(doc)
        }
        ExtCmd::Units { lo, schema, functions, dump, plugin, all } => {
            let fp = functions.unwrap_or_else(|| schema.join("..").join("script").join("functions.json"));
            let table = FunctionTable::load(&fp).with_context(|| fp.display().to_string())?;
            let plugins = open_load_order(&lo)?;
            let order = LoadOrder::new(&plugins)?;
            let schema = Schema::load_dir(&schema)?;
            let (units, _) = read_load_order(&order, &schema);
            dump_units(&units, &table, plugin.as_deref(), all, &dump)
        }
    }
}

/// `ext report`'s document from what the game's source compile made of each unit. Error
/// details (which may quote script text) only with `detail`.
pub(crate) fn source_report(units: &[fnv_game::source_compile::SourceUnit], order: Vec<String>, detail: bool) -> Value {
    let per: Vec<Value> = order
        .iter()
        .filter_map(|plugin| {
            let mine: Vec<&fnv_game::source_compile::SourceUnit> = units.iter().filter(|u| u.plugin.eq_ignore_ascii_case(plugin)).collect();
            if mine.is_empty() {
                return None;
            }
            let failed: Vec<Value> = mine
                .iter()
                .filter_map(|u| {
                    let e = u.error.as_ref()?;
                    let mut row = json!({"unit": u.key, "line": e.line, "kind": e.kind});
                    if detail {
                        row["detail"] = json!(e.detail);
                    }
                    Some(row)
                })
                .collect();
            let owned: Vec<fnv_game::source_compile::SourceUnit> = mine.iter().map(|u| (*u).clone()).collect();
            let unknown: Vec<Value> = fnv_game::source_compile::unknown_functions(&owned).into_iter().map(|(name, n)| json!({"name": name, "units": n})).collect();
            let enums: std::collections::BTreeSet<&str> = mine.iter().flat_map(|u| u.unknown_enums.iter().map(String::as_str)).collect();
            Some(json!({
                "plugin": plugin,
                "units_from_source": mine.len() - failed.len(),
                "units_kept_bytes": failed.len(),
                "failed": failed,
                "unknown_functions": unknown,
                "unknown_enum_names": enums,
            }))
        })
        .collect();
    let failed = units.iter().filter(|u| u.error.is_some()).count();
    json!({
        "format": "fnv.ext.report/1",
        "load_order": order,
        "summary": {
            "units_picked": units.len(),
            "units_from_source": units.len() - failed,
            "units_kept_bytes": failed,
            "unknown_functions": fnv_game::source_compile::unknown_functions(units).into_iter().map(|(name, n)| json!({"name": name, "units": n})).collect::<Vec<_>>(),
        },
        "per_plugin": per,
    })
}

/// The units `ext units` writes: with compiled bytes, without source unless `all`, of
/// `plugin` when given.
pub(crate) fn units_to_dump<'a>(units: &'a [ScriptUnit], plugin: Option<&str>, all: bool) -> Vec<&'a ScriptUnit> {
    units
        .iter()
        .filter(|u| u.has_scda && (all || u.source.is_none()))
        .filter(|u| plugin.is_none_or(|p| u.plugin.eq_ignore_ascii_case(p)))
        .collect()
}

fn dump_units(units: &[ScriptUnit], table: &FunctionTable, plugin: Option<&str>, all: bool, dir: &Path) -> anyhow::Result<Value> {
    // The folder is guarded before anything is made: the files used to be written first and
    // only units.json refused, inside the game's Data folder [#1987].
    fnv_install::guard_write(dir, None)?;
    std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    let mut index = Vec::new();
    for (n, u) in units_to_dump(units, plugin, all).into_iter().enumerate() {
        let file = format!("{n:04}.scda");
        std::fs::write(dir.join(&file), &u.bytes).with_context(|| file.clone())?;
        index.push(json!({
            "file": file,
            "key": u.key(),
            "plugin": u.plugin,
            "owner": u.owner.label(),
            "has_source": u.source.is_some(),
            "bytes": u.bytes.len(),
            "uses_extender": bytecode_uses_extender(&u.bytes, table),
            "unit": u,
        }));
    }
    let doc = json!({"format": "fnv.ext.units/1", "units": index});
    fnv_install::guard_write(&dir.join("units.json"), None)?;
    std::fs::write(dir.join("units.json"), serde_json::to_string_pretty(&doc)?).context("units.json")?;
    Ok(json!({"dir": dir.display().to_string(), "units": doc["units"].as_array().map_or(0, |a| a.len())}))
}

/// The probe set of a query job (its name with `_` as `-`, as jip_probes.py names the
/// golden) and the runner input: one `query` action per step, in order, each followed by
/// one frame, as nvf_capture runs one step per game loop (xnvse-capture.md §4); first a
/// move to `start_cell` when given.
pub(crate) fn query_input(job: &Value, start_cell: Option<&str>) -> anyhow::Result<(String, Value)> {
    let (set, steps) = fnv_game::ext_probes::query_steps(job).map_err(anyhow::Error::msg)?;
    let mut actions = Vec::new();
    if let Some(cell) = start_cell {
        actions.push(json!({"op": "go_to_cell", "cell": cell}));
    }
    for s in steps {
        actions.push(json!({"op": "query", "script": s.script, "label": s.label, "text": s.text}));
        actions.push(json!({"op": "wait_frames", "frames": 1}));
    }
    Ok((set, json!({"format": "fnv.script.actions/1", "new_game": false, "actions": actions})))
}

/// `ext script-runner`: the Script Runner files of `roots` and Data, compiled (and run).
fn script_runner(lo: &LoadOrderArgs, schema: &Path, functions: Option<PathBuf>, mut roots: Vec<PathBuf>, run: bool, detail: bool, no_data_files: bool) -> anyhow::Result<Value> {
    use fnv_game::script_runner::{Moment, SrRun};
    let fp = functions.unwrap_or_else(|| schema.join("..").join("script").join("functions.json"));
    let vfs = std::sync::Arc::new(fnv_vfs::Vfs::open_for_plugins(&lo.data_dir, &lo.plugins).ok());
    let reader: fnv_game::mod_files::Reader = std::sync::Arc::new(move |p: &str| vfs.as_ref().as_ref().and_then(|v| v.read(p)).and_then(Result::ok));
    if !no_data_files {
        roots.push(lo.data_dir.clone());
    }
    fnv_game::mod_files::set_roots(fnv_game::mod_files::Roots { hosted_from: None, read: roots, config: None, write: None, lookup: None, reader: Some(reader), archives: Vec::new(), list: None });
    let ftext = std::fs::read_to_string(&fp).with_context(|| fp.display().to_string())?;
    let fjson: Value = serde_json::from_str(&ftext).with_context(|| fp.display().to_string())?;
    let table = FunctionTable::load(&fp).with_context(|| fp.display().to_string())?;
    let plugins = open_load_order(lo)?;
    let order = LoadOrder::new(&plugins)?;
    let schema = Schema::load_dir(schema)?;
    let forms = fnv_sim::FormStore::build(&order, &schema);
    let refs = fnv_sim::RefTable::build(&order, &schema);
    let data = fnv_game::GameData::build(&order, &schema, table, &fjson, &refs);
    let world = fnv_sim::World::new(&forms, &refs, fnv_sim::SimConfig::default());
    let mut g = fnv_game::game::Game::new(&data, world, fnv_game::game::GameConfig::default());
    let rows = |runs: Vec<SrRun>| -> Vec<Value> {
        runs.into_iter()
            .map(|r| match r.result {
                Ok(()) => json!({"file": r.path, "ok": true}),
                Err(e) => json!({"file": r.path, "ok": false, "error": e}),
            })
            .collect()
    };
    let compiled = rows(g.compile_script_runner());
    let failed = compiled.iter().filter(|r| r["ok"] == false).count();
    let mut doc = json!({
        "schema": "fnv.ext.script_runner/1",
        "files": compiled.len(),
        "compiled": compiled.len() - failed,
        "failed": failed,
        "compile": compiled,
    });
    if run {
        let restart = rows(g.run_script_runner(Moment::Restart));
        g.new_game();
        let new_game = rows(g.run_script_runner(Moment::NewGame));
        let exit = rows(g.run_exit_scripts());
        doc["ran"] = json!({"restart": restart, "new_game": new_game, "exit": exit});
        doc["diagnostics"] = crate::runner_diagnostics::document(&g.events, detail);
    }
    Ok(doc)
}

/// fnv.ext.probes/1 from a run's query observations, as jip_probes.py's collector writes
/// it from the game's capture: numbers as `%.6g`, errors listed apart
/// (`fnv_game::ext_probes`, shared with the engine's query job).
pub(crate) fn queries_doc(set: &str, trace: &Value) -> Value {
    fnv_game::ext_probes::probes_doc(set, trace["observations"].as_array().into_iter().flatten(), "fnvtool ext queries")
}

/// Extender function names and aliases (lower case) → (catalogue name, origin). A word that
/// also names a base-game function is left out.
pub(crate) struct Catalogue(HashMap<String, (String, String)>);

impl Catalogue {
    pub(crate) fn load(path: &Path) -> anyhow::Result<Catalogue> {
        let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
        let doc: Value = serde_json::from_str(&text).with_context(|| path.display().to_string())?;
        Ok(Catalogue::from_json(&doc))
    }

    pub(crate) fn from_json(doc: &Value) -> Catalogue {
        let rows = doc["functions"].as_array().cloned().unwrap_or_default();
        let names = |f: &Value| {
            let mut v = vec![f["name"].as_str().unwrap_or_default().to_ascii_lowercase()];
            v.extend(f["aliases"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_ascii_lowercase));
            v
        };
        let vanilla: HashSet<String> = rows.iter().filter(|f| f["family"] != "extender").flat_map(names).collect();
        let mut map = HashMap::new();
        for f in rows.iter().filter(|f| f["family"] == "extender") {
            let entry = (f["name"].as_str().unwrap_or_default().to_string(), f["origin"].as_str().unwrap_or_default().to_string());
            for n in names(f) {
                if !vanilla.contains(&n) {
                    map.entry(n).or_insert_with(|| entry.clone());
                }
            }
        }
        Catalogue(map)
    }
}

/// What one unit's source uses.
#[derive(Default, Debug)]
pub(crate) struct UnitUse {
    pub functions: BTreeMap<String, u64>,
    pub syntax: BTreeMap<&'static str, u64>,
}

impl UnitUse {
    fn any(&self) -> bool {
        !self.functions.is_empty() || self.syntax.values().any(|&n| n > 0)
    }
}

/// A source line with its `;` comment cut and quoted text blanked.
fn code_of(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                out.push(' ');
            }
            ';' if !quoted => break,
            _ if quoted => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

fn words(code: &str) -> Vec<&str> {
    code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).collect()
}

pub(crate) fn scan_source(src: &str, locals: &HashSet<String>, cat: &Catalogue) -> UnitUse {
    let mut u = UnitUse { syntax: SYNTAX.iter().map(|k| (*k, 0)).collect(), ..Default::default() };
    let bump = |u: &mut UnitUse, k: &'static str, n: usize| *u.syntax.get_mut(k).unwrap() += n as u64;
    for line in src.lines() {
        let code = code_of(line);
        let w = words(&code);
        let lw: Vec<String> = w.iter().map(|x| x.to_ascii_lowercase()).collect();
        let first = lw.first().map(String::as_str).unwrap_or("");
        let second = lw.get(1).map(String::as_str).unwrap_or("");
        if first == "let" {
            bump(&mut u, "let", 1);
        }
        bump(&mut u, "assign", code.matches(":=").count());
        bump(&mut u, "compound", COMPOUND.iter().map(|op| code.matches(op).count()).sum());
        if (first == "if" || first == "elseif") && second == "eval" {
            bump(&mut u, "if_eval", 1);
        }
        if first == "while" {
            bump(&mut u, "while", 1);
        }
        if first == "foreach" {
            bump(&mut u, "foreach", 1);
        }
        bump(&mut u, "string_var", lw.iter().filter(|x| *x == "string_var").count());
        bump(&mut u, "array_var", lw.iter().filter(|x| *x == "array_var").count());
        if first == "begin" && second.starts_with('_') {
            bump(&mut u, "begin_override", 1);
        }
        bump(&mut u, "subscript", code.matches('[').count());
        bump(&mut u, "member", code.matches("->").count());
        for x in &lw {
            if locals.contains(x) {
                continue;
            }
            if let Some((name, _)) = cat.0.get(x) {
                *u.functions.entry(name.clone()).or_default() += 1;
            }
        }
    }
    u
}

/// Whether compiled bytes call a function past the base game's opcode range.
pub(crate) fn bytecode_uses_extender(bytes: &[u8], table: &FunctionTable) -> bool {
    let d = decode(bytes, table);
    let in_expr = |e: &Expr| e.0.iter().any(|t| matches!(t, Token::Call { call } if call.opcode >= EXTENDER_OPCODE_BASE));
    let mut hit = false;
    d.program.walk(&mut |s| {
        hit |= match s {
            Stmt::Call { call } => call.opcode >= EXTENDER_OPCODE_BASE,
            Stmt::Raw { opcode, .. } => *opcode >= EXTENDER_OPCODE_BASE,
            Stmt::Set { expr, .. } => in_expr(expr),
            Stmt::If { arms, .. } => arms.iter().any(|a| in_expr(&a.cond)),
            Stmt::Unmatched { cond, .. } => cond.as_ref().is_some_and(in_expr),
            _ => false,
        }
    });
    hit
}

#[derive(Default)]
struct PluginUse {
    units: u64,
    ext_units: u64,
    no_source: u64,
    no_source_ext: u64,
    functions: BTreeMap<String, u64>,
    syntax: BTreeMap<&'static str, u64>,
}

fn scan(lo: &LoadOrder<'_>, schema: &Schema, functions: &Path) -> anyhow::Result<Value> {
    let cat = Catalogue::load(functions)?;
    let table = FunctionTable::load(functions).with_context(|| functions.display().to_string())?;
    let (units, _) = read_load_order(lo, schema);
    Ok(report(&units, &cat, &table, lo.plugins.iter().map(|p| p.name.clone()).collect()))
}

pub(crate) fn report(units: &[ScriptUnit], cat: &Catalogue, table: &FunctionTable, order: Vec<String>) -> Value {
    let mut per: BTreeMap<String, PluginUse> = order.iter().map(|p| (p.clone(), PluginUse::default())).collect();
    // Units with neither compiled bytes nor source (empty result scripts) are not scripts.
    for u in units.iter().filter(|u| u.has_scda || u.source.is_some()) {
        let p = per.entry(u.plugin.clone()).or_default();
        p.units += 1;
        match &u.source {
            Some(src) => {
                let locals: HashSet<String> = u.variables.iter().map(|v| v.name.to_ascii_lowercase()).collect();
                let used = scan_source(src, &locals, cat);
                if used.any() {
                    p.ext_units += 1;
                }
                for (k, n) in used.functions {
                    *p.functions.entry(k).or_default() += n;
                }
                for (k, n) in used.syntax {
                    *p.syntax.entry(k).or_default() += n;
                }
            }
            None => {
                p.no_source += 1;
                if bytecode_uses_extender(&u.bytes, table) {
                    p.ext_units += 1;
                    p.no_source_ext += 1;
                }
            }
        }
    }
    let origin = |name: &str| cat.0.get(&name.to_ascii_lowercase()).map(|e| e.1.clone()).unwrap_or_default();
    let mut totals: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut syntax: BTreeMap<&str, u64> = SYNTAX.iter().map(|k| (*k, 0)).collect();
    for p in per.values() {
        for (k, n) in &p.functions {
            let t = totals.entry(k.clone()).or_default();
            t.0 += n;
            t.1 += 1;
        }
        for (k, n) in &p.syntax {
            *syntax.entry(k).or_default() += n;
        }
    }
    let mut ranked: Vec<(&String, &(u64, u64))> = totals.iter().collect();
    ranked.sort_by(|a, b| b.1 .1.cmp(&a.1 .1).then(b.1 .0.cmp(&a.1 .0)).then(a.0.cmp(b.0)));
    let sum = |f: fn(&PluginUse) -> u64| per.values().map(f).sum::<u64>();
    json!({
        "format": "fnv.ext.scan/1",
        "load_order": order,
        "summary": {
            "plugins": per.len(),
            "units": sum(|p| p.units),
            "units_with_extender_use": sum(|p| p.ext_units),
            "units_without_source": sum(|p| p.no_source),
            "units_without_source_using_extender": sum(|p| p.no_source_ext),
        },
        "functions": ranked.iter().map(|(n, (calls, plugins))| json!({"name": n, "origin": origin(n), "calls": calls, "plugins": plugins})).collect::<Vec<_>>(),
        "syntax": syntax,
        "per_plugin": per.iter().map(|(name, p)| json!({
            "plugin": name,
            "units": p.units,
            "units_with_extender_use": p.ext_units,
            "units_without_source": p.no_source,
            "units_without_source_using_extender": p.no_source_ext,
            "functions": p.functions,
            "syntax": p.syntax,
        })).collect::<Vec<_>>(),
    })
}

/// One headless run: a probe quest, and for a selector script the probe number.
pub(crate) struct ProbePlan {
    pub quest: String,
    pub which: Option<u32>,
}

impl ProbePlan {
    /// The fnv.script.actions/1 document of the run.
    fn input(&self, frames: u64) -> Value {
        let mut actions = vec![json!({"op": "start_quest", "quest": self.quest})];
        if let Some(k) = self.which {
            actions.push(json!({"op": "wait_frames", "frames": frames / 2}));
            actions.push(json!({"op": "set_quest_var", "quest": self.quest, "var": "iWhich", "value": k}));
        }
        actions.push(json!({"op": "wait_frames", "frames": frames}));
        json!({"format": "fnv.script.actions/1", "new_game": false, "actions": actions})
    }
}

/// Scripts the in-game runner also leaves out unless asked (they hang or crash the
/// original): the runaway loop and the deep recursion.
const RISKY: [&str; 2] = ["ExtProbeLoops09", "ExtProbeUdf05"];

/// The runs for a probe folder, as tools/capture/ext_probe_run.py plans them: every
/// `scn` file's quest (script name + "Q"), a selector script (one with `iWhich`) once per
/// probe number.
pub(crate) fn probe_plans(dir: &Path, risky: bool) -> anyhow::Result<Vec<ProbePlan>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| dir.display().to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).with_context(|| f.display().to_string())?;
        let Some(name) = text.lines().next().and_then(|l| l.strip_prefix("scn ")).map(str::trim) else { continue };
        if !risky && RISKY.contains(&name) {
            continue;
        }
        let quest = format!("{name}Q");
        let n = text
            .lines()
            .map(str::trim_start)
            .filter(|t| t.starts_with("if iWhich == ") || t.starts_with("elseif iWhich == "))
            .filter(|t| t.rsplit(' ').next().is_some_and(|n| n.parse::<u32>().is_ok()))
            .count() as u32;
        if n == 0 {
            out.push(ProbePlan { quest, which: None });
        } else {
            out.extend((1..=n).map(|k| ProbePlan { quest: quest.clone(), which: Some(k) }));
        }
    }
    Ok(out)
}

/// Every printed line of a run's trace that carries a probe result: any event text with
/// `EXTPROBE|` (the VM's script-print events).
pub(crate) fn probe_lines(trace: &Value, out: &mut Vec<String>) {
    match trace {
        Value::Object(m) => {
            if let Some(Value::String(t)) = m.get("text") {
                if let Some(i) = t.find("EXTPROBE|") {
                    out.push(t[i..].trim_end().to_string());
                }
            }
            m.values().for_each(|v| probe_lines(v, out));
        }
        Value::Array(a) => a.iter().for_each(|v| probe_lines(v, out)),
        _ => {}
    }
}

/// fnv.ext.probes/1 from probe lines, as tools/capture/ext_probes.py writes it: rows in
/// first-seen order (the last value of a repeated id wins), begin/end markers per script.
pub(crate) fn probes_doc(set: &str, lines: &[String]) -> Value {
    let mut order: Vec<String> = Vec::new();
    let mut values: HashMap<String, String> = HashMap::new();
    let mut dups = Vec::new();
    let mut files: BTreeMap<String, (bool, bool)> = BTreeMap::new();
    for l in lines {
        let Some(rest) = l.strip_prefix("EXTPROBE|") else { continue };
        let Some((id, value)) = rest.split_once('|') else { continue };
        if id.contains('/') && (value == "begin" || value == "end") {
            let f = files.entry(id.to_string()).or_default();
            if value == "begin" {
                f.0 = true
            } else {
                f.1 = true
            }
            continue;
        }
        match values.get(id) {
            Some(old) if old != value => dups.push(json!({"id": id, "values": [old, value]})),
            Some(_) => {}
            None => order.push(id.to_string()),
        }
        values.insert(id.to_string(), value.to_string());
    }
    json!({
        "format": "fnv.ext.probes/1",
        "set": set,
        "version": "nvforever",
        "sources": ["fnvtool ext probes"],
        "rows": order.iter().map(|id| json!({"id": id, "value": values[id]})).collect::<Vec<_>>(),
        "files": files.iter().map(|(n, (b, e))| json!({"name": n, "begun": b, "ended": e})).collect::<Vec<_>>(),
        "duplicates": dups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_esm::raw::{write_subrecords, Subrecord};
    use fnv_esm::Plugin;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
        Subrecord { sig: *sig, data, extended: false }
    }

    fn record(sig: &[u8; 4], flags: u32, fid: u32, subs: &[Subrecord]) -> Vec<u8> {
        let payload = write_subrecords(subs);
        [&sig[..], &(payload.len() as u32).to_le_bytes(), &flags.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload]
            .concat()
    }

    fn group(label: [u8; 4], body: &[u8]) -> Vec<u8> {
        [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &0i32.to_le_bytes(), &[0; 8], body].concat()
    }

    /// An object script: SCHR + SCDA + variables + optional SCTX.
    fn script(fid: u32, scda: &[u8], vars: &[(u32, &str, u8)], source: Option<&str>) -> Vec<u8> {
        let mut schr = vec![0u8; 4];
        schr.extend(0u32.to_le_bytes());
        schr.extend((scda.len() as u32).to_le_bytes());
        schr.extend((vars.len() as u32).to_le_bytes());
        schr.extend(0u16.to_le_bytes());
        schr.extend(1u16.to_le_bytes());
        let mut subs = vec![sub(b"SCHR", schr), sub(b"SCDA", scda.to_vec())];
        if let Some(s) = source {
            subs.push(sub(b"SCTX", s.as_bytes().to_vec()));
        }
        for (i, n, t) in vars {
            let mut slsd = i.to_le_bytes().to_vec();
            slsd.extend([0u8; 12]);
            slsd.push(*t);
            slsd.extend([0u8; 7]);
            subs.push(sub(b"SLSD", slsd));
            subs.push(sub(b"SCVR", [n.as_bytes(), &[0]].concat()));
        }
        record(b"SCPT", 0, fid, &subs)
    }

    const SOURCE: &str = "scn ExtScanTest
string_var s
array_var a
int Rand
Begin GameMode
    let s := \"Sv_Length in quotes\" ; Sv_Length in a comment
    let a := Ar_List 1, 2
    if eval Sv_Length s > 0
        let a[0] += 1
    endif
    let Rand := Ar_Size a
End
Begin _MenuMode
End
";

    fn plugin() -> Vec<u8> {
        let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat();
        let tes4 = record(b"TES4", 1, 0, &[sub(b"HEDR", hedr)]);
        // ScriptName, then (no source) a call statement with an extender-range opcode.
        let name = [0x1D, 0, 0, 0];
        let ext_call = [0x1D, 0, 0, 0, 0x00, 0x15, 0, 0];
        let scpt = [
            script(0x800, &name, &[(1, "s", 0), (2, "a", 0), (3, "Rand", 1)], Some(SOURCE)),
            script(0x801, &ext_call, &[], None),
            script(0x802, &name, &[], None),
        ]
        .concat();
        [tes4, group(*b"SCPT", &scpt)].concat()
    }

    #[test]
    fn counts_functions_syntax_and_units_without_source() {
        let fp = root().join("schemas/script/functions.json");
        let cat = Catalogue::load(&fp).unwrap();
        let table = FunctionTable::load(&fp).unwrap();
        let schema = Schema::load_dir(root().join("schemas/fnv")).unwrap();
        let plugins = vec![Plugin::parse(&plugin(), "ExtScan.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let (units, _) = read_load_order(&lo, &schema);
        let r = report(&units, &cat, &table, vec!["ExtScan.esm".into()]);
        assert_eq!(r["format"], "fnv.ext.scan/1");
        let s = &r["summary"];
        assert_eq!((s["units"].as_u64(), s["units_with_extender_use"].as_u64()), (Some(3), Some(2)));
        assert_eq!((s["units_without_source"].as_u64(), s["units_without_source_using_extender"].as_u64()), (Some(2), Some(1)));
        let calls = |n: &str| r["functions"].as_array().unwrap().iter().find(|f| f["name"] == n).map(|f| f["calls"].as_u64().unwrap());
        // Not counted: the quoted and commented Sv_Length, and the local variable Rand.
        assert_eq!(calls("Sv_Length"), Some(1));
        assert_eq!(calls("Ar_List"), Some(1));
        assert_eq!(calls("Ar_Size"), Some(1));
        assert_eq!(calls("Rand"), None);
        let syn = &r["syntax"];
        for (k, n) in [("let", 4), ("assign", 3), ("compound", 1), ("if_eval", 1), ("string_var", 1), ("array_var", 1), ("begin_override", 1), ("subscript", 1), ("member", 0)] {
            assert_eq!(syn[k].as_u64(), Some(n), "{k}");
        }
    }

    #[test]
    fn units_dump_writes_the_units_without_source() {
        let fp = root().join("schemas/script/functions.json");
        let table = FunctionTable::load(&fp).unwrap();
        let schema = Schema::load_dir(root().join("schemas/fnv")).unwrap();
        let plugins = vec![Plugin::parse(&plugin(), "ExtScan.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let (units, _) = read_load_order(&lo, &schema);
        assert_eq!(units_to_dump(&units, None, false).len(), 2);
        assert_eq!(units_to_dump(&units, None, true).len(), 3);
        assert_eq!(units_to_dump(&units, Some("other.esp"), false).len(), 0);
        let dir = std::env::temp_dir().join(format!("nvf-ext-units-{}", std::process::id()));
        let out = dump_units(&units, &table, Some("extscan.esm"), false, &dir).unwrap();
        assert_eq!(out["units"], 2);
        let doc: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("units.json")).unwrap()).unwrap();
        assert_eq!(doc["format"], "fnv.ext.units/1");
        let first = &doc["units"][0];
        let bytes = std::fs::read(dir.join(first["file"].as_str().unwrap())).unwrap();
        assert_eq!(bytes.len() as u64, first["bytes"].as_u64().unwrap());
        assert_eq!(doc["units"].as_array().unwrap().iter().filter(|u| u["uses_extender"] == true).count(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn probe_plans_follow_the_probe_folder() {
        let dir = std::env::temp_dir().join(format!("ext_probe_plans_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("01_a.txt"), "scn ExtProbeX01\nBegin GameMode\nEnd\n").unwrap();
        std::fs::write(dir.join("02_sel.txt"), "scn ExtProbeX02\nint iWhich\nBegin GameMode\n    if iWhich == 1\n    elseif iWhich == 2\n    endif\nEnd\n").unwrap();
        std::fs::write(dir.join("09_run.txt"), "scn ExtProbeLoops09\nBegin GameMode\nEnd\n").unwrap();
        std::fs::write(dir.join("README.md"), "not a script").unwrap();
        let plans = probe_plans(&dir, false).unwrap();
        let got: Vec<(String, Option<u32>)> = plans.iter().map(|p| (p.quest.clone(), p.which)).collect();
        assert_eq!(got, vec![("ExtProbeX01Q".into(), None), ("ExtProbeX02Q".into(), Some(1)), ("ExtProbeX02Q".into(), Some(2))]);
        assert_eq!(probe_plans(&dir, true).unwrap().len(), 4, "--risky adds the runaway loop");
        let input = plans[1].input(600);
        assert_eq!(input["actions"][1], json!({"op": "wait_frames", "frames": 300}));
        assert_eq!(input["actions"][2], json!({"op": "set_quest_var", "quest": "ExtProbeX02Q", "var": "iWhich", "value": 1}));
        let _: fnv_game::runner::Input = serde_json::from_value(input).expect("a valid actions document");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn query_jobs_become_runner_queries_and_rows() {
        let job = json!({"format": "fnv.capture.steps/1", "name": "jip_forms", "steps": [
            {"query": "Begin Function { }\nSetFunctionValue 1\nEnd\n", "label": "a", "field": "value"},
            {"query": "Begin Function { }\nSetFunctionValue (GetModName 0)\nEnd\n", "label": "b", "field": "value", "text": true},
        ]});
        let (set, input) = query_input(&job, None).unwrap();
        assert_eq!(set, "jip-forms");
        assert_eq!(input["actions"][1], json!({"op": "wait_frames", "frames": 1}));
        assert_eq!(input["actions"][2]["op"], "query");
        assert_eq!(input["actions"][2]["text"], true);
        let _: fnv_game::runner::Input = serde_json::from_value(input).expect("a valid actions document");
        assert!(query_input(&json!({"format": "fnv.capture.steps/1", "name": "x", "steps": [{"console": "tgm"}]}), None).is_err());
        let (_, moved) = query_input(&job, Some("0x000DEB16")).unwrap();
        assert_eq!(moved["actions"][0], json!({"op": "go_to_cell", "cell": "0x000DEB16"}));
        let _: fnv_game::runner::Input = serde_json::from_value(moved).expect("a valid actions document");

        let trace = json!({"observations": [
            {"label": "a", "step": 0, "value": 44.99977},
            {"label": "b", "step": 1, "value": "FalloutNV"},
            {"label": "c", "step": 2, "error": "script did not compile"},
        ]});
        let doc = queries_doc("jip-forms", &trace);
        assert_eq!(doc["rows"], json!([{"id": "a", "value": "44.9998"}, {"id": "b", "value": "FalloutNV"}]));
        assert_eq!(doc["errors"], json!([{"id": "c", "error": "script did not compile"}]));
    }

    #[test]
    fn probe_lines_and_document() {
        let trace = json!({"events": [
            {"kind": "script_print", "unit": "u", "text": "EXTPROBE|strings/S|begin"},
            {"kind": "script_print", "unit": "u", "text": "console: EXTPROBE|len.basic|5\n"},
            {"kind": "other", "text": "no probe here"},
            {"kind": "script_print", "unit": "u", "text": "EXTPROBE|replace.result|[a|b]"},
            {"kind": "script_print", "unit": "u", "text": "EXTPROBE|len.basic|6"},
        ]});
        let mut lines = Vec::new();
        probe_lines(&trace, &mut lines);
        assert_eq!(lines.len(), 4);
        let doc = probes_doc("strings", &lines);
        assert_eq!(doc["format"], "fnv.ext.probes/1");
        assert_eq!(doc["rows"], json!([{"id": "len.basic", "value": "6"}, {"id": "replace.result", "value": "[a|b]"}]));
        assert_eq!(doc["duplicates"][0]["values"], json!(["5", "6"]));
        assert_eq!(doc["files"], json!([{"name": "strings/S", "begun": true, "ended": false}]));
    }

    #[test]
    fn base_game_words_are_not_extender_functions() {
        let doc = json!({"functions": [
            {"name": "GetAV", "family": "vanilla"},
            {"name": "Sv_Length", "family": "extender", "origin": "NVSE", "aliases": ["GetAV"]},
        ]});
        let cat = Catalogue::from_json(&doc);
        assert!(cat.0.contains_key("sv_length"));
        assert!(!cat.0.contains_key("getav"));
    }
    #[test]
    fn connections_cli_filter_names_are_separate_from_positional_load_order() {
        use clap::Parser;
        for (flags, expected) in [
            (vec![], vec![]),
            (vec!["--plugin", "Missing.esp"], vec!["Missing.esp"]),
            (vec!["--plugin", "first.esm", "--plugin", "Missing.esp"], vec!["first.esm", "Missing.esp"]),
        ] {
            let mut args = vec!["fnvtool", "ext", "connections", "owned-data", "First.esm", "Second.esp", "--schema", "owned-schema"];
            args.extend(flags);
            let cli = crate::Cli::try_parse_from(args).unwrap();
            let crate::Cmd::Ext(ExtCmd::Connections { lo, selected_plugins, .. }) = cli.cmd else {
                panic!("expected the real connections command");
            };
            assert_eq!(lo.data_dir, std::path::PathBuf::from("owned-data"));
            assert_eq!(lo.plugins, vec!["First.esm", "Second.esp"]);
            assert_eq!(selected_plugins, expected);
        }
    }

    #[test]
    fn connections_walks_actual_source_compiled_units_without_executing_them() {
        let fp = root().join("schemas/script/functions.json");
        let schema = Schema::load_dir(root().join("schemas/fnv")).unwrap();
        let table = FunctionTable::load(&fp).unwrap();
        let fjson: Value = serde_json::from_str(&std::fs::read_to_string(&fp).unwrap()).unwrap();
        let source = "scn OwnedConnections\nstring_var ownedSourceSelection\nBegin GameMode\nif 0\nNvfDiagnosticOwnedMissing\nSv_Length \"owned\"\nendif\nEnd\n";
        let header = record(b"TES4", 1, 0, &[sub(b"HEDR", [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat())]);
        let bytes = [header, group(*b"SCPT", &script(0x800, &[0x1D, 0, 0, 0], &[], Some(source)))].concat();
        let plugins = vec![Plugin::parse(&bytes, "OwnedDiagnostic.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let refs = fnv_sim::RefTable::build(&lo, &schema);
        let data = fnv_game::GameData::build(&lo, &schema, table, &fjson, &refs);
        // Exercise the normal Auto source path through a real extender declaration.
        assert!(fnv_game::source_compile::declares_extender_vars(source));
        assert_eq!(data.source_units.len(), 1, "{:?}", data.source_units);
        assert_eq!(data.source_units[0].plugin, "OwnedDiagnostic.esm");
        assert!(data.source_units[0].unknown_functions.iter().any(|name| name.eq_ignore_ascii_case("NvfDiagnosticOwnedMissing")), "{:?}", data.source_units);
        assert!(data.source_units.iter().all(|u| u.error.is_none()), "{:?}", data.source_units);
        let report = fnv_game::connections::check(&data, &Default::default()).unwrap();
        let functions = &report[0].functions;
        let absent = functions.iter().find(|f| f.name.eq_ignore_ascii_case("NvfDiagnosticOwnedMissing")).unwrap();
        assert_eq!(absent.status, fnv_game::connections::Status::Unknown);
        assert_eq!((absent.units, absent.calls), (1, 1));
        let named = functions.iter().find(|f| f.name.eq_ignore_ascii_case("Sv_Length")).unwrap();
        assert_eq!(named.status, fnv_game::connections::Status::Unchecked);
        assert!(fnv_game::connections::check(&data, &std::collections::BTreeSet::from(["other.esp".into()])).unwrap().is_empty());
    }

    #[test]
    fn runner_summary_consumes_real_vm_print_and_empty_reference_failure() {
        let fp = root().join("schemas/script/functions.json");
        let schema = Schema::load_dir(root().join("schemas/fnv")).unwrap();
        let table = FunctionTable::load(&fp).unwrap();
        let fjson: Value = serde_json::from_str(&std::fs::read_to_string(&fp).unwrap()).unwrap();
        let plugins = vec![Plugin::parse(&plugin(), "OwnedDiagnostic.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let refs = fnv_sim::RefTable::build(&lo, &schema);
        let forms = fnv_sim::FormStore::build(&lo, &schema);
        let data = fnv_game::GameData::build(&lo, &schema, table, &fjson, &refs);
        let world = fnv_sim::World::new(&forms, &refs, Default::default());
        let mut game = fnv_game::Game::new(&data, world, Default::default());
        game.run_source("ref r\nPrintC \"authored-print-witness\"\nr.Disable\n", Default::default()).unwrap();
        use fnv_game::trace::EventKind;
        assert!(game.events.iter().any(|e| matches!(&e.kind, EventKind::CallFailed { error, .. } if error.contains("empty reference"))), "{:?}", game.events);
        assert!(game.events.iter().any(|e| matches!(&e.kind, EventKind::ScriptPrint { text, .. } if text.contains("authored-print-witness"))), "{:?}", game.events);
        let summary = crate::runner_diagnostics::summarize(&game.events, true).unwrap();
        assert!(summary["counts"]["call_failed"].as_u64().unwrap() > 0);
        assert!(summary["entries"].as_array().unwrap().iter().any(|r| r["kind"] == "printed" && r["words"].as_str().unwrap().contains("authored-print-witness")));
    }

    #[test]
    fn script_runner_compiles_and_runs_gx_exit_stage() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let base = std::env::temp_dir().join(format!("nvf-ext-sr-gx-{}-{stamp}", std::process::id()));
        let data = base.join("Data");
        let mods = base.join("mods");
        let scripts = mods.join("NVSE/Plugins/Scripts");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(scripts.join("optional")).unwrap();
        std::fs::write(data.join("OwnedSR.esm"), plugin()).unwrap();
        std::fs::write(scripts.join("gr_start.txt"), "RegisterSRScript \"optional\\gx_late.txt\"\n").unwrap();
        std::fs::write(scripts.join("gx_flush.txt"), "PrintC \"gx-exit-witness\"\n").unwrap();
        std::fs::write(scripts.join("optional/gx_late.txt"), "PrintC \"gx-late-witness\"\n").unwrap();

        let lo = LoadOrderArgs {
            data_dir: data.clone(),
            plugins: vec!["OwnedSR.esm".into()],
        };
        let doc = script_runner(&lo, &root().join("schemas/fnv"), None, vec![mods.clone()], true, true, true).unwrap();
        assert_eq!(doc["schema"], "fnv.ext.script_runner/1");
        assert_eq!(doc["files"], 2);
        assert_eq!(doc["compiled"], 2);
        assert_eq!(doc["failed"], 0);
        assert_eq!(doc["ran"]["restart"], json!([{"file": "gr_start.txt", "ok": true}]));
        assert_eq!(doc["ran"]["new_game"], json!([]));
        assert_eq!(
            doc["ran"]["exit"],
            json!([{"file": "gx_flush.txt", "ok": true}, {"file": "optional\\gx_late.txt", "ok": true}])
        );
        let entries = doc["diagnostics"]["entries"].as_array().unwrap();
        assert!(entries.iter().any(|r| r["kind"] == "printed" && r["words"].as_str().unwrap().contains("gx-exit-witness")));
        assert!(entries.iter().any(|r| r["kind"] == "printed" && r["words"].as_str().unwrap().contains("gx-late-witness")));

        // Absent gx_ files: ran.exit stays empty.
        std::fs::remove_file(scripts.join("gr_start.txt")).unwrap();
        std::fs::remove_file(scripts.join("gx_flush.txt")).unwrap();
        let empty = script_runner(&lo, &root().join("schemas/fnv"), None, vec![mods], true, false, true).unwrap();
        assert_eq!(empty["files"], 0);
        assert_eq!(empty["ran"]["exit"], json!([]));

        std::fs::remove_dir_all(&base).ok();
    }
}
