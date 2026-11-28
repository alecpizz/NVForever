# Density-correct Android touch SVG icons

[design] This records the reviewed first 23-icon/profile-v1 increment. The
[selectable family follow-up](android-icon-families.md) explicitly extends the compiler to
bounded fills/profile-v2 and 26 semantic IDs; its renderer retains the sizing, ordering
and ownership corrections described here.

[design] This source-only follow-up replaces fixed-size generated bitmaps with original,
inspectable SVG artwork. It does not lock the touch layout or change the installed app.

## Artwork and provenance

[design] The action set uses a shared 32-unit viewBox, 1.8-unit white strokes, round caps
and joins. The manifest names each action and its legacy input index. Fire, circular aim,
bracketed VATS, return/back and left navigation have deliberately distinct silhouettes.
A generic wrist display represents the Pip-Boy action; no game artwork or logo is copied.
All artwork is original NVForever work under GPL-3.0-or-later; the asset README records it.

## Small SVG profile, compile-time consumption

[documented: SVG 2 paths](https://www.w3.org/TR/SVG2/paths.html)
SVG path commands define moves, lines, quadratic/cubic curves and closed contours.

[design] The owned `nvf-touch-svg/1` profile supports explicit absolute M/L/Q/C/Z path
commands, circles, lines, polylines and polygons. Styling is one shared stroke; no fills,
transforms, CSS, text rendering, filters, images, references, entities or scripting are
accepted. Unsupported input fails validation instead of being drawn incorrectly. A bounded
Python standard-library tool (`tools/android/svg_icons.py`) validates the XML/manifest and
emits typed geometry into the committed `engine/nvforever/src/touch/icons_generated.rs`
file when run by hand after editing SVGs. `engine/nvforever/build.rs` does not invoke
Python during Cargo builds; `tools/android/tests/test_svg_icons.py` verifies the committed
Rust file stays byte-for-byte in sync with the SVG sources. Limits cover source size,
count, command count, coordinates and painted bounds.

[design] This avoids a general-purpose runtime SVG parser and resvg/usvg dependency graph.
Rasterization reuses tiny-skia 0.11.4 (BSD-3-Clause), already present in engine/Cargo.lock.
Only a direct dependency is requested. Root must approve it and regenerate Cargo.lock with
Cargo; the lock is not edited by hand. Existing Python 3 build tooling is reused.

## Physical sizing and lifetime

[documented: Bevy ComputedNode](https://docs.rs/bevy_ui/latest/bevy_ui/struct.ComputedNode.html)
Computed node sizes are physical pixels after the PostUpdate UI layout stage; multiplying
by display density again would double-scale them.

[documented: tiny-skia 0.11.4 source, Pixmap::stroke_path/PathBuilder/Transform]
Paths can be stroked with antialiasing, round caps/joins and a geometric transform.

[design] After layout, each icon is rasterized to the exact integer physical dimensions
of its node (fractional dimensions round up), with square viewBox content letterboxed
uniformly for a non-square node. DPI, editor size and window changes replace that node's
existing Image asset. Position, opacity and colour changes reuse it. There is no fixed64
texture scaled upward. White alpha masks are converted from premultiplied raster coverage
to straight-alpha white pixels before Bevy tinting, avoiding dark fringes.

[design] Cache entries belong to live icon entities, not every historical size. Same-size
frames reuse handles and raster pixels; resize replaces one asset. Despawn removes the
entry/asset. Explicit entry, dimension and byte budgets reject impossible/oversized input;
there is no silent resolution clamp followed by upscaling. A rejected icon is transparent
with one diagnostic rather than an unbounded allocation or a blurred fallback. The budget
also accounts for replacement sizes; transient rasterization has a separate bounded size.

## Qualification

[design] Cheap compiler tests cover malformed XML, unsupported features, unsafe manifests,
nonfinite/out-of-bounds geometry, style consistency and distinct owned silhouettes. Rust
source tests cover physical/DPI sizing, resize replacement, malformed size refusal,
straight-alpha conversion and cache budgets. The preview renders only owned artwork and
is not a game capture. Rust type checking, actual raster tests, Android drawing/rotation
and GPU uploads remain pending root admission. Visual feedback precedes final art/layout.

## Appearance boundary

[design] The cache generates white coverage and leaves `ImageNode.color` untouched.
Global/per-action tint and opacity can therefore change without regenerating or colouring
SVG sources. Icon scale changes the child Node dimensions and requests a new raster.
Backing shape/fill/outline, labels, contrast and hit-area previews belong to the independent
[appearance v1 proposal](android-touch-appearance.md); it preserves layout and bindings.
Selectable original generic appearances/layouts remain a separate follow-up.

## Source packet limits and build impact

[design] The generated icon geometry is committed at
`engine/nvforever/src/touch/icons_generated.rs` and included directly by
`engine/nvforever/src/touch/icons.rs`, so desktop and Android Cargo builds require no
Python (`engine/nvforever/build.rs` is unchanged vs `main`). Run
`python tools/android/svg_icons.py --out engine/nvforever/src/touch/icons_generated.rs`
by hand whenever SVGs under `engine/nvforever/assets/touch/` change. Runtime touch
activation remains Android-only; normal desktop input/UI behaviour is unchanged. Generated
geometry is embedded, so no runtime SVG files need Android asset staging.

[design] The cache bounds are 64 live icon nodes, 512 physical pixels per dimension and
8 MiB of current CPU image pixels, plus at most one 1 MiB raster allocation during a
replacement. Renderer/GPU staging and preceding-frame textures are owned by Bevy and are
not counted as current CPU cache pixels. Capacity failures are transparent; diagnostics
are deduplicated until request or available cache bytes change. Freed budget allows the
same requested size to retry. Hidden/pre-layout zero boxes release their pixels silently.

[documented: Bevy 0.20.1 local source `bevy_ui/src/ui_node.rs`,
`bevy_ui_render/src/lib.rs::extract_uinode_images`] ImageNode's default visual box is the
content box, which subtracts border and padding from the physical node rectangle. The
renderer reads `ComputedNode::content_box().size()` to match it, including future inset
changes. The raster is never sized from logical coordinates or a second DPI multiplier.

[design] Reproduce cheap checks with
`python -m unittest discover -s tools/android/tests -p test_svg_icons.py` and
`python tools/android/svg_icons.py --preview <private-directory>/contact-sheet.svg`.
The vector sheet includes each owned action at 24 and 64 physical pixels. Preview PNGs
are review-only and are not tracked or captured from gameplay. Engine tests, compilation,
clippy, Cargo lock generation/deny, GPU extraction ordering and phone rotation/DPI remain
pending admission; a Python pass is not evidence that the Rust renderer has run.


## Same-frame extraction and retirement

[documented: Bevy 0.20.1 source `bevy_asset/src/lib.rs` (`AssetEventSystems`),
`bevy_render/src/render_asset.rs::extract_render_asset`,
`bevy_ui_render/src/lib.rs::extract_uinode_images`] Asset events drive GPU asset updates;
GPU retirement uses `Unused`, and ImageNode Auto sizing reads a cached image aspect ratio.

[design] Raster updates run after `UiSystems::Layout` and explicitly before
`AssetEventSystems`, so new and same-handle resized images publish their events before
render extraction. Owned image nodes use Stretch + ContentBox: the texture itself already
letterboxes the square art, so extraction must use the current whole box and must not
consult the preceding frame's cached Auto aspect ratio. Tint is still independent.
Retiring a cache entry resets a surviving ImageNode to transparent only when it references
that owned handle; then it removes the CPU asset and drops the cache handle. Dropping both
strong references allows Bevy to issue Unused and retire GPU state. Another system's
replacement image is preserved.

[design] The five authored Rust tests now use the real headless AssetPlugin schedule.
The resize test writes a new physical box in the Layout set and checks same-frame Added
and Modified events, an aspect-changing Stretch mode and unchanged tint-only pixel
storage. Cleanup checks marker and ComputedNode removal, transparent surviving nodes,
subsequent Unused events and preservation of an externally replaced image. These tests
remain UNRUN; source ordering review is not GPU or native qualification.
