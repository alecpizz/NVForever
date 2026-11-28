# MCM: the mod author's contract, checked against NVForever (issue #2298)

The Mod Configuration Menu (MCM) is a mod: its own menu files (`menus\prefabs\MCM\*.xml`,
included into `StartMenu`) and its own scripts (`The Mod Configuration Menu.esp`) draw
the window and run it. A mod that wants a page talks to MCM only through traits of the
`StartMenu/MCM` tile tree, with the extender's UI functions, as MCM's author describes in
his guide for mod authors. NVForever does not reimplement MCM: it runs MCM's files and
scripts (menu-xml.md §10) and has to give them, and the mods' scripts, what the original
engine and extender give. This note lists what the guide asks of a menu, item by item, and
says where NVForever stands on each.

Sources: [documented: MCM Guide v6 (Pelinor, 2012-05-10), the owner's copy, kept
private, cited by section]; MCM 1.5's own files and scripts as installed in the owner's
Blue Moon list [observed: the files; the scripts' source text in the plugin]; runs of
NVForever on that list (§5). No extender or MCM DLL code was read. Nothing of the guide
is copied here; it is described.

## 1. How a mod's page works [documented: the guide, "The Basics"]

- The mod makes a quest that starts with the game, with a script whose `MenuMode 1013`
  block (the start menu, which the pause menu is) does the work, and a misc item whose
  name is the mod's line in MCM's list. At `GetGameRestarted` it adds that item to MCM's
  form list, form `0xAE6` of MCM's plugin (by `BuildRef` of MCM's mod index and 2790),
  when `IsModLoaded "The Mod Configuration Menu.esp"`.
