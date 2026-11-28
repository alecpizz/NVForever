# Player reports: opt-in session data, crash reports and a dashboard

[design, proposal 2026-11-10] A specification for collecting data from players' games so
bugs and crashes can be found and fixed without a player having to file an issue by hand.
What is built is listed under "Status" below. Owner's direction (2026-11-10): opt-in only, asked once in a
popup at the first launch; once a player opts in, their sessions are sent automatically;
a player can also send a report by hand; the receiving end and a dashboard run on
Cloudflare.

What exists already and is reused, not rewritten:

- [documented: `engine/nvforever/src/crash.rs`] The run log
  (`<home>\logs\nvforever-<UTC time>-<pid>.log`, last 10 kept), the `CLEAN_EXIT` line, the
  check at start for an earlier run that ended with no clean exit and no crash report (a
  native crash, a kill), crash reports for panics (`crash-<time>-<pid>.txt`: thread,
  message, location, backtrace, command line, cell and position), and `redact`, which
  writes the player's home folder as `%USERPROFILE%` (#1710). Off Windows it also
  hides the removable-drive mounts under the player's name (`/run/media/<user>`,
  `/media/<user>`); one list (`crash::name_folders`) feeds it, the reports and the Linux
  signal handler. A report also writes the Data, game and NVForever folders and, on a
  Mod Organizer 2 run, the instance and its mods and overwrite folders as names
  (`<Data>`, `<MO2 mods>`, ...) (#3152).
- [documented: `docs/design/frame-telemetry.md`] Bounded frame telemetry (JSONL schema 1),
  a developer aid behind `NVF_PLAY_TEST_FRAME_TELEMETRY`. Its schema is a model for the
  performance part below, but it stays a developer aid; nothing below turns it on.
- [documented: `engine/nvforever/src/setup.rs`] The first-start flow (welcome box, folder
  picker) and `NVForever.ini` in the home, where the choice below is stored.

## Status (2026-11-11)

[documented: `engine/nvforever/src/reports/`, `crash.rs`, `native_crash.rs`, `events_log.rs`]

- Built: the popup and INI keys (§2), the session summary with plugin sizes and CRC32s
  (§3.1), crash reports with the run log, the flight recorder, the world snapshot and, on
  Windows, a minidump, on Linux a native crash's signal line and top frames in the run log
  (§3.2), "Delete my reports" (§6), `[Log]` levels and folded repeats
  (§4.1), the structured events file (§4.1, local only, not sent), the flight recorder
  (§4.2, log events only), the world snapshot (§4.3) and the anomaly detectors in
  `anomaly.rs` (§4.4), and the hand report form (§3.3, #2151).
- Not yet:
  - the settings page's "Open reports folder" button and last-sent line (§1 point 3, §2.2:
    the Reports rows are only Send reports and Delete my reports);
  - the summary's count of loose files in Data (§3.1; it needs the server's schema first);
  - breadcrumbs the log does not already carry (§4.2);
  - the AI's procedure and the most expensive scripts in the snapshot (§4.3);
  - the script-time detector and thresholds set from
    `/v1/config` (§4.4);
  - symbolising dumps (§5.4: the release PDB is over the Workers request limit).

## 1. Principles

1. **Off until the player says yes.** No byte leaves the machine before the player has
   answered the first-launch popup with yes. "Not now", closing the popup, a scripted run
   (`--data` / `NVFOREVER_DATA`, the smoke tests) and any run where the popup could not be
   shown all mean off.
