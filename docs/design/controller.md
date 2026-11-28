# Controller support (design)

The owner asked on 2026-10-16 for a controller layer: "both keyboard and mouse and the ability
to use a controller depending on a setting under controls", with prompts that follow the
device ("Press E) to open could become Press A) if on an xbox controller") and the base
game's defaults. Facts about the original are in docs/spec-notes/controls.md §2.1. Code:
`engine/nvf-play/src/pad.rs` (the controller), `bindings.rs` (the bindings and
`ControlInput`), `pause.rs` (the Controls tab), `aim_assist.rs` (§5).

## 1. One setting picks the device

`Input Device` in `[Controls]` of `NVForever.ini`, `Keyboard and Mouse` (the default) or
`Controller`, and the first row of the settings page's Controls tab. Every control is
asked through `ControlInput`, whose presses answer from the keyboard and mouse or the
controller as the setting says; releases remain available from both devices [design,
#2200; the owner's "depending on a setting"]. Esc and
the console's key keep working with a controller chosen, so a player is never locked
out. Any connected controller counts (Bevy's gilrs backend, on by default); its arrival
and departure are logged (`controller: … connected`).

[design, #2200] **Automatic Input Switching**, immediately after Input Device on the
Controls tab, defaults to ON (`bindings::AUTOMATIC_INPUT_SWITCHING`). It is saved as
`Automatic Input Switching=1` in `[Controls]`; `0` keeps device selection manual. Old
INIs keep reading both existing Input Device names and gain the missing switch line.

[design, #2200] A fresh controller button press, either stick crossing `pad::STICK_DEAD`
(0.12), or either analog trigger crossing that dead zone selects Controller. A fresh key
press or mouse button click selects Keyboard and Mouse. Connection, mouse motion, wheel
and an already-held input do not select a device. When both devices become active in
the same frame, the current choice wins; while rebinding, the selected device stays put.
An automatic switch also requires the old device **idle at frame start, with no release
in that frame**: no physical keyboard key or mouse button held for Keyboard and Mouse;
no pad button held or stick/trigger past its dead zone for Controller. Returning an old
stick/trigger to neutral still waits out that frame. New-device activity while the old
device is busy is discarded, never queued: release/neutralize the old device, then make
a fresh press or dead-zone crossing on a later frame. Physical keyboard state excludes
controller-generated menu keys. `digital_busy` checks held buttons excluding fresh
presses and includes every release (even a press/release pair); analog history supplies
frame-start dead-zone occupancy across all connected pads.
Previous pad button/analog occupancy also waits out a pad's disconnect frame, so
removing its component cannot bypass the old device's readers.
`pad/device.rs::switch_on_use` defines this rule; selection calls `bindings::set_device`,
so the existing INI writer saves it, prompts follow, and the Controls tab shows the
chosen device's rows, including Stick Sensitivity (`settings_view::ControlRow::rows`).

[design, #2200] **Ignore the switching press edge only; never consume a release.**
`pad/device.rs::SwitchingInput` is the one central action filter. On the switching frame
it records the incoming device's fresh button/key presses and analog crossings. It
ignores those presses for actions (including raw text/repeat messages in that frame).
An already-held input is not added. Later holds/repeats work normally; every hardware
release remains available on either device, even a press/release pair within one frame.
No hardware snapshots are restored and no hardware state is reset by device switching.
`ActionButtons` and `ActionPads` expose filtered action views; `ControlInput` queries
releases from both devices. No switching-frame action is treated as harmless.

[design, #2200; `pad::InputSwitchPlugin`, `pipboy.rs::drive_pointer`] Physical keyboard
state is tracked independently of controller-generated menu keys, including focus loss
and test-aid presses/releases. Selection follows Bevy input and the hardware aids,
before fullscreen, UI focus and picking. Pip-Boy raw click production runs in
`PreUpdate` after selection and before `PickingSystems::ProcessInput`, consulting the
same filter for presses and passing releases. Synthetic menu keys are released when
deselected without releasing a physical key that shares their code. Derived stick
history follows physical state so the next frame's continued hold cannot manufacture a
fresh press; action readers filter only the current crossing.

[design, #2200] Each automatic change still logs
`controller: input device switched to Controller (button)` (or the selected device and
cause: `stick`, `trigger`, `key`, `mouse button`). Selection, INI saving, prompts and the
Controls rows follow the same setter as manual changes. Synthetic smoke coverage uses
`NVF_PLAY_TEST_KEYS=13:pad-A:1,16:enter:1` (`test_aid.rs::real_presses`), not the pause
menu's `pad:` shortcut, which already translates directly into a menu key.

[design, #2200; `pad/device.rs::automatic`, `bindings::set_device`] The **idle gate**
covers release-triggered actions as a class: an automatic switch cannot split an old
device's press/hold from its release, or commit/latch its tap, click or timer. Refused
switch attempts leave that device selected; its real release can finish its own action
normally, before a later fresh incoming press switches. The setter's existing
cancellations below remain backstops (including manual selection); they are not a
complete list of every pending release action. Assigning the same device preserves
holds. Audit:

| State (file references) | Across a device change |
|---|---|
| Pending melee Use hold (`combat::Combat::swings`, `combat::melee`) | Idle gate refuses while held and on release; the switch cannot manufacture the falling trigger that starts a tap swing. Attacks already underway continue. |
| World Map marker press (`pipboy/data.rs::DataState::press`, `take_input`, `travel_ask`) | Idle gate keeps the original device through the click release, so it cannot ask to travel across an automatic switch. |
| Dead Eye Block timer (`vats_mode::VatsMode::block_at`, `read_keys`) | Idle gate refuses even when the Block release and incoming pad press arrive together; unmark stays on the original device. |
| Activate hold on a holdable item (`grab::GrabState::tap`, `released_tap`; `activate::activate`) | Clear the pending tap; no take, steal or other activation. |
| Weapon-wheel hotkey hold, pending or open (`weapon_wheel::WeaponWheel`) | Clear the pending press and close without selecting/using its picked item. |
| Toggle POV / orbit held/moved (`control::PlayerControl`) | Cancel without a POV tap; preserve angles, detached state and chosen POV. |
| Ready Item timer (`control::PlayerControl`) | Cancel without reload or holster. |
| Aim/block and attack (`control::PlayerControl`) | Clear immediately, then read the selected device with the switching edge filtered. |
| UI press (`ui_pointer::PointerState::Pressed`) | Clear to None, including persistent quantity-menu meters; eventual physical mouse-up still arrives. |
| Sneak, Always Run, Auto Move (`control::read_controls`) | Intentional press toggles remain chosen. |
| Run modifier, movement (`player::read_input`) | Read from the selected device each frame, without a retained release action. |
| Physical keyboard, mouse and pad state (`pad/device.rs`) | Never masked by switching; all releases remain visible. |
| Synthetic menu keys (`pad::MenuKeys::held`) | Deselection releases synthetic keys and empties the list; physical overlap stays held. |
| Menu adjustment repeats (`pad::sideways`, `chargen`, `levelup`) | Only the switching frame is filtered; later repeat messages work. |
| Pointer/slider holds (`pause`, `tiles/input`, `ui_pointer`) | Filter switching clicks and clear UI press state; physical mouse-up ends the hold. |
| Fullscreen Alt bookkeeping (`full_screen_key`) | Every Alt release clears bookkeeping; switching presses use the central filter. |

[design / file refs] Scheduled regressions exercise the real melee, map input, VATS
Block, Activate, wheel, pointer, report-form, menu-key and PlayerControl readers,
including both earlier Enter overlap regressions, Alt release and manual orbit/Ready
Item cancellation backstops. Busy/refused switching is tested in both directions with
digital and analog input, release frames, continued incoming holds and later fresh
presses. Engine tests require execution by the main session; a writer's cargo check establishes compilation only.

[design / file refs] **Input-reader sweep (#2200, rounds 3-4).** Each declaration below
uses the central action filter; shared `ControlInput` covers its gameplay consumers
(including Activate, grab, wheel, control, movement and VATS). Line numbers refer to
this branch. Test fixtures use synthetic inputs, not game data.

| Reader declaration(s), file:line | Treatment |
|---|---|
| `engine/nvf-play/src/aim_assist.rs:355` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/bindings.rs:1111, 1112, 1115` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/blackjack.rs:485` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/caravan.rs:684, 744` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/chargen/vigor.rs:226, 227` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/chargen.rs:330` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/companion_wheel.rs:328, 329` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/console.rs:348, 978` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/crafting.rs:347` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/creator.rs:355, 356, 360` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/credits.rs:280, 281` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/dialogue.rs:733` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/free_cam.rs:203` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/intro.rs:476, 477` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/levelup.rs:228` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/lockpick/help.rs:107` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/lockpick.rs:526` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/main_menu.rs:412` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/menu.rs:237, 472, 565, 627` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/pad.rs:919, 1122, 1346` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/pause.rs:1983, 2450, 2629, 2647, 2654` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/pipboy/data.rs:725, 730, 734` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/pipboy/hotkey_panel.rs:121` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/pipboy/item_mod.rs:302` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/pipboy/items.rs:354, 359, 438` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/pipboy/repair.rs:254` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/pipboy/stats/mod.rs:386` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/pipboy.rs:838, 858` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/player.rs:735` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/poker.rs:456` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/radio.rs:631` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/repair.rs:359` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/roulette.rs:420` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/script_hud.rs:391` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/script_input.rs:255` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/script_quantity.rs:48` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/slots.rs:401` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/terminal.rs:746, 783` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/tiles/input.rs:195, 196, 197` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/tutorial_menu.rs:345, 350` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/ui_pointer.rs:153` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvf-play/src/wait.rs:582` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvforever/src/fly.rs:33, 34` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvforever/src/main.rs:2070` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvforever/src/pipboy_arm.rs:645` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvforever/src/reports/form.rs:481` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvforever/src/sky.rs:1766` | `ActionButtons` / `ActionPads` (filtered). |
| `engine/nvforever/src/terminal_view.rs:244` | `ActionButtons` / `ActionPads` (filtered). |

[design / file refs] Raw-message readers in `chargen.rs::input`, `levelup.rs::input`
and `console.rs::typing` also use `SwitchingInput::allows` (beside the button declarations
above); `pad::sideways`, `intro::Film::skipped_by`, `pause::Pressed`,
`pipboy/hotkey_panel::held_slot` and `pipboy/stats::keys` receive already-filtered views.
`vats_mode.rs` uses filtered `ControlInput::stick_buttons`; the tutorial uses
`ActionPads::filter_sticks`. PointerState consumers receive filtered click production
and the setter's press cancellation rather than reading hardware independently.

| Raw/special reader or producer, file:line | Filter or reason it needs no action filter |
|---|---|
| `engine/nvf-play/src/pad/device.rs:190` | Hardware capture; produces the central physical keyboard tracker, not actions. |
| `engine/nvf-play/src/pad/device.rs:202` | Hardware activity detection; owns/clears the frame-local filter, not actions. |
| `engine/nvf-play/src/full_screen_key.rs:83` | Mutable keyboard shortcut reader: uses `SwitchingInput::filter` and `allows`; releases pass the shared filter. |
| `engine/nvf-play/src/script_input.rs:71` | Mutable script-input bridge: filters held keyboard/mouse reads with `SwitchingInput::filter`; writes synthetic presses/releases. |
| `engine/nvf-play/src/pad.rs:244` | Raw pad identity tracking for labels; does not activate controls. |
| `engine/nvf-play/src/pad.rs:930` | Raw derived stick history; actions use `filter_sticks` / `ControlInput::pad_is`, avoiding delayed fresh edges. |
| `engine/nvf-play/src/pad.rs:1121` | Filtered pad reads; mutable keyboard parameter only produces translated presses/releases, preserving physical overlap. |
| `engine/nvf-play/src/pad.rs:1351` | Connection/removal logging only; no button actions. |
| `engine/nvf-play/src/loading.rs:252` | Existing independent loading gate writes keyboard/mouse state; does not act on input or implement switching consumption. |
| `engine/nvf-play/src/test_aid.rs:345` | Hardware test-aid producer; mirrors keyboard into detection and precedes automatic selection. |
| `engine/nvf-play/src/pause.rs:2494` | Keyboard event producer for tests; precedes physical capture. |
| `engine/nvf-play/src/pause.rs:2550` | Mouse state producer for UI tests, not an action reader. |
| `engine/nvf-play/src/wait.rs:458` | Mouse state producer for UI tests, not an action reader. |
| `engine/nvf-play/src/pause.rs:2448` | Raw keyboard messages use `SwitchingInput::allows`; held modifiers use `ActionButtons`. |
| `engine/nvf-play/src/pipboy.rs:1009` | Raw mouse/window events use `SwitchingInput::allows_mouse`; releases pass. |
| `engine/nvforever/src/reports/form.rs:481` | Raw keyboard messages use public `SwitchingInput::allows`; held modifiers use `ActionButtons`. |
| `engine/nvf-play/src/rumble.rs:120` | Connected-entity query only sends rumble output; no input action. |
| `engine/nvf-play/src/pause.rs:2628` | Raw stick history is used only while rebinding (`Listening`), when automatic switching is disabled; other buttons/pads use filtered parameters. |
| `engine/nvf-play/src/pad.rs:1165` | Pure helper receives a filtered keyboard view and filtered raw messages from its action callers. |
| `engine/nvf-play/src/intro.rs:135` | Pure helper receives filtered button snapshots. |
| `engine/nvf-play/src/pipboy/hotkey_panel.rs:112` | Pure helper receives a filtered keyboard snapshot. |
| `engine/nvf-play/src/pipboy/stats/mod.rs:663` | Pure helper receives a filtered keyboard snapshot. |
| `engine/nvf-play/src/pause.rs:2054` | `Pressed` helper receives the filtered keyboard snapshot from `choose`. |

## 2. Defaults are the game's

Each control's controller input comes from the game's own `[Controls]` byte `JJ`
(spec-notes/controls.md §2.1, read as hypothesis until checked in the original):

| Control | Button | | Control | Button |
|---|---|---|---|---|
| Forward / Back / Slide Left / Right | left stick | | Vats | RB |
| Activate | A | | Toggle POV | LB |
| Menu Mode (Pip-Boy) | B | | Rest (Wait) | Back |
| Ready Item | X | | Crouch/Sneak | LS (press) |
| Jump | Y | | Grab | RS (press) |
| Block (aim) | LT | | Hotkey1, Ammo Swap, Hotkey3, Hotkey4 | D-pad up, right, down, left [design] |
| Use (attack) | RT | | Run, Always Run, Auto Move, Hotkey5–8, saves | none |

The right stick looks at `fXenonHorizLookSpeed` 1500 and `fXenonVertLookSpeed` 1200,
read as milliradians a second at full push [hypothesis], pushed a little turning slowly
(the push squared) [design]. The left stick moves the way it is pushed; under 0.6 of a
full push the player walks while Always Run is on [design]. A stick pushed past half way
counts as its direction's button (for Auto Move's stop and getting up from a seat).
`Invert Y` serves both mouse and stick.

The buttons are kept in `[Controller]` by the controls' INI names (`Activate=A`,
`Forward=Left Stick Up`, `Hotkey5=None`) with `Look Speed Horizontal` and `Look Speed
Vertical`; the Controls tab rebinds them (the next controller button pressed, a swap with
the control that had it, Start cancels, Delete clears, Defaults puts back the game's).
Start cannot be bound.

## 3. Menus and prompts

- Start is Esc (the pause menu) everywhere. In a menu the D-pad is the arrow keys, A
  Enter, B Esc, LB and RB, LT and RT Page Up and Page Down: the menus read keys, and the
  controller stands in for them (`pad::menu_keys`), pressing a key only as its button
  goes down. On the pause menu LB and RB go from heading to heading of the settings
  page, LT and RT are F1 and Tab (the tab before and after), Y is F6 (the keys onto the
  rail and back; there the D-pad passes over Search, which is typed and says "Keyboard
  search", and the controller works the results a keyboard found) and X Delete [design:
  #1953; settings-page.md §4, §6].
- Each menu's own buttons follow the original's menu files (`_xbox_button` of the button
  tiles [documented: the vanilla menu files]) and press the key NVForever's menu gives
  that control (`pad::menu_buttons`, keyed by the open menu's MenuMode code); a menu's
  own button wins over the shared ones above (#1393):

  | Menu | Buttons |
  |------|---------|
  | Container | X Take All; LT / RT pick the player's list / the container's, "LT)" and "RT)" drawn under them; D-pad left / right the picked list's category [observed: the original, #1827; documented: `CM_Items_TitleButton`, `CM_Container_TitleButton`]; while "How many?" is up its own buttons rest: the D-pad steps the amount, LT / RT by ten [design, #1827] |
  | Barter | X Accept; LT / RT and the D-pad as the container menu's [documented: `BM_Items_TitleButton`, `BM_Container_TitleButton`; the D-pad: hypothesis, #1827] |
  | Pip-Boy (every page) | LT / RT the previous / next page (STATS, ITEMS, DATA, round again); D-pad left / right the tabs [design: owner 2026-11-02] |
  | Pip-Boy ITEMS | X Drop (`pipboy::items`, not while "How many?" is up), Y Repair, LB Mod; RB Hotkey [design, #2312] |
  | Pip-Boy STATS | status tab: A Stimpak, Y Doctor's Bag, X Rad-X; General tab: X switch lists |
  | Pip-Boy DATA | Y Challenges (Notes tab); A Fast Travel on the World Map's marker, Set Active on Quests, Tune on Radio; X Place Marker (the custom marker's question, #1954), Show Location, the notes' filter (`pipboy::data`, #1897) |
  | Crafting | X Accept |
  | Repair services | X Repair All |
  | Level up, traits | Y Reset, X Continue, B Back |
  | Tag skills | Y Reset, X Done |
  | Vigor Tester, SPECIAL page (1048), traits | the left stick is the D-pad: left / right lower / raise the attribute (the Vigor Tester's review row, the SPECIAL page's row), up / down the rows; a step a push past half way (`pad::STICK_PRESS`), no repeat while held (`pad::menu_stick`) [documented: `love_tester_menu.xml`, `specialbookmenu.xml` give `increase_value` / `decrease_value` the left stick's picture, `trait_select_menu.xml` its Navigate hint; hypothesis: the stick works their `xleft` / `xright`; design: the trait menu, whose file names no stick, #2313] |
  | Lockpicking | X Force Lock |
  | Blackjack | A Hit, Y Double/Deal, RT Split/Raise, LT Switch/Lower, X Surrender/Max Bet, B Stay/Exit |
  | Roulette | A Place, Y Remove, X Finish, LT Lower, RT Raise, B Exit |
  | Slot machine | A Spin, RT Raise, LT Lower, Y Payouts, X Max Bet, B Exit |
  | Tutorial | RT Next, LT Previous; the D-pad or the left stick up / down scrolls the text, a step a push [observed, #1905: the left stick in the original] |
  | Caravan bet [design, #1662] | A Accept, Y Auto Match, RT Raise, B Exit |
  | Caravan deck [design, #1662] | A Add, X Remove, Y Randomize, RT Play Caravan (B, the shared Esc, leaves) |
  | Caravan game [design, #1662] | A Select Card, X Discard Card, Y Discard Track, B Cancel; D-pad left / right the hand |
  | Poker [design, #1662] | A Check/Call (between hands Next Hand), RT Raise, Y All In, X Fold, B Leave; D-pad up / down the raise |
  | Pause menu [design, #1662] | X Delete (a save on the Load page, a binding on the Controls tab) |
  | Pip-Boy STATS, RAD view [design, #1662] | A RadAway (not used from the page, as its key), X Rad-X |

  Caravan has no menu file read yet and poker and the pause menu are NVForever's own,
  so those buttons are NVForever's, following the other card games (A the main action,
  Y and X the next, B leave). A minigame's help box rests the minigame's own buttons, so
  A (Enter) and B (Esc) close it as its keys do (blackjack's A is otherwise its F)
  (`pad::MenuPages`, `pad::HELP_PAGE`). Poker and the pause menu have no MenuMode code
  and stand in `pad::MenuAt` as codes of their own (`POKER_MENU`, `PAUSE_MENU`); while
  the pause menu is up its buttons are the menu's own, not those of a menu under it.

  [design, #2312; `pad::menu_buttons`, `pipboy::hotkey_panel`] On ITEMS the triggers
  change pages, LB is Mod and RB opens the hotkey panel for the selected item. The
  D-pad normally steps rows/tabs; while the panel is open it owns all pad menu input.
  On the STATS status tab X is Rad-X, which only acts on the RAD view; on CND and EFF it
  does nothing.

  [documented: #2312's supplied `menus\main\inventory_menu.xml` facts] The Hotkey
  button is `IM_HotkeyButton`, RB, labelled by `sInventoryHotKey`. [design, #2312]
  Both the hand-built and tiled ITEMS pages open the existing panel at slot 1. D-pad
  right/down advances one slot and left/up goes back, wrapping across all eight
  (`hotkey_panel::PAD_SLOT_COUNT`); Ammo Swap remains reserved and refuses assignment.
  A assigns the item and closes on success; a refusal stays visible. B cancels without
  changing slots. Closing/changing the page, changing input device or opening another
  modal cancels. The stepping order is provisional [hypothesis: original selection
  method unknown; observer check 211]. The hand-built hint uses the load order's
  `sInventoryHotKey` word (our built-in fallback otherwise), followed by RB/R1 and the
  existing glyph renderer. The tile page shows its optional `IM_HotkeyButton` with the
  same pad hint and keeps the file's label. Held-digit/click/Enter assignment with the
  keyboard is unchanged. Smokes: `pipboy-hotkey-assign-pad`, `tiles-items-hotkey-pad`.
- On the Pip-Boy DATA page's maps the right stick moves the cursor (the highlight box),
  pushed past an edge it pans the map, and the left stick's up and down zooms in and out
  smoothly while held [observed: the original; the speeds are design], built in
  `pipboy/data.rs` (`stick_cursor`, `scroll_map`, `stick_zoom`). On the World Map, A (the
  confirm button) on a highlighted marker asks to fast travel there, as a click does
  [observed: the original], also in `pipboy/data.rs`.
- On the DATA page's Quests and Misc tabs the right stick's up and down scrolls the right
  pane (the quest's objectives, the note's text) smoothly while held [observed: the
  original, issue #1667], a note window (385) a second at a full push [design: as far a
  second as one LB/RB page]; LB/RB (Page Up/Down) page it (`pipboy/data.rs`
  `scroll_pane`, docs/spec-notes/pipboy.md §5.3).
- The prompt under the crosshair names the Activate control's own key ("[E]") or, with a
  controller, its button ("[A]") (`Bindings::prompt`). The original shows the button's
  picture there (`&xbuttona;`, `general_button_a.dds`): NVForever draws its own for the
  four face buttons under the PlayStation names (§3a), and letters stand in for the rest.
- **Hint shape** (#4495): every key hint is the key's name in square brackets, "[E]",
  "Take All [R]", "Close [B]", the shape NVForever's own labels for `sPCMenuHint*` use
  [design: the original's "E)" shape was read from its screen, and strings seen only on the
  screen are not transcribed (LEGAL.md)]. The "X)" examples below and in the spec notes
  describe the original.
- **Menu hints follow the device** (#1662). Every hint names the key its menu answers
  ("Take All R)", "Close [E]") or, with a controller, the button that presses that key
  there (`pad::hint`, looked up in the tables above through `pad::button_for`, the
  menu's own buttons first, then the shared ones, B before Start for Esc): "Take All X)",
  "Close [B]". A hint whose keys no button presses keeps the key's name (a unit test
  lists every hint and fails on such a gap). Which names are in use is one setting for
  every menu (`pad::labels`, kept in step with Input Device each frame) [design, #1662:
  as a language is; the drawing code reads it without each menu taking the bindings].
  Built: the container, barter and "How many?" buttons, the tiled container menu's Take
  All, the Pip-Boy's Repair/Maintain, Mod, mod menu, repair list, STATS hints and
  General/Reputation switch, DATA's Challenges/Notes, crafting, repair services,
  level-up and the tag-skill/trait lists, lockpicking and every minigame's help box
  Close, blackjack, roulette, slots, caravan, poker, the Wait/Sleep menu, the dialogue's
  Goodbye hint, the pause menu's Delete and Yes/No and its pages' Back ("Back  B)" [observed, #1893]; Defaults has none, its Y in the original being the rail key here, #1953), the tutorial menu's Next, Previous
  and Close ("Close E)", with a controller "Close B)" [observed, #1904]). The help texts' control strings
  (`&sUActnCrouch;` …, `lockpick::help::expand_keys`) name the control's button. Menus
  drawn with a view kept for change detection redraw when the device changes; the others
  (the container, barter and Pip-Boy pages) take the new names at their next redraw.
  Not followed: the hint words the tiled Wait/Sleep, tutorial and message menus take
  from their files' `_PCButtonText` (opt-in `[UI] TileMenus`), and the original's own
  button pictures (NVForever draws its own glyphs in their place, §3a).
- **The mouse pointer sleeps with the controller** [design, #2310; whether the original
  hides its pointer with a controller is a hypothesis not checked]. With Input Device =
  Controller the window's pointer is hidden in every menu, box and conversation
  (`player::grab_cursor`) and points at nothing: no button, row or tab is hovered under
  where the mouse was left (`ui_pointer`, the menus drawn from their files in
  `tiles::input`, the Pip-Boy's flat image `pipboy::flat_pointer` and its screen on the
  arm, nvforever `pipboy_arm`). It wakes when the mouse is used (8 pixels of motion in
  all, a button or the wheel; `pad::pointer::WAKE_TRAVEL`) and then shows and hovers as
  with the keyboard and mouse; it sleeps again on the next controller button or stick
  push, and whenever the device turns to Controller. Mouse motion still selects no device
  (§1): it only wakes the pointer. `pad/pointer.rs` (`PadPointer`, `step`) holds the
  rule and logs `pointer: asleep …` / `pointer: awake …`. The test aids' own pointers
  (`NVF_PLAY_TEST_PIPBOY`'s `point=`, `NVF_PLAY_TEST_POINTER`) stand in for the mouse and
  do not sleep.
- **Only a pointer that moves takes a list's selection** (#2310): a row redrawn or
  scrolled under a resting pointer as the D-pad or the keys step the list leaves their
  choice alone. The Pip-Boy's hand-built lists (ITEMS, the repair list, the mod menu,
  DATA's rows) read `pipboy::PointerMoved` (the screen's pointer moved this frame,
  `pipboy::drive_pointer`); the window menus (`menu::menu_mouse`, `repair`, `crafting`,
  `pause`) and the pages drawn from their files (`tiles::container_menu::hover_selects`,
  `tiles::item_menu::hover_selects`, the STATS page's `drawn_at`) already had the rule.

## 3a. PlayStation names (#1452)

The original shows only Xbox button pictures; a PlayStation controller's names are
NVForever's own [design, #1452]:

- **Which names.** The settings page's Button style (registered in the Controller group,
  shown on the Controls tab with a controller as the input device; `button-style` in
  `[Settings]` of NVForever.ini): Auto (the default), Xbox or PlayStation (`pad::ButtonStyle`).
  Auto takes the PlayStation names while the controller in use reports a PlayStation
  name (`pad::playstation_pad`: "DualSense", "DualShock", "PlayStation", "Sony", "PS3",
  "PS4", "PS5", or the bare "Wireless Controller" of a DualShock 4 or DualSense; a name
  with "Xbox" or "XInput" never), else the Xbox ones. The controller in use is the last
  connected or the last whose button went down, so the names follow a pad picked up
  mid-game (`pad::PadInUse`; the log: `controller: <name> in use (PlayStation names)`).
- **The names**: A Cross, B Circle, X Square, Y Triangle, LB L1, RB R1, LT L2, RT R2,
  LS L3, RS R3, Back Select, Start Start (the PS3-era pair, the game's era, not Share/
  Create and Options); the D-pad and the stick keep their names (`pad::PLAYSTATION_NAMES`).
  They are used everywhere a button is named on screen: the crosshair prompt, the menu
  hints (§3), the Controls tab's bindings and the help texts' control strings.
- **The INI keeps the Xbox names** (`[Controller] Jump=Y`) whatever the style, so every
  existing NVForever.ini keeps loading and an INI does not change when the pad does;
  the rebinding log names them so too.
- **Glyphs**: NVForever draws its own Cross, Circle, Square and Triangle — a solid cross
  and three ring outlines — computed at start into small pictures (`pad::Glyph`, nothing
  from the game's or Sony's art). The eight labelled buttons are drawn as an amber capsule
  ring (L1/R1/L2/R2/Select/Start) or a circle knob (L3/R3) with the label written inside in
  the game's font, rasterized from the font's bitmap texture (`pad::Glyph::labeled_image`).
  Every menu hint draws them in place of the names — "Cross)" becomes a cross picture and
  ")", "Take All Square)" its square (`pad::set_hint`; the crosshair prompt draws the four
  face buttons the same way) — with a gap after the word and tucked against the following
  ")" or "]" (#1829). The hints stay text with Xbox names or keys, and the stick and D-pad
  directions and "LB"/"RB" have no glyph.

## 4. Rumble

The original rumbles the pad by its `bGamePadRumble` switch (`1` in the owner's
`FalloutPrefs.ini` [observed 2026-10-16]); how strongly and on what is not captured, so
everything below is [design] (combat is not a parity target). Built in
`engine/nvf-play/src/rumble.rs`.

- **When:** only while Controller works the controls and `Rumble=1` in `[Controller]` of
  NVForever.ini (default 1, the game's switch). With Controller chosen the settings page's
  Controls tab has a last row **Rumble** (ON/OFF). Each rumble is a Bevy
  `GamepadRumbleRequest` sent to every connected controller (gilrs plays it); pulses of one
  frame are joined (the strongest of each motor, the longest). The first of each kind is
  logged: `rumble: first shot (strong …, weak …, … s) on N controller(s)`.
- **The player fires** (each `ShotFired` of the player; melee swings fire no projectile
  and do not rumble): by the weapon's WEAP weight `w` (pounds), `h = min(w / 20, 1)`:
  strong `0.7 h`, weak `0.3 + 0.4 h`, `0.07 + 0.13 h` s. A 9mm pistol (2) is a short buzz,
  the minigun (25) and Fat Man (30) the full thump.
- **The player takes damage** (each `ActorHit` on the player that took health): by the
  share `s` of the maximum health it took, `t = min(4 s, 1)`: strong `0.35 + 0.65 t`, weak
  `0.5`, `0.15 + 0.30 t` s.
- **A hard landing** (the physics step's touchdown speed; fall damage is physics.md §6.12):
  from 600 units/s (a drop of about 260 units under gravity 687 units/s²; a jump lands at
  about 290), `t` up to 1 at 1200 (a drop of about 1050): strong `0.45 + 0.55 t`, weak
  `0.3`, `0.15 + 0.25 t` s.
- **An explosion near the player** (each `ExplosionFx`): full inside the EXPL's radius,
  fading to none 1400 units (about 20 m) beyond it, `t`: strong `t`, weak `0.6 t`,
  `0.25 + 0.35 t` s; none under `t = 0.05`.

## 5. Aim assist

Built in `engine/nvf-play/src/aim_assist.rs` (#1196, #1197). Controller only: with
Keyboard and Mouse chosen nothing changes, and the mouse never gets either part. All of it
is [design] (combat is not a parity target): the original's auto aim (`fAutoAim*`) bends
shots toward a target near the crosshair and leaves the view alone [documented: GECK wiki
"Auto Aim Settings", owner's export]; NVForever takes its range and on-target area from
those settings so a mod that changes them changes this too.

- **The target**, shared by both parts: a living actor drawn now, hostile to the player
  (fighting it, or would attack it: the rule that turns the crosshair red), within
  `fAutoAimMaxDistance` (1800) of the crosshair, inside a cone around the crosshair whose
  radius is `fAutoAimScreenPercentage` (10) % of the screen's width at a 75° field of view
  (about 8.7°) [hypothesis: how the original measures the percentage], and seen: a ray from
  the crosshair to its aim point meets it first, passing what shots pass (water, triggers,
  NPCs' walking capsules: a capsule is not the body), so no lock through walls. The nearest to the
  crosshair by angle wins; the aim point is the middle of its posed torso part, else 80
  units over its feet.
- **Slowdown** (#1196): while the crosshair is within 40 units of a target's aim point (as
  seen from the crosshair), the right stick's turn is scaled by `1 − 0.75 × strength`
  (45% slower at the default 0.6), eased in and out over 0.1 s. Friction only, no pull.
- **Lock-on** (#1197): when aiming down the sights begins (LT with the weapon out) and a
  target is picked, the view turns toward its aim point for 0.3 s, easing in with a time
  constant of 0.08 s / strength and at most 4 × strength radians a second, following the
  target as it moves. It lets go at the end, when the target dies or leaves sight, range or
  1.5 times the cone, when aiming ends, or when the right stick is pushed at least 0.7 of
  the way against the pull. A new lock needs 0.6 s since the last began.
- **Not while** the free camera, the weapon wheel, a kill camera, VATS mode, an animation
  driving the view, furniture holding the player, a script's look lock or a circling or
  detached camera has the look, without the cursor grabbed, or outside play.
- **Settings**: the settings page's Controller group (shown on the Controls tab after the
  controls, with a controller as the input device: settings-page.md), kept in `[Settings]` of
  NVForever.ini: `aim-assist` (on/off, on), `aim-slowdown` and `aim-lock` (strengths 0 to
  1, 0.6 each; 0 turns that part off).
- The log says when a lock starts and how it ends (`aim assist: lock-on to 0x…`).

## 6. Not built yet

- The original's own button pictures from `Interface\Shared\Button\` in place of letters
  (NVForever draws its own glyphs instead, §3a).
- [hypothesis, #2312] The original's controller slot-selection method remains unknown
  (observer check 211); the provisional D-pad stepping rule is built (§3).
- The tiled menus' remaining file hints following the device (§3); ITEMS Hotkey's
  optional RB button is built [design, #2312; `tiles/inventory_menu.rs`].
- A pointer moved by a stick for the mouse-only parts of menus still left: the dialogue
  list (the Pip-Boy map's stick cursor and zoom are built, §3).
- Several controllers told apart for the controls (any pad's buttons work them); only the
  button names follow the pad in use (§3a).
