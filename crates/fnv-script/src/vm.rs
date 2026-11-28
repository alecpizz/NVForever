//! The interpreter: runs the IR of [`crate::ir`] against a [`Host`].
//!
//! The VM knows the language, not the game: values, the postfix expression stack, the
//! `If`/`ElseIf`/`Else` chains (stray markers included), `Set`, `Return`, and a
//! deterministic step budget. Everything else (where a variable lives, what a reference
//! list entry names, what a function does) goes through the [`Host`] trait, which
//! `fnv-game` implements over the Layer 4 world. Behaviour and its evidence are in
//! docs/spec-notes/scripts.md §4.

use crate::ir::*;
use crate::table::FunctionTable;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod anh;
mod itr;
mod jip;
mod quat;
pub use jip::{JIP_NAMES, JIP_VERSION};

/// A script value. Numbers are kept as `f64` [observed: float variables keep
/// 16777217, 0.1 and 3000000000.5 exactly, so they are not single precision;
/// docs/spec-notes/scripts.md §4.1]. A reference is a load-order FormID; 0 is "none".
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Value {
    Num(f64),
    Ref(u32),
}

impl Default for Value {
    fn default() -> Self {
        Value::ZERO
    }
}

impl Value {
    pub const ZERO: Value = Value::Num(0.0);
    pub const NONE: Value = Value::Ref(0);

    pub fn bool(b: bool) -> Value {
        Value::Num(if b { 1.0 } else { 0.0 })
    }

    /// The value as a number; a reference counts as its FormID [hypothesis].
    pub fn num(self) -> f64 {
        match self {
            Value::Num(x) => x,
            Value::Ref(f) => f as f64,
        }
    }

    pub fn truthy(self) -> bool {
        match self {
            Value::Num(x) => x != 0.0,
            Value::Ref(f) => f != 0,
        }
    }

    /// The value as a reference: a `Ref`, or a whole positive number taken as a FormID
    /// (a `ref` variable that was set from a number) [hypothesis]. 0 is none.
    pub fn as_form(self) -> Option<u32> {
        match self {
            Value::Ref(0) => None,
            Value::Ref(f) => Some(f),
            Value::Num(x) if x >= 1.0 && x <= u32::MAX as f64 && x.fract() == 0.0 => Some(x as u32),
            Value::Num(_) => None,
        }
    }
}

/// What a `Set` into an integer variable (`short`, `long`, `int`) stores: the number
/// truncated toward zero to a signed 32-bit integer, or `i32::MIN` when it is out of
/// range or not a number [observed: 2.7 → 2, −2.7 → −2, 40000 → 40000 (no 16-bit wrap
/// for `short`), 2147483647.5 → 2147483647, 3000000000.5 and −3000000000.5 →
/// −2147483648; `L5.quest.var_storage`, docs/spec-notes/scripts.md §4.1].
pub fn to_int_storage(x: f64) -> i32 {
    if !(-2_147_483_648.0..2_147_483_648.0).contains(&x) {
        i32::MIN
    } else {
        x.trunc() as i32
    }
}

/// The value a number literal of an expression has. A literal written without a decimal
/// point is read as an integer and saturates at 2147483647; one with a point is read as
/// a double [observed: `set x to 3000000000` stores 2147483647 in float and integer
/// variables alike, `3000000000.5` stores 3000000000.5 in a float; the compiler writes
/// `-3000000000` as `3000000000 ~`, which gives −2147483647; scripts.md §4.1].
pub fn literal(text: &str, value: f64) -> f64 {
    if text.contains('.') {
        return value;
    }
    match text.parse::<u64>() {
        Ok(v) => v.min(i32::MAX as u64) as f64,
        Err(_) if text.bytes().all(|b| b.is_ascii_digit()) && !text.is_empty() => i32::MAX as f64,
        Err(_) => value,
    }
}

/// A function argument after evaluation.
#[derive(Debug, Clone, PartialEq)]
pub enum ArgValue {
    /// A tagged value: an integer or float literal, a reference list entry, a variable.
    Value(Value),
    /// A bare enumeration code (actor value, animation group, sex, …).
    Enum(u16),
    Axis(char),
    Str(String),
    /// The format variables of a message.
    Vars(Vec<Value>),
    /// The untagged i32 after a message's format variables (its duration).
    Int32(i32),
}

impl ArgValue {
    pub fn value(&self) -> Option<Value> {
        match self {
            ArgValue::Value(v) => Some(*v),
            ArgValue::Enum(c) => Some(Value::Num(*c as f64)),
            ArgValue::Int32(i) => Some(Value::Num(*i as f64)),
            _ => None,
        }
    }
}

/// Why a run stopped early.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum VmError {
    #[error("step budget of {0} statements exceeded")]
    Budget(u64),
    #[error("opcode 0x{0:04X} is not in the function table")]
    UnknownOpcode(u16),
    #[error("undecodable {0}")]
    Raw(&'static str),
    #[error("malformed expression: {0}")]
    Expression(String),
    #[error("{0}")]
    Host(String),
    /// Something of the script extender's that is not built yet (task named).
    #[error("not built yet: {0}")]
    NotBuilt(String),
    /// A value of the wrong kind in an extended expression, or another of the run-time
    /// errors the extender reports [documented: NVSE Error].
    #[error("{0}")]
    Type(String),
    /// An absent array element, separately inspectable by TypeOf (typeof-missing-key.md).
    #[error("no element at key {0}")]
    MissingElement(String),
    /// An extender function called on no reference: it stops the script, as no other
    /// extender error does [observed: nvse-inv foreach.read.after, "script call failed"].
    #[error("{0} called on no reference")]
    NoReference(String),
    /// A host command whose invalid argument aborts the calling script at runtime in
    /// both strict and default modes (for example `SetClass` given a non-CLAS or null
    /// class argument) [observed: w4g_05_isactordetected_and_setclass_invalid].
    #[error("{0}")]
    RuntimeAbort(String),
}

/// What the VM needs from the game. Every method may fail with a message; a failure
/// aborts the running script (the host decides which problems are failures and which
/// are logged and answered with 0).
pub trait Host {
    /// The value of a variable: a local of the running instance, another script's
    /// variable (`on`), or a global.
    fn get_var(&mut self, var: &VarRef) -> Result<Value, String>;
    fn set_var(&mut self, var: &VarRef, value: Value) -> Result<(), String>;
    /// The value of a reference list entry: the form of an `SCRO`, the variable of an
    /// `SCRV`.
    fn ref_value(&mut self, r: RefIndex) -> Result<Value, String>;
    /// Run a function. `on` is the explicit calling reference (`Ref.Func`), already
    /// resolved.
    fn call(&mut self, opcode: u16, on: Option<Value>, args: &[ArgValue]) -> Result<Value, String>;
    /// An opcode that is not in the function table was skipped (non-strict runs).
    fn skipped(&mut self, _what: &VmError) {}
    /// Diagnostic-only notification before a fatal no-reference call is returned.
    /// `on` is already evaluated; implementations must not evaluate the call again.
    /// Other hosts keep the default no-op and the original error contract.
    fn no_reference_call(&mut self, _name: &str, _on: Option<Value>, _call: &XCall) {}
    /// Whether an extender/source call with an explicit null receiver (`0`) still
    /// dispatches to [`Host::ext_call`] instead of aborting with
    /// [`VmError::NoReference`]. Defaults to `false`.
    fn allows_null_receiver(&self, _name: &str) -> bool {
        false
    }

    /// The statement whose run caused the skips reported since the last call: the
    /// innermost that holds them (for the mod log, which names the line).
    fn skipped_in(&mut self, _stmt: &Stmt) {}
    /// Whether the last call ended the run the way `Return` does (a function such as
    /// `RemoveMe`, which "acts like a return" [documented: catalogue notes]). The VM
    /// asks after every statement; answering true also clears the request.
    fn take_return(&mut self) -> bool {
        false
    }
    /// The step counter of the outermost run, which every run nested under it (a
    /// user-defined function, a result script a statement started) draws on, so the
    /// budget ([`RunOpts::max_steps`]) holds for the whole call tree [#1991]; `None`: each
    /// run counts its own.
    fn shared_steps(&mut self) -> Option<&mut u64> {
        None
    }
    /// How a variable is stored (the extender's `string_var` and `array_var` hold ids).
    fn var_kind(&mut self, _var: &VarRef) -> VarKind {
        VarKind::Value
    }
    /// The extender's strings and arrays; `None`: this host has none.
    fn ext(&mut self) -> Option<&mut ExtStore> {
        None
    }
    /// A form's name, as `$` and ToString print it.
    fn form_text(&mut self, _form: u32) -> Option<String> {
        None
    }
    /// A line for the console (Ar_Dump and the extender's printing functions).
    fn print(&mut self, _text: &str) {}
    /// A script asked whether a plugin is there (IsPluginInstalled, GetPluginVersion,
    /// GetModIndex, by the function's lower-case name) and read `answer`: for a host
    /// that keeps what the load order's scripts look for.
    fn plugin_asked(&mut self, _function: &str, _name: &str, _answer: f64) {}
    /// A script error worth the console's attention: a call that crashes the original
    /// game answers its default value here and says so [decision: the owner, 2026-10-26,
    /// "scripts should never crash the game; log it on the console as a script error"].
    fn script_error(&mut self, _text: &str) {}
    /// An extender function the VM does not build, by catalogue name (lower case), on a
    /// reference if given, with its arguments evaluated (an axis as its letter); `None`
    /// when the host does not build it either.
    fn ext_call(&mut self, _name: &str, _on: Option<Value>, _args: &[XVal]) -> Option<Result<XVal, String>> {
        None
    }
    /// The plugin the running script comes from; `None` for a runtime-compiled or console
    /// script (JIP's private variable stores are per plugin).
    fn calling_plugin(&mut self) -> Option<String> {
        None
    }
    /// The script form the running code belongs to; `None` for a result script or a
    /// runtime-compiled one (GetCurrentScript).
    fn current_script(&mut self) -> Option<u32> {
        None
    }
    /// A local variable of the running script, by name (the quaternion functions write
    /// into variables named by strings).
    fn local_var(&mut self, _name: &str) -> Option<VarRef> {
        None
    }
    /// The name a script variable was declared with, for PrintVar.
    fn var_name(&mut self, _var: &VarRef) -> Option<String> {
        None
    }
    /// A number for a variable the host can read again later, outside this run (SUP's
    /// HudBarSetValueScriptVar); `None` when it cannot (a result script's locals).
    fn var_place(&mut self, _var: &VarRef) -> Option<u32> {
        None
    }
    /// The UDF MessageBoxExAlt runs with the pressed button.
    fn box_callback(&mut self, _func: XVal) {}
    /// A message without a Message form (MessageEx; MessageBoxEx with its buttons).
    fn show_text(&mut self, _text: &str, _buttons: Vec<String>) {}
    /// The calling script's plugin's debug mode (DebugPrint), after setting it when `set`
    /// is given (SetDebugMode; `plugin` a load-order index from the console).
    fn debug_mode(&mut self, _set: Option<bool>, _plugin: Option<u32>) -> bool {
        false
    }
    /// Run a user-defined function or a lambda (`Call`, the ar_* functions that take a
    /// function): its value (what SetFunctionValue set; 0 when nothing).
    fn call_function(&mut self, _f: &XVal, _on: Option<Value>, _args: Vec<XVal>) -> Result<XVal, String> {
        Err("user-defined functions are not run by this host".into())
    }
    /// SetFunctionValue inside a function.
    fn set_function_value(&mut self, _v: XVal) -> Result<(), String> {
        Err("SetFunctionValue outside a user-defined function".into())
    }
    /// The script that called the running function (GetCallingScript).
    fn calling_script(&mut self) -> Option<u32> {
        None
    }
    /// The forms of a form list, or the items (one per kind) a container or actor holds.
    fn members(&mut self, _form: u32) -> Option<Vec<u32>> {
        None
    }
    /// A random number in [0, 1) (Rand).
    fn random(&mut self) -> f64 {
        0.5
    }
    /// A form's record type (`REFR`, `NPC_` …), None for no such form.
    fn form_sig(&mut self, _form: u32) -> Option<[u8; 4]> {
        None
    }
    /// A reference's base form.
    fn base_form(&mut self, _form: u32) -> Option<u32> {
        None
    }
    /// A plugin's load-order index, by file name.
    fn mod_index(&mut self, _name: &str) -> Option<u32> {
        None
    }
    /// The file name of the plugin at a load-order index.
    fn mod_name(&mut self, _index: u32) -> Option<String> {
        None
    }
    /// A decoded record field (JIP's trait getters, jip-records.md): subrecord `sub`,
    /// field path `path`; a number, a form or a string.
    fn record_field(&mut self, _form: u32, _sub: &str, _path: &str) -> Option<XVal> {
        None
    }
    /// A value of an actor's current or last hit (JIP's GetHit…, jip-hit-info.md):
    /// `what` is attacker, weapon, projectile, health, limb, fatigue, base_weapon,
    /// flag (`k` the flag number) or 3d (`k` the data type 0..=2).
    fn hit_value(&mut self, _actor: u32, _what: &str, _k: i64) -> Option<XVal> {
        None
    }
    /// The name of a body part, by part type (GetBodyPartName).
    fn body_part_name(&mut self, _form: u32, _part: i64) -> Option<String> {
        None
    }
    /// A game setting (GMST) by Editor ID, without regard to case.
    fn game_setting(&mut self, _name: &str) -> Option<XVal> {
        None
    }
    /// A quest objective's display text.
    fn objective_text(&mut self, _quest: u32, _index: i32) -> Option<String> {
        None
    }
    /// The reference the script runs on, if any (JIP's stores default to it).
    fn calling_ref(&mut self) -> Option<u32> {
        None
    }
    /// An actor value's code by name, without regard to case.
    fn actor_value_code(&mut self, _name: &str) -> Option<u16> {
        None
    }
    /// GetGameLoaded (`restarted` false) or GetGameRestarted for the running script.
    fn game_loaded(&mut self, _restarted: bool) -> bool {
        false
    }
    /// The event functions (SetEventHandler and the rest).
    fn event_call(&mut self, _c: EventCall) -> Result<XVal, String> {
        Err("events are not run by this host".into())
    }
    /// The value that names a lambda this run made (`let f := ({x} => x)`), kept by the
    /// host with the variables it shares.
    fn make_lambda(&mut self, _l: &XExpr) -> Result<XVal, String> {
        Err("lambdas are not run by this host".into())
    }
    /// JIP's ScriptWait: whether the last call asked the running block to wait, and for
    /// how many processing cycles. The VM asks after every statement; answering takes the
    /// request. The VM then stops the run ([`Exit::Waiting`]) and keeps where it stood
    /// ([`Vm::take_wait`]) for [`Vm::resume`].
    fn take_wait(&mut self) -> Option<u32> {
        None
    }
}

/// Where a waiting run stands (JIP's ScriptWait): from the block's statement list down,
/// at each level the statement's index and, for an `If`, the arm that was taken (the
/// number of arms for its `Else`); the last entry is the statement that asked to wait.
pub type WaitPoint = Vec<(usize, usize)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOpts {
    /// Unknown opcodes and undecodable pieces fail the run (tests) instead of being
    /// logged and skipped (the game).
    pub strict: bool,
    /// Statements (and calls) one run may execute, with the runs nested under it when the
    /// host shares its counter ([`Host::shared_steps`]): a runaway loop of the extender's
    /// (`while`, each round counted) or runaway recursion through the host (a stage that
    /// sets itself) stops there.
    pub max_steps: u64,
}

impl Default for RunOpts {
    fn default() -> Self {
        RunOpts { strict: false, max_steps: 100_000 }
    }
}

/// How a statement list ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Normal,
    /// `Return`: the whole run ends.
    Return,
    /// A stray `Else`/`ElseIf` with no `EndIf` after it in its list: the list ends here
    /// and the enclosing chain is left (see [`Vm::list`]).
    Leave,
    /// `break` / `continue`: up to the innermost loop.
    Break,
    Continue,
    /// `GoTo n`: up to the statement list that holds `Label n`.
    GoTo(i32),
    /// ScriptWait: up to the top, the place kept in [`Vm::wait`].
    Wait,
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    /// The statements ran to the end.
    Finished,
    /// A `Return` statement ended the run.
    Returned,
    /// ScriptWait stopped the run; [`Vm::take_wait`] says where and for how long.
    Waiting,
}

pub struct Vm<'t, 'h> {
    table: &'t FunctionTable,
    host: &'h mut dyn Host,
    opts: RunOpts,
    steps: u64,
    /// A ScriptWait's cycles and place, built as the run unwinds (innermost first).
    wait: Option<(u32, WaitPoint)>,
    /// Inside a loop or a chain of stray markers, where a place cannot be kept: a
    /// ScriptWait there does not wait [decision: JIP's pages say nothing of loops].
    no_wait: u32,
    /// Skips so far, and how many of them a statement was told for ([`Host::skipped_in`]).
    skips: u64,
    skips_told: u64,
    /// When set by [`Vm::run_capturing_last`], retain the last value-producing statement's
    /// result across the per-statement `ExtStore::sweep` so interactive console lines can
    /// format temporary arrays before cleanup.
    capture_last: bool,
    last_value: Option<XVal>,
}

impl<'t, 'h> Vm<'t, 'h> {
    pub fn new(table: &'t FunctionTable, host: &'h mut dyn Host, opts: RunOpts) -> Self {
        Vm {
            table,
            host,
            opts,
            steps: 0,
            wait: None,
            no_wait: 0,
            skips: 0,
            skips_told: 0,
            capture_last: false,
            last_value: None,
        }
    }

    fn record_last(&mut self, value: XVal) {
        if !self.capture_last {
            return;
        }
        if let Some(ext) = self.host.ext() {
            ext.retain(&value);
            if let Some(old) = self.last_value.replace(value) {
                ext.release(&old);
            }
        } else {
            self.last_value = Some(value);
        }
    }

    fn skip(&mut self, e: &VmError) {
        self.skips += 1;
        self.host.skipped(e);
    }

    /// After a run that ended [`Exit::Waiting`]: the cycles to wait and the place to
    /// resume from (outermost first), for [`Vm::resume`].
    pub fn take_wait(&mut self) -> Option<(u32, WaitPoint)> {
        self.wait.take().map(|(n, mut p)| {
            p.reverse();
            (n, p)
        })
    }

    /// Go on with a run that waited, after the statement that asked to (`at` from
    /// [`Vm::take_wait`]). The `If` arms on the way are not tested again; a place that no
    /// longer names an `If` arm ends the run.
    pub fn resume(&mut self, stmts: &[Stmt], at: &[(usize, usize)]) -> Result<Exit, VmError> {
        Ok(Self::exit(self.list_resume(stmts, false, at)?))
    }

    fn exit(f: Flow) -> Exit {
        match f {
            Flow::Return => Exit::Returned,
            Flow::Wait => Exit::Waiting,
            Flow::Normal | Flow::Leave | Flow::Break | Flow::Continue | Flow::GoTo(_) => Exit::Finished,
        }
    }

    fn list_resume(&mut self, stmts: &[Stmt], in_else: bool, at: &[(usize, usize)]) -> Result<Flow, VmError> {
        let Some(&(i, arm)) = at.first() else { return Ok(Flow::Normal) };
        if at.len() == 1 {
            return self.list_from(stmts, in_else, i + 1);
        }
        let Some(Stmt::If { arms, otherwise }) = stmts.get(i) else { return Ok(Flow::Normal) };
        let body = match arms.get(arm) {
            Some(a) => Some((&a.body, false)),
            None => otherwise.as_ref().map(|o| (o, true)),
        };
        let Some((b, is_else)) = body else { return Ok(Flow::Normal) };
        match self.list_resume(b, is_else, &at[1..])? {
            Flow::GoTo(n) => match label_at(stmts, n) {
                Some(to) => return self.list_from(stmts, in_else, to),
                None => return Ok(Flow::GoTo(n)),
            },
            Flow::Wait => {
                if let Some(w) = self.wait.as_mut() {
                    w.1.push((i, arm));
                }
                return Ok(Flow::Wait);
            }
            f @ (Flow::Return | Flow::Break | Flow::Continue) => return Ok(f),
            Flow::Normal | Flow::Leave => {}
        }
        self.list_from(stmts, in_else, i + 1)
    }

    /// Statements executed so far by this run (not counting the runs nested under it).
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Run a statement list (a block body, or the top of a result script).
    pub fn run(&mut self, stmts: &[Stmt]) -> Result<Exit, VmError> {
        Ok(Self::exit(self.list(stmts, false)?))
    }

    /// Run a statement list and return the value produced by the last value-producing
    /// statement (`Set`, `Call`, `Let`, `Eval`). If that value is an array, it stays
    /// temporarily retained in `ExtStore` so the caller can format it before calling
    /// `hand_over` and `sweep`.
    pub fn run_capturing_last(&mut self, stmts: &[Stmt]) -> Result<(Exit, Option<XVal>), VmError> {
        let prev_capture = std::mem::replace(&mut self.capture_last, true);
        let prev_value = self.last_value.take();
        let res = self.list(stmts, false);
        self.capture_last = prev_capture;
        let captured = std::mem::replace(&mut self.last_value, prev_value);
        match res {
            Ok(flow) => Ok((Self::exit(flow), captured)),
            Err(e) => {
                if let (Some(v), Some(ext)) = (captured, self.host.ext()) {
                    ext.release(&v);
                }
                Err(e)
            }
        }
    }

