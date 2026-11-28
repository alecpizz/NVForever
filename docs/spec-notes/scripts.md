# Spec note: compiled scripts (Layer 5)

Clean-room description of the compiled script format of the shipped masters and of the
canonical text used to validate the decoder. Sections 1–3 belong to workstream 5A (reader,
IR, decompiler; docs/design/layer5.md §7); sections 4 and later (variables at run time,
events, quests, conditions) belong to workstream 5B.

Tags: **[documented]** (source named), **[observed]** (checked against the owner's FNV
1.4.0.525 masters: FalloutNV.esm, DeadMoney, HonestHearts, OldWorldBlues, LonesomeRoad,
GunRunnersArsenal and the four packs, in that load order), **[hypothesis]**. Counts come
from `fnvtool script units|decode|coverage` over those ten masters (§3.5) unless said
otherwise. The function table, the opcodes and the Begin-block codes are in
[script-functions.md](script-functions.md); this note does not repeat them.

Code: `crates/fnv-script` (`unit`, `ir`, `decode`, `table`, `decompile`, `check`,
`resolve`), `crates/fnvtool/src/script_cmd.rs`. Oracle: `tools/capture/oracle_script.py`.

## 1. Script units

### 1.1 Where units live

A *unit* is one `SCHR` subrecord and the script subrecords that follow it: `SCDA`
(compiled bytes), `SCTX` (source text), `SLSD`+`SCVR` pairs (local variables), `SCRO` /
`SCRV` (reference list) [documented: xEdit `wbEmbeddedScript`; observed]. The unit ends at
the first subrecord that is none of these [observed: every unit]. The ten masters hold
80,736 units in seven record types [observed]:

| Owner record | Units | With SCDA | With SCTX | With both |
|---|---|---|---|---|
| SCPT | 3,715 | 3,715 | 3,715 | 3,715 |
| INFO | 57,866 | 8,651 | 8,921 | 8,650 |
| PACK | 14,625 | 558 | 595 | 557 |
| QUST | 1,359 | 1,058 | 1,102 | 1,058 |
| REFR | 1,814 | 141 | 157 | 140 |
| TERM | 1,350 | 384 | 407 | 384 |
| PERK | 7 | 7 | 7 | 7 |
| **total** | **80,736** | **14,514** | **14,904** | **14,511** |

Most embedded units are empty slots (an `SCHR` with nothing after it). 14,907 units have
compiled bytes or source; those are the ones `decode`/`decompile` list. Three have bytes
without source (one INFO, one PACK, one REFR) and 393 have source without bytes; the
source of all but three of those holds no statement, only comments or nothing (the three
are in §3.6).

### 1.2 Header, variables, references

`SCHR` (layout in `schemas/fnv/common.json`, `ScriptHeader` [documented: xEdit]) carries a
reference count, the compiled size, a variable count, the type (0 object, 1 quest,
0x100 effect) and flags.

- *compiled_size* equals the `SCDA` length in all 14,514 units that have one [observed].
- *ref_count* equals the number of `SCRO`/`SCRV` entries in 80,733 units. The other three
  are units whose source was never compiled (§3.6) [observed].
- *variable_count* is not a count to rely on [observed]: it equals both the number of
  `SLSD` entries and the highest index in 79,779 units, only the number in 4, only the
  highest index in 570 and neither in 383. Use the `SLSD` list.
- Script type: SCPT 2,959 object, 490 quest, 266 effect; every embedded unit with bytes
  is type 0 (object); seven empty embedded units (5 TERM, 2 QUST) say 1 [observed].

Local variables: `SLSD` gives the index and the type, `Integer` or `FloatOrRef`
[documented: xEdit]; `SCVR` that follows gives the name. Indices are not in declaration
order, and in 10 SCPT scripts (FalloutNV 0x00122B79, 0x00125495 (two), 0x00143835,
0x0014D3A7, 0x0015B7A5, 0x0015BD04, 0x00163439, 0x00164509, 0x00167720, 0x0017A85C) two
declarations share an index [observed]. Variables are addressed by index; with a shared
index the first declaration's name is used for printing [decision].

