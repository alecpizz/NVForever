# Touch practice (#2436)

[design] Android Settings > Mobile > Touch layout and guide offers an optional
**Controls help / gesture practice** action beside **Customize layout**. The
[editor entry and return path](android-mobile-entrypoints.md) has no persistent
settings/help shortcut on the gameplay HUD (Controls > Touch on desktop).
Practice provides a first-use explanation,
not an automatic interruption: read movement-thumb and opposite-thumb look
instructions on separate short pages before pressing Begin. Reset
returns to that guide. Close and Escape return to Settings. No Seen bit or active
session is persisted.

[design] The screen opens only over the paused Settings page, without changing the play
state. It owns FormOver while open and through its closing drain, following the existing
hand-report form contract. The settings choice is an action (not saved); loading any
stored row text cannot open it. It refuses an existing form.

[design] Practice consumes its own raw TouchInput route before live touch contacts,
mouse-look synthesis, aim latches or drawer commands. It only changes local lesson
state: joystick direction/dead-zone display, a pretend look marker and Fire/Aim/Jump
acknowledgements. The current dead zone, analog/legacy mode, floating/fixed explanation,
Aim hold/toggle mode and optional drawer distance are copied on opening. The movement
side follows the current stick position, including mirrored/custom layouts;
the movement ring/target and opposite look panel/marker mirror together. The analog
dead-zone display is circular; legacy directional movement retains a square.
Its optional drawer exercise only acknowledges a local swipe. It never
presses input buttons or writes gameplay controls, inventory messages, ammo/AP, player
transforms or gameplay saves. Live touch contacts, aim latch, held-sneak and drawer
recognition are cancelled/synced on entry, throughout practice and on closing; hardware
key/mouse ownership remains with the existing merger.

[design] The first opening frame drops the entry gesture. Closing consumes the remaining
raw-touch batch, and two following updates drain trailing input before the form guard is
released. Lost focus, suspension, missing/resized window or leaving the paused Settings
page cancels the practice session. Raw keyboard Escape closes the form while the pause
menu remains guarded. The existing editor-active marker prevents automatic controller
takeover from KeyboardMouse while the form owns input. Real touch activity can still
restore KeyboardMouse from Controller through the normal physical-device selector; it
never creates a live practice action.

[design] Entry is unavailable below 400 x 300 logical screen units, with a
rotation/window-size explanation in Settings. The screen uses original text, geometric
controls, large labelled targets and fixed logical-screen geometry. Text labels use Bevy
Label markers, refreshed when their words change; native screen-reader activation
remains unqualified. The joystick includes a visible dead-zone and knob/direction
feedback; the look area is separate from action targets. The drawer exercise is shown
only when the real optional drawer setting is enabled, but never uses its OpenInventory
bridge. No dependencies or game-derived artwork are added.

[qualification] The [R9 hosted CPU run](https://github.com/LootJunkieAzza/NVForever/actions/runs/43669164991/job/132675180379)
passed all fifteen `touch::practice` routing, lifecycle, settings and gesture cases
at source `28d5469`. This includes hardware-preserving entry/close and isolated
synthetic actions. Phone layout, accessibility and font fit remain unqualified.

[design, review revision 2] Floating movement admits the selected movement-side panel and
anchors the ring/dead-zone at the owning contact's Started position. That origin
stays fixed until release/cancellation; another movement finger cannot replace it.
Fixed movement retains its demonstration-ring anchor and ring-only
admission. The ring renderer follows the same owned origin used by direction feedback.

[documented: engine/nvforever/src/touch.rs, toggle aim and TouchPhase::Canceled]
The synthetic Aim model mirrors live source timing: toggle at Started once while
no other Aim contact is down, retain through drag and Ended, and clear the toggle
on any Canceled touch. Hold mode stays active while an Aim contact remains. These
changes only update the local guide model; live input synthesis remains blocked.
The corrected Aim route and floating/fixed-origin cases both passed in the R9
CPU run; that result does not establish physical-device gesture usability.

[documented: touch/profiles.rs and touch.rs] The Style panel's "Pad backup" preset
is a fallback touch layout. Its persisted id remains `controller`; it changes
only touch placement/size. Physical gamepad mappings and Auto/Xbox prompt names
remain in Controls > Controller and are not selected by this touch preset.

[qualification] The handedness/dead-zone/preset-wording correction adds authored
App/provider/serialization controls. They passed ARM library/test-code type/lint
checks at code checkpoint `81dc1347`, with the two disclosed local lint-category
exclusions. That checkpoint also passed
the 1,517-entry scenario-file verifier and merge-parent integrity, not game
scenarios. The historical R9 result above does not qualify the changed presentation.
The later R31 hosted run passed the dead-zone and Pad backup controls but failed
the mirrored practice control's final full-config equality at `practice.rs:1566`:
only the implicit tray centre differed after its ordinary migration. The fixture
now lets the real provider settle before taking its baseline and checks that a
second non-practice frame preserves that whole baseline. All original subsequent
gestures and the final whole-config equality remain intact. This fixture-only
correction is unexecuted; it does not establish a runtime pass or phone acceptance.
