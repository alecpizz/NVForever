# Loading screens in `--play`

Owner request 2026-10-10: when the player changes cell through a door, pause the game and
show a loading screen while the destination loads; everything is on hold until loading
is done; the picture is one of the game's loading-screen textures, as the plugins list
them. Records and what is known about them: docs/spec-notes/loading-screens.md.

Code: `engine/nvf-play/src/loading.rs` (state, screen), `crates/fnv-world/src/loadscreen.rs`
(records, choice), `engine/nvforever/src/world.rs` (`loading_gate`, faster streaming
behind the screen).

## 1. What happens

1. A door (activation) or a script moving the player sends `TravelRequest` instead of
   `Travel`.
2. The same frame the loading screen is spawned, `PlayState` goes to `Loading` and Bevy's
   virtual clock is paused.
3. Two frames later (`SHOW_FRAMES`), when the screen has been drawn, the request goes on
   as `Travel`. Physics rebuilds its cells and places the player; the renderer drops its
   cells and streams the destination's. These are the long frames, and they now pass
   behind the picture instead of in front of a frozen view of the old place.
4. Behind the screen, streaming is faster than in play: the renderer loads cells for
   200 ms each frame instead of one cell a frame, physics takes up to 8 prepared cells a
   frame instead of 1.
   Until then the renderer loads the cells around the arrival's grid square
   (`Streamer::arrival`), not around the camera: in the frame of the travel the camera
   still has its transform of the place left, and around it (an interior's coordinates,
   far out in the worldspace) empty cells loaded at once and counted as the
   destination's; the screen then came down with the arrival's own cells still to load
   one a frame, and they faded in in front of the player (playtest list 4, items 9 and
   10, 2026-10-13) [observed: `--cell GSGasStation` through its door: 26 cell loads in
   all after the fix, no fade in the frame after the screen].
5. The screen comes down when physics has every cell it asked for around the player
   (`PhysState::loading_done`), the renderer has all the cells around the camera
   (`Streamer::settled`, reported through `Loading::render_busy`) and no actor in view is
   still waiting for its parts to be read (`readahead::actors_waiting`, reported through
   `Loading::actors_busy`; owner 2026-10-24: NPCs were seen being drawn in after the
   screen), the pipelines they are drawn with are ready (`pipeline_gate.rs`, reported
   through `Loading::pipelines_busy`, #1520), and that has been so
   for 0.25 s and 3 frames (`SETTLE_SECS`, `SETTLE_FRAMES`: time for textures and meshes
   to reach the graphics card). With Volumetric clouds on, a screen that has settled stays up
   until every texel of their picture has been cast (`Loading::clouds_busy`, nvforever `medium.rs`
   `clouds_wait`: drawn from nine casts since the last history cut, three frames in a row), but never
   past 1 s from when all else was in (`CLOUDS_HOLD_SECS`, the settle time counted) [design]:
   volumetric-medium.md "Under a loading screen". `PlayState` returns to `Playing`, the clock
   runs again.
6. If loading has not finished after 120 s (`TIMEOUT_SECS`) the screen comes down with a
   warning, so an unattended run cannot hang there.
   Pipelines have a shorter limit of their own: 15 s (`PIPELINE_CAP_SECS`) once they alone
   are waited for, with a warning naming those still waiting. One that has failed for good,
   or is still being retried 15 s after it was asked for, is logged with its label and not
   waited for again.

   Without this wait the view stayed flat under the HUD for 1-2 s after a start's screen
   came down, then the world appeared in one frame, with 104-125 pipelines still waiting as
   it came down [observed: Goodsprings, main e2517687 and the same with logging, all 19 starts with shots, across
   pool sizes]. A task holds Bevy's shader cache lock while it composes a shader [design:
   bevy_render 0.20.1 `pipeline_cache.rs`], and more threads did not shorten the wait
   [observed: 1 to 16 threads, about 3.7-4.1 s to clear in all but one batch]. Objects first seen after the screen is down are not covered.