2. **Nothing personal, nothing from the game's files.** No account names, no paths outside
   `redact`, no IP addresses kept (§6), no save files unless the player attaches one by
   hand (§3.3). No game data is sent beyond names and form IDs the engine already logs
   (the charter's "ship no game data" holds both ways: reports are kept private, §6).
3. **The player can see it.** Everything that is sent is first written as a file in
   `<home>\reports\` the player can open; the settings page has a button that opens the
   folder and a line saying how many reports were sent and when.
4. **Cheap and bounded.** One small upload per session, sent at the next start (never
   while playing), with hard size caps. The game never waits on the network: no upload
   blocks a frame, a load or the exit.
5. **Revocable.** Turning it off stops sending at once. A "Delete my reports" button asks
   the server to delete everything sent under this install's ID (§6).

## 2. The choice

### 2.1 The first-launch popup

[design] Shown once, after the game folder is settled (`setup.rs`) and before the main
menu, on a player's start only (not with `--data`, `NVFOREVER_DATA`, a test aid or MO2's
`--no-dialogs`-style scripted start). Our own words, not the original's:

> **Help fix NVForever?**
> NVForever can send a report after each session: the build, your PC's hardware and
> settings, your mod list, how it ran, the game's log and any problems it noticed.
> No personal details, no saves.
> You can change this at any time in Settings → Gameplay → Bug Reports.
>
> [ Send reports ]  [ Not now ]   *What is sent?* (opens §3 in the browser)

- Neither button is pre-selected; Enter does nothing until one is focused, Esc = Not now.
- The answer is stored straight away (§2.2), so a crash right after still respects it.
- An existing install that predates this feature sees the popup once at its next start.
- If the INI already has an answer, the popup is never shown again. A new version of what
  is sent (a bump of `ConsentVersion`, §2.2, only when §3 grows in kind, not in detail)
  asks again; "no" stays no until asked again.

### 2.2 Stored in `NVForever.ini`

```ini
[Reports]
Send=0              ; 0 off (default, and after "Not now"), 1 on
ConsentVersion=1    ; the version of §3 the player said yes to
InstallId=          ; random UUID v4, made when the player says yes; empty while off
Endpoint=           ; empty = the project's server; set for a self-hosted one
```

- `InstallId` is random, made at opt-in, not derived from hardware or account. Turning
  reports off and on again makes a new one (the old one is unlinked).
- Settings → Gameplay → Bug Reports (`crates/nvf-settings-layout/src/lib.rs` `SECTIONS`;
  the launcher shows the same rows): a toggle
  (Send reports) and "Delete my reports" (§6); "Open reports folder" and the last sent
  time are designed but not built. The hand report (§3.3) is opened from the
  pause menu's "Report a bug", not from Settings.
- `NVF_REPORTS=0` forces off for any run (developers, CI, the smoke tests set it).

## 3. What is captured

Three kinds. Kinds 1 and 2 are automatic once opted in; kind 3 is only by hand.

### 3.1 Session summary (every session, automatic)

[design] Written by the game to `<home>\reports\pending\session-<UTC time>-<pid>.json`
as it runs (rewritten every 60 s and at exit, so a crash leaves the latest copy), and
sent at the **next** start. One JSON document, `schema: "nvf-session/1"`, capped at 64 KiB:

| Field | What | Why |
|---|---|---|
| `install_id`, `session_id` | random UUIDs | group sessions of one install; nothing else |
| `build` | version, git commit, profile (release/dev), dirty flag | which build has the bug |
| `start_utc`, `duration_s` | wall clock, rounded to the minute | |
| `end` | `clean` / `panic` / `native` (no clean exit, no crash report) / `killed_by_user` (?) / `unknown` | crash rate per build; `native` is the run-log check `crash.rs` already does |
| `system` | OS name and build, CPU model and core count, RAM (GiB, rounded), GPU name, vendor, driver version, VRAM, graphics API/backend | hardware-specific bugs (e.g. #1425) |
| `display` | resolution, refresh, window mode, HDR on/off | |
| `settings` | the INI's settings that differ from the defaults, by key (no paths: `[Game] Path` and `Endpoint` are left out) | settings-specific bugs |
| `load_order` | plugin file names in order, with size and CRC32; `mo2: bool`; count of loose files in Data | the mod setup; names are public files, sizes/CRCs tell versions apart |
| `play` | cells visited (form IDs + editor IDs, max 200, first visit order), time in each worldspace, main quest stage reached (form ID + index) | where problems happen |
| `perf` | per worldspace/interior: frame-time histogram (fixed buckets 0–8, 8–12, 12–16.7, 16.7–25, 25–33, 33–50, 50–100, 100+ ms), p50/p95/p99, hitches > 100 ms count with the top 10 (time into session, cell, the phase that was longest if known), peak RAM and VRAM | performance regressions by place and hardware |
| `loads` | count and p50/p95 of load times (new game, save load, door, fast travel) | loading regressions |
| `warnings` | counts of warning/error log lines grouped by their message with numbers stripped (top 50 groups, each with one example line, redacted) | the most common problems without the whole log |
| `script_stubs` | names of unimplemented script functions/commands called, with counts | what mods and quests still need |

| `events` | the flight recorder's anomaly events of the session (§4.4), up to 500, each with time, cell, position and its context snapshot | soft bugs that never crash: stuck actors, falls through the world, stalled quests |
| `log` | the whole run log, redacted, zstd-compressed, as a separate part (cap 2 MiB compressed; over that, the first 500 and last 5,000 lines) | the owner wants as much as helps (2026-11-10); the log is the richest source we have |

Not in it: save names, character name, screenshots, typed console text (console
*commands* are recorded by name and arguments' form IDs, not free text).

### 3.2 Crash report (automatic when opted in)

[documented: `engine/nvforever/src/crash.rs`] Local panic reports keep the established
`crash-<UTC second>-<pid>.txt` name. Concurrent or successive panics using that same
name preserve the first report and its recorder/snapshot sidecars. Each later panic
still logs its own thread, message and location, and says that the earlier report was
preserved. A later second can produce another report; the collector's existing latest
report selection is unchanged. This preserves same-name panic evidence only: earlier
GPU errors remain in the run log and structured events, and the panic report does not
establish a GPU failure's cause. Report write errors are still logged, and sidecar
collection is still attempted when writing the text fails.

[design] When a session ends `panic` or `native`, the next start sends with its summary:

- the panic report that `crash.rs` already writes (already redacted), now with the
  flight recorder's last 60 s and a world snapshot (§4.3);
- the session's run log (as in §3.1);
- for `native`: a **minidump** — new work. Windows: a vectored/unhandled-exception
  handler writes a small minidump (thread stacks and module list only, no heap:
  `MiniDumpNormal`-sized, typically < 1 MiB) to `<home>\logs\crash-<time>-<pid>.dmp`, so
  start-up access violations (#1425), shutdown faults (#1825) and driver crashes are
  covered. The dump is binary and is sent as written: the server scrubs the user name
  from its module paths [documented: LootJunkieAzza/NVForever-Data, #2109], but thread
  stacks can still hold other text, so dumps are private like logs (§6). Writing the dump does not depend on opting in (it helps players who send
  files by hand); sending it does. Symbols (PDBs) for each release are uploaded to the
  server's private store at release time (§5.4), never shipped.

  Linux [documented: `engine/nvforever/src/native_crash_unix.rs`, #2167]: there is no
  minidump; a `sigaction` handler for `SIGSEGV`, `SIGBUS`, `SIGABRT`, `SIGFPE` and
  `SIGILL`, on a `sigaltstack` of its own (so a stack overflow is covered too), writes the
  evidence into the session's run log at the moment of the signal — a line, then the top
  frames:

  ```
  20261112-183005 ERROR nvforever::crash: native crash, signal SIGSEGV (11), fault address 0x10; at 0x7f2c1a0b2e40 /usr/lib/libc.so.6+0x2a2e0
  20261112-183005 ERROR nvforever::crash:   frame 1: 0x7f2c1a0b2e40 /usr/lib/libc.so.6+0x2a2e0
  ```

  The line is written with `write(2)` from a fixed buffer (no allocation, no lock: the
  log plugin itself may be what crashed); the module and offset come from
  `/proc/self/maps`, the frames from libgcc's `_Unwind_Backtrace` (addresses only; the
  release's line tables symbolise them later, §5.4). The handler is armed from
  `.init_array`, before Rust's std installs its own SIGSEGV handler (std's per-thread
  alternate stacks are too small for the unwinder); the alternate stack this module
  installs is the main thread's, so a stack overflow there leaves the line, while one on
  another thread (none of which has an alternate stack) does not. The handler then restores the
  default disposition and re-raises, so the process still ends by the same signal (exit
  status `-11`, which `tools/smoke` reports as a failed scenario rather than a pass). The
  home folder and the removable-drive mounts under the player's name (`/run/media/<user>`,
  `/media/<user>`) are written `~` as everywhere else. Because the evidence is in the run log,
  a Linux `native` end is log-only (`native-nofile`) until a Linux dump pipeline exists;
  nothing new is uploaded. (`native-nofile` is a server-side report kind: nothing in this
  repo implements it yet — planned, and its home is the reports service, not the engine.)

**A run that just stops** (#2755) [documented: `engine/nvforever/src/exit_watch.rs`,
`crash.rs`]: 13 sessions on 0.7.0 ended mid-play with no panic, no dump and no line. The
exception filter does not see a stack overflow (it returns at once: no stack to write
with), a fail-fast or abort (no handler runs; Rust's abort on a failed allocation is one),
a crash after a driver replaced the filter, or the process being ended from outside. What
tells them apart is the process's exit code, which only another process can read:

- At its start the game starts a watcher: the same executable with `--watch-exit <pid>
  <run log>`, no window, which waits for the game's process, reads its exit code and, when
  it is not 0, adds a line to the end of that run's log: `run log: the process ended with
  exit code 0xC00000FD (stack overflow)`. Codes named: `0xC00000FD` stack overflow,
  `0xC0000409` fail-fast, `0xC0000005` access violation, `0xC0000374` heap corruption,
  `0xC0000017` no memory, `0xC000013A` console closed, 1 an error exit or ended from
  outside, 101 a panic in the main thread. Windows only; `NVF_EXIT_WATCH=0` turns it off.
- The line is part of the run log, so it is in the log sent with the crash, and the next
  start repeats it (`run log: the previous run's process ended with exit code …`).
- Something outside that ends the game writes `run log: ended from outside: <key> (<words>)`
  as it happens (`run_end.rs`; keys `console-closed`, `sign-out`, `shutdown`, `close-app`):
  a console control handler, added after Bevy's own so that it is called first, and a hidden
  window that Windows tells when it ends the session (`WM_ENDSESSION`). Ctrl+C and
  Ctrl+Break are not noted: Bevy closes the game normally on them (exit code 130).
  Checked in the game [observed, 2026-11-27]: the game's console window closed (WM_CLOSE
  to it) writes `console-closed` and the process ends with `0xC000013A`, as in the
  player's log; `WM_ENDSESSION` sent to the hidden window writes `shutdown`. `nvforever.exe` is a console program,
  so a player started from Explorer or the launcher has a console window beside the game,
  and closing it ends the game. Found from reports of 0.8.0-0.8.1 [observed, 2026-11-27]:
  the RTX 4060 install of this issue stopped so six times on builds with the watcher; one
  log ends with `0xC000013A` (console closed or Ctrl+C), the other five record no exit code
  at all (the watcher ended too, or the code was 0 [hypothesis: Windows shutting down or
  signing out, which ends both]), and each next start came 7 to 23 hours later. The next
  start repeats the reason (`run log: the previous run was ended from outside: …`). The
  run is still reported as not ended normally. `NVF_RUN_END=0` turns it off.