- Every frame of that block, it returns unless MCM is loaded, and acts only while
  `StartMenu/MCM/_ActiveMod` equals `GetModIndex` of its own plugin. It reads
  `_ActiveOption` (the option the player works) and `_Value` into locals, then answers at
  most one of MCM's requests, each a trait of `StartMenu/MCM` that MCM sets and the mod
  clears: `_Reset` (fill the page in again: clear it, set `_columns` and every option's
  traits), `_Default` (the Defaults button: set the mod's defaults, clear it, set `_Reset`),
  `_NewValue` (the player chose a value: take `_Value`, or `_Value1`-`_Value3` for types 8
  and 9, clear it, set `_Reset` so the page shows it), `_ShowList == 1` (fill the list
  window, set it to 2), `_ShowScale == 1` (fill the slider window, set it to 2) and
  `_DefaultScale` (the slider window's Default: put the default in `_Value`, clear it, set
  `_ShowScale` 2). The mouse-over text is a separate test on `MCM_Options`' `_optionID`.
- Paths are written with the extender's `GetUIFloat`, `SetUIFloat`, `SetUIString` and
  `SetUIStringEx` (formatted as PrintToConsole), either by name
  (`StartMenu/MCM/MCM_Options/Option2/_value`) or by place: `*:N` is the N-th child,
  counted from 1 (`StartMenu/MCM/*:4` is MCM's fourth child; `*:1/*:2` Option2 of the
  options panel; `*:2` the slider window, `*:3` the list window, `*:5` the mod list,
  `*:8` the title, `*:9` the info line). Names are exact but not case-sensitive.

## 2. Coverage

The Mods page's physical key-event producer is tracked in
[mcm-page-key-events.md](mcm-page-key-events.md). Its authored regressions do not
qualify the installed MCM Extender wheel/search profile until executed.

Status: **ok** (works as the guide says, by MCM's own files and scripts on NVForever's
tree and functions), **fixed** (wrong before #2298, right now), **partly**, **open**.
"MCM's" means the behaviour comes from MCM's files or scripts, which NVForever runs.

### 2.1 Paths and functions

| Guide item | NVForever | Status |
|---|---|---|
| GetUIFloat, SetUIFloat, SetUIString, SetUIStringEx on `StartMenu/MCM/…` | `fnv_game::ui_functions` | ok |
| `*:N` is the N-th child from 1 (Introduction, The Options Panel, Using Lists, The Mouse-over Panel, Sub-Menus) | the extender's four count from the first child (`fnv_ui::Count::FromFirst`: 1 the first, 0 the last); before, every UI function counted from the last child from 0, as JIP's `…Alt` functions were measured to (hud.md §14.8), so `*:1/*:4/_value` reached a tile near the end of MCM's tree and a mod's page stayed empty or kept old values | fixed |
| `*:1/*:N/value/*:1/string` is the option's value text (types 1, 2.5, 3, 7, 8) | the `value` rect's first child is `text` [observed: OptionRect.xml], reached since the fix above (before: its last, `toggle_check`) | fixed |
| Names without case; `_` prefixes kept | `fnv_ui::Ui::find_trait` | ok |
| SetUIStringEx's format codes (`%g`, `%.1f`, `%.2f`, `%q`) | the VM's formatter | ok |
| `%k`: a key's display for type 3 options | the key's name from GetKeyName's observed table (`fnv_script::keys`; code 18 gives "E") [hypothesis: `%k` gives GetKeyName's text] | fixed (c3429025e on #2276) |
| GetModINISetting / SetModINISetting `ini/section/key` under `Data\Config` (Variable Storage) | `fnv_game::mod_files`: read and written in NVForever's own mod-files store (never the game folder) | ok |
| The short names GetModINI / SetModINI | aliases in the function table (`schemas/script/functions.json`) | ok |
| MCM's own log of those calls (`mcm.log`) | not written | open (not needed by any script) |

### 2.2 Registering and the list (Getting Started, Multiple Menus, Sub-Menus)

| Guide item | NVForever | Status |
|---|---|---|
| A start-enabled quest's `MenuMode 1013` block runs while MCM is open | the pause menu's Mods page runs the start menu's MenuMode blocks every frame (`nvf_play::tiles::mods`, menu-xml.md §10.1) | ok |
| Low script processing delay | MenuMode blocks run every frame there | ok |
| `ListAddForm` of the mod's item to `BuildRef iMaster 2790` at `GetGameRestarted` | ok; the start menu's frames before the game run (`Game::came_through_start_menu`) | ok |
| The item's name is the mod's line; the order is not by the name | MCM's list script sorts by plugin then by the item's value, `SetMCMModList` writes the row (menu-xml.md §10.2) | ok |
| `_ActiveMod` is the chosen mod's `GetModIndex` | `SetMCMModList` writes the row's `_modIndex` from the item's source mod; MCM copies it on a click | ok |
| Clicking a mod's line chooses it | MCM ignores the pointer while its `_AltInput` is on; with Vanilla UI Plus and MCM Extender that trait is `globals()`'s `_Has360Controller`, which the file sets to 1 and the code keeps to whether a controller is connected (§3) | fixed |
| Several lines for one mod: each item's value higher than the last; `_ActiveMenu` says which | the row's `_modIndex2` is the item's value, MCM copies it into `_ActiveMenu` | ok |
| Up to 10 sub-menus: `*:5/SubMenu#/_enable`, `SubMenu#/text/string`, `*:8/SubTitle#/string`, `_ActiveSubMenu` (0 the main page) | by name (the guide warns against `*:N` there); placed by MCM's files and list script | ok |
| A mod whose sub-menus appear only while it is chosen | JIP Companions Command & Control compares `_ActiveMod` with a global JIP LN writes; unwritten, its three sub-menu lines stood over the mod names with no mod chosen (§4) | fixed |

### 2.3 The options panel (The Options Panel, option types)

| Guide item | NVForever | Status |
|---|---|---|
| `_columns` 1: one list over the panel's width; 2: odd options left, even right | MCM's OptionRect `_x1`/`_x2` and `y` by `_optionID / _columns`; `src="me"` read as the tile itself (#2270, menu-xml.md §10.3) | ok |
| Default columns | Options.xml `_columns` 1 | ok |
| `_enable` 0 hidden, 1 shown, 2 shown but greyed and not chosen | MCM's `visible`, `target` (only `_enable` 1) and `_alpha` | ok |
| `_title`, `_prefix` (before the value), `_suffix` 1 with `_suffixText` (after it) | MCM's `title`, `value/prefix`, `value/suffix` texts | ok |
| `_type` 0 title only (no value, not chosen: MCM's `_target` needs a type) | MCM's | ok |
| `_type` 1 list: value text set by the mod; chosen, MCM shows the list window | MCM's main script starts its list quest | ok |
| `_type` 2 whole number: MCM writes the value text from `_value` (`%g`) | MCM's main script, SetMCMString | ok |
| `_type` 2.5 decimal: value text set by the mod with SetUIStringEx | reached by `*:N` since the fix | fixed |
| `_type` 3 key: chosen, "press the new control"; the key's scan code becomes `_Value` | MCM's script reads `GetKeyPress 0`, `GetMouseButtonPress 0`, `IsKeyPressed 264/265` (`fnv_game::script_input`); the name shown by `%k` | ok |
| `_type` 4 OFF/ON, 5 check box, 6 `_textOff`/`_textOn`: a click flips the value (`_Value` = 1 − `_value`, `_NewValue` 1) | MCM's main script; the click reaches it since §3 | fixed |
| `_type` 5's box picture | MCM writes `value/toggle_check/filename` `MCM\Check0.dds` / `Check1.dds` | ok |
| `_type` 7 fixed text | value text by the mod | ok |
| `_type` 8 two or three numbers; `_type` 9 a colour box from `_value1`-`_value3` | MCM's scale script; the mod reads `_Value1`-`_Value3` | ok |
| `_altFont` 1-8, `_brightness`, `_highlight` 0/1, `_indent` (×4 × `_padding`), `_RGB` / `_RGBAlt` nine-digit colours (defaults 255,182,066 and 255,255,255) | MCM's OptionRect expressions (Vanilla UI Plus replaces the colours by the HUD colour and brightness) | ok |
| `_dUI` for DarNified UI | MCM's `_dUI` from `StartMenu/_isPauseMenu` | ok |
| Scrolling the options (more lines than fit) | MCM's arrows and `_offset`, its navigate script; the wheel through `IsKeyPressed 264/265` | partly (not run here) |

### 2.4 The list, slider and info windows

| Guide item | NVForever | Status |
|---|---|---|
| List: `*:3/_title`, `*:3/*:N/_enable`, `*:3/*:N/text/string`, `_Value` 1-10 the item shown chosen | by `*:N` since the fix; MCM's list script sizes and shows it | fixed |
| A list item clicked becomes `_Value`, `_NewValue` 1 | MCM's list script (`_active` of MCM_List) | ok (by the files) |
| Slider: `_Value`, `_ValueDecimal`, `_ValueIncrement`, `_ValueMax`, `_ValueMin`, `*:2/_suffixText`, `*:2/_title` | named traits of MCM's rect; MCM's scale script | ok |
| Slider buttons Accept / Cancel / Default; a cancelled change keeps the old value | MCM's scale script (`iNewValue` only on Accept) | ok (by the files) |
| Multiple sliders: `_Value1`-`_Value3` and their `Min`/`Max`/`Increment`/`Decimal`, `*:2/_scales` 2 or 3, `*:2/_setting1`-`3` (not for type 9, which is R, G, B 0-255) | MCM's scale script | ok (by the files) |
| Info line: `MCM_Options/_optionID` is the option under the pointer (0 for none or `_enable` 0/2); `*:9/string`, `MCM_Info/visible` | MCM_Options' sum of its rows' `_optionIDX`; `*:9` by place since the fix | fixed |
| Defaults button resets all the mod's options | MCM sets `_Default` 1; the mod's block answers | ok (by the files) |
| Quest-stage result scripts for big menus (`SetStage` with repeated stages) | SetStage runs result scripts | ok |

### 2.5 Drawing

| Guide / MCM item | NVForever | Status |
|---|---|---|
| The mod list's lines one under another (`y` from ArrowUp, `MCM_ModList_Size` height, `_spacing`), a chosen mod's sub-menus under it | MCM's files and list script; laid out by the `locus` rule | ok (CCC's lines: §4) |
| Vanilla UI Plus's traits named with `+` (`_VUI+brightness`, `_VUI+OverwriteVerify`, …) | the reader cut names at the `+`, so they all became one `_VUI`: option titles lost their brightness and VUI+'s self-check showed its "installation error" text over the panel; `+` is part of a name now | fixed |
| A menu's `sibling(name)` (VUI+'s start menu reads `sibling(HUDMainMenu)`) | another menu of that name | fixed |
| Each option's title on its row | Vanilla UI Plus's OptionRect places the title by `<copy src="parent" trait="_y"/>`, a source without brackets; read as a tile named "parent" it gave 0, and every title of a page stood at the top of the screen, one over the other. A bracket-less `parent`, `grandparent`, `child`, `sibling`, `globals`, `screen`, `io` or `strings` is now that source when no tile bears the name (as `me` already was, #2270) [observed: Vanilla UI Plus writes them so 1,739 times for `parent` alone] | fixed |
| Texts drawn with the load order's fonts | a font whose picture is not read is drawn with a stand-in that lost the spaces between words ("ExtraHUD", "Selectamodto…"); and a text's leading spaces (a suffix " seconds" drawn as "6seconds" [observed: Immersive HUD's page, Blue Moon run 2026-11-13]); both are kept now (`fnv_font::Line::text_spaced`) | fixed (the stand-in) |
| MCM Extender's JSON menus in MCM's list | its handler for the start menu's opening (`SetOnMenuOpenEventHandler … 1013`, OnPauseMenuOpen.gek) adds a temporary clone of Caps001 per menu to MCMMods, from names its Script Runner file kept in private auxiliary variables; MCM marks each listed mod by adding it to the container MCMModsListed. Three faults kept them out: the Mods page sent no opening of 1013, the Script Runner file and CompileScript's functions each had their own private variables, and a container refused a clone [observed: Begin Again list 2026-11-13, 3 of 27 mods listed; MCM's script source; MCM Extender's files] | fixed: the page sends 1013's opening and closing (`Game::page_menu_event`), runtime scripts share their private names, a clone of an item goes into containers (`World::add_clone_item`) |
| A mod's place in MCM's list after a load | a mod adds itself to MCMMods from its own MenuMode 1013 block (Just Assorted Mods: half a second of GetSecondsPassed, then ListAddForm); ListAddForm's changes are not saved [documented: ListAddForm], but the original keeps them in memory through a load and loads from its main menu, the start menu, whose MenuMode 1013 frames run before the save is read with the mod's timer at its start. NVForever ran its start-menu frames after the restore and dropped list edits in it: JAM's saved timer (fTimerStart 1) skipped registering, the list was empty, and MCM's install check, made once on its first MenuMode 1013 frame after a load, stopped at 'No MCM Mods' [observed: JAM's and MCM's script source, the owner's run 2026-11-17] | fixed (#2944): a run that begins in a save runs the start menu's frames before the save is read (`nvf_play::script::start_menu_frames`), list edits last through a load (`Game::take_session`), and the Mods page's click waits for MCMMods to have members, at most 180 frames (`tiles::mods::click_now`); smoke pause-mods-save-jam / pause-mods-load-jam |
| MCM added to a game already under way | MCM's quest is start-game-enabled; a save from before MCM was in the load order has no entry for it. Game::restore took the save's quest table whole, so the quest never started: its GameMode and MenuMode blocks never ran and the Mods page stayed empty [observed: the owner's saves of 2026-11-11 to 13, GetQuestRunning MCM 0 after the load; hypothesis: the original starts a newly added plugin's start-game-enabled quests on a load] | fixed (#2944): a load starts each start-game-enabled quest the save has no entry for (`Game::restore`); test a_load_starts_the_start_game_quests_of_plugins_added_since_the_save, smoke pause-mods-added-save / pause-mods-added-load |
| More than 23 mods in MCM's list | MCM's loop calls SetMCMModList for every mod in MCMMods, past Mod23 too; a row past 23 wrote mod names into SubMenu1 and on, drawn over the top of the list [observed: Begin Again's 27 mods, 2026-11-13; MCM's script source] | fixed: SetMCMModList changes rows 1 to 23 only [hypothesis] |
| MCM Extender's names that are translation keys ("$modDisplayName") | its Script Runner file translates a name whose first character is `$` from `..\MCM\Translations\<mod>.ini` (GetINIString_Cached, a path from Data\Config); the `$` test runs, but the file's path was refused for its `..` part [observed: Begin Again list 2026-11-13, the real gr_MCMExtender.txt run in a test] | fixed on PR #2336 (an INI path's `.` and `..` worked out inside Data) |
| Reopening the Pause menu's Mods page after closing it via Esc or Back (issue #3916) | leaving `Page::Mods` sent `OnMenuClose 1013` while `StartMenu` was still loaded with stale `_MCMExt+Init` and `MCM/_Menu` traits, and `tiles::mods::tick` did not re-assert `MCM_Trigger/_clicked` across frames when XML `_UnClick` expressions cleared it [design] | partly (#3916): `tiles::mods::close` unloads `StartMenu` before `page_menu_event(1013, false)` and resets `ModsPage`, and `tiles::mods::tick` holds `_clicked` until `_Menu` reads 1; `ResetQuest` behaviour for Start Game Enabled quests stays separate pending a base-game observation |
| The load order's fonts whose picture is a `.tex` with no `.dds` (Blue Moon's) | drawn with the stand-in (fonts.md §2.2 reads a `.tex` only beside a `.dds`) | open |

## 3. `_Has360Controller` and MCM's controller mode

MCM's main script reads its rows' and options' clicks only while `iAltInput` (MCM's
`_AltInput`) is 0; with it on, it waits for the keyboard and controller instead
[observed: MCMMainScript's source, `iAltInput == 0 && … "_clicked"`]. MCM 1.5's own file
gives `_AltInput` `&xbox;` (0 on the PC); Vanilla UI Plus's edit of that file copies it from
`globals()`'s `_Has360Controller`, and MCM Extender's patch copies it
from `io()`'s `_controller`, which its StartMenu patch copies from the same global
[observed: the files]. Vanilla UI Plus's `globals.xml` gives `_Has360Controller` 1 with
the note that, unlike `&xbox;`, the game updates it while playing, and its start menu
uses it to follow a controller plugged in or out [observed: the file's comments].
NVForever never wrote it, so the file's 1 stood: MCM was in controller mode and every
click on a mod's line or an option did nothing [observed: run base2, `_AltInput 1`, a
click on "Just Mods" left `_selected` -1]. The engine now writes it each frame on
`globals()` where a file gave it, 1 while a controller is connected and 0 otherwise
(`nvf_play::tiles::write_has_controller`) [hypothesis: connected, not last used].

## 4. JIP Companions Command & Control's sub-menu lines

`crate::jip_ccc` (fnv-game): CCC's MCM script compares `_ActiveMod` with the global
`iJIPCCCModIndex`, which nothing in the plugin writes; at the file's 0 it equals MCM's "no
mod chosen", so CCC switched on `SubMenu1`-`SubMenu3` while no mod was chosen and they
were laid over the first mod lines (MCM places sub-menu lines under the chosen mod, at
`_selected` 0 under none). The global is now written with CCC's own load index when a
game is made [hypothesis: what JIP LN does; the comparison works with no other value].
`iJIPCCCStartup` is left alone (at its ready value CCC calls JIP's own `CCC…` functions,
which NVForever lacks, issue #2271).

## 5. Runs

On the owner's Blue Moon list (MCM 1.5, MCM Extender, Vanilla UI Plus; 211 plugins;
`--data D:\Modlists\ABM\nvf-merged\Data`), Goodsprings, the pause menu's Mods page opened
at 6 s, the pointer aid (`NVF_PLAY_TEST_POINTER`, interface units) on MCM's lines.

- Before (base2): MCM's window opened with 11 mods; the click on "Just Mods" reached the
  row (`clicked 1`) but MCM did nothing (`_AltInput 1`, `_selected` -1, `_ActiveMod` 0);
  the texts ran together.
- After: the click chooses Just Mods (`_ActiveMod 108`, `_AltInput 0`); its 11 options
  fill in on their rows with their check boxes and its 9 sub-menus stand under it; a click
  on Enable Dynamic Crosshair gives MCM's `_Value 0`, `_NewValue 1`, the mod takes it and
  the row reads `_value 0` with its box cleared. The setting is kept in the mod's INI
  (SetModINI): the next run started from 0 and the click set 1. Smoke scenario
  `pause-mods-page-set-value` repeats this on the owner's plain MCM install.
