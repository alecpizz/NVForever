# Android render cache admission evidence

[design] This observer investigates short-lived flat actor appearance without changing
materials, visibility, uploads, shader capabilities, loading gates or frame presentation.
It reads existing caches. Its records do not establish queue membership, submission,
successful drawing, pixels, or a cause of device loss or visual corruption.

## Actual debug gate

[documented: `GameActivity`, `android_render_admission.rs`] The observer requires a
debuggable APK and a local Android logging opt-in. Before starting the game process,
use `adb shell setprop log.tag.NVFRenderAdmission DEBUG` to enable it or set the
tag to `INFO` to turn it off. The JNI results are cached once per process. Missing
methods, errors, unknown values and release APKs disable the observer, regardless
of Rust optimization. Output is local only; no token, path, or device identity
crosses the gate. Host tests inject a synthetic gate.

## Bounds and timing

[design] The main-world `Last` observer requires an admitted world state, a settled
streamer, no active/unseen loading screen and no open main menu. Dialogue and the pause
menu still render that world. It can start a burst after loading, lifecycle resume, or
a newly visible skinned mesh. Visibility is the engine's CPU `ViewVisibility` observation,
not a pixel test. Concurrent triggers coalesce into the current burst.

Only eight frame offsets are sampled: 0, 1, 2, 4, 8, 16, 32 and 64. At most eight bursts
start in one process. Suspension/loading cancels the current burst and stale snapshots;
neither cancellation nor new meshes resets the process budget. Each sample retains at
most 128 skinned and 32 non-skinned NIF meshes, deterministically selected by internal
entity order. Total visible counts are separate from sample counts. Entity/asset handles
stay internal; no names, IDs, paths, coordinates, game records or pixels are logged.
After the budget expires the main observer stops scanning shapes.

Idle new-skin detection scans only once every eight eligible frames and retains
IDs only. Load/resume still start immediately; active non-sample frames perform no
shape scan or CPU material lookup. Full bounded shape/material capture occurs only
at actual sample offsets. New-skin detection observes only the retained lowest 128
visible entity IDs: a newly visible entity outside that selection cannot trigger
a burst. This is a capped sample, not comprehensive actor discovery.

[documented: plugin finish, bevy_extract] Render wiring is added in `Plugin::finish`
because this plugin is installed before DefaultPlugins creates RenderApp. The real
Extract schedule copies only the bounded snapshot. Render Cleanup reads cache state
and refuses duplicate tickets. It never acquires or changes `MaterialQueueProbe`, which
the existing Pip-Boy observer owns. No graphics device is required by the authored
extraction tests, and no GPU validation is claimed by those tests.

## Fixed numeric schema

One line starts `render: gpu_admission_v1`. It contains only fixed field names and
unsigned numbers: `burst`, `trigger` (1 load, 2 resume, 3 new visible skin), `sample`
(0–7), `camera` (a unique player camera was found), then `skin_*` and `rigid_*` groups.

For each group, `total` counts CPU-visible shapes, `sampled` counts retained shapes,
`cpu_mat` counts CPU material assets present, and `diffuse_ref` counts actual CPU diffuse
image references. `gpu_mesh` and `slab` read GPU mesh asset/allocation caches;
`instance` means the extracted render material instance still matches the sampled
material asset; `gpu_diffuse` reads GPU images for the sampled diffuse references.
`prepared` and `bound` read prepared materials and allocated bind groups. `main` and
`prepass` count shapes in the player's specialized pipeline caches; corresponding
`*_ready` counts shapes whose cached pipelines are ready. Missing prepass entries can
be legitimate. Different counters are independent; they are not combined into a claim
that an object was drawn. No camera means no matching-view pipeline evidence.

`rigid` means non-skinned NIF control meshes, not a claim that every shape is scenery.
The records are local log lines, not reports. A synthetic fixture checks the
Rust numeric schema and contains no game data.

## Qualification

Authored tests cover the APK metadata gate, actual App cap/order/hidden filtering,
sparse bursts and lifecycle/new-skin cancellation/budget, real Extract copying and
stale clearing with an existing foreign queue observer retained, and independent CPU/
GPU/stale-instance counter projection. These controls do not establish Android
driver behavior or device acceptance.

The actual App cadence test also measures test-only scan/capture counters: exactly
eight captures in a 65-frame burst, no detection scans during it, one detection
after eight idle frames, immediate resume capture, and no work after budget exhaustion.
