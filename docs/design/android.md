# Android port (#2436)

Status: experimental implementation; device acceptance is pending. This document describes
NVForever's own platform choices, not behavior inferred from Bethesda code.

## Runtime boundary

[documented: Bevy 0.20.1 mobile example](https://github.com/bevyengine/bevy/blob/v0.20.1/examples/mobile/android_basic/Cargo.toml)
Android loads a `cdylib` through NativeActivity and Bevy's `#[bevy_main]` entry point.
The tiny `nvf-android` package supplies that entry; `nvforever` is a shared Rust library
with its ordinary desktop binary. Desktop builds do not produce an Android shared library.

[design] The initial package targets ARM64, Android 10/API 29 or later, and Vulkan 1.1.
This is a build target, not a supported-device or frame-rate guarantee. Android UI uses
the device's display density; the desktop's fixed screenshot scale is retained on desktop. It starts with
MSAA off and the existing vanilla graphics preset. APKs ship code, the project's runtime
resources and licensed artwork, but no original game content. Android has no player-report
collection or submission path.

## Files and import

[documented: Android Storage Access Framework](https://developer.android.com/training/data-storage/shared/documents-files)
The launcher asks the user to choose a game folder or Data folder. It requests read
access and copies the Data tree, plus Fallout_default.ini when the game root was chosen,
into a unique directory in the app's external files directory. File streams are bounded
buffers; source documents are never opened for writing. Providers with recursive IDs,
unsafe names or unreadable files fail the import. The existing selected install remains
selected until the new copy completes and the configuration is atomically replaced.

[design] Only UUID directories directly under this app's external files directory,
marked by the importer, can be removed as failed or superseded imports. The currently
selected import is protected. Cleanup does not follow symbolic links. The user needs
space for both the old and new copies until replacement completes. App uninstall removes
app-owned files, including saves; export important saves before uninstalling.

[design] Settings, logs, saves and generated caches use the internal `files/home` directory
via `NVFOREVER_HOME`. Schema files are extracted from this APK to `files/runtime/schemas`
before parsing engine arguments, and translations to `files/home/lang`. Game input and
engine output stay separate, retaining the existing write guards.

## In-app APK updater

[design] The Android launcher (`crates/nvf-launcher/src/mobile.rs`,
`android/app/src/main/java/org/nvforever/android/{MobileLauncher,UpdatePolicy,ApkUpdateProvider,LauncherActivity}.java`)
offers an optional, user-confirmed in-app APK updater modeled on the desktop launcher's
updater (`crates/nvf-launcher/src/update.rs`):

- **Where it checks and downloads from:**
  - Release metadata is queried from `https://api.github.com/repos/LootJunkieAzza/NVForever/releases?per_page=10`
    (the same `news::RELEASES` endpoint used by the desktop launcher). Draft releases are ignored.
  - Both the selected ARM64 APK and `SHA256SUMS.txt` must have exact URLs under
    `https://github.com/LootJunkieAzza/NVForever/releases/download/<tag>/<name>` with
    strict ASCII safe-component validation (`repo_asset_url` in `mobile.rs` and
    `UpdatePolicy.isRepoReleaseAssetUrl` in Java).
  - All requests use HTTPS (`HttpsURLConnection` with Android's system trust store) and
    `AndroidManifest.xml` enforces `android:usesCleartextTraffic="false"` across both debug
    and release builds. Redirects (such as GitHub's 302 to its asset CDN) are handled
    manually up to 5 hops (`UpdatePolicy.MAX_REDIRECTS`) and refused unless every hop stays
    on `https://` with no userinfo and one of four pinned GitHub release hosts:
    `github.com`, `objects.githubusercontent.com`, `release-assets.githubusercontent.com`, or
    `github-releases.githubusercontent.com`.
- **How it verifies what it downloads:**
  - Asset selection (`select_android_apk` in `mobile.rs`) filters release assets by filename
    and repository URL — selecting ARM64 `.apk` candidates under the official release URL
    while excluding names containing `unsigned`, `x86`, or `armeabi` (a filename/URL filter,
    not by itself cryptographic signature verification).
  - **Fail-closed SHA-256 integrity check:** direct in-app download always requires a
    published `SHA256SUMS.txt` asset on the same release with a valid 64-hex-char SHA-256
    entry for the selected APK (matching desktop `update.rs`) to catch download corruption
    or truncation. If `SHA256SUMS.txt` is absent, fails to fetch, or omits the APK,
    `check_update` omits `asset` so the launcher offers only `"Open release page"` in the
    system browser instead of an unverified download.
  - `MobileLauncher.downloadAndInstallUpdate` re-checks the URL and 64-hex SHA-256 before
    opening a socket, enforces a 512 MiB cap (`UpdatePolicy.MAX_APK_BYTES`) and non-empty
    download check, computes SHA-256 while streaming into `cache/updates/<name>.part`,
    fsyncs, verifies the digest in constant time (`UpdatePolicy.checksumMatches`), deletes
    `.part` on any error or cancellation, and only then atomically renames to
    `cache/updates/<name>`.
  - **Pre-install package, signing-certificate, and versionCode authenticity check:**
    before handing the downloaded APK to Android's package installer, `MobileLauncher`
    inspects the archive with `PackageManager.getPackageArchiveInfo(..., GET_SIGNING_CERTIFICATES)`
    (API 29+) and fails closed via `UpdatePolicy.verifyApkMetadata` — deleting the APK —
    unless (a) `archiveInfo.packageName` equals the installed app's `packageName`,
    (b) the archive's signing certificate SHA-256 digest set matches the currently installed
    app's signing certificate set, and (c) `archiveInfo.getLongVersionCode()` is strictly
    greater than the installed app's `longVersionCode` (`android/app/build.gradle` derives
    a monotonic `versionCode` from the release version/revision or `NVF_ANDROID_VERSION_CODE`
    so downgrade protection applies on every release).
  - **System installer handoff:** the verified APK is exposed read-only (`mode == "r"`) via
    the non-exported `ApkUpdateProvider` (`android:exported="false"`,
    `FLAG_GRANT_READ_URI_PERMISSION`, canonical path check inside `cache/updates/`) to
    Android's system PackageInstaller (`Intent.ACTION_VIEW` with
    `application/vnd.android.package-archive`), which requires user confirmation (and
    `"Allow from this source"` permission on first install) and enforces Android's own
    upgrade signature and downgrade checks.
- **User control & opt-in:**
  - Startup update checks run only when project news fetching is enabled
    (`[Launcher] News != 0`, which the user can turn off in the launcher); users can also
    trigger `"Check for updates"` manually from the Home page.
  - No APK is ever downloaded or installed automatically: an explicit tap on `"Update"` in
    the confirmation dialog is required to start the download (and `"Cancel download"` aborts
    it immediately), and the game must not be running.

## Loading and process lifetime

[documented: `engine/nvforever/src/handoff.rs`] The desktop engine starts a new process to
load a save or return to the main menu, waiting for pending save writes first.

[design] Android has no game executable to spawn. The engine asks a non-exported restart
activity in the launcher process to terminate only this UID's identified `:game` process.
The activity waits until that process has disappeared before starting a fresh NativeActivity
with the successor arguments. Its NEW_TASK | CLEAR_TASK flags replace the old game task
instead of reusing a retained singleTask activity/Intent ([Android Intent flags](https://developer.android.com/reference/android/content/Intent#FLAG_ACTIVITY_CLEAR_TASK)). Desktop handoff markers and hidden-window placement are
omitted. Restart activities cannot be invoked by other apps. The game activity is a
single task, so selecting Start again cannot create two native engines in one process.
This path needs device testing for load failures, task switching and OS process death.

## Input and remaining qualification

[design] Touch buttons aggregate their held inputs with independently recorded physical
key/mouse events after Bevy's input systems. Releasing either source keeps an action held
until both sources release, preserving the combined press/release edges, including taps
whose press and release both arrive in one frame. Within a frame, hardware events are
processed first and then touch events, each in its own receive order. They use current keyboard bindings for game actions and ordinary navigation keys
in menus. Drag the lower left to move and the right free area to look. Pause, Pip-Boy,
VATS, reload, jump, activate, aim, attack and sneak have buttons. Menu arrows, confirmation,
back, tab and page navigation have buttons. External keyboard/mouse use the existing path.
Controllers, soft-keyboard text entry, accessibility and a fully configurable touch layout
remain unqualified. Background audio/pause and graphics surface resume need device tests.

Do not mark #2436 complete on the strength of compilation alone. Acceptance requires an
installed APK on representative hardware: import a lawful install, start/new game, move,
look, activate, dialogue and menu navigation, save/load/return to menu, audio, background
and resume, restart/process death, a malformed import, and sustained memory/thermal behavior.
Physical-device qualification is pending.

## Touch layout revision after initial device testing

[NVForever device test: user, 2026-11-13; not an original-game observation] The first device build reached the opening interior.
The user found the invisible movement area unclear and the text-only action grid crowded.
Leaving the interior exposed a Rapier arrival-query panic; that is under separate physics
regression work, not evidence of a GPU or memory failure.

[documented] Activision's [mobile control guide](https://blog.activision.com/au/en/call-of-duty/2019-10/Getting-a-Grip-on-the-Call-of-Duty-Mobile-Controls) describes a virtual stick and an editor for position, size and opacity.
PUBG's [control guide](https://pubgmobile.helpshift.com/hc/en/3-pubg-mobile/faq/37-what-are-the-controls/?l=en) describes left-side movement, free-area look and icon actions. These inform common interaction conventions; no artwork or implementation is copied.

[design] The revised overlay draws its own geometric action icons at runtime, shows a
movement base and thumb position, and lights held buttons. Help explains the two thumb
areas and toggles action names. The cog opens a layout editor, releases held gameplay
inputs and requests the ordinary pause menu. Drag selects/repositions a control; explicit
buttons resize it, adjust opacity, show/hide secondary actions, mirror hands, choose a
fixed/floating stick, edit menu controls, reset, save or cancel. The save is versioned
JSON in the engine's own home; write guards and atomic rename protect its previous copy.
Cancelling restores the original layout. Visual bounds and hit testing share geometry;
action targets retain a minimum 48 logical-pixel diameter on a phone. Menu navigation
remains separate from gameplay actions. Editing stays paused until the player resumes.

## Clean-room classification of the port's constants and behaviour

[design] Every number and rule the Android port adds is an NVForever engine/port design
choice, not a claim about how Fallout: New Vegas behaves. None is backed by an
observation of the original game, and none needs to be, because the original game has
no touch, phone, launcher or Android behaviour to match. In the `docs/spec-notes/android-*.md`
notes, `[hypothesis]` marks a port design proposal or pending device acceptance, and
`[documented]` cites this repository's source, a pinned dependency or public platform
documentation. Neither tag there claims anything about the original game. The groups are:

- Touch geometry and gesture timing: 48-logical-unit minimum targets and rows (52 and 51
  for hand-built workbench and repair rows), 12-unit drag/tap slop (`menu/normal_touch.rs`,
  `pipboy/data/touch_map.rs`, `maintenance_touch.rs`, `menu/barter_touch.rs`,
  `touch/world_activate.rs`), 0.35 s double tap (`touch/world_activate.rs`), 0.6 s hold
  to activate (`activate/touch.rs`), 0.45 s VATS hold (`vats_mode/touch.rs`), the
  0.1/0.18/0.25/0.35 stick dead zones (`touch/aiming.rs`), the Pip-Boy swipe directions
  and distances, and the fixed, floating and legacy stick schemes.
- Sizes and defaults: phone dialogue text base 16 logical units against 20 on desktop
  (`dialogue_touch.rs`), 85% choice and 100% subtitle defaults on Android, 50–150% menu
  text sizes (Android defaults 125% for barter, repair and workbench menus via
  `menu_readability.rs`, desktop 100%), `settings-menu-scale-v1` 80–140% list defaulting
  to 100% (exact desktop pause/settings menu layout with touch enhancement directly over
  it, `pause.rs`), `hud-scale-v2` 25–250% in 5% steps (46 choices, `interface.rs`),
  75–150% lockpick sizes (`lockpick/touch.rs`), 0.85–1.35 Pip-Boy presentation scale
  (`pipboy/touch_presentation.rs`), the 78-unit appearance preview and the eight-pixel
  split gap, Android-only defaults (gyro aiming On with X and Y axes inverted, tap-on-VATS
  On, lockpicking hints Off, Pip-Boy button overlays Off, Pip-Boy downward dismissal Both,
  bottom swipe handle On, touch maintenance On), upper-viewport placement of text-entry
  boxes (`NameMenu` at 8% top padding, `Console` at top 4%) on soft-keyboard builds, and
  a 15 s display window followed by a 1.5 s fade-out before automatically turning off
  default touch overlay hints and button text labels (`touch.rs`).
- Bounds and limits: at most 128 queued menu-sheet edges and 32 owned contact IDs
  (`touch/menu_sheets.rs`), 32 queued maintenance requests (`maintenance_touch.rs`),
  16 retired VATS marks and 32 points (`vats_mode/touch.rs`), 16 touch-map edits, 8 layout
  profile slots and 128 KiB profile files (`touch/profiles.rs`), 64 tray axis cells
  (`touch/hotbar_layout.rs`), 512-pixel and 8 MiB SVG raster limits (`touch/icons.rs`),
  a 4096-byte companion-wheel settings read, the controller event queue of 512
  (`android_controller.rs`), the render admission limits (128 skins, 32 rigid bodies,
  8 bursts, 8 idle frames, `android_render_admission.rs`), 32 render diagnostics reports,
  16 Pip-Boy UV attempts (`dialogue_pipboy.rs`) and the eight NIF rejection samples
  ([NIF numeric admission](../spec-notes/nif-numeric-admission.md)).
- Timeouts and lifetimes: thermal sampling (10 s, 25 s stale), gyro sample gap
  and age (50 ms, 100 ms), and the loading-cover ticket, lease and generation
  checks ([loading cover](android-loading-cover.md)).
- Mode and layout policy: the Mobile settings tab and its placements including Mobile
  performance (`crates/nvf-settings-layout`, [settings page](settings-page.md)),
  Battery/Balanced/Quality performance presets with 30 or 60 FPS caps
  (`mobile_performance.rs`), borderless and one MSAA sample as Android start defaults,
  Android-only defaults (gyro aiming on with both axes inverted, tap-on-V.A.T.S. on,
  150% Barter/Repair/Workbench menu size with 50%–200% range, 50%–200% Android menu
  scale, 80%–200% pause Settings menu UI scale defaulting to 100%, lockpicking hints off,
  Pip-Boy button graphics off, downward Pip-Boy dismissal enabled on both sides, bottom
  swipe handle on, and 15-second auto-fadeout for default touch hints), the five dialogue
  styles, speaker side and safe-area framing, compact 0.78× default text scale in Wrist
  Terminal and Classic dialogue, enlarged Wrist Terminal framing around the glass with
  tappable power-button quick-exit (enabled only when a normal goodbye option exists),
  standalone touch hit-testing and backdrop-cancel for tiled and hand-built QuantityMenu
  over Barter/Container/Inventory, larger category arrows and horizontal swipe on Barter
  and Workbench category headers, tapping the Pause touch button while paused to toggle
  the Pause menu closed, a compact top-left 44×44 logical-pixel touch-overlay Hide/Show
  toggle button that hides all other touch controls and lets touches pass through
  everywhere else on screen until restored (`touch.rs`), full touch controls on the
  character appearance customizer (`creator.rs`), upper-viewport Android text entry
  placement (`chargen/name.rs`, `console.rs`), explicit soft-keyboard suppression on
  non-text `FormOver` overlays (`soft_keyboard.rs`, `FormTextInput`, `AndroidManifest.xml`),
  touch input ownership and cancellation, touch controls reaching scripts through
  `IsControlPressed` as their bound control without inventing physical keys, and the
  launcher's pages, top-left app icon, read-only folder import, newest-save Continue
  with thumbnail/metadata preview card (`mobile.rs`, `MobileLauncher.java`), Markdown
  news/release formatting (`MarkdownParser.java` / `MarkdownRenderer.java`,
  700/400-character preview caps), user-confirmed HTTPS GitHub Releases ARM64 APK
  updater (`UpdatePolicy.java`, `ApkUpdateProvider.java`, 512 MiB cap, 5 HTTPS-only
  redirects to pinned GitHub release hosts, mandatory fail-closed `SHA256SUMS.txt`
  verification), and Google's
  Maven Central mirror ahead of `mavenCentral()` in `android/settings.gradle` as a
  CI rate-limit resilience fallback (`google()` remains first, `mavenCentral()` and
  `gradlePluginPortal()` remain fallbacks).
- Classic dialogue casing: the 1774 × 887 frame and its four apertures
  ([Classic runtime](dialogue-classic-runtime.md)) are project design, not original-game
  observations. The entire `dialogue/classic.png` image stretches cleanly to fill 100% × 100%
  of the mobile viewport (zero black borders, no edge-only strip stretching) with the live
  portrait, speech, responses, caps and TRADE openings mapped proportionally inside the
  stretched layout (keeping interactive text/controls inside safe insets). Original
  AI-generated artwork created by the PR author using ChatGPT image generation (not
  extracted or derived from *Fallout: New Vegas*, Bethesda assets, or any third-party mod)
  and contributed under the repository's `GPL-3.0-or-later` licence (carrying no upstream
  third-party copyright/licence restrictions). Its TRADE control opens the existing barter menu.
- Round-2 and Round-3 mobile defaults and layout rules (`[design]`): Android defaults
  double-tap look space for VATS (`touch-vats-double-tap-v1`) On, auto-move lock target
  (`touch-auto-move-lock-v1`) On positioned `1.95 × stick_radius` above the movement
  stick, horizontal/mouse and vertical look sensitivities to `1.25` (desktop stays `1.0`),
  Gyro default position at middle-left `(0.075, 0.48)` (mirrored in southpaw), compact 5×3
  top/bottom-dockable and collapsible touch-editor toolbar that auto-hides while dragging,
  virtual on-screen navigation keys active in Pause and Settings menus alongside direct
  touch, viewport-fit clamping for Main Menu entries, the character appearance customizer
  (`creator.rs` / `creator/view.rs`: pinned header, tabs and action bar with scrollable row
  viewport inside safe insets and a 168-unit minimum row budget), and sibling chargen,
  level-up and message-box modals (`chargen.rs`, `chargen/name.rs`, `levelup.rs`,
  `message_menu.rs`) above `100%` `menu-scale` or within non-zero safe-area insets (keeping
  desktop `<= 100%` with zero safe insets unchanged), enlarged
  straight-on Wrist Terminal and Retro Console side split (`dialogue-device-side-v1`,
  Right default / Left selectable, 66% width × full safe height, area-weighted normal,
  front-depth clearance, opaque dark screen backdrop, dialogue glare skipped, and
  density-scaled tappable power-off quick-exit on both devices), and a retro-terminal
  styled top-right next-release countdown badge (`CountdownFormat.java`,
  `MobileLauncher.java`) in the Android launcher.
- Wrist Terminal fallback policy (`[design]`): Wrist Terminal reads the worn Pip-Boy
  from NVForever's live player state; when no worn Pip-Boy is present (such as a new
  character before receiving one), dialogue falls back to Normal without altering the
  saved theme so conversation stays playable.
- Engine library choices: the vendored Android `winit` and Rapier 0.36.1 query refresh
  ([query refresh](../spec-notes/query-refresh-island-lifecycle.md)), the DDS decode
  fallback when BC sampling is missing ([DDS](../spec-notes/dds.md)), NIF numeric
  admission, owned SVG icon families and the launcher icon adaptation.

[design] No text the game shows is transcribed. Touch labels, help, launcher text and
companion-wheel refusal messages are NVForever's own wording. Text from the game (dialogue,
item names, menu settings) is read from the player's installation at run time, and the
Terminal and Pip-Boy models and Font 5 come from that installation; the APK contains none
of them.
