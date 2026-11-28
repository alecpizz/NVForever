# Quest pilot: playing the walkthrough chains as a player

Issue #2365. Code: `engine/nvforever/src/pilot.rs` (the pilot), `crates/fnv-game/src/softlock.rs`
and `engine/nvf-play/src/softlock.rs` (the soft-lock watchdog). Everything here is [design]:
the original game has no such tool.

## 1. Why

The headless walkthroughs (`crates/fnv-game/walkthrough/*.actions.json`, run by
`fnv_game::runner`, `fnvtool script run`) reach about nine in ten of the quest stages the load
order can set, yet players hit soft locks in quests that pass headless. The chains stand in for
what breaks in play: travel (`move_player` puts the player there), AI arrivals (`package ... done`,
`X.MoveTo marker`), the player's dialogue picks (`say_info` says the answer directly), fights
(`kill`), activation reach, locked doors, terminals and items (`add_item`). The pilot plays the
same chains in the running game with the player character and leaves those stand-ins to the game.

## 2. Running it

```
NVFOREVER_HOME=<a folder of its own> nvforever --data <Data> --play --new-game \
  --pilot <chain> [--pilot <chain> ...] [--pilot-expect <headless trace> ...] \
  [--pilot-campaign] [--pilot-given] [--pilot-no-god] [--pilot-out <report.json>]
```

- `--pilot-expect`: the trace `fnvtool script run <Data> <plugins> --schema schemas/fnv
  --actions <chain>` prints, one per chain in the same order. Each labelled read is compared with
  the headless one of the same step; a `get_quest` that differs is read again for 45 s first.
- `--pilot-campaign` (with `--new-game` or `--load`): the chains go on from the game's own state.
  A chain's `given` quest entries are checked as preconditions (`precondition` findings), not set;
  the first chain's `given` is skipped. A save `pilot-after-<chain>` is written after each chain,
  so a later run can `--load` it and play only the chains after it.
