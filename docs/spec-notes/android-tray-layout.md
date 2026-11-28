# Profile-owned tray layout

Clean-room scope: this note describes NVForever's own Android port design, not the original game. Here `[hypothesis]` marks a port design proposal or pending device acceptance, and `[documented]` cites repository source, an owner request, a pinned dependency or public platform documentation; neither is a claim about Fallout: New Vegas (see the [classification](../design/android.md#clean-room-classification-of-the-ports-constants-and-behaviour)).

- [documented] The owner requested tray movement/size editing, per-profile persistence,
  safe-area/orientation bounds, preview Save/Cancel, and shared drawn/hit geometry.
- [documented] Existing `touch::layout::Config` preserves unknown JSON fields and profile
  snapshots; its editor already provides transactional Save/Cancel. The new tray target
  belongs to that transaction rather than a second input reader/editor.
- [documented] A nested `tray` object has its own version1 and normalized whole-window
  x/y, scale and visibility. Absent tray is eligible for legacy migration. Unknown future
  nested versions remain unchanged and refuse tray editing; other known fields may
  still be saved. Unknown fields inside a supported tray are overlaid, not removed.
- [documented] Legacy hotbar placement normalizes its anchor against the usable height
  after bottom inset and navigation margin; Config positions use the whole window.
  Migration converts coordinates after settings load and does not rewrite disk until
  explicit Save. The old size key stays stable until a profile-owned tray is explicit.
- [documented] Pins and bound item identities retain their existing stores. Legacy
  hotbar JSON unknown fields survive pin writes. Profiles without a tray retain the
  current one when merged; supported profiles carry their own tray placement/scale.
- [documented] The existing hotbar fitter remains the sole slot-rectangle source for
  rendering, editor preview, hit testing and navigation exclusions. Safe area is the
  common logical L/T/R/B resource. Editor preview emits no live slot action.
- [hypothesis] Physical reach, cutout/orientation behavior and fit comfort require
  device acceptance; headless route/JSON tests do not establish it.

- [documented] The ordinary size setting reads committed placement during every
  layout transaction and refuses a size write until Save/Cancel. The dedicated
  Smaller/Larger buttons alone change the draft; an unrelated settings save cannot
  persist its draft scale to the legacy INI key.
- [documented] Native safe-inset polling runs in common `touch_safe_area::Publish`
  before the tray admission/geometry guard. Final preview publication follows the
  actual device selector and rechecks its consumed-frame gate plus persistent
  controller/suspend state. Ordinary pause/disabled bar remain previewable; blocked
  lifecycle/platform/modal/device routes retire drag and never publish a live bar.

- [documented: touch::hotbar::Bridge] Returning from an explicitly canceled editor
  can reconcile the retired bar into gameplay without discarding a new tray request.
  This exception is limited to mode0-to-mode1 with unchanged menu session, window
  size, safe bottom and fresh-input gate. Owned contacts and binding state still
  retire. All other existing full cancellation paths still discard queued requests;
  fitting-only reconciliation keeps its existing policy. The boolean request has no
  epoch and does not establish freshness across an inactive interval.
