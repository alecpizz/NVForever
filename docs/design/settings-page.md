# The settings page: tabs, categories and rows (#1953, the menu overhaul)

The pause menu's settings page (`engine/nvf-play/src/pause.rs`) shows every registered
setting (`engine/nvf-play/src/settings.rs`) and the controls of
`engine/nvf-play/src/bindings.rs`. Since #1953 it shows them in eight tabs (seven until NVF Features, 2026-11-15) with headings,
from a table that only says where each row is drawn. This note is the page's model; the
code's module notes say the same at their own level.

On a normal Android launcher start, borderless and one MSAA sample are platform
defaults, not command-line overrides of settings rows. The defaults carry across
in-app restarts. Explicit row switches still prevent exporting `settings-menu.json`,
as does a reduced registry when a full export from the same build already exists.

## 0. Adding or changing a row: the rules (the menu overhaul, 2026-11-15)

The owner reorganised the page on 2026-11-15, tab by tab (the menu overhaul). Every row
added or changed later keeps to these rules; the checks named fail CI or log a warning when
one is broken. Read §1–§4 for how the page works.

1. **Place the row** in `crates/nvf-settings-layout/src/lib.rs` `SECTIONS` (the table the
   pause menu and the launcher share, re-exported as `nvf_play::settings_layout`; the
   launcher's Settings page follows it and the game's `settings-menu.json`,
   docs/design/launcher.md), and list it in `docs/settings-reference.md` under the same tab
   and category (a test fails while it is missing or elsewhere): its tab, its
   category (heading) and its position. A row the table does not name falls to its group's
   tab with no heading (and a Tweaks row to the group Other); the layout tests list every
   row there is (`KNOWN`), so a new row fails them until it is placed.
2. **Tabs**: Gameplay, Display, Audio, Controls, Graphics, NVF Features, Tweaks, Debug.
   Display holds the screen, the frame rate, upscaling, anti-aliasing, frame generation and
   latency; Graphics holds what the picture shows (shadows, lighting and effects, water,
   detail and distance). No Graphics row or preset touches a Display row.
3. **NVF Features** holds only what the owner named (`NVF_FEATURES_KEYS`: Advanced Movement,
   Grass Fires). A bigger gameplay change is *offered* to the owner for that tab; never put a
   row there without the owner's word (`settings_layout::check` refuses another key).
4. **Within a category**: the main switch first; a row that only matters with another right
   after it; otherwise lists, then switches, then sliders. No row is indented: every label is
   on the left edge.
5. **7 or fewer values are a list**, never a slider (`settings::LIST_MOST`,
   `few_step_slider`): the renderer's rows' test and the start's log
   (`settings: N sliders checked, 0 with 7 or fewer values`) say so. A slider is for a
   quantity of many steps (volumes, distances, field of view). When a slider becomes a list,
   its `load` still takes the older number (`settings::choice_of`, `nearest`).
6. **A row that cannot apply** is greyed with its value kept and the reason in its note:
   name it in `settings_layout::NEEDS` (the row, the row it needs on, why) or `NEEDS_CHOICE`
   (the row, a list row and the choices it needs, why). Hide it instead, by
   `settings_layout::SHOWN_WITH` (the row, the list row that stands for it, the choices it
   shows with), only where the owner asked. These three tables are in the crate the
   launcher shares (`crates/nvf-settings-layout`), so its Settings page hides and greys
   the same rows: a rule that other rows' values decide goes in them, not in code.
   `set_disabled_status` and `settings::set_shown_when` are for what the INI cannot say
   (the GPU, missing files, a run's command line); the launcher cannot follow those. The
   rows hidden so far, where the owner asked: the upscaler's own rows (Upscaling Mode, DLSS
   Model, FSR Library, Anti-Aliasing), Generated Frames, the colour channels unless Custom,
   the controller rows, and Advanced Movement's categories while its Global Toggle is off.
7. **Words**: write the label in sentence case; the page shows it in Title Case in English,
   and so in another language until its file has a line for it (`pause::on_page`: the short words a, an, the, and, but, or, nor, for, so, yet, as,
   at, by, in, of, off, on, per, to, via stay lower case inside a label). A word written with
   a capital stays as written ("Close In on the Speaker", "HUD", "Pip-Boy"). States are
   On/Off. Under a category, leave out what the heading already says ("Mirrored reflections"
   under Water). The note: what the row does in one or two sentences, at most 200
   characters (§10).
8. **Categories** are open by default and stay as the player leaves them (`[UI]
   SettingsCategories`); the Tweaks tab's are closed by default and it shows one group at a
   time by its Category row.

## 1. The registry stays as it is

A row is registered once, by the module that owns its state (`add_setting`,
`add_choice_list`), with a key, a group, a label, a note and its default (the state before
the INI is read) [documented: `settings.rs` `Settings::add`, `apply_stored`]. The
registry's order (groups in the order they first appear, each row after its group's last)
is what the values, the INI, Defaults' snapshot (`StartStates`, `StartChoices`), the
pending resolution question and the test aids go by, all by a row's index. #1953 changes
none of that: no key, label, note, default or group changes, and nothing is reordered.

What changed is presentation only: `engine/nvf-play/src/settings_layout.rs` (where a row
is shown) and `engine/nvf-play/src/settings_view.rs` (what the page lists, as data).

**A row's command-line switches** [design: #2377]: `Setting::switches` names the switches
whose state the row replaces, as `--a, --b` (`Setting::with_switches` for a slider row;
empty: none). Until #2377 they were written at the end of the note, which the page showed;
now the note says only what the row does (§10) and the switches are their own field, not
shown (the search finds them, §6; docs/settings-reference.md lists them). `note_switches`
reads the field with the parse the notes had (each `--name` once, a value after it
ignored, one the text is "a part of" left out), so every row names the same switches as
before: in a test run a switch given keeps its rows at the command line's state over the
INI's (`GivenSwitches`, `apply_stored`, `after_ini`), as #1382 has it.

## 2. The layout table