    /// Evaluate one expression (conditions, tests).
    pub fn eval(&mut self, e: &Expr) -> Result<Value, VmError> {
        let mut stack: Vec<Value> = Vec::with_capacity(e.0.len());
        for t in &e.0 {
            let v = match t {
                Token::Num { value, text } => Value::Num(literal(text, *value)),
                Token::Var { var } => self.host.get_var(var).map_err(VmError::Host)?,
                Token::Ref { r } => self.host.ref_value(*r).map_err(VmError::Host)?,
                Token::Call { call } => self.call(call)?,
                // A failing extended condition counts as false and the script goes on
                // [observed: probe loops/03, reading past a shrunk list in `if eval` does not
                // stop the original's script]; strict runs stop.
                Token::X { e } => {
                    let evaluated = self.xeval_truthy(e).and_then(|(v, truthy)| match v {
                        XVal::Str(_) => Ok(Value::bool(truthy)),
                        other => other.value(),
                    });
                    match evaluated {
                        Ok(v) => v,
                        Err(err @ (VmError::Type(_) | VmError::MissingElement(_) | VmError::NotBuilt(_) | VmError::Expression(_) | VmError::Host(_))) if !self.opts.strict => {
                            self.skip(&err);
                            Value::ZERO
                        }
                        Err(err) => return Err(err),
                    }
                }
                Token::Op { op } => {
                    let b = stack.pop().ok_or_else(|| VmError::Expression(format!("{} without operands", op.symbol())))?;
                    if *op == Op::Neg {
                        Value::Num(-b.num())
                    } else {
                        let a = stack
                            .pop()
                            .ok_or_else(|| VmError::Expression(format!("{} with one operand", op.symbol())))?;
                        binary(*op, a, b)
                    }
                }
                Token::Raw { .. } => {
                    let e = VmError::Raw("expression token");
                    if self.opts.strict {
                        return Err(e);
                    }
                    self.skip(&e);
                    // The rest of the expression is unknown: the value is 0.
                    return Ok(Value::ZERO);
                }
            };
            stack.push(v);
        }
        // Leftover values stop the script, in strict mode or not: the original stops a
        // script at `if GetStage Q 100 == 1` (GetStage given an extra argument, as three
        // INFO begin scripts of FalloutNV.esm are compiled), running neither arm
        // [observed 2026-10-30, capture o4_00_probe; observer-checks.md check 32].
        match stack.len() {
            1 => Ok(stack[0]),
            0 => Err(VmError::Expression("empty".into())),
            n => Err(VmError::Expression(format!("{n} values left on the stack"))),
        }
    }

    fn step(&mut self) -> Result<(), VmError> {
        self.steps += 1;
        let used = match self.host.shared_steps() {
            Some(shared) => {
                *shared += 1;
                *shared
            }
            None => self.steps,
        };
        if used > self.opts.max_steps {
            return Err(VmError::Budget(self.opts.max_steps));
        }
        Ok(())
    }

    /// Run a statement list. `in_else` is true for the `otherwise` body of an `If`.
    ///
    /// Stray markers (`Stmt::Unmatched`, docs/spec-notes/scripts.md §2.6 and §4.2):
    /// - `EndIf` with nothing open does nothing [observed: the stray EndIf after
    ///   BeatrixFX's ElseIf branches, `L5.quest.stray_elseif`];
    /// - an `ElseIf` reached after its chain was closed tests its condition and runs the
    ///   statements up to the next stray marker of the list when it holds, as a new `If`
    ///   would; the next stray `ElseIf` or `Else` continues that chain and the next stray
    ///   `EndIf` closes it [observed: BeatrixFX's six stray ElseIf steps all ran
    ///   (`L5.quest.stray_elseif`); the dead Techatticup hostage's `elseif GetDead == 1 /
    ///   activate` opened its loot menu (`L5.quest.dead_activation`); layer5-captures.md
    ///   §8.3 c]. Whether a branch taken just before the marker would suppress it was not
    ///   exercised [hypothesis: it does not; the chain starts fresh]. A stray `Else`
    ///   starts a chain whose only branch runs [hypothesis, by the same reading];
    /// - inside an `Else` body, a marker continues the closed chain (a later `ElseIf` of
    ///   the same chain, §2.6): one branch of the chain has run, so reaching it ends the
    ///   chain, as running into the next marker of an ordinary chain does [hypothesis].
    fn list(&mut self, stmts: &[Stmt], in_else: bool) -> Result<Flow, VmError> {
        self.list_from(stmts, in_else, 0)
    }

    /// [`Vm::list`] from the statement at `start`.
    fn list_from(&mut self, stmts: &[Stmt], in_else: bool, start: usize) -> Result<Flow, VmError> {
        let mut i = start;
        while i < stmts.len() {
            self.step()?;
            let mark = self.skips;
            match &stmts[i] {
                Stmt::Set { target, expr } => {
                    let v = self.eval(expr)?;
                    // A call in the expression that stops the script (a call on an
                    // empty reference) leaves the variable unchanged.
                    if self.host.take_return() {
                        return Ok(Flow::Return);
                    }
                    self.host.set_var(target, v).map_err(VmError::Host)?;
                    if self.capture_last {
                        let stored = self.host.get_var(target).unwrap_or(v);
                        self.record_last(XVal::from_value(stored));
                    }
                }
                Stmt::If { arms, otherwise } => {
                    let mut taken = None;
                    for (k, a) in arms.iter().enumerate() {
                        let hold = self.eval(&a.cond)?.truthy();
                        if self.host.take_return() {
                            return Ok(Flow::Return);
                        }
                        if hold {
                            taken = Some((&a.body, false, k));
                            break;
                        }
                    }
                    let body = taken.or(otherwise.as_ref().map(|o| (o, true, arms.len())));
                    if let Some((b, is_else, k)) = body {
                        match self.list(b, is_else)? {
                            Flow::GoTo(n) => match label_at(stmts, n) {
                                Some(at) => {
                                    i = at;
                                    continue;
                                }
                                None => return Ok(Flow::GoTo(n)),
                            },
                            Flow::Wait => {
                                if let Some(w) = self.wait.as_mut() {
                                    w.1.push((i, k));
                                }
                                return Ok(Flow::Wait);
                            }
                            f @ (Flow::Return | Flow::Break | Flow::Continue) => return Ok(f),
                            Flow::Normal | Flow::Leave => {}
                        }
                    }
                }
                Stmt::Return => return Ok(Flow::Return),
                Stmt::Call { call } => {
                    let v = self.call(call)?;
                    if self.capture_last {
                        self.record_last(XVal::from_value(v));
                    }
                }
                // A failing extended statement reports its error and the script carries on
                // with the next line [documented: Let; NVSE Error]; strict runs (tests)
                // stop.
                Stmt::Let { expr } | Stmt::Eval { expr } => {
                    match self.xeval(expr) {
                        Ok(v) => {
                            if self.capture_last {
                                self.record_last(v);
                            }
                        }
                        Err(e @ (VmError::Type(_) | VmError::MissingElement(_) | VmError::NotBuilt(_) | VmError::Expression(_) | VmError::Host(_))) if !self.opts.strict => {
                            self.skip(&e)
                        }
                        Err(e) => return Err(e),
                    }
                    if self.host.take_return() {
                        return Ok(Flow::Return);
                    }
                }
                // Each round counts against the step budget, so a loop that never ends
                // stops the script there [decision: what the original does is to be
                // observed, extender-core-tasks.md B4].
                Stmt::While { cond, body } => loop {
                    self.step()?;
                    let go = self.xeval_truthy(cond)?.1;
                    if self.host.take_return() {
                        return Ok(Flow::Return);
                    }
                    if !go {
                        break;
                    }
                    self.no_wait += 1;
                    let f = self.list(body, false);
                    self.no_wait -= 1;
                    match f? {
                        Flow::Return => return Ok(Flow::Return),
                        Flow::Break => break,
                        f @ Flow::GoTo(_) => return Ok(f),
                        Flow::Normal | Flow::Leave | Flow::Continue | Flow::Wait => {}
                    }
                },
                Stmt::ForEach { head, body } => {
                    self.no_wait += 1;
                    let f = self.foreach(head, body);
                    self.no_wait -= 1;
                    if let f @ (Flow::Return | Flow::GoTo(_)) = f? {
                        return Ok(f);
                    }
                }
                Stmt::Label { .. } => {}
                Stmt::GoTo { n } => match label_at(stmts, *n) {
                    Some(at) => {
                        i = at;
                        continue;
                    }
                    None => return Ok(Flow::GoTo(*n)),
                },
                Stmt::Break => return Ok(Flow::Break),
                Stmt::Continue => return Ok(Flow::Continue),
                Stmt::Unmatched { marker: Marker::EndIf, .. } => {}
                Stmt::Unmatched { .. } if in_else => {
                    let end = stmts[i + 1..]
                        .iter()
                        .position(|s| matches!(s, Stmt::Unmatched { marker: Marker::EndIf, .. }));
                    match end {
                        Some(k) => i += k + 1,
                        None => return Ok(Flow::Leave),
                    }
                }
                Stmt::Unmatched { .. } => {
                    self.no_wait += 1;
                    let r = self.stray_chain(stmts, i);
                    self.no_wait -= 1;
                    let (flow, next) = r?;
                    if matches!(flow, Flow::Return | Flow::Break | Flow::Continue) {
                        return Ok(flow);
                    }
                    // `next` is the closing stray EndIf (or the end of the list).
                    i = next;
                }
                Stmt::Raw { opcode, .. } => {
                    let e = if *opcode >= crate::decode::op::FIRST_FUNCTION && self.table.get(*opcode).is_none() {
                        VmError::UnknownOpcode(*opcode)
                    } else {
                        VmError::Raw("statement")
                    };
                    if self.opts.strict {
                        return Err(e);
                    }
                    self.skip(&e);
                }
            }
            if self.host.take_return() {
                return Ok(Flow::Return);
            }
            if let Some(x) = self.host.ext() {
                x.sweep();
            }
            if self.skips > mark && self.skips > self.skips_told {
                self.skips_told = self.skips;
                self.host.skipped_in(&stmts[i]);
            }
            if let Some(n) = self.host.take_wait() {
                if self.no_wait == 0 {
                    self.wait = Some((n, vec![(i, 0)]));
                    return Ok(Flow::Wait);
                }
            }
            i += 1;
        }
        Ok(Flow::Normal)
    }

    /// A chain of stray markers starting at `stmts[start]` (an `ElseIf` or `Else` with
    /// no open chain, see [`Vm::list`]). Returns how it ended and the index of its
    /// closing stray `EndIf` (the list's length when there is none).
    fn stray_chain(&mut self, stmts: &[Stmt], start: usize) -> Result<(Flow, usize), VmError> {
        let next_marker = |from: usize| {
            stmts[from + 1..].iter().position(|s| matches!(s, Stmt::Unmatched { .. })).map_or(stmts.len(), |k| from + 1 + k)
        };
        let mut at = start;
        loop {
            let hold = match &stmts[at] {
                Stmt::Unmatched { marker: Marker::ElseIf, cond: Some(c), .. } => {
                    let v = self.eval(c)?.truthy();
                    if self.host.take_return() {
                        return Ok((Flow::Return, stmts.len()));
                    }
                    v
                }
                // An ElseIf without a condition cannot be tested (none in the masters).
                Stmt::Unmatched { marker: Marker::ElseIf, cond: None, .. } => false,
                Stmt::Unmatched { marker: Marker::Else, .. } => true,
                _ => return Ok((Flow::Normal, at)),
            };
            let next = next_marker(at);
            if hold {
                let f = self.list(&stmts[at + 1..next], false)?;
                if matches!(f, Flow::Return | Flow::Break | Flow::Continue) {
                    return Ok((f, stmts.len()));
                }
                let close = stmts[next..]
                    .iter()
                    .position(|s| matches!(s, Stmt::Unmatched { marker: Marker::EndIf, .. }))
                    .map_or(stmts.len(), |k| next + k);
                return Ok((Flow::Normal, close));
            }
            if next >= stmts.len() {
                return Ok((Flow::Normal, stmts.len()));
            }
            at = next;
        }
    }

    fn call(&mut self, c: &Call) -> Result<Value, VmError> {
        self.step()?;
        if self.table.get(c.opcode).is_none() {
            let e = VmError::UnknownOpcode(c.opcode);
            if self.opts.strict {
                return Err(e);
            }
            self.skip(&e);
            return Ok(Value::ZERO);
        }
        let on = match c.on {
            Some(r) => Some(self.host.ref_value(r).map_err(VmError::Host)?),
            None => None,
        };
        let mut args = Vec::with_capacity(c.args.len());
        for a in &c.args {
            match self.arg(a)? {
                Some(v) => args.push(v),
                None => {
                    let e = VmError::Raw("parameter");
                    if self.opts.strict {
                        return Err(e);
                    }
                    self.skip(&e);
                    return Ok(Value::ZERO);
                }
            }
        }
        self.host.call(c.opcode, on, &args).map_err(VmError::Host)
    }

    fn arg(&mut self, a: &Arg) -> Result<Option<ArgValue>, VmError> {
        Ok(Some(match a {
            Arg::Int(i) => ArgValue::Value(Value::Num(*i as f64)),
            Arg::Float(f) => ArgValue::Value(Value::Num(*f)),
            Arg::Ref(r) => ArgValue::Value(self.host.ref_value(*r).map_err(VmError::Host)?),
            Arg::Var(v) => ArgValue::Value(self.host.get_var(v).map_err(VmError::Host)?),
            Arg::Enum(c) => ArgValue::Enum(*c),
            Arg::Axis(c) => ArgValue::Axis(*c),
            Arg::String(s) => ArgValue::Str(s.clone()),
            Arg::FormatVars(vs) => {
                let mut out = Vec::with_capacity(vs.len());
                for v in vs {
                    match self.arg(v)? {
                        Some(ArgValue::Value(x)) => out.push(x),
                        Some(other) => out.push(other.value().unwrap_or(Value::ZERO)),
                        None => return Ok(None),
                    }
                }
                ArgValue::Vars(out)
            }
            Arg::Int32(i) => ArgValue::Int32(*i),
            Arg::Raw(_) => return Ok(None),
        }))
    }
}

/// A value of the extender's expressions (docs/spec-notes/ext-values.md §1): a number or a
/// form as in vanilla, a string (its text; a `string_var` holds the id of its own copy in
/// the [`ExtStore`]), or an array by id (B3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum XVal {
    Num(f64),
    Ref(u32),
    Str(String),
    Array(u32),
}

impl XVal {
    pub fn from_value(v: Value) -> XVal {
        match v {
            Value::Num(x) => XVal::Num(x),
            Value::Ref(f) => XVal::Ref(f),
        }
    }

    /// What counts as true [documented: ext-values.md §3 for numbers, forms and arrays (an
    /// initialised array, non-zero id); a string is true when not empty, hypothesis].
    pub fn truthy(&self) -> bool {
        match self {
            XVal::Num(x) => *x != 0.0,
            XVal::Ref(f) => *f != 0,
            XVal::Str(s) => !s.is_empty(),
            XVal::Array(id) => *id != 0,
        }
    }

    /// The value as a number where one is needed: a form counts as its FormID and an array
    /// as its id (a number underneath [documented: Array Variable]); a string is a type
    /// error [decision: ext-values.md §2 leaves implicit conversion open; NVSE Error lists
    /// such type errors].
    pub fn num(&self) -> Result<f64, VmError> {
        match self {
            XVal::Num(x) => Ok(*x),
            XVal::Ref(f) => Ok(*f as f64),
            XVal::Array(id) => Ok(*id as f64),
            XVal::Str(_) => Err(VmError::Type("a string where a number is expected (use #)".into())),
        }
    }

    /// The value in a vanilla slot (a number variable, a vanilla expression).
    pub fn value(&self) -> Result<Value, VmError> {
        Ok(match self {
            XVal::Ref(f) => Value::Ref(*f),
            other => Value::Num(other.num()?),
        })
    }
}

/// An event function's request to the host (tasks E1–E3; docs/spec-notes/ext-events.md).
#[derive(Debug, Clone, PartialEq)]
pub enum EventCall {
    /// SetEventHandler (`alt` false) or SetEventHandlerAlt: the event, the handler, the
    /// `key::value` pairs (filters, priority).
    Set { event: String, func: XVal, pairs: Vec<(XVal, XVal)>, alt: bool },
    /// RemoveEventHandler: the event, the handler, the filters given (pairs or plain).
    Remove { event: String, func: XVal, pairs: Vec<(XVal, XVal)> },
    /// DispatchEvent: the event, the info map, the sender name.
    Dispatch { event: String, map: Option<XVal>, sender: Option<String> },
    /// DispatchEventAlt: the event, the calling reference, the arguments.
    DispatchAlt { event: String, on: Option<Value>, args: Vec<XVal> },
    /// GetCurrentEventName.
    CurrentName,
    /// JIP LN's `SetOn<event>EventHandler func add filters…` (`event` lower case).
    JipSet { event: String, func: XVal, add: bool, filters: Vec<XVal> },
    /// JIP LN's SetGameMainLoopCallback.
    MainLoop { func: XVal, add: bool, delay: u64, flags: u32, on: Option<Value> },
}

/// How the host stores a variable (the extender's types live in ordinary slots).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    /// A number or a reference (`short`, `int`, `long`, `float`, `ref`).
    Value,
    /// `string_var`: the slot holds the id of the variable's string in the [`ExtStore`].
    String,
    /// `array_var`: the slot holds an array id (B3).
    Array,
}

/// The extender's strings (and later arrays), by id; ids start at 1, 0 is "none".
/// Owned by the game and kept in its save (task F1).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ExtStore {
    pub strings: BTreeMap<u32, String>,
    pub arrays: BTreeMap<u32, XArray>,
    next: u32,
    /// Arrays made since the last [`ExtStore::sweep`].
    #[serde(skip)]
    fresh: Vec<u32>,
    /// JIP LN's variable stores (`vm/jip.rs`; docs/spec-notes/jip-variable-stores.md):
    /// the saved ones, and the temporary ones (names starting `*`) that are not.
    #[serde(default, skip_serializing_if = "jip::JipStores::is_empty")]
    pub jip: jip::JipStores,
    #[serde(skip)]
    pub jip_temp: jip::JipStores,
}

/// The longest string the extender keeps [documented: String Variable, 16,384
/// characters].
pub const MAX_STRING: usize = 16_384;

impl ExtStore {
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty() && self.arrays.is_empty() && self.jip.is_empty()
    }

    pub fn new_string(&mut self, text: String) -> u32 {
        self.next += 1;
        let id = self.next;
        self.strings.insert(id, clip(text));
        id
    }
    pub fn text(&self, id: u32) -> Option<&str> {
        self.strings.get(&id).map(String::as_str)
    }
    pub fn set_text(&mut self, id: u32, text: String) {
        self.strings.insert(id, clip(text));
    }
    pub fn destroy(&mut self, id: u32) {
        self.strings.remove(&id);
    }
}

fn clip(mut s: String) -> String {
    if s.chars().count() > MAX_STRING {
        s = s.chars().take(MAX_STRING).collect();
    }
    s
}

// ---------------------------------------------------------------------------- arrays (B3)

/// An array's key: a number (lists, maps) or a string (string maps) [documented: Ar Map].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum XKey {
    Num(f64),
    Str(String),
}

