# Selectable original touch icon families

[design, #2436] A follow-up atop the reviewed density-correct SVG renderer adds four
original art families, selected independently of touch appearance, placement and bindings:
**Precise line**, **Bold filled**, **Rounded** and **Industrial**. Stable semantic IDs and
legacy input indices remain identical across families. Companion and Aid add art identities
for future controls; they do not add game actions or mappings in this increment.

[design] These are different drawings and negative spaces, not stroke/palette variants.
Jump uses a raised boot/leg and upward cue rather than a star-like stick person. Sneak is
a low crouch under an overhead boundary, separate from Sprint's higher running silhouette.
Quickwheel uses detached radial menu sectors with an empty centre, avoiding a steering
wheel. VATS frames a selected person, separate from Aim's circular reticle. Fire keeps a
projectile/burst reading. All artwork is original GPL-3.0-or-later; no game/brand artwork.

## Bounded explicit fill profile

[documented: SVG 2 painting](https://www.w3.org/TR/SVG2/painting.html)
Fill paints the interior of a shape, and an even-odd fill rule can represent holes.

[design] The owned profile v2 explicitly validates each family's fill/stroke vocabulary
and shared 32-unit viewBox. It permits currentColor/none fill and stroke, an even-odd
fill rule, bounded stroke width, round/square caps and round/bevel joins. Primitive
fill/stroke overrides use only those colours. No CSS, transforms, filters, external
references or runtime XML parsing are added. The compiler emits typed geometry with
explicit paint flags; tiny-skia fills with EvenOdd and strokes only when requested.
Painted bounds account for the family's stroke and square/bevel diagonal extension.

## Selection, cache and persistence

[design] Android Controls > Touch contains a Touch icon family list. The normal settings
routes persist a stable family ID; unknown values leave the current family unchanged.
The selection resource contains no tint, shape, placements, control IDs or bindings. It
is initialized to Precise line; desktop touch remains disabled as before. Tests can
register the row without enabling Android runtime.

[design] The raster key includes family ID/index alongside icon identity and physical
size. Switching family replaces the same live Image handles at actual content-box
physical dimensions, preserving tint and the reviewed Stretch + ContentBox/asset-event
ordering and cleanup policy. No historical-family cache accumulates. New family choice
never resizes a hit target or changes an action.

## Qualification

[design] Contact sheets show original art at actual 24, 32 and 48 pixel sizes on light,
dark and a synthetic complex backdrop. The backdrop contains no game data. Compiler tests
cover family identity/semantic alignment, explicit fills, holes, profile and painted
bounds, malformed SVG, deterministic generation and preview sizes. Source-authored Rust
tests cover real settings persistence, unknown family values and same-size family cache
replacement with stable handles/tint. Native tests, GPU/phone drawing and final visual
comfort remain pending; this slice does not block the existing crash-fix candidate.

[design] Block is a separate stable shield/guard identity at index25 in every family. Aim
remains index6 and retains its identity. Adaptive Aim/Block presentation can switch artwork
and caption at the same action position without changing the Control::Block binding;
that capability-aware presentation belongs to the separate action-presentation increment.