- An allocation that fails writes `out of memory: an allocation of N bytes failed` into
  the run log before Rust aborts (the game's global allocator, crash.rs).
- A lost graphics device (the driver stopped answering) ends the run as one (#2934): the
  frames in flight usually panic first on the dead device, so from Bevy's loss line on a
  panic's crash report says `thread: the graphics driver`, `message: graphics device lost:
  <reason>`, `at: reported by wgpu`, with the panic that followed and its backtrace under
  `backtrace:`. The session's `end` is `panic` as before; the message, and so the
  signature, is the loss's whatever panic followed. With no panic the handler Bevy asks
  writes the same report. The player gets a message saying the graphics driver stopped
  responding and what to try (not in unattended runs), as for GPU memory running out.
- [design, not built] The summary carries no `exit_code` field yet: the server's schema
  check would have to know it first. Until then the dashboard can split `native` ends by
  the log's line.

The crash is grouped server-side by a **signature**: for panics, the message with numbers
stripped plus the top 3 frames inside NVForever's own crates; for dumps, the faulting
module + offset bucket plus the top 3 symbolised frames; for a Linux `native` end, the run
log's signal line (module + offset) and its top frames.

### 3.3 Report by hand (any player, opted in or not)

[design] "Send a report now…" in Settings, and a key in the pause menu (bindable, default
none in release builds; F12-style keys are taken). It opens a small form:

- a description (free text, required) and a category (bug / crash / differs from the
  original / performance), matching the issue forms in `.github/ISSUE_TEMPLATE`;
- ticked by default: the current session summary so far, the last 2,000 log lines,
  a screenshot of the frame before the form opened (shown as a thumbnail; untick to drop);
- unticked by default: the current save (a fresh save written to the report, not the
  player's own files; capped at 20 MiB), contact (a GitHub name, for us to reply on an issue).

A player who has not opted in sees the same form; sending it is consent for that one
report only and does not change `Send`. The form shows what will be sent and the total
size before the Send button.

[built, #2151] `engine/nvforever/src/reports/form.rs`. Opened from **Report a bug** at the
pause menu's bottom left, over the version line, on its first page and the settings page
(in a game; not under the main menu) [owner 2026-11-12: a pause-menu entry, not a settings
row]; the pause-menu key is not built yet.
It also sends the world snapshot and the flight recorder (§4.2, §4.3) under the summary's
and the log's tick boxes; without reports on there is no summary, only the snapshot, and
no install ID. Sending follows §8: never from a developer or test run without
`NVF_REPORTS_ENDPOINT`, the project's server only from an official build. The screenshot
is PNG, at most 1280 wide and halved until under 1.5 MiB; the save is written for the
report under `<home>\reports\form\` and deleted once sent. Bevy's font draws the form
[our choice: the player's text may be in any script].

## 4. Extending the logger for bug hunting

[design] Owner (2026-11-10): capture as much as helps us find bugs, and extend the logger
for it. Today the run log is plain text lines from `tracing` (`crash.rs`'s layer). Four
additions, all local first (useful in `<home>\logs` to anyone, opted in or not), sent
only under §2.

### 4.1 Structured events

- A second `tracing` layer writes `<home>\logs\events-<time>-<pid>.jsonl` beside the run
  log: every event at `info` and above as one JSON line with `t` (ms since start),
  `frame`, `level`, `target` (module), `msg`, its fields, and the **context**: worldspace
  or interior, cell form ID, player position, game hour, game state (menu, loading,
  playing, dialogue, VATS, Pip-Boy).
- Engine code logs with fields instead of formatting values into the message
  (`warn!(form = %id, script = %edid, "script error")`), so the server can group and
  filter. Existing lines keep working; we convert the noisy and important ones first.
- Log targets per subsystem (`nvf::script`, `nvf::ai`, `nvf::nav`, `nvf::physics`,
  `nvf::assets`, `nvf::save`, `nvf::quest`, `nvf::dialogue`, `nvf::render`, `nvf::audio`,
  `nvf::ui`), with levels settable in the INI (`[Log] script=debug`), so a player asked
  to reproduce a bug can turn one subsystem up.
- Repeated lines are folded (`… repeated 312 times in 4 s`) so a spammed warning cannot
  push the useful lines out of the caps.

### 4.2 The flight recorder

An in-memory ring buffer of the last **60 s** of `debug`-level events and breadcrumbs,
kept always (bounded at 8 MiB), written out only when something goes wrong (a panic, an
anomaly, a hand report). Breadcrumbs, recorded cheaply each time they happen:

- cell changes, loads (kind, time taken), saves, fast travel, doors;
- quest stage changes, dialogue topics chosen (form IDs), menus opened;
- scripts started/stopped, script errors (script editor ID, the command, the line in the
  compiled script), unimplemented functions called;
- combat start/end, actors killed, the player's damage taken and source;
- console commands run (name and form IDs), keys and buttons that changed state (actions,
  not raw keys: "activate", "jump");
- asset failures (missing or broken NIF, DDS, sound, with the path inside the archive);
- AI package changes of actors within 2 cells, pathfinding failures (start, goal, reason);
- physics: actor velocity spikes, teleports by the engine, ragdoll starts.

### 4.3 World snapshot

On a panic, an anomaly or a hand report, a JSON snapshot (cap 256 KiB) of the state most
bugs need: the player (position, cell, health, AP, limb states, active effects,
equipped items, level, SPECIAL, karma, faction reputations), quests (running quests with
stage and objectives), global variables changed from their default, the 30 nearest
actors (form ID, base, editor ID, position, AI package and procedure, combat target,
health, dead/unconscious, current animation), the active script count and the 20 most
expensive scripts this second, loaded cells, memory (RAM, VRAM, asset cache sizes).

### 4.4 Anomaly detectors

[design / file refs: `engine/nvforever/src/reports/{mod,anomaly,controls,snapshot}.rs`]
Checks sampled each frame or from log events, recorded in the session summary's `events`
with place, cell, position, game hour and state. The world snapshot is refreshed separately
(§4.3). Each kind fires at most 3 times per session; `controls_locked` also fires only once
per unchanged lock. The detectors are registered only while reports are enabled.

| Anomaly | When |
|---|---|
| fell out of world | the player or an actor is below the land height by > 500 units, or outside the cell grid |
| object fell through | a dropped item or a corpse sank through the land and the engine put it back (#2753, #2758) |
| GPU error | a GPU error stopped the game: out of GPU memory or a lost device (#2754, #2758) |
| player stuck | movement input held for 5 s with < 16 units moved, not in a menu |
| controls locked (`controls_locked`) | [design, #2403] unchanged dialogue, menu or scripted control groups for 60 real seconds, with at least 5 key/mouse/controller button press edges in the trailing 60 seconds |
| path failure | pathfinding fails 3 times in a row for one actor |
| script loop | one script runs > 5 ms in a frame, or the same script errors 100 times |
| hitch | a frame > 250 ms outside a load; frames with the game away (window unfocused, covered or minimised, app suspended, the second after each, or a frame of 30 s or more: the machine slept) count neither as hitches nor in the places' frame times [design, #3043] |
| slow load | a load > 20 s; it says where the time went: each wait's end, the longest frame, the files read and the time in the reads (docs/design/loading-screen.md §1 step 7, #2407) |
| memory | RAM or VRAM within 10 % of the budget, once a game is loaded (not at the start, when the RAM in use is other programs') |
| missing asset | the first failure of each path. For a model or an actor file that a plugin's record names, the event says the `plugin` (the winning record's; a mod's over the game's own when both name it). A file the base game's or an add-on's own record names and no archive has is not reported: it is the data's own dangling name (`meshes\creatures\ghoul\mmm_ghoulvariant_v1.nif`, in FalloutNV.esm creatures' model lists, 11 of 49 installs of build 0.8.0) [observed, #3045; `reports/asset_sources.rs`]. A file of the game's that is there and does not read is still reported. Textures and particles have no record of their own and are reported as before |
| save trouble | a save that fails to write, or a load that falls back (#1360) |
| audio | the device lost or a voice file missing for a spoken line (a line with no words, such as `...`, has none by design) |
| soft lock | the player is held and nothing moves on to let them go: scripts hold the movement controls (or the player is restrained) for 30 s with no story event; a conversation with no line and no choice for 5 s; one response said for 120 s; a menu set under a conversation that never opens (`fnv_game::softlock`, #2365) |

[design, #2403 / file refs: `reports/controls.rs`, `reports/anomaly.rs`]
`controls_locked` resets its timer, input window and once-per-lock latch when the dialogue
line/response or offered choices change, a menu/page/tab/selection changes, any effective
control group changes (including a group re-enabled), or the hold ends. Loading clears the
watch. Countdown and animation clocks do not count as progress. Keyboard auto-repeat and
holding a button do not add presses; controller menu keys synthesized by `nvf_play::pad`
are not counted again. The built-in progress audit below follows `nvf-play/src/script.rs::{hold_player,
EngineMenus,is_script_menu}` and the resources read by `reports/controls.rs::Menus`.
Every fingerprint stays local; screen text and input identities are not report fields.

[design, #2403 / file refs: `reports/controls.rs`; resource paths below are under
`engine/nvf-play/src/` unless prefixed `crates/`]

| Built-in hold | Logical state sampled | State owner |
|---|---|---|
| Recipes / crafting | Station/category, filter, selected/first row, quantity prompt/amount; recipe readiness, ingredients held/required and skill figures | `crafting.rs::RecipeMenu` |
| Lockpicking | Reference, pin angle, cylinder turn, pins remaining, phase identity, status/help, leaving flag | `lockpick.rs::LockpickScreen` |
| Hacking | Reference/base, page, cursor, puzzle characters, remaining attempts, log and outcome; lockout lines | `terminal.rs::TerminalScreen` |
| Terminal | Reference/base, page, submenu ancestry, item IDs/labels, selected row, result, note and note scroll | `terminal.rs::TerminalScreen` |
| Inventory / Pip-Boy ITEMS | Mode/focus, selection/scroll/category, folder, quantity prompt/amount, status; list sizes/values and selected item/count/condition/equipped/mod state; hotkey slot, assignment feedback and assignments | `menu/model.rs::MenuModel`, `pipboy/hotkey_panel.rs::HotkeyHold`, `hotkeys.rs::Hotkeys` |
| Barter | Inventory signals plus offered units, purses and trade balance | `menu/model.rs::MenuModel` |
| Container | Inventory signals plus Take All confirmation | `menu/model.rs::MenuModel` |
| Repair services | Vendor, selected/first row, confirmation, item conditions/quotes/readiness, skill, caps and total | `repair.rs::RepairServices` |
| Pip-Boy repair | Selected/first row, preview, target condition, offered parts and merge parameters | `pipboy/repair.rs::PipRepair` |
| Item modification | Selected/first row, target unit/mod flags and offered modifications/counts/attachment state | `pipboy/item_mod.rs::PipItemMod` |
| Sleep / wait | Sleep/wait mode, chosen hours, accepted hours and whole hours remaining | `wait.rs::WaitMenu` |
| Local / world map | Page/tab, map centre, magnification, cursor, custom marker; popup identity/text/answers and selected answer | `pipboy.rs::PipBoy`, `pipboy/data.rs::DataState` |
| Pip-Boy DATA lists | Selected/first row, challenges and active-note filters, pane identity/scroll, active quest; tuned radio station/on state; playing holotape identity/pause state | `pipboy/data.rs::DataState`, `holotape.rs::Holotape`, `crates/fnv-game/src/game.rs::Game` |
| Pip-Boy STATS | Page/tab, status subpage, selected/top rows, description scroll, reputation mode and limb; displayed HP/AP/XP, limb condition, rads and needs | `pipboy/stats/mod.rs::{StatsView,Prepared}` |
| Dialogue | Speaker, line/response IDs, line serial/count, offered choices/topic-list mode and selected choice | `dialogue.rs::DialogueScreen`, `crates/fnv-game/src/dialogue.rs::Conversation` |
| Level-up | Level/page, selected row/scroll, skill allocation, perk choices and answered flag | `levelup.rs::LevelUpMenu` |
| Messages | Box serial, panel content/buttons, selected button and initial choice flag | `message_menu.rs::MessagePanel`, `crates/fnv-game/src/game.rs::Game` |
| Standalone quantity | Chosen amount and maximum | `script_quantity.rs::ScriptQuantity` |
| Vigor Tester | Page, review row and attribute allocation | `chargen/vigor.rs::VigorMenu` |
| Tutorial | Page, message IDs and text scroll | `tutorial_menu.rs::TutorialView` |
| Name / tag skills / traits / attributes | Name text, list selection/scroll, allocation sheet and answered flag | `chargen.rs::ChargenMenus` |
| Race / appearance / barber | Look, tab, rows/groups/scroll, preset, action selection, camera view and answered flag | `creator.rs::CreatorMenu`, `creator/model.rs::Model` |
| Pause / settings / save pages | Page, logical `PauseView::progress` and settings tab | `pause.rs::{Page,PauseView,SettingsTabStore}` |
| Console | Input, history recall, output scroll/lines and selected reference | `console.rs::Console` |
| Companion wheel | Companion, selected command, command state and reply line | `companion_wheel.rs::WheelScreen`, `script_hud.rs::ScriptHud` |
| Blackjack | Bet/chips/earnings, help/status, phase, hands/active hand, dealer cards/reveal, completed rounds and leaving flag | `blackjack.rs::BlackjackScreen` |
| Roulette | Bet/cursor, placed bets/completed spins, phase identity, chips/earnings, help/status, last pocket and ban flag | `roulette.rs::RouletteScreen` |
| Slots | Bet, reel stops, phase identity, chips/earnings, help/status, payout-page and ban flags | `slots.rs::SlotsScreen` |
| Poker | Raise amount, status/end reason/leaving flag, seats, button, board/street/turn, current bet, hand number and winners/showdown | `poker.rs::PokerScreen` |
| Caravan | Stage, bet/deck/cursor, hand/pick, forfeit confirmation, help/status, sides/turn/phase/winner and leaving flag | `caravan.rs::CaravanScreen` |
| Script control locks / fallback menu | Effective standard/Alt control-group flags, runtime menu code/prompt, message serial and dialogue signals above | `crates/fnv-game/src/game.rs::Game` |

[design, #2403 / file refs: the state owners in the table]
Redraw flags, pointer-only hover bookkeeping, attempted-navigation counters, RNG,
scheduled sounds, radio/holotape playback clocks, partial-hour wait timers, pin-break
age and fade/animation timestamps do not count as progress. Actual selection movement
from the pointer does count. Hidden inactive STATS state is ignored. Regressions in
`reports/controls.rs` exercise each menu family through the detector: logical changes
restart the 60-second watch, while unchanged state and incidental counters/clocks with
ineffective presses still raise one finding. The detector does not inspect pixels or
arbitrary scripted tile values; a page change entirely inside a mod's tile tree is not
currently a progress signal.

[design, #2403 / file refs: `reports/controls.rs`, `fnv-game/src/nvse_other.rs`]
The finding names the menu or speaker (reference and bounded name) and the current topic,
INFO and response, if a line is active, plus up to ten offered topic IDs. It includes the
locked groups, their known source API, and up to ten Alt-lock plugin sources, plus an
omitted count. Standard
`DisablePlayerControls` flags retain no caller, so their script is explicitly unknown
(`null`); no script is inferred from a running quest. Up to ten running, uncompleted
quests (including background quests), in FormID order, supply their IDs and current stages.
Names are capped at 128 characters. No input key identities, dialogue text or quest text
are added. These limits and the 60-second/five-press rule are named together in
`reports/anomaly.rs`.

[design, #2403 / file refs: `residents.rs`, `fnv-ai/src/carry.rs`,
`fnv-nav/src/follow.rs`, `fnv-game/src/{quest,quest_targets}.rs`]
Removed from this reports specification: `actor_stuck` and `quest_stall`. Reports can read
actor positions and current package IDs, but not a travel goal/progress contract spanning
detours, doors and replans; distance travelled alone is not progress toward the goal.
That requires coordinated AI/navigation state beyond a small read-only reports hook.
Quest target checks exist, but reports has no main-quest-only selection or separate
quest-stall opt-in. Applying the old rule to all quests would broaden its scope and flag
intentionally dead/disabled targets; an absent snapshot reference also does not establish
that it has no cell. These detectors need those contracts before being specified again.
The separate existing `softlock` watchdog remains built as listed above.

[design / file refs: `reports/anomaly.rs`] Thresholds are local constants. Server
`/v1/config` tuning is not built.

## 5. The server (Cloudflare)

[our choice] Cloudflare's stack fits: a Worker takes uploads, D1 (SQLite) keeps the rows
the dashboard queries, R2 keeps the blobs (logs, dumps, screenshots, saves), Pages hosts
the dashboard, Cloudflare Access guards it. The free/paid tiers cover this scale; costs
are bounded by the caps below.

### 5.1 Upload API (Worker, `reports.<project domain>`)

```
POST /v1/session      body: nvf-session/1 JSON (gzip)            → 202 {id}
POST /v1/crash        multipart: summary JSON + log + dump       → 202 {id}
POST /v1/report       multipart: form fields + attachments      → 202 {id, ref}
DELETE /v1/install/{install_id}   header X-NVF-Delete-Token      → 202
GET  /v1/config       → {min_build, sampling, kill: bool, consent_version}
```

- Validation: JSON schema check, size caps per part (summary 64 KiB, log 2 MiB, dump
  4 MiB, screenshot 2 MiB, save 20 MiB, whole request 25 MiB), content types, gzip only.
  Anything else → 400 and the client deletes the pending file (no retry loop).
- Abuse: Workers rate limiting per install ID and per IP (IP used only for the limit,
  never stored): 20 sessions / 5 hand reports / 5 crashes per install per day. No API
  keys in the client (they would be public anyway); a build header lets bad builds be
  turned away by `/v1/config`.
- `DELETE` uses a random token the client makes with the install ID and keeps in the INI
  (`DeleteToken=`), so nobody else can delete an install's reports.
- `kill: true` in `/v1/config` stops all sending at once (e.g. a bad build floods us).

### 5.2 Storage

- **D1** tables: `sessions` (one row per summary, the flat fields plus JSON columns),
  `crashes` (session ref, signature, kind, build), `signatures` (first/last seen, count,
  builds, linked GitHub issue), `reports` (hand reports: category, text, status, linked
  issue), `perf_buckets` (per build × place × GPU vendor histogram rows, for fast charts).
- **R2** keys: `crash/<signature>/<id>/{log.txt.gz,dump.dmp,panic.txt}`,
  `report/<id>/{screenshot.png,save.fos,log.txt.gz}`, `symbols/<version>/<pdb>`.
- **Retention**: raw logs, dumps, screenshots and saves 90 days; session rows 1 year;
  aggregates (`perf_buckets`, signature counts) kept. A daily Cron Trigger deletes old
  rows and R2 objects.

### 5.3 Processing (Worker, Queues)

Uploads are acknowledged at once and put on a Queue; a consumer:

1. re-runs redaction (Windows user paths, `\\server\` shares, e-mail-like strings) as a
   second line of defence;
2. computes crash signatures; symbolises dumps against `symbols/` (a small Rust/WASM
   symboliser of module + offset → function with the PDB's public symbols, or done by a
   GitHub Action on demand if WASM proves too heavy);
3. updates `signatures` and `perf_buckets`;
4. for a new signature seen from ≥ 3 installs, opens a **draft** GitHub issue (label
   `player-report`, signature, counts, builds, top frames, no log text) through a GitHub
   App with issues-only scope. A human or session decides whether it is real; log text
   and dumps stay in R2, linked from the dashboard only.

### 5.4 Release hooks

`tools/release` gains a step: upload the release's PDBs to R2 `symbols/<version>/` and
register the version (build string → commit) through an authenticated admin endpoint.

## 6. Privacy and the law

- What is sent is listed on a public page (this file's §3 and §4, rendered on the site) linked
  from the popup. The page says what is kept, for how long, and how to delete it.
- No IP addresses, no cookies, no account. Cloudflare's own request logs are set to not
  keep IPs for this zone where the plan allows; Logpush off.
- Raw reports are private: only the owner (and people the owner adds in Cloudflare
  Access) can see logs, dumps, screenshots and saves. Aggregates may be made public
  later (owner's call).
- Deletion: "Delete my reports" calls `DELETE /v1/install/{id}`; the Worker removes rows
  and R2 objects for that install within 7 days and the client then makes new IDs.
- GDPR: lawful basis is consent; the popup is the consent; the install ID is
  pseudonymous; the delete button covers erasure. A save attached by hand can hold a
  character name the player chose: the form says so.

## 7. The dashboard (Pages + Access)

One page app reading D1 through a read-only Worker API, behind Cloudflare Access (GitHub
login, allow-list).

- **Overview**: sessions/day, crash-free session rate per build (the headline number),
  active installs per build, OS/GPU vendor split.
- **Crashes**: signatures ranked by installs affected, sparkline per build, first/last
  build seen, linked issue; a signature's page lists its crashes with log tail, panic
  text, symbolised stack, system details, load order diff from the most common one.
- **Performance**: p50/p95/p99 frame time by build × place (worldspace, interior), with a
  build-to-build compare; hitch places ranked; load-time trends.
- **Mods**: plugins most present in crashing sessions vs. clean ones (a simple ratio,
  min 20 sessions), to spot mod-specific breakage.
- **Script stubs**: unimplemented functions ranked by installs calling them (feeds
  `docs/design/script-stubs.md` and the NVSE tracking issue #1027).
- **Hand reports**: inbox with status (new / triaged / issue opened / not a bug / dup),
  screenshot, text, attachments, button to open a GitHub issue prefilled from the report
  (text and summary only; attachments stay private).

## 8. Client implementation notes

- New module `engine/nvforever/src/reports/` (`consent.rs` popup and INI, `session.rs`
  collector, `upload.rs` sender, `form.rs` hand report). The collector reads what the
  engine already counts (the stress sampler's phases, the perf overlay, `crash.rs`'s
  place) rather than adding new timing.
- Sending: at start, after the main menu shows, a background thread sends each file in
  `reports\pending\` (oldest first, at most 5 per start), moves sent ones to
  `reports\sent\` (last 20 kept for the player to see), drops files older than 14 days.
  HTTP with `ureq` or `reqwest` (blocking, rustls), 10 s timeout, no retries in the same
  run. Offline is normal and silent. **As built** [documented:
  `engine/nvforever/src/reports/upload.rs`, dependencies.md]: `ureq` 3 over the system's
  TLS and certificates, on Windows (SChannel) and on Linux (the system's OpenSSL, #3144).
  Until #3144 the Linux build had no HTTP client: Report a bug, Send reports and Delete
  my reports never got an answer there and said the network was at fault. On Linux the
  first-launch popup is still not shown (§2.1), so reports start off and are switched on
  in Settings; an earlier run still going is told from `/proc/<pid>/exe`.
- All files go through `fnv_install::guard_write` under the home; nothing is written to
  the game folder or `Documents\My Games\FalloutNV`.
- **Developers do not send reports** [owner 2026-11-11]: a development build, a
  half-finished change or a test session would skew the numbers that are meant to show
  players' games. Developer and smoke runs (`--data` / `NVFOREVER_DATA`, test aids) never
  ask or send; a developer starting a build like a player answers Not now or sets
  `NVF_REPORTS=0` (CONTRIBUTING.md, CLAUDE.md). `NVF_REPORTS=1` sends only to
  `NVF_REPORTS_ENDPOINT`, never to the project's server. Enforced too: only official
  builds (the release workflow sets `NVF_OFFICIAL_BUILD=1` when it builds) ask and send to
  the project's server; a local build never does, unless it is pointed at a server of its
  own or `NVF_REPORTS_PRODUCTION=1` is set (the owner's own testing).
- Test aids: `NVF_REPORTS_ENDPOINT` to point at a local `wrangler dev` server; a smoke
  scenario that checks the popup is not shown in a scripted run, that `NVF_REPORTS=0`
  sends nothing, and that a session file is written and is valid `nvf-session/1`.
- The server lives in `services/reports/` (Worker + dashboard, TypeScript or Rust-WASM,
  `wrangler.toml`), with its own tests; it holds no secrets in the repo.

## 9. Order of work

1. Consent popup, INI keys, settings rows, `NVF_REPORTS=0` (nothing sent yet).
2. Logger: structured events, subsystem targets, folding, flight recorder, world
   snapshot (§4.1–4.3); useful locally before any server exists.
3. Session summary written locally to `reports\pending\`; anomaly detectors (§4.4).
4. Worker + D1 + R2 receiving sessions and crashes; the sender in the client.
5. Dashboard: overview and crashes.
6. Native minidumps + release symbol upload + symbolising.
7. Hand report form with screenshot/save.
8. Performance and mods views; draft-issue automation.

## 10. Open questions for the owner

- The domain for the endpoint and the dashboard, and who besides the owner gets access.
- Whether aggregate numbers (crash-free rate, perf by GPU) are public.
- Whether the hand-report key exists in release builds by default.
- Whether draft issues are opened automatically (step 8) or only from the dashboard.
