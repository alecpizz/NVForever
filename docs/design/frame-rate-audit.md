# Frame-rate audit: the mantis ootheca and the REPCONN test site

Owner's playtest list 7 (2026-10-23): item 4, shooting the mantis egg in the Goodsprings
schoolhouse drops the frame rate to single digits; item 47, the frame rate drops on the
REPCONN test site's facility floor, "not an intense location". What costs the frames
there, what was changed, the numbers before and after, and what is left.

Every number here is [observed] on the owner's desktop (32 threads) while other sessions
built and ran the game beside these runs; single windows vary by 2× with that load, so
only differences far larger than that are claimed. Logs are private
(`<scratchpad>/l7-perf/runs`).

## 1. Measuring without a sampling profiler

xperf needs elevation (docs/design/streaming-hitches.md §1), so two in-engine aids were
added (`engine/nvforever/src/profile.rs`):

- `NVF_PLAY_TEST_PROFILE=<seconds>`: every window logs `profile: <s> s, <n> frames, frame
  mean <m> ms, cpu <c> ms a frame; per frame ms: <span> <ms>, …`: the 40 costliest
  tracing spans, summed over all threads and divided by the frames (each Bevy system by
  name, the schedules, the render graph's nodes, and the spans of the libraries, rapier's
  character controller among them). The spans exist only in a build with Bevy's `trace`
  feature: `cargo build --release -p nvforever --features nvforever/profile` (keep it in
  its own target folder; it rebuilds Bevy). The `cpu` figure is the process's processor
  time over all threads (`GetProcessTimes`), which other programs on a busy machine do not
  inflate as they do wall times; it is there in a normal build too.
- The same windows log `profile scene: <n> mesh entities (<v> visible, <c> moved this
  frame), <m> distinct meshes, <u> distinct materials in use (<a> material assets)`: what
  the renderer batches by.

A span is entered and left on one thread; spans nested in one another all count (the
`update` span holds `main app`, which holds `Update`, …), so the list is read from the
top down to the first leaf that explains the time. `--log-frame-times <s>` (no vsync)
and `NVF_PLAY_TEST_SAMPLE` (stress_sample.rs, actors and audio counts) were used beside
it.

`--log-frame-times <s>` also logs, each period, `gpu resources: <n> buffers, <n> textures,
<n> texture views, <n> bind groups, …` (graphics.rs; wgpu's count of what the GPU API
holds). A count that only grows over a session is a leak, and it costs every frame: each
command buffer takes longer to finish and submit the more there are. It found the bind
groups the lamps' shadow views left behind at every cell change (2026-11-25,
graphics-enhanced.md §3.17 "Third round").

### Profiling options together

