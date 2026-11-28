//! `fnvtool script …`: compiled scripts (crate `fnv-script`). Output contracts:
//! docs/spec-notes/scripts.md §3.4.

use crate::esm_cmd::open_load_order;
use crate::LoadOrderArgs;
use anyhow::{bail, Context};
use clap::{Args, Subcommand};
use fnv_esm::{LoadOrder, Schema};
use fnv_script::decompile::Decompiler;
use fnv_script::resolve::{ScriptIndex, Viewer};
use fnv_script::unit::{read_load_order, ReadIssues, RefSlot};
use fnv_script::{check, decode, Arg, Decoded, FunctionTable, ScriptUnit, Stmt, Token, VarLetter, VarSlot, VarType};
use rayon::prelude::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum ScriptCmd {
    /// Every script unit with its owner and sizes (fnv.script.units/1).
    Units {
        #[command(flatten)]
        a: ScriptArgs,
    },
    /// Decode units to the IR, with statistics over all units (fnv.script.decode/1).
    Decode {
        #[command(flatten)]
        a: ScriptArgs,
        #[command(flatten)]
        sel: Select,
        /// List the IR of every unit (large); by default only selected units are listed.
        #[arg(long)]
        all: bool,
    },
    /// Canonical text of every unit, or of the selected ones (fnv.script.decompile/1).
    Decompile {
        #[command(flatten)]
        a: ScriptArgs,
        #[command(flatten)]
        sel: Select,
    },
    /// Compile every unit's source text and compare the result with its compiled bytes
    /// (fnv.script.compile/1; docs/spec-notes/script-compiler.md).
    Compile {
        #[command(flatten)]
        a: ScriptArgs,
        #[command(flatten)]
        sel: Select,
        /// Start from empty variable and reference lists and resolve every name through
        /// the load order. By default each unit's own lists seed the compiler, as when the
        /// editor recompiles a script.
        #[arg(long)]
        fresh: bool,
        /// List every unit with the SHA-256 of its compiled bytes (the candidate of
        /// L5.script.compile.vanilla).
        #[arg(long)]
        digests: bool,
    },
    /// Per-opcode counts: calls decoded, raw parameters, implemented (fnv.script.coverage/1).
    Coverage {
        #[command(flatten)]
        a: ScriptArgs,
    },
    /// Every stubbed function with why it is one and its use in the load order's scripts,
    /// and the functions the scripts use that are not registered at all
    /// (fnv.script.stubs/1; docs/design/script-stubs.md).
    Stubs {
        #[command(flatten)]
        a: ScriptArgs,
    },
    /// Per quest, the functions its scripts call and its conditions test, by the intro
    /// chain's rule (the quest's scripts, stages, objectives and dialogue lines and one
    /// level of what they name; fnv_game::intro::quest_scope), for the playthrough
    /// blockers of docs/design/script-function-coverage.md (fnv.script.uses/1).
    Uses {
        #[command(flatten)]
        a: ScriptArgs,
    },
    /// Run scripts headless from the new-game state through a fnv.script.actions/1 file
    /// and print the trace (fnv.script.trace/1; docs/spec-notes/layer5-captures.md §4,
    /// scripts.md §8).
    Run {
        #[command(flatten)]
        a: ScriptArgs,
        /// Actions file (JSON, fnv.script.actions/1).
        #[arg(long)]
        actions: PathBuf,
        /// Keep every dialogue line at the length fitted to its text. By default a line
        /// lasts as long as its voice files, read from the archives next to the plugins;
        /// a line without a file keeps the fitted length (docs/spec-notes/voice.md §8).
        /// The environment variable FNVTOOL_VOICE_LENGTHS=0 does the same.
        #[arg(long)]
        no_voice_lengths: bool,
        /// The game's INI files the extender's INI functions read (GetNumericINISetting
        /// …), in order, later ones overriding earlier ones; only read.
        #[arg(long)]
        ini: Vec<PathBuf>,
        /// A folder that stands for Data\Config: the INI files mods read with LU,
        /// ShowOff and MCM functions (fnv_game::mod_files). Writes stay in memory.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Also write what the run reached of the scripts (quest stages set, script
        /// blocks run, dialogue lines said, result scripts run) to this file, a
        /// fnv.script.coverage-log/1 document (docs/handoff/mq-walkthrough-2026-10-27.md §5).
        #[arg(long)]
        coverage: Option<PathBuf>,
    },
    /// Run every quest stage, dialogue line, terminal and package result script of the
    /// load order once, headless and by itself, and group the script errors and unknown
    /// functions met by cause (fnv.script.sweep/1; docs/handoff/mq-walkthrough-2026-10-27.md
    /// §7). Prints the totals and the commonest causes of each kind.
    Sweep {
        #[command(flatten)]
        a: ScriptArgs,
        /// How many causes of each kind the output lists (the rest in `--detail`).
        #[arg(long, default_value_t = 12)]
        top: usize,
        /// Write every cause with up to five of its scripts here.
        #[arg(long)]
        detail: Option<PathBuf>,
        /// Write the scripts run as a coverage log (fnv.script.coverage-log/1) here, for
        /// `script cover --logs`.
        #[arg(long)]
        log: Option<PathBuf>,
    },
    /// Run each action chain twice, as it is and with a save and load (fnv-save) before
    /// its middle action, and compare the quests at the end and the observations
    /// (fnv.script.roundtrip/1; docs/handoff/mq-walkthrough-2026-10-27.md §6).
    Roundtrip {
        #[command(flatten)]
        a: ScriptArgs,
        /// Action files (fnv.script.actions/1) or folders of `*.actions.json` to run.
        #[arg(long, num_args = 1..)]
        actions: Vec<PathBuf>,
    },
    /// Run action chains with coverage recorded, merge their coverage with any saved
    /// coverage logs, and compare it with the load order's quests, scripts and dialogue
    /// lines (fnv.script.coverage/1; docs/handoff/mq-walkthrough-2026-10-27.md §5). Prints
    /// the totals, each chain's step errors and the untouched quests ranked main quest,
    /// factions, add-ons, the rest; the per-quest, per-script and per-actor detail goes to
    /// `--detail`.
    Cover {
        #[command(flatten)]
        a: ScriptArgs,
        /// Action files (fnv.script.actions/1) or folders of `*.actions.json` to run.
        #[arg(long, num_args = 1..)]
        actions: Vec<PathBuf>,
        /// Saved coverage logs (fnv.script.coverage-log/1) to merge in.
        #[arg(long, num_args = 1..)]
        logs: Vec<PathBuf>,
        /// Write the full report (every quest, script and actor) here.
        #[arg(long)]
        detail: Option<PathBuf>,
        /// Write each chain's coverage log into this folder (`<chain>.coverage.json`).
        #[arg(long)]
        log_dir: Option<PathBuf>,
        /// How many untouched quests the summary lists.
        #[arg(long, default_value_t = 40)]
        untouched: usize,
    },
}

#[derive(Args)]
pub struct ScriptArgs {
    #[command(flatten)]
    pub lo: LoadOrderArgs,
    /// Layer 1 schema directory (schemas/fnv).
    #[arg(long)]
    pub schema: PathBuf,
    /// Function catalogue (default: ../script/functions.json next to --schema).
    #[arg(long)]
    pub functions: Option<PathBuf>,
}

#[derive(Args)]
pub struct Select {
    /// One unit by key (`<plugin>|<owner label>`, as `units` prints it).
    #[arg(long)]
    pub unit: Option<String>,
    /// Every unit of one owner record, by load-order FormID (hex).
    #[arg(long, value_parser = crate::parse_formid)]
    pub form: Option<u32>,
}

impl Select {
    fn any(&self) -> bool {
        self.unit.is_some() || self.form.is_some()
    }
    fn matches(&self, u: &ScriptUnit) -> bool {
        self.unit.as_ref().is_none_or(|k| *k == u.key()) && self.form.is_none_or(|f| f == u.owner.form())
    }
}

struct Loaded {
    units: Vec<ScriptUnit>,
    issues: ReadIssues,
    table: FunctionTable,
    load_order: Vec<String>,
}