- `--pilot-given` (with `--pilot-campaign`): a campaign's means, but each chain from its own
  `given`. For chains no campaign has reached yet; expect the false alarms a thin `given` brings
  (Doc Mitchell's OnActivate restarted the opening when VCG01 was only marked done).
- `--pilot-from N` (with `--load`): the first chain begins at step N from the loaded game, to
  replay a stall from a save near it (a batch run's autosave) in minutes rather than the
  whole chain.
- Set `NVFOREVER_HOME` to a folder of its own: the game autosaves after travel, and early runs
  overwrote the player's own autosave slot.
- Test aids the runs use: `NVF_PLAY_TEST_SCRIPT_MENUS=900`, `NVF_PLAY_AIDS_FROM_SCRIPTS=1`,
  `NVF_PLAY_TEST_UNATTENDED=1` (and `NVF_PLAY_TEST_INTRO_SKIP=1` from a new game).

## 3. What a step becomes

| step | in the pilot |
|---|---|
| `move_player`, `go_to_cell`, `trigger_enter` | (`go_to_cell` of an exterior cell with no COC marker: its middle, at the middle height of what is placed in it) a walk on the navmesh (`fnv_nav`) on the player's own movement controls, through doors; a long way out of doors is cut to its last 3,000 units (a fast-travel stand-in), only within the player's own worldspace, into the Mojave's or another of the base game's (the Strip, Freeside; never a DLC's), and never past a door whose script runs on activation (it is cut to 3,000 units before that door, or not at all when the way to it is indoors: Lonesome Road's Divide east door takes the player's controls, and Ulysses' trigger beyond it gives them back); also on its own side when the search gives up before an end (Vault 112 to the Jefferson Memorial, past 400,000 triangles) or no navmesh joins the two at all with no locked door met (Rivet City's swing bridge: a `no_navmesh_way` finding); into an interior of the other wastelands, outside a door of it that runs no script when there is one (GNR's back door from Dupont Circle, DuponttoGNRRef, only says it is barred: MQ02GNRBackDoorScript [observed]); a load door into its own worldspace (the Strip's gates) counts as gone through by the jump; two walkable areas of one interior that no navmesh link joins, within 48 units on the ground and in height, are stepped across on the player's own movement (SSHQ01's prize room stairs, #3473: the original's followers do not cross there either, observer check 279); never put where it could not walk |
| `talk`, `player_activate` | walk there, turn the view on it, ask the crosshair (`could_not_target` when it does not take it), activate as E does (a `talk` to a teammate opens its companion wheel: Talk is picked on it, Sarah Lyons in Take it Back!) |
| `choose`, `wait_talk`, `end_talk` | the dialogue screen's own calls; leaving is by the Goodbye choice with its line heard out (#2157), one whose answer runs no script first (the Gomorrah guard's "Here you go. <Hand over all weapons>" is a Goodbye too: taken to leave, it handed the weapons over outside and the next way in kept the controls the door's script took), Tab on a topic list, `no_way_out` otherwise |
| `say_info` (campaign) | the player brings the line about: keeps the speaker company, talks to it when it stands silent, picks the choices that lead to the line (`choice_toward`, up to 5 picks deep), or, with no known way, tries the choices on offer one topic at a time (up to 12) |
| `hit` by the player with a weapon it carries | the weapon in hand, from 150 to 600 units off (a step back when nearer), aimed at the object, the trigger held until a shot runs its OnHit / OnHitWith blocks (Lonesome Road's laser detonator on the camp barrier's warhead, 30 shots); a weapon not carried, or 20 s of shots that run nothing, is the headless hit |
| `kill` | a fight with the player's own weapon (a 9mm pistol is given once, god mode on by default); the body is searched for quest items and keys |
| `terminal` | walk there, activate, the screen's own input: password on the hacking page, into the sub menu, the entry |
| `add_item` to the player (campaign) | taken from whoever holds it in the loaded cells, or where it lies; `item_not_found` and the headless gift otherwise |
| `X.MoveTo marker` (campaign) | the player goes to the marker and waits there on the Wait menu, an hour at a time, up to 24 hours (the actor counts as there only in the marker's own space: two interiors share coordinates, Dad was taken as at MQ05ControlRoomMarker in PPurity01 from PPurity02); `never_came` otherwise |
| `package`, `patrol_point`, `dialogue_package` | not carried out: the AI has to do it. A `patrol_point`, and an actor's `trigger_enter`, is checked afterwards: the actor within 256 units of the point or the trigger's reference within 180 s, else an `ai_not_done` finding (also when the chain ends first) |
| `call_until` | the console line read each frame until it is not 0, for up to its own `max_frames` (30 minutes at most; the 240 s step limit does not apply): Lonesome Road's end slides take about three minutes |
| `wait_menu`, `get_menus`, `press_button`, `dismiss_message_boxes` | the game's own menu; a box nobody waits for gets its last shown button after 1.5 s |
| anything else | the headless step on the running game, counted in the report's `headless` by kind |

Of its own accord, between steps: it fights what attacks the player or the actor it is with,
answers the level-up menu (points spread down the list, the first perk on offer), keeps its weapon
whole, searches bodies, meets a line's carry-weight limit by putting its heaviest things into the
nearest container of the cell and talking again (Jed's "There's a chest over yonder" before the
caravan to Zion: a `GetActorValue InventoryWeight` condition on the line said instead of the
chain's; counted in `headless`), uses a load door from where a walk is held up within 320 units of it
(Honest Hearts' Northern Passage door is an invisible slab 512 units wide that stops the player
short of the door's point on the navmesh), keeps off a door that is out of the world
(disabled) unless a switch's script enables it, and after using that switch waits up to 150 s at
the door for it to come (Lonesome Road's Ashton silo lift: its button enables the far door 75 s
into the ride; the direct door stays disabled until the player is down there; when the only
way there is such a door, out of the world and on a navmesh the ride enables, the player goes
to its switch first, then waits and plans again), stands on the navmesh again when it is off it (onto the nearest point within 500 units; with no usable triangle within the search's 512, within 4,096 and a `no_navmesh_under` finding: MQ05's landing vertibirds cut the Jefferson Memorial's ground from under the player, #2846), takes the navmesh's ledges (drops and climbs)
when no way is walkable without them (Lonesome Road's Divide is ten areas joined by ledges),
gives a step up when the run shows no progress for 90 s (the stall watchdog: progress is the step moving on, the player walking 150 units, a line said, an hour waited or the trigger pulled; `call_until`, `advance_seconds`, `wait_quest`, `wait_menu` and `wait_frames` keep their own limits: Vault 101's birth waits up to 120 s for each of Dad's lines), with a `stalled` finding that records the step, the place, the control locks, the menu and the conversation (the step's own 240 s limit could be reset by a step that kept trying, and a walk with the controls held sat it out step after step), fetches a locked door's key from a body, a container or where it lies (a key only a living actor carries is not taken: a `key_held` finding names the holder; a disabled reference, as TTW's test container behind TTWTestArmorEnableParentREF, is not in the world for it), keeps by a locked door's owner when one is alive within 6000 units, up to 120 s, and goes through once the owner has it open (an actor passes the locked doors it owns, `fnv_ai::carry`; The Pitt's Wernher escorts the player through his own fence gate PittFenceGateRef, XOWN DLC01Wernher [observed], and an escort waits while the one it leads is more than its radius away, so the player follows rather than waits at the gate; `owner_did_not_open` when it never does), counts a door's 8 s from when the player's movement or fighting is given back (a script holding them is doing something: the Hole's arena takes fighting while the announcer speaks, then opens the player's gate), and at a door that stays locked (or, unlocked, does
not take the player: a vault door "activated elsewhere") tries the door's switch (one of its activate parents first, XAPR: Vault 112's floor hatch opens only from them, the XAPD flag [observed]; then a script that
names the door), a talking activator beside it, any other scripted activator beside it, a terminal entry
that names the door, then another way round (a load door that is not locked but turns the player away too: Paradise Falls' sewer grate).

## 4. What it reports

`fnv.pilot.report/1`: steps (with errors), observations, divergences from the headless trace,
findings, the watchdog's soft-lock count, and `headless` (what was still done the headless way).
Finding kinds: `unreachable`, `stuck`, `could_not_target`, `no_way_out`, `line_not_said`,
`no_conversation`, `never_came`, `item_not_found`, `fight_not_won`, `wait_timed_out`,
`precondition`, `menu_closed`, `no_navmesh_way`, `no_navmesh_under`, `not_carried_out`, `step_timed_out`, `chain_abandoned`,
`pilot_held`.

Limits: a step 240 s; a wait 220 s; a fight 75 s; 12 failed steps in a row abandon a chain (a read, a note or a wait that passes between them does not break the run); 120 s of real time
in which the pilot cannot act end the run.

## 5. What it has found so far (2026-11-13/14)

- Hardin's power armour training holds the player for good (#2309): found by the watchdog.
- The Silver Rush intro's customer fights instead of fleeing, and the Van Graffs join him
  (#2520, fix #2522).
- Companions die outside Hardcore (#2623): Cass, Veronica and Boone were killed; Veronica's
  death left the Hidden Valley bunker door locked.
- Every other stall so far was the pilot's own (no level-up answer, the wrong box button, a
  conversation broken off, a fight not fought). The places players report breaks (Hidden Valley,
  Freeside, the Strip) are where it is being pointed now.

## 7. Every chain at once

`tools/smoke/pilot/batch.py` runs every walkthrough chain (or the ones named) through the pilot
and ranks them:

```
python tools/smoke/pilot/batch.py --exe <nvforever.exe> --data <Data> --fnvtool <fnvtool.exe>     --home F:/nvf-pilot-home [--jobs 2] [--minutes 30] [--only beyond-the-beef gi-blues] [--skip-fail]
```

Each chain gets its headless trace (cached), a game of its own in `<home>/batch-<slot>` started from
`<home>/saves/pilot-after-intro.nvfsave` (a world past the opening; `--new-game` when it is not
there), `--pilot-campaign --pilot-given`, and at most `--minutes`. `summary.md` and `summary.json`
in `--out` (default `target/pilot-batch/`) list the chains worst first: soft locks, then findings
that stop a quest (`no_way_out`, `unreachable`, `never_came`, `chain_abandoned`), then the pilot's
own trouble; each row says what was still done the headless way. With 217 chains and two games at
once a full pass takes about a day; `--skip-fail` leaves out the 51 `fail-*` chains (a
quest's failure paths).

A mod list (Tale of Two Wastelands, #2761): `tools/smoke/pilot/mo2_merge.py --instance <MO2 folder>
--profile <name> --out <folder>` writes the profile merged into a game folder of its own as hard
links (the game and the mods are never written) and its `plugins.txt`; then `batch.py --data
<folder>/Data --plugins-file <folder>/plugins.txt` moves each chain's FormIDs to that load order
for the headless trace and the game, and `--chains-dir crates/fnv-game/walkthrough/ttw` adds the
Fallout 3 chains. Give the batch a `--home` of its own with the mod list's start save.

Read a finding against the chain's notes before calling it a bug: a chain run from its `given`
state carries the false alarms a thin `given` brings (§2).

## 6. Not yet

- A terminal opened from a message box button is refused in play (East Pump Station, Vault 34):
  being looked into separately.
- Melee, cover and reach as the NPCs judge them in fights.
- A `package` step's own end is not checked afterwards (patrols, sandboxes and guards never end; a deadline belongs to the scene).
- From the batch triage of 2026-11-15 (the pilot's own trouble, not filed as bugs):
  picking a lock within the player's Lockpick skill (The Pitt's mine door); going to,
  talking to or searching the actor who holds a door's key (Pacer's key at the King's
  school); waiting for an actor a schedule takes behind a locked door (Francine at the
  Atomic Wrangler); a chain's made-up `SetPos` off the navmesh (set onto the nearest
  point within 4,096 units); talking across a counter (the Gun Runners' Vendortron);
  a goal no door reaches (Lucky 38's top floor, reached by scripted travel); a walk
  planned out of a place that locks behind the player (Dead Money's theater: the pilot
  walked out on the way to Dean's holotape, then met the theater door at lock level 255); waiting out the end of a fight before activating (ED-E's
  repair box refused "in combat"); the scripted activators near a door taken for its
  switch (the Atomic Wrangler's slot machines).
- The static sweep (`fnvtool`, #2365 item 5) is its own work.
