# Optional Pip-Boy touch controls

Clean-room scope: this note describes NVForever's own Android port design, not the original game. Here `[hypothesis]` marks a port design proposal or pending device acceptance, and `[documented]` cites repository source, an owner request, a pinned dependency or public platform documentation; neither is a claim about Fallout: New Vegas (see the [classification](../design/android.md#clean-room-classification-of-the-ports-constants-and-behaviour)).

[documented: engine/nvforever/src/touch/pipboy_drag.rs; engine/nvf-play/src/pipboy/gesture.rs] The existing bottom handle drives the actual first-person Pip-Boy clip through ArmGesture. Its release settles to an endpoint, cancellation returns to the original endpoint, and the existing menu consumer owns opening/closing. This path predates this extension.

[documented: engine/nvforever/src/touch/pipboy_model.rs; engine/nvforever/src/pipboy_arm.rs] The physical ScrollKnob has a target derived from the current CPU-posed model. Its vertical drag already uses 20 logical pixels per line and the shared late-checked ListScrollRequest. No guessed wheel bone, mesh or fixed screen rectangle is introduced. An unavailable/malformed/unprojected wheel remains unavailable.

[design] Independent saved switches enable left and right downward dismissal handles and an explicit close button. Their 48-logical-pixel regions sit inside all four published system insets. Existing painted model controls, menu canvas, scrollbar, tray and active gestures retain priority. A new contact must start in a configured unclaimed region. A downward drag reverses while held; releasing past halfway closes, cancellation returns open. The ordinary bottom handle remains separately controlled by its existing preference.

[design] Where the real worn first-person rig and clip are available, all dismissal handles use that same reversible arm animation. A screen-only or unavailable-rig menu has no invented arm: a deliberate downward swipe or close tap requests the ordinary guarded native close. The adapter clears on resize, inset changes, focus/suspend, controller or modal takeover and never synthesizes physical key/mouse state.

[design] Show Pip-Boy button graphics changes paint only. When off, overlay icon/background/label nodes are not drawn, while the exact same targets, bindings, ownership, clipping, cancellation and native action dispatch remain active. This is independent of disabling a feature. Native game-model parts are not hidden. Gameplay settings/help relocation is a separate change.

[design] The physical model wheel switch only gates its touch interaction; physical mouse/controller scrolling and generic on-screen scrollbars remain unchanged. Disabling it during a drag retires that contact and its queued lines; reenabling requires a fresh Started.

[hypothesis: qualification] New source fixtures must exercise actual projected wheel-to-list state, actual arm/menu consumer endpoints, visible/invisible equivalent hit dispatch, saved settings round trips and cancellation/owner coexistence. Compilation, runtime and phone acceptance remain pending until separately executed.

[design] Added dismissal contact IDs use a fixed 32-slot mask shared with the existing post-selector UI-focus publisher. Overflow consumes further raw edges until actual touch neutral; release/cancel edges remain masked. This does not suppress physical mouse input or fabricate release edges. A graphics toggle updates color alpha in place, without despawning its graphic nodes, changing visibility, or retiring the captured contact.

[design] Overlay roots explicitly target a validated full-primary-window camera, preserving any foreign default-UI owner. Logical window target positions/sizes and font/border sizes are divided by actual UiScale before layout, so their computed physical bounds agree with the raw window-logical targets at non-unit UI scales. The visual switch still only updates alpha on those same nodes. Dismissal's captured window stamp includes the actual Window Entity as well as logical/physical size and density; a same-size replacement cannot inherit a pending close from the old window.

[design] Model buttons and the physical wheel also retain a fixed 32-slot
window/contact-ID mask. Successful release, cancellation and wheel Off/On do not
turn an owned tail into a generic UI click. The existing post-selector publisher
uses only the current primary window's records; replacement windows cannot inherit
another window's same-numbered contact. Overflow refuses further model effects for
that window until actual touch neutral. It does not clear physical input or suppress
unrelated pointer IDs. Before InputSystems, neutral additionally requires the sole
reader's unread batch to be empty. The model request stamp includes actual window
entity, logical/physical size and density and is checked again at dispatch; queued
list-scroll application repeats the window check at its existing effect consumer.
Model target publication, live admission and both effect boundaries additionally
require the actual world camera destination to normalize to that primary window.
Primary and an explicit reference to the same window are equivalent, including a
change between those aliases during a contact. An offscreen image or another
window is not evidence of presentation in this window, even at the same dimensions.
This does not constrain internal render-resolution overrides, viewport or jitter.
Page commands keep the existing PipBoyInput route. Source controls for actual parent
input, Taffy/UI focus, late-window refusal and neutral recovery remain unexecuted
until separately qualified; they do not establish native XML or phone acceptance.

## Diagonal dismissal and explicit flat space

[design: requested Android touch follow-on, 2026-11-16] Left/right downward
dismissal admits vertical-dominant diagonal travel rather than imposing an
absolute 32-unit sideways cap. Horizontal-first movement beyond that initial
slop still cancels permanently. The safe bounds, reversible travel, halfway
release, current window/model/menu gates and captured-ID mask remain authoritative.