impl XKey {
    /// Key order: numbers by value; strings alphabetically without regard to case
    /// [documented: maps keep their keys sorted ascending, Ar First; case: hypothesis, as
    /// for string comparisons].
    fn cmp(&self, o: &XKey) -> std::cmp::Ordering {
        match (self, o) {
            (XKey::Num(a), XKey::Num(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
            (XKey::Str(a), XKey::Str(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
            (XKey::Num(_), XKey::Str(_)) => std::cmp::Ordering::Less,
            (XKey::Str(_), XKey::Num(_)) => std::cmp::Ordering::Greater,
        }
    }
    /// Key order, for code that keeps an array's entries sorted by hand.
    pub fn cmp_key(&self, o: &XKey) -> std::cmp::Ordering {
        self.cmp(o)
    }
    pub fn value(&self) -> XVal {
        match self {
            XKey::Num(x) => XVal::Num(*x),
            XKey::Str(s) => XVal::Str(s.clone()),
        }
    }
}

/// The three kinds of array [documented: Ar Construct; Ar Map].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrayKind {
    /// Keys 0, 1, 2 … without gaps [documented: Ar Erase; Ar Insert].
    List,
    Map,
    StringMap,
}

/// An array: its entries in key order, and how many variables and elements hold it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct XArray {
    pub kind: ArrayKind,
    pub entries: Vec<(XKey, XVal)>,
    pub refs: u32,
}

impl XArray {
    fn find(&self, k: &XKey) -> Result<usize, usize> {
        self.entries.binary_search_by(|(e, _)| e.cmp(k))
    }
    pub fn get(&self, k: &XKey) -> Option<&XVal> {
        self.find(k).ok().map(|i| &self.entries[i].1)
    }
    fn renumber(&mut self) {
        if self.kind == ArrayKind::List {
            for (i, e) in self.entries.iter_mut().enumerate() {
                e.0 = XKey::Num(i as f64);
            }
        }
    }
    /// The bad index of this array's key type [documented: Ar BadNumericIndex;
    /// Ar BadStringIndex].
    pub fn bad_index(&self) -> XVal {
        match self.kind {
            ArrayKind::StringMap => XVal::Str(String::new()),
            _ => XVal::Num(BAD_NUMERIC_INDEX),
        }
    }
}

/// Ar_BadNumericIndex [documented].
pub const BAD_NUMERIC_INDEX: f64 = -99999.0;

impl ExtStore {
    /// A new array no variable holds yet; it is freed at the end of the statement unless
    /// something keeps it ([`ExtStore::sweep`]).
    pub fn new_array(&mut self, kind: ArrayKind, entries: Vec<(XKey, XVal)>) -> u32 {
        self.next += 1;
        let id = self.next;
        for (_, v) in &entries {
            if let XVal::Array(c) = v {
                if let Some(a) = self.arrays.get_mut(c) {
                    a.refs += 1;
                }
            }
        }
        let mut a = XArray { kind, entries, refs: 0 };
        if kind != ArrayKind::List {
            a.entries.sort_by(|x, y| x.0.cmp(&y.0));
            a.entries.dedup_by(|x, y| x.0.cmp(&y.0) == std::cmp::Ordering::Equal);
        }
        a.renumber();
        self.arrays.insert(id, a);
        self.fresh.push(id);
        id
    }
    pub fn array(&self, id: u32) -> Option<&XArray> {
        self.arrays.get(&id)
    }
    pub fn array_mut(&mut self, id: u32) -> Option<&mut XArray> {
        self.arrays.get_mut(&id)
    }
    /// One more holder of the value (an array's count; anything else nothing).
    pub fn retain(&mut self, v: &XVal) {
        if let XVal::Array(id) = v {
            if let Some(a) = self.arrays.get_mut(id) {
                a.refs += 1;
            }
        }
    }
    /// One holder fewer; an array nothing holds any more is freed with what it held
    /// [documented: xNVSE 6 frees arrays by reference counting, ext-save.md].
    pub fn release(&mut self, v: &XVal) {
        let XVal::Array(id) = v else { return };
        let mut stack = vec![*id];
        while let Some(id) = stack.pop() {
            let Some(a) = self.arrays.get_mut(&id) else { continue };
            a.refs = a.refs.saturating_sub(1);
            if a.refs == 0 {
                if let Some(a) = self.arrays.remove(&id) {
                    stack.extend(a.entries.iter().filter_map(|(_, v)| if let XVal::Array(c) = v { Some(*c) } else { None }));
                }
            }
        }
    }
    /// Let go of a hold on `v` without freeing it now: it becomes a temporary of the
    /// running statement (a function's result handed to its caller).
    pub fn hand_over(&mut self, v: &XVal) {
        if let XVal::Array(id) = v {
            if let Some(a) = self.arrays.get_mut(id) {
                a.refs = a.refs.saturating_sub(1);
                self.fresh.push(*id);
            }
        }
    }

    /// Free the arrays made since the last sweep that nothing holds (the temporaries of a
    /// statement), with what only they held.
    pub fn sweep(&mut self) {
        for id in std::mem::take(&mut self.fresh) {
            if let Some(a) = self.arrays.get_mut(&id).filter(|a| a.refs == 0) {
                a.refs = 1;
                self.release(&XVal::Array(id));
            }
        }
    }
}

/// A key as text.
fn key_text(k: &XKey) -> String {
    match k {
        XKey::Num(x) => number_text(*x),
        XKey::Str(s) => format!("{s:?}"),
    }
}

/// Two values the same, as Ar_Find and Ar_Unique judge it [hypothesis: as `==`, strings
/// without regard to case; ext-arrays.md leaves it open].
fn same(a: &XVal, b: &XVal) -> bool {
    match (a, b) {
        (XVal::Str(x), XVal::Str(y)) => x.to_lowercase() == y.to_lowercase(),
        (XVal::Ref(x), XVal::Ref(y)) => x == y,
        (XVal::Array(x), XVal::Array(y)) => x == y,
        (XVal::Num(x), XVal::Num(y)) => x == y,
        _ => false,
    }
}

/// A copy of an array; nested arrays shared (Ar_Copy) or copied all the way down
/// (Ar_DeepCopy) [documented: Ar Copy; Ar DeepCopy].
/// An array that holds itself, directly or through others, is copied with the same shape:
/// an entry that leads back to an array still being copied gets that array's copy (#1089).
/// An array met twice on separate paths (`[b, b]`) is copied twice, so the copies stay
/// apart, as before #1089 [documented: Ar DeepCopy, "all the way down"; hypothesis: the
/// original also copies each entry apart, not probed].
fn copy_array(store: &mut ExtStore, id: u32, deep: bool) -> u32 {
    if !deep {
        let Some(a) = store.array(id).cloned() else { return 0 };
        return store.new_array(a.kind, a.entries);
    }
    deep_copy(store, id, &mut std::collections::HashMap::new())
}

fn deep_copy(store: &mut ExtStore, id: u32, copies: &mut std::collections::HashMap<u32, u32>) -> u32 {
    if let Some(&c) = copies.get(&id) {
        return c;
    }
    let Some(a) = store.array(id).cloned() else { return 0 };
    // The copy exists before its entries, so an entry leading back here finds it.
    let new = store.new_array(a.kind, Vec::new());
    copies.insert(id, new);
    let mut es = Vec::with_capacity(a.entries.len());
    for (k, v) in a.entries {
        let v = match v {
            XVal::Array(c) => XVal::Array(deep_copy(store, c, copies)),
            other => other,
        };
        store.retain(&v);
        es.push((k, v));
    }
    if let Some(n) = store.array_mut(new) {
        n.entries = es;
        n.renumber();
    }
    // Only the arrays on the path being copied stand for themselves.
    copies.remove(&id);
    new
}

/// Same key-element pairs in the same order, nested arrays included; two nulls are equal
/// [documented: Ar DeepEquals].
/// A pair of arrays met again on the way down is taken as equal, so arrays that hold
/// themselves compare without end (#1089).
fn deep_equal(store: &ExtStore, a: &XVal, b: &XVal) -> bool {
    deep_equal_in(store, a, b, &mut std::collections::HashSet::new())
}

fn deep_equal_in(store: &ExtStore, a: &XVal, b: &XVal, seen: &mut std::collections::HashSet<(u32, u32)>) -> bool {
    match (a, b) {
        (XVal::Array(x), XVal::Array(y)) => match (store.array(*x), store.array(*y)) {
            (None, None) => true,
            (Some(_), Some(_)) if !seen.insert((*x, *y)) => true,
            (Some(p), Some(q)) => {
                p.kind == q.kind
                    && p.entries.len() == q.entries.len()
                    && p.entries.iter().zip(&q.entries).all(|(e, f)| e.0 == f.0 && deep_equal_in(store, &e.1, &f.1, seen))
            }
            _ => false,
        },
        (XVal::Num(0.0), XVal::Array(y)) | (XVal::Array(y), XVal::Num(0.0)) => store.array(*y).is_none(),
        _ => a == b,
    }
}

/// A number as `$` and ToString write it: whole numbers without decimals, others with up
/// to six [hypothesis: ext-values.md §2 leaves `$` of a float open; probes G3 strings].
pub fn number_text(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        let s = format!("{x:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// A string read as a number, as `#` and ToNumber read it: surrounding spaces ignored,
/// `0x…` as hex, anything unreadable 0 [documented: ToNumber; the unreadable case
/// hypothesis].
pub fn text_number(s: &str) -> f64 {
    let t = s.trim();
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return hex_prefix(h);
    }
    // The number the text starts with: "12abc" is 12 [observed: probe strings/04].
    let mut end = t.len();
    while end > 0 {
        if let Ok(v) = t[..end].parse::<f64>() {
            return v;
        }
        end = t[..end].char_indices().last().map_or(0, |(i, _)| i);
    }
    0.0
}

/// The hex digits a text starts with, as a number.
fn hex_prefix(h: &str) -> f64 {
    let n = h.chars().take_while(|c| c.is_ascii_hexdigit()).count();
    i64::from_str_radix(&h[..n], 16).map_or(0.0, |v| v as f64)
}

/// The widest field and the most digits a format code writes; a larger width or precision
/// is cut to this, so a mod's `%.4000000000f` cannot build a huge string [design: as #1134
/// asks of the message formatter].
const MAX_FORMAT_FIELD: usize = 512;

/// The most elements an array function makes or grows an array to (Ar_Init, Ar_Range,
/// Ar_Generate, Ar_Resize) [design: a bound against a runaway script, far above what a mod
/// keeps].
const MAX_ELEMENTS: usize = 1_000_000;

/// A number by a printf-style code, as the C library writes it [observed: the game's
/// sv_Construct, probe format1 and jip5 (#1006): `%g` 0.5 "0.5", 2.5 "2.5", 1000000
/// "1e+06", 123.456789 "123.457", 1234567 "1.23457e+06", 10020030 "1.002e+07"; `%G`
/// 1234567 "1.23457E+06"; `%.2g` 123.456 "1.2e+02"; `%.3e` 123000 "1.230e+05"; rules1, a
/// denormal "7.09985e-318"]: `%X.Yf` (width X, Y decimals), `%g`/`%G` (Y significant
/// digits, 6 by default, scientific below 1e-4 or from 10^Y, trailing zeros dropped),
/// `%.Ye`/`%.YE`; switches `+`, space, `-` (left-aligned), `0` (zero padding). The wiki's
/// "%g as an integer, 1E+006" (String Formatting, Vanilla Game Options) differs [hypothesis:
/// the older runtime's form]. The sign is written apart from the digits, so `+` never
/// doubles one; `%f` writes -0 with its sign, as the C library does [observed: nvse.math
/// `qtoeuler.z90`, `%.4f` of QToEuler's -0 Y angle reads "-0.0000" in the original,
/// #1919]; `%g` and `%e` write -0 as 0, as NVForever wrote it before [hypothesis: not
/// probed; the C library writes `-0`]; infinity and NaN write `inf` and `nan` (`INF`, `NAN`
/// for an upper-case code) and are padded with spaces, as the C library does
/// [hypothesis: not probed].
fn printf_number(spec: &str, code: char, x: f64) -> String {
    let flags: String = spec.chars().take_while(|c| "+- 0".contains(*c)).collect();
    let rest = &spec[flags.len()..];
    let (w, p) = match rest.split_once('.') {
        Some((w, p)) => (w, Some(p.parse::<usize>().unwrap_or(0).min(MAX_FORMAT_FIELD))),
        None => (rest, None),
    };
    let width: usize = w.parse::<usize>().unwrap_or(0).min(MAX_FORMAT_FIELD);
    let upper = code.is_ascii_uppercase();
    let negative = x < 0.0 || (x == 0.0 && x.is_sign_negative() && code.eq_ignore_ascii_case(&'f'));
    let a = x.abs();
    // The C library's scientific form of a value at or above 0: the mantissa, then e (or
    // E), a sign and at least two digits.
    let sci = |a: f64, digits: usize| {
        let s = format!("{:.*e}", digits, a);
        match s.split_once('e') {
            Some((m, e)) => {
                let e: i32 = e.parse().unwrap_or(0);
                format!("{m}{}{}{:02}", if upper { 'E' } else { 'e' }, if e < 0 { '-' } else { '+' }, e.abs())
            }
            None => s,
        }
    };
    let trim = |s: String| if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    let finite = a.is_finite();
    let mut body = if !finite {
        let t = if a.is_nan() { "nan" } else { "inf" };
        if upper { t.to_ascii_uppercase() } else { t.to_string() }
    } else {
        match code.to_ascii_lowercase() {
            'f' => format!("{:.*}", p.unwrap_or(6), a),
            'e' => sci(a, p.unwrap_or(6)),
            _ => {
                let p = p.unwrap_or(6).max(1);
                if a == 0.0 {
                    "0".to_string()
                } else {
                    // The exponent the scientific form would have, after rounding to p digits.
                    let e: i64 = format!("{:.*e}", p - 1, a).split_once('e').and_then(|(_, e)| e.parse().ok()).unwrap_or(0);
                    if e < -4 || e >= p as i64 {
                        let s = sci(a, p - 1);
                        match s.split_once(if upper { 'E' } else { 'e' }) {
                            Some((m, rest)) => format!("{}{}{rest}", trim(m.to_string()), if upper { 'E' } else { 'e' }),
                            None => s,
                        }
                    } else {
                        trim(format!("{:.*}", (p as i64 - 1 - e).max(0) as usize, a))
                    }
                }
            }
        }
    };
    let sign = if negative {
        "-"
    } else if flags.contains('+') {
        "+"
    } else if flags.contains(' ') {
        " "
    } else {
        ""
    };
    let len = sign.chars().count() + body.chars().count();
    if len < width {
        let pad = width - len;
        if flags.contains('-') {
            body.push_str(&" ".repeat(pad));
        } else if flags.contains('0') && finite {
            body.insert_str(0, &"0".repeat(pad));
        } else {
            return format!("{}{sign}{body}", " ".repeat(pad));
        }
    }
    format!("{sign}{body}")
}

/// `%q`, `%r` and a bare `%e` in a string literal: a double quote, a line break, nothing
/// [documented: String Formatting, they work in any string literal]. `%%` stays for the
/// format functions.
fn literal_codes(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let cs: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '%' {
            match cs.get(i + 1) {
                Some('%') => {
                    out.push_str("%%");
                    i += 2;
                    continue;
                }
                Some('q') => {
                    out.push('"');
                    i += 2;
                    continue;
                }
                Some('r') => {
                    out.push('\n');
                    i += 2;
                    continue;
                }
                Some('e') => {
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        out.push(cs[i]);
        i += 1;
    }
    out
}

/// The form-type code of a record type [documented: Form Type IDs, docs/spec-notes/ext-forms.md
/// appendix].
pub fn form_type_code(sig: &[u8; 4]) -> Option<u16> {
    const CODES: [&[u8; 4]; 121] = [
        b"NONE", b"TES4", b"GRUP", b"GMST", b"TXST", b"MICN", b"GLOB", b"CLAS", b"FACT", b"HDPT", b"HAIR", b"EYES", b"RACE", b"SOUN", b"ASPC", b"SKIL",
        b"MGEF", b"SCPT", b"LTEX", b"ENCH", b"SPEL", b"ACTI", b"TACT", b"TERM", b"ARMO", b"BOOK", b"CLOT", b"CONT", b"DOOR", b"INGR", b"LIGH", b"MISC",
        b"STAT", b"SCOL", b"MSTT", b"PWAT", b"GRAS", b"TREE", b"FLOR", b"FURN", b"WEAP", b"AMMO", b"NPC_", b"CREA", b"LVLC", b"LVLN", b"KEYM", b"ALCH",
        b"IDLM", b"NOTE", b"COBJ", b"PROJ", b"LVLI", b"WTHR", b"CLMT", b"REGN", b"NAVI", b"CELL", b"REFR", b"ACHR", b"ACRE", b"PMIS", b"PGRE", b"PBEA",
        b"PFLA", b"WRLD", b"LAND", b"NAVM", b"TLOD", b"DIAL", b"INFO", b"QUST", b"IDLE", b"PACK", b"CSTY", b"LSCR", b"LVSP", b"ANIO", b"WATR", b"EFSH",
        b"TOFT", b"EXPL", b"DEBR", b"IMGS", b"IMAD", b"FLST", b"PERK", b"BPTD", b"ADDN", b"AVIF", b"RADS", b"CAMS", b"CPTH", b"VTYP", b"IPCT", b"IPDS",
        b"ARMA", b"ECZN", b"MESG", b"RGDL", b"DOBJ", b"LGTM", b"MUSC", b"IMOD", b"REPU", b"PCBE", b"RCPE", b"RCCT", b"CHIP", b"CSNO", b"LSCT", b"MSET",
        b"ALOC", b"CHAL", b"AMEF", b"CCRD", b"CMNY", b"CDCK", b"DEHY", b"HUNG", b"SLPD",
    ];
    // The table lists ARMA as 96 and ARMO as "armor (armor addons too)": the code table
    // is followed here [documented: Form Type IDs; which an addon reports is open].
    CODES.iter().position(|c| *c == sig).map(|i| i as u16)
}

/// A position in a string: from the end when negative [documented: NVSE Expressions,
/// for slices; for one character, hypothesis].
fn position(i: f64, len: usize) -> Option<usize> {
    let i = i.trunc() as i64;
    let p = if i < 0 { len as i64 + i } else { i };
    (0..len as i64).contains(&p).then_some(p as usize)
}

/// A character for a search without regard to case: its lowercase when that is one
/// character, else itself. Lowering a whole string changes its byte length for some
/// characters (`İ` 2 → 3 bytes, the Kelvin sign 3 → 1), so a position found in a
/// lowered copy is not one in the original; character by character it is [#1989].
fn fold(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

/// The first `needle` in `hay` from character `from`, as a character index; `case`
/// compares as they are, else through [`fold`]. An empty needle is found at `from`.
fn find_chars(hay: &[char], needle: &[char], from: usize, case: bool) -> Option<usize> {
    if needle.is_empty() {
        return (from <= hay.len()).then_some(from);
    }
    let same = |a: &char, b: &char| if case { a == b } else { fold(*a) == fold(*b) };
    (from..=hay.len().checked_sub(needle.len())?).find(|&i| hay[i..i + needle.len()].iter().zip(needle).all(|(a, b)| same(a, b)))
}

impl Vm<'_, '_> {
    /// Set a variable from an extended value: a function's parameter before it runs.
    pub fn bind(&mut self, var: &VarRef, v: XVal) -> Result<(), VmError> {
        self.write_var(var, v)
    }

    fn store(&mut self) -> Result<&mut ExtStore, VmError> {
        self.host.ext().ok_or_else(|| VmError::NotBuilt("the extender's values in this host".into()))
    }

    fn read_var(&mut self, var: &VarRef) -> Result<XVal, VmError> {
        let v = self.host.get_var(var).map_err(VmError::Host)?;
        Ok(match self.host.var_kind(var) {
            VarKind::Value => XVal::from_value(v),
            VarKind::Array => XVal::Array(v.num() as u32),
            VarKind::String => {
                let id = v.num() as u32;
                XVal::Str(self.store()?.text(id).unwrap_or_default().to_string())
            }
        })
    }

    fn write_var(&mut self, var: &VarRef, v: XVal) -> Result<(), VmError> {
        match self.host.var_kind(var) {
            VarKind::Value => {
                let v = v.value()?;
                self.host.set_var(var, v).map_err(VmError::Host)
            }
            // Assigning an array shares it [documented: NVSE Expressions]; Ar_Null (0) lets
            // go of the variable's array [documented: Ar Null].
            VarKind::Array => {
                let new = match v {
                    XVal::Array(id) => id,
                    XVal::Num(0.0) => 0,
                    _ => return Err(VmError::Type("an array variable takes an array".into())),
                };
                let old = self.host.get_var(var).map_err(VmError::Host)?.num() as u32;
                if new != old {
                    let store = self.store()?;
                    store.retain(&XVal::Array(new));
                    store.release(&XVal::Array(old));
                    self.host.set_var(var, Value::Num(new as f64)).map_err(VmError::Host)?;
                }
                Ok(())
            }
            // Assigning a string copies it into the variable's own string [documented: NVSE
            // Expressions].
            VarKind::String => {
                let text = match v {
                    XVal::Str(s) => s,
                    _ => return Err(VmError::Type("a string variable takes a string (use $)".into())),
                };
                let id = self.host.get_var(var).map_err(VmError::Host)?.num() as u32;
                let store = self.store()?;
                if id != 0 && store.text(id).is_some() {
                    store.set_text(id, text);
                } else {
                    let id = store.new_string(text);
                    self.host.set_var(var, Value::Num(id as f64)).map_err(VmError::Host)?;
                }
                Ok(())
            }
        }
    }

    /// Evaluate an extended expression together with its truthiness.
    ///
    /// [observed: 2026-11-23 & 2026-11-24, GSDocMitchellHouse (0x00103DF9), Fallout: New
    /// Vegas 1.4.0.525, xNVSE 6.4.9 with the 10 official base masters, clean-room script
    /// probes `core_05_eval_empty_array_and_string_truth` (`1,0,1,1`),
    /// `core_24_eval_string_var` / `#3679` `core_24_string_var_destruct_zero`
    /// (`eval s`: `1 -> 0` across `sv_Destruct`, `eval sEmp == 1`, `eval (!sEmp) == 0`,
    /// `eval (sEmp && 5) == 1`, `eval a1 == 1`, `eval a2 == 1`, `eval ar_Null == 0`,
    /// `eval (!a1) == 0`, `eval (!aNull) == 1`), and `w4d_03_eval_empty_str_lit`
    /// (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3802 / PR #3803, isolated
    /// clean-room staging copy, read-only `PrintToConsole` probe; zero writes to game
    /// install or Saves:
    /// `PROBE|w4d_03_eval_empty_str_lit|lit0(cmp=1,val=0)|lit1(cmp=1,val=1)|con0Inline(cmp=0,val=-9)|c0=1,c1=1|v0=1,v1=1|saLit0(1:0)|saLit1(1:1)|saV0(1:1)`);
    /// zero DLL, PDB, or third-party source consulted]: an initialised empty array and any
    /// allocated `string_var` (even empty `""` or `sv_Construct "%z" s0`, because its string
    /// ID is non-zero) evaluate truthy (`1`) under both `if eval` and standalone `eval`;
    /// `ar_Null`, an uninitialised or `sv_Destruct`-ed `string_var`, and a bare empty string
    /// literal `""` evaluate `0`, while a non-empty string literal `"abc"` evaluates `1`.
    fn xeval_truthy(&mut self, e: &XExpr) -> Result<(XVal, bool), VmError> {
        if let XExpr::Var { var, .. } = e {
            if self.host.var_kind(var) == VarKind::String {
                let id = self.host.get_var(var).map_err(VmError::Host)?.num() as u32;
                let text = self.store()?.text(id).map(str::to_string);
                let valid = id != 0 && text.is_some();
                return Ok((XVal::Str(text.unwrap_or_default()), valid));
            }
        }
        let v = self.xeval(e)?;
        let truthy = v.truthy();
        Ok((v, truthy))
    }

    /// Evaluate an extended expression (docs/spec-notes/ext-syntax.md, ext-values.md).
    /// Arrays and the extender's own functions are tasks B3 and D, and stop the statement
    /// with `NotBuilt`.
    pub fn xeval(&mut self, e: &XExpr) -> Result<XVal, VmError> {
        Ok(match e {
            XExpr::Num { value, text } => XVal::Num(literal(text, *value)),
            XExpr::Str { s } => XVal::Str(literal_codes(s)),
            // An unset `ref` variable is no form: ToString gives `<NULL>` [observed: probe
            // strings tostring.unset_ref].
            XExpr::Var { var, form } => match self.read_var(var)? {
                XVal::Num(x) if *form => XVal::Ref(Value::Num(x).as_form().unwrap_or(0)),
                v => v,
            },
            XExpr::Ref { r } => XVal::from_value(self.host.ref_value(*r).map_err(VmError::Host)?),
            XExpr::Enum { code } => XVal::Num(*code as f64),
            XExpr::Axis { .. } => return Err(VmError::Expression("an axis outside a parameter".into())),
            XExpr::Member { l, key } => {
                let a = self.xeval(l)?;
                self.element(&a, &XVal::Str(key.clone()))?
            }
            XExpr::Lambda { .. } => self.host.make_lambda(e).map_err(VmError::Host)?,
            XExpr::Call { call } => self.xcall(call)?,
            XExpr::Unary { op: XUnary::Not, e } => {
                XVal::Num(if self.xeval_truthy(e)?.1 { 0.0 } else { 1.0 })
            }
            XExpr::Unary { op, e } => {
                let v = self.xeval(e)?;
                match op {
                    XUnary::Neg => XVal::Num(-v.num()?),
                    XUnary::Not => unreachable!(),
                    XUnary::Stringize => XVal::Str(self.text_of(&v)?),
                    XUnary::Numericize => match v {
                        XVal::Str(s) => XVal::Num(text_number(&s)),
                        other => XVal::Num(other.num()?),
                    },
                    // `&x`: a one-element array; `*a`: a string map's "value", else the first
                    // element [documented: NVSE Expressions].
                    XUnary::Box => {
                        let store = self.store()?;
                        XVal::Array(store.new_array(ArrayKind::List, vec![(XKey::Num(0.0), v)]))
                    }
                    XUnary::Unbox => {
                        let XVal::Array(id) = v else { return Err(VmError::Type("* of something that is not an array".into())) };
                        let store = self.store()?;
                        let a = store.array(id).ok_or_else(|| VmError::Type("* of an empty array variable".into()))?;
                        let got = match a.kind {
                            ArrayKind::StringMap => a.get(&XKey::Str("value".into())).cloned(),
                            _ => a.entries.first().map(|e| e.1.clone()),
                        };
                        got.ok_or_else(|| VmError::Type("* of an empty array".into()))?
                    }
                }
            }
            // `||` and `&&` return the deciding operand's value, a non-number as 1 or 0,
            // and evaluate the right one only when it decides [documented: NVSE
            // Expressions; Eval (short-circuit)].
            XExpr::Binary { op: XOp::Or, l, r } => {
                let (a, at) = self.xeval_truthy(l)?;
                let (d, dt) = if at { (a, at) } else { self.xeval_truthy(r)? };
                XVal::Num(as_operand(&d, dt))
            }
            XExpr::Binary { op: XOp::And, l, r } => {
                let (_, at) = self.xeval_truthy(l)?;
                if !at {
                    XVal::Num(0.0)
                } else {
                    let (b, bt) = self.xeval_truthy(r)?;
                    XVal::Num(as_operand(&b, bt))
                }
            }
            XExpr::Binary { op: XOp::Subscript, l, r } => {
                let s = self.xeval(l)?;
                if let XVal::Array(id) = s {
                    if let XExpr::Binary { op: XOp::Slice, l: lo, r: hi } = r.as_ref() {
                        let (a, b) = (self.xeval(lo)?, self.xeval(hi)?);
                        return self.slice(id, &a, &b);
                    }
                    let k = self.xeval(r)?;
                    return self.element(&XVal::Array(id), &k);
                }
                let XVal::Str(s) = s else { return Err(VmError::Type("[] of something that is neither a string nor an array".into())) };
                let chars: Vec<char> = s.chars().collect();
                // `s[a:b]`: the characters from a to b, both included [documented: NVSE
                // Expressions, Slice]; `s[i]`: one character [documented: Subscript].
                if let XExpr::Binary { op: XOp::Slice, l: lo, r: hi } = r.as_ref() {
                    let (a, b) = (self.xeval(lo)?.num()?, self.xeval(hi)?.num()?);
                    // An end past the string stops at its last character [observed: probe
                    // strings/02, "abcdef"[4:10] is "ef"].
                    let last = chars.len().saturating_sub(1) as f64;
                    let b = if b > last { last } else { b };
                    match (position(a, chars.len()), position(b, chars.len())) {
                        (Some(a), Some(b)) if a <= b => XVal::Str(chars[a..=b].iter().collect()),
                        _ => XVal::Str(String::new()),
                    }
                } else {
                    let i = self.xeval(r)?.num()?;
                    match position(i, chars.len()) {
                        Some(p) => XVal::Str(chars[p].to_string()),
                        None => return Err(VmError::Type(format!("position {i} is outside the string"))),
                    }
                }
            }
            XExpr::Binary { op, l, r } => {
                let (a, b) = (self.xeval(l)?, self.xeval(r)?);
                // Arrays compare by their top-level elements [documented: NVSE Expressions].
                if let (XOp::Eq | XOp::Ne, XVal::Array(x), XVal::Array(y)) = (op, &a, &b) {
                    let store = self.store()?;
                    let same = x == y || store.array(*x).map(|a| &a.entries) == store.array(*y).map(|b| &b.entries);
                    return Ok(XVal::Num(if same == (*op == XOp::Eq) { 1.0 } else { 0.0 }));
                }
                xbinary(*op, a, b)?
            }
            XExpr::Assign { op, target, value } if !matches!(target.as_ref(), XExpr::Var { .. }) => {
                // `let a[k] := v`, `let m->key := v` [documented: Let; NVSE Expressions].
                let (holder, key) = match target.as_ref() {
                    XExpr::Binary { op: XOp::Subscript, l, r } => (self.xeval(l)?, self.xeval(r)?),
                    XExpr::Member { l, key } => (self.xeval(l)?, XVal::Str(key.clone())),
                    _ => return Err(VmError::Type("an assignment to something that is not a variable".into())),
                };
                let v = self.xeval(value)?;
                let v = match op {
                    XAssign::Set => v,
                    other => {
                        let cur = self.element(&holder, &key)?;
                        xbinary(assign_op(*other), cur, v)?
                    }
                };
                self.set_element(&holder, &key, v.clone())?;
                v
            }
            XExpr::Assign { op, target, value } => {
                let XExpr::Var { var, .. } = target.as_ref() else { unreachable!("guarded above") };
                let v = self.xeval(value)?;
                let v = match op {
                    XAssign::Set => v,
                    other => {
                        let cur = self.read_var(var)?;
                        xbinary(assign_op(*other), cur, v)?
                    }
                };
                self.write_var(var, v.clone())?;
                // The value of an assignment is its right-most operand
                // [documented: NVSE Expressions].
                v
            }
        })
    }

    /// A value as text (`$`, ToString): a number's text, a form's name or, without one, its
    /// FormID in hex [documented: ToString].
    /// An array on one line, as PrintVar shows it: `[2, 4]`, a map's pairs `[k: v, …]`. An
    /// array already being written further out is written `array #n`, as Ar_Dump writes a
    /// nested one, so an array holding itself ends (#1089).
    fn brief_array(&mut self, id: u32) -> Result<String, VmError> {
        self.brief_array_in(id, &mut Vec::new())
    }

    fn brief_array_in(&mut self, id: u32, path: &mut Vec<u32>) -> Result<String, VmError> {
        let Some(arr) = self.store()?.array(id).cloned() else { return Ok("(no array)".into()) };
        path.push(id);
        let mut parts = Vec::new();
        for (k, v) in &arr.entries {
            let vt = match v {
                XVal::Array(inner) if path.contains(inner) => format!("array #{inner}"),
                XVal::Array(inner) => self.brief_array_in(*inner, path)?,
                other => self.text_of(other)?,
            };
            parts.push(match (arr.kind, k) {
                (ArrayKind::List, _) => vt,
                (_, XKey::Num(x)) => format!("{}: {vt}", number_text(*x)),
                (_, XKey::Str(s)) => format!("{s}: {vt}"),
            });
        }
        path.pop();
        Ok(format!("[{}]", parts.join(", ")))
    }

    fn text_of(&mut self, v: &XVal) -> Result<String, VmError> {
        Ok(match v {
            XVal::Num(x) => number_text(*x),
            XVal::Str(s) => s.clone(),
            // `<NULL>` for no form and `<no name>` for one without a name [observed: probes
            // strings/04 and udf/03 in the original game, against the page's "its FormID in
            // hex"].
            XVal::Ref(0) => "<NULL>".to_string(),
            XVal::Ref(f) => self.host.form_text(*f).unwrap_or_else(|| "<no name>".to_string()),
            XVal::Array(id) => self.dump(*id),
        })
    }

    /// The string variable a function edits in place: the variable itself, or the one an
    /// assignment in its place sets, after the assignment runs (`Sv_Replace "a|b" (sText =
    /// $rForm)` [hypothesis: AguaFria.esm ships such lines with compiled bytes and edits
    /// sText with the next line, #2275]).
    fn string_var_arg(&mut self, e: Option<&XExpr>) -> Result<Option<crate::ir::VarRef>, VmError> {
        Ok(match e {
            Some(XExpr::Var { var, .. }) => Some(*var),
            Some(a @ XExpr::Assign { target, .. }) => match &**target {
                XExpr::Var { var, .. } => {
                    let var = *var;
                    self.xeval(a)?;
                    Some(var)
                }
                _ => None,
            },
            _ => None,
        })
    }

    fn xcall(&mut self, c: &XCall) -> Result<XVal, VmError> {
        let opcode = match &c.func {
            FuncId::Opcode(o) => *o,
            FuncId::Name(n) => return self.ext_function(n, c),
        };
        self.step()?;
        if self.table.get(opcode).is_none() {
            let e = VmError::UnknownOpcode(opcode);
            if self.opts.strict {
                return Err(e);
            }
            self.skip(&e);
            return Ok(XVal::Num(0.0));
        }
        let on = match &c.on {
            Some(e) => Some(self.xeval(e)?.value()?),
            None => None,
        };
        let mut args = Vec::with_capacity(c.args.len());
        for (k, a) in c.args.iter().enumerate() {
            args.push(match a {
                XExpr::Enum { code } => ArgValue::Enum(*code),
                XExpr::Axis { c } => ArgValue::Axis(*c),
                other => match (self.xeval(other)?, self.table.param_enc(opcode, k)) {
                    (XVal::Str(s), crate::table::ParamEnc::String) => ArgValue::Str(s),
                    // A string in an enumeration's or an axis's place is the name, in any
                    // case; an unknown name is an error, so a `let` leaves its variable
                    // and the script goes on [observed: GetAV ("Strength"), GetAV (s),
                    // GetAV ($s), SetAV ("Luck") 7 and GetPos ("Z") read and set as the
                    // bare names; GetAV ("NoSuchAV") assigned nothing, #2276 job A].
                    (XVal::Str(s), crate::table::ParamEnc::Enum(kind)) => {
                        let code = self.table.enums.get(&kind).and_then(|m| m.iter().find(|(_, n)| n.eq_ignore_ascii_case(&s)).map(|(&c, _)| c));
                        ArgValue::Enum(code.ok_or_else(|| VmError::Type(format!("no {kind} named {s:?}")))?)
                    }
                    (XVal::Str(s), crate::table::ParamEnc::Axis) => match s.to_ascii_uppercase().as_str() {
                        a @ ("X" | "Y" | "Z") => ArgValue::Axis(a.chars().next().unwrap_or('X')),
                        _ => return Err(VmError::Type(format!("no axis named {s:?}"))),
                    },
                    (v, _) => ArgValue::Value(v.value()?),
                },
            });
        }
        Ok(XVal::from_value(self.host.call(opcode, on, &args).map_err(VmError::Host)?))
    }

    /// The key `k` as array `id` takes it: a number for a list or a map, a string for a
    /// string map [documented: Ar Map]; a list's key is a whole position.
    fn key_for(&mut self, id: u32, k: &XVal) -> Result<XKey, VmError> {
        let kind = self.store()?.array(id).map(|a| a.kind).ok_or_else(|| VmError::Type("[] of an empty array variable".into()))?;
        Ok(match (kind, k) {
            (ArrayKind::StringMap, XVal::Str(s)) => XKey::Str(s.clone()),
            (ArrayKind::StringMap, _) => return Err(VmError::Type("a string map takes string keys".into())),
            (_, XVal::Str(_)) => return Err(VmError::Type("a list or a map takes number keys".into())),
            (ArrayKind::List, v) => XKey::Num(v.num()?.trunc()),
            (ArrayKind::Map, v) => XKey::Num(v.num()?),
        })
    }

    /// `a[k]`, `m->k`: a missing key is an error [documented: NVSE Error; TestExpr].
    fn element(&mut self, holder: &XVal, k: &XVal) -> Result<XVal, VmError> {
        let XVal::Array(id) = holder else { return Err(VmError::Type("[] of something that is not an array".into())) };
        let key = self.key_for(*id, k)?;
        let store = self.store()?;
        let a = store.array(*id).ok_or_else(|| VmError::Type("[] of an empty array variable".into()))?;
        a.get(&key).cloned().ok_or_else(|| VmError::MissingElement(key_text(&key)))
    }

    /// `let a[k] := v`: replaces the element, or adds it to a map; a list takes a key up to
    /// its size (the size appends) and a key beyond it is an error [documented: NVSE Error
    /// for beyond; appending at the size hypothesis].
    fn set_element(&mut self, holder: &XVal, k: &XVal, v: XVal) -> Result<(), VmError> {
        let XVal::Array(id) = holder else { return Err(VmError::Type("[] of something that is not an array".into())) };
        let key = self.key_for(*id, k)?;
        let store = self.store()?;
        // Held once the array is there: an empty array variable left `v` held for good [#1990].
        if store.array(*id).is_none() {
            return Err(VmError::Type("[] of an empty array variable".into()));
        }
        store.retain(&v);
        let a = store.array_mut(*id).expect("looked up just above");
        let old = match (a.kind, &key) {
            (ArrayKind::List, XKey::Num(x)) => {
                let len = a.entries.len();
                match *x {
                    i if i >= 0.0 && (i as usize) < len => Some(std::mem::replace(&mut a.entries[i as usize].1, v)),
                    // A key at or past the size appends at the end [observed: probe
                    // arrays/03, `let aA[3] := 9` on a one-element list gives size 2; the page
                    // calls a key beyond the size an error].
                    i if i >= 0.0 => {
                        a.entries.push((XKey::Num(len as f64), v));
                        None
                    }
                    i => {
                        store.release(&v);
                        return Err(VmError::Type(format!("list key {i} is negative")));
                    }
                }
            }
            _ => match a.find(&key) {
                Ok(i) => Some(std::mem::replace(&mut a.entries[i].1, v)),
                Err(i) => {
                    a.entries.insert(i, (key, v));
                    None
                }
            },
        };
        if let Some(o) = old {
            store.release(&o);
        }
        Ok(())
    }

    /// `a[i:j]`: a copy of the elements from key i to key j, both included [documented:
    /// NVSE Expressions]; of the same kind [hypothesis].
    fn slice(&mut self, id: u32, lo: &XVal, hi: &XVal) -> Result<XVal, VmError> {
        let (lo, hi) = (self.key_for(id, lo)?, self.key_for(id, hi)?);
        let store = self.store()?;
        let a = store.array(id).ok_or_else(|| VmError::Type("[] of an empty array variable".into()))?;
        let kind = a.kind;
        let entries: Vec<(XKey, XVal)> = a
            .entries
            .iter()
            .filter(|(k, _)| k.cmp(&lo) != std::cmp::Ordering::Less && k.cmp(&hi) != std::cmp::Ordering::Greater)
            .cloned()
            .collect();
        Ok(XVal::Array(store.new_array(kind, entries)))
    }

    /// An array as text, for Ar_Dump and `$` [decision: the original's print format is
    /// open, ext-arrays.md]: `[k: v, …]`, a nested array by its id.
    fn dump(&mut self, id: u32) -> String {
        let Some(a) = self.host.ext().and_then(|x| x.array(id).cloned()) else { return "(no array)".into() };
        let mut parts = Vec::new();
        for (k, v) in &a.entries {
            let t = match v {
                XVal::Array(c) => format!("array #{c}"),
                XVal::Str(s) => format!("{s:?}"),
                other => self.text_of(other).unwrap_or_default(),
            };
            parts.push(format!("{}: {t}", key_text(k)));
        }
        let kind = match a.kind {
            ArrayKind::List => "array",
            ArrayKind::Map => "map",
            ArrayKind::StringMap => "stringmap",
        };
        format!("{kind} #{id} [{}]", parts.join(", "))
    }

    /// The loop of [`Vm::foreach`] over array `id`, its source held by the caller.
    fn foreach_array(&mut self, var: &VarRef, id: u32, body: &[Stmt]) -> Result<Flow, VmError> {
        let mut i = 0;
        loop {
            self.step()?;
            let Some((k, v)) = self.store()?.array(id).and_then(|a| a.entries.get(i).cloned()) else { return Ok(Flow::Normal) };
            let store = self.store()?;
            let e = store.new_array(ArrayKind::StringMap, vec![(XKey::Str("key".into()), k.value()), (XKey::Str("value".into()), v)]);
            self.write_var(var, XVal::Array(e))?;
            match self.list(body, false)? {
                f @ (Flow::Return | Flow::GoTo(_)) => return Ok(f),
                Flow::Break => return Ok(Flow::Normal),
                Flow::Normal | Flow::Leave | Flow::Continue | Flow::Wait => {}
            }
            i += 1;
        }
    }

    /// `foreach <var> <- <source>` … `loop`. Over an array each entry is a string map with
    /// its "key" and "value"; over a string each character [documented: ForEach]. The
    /// source is read element by element as the loop goes, so erasing during it skips
    /// elements, as documented [documented: Ar Erase]. Form lists and containers: B5.
    fn foreach(&mut self, head: &XForEach, body: &[Stmt]) -> Result<Flow, VmError> {
        let XExpr::Var { var, .. } = &head.var else { return Err(VmError::Type("foreach needs a variable".into())) };
        let src = self.xeval(&head.source)?;
        match src {
            XVal::Array(id) => {
                // The source is held for the loop and let go however it ends: an error in
                // the body (a function on no reference, the step budget) kept it held for
                // ever, swept never and saved [#1990].
                self.store()?.retain(&src);
                let flow = self.foreach_array(var, id, body);
                if let Ok(store) = self.store() {
                    store.release(&src);
                }
                flow
            }
            XVal::Str(s) => {
                for ch in s.chars() {
                    self.step()?;
                    self.write_var(var, XVal::Str(ch.to_string()))?;
                    match self.list(body, false)? {
                        f @ (Flow::Return | Flow::GoTo(_)) => return Ok(f),
                        Flow::Break => break,
                        Flow::Normal | Flow::Leave | Flow::Continue | Flow::Wait => {}
                    }
                }
                Ok(Flow::Normal)
            }
            // A form list's forms, or a container's or actor's items, one per kind of item
            // [documented: ForEach; observed: probe loops/02, as many as GetNumItems]. Each
            // entry is what the host gives: the game makes a temporary reference per kind
            // of item, valid until the next frame (B5) [observed: probe loops/02].
            XVal::Ref(f) => {
                let Some(members) = self.host.members(f) else {
                    return Err(VmError::Type("foreach over a form that is neither a form list nor a container".into()));
                };
                let container = self.host.form_sig(f).is_some_and(|s| &s != b"FLST");
                for m in members {
                    self.step()?;
                    self.write_var(var, XVal::Ref(m))?;
                    match self.list(body, false)? {
                        f @ (Flow::Return | Flow::GoTo(_)) => return Ok(f),
                        Flow::Break => break,
                        Flow::Normal | Flow::Leave | Flow::Continue | Flow::Wait => {}
                    }
                }
                // After a container's loop the variable holds no form, a copy made inside
                // stays valid [observed: nvse-inv foreach.read.after.valid 0, probe
                // loops/02 tempref_after_loop 1].
                if container {
                    self.write_var(var, XVal::Ref(0))?;
                }
                Ok(Flow::Normal)
            }
            _ => Err(VmError::Type("foreach over something that is not an array, a string or a form".into())),
        }
    }

    /// The `ar_*` functions and TypeOf [documented: each page; ext-arrays.md §5].
    fn array_function(&mut self, n: &str, c: &XCall) -> Result<XVal, VmError> {
        let nargs = c.args.len();
        let val = |vm: &mut Self, k: usize| -> Result<XVal, VmError> {
            match c.args.get(k) {
                Some(a) => vm.xeval(a),
                None => Err(VmError::Expression(format!("{n} without argument {}", k + 1))),
            }
        };
        let array_of = |v: &XVal| -> Option<u32> { if let XVal::Array(id) = v { Some(*id).filter(|&i| i != 0) } else { None } };
        let num = |x: f64| Ok(XVal::Num(x));
        match n {
            "ar_construct" => {
                let kind = match val(self, 0)? {
                    XVal::Str(s) if s.eq_ignore_ascii_case("array") => ArrayKind::List,
                    XVal::Str(s) if s.eq_ignore_ascii_case("map") => ArrayKind::Map,
                    XVal::Str(s) if s.eq_ignore_ascii_case("stringmap") => ArrayKind::StringMap,
                    // An unknown kind makes a list [observed: probe arrays/01, size 0].
                    _ => ArrayKind::List,
                };
                Ok(XVal::Array(self.store()?.new_array(kind, Vec::new())))
            }
            "ar_list" => {
                let mut es = Vec::new();
                for k in 0..nargs {
                    es.push((XKey::Num(k as f64), val(self, k)?));
                }
                Ok(XVal::Array(self.store()?.new_array(ArrayKind::List, es)))
            }
            // Pairs `k::v`; the keys' type picks map or string map; form keys or both kinds
            // fail [documented: Ar Map].
            "ar_map" => {
                let mut es = Vec::new();
                let mut kind = None;
                for a in &c.args {
                    let XExpr::Binary { op: XOp::Pair, l, r } = a else { return Err(VmError::Type("Ar_Map takes key::value pairs".into())) };
                    let (k, v) = (self.xeval(l)?, self.xeval(r)?);
                    let (key, kk) = match k {
                        XVal::Num(x) => (XKey::Num(x), ArrayKind::Map),
                        XVal::Str(s) => (XKey::Str(s), ArrayKind::StringMap),
                        _ => return Err(VmError::Type("Ar_Map keys are numbers or strings".into())),
                    };
                    // Keys of another kind than the first are dropped [observed: probe
                    // arrays/01, `Ar_Map 1::10, "x"::20` has size 1].
                    if kind.is_some_and(|k| k != kk) {
                        continue;
                    }
                    kind = Some(kk);
                    es.push((key, v));
                }
                Ok(XVal::Array(self.store()?.new_array(kind.unwrap_or(ArrayKind::Map), es)))
            }
            "ar_null" => num(0.0),
            "ar_badnumericindex" => num(BAD_NUMERIC_INDEX),
            "ar_badstringindex" => Ok(XVal::Str(String::new())),
            "ar_size" => {
                let a = val(self, 0)?;
                let size = array_of(&a).and_then(|id| self.host.ext().and_then(|x| x.array(id)).map(|a| a.entries.len() as f64));
                num(size.unwrap_or(-1.0))
            }
            "ar_packed" => {
                let a = val(self, 0)?;
                let kind = array_of(&a).and_then(|id| self.host.ext().and_then(|x| x.array(id)).map(|a| a.kind));
                num(if kind == Some(ArrayKind::List) { 1.0 } else { 0.0 })
            }
            "ar_haskey" => {
                let (a, k) = (val(self, 0)?, val(self, 1)?);
                let Some(id) = array_of(&a) else { return num(0.0) };
                let key = self.key_for(id, &k)?;
                num(if self.store()?.array(id).is_some_and(|a| a.get(&key).is_some()) { 1.0 } else { 0.0 })
            }
            "ar_keys" => {
                let a = val(self, 0)?;
                let id = array_of(&a).ok_or_else(|| VmError::Type("Ar_Keys of an empty array variable".into()))?;
                let store = self.store()?;
                let keys: Vec<(XKey, XVal)> =
                    store.array(id).map(|a| a.entries.iter().enumerate().map(|(i, (k, _))| (XKey::Num(i as f64), k.value())).collect()).unwrap_or_default();
                Ok(XVal::Array(store.new_array(ArrayKind::List, keys)))
            }
            "ar_first" | "ar_last" | "ar_next" | "ar_prev" => {
                let a = val(self, 0)?;
                let id = array_of(&a).ok_or_else(|| VmError::Type(format!("{n} of an empty array variable")))?;
                let from = if matches!(n, "ar_next" | "ar_prev") { Some(val(self, 1)?) } else { None };
                let from = match from {
                    Some(k) => Some(self.key_for(id, &k)?),
                    None => None,
                };
                let store = self.store()?;
                let arr = store.array(id).ok_or_else(|| VmError::Type(format!("{n} of an empty array")))?;
                let got = match (n, from) {
                    ("ar_first", _) => arr.entries.first().map(|e| e.0.clone()),
                    ("ar_last", _) => arr.entries.last().map(|e| e.0.clone()),
                    // A key the array does not have gives the bad index [observed: probe
                    // arrays/02].
                    (_, Some(k)) if arr.get(&k).is_none() => None,
                    ("ar_next", Some(k)) => arr.entries.iter().find(|e| e.0.cmp(&k) == std::cmp::Ordering::Greater).map(|e| e.0.clone()),
                    (_, Some(k)) => arr.entries.iter().rev().find(|e| e.0.cmp(&k) == std::cmp::Ordering::Less).map(|e| e.0.clone()),
                    _ => None,
                };
                Ok(got.map(|k| k.value()).unwrap_or_else(|| arr.bad_index()))
            }
            "ar_append" => {
                let (a, v) = (val(self, 0)?, val(self, 1)?);
                let id = array_of(&a).ok_or_else(|| VmError::Type("Ar_Append to an empty array variable".into()))?;
                let len = self.store()?.array(id).filter(|a| a.kind == ArrayKind::List).map(|a| a.entries.len()).ok_or_else(|| VmError::Type("Ar_Append takes a list".into()))?;
                self.set_element(&a, &XVal::Num(len as f64), v)?;
                num(0.0)
            }
            "ar_insert" => {
                let (a, i, v) = (val(self, 0)?, val(self, 1)?.num()?, val(self, 2)?);
                let id = array_of(&a).ok_or_else(|| VmError::Type("Ar_Insert into an empty array variable".into()))?;
                let store = self.store()?;
                let ok = match store.array(id) {
                    Some(arr) if arr.kind == ArrayKind::List && i >= 0.0 && i as usize <= arr.entries.len() => true,
                    Some(arr) if arr.kind != ArrayKind::List => return Err(VmError::Type("Ar_Insert takes a list".into())),
                    _ => false,
                };
                if ok {
                    store.retain(&v);
                    if let Some(arr) = store.array_mut(id) {
                        arr.entries.insert(i as usize, (XKey::Num(0.0), v));
                        arr.renumber();
                    }
                }
                num(if ok { 1.0 } else { 0.0 })
            }
            // [documented: Ar Erase] the key, a range `i:j`, or everything; a missing key
            // erases nothing.
            "ar_erase" => {
                let a = val(self, 0)?;
                let id = array_of(&a).ok_or_else(|| VmError::Type("Ar_Erase of an empty array variable".into()))?;
                let range = match c.args.get(1) {
                    None => None,
                    Some(XExpr::Binary { op: XOp::Slice, l, r }) => {
                        let (lo, hi) = (self.xeval(l)?, self.xeval(r)?);
                        Some((self.key_for(id, &lo)?, self.key_for(id, &hi)?))
                    }
                    Some(k) => {
                        let k = self.xeval(k)?;
                        let k = self.key_for(id, &k)?;
                        Some((k.clone(), k))
                    }
                };
                let store = self.store()?;
                let Some(arr) = store.array_mut(id) else { return num(0.0) };
                let mut gone = Vec::new();
                arr.entries.retain(|(k, v)| {
                    let hit = match &range {
                        None => true,
                        Some((lo, hi)) => k.cmp(lo) != std::cmp::Ordering::Less && k.cmp(hi) != std::cmp::Ordering::Greater,
                    };
                    if hit {
                        gone.push(v.clone());
                    }
                    !hit
                });
                arr.renumber();
                for v in &gone {
                    store.release(v);
                }
                num(gone.len() as f64)
            }
            "ar_resize" => {
                let (a, size) = (val(self, 0)?, val(self, 1)?.num()?);
                let pad = if nargs > 2 { val(self, 2)? } else { XVal::Num(0.0) };
                let Some(id) = array_of(&a) else { return num(0.0) };
                let store = self.store()?;
                let Some(arr) = store.array_mut(id).filter(|a| a.kind == ArrayKind::List && size >= 0.0) else { return num(0.0) };
                // At most as many as Ar_Init makes: `Ar_Resize a 1e12` tried to allocate that [#1991].
                let size = (size as usize).min(MAX_ELEMENTS);
                let cut: Vec<XVal> = if arr.entries.len() > size { arr.entries.split_off(size).into_iter().map(|e| e.1).collect() } else { Vec::new() };
                let grow = size.saturating_sub(arr.entries.len());
                for _ in 0..grow {
                    arr.entries.push((XKey::Num(0.0), pad.clone()));
                }
                arr.renumber();
                for _ in 0..grow {
                    store.retain(&pad);
                }
                for v in &cut {
                    store.release(v);
                }
                num(1.0)
            }
            "ar_copy" | "ar_deepcopy" => {
                let a = val(self, 0)?;
                let id = array_of(&a).ok_or_else(|| VmError::Type(format!("{n} of an empty array variable")))?;
                Ok(XVal::Array(copy_array(self.store()?, id, n == "ar_deepcopy")))
            }
            // [documented: Ar Find] optionally within a range of keys.
            "ar_find" => {
                let (v, a) = (val(self, 0)?, val(self, 1)?);
                let id = array_of(&a).ok_or_else(|| VmError::Type("Ar_Find in an empty array variable".into()))?;
                let range = match c.args.get(2) {
                    Some(XExpr::Binary { op: XOp::Slice, l, r }) => {
                        let (lo, hi) = (self.xeval(l)?, self.xeval(r)?);
                        Some((self.key_for(id, &lo)?, self.key_for(id, &hi)?))
                    }
                    _ => None,
                };
                let store = self.store()?;
                let arr = store.array(id).ok_or_else(|| VmError::Type("Ar_Find in an empty array".into()))?;
                let inside = |k: &XKey| range.as_ref().is_none_or(|(lo, hi)| k.cmp(lo) != std::cmp::Ordering::Less && k.cmp(hi) != std::cmp::Ordering::Greater);
                Ok(arr.entries.iter().find(|e| inside(&e.0) && same(&e.1, &v)).map(|e| e.0.value()).unwrap_or_else(|| arr.bad_index()))
            }
            // [documented: Ar InsertRange] Ar_Insert for every element of the other list.
            "ar_insertrange" => {
                let (a, i, b) = (val(self, 0)?, val(self, 1)?.num()?, val(self, 2)?);
                let (Some(id), Some(other)) = (array_of(&a), array_of(&b)) else { return num(0.0) };
                let store = self.store()?;
                let vals: Vec<XVal> = store.array(other).map(|o| o.entries.iter().map(|e| e.1.clone()).collect()).unwrap_or_default();
                let ok = store.array(id).is_some_and(|arr| arr.kind == ArrayKind::List && i >= 0.0 && i as usize <= arr.entries.len());
                if ok {
                    for v in &vals {
                        store.retain(v);
                    }
                    if let Some(arr) = store.array_mut(id) {
                        let at = i as usize;
                        for (j, v) in vals.into_iter().enumerate() {
                            arr.entries.insert(at + j, (XKey::Num(0.0), v));
                        }
                        arr.renumber();
                    }
                }
                num(if ok { 1.0 } else { 0.0 })
            }
            // [documented: Ar CustomSort] `fn` gets two one-element arrays and says whether the
            // first comes before the second; a list.
            "ar_customsort" => {
                let (a, f) = (val(self, 0)?, val(self, 1)?);
                let reverse = nargs > 2 && val(self, 2)?.truthy();
                let id = array_of(&a).ok_or_else(|| VmError::Type("Ar_CustomSort of an empty array variable".into()))?;
                let mut vals: Vec<XVal> = self.store()?.array(id).map(|a| a.entries.iter().map(|e| e.1.clone()).collect()).unwrap_or_default();
                // Insertion sort with the script's test (a test that is not consistent cannot
                // loop forever here).
                for i in 1..vals.len() {
                    let mut j = i;
                    while j > 0 {
                        // Each call of the script's test is a step of this run [#1991].
                        self.step()?;
                        let store = self.store()?;
                        let x = store.new_array(ArrayKind::List, vec![(XKey::Num(0.0), vals[j].clone())]);
                        let y = store.new_array(ArrayKind::List, vec![(XKey::Num(0.0), vals[j - 1].clone())]);
                        let before = self.host.call_function(&f, None, vec![XVal::Array(x), XVal::Array(y)]).map_err(VmError::Host)?.truthy();
                        if !before {
                            break;
                        }
                        vals.swap(j, j - 1);
                        j -= 1;
                    }
                }
                if reverse {
                    vals.reverse();
                }
                let es = vals.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
                Ok(XVal::Array(self.store()?.new_array(ArrayKind::List, es)))
            }
            // [documented: Ar Sort] all of one type, else an empty array; strings without
            // regard to case, numbers by value, forms by FormID; a list.
            "ar_sort" | "ar_sortalpha" => {
                let a = val(self, 0)?;
                let desc = nargs > 1 && val(self, 1)?.truthy();
                let id = array_of(&a).ok_or_else(|| VmError::Type(format!("{n} of an empty array variable")))?;
                let vals: Vec<XVal> = self.store()?.array(id).map(|a| a.entries.iter().map(|e| e.1.clone()).collect()).unwrap_or_default();
                // Ar_SortAlpha puts 9 before 10: numbers sort by value there too, ahead of
                // the rest [observed: probe arrays in the original game, which beats the
                // page's "as text"; numbers first, hypothesis].
                let mut keyed: Vec<(u8, String, f64, XVal)> = Vec::new();
                let alpha = n == "ar_sortalpha";
                // Ar_Sort keeps the elements of the first element's type and drops the rest
                // ([3, "x"] → [3], ["x", 3] → ["x"]) [observed: the original game, which beats
                // the page's "empty array"; with more elements of each type, hypothesis].
                let vals: Vec<XVal> = if alpha {
                    vals
                } else {
                    let first = vals.first().map(std::mem::discriminant);
                    vals.into_iter().filter(|v| Some(std::mem::discriminant(v)) == first).collect()
                };
                let same_kind = true;
                if alpha || same_kind {
                    for v in vals {
                        let is_num = matches!(v, XVal::Num(_));
                        let t = if !is_num && (alpha || matches!(v, XVal::Str(_))) { self.text_of(&v)?.to_lowercase() } else { String::new() };
                        let x = if is_num || !alpha { v.num().unwrap_or(0.0) } else { 0.0 };
                        keyed.push((u8::from(!is_num), t, x, v));
                    }
                    keyed.sort_by(|p, q| p.0.cmp(&q.0).then(p.1.cmp(&q.1)).then(p.2.partial_cmp(&q.2).unwrap_or(std::cmp::Ordering::Equal)));
                    if desc {
                        keyed.reverse();
                    }
                }
                let es = keyed.into_iter().enumerate().map(|(i, (_, _, _, v))| (XKey::Num(i as f64), v)).collect();
                Ok(XVal::Array(self.store()?.new_array(ArrayKind::List, es)))
            }
            // [documented: Ar Range] start to end, both included, `step` apart.
            "ar_range" => {
                let (a, b) = (val(self, 0)?.num()?.trunc(), val(self, 1)?.num()?.trunc());
                let step = if nargs > 2 { val(self, 2)?.num()?.trunc() } else { 1.0 };
                // A step of 0 crashes the original: here no array and a script error on the
                // console [decision: the owner, 2026-10-26].
                if step == 0.0 {
                    self.host.script_error("Ar_Range with a step of 0");
                    return Ok(XVal::Array(0));
                }
                let mut es = Vec::new();
                let mut x = a;
                while (step > 0.0 && x <= b) || (step < 0.0 && x >= b) {
                    es.push((XKey::Num(es.len() as f64), XVal::Num(x)));
                    x += step;
                    if es.len() > MAX_ELEMENTS {
                        break;
                    }
                }
                Ok(XVal::Array(self.store()?.new_array(ArrayKind::List, es)))
            }
            "ar_init" => {
                let (count, v) = (val(self, 0)?.num()?.max(0.0) as usize, val(self, 1)?);
                let es = (0..count.min(MAX_ELEMENTS)).map(|i| (XKey::Num(i as f64), v.clone())).collect();
                Ok(XVal::Array(self.store()?.new_array(ArrayKind::List, es)))
            }
            // [documented: Ar Unique] the first of each value, in its place.
            "ar_unique" => {
                let a = val(self, 0)?;
                let id = array_of(&a).ok_or_else(|| VmError::Type("Ar_Unique of an empty array variable".into()))?;
                let store = self.store()?;
                let arr = store.array(id).cloned().ok_or_else(|| VmError::Type("Ar_Unique of an empty array".into()))?;
                let mut kept: Vec<(XKey, XVal)> = Vec::new();
                for e in arr.entries {
                    if !kept.iter().any(|k| same(&k.1, &e.1)) {
                        kept.push(e);
                    }
                }
                Ok(XVal::Array(store.new_array(arr.kind, kept)))
            }
            "ar_deepequals" => {
                let (a, b) = (val(self, 0)?, val(self, 1)?);
                let store = self.store()?;
                num(if deep_equal(store, &a, &b) { 1.0 } else { 0.0 })
            }
            "ar_dump" => {
                let a = val(self, 0)?;
                let text = match array_of(&a) {
                    Some(id) => self.dump(id),
                    None => "(no array)".into(),
                };
                self.host.print(&text);
                num(0.0)
            }
            // [documented: Ar DumpF] Ar_Dump into a file in the Fallout folder: nothing is
            // written [decision: NVForever writes nothing beside the game].
            "ar_dumpf" => num(0.0),
            // [documented: Ar DumpID] Ar_Dump by the array's id.
            "ar_dumpid" => {
                let id = val(self, 0)?.num()? as u32;
                let text = if self.store()?.array(id).is_some() { self.dump(id) } else { "(no array)".into() };
                self.host.print(&text);
                num(0.0)
            }
            // [documented: TypeOf] "Number", "Form", "String", "Array", "Map", "StringMap".
            "typeof" => {
                let v = match val(self, 0) {
                    Ok(value) => value,
                    Err(VmError::MissingElement(_)) => return Ok(XVal::Str("NULL".into())),
                    Err(error) => return Err(error),
                };
                let t = match &v {
                    XVal::Num(_) => "Number",
                    XVal::Ref(_) => "Form",
                    XVal::Str(_) => "String",
                    XVal::Array(id) => match self.store()?.array(*id).map(|a| a.kind) {
                        Some(ArrayKind::Map) => "Map",
                        Some(ArrayKind::StringMap) => "StringMap",
                        _ => "Array",
                    },
                };
                Ok(XVal::Str(t.into()))
            }
            // The functions that run a function per element: it gets one array per
            // element, `*it` the value and `it["key"]` the key [documented: Ar Filter; Ar
            // MapTo; Ar ForEach; Ar Any; Ar All].
            "ar_foreach" | "ar_filter" | "ar_mapto" | "ar_any" | "ar_all" | "ar_findwhere" => {
                let (a, f) = (val(self, 0)?, val(self, 1)?);
                let id = array_of(&a).ok_or_else(|| VmError::Type(format!("{n} of an empty array variable")))?;
                let kind = self.store()?.array(id).map(|a| a.kind).unwrap_or(ArrayKind::List);
                let entries: Vec<(XKey, XVal)> = self.store()?.array(id).map(|a| a.entries.clone()).unwrap_or_default();
                let mut kept = Vec::new();
                for (k, v) in entries {
                    // Each call of the function is a step of this run: a million elements
                    // called it a million times on no budget [#1991].
                    self.step()?;
                    let store = self.store()?;
                    let it = store.new_array(ArrayKind::StringMap, vec![(XKey::Str("key".into()), k.value()), (XKey::Str("value".into()), v.clone())]);
                    let r = self.host.call_function(&f, None, vec![XVal::Array(it)]).map_err(VmError::Host)?;
                    match n {
                        "ar_filter" if r.truthy() => kept.push((k, v)),
                        "ar_mapto" => kept.push((k, r)),
                        "ar_any" if r.truthy() => return num(1.0),
                        "ar_all" if !r.truthy() => return num(0.0),
                        "ar_findwhere" if r.truthy() => return Ok(v),
                        _ => {}
                    }
                }
                match n {
                    "ar_filter" | "ar_mapto" => Ok(XVal::Array(self.store()?.new_array(kind, kept))),
                    "ar_all" => num(1.0),
                    _ => num(0.0),
                }
            }
            // [documented: Ar Generate] `count` values from `valueFn`, keys from `keyFn` when
            // given (its first key's type picks the kind).
            "ar_generate" => {
                let (count, vf) = (val(self, 0)?.num()?.max(0.0) as usize, val(self, 1)?);
                let kf = if nargs > 2 { Some(val(self, 2)?) } else { None };
                let mut es = Vec::new();
                let mut kind = ArrayKind::List;
                for i in 0..count.min(MAX_ELEMENTS) {
                    self.step()?;
                    let v = self.host.call_function(&vf, None, Vec::new()).map_err(VmError::Host)?;
                    let key = match &kf {
                        None => XKey::Num(i as f64),
                        Some(kf) => match self.host.call_function(kf, None, Vec::new()).map_err(VmError::Host)? {
                            XVal::Num(x) => {
                                if i == 0 {
                                    kind = ArrayKind::Map;
                                }
                                XKey::Num(x)
                            }
                            XVal::Str(s) => {
                                if i == 0 {
                                    kind = ArrayKind::StringMap;
                                }
                                XKey::Str(s)
                            }
                            _ => return num(0.0),
                        },
                    };
                    es.push((key, v));
                }
                Ok(XVal::Array(self.store()?.new_array(kind, es)))
            }
            // The plugins' own array functions are the host's (JohnnyGuitar's Ar_Shuffle,
            // Ar_SortEditor; SUP's Ar_GetRandomKey …); one that writes script variables
            // (SUP's Ar_SetFloatsFrom) goes as the other output functions do.
            _ => {
                if let Some(outs) = out_params(n) {
                    return self.out_call(n, c, outs);
                }
                let mut args = Vec::with_capacity(nargs);
                for k in 0..nargs {
                    args.push(val(self, k)?);
                }
                match self.host.ext_call(n, None, &args) {
                    Some(r) => r.map_err(VmError::Host),
                    None => Err(VmError::NotBuilt(format!("the extender function {n} (task D2)"))),
                }
            }
        }
    }

    /// How many values a format string takes: one per code that takes one
    /// [documented: String Formatting, the count must match exactly].
    fn format_values(fmt: &str) -> usize {
        let cs: Vec<char> = fmt.chars().collect();
        let mut n = 0;
        let mut i = 0;
        while i < cs.len() {
            if cs[i] != '%' {
                i += 1;
                continue;
            }
            i += 1;
            while i < cs.len() && "+- 0123456789.".contains(cs[i]) {
                i += 1;
            }
            match cs.get(i) {
                Some('f' | 'g' | 'G' | 'a' | 'i' | 'k' | 'n' | 'v' | 'x' | 'z' | '{') => n += 1,
                Some('c') => n += 2,
                Some('p') => {
                    n += 1;
                    i += 1;
                }
                Some('e' | 'E') if cs[i - 1] != '%' => n += 1,
                _ => {}
            }
            i += 1;
        }
        n
    }

    /// Expand a format string with its values [documented: String Formatting; ext-format.md
    /// §2–3]. `%B`/`%b` (console colours) write nothing here [decision: NVForever's console
    /// has no colours]; `%k` writes the scancode's number [hypothesis until the scancode
    /// table is in the export]; `%c` and the `%p` pronouns need form data (task D4) and
    /// write nothing yet.
    fn format(&mut self, fmt: &str, values: &[XVal]) -> Result<String, VmError> {
        let cs: Vec<char> = fmt.chars().collect();
        let mut out = String::new();
        let mut vals = values.iter();
        let mut next = |what: &str| vals.next().cloned().ok_or_else(|| VmError::Type(format!("a format string's {what} has no value")));
        let mut skip_depth = 0usize;
        let mut i = 0;
        while i < cs.len() {
            let c = cs[i];
            if c != '%' {
                if skip_depth == 0 {
                    out.push(c);
                }
                i += 1;
                continue;
            }
            i += 1;
            let start = i;
            while i < cs.len() && "+- 0123456789.".contains(cs[i]) {
                i += 1;
            }
            let spec: String = cs[start..i].iter().collect();
            let Some(&code) = cs.get(i) else {
                return Err(VmError::Type("a format string ends with %".into()));
            };
            i += 1;
            let text = match code {
                '%' => "%".to_string(),
                'q' => "\"".to_string(),
                'r' => "\n".to_string(),
                'B' | 'b' => String::new(),
                'e' | 'E' if spec.is_empty() => String::new(),
                'f' | 'g' | 'e' | 'G' | 'E' => printf_number(&spec, code, next("number")?.num()?),
                'a' => char::from_u32(next("character")?.num()? as u32).map(String::from).unwrap_or_default(),
                'i' => {
                    let v = next("form")?;
                    format!("{:08X}", v.value()?.as_form().unwrap_or(0))
                }
                'n' => {
                    let v = next("form")?;
                    let f = v.value()?.as_form().unwrap_or(0);
                    self.host.form_text(f).unwrap_or_default()
                }
                'v' => {
                    let code = next("actor value")?.num()? as u16;
                    self.table.enum_name("actor_value", code).unwrap_or_default().to_string()
                }
                'x' => {
                    // `%x4`: the digit after the code is the width [documented].
                    let mut width = 0;
                    if let Some(d) = cs.get(i).and_then(|d| d.to_digit(10)) {
                        width = d as usize;
                        i += 1;
                    }
                    format!("{:0width$X}", next("number")?.num()? as i64)
                }
                'z' => match next("string")? {
                    XVal::Str(s) => s,
                    other => self.text_of(&other)?,
                },
                // The key's name, as GetKeyName gives it [documented: String Formatting,
                // `%k`, a key's description; hypothesis: GetKeyName's text; observed: MCM's
                // key options print their key with `%k`, #2299].
                'k' => crate::keys::key_name(next("key")?.num()? as i64).to_string(),
                'c' => {
                    next("form")?;
                    next("index")?;
                    String::new()
                }
                'p' => {
                    i += 1;
                    next("form")?;
                    String::new()
                }
                // `%{ … %}`: left out when the value is 0 [documented].
                '{' => {
                    if skip_depth > 0 || !next("condition")?.truthy() {
                        skip_depth += 1;
                    }
                    String::new()
                }
                '}' => {
                    skip_depth = skip_depth.saturating_sub(1);
                    String::new()
                }
                other => format!("%{spec}{other}"),
            };
            if skip_depth == 0 {
                out.push_str(&text);
            }
        }
        Ok(out)
    }

    /// The string of a format argument at `k` and its values, and the index after them.
    fn format_arg(&mut self, c: &XCall, k: usize) -> Result<(String, usize), VmError> {
        let fmt = match c.args.get(k) {
            Some(a) => self.xeval(a)?,
            None => return Err(VmError::Expression("a format string is missing".into())),
        };
        let fmt = match fmt {
            XVal::Str(s) => s,
            other => self.text_of(&other)?,
        };
        let n = Self::format_values(&fmt);
        let mut vals = Vec::with_capacity(n);
        for j in 0..n {
            match c.args.get(k + 1 + j) {
                Some(a) => vals.push(self.xeval(a)?),
                None => return Err(VmError::Type(format!("the format string takes {n} values"))),
            }
        }
        Ok((self.format(&fmt, &vals)?, k + 1 + n))
    }

    /// The `sv_*` functions, the string helpers and the version reports (tasks D1, D4, C5).
    fn string_function(&mut self, n: &str, c: &XCall) -> Result<Option<XVal>, VmError> {
        let val = |vm: &mut Self, k: usize| -> Result<XVal, VmError> {
            match c.args.get(k) {
                Some(a) => vm.xeval(a),
                None => Err(VmError::Expression(format!("{n} without argument {}", k + 1))),
            }
        };
        let opt = |vm: &mut Self, k: usize| -> Result<Option<XVal>, VmError> { c.args.get(k).map(|a| vm.xeval(a)).transpose() };
        let text = |v: XVal| -> Option<String> { if let XVal::Str(s) = v { Some(s) } else { None } };
        Ok(Some(match n {
            // [documented: Sv Construct] a new string from the format and its values.
            "sv_construct" => XVal::Str(self.format_arg(c, 0)?.0),
            // [documented: Sv Length] -1 for an invalid string.
            "sv_length" => {
                // A destructed or never-set string variable is not a valid string: -1
                // [documented: Sv Length; observed: probe strings, a destructed string].
                if let Some(XExpr::Var { var, .. }) = c.args.first() {
                    if self.host.var_kind(var) == VarKind::String {
                        let id = self.host.get_var(var).map_err(VmError::Host)?.num() as u32;
                        if self.store()?.text(id).is_none() {
                            return Ok(Some(XVal::Num(-1.0)));
                        }
                    }
                }
                XVal::Num(text(val(self, 0)?).map_or(-1.0, |s| s.chars().count() as f64))
            }
            // [documented: Sv Count; observed: probe strings, matches do not overlap] as
            // Sv_Find, the number of matches.
            "sv_count" => {
                let (needle, k) = self.format_arg(c, 0)?;
                let hay = text(val(self, k)?).unwrap_or_default();
                let start = opt(self, k + 1)?.map(|v| v.num()).transpose()?.unwrap_or(0.0).max(0.0) as usize;
                let len = opt(self, k + 2)?.map(|v| v.num()).transpose()?;
                let case = opt(self, k + 3)?.is_some_and(|v| v.truthy());
                let chars: Vec<char> = hay.chars().collect();
                let end = len.map_or(chars.len(), |l| (start + l.max(0.0) as usize).min(chars.len()));
                let region: String = chars.get(start..end).map(|s| s.iter().collect()).unwrap_or_default();
                if needle.is_empty() {
                    return Ok(Some(XVal::Num(0.0)));
                }
                let (region, needle): (Vec<char>, Vec<char>) = (region.chars().collect(), needle.chars().collect());
                let (mut n, mut at) = (0usize, 0usize);
                while let Some(i) = find_chars(&region, &needle, at, case) {
                    n += 1;
                    at = i + needle.len();
                }
                XVal::Num(n as f64)
            }
            // [documented: Sv Destruct] the variable's string goes; it reads 0 after.
            "sv_destruct" => {
                for a in &c.args {
                    if let XExpr::Var { var, .. } = a {
                        let id = self.host.get_var(var).map_err(VmError::Host)?.num() as u32;
                        self.store()?.destroy(id);
                        self.host.set_var(var, Value::ZERO).map_err(VmError::Host)?;
                    }
                }
                XVal::Num(0.0)
            }
            // [documented: Sv Find] the first match from startPos within numChars, without
            // regard to case unless asked; -1 when absent.
            "sv_find" => {
                let (needle, k) = self.format_arg(c, 0)?;
                let hay = text(val(self, k)?).unwrap_or_default();
                let start = opt(self, k + 1)?.map(|v| v.num()).transpose()?.unwrap_or(0.0).max(0.0) as usize;
                let len = opt(self, k + 2)?.map(|v| v.num()).transpose()?;
                let case = opt(self, k + 3)?.is_some_and(|v| v.truthy());
                let chars: Vec<char> = hay.chars().collect();
                let end = len.map_or(chars.len(), |l| (start + l.max(0.0) as usize).min(chars.len()));
                let region = chars.get(start..end).unwrap_or_default();
                let needle: Vec<char> = needle.chars().collect();
                let found = find_chars(region, &needle, 0, case);
                XVal::Num(found.map_or(-1.0, |i| (start + i) as f64))
            }
            // [documented: Sv Replace] "old|new", in place, every match without regard to
            // case by default; the number replaced.
            "sv_replace" => {
                let (pair, k) = self.format_arg(c, 0)?;
                let Some(var) = self.string_var_arg(c.args.get(k))? else { return Err(VmError::Type("Sv_Replace needs a string variable".into())) };
                let var = &var;
                let s = text(self.read_var(var)?).unwrap_or_default();
                let start = opt(self, k + 1)?.map(|v| v.num()).transpose()?.unwrap_or(0.0).max(0.0) as usize;
                let case = opt(self, k + 3)?.is_some_and(|v| v.truthy());
                let max = opt(self, k + 4)?.map(|v| v.num()).transpose()?.map(|m| m as usize);
                let (old, new) = pair.split_once('|').unwrap_or((pair.as_str(), ""));
                // An empty old text crashes the original: here nothing is replaced, 0, and a
                // script error on the console [decision: the owner, 2026-10-26].
                if old.is_empty() {
                    self.host.script_error("Sv_Replace with an empty text to replace");
                    return Ok(Some(XVal::Num(0.0)));
                }
                let chars: Vec<char> = s.chars().collect();
                let old: Vec<char> = old.chars().collect();
                let from = start.min(chars.len());
                let (mut out, mut count, mut at): (String, usize, usize) = (chars[..from].iter().collect(), 0, from);
                while max.is_none_or(|m| count < m) {
                    let Some(i) = find_chars(&chars, &old, at, case) else { break };
                    out.extend(&chars[at..i]);
                    out.push_str(new);
                    at = i + old.len();
                    count += 1;
                }
                out.extend(&chars[at..]);
                self.write_var(var, XVal::Str(out))?;
                XVal::Num(count as f64)
            }
            // [documented: Sv Insert] the text into the variable at the position (past the
            // end: at the end [hypothesis]).
            "sv_insert" => {
                let (ins, k) = self.format_arg(c, 0)?;
                let Some(var) = self.string_var_arg(c.args.get(k))? else { return Err(VmError::Type("Sv_Insert needs a string variable".into())) };
                let var = &var;
                let s = text(self.read_var(var)?).unwrap_or_default();
                let at = opt(self, k + 1)?.map(|v| v.num()).transpose()?.unwrap_or(0.0).max(0.0) as usize;
                let chars: Vec<char> = s.chars().collect();
                // Past the end the string is left as it is [observed: probe strings/07
                // insert.past_end].
                if at > chars.len() {
                    return Ok(Some(XVal::Num(0.0)));
                }
                let out: String = chars[..at].iter().collect::<String>() + &ins + &chars[at..].iter().collect::<String>();
                self.write_var(var, XVal::Str(out))?;
                XVal::Num(0.0)
            }
            // [documented: Sv Erase] `count` characters from the position, to the end when
            // left out or past it.
            "sv_erase" => {
                let Some(var) = self.string_var_arg(c.args.first())? else { return Err(VmError::Type("Sv_Erase needs a string variable".into())) };
                let var = &var;
                let s = text(self.read_var(var)?).unwrap_or_default();
                let at = opt(self, 1)?.map(|v| v.num()).transpose()?.unwrap_or(0.0).max(0.0) as usize;
                let n = opt(self, 2)?.map(|v| v.num()).transpose()?;
                let chars: Vec<char> = s.chars().collect();
                let at = at.min(chars.len());
                let end = n.map_or(chars.len(), |n| (at + n.max(0.0) as usize).min(chars.len()));
                let out: String = chars[..at].iter().chain(&chars[end..]).collect();
                self.write_var(var, XVal::Str(out))?;
                XVal::Num(0.0)
            }
            // [documented: Sv SubString] `count` characters from the position.
            "sv_substring" => {
                let s = text(val(self, 0)?).unwrap_or_default();
                let at = opt(self, 1)?.map(|v| v.num()).transpose()?.unwrap_or(0.0).max(0.0) as usize;
                let n = opt(self, 2)?.map(|v| v.num()).transpose()?;
                let chars: Vec<char> = s.chars().collect();
                let at = at.min(chars.len());
                let end = n.map_or(chars.len(), |n| (at + n.max(0.0) as usize).min(chars.len()));
                XVal::Str(chars[at..end].iter().collect())
            }
            // [documented: Sv ToUpper; Sv ToLower] a new string; the source is kept.
            "sv_toupper" => XVal::Str(text(val(self, 0)?).unwrap_or_default().to_uppercase()),
            "sv_tolower" => XVal::Str(text(val(self, 0)?).unwrap_or_default().to_lowercase()),
            // [documented: Sv Percentify] every `%` doubled.
            "sv_percentify" => XVal::Str(text(val(self, 0)?).unwrap_or_default().replace('%', "%%")),
            // [documented: Sv Compare; observed: probe strings/03] 0 when equal, 1 when the
            // format text sorts after the variable's, -1 before; without regard to case unless
            // the flag is set.
            "sv_compare" => {
                let (a, k) = self.format_arg(c, 0)?;
                let b = text(val(self, k)?).unwrap_or_default();
                let case = opt(self, k + 1)?.is_some_and(|v| v.truthy());
                let ord = if case { a.cmp(&b) } else { a.to_lowercase().cmp(&b.to_lowercase()) };
                XVal::Num(match ord {
                    std::cmp::Ordering::Less => -1.0,
                    std::cmp::Ordering::Equal => 0.0,
                    std::cmp::Ordering::Greater => 1.0,
                })
            }
            // [documented: Sv Split] the pieces between delimiter characters; with the flag
            // the whole delimiter string separates. Empty pieces are dropped [observed: probe
            // strings/03, "a,,b" gives 2].
            "sv_split" => {
                let s = text(val(self, 0)?).unwrap_or_default();
                let d = text(val(self, 1)?).unwrap_or_default();
                let whole = opt(self, 2)?.is_some_and(|v| v.truthy());
                let pieces: Vec<String> = if d.is_empty() {
                    vec![s]
                } else if whole {
                    s.split(d.as_str()).filter(|p| !p.is_empty()).map(str::to_string).collect()
                } else {
                    s.split(|ch| d.contains(ch)).filter(|p| !p.is_empty()).map(str::to_string).collect()
                };
                let es = pieces.into_iter().enumerate().map(|(i, p)| (XKey::Num(i as f64), XVal::Str(p))).collect();
                XVal::Array(self.store()?.new_array(ArrayKind::List, es))
            }
            // [documented: Sv ToNumeric] sign, digits and exponent, up to the first other
            // character.
            "sv_tonumeric" => {
                let s = text(val(self, 0)?).unwrap_or_default();
                let t = s.trim_start();
                let mut end = 0;
                for (i, ch) in t.char_indices() {
                    let ok = ch.is_ascii_digit() || ch == '.' || ((ch == '-' || ch == '+') && (i == 0 || t[..i].ends_with(['e', 'E']))) || ((ch == 'e' || ch == 'E') && i > 0);
                    if !ok {
                        break;
                    }
                    end = i + ch.len_utf8();
                }
                let mut num = &t[..end];
                while !num.is_empty() && num.parse::<f64>().is_err() {
                    num = &num[..num.len() - 1];
                }
                XVal::Num(num.parse().unwrap_or(0.0))
            }
            // [documented: Sv GetChar] the character's ASCII code.
            "sv_getchar" => {
                let s = text(val(self, 0)?).unwrap_or_default();
                let p = val(self, 1)?.num()?;
                XVal::Num(position(p, s.chars().count()).and_then(|p| s.chars().nth(p)).map_or(-1.0, |ch| ch as u32 as f64))
            }
            // [documented: NumToHex] upper-case hex, zero-padded to the width (default 8, at
            // most 8), `0x` with the flag.
            "numtohex" | "inttohex" => {
                let v = val(self, 0)?.num()? as i64 as u32;
                let width = opt(self, 1)?.map(|w| w.num()).transpose()?.unwrap_or(8.0).clamp(0.0, 8.0) as usize;
                let prefix = opt(self, 2)?.is_some_and(|p| p.truthy());
                XVal::Str(format!("{}{v:0width$X}", if prefix { "0x" } else { "" }))
            }
            // [documented: GetFormIDString] the form ID in hex.
            "getformidstring" => {
                let v = val(self, 0)?;
                XVal::Str(format!("{:08X}", v.value()?.as_form().unwrap_or(0)))
            }
            // [documented: PrintToConsole; Print] to the console.
            "printtoconsole" | "printc" => {
                let (t, _) = self.format_arg(c, 0)?;
                self.host.print(&t);
                XVal::Num(0.0)
            }
            // [documented: DebugPrint; SetDebugMode, ext-format.md §4] PrintC only while the
            // calling mod's debug mode is on.
            "debugprint" => {
                let (t, _) = self.format_arg(c, 0)?;
                if self.host.debug_mode(None, None) {
                    self.host.print(&t);
                }
                XVal::Num(0.0)
            }
            // [documented: PrintDebug] Print, only while the calling mod's debug mode is on.
            // `var [var …]` (up to 15): each as `name: value` on a line of its own
            // [documented: PrintVar, "fMyVariable: 2.5", "aMyVariable: [2, 4]"];
            // DebugPrintVar only with the calling mod's debug mode on [documented:
            // DebugPrintVar]. Numbers as Print gives them; an array as its elements' texts
            // in brackets [documented: the page's list example; hypothesis: a map shows
            // `key: value` pairs].
            "printvar" | "printv" | "debugprintvar" | "dprintv" => {
                if n.starts_with('d') && !self.host.debug_mode(None, None) {
                    return Ok(Some(XVal::Num(0.0)));
                }
                for a in &c.args {
                    let name = match a {
                        XExpr::Var { var, .. } => self.host.var_name(var),
                        _ => None,
                    };
                    let v = self.xeval(a)?;
                    let t = match v {
                        XVal::Array(id) => self.brief_array(id)?,
                        v => self.text_of(&v)?,
                    };
                    self.host.print(&format!("{}: {t}", name.unwrap_or_else(|| "?".into())));
                }
                XVal::Num(0.0)
            }
            "printdebug" | "printd" => {
                let v = val(self, 0)?;
                let t = self.text_of(&v)?;
                if self.host.debug_mode(None, None) {
                    self.host.print(&t);
                }
                XVal::Num(0.0)
            }
            // [documented: GetDebugMode] the calling mod's debug mode, or the mod index's;
            // 0 from a runtime script, which belongs to no mod [observed: nvse-sixth].
            "getdebugmode" | "getdbmode" => {
                let plugin = opt(self, 0)?.map(|v| v.num()).transpose()?.map(|x| x as u32);
                XVal::Num(f64::from(u8::from(self.host.debug_mode(None, plugin))))
            }
            // [documented: Assert] a false assertion is reported as a script error; the
            // script goes on [observed: nvse-sixth, the step after Assert 0 still ran].
            "assert" => {
                if !val(self, 0)?.truthy() {
                    self.host.script_error("Assert failed");
                }
                XVal::Num(0.0)
            }
            "setdebugmode" => {
                let on = val(self, 0)?.truthy();
                let plugin = opt(self, 1)?.map(|v| v.num()).transpose()?.map(|x| x as u32);
                self.host.debug_mode(Some(on), plugin);
                XVal::Num(0.0)
            }
            // [documented: MessageEx] a corner message without a Message form.
            "messageex" => {
                let (t, _) = self.format_arg(c, 0)?;
                self.host.show_text(&t, Vec::new());
                XVal::Num(0.0)
            }
            // [documented: MessageBoxEx] `text|button|…`, up to 10 buttons numbered from 0;
            // without one the box gets "Ok" [hypothesis: a box needs a button to close].
            "messageboxex" => {
                let (t, _) = self.format_arg(c, 0)?;
                let mut parts = t.split('|').map(str::to_string);
                let text = parts.next().unwrap_or_default();
                let mut buttons: Vec<String> = parts.take(10).collect();
                if buttons.is_empty() {
                    buttons.push("Ok".into());
                }
                self.host.show_text(&text, buttons);
                XVal::Num(0.0)
            }
            "print" => {
                let v = val(self, 0)?;
                let t = self.text_of(&v)?;
                self.host.print(&t);
                XVal::Num(0.0)
            }
            // [documented: GetObjectType / GetType] the form-type code of a base form, or of
            // a reference's base form (the codes: docs/spec-notes/ext-forms.md).
            "getobjecttype" | "gettype" => {
                let f = match c.args.first() {
                    Some(a) => self.xeval(a)?.value()?.as_form(),
                    None => match &c.on {
                        Some(e) => self.xeval(e)?.value()?.as_form(),
                        None => None,
                    },
                };
                let Some(mut f) = f else { return Ok(Some(XVal::Num(0.0))) };
                if self.host.form_sig(f).is_some_and(|s| matches!(&s, b"REFR" | b"ACHR" | b"ACRE" | b"PGRE" | b"PMIS" | b"PBEA" | b"PFLA")) {
                    f = self.host.base_form(f).unwrap_or(f);
                }
                XVal::Num(self.host.form_sig(f).and_then(|s| form_type_code(&s)).unwrap_or(0) as f64)
            }
            // [documented: IsFormValid] a reference or base form that is there; every form
            // of the load order is in memory here [hypothesis: NVForever keeps no form out
            // of memory].
            "isformvalid" => {
                let v = val(self, 0)?;
                let ok = v.value()?.as_form().is_some_and(|f| self.host.form_sig(f).is_some());
                XVal::Num(if ok { 1.0 } else { 0.0 })
            }
            // [documented: ListGetCount; GetNumItems] the forms of a list; the kinds of item
            // a container or actor holds [observed: probe loops/02, as many as foreach
            // gives].
            "listgetcount" | "getnumitems" => {
                let f = match (c.args.first(), &c.on) {
                    (Some(a), _) => self.xeval(a)?.value()?.as_form(),
                    (None, Some(e)) => self.xeval(e)?.value()?.as_form(),
                    (None, None) => self.host.calling_ref(),
                };
                XVal::Num(f.and_then(|f| self.host.members(f)).map_or(0, |m| m.len()) as f64)
            }
            // [documented: GetBaseObject] the base form of the reference it is called on.
            "getbaseobject" => {
                let f = match (&c.on, c.args.first()) {
                    (Some(e), _) => self.xeval(e)?.value()?.as_form(),
                    (None, Some(a)) => self.xeval(a)?.value()?.as_form(),
                    (None, None) => self.host.calling_ref(),
                };
                XVal::Ref(f.and_then(|f| self.host.base_form(f)).unwrap_or(0))
            }
            // [documented: IsReference] a reference, not a base form.
            "isreference" => {
                let v = val(self, 0)?;
                let r = v.value()?.as_form().and_then(|f| self.host.form_sig(f)).is_some_and(|s| matches!(&s, b"REFR" | b"ACHR" | b"ACRE" | b"PGRE" | b"PMIS" | b"PBEA" | b"PFLA"));
                XVal::Num(if r { 1.0 } else { 0.0 })
            }
            // [documented: GetModIndex] 255 when the plugin is not loaded.
            "getmodindex" => {
                let n = text(val(self, 0)?).unwrap_or_default();
                let index = self.host.mod_index(&n).unwrap_or(255) as f64;
                self.host.plugin_asked("getmodindex", &n, index);
                XVal::Num(index)
            }
            // [documented: GetSourceModIndex] the load-order index of the form's plugin: its
            // FormID's top byte here, where FormIDs are load-order FormIDs.
            "getsourcemodindex" => {
                let f = match c.args.first() {
                    Some(a) => self.xeval(a)?.value()?.as_form(),
                    None => match &c.on {
                        Some(e) => self.xeval(e)?.value()?.as_form(),
                        None => None,
                    },
                };
                // A form made at run time (a clone) reads 255 [observed: nvse-inv,
                // TempCloneForm].
                // No form: -1 [observed: nvse-fourth, GetSourceModIndex of a runtime query's
                // GetCurrentScript].
                XVal::Num(f.map_or(-1.0, |f| if f >> 24 >= 0xFE { 255.0 } else { f64::from(f >> 24) }))
            }
            // The extender's math [documented: each page; the trigonometric functions in
            // degrees, observed: `Sin 90 > 0.5` is 1 in the original game].
            "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh" | "log" | "log10" | "exp" | "floor" | "ceil"
            | "squareroot" | "sqrt" => {
                let x = val(self, 0)?.num()?;
                let d = x.to_radians();
                XVal::Num(match n {
                    "sin" => d.sin(),
                    "cos" => d.cos(),
                    "tan" => d.tan(),
                    "asin" => x.asin().to_degrees(),
                    "acos" => x.acos().to_degrees(),
                    "atan" => x.atan().to_degrees(),
                    "sinh" => x.sinh(),
                    "cosh" => x.cosh(),
                    "tanh" => x.tanh(),
                    "log" => x.ln(),
                    "log10" => x.log10(),
                    "exp" => x.exp(),
                    // A whole number as a 32-bit integer: out of its range (or not a
                    // number) it reads -2147483648 [observed: gaps2, Floor of
                    // GetDistance2D to a reference in another cell].
                    "floor" | "ceil" => {
                        let r = if n == "floor" { x.floor() } else { x.ceil() };
                        if r.is_nan() || r < f64::from(i32::MIN) || r > f64::from(i32::MAX) { f64::from(i32::MIN) } else { r }
                    }
                    _ => x.sqrt(),
                })
            }
            // [documented: https://obse.silverlock.org/obse_command_doc.html (`(atan2:float)
            // datan2 arg1:float arg2:float`, "ATan2 and DATan2 use degrees"); cross-referenced
            // on https://geckwiki.com/index.php?title=Fatan2 ("significantly faster than dATan2");
            // observed: 2026-11-23, GSDocMitchellHouse (0x00103DF9), Fallout: New Vegas
            // 1.4.0.525 + xNVSE 6.4.9 (10 official base masters), #3678 / PR #3679 probe
            // `w1_3495_compound_assign_and_datan2` (10 quadrant/axis calls: `(0,0)=0`,
            // `(1,0)=90`, `(0,1)=0`, `(-1,0)=-90`, `(0,-1)=180`, `(1,1)=45`, `(-1,1)=-45`,
            // `(-1,-1)=-135`, `(1,-1)=135`, `(1,1.7320508)=30.000002`)].
            "atan2" | "datan2" => {
                let (y, x) = (val(self, 0)?.num()?, val(self, 1)?.num()?);
                XVal::Num(if y == 0.0 && x == 0.0 { 0.0 } else { y.atan2(x).to_degrees() })
            }
            "pow" => {
                let (a, b) = (val(self, 0)?.num()?, val(self, 1)?.num()?);
                XVal::Num(a.powf(b))
            }
            // [documented: fmod] x modulo n, plus the offset.
            "fmod" => {
                let (x, m) = (val(self, 0)?.num()?, val(self, 1)?.num()?);
                let off = opt(self, 2)?.map(|v| v.num()).transpose()?.unwrap_or(0.0);
                XVal::Num(if m == 0.0 { 0.0 } else { (x - off).rem_euclid(m) + off })
            }
            // [documented: Rand] a number from the minimum up to the maximum [observed: Rand 5,
            // 5 gives 5].
            "rand" => {
                let (a, b) = (val(self, 0)?.num()?, val(self, 1)?.num()?);
                XVal::Num(a + (b - a) * self.host.random())
            }
            "v3length" => {
                let (x, y, z) = (val(self, 0)?.num()?, val(self, 1)?.num()?, val(self, 2)?.num()?);
                XVal::Num((x * x + y * y + z * z).sqrt())
            }
            // What the engine reports (task C5; docs/spec-notes/ext-version.md): xNVSE 6.4.9
            // [decision: the owner, 2026-10-26; observed: 6 / 4 / 9 split as version,
            // revision, beta in the original]. A plugin without a module here is
            // absent: IsPluginInstalled 0, GetPluginVersion -1 [documented: -1 when absent].
            "getnvseversion" => XVal::Num(6.0),
            "getnvserevision" => XVal::Num(4.0),
            // [observed: GetNVSEBeta 9 under 6.4.9, the third number; owner 2026-10-26:
            // target 6.4.9].
            "getnvsebeta" => XVal::Num(9.0),
            // JIP LN and the other plugins whose functions NVForever builds
            // (`jip::PLUGINS`): reported installed at the version their functions follow.
            "isplugininstalled" | "getpluginversion" => {
                let p = text(val(self, 0)?).unwrap_or_default();
                let found = jip::PLUGINS.iter().find(|(name, _)| name.eq_ignore_ascii_case(&p)).map(|(_, v)| *v);
                let answer = match (n, found) {
                    ("isplugininstalled", f) => f64::from(u8::from(f.is_some())),
                    (_, Some(v)) => v,
                    (_, None) => -1.0,
                };
                self.host.plugin_asked(n, &p, answer);
                XVal::Num(answer)
            }
            _ => return Ok(None),
        }))
    }

    /// The extender's own functions, by catalogue name (task D): the conversions of B1,
    /// the arrays of D2, the strings of D1, output and the version reports so far.
    fn ext_function(&mut self, name: &str, c: &XCall) -> Result<XVal, VmError> {
        let lname = name.to_ascii_lowercase();
        if let Some(v) = self.string_function(&lname, c)? {
            return Ok(v);
        }
        // JIP LN's and lutana's functions (catalogue origins JIP, LU).
        let origin = self.table.unnumbered.get(&lname).map(|u| u.origin.clone()).unwrap_or_default();
        if lname == "validateregex" {
            return self.regex_function(&lname, c);
        }
        if let Some(v) = self.quat_function(&lname, c)? {
            return Ok(v);
        }
        if origin == "Anh" {
            if let Some(v) = self.anh_function(&lname, c)? {
                return Ok(v);
            }
        }
        if origin == "ITR" {
            if let Some(v) = self.itr_function(&lname, c)? {
                return Ok(v);
            }
        }
        // [documented: the catalogue's parameters (schemas/script/functions.json, from the
        // pages SetUIStringEx, SetUIStringAlt, InjectUIComponent, AttachUIComponent,
        // AddTileFromTemplate)]:
        // the first string (AddTileFromTemplate's path) or the one after the path is a
        // format string with its values, formatted here; the game gets the text in their
        // place (fnv_game::ui_functions).
        let format_at = match lname.as_str() {
            "addtilefromtemplate" => Some(0),
            // [documented: ShowTextInputMenu `callback width height "title" values…`,
            // SetTextInputString `"text" values…`] (fnv_game::nvse_jip12).
            "settextinputstring" => Some(0),
            "showtextinputmenu" => Some(3),
            "setuistringex" | "setuistringalt" | "injectuicomponent" | "attachuicomponent" => Some(1),
            _ => None,
        };
        if let Some(at) = format_at {
            return self.formatted_host_call(&lname, c, at)?.ok_or_else(|| VmError::NotBuilt(format!("the extender function {name}")));
        }
        // [documented: SetFormDescription `form "format" values…`, observed: gaps6,
        // "NVF desc %g" 3 reads "NVF desc 3"; MessageExAlt `seconds "format" values…`,
        // the game reading the icon and sound off the front]: formatted here, done by
        // the game.
        // [documented: SetBipedModelPathAlt `armor slot "format" values…`; observed:
        // gaps8, "NVF\Ground%g.nif" 7 reads NVF\Ground7.nif]: the format third.
        // RunScriptSnippet `delay "format" values…` likewise [documented].
        // [documented: SetObjectiveText `quest index "format" valuesâ€¦`, WriteStringToFile
        // `path append "format" valuesâ€¦`, ModLogPrint `indent "format" valuesâ€¦`] likewise.
        if matches!(lname.as_str(), "setformdescription" | "messageexalt" | "setbipedmodelpathalt" | "runscriptsnippet" | "setobjectivetext" | "writestringtofile" | "modlogprint") {
            let at = if matches!(lname.as_str(), "setbipedmodelpathalt" | "setobjectivetext" | "writestringtofile") { 2 } else { 1 };
            let mut first = Vec::new();
            for e in c.args.iter().take(at) {
                first.push(self.xeval(e)?);
            }
            // Too few values for its specifiers: nothing happens (`formatted_host_call`).
            let (t, _) = match self.format_arg(c, at) {
                Err(VmError::Type(e)) if e.starts_with("the format string takes") => return Ok(XVal::Num(0.0)),
                r => r?,
            };
            first.push(XVal::Str(t));
            return match self.host.ext_call(&lname, None, &first) {
                Some(r) => r.map_err(VmError::Host),
                None => Ok(XVal::Num(0.0)),
            };
        }
        // [documented: HudBarSetValueScriptVar `barName MyQuest.MyVariable calcMax`, the
        // bar reading the variable later]: the variable goes to the host by its place
        // (`Host::var_place`), as -1 - its number; anything that is no placed variable as
        // 0, which the host refuses.
        if lname == "hudbarsetvaluescriptvar" {
            let mut args = Vec::with_capacity(c.args.len());
            for (k, a) in c.args.iter().enumerate() {
                args.push(match a {
                    XExpr::Var { var, .. } if k == 1 => XVal::Num(self.host.var_place(var).map_or(0.0, |p| -1.0 - f64::from(p))),
                    _ if k == 1 => XVal::Num(0.0),
                    other => self.xeval(other)?,
                });
            }
            return match self.host.ext_call(&lname, None, &args) {
                Some(r) => r.map_err(VmError::Host),
                None => Err(VmError::NotBuilt(format!("the extender function {name}"))),
            };
        }
        // [documented: MarkScriptOnLoad, "Marks calling script"]: the host gets the
        // running script form (0 for none) as the argument.
        if lname == "markscriptonload" {
            let script = XVal::Ref(self.host.current_script().unwrap_or(0));
            return match self.host.ext_call(&lname, None, &[script]) {
                Some(r) => r.map_err(VmError::Host),
                None => Err(VmError::NotBuilt(format!("the extender function {name}"))),
            };
        }
        // Functions that give their results in script variables: the host answers with an
        // array of the return value and then each output, written to the variables here.
        if let Some(outs) = out_params(&lname) {
            return self.out_call(&lname, c, outs);
        }
        // ShowOff's string maps live with JIP's stores (`vm::jip`).
        if lname.starts_with("auxstringmaparray") {
            if let Some(v) = self.strmap_function(&lname, c)? {
                return Ok(v);
            }
        }
        if origin == "JIP" || origin == "LU" {
            return match self.jip_function(&lname, c)? {
                Some(v) => Ok(v),
                None => self.host_function(&lname, c)?.ok_or_else(|| VmError::NotBuilt(format!("the JIP LN function {name} (tasks J5–J10)"))),
            };
        }
        let arg = |vm: &mut Self, k: usize| -> Result<XVal, VmError> {
            match c.args.get(k) {
                Some(a) => vm.xeval(a),
                None => Err(VmError::Expression(format!("{name} without its argument"))),
            }
        };
        match name.to_ascii_lowercase().as_str() {
            // `(eval Expression)` inside an expression: coerces string and array results
            // to `1.0`/`0.0` via `xeval_truthy` [observed: 2026-11-23 & 2026-11-24,
            // GSDocMitchellHouse (0x00103DF9), Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9
            // with the 10 official base masters, probes `core_24_eval_string_var` /
            // `#3679` `core_24_string_var_destruct_zero` (`eval s`: `1 -> 0` across
            // `sv_Destruct`, `eval sEmp == 1`, `eval a1 == 1`, `eval a2 == 1`,
            // `eval ar_Null == 0`), `core_05_eval_empty_array_and_string_truth`, and
            // `w4d_03_eval_empty_str_lit` (#3802 / PR #3803: `saLit0(1:0)|saLit1(1:1)|saV0(1:1)`,
            // bare `""` -> `0`, `"abc"` -> `1`, allocated empty `string_var` -> `1`);
            // hypothesis for non-var string-returning calls].
            // Numeric and ref values pass through unchanged [design: preserves `#2275`,
            // where Blue Moon's Script Runner files write
            // `AuxVarSetFlt "*x" (eval GetINIFloat_Cached "s:k" "f.ini" 1) 5`; note
            // `core_24_eval_string_var` observed standalone `eval 5 == 1`, `eval -3 == 1`,
            // `eval 0 == 0` in xNVSE 6.4.9].
            "eval" => {
                let Some(a) = c.args.first() else {
                    return Err(VmError::Expression(format!("{name} without its argument")));
                };
                let (v, truthy) = self.xeval_truthy(a)?;
                Ok(match v {
                    XVal::Str(_) | XVal::Array(_) => XVal::Num(if truthy { 1.0 } else { 0.0 }),
                    other => other,
                })
            }
            // [observed: w4g_05_isactordetected_and_setclass_invalid] Calling
            // IsActorDetected on an explicit null/non-form receiver logs a null-reference
            // diagnostic, returns 0 without falling back to implicit self, and continues
            // the script instead of aborting.
            "isactordetected" => {
                let on = match &c.on {
                    Some(e) => Some(self.xeval(e)?.value()?),
                    None => None,
                };
                if on.is_some_and(|v| v.as_form().is_none()) {
                    self.host.no_reference_call(&lname, on, c);
                    return Ok(XVal::Num(0.0));
                }
                let mut args = Vec::with_capacity(c.args.len());
                for a in &c.args {
                    args.push(self.xeval(a)?);
                }
                match self.host.ext_call(&lname, on, &args) {
                    Some(r) => r.map_err(VmError::Host),
                    None => Err(VmError::NotBuilt(format!("the extender function {name} (tasks C, D)"))),
                }
            }
            // [documented: ToString; ToNumber] (the hex and reportError arguments of
            // ToNumber: D1). List arrays and stringmaps format as "NULL" [observed:
            // Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9, 2026-11-23, `GSDocMitchellHouse`,
            // `b3_core_05_tostring_array_map` (`wave4b_extenders.json`, #3679):
            // `PROBE|b3_core_05_tostring_array_map|L=[NULL]|M=[NULL]|N=[42]`]. Applying
            // "NULL" to all `XVal::Array(_)` variants in `ToString` while leaving `$`,
            // `PrintVar`, `ArrayToString`, and `Ar_Dump` on `text_of` stays [hypothesis].
            "tostring" => {
                let v = arg(self, 0)?;
                Ok(XVal::Str(match v {
                    XVal::Array(_) => "NULL".to_string(),
                    _ => self.text_of(&v)?,
                }))
            }
            // With the hexadecimal flag the text is read as hex [documented: ToNumber;
            // observed: "1F" gives 31].
            "tonumber" => {
                let hex = c.args.get(1).map(|a| self.xeval(a)).transpose()?.is_some_and(|v| v.truthy());
                Ok(match arg(self, 0)? {
                    XVal::Str(s) if hex => XVal::Num(hex_prefix(s.trim().trim_start_matches("0x").trim_start_matches("0X"))),
                    XVal::Str(s) => XVal::Num(text_number(&s)),
                    other => XVal::Num(other.num()?),
                })
            }
            n if n.starts_with("ar_") || n == "typeof" => self.array_function(n, c),
            // `[ref.]Call udf args…` [documented: Call; User Defined Function].
            "call" => {
                let on = match &c.on {
                    Some(e) => Some(self.xeval(e)?.value()?),
                    None => None,
                };
                if on.is_some_and(|v| v.as_form().is_none()) {
                    self.host.no_reference_call("call", on, c);
                    return Err(VmError::NoReference("call".to_string()));
                }
                let f = arg(self, 0)?;
                let mut args = Vec::with_capacity(c.args.len().saturating_sub(1));
                for k in 1..c.args.len() {
                    args.push(arg(self, k)?);
                }
                self.host.call_function(&f, on, args).map_err(VmError::Host)
            }
            // [documented: SetFunctionValue] the last value set is returned; it does not
            // end the function.
            "setfunctionvalue" => {
                let v = arg(self, 0)?;
                self.host.set_function_value(v).map_err(VmError::Host)?;
                Ok(XVal::Num(0.0))
            }
            // The event functions [documented: SetEventHandler; SetEventHandlerAlt;
            // RemoveEventHandler; DispatchEvent; DispatchEventAlt; GetCurrentEventName].
            "seteventhandler" | "seteventhandleralt" | "removeeventhandler" | "dispatchevent" | "dispatcheventalt" | "getcurrenteventname" => {
                let lname = name.to_ascii_lowercase();
                if lname == "getcurrenteventname" {
                    return self.host.event_call(EventCall::CurrentName).map_err(VmError::Host);
                }
                let event = match arg(self, 0)? {
                    XVal::Str(s) => s,
                    other => self.text_of(&other)?,
                };
                let call = match lname.as_str() {
                    "dispatchevent" => {
                        let map = c.args.get(1).map(|a| self.xeval(a)).transpose()?;
                        let sender = match c.args.get(2).map(|a| self.xeval(a)).transpose()? {
                            Some(XVal::Str(s)) => Some(s),
                            _ => None,
                        };
                        EventCall::Dispatch { event, map, sender }
                    }
                    "dispatcheventalt" => {
                        let on = match &c.on {
                            Some(e) => Some(self.xeval(e)?.value()?),
                            None => None,
                        };
                        let mut args = Vec::new();
                        for k in 1..c.args.len() {
                            args.push(arg(self, k)?);
                        }
                        EventCall::DispatchAlt { event, on, args }
                    }
                    _ => {
                        let func = arg(self, 1)?;
                        let mut pairs = Vec::new();
                        for (k, a) in c.args.iter().enumerate().skip(2) {
                            match a {
                                XExpr::Binary { op: XOp::Pair, l, r } => {
                                    let (key, v) = (self.xeval(l)?, self.xeval(r)?);
                                    pairs.push((key, v));
                                }
                                // RemoveEventHandler's plain filters: the first and second.
                                other => {
                                    let v = self.xeval(other)?;
                                    pairs.push((XVal::Str(if k == 2 { "first" } else { "second" }.into()), v));
                                }
                            }
                        }
                        if lname == "removeeventhandler" {
                            EventCall::Remove { event, func, pairs }
                        } else {
                            EventCall::Set { event, func, pairs, alt: lname == "seteventhandleralt" }
                        }
                    }
                };
                self.host.event_call(call).map_err(VmError::Host)
            }
            // [documented: GetGameLoaded] true once per script after each load (and a new
            // game); [documented: GetGameRestarted] true once per script per session.
            "getgameloaded" => Ok(XVal::Num(if self.host.game_loaded(false) { 1.0 } else { 0.0 })),
            "getgamerestarted" => Ok(XVal::Num(if self.host.game_loaded(true) { 1.0 } else { 0.0 })),
            // Characters as ASCII codes [documented: IsDigit; IsLetter; IsPrintable;
            // IsPunctuation; IsUpperCase; ToUpper; ToLower; CharToAscii; AsciiToChar].
            "isdigit" | "isletter" | "isprintable" | "ispunctuation" | "isuppercase" | "toupper" | "tolower" => {
                let code = arg(self, 0)?.num()? as i64;
                let ch = u8::try_from(code).ok();
                let t = |f: fn(&u8) -> bool| XVal::Num(f64::from(u8::from(ch.as_ref().is_some_and(f))));
                Ok(match name.to_ascii_lowercase().as_str() {
                    "isdigit" => t(u8::is_ascii_digit),
                    "isletter" => t(u8::is_ascii_alphabetic),
                    "isprintable" => t(|c| (0x20..0x7F).contains(c)),
                    "ispunctuation" => t(u8::is_ascii_punctuation),
                    "isuppercase" => t(u8::is_ascii_uppercase),
                    "toupper" => XVal::Num(ch.map_or(code, |c| i64::from(c.to_ascii_uppercase())) as f64),
                    _ => XVal::Num(ch.map_or(code, |c| i64::from(c.to_ascii_lowercase())) as f64),
                })
            }
            "chartoascii" => Ok(XVal::Num(match arg(self, 0)? {
                XVal::Str(s) => s.bytes().next().map_or(-1.0, f64::from),
                _ => -1.0,
            })),
            "asciitochar" => {
                let code = arg(self, 0)?.num()? as i64;
                Ok(XVal::Str(u8::try_from(code).ok().filter(|c| *c > 0).map(|c| char::from(c).to_string()).unwrap_or_default()))
            }
            // Bits of 32-bit integers [documented: GetBit; SetBit; ClearBit; LeftShift;
            // RightShift; LogicalAnd; LogicalOr; LogicalXor; LogicalNot]; the shifts are on
            // the unsigned value [hypothesis].
            "getbit" | "setbit" | "clearbit" | "leftshift" | "rightshift" | "logicaland" | "logicalor" | "logicalxor" => {
                let a = arg(self, 0)?.num()? as i64 as u32;
                let b = arg(self, 1)?.num()? as i64;
                let bit = if (0..32).contains(&b) { 1u32 << b } else { 0 };
                let sh = u32::try_from(b).unwrap_or(32);
                let on = c.args.get(2).map(|e| self.xeval(e)).transpose()?.is_none_or(|v| v.truthy());
                let r: u32 = match name.to_ascii_lowercase().as_str() {
                    "getbit" => u32::from(a & bit != 0),
                    "setbit" if on => a | bit,
                    "setbit" | "clearbit" => a & !bit,
                    "leftshift" => a.checked_shl(sh).unwrap_or(0),
                    "rightshift" => a.checked_shr(sh).unwrap_or(0),
                    "logicaland" => a & b as u32,
                    "logicalor" => a | b as u32,
                    _ => a ^ b as u32,
                };
                Ok(XVal::Num(f64::from(r as i32)))
            }
            "logicalnot" => Ok(XVal::Num(f64::from(!(arg(self, 0)?.num()? as i64 as u32) as i32))),
            // [documented: IntToBin] padded to the width; the prefix is "0b" [hypothesis:
            // the page names the option without showing it].
            "inttobin" => {
                let v = arg(self, 0)?.num()? as i64 as u32;
                // The width capped as the format fields are: it went straight into the padding [#1991].
                let width = (c.args.get(1).map(|e| self.xeval(e)).transpose()?.map(|v| v.num()).transpose()?.unwrap_or(0.0).max(0.0) as usize).min(MAX_FORMAT_FIELD);
                let prefix = c.args.get(2).map(|e| self.xeval(e)).transpose()?.is_some_and(|v| v.truthy());
                Ok(XVal::Str(format!("{}{:0width$b}", if prefix { "0b" } else { "" }, v)))
            }
            // [documented: GetCurrentScript; observed: none in a runtime query].
            "getcurrentscript" => Ok(XVal::Ref(self.host.current_script().unwrap_or(0))),
            // [documented: TestExpr; observed: nvse-leftovers, (1 + 1) 1, an element past
            // the end 0, an unset reference's GetName 0, ar_Size of an unassigned array 1]
            // 1 when the expression evaluates without an error.
            "testexpr" => {
                let ok = match c.args.first() {
                    Some(e) => self.xeval(e).and_then(|v| v.value().map(|_| ())).is_ok(),
                    None => false,
                };
                Ok(XVal::Num(f64::from(u8::from(ok))))
            }
            // [documented: Let; observed: JustAssortedMods.esp, `eval x && let s += "+"`]
            // Nested `let` inside an expression evaluates its assignment argument.
            "let" => match c.args.first() {
                Some(e) => self.xeval(e),
                None => Ok(XVal::Num(0.0)),
            },
            // [documented: GetCallingScript] 0 from a dialogue or stage result.
            "getcallingscript" => Ok(XVal::Ref(self.host.calling_script().unwrap_or(0))),
            // [documented: SetNameEx `"format" values… form`; SetNthFactionRankNameEx
            // `"format" values… faction rank [female]`]: formatted here, set by the game
            // (SetName's change; the rank title). SetUIStringEx and InjectUIComponent:
            // formatted at the top (`format_at`).
            // [documented: SetMCMString `child grandchild setting "format" values…`]: the
            // format is the fourth argument.
            // SetMCMStringMass `child first last setting "format" values…` likewise, its
            // format the fifth [observed: MCM 1.5's calls; job mcm976e, `"a%gb" 5` set
            // "a5b", menu-xml.md §10.2].
            "setmcmstring" | "setmcmstringmass" => {
                let at = if lname == "setmcmstring" { 3 } else { 4 };
                let mut args = Vec::new();
                for e in c.args.iter().take(at) {
                    args.push(self.xeval(e)?);
                }
                let (t, _) = self.format_arg(c, at)?;
                args.push(XVal::Str(t));
                match self.host.ext_call(&lname, None, &args) {
                    Some(r) => Ok(r.map_err(VmError::Host)?),
                    None => Ok(XVal::Num(0.0)),
                }
            }
            // [documented: SetModelPathEx; SetIconPathEx; SetBipedModelPathEx;
            // SetBipedIconPathEx] likewise, then the plain setter.
            "setnameex" | "setnthfactionranknameex" | "setmodelpathex" | "seticonpathex" | "setbipedmodelpathex" | "setbipediconpathex" => {
                let (t, k) = self.format_arg(c, 0)?;
                let mut args = vec![XVal::Str(t)];
                for e in c.args.iter().skip(k) {
                    args.push(self.xeval(e)?);
                }
                let on = match &c.on {
                    Some(e) => Some(self.xeval(e)?.value()?),
                    None => None,
                };
                let to = match lname.as_str() {
                    "setnameex" => "setname",
                    "setnthfactionranknameex" => "setnthfactionranknameex",
                    other => other.strip_suffix("ex").unwrap_or(other),
                };
                match self.host.ext_call(to, on, &args) {
                    Some(r) => r.map_err(VmError::Host)?,
                    None => XVal::Num(0.0),
                };
                Ok(XVal::Num(0.0))
            }
            // [observed: w4g_05_isactordetected_and_setclass_invalid] SetClass on a
            // null/empty receiver continues safely without mutating anything or falling
            // back to implicit self; a non-CLAS or null class argument aborts the script
            // at runtime in both strict and default modes.
            "setclass" => {
                let on = match &c.on {
                    Some(e) => Some(self.xeval(e)?.value()?),
                    None => None,
                };
                let mut args = Vec::with_capacity(c.args.len());
                for a in &c.args {
                    args.push(match a {
                        XExpr::Axis { c } => XVal::Str(c.to_string()),
                        other => self.xeval(other)?,
                    });
                }
                if on.is_some_and(|v| v.as_form().is_none()) {
                    self.host.no_reference_call(&lname, on, c);
                    return Ok(XVal::Num(0.0));
                }
                match self.host.ext_call(&lname, on, &args) {
                    Some(Ok(v)) => Ok(v),
                    Some(Err(e)) => Err(VmError::RuntimeAbort(e)),
                    None => Err(VmError::NotBuilt(format!("the extender function {name} (tasks C, D)"))),
                }
            }
            _ => self.host_function(&lname, c)?.ok_or_else(|| VmError::NotBuilt(format!("the extender function {name} (tasks C, D)"))),
        }
    }

    /// As [`Self::host_function`], with the format string at argument `at` and its values
    /// handed over as one formatted string.
    fn formatted_host_call(&mut self, lname: &str, c: &XCall, at: usize) -> Result<Option<XVal>, VmError> {
        let on = match &c.on {
            Some(e) => Some(self.xeval(e)?.value()?),
            None => None,
        };
        let mut args = Vec::with_capacity(c.args.len());
        for e in c.args.iter().take(at) {
            args.push(self.xeval(e)?);
        }
        // A format string whose specifiers have fewer values after it than they take:
        // the function does nothing and the script goes on [observed: #2430 probe
        // 2026-11-14, SetFormDescription "test %.0f left" with no value left the text as
        // it was, the script ran on; hypothesis: the same for the other functions that
        // take a formatted string].
        let (t, k) = match self.format_arg(c, at) {
            Err(VmError::Type(e)) if e.starts_with("the format string takes") => return Ok(Some(XVal::Num(0.0))),
            r => r?,
        };
        args.push(XVal::Str(t));
        for e in c.args.iter().skip(k) {
            args.push(self.xeval(e)?);
        }
        match self.host.ext_call(lname, on, &args) {
            Some(r) => r.map(Some).map_err(VmError::Host),
            None => Ok(None),
        }
    }

    /// An extender function the host builds (`Host::ext_call`); `None` when it does not.
    /// A function whose arguments at `outs` are script variables it writes: the other
    /// arguments go to the host, whose answer is an array of the return value followed by
    /// one value per output.
    fn out_call(&mut self, lname: &str, c: &XCall, outs: &[usize]) -> Result<XVal, VmError> {
        let on = match &c.on {
            Some(e) => Some(self.xeval(e)?.value()?),
            None => None,
        };
        let mut args = Vec::with_capacity(c.args.len());
        for (k, a) in c.args.iter().enumerate() {
            if !outs.contains(&k) {
                args.push(self.xeval(a)?);
            }
        }
        let r = match self.host.ext_call(lname, on, &args) {
            Some(r) => r.map_err(VmError::Host)?,
            None => return Err(VmError::NotBuilt(format!("the extender function {lname}"))),
        };
        let XVal::Array(id) = r else { return Ok(r) };
        let entries: Vec<(XKey, XVal)> = self.store()?.array(id).map(|a| a.entries.clone()).unwrap_or_default();
        let at = |idx: usize| entries.iter().find(|(k, _)| matches!(k, XKey::Num(x) if *x == idx as f64)).map(|(_, v)| v.clone());
        for (i, &k) in outs.iter().enumerate() {
            if let (Some(XExpr::Var { var, .. }), Some(v)) = (c.args.get(k), at(i + 1)) {
                self.write_var(var, v)?;
            }
        }
        Ok(at(0).unwrap_or(XVal::Num(0.0)))
    }

    fn host_function(&mut self, lname: &str, c: &XCall) -> Result<Option<XVal>, VmError> {
        let on = match &c.on {
            Some(e) => Some(self.xeval(e)?.value()?),
            None => None,
        };
        // Called on no reference, the function fails and stops the script [observed:
        // nvse-inv foreach.read.after, "script call failed"], unless the host admits an
        // explicit null receiver (`0`) for that global command (for example player-source
        // commands [observed: w4g_06_player_cmds_explicit_receiver]).
        // Exception: JIP LN ProjectUITile logs the null-ref diagnostic, returns 0.0,
        // and continues script execution without enqueueing a projection [observed:
        // w4g_10_project_ui_tile_errors, tools/capture/nvse/catalogs/wave4g_f2_extenders.json].
        let explicit_null = matches!(on, Some(Value::Ref(0))) || matches!(on, Some(Value::Num(x)) if x == 0.0);
        if on.is_some_and(|v| v.as_form().is_none()) && !(explicit_null && self.host.allows_null_receiver(lname)) {
            self.host.no_reference_call(lname, on, c);
            if lname == "projectuitile" {
                return Ok(Some(XVal::Num(0.0)));
            }
            return Err(VmError::NoReference(lname.to_string()));
        }
        let mut args = Vec::with_capacity(c.args.len());
        for a in &c.args {
            args.push(match a {
                XExpr::Axis { c } => XVal::Str(c.to_string()),
                other => self.xeval(other)?,
            });
        }
        match self.host.ext_call(lname, on, &args) {
            // The host words a calling object that is no reference (a base form) as
            // NoReference does: that stops the script too [observed: jipppln clamp.base,
            // "script call failed"].
            Some(Err(e)) if e == VmError::NoReference(lname.to_string()).to_string() => {
                self.host.no_reference_call(lname, on, c);
                Err(VmError::NoReference(lname.to_string()))
            },
            Some(r) => r.map(Some).map_err(VmError::Host),
            None => Ok(None),
        }
    }
}

/// The argument positions a function writes its results into, for [`Vm::out_call`]
/// [documented: the functions' pages, "outputs … into the specified script variables"].
fn out_params(lname: &str) -> Option<&'static [usize]> {
    Some(match lname {
        "getangleex" => &[0, 1, 2],
        // [documented: GetActorVelocityAlt, "placed into the 3 specified Script Variables"].
        "getactorvelocityalt" => &[0, 1, 2],
        // JIP batch 13 (fnv_game::nvse_jip13).
        "getangularvelocityalt" | "getlinearvelocityalt" => &[1, 2, 3],
        "getreticleposalt" => &[0, 1, 2],
        "getanglesbetweenpoints" => &[6, 7],
        "getpointraycastpos" => &[5, 6, 7],
        "geteditorposition" => &[0, 1, 2, 3, 4, 5],
        "getstringuidimensions" => &[3, 4, 5],
        // [documented: GetPlayerCameraRotation, "store the players camera rotation in fx fy
        // fz"; GetAkimboWeaponsAlt, the left and right weapons into the two variables].
        "getplayercamerarotation" => &[0, 1, 2],
        "getakimboweaponsalt" => &[1, 2],
        // [documented: GetTFCPosEx, GetTFCAngleEx, GetTFCRotEx, "pass … to … script
        // variables"; GetCalculatedPos and GetCalculatedPosAlt, xOut and yOut].
        "gettfcposex" => &[0, 1, 2],
        "gettfcangleex" | "gettfcrotex" => &[0, 1],
        "getcalculatedpos" | "getcalculatedposalt" => &[2, 3],
        // [documented: GetPosEx, GetNifBlockTranslationAlt, GetNifBlockRotationAlt, "the
        // result XYZ are placed into the 3 specified Script Variables"].
        "getposex" => &[0, 1, 2],
        "getnifblocktranslationalt" | "getnifblockrotationalt" => &[1, 2, 3],
        // [documented: GetMousePosition, GetMouseMovement, "output … to fPosX and fPosY
        // script variables"].
        "getmouseposition" | "getmousemovement" => &[0, 1],
        // [documented: GetCameraTranslation and WorldToScreen, "Stores … in xOut, yOut and
        // zOut variables"].
        "getcameratranslation" | "worldtoscreen" => &[0, 1, 2],
        // [documented: Ar_SetFloatsFrom, "Sets float variables from variables in array (up
        // to 10)"].
        "ar_setfloatsfrom" => &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        // [documented: HSVtoRGB, RGBtoHSV, three script variables in and three out].
        "hsvtorgb" | "rgbtohsv" => &[3, 4, 5],
        // [documented: GetSystemColorAlt and GetPixelFromBMP, three variables for R, G, B].
        "getsystemcoloralt" | "getpixelfrombmp" => &[1, 2, 3],
        // [documented: GetLinearVelocity and GetCurrentSkyColor, three script variables].
        "getlinearvelocity" | "getcurrentskycolor" => &[1, 2, 3],
        _ => return None,
    })
}

/// Where `Label n` stands in a statement list.
fn label_at(stmts: &[Stmt], n: i32) -> Option<usize> {
    stmts.iter().position(|s| matches!(s, Stmt::Label { n: m } if *m == n))
}

/// The binary operator of a compound assignment.
fn assign_op(op: XAssign) -> XOp {
    match op {
        XAssign::Add => XOp::Add,
        XAssign::Sub => XOp::Sub,
        XAssign::Mul => XOp::Mul,
        XAssign::Div => XOp::Div,
        XAssign::Pow => XOp::Pow,
        XAssign::BitOr => XOp::BitOr,
        XAssign::BitAnd => XOp::BitAnd,
        XAssign::Mod | XAssign::Set => XOp::Mod,
    }
}

/// A deciding operand of `&&`/`||` as their value: a number as itself, anything else as
/// 1 or 0 [documented: NVSE Expressions].
fn as_operand(v: &XVal, truthy: bool) -> f64 {
    match v {
        XVal::Num(x) => *x,
        _ => {
            if truthy {
                1.0
            } else {
                0.0
            }
        }
    }
}

/// A binary operator of the extended expressions [documented: NVSE Expressions: `+` adds
/// numbers or joins strings; strings compare without regard to case for `<` `>` `<=`
/// `>=`; the bitwise operators and shifts demote their operands to integers; `^` raises to
/// a power]. `==`/`!=` on strings ignore case too [hypothesis: ext-values.md §4 leaves it
/// open]. Division by zero is an error [documented: NVSE Error], unlike vanilla's.
fn xbinary(op: XOp, a: XVal, b: XVal) -> Result<XVal, VmError> {
    if let (XVal::Str(x), XVal::Str(y)) = (&a, &b) {
        let (lx, ly) = (x.to_lowercase(), y.to_lowercase());
        let t = |c: bool| XVal::Num(if c { 1.0 } else { 0.0 });
        return Ok(match op {
            XOp::Add => XVal::Str(format!("{x}{y}")),
            XOp::Eq => t(lx == ly),
            XOp::Ne => t(lx != ly),
            XOp::Lt => t(lx < ly),
            XOp::Le => t(lx <= ly),
            XOp::Gt => t(lx > ly),
            XOp::Ge => t(lx >= ly),
            _ => return Err(VmError::Type(format!("{op:?} on strings"))),
        });
    }
    if matches!(a, XVal::Str(_)) || matches!(b, XVal::Str(_)) {
        return Err(VmError::Type(format!("{op:?} of a string and a number (use $ or #)")));
    }
    if matches!(op, XOp::Div | XOp::Mod) && b.num()? == 0.0 {
        return Err(VmError::Type("division by zero".into()));
    }
    let (av, bv) = (a.value()?, b.value()?);
    let int = |v: Value| v.num().trunc() as i64;
    let vanilla = |o: Op| XVal::from_value(binary(o, av, bv));
    Ok(match op {
        XOp::Eq => vanilla(Op::Eq),
        XOp::Ne => vanilla(Op::Ne),
        XOp::Lt => vanilla(Op::Lt),
        XOp::Le => vanilla(Op::Le),
        XOp::Gt => vanilla(Op::Gt),
        XOp::Ge => vanilla(Op::Ge),
        XOp::Add => vanilla(Op::Add),
        XOp::Sub => vanilla(Op::Sub),
        XOp::Mul => vanilla(Op::Mul),
        XOp::Div => vanilla(Op::Div),
        XOp::Mod => vanilla(Op::Mod),
        XOp::Pow => XVal::Num(av.num().powf(bv.num())),
        XOp::BitOr => XVal::Num((int(av) | int(bv)) as f64),
        XOp::BitAnd => XVal::Num((int(av) & int(bv)) as f64),
        XOp::Shl => XVal::Num(int(av).checked_shl(int(bv).clamp(0, 63) as u32).unwrap_or(0) as f64),
        XOp::Shr => XVal::Num(int(av).checked_shr(int(bv).clamp(0, 63) as u32).unwrap_or(0) as f64),
        XOp::Or => XVal::Num(if av.truthy() { av.num() } else { bv.num() }),
        XOp::And => XVal::Num(if av.truthy() { bv.num() } else { 0.0 }),
        XOp::Slice | XOp::Pair | XOp::Subscript => return Err(VmError::NotBuilt("arrays (B3)".into())),
    })
}

/// A binary operator [hypothesis unless tagged: comparisons give 1 or 0, `&&` and `||`
/// test for non-zero, arithmetic is in double precision; division by zero gives 0 and
/// `%` is the remainder of the operands truncated to integers (0 for a zero divisor)].
fn binary(op: Op, a: Value, b: Value) -> Value {
    let (x, y) = (a.num(), b.num());
    match op {
        Op::Eq => Value::bool(x == y),
        Op::Ne => Value::bool(x != y),
        Op::Lt => Value::bool(x < y),
        Op::Le => Value::bool(x <= y),
        Op::Gt => Value::bool(x > y),
        Op::Ge => Value::bool(x >= y),
        Op::And => Value::bool(a.truthy() && b.truthy()),
        Op::Or => Value::bool(a.truthy() || b.truthy()),
        Op::Add => Value::Num(x + y),
        Op::Sub => Value::Num(x - y),
        Op::Mul => Value::Num(x * y),
        Op::Div => Value::Num(if y == 0.0 { 0.0 } else { x / y }),
        Op::Mod => {
            let (p, q) = (x.trunc() as i64, y.trunc() as i64);
            // checked_rem: also 0 for i64::MIN % -1, which overflows (a huge operand).
            Value::Num(p.checked_rem(q).unwrap_or(0) as f64)
        }
        Op::Neg => Value::Num(-y),
    }
}

/// Blocks of a program with this event code, in order (a script may have several,
/// e.g. `OnDeath` and `OnDeath player`).
pub fn blocks(p: &Program, event: u16) -> impl Iterator<Item = &Block> {
    p.blocks.iter().filter(move |b| b.event == event)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `%` of a huge operand by -1 (i64::MIN % -1 overflows) gives 0, not a panic.
    #[test]
    fn remainder_never_overflows() {
        assert_eq!(binary(Op::Mod, Value::Num(-1e300), Value::Num(-1.0)), Value::Num(0.0));
        assert_eq!(binary(Op::Mod, Value::Num(7.0), Value::Num(0.0)), Value::Num(0.0));
        assert_eq!(binary(Op::Mod, Value::Num(7.5), Value::Num(2.0)), Value::Num(1.0));
    }
}