pub(crate) fn table_path(a: &ScriptArgs) -> PathBuf {
    a.functions.clone().unwrap_or_else(|| a.schema.join("..").join("script").join("functions.json"))
}

pub fn run(cmd: ScriptCmd) -> anyhow::Result<Value> {
    match cmd {
        ScriptCmd::Units { a } => with_load_order(&a, |l, _, _| Ok(units(l))),
        ScriptCmd::Decode { a, sel, all } => with_load_order(&a, |l, lo, schema| {
            let names = ScriptIndex::build(lo, schema, &l.units);
            decode_all(l, &names, &sel, all)
        }),
        ScriptCmd::Decompile { a, sel } => with_load_order(&a, |l, lo, schema| {
            let names = ScriptIndex::build(lo, schema, &l.units);
            decompile_all(l, &names, &sel)
        }),
        ScriptCmd::Compile { a, sel, fresh, digests } => with_load_order(&a, |l, lo, schema| {
            let names = ScriptIndex::build(lo, schema, &l.units);
            compile_all(l, lo, &names, &sel, fresh, digests)
        }),
        ScriptCmd::Coverage { a } => {
            let fjson = functions_json(&a)?;
            with_load_order(&a, |l, lo, schema| Ok(coverage(l, lo, schema, &fjson)))
        }
        ScriptCmd::Stubs { a } => {
            let fjson = functions_json(&a)?;
            with_load_order(&a, |l, lo, schema| Ok(stubs(l, lo, schema, &fjson)))
        }
        ScriptCmd::Uses { a } => {
            let fjson = functions_json(&a)?;
            with_load_order(&a, |l, lo, schema| Ok(uses(l, lo, schema, &fjson)))
        }
        ScriptCmd::Run { a, actions, no_voice_lengths, ini, config, coverage } => {
            // Mods' files: the Data folder (and its archives) read-only, `--config` before
            // it; nothing is written to disk.
            let vfs = std::sync::Arc::new(fnv_vfs::Vfs::open_for_plugins(&a.lo.data_dir, &a.lo.plugins).ok());
            let v2 = vfs.clone();
            let archives: Vec<String> = vfs.as_ref().as_ref().map(|v| v.archives().map(str::to_string).collect()).unwrap_or_default();
            let lookup: fnv_game::mod_files::Lookup = std::sync::Arc::new(move |p: &str| match vfs.as_ref() {
                Some(v) if v.loose_path(p).is_some() => 1,
                Some(v) if v.exists(p) => 2,
                _ => 0,
            });
            let reader: fnv_game::mod_files::Reader = std::sync::Arc::new(move |p: &str| v2.as_ref().as_ref().and_then(|v| v.read(p)).and_then(Result::ok));
            fnv_game::mod_files::set_roots(fnv_game::mod_files::Roots { hosted_from: None, read: vec![a.lo.data_dir.clone()], config: config.clone(), write: None, lookup: Some(lookup), reader: Some(reader), archives, list: None });
            weather_hook(&a);
            run_actions(&a, &actions, !no_voice_lengths, &ini, coverage.as_deref())
        }
        ScriptCmd::Roundtrip { a, actions } => roundtrip(&a, &actions),
        ScriptCmd::Sweep { a, top, detail, log } => sweep(&a, top, detail.as_deref(), log.as_deref()),
        ScriptCmd::Cover { a, actions, logs, detail, log_dir, untouched } => {
            cover(&a, &actions, &logs, detail.as_deref(), log_dir.as_deref(), untouched)
        }
    }
}

fn with_load_order(
    a: &ScriptArgs,
    f: impl FnOnce(&Loaded, &LoadOrder<'_>, &Schema) -> anyhow::Result<Value>,
) -> anyhow::Result<Value> {
    let tp = table_path(a);
    let table = FunctionTable::load(&tp).with_context(|| tp.display().to_string())?;
    let plugins = open_load_order(&a.lo)?;
    let lo = LoadOrder::new(&plugins)?;
    let schema = Schema::load_dir(&a.schema)?;
    let (units, issues) = read_load_order(&lo, &schema);
    let l = Loaded { units, issues, table, load_order: plugins.iter().map(|p| p.name.clone()).collect() };
    f(&l, &lo, &schema)
}

fn has_text(u: &ScriptUnit) -> bool {
    u.has_scda || u.source.is_some()
}

fn units(l: &Loaded) -> Value {
    let mut by_sig: BTreeMap<String, [u64; 4]> = BTreeMap::new();
    let rows: Vec<Value> = l
        .units
        .iter()
        .map(|u| {
            let c = by_sig.entry(u.owner.sig().to_string()).or_default();
            c[0] += 1;
            c[1] += u.has_scda as u64;
            c[2] += u.source.is_some() as u64;
            c[3] += (u.has_scda && u.source.is_some()) as u64;
            json!({
                "unit": u.key(),
                "plugin": u.plugin,
                "owner": u.owner,
                "kind": u.kind,
                "has_scda": u.has_scda,
                "has_sctx": u.source.is_some(),
                "compiled_bytes": u.bytes.len(),
                "source_bytes": u.source.as_ref().map(|s| s.len()),
                "variables": u.variables.len(),
                "refs": u.refs.len(),
                "header": u.header,
            })
        })
        .collect();
    // SCHR counters against what the unit carries (docs/spec-notes/scripts.md §1.2).
    let mut header: BTreeMap<&str, u64> = BTreeMap::new();
    for u in &l.units {
        let n = u.variables.len() as u32;
        let max = u.variables.iter().map(|v| v.index).max().unwrap_or(0);
        let vc = u.header.variable_count;
        let k = if vc == n && vc == max {
            "variable_count_equals_both"
        } else if vc == n {
            "variable_count_equals_declarations"
        } else if vc == max {
            "variable_count_equals_highest_index"
        } else {
            "variable_count_other"
        };
        *header.entry(k).or_default() += 1;
        let r = if u.header.ref_count as usize == u.refs.len() { "ref_count_equal" } else { "ref_count_differs" };
        *header.entry(r).or_default() += 1;
        if u.has_scda {
            let c = if u.header.compiled_size as usize == u.bytes.len() { "compiled_size_equal" } else { "compiled_size_differs" };
            *header.entry(c).or_default() += 1;
        } else if u.header.compiled_size != 0 {
            *header.entry("compiled_size_without_scda").or_default() += 1;
        }
    }
    let summary: BTreeMap<String, Value> = by_sig
        .into_iter()
        .map(|(k, c)| (k, json!({"units": c[0], "with_scda": c[1], "with_sctx": c[2], "with_both": c[3]})))
        .collect();
    json!({
        "format": "fnv.script.units/1",
        "load_order": l.load_order,
        "count": rows.len(),
        "with_both": l.units.iter().filter(|u| u.has_scda && u.source.is_some()).count(),
        "by_owner": summary,
        "header_checks": header,
        "read_issues": l.issues,
        "units": rows,
    })
}

fn arg_kind(a: &Arg) -> &'static str {
    match a {
        Arg::Int(_) => "int",
        Arg::Float(_) => "float",
        Arg::Ref(_) => "ref",
        Arg::Var(v) => match (v.on, v.var) {
            (_, VarSlot::Global { .. }) => "var_global",
            (Some(_), _) => "var_remote",
            (None, _) => "var_local",
        },
        Arg::Enum(_) => "enum",
        Arg::Axis(_) => "axis",
        Arg::String(_) => "string",
        Arg::FormatVars(_) => "format_vars",
        Arg::Int32(_) => "int32",
        Arg::Raw(_) => "raw",
    }
}

#[derive(Default)]
struct Stats {
    statements: BTreeMap<&'static str, u64>,
    args: BTreeMap<&'static str, u64>,
    tokens: BTreeMap<&'static str, u64>,
    /// opcode → [statement calls, expression calls, calls with a raw parameter]
    calls: BTreeMap<u16, [u64; 3]>,
}