[design] An enabled left/right handle reserves 64 logical units on that side in
the flat crop's existing fit and padding: eight outer, 48 target and eight inner.
The optional Close button reserves the same bottom space. This ephemeral
presentation request changes neither the saved Pip-Boy size nor system insets.
Disabled requests preserve the exact old fit. A width-limited view could otherwise
lose the entire configured handle when its rectangle overlaps the flat canvas.
This is not assumed to occur on every landscape phone. Existing UiScale and
density conversion remains the actual renderer's; saved interface scale is separate.

[design] The flat pointer and map's late stamp derive the same requested padding.
Changing a registered dismissal option invalidates an old projected map batch;
its release cannot become a marker or item action on moved content. The central
canvas remains the ordinary list/map owner's territory. For the real arm, only
actual free projected screen/button/knob space may admit a side handle: whole
noninteractive wrist/arm art is not excluded, and interactive screen geometry
is not overridden to make a handle fit. Graphics remains paint-only.

[hypothesis: qualification] Ten authored cases add real Taffy/raw/native Close,
small-view/density/UiScale/southpaw, reversal/cancel/focus/ID, actual map late-gate
and arm-diagonal progress controls. They retain the old fixtures and are
uncompiled/unrun. The direct-arm fixture does not prove the full visible clip
or native endpoint; no physical phone success or cause is inferred.

[design] If those gutters leave less than a 96-unit canvas extent, the original
flat fit remains available instead of blanking the menu; overlapping handles
remain refused by their existing geometry gate. This is a tiny-window fallback,
not a claim that every enabled handle can fit every arbitrary window.

[design] Fallback opening now stamps all four actual safe insets as well as its
existing threshold/bottom/destination context. A side-inset change retires the
held drag before its End can request inventory. Additive controls use the real
registered Direct Pip-Boy swipe Off callback, actual rendered broad band and
MenuPlugin OpenInventory consumer. An actual open Pins tray excludes a batched
opening start through the shared hotbar mask; no hotbar rectangles are forged.
Ordinary fitted slots already remain above the band through their 64-unit bottom
reservation. These new controls remain uncompiled/unrun.

## Outside-display downward dismissal

[design: user clarification, 2026-11-16] The selectable left/right regions are
all admitted space beside the actual display, not merely the small edge cues.
Left covers the noninteractive wrist/arm side; right covers world beside the
device. Current projected SCREEN triangles, page buttons and wheel are excluded;
the whole arm-art bounding box is not. The flat fallback uses its actual fitted
canvas rectangle. Existing hotbar, UI, map, safe-area and control ownership win
before a new contact starts. Small graphics identify the gesture but do not
limit its hit region, and hiding those graphics does not disable input.

[design] Mobile adds **Pip-Boy downward dismissal**: Disabled, Left only, Right
only or Both, plus **Pip-Boy dismissal travel**, 48–240 logical units (default
120). These two rows replace the two old independent side-switch rows in the
visible settings page. The old booleans remain the only side-state storage and
their existing callbacks remain operational. Search/Changed redirects their
hidden rows to the chooser. A valid explicit new choice overrides old flags in
virtual INI text before the ordinary loader; explicit Disabled also wins its
equal-default short circuit. Absent/malformed new text is left untouched, so
valid old flags load normally and missing/malformed rows retain existing state.
The sole ordinary writer saves the chooser and old flags together; reading never
writes the file or uses a second boolean grammar.

[design] A downward real-arm drag continuously lowers the existing ArmGesture
pose. Travel is capped by actual safe space remaining below its start. An
8-unit deadzone is capped to one quarter of that travel in a very short range.
After vertical intent first wins beyond 32 units, reversing upward with the
same sideways drift raises the arm smoothly. Horizontal-first intent still
cancels permanently. Releasing settles at the existing halfway threshold;
cancel restores the original state. A finger remains owned even when its own
lowering animation moves the display underneath it. The normal producer and
Animator consume the pose; no wrapper writes clip time or fabricates animation.
Direct-arm policy remains independently saved; screen-only/direct-Off fallback
uses the ordinary guarded native Close on release and claims no manual arm pose.

[design] Contact admission captures window/density/safe area, saved side/travel,
actual Bevy UiScale, frame and real camera identity/target/viewport/projection.
Open gestures also capture page/tab/image/source scale. Changing these retires
that contact; an old End cannot act on a replacement view. The arm's own moving
screen bounds and animated lens/rig transforms are deliberately not treated as
external camera changes. External camera-transform motion is not separately
identified by this adapter; normal player input remains blocked by the existing
owned-arm gate. No broader camera-motion cancellation claim is made.

[hypothesis: qualification] Authored real Settings load/save controls, sole-reader
raw input, Taffy display/cue geometry, native flat Close/open, mode/camera refusal,
continuous progress/reversal and preserved display scrolling are uncompiled and
unrun. Full public-plugin clip/Animator/pose coupling is a separate authored
provider fixture; progress alone is not evidence that the visible arm moved.
GPU rendering, actual handset reach and physical phone success remain unproved.
Earlier narrow-edge cases are retained as compatibility controls, not as proof
that they alone satisfied the clarified outside-display requirement.
