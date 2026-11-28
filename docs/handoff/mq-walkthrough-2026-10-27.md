# Main-quest walkthroughs past Novac, headless (2026-10-27)

This is item 3 of project-audit-2026-10-23.md §7: one `fnvtool script run` action chain for
each main-quest chain past Novac, in `crates/fnv-game/walkthrough/`, and a smoke scenario
for each (`mq-*` in tools/smoke/scenarios.toml). Each chain starts from a `given` state
(the earlier quests' stages, globals and items). It drives the quests by the game's own
means where the runner has them: the player's dialogue picks, terminals, activations,
triggers, deaths, AI package events and message box buttons. Then it reads the stages the
game's own scripts set.

**Result: every chain reaches its final stage headlessly** [observed: the eight chains
run with no step error on main at #870 plus this branch; `python tools/smoke run --only mq-…`:
8 passed]. Three hard stalls were found, all in the engine, all small; they are fixed on
this branch (§3). Nothing past Novac is now untested headless. These are still not game
runs: travel, combat and AI arrivals are stand-ins (§4), and the Hoover Dam load test (§7
item 6) is a separate task.

## 1. Chains

| chain file (smoke scenario) | path taken | final stage reached |
|---|---|---|
| boulder-city (`mq-boulder-city`) | VMS20 negotiator: Monroe, Jessup talked round with Speech 45, back to Monroe; Jessup's Benny line | VMS20 100, VMQ01 100, VMQTops 10 |
| tops (`mq-tops`) | Strip gate: the Securitron's 2000-cap check unlocks the north gate; Benny invites the player to the suite (Speech 60); Benny killed there; the chip looted | VMQTops 80 |
| house (`mq-house`) | Victor's door, the elevator's floor box, the chip handed over, the basement demo; House II (Fort upgrade), III (Boomers), IV (Omertas), V (Hidden Valley bunker blown), VI (Kimball saved), VII (El Dorado), VIII (point of no return) | VMQHouse1-7 100, VMQHouse8 80, VHDHouseBattle 10 |
| yesman (`mq-yesman`) | Yes Man at the Tops; the Fort's vault console; House removed at the control-room terminal; the five tribes given as done | VMQYesMan01, 01a, 02, 03 100; VMQYesManFailsafe 100 |
| dam-panel (`mq-dam-panel`) | No Gods, No Masters: the control-center panel (a talking activator in Yes Man's voice, stage 20), the east power-plant console (30), the Legate | VHDIndependentBattle 100 |
| ncr (`mq-ncr`) | Things That Go Boom, Kings' Gambit (only Crocker's Gambit lines set VMQ05 10), For the Republic Part 2 (House unplugged), You'll Know It When It Happens (sniper killed), Moore's box, Eureka! (Lanius talked down) | VMS23 100, VMS29 105, VMQ03a 100, VMQ05 100, vHDBattleController 30 |
| legion (`mq-legion`) | Render Unto Caesar (Cottonwood Cove, the Fort, the vault), Et Tumor Brute? (surgery), Arizona Killer, Veni Vidi Vici | VMQ02, VMS35, VMQ03b 100; VHDLegionBattle 100 (starts VEnding) |
| ending (`mq-ending`) | Eureka! 30 sets the NCR ending; VEnding moves the player to the slide room; the narrator's SayTo chain runs every slide | FadeToCreditsTimer faded out, ExitGame called |
| ending-legion (`mq-ending-legion`) | VHDLegionBattle 100 sets story state 62; the same slide chain (12 slides) | FadeToCreditsTimer faded out, ExitGame called |
| ending-house (`mq-ending-house`) | VHDHouseBattle 100 sets 63 (9 slides) | FadeToCreditsTimer faded out, ExitGame called |
| ending-independent (`mq-ending-independent`) | VHDIndependentBattle 100 sets 64, Securitron vault not opened (65 when it was; not run) (11 slides) | FadeToCreditsTimer faded out, ExitGame called |
| reputation-axes (`mq-reputation-axes`) | Crocker's messenger and Vulpes's Strip greeting, which call SetReputation with axis 2 (item 10) | NCR and Legion infamy 0; VMS23's message flag set, its cleared flag not (the axis-2 call stops the script, #1127) |

Notes [observed]:
- **The Lucky 38 elevator** is an actor with one reference per floor. Activation adds a
  Dialogue package; after the conversation its GameMode shows the floor box (Penthouse is
  button 5), and the button moves the player. It fires headlessly once the package's
  conversation is stood in (§4).
- **The Dam panel** is not a terminal. It is a talking activator (0x00153FCE) that stage 10
  gives Yes Man's voice. All or Nothing uses the same panel step, and it works there too
  (House's line, VHDHouseBattle 20; a probe run, not a committed chain).
- **The Fort's player storage** (NVCCPlayerStorage): VMQ02 110, which its OnOpen sets, is
  the *fail* stage (the Legion turns hostile when the player robs the storage). The
  storage is locked at 255, and the gear comes back when the player leaves the Fort. So
  project-audit §1 item 1's "OnOpen stalls the Legion's turn" should read "OnOpen is how
  the Legion turns hostile". OnOpen fires in the engine since 2026-10-23.
- **Eureka!'s battle controller** counts Legion and NCR respawn pools per zone; no stage
  waits on them. Stage 20 comes from Lanius yielding (or his guards' counts if he dies),
  and 30 from Oliver's greeting.
- **`VMQYesMan01MoveToTerminal` unreachable** (`fnvtool ai reach`) is a false alarm of the
  check, not a stall. `ai reach` judges the package from Yes Man's placed position at the
  Tops, and no door chain joins that to the penthouse. In the game, two scripts move him to
  his penthouse marker 0x00166304 before the package can be chosen. From there it is a
  98-unit walk on one navmesh (`nav path`) [observed].

## 2. Stalls, ranked

**Hard** (a player would stall in the engine too), all fixed on this branch:

1. **The end slides stopped at the fourth slide**: SayTo found no line. VEndingBoomers'
   lines name a Boomer (NellisPete) as fixed speaker (INFO ANAM) and GetIsID Narrator as
   their condition. Our rule "only the fixed speaker says the line" made them unsayable.
   262 lines of FalloutNV.esm name a fixed speaker other than the actor of their own
   GetIsID condition; 167 are end slides, and others are the Lucky 38 elevator Victor,
   Freeside and the Tops. Fix: `Game::can_say_as` (dialogue.rs) lets the line's own
   GetIsID condition decide [hypothesis; documented, GECK wiki Category:Dialogue: Speaker
   "will act like GetIsID when dialogue is exported"]. Observer check 29.
2. **The end slides stopped at the Brotherhood slide**: the narrator's SayToDone chain
   (SCPT 0x00161EB4) SayTos VEndingBrotherhood unconditionally. That topic has no line for
   the NCR ending with the Brotherhood never met, and our SayTo with no line never
   finished. Fix: `Game::say` queues the speaker's SayToDone blocks for the topic in the
   next frame [hypothesis: the chain must go on, as the script calls every topic in turn;
   no document says]. Observer check 28. With both fixes the NCR ending says 11 slides
   (3 topics have no line) and reaches VEndingConclusion, FadeToCreditsTimer and ExitGame.
3. **Message box buttons read in MenuMode 1001 were lost headless**: `press_button` closed
   the box before a MenuMode frame ran. This affects the Fort's hatch and vault consoles
   (House II, Yes Man) and Caesar's surgery box (VMS35); 21-23 scripts read buttons this
   way. The play loop was right (nvf-play `answer`/`finish_answer`); only the runner was
   wrong, so this is hard only for the walkthroughs. Fix: the runner's press sets the
   button, runs one frame with menu 1001 open, then closes the box (runner.rs).

**Soft** (headless only, open; each chain works around it with a stand-in):

4. Done: the runner's `patrol_point {actor, point}` op calls
   `Game::patrol_point_reached` (runner.rs). Was hit by: Moore at Kimball's podium (REFR
   0x00130C75 gives him 0x00145EC3); Yes Man's and Oliver's markers.
5. **Done** (2026-10-30, issue #909): `package {event: done}` on the actor's current
   package now calls `package_goal_met`, so a script package is dropped at the next choice
   (Kimball's walk back to the vertibird, 0x001377CC, then 0x00130C7A). Test:
   perk_talk.rs `the_runner_finishes_packages_and_force_greets_for_dialogue_packages`.
6. **Done** (2026-10-30, issue #909): a `dialogue_package {actor, package?}` op carries
   out a Dialogue package reaching its target as fnv-ai's `carry.rs` does: a Say To
   package says its topic, any other force-greets the player with the package's topic
   (`Game::force_greet`), and the goal is met. The Lucky 38 elevator uses it (house
   chain); Cato, Oliver and Lanius still use `talk` with the topic (their packages were not
   looked up).
7. **Done** (2026-10-30, issue #909): `trigger_enter` takes `frames: N` with `inside`:
   OnTrigger runs N frames, the game going on between them. The Hidden Valley escort
   trigger now runs 180 frames instead of having its timer set (brotherhood chain).
8. Opening a container headless runs no OnOpen (the engine queues it from the container
   menu). Not on any main path (see the storage note in §1). Fix: queue OnOpen/OnClose in
   the runner's container menus (small).
9. **Done** (2026-10-30, issue #909): `fnvtool ai reach` (the code is in
   crates/fnvtool/src/ai_run.rs, not fnv-ai) also tries, when a package is unreachable
   from the editor placement, the places scripts `MoveTo` the actor (`<actor>.MoveTo
   <marker>` with both in the unit's reference list); reached from one of them, the pair
   gets its own verdict, "reachable from where a script moves it (MoveTo)".

**Possibly hard, unverified** (not on the canonical paths; observer checks 30-32):

10. Reputation axes other than 0/1. FalloutNV.esm calls SetReputation with axis 2
    (Crocker's messenger, Vulpes's and Alerio's Strip greetings), AddReputation with axis 2
    (Boulder City Showdown stage 100, one ending) and 3 (Severus's three tag trades), and
    conditions use GetReputation axis 2 (VMS36, Crocker's topics 013/014) and
    GetReputationThreshold axis 3 (Benny's two greetings) [observed: decompiled scripts,
    INFO conditions]. **Not a stall**: the original reads 0 for axes 2/3, and a write with
    one fails and stops the script (observer check 30, o5_00/o5_01); ours does the same since
    #1127 (reputation.rs `write_axis`). So the messenger's line 0x00119094 leaves VMS23
    bReputationClear 0 (its other lines 0x00117E68-6B and stages 10/40 set it; its one
    reader, SCPT 0x0016829F, also takes an NCR threshold), and Vulpes's greeting skips its
    two SetEnemy calls. AddReputation's axis 2/3 was not captured and still goes on
    (observer check 62): if it stops as SetReputation does, Boulder City Showdown's stage
    100 entry 1 (VMS20) would never complete the quest and Severus's trades would not
    count the tags [observed: decompiled scripts; hypothesis: the effect]. One open effect: Benny's
    greeting for a player liked on the Strip needs axis 3 >= 3, so with 0 it is never
    chosen; what the original reads for axis 3 decides it.
11. Say Once per actor: the Yes Man mainframe repeats the Tops Yes Man's first-meeting
    greeting (0x001164F1, shared by GetIsID OR) once (dialogue.md question 13).
    **Fixed (#1113)**: the original does not repeat it (observer check 31); a Say Once
    line now holds for every actor once any has said it.
12. `GetStage Q N == 1` compiled with an extra argument leaves two values on the stack.
    Three begin scripts (e.g. INFO 0x00160731) stop there; ours gives a script error.
    Settled 2026-10-30: the original stops them too (scripts.md §4.2, issue #908).
13. A line with both a follow-up and choices (0x00158A3D, Topic056) plays the follow-up and
    never offers Topic068 [observed in the runner; the original's order is not known].

## 3. Stand-ins (what the chains do by hand)

- Travel: `move_player` / `go_to_cell` to the next scene (all chains).
- Walking into triggers: `trigger_enter` (physics sensors).
- Fights: `kill` with the player as killer (Benny, ambushers, the Kimball sniper, the
  Legion targets, the Dam's Legate guard, 20 kills in the Legion chain).
- AI arrivals: `package {event: done}` and `talk` (soft stalls 4-6).
- Side quests outside the main path: in `given` or `set_stage` with a note (How Little We
  Know 50, Volare!, G.I. Blues, the five Yes Man tribe outcomes, VMQ03a 100 in the House
  chain).
- The ending chain sets Eureka!'s stage 30 itself (the NCR chain reaches it), so it can run
  alone.
- The hacking minigame and the crafting/surgery menus: their results.

## 4. What is left

- Soft stalls 4-9: 4 done here, 5, 6, 7 and 9 in PR #925 (issue #909); 8 (OnOpen in the
  runner's container menus) is still open.
- Observer checks 28-32 (docs/handoff/observer-checks.md): the two end-slide fixes are
  hypotheses until the original confirms them.
- The other endings' slides: all four endings now run every slide to ExitGame (2026-10-27,
  second batch); no new stall. The independent ending with the Securitron vault opened
  (story state 65) was not run.
- No add-on quest was walked (DM, HH, OWB, LR).
- Smoke `sunny-wells` fails on main (325cdaf1): Cheyenne runs after Sunny instead of walking. It was rerun alone with this branch's two play changes switched off and still failed, so it is marked `known_failing`; not explained yet.
- Game runs: these chains say the quest flow holds. They do not show that the AI walks
  there, the fights end or the Dam's ~300 actors run at speed (audit §7 item 6).

## 5. Coverage: what the chains reach, what is untouched

`fnvtool script cover <Data> <the ten masters> --schema schemas/fnv --actions
crates/fnv-game/walkthrough [--detail report.json] [--log-dir logs/] [--logs more.json ...]`
runs every chain over the load order it names (`plugins` in the actions file, else
FalloutNV.esm alone: the add-ons' start-up message boxes would hold the base-game chains
that keep menus), records what each reached, merges that with any saved logs (`script run
--coverage <file>` writes one per run; the result-script sweep will write one) and
compares it with the ten masters. The smoke scenario `walkthrough-coverage` checks the
printed summary against `tools/smoke/expected/walkthrough-coverage.toml`: a branch that
adds a chain or reaches further writes the new numbers there; a drop fails the run.

Reading the summary:
- `quests`: quests with stages (`total`), those with at least one stage reached
  (`touched`), those where a stage that completes the quest (QSDT flag Complete Quest) was
  reached (`completed`). `stages`: stages reached of all stages.
- `scripts` / `blocks`: SCPT scripts with blocks, those with a block run, and blocks run of
  all blocks (unit and block, so a script's GameMode and OnActivate count apart).
- `lines` / `actors`: dialogue lines (INFO) started; actors with lines (a line's own
  GetIsID actor, else its fixed speaker) and those with any line said.
- `results`: result scripts with statements, by kind (quest stage, INFO, terminal,
  package), and those run.
- `untouched_quests`: quests with no stage reached, by rank: `main` (FalloutNV.esm's VMQ,
  VHD, VEnding, VCG0* quests), `faction` (another base-game quest whose stage scripts
  change a reputation [hypothesis: a working rule for ranking, not a category in the data]),
  `add-on` (any other plugin's), `other`. The `untouched` list names them in that order.
- `chains`: each chain's step and given errors (they should all be 0).

Picking the next chain: take the first quests of the `untouched` list (the main quest, then
factions, then add-ons); `--detail` gives per quest the stages reached and its completing
stages, per script the blocks run, per actor the lines said, so a quest half walked shows
which stages are left. Totals at the end of the second batch: §1's table.

## 6. Save and load mid-chain

The runner's `save_load` op writes the game's snapshot (`Game::snapshot`, the state the
engine's saves hold) into a save file's bytes through `fnv_save`, reads it back and
restores the game from it (`Game::restore`, `World::restore`) in the same frame, with the
random numbers where they were. `fnvtool script roundtrip <Data> <masters> --schema
schemas/fnv --actions crates/fnv-game/walkthrough` runs every chain twice, as it is and
with a `save_load` before its middle action, and compares the quests at the end (running,
completed, stages done, variables) and every observation; smoke `walkthrough-roundtrip`
expects no chain to differ.

Result [observed, 2026-10-27]: of 19 chains, every one ends the same after a save and load
at its midpoint (saves of 70-370 KB). With 41 chains, two differ, `fail-boulder-force` and
`come-fly-with-me`: both save while the player is hunted (after a fight); awareness (detection states,
who hunts whom) is not part of a save by design (save-load.md: the engine measures again),
and a headless run has no detection to measure it, so after the load the player is out of
combat and CGTutorial's script resets its `VATSTimer` (0, not 16). (Since 2026-11-01, #971,
the save keeps each actor's detection state and lists, so the player stays hunted.) Soft, and a lead for
play: in the engine the quest script may run in the first frame after a load before
detection does, read IsInCombat 0 and show the VATS tutorial message again when combat
comes back [hypothesis, not run]. The smoke check carries it as known failing. The first run differed in two of intro's stopwatch
readings by one frame: the op let a frame pass, which a save and load does not; it now
takes none.

## 7. The result-script sweep

`fnvtool script sweep <Data> <masters> --schema schemas/fnv [--detail sweep.json] [--log
sweep-coverage.json]` (crates/fnv-game/src/sweep.rs) runs every result script with
statements once, each in a game of its own started as the runner starts one, on the
reference it would run on where the data names one (a placed reference of a line's actor,
of the terminal, of the first placed actor whose package list holds the package; a stage
script on its quest). It groups the call failures, script errors and unknown functions met
by cause, the numbers in the error taken out. Smoke `script-sweep` checks the totals
against `tools/smoke/expected/script-sweep.toml`. `--log` writes the scripts run as a
coverage log, which `script cover --logs` merges (it is not part of the walkthrough
totals: it runs everything once, out of context).

Result over the ten masters [observed, 2026-10-27]: 8639 line, 1040 stage,
384 terminal and 558 package scripts run; 292 met a problem, under
56 causes. No function is unknown (no `unhandled` cause). By what they are:
- **Context the sweep cannot give** (most): "no calling reference" (a terminal used only as
  a sub menu, a package only AddScriptPackage gives), "called on an empty reference" (a
  reference variable an earlier scene sets), AddItem "count must be positive"
  (Nipton's and the Kings' trade lines move as many as the player carries: 0 alone;
  RemoveItem of 0 removes nothing and goes on since #1113, observer check 44; AddItem of
  0 is still refused, not observed),
  EquipItem of an item the actor does not carry yet.
- **AddPerk on an actor other than the player** (26 scripts, companion perks among them):
  was refused; fixed by the companions walkthrough (perks kept per actor), which brought
  the totals to 386 failed calls in 54 causes and 114 line scripts with a problem.
- **`GetStage Q N == 1` with an extra argument** (3 lines, e.g. INFO 0x001588E5): a script
  error stops the begin script before its SetStage; item 12 of §2. The original stops it
  the same way [observed 2026-10-30; scripts.md §4.2].

The commonest call failures:

  | scripts | cause | e.g. |
  |---|---|---|
  | 47 | GetLinkedRef: no calling reference | TERM FalloutNV.esm:064B6E item 0 |
  | 34 | SetActorValue: no calling reference | PACK FalloutNV.esm:02800E begin |
  | 26 | AddPerk: perks of actors other than the player are not modelled | INFO FalloutNV.esm:0B16C8 begin |
  | 21 | AddItem: count must be positive, got # | INFO FalloutNV.esm:0F3E50 end |
  | 20 | Disable: no calling reference | PACK FalloutNV.esm:07E1D1 end |
  | 17 | GetInFaction: no calling reference | INFO FalloutNV.esm:145C6B end |
  | 13 | Disable: called on an empty reference | PACK FalloutNV.esm:1231B6 end |
  | 13 | RemoveItem: no calling reference | PACK FalloutNV.esm:0273AE begin |
  | 12 | AddItemHealthPercent: no calling reference | PACK FalloutNV.esm:0273AE begin |
  | 12 | EquipItem: no calling reference | PACK FalloutNV.esm:0273AE begin |
  | 11 | SetDestroyed: called on an empty reference | TERM DeadMoney.esm:006CAF item 0 |

**The call probe** (#1122, `probe_calls` in the same file; `summary.call_probe` and
`call_probe` in the output, the shapes it could not judge under `call_probe_not_judged`
with `--detail`). A function that is given an argument of a kind its accessor cannot read
(ShowMessage's compiled variables, one `Vars` list, read as a number) and then goes on with
a default fails nowhere: the sweep's causes, `script stubs` and `script coverage` cannot see
it. The `Args` accessors (crates/fnv-game/src/functions/mod.rs) record such an argument
(present but unreadable; an absent one is an optional parameter), and a call that succeeds
with one is traced as `arg_unread` (once per opcode, unit and parameter) in every run, the
walkthroughs' `script run` traces and the game included. The probe runs every implemented
function called by any compiled script (object, quest and effect scripts too, which the
sweep does not run) once per distinct shape of its arguments, as a statement by itself, on
the player, with the script's variables at 0. Result over the ten masters [observed,
2026-10-31]: 427 shapes, 41 not judged (their call failed for want of context: 26 on an
empty reference variable, 4 a form in a variable left at 0, 11 state the player lacks such
as an equipped weapon or sleep; one of them has an argument other than a
value, DamageActorValue's actor-value code, which the accessor reads), one cause before
#1120 (ShowMessage parameter 2, 1713 call sites, 241 result scripts), none after.
What it does not see: a function that returns before it reads the argument (context the
probe lacks); arguments a function reads without the accessors (`a.get(i)`, `a.0`;
ShowMessage's `message_vars`, navmesh and camera arguments, each matching every kind);
calls inside the extender's expressions (Let, Eval, While, ForEach), which the probe does
not walk (they are traced as `arg_unread` when they run). A parameter that takes two kinds
reads the first with a peek that records nothing (`Args::peek_str`, `peek_form`:
GetIsUsedItemType's name or code, GetVATSValue's form or code).

## 8. Second and third batches (2026-10-27/28)

Every chain below runs with no step or given error (`script cover`: 32 chains, all 0/0),
each with its `mq-*` smoke scenario. Add-on chains name their load order (`plugins`).

| chain(s) | path | reached |
|---|---|---|
| ending-legion, ending-house, ending-independent | the other three endings' slides | ExitGame (12, 9, 11 slides) |
| reputation-axes | Crocker's messenger, Vulpes's greeting (axis 2) | infamy cleared; the axis-2 call stops the script (#1127) |
| volare, how-little-we-know, beyond-the-beef, brotherhood, khans | the tribe outcomes Yes Man and House count (VMS18 is Beyond the Beef; G.I. Blues, VMS19, is not stood in by any chain) | VMS15 110, VMS21 50, VMS18 obj. 80, VMS55 100, VMS45 50 and the VStoryEvent globals |
| honest-hearts | Daniel's path, Prisoners of War's door OnOpen | NVDLC02MQ07 100 |
| dead-money, dead-money-heist | Elijah left in the vault; Dog and Christine live | NVDLC01MQ03 100, the ending sequence |
| old-world-blues | the Think Tank talked down | NVDLC03MQ05 100 |
| lonesome-road | Ulysses talked down, no launch (launch at both: NVDLC04MQ04 100, probed) | NVDLC04MQ03b 100 |
| fail-benny-fled, fail-house-killed, fail-legion-betrayed, fail-boulder-force, fail-fort-storage | branches and fail paths | every fail leaves a way on (the Yes Man failsafe, VMQ05, VMQ01) |
| ghost-town-gunfight, run-goodsprings-run, my-kind-of-town, come-fly-with-me | early side quests (VMS16b is Joe Cobb's side) | VMS16 100 / VMS16b 100, nVPrimmDeputyConv 110, VMS01 100 |

**Hard, fixed** (a player would stall in play too):
1. A voice holotape ran no result script: Dead Money's Dog command tape is its quest's only
   way to NVDLC01MQ01a 40 [hypothesis that the original runs them]. `Game::holotape_script`,
   runner `play_note`, and nvf-play holotape.rs runs the begin script as the tape starts and
   the end script as it plays out.
2. A line said for its fixed speaker by another actor (Old World Blues' Think Tank through
   Dr. Klein, end slides by the narrator) had no voice file in the speaker's folder: timing
   (fnv-game voice.rs) and playback (nvf-play voice.rs) fall back to the fixed speaker's file
   [observed: the files exist only there; hypothesis: the game plays them].

**Soft, fixed** (the headless runner only): `patrol_point`, `fire`, `hit`, `play_note`,
`save_load` ops; `player_activate` opens the conversation when the activation's default is
to talk (the play loop does); a container opened by the player runs OnOpen/OnClose (§2 item
8); `get_quest` reads `failed`.

**Open, ranked:**
1. **Done** (2026-11-06, issue #965): three Dead Money narrator lines name Elijah or Vera as
   fixed speaker with no GetIsID condition (INFO 0x01013C60, 0x01011DE9, 0x01011DE8), so
   `can_say_as` refused them and their slides were skipped [observed]. A script's Say/SayTo
   whose topic has no line passing the fixed-speaker check now tries the lines again
   without it (`Game::say_by_script`, dialogue.md §4) [hypothesis, fitted to every such
   call in the masters and add-ons]; what the original does is still observer check 29.
2. **Done** (2026-10-30, issue #906): the REPCONN intercom's trigger (SCPT 0x000CD04E)
   runs `<intercom>.Activate player`, so the player is the activator, not the intercom
   [observed: `fnvtool script decompile`], and the play loop's `talk_after` (player's
   activations) already opened it. Only the runner lost it: `talk_since` ran for
   `player_activate` alone. The runner now opens the conversation for a script's Talk
   activation by the player after any other op and in every frame it waits (`scripted_talk`);
   the come-fly-with-me chain no longer activates the intercom by hand. Test: perk_talk.rs
   `a_scripted_activate_player_of_a_talking_activator_opens_the_conversation`; smoke
   `intercom-trigger-activate`.
3. Possibly hard: dialogue conditions with a null form fail with "parameter 1 is not a form"
   (GetQuestVariable on INFO 0x0014B595-7, GetIsID on 0x000FF5BA) [observed]; if the original
   says those lines they are lost. Settled 2026-10-30: the original reads 0 for both
   (dialogue.md §4, issue #907); ours now gives 0 without a failed call, which keeps the
   lines chosen as before.
4. Soft: AddScriptPackage is built (`functions/actor.rs`, one slot per actor); whether ED-E's
   Lonesome Road packages then run headless is unchecked;
   dialogue packages start no conversation headless (§2 item 6); AI arrivals and sitting are
   stood in; MoveTo travel leaves Dead Money's collar radios counting the player in range
   (their check runs only while their cell is loaded); no pickpocket, loot or sit op.
5. Fixed 2026-11-01 (#971): save and load dropped awareness (§6), so a headless run loaded
   mid-fight read IsInCombat 0; detection states and lists are now saved.
6. Data notes [observed]: SetStage on stages a quest does not define does nothing (Dead
   Money); NVDLC02MQ02 is never started (its errands take its place); the Pine Creek tunnel
   trigger script and Jason's farewell trigger have no placed reference; `scripted:X#n`
   counts from 1 (#0 and #1 name the first reference).

**Tools** (§5-§7, and docs/design/stress-tests.md §3, §3b): `script cover` (coverage),
`script roundtrip` (save and load mid-chain), `script sweep` (every result script once),
stress `cells` now reports missing files and script errors per cell, and stress `markers`
fast-travels to every map marker (tried on four: three travelled, one refused for a hostile
nearby; the World Map now reads its markers on an ask from the test aid).

Not done in this batch: the companion quests and the Gun Runners' Arsenal / Hardcore chains
(helpers stopped by the usage limit, resumed), the three scenes as real game runs (a helper
is on it), and a full markers or cells sweep (hours of game time).

## 9. Fourth batch (2026-10-30, issue #918)

46 new chains from four groups (main quest and factions, more factions and the half-walked
quests, add-ons, base-game side quests), each with its `mq-*` smoke scenario. `script cover`
over all 87 chains: 0 step and 0 given errors; **166 of 228 quests touched (was 113), 132
completed (77), 639 of 1405 stages (378), 1966 lines said (1120), 209 actors (97)**
[observed]. The totals are in tools/smoke/expected/walkthrough-coverage.toml.

| group | chains (quests) |
|---|---|
| main quest, factions | gi-blues (VMS19), forlorn-hope (VMS30, VMS31), i-fought-the-law (VMS02), anywhere-i-wander (VTechatticup), beggar (VMS04), talent-pool (VMSTopsTalentPool, stage 60), moon-over-the-tower (vfreeformlucky38, 25), flags-of-our-foul-ups (VFreeformCampGolf, camaraderie path), birds-of-a-feather (VMS29a) |
| factions, half walked | aba-daba-honeymoon (VMS46), hard-luck-blues (VES34Vault), forgot-to-remember (VMS39), why-cant-we-be-friends (VMS42), finger-of-suspicion (VMS33), i-hear-you-knocking (VMS32), i-dont-hurt-anymore (VMS56), find-it-in-your-heart (vMojave), no-not-much (VMS36), bitter-springs-mountain (VMS37, VMS40), pressing-matters (VMS51), nipton (vMojaveGhost, VNipton), back-in-your-own-backyard (VNelson), we-are-legion (VDeadSea), team-spirit (vFreeformForlornHope03), caesars-favor / caesars-hire (VEFR01/00 Legion), emergency-radio (VEFR01NCRGood2EmergencyRadio) |
| add-ons | honest-hearts-side (NVDLC02MS01-04), honest-hearts-joshua (NVDLC02MQ04), fail-honest-hearts-chaos (NVDLC02MQ06), old-world-blues-think-tank (the Think Tank quest and its six doctors), old-world-blues-sink (HQBuddy, Upgrade, Samples), old-world-blues-sonic-emitter |
| side quests | bye-bye-love (VMS21a), bleed-me-dry (VMS38), return-to-sender (VMS52, the ranger stations' lines), pheeble-will (VMS25), white-wash (VMS11, VMS08's objectives), three-card-bounty (VMS12), camp-searchlight (VMS43, VMS43a), you-can-depend-on-me (VMS05), classic-inspiration (VMS22), guess-who-i-saw-today (VMS41), wheel-of-fortune (VMS44), there-stands-the-grass (VMS06), vault-21-tour (VVault21Tour) |

Each chain's notes name its stand-ins (travel, kills, skill values given, dialogue packages
replaced by `talk`, AI arrivals by `package done`, as in §3).

**Not walkable** (no script, line or stage sets their stages, or they only start themselves)
[observed, decompiled scripts and INFO/QUST result scripts]: VMQ04 (orphan; VMQ02 owns the
regulators), VMQ06 and VMQHouse (old stubs), VMQHouseLockdown, VDialogueCaesar (dialogue
holder), VEFR02NCRBad2MostWanted, VEFR00NCRMixed2LimitedAccess, VEFR02LegionBad2CaesarsFoe
(only started; their scripts stand down hit squads), NVDLC01MQ01d, MQ03d,
FountainStartSequence, ElijahBarkTown, NVDLC03GenericPerkQUEST, NVDLC01EnemyTest (test),
NVDLC02MQ02 (never started), NVDLC03Enc (starts itself), VMS10 (every stage script commented
out), VMSRSGlobalCommBreakdown (a test script only; VMS52 replaced it), the three ranger
station dialogue quests, VMS26, VFreeformTheStreet01, the VES01/01a/01b/03/04/05 bark quests,
vDialogueSL, VFactionSquad, VMS01REPCONGhouls, WaterBeggars, PerkWildWasteland,
FriendOfTheNightQuest, FerociousLoyaltyQuest.

**Walked but not counted as completed** (they end by CompleteQuest, or their last stage has
no Complete Quest flag): VMS39, vFreeformForlornHope03, VMS11, VMS05, VMS44, VVault21Tour,
VMS43a.

**Left, walkable** (they have setters; out of time): VDialogueVegasNorth (25 stages, not
dialogue-only), VMS13, VMS50, VMS24 (behind the King's first greeting), VES02DialogueGeckoman,
VNellisInfestation, VFSAtomicPimp, vNiptonVulpes, VDoctors, Doctors, VMS03, VMS09, VMS09a,
vCCEyeforaneye, vCCLeftMyHeart, VT74, VMS57, nvNelsonExplorerBark, VMS18, VFSHighTimes,
VFSDebtCollector, vFreeformForlornHope02, VMS55a, VMS55b, NellisSunshineBoogie, and the Old
World Blues X-8/X-13 setups and Lonesome Road's launch-at-both ending (NVDLC04MQ04).

### Engine and runner changes (each with a test)

1. **Hard: a chosen topic said the wrong line, so the Sink was unreachable.** The Think
   Tank's Topic001 gave 0x01013555, the first line of a follow-up chain, never the chip line
   0x01013559. `Game::select_choice_line` (dialogue.rs) passes over lines another line names
   as its follow-up (TCFU) while another line can be said; greetings and SayTo keep the old
   choice (applying it there broke five chains). [hypothesis; observer check 37;
   dialogue.md §3.4]
2. **Hard: `Begin OnHitWith <form list>` never ran** (the Sonic Emitter's force fields).
   `block_accepts` (game.rs) accepts a member of the list. [hypothesis; observer check 38]
3. **Runner: hidden message-box buttons could be pressed.** It pressed the Lucky 38
   mainframe's bug button before Emily gave the bug. Buttons whose conditions fail at
   ShowMessage are kept (`MessageBox.hidden`) and refused by `press_button`, in play and in
   the runner. This caught one chain (emergency-radio pressed a call button outside the
   support region) [observed].
4. **Runner: `created:<base>`** names the newest reference made at run time from a base
   (PlaceAtMe), as the sim runner does (the Emergency Radio's supply cache).

### Stalls, ranked

**Possibly hard** (observer checks 39-41):
1. StartConversation from a Goodbye line's end script is refused: the old conversation is
   still open when the end script runs (`finish_line` runs it before `end_dialogue_as`).
   8 INFO end scripts in FalloutNV.esm do this (0x00116C36, 0x00129351, 0x00142A9C,
   0x001596F2, 0x001598B4, 0x001598AF, 0x0015D0AA, 0x0015D0AB); Anders' thanks in VMS46 is
   lost (not blocking). Fix: queue the force-greet and start it in `end_dialogue_as`.
2. Re-displaying a completed objective un-completes it (`Game::set_objective`). In G.I.
   Blues, Roy's chained line 0x0015C410 re-displays objective 65; if it plays after the King
   completes 65, his greeting 0x00113634 holds and stage 100 cannot be reached. Fixed
   (#1108): re-displaying keeps the flag, as the original does (observer check 40).
3. ResetQuest drops a SetQuestDelay override (`reset_quest` replaces the QuestState): the
   Emergency Radio's drop then waits 5 s of menu mode instead of 0.1 s. Fixed (#1109):
   the delay is kept, as the original does (observer check 41).
4. **Done** (#1111): a talking activator's bare `Activate` in GameMode opened no
   conversation (no action reference); it now uses the reference's last activator, and
   the runner opens a talk a script starts in the frames an activation op waits. All of
   the Sink's appliances talk after their install; the old-world-blues-sink chain no
   longer stands in `talk` for the station and the Jukebox.

**Soft** (headless only, stood in): **done** (#1282): `wait_talk` waits while a Goodbye
greeting is said at the player, so its end script runs (Carlitos INFO 0x0011F861, Joana
0x00127A55; bye-bye-love and the Yes Man chains wait for it with no timed stand-in);
`end_talk` on an ended conversation does nothing; a runner `hit` on an actor provokes it as
the engine's detection pass does, and every runner frame settles awareness, so a fight whose
target died ends (Simon after the bomber: birds-of-a-feather drops its StopCombat). Left
(#1114): AddScriptPackage is a stub (Joana, the Omertas, Sarah's tour, Logan, Boone's
BSArrive, the VEFR messengers); SayTo ping-pong with no line runs every frame until a
condition passes (VFSPacerGreetsSimon, SCPT 0x001205B7 / 0x001251C6); StartConversation
starts at any distance, so Nelson's explorers pull the player in from about 4000 units
(nvNelsonExplorerBarkSCRIPT 0x0014EA06).

**Data notes** [observed]: Billy Knight is enabled only by the Strip code box
(VStreetEventStarterScript 0x00163E9B), which needs its `Bot05` filled by the Freeside gate
or monorail door; arriving another way, the box stops every frame at `Bot05.Disable`. Talent
Pool's stages 20-50 and VMS12's 10-60 are set by nothing. VMS43a 30 can be set by its trigger
while the quest is not running. Honest Hearts' four side quests must be done before Gathering
Storms' choice (MQ04/MQ05 fail any still running).

**Found on main, not from this batch:** VMQNCRFail stage 2 is no longer reached by the legion
chain (379 stages at db11b046, 378 on main 1e9a14c7). Its script (SCPT 0x0016829F) sets stage
2 one second after VMQ02 objective 25 or 30 completes; on main the legion chain reaches that
point about 12000 frames later and the script takes its stage-5 branch instead. One of the
two fnv-game changes since db11b046 (4449628f radiation falloff, cd93d5c7 aid effects) moved
it; not bisected.

## 10. Fifth batch (2026-10-30, issue #932)

24 new chains (one older chain runs further), from four groups, each with its `mq-*` smoke
scenario, plus a coverage change. `script cover` over all chains: 0 step and 0 given errors;
**191 of 228 quests touched (was 166), 165 completed (132), 710 of 1405 stages (639), 2249 lines said (1966), 246 actors (209)** [observed]. The totals are in tools/smoke/expected/walkthrough-coverage.toml.

| group | chains (quests: stages reached) |
|---|---|
| larger base-game | someone-to-watch-over-me (VDialogueVegasNorth 10-100, Alice goes home), nothin-but-a-hound-dog (VMS24 100), the-coyotes (VMS13 100), unfriendly-persuasion (VMS50 100, bought off), eyesight-to-the-blind (VMS55b 100), tend-to-your-business (VMS55a 100, VMS55 101), ant-misbehavin (VNellisInfestation 20, completed), sunshine-boogie (NellisSunshineBoogie 100) |
| add-ons | old-world-blues-setups (NVDLC03X8Setup 100, NVDLC03X13Setup 100, NVDLC03Enc 10, NVDLC03GenericPerkQUEST 10/11), lonesome-road-launch (NVDLC04MQ04 100, launch at both) |
| small side | exploring-vault-74 (VT74 100), nelson-explorer (nvNelsonExplorerBark 100), geckoman (VES02DialogueGeckoman 50-70), legend-of-the-star (VMS09 30 fail, VMS09a 100), crazy-crazy-crazy (VMS57 100), cold-cold-heart (vNiptonVulpes 100), doctors (VDoctors 10-30, Doctors 20), that-lucky-old-sun (VMS03, by CompleteQuest), cottonwood-cove (vCCLeftMyHeart by CompleteQuest, vCCEyeforaneye 100), fail-beyond-the-beef (VMS18 255); beyond-the-beef now runs on to Heck's reward |
| freeform | stocking-up (vFreeformForlornHope02 10), high-times (VFSHighTimes, CompleteQuest), debt-collector (VFSDebtCollector, CompleteQuest), wang-dang-atomic-tango (VFSAtomicPimp 100) |

New stand-ins beyond §3's: skill values and items given (each chain's notes), `add_spell
WithdrawalJet` for an addiction, ShowMap for a marker found on arrival, Ibsen's datastore
virus hunt replaced by its result (`VirusWiped 1`: its infected hosts are drawn at random),
Rex moved to Doctor Henry by MoveTo, and two new runner ops (below).

**Coverage change.** A quest the run completes (`Game::quest_completed`, which CompleteQuest
and a Complete Quest stage both go through) now counts as completed, and as touched even with
no stage reached; `given` state does not count. Of §9's seven "walked but not counted", VMS39,
VMS11 and VMS05 now count. VVault21Tour and vFreeformForlornHope03 are completed by nothing in
the ESM, VMS43a ends by StopQuest, and VMS44's CompleteQuest is only on the Logan-dead branch
[observed, decompiled scripts]. Test: `coverage_counts_a_quest_completed_by_complete_quest`.

**Runner ops** (test: `the_runner_picks_a_perks_activate_choice`, perk_talk.rs):
`perk_choice {target, choice}` opens the perks' activate choice list on a reference and picks
one (Robot Compliance's "Deactivate"); `player_sneak {sneaking}` sets the player's measured
crouch, which IsSneaking reads (SetForceSneak does nothing for the player).

**Set by nothing** [observed, ESM]: VES02DialogueGeckoman 10 and 100, VMS09 10, vNiptonVulpes
15, VNellisInfestation 10, VMS55b objective 20, Doctors 10 (Argyll's script sets a stage the
quest does not define), vCCLeftMyHeart 0 and 100, NVDLC03Enc 20. Not walkable, as §9 said:
NVDLC01ElijahBarkTown, NVDLC01FountainStartSequence, NVDLC01MQ01d/MQ03d, NVDLC01EnemyTest,
NVDLC02MQ02.

**Data notes** [observed]: VMS24 is offered only from the King's plain greeting, after G.I.
Blues; Raquel and the Nellis gate guard start disabled and are enabled by the front gate
script and the guard's greeting; Greasy Johnny is WatchFiend2REF; Mrs Hostetler's Topic034 and
Jules' Topic007 borrow another topic's line (INFC/INFX).

### Stalls

1. **Possibly hard: the doctors' addiction cure does nothing.** VDoctors and Doctors stage
   20 call RemoveSpell with no reference; a stage result script has no calling reference
   (script-language.md), so all 13 calls fail and the withdrawal stays [observed]. Observer
   check 42. **Fixed (#1110)**: the original cures the player (o5_03); RemoveSpell with no
   reference now acts on the player, and `mq-doctors` checks the withdrawal is gone after
   both cures.
2. **Soft: Old World Blues' first quest script never runs in the older chains.**
   old-world-blues and the chains that copy its start talk to Dr. Klein in the same second
   the quest starts; MQ01 reaches 100 before its 5 s script has run, so the Act XP values and
   NVDLC03Enc 10 are never set there. old-world-blues-setups spends 6 s first and reaches
   them [observed]. Observer check 43. **The original's too (#1113)**: started and set to
   100 at once, MQ01's script never runs there either (o34_43, 2026-11-26).
3. **Soft: `RemoveItem x 0` is refused** (Loyal's reward line, INFO 0x0010A1BF, when the
   arrays were fixed by hand). Not blocking. Observer check 44. **Fixed (#1113)**: the
   original removes nothing and goes on (o5_00); ours does the same.
4. **Soft:** a second Nelson explorer force-greets at once (the any-distance
   StartConversation stall, check 45; since #1113 it finds no greeting, the explorers'
   0x0014E9FF being Say Once and held for every actor once said, check 31; answered: in the
   original the far explorers do not come at all, #4919); the HELIOS
   One west-terminal password was the walkthrough's own (check 46, #1113: it comes with
   Fantastic's GOODBYE line, and the chain now says goodbye and finds the note with the
   player); Kilborn's conversation ends when stage 100 stops
   vNiptonVulpes: every line he has as a fixed speaker is that quest's, so nothing is left
   to offer and ours closes it, as the original does (check 72, answered 2026-11-26). Two are the
   data's own (#1113) [observed]: Doctors 30's line (DoctorRadiationYES 0x000BE417, Info
   Refusal, no conditions) can be said by no placed actor: Doctors' quest conditions ask
   for DoctorsDialogueFaction and not Doc Mitchell's voice type 0x0010512F; of the
   faction's NPCs, Sawbones and Markland have VDoctors lines of the topic (priority 76 over
   Doctors' 75, a GetIsID line with no other condition always among them), Doc Mitchell is
   left out by his voice, Doctor Barrows and DocAATEMPLATE are placed nowhere, and Doctor
   Rotson (Hell's Motel, templated on DocAATEMPLATE with no template flags) is not in the
   faction. VNellisInfestation's iInfestation stays 0 when the emitter is used before the
   power: the emitter's OnActivate sets it only when objective 15 (the power) is done, the
   ants' OnDeath only when objective 20 is still open (the emitter has completed it), and
   the generator door's script asks for GetDeadCount NellisAntWorkerExplosive == 15 while
   5 are placed (10 soldiers), so no script sets it (a door activation after the power
   reads 5 and 10 headless).
5. **Not from this batch:** `GetQuestVariable` "parameter 1 is not a form" in every Hidden
   Valley/Freeside run (INFO 0x0014B595-97); issue #907.

**Not walked:** other branches (VDialogueVegasNorth 72-89, VMS03's other power targets and
Ignacio's ending, Eye for an Eye's other paths, VMS57's peaceful ending) and the fail stages.

## 11. Stage coverage (2026-10-30, issue #940)

95 new chains (two older ones run further), from four groups, each with its `mq-*` smoke
scenario, plus a report of which stages anything can set. `script cover` over all 206 chains:
0 step and 0 given errors; **927 of 1405 stages reached (was 710), and 927 of the 1043 stages
the data can set (89%)**; 195 of 228 quests touched (191), 166 completed (165), 2435 lines
said (2249) [observed]. The totals are in tools/smoke/expected/walkthrough-coverage.toml.

**Settable stages** (`coverage.rs`, `stage_setters`; test
`coverage_reports_which_missing_stages_the_data_can_set`). A stage is settable when some unit
in the load order calls SetStage on that quest and stage: any script, quest stage result
(other than the stage's own), INFO begin/end, terminal item, package begin/end/change, perk
entry or patrol script, as a statement, inside an expression or an extender lambda. Perks'
Quest + Stage effects count too (the engine sets them in `perks.rs gain_rank`). No condition
function sets a stage. The summary gains `stages.settable`, `missing_settable`,
`by_variable` and `reached_unsettable`; `--detail` lists per quest the missing settable and
unsettable stages and up to three setters for each. In the ten masters, no SetStage takes a
variable quest or stage, and every reached stage has a setter [observed]. The other 362
stages are set by nothing: cut content (VCG00 5-85, the cemetery scene stage 0's script skips),
test quests, and stages only a quest's own stage names.

| group | chains (stages) |
|---|---|
| main quest | house runs on through All or Nothing (VHDHouseBattle 20-100); come-fly-with-me on to VMQ01 50; fail-house-boomers/-omertas/-brotherhood/-kimball/-el-dorado/-dam (VMQHouse3-8 fails); tops-ambush, tops-swank, tops-swank-assassin, tops-swank-guns (VMQTops 5-80); new-game (VCG00 0, 90, 95, 100); fail-sunny-killed; fail-kimball-graham; fail-kimball-left; fail-cato-killed; dam-generators (VHDIndependentBattle 25); yesman-vault-destroyed |
| factions | Camp Golf range and fail; Forlorn Hope's no-medic, patient killed/harmed; Medical Mystery bribe and let-off; I Fought the Law NCR side and fail; fail chains for Talent Pool, Lucky 38, ET Tumor Brute (two), Khans, Volare, Finger of Suspicion, Come Fly With Me, G.I. Blues, Nipton, We Are Legion, Bitter Springs, Pressing Matters, Run Goodsprings Run, Beggar, Nelson and others; hard-luck-blues-survivors; gi-blues-kieran; beggar-expose-tyrone |
| side quests | fail-killed-givers-1/2/3 (each quest giver killed: about 30 fail stages); Pheeble Will (5 branches), Bye Bye Love (4), North Vegas (3), Camp Searchlight (3); tutorial-hints (CGTutorial 200-218); perk-quests (Generic 10-110, Ferocious Loyalty, Friend of the Night); and single branches of Nellis, Wheel of Fortune, White Wash, Atomic Tango, Cottonwood Cove, Primm, Cass, Lem |
| add-ons | fail chains for the Think Tank and its six doctors, Honest Hearts' arrival, supply, deliverer, Gathering Storms, Crush and Flight, Dead Money's dog, gala and act 3; honest-hearts-pine-creek; honest-hearts-happy-trails; lonesome-road-launch-ncr/-legion and -camarader-e (NVDLC04GenericPerkQUEST 10, 12, 70, 80) |

New stand-ins (each chain's notes): AddPerk for a perk picked at a level-up, GameHour for the
wait menu, `enable NellisRaquelREF` for the front gate, `call "...StartCombat player"` for the
player attacking an actor, trigger_enter with an NPC as the actor for his walk.

**Runner change:** `package {event: done}` now marks the goal met when that package is the
actor's current one, as `Game::package_goal_met` does, so GetCurrentAIProcedure reads DONE
after it (Camp Golf's range waits for three misfits to read it). Test:
`the_runners_package_done_marks_the_current_packages_goal_met` (package_choice.rs); the 17
chains using the op still pass.

### Stalls

1. **Done** (#1111): a talking activator's scripted `Activate player` (the Tops intercom in
   Benny's voice) opens the conversation since #906 (runner `scripted_talk`, play
   `talk_after`); the tops-ambush chain waits for it with no stand-in. Observer check 47
   confirmed the original opens it.
2. **Possibly hard (check 40 again):** VFSAtomicPimpSCRIPT re-displays objective 34 after
   completing it on every pass, so a recruit killed after the quest starts leaves 24/34 open
   and Garret's short-handed line unoffered. The chain kills them before the quest starts.
   Fixed (#1108): the original keeps the flag (check 40), and so does `set_objective` now.
3. **Soft, headless only:** a hit starts no AI combat (Daniel, check 49; **done**, #1282: the
   runner's `hit` provokes the actor, and fail-honest-hearts-flight and fail-boulder-force
   hit instead of calling StartCombat); no line of sight
   (VMS31 60, check 50); Jed and the caravan guards are essential until stage 100, so
   NVDLC02MQ00 255 cannot be reached; CGTutorial 214 counts only markers play discovers.
4. **Soft:** Oliver's Topic065 has two lines with the same conditions; ours always says
   0x00159932, so his retreat lines are never offered (check 48: the original says
   0x00159932 too, the data's own, #1113). EDEScript calls on an
   empty reference every frame: the original does not short-circuit `&&` either (check 51);
   since #1112 the failure is traced once.

**Settable, not walked (116 left):** VMS29's nine Kings' Gambit alternatives; VMS20, VMS21,
VMS23, VMS55 branches; VMQ02 40, VMQ03a 110, VMQ03b 110/115/200, VMQHouse3/4/6 report lines;
VMS35 110/255; VMS21a 5, VMS43a 19/25, VMS06 31-33, VMS42 50, VMS11 90, VMS47 20;
CGTutorial 18-24, 62, 72, 92 (a branch of the intro chain); NVDLC01MQ02c 60, MQ03 255, MQ03c
20; NVDLC02MS02 27/35/40, MQ03a/b/c 20; NVDLC03 HQBuddySamples 20, SonicEmitterQuest 10,
ThinkTankQuest 35; NVDLC04 GenericPerkQUEST 30, MQ02 30; VMQNCRFail 2 (#931). The --detail
report lists them all with their setters.

**Triage of the settable, not walked (2026-11-01, issue #1115, in progress).** On main
`5fb0b80e` `script cover` counts 114 (`missing_settable`; two were walked since). 32 of them
are VCG00 5-85, the cut cemetery scene: VCG00ScriptNV's GameMode would step through them
only from stage 0, but stage 0's own result script plays the intro film and sets 90 at its
end, so the GameMode never sees stage 0 [observed, decompiled QUST 0x00102037 stage 0 and
SCPT 0x00102035]. They count as settable (a script names them) but cannot be walked. That
leaves 82.

**Batch 1 (2026-11-01, issue #1115):** 11 chains, each with its `mq-*` smoke scenario, reach
12 of the 82 [observed: each chain's run, 0 step and 0 given errors]:

| chain | stages | path |
|---|---|---|
| kings-gambit-pacer-killed | VMS29 102 | Pacer killed outright; Crocker refuses a reward (Topic016, 017) |
| kings-gambit-energy-weapon | VMS29 100 | Pacer shot with a laser rifle (his OnHitWith marks the energy weapon): the Van Graff frame |
| kings-gambit-psycho | VMS29 101 | the jet on Pacer's table spiked (its box, button 4, needs Psycho carried); Pacer's jet package ends |
| kings-gambit-moore | VMS29 20, 111 | the King, owed no favour, refuses; Moore's squad gives the ultimatum and fights the Kings |
| kings-gambit-hsu-peace | VMS29 20, 120 | refused; Hsu's offer, the King agrees, Pacer breaks with him (KingAndPacerArgue) and is killed |
| kings-gambit-hsu-fight | VMS29 20, 121 | refused; Hsu's offer taken as a challenge, the squad fights |
| kings-gambit-crocker-killed | VMS29 200 | Crocker killed while the quest runs |
| things-that-go-boom-wait | VMS23 40 | Volare! done, the Boomers not yet asked: Crocker waits (Topic006, 023) |
| things-that-go-boom-no-boomers | VMS23 102 | Volare! failed (Jack killed) before the player meets Crocker |
| things-that-go-boom-crocker-killed | VMS23 200 | Crocker killed while the quest runs |
| why-cant-we-be-friends-lem-gives-up | VMS42 50 | Lem's job, the sulfur cave's microcline charge armed (Science 40, Explosives 60, three charges), Lem talked out of the gang (Topic179, 197) |

The VMS29 and VMS23 chains fork the NCR chain (ncr.actions.json) before Crocker's ask; the
VMS42 one goes on from why-cant-we-be-friends-lem. Stand-ins: `kill` for the fights with
Pacer and Crocker, `hit` with a laser rifle for the shot, `package done` for Pacer's walk to
his jet and the NCR squad's return, `move_player` for the walks.

**Settable but cannot be reached** [observed, decompiled]:
- VMS29 110: Crocker's line 0x00120580 needs objective 80 shown, and nothing in the load
  order shows it (the squad's package shows 81 or 91, Pacer's OnDeath 20 or 90).
- VMSTopsTalentPool 25, 35, 45: only Tommy's Billy haggle (Topic013, 0x00120005/06) sets
  them, and its topic TommyToriniBillyBargain (0x00120000) needs BillyRecruited = 2, which
  nothing sets (Billy's two recruiting lines set 1). Stages 20, 30 and 40, which the
  haggle's 35 and 45 branches would follow, are set by nothing either.