impl Stats {
    fn call(&mut self, c: &fnv_script::Call, in_expr: bool) {
        let e = self.calls.entry(c.opcode).or_default();
        e[in_expr as usize] += 1;
        let mut raw = false;
        let mut stack: Vec<&Arg> = c.args.iter().collect();
        while let Some(a) = stack.pop() {
            *self.args.entry(arg_kind(a)).or_default() += 1;
            raw |= matches!(a, Arg::Raw(_));
            if let Arg::FormatVars(v) = a {
                stack.extend(v.iter());
            }
        }
        if raw {
            e[2] += 1;
        }
    }
    fn expr(&mut self, e: &fnv_script::Expr) {
        for t in &e.0 {
            let k = match t {
                Token::Num { .. } => "num",
                Token::Var { var } => match (var.on, var.var) {
                    (_, VarSlot::Global { .. }) => "var_global",
                    (Some(_), _) => "var_remote",
                    (None, _) => "var_local",
                },
                Token::Ref { .. } => "ref",
                Token::Call { call } => {
                    self.call(call, true);
                    "call"
                }
                Token::Op { .. } => "op",
                Token::X { .. } => "extended",
                Token::Raw { .. } => "raw",
            };
            *self.tokens.entry(k).or_default() += 1;
        }
    }
    fn program(&mut self, d: &Decoded) {
        let p = &d.program;
        if p.script_name {
            *self.statements.entry("script_name").or_default() += 1;
        }
        for b in &p.blocks {
            *self.statements.entry("begin").or_default() += 1;
            *self.statements.entry("end").or_default() += 1;
            for a in b.params.iter().flatten() {
                *self.args.entry(arg_kind(a)).or_default() += 1;
            }
        }
        let mut st: Vec<&Stmt> = Vec::new();
        p.walk(&mut |s| st.push(s));
        for s in st {
            let k = match s {
                Stmt::Set { expr, .. } => {
                    self.expr(expr);
                    "set"
                }
                Stmt::If { arms, otherwise } => {
                    for a in arms {
                        self.expr(&a.cond);
                    }
                    *self.statements.entry("elseif").or_default() += arms.len() as u64 - 1;
                    *self.statements.entry("else").or_default() += otherwise.is_some() as u64;
                    *self.statements.entry("endif").or_default() += 1;
                    "if"
                }
                Stmt::Return => "return",
                Stmt::Call { call } => {
                    self.call(call, false);
                    "call"
                }
                Stmt::Unmatched { marker, cond, .. } => {
                    if let Some(c) = cond {
                        self.expr(c);
                    }
                    match marker {
                        fnv_script::Marker::Else => "unmatched_else",
                        fnv_script::Marker::ElseIf => "unmatched_elseif",
                        fnv_script::Marker::EndIf => "unmatched_endif",
                    }
                }
                Stmt::Let { .. } => "let",
                Stmt::Eval { .. } => "eval",
                Stmt::While { .. } => "while",
                Stmt::ForEach { .. } => "foreach",
                Stmt::Break => "break",
                Stmt::Label { .. } => "label",
                Stmt::GoTo { .. } => "goto",
                Stmt::Continue => "continue",
                Stmt::Raw { .. } => "raw",
            };
            *self.statements.entry(k).or_default() += 1;
        }
    }
}

struct Done<'a> {
    unit: &'a ScriptUnit,
    decoded: Decoded,
    checks: Vec<fnv_script::Issue>,
}

fn decode_units(l: &Loaded) -> Vec<Done<'_>> {
    l.units
        .par_iter()
        .filter(|u| has_text(u))
        .map(|u| {
            let decoded = decode(&u.bytes, &l.table);
            let checks = check::check(u, &decoded.program);
            Done { unit: u, decoded, checks }
        })
        .collect()
}

#[derive(Default)]
struct IssueTally(BTreeMap<String, (u64, Vec<String>)>);

impl IssueTally {
    fn add(&mut self, kind: &str, sample: impl FnOnce() -> String) {
        let e = self.0.entry(kind.to_string()).or_default();
        e.0 += 1;
        if e.1.len() < 8 {
            e.1.push(sample());
        }
    }
    fn json(&self) -> Value {
        Value::Array(
            self.0.iter().map(|(k, (n, s))| json!({"kind": k, "count": n, "samples": s})).collect(),
        )
    }
}

fn decode_all(l: &Loaded, names: &ScriptIndex, sel: &Select, all: bool) -> anyhow::Result<Value> {
    let done = decode_units(l);
    let mut stats = Stats::default();
    let mut issues = IssueTally::default();
    let mut checks = IssueTally::default();
    let (mut statements, mut raw, mut with_raw, mut unresolved_remote) = (0usize, 0usize, 0usize, 0usize);
    let mut rows = Vec::new();
    for d in &done {
        stats.program(&d.decoded);
        statements += d.decoded.report.statements;
        raw += d.decoded.report.raw_total();
        with_raw += (d.decoded.report.raw_total() > 0) as usize;
        for i in &d.decoded.report.issues {
            issues.add(i.kind, || format!("{} #{}: {}", d.unit.key(), i.statement, i.detail));
        }
        for i in &d.checks {
            checks.add(i.kind, || format!("{}: {}", d.unit.key(), i.detail));
        }
        let (vars, _) = check::uses(&d.decoded.program);
        let viewer = names.viewer_of(d.unit);
        for v in vars {
            if let (Some(on), VarSlot::Local { index, letter }) = (v.on, v.var) {
                let target = match d.unit.slot(on) {
                    Some(RefSlot::Form(f)) => Some(f),
                    _ => None,
                };
                let seen = target.and_then(|f| names.remote_var(f, index as u32, viewer));
                let runs = target.and_then(|f| names.remote_var(f, index as u32, Viewer::LoadOrder));
                let what = || format!("{}: ref {} var {index}", d.unit.key(), on.0);
                match seen {
                    None => {
                        unresolved_remote += 1;
                        let kind = if target.is_none() { "remote_variable_on_local_ref" } else { "remote_variable_unresolved" };
                        checks.add(kind, what);
                    }
                    Some(decl) => {
                        let ok = matches!(
                            (letter, &decl.ty),
                            (VarLetter::Short, VarType::Int) | (VarLetter::Float, VarType::FloatOrRef)
                        );
                        if !ok {
                            checks.add("remote_variable_letter_mismatch", || format!("{} ({} {:?})", what(), decl.name, decl.ty));
                        }
                    }
                }
                if let (Some(a), Some(b)) = (seen, runs) {
                    if a.name != b.name || a.ty != b.ty {
                        checks.add("remote_variable_overridden", || format!("{} ({} -> {})", what(), a.name, b.name));
                    }
                }
            }
        }
        if all || (sel.any() && sel.matches(d.unit)) {
            rows.push(json!({
                "unit": d.unit.key(),
                "owner": d.unit.owner,
                "kind": d.unit.kind,
                "variables": d.unit.variables,
                "refs": d.unit.refs,
                "program": d.decoded.program,
                "report": d.decoded.report,
                "checks": d.checks,
            }));
        }
    }
    if sel.any() && rows.is_empty() {
        bail!("no unit matches the selection");
    }
    let calls: Vec<Value> = stats
        .calls
        .iter()
        .map(|(op, c)| {
            json!({"opcode": format!("0x{op:04X}"), "name": l.table.get(*op).map(|f| f.name.clone()),
                   "statement_calls": c[0], "expression_calls": c[1], "raw": c[2]})
        })
        .collect();
    Ok(json!({
        "format": "fnv.script.decode/1",
        "load_order": l.load_order,
        "summary": {
            "units": done.len(),
            "units_with_both": done.iter().filter(|d| d.unit.has_scda && d.unit.source.is_some()).count(),
            "statements": statements,
            "raw": raw,
            "units_with_raw": with_raw,
            "remote_variables_unresolved": unresolved_remote,
            "statements_by_kind": stats.statements,
            "args_by_kind": stats.args,
            "tokens_by_kind": stats.tokens,
        },
        "issues": issues.json(),
        "checks": checks.json(),
        "read_issues": l.issues,
        "calls": calls,
        "units": rows,
    }))
}

