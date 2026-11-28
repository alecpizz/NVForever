//! The compiler without game data: source → bytes → the decoder's IR
//! (docs/spec-notes/script-compiler.md). The parity over the shipped units is the lab
//! scenario L5.script.compile.vanilla.

#[path = "compile/inline_surplus_operands.rs"]
mod inline_surplus_operands;
#[path = "compile/extender_params.rs"]
mod extender_params;

use fnv_script::compile::{compile, Compiled, Env, FormInfo, Seed, PLAYER};
use fnv_script::unit::RefSlot;
use fnv_script::{decode, Arg, Expr, FunctionTable, Op, Program, Stmt, Token, VarDecl, VarType};

fn table() -> FunctionTable {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
    FunctionTable::load(&p).expect("functions.json loads")
}

const QUEST: u32 = 0x0100_1000;
const CAPS: u32 = 0x0000_000F;
const GLOBAL: u32 = 0x0100_2000;
const DOOR: u32 = 0x0100_3000;

struct World;

impl Env for World {
    fn form(&self, editor_id: &str) -> Option<FormInfo> {
        let (form, sig) = match editor_id.to_ascii_lowercase().as_str() {
            "testquest" => (QUEST, *b"QUST"),
            "caps001" => (CAPS, *b"MISC"),
            "testglobal" => (GLOBAL, *b"GLOB"),
            "testdoorref" => (DOOR, *b"REFR"),
            _ => return None,
        };
        Some(FormInfo { form, sig })
    }
    fn info(&self, form: u32) -> Option<FormInfo> {
        [("testquest", QUEST), ("caps001", CAPS), ("testglobal", GLOBAL), ("testdoorref", DOOR)]
            .iter()
            .find(|(_, f)| *f == form)
            .and_then(|(n, _)| self.form(n))
    }
    fn editor_id(&self, form: u32) -> Option<String> {
        [("TestQuest", QUEST), ("Caps001", CAPS), ("TestGlobal", GLOBAL), ("TestDoorREF", DOOR)]
            .iter()
            .find(|(_, f)| *f == form)
            .map(|(n, _)| n.to_string())
    }
    fn local_form(&self, local: u32) -> Option<u32> {
        Some(local)
    }
    fn script_vars(&self, form: u32) -> Option<Vec<VarDecl>> {
        (form == QUEST).then(|| {
            vec![
                VarDecl { index: 1, name: "bDone".into(), ty: VarType::Int },
                VarDecl { index: 2, name: "fTimer".into(), ty: VarType::FloatOrRef },
            ]
        })
    }
}

fn build(src: &str) -> (Compiled, Program) {
    let t = table();
    let c = compile(src, &t, &World, None);
    let errors: Vec<_> = c.errors().collect();
    assert!(errors.is_empty(), "compile errors: {errors:?}");
    let d = decode(&c.bytes, &t);
    assert!(d.report.issues.is_empty(), "decoder issues: {:?}", d.report.issues);
    assert_eq!(d.report.raw_total(), 0);
    (c, d.program)
}

fn body(p: &Program) -> &[Stmt] {
    &p.blocks[0].body
}

fn ops(e: &Expr) -> Vec<String> {
    e.0.iter()
        .map(|t| match t {
            Token::Num { text, .. } => text.clone(),
            Token::Op { op } => op.symbol().to_string(),
            Token::Var { .. } => "var".into(),
            Token::Ref { .. } => "ref".into(),
            Token::Call { call } => format!("call{:04X}", call.opcode),
            Token::X { .. } => "x".into(),
            Token::Raw { .. } => "raw".into(),
        })
        .collect()
}

#[test]
fn a_small_script_round_trips_through_the_decoder() {
    let (c, p) = build(
        "scn TestScript\nshort a\nref r\n\nbegin GameMode\n  set a to 1 + 2 * 3\n  if a == 7 && GetStage TestQuest >= 10\n    player.AddItem Caps001 5\n  elseif a\n    r.Activate player 1\n  else\n    set TestQuest.bDone to 1\n  endif\nend\n",
    );
    assert!(p.script_name);
    assert_eq!(p.blocks.len(), 1);
    assert_eq!(p.blocks[0].params, None, "GameMode has no parameter block");
    let Stmt::Set { expr, .. } = &body(&p)[0] else { panic!("set first") };
    assert_eq!(ops(expr), ["1", "2", "3", "*", "+"]);
    let Stmt::If { arms, otherwise } = &body(&p)[1] else { panic!("if second") };
    assert_eq!(arms.len(), 2);
    assert!(otherwise.is_some());
    assert_eq!(c.variables.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["a", "r"]);
    // The quest (GetStage), the player, the caps, the local `r`, in order of first use.
    assert_eq!(c.refs, [RefSlot::Form(QUEST), RefSlot::Form(PLAYER), RefSlot::Form(CAPS), RefSlot::Local(2)]);
}

#[test]
fn or_binds_tighter_than_and_and_minus_is_unary() {
    let (_, p) = build("short a\nshort b\nshort c\nbegin GameMode\nif a && b || c\nset a to -1\nendif\nend");
    let Stmt::If { arms, .. } = &body(&p)[0] else { panic!() };
    assert_eq!(ops(&arms[0].cond), ["var", "var", "var", "||", "&&"]);
    let Stmt::Set { expr, .. } = &arms[0].body[0] else { panic!() };
    assert_eq!(ops(expr), ["1", "~"]);
    assert!(matches!(expr.0[1], Token::Op { op: Op::Neg }));
}

#[test]
fn parameter_types_decide_how_values_are_stored() {
    let (c, p) = build(
        "ref r\nfloat f\nbegin GameMode\nSetQuestDelay TestQuest 3\nset f to GetGlobalValue TestGlobal\nset f to TestGlobal\nplayer.GetDistance r\nend",
    );
    let b = body(&p);
    // A number in a float parameter is a float, even written without a point.
    let Stmt::Call { call } = &b[0] else { panic!() };
    assert_eq!(call.args[1], Arg::Float(3.0));
    // A global in a form parameter is the form; in an expression it is the value.
    let Stmt::Set { expr, .. } = &b[1] else { panic!() };
    let Token::Call { call } = &expr.0[0] else { panic!() };
    assert!(matches!(call.args[0], Arg::Ref(_)));
    let Stmt::Set { expr, .. } = &b[2] else { panic!() };
    assert!(matches!(expr.0[0], Token::Var { .. }));
    // A `ref` local as a parameter is its reference list entry.
    let Stmt::Call { call } = &b[3] else { panic!() };
    let Arg::Ref(r) = call.args[0] else { panic!("ref local passed as a reference: {:?}", call.args) };
    assert_eq!(c.refs[r.0 as usize - 1], RefSlot::Local(1));
}

#[test]
fn player_spellings_get_their_own_entries() {
    let (c, _) = build("begin GameMode\nplayer.Disable\nPlayerREF.Enable\nplayer.Enable\nend");
    assert_eq!(c.refs, [RefSlot::Form(PLAYER), RefSlot::Form(PLAYER)]);
}

#[test]
fn surplus_arguments_are_dropped() {
    let (_, p) = build("begin GameMode\nStopCombat player\nend");
    let Stmt::Call { call } = &body(&p)[0] else { panic!() };
    assert!(!call.has_params, "StopCombat takes no parameters: no parameter block");
}

#[test]
fn a_seed_keeps_the_units_indices() {
    let t = table();
    let vars = vec![VarDecl { index: 7, name: "a".into(), ty: VarType::Int }];
    let refs = vec![RefSlot::Form(CAPS)];
    let c = compile("short a\nbegin GameMode\nset a to 1\nplayer.AddItem Caps001 a\nend", &t, &World, Some(Seed { variables: &vars, refs: &refs, unknown_calls: false }));
    assert_eq!(c.variables, vars);
    assert_eq!(c.refs, [RefSlot::Form(CAPS), RefSlot::Form(PLAYER)]);
    assert!(c.diags.iter().any(|d| d.kind == "reference_added"));
    let p = decode(&c.bytes, &t).program;
    let Stmt::Call { call } = &body(&p)[1] else { panic!() };
    assert_eq!(call.on.map(|r| r.0), Some(2));
    assert_eq!(call.args[0], Arg::Ref(fnv_script::RefIndex(1)));
}

#[test]
fn unknown_names_are_errors() {
    let t = table();
    let c = compile("begin GameMode\nNoSuchFunction 1\nset x to 2\nend", &t, &World, None);
    let kinds: Vec<&str> = c.errors().map(|d| d.kind).collect();
    assert_eq!(kinds, ["unknown_function", "bad_set_target"]);
    assert_eq!(c.errors().next().map(|d| d.line), Some(2));
}

#[test]
fn begin_blocks_with_parameters() {
    let (c, p) = build("begin OnTrigger player\nend\nbegin OnLoad\nend");
    assert_eq!(p.blocks[0].params.as_ref().map(Vec::len), Some(1));
    assert_eq!(p.blocks[1].params, None);
    assert_eq!(c.refs, [RefSlot::Form(PLAYER)]);
}

// ---- the extender's expressions (task A7) --------------------------------------

use fnv_script::ir::{FuncId, VarSlot, XAssign, XExpr, XOp};
use fnv_script::vm::{ArgValue, Host, RunOpts, Value, Vm};

/// The tree as text, every operation in parentheses.
fn x(e: &XExpr) -> String {
    match e {
        XExpr::Num { text, .. } => text.clone(),
        XExpr::Str { s } => format!("{s:?}"),
        XExpr::Var { var, .. } => match var.var {
            VarSlot::Local { index, .. } => format!("v{index}"),
            VarSlot::Global { global } => format!("g{}", global.0),
        },
        XExpr::Ref { r } => format!("r{}", r.0),
        XExpr::Enum { code } => format!("#{code}"),
        XExpr::Axis { c } => c.to_string(),
        XExpr::Call { call } => {
            let f = match &call.func {
                FuncId::Opcode(o) => format!("{o:04X}"),
                FuncId::Name(n) => n.clone(),
            };
            let on = call.on.as_ref().map(|o| format!("{}.", x(o))).unwrap_or_default();
            format!("{on}{f}({})", call.args.iter().map(x).collect::<Vec<_>>().join(" "))
        }
        XExpr::Unary { op, e } => format!("({op:?} {})", x(e)),
        XExpr::Binary { op, l, r } => format!("({} {op:?} {})", x(l), x(r)),
        XExpr::Member { l, key } => format!("{}->{key}", x(l)),
        XExpr::Assign { op, target, value } => format!("({} {op:?}= {})", x(target), x(value)),
        XExpr::Lambda { params, body, value, .. } => format!(
            "lambda[{}]({})",
            params.len(),
            value.as_ref().map(|v| x(v)).unwrap_or_else(|| format!("{} statements", body.len()))
        ),
    }
}

fn lets(src: &str) -> Vec<String> {
    let (_, p) = build(src);
    let mut out = Vec::new();
    p.walk(&mut |s| match s {
        Stmt::Let { expr } => out.push(x(expr)),
        Stmt::If { arms, .. } => {
            for a in arms {
                if let [Token::X { e }] = &a.cond.0[..] {
                    out.push(format!("if {}", x(e)));
                }
            }
        }
        _ => {}
    });
    out
}

#[test]
fn extender_precedence_follows_the_operator_table() {
    let l = lets("int a\nint b\nbegin GameMode\nlet a := 1 + 2 * 3 ^ 2\nlet a := b := 2\na += 1\nlet a := 1 || 0 && 2\nlet a := -!b\nend");
    assert_eq!(l[0], "(v1 Set= (1 Add (2 Mul (3 Pow 2))))");
    assert_eq!(l[1], "(v1 Set= (v2 Set= 2))", ":= groups right to left");
    assert_eq!(l[2], "(v1 Add= 1)", "the Let macro");
    assert_eq!(l[3], "(v1 Set= (1 Or (0 And 2)))", "&& binds tighter than ||");
    assert_eq!(l[4], "(v1 Set= (Neg (Not v2)))");
}

#[test]
fn a_functions_argument_is_an_operand() {
    // [documented: Inline Expression] `Player.GetActorValue Health + 50` is
    // (GetAV Health) + 50.
    let l = lets("int a\nbegin GameMode\nif eval GetStage TestQuest >= 10\nendif\nlet a := player.GetAV Luck\nlet a := GetRandomPercent * 2\nlet a := Abs -3 + 1\nend");
    assert_eq!(l[0], "if (103A(r1) Ge 10)");
    assert_eq!(l[3], "(v1 Set= (1221((Neg 3)) Add 1))", "a minus starts the first argument");
    assert!(l[1].starts_with("(v1 Set= r2.100E(#"), "an actor value name is its code: {}", l[1]);
    assert_eq!(l[2], "(v1 Set= (104D() Mul 2))", "a function without parameters takes no operand");
}

#[test]
fn extender_functions_are_named() {
    let t = table();
    let c = compile("int a\nbegin GameMode\nlet a := Ar_Size b\nend", &t, &World, None);
    // An undeclared word is its own text, with a warning [observed: nvse-fourth, the
    // editor's "Unquoted argument"].
    assert!(c.errors().next().is_none(), "{:?}", c.diags);
    assert!(c.diags.iter().any(|d| d.kind == "unquoted_argument" && !d.error), "b is not declared: {:?}", c.diags);
    let c = compile("int a\nint b\nbegin GameMode\nlet a := Ar_Size b\nend", &t, &World, None);
    assert!(c.errors().next().is_none(), "{:?}", c.diags);
    let p = decode(&c.bytes, &t).program;
    let Stmt::Let { expr: XExpr::Assign { op: XAssign::Set, value, .. } } = &body(&p)[0] else { panic!() };
    assert!(matches!(value.as_ref(), XExpr::Call { call } if call.func == FuncId::Name("Ar_Size".into())));
}

#[test]
fn bad_extended_lines_are_errors() {
    let t = table();
    for (src, kind) in [
        ("begin GameMode\nlet 5 := 1\nend", "bad_assignment_target"),
        ("int a\nbegin GameMode\nlet a + 1\nend", "let_without_assignment"),
        ("int a\nbegin GameMode\nlet a := (1 + 2\nend", "syntax"),
        ("int a\nbegin GameMode\nlet a := 1 2\nend", "syntax"),
    ] {
        let c = compile(src, &t, &World, None);
        assert_eq!(c.errors().map(|d| d.kind).collect::<Vec<_>>(), [kind], "{src}");
    }
}

#[derive(Default)]
struct Locals(
    std::collections::BTreeMap<u16, Value>,
    fnv_script::vm::ExtStore,
    std::collections::BTreeSet<u16>,
    std::collections::BTreeSet<u16>,
    Vec<String>,
    Option<u32>,
);

impl Host for Locals {
    fn var_kind(&mut self, var: &fnv_script::VarRef) -> fnv_script::vm::VarKind {
        match var.var {
            VarSlot::Local { index, .. } if self.2.contains(&index) => fnv_script::vm::VarKind::String,
            VarSlot::Local { index, .. } if self.3.contains(&index) => fnv_script::vm::VarKind::Array,
            _ => fnv_script::vm::VarKind::Value,
        }
    }
    fn ext(&mut self) -> Option<&mut fnv_script::vm::ExtStore> {
        Some(&mut self.1)
    }
    fn print(&mut self, text: &str) {
        self.4.push(text.to_string());
    }
    fn get_var(&mut self, var: &fnv_script::VarRef) -> Result<Value, String> {
        let VarSlot::Local { index, .. } = var.var else { return Err("global".into()) };
        Ok(self.0.get(&index).copied().unwrap_or_default())
    }
    fn set_var(&mut self, var: &fnv_script::VarRef, value: Value) -> Result<(), String> {
        let VarSlot::Local { index, .. } = var.var else { return Err("global".into()) };
        self.0.insert(index, value);
        Ok(())
    }
    fn ref_value(&mut self, _r: fnv_script::RefIndex) -> Result<Value, String> {
        Ok(Value::Ref(PLAYER))
    }
    fn call(&mut self, _opcode: u16, _on: Option<Value>, _args: &[ArgValue]) -> Result<Value, String> {
        Ok(Value::Num(4.0))
    }
    fn calling_ref(&mut self) -> Option<u32> {
        self.5
    }
    fn mod_name(&mut self, index: u32) -> Option<String> {
        (index == 0).then(|| "FalloutNV.esm".to_string())
    }
    fn mod_index(&mut self, name: &str) -> Option<u32> {
        name.eq_ignore_ascii_case("FalloutNV.esm").then_some(0)
    }
    fn form_sig(&mut self, form: u32) -> Option<[u8; 4]> {
        (form == 0x14 || form == 0x104C0C).then_some(*b"NPC_")
    }
    fn actor_value_code(&mut self, name: &str) -> Option<u16> {
        name.eq_ignore_ascii_case("Strength").then_some(5)
    }
}

/// A host whose string variables are the compiled unit's `string_var`s.
fn host_for(c: &Compiled) -> Locals {
    let of = |t: &str| c.variables.iter().filter(|v| v.ty == VarType::Other(t.into())).map(|v| v.index as u16).collect();
    Locals(Default::default(), Default::default(), of("string_var"), of("array_var"), Vec::new(), None)
}

fn run(src: &str) -> Locals {
    let t = table();
    let (c, p) = build(src);
    let mut h = host_for(&c);
    Vm::new(&t, &mut h, RunOpts { strict: true, ..RunOpts::default() }).run(&p.blocks[0].body).expect("runs");
    h
}

#[test]
fn let_runs_on_numbers() {
    let h = run("float a\nfloat b\nfloat c\nbegin GameMode\nlet a := 2 + 3 * 4\nlet b := c := 7\nb += 1\nlet c := 0 || 5\nlet a := (1 << 3) | 1\nend");
    assert_eq!(h.0[&1], Value::Num(9.0));
    assert_eq!(h.0[&2], Value::Num(8.0));
    // `||` returns the deciding operand's value [documented: NVSE Expressions].
    assert_eq!(h.0[&3], Value::Num(5.0));
}

