# Typed held UI projection requests

[hypothesis] ProjectUITile is called on a reference with two strings and four rectangle numbers, and an exact 1x1 rectangle withdraws an active projection. Process evidence: historical public-command metadata labels it JIP/reference-only with numeric return and opcode 0x2925; this is not a new native measurement. No installed source or game text is transcribed into these authored fixtures.

[hypothesis] Correct the existing unnumbered source-command catalogue row in place and ordinary ExtCall routing. The existing `schemas/script/command_opcodes.json` mapping already names ProjectUITile as 10533 (`0x2925`); this contribution leaves that entire map unchanged. A precompiled call decoded through that mapping may reach the typed handler, but the native parameter-frame adapter and its compatibility remain unqualified. No opcode or native parameter descriptor is added. The run-on reference is required at runtime. Accepted/withdrawn calls return the engine's neutral numeric zero; native return parity is unmeasured.

[hypothesis] Store Requests inside existing unsaved UiExtra. Existing Default construction and tree_rebuilt clearing require no Game.rs field/constructor/snapshot changes. Bind each accepted request to the complete current Ui identity/source TileId, target reference/block and checked revision token. A source reload/rename is unavailable until explicit registration, never an all-HUD fallback. Identical repeated calls keep the revision, changed requests get a fresh one; counter exhaustion refuses changes without token reuse.

[hypothesis] Policy limits are 16 requests, 2048 pixels per edge, 8 Mi aggregate requested pixels, 4096-byte source paths, 512-byte blocks and finite viewport offsets of magnitude <=1e7. Exact 1x1 withdraws; other tiny viewports are not guessed to cancel. Fractional positive dimensions round up only after validation. No native size/rounding guarantees are claimed. Invalid same-session requests leave existing entries intact. On a different actual UI identity old entries are conservatively discarded before new admission.

[hypothesis] Command acceptance is not renderer readiness or target binding. The source_requests iterator checks source identity only; physical freshness, visibility/isolation, camera output, pixels and model binding are separate consumers. Ordinary UI drawing is unchanged until a separate projection consumer is integrated. Authored controls cover registry admission and actual VM/tree-reset behavior. Full load/newGame cases remain separate; no native DLL/modpack compatibility claim.

[hypothesis: source reconciliation policy] A separate actual pre-draw reconcile permanently retires observed stale/index-shadowed sources; restoring an old path alone cannot resurrect a token. Admission also prunes stale entries before count/pixel quotas. This does not detect a transient path change that no consumer ever observes. The registry and VM fixtures exercise sticky retirement and quota admission.

[hypothesis: catalogue correction] Replace only the existing ProjectUITile placeholder parameters with the observed source-caller shape: two strings and four numbers, with a required runtime reference. Retain its existing int return descriptor, page, categories, origin, family and position; do not append a duplicate. The former six-float row routed to a generic argument-ignoring zero-effect stub, not an implemented six-number contract. Changing that source behavior is explicit; other mods/precompiled calls and native returns remain unmeasured. FunctionTable unnumbered metadata omits return/reference fields, so source signature lookup plus real Game/VM neutral-zero/request/withdrawal behavior need a new control. Prior controls executed with a duplicated catalogue do not qualify this corrected input. The twelve original controls and an additional catalogue/VM control cover the effective source-command signature, neutral-zero acceptance, exact request state, withdrawal and rejection of the former six-numeric stub shape. No native 0x2925 decoder/parameter adapter is added.
## Implementation coverage and historical validation

Repository/process evidence: public source at
`cd7ff8230021bc5a3c5446438b379107eded4c78`,
`crates/fnv-game/src/ui_projection.rs` and
`crates/fnv-game/tests/ui_more.rs`. Thirteen authored controls cover ten
registry cases and three actual VM/tree/catalogue cases. They exercise bounded
admission, revision reuse and exhaustion, source/UI identities, sticky observed
path retirement, withdrawal, required runtime reference and the effective
FunctionTable signature. They do not exercise native parameter decoding,
renderer output or material binding.

