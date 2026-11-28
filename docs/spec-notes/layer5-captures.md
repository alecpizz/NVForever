# Layer 5 in-game captures: quest state and quest-script behaviour (workstream 5C)

Captures made with the `nvf_capture` xNVSE plugin (version 7) on the owner's install (FNV
1.4.0.525, Steam, xNVSE 6.4.9, the ten masters), unattended through
`tools/capture/capture_run.py`. They establish the quest state of the two capture saves
and the behaviour that docs/design/layer5.md §3.3–3.5 left as [hypothesis]. The tool is
`tools/capture/capture_quests.py` (README there); scenario inputs are in
`parity-lab/inputs/layer5/`, scenarios in `parity-lab/scenarios/layer5_quests.json`.

Private files (never committed): definitions, jobs and outputs under
`C:\fnv-private\capture\` (`quest\`, `out\`), goldens under `C:\fnv-private\goldens\quest\`.
No job saved the game; every change a job makes is lost when the game quits. One round 4
trial let the game's own scripted `autosave` (VCG01 stage 55) run, which wrote
`autosave.fos` (§10).

Tags as everywhere: [documented] (source named), [observed] (this capture or the masters),
[hypothesis].

Summary of what was settled (details and evidence in §2 and §5):

| Question (layer5.md) | Answer | Tag |
|---|---|---|
| Does `SetStage` start a stopped quest? | yes | [observed] |
| `GetStage`: highest or last stage set? | the highest | [observed] |
| Does a lower stage's result script run? | yes, and its stage is marked done | [observed] |
| Same stage again, no "allow repeated stages" | the result script does not run again | [observed] |
| Same stage again, with the flag | the result script runs again | [observed] |
| Undefined stage | no effect | [observed] |
| When does the result script run? | inside `SetStage`, before it returns | [observed] |
| Complete / Fail Quest stage flags | set "completed" (also for Fail); neither stops the quest | [observed] |
| Quest script delay | the `DATA` delay whenever it is non-zero, whatever the "Default Script Processing Delay" flag says; delay 0 (flag clear) → 5 s; a delay under one frame → every frame | [observed] |
| short / long / int storage | no 16-bit wrap; truncation toward zero; out of 32-bit range → −2147483648 | [observed] |
| float storage | double precision | [observed] |
| Whole-number literals | saturate at 2147483647; a negative literal is the negation of that | [observed] |

## 1. How jobs name quests [observed, trials `q5_trial`, `q5_trial2`, 2026-10-09]

Jobs: `tools/capture/jobs/q5_trial.steps.json` and `q5_trial2.steps.json`
(`make_quest_jobs.py`), save `nvftest`.

- **Quest Editor IDs do not compile in job scripts.** `GetStage VCG01`,
  `GetQuestRunning VCG01`, `VCG01.fTimer` (read with `let` or `set`) all failed with
  "script did not compile". Prediction before the run was the opposite (quests keep their
  Editor IDs at run time like globals); refuted. Globals still compile (`ScroungerOdds`
  read 100).
- **Console lines with a quest Editor ID have no effect** in these unattended sessions:
  `set VMS22.VMS22DinoPic to 5` left the variable 0, `StartQuest VMQ03aTimer` left the quest
  stopped, although `RunScriptLine` reported success. Why is not known [hypothesis: the
  console resolves Editor IDs only for some form types at run time].
- **A quest in a ref variable works**: `let rQ := BuildRef <FalloutNV.esm index> <local FormID>`,
  then `GetStage rQ`, `GetStageDone rQ n`, `GetQuestRunning rQ`, `GetQuestCompleted rQ`,
  `GetObjectiveDisplayed rQ n`, `GetObjectiveCompleted rQ n`, `SetStage rQ n`,
  `StartQuest rQ`, `StopQuest rQ` all compile and act on the quest.
- **Quest variables by name through xNVSE**: `GetVariable "name" rQ` reads them and
  `SetVariable "name" value rQ` writes them. `GetVariable` of a name the quest script does not
  declare returns 0 without an error.
- `$` of a float gives six decimals (`0.333333`), so numbers are always returned as numbers.

So `capture_quests.py` builds every quest with `BuildRef` and reads and writes quest
variables with `GetVariable` / `SetVariable`. The vanilla `set quest.var to …` statement
cannot be issued from a job; what a Set statement stores was measured on the job script's
own locals instead (§5.5).

## 2. Quest state of the capture saves [observed, `q5_state_nvftest`, `q5_state_nvfdoc`]

Definitions from `capture_quests.py defs` (fnvtool esm decode of FalloutNV.esm, 18 quests:
the intro chain VCG00–VCG04, VCG01Test, VMQ01, VMQTops, Goodsprings quests VMS16, VMS16b,
VFreeformGoodsprings, VDialogueGoodspringsGeneric, VERGoodsprings01, vGSRadioQuest,
CGTutorial, and Generic, VMS22, VMQ03aTimer used by §5), 584 reads per save. State
documents: `goldens/quest/state.nvftest.json`, `state.nvfdoc.json` (private,
`fnv.quest.state/1`).

**Both saves are at the same point of the chain**: the only differences are VCG04's
player-position variables and VERGoodsprings01's random-encounter frame counter and roll.

| Quest | Running | Completed | GetStage | Stages done (of those defined) | Objectives | Variables (non-zero) |
|---|---|---|---|---|---|---|
| VCG00 | no | no | 100 | 0, 90, 95, 100 (not 5–85) | — | bRunTimer 1, fTimer −0.0140 |
| VCG01 | no | yes | 200 | all but 9, 17, 20, 37, 78 | 10, 30, 40, 50 displayed and completed | bRunTimer 1, fTimer −0.0160, bBeganTest 1, nDocReaction 4, nMostExtremeStat 7, fMaxDeviation 4.5, fCurrentValue −0.5, fAbsCurrentValue 0.5 (bChooseSex 0) |
| VCG01Test | no | no | 0 | — | — | the psych-exam scores (bDoFinalCalculation 2) |
| VCG02 | **no** | no | 5 | 5 | 3 displayed | — |
| VCG03 | no | no | 0 | none | none | — |
| VCG04 | yes | no | 0 | (no stages) | — | Initialized 1, borders set, Done 0 |
| VMQ01 | yes | no | 10 | 10 | 10, 25, 30 displayed | — |
| VMQTops, CGTutorial (stage 2 done), VFreeformGoodsprings (bVictorPatrol 1), VDialogueGoodspringsGeneric, VERGoodsprings01, vGSRadioQuest, Generic, VMS22 | yes | no | 0 (CGTutorial 2) | — | — | — |
| VMS16, VMS16b, VMQ03aTimer | no | no | 0 | none | none | — |

Reading [observed unless tagged]: VCG01 ("Ain't That a Kick in the Head") is complete; its
stage 200 started VMQ01 ("They Went That-a-Way", stage 10, objectives 10/25/30 shown) and
VCG04 (character revision, waiting for the player to leave the Goodsprings area). VCG02
("Back in the Saddle") has stage 5 done and objective 3 ("talk to Sunny") shown, but is
**not running**, although SetStage starts a stopped quest (§5.1); stage 5 is set by
VGenericTimer's countdown (`nEvent` 3 in the save) together with VCG01's 200 [observed in
the masters' source text]. Why VCG02 is stopped is open. VCG03 and the Goodsprings
side quests are untouched. VCG00's stages 5–85 are not done although its timers were
[hypothesis: the opening cinematic path sets only 0, 90, 95, 100 in this version]. Both
VCG00 and VCG01 stopped with `bRunTimer` 1 and `fTimer` a little below 0: the countdown
ran out and the stage switch then stopped the quest.

**Save `nvfnew` (owner, 2026-10-09 09:21; `q5_state_nvfnew`, same 18-quest defs,
`state.nvfnew.json` private) is before the intro chain** [observed]: VCG00 is running at
stage 90 (stages 0 and 90 done, `bRunTimer` 1, `fTimer` 3.0, a countdown still to run);
VCG01 is running with **no stage done** (GetStage 0, all of 0–200 not done, objectives
10/30/40/50 neither shown nor completed, every variable 0); VCG01Test and CGTutorial are
running with nothing done; VCG02, VCG03, VCG04 and VMQ01 are stopped and untouched. So it
starts before VCG01's stage 0 (the stage whose result script stops VCG00), earlier than
Doc Mitchell's first line (stage 3) and the name menu (stage 10): every VCG01 stage can be
captured from it.

**Neither `nvftest` nor `nvfdoc` is at the start of VCG01.** `nvfnew` is, and the chain
is captured from it in §10.

## 3. Predictions (written before each run)

Written in the session notes before the behaviour runs (2026-10-09, about 02:30, plan
"p3"; the delay follow-ups at 02:45 after the first delay results):

- P1 SetStage on a stopped quest starts it — **held**.
- P2 GetStage after 50 then 10 is 50 (highest) — **held**.
- P3 a lower stage's result script still runs — **held**.
- P4 without "allow repeated stages", setting a done stage again does not run its script — **held**.
- P5 with the flag it runs again — **held**.
- P6 an undefined stage changes nothing — **held**.
- P7 a Fail Quest stage leaves GetQuestCompleted 0 — **refuted** (1; the GECK wiki's
  GetQuestCompleted page says failed quests count as completed [documented]).
- P8 a Complete Quest stage with an empty script sets completed and leaves the quest running — **held**.
- P9 the result script has run when SetStage returns — **held** (also [documented: GECK
  wiki SetStage notes]).
- P10 VCG03 stage 10 sets its variable from the player's item count and enables its
  references — **held**.
- P11 a result script with if/else and AddItem (held-out scenario; result not given here).
- P12 integer variables truncate toward zero, no 16-bit wrap — **held**; the 32-bit edge
  (−2147483648 for out-of-range floats; saturating literals) was not predicted.
- P13 float variables hold doubles — **held**.
- P14 the "Default Script Processing Delay" flag gives the 5 s default — **refuted** (VCG01,
  flag set, DATA 0.01, runs every frame). Follow-up prediction (02:45): a non-zero DATA delay
  is used whatever the flag; DATA 0 gives 5 s — **held** for the visible scenarios.
- P15 a stopped quest's script does not run — **held**.
- P16 the first countdown drop after StartQuest is larger than a period — **mixed** (§5.4).

## 4. Contract for the candidate: `fnvtool script run` (workstream 5B)

```
fnvtool script run <data_dir> <plugin>... --schema <dir> --actions <file>   → fnv.script.trace/1 on stdout
```

This is docs/design/layer5.md §3.8 made concrete for the `L5.quest.*` scenarios. It
follows `fnv-sim`'s `sim run` (docs/spec-notes/simulation.md §7): the same load-order
arguments, the same naming of forms (global FormID `"0x%08X"`, Editor ID, or `"player"`),
the same item and reference operations, and observations keyed by `label`. The capture
side of the same file is `capture_quests.py job` / `golden`, so the inputs are run by the
original game and by the candidate unchanged.

### 4.1 Input `fnv.script.actions/1`

```json
{"format": "fnv.script.actions/1", "seed": 0, "frame_seconds": 0.016666666666666666,
 "title": "…", "save": "nvftest",
 "given": [{"kind": "quest", "quest": "VMS16", "running": false, "completed": false, "stage": 0,
            "done": [], "not_done": [5, 10], "objectives": [{"index": 5, "displayed": false}],
            "vars": {"nGangerDeathCount": 0}},
           {"kind": "global", "name": "ScroungerOdds", "value": 100},
           {"kind": "item_count", "of": "player", "item": "0x00136A1D", "count": 0},
           {"kind": "disabled", "of": "0x0015D982", "disabled": true}],
 "actions": [{"op": "set_stage", "quest": "VMS16", "stage": 50},
             {"op": "get_quest", "quest": "VMS16", "label": "VMS16 after stage 50",
              "fields": ["running", "stage", "done.50", "objective.10.displayed", "var.bMetJoeCobb"]}]}
