# Visible touch icon cache ownership

[design, #2436] This small follow-up applies after the frozen selectable-family packet
935933efa9403047bbc122d73a71ad5e447d3af6d796ff8095e75193eb517398. It does not replace
that snapshot, change artwork/families, alter bindings or change physical resolution.

[documented: Bevy 0.20.1 local `bevy_ui/src/ui_node.rs` (`Node`),
`bevy_camera/src/visibility/mod.rs` (`InheritedVisibility`, `VisibilitySystems`)]
Visibility::Hidden preserves layout; a nonzero ComputedNode is not evidence that a glyph
is visible. InheritedVisibility is propagated through the visibility hierarchy in
PostUpdate VisibilitySystems::VisibilityPropagate. Display::None collapses UI layout.

[design] Cache updates now follow both UI layout and visibility propagation, while still
preceding AssetEventSystems. A node is eligible only when its propagated hierarchy
visibility is true and its content box has positive extent. No cache entry or texture is
created for a hidden or collapsed node. The retirement pass removes every now-hidden or
removed owned entry before any visible allocation, independent of entity/query order.
Thus hidden gameplay/menu/controller groups cannot consume the 64-entry/8 MiB budget or
prevent a visible group receiving its full requested physical resolution. There is no
resolution clamp or small scaled fallback.

[design] Retirement keeps the reviewed handle policy: reset a surviving node only if it
still owns the cached image, remove the CPU asset and drop the final cache handle so
Bevy can emit Unused and retire GPU state. A newly shown node gets a fresh raster at the
current size/family; tint and placement remain untouched. Same-size visible frames still
reuse the existing handle and pixels. Visibility policy here follows hierarchy visibility
and layout, without adding per-frame game-state guesses.

[design] The new source-authored regression uses Bevy's actual VisibilityPlugin and asset
schedule. More than 64 hidden 512px glyphs coexist with eight visible 512px glyphs at the
8 MiB budget. It hides the old visible hierarchy while adding the next group, checks all
new visible nodes receive images and old images issue Unused, then simulates controller
Visibility-only hiding, re-showing and a collapsed box. Existing headless raster fixtures
explicitly publish inherited visibility alongside their mocked computed geometry. Any
combined adaptive fixture that mocks physical size must also publish visibility or run
VisibilityPlugin; its frozen source is not edited here.

[design] Eight icon tests (the prior seven plus this regression) remain UNRUN. Rustfmt
syntax/format checking, whitespace checks and patch parsing do not establish native, GPU
or device behavior. Compilation/clippy, actual hierarchy/raster tests and phone mode
switching still need admission; this follow-up is not a gate on the crash-fix APK.
