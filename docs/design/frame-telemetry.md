# Frame telemetry: JSONL schema 1

[hypothesis] This proposed contract for [#1276](https://github.com/LootJunkieAzza/NVForever/issues/1276) describes the design, not runtime results.

## 1. Enablement and output

[hypothesis] `NVF_PLAY_TEST_FRAME_TELEMETRY=<seconds>` requests one frames-only capture per process.
An unset value or zero disables it. Reject negative, nonnumeric and non-finite values; cap positive values
at the hard maximum of 60 real seconds. `NVF_PLAY_TEST_FRAME_TELEMETRY_GPU=1` additionally requests
render CPU and GPU observations; unset or zero leaves them off, and other values are errors.
The GPU aid alone does not enable capture. While capture is off, the GPU and ID aids are not read, so
a stray one logs nothing (#1312). Neither aid requires tracing or the custom span collector.
They do not change display sync, frame caps or graphics settings. The fixed limits are 100,000 attempted
frame records and 67,108,864 bytes (64 MiB), including the header, trailer and line separators.

[documented] Home selection and logs follow `NVFOREVER_HOME` or platform defaults (`crates/fnv-install/src/lib.rs:162-184,209-210`).
The run-log directory handler guards and creates that directory (`engine/nvforever/src/crash.rs:54-62`).
The write guard refuses the game install and original user folder (`crates/fnv-install/src/lib.rs:352-371`).

[hypothesis] Reuse that directory handling and `fnv_install::guard_write`, guarding the final file against
the explicit game install. Create `<home>/logs/frame-telemetry-<capture_id>.jsonl` exclusively, outside
ordinary run-log rotation. [design, #1312] At start the oldest captures are removed so that eight remain
with the new one (`KEEP_CAPTURES`; at most 8 × 64 MiB), since every smoke run of a telemetry scenario
writes one; the start logs `frame telemetry: removed <n> older captures (keeping 8)` when it removes any. Report home or write failures; never fall back to the current directory,
the game folder or `Documents\My Games\FalloutNV`. Pair the epoch clocks before the first update.
Duration includes startup. Producers check duration and frame limits before each payload and stop after
the Nth frame attempt. The writer enforces the byte budget before writing each line, reserving 16 KiB for
the trailer. If the next line would exceed that budget, it stops with `byte_limit` and cuts the interval
at the last wholly written record. Attempts with higher seq values are outside that interval and are not
exporter loss. A stalled update may delay finalization. Bounds stop capture without restarting it.
The first producer stop wins; simultaneous checks prefer duration, then frames. A writer byte cut may
shorten an already frozen interval. Writer errors override the stop, shutdown timeouts truncate, and the
game continues.

## 2. Timing and provenance

[documented] Here, `bevy_*` source paths refer to the pinned 0.20.1 crates. `TimePlugin` updates
`Time<Real>` in `First/TimeSystems` (`bevy_time/src/lib.rs:82-87`). Automatic time uses a received
renderer instant or falls back to `Instant::now()` (`:152-192`). The renderer sends an instant after its
render schedule attempt, even without rendering (`bevy_render/src/lib.rs:462-465,522-534`).
Real delta is the saturating difference between update instants; its first delta is zero and excludes
pre-first-update startup time (`bevy_time/src/real.rs:23-38,99-108`).
`FrameCount` is u32, starts at zero during Update, increments in Last and wraps; extraction copies it
into the render world (`bevy_diagnostic/src/frame_count.rs:10-39`; `bevy_render/src/globals.rs:16-37`).

[hypothesis] In main `Update`, after the named engine systems and phase and marker producers, read
`Time<Real>::delta().as_nanos()`. It measures cadence between First's instants, including waits, rather than
Update execution or CPU or GPU time. First duration is null; later zeros are valid. Require automatic time
outside synthetic tests. Collection time is separate from interval endpoints.

[design: `engine/nvforever/src/stress_sample.rs:17-37`] The stress sampler (`NVF_PLAY_TEST_SAMPLE`) does not
use this delta since issue #1415: its frame is the time between its own `Instant::now()` calls in
consecutive `First` runs, so the frame's time is taken at the same boundaries as its phase, counters
and place. That is the main thread's loop including the wait for the render thread at extraction
(`bevy_render/src/pipelined_rendering.rs:115-136`), whereas the received renderer instant can belong
to the frame before under pipelined rendering. The `stress sample:`, `stress frames:` and `stress hitch`
figures and `duration_ns` therefore need not agree frame by frame; means over the same run of frames
should agree up to the instants at its ends [hypothesis].

[documented] Render diagnostics import in PreUpdate and timestamp that import, not GPU execution.
Private frame data and completion callbacks provide no source frame number, and mapping can remain
pending (`bevy_render/src/diagnostic/mod.rs:66-74,126-135`; `diagnostic/internal.rs:217-237,572-583,727-746`).
CPU values measure host begin/end instants; GPU values measure timestamp query intervals. Only explicit
instrumented scopes are covered (`diagnostic/internal.rs:603-639`; `diagnostic/mod.rs:43-58`).

[hypothesis] In GPU mode, collect after PreUpdate sync. Keep the last measurement time per path; export
only a newer `measurement()`, never `average()`, advancing cursors even on drops. Exclude pre-capture samples.
State unavailable origin and age once in the header. Import time cannot establish GPU staleness or
associate a result with this CPU frame. Never sum overlapping scopes into an additive breakdown.
Keep spans and process CPU in legacy output. Never wait for readbacks; history omissions and upstream losses are not exporter drops.

[observed] `frame_telemetry/plugin.rs` collects in main `Update`, which follows the entire `PreUpdate`
diagnostic import schedule. Its per-path cursor holds at most 4,096 paths; an import-time watermark
deduplicates observations from paths that cannot enter that map. Cursors advance before queue admission,
including on exporter drops. The collector uses only `measurement()` and preserves its import timestamp.
GPU mode scans the whole diagnostic store on each collection update, including non-render paths before
filtering them. Traversal cost is bounded by the store's population, not by the 4,096-path cursor capacity.

[design, #1312] The commit is read at run time, not embedded at build time. Embedding it
(`build.rs` watching `HEAD` and refs, `option_env!`) recompiled the whole nvforever crate on every
commit or branch switch, documents-only ones included, and a separate crate would not help: its
dependents recompile when it changes. `engine/nvforever/build.rs` keeps only the Windows icon.

[observed] `frame_telemetry/git_paths.rs` runs when capture is on, in plugin start-up before the
epoch clocks are paired, so Git's time is not part of the measured duration. The checkout is the
crate's folder, a compile-time constant (`CARGO_MANIFEST_DIR`) that does not change with the commit.
The commit is reported only when:

- Git answers there and the checkout's top level holds the crate as `engine/nvforever`, so a source tree
  unpacked inside another repository never reports that repository's commit
  (`checkout_not_engine_repository`);
- no file a change of commit rewrites (the worktree's `HEAD`; the branch's loose ref, else the
  `packed-refs` files) is newer than the running executable (`checkout_changed_since_build`), so a
  checkout moved on after the build is not mistaken for the build's commit. Packing refs after the build
  also counts, which leaves the field unavailable, never wrong.

Otherwise `unavailable` gives the reason: `source_checkout_unavailable` (the folder is gone, as for a
copied executable), `git_unavailable`, `build_time_unavailable`, or one of the two above. Any
commit made after the build (building, then committing what was built, or a documents-only commit)
leaves the field unavailable until a build relinks the executable: unavailable rather than possibly
wrong [design]. The engine does not record uncommitted changes. The
stress runner records its own checkout's commit and dirty state (#1277), which describe the engine only
when both come from the same checkout. Git runs without a console window.

## 3. Records

[hypothesis] Write compact UTF-8 JSONL, one object and LF per line, without a BOM or non-finite numbers.
Keep required fields and exact u64 integers; unavailable values are null. Put the header first and trailer
last. Ignore unknown fields; report unsupported versions and record types. Drop and count payloads over
8 KiB; lines are at most 16 KiB. Optional header strings over 1,024 UTF-8 bytes become null with a reason.

| Common field | JSON type | Unit; nullable | Meaning |
|---|---|---|---|
| `type` | string | None; no | Identifies `header`, `frame`, `diagnostic` or `trailer`. |
| `seq` | u64 | Records; no | Counts capture-wide record attempts from header 0, assigned before capacity checks and never reused. |
| `t_ns` | u64 | ns; no | Gives collection time since the monotonic epoch; the header uses zero and trailer uses stop time. |

| Header field | JSON type | Unit; nullable | Meaning |
|---|---|---|---|
| `schema_version` | u32 | None; no | Is exactly 1; incompatible semantics require a new version. |
| `capture_id` | string | None; no | Is a unique token matching `[A-Za-z0-9_-]{1,64}` and the filename. |
| `run_id`, `scenario_id` | string | None; yes | Carry opaque harness IDs from `NVF_PLAY_TEST_FRAME_TELEMETRY_RUN_ID` and `NVF_PLAY_TEST_FRAME_TELEMETRY_SCENARIO_ID`; each matches `[A-Za-z0-9_-]{0,64}`. |
| `started_at_utc` | string | UTC; yes | Records the RFC3339 epoch clock pair, never the source for elapsed calculations. |
| `engine_revision` | string | None; yes | Gives the engine checkout's commit when it is unchanged since the executable was built (§2), never an inference from the runner or executable name. |
| `build_features` | array of strings | None; no | Lists sorted enabled nvforever crate features from compile-time cfg, not transitive features. |
| `adapter`, `backend`, `driver`, `driver_info` | string | None; yes | Copy AdapterInfo's corresponding fields; empty values become null. |
| `width_px`, `height_px` | u32 | Physical pixels; yes | Give the primary window size at the header snapshot. |
| `display_sync` | string | None; yes | Gives applied FramePace sync: `off`, `vsync` or `variable`. |
| `cap_fps` | u32 | Frames/s; yes | Gives the applied effective cap; zero means uncapped. |
| `refresh_millihz` | u32 | mHz; yes | Gives reported monitor refresh, without promising actual display cadence. |
| `window_present_mode` | string | None; yes | Gives the applied Window PresentMode, including Auto modes; it is not the driver's concrete mode. |
| `unavailable` | object<string,string> | None; no | Maps unavailable header metadata fields to reasons, excluding the conditional diagnostics_reason field. |
| `max_duration_ns`, `max_frames`, `max_bytes` | u64 | ns, frames, bytes respectively; no | Give the clamped duration and fixed frame and byte limits. |
| `diagnostics` | string | None; no | Is `enabled`, `disabled` or `unavailable`; it does not promise GPU samples. |
| `diagnostics_reason` | string | None; yes | Is null when enabled, otherwise `not_requested` or `no_renderer`. |
| `gpu_origin` | string | None; no | Is `unavailable_from_bevy_0_20_1`: source render frame and GPU result age are unavailable for all observations. |
| `render_cpu_origin` | string | None; no | Is `source_frame_unavailable_from_bevy_0_20_1`: render CPU observations also lack source frame identity. |

[documented] AdapterInfo exposes these identity fields (`wgpu-types-30.0.1/src/adapter.rs:145-185`).
FramePace computes caps and applies modes (`engine/nvforever/src/frame_pace.rs:134-167`).
[hypothesis] Snapshot after initial settings application. Stop with `settings_changed` before admitting
a later frame with different resolution or presentation settings; the game and legacy output continue.

[documented] Bevy makes renderer identity and capability resources available to the main world:
`RenderResources::unpack_into` clones `RenderDevice` and `RenderAdapterInfo` there
(`bevy_render-0.20.1/src/settings.rs:195-198`), called from `RenderPlugin::finish`
(`bevy_render-0.20.1/src/lib.rs:493-505`). No custom render-to-main transfer is required.
[observed] Telemetry snapshots those main-world resources on its first collection update, after the
settings loaded in `PostStartup`, settings UI, window mode application and frame pacing application.
It records enabled nvforever features `default`, `dev-aids`, `intro-video`, `profile`, `tracy` and
`tracy-memory` from compile-time cfg.
The collector follows `PlaySet::Sync`, the loading gate and handoff reveal. It emits `phase_enter` before
the corresponding frame, using a startup accumulator independent of the stress sample aid. Window mode,
physical size, sync, effective cap, refresh and present mode changes end the capture before another frame.

| Frame field | JSON type | Unit; nullable | Meaning |
|---|---|---|---|
| `main_frame` | u32 | Updates; no | Gives FrameCount observed in this Update, independently of seq. |
| `duration_ns` | u64 | ns; yes | Gives real delta at the §2 boundary; only the first update is null. |
| `duration_reason` | string | None; yes | Is `no_previous_time_update` for the null duration, otherwise null. |
| `phase` | string | None; no | Classifies this observation as `startup`, `loading` or `gameplay`. |
| `markers` | array of objects | None; no | Holds up to eight events observed since prior collection, in producer order; it may be empty. |

[hypothesis] Each marker has non-null `kind`, `id` and `t_ns`. Kind is a string chosen from `phase_enter`,
`scenario_begin`, `scenario_end`, `waypoint` and `event`. ID is an ASCII string of at most 64 bytes holding
the phase name, scenario ID or ordinal. Timestamp is u64 nanoseconds since epoch when emitted.
Markers may precede their frame and imply no cause. They contain no game data or copied log text.

[documented] Startup lasts through the sample seeing Streamer settled or absent, or its startup sum over
60 s; later hidden frames are excluded (`engine/nvforever/src/stress_sample.rs:380-404`;
`engine/nvf-play/src/loading.rs:170-173`). [hypothesis] Use that policy independently of the sample aid.
Startup takes priority; loading follows when Loading::unseen_frames is true. Gameplay includes the
remaining frames, including menus. Mark every phase entry.

| Diagnostic field | JSON type | Unit; nullable | Meaning |
|---|---|---|---|
| `path` | string | None; no | Preserves the Bevy render path, at most 1,024 UTF-8 bytes. |
| `unit` | string | None; no | Is `ms`, checked against the producer suffix. |
| `kind` | string | None; no | Is `render_cpu_elapsed` or `gpu_elapsed`, selected by path suffix. |
| `value` | number | ms; yes | Holds a new finite measurement, not an average; null denotes unavailable or invalid data. |
| `status` | string | None; no | Is `available`, `unavailable` or `invalid`. |
| `reason` | string | None; yes | Is null when available, otherwise `timestamp_queries_unsupported`, `no_sample_observed`, `non_finite_value` or `unit_mismatch`. |
| `observation_t_ns` | u64 | ns; no | Gives DiagnosticMeasurement.time relative to epoch, which is store import time; for absence it gives check time. |
| `observation_main_frame` | u32 | Updates; no | Gives FrameCount when the collector first observes the measurement or absence, without causal attribution. |

[hypothesis] GPU mode exports render paths ending in elapsed_cpu or elapsed_gpu. Without GPU samples,
emit a terminal `render/elapsed_gpu` sentinel, not a measured total. Check capability before reporting
unsupported, otherwise use no_sample_observed. Frames-only mode never exports diagnostics from other aids.

| Trailer field | JSON type | Unit; nullable | Meaning |
|---|---|---|---|
| `status` | string | None; no | Is `complete` when the capture stopped at a limit or a normal exit without exporter loss, otherwise `truncated`. |
| `stop_reason` | string | None; no | Is `duration_limit`, `frame_limit`, `byte_limit`, `normal_exit`, `settings_changed`, `writer_error` or `shutdown_timeout`. |
| `cut_seq` | u64 | Records; yes | Gives the last wholly written record's seq when the writer cuts at the byte limit; it is 0 for a header-only interval and null when there is no byte cut. |
| `frames_attempted`, `frames_written` | u64 | Frames; no | Count attempted and wholly written frame records, including null-duration records; a byte cut includes only attempts with seq at most cut_seq. |
| `dropped_frames`, `dropped_diagnostics`, `dropped_markers` | u64 | Records, records, markers respectively; no | Count unwritten record attempts, and markers beyond eight or carried by an unwritten frame; a byte cut includes only records and their frame-associated markers at or before cut_seq. |
| `gpu_observations_written` | u64 | Records; no | Counts written available GPU observations, excluding the sentinel. |
| `phases` | object | None; no | Contains exactly startup, loading and gameplay summary objects, including empty populations. |

[hypothesis] Each phase summary has non-null u64 fields `frames_written`, `samples`, `above_33_ms`,
`above_50_ms`, `above_100_ms` and `above_250_ms`. Samples count non-null durations; thresholds use strict
greater-than comparisons against integer nanoseconds. Nullable finite numbers `mean_ms`, `worst_ms`,
`p50_ms`, `p95_ms`, `p99_ms` and `slowest_1pct_mean_ms` are milliseconds. Empty populations have zero
counts and null measures. Never combine phases or reconstruct distributions from periodic averages.
Sort durations ascending as x[1..N]; nearest-rank p(q) is x[ceil(q*N)] for q = 0.50, 0.95 and 0.99,
without interpolation. For [10,20,30,40,100] ms, p50 is 30 and p95 and p99 are 100; strict counts above
33, 50 and 100 ms are 2, 1 and 0. The slowest-1% mean averages the largest max(1,floor(N/100)) values,
giving 100 ms in this example.
[documented] That floor rule preserves the existing metric, which is not p99 (`stress_sample.rs:453-465`).

## 4. Collection, volume and completeness

[hypothesis] Preallocate 1,024 frame slots and 3,072 diagnostic slots with reserved header and trailer
capacity. Drop diagnostics before using frame slots; drop frames when their queue fills. A bounded cursor
map drops and counts new-path observations on overflow; an import watermark prevents duplicate counts.
Per-frame collection never encodes JSON or performs file I/O. The only main-thread I/O is outside it:
plugin start-up resolves and creates the logs directory and guards the path before the first frame,
and the capture writes two run-log lines (start and summary) through the engine's ordinary logger,
which writes synchronously like every other log line. Producers assign seq, check stop conditions and field
bounds, and use a nonblocking queue send. Frame construction bounds markers to eight and their IDs to
64 ASCII bytes; diagnostic paths are at most 1,024 UTF-8 bytes. These bounds keep each encoded payload
under 8 KiB. A writer thread merges by seq, serializes and writes without blocking main or render threads.
It measures actual lines as a backstop, dropping and counting any oversized payload as exporter loss.
It keeps only wholly written durations, at most max_frames, and computes statistics at stop.

[observed] The first collector passes bounded header values and the already guarded path to the writer.
Only thread creation happens synchronously there. The writer opens the file with `create_new`, encodes
the header once, checks its line size and trailer reservation, and writes it before draining payloads.
Opening and header failures freeze capture through shared writer-error state and return an error report;
the polling system logs that report once. The header constructor performs no JSON encoding or file I/O.

[hypothesis] Before a line would exceed `max_bytes - 16 KiB`, the writer freezes producers and cuts at
the last wholly written record, publishing its seq as `cut_seq` and its collection time as the byte stop
time. Already queued attempts and in-flight submissions with higher seq values are outside the interval.
Counters carried with records preserve cumulative attempts through that cut without an unbounded history.
Markers, including overflow, belong to their frame attempt for cut accounting; pending markers without
a frame attempt at or before the cut are outside the interval. The trailer keeps the next capture-wide
seq without reusing numbers assigned after the cut, so its seq may be greater than `cut_seq + 1`.

[documented] Legacy output lists at most 12 top-level GPU paths, not an exhaustive path count
(`engine/nvforever/src/graphics.rs:353-361`). Bevy constructs nested paths and may emit both elapsed_cpu
and elapsed_gpu for each timed path (`bevy_render/src/diagnostic/internal.rs:515-519,603-639`).
[hypothesis] Budget about 200–300 bytes for an ordinary frame and 250–350 bytes for a diagnostic record,
including LF; markers and longer paths increase these estimates. At 240 fps, 60 s gives 14,400 frames:
about 2.7–4.1 MiB without frequent markers, leaving ample margin in 64 MiB. Using 12 distinct top-level
paths with both CPU and GPU values gives about 24 observations plus one frame per update; nested paths
increase that. At 300 bytes per observation and frame, this is about 103 MiB over 60 s, or a byte stop
near 37 s; 48 observations give about 202 MiB and a stop near 19 s. GPU mode cannot fit 60 s at those
rates; it stops early at the byte limit with a shorter, still complete capture. These estimates require measured delivery and byte rates.

[hypothesis] Record drops leave seq gaps; any exporter loss within the interval, including markers alone, truncates capture.
Statistics describe written samples and may be biased. Attempted frames equal written plus dropped
frames; written frames equal the sum of phase frame counts; seq gaps equal dropped frame and diagnostic
records. With a byte cut, counts and seq-gap checks cover only records with seq at most `cut_seq`, excluding
the later trailer seq and all attempts after the cut. Loss before or at the cut still truncates; attempts
after the cut are not exporter loss. Duration, frame and byte limits end a lossless bounded interval and
stay `complete` when nothing in that interval was dropped; the trailer's counts give its actual length.
Settings changes, writer errors before the trailer and shutdown timeouts truncate (a final-flush failure
after a written trailer is the exception described below). Freeze producers, drain the retained interval,
write the trailer and flush, allowing 2 s at shutdown plus the 100 ms trailer allowance described below.
The terminal sentinel is exempt from duration checks, but not byte capacity. On I/O failure, stop, log one error and attempt a
trailer only if writable. Mark partial lines or absent trailers incomplete; never invent exact final drops.

[observed] Normal collection and limit stops use `Capture::begin_finish_at` and `poll_finish`, which
close producers and poll the writer without waiting on a game frame. `AppExit` starts the same shutdown;
resource teardown outside collection waits only for the remainder of the original 2 s drain grace.
An atomic claim resolves timeout against trailer construction: timeout may claim an unbegun trailer and
omit it permanently, leaving the file incomplete. The writer claims before constructing the trailer and
rechecks the drain deadline immediately after its claim; if it has expired, it also omits the trailer.
Once construction begins, neither polling nor teardown assigns a different timeout status to that trailer.
They allow up to 100 ms beyond the original drain deadline for its write and final flush. If still pending,
they report `trailer in progress, final status unavailable`, detach the writer and claim no final status
or counts. A later trailer retains the status selected by its writer. A successful final flush returns the
exact serialized trailer. A final flush failure also preserves any wholly written trailer in the report,
with its original status, an error and `flush_confirmed = false`; the summary flags `flush unconfirmed`
instead of changing a trailer already written. `flush_confirmed` exists only in that report and the log
summary, not in the JSONL file: a consumer comparing captures must read the run log's summary line too and
treat a `flush unconfirmed` capture as unconfirmed.
The GPU sentinel distinguishes unavailable device capability from a known device lacking `TIMESTAMP_QUERY`
and `TIMESTAMP_QUERY_INSIDE_ENCODERS`; only the latter reports `timestamp_queries_unsupported`.

## 5. Coexistence, consumption and validation

[documented] Existing frame windows use Time<Real> in Update and omit cycle warmup; profile windows
use it in Last, while profile hitches use consecutive Instant calls (`graphics.rs:329-350`;
`profile.rs:165,267-316`). Branch `codex/1275-render-diagnostics` provides
`graphics::ensure_render_diagnostics` (`engine/nvforever/src/graphics.rs:320-324` on that branch).
[hypothesis] Keep all existing text lines unchanged. Telemetry retains warmup and all phases. GPU mode
must call that helper; frames-only mode does not request the plugin. Add only export-specific log lines.

[observed] The plugin logs one start line, `frame telemetry: capturing <seconds> s to <path>
(frames[ + render timings])`, and one terminal summary, `frame telemetry: <path> <status>
(<stop_reason>), <frames> frames, gameplay p50 <x> ms p95 <y> ms p99 <z> ms, dropped <n>, <bytes> bytes`.
GPU mode appends `, gpu observations <n>`, counting available wholly written GPU observations and excluding
the sentinel. Empty quantiles print `none`. A missing trailer produces a truncated summary with its reason,
without inventing final counts. A claimed trailer still in progress has the separate uncertainty message
described in §4. Invalid aids or synchronous output setup failures log one `frame telemetry: disabled:`
warning; asynchronous file creation or header failures appear once in the terminal writer-error summary.
An unset or zero aid installs no telemetry systems, resources or render diagnostics and logs nothing.
The smoke scenarios are `frame-telemetry-frames`, `frame-telemetry-gpu`, `frame-telemetry-off`
and `frame-telemetry-bad-aid`. The frames and GPU scenarios request 18 s and allow a 25 s run; they inspect
start/summary log lines, not JSONL validity or plugin installation. The GPU scenario requires a nonzero
logged GPU observation count. Runtime JSONL delivery, pacing and overhead validation remain supervisor work.

[documented] Stress gives each launch its own home and captures stdout (`tools/stress/stress/lib.py:251-280`).
[observed] Sibling #1277 records provenance in `tools/stress/stress/metadata.py:158-198`, attaches it in
`cli.py:285-286` and creates serial repetition folders in `cli.py:406-419`; those changes are not on this branch.
[hypothesis] Stress will pass unique run and scenario IDs and discover new matching files under each
launch's home/logs. Require one matching capture; save its path and validation per repetition. Validate
field types, finite values, order, strictly increasing seq, nondecreasing t_ns, bounds, IDs, null reasons,
count invariants and recomputed statistics, restricting counts and seq gaps to seq at most `cut_seq` when
it is non-null. Allow FrameCount wrap and import times preceding collection.
Keep engine identity separate from runner provenance. Older runs without files retain coarse results
and unavailable percentiles. Reject missing, invalid, partial or lossy requested exports for comparisons.
Preserve run-status and load gates; averaged repetition quantiles must never be called pooled percentiles.

[hypothesis] Synthetic tests should cover known distributions, N=99,100,199,200, threshold equality, first
null and later zero deltas, phases, seq gaps, FrameCount wrap, queues, cursor overflow, markers, all bounds,
write failures, partial shutdown, import deduplication, non-finite diagnostics and unavailable origin metadata.
Cover lossless byte cuts, losses before the cut, excluded later attempts and maximum-size escaped payloads.
Cover delayed or missing GPU data and unsupported timestamps. Test provenance
after commits and branch changes, with missing Git or outside a repository, legacy runs and repetition discovery.
Supervisor game checks should parse a capture, match markers and phases, inspect adapter and backend
availability, and exercise limits. After builds and checks finish, compare disabled release, frames-only
and GPU captures serially with equivalent settings, cache and load. Report overhead, CPU, available GPU
evidence, byte rates, size and drops; check pacing and console and display state. No overhead target is assumed.

[hypothesis] GPU mode exports all timed paths; on fast machines the byte limit shortens the capture
instead of a path filter. Cursor capacity is an implementation choice. Overhead, rates, device
availability, phase and marker ordering, and provenance refresh behaviour await implementation checks.
