# Rapier 0.36.1 collision-query refresh corrections

[documented] The baseline is the published `rapier3d` 0.36.1 crate, SHA256
`033e2edc815d61ab97bd74ed5d1cbea5ebe575218774a7cfedbd3551f0033317`.
Its published VCS metadata names `475caa3ff4646fd9a796b67d14116256f4962bc7`
and records `dirty: true`; the package checksum identifies the exact published
source. All 326 published files are retained. Only the two Rust files listed
below differ from that package, each with a modification notice. The standard
Apache-2.0 LICENSE text is retained unchanged from the earlier vendored baseline.
No global Cargo registry source is modified.

[design] NVForever runs a zero-duration physics step to register body changes and
refresh collision queries without advancing simulation time. Two bounded changes
repair the lifecycle of this supported step:

- `src/pipeline/physics_pipeline/substep.rs`: join a deferred broad-phase BVH
  optimization before the zero-duration early return. Otherwise the tree can
  remain temporarily empty and scene queries miss colliders. This is the same
  production change proposed by [Rapier PR #1067](https://github.com/dimforge/rapier/pull/1067);
  it is not claimed as part of the published 0.36.1 release.
- `src/dynamics/island_manager/manager.rs`: after rebuilding persistent sleep
  components during bootstrap, call upstream `0.36.1`'s already-existing
  `IslandManager::commit_sleeping_chunks` (unchanged in upstream
  `src/dynamics/island_manager/sleep.rs`) for sleeping bodies still present in
  the awake island. Mixed components were already woken by the original bootstrap
  and remain awake. This keeps the persistent graph and active chunks coherent
  after query registration followed by an explicit sleep request. It does not
  suppress island assertions.

[implementation evidence] Compared with the exact official 0.36.0 package,
0.36.1 changes only published VCS/package-version metadata, its internal lock's
package version, and adds `NarrowPhase::set_query_dispatcher`. The two files
patched above are identical between those official versions, so both existing
lifecycle corrections are retained byte for byte. The root and engine Cargo
path patches must select this 0.36.1 package together; their workspace locks
must be resolved for the actual combined source and features.

[implementation evidence] The earlier isolated 0.36.0 candidate passed all 114
unchanged fnv-phys synthetic fixtures and ten focused query lifecycle controls.
Those earlier results do not qualify this 0.36.1 vendor update or its combination
with the newer interpolation, quarantine, ragdoll, debug-lines and rate consumers.
The combined backend must retain both query/queued-motion and new main behavior
proof sets. Query refresh must not overwrite a queued kinematic target, advance
simulation or interpolation time, lose quarantine reports, or publish an
unfinished collision tree.

[implementation evidence] The combined source at `d66eec9a7780cef13ad1f56e4ca7adbe3fe3c93e`
passes 225 host physics tests across all seven package test binaries, with one
existing diagnostic probe ignored and unexecuted. Rust optimization level 2 keeps
debug assertions and overflow checks enabled. The default island stress retains
all eight seeds and 1,200 steps per seed. The earlier unoptimized stress attempt
hit its time bound and is not counted as a pass.

[design] These checks do not qualify Android gameplay, GPU behavior, the full engine
workspace, phone crashes, or any mod/profile. Those checks remain separate. The vendor's
internal Cargo.lock is the exact published package file; it does not stand in
for normal regeneration of the two workspace locks.
