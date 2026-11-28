# Split conversation view

Clean-room scope: this note describes NVForever's own Android port design, not the original game. Here `[hypothesis]` marks a port design proposal or pending device acceptance, and `[documented]` cites repository source, an owner request, a pinned dependency or public platform documentation; neither is a claim about Fallout: New Vegas (see the [classification](../design/android.md#clean-room-classification-of-the-ports-constants-and-behaviour)).

- [documented] The owner's `split-dialogue` request requires selectable left/right
  speaker view and opposite readable choices, actual actor framing, safe areas and
  clean conversation/companion transitions. Standard remains selectable.
- [documented] Existing `dialogue_look::face_point` uses the current conversation's
  drawn head, then actor volume, then placed reference. Split framing reuses that
  identity and fallback order, and uses the activator for conversations through one.
- [documented] `control::place_camera` writes the ordinary player camera. The split
  camera lease runs after it and `dialogue_zoom::apply`, changes only render viewport
  and rotation, and restores only values it still owns. Player heading, script rules,
  choice eligibility, subtitles and saves remain governed by their existing systems.
- [documented] Bevy0.20.1 `bevy_core_pipeline/src/upscaling/node.rs` obtains the output
  attachment before beginning its render pass and sets viewport scissor afterwards.
  `engine/vendor/bevy_render/src/texture/texture_attachment.rs::OutputColorAttachment`
  selects an attachment clear/load operation. A pre-world UI pass therefore cannot
  rely on a later world viewport to protect its already drawn pixels from clear.
- [documented] The dialogue-only 2D camera draws after the world with transparent
  intermediate clear and an explicit alpha-blended, no-clear output mode. Only the
  dialogue root has its explicit `UiTargetCamera`. `DefaultUiCamera::get` chooses the
  marked camera or highest-order window camera, so an owned temporary marker preserves
  the exact previous default when no foreign marker exists. Foreign markers are not
  replaced. On exit the overlay and only the owned marker retire.
- [documented] The platform's atomic snapshot carries version, physical window size
  and four physical insets. The shared `touch_safe_area::SafeInsets` rejects malformed
  or stale-window-size snapshots, converts using the actual Window scale factor and
  clamps untrusted edges. Dialogue adds logical margins and an eight-pixel split gap.
- [hypothesis] Alpha composition, speaker comfort and cutout behavior still need
  actual renderer/device acceptance. Headless layout/input/camera tests do not claim
  GPU execution or physical acceptance.