#[test]
fn if_eval_tests_an_extended_condition() {
    let h = run("float a\nbegin GameMode\nif eval 2 ^ 3 == 8 && GetRandomPercent\nlet a := 1\nelseif eval 1\nlet a := 2\nendif\nend");
    assert_eq!(h.0[&1], Value::Num(1.0));
}

#[test]
fn not_built_yet_stops_the_statement() {
    let t = table();
    // GetPluginHeaderVersion is not built in the VM (the game answers it).
    let (_, p) = build("ref r\nbegin GameMode\nGetPluginHeaderVersion \"FalloutNV.esm\"\nend");
    let mut h = Locals::default();
    let r = Vm::new(&t, &mut h, RunOpts { strict: true, ..RunOpts::default() }).run(&p.blocks[0].body);
    assert!(matches!(r, Err(fnv_script::vm::VmError::NotBuilt(_))), "{r:?}");
    let _ = XOp::Add;
}

// ---- loops (task A8) ------------------------------------------------------------

#[test]
fn while_loops_run_with_break_and_continue() {
    // Sum 1..=10 skipping 5, stopping after 8: 1+2+3+4+6+7+8 = 31.
    let h = run(
        "float i\nfloat s\nbegin GameMode\nwhile i < 10\n  i += 1\n  if eval i == 5\n    continue\n  endif\n  if eval i > 8\n    break\n  endif\n  s += i\nloop\nend",
    );
    assert_eq!(h.0[&2], Value::Num(31.0));
    assert_eq!(h.0[&1], Value::Num(9.0));
}

#[test]
fn nested_loops_and_their_structure() {
    let (_, p) = build("float i\nfloat j\nfloat n\nbegin GameMode\nwhile i < 3\n  i += 1\n  let j := 0\n  while j < 2\n    j += 1\n    n += 1\n  loop\nloop\nend");
    let Stmt::While { body, .. } = &body(&p)[0] else { panic!("outer while") };
    assert!(matches!(body.last(), Some(Stmt::While { .. })), "the inner loop is in the outer body");
    let h = run("float i\nfloat j\nfloat n\nbegin GameMode\nwhile i < 3\n  i += 1\n  let j := 0\n  while j < 2\n    j += 1\n    n += 1\n  loop\nloop\nend");
    assert_eq!(h.0[&3], Value::Num(6.0));
}

#[test]
fn a_runaway_loop_stops_at_the_step_budget() {
    let t = table();
    let (_, p) = build("float i\nbegin GameMode\nwhile 1\n  i += 1\nloop\nend");
    let mut h = Locals::default();
    let r = Vm::new(&t, &mut h, RunOpts { strict: true, max_steps: 1000 }).run(&p.blocks[0].body);
    assert!(matches!(r, Err(fnv_script::vm::VmError::Budget(1000))), "{r:?}");
}

#[test]
fn foreach_compiles_and_waits_for_arrays() {
    let (_, p) = build("float e\nfloat a\nbegin GameMode\nforeach e <- a\n  break\nloop\nend");
    assert!(matches!(&body(&p)[0], Stmt::ForEach { body, .. } if body == &[Stmt::Break]));
}

#[test]
fn loop_mistakes_are_errors() {
    let t = table();
    for (src, kind) in [
        ("begin GameMode\nloop\nend", "loop_without_while"),
        ("begin GameMode\nbreak\nend", "outside_a_loop"),
        ("float i\nbegin GameMode\nwhile i\nend", "while_without_loop"),
        ("float e\nbegin GameMode\nforeach e a\nloop\nend", "syntax"),
    ] {
        let c = compile(src, &t, &World, None);
        assert_eq!(c.errors().map(|d| d.kind).collect::<Vec<_>>(), [kind], "{src}");
    }
}

// ---- inline expressions, new variable types (tasks A9, A10) -----------------------

#[test]
fn inline_expressions_as_arguments_in_an_extended_unit() {
    // [documented: Inline Expression] `Player.SetActorValue Health (Player.GetActorValue
    // Health + 50)`; the unit is extended (it has a let), so the call line is one too.
    let (_, p) = build("float f\nbegin GameMode\nlet f := 1\nplayer.SetActorValue Health (player.GetActorValue Health + 50)\nend");
    let Stmt::Eval { expr } = &body(&p)[1] else { panic!("an expression statement: {:?}", body(&p)[1]) };
    let s = x(expr);
    assert!(s.starts_with("r1.100F(#"), "{s}");
    assert!(s.contains("(r1.100E(#") && s.ends_with("Add 50))"), "{s}");
}

#[test]
fn a_vanilla_unit_keeps_its_parentheses_dropped() {
    // Without any extender syntax, parentheses among arguments are dropped as the editor
    // drops them (scripts.md §3.4): a plain call statement.
    let (_, p) = build("begin GameMode\nplayer.AddItem (Caps001) 5\nend");
    assert!(matches!(&body(&p)[0], Stmt::Call { .. }));
}

#[test]
fn extender_functions_as_statements_make_the_unit_extended() {
    let t = table();
    let c = compile("array_var a\nbegin GameMode\nAr_Append a 5\nend", &t, &World, None);
    assert!(c.errors().next().is_none(), "{:?}", c.diags);
    let p = decode(&c.bytes, &t).program;
    assert!(matches!(&body(&p)[0], Stmt::Eval { expr: XExpr::Call { call } } if call.func == FuncId::Name("Ar_Append".into())));
    assert_eq!(c.variables[0].ty, VarType::Other("array_var".into()));
}

#[test]
fn inline_declarations_hex_and_binary_and_block_comments() {
    let h = run("begin GameMode\nint i = 0x10 + 0b101\n/* a comment\nint j = 99\n*/\nfloat f = i * 2 ; 42\nend");
    assert_eq!(h.0[&1], Value::Num(21.0));
    assert_eq!(h.0[&2], Value::Num(42.0), "j was inside the comment, so f is variable 2");
}

#[test]
fn inline_declarations_in_let_eval_and_while() {
    // `let int i := 10`, a type in `if eval` and in a `while` condition [documented: Inline
    // variable declaration]; `Let array_var a = …` with the macro's `=` (#1460, the
    // Script Runner files of the owner's mod lists).
    let h = run("begin GameMode\nlet int i := 10\nif eval (int j = i + 1) > 10\nlet float f := j * 2\nendif\nwhile (int k = 0) > 1\nloop\nend");
    assert_eq!(h.0[&1], Value::Num(10.0));
    assert_eq!(h.0[&2], Value::Num(11.0));
    assert_eq!(h.0[&3], Value::Num(22.0));
    let (c, _) = build("begin GameMode\nLet array_var a = ar_List 1, 2\nforeach array_var e <- a\nloop\nend");
    assert!(c.errors().next().is_none(), "{:?}", c.diags);
    assert_eq!(c.variables.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["a", "e"]);
    // A lambda's typed parameters stay parameters.
    let (c, _) = build("begin GameMode\nSetEventHandler \"OnHit\" ({ref rT, int iX} => PrintC \"x\")\nend");
    assert!(c.errors().next().is_none(), "{:?}", c.diags);
}

#[test]
fn a_block_lambdas_body_declares_its_variables() {
    // `ref r = …` inside `(Begin Function {…} … End)`: declared before the body compiles,
    // as the unit's declarations are (#1460, gr_ files of the owner's mod lists).
    let (c, _) = build("begin GameMode\nSetOnControlUpEventHandler (Begin Function {int iCon}\nref rW = Player.GetEquippedObject 5\nint iN = 0\niN = 1\nEnd) 1\nend");
    assert!(c.errors().next().is_none(), "{:?}", c.diags);
    assert!(c.variables.iter().any(|v| v.name == "rW") && c.variables.iter().any(|v| v.name == "iN"));
}

#[test]
fn calls_on_results_chain() {
    let l = lets("ref r\nfloat f\nbegin GameMode\nlet f := (GetSelf).GetAV Luck\nend");
    assert!(l[0].starts_with("(v2 Set= "), "{}", l[0]);
    assert!(l[0].contains(".100E(#"), "a call on the result of GetSelf: {}", l[0]);
}

#[test]
fn compiler_override_blocks_read_every_line_with_the_extender_grammar() {
    // Under `Begin _GameMode` `if` and `set` take extended expressions [documented:
    // Script Compiler Override]; xNVSE 6.4.9 reads `Sin 90 > 0.5` as (Sin 90) > 0.5 there
    // [observed: probe expressions/09], not as the page's `Sin (14 == 1)`.
    let (_, p) = build("float f\nbegin _GameMode\nif Sin 14 == 1\nset f to f + 1\nendif\nend");
    assert_eq!(p.blocks[0].event, 0, "the block is GameMode");
    let Stmt::If { arms, .. } = &body(&p)[0] else { panic!() };
    let [Token::X { e }] = &arms[0].cond.0[..] else { panic!("an extended condition: {:?}", arms[0].cond) };
    assert_eq!(x(e), "(Sin(14) Eq 1)", "Sin is the extender's (no opcode)");
    assert!(matches!(&arms[0].body[0], Stmt::Let { .. }));
    // The same line outside an override block: (Sin 14) == 1.
    let l = lets("float f\nbegin GameMode\nif eval Sin 14 == 1\nendif\nend");
    assert_eq!(l[0], "if (Sin(14) Eq 1)");
}

// ---- user-defined functions and lambdas (task A11) --------------------------------