7. Where the time went (#2407): when the screen comes down, `loading screen phases: up
   <s> s; <wait> <s> s, ...; longest frame <s> s of <n>; read <files> files, <MB> MB, <s> s
   reading, <s> s queued for an archive` (`LoadPhases`). Each wait (step 5's, `the clouds` among them, plus the
   start's scripts, its residents to draw, `actors to place` (§1a), and Pip-Boy warm-up,
   and the two frames before the travel goes out) is
   given the time from the screen coming up to the last frame it was still missing, so
   the one that ends last is what held the screen. The longest frame says whether the work
   was on the main thread; the reads are the game files' (`fnv_vfs::read_stats`, every
   archive and loose file read, from the screen's first counted frame): the time inside
   the reads, summed over the threads, and the time spent waiting for another thread's
   read of the same archive. A load over 20 s sends the same in its `slow_load` report
   (`phases`: `until_ms`, `longest_frame_ms`, `frames`, `read_files`, `read_mb`,
   `read_ms`, `queued_ms`) [design].

   Measured 2026-11-15 [observed: release build, i9-14900K, RTX 4070, Data on a SATA SSD,
   48 GB RAM, the game files in the OS cache; one run each]:

   | load | screen up | held last by | longest frame | files read |
   |---|---|---|---|---|
   | start in Goodsprings | 3.54 s | pipelines 3.27 s | 1.06 s | 1,361 (165 MB, 0.07 s) |
   | start in Doc Mitchell's house | 2.42 s | pipelines 2.13 s | 0.55 s | 1,132 (54 MB) |
   | Doc Mitchell's door out to Goodsprings | 2.48 s | pipelines 2.20 s, actors 1.95 s | 0.84 s | 2,106 (248 MB, 0.10 s) |
   | Goodsprings into the Prospector Saloon (first time) | 0.85 s | pipelines 0.57 s | 0.30 s | 755 (66 MB) |
   | the saloon's back door out | 0.76 s | pipelines 0.49 s | 0.39 s | 382 (35 MB) |
   | into the saloon again | 0.29 s | the travel's frames | 0.02 s | 0 |
   | a save in the saloon loaded (the run's start) | 2.40 s | pipelines 2.12 s | 0.59 s | 1,081 (47 MB) |

   With the driver's shader cache empty (`__GL_SHADER_DISK_CACHE_PATH` set to an empty
   folder, NVIDIA's Vulkan driver honours it) the same loads took 5.03 s, 1.26 s and 1.04 s:
   pipeline compiles add 0.3-1.5 s here, not the 20-37 s the reports saw (#2407: an Intel
   Core 5 210H laptop with an RTX 5050, an i5-14400F with an RTX 5060, an i7-11700K with an
   Arc B580). Nothing on this machine is 5-10 times slower; the reads took 0.1 s because
   the files were cached. [hypothesis] A slow or cold disk is the likeliest difference:
   the door out of Doc Mitchell's reads 2,106 files, one at a time per archive, so a hard
   disk at 8-10 ms a seek would spend 17-21 s on them alone. The reports' `read_ms` and
   `queued_ms` will say: seconds of reading for a few hundred MB is the disk; a long
   `pipelines` wait with little reading is shader compiles.

The player arrives standing: physics puts the character on the support the arrival rule
found and counts it as on the ground until its first step, so the body does not play the
jump loop behind the screen and a landing when it comes down (`PhysState::arrive_player`).

## 1a. The start (owner 2026-10-24)

A `--play` run that starts in the world (not on the main menu, `--menu`) is held behind
the same screen from its first frame (`nvf_play::loading::StartCover`, asked for by
nvforever). Before, the world was in view while the script data still loaded on its
thread (4–9 s on an unbusy machine, up to 21 s with three smoke games starting at once),
and NPCs were drawn in as their parts were read. The screen comes down when what step 5
waits for is in, and the scripts are made (and a `--load`ed save put back into them).
A travel asked for under it goes out behind it and is waited for too.

- Its time counts on no clock: the test aids' clock (`pause::PlayClock`) and
  `--run-seconds` count from when it comes down, as if the run began then. A step at
  14 s comes 14 s after the world can be played, with the world and the scripts run for
  14 s; the world aids' late-start rule (`pause::WorldAidClock`, `SCRIPTS_START_SECONDS`)
  then never holds them back. A run lasts its seconds plus its start.
- **The picture first.** The first render frame blocks on a blit pipeline [design:
  bevy_core_pipeline 0.20.1 `prepare_view_upscaling_pipelines`], compiled on the pool that
  also reads models and cells, so the world's reads delayed the screen: its picture showed
  in at most one capture of three starts [observed]. So under the start's screen cells and
  actors start loading only once the screen has had `PICTURE_FRAMES` render frames with the
  pipelines it is drawn with ready, or after `PICTURE_CAP_SECS` (`Loading::world_held`). Log: `loading
  screen: the world starts loading <s> s after the screen (drawn)`.
- **The residents in view are drawn under it** (nvforever `residents.rs`, `StartPlacing`;
  `Loading::start_actors`). Before, nobody was drawn under the start's screen: the AI
  waited for the game to go on, and the first frames of play drew the residents in view
  three a frame (`SPAWNS_PER_FRAME`): after a Novac start at 22:00, one frame of 65-106 ms
  right after the screen and about a second of 20-54 ms frames, against 6.5-7 ms after,
  with the driver's shader cache on and off [observed 2026-11-19: 0.1 s frame windows; 0
  spawns before the screen came down, 15-17 after it]. Trips already drew theirs under
  the screen (`Core::arrive`). Now, once the cells around the player are in (drawn and in
  physics) and the scripts are made, each resident present in a drawn cell and not yet
  handed to the AI is drawn where the AI will take it up on the first frame of play
  (`fnv_ai::Ai::add`: where the game has it, facing the same way), within 100 ms of each
  frame (`START_PLACE_BUDGET`, at least one a frame) and not three a frame. It begins the
  second frame the scripts are there, so a loaded game's race and sex changes have reached
  the actors' resolver (`appearance.rs`). The screen waits for them (`actors to place` in
  the phases line), then for their parts (`actors to draw`), their pipelines and the
  settle time as for the rest.
  - Only the bodies come earlier. The residents are not handed to the AI under the
    screen: they choose their first package on the first frame of play, after the
    scripts' first frame (OnLoad, GameMode), as before, so the packages and places are
    those it would give. Choosing them under the screen would have run their begin
    scripts and OnPackageStart before OnLoad, and judged their conditions before that
    frame [design: `fnv_game::ai` `set_package`]. Nobody would have walked anyway: the
    clock is paused.
  - What changes with it: a script's first frame acts on bodies that are there. An actor
    it disables is taken away in that frame, before it is seen; one it enables is drawn
    after the screen, as before [design: `residents::drive` runs after the scripts]. A KillActor naming a limb on a body in view (two corpses
    in Novac at the start, limb location 1) now finds its body and dismembers it as asked
    (`dismember::scripted_check`), where before it logged `no part at location 1 can go
    (or its body is not drawn)` [observed, Novac starts at 22:00 (#3081's description):
    before, those lines; with it, `dismember: Raider 0x000CEDCA: Head exploded
    (KillActor)` and the same for Glowing One 0x000CD346]. A body drawn under the screen
    is put at its walker's place on the first frame of play, not turned or walked there,
    so a first-frame MoveTo moves it as it did a body drawn then [design].
  - Their parts are read from when the cells are in (`readahead.rs`: the placed actors
    of the loaded cells, listed once under the start's screen), not from when they may be
    drawn, after the scripts. Log: `actor read-ahead: under the start's screen, <n>
    actors of the <m> loaded cells listed`.
  - The cap: 3 s (`PLACE_CAP_SECS`) from the first frame they may be drawn (the cells in,
    the scripts made: `StartActors::drawing`), counted afresh if that stops; then it lets
    them go with `loading screen: <n> residents in view still not drawn after 3 s;
    going on without them (drawn after the screen)`, nvforever stops drawing them under
    it, and those left are drawn after it, three a frame, as before.
  - Test aid `NVF_START_ACTORS_UNDER_SCREEN=0`: drawn after the screen, as before (logged
    at the start: `test aid NVF_START_ACTORS_UNDER_SCREEN=0: …`).
  - Log, as the screen comes down: `loading screen: start residents: <n> drawn under the
    screen, <m> in view left for after it` (with the aid: `… (test aid
    NVF_START_ACTORS_UNDER_SCREEN=0: all drawn after it)`; after the cap: `… (the screen
    stopped waiting for them after its cap)`); when drawing begins: `ai: the start's
    screen waits for <n> residents in view to be drawn under it`.
- Log: `loading screen: the start is held until the world can be played`, `loading
  screen down after <s> s (the start: cells, actors and scripts ready); …`, or after
  120 s `the start still waits for <what> after <s> s; going on without it`.
- A run taking over from another (`--handoff`, loading a save while playing) shows its
  window once this screen is down.
- [hypothesis] The original never shows the world before its scripts run: it reads the
  plugins, scripts and all, before the main menu.
- **The start's models on the read pool** (2026-11-19, `world::start_pool_prep`). The
  model reads have their own pool since #1732 (`nvf_assets::read_pool`), so under the
  picture hold `world::read_ahead_under_hold` already reads the cells around the camera
  ahead, as `stream`'s first frame would: their lists, landscape and models are prepared
  on that pool while the picture comes up, and `take_cell_work` takes the models in.
  Once the hold is let go, a cell whose models are not all in waits for that read-ahead
  (`world::wait_over`), nearest cell first, for at most 1.5 s of real time from the first
  cell asked for, after which the rest are drawn at once with their models prepared on
  the main thread as before; the screen still waits until every cell is drawn. Before, the
  read-ahead began only after the hold, and in the same frame every cell was drawn with
  each model prepared on the main thread, one after another: 0.48-0.59 s of main-thread
  time for the 25 cells and about 230 models of a Novac start (`streaming summary`) in
  two or three long frames, while the 4-thread pool had none of that work, then prepared
  67-111 of them a second time [observed: `NVF_LOADING_PROBE`, a local probe, Novac
  start, 4 runs]. Whether the hold itself is still worth its 0.7 s has not been measured
  (#1732); this uses it instead. Logs: `world: the start's read-ahead
  begins under the loading screen's picture hold: <n> cells around (<x>, <y>) read on the
  read pool`, and once the cells are all in `world: the start's cells drawn: <n> with
  their models prepared on the read pool, <n> before (the rest prepared on the main
  thread); <n> cells read ahead under the picture hold, <s> s from the first cell to the
  last`. Test aid `NVF_START_POOL_PREP=0`: as before (logged). A travel's screen is
  unchanged: no hold comes before it, and its cells are drawn at once. So are those of a
  travel asked for under the start's screen, and they are not counted in the start's line
  (`world::start_pool_cells`).

[design: `engine/nvforever/src/pipboy_arm.rs`, `player_body.rs`,
`engine/nvf-play/src/pipboy.rs`, `loading.rs`; #1803] Once the start's picture hold is
released and the real first-person rig exists, one hidden Pip-Boy warm-up borrows that
rig at the raise clip's hold pose, swaps the screen/glare materials and activates the
existing image camera. Without a worn Pip-Boy it warms only that camera and page. It
draws STATS Status under the cover, since ITEMS needs an open inventory, then restores
the player's page and STATS tab (`pipboy/items.rs::draw`, `pipboy/stats/mod.rs`). It
opens no inventory or script menu and borrows no player POV, weapon or input state.
The temporary animator pose is applied only after the normal body has supplied those
states. Transforms, root visibility and only the warm-up's culling overrides are restored
before the next body tick; materials and the inactive image camera follow in that tick's
UI pass. Loading waits explicitly for this teardown as well as its existing pipeline gate.

[design: `engine/nvforever/src/pipboy_arm.rs`, `pipboy_ready.rs`; #1803]
The start's hidden interval has a four-render-report floor and a 250 ms wall-clock
budget, followed by teardown. It waits for submission, not the global busy pipeline
count: this screen already owns the compilation wait. If submission is incomplete at
the budget, the rig stays cold. A first visible device opening verifies its own main
queue, prepared materials, bind groups and main/prepass pipelines behind the camera;
Tab/Escape can cancel it through the normal key handler. At three seconds it raises
anyway and logs incomplete readiness, bounding failed shaders/uploads or a lost rig.
The first visible report and device access sound wait for hidden-pose restoration.
Real held openings report their remembered script page immediately; startup stays silent.

[observed 2026-11-07, commit `4fbe5750d`, 8 alternating fresh launches, one machine] 4/4 clean first openings enabled and 4/4 blue or partial with `NVF_PLAY_TEST_NO_PIPBOY_WARM=1`. Screen-down medians 7.68 s enabled (7.38-7.98) and 6.81 s disabled (6.41-6.96): about +0.9 s in these launches.

[observed: 2026-11-07, commit `8fa3f8a04`, superseded by the set above] An earlier set of
eight alternating launches gave the same 4/4 and 4/4, with noisier screen-down medians
(8.84 s enabled, 8.27 s disabled). A later run without the vendored main-pass retries passed the same scenarios with a
clean first opening, so those retries were dropped (docs/spec-notes/pipboy.md §8).

[observed: earlier supplied first-open log and disabled twin, 2026-11-07]
The failed first attempt capped its startup warm-up at 3011 ms and took 12.98/10.79
seconds in one enabled/disabled pair. Zero new pipelines during the raise still left
blue silhouettes in the first three shots. See `docs/spec-notes/pipboy.md` section 8.

[design: `engine/nvforever/src/pipboy_arm.rs`, `pipboy_ready.rs`, `pipeline_gate.rs`]
The warm-up log's total requests still include concurrent world work. `own rig pipelines`
now names the actual main/prepass IDs for this rig. The existing image camera requests
four compatible UI/sprite/postprocessing/tonemapping keys; shared IDs are not extra
compilations. `NVF_PLAY_TEST_PIPBOY_DRAW=1` logs the first visible submission and changes
thereafter, including opaque entries and material/binding readiness; no logging system
is registered by default. `NVF_PLAY_TEST_NO_PIPBOY_WARM=1` remains the disabled twin.

[design: `tools/smoke/scenarios.toml`, `engine/nvforever/src/pipboy_arm.rs`]
Compare the four first-open shots and the restored-pose readiness report. The startup
hold is bounded by 250 ms plus teardown and scheduling; a cold first device opening
has a separate three-second cap. Check save load, no device, screen-menu frame, key
cancellation and a cold New Game gift at 16:9 and 4:3 after this review revision.

## 2. What is on hold

- Everything that runs only in `PlayState::Playing`: the player's input, movement and
  physics steps, activation, combat, the game clock, opening menus and the pause menu.
- Scripts: `run_scripts` returns while a loading screen is up, as it does under the
  pause menu. Loading-screen time does not count for scripts in the original either
  [observed: layer5-captures.md §8.3 e]. The two frozen frames after a change of place
  that stood in for a loading screen remain (they now follow the real one).
- Everything driven by `Time`: animations, doors, travelling NPCs, the sky's clock,
  on-screen messages, the `NVF_PLAY_TEST_*` aids and `--run-seconds` (a run lasts its
  seconds of game, plus the loading).
- All input (owner 2026-10-27: New Game could be clicked and the arrow keys moved the
  main menu while a save loaded): `loading::hold_input` lets go of every key and mouse
  button in `PreUpdate`, after the controller's menu keys and the test aids' presses and
  before the interface reads the pointer, and the screen blocks the pointer from what
  is under it. A key held through the screen counts only once pressed again.
- Not on hold: streaming (render and physics), distant LOD generation, and a voice line
  already playing (as under the pause menu; voice.rs).

## 3. The screen

- The record: `fnv_world::loadscreen::choose` over every LSCR of the load order. Screens
  that belong to the destination (its interior cell, or a grid square within 2 of the
  arrival square in its worldspace) and screens for anywhere are both allowed; when the
  destination has its own, one of them is taken every other time. Both rules are
  hypotheses (spec note §3).
- The picture: the record's ICON, read through the texture cache like any other
  texture. Stretched to the window, as the original's (spec note §4). A record without a
  picture, or one whose texture is missing, gives a black screen with the rest. The whole
  screen is over black from its first frame.
- The tip (`loading_look::fit_tip`): the record's DESC at the top, set by the LSCT its WMI1
  names: box, font, colour and alignment (`fnv_world::loadscreen::TipStyle`; without an LSCT
  of the tip type, the values of the base game's one). x and width follow the window's width
  as pixels of a window 1280 wide, y and the text's size its height as parts of 960. It lies
  on `loading_text_bar.dds` across the window, as high as the text. No text, no band. Drawn
  again when the window's size changes, since the wrapping follows its shape.
- The emblem (`loading_look::Emblem`): `meshes\interface\circular loading\loading01.nif`,
  the wheel's centre at (0.5 W, 0.794 H) and the wheel 0.172 H across, its nodes turned by
  the sequence `Idle` on real time, since the virtual clock is paused behind the screen. By
  elapsed time, so a long frame makes it jump to where it should be. Without the model (or
  with one that does not read) no emblem and one log line; nothing waits for it.
- Read once and kept (`loading_look::Look`): the emblem, its textures, the band and the font.
- The original's picture stage, objective, stats and level bar are not built (spec note §4).

## 4. Scope and open points

- [design: `engine/nvf-play/src/graphics_resume.rs`, `pause.rs`;
  `engine/nvforever/src/pipeline_gate.rs`] The pause menu reuses the pipeline cap and
  settling constants for effective graphics edits (#1820), retaining its own picture
  and pause state. Its bridge always counts shadows and requires fresh render samples.
  See [graphics settings](graphics-enhanced.md#3-what-was-built) for the policy and tests.
  This is only the pipeline portion of #1820: draw-distance streaming still needs a
  separate readiness contract and remains open.

- Travel and a world start (§1a) show a loading screen. A New Game chosen on the main
  menu still loads behind the menu and the intro film.
- Travel without `--play` (the free camera following `--npc-travel` NPCs) is unchanged:
  no loading screen.
- The game's rules for choosing a screen are open (spec note §3).
- Whether a travel's screen should wait for the sun's shadow pipelines is open (#1732): test
  aid `NVF_PLAY_TEST_TRAVEL_SKIP_SUN_SHADOWS=1` has it not wait, so that both can be measured
  (`engine/nvforever/src/pipeline_gate.rs`, `sun_shadow`). The start's screen always waits.
- Test aid: `NVF_PLAY_TEST_LOADING_SHOT=<png>` saves the window when the travel goes out
  (the screen is up); later screens of the run go to `<name>-2.png`, and so on.

## 5. Checks

- `fnv-world`: `loadscreen::tests` (locations, choice), `tests/load_screen_type.rs` (LSCT
  on a plugin written for the test).
- `nvf-play`: `loading::tests` (cover, destination, a tip only with text, and the sequence:
  screen up and clock paused before the travel goes out, held while the renderer is busy,
  down and playing again after; held while actors are read; the start's screen and its
  clock; a travel under the start's screen; the start held while residents in view are to
  place, not with the aid, and let go at the cap, counted only while they may be drawn; the clouds'
  hold and its limit, which never hangs a screen), `loading_look::tests` (the tip's box and band
  and the emblem's place at 1280x720 and 1280x1024, its turning).
- `nvforever`: `residents::tests` (the start's residents drawn where the AI takes them up,
  the frame's budget, the aid).
- Smoke: `loading-screen-look`; `start-residents-under-screen` and its twin with the aid,
  `start-residents-after-screen-aid`.
- In the game (owner's data): Doc Mitchell's front door, both ways; see the pull request.
- Where a start's first seconds go (`engine/nvforever/src/start_times.rs`, every run): `start:
  Startup <s> s; frame 1 <s> s: <s> s before Update, <s> s in Update, <s> s after it` and
  `start: first render frame finished <s> s after frame 1 ended`. For the systems inside
  them: a `--features profile` build with `NVF_PLAY_TEST_PROFILE=1`, whose first `profile:`
  line covers the first frames.