**Since the menu overhaul (2026-11-15)** the table is `SECTIONS` (`crates/nvf-settings-layout`
since 2026-11-16, shared with the launcher) and the
tables below are its earlier state, kept for #1953's history. Eight tabs: **Gameplay,
Display, Audio, Controls, Graphics, NVF Features, Tweaks, Debug**, each a run of categories
in the owner's order (`dev` plan of 2026-11-15; `docs/settings-reference.md` lists every row
in it). What a category is on the page:

- its heading takes a line and the keys can choose it ([`settings_view::RowId::Heading`]):
  Enter, A or a click open or close it, Left closes, Right opens; Page Up/Down go from
  category to category (its first row while open, else its heading);
- open, its rows follow and an empty line after them (`ViewEntry::Gap`); closed, only its
  heading with its row count, no empty line;
- the Controls tab's own rows are named in the table by `controls:` and their name, beside
  the registered rows of their category (Input Device and Button Style under Input);
- the Tweaks tab's categories carry their group (`Placement::subgroup`); its first row,
  Category, picks the group shown (`docs/design/settings-tweaks-layout.md`).

The search and the Changed list keep #1953's crumbs over their rows (`ViewEntry::Crumb`),
every category open, nothing to choose but the rows.

**The value cell** [design: the overhaul]: every row's value is drawn in one cell, the list's
box; a list's words, a switch's On/Off, a binding and a greyed value share one centre; a
slider's meter lies between its two arrows, its number right-aligned after them at the
cell's right end. The note sits an empty line under the window (no rule); one soft dark
column runs behind the list, its titles and the note from the view's top to its bottom;
"Report a Bug" and the version sit, in the small print, under the pause column's last entry
after an empty line; the window holds 11 lines, Previous/Next Category under the note at its
left. A category's title shows its row count at the right, without brackets, while it is hovered
or chosen. Back says "Back" alone with the keyboard. The game's own font draws the menus by
default; in the sharp letters (`menu-font` Sharp) the column, the categories and the tab rail
are bold, every
line has a soft dark glow; the arrows and the scroll bar are drawn shapes
(`engine/nvf-play/src/menu_shapes.rs`), and they, the list boxes and the hover and selection
boxes have the glow too; the selection box is drawn under a row and takes no room, so nothing
moves when a row is chosen [design: owner 2026-11-15, second and third tests]. The quit
question is two columns of three lines, centred in the view: "Quit", what is lost and the
question at the left, the answers (bold) at the right. The load page's band reaches as far past
the saves' right side as past their left, with the same dark column; the sliders' meters and
the scroll bar have the glow too (fourth build).