Repository/process evidence: historical public CI
[job 132660356874](https://github.com/LootJunkieAzza/NVForever/actions/runs/43664352646/job/132660356874).
That job checked out merge `e64be551fff659c173405e51ad924d5bb4413877`,
combining source head `cd7ff8230021bc5a3c5446438b379107eded4c78` with
main `78408bcba623baaabbab4bb77a0e19d3a5e4fd72`. All thirteen selected
controls passed once. Its complete core summaries reported 3032 passes, zero
failures and four ignored cases. This is pinned repository/process evidence,
not an original-game observation or a claim that a later head was tested.
A subsequent documentation or source head has separate exact-head CI and
review gates. Full load/newGame, native `0x2925`, renderer, physical input,
model-material binding and installed held/local-map acceptance remain separate.

## Regeneration and deliberate refusal policy

[hypothesis: source-contract policy] `tools/capture/script_functions.py` owns the
ProjectUITile pageless row: two strings, four floats, integer return and reference
metadata. `schemas/script/notes.json` carries the same summary as the catalogue.
An authored empty-wiki/empty-Data CLI regression generates the complete row and
compares it with the one actual catalogue row consumed by FunctionTable and the
existing source-to-Game/VM request control. These two controls form the generator
to consumer chain; a passing Python regeneration alone is not VM execution.

[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
`GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated staging copy,
read-only, committed probe `w4g_10_project_ui_tile_errors` in
`tools/capture/nvse/catalogs/wave4g_f2_extenders.json` (companion catalog PR #3803);
no DLL or source consulted] Across all seven tested calls — valid `64x64`, `1x1`
withdrawal, missing tile `"NoSuchMenu/NoSuchTile"`, `0x0`, negative `-10x-10`,
fractional `64.7x64.7`, and null-ref `rNull = 0` (which logs a NULL-ref warning) —
`ProjectUITile` returns `0.0` and continues script execution (`ok=1`), superseding the
earlier script-aborting `Err` hypothesis. Verbatim printed probe output (2 lines):
- `valid(ok=1,r=0.0)|w1x1(ok=1,r=0.0)|miss(ok=1,r=0.0)|zero(ok=1,r=0.0)`
- `neg(ok=1,r=0.0)|frac(ok=1,r=0.0)|null(ok=1,r=0.0)`
In NVForever, `ui_projection::Requests::set` still validates inputs and rejects
invalid/missing/over-quota requests without enqueueing them into the renderer, while
`ui_projection::call` and the VM null-receiver path return `Ok(0.0)` so calling scripts
continue; mismatched non-source argument shapes (such as the retired six-float stub
signature) still return `Err` at the host boundary [design]. Historical six-number
probes and lab expectations remain unchanged.

Process qualification: historical CI belongs to its pinned checkout. Every later
head requires its own applicable tests, lint, audits and review; the generator
regression and existing real FunctionTable/VM control must both execute. This note
records coverage rather than claiming unexecuted current-head results.

## Off-screen projected painter and 3D model surface consumer

[hypothesis] / [design] **Provenance clarification (`#2418`):** There is no published GECK
Wiki / JIP LN documentation page for `ProjectUITile` and no named public mod script +
version cited here documenting that `ProjectUITile` renders an isolated XML subtree onto
the named block of a reference's model or that an exact `1x1` rectangle withdraws it.
Inspection of an earlier local NVForever prototype is not an `[observed]` fact about the
original JIP LN `ProjectUITile` command; accordingly, both behaviors (drawing an isolated
XML subtree onto the named block of the reference's model, and treating an exact `1x1`
rectangle as withdrawal) are explicitly tagged `[hypothesis]` / `[design]` (and the
corresponding line in `#2418` is retagged from `[observed]` to `[design]`/`[hypothesis]`).

[design] `nvf_play::tiles::projected_ui` (`ProjectedUiTargets`) now consumes
`game.ui_extra.projections`: `projected_ui::reconcile` runs before `PlaySet::Ui` to retire
stale/shadowed sources, and `projected_ui::draw` paints each admitted subtree in isolation
(`layout::layout_isolated`, viewport shift and clip intersection) into a per-target
`Camera2d` + `RenderTarget::Image` (`order = -3`) via `tiles::draw::paint` (reusing
`runtime_images::mark` and `runtime_images::present`). `nvforever::ui_project`
(`UiProjectPlugin`) binds each published texture onto matching `(reference, block)` mesh
surfaces in `PostUpdate` after `nif_mirror::MirrorSystems` using an owned unlit
`NifMaterial` copy with ownership-conditional restoration. Native `0x2925` parameter-frame
decoding and live GPU held-map captures remain separate follow-up gates.