fn decompile_all(l: &Loaded, names: &ScriptIndex, sel: &Select) -> anyhow::Result<Value> {
    let done = decode_units(l);
    let rows: Vec<Value> = done
        .iter()
        .filter(|d| sel.matches(d.unit))
        .map(|d| {
            let text = Decompiler { unit: d.unit, table: &l.table, names }.lines(&d.decoded.program);
            json!({"unit": d.unit.key(), "text": text})
        })
        .collect();
    if sel.any() && rows.is_empty() {
        bail!("no unit matches the selection");
    }
    Ok(json!({
        "format": "fnv.script.decompile/1",
        "load_order": l.load_order,
        "count": rows.len(),
        "units": rows,
    }))
}

/// Mismatches of one kind: a count and a few samples.
#[derive(Default)]
struct Kinds(BTreeMap<String, (u64, Vec<Value>)>);

impl Kinds {
    fn add(&mut self, kind: String, sample: impl FnOnce() -> Value) {
        let e = self.0.entry(kind).or_default();
        e.0 += 1;
        if e.1.len() < 25 {
            e.1.push(sample());
        }
    }
    fn json(&self) -> Value {
        let mut v: Vec<(&String, &(u64, Vec<Value>))> = self.0.iter().collect();
        v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(b.0)));
        Value::Array(v.into_iter().map(|(k, (n, s))| json!({"kind": k, "count": n, "samples": s})).collect())
    }
}

/// What a differing line is about: its statement word, and the function for a call.
fn line_kind(line: &str) -> String {
    let mut w = line.split_whitespace();
    let first = w.next().unwrap_or("");
    match first {
        "set" | "if" | "elseif" => {
            let call = line.split_whitespace().find_map(|t| t.split_once('(').map(|(f, _)| f.rsplit('.').next().unwrap_or(f)));
            match call {
                Some(f) => format!("{first} {f}"),
                None => first.to_string(),
            }
        }
        "begin" => format!("begin {}", w.next().unwrap_or("")),
        f => format!("call {}", f.rsplit('.').next().unwrap_or(f)),
    }
}

fn compile_all(
    l: &Loaded,
    lo: &LoadOrder<'_>,
    names: &ScriptIndex,
    sel: &Select,
    fresh: bool,
    digests: bool,
) -> anyhow::Result<Value> {
    use fnv_script::compile::{compile, env::LoadOrderIndex, Seed};
    let ix = LoadOrderIndex::build(lo, &l.units);
    struct Row {
        key: String,
        sha256: String,
        bytes_equal: bool,
        text_equal: bool,
        refs_equal: bool,
        refs_same_set: bool,
        vars_equal: bool,
        vars_same_set: bool,
        errors: Vec<Value>,
        diff: Option<(usize, String, String)>,
        detail: Option<Value>,
    }
    let rows: Vec<Row> = l
        .units
        .par_iter()
        .filter(|u| u.has_scda && u.source.is_some() && sel.matches(u))
        .map(|u| {
            let pi = ix.plugin_index(&u.plugin).unwrap_or(0);
            let env = ix.env(pi);
            let seed = (!fresh).then_some(Seed { variables: &u.variables, refs: &u.refs, unknown_calls: false });
            let c = compile(u.source.as_deref().unwrap_or(""), &l.table, &env, seed);
            let ours_unit = ScriptUnit { variables: c.variables.clone(), refs: c.refs.clone(), bytes: c.bytes.clone(), ..u.clone() };
            let ours = decode(&c.bytes, &l.table);
            let theirs = decode(&u.bytes, &l.table);
            let a = Decompiler { unit: &ours_unit, table: &l.table, names }.lines(&ours.program);
            let b = Decompiler { unit: u, table: &l.table, names }.lines(&theirs.program);
            let diff = (a != b).then(|| {
                let i = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
                (i, a.get(i).cloned().unwrap_or_default(), b.get(i).cloned().unwrap_or_default())
            });
            let errors: Vec<Value> =
                c.errors().map(|d| json!({"line": d.line, "kind": d.kind, "detail": d.detail})).collect();
            let detail = sel.any().then(|| {
                json!({
                    "diags": c.diags.iter().map(|d| json!({"line": d.line, "kind": d.kind, "error": d.error, "detail": d.detail})).collect::<Vec<_>>(),
                    "ours": {"text": a, "bytes": fnv_util::sha256::to_hex(&c.bytes), "refs": c.refs, "variables": c.variables},
                    "shipped": {"text": b, "bytes": fnv_util::sha256::to_hex(&u.bytes), "refs": u.refs, "variables": u.variables},
                })
            });
            Row {
                key: u.key(),
                sha256: fnv_util::sha256::hex(&c.bytes),
                bytes_equal: c.bytes == u.bytes,
                text_equal: diff.is_none(),
                refs_equal: c.refs == u.refs,
                refs_same_set: {
                    let key = |r: &RefSlot| format!("{r:?}");
                    let mut x: Vec<String> = c.refs.iter().map(key).collect();
                    let mut y: Vec<String> = u.refs.iter().map(key).collect();
                    x.sort();
                    y.sort();
                    x == y
                },
                vars_equal: c.variables == u.variables,
                vars_same_set: {
                    let key = |v: &fnv_script::VarDecl| (v.name.to_ascii_lowercase(), format!("{:?}", v.ty));
                    let mut x: Vec<_> = c.variables.iter().map(key).collect();
                    let mut y: Vec<_> = u.variables.iter().map(key).collect();
                    x.sort();
                    y.sort();
                    x == y
                },
                errors,
                diff,
                detail,
            }
        })
        .collect();
    if sel.any() && rows.is_empty() {
        bail!("no unit with source and compiled bytes matches the selection");
    }
    let mut text_kinds = Kinds::default();
    let mut byte_only = Kinds::default();
    let mut error_kinds = Kinds::default();
    for r in &rows {
        if let Some((i, ours, theirs)) = &r.diff {
            text_kinds.add(line_kind(if theirs.is_empty() { ours } else { theirs }), || {
                json!({"unit": r.key, "line": i, "ours": ours, "shipped": theirs})
            });
        } else if !r.bytes_equal && !fresh {
            byte_only.add("bytes".into(), || json!({"unit": r.key}));
        }
        for e in &r.errors {
            let k = e["kind"].as_str().unwrap_or("").to_string();
            error_kinds.add(k, || json!({"unit": r.key, "error": e}));
        }
    }
    let n = |f: &dyn Fn(&Row) -> bool| rows.iter().filter(|r| f(r)).count();
    let mut out = json!({
        "format": "fnv.script.compile/1",
        "load_order": l.load_order,
        "mode": if fresh { "fresh" } else { "seeded" },
        "summary": {
            "units": rows.len(),
            "bytes_equal": n(&|r| r.bytes_equal),
            "text_equal": n(&|r| r.text_equal),
            "refs_equal": n(&|r| r.refs_equal),
            "refs_same_set": n(&|r| r.refs_same_set),
            "variables_equal": n(&|r| r.vars_equal),
            "variables_same_set": n(&|r| r.vars_same_set),
            "units_with_errors": n(&|r| !r.errors.is_empty()),
        },
        "text_differs": text_kinds.json(),
        "bytes_differ_text_equal": byte_only.json(),
        "errors": error_kinds.json(),
    });
    if sel.any() {
        out["units"] = Value::Array(rows.iter().map(|r| json!({"unit": r.key, "detail": r.detail})).collect());
    } else if digests {
        out["count"] = json!(rows.len());
        out["units"] = Value::Array(rows.iter().map(|r| json!({"unit": r.key, "sha256": r.sha256})).collect());
    }
    Ok(out)
}

fn functions_json(a: &ScriptArgs) -> anyhow::Result<Value> {
    let tp = table_path(a);
    let text = std::fs::read_to_string(&tp).with_context(|| tp.display().to_string())?;
    serde_json::from_str(&text).with_context(|| tp.display().to_string())
}

