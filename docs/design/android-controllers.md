# Android controller admission

Status: source follow-up to the experimental Android app; device qualification pending.

- [documented] [Android controller input](https://developer.android.com/games/sdk/game-controller/controller-input)
  distinguishes gamepad button keys from joystick motion, requires checking input sources,
  and publishes standard axes plus device-provided flat ranges. Bluetooth/USB transport is
  handled by Android's input-device layer; this app does not pair devices or request
  Bluetooth permissions.
- [documented] [InputManager.InputDeviceListener](https://developer.android.com/reference/android/hardware/input/InputManager.InputDeviceListener)
  reports input-device arrival, change and removal. Metadata comes from InputDevice;
  a disconnect is distinct from a neutral sample.
- [documented] [gilrs 0.11.2 supported platforms](https://docs.rs/crate/gilrs/0.11.2)
  does not provide an Android backend.
- [implementation evidence: pinned dependency source review] winit 0.30.13's Android
  `handle_input_event` consumes NativeActivity's input queue. It treats motion as touch
  and keys as keyboard and normally returns Handled. Its android-activity 0.6.1 queue
  iterator is single-consumer and Android-main-thread-only. A second poller would race
  window input; Java Activity dispatch callbacks alone cannot reliably recover already
  handled native events. See [winit Android source](https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/android/mod.rs)
  and [NativeActivity](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/core/java/android/app/NativeActivity.java).

## Admission boundary

[design] A small Android-only winit hook runs before its ordinary input translation.
It allows one application handler to consume recognized controller events while returning
None for ordinary keyboard, touch, mouse, volume and unknown events. NVForever owns the
single handler. The vendored source is the exact Cargo.lock checksum-verified crate,
retains upstream licences, and has a separately captured source manifest and tiny patch.
The registry and every other platform backend stay unchanged.

[design] The callback copies recognized input into a bounded application queue; no ECS,
JNI work or game actions occur inside winit dispatch. Motion samples coalesce per device;
button transitions retain receive order. An overflow invalidates held state rather than
silently losing a release. The engine drains once before Bevy InputSystems, creates one
Gamepad per Android device and writes Bevy RawGamepadEvent messages. Existing input
filtering, remapping, prompts and menu translation then use the ordinary route.

[design] Java supplies device metadata and removal/change notifications, never the native
button/motion stream. Pause, resume and focus transitions increment a reset generation;
observing a new generation discards pending samples. The ECS consumer also discards
ordinary samples on an inactive-to-active boundary, covering a generation already polled
while inactive. Repeated JNI polling bounds local references and clears a failed Java
exception before retrying. Source checks admit only real gamepad/joystick devices; a keyboard's
arrow keys or a TV remote are not reclassified merely because they share key codes.
Standard Android axes are used, with published common-variant fallbacks selected from
reported motion ranges. Sticks honor hardware flat ranges and triggers are clamped to
0..1. D-pad key and hat ownership combine so either physical representation can release
without cancelling the other. Digital/analog trigger reports combine similarly.

## Lifetime and input selection

[design] Disconnect, focus loss, suspension and queue overflow release buttons and zero
axes. Input holds are cancelled so a lost controller cannot complete a grenade throw,
grab, weapon-wheel selection or pending UI click. ActionPads aggregates all pads, so any
pad invalidation cancels aggregate pending holds, including a pad other than the last
one used. Focus cleanup preserves the saved input choice; overflow/metadata failure
falls back to keyboard/touch, while disconnecting the active pad returns the overlay. Device IDs are not treated as lasting
identities after removal. A returning pad begins neutral and requires fresh activity.

[design] Android touch opts into the existing automatic device-switch policy. Raw touch
starts request keyboard/touch use; held/released touches keep that old device busy just
as physical keys do. Switching consumes the triggering action edge while preserving
releases. The first touch while Controller is selected restores the overlay only;
ordinary contacts/actions begin on a subsequent tap. A touch editor keeps the selector
on touch until it closes, so a pad cannot operate the menu behind it. The resource exposes allows_actions() for virtual hold controls, and receives
synthesized touch key/mouse ownership separately from physical keyboard history. The
saved opt-out remains; absent controllers allow a safety return to touch. Desktop input
selection is unchanged because this touch policy is opt-in.

[design] The touch overlay hides on accepted controller use and returns on accepted touch
use or loss of the active controller. Controller connection alone does not hide it. The
existing Controls page handles remapping and button style; no independent binding table
is introduced. Actual USB/Bluetooth devices, labels, hot unplug and interruption resume
must still be exercised on Android before support is claimed.

## Qualification packet

[implementation evidence: source-only implementation checks] The metadata and activity Java sources
compile against Android API 35 with JDK 17. Rust source has been parser-formatted, but
its Android callback, locked dependency resolution, lint, App tests and physical-device
routes remain unrun. The authored App fixtures include real queue-to-RawGamepadEvent
processing, remapping/prompts, menu translation, overlay transitions, physical-arrow
ownership, disconnect/focus/overflow, mixed-pad invalidation and both resume metadata
poll orderings. They are not claimed as execution evidence. The target test device's
controller is generic Bluetooth with Xbox-style layout; unknown names use the existing
Xbox prompt fallback without a Microsoft vendor requirement. Rumble remains unqualified.

## Combined aiming integration

[design] The combined controller/touch-aiming candidate preserves the native sneak
toggle underneath a touch-only momentary hold. Controller selection cancels touch
contacts and aim latches and clears the held-sneak source after lifecycle release
synchronization. Both normal controls and same-frame movement preview consult the
optional TouchDeviceSwitch action-consumption gate, so the first touch after
controller use restores the overlay without buffering aim, sneak or editor input.
An active touch contact keeps the existing automatic-selection busy policy; explicit
device selection still cancels it. The combined regression covers both independent
sneak-base values, accepted controller activity, explicit takeover of a held touch,
and first-touch return followed by fresh working actions. These are authored tests
until the combined source is admitted for compilation/execution.

[documented: engine/vendor/winit/Cargo.toml and LICENSE] The verified winit 0.30.13
archive declares Apache-2.0 and includes four example PNGs used by examples/window.rs.
They are retained byte-for-byte with their upstream licence; they are not runtime
game assets. Visual source review found two cursor crosses, a gradient and the winit
mascot icon. Their exact hashes are recorded beside the existing vendor allowance in
.github/workflows/game-data-guard.yml. The existing vendor rule is unchanged.

## First controller bundle with the optional Pip-Boy handle

[design] This composition starts from the public R7 physics/packaging base. It changes
no physics source or vendored Rapier bytes. The controller bridge, touch-only aiming
and optional drawer retain their separate source/provenance packets. The drawer is
canceled on controller takeover and hides with the gameplay overlay. The first touch
from Controller restores the overlay only, including a touch beginning on its handle.
A committed drawer request cancels aim/contact latches and the touch-only held-sneak
source before normal action readers run; the independent physical/controller sneak
base survives. The request still goes through OpenInventory and its existing locks.
No HUD sizing, SVG, adaptive performance, dialogue or minigame follow-up joins this
first bundle. Its new regressions remain authored until qualification is admitted.

## Review revision 2: commit and native fallthrough ownership

[design] A successful drawer release ends the touch source's entire current message
batch. Remaining fresh contacts in that batch cannot rearm Fire, toggle aim or held
sneak before the ordinary UI menu consumer runs. This clears only that touch reader;
physical key/mouse ownership and the independent raw device selector remain intact.

[documented: engine/vendor/winit/src/platform_impl/android/mod.rs] Unclaimed native
motion/key events continue into winit's ordinary touch/keyboard translation. A late
controller sample rejected by the controller queue must therefore be claimed and
dropped, rather than returned as unhandled. The shared ingress decision distinguishes
admitted, dropped and ordinary-fallback events. Explicit joystick/gamepad sources and
known retired-controller D-pad samples cannot leak into touch/keyboard. Real mouse,
touch, keyboard and unknown OS-reserved keys retain their upstream fallback. These
new decisions require fresh native compilation and executable regression admission;
the earlier 41-test compilation applies only to the preserved first revision.
