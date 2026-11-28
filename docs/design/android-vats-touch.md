# Direct touch selection in VATS

[design] An independently saved `touch-vats-targeting-v1` choice offers Off (the
existing controls), Tap, and Hold. Off is the compatibility default. This setting
does not enable the separate double-tap gesture that enters VATS. Tap and Hold
work in Classic and Mark and Fire (the default style). Explicit Off and existing
physical mouse/keyboard and controller selection retain their behavior; the saved
key and values are unchanged and no preference is migrated or forced.

[documented: engine/nvf-play/src/vats_mode.rs, tiles/vats_menu.rs, hitvol.rs]
Classic VATS already projects body-part labels, casts pointer rays against posed
actor part capsules, highlights its selected part, reserves AP in `mark_actor`,
and executes only through its separate accept path. Direct touch reuses the same
camera-to-world ray and physics filter; it never moves the global mouse cursor or
injects a mouse button. A finger must hit an actual current target's part capsule
(or its whole-actor capsule when no part volumes exist), rather than the nearest
unrelated label. This works independently of the native-file/hand-built label
presentation. It does not establish pixel-accurate mesh picking.

[design] The sole wrapper reader routes admitted contacts to the core Bridge.
Tap queues once on a matching release; Hold queues once after 450 ms on the same
part. A highlight and a short progress caption explain the pending selection.
Movement off that part, cancel, a second contact, changed window/viewport/FOV,
changed stay/style/weapon, focus loss, a higher modal or hardware takeover cancels the
pending action. Retired contact IDs remain quarantined until an actual neutral
batch; the bounded ID limit fails closed. Moving an actor or camera is permitted
only while the current ray still hits that same actor and part. No drag-to-paint
or repeating hold is supported.

[design] The actual VATS update rechecks the camera/window, platform, device,
modal, current stay, current body ray and actor eligibility before changing its
selection. Classic also requires its current candidate list. Mark and Fire uses
the live ray-hit actor without creating a Classic list: the existing preview
builds that actor for the active finger, resolves the actual hit part and updates
its highlight. Without a finger, ordinary crosshair preview resumes. No camera
turn or reticle change is requested for Mark and Fire. It then calls the existing `mark_actor` consumer, which retains current
death/targetability/cloak/melee/AP/ammunition rules. Selection never sets accept,
advances to Firing, spends AP or fires a shot. In enhanced mode only, the wrapper's
real touch Fire control calls `Bridge::fire`: it requests the existing accept flow
for a nonempty queue. Physical Use/Activate and the controller retain their old
queue/accept meanings. Root owns shared input/provenance/settings placement; the
leaf introduces no second TouchInput reader or synthetic hardware state.

[design] Tests must route real projected TouchInput through Bridge into the actual
VATS update, inspect selected actor/part, MarkQueue/AP reservations and phase,
and retain valid Fire and controller controls. Authored headless fixtures are
implementation evidence only; compiler, runtime and phone acceptance are pending.

[design] The parent prepares once per input frame, including eventless holds,
from the real primary window and world PlayerCamera. Before InputSystems a neutral
witness also requires an empty unread raw batch. The routed admission ignores only
the previous frame's TouchDeviceSwitch; the real VATS consumer still checks the
current selector result before an effect. Existing editor, practice, modal,
maintenance, model, arm and map owners keep priority. The actual Fire contact
continues outside its button without synthesizing a generic Use edge.

[design] Physical input hides and cancels enhanced selection. One actual first
touch restores it without selecting or firing; the rest of that raw batch is
quarantined too. Controller return retires actual contact IDs, then uses the
existing post-selector restore policy. Only the existing menu-sheet publisher
adds those owned IDs to the UI mask. A later fresh tap needs no extra idle frame.
The progress caption updates from the user's live HUD colour; it introduces no
separate colour preference or fixed amber palette.

[design] During a validated live body contact, Classic target-follow does not turn
the player camera onto the selected actor. That automatic turn would move the
body away from a stationary finger before its Tap release or Hold deadline. The
existing view follow resumes on release, cancel or refused current-ray validation.
Actor movement, changed part/camera/projection and ordinary hardware gates still
require the current ray to meet the same live body. This does not freeze actors or
ignore camera movement from another source. Off remains the saved default.
Mark and Fire does not run this Classic follow path; touch selection changes
its temporary preview and queue through the shared consumers instead.

[design] Five additional authored App cases pair the production place_camera and
TransformPlugin with the existing posed-capsule fixture, real camera rays and VATS
update. These are separate from the ten original static-camera cases. Baseline
and candidate use identical case bytes. All new cases are uncompiled/unrun; no
wrapper/phone pass or cause of the reported device failure is inferred.

[design] A current-ray contact rejected by actual SimState present/barred admission
releases camera follow in that same update. Preview-only rejection occurs after
classic_frame in the existing update order; its refused contact releases follow
on the next frame. No duplicated target/AP/preview legality rules are introduced.

[qualification] Additive Mark-and-Fire controls are authored for real off-reticle
posed parts through Bridge and VATS update, Tap/Hold, unchanged Classic state and
camera, AP/ammunition limits, empty Fire, canceled or stale body/geometry/style/
modal/hardware contacts, late actor/cloak rejection, native reticle mouse/controller
behavior with explicit Off, and actual wrapper raw-touch Fire confirmation. Original
Classic cases, both explicit-Off style controls and Classic camera-follow assertions
remain. The former enabled-DeadEye refusal section in the live-camera policy test
intentionally becomes a live body Hold/short-cancel/queue/explicit-Fire control;
its old unsupported-policy oracle does not apply to this feature. These controls are uncompiled
and unrun; no device acceptance or default-APK VATS repair is claimed.
