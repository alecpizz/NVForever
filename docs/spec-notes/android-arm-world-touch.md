# Direct arm gesture and world activation

Clean-room scope: this note describes NVForever's own Android port design, not the original game. Here `[hypothesis]` marks a port design proposal or pending device acceptance, and `[documented]` cites repository source, an owner request, a pinned dependency or public platform documentation; neither is a claim about Fallout: New Vegas (see the [classification](../design/android.md#clean-room-classification-of-the-ports-constants-and-behaviour)).

[hypothesis: Android interaction design, requested 2026-11-14, issue #2436]
The owned bottom handle controls the real first-person Pip-Boy raise pose while a
finger moves vertically. Up and down are reversible. Release settles to the nearest
endpoint, then uses the ordinary inventory open/close routes. A canceled drag returns
to its original endpoint. Focus loss, loading, controller takeover, resize, an editor
or another modal owner cancels the contact and restores a valid native state. A
Pip-Boy worn on the arm is required for direct pose control; screen-only menus keep
their existing opening behavior. Utility-tray swipes keep their existing recognizer.

[documented: engine/nvforever/src/touch/pipboy_drag.rs]
Opening retains the user's saved upward distance. Closing starts near the bottom
edge and uses the finite positive distance remaining from that actual contact to
the bottom safe boundary. A changed window or bottom inset invalidates the owned
closing range. The authored parent tests keep every raw sample inside that range;
synthetic movement below the screen is not evidence of usable closing travel.
Android derives the handle's legacy bottom resource from the same common logical
inset snapshot before raw input and tray migration; it no longer independently
polls a second Java/density value later in Update. Desktop defaults are retained.

[documented: engine/nvf-play/src/pipboy.rs; engine/nvforever/src/player_body.rs]
The existing `PipBoy.clip_time` feeds `pose_rig`, whose Pip-Boy Animator layer is
sampled at an explicit time and zero playback rate. Direct manipulation reuses this
route rather than drawing a second arm or a progress-only panel. Preview does not
report a script menu or accept menu input. Existing cold-rig warming remains owned
by `pipboy_arm`; preview waits for that interval without changing its render proof,
caps or borrowed-pose restoration. A new/replaced first-person rig cancels ownership.

[hypothesis: input ownership design]
The existing platform quarantine has independent native-loading and arm reasons;
their OR blocks ordinary controls, pointer, analog movement and look. Only the
captured arm ID may continue while the arm reason alone is set. A native block always
cancels it. Physical buttons are never cleared: held input and other touch IDs stay
quarantined until neutral. Arm-only Back cancels the gesture and never invokes the
Java loading-recovery action.

[hypothesis: Android interaction design, requested 2026-11-14, issue #2436]
Direct world touch uses selectable tap, double-tap or hold confirmation. Movement
cancels the candidate; release/cancel, target loss or a new modal owner cannot replay
an activation. The pending target's native name/action and a circular hold indicator
describe what confirmation would do. This route never emits Fire or an unrestricted
post-lock activation request.

[documented: engine/nvf-play/src/activate.rs and activate/pick.rs]
Selection uses a ray through the touched screen position, with the same activation
reach, collision/door/bounds arbitration and visible-reference descriptions as native
activation. Confirmation re-picks and requires the same target. Dispatch preserves
movement locks, VATS/wheel ownership, lock/key rules, crime checks and script
OnActivate behavior. Native crosshair/script-sense state remains the crosshair's;
the touch candidate is separate. No new attack, sprint, range or script permission
is introduced.

[documented: activate.rs::Targeted::prompted, merged main #2570]
The shared hidden-target policy also applies to touch prompts. A living hostile
actor cannot acquire an interaction label, hold ring or confirmed touch action;
once rejected, that contact must end before a fresh legal target is admitted.

[hypothesis: qualification plan]
Authored App/event tests must exercise real Animator sampling and native inventory
and activation effects, including forward/reverse/settle/cancel/model replacement,
blocked/overlapping input reasons and physical neutral-release. No game files,
screenshots or device identity belong in fixtures. Source review, CPU tests, native
compilation and phone acceptance are recorded separately; none proves correct pixels
or a fix for the separately investigated DeviceLost error.

[hypothesis: composed inventory gesture authority]
An existing item contact, painted header, tab, button, scrollbar or tray wins over
the closing arm handle. The wrapper's closing entry point requires the parent's
current projected unclaimed result before creating a new item contact. It checks
the actual Inventory mode, device frame, first-person model and raise-clip
availability. Flat/screen-only inventory retains its existing gesture path.
The parent composes this small hook against its last-painted device/flat bounds;
the leaf does not substitute guessed menu rectangles or a second raw reader.

[documented: engine/nvf-play/src/settings.rs]
Both new choices use the existing settings registry and INI persistence. Direct
arm manipulation is subordinate to the existing optional Inventory bottom swipe;
disabling it preserves that recognizer. World touch defaults to hold confirmation
on the Android adapter; the standalone core resource defaults off and changes no
desktop input. Existing bindings and script-disabled controls remain authoritative.

[documented: touch/world_activate.rs and touch.rs]
An unconfirmed short Hold with only a pending screen ray does not own the optional
VATS tap gesture, including movement below the existing drag threshold. A sampled
native target retains world-touch priority for the rest of that contact. Tap and
double-tap confirmation retain their pending reservation. Actual dragging, the VATS
duration bound and the parent's same-batch fire-tail quarantine remain unchanged.
