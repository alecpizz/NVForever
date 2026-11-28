# Spec note: extender script syntax

Covers the statements the extender adds to the script language: `let` and `:=`, `eval`,
`while`/`loop`, `foreach`, `break` and `continue`, inline declarations,
`Begin Function {...}`, lambdas and inline expressions as arguments, and how `&&`/`||`
bind.

Filled for task G4 of the extender plan ([extender-core-tasks.md](../handoff/extender-core-tasks.md))
from the GECK wiki export, as the source for A7 (the extender expression grammar).
Everything the pages state is tagged `[documented: <page>]`; what they leave out is
**open (to observe)**, to be settled by the GECK compile cases
(tools/capture/ext-compile/) or captures in the original game.

Behaviour is our own wording of the GECK wiki export (`C:\fnv-private\geckwiki\`,
private); no extender source, no opcode numbers or argument encodings (those come only
from a cited document or from compiling our own test scripts with xNVSE,
[extender-compat.md](../design/extender-compat.md) §4.1). Version numbers are the
xNVSE release a page says a feature arrived in.

## 1. Where extender expressions are allowed

The extender has its own expression compiler beside the GECK's [documented: Inline
Expression]. It is used:

- **in the arguments of extender-aware commands**: Let, Eval, TestExpr, While, ForEach,
  Print and the like [documented: NVSE Expressions; Let; TestExpr notes]. Any of them
  accepts an assignment (`:=`, `+=`, …) inside its expression, not only Let
  [documented: TestExpr; While];
- **inside parentheses passed as a function argument** (inline expressions, xNVSE
  6.1.0): `Player.SetActorValue Health (Player.GetActorValue Health + 50)` compiles
  from then on and did not before [documented: Inline Expression]. A string there, in
  an actor value's or an axis's place, is the name in any case: `GetAV ("Strength")`,
  `GetAV (s)` and `GetAV ($s)` read Strength, `SetAV ("Luck") 7` sets Luck, `GetPos
  ("Z")` is `GetPos Z`; an unknown name (`GetAV ("NoSuchAV")`) raises no error the
  script stops on, but a `let` assigns nothing; a bare `GetAV s` does not compile
  [observed: #2276 job A, save nvfairport];
- **in a whole block whose type has a leading underscore** (the compiler override,
  deprecated): see §8.

Everywhere else a line is compiled by the GECK's own rules, the base game's language
(script-language.md). A GECK script line is at most **512 characters**, which long
inline expressions and lambdas reach first [documented: Inline Expression].

## 2. Operators

From the operator table [documented: NVSE Expressions]. Higher precedence binds tighter.
Associativity is stated only for `:=` (right to left); for the others it is **open (to
observe)**; the compile cases settle it from the compiled order.

| Prec. | Operator | Operands | Meaning |
|---|---|---|---|
| 0 | `:=` | 2 | Assign the right side to the variable or array element on the left. **Right-associative**; its value is the value assigned, so `a := b := c := 0` sets all three. A string is copied, an array is shared. |
| 1 | `\|\|` | 2 | Logical or (§3). |
| 2 | `&&` | 2 | Logical and (§3). |
| 2 | `+=` `-=` `*=` `/=` `^=` | 2 | Arithmetic on the left variable or array element, then assign. `^=` raises to a power (`let x ^= y` confirmed: `3 ^= 4 = 81` [observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9, 10 official base masters, `#3678` / PR `#3679` probe `w1_3495_compound_assign_and_datan2`: `int x = 3; let x ^= 4` -> `81`; no DLL or source consulted]). |
| 2 | `\|=` `&=` | 2 | Bitwise or (and) after demoting both sides to integers, then assign (`5 \|= 2 = 7`, `7 &= 3 = 3`, float truncation `5.9 \|= 2.8 = 7` [observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9, 10 official base masters, `#3678` / PR `#3679` probe `w1_3495_compound_assign_and_datan2`: `let x \|= 2`, `let x &= 3`, and `float f = 5.9; let f \|= 2.8` -> `7`; no DLL or source consulted]). |
| 2 | `%=` | 2 | Remainder of integer division, then assign (`13 %= 5 = 3` [observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9, 10 official base masters, `#3678` / PR `#3679` probe `w1_3495_compound_assign_and_datan2`: `int x = 13; let x %= 5` -> `3`; no DLL or source consulted]). |
| 3 | `:` | 2 | Range (slice): a substring, or a copy of the array elements in the range; **the upper end is included**; negative string indexes count from the end. |
| 3 | `::` | 2 | Key–value pair: a number or string key, a value of any type. |
| 4 | `==` `!=` | 2 | Equality of comparable values. Arrays: by top-level contents since xNVSE 6.2.1 (by identity before); nested arrays by identity. |
| 5 | `>` `<` `>=` `<=` | 2 | Ordering of comparable, ordered values; strings compare without regard to case. |
| 6 | `\|` | 2 | Bitwise or, operands demoted to integers. |
| 7 | `&` | 2 | Bitwise and, operands demoted to integers. |
| 8 | `<<` `>>` | 2 | Shift left (right) by a number of bits, operands demoted to integers. |
| 9 | `+` `-` | 2 | Addition, or joining two strings; subtraction. |
| 10 | `*` `/` `%` | 2 | Multiplication, division, remainder of integer division. |
| 11 | `^` | 2 | Power. |
| 12 | `-` | 1 | Negation. |
| 12 | `$` | 1 | To string (ToString; ext-strings.md). |
| 12 | `#` | 1 | To number (ToNumber; ext-strings.md). |
| 12 | `*` | 1 | Unbox: a string map's "value" element, otherwise the first element (ext-arrays.md). |
| 12 | `&` | 1 | Box: a one-element array holding the value. |
| 13 | `!` | 1 | Logical not. |
| 14 | `( )` | — | Grouping. |
| 15 | `[ ]` | 2 | Subscript: an array element by key, or a one-character string; the inside acts as if parenthesised. |
| 15 | `->` | 2 | String-map member: `m->key` is `m["key"]`. |
| 15 | `.` | — | Calling reference (`ref.Func`), or a quest script's variable (`Quest.var`); chains (§5). |
| 16 | `{ }` | — | The parameter list of a user-defined function or lambda (ext-udf.md). |

Notes and gaps:

- Unary and binary `-`, `*` and `&` share a symbol; which one is meant follows from
  position (an operand before it or not) [documented: NVSE Expressions, by the table's
  operand counts]. How the grammar decides: open.
- `^` on a negative base or a fractional power, `/` by zero, `%` with float operands:
  open.
- Mixing types (a string `+` a number, comparing a form to a number): open; `$` is the
  documented way to join a number to a string [documented: NVSE Expressions examples].
- **How far a function's arguments reach.** In an ordinary block, a call's arguments
  are single operands and the operators after them apply to the call's result:
  `let fTemp := Player.GetActorValue Health + 50` is (GetActorValue Health) + 50, the
  value the Inline Expression page then passes on as the new Health [documented: Inline
  Expression, its "compiles pre-6.1" example, read with the use it makes of fTemp; Let,
  whose examples assign call results the same way]. **Only in a compiler-override block**
  does a call take the whole rest of the expression: `if Sin 14 == 1` there evaluates
  `14 == 1` first, leaving `Sin 0`, which the page calls evaluating "differently"
  [documented: Script Compiler Override, "Unforeseen Consequences"]. So A7 parses
  arguments as operands, and greedily only inside `_Block`s. Probes expressions/05 (normal)
  and expressions/09 (override) check the same line `let r := Abs -3 + 1` both ways.

## 3. `&&` and `||`

- **Value, not just truth.** In an assignment, `||` gives the left value when it is
  non-zero and otherwise the right value; `a := 1 || 2` is 1, `0 || 2` is 2, `0 || 0`
  is 0 [documented: NVSE Expressions]. Since xNVSE 6.2.5, `&&` gives 0 when the left is
  false and otherwise the right value: `-5 && -5` and `1 && -5` are -5, `0 && -5` is 0
  [documented: NVSE Expressions]. An operand that is not a number (form, array, string)
  becomes 1 or 0 when it is the value given [documented: NVSE Expressions].
- **The base game's `&&` and `||` give only 1 or 0** [documented: NVSE Expressions];
  the difference only shows where the result is stored or used as a number.
- **Short-circuit**: under xNVSE 6, `eval` stops evaluating at the first operand that
  decides the result [documented: Eval]. Elsewhere (Let, While, TestExpr): open.
- **Precedence**: `||` (1) binds looser than `&&` (2), so `a || b && c` is
  `a || (b && c)` [documented: NVSE Expressions, by the table]. Case 21 of the compile
  cases checks the compiled order.
- `&&` sits at the **same level as the compound assignments** (2) in the table; how an
  expression mixing them parses: open.

## 4. Statements

**let** `let target op value` with `op` one of `:=` or the compound assignments. The
target may be a local, a quest variable (`Quest.var`), an array element or a string.
Compared with `set`: more operators, arrays, strings and function results are allowed,
and it is faster; **a failed assignment prints an extender error naming the mod and
script, and the script continues**, where `set` fails without a word [documented: Let].
A function returning a string or array, or a user-defined function's result, must be
assigned with `let`, not `set` [documented: Let; SetFunctionValue; Ar List].

**The let macro** (xNVSE 6.1.0): a line `x = 10`, `x += 10`, `x -= 5`, `x *= 5` with no
keyword is a `let` [documented: Let Macro]. `x /= 5` is taken too [observed:
JustAssortedMods.esp 4.6 ships `fAP /= GetNumericGameSetting …` in JBTOnFireWeaponUDF with
compiled bytes, so the original's compiler takes it]. Bare `|=`, `&=`, and `%=` (alongside
`+=`, `-=`, `*=`, `/=`) also compile and execute without `let`, whereas bare `^=` fails to
precompile (requires `let`), and `<<=`/`>>=` do not exist even under `let` (bare forms fail
to precompile, while `let x <<= y` / `let x >>= y` log `Error: Too many operators.` and
fall back to plain RHS assignment, whereas binary `<<` and `>>` work: `3 << 2 = 12`,
`20 >> 2 = 5`) [observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New
Vegas 1.4.0.525 + xNVSE 6.4.9, 10 official base masters, `#3678` / PR `#3679` probes
`b3_syn_01_shift_compound_assign` (`let iA <<= 4; let iB >>= 2` -> `a=4, b=2`),
`b3_syn_02_bare_compound_assign` (bare `iV += 5; iV -= 3; iV *= 2; iV /= 4` -> `6`), and
`w1_3495_compound_assign_and_datan2` (individual `CompileScript` + `Call` scripts testing
bare `+=`, `/=`, `|=`, `&=`, `%=` -> `8, 5, 7, 3, 3`; bare `^=`, `<<=`, `>>=` ->
`COMPILE_FAIL`; `let <<=`/`>>=` -> `Error: Too many operators.` + RHS `2`; binary
`3 << 2 = 12`, `20 >> 2 = 5`); no DLL or source consulted]. NVForever rejects both bare and
`let` forms of `<<=`/`>>=` at compile time rather than emitting a fallback RHS assignment,
and restricts bare inline typed declarations (`int x <op> ...`) to the same `MACRO_OPS` set
[design].

**eval / if eval** `if eval expr`, `elseif eval expr`: the condition is compiled as an
extender expression and reduced to true or false. It is needed when the condition uses
a string, an array or a user-defined function, which the base compiler misreads; on a
plain condition it is harmless but slightly slower, and a failing call then reports an
error instead of failing silently [documented: Eval]. `while` does not need it
[documented: Eval].

**eval on a line of its own** `eval expr`: the expression is run for what it does. Mods
write it before a call that needs the extender's reading of its arguments (`eval
Quest.rRef.Activate PlayerRef 1`) and before an assignment (`eval iTemp = iTemp * (1 -
Quest.rRef.GetAV Invisibility)`), which then assigns as a `let` does [documented: Eval, an
NVSE-aware command taking an expression; observed: JustAssortedMods.esp 4.6 ships 32 such
lines with compiled bytes, two of them assignments; hypothesis: the value the original's
eval returns for an assignment, which a line of its own drops]. `fnv_script::compile`
reads the tokens after `eval` as one expression (`Kw::Eval`).

**TestExpr** `TestExpr expr` (NVSE 4.1): evaluates the expression, side effects
included, and returns 1 if it ran without an extender error, 0 if it raised one; the
error is not printed [documented: TestExpr]. `if TestExpr a[k] += 1` increments when the
key exists and is false otherwise.

**Inline declaration** (xNVSE 6.1.0): a type may stand before the first use instead of
in the variable list at the top: `let int i := 10`, `int i = 10`, `foreach array_var it
<- arr`, `if eval ref r = GetCrosshairRef && IsReference r` [documented: Inline
variable declaration]. The scope of such a variable (the whole script, or the block):
open.

**Return** stops the script for this run, every block below the line included; a
GameMode-type block runs again next frame. **It cannot be used inside a while or
foreach loop** [documented: Return]. (A base-game statement; its restriction inside
loops is the extender's.) Whether the GECK refuses it there or it misbehaves at run
time: open.

## 5. Additions in xNVSE 6.1

- Inline expressions (§1), the let macro and inline declarations (§4) [documented:
  Inline Expression; Let Macro; Inline variable declaration].
- **Chained dot syntax**: inside extender expressions, `.` also works on the result of a
  function (`GetSelf.SetActorValue …`), on another quest's or reference's ref variable
  (`Quest.rVar.Func`) and on an array element holding a reference
  (`a[100].GetBaseObject`); before, only on a local ref variable or a reference form
  [documented: Chained dot syntax]. A head that names no variable or form but a
  function (`GetSelf.GetProjectileRefDamage`) compiles as that function called with no
  arguments, the tail called on its result [observed: projectile set rows proj.reg and
  proj.reg2 compile in the original, #2036]; a head naming nothing is still refused.
- **Binary literals**: `0b` followed by bits, highest first, wherever extender
  expressions are allowed [documented: Binary Notation]. The page's own example says
  `0b101010` equals 52; by its stated rule (bits highest first) it is 42, so the
  example is a slip [documented: Binary Notation; the arithmetic is ours]. Compile case
  to add: open.
- **Hexadecimal literals** (xNVSE 6.1.0): `0x` followed by hex digits, wherever
  extender expressions are allowed; `0x14` is 20 [documented: Hexadecimal Notation].
  Letter case of the digits and of the `x`: open.
- **Block comments** (xNVSE 6.1.7): `/*` starts a comment, `*/` ends it, across any
  number of lines [documented: Multi-line comments]. Nesting, and a `/*` inside a
  string: open.

## 6. Loops

**while** `while expr` … `loop`: repeats the lines up to `loop` while the condition is
true. The condition is an extender expression and may assign (`while 5 > (i += 1)`),
which then runs on every test, the last failing one included. A condition that never
becomes false hangs the game (a crash in the original) [documented: While].

**foreach** `foreach entry <- source` … `loop`: over arrays, strings, containers and
form lists (xNVSE 6.2.5); the entry for each kind is ext-arrays.md §3 [documented:
ForEach]. The type may be given on each name (`e:array <- src:array`, `item:tempref <-
container:ref`). The whole head may stand in one pair of parentheses, `foreach (entry <-
source)` [observed: Unofficial Patch NVSE Plus and JSawyer Ultimate ship such lines with
compiled bytes, the source a variable, a call and an `ar_List 0, 1, …`].

**A comma after a function's name.** `rForm = ListGetNthForm, SomeList, iPos`: a comma
between the name and a first argument the function cannot go without separates as a
space does [observed: Unofficial Patch NVSE Plus ships that line with compiled bytes;
hypothesis: the original's compiler takes commas as spaces there]. After a function that
takes no argument, or can go without its first, the comma belongs to what the call stands
in (`ar_List GetSelf, 2` is two elements) [design].

**GetFactionRank in an extender expression** takes its faction, though the catalogue's
row has no parameter list: a numbered function with no listed parameters takes its
condition's [documented: the xEdit definitions give `ptFaction`; observed: JIP Companions
Command & Control 2.60, `(rRef.GetFactionRank <faction> > 9)` beside an extender call].

**break** leaves the innermost while or foreach at once; **continue** skips to the next
test (while) or entry (foreach). Both are only allowed inside such a loop
[documented: Break; Continue; While; ForEach]. Nested `while` loops: `break` and `continue`
act strictly on the innermost loop [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9,
2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only
`PrintToConsole` probes `core_06_nested_loop_break_continue`
(`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679):
`PROBE|core_06_nested_loop_break_continue|3,3` and `w4c_b2_07_nested_break_continue`
(`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679, nested `while` loops testing
inner `break` and inner `continue`): `PROBE|w4c_b2_07_nested_break_continue|brk=3,cont=6`;
no DLL or source consulted].

**Label / GoTo** (FOSE1): `Label n` marks a place, `GoTo n` jumps back to it; `n` is
0–255 (256 labels; other values do not work); the label must come before the GoTo. The
pages call it outdated next to while/foreach [documented: Label; Goto]. GoTo to a label
in a different block, or forwards: open.

## 7. Blocks

- **Begin Function {params}** declares a user-defined function; lambdas are
  `(begin function {params} … end)` or `{params} => expr` inline (ext-udf.md)
  [documented: Lambda; SetFunctionValue].
- Event blocks are the base game's (script-functions.md §3.3); extender events by
  handler are ext-events.md's.

## 8. The script compiler override (deprecated)

A block type with a leading underscore (`Begin _GameMode`, `_MenuMode`,
`_ScriptEffectStart`, `_OnActivate`, `_Function`, …) compiles its whole block with the
extender's compiler. That lets array elements, string variables and function results be
passed straight to base-game and older extender functions whose parameters the base
compiler would refuse (`floor somearray[k]`, `rActor.GetAV sAVName`), at the scripter's
risk: types are checked at run time, not compile time, and parentheses are needed more
often. Inline expressions (xNVSE 6.1) replace it and behave more predictably
[documented: Script Compiler Override]. Compile case 23 uses it. Inside such a block a
function call takes everything after it as its argument (`Sin 14 == 1` runs as
`Sin (14 == 1)`), unlike an ordinary block; §2 "How far a function's arguments reach"
[documented: Script Compiler Override].

## 9. Out of scope here: NVSEScript

xNVSE 6.3.6 added a second, C-like script language ("NVSEScript": scripts start with
`name X;`, statements end with `;`, calls use `f(a, b)`, `//` comments, event blocks as
`GameMode { … }`). Its scripts can call and be called by ordinary scripts [documented:
NVSEScript, marked work in progress]. The plan compiles the ordinary language first;
NVSEScript is a later task if mods need it.

## 9a. What the GECK accepted (compile cases, 2026-10-25)

The owner compiled the 23 cases of tools/capture/ext-compile/ in the GECK with xNVSE
loaded, as Object scripts [observed: ext-compile README, Observed column]:

- **All 22 forms expected to compile were saved without an error**: `let` and `:=`,
  chained `:=`, the let macro (`x = 10`, `x += 10` without `let`), the compound
  assignments, `if eval` with `!` on a call, `while`/`loop` with an assignment in the
  condition, `break`/`continue`, `foreach` over an array and over a container,
  `string_var`, `array_var`, `sv_Length`, subscripts and `ar_size`, inline `int`
  declarations (with `let` and with the macro), `Begin Function {a, b}`, `Call` with
  arguments and in `if eval`, `SetFunctionValue` in branches, block and one-line
  lambdas, an inline expression as a function argument, `&&` and `||` without
  parentheses (in `let` and in `if eval`), and `Begin _GameMode`.
- **`let := 5` (no variable) was refused** with two messages on its line: "Missing
  required parameter 'expression' for parameter #'1'." and "Could not parse this line."
  So the GECK treats everything after `let` as one parameter of type "expression".

Compiling says the forms parse; how they group (associativity, `&&` against `||`) needs
the compiled bytes or the run-time probes (ext-probes/expressions).

## 10. Open (to observe), collected

Associativity of every operator but `:=`; how unary and binary `-`/`*`/`&` are told
apart; `&&` against the compound assignments at the same level; short-circuit outside
`eval`; the let macro's other operators; inline-declared variable scope; `Return` in a
loop; hex digit case; nested and unclosed block comments; break/continue in nested loops;
GoTo across blocks.

## 11. Refused at run time (#2037)

xNVSE 6.4.9's runtime compiler (the console, CompileScript, the Script Runner's files,
nvf_capture's query steps) refuses forms that NVForever's compiler took. The owner ruled on
2026-11-10 that NVForever refuses them too ("Yes, refuse them", #2037), so
`fnv_script::compile::compile_runtime` adds these errors on top of `compile`. Scripts the
editor compiled keep their bytes, and the source a plugin carries is compiled by `compile`
with a seed (`fnv_game::source_compile`), so neither meets them [decision].

- **A fractional number literal for an int parameter of a plugin function** does not
  compile [observed: no golden row for jip.n7 karma.set (`SetBaseKarma 37.5 rBase`), jip6
  dest.settrait.1.1.3.5 (`SetNthDestructionStageTrait rObj 1 1 3.5`), showoff3
  so3.ease.badcode (`ApplyEasingAlt 0.5 1.5 0`) and so3.seed.range.fraction
  (`RandSeeded 1.7 3.2`), jg3 casino.setwin.tops.frac (2.7), jg4 regn.setpriority.gs.frac
  (20.6), sup1 roundalt.flag2 (`RoundAlt 37.66 2.7 1`) and words.7p9 (`NumberToWords
  7.9`); the whole-number rows beside each compiled]. A fraction for a float parameter
  compiles (so3.ease.outside, `ApplyEasingAlt 1.5 1 0`; lutana1 radius.set.base.frac), and
  so does a float variable for an int parameter (jip6's put-back step, `… fSave`). "Int"
  is the catalogue's kind, plus SetBaseKarma's karma, which the wiki types float
  [observed: karma.set against karma.set.int]. A literal with nothing after the point
  (`37.0`) is not refused [hypothesis: no row]. Functions of the base game are not checked
  [hypothesis: no row].
- **The result of a command registered without a return type, given to a form parameter**,
  does not compile [observed: nvse.misc pkg.nth0.location, `GetFormIDString
  (GetPackageLocation rG)`; nvse-seventh pkg.removeat0, `GetFormIDString (rDoc.RemovePackageAt
  0)`; ext-jg6.md, `GetFormIDString (rP.GetCurrentFurnitureRef)`]. Through a variable it
  compiles (jg6 furniture.player). The catalogue's return types do not tell these commands
  apart (GetPackageLocation's page says it returns a ref), so the three are listed by name
  (`UNTYPED_RESULTS`, compile/gen/x.rs); other form parameters than GetFormIDString's
  [hypothesis].
- **Not pinned, kept compiling** [decision: #2037, the owner's ruling applies only where
  the data gives a rule]: jg8 shuffle (`Ar_Shuffle (Ar_List 1, 2, 3, 4, 5)` then
  `aOut[0] + aOut[1] + …`, refused both as a sum inside sv_Construct and summed with `let`;
  an inline `ar_List` argument compiles for ShowOff's matrix functions and string `+` with
  an element compiles (nvse.n9 files.plugins), so either Ar_Shuffle's parameter or numeric
  element arithmetic could be the cause, ext-jg8.md) and showoff3 so3.jumping
  (`rP.IsJumping`: not registered under that name, or no calling reference, ext-showoff.md).

## Observed in the original game (2026-10-26)

[observed: <id>] = tools/capture/ext-probes, run unattended in the original game (xNVSE 6.4.9, JIP 57.30) on 2026-10-26 by tools/capture/ext_probe_run.py from the owner's private test plugin; the results file is private (`C:\fnv-private\goldens\ext\`). Where a row settles an "open" point above, the observation wins.

| Probe | Input | Result | So |
|---|---|---|---|
| assoc.pow / sub / div / mod | 2^3^2, 10-4-3, 64/4/2, 17%5%3 | 64, 3, 8, 2 | all four group left to right |
| shift.right_neg | -8 >> 1 | -4 | the shift keeps the sign |
| bitor.neg_fraction | -5.7 \\| 0 | -5 | fractions are truncated toward zero |
| mod.fraction | 7.5 % 2 | 1 | % works on integers |
| rand.comma / rand_nocomma | Rand 5, 5 and Rand 5 5 | 5 and 5 | equal bounds give that value; arguments without a comma are accepted |
| chain.compound | let x += y += 1 (x=1, y=1) | x 3, y 2 | compound assignments are right-associative |
| and.vs_compound | let iR := 1 && iX += 5 | run-time error, iX unchanged | grouped as (1 && iX) += 5: same level, left to right |
| fn.abs_swallow / fn.sin_eq | Abs -3 + 1; Sin 90 > 0.5 (ordinary block) | 4; 1 | a call's argument is one operand |
| override.abs_swallow | Abs -3 + 1 under Begin _GameMode | 2 | the call takes the arithmetic after it |
| override.sin_eq | Sin 90 > 0.5 under Begin _GameMode | 1 | **differs from the wiki's Sin 14 == 1 example**: the comparison is not taken into the argument |
| neg.not / not.neg | -!0 / !-1 | -1 / refused to compile | ! straight before unary - is refused |
| nested.inner_break / continue | break / continue in an inner while | 3 / 6 | they act on the innermost loop |
| foreach.formlist | foreach over ExtProbeList, directly and via a ref | 3 = ListGetCount | form lists work both ways (the refusal seen earlier was the list missing) |
| foreach.container | foreach over the player's inventory | 27 = GetNumItems | one entry per item kind |
| foreach.tempref | an inventory entry kept after the loop | valid that frame, invalid the next | temporary references last the frame |
| change.forward_erase | erase even values in a forward while over 1..6 | size 3 | no error; the stale size reads past the end silently |
| change.foreach_sees_write | write element 2 during foreach | 99 | foreach reads live values |

Under the compiler override, then, a call's argument takes the arithmetic after it
(`Abs -3 + 1` is 2) but stops at a comparison (`Sin 90 > 0.5` is 1) [observed: both rows;
the rule is a hypothesis drawn from two cases]. Whether `==`, as in the wiki's example,
behaves like `>`: open.

## 12. Extender `actor_value` parameter typing

- **Unnumbered extender `actor_value` parameters** (`crates/fnv-script/src/compile/gen/x.rs`):
  extender functions whose catalogue parameter has `kind: "actor_value"` — including
  `GetThresholdedActorValue` / `GetThresholdAV` [documented:
  https://geckwiki.com/index.php?title=GetThresholdedActorValue], `GetActorValueModifier`
  (`GetAVMod`), `GetActorValueModifierAlt` (`GetAVModAlt`), `GetBaseActorValueAlt`
  (`GetBaseAVAlt`), `ModBaseActorValue` (`ModBaseAV`), `SetEmbeddedWeaponAV`, `UnforceAV`,
  `UnsetAV`, `GetActorValueDamage` (`GetAVDamage`), and `GetActorValueName` (`GetAVName`) —
  resolve a bare known actor-value keyword (when not shadowed by a local variable, a `GLOB`
  record, or a function name) to `ParamEnc::Enum("actor_value")` (`XExpr::Enum { code }`)
  without emitting `unquoted_argument` [design]. Quoted strings, numeric codes, local
  variables, globals, dotted quest variables, function calls, and unknown bare words retain
  their existing `ParamEnc::Value` handling.
