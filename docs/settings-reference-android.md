# Android settings reference supplement

These additional rows and compatibility notes belong to the Android-integrated
engine. The shared desktop reference remains [settings-reference.md](settings-reference.md);
the desktop launcher's four editable tabs and its built-in table are unchanged.
The Android launcher reads the Mobile catalogue exported by a normal player launch;
actions such as layout editing and practice still open in the game. Android rows keep
their existing in-game placements in desktop builds; selecting a physical input
device does not change the Android build's Mobile grouping.

## Mobile › Touch layout and guide

Android groups these actions under Mobile; desktop keeps them in Controls › Touch.

### `touch-layout-editor-v1`: Customize layout

Opens the existing control editor over the paused Settings page. Move or resize
controls and change opacity, aiming or profiles. Save uses `touch-controls.json`;
Cancel or Escape restores the original draft. Both return to Settings. The action
does not persist an open session and refuses another active form or a window
smaller than 400 × 300 logical units.

### `touch-practice-v1`: Controls help / gesture practice

Opens isolated movement, aiming and gesture lessons without sending gameplay
actions. Close or Escape returns to Settings. Its existing saved key is unchanged.
These actions replace the removed persistent settings/help shortcuts on the HUD.

## Mobile optional interaction preferences

These rows use the normal `[Settings]` INI consumer. Enhanced views, hints,
appearance and the existing text-size/confirmation choices are independent.
Android uses the Mobile headings below; the desktop settings layout retains
Gameplay > Dialogue/Inventory menus and Controls > Touch.

| Mobile heading | Key | Behavior |
|---|---|---|
| Inventory and quick access | `touch-terminal-v1` | Enhanced terminal view; Off retains the standard menu/input path. |
| Inventory and quick access | `touch-companion-v1` | Enhanced companion wheel; independent of drag-release preference. |
| Inventory and quick access | `touch-pipboy-dismiss-side-v2` | Downward drags outside the display: Both (Android default), Disabled (desktop default), Left only or Right only. Legacy side switches still load and save; the chooser replaces their visible rows. |
| Inventory and quick access | `touch-pipboy-dismiss-travel-v1` | Arm-lowering drag distance, 48–240 logical units, default 120; capped by remaining safe space. Reverse while held to raise it again. |
| Inventory and quick access | `touch-pipboy-close-button` | Optional explicit Close target; off by default. |
| Inventory and quick access | `touch-pipboy-button-graphics` | Show overlay paint; Off by default on Android (On on desktop) while keeping identical active targets and dispatch. |
| Inventory and quick access | `touch-pipboy-model-wheel` | Touch the physical model wheel to scroll; on by default, independent of mouse/pad scrolling. |
| Movement and gestures | `touch-vats-targeting-v1` | Tap (Android default), Off (desktop default) or Hold; body selection queues, separate Fire confirms. |
| Dialogue | `dialogue-theme-v1` | Normal (default), Classic, Field Notes, Retro Console or Wrist Terminal. Minimal, Classic Glass and Split Panel load as Normal, Classic and Field Notes. |
| Dialogue | `dialogue-device-side-v1` | Place Wrist Terminal / Retro Console on the Right (default) or Left side of the screen while framing the speaker opposite. |
| Dialogue | `dialogue-background-tint-v1` | HUD colour (default), Neutral or Warm amber backing; text and selection retain the chosen HUD colour. |
| Dialogue | `dialogue-background-opacity-v1` | Backing opacity, default 18%, maximum 80%; the stored slider is normalized. |
| Dialogue | `dialogue-frame-opacity-v1` | Accent opacity, default 60%; independent of backing and full-contrast text/feedback. |
| Dialogue | `dialogue-hints-v1` | Show explanatory dialogue hints, off by default; choices, speech and requirements stay. |
| Lockpicking | `lockpick-touch-hints` | Show lockpicking hints, Off by default on Android (On on desktop); scheme, Turn, status and errors stay. |

The backing, tint and accent controls apply to Field Notes. Classic uses a
locally supplied template; Retro Console and Wrist Terminal use the installed
models. When their presentation is unavailable, Normal preserves access to the
same conversation without overwriting the selected theme. These choices do not
change dialogue words, offered order or actions. Registration is not device acceptance.

## Input preferences alongside Tweaks

[design] The explicit custom left/right stick-deadzone preference supplies its
per-stick values only while enabled. Otherwise the separately saved generic
controller calibration applies. Neither option rewrites the other preference.
Keyboard/mouse with a selected controller remains opt-in: actual physical holds
stay independent of controller aim/sneak toggles and of virtual touch input.
Movement and look device transforms are applied before the independently
configured gyroscope contribution. Device/cover/focus cancellation also clears
deferred reload input and stale held Jump state.

These are the integrated source contracts and authored control boundaries;
current-head compiler/runtime and real Bluetooth-controller acceptance are pending.

## Display › Interface

- Compatibility row: the main list and its saved names remain registered; the wider `hud-scale-v2` row represents it in the combined settings page.

### `hud-scale-v2`: HUD scale

- On the page: Size of the in-game HUD, from 25% to 250%. Menus have separate sizes. Takes effect at the next start.
- A slider with 46 choices, in 5% steps. The stored value is the slider part, rather than the percentage shown on the page.
- A missing new key reads the older numeric `ui-scale` value with its original 50–150% interpretation, or Main's named 50–100% choices, preserving the player's HUD size. An explicit new key wins. The independent `menu-scale` key retains its original units and precedence.

### `settings-menu-scale-v1`: Settings menu UI scale

- On the page: Size of the pause settings menu, independent of Menu scale. Takes effect immediately.
- A 7-choice list: `80%`, `100%` (default, identical to desktop proportions), `120%`, `140%`, `160%`, `180%`, `200%`.