/// Shares of calls and condition uses covered by implemented functions.
fn shares(rows: &[(u64, Value)], key: &str) -> Value {
    let (mut sc, mut cu, mut sc_i, mut cu_i, mut sc_s, mut cu_s, mut n_i, mut n_s) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    for (_, r) in rows {
        let calls = r[key].as_u64().unwrap_or(0);
        let conds = r["condition_uses"].as_u64().unwrap_or(0);
        sc += calls;
        cu += conds;
        if r["implemented"] == true {
            sc_i += calls;
            cu_i += conds;
            n_i += 1;
        } else if r["stub"] == true {
            sc_s += calls;
            cu_s += conds;
            n_s += 1;
        }
    }
    let f = |a: u64, b: u64| if b == 0 { 0.0 } else { a as f64 / b as f64 };
    json!({
        "opcodes": rows.len(), "implemented_opcodes": n_i, "stub_opcodes": n_s,
        "calls": sc, "implemented_calls": sc_i, "stub_calls": sc_s,
        "condition_uses": cu, "implemented_condition_uses": cu_i, "stub_condition_uses": cu_s,
        "implemented_share": f(sc_i + cu_i, sc + cu), "handled_share": f(sc_i + cu_i + sc_s + cu_s, sc + cu),
    })
}

/// First opcode past the base game's range [documented: GECK wiki "NVSE Opcode Base"].
const EXTENDER_OPCODE_BASE: u16 = 0x1400;

/// The extender opcodes among `ops` that have no game opcode registered but whose names
/// reach a named handler, as the game runs them (`fnv_game::nvse::reaches_handler`,
/// #1408): built, though the game's registry does not list them. Probed over an empty
/// load order.
fn extender_handled(schema: &Schema, table: &FunctionTable, fjson: &Value, ops: impl IntoIterator<Item = u16>) -> std::collections::HashSet<u16> {
    let ops: Vec<u16> = ops.into_iter().filter(|op| *op >= EXTENDER_OPCODE_BASE && fnv_game::functions::lookup(*op).is_none()).collect();
    if ops.is_empty() {
        return Default::default();
    }
    let plugins: Vec<fnv_esm::Plugin> = Vec::new();
    let Ok(lo) = LoadOrder::new(&plugins) else { return Default::default() };
    let refs = fnv_sim::RefTable::build(&lo, schema);
    let forms = fnv_sim::FormStore::build(&lo, schema);
    let data = fnv_game::GameData::build(&lo, schema, table.clone(), fjson, &refs);
    let mut g = fnv_game::game::Game::new(&data, fnv_sim::World::new(&forms, &refs, fnv_sim::SimConfig::default()), fnv_game::game::GameConfig::default());
    ops.into_iter().filter(|op| fnv_game::nvse::reaches_handler(&mut g, table, *op)).collect()
}

/// The opcodes the catalogue counts a use of (script calls or condition uses).
fn used_in_catalogue(table: &FunctionTable) -> Vec<u16> {
    table.functions.iter().filter(|(_, f)| f.script_calls + f.condition_uses > 0).map(|(op, _)| *op).collect()
}

fn coverage(l: &Loaded, lo: &LoadOrder<'_>, schema: &Schema, fjson: &Value) -> Value {
    let done = decode_units(l);
    let mut stats = Stats::default();
    for d in &done {
        stats.program(&d.decoded);
    }
    // Extender functions the game runs by name count as implemented (#1408).
    let by_name = extender_handled(schema, &l.table, fjson, stats.calls.keys().copied().chain(used_in_catalogue(&l.table)));
    let implemented = |op: u16| fnv_game::functions::implemented(op) || by_name.contains(&op);
    let stub = |op: u16| fnv_game::functions::is_stub(op);
    let mut rows: Vec<(u64, Value)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (op, c) in &stats.calls {
        seen.insert(*op);
        let f = l.table.get(*op);
        let uses = f.map(|f| f.script_calls + f.condition_uses).unwrap_or(0);
        rows.push((
            uses,
            json!({
                "opcode": format!("0x{op:04X}"),
                "name": f.map(|f| f.name.clone()),
                "calls": c[0] + c[1],
                "decoded": c[0] + c[1] - c[2],
                "raw": c[2],
                "script_calls": f.map(|f| f.script_calls),
                "condition_uses": f.map(|f| f.condition_uses),
                "implemented": implemented(*op),
                "stub": stub(*op),
            }),
        ));
    }
    // Condition-only functions: used by CTDA, never called by a script.
    for (op, f) in &l.table.functions {
        if !seen.contains(op) && f.condition_uses > 0 {
            rows.push((
                f.condition_uses,
                json!({"opcode": format!("0x{op:04X}"), "name": f.name, "calls": 0, "decoded": 0, "raw": 0,
                       "script_calls": f.script_calls, "condition_uses": f.condition_uses,
                       "implemented": implemented(*op), "stub": stub(*op)}),
            ));
        }
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1["opcode"].as_str().cmp(&b.1["opcode"].as_str())));
    let total_calls: u64 = stats.calls.values().map(|c| c[0] + c[1]).sum();
    let raw_calls: u64 = stats.calls.values().map(|c| c[2]).sum();
    let implemented_calls: u64 = stats.calls.iter().filter(|(op, _)| implemented(**op)).map(|(_, c)| c[0] + c[1]).sum();
    // The intro chain (fnv_game::intro): its own calls and condition uses per opcode.
    let refs = fnv_sim::RefTable::build(lo, schema);
    let data = fnv_game::GameData::build(lo, schema, l.table.clone(), fjson, &refs);
    let intro = fnv_game::intro::intro(&data, &refs);
    let mut icalls: BTreeMap<u16, u64> = BTreeMap::new();
    for &u in &intro.units {
        fnv_game::intro::calls(&data.unit(u).program, &mut icalls);
    }
    let mut iconds: BTreeMap<u16, u64> = BTreeMap::new();
    for c in &intro.conditions {
        *iconds.entry(c.opcode()).or_default() += 1;
    }
    let mut irows: Vec<(u64, Value)> = icalls
        .keys()
        .chain(iconds.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|op| {
            let (c, k) = (icalls.get(op).copied().unwrap_or(0), iconds.get(op).copied().unwrap_or(0));
            (c + k, json!({"opcode": format!("0x{op:04X}"), "name": l.table.get(*op).map(|f| f.name.clone()),
                           "intro_calls": c, "condition_uses": k, "implemented": implemented(*op), "stub": stub(*op)}))
        })
        .collect();
    irows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1["opcode"].as_str().cmp(&b.1["opcode"].as_str())));
    json!({
        "format": "fnv.script.coverage/1",
        "load_order": l.load_order,
        "summary": {
            "opcodes": rows.len(),
            "calls": total_calls,
            "calls_decoded": total_calls - raw_calls,
            "implemented_opcodes": rows.iter().filter(|r| r.1["implemented"] == true).count(),
            "implemented_calls": implemented_calls,
            "by_catalogue_use": shares(&rows, "script_calls"),
            "intro": {
                "quests": intro.quests,
                "units": intro.units.len(),
                "conditions": intro.conditions.len(),
                "coverage": shares(&irows, "intro_calls"),
            },
        },
        "opcodes": rows.into_iter().map(|r| r.1).collect::<Vec<_>>(),
        "intro_opcodes": irows.into_iter().map(|r| r.1).collect::<Vec<_>>(),
    })
}

