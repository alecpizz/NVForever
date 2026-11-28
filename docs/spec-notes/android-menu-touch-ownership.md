# Mobile menu touch ownership

Clean-room scope: this note describes NVForever's own Android port design, not the original game. Here `[hypothesis]` marks a port design proposal or pending device acceptance, and `[documented]` cites repository source, an owner request, a pinned dependency or public platform documentation; neither is a claim about Fallout: New Vegas (see the [classification](../design/android.md#clean-room-classification-of-the-ports-constants-and-behaviour)).

- [documented] `nvforever/src/touch.rs::input` is the wrapper's sole raw touch
  reader. Existing touch actions change aggregate `ButtonInput`; controller menu
  translation in `nvf-play/src/pad.rs::menu_keys` also changes that aggregate
  without emitting `KeyboardInput`. Touch look emits `MouseMotion`, so that
  message cannot establish physical mouse ownership.
- [hypothesis] Keep one wrapper hardware observation before touch synthesis.
  Raw keyboard presses, mouse-button presses, nonzero mouse wheel and actual
  window `CursorMoved` activity can yield the alternate touch sheet to hardware.
  Do not clear hardware state or treat synthesized look/menu actions as hardware.
- [hypothesis] The pre-input reader queues at most 128 sheet edges and retains
  at most 32 owned IDs. Replay occurs after the actual input selector and platform
  hold sampler, before UI focus/picking. Overflow cancels proposals and waits for
  actual neutral. No second production raw-touch reader is added. Camera/window,
  DPI, safe insets and UI scale must still match the captured batch at replay.
- [hypothesis] The selected alternate sheet receives admitted touch edges before
  dialogue, generic buttons, tabs, item picking and world input. A gesture belongs
  to one owner. Hardware use, lifecycle loss, presentation changes and fresh menu
  transitions cancel pending actions; surviving contacts drain to neutral.
- [hypothesis] The original hand-built UI must ignore only touches owned or
  quarantined by an alternate sheet, including its closing frame. Ordinary mouse,
  keyboard and controller handling remains available. The filter is an optional
  resource; absent ownership preserves desktop behavior.
- [hypothesis] A visible sheet suppresses the stale underlying cursor through
  derived pointer views only. Actual hardware intent yields the sheet before
  focus; raw Window and button state remain intact. Dialogue's separate existing
  reader filters only owned contact IDs, including release/cancel edges.
- [documented] `dialogue_split.rs` already owns a full-window UI camera while
  cropping only the world camera. UI hit tests use physical camera viewports;
  absolute sheet rectangles are logical full-window coordinates.
- [hypothesis] Reuse a live full-window primary-window UI camera and pass its
  entity explicitly. Reject stale/cropped/offscreen camera geometry. Avoid adding
  a camera that changes the implicit UI default or duplicates a scene render.
- [hypothesis] Exercise actual menu/App effects with native-selected and
  hand-built presentations, overlapping contacts, scroll cancellation, stale
  rows/quotes, controller switching and real hardware preservation. Painting a
  sheet or unit-testing its hit rectangles alone is insufficient acceptance.
