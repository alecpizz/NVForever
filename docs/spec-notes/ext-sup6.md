# SUP NVSE batch 6: the mouse, the cursor, debug texts, the screen, the lockpick menu (2026-11-08, #1008)

22 SUP functions: the mouse (4), the cursor's look (3), the debug texts (6), the HUD's
visibility flags (2), the screen, fonts, plugin headers and the machine (4), the lockpick
menu (2) and the throw held (1). Code: `crates/fnv-game/src/nvse_sup6.rs` (the
functions and `Game::sup`, what the engine fills and takes), the engine's side in
`engine/nvf-play/src/sup_screen.rs` (the mouse, the window, the fonts, the lockpick
menu, the throw; the fake moves, the pin's health, the debug texts) and
`engine/nvf-play/src/cursor.rs` (the cursor's traits). Tests:
`crates/fnv-game/tests/sup6.rs` (the engine's part played by the test) and the unit tests
of `nvse_sup6.rs`.

Source: the GECK wiki export [documented: the page named]. Probed in the original on
2026-11-08: set `sup6` of `tools/capture/jip_probes.py` (lab `L5.ext.sup6`), the answers
in §7 and as [observed] in the tables; a value a page leaves open and the run did not
answer is still a [hypothesis].

## 1. The mouse

| Function | NVForever |
|---|---|
| GetMousePosition `xOut yOut` | the system's pointer [documented: "system mouse position"]: the window's pixels from its top left, the last place while the pointer is outside the window [hypothesis: the engine knows only its window] |
| GetMouseMovement `xOut yOut request` | this frame's movement (0) or the last frame's that moved (1), in pixels, only in a menu [documented]; 0 0 out of menus [hypothesis] |
| GetMouseWheelMovement `request` | the wheel's notches the same way [documented]; a wheel that gives pixels counts 40 a notch [hypothesis] |
| FakeMouseMovement `x y` | the engine adds finite moves to the next frame's `MouseMotion` (turning the camera and feeding `GetMouseMovement` / lockpicking) and, while in a menu or HUD-cursor mode with no explicit `SetCursorPos` pending, also queues a relative pixel cursor warp through `script_switches::sync` / `script_cursor` [documented: "Will move cursor\camera by 1 pixel"; design: non-finite deltas ignored; routed through the cursor warp pipeline] |

The `…Out` arguments are written by the VM (`fnv_script::vm` `out_params`), as
GetTFCPosEx. The original's runtime compiler refused GetMousePosition and
GetMouseMovement with their variables [observed: sup6 mouse.pos, mouse.moves, "script did
not compile"], as it refused SUP's other getters into variables (ext-sup3.md, the TFC
`…Ex` getters); scripts compiled ahead keep the pages' form, so NVForever compiles them.

## 2. The cursor's look

| Function | NVForever |
|---|---|
| GetCursorTrait, SetCursorTrait `trait [value]` | 0 alpha, 1 width, 2 height, 3-5 red, green, blue [documented]; at rest 255, 32 × 32, 255 255 255 [observed: sup6 cursor.rest]; set and read back at once [observed: cursor.set] |
| SetCursorTraitGradual `trait [start [end [duration [mode]]]]` | the trait set to start, then to end in the duration, modes 0-3 as the page's table [documented]; the trait alone stops it where it is [documented]; a start alone sets it [hypothesis: as SetUIFloatGradual]; another mode reads 0. Out of menus the value read does not move: 255 at the call and a frame on, the same after the stop [observed: cursor.gradual, cursor.gradual.next with 200 to 50 over 2 s]; NVForever reads the value from before the change out of menus and the changing value in a menu [hypothesis: the change runs only while the cursor is shown] |

The values are worked out from the game time when read (no per-frame step). The engine
(`cursor.rs`) redraws the arrow when they change: scaled to width × height, its colours
times red/green/blue over 255, its alpha times alpha over 255 [hypothesis: the page
gives no scale].

## 3. The debug texts

| Function | NVForever |
|---|---|
| DebugTextCreate `name x y text` | creates a debug text at `x, y` in the window's pixels [hypothesis: the page's 100 100 and 600 700] and returns `1`; if `name` already exists, moves and changes it in place (preserving zoom, not doubled) [documented: DebugTextCreate; hypothesis: `w4c_b4_03` does not read back x, y, or text] while returning `-1` [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `w4c_b4_03_sup6_bobbypin_debugtext` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `mw(0,0)|bp(init=-1.00,set=-1.00,rd=-1.00)|dt(1,-1)`; no DLL or source consulted] |
| DebugTextDestroy, DebugTextExists | gone; 0/1 [documented] |
| DebugTextSetPos, DebugTextSetString, DebugTextSetZoom | its place, its text, its size [documented]; the zoom is the letters' height in pixels, 16 until set [hypothesis: the page's example sets 20] |

The engine draws them as plain white text over everything [hypothesis: the page shows
no picture].

## 4. The HUD's visibility flags

GetHUDVisibilityFlags and SetHUDVisibilityFlags are "replaced by an equivalent function
in JohnnyGuitar NVSE" [documented]: they read and write the same flags as
Get/SetHUDVisibilityOverride (`crate::nvse_hud`, `Game::hud.visibility`).

## 5. The screen, fonts, plugins and the machine

| Function | NVForever |
|---|---|
| GetScreenTrait `trait` | 0, 1 the UI's width and height, 2, 3 the window's pixels, 4 the resolution converter [documented: the page's table]: in interface units with no menu open too, 960 high and 960 × width/height wide, the converter 960/height [observed: sup6 screen, `1706.67 960 1280 720 1.33333` in a 1280 × 720 window; as the hud_menus capture of 2026-10-31, menu-xml.md]. Worked out from the window, not from `screen()` (which keeps pixels with no menu open). With no window, a headless run, 0 for all five [decision: these are the window's values; a headless run does not read the player's display settings, so the lab's run does not depend on the machine] |
| GetFontTrait `font trait` | 0 the base height, 1 the line height [documented]; both are the font file's line height, its first number, in fonts 1-8, and font 9 reads 0 0 [observed: sup6 fonts, `31/31 31/31 26/26 36/36 25/25 34/34 36/36 52/52 0/0`; fonts.md, monofonto_large 31 and glow_monofonto_vl_dialogs 34]. The engine gives its fonts (the install's own files, `nvf_play::interface::FONT_FILES`); a headless run reads the default files (`nvse_sup6::FONT_FILES`) |
| GetModTraitSTR `index trait` | the plugin's author (0) and description (1), TES4 CNAM and SNAM [documented]; "(null)" for a field the header lacks [observed: sup6 modtraits, `[ipely] [(null)] []` for FalloutNV.esm, which has no SNAM]; "" for another index or trait [observed: the third] (`GameData::plugin_texts`) |
| GetPCTrait | 0 [decision: no channel to the machine's Windows version, memory and time since it started (the 2026-11-08 brief); follow-up]. The original gave values above 0 for 0, 2 and 6 [observed: sup6 pctraits]: the machine's, not compared |

## 6. The lockpick menu and the throw

| Function | NVForever |
|---|---|
| GetLockPickMenuValue `request` | 0 the pin's angle (0 left, 90 up, 180 right, as NVForever's menu, `fnv_game::lockpick::Picking::pin` [design: original SUP NVSE 855 returned `0.89` in `w4h_11_sup_lockpick_menu_tiers` and `952.47` in `w4e_12_sup_lockpick_menu_value_tiers`, not a `0..180` angle centered at `90`]), 1 the lock's difficulty tier enum (`0` Very Easy/`0`, `1` Easy/`1..=25`, `2` Average/`26..=50`, `3` Hard/`51..=75`, `4` Very Hard/`76..=100`, via `lockpick::Level::of(level).index()`), 2 the Lockpick skill, other requests `0.0` while open; -1 with the menu shut [documented; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes to game install or Saves), `sup_20_lockpick_and_bobbypin_closed`, `w4e_12_sup_lockpick_menu_value_tiers` (`tools/capture/nvse/catalogs/wave4e_mods.json`, #3802 / PR #3803), and `w4h_11_sup_lockpick_menu_tiers` (`tools/capture/nvse/catalogs/wave4h_reconcile.json`, #3802 / PR #3803): `PROBE\|w4h_11_sup_lockpick_menu_tiers\|shut(-1.0,-1.0,-1.0,-1.0,-1.0)\|open50(mm=1014,0.89,2.0,100.0,0.0,0.0)\|tiers(1=1.0,25=1.0,50=2.0,75=3.0,100=4.0)`; zero DLL, PDB, or third-party source consulted] |
| SetBobbyPinHealth `[health]` | sets the pin's health, above 1 too, and returns it; without a value returns it; a new pin starts at 1 [documented; `fnv_game::lockpick::PIN_HEALTH` is 1]; -1 with the menu shut [observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), `sup_20_lockpick_and_bobbypin_closed`] |
| GetGrenadeTimeHeld | the seconds the attack was held with a throwable weapon in hand (Grenade Throw, Thrown, Land Mine, Mine Drop), kept after the throw and reset at the next press [documented: the page's note] |

The engine writes the menu's state into `Game::sup.lockpick` before the scripts and takes
the new health after them (`sup_screen.rs`); the throw from `PlayerControl::trigger` and
the weapon's animation type (`Combat::player_weapon_is_throwable`).

## 7. Probe set `sup6`

Read-only rows and writes restored at the end (the cursor's traits, a debug text made
and destroyed, the HUD flags put back). The questions: the cursor traits at rest (alpha,
size, colours); SetCursorTraitGradual's value halfway; a debug text's default zoom; the
mouse functions out of menus; GetScreenTrait's five values; GetFontTrait of fonts 1-8;
GetModTraitSTR of FalloutNV.esm; GetPCTrait (for the record only: NVForever gives 0);
GetLockPickMenuValue and SetBobbyPinHealth with the menu shut; GetGrenadeTimeHeld at
rest. The lockpick menu's values while it is open and the throw held need a person at
the controls: not in the set.

**Run of 2026-11-08** (the original, save nvfairport, a 1280 × 720 window; golden
private). Equal to NVForever before this note: cursor.rest, cursor.set, debugtext,
hudflags, lockpick.shut, grenade. Followed since: cursor.gradual and cursor.gradual.next
(§2), fonts (§5, headless too), modtraits (§5). Left: screen (the window's: 0 in a
headless run by decision, the rule followed in the engine), pctraits (the machine's, by
decision 0). Did not compile in the original: mouse.moves, mouse.pos (§1).