/// `script uses`: every quest with the calls of its units and its condition uses, per
/// function name (fnv.script.uses/1).
fn uses(l: &Loaded, lo: &LoadOrder<'_>, schema: &Schema, fjson: &Value) -> Value {
    let refs = fnv_sim::RefTable::build(lo, schema);
    let data = fnv_game::GameData::build(lo, schema, l.table.clone(), fjson, &refs);
    let name = |op: u16| l.table.get(op).map(|f| f.name.clone()).unwrap_or_else(|| format!("0x{op:04X}"));
    let quests: Vec<Value> = data
        .quests
        .values()
        .map(|q| {
            let scope = fnv_game::intro::quest_scope(&data, &refs, vec![q.form]);
            let mut calls: BTreeMap<u16, u64> = BTreeMap::new();
            for &u in &scope.units {
                fnv_game::intro::calls(&data.unit(u).program, &mut calls);
            }
            let mut conds: BTreeMap<u16, u64> = BTreeMap::new();
            for c in &scope.conditions {
                *conds.entry(c.opcode()).or_default() += 1;
            }
            json!({
                "form": format!("0x{:08X}", q.form.0),
                "edid": q.edid,
                "name": q.name,
                "start_enabled": q.start_enabled(),
                "units": scope.units.len(),
                "conditions": scope.conditions.len(),
                "calls": calls.iter().map(|(op, n)| (name(*op), json!(n))).collect::<serde_json::Map<_, _>>(),
                "condition_uses": conds.iter().map(|(op, n)| (name(*op), json!(n))).collect::<serde_json::Map<_, _>>(),
            })
        })
        .collect();
    json!({"format": "fnv.script.uses/1", "load_order": l.load_order, "quests": quests})
}

/// `script stubs`: the audit of docs/design/script-stubs.md. Calls are counted in the
/// load order's decoded scripts (object, quest and effect scripts, quest stage, dialogue,
/// package and terminal result scripts); condition uses are the catalogue's
/// (`functions.json`, CTDA over the masters).
fn stubs(l: &Loaded, lo: &LoadOrder<'_>, schema: &Schema, fjson: &Value) -> Value {
    use fnv_game::functions::stubs::{audit, Status};
    let done = decode_units(l);
    let mut stats = Stats::default();
    for d in &done {
        stats.program(&d.decoded);
    }
    // Extender functions the game runs by name are handled (#1408).
    let by_name = extender_handled(schema, &l.table, fjson, stats.calls.keys().copied().chain(used_in_catalogue(&l.table)));
    let calls = |op: u16| stats.calls.get(&op).map_or(0, |c| c[0] + c[1]);
    let conds = |op: u16| l.table.get(op).map_or(0, |f| f.condition_uses);
    let origin = |op: u16| l.table.get(op).and_then(|f| f.origin.clone()).unwrap_or_default();
    let mut rows: Vec<(u64, Value)> = Vec::new();
    // Every status, 0 when no stub has it, so a total can be expected to be 0.
    let mut by_status: BTreeMap<&str, u64> = Status::KEYS.iter().map(|k| (*k, 0)).collect();
    let mut by_workstream: BTreeMap<&str, u64> = BTreeMap::new();
    let mut by_origin: BTreeMap<String, u64> = BTreeMap::new();
    let (mut stub_calls, mut stub_conds) = (0u64, 0u64);
    for (op, name, st) in audit() {
        *by_status.entry(st.key()).or_default() += 1;
        let why = match st {
            Status::NeedsSystem(w) | Status::Assigned(w) | Status::Implementable(w) | Status::NoOp(w) => w,
        };
        if let Status::Assigned(w) = st {
            *by_workstream.entry(w).or_default() += 1;
        }
        let (c, k) = (calls(op), conds(op));
        if st.key() != "no_op" {
            stub_calls += c;
            stub_conds += k;
            *by_origin.entry(origin(op)).or_default() += 1;
        }
        rows.push((c + k, json!({"opcode": format!("0x{op:04X}"), "name": name, "origin": origin(op), "status": st.key(), "why": why, "calls": c, "condition_uses": k})));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1["opcode"].as_str().cmp(&b.1["opcode"].as_str())));
    // Functions used that have no registration at all.
    let mut unhandled: Vec<(u64, Value)> = l
        .table
        .functions
        .iter()
        .filter(|(op, _)| **op >= 0x1000 && fnv_game::functions::lookup(**op).is_none() && !by_name.contains(op))
        .map(|(op, f)| (calls(*op), f.condition_uses, op, f))
        .filter(|(c, k, ..)| c + k > 0)
        .map(|(c, k, op, f)| (c + k, json!({"opcode": format!("0x{op:04X}"), "name": f.name, "origin": origin(*op), "calls": c, "condition_uses": k})))
        .collect();
    // Opcodes the load order's scripts call that the function table has no name for (a
    // plugin's commands the catalogue never saw, or bytes no name matches: the
    // catalogue's `unresolved_opcodes`), and nothing answers (#1408). Kept apart from
    // `unhandled`, which lists named functions.
    let mut unnamed: Vec<(u64, Value)> = stats
        .calls
        .iter()
        .filter(|(op, _)| **op >= 0x1000 && l.table.get(**op).is_none() && fnv_game::functions::lookup(**op).is_none() && !by_name.contains(op))
        .map(|(op, c)| (c[0] + c[1], json!({"opcode": format!("0x{op:04X}"), "calls": c[0] + c[1]})))
        .collect();
    unnamed.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1["opcode"].as_str().cmp(&b.1["opcode"].as_str())));
    // Condition indices the catalogue does not count (no `condition_uses`, often no page
    // at all) that a CTDA in the load order uses and nothing answers: the registry has
    // no function and `Game::condition_value` no handler (#3614: 123 and 258 reached
    // every player before they were named).
    let listed: std::collections::BTreeSet<String> = unhandled.iter().filter_map(|r| r.1["opcode"].as_str().map(str::to_string)).collect();
    let mut ctda: BTreeMap<u16, u64> = BTreeMap::new();
    for recs in &lo.records {
        for r in recs {
            let _ = r.visit_subrecords(|sig, data| {
                if sig == b"CTDA" && data.len() >= 10 {
                    let f = u16::from_le_bytes([data[8], data[9]]);
                    if fnv_game::functions::lookup(0x1000 + f).is_none() && !fnv_game::cond::answered_without_opcode(f) && !by_name.contains(&(0x1000 + f)) {
                        *ctda.entry(f).or_default() += 1;
                    }
                }
            });
        }
    }
    // An extender condition is answered by name too (`Game::extender_condition`).
    let by_name_ctda = extender_handled(schema, &l.table, fjson, ctda.keys().map(|f| 0x1000 + f));
    for (f, k) in ctda {
        let op = 0x1000 + f;
        let opcode = format!("0x{op:04X}");
        if listed.contains(&opcode) || by_name_ctda.contains(&op) {
            continue;
        }
        let name = l.table.get(op).map(|t| t.name.clone()).unwrap_or_else(|| format!("condition index {f}"));
        unhandled.push((k, json!({"opcode": opcode, "name": name, "origin": origin(op), "calls": 0, "condition_uses": k, "counted_from": "CTDA"})));
    }
    unhandled.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1["opcode"].as_str().cmp(&b.1["opcode"].as_str())));
    let stubs = by_status.iter().filter(|(k, _)| **k != "no_op").map(|(_, n)| n).sum::<u64>();
    json!({
        "format": "fnv.script.stubs/1",
        "load_order": l.load_order,
        "summary": {
            "units": done.len(),
            "stubs": stubs,
            "by_status": by_status,
            "by_workstream": by_workstream,
            "by_origin": by_origin,
            "stub_calls": stub_calls,
            "stub_condition_uses": stub_conds,
            "unhandled_used": unhandled.len(),
            "unnamed_called": unnamed.len(),
        },
        "functions": rows.into_iter().map(|r| r.1).collect::<Vec<_>>(),
        "unhandled": unhandled.into_iter().map(|r| r.1).collect::<Vec<_>>(),
        "unnamed": unnamed.into_iter().map(|r| r.1).collect::<Vec<_>>(),
    })
}

/// Environment switch for `script run --no-voice-lengths` (`0`: fitted line lengths), to
/// run unchanged lab scenarios both ways.
pub const VOICE_LENGTHS_ENV: &str = "FNVTOOL_VOICE_LENGTHS";

