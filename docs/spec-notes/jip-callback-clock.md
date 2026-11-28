# JIP main-loop callback elapsed time

[documented: GECK Wiki, [SetGameMainLoopCallback](https://geckwiki.com/index.php/SetGameMainLoopCallback)]
A registration invokes a zero-argument function on its selected reference, with
the player as the omitted reference. Its delay is in frames; flags select game
or menu execution, and flag 8 removes the registration on loading.

[documented: GECK Wiki, [SetAnimCurrentTime](https://geckwiki.com/index.php/SetAnimCurrentTime)
and [SetGlobalAnimTime](https://geckwiki.com/index.php/SetGlobalAnimTime)]
The examples use GetSecondsPassed inside a game-mode main-loop callback to
advance animation time. They do not specify the first interval, delayed-call
intervals, nested-call inheritance, or session-transfer timing. These citations
were read by the source author on 2026-11-14; no native implementation was used.

[documented: pinned public source at
[576d3963 Game](https://github.com/LootJunkieAzza/NVForever/blob/576d39630287845c598914d17ddffd71eebbef28/crates/fnv-game/src/game.rs),
[JIP dispatch](https://github.com/LootJunkieAzza/NVForever/blob/576d39630287845c598914d17ddffd71eebbef28/crates/fnv-game/src/jip_events.rs),
and [clock functions](https://github.com/LootJunkieAzza/NVForever/blob/576d39630287845c598914d17ddffd71eebbef28/crates/fnv-game/src/functions/misc.rs)]
Compiled function invocations use temporary locals and have no persistent script
instance. GetSecondsPassed returns zero without such an instance, and main-loop
dispatch supplies no callback-specific clock. The existing game clock is
quantized to 1/64 second and pauses during menus/loading. Ordinary instance and
trigger clocks are separate consumers and retain their existing behavior.

[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
GSDocMitchellHouse (`0x00103DF9`), 10 official base masters, isolated staging copy
(read-only), committed probe `w4g_03_jip_callback_getsecondspassed` in
`tools/capture/nvse/catalogs/wave4g_f2_extenders.json` (companion catalog PR #3803);
no DLL or source consulted] Both `SetGameMainLoopCallback` (at `callDelay=1`, `5`, and
`10`) and ordinary stateless UDFs (`CallAfterSeconds 0.05`) return the **single-frame**
delta from `GetSecondsPassed`, NOT accumulated time across skipped frames and NOT `0.0`.
Repeated reads (`r1 == r2`) and nested `Call`s (`nest == r1`) share the exact same
delta within a tick. Verbatim probe output:
- `PROBE|w4g_03_jip_callback_getsecondspassed|harnessLoop(r1=0.010000,r2=0.010000,nest=0.010000)`
- `PROBE|w4g_03_jip_callback_getsecondspassed|cb_d1(tick=0,r1=0.015000,r2=0.015000,nest=0.015000)`
- `PROBE|w4g_03_jip_callback_getsecondspassed|cb_d1(tick=1,r1=0.016000,r2=0.016000,nest=0.016000)`
- `PROBE|w4g_03_jip_callback_getsecondspassed|cb_d1(tick=2,r1=0.010000,r2=0.010000,nest=0.010000)`
- `PROBE|w4g_03_jip_callback_getsecondspassed|callAfterSec(r1=0.010000,r2=0.010000,nest=0.010000)`
- `PROBE|w4g_03_jip_callback_getsecondspassed|cb_d5(tick=0,r1=0.010000,r2=0.010000,nest=0.010000)`
- `PROBE|w4g_03_jip_callback_getsecondspassed|cb_d5(tick=1,r1=0.016000,r2=0.016000,nest=0.016000)`
- `PROBE|w4g_03_jip_callback_getsecondspassed|cb_d5(tick=2,r1=0.016000,r2=0.016000,nest=0.016000)`

[design] NVForever supplies `Game::stateless_udf_seconds()` (`0.0` while paused in
menus, loading, or `FreezeTime`, otherwise the current frame step `cfg.frame_seconds`)
to `SetGameMainLoopCallback`, `run_function` (including `CallAfterSeconds` and nested
`Call`), and `run_query`, captured at UDF entry so repeated reads and nested `Call`s
within the same execution see the identical value. Only `GetSecondsPassed` without a
persistent instance uses it; persistent script instances (`ctx.inst.is_some()`) and
result scripts (`run_source`, which pass `seconds: None`) keep their existing paths.
No global override, serialized clock field, opcode change, actor-value special case or
script-name matching is introduced.

[documented: authored control source in
`crates/fnv-game/tests/common/jip_callback_clock.rs`, registered by the existing
`synthetic` integration target] Six controls exercise compiled OnHit callbacks
and Game::tick; independent subjects/delays and nested reads; paused menus;
explicit removal and re-registration; flag-8 cleanup/session rebasing; and
ordinary/result/instance neighbors. The fixture explicitly binds SGMLC's
receiver. It does not infer a receiver from the query's RunCtx alone. All public
576 compilation and execution remain unqualified until separately reported.

[hypothesis: bounded compatibility policy] JohnnyGuitar's RenderUpdate handlers
(SetJohnnyOnRenderUpdateEventHandler) run every frame the same way, as functions
without an instance: each dispatch hands them, and their nested calls, the
quantized game-clock time since the previous dispatch (0 the first time, and when
the clock went back). B42 True Leaning eases its lean by `GetSecondsPassed` in
such a handler; with 0 it never moved [observed: mods_reference
`ln_B42Leaning.txt`, its lerp variables stayed 0 with the key held].

[hypothesis: limits] This leaf does not repair previously saved modifiers or
establish native timing, automatic engine event coverage, whole-mod support,
animation, audio, renderer or device behavior.
