# Android Pip-Boy presentation size

Clean-room scope: this note describes NVForever's own Android port design, not the original game. Here `[hypothesis]` marks a port design proposal or pending device acceptance, and `[documented]` cites repository source, an owner request, a pinned dependency or public platform documentation; neither is a claim about Fallout: New Vegas (see the [classification](../design/android.md#clean-room-classification-of-the-ports-constants-and-behaviour)).

[hypothesis: NVForever touch presentation policy] A separate saved size controls the
Pip-Boy device and its flat-screen fallback. Android starts at 110%; the bounded
range is 85–135%. Desktop starts at the existing 100%. This preference does not
change general HUD/menu scale, world field of view, inventory identity or actions.

[documented: player_body.rs::place_lens, pipboy_arm.rs::Posed and
pipboy.rs::crop_point] Device input is projected through the actually posed mesh;
flat input uses the actual laid-out image bounds. Apply size to the arm's existing
Pip-Boy lens and to the fallback's visible bounds, rather than multiplying input
coordinates independently. Use the same lens during covered warm poses. The
device's texture allocation remains unchanged.

[hypothesis: input ownership policy] A live preference change cancels pending
virtual item/drag intent before projected picking. A held contact must not become
a fresh click in moved content. Physical mouse/controller state is not cleared.

[documented: pipboy/touch_presentation.rs::cancel_changed] Before the pointer
driver, a presentation stamp compares logical/physical window size, density,
common safe insets, Pip-Boy ratio, menu units and UI scale. Changed or invalid
metadata cancels queued projections and virtual intent before they can release
on old bounds. A first valid stamp preserves fresh input. The actual laid-out
node's geometry check remains an additional guard, rather than the only signal.
Platform inset publication precedes this invalidation. The [R9 hosted CPU run](https://github.com/LootJunkieAzza/NVForever/actions/runs/43669164991/job/132675180379)
passed all nine `pipboy::touch_presentation` cases at source `28d5469`, including
actual flat image/layout/inverse projection, independent settings, metadata/inset
cancellation, invalid-layout recovery and a fresh-start control. This does not
qualify physical device readability or later map integration.

[hypothesis: qualification] Verify independent INI persistence, original 100%
projection, device and flat inverse mappings at multiple sizes/densities, warm
pose agreement, and cancellation without item use. Compilation, source tests and
physical readability are separate; this note does not claim phone acceptance.


## Direct map gestures

[design: Android touch request, issue #2436] Local and world maps admit a single
finger only within the actual painted map window, excluding hints and popups.
Moving past the existing touch slop pans the map. A second map finger locks the
whole contact to pinch/pan; moving their midpoint translates the map while their
separation changes zoom. Release after any pan/pinch never becomes a marker tap.
Zoom keeps the existing 1-to-4 bounds; world pan keeps normalized map bounds and
touch-only local pan stays within the current projected outline frame.

[documented: `pipboy/data.rs`, `pipboy/data/view.rs`, `tiles/map_menu.rs`]
The existing shared MapView is authoritative for the hand-built and native tile
renderers. Touch uses the actual flat crop or the same nearest visible posed
screen ray as the existing pointer, including device content zoom. Raw hardware
buttons and axes are not synthesized or changed. World marker taps request the
existing FastTravelAsk route, preserving legal refusals and confirmation; local
marker/door taps select their existing map highlight and do not invent travel.

[design] A map gesture owns both contact IDs until release/cancel. Context,
projection, focus, controller, platform or competing UI changes discard queued
map edits and require a fresh contact. A third finger or projection failure
cancels the gesture; stale tails cannot become tabs/items/world/fire actions.
Source tests and device acceptance are separate validation boundaries.
