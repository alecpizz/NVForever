# Interior window beams

The first slice of [#4698](https://github.com/LootJunkieAzza/NVForever/issues/4698) identifies
candidate window beams on static shapes in loaded interiors. The existing cards continue
to draw.

## Which shapes qualify

- The placed reference lives in an interior. The shape is unlit, uses additive blending,
  and sets External Emittance. The census matches the drawn shape by its NIF block
  number, including when shape names repeat or are empty.
- Its XEMI resolves to a region whose most likely weather has more sunlight at noon than
  at midnight. Light-backed XEMI, inverted daylight and unresolved sources leave their
  shapes as they are. This is a selection heuristic [design]. The renderer's existing
  emittance path still sets each card's colour. [External emittance and Saloon ray data](../spec-notes/world.md#8-interior-lighting-and-lights)
- The fit takes the common direction of the edges along which the vertex alpha changes.
  The fit uses the renderer's vertex-alpha rule. The alpha-weighted centre must give a
  clear bright end. If either end has a section thinner than a tenth of that end's
  widest side, the fit rejects it as a flat patch. A beam must be at least 1.5 times
  its mean width in length [design].
- A set of cards with separate clusters can describe several shafts. The fit groups
  connected cards by the distance between their centres across the beam, excluding the
  wide cards that cover the whole section. The result is capped at six shafts per shape.
  A single fitted volume is reported when there are fewer than two groups [design].

## Inspecting a cell

In a debug or `dev-aids` build, set `NVF_PLAY_DEBUG_ROOM_BEAM_CENSUS=1` and enter an
interior such as `GSProspectorSaloonInterior`. After a short stretch with no new ray
shapes, the log lists the cell's candidates, shafts and rejection reasons. The debug
log adds the block and fitted position, axis and length of each candidate. The census
does not install systems or read ray meshes while the switch is off. Release builds
without `dev-aids` omit the census.

This slice covers static shapes streamed with the cell. Rigged, posed, morphing and
billboard shapes stay outside the census. Later slices will handle script-drawn rays,
the live settings row and rendering the beams in place of their cards.