#[test]
fn a_function_script_has_a_function_block_with_its_parameters() {
    let (c, p) = build("scn TestUDF
int a
int b
Begin Function {a, b}
  SetFunctionValue a + b
End
");
    assert_eq!(p.blocks[0].event, fnv_script::ir::FUNCTION_BLOCK);
    assert_eq!(p.blocks[0].params.as_ref().map(Vec::len), Some(2));
    assert!(matches!(&p.blocks[0].body[0], Stmt::Eval { .. }), "SetFunctionValue is the extender's");
    assert_eq!(c.variables.len(), 2);
}

#[test]
fn call_takes_the_function_and_up_to_fifteen_arguments() {
    let l = lets("int r
ref f
begin GameMode
let r := Call f 1 2 3
let r := Call f, 1, (2 + 3)
end");
    // A `ref` local holding the function is the variable [documented: Lambda, `Call
    // rCallback 10`].
    assert_eq!(l[0], "(v1 Set= Call(v2 1 2 3))");
    assert_eq!(l[1], "(v1 Set= Call(v2 1 (2 Add 3)))");
}

#[test]
fn a_joined_block_lambda_is_the_calls_first_argument() {
    // Public issue #2985: the opener's `(` can be the only separator.
    for opener in ["Call (begin function", "Call ( begin function", "Call(begin function"] {
        let source = format!(
            "int result\nint input\nbegin GameMode\n\
             let result := {opener}{{input}}\nSetFunctionValue input\nend) 7\n\
             {opener}{{input}}\nSetFunctionValue input\nend) 7\nend"
        );
        let (_, p) = build(&source);
        let [Stmt::Let { expr: XExpr::Assign { value, .. } }, Stmt::Eval { expr: XExpr::Call { call: statement } }] = body(&p)
        else {
            panic!("{opener}: expected a let and a call statement, got {:?}", body(&p));
        };
        let XExpr::Call { call: assigned } = value.as_ref() else {
            panic!("{opener}: expected a call on the right of let, got {value:?}");
        };
        for call in [assigned, statement] {
            assert_eq!(call.func, FuncId::Name("Call".into()), "{opener}");
            let [XExpr::Lambda { params, body, value: None, .. }, XExpr::Num { value, .. }] = call.args.as_slice() else {
                panic!("{opener}: expected a block lambda and the trailing argument, got {:?}", call.args);
            };
            assert_eq!(*value, 7.0, "{opener}");
            let [param] = params.as_slice() else { panic!("{opener}: expected one lambda parameter, got {params:?}") };
            assert!(param.on.is_none() && matches!(param.var, VarSlot::Local { index: 2, .. }), "{opener}: {param:?}");
            let [Stmt::Eval { expr }] = body.as_slice() else { panic!("{opener}: expected the lambda body, got {body:?}") };
            assert_eq!(x(expr), "SetFunctionValue(v2)", "{opener}");
        }
    }
}

#[test]
fn lambdas_in_both_forms() {
    let src = "int r
int x
ref f
begin GameMode
  let f := (begin function {x}
    SetFunctionValue x * 2
    let r := 1
  end)
  let f := ({x} => x + 1)
end";
    let l = lets(src);
    assert_eq!(l[0], "(v3 Set= lambda[1](2 statements))");
    assert_eq!(l[1], "(v3 Set= lambda[1]((v2 Add 1)))");
}

// ---- strings (task B1) ---------------------------------------------------------------

impl Locals {
    fn text(&self, index: u16) -> String {
        let id = self.0.get(&index).copied().unwrap_or_default().num() as u32;
        self.1.text(id).unwrap_or_default().to_string()
    }
}

#[test]
fn strings_join_copy_and_convert() {
    let h = run("string_var a\nstring_var b\nfloat n\nbegin GameMode\nlet a := \"Hello\"\nlet b := a\nlet a += \", world\"\nlet n := #\"0x10\" + #\" 2.5 \"\nlet b := b + $n + $3 + $1.25\nend");
    assert_eq!(h.text(1), "Hello, world");
    // Assignment copies: b kept "Hello" when a changed [documented: NVSE Expressions].
    assert_eq!(h.text(2), "Hello18.531.25");
    assert_eq!(h.0[&3], Value::Num(18.5));
}

#[test]
fn strings_compare_without_case_and_slice() {
    let h = run("string_var s\nstring_var t\nfloat a\nfloat b\nbegin GameMode\nlet s := \"Abcdef\"\nlet t := s[1:3] + s[-1] + s[0]\nif eval s == \"ABCDEF\" && \"apple\" < \"Banana\"\n  let a := 1\nendif\nlet b := s != \"x\"\nend");
    assert_eq!(h.text(2), "bcdfA");
    assert_eq!(h.0[&3], Value::Num(1.0));
    assert_eq!(h.0[&4], Value::Num(1.0));
}

#[test]
fn a_type_error_is_reported_and_the_script_goes_on() {
    // [documented: Let] a failing let reports and the script carries on.
    let t = table();
    let (c, p) = build("string_var s\nfloat a\nbegin GameMode\nlet a := \"x\" + 1\nlet a := 1 / 0\nlet a := 7\nend");
    let mut h = host_for(&c);
    Vm::new(&t, &mut h, RunOpts::default()).run(&p.blocks[0].body).expect("non-strict runs go on");
    assert_eq!(h.0[&2], Value::Num(7.0));
    let r = Vm::new(&t, &mut h, RunOpts { strict: true, ..RunOpts::default() }).run(&p.blocks[0].body);
    assert!(matches!(r, Err(fnv_script::vm::VmError::Type(_))), "{r:?}");
}

#[test]
fn tostring_and_tonumber() {
    let h = run("string_var s\nfloat n\nstring_var sl\nstring_var sm\nbegin GameMode\nlet s := ToString 42\nlet n := (ToNumber \"12\") * 2\nlet sl := ToString (Ar_List 1, 2)\nlet sm := ToString (Ar_Map \"k\"::5)\nend");
    // ToNumber is NVSE-aware, so without the parentheses its argument would be "12" * 2.
    assert_eq!(h.text(1), "42");
    assert_eq!(h.0[&2], Value::Num(24.0));
    // List arrays and stringmaps format as "NULL" [observed: b3_core_05_tostring_array_map].
    assert_eq!(h.text(3), "NULL");
    assert_eq!(h.text(4), "NULL");
}

// ---- arrays (task B3, the D2 functions) -------------------------------------------------

impl Locals {
    fn array_id(&self, index: u16) -> u32 {
        self.0.get(&index).copied().unwrap_or_default().num() as u32
    }
}

#[test]
fn arrays_share_on_assignment_and_index() {
    let h = run("array_var a\narray_var b\nfloat n\nfloat m\nbegin GameMode\nlet a := Ar_List 10, 20, 30\nlet b := a\nlet b[1] := 99\nlet n := a[1] + Ar_Size a\nlet a[3] := 5\nlet m := a[-0] + a[3]\nend");
    assert_eq!(h.array_id(1), h.array_id(2), "assignment shares [documented: NVSE Expressions]");
    assert_eq!(h.0[&3], Value::Num(102.0));
    assert_eq!(h.0[&4], Value::Num(15.0), "setting the key equal to the size appends");
}

#[test]
fn maps_keep_their_keys_sorted() {
    let h = run("array_var m\narray_var k\nstring_var s\nfloat f\nbegin GameMode\nlet m := Ar_Map \"b\"::2, \"A\"::1, \"c\"::3\nlet k := Ar_Keys m\nlet s := k[0] + k[1] + k[2]\nlet f := m->b + m[\"c\"]\nend");
    let text = { let id = h.0[&3].num() as u32; h.1.text(id).unwrap_or_default().to_string() };
    assert_eq!(text, "Abc");
    assert_eq!(h.0[&4], Value::Num(5.0));
}

#[test]
fn foreach_walks_an_array_and_a_string() {
    let h = run("array_var a\narray_var e\nfloat sum\nstring_var t\nstring_var c\nbegin GameMode\nlet a := Ar_Range 1, 5\nforeach e <- a\n  if eval *e == 4\n    break\n  endif\n  let sum += e[\"value\"] * 10 + e[\"key\"]\nloop\nforeach c <- \"abc\"\n  let t := c + t\nloop\nend");
    // 1..3 counted: values 10+20+30, keys 0+1+2.
    assert_eq!(h.0[&3], Value::Num(63.0));
    let id = h.0[&4].num() as u32;
    assert_eq!(h.1.text(id), Some("cba"));
}

#[test]
fn array_functions() {
    let h = run(
        "array_var a\narray_var b\nfloat f\nfloat g\nfloat k\nstring_var t\nbegin GameMode\nlet a := Ar_List 3, 1, 2\nlet b := Ar_Sort a\nlet f := b[0] * 100 + b[2]\nAr_Append a, 7\nAr_Insert a, 0, 9\nlet g := Ar_Erase a, 1:2\nlet k := Ar_Find 7, a\nlet t := TypeOf (Ar_Construct \"stringmap\")\nAr_Dump a\nend",
    );
    assert_eq!(h.0[&3], Value::Num(103.0));
    assert_eq!(h.0[&4], Value::Num(2.0), "two erased");
    assert_eq!(h.0[&5], Value::Num(2.0), "[9, 2, 7]: 7 is at key 2");
    let id = h.0[&6].num() as u32;
    assert_eq!(h.1.text(id), Some("StringMap"));
    assert_eq!(h.4, ["array #1 [0: 9, 1: 2, 2: 7]"]);
}

#[test]
fn temporary_arrays_are_freed() {
    let h = run("array_var a\nfloat n\nbegin GameMode\nlet n := Ar_Size (Ar_List 1, 2, 3)\nlet a := Ar_List (Ar_List 1), 2\nlet a := Ar_Null\nend");
    assert_eq!(h.0[&2], Value::Num(3.0));
    assert!(h.1.arrays.is_empty(), "nothing holds any array at the end: {:?}", h.1.arrays.keys().collect::<Vec<_>>());
}

/// A foreach whose body stops on an error lets its source go, and an element set on an
/// empty array variable does not hold the value: each held an array for ever, never swept
/// and saved [#1990]. The list's only holder after the error is the variable `a`.
#[test]
fn an_error_inside_foreach_or_an_element_set_holds_no_array() {
    let t = table();
    let list_refs = |h: &Locals| h.1.arrays.values().filter(|a| a.kind == fnv_script::vm::ArrayKind::List).map(|a| a.refs).collect::<Vec<_>>();
    for src in [
        "array_var a\narray_var e\nfloat n\nbegin GameMode\nlet a := Ar_List 1, 2\nforeach e <- a\n  let n := e[\"nvf no key\"]\nloop\nend",
        "array_var a\narray_var b\nbegin GameMode\nlet a := Ar_List 1, 2\nlet b[0] := a\nend",
    ] {
        let (c, p) = build(src);
        let mut h = host_for(&c);
        let r = Vm::new(&t, &mut h, RunOpts { strict: true, ..RunOpts::default() }).run(&p.blocks[0].body);
        assert!(r.is_err(), "{src}");
        assert_eq!(list_refs(&h), [1], "{src}");
    }
}

// ---- string functions, formats, output, version reports (D1, C4, D4, C5) ------------------

#[test]
fn format_codes() {
    // [documented: String Formatting] "i = %g and f = %.2f" with 42 and 3.1415926535.
    let h = run("string_var s\nstring_var t\nbegin GameMode\nlet s := Sv_Construct \"i = %g and f = %.2f\", 42, 3.1415926535\nlet t := Sv_Construct \"%x4|%5.1f|%-4g|%+g|%05.1f|%%|%{%g%}\", 255, 2.25, 7, 3, -1.5, 0, 9\nend");
    assert_eq!(h.text(1), "i = 42 and f = 3.14");
    assert_eq!(h.text(2), "00FF|  2.2|7   |+3|-01.5|%|");
}

#[test]
fn big_numbers_in_g_are_scientific() {
    // [observed: probe format1 in the game: "1e+06", "1.230e+05", "0.5", "123.457",
    // "1.23457E+06", "1.2e+02"].
    let h = run("string_var s\nstring_var t\nbegin GameMode\nlet s := Sv_Construct \"%g %.3e\", 1000000, 123000\nlet t := Sv_Construct \"%g|%g|%G|%.2g|%g\", 0.5, 123.456789, 1234567, 123.456, 100000\nend");
    assert_eq!(h.text(1), "1e+06 1.230e+05");
    assert_eq!(h.text(2), "0.5|123.457|1.23457E+06|1.2e+02|100000");
}

#[test]
fn eval_truthiness_for_arrays_and_strings() {
    // [observed: 2026-11-23 & 2026-11-24, GSDocMitchellHouse (0x00103DF9), Fallout: New
    // Vegas 1.4.0.525, xNVSE 6.4.9 with the 10 official base masters, probes
    // `core_05_eval_empty_array_and_string_truth` (1,0,1,1),
    // `core_24_eval_string_var` / `#3679` `core_24_string_var_destruct_zero`
    // (eval s: 1->0 across sv_Destruct, eval sEmp=1, eval (!sEmp)=0,
    // eval (sEmp && 5)=1, eval a1=1, eval a2=1, eval ar_Null=0, eval (!a1)=0,
    // eval (!aNull)=1), and `w4d_03_eval_empty_str_lit` (#3802 / PR #3803:
    // `PROBE|w4d_03_eval_empty_str_lit|lit0(cmp=1,val=0)|lit1(cmp=1,val=1)|con0Inline(cmp=0,val=-9)|c0=1,c1=1|v0=1,v1=1|saLit0(1:0)|saLit1(1:1)|saV0(1:1)`,
    // covering both `if eval` and standalone `eval` on bare `""`, `"abc"`, and allocated
    // empty `string_var`)]:
    // initialized empty arrays and allocated string_vars (even empty) evaluate truthy (1);
    // ar_Null, uninitialized or sv_Destruct-ed string_vars, and bare `""` literals
    // evaluate 0. Standalone `(eval ...)` coerces arrays (including array ID >= 2)
    // and strings to 1/0 while preserving numeric passthrough (`eval 5 == 5`, #2275
    // [design]).
    let h = run(
        "array_var aEmp\narray_var aTwo\narray_var aNul\nstring_var sUninit\nstring_var sEmp\nstring_var sNon\n\
         int bAE\nint bAN\nint bSU\nint bSE\nint bSN\nint bBefore\nint bAfter\nint bNotEmp\nint bNotUninit\nint bAnd\n\
         int bArrTwo\nint bArrNull\nint bNum\nint bEvalEmp\nint bNotArr\nint bNotArrNull\nint bIfEmpAnd\n\
         string_var sConEmp\nint bLitEmpty\nint bLitNon\nint bEvalLitEmpty\nint bEvalLitNon\nint bConEmp\n\
         begin GameMode\n\
         let aEmp := ar_Construct \"array\"\n\
         let aTwo := ar_Construct \"array\"\n\
         let aNul := ar_Null\n\
         let sEmp := \"\"\n\
         let sNon := \"x\"\n\
         let sConEmp := sv_Construct \"%z\" sEmp\n\
         if eval aEmp\n  let bAE := 1\nendif\n\
         if eval aNul\n  let bAN := 1\nendif\n\
         if eval sUninit\n  let bSU := 1\nendif\n\
         if eval sEmp\n  let bSE := 1\nendif\n\
         if eval sNon\n  let bSN := 1\nendif\n\
         if eval \"\"\n  let bLitEmpty := 1\nendif\n\
         if eval \"abc\"\n  let bLitNon := 1\nendif\n\
         let bEvalLitEmpty := eval \"\"\n\
         let bEvalLitNon := eval \"abc\"\n\
         if eval sConEmp\n  let bConEmp := 1\nendif\n\
         let bBefore := eval sNon\n\
         sv_Destruct sNon\n\
         let bAfter := eval sNon\n\
         let bNotEmp := !sEmp\n\
         let bNotUninit := !sUninit\n\
         let bAnd := (sEmp && 5) + (sUninit && 5)\n\
         let bArrTwo := eval aTwo\n\
         let bArrNull := eval aNul\n\
         let bNum := eval 5\n\
         let bEvalEmp := eval sEmp\n\
         let bNotArr := eval (!aTwo)\n\
         let bNotArrNull := eval (!aNul)\n\
         if eval (sEmp && 5)\n  let bIfEmpAnd := 1\nendif\n\
         end",
    );
    let get = |k: u16| h.0.get(&k).copied().unwrap_or(Value::ZERO);
    assert_eq!(get(7), Value::Num(1.0), "initialized empty array is truthy");
    assert_eq!(get(8), Value::Num(0.0), "ar_Null is falsy");
    assert_eq!(get(9), Value::Num(0.0), "uninitialized string_var is falsy");
    assert_eq!(get(10), Value::Num(1.0), "initialized empty string_var is truthy");
    assert_eq!(get(11), Value::Num(1.0), "non-empty string_var is truthy");
    assert_eq!(get(12), Value::Num(1.0), "eval sNon before sv_Destruct is 1");
    assert_eq!(get(13), Value::Num(0.0), "eval sNon after sv_Destruct is 0");
    assert_eq!(get(14), Value::Num(0.0), "!sEmp is 0");
    assert_eq!(get(15), Value::Num(1.0), "!sUninit is 1");
    assert_eq!(get(16), Value::Num(5.0), "sEmp && 5 == 5, sUninit && 5 == 0");
    assert_eq!(get(17), Value::Num(1.0), "eval aTwo (array ID >= 2) coerces to 1, not raw array ID");
    assert_eq!(get(18), Value::Num(0.0), "eval ar_Null coerces to 0");
    assert_eq!(get(19), Value::Num(5.0), "eval 5 preserves numeric passthrough (#2275)");
    assert_eq!(get(20), Value::Num(1.0), "eval sEmp (empty alive string_var) is 1");
    assert_eq!(get(21), Value::Num(0.0), "eval (!aTwo) is 0");
    assert_eq!(get(22), Value::Num(1.0), "eval (!aNul) is 1");
    assert_eq!(get(23), Value::Num(1.0), "if eval (sEmp && 5) is truthy");
    assert_eq!(get(25), Value::Num(0.0), "if eval \"\" bare empty string literal is falsy (w4d_03)");
    assert_eq!(get(26), Value::Num(1.0), "if eval \"abc\" bare non-empty string literal is truthy (w4d_03)");
    assert_eq!(get(27), Value::Num(0.0), "eval \"\" bare empty string literal coerces to 0 (w4d_03)");
    assert_eq!(get(28), Value::Num(1.0), "eval \"abc\" bare non-empty string literal coerces to 1 (w4d_03)");
    assert_eq!(get(29), Value::Num(1.0), "if eval sConEmp (sv_Construct empty string_var) is truthy (w4d_03)");
}

#[test]
fn format_signs_and_huge_fields() {
    // %g writes -0 as 0 (see crates/fnv-script/src/vm.rs, `printf_number`), %f keeps its sign
    // [observed: "%.4f" of -0 reads "-0.0000" in the original, #1919], + never doubles a minus,
    // and a huge precision or width is cut (#1145 review, #1134) [hypothesis: the C library's forms].
    let h = run("string_var s
string_var t
string_var u
begin GameMode
let s := Sv_Construct \"%g|%+.2f|%+g|%05.1f\", -0, -0, -2.5, -1.5
let t := Sv_Construct \"%.4000000000f|%4000000000g\", 1, 2
let u := Sv_Construct \"%.4f|%.4f|%+.1f\", -0, 0, 0
end");
    assert_eq!(h.text(1), "0|-0.00|-2.5|-01.5");
    assert_eq!(h.text(3), "-0.0000|0.0000|+0.0");
    assert!(h.text(2).len() < 1200, "{}", h.text(2).len());
}

#[test]
fn literal_codes_in_any_string() {
    let h = run("string_var s\nbegin GameMode\nlet s := \"say %qhi%q%rnext%e\"\nend");
    assert_eq!(h.text(1), "say \"hi\"\nnext");
}

#[test]
fn sv_functions() {
    let h = run(
        "string_var s\narray_var a\nfloat f\nfloat g\nfloat r\nfloat n\nstring_var x\nbegin GameMode\nlet s := \"One two ONE three\"\nlet f := Sv_Find \"one\", s, 1\nlet g := Sv_Length s\nlet r := Sv_Replace \"one|1\", s\nlet a := Sv_Split \"a,b:c\", \",:\"\nlet n := Sv_ToNumeric \"  -12.5e1xyz\" + Sv_GetChar \"A\", 0\nlet x := NumToHex 255, 4, 1\nend",
    );
    assert_eq!(h.0[&3], Value::Num(8.0), "found from position 1, without regard to case");
    assert_eq!(h.0[&4], Value::Num(17.0));
    assert_eq!(h.0[&5], Value::Num(2.0));
    assert_eq!(h.text(1), "1 two 1 three");
    assert_eq!(h.0[&6], Value::Num(-125.0 + 65.0));
    assert_eq!(h.text(7), "0x00FF");
}

/// Without regard to case, positions are characters of the text as it is: `İ` grows and
/// the Kelvin sign shrinks when lowered, which put a search's position in the wrong
/// place or out of the string [#1989]. Synthetic text (charter).
#[test]
fn sv_searches_without_case_keep_positions_in_non_ascii_text() {
    let h = run(
        "string_var s\nstring_var t\nfloat f\nfloat g\nfloat n\nfloat r\nfloat q\nbegin GameMode\nlet s := \"a\u{130}b\u{212A}c\u{130}d\"\nlet t := s\nlet f := Sv_Find \"k\", s\nlet g := Sv_Find \"\u{130}d\", s\nlet n := Sv_Count \"\u{130}\", s\nlet r := Sv_Replace \"k|-\", s\nlet q := Sv_Replace \"\u{130}|I\", t\nend",
    );
    assert_eq!(h.0[&3], Value::Num(3.0), "the Kelvin sign is k without regard to case");
    assert_eq!(h.0[&4], Value::Num(5.0));
    assert_eq!(h.0[&5], Value::Num(2.0));
    assert_eq!(h.0[&6], Value::Num(1.0));
    assert_eq!(h.text(1), "a\u{130}b-c\u{130}d");
    assert_eq!(h.0[&7], Value::Num(2.0));
    assert_eq!(h.text(2), "aIb\u{212A}cId");
}

#[test]
fn printc_and_version_reports() {
    let h = run("float v\nfloat p\nbegin GameMode\nPrintC \"v%g\", 1\nPrint \"x\" + $2\nlet v := GetNVSEVersion\nlet p := GetPluginVersion \"ShowOff xNVSE Plugin\"\nend");
    assert_eq!(h.4, ["v1", "x2"]);
    assert_eq!(h.0[&1], Value::Num(6.0));
    assert_eq!(h.0[&2], Value::Num(-1.0), "a plugin without a module here is absent");
}

#[test]
fn override_arguments_take_arithmetic_not_comparisons() {
    // [observed: probes expressions/08, 09 in the original game] under _GameMode,
    // `Abs -3 + 1` is 2 and `Sin 90 > 0.5` is (Sin 90) > 0.5.
    let l = lets("float r\nbegin _GameMode\nlet r := Abs -3 + 1\nlet r := Sin 90 > 0.5\nend");
    assert_eq!(l[0], "(v1 Set= 1221(((Neg 3) Add 1)))");
    assert_eq!(l[1], "(v1 Set= (Sin(90) Gt 0.5))");
}

#[test]
fn label_and_goto() {
    // [observed: probe loops/04 gives 3].
    let h = run("float r\nbegin GameMode\nlet r := 0\nLabel 1\nlet r += 1\nif r < 3\n  GoTo 1\nendif\nend");
    assert_eq!(h.0[&1], Value::Num(3.0));
}

#[test]
fn a_failing_condition_is_false_and_the_script_goes_on() {
    // [observed: probe loops/03] erasing forward runs past the shrunk list; the original
    // goes on and ends with 3 elements.
    let t = table();
    let src = "array_var a\nfloat i\nfloat n\nbegin GameMode\nlet a := Ar_List 1, 2, 3, 4, 5, 6\nlet i := -1\nlet n := Ar_Size a\nwhile n > (i += 1)\n  if eval a[i] % 2 == 0\n    Ar_Erase a, i\n  endif\nloop\nlet n := Ar_Size a\nend";
    let (c, p) = build(src);
    let mut h = host_for(&c);
    Vm::new(&t, &mut h, RunOpts::default()).run(&p.blocks[0].body).expect("goes on");
    assert_eq!(h.0[&3], Value::Num(3.0));
}

#[test]
fn math_and_rand() {
    let h = run("float s\nfloat r\nfloat f\nbegin GameMode\nlet s := (Sin 90) + Cos 0\nlet r := Rand 5, 5\nlet f := Floor 2.7 + Pow 2, 3\nend");
    assert_eq!(h.0[&1], Value::Num(2.0), "degrees [observed]");
    assert_eq!(h.0[&2], Value::Num(5.0));
    assert_eq!(h.0[&3], Value::Num(10.0));
}

#[test]
fn a_semicolon_in_an_extender_string_is_a_mismatched_quote() {
    // [observed: the GECK refused this line of probe strings/03, ext-bytecode.md §7].
    let t = table();
    let c = compile("scn X
array_var a
begin GameMode
let a := Sv_Split \"a,b;c\", \",;\"
end", &t, &World, None);
    let kinds: Vec<&str> = c.errors().map(|d| d.kind).collect();
    assert!(kinds.contains(&"mismatched_quotes"), "{kinds:?}");
}

#[test]
fn jip_regex_follows_the_probes() {
    // [observed: jip-forms regex.* rows, jip-strings.md §1].
    let src = [
        "string_var t\nstring_var r\nstring_var e\nfloat a\nfloat b\nfloat c\nfloat n\nfloat v\narray_var m\nbegin GameMode",
        "let t := \"Whatchamacallit\"",
        "let a := Sv_RegexMatch t, \"(What)(.*)\"",
        "let b := Sv_RegexMatch t, \"What\"",
        "let c := Sv_RegexMatch t, \"x(.*)\"",
        "let t := \"This subject has a submarine as a subsequence\"",
        "let m := Sv_RegexSearch t, \"\\b(sub)([^ ]*)\"",
        "let n := Ar_Size m",
        "let r := Sv_RegexReplace t, \"blip!|\\b(sub)([^ ]*)\"",
        "let v := ValidateRegex \"[\", e",
        "end",
    ]
    .join("\n");
    let h = run(&src);
    assert_eq!((h.0[&4], h.0[&5], h.0[&6]), (Value::Num(1.0), Value::Num(0.0), Value::Num(0.0)), "whole-string match");
    assert_eq!(h.0[&7], Value::Num(3.0));
    assert_eq!(h.text(2), "This blip! has a blip! as a blip!");
    assert_eq!(h.0[&8], Value::Num(5.0));
}

#[test]
fn typed_parameters_in_braces_declare_them() {
    // [observed: the original's runtime-compiled function scripts, jip-events.md §4].
    let t = table();
    let c = compile("Begin Function {float fD, ref rS}\nlet fD := ({int n} => n * 2)\nEnd\n", &t, &World, None);
    assert!(c.errors().next().is_none(), "{:?}", c.diags);
    let names: Vec<(&str, &VarType)> = c.variables.iter().map(|v| (v.name.as_str(), &v.ty)).collect();
    assert!(names.contains(&("fD", &VarType::FloatOrRef)) && names.contains(&("rS", &VarType::FloatOrRef)) && names.contains(&("n", &VarType::Int)), "{names:?}");
}

#[test]
fn a_signed_number_after_a_space_starts_the_next_argument() {
    // [observed: maxof.neg, aux.append.* with JIP in the original game].
    let h = run("float m\nfloat d\nfloat s\nbegin GameMode\nlet m := GetMaxOf -1 -5\nlet d := 7 -5\nlet s := GetMaxOf (3 -5) 0\nend");
    assert_eq!(h.0[&1], Value::Num(-1.0), "two arguments");
    assert_eq!(h.0[&2], Value::Num(2.0), "outside arguments a subtraction");
    assert_eq!(h.0[&3], Value::Num(0.0), "inside parentheses a subtraction");
}

#[test]
fn character_and_bit_functions() {
    let h = run("float d
float u
float a
float b
float s
float x
string_var t
string_var c
begin GameMode
let d := IsDigit 55
let u := ToUpper 97
let a := CharToAscii \"Az\"
let b := GetBit 5, 2
let s := SetBit 0, 3
let x := LogicalXor 6, 3
let t := IntToBin 5, 8, 1
let c := AsciiToChar 66
end");
    assert_eq!((h.0[&1], h.0[&2], h.0[&3]), (Value::Num(1.0), Value::Num(65.0), Value::Num(65.0)));
    assert_eq!((h.0[&4], h.0[&5], h.0[&6]), (Value::Num(1.0), Value::Num(8.0), Value::Num(5.0)));
    assert_eq!(h.text(7), "0b00000101");
    assert_eq!(h.text(8), "B");
}

/// A host whose user-defined functions all answer 0 at once, to count the calls.
struct Answers(Locals, u32);

impl Host for Answers {
    fn var_kind(&mut self, var: &fnv_script::VarRef) -> fnv_script::vm::VarKind {
        self.0.var_kind(var)
    }
    fn ext(&mut self) -> Option<&mut fnv_script::vm::ExtStore> {
        self.0.ext()
    }
    fn get_var(&mut self, var: &fnv_script::VarRef) -> Result<Value, String> {
        self.0.get_var(var)
    }
    fn set_var(&mut self, var: &fnv_script::VarRef, value: Value) -> Result<(), String> {
        self.0.set_var(var, value)
    }
    fn ref_value(&mut self, r: fnv_script::RefIndex) -> Result<Value, String> {
        self.0.ref_value(r)
    }
    fn call(&mut self, opcode: u16, on: Option<Value>, args: &[ArgValue]) -> Result<Value, String> {
        self.0.call(opcode, on, args)
    }
    fn call_function(&mut self, _f: &fnv_script::vm::XVal, _on: Option<Value>, _args: Vec<fnv_script::vm::XVal>) -> Result<fnv_script::vm::XVal, String> {
        self.1 += 1;
        Ok(fnv_script::vm::XVal::Num(0.0))
    }
}

/// The array functions that call a function per element count a step per call, so the
/// budget stops a long array as it stops a loop; Ar_Resize and IntToBin's width are
/// capped as Ar_Init and the format fields are [#1991].
#[test]
fn array_calls_count_steps_and_sizes_are_capped() {
    let t = table();
    let src = "array_var a\nfloat n\nbegin GameMode\nlet a := Ar_Range 1, 1000\nlet n := Ar_Any a, 1\nend";
    let (c, p) = build(src);
    let mut h = Answers(host_for(&c), 0);
    let r = Vm::new(&t, &mut h, RunOpts { strict: true, max_steps: 100 }).run(&p.blocks[0].body);
    assert!(r.is_err(), "the budget stops the calls");
    assert!(h.1 < 100, "{} calls on a budget of 100 steps", h.1);
    let mut h = Answers(host_for(&c), 0);
    Vm::new(&t, &mut h, RunOpts { strict: true, ..RunOpts::default() }).run(&p.blocks[0].body).expect("runs within the default budget");
    assert_eq!(h.1, 1000);

    let h = run("array_var a\nfloat n\nstring_var s\nbegin GameMode\nlet a := Ar_List 1\nAr_Resize a, 1000000000000\nlet n := Ar_Size a\nlet s := IntToBin 5, 1000000000\nend");
    assert_eq!(h.0[&2], Value::Num(1_000_000.0));
    assert_eq!(h.text(3).len(), 512);
}

#[test]
fn a_variable_named_like_an_extender_function_is_refused() {
    // [observed: `ref rK`, ReleaseKey's alias, does not compile in the original].
    let t = table();
    let c = compile("Begin Function { }
ref rK
ref rF
End
", &t, &World, None);
    let bad: Vec<&str> = c.errors().map(|d| d.detail.as_str()).collect();
    assert_eq!(bad, ["rK"]);
}

#[test]
fn quaternions_follow_the_probes() {
    // [observed: nvse-math rows in the original game]. Results go to the named variables.
    let t = table();
    let (c, p) = build("float w
float x
float y
float z
begin GameMode
QFromEuler \"w\" \"x\" \"y\" \"z\" 90 0 0
end");
    struct Named(Locals, Vec<(String, u16)>);
    impl Host for Named {
        fn var_kind(&mut self, v: &fnv_script::VarRef) -> fnv_script::vm::VarKind {
            self.0.var_kind(v)
        }
        fn get_var(&mut self, v: &fnv_script::VarRef) -> Result<Value, String> {
            self.0.get_var(v)
        }
        fn set_var(&mut self, v: &fnv_script::VarRef, x: Value) -> Result<(), String> {
            self.0.set_var(v, x)
        }
        fn ref_value(&mut self, r: fnv_script::RefIndex) -> Result<Value, String> {
            self.0.ref_value(r)
        }
        fn call(&mut self, o: u16, on: Option<Value>, a: &[ArgValue]) -> Result<Value, String> {
            self.0.call(o, on, a)
        }
        fn local_var(&mut self, name: &str) -> Option<fnv_script::VarRef> {
            let index = self.1.iter().find(|(n, _)| n == name)?.1;
            Some(fnv_script::VarRef { on: None, var: VarSlot::Local { index, letter: fnv_script::ir::VarLetter::Float } })
        }
    }
    let names = c.variables.iter().map(|v| (v.name.clone(), v.index as u16)).collect();
    let mut h = Named(host_for(&c), names);
    Vm::new(&t, &mut h, RunOpts { strict: true, ..RunOpts::default() }).run(&p.blocks[0].body).expect("runs");
    let g = |i: u16| h.0 .0[&i].num();
    assert!((g(1) - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-4 && (g(2) - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-4 && g(3).abs() < 1e-9 && g(4).abs() < 1e-9);
}

#[test]
fn jip_reports_and_pure_functions() {
    let h = run(
        "float i
float v
float a
float q
float m
float av
float ok
string_var n
string_var s
ref r
begin GameMode
let i := IsPluginInstalled \"JIP NVSE Plugin\"
let v := GetPluginVersion \"jip nvse plugin\"
let a := Fatan2 1, -1
let q := Fsqrt -1
let m := GetMaxOf 1, 7, 3
let av := StringToActorValue \"strength\"
let ok := ValidateModIndex 200
let n := GetModName 0
let r := StringToRef \"FalloutNV.esm:104C0C\"
let s := RefToString r
end",
    );
    assert_eq!(h.0[&1], Value::Num(1.0));
    assert_eq!(h.0[&2], Value::Num(57.3));
    // 135 at six digits; 135.00020525 at eight [observed: jip-probes rules1, #1025].
    assert!(matches!(h.0[&3], Value::Num(a) if (a - 135.000_205_25).abs() < 2e-5), "{:?}", h.0[&3]);
    assert_eq!(h.0[&4], Value::Num(0.0), "[observed]");
    assert_eq!(h.0[&5], Value::Num(7.0));
    assert_eq!(h.0[&6], Value::Num(5.0));
    assert_eq!(h.0[&7], Value::Num(-1.0));
    assert_eq!(h.text(8), "FalloutNV");
    assert_eq!(h.text(9), "FalloutNV.esm:104C0C");
}

/// Tale of Two Wastelands' start-up checks pass for the plugins built into the engine:
/// Stewie's Tweaks at 960 (above TTW's minimum 941, #3183) and its own ttw_nvse (ext-version.md §5).
#[test]
fn ttws_plugin_checks_find_the_plugins_built_in() {
    let h = run(
        "float s
float t
float u
begin GameMode
let s := GetPluginVersion \"lStewieAl's Tweaks\"
let t := IsPluginInstalled \"ttw_nvse\"
let u := IsPluginInstalled \"Not A Plugin\"
end",
    );
    assert_eq!(h.0[&1], Value::Num(960.0));
    assert_eq!(h.0[&2], Value::Num(1.0));
    assert_eq!(h.0[&3], Value::Num(0.0));
}

#[test]
fn fatan_follows_the_games_approximation() {
    // [observed: jip-probes rules1 and rules2 (#1025), eight and ten decimals].
    let h = run(
        "float a
float b
float c
float d
float e
float f
begin GameMode
let a := Fatan 1
let b := Fatan 0.5
let c := Fatan 10
let d := Fatan2 -1, -1
let e := Fatan2 1, 0
let f := Fatan 0.3
end",
    );
    for (k, want) in [(1, 44.999_803_18), (2, 26.564_936_48), (3, 84.289_417_94), (4, -135.000_205_25), (5, 90.000_002_5), (6, 16.699_303_67)] {
        assert!(matches!(h.0[&k], Value::Num(v) if (v - want).abs() < 2e-5), "{k}: {:?} against {want}", h.0[&k]);
    }
}

#[test]
fn anh_functions_follow_their_pages() {
    // [documented: Ar_Max, Ar_Min, Ar_RangeMath ((6,2,10,7): 10, 2, 8); V3Mult ((1,2,3),
    // (4,5,6): (4,10,18)); V3MultByScalar ((1,2,3) by 2: (2,4,6)); Sv_Capitalize
    // ("pepeglare": "Pepeglare"); GetAngleQuadrant (0.785: 1); Sv_PadStart; DegToRad].
    let h = run(
        "float a
float b
float c
float d
float e
float f
float g
float q
string_var s
string_var t
array_var aA
array_var aB
array_var aOut
begin GameMode
let aA := ar_List 6, 2, 10, 7
let a := Ar_Max aA
let b := Ar_Min aA
let c := Ar_RangeMath aA
let aA := ar_List 1, 2, 3
let aB := ar_List 4, 5, 6
let aOut := V3Mult aA aB
let d := aOut[2]
let aOut := V3MultByScalar aA 2
let e := aOut[1]
let f := V3Dotproduct aA aB
let g := DegToRad 180
let q := GetAngleQuadrant 0.785
let s := Sv_Capitalize \"pepeglare snig\"
let t := Sv_PadStart \"5\" 3 \"0\"
end",
    );
    assert_eq!((h.0[&1], h.0[&2], h.0[&3]), (Value::Num(10.0), Value::Num(2.0), Value::Num(8.0)));
    assert_eq!((h.0[&4], h.0[&5], h.0[&6]), (Value::Num(18.0), Value::Num(4.0), Value::Num(32.0)));
    assert!(matches!(h.0[&7], Value::Num(x) if (x - std::f64::consts::PI).abs() < 1e-6));
    assert_eq!(h.0[&8], Value::Num(1.0));
    assert_eq!(h.text(9), "Pepeglare snig");
    assert_eq!(h.text(10), "005");
}

#[test]
fn anh_wave4_degrees_scancode_and_guards() {
    // [observed: 2026-11-23, GSDocMitchellHouse (0x00103DF9), Fallout: New Vegas
    // 1.4.0.525 + xNVSE 6.4.9 + AnhNVSE 131 with the 10 official base masters, probes
    // anh_01..anh_05, anh_09, anh_10]: GetAngleQuadrant and QFromAxisAngleAlt take
    // degrees, ScancodeToChar returns uppercase letters and DirectInput key names, and
    // Ar_Max/Ar_Min/Ar_RangeMath return safe 0 [design] on empty/string arrays.
    let h = run(
        "float q0
float q45
float q90
float q135
float q180
float q225
float q270
float q315
float qNeg45
float qNegHalf
float q0w
float q0z
float q90w
float q90x
float q90y
float q90z
float qSmallZ
float eMax
float eMin
float eRng
float sMax
float lerpMid
float lerpEx2
float lerpExNeg
string_var kEsc
string_var kF1
string_var kE
string_var kA
string_var kNone
string_var kMouse
string_var kSpace
string_var kUp
string_var k220
string_var kZero
string_var kNeg
string_var k221
string_var k255
array_var aAxis
array_var aQ
array_var aEmpty
array_var aStrs
array_var aA
array_var aB
array_var aL
begin GameMode
let q0 := GetAngleQuadrant 0
let q45 := GetAngleQuadrant 45
let q90 := GetAngleQuadrant 90
let q135 := GetAngleQuadrant 135
let q180 := GetAngleQuadrant 180
let q225 := GetAngleQuadrant 225
let q270 := GetAngleQuadrant 270
let q315 := GetAngleQuadrant 315
let qNeg45 := GetAngleQuadrant -45
let qNegHalf := GetAngleQuadrant -0.5
let aAxis := ar_List 0, 0, 1
let aQ := QFromAxisAngleAlt aAxis 0
let q0w := aQ[0]
let q0z := aQ[3]
let aQ := QFromAxisAngleAlt aAxis 90
let q90w := aQ[0]
let q90x := aQ[1]
let q90y := aQ[2]
let q90z := aQ[3]
let aQ := QFromAxisAngleAlt aAxis 1.570796
let qSmallZ := aQ[3]
let aEmpty := ar_Construct \"array\"
let eMax := Ar_Max aEmpty
let eMin := Ar_Min aEmpty
let eRng := Ar_RangeMath aEmpty
let aStrs := ar_List \"alpha\", \"beta\"
let sMax := Ar_Max aStrs
let aA := ar_List 0, 0, 0
let aB := ar_List 10, 20, 30
let aL := V3Lerp aA aB 0.5
let lerpMid := aL[1]
let aL := V3Lerp aA aB 2
let lerpEx2 := aL[0]
let aL := V3Lerp aA aB -1
let lerpExNeg := aL[0]
let kEsc := ScancodeToChar 1
let kF1 := ScancodeToChar 59
let kE := ScancodeToChar 18
let kA := ScancodeToChar 30
let kNone := ScancodeToChar 999
let kMouse := ScancodeToChar 256
let kSpace := ScancodeToChar 57
let kUp := ScancodeToChar 200
let k220 := ScancodeToChar 220
let kZero := ScancodeToChar 0
let kNeg := ScancodeToChar -1
let k221 := ScancodeToChar 221
let k255 := ScancodeToChar 255
end",
    );
    assert_eq!(h.0[&1], Value::Num(1.0), "0 deg -> Q1");
    assert_eq!(h.0[&2], Value::Num(1.0), "45 deg -> Q1");
    assert_eq!(h.0[&3], Value::Num(2.0), "90 deg -> Q2");
    assert_eq!(h.0[&4], Value::Num(2.0), "135 deg -> Q2");
    assert_eq!(h.0[&5], Value::Num(3.0), "180 deg -> Q3");
    assert_eq!(h.0[&6], Value::Num(3.0), "225 deg -> Q3");
    assert_eq!(h.0[&7], Value::Num(4.0), "270 deg -> Q4");
    assert_eq!(h.0[&8], Value::Num(4.0), "315 deg -> Q4");
    assert_eq!(h.0[&9], Value::Num(4.0), "-45 deg -> Q4");
    assert_eq!(h.0[&10], Value::Num(1.0), "-0.5 truncates to 0 -> Q1");
    assert_eq!((h.0[&11], h.0[&12]), (Value::Num(1.0), Value::Num(0.0)), "0 deg -> [1,0,0,0]");
    let half_sqrt2 = std::f64::consts::FRAC_1_SQRT_2;
    assert!(matches!(h.0[&13], Value::Num(v) if (v - half_sqrt2).abs() < 1e-6), "90 deg w: {:?}", h.0[&13]);
    assert!(matches!(h.0[&14], Value::Num(v) if v.abs() < 1e-6), "90 deg x: {:?}", h.0[&14]);
    assert!(matches!(h.0[&15], Value::Num(v) if v.abs() < 1e-6), "90 deg y: {:?}", h.0[&15]);
    assert!(matches!(h.0[&16], Value::Num(v) if (v - half_sqrt2).abs() < 1e-6), "90 deg z: {:?}", h.0[&16]);
    assert!(matches!(h.0[&17], Value::Num(v) if (v - (std::f64::consts::FRAC_PI_2.to_radians() / 2.0).sin()).abs() < 1e-6), "1.570796 deg z: {:?}", h.0[&17]);
    assert_eq!((h.0[&18], h.0[&19], h.0[&20], h.0[&21]), (Value::Num(0.0), Value::Num(0.0), Value::Num(0.0), Value::Num(0.0)));
    assert_eq!((h.0[&22], h.0[&23], h.0[&24]), (Value::Num(10.0), Value::Num(20.0), Value::Num(-10.0)));
    assert_eq!(h.text(25), "Escape");
    assert_eq!(h.text(26), "F1");
    assert_eq!(h.text(27), "E");
    assert_eq!(h.text(28), "A");
    assert_eq!(h.text(29), "\x1d");
    assert_eq!(h.text(30), "Left Mouse Button");
    assert_eq!(h.text(31), "Spacebar");
    assert_eq!(h.text(32), "Up Arrow");
    assert_eq!(h.text(33), "\x1d");
    assert_eq!(h.text(34), "\x1d");
    assert_eq!(h.text(35), "\x1d");
    assert_eq!(h.text(36), "\x1d");
    assert_eq!(h.text(37), "\x1d");
}

#[test]
fn jip_aux_variables_and_refmaps_follow_the_probes() {
    // [observed: jip-stores rows, jip-variable-stores.md] on the calling reference.
    let src = [
        "float a\nfloat b\nfloat c\nfloat d\nfloat e\nfloat f\nfloat g\nfloat h\nstring_var s\nfloat k\nfloat m\nfloat q\nbegin GameMode",
        "AuxVarSetFlt \"nvfA\" 3.5",
        "let a := AuxVarGetFlt \"NVFa\"",
        "AuxiliaryVariableSetFloat \"nvfL\", 1, -1\nAuxiliaryVariableSetFloat \"nvfL\", 2, -1\nAuxiliaryVariableSetFloat \"nvfL\", 3, -1",
        "let b := AuxiliaryVariableGetSize \"nvfL\"",
        "let c := AuxiliaryVariableGetFloat \"nvfL\", -1",
        "AuxiliaryVariableSetFloat \"nvfGap\" 1 5",
        "let d := AuxiliaryVariableGetSize \"nvfGap\"",
        "AuxiliaryVariableSetString \"nvfA\" \"abc\"",
        "let e := AuxiliaryVariableGetType \"nvfA\"",
        "let f := AuxiliaryVariableErase \"nvfL\" 0",
        "let g := AuxiliaryVariableGetFloat \"nvfL\"",
        "let h := AuxiliaryVariableErase \"nvfL\"",
        "let s := AuxiliaryVariableGetString \"nvfA\"",
        "RefMapArraySetFloat \"*nvfM\" 4.5\nRefMapArraySetFloat \"*nvfM\" 5.5",
        "let k := RefMapArrayGetFloat \"*nvfM\"",
        "let m := RefMapArrayGetSize \"*nvfM\"",
        "let q := ar_Size (RefMapArrayGetNext \"*nvfM\")",
        "end",
    ]
    .join("\n");
    let t = table();
    let (c, p) = build(&src);
    let mut h = host_for(&c);
    h.5 = Some(PLAYER);
    Vm::new(&t, &mut h, RunOpts { strict: true, ..RunOpts::default() }).run(&p.blocks[0].body).expect("runs");
    assert_eq!(h.0[&1], Value::Num(3.5), "names ignore case");
    assert_eq!(h.0[&2], Value::Num(3.0), "-1 appends");
    assert_eq!(h.0[&3], Value::Num(3.0), "-1 reads the last");
    assert_eq!(h.0[&4], Value::Num(0.0), "past the end does nothing");
    assert_eq!(h.0[&5], Value::Num(4.0), "a new type replaces the element");
    assert_eq!(h.0[&6], Value::Num(2.0), "erase gives the new count");
    assert_eq!(h.0[&7], Value::Num(2.0), "later elements move down");
    assert_eq!(h.0[&8], Value::Num(-1.0), "erasing the whole variable gives -1");
    assert_eq!(h.text(9), "abc");
    assert_eq!(h.0[&10], Value::Num(5.5));
    assert_eq!(h.0[&11], Value::Num(1.0), "one key, set twice");
    // GetNext without a GetFirst starts at the first pair; ar_Size of ar_Null is -1 only
    // after the last pair, here the pair itself has 2 elements.
    assert_eq!(h.0[&12], Value::Num(2.0));
    let x = &h.1;
    assert!(x.jip_temp.refmaps.contains_key("*nvfm"), "a * name is temporary");
    assert!(!x.jip.aux.is_empty() && x.jip.refmaps.is_empty());
}

#[test]
fn string_edits_from_the_probes() {
    let h = run(
        "string_var s\nstring_var t\nstring_var u\nfloat c\nbegin GameMode\nlet s := \"lo\"\nSv_Insert \"Hel\", s, 0\nlet t := \"abcdef\"\nSv_Erase t, 1, 2\nlet u := Sv_SubString \"abcdef\", 1, 3\nlet c := Sv_Compare \"Beta\", \"Alpha\"\nend",
    );
    assert_eq!(h.text(1), "Hello");
    assert_eq!(h.text(2), "adef");
    assert_eq!(h.text(3), "bcd");
    assert_eq!(h.0[&4], Value::Num(1.0));
}

#[test]
fn let_macro_assigns_to_a_subscript() {
    // [observed: JustAssortedMods.esp's handlers, `aEvent["Count"] = …`, compile in the
    // original]
    let h = run("array_var a\nfloat n\nbegin GameMode\nlet a := Ar_List 10, 20\na[1] = 7\na[0] += 1\nlet n := a[0] + a[1]\nend");
    assert_eq!(h.0[&2], Value::Num(18.0));
}

#[test]
fn ignore_crippled_limbs_is_an_actor_value() {
    // [documented: xEdit ActorValue 72; observed: JustAssortedMods.esp names it]
    let (c, _) = build("float f\nbegin GameMode\nlet f := player.GetAV IgnoreCrippledLimbs\nend");
    assert!(c.diags.iter().all(|d| !d.error), "{:?}", c.diags);
}

// ---- grammar forms of the owner's mods' scripts (#1470) ----------------------------

#[test]
fn a_typed_declaration_as_an_output_argument() {
    // `(float f)` where a function writes a variable declares it there (#1470: ln_
    // files of the owner's mod lists, GetNifRotAlt and GetAngleEx).
    let (c, p) = build("begin GameMode\nPlayer.GetNifRotAlt \"RadNeedle\" (float fA) (float fB) (float fC) 2 2\nPlayer.GetAngleEx (float fX) (float fY) (float fZ)\nend");
    assert_eq!(c.variables.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["fA", "fB", "fC", "fX", "fY", "fZ"]);
    let Stmt::Eval { expr: XExpr::Call { call } } = &body(&p)[0] else { panic!("{:?}", body(&p)[0]) };
    assert_eq!(call.args.iter().map(x).collect::<Vec<_>>(), ["\"RadNeedle\"", "v1", "v2", "v3", "2", "2"]);
}

#[test]
fn a_declaration_inside_an_arguments_inline_expression() {
    let (c, p) = build("begin GameMode\nSetUIFloatAlt \"HudMainMenu\\X\\_y\" (int iState = GetPCDetectionState)\nend");
    assert_eq!(c.variables.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["iState"]);
    let Stmt::Eval { expr: XExpr::Call { call } } = &body(&p)[0] else { panic!("{:?}", body(&p)[0]) };
    assert_eq!(x(&call.args[1]), "(v1 Set= GetPCDetectionState())");
}

#[test]
fn a_format_string_takes_its_values_before_the_next_parameter() {
    // [documented: SetModelPathEx, Bugs: 20 `%z` values, then the base form.]
    let z = "%z".repeat(20);
    let s = vec!["s"; 20].join(" ");
    let (_, p) = build(&format!("begin GameMode\nstring_var s = \"\"\nref rB = Caps001\nSetModelPathEx \"a\\b.nif{z}\" {s} rB\nend"));
    let Some(Stmt::Eval { expr: XExpr::Call { call } }) = body(&p).iter().find(|s| matches!(s, Stmt::Eval { .. })) else { panic!() };
    assert_eq!(call.args.len(), 22);
    assert_eq!(x(&call.args[21]), "v2");
}

#[test]
fn values_follow_variables_among_dispatched_arguments() {
    let (_, p) = build("int xkD\nbegin GameMode\nDispatchEventAlt \"B42OCND\" 0\nDispatchEventAlt \"name\" xkD 0\nend");
    let calls: Vec<_> = body(&p).iter().filter_map(|s| match s {
        Stmt::Eval { expr: XExpr::Call { call } } => Some(call.args.iter().map(x).collect::<Vec<_>>()),
        _ => None,
    }).collect();
    assert_eq!(calls, [vec!["\"B42OCND\"", "0"], vec!["\"name\"", "v1", "0"]]);
}

#[test]
fn a_call_lines_surplus_arguments_are_dropped() {
    // ln_B42Optics.txt hands SetJohnnyOnRenderUpdateEventHandler (three parameters) four:
    // the fourth is dropped with a warning, as the vanilla compiler drops them (#1470).
    let (c, _) = build("begin GameMode\nSetJohnnyOnRenderUpdateEventHandler 1 (begin function {}\nDispatchEventAlt \"B42OCND\" 0\nfloat xiR = 1\nend) 0 0\nend");
    let warned: Vec<_> = c.diags.iter().filter(|d| d.kind == "surplus_arguments").collect();
    assert!(warned.len() == 1 && !warned[0].error, "{:?}", c.diags);
    // What reading the dropped ones registered is put back: no reference entry for
    // Caps001, no warning for the bare word, and the next lambda keeps the next number.
    let (c, p) = build("begin GameMode\nSetJohnnyOnRenderUpdateEventHandler 1 ({} => PrintC \"a\") 0 Caps001 Nothing ({} => PrintC \"b\")\nSetJohnnyOnRenderUpdateEventHandler 1 ({} => PrintC \"c\") 0\nend");
    assert!(c.refs.is_empty(), "{:?}", c.refs);
    assert_eq!(c.diags.iter().map(|d| d.kind).collect::<Vec<_>>(), ["surplus_arguments"]);
    let lambdas: Vec<u32> = body(&p).iter().filter_map(|s| match s {
        Stmt::Eval { expr: XExpr::Call { call } } => match &call.args[1] {
            XExpr::Lambda { index, .. } => Some(*index),
            _ => None,
        },
        _ => None,
    }).collect();
    assert_eq!(lambdas, [0, 1]);
    // An argument that cannot be read is not dropped: the line fails.
    let c = compile("begin GameMode\nSetJohnnyOnRenderUpdateEventHandler 1 ({} => PrintC \"a\") 0 Nobody.GetAV Luck\nend", &table(), &World, None);
    assert!(c.errors().any(|d| d.kind == "syntax"), "{:?}", c.diags);
}

#[test]
fn a_block_lambda_may_open_with_a_space() {
    // `CallWhen ( begin function {}` (Bullet Belt Framework's ln_ file, #1470).
    let (c, _) = build("begin GameMode\nCallWhen ( begin function {}\nPrintC \"x\"\nend) ({}=> (GetSecondsPassed) != 0) (0b10100)\nend");
    assert!(c.errors().next().is_none(), "{:?}", c.diags);
}

/// The projectile-impact handlers of the probe set (`tools/capture/jip_probes.py`, rows
/// proj.reg and proj.reg2): both compile in the original [observed: golden projectile,
/// #2036].
const PROJ_REG: &str = r#"ref rP
ref rGun
ref rProj
float fRes
Begin Function { }
let rP := GetFormFromMod "FalloutNV.esm" "14"
let rGun := GetFormFromMod "FalloutNV.esm" "434F"
let rProj := GetWeaponProjectile rGun
SetOnProjectileImpactEventHandler (Begin Function { }
  rP.AuxVarSetFlt "*nvfPJ.count" (1) -1
  rP.AuxVarSetFlt "*nvfPJ.impact.isref" (IsReference (GetSelf.GetProjectileRefImpactRef)) -1
  rP.AuxVarSetFlt "*nvfPJ.source.player" ((GetSelf.GetProjectileRefSource) == rP) -1
  rP.AuxVarSetFlt "*nvfPJ.self.type" (GetType (GetSelf.GetBaseObject)) -1
End) 1 rProj
SetFunctionValue 1"#;

const PROJ_REG2: &str = r#"ref rP
ref rGun
ref rProj
float fRes
Begin Function { }
let rP := GetFormFromMod "FalloutNV.esm" "14"
let rGun := GetFormFromMod "FalloutNV.esm" "434F"
let rProj := GetWeaponProjectile rGun
SetOnProjectileImpactEventHandler (Begin Function { }
  rP.AuxVarSetFlt "*nvfPJ.impact.type" (GetType ((GetSelf.GetProjectileRefImpactRef).GetBaseObject)) -1
  rP.AuxVarSetFlt "*nvfPJ.weapon.ispistol" ((GetSelf.GetProjectileRefWeapon) == rGun) -1
  rP.AuxVarSetFlt "*nvfPJ.damage" (GetSelf.GetProjectileRefDamage) -1
End) 1 rProj
SetFunctionValue 1"#;

#[test]
fn projectile_impact_lambdas_compile() {
    // `GetSelf.Function` inside an extender expression: the call on the function's
    // result [documented: Chained dot syntax]. build() also decodes the bytes; the probe
    // runner adds the outer block's End.
    for src in [PROJ_REG, PROJ_REG2] {
        build(&format!("{src}\nEnd\n"));
    }
}

#[test]
fn a_dotted_head_naming_nothing_is_still_refused() {
    let t = table();
    let c = compile("Begin Function { }
SetFunctionValue (NoSuchThing.GetBaseObject)
End
", &t, &World, None);
    let bad: Vec<&str> = c.errors().map(|d| d.detail.as_str()).collect();
    assert_eq!(bad, ["NoSuchThing"]);
}

/// A probe step's body as a function script, with the locals the probes declare.
fn probe(body: &str) -> String {
    format!("ref rObj\nref rBase\nref rG\nref rP\nfloat fSave\nBegin Function {{ }}\n{body}\nEnd\n")
}

fn runtime_errors(body: &str) -> Vec<&'static str> {
    let c = fnv_script::compile::compile_runtime(&probe(body), &table(), &World);
    c.errors().map(|d| d.kind).collect()
}

fn editor_errors(body: &str) -> Vec<&'static str> {
    let c = compile(&probe(body), &table(), &World, None);
    c.errors().map(|d| d.kind).collect()
}

#[test]
fn runtime_refuses_a_fraction_for_an_int_parameter() {
    // [observed: L5.ext rerun 2026-11-10, #2037] each of these did not compile in the
    // original's runtime compiler (no row in the golden).
    for body in [
        "SetBaseKarma 37.5 rBase",                                  // jip.n7 karma.set
        "SetNthDestructionStageTrait rObj 1 1 3.5",                 // jip6 dest.settrait.1.1.3.5
        "SetFunctionValue (sv_Construct \"%g\" (ApplyEasingAlt 0.5 1.5 0))", // showoff3 so3.ease.badcode
        "SetFunctionValue (RandSeeded 1.7 3.2)",                    // showoff3 so3.seed.range.fraction
        "SetCasinoWinnings rObj 2.7",                               // jg3 casino.setwin.tops.frac
        "SetRegionWeatherPriority rObj 20.6",                       // jg4 regn.setpriority.gs.frac
        "SetFunctionValue (RoundAlt 37.66 2.7 1)",                  // sup1 roundalt.flag2
        "SetFunctionValue (NumberToWords 7.9)",                     // sup1 words.7p9
        "SetFunctionValue (NumberToWords -7.9)",
    ] {
        let mut kinds = runtime_errors(body);
        kinds.dedup();
        assert_eq!(kinds, ["fraction_for_int_parameter"], "{body}");
        // The editor's compile (the plugins' source) keeps taking it.
        assert_eq!(editor_errors(body), Vec::<&str>::new(), "{body}");
    }
}

#[test]
fn runtime_keeps_whole_numbers_floats_and_variables() {
    // [observed: L5.ext rerun 2026-11-10] these compiled in the original: the rows beside
    // the refused ones, and fractions for float parameters.
    for body in [
        "SetBaseKarma 37 rBase",                                    // jip.n7 karma.set.int
        "rBase.SetBaseKarma -12",                                   // jip.n7 karma.set.ref
        "SetNthDestructionStageTrait rObj 1 0 -5",                  // jip6 dest.settrait.1.0.-5
        "SetNthDestructionStageTrait rObj 1 1 fSave",               // jip6's put-back step
        "SetFunctionValue (sv_Construct \"%g\" (ApplyEasingAlt 1.5 1 0))", // showoff3 so3.ease.outside
        "SetFunctionValue (RandSeeded 0 1000000000)",               // showoff3 so3.seed.range.wide
        "SetFunctionValue (RoundAlt 37.66 1 1)",                    // sup1 roundalt.*
        "SetFunctionValue (NumberToWords -15)",                     // sup1 words.m15
    ] {
        assert_eq!(runtime_errors(body), Vec::<&str>::new(), "{body}");
    }
}

#[test]
fn runtime_refuses_an_untyped_result_for_a_form_parameter() {
    // [observed: nvse.misc pkg.nth0.location, nvse-seventh pkg.removeat0, ext-jg6.md's
    // GetCurrentFurnitureRef] refused under GetFormIDString in the original.
    for body in [
        "SetFunctionValue (GetFormIDString (GetPackageLocation rG))",
        "SetFunctionValue (GetFormIDString (rObj.RemovePackageAt 0))",
        "SetFunctionValue (GetFormIDString (rP.GetCurrentFurnitureRef))",
    ] {
        assert_eq!(runtime_errors(body), ["untyped_result_for_form"], "{body}");
        assert_eq!(editor_errors(body), Vec::<&str>::new(), "{body}");
    }
    // Through a variable it compiles there [observed: jg6 furniture.player].
    assert_eq!(runtime_errors("set rObj to rP.GetCurrentFurnitureRef\nSetFunctionValue (GetFormIDString rObj)"), Vec::<&str>::new());
    // A typed form getter under GetFormIDString compiles [observed: nvse.misc pkg.nth0].
    assert_eq!(runtime_errors("SetFunctionValue (GetFormIDString (rObj.GetNthPackage 0))"), Vec::<&str>::new());
}

/// The statements of a unit's first block, compiled as the engine compiles a mod's source
/// (`Seed::unknown_calls`), with the compile's diagnostics.
fn mod_source(src: &str) -> (Compiled, Program) {
    let t = table();
    let c = compile(src, &t, &World, Some(Seed { variables: &[], refs: &[], unknown_calls: true }));
    let p = decode(&c.bytes, &t).program;
    (c, p)
}

fn warnings<'a>(c: &'a Compiled, kind: &str) -> Vec<&'a str> {
    c.diags.iter().filter(|d| !d.error && d.kind == kind).map(|d| d.detail.as_str()).collect()
}

#[test]
fn an_eval_line_assigns_or_calls() {
    // [observed: JustAssortedMods.esp 4.6, `eval iTemp = iTemp * (1 - …)` and `eval
    // Quest.rRef.Activate PlayerRef 1`, shipped with compiled bytes.]
    let (c, p) = build("short a\nref r\nbegin GameMode\nlet a := 3\neval a = a * (1 + 1)\neval r.Activate player 1\nend");
    assert!(c.diags.iter().all(|d| d.kind != "assignment_to_a_value"), "{:?}", c.diags);
    let b = body(&p);
    assert!(matches!(&b[1], Stmt::Let { expr: fnv_script::ir::XExpr::Assign { .. } }), "{:?}", b[1]);
    assert!(matches!(&b[2], Stmt::Eval { expr: fnv_script::ir::XExpr::Call { .. } }), "{:?}", b[2]);
}

#[test]
fn the_let_macro_takes_a_division() {
    // [observed: JustAssortedMods.esp 4.6, `fAP /= GetNumericGameSetting …`.]
    let (_, p) = build("float f\nbegin GameMode\nf = 6\nf /= 3\nend");
    assert!(matches!(&body(&p)[1], Stmt::Let { expr: fnv_script::ir::XExpr::Assign { op: fnv_script::ir::XAssign::Div, .. } }), "{:?}", body(&p)[1]);
}

#[test]
fn a_lambdas_value_takes_an_assignment() {
    // `r = ({} => a = b = 1)`: the lambda assigns when it runs; it is not `(r = lambda)
    // = 1` [observed: JustAssortedMods.esp 4.6 JLMScript].
    let (c, p) = build("short a\nshort b\nref r\nbegin GameMode\nr = ({} => a = b = 1)\nend");
    assert!(c.diags.iter().all(|d| d.kind != "assignment_to_a_value"), "{:?}", c.diags);
    let Stmt::Let { expr: fnv_script::ir::XExpr::Assign { target, value, .. } } = &body(&p)[0] else { panic!("{:?}", body(&p)[0]) };
    assert!(matches!(**target, fnv_script::ir::XExpr::Var { .. }), "{target:?}");
    let fnv_script::ir::XExpr::Lambda { value: Some(v), .. } = &**value else { panic!("{value:?}") };
    assert!(matches!(**v, fnv_script::ir::XExpr::Assign { .. }), "{v:?}");
}

#[test]
fn the_jump_groups_have_their_codes() {
    // [observed: JustAssortedMods.esp 4.6 JVSAnimEffectScript's compiled bytes, 227 and 228.]
    let c = compile("begin GameMode\nif IsAnimPlaying JumpStart == 0\nelseif IsAnimPlaying JumpLoop\nendif\nend", &table(), &World, None);
    assert_eq!(c.errors().count(), 0, "{:?}", c.diags);
    let at = |code: u16| c.bytes.windows(8).any(|w| w[..2] == 0x1128u16.to_le_bytes() && w[6..8] == code.to_le_bytes());
    assert!(at(227) && at(228), "{:02X?}", c.bytes);
}

#[test]
fn a_mods_source_survives_functions_nothing_knows() {
    // Every place a call can stand [observed: JustAssortedMods.esp 4.6 calls
    // DisableScriptedActivate, GetContainerOpenSound and ySIGetTagTrait, none with a page,
    // on a quest's ref variable, in parentheses, after an element and in a let].
    let src = "short a\nref r\narray_var ar\nbegin GameMode\n\
        NoSuchFn 1\n\
        let a := NoSuchFn 2\n\
        a = r.NoSuchFn 3\n\
        eval TestQuest.fTimer.NoSuchFn 4\n\
        a = ar[0].NoSuchFn \"x\"\n\
        if NoSuchFn 5\nendif\n\
        if r.NoSuchFn\nendif\n\
        set a to r.NoSuchFn 6\n\
        PlaySound (TestQuest.fTimer.NoSuchFn)\n\
        end";
    let (c, _) = mod_source(src);
    assert_eq!(c.errors().count(), 0, "{:?}", c.errors().collect::<Vec<_>>());
    assert!(warnings(&c, "unknown_function").len() >= 9, "{:?}", c.diags);
    // The editor-parity compile keeps the errors.
    assert!(compile(src, &table(), &World, None).errors().count() > 0);
    // A unit whose only extender use is one unknown function on a call line.
    let (c, _) = mod_source("ref r\nbegin GameMode\nr.NoSuchFn 1\nNoSuchFn\nend");
    assert_eq!(c.errors().count(), 0, "{:?}", c.errors().collect::<Vec<_>>());
    // What does resolve is left as it was: a remote variable, a local, a form.
    let (c, _) = mod_source("short a\nbegin GameMode\nset a to TestQuest.bDone\nif a == TestGlobal\nendif\nend");
    assert_eq!(c.diags.iter().filter(|d| d.kind == "unknown_function").count(), 0, "{:?}", c.diags);
}

#[test]
fn a_mods_source_survives_an_enum_name_the_table_lacks() {
    let src = "begin GameMode\nif IsAnimPlaying NoSuchGroup\nendif\nend";
    let (c, _) = mod_source(src);
    assert_eq!(c.errors().count(), 0, "{:?}", c.errors().collect::<Vec<_>>());
    assert_eq!(warnings(&c, "unknown_enum_name"), ["anim_group: NoSuchGroup"]);
    // The code nothing has, not group 0 (Idle).
    assert!(c.bytes.windows(8).any(|w| w[..2] == 0x1128u16.to_le_bytes() && w[6..8] == u16::MAX.to_le_bytes()), "{:02X?}", c.bytes);
    assert_eq!(compile(src, &table(), &World, None).errors().map(|d| d.kind).collect::<Vec<_>>(), ["unknown_enum_name"]);
}

/// How many arguments the call named `name` got, anywhere in the unit's first block.
fn call_args(p: &Program, name: &str) -> Option<usize> {
    let json = serde_json::to_string(&p.blocks[0].body).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&json).unwrap();
    fn find(v: &serde_json::Value, name: &str) -> Option<usize> {
        match v {
            serde_json::Value::Object(m) => {
                let named = m.get("func").is_some_and(|f| f.to_string().to_ascii_lowercase().contains(&name.to_ascii_lowercase()));
                match (named, m.get("args").and_then(|a| a.as_array())) {
                    (true, Some(a)) => Some(a.len()),
                    _ => m.values().find_map(|x| find(x, name)),
                }
            }
            serde_json::Value::Array(a) => a.iter().find_map(|x| find(x, name)),
            _ => None,
        }
    }
    find(&doc, name)
}

#[test]
fn a_foreach_head_may_stand_in_parentheses() {
    // [observed: Unofficial Patch NVSE Plus and JSawyer Ultimate ship `foreach (item <-
    // source)` lines with compiled bytes, the source a variable, a call and an ar_List.]
    let (_, p) = build("array_var a\narray_var e\nref r\nbegin GameMode\nforeach (e <- a)\nloop\nforeach (e <- ar_List 1, 2, 3)\nloop\nforeach (e <- GetListForms r)\nloop\nforeach e <- (a)\nloop\nend");
    let b = body(&p);
    assert_eq!(b.len(), 4, "{b:?}");
    assert!(b.iter().all(|s| matches!(s, Stmt::ForEach { head, .. } if matches!(head.var, fnv_script::ir::XExpr::Var { .. }))), "{b:?}");
    assert_eq!(call_args(&p, "ar_List"), Some(3));
    // Only one pair around all of it is the head's.
    use fnv_script::compile::lex::{unwrapped, xtokens};
    assert_eq!(unwrapped(&xtokens("(e <- a)")).len(), 3);
    assert_eq!(unwrapped(&xtokens("(e) <- (a)")).len(), 7);
    assert_eq!(unwrapped(&xtokens("((e <- a))")).len(), 5);
}

#[test]
fn a_comma_may_follow_a_functions_name() {
    // [observed: Unofficial Patch NVSE Plus ships `rForm = ListGetNthForm, <list>, iPos`
    // with compiled bytes.]
    for line in ["r = ListGetNthForm, Caps001, i", "let r := ListGetNthForm, Caps001, i", "set r to ListGetNthForm, Caps001, i"] {
        let (_, p) = build(&format!("ref r\nshort i\nbegin GameMode\n{line}\nend"));
        assert_eq!(call_args(&p, "ListGetNthForm"), Some(2), "{line}");
    }
    // After a function that can go without its first argument, or takes none, the comma
    // is the list's: two elements each.
    for first in ["GetActiveFactions", "GetSelf"] {
        let (_, p) = build(&format!("array_var a\nbegin GameMode\nlet a := ar_List {first}, 2\nend"));
        assert_eq!(call_args(&p, "ar_List"), Some(2), "{first}");
    }
    let (_, p) = build("array_var a\nbegin GameMode\nlet a := ar_List GetActiveFactions, 2\nend");
    assert_eq!(call_args(&p, "GetActiveFactions"), Some(0));
}

#[test]
fn getfactionrank_takes_its_faction_in_an_extender_expression() {
    // The catalogue's row has no parameter list; its condition takes a faction
    // [documented: the xEdit definitions' `ptFaction`]. [observed: JIP Companions Command
    // & Control 2.60, `elseif (… == 0) || (rRef.GetFactionRank <faction> > 9)`.]
    assert_eq!(table().get(0x1049).map(|f| (f.name.as_str(), f.encodings.len())), Some(("GetFactionRank", 1)));
    let (_, p) = build("ref r\nshort a\nbegin GameMode\nlet a := (a == 0) || (r.GetFactionRank TestQuest > 9)\nend");
    assert_eq!(call_args(&p, "4169").or(call_args(&p, "GetFactionRank")), Some(1), "{:?}", body(&p));
}

#[test]
fn an_unknown_name_before_an_operand_in_parentheses_is_a_call() {
    // [observed: JIP Companions Command & Control 2.60, `SetFunctionValue (CCCInFaction
    // <faction> == 0)`: a function with no page, as an argument in parentheses.]
    let src = "begin Function { }\nSetFunctionValue (NoSuchFn TestQuest == 0)\nend";
    let (c, p) = mod_source(src);
    assert_eq!(c.errors().count(), 0, "{:?}", c.errors().collect::<Vec<_>>());
    assert_eq!(warnings(&c, "unknown_function"), ["NoSuchFn"]);
    assert_eq!(call_args(&p, "NoSuchFn"), Some(1));
    // Alone in parentheses it stays its own text, as in the original [observed:
    // nvse-fourth, `SetFunctionValue (GetFOSEVersion)`].
    let (c, _) = mod_source("begin Function { }\nSetFunctionValue (NoSuchFn)\nend");
    assert_eq!(warnings(&c, "unknown_function"), Vec::<&str>::new());
    assert_eq!(warnings(&c, "unquoted_argument"), ["NoSuchFn"]);
    // The editor-parity compile keeps its error.
    assert!(compile(src, &table(), &World, None).errors().count() > 0);
}

#[test]
fn names_read_from_mods_compiled_bytes_have_their_codes() {
    // [observed: the compiled scripts of Project Nevada 2.5 and Unofficial Patch NVSE Plus
    // 1.7.3 beside their source: PlayGroup Attack4 44, Attack4IS 47, Attack5 50, Attack7
    // 62, JamB 201, Stagger 223; GetIsObjectType Container 27, stored as an animation
    // group is (a count, then the code).]
    let t = table();
    for (group, code) in [("Attack4", 44u16), ("Attack4IS", 47), ("Attack5", 50), ("Attack7", 62), ("JamB", 201), ("Stagger", 223)] {
        let c = compile(&format!("begin GameMode\nPlayGroup {group} 1\nend"), &t, &World, None);
        assert_eq!(c.errors().count(), 0, "{group}: {:?}", c.diags);
        assert!(c.bytes.windows(8).any(|w| w[..2] == 0x1013u16.to_le_bytes() && w[6..8] == code.to_le_bytes()), "{group}: {:02X?}", c.bytes);
    }
    let c = compile("ref r\nshort a\nbegin GameMode\nif r.GetIsObjectType Container == 1 && a\nendif\nend", &t, &World, None);
    assert_eq!(c.errors().count(), 0, "{:?}", c.diags);
    assert!(c.bytes.windows(8).any(|w| w == [0xB1, 0x11, 0x04, 0x00, 0x01, 0x00, 0x1B, 0x00]), "{:02X?}", c.bytes);
    // A form type not read yet: an error in the editor's compile, a warning in a mod's.
    let src = "ref r\nbegin GameMode\nif r.GetIsObjectType NoSuchType\nendif\nend";
    assert_eq!(compile(src, &t, &World, None).errors().map(|d| d.kind).collect::<Vec<_>>(), ["unknown_enum_name"]);
    let (c, _) = mod_source(src);
    assert_eq!(c.errors().count(), 0, "{:?}", c.diags);
    assert_eq!(warnings(&c, "unknown_enum_name"), ["form_type: NoSuchType"]);
}

#[test]
fn a_mods_variable_may_bear_an_extender_functions_name() {
    // [observed: Project Nevada - Core.esm 2.5, a variable `break` in a stage's result
    // script, shipped with compiled bytes.] The editor's compile refuses the name.
    let src = "short break\nbegin GameMode\nset break to 1\nif break\nendif\nend";
    let (c, _) = mod_source(src);
    assert_eq!(c.errors().count(), 0, "{:?}", c.errors().collect::<Vec<_>>());
    assert_eq!(warnings(&c, "variable_named_like_a_function"), ["break"]);
    assert!(compile(src, &table(), &World, None).errors().any(|d| d.kind == "variable_named_like_a_function"));
}
// ---------------------------------------------------------------- mods of the Blue Moon list
// What units of the owner's 211-plugin list (#2275) failed to compile from their source,
// though each ships compiled bytes the GECK with xNVSE wrote from it. Each case is the
// line's shape, not the mod's text.

fn mod_ok(src: &str) -> (Compiled, Program) {
    let (c, p) = mod_source(src);
    assert_eq!(c.errors().count(), 0, "{:?}", c.errors().collect::<Vec<_>>());
    (c, p)
}

#[test]
fn a_bare_colon_equals_line_is_a_let() {
    let (_, p) = mod_ok("short d\nbegin GameMode\nd := 1\nTestQuest.bDone := 1\nend");
    assert!(body(&p).iter().all(|s| matches!(s, Stmt::Let { .. })), "{:?}", body(&p));
    assert_eq!(body(&p).len(), 2);
}

#[test]
fn a_vanilla_functions_argument_in_parentheses_is_an_inline_expression_in_mod_source() {
    let src = "begin GameMode\nPlayer.RestoreAv Health (player.getav Health)\nPlayer.RestoreAv Health ((player.getav Health) * 2)\nend";
    let (_, p) = mod_ok(src);
    assert!(body(&p).iter().all(|s| matches!(s, Stmt::Eval { .. })), "{:?}", body(&p));
    assert!(compile(src, &table(), &World, None).errors().count() > 0, "the editor-parity compile reads it as vanilla");
}

#[test]
fn set_event_handler_alt_takes_its_filters_and_a_priority() {
    let (_, p) = mod_ok("begin GameMode\nSetEventHandlerAlt \"ShowOff:OnPreActivate\" TestQuest 0::TestDoorREF 1::PlayerREF \"priority\"::3\nend");
    assert_eq!(call_args(&p, "SetEventHandlerAlt"), Some(5));
}

#[test]
fn spellings_the_original_compiles() {
    // `begin function{} =>`, `Let r : = …`, `This`, a compound assignment declaring.
    mod_ok("short b\nbegin GameMode\nCallAfter 2.1 (begin function{} =>\n  set b to 1\nend)\nend");
    let (_, p) = mod_ok("ref r\nbegin GameMode\nLet r : = (GetSelf)\nlet r := This\nend");
    assert!(body(&p).iter().all(|s| matches!(s, Stmt::Let { .. })));
    let (c, _) = mod_ok("float fMin\nfloat fMax\nbegin GameMode\nfloat fChance += rand fMin, fMax\nend");
    assert_eq!(c.variables.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["fMin", "fMax", "fChance"]);
}

#[test]
fn alignment_names_have_their_codes() {
    // [observed: Karmic Balance.esp and Vegas Elysium.esp, GetIsAlignment (0x11DA).]
    let t = table();
    for (name, code) in [("Good", 0u16), ("Neutral", 1), ("Evil", 2), ("VeryGood", 3), ("VeryEvil", 4)] {
        let c = compile(&format!("begin GameMode\nif player.GetIsAlignment {name}\nendif\nend"), &t, &World, None);
        assert_eq!(c.errors().count(), 0, "{name}: {:?}", c.diags);
        assert!(c.bytes.windows(8).any(|w| w[..2] == 0x11DAu16.to_le_bytes() && w[6..8] == code.to_le_bytes()), "{name}: {:02X?}", c.bytes);
    }
}

#[test]
fn what_the_original_drops_from_a_line() {
    // `say Topic:Ugh! || …` says the topic; `DebugPrint "t= " + $f` prints the string.
    let (c, _) = mod_ok("begin GameMode\nplayer.say TestQuest:Ugh! || player.say TestQuest:Oof!\nend");
    assert_eq!(warnings(&c, "rest_of_line_dropped").len(), 1);
    let (c, p) = mod_ok("float f\nbegin GameMode\nDebugPrint \"t= \" + $f\nend");
    assert_eq!(warnings(&c, "rest_of_line_dropped").len(), 1);
    assert_eq!(call_args(&p, "DebugPrint"), Some(1));
    // In parentheses: after a call with all its arguments, or from a comma on.
    let (c, p) = mod_ok("ref q\nshort r\nbegin GameMode\nr = (ar_size (GetActiveObjectives q)) < 0\nend");
    assert_eq!(warnings(&c, "rest_of_parentheses_dropped"), ["q"]);
    assert_eq!(call_args(&p, "GetActiveObjectives"), Some(0));
    let (c, _) = mod_ok("short i\nbegin GameMode\nSetFormDescription TestQuest ((GetFormDescription TestQuest), i)\nend");
    assert_eq!(warnings(&c, "rest_of_parentheses_dropped"), [", i"]);
}

#[test]
fn a_short_form_id_names_its_form_in_mod_source() {
    // [observed: ABM- Patched Perks.esp, `player.AddItem f iAmount` adds the caps.]
    let src = "short i\nbegin GameMode\nplayer.AddItem f i\nend";
    let (c, _) = mod_ok(src);
    assert!(c.refs.contains(&RefSlot::Form(CAPS)), "{:?}", c.refs);
    assert!(compile(src, &table(), &World, None).errors().count() > 0);
}

#[test]
fn a_short_form_id_names_its_form_in_an_extender_functions_form_parameter() {
    // [observed 2026-11-14, nvse-cmd-probes in the stock game, xNVSE's runtime compiler:
    // `AuxVarSetFlt "*_q" 7 0 f` reads back under Caps001 (0xF); `g` and `FFFFFF` (no
    // form) fail the compile; shipped in Armor Damage Fix's gr_CondDamFix.txt.]
    let (c, _) = mod_ok("begin GameMode
AuxVarSetFlt \"*_q\" 7 0 f
end");
    assert!(c.refs.contains(&RefSlot::Form(CAPS)), "{:?}", c.refs);
    let (c, _) = mod_ok("begin GameMode
AuxVarSetFlt \"*_q\" 7 0 14
end");
    assert!(c.refs.contains(&RefSlot::Form(fnv_script::compile::PLAYER)), "14 is the player's reference: {:?}", c.refs);
    // The value and the index are no forms, whatever they look like.
    let (c, _) = mod_ok("begin GameMode
AuxVarSetFlt \"*_q\" 7 0
end");
    assert!(c.refs.is_empty(), "{:?}", c.refs);
}

#[test]
fn pc1st_in_parentheses_is_the_call_not_text() {
    // [observed: ln_B42Core.txt, `Goo1.AuxVarSetFlt "*_CrPOV" (PC1st)`; Enhanced FOV.esp's
    // bytes call IsPC1stPerson for `if PC1st`.]
    let (c, _) = mod_ok("begin GameMode
AuxVarSetFlt \"*_q\" (PC1st)
end");
    assert!(warnings(&c, "unquoted_argument").is_empty(), "{:?}", c.diags);
}

#[test]
fn set_string_setting_is_lus_function_not_the_console_command() {
    let (_, p) = mod_ok("begin GameMode\nSetStringSetting \"sTargetTypeOpen\" \"Open\"\nend");
    assert_eq!(call_args(&p, "SetStringSetting"), Some(2));
}

#[test]
fn an_arrow_lambda_may_span_lines() {
    // [observed: FreePostMojave.esm, `CallWhen ({} =>` … `) ({} => MenuMode == 0)`.]
    let (_, p) = mod_ok("string_var s\nbegin GameMode\nCallWhen ({} =>\n  MessageBoxExAlt TestQuest (s)\n  player.RewardXP 250\n) ({} => MenuMode == 0)\nprint \"done\"\nend");
    assert_eq!(body(&p).len(), 2, "{:?}", body(&p));
    assert_eq!(call_args(&p, "CallWhen"), Some(2));
}

#[test]
fn an_if_with_open_parentheses_takes_the_lines_that_continue_it() {
    // [observed: NpcCombatChem.esp, `If (( … )` and `|| ( … )` lines below it.]
    let (_, p) = mod_ok("short a\nshort b\nbegin GameMode\nIf (( a == 4 && b < 1 )\n || ( a == 3 && b < 2 ) )\n set a to 1\nendif\nend");
    let Stmt::If { arms, .. } = &body(&p)[0] else { panic!("{:?}", body(&p)) };
    assert_eq!(ops(&arms[0].cond), ["var", "4", "==", "var", "1", "<", "&&", "var", "3", "==", "var", "2", "<", "&&", "||"]);
}

#[test]
fn a_mods_runtime_file_calls_a_function_nothing_knows() {
    // A Script Runner file naming a plugin function with no page compiles; the console's
    // line keeps the error.
    // (EnableImprovedRecipeMenu and RefreshAnimData, the names first used here, have
    // catalogue rows since #2318/#2326: names no plugin registers stand in.)
    let src = probe("NvfUnregisteredCommandA 1\nplayer.NvfUnregisteredCommandB");
    let c = fnv_script::compile::compile_mod_runtime(&src, &table(), &World);
    assert_eq!(c.errors().count(), 0, "{:?}", c.errors().collect::<Vec<_>>());
    assert!(!runtime_errors("NvfUnregisteredCommandA 1").is_empty());
}

#[test]
fn xnvses_own_int_parameter_keeps_a_fraction_at_run_time() {
    assert_eq!(runtime_errors("SetWeaponFireRate 7.5"), Vec::<&str>::new());
}

#[test]
fn spellings_the_extender_registers_are_names_too() {
    // [observed: the Blue Moon install's nvse.log, `ReadFromJSONFile (ReadFromJSON)`.]
    let t = table();
    assert_eq!(t.unnumbered.get("readfromjsonfile").map(|u| u.name.as_str()), Some("ReadFromJSON"));
    assert!(t.unnumbered.contains_key("getedid") || t.by_name("getedid").is_some());
    // [observed: TTW No Hardcore Sacrifice.esp's bytes call MoveTo for `MoveToMarker`.]
    assert_eq!(t.by_name("movetomarker"), t.by_name("moveto"));
    assert_eq!(t.by_name("pc1st"), t.by_name("ispc1stperson"), "[observed: Enhanced FOV.esp's bytes]");
    assert!(t.by_name("moveto").is_some());
}

#[test]
fn a_quoted_form_id_before_a_dot_is_the_reference() {
    // [observed: Veronica Outfit Replacer.esp ships `If "E32A9".GetItemCount X == 0` and
    // `"E32A9".AddItemAlt X 1 100 1` with compiled bytes calling both on 0x000E32A9.]
    let src = "short i
begin GameMode
If \"1003000\".GetItemCount Caps001 == 0
\"1003000\".AddItem Caps001 1
endif
end";
    let (c, _) = mod_ok(src);
    assert!(c.refs.contains(&RefSlot::Form(DOOR)), "{:?}", c.refs);
    // A quoted Editor ID before a dot names its form too.
    let (c, _) = mod_ok("begin GameMode
\"TestDoorREF\".Disable
end");
    assert_eq!(c.refs, [RefSlot::Form(DOOR)]);
}

#[test]
fn set_after_a_reference_sets_the_scripts_own_variable() {
    // [observed: Fallout3.esm and RAD.esp, `MS14ArmorVoiceRef.Set SayTimer to
    // GameDaysPassed + 0.1` compiles to `1C 00 <ref> 15 00 …`, a set of the local SayTimer
    // with the reference as its prefix.]
    let src = "float SayTimer
begin GameMode
TestDoorREF.Set SayTimer to 2 + 0.1
end";
    let (c, p) = build(src);
    assert_eq!(c.refs, [RefSlot::Form(DOOR)]);
    let set = [0x1Cu8, 0x00, 0x01, 0x00, 0x15, 0x00];
    assert!(c.bytes.windows(set.len()).any(|w| w == set), "{:02X?}", c.bytes);
    let (_, q) = build("float SayTimer
begin GameMode
Set SayTimer to 2 + 0.1
end");
    assert_eq!(body(&p), body(&q), "decoded as the plain set");
}

// ---------------------------------------------------------------- mods of the High and Dry list
// The same for the owner's 232-plugin High and Dry list (#2275).

#[test]
fn a_box_after_a_comma_is_an_argument() {
    // [observed: Famine.esp, `Call Fn, &rRef, 0, 0, 0, 0`: five arguments, the first boxed.]
    let (_, p) = mod_ok("ref r
begin GameMode
Call TestQuest, &r, 0, 0
end");
    assert_eq!(call_args(&p, "Call"), Some(4));
    let json = serde_json::to_string(body(&p)).unwrap();
    assert!(json.contains("\"box\""), "{json}");
}

#[test]
fn braces_around_an_argument_are_the_argument() {
    // [observed: Animated Ingestibles.esp, `call Fn {(BuildRef 0 n)}` and `call Fn {Q.v}`.]
    let (_, p) = mod_ok("begin GameMode
call TestQuest {(GetSelf)}
call TestQuest {TestQuest.bDone}
end");
    assert_eq!(body(&p).len(), 2);
    assert_eq!(call_args(&p, "Call"), Some(2));
}

#[test]
fn an_operand_meeting_a_whole_expression_in_parentheses_is_dropped() {
    // [observed: MrShersh Functional Backpack.esp, `("…" + $sv ".nif")` ends at the variable.]
    let (c, _) = mod_ok("string_var s
begin GameMode
AttachModel Player 2 (\"a\" + $s \".nif\")
end");
    assert_eq!(warnings(&c, "rest_of_parentheses_dropped"), [".nif"]);
}

#[test]
fn get_refs_in_cell_takes_six_arguments() {
    // [observed: JAM Misc Tweaks.esp, `GetRefsInCell (Player.GPC) 28 2 0 2000 45`.]
    let (_, p) = mod_ok("array_var a
begin GameMode
a = Player.GetRefsInCell (Player.GPC) 28 2 0 2000 45
end");
    assert_eq!(call_args(&p, "GetRefsInCell"), Some(6));
}

#[test]
fn a_set_lines_parentheses_do_not_end_a_call() {
    // [observed: Animated Ingestibles.esp, `set t to 1 + (getVariable ("b" + $i)) Quest`:
    // GetVariable takes the quest after the `)`.]
    let (_, p) = mod_ok("short i
short t
begin GameMode
set t to 1 + (GetVariable (\"bDo\" + $i)) TestQuest
end");
    assert_eq!(call_args(&p, "GetVariable"), Some(2));
}

#[test]
fn an_nvsescript_unit_compiles_through_the_first_grammar() {
    // [observed: WeaponModCraftInfo.esp's four units, `name X;` and `fn (int i) { … }`,
    // ship with bytes the extender's second compiler wrote; documented: NVSEScript.]
    let src = "name Handler;

fn (int iMenuId) {
    TestQuest.bDone = iMenuId;
    if (!TestQuest.bDone) {
        return;
    }
    Print(\"menu ${iMenuId}\");
}
";
    let (c, p) = mod_ok(src);
    assert_eq!(c.variables.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["iMenuId"]);
    assert_eq!(p.blocks.len(), 1);
    // A diagnostic names the source's line.
    let (c, _) = mod_source("name Bad;

fn () {
    NoSuchVar.x = 1;
}
");
    assert_eq!(c.errors().next().map(|d| d.line), Some(4), "{:?}", c.diags);
}

/// Inside an expression an NVSE-aware function's argument stops at a comparison, which
/// applies to the call's result [observed: Party Banter 2.esm ships `if eval Ar_HasKey
/// aConvo, "Conditions" == 1` with bytes that compare the call with 1, #2275]; a range or
/// a pair still continues it.
#[test]
fn a_comparison_after_an_aware_calls_argument_compares_its_result() {
    let h = run("array_var a\nint r\nint s\nbegin GameMode\nlet a := ar_Map \"x\"::1\nif eval ar_HasKey a \"minMCMVersion\" == 0\nlet r := 5\nendif\nif eval ar_HasKey a \"x\" == 1 && ar_Size a == 1\nlet s := 7\nendif\nend");
    assert_eq!(h.0[&2], Value::Num(5.0));
    assert_eq!(h.0[&3], Value::Num(7.0));
}

/// An assignment to a string variable in the place of the variable an Sv_ function edits:
/// the assignment runs and the function edits that variable (AguaFria.esm's
/// `sv_Replace " (GRA)|" (sText = $rForm) 0 99 0 1`, #2275).
#[test]
fn sv_replace_edits_the_variable_an_assignment_in_its_place_sets() {
    let h = run("string_var s\nstring_var t\nfloat n\nbegin GameMode\nlet t := \"Rifle (GRA)\"\nlet n := Sv_Replace \" (GRA)|\" (s = t)\nend");
    assert_eq!(h.text(1), "Rifle");
    assert_eq!(h.0[&3], Value::Num(1.0));
}

#[test]
fn typeof_absent_key_is_null_without_creating_it() {
    let h = run("array_var a\nstring_var missing\nstring_var present\nint count\nbegin GameMode\nlet a := ar_Map \"present\"::7\nlet missing := TypeOf a[\"absent\"]\nlet present := TypeOf a[\"present\"]\nlet count := ar_Size a\nend");
    assert_eq!(h.text(2), "NULL");
    assert_eq!(h.text(3), "Number");
    assert_eq!(h.0[&4], Value::Num(1.0));
}

#[test]
fn typeof_does_not_hide_invalid_indexing_or_direct_missing_access() {
    for expression in ["a[\"absent\"]", "TypeOf a[\"present\"][\"absent\"]", "TypeOf a[a]"] {
        let (c, p) = build(&format!("array_var a\nstring_var result\nbegin GameMode\nlet a := ar_Map \"present\"::7\nlet result := {expression}\nend"));
        let mut host = host_for(&c);
        let error = Vm::new(&table(), &mut host, RunOpts { strict: true, ..Default::default() }).run(&p.blocks[0].body);
        assert!(error.is_err(), "{expression}: {error:?}");
    }
}

/// `*` with a space before and a name right after unboxes the argument
/// (`Ar_Size *aKeybinds`, keybinder's ConstructModAddedJson.gek); `a * 2` still multiplies.
#[test]
fn a_spaced_star_before_a_name_is_the_unbox_prefix() {
    let h = run("array_var a\narray_var b\nint n\nint m\nbegin GameMode\nlet a := ar_List 1, 2, 3\nlet b := &a\nif eval (1 + (Ar_Size *b) * 2) == 7\nlet n := 5\nendif\nlet m := (Ar_Size a) * 2\nend");
    assert_eq!(h.0[&3], Value::Num(5.0));
    assert_eq!(h.0[&4], Value::Num(6.0));
}

/// A quoted hex FormID as the calling reference is that form [observed: AguaFria.esm
/// ships `"171B39".AddItemAlt …` with bytes that call on 0x00171B39, #2275].
#[test]
fn a_quoted_hex_formid_calls_on_that_form() {
    let (c, _) = build("begin GameMode\neval \"1003000\".Activate player\nend");
    assert!(c.refs.contains(&RefSlot::Form(DOOR)), "{:?}", c.refs);
}

/// A declaration whose value declares inline (`int n = Ar_Size (array_var b = …)`)
/// declares both, in a lambda's body too, so that later lines there assign the inline one
/// [observed: B42 Interact's ln_B42Interact.txt, `xkR = ar_null` in an event handler's
/// lambda, runs in the original].
#[test]
fn a_declarations_value_declares_inline_in_a_lambda() {
    for body in [
        "int n = ar_size (array_var b = ar_list 1 2)\nb = ar_null",
        "SetEventHandlerAlt \"x\" (begin Function {int k}\nint n = ar_size (array_var b = ar_list 1 2)\nb = ar_null\nend)",
    ] {
        let src = format!("Begin Function {{ }}\n{body}\nEnd\n");
        let c = fnv_script::compile::compile_mod_runtime(&src, &table(), &World);
        assert_eq!(c.errors().count(), 0, "{body}: {:?}", c.errors().collect::<Vec<_>>());
    }
}

/// `SetOnPlayGroupEventHandler` parameter index 3 resolves bare known `anim_group` keywords
/// to `XExpr::Enum`, while leaving quoted strings (including names outside `encodings.json`),
/// runtime `string_var`s, numeric codes, globals, dotted quest variables, and function calls
/// as ordinary `ParamEnc::Value` expressions.
#[test]
fn set_on_play_group_event_handler_anim_group_arg_forms() {
    let (_, p) = build(
        "ref fnH\nstring_var sGrp\nbegin GameMode\n\
         SetOnPlayGroupEventHandler fnH 1 PlayerREF Idle\n\
         SetOnPlayGroupEventHandler fnH 1 PlayerREF Equip\n\
         SetOnPlayGroupEventHandler fnH 1 PlayerREF \"Equip\"\n\
         SetOnPlayGroupEventHandler fnH 1 PlayerREF \"TurnRight\"\n\
         SetOnPlayGroupEventHandler fnH 1 PlayerREF sGrp\n\
         SetOnPlayGroupEventHandler fnH 1 PlayerREF 0\n\
         SetOnPlayGroupEventHandler fnH 1 PlayerREF 24\n\
         SetOnPlayGroupEventHandler fnH 1 PlayerREF TestGlobal\n\
         SetOnPlayGroupEventHandler fnH 1 PlayerREF TestQuest.bDone\n\
         SetOnPlayGroupEventHandler fnH 1 PlayerREF GetAnimAction\n\
         end\n",
    );
    let fourth_args: Vec<String> = body(&p)
        .iter()
        .map(|s| match s {
            Stmt::Eval { expr } => match expr {
                XExpr::Call { call } => x(&call.args[3]),
                other => panic!("expected call, got {other:?}"),
            },
            other => panic!("expected eval stmt, got {other:?}"),
        })
        .collect();
    assert_eq!(fourth_args[0], "#0", "bare keyword Idle");
    assert_eq!(fourth_args[1], "#24", "bare keyword Equip");
    assert_eq!(fourth_args[2], "\"Equip\"", "quoted string must stay XExpr::Str for runtime normalization");
    assert_eq!(fourth_args[3], "\"TurnRight\"", "unknown quoted string must compile as XExpr::Str");
    assert_eq!(fourth_args[4], "v2", "string_var stays a variable reference");
    assert_eq!(fourth_args[5], "0", "numeric 0");
    assert_eq!(fourth_args[6], "24", "numeric 24");
    assert!(fourth_args[7].starts_with('g'), "global variable must stay global var, got {}", fourth_args[7]);
    assert_eq!(fourth_args[8], "v1", "dotted quest variable must compile as variable, not unknown enum");
    assert!(fourth_args[9].contains('('), "zero-arg function call must compile as call, got {}", fourth_args[9]);
}

/// Interactive console sessions compile lines against persistent local variables:
/// 1-based indices and `ref`-ness carry across lines, no spurious `variable_added`
/// warning is emitted, and re-declaring a `ref` as another type clears its `ref`-ness.
#[test]
fn compile_runtime_with_locals_preserves_slots_and_ref_vars() {
    use std::collections::HashSet;
    let t = table();
    let (c1, refs1) = fnv_script::compile::compile_runtime_with_locals("ref r = player\nint n = 5", &t, &World, &[], &HashSet::new());
    assert_eq!(c1.errors().count(), 0, "{:?}", c1.diags);
    assert!(c1.diags.iter().all(|d| d.kind != "variable_added"), "{:?}", c1.diags);
    assert!(refs1.contains("r"));
    let r_idx = c1.variables.iter().find(|v| v.name == "r").unwrap().index;
    let n_idx = c1.variables.iter().find(|v| v.name == "n").unwrap().index;

    let (c2, refs2) = fnv_script::compile::compile_runtime_with_locals("eval (r.GetActorValue Health + n)", &t, &World, &c1.variables, &refs1);
    assert_eq!(c2.errors().count(), 0, "{:?}", c2.diags);
    assert_eq!(c2.variables.iter().find(|v| v.name == "r").unwrap().index, r_idx);
    assert_eq!(c2.variables.iter().find(|v| v.name == "n").unwrap().index, n_idx);
    assert!(refs2.contains("r"));

    let (c3, refs3) = fnv_script::compile::compile_runtime_with_locals("float r = 1.5", &t, &World, &c2.variables, &refs2);
    assert_eq!(c3.errors().count(), 0, "{:?}", c3.diags);
    assert_eq!(c3.variables.iter().find(|v| v.name == "r").unwrap().index, r_idx);
    assert!(!refs3.contains("r"), "re-declaring ref as float clears ref-ness");
}

/// Compound bitwise and modulo assignments work in `let`, bare Let-macro, inline
/// declaration, quest-variable, array-element, and NVSEScript forms (including float
/// truncation and nested calls shifted before bitwise-OR: `iINITemp |= (Floor (Clamp ...)) << 1`),
/// while `^=` works in `let` only and `<<=`/`>>=` do not exist in xNVSE 6.4.9.
#[test]
fn compound_bitwise_assignments_let_and_macro_match_xnvse() {
    let h = run(
        "int a
int b
int c
int d
int e
int f
int g
int iINITemp
float fIn
float fTrunc
array_var ar
begin GameMode
a = 1
a |= 4
let a |= 8
b = 15
b &= 6
let b &= 7
c = 3
let c ^= 4
d = 3 << 2
e = 20 >> 2
f = 13
f %= 5
let f %= 2
int h |= 5
ar = ar_List 1, 2
ar[0] |= 4
ar[1] &= 3
g = ar[0] + ar[1]
fIn = 1.7
iINITemp = 1
iINITemp |= (Floor (GetMinOf (GetMaxOf fIn 0) 1)) << 1
fTrunc = 5.9
let fTrunc |= 2.8
end",
    );
    assert_eq!(h.0[&1], Value::Num(13.0), "a: 1 | 4 | 8 = 13");
    assert_eq!(h.0[&2], Value::Num(6.0), "b: 15 & 6 & 7 = 6");
    assert_eq!(h.0[&3], Value::Num(81.0), "c: 3 ^ 4 = 81 [documented: NVSE ^ is power; observed]");
    assert_eq!(h.0[&4], Value::Num(12.0), "d: binary 3 << 2 = 12 [observed]");
    assert_eq!(h.0[&5], Value::Num(5.0), "e: binary 20 >> 2 = 5 [observed]");
    assert_eq!(h.0[&6], Value::Num(1.0), "f: (13 % 5) % 2 = 1 [observed]");
    assert_eq!(h.0[&7], Value::Num(7.0), "g: (1 | 4) + (2 & 3) = 7");
    assert_eq!(h.0[&8], Value::Num(3.0), "iINITemp: 1 | (Floor (GetMinOf (GetMaxOf 1.7 0) 1) << 1) = 3");
    assert_eq!(h.0[&10], Value::Num(7.0), "fTrunc: 5.9 |= 2.8 -> trunc(5.9) | trunc(2.8) = 7 [observed]");
    assert_eq!(h.0[&12], Value::Num(5.0), "inline decl int h |= 5");

    // Dotted quest variable and mod-runtime nested Clamp/Floor/GetINIFloat_Cached pattern.
    let src = "int iINITemp\nbegin GameMode\n\
        TestQuest.bDone |= 2\n\
        TestQuest.bDone &= 3\n\
        TestQuest.bDone %= 5\n\
        let TestQuest.bDone ^= 2\n\
        iINITemp |= (Floor (Clamp (GetINIFloat_Cached \"Main:bVisible\" \"Widget.ini\" 1) 0 1)) << 1\n\
        end\n";
    let c = fnv_script::compile::compile_mod_runtime(src, &table(), &World);
    assert_eq!(c.errors().count(), 0, "{:?}", c.errors().collect::<Vec<_>>());
    let (_, p) = build(src);
    let b = body(&p);
    assert!(matches!(&b[0], Stmt::Let { expr: XExpr::Assign { op: XAssign::BitOr, .. } }));
    assert!(matches!(&b[1], Stmt::Let { expr: XExpr::Assign { op: XAssign::BitAnd, .. } }));
    assert!(matches!(&b[2], Stmt::Let { expr: XExpr::Assign { op: XAssign::Mod, .. } }));
    assert!(matches!(&b[3], Stmt::Let { expr: XExpr::Assign { op: XAssign::Pow, .. } }));
    assert!(matches!(&b[4], Stmt::Let { expr: XExpr::Assign { op: XAssign::BitOr, .. } }));

    // NVSEScript compound bitwise/modulo assignments translate and execute cleanly.
    let nvse_src = "name BitwiseAssign;\nint a;\nint b;\nGameMode {\n  a = 1;\n  a |= 4;\n  b = 15;\n  b &= 6;\n  b %= 4;\n}\n";
    let h2 = run(nvse_src);
    assert_eq!(h2.0[&1], Value::Num(5.0), "NVSEScript: 1 | 4 = 5");
    assert_eq!(h2.0[&2], Value::Num(2.0), "NVSEScript: (15 & 6) % 4 = 2");
}

/// Negative compiler checks matching xNVSE 6.4.9: `<<=` and `>>=` do not exist even under
/// `let` (`Too many operators`, rejected at compile time here [design]), and bare
/// Let-macro / bare inline typed declarations reject `^=` (requires `let`).
#[test]
fn unsupported_shift_assign_and_bare_pow_macro_are_rejected() {
    for (label, src) in [
        ("let <<=", "int x\nbegin GameMode\nlet x <<= 2\nend\n"),
        ("let >>=", "int x\nbegin GameMode\nlet x >>= 2\nend\n"),
        ("bare <<=", "int x\nbegin GameMode\nx <<= 2\nend\n"),
        ("bare >>=", "int x\nbegin GameMode\nx >>= 2\nend\n"),
        ("bare ^=", "int x\nbegin GameMode\nx ^= 4\nend\n"),
        ("nvsescript <<=", "name BadShl;\nint x;\nGameMode {\n  x <<= 2;\n}\n"),
        ("nvsescript >>=", "name BadShr;\nint x;\nGameMode {\n  x >>= 2;\n}\n"),
    ] {
        let c = fnv_script::compile::compile_mod_runtime(src, &table(), &World);
        assert!(
            c.errors().count() > 0,
            "{label} must fail to compile (matching xNVSE 6.4.9), but compiled with no errors"
        );
    }
    // Bare inline typed declaration (`int x ^= 4`) follows `MACRO_OPS` strictly: no
    // initialized `XAssign::Pow` statement is emitted without `let`.
    let lines = fnv_script::compile::lex::lines("int x ^= 4\n", &mut Vec::new());
    let stmt = fnv_script::compile::parse::statement(&lines[0]).expect("parses as plain decl");
    assert!(
        matches!(stmt.kw, fnv_script::compile::parse::Kw::Decl { init: None, .. }),
        "bare inline `int x ^= 4` must not parse with an initializer, got {:?}",
        stmt.kw
    );
}

/// `dATan2(y, x)` returns exact degrees in [-180, 180] across all four quadrants and axis
/// edges, with `(0, 0) -> 0.0` [observed: 2026-11-23, GSDocMitchellHouse (0x00103DF9),
/// Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 (10 official base masters), #3678 / PR #3679
/// probe `w1_3495_compound_assign_and_datan2`].
#[test]
fn datan2_covers_quadrants_and_axes() {
    let h = run(
        "float a0
float a1
float a2
float a3
float q1
float q2
float q3
float q4
float z
float d30
begin GameMode
let a0 := dATan2 0, 1
let a1 := dATan2 1, 0
let a2 := dATan2 0, -1
let a3 := dATan2 -1, 0
let q1 := dATan2 1, 1
let q2 := dATan2 1, -1
let q3 := dATan2 -1, -1
let q4 := dATan2 -1, 1
let z := dATan2 0, 0
let d30 := dATan2 1, 1.7320508
end",
    );
    for (k, want, tol) in [
        (1, 0.0, 1e-9),
        (2, 90.0, 1e-9),
        (3, 180.0, 1e-9),
        (4, -90.0, 1e-9),
        (5, 45.0, 1e-9),
        (6, 135.0, 1e-9),
        (7, -135.0, 1e-9),
        (8, -45.0, 1e-9),
        (9, 0.0, 1e-9),
        (10, 30.0, 1e-5),
    ] {
        assert!(matches!(h.0[&k], Value::Num(v) if (v - want).abs() < tol), "var {k}: {:?} vs {want}", h.0[&k]);
    }
}

#[test]
fn aim_anim_group_names_have_documented_codes() {
    // [documented: https://geckwiki.com/index.php?title=AnimGroups]: AimUp (18),
    // AimDown (19), AimIS (20), AimISUp (21), AimISDown (22).
    let t = table();
    for (group, code) in [("AimUp", 18u16), ("AimDown", 19), ("AimIS", 20), ("AimISUp", 21), ("AimISDown", 22)] {
        let play = compile(&format!("begin GameMode\nPlayGroup {group} 1\nend"), &t, &World, None);
        assert_eq!(play.errors().count(), 0, "{group}: {:?}", play.diags);
        assert!(play.bytes.windows(8).any(|w| w[..2] == 0x1013u16.to_le_bytes() && w[6..8] == code.to_le_bytes()), "{group}: {:02X?}", play.bytes);
        let check = compile(&format!("begin GameMode\nif IsAnimPlaying {group}\nendif\nend"), &t, &World, None);
        assert_eq!(check.errors().count(), 0, "{group}: {:?}", check.diags);
        assert!(check.bytes.windows(8).any(|w| w[..2] == 0x1128u16.to_le_bytes() && w[6..8] == code.to_le_bytes()), "{group}: {:02X?}", check.bytes);
        let (_, p) = build(&format!("ref fnH\nbegin GameMode\nSetOnPlayGroupEventHandler fnH 1 PlayerREF {group}\nend"));
        let Stmt::Eval { expr: XExpr::Call { call } } = &body(&p)[0] else { panic!("{:?}", body(&p)[0]) };
        assert_eq!(x(&call.args[3]), format!("#{code}"), "{group}");
    }
}

#[test]
fn plain_declaration_ignores_unseparated_trailing_words() {
    // A plain declaration without `=` declares only its variable (and any comma-separated
    // names); unseparated trailing words do not declare a function name as a local that
    // shadows later calls.
    let src = "SetEventHandlerAlt \"OwnedA\" (begin Function {}\n\
               array_var aFirst ar_Construct \"array\"\n\
               end)\n\
               SetEventHandlerAlt \"OwnedB\" (begin Function {}\n\
               array_var aSecond = ar_Construct array\n\
               end)\n";
    let c = fnv_script::compile::compile_mod_runtime(src, &table(), &World);
    assert_eq!(c.errors().count(), 0, "{:?}", c.diags);
    assert!(c.variables.iter().any(|v| v.name == "aFirst"), "{:?}", c.variables);
    assert!(c.variables.iter().any(|v| v.name == "aSecond"), "{:?}", c.variables);
    assert!(!c.variables.iter().any(|v| v.name.eq_ignore_ascii_case("ar_construct")), "{:?}", c.variables);
    assert!(warnings(&c, "variable_named_like_a_function").is_empty(), "{:?}", c.diags);
    // Comma-separated plain declarations still declare every named variable.
    let (c, _) = build("short iA, iB, iC\nbegin GameMode\nset iA to 1\nset iB to 2\nset iC to 3\nend");
    assert_eq!(c.variables.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["iA", "iB", "iC"]);
}

#[test]
fn braced_argument_lists_pass_multiple_arguments() {
    // Multiple space- or comma-separated arguments inside `{ ... }` (not followed by `=>`).
    let (_, p) = mod_ok(
        "ref fnH\nshort a\nshort b\nbegin Function {}\n\
         SetFunctionValue (call fnH {a b})\n\
         call fnH {a, b}\n\
         call fnH {a + 1, b - 1}\n\
         call fnH {a -1}\n\
         call fnH {}\n\
         end",
    );
    let b = body(&p);
    assert_eq!(b.len(), 5, "{b:?}");
    let Stmt::Eval { expr: XExpr::Call { call: sfv } } = &b[0] else { panic!("{:?}", b[0]) };
    let XExpr::Call { call: nested } = &sfv.args[0] else { panic!("{:?}", sfv.args[0]) };
    assert_eq!(nested.args.len(), 3, "fnH + 2 braced args: {:?}", nested.args);
    let counts: Vec<usize> = b[1..].iter().map(|s| match s {
        Stmt::Eval { expr: XExpr::Call { call } } => call.args.len(),
        other => panic!("{other:?}"),
    }).collect();
    assert_eq!(counts, [3, 3, 3, 1]);
    // Unclosed braced argument list still reports missing `}`.
    let bad = compile("ref fnH\nshort a\nshort b\nbegin GameMode\ncall fnH {a b\nend", &table(), &World, None);
    assert!(bad.errors().any(|d| d.kind == "syntax" && d.detail == "missing }"), "{:?}", bad.diags);
    // Arrow lambda `{params} => expr` stays a lambda.
    let (_, p) = build("short a\nshort b\nbegin GameMode\nCallWhen ({a, b} => a + b) ({} => 1)\nend");
    let Stmt::Eval { expr: XExpr::Call { call } } = &body(&p)[0] else { panic!("{:?}", body(&p)[0]) };
    assert!(matches!(call.args[0], XExpr::Lambda { .. }), "{:?}", call.args[0]);
    // Unary `&` / `*` (leading, after comma without space, and space-separated) and nested
    // arrow lambdas inside `{ ... }` keep compiling cleanly.
    let (_, p) = mod_ok(
        "ref fnH\nshort a\nshort x\narray_var b\nbegin GameMode\n\
         call fnH {&a}\n\
         call fnH {*b}\n\
         call fnH {a,*b}\n\
         call fnH {a *b}\n\
         call fnH {a &b}\n\
         call fnH {{x} => x + 1}\n\
         call fnH {a, {x} => x + 1}\n\
         end",
    );
    let b = body(&p);
    assert_eq!(b.len(), 7, "{b:?}");
    let arg_counts: Vec<usize> = b.iter().map(|s| match s {
        Stmt::Eval { expr: XExpr::Call { call } } => call.args.len(),
        other => panic!("{other:?}"),
    }).collect();
    assert_eq!(arg_counts, [2, 2, 3, 3, 3, 2, 3]);
    let Stmt::Eval { expr: XExpr::Call { call: box_call } } = &b[0] else { panic!() };
    assert!(matches!(box_call.args[1], XExpr::Unary { op: fnv_script::ir::XUnary::Box, .. }), "{:?}", box_call.args[1]);
    let Stmt::Eval { expr: XExpr::Call { call: unbox_call } } = &b[1] else { panic!() };
    assert!(matches!(unbox_call.args[1], XExpr::Unary { op: fnv_script::ir::XUnary::Unbox, .. }), "{:?}", unbox_call.args[1]);
    let Stmt::Eval { expr: XExpr::Call { call: lam1 } } = &b[5] else { panic!() };
    assert!(matches!(lam1.args[1], XExpr::Lambda { .. }), "{:?}", lam1.args[1]);
    let Stmt::Eval { expr: XExpr::Call { call: lam2 } } = &b[6] else { panic!() };
    assert!(matches!(lam2.args[2], XExpr::Lambda { .. }), "{:?}", lam2.args[2]);
}

#[test]
fn inline_declarations_in_if_testexpr_and_typed_colon_equals() {
    // `if TestExpr <type> <var> = <expr>` declares the local, strips its type word, and
    // keeps the assignment inside `TestExpr` (no `assignment_to_a_value` warning).
    let h = run(
        "array_var aSrc\nint hit\nint miss\nint sized\nstring_var sOut\nfloat fInit\n\
         begin GameMode\n\
         let aSrc := ar_List 42, 99\n\
         if TestExpr int iFirst = aSrc[0]\n\
           let hit := iFirst\n\
         endif\n\
         if TestExpr int iMissing = aSrc[9]\n\
           let miss := 1\n\
         endif\n\
         if (int iLen = ar_Size aSrc) == 2\n\
           let sized := iLen\n\
         endif\n\
         eval 1 && let sOut := \"ok\"\n\
         float fLocal := 3.5\n\
         let fInit := fLocal\n\
         end",
    );
    assert_eq!(h.0[&2], Value::Num(42.0), "hit");
    assert_eq!(h.0.get(&3).copied().unwrap_or(Value::Num(0.0)), Value::Num(0.0), "miss");
    assert_eq!(h.0[&4], Value::Num(2.0), "sized");
    assert_eq!(h.text(5), "ok");
    assert_eq!(h.0[&6], Value::Num(3.5), "fInit");
    // Unparenthesized inline declaration inside a declaration initializer and known remote quest var.
    let (c, _) = build(
        "array_var aSrc\nbegin GameMode\n\
         int ok = TestExpr int iElem = aSrc[0]\n\
         Let TestQuest.bDone := ok\n\
         end",
    );
    assert!(warnings(&c, "assignment_to_a_value").is_empty(), "{:?}", c.diags);
    assert!(warnings(&c, "unknown_function").is_empty(), "{:?}", c.diags);
    assert!(c.variables.iter().any(|v| v.name == "iElem"), "{:?}", c.variables);
}