### Existing-menu sizes

- `barter-menu-size-v1`: Buying and selling size.
- `repair-menu-size-v1`: Repair menu size.
- `workbench-menu-size-v1`: Workbench menu size, including campfires and reloading benches.
- Each slider has 31 choices from 50% to 200%, in 5% steps, saved independently. Android defaults to 150%; desktop defaults to 100%. These sizes are for the ordinary menus; their text, row layout, scrolling and hit geometry must remain aligned.
- `menu-scale`: On Android, a 16-step slider from 50% to 200% (10% steps, default 100%); desktop keeps its 50%–100% list.
- [qualification] Settings/migration controls are authored but unexecuted. Actual menu rendering/drag integration is pending its owning consumer qualification; these settings alone do not establish working menu sizing.

## Android placement by stored key

The shared layout assigns the following named settings. Rows that cannot apply keep
their stored value; the settings page shows why they are disabled. The launcher uses
the exported catalogue after a normal Android start. In-game actions and the live
thermal status are not editable in the launcher.

| Android placement | Stored keys | Notes |
| --- | --- | --- |
| Mobile > Touch layout and guide | `touch-layout-editor-v1`, `touch-practice-v1`, `touch-icon-family`, `adaptive-touch-actions` | Edit or practice without sending gameplay actions. Icon family selects the artwork; adaptive controls follow context. |
| Mobile > Movement and gestures | `touch-analog-movement-v1`, `touch-fire-drag-look-v1`, `touch-auto-move-lock-v1`, `touch-vats-double-tap-v1`, `touch-vats-targeting-v1`, `touch-world-activation-v1` | Configure stick movement, swipe aiming, running, V.A.T.S. targeting and interacting with the world. Android defaults `touch-auto-move-lock-v1` and `touch-vats-double-tap-v1` On and `touch-vats-targeting-v1` to Tap; desktop defaults remain Off. |
| Mobile > Gyroscope | `touch-gyro-v1`, `touch-gyro-recenter-v1`, `touch-gyro-toggle-v1`, `touch-gyro-ads-only-v1`, `touch-gyro-invert-x-v1`, `touch-gyro-invert-y-v1`, `touch-gyro-sensitivity-v1`, `touch-gyro-ads-v1` | Android defaults gyro aiming On with both X and Y axes inverted and places the Gyro control at middle-left; desktop defaults Off. Gyroscope off leaves the secondary values saved. |
| Mobile > Inventory and quick access | `touch-hotbar`, `touch-hotbar-slots`, `touch-hotbar-tray`, `touch-tray-editor-v1`, `touch-hotbar-size`, `touch-pipboy-swipe-v1`, `touch-bottom-swipe-target-v1`, `touch-pipboy-distance-v1`, `touch-pipboy-size-v1`, `touch-pipboy-tab-swipes`, `touch-direct-pipboy-v1`, `touch-pipboy-dismiss-left`, `touch-pipboy-dismiss-right`, `touch-pipboy-dismiss-side-v2`, `touch-pipboy-dismiss-travel-v1`, `touch-pipboy-close-button`, `touch-pipboy-button-graphics`, `touch-pipboy-model-wheel`, `touch-barter-v1`, `touch-maintenance-v1`, `touch-terminal-v1`, `touch-companion-v1` | Quick slot count is a 4/6/8 list. Android defaults bottom swipe handle On, Pip-Boy downward dismissal Both, and Pip-Boy button graphics Off; legacy side switches still load. |
| Mobile > Dialogue | `touch-dialogue`, `dialogue-theme-v1`, `dialogue-touch-choice`, `dialogue-speaker-side`, `dialogue-device-side-v1`, `dialogue-background-tint-v1`, `dialogue-hints-v1`, `dialogue-choice-text-size`, `dialogue-subtitle-text-size`, `dialogue-background-opacity-v1`, `dialogue-frame-opacity-v1` | Touch dialogue off leaves its choices saved. Speaker side applies to Field Notes, Retro Console and Wrist Terminal; Device dialogue side places Wrist Terminal and Retro Console Right (default) or Left; backing, accent, tint and hints apply to Field Notes only. |
| Mobile > Lockpicking | `lockpick-touch`, `lockpick-touch-hints`, `lockpick-touch-size` | Choose the touch scheme, its hints (Off by default on Android) and control size. |
| Mobile > Interface size | `menu-scale`, `settings-menu-scale-v1`, `ui-scale`, `hud-scale-v2`, `hud-health-size-v1`, `hud-action-points-size-v1`, `hud-compass-size-v1`, `barter-menu-size-v1`, `repair-menu-size-v1`, `workbench-menu-size-v1` | HUD and menu sizes are independent. `settings-menu-scale-v1` (80–200%, default 100% = exact desktop) and `menu-scale` (50–200% on Android, default 100%, with main-menu viewport clamping so entries stay on screen); Barter/Repair/Workbench (50–200%) default to 150% on Android. |
| Mobile > Mobile performance | `mobile-performance`, `mobile-fps`, `mobile-resolution`, `mobile-thermal-auto`, `mobile-thermal-status` | Preset, frame rate and world resolution are lists. Thermal adjustment is off by default, affects the running session only, and keeps the selected values. |
| Controls > Controller | `controller-ads-v1`, `controller-dead-zone-v1`, `controller-toggle-aim-v1`, `controller-hold-sneak-v1` | These rows are shown for controller input. Main's Input and Aim Assist rows keep their original placement. |

Additionally, Android defaults the existing `Controls > Look` horizontal/mouse (`controls:sensitivity`) and vertical (`controls:vertical`) look sensitivities to `1.25` (1.25x); desktop defaults remain `1.0` (`0.0020` / `1.0`).