[documented in source] `graphics::ensure_render_diagnostics` lets
`NVF_PLAY_TEST_PROFILE=<seconds>` and `--log-frame-times <seconds>` request the same
render diagnostics plugin. Both requests share one registration, including when a
caller has already installed it. With neither option enabled, these paths do not
install it. The enable conditions and existing report formats are unchanged (#1275).

[documented in pinned Bevy 0.20.1 source; `engine/Cargo.lock`,
`bevy_app::Plugin::is_unique`, `App::add_boxed_plugin`, and
`bevy_render::diagnostic::RenderDiagnosticsPlugin`] Registering this unique plugin
twice takes Bevy's duplicate-plugin panic path; the shared guard avoids it.

[observed: release builds of #1275, default and `nvforever/profile`] Both options
alone, together and neither start and finish normally, with the expected reports
present or absent (smoke scenarios `render-diagnostics-*`). Broader profiling and
performance validation remain separate work.

[documented: `crash.rs` installs the tracing subscriber without the `profile`
feature; project `info_span!` calls are unconditional] The profile feature enables
Bevy's own tracing spans, but manually instrumented project spans still appear in a
default build, so a non-empty profile window does not prove the feature is on.

[documented: `bevy_render-0.20.1/src/diagnostic/internal.rs` (checks device
features), `wgpu-hal-30.0.1` adapters] GPU timings depend on the device features
the backend exposes, not on a backend list. GPU timings need `TIMESTAMP_QUERY` and
`TIMESTAMP_QUERY_INSIDE_ENCODERS`, plus `TIMESTAMP_QUERY_INSIDE_PASSES` for scopes
inside a pass (the Vulkan, DX12 and Metal back ends expose these, subject to the
device). Pipeline statistics are separate and optional: they need
`PIPELINE_STATISTICS_QUERY`, which wgpu-hal 30.0.1 does not yet expose on DX12.
Without usable timestamp features the timings are CPU-only. Missing GPU timings are
not zero GPU cost.

[documented: `main.rs`, `settings.rs`] `--log-frame-times` also selects display
sync off and disables the frame cap settings; match effective presentation settings
before comparing instrumentation overhead.

### Tracy: the same spans on a live timeline

The span profile above sums spans into log lines; the Tracy profiler shows them on a
timeline, every thread frame by frame, which is how a single hitch is read rather than
averaged (#1329).

```
cd engine
cargo build --release -p nvforever --features nvforever/tracy    # or nvforever/tracy-memory
```

- `tracy` is `profile` plus Bevy's `trace_tracy`; `tracy-memory` also records every
  allocation (slower). Keep the build in its own target folder: it rebuilds Bevy. A
  default build compiles no Tracy code.
- [documented: `engine/Cargo.lock`, `tracy-client-sys-0.30.1/tracy/common/TracyVersion.hpp`]
  Bevy 0.20.1 pins `tracing-tracy` 0.12.0, `tracy-client` 0.19.0 and `tracy-client-sys`
  0.30.1, which carries the Tracy 0.14.1 client. Use the Tracy 0.14.x viewer
  (github.com/wolfpld/tracy releases); a viewer of another protocol version refuses the
  connection. Recheck this file after a Bevy upgrade.
- [observed 2026-11-01, Goodsprings, `--play`] The game listens on TCP 8086 from start-up;
  start it, then Connect in the viewer (localhost). `NVF_PLAY_TEST_PROFILE` works beside
  it, unchanged.
- [documented: `tracy-client-sys-0.30.1/build.rs`, `tracy/client/TracyProfiler.cpp`,
  `tracy/common/TracySocket.cpp`] The client is built `ondemand` and `only-localhost`
  (`engine/nvforever/Cargo.toml`). On demand: nothing is recorded until a viewer
  connects, so a Tracy build left running without one does not fill memory, and what
  happened before connecting (start-up, the first cell's load) is not in the capture;
  connect first, then trigger the scene. Only localhost: the port listens on the loopback
  address and the viewer-discovery broadcast goes to 127.255.255.255, so nothing is
  reachable from another machine and no firewall rule is needed; profiling across
  machines takes a build without it.
- [hypothesis: Tracy's manual, not checked here] The client also records context switches
  and samples call stacks only when run as administrator on Windows; the spans need
  neither. Frame telemetry lists `tracy` among the build features, so its numbers are
  not mistaken for a plain build's.
- Tracy sees CPU spans only here; the GPU's passes still want PIX or Nsight Graphics.

### The physics line and the `phys::*` zones

[design: `engine/nvf-play/src/phys_load.rs`, `player.rs` `step` and `move_npcs`,
`graphics.rs`, `phys_rate.rs`] `nvf_play::player::step` advances an accumulator of game
time (`fnv_phys::Stepper::advance`, kept in `f64`) and runs the fixed steps the frame's
time owes at the physics rate (60 a second by default; the settings page's Physics rate or
`--physics-hz`, physics.md §5.4), up to a quarter second of them a frame (15 at 60, 38 at
152, 40 at 160: `phys_rate::catch_up_steps`; a whole 0.25 s frame of the virtual clock is
caught up, deliberately, so actors near the player cover the ground the clock passes). Its
cost is therefore about *ticks a frame × ms a tick*, grows with the rate, and grows with the
previous frame's length: a slow frame owes more steps to the next one. With
`--log-frame-times <s>` the system counts and times itself, and each period logs, beside
`frame time:`:

`physics: 2.00 ticks a frame (max 4) over 600 frames, 7.1 controller moves a tick (max 12), 31 blocks, 5.68 ms a frame in player::step, 2.84 ms a tick, at 152 Hz`

(an example of the format, not a measurement). The rate at the end is the one in effect
over the period, or the range it ran over (`at 120-152 Hz`) when the cost guard switched it
(physics.md §5.4). Frames are those in which the system ran
while playing; frames with no player character or waiting for a cell to load are not
counted (`Meter::skip`). Controller moves are the player's (on the walking path) and each
walked actor's per step; blocks is the mean length of the actor block list
(`crate::blocking`); the two times are `Instant`-based, the whole system per frame and the
steps alone per step. Without frame-time logging no `PhysLoad` statistics are kept, the
steps are not timed one by one and the line is not logged (the `phys::*` spans are
independent of it); the system's whole time is still read, two `Instant`s a frame, for the
physics rate's cost guard (`phys_rate::Guard`).
The two lines do not count the same frames: the physics counts cover the frames of the
period in which the game was playing (less those skipped above), while the
`frame time:` mean also counts frames that were not (menus, loading) and, only with
`--graphics-cycle`, leaves out the first 0.5 s of each period (`graphics.rs`). The logging
system and `step` are not ordered, so the frame at a period's boundary can land in either
period. Compare the two as rates, not as one sample.

In a Tracy (or `profile`) build the steps show as zones: `phys::tick` per fixed step,
holding `phys::blocks` (the block list and the push and clip passes),
`phys::player_move` (the player's controller move and its landing, fall and push
follow-up), `phys::npc_plan` (query refresh and the walked actors' planned moves, which
run on the compute pool from 8 actors: the zone is the calling thread's wait) and
`phys::npc_apply` (their moves put into the world), and `phys::world_step`, which holds
`fnv_phys`' own `phys::grip_buoy` and `phys::rapier_step`; `phys::after` is the
once-a-frame work after the steps (camera interpolation, drawn sinks).
`phys::query_refresh` is the broad-phase refresh wherever it does work (inside a step or
before a query). Held by furniture or flying with ToggleCollision the steps still call
`PhysWorld::step`, so `phys::tick` holds the actors' zones, `phys::grip_buoy`,
`phys::rapier_step` and perhaps `phys::query_refresh`, but not the walking path's
`phys::blocks`, `phys::player_move`, `phys::world_step` and `phys::after` wrappers.

### Many cells, each option, and the stored settings (#1375)

- **Stored settings win over the command line** [documented: `settings.rs` module notes]:
  `NVForever.ini` `[Settings]` (the settings page) sets the resolution, the anti-aliasing,
  sun shadows and every enhanced key after the arguments are read, so `--size`, `--msaa`,
  `--no-sun-shadows` and `--graphics` are silently overruled there. [observed 2026-11-02]
  On the desktop the stored `resolution=1920x1080;desktop` and `anti-aliasing=0.50` (8x
  MSAA) held every run. Measure with `NVFOREVER_HOME` set to a home whose `NVForever.ini`
  is a copy without `[Settings]` (with `plugins.txt`); `--graphics-cycle` steps switch at
  run time and hold either way. Test runs (`--run-seconds`) default to `--graphics vanilla`.
  Since #1382 a switch given on the command line of a test run (`--run-seconds`,
  `--log-frame-times`, `--screenshot`, `--timelapse`, `--frame-json`) keeps its state over
  the stored one, and the log says so (`settings: anti-aliasing kept at 4x as --msaa on the
  command line set it; ...`); outside test runs the stored state is still applied, with a
  warning naming the switch it overrode (nvf-play `settings.rs` module notes). Switches
  not given still take the stored states, so a clean home stays the surest base.
- `tools/profile/tracy-tour.ps1` + `cells.csv`: one launch per cell of a Tracy build, a
  `tracy-capture` after the load, CPU and GPU (`tracy-csvexport -g`) zone CSVs per cell;
  `summarise.py <folder>` gives per cell the main/extract/render-thread split, the GPU passes
  and the heaviest spans. `-NvfHome`, `-Graphics`, `-Repeat`, `-Vsync`, `-NoCollector`.
- `tools/profile/option-costs.ps1`: per cell, `--graphics-cycle` through all on, all on but
  each enhanced key in turn, vanilla and no sun shadows; a key's cost is "all on" minus
  "all on but it". Periods are grouped by the set the log line names, not by position.
- With `NVF_PLAY_TEST_PROFILE`: `profile shadows:` (casters per cascade of each
  directional light, nearest first), `profile visibility:` (entities Bevy's visibility
  systems walk every frame, meshes or not) and `profile transforms:` (transforms written
  that frame, with a mesh, by name: about 950 of 7,200 at the Strip and Novac, nearly all
  the animated actors' skeleton nodes [observed 2026-11-02]).
- Test aids, off unless `=1`: `NVF_PLAY_TEST_NO_REFLECTION` (the water reflection camera
  stays off), `NVF_PLAY_TEST_NO_ARMS_SHADE` (no arms shade light, #1374),
  `NVF_PLAY_TEST_ACTOR_NO_SHADOWS` (actor meshes cast no shadows; still drawn).
- [observed 2026-11-02, i9-14900K, RTX 4070, 1920x1080, enhanced, vsync off] What they
  found: the sun's 4096² maps (#1367), moon shadows (#1369, fixed in #1371), full-size
  ambient occlusion (#1370), the arms shade light (#1374: 3.6 ms at Novac 07:00); 8x MSAA
  costs about 2.3 ms against none at Novac and Goodsprings; a low sun about doubles the
  sun's shadow cost (Novac 4.4 ms at noon, 7.9-8.3 ms at 07:00 and 19:00, with the shade
  light); the reflection camera at Hoover Dam 0.6 ms; the Tracy build adds about 0.85 ms
  where the CPU sets the frame (Hoover) and nothing where the GPU does (Novac).

## 2. Item 4: the mantis ootheca

**What happens** [observed: `fnvtool script decompile --form 000EF406`, `esm decode`]:
the ootheca `0x0010AC54` (`V22MantisOotheca`, a container, health 1) runs
`OnDestructionStageChange`: at destruction stages 1 and 2 it calls
`PlaceAtMe NVCrGiantMantisNymph 3 1 <dir>` for four directions, 24 nymphs in all (stage
1 "Cap Damage, Destroy" with 50 self-damage a second reaches stage 2 a moment later).
PlaceAtMe puts every reference at the caller's own position (the distance and direction
do nothing, catalogue notes; `fnv_game` `place_at_me`): all 24 at the egg's centre,
against the lockers.

**Cause** [observed: `NVF_PLAY_TEST_PROFILE`, schoolhouse, egg broken by
`NVF_PLAY_TEST_SCRIPT=10:call:0x0010AC54:DamageObject:5`]: frames of 0.8–1.3 s for as long
as the nymphs lived, 1 % low 1,162 ms (0.9 fps). Per frame: `player::step` 943 ms, of
which rapier's `move_shape` 853 ms (`detect_grounded_status_and_apply_friction` 618,
`cast_shape` 243, `handle_stairs` 112). Each nymph's character started on or in the egg,
hemmed in by the lockers; every move hit something at once, and the controller ran out
its 20 iterations, each with a ground test against every collider around, for every
nymph and every one of the up to 15 physics steps a slow frame catches up on. Placing
the same 24 nymphs with the same call at a placed nymph on open floor cost 30–50 ms
frames at first, falling to 12 ms as they spread; placing them on the unbroken egg cost
the same seconds-long frames as breaking it. So the place, not the number of actors or
the destruction, was the cause. FaceGen and parts (creatures have none), AI (`residents::
drive` 3–7 ms), detection (`detection::measure` 20–40 ms at 1 s frames, 3 ms after) and
drawing (29 actors drawn in 21 ms over 10 frames) were not.

A second cost showed once the nymphs could walk: their footsteps. 24 nymphs started
about 130 footstep sounds a second, each mixed through its own copy of the room's
reverb; the mixer fell behind real time (13 s mixed in a 25 s run), sounds stopped ending
and 1,800 sound entities piled up.

**What was changed**:
- `engine/nvf-play/src/created_spots.rs`: an actor a script creates is moved, before the
  engine first takes it up (`residents.rs` `spread_created`), to the nearest free spot on
  rings round where it was put (0 to 384 units): a walkable floor that is not the caller
  (the reference standing exactly where the actor was put), no more than 24 above it,
  room for a person's capsule (else for a 12 × 40 one), in sight of where it was put, and
  32 units from every other actor. Logged `… created at (…), stands at (…), <d> units away
  (the nearest free spot)`. [engineering choice: the game's own rule is unknown beyond "it
  lands near the caller"]
- `engine/nvf-play/src/sfx.rs`: at most 64 one-shot sound effects play at once and 8 of
  one SOUN; one asked for beyond that is not started (counted, and the most at once
  logged in the `sfx:` exit line). Interface sounds are not limited. [engineering choice]
- `engine/nvforever/src/ragdoll.rs`: a ragdoll still moving 3 s after it started (or was
  roused by a hit or a blast) is set at rest where it lies (`MAX_AWAKE`; logged `ragdoll:
  actor … held at rest still moving after 3 s`). [engineering choice]

A third cost came once the nymphs reached the player: the nymph's script kills it within
50 units of the player (`GetDistance player < 50` → `KillActor player`, stepping on it),
so they died one after another at the player's feet, 23 ragdolls of about 20 bodies each
in one heap that never calmed (the calm rule of ragdoll.md §4 never held for any of them).
From 20 s after the spawn the contact solver (`init_and_solve`) took 160–310 ms a frame,
frames 200–450 ms, until the run ended. With `MAX_AWAKE` at 5 s the worst 3 s window was
84 ms a frame (`init_and_solve` 40 ms) and frames were back to 4 ms once the last corpse
was held; at 3 s the smoke run (vsync on) stayed at 17–18 ms a frame through the die-off.

**After** (same run, profile build, 30 s): the 24 nymphs stood 32–384 units round the
egg, all on the floor; frames 8–11 ms through the fight (one 2 s window at 61 ms and one
at 253 ms right after the spawn in the first version, before the spots kept their
distance from the first batch); 1 % low 55 ms, of which the start-up's 513 ms frame is
most; the sound mixer kept up (29.2 s mixed in 30 s), at most 64 one-shots at once. The
smoke run (`mantis-egg-spawn`, 50 s, vsync on, the die-off included): every 2 s window
17–18 ms a frame, 1 % low 51 ms, all 23 corpses at rest at the exit.

## 3. Item 47: the REPCONN test site (RocketLabMid)

**What it holds** [observed: `fnvtool world refs --cell RocketLabMid`]: 2,095 references
(1,064 statics, 591 items, 88 lights, 105 containers), 17 room markers and 31 portals; in
the engine 4,221 mesh entities of 1,142 distinct meshes and 1,162 distinct materials
(about one material per mesh: nothing is drawn instanced across models).

**Where the time goes** (unseen, standing at the start; the best windows): frames
15–17 ms, of which the render thread about 15 ms: command encoding and submission
(`RenderContextState::apply` 3.4, `queue_submit` 2.1, the `Core3d` graph 4.9) and
preparing the draws (`write_binned_instance_buffers` 3.6, `prepare_preprocess_bind_groups`
1.9). The main thread took about 4.5 ms and the GPU 3–7 ms (`gpu time:` lines). So the
place is bound by the CPU work of drawing ~3,450 meshes one by one, not by lights,
shadows (none indoors), particles or scripts. With the ghouls awake (26 actors, 15 dead
ones as ragdolls) frames rose to 20–29 ms: `player::step` (their characters) 2–5 ms,
`residents::drive` up to 2 ms, and their skinned meshes on the render thread.

**What was changed**: `engine/nvforever/src/rooms.rs`, room and portal culling as the
original's renderer does it (docs/spec-notes/level-design.md §3.1): with the camera in a
room, only the rooms seen through its portals are drawn. Membership by bounds (a mesh
belongs to every room its bounds touch, rooms grown by 32 units), portals followed
through screen rectangles, the camera in no room draws everything; actors, dropped items,
lights and effects are never culled; culled meshes move to a render layer no camera
sees. `--no-room-culling` turns it off; `NVF_PLAY_TEST_ROOM_CULLING_CYCLE=<s>` switches it
off and on in one run for comparisons.

**After**: from the start 11 of 17 rooms and the unbounded space are in view and 1,185
meshes hidden (3,455 → 2,420 visible); from room 0008D44B looking west 4 rooms and 2,321
meshes hidden. Screenshots with and without culling at both places are the same
(1280 × 720: no pixel differs by more than 24 at the second, 5 pixels at the first, a
flickering light) [observed]. The frame time did not measurably change: with the culling
switched every 8 s in one run, frame means 17.9 against 18.1 ms and processor time 37.7
against 34.6 ms a frame, inside the machine's noise. So hiding a third of the meshes
does not remove most of the render thread's work here; what does scale is not settled
(see §4). Smoke: `rooms-repconn`.

**Occlusion planes** (#1434, `engine/nvforever/src/occlusion.rs`, level-design.md §3.2):
a placed object's mesh wholly behind one plane is hidden as well, and a portal wholly
behind one is not followed; interiors only by default (`--occlusion-planes-outdoors` for
the exterior ones, `--no-occlusion-planes` off; `NVF_PLAY_TEST_OCCLUSION_CYCLE=<s>`
switches them in one run). Hidden from the start [observed, 2026-11-06]: OVWestSewers02
1,549 of 3,600 meshes, 2EOVault21 1,127 of 4,923 (and 8 portals), RocketLabMid 943 of
4,134, HooverDamIntLowerLevel 127 of 4,005; outdoors at Primm 1,504 of 3,152. Pictures
with and without were the same in the sewers (no pixel off by more than 1), Vault 21
and REPCONN apart from HUD messages and flicker; at Primm a roof sign was hidden, hence
interiors only. Frame time, the planes switched every 10 s in one run, frame times
logged every 5 s: in the sewers 16.67 ms both ways (the run held at 60 Hz, so not
measurable), in an earlier unlocked run 5.0 against 5.0 ms; at Primm (outdoors switch)
5.9–6.4 ms on against 5.8–6.6 ms off. No measurable gain at these counts: as with rooms,
hiding a third to a half of the meshes is not what these frames are bound by.

## 4. Left to do

- **Batching** (REPCONN, and every interior): about one material asset per mesh, and
  `NifMaterial` was not bindless, so Bevy drew every mesh with its own bind group. Done
  2026-10-23: `NifSurface` is bindless (graphics-enhanced.md §5.6); at Goodsprings the main
  view's batch sets went from 316 to 23. REPCONN not measured again.
- **What the render thread's 15 ms scale with** in REPCONN is not settled: a third fewer
  visible meshes left it unchanged within the noise. A profile on an idle machine (no
  other builds or games) is the next step; xperf when elevation can be granted.
- **Physics steps on slow frames**: a frame of 0.25 s catches up 15 physics steps
  (`phys::play_params`), each moving every character; when something else makes frames
  slow, this makes them slower still. Left as designed (it keeps actors' walking in step
  with the game clock, detection.md §5).
- **A stuck character** still costs its controller's full 20 iterations every step;
  created actors no longer start stuck, but an actor pushed into geometry would. Not
  changed (the character controller is the physics workstream's).
- The voice limit's numbers (64, 8) are guesses; whether the original limits voices is
  not known. Since #1098 (2026-10-31) a sound at 8 of its own SOUN takes over that SOUN's
  oldest voice instead of not playing (an automatic weapon's burst had gaps); the voices
  stay within the same limits, and mantis-egg-spawn still mixed in real time (66.1 s
  mixed in 66.1 s, then 63.8 s in 63.8 s; 64 one-shots at most; 1 % low 42 and 77 ms
  with other games running, against 400 the scenario allows) [observed].
- **Heaps of corpses**: done 2026-10-30 (#910): corpses smaller than 64 units (rats,
  geckos, mantis nymphs, ...) pass through each other and still land on the world and on
  bigger corpses; bigger ones still collide (ragdoll.md, "Corpse against corpse"). 25
  small corpses dropped on one spot: 61 touching pairs at the peak and 0.38 ms a physics
  step before, 0 and 0.09 ms after (fnv-phys test, release). A heap of big corpses still
  costs the solver for its 3 s before rest.

## 5. The profiling campaign (#1421, 2026-11-03)

The owner asked for every kind of scene to be profiled: standing in places, combat, crowds,
interiors and the player moving. `tools/profile/campaign.py` runs the scenarios of
`tools/profile/campaign.toml` in one go:

- 12 exterior towns at several hours
- 7 crowd scenes
- 18 interiors
- stress battles of 10, 25 and 50 a side, plus 25 a side with creatures
- 7 player-movement runs: running, walking, third person, turning in place, walking interiors
- 3 streaming runs: a 2-minute run east across the desert and two free-camera fly-throughs

Each scenario is one launch of a `profile` build through tools/stress's launcher. That gives:

- the game locks
- a fresh home without stored settings
- god mode, unseen, script boxes closed
- crash and hang handling
- memory samples

Each launch runs at 1920 × 1080 with `--graphics enhanced` and vsync off, and logs the span
profile and stress samples every 5 s. The first 15 s after the first frame-time line are
left out. `NVF_PLAY_TEST_PROFILE_HITCH_MS` (new, `profile.rs`) lists every frame over the
scenario's `slow_ms` (16 ms; battles 40 or 100) with its 25 heaviest spans
(`NVF_PLAY_TEST_PROFILE_HITCH_SPANS=<n>` lists more). The report subtracts each
span's mean over every frame from its mean in the slow frames, which names what makes the
slow frames slow.

```
cd engine; cargo build --release -p nvforever --features nvforever/profile --target-dir F:/nvf-target/profile
python tools/profile/campaign.py run --exe F:/nvf-target/profile/release/nvforever.exe --data <Data> [--only …] [--group …]
python tools/profile/campaign.py report --reparse target/profile-campaign/<run>   # after a parser change
```

[observed 2026-11-03, main abd2c02f, i9-14900K, RTX 4070 (Vulkan), two full runs of 51
scenarios, about 1 h 45 min each, nothing else running (others' CPU share 1–2 %)]

| Scenario kind | Frame (ms) | Slowest frame (ms) |
|---|---|---|
| Interiors | 4.2-7.4 | 11-20 |
| Crowds (Strip, Freeside, McCarran) | 6.1-7.2 | 21-23 |
| Exterior towns | 6.9-11.5 (Novac 11.5 at 07:00, Jacobstown 10.2, Primm 9.3) | 19-28 |
| Player moving | 5.3-9.2 | 13-28 |
| Battles: 10 / 25 / 50 a side, 25 with creatures | 9.6 / 13.7 / 46.0 / 16.8 | 28 / 55 / 398 / 221 |
| Fly-throughs (free camera) | 4.4-5.2 | 6.5-8.8 |

What it found, each with its issue:

- **The 20 ms stutters in every exterior are first plays of sounds** (#1428). In 26 of 29
  exterior and crowd scenarios, `sfx::play` is what grows most in the slow frames, by
  9–15 ms: a new sound's file is read and decoded whole on the main thread.
- **The 50-a-side battle collapses** (#1426). It runs at 46 ms frames with 98 frames over
  100 ms. `detection::measure` is +81 ms in those frames: every pair within reach gets a
  ray, about 6,900 pairs for 118 actors. The fixed-step physics of the character
  controller adds +31 ms (#1383, #1404 are spooknik's).
- **Submitting the frame costs 1.5–3.5 ms of the render thread** almost everywhere
  (`submit_pending_command_buffers`, `queue_submit`; 23 command buffers a frame) (#1427;
  broken down below).
- **Single 8–16 ms stalls** (#1429):
  - a light probe upload on the Lucky 38 floor
  - an instance-buffer write when walking into REPCONN's rooms
  - an idle choice in Gomorrah
  - a late placement in the Sierra Madre lobby
- **Bugs found on the way:**
  - The Sierra Madre lobby stops on GPU validation errors: the hologram effect shader on
    skinned meshes (#1424).
  - One start-up crash in 53 launches: an access violation after 1 s with an empty log,
    not repeated (#1425).
- **Not new:** the towns that wait on the GPU (Novac, Jacobstown, Primm: 3–4 ms in
  `prepare_windows`) are mostly sun shadows. PRs #1418 (casters over the slice) and #1419
  (2048² maps) cut Novac by 2–2.4 ms each. Measure them again after those merge. Actor
  placement at start-up, 100–450 ms, is #1322.
- **Memory:** private bytes peak at 2.2–2.7 GB in interiors and fly-throughs and
  3.9–4.2 GB in towns and battles. No growth was seen within a run.

The scenario logs are game-derived and stay private, under `target/profile-campaign/` (not
committed).

### The ragdoll solver in the 50-a-side battle (#1798, 2026-11-07)

Measured on the bench machine (i9-14900K), `battle-50`, profile builds; logs private in
`C:/fnv-private/ragsolve-1798/`.

- **Where the time goes** [observed]. The stress sample now prints the solver's load
  (`bodies awake`, `contact pairs`, `manifolds`, `solver contacts`, `joints`) and, in the
  profile build, rapier's stage timers summed since the start (`solver ms`, feature
  `fnv-phys/solver-timers`, turned on by `nvforever/profile`). Over the battle rapier's
  steps took 14.1 s: 11.9 s in the solve (velocity resolution: the constraint iterations
  and writeback), 1.2 s in collision detection, 0.08 s in the serial set-up, 0 in CCD. At
  the peak (45–65 s) about 6–7 ragdolls are moving at once: 70–190 joints, 380–610
  touching pairs, 1,300–1,500 contact points; the solve takes 3–5 ms a step, at 2.4
  steps a frame.
- **Why the ragdolls are moving** [observed: the exit report]. 62 ragdolls came to rest
  141 times: 79 of the rests followed a shove that woke a corpse already at rest (248
  shoves by hits), and 92 of the 141 were the 3 s hold (`MAX_AWAKE`) on a corpse still
  moving. So about half the solver's ragdoll time is corpses woken again by stray shots.
  That is a look decision ([owner-review](../owner-review.md) item 51).
- **Lead 1, rapier's `parallel` feature: results unchanged, no gain in the game; not
  taken.** [observed] With `parallel` (and a 4-thread pool of its own), a pile of 42
  jointed figures, and 14 or 30 human skeletons (`_male\skeleton.nif`, 18 bodies, 17
  joints) on flat ground, gave the same state digest bit for bit as the build without
  it, on 1 to 32 threads; all fnv-phys tests passed. Off the game the step got 1.5–2.2×
  faster on 4–8 threads (14 skeletons: 1.64 → 0.80 ms a step). In the battle it did
  not: per frame at 25–75 s `init_and_solve` was 1.9–13.7 ms on main, 2.8–21 ms on
  1 thread and 1.8–21.7 ms in two runs on 4 threads, following how many ragdolls moved
  (frames 31–40, 31–53, 27–53 ms); per step in the slow frames 3.5–6.4 ms on 1 thread,
  2–8.9 ms on 4. [hypothesis] The same load costs about 10× more a step in the game than
  off it (one or two moving skeletons: about 0.8 ms of solve a step in the game, 0.07 ms
  off it, 0.16 ms with 200 moving kinematic actor bodies added), which points at the
  machine (the step's and the pool's threads on efficiency
  cores, or sharing cores with the render and task-pool threads) more than at the
  solver's work; not measured (no sampling profiler without administrator rights).
- **Lead 2, settling settings: not changed.** Solver iterations are global (every
  physics trace and the character controller's pushes would change), and halving them
  bought only 10–35 % off the game; joint count and damping did not stop the shiver
  before ([ragdoll §4](../spec-notes/ragdoll.md)). Shorter wakes and holds change the
  look, so they are the owner's.
- **Lead 3, the step off the frame's chain: a design, not built.** The rapier step
  (`PhysWorld::step` inside `player::step`) would start on a thread of its own once the
  frame's last fixed step has its inputs (the player's and NPCs' moves, `move_npcs`), and
  be joined by the first system that reads the world after it (ragdoll bone writes,
  detection rays, projectile casts, the next frame's character steps). For that:
  1. `player::step`'s loop hands the last step's world to a task (the `PhysWorld` moved
     into it and back), so nothing can touch the world before the join.
  2. Between the start and the join only work that does not read physics can run:
     animation, audio, UI, render extraction. Today detection, residents, AI and combat
     all cast rays right after `player::step` and would wait at once, so the gain needs
     them to read last step's world (a query copy: broad phase and colliders cloned
     after the previous step, as `PhysWorld::snapshot` does for the whole world) or to
     run after the join.
  3. Results stay bit-identical (the same steps in the same order); only when they are
     read changes. Worth building only if the ragdolls' share is still large after the
     owner rules on item 51.
- **Re-shot corpses move for 1 s (2026-11-08)** [observed, battle-50, profile builds,
  quiet CPU, 2 runs each]. A corpse at rest roused by a hit's shove is held again after
  1 s instead of 3 s (`MAX_AWAKE_SHOVED`, owner-review item 51). Seconds corpses spent
  moving: 258 and 360 on main (36 and about 70 re-wakes), 225 and 219 after (46 and 39).
  `init_and_solve` 4.40 and 6.24 → 3.58 and 3.38 ms a frame; frame mean 30.5 and 30.0 →
  28.6 and 27.6 ms; slowest frame 115 and 99 → 98 and 94 ms. Fresh corpses still almost
  all move the full 3 s: 53 of 58 were held still moving in one run, most with a neck or
  head turning at 0.7–3 rad/s, or calm for under 1.5 s when held.
- **The stuck actor's retries (#1797) and the hit-volume refresh: left as they are**
  [observed, the same runs]. One actor was held stuck. Its full retry move ran 10 times
  in 90 s, at 3–9 ms each. The 6–9 ms ones came after it asked for a new direction (0.3–1.1
  rad off, 110–464 units/s), so they cannot be skipped without changing where it goes.
  About one broad-phase refresh a frame is dirtied by hit volumes alone, asked for by a
  gameplay cast, at about 0.5 ms (`phys::query_refresh` 1.6 ms a frame over 2–3
  refreshes). Skipping it would leave the broad phase stale for the next detection, the
  way #1392 changed step results, so it stays.

### What the submission costs (#1427)

[documented: bevy_core_pipeline 0.20.1 `schedule.rs`, bevy_render 0.20.1
`renderer/render_context.rs`] In Bevy 0.20 a render system that records GPU work gets its
own command encoder for each view. `submit_pending_command_buffers` first finishes every
pending encoder (`finish_command_buffers`, spread over the compute threads, one
`finish_command_buffer{system=…}` span each) and then submits them all (`queue_submit`).
With wgpu 29, `CommandEncoder::finish` is where the recorded passes are encoded for
Vulkan. The span profile now keys each finished encoder by the system that recorded it
(`finish_command_buffer[light::per_view_shadow_pass<…>]`). The `profile batches:` line now
also counts the sorted blended draws (`transparent <v> views, <n> sorted draws`).

wgpu-core's own scopes (`Queue::submit`, `CommandEncoder::finish`, `cleanup`,
`triage_submissions`, `Queue::write_buffer`, `Device::create_bind_group`) are tracing
spans too. Bevy's default log filter (`wgpu=error`) hides them. To profile them, run with
`RUST_LOG=info,wgpu=warn,wgpu_core=info,wgpu_hal=warn,naga=warn`. Those spans make the
frames about 1.5 ms slower, so compare their shares, not frame times.

[observed 2026-11-03, this branch on main 8a83b44e, the `profile` build, i9-14900K, RTX 4070,
Vulkan, `campaign.py --only goodsprings-noon novac-noon`, means a frame after the warm-up]

| | Goodsprings noon | Novac noon |
|---|---|---|
| Frame | 8.9 | 12.2 |
| `submit_pending_command_buffers` | 2.36 | 2.34 |
| … finishing the encoders (wall time) | 1.14 | 1.09 |
| … `queue_submit` | 1.20 | 1.24 |

These are the encoders' finishing times, summed over the threads. They run side by side,
so the wall time is about the longest one.

| Encoder (system) | Goodsprings | Novac |
|---|---|---|
| sun and light shadows (`per_view_shadow_pass`, 4 shadow views) | 1.05 | 1.04 |
| blended draws (`main_transparent_pass_3d`, about 240 / 200 sorted draws) | 1.02 | 0.90 |
| depth prepass (`early_prepass`) | 0.70 | 0.68 |
| opaque pass | 0.69 | 0.68 |
| GPU culling and indirect parameters (4 compute encoders) | 1.59 | 1.46 or more (one below the list) |
| Bevy's bloom | 0.37 | below 0.25 |
| NVForever's passes (eye mask, bright pass, ambient occlusion, sun shafts, bloom and sharpen, grade) | below 0.25 each | below 0.25 each |

What this says:

- **The cost is Bevy's core passes and wgpu's work for each command buffer. The enhanced
  look's own passes do not add much.** Goodsprings vanilla against enhanced, run one after
  the other: submit 2.14 against 2.86, finishing 4.19 against 5.38 summed. That batch was
  slowed by something else on the machine (frames near 17 ms), so only its split is used.
  The opaque phases are already only 17-22 multi-drawn sets. The shadow encoder covers 4
  views.
- **None of NVForever's passes records when it has nothing to do.** Each returns before
  asking for an encoder (sun off screen, Bright Scale 0, no modifier), or Bevy skips it
  for a view without its component. Merging passes into fewer encoders would mean
  changing Bevy's encoder-per-system design: a patched `bevy_render`, not engine code.
- **Inside `Queue::submit`** (the run with wgpu-core spans, Goodsprings) the largest
  parts are cleaning up finished submissions, 0.8 ms (`cleanup`, `triage_submissions`,
  of which `drop command buffer trackers` is 0.4), and `prepare`, 0.4 ms. These scale
  with the command buffers and the resources each one tracks. The game materials'
  bindless slabs (`#[bindless(limit(512))]`, nif_material.rs) are among those resources.
  [hypothesis] Fewer resources tracked per pass would make these cheaper.
- **Seen on the way:** `Queue::write_buffer` takes 2.2 ms a frame and
  `Device::create_bind_group` 1.0 ms (Bevy's `prepare_preprocess_bind_groups` makes its
  bind groups again every frame). Both are on the render thread before the submission.

#### Moved off the render thread (#1427, done-when 2)

Two changes. Neither changes what is recorded or the order it is submitted in.

- **wgpu's clean-up of finished frames runs on a thread of its own**
  (`engine/nvforever/src/gpu_poll.rs`). [documented: wgpu-core 30.0.1 `Queue::submit`,
  `triage_submissions`] Every submit ends by polling the device, which drops the
  submissions the GPU has finished and every resource reference their command buffers
  held. Bevy polls nowhere else, so this ran inside every frame's `queue_submit`. A
  `gpu poll` thread now polls (without waiting) every millisecond, so a frame is cleaned up
  soon after the GPU finishes it, while the render thread records the next one.
- **Encoders are finished as soon as their system is done** (a patched `bevy_render`,
  engine/vendor/README.md), on the compute threads, instead of all at once at submission.

[observed 2026-11-07, main 644fee49, the `profile` build, i9-14900K, RTX 4070, Vulkan,
1920x1080, enhanced; `campaign.py --only goodsprings-noon novac-noon battle-50`; both off
and both on are the same exe (switched by a measuring-only environment switch, since
removed), runs interleaved; two runs each, ms a frame]

| | Goodsprings off | on | Novac off | on | battle-50 off | on |
|---|---|---|---|---|---|---|
| render thread | 5.53, 5.53 | 5.23, 5.01 | 6.04, 6.00 | 5.63, 5.69 | 10.37, 9.96 | 9.18 |
| `submit_pending_command_buffers` | 1.77, 1.80 | 1.44, 1.19 | 1.99, 2.01 | 1.47, 1.50 | 3.20, 3.13 | 2.29 |
| … `queue_submit` | 0.89, 0.89 | 0.66, 0.62 | 1.02, 1.03 | 0.74, 0.78 | 0.86, 1.09 | 0.07 |

One run each with only one of the two on: the poll thread alone took `queue_submit` to 0.69
(Goodsprings), 0.77 (Novac) and 0.04 ms (battle-50, where the GPU is done long before the
next submit); early finishing alone took the encoders' finishing wall time (the span less
`queue_submit`) from 0.88 to 0.58 ms at Goodsprings and from 2.3 to 1.8 ms in battle-50.
The second battle-50 run with both on is left out: its physics piled up (`phys::npc_plan`
and `move_shape` 250-370 ms a frame, main thread 169 ms), which is the battle, not the
renderer; its render thread was 11.3 ms and its submission 2.56 ms.

The frame time does not move in these three: Goodsprings and Novac are held by the main
app (6.3 and 7.3 ms against the render thread's 5-6 ms), battle-50 by physics. The render
thread gains 0.3-0.5 ms in towns and about 1 ms in the battle, room for scenes where it
is the slower thread.

Screenshots at Goodsprings at 12:00 (free camera, `--graphics enhanced`), both off against
both on, differ no more than two runs with both off do (mean absolute difference 1.45
against 1.48 of 765 a pixel: grass, clouds and water move between runs).

Still left in the submission:

- `queue_submit`'s `prepare` (about 0.4 ms in towns): each command buffer's barriers
  against the device's tracker, which scales with the command buffers (about 23) and
  the resources each tracks.
- The blended pass (`main_transparent_pass_3d`) is recorded last among the large passes,
  and its encoder takes 0.8 ms (towns) to 1.8 ms (battle-50) to finish, so the submission
  still waits for it. [hypothesis] Splitting its sorted draws over two or more encoders
  (each a render pass that loads what the one before stored) would let them finish side
  by side; that needs a patched `bevy_core_pipeline` and costs extra pass starts (and MSAA
  resolves) on the GPU.
- Finishing costs about 4-5 ms a frame summed over the compute threads; the opaque pass
  (about 33 multi-drawn sets) takes as long as the depth prepass, about 0.45 ms, so most
  of it is not per draw. [hypothesis] It is the bindless material slabs' resources being
  tracked for each pass (above).

### Particle systems no camera sees (#1417, 2026-11-08)

Every particle system with live particles rebuilt its whole mesh each frame (positions,
normals, UVs, colours, indices) and handed it to the renderer, seen or not. A counting aid
on a local branch measured, per frame, the systems whose mesh was written and how many of
them lay outside every active camera [observed 2026-11-08]:

| Place | Written | Outside every camera |
|---|---|---|
| Goodsprings, standing | 20 | 12 |
| Primm | 8 | 3.5 |
| Novac | 4 | 4 |
| The Strip | 7 | 7 |
| Running through Goodsprings | 25 | 15-18 |

None of them wrote geometry identical to the frame before (ages, spin and the camera's
turn change it), so leaving out identical writes would save nothing; leaving out the
unseen ones is the lever.

What is built now [design: `particles.rs` and `effect_particles.rs`, module notes]:

- Emission, simulation, random draws, lifetimes and clean-up are unchanged. Only the
  geometry build and the mesh write are left out for a system whose live particles' box
  (squares within half their size × √2, ribbons within their half width of every trail
  point, effect-shader squares the same) lies outside every active 3D camera drawing its
  layers: the main camera, the water reflection camera while it is on, a 3D scope's lens
  camera. The test uses each frustum's four sides only, so a box between a camera and its
  near plane, or under the reflection's water clip plane, counts as seen.
- Meanwhile its render entity is hidden, so its old mesh cannot draw; its emitter is not
  touched. In doubt (no box, no placement, no active camera, a frustum not set) it is built
  and shown.
- The build moved from `Update` to `PostUpdate` (`particles::ParticleDrawSet`): after the
  frusta are updated (and the reflection camera switched on or off), before visibility
  propagation and before the asset events. A system coming into view is therefore rebuilt
  and shown in the frame it is first drawn. The camera basis the squares face is still
  taken in `Update` where the draw took it, so a visible system's vertices, and their
  order, are bit for bit what they were. Hiding and showing a system takes its blended
  draw out of and back into the renderer's sorted list, so overlapping blended surfaces at
  exactly the same sort distance may swap order, as with any visibility change [design:
  Bevy sorts the blended phase by distance only].
- The world camera is found by its marker (`FlyCam`) instead of as the only 3D camera
  beside the reflection's. The draw used to stop, every system frozen as it was, while a
  scope's or a menu view's camera existed; particles now keep animating, and are culled
  and shown again, with a scope up (an intended change).
- No distance cut-off was added, and the fades and `NoFrustumCulling` stay.
- `NVF_PLAY_TEST_PROFILE` logs `profile particles: <n> frames; per frame: regular <p>
  published, <c> culled; effect …` (profile.rs). Smoke scenario
  `particles-culled-off-screen`: Gomorrah's roof fires behind the player, then turned to.

## 6. CPU placement on processors with unequal L3 domains (#1842, 2026-11-08)

Some processors split their L3 cache into domains of different sizes: AMD's dual-CCD X3D
parts have one CCD with the stacked cache (96 MiB) and one without (32 MiB). Windows
schedules the game's threads over both. On such a machine the game now runs on the
logical processors of the largest L3 and sizes Bevy's task pools to them
(`engine/nvforever/src/cpu_placement.rs`); every other machine is left exactly as before.

**Measurements.** Ryzen 9 7945HX3D (two CCDs; 96 MiB L3 on logical processors 0-15, 32 MiB
on 16-31), RTX 5070 Ti, profile build of main 01df32361, 1920x1080, enhanced look, vsync
off, standing 60 s, mean frame time [observed 2026-11-08, Ryzen 9 7945HX3D, the process
restricted by a hard affinity mask]:

| Scene | Whole CPU | 96 MiB domain only |
|---|---|---|
| Goodsprings, noon | 17.50 ms | 12.57 ms |
| Primm, noon | 17.23 ms | 12.48 ms |
| The Strip, night | 17.00 ms | 12.18 ms |

Every main- and render-thread span was 30-70 % slower on the whole CPU, as code waiting
on memory would be [observed, same runs; the cause is a hypothesis]. Earlier on the same
machine (#1416): 15.37 → 12.09 ms standing at Goodsprings on the 96 MiB domain, and 15.25 ms
on the 32 MiB domain alone [observed, #1416], so it is the large cache that helps, not
running on one CCD. Bevy kept sizing its pools for 32 processors when restricted (I/O 4,
async 4, compute 24: 24 compute threads for 16 processors); a budget of 16 threads (I/O 4,
async 4, compute 8) screened slightly faster still [observed, #1416, single screens].
Marking the program as a game in the Xbox Game Bar gives the same on this machine through
AMD's driver (#1416's closing comment), but only with the chipset driver, Game Mode, the
Balanced power plan and the player ticking that box [hypothesis: AMD's and Microsoft's
public material, not checked].

**Rules** (`cpu_placement::decide`, unit tests on synthetic topologies). Windows only; read
with `GetLogicalProcessorInformationEx` (level-3 unified caches). Nothing is done (no
affinity or CPU-set call, Bevy's pools exactly as before; only the log line is new)
unless all hold:

1. one processor group (64 logical processors or fewer);
2. two or more L3 domains, one strictly larger than every other (two equal 32 MiB L3s, a
   single-CCD part such as the 7800X3D or 9800X3D, Intel's single L3: nothing);
3. that domain has at least 8 logical processors (a 7900X3D's cache CCD has 12 and
   qualifies; 6 with SMT off does not, as the game's own threads and the I/O and async
   pools would crowd the main and render threads [hypothesis]);
4. that domain has at least as many logical processors as every other domain, so the game
   never trades threads for cache: every dual-CCD X3D part has two CCDs of one core count
   and qualifies, while a Ryzen AI 300 "Strix Point" laptop part (4 full cores sharing
   16 MiB, 8 compact cores sharing 8 MiB [hypothesis: from AMD's public specifications,
   not checked here]) is left alone, as moving it would give up 16 of its 24 threads;
5. the process is not restricted already: its affinity mask is the system's and it has no
   default CPU sets (`start /affinity`, Process Lasso or the player chose: left alone).

Then the process's default CPU sets (and the running main thread's) become the domain's
processors and Bevy's `TaskPoolOptions::max_total_threads` the domain's processor count.
It runs in `main()` right after the command line is read, before any thread starts.
**CPU sets rather than the affinity mask**: `SetProcessDefaultCpuSets` is the mechanism
Microsoft documents for games, it is not inherited by the processes the game starts (the
load hand-off's relaunch decides again), and it leaves the affinity mask as the system's,
so rule 4 still means someone else chose. The measurements above used a hard affinity
mask; that CPU sets give the same frame times is to be measured [hypothesis].

**Which CCD on a 7950X3D / 9950X3D.** There the cache CCD clocks lower than the other one
(a few hundred MHz). The largest L3 is still chosen: on this machine the cache domain beat
the whole CPU by about 28 % and the small domain alone gained nothing, more than a clock
lead of under 10 % could give back [hypothesis for those parts; needs one of them].
`off` is there for a player who measures otherwise.

**Setting.** `cpu-cache-placement=auto|off` in `[Settings]` of NVForever.ini (default
`auto`, read at start only); `--cpu-cache-placement auto|off` wins over it for one run,
for A/B runs. The start logs one line, e.g. `cpu placement: L3 domains 96 MiB (16 LPs),
32 MiB (16 LPs); restricted to the 96 MiB domain (16 LPs), task pools 16`, or `cpu
placement: one L3 domain, nothing to do`, `... all the same size, nothing to do`, `...
affinity already set, left alone`, `cpu placement: off by setting (...)`; smoke scenarios
`cpu-placement` and `cpu-placement-off` match it.

**Rayon (#3148).** Under a restriction, rayon's global pool (the load-time data building:
forms, references, script compile) is built with the task pools' budget right after the
placement, and a second line says so (`cpu placement: rayon's pool 16 threads, as the task
pools`). Without it rayon sized itself from `available_parallelism`, which counts every
logical processor under CPU sets: 32 workers on a 16-LP domain. A `RAYON_NUM_THREADS` the
player set is left alone. [hypothesis: the load-time gain is not measured; no dual-CCD
machine was at hand.]

**Left to do.** Linux (`/sys/devices/system/cpu/cpu*/cache/index3`, `sched_setaffinity`,
same rules). Putting the background threads (generated-terrain bake, read pool) on the
other domain with `SetThreadSelectedCpuSets`, leaving the cache domain to the main,
render and compute threads [hypothesis]. A row on the settings page.

## 7. Indirect-parameter build jobs in Bevy's GPU culling (#1887, 2026-11-08)

Measured on a Ryzen 7 7700X with an RTX 3070 (Vulkan) at 2560 × 1440, the owner's
settings (4x MSAA, ambient occlusion, longer shadows, bloom, light glow, sun shafts, sun
shadows), vsync off, `--no-actors` unless said. The owner's outdoor frame rate:
Goodsprings at noon runs at about 90 fps where interiors reach the 180 fps vsync cap.

[observed] Where the frame goes before these changes:

- **The frame waits on the GPU.** The swap chain's acquire waits about 4.5 ms on the frame
  three back, so the GPU is the limit. Frame latency 1–3 and mailbox or immediate
  presentation change nothing.
- **About 3 ms of GPU time falls outside the pass timers** (`--log-frame-times`: 10.2 ms a
  frame, 6.8 ms timed). It grows with MSAA.
- **The CPU floor** at 640 × 360 is about 5 ms a frame with no actors. The campaign's
  main thread is about 7 ms with Goodsprings' residents.

**Bevy's indirect-parameter build jobs are merged** (engine/vendor/README.md,
bevy_render). An Nsight Graphics GPU Trace (5 frames) showed what the timers missed. The
graphics engine was busy 93 % of the frame, but the shader units were only 28 % busy. The
front end waited for the GPU to go idle 37 % of the time, and the frame had 2,518 compute
dispatches. Bevy's GPU culling builds the indirect draw parameters with one dispatch per
batch range of each view and phase. That pass has no timer, and wgpu puts a barrier
between dispatches that write the same buffers. Merging each view's ranges that follow one
another leaves 904 dispatches. Compute is then active 9 % of the frame instead of 16 %.
[observed] Interleaved runs of one build with and without it (terrain layers of #1886
applied in both): 9.0 → 8.5 ms a frame.

[observed 2026-11-08, `tools/profile/campaign.py`, `profile` builds of main at 4eede5348 and
of this change on it, Ryzen 7 7700X, RTX 3070, Vulkan, enhanced look, with the scenes'
residents] Frame time a frame, main → with the change:

| scenes | main | with it |
|---|---|---|
| 1920 × 1080, the 12 `stand` scenes, mean | 8.47 ms | 7.82 ms (−7.7 %) |
| 1920 × 1080, Jacobstown (slowest) | 11.02 ms | 9.93 ms |
| 1920 × 1080, Novac at noon | 10.63 ms | 9.41 ms |
| 1920 × 1080, Hoover Dam (the only one not faster) | 7.15 ms | 7.30 ms |
| 2560 × 1440, Goodsprings at noon, two runs | 10.75, 10.62 ms | 10.11, 9.80 ms |
| 2560 × 1440, Goodsprings at 19:00, two runs | 10.50, 10.57 ms | 9.96, 9.85 ms |
| 2560 × 1440, Novac at noon, two runs | 13.37, 13.62 ms | 12.32, 12.32 ms |

The pass timers' total barely moves (Goodsprings at noon, 1080p: 5.68 → 5.87 ms): the
dispatches saved are in the culling pass, which has no timer. Four of the 1080p scenes on
main first ran capped at 16.67 ms a frame (the window held to 60 Hz); they were run again
for both builds and those runs are the ones counted.

Not done, measured on the way [observed]:

- **Light glow and sun shadows.** Light glow costs about 0.9 ms of the frame and sun
  shadows about 1.1 ms, each about half outside their timers.
- **Uniform allocation and bin unpacking** (Bevy) still make one dispatch per batch set:
  about 90 each a frame, 0.3–0.5 ms each pass.
- **The opaque pass** (2.6–4.2 ms) is the largest timed pass.
- **DX12 does not run** (the bindless NIF material's bind group layout fails validation),
  so it could not be compared.

## 8. Terrain texture layers drawn only where they paint (#1886, 2026-11-08)

Measured on the same machine and settings as §7.

In `terrain.rs` (`quadrant_mesh`), each texture layer of a quadrant was a blended copy of
the whole 17 × 17 grid, drawn at full size even where its opacity is 0. A layer mesh now keeps only the triangles with a corner
the layer paints, and a layer that paints nothing has no mesh. The picture is the same:
screenshots differ from main by less than two runs of main differ from each other.
[observed] The blended pass took 1.2 → 0.74 ms looking at the town and 1.7 → 0.48 ms
looking at the ground; the frame took 10.2 → 9.7 ms and 7.6 → 6.4 ms.

[observed 2026-11-08, `tools/profile/campaign.py`, `profile` builds of main at 4eede5348 and
of this change on it, Ryzen 7 7700X, RTX 3070, Vulkan, enhanced look, with the scenes'
residents] Frame time a frame, main → with the change:

| scenes | main | with it |
|---|---|---|
| 1920 × 1080, the 12 `stand` scenes, mean | 8.47 ms | 7.56 ms (−10.7 %) |
| 1920 × 1080, Novac in the morning | 10.94 ms | 8.90 ms |
| 1920 × 1080, Jacobstown (slowest) | 11.02 ms | 9.57 ms |
| 1920 × 1080, Hoover Dam (the only one not faster) | 7.15 ms | 7.35 ms |
| 2560 × 1440, Goodsprings at noon, two runs | 10.98, 10.79 ms | 9.68, 9.62 ms |
| 2560 × 1440, Goodsprings at 19:00, two runs | 10.89, 10.87 ms | 9.71, 9.63 ms |
| 2560 × 1440, Novac at noon, two runs | 13.80, 13.83 ms | 11.43, 11.25 ms |

Frames of 16 ms or more in Novac at 1440p: 13 and 16 → 0. The 1080p main runs are those made
for the merged build jobs (#1887, the same main build); the 1440p ones were run interleaved
with this change. Screenshots of Goodsprings (the town, the ground below the player) and Novac
against main: bare ground differs by at most 1 of 255 in any channel; the sky and grass
differ as much as two runs of main do (clouds drift and grass sways with time since start).

## 9. 2560 × 1440 with 4x multisampling: what the GPU does in a frame (2026-11-10)

Asked for: 180 frames a second at 2560 × 1440 on a contributor's machine with their saved
settings. Reached: Goodsprings at noon 106 → 130, Novac at noon 89 → 111, the Strip
156 → 176; what is left, and why 180 outdoors is not reached, is at the end.

Every number here is [observed 2026-11-10, Ryzen 7 7700X, RTX 3070 (Vulkan), 2560 × 1440
borderless, 4x multisampling, ambient occlusion, longer shadows, bloom, light glow, sun
shafts, sun shadows in 3 cascades, grass and distant LOD on, vsync off, standing still,
release builds unless a `profile` build is named, main at f4015fae8]. Runs of 30-45 s, the
first 15 s left out, the two builds interleaved; a scene's 5-second windows differ by
about 0.1 ms.

### Measuring

- **Which side holds the frame.** The same scene at 1280 × 720: 5.9 ms against 9.4 ms at
  1440p, so at 1440p the frame waits for the GPU (the render thread sits 4 ms in
  `window::prepare_windows`, waiting for the next image), and the main thread is the next
  limit, at 5.7-5.9 ms.
- **GPU time per command encoder** (`NVF_PLAY_TEST_PROFILE_ENCODERS=1` with
  `NVF_PLAY_TEST_PROFILE`, a `profile` build; engine/vendor/README.md, bevy_render). Bevy's
  render diagnostics time each pass from inside it, which leaves out what a system records
  around its passes. With a span around each system's encoder the `profile gpu:` line lists
  `enc:<system>` beside the passes, and up to 80 entries. At Goodsprings the encoders summed
  to 8.2 ms of a 9.2 ms frame:

  | Encoder | ms | of which outside its pass |
  |---|---|---|
  | opaque pass | 3.28 | 0.20 (the resolve) |
  | depth prepass | 1.08 | 0.23 (the copy of the multisampled depth) |
  | transparent pass | 0.94 | 0.16 (the resolve) |
  | ambient occlusion | 0.63 | |
  | culling and indirect parameters (5 compute encoders) | 0.79 | |
  | sun and arms shadow maps | 0.30 | |
  | transmissive pass | 0.26 | 0.26 (the copy of the frame and a resolve) |
  | bloom, sharpen, grade, auto exposure, tonemapping, upscaling, eye mask, UI | 0.95 | |

- **Features off, one at a time** (frame, release): multisampling off 9.4 → 7.6 ms, sun
  shadows off 9.4 → 7.9, both 6.0; grass off 8.5 and distant LOD off 8.7 (`profile` build,
  9.2 with both).
- **A RenderDoc capture** of the same view (replayed with per-event GPU durations): the main
  view draws 1.93 million triangles in the prepass and again in the opaque pass, and the
  third shadow cascade 1.24 million.
- **SpeedTree trees.** 57 in view. Without them the frame is 8.0 ms instead of 9.3; without
  their leaf cards alone 8.05. The leaves were 0.68 ms of the opaque pass and 0.13 ms of the
  prepass: thousands of overlapping alpha-tested cards a tree.

### What was changed

1. **The main pass tests depth for equality with the prepass and writes none**
   (`nvf_assets::depth_equal`): opaque NIF surfaces and tree bark, and alpha-tested shapes
   without a soft edge. Goodsprings 9.32 → 9.03 ms. Screenshots differ from main as two
   runs of main do.
2. **Cut-outs' soft edges are written by the depth prepass** (`nvf_assets::soft_edges`,
   coverage.wesl; nif_material.rs "Prepass coverage"). With multisampling an alpha-tested
   shape covered, where its alpha falls below the cutoff, a share of the pixel's samples,
   through alpha to coverage in the main pass and in front of the depth the prepass (which
   tested at the cutoff) had written. So the shader ran for the transparent texels and soft
   edges of every card nearer than the visible one, and no depth test could reject them
   early. Now the prepass fragment returns the share as a sample mask, and the main pass of
   NIF surfaces, grass, leaf cards and tree billboards tests for equality with it, without
   an alpha test or alpha to coverage. Kept as they were: shadow passes, first-person and
   refracting shapes, views whose prepass also writes normals or motion vectors (TAA, FSR),
   views without multisampling.
   - What differs in the picture: which samples a partly covered pixel takes (this module's
     order, rotated from pixel to pixel, instead of the GPU's pattern), and the depth
     prepass that ambient occlusion and soft intersections read now holds the soft edge.
     Screenshots at Goodsprings, Novac, Freeside and the Lucky 38 floor differ from main in
     0.03-1.2 % of their pixels by more than 24 of 255 (two runs of main: 0.06 %): swaying
     grass, actors that moved, and single pixels along cut-out edges.
   - A first try without the sample mask (depth equality with the old prepass) lost the
     soft edges: grass thinned out and fence wires broke up. That is why the prepass writes
     them.
   - On Vulkan it needs the vendored naga's fix (engine/vendor/README.md, "naga"; #2413):
     upstream naga declares the sample mask output as a scalar, which Vulkan forbids, and
     Intel's shader compiler crashed on it.
3. **One multisample resolve instead of three** (engine/vendor/README.md, "one resolve").
   The opaque, transmissive and transparent passes each resolved the 4x HDR target, and the
   transmissive pass copied the resolved frame. Now the samples are resolved once into the
   transmission texture and once at the end of the transparent pass. Goodsprings
   7.85 → 7.64 ms (three interleaved pairs, the `fast` test profile).
4. **`Update` runs on Bevy's single-threaded executor** (main.rs; test aid
   `NVF_PLAY_TEST_UPDATE_EXECUTOR=multi` for the default). With frames held by the CPU:
   Goodsprings at 720p 5.67 → 5.36 ms, the Strip 5.93 → 5.68, a 25-a-side battle
   10.72 → 10.62 (two runs each), the Prospector Saloon unchanged. `PreUpdate`,
   `PostUpdate`, `First` and `Last` measured no different and keep the default.

Tried and dropped: making Bevy's copy of the frame only when an additive shape needs it
(7.78 against 7.75 ms without the transmission anchor: nothing); the arms' shade light
off (7.56 against 7.49 ms).

### Result

Release builds of main (f4015fae8) and of the four changes together, two interleaved
runs of each scene, frame time in ms and the frames a second of their mean:

| Scene | main | changed | |
|---|---|---|---|
| Goodsprings 12:00 | 9.43, 9.39 (106) | 7.69, 7.62 (131) | −19 % |
| Novac 12:00 | 11.20, 11.15 (89) | 9.04, 8.95 (111) | −20 % |
| Primm 12:00 | 11.72, 11.72 (85) | 10.14, 10.06 (99) | −14 % |
| The Strip by Gomorrah 20:00 | 6.52, 6.28 (156) | 5.87, 5.46 (176) | −11 % |
| Freeside 14:00 | 9.05, 8.75 (112) | 7.64, 7.25 (134) | −16 % |
| Prospector Saloon | 4.61, 4.60 (217) | 4.21, 4.28 (236) | −8 % |
| Lucky 38 casino floor | 6.46, 6.39 (156) | 5.65, 5.61 (178) | −12 % |

Only the saloon is past 180 a second; the Strip and the Lucky 38 floor are within 2-4 of it.

### After the table: anti-aliasing other than 4x, and the terrain's layers

5. **Cut-outs test depth for equality without multisampling too.** With FXAA, SMAA, TAA or
   no anti-aliasing, grass, leaf cards and tree billboards had kept the old main pass
   (change 2 covers multisampled views; only NIF surfaces took change 1). Both passes test
   the alpha at the cutoff there, so they now test for equality as well.
6. **A quadrant's first four texture layers are drawn with its base** (terrain.rs, "One
   draw for the base and its first layers"; terrain.wesl), where the GPU has bindless
   textures. A ground triangle at Goodsprings carries 2.2 painted layers on average (50
   cells: 102,400 base triangles, 224,778 painted layer triangles), each a blended copy lit
   in full. Up to 6 layers a quadrant were seen; the first four hold 85 % of the painted
   triangles at Goodsprings and 95 % at Novac, and the rest keep their blended draws.
   Goodsprings, the same exe with `NVF_PLAY_TEST_TERRAIN_LAYERS=blended` and without:
   FXAA 6.39, 6.32 → 6.17, 6.13 ms; 4x multisampling 7.72 → 7.52 ms. A screenshot folded
   against blended differs in 0.07 % of its pixels by more than 24 of 255 and in 0.6 % by
   more than 8. Less than the 0.56 ms the layers cost: the folded shader samples every
   layer's two textures over the whole quadrant.

Goodsprings by anti-aliasing choice (release, changes 1-5; frames a second, before change 5
in brackets where it differs): 4x multisampling 131-134, 2x 149 (151), none 161 (144),
FXAA 158 (142), SMAA 154 (142), TAA 155 (141).

With FXAA and changes 1-6 (release, one run a scene): Goodsprings 163, Novac 130, Primm 125,
the Strip 190, Freeside 171, the Prospector Saloon 276, the Lucky 38 floor 204.

Looked at and left: a layer hidden under a later, fully opaque one (0.3 % of the layer
triangles: opacities seldom reach 1); the swapchain's frame latency (1 to 4: 7.62-7.70 ms,
the GPU 97-98 % busy at each); the depth-only passes reading single fields of the material
instead of whole structs (Primm's prepass 1.74 against 1.77 ms: the driver already did).

### Sun shadows, and Goodsprings at 180 with SMAA

What the sun's shadows cost at Goodsprings with SMAA (release, 6.3 ms a frame): off
5.06 ms. No single part of them explains it:

| Tried, one at a time | Frame (ms) |
|---|---|
| as they are (3 cascades, Gaussian filter, reach 8,000 x 1.5) | 6.23-6.40 |
| hardware 2x2 filter | 6.20 |
| reach x 1 instead of x 1.5 | 6.30 |
| shadow distance halved | 6.16 |
| `--shadow-thin` 0.02, 0.05, 0.1 | 6.28, 6.28, 6.12 |
| the maps drawn but never sampled (a shader edit for the measurement) | 6.08-6.21 |
| no shade light for the first-person arms | 6.21 |
| 2 cascades; 1 cascade | 6.24; 6.09 |
| trees cast no shadow (leaves only cast: 6.39; bark only: 6.29) | 6.21 |
| off | 5.06-5.08 |

So the cost is not the filter, the reach or the map size, and no one view or kind of
caster: sampling is about 0.2 ms, each view about 0.13, the trees 0.19, and the rest goes
only with the last view.

7. **Sun shadows' Performance mode** (nvforever shadow_rate.rs; the Sun shadows row's
   middle choice of Off / Performance / Quality, `--sun-shadows=performance`; measured
   while it was a separate switch, "Lighter sun shadows", #2182). In it: the sun's shadow
   maps are drawn every other frame and kept in between, with the cascades of the frame
   that drew them (a patched bevy_pbr, engine/vendor/README.md "held shadow maps"); 2
   cascades instead of 3; trees cast within 3,000 units of the camera only; no shade light
   for the arms.
   Goodsprings with SMAA: held maps alone 6.40 → 6.03 ms (two pairs); the whole mode
   6.41 → 5.67 ms (two pairs). What it costs in the picture: shadows a little coarser at
   mid distance, the shadow of a moving thing at half the frame rate, no tree shadows
   beyond about 43 m, and on a fast turn the edge of the view can lie outside a held
   cascade for a frame. A screenshot on Performance against Quality differs in 0.6 % of
   its pixels by more than 24 of 255 (shadow edges).
8. **Every layer of a quadrant is folded into its base** where the texture repeat divides
   the cell (change 6 folded four): the texture coordinate is the fragment's place in its
   mesh over the repeat, which terrain.wesl works out, and its vertex slot carries the
   fifth and sixth opacities. The two layers left blended had cost 0.18 ms; a screenshot
   folded against blended differs in 0.25 % of its pixels by more than 24 of 255.

Goodsprings at 12:00 with SMAA, Performance and changes 1-8 (release): 5.51, 5.55 ms
(181.6, 180.2 a second) in two runs, and 5.49-5.56 ms in the undisturbed 5-second windows
of five more made while the machine was in use (two of those runs had a 0.9 s stall at
their start and one was held at 60 a second for part of it; they are left out). On
Quality: 6.23 ms (160). That is at the 5.56 ms of 180 a second with nothing to spare.
The smoke scenario `sun-shadows-performance` changes the mode in play; it passed on
#2182's branch, as did the rest of that branch's smoke run (122 scenarios).

### What is left, and why Goodsprings is not at 180

At 7.7 ms Goodsprings still waits for the GPU; 180 a second is 5.56 ms. Per feature, on
the changed build: sun shadows off 6.3 ms, multisampling off 7.0 ms, terrain texture
layers off 7.2 ms. So no single piece that keeps the settings is worth the 2.1 ms
missing; the pieces, largest first:

- **Sun shadows, 1.3 ms**: 0.3 the three cascade passes (1.4 million triangles), about
  0.3 culling and indirect parameters for the four shadow views, about 0.5 sampling them in
  the lit passes (Gaussian filter, 9 taps a fragment), the rest uploads.
- **Culling compute passes, 0.7 ms**: `uniform_allocation` and `bin_unpacking` dispatch
  once per batch set of each phase of each view (94 dispatches a pass), each reading what
  the one before wrote. Bevy's design; merging them is a patch to its GPU preprocessing.
- **Terrain texture layers, 0.56 ms**: each layer is a blended copy of the ground, lit in
  full (terrain.rs, module notes). The single-pass splatting shader its notes plan would
  light the ground once.
- **Ambient occlusion, 0.6 ms**, 0.3 of it reading the multisampled depth; a depth resolved
  once to one sample would serve it, soft intersections and the water, and replace the
  0.23 ms copy of the multisampled depth. That changes Bevy's prepass textures.
  2026-11-19 (graphics-enhanced.md §3.1.2): the occlusion's passes now read a depth it resolves
  once itself (sample 0, bit for bit; `NVF_AO_RESOLVE_DEPTH=0` for the old way), not yet
  measured. The copy stays: it is in `bevy_core_pipeline` (not vendored), and the water's edge
  test reads every sample of it.
- **The main thread** is at 5.4 ms outdoors once the GPU is out of the way.
- **About 1 ms of GPU time a frame is in no encoder** (a `profile` run held by the GPU, 8x
  multisampling: the frame 9.81 ms, the encoders 8.80 ms; the same gap at 4x and with
  FXAA). The frame's 340 buffer uploads are 0.8 MB in all (a RenderDoc replay); batching
  them was tried before (#1955) for about 3 % and parked. The rest is not found.
- **Alpha-tested overdraw in the depth prepass**: Primm's view, a quarter of it a dry
  bush, spends 1.74 ms in the prepass against Goodsprings' 0.56 ms.
- **Shadow passes at Novac and Primm**, 0.7 ms (about 1,500 casters in the third cascade)
  against Goodsprings' 0.25 ms.

## 10. The four follow-ups of §9: uploads, culling dispatches, leaf cards, shadows (#2063-#2066, 2026-11-10)

[observed 2026-11-10 throughout: Ryzen 7 7700X, RTX 3070 (Vulkan, driver 621.01), 2560 ×
1440 borderless, vsync off, standing still, release builds unless a `profile` build is
named; runs of 30 s with the first 15 s left out, two runs per variant.] Settings: the
owner's (SMAA, Sun shadows on Performance) unless said otherwise.

### Where the GPU time outside the encoders went (#2064)

The encoder-span aid now also reports `zz:wall` (the frame's first encoder to its last),
`zz:gaps` (the time inside that no encoder covers) and `zz:between_frames` (the last
encoder of one frame to the first of the next) [vendored bevy_render, `NVForever encoder
gaps`]. Inside a frame the encoders leave 0.04-0.06 ms uncovered; the missing time was
between frames: 0.60-0.65 ms when the GPU holds the frame (Primm, 8x multisampling,
`profile` build), whether windowed or borderless and at 2560 × 1400 as at full screen.

- Not the swapchain: wgpu makes the first submission that uses a swapchain image wait at
  the top of the pipe for the presentation engine [documented: wgpu-hal 30
  `vulkan/mod.rs` `Queue::submit`], and the frame is one submission; submitting the scene
  before the upscaling pass (a `FlushCommands` system before `upscaling`) changed nothing
  (11.45 against 11.47 ms, 11.40 against 11.13 ms) and was dropped.
- An Nsight Graphics GPU Trace (5 frames, Goodsprings, clocks locked to base) put the
  graphics front end in wait-for-idle for 40 % of the frame, with 261 go-idle commands a
  frame: barriers.
- `RenderQueue::write_buffer` calls counted by caller (a measurement build): 379 a frame
  at Goodsprings, 297 of them the three buffers of each of the 99 multi-drawn batch sets
  (`gpu_preprocessing.rs`, binned mesh instances, bin metadata, bin index map), written
  again every frame whether they changed or not. Each is a copy with its own barrier in
  the frame's submission, before its first pass.

Change: `RawBufferVec::write_buffer_if_changed` (vendored bevy_render, `NVForever
unchanged uploads`) skips a write whose bytes match the last write into the same buffer,
used for those three, which the compute shaders only read. Writes 395 → 73 a frame, GPU
time between frames 0.60 → 0.12 ms, Primm at 8x 11.52 → 10.83 ms (`profile` build). With
the owner's settings: Goodsprings 5.45 → 5.20 ms, Novac 6.16 → 5.70, Primm 6.81 → 6.45.
Test aid `NVF_PLAY_TEST_SAME_WRITES=1` writes every frame.

### Culling dispatches per batch set (#2065): measured, not changed

`uniform_allocation` and `bin_unpacking` cost 0.36 ms a frame at Goodsprings and 0.5 ms at
Novac after the change above (0.54 and 0.73 with every cascade drawn). Each batch set is a
dispatch in each, so fewer batch sets would be the game's lever. A dump of the batch sets'
pipelines (a measurement build) shows what splits them: vertex layouts (with or without
colours, tangents, a second UV set, skinning), cull mode, depth bias and the material
slab (4-5 per phase: `NifSurface`'s bindless slabs hold 512 resources, chosen in
streaming-hitches.md §3.1 against night stutter). Pipelines with identical descriptors
are 2-3 a phase; merging them would leave 98 of 102 batch sets at Goodsprings and 125 of
132 at Novac. What remains is Bevy's design, one dispatch (and a barrier) per batch set;
merging the dispatches of a phase means one set of buffers per phase instead of per batch
set, a rework of its GPU preprocessing, left for upstream. [2026-11-24] Done after all:
§13, "Merged scene unpacking".

### Leaf cards in the depth prepass (#2066)

Pipeline statistics (a measurement build): Primm's prepass runs 166 million fragment
invocations a frame (Goodsprings 14 million; the main pass, depth-equal, 3.7 million at
both). Without grass: the same. Without SpeedTree leaf cards: 6 million and 0.37 ms. The
shrub at the start of Primm is a SpeedTree shrub whose camera-facing cards stack about 45
deep. An alpha-tested fragment shader may discard, so the GPU tests its depth after
running it; only the coarse per-tile test spares a card behind others, and only if the
nearer ones were drawn first [hypothesis: the order experiment below bears it out].

- Cards cut to their cell's visible part (alpha above 8, two texels wider; tree.rs,
  `AlphaMap::kept_part`): 89 % of the card area left, 166 → 149 million, 1.76 → 1.59 ms.
- Draw order, cut cards: outermost cards first 86 million (1.13 ms), innermost first 209
  million; nearest the camera first (8 orders per leaf mesh, one per heading around the
  trunk, chosen each frame by `order_leaves`) 67 million, 0.91 ms. Goodsprings and Novac
  unchanged (no dense tree near the camera).
- Screenshots against whole cards in one order: Primm 1.9 % of pixels over 24/255 (two
  runs of the old 2.9 %: the leaves move in the wind), Goodsprings 0.05 % (0.14 %).

Test aids `NVF_PLAY_TEST_LEAF_CARDS=whole`, `NVF_PLAY_TEST_LEAF_ORDER=authored`.

### Staggered sun cascades (#2063)

On Quality, all three cascades were drawn every frame: the shadow pass
0.26 / 0.62 / 0.49 ms at Goodsprings / Novac / Primm, plus about 0.15 ms of culling for
the three views; the farthest cascade holds 1,148 / 1,571 / 1,485 of the 1,418 / 2,138 /
1,870 casters. Now (shadow_rate.rs `STAGGERED_SCHEDULE`) the nearest is drawn every frame,
the middle one every other frame, the farthest every fourth, on different frames; a held
cascade keeps its map and the matrices of the frame that drew it
(`HoldDirectionalShadowMaps`, now a bit per cascade). On Quality: Goodsprings
5.78 → 5.42 ms, Novac 7.02 → 6.34, Primm 6.70 → 6.21. Screenshots against every frame:
0.26 % and 0.61 % of pixels over 24/255 at Goodsprings and Novac (two runs drawing every
frame: 0.20 % and 0.42 %; NPCs' shadows move). Performance keeps its schedule, both
cascades every other frame. What a held cascade did to the shadows' shimmer under the
sun's turn, and the fix: §11.

A cache of static casters (#2063's proposal) was prototyped and measured, and not adopted
[observed 2026-11-10, branch `perf/shadow-cache-prototype`, commit 056695cf3]: cascades
from the second on kept a cache of what stands still, with matrices 15 % wider (texels
30 % coarser), and drew only what moves on it each frame, putting the cache back by tiles
where moving things lay. Quality, ms, every frame / staggered / cached:
Goodsprings 5.71 / 5.40 / 5.48, Novac 6.96 / 6.34 / 6.12, Primm 6.60 / 6.13 / 6.18,
Freeside 5.39 / 5.17 / 5.28; Performance, no cache / cached: Goodsprings 5.12 / 5.10, Novac
5.62 / 5.58, Primm 5.51 / 5.44, Freeside 4.79 / 4.92. No better than the staggered
schedule overall, with coarser far shadows and far more vendored code (a split shadow
pass, cache textures, a part-render of a binned phase).

### Result

The owner's settings (SMAA, Sun shadows on Performance), the branch before these changes
(337d21dd7) against after, two interleaved runs each, mean frame time:

| Scene | Before (ms) | After (ms) | Frames a second |
|---|---|---|---|
| Goodsprings 12:00 | 5.49, 5.39 | 5.13, 5.23 | 184 → 193 |
| Novac 12:00 | 6.20, 6.12 | 5.66, 5.68 | 162 → 176 |
| Primm 12:00 | 6.82, 6.73 | 5.58, 5.56 | 148 → 180 |
| the Strip 20:00 | 5.27, 5.12 | 5.18, 5.22 | 193 → 192 |
| Freeside 14:00 | 5.29, 5.21 | 4.86, 4.78 | 191 → 208 |
| Prospector Saloon | 3.48, 3.46 | 3.37, 3.34 | 288 → 298 |
| Lucky 38 floor | 4.55, 4.62 | 4.48, 4.37 | 218 → 226 |

Every outdoor scene measured but the Strip (unchanged within noise) is now over 175 a
second, Primm over 180.

## 11. Shadow shimmer under a turning sun (#2833, 2026-11-16)

Players reported the sun's shadows shimmering while they stood still. Bevy places each
cascade (`bevy_light` `calculate_cascade`) on a grid of whole texels in the light's axes,
so that the edges stand still while the camera moves under a still light; that grid runs
through the world's origin. The sun light turns a little every frame with the clock (sky.rs
`aim_sun`, `SkyState::sun_light_hour`), and a turn of `dθ` slides the grid under a camera
`d` units from the origin by `d × dθ`: about 2.5 units, four texels of the nearest
cascade, a frame at 70,000 units out with a game day of 48 minutes. Every frame then
samples every edge at a new fraction of a texel. The staggered schedule (§10) turns this
into a pulse: a held cascade keeps the matrices it was drawn with and stands still, and
changes all at once on the frame it is drawn again.

The fix (nvforever shadow_anchor.rs) [engineering choice]: after Bevy builds the cascades,
each cascade of the sun and of the arms' shade light is moved across the light by at most
about a texel, toward the middle of its slice, onto a grid through an anchor near the
cascade's middle. The anchor stays put until the cascade's middle is more than 512 texels
from it on some world axis (up to about 887 texels in a straight line) and then moves by
whole texels in the light's axes. The grid then slides by the distance from the anchor
times `dθ`: at most about 0.03 texel a frame at the cascade's middle and 0.07 at its edge,
against several texels a frame on the origin's grid.
Then the cascades are stretched toward the sun and the held ones given back, in that order
(shadow_anchor.rs `cascade_edits`). Test aid `NVF_PLAY_TEST_SHADOW_ANCHOR=0` leaves the grid
through the origin.

[observed 2026-11-16: Goodsprings' start view standing still, 1280 × 720, vertical sync
at 60 Hz, `--no-actors --no-grass`; the mean change of luma (of 255) between consecutive
frames over the ground and the house]

| Sun shadows, hour | Grid through the origin | Anchored |
|---|---|---|
| Quality, 09:00 | 0.02 / 0.10 alternating (0.10 on the frames the middle cascade is drawn; 0.10 every frame with `NVF_PLAY_TEST_SHADOW_STAGGER=0`) | 0.01, staggered or not |
| Quality, 09:00, the sun light held still | 0.02-0.03 | |
| Quality, 07:00 | 0.114 | 0.012 |
| Performance, 07:00 | 0.25 / 0.00 alternating (0.127) | 0.013 |

Anchored, about 90 % of the change between frames is gone, below what is left with the
light held still on the origin's grid; the pulse at half the frame rate is gone with it.
The anchoring is a few matrices per cascade a frame; its frame time was not measured
separately.

## 12. Where the sun's cascades end (#2843, 2026-11-16)

Players found the sun's shadows sharp only in a small radius around the player and coarse
beyond, worst on Performance. Every count had the same layout (Bevy's builder): the first
cascade ends at a twelfth of the shadow distance (667 of the default 8,000 units, about
9.5 m), the others spaced geometrically up to the distance times the reach (1.5 with longer
shadows). Each cascade's texels were about 4.2 times the size of the one before (0.77, 3.2,
13.8 units with longer shadows), and the Smooth filter's blur is a fixed number of texels,
so an edge got about four times softer at each seam. On Performance's 2 cascades
everything past 667 units lay on the last cascade.

[observed 2026-11-16: Goodsprings' start view at 09:00 standing still, 2560 × 1440, an RTX
5070 Ti, ambient occlusion, longer shadows (reach 12,000), light glow, 16x anisotropic
filtering, sun shafts, `--no-actors`, cascades anchored (§11); the median of five 4-second
periods, two rounds; sun shadows off 2.81-2.85 ms a frame. Third cascades on Performance
drawn every fourth frame; four cascades on Quality every frame / every other / every fourth
/ every fourth, the last two on different frames]

| Layout | Far bounds (units) | Maps | Texel sizes (units) | ms a frame |
|---|---|---|---|---|
| Performance as it was | 667 / 12,000 | 2048² | 0.77 / 13.8 | 3.05-3.10 |
| Performance, first bound 1,500 | 1,500 / 12,000 | 2048² | 1.72 / 13.8 | 3.07 |
| Performance, three cascades | 1,000 / 3,500 / 12,000 | 2048² | 1.15 / 4.0 / 13.8 | 3.07-3.10 |
| **Performance, three cascades, farther** | 1,500 / 4,500 / 12,000 | 2048² | 1.72 / 5.2 / 13.8 | 3.09-3.11 |
| Quality as it was | 667 / 2,828 / 12,000 | 2048² | 0.77 / 3.2 / 13.8 | 3.15-3.16 |
| Quality, four cascades | 700 / 2,000 / 5,000 / 12,000 | 2048² | 0.80 / 2.3 / 5.7 / 13.8 | 3.22 |
| Quality, four cascades, wider | 1,000 / 2,800 / 6,500 / 12,000 | 2048² | 1.15 / 3.2 / 7.5 / 13.8 | 3.24-3.27 |
| Quality as it was | 667 / 2,828 / 12,000 | 4096² | 0.38 / 1.6 / 6.9 | 3.38 |
| Quality, four cascades, wider | 1,000 / 2,800 / 6,500 / 12,000 | 4096² | 0.57 / 1.6 / 3.7 / 6.9 | 3.42-3.43 |
| **Quality, four cascades, farther, blend band 0.4** | 1,500 / 3,500 / 7,000 / 12,000 | 4096² | 0.86 / 2.0 / 4.0 / 6.9 | 3.43-3.48 |

In pictures of a long pole shadow (one per row, the same view): with 2048² maps a farther
first seam costs sharpness at the player's feet (a wire's shadow is crisp at 0.77 units a
texel, soft at 1.15, blurred at 1.72); with 4096² maps the near shadow stays crisp with the
seams farther out. The two rows in bold looked best to the contributor in play.
Performance with three cascades keeps a clean shadow of a telegraph pole's crossarm where
the two-cascade layout has a shapeless blob, for 0.01-0.05 ms here.

Proposed [engineering choice: the contributor's proposal, for the maintainer to decide]
(sky.rs `shadow_cascade_layout`, a pure function of the distance, the reach, the layout
and the shadow maps' side): the bounds as shares of the shadow distance, so the Shadow
distance slider and `--shadow-distance` move them, with only the last stretched by the
reach (longer shadows); the moon's single cascade unchanged.

- **Performance**, 3 cascades (2 before): far bounds 0.1875 and 0.5625 of the distance
  (1,500 and 4,500 of 8,000), whatever the maps' size, overlap 0.2. The schedule
  (shadow_rate.rs `PERFORMANCE_SCHEDULE`) draws the first two every other frame,
  together, and the third every fourth frame on a frame the first two are not drawn.
- **Quality**, 4 cascades (sky.rs `CascadeLayout::Quality`, the default; 3 before), laid out by the maps'
  side (the Shadow resolution row, `shadow-resolution`): with 2048² maps 0.0875, 0.25
  and 0.625 of the distance (700, 2,000, 5,000), overlap 0.2; with 4096² maps 0.1875,
  0.4375 and 0.875, the second and third capped at 0.3 and 0.6 of the last bound, and a
  blend band (overlap) of 0.4. With Longer shadows (reach 1.5) the caps leave 1,500,
  3,500, 7,000, 12,000, the layout measured and pictured above. Without them (reach 1, the
  play default) the uncapped bounds would be 1,500, 3,500, 7,000, 8,000, where the fourth
  cascade (4,200-8,000 with the 0.4 band) adds almost nothing beside the third
  (2,100-7,000) for a fourth draw; the caps give 1,500, 2,400, 4,800, 8,000. That reach-1
  layout is derived by this rule [engineering choice, for the maintainer to decide] and
  was not measured: only reach 1.5 was. With 2048² maps the wider
  layout's near shadow is softer than before, so 2048 keeps the nearer seams. The schedule (shadow_rate.rs `STAGGERED_SCHEDULE`) draws the nearest every
  frame, the second every other frame, the third and the fourth every fourth frame on
  different frames: two cascades drawn in each frame, 2 of 4 on average against 1.75
  of 3.
- **Shadow resolution** 2048 (as before, the default) or 4096: the side of
  `DirectionalLightShadowMap`, changed in play. Bevy keeps every directional light's
  cascades and every shadowed spot light's map as layers of one 32-bit depth texture of
  that side (the vendored bevy_pbr light.rs `prepare_lights`: the most cascades any view's
  directional lights have, plus one layer per shadowed spot light), so the row sizes the
  sun's cascades, the first-person arms' shade light's cascade (Quality only) and the
  flashlight's shadow, whose pass draws 4096² too. By arithmetic a layer is 64 MiB at
  4096² against 16 MiB: about 240 MiB more on Quality by day in first person (five layers), 144 MiB on
  Performance (three), and 48 MiB more again with the flashlight on; not measured. 4096²
  maps measured 0.2-0.3 ms on this GPU (the table); not measured on a slower GPU. No
  quality preset sets the row.
- `--graphics-cycle cascades=<n>` (the test aid) asks for n cascades in the spacing of
  before (sky.rs `CascadeLayout::Spaced`), so `cascades=3` still measures the 3 cascades
  of before against the modes' layouts.

For the maintainer to decide: the 3 cascades to 8,000 units are the choice of 2026-10-12
after the playtest's "shadow rendering seriously affects performance" (graphics-enhanced.md
§5.3; sky.rs `DEFAULT_SHADOW_DISTANCE`: the old default of 16,000 units in 4 cascades took
19.0-20.8 ms a frame and 8,000 in 3 13.7-15.2; 8,000 in 4 took 15.0-16.8, every cascade
drawn every frame, before the casters were culled to the slice, graphics-enhanced.md
§5.7, and before the schedule). This keeps the 8,000 units; with the staggered schedule of
#2182 a fourth cascade measured 0.06-0.1 ms here. Performance is the mode this audit says
the maintainer plays on; its third cascade measured within the noise of the two of
before. Whether 4096 is any preset's default is left open.

At short distances (the Shadow distance list goes from 0 % to 100 % of `--shadow-distance`
in steps of 20 %; 0 % casts none) the first bound keeps its floor of 200 units, or half
the last bound where that is nearer; each in-between bound is then
its (capped) share held at least `i` steps above the first bound and `n − 1 − i` steps
below the last, a step being 5 % of the room between them, so the bounds always rise. The
limits act only when a share comes within a step of them: at 8,000 units every layout of
the table keeps its shares exactly, with or without Longer shadows (at 800 units Quality
with 2048² maps gets 200, 230, 500, 800: its second share, 200, is lifted a step above
the first). The log says the layout whenever the bounds or the maps' side
change: `sun shadows: cascades far bounds [700, 2000, 5000, 8000] texels [..] units`.

## 13. Graphics pass at 4K with DLSS: every setting's cost, Bevy backports (2026-11-24)

Asked for: go over each graphics setting and feature, find what costs frame time or GPU time
for what it shows, and what Bevy users do that NVForever does not yet.

[observed 2026-11-23/24 throughout unless said otherwise: Ryzen 9 9950X3D (the game placed
on the V-cache CCD, cpu_placement.rs), RTX 5090 (Vulkan), 3840 × 2160 borderless, DLSS
Quality (model L), every graphics switch and list at its top step (sun shadows Quality,
Smooth filter, 4096 maps, lamp shadows Quality, ambient occlusion Quality, full detail
9 × 9, distant objects 12 cells, smooth terrain 4 × 4, every slider 1.00), frame generation
and latency reduction off, `--log-frame-times 5` (no vsync); standing views, the windows of
the first 15 s left out, the median of 5-s windows, runs interleaved, two rounds unless
said. The bench made an INI from the owner's with those values and kept the games muted]

### Measuring

- **The frame time hides most of it here.** This display holds presents to 160 Hz even with
  no vsync asked for (the driver's vertical sync with variable refresh), so every view but
  Novac measured 6.25 ms a frame whatever changed. The GPU's own time (the `gpu time` line:
  the sum of the top-level passes' timestamps) and processor time show a change instead:
  `--log-frame-times` now also logs `cpu time: main thread <m> ms a frame, process <p> ms a
  frame` (graphics.rs; `profile::thread_cpu_secs`).
- **Novac at noon is the one view that is not held**: 7.1-7.7 ms a frame with the GPU at
  5.0-5.5 ms and the main thread at 4.4-4.9 ms of processor time a frame. A `profile` build
  (spans summed over threads, slower than release) put the render thread's
  `gpu_preprocess::prepare_preprocess_bind_groups` at 1.09 ms a frame (0.94 at Goodsprings),
  `material::check_entities_needing_specialization` at 0.66-0.71, the transform propagation
  workers at 1.2-1.4, the sun's caster visibility (`check_dir_light_mesh_visibility`
  0.35-0.41 plus `sky::sun_casters_over_slice` 0.27), `water::distant_water` 0.26-0.29 (it
  should return early standing still; not explained) and, in Novac, the NPCs' character
  controller planning (`phys::npc_plan` 1.6).
- **Per thread** (each thread's processor time over 8-10 s, standing at Goodsprings at 160 a
  second): the 8 compute-pool threads 330-370 ms of each second, the main thread 500, the
  render thread 330. With the compute pool at 4 threads (a measuring build's test aid,
  not kept: #1884 adds `NVF_PLAY_TEST_COMPUTE_THREADS`) the
  process used 10 % less processor time but frames were 3-6 % slower (Goodsprings 6.25 →
  6.42 ms, Novac 7.24 → 7.69): the pool's time is parallel work, not idle wake-ups.

### What was changed

1. **Auto exposure's histogram** (vendored bevy_post_process, upstream Bevy's fix): every
   invocation of a 16 × 16 workgroup added a bin into the 64-bin histogram, the 192 past the
   last bin clamped onto bin 63, so the brightest bin was counted 194 times over and each
   workgroup made ~200 contended atomics on one address. The pass went from 0.14 ms to
   0.04 ms or below the log's top entries in every view.
2. **Shadow fetches skipped where a light adds nothing** (vendored bevy_pbr
   `pbr_functions.wesl`): beyond a point or spot light's range, behind its shading normal,
   or facing away from the sun, no shadow map is read and no contact shadow marched (the
   contribution there is 0 either way). Opaque pass 0.03-0.10 ms less (Novac 1.05 → 0.98,
   Freeside 1.06 → 0.96, the saloon 0.43 → 0.37).
3. **Small casters left out of far cascades** (vendored bevy_light,
   `NVF_SMALL_CASTER_TEXELS`, default 1, 0 off): a caster whose bounding radius is under one
   texel of a cascade is not drawn into it. Novac: shadow pass 0.35 → 0.32 ms and frame 7.76
   → 7.59 ms (fewer casters to gather and queue).
4. **GPU preprocessing bind groups kept** (bevy#26418 backported): Novac 7.67 → 7.24 ms a
   frame (−5.6 %), the one view the render thread holds.
5. **Two-phase GPU occlusion culling, as an experiment** (`NVF_OCCLUSION_CULLING=1`,
   gpu_occlusion.rs; bevy#26239 backported, bevy_core_pipeline vendored for it). Without
   bevy#26239 it culled visible rocks, posts and plants under DLSS: Bevy built the depth
   pyramid from the whole display-size depth texture while the main pass draws only the
   render rectangle, and NVForever's upscaler glue copies the expanded depth back over that
   texture (upscaler/prep.rs). With it, screenshots against a run without differ only where
   actors moved. Novac: GPU 5.30 → 4.58 ms (prepass 0.59 → 0.30, opaque 1.07 → 0.75), but the
   late culling phase costs the CPU: frames 7.24 → 7.49 ms. Not on by default: it pays on a
   machine the GPU holds, not on this one. First-person shapes keep `NoFrustumCulling`,
   which the vendored mesh_preprocess.wesl now also reads as "never occlusion culled".
   [2026-11-24, #3783] The player's choice since: the Graphics row Occlusion culling (off by
   default, read at the start; docs/settings-reference.md), the test aid still overriding
   it. On the build with the merged scene unpacking: GPU 4.96 → 4.31 ms, frame 6.96 → 7.18.
6. **GPU light clustering's buffer churn** (bevy#25869, bevy#26588 backported): the reason
   for CPU clustering (#897) is gone, but `NVF_GPU_CLUSTERING=1` measured no better (Novac
   7.59 → 7.88 ms, Goodsprings at night the same), so the CPU stays the default.

### What each setting costs (Novac and Goodsprings at 12:00, one run each)

From the maxed settings, one setting changed at a time. GPU sums move ±0.2 ms between runs
with the NPCs, so the passes that moved are what to read:

| Setting changed | Frame (ms) | What moved (ms a frame) |
|---|---|---|
| (all at the top) | 7.07 | DLSS 1.95, opaque 1.03, prepass 0.57, shadows 0.35, culling 0.39 |
| Mesh terrain blend off | 7.12 | opaque → 0.65, shadows → 0.28, culling → 0.28 (Goodsprings: opaque 1.18 → 0.94, culling 0.49 → 0.44) |
| Full detail Off (from 9 × 9) | 6.56 | opaque → 0.72, prepass → 0.28 (Goodsprings: opaque 1.18 → 0.86, prepass 0.42 → 0.22) |
| Distant LOD off | 6.33 | main thread 4.35 → 4.00, process 26.7 → 22.7 (Goodsprings: main thread 3.51 → 2.21, GPU −0.17) |
| Sun shadows Performance | 6.70 | shadows → 0.08 |
| Shadow resolution 2048 | 7.02 | shadows → 0.16 |
| Shadow filtering Fast | 7.12 | opaque → 0.84 |
| Ambient occlusion Off | 7.05 | opaque → 0.81, the occlusion pass 0.08 → 0 |
| Lamp shadows Off | 6.83 | process 26.7 → 24.0 |
| Smooth terrain Off | 6.81 | main thread 4.35 → 4.25 |
| DLSS Balanced | 7.14 | DLSS → 1.67 |
| DLSS model K or Default | 7.09, 7.03 | DLSS the same (1.96) |
| FSR 3.1 Quality instead of DLSS | 7.07 | the upscaler → 0.70 (Goodsprings: 2.14 → 0.81) |
| Light shafts, bloom, glow, sharpen, haze, clouds, terrain shadows, water, grass, aerial perspective, terrain textures, anisotropic, eye adaptation, each off | 7.02-7.28 | within the noise |

The dearest switches for what they show: the mesh terrain blend (about 0.5 ms of GPU time:
opaque 0.38, culling 0.11, shadows 0.07), the 9 × 9 full-detail ring (0.6 ms) and, on the
processor, the distant LOD (0.7 ms). [corrected 2026-11-24] The blend's cost is not its
material clones: there is one clone per source material and publication
(mesh_terrain_blend.rs), and a bindless slab holds each resource once, so the clones of a
publication share the 421 resources they bind (mostly the fallback image at the unused
palette indices) and their slabs. The opaque share is the blend's own fragment work, which
already stops after three height reads where a fragment has no ground contact
(nif_surface_bindings.wesl `mesh_terrain_lookup`); the culling share was its extra batch
sets' dispatches, which the merged scene unpacking below removes.

### Result

Main and the branch in one session, interleaved, two rounds (GPU time without the DLSS pass,
whose own time drifted ±0.2 ms from run to run in both): Goodsprings 2.89 → 2.82 ms (−2.4 %),
Novac 3.48 → 3.33 (−4.2 %), Freeside 2.31 → 2.10 (−8.9 %), the Prospector Saloon 1.20 → 1.06
(−12.1 %), Goodsprings at 23:00 2.77 → 2.61 (−5.8 %). Novac's frame, the one not held at
160 Hz: 7.62 → 7.17 ms (−5.9 %). Screenshots at a fixed frame time differ from main's by no
more than two runs of main differ (0.05 % of the pixels by more than 24 of 255 at
Goodsprings, 0.000 % at night); the Vulkan validation layer reports nothing new; 21 smoke
scenarios of the touched systems pass.

### Merged scene unpacking (2026-11-24)

`uniform_allocation` and `bin_unpacking` were one dispatch per batch set (98-132 a frame in
the main view alone, §10 "Culling dispatches per batch set"), each followed by a barrier,
so a few microseconds of work each took 0.07-0.25 ms of GPU time per pass. Now every batch
set of a view, phase and mesh indexed-ness is packed into one set of buffers (bevy#26580's
idea, which does not apply over #2064 as it is) and each step is one dispatch for all of
them: a job per batch set in a storage array, a workgroup-to-job table, the job's offsets
added to the batch-local indices (vendored bevy_render and bevy_pbr, engine/vendor/README.md
"Merged scene unpacking"). The batch sets' own buffers are no longer uploaded.

[observed 2026-11-24, the branch before and after in one session, interleaved, two rounds,
the settings above] Both passes fell from 0.07-0.25 ms each to under the log's top entries
(below 0.06 ms in Novac,
0.04 elsewhere) in every view. GPU time (the sum of the top passes, DLSS included):

| View | Before | After |
|---|---|---|
| Novac 12:00 | 5.31 | 5.00 |
| Goodsprings 12:00 | 4.97 | 4.55 |
| Freeside 14:00 | 4.38 | 4.05 |
| Prospector Saloon 20:00 | 3.00 | 2.88 |
| Goodsprings 23:00 | 4.84 | 4.43 |

Novac's frame, the one not held at 160 Hz: 7.25 → 6.94 ms (−4.3 %). Screenshots differ from
the build before by no more than two runs of that build differ from each other (Goodsprings
0.04 % of the pixels by more than 24 of 255, Goodsprings at night and the saloon 0.000 %),
and the multi-chunk scan (a batch set of more than 256 draws) ran at Goodsprings.

### Left, largest first

- **The mesh terrain blend's fragment cost** (opaque 0.38 ms in Novac): not its material
  clones (see the correction above). What is left is the blend's own lookup and detail
  sampling; measuring it needs a GPU trace of the blended draws.
- **Cell fades leave every streamed mesh dithering**: `--cell-fade 0` took Novac's prepass and
  opaque pass down about 0.07 ms each (fade.rs keeps the fully drawn range for life, so the
  prepass keeps a discarding fragment stage); taking the ranges off after a fade needs the
  non-dither pipelines ready, or meshes vanish while they compile.
- **`water::distant_water`** spends 0.26-0.29 ms a frame standing still in the `profile`
  build although it should return early.
- **The distant trees' upkeep** (`treelod::update_distant_trees`, 0.15 ms a frame standing
  still) walks every drawn tree cell's fade step and every wanted cell each frame; only cells
  near the loaded area and the ring can change step.
