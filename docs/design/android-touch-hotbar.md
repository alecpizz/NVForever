# Android touch hotbar and item assignment

[design] This is an opt-in touch presentation of the existing eight hotkey slots.
The second slot remains Ammo Swap. Four, six or eight visible slots, size and bar
position are persisted as touch preferences. Pinned consumables reference hotkey
slot indices, not parallel item IDs: existing save/load-order remapping remains
authoritative. Only carried Aid items can be pinned to the optional utility tray.

[documented] `hotkeys::Hotkeys::assign` controls eligible item kinds, mod flags,
duplicate movement and the reserved slot. `weapon_wheel::read`/`act` controls actual
use, holster waits, sounds and `ItemUsed` events. The touch bar sends a bounded
semantic slot request through that path rather than synthesizing hardware keys or
duplicating equip/consume behavior. Bindings and physical ownership stay unchanged.

[design] A tap activates only on release inside the original slot. A hold of450ms
opens a safe context without using anything; release of that same finger never
chooses a context action. A subsequent tap can pin/unpin an Aid slot, clear a slot,
or reposition the bar. Moving, canceling, resizing, mode changes, input-device
handoff, focus/background and platform covers cancel pending use. Other fingers
cannot inherit a contact. Visible rectangles and touch bounds share one geometry.

[design] Pip-Boy row assignment uses the real rendered item row and existing
`RowKey`/mode identity, preserving item/mod selection. Raw touch coordinates feed
the existing flat crop or posed-model ray-to-screen mapping. A touch-only pointer
is distinct from the hardware mouse/custom legacy pointer. Dragging onto a bar
slot assigns; it never consumes the item. Long-held or dragged row contacts
suppress ordinary release clicks. Assignment is refused after the inventory
session or row identity changes, under a platform cover or during controller
switching. Ordinary desktop mouse/controller behavior has no touch resource.

[design] The optional utility tray is opened by an explicit tray button/request;
it does not compete with the bottom swipe that opens Pip-Boy. The parent can send
the tray request from its inventory gesture owner. Configuration and gesture
replays, plus actual App outcomes for assignment/use/refusal and lifecycle gates,
are required before device qualification. No game-derived assets are added.


[design] Responsive fitting reserves the actual active button shapes, expanded
stick hit disk, reticle, help/editor targets, enabled gyro target and fixed
run-lock rectangle. It searches bounded row/column placements at the preferred
saved center, then smaller cells down to48 logical pixels if needed. Resolved
geometry is cached until its inputs change; it never overwrites the saved anchor.
Every visible cell and touch target uses this resolved geometry. A fully occupied
custom layout shows a noninteractive request to make space in the touch editor.

[design] Expanded pins, long-press context and Move bar are explicit captured UI.
Admission includes pending outcomes before the Update dispatcher, idle hold ticks
before the gyro reader, and a closure-frame drain. Only touch-owned controls and
gesture history cancel; hardware owners remain intact. The single existing gyro
reader uses the shared capture gate. Floating run-lock markers yield to actual
hotbar rectangles, and invisible gyro controls do not reserve normal hit priority.


[design] The existing optional bottom-handle recognizer has one persisted target:
Inventory by default, or Utility tray. Existing enable/distance keys and Android
insets remain authoritative; changing the target never enables the handle.
Utility tray requires the touch hotbar and its visible Pins button. A saved choice
remains saved when dependencies are off, while the handle is hidden/unavailable.
The same UtilityTrayRequest route opens the tray; admission captures generic touch
and the later raw gyro batch immediately, before Update dispatch. Target, distance,
inset and mode changes cancel a started swipe. The ordinary visible Pins control
continues to open/close the tray without enabling a swipe.
