# Android touch aiming settings

[design] These are NVForever touch-input choices, not claims about the original game.
They extend `touch-controls.json` version 1 with an optional `aiming` object. Existing
files retain the current defaults: look and ADS scale 1, movement dead zone 0.18 of
stick radius on each axis, hold-to-aim, and tap-to-toggle sneak. Values are bounded and
non-finite or malformed values fall back to defaults. Layout save/cancel includes the
settings, while a separate aiming page avoids enlarging the layout toolbar.

[documented: engine/nvf-play/src/aim_zoom.rs] `AimZoom.aimed` is calculated from an
actual sighted weapon that is ready and out, the player's aiming intent, play state,
and reload/jam restrictions. Merely holding the Block binding is also used for melee
blocking and is not evidence that ADS is active. Touch look therefore reads the
engine's `AimZoom.aimed`; the selected multiplier applies only to MouseMotion generated
from a touch-look contact. Hardware mouse and controller deltas are unchanged. The
ordinary engine sensitivity and vertical/invert settings still apply afterward.

[documented: engine/nvf-play/src/control.rs] Native aiming reads a held Block action;
native sneaking toggles its persistent state on a CrouchSneak press. Touch toggle aim
therefore owns an ordinary held Block binding, combined with physical ownership by
the existing touch aggregator. It clears on cancellation, focus loss, suspension,
leaving gameplay, or opening the editor. A second tap releases only touch ownership.

[design] Hold-to-sneak is an optional momentary overlay in the ordinary control route,
not a fabricated pair of toggle presses. It keeps the independent keyboard/controller
and explicitly script-updated sneak base, applies held touch sneak while permitted, and restores
that base on release/cancel. Real toggle presses while a touch hold is active update
the base and survive its release. The optional resource leaves all callers without it
unchanged. The current device, capture, the same preferred key/mouse binding as touch translation,
and gameplay-control restrictions
still apply; the normal lock enforcement remains last. Player movement reads the same
momentary result in its existing input phase, avoiding a delayed release.

[design] The editor's Aiming page is an independent bottom sheet: paired look/ADS
value and minus/plus groups, an aim/sneak mode row, a dead-zone/reset row, then explicit
Save/Cancel/Layout/Help controls. It uses separate geometry from the layout toolbar,
so later canvas and secondary-tray designs can reuse it without a central-grid contract.
The bounded dead-zone cycle and aiming reset leave custom button positions alone.
Changes are previewed in the editor but editing
continues to request the normal pause. No gyro, controller sensitivity or automatic
firing is introduced.

[qualification] Authored tests must exercise real TouchInput/KeyboardInput routes,
within-frame taps, source masking, cancellation/menu/lifecycle cleanup, actual ADS
resource state versus a held aim button, persisted values and setting-page behavior.
Compilation, CPU execution and rendered device checks are separate gates.