The earlier tables (#1953):

Seven tabs, the original's four categories first: **Gameplay, Display, Audio, Controls,
Graphics, Tweaks, Debug** [design: #1953; the original's Settings opens Gameplay, Display,
Audio and Controls, observed in the original's pause menu, 2026-11-09].

| Tab | Heading | Rows (keys), in order |
|---|---|---|
| Gameplay | Game | `difficulty`, `hardcore`, `kill-camera`, `vats-style`, `vats-slowdown`, `reload-slows`, `UsePipBoy`, `npc-weapon-mods` (#3876), `sneak-bounding-box` (#3845) |
| Gameplay | Saving | `autosave-travel`, `autosave-fast-travel`, `autosave-wait`, `autosave-sleep`, `autosave-scripts` |
| Gameplay | Combat | `localized-dt-dr` (#3843) |
| Gameplay | Menus | `quantity-select-short-keys` (#3799) |
| Gameplay | Dialogue | `menu-pause`, `dialogue-goodbye-hint`, `dialogue-look`, `dialogue-recentre`, `dialogue-zoom`, `lip-sync` |
| Gameplay | Camera | `detached-camera`, `true-iron-sights`, `first-person-body`, `weapon-lower`, `field-of-view`, `viewmodel-field-of-view` |
| Gameplay | World | `grass-fire` (#2662), `sound-cover` (#2688) |
| Display | Screen | `resolution`, `borderless`, `monitor`, `display-sync`, `frame-cap`, `physics-rate`, `ui-scale`, `menu-scale` |
| Display | Interface | `hud-colour`, `hud-red`, `hud-green`, `hud-blue`, `pipboy-colour`, `pipboy-red`, `pipboy-green`, `pipboy-blue`, `language`, `dynamic-crosshair`, `crosshair-style`, `sneak-gauges`, `location-load-screens` (#3844) |
| Display | Performance | `fps-counter`, `fps-counter-fps`, `fps-counter-low`, `fps-counter-graph`, `fps-counter-split`, `fps-counter-cpu`, `fps-counter-gpu`, `fps-counter-vram`, `fps-counter-ram`, `fps-counter-loaded`, `fps-counter-upscaling` |
| Audio | (none) | `volume-master`, `volume-music`, `volume-footstep`, `volume-voice`, `volume-effects`, `volume-radio`, `music`, `voice`, `reverb`, `voice-reverb` |
| Controls | (none) | the Controls tab's own rows (§4), then `button-style`, `aim-assist`, `aim-slowdown`, `aim-lock` |
| NVF Features | Advanced Movement | `advanced-movement` (the Global Toggle), `movement-weight` (a dropdown, 25-100 %) |
| NVF Features | Gameplay (after an empty line) | `movement-inertia`, `weapon-handling`, `advanced-recoil`, `sprint` (#3000; not greyed under the Global Toggle, as its neighbours; its binding at the end of Controls › Movement, unbound by default and greyed while off; Run keeps Left Shift, the maintainer's decision on #3002) |
| NVF Features | Visual Only (after an empty line) | `weapon-inertia`, `animation-smoothing`, `camera-motion` (player-body.md §1.4a.3) |
| Graphics | Quality | `quality` (#1966; only when the install has its preset files: docs/spec-notes/quality-presets.md) |
| Graphics | Picture | `enhanced` (#1966; removed by the overhaul), `grade`, `sun-shadows`, `upscaler`, `upscaler-quality`, `dlss-model`, `fsr-library`, `frame-generation`, `frame-generation-frames`, `anti-aliasing`, `latency-reduction`, `shadow-filter`, `ambient-occlusion`, `fine-shadows`, `cloud-shadows` (#2670), `terrain-shadows` (#2671), `light-shadows`, `bloom`, `glow`, `eye-adaptation`, `neon-glow`, `sharpen`, `anisotropic`, `terrain-height-blend` (#2550), `terrain-anti-tiling` (#2551), `terrain-parallax` (#3322; under Detail and Distance after `terrain-anti-tiling`, greyed while `authored-materials` is off), `light-shafts`, `heat-haze`, `desert-haze` (#2675; under Lighting and Effects since the overhaul), `aerial-perspective` (#2993, after `desert-haze`), `night-sky-detail` (#2995, after `aerial-perspective`), `water-reflections`, `water-mirror`, `smooth-terrain` (#2149) |
| Graphics | Distance | `distant-lod`, `full-detail` (#2701; a list Off, 7 x 7, 9 x 9, after `distant-lod` in Detail and Distance; 7 x 7 in `enhanced`), `distant-objects` (#3014; a list, after `full-detail`), `grass`, `authored-materials` (#2981), `authored-reflections` (#2981 reflections; right after `authored-materials`, greyed while it is off), `object-distance`, `actor-distance`, `item-distance`, `shadow-distance`, `grass-distance`, `tree-distance` (#3016), `distant-lights` (#3013; the last row of Detail and Distance) |
| Graphics | World | `head-tracking`, `foot-ik`, `sky-clock` |
| Tweaks | (none) | none named yet |
| Debug | (none) | `overlay`, `hit-markers`, `god-mode`, `navmesh`, `terrain-wireframe` (#2149), `scope-3d` |

The Smooth terrain row is a list, Off, 2 x 2, 4 x 4 (the default) and 4 x 4 near, and
applies at the next start. Its recipe is detail, bound 8, thresholds 2:6: smooth height
steps while keeping bumps and ridges, with one surface for drawing and collision
[design, #2149: the
contributor's pick from in-game comparisons (2026-11-15), for the maintainer to confirm;
world.md §3 "Smoothing"]. 4 x 4 near draws 4 × 4 near the camera and 2 × 2 further on the 4 × 4
surface [design, #2676]; the INI's older on/off reads as 4 x 4 and Off.

The table names every row the registry can hold [documented: `settings_layout.rs::SECTIONS`],
matching `KNOWN` in `engine/nvf-play/src/settings_layout.rs` and the per-row entries in
`docs/settings-reference.md` (checked without building the engine by
`tools/smoke/tests/test_settings_count.py`, and against `SECTIONS` by
`settings_layout.rs::the_table_names_each_key_once_in_one_of_the_tabs`). Beyond the #1953
table above, added rows include #1966's `quality`, the overhaul's `menu-font` (#1966's `enhanced` is gone),
#2014's nine `fps-counter-*` rows, #2242's `fps-counter-upscaling`, #2317's
`frame-generation-frames`, #2550's `terrain-height-blend`, #2662's `grass-fire`, #2551's `terrain-anti-tiling`, #3322's `terrain-parallax`, #2688's `sound-cover`, the casino screens' `caravan-tips` (Display › Interface, after `sneak-gauges`), #2670's `cloud-shadows`, #2671's `terrain-shadows`, `light-shadows` (ALC shadows, once Lamp shadows, Graphics › Shadows after `terrain-shadows`; light_shadows.rs), #2910's `gore-shake`, #2149's `smooth-terrain` and `terrain-wireframe`, the Advanced Movement rows, #2843's `shadow-resolution` (under Graphics › Shadows after `shadow-filter`), #2661's `mesh-terrain-blend` (Graphics › Detail and Distance, after the terrain rows), the six `quick-loot` rows (#2933, NVF Features › Quick Loot), the heat haze prototype's `heat-haze` (Graphics › Lighting and Effects, after `light-shafts`), #2675's `desert-haze` (after `heat-haze`), #3000's `sprint` (NVF Features › Gameplay), #2993's `aerial-perspective` (after `desert-haze`), #2995's `night-sky-detail` (after `aerial-perspective`), #3032's `volumetric-clouds`, `volumetric-light` and `volumetric-light-strength` (Graphics › Lighting and Effects), #3016's `tree-distance`, #2981's
`authored-materials` and `authored-reflections` (Graphics › Detail and Distance, the second
right after the first), #2701's `full-detail` and #3014's `distant-objects` (Graphics › Detail
and Distance, after `distant-lod`), #3013's `distant-lights` (Detail and Distance, the last
row), #3569's `difficulty-player-damage`, `difficulty-npc-damage` and `difficulty-kill-xp`
(Gameplay › Game, under `difficulty`, greyed unless Custom), #3348's `hdr-output` (Display › Screen,
after `resolution`: a list, so before the switch `borderless`), #3783's `occlusion-culling`
(Detail and Distance, after `mesh-terrain-blend`; read at the next start), #3843's
`localized-dt-dr` (Gameplay › Combat), `physical-pbr` (Lighting and Effects, after
`night-sky-detail`; read at the next start), and #4690's `tonemap-preset` (after
`physical-pbr`, a list of the presets found at the start). The
original puts the HUD and Pip-Boy colours and the crosshair in its Display category
[observed: #1953], hence Interface there.

**How the table is applied** (`resolve_layout`) [design: #1953]:

1. Each section takes the rows of its keys that are registered; a missing optional row
   (lip sync without lip data, the draw distances without `DrawDistances`, …) is left
   out, a section left with none is dropped.
2. All eight tabs are there, an empty one too (Tweaks today).
3. **A row the table does not name** keeps the group it registered with as its tab, with
   no heading, after the tab's named rows, in registry order. A group that is none of the
   eight becomes a tab of its own after Debug, in the order such groups first appear in
   the registry. No group is renamed: an unnamed `Sound` row shows under a `Sound` tab,
   not under Audio.
4. Every registered row is listed exactly once.

So #1892's tweaks, registered under `Tweaks`, show under Tweaks in registry order without
#1892 naming them here. The `layout` aid's `<n> not in the table` counts the rows the table
leaves out everywhere but the Tweaks tab (`ResolvedLayout::unnamed`), so it stays 0 with
the Tweaks tab filled (88fbce1c4) and still catches a row another tab's table forgot.

**The Tweaks list** (#1953 point 4, a later change): `Placement` has a `subgroup`, `None`
throughout today. The later change gives Tweaks rows a placement `Tweaks › <group> ›
<heading>` in `SECTIONS` and draws the groups in place of the rail; neither the
registrations nor `resolve_layout`'s interface change for it.

### Android mobile organization

[design] Android uses the same dynamic settings page, adding **Mobile** after Controls.
Desktop keeps its eight tabs and existing placements. This changes presentation only:
the settings retain their registration order, keys, callbacks, saved values and defaults.
Missing optional rows and empty headings stay omitted. Search, Changed,
keyboard/controller navigation and row identity use the existing page implementation.

| Mobile heading | Existing controls |
| --- | --- |
| Touch layout and guide | Safe practice, icon family, adaptive Aim/Block presentation |
| Movement and gestures | Analog movement, fire drag look, run lock, optional VATS gesture |
| Gyroscope | Enable, sensitivity, ADS, hold/toggle, ADS-only, recenter |
| Inventory and quick access | Hotbar enable/count/size/tray, bottom swipe enable/target/distance |
| Dialogue | Touch dialogue, choice/subtitle text size, tap-selection policy |
| Lockpicking | Touch scheme and scale |
| Interface size | Existing menu/UI scale and canonical health/AP/compass sizes |

Preset, FPS, resolution and opt-in thermal response stay in **Display > Mobile
performance**. The preset changes the frame limit and render resolution, not Graphics.

The four Android-specific controller rows are in **Controls > Controller**. The four
shared controller rows keep their **Controls > Input** and **Controls > Aim Assist**
placements. Hardware bindings and Input Device remain in Controls; changing devices
keeps the existing hidden-row projection and remapping behavior.
The existing transactional layout editor opens from **Pause > Settings > Mobile >
Touch layout and guide** on Android: Style / profiles
for appearance, Save to keep changes, Cancel to restore them. The practice note points
to this route; there is no second editor or new persistence schema.

The shared crate's `MOBILE_SECTIONS` takes precedence over `SECTIONS` only on
Android. The same profile determines the exported menu's layout stamp and the
launcher's acceptance of that export. The launcher shows the in-game titles,
choices and dependencies for its editable subset; game-only actions (such as
the layout editor) stay in Pause. Before a normal Android start exports the
catalogue, the launcher uses its smaller built-in settings copy.

Dependencies on stored rows live in `NEEDS` and `NEEDS_CHOICE`. For example,
turning off Touch dialogue, Gyroscope or Touch hotbar leaves its dependent
settings visible with their saved value and reason. Renderer or hardware
availability still comes from the live engine. See the
[Android settings reference](../settings-reference-android.md) for placements
and compatibility rows.

## 3. What the page lists (`settings_view.rs`)

- **A row** (`RowId`) is a registered setting by its registry index, or one of the
  Controls tab's own rows by what it is (`ControlRow`: `Device`, `Binding(control,
  device)`, `Invert`, `Sensitivity(device)`, `Rumble`), never by where it stands.
- **A line** (`ViewEntry`) is a heading or a row. A heading takes one of the window's lines
  (§4) and counts in the scroll bar, but it is never chosen: no box, no pointer, no value
  [design: #1953]. It is drawn dim in the page's font (font 7, `Shade::Dim`).
- **What is listed** (`SettingsScope`): a tab, or the search's results or the Changed
  list (§6, §7). These two list rows of every tab, each under a heading saying where it
  lives (`listing`: `Gameplay > Dialogue`, `Controls`); an empty one says "No results."
  or "No changed settings.", an empty tab "No settings here.".
- **What is shown** (`Visibility`, worked out each frame from the game's state): a
  colour's red, green and blue only while the colour is Custom (§5); the four
  controller rows of the Controls tab only with Controller as the input device. A hidden
  row keeps its tab and heading. The choice on a hidden row moves to the row that stands
  for it (`parent_of`: the colour's list; the Input Device row), on a control of the other
  device to that control's row of the device in use; else to the first row.
- **`normalize`** makes the page whole for the frame: on a tab, the tab is the chosen
  row's (a test aid or a search may choose a row of another tab), the choice is repaired
  as above, the lines listed are returned. The page runs it before and after the frame's
  keys, and again in drawing, after the frame's changes (so a slider hidden by a colour
  choice gives the choice back to its colour at once; an open dropdown or a drag on a row
  no longer listed closes).

## 4. The keys, the pointer and the window

| Action | Keyboard | Controller | Pointer |
|---|---|---|---|
| Row before/after (round the ends, headings passed) | Up/Down, W/S | D-pad Up/Down | moving onto a row |
| The category before/after (its first row while open, else its heading; round) | Page Up/Page Down | LB/RB | "Previous Category", "Next Category" over the rows |
| Tab before/after (round) | Shift+Tab or F1 / Tab | LT/RT | a click on a tab |
| Keys onto the rail and back | F6 | Y | — |
| On the rail: line before/after; take it | Up/Down; Enter | D-pad; A | a click |
| Act on the row | Enter, Space, Left/Right (A/D on sliders and lists), Delete | A, D-pad Left/Right, X | a click, the arrows, the meter |
| Back | Esc, Backspace | B | Back |

[design: #1953] The controller's keys are the pause menu's own in `pad.rs`
(`menu_buttons(PAUSE_AT)`: X Delete, LT F1, RT Tab, Y F6; LB and RB the shared Page Up and
Page Down). A tab without headings is one section: Page Up/Down go to its first row. The
rail's lines are Search, Changed, the tabs, then Defaults and Back (those two keep their
place at the bottom right); with a controller in use the rail's keys pass over Search.
Ctrl+F opens the search (§6).

**The window's lines** [design: #2377]: the original's options list shows nine items
[documented: `options_container` `_number_of_visible_items` 9], and so did this page until
#2377. Now the count comes from the layout (`pause::window_lines`): as many lines as fit
between the rows' top (`OPTIONS_TOP`) and a three-line note (`NOTE_LINES`) that ends above
the view's bottom margin (`NOTE_BOTTOM`; before the menu overhaul "Report a bug" at the bottom left), 12 today. It is worked out in one place and
carried as a value (`PauseView::lines`, `Drawn::lines`) to the drawing, the keys, the
pointer, the wheel, the scroll bar and the inspect aid, so a later layout can make it
depend on the view; the `settings_view` functions (`follow`, `window_first`,
`drawn_slots`, `clamp_into_window`) take it as an argument and work with any count of at
least one (with one line the chosen row is shown rather than its heading). A new count is
logged (`pause menu: settings window of 12 lines`). The note under the window is drawn to
three lines at most: two for a row's own note, one for what it stands for (§6) or a
choice's reason.

The window shows its lines from `PauseView::scroll`: the keys keep the chosen row in it,
and its heading too when it is the heading's first row (`follow`); a heading is never drawn
alone on the window's last line (`drawn_slots`: it comes with its first row in the next
window); the wheel and the scroll bar move the window, the choice moving to the nearest row
still drawn (`clamp_into_window`), and the window stays where they put it: the heading over
the chosen row is brought back only when the choice changes (`window_first`, after
`PauseView::followed`). The pointer takes a row, its meter or its arrows only
when the row is drawn in the current window.

**The list's width** [design: #2377; #1210 for the rail]: the list (its rows, headings,
note, search line and heading links; the scroll bar right of it, the band round both) is as
wide as the room left of the rail allows (`pause::ListBox::of`): the band starts
`LEFT_MARGIN` (60 units) from the view's left edge and ends `TABS_GAP` clear of the rail as
wide as its translated words, the list at most `OPTIONS_MAX` (1300) and centred in the room
once capped. No fixed least width: a narrow view gets a narrower list rather than one under
the rail. In interface units (960 high): a 16:9 view (1280x720, 1920x1080) is 1707 wide and,
with the English rail as measured in the smoke runs, gets a list 1101 wide (820 before
#2377); 16:10 (1536) 930; 4:3 (1280) about 674; 3440x1440 (2293) the full 1300. The
width follows the rail's measured words, so another language's longer tab names give a
narrower list. The list's place is logged when it changes
(`pause menu: settings list <w> wide from <x> in a view <w> wide, band <l>-<r>`).

**A view narrower than 4:3** [design: #3334]: a portrait display (9:16 is 540 units wide)
left the list 0 wide beside the rail. Such a view gets the whole page drawn smaller, over
the whole view rather than in the spread frame, until it is `NARROW_VIEW_W` (1280 units, the
4:3 layout) wide (`pause::narrow_fit`, `interface::whole_view`); at 1080x1920 a unit is
then about 0.84 pixels, near a 1080p landscape screen's 1.125. The rows, rail and note keep
the 4:3 places, and the window has more lines (the page is taller in its units). A view 4:3
or wider is drawn as before. Checked by smoke `settings-layout-portrait` (a 540x960 window).

**A row's parts** [design: #2377]: the places in a row (a switch's state and a reason
centred at the value's place, a slider's arrows, meter and value, a list's box, a key's
binding) keep their distance from the row's right end that they had in the 820-unit row
(`pause::RowX`), so a row of that width is drawn as before and a wider one gives its extra
room to the label. `pause::row_parts` lays every row out from its fonts' measured widths:
the label ends 16 units (`LABEL_GAP`) before the value's part; a wide reason or binding
takes room from the label, down to 160 units (`LABEL_MIN`); wide slider words ("Performance")
take room from the meter, down to 100 units (`METER_MIN`); a list's words stay in its box.
What still does not fit is cut with "..." at its measured width (`fit_text`) instead of
running into its neighbour: labels, values, reasons, a list's words, the open dropdown's
choices and heading lines. Only the drawing is cut: the search, the log and the hidden texts
the aids read keep the full words. When the page's fonts are first loaded,
`check_page_text` lays every registered row out at the 16:9 page width over all its values
and logs each row it had to cut (`settings: text of `<key>` cut to fit: ...`) and a summary
(`settings: <n> rows laid out, <cut> cut to fit`).

**One dispatcher** (`pause.rs` `act_on_row`, `RowAction::{Activate, Step, Clear}`) does
what the keys, the pointer and the aids ask of a row, a registered row or a Controls row
alike, so the search and the Changed list act on their rows the same way, drawn by the
same `row_line`.

[design, #2200; `settings_view.rs::ControlRow::rows`] The Controls tab lists Input Device,
Automatic Input Switching (default ON), the 29 controls, Invert Y, the device's sensitivity,
Vertical Sensitivity and Aim Sensitivity (the look while aiming down the sights or through a
scope, following the aim's zoom; 100% as unzoomed; owner 2026-11-15), and with a controller
Rumble, then the four controller rows.
Automatic Input Switching selects and saves Input Device on fresh use only when the old
device was idle at frame start and has no release in that frame. Release its buttons and
return sticks/triggers to neutral, then press again on a later frame. Only the switching
press edge is ignored; later holds/repeats and all releases work (controller.md section 1).
Turning it off keeps device selection manual. Its Defaults puts back the controls of the device in use (`bindings::reset`, as before) and the tab's
registered rows; Defaults on every other tab puts back every registered row, as before
(`pause menu: settings back to their defaults`). A control waiting for its new key is
`pause::Listening` (the pad presses no menu keys meanwhile).

**A list's choices** [design: #2198, step 7 of docs/design/dlss-upscaling.md]: besides a
whole row being disabled with a status in place of its value (`set_disabled_status`, the
anti-aliasing row's "Off (upscaling active)" while an upscaler draws), a list row can show
its choices by other words than their names (`set_choice_labels`) and hold some out of reach
for a reason (`set_choice_unavailable`), both kept per row key in the registry, so the other
lists draw as before. The names stay what the INI, the log and the test aids use; only the
words shown change (the Upscaler row shows its requested `DLSS` as "DLSS (restart)" while a
restart is pending for it, the long reason in the row's note). A choice out of
reach is drawn dim in the open list, and in the row's box when it is the chosen one; its
reason is the note under the rows while the keys or the pointer are on it; Enter, A or a
click leave the list open, and Left/Right on the row pass over it. `settings::pick` refuses
it centrally, also by a name in another case (`settings: <key> <name> not taken: <reason>`),
so the dropdown, Left/Right, Defaults' fallback by index and the `NVF_PLAY_TEST_SETTINGS` aid
cannot take it. Loading the INI does not go through that check: a stored choice out of reach
still loads, so a request survives a run in which it cannot be met.

**A choice that is an action** [design: #2250]: a list's `names` may hold a choice that does
something instead of being kept, as Reports' "Delete them now" does (`reports/delete.rs`).
The Upscaler row lists `Download DLSS...` (shown as "Download DLSS…", or "Downloading DLSS…
n%" while its download runs) while DLSS is out of reach only because its files are missing
(`upscaler::Unavailable::FilesMissing`, Windows with Vulkan) and no download installed them
this session. Its `pick` asks for nvforever's `dlss_download` dialog (over the page as the
hand report's form is, `pause::FormOver`) and leaves `Graphics::upscaler` as it is. The row
marks it as an action (`set_choice_action`): `settings::pick` hands it to the list's `pick` and
then neither owns the row nor writes the INI, so a run whose command line set another upscaler
does not write that over the stored choice (`KeptRows`); the log says `settings: upscaler
Download DLSS... (an action; nothing kept)`, or `... not taken` where it is not offered. The
test aids name it `upscaler=Download DLSS...`. The dialog and its test aid
`NVF_PLAY_TEST_DLSS_DOWNLOAD` are described in docs/dlss.md and
`engine/nvforever/src/dlss_download/`.

**A list that took over from an older row** [design: #2198's play-test]: the Upscaler and
Upscaling mode rows replaced the one `fsr-upscaling` row. `set_older_key` lets a list read
the older row's INI key while its own is not in the file (its `load` takes the older text;
the row is written under its own key from then on, the older line left as it is). Where the
older row's one value stands for several rows, `set_older_split` splits it into each row's
own value before the INI is read (`with_older_split`: only while the row of `set_older_key`
lacks its own key, and only for rows whose own key is missing too), so each row loads, and in
a test run keeps (`KeptRows`), a value of its own [design: from the review of 2026-11-12: the
Upscaler row alone kept the older text, and a save then wrote the command line's quality
beside it]. And
`set_list_alias` lets the test aids still name the older row (`fsr-upscaling=<choice>`):
`pick_alias` takes the older row's choice as `pick` takes a list's, refused with a log line
while out of reach, logged as `settings: <older key> <choice>`, the rows it sets the
player's and kept in the INI. The older key is no row of its own: it is not on the page, in
the search or in the Changed list.

## 5. Custom colours (`ui_colour.rs`)

The HUD's and the Pip-Boy's colour lists always offer Custom [design: #1953]. Taking it
keeps the colour as it is and shows its three sliders under it; taking a named colour sets
it and hides them. A colour is Custom while it was chosen so (`CustomColours`, also set
when a slider moves, so the sliders stay up as they pass a named colour's values) or while
its values are none of the named colours (an INI's, a test aid's). `hud-colour=Custom` in
the INI keeps the choice; the six channel keys keep the values; there is no new key.
Defaults loads the start's text (`Amber`) and so leaves Custom. A hidden slider keeps its
key: the INI and a test aid (`pipboy-red=0.2`) set it as before.

## 6. The search (#1953 point 5)

**Opening and closing** [design: #1953]: the rail's Search line (a click, or Enter on it
with the keys on the rail), or Ctrl+F anywhere on the page. A key being awaited, an open
dropdown and the keep question keep the keys until they are done, so Ctrl+F does nothing
then. Esc (a controller's B), Back or the rail's Back closes the search, clears its text
and shows what was listed before, with the same row chosen and the same first line; it
goes no further (the settings page stays; only with the search closed does Back leave
it). Taking a tab or the Changed list closes it too.

**The text** is the window's (`KeyboardInput`), read every frame in `PreUpdate`, whether
the search is open or not, so nothing typed before it opened comes in later
(`pause::collect_search_keys`). A pressed key's text goes in (repeats too) unless Ctrl,
Alt or the logo key is held, or it is a control character, or the game's fonts cannot draw
it (`fnv_util::text::encodable`, as the console); Backspace takes the last letter off.
A frame's keys are taken in the order typed: text and Backspace go into the text, Enter
moves from the text to the results, Esc closes the search and drops what came after it
(an Enter alone on the results is theirs: it acts on the chosen row). The commands that
come as keys without text (a controller's B, A and D-pad Down, the aids' `back`, `pick`,
`down`) are taken after that frame's text, never lost to it. A click on the pause column's
entries from an open search closes it as Esc does, and that frame's keys reach no other
page.
Ctrl+F's F is not typed: the search opens after that frame's text was read.

**Who has the keys**: while the search is open the letters are its own. On the pause menu
the search is looked at before every other key: what was typed goes into the text (and
the keys back into it), Esc closes it; in its text Down or Enter put the keys on the
results and every other key does nothing. W/S, A/D and Space are no keys of the page there
(Up/Down, Left/Right and Enter are). The game's controls read no key while it is open
(`bindings::KeyboardCapture`: `ControlInput::suppressed()` and `just_released()`), set in
`PreUpdate` before the game's input and still on the frame the search closes.

**Matching** (`settings_view::search`) [design: #1953]: a row's words are its label and
note in the language shown and in English, its INI key (a tweak's note names its
Stewie's Tweaks key) and its command-line switches (§1; `--msaa` finds Anti-aliasing,
#2377); the Controls tab's own rows have their label, note and INI name
(`pause::control_words`), both devices' controls, so a controller's button is found from
the keyboard. The query is split at whitespace and lowercased (Unicode); a row is found
when every word is somewhere in its words. No stemming, no ranking: the results are in the
page's order (`catalogue`: tab by tab, the Controls tab's own rows first on it). An empty
text lists every row. A row hidden where it lives is listed as the row that stands for it
(`shown_as`: a colour's channel as its colour; a controller row, a control's binding or the
sensitivity of the device not in use, and Rumble without a controller as Input Device),
once, in the page's order of the rows shown. Its note then begins with what it stands for
(`stands_for`: "Stands for: Jump (Controller)."; past three rows, "Stands for: A, B, C and
2 more.", so the note keeps to its lines, #2377). So a result of the other device is never
edited through the device in use: acting on it switches the device, and the row is then
listed as itself.

The search's line takes the place of the "Previous Category" and "Next Category" links, the
rows' whole width; Page Up/Down (LB/RB) still go from heading to heading of the results.

**On the results** the keys act as on the row's own tab (Enter, Left/Right, Delete, a
click, the dropdown, the keep question, a key's rebinding), Page Up/Down go from place to
place, and the search stays open; typing goes back to the text, a click on the search's
line too. Defaults on the search is the page's global one.

**A controller** types nothing yet: the rail's keys pass over Search, which says
"Keyboard search". A keyboard can still open it with a controller in use, and the
controller then works its results (D-pad, A, B).

## 7. The Changed list (#1953 point 6)

**What it lists** [design: #1953]: every row set away from what Defaults puts back, of
every tab, hidden and disabled rows too, in the page's order, under where each lives:

| Row | Away from its default when |
|---|---|
| A switch | its state differs from Defaults' (`StartStates`, or the states the page was first shown with) |
| A slider | its value as `Slider::fit` keeps it differs (a getter's rounding is no change) |
| A list | its state as the INI keeps it (`ChoiceList::save`) differs from the start's (`StartChoices`; taken when the page is first shown when there is none) |
| A control's key and mouse button; its controller button | it is not the game's (`Bindings::game_defaults`), each device's its own row |
| Invert Y, Rumble, Input Device, each sensitivity | as the game has it by default |
| Automatic Input Switching | OFF, away from its ON default [design, #2200; `bindings::changed_controls`] |

(`settings::changed_rows`, `bindings::changed_controls`.) A disabled row counts by its
saved state, not by the words shown in its place. A hidden row is listed as the row that
stands for it, as on the search; the log and Defaults go by the rows themselves.

**Defaults on the Changed list** puts back exactly the rows it lists, wherever they are
drawn: the registered rows by `settings::restore` (the resolution's keep question as
ever), twice over so a row another one held disabled (the anti-aliasing under FSR) goes
back once that one has, the Controls rows by `bindings::restore_rows`, each set to the
game's directly with no swap (a control's key goes back without taking another's). A row
still disabled stays as it is, and stays listed. The page's other Defaults are as before:
on the Controls tab its controls and registered rows, elsewhere every registered row,
logged `pause menu: settings back to their defaults`.

A row put back leaves the list; the choice goes to the row after it (else the one before).

## 8. Where the page opens; the rail under the pointer

**Where it opens** [design: #1953, the fork owner's request 2026-11-09]: Settings (the
pause menu's column, S, the main menu's Settings, a script's ShowPauseMenu 3) opens the
page where it was left this run: the same tab (or the Changed list), the same chosen row
and first line (`pause::open_settings`, `PauseView::last`). Left from the search, it opens
on what the search came from; the search's text is not kept. A remembered row no longer
listed gives way to the tab's first row. Continue and a save's load start NVForever again,
so the **tab** is also kept in `NVForever.ini` as `[UI] SettingsTab=<tab>` (its English
name, beside the other UI switch, `[UI] TileMenus`), written when the tab shown changes
and read at start (`pause::SettingsTabStore`); the first opening of a run is on that tab,
an unknown or missing name on the first tab. As every INI line here, an unattended run
in the player's own home reads and writes none (`settings::SettingsFile`). The Changed
list and the search are not kept across runs.

**The rail under the pointer** [design: #1953; as the screen menu's page names,
docs/design/screen-menu.md §3.1, and the original's lists' mouse-over sound,
list_box_template.xml `mouseoversound`]: a rail line (Search, Changed, a tab) under the
pointer is drawn bright with the highlight box's line round it, unfilled, and the focus
sound plays once as the pointer comes onto it (not again while it rests there). The line
shown keeps its bright look, the keys' line on the rail its filled box; Defaults and Back
are as before. Resting on a line takes nothing: a click does.

## 9. Test aids and the log

`NVF_PLAY_TEST_SETTINGS` adds `tab:<name>`, `select:<key>`, `layout`, `search:<text>`
(the search open on that text, the keys on its results) and `changed`;
`NVF_PLAY_TEST_PAUSE_MENU` adds `heading-down`, `heading-up`, `tab-down`, `tab-up`, `rail`,
`left`, `right`, `pad:<button>` (the key the button presses on the pause menu), `search`
(the rail's Search), `settings` (the column's Settings: the page where it was left) and
`inspect` (also the rail line hovered); they press the keys they stand for. Real input, the next
frame: `type:<text>` (each character's key pressed with its text, as the window's
`KeyboardInput` messages before Bevy reads them, so the keys' state has the letters too
and the search's hold on them is tested), `backspace`, `ctrl-f`; `point:<target>` (the
pointer onto the middle of what was drawn for `search`, `search-field`, `changed`,
`defaults`, `back`, `heading-up`, `heading-down`, `tab:<name>`, `row:<key>`, `choice:<n>`),
`press` and `release` (the left button, before the UI looks under the pointer, as the
Wait menu's aid). The existing aids keep their grammar and their log lines. The new lines:

```text
settings layout: tabs Gameplay, Display, Audio, Controls, Graphics, Tweaks, Debug
settings layout: menu-pause -> Gameplay > Dialogue
settings layout: 87 row(s) in 7 tab(s), 0 not in the table
pause menu: tab Display; selected resolution
pause menu: heading Gameplay > Saving; selected autosave-travel
pause menu: the keys on the rail (Display)
pause menu: rail Changed
pause menu: colour sliders HUD shown, Pip-Boy hidden
pause menu: inspect: Display, keys on the rows, selected hud-colour, lines 1-12 of 21
pause menu: inspect: window 12 lines, 12 drawn
pause menu: settings window of 12 lines
pause menu: settings list 1101 wide from 90 in a view 1707 wide, band 60-1257
settings: 103 notes checked, 0 over the budget (200 characters)
settings: 103 notes checked, 0 over the budget (200 characters, two lines)
settings: 103 rows laid out, 0 cut to fit (a list 1036 units wide in a 16:9 view)
pause menu: search open
pause menu: search "menu-pause": 1 result(s); keys menu-pause
pause menu: the keys on the results
pause menu: search closed
pause menu: list Changed; selected difficulty
pause menu: changed 3 row(s); keys difficulty,crosshair-style,god-mode
pause menu: changed defaults; keys difficulty,crosshair-style,god-mode
controls: padbind:Jump, rumble back to the game's
pause menu: the pointer on heading-down
pause menu: settings open on Debug; selected god-mode
settings page: [UI] SettingsTab=Graphics kept in <home>/NVForever.ini
settings page: [UI] SettingsTab=Graphics read from <home>/NVForever.ini
pause menu: hover Debug
pause menu: inspect: hovered Debug
```

Smoke scenarios: `settings-layout`, `settings-headings`, `settings-scroll-headings`, `settings-custom-sliders`,
`settings-controller-navigation`, `settings-search`, `settings-search-input`,
`settings-changed`, `settings-heading-pointer`, `settings-search-language`,
`settings-reopen`, `settings-tab-write` and `settings-tab-read` (a pair), `settings-rail-hover`,
`settings-note-budget`, `settings-layout-ultrawide`
(tools/smoke/scenarios.toml).

## 10. The notes: short, within a budget (#2377)

[design: #2377] A row's note says what the row does in one or two short sentences: at most
200 characters (`settings::NOTE_BUDGET`, counted as characters) and no command-line switch
(those are the row's `switches`, §1). That is about two lines at the page's width in a 16:9
view in the small face (font 3); the note's third line is left for what a search result
stands for or a choice's reason. The explanations cut from the notes in #2377, and the
switches, are in docs/settings-reference.md, one entry per key in the page's order, with
links to the specialist docs.

The budget is checked twice. Once every row is registered (`settings::check_notes`,
`PostStartup`) each note goes through `settings::note_over_budget`: one over it logs
`settings: note of `<key>` over the budget: <reason>`, then `settings: <n> notes checked,
<bad> over the budget (200 characters)`. When the pause menu's fonts are first loaded
(`pause::check_page_text`) each note is also wrapped at the page's 16:9 width: one wrapping
to more than two lines is over the budget too (`... over the budget (200 characters, two
lines)`). Unit tests cover the check itself, the Controls tab's own notes (which are not in
the registry) and the renderer's rows that register without the game's files; the smoke
scenario `settings-note-budget` reads the two summaries. The page still cuts a note to its
three lines when one is longer (a translation, a narrow view: at 4:3 the list is about 609
units wide and a two-line note at 16:9 may take three or four).


## Performance usage availability (#2249)

[design, #2249: `engine/nvforever/src/perf_overlay/{usage,readings,mod}.rs`,
`engine/nvforever/src/settings.rs`] The existing switches and defaults stay the same.
CPU and GPU rows each say **Not available** when their reading is missing, including
while an initial rate awaits its second sample. A CPU row can still show the game's
cores when the machine reading is missing. These are NVForever's own words; the new
unavailable and game-GPU wording uses `lang/*.toml`.

| Existing setting | Systems and scope [design: `perf_overlay/usage.rs`] |
|---|---|
| `fps-counter-cpu` | Windows PDH machine use and process time; Linux `/proc/stat` machine use and `/proc/self/stat` process time. Other systems show Not available. |
| `fps-counter-gpu` | Windows PDH busiest engine over all programs. Linux tries runtime NVML, readable amdgpu `gpu_busy_percent`, then DRM client nanosecond counters. Unsupported telemetry shows Not available. |

[design, #2249] `usage::linux::GPU_SOURCE_ORDER` prefers NVML then amdgpu to keep
whole-device use where available; DRM is the fallback and shows **game 71%**, for
example. NVML and amdgpu take the busiest readable device; these readers do not match
a device to the renderer on a multi-GPU machine. Selection occurs at the first GPU
request, logs once (including no available source), and keeps its reader. DRM rescans
fdinfo each requested second so clients opened later can start reporting. A lost
selected source shows unavailable; restarting retries the higher-priority sources.
Hidden overlays and disabled GPU rows reset DRM rate baselines. The first numeric
Windows usage log stays unchanged; a separate availability log waits at most ten
half-second updates with both usage rows enabled, then spells out missing parts.

[documented: Linux kernel, [DRM client usage stats](https://docs.kernel.org/gpu/drm-usage-stats.html)]
The usage and identity fields are optional: `drm-engine-<name>` gives client busy nanoseconds,
`drm-engine-capacity-<name>` gives grouped engine capacity (default one), and
`drm-client-id` with `drm-pdev` identifies duplicate client descriptors. Counters can
briefly decrease; readers must retain their high-water mark.
[design, #2249: `perf_overlay/drm_usage.rs`] We sum distinct clients per device and
engine, divide busy-time deltas by elapsed time and capacity, choose the busiest
engine and clamp to 0-100%. New clients need two samples; unreadable files, malformed
units, missing client IDs or unsupported counters cannot supply a percentage.
Without a PCI device identity, clients are kept separate when choosing the busiest
engine, so work on unknown devices is not added together [design, #2249].
This applies to drivers exporting these fields (including supporting amdgpu, i915/xe
and nouveau versions); a driver name alone does not guarantee telemetry.

[documented: NVIDIA, [NVML Device Queries](https://docs.nvidia.com/deploy/nvml-api/620/api/group__nvmlDeviceQueries.html),
[Initialization and Cleanup](https://docs.nvidia.com/deploy/nvml-api/620/api/group__nvmlInitializationAndCleanup.html),
[nvmlUtilization_t](https://docs.nvidia.com/deploy/nvml-api/620/api/structnvmlUtilization__t.html)]
NVML supplies device GPU utilization on supported NVIDIA proprietary drivers. We load
`libnvidia-ml.so.1`, initialize with `nvmlInit_v2`, enumerate with
`nvmlDeviceGetCount_v2` / `nvmlDeviceGetHandleByIndex_v2`, and read
`nvmlDeviceGetUtilizationRates`; unavailable libraries, symbols, permissions and
unsupported utilization allow fallback [design: `perf_overlay/nvml.rs`].

[design, #2249] Synthetic fixtures test parser/arithmetic on every test platform.
Smoke scenario `perf-overlay-usage-availability` accepts a numeric or explicit missing
part independently. The older `fps-counter` numeric expectation is preserved.
Linux validation still needs Intel/nouveau game-only rates and labels, NVIDIA with
and without NVML, amdgpu fallback, no telemetry, and hide/show or GPU-row off/on.