```

- `frame_seconds`: the runner's fixed frame step (deterministic traces, layer5.md §3.4).
  The game ran at about 60 game loops per second [observed: 1,433 loops for 24 s,
  xnvse-capture.md §7].
- `save` is informational: which capture save the golden came from. The candidate cannot
  load saves; it starts from the new-game state of the load order and applies `given`.
- `given`: the facts of the save that the scenario depends on. The candidate **sets**
  them before action 0 (quest running/completed flags, current stage, stages done and not
  done, objective flags, quest variables by name, globals, item counts, disabled flags).
  They produce no observation. When the golden is built, every `given` is checked against
  what the save reported; a mismatch stops the golden (`capture_quests.py golden`), so a
  golden never exists for a precondition the save did not have.
- Quests are named by Editor ID; quest variables by name (the candidate maps the name to
  its index through the quest script's `SCVR`/`SLSD`, layer5.md §2).

Operations (all names as in the examples; a `label` makes an observation):

| op | fields | what it does | observation |
|---|---|---|---|
| `set_stage` | `quest`, `stage` | `SetStage quest stage` from a script | — |
| `set_stage_read` | `quest`, `stage`, `field`, `label` | `SetStage`, then reads one quest field **in the same script** | `{<field>: value}` |
| `start_quest`, `stop_quest` | `quest` | `StartQuest` / `StopQuest` | — |
| `set_quest_var` | `quest`, `var`, `value` | stores the number in the quest variable as it is (captured with xNVSE's `SetVariable`, §1; used only for values every variable type keeps exactly) | — |
| `set_local` | `type` (`short`, `long`, `int`, `float`), `value`, `label` | a Set statement `set x to <literal>` into a fresh local of that type, read back | `value` |
| `set_objective` | `quest`, `index`, `displayed` and/or `completed` (bool) | `SetObjectiveDisplayed` / `SetObjectiveCompleted` | — |
| `get_quest` | `quest`, `fields`, `label` | reads each field | one key per field |
| `sample_var` | `quest`, `var`, `frames`, optional `runs` and `skip`, `label` | reads the variable once per frame for `frames` frames | without `runs`: `changes` (int, how often the value changed); with `runs`: `drops`, `runs` changes after the first `skip`, each as previous − new value (the GetSecondsPassed countdowns of §5.4 make each drop the time between two runs of the quest script) |
| `wait_frames` | `frames` | lets frames pass | — |
| `add_item`, `remove_item`, `remove_all_items`, `enable`, `disable`, `set_global`, `activate`, `get_item_count`, `get_disabled`, `get_global` | as in `fnv.sim.actions/1` | as in `sim run` | `count` (int), `disabled` (bool), `value` (number) |

**Literals.** `set_local` writes `value` into the script as text the way the capture does
(`capture_quests.literal`): a whole number without a decimal point (`3000000000`), any
other number as its shortest round-trip decimal, never in exponent form (`0.00000001`).
The compiled form keeps numbers as text (layer5.md §2), so the candidate evaluates that
text the way the game's expression evaluator does; §5.5 shows that this matters
(whole-number literals saturate).

Quest fields: `running`, `completed` (GetQuestRunning, GetQuestCompleted; bool), `stage`
(GetStage; int), `done.<n>` (GetStageDone; bool), `objective.<n>.displayed`,
`objective.<n>.completed` (GetObjectiveDisplayed / GetObjectiveCompleted; bool),
`var.<name>` (the quest variable; number as stored). The walk-through runner also reads
`failed` (the quest's failed flag alone, which `completed` folds in, as GetQuestCompleted
does in the engine; no script function reads it).

**Frames between actions.** In the game every step takes at least one game loop, and the
capture pauses after changes; the quest scripts run in between. The candidate advances the
same number of frames so quest scripts get the same chances to run: `set_stage` and
`set_stage_read` 1 + 30; `start_quest`, `stop_quest`, `set_quest_var`, `set_objective` 1 + 10;
`set_local` 1;
the `fnv.sim.actions/1` operations as `capture_steps.py` schedules them (1 + 10, or 1 + 30
for `activate`, `remove_all_items`); `get_quest` 1 per field; `sample_var` 1 per frame;
`wait_frames` its `frames`. Scenarios whose observations depend on this count are
avoided (the timing scenarios compare `drops`, which do not).

### 4.2 Output `fnv.script.trace/1`

```json
{"format": "fnv.script.trace/1", "seed": 0, "frame_seconds": 0.016666666666666666,
 "observations": [{"label": "VMS16 after stage 50", "step": 2, "running": true, "stage": 50,
                   "done.50": true, "objective.10.displayed": false, "var.bMetJoeCobb": 0},
                  {"label": "VMQ03aTimer timer while running", "step": 5, "drops": [0.5, 0.5, 0.5]}],
 "events": [{"frame": 31, "kind": "stage_set", "quest": "0x00104EAE", "stage": 50},
            {"frame": 31, "kind": "objective_displayed", "quest": "0x00104EAE", "index": 50}],
 "final": {"quests": {"0x00104EAE": {"running": true, "stage": 50, "stages_done": [50]}}}}
