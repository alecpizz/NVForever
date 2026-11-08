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
`graphics.rs`] `nvf_play::player::step` advances a 1/60 s accumulator
(`fnv_phys::Stepper::advance`) and runs the fixed steps the frame's time owes, up to 15 a
frame (`phys::play_params`: a whole 0.25 s frame of the virtual clock is caught up,
deliberately, so actors near the player cover the ground the clock passes). Its cost is
therefore about *ticks a frame × ms a tick*, and grows with the previous frame's length:
a slow frame owes more steps to the next one. With `--log-frame-times <s>` the system
counts and times itself, and each period logs, beside `frame time:`:

`physics: 2.00 ticks a frame (max 4) over 600 frames, 7.1 controller moves a tick (max 12), 31 blocks, 5.68 ms a frame in player::step, 2.84 ms a tick`

(an example of the format, not a measurement). Frames are those in which the system ran
while playing; frames with no player character or waiting for a cell to load are not
counted (`Meter::skip`). Controller moves are the player's (on the walking path) and each
walked actor's per step; blocks is the mean length of the actor block list
(`crate::blocking`); the two times are `Instant`-based, the whole system per frame and the
steps alone per step. Without frame-time logging no `PhysLoad` statistics are kept, no
`Instant` is read and the line is not logged (the `phys::*` spans are independent of it).
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

**Left to do.** Linux (`/sys/devices/system/cpu/cpu*/cache/index3`, `sched_setaffinity`,
same rules). Putting the background threads (generated-terrain bake, read pool) on the
other domain with `SetThreadSelectedCpuSets`, leaving the cache domain to the main,
render and compute threads [hypothesis]. A row on the settings page.

## 7. Exterior GPU time at 1440p (2026-11-08)

The owner's outdoor frame rate: Goodsprings at noon runs at about 90 fps where interiors
reach the 180 fps vsync cap. Measured on a Ryzen 7 7700X with an RTX 3070 (Vulkan) at
2560 × 1440, the owner's settings (4x MSAA, ambient occlusion, longer shadows, bloom, light
glow, sun shafts, sun shadows), vsync off, `--no-actors` unless said.

[observed] Where the frame goes before these changes:

- **The frame waits on the GPU.** The swap chain's acquire waits about 4.5 ms on the frame
  three back, so the GPU is the limit. Frame latency 1–3 and mailbox or immediate
  presentation change nothing.
- **About 3 ms of GPU time falls outside the pass timers** (`--log-frame-times`: 10.2 ms a
  frame, 6.8 ms timed). It grows with MSAA.
- **The CPU floor** at 640 × 360 is about 5 ms a frame with no actors. The campaign's
  main thread is about 7 ms with Goodsprings' residents.

**Terrain layers draw only where they paint** (`terrain.rs`, `quadrant_mesh`). Each
texture layer of a quadrant was a blended copy of the whole 17 × 17 grid, drawn at full
size even where its opacity is 0. A layer mesh now keeps only the triangles with a corner
the layer paints, and a layer that paints nothing has no mesh. The picture is the same:
screenshots differ from main by less than two runs of main differ from each other.
[observed] The blended pass took 1.2 → 0.74 ms looking at the town and 1.7 → 0.48 ms
looking at the ground; the frame took 10.2 → 9.7 ms and 7.6 → 6.4 ms.

**Bevy's indirect-parameter build jobs are merged** (engine/vendor/README.md,
bevy_render). An Nsight Graphics GPU Trace (5 frames) showed what the timers missed. The
graphics engine was busy 93 % of the frame, but the shader units were only 28 % busy. The
front end waited for the GPU to go idle 37 % of the time, and the frame had 2,518 compute
dispatches. Bevy's GPU culling builds the indirect draw parameters with one dispatch per
batch range of each view and phase. That pass has no timer, and wgpu puts a barrier
between dispatches that write the same buffers. Merging each view's ranges that follow one
another leaves 904 dispatches. Compute is then active 9 % of the frame instead of 16 %.
[observed] Interleaved runs of one build: 9.0 → 8.5 ms a frame.

Not done, measured on the way [observed]:

- **Light glow and sun shadows.** Light glow costs about 0.9 ms of the frame and sun
  shadows about 1.1 ms, each about half outside their timers.
- **Uniform allocation and bin unpacking** (Bevy) still make one dispatch per batch set:
  about 90 each a frame, 0.3–0.5 ms each pass.
- **The opaque pass** (2.6–4.2 ms) is the largest timed pass.
- **DX12 does not run** (the bindless NIF material's bind group layout fails validation),
  so it could not be compared.
