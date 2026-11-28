# Android native-window loading cover

[qualification] The R6 phone report describes a visible main menu with neither touch nor virtual
buttons responding. Automated native input tests passed, but did not exercise the
Android window that supplies the loading-cover acknowledgement.

[documented] Android's NativeActivity takes its window surface and input queue.
ViewRootImpl skips Java drawing for an application-owned surface. A child added
with addContentView therefore cannot provide the hardware frame-commit callback
used by the cover protocol. See the Android 15 implementations of
[NativeActivity](https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/android15-release/core/java/android/app/NativeActivity.java)
and [ViewRootImpl](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/android15-release/core/java/android/view/ViewRootImpl.java),
and the hardware-only [frame-commit contract](https://developer.android.com/reference/android/view/ViewTreeObserver#registerFrameCommitCallback(java.lang.Runnable)).

[documented: `GameActivity.java`, `LoadingState.java`] The historical R6 source requires both that cover commit and a matching native
content-submission witness before revealing content. An absent cover commit keeps
LoadingState blocking and the shared PlatformInputGate closed, including native
menu pointers, menu action buttons and wrapper controls. This source defect
provides a source-level explanation to check against the reported failure;
physical confirmation remains required.

The recovery uses a separate ordinary application dialog window for the cover.
Configure hardware acceleration before its content view. Keep it nonfocusable and
nontouchable so the native activity retains focus and its original surface/input
queue. Remove dim-behind. The platform gate continues quarantining game input;
the cover does not manufacture input events or consume native queue ownership.
Window lifecycle and generation checks reject callbacks from dismissed covers.
Real cover frame commitment and the native request/epoch/surface/content witness
remain required; no timer, posted callback or unconditional gate bypass stands in
for either. Back/refusal recovery remains in a separate interactive dialog.

[documented] Dialog creates an independent PhoneWindow and attaches its decor
through WindowManager. Relevant contracts are
[Dialog](https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/android15-release/core/java/android/app/Dialog.java)
and [WindowManager flags](https://developer.android.com/reference/android/view/WindowManager.LayoutParams).

The historical labeled Java recovery APK retained the qualified R6 native library
pair. Its SOURCE identified the Java/package revision; NATIVE-SOURCE identified
the unchanged native CI revision. New packaging still requires current source-bound
native/final APK qualification. Local window-policy tests and SDK compilation do
not substitute for a fresh phone main-menu, interruption and loading test.

## Completing only after logical cover hiding

[design] `GameActivity.revealIfReady` delegates its queued callback to the shared
`LoadingReveal` protocol. Both current Java-cover and native-content witnesses
remain necessary. The protocol checks the current state, request/epoch, cover and
lease generation, asks that lease to hide, and inspects the backend postcondition
before committing completion or sending the existing success callback. A failed
or unavailable hide postcondition keeps loading blocked and uses the existing
failure/recovery callback. Existing lifecycle handling still owns watchdog
scheduling; the protocol runs its completion callback only on success. Reentrant callbacks that
retire the episode, activity or cover cannot complete or fail the newer owner.
There is no timer-based success, repeated hide retry or hardware-input reset.

[documented] Android's `Dialog.hide` changes decor visibility, while `isShowing`
reads that visibility. The cover backend uses these existing APIs. See the
[Android 10](https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/android-10.0.0_r1/core/java/android/app/Dialog.java)
and [Android 15](https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/android-15.0.0_r1/core/java/android/app/Dialog.java)
implementations. This postcondition is logical window visibility; it does not
prove compositor output, physical pixels or native input responsiveness.

[qualification] Paired Java 17/SDK 35 checks compiled the complete production Java
caller and test sources with diagnostics disabled. The production-equivalent
baseline passed five named boundary cases and failed six actual assertions; the
correction passed the same eleven named cases. The unchanged handoff and window
mains passed 41 and 49 checks respectively in both variants. The candidate's
actual no-argument workflow entry also executed all eleven cases, totaling 169
checks. Original failure oracles and old test bytes were retained.

These are Windows JVM checks using the real shared protocol, `CoverLease` and
`LoadingState` with an authored backend. They do not instantiate Android's dialog
at runtime or establish visible resume/input recovery on a phone. The historical
`NATIVE_CONTENT_SUBMITTED` event remains evidence of admission and a hide attempt;
its old meaning is not strengthened by this later correction. The reported resume
hang and Doc Mitchell exit still require current APK/device acceptance.


[design: `ui_pointer.rs`] After a covered/focus transition, the retained mouse
coordinate must not redirect a new admitted touch on the main menu. When no
admitted left-mouse hold/press/release owns the pointer, use the current admitted
touch position, including its ordinary release frame, before passive mouse hover.
Keep mouse hold/release priority, controller pointer sleeping and platform/sheet
contact filtering. No hardware state, loading epoch or raw input stream changes.

[qualification] The separate retained-cursor source proposal adds real Taffy,
raw-input and Main Menu action controls. Its baseline/candidate outcomes are
uncompiled/unrun predictions. This source path is not evidence that it caused the
reported R10 resume hang or that an actual Android window recovered. The existing
Java protocol and all-DEX checks retain their prior CPU/structural scope.
