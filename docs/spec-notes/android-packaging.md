# Android launcher identity and APK privacy

Clean-room scope: this note describes NVForever's own Android port design, not the original game. Here `[hypothesis]` marks a port design proposal or pending device acceptance, and `[documented]` cites repository source, an owner request, a pinned dependency or public platform documentation; neither is a claim about Fallout: New Vegas (see the [classification](../design/android.md#clean-room-classification-of-the-ports-constants-and-behaviour)).

## Sources and intended behavior

- [documented] The project's own launcher artwork is
  `engine/nvforever/assets/icon.png`; `engine/nvforever/src/window_icon.rs`
  embeds it for desktop. Android adaptations must derive from that asset.
- [documented] Android adaptive icons use 108 dp foreground/background layers,
  with important artwork inside the central 66 dp safe zone. Launchers may apply
  different masks. Android 13 supports a monochrome layer for themed icons.
  [Android adaptive icons](https://developer.android.com/develop/ui/compose/system/icon_design_adaptive).
- [hypothesis] Center the complete canonical artwork within the 66 dp safe circle on
  an opaque near-black adaptive background. Supply legacy density variants and
  an alpha-derived monochrome layer; do not redraw the brand or use game art.
  `tools/android/LauncherIcons.java` is the reproducible source adaptation.
- [design] Neither debug nor release packages contain the LAN diagnostics UI or
  uploader. The manifest denies cleartext traffic. `INTERNET` permission remains
  for the launcher's optional HTTPS project news and GitHub Release update checks,
  not for telemetry or uploads. `REQUEST_INSTALL_PACKAGES` and the non-exported
  read-only `ApkUpdateProvider` are declared solely for user-confirmed in-app
  ARM64 APK updates downloaded over HTTPS from this repository's GitHub Releases.
- [documented] Local native crash/event logs (`engine/nvforever/src/crash.rs`,
  `events_log.rs`) and the local [render observer](../design/android-render-admission.md)
  are separate from player reports. Android startup disables player reports.

## Validation boundary

- [hypothesis] Inspect packaged icon references, each density, adaptive masks
  and monochrome alpha against the canonical source. Compile production Java.
  Inspect the merged manifest and every DEX in the signed debug and unsigned
  release packages. Check application/update/save identity separately.
- [hypothesis] Keep implementation checks separate from device acceptance.
  Launcher appearance and touch ownership on a physical device require a device
  run; source or package inspection alone does not establish those outcomes.
