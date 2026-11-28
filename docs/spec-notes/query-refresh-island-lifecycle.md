# Collision query refresh and registered bodies

- [implementation evidence: authored CPU fixture, initial registration study]
  A world with one previously stepped dynamic body, a fixed support, and a newly added
  touching dynamic body reaches actual contact detection with two awake dynamic bodies
  but only one registered active body. The cached release-profile regression fails its
  population assertion; the unchanged-body neighbor passes. No game/device run occurred.
- [documented: pinned rapier3d0.36.0 PhysicsPipeline::step docs and substep.rs]
  A zero-dt pipeline step applies user changes with the real IslandManager and joints,
  performs contact detection, then returns before solver/integration/CCD. CollisionPipeline
  instead handles body changes without island registration. That distinction matters
  once contact notifications use an already populated manager.
- [design: initial candidate, historical policy] The initial candidate used the documented
  zero-dt path with copied IntegrationParameters, actual world fields and existing
  ragdoll hooks, without a dependency patch. Qualification later required the two
  vendor lifecycle corrections below. The final route does not take a positive step,
  use a scratch manager, or suppress island invariants.
- [hypothesis: qualification] Actual pose/velocity/queued-force, sleeping and struck-body,
  jointed and insertion/removal neighbor controls must qualify this change. A cached
  dependency with debug assertions disabled can establish the registration regression
  without claiming the exact debug-only Android panic was executed. Full arrival/load,
  device/game/native behavior, all physics suites and performance remain separate.

- [implementation evidence: authored CPU controls, 2026-11-14] The candidate registers both awake
  dynamic bodies (2/2) where the unchanged baseline registers only one (1/2). Five
  public-API controls and two internal-state controls passed, covering new contacts,
  unchanged neighbors, sleeping clutter, pushed-body motion, joints, queued force,
  configured step size and disable/re-enable. The cached Rapier dependency has debug
  assertions disabled, so this establishes the registration invariant repair without
  claiming the exact Android assertion was executed. The Android integration retains
  the reviewed runtime and all seven authored tests unchanged.

- [documented: selected backend] The reviewed source integration uses the published
  Rapier 0.36.1 package with the existing deferred-BVH join and sleeping-bootstrap
  corrections. Both official 0.36.0/0.36.1 files receiving those fixes are identical;
  the vendor keeps the same two changes. The 0.36.1 package adds a narrow-phase
  query-dispatcher setter and changes package/VCS metadata. This source comparison
  does not qualify the combined backend; see `vendor/rapier3d/README.nvforever.md`.
- [design: quarantine composition] Every pipeline call, including zero-dt registration,
  clears Rapier's previous quarantine report. Query registration therefore consumes
  each report immediately through the existing wrapper remedies, before another
  pipeline call. A remedy that re-enables a finite body or rebuilds a resting ragdoll
  is published by one further zero-dt pass before queries are marked clean. The
  inspected closed remedies need at most these two passes; invalid rebuilt geometry
  stays disabled. An unexpected exhausted bound logs and retains dirty flags for a
  later query, rather than claiming that partially published state is clean.
- [design: time and bookkeeping] Registration does not capture interpolation poses or
  advance steps, stepped seconds or configured dt. A true step retires its consumed
  queues before applying quarantine remedies, retaining their new dirty marks and
  rebuilt bodies for the next query. Reports remain one-time owner reports, and
  drawing forgets quarantined poses through the existing handler.
- [implementation evidence] The actual host wrapper/pipeline/query baseline passes
  its normal queued-motion control and fails five same-query remedy/report and
  positive-step bookkeeping assertions. The unchanged six controls then pass with
  the bounded correction. A separate post-fix finite kinematic `Enabled` control
  passes; no pre-fix result is claimed for that additional case. The combined source
  at `d66eec9a7780cef13ad1f56e4ca7adbe3fe3c93e` passes 225 physics tests across seven
  binaries, including the three unchanged queued-motion contracts, main's drawing,
  ragdoll and default island stress checks. One existing diagnostic probe remains
  ignored/unexecuted. Optimization level 2 retains debug assertions and overflow
  checks; no seed, step count or assertion was reduced.
- [implementation status] Full engine/wrapper execution, strict hosted checks,
  source-identical native packaging and device/game acceptance remain separate.
  These CPU results do not establish an Android crash cause or cure.