/// `script run`: build the Layer 4 world and the Layer 5 data, run the actions.
fn run_actions(
    a: &ScriptArgs,
    actions: &std::path::Path,
    voice_lengths: bool,
    ini: &[PathBuf],
    coverage: Option<&std::path::Path>,
) -> anyhow::Result<Value> {
    let input = read_actions(actions)?;
    let tp = table_path(a);
    let ftext = std::fs::read_to_string(&tp).with_context(|| tp.display().to_string())?;
    let fjson: Value = serde_json::from_str(&ftext).with_context(|| tp.display().to_string())?;
    let table = FunctionTable::load(&tp).with_context(|| tp.display().to_string())?;
    let plugins = open_load_order(&a.lo)?;
    let lo = LoadOrder::new(&plugins)?;
    let schema = Schema::load_dir(&a.schema)?;
    let forms = fnv_sim::FormStore::build(&lo, &schema);
    let refs = fnv_sim::RefTable::build(&lo, &schema);
    let mut data = fnv_game::GameData::build(&lo, &schema, table, &fjson, &refs);
    for p in ini {
        data.ini_texts.push(std::fs::read_to_string(p).with_context(|| p.display().to_string())?);
    }
    let voice = voice_lengths_hook(a, &lo, voice_lengths);
    let Some(path) = coverage else {
        return fnv_game::runner::run_with_voice(&data, &forms, &refs, &input, voice).map_err(anyhow::Error::msg);
    };
    let (trace, c) = fnv_game::runner::run_with_coverage(&data, &forms, &refs, &input, voice).map_err(anyhow::Error::msg)?;
    let title = actions.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    fnv_install::guard_write(path, None)?;
    std::fs::write(path, serde_json::to_string(&c.log(&data, &title))?).with_context(|| path.display().to_string())?;
    Ok(trace)
}

fn read_actions(path: &std::path::Path) -> anyhow::Result<fnv_game::runner::Input> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    serde_json::from_str(&text).with_context(|| format!("{}: not a fnv.script.actions/1 document", path.display()))
}

/// The action files named, folders read for their `*.actions.json` (sorted).
fn action_files(paths: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for p in paths {
        if p.is_dir() {
            let mut v: Vec<PathBuf> = std::fs::read_dir(p)
                .with_context(|| p.display().to_string())?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|f| f.file_name().is_some_and(|n| n.to_string_lossy().ends_with(".actions.json")))
                .collect();
            v.sort();
            files.extend(v);
        } else {
            files.push(p.clone());
        }
    }
    Ok(files)
}

/// An action chain with its file name.
type NamedChain = (String, fnv_game::runner::Input);

/// `script cover`: every chain run over the load order it names (`plugins`, else the
/// first plugin given; the data built once per load order), the coverage merged with
/// the saved logs and compared with the whole load order given, the summary printed.
fn cover(
    a: &ScriptArgs,
    actions: &[PathBuf],
    logs: &[PathBuf],
    detail: Option<&std::path::Path>,
    log_dir: Option<&std::path::Path>,
    untouched: usize,
) -> anyhow::Result<Value> {
    let files = action_files(actions)?;
    if let Some(d) = log_dir {
        fnv_install::guard_write(d, None)?; // before the folder is made [#1987]
        std::fs::create_dir_all(d).with_context(|| d.display().to_string())?;
    }
    let mut all = Vec::new();
    let mut chains = Vec::new();
    let runs = each_chain(a, &files, |c, name, input| {
        Ok(match fnv_game::runner::run_with_coverage(c.data, c.forms, c.refs, input, c.voice.clone()) {
            Ok((trace, cov)) => {
                let log = cov.log(c.data, name);
                if let Some(d) = log_dir {
                    let p = d.join(format!("{}.coverage.json", name.trim_end_matches(".actions.json")));
                    fnv_install::guard_write(&p, None)?;
                    std::fs::write(&p, serde_json::to_string(&log)?).with_context(|| p.display().to_string())?;
                }
                let given = trace["given_errors"].as_array().map_or(0, Vec::len);
                (serde_json::json!({"chain": name, "plugins": c.plugins, "step_errors": step_errors(&trace), "given_errors": given}), Some(log))
            }
            Err(e) => (serde_json::json!({"chain": name, "plugins": c.plugins, "error": e}), None),
        })
    })?;
    for (chain, log) in runs {
        chains.push(chain);
        all.extend(log);
    }
    let tp = table_path(a);
    let ftext = std::fs::read_to_string(&tp).with_context(|| tp.display().to_string())?;
    let fjson: Value = serde_json::from_str(&ftext).with_context(|| tp.display().to_string())?;
    let schema = Schema::load_dir(&a.schema)?;
    for p in logs {
        let text = std::fs::read_to_string(p).with_context(|| p.display().to_string())?;
        all.push(serde_json::from_str(&text).with_context(|| p.display().to_string())?);
    }
    // The report compares with the whole load order given.
    let table = FunctionTable::load(&tp).with_context(|| tp.display().to_string())?;
    let plugins = open_load_order(&a.lo)?;
    let lo = LoadOrder::new(&plugins)?;
    let refs = fnv_sim::RefTable::build(&lo, &schema);
    let data = fnv_game::GameData::build(&lo, &schema, table, &fjson, &refs);
    let mut r = fnv_game::coverage::report(&data, &all, untouched).map_err(anyhow::Error::msg)?;
    if let Some(d) = detail {
        fnv_install::guard_write(d, None)?;
        std::fs::write(d, serde_json::to_string_pretty(&r)?).with_context(|| d.display().to_string())?;
    }
    if let Some(o) = r.as_object_mut() {
        o.remove("detail");
        o.insert("chains".into(), Value::Array(chains));
    }
    Ok(r)
}

/// One load order's data, for the chains written for it ([`each_chain`]).
struct ChainData<'a> {
    plugins: &'a [String],
    data: &'a fnv_game::GameData,
    forms: &'a fnv_sim::FormStore,
    refs: &'a fnv_sim::RefTable,
    voice: Option<fnv_game::runner::Voice>,
}