Reference list: entries in record order, addressed by a 1-based index from the bytecode
[observed, script-functions.md §3.2]. `SCRO` holds a FormID (stored file-local, mapped
through the plugin's master list like every FormID); `SCRV` holds the index of a local
variable, the way a `ref` variable is used where a reference is expected
(`rDoor.Activate`) [documented: xEdit "Local Variable Reference"; observed].

### 1.3 Owners and unit keys

Each unit gets an owner label from its record and the subrecords before it:

| Record | Label | Rule [observed unless tagged] |
|---|---|---|
| SCPT | `SCPT <FormID>` | one unit per record |
| QUST | `QUST <FormID> stage <s> entry <n>` | `s` = value of the last `INDX` (i16, schema `gameplay.json`); `n` counts the units after that `INDX` from 0. Each stage entry (`QSDT`, then its conditions and log text) is followed by exactly one unit (1,359 of 1,359). |
| INFO | `INFO <FormID> begin` / `end` | every INFO with units has two, separated by `NEXT` (28,933 records); the first is called *begin*, the second *end* [names: layer5.md; when each runs: 5B] |
| PACK | `PACK <FormID> begin` / `end` / `change` | the unit after `POBA`, `POEA`, `POCA` (all 4,875 PACK records with units have the three, in that order) |
| TERM | `TERM <FormID> item <n>` | `n` = number of `ITXT` (menu items) before the unit, minus 1 (every TERM unit follows an `ITXT`) |
| PERK | `PERK <FormID> entry <n>` | `n` = number of `PRKE` before the unit, minus 1 |
| REFR | `REFR <FormID> patrol` | the unit after `XPPA` (patrol script marker [documented: xEdit]) |
| other | `<SIG> <FormID> unit <n>` | `n` counts the record's units; none in the masters |

FormIDs are load-order (global) FormIDs, printed `0x%08X`. The unit key is
`<plugin file>|<label>`, e.g. `FalloutNV.esm|QUST 0x00104C1C stage 10 entry 0`. A record
overridden by a later plugin keeps its own units under each plugin's name.

## 2. Bytecode

### 2.1 Statement stream

Confirms script-functions.md §3.2 for all 14,514 compiled units [observed]: `SCDA` is a
sequence of statements, each opcode u16, body length u16, body. Opcode `0x001C` is a
prefix with no body: u16 reference index, then the statement it applies to. The prefix
occurs only before function calls (28,452 times). Every byte of every unit splits into
statements: 142,218 statements, none truncated. A `0x001C`-prefixed call counts as one
statement (also for skip counts, §2.6).

### 2.2 Statements

| Opcode | Statement | Body | Count |
|---|---|---|---|
| 0x0010 | Begin | event u16; u32 = bytes after this statement up to and including the matching End (5,702 of 5,702); then the event's parameter block (§2.4) | 5,702 |
| 0x0011 | End | empty | 5,702 |
| 0x0015 | Set | target (§2.5), u16 expression length, expression | 30,622 |
| 0x0016 | If | u16 skip count (§2.6), u16 expression length, expression | 16,851 |
| 0x0017 | Else | u16 skip count | 2,619 |
| 0x0018 | ElseIf | as If | 5,931 |
| 0x0019 | EndIf | empty | 16,906 |
| 0x001D | ScriptName | empty, first statement of every SCPT | 3,715 |
| 0x001E | Return | empty | 2,559 |
| ≥ 0x1000 | function call | the parameter block (§2.4) | 51,611 |

[observed; counts over the 14,907 listed units.] No other opcode below 0x1000 occurs.

Parameter block: either **empty** (body length 0, no count at all) or **u16 parameter
count followed by the parameters** [observed]. Which one is a property of the function:
112 of the 355 opcodes called are always empty, the other 243 always have a count, even
when it is 0 (8,630 calls) [observed]. The empty ones are functions without parameters
(`EvaluatePackage`, `StopCombat`, `GetDead`, …) [hypothesis from the names; consistent
with §3.6 where the compiler drops source arguments to them]. Begin blocks follow the same
rule: 1,901 have no parameter block (GameMode, OnLoad, ScriptEffect*, …), 3,801 have one,
1,569 of them with count 0 [observed].

### 2.3 Expressions

The body of If, ElseIf and Set is a postfix (reverse Polish) token list; every token is
preceded by one space, 0x20 [observed]. Token forms [observed, counts over all units]:

| Bytes after the space | Token | Count |
|---|---|---|
| ASCII digits with at most one `.` | number literal, **the text as written in the source** (`19.50`, `.5`, `0.0`) | 50,769 |
| `==` `!=` `<` `<=` `>` `>=` `&&` `\|\|` `+` `-` `*` `/` `%` | binary operator | 39,805 |
| `~` | unary minus; the compiler writes `-1` as `1 ~` | 230 |
| `s` / `f` + u16 | local variable (§2.5) | 12,538 |
| `G` + u16 | global variable: reference list entry of a GLOB | 1,499 |
| `Z` + u16 | a reference list entry used as a value (`set rDoor to SomeREF`) | 2,608 |
| `r` + u16, then `s`/`f` + u16 | another script's variable (§2.5) | 9,478 |
| [`r` + u16] `X` + opcode u16 + u16 length + parameter block | function call, on that reference if `r` precedes | 16,320 |

Tokens are parsed by structure, not by splitting on spaces: binary operands contain 0x20
bytes. Parentheses never appear: the compiler resolves them into the token order. The
operator precedence it applies is in §3.3.

### 2.4 Parameter encodings

Parameters are **not self-describing**: the same leading byte means different things for
different parameters. 0x47 (`G`) is a global variable in `RewardKarma` but actor value 71
(Variable10) in `SetActorValue`; 0x5A (`Z`) is the axis letter in `SetPos` but the
reference-value token in an expression [observed]. The decoder therefore needs the
encoding of each parameter of each function. Five encodings occur [observed; resolves
script-functions.md §7 "parameter encodings beyond r/n"]:

| Encoding | Bytes | Count |
|---|---|---|
| value | `r`+u16 reference list entry (43,494); `n`+i32 integer (32,447); `z`+f64 float (1,561); `s`/`f`+u16 local variable (376); `G`+u16 global (60); `r`+u16+`s`/`f`+u16 another script's variable (216) | 78,154 |
| enum | a bare u16 code | 2,845 |
| axis | one ASCII byte `X`, `Y` or `Z` | 294 |
| string | u16 length + that many bytes (Windows-1252), no terminator | 1,391 |
| message trailer | after the parameters of `ShowMessage`: u16 count + that many values (the format variables), then an untagged i32; of `ShowWarning`: u16 count + values | 1,717 + 71 |

`crates/fnv-script/data/encodings.json` (`fnv.script.encodings/1`) lists the 35
functions with a parameter that is not a value, with the spellings of the enum codes the
shipped source uses. Every other function called by shipped scripts (319 functions, plus
the unknown opcode 0x127C) decodes with values only [observed]. The functions and calls (statement + expression calls):

- **axis**: Rotate 4, GetPos 82, SetPos 148, GetAngle 10, SetAngle 50.
- **enum, actor value**: GetActorValue 515, SetActorValue 730, ModActorValue 62,
  ForceActorValue 31, GetBaseActorValue 6, DamageActorValue 138, RestoreActorValue 284,
  SetPlayerTagSkill 23, IsPlayerTagSkill 23, GetPermanentActorValue 7. The codes are the
  indices of the documented actor-value list (`ActorValue` in `schemas/fnv/common.json`
  [documented: xEdit]); the source spellings differ from that list's names in places
  (`DamageResist`, `SpeedMult`, `RadiationRads` for "Rad Level", `SleepDeprevation`)
  [observed].
- **enum, animation group**: PlayGroup 864, IsAnimPlaying 114 (codes 0 Idle, 2
  SpecialIdle, 3 Forward, 4 Backward, 5 Left, 6 Right, 7–10 Fast*, 15 TurnLeft, 17 Aim,
  24 Equip, 25 Unequip [observed]; 227 JumpStart, 228 JumpLoop [observed: a mod's compiled
  script, JustAssortedMods.esp 4.6 `JVSAnimEffectScript`, whose two IsAnimPlaying calls
  carry these codes in source order; no shipped unit names them]; 44 Attack4, 47
  Attack4IS, 50 Attack5, 62 Attack7, 201 JamB, 223 Stagger [observed: the PlayGroup calls
  of Project Nevada 2.5's and Unofficial Patch NVSE Plus 1.7.3's compiled scripts beside
  their source, each name with one code in every unit that has it]).
- **enum, form type**: GetIsObjectType's parameter, stored as an animation group is
  (xEdit's `ptFormType`). 27 Container [observed: Project Nevada - Core.esm 2.5,
  `obj.GetIsObjectType Container` compiled to `B1 11 04 00 01 00 1B 00`; no shipped unit
  calls the function]. Other type names are not read yet: an error in the editor's
  compile, a warning and code 65535 in a mod's source (script-compiler.md §7).
- **enum, sex**: GetIsSex 6, GetPCIsSex 8, SexChange 12 (0 Male, 1 Female).
- **enum, misc stat**: ModPCMiscStat 6, GetPCMiscStat 4 (the source writes the stat name
  quoted, `"Corpses Eaten"`; the bytes hold the code).
- **enum, critical stage**: SetCriticalStage 7, IsInCriticalStage 24 (0 None, 1 GooStart,
  2 GooEnd, 3 DisintegrateStart, 4 DisintegrateEnd).
- **string**: PlayIdle 318 (the idle's Editor ID as text), GetGameSetting 137, SwapTexture
  383 and SetSecuritronExpression 37 (value, string, string), PlayBink 4 (file name, then
  values), PipBoyRadio 6, EnableNavMesh 5 and DisableNavmesh 2 (the navmesh FormID as hex
  text, `BE880`), IsDLCInstalled 8.
- **message trailer**: ShowMessage 1,717 (parameter count 1, the message; then 0–2
  format variables in the masters; the i32 is the duration, 0 in 1,716 calls and 1 where
  the source writes `ShowMessage VCG01CasualHardcoreMessage 1`), ShowWarning 71 (one
  string, then a variable count, 0 in all 71).

Value positions: `r`+u16 followed by `s`/`f` reads either as one remote variable or as a
reference followed by a local-variable parameter (`RemoveItem Ammo556mm ammoCount` is
`r`, `s`). The stored parameter count decides: in every call of the masters exactly one
reading uses the bytes with that count [observed]. The decoder tries the remote reading
first and backtracks.

For functions that shipped scripts never call, encodings.json maps xEdit condition
parameter types to the observed encodings (`ptActorValue` → actor value, `ptAxis` → axis,
`ptSex`, `ptMiscStat`, `ptCriticalStage`) [hypothesis: same encoding as the called
functions with that type]. Any other unexplained parameter bytes become `Arg::Raw` and
are counted. **Result: 0 Raw parameters, tokens or statements in all 14,907 units**
[observed].

### 2.5 Variables and Set targets

- `s` + u16 is a local declared `Integer`, `f` + u16 a local declared `FloatOrRef`
  (`float` or `ref` in the source) [observed: every local use in every unit matches its
  `SLSD` type; resolves the letter hypothesis of script-functions.md §3.2].
- Set targets: `s`/`f` + u16 (local; 9,207 short, 4,767 float), `G` + u16 (global; 782),
  `r` + u16 + `s`/`f` + u16 (another script's variable; 13,825 short, 2,041 float)
  [observed].
- `r` + u16 + `s`/`f` + u16 names variable *index* u16 of the script that the reference
  list entry runs: a quest's `SCRI`, or a placed reference's base form's `SCRI` (`NAME` →
  `SCRI`), or the form's own `SCRI` [observed: resolves for every remote use but one unit,
  below]. A later plugin can override the target script, so the name behind an index
  depends on who looks (`fnv_script::resolve::Viewer`): the plugin that holds the unit and
  its masters (what the source was compiled against) or the whole load order (what runs).
  Data that matter to the interpreter [observed]:
  - FalloutNV SCPT 0x00127959 uses variables 1 and 2 of the script of
    `GomorrahCasinoEnterTriggerREF`; that script now declares only indices 3 and 4. The
    unit was compiled against an older version.
  - LonesomeRoad SCPT 0x0400AC93 writes variable 8 of six tunneler references' script,
    compiled as `bRetreat`; in the shipped script index 8 is `fTimer` and `bRetreat` is 9.
  - 12 remote uses in DeadMoney INFO result scripts write `s` (short) to variables the
    target script declares `float` (`iL1`…).
  - DeadMoney overrides `VMQTopsScript`; index 37, `AmbushDefeated` in FalloutNV, is named
    `PlayerKilledAmbush` there. FalloutNV SCPT 0x00117509 writes index 37.
  What the original engine does with a missing index or a type mismatch is for 5B/5C to
  establish [hypothesis: it uses the index as stored].

### 2.6 If nesting and skip counts

The skip count of If, ElseIf and Else is the number of statements strictly between the
marker and the next marker of the same chain at the same depth (If/ElseIf → the next
ElseIf, Else or EndIf; Else → EndIf) [observed: 25,395 of 25,401]. The decoder rebuilds
the nesting from the markers and checks every count.

The six others (FalloutNV SCPT 0x0007EA22, 0x0007FD62, 0x00080777, 0x000B9A39,
0x000EAB00, 0x000F6272) are one too large, and each span contains exactly one local
variable declared inside the block (`ref mytarget` and similar): the compiler counted the
declaration line, which produces no statement [observed]. If the original engine jumps by
the stored count, it lands one statement past the marker; in all six that statement is
End, the first statement of an Else body, or (0x000EAB00) a stray Else, so the structured
reading runs the same statements [hypothesis on how the engine jumps].

Markers without a matching If are accepted by the compiler and kept in place
(`Stmt::Unmatched`) [observed]:
- 55 EndIf with no open If, in 45 units; 16 ElseIf and 1 Else after the chain was closed,
  in 8 and 1 units (FalloutNV 0x0011FC65, 0x00123191, 0x0014B084, 0x001746C1, 0x0017B778,
  0x0017B779, HonestHearts 0x02009FE4, LonesomeRoad 0x04009C97; Else: 0x000EAB00);
- 24 markers (20 ElseIf, 4 Else) after the Else of the same chain, in 4 units (FalloutNV
  0x0015D9ED, 0x00161F06 (20), 0x001746C1, OldWorldBlues 0x0301144C). The compiler chains
  their skip counts with the rest of the chain (the counts check out against that
  reading), so the decoder keeps them inside the Else body as continuation markers.
How the original engine runs these markers is open [hypothesis: an EndIf with nothing
open does nothing; a marker reached at the end of a taken branch ends the chain].

Opcode 0x127C (one call, OldWorldBlues SCPT 0x030154E8, source `GetInCharGen`) has no
function in the table (script-functions.md §4 anomaly). It decodes as a call with the
unknown opcode and value parameters.

### 2.7 Decoder report

`decode::Report` counts statements, calls and Raw pieces and lists *issues*:
`skip_mismatch`, `block_length_mismatch`, `chain_after_else`, `unmatched_else`,
`unmatched_elseif`, `unmatched_endif`, `unknown_opcode`, `raw_params`, `raw_token`,
`raw_statement`, `misplaced_scriptname`, `begin_inside_block`, `if_open_at_begin`,
`if_open_at_end`, `end_without_begin`, `statements_outside_blocks`, `unterminated_block`,
`unterminated_if`. `check::check` adds `ref_count_mismatch`, `compiled_size_mismatch`,
`duplicate_variable_index`, `undeclared_variable`, `variable_letter_mismatch`,
`ref_index_out_of_range`, `scrv_undeclared_variable`; `fnvtool script decode` adds
`remote_variable_unresolved`, `remote_variable_on_local_ref`,
`remote_variable_letter_mismatch`, `remote_variable_overridden`. Over the masters only the
kinds named in §1.2, §2.5 and §2.6 occur, with the counts given there.

### 2.8 The IR (contract with 5B)

`fnv_script::ir`, the decoded form the interpreter runs (docs/design/layer5.md §3.2, with
the additions marked *new*). Flattening it statement by statement gives back the compiled
statement sequence; nothing is dropped.

- `ScriptUnit` (`unit.rs`): `plugin`, `owner: UnitOwner` (§1.3: `Script{form}`,
  `QuestStage{form, stage, entry}`, `Info{form, end}`, `Package{form, event:
  Begin|End|Change}`, `Terminal{form, item}`, `Perk{form, entry}`, `Patrol{form}`,
  `Other{sig, form, ordinal}`), `kind: Object|Quest|Effect|Other(u16)`, `header` (the
  `SCHR` counters), `variables: Vec<VarDecl{index, name, ty: Int|FloatOrRef|Other}>`,
  `refs: Vec<RefSlot>` with `RefSlot::Form(global FormID)`, `Local(variable index)` and
  *new* `Unmapped(stored FormID)`, `bytes`, *new* `has_scda`, `source`.
- `Program { script_name: bool (new), blocks: Vec<Block>, top: Vec<Stmt> }`;
  `Block { event: u16, params: Option<Vec<Arg>>, body }` (`None` = no parameter block).
- `Stmt`: `Set { target: VarRef, expr }`; `If { arms: Vec<Arm{cond, body}>, otherwise:
  Option<Vec<Stmt>> }`; `Return`; `Call { call }`; *new* `Unmatched { marker:
  Else|ElseIf|EndIf, skip, cond }` (§2.6; a marker that is a direct child of an
  `otherwise` list continues that chain); *new* `Raw { opcode, body }` (an undecodable
  statement; none in the masters).
- `VarRef { on: Option<RefIndex>, var: VarSlot }`, `VarSlot::Local { index, letter:
  Short|Float }` (`s`/`f`) or `VarSlot::Global { global: RefIndex }` (`G`). `RefIndex` is
  the 1-based reference list index as stored.
- `Call { on: Option<RefIndex>, opcode, has_params (new: false = no parameter block),
  args }`.
- `Arg`: `Int(i32)`, `Float(f64)`, `Ref(RefIndex)`, `Var(VarRef)`, `Enum(u16)`,
  `String`, and *new* `Axis(char)`, `FormatVars(Vec<Arg>)`, `Int32(i32)` (the message
  trailer), `Raw(bytes)`.
- `Expr(Vec<Token>)`, postfix: `Num { value: f64, text }` (text as stored), `Var { var }`,
  *new* `Ref { r }` (`Z`), `Call { call }`, `Op { op }` (`Eq Ne Lt Le Gt Ge And Or Add
  Sub Mul Div Mod Neg`; `Neg` takes one operand), *new* `Raw { bytes }`.
- `resolve::ScriptIndex` (*new*): which script a form runs (`SCRI`, or base `NAME` →
  `SCRI`) and that script's variables, per `Viewer::Plugin(i)` or `Viewer::LoadOrder`
  (§2.5).

## 3. Canonical text

### 3.1 Purpose

The decoder is validated against an independent reading of the same unit: the oracle
builds the text from the shipped source (`SCTX`) and the reference list, never from the
bytes; `fnvtool script decompile` builds it from the decoded IR. Both print one line per
compiled statement, in compiled order, in the normalised form below. Text is compared for
units with both bytes and source (14,511).

### 3.2 Lines and atoms

Everything is lower-case except FormIDs.

| Statement | Line |
|---|---|
| ScriptName | `scriptname` |
| Begin | `begin <block>[ <arg>…]` — block name from the catalogue's `begin_blocks`, lower-case; unknown code `#<code>` |
| End, Else, EndIf, Return | `end`, `else`, `endif`, `return` |
| If, ElseIf | `if <expr>`, `elseif <expr>` |
| Set | `set <variable> to <expr>` |
| Call | `[<ref>.]<function>[ <arg>…]` |

Atoms:
- **function**: the catalogue name of the opcode (`functions.json` `name`), lower-case
  (`setav` prints `setactorvalue`); an opcode not in the table prints `#0x127c`.
- **reference list entry**: the load-order FormID `0x%08X` of an `SCRO`; the variable
  name of an `SCRV`.
- **local variable**: its declared name; **global**: the GLOB's FormID; **another
  script's variable**: `<FormID>.<name>`, the name as the unit's plugin sees the target
  script (§2.5); unresolved `<FormID>.<s|f><index>`.
- **number**: an integral value without a fraction (`3`, `-300`), anything else as the
  shortest decimal that reads back to the same double, never with an exponent (`19.5`,
  `0.002`). Expression literals are normalised the same way (`19.50` → `19.5`).
- **enum**: the shipped source's spelling of the code (encodings.json), as a word; a code
  with no known spelling `#<code>`. **axis**: `x`/`y`/`z`. **string**: as a word.
- **word**: lower-case; quoted with `"` when it contains anything but ASCII letters,
  digits and `_` (`"corpses eaten"`, `"vb01.bik"`).
- **message duration** (the i32 after ShowMessage's variables): printed only when not 0.
  The format variables print as ordinary arguments.

### 3.3 Expressions

Postfix, tokens joined by one space: atoms as above, operators as their symbols, unary
minus as `~`, a call as `[<ref>.]<function>(<arg> <arg>)` (`getstagedone(0x0011A2B3 10)`,
`getdead()`). From infix source the order follows this precedence, loosest first:
`&&`; `||`; `==` `!=`; `<` `>` `<=` `>=`; `+` `-`; `*` `/` `%`; unary minus. Binary
operators group left to right. **`||` binds tighter than `&&`** [observed: the source
`A && B || C` compiles to `A B C || &&` and `A || B && C` to `A B || C &&` wherever the
masters mix them without parentheses]; comparisons bind tighter than both [observed]. The
relative order of `==`/`!=` and `<`/`>` is as in C [hypothesis; no unit tells them
apart].

### 3.4 From source text (the oracle's rules)

How the compiler turns a source line into a statement, as far as the canonical text needs
it [observed: these rules reproduce the bytes for the units of §3.6's result]:
- Comments run from `;` (outside double quotes) to the end of the line. Blank lines,
  variable declarations (`short`, `int`, `long`, `float`, `ref`, `reference`) and **lines
  without any letter or digit** produce no statement. The last rule explains the ten
  units whose line count does not match (script-functions.md §3.2 and §7), one by one:
  FalloutNV SCPT 0x0014B67A (VHDLegionOliverRangerTrapTripwireScript), 0x00099D11
  (VHeliosStasisTriggerSCRIPT), 0x0005FCD0 (TrapMotionSensorSCRIPT), 0x00017E1B
  (TrapGenericTripwireSCRIPT), DeadMoney 0x0100C54C (NVDLC01TrapTripwireSCRIPT),
  0x0100A84A (NVDLC01TrapGenericTripwireSCRIPT), 0x0100496B (NVDLC01DogDialogueScript),
  HonestHearts 0x020114F4 (NVDLC02TrapGenericTripwireSCRIPT) each have one line of `=`
  signs between blocks; FalloutNV 0x000F81BC (VMS12QuestScript) has four lines of `-`;
  FalloutNV 0x001164A4 (VCG04ActivatorScript) has a line with two backquotes. With them
  dropped, every one of the ten lines up.
- Commas separate arguments like spaces. `Ref. Func` (space after the dot) is
  `Ref.Func`; `.Func` (nothing before the dot) is a call without a reference.
- Name lookup: a local variable of the unit; else the Editor ID of an `SCRO` entry of the
  unit (case-insensitive); `player` and `playerref` are FormID 0x00000014 (the reference
  list holds 0x14 wherever the source says either); an 8-digit hex number is a FormID
  written out (file-local, like a stored one) when the reference list holds it; a
  quoted string equal to an entry's Editor ID is that form (`AddNote "Megaton…Password"`).
  Anything else is a word (enum names, idle names, axis letters).
- In a parameter position that takes an actor value (catalogue kind `actor_value` or
  xEdit type `ptActorValue`) a name is always a word, never a form (`GetAV Lockpick` with
  the `Lockpick` item in the reference list).
- **The compiler ignores arguments beyond a function's parameters**: `StopCombat player`
  compiles with no parameter block. Without this rule 114 units differ (StopCombat 173
  lines, RemoveScriptPackage 125, StopLook 17, EvaluatePackage 5, ReleaseWeatherOverride
  2, GetFurnitureMarkerID, ApplyImageSpaceModifier, ResetAI). The oracle cuts arguments at the catalogue's
  documented parameter count; it cannot do this for functions whose wiki parameter list
  is incomplete (§3.6). A parameter list containing `...` (ShowMessage) is open-ended.
  Stray punctuation among arguments is dropped (`ShowMessage Msg :`).
- Begin block spellings: `OnPackageEnd` is the block the catalogue calls
  `OnPackageDone` (code 0x10) [observed].
- In expressions, a function takes as many following atoms as the catalogue documents
  parameters (all following atoms for the eight functions without a wiki page).

### 3.5 CLI contracts

`fnvtool script <cmd> <data_dir> <plugin>... --schema <schemas/fnv> [--functions <path>]`
(default `--functions`: `../script/functions.json` next to `--schema`). JSON on stdout,
exit codes as every fnvtool command (docs/cli-contract.md).

- `units` → `fnv.script.units/1`: `load_order`, `count`, `with_both`, `by_owner`
  (per record type: `units`, `with_scda`, `with_sctx`, `with_both`), `header_checks`
  (§1.2 counters), `read_issues` (`counts`, `samples`), `units` (`unit` key, `plugin`,
  `owner` {`owner`: script|quest_stage|info|package|terminal|perk|patrol|other, `form`,
  plus `stage`/`entry`/`end`/`event`/`item`}, `kind`, `has_scda`, `has_sctx`,
  `compiled_bytes`, `source_bytes`, `variables`, `refs`, `header`).
- `decode [--unit KEY | --form FORMID] [--all]` → `fnv.script.decode/1`: `summary`
  (`units`, `units_with_both`, `statements`, `raw`, `units_with_raw`,
  `remote_variables_unresolved`, `statements_by_kind`, `args_by_kind`,
  `tokens_by_kind`), `issues` and `checks` ([{`kind`, `count`, `samples`}], §2.7),
  `calls` (per opcode: `statement_calls`, `expression_calls`, `raw`), and `units`: the IR
  (serde form of `fnv_script::ir`) with `variables`, `refs`, `report` and `checks`, for
  the selected units (all with `--all`).
- `decompile [--unit KEY | --form FORMID]` → `fnv.script.decompile/1`: `load_order`,
  `count`, `units` ([{`unit`, `text`: [lines]}]) for every unit with bytes or source.
  The oracle writes the same format (`text` only for units with both).
- `coverage` → `fnv.script.coverage/1`: `summary` (`opcodes`, `calls`, `calls_decoded`,
  `implemented_opcodes`, `implemented_calls`) and `opcodes`: every opcode called by a
  script or used by a condition, ordered by `script_calls` + `condition_uses` from the
  catalogue, with `calls`, `decoded`, `raw`, `script_calls`, `condition_uses` and
  `implemented` (false until the interpreter exists).

### 3.6 `L5.script.decode`

Oracle `python tools/capture/oracle_script.py decompile <Data> <the ten masters>
--functions schemas/script/functions.json -o <goldens>/script/decode.json` (private
golden); candidate `fnvtool script decompile`; keyed comparator on `unit`.

Result 2026-10-09: **14,863 of 14,907 rows equal (score 0.99705)**, no missing or extra
unit. The 44 units that differ are cases where the source does not determine the bytes;
the decoder's reading is the bytes' [observed, each checked]:

| Units | Why source and bytes differ |
|---|---|
| 32 | The catalogue documents fewer parameters than the function takes, so the oracle cuts real arguments: ShowCaravanMenu (wiki: none; compiled with 3), SetNPCRadio (1; compiled with 2), StartCannibal, EnableLoadingMenu, Unlock (none; compiled with 1). A gap in `functions.json`, not in the decoder. |
| 7 | The form was renamed after the script was compiled; the reference list keeps the FormID, the source the old Editor ID: DeadMoney QUST 0x01013AA5 stage 21 (NVDLC01DeanPlayerChairREF), DeadMoney SCPT 0x0100636B (HologramStopPackage), FalloutNV REFR 0x001551B2 and 0x001551B3 patrols (TenofSpades/BetsyArrivalMarker), HonestHearts SCPT 0x0200B667 (NVDLC02SorrowMQ05REF), 0x0200F8EF (NVDLC02JoshuaGrahamLeaveForceGreetPackage), OldWorldBlues SCPT 0x0300D592 (NVDLC03SinkNukaColaMachineMsg). |
| 2 | Compiled against an older version of another script (§2.5): FalloutNV SCPT 0x00127959, LonesomeRoad SCPT 0x0400AC93. |
| 1 | Opcode 0x127C where the source says GetInCharGen (§2.6): OldWorldBlues SCPT 0x030154E8. |
| 1 | An argument to a Begin block that takes none (`begin ScriptEffectStart EncHArchimedesEffect`, FalloutNV SCPT 0x0011F676), dropped by the compiler. |
| 1 | A fourth argument to AddItem after a quoted `"*"` (FalloutNV INFO 0x0014DDD7 end); the compiler kept two. |

Three units have source but were never compiled (no `SCDA`, `compiled_size` 0 and a
`ref_count` larger than their reference list): FalloutNV INFO 0x0015AD0D end, DeadMoney
TERM 0x0100923D item 0, OldWorldBlues TERM 0x03004902 item 2. They have no text to
compare (§3.1).

## 4. The interpreter (`fnv_script::vm`, workstream 5B)

The VM runs the IR of §2.8 against a `Host` trait and knows no game rules. The host
(`fnv-game`, §5–8) says where variables live, what a reference list entry names and what a
function does. Tests run the VM in strict mode; the game runs it in lenient mode.

### 4.1 Values and storage

- A value is a number (`f64`) or a reference (a load-order FormID, 0 = none).
- **Float variables keep doubles** [observed: `L5.quest.var_storage`; 16777217, 0.1,
  123456789.123 and 3000000000.5 read back unchanged; layer5-captures.md §5.5].
- **Integer variables (`short`, `long`, `int`) truncate toward zero to 32 bits; out of
  range (or NaN) stores −2147483648** [observed: 2.7 → 2, −2.7 → −2, 40000 → 40000 (no
  16-bit wrap), 2147483647.5 → 2147483647, ±3000000000.5 → −2147483648]. The storage type
  is the target script's `SLSD` declaration; the bytecode letter (`s`/`f`) decides only
  when the target does not declare the index (§4.6).
- **Number literals**: a literal written without a decimal point is read as an integer
  that saturates at 2147483647; one with a point is a double; the compiler writes a
  negative literal as the positive one and `~` [observed: `3000000000` stores
  2147483647 in float and integer variables alike; `-3000000000` gives −2147483647].
  `vm::literal` re-reads the literal's stored text (§2.3), not its parsed value.
- A reference read as a number is its FormID; a whole positive number used where a
  reference is expected is taken as a FormID [hypothesis].

### 4.2 Expressions and statements

- Postfix evaluation with a stack, left to right (§2.3); the compiler already applied the
  precedence (§3.3). One value must remain, else the script stops with an error. The
  original does the same with two left [observed 2026-10-30, capture `o4_00_probe`, issue
  #908]: three INFO begin scripts of FalloutNV.esm compile GetStage with an extra
  argument (`GetStage Q 100 == 1`, i.e. GetStage(Q), 100, 1, `==`: 0x001588E5, 0x001588E6
  of VMQYesMan01a and 0x00160731 of VMQYesMan02, Side Bets). The same text compiled in
  game (`if GetStage <quest> N == 1` and `!= 1`, N 100 or 1, on five quests at stages 0, 5
  and 10) failed the script call at the `if` every time: the statement before it had run
  and neither arm's did, while `if GetStage <quest> == 0` ran its arm. So those lines' own
  SetStage / SetObjectiveDisplayed never run in the original either. That the in-game
  compiler makes the same bytes as the GECK for this text is a [hypothesis].
- Comparisons give 1 or 0; `&&` and `||` test for non-zero; arithmetic is in double
  precision; `/` by 0 gives 0; `%` is the remainder of both operands truncated to
  integers, 0 for a zero divisor [hypothesis, all of this line].
- **`&&` and `||` do not short-circuit**: both operands are evaluated, calls included
  [observed: o5_01, 2026-10-31, issue #1112: `if 0 && player.ModAV Variable09 10`,
  `if 1 || …`, `if 0 == 1 && … == 0` and five more forms all ran the ModAV;
  observer-checks.md check 51; script-language.md §4]. Only the extender's `eval`
  expressions short-circuit [documented: Eval]. So a script that guards a call on a
  reference with `&&` (EDEScript) makes the call on an empty reference every frame in the
  original too.
- `If`/`ElseIf`/`Else`: the first true arm runs, else the `Else` body.
- `Return` ends the whole run (block or result script), from any depth.
- **Stray markers** (§2.6). An `EndIf` with nothing open does nothing [observed]. An
  `ElseIf` reached after its chain was closed **tests its condition and runs its body
  when true**, as a new `If` would: the statements up to the next stray marker of the
  list are its body, a following stray `ElseIf`/`Else` continues that chain and the next
  stray `EndIf` closes it [observed: BeatrixFX's quest script (FalloutNV SCPT
  0x0017B778) stepped `bump` 0 → 7 through its six stray ElseIf branches,
  `L5.quest.stray_elseif`; the dead Techatticup hostage (SCPT 0x00123191) activated
  with the run-OnActivate flag opened its loot menu through `elseif GetDead == 1 /
  Activate`, `L5.quest.dead_activation`; layer5-captures.md §8.3 c]. Whether a branch
  taken just before the marker suppresses it was not exercised [hypothesis: it does
  not, the chain starts fresh]. A stray `Else` (FalloutNV 0x000EAB00) is read the same
  way: a chain whose only branch runs [hypothesis]. A marker inside an `Else` body (a
  later `ElseIf`/`Else` of the same chain, §2.6) ends the chain when reached: one branch
  of the chain has run [hypothesis: the engine keeps a "branch taken" state per open
  chain that `Else` sets and `EndIf` clears, which also explains the observed stray
  ElseIf].
- **Functions that end the script**: `RemoveMe` "acts like a return" [documented:
  catalogue notes]; the host asks the VM to stop after the call (`Host::take_return`).
- **Off-by-one skip counts** (§2.6, six units): the structured reading runs, which per
  §2.6 runs the same statements as a jump by the stored count [hypothesis, from 5A].
- Calls: the explicit reference (`Ref.Func`, the `0x001C` prefix or `r` before `X`) is
  resolved first, then every argument (references through the reference list; `SCRV`
  entries read the local variable), then `Host::call`. **A call on an empty reference
  variable stops the script for this run**, silently in the game [documented: GECK wiki
  Ref Variable, Eval and Let pages, owner export 2026-10-09; docs/spec-notes/script-language.md §7.3 row
  1]; here it is traced as `call_failed`, once per (script record, function): a record's
  script runs every frame, so a repeated failure would trace every frame (#1112); result,
  stage and console scripts trace every time [design]. A `Set` whose expression stopped the script
  leaves its variable unchanged [documented: Let, "a failed set stops the script"].
  Until round 4 the call read 0 and the script went on. In the original with xNVSE 6.4.9,
  a function script run through CallFunction **went on** after `rE.GetDisabled` on an
  unset `ref`, alone and as an `if` condition (the arm not taken) [observed: o5_00,
  2026-10-31; observer-checks.md check 14]; whether a quest or object script run by the
  game does the same is not captured.
- **Return ends the script for this run**, not just its block: a later block of the same
  event does not run [documented: GECK wiki Adding an Options Menu, owner export 2026-10-09; script-language.md
  §7.3 row 2, which lists the five shipped units where this can matter].
- **Unknown opcodes** (not in `functions.json`; one in the masters, 0x127C, §2.6) and
  `Raw` pieces: strict runs fail; lenient runs skip the call (value 0) and trace
  `skipped`.
- **Step budget**: each statement and call counts; a run stops after
  `RunOpts::max_steps` (100,000) with a `script_error`. Scripts have no loops, so only a
  runaway chain through the host (a stage result setting stages) can reach it. Nested
  runs (SetStage inside a script, Activate with the block flag) are limited to 256 levels
  [documented: GECK wiki SetStage, at least 250 recursive SetStage calls work].

### 4.3 Variables of other scripts (§2.5)

`ref.var` addresses the instance of the script the form runs: a quest's script, or the
script of a reference's base form. An index the target script does not declare (the two
units of §2.5 compiled against older scripts) reads 0 and is written as given; each such
access is traced as `undeclared_variable` [hypothesis: the engine uses the index as
stored].

## 5. Quests (`fnv_game::quest`, `game.rs`)

Read from the winning `QUST`: `DATA` flags, priority and script delay; quest conditions;
stages (`INDX`), each with entries (`QSDT` flags, `CTDA` conditions, `CNAM` log text,
the result script, `NAM0`); objectives (`QOBJ`, `NNAM`, targets `QSTA` with conditions)
[documented: xEdit; schema `gameplay.json`].

Run time: running, completed, failed, stages done, objectives displayed/completed, the
time since the script last ran, a `SetQuestDelay` override. Behaviour, all [observed]
in the 5C captures (layer5-captures.md §5) unless tagged:

- **GetStage is the highest stage done** (0 when none); a lower stage set later is still
  marked done and runs its result script (VMS16 50 then 10 and 5; Generic 70 then 60).
  [documented: GECK wiki GetStage]
- **SetStage** of a stage the quest does not define does nothing (traced
  `stage_ignored`); of a stage already done does nothing unless the quest has "Allow
  repeated stages" (VMS16 and VCG03 did not rerun their scripts; Generic did); starts a
  stopped quest; marks the stage done; then runs the result script of **every entry
  whose conditions pass**, immediately, nested in the caller (the job script read the
  result script's variable on the next line) [documented: GECK wiki SetStage]. **All
  passing entries run, in record order** [observed: the five entries of ControlRumble's
  stage 0 all ran, the last one's StopQuest last, `L5.quest.multi_entry_stage`,
  layer5-captures.md §8.3 a]; every entry's conditions are evaluated before the first
  script runs [hypothesis; no stage in the masters can tell]. The entry's Complete /
  Fail flag is applied after its script [hypothesis on the order].
- **Complete and Fail Quest flags both set completed** (GetQuestCompleted 1) and do not
  stop the quest; only a script's StopQuest does (VMS22, VMS16 stage 110).
- **SetObjectiveDisplayed 1 on a hidden objective clears its completed flag;
  SetObjectiveCompleted does not display** (VMS16 objectives 5 and 10). On an objective
  already displayed it **keeps** the flag [observed: o5_00, 2026-10-31, VMS19 65 and
  VFSAtomicPimp 34 displayed, completed, displayed again: still completed; hidden with 0
  and shown again: not completed; layer5-captures.md §5.2]; `Game::set_objective` does the
  same (issue #1108). Only objectives the quest defines
  change; displaying one starts a stopped quest [documented: GECK wiki
  SetObjectiveDisplayed; also Quest scripts and Quest Data Tab, owner export 2026-10-09: SetStage, StartQuest
  and SetObjectiveDisplayed start a quest].
- **A stopped quest keeps its variables**, readable and writable [documented: GECK wiki
  Quest scripts, owner export 2026-10-09]: `quest.var` reaches the instance whether or not the quest runs.
- **Complete Quest with a Next quest** makes that quest the player's active quest
  [documented: GECK wiki Quest Stages Tab, owner export 2026-10-09]. Not modelled (the active quest is the
  Pip-Boy's, Layer 6); `StageEntry::next_quest` is read.
- **Script delay: the `DATA` delay whenever it is non-zero, whatever the "Default Script
  Processing Delay" flag says; 0 means 5 s; a delay shorter than a frame runs every
  frame** (VMQ03aTimer 0.5 s, CGTutorial 1.0 s, VCG01 and VCG00 0.01 s, VMS40Timer 0 → 5 s)
  [also documented: GECK wiki SetQuestDelay]. The wiki's Quest Data Tab and Quest scripts
  pages (owner export 2026-10-09) say the flag means the INI's 5 s whatever the field; the captures show the
  field used whenever it is non-zero, and the observed rule is kept
  (docs/spec-notes/quests-dialogue.md §5). A stopped quest's script does not run.
  The first run after StartQuest is not modelled (it was irregular in game; the
  scenarios skip it): the counter starts at 0 on StartQuest. [observed: VMS40Timer's first
  run came about 3.9 s after it; NVDLC03MQ01 (delay 0) first ran 98-99 game loops, about
  1.65 s, after a console StartQuest, in 4 launches of 4, and not at all when set to 100
  on the next loop, observer check 43, 2026-11-26, #1113.]
- **ResetQuest keeps a SetQuestDelay override** and zeroes the variables [observed: o41,
  VMQ03aTimer, layer5-captures.md §5.7]; `reset_quest` keeps it (issue #1109).
- **ResetQuest from another script stops the quest** [observed: o41, called from a job
  script: GetQuestRunning 0 after it and no GameMode run until StartQuest,
  layer5-captures.md §5.7]; `reset_quest` stops it (issue #1232). A reset from the
  quest's own script or stage result leaves it running [hypothesis, not observed: the
  Emergency Radio, VEFR01NCRGood2EmergencyRadio, resets itself in its MenuMode and goes
  on to set up the supply drop; observer check 66].
- **GetSecondsPassed** returns the game time between the start of the instance's
  previous per-frame run (GameMode, ScriptEffectUpdate) and the start of this one, read
  from a clock with 1/64 s resolution [observed: every drop is a multiple of 0.015625 s;
  hypothesis that the clock, not the frame, is quantised], 0 on the first run, and the
  same value for every call in one run [documented: GECK wiki GetSecondsPassed]. It does
  **not** depend on when the script last called it [observed: VCG01's countdowns,
  restarted by a stage after a pause in which the script had not called it (the
  countdown before had run out, or Doc's lines ran), came at 0.95–1.0 × their fTimer:
  7 at 3.22 s of 3.25, 70 at 0.96 of 1.0, 95, 100 and 105 at 0.96–0.98 of 1.0,
  layer5-captures.md §10.7; `Game::mark_run`]. Round 3 counted the time since the last
  call, which made the walk-through's 85 → 90 countdown expire at once; that rule is
  gone. **A trigger's OnTrigger** is its per-frame run when the script has no GameMode
  block (`Game::trigger_frame`); it runs once for each actor inside, and only the
  first run of a frame gets the frame's time, the later ones 0 (`Game::trigger_again`)
  [documented: the GECK wiki's "time since the script last ran"; hypothesis: that the
  original also runs it once per reference inside]. No trigger block runs while the game
  is paused (a menu, a message box, a conversation, a loading screen; nvf-play
  `script.rs`) [hypothesis: OnTrigger is held as GameMode is]. Both made Hidden Valley's
  virus purge (HVHuntingVirusTrigger, SCPT 0015A154, `Begin OnTrigger` with no actor,
  20 s by GetSecondsPassed) run out in a fraction of its time, with Ibsen and two scribes
  in the room and under message boxes (issue #2455; smoke `hv-virus-purge-timer`).
  **Menus and loading screens do not count**: the game time
  (`Game::time`) and the GameMode quest delay stop while a menu is open or a loading
  screen is up [observed: after a 900-frame container menu the first drop was one frame
  for VCG00 and VMQ03aTimer's own 0.5 s, `L5.quest.seconds_after_menu`; a `coc` loading
  screen was not counted either; layer5-captures.md §8.3 e; a quest timer and an object
  script's stopwatch stood still in the name menu, §10.1]. A Wait-menu gap is not
  captured.
- **New game**: every quest flagged "Start game enabled" runs, with no stage set
  [documented: GECK wiki Quest Data Tab, owner export 2026-10-09]; VCG00 is the quest the New Game menu starts
  [documented: GECK wiki SetStage notes]; the runner starts it as an input, or (round 4)
  starts from the facts of the new-game save `nvfnew` (§8, §9).

## 6. Conditions (`fnv_game::cond`)

- Layout from `common.json` (`Condition`): type byte, comparison value, function index,
  two parameters, run on, reference [documented: xEdit].
- Type byte: comparison in the top three bits (0 `==`, 1 `!=`, 2 `>`, 3 `>=`, 4 `<`,
  5 `<=`); flag 0x01 OR, 0x02 run on target, 0x04 the comparison value is a global
  [documented: xEdit].
- Parameters are typed by the function's xEdit parameter types (`xedit_param_types` in
  `functions.json`): record types are FormIDs mapped through the plugin's masters;
  `ptInteger`, `ptQuestStage`, `ptQuestObjective`, `ptVariableName` (a variable index)
  and `ptVATSValueParam` are integers; actor values, sex, axis, misc stats, menu modes and
  the other enumerations are codes [documented: xEdit types; the reading is ours].
- Run on: Subject, Target, Reference (the stored reference), Combat Target (none until
  Layer 6), Linked Reference (the subject's `XLKR`) [documented: xEdit].
- Chaining: a condition with the OR flag joins the next; the OR groups are joined with
  AND, so **OR binds tighter than AND** [observed: GoodspringsFleePackage's `A OR B, C`
  and SunnySmilesStayAtCurrentLocation's `D, Q, S45 OR S40 OR S20` chose as `(A OR B)
  AND C` and `D AND Q AND (S45 OR S40 OR S20)`, neither left to right nor AND first,
  `L5.quest.condition_groups`, layer5-captures.md §8.3 b; also documented: GECK wiki
  Category:Conditions, owner export 2026-10-09]; it matches `||` binding tighter than `&&` in compiled
  expressions (§3.3).
- Subjects: quest stage conditions run on the player [hypothesis]; the wiki says "Run on
  Target" and most reference functions are invalid on stages and quest targets, since a
  stage is not a reference [documented: GECK wiki Quest Stages Tab and Quest Objectives
  Tab, owner export 2026-10-09; docs/spec-notes/quests-dialogue.md §2], which leaves
  the subject of the valid ones open. A dialogue line's run on its speaker, with the
  listener (the player, or SayTo's target) as target [documented: Category:Conditions:
  Subject is the speaker, Target the listener]; an AI package's on its actor (§7.5); an
  effect's on the effect's target [documented: same page].
- Evaluation calls the same implementations as scripts (index = opcode − 0x1000). Every
  condition function the intro chain's quests and dialogue use (248 conditions) is
  implemented. An unhandled one reads 0.

## 7. Instances, events and functions

### 7.1 Script instances

One instance per scripted reference (its base form's `SCRI`; for an actor whose base has
the template flag "Script", its template's, through a leveled list the reference's pick
[documented: GECK wiki Template Data, "Use Script: Uses the Script of the ActorBase"]. An
actor scripted only through its template gets the lifecycle blocks OnLoad and OnReset
through the lifecycle index [design, #3228]. Existing per-frame and directed events
continue to read the script through `Game::ref_script`. The current pick decides which
script runs, and a reset whose new pick runs another script starts a fresh instance,
`GameData::script_through_template`, dead-on-spawn.md §4.1, #3228), per running quest's script,
and per scripted item in an inventory (holder, item). Instances are created on first use
(an event that runs the script, or a variable access): an instance never touched has all
variables 0, so this is the same state as creating it when the reference is materialised.
`Game::snapshot` (`fnv.game.state/1`) carries the instances, quest states, the extra
per-reference state (dead, destroyed, factions, actor value overrides, modifiers and
damage, script packages), the player state (XP, perks, notes, reputation, tag skills, controls,
misc stats), run-time form-list additions, loaded cells, menu and message box, clock and
the RNG position, on top of the Layer 4 snapshot; a restored game continues exactly as
the original (test `snapshots_round_trip`).

### 7.2 Dispatch

| Event | When (here) | Tag |
|---|---|---|
| GameMode | every frame: the OnLoad blocks due (below), then running quests whose delay has passed (FormID order), then scripted references in loaded cells, disabled ones included (FormID order), then the scripted items the player carries (FormID order), then active magic effects (§7.6), then dialogue lines (§7.7). A menu opened by one of these scripts holds the ones after it until it closes (§7.7) | quest scripts before object scripts [observed for one pair: BeatrixFX's GameMode moved GameHour on and two Goodsprings windows (WastelandWindowScript, copying GetCurrentTime every frame) saw the new hour in the same frame, `L5.quest.script_order`, layer5-captures.md §9.3 c]; every other part of the order [hypothesis]; delays [observed]; carried items [observed: JarPistolPowder, `L5.quest.onadd_gamemode`; other holders not modelled, hypothesis: the wiki's Scripting for Beginners page says an item's script runs while its holder is loaded, owner export 2026-10-09]; disabled references [documented: GECK wiki Object scripts, owner export 2026-10-09] |
| MenuMode | instead of GameMode while a menu is open (`Game::menu`, a message box included); a block's menu code must match; quests run it on their delay, counted in menu time only | VCG01's `MenuMode 1036` set stage 40 while the race menu was open [observed: layer5-captures.md §10.3]; the delay counter [hypothesis] |
| OnActivate | an activation by hand runs the block *instead of* the default action, inside the activation; the default runs only if the block calls `Activate`; a scripted `Activate` (flag 0) runs the default and never the block; with flag 1 the block runs instead, or the default when the target has no block; the block runs on dead actors too; a block parameter (`OnActivate player`) must match the activator | [observed: `L5.quest.ref_activation`, `.dead_activation`, layer5-captures.md §8.3 d, f; documented: GECK wiki Activate] |
| OnLoad | scripted references in a cell when it becomes loaded (the player's cell, plus the 5×5 grid outdoors), at once, disabled ones included; a reference enabled in a loaded cell at the start of the next frame, not inside Enable (again, for one that ran it disabled at the cell load) | cell load [hypothesis]; disabled ones at the cell load [observed: VMS15's rising plane had run its OnLoad while disabled, read on the line after its Enable, layer5-captures.md §8.3 d; that it was the cell load and not Enable is the narrowest reading, hypothesis]; after Enable [observed: jukebox and broken radio, layer5-captures.md §8.3 d] |
| OnAdd / OnEquip / OnUnequip | AddItem (each item of a form list), pick-up, EquipItem / UnequipItem, on the item's script, inside the call; the item's GameMode follows by the next frame | [documented: AddItem's OnAdd; per call, not per item, hypothesis; observed: OnAdd then GameMode by the next frame, `L5.quest.onadd_gamemode`] |
| OnDeath | KillActor | [hypothesis: at once] |
| SayToDone | when a line said by SayTo or Say ends, after its end result script (§7.7); also the runner input `say_to_done` | after Say too [documented: GECK tutorials, level-design.md §11.2 D1]; the order [hypothesis] |
| OnTriggerEnter / OnTrigger / OnTriggerLeave | runner inputs (`trigger_enter`); in `nvforever --play` any actor's capsule against the volume (docs/design/layer5-play.md §3) | who and how [documented: level-design.md §11.2 T1–T3]; Enter before OnTrigger on the entering frame [hypothesis] |
| OnPackageStart/Done/Change | runner inputs until Layer 6 | — |

A dead actor's default activation opens its inventory (the loot menu) [observed:
`L5.quest.dead_activation`]. A scripted item's instance is per holder and item;
`RemoveMe` drops it when the holder has none left.

**Engine API (workstream 5D).** The engine drives `fnv_game::Game` with three pieces of
state and one call per frame:
- `game.menu = Some(code)` while a menu is open (`None` when closed). MenuMode runs
  instead of GameMode, and game time stops for scripts: `Game::time`, GetSecondsPassed
  and the GameMode quest delay do not advance (a message box counts as a menu).
- `game.loading = true` while a loading screen is up (a door to another cell, `coc`).
  `Game::tick` then only counts the frame: no script runs and game time stops.
  `Game::paused()` is true in either case.
- `game.cfg.frame_seconds = dt; game.tick()` once per frame.
- `Game::note_enabled(r)` is called by Enable itself; OnLoad of a reference enabled in a
  loaded cell runs at the start of the next tick (`Game::pending_onload`), so the engine
  needs no OnLoad handling of its own for Enable.
- `game.message_box.pressed = Some(button)` when the player picks a button. Only the
  script that showed the box (`MessageBox::owner`: its instance, or the unit of a result
  script) receives it from GetButtonPressed; every other script reads −1 and the box
  stays [hypothesis, from the GECK wiki's ShowMessage and GetButtonPressed pages; with a
  global button the `--play` integration saw Doc Mitchell's chemistry set take every
  box's button]. A message box without buttons shows a default OK [documented: GECK
  wiki ShowMessage notes] and has a `message_box` too (`buttons: 0`): its OK is button 0,
  read once by the script that showed it [hypothesis: inferred from SCPT 0x000E9483,
  Black Mountain's Broken Mr. Handy, which waits for button 0 after such a box, #2925;
  observed 2026-11-25, observer check 240: OK on BMRhondaFixMsg repairs it]. It does not replace a box with buttons still waiting for the
  player [design, #2925]. A corner message (no Message Box flag) has none: −1.
- Round 4 (§7.7): scripts now open menus themselves (`Game::open_menu`: a message box
  is 1001, the name, race, tag-skill, trait and S.P.E.C.I.A.L. menus have their codes),
  and boxes wait in `Game::boxes`. The engine should show `game.menu` and call
  `game.close_menu()` when the player closes it (OK / Done) and `game.press_button(b)` for
  a button, instead of overwriting `game.menu` every frame. `nvforever --play` does so
  since 5D round 2 (docs/design/layer5-play.md §2): it sets `game.message_box.pressed`
  before the frame's tick and calls `close_menu` after it, so a `MenuMode 1001` block
  still reads the button once (with `press_button` the box would close before MenuMode
  ran).
  `GameConfig::modal_message_boxes` (default on) switches boxes back to notifications.
  Dialogue lines advance in `tick` (`Game::speech`); the engine can play their voice
  files from the `line_started` events.
These replace the engine-side handling `nvf-play` (PR #62) has for "no script time in
menus, message boxes and the frames after a door load" and "OnLoad the frame after
Enable". Until it switches: an OnLoad the engine runs itself (`ref_event(r, ON_LOAD)`)
clears the one Enable queued, so OnLoad never runs twice; and setting `frame_seconds`
to 0 in menus still stops game time, but also stops MenuMode's quest delay counter, so
quest MenuMode blocks with a delay never run (keeping `dt` and setting `menu` avoids
that). The frames right after a door's cell load are the engine's to mark with
`loading` (the captures saw about 22 frames without script runs after a `coc`).

Disabled references run their per-frame blocks [documented: GECK wiki Object scripts,
owner export 2026-10-09; observed for GameMode: capture job `od_09a_disabled_ref_script`,
2026-10-18, the skull's timer fell at its enabled rate while disabled,
owner-decision-captures.md; round 3 had them stopped, a hypothesis]; deleted ones do not.
OnLoad at a cell load runs for disabled references too (§7.2 OnLoad row; it was "needs
the reference enabled" until 2026-10-24, a hypothesis the rising plane refuted).
Return ends the script for the run, later blocks of the same event included (§4.2).

### 7.3 Functions

`fnv_game::functions::registry()` lists every handled opcode with its catalogue name (a
test checks the name against `functions.json`). Dispatch is by opcode only.

- **Implemented (133 in round 2; 225 after round 3; 232 after round 4)**: quests (SetStage, GetStage, GetStageDone, Start/StopQuest,
  GetQuestRunning, GetQuestCompleted, CompleteQuest, Set/GetObjectiveDisplayed,
  Set/GetObjectiveCompleted, GetQuestVariable, GetScriptVariable, SetQuestDelay);
  references (Enable, Disable, GetDisabled, MoveTo, MoveToMarkerWithFade, GetSelf,
  GetIsID, GetIsReference, GetInCell, GetInWorldspace, GetInSameCell, IsInInterior,
  GetDistance, Get/SetPos, Get/SetAngle, GetLinkedRef, GetParentRef, MarkForDelete,
  Set/GetDestroyed, Lock, Unlock, GetLocked, Activate, GetActionRef, IsActionRef, IsActor,
  GetIsCreature, IsInList, AddFormToFormList, Set/GetOpenState, EnterTrigger); items
  (AddItem, RemoveItem, GetItemCount, RemoveAllItems, EquipItem, UnequipItem,
  GetEquipped, GetWeaponHealthPerc, RemoveMe); AI packages (EvaluatePackage,
  GetIsCurrentPackage, §7.5); actors (Get/Set/Mod/Force/Damage/RestoreActorValue,
  GetBaseActorValue, GetPermanentActorValue, GetDead, KillActor, ResurrectActor,
  GetDeadCount, GetInFaction, AddToFaction, RemoveFromFaction, Get/SetFactionRank,
  GetIsVoiceType, GetIsSex, GetPCIsSex, SexChange, GetIsRace, GetIsClass, SetGhost,
  Set/GetRestrained, Set/GetUnconscious, ResetHealth, GetHealthPercentage,
  Set/GetPlayerTeammate, AddScriptPackage, RemoveScriptPackage); the player (RewardXP,
  ResetXP, HasPerk, AddPerk, RemovePerk, GetHasNote, AddNote, RemoveNote, IsHardcore,
  SetHardcore, IsPlayerTagSkill, SetPlayerTagSkill, AddAchievement,
  Disable/EnablePlayerControls, GetPlayerControlsDisabled, GetLevel, Get/SetReputation,
  RewardKarma, Get/ModPCMiscStat, ShowMap, GetMapMarkerVisible); messages and misc
  (ShowMessage, GetButtonPressed, ShowWarning, MenuMode, GetRandomPercent,
  GetSecondsPassed, GetGlobalValue, GetCurrentTime, GetGameSetting, Abs); and four Layer 6
  states that read "no" without Layer 6 (IsInCombat; IsTalking until round 4) or the PC platform
  (IsXBox, IsPS3) [decision].
- **Added in round 3 (92, 2026-10-09; rules in §7.6)**: reputations
  (GetReputationThreshold, Add/RemoveReputation, AddReputationExact, GetReputationPct);
  objects (PlaceAtMe, PlaceLeveledActorAtMe, DropMe, Drop, Set/ClearOwnership, IsOwner,
  SetCellOwnership, SetCellPublicFlag, DamageObject, GetDestructionStage,
  ClearDestruction, IsPlayerInRegion, PlayerInRegion, GetHeadingAngle, HasLoaded3D,
  Get/SetScale, GetLockLevel, GetInCellParam, GetContainer, SetMinimalUse,
  Set/IsInCriticalStage, Set/GetBroadcastState, SetTalkingActivatorActor); inventories
  (RemoveAllTypedItems, AddItemHealthPercent, Mod/SetWeaponHealthPerc, IsWeaponInList,
  IsWeaponSkillType, ResetInventory, GetContainerInventoryCount, SetItemValue,
  SetQuestObject); magic (AddSpell, RemoveSpell, CastImmediateOnSelf, Dispel,
  DispelAllSpells, IsSpellTarget, HasMagicEffect, ScriptEffectElapsedSeconds); actors
  and factions (IsChild, GetIsCreatureType, SetEssential, SetActorRefEssential,
  IsEssential, IsActorRefEssential, SetActorFullName, IsKiller, GetFactionRelation,
  GetFactionCombatReaction, SetEnemy, SetAlly, RemoveFromAllFactions,
  GetFactionRankDifference, Set/GetPCEnemyOfFaction, ClearFactionPlayerEnemyFlag,
  GetActorFactionPlayerEnemy, IgnoreCrime, GetIgnoreCrime, Set/GetIgnoreFriendlyHits,
  GetIsGhost, SetLevel, GetPlayerTeammateCount); the player (EnableFastTravel,
  UnlockChallenge, GetChallengeCompleted, RemoveRecurringFromChallenge,
  Set/GetInCharGen, GetCasinoWinningsLevel, GetCasinoWinningStage,
  SetPCCanUsePowerArmor, AddTopic, IsAlwaysHardcore); quests (CompleteAllObjectives,
  ResetQuest); GetDayOfWeek, Sqrt, IsWin32 (1, the PC version) and IsGoreDisabled (0,
  the default) [decision]. Seven of them were stubs before (SetEnemy, SetAlly,
  IgnoreCrime, SetIgnoreFriendlyHits, SetScale, CastImmediateOnSelf, RemoveSpell).
- **Added in round 4 (7, §7.7)**: SayTo, Say (dialogue lines), IsTalking (a line in
  progress), and the menus GetPlayerName, ShowRaceMenu, SetTagSkills, ShowTraitMenu,
  ShowLoveTesterMenuParams (each opens its menu). All seven were stubs before.
- **Added with dialogue (3, 2026-10-10, docs/spec-notes/dialogue.md §6)**:
  StartConversation (with the player as target: the conversation starts at once),
  GetTalkedToPC, GetDialogueEmotion. All three were stubs before; they are still named
  in the stub list below as it stood after round 4.
- **Furniture (2026-10-10)**: GetSitting, GetSleeping, GetFurnitureMarkerID,
  IsCurrentFurnitureRef and IsCurrentFurnitureObj answer from the use set on the actor
  (`Game::set_furniture_use`, docs/spec-notes/furniture.md §2–3); 0 for an actor on no
  furniture, as the stubs gave. All five are still named in the stub list below as it
  stood after round 4.
- **Image space modifiers (2026-10-10)**: ApplyImageSpaceModifier,
  RemoveImageSpaceModifier and IsImageSpaceActive keep the modifiers on the screen in
  `Game::image_modifiers` (docs/spec-notes/image-modifiers.md §3: an animatable one
  ends by itself after its duration, the others stay until removed); trace events
  `image_modifier_applied` and `image_modifier_removed`. They were stubs before and
  are still covered by "image space" in the stub list below as it stood after round
  4; the screen splatter functions and SetCellImageSpace remain stubs.
- **Layer 6 stubs (170 after round 3; 163 after round 4)**, traced as `stub` with
  their arguments and no effect; `functions::registry()` names each one's system. From
  round 2: AI packages (ResetAI, GetCurrentAIPackage, GetCurrentAIProcedure, SetAlert,
  GetFurnitureMarkerID, IsCurrentFurnitureRef), dialogue (StartConversation,
  GetTalkedToPC, SetDisposition), combat (StartCombat, StopCombat,
  GetHitLocation, GetShouldAttack, IsWeaponOut, GetWeaponAnimType, GetVATSValue), crime
  (SendTrespassAlarm), animation (PlayGroup, PlayIdle, Look, StopLook), appearance
  (AgeRace), sound and video, image space, weather, SetGlobalTimeMultiplier, rendering
  (SwapTexture, SetActorAlpha, PlayMagicShaderVisuals), HUD and Pip-Boy, menus,
  Autosave (a recorded no-op then; since built, with SystemSave and ForceSave:
  script-saves-and-reticle.md), IncrementScriptedChallenge, IsSneaking. Round 3 adds 117: animation
  (IsAnimPlaying, GetAnimAction, IsIdlePlaying, PickIdle, IsLeftUp, IsFacingUp,
  Rotate), AI (GetSitting, IsCurrentFurnitureObj, GetSleeping, GetIsAlerted, ForceFlee,
  SetPackDuration, the used-item checks, the Cannibal and Mister Sandman perk checks),
  combat (StopCombatAlarmOnActor, SetCombatStyle, group counts, GetCombatTarget,
  FireWeapon, UseWeapon, GetKnockedState, IsLimbGone, VATS, explosives), detection
  (GetDetected, GetLineOfSight, SetAllVisible, CreateDetectionEvent), crime
  (SendAssaultAlarm, GetTrespassWarningLevel, IsTrespassing, GetAlarmed,
  GetMajorCrimeCount, GetOwnerLastTarget), dialogue (IsActorTalkingThroughActivator,
  GetDialogueEmotion, GetDisposition, Exists), movement (IsMoving, IsTurning,
  IsRunning, IsSwimming, SetForceSneak), physics (PushActorAway, IsPlayerGrabbedRef,
  trapUpdate), navigation, rendering and image space (StopMagicShaderVisuals,
  SetSecuritronExpression, Reset3DState, AttachAshPile, SetCellImageSpace), weather
  (SetWeather, IsRaining, GetIsCurrentWeather), GetRadiationLevel, radio, menus
  (ForceTerminalBack, ShowCaravanMenu, OpenTeammateContainer, Add/SetSPECIALPoints,
  AddTagSkills, the repair, recipe, casino, barber, surgeon, tutorial and loading
  menus), ShowAllMapMarkers (map markers are not read), GetIsLockBroken
  (lockpicking), saving, ExitGame. AddScriptPackage and RemoveScriptPackage keep the
  actor's package list and also trace a stub.
- **Unhandled (5 used opcodes, 35 uses)**: AddItemToLeveledList (20: run-time leveled
  list additions belong in `fnv-sim`'s leveled resolution, Layer 4), IsDLCInstalled (8:
  no wiki page, behaviour undocumented), GetIsAlignment (3: NPC alignment is not read),
  GetXPForNextLevel (3: the experience settings page is not in the dump) and
  GetIsObjectType (1: form type names). The first call per opcode and unit is traced, all
  are counted in `final.unhandled`, the value is 0.

**Actor values** (round 3; the base values and the separate pools since 2026-10-10,
docs/spec-notes/actor-values.md). Each value is a base, a permanent modifier and a
damage part [documented: GECK wiki ActorValue]. SetActorValue puts a number in the
base's place; ModActorValue adds to the permanent modifier; ForceActorValue changes that
modifier by the difference between the target and the current value; DamageActorValue
adds to the damage; RestoreActorValue takes it off, never below zero, so a value is
never restored past its permanent value [documented: the pages of those functions;
observed: Contreras' supplies (three blocks of `RestoreActorValue <limb> 100`) left a
head condition damaged by 40 at 100, `L5.quest.effect_start`, layer5-captures.md §9.3 b].
DamageActorValue that leaves Health at 0 or less kills, or knocks out an essential actor
[hypothesis: unconscious.md §2.2, whose one sighting in the original is not isolated].
GetBaseActorValue reads the base and GetPermanentActorValue the base plus the permanent
modifier [documented]. The override, the modifier and the damage are per reference in
`RefExtra` (`actor_values`, `av_permanent`, `av_damage`); the permanent value of
S.P.E.C.I.A.L. is in the Layer 4 world.

Where a value starts: S.P.E.C.I.A.L., skills, the derived statistics and the AI
attributes from the actor's record and the formulas (actor-values.md §2 to §5);
body-part conditions (codes 25–31) at 100; the others at 0. A living actor is at full
health; with no weapon equipped GetWeaponHealthPerc is 100. Without the last three
the CGTutorial script read a new character as hurt, crippled and with a broken weapon
on its first run (its stages 200, 210 and 218), which the capture saves never show
(layer5-captures.md §2: only stage 2 done).

**Fallout 3's functions** (2026-11-03, `functions::fo3`; Tale of Two Wastelands, audit
docs/handoff/ttw-fo3-script-audit-2026-11-02.md §1). The New Vegas masters never call
them, so the catalogue had no opcode for them; each opcode is the in-game GetCommandOpcode
capture's (`schemas/script/command_opcodes.json`) [observed], and TTW's compiled calls
agree (`Dad.MatchRace player` compiles to 0x11E5 in Vault101ExitScript) [observed].
ModDisposition adds to what GetDisposition reads and keeps the sum as SetDisposition
keeps its value, within 0–100 [documented: catalogue summary; the range is a
hypothesis]. GetPCIsRace is GetIsRace on the player. GetForceSneak reads SetForceSneak's
flag. SetMerchantContainer puts the actor's goods in another container
(`RefExtra::merchant_container`, read by `Game::merchant_container` for the barter menu,
actors that buy, and GetMerchantContainer), refused for a container that is not
persistent [documented: catalogue notes]. MatchRace gives the caller the race the source
has now and is drawn again as AgeRace's change is (appearance-changes.md).
ClearActorsFactionsPlayerEnemyFlag clears the player's enemy flag in each crime-tracking
faction of the actor. GetPCSleepHours reads the hours of a sleep in progress, 0 while
awake or waiting. SetLastExtDoorActivated sets the door GetPCLastExteriorDoor reads.
SetPCYoung, SetPCToddler, SetNoActivationSound / ClearNoActivationSound and
SetLocationSpecificLoadScreensOnly are kept in `PlayerState` (`young`, `toddler`,
`no_activation_sound`, `location_load_screens_only`). `SetPCYoung` forces the
race's default hair and suppresses face attachments in the actor resolver and
rebuilds the player body (`appearance.rs`, `fnv_actor::resolve`); `SetPCToddler`
switches player locomotion clips to the toddler walk set, forces walking pace
(blocking sprint), and follows the toddler walk clip's root speed when loaded
(`pc-toddler-young.md`). The refused-activation sound and the load screen choice
are not built yet.

The rest of the audit's §1 (2026-11-03, same module, opcodes the same way; Wait 0x10B7
and IsWaiting 0x106F come with StopWaiting). Two opcodes have no catalogue name and
none in the GetCommandOpcode capture: 0x1231 is KillQuestUpdates and 0x11F3
TriggerLODApocalypse, each read by lining its quest stage's source text up with the
compiled order (CG01 stage 90, MS11 stage 60) [observed]; the catalogue has them as
`observed-only` entries. What each does, and the evidence, is on its function:
MatchFaceGeometry morphs an NPC's FaceGen shape coefficients towards another's
(appearance-changes.md, `appearance.rs`; the engine applies the morphs as it draws the
head). Wait / StopWaiting / IsWaiting hold an actor in its current Follow or Accompany
package where it stands (`RefExtra::waiting`, the AI's follow step). SetCellFullName
renames an interior (the load door prompt reads it). ResetInterior resets an interior at
its next load without bringing actors back (`crate::respawn`). SetZoneRespawns turns an
encounter zone's resets on or off over its Never Resets flag (`fnv_sim` zones, saved).
DisableAllMines disables the live mines of the player's cell (`nvf_play::mines`).
RemapWaterType draws one water type as another (`nvforever` water.rs). KillQuestUpdates
drops the HUD's quest lines waiting to show. TriggerScreenBlood is drawn as the screen
splatter. GetLastPlayerAction answers the player's action now, as IsPlayerActionActive
reads it. Kept, with nothing built on them yet: SetFactionReaction's disposition
modifiers (`FormChanges::faction_modifiers`; dispositions do not read faction modifiers),
TriggerLODApocalypse (`FormChanges::lod_apocalypse`), TriggerHitShader's blur and
FadeSFX's fade (`UiRequests::hit_shaders`, `sfx_fades`).

### 7.4 Coverage (`fnvtool script coverage`, 2026-10-09, after rounds 3 and 4)

`implemented` and `stub` are filled per opcode. Over the ten masters (401 opcodes called
or used by conditions; shares by the catalogue's `script_calls` + `condition_uses`):

| | Opcodes | Calls + condition uses | With stubs |
|---|---|---|---|
| all, round 2 | 133 implemented, 60 stubs | 88.1% | 95.0% |
| all, round 3 | 225 implemented, 170 stubs, 5 unhandled (+ the unknown 0x127C) | 92.6% | 99.98% |
| all, round 4 | 232 implemented, 163 stubs, 5 unhandled (+ 0x127C) | 93.7% | 99.98% |
| all, stub audit (2026-10-12, with the combat merge) | 301 implemented (4 deliberate no-ops among them), 94 stubs, 5 unhandled | 98.5% | 99.98% |
| top 40 by use | 36 implemented, 4 stubs (SayTo, StartCombat, PlayGroup, ResetAI) | 78.4% of all uses are in these 40 | |
| intro chain (295 units, 248 conditions; `fnv_game::intro`), round 3 | 72 of 110 implemented, 38 stubs | 83.2% (all 248 conditions) | 100% |
| intro chain, round 4 | 79 of 110 implemented, 31 stubs | 88.5% | 100% |

The intro chain has no unhandled function left: SetLevel is implemented and
SetSPECIALPoints is a menu stub (character creation). After the stub audit of 2026-10-12
the intro chain has 98 of 110 implemented, 12 stubs (94.5%). Every remaining stub, its use
and what it waits for: docs/design/script-stubs.md (`fnvtool script stubs`).

### 7.6 Round 3 state and rules (2026-10-09)

Where the state lives: `PlayerState` (reputation, challenges, fast travel, crime-enemy
factions, chargen, casino levels, power armor, topics, level, `hardcore_was_off`),
`RefExtra` per reference (essential, killer, object damage, added spells, broadcast
state, crime and friendly-hit flags, talking actor, critical stage, scale, level,
minimal use), `Game::forms` (`FormChanges`: faction reactions, base essential flags,
names, quest items (no longer written since 2026-10-09: SetQuestObject sets
`fnv_sim::World::set_quest_object`, see simulation.md §3 "Quest items"), cell owners,
public cells, item values), `Game::effects` (active magic
effects). All are in `fnv.game.state/1` snapshots (new fields default when absent).
Records read (schema layouts added in this round where marked): REPU DATA (new), CHAL
DATA (new), REGN WNAM/RPLI/RPLD (new), MGEF DATA (new), the EFID/EFIT/CTDA effects of
ALCH, SPEL, ENCH, INGR, RACE DATA flags, FACT XNAM, WEAP DNAM skill, CREA DATA type,
DEST/DSTD on any record.

**Reputations** (rules replaced by the round 3 captures, layer5-captures.md §9.3 a,
`L5.quest.reputation`). Infamy (axis 0) and fame (axis 1) per REPU, the same axis
argument for all six functions [documented: GECK wiki GetReputation; observed]. A REPU's
`DATA` is its scale [documented: GECK wiki GetReputationPct; observed: scales 10–100 in
FalloutNV.esm; the limits and levels below are fractions of it on five reputations,
scales 10–70].
- **Axes 2 and 3** (FalloutNV.esm uses them, reputation.rs `axis`): GetReputation reads 0
  and GetReputationThreshold axis 3 reads 0 at every value; SetReputation with axis 2 or
  3 is a failed call that stops the script, the next line not run [observed: o5_00-02,
  2026-10-31, RepNVNCR at ten infamy/fame pairs from 0/0 to 100/100; the threshold
  axes 0-2 matched `reputation::tiers` at all ten]. Runtime: SetReputation with such an
  axis fails (`call_failed`) and stops the script for this run (`reputation::write_axis`;
  #1127). AddReputation, RemoveReputation and AddReputationExact do the same: with axis 2
  or 3 each changed nothing and the next line did not run, with axes 0 and 1 they changed
  the value and went on [observed: o11_00, 2026-11-01, RepNVNCR; observer check 62]. A
  quest stage's result script stops there too: VMS20 stage 100 with
  `bBoulderCityOutcome` 2 (the betrayal entry: `AddReputation RepNVNCR 1 1`, then
  `AddReputation RepNVGreatKhans 2 5`, RewardXP, CompleteAllObjectives, CompleteQuest,
  StopQuest) gave NCR fame +1, nothing to the Khans, and left the quest running, while
  the stage's Complete Quest flag still made GetQuestCompleted 1; with outcome 1 the whole
  entry ran (fame 12 each, karma +100, quest stopped) [observed: o11_05, nvftest]. So
  Boulder City Showdown completes either way. Runtime: all four use
  `reputation::write_axis`.
- AddReputation / RemoveReputation sizes 1–5 add or take 1, 2, 4, 7, 12 points, the
  settings fReputationBumpVeryMinor … VeryMajor (no master defines them; their defaults)
  [documented: GECK wiki AddReputation; observed: the same points on scales 10 and 70,
  and the game read the five settings as 1, 2, 4, 7, 12]. Any other size does nothing
  [observed: 0, 6, −2].
- AddReputation's result is at most the scale, also when the value was set above it
  (20 + 1 → 10 on a scale of 10); RemoveReputation's is at least 0 and leaves a value
  above the scale above it (20 − 1 → 19) [observed]. A loss from a negative value ends
  at 0, AddReputationExact is limited the same way by its sign, and a REPU without a
  scale is not limited above [hypothesis, none captured].
- SetReputation stores any value: negative, fractional, above the scale [observed: −3,
  2.5, twice the scale].
- GetReputationPct = value / scale, not capped (−0.3, 2) [observed].
- GetReputationThreshold: per axis a level, 0 below 15 % of the scale, 1 from 15 %, 2 from
  50 %, 3 from 100 %, a value on a boundary already at the higher level [observed: one
  axis swept in steps of scale/1000]; the pair (fame level, infamy level) picks a cell of
  the 4 × 4 grid in `reputation::tiers` (layer5-captures.md §9.3 a's table; the same on
  all five scales) [observed; the tier names are documented on the GECK wiki]. The grid
  is the one this note guessed in round 3; only the boundaries changed (they were
  25/50/75 %).
- Axes other than 0 and 1 (FalloutNV.esm calls SetReputation with 2, AddReputation with 2
  and 3, and conditions GetReputation with 2 and GetReputationThreshold with 3) change
  nothing and read 0, with no call error; GetReputationThreshold reads 0 for axes other
  than 0-2 [hypothesis: the wiki documents only these axes; observer check 30].
Trace event `reputation_changed` (SetReputation included).

**Effect scripts.** `Game::apply_magic_item(target, item, caster, permanent)`: each
effect of the item (EFID/EFIT) whose conditions pass on the target becomes an
`ActiveEffect` with the EFIT duration; when its MGEF's archetype is 1 (Script)
[documented: xEdit wbArchtypeEnum, wbMGEFFAssocItemDecider] its associated script runs
in an instance of its own (`InstKey::Effect`, `effect:<id>`) on the target. Timing
(observed unless tagged: aid items Cateye, Nuka-Cola and Contreras' medical supplies and
the ability ImplantRegenBoost, layer5-captures.md §9.3 b, `L5.quest.effect_start`,
`L5.quest.effect_duration`):
- ScriptEffectStart runs inside the use (`EquipItem item 0 1`): its effects are there on
  the script's next line, and the item count is already one lower.
- A zero-duration effect: nothing of its script runs after the use. Here its
  ScriptEffectFinish runs inside the call, right after the start [hypothesis: Finish
  inside the call rather than not at all; with the supplies, whose three blocks all
  restore the limbs, the two cannot be told apart].
- ScriptEffectUpdate runs every frame of game time, with ScriptEffectElapsedSeconds = the
  time since the previous Update (an ability adding Elapsed / 10 health changed it in 59
  of 59 frame steps, at 0.1 HP per second of script time) [also documented: GECK wiki
  ScriptEffectElapsedSeconds, non-zero only in the update block].
- ScriptEffectFinish comes when the duration has passed in script time (the time
  GetSecondsPassed counts): Cateye's 120 s ended 119.82–119.94 s of that time after the
  use (here 120.0 s; the game ends effects up to 0.2 s early, hypothesis on why in
  layer5-captures.md §9.3 b); then the instance is dropped.
- A container menu stops Update and the duration (31 s of real time in a menu did not
  count). Loading screens do the same [hypothesis].
- The same item used again while its effect is active adds its duration to that effect:
  one effect and one Finish, 240 s after the first of two Cateyes taken 10.2 s apart. The
  start block does not run again (trace `effect_extended`) [hypothesis: not visible with
  Cateye], and only the same item and effect index merge [hypothesis].
- In a frame, effects run after the quest, object and item scripts [hypothesis].
Effect conditions are tested once, at the start [hypothesis]. The engine API is
`Game::consume(actor, item)`:
one ingestible leaves the inventory and its effects start (the player using an aid
item); EquipItem of an aid item does the same; the runner op `consume` stands in for it.
AddSpell applies a spell permanently (no duration; abilities and addictions in the
masters), RemoveSpell removes it, CastImmediateOnSelf applies a spell with durations,
Dispel / DispelAllSpells end effects with their finish blocks, except an effect whose
own script is running: that script stops like a return and its finish block does not
run [documented: catalogue notes of Dispel]. IsSpellTarget and HasMagicEffect read the
active effects. What Value Modifier, Value And Parts and Limb Condition effects do to
actor values, the potency of aid items, their usage monitor and addiction:
docs/spec-notes/aid-items.md (2026-10-12; GetSpellUsageNum added). Shaders and the other
archetypes are Layer 6. In the intro walk-through the concussion spell (PlayerConcussed, cast by
CastImmediateOnSelf) now starts its Concussion effect; its second effect's condition
(PerceptionCondition ≤ 0) fails, so it does not start.

**Objects.** PlaceAtMe creates references with fnv-sim's `World::place` at the caller's
cell and position with its angles [documented: catalogue notes; the distance and
direction arguments do nothing, documented bug]: an inventory item as one reference
holding the count, other forms one reference per count, the last one returned
[hypothesis]; a leveled actor list is placed as the list (Layer 6 resolves it). A
created actor (an NPC_, CREA or leveled actor list placed so, `Game::created_actors`)
is an actor from the frame after: the engine makes it a resident where it was put
(nvforever `residents.rs` `take_created`: drawn with its rig, walked by the AI,
measured by detection both ways, fighting as placed actors do), it chooses packages,
and counts as kin of placed actors of its base; it is not drawn as a dropped item
[design; until 2026-10-16 it was drawn as a static model and never fought, owner's
report; smoke `placeatme-fights`]. A
created reference with a script runs it like a placed one from the next frame (OnLoad
then, in a loaded cell) [hypothesis]; trace event `placed`. DropMe and Drop use fnv-sim's
drop rule; DropMe acts as a return and neither runs OnDrop [documented: catalogue notes].
Ownership lives in the reference's Layer 4 extra data; SetOwnership with no owner gives
the player's base (0x7) [documented]. Destruction: the stage is the highest DSTD index
whose health percent the remaining health (DEST health − damage) has fallen to, and
GetDestructionStage gives it counted from 1 (index + 1), 0 when damaged above every stage,
−1 without data or damage [documented −1: GECK wiki GetDestructionStage; the rest
hypothesis; the count from 1 fits the masters' scripts, which compare it with values only
it reaches: Zeta's generator core (eight stages) disables itself at 7, its Destroy stage,
and tests 8; Point Lookout's Brain (seven stages) bursts at 7; DLC04DarMuckBubble (two
stages) tests `>= 2` (#3053, observer check 247)]; a stage with the Destroy flag sets the
destroyed state, one with Disable disables the reference, a stage change runs
OnDestructionStageChange [hypothesis from the flag names]; a block with a parameter
(`Begin OnDestructionStageChange 3`) runs only for the stage reached counted from 1 (DSTD
index + 1), one without a parameter for any change [hypothesis: fit to the only such
block in a TTW load order, SCPT 0x0A096CC8 on MQ11vertibirdPartyBus, whose 3 is index 2
in both Fallout3.esm's four-stage and TTW's three-stage DEST, DSTD damage stage 3 too;
observed, #3112: without it Liberty Prime's march never passed the first wreck]. Regions: the player is in a
region when in an exterior cell of the region's worldspace and inside one of its area
polygons (even-odd rule; the RPLI edge fall-off is ignored) [documented: never in an
interior, nested regions both true; hypothesis: the polygon test]. GetHeadingAngle
measures clockwise from the caller's facing in (−180, 180], 0 for a caller that is not
an actor [documented]. RemoveAllTypedItems type codes: 40 weapons [documented: GECK wiki
example]; 24 armor, 25 books, 29 ingredients, 30 lights, 31 misc, 41 ammo, 46 keys,
47 aid, 49 notes [hypothesis: the form type numbering published by the script extender
community; the masters pass 24, 26, 31, 40, 41, 47 together, observed].

**Actors and factions.** IsChild reads the race's Child flag; GetIsCreatureType the
CREA DATA type [documented]. Essential = the base flag (ACBS, or SetEssential) or the
reference flag (SetActorRefEssential), which cannot clear the base's [documented].
KillActor records its killer for IsKiller. Faction reactions are FACT XNAM with
SetEnemy/SetAlly overrides (flag 0 enemy / ally, 1 neutral / friend) [documented].
GetFactionRelation between two actors: over the caller's and the other's factions,
enemy if any reaction is enemy, else friend, else ally, else neutral; the player is an
enemy of a caller in a faction the player is a crime enemy of [hypothesis].

**The player.** EnableFastTravel keeps the travel / wait / keep flags; waiting is
forced on with travel, and without keep an interior → exterior move allows travel again
[documented: catalogue notes]. Challenges keep unlocked / completed / not-recurring;
completion comes from the Layer 6 challenge counters (none yet). Casino winnings levels
are 0 until the casino games (Layer 6) set them. GetDayOfWeek is the Gregorian weekday of
the GameYear / GameMonth (from 0) / GameDay globals [hypothesis; 18 October 2281, the
masters' start date, is a Tuesday]. IsAlwaysHardcore: on now and never switched off
[hypothesis].

The source handler `functions::reputation::remove_reputation_exact` reuses the
fractional reputation writer and its events for `RemoveReputationExact`.
See [player source commands](player-source-commands.md) for limits, invalid-axis
policy and revision-specific authored controls. Portable controls passed for c4 in
hosted composite `23388a32d`; revision-specific results are recorded in PR #2772.
Native parameter encoding and original-game parity are unmeasured for this contribution.

### 7.7 Menus, message boxes and dialogue lines (round 4, 2026-10-09)

Evidence: the round 4 captures from the new-game save `nvfnew` (layer5-captures.md §10),
the masters' source text, and the GECK wiki pages the owner exported (owner export 2026-10-09).

**Menus.** `Game::menu` is the open menu's code. Scripts open five menus, each with the
code observed right after the stage whose result script calls the function
[observed: layer5-captures.md §10.2–10.4]: GetPlayerName 1051 (name), ShowRaceMenu 1036
(race), SetTagSkills 1048 (tag skills), ShowTraitMenu 1084 (traits),
ShowLoveTesterMenuParams 1074 (the Vigor Tester's S.P.E.C.I.A.L. menu, opened inside
the tester's OnActivate, inside `Activate`). What the player does in them (the name, the
race, the picks) is outside the scripts; `Game::close_menu` closes the menu as the
player's OK / Done or the console's CloseAllMenus would [observed: CloseAllMenus closed
each of them, and the chain went on as after the player's OK, §10.1, §10.3]. While a menu
is open, game time stops, GameMode does not run and MenuMode does (a conversation with
the player is such a menu too, code 1009, kept in `Game::dialogue` rather than
`Game::menu`: `Game::in_menu`, docs/spec-notes/dialogue.md §6) [observed: the
stopwatch read 0.0 s after 120 frames of the name menu; VCG01's 1 s timer of stage 10
ended 0.95 s after the menu closed; §10.7]. A second menu replaces the first
[hypothesis].

**Message boxes.** ShowMessage of a message with the Message Box flag (or buttons) is a
menu, code 1001, until the player closes it [observed: the four pack plugins' "items
added" boxes held GameMode, VCG00's countdown and the world clock, §10.1]; other
messages are corner notifications and only traced. A box shown while a menu or another
box is open waits and opens when they close (`Game::boxes`) [hypothesis]. Closing a box
that waits for a button presses button 0 (OK) [hypothesis]; `Game::press_button` picks
another. **A menu opened during GameMode holds the frame's remaining GameMode scripts**
(quests keep their due delay and run once it closes) [hypothesis, generalised from the
pack boxes opening one at a time, each after the previous one was closed, as their
quests ran, §10.1–10.2]. Which pack box opens first varies in the game; here the pack
quests run in FormID order, 5 s (their quest delay 0) after the game starts: Classic,
Mercenary, Caravan, Tribal. In the game the next box came 0.45–1.4 s after a close
[observed, §10.2]; here 1 frame (they were all due) — a difference.

**Dialogue lines** (`Game::say`, `Game::speech`). `speaker.SayTo target topic` and
`speaker.Say topic` say the topic's first line the speaker can say. SayTo's
`noTargetLookFlag` (4th parameter) is read for who turns to whom (the original turns a
standing speaker to the target with the flag 1 as with 0 [observed 2026-11-25, observer
check 218]; ours turns nobody with 1, a difference)
(docs/design/dialogue-play.md §9); `forceSubtitleFlag` is not (subtitles always show). Since 2026-10-10 the
choice of the line is shared with conversations (`Game::select_line`,
docs/spec-notes/dialogue.md §4: quest priority, then stored order, Random stacks picked
at random, Say Once a Day), and a response is timed in one place
(`Game::response_seconds`). As first written:
- The topic's lines (INFO) come from the "topic children" group after each DIAL, in file
  order of the plugin that first defines each [documented: group types, xEdit;
  hypothesis that file order is the editor's list order; the masters' INFOs have no PNAM].
  Lines of higher-priority quests come first [documented: GECK wiki Quest Data Tab].
- A line is said when its quest runs [hypothesis] and the quest's conditions pass
  [documented: Quest Data Tab: checked first, for all of the quest's dialogue], its own
  conditions pass (subject the speaker, target the listener; the player for Say)
  [documented: Category:Conditions], and, with Say Once, no actor has said it
  [documented: Category:Dialogue: once per actor; observed: observer check 31, a line two
  actors share was not said again by the second, dialogue.md]. Random lines are taken in order, not
  at random [hypothesis]. ANAM restricts the speaker.
- The begin result script runs when the line starts, the end script when it ends
  [documented: Category:Dialogue], on the speaker, with the line's quest as the quest.
  Then the speaker's SayToDone blocks run, for Say as well as SayTo [documented: GECK
  tutorials, level-design.md §11.2 D1; hypothesis on the order].
- A line lasts 1.1 s plus 1/16 s per character of each response's text
  (`LINE_BASE_SECONDS`, `LINE_SECONDS_PER_CHAR`) [hypothesis: fitted to the observed
  spans of Doc Mitchell's lines (29 characters: 2.9 s; three lines of 193: 15.4 s; two of
  129: 10 s), §10.2–10.3]. Since step D3 a response lasts as long as its voice file
  when the runtime is given the files (`Game::voice`; `fnvtool script run` and
  `nvforever --play` do so), and keeps this fitted length otherwise or when it has no
  file; the sounds leave the captured spans 0.3 to 0.4 s per line short
  [observed: docs/spec-notes/voice.md §8]. Only game time counts (menus stop a line) [hypothesis, consistent with Doc's
  stage 60 line ending after the message boxes, §10.3].
- A speaker saying a line that starts another (SayTo in a result script, or a new SayTo)
  stops the first without its end script [hypothesis]; a new Say or SayTo of another
  topic takes over at once, and one of the topic already being said (by Say or SayTo)
  renews the speaker's talking state without a queue [observed: observer batch o30, PR
  #2580]. Since #2582 a script's Say or SayTo of the topic its speaker is saying leaves
  that line playing (`Game::say_by_script`) [hypothesis: the voice and the subtitle do
  not restart with each call, observer check 144 open]: the Strip gate's kill zone
  (SCPT 0x00119786) SayTos its topic every frame the player is inside, and its line
  restarted every frame before. A line's own end script runs after the line is over,
  so a chain of SayTo of one topic from end scripts says a new line each time. A
  difference: o30 saw IsTalking stay on about 4.9 s after a line's file ended; here it
  ends with the line. A speaker that is disabled,
  deleted or dead stops its line [hypothesis]. IsTalking is 1 while a line is said.
- A Say or SayTo on a speaker already talking takes over at once, for the same topic or
  another: nothing is queued and the call is not ignored [observed: campaigns o30_say,
  o30b_say, 2026-11-14 (#1858 batch o30), the Strip Gatekeeper VFSSecuritronGreeterREF with
  the gate volumes disabled, IsTalking read every game loop]. One line said alone keeps
  IsTalking on for its file's length plus about 4.9 s (Halt, 4.35 s: 9.21 s; Kill lines
  1.1-2.9 s: 5.2-6.2 s). Halt and Halt again 1 s later: 10.23 s (= 1 + 9.21; ignored
  would be 9.21, queued about 18); 3 s later: 12.24 s (= 3 + 9.21). Kill then Halt 0.33 s
  later: 9.58 s; Halt then Kill 1 s later: 7.15 s; Kill then Kill 0.5 s later: 5.61 s.
  SayTo every game loop for about 20 s (Say the same, the subtitle flag 0 the same) keeps
  IsTalking on without a break and lets it go one span after the last call. What these
  runs could not show is whether the voice and the subtitle restart with each call too:
  the game, not in front, played no sound into the loopback recording and the screenshots
  caught the desktop. That is observer check 161, for the owner.
- What this reproduces [observed, §10.2–10.3]: Doc's SayTo VCG01Intro takes the next
  unsaid line each time; three lines' end scripts chain SayTo to reach the line that sets
  stage 10; the first line of the topic is said again at stage 60 when it was never said
  (stage 5 then set while GetStage stays 60); VCG01DocReactions picks the reaction by
  `nDocReaction`.
- Conversations with the player (the greeting, choices, StartConversation, Goodbye, Run
  Immediately, follow-up lines) are docs/spec-notes/dialogue.md §5–6
  (`crate::dialogue`). Not modelled: the voice file lengths (step D3), conversations
  between NPCs.

### 7.5 AI packages (the start of Layer 6, `Game::select_package`)

Read: every `PACK`'s conditions (the `CTDA` before its begin/end/change sections) and
every actor base's package list (`PKID`, in order), through the template (`TPLT`) when
its template flags include "AI Packages" [documented: xEdit PACK and NPC_ layouts].

- An actor's package is the **first valid package of its list**: in its schedule, with
  the linked reference it needs, not a Once Per Day package chosen within the last 24
  hours, and its conditions passing with the actor as the subject (packages.md §3–§4,
  docs/design/actor-behaviour.md §3); with none valid the actor keeps its current list
  package [observed for the conditions: the three Goodsprings settlers took
  GoodspringsFleePackage, their first, exactly when its conditions held and the next one
  otherwise; Sunny Smiles took her third package when the two before it failed;
  `L5.quest.condition_groups`; hypothesis: list order decides, as these cases show
  without proving it in general].
- A script package (AddScriptPackage) overrides the list until RemoveScriptPackage
  [documented: catalogue summary].
- `EvaluatePackage` chooses again and keeps the choice as the current package (trace
  `package_chosen`); `GetIsCurrentPackage` compares with it, choosing first if the
  actor never chose (an actor in the world always runs a package [hypothesis]). The
  runner's `package` input with `start`/`change` also makes its package current.
- The game's own periodic choice and the packages' begin / end / change scripts run only
  with `GameConfig::package_interval` set (`fnv_game::ai`); without it, as in every
  Layer 5 run, a package starts only through the runner's `package` input.
- Not modelled yet (Layer 6): the package's procedure (what the actor does), locations
  and the flags other than Once Per Day, Must Complete, Must Reach Location and Continue
  If PC Near.

## 8. The runner (`fnvtool script run`, `fnv_game::runner`)

Input `fnv.script.actions/1` and output `fnv.script.trace/1` follow 5C's contract
(layer5-captures.md §4): `seed`, `frame_seconds` (default 1/60), `given` (quest
running/completed/stage/done/not_done/objectives/variables by name, globals, item counts,
disabled flags; set before action 0 with no observation), the operations, the frames
after each, and observations keyed by `label`. A `given` stage that the stages done do
not explain is kept as the quest's current stage.

An input may name the load order it is written for (`plugins`, default the first plugin
given): `script run` uses its command line's plugins and ignores it, `script cover` runs
the chain over it. `script run --coverage <file>` also writes what the run reached
(`fnv.script.coverage-log/1`, `crates/fnv-game/src/coverage.rs`): the quest stages set,
the script blocks run (unit and block), the dialogue lines started and the result scripts
run, FormIDs named by their plugin (`FalloutNV.esm:10E908`) and unit keys without their
plugin, so runs over different load orders merge.

Additions (headless stand-ins for Layer 6 and the engine; scripted inputs, like
`fnv-sim`'s actions stand in for the player's hands):

| op | stands in for |
|---|---|
| `advance_seconds` | time passing at the fixed frame step |
| `player_activate {target, actor?, label?}` | activation by hand (OnActivate instead of the default); when the player's activation ends in the default action of talking (an actor, or a talking activator whose OnActivate calls `Activate`), the conversation opens as in the play loop; observation `info`. When it opens a container, the container menu opens and closes with nothing taken (`remove_all_items` stands in for taking): its OnOpen and then OnClose run at the start of the next frame, as the play loop queues them; observation `opened` |
| `say_info {info, speaker?, force?}` | a dialogue line being said: its begin and end result scripts run on the speaker; refused when its conditions fail (with the conditions and their values in the error) unless `force` |
| `say_first {infos, speaker?, label?}` | the speaker picking the first line whose conditions pass; observation `said` |
| `package {actor, package, event: start/done/change}` | an AI package event: the package's result script and the actor's OnPackage* blocks |
| `say_to_done {actor, topic?}` | a SayTo or Say line finishing (SayToDone blocks) |
| `patrol_point {actor, point}` | an actor on a patrol reaching a point: the point's patrol script runs on the actor and its topic is said (`Game::patrol_point_reached`, as fnv-ai's carry calls it); refused when the point has neither [hypothesis: the order script, then topic, is fnv-ai's] |
| `fire {weapon, actor?}` | the player (or an actor) firing a weapon it carries: the weapon's OnFire blocks (`Game::weapon_fired`, as nvf-play's combat calls it); refused when the weapon is not carried or has no OnFire block (Honest Hearts' Pine Creek detonator ends Flight from Zion this way) |
| `trigger_enter {trigger, actor?, event: enter/inside/leave}` | an actor in a trigger volume (OnTriggerEnter / OnTrigger / OnTriggerLeave) |
| `press_button {button}` | the player picking a message box button: the button is set, one frame runs with the box open (MenuMode 1001 blocks read it, as in the play loop), then the box closes |
| `save_load {label?}` | the player saving and loading that save at once: the game's snapshot written into a save's bytes (`fnv_save`) and restored from them in the same frame, the random numbers where they were; observation `bytes` |
| `menu {menu}` | a menu opening (MenuMode with that code) or closing (`null`) |
| `move_player {to}`, `load_cell {cell}` | the player moving (MoveTo) and cells loading (OnLoad) |
| `get_var {of, var}`, `get_ref {of}` | reads of any script variable, and of a reference's state |
| `kill {actor, killer?}` | combat ending in a death (KillActor, OnDeath) |
| `hit {target, attacker?, weapon?, label?}` | a shot or blow reaching a reference that is not an actor (`Game::hit_object`: its OnHit and OnHitWith blocks; attacker the player by default); observation `ran` |
| `consume {item, actor?, label?}` | the player (or an actor) using an aid item: `Game::consume`, effect scripts start (§7.6); observation `effects` |
| `note {text}` | a comment; no effect |
| `talk {actor, topic?, label?}` | a conversation starting (`Game::start_dialogue`): the player activating the actor, or the actor's dialogue package; observation `info` (docs/spec-notes/dialogue.md §8) |
| `choose {topic \| prompt \| index, max_frames?, label?}` | the player picking one of the choices offered, after waiting for the actor to finish speaking; observation `info` |
| `wait_talk {start?, max_frames?, label?}` | waiting until choices are offered or the conversation is over (with `start`: until a script's StartConversation has started one first); observations `state`, `choices`, `actor` |
| `end_talk {}` | the player leaving the conversation |
| `terminal {target, item, menu?, label?}` | the player picking a terminal entry (`terminal::choose`, hacking.md §6): its result script runs on the terminal reference; `menu` names a sub menu's TERM; refused when the entry is not listed; observations `submenu`, `note`, `back` |
| `play_note {note, label?}` | the player playing a holotape from the Pip-Boy: a Voice note's line has its begin and then its end result script run on the player (`Game::holotape_script`, holotapes.md §3; the voice's length is not waited for); observation `info` (null for a note that plays no line) |

Round 2 of the contract (layer5-captures.md §8.2), as the capture issues them:

| op | runner |
|---|---|
| `activate` + `run_on_activate` | `ActMode::Block`; `opened` is whether any activation during the call opened a container (a nested default activation included) |
| `set_ref_var`, `get_ref {ref, fields}` | the reference's object-script variables by name; `get_ref` also reads `dead` and `disabled`, one frame per field; an undeclared name reads 0 |
| `activate_read` | activation and the read with no frame between |
| `sample_ref_var`, `sample_var` + `values` | as `sample_var`; `values` lists the successive distinct sampled values |
| `add_item_read` | AddItem (silent) and the counts (`count.<item>`), then one frame |
| `sample_items` | one sample per frame, read before the frame runs: `first.`, `final.`, `settled.` per item |
| `menu_gap` | the player's default activation of `via` must open a container; `menu` is 1008 for `frames` frames (the opening frame counted), the variable is read in the last, then `after` frames; `drop` is the first change |
| `kill {target}` | KillActor without a killer, 1 + 60 frames (`actor`/`killer` remain for the walk-through) |
| `evaluate_package`, `is_current_package` | §7.5; `current` |

`given` kind `ref` sets object-script variables, `dead` and `disabled`.

Round 3 of the contract (layer5-captures.md §9.2), frames as there:

| op | runner |
|---|---|
| `set_reputation`, `add_reputation`, `remove_reputation` | the script functions with axis 1 (`fame`) or 0 (`infamy`) |
| `get_reputation`, `set_reputation_read` | the fields through GetReputation, GetReputationPct and GetReputationThreshold (thresholds as integers); one frame per field, or none between the sets and the reads |
| `use_item` | `EquipItem item 0 1` on the actor (an error if it carries none), then `av.<name>` / `count.<form>`; with `stopwatch`, the stopwatch's zero is the current game time |
| `wait_av` | checks the value (to 1e-6) before each frame, at most `max_frames` frames; `seconds` = game time since the stopwatch's zero |
| `hold_menu` | the player's default activation of `via` must open a container; menu 1008 for `frames` frames, then closed |
| `damage_av`, `restore_av`, `get_av`, `set_av` | Damage/Restore/Get/SetActorValue (§7.3) |
| `add_spell`, `remove_spell` | AddSpell / RemoveSpell (§7.6) |
| `sample_av`, `sample_av_damage` | one sample per frame read before the frame runs (as `sample_items`); `per_second` over the game time of the span |
| `go_to_cell` | the player moves to the cell's COCMarkerHeading reference, else (outdoors) the middle of the cell at height 0, and the cells around it load [hypothesis: where `coc` puts the player matters here only for the cells it loads; the Goodsprings exterior cell has no COC marker]; no loading-screen frames |
| `sample_jumps` | every read once per frame after the frame's scripts ran; a jump is a change of at least `min_step` from the previous sample |

`given` kinds `reputation` (SetReputation) and `actor_value` (the current value, with no
damage). Actor values are named by their script name (`encodings.json`, e.g. `NightEye`)
or xEdit name, ignoring case and spaces (`Nighteye`). Codes shipped source never spells
take their AVIF Editor ID without `AV` (`PoisonResist` 19, `CritChance` 14, `FireResist`
52, `EnergyResist` 60, `DetectLifeRange` 51) [observed: FalloutNV.esm AVIF; the original
compiler takes `GetAV PoisonResist`, capture abilities-1; hypothesis for the others], and
`ElectricResist`, `FrostResist`, `EMPResist` (58, 59, 61; no AVIF in the masters)
[hypothesis: the same pattern]. They live in `av_code` (crates/fnv-game), not in
encodings.json, which holds shipped spellings only and drives the decompiler's text (a
code 19 still prints `#19`).

Round 4 of the contract (layer5-captures.md §10.4), frames as there:

| op | runner |
|---|---|
| `start_stopwatch` | the stopwatch's zero is now (`Game::time`, the script time); 1 + 2 frames |
| `set_quest_var` + `stopwatch` | the variable, and the stopwatch's zero in the same frame; 1 + 10 |
| `dismiss_message_boxes` | `max` times: wait (checked before each frame, at most `wait_frames`) for menu 1001, count it if open, `Game::close_menu`, 3 frames; `closed` |
| `close_menus` | `Game::close_menu`; 1 + 3 |
| `wait_quest`, `wait_menu` | checked before each frame, at most `max_frames`; one frame later `found` (read again) and, with `stopwatch` and found, `seconds` |
| `get_menus`, `get_tag_skills` | `menu.<code>` (`Game::menu`), `tag.<Skill>` (IsPlayerTagSkill); one frame each |
| `get_ref` field `destroyed` | GetDestroyed |
| `press_button`, `menu` (stand-ins) | `Game::press_button` (the box closes); `Game::open_menu` / `close_menu` |

`given` kind `menu` opens a menu before action 0; for 1001 a box of an unnamed message.
**Message boxes hold GameMode only when the input states the menus of its save** (any
`given` kind `menu`): rounds 1–3 do not, and their saves (`nvftest`, `nvfdoc`) had
closed long ago the boxes a new game shows (the pack plugins' "items added", the
add-ons' "loaded" messages), which the runner, starting from the new-game state, would
otherwise show and keep open [decision; owner review: the alternative is to give those
inputs the pack and add-on quest states]. **A new game puts the player at the opening's
start marker** (`VCG01PlayerStartMarkerREF`, where VCG00 stage 0 moves the player and
where `nvfnew` has it [observed: layer5-captures.md §10.2]); before round 4 the player
had no place, and the add-ons' start quests, which wait for the player to leave Doc
Mitchell's house, acted at once [decision]. **Reads take one frame**
(`get_item_count`, `get_disabled`, `get_global`; they took 11): the capture schedules a
pause only after steps that change the game (`capture_steps.py`).

Names: `player`, a hex FormID, an Editor ID of any record, `scripted:<SCPT>[#n]` (the
n-th placed reference whose base runs that script) and `placed:<base>` (the first placed
reference of a base form).

Output: `observations`; `steps` (frame and error of each action); `given_errors`;
`events` (§3.8 kinds: `quest_started/stopped/completed/failed`, `stage_set` with the
entries that ran, `stage_ignored`, `objective_displayed/completed`, `var_set` for writes
to another script's variables (own locals with `trace_locals`), `global_set`,
`ref_enabled/disabled` on change, `item_added/removed`, `message_shown` with text and
buttons, `moved`, `activated`, `block_run`, `result_script`, `stub` (each distinct call
once per unit), `unhandled`, `call_failed`, `script_error`, `skipped`,
`undeclared_variable`, and from round 4 `menu_opened`, `menu_closed`, `line_started`,
`line_ended`, `no_line`, and from the dialogue work `dialogue_started`, `response_said`,
`choices_offered`, `choice_made`, `topic_added`, `dialogue_ended`, and `assault_alarm`
for SendAssaultAlarm with a faction: its victim and the members alive, absent and dead,
#2926); `final` (frames,
the running conversation `dialogue`, every touched quest with stages, objectives and
named variables, the player state, reported references, unhandled and stubbed call
counts).

**`L5.quest.*` (2026-10-09, round 3): 24 of 27 pass**, holdouts included (all three
rounds' holdouts pass; before round 3's runtime work the six round 3 scenarios errored
on the new operations). The three that fail compare GetStage of a quest whose `given`
names no stage: CGTutorial (save 2, new game 0), VCG01 (save 200, new game 0), VCG00
(save 100, new game 0). The candidate starts from the new-game state (§4.1 of the
contract), so these rows need `"stage"` (or `"done"`) in the input's `given`; the delays
and drops of the same scenarios match. (`L5.script.decode` is 5A's.)

**`L5.*` (2026-10-09, round 4): 28 of 32 pass** (24 before): `L5.intro.opening`,
`.tester`, `.complete` and the round 4 holdout pass; the four known fails remain (the
three `delay_*` inputs without a stage, `L5.script.decode`).

## 9. Headless intro chain (`crates/fnv-game/walkthrough/intro.actions.json`)

Round 4: the walk-through starts from the facts of the new-game save `nvfnew`
(layer5-captures.md §2, §10): a pack plugin's message box open, VCG00 running at 90 with
its 3 s countdown (stages 0 and 90 done, `bRunTimer` 1), VCG01 running with nothing
done, VCG01Test, CGTutorial and the pack quests running, VCG02–VCG04, VMQ01 and
VGenericTimer stopped, S.P.E.C.I.A.L. all 5, the player at the opening's start marker.
From there the game's own timers, Doc Mitchell's SayTo lines (§7.7), his two
conversations and the menus drive the chain. Since 2026-10-10 the psych test and the
farewell are real conversations (docs/spec-notes/dialogue.md §9): the couch trigger's
own script calls StartConversation, the player's fourteen answers are `choose` inputs,
and the farewell runs from its greeting through follow-up lines to the Goodbye line
that starts VGenericTimer's event 3. Stand-ins are left for what the player does and
for AI: the OK / Done of the boxes and menus, walking into the two triggers (60, 115),
sitting on the couch (objective 40 and the trigger's `bRunTimer`), Doc's dialogue
package for the farewell (`talk`) and the hardcore question (button 0). Run with
`fnvtool script run <Data> <ten masters> --schema schemas/fnv --actions
crates/fnv-game/walkthrough/intro.actions.json` (16,357 frames with the voice files' lengths, 19,369 with
`--no-voice-lengths`; about 3 s). The two conversations pause the game behind them
(docs/spec-notes/dialogue.md §6, §9): their frames count, script time does not. The VCG02 / VCG03 part is the round 3 one (Sunny's lines, packages,
deaths and crafting as inputs).

Results: VCG00 stages done 0, 90, 95, 100; VCG01 all defined stages but 9, 17, 20, 37,
78 — **both the sets of the capture saves** (§2 there); VCG01 200 completes and stops
VCG01, starts VMQ01 (10, objectives 10/25/30), VCG04 and VCG02 (5, objective 3,
running), GameHour 8, as `L5.intro.complete` saw; tag skills Guns, Science, Sneak;
CGTutorial 2 at 55 with no box (as observed); 55's `autosave` a recorded no-op. VCG02
and VCG03 end as in round 3 (VCG02 stopped by Trudy's line at 50, VCG03 completed).

**Stage timeline against the capture** (script seconds, from the first box closed, then
each step from the stage before it; capture: §10.2, §10.3, §10.7, natural order):

| Stage | Walk-through | Capture | Note |
|---|---|---|---|
| VCG00 95 / 100 / VCG01 0 | 3.03 / +1 frame / same frame | 3.03 / +1 sample / same sample | |
| VCG01 1, 3 | +0.22, +2.83 (6.10) | 3.28, 6.09–6.13 | |
| 5 (Doc's first line) | +2.90 (9.00) | 9.05 | line length fitted |
| 7, 8 | +3.27, +1 frame | 3.22 after 5, +1 sample | |
| 10 (three lines), name menu | +15.37 (27.65), menu 1051 | 27.81, menu 1051 | fitted |
| 15 after the name menu | 1.02 | 0.95–0.97 | |
| 25, 30 | +10.5, +14.8 (Doc's lines) | lines (not timed) | |
| 35, 36, race menu, 40 | +3.02, +1.78, 1036, 40 inside it | 2.97, a line, 1036, 40 inside | |
| 45, 50 | +11.1 (lines), +3.02 | lines, 2.98 | |
| 55 | +10.2 (two lines) | about 10 s | fitted |
| 60 → objective 30 | 8.75 (Doc's line; trigger by input) | about 10 s | line length |
| 65, menu 1074 | inside the activation | inside the activation | |
| 70 | 1.02 after the menu | 0.96 | |
| 75, 76, 79 | one frame | one frame | |
| 80 | +18.1 (three psych intro lines) | about 13 s | **line lengths** |
| 85 → 90, menu 1048 | 1.0 | 1 s | 85 by the end script of the last psych answer's line |
| 95 | 1.02 after the menu | 0.96 | |
| 98, 100, 102, menu 1084 | lines (17.9), +1.02, +1 frame | lines, 1 s (0.98), next frame | |
| 105 | 1.02 after the menu | 0.97 | |
| 110 | +3.47 (lines) | lines | |
| 115, 200 | 115 by input; 200 0.4 s after the farewell conversation's Goodbye line | by the job; event 3 | |

Differences: (1) the pack boxes came 5 s after the start (the pack quests' 5 s delay
from a new game) and then one frame after each close; in the game they came 0.45–1.4 s
after each close, and which pack came first varied; (2) every span made of Doc's lines
uses the fitted line length, off by up to about 5 s (80: 18.1 against about 13 s; 60's
line 8.75 against about 10 s); (3) the timer steps run 1/60 s frames against the
capture's 25–40 loops per second, so they read 1.02 against 0.95–0.97 (within one capture
frame). Everything else (which stage follows which, by what, and which menu each opens)
is as captured.

Round 3's walk-through (from the bare new-game state with every Doc line as an input)
reached the same stages done; its round 3 note on the 85 → 90 countdown expiring at once
is gone with the GetSecondsPassed rule of §5.

What still blocks a run without inputs: the player's own actions (menus' OK, the walk,
the couch, the answers in the two conversations, the hardcore answer), AI packages
(Doc's dialogue package, Sunny's travel) and combat (VCG02), crafting (VCG03). Sunny's
and Trudy's lines in the VCG02 / VCG03 part are still `say_info` / `say_first` inputs.

## 10. Open questions (5B)

- A stray `ElseIf` right after a chain that took a branch (§4.2): suppressed or tested?
  No capture had two thresholds in one run.
- GetSecondsPassed across a Wait-menu gap, and inside a MenuMode block (§5).
- Round 4 (§7.7): the length of a line (voice files are not read; the fit is off by up to
  about 5 s); whether a line advances in a menu; whether a new SayTo cuts the line being
  said or waits; the box queue and the "menu holds the frame's GameMode" rule (the pack
  boxes came 0.45–1.4 s after each close in the game, one frame here); Random lines.
- Documented rules listed by the coordinator but not done (docs/spec-notes/
  script-language.md and actor-behaviour.md cross-checks): an inventory item's object
  script runs while its holder is loaded (only the player's items run here; containers'
  untouched inventories live in their base forms); a dialogue result script that declares
  variables overwrites the quest script's variable at the same index (a documented bug,
  not emulated); one script package per actor (a second AddScriptPackage replaces the
  first, here they stack) and EvaluatePackage dropping a script package without
  must-complete / must-reach-location (Layer 6 package logic).
- A stage entry whose conditions fail next to ones that pass (skipped, the others run:
  hypothesis); whether an entry's conditions see the effects of earlier entries' scripts.
- Frame order: quest scripts before object scripts is observed for one pair only
  (layer5-captures.md §9.3 c); where effect scripts, inventory items' scripts and actors'
  scripts come is not captured. The rising plane's GameMode stall after Enable
  (layer5-captures.md §8.3 d, unexplained; its OnLoad is read as the cell load's).
- Scripted items held by actors other than the player, and by containers: do they run
  GameMode (§7.2)? OnAdd once per call or once per item.
- MenuMode quest delays (one counter per mode, §7.2): not captured.
- AI packages (§7.5): schedules, re-evaluation, and whether list order alone decides.
- The `given` of `L5.quest.delay_flagged_1s`, `.delay_flagged_short` and `.delay_frame`
  lacks the quests' stages (§8).
- Round 3 (§7.6), settled by the round 3 captures: the reputation limits, levels and
  grid, and effect-script timing. Still open, each a capture candidate: a zero-duration
  effect's Finish (inside the use or not at all); whether the second dose's Start runs;
  whether two different items with the same base effect merge; effects on actors other
  than the player and abilities' Start/Finish; ScriptEffectElapsedSeconds read in a
  script; the early end of effects (up to 0.2 s); AddReputationExact's limits and a
  loss from a negative value; GetDayOfWeek on the start date; GetDestructionStage after
  DamageObject; PlaceAtMe of a stackable item with a count; GetFactionRelation between
  members of factions with mixed reactions.
- The game clock: `Game::tick` does not move GameHour (the engine and `fnv-sim` own the
  clock); `L5.quest.script_order` showed the game clock advances before object scripts
  run in a frame, which matters once the runner advances it.