```

- `observations`: one per labelled action, in action order, with `label`, `step` (the
  action index) and the fields above. A field that cannot be read is left out and the
  observation gets `"errors": {"<field>": "<why>"}`. The golden has the same shape, plus
  `diagnostics` (per `sample_var`: loops between changes, all drops; not compared) and
  `mod_indices`.
- `events` and `final` are the candidate's diagnostics (layer5.md §3.8: stage set,
  variable written, quest started/stopped/completed/failed, objective shown/completed,
  reference enabled/disabled, item added/removed, message shown, unhandled call, each
  with its frame). They are not compared by the `L5.quest.*` scenarios: the game shows
  only the states the jobs query, not an event log.

Lab comparator: **keyed** on `label` over `/observations` (every field the golden row
has; extra candidate fields are ignored); the timing scenarios compare with `abs_tol`
0.05 (seconds), the others exactly.

## 5. Behaviour [observed unless tagged; save `nvftest` unless named; each capture repeated once, see §6]

### 5.1 SetStage, GetStage, GetStageDone (`L5.quest.stage_order`, `.repeated_stage`, `.result_effects`)

VMS16 ("Ghost Town Gunfight"; its result scripts only change objectives), stopped, no
stage done:

| Step | running | GetStage | done | objectives (D displayed, C completed) |
|---|---|---|---|---|
| start | no | 0 | none | none |
| SetStage 50 | **yes** | 50 | 50 | 10 C (never displayed), 50 D |
| SetStage 10 (lower) | yes | **50** | 10, 50 | 5 C, 10 D (C cleared), 50 D |
| objective 10 hidden by script, SetStage 10 again | yes | 50 | 10, 50 | 10 stays hidden: **script not run** |
| SetStage 15 (no such stage) | yes | 50 | 15 not done | unchanged |
| SetStage 5 (lower) | yes | 50 | 5, 10, 50 | 5 D (its C cleared) |
| SetStage 110 (Fail Quest flag; script `StopQuest`) | no | **110** | 110 too | unchanged; GetQuestCompleted **1** |

- SetStage starts a stopped quest; GetStage is the highest stage done; a lower stage is
  still marked done and runs its result script; a done stage set again does not run its
  script when the quest lacks "allow repeated stages"; an undefined stage does nothing.
  These agree with the GECK wiki's SetStage and GetStage pages [documented].
- Generic ("allow repeated stages"): stage 70's script sets the global `ScroungerOdds` to
  25; reset to 0 and set again, it is 25 again: **the script runs every time**. GetStage
  stays 70 after the lower stage 60 (whose script ran: FortuneFinderOdds 90). GetStageDone
  is 1 for each.
- VCG03 stage 10 (no repeat flag): the result script's
  `Set VCG03.nHealingPowdersAtStart to Player.GetItemCount NVHealingPowder` gave 2 with two
  powders added, and the value was already there when read **in the job script that
  called SetStage, on the line after it**: the result script runs inside SetStage. The
  Xander root, broc flower and the first survival critter (disabled in the save) were
  enabled. After adding a powder and disabling the root, SetStage 10 again left the
  variable at 2 and the root disabled.

### 5.2 Objectives [observed, VMS16]

- `SetObjectiveCompleted q n 1` on an objective never displayed marks it completed without
  displaying it.
- `SetObjectiveDisplayed q n 1` **clears the completed flag** (objective 10, then 5: C → not C).
  These objectives were completed while hidden. On a displayed one it keeps the flag
  [observed: o5_00, 2026-10-31, VMS19 65 and VFSAtomicPimp 34: display, complete, display
  again → still completed; display 0 then 1 → not completed].

### 5.3 Complete and Fail Quest stage flags (`L5.quest.complete_flag`, `.stage_order`)

VMS22 (running, "allow repeated stages"): stage 90 (Complete Quest, empty script) →
completed 1, still running, GetStage 90. Then stage 100 (Fail Quest, empty script) →
completed stays 1, still running, GetStage 100. VMS16's Fail Quest stage 110, whose script
calls StopQuest: running 0, completed 1. So the flags set the completed state (failed
counts as completed, [documented: GECK wiki GetQuestCompleted]); only the script's
StopQuest stops a quest.

### 5.4 Quest script delay and GetSecondsPassed (`L5.quest.delay_*`)

Method: quest scripts that count a timer down by `GetSecondsPassed` in their GameMode
block. The job sets the timer to 100000 (and the script's run flag to 1), samples it every
game loop, starts the quest if it was stopped, and keeps the drop at each change: the
time between two runs of the quest script.

| Quest | flag "Default Script Processing Delay" | DATA delay | runs every | drops (s) |
|---|---|---|---|---|
| VMQ03aTimer | clear | 0.5 | 30 loops | 0.5 (0.4921875–0.5078125) |
| CGTutorial (running) | set | 1.0 | 60 loops | 1.0 |
| VCG01 | set | 0.01 | every loop | 0.015625 (sometimes 0.03125) |
| VCG00 | clear | 0.01 | every loop | 0.015625 |
| VMS40Timer | clear | 0 | 300 loops | 5.0 (4.984375–5.015625) |
| (held out) | | | | |

- **The DATA delay is used whenever it is non-zero, with or without the flag; 0 (flag clear) means 5 s**
  (the GECK wiki's SetQuestDelay page gives 5 s as the INI default, and 0 as "use the
  default" [documented]). A delay shorter than a frame runs the script every frame. What
  the flag does is not visible here [hypothesis: an editor-side default for the field].
- A stopped quest's GameMode does not run (no change in 120 loops, every case).
- **GetSecondsPassed comes in steps of 1/64 s** (0.015625): every drop is a multiple of it,
  while the game ran at about 60 loops per second (300 loops per 5 s)
  [hypothesis: the game's frame timer has the 15.625 ms resolution of the Windows system
  clock]. 0.5 s came as 32/64, sometimes 31 or 33.
- The first run after StartQuest is not regular: VMQ03aTimer, VCG00 and VCG01 ran within
  the 12 loops between StartQuest and the first sample (drops of 1/64), VMS40Timer only
  about 3.9 s after StartQuest, with a drop of 3.95 s [hypothesis: the time to a quest's
  first run depends on a delay counter that runs while the quest is stopped]. The
  scenarios therefore skip the first change seen (`skip` 1).

### 5.5 What a Set statement stores (`L5.quest.var_storage`)

`set xV to <literal>` with `xV` a local of the job script declared `short`, `long`,
`int` or `float`, read back in the same script. The compiled form records only Integer vs
FloatOrRef for a variable (`SLSD`), so a quest script's locals are expected to behave the
same [hypothesis].

| Literal | short / long / int | float |
|---|---|---|
| 32768, 40000, 65536, 70000, −32769, −40000 | unchanged (no 16-bit wrap) | — |
| 2.7, 2.5, 0.5, −0.5, −2.5, −2.7, 0.3333… | 2, 2, 0, 0, −2, −2, 0 (toward zero) | unchanged |
| 40000.5, 2147483647.5 | 40000, 2147483647 | — |
| 2147483647, 2147483648, 3000000000 (no decimal point) | 2147483647, **2147483647, 2147483647** | 3000000000 → **2147483647** |
| −3000000000 | **−2147483647** | **−2147483647** |
| 3000000000.5, −3000000000.5 | **−2147483648**, −2147483648 | unchanged |
| 0.3333333333333333, 0.1, 16777217, 0.00000001, 123456789.123, 4294967296.5 | — | unchanged (double precision) |

- Integer variables: truncation toward zero; out of the 32-bit range the result is
  −2147483648 [hypothesis: the x86 truncating conversion's "integer indefinite" value].
- **Whole-number literals saturate at 2147483647 even into a float**, and a minus sign
  negates the saturated value (−2147483647) [hypothesis: the literal is read as a 32-bit
  integer and negated as a separate step]; literals with a decimal point are doubles.
- Float variables keep doubles. xNVSE's `SetVariable`/`GetVariable` on quest variables
  also returned the doubles written (`fTimer` 100000, VMS22's variables).

### 5.6 Result script with if/else and AddItem (`L5.quest.result_additem`, save `nvfdoc`)

Held out; its result is in the private golden only.

### 5.7 ResetQuest and SetQuestDelay (o41, observer check 41, issue #1109)

Method (2026-10-31, save `nvftest`, job `o41_reset_delay` built with
`capture_quests.py`'s `set_quest_delay` / `reset_quest` ops; never saved): the §5.4
countdown on VMQ03aTimer (DATA delay 0.5 s). StartQuest, sample; `SetQuestDelay 2`,
sample; `ResetQuest`, read; set the timer again, sample; StartQuest, sample.

| Step | running | drops (s) |
|---|---|---|
| started, DATA delay | 1 | 0.5 (30 loops) |
| after SetQuestDelay 2 | 1 | 2.0 (119–120 loops) |
| after ResetQuest (timer set again) | **0** | none in 500 loops |
| after StartQuest | 1 | **2.0** (1.994–2.009) |

- **ResetQuest keeps a SetQuestDelay override** [observed]: after the reset and a new
  StartQuest the script ran every 2.0 s, not at the DATA 0.5 s.
- **ResetQuest set the script variables to 0** (fTimer 100000 → 0, bRunTimer 1 → 0)
  [observed], as the catalogue summary says [documented].
- **ResetQuest stopped the running quest** [observed]: GetQuestRunning 0 after it, and
  its GameMode did not run until StartQuest. Called here from a job script, not from the
  quest's own script (the Emergency Radio, VEFR01NCRGood2EmergencyRadio, resets itself in
  its MenuMode and goes on); whether a self-reset also stops it is not observed. The
  runtime stops the quest on a reset from any other script and keeps it running on a
  self-reset [hypothesis for the self-reset; issue #1232, observer check 66].

### 5.8 SetStage and objectives on a completed or failed quest (#2958, observer check 226)

Four jobs on save `nvftest`, 2026-11-18, each from a fresh load, reading GetQuestRunning,
GetStage, GetQuestCompleted, GetStageDone and GetObjectiveDisplayed / Completed of VMS16
(Ghost Town Gunfight) after each step [observed]:

- **Failed by its stage 110** (the Fail Quest flag; its script is `StopQuest VMS16`).
  `SetStage VMS16 10` then `SetStage VMS16 110`: running 0, stage 110, completed 1;
  objective 10 still displayed, not completed; VMS16b (Run Goodsprings Run) not running.
  - `SetStage VMS16 50` after it: GetStageDone 50 is 1, the quest stays stopped, and
    objective 50 is not displayed nor 10 completed (stage 50's script is
    `SetObjectiveCompleted VMS16 10 1`, `SetObjectiveDisplayed VMS16 50 1`).
  - `SetObjectiveDisplayed VMS16 50 1` and `SetObjectiveCompleted VMS16 10 1` given
    directly: no change.
  - `SetStage VMS16 100` after it: GetStageDone 100 is 1, still stopped, and objective 10
    reads completed (stage 100's script calls `CompleteAllObjectives VMS16`).
  - `StartQuest VMS16` after it: running 1, still completed.
- **Completed by its stage 100** (the Complete Quest flag; its script ends with
  `CompleteQuest`, `StopQuest`). `SetObjectiveDisplayed VMS16 60 1`: no change, stopped
  and again after `StartQuest`. `SetStage VMS16 50`: GetStageDone 1, still stopped.
- **Stopped without being completed** (`SetStage VMS16 10`, `StopQuest VMS16`): `SetStage
  VMS16 50` starts it again (running 1, stage 50, objective 50 displayed, 10 completed),
  and `SetObjectiveDisplayed VMS16 60 1` on the stopped quest displays it and starts the
  quest (as the wiki says of SetObjectiveDisplayed).

As built (`Game::set_stage`, `Game::set_objective`): on a completed or failed quest
SetStage marks the stage and runs its entries without starting the quest, and
SetObjectiveDisplayed / SetObjectiveCompleted do nothing; CompleteAllObjectives still
completes [hypothesis that the function did it and not stage 100's flag].

Why it was looked at: each of Joe Cobb's three "reason" lines (INFO 0x0015EC53, 0x0015EC54,
0x0015EF99) ends with `if GetStage VMS16 > 0, SetStage VMS16 110`, so giving him a reason
fails Ghost Town Gunfight in the original too, whatever the player says next; Run
Goodsprings Run (VMS16b) starts only on the lines that agree to help him or report Ringo
dead. Before this, NVForever then started the failed quest again and showed its next
objectives as the townspeople's lines set its stages.

## 6. Scenarios and goldens

`parity-lab/scenarios/layer5_quests.json`, comparator keyed on `label` over
`/observations`; inputs `parity-lab/inputs/layer5/quest_<name>.actions.json` (written by
`make_inputs.py` there); goldens `goldens/quest/quest_<name>.trace.json` (private). The
candidate command does not exist yet, so all twelve **error** in the lab until 5B lands
(checked: `python -m fnvlab run --layer 5` → 0/12 pass, 12 error).

| Scenario | Rows | Tolerance | Notes |
|---|---|---|---|
| `L5.quest.stage_order` | 8 | exact | §5.1–5.3 |
| `L5.quest.repeated_stage` | 7 | exact | §5.1 |
| `L5.quest.result_effects` | 7 | exact | §5.1 |
| `L5.quest.complete_flag` | 2 | exact | §5.3 |
| `L5.quest.var_storage` | 46 | exact | §5.5 |
| `L5.quest.delay_data`, `.delay_flagged_short`, `.delay_frame`, `.delay_flagged_1s`, `.delay_zero` | 4, 4, 4, 2, 4 | 0.05 s | §5.4 |
| `L5.quest.result_additem` | 3 | exact | **holdout** (save nvfdoc) |
| `L5.quest.delay_flagged_zero` | 4 | 0.05 s | **holdout** |

Stability: every job was run twice (plans p3/p5 and p6, separate game sessions). The second
run compared with the golden under each scenario's comparator scored 1.0 everywhere; the
delay drops differed by up to 1/32 s between runs (inside the 0.05 tolerance), everything
else was identical. Every `given` precondition matched the save in both runs.

Rebuilding (owner's machine; take the run lock first, never save in game):

```
python parity-lab/inputs/layer5/make_inputs.py
python tools/capture/capture_quests.py job parity-lab/inputs/layer5/quest_<name>.actions.json --name q5_quest_<name> --start <save> --defs C:/fnv-private/capture/quest/defs.json --data "<Data>" -o C:/fnv-private/capture/quest/q5_quest_<name>.steps.json
python tools/capture/capture_run.py --game "<FNV>" --save <save> --kill-on-timeout C:/fnv-private/capture/quest/q5_quest_<name>.steps.json
python tools/capture/capture_quests.py golden C:/fnv-private/capture/out/q5_quest_<name>.<time>.jsonl --actions parity-lab/inputs/layer5/quest_<name>.actions.json -o <somewhere outside goldens>/quest_<name>.trace.json
```

One scenario per game session (the jobs change quest state). `golden` refuses to write when
a `given` does not match the save.

## 7. Open items and owner review

- **A save at the start of a new game** (before Doc Mitchell speaks, VCG01 stage 0 or 1) is
  needed to capture the intro chain itself (layer5.md §4 item 4). It can only come from the
  owner: start a new game, and as soon as the player can look around, open the console and
  `save nvfnew` (the plain name lets `capture_run.py --save nvfnew` load it). Captures from
  it would drive VCG01's stages with SetStage and record stage, objectives, variables and
  the player's controls after each. (Done: `nvfnew`, §2 and §10.)
- VCG02 has stage 5 done but is not running in both saves (§2), against §5.1's "SetStage
  starts a stopped quest". Not explained.
- Job scripts cannot issue the vanilla `set quest.var to …` (§1); §5.5 rests on the job's
  own locals and the hypothesis that a quest script's locals store the same way.
- The first run after StartQuest (§5.4) is not modelled; the scenarios skip it.
- Not captured: the order of scripts within one frame, whether GameMode runs on the frame
  of an activation, object scripts (OnActivate, OnLoad), conditions (`CTDA`) on stage
  entries (none of the stages used has any). They need further jobs.

## 8. Round 2: stage entries, conditions, object scripts, timing (2026-10-09)

Second capture round (workstream 5C, branch `claude/layer5-captures-2`), made without
reading the runtime (`crates/fnv-game`, `crates/fnv-script/src/vm*`) or scripts.md §4 and
later, from the original game's behaviour only. Same install, plugin (v7) and saves as
§1–6; unattended through `capture_run.py`, one job per game session, nothing saved.
**No save `nvfnew` exists** (the Saves folder holds `nvftest`, `nvfdoc`, the owner's own
numbered saves and an autosave), so the intro quest VCG01 could not be captured (§7).

Diagnostic jobs: `tools/capture/jobs/q5b_trial_test{,2,3}.steps.json` (save `nvftest`)
and `q5b_trial_doc{,2,3}.steps.json` (save `nvfdoc`), written by
`tools/capture/jobs/make_quest2_jobs.py`. Scenario inputs:
`parity-lab/inputs/layer5/quest2_*.actions.json` (`make_inputs2.py`), scenarios
`parity-lab/scenarios/layer5_quests2.json`, goldens `goldens/quest2/` (private).
Candidates were found by scanning the masters' records and script sources (read-only).

### 8.1 Summary

| Question | Answer | Tag |
|---|---|---|
| (a) A stage with several log entries whose conditions all pass | **every entry's result script runs**, in entry order | [observed] |
| (b) AND/OR grouping in a condition list | **OR binds tighter than AND**: a run of conditions joined by OR flags is one group, the groups are ANDed; not left to right, not AND first | [observed] |
| (c) Stray ElseIf after its If chain was closed by EndIf | **its condition is tested and its body runs when true**; the stray EndIf after it does nothing visible | [observed] |
| (d) GameMode on the frame of an activation | runs (no run skipped or doubled); OnActivate itself runs **inside** the Activate call | [observed] |
| (d) OnAdd / OnLoad against GameMode | an item's GameMode acts on its OnAdd by the next frame; OnLoad after Enable runs by the next frame, not inside Enable | [observed] |
| (d) Quest scripts against object scripts within one frame | not settled (§8.3 d) | — |
| (e) GetSecondsPassed after a long menu or a loading screen | **the gap is not counted**: the first value after it is one ordinary frame (or the quest's own delay) | [observed] |
| (f) Object-script variables of a reference | xNVSE's `ref.GetVariable "name"` / `ref.SetVariable "name" value` read and write them | [observed] |
| (f) OnActivate of a scripted `Activate` | runs only with Activate's second parameter (run-OnActivate flag) set; without it the default activation happens and the block does not run, also on dead actors | [observed] |

### 8.2 Contract additions (`fnv.script.actions/1`, §4)

Additions only; every existing operation, field and observation is unchanged. Frames
are counted as in §4.1 unless given here.

| op | fields | what it does | observation |
|---|---|---|---|
| `activate` (existing) + `run_on_activate: true` | as in `fnv.sim.actions/1` | `Activate <actor> 1`: the target's OnActivate block runs instead of the default activation ([documented: GECK wiki Activate, RunOnActivateBlockFlag]); 1 + 30 frames | with `label`: `opened` (container menu open, as before) |
| `set_ref_var` | `ref`, `var`, `value` | sets a variable of the reference's object script (xNVSE `ref.SetVariable`); 1 + 10 | — |
| `get_ref` | `ref`, `fields`, `label` | reads `var.<name>` (object-script variable), `dead` (GetDead), `disabled`; 1 per field | one key per field (`dead`, `disabled` bool) |
| `activate_read` | `target`, `actor` (default `player`), optional `run_on_activate`, `read` = `{"ref", "field"}` (a `get_ref` field) or `{"quest", "field"}` (a `get_quest` field), `label` | Activate and the read **in the same script**; 1 + 30 | `{<field>: value}` |
| `sample_ref_var` | `ref`, `var`, `frames`, optional `runs`, `skip`, `values`, `label` | as `sample_var`, for an object-script variable | as `sample_var` |
| `sample_var` (existing) + `values: true` | | | adds `values`: the successive distinct values seen |
| `add_item_read` | `to`, `item`, `count`, `read` (items), `label` | AddItem (silent) and the counts of the `read` items held by `to`, in the same script; 1 frame, no pause | `count.<item>` (int) per item |
| `sample_items` | `of`, `items`, `frames`, `label` | the counts once per frame | per item: `first.<item>` (first sample), `final.<item>` (last), `settled.<item>` (index of the first sample from which the count stays final) |
| `menu_gap` | `via` (a container), `frames`, `quest`, `var`, `after`, `label` | the player activates `via` (its container menu opens); the menu stays open `frames` frames counting the opening frame; the quest variable is read in its last frame; the menu closes (console `CloseAllMenus`) and the variable is read once per frame for `after` frames | `drop`: the value read in the menu minus the value at the first change after it |
| `kill` | `target` | `Kill` (no killer); 1 + 60 | — |
| `evaluate_package` | `of` | `EvaluatePackage`; 1 + 60 | — |
| `is_current_package` | `of`, `package`, `label` | whether the actor's current package (xNVSE `GetCurrentPackage`) is `package`; 1 | `current` (bool) |

`given` gains the kind **`ref`**: `{"kind": "ref", "ref": <FormID>, "vars": {...},
"dead": bool, "disabled": bool}` (any subset), checked like the others.

### 8.3 Findings [observed unless tagged]

**(a) Several log entries in one stage.** ControlRumble (FalloutNV 0x00070EC9, allow
repeated stages) has five entries in stage 0, none with conditions; entry 0 resets
`Stage1Fired`…, entry 1 `Stage1FireOnTimerValue`…, entry 2 `stage1RumbleLeft`…, entry 3
`Stage1Duration`… and sets `timerMaxValue` to 10, entry 4 calls `StopQuest ControlRumble`.
With the five variables set to 7, `SetStage ControlRumble 0` left all of them 0,
`timerMaxValue` 10 and the quest stopped (`q5b_trial_test`; scenario
`L5.quest.multi_entry_stage`, both runs). So every entry whose conditions pass runs its
result script, in record order (entry 4's StopQuest came last: the quest ended stopped
although SetStage starts a stopped quest, §5.1). Both saves already show that end state
(stopped, stage 0 done, `timerMaxValue` 10), consistent with the quest script's own
`SetStage ControlRumble 0` at game start. No stage in the masters has two entries with
conditions that can both pass and observable, harmless effects, so an entry whose
conditions fail is not captured [hypothesis: it is skipped and the others still run].

**(b) AND/OR grouping.** No stage entry or objective in the masters mixes AND and OR in a
way a job can drive and see, so the check uses AI package conditions, which are evaluated
without dialogue (`EvaluatePackage`, then xNVSE's `GetCurrentPackage`; `q5b_trial_test`,
`q5b_trial_test2`, scenario `L5.quest.condition_groups`):
- GoodspringsFleePackage (0x00107480), the first package of three Goodsprings settlers:
  `VMS16.bGunFightStart == 1` (OR) / `VMS16b.bPowderGangAttacking == 1` /
  `VMS16.bTrudyHelp == 0`, i.e. A (OR) B, C. With A = 1, B = 0 and C false (bTrudyHelp 1)
  the settlers did **not** take it (they took their second packages, which need
  bGunFightStart 1); with C true they did; with B = 1 instead of A the same. So not
  `A OR (B AND C)`.
- SunnySmilesStayAtCurrentLocation (0x0010A21C), Sunny's third package (the two before it
  fail with `bMetSunny` 1 and VCG02 objective 10 not completed): objective 70 not shown,
  quest not completed, `GetStage VCG02 == 45` (OR) / `== 40` (OR) / `== 20`. At stage 20
  (and at 45) it was chosen with objective 70 hidden and **not** chosen with objective 70
  shown. Left to right (`((D AND Q AND S45) OR S40) OR S20`) would choose it; so not left
  to right.
- Together: a condition whose OR flag is set is ORed with the next one; such runs form
  groups, and the groups are ANDed: `(A OR B) AND C`, `D AND Q AND (S45 OR S40 OR S20)`.
  This is also how the masters' expression compiler orders `||` and `&&` (scripts.md §3.3).
  The run-on subject of these conditions is the package's actor; GetQuestVariable,
  GetStage, GetObjectiveDisplayed and GetQuestCompleted do not depend on it.

**(c) Stray Else/ElseIf/EndIf.** TecMineHostage (FalloutNV SCPT 0x00123191) compiles
`if GetDead == 0 … endif / elseif GetDead == 1 / activate / endif` in its OnActivate
block. Its only effect is that `Activate` (the default activation, a dead hostage's loot
menu), so it can be seen only when the block runs on a dead hostage. A scripted
`Activate player` never runs the block (finding f), so the first trials (dead hostage →
loot menu) showed nothing about the marker. With `Activate player 1` (`q5b_trial_test3`,
scenario `L5.quest.dead_activation`, both runs):
- a dead actor without an OnActivate block (a Techatticup legionary) → loot menu (default);
- ED-E's disabled body (dead; its OnActivate sets `vDialogueEDE.bEDEExamined` and shows a
  message, never calls Activate) → message box, variable 1, **no** loot menu: the block
  replaces the default on a dead actor too;
- the dead hostage → **loot menu**: the `activate` after the stray ElseIf ran;
- the living hostage with `Freed` 1 → nothing opens; with `Freed` 0 → its message box.
A clearer case is a quest script: BeatrixFX (0x0017BA11, script 0x0017B778, delay 0.1 s)
counts `timer` up with GetSecondsPassed; after `if timer > 1.5 && bump == 0 … endif` come
six `elseif timer > T && bump == n … endif` pairs (T = 2, 2.3, 2.5, 3.5, 4.5, 5.5) that set
`bump` to n + 1. After StartQuest, `bump` went 0 → 1 → 2 → … → 7, each step at the first
run past its threshold (timer 1.547, 2.062, 2.375, 2.547, 3.562, 4.547, 5.562 s), and at
9.078 s the closing `if timer > 9` stopped the quest (`q5b_trial_test3`; scenario
`L5.quest.stray_elseif`, both runs: values 0…7, then not running, `reset` 0, `bump` 7). So
an ElseIf reached after its chain was closed tests its condition and runs its body when
true. Whether a branch taken just before it would suppress it is not exercised (no two
thresholds fell in one run) [hypothesis: the marker acts as a new If].

**(d) Order and event timing.**
- GameMode on the frame of an activation: VCG01SkullBrahminActivator (Doc Mitchell's
  house; its GameMode counts `timer` down with GetSecondsPassed while `runTimer` is 1; its
  OnActivate block is empty), sampled every game loop, changed on every loop before,
  across and after `Activate player 1` (2–3 ticks of 1/64 s per loop; `q5b_trial_doc2`,
  `q5b_trial_doc3`): no run is skipped or doubled on the activation frame.
- OnActivate runs inside the Activate call: the house door's OnActivate
  (`set DocMitchellREF.PlayerLeft to 1`, then `activate`) had set the variable when it was
  read on the next line of the script that called `Activate player 1`, and the player was
  outside afterwards (scenario `L5.quest.ref_activation`, both runs).
- OnAdd and GameMode of an item: JarPistolPowder (OnAdd Player sets `killme`; GameMode
  adds 200 PowderPistol and calls RemoveMe). In the script that added the jar: jar 1,
  powder 0; at the first sample one frame later: jar 0, powder 200 (`q5b_trial_doc`;
  scenario `L5.quest.onadd_gamemode`, both runs). So an item's GameMode runs while it is
  in the player's inventory, and its OnAdd had run before that GameMode, at the latest in
  the same frame.
- OnLoad: the saloon jukebox (VintageRadioStartOnSCRIPT: OnLoad sets `init` to 1 when it
  is 0) and the broken radio (GoodspringsRadioScript: OnLoad calls `SetDestroyed 1`),
  disabled and enabled again while the player was in their cell: neither effect was there
  on the line after `Enable`; both were there at the first sample (`q5b_trial_test2`). An
  exterior case (VMS15's rising plane, enabled in its loaded cell) read `bLoaded` 1 already
  on the line after `Enable`, and its GameMode (`bLoaded` 1 → 2) did not run in 120 loops.
  Read again 2026-10-24 (trace `q5b_trial_test3`, private): the plane was disabled with
  `bLoaded` 0 before `cow WastelandNV 17 14`, and `PlaneFloated` was 0; its OnLoad is the
  only thing that sets `bLoaded` to 1 (only while `PlaneFloated` is 0). So its OnLoad ran
  between the two reads, while it was disabled: at the cell load (600 frames before the
  Enable), or inside Enable [observed]. The jukebox and the radio did not run OnLoad inside
  Enable, so the runtime takes the cell load: references disabled when their cell loads run
  OnLoad then, and again the frame after their Enable [hypothesis, the narrowest reading;
  `Game::set_loaded`, `Game::note_enabled`]. The GameMode that did not run is still not
  understood: a disabled skull's GameMode runs (`od_09a_disabled_ref_script`), and in the
  quest the bomber does rise after the detonator, which needs that GameMode [hypothesis:
  something of the capture's setup, e.g. the console Enable or the 3D not loading, held
  it]. Not emulated: here the plane's GameMode runs while it is disabled (`bLoaded` 2), and
  after the detonator's Enable it plays `Forward` once its 3D is loaded (smoke `b29-rise`).
- Quest scripts against object scripts within one frame: **not settled**. A job samples
  between frames, so the order shows only where a quest script and an object script write
  and read the same variable in one frame. The masters have no such pair that a job can
  drive without dialogue, message-box buttons or combat (searched: object scripts' GameMode
  blocks that read or write variables a quest script changes, and the reverse), and job
  scripts cannot attach scripts. The skull's object timer and VCG00's quest timer (both
  every frame) always dropped by the same amount in the same loop.

**(e) GetSecondsPassed after a gap.**
- Container menu: VCG00 (delay 0.01, so every frame; has a MenuMode block), VMQ03aTimer
  (delay 0.5; GameMode only) and the skull's object script, all counting down with
  GetSecondsPassed. While Doc's dresser's container menu was open (1,500 loops, 54 s of
  real time by the screenshot timestamps of `q5b_trial_doc3`) none of them changed; after
  `CloseAllMenus` the first drop was one ordinary frame for VCG00 and the skull (3/64 s,
  as before the menu) and 0.516 s for VMQ03aTimer (its normal 0.5 s). Scenario
  `L5.quest.seconds_after_menu` (900-frame menu): 0.046875 and 0.5. So the menu time is not
  counted at all (a cap would show as a larger, fixed value).
- Loading screen (`coc` to the saloon and back): VCG00 did not change for about 22 loops,
  then dropped by an ordinary frame (`q5b_trial_doc3`).
- The game clock (`GameDaysPassed`) advanced by 0.000118 days (10 game seconds at
  TimeScale 30, about 0.3 s of real time) during the 54 s container menu: the world clock
  stops in that menu too. (xnvse-capture.md §5 says the clock keeps running while the
  player is in menus; that was seen with the console and the Wait menu, not a container
  menu.)
- A Wait-menu gap was not captured (no unattended way to open the Wait menu; the
  `GameHour` substitute does not stop scripts).

**(f) Object scripts on references.**
- `rRef.GetVariable "name"` returns a variable of the reference's object script (the
  skull, Doc Mitchell, the hostages, the jukebox); a name the script does not declare
  returns 0; `rRef.HasVariable "name"` returns 1. `GetVariable "name" rRef` (the quest
  spelling of §1) returned 0 for a reference, and `rRef.timer` does not compile
  (`q5b_trial_doc`). `rRef.SetVariable "name" value` writes it (read back, and the
  skull's script counted down from the value written).
- **A scripted `ref.Activate player` does not run the target's OnActivate block**; it
  performs the default activation (a door moves the player, a corpse opens its loot menu,
  an activator does nothing). The door's OnActivate did not set `PlayerLeft`, dead ED-E's
  did not set `bEDEExamined` (`q5b_trial_test2`, `q5b_trial_doc2`; scenarios
  `L5.quest.dead_activation`, `L5.quest.ref_activation`). `ref.Activate player 1` runs the
  block instead (and the block's own `Activate` performs the default). This is the GECK
  wiki's RunOnActivateBlockFlag [documented].
- An actor's OnActivate block runs on a dead actor when it is run (flag set): ED-E's
  disabled body showed its message.

Other observations of this round:
- The game ran at about 25 loops per second in these sessions (the skull's per-loop drops
  of 2–3/64 s; round 1 saw about 60), so frame counts in jobs are not seconds.
- `$` of a number gives six significant digits (`99999.5`), not six decimals as §1 says;
  trial samples therefore encode countdowns as whole 1/64 s ticks.
- A container menu opened by a scripted Activate is open (`MenuMode 1008`) only one or two
  frames later; `CloseAllMenus` closes container menus and message boxes.

### 8.4 Predictions (written before each run; plans r2-p1 to r2-p4)

- Pa (all entries of ControlRumble's stage 0 run) — **held**.
- Pb (OR before AND for the flee package) — **held**; follow-up (not left to right, with
  Sunny's package) — **held**.
- Pc (a stray ElseIf reached in sequence is skipped) — **refuted** (BeatrixFX reaches 7;
  the dead hostage's flagged activation opened the loot menu).
- Pd (GameMode not skipped around an activation; OnActivate inside Activate; the jar
  replaced by the next sample) — **held**. OnLoad by the next frame, not inside Enable —
  **held** for the jukebox and the radio, **refuted** for the rising plane (its OnLoad
  had run at the cell load, while disabled; its GameMode stall is unexplained; §8.3 d).
- Pe (menu time not counted; made weakly, the alternative being the whole gap) — **held**.
- Pf (`rRef.GetVariable` works; `rRef.var` does not compile; the door's OnActivate sets
  PlayerLeft) — the first two **held**; the third **failed** until the run-OnActivate flag
  was used: a scripted Activate without it skips the block (not predicted).
- Scenario predictions (r2-p4) for the seven visible scenarios — all **held**.

### 8.5 Scenarios and goldens

`parity-lab/scenarios/layer5_quests2.json`, comparator keyed on `label` over
`/observations` (exact; `L5.quest.seconds_after_menu` with `abs_tol` 0.05 s); inputs
`parity-lab/inputs/layer5/quest2_<name>.actions.json`; goldens
`goldens/quest2/quest2_<name>.trace.json` (private). `L5.quest.multi_entry_stage` uses
only §4 operations; the others need the §8.2 additions, which the current candidate
rejects as unknown, so they **error** until 5B adds them (checked 2026-10-09 with this
branch's fnvtool: `python -m fnvlab run --scenario <id>` for each visible scenario →
`multi_entry_stage` passes, the six others error; the candidate was not run on the
holdouts).

| Scenario | Save | Rows | Notes |
|---|---|---|---|
| `L5.quest.multi_entry_stage` | nvftest | 2 | (a) |
| `L5.quest.condition_groups` | nvftest | 20 | (b) |
| `L5.quest.stray_elseif` | nvftest | 2 | (c) |
| `L5.quest.dead_activation` | nvftest | 9 | (c), (f) |
| `L5.quest.ref_activation` | nvfdoc | 5 | (d), (f) |
| `L5.quest.onadd_gamemode` | nvftest | 2 | (d) |
| `L5.quest.seconds_after_menu` | nvfdoc | 2 | (e); 0.05 s |
| `L5.quest.r2_holdout_a` | nvftest | — | **holdout** |
| `L5.quest.r2_holdout_b` | nvftest | — | **holdout** |

Stability: every scenario job ran twice, in separate game sessions (plans r2-p4 and
r2-p5). Every `given` matched the save in both runs (`golden` refuses otherwise), and the
second run scored 1.0 against the first under each scenario's comparator, holdouts
included. The only difference: VMQ03aTimer's drop after the menu, 0.5 then 0.484375
(inside the 0.05 s tolerance). The first run's traces are the goldens.

Rebuilding (owner's machine; both run locks first; never save in game): as §6, with
`make_inputs2.py`, a defs file for the quests the inputs name (ControlRumble, VTechatticup,
BeatrixFX, vDialogueEDE, VMS16, VMS16b, VFreeformGoodsprings, VCG02, VCG00, VMQ03aTimer)
and `-o C:/fnv-private/goldens/quest2/quest2_<name>.trace.json`.

### 8.6 Open items

- **VCG01 from a new game** still needs the owner's `nvfnew` save (§7).
- The order of quest and object scripts within a frame (§8.3 d).
- The rising plane's GameMode after Enable (§8.3 d; its OnLoad is read as a cell load
  while disabled, 2026-10-24). A job to tell "at the cell load" from "inside Enable": read
  `bLoaded` after the `cow` and before the Enable.
- `L5.quest.condition_groups` needs AI package choice in the candidate (the conditions are
  Layer 5's, choosing a package is Layer 6's).
- A Wait-menu gap for GetSecondsPassed.

## 9. Round 3: reputation, effect scripts, script order in a frame (2026-10-09)

Third capture round (workstream 5C, branch `claude/layer5-captures-3`), made without
reading the runtime (`crates/fnv-game`, `crates/fnv-script/src/vm*`) or scripts.md §4 and
later (only §10's open questions and capture proposals). Same install, plugin (v7) and
saves as §1–8; unattended through `capture_run.py`, one job per game session, nothing
saved. **There is still no save `nvfnew`** (Saves holds `nvftest`, `nvfdoc`, the owner's
numbered saves and an autosave dated 05:43, before this round; no round 3 run changed it),
so VCG01 could not be captured (§7).

Diagnostic jobs: `tools/capture/jobs/q5c_trial_rep.steps.json` (save `nvfdoc`),
`q5c_trial_fx.steps.json` (`nvfdoc`), `q5c_trial_order.steps.json` (`nvftest`), written by
`tools/capture/jobs/make_quest3_jobs.py`. Scenario inputs:
`parity-lab/inputs/layer5/quest3_*.actions.json` (`make_inputs3.py`; the two held-out
inputs by `make_inputs3_holdout.py`), scenarios `parity-lab/scenarios/layer5_quests3.json`,
goldens `goldens/quest3/` (private). Candidates were found by scanning the masters'
records (`fnvtool esm inventory` / `esm decode`) and script sources, read-only.

### 9.1 Summary

| Question | Answer | Tag |
|---|---|---|
| Deed sizes of AddReputation / RemoveReputation | 1, 2, 4, 7, 12 points for sizes 1–5, the same on every reputation (the GMSTs `fReputationBump*` read 1, 2, 4, 7, 12); other sizes do nothing | [observed] |
| Second argument (axis) | 1 = fame, 0 = infamy, for Get/Set/Add/Remove and Pct | [observed] |
| Limits | the result of AddReputation is at most the REPU's DATA value (the scale), of RemoveReputation at least 0; SetReputation stores any value (negative, above the scale) | [observed] |
| GetReputationPct | value / scale (negative or above 1 when the value is) | [observed] |
| GetReputationThreshold levels | per axis: level 0 below 15 % of the scale, 1 from 15 %, 2 from 50 %, 3 from 100 % | [observed] |
| The tier grid | a 4 × 4 grid of (fame level, infamy level), table in §9.3; the same for all five scales measured (10–70) | [observed] |
| ScriptEffectStart | runs inside the EquipItem call that uses the aid item | [observed] |
| A duration-0 effect | nothing of its script runs after the use | [observed] |
| ScriptEffectUpdate | runs every frame; an ability's Update adding ScriptEffectElapsedSeconds / 10 health gives 0.1 HP per second of GetSecondsPassed time: Elapsed is the time since the previous Update | [observed] |
| ScriptEffectFinish | 119.82–119.94 s of GetSecondsPassed time after the use for a 120 s effect | [observed] |
| Menus | a container menu pauses the duration (and Update) | [observed] |
| The same item again while active | durations add: one Finish, 240 s after the first dose | [observed] |
| Quest against object scripts in one frame | the quest script (BeatrixFX) ran before the object scripts of two windows in the same frame | [observed] |

### 9.2 Contract additions (`fnv.script.actions/1`, §4 and §8.2)

Additions only; every existing operation, field and observation is unchanged. Frames as in
§4.1 unless given here. Reputations are named like other forms (FormID or Editor ID of the
REPU); `axis` is `"fame"` or `"infamy"`.

| op | fields | what it does | observation |
|---|---|---|---|
| `set_reputation` | `rep`, `axis`, `value` | `SetReputation rep <1 fame / 0 infamy> value`; 1 + 10 | — |
| `add_reputation`, `remove_reputation` | `rep`, `axis`, `size` (integer) | `AddReputation` / `RemoveReputation` with that deed size; 1 + 10 | — |
| `get_reputation` | `rep`, `fields`, `label` | reads each field; 1 per field | one key per field |
| `set_reputation_read` | `rep`, `fame`, `infamy`, `fields`, `label` | sets fame, then infamy, and reads the fields **in the same script**; 1 | one key per field |
| `use_item` | `actor` (default `player`), `item`, `read`, `label`, optional `stopwatch: true` | the actor uses the aid item (`EquipItem item 0 1`, which consumes an ingestible [documented: GECK wiki EquipItem]); the `read` list is read in the same script: `{"av": name}` → `av.<name>`, `{"item": form}` → `count.<form>`; with `stopwatch` the stopwatch is read there too and becomes the zero of later `wait_av`s; 1 + 10 | one key per read |
| `wait_av` | `of` (default `player`), `av`, `value`, `max_frames`, `label` | waits, checking once per frame, until the actor value equals `value`, at most `max_frames` frames, then reads the stopwatch | `found` (bool); when found, `seconds`: stopwatch seconds since the last `use_item` with `stopwatch` |
| `hold_menu` | `via`, `frames` | the player activates the container `via`; its menu stays open `frames` frames; closed with the console's `CloseAllMenus`; 1 + frames + 5 | — |
| `damage_av`, `restore_av` | `of`, `av`, `value` | `DamageAV` / `RestoreAV`; 1 + 10 | — |
| `get_av`, `set_av` | as in `fnv.sim.actions/1` (`of`, `name`, `value`, `label`) | as in `sim run`; `set_av` 1 + 30 | `value` |
| `add_spell`, `remove_spell` | `of`, `spell` | `AddSpell` / `RemoveSpell`; 1 + 10 | — |
| `sample_av` | `of`, `av`, `frames` (≥ 2), `label` | the value and the stopwatch once per frame | `changes` (samples that differ from the one before), `per_second` (change of the value over the span / stopwatch seconds of the span) |
| `sample_av_damage` | `of`, `av`, `damage`, `frames`, `label` | once per frame: the value is read, then damaged by `damage` | `values` (the values read) |
| `go_to_cell` | `cell` (Editor ID) | the console's `coc`; 600 frames | — |
| `sample_jumps` | `reads` (≥ 2 of `{"global"}`, `{"ref", "var"}`, `{"quest", "var"}`), `frames`, `min_step`, `label` | every read once per frame | `lags`: for each read after the first, the index of the sample where it first changed by at least `min_step` minus that of the first read (`null` if it never did) |

Reputation fields: `fame`, `infamy` (GetReputation), `pct.fame`, `pct.infamy`
(GetReputationPct; numbers), `threshold.mixed`, `threshold.good`, `threshold.bad`
(GetReputationThreshold with 0, 1, 2; integers).

`given` gains `{"kind": "reputation", "rep", "fame", "infamy"}` and `{"kind":
"actor_value", "of", "av", "value"}`, checked like the others.

**The stopwatch.** In the game it is VCG00 (FalloutNV 0x00102037), whose GameMode counts
`fTimer` down by GetSecondsPassed every frame while `bRunTimer` is 1 (delay 0.01, §5.4).
Before the first action of an input that uses `use_item` with `stopwatch`, `wait_av` or
`sample_av`, the job sets `fTimer` to 100000 and `bRunTimer` to 1 and starts the quest; the
stopwatch reads 100000 − `fTimer`. No observation reads VCG00 itself. For the candidate the
stopwatch is the script time GetSecondsPassed counts (frames × `frame_seconds`, stopped
while a menu is open, §8.3 e). In the game its resolution is one frame (`wait_av` reads it
a frame after the condition first held) and 1/64 s (§5.4), and the game ended effects up to
0.2 s early (§9.3 b), so the effect-duration scenarios compare `seconds` within 0.25 s.

Text results joined with `|` (`set_reputation_read`, `use_item`, `wait_av`, `sample_av`,
`sample_jumps`) carry six significant digits (`$`, §8.3); hence the reputation scenarios'
tolerance of 0.0001 (the thresholds are integers and compare exactly within it).

### 9.3 Findings [observed unless tagged]

**(a) Reputation** (`q5c_trial_rep`, save `nvfdoc`; scenario `L5.quest.reputation`). All 13
FalloutNV reputations are 0/0 in `nvfdoc` (read at the start of the trial). Every
reputation function compiles and runs with the REPU in a ref variable (BuildRef), like
quests (§1). GetReputation returns its value in a `let` (the GECK wiki's "only inside an
if" was not seen). Only reputations that no quest script watches in GameMode and whose
members are far from the saves were changed: White Glove Society (scale 10), Novac (20),
Great Khans (30), Followers (50), Freeside (70) (scale = the REPU's DATA value, read with
`fnvtool esm decode`).
- The second argument is 1 for fame and 0 for infamy, for all six functions.
- Deed sizes 1–5 add 1, 2, 4, 7, 12 points on White Glove Society and on Freeside alike
  (not scaled), and RemoveReputation subtracts the same. xNVSE's `GetNumericGameSetting`
  reads `fReputationBumpVeryMinor` … `VeryMajor` as 1, 2, 4, 7, 12 (no FalloutNV GMST record
  sets them), as the GECK wiki's AddReputation page says [documented].
- AddReputation stops at the scale (9 + 12 → 10 on White Glove Society, 69 + 12 → 70 on
  Freeside) and even pulls a value set above the scale down to it (20 + 1 → 10, 140 + 1 →
  70; scenario run); RemoveReputation stops at 0 (1 − 12 → 0) and does not pull a value above the
  scale down to it (20 − 1 → 19 on a scale of 10). Sizes 0, 6 and −2 change nothing.
  SetReputation stores −3, 2.5 and twice the scale as given.
- GetReputationPct = value / scale, including −0.3 and 2.
- The values read in the script that called SetReputation equal those read two frames
  later.
- **Levels.** One axis swept from 0 to 1.2 × scale in steps of scale/1000 (the other at 0)
  on all five reputations: the codes change exactly at 0.15, 0.5 and 1.0 × scale, and a
  value equal to the boundary is already at the higher level (fame 1.5 of 10 → Accepted).
  So per axis: level 0 below 15 %, 1 from 15 %, 2 from 50 %, 3 from 100 % of the scale. A
  fame or infamy below 15 % counts as none (fame 1 of 10: all three axes 1).
- **Grid.** Fame and infamy each at 0, 5, … 100 % of the scale (21 × 21) on all five
  reputations: identical tables, a 4 × 4 grid of the two levels. GetReputationThreshold
  (mixed, good, bad):

| fame level \ infamy level | 0 (< 15 %) | 1 (15–50 %) | 2 (50–100 %) | 3 (≥ 100 %) |
|---|---|---|---|---|
| 0 | 1, 1, 1 | 0, 0, 4 | 0, 0, 5 | 0, 0, 6 |
| 1 | 0, 4, 0 | 3, 0, 0 | 0, 0, 2 | 0, 0, 3 |
| 2 | 0, 5, 0 | 0, 2, 0 | 4, 0, 0 | 2, 0, 0 |
| 3 | 0, 6, 0 | 0, 3, 0 | 2, 0, 0 | 5, 0, 0 |

  Exactly one axis is non-zero, except with neither fame nor infamy, where all three are 1
  (as the wiki says). Fame 2 with infamy 3 and fame 3 with infamy 2 share mixed tier 2;
  which title the game shows for each is not visible through these functions
  [hypothesis: it depends on which level is higher].

**(b) Effect scripts of aid items** (`q5c_trial_fx`, save `nvfdoc`; scenarios
`L5.quest.effect_start`, `L5.quest.effect_duration`). Items whose base effect has the
Script archetype and an observable script: Cateye (CateyeBaseEffect, 120 s: Start sets the
player's Nighteye to 1, Finish to 0), Nuka-Cola (NukaColaAddCap, no duration: Start adds a
cap), Contreras' medical supplies (RestoreAllLimbs, duration 0: Start, Update and Finish
each restore the six limb conditions); for Update, the ability ImplantRegenBoost
(ImplantRegenScript: each Update restores ScriptEffectElapsedSeconds / 10 health).
- **Start runs inside the use.** In the script that called `player.EquipItem <item> 0 1`,
  on the next line: one cap more (18 → 19), Nighteye 1, the damaged head condition back
  at 100; the item count is already one lower.
- **Duration 0.** After the supplies restored the head inside the call, the head was
  damaged again in the same script and then once per frame: nothing restored it in the
  next 20 frames. So whatever of Update and Finish runs for a duration-0 effect runs inside
  the call as well [hypothesis: Start, one Update and Finish in a row].
- **Update.** With the ability added (health damaged by 40), health rose by 0.1 HP per
  second of the stopwatch (0.0997 over 2.4 s after a menu; Nuka-Cola's 2 HP/s healing ran
  at the same time in the trial and was subtracted; the scenario has no other healing);
  without the ability, 0. A cumulative ScriptEffectElapsedSeconds would make the rate grow,
  so Elapsed is the time since the previous Update. In the scenario (no other healing)
  health changed in 59 of 59 frame steps, at 0.101 HP/s: **Update runs every frame**.
  While a container menu was open, health did not change (no Update in a menu).
- **Finish.** Cateye used at stopwatch 18.516 s; Nighteye back to 0 at 138.453 s: 119.94 s,
  i.e. the 120 s duration counted in GetSecondsPassed time (2,992 game loops, 120 s of real
  time by the screenshots: 25 loops/s). Over all captures the reading was 119.82–119.94 s,
  although it is up to a frame late: the game ends the effect up to 0.2 s of stopwatch
  time before 120 s [hypothesis: the effect counts unrounded frame times while
  GetSecondsPassed comes in 1/64 s steps, or the use frame counts]. The duration
  scenarios therefore compare `seconds` within 0.25 s.
- **Menus pause it.** A second Cateye, with Doc's dresser's container menu open for 31 s of
  real time (screenshots) during the effect: Finish again 119.94 s of stopwatch time after
  the use (159 s of real time). GameDaysPassed also stood still in the menu (§8.3 e).
- **Taking it again while active.** A third Cateye, then a fourth 10.2 s later: Nighteye
  went back to 0 only 239.87 s after the third (229.7 s after the fourth); set to 1 again
  then, it stayed 1 for 3,000 more loops. So the second dose of the same item added its
  120 s to the active effect, with one Finish, not two [hypothesis: effects of the same
  item merge; whether two different items with the same base effect merge is held out].

**(c) Quest against object scripts in one frame** (`q5c_trial_order`, save `nvftest`;
scenario `L5.quest.script_order`). Round 2 found no quest/object pair sharing a variable
(§8.3 d). This round searched all ten masters again (object scripts of inventory items,
which a job can add; references near the saves; stage results that write reference
variables; globals written by a quest script and read by an object script) and found a
pair through **GameHour**: BeatrixFX's GameMode adds one hour to GameHour about 9 s after
it starts (round 2's quest, §8.3 c), and Goodsprings' house windows (WastelandWindowScript)
copy GetCurrentTime into their variable `Time` in GameMode every frame. Two windows in the
Goodsprings exterior cell were used, after a `coc Goodsprings`.
- Sampled once per game loop (GameHour, both windows' `Time`), the windows' `Time` equals
  GameHour to six digits in every sample, also in the sample where GameHour first shows
  BeatrixFX's extra hour (10.4853 → 11.4854 for all three). Had the windows' scripts run
  before the quest script in that frame, they would have shown the old hour for one more
  sample. **So the quest script ran before the windows' object scripts in that frame.**
- Calibration: when the job itself adds an hour (at the plugin's point in the loop), the
  windows still show the old hour in the same script and the new one in the next sample.
- Equal values in every sample also mean that the game clock advances before the windows'
  scripts run and does not change between them and the next sample.
- One quest, one kind of object script: whether all quest scripts run before all object
  scripts, and where effect scripts and inventory items' scripts come, is [hypothesis].

### 9.4 Predictions (written before each run; plans r3-p1, r3-p2)

r3-p1 (before the three trials):
- Pr1 the reputation functions compile with the REPU in a ref variable — **held**.
- Pr2 axis 1 = fame, 0 = infamy; sizes 1–5 = 1, 2, 4, 7, 12 points on any scale — **held**;
  the clamp at the scale and the ignored sizes 0, 6, −2 were not predicted.
- Pr3 RemoveReputation subtracts the same and stops at 0 — **held**.
- Pr4 SetReputation keeps values above the scale, clamps negative ones to 0 — **half
  refuted** (−3 is kept too).
- Pr5 GetReputationPct = value / scale — **held**.
- Pr6 fame-only levels at fixed fractions, guessed 25/50/75 % — **refuted** (15/50/100 %).
- Pr7 same-script read = next-frame read — **held**.
- Pf1 ScriptEffectStart inside EquipItem — **held**.
- Pf2 Finish after 120 s ± 0.1 of GetSecondsPassed time — **held** (119.94 s).
- Pf3 a container menu pauses the duration — **held**.
- Pf4 a second Cateye while active makes a second instance with its own Finish —
  **refuted** (one Finish, durations added).
- Pf5 a duration-0 effect's Update/Finish run once on the next frame — **refuted** (nothing
  after the call).
- Pf6 the ability's Update every frame, 0.1 HP/s, nothing in a menu — **held** (rate and
  menu in the trial; every frame in the scenario: 59 changes in 60 samples).
- Po1 calibration (the windows see the job's hour at the next sample) — **held**.
- Po2 the quest script runs before the object scripts in a frame — **held**.

r3-p2 (before the scenario jobs): every visible scenario as the trials showed, the new
AddReputation-from-above case guessed as "clamped to the scale" (**held**); for the
held-out scenarios the predictions are in the session notes only (results not given
here).

### 9.5 Scenarios and goldens

`parity-lab/scenarios/layer5_quests3.json`, comparator keyed on `label` over
`/observations`; inputs `parity-lab/inputs/layer5/quest3_<name>.actions.json`; goldens
`goldens/quest3/quest3_<name>.trace.json` (private). They use §9.2's additions, which the
current candidate rejects as unknown, so they **error** until 5B adds them (checked
2026-10-09 with this branch's fnvtool: `python -m fnvlab run --scenario <id>` for
`L5.quest.reputation`, `.effect_start` and `.script_order` → error each; fnvtool's parser
names the first unknown variant of each input; the candidate was not run on the
holdouts).

| Scenario | Save | Rows | Tolerance | Notes |
|---|---|---|---|---|
| `L5.quest.reputation` | nvfdoc | 95 | 0.0001 | §9.3 a |
| `L5.quest.effect_start` | nvfdoc | 10 | 0.01 | §9.3 b (Start, duration 0, Update) |
| `L5.quest.effect_duration` | nvfdoc | 8 | 0.25 s | §9.3 b (Finish, menu, second dose); about 9 min of game time |
| `L5.quest.script_order` | nvftest | 2 | exact | §9.3 c; travels to Goodsprings with `coc` |
| `L5.quest.r3_holdout_a` | nvfdoc | — | 0.0001 | **holdout** |
| `L5.quest.r3_holdout_b` | nvfdoc | — | 0.25 s | **holdout** |

Stability: every scenario job ran twice, in separate game sessions (plans r3-p2 run 1 and
run 2). Every `given` matched the save in both runs (`golden` refuses otherwise), and the
second run matched the first under each scenario's comparator, holdouts included
(0 differences in all six). Numbers that differed: the effect durations (119.82 and
119.93 s, 119.87 and 119.88 s, 239.84 and 239.86 s; held-out B within 0.02 s) and the
ability's rate (0.1014 and 0.1006 HP/s); everything else was identical. The first run's
traces are the goldens.

Rebuilding (owner's machine; both run locks first; never save in game): as §6, with
`make_inputs3.py` / `make_inputs3_holdout.py`, a defs file for BeatrixFX and VCG00
(`capture_quests.py defs … --quest BeatrixFX --quest VCG00`), `--start nvftest` for
`script_order` (nvfdoc for the others) and `-o
C:/fnv-private/goldens/quest3/quest3_<name>.trace.json`.

### 9.6 Open items

- **VCG01 from a new game** still needs the owner's `nvfnew` save (§7).
- Script order: one quest script against one kind of object script (§9.3 c). Effect
  scripts, inventory items' scripts and actors' scripts against quest scripts in a frame
  are not captured; MenuMode quest delays still not (scripts.md §10).
- Which title the original shows for each of the two cells of mixed tier 2, and anything that
  is shown only in the Pip-Boy, are not visible through script functions.
- Effects: a duration-0 effect's Update and Finish run inside the call or not at all (not
  distinguishable with RestoreAllLimbs, whose three blocks do the same). Effects on actors
  other than the player, abilities' Start/Finish, and `ScriptEffectElapsedSeconds` read in
  a script were not captured.
- Reputation: only five of the 13 FalloutNV reputations were changed (the others are
  watched by quest scripts or near the saves); all five follow the same fractions, so the
  rule is taken as general [hypothesis for the other eight and the DLC ones].

## 10. Round 4: the intro chain from the new-game save `nvfnew` (2026-10-09)

Fourth capture round (workstream 5C, branch `claude/layer5-captures-4`), made without
reading the runtime (`crates/fnv-game`, `crates/fnv-script/src/vm*`) or scripts.md §4 and
later (only §10's open questions). Same install and plugin (v7) as §1–9; save `nvfnew`
(§2: VCG00 at stage 90, VCG01 running with nothing done). Unattended through
`capture_run.py`, one job per game session. The stage results, INFO result scripts and
object scripts named below were read from the masters' source text (`SCTX`, with `fnvtool
esm decode`); facts from them are [observed in the masters], facts from the runs
[observed].

**One run saved the game.** VCG01 stage 55's result script calls `autosave`. Trial
`q5d_trial_drive1` drove the chain through 55 and the game wrote `autosave.fos` at the
moment stage 55 ran (09:45:56; the Saves folder listing changed during that run only),
although both INI files on the install have `bAllowScriptedAutosave=0` [observed]. The
owner's previous autosave (05:43) is now `autosave.fos.bak`; the older `.bak` was
replaced. Nothing was written back (no writes to the Saves folder). No other run reached
stage 55 (the run script compares the Saves folder listing before and after every run;
unchanged in all other runs). Every scenario input avoids stage 55 (§10.6).

Files: trial jobs written by `tools/capture/jobs/make_quest4_jobs.py` into
`C:\fnv-private\capture\quest4\` (6–16 MB each, not committed: every state sample carries
a scan of 90 menu codes); scenario inputs `parity-lab/inputs/layer5/intro_*.actions.json`
(`make_inputs4.py`, the holdout by `make_inputs4_holdout.py`); scenarios
`parity-lab/scenarios/layer5_intro.json`; goldens `goldens/intro/` (private).

### 10.1 Summary

| Question | Answer | Tag |
|---|---|---|
| What happens after loading nvfnew with no input? | Nothing moves: one pack plugin's message box is open (`MenuMode 1001`) and holds GameMode; VCG00's countdown (fTimer 3.0) and the world clock stand still (GameDaysPassed +0.00007 in 15 s) | [observed] |
| Which box? | One of the four pack plugins' "items added" boxes; which one opens first varies (Tribal Pack in six of the sessions where it was identified, Classic Pack in four); the others follow one at a time as their quests run, each after the previous one is closed | [observed] |
| Does a key press close it? | xNVSE `TapKey 28` (Enter) did not; the console's `CloseAllMenus` did, like the player's OK (the next pack box then opened and its items were given) | [observed] |
| Which stages fire on their own? | All but the player's choices: VCG00 95 and 100, VCG01 0, 1, 3 by timers; 5, 8, 10, 25, 30, 36, 45, 55 at the end of Doc Mitchell's lines; 7, 15, 35, 50 by timers; 40 inside the race menu (VCG01's `MenuMode 1036` block) | [observed] |
| Where does the player have to act? | The pack boxes; the name menu (stage 10, `MenuMode 1051`); the race menu (36, 1036); walking to the vigor tester (its trigger sets 60); using it (OnActivate: SPECIAL menu 1074, stage 65); the couch and the psych test (dialogue choices, ending in 85); the tag-skill menu (90, 1048); the trait menu (102, 1084); the exit trigger (115); Doc's farewell dialogue (Pip-Boy, hardcore choice, VGenericTimer event 3 → 200) | [observed]; the trigger, couch and dialogue scripts [observed in the masters] |
| Does a menu stop script time? | Yes: the stopwatch (an object script's GetSecondsPassed timer) did not move in 120 frames of the name menu; quest timers resumed where they stopped (15 came 1 s after the name menu closed) | [observed] |
| Timer accuracy | Every timer step came within 0.09 s of its fTimer sum (3.0, 0.2, 2.8, 3.25, 1.0, 3.0), read one frame late; two sessions differed by at most 1/64 s | [observed] |

### 10.2 The opening without input [observed, `q5d_trial_open`]

State lines of VCG00/VCG01 every three game loops (about 40 loops per second in these
sessions), screenshots every 50 samples:
- From the load: a pack box, the screen white behind it (VCG00 stage 0's `imod
  WhiteISFX`); VCG00 stage 90, fTimer 3.0; VCG01 0, nothing done; player controls
  disabled except looking (`GetPlayerControlsDisabled` per control: 1 1 1 0 1 0 0),
  Victor disabled, no Pip-Boy, the player in Doc Mitchell's house (cell 0x00103DF9) at
  the start marker, Doc 60 units away; TimeScale 30. Four rounds of `TapKey 28` and 2,000
  loops changed nothing.
- `CloseAllMenus` → GameMode ran: VCG00 fTimer fell (2.834 three loops later) and the
  object-script stopwatch started; 6, 8 and 19 samples later the next pack boxes opened
  (each stopping both again). After the fourth close, all four packs' items were given
  (`VDLCPackQuest.nGiven` 4) and no box came back.
- Then, by the stopwatch (seconds of script time from the first close): VCG00 95 at 3.04,
  100 one sample later with VCG00 stopped and VCG01 0 done in the same sample (stage 100
  sets VCG01 0, whose script stops VCG00), VCG01 1 at 3.29 (0.2 s), 3 at 6.13 (2.8 s),
  Doc talking; 5 at 9.05 (the end of Doc's first line), 7 at 12.30 (3.25 s), 8 one sample
  later (stage 7 sets fTimer 0), 10 at 27.81 (the end of Doc's three next lines) with menu
  1051 open, the screenshot showing "Enter character name". Nothing changed for the rest
  of the run (900 samples): the chain waits for the name.

Stage 10's source line is `GetPlayerName;` [observed in the masters]; the name menu
opened at that stage [observed]. Menu code 1004 reads open the whole time while
`MenuMode` (no code) reads 0 [observed; hypothesis: the HUD].

### 10.3 Driving the chain [observed, `q5d_trial_drive1`, `q5d_trial_drive2`]

Menus closed with `CloseAllMenus`, stages waited for with `GetStageDone` polled every
loop. Natural order (`q5d_trial_drive1`), seconds of script time from the first box
closed:

| Stage | Seconds | Set by | State seen |
|---|---|---|---|
| VCG00 95, 100; VCG01 0 | 3.0 (+1 frame) | VCG00's countdown; 100's script | VCG00 stopped, completed 0; Victor disabled, house door destroyed, Pip-Boy 0 |
| VCG01 1, 3 | +0.2, +2.8 | VCG01's timer | Doc speaks |
| 5, 7, 8 | line, +3.25, +1 frame | Doc's line; timer | |
| 10 | +15.4 (three lines) | Doc's last line | name menu 1051; stopwatch frozen |
| 15 | 1.0 after the close | timer | |
| 25, 30 | lines | Doc's lines | |
| 35, 36 | +3.0; line | timer; Doc's line | race menu 1036 open, **40 already done** in it |
| 45, 50 | lines; +3.0 | Doc's lines; timer | |
| 55 | lines (10 s) | Doc's last line | objective 10 shown, CGTutorial 2, controls: move and look enabled; **autosave written** |
| 60 | set by the job | (the tester trigger's SetStage) | Doc's next line shows objective 30 (10 s later) |
| 65 | inside Activate | the tester's OnActivate (`Activate player 1`) | SPECIAL menu 1074; objective 30 completed, tester destroyed |
| 70 | 1.0 after the close | timer, Doc's reaction block | nDocReaction 0, nMostExtremeStat 5, bLow 1, fMaxDeviation 0.5 |
| 75, 76, 79 | end of Doc's reaction line | the line's result; 75's and 76's scripts | all three done in one frame |
| 80 | 13 s of lines | the psych intro lines | objective 40 shown |

CGTutorial 2 (set by 55) has two entries whose conditions select the PC or the console
movement message (functions 309 and 523 [observed in the masters]); no message box was
open in the 30 s after it [observed]. The three tutorial waits timed out.

**Reaction block.** The player's SPECIAL are all 5 in nvfnew: Intelligence is checked
first (deviation 0.5, bLow 1), the others tie and do not replace it (`>`), so
nMostExtremeStat 5, and below the reaction thresholds nDocReaction stays 0 [observed;
matches the source].

**Without stage 55** (`q5d_trial_drive2`): with the first pack box still open (no
GameMode), `SetStage VCG01 0` stopped VCG00 at 90 (95, 100 never done) and set VCG01's
timer (0.2, frozen); `SetStage VCG01 60` and objective 30 shown by the job. After the
boxes, Doc's 60 `SayTo` played the first line of his topic again (its result set 5;
GetStage stayed 60, so no timer stage followed). From there every later step ran as in
the natural order, and:
- **The psych test** (`VCG01Test`): the job set the scores the answers' result scripts
  add (sneak 2, science 3, speech 1, small guns 2, medicine 1, survival 1) and
  `bDoFinalCalculation` 1; within one quest delay (0.5 s) the quest script tagged Guns,
  Science and Sneak (sneak bias: `nSkillsLeftToBeat` 2), set `bDoFinalCalculation` 2 and
  stopped the quest.
- 85 → 90 after 1 s (tag-skill menu 1048), 95 1 s after closing it, 98 after Doc's lines,
  100 after 1 s, 102 on the next frame (fTimer 0) with the trait menu 1084, 105 1 s after
  closing it, 110 after Doc's lines (objective 50 shown).
- 115 (the exit trigger's SetStage, set by the job): Doc's package moved him, but no
  dialogue menu opened in 1,800 loops.
- The farewell dialogue's result scripts were then run by the job: adding and equipping
  the Pip-Boy **ended the game** (the job's next step never ran; no end line).

**Stage 200** (`L5.intro.complete`, both runs): VGenericTimer with fTimer 0.1 and nEvent
3, started → VCG01 200 done, VCG01 **completed and stopped**, GameHour 8.005; VMQ01
running at 10 with objectives 10, 25, 30 shown (20 not); **VCG02 running** at 5 with
objective 3 shown; VCG04 running; VGenericTimer stopped (its own StopQuest), nEvent 3;
the cemetery map marker enabled. So VCG02's stopped state in `nvftest`/`nvfdoc` (§2, §7)
came after VCG02 stage 5, not from it [observed].

### 10.4 Contract additions (`fnv.script.actions/1`, §4, §8.2, §9.2)

Additions only. Frames as in §4.1 unless given. Menu codes are `MenuMode` codes; those
seen: 1001 message box, 1036 race menu, 1048 tag-skill menu, 1051 name menu, 1074 the
vigor tester's SPECIAL menu, 1084 trait menu [observed with the stage that opened each].

| op | fields | what it does | observation |
|---|---|---|---|
| `start_stopwatch` | — | zero of `seconds` (below); 1 + 2 | — |
| `set_quest_var` (existing) + `stopwatch: true` | | sets the variable and starts the stopwatch in the same frame; 1 + 10 | — |
| `dismiss_message_boxes` | `max`, `wait_frames`, optional `label` | up to `max` times: wait (checking each frame, at most `wait_frames`) for a message box, then close menus; the player pressing OK on each box as it opens | with `label`: `closed` (boxes that were open when closed) |
| `close_menus` | — | closes the open menu as the player's OK / Done would (the console's `CloseAllMenus`; a name menu closes with the name it had); 1 + 3 | — |
| `wait_quest` | `quest`, `field` (a quest field, §4.1), `value` (true/false/number), `max_frames`, `label`, optional `stopwatch` | waits, checking once per frame, until the field equals `value`, at most `max_frames`; one frame later reads the field again and with `stopwatch` the stopwatch | `found` (bool); with `stopwatch` and found, `seconds` |
| `wait_menu` | `menu` (code), `max_frames`, `label`, optional `stopwatch` | the same for "menu `menu` is open" | as `wait_quest` |
| `get_menus` | `menus` (codes), `label` | whether each menu is open; 1 per code | `menu.<code>` (bool) |
| `get_tag_skills` | `skills` (actor value names), `label` | `IsPlayerTagSkill`; 1 per skill | `tag.<Skill>` (bool) |
| `get_ref` (existing) + field `destroyed` | | `GetDestroyed` (SetDestroyed by VCG01 0 and 65) | bool |

`given` gains `{"kind": "menu", "menu": <code>, "open": bool}`: a menu open at the start.
nvfnew loads with a message box open, which holds GameMode until it is closed; the inputs
set stages while it holds (`L5.intro.tester`, `.complete`). For the candidate this is "a
message box is open before action 0"; the pack plugins' own quests then show theirs.

**The stopwatch.** Seconds of script time: in the game the timer of the skull in Doc
Mitchell's house (FalloutNV 0x00104C10, `VCG01BrahminSkullSCRIPT`: GameMode counts
`timer` down by GetSecondsPassed while `runTimer` is 1; with `grabbed` 0 nothing happens
when it runs out), set to 100000 and read as 100000 − `timer`. An object script, because
round 3's stopwatch (VCG00) is observed here. It stops while a menu is open (§8.3 e).
For the candidate: frames × `frame_seconds` outside menus since the last
`start_stopwatch` / `set_quest_var` with `stopwatch`.

**Tolerance of `seconds`** (`L5.intro.*` compare with `abs_tol` 0.1): the stopwatch is
read the frame after the wait saw the change, `start_stopwatch` takes effect a frame after
the previous read, and GetSecondsPassed comes in 1/64 s steps (§5.4). At the 25–40 loops
per second of the capture sessions a frame is 0.025–0.04 s; at the candidate's 1/60 s it
is 0.017 s. Two frames plus one step is under 0.1 s. Measured: restarted timers came at
0.95–1.0 × their fTimer (0.953 and 0.961 for 1.0 s, 2.969 for 3.0 s); the two passes
differed by at most 1/64 s (§10.7).

### 10.5 What was not driven, and why

- **Stage 55 and anything that needs it**: its result script autosaves (above). The
  natural stage 60 also needs 55 first: Doc's 60 `SayTo` takes the next unsaid line of
  his topic, which before 55 is the line whose result sets 55. The scenarios therefore
  jump to 60 and 80 with SetStage while the first pack box holds GameMode.
- **The psych test dialogue and Doc's farewell dialogue**: dialogue menus with player
  choices; Doc's farewell conversation did not even open within 1,800 loops of stage 115
  (his package had not reached the trigger point). Their result scripts were run by the
  job instead (the test's scores; VGenericTimer event 3).
- **The Pip-Boy**: adding and equipping it from a job script ended the game (once, not
  repeated); the real result script also calls `ResetPipBoyManager` and shows a tutorial
  message. Not part of any scenario.
- **The hardcore-mode message** (`VCG01CasualHardcoreMessageREF.Activate player 1`, a
  message box whose button chooses the mode, read by `GetButtonPressed`): not driven.
- **Walking into the tester trigger / sitting on the couch / walking out**: replaced by
  the stage those scripts set (60, objective 40 completed with `bBeganTest`, 115).

### 10.6 Scenarios and goldens

`parity-lab/scenarios/layer5_intro.json`, comparator keyed on `label` over
`/observations`; inputs `parity-lab/inputs/layer5/intro_<name>.actions.json`; goldens
`goldens/intro/intro_<name>.trace.json` (private). The current candidate rejects the
given kind `menu` (checked 2026-10-09 with this branch's fnvtool on the three visible
inputs: `unknown variant menu`; without it the new ops and `set_quest_var`'s `stopwatch` field are the first unknown
names; without those, the rest of `intro_tester` and `intro_complete` parses and runs), so the scenarios error until 5B adds §10.4.
The candidate was not run on the holdout.

| Scenario | Rows | Tolerance | What |
|---|---|---|---|
| `L5.intro.opening` | 31 | 0.1 | VCG00 90 → VCG01 50 in the game's own order: pack boxes, timers with seconds, Doc's lines (found only: line lengths are audio), name and race menus (stage 40 inside the race menu, script time stopped in the name menu), the state at 50; ends by disabling Doc so that his last lines cannot reach 55 before the game quits |
| `L5.intro.tester` | 14 | 0.1 | 0 and 60 set while a box holds GameMode; the tester's OnActivate (65 inside Activate, menu 1074, destroyed, objective 30 completed); 65's timer; the reaction variables; 75/76/79 in one frame; 80 |
| `L5.intro.complete` | 23 | 0.1 | the psych test's tag skills; 85 → 110 with the two menus and 1 s timers; 115; VGenericTimer event 3 → 200, VMQ01 10, VCG02 5, VCG04, GameHour 8 |
| `L5.intro.r4_holdout` | — | 0.0001 | **holdout** |

Every `given` is a fact of nvfnew: a message box open, VCG00 at 90 (fTimer not given: it
read 3.0 or 1.763 at the start of a job, depending on how soon the first box opened; the
inputs set it), VCG01 untouched, VCG01Test, CGTutorial, VDLCPackQuest and the four pack
quests running, VCG02/VCG03/VCG04/VMQ01/VGenericTimer stopped, the cemetery marker
disabled, SPECIAL all 5. Which pack's flag is set at the start varies, so it is not
given and the number of boxes closed is not observed.

Rebuilding (owner's machine; both run locks first; never save in game):

```
python parity-lab/inputs/layer5/make_inputs4.py
python tools/capture/capture_quests.py job parity-lab/inputs/layer5/intro_<name>.actions.json --name q5d_intro_<name> --start nvfnew --data "<Data>" -o C:/fnv-private/capture/quest4/q5d_intro_<name>.steps.json
python tools/capture/capture_run.py --game "<FNV>" --save nvfnew --kill-on-timeout C:/fnv-private/capture/quest4/q5d_intro_<name>.steps.json
python tools/capture/capture_quests.py golden C:/fnv-private/capture/out/q5d_intro_<name>.<time>.jsonl --actions parity-lab/inputs/layer5/intro_<name>.actions.json -o C:/fnv-private/goldens/intro/intro_<name>.trace.json
```

### 10.7 Stability

Every scenario job ran twice more with the final inputs, in separate game sessions (plan
r4-p5, passes A and B, 2026-10-09 10:09–10:19), after a first pair of runs with an earlier
form of the inputs (per-pack givens, which failed in pass 2 because the first box was
another pack's: the reason those givens were dropped). Every `given` matched the save in
both final passes (`golden` refuses otherwise), and pass B matched pass A under each
scenario's comparator with 0 differences, holdout included. Numbers that differed: the
stopwatch readings, by at most 1/64 s (0.0156; e.g. VCG01 15 at 0.953 and 0.969 s), and
GameHour after 200 (by 0.00001 h). Pass A's traces are the goldens. The stopwatch
readings: VCG00 95 at 3.031 s, 100 at 3.078, VCG01 0 at 3.109, 1 at 3.281, 3 at 6.086
(one start); after restarts, 7 at 3.219, 15 at 0.953, 35 at 2.969, 50 at 2.984, 70 at
0.961, 95 at 0.961, 100 and 102 at 0.984, 105 at 0.969; 0.0 after 120 frames of the
name menu. The Saves folder was unchanged after every one of these runs.

### 10.8 Predictions (written before each run; plans r4-p1 to r4-p5)

- Po1 (nothing moves after the load because a menu is open) — **held** (a pack box).
- Po2 (`CloseAllMenus` lets the chain run until the race menu) — **refuted** in part: the
  chain stopped at the name menu (stage 10), which was not predicted.
- Pp1 (`TapKey 28` closes a message box) — **refuted**; the four boxes, one at a time —
  **held**.
- Pp2/Pp3 (VCG00 3 s, VCG01 0.2 s, 2.8 s, Doc's lines chaining 5 → 10, 3.25 s) — **held**;
  "stage 10 opens no menu" — **refuted** (name menu).
- Pd1–Pd2 (name menu closed by `CloseAllMenus`, 15 one second later; race menu at 36 with
  40 set inside it) — **held**.
- Pd3 (55: objective 10, CGTutorial 2 with two message boxes, **no autosave** because of
  the INI setting) — objective and stage **held**; no box appeared; the autosave
  prediction was **wrong** and the game saved.
- Pd4 (tester → 65 inside Activate, reaction 0/5/1/0.5, 75–79 together, 80) — **held**.
- r4-p4 (drive2: stages while the box holds, Doc's first line again setting 5, tags Guns,
  Science and Sneak, 85–110 timers and menus, 200 with VCG02 running) — **held**; Doc's
  dialogue at 115 and the Pip-Boy were not predicted to fail.
- r4-p5 (scenario jobs) — all visible predictions **held**; the holdout's prediction is in
  the session notes only.

### 10.9 Open items

- **The owner's autosave was overwritten** by the game's own `autosave` (VCG01 55) in one
  trial; the previous one is `autosave.fos.bak`. See the PR's owner review.
- Stage 55 and the natural stage 60 need either the owner's consent to let the game
  autosave during captures, or a way to stop scripted autosaves that does not write
  outside the capture folder.
- The psych test and farewell dialogues (player choices, Layer 6), the Pip-Boy (crash),
  the hardcore message, the couch and the walk to the exit are not captured.
- CGTutorial stage 2's two conditional entries showed no message box; which entry ran is
  not visible.
- Which pack box opens first, and when, varies between sessions [observed]; why is not
  known [hypothesis: the order in which the four pack quests first run after the load].
