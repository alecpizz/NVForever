# Discoverable Pip-Boy handle

[design] First functional gesture increment for #2436: an optional visible center-bottom
handle, a 48 logical-pixel hit area, and configurable upward travel. It requests the
existing `menu::OpenInventory` action only on a successful release. The normal menu
opener retains script-control locks, active-menu, wait and game-state admission.
No item, equip, fire or save action is emitted by this gesture.

[design] The handle stays above cached Android navigation/mandatory-gesture/cutout
insets, converted by display density to the window's logical coordinates. A read-only global-layout observer caches root insets without replacing normal
inset handling or installing a system-gesture exclusion.
The drawing follows vertical drag progress; short, horizontal, canceled, resized,
backgrounded, unfocused, text-captured or editor-owned gestures reset without opening.
Only an unclaimed start inside the handle is admitted. A start on a moved gameplay
button belongs to that button. A second finger cannot inherit the first finger's drag.

[design] The normal settings page persists `touch-pipboy-swipe-v1` (off by default)
and `touch-pipboy-distance-v1` (48–200 logical pixels in 4-pixel steps). Android logical
pixels are density-independent here. Existing layouts and bindings are unaffected.
The current global UI scale and custom button hit areas remain authoritative.

[design] Three pure routing tests and four real TouchControlsPlugin event-route tests
are authored for release-only commit, no fire/look leakage, repeated release,
horizontal/cancel/resize rejection, second-finger ownership, custom-button precedence,
and menu/suspend/focus/text/editor cancellation. They remain unrun. The fourth event-route regression retains focus loss across
frames until an explicit focus gain. Java compilation
against API35/JDK17 passed; deprecated API notes concern the API29-compatible inset
accessor. Rust parser and diff checks pass. Phone/rendered/native qualification is pending.

[design] This increment does not yet implement a quick tray, downward close, two-stage
tray/Pip-Boy mode, drag-to-assign, long-press item actions, sprint lock, gesture practice,
or experimental VATS gestures. These are approved subsequent increments. In particular,
VATS recognition must be per-screen, use a valid stable target and enter selection only;
it must never queue or fire a shot. Minigame/map two-finger actions own their contacts.

[design] Composition with the controller/aiming branch must cancel this drag on takeover,
preserve strong first-touch consumption, hide the handle with the gameplay overlay, and
use the combined `cancel_touch`/held-sneak release path on commit. The minigame/dialogue
adapters also need the central modal capture bridge before combined qualification.
Java `onCreate`/`onResume` additions must be combined with controller metadata lifecycle
hooks; neither listener nor reset generation may be lost. This source work stays out of
the corrective R4 APK.

## Diagonal opening refinement

[design: requested Android touch follow-on, 2026-11-16] The ordinary opening
start region is a 48-logical-unit band above the safe bottom, clipped to safe
side insets and a 16-unit outer margin. The existing centered closing handle
and dedicated utility tray layout keep their geometry. Actual gameplay buttons,
hotbar cells, modal/editor/practice owners and system edges retain priority.
The same sole reader supplies both the real arm and fallback recognizer.

[design] Vertical travel may include a diagonal component. After the existing
32-unit initial slop, horizontal-dominant intent permanently cancels the gesture;
it cannot become an opening by returning vertically later. Successful fallback
opening retains its saved upward threshold and a latched vertical-first intent;
sideways drift after that win does not reclassify release. Cancellation/reversal,
ownership, focus and native script/menu
admission keep their original meanings. No saved layout or preference changes.
The band's actual Node geometry and font use absolute logical/UI-scale conversion.

[hypothesis: qualification] New authored cases drive actual parent raw input and
Taffy geometry, plus actual flat native menu close and late map refusal. They are
uncompiled/unrun. Arm progress is real gesture state, but that parent fixture
does not run the full visible clip/settlement graph. Existing core arm settlement
controls remain separate; no coupled visible-arm or phone pass is claimed.

[design] The fallback recognizer compares all four common insets during a held
start. The actual parent route and native menu opener are exercised by additive
Direct Pip-Boy Off fallback controls. The actual open Pins tray case verifies
whole-window parent priority. It does not discriminate the fallback hook because
the tray captures earlier in the parent route; the former minus-hook prediction
is withdrawn. These controls remain authored, not retained actual runs.

[design: 2026-11-16 follow-on] The opening and dismissal recognizers retain the
first decisive axis. Vertical-first diagonal travel stays owned while reversing
with a persistent sideways offset; horizontal-first travel is permanently
retired. Real-arm motion is continuous through the existing ArmGesture producer.
The screen-only Drawer still waits for upward release past its saved threshold,
then uses the existing OpenInventory consumer exactly once. Its contact stamp
includes all four safe insets. This is authored source behavior, not a phone
observation; full clip/pose and native runtime qualification remain separate.
