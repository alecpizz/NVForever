# Independent touch appearance extension

[design] This is the next appearance-settings contract, separate from the SVG renderer
and the current placement editor. No presets or appearance persistence are implemented by
the SVG source increment. Original SVG masks and their physical-size cache are the usable
extension point; visual feedback still precedes final artwork and layout choices.

## Independent state

[design] Keep three stores with separate versioning and reset/apply commands:

| Store | Owns | Applying a preset changes |
| --- | --- | --- |
| Layout | Control centre, hit target diameter, visibility, anchors | Positions and target sizes only |
| Bindings | Action mapped to each control, input scheme | Mappings only |
| Appearance v1 | Shapes, fills, outlines, icon styling, labels | Visual styling only |

[design] Appearance uses stable semantic IDs from `assets/touch/manifest.json`, never
array positions as persisted identities. Unknown control IDs survive a load/save round
trip so another control scheme can restore its overrides. Reject unknown schema versions
without rewriting the source file. Validate a complete candidate before atomically
replacing current appearance; a malformed override must not erase working settings.

## Proposed appearance v1 schema

[design] An independent JSON document has `schema: 1`, `preset` (an original generic ID),
`global` (a complete style below), `controls` (stable ID to partial style override),
`labels` and `previewHitAreas` booleans, `contrast` (`normal` or `high`), and
`reducedDecoration` boolean. Persist colour channels as straight-alpha sRGB arrays of four
finite numbers in [0,1]. Reject duplicate JSON keys, unknown style fields, nonfinite or
out-of-range numbers. A partial control override inherits missing fields from global;
resetting an override removes only that override. The editor exposes both scopes clearly.

| Style field | Type and bounds | Responsibility |
| --- | --- | --- |
| `shape` | `circle`, `rounded-square`, `diamond` | Visible backing only; hit geometry remains in Layout |
| `fill` | RGBA | Idle backing colour |
| `outline` | RGBA | Boundary colour |
| `outlineWidth` | Finite logical pixels in [0,6] | Backing stroke; never changes target diameter |
| `iconTint` | RGBA | Multiplies the white SVG coverage mask |
| `iconScale` | Finite ratio in [0.35,0.9] | Child image box relative to target; regenerates raster at its resulting physical size |
| `idleOpacity` | Finite number in [0,1] | Multiplies backing/outline/icon alpha while idle |
| `pressedOpacity` | Finite number in [0,1] | Corresponding pressed value |

[design] Example (illustrative values, not shipped defaults):

```json
{
  "schema": 1,
  "preset": "clear-outline",
  "global": {
    "shape": "circle",
    "fill": [0.02, 0.06, 0.07, 0.25],
    "outline": [0.9, 1.0, 0.97, 1.0],
    "outlineWidth": 1.5,
    "iconTint": [1.0, 1.0, 1.0, 1.0],
    "iconScale": 0.66,
    "idleOpacity": 0.7,
    "pressedOpacity": 1.0
  },
  "controls": {"fire": {"shape": "rounded-square", "iconScale": 0.75}},
  "labels": false,
  "previewHitAreas": false,
  "contrast": "normal",
  "reducedDecoration": false
}
```

## Original generic preset and rendering policy

[design] Candidate appearance names are **Clear outline**, **Soft panels** and
**High contrast**. Candidate layout names are **Compact**, **Wide reach** and
**Left hand**. They contain no game or brand references and must use original artwork.
Selecting appearance must leave layout and bindings byte-for-byte unchanged; selecting a
layout must leave appearance and bindings unchanged. An explicit combined setup can apply
several stores only when its UI names the stores it changes. Later schemes can use the
same visual style without storing action identity in the backing geometry.

[design] Appearance resolves global + selected control override into a style, then a
separate visual system sets backing shape/fill/border, label visibility and
`ImageNode.color = iconTint * stateOpacity`. Keep the SVG and cached texture white;
colour/opacity changes cause no rasterization. `iconScale` changes the child Node box,
which the SVG cache observes after layout. Shape changes alter the backing without
squashing the icon. The hit-area preview is an editor overlay of actual Layout hit
geometry, not a decorative backing. Optional labels use the normal text system.

[design] High contrast supplies a visible backing and explicit boundary so icon colour
alone is not the feedback cue. Reduced decoration removes glows, extra rings and motion;
pressed state keeps a boundary/fill change. Do not claim measured accessibility contrast
until tested against the composited background. Labels, opacity, contrast and decoration
remain explicit user choices, with previews before saving.

## Required tests before implementation ships

[design] Schema round trips and migration/refusal; partial inheritance; invalid numbers,
colours and duplicate keys; stable IDs across schemes; reset one override vs global;
appearance/layout/binding independence; idle-to-pressed-to-release transitions;
transparent/low-opacity and high-contrast previews; reduced decoration; labels and actual
hit-area overlay; tint-only stable image handles; icon-scale/DPI/resize replacement;
and rendering of every supported backing shape on a phone. No game data belongs in art
fixtures. These are pending appearance work, not validation claimed by the SVG packet.

[design] The selectable icon-family resource is independent of Appearance v1. A family
chooses original glyph geometry; appearance supplies tint/scale/opacity/backing. Applying
a family must leave every appearance override, placement and binding unchanged.