/// Run `f` on every chain in `files`, over the load order the chain names (`plugins`,
/// else the first plugin of `a`'s), the data built once per load order; the results in
/// the order the load orders are first named, each load order's in file order. A load
/// order's chains run side by side on the thread pool: every run builds its own world
/// and game over the shared data, so a chain's result does not depend on the others
/// (#1241: 206 chains one after another outran the smoke timeout).
fn each_chain<T: Send>(
    a: &ScriptArgs,
    files: &[PathBuf],
    f: impl Fn(&ChainData<'_>, &str, &fnv_game::runner::Input) -> anyhow::Result<T> + Sync,
) -> anyhow::Result<Vec<T>> {
    let tp = table_path(a);
    let ftext = std::fs::read_to_string(&tp).with_context(|| tp.display().to_string())?;
    let fjson: Value = serde_json::from_str(&ftext).with_context(|| tp.display().to_string())?;
    let schema = Schema::load_dir(&a.schema)?;
    let mut groups: Vec<(Vec<String>, Vec<NamedChain>)> = Vec::new();
    for file in files {
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let input = read_actions(file)?;
        let wanted = if input.plugins.is_empty() { a.lo.plugins[..1].to_vec() } else { input.plugins.clone() };
        match groups.iter_mut().find(|(p, _)| *p == wanted) {
            Some((_, v)) => v.push((name, input)),
            None => groups.push((wanted, vec![(name, input)])),
        }
    }
    let mut out = Vec::with_capacity(files.len());
    for (wanted, inputs) in &groups {
        let args = ScriptArgs {
            lo: crate::LoadOrderArgs { data_dir: a.lo.data_dir.clone(), plugins: wanted.clone() },
            schema: a.schema.clone(),
            functions: a.functions.clone(),
        };
        let table = FunctionTable::load(&tp).with_context(|| tp.display().to_string())?;
        let plugins = open_load_order(&args.lo)?;
        let lo = LoadOrder::new(&plugins)?;
        let forms = fnv_sim::FormStore::build(&lo, &schema);
        let refs = fnv_sim::RefTable::build(&lo, &schema);
        let data = fnv_game::GameData::build(&lo, &schema, table, &fjson, &refs);
        let c = ChainData { plugins: wanted, data: &data, forms: &forms, refs: &refs, voice: voice_lengths_hook(&args, &lo, true) };
        let done: Vec<T> = inputs.par_iter().map(|(name, input)| f(&c, name, input)).collect::<anyhow::Result<_>>()?;
        out.extend(done);
    }
    Ok(out)
}

/// `script sweep`: [`fnv_game::sweep::sweep`] over the whole load order.
fn sweep(a: &ScriptArgs, top: usize, detail: Option<&std::path::Path>, log: Option<&std::path::Path>) -> anyhow::Result<Value> {
    let tp = table_path(a);
    let ftext = std::fs::read_to_string(&tp).with_context(|| tp.display().to_string())?;
    let fjson: Value = serde_json::from_str(&ftext).with_context(|| tp.display().to_string())?;
    let table = FunctionTable::load(&tp).with_context(|| tp.display().to_string())?;
    let plugins = open_load_order(&a.lo)?;
    let lo = LoadOrder::new(&plugins)?;
    let schema = Schema::load_dir(&a.schema)?;
    let forms = fnv_sim::FormStore::build(&lo, &schema);
    let refs = fnv_sim::RefTable::build(&lo, &schema);
    let data = fnv_game::GameData::build(&lo, &schema, table, &fjson, &refs);
    let (mut r, cov) = fnv_game::sweep::sweep(&data, &forms, &refs, 5);
    // Every script's calls, one per argument shape: arguments read wrong without failing.
    let mut probe = fnv_game::sweep::probe_calls(&data, &forms, &refs, 5);
    r["summary"]["call_probe"] = json!({"shapes": probe["shapes"], "not_judged": probe["not_judged"].as_array().map_or(0, Vec::len), "causes": probe["causes"], "calls": probe["calls"]});
    r["call_probe"] = probe["arg_unread"].take();
    if detail.is_some() {
        r["call_probe_not_judged"] = probe["not_judged"].take();
    }
    if let Some(p) = log {
        fnv_install::guard_write(p, None)?;
        std::fs::write(p, serde_json::to_string(&cov.log(&data, "result-script sweep"))?).with_context(|| p.display().to_string())?;
    }
    if let Some(p) = detail {
        fnv_install::guard_write(p, None)?;
        std::fs::write(p, serde_json::to_string_pretty(&r)?).with_context(|| p.display().to_string())?;
    }
    if let Some(list) = r["call_probe"].as_array_mut() {
        list.truncate(top);
        for c in list.iter_mut() {
            if let Some(e) = c["e.g."].as_array_mut() {
                e.truncate(2);
            }
        }
    }
    if let Some(causes) = r["causes"].as_object_mut() {
        for v in causes.values_mut() {
            if let Some(list) = v.as_array_mut() {
                list.truncate(top);
                for c in list.iter_mut() {
                    if let Some(e) = c["e.g."].as_array_mut() {
                        e.truncate(2);
                    }
                }
            }
        }
    }
    Ok(r)
}

/// Steps of a trace that failed.
fn step_errors(trace: &Value) -> usize {
    trace["steps"].as_array().map_or(0, |v| v.iter().filter(|s| s.get("error").is_some()).count())
}

const ROUNDTRIP_LABEL: &str = "roundtrip: saved and loaded";

/// `script roundtrip`: every chain run twice over its load order, once as it is and once
/// with a save and load (the runner's `save_load`) before its middle action; the runs'
/// quests at the end (running, completed, stages done, variables) and observations
/// (without their step numbers) compared. A chain whose runs differ lists the first
/// differences.
fn roundtrip(a: &ScriptArgs, actions: &[PathBuf]) -> anyhow::Result<Value> {
    let files = action_files(actions)?;
    let chains = each_chain(a, &files, |c, name, input| {
        let plain = fnv_game::runner::run_with_voice(c.data, c.forms, c.refs, input, c.voice.clone()).map_err(anyhow::Error::msg)?;
        let mut with = input.clone();
        let at = with.actions.len() / 2;
        with.actions.insert(at, serde_json::from_value(serde_json::json!({"op": "save_load", "label": ROUNDTRIP_LABEL}))?);
        let loaded = fnv_game::runner::run_with_voice(c.data, c.forms, c.refs, &with, c.voice.clone()).map_err(anyhow::Error::msg)?;
        let strip = |t: &Value| -> Vec<Value> {
            t["observations"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|o| o["label"] != ROUNDTRIP_LABEL)
                .map(|o| {
                    let mut o = o.clone();
                    if let Some(m) = o.as_object_mut() {
                        m.remove("step");
                    }
                    o
                })
                .collect()
        };
        let mut diffs = Vec::new();
        json_diff("final.quests", &plain["final"]["quests"], &loaded["final"]["quests"], &mut diffs);
        json_diff("observations", &Value::Array(strip(&plain)), &Value::Array(strip(&loaded)), &mut diffs);
        let errors = (step_errors(&plain), step_errors(&loaded));
        let differs = !diffs.is_empty() || errors.0 != errors.1;
        let bytes = loaded["observations"].as_array().and_then(|v| v.iter().find(|o| o["label"] == ROUNDTRIP_LABEL)).map(|o| o["bytes"].clone());
        let n = diffs.len();
        diffs.truncate(10);
        Ok((
            differs,
            serde_json::json!({
                "chain": name, "saved_before_action": at, "save_bytes": bytes,
                "step_errors": [errors.0, errors.1], "differences": n, "first": diffs,
            }),
        ))
    })?;
    let differing = chains.iter().filter(|(d, _)| *d).count();
    let chains: Vec<Value> = chains.into_iter().map(|(_, c)| c).collect();
    Ok(serde_json::json!({"format": "fnv.script.roundtrip/1", "chains": chains.len(), "differing": differing, "runs": chains}))
}

/// Paths where `a` and `b` differ (objects by key, arrays of one length by place,
/// anything else by value).
fn json_diff(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let keys: std::collections::BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            for k in keys {
                json_diff(&format!("{path}.{k}"), x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null), out);
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                json_diff(&format!("{path}[{i}]"), p, q, out);
            }
        }
        _ if a != b => out.push(format!("{path}: {a} / {b}")),
        _ => {}
    }
}

/// The line-length hook of a headless run: the voice files in the archives next to the
/// plugins (docs/spec-notes/voice.md §8), unless `wanted` is false or the environment
/// says [`VOICE_LENGTHS_ENV`]`=0`. Without archives (synthetic plugins, a folder of
/// plugins only) no line has a file and every line keeps its fitted length.
pub fn voice_lengths_hook(a: &ScriptArgs, lo: &LoadOrder<'_>, wanted: bool) -> Option<fnv_game::runner::Voice> {
    if !wanted || std::env::var(VOICE_LENGTHS_ENV).is_ok_and(|v| v == "0") {
        return None;
    }
    match crate::voice_cmd::open_vfs(&a.lo.data_dir, &a.lo.plugins) {
        Ok(vfs) => Some(std::sync::Arc::new(fnv_voice::VoiceBank::build(lo, std::sync::Arc::new(vfs)))),
        Err(e) => {
            tracing::warn!("voice files not read, dialogue lines keep their fitted length: {e:#}");
            None
        }
    }
}

/// The headless run's first weather in an exterior (fnv_game::weather::settle_start_weather):
/// the region or climate choice the engine's sky starts with (fnv_world::sky::start_weather).
fn weather_hook(a: &ScriptArgs) {
    let Ok(plugins) = open_load_order(&a.lo) else { return };
    let Ok(lo) = LoadOrder::new(&plugins) else { return };
    let Ok(schema) = Schema::load_dir(&a.schema) else { return };
    let regions = fnv_world::sky::region_weathers(&lo, &schema);
    let ix = fnv_world::WorldIndex::build(&lo, &schema);
    let climates: std::collections::HashMap<u32, fnv_world::sky::Climate> =
        ix.worldspaces.values().filter_map(|w| Some((w.form_id, fnv_world::sky::climate(&lo, &schema, w.climate?)?))).collect();
    let hook: fnv_game::weather::StartWeather = std::sync::Arc::new(move |ws: fnv_sim::FormId, p: [f32; 3]| {
        fnv_world::sky::start_weather(&regions, climates.get(&ws.0), ws.0, p[0], p[1], |_| None).map(fnv_sim::FormId)
    });
    fnv_game::weather::set_start_weather(hook);
}
